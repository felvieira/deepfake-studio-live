//! Windows: Python embeddable, Visual C++ Build Tools, bibliotecas CUDA,
//! câmera virtual Unity Capture e atalhos .lnk.

use crate::download::{download_resumable, DownloadSpec};
use crate::progress::{Reporter, Step};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Não abrir janela de console junto com os processos filhos.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Esconde a janela de console de um processo filho.
pub fn hide_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Raiz da instalação: %LOCALAPPDATA%\DeepLiveCam. Gravável pelo usuário
/// sem elevação, sobrevive a atualizações e sai limpo na desinstalação.
pub fn default_root() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA")
        .map_err(|_| "LOCALAPPDATA não está definido — ambiente Windows inesperado".to_string())?;
    Ok(PathBuf::from(base).join("DeepLiveCam"))
}

// ------------------------------------------------------------------ Python

/// Python embeddable oficial, versão e arquivo fixos (reprodutível: "o mais
/// novo" faria a instalação de amanhã diferir da de hoje).
pub const PYTHON_LABEL: &str = "Python 3.12.8 embeddable";
const PYTHON_URL: &str = "https://www.python.org/ftp/python/3.12.8/python-3.12.8-embed-amd64.zip";
const PYTHON_SHA256: &str = "8d3f33be9eb810f23c102f08475af2854e50484b8e4e06275e937be61ce3d2fb";

/// Cabeçalhos e libs para compilar extensões — ver ensure_dev_headers.
const PYTHON_NUGET_URL: &str =
    "https://api.nuget.org/v3-flatcontainer/python/3.12.8/python.3.12.8.nupkg";
const PYTHON_NUGET_SHA256: &str =
    "406856be971d957e0bee7a5cefe20a5ec78d70a495e9e33cd0e53d31faec049d";

pub fn python_executable(python_dir: &Path) -> PathBuf {
    python_dir.join("python.exe")
}

pub fn site_packages(python_dir: &Path) -> PathBuf {
    python_dir.join("Lib").join("site-packages")
}

/// Baixa e extrai o embeddable. Só roda quando python.exe ainda não existe.
pub async fn install_python(
    client: &reqwest::Client,
    rep: &Reporter,
    python_dir: &Path,
) -> Result<(), String> {
    let archive = crate::paths::root()?.join("python-embed.zip");
    download_resumable(
        client,
        &DownloadSpec {
            url: PYTHON_URL,
            target: &archive,
            expected_size: None,
            sha256: Some(PYTHON_SHA256),
        },
        |done, total| {
            rep.progress(Step::Python, "Baixando os arquivos do ambiente…", done, total);
        },
    )
    .await?;

    rep.running(Step::Python, "Preparando o ambiente…");
    unzip(&archive, python_dir)?;
    let _ = std::fs::remove_file(&archive);
    Ok(())
}

/// Ajustes que rodam a cada instalação, mesmo com o Python já presente.
///
/// Os dois foram adicionados depois que python.exe já podia existir de uma
/// tentativa anterior; sem reaplicar aqui, uma reinstalação nunca fecharia
/// essa lacuna (foi o que aconteceu com fix_pth_restrictions). São baratos e
/// idempotentes.
pub async fn repair_python(
    client: &reqwest::Client,
    rep: &Reporter,
    python_dir: &Path,
    app_dir: &Path,
) -> Result<(), String> {
    fix_pth_restrictions(python_dir, app_dir)?;
    ensure_dev_headers(client, rep, python_dir).await
}

