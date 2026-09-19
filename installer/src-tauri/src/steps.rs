//! Os cinco passos da instalação.
//!
//! Todos são idempotentes: cada um checa o que já existe e pula. Rodar de
//! novo depois de uma falha retoma de onde parou em vez de recomeçar — o que
//! importa quando o passo mais longo baixa mais de 1 GB.

use crate::download::{download_resumable, DownloadSpec};
use crate::models::{self, HF_BASE};
use crate::paths;
use crate::progress::{Reporter, Step};
use std::path::Path;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Python embeddable oficial. Versão fixa de propósito: o app declara
/// suporte a 3.11–3.13, e uma versão fixa é reprodutível — "o mais novo"
/// significaria que uma instalação de amanhã difere da de hoje.
const PYTHON_VERSION: &str = "3.12.8";
const PYTHON_URL: &str =
    "https://www.python.org/ftp/python/3.12.8/python-3.12.8-embed-amd64.zip";

/// Espaço necessário, com folga: modelos + venv com onnxruntime-gpu (que
/// sozinho passa de 1 GB) + Python + margem. Checado ANTES de baixar
/// qualquer coisa — descobrir que falta espaço com 900 MB já baixados é a
/// pior hora de descobrir.
pub fn required_bytes() -> u64 {
    models::total_bytes() + 3 * 1024 * 1024 * 1024
}

/// Passo 1 — Python embeddable.
pub async fn ensure_python(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let dir = paths::python_dir()?;
    let exe = dir.join("python.exe");
    if exe.exists() {
        rep.done(Step::Python, format!("Python {PYTHON_VERSION} já instalado"));
        return Ok(());
    }

    rep.running(Step::Python, format!("Baixando Python {PYTHON_VERSION}…"));
    let archive = paths::root()?.join("python-embed.zip");
    download_resumable(
        client,
        &DownloadSpec { url: PYTHON_URL, target: &archive, expected_size: None },
        |done, total| {
            rep.progress(Step::Python, "Baixando Python…", done, total);
        },
    )
    .await?;

    rep.running(Step::Python, "Extraindo…");
    unzip(&archive, &dir)?;
    let _ = std::fs::remove_file(&archive);

    // O embeddable vem com um ._pth que desliga o import de site-packages.
    // Sem corrigir isso, o venv criado a partir dele não enxerga nada do que
    // o pip instalar.
    enable_site_packages(&dir)?;

    if !exe.exists() {
        return Err("o pacote do Python não trouxe python.exe".into());
    }
    rep.done(Step::Python, format!("Python {PYTHON_VERSION} pronto"));
    Ok(())
}

const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";

