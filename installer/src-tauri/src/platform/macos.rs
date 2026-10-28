//! macOS: Python portátil (python-build-standalone), ferramentas de linha de
//! comando da Apple, aceleração pelo CoreML, câmera virtual do OBS e um
//! atalho "Deepfake Studio Live.app" em ~/Applications.

use crate::download::{download_resumable, DownloadSpec};
use crate::progress::{Reporter, Step};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// No macOS processos filhos não abrem janela de terminal; nada a fazer.
pub fn hide_console(_command: &mut Command) {}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME não está definido — ambiente macOS inesperado".to_string())
}

/// Raiz da instalação: ~/Library/Application Support/DeepLiveCam — o lugar
/// do macOS para dados de app do usuário, gravável sem senha de admin.
pub fn default_root() -> Result<PathBuf, String> {
    Ok(home()?.join("Library").join("Application Support").join("DeepLiveCam"))
}

// ------------------------------------------------------------------ Python

/// O macOS não tem um "Python embeddable" oficial. O python-build-standalone
/// (o mesmo que o `uv` usa) é um Python completo e relocável — já traz pip,
/// venv e os cabeçalhos para compilar extensões —, num tar.gz por
/// arquitetura. Release e arquivo fixos, conferidos por SHA256 (os valores
/// vêm do SHA256SUMS publicado no próprio release).
pub const PYTHON_LABEL: &str = "Python 3.12.15 (python-build-standalone 20261003)";
const PYTHON_MINOR: &str = "python3.12";
const PYTHON_ARM64_URL: &str = "https://github.com/astral-sh/python-build-standalone/releases/download/20261003/cpython-3.12.15%2B20261003-aarch64-apple-darwin-install_only.tar.gz";
const PYTHON_ARM64_SHA256: &str = "316a463172740e71d8dca1f2730784e325f3f720941137b5d674d5801a632213";
const PYTHON_X64_URL: &str = "https://github.com/astral-sh/python-build-standalone/releases/download/20261003/cpython-3.12.15%2B20261003-x86_64-apple-darwin-install_only.tar.gz";
const PYTHON_X64_SHA256: &str = "a8fd7a91852f19b6d959793ef41fad048631ccb2a334a9ecdf573255298f7978";

pub fn python_executable(python_dir: &Path) -> PathBuf {
    python_dir.join("bin").join("python3")
}

pub fn site_packages(python_dir: &Path) -> PathBuf {
    python_dir.join("lib").join(PYTHON_MINOR).join("site-packages")
}

/// Baixa e extrai o Python. Só roda quando o executável ainda não existe.
pub async fn install_python(
    client: &reqwest::Client,
    rep: &Reporter,
    python_dir: &Path,
) -> Result<(), String> {
    let (url, sha256) = if cfg!(target_arch = "aarch64") {
        (PYTHON_ARM64_URL, PYTHON_ARM64_SHA256)
    } else {
        (PYTHON_X64_URL, PYTHON_X64_SHA256)
    };
    let archive = crate::paths::root()?.join("python-standalone.tar.gz");
    download_resumable(
        client,
        &DownloadSpec { url, target: &archive, expected_size: None, sha256: Some(sha256) },
        |done, total| {
            rep.progress(Step::Python, "Baixando os arquivos do ambiente…", done, total);
        },
    )
    .await?;

    rep.running(Step::Python, "Preparando o ambiente…");
    // O tar.gz vem com tudo dentro de `python/`; sem esse nível, cai direto
    // em python_dir.
    std::fs::create_dir_all(python_dir)
        .map_err(|e| format!("não consegui criar {}: {e}", python_dir.display()))?;
    crate::steps::untar_strip_root(&archive, python_dir)?;
    let _ = std::fs::remove_file(&archive);

    // Garante o bit de execução (o tar normalmente o preserva; isto cobre
    // extratores que não preservam).
    if let Ok(entries) = std::fs::read_dir(python_dir.join("bin")) {
        use std::os::unix::fs::PermissionsExt;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if meta.is_file() {
                    let mut perms = meta.permissions();
                    perms.set_mode(perms.mode() | 0o755);
                    let _ = std::fs::set_permissions(&path, perms);
                }
            }
        }
    }
    Ok(())
}

/// Nada a corrigir: o Python standalone já vem completo.
pub async fn repair_python(
    _client: &reqwest::Client,
    _rep: &Reporter,
    _python_dir: &Path,
    _app_dir: &Path,
) -> Result<(), String> {
    Ok(())
}

pub fn prepare_launch(_python_dir: &Path, _app_dir: &Path) -> Result<(), String> {
    Ok(())
}

// --------------------------------------------------------------- pip e build

