//! O que muda entre Windows e macOS.
//!
//! O resto do instalador (passos, downloads, progresso, abrir o app) é o
//! mesmo nos dois sistemas e só fala com este módulo para o que é
//! inevitavelmente diferente: de onde vem o Python, como se compila, como a
//! câmera virtual existe, como se cria um atalho, como se abre um arquivo.
//! Os dois arquivos expõem exatamente as mesmas funções.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("o instalador existe para Windows e macOS; no Linux, siga a instalação pelo código-fonte");
