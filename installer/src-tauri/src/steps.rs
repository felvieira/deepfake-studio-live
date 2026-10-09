//! Os cinco passos da instalação.
//!
//! Todos são idempotentes: cada um checa o que já existe e pula. Rodar de
//! novo depois de uma falha retoma de onde parou em vez de recomeçar — o que
//! importa quando o passo mais longo baixa mais de 1 GB.

use crate::download::{download_resumable, DownloadSpec};
use crate::models::{self, HF_BASE};
use crate::paths;
use crate::progress::{Reporter, Step};
use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

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

/// Espaço necessário, com folga: modelos + dependências Python com
/// onnxruntime-gpu (que sozinho passa de 1 GB) + Python + as ferramentas de
/// build do C++ (só o workload VCTools, mas ainda assim ~2-3 GB — ver
/// ensure_build_tools) + margem. Checado ANTES de baixar qualquer coisa —
/// descobrir que falta espaço com 900 MB já baixados é a pior hora de
/// descobrir.
pub fn required_bytes() -> u64 {
    // 8 GB além dos modelos: ~6 GB de Python + dependências + build, e mais
    // ~2 GB das bibliotecas CUDA quando há placa NVIDIA.
    models::total_bytes() + 8 * 1024 * 1024 * 1024
}

/// Há um driver NVIDIA instalado? `nvcuda.dll` é o que o driver coloca em
/// System32, e é exatamente o que o onnxruntime procura para usar a placa.
pub fn has_nvidia_gpu() -> bool {
    std::env::var_os("SystemRoot")
        .map(|root| std::path::Path::new(&root).join("System32").join("nvcuda.dll").exists())
        .unwrap_or(false)
}

/// Pacotes com as bibliotecas CUDA 12 que o onnxruntime-gpu carrega
/// (cuBLAS, cuDNN, cuFFT, cuRAND, runtime, NVRTC). O requirements.txt só
/// lista o `onnxruntime-gpu`: sem estas DLLs o provider CUDA falha ao
/// carregar ("cublasLt64_12.dll is missing") e o app cai para CPU em
/// silêncio, ~17x mais lento. O upstream pede para o usuário instalar o
/// CUDA Toolkit e o cuDNN à mão; aqui vêm por pip, sem instalar nada no
/// sistema, e o run.py já registra nvidia/*/bin como pasta de DLLs.
const CUDA_PACKAGES: &[&str] = &[
    "nvidia-cuda-runtime-cu12",
    "nvidia-cublas-cu12",
    "nvidia-cudnn-cu12",
    "nvidia-cufft-cu12",
    "nvidia-curand-cu12",
    "nvidia-cuda-nvrtc-cu12",
];

/// As bibliotecas CUDA já estão no Python da instalação?
pub fn cuda_libs_installed() -> bool {
    paths::python_dir()
        .map(|p| {
            p.join("Lib").join("site-packages").join("nvidia").join("cublas")
                .join("bin").join("cublasLt64_12.dll").exists()
        })
        .unwrap_or(false)
}

/// Passo 1 — Python embeddable.
pub async fn ensure_python(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    let dir = paths::python_dir()?;
    let exe = dir.join("python.exe");
    if exe.exists() {
        rep.done(Step::Python, "Ambiente já configurado");
        // Ainda assim reaplica os dois ajustes seguintes, mesmo já tendo
        // "terminado" este passo antes: cada um foi adicionado depois que
        // python.exe já podia existir de uma tentativa anterior, e sem
        // reaplicar aqui uma reinstalação nunca fecharia essa lacuna — foi
        // exatamente isso que aconteceu com fix_pth_restrictions ao ser
        // adicionada. Os dois são baratos e idempotentes.
        fix_pth_restrictions(&dir, &paths::app_dir()?)?;
        return ensure_dev_headers(client, rep, &dir).await;
    }

    rep.running(Step::Python, "Configurando o ambiente…");
    rep.log(&format!("baixando Python {PYTHON_VERSION} embeddable"));
    let archive = paths::root()?.join("python-embed.zip");
    download_resumable(
        client,
        &DownloadSpec { url: PYTHON_URL, target: &archive, expected_size: None },
        |done, total| {
            rep.progress(Step::Python, "Baixando os arquivos do ambiente…", done, total);
        },
    )
    .await?;

    rep.running(Step::Python, "Preparando o ambiente…");
    unzip(&archive, &dir)?;
    let _ = std::fs::remove_file(&archive);

    // O embeddable vem com um ._pth que desliga o import de site-packages.
    // Sem corrigir isso, o venv criado a partir dele não enxerga nada do que
    // o pip instalar.
    fix_pth_restrictions(&dir, &paths::app_dir()?)?;

    if !exe.exists() {
        return Err("o pacote do Python não trouxe python.exe".into());
    }

    ensure_dev_headers(client, rep, &dir).await?;

    rep.done(Step::Python, "Ambiente configurado");
    Ok(())
}