/// Antes de abrir o app: reaplica o ajuste do `._pth`, que também cobre quem
/// instalou com uma versão antiga do instalador.
pub fn prepare_launch(python_dir: &Path, app_dir: &Path) -> Result<(), String> {
    fix_pth_restrictions(python_dir, app_dir)
}

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
    let nuget = crate::paths::root()?.join("python-dev.nupkg");
    download_resumable(
        client,
        &DownloadSpec {
            url: PYTHON_NUGET_URL,
            target: &nuget,
            expected_size: None,
            sha256: Some(PYTHON_NUGET_SHA256),
        },
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
        return Err("o pacote de cabeçalhos do Python não trouxe include/Python.h".into());
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

// --------------------------------------------------------------- pip e build

const GET_PIP_URL: &str = "https://bootstrap.pypa.io/get-pip.py";

/// O embeddable não traz `pip` nem `ensurepip` (nem `venv`). A forma oficial
/// de destravar pip nele é baixar get-pip.py e rodá-lo; depois disso ele se
/// comporta como qualquer outro Python para fins de `pip install`.
pub async fn ensure_pip(client: &reqwest::Client, rep: &Reporter, python: &Path) -> Result<(), String> {
    let mut check = Command::new(python);
    check.args(["-m", "pip", "--version"]);
    hide_console(&mut check);
    if check.output().map(|o| o.status.success()).unwrap_or(false) {
        return Ok(());
    }

    rep.running(Step::Dependencies, "Preparando a instalação dos componentes…");
    let get_pip = crate::paths::root()?.join("get-pip.py");
    download_resumable(
        client,
        &DownloadSpec { url: GET_PIP_URL, target: &get_pip, expected_size: None, sha256: None },
        |_, _| {},
    )
    .await?;
    crate::steps::run_checked(
        // --no-warn-script-location: os scripts (pip.exe etc.) vão para
        // Scripts/, que não está no PATH deste Python isolado — e não
        // precisa estar, já que só o instalador o invoca diretamente.
        Command::new(python).args([get_pip.to_string_lossy().as_ref(), "--no-warn-script-location"]),
        "instalar o pip",
    )
}

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
pub async fn ensure_build_tools(client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    if vs_installation_path().is_some() {
        rep.log("compilador MSVC já presente, pulando Build Tools");
        return Ok(());
    }

    rep.running(Step::Dependencies, "Baixando ferramentas do Windows necessárias…");
    let installer = crate::paths::root()?.join("vs_buildtools.exe");
    download_resumable(
        client,
        &DownloadSpec { url: VS_BUILDTOOLS_URL, target: &installer, expected_size: None, sha256: None },
        |_, _| {},
    )
    .await?;

    rep.running(
        Step::Dependencies,
        "Instalando ferramentas do Windows (pode pedir permissão e levar alguns minutos)…",
    );
    // --wait: o instalador da Microsoft normalmente se desacopla do processo
    // pai e retorna na hora, então sem isso achamos (erradamente) que já
    // terminou.
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

    if vs_installation_path().is_none() {
        return Err("as ferramentas de compilação foram instaladas, mas o compilador \
                    não foi encontrado — pode ser necessário reiniciar o Windows e \
                    tentar de novo"
            .into());
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
    let mut command = Command::new(&vswhere);
    command.args([
        "-latest",
        "-products",
        "*",
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-property",
        "installationPath",
    ]);
    hide_console(&mut command);
    let output = command.output().ok()?;
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

/// Monta um `cmd /C` que ativa o ambiente do MSVC (se disponível) e então
/// roda o `python` pedido.
///
/// Instalar o Build Tools só coloca os arquivos no disco — nenhum processo,
/// nem os que o instalador cria depois, herda automaticamente as variáveis
/// de ambiente que apontam para o compilador. É por isso que, mesmo com o
/// compilador instalado e detectado, `pip install` de um pacote que precisa
/// compilar continuava falhando: o subprocesso do pip não via cl.exe em
/// lugar nenhum. Um "Developer Command Prompt" resolve isso rodando
/// vcvarsall.bat antes de mais nada — é o que isto replica. Sem Visual
/// Studio instalado, roda o comando puro.
pub fn pip_command(python: &Path, args: &[&str]) -> Command {
    let inner = format!(
        "\"{}\" {}",
        python.to_string_lossy(),
        args.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(" ")
    );

    if let Some(vs_root) = vs_installation_path() {
        let vcvarsall = vs_root.join("VC").join("Auxiliary").join("Build").join("vcvarsall.bat");
        if vcvarsall.exists() {
            let mut command = Command::new("cmd");
            // Command::args() faz o quoting automático de cada argumento, e
            // como este argumento já tem aspas internas (em volta do caminho
            // do vcvarsall.bat, que tem espaços), esse quoting as escapa com
            // barra invertida, o que o cmd.exe não entende. raw_arg() passa a
            // string exatamente como está.
            command.raw_arg("/D");
            command.raw_arg("/C");
            command.raw_arg(format!("\"call \"{}\" amd64 && {inner}\"", vcvarsall.display()));
            return command;
        }
    }

    let mut command = Command::new(python);
    command.args(args);
    command
}

// -------------------------------------------------------------- aceleração

/// Há um driver NVIDIA instalado? `nvcuda.dll` é o que o driver coloca em
/// System32, e é exatamente o que o onnxruntime procura para usar a placa.
fn has_nvidia_gpu() -> bool {
    std::env::var_os("SystemRoot")
        .map(|root| Path::new(&root).join("System32").join("nvcuda.dll").exists())
        .unwrap_or(false)
}

/// Pacotes com as bibliotecas CUDA 12 que o onnxruntime-gpu carrega
/// (cuBLAS, cuDNN, cuFFT, cuRAND, runtime, NVRTC). O requirements.txt só
/// lista o `onnxruntime-gpu`: sem estas DLLs o provider CUDA falha ao
/// carregar ("cublasLt64_12.dll is missing") e o app cai para CPU em
/// silêncio, ~17x mais lento. Vêm por pip, sem instalar nada no sistema, e o
/// run.py já registra nvidia/*/bin como pasta de DLLs.
const CUDA_PACKAGES: &[&str] = &[
    "nvidia-cuda-runtime-cu12",
    "nvidia-cublas-cu12",
    "nvidia-cudnn-cu12",
    "nvidia-cufft-cu12",
    "nvidia-curand-cu12",
    "nvidia-cuda-nvrtc-cu12",
];

/// Pacotes extras de aceleração para esta máquina (vazio sem placa NVIDIA).
pub fn gpu_packages() -> &'static [&'static str] {
    if has_nvidia_gpu() {
        CUDA_PACKAGES
    } else {
        &[]
    }
}

pub const GPU_PACKAGES_MESSAGE: &str = "Ativando a aceleração pela placa de vídeo (~1,7 GB)…";

/// A aceleração está completa? Com placa NVIDIA, exige as bibliotecas CUDA:
/// sem elas o app abre e roda ~17x mais devagar, em CPU, sem erro à vista.
pub fn gpu_libs_ready(python_dir: &Path) -> bool {
    !has_nvidia_gpu()
        || site_packages(python_dir)
            .join("nvidia")
            .join("cublas")
            .join("bin")
            .join("cublasLt64_12.dll")
            .exists()
}

// ----------------------------------------------------------- câmera virtual

/// Registra o driver Unity Capture (o único passo que pede elevação).
///
/// Falha aqui é **degradada, não fatal**: sem o driver o app funciona
/// normalmente, só não publica para Zoom/Discord. Abortar a instalação
/// inteira porque o usuário recusou o UAC seria desproporcional.
pub fn ensure_virtual_camera(rep: &Reporter, app_dir: &Path) -> Result<(), String> {
    let script = app_dir.join("install_virtual_camera.bat");
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
    command.arg("/D").arg("/C").arg(script.as_os_str()).current_dir(app_dir);
    hide_console(&mut command);

    match command.output() {
        Ok(output) if output.status.success() => {
            rep.done(Step::VirtualCamera, "Câmera virtual ativada");
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

// ------------------------------------------------------------------ atalhos

/// Nome do atalho, que é o nome que o usuário vê e procura.
const SHORTCUT_NAME: &str = "Deepfake Studio Live";

/// Onde os atalhos ficam, dito para quem instalou.
pub const SHORTCUT_HINT: &str =
    "Use o atalho \"Deepfake Studio Live\" na Área de Trabalho ou no Menu Iniciar.";

/// Cria (ou recria) os atalhos "Deepfake Studio Live" na Área de Trabalho e
/// no Menu Iniciar. Eles chamam este mesmo executável com `--launch`, que
/// abre o app direto, sem janela de instalação. Idempotente.
///
/// Devolve as pastas onde criou.
pub fn create_app_shortcuts() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("não achei o executável: {e}"))?;
    let exe_dir = exe.parent().ok_or("o executável não tem pasta")?.to_path_buf();

    // O caminho vai por variável de ambiente, nunca interpolado no script:
    // um perfil de usuário com aspas ou `$` no nome quebraria (ou
    // reinterpretaria) a linha de comando do PowerShell.
    let script = r#"
$ErrorActionPreference = 'Stop'
$shell = New-Object -ComObject WScript.Shell
$made = @()
foreach ($dir in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) {
    if (-not $dir) { continue }
    $link = Join-Path $dir ($env:DSL_NAME + '.lnk')
    $s = $shell.CreateShortcut($link)
    $s.TargetPath = $env:DSL_TARGET
    $s.Arguments = '--launch'
    $s.WorkingDirectory = $env:DSL_WORKDIR
    $s.IconLocation = $env:DSL_TARGET + ',0'
    $s.Description = 'Troca de rosto ao vivo com câmera virtual'
    $s.Save()
    $made += $dir
}
$made -join "`n"
"#;

    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("DSL_NAME", SHORTCUT_NAME)
        .env("DSL_TARGET", &exe)
        .env("DSL_WORKDIR", &exe_dir);
    hide_console(&mut command);

    let output = command.output().map_err(|e| format!("não consegui rodar o PowerShell: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "falha ao criar os atalhos: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// No Windows o atalho chama este executável com `--launch`; não há outra
/// forma de ele ser invocado como launcher.
pub fn is_launcher_invocation() -> bool {
    false
}

/// No Windows não há bundle de launcher para abrir: quem chama abre o app
/// diretamente (ver main::open_app).
pub fn open_launcher_bundle() -> Result<bool, String> {
    Ok(false)
}

// -------------------------------------------------------------------- misc

/// Espaço livre no volume que contém `path` (que precisa existir).
pub fn free_disk_bytes(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
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

/// Abre um arquivo no programa padrão do sistema.
///
/// Pelo shell do Windows em vez de `cmd /C start`: o cmd reinterpreta &, ^,
/// % e aspas no que recebe, e um perfil de usuário com um desses caracteres
/// bastaria para o comando fazer outra coisa. ShellExecuteW recebe o
/// caminho como argumento, sem passar por parser de linha de comando.
pub fn open_path(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW devolve > 32 em sucesso; abaixo disso é código de erro.
    if (result as isize) <= 32 {
        return Err(format!("não consegui abrir {}", path.display()));
    }
    Ok(())
}

/// Caixa de erro — no modo launcher não há UI nem console para mostrá-lo.
pub fn show_error(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (title, text) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

/// No build de release o executável é de subsistema GUI e nasce sem
/// console; o modo `--install-headless` se pendura no console de quem o
/// chamou para que a saída apareça no terminal.
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

// ------------------------------------------------------------ utilitários

fn unzip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("não consegui abrir {}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip inválido: {e}"))?;
    std::fs::create_dir_all(dest).map_err(|e| format!("não consegui criar {}: {e}", dest.display()))?;
    zip.extract(dest).map_err(|e| format!("falha ao extrair: {e}"))?;
    Ok(())
}

/// Extrai só as entradas de um zip cujo caminho comece por um dos prefixos
/// em `mappings`, remapeando cada prefixo para uma pasta de destino. Usado
/// para tirar `tools/include/` e `tools/libs/` de dentro do .nupkg do Python
/// sem extrair o resto.
fn unzip_subdirs(archive: &Path, dest: &Path, mappings: &[(&str, &str)]) -> Result<(), String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("não consegui abrir {}: {e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("zip inválido: {e}"))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("entrada inválida no zip: {e}"))?;
        let name = entry.name().replace('\\', "/");

        let Some((prefix, target_root)) = mappings
            .iter()
            .find(|(prefix, _)| name.starts_with(&format!("{prefix}/")))
        else {
            continue;
        };
        let relative = &name[prefix.len() + 1..];
        if relative.is_empty() {
            continue;
        }
        // O nome vem do zip sem qualquer sanitização; uma entrada como
        // "tools/include/../../../Windows/evil.dll" escreveria fora de dest
        // se não for barrada aqui.
        if Path::new(relative).components().any(|c| matches!(c, std::path::Component::ParentDir)) {
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

#[cfg(test)]
mod pth_tests {
    use super::fix_pth_restrictions;

    /// O `._pth` real do embeddable usa CRLF. Estado de partida = o que
    /// versões antigas do instalador deixaram (só `import site`, sem o
    /// caminho do app) — é exatamente o que a VM tinha.
    const OLD_STATE: &str = "python312.zip\r\n.\r\n\r\n# Uncomment to run site.main() automatically\r\nimport site\r\n";

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