/// Passo 2 — pip e dependências, direto no Python embeddable.
///
/// O embeddable não traz `pip` nem `ensurepip` (nem `venv` — ver o comentário
/// em paths::venv_python). A forma oficial de destravar pip nele é baixar
/// get-pip.py e rodá-lo; depois disso ele se comporta como qualquer outro
/// Python para fins de `pip install`.
///
/// Depende do passo 3 (código do app) já ter acontecido, porque
/// requirements.txt vem de lá.
pub async fn ensure_dependencies(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let python = paths::venv_python()?;
    let requirements = paths::app_dir()?.join("requirements.txt");

    if !requirements.exists() {
        return Err(format!(
            "requirements.txt não está em {} — o passo do aplicativo falhou",
            requirements.display()
        ));
    }

    let has_pip = Command::new(&python)
        .args(["-m", "pip", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_pip {
        rep.running(Step::Dependencies, "Preparando o instalador de pacotes…");
        let get_pip = paths::root()?.join("get-pip.py");
        download_resumable(
            client,
            &DownloadSpec { url: GET_PIP_URL, target: &get_pip, expected_size: None },
            |_, _| {},
        )
        .await?;
        run_checked(
            // --no-warn-script-location: os scripts (pip.exe etc.) vão para
            // Scripts/, que não está no PATH deste Python isolado — e não
            // precisa estar, já que só o instalador o invoca diretamente.
            Command::new(&python).args([
                get_pip.to_string_lossy().as_ref(),
                "--no-warn-script-location",
            ]),
            "instalar o pip",
        )?;
    }

    // Este é o passo longo: onnxruntime-gpu e as libs da NVIDIA passam de
    // 1 GB. Sem streaming de progresso por enquanto — o pip não dá números
    // confiáveis para uma barra, e uma barra que mente é pior que nenhuma.
    rep.running(
        Step::Dependencies,
        "Instalando dependências (demora vários minutos)…",
    );
    run_checked(
        Command::new(&python).args([
            "-m",
            "pip",
            "install",
            "-r",
            &requirements.to_string_lossy(),
        ]),
        "instalar as dependências",
    )?;

    rep.done(Step::Dependencies, "Dependências instaladas");
    Ok(())
}

/// Repositório de onde vem o código quando o instalador roda numa máquina
/// que não tem o projeto. Público de propósito: um Release privado exigiria
/// um token embutido no .exe distribuído, o que não é segredo nenhum — é
/// extraível por qualquer um que baixe o instalador.
const RELEASE_REPO: &str = "felvieira/deepfake-studio-live";

/// Passo 3 — código do app, baixado do Release mais recente.
///
/// O tarball do GitHub vem com um diretório raiz do tipo
/// `deepfake-studio-live-<sha>/`, que precisa ser removido na extração para
/// o conteúdo cair direto em app/.
pub async fn fetch_app_code(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let dest = paths::app_dir()?;
    if dest.join("run.py").exists() {
        rep.done(Step::AppCode, "Aplicativo já instalado");
        return Ok(());
    }

    rep.running(Step::AppCode, "Procurando a versão mais recente…");

    // A API pública não precisa de autenticação para um repo público. Sem
    // token: ver o comentário em RELEASE_REPO.
    let api = format!("https://api.github.com/repos/{RELEASE_REPO}/releases/latest");
    let response = client
        .get(&api)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("não consegui falar com o GitHub: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "não consegui consultar as versões ({}). \
             Verifique a conexão e tente de novo.",
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

    rep.running(Step::AppCode, format!("Baixando {tag}…"));
    let archive = paths::root()?.join("app-source.tar.gz");
    download_resumable(
        client,
        &DownloadSpec { url: &tarball, target: &archive, expected_size: None },
        |done, total| {
            rep.progress(Step::AppCode, format!("Baixando {tag}…"), done, total);
        },
    )
    .await?;

    rep.running(Step::AppCode, "Extraindo…");
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("não consegui criar {}: {e}", dest.display()))?;
    untar_strip_root(&archive, &dest)?;
    let _ = std::fs::remove_file(&archive);

    if !dest.join("run.py").exists() {
        return Err("o pacote baixado não contém run.py".into());
    }

    rep.done(Step::AppCode, format!("Aplicativo {tag} instalado"));
    Ok(())
}

/// Passo 3 (modo local) — copia de `source` em vez de baixar.
///
/// Usado quando o instalador roda de dentro do repositório, que é o caso em
/// desenvolvimento. Numa máquina limpa não existe `source`, e aí
/// `fetch_app_code` assume.
pub fn ensure_app_code(rep: &Reporter, source: &Path) -> Result<(), String> {
    let dest = paths::app_dir()?;
    if !source.exists() {
        return Err(format!("código-fonte não encontrado em {}", source.display()));
    }

    rep.running(Step::AppCode, "Copiando o aplicativo…");
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("não consegui criar {}: {e}", dest.display()))?;

    // Não copia venv/, models/ nem lixo de desenvolvimento: o venv é
    // recriado para esta máquina e os modelos vêm do passo 4. Copiar um venv
    // de outra máquina traria caminhos absolutos quebrados.
    let skip = ["venv", "models", ".git", "__pycache__", "installer", ".auto", ".bot"];
    copy_tree(source, &dest, &skip)?;

    rep.done(Step::AppCode, "Aplicativo copiado");
    Ok(())
}

/// Passo 4 — modelos.
pub async fn ensure_models(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let models_dir = paths::models_dir()?;
    let total = models::total_bytes();
    let mut completed: u64 = 0;

    for model in models::MODELS {
        let relative = model.name.replace('/', std::path::MAIN_SEPARATOR_STR);
        let target = models_dir.join(&relative);
        let url = format!("{HF_BASE}{}", model.name);
        let short = model.name.rsplit('/').next().unwrap_or(model.name).to_string();

        let base = completed;
        download_resumable(
            client,
            &DownloadSpec { url: &url, target: &target, expected_size: Some(model.size) },
            |done, _| {
                rep.progress(
                    Step::Models,
                    format!("Baixando {short}…"),
                    base + done,
                    total,
                );
            },
        )
        .await
        .map_err(|e| format!("{}: {e}", model.name))?;

        completed += model.size;
    }

    rep.done(
        Step::Models,
        format!("{} modelos prontos", models::MODELS.len()),
    );
    Ok(())
}

/// Passo 5 — câmera virtual.
///
/// Único passo que pede elevação (regsvr32 precisa). Falha aqui é
/// **degradada, não fatal**: sem o driver o app funciona normalmente, só não
/// publica para Zoom/Discord. Abortar a instalação inteira porque o usuário
/// recusou o UAC seria desproporcional.
pub fn ensure_virtual_camera(rep: &Reporter) -> Result<(), String> {
    let script = paths::app_dir()?.join("install_virtual_camera.bat");
    if !script.exists() {
        rep.degraded(
            Step::VirtualCamera,
            "Script do driver não encontrado; o app funciona sem a câmera virtual",
        );
        return Ok(());
    }

    rep.running(Step::VirtualCamera, "Registrando a câmera virtual (pede permissão)…");

    // Um .bat precisa do cmd, mas o caminho vai depois de /D para desligar
    // o AutoRun do registro, e como argumento próprio — não concatenado numa
    // linha de comando que o cmd reinterpretaria se o caminho do usuário
    // tivesse & ou %.
    let mut command = Command::new("cmd");
    command
        .arg("/D")
        .arg("/C")
        .arg(script.as_os_str())
        .current_dir(paths::app_dir()?);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    match command.output() {
        Ok(output) if output.status.success() => {
            rep.done(Step::VirtualCamera, "Câmera virtual registrada");
        }
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            rep.degraded(
                Step::VirtualCamera,
                format!(
                    "Não foi possível registrar o driver ({}). O app funciona, \
                     mas sem saída para Zoom/Discord. Rode install_virtual_camera.bat \
                     como administrador depois.",
                    detail.trim().chars().take(120).collect::<String>()
                ),
            );
        }
        Err(e) => {
            rep.degraded(
                Step::VirtualCamera,
                format!("Não foi possível executar o instalador do driver: {e}"),
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- utilitários

fn run_checked(command: &mut Command, what: &str) -> Result<(), String> {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|e| format!("falha ao {what}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    // stderr do pip costuma ser longo; as últimas linhas é que dizem o
    // motivo real.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = stderr.lines().rev().take(6).collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(format!("falha ao {what}:\n{tail}"))
}

/// Extrai um tar.gz removendo o primeiro nível de diretório.
///
/// O tarball do GitHub embrulha tudo em `<repo>-<sha>/`; sem remover esse
/// nível o app cairia em app/<repo>-<sha>/run.py e nada acharia nada.
fn untar_strip_root(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("não consegui abrir {}: {e}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);

    for entry in tar
        .entries()
        .map_err(|e| format!("pacote inválido: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("pacote inválido: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("caminho inválido no pacote: {e}"))?
            .into_owned();

        // Descarta o diretório raiz do tarball.
        let mut parts = path.components();
        parts.next();
        let relative: std::path::PathBuf = parts.collect();
        if relative.as_os_str().is_empty() {
            continue;
        }

        // Um tar malicioso pode trazer `..` para escrever fora do destino.
        // Improvável vindo do GitHub, mas a checagem é barata e o estrago
        // não seria.
        if relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
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
        entry
            .unpack(&target)
            .map_err(|e| format!("falha ao extrair {}: {e}", relative.display()))?;
    }
    Ok(())
}

fn unzip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("não consegui abrir {}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip inválido: {e}"))?;
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("não consegui criar {}: {e}", dest.display()))?;
    zip.extract(dest).map_err(|e| format!("falha ao extrair: {e}"))?;
    Ok(())
}

/// O Python embeddable vem com `python312._pth` que comenta `import site`.
/// Com isso o venv não enxerga site-packages e nenhuma dependência carrega.
fn enable_site_packages(python_dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(python_dir)
        .map_err(|e| format!("não consegui ler {}: {e}", python_dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("_pth") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("não consegui ler {}: {e}", path.display()))?;
        if text.contains("\nimport site") && !text.contains("\n#import site") {
            continue; // já habilitado
        }
        let fixed = text.replace("#import site", "import site");
        let fixed = if fixed.contains("import site") {
            fixed
        } else {
            format!("{}\nimport site\n", fixed.trim_end())
        };
        std::fs::write(&path, fixed)
            .map_err(|e| format!("não consegui escrever {}: {e}", path.display()))?;
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
            std::fs::create_dir_all(&dst)
                .map_err(|e| format!("não consegui criar {}: {e}", dst.display()))?;
            copy_tree(&src, &dst, skip)?;
        } else {
            std::fs::copy(&src, &dst)
                .map_err(|e| format!("não consegui copiar {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// Espaço livre no volume da instalação, via GetDiskFreeSpaceExW.
#[cfg(windows)]
pub fn free_disk_bytes() -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    let root = paths::root()?;
    // O diretório ainda pode não existir; sobe até achar um que exista.
    let mut probe = root.as_path();
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return Err("não consegui determinar o volume de instalação".into()),
        }
    }
    let wide: Vec<u16> = probe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut free_for_caller: u64 = 0;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_for_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err("não consegui medir o espaço livre em disco".into());
    }
    Ok(free_for_caller)
}

#[cfg(not(windows))]
pub fn free_disk_bytes() -> Result<u64, String> {
    Err("instalador disponível apenas no Windows".into())
}