const PYTHON_NUGET_URL: &str =
    "https://api.nuget.org/v3-flatcontainer/python/3.12.8/python.3.12.8.nupkg";

/// O embeddable não traz `include/Python.h` nem `libs/python312.lib` — é
/// deliberadamente uma distribuição de execução, não de desenvolvimento.
/// Sem esses dois, nenhuma extensão C/Cython consegue compilar contra ele,
/// não importa quantas ferramentas de build estejam instaladas no sistema:
/// confirmado reproduzindo localmente a mesma falha do `insightface` com
/// vcvarsall ativado e o compilador funcionando, e só passando depois de
/// testar contra um Python instalado normalmente (que tem os dois).
///
/// O pacote NuGet oficial `python` da própria Microsoft empacota exatamente
/// esses dois diretórios (em tools/include/ e tools/libs/) sem trazer o
/// Python inteiro — é a forma documentada de completar um embeddable para
/// compilação, sem precisar instalar Visual Studio's Python payload nem o
/// instalador cheio do python.org.
async fn ensure_dev_headers(
    client: &reqwest::Client,
    rep: &Reporter,
    python_dir: &Path,
) -> Result<(), String> {
    if python_dir.join("include").join("Python.h").exists() {
        return Ok(());
    }

    rep.running(Step::Python, "Baixando arquivos complementares do ambiente…");
    let nuget = paths::root()?.join("python-dev.nupkg");
    download_resumable(
        client,
        &DownloadSpec { url: PYTHON_NUGET_URL, target: &nuget, expected_size: None },
        |done, total| {
            rep.progress(Step::Python, "Baixando arquivos complementares…", done, total);
        },
    )
    .await?;

    rep.running(Step::Python, "Preparando arquivos complementares…");
    // O .nupkg é um zip comum; só interessam tools/include e tools/libs.
    unzip_subdirs(&nuget, python_dir, &[("tools/include", "include"), ("tools/libs", "libs")])?;
    let _ = std::fs::remove_file(&nuget);

    if !python_dir.join("include").join("Python.h").exists() {
        return Err(
            "o pacote de cabeçalhos do Python não trouxe include/Python.h".into(),
        );
    }
    Ok(())
}

const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";

/// Bootstrapper oficial da Microsoft — só baixa o instalador (poucos MB); os
/// componentes de verdade (compilador + Windows SDK, alguns GB) ele busca
/// sozinho na hora de instalar.
const VS_BUILDTOOLS_URL: &str = "https://aka.ms/vs/17/release/vs_BuildTools.exe";