/// O Python standalone já traz o pip; `ensurepip` cobre o caso de ele ter
/// sido removido.
pub async fn ensure_pip(_client: &reqwest::Client, rep: &Reporter, python: &Path) -> Result<(), String> {
    let ok = Command::new(python)
        .args(["-m", "pip", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        return Ok(());
    }
    rep.running(Step::Dependencies, "Preparando a instalação dos componentes…");
    crate::steps::run_checked(
        Command::new(python).args(["-m", "ensurepip", "--upgrade"]),
        "instalar o pip",
    )
}

/// `insightface==0.7.3` só existe como código-fonte e compila uma extensão
/// em C; no Mac o compilador vem das Ferramentas de Linha de Comando do
/// Xcode. Se faltarem, `xcode-select --install` abre o instalador da própria
/// Apple (pede confirmação numa janela do sistema) e aqui se espera ele
/// terminar.
pub async fn ensure_build_tools(_client: &reqwest::Client, rep: &Reporter) -> Result<(), String> {
    if command_line_tools_present() {
        rep.log("ferramentas de linha de comando da Apple já presentes");
        return Ok(());
    }

    rep.running(
        Step::Dependencies,
        "Instalando ferramentas da Apple necessárias — confirme na janela que abriu (pode levar alguns minutos)…",
    );
    let _ = Command::new("xcode-select").arg("--install").output();

    // A instalação acontece fora deste processo; espera aparecer.
    let deadline = Instant::now() + Duration::from_secs(45 * 60);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if command_line_tools_present() {
            rep.log("ferramentas de linha de comando da Apple instaladas");
            return Ok(());
        }
    }
    Err("as Ferramentas de Linha de Comando do Xcode não foram instaladas. \
         Rode `xcode-select --install` no Terminal, conclua a instalação e tente de novo."
        .into())
}

/// Há um clang utilizável? Só olha arquivos: chamar `clang`/`xcrun` sem as
/// ferramentas instaladas abre o diálogo de instalação da Apple sozinho.
fn command_line_tools_present() -> bool {
    let Ok(output) = Command::new("xcode-select").arg("-p").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let developer_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    developer_dir.join("usr/bin/clang").exists()
        || developer_dir
            .join("Toolchains/XcodeDefault.xctoolchain/usr/bin/clang")
            .exists()
}

/// No Mac o compilador já está no PATH quando as ferramentas existem.
pub fn pip_command(python: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(python);
    command.args(args);
    command
}

// -------------------------------------------------------------- aceleração

/// A aceleração no Mac (CoreML: GPU e Neural Engine) já vem no pacote
/// onnxruntime para macOS; não há bibliotecas extras como o CUDA.
pub fn gpu_packages() -> &'static [&'static str] {
    &[]
}

pub const GPU_PACKAGES_MESSAGE: &str = "";

pub fn gpu_libs_ready(_python_dir: &Path) -> bool {
    true
}

// ----------------------------------------------------------- câmera virtual

/// No macOS a câmera virtual é a do OBS Studio: o app manda o vídeo para
/// ela, e Zoom/Teams/Discord a enxergam como "OBS Virtual Camera". Instalar
/// a extensão de câmera exige a aprovação da própria pessoa em Ajustes do
/// Sistema, então não dá para fazer silenciosamente como no Windows: aqui só
/// se verifica e se explica. Sem OBS o app funciona normalmente.
pub fn ensure_virtual_camera(rep: &Reporter, _app_dir: &Path) -> Result<(), String> {
    let candidates = [
        PathBuf::from("/Applications/OBS.app"),
        home().map(|h| h.join("Applications").join("OBS.app")).unwrap_or_default(),
    ];
    if candidates.iter().any(|p| p.exists()) {
        rep.done(
            Step::VirtualCamera,
            "OBS encontrado. Na primeira vez, ative a câmera virtual uma vez dentro do OBS.",
        );
    } else {
        rep.degraded(
            Step::VirtualCamera,
            "Para enviar o vídeo ao Zoom, Teams ou Discord no Mac, instale o OBS Studio \
             (gratuito, obsproject.com) e ative a câmera virtual uma vez nele. \
             O app funciona normalmente sem isso.",
        );
    }
    Ok(())
}

// ------------------------------------------------------------------ atalhos

/// Nome do app de atalho e do executável dentro dele.
const LAUNCHER_NAME: &str = "Deepfake Studio Live";
const LAUNCHER_BUNDLE_ID: &str = "com.felvieira.deepfake-studio-live.launcher";

pub const SHORTCUT_HINT: &str =
    "Abra o \"Deepfake Studio Live\" pela pasta Aplicativos, pelo Launchpad ou pelo Spotlight.";

fn launcher_bundle() -> Result<PathBuf, String> {
    Ok(home()?.join("Applications").join(format!("{LAUNCHER_NAME}.app")))
}

