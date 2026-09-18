//! Onde a instalação vive.
//!
//! Tudo fica sob %LOCALAPPDATA%\DeepLiveCam: é gravável pelo usuário sem
//! elevação, sobrevive a atualizações e sai limpo na desinstalação. A única
//! etapa que pede admin é o registro do driver da câmera virtual, que se
//! auto-eleva sozinho.

use std::path::PathBuf;

/// Raiz da instalação. Erra se LOCALAPPDATA não existir — em Windows isso só
/// acontece num ambiente quebrado, e falhar cedo é melhor que instalar num
/// lugar imprevisível.
pub fn root() -> Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA")
        .map_err(|_| "LOCALAPPDATA não está definido — ambiente Windows inesperado".to_string())?;
    Ok(PathBuf::from(base).join("DeepLiveCam"))
}

pub fn python_dir() -> Result<PathBuf, String> {
    Ok(root()?.join("python"))
}

/// O venv vive DENTRO de app/, não ao lado.
///
/// run.py:15 monta o caminho das DLLs da NVIDIA como
/// `<pasta do run.py>/venv/Lib/site-packages/nvidia/*/bin` e passa cada um
/// para os.add_dll_directory(). Com o venv em qualquer outro lugar esse
/// laço não encontra nada, o onnxruntime não carrega cuDNN/cuBLAS e o app
/// cai para CPU **sem erro nenhum** — só fica lento. Mover o venv daqui
/// quebra a aceleração de GPU de um jeito que nenhum teste de instalação
/// pega.
pub fn venv_dir() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("venv"))
}

/// Python do venv. É este que roda o app — nunca o Python do sistema, que
/// pode não ter as dependências nem a versão certa.
pub fn venv_python() -> Result<PathBuf, String> {
    Ok(venv_dir()?.join("Scripts").join("python.exe"))
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