/// Garante o compilador MSVC no sistema.
///
/// `insightface==0.7.3` não tem wheel pré-compilado para nenhuma plataforma
/// no PyPI — só existe o sdist — e seu setup.py compila uma extensão
/// Cython/C. O upstream (hacksider/Deep-Live-Cam) documenta isso como
/// pré-requisito manual: "Visual Studio 2022 Runtimes — Visual C++ Build
/// Tools". Numa máquina de desenvolvimento isso quase sempre já está
/// presente — Visual Studio é comum — e por isso o requisito nunca aparecia
/// nos testes até rodar numa VM realmente limpa.
///
/// Instala só o workload VCTools (compilador + Windows SDK), não o Visual
/// Studio inteiro — ainda assim leva vários minutos e alguns GB, e é o
/// único outro passo que pede elevação além da câmera virtual.
async fn ensure_build_tools(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    if msvc_compiler_present() {
        rep.log("compilador MSVC já presente, pulando Build Tools");
        return Ok(());
    }

    rep.running(
        Step::Dependencies,
        "Baixando ferramentas do Windows necessárias…",
    );
    let installer = paths::root()?.join("vs_buildtools.exe");
    download_resumable(
        client,
        &DownloadSpec { url: VS_BUILDTOOLS_URL, target: &installer, expected_size: None },
        |_, _| {},
    )
    .await?;

    rep.running(
        Step::Dependencies,
        "Instalando ferramentas do Windows (pode pedir permissão e levar alguns minutos)…",
    );
    // --passive: mostra progresso sem exigir clique; --wait: o instalador da
    // Microsoft normalmente se desacopla do processo pai e retorna na hora,
    // então sem isso achamos (erradamente) que já terminou.
    let output = Command::new(&installer)
        .args([
            "--quiet",
            "--wait",
            "--norestart",
            "--nocache",
            "--add",
            "Microsoft.VisualStudio.Workload.VCTools",
            "--includeRecommended",
        ])
        .output()
        .map_err(|e| format!("não consegui iniciar o instalador de build tools: {e}"))?;

    // 3010 = sucesso, mas pede reinício do Windows — não impede pip install
    // funcionar nesta mesma sessão, então trata como sucesso.
    let code = output.status.code().unwrap_or(-1);
    if !(output.status.success() || code == 3010) {
        return Err(format!(
            "instalação das ferramentas de compilação falhou (código {code}). \
             Instale manualmente o 'Visual C++ Build Tools' e tente de novo."
        ));
    }

    if !msvc_compiler_present() {
        return Err(
            "as ferramentas de compilação foram instaladas, mas o compilador \
             não foi encontrado — pode ser necessário reiniciar o Windows e \
             tentar de novo"
                .into(),
        );
    }

    rep.log("Build Tools instalado com sucesso");
    Ok(())
}

