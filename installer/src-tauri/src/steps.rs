//! Os cinco passos da instalação, iguais no Windows e no macOS — o que muda
//! entre os dois está em `platform`.
//!
//! Todos são idempotentes: cada um checa o que já existe e pula. Rodar de
//! novo depois de uma falha retoma de onde parou em vez de recomeçar — o que
//! importa quando o passo mais longo baixa mais de 1 GB.

use crate::download::{download_resumable, DownloadSpec};
use crate::models::{self, HF_BASE};
use crate::paths;
use crate::platform;
use crate::progress::{Reporter, Step};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Espaço necessário, com folga: modelos + dependências Python (o
/// onnxruntime-gpu sozinho passa de 1 GB) + o Python + as bibliotecas CUDA
/// quando há placa NVIDIA + margem. Checado ANTES de baixar qualquer coisa —
/// descobrir que falta espaço com 900 MB já baixados é a pior hora.
pub fn required_bytes() -> u64 {
    models::total_bytes() + 8 * 1024 * 1024 * 1024
}

/// Espaço livre no volume da instalação. A raiz ainda pode não existir;
/// mede na pasta existente mais próxima dela.
pub fn free_disk_bytes() -> Result<u64, String> {
    let root = paths::root()?;
    let mut probe = root.as_path();
    while !probe.exists() {
        probe = probe
            .parent()
            .ok_or_else(|| "não consegui determinar o volume de instalação".to_string())?;
    }
    platform::free_disk_bytes(probe)
}

/// Passo 1 — o Python da instalação.
pub async fn ensure_python(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let dir = paths::python_dir()?;
    let exe = platform::python_executable(&dir);
    let existed = exe.exists();

    if !existed {
        rep.running(Step::Python, "Configurando o ambiente…");
        rep.log(&format!("baixando {}", platform::PYTHON_LABEL));
        platform::install_python(client, rep, &dir).await?;
        if !exe.exists() {
            return Err(format!("o pacote do Python não trouxe {}", exe.display()));
        }
    }
    platform::repair_python(client, rep, &dir, &paths::app_dir()?).await?;

    rep.done(
        Step::Python,
        if existed { "Ambiente já configurado" } else { "Ambiente configurado" },
    );
    Ok(())
}

/// Passo 3 — pip e dependências, direto no Python da instalação.
///
/// Depende do passo do aplicativo já ter acontecido, porque o
/// requirements.txt vem de lá.
pub async fn ensure_dependencies(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let python = paths::python_exe()?;
    let requirements = paths::app_dir()?.join("requirements.txt");
    if !requirements.exists() {
        return Err(format!(
            "requirements.txt não está em {} — o passo do aplicativo falhou",
            requirements.display()
        ));
    }

    platform::ensure_build_tools(client, rep).await?;
    platform::ensure_pip(client, rep, &python).await?;

    // `insightface` (e outros pacotes sem wheel pronto para esta combinação
    // de Python/SO) precisa compilar sua extensão, e o backend de build PEP
    // 517 padrão (setuptools.build_meta) não existe se setuptools não
    // estiver instalado — falha com "BackendUnavailable: Cannot import
    // 'setuptools.build_meta'".
    rep.running(Step::Dependencies, "Preparando os componentes…");
    run_streamed(
        Command::new(&python).args(["-m", "pip", "install", "--progress-bar", "off", "setuptools", "wheel"]),
        "instalar setuptools/wheel",
        rep,
        Step::Dependencies,
    )?;

    // insightface compila extensões Cython/C e seu setup.py importa numpy e
    // Cython diretamente. O isolamento de build do pip cria um ambiente novo
    // por pacote, então listar numpy/Cython antes de insightface no
    // requirements.txt não basta — precisam existir ANTES de processar o
    // requirements.txt inteiro. A versão do numpy vem do próprio
    // requirements.txt para não divergir; Cython só é preciso para compilar.
    let numpy_spec = requirements_line(&requirements, "numpy")?.unwrap_or_else(|| "numpy".into());
    rep.running(Step::Dependencies, "Preparando os componentes…");
    run_streamed(
        &mut platform::pip_command(
            &python,
            &["-m", "pip", "install", "--progress-bar", "off", &numpy_spec, "Cython"],
        ),
        "instalar numpy/Cython",
        rep,
        Step::Dependencies,
    )?;

    // O passo longo. platform::pip_command prepara o compilador do sistema
    // (no Windows, ativa o ambiente do MSVC — instalar o compilador não
    // basta, nenhum processo herda INCLUDE/LIB/PATH sozinho).
    let requirements_str = requirements.to_string_lossy();
    rep.running(Step::Dependencies, "Instalando os componentes (pode levar vários minutos)…");
    run_streamed(
        &mut platform::pip_command(
            &python,
            &["-m", "pip", "install", "--progress-bar", "off", "-r", &requirements_str],
        ),
        "instalar as dependências",
        rep,
        Step::Dependencies,
    )?;

    // Aceleração extra desta máquina (no Windows com NVIDIA, as bibliotecas
    // CUDA). O pip pula o que já está instalado, então repetir é barato.
    let extras = platform::gpu_packages();
    if !extras.is_empty() {
        rep.running(Step::Dependencies, platform::GPU_PACKAGES_MESSAGE);
        let mut args = vec!["-m", "pip", "install", "--progress-bar", "off"];
        args.extend(extras);
        run_streamed(
            Command::new(&python).args(&args),
            "instalar as bibliotecas de aceleração",
            rep,
            Step::Dependencies,
        )?;
    }

    rep.done(Step::Dependencies, "Componentes instalados");
    Ok(())
}

/// Repositório de onde vem o código quando o instalador roda numa máquina
/// que não tem o projeto. Público de propósito: um Release privado exigiria
/// um token embutido no instalador distribuído, o que não é segredo nenhum.
const RELEASE_REPO: &str = "felvieira/deepfake-studio-live";

/// Passo 2 — código do app, baixado do Release mais recente.
///
/// O tarball do GitHub vem com um diretório raiz do tipo
/// `deepfake-studio-live-<sha>/`, removido na extração para o conteúdo cair
/// direto em app/.
pub async fn fetch_app_code(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let dest = paths::app_dir()?;
    // Versão do código instalado. Antes, bastava existir run.py para pular
    // o passo — e quem rodava um instalador novo sobre uma instalação antiga
    // continuava com o app antigo para sempre.
    let tag_file = dest.join(".release-tag");
    let installed_tag = std::fs::read_to_string(&tag_file).ok().map(|t| t.trim().to_string());

    rep.running(Step::AppCode, "Procurando a versão mais recente…");

    let api = format!("https://api.github.com/repos/{RELEASE_REPO}/releases/latest");
    let response = client
        .get(&api)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("não consegui falar com o GitHub: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "não consegui consultar as versões ({}). Verifique a conexão e tente de novo.",
            response.status()
        ));
    }
    let release: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("resposta inesperada do GitHub: {e}"))?;

    let tag = release
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("desconhecida")
        .to_string();
    let tarball = release
        .get("tarball_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "a versão publicada não tem código para baixar".to_string())?
        .to_string();

    if dest.join("run.py").exists() && installed_tag.as_deref() == Some(tag.as_str()) {
        rep.done(Step::AppCode, format!("Aplicativo {tag} já instalado"));
        return Ok(());
    }

    rep.running(Step::AppCode, format!("Baixando o aplicativo ({tag})…"));
    let archive = paths::root()?.join("app-source.tar.gz");
    download_resumable(
        client,
        &DownloadSpec { url: &tarball, target: &archive, expected_size: None, sha256: None },
        |done, total| {
            rep.progress(Step::AppCode, format!("Baixando o aplicativo ({tag})…"), done, total);
        },
    )
    .await?;

    rep.running(Step::AppCode, "Descompactando o aplicativo…");
    std::fs::create_dir_all(&dest).map_err(|e| format!("não consegui criar {}: {e}", dest.display()))?;
    untar_strip_root(&archive, &dest)?;
    let _ = std::fs::remove_file(&archive);

    if !dest.join("run.py").exists() {
        return Err("o pacote baixado não contém run.py".into());
    }
    let _ = std::fs::write(&tag_file, &tag);
    rep.done(Step::AppCode, format!("Aplicativo {tag} instalado"));
    Ok(())
}