/// Cria (ou atualiza) ~/Applications/Deepfake Studio Live.app.
///
/// O executável do bundle é uma cópia deste próprio binário, com o nome do
/// app; ao se ver rodando com esse nome ele entra no modo launcher (ver
/// is_launcher_invocation) em vez de abrir o instalador. Ser um executável
/// de verdade dentro de um bundle — e não um script — é o que faz o macOS
/// atribuir a ele (e ao Python que ele abre) o pedido de acesso à câmera,
/// usando o texto de NSCameraUsageDescription do Info.plist. LSUIElement o
/// mantém fora do Dock: quem aparece é a janela do app.
///
/// Recriado a cada instalação, então uma versão nova do instalador também
/// atualiza o launcher.
pub fn create_app_shortcuts() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("não achei o executável: {e}"))?;
    let bundle = launcher_bundle()?;
    let contents = bundle.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources = contents.join("Resources");
    for dir in [&macos_dir, &resources] {
        std::fs::create_dir_all(dir).map_err(|e| format!("não consegui criar {}: {e}", dir.display()))?;
    }

    let launcher = macos_dir.join(LAUNCHER_NAME);
    let _ = std::fs::remove_file(&launcher);
    std::fs::copy(&exe, &launcher)
        .map_err(|e| format!("não consegui copiar o launcher: {e}"))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755));
    }

    // Ícone: o mesmo do instalador, que o bundle do Tauri guarda em
    // Contents/Resources/icon.icns.
    let mut has_icon = false;
    if let Some(icns) = exe
        .parent()
        .and_then(|macos| macos.parent())
        .map(|contents| contents.join("Resources").join("icon.icns"))
    {
        if icns.exists() && std::fs::copy(&icns, resources.join("AppIcon.icns")).is_ok() {
            has_icon = true;
        }
    }

    let icon_entry = if has_icon {
        "    <key>CFBundleIconFile</key>\n    <string>AppIcon</string>\n"
    } else {
        ""
    };
    let version = env!("CARGO_PKG_VERSION");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{LAUNCHER_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>{LAUNCHER_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>{LAUNCHER_BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>
    <string>{LAUNCHER_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
{icon_entry}    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.video</string>
    <key>NSCameraUsageDescription</key>
    <string>O Deepfake Studio Live usa a câmera para a troca de rosto ao vivo.</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
"#
    );
    std::fs::write(contents.join("Info.plist"), plist)
        .map_err(|e| format!("não consegui escrever o Info.plist: {e}"))?;

    // Assinatura ad-hoc do bundle: o executável copiado veio de outro bundle,
    // e sem reassinar o macOS veria a assinatura apontando para o Info.plist
    // errado. "-" = ad-hoc (não exige conta de desenvolvedor).
    let _ = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(&bundle)
        .output();
    // Avisa o Finder/Launchpad de que o app mudou.
    let _ = Command::new("touch").arg(&bundle).output();

    Ok(vec![bundle
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default()])
}

/// Este processo é o launcher (o executável dentro do atalho)?
pub fn is_launcher_invocation() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_name().map(|n| n == LAUNCHER_NAME))
        .unwrap_or(false)
}

/// Abre o app pelo atalho, para que a câmera seja pedida em nome dele (ver
/// create_app_shortcuts). `Ok(false)` se o atalho ainda não existe.
pub fn open_launcher_bundle() -> Result<bool, String> {
    let bundle = launcher_bundle()?;
    if !bundle.exists() {
        return Ok(false);
    }
    let status = Command::new("open")
        .arg("-n")
        .arg(&bundle)
        .status()
        .map_err(|e| format!("não consegui abrir {}: {e}", bundle.display()))?;
    if !status.success() {
        return Err(format!("o macOS não abriu {}", bundle.display()));
    }
    Ok(true)
}

// -------------------------------------------------------------------- misc

/// Espaço livre no volume que contém `path` (que precisa existir).
pub fn free_disk_bytes(path: &Path) -> Result<u64, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "caminho inválido".to_string())?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return Err("não consegui medir o espaço livre em disco".into());
    }
    #[allow(clippy::unnecessary_cast)]
    Ok(stat.f_bavail as u64 * stat.f_frsize as u64)
}

/// Abre um arquivo no programa padrão (`open` recebe o caminho como
/// argumento, sem shell no meio).
pub fn open_path(path: &Path) -> Result<(), String> {
    let status = Command::new("open")
        .arg(path)
        .status()
        .map_err(|e| format!("não consegui abrir {}: {e}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("não consegui abrir {}", path.display()))
    }
}

/// Alerta do sistema. Título e texto vão como argumentos do script, nunca
/// interpolados nele — aspas ou barras na mensagem não mudam o que roda.
pub fn show_error(title: &str, text: &str) {
    let _ = Command::new("osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "display alert (item 1 of argv) message (item 2 of argv) as critical",
            "-e",
            "end run",
            title,
            text,
        ])
        .output();
}

/// No macOS o processo já herda o terminal de quem o chamou.
pub fn attach_parent_console() {}