/// Usa o vswhere oficial (vem com todo Visual Studio/Build Tools desde
/// 2017, em local fixo) para achar uma instalação com o workload de C++.
/// `None` cobre tanto "vswhere não existe" (nenhum VS/Build Tools no
/// sistema) quanto "existe mas sem esse componente".
fn vs_installation_path() -> Option<PathBuf> {
    let vswhere = PathBuf::from(std::env::var("ProgramFiles(x86)").unwrap_or_default())
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    if !vswhere.exists() {
        return None;
    }
    let output = Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn msvc_compiler_present() -> bool {
    vs_installation_path().is_some()
}

/// Caminho do script que configura INCLUDE/LIB/PATH para achar cl.exe.
///
/// Instalar o Build Tools só coloca os arquivos no disco — nenhum processo,
/// nem os que o instalador cria depois, herda automaticamente as variáveis
/// de ambiente que apontam para o compilador. É por isso que, mesmo com o
/// compilador instalado e detectado (msvc_compiler_present() == true), `pip
/// install` de um pacote que precisa compilar continuava falhando do
/// mesmo jeito: o subprocesso do pip não via cl.exe em lugar nenhum. Um
/// "Developer Command Prompt" resolve isso rodando este script antes de
/// mais nada — é o que build_command() replica por baixo dos panos.
fn vcvarsall_path(vs_root: &Path) -> PathBuf {
    vs_root
        .join("VC")
        .join("Auxiliary")
        .join("Build")
        .join("vcvarsall.bat")
}

/// Monta um `cmd /C` que ativa o ambiente do MSVC (se disponível) e então
/// roda o comando pedido. Sem Visual Studio instalado, roda o comando puro
/// — cobre o caso em que requirements.txt algum dia não precisar mais
/// compilar nada.
fn build_command(python: &Path, args: &[&str]) -> Command {
    let python_str = python.to_string_lossy();
    let inner = format!(
        "\"{}\" {}",
        python_str,
        args.iter()
            .map(|a| format!("\"{a}\""))
            .collect::<Vec<_>>()
            .join(" ")
    );

    if let Some(vs_root) = vs_installation_path() {
        let vcvarsall = vcvarsall_path(&vs_root);
        if vcvarsall.exists() {
            let mut command = Command::new("cmd");
            // Command::args() faz o quoting automático de cada argumento
            // antes de montar a linha de comando que o Windows recebe — e
            // como este argumento já tem aspas internas (em volta do
            // caminho do vcvarsall.bat, que tem espaços), esse quoting
            // automático as escapa com barra invertida, o que o cmd.exe não
            // entende: ele via `\"C:\Program Files\...\"` literal em vez de
            // um caminho entre aspas, e recusava com "não é reconhecido
            // como um comando". raw_arg() passa a string exatamente como
            // está, sem esse processamento — que é o que este caso exige,
            // já que estamos montando a linha de comando à mão.
            command.raw_arg("/D");
            command.raw_arg("/C");
            command.raw_arg(format!(
                "\"call \"{}\" amd64 && {inner}\"",
                vcvarsall.display()
            ));
            return command;
        }
    }

    let mut command = Command::new(python);
    command.args(args);
    command
}

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

    ensure_build_tools(client, rep).await?;

    if !has_pip {
        rep.running(Step::Dependencies, "Preparando a instalação dos componentes…");
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

    // get-pip.py só traz o pip. `insightface` (e outros pacotes do
    // requirements.txt sem wheel pronto para esta combinação de Python/SO)
    // precisa compilar sua extensão, e o backend de build PEP 517 padrão
    // (setuptools.build_meta) não existe se setuptools não estiver
    // instalado — falha com "BackendUnavailable: Cannot import
    // 'setuptools.build_meta'". Isso não aparece com o Python do sistema
    // porque a maioria das instalações já traz setuptools de fábrica.
    rep.running(Step::Dependencies, "Preparando os componentes…");
    run_streamed(
        Command::new(&python).args([
            "-m", "pip", "install", "--progress-bar", "off", "setuptools", "wheel",
        ]),
        "instalar setuptools/wheel",
        rep,
        Step::Dependencies,
    )?;

    // insightface compila extensões Cython/C e seu setup.py importa numpy e
    // Cython diretamente (numpy.get_include(), cythonize()) para fazer isso.
    // O isolamento de build do pip cria um ambiente novo por pacote, então
    // listar numpy/Cython antes de insightface no requirements.txt não
    // basta — o ambiente de build do insightface não enxerga o que ainda
    // não foi instalado no ambiente real. Precisam existir ANTES de
    // processar o requirements.txt inteiro. A versão do numpy vem do
    // próprio requirements.txt para não divergir da faixa que o projeto
    // pede; Cython não está listado lá — só é preciso para compilar, o
    // pacote final não depende dele em runtime — então vai sem pin.
    let numpy_spec = requirements_line(&requirements, "numpy")?.unwrap_or_else(|| "numpy".into());
    rep.running(Step::Dependencies, "Preparando os componentes…");
    run_streamed(
        &mut build_command(
            &python,
            &["-m", "pip", "install", "--progress-bar", "off", &numpy_spec, "Cython"],
        ),
        "instalar numpy/Cython",
        rep,
        Step::Dependencies,
    )?;

    // Este é o passo longo: onnxruntime-gpu e as libs da NVIDIA passam de
    // 1 GB. Sem streaming de progresso por enquanto — o pip não dá números
    // confiáveis para uma barra, e uma barra que mente é pior que nenhuma.
    //
    // build_command() ativa o ambiente do MSVC antes de chamar o pip:
    // instalar o compilador sozinho não basta, porque nenhum processo
    // herda INCLUDE/LIB/PATH automaticamente — só um "Developer Command
    // Prompt" (ou o vcvarsall.bat que ele roda) configura isso. Sem essa
    // ativação, `pip install -r requirements.txt` falhava ao compilar
    // insightface mesmo com o compilador já instalado e detectado.
    let requirements_str = requirements.to_string_lossy();
    rep.running(
        Step::Dependencies,
        "Instalando os componentes (pode levar vários minutos)…",
    );
    run_streamed(
        &mut build_command(
            &python,
            &["-m", "pip", "install", "--progress-bar", "off", "-r", &requirements_str],
        ),
        "instalar as dependências",
        rep,
        Step::Dependencies,
    )?;

    // Bibliotecas CUDA: só com placa NVIDIA, e pip pula o que já está
    // instalado, então rodar de novo é barato.
    if has_nvidia_gpu() {
        rep.running(
            Step::Dependencies,
            "Ativando a aceleração pela placa de vídeo (~1,7 GB)…",
        );
        let mut args = vec!["-m", "pip", "install", "--progress-bar", "off"];
        args.extend(CUDA_PACKAGES);
        run_streamed(
            Command::new(&python).args(&args),
            "instalar as bibliotecas CUDA",
            rep,
            Step::Dependencies,
        )?;
    }

    rep.done(Step::Dependencies, "Componentes instalados");
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

    rep.running(Step::AppCode, format!("Baixando o aplicativo ({tag})…"));
    let archive = paths::root()?.join("app-source.tar.gz");
    download_resumable(
        client,
        &DownloadSpec { url: &tarball, target: &archive, expected_size: None },
        |done, total| {
            rep.progress(Step::AppCode, format!("Baixando o aplicativo ({tag})…"), done, total);
        },
    )
    .await?;

    rep.running(Step::AppCode, "Descompactando o aplicativo…");
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

    rep.running(Step::AppCode, "Copiando os arquivos do aplicativo…");
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
            &DownloadSpec { url: &url, target: &target, expected_size: Some(model.size) },
            |done, _| {
                rep.progress(
                    Step::Models,
                    label.clone(),
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
            "Câmera virtual indisponível nesta instalação; o app funciona normalmente sem ela",
        );
        return Ok(());
    }

    rep.running(Step::VirtualCamera, "Ativando a câmera virtual (o Windows pode pedir permissão)…");

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
            rep.log(&format!("câmera virtual: falha ao registrar o driver: {}", detail.trim()));
            rep.degraded(
                Step::VirtualCamera,
                "Não foi possível ativar a câmera virtual. O app funciona normalmente, \
                 mas sem enviar vídeo para Zoom, Discord ou Teams. Para tentar de novo, \
                 abra o instalador e clique em Instalar.",
            );
        }
        Err(e) => {
            rep.log(&format!("câmera virtual: não executou o script do driver: {e}"));
            rep.degraded(
                Step::VirtualCamera,
                "Não foi possível ativar a câmera virtual. O app funciona normalmente sem ela.",
            );
        }
    }
    Ok(())
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
/// `pip install -r requirements.txt` leva vários minutos e antes não
/// emitia nada até terminar: a tela ficava parada na mesma frase e quem
/// instalava não tinha como saber se estava trabalhando ou travado. Aqui a
/// saída é lida linha a linha e as linhas que dizem algo útil viram uma
/// mensagem curta ("Baixando onnxruntime_gpu (207 MB)…") enviada à tela.
///
/// O stderr é lido numa thread à parte: se ninguém o esvaziar, um pipe cheio
/// bloqueia o processo filho e a instalação trava de verdade.
fn run_streamed(
    command: &mut Command,
    what: &str,
    rep: &Reporter,
    step: Step,
) -> Result<(), String> {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("falha ao {what}: {e}"))?;

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

    let status = child
        .wait()
        .map_err(|e| format!("falha ao {what}: {e}"))?;
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
        return Some(
            "Otimizando componentes para o seu computador (pode levar vários minutos)…".to_string(),
        );
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