/// Passo 2 (modo local) — copia de `source` em vez de baixar.
///
/// Usado quando o instalador roda de dentro do repositório (desenvolvimento
/// e CI). Numa máquina limpa não existe `source`, e aí `fetch_app_code`
/// assume.
pub fn ensure_app_code(rep: &Reporter, source: &Path) -> Result<(), String> {
    let dest = paths::app_dir()?;
    if !source.exists() {
        return Err(format!("código-fonte não encontrado em {}", source.display()));
    }

    rep.running(Step::AppCode, "Copiando os arquivos do aplicativo…");
    std::fs::create_dir_all(&dest).map_err(|e| format!("não consegui criar {}: {e}", dest.display()))?;

    // Não copia venv/, models/ nem lixo de desenvolvimento: o ambiente é
    // recriado para esta máquina e os modelos vêm do passo 4. Copiar um venv
    // de outra máquina traria caminhos absolutos quebrados.
    let skip = [
        "venv", ".venv", "models", ".git", ".github", "__pycache__", "installer",
        ".auto", ".bot", "ui-gallery", "test-vm", "graphify-out",
    ];
    copy_tree(source, &dest, &skip)?;

    rep.done(Step::AppCode, "Aplicativo copiado");
    Ok(())
}

/// Passo 4 — modelos.
pub async fn ensure_models(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let models_dir = paths::models_dir()?;
    let total = models::total_bytes();
    let mut completed: u64 = 0;

    let count = models::MODELS.len();
    for (index, model) in models::MODELS.iter().enumerate() {
        let relative = model.name.replace('/', std::path::MAIN_SEPARATOR_STR);
        let target = models_dir.join(&relative);
        let url = format!("{HF_BASE}{}", model.name);
        // A tela mostra "modelo 3 de 9", não o nome do arquivo: quem instala
        // não precisa saber quais modelos são. O nome vai para o log.
        let label = format!("Baixando modelo {} de {count}…", index + 1);
        rep.log(&format!("modelo {}/{count}: {}", index + 1, model.name));

        let base = completed;
        download_resumable(
            client,
            &DownloadSpec { url: &url, target: &target, expected_size: Some(model.size), sha256: None },
            |done, _| {
                rep.progress(Step::Models, label.clone(), base + done, total);
            },
        )
        .await
        .map_err(|e| format!("{}: {e}", model.name))?;

        completed += model.size;
    }

    rep.done(Step::Models, format!("{count} modelos prontos"));
    Ok(())
}

