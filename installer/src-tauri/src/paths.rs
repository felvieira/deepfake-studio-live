//! Onde a instalação vive.
//!
//! Windows: %LOCALAPPDATA%\DeepLiveCam. macOS: ~/Library/Application
//! Support/DeepLiveCam. Os dois são graváveis pelo usuário sem elevação e
//! sobrevivem a atualizações (ver platform::default_root).
//!
//! `DSL_INSTALL_ROOT` troca a raiz — usado pelo CI e por testes para instalar
//! do zero numa pasta descartável sem tocar a instalação de verdade.

use std::path::PathBuf;

pub fn root() -> Result<PathBuf, String> {
    match std::env::var_os("DSL_INSTALL_ROOT") {
        Some(custom) if !custom.is_empty() => Ok(PathBuf::from(custom)),
        _ => crate::platform::default_root(),
    }
}

/// Python próprio da instalação. Não há venv: o Python da instalação já é
/// isolado e descartável, então as dependências vão direto nele. No Windows
/// é o embeddable oficial (sem `venv` nem `pip` de fábrica — ver
/// platform::ensure_pip); no macOS, o python-build-standalone.
pub fn python_dir() -> Result<PathBuf, String> {
    Ok(root()?.join("python"))
}

pub fn python_exe() -> Result<PathBuf, String> {
    Ok(crate::platform::python_executable(&python_dir()?))
}

pub fn app_dir() -> Result<PathBuf, String> {
    Ok(root()?.join("app"))
}

pub fn models_dir() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("models"))
}

pub fn logs_dir() -> Result<PathBuf, String> {
    Ok(root()?.join("logs"))
}

pub fn install_log() -> Result<PathBuf, String> {
    Ok(logs_dir()?.join("install.log"))
}

/// Saída (stdout/stderr) do app da última vez que foi aberto.
pub fn app_log() -> Result<PathBuf, String> {
    Ok(logs_dir()?.join("app.log"))
}

/// Arquivo que o app cria quando a janela aparece (ver launch.rs e
/// modules/ui.py:_signal_ready).
pub fn ready_file() -> Result<PathBuf, String> {
    Ok(logs_dir()?.join("app.ready"))
}