fn run_checked(command: &mut Command, what: &str) -> Result<(), String> {
    run_checked_logged(command, what, None)
}

/// Como run_checked, mas quando falha grava o stderr inteiro no
/// install.log (via `rep`) antes de devolver só um resumo pro chamador.
///
/// As últimas linhas do stderr de uma falha de compilação costumam ser só
/// o resumo genérico do pip ("ERROR: Failed building wheel for X") — o
/// erro real do cl.exe/Cython fica minutos antes, no meio da saída. Cortar
/// para "as últimas N linhas" perde exatamente a informação que diagnostica
/// o problema. O log grava tudo; a UI mostra um resumo maior que os 6
/// linhas de antes, o suficiente pra a maioria dos casos sem inundar a tela.
fn run_checked_logged(
    command: &mut Command,
    what: &str,
    rep: Option<&Reporter>,
) -> Result<(), String> {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|e| format!("falha ao {what}: {e}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if let Some(rep) = rep {
        rep.log(&format!(
            "--- falha ao {what}: saída completa ---\n[stdout]\n{stdout}\n[stderr]\n{stderr}\n--- fim ---"
        ));
    }

    let tail: Vec<&str> = stderr.lines().rev().take(25).collect();
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

/// Extrai só as entradas de um zip cujo caminho comece por um dos prefixos
/// em `mappings`, remapeando cada prefixo para uma pasta de destino.
///
/// Usado para tirar `tools/include/` e `tools/libs/` de dentro do .nupkg do
/// Python (que tem muito mais coisa que isso — o pacote inteiro do Python)
/// sem extrair o resto.
fn unzip_subdirs(archive: &Path, dest: &Path, mappings: &[(&str, &str)]) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("não consegui abrir {}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip inválido: {e}"))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("entrada inválida no zip: {e}"))?;
        let name = entry.name().replace('\\', "/");

        let Some((_, target_root)) = mappings
            .iter()
            .find(|(prefix, _)| name.starts_with(&format!("{prefix}/")))
        else {
            continue;
        };
        let prefix = mappings
            .iter()
            .find(|(prefix, _)| name.starts_with(&format!("{prefix}/")))
            .map(|(p, _)| *p)
            .unwrap();
        let relative = &name[prefix.len() + 1..];
        if relative.is_empty() {
            continue;
        }
        // O nome vem do zip sem qualquer sanitização; uma entrada como
        // "tools/include/../../../Windows/evil.dll" escreveria fora de
        // dest se não for barrada aqui. O NuGet oficial não faz isso, mas
        // a função é genérica o bastante pra ser reaproveitada com uma
        // fonte menos confiável mais tarde — mesma checagem que
        // untar_strip_root já faz para o tarball do GitHub.
        if Path::new(relative)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("pacote contém caminho inválido: {name}"));
        }

        let target = dest.join(target_root).join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("não consegui criar {}: {e}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("não consegui criar {}: {e}", parent.display()))?;
        }
        let mut out = std::fs::File::create(&target)
            .map_err(|e| format!("não consegui criar {}: {e}", target.display()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("falha ao extrair {}: {e}", target.display()))?;
    }
    Ok(())
}