/// Passo 5 — câmera virtual. Falhar aqui nunca é fatal: o app funciona sem.
pub fn ensure_virtual_camera(rep: &Reporter) -> Result<(), String> {
    platform::ensure_virtual_camera(rep, &paths::app_dir()?)
}

// ---------------------------------------------------------------- utilitários

/// Acha a linha de `pkg` num requirements.txt e devolve o especificador
/// pronto para `pip install` (ex.: "numpy>=2.0,<3"), ignorando comentários.
/// `None` se o pacote não estiver listado.
fn requirements_line(requirements: &Path, pkg: &str) -> Result<Option<String>, String> {
    let text = std::fs::read_to_string(requirements)
        .map_err(|e| format!("não consegui ler {}: {e}", requirements.display()))?;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let name = line
            .split(|c: char| "><=!;~ ".contains(c))
            .next()
            .unwrap_or("")
            .trim();
        if name.eq_ignore_ascii_case(pkg) {
            return Ok(Some(line.to_string()));
        }
    }
    Ok(None)
}

/// Roda um comando longo mostrando, ao vivo, o que ele está fazendo.
///
/// `pip install -r requirements.txt` leva vários minutos; a saída é lida
/// linha a linha e as linhas que dizem algo útil viram uma mensagem curta
/// para a tela (a linha técnica vai para o log).
///
/// O stderr é lido numa thread à parte: se ninguém o esvaziar, um pipe cheio
/// bloqueia o processo filho e a instalação trava de verdade.
pub fn run_streamed(command: &mut Command, what: &str, rep: &Reporter, step: Step) -> Result<(), String> {
    platform::hide_console(command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|e| format!("falha ao {what}: {e}"))?;

    let stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut e) = stderr {
            let _ = e.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut stdout_text = String::new();
    if let Some(out) = child.stdout.take() {
        for raw in BufReader::new(out).split(b'\n').flatten() {
            let line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_string();
            if let Some(msg) = friendly_pip_line(&line) {
                // A linha original (com nomes de pacotes) fica só no log.
                rep.log(&format!("[pip] {}", line.trim()));
                rep.detail(step, msg);
            }
            stdout_text.push_str(&line);
            stdout_text.push('\n');
        }
    }

    let status = child.wait().map_err(|e| format!("falha ao {what}: {e}"))?;
    let stderr_text = err_thread.join().unwrap_or_default();
    if status.success() {
        return Ok(());
    }

    rep.log(&format!(
        "--- falha ao {what}: saída completa ---\n[stdout]\n{stdout_text}\n[stderr]\n{stderr_text}\n--- fim ---"
    ));
    let tail: Vec<&str> = stderr_text.lines().rev().take(25).collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(format!("falha ao {what}:\n{tail}"))
}

