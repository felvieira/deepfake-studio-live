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

/// Não existe venv de verdade: o Python embeddable oficial não inclui o
/// módulo `venv` nem `pip` (é uma distribuição deliberadamente mínima —
/// sem ensurepip, sem tkinter). Rodar `python -m venv` nele falha com
/// "No module named venv" — só se descobre isso testando numa máquina
/// sem outro Python instalado, porque em dev sempre havia um Python do
/// sistema por perto para criar o venv.
///
/// A saída é não precisar de venv: instalamos o pip nesse próprio Python
/// (get-pip.py) e instalamos as dependências direto nele, sem isolar em
/// outra pasta. Ele já é auto-contido dentro de python_dir(), então isso
/// não "suja" nada do sistema — o embeddable inteiro é descartável.
///
/// O motivo de ainda existir esta função, e não simplesmente usar
/// python_dir() direto: run.py:15 também procura DLLs da NVIDIA em
/// `<pasta do run.py>/venv/Lib/site-packages/nvidia/*/bin`. Sem essa
/// pasta a busca simplesmente não encontra nada ali — mas run.py:14
/// também varre `sys.prefix/Lib/site-packages`, que é onde os pacotes
/// deste Python realmente vão parar. Então basta que o interpretador
/// usado seja este aqui; nenhuma pasta venv/ precisa existir.
pub fn venv_python() -> Result<PathBuf, String> {
    Ok(python_dir()?.join("python.exe"))
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