/// Corrige as duas restrições do `._pth` que o Python embeddable vem com
/// por padrão.
///
/// Um arquivo `<versão>._pth` presente faz o Python ignorar TODA forma
/// usual de montar sys.path — variáveis de ambiente como PYTHONPATH
/// incluídas — e usar só o que está listado nesse arquivo. Isso quebra
/// duas coisas ao mesmo tempo, e as duas só aparecem testando de verdade:
///
/// 1. `import site` vem comentado, então site-packages nunca é adicionado
///    e nada que o pip instalar carrega.
/// 2. sys.path[0] normalmente seria o diretório do script (aqui, app/,
///    onde run.py mora) — mas o `._pth` também desativa essa adição
///    automática. `from modules import platform_info` em run.py falhava
///    com "ModuleNotFoundError: No module named 'modules'" mesmo com
///    modules/ presente e correto, porque app/ nunca chegava a entrar no
///    sys.path — nem current_dir() nem PYTHONPATH mudam isso, confirmado
///    testando os dois isoladamente contra um embeddable real. Só editar
///    o próprio `._pth` funciona.
///
/// `app_dir` ainda pode não existir quando isto roda (é chamado durante o
/// passo do Python, antes do código do app ser copiado) — o caminho é
/// gravado de qualquer forma, porque é fixo (sempre root()/app) e o Python
/// só precisa que ele exista no momento de rodar run.py, não agora.
pub fn fix_pth_restrictions(python_dir: &Path, app_dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(python_dir)
        .map_err(|e| format!("não consegui ler {}: {e}", python_dir.display()))?;
    let app_dir_str = app_dir.to_string_lossy().to_string();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("_pth") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("não consegui ler {}: {e}", path.display()))?;

        let mut fixed = text.replace("#import site", "import site");
        if !fixed.contains("import site") {
            fixed = format!("{}\nimport site\n", fixed.trim_end());
        }
        if !fixed.lines().any(|l| l.trim() == app_dir_str) {
            // Logo depois da primeira linha (o "." padrão do embeddable),
            // antes do bloco de comentário/import site.
            fixed = fixed.replacen('\n', &format!("\n{app_dir_str}\n"), 1);
        }

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