/// Traduz uma linha da saída do pip numa frase curta para a tela, sem nomes
/// de pacotes nem termos técnicos: quem instala vê o que está acontecendo
/// (verificando, baixando, otimizando), não com quais tecnologias. `None`
/// para as linhas que não dizem nada de útil (metadados, cache).
fn friendly_pip_line(line: &str) -> Option<String> {
    let l = line.trim();
    if l.starts_with("Collecting ") {
        return Some("Verificando os componentes…".to_string());
    }
    if let Some(rest) = l.strip_prefix("Downloading ") {
        let (file, size) = match rest.rfind(" (") {
            Some(i) => (&rest[..i], rest[i + 2..].trim_end_matches(')')),
            None => (rest, ""),
        };
        if file.ends_with(".metadata") {
            return None;
        }
        return Some(if size.is_empty() {
            "Baixando componente…".to_string()
        } else {
            format!("Baixando componente ({size})…")
        });
    }
    if l.starts_with("Building wheel for ") {
        return Some("Otimizando componentes para o seu computador (pode levar vários minutos)…".to_string());
    }
    if let Some(rest) = l.strip_prefix("Installing collected packages:") {
        let n = rest.split(',').filter(|p| !p.trim().is_empty()).count();
        return Some(format!("Instalando {n} componentes…"));
    }
    if l.starts_with("Successfully installed") {
        return Some("Componentes instalados".to_string());
    }
    None
}