#[cfg(test)]
mod requirements_tests {
    use super::requirements_line;
    use std::io::Write;

    fn write_requirements(name: &str, content: &str) -> std::path::PathBuf {
        // Nome único por teste: os testes rodam em paralelo por padrão, e um
        // nome de arquivo compartilhado faz um teste sobrescrever o arquivo
        // que outro acabou de escrever antes de lê-lo de volta.
        let path = std::env::temp_dir().join(format!(
            "dlc-req-test-{}-{}.txt",
            std::process::id(),
            name
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn finds_versioned_spec() {
        let path = write_requirements(
            "versioned",
            "numpy>=2.0,<3\nopencv-python==4.14.0.94\n",
        );
        let found = requirements_line(&path, "numpy").unwrap();
        assert_eq!(found, Some("numpy>=2.0,<3".to_string()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn ignores_platform_marker_and_comments() {
        let path = write_requirements(
            "marker",
            "# a dependency\npygrabber; sys_platform == 'win32'\nnumpy>=2.0,<3  # pinned\n",
        );
        let found = requirements_line(&path, "numpy").unwrap();
        assert_eq!(found, Some("numpy>=2.0,<3".to_string()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_package_returns_none() {
        let path = write_requirements("missing", "opencv-python==4.14.0.94\n");
        let found = requirements_line(&path, "numpy").unwrap();
        assert_eq!(found, None);
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod pth_tests {
    use super::fix_pth_restrictions;

    /// O `._pth` real do embeddable usa CRLF. Estado de partida = o que
    /// versões antigas do instalador deixaram (só `import site`, sem o
    /// caminho do app) — é exatamente o que a VM tinha.
    const OLD_STATE: &str = "python312.zip
.

# Uncomment to run site.main() automatically
import site
";

    fn setup(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dlc-pth-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("python312._pth"), OLD_STATE).unwrap();
        let app = dir.join("app");
        (dir, app)
    }

    #[test]
    fn adds_app_dir_to_an_already_site_enabled_pth() {
        let (dir, app) = setup("add");
        fix_pth_restrictions(&dir, &app).unwrap();
        let text = std::fs::read_to_string(dir.join("python312._pth")).unwrap();
        assert!(text.lines().any(|l| l.trim() == app.to_string_lossy()), "{text}");
        assert!(text.lines().any(|l| l.trim() == "import site"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_idempotent() {
        let (dir, app) = setup("idem");
        fix_pth_restrictions(&dir, &app).unwrap();
        fix_pth_restrictions(&dir, &app).unwrap();
        let text = std::fs::read_to_string(dir.join("python312._pth")).unwrap();
        let n = text.lines().filter(|l| l.trim() == app.to_string_lossy()).count();
        assert_eq!(n, 1, "{text}");
        std::fs::remove_dir_all(&dir).ok();
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