/// Roda um comando curto; em falha devolve as últimas linhas do stderr.
pub fn run_checked(command: &mut Command, what: &str) -> Result<(), String> {
    platform::hide_console(command);
    let output = command.output().map_err(|e| format!("falha ao {what}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = stderr.lines().rev().take(25).collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(format!("falha ao {what}:\n{tail}"))
}

/// Extrai um tar.gz removendo o primeiro nível de diretório.
///
/// O tarball do GitHub embrulha tudo em `<repo>-<sha>/` e o Python do macOS
/// em `python/`; sem remover esse nível os arquivos cairiam uma pasta abaixo
/// do esperado.
pub fn untar_strip_root(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("não consegui abrir {}: {e}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);

    for entry in tar.entries().map_err(|e| format!("pacote inválido: {e}"))? {
        let mut entry = entry.map_err(|e| format!("pacote inválido: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("caminho inválido no pacote: {e}"))?
            .into_owned();

        // Descarta o diretório raiz do tarball.
        let mut parts = path.components();
        parts.next();
        let relative: PathBuf = parts.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }

        // Um tar malicioso pode trazer `..` para escrever fora do destino.
        if relative.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err("pacote contém caminho inválido".into());
        }

        let target = dest.join(&relative);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("não consegui criar {}: {e}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("não consegui criar {}: {e}", parent.display()))?;
        }
        // Um symlink de uma tentativa anterior faria o unpack falhar.
        if std::fs::symlink_metadata(&target).is_ok() {
            let _ = std::fs::remove_file(&target);
        }
        entry
            .unpack(&target)
            .map_err(|e| format!("falha ao extrair {}: {e}", relative.display()))?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, skip: &[&str]) -> Result<(), String> {
    for entry in std::fs::read_dir(from)
        .map_err(|e| format!("não consegui ler {}: {e}", from.display()))?
        .flatten()
    {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip.iter().any(|s| *s == name_str) {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            std::fs::create_dir_all(&dst).map_err(|e| format!("não consegui criar {}: {e}", dst.display()))?;
            copy_tree(&src, &dst, skip)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("não consegui copiar {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod requirements_tests {
    use super::requirements_line;
    use std::io::Write;

    fn write_requirements(name: &str, content: &str) -> std::path::PathBuf {
        // Nome único por teste: os testes rodam em paralelo por padrão.
        let path = std::env::temp_dir().join(format!("dlc-req-test-{}-{}.txt", std::process::id(), name));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn finds_versioned_spec() {
        let path = write_requirements("versioned", "numpy>=2.0,<3\nopencv-python==4.14.0.94\n");
        assert_eq!(requirements_line(&path, "numpy").unwrap(), Some("numpy>=2.0,<3".to_string()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn ignores_platform_marker_and_comments() {
        let path = write_requirements(
            "marker",
            "# a dependency\npygrabber; sys_platform == 'win32'\nnumpy>=2.0,<3  # pinned\n",
        );
        assert_eq!(requirements_line(&path, "numpy").unwrap(), Some("numpy>=2.0,<3".to_string()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_package_returns_none() {
        let path = write_requirements("missing", "opencv-python==4.14.0.94\n");
        assert_eq!(requirements_line(&path, "numpy").unwrap(), None);
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod pip_line_tests {
    use super::friendly_pip_line;

    #[test]
    fn translates_the_lines_that_matter() {
        assert_eq!(
            friendly_pip_line("Collecting onnxruntime-gpu==1.26.0").as_deref(),
            Some("Verificando os componentes…")
        );
        assert_eq!(
            friendly_pip_line("  Downloading onnxruntime_gpu-1.26.0-cp312-cp312-win_amd64.whl (207.3 MB)").as_deref(),
            Some("Baixando componente (207.3 MB)…")
        );
        assert_eq!(
            friendly_pip_line("  Building wheel for insightface (pyproject.toml): started").as_deref(),
            Some("Otimizando componentes para o seu computador (pode levar vários minutos)…")
        );
        assert_eq!(
            friendly_pip_line("Installing collected packages: a, b, c").as_deref(),
            Some("Instalando 3 componentes…")
        );
    }

    #[test]
    fn ignores_noise_and_metadata() {
        assert_eq!(friendly_pip_line("  Downloading foo-1.0-py3-none-any.whl.metadata (6.8 kB)"), None);
        assert_eq!(friendly_pip_line("  Using cached foo-1.0.whl.metadata (5 kB)"), None);
        assert_eq!(friendly_pip_line("  Using cached foo-1.0-py3-none-any.whl (12 kB)"), None);
        assert_eq!(friendly_pip_line("  Preparing metadata (pyproject.toml): finished with status 'done'"), None);
        assert_eq!(friendly_pip_line(""), None);
    }
}
