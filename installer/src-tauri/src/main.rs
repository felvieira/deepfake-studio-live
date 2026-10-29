#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Instalador do Deepfake Studio Live (Windows e macOS).
//!
//! Instala na pasta de dados do usuário (ver paths): um Python próprio com
//! as dependências instaladas nele direto, o código do app, os modelos ONNX
//! e a câmera virtual. Depois abre o app.
//!
//! O mesmo executável tem três papéis:
//! - sem argumentos: a janela de instalação;
//! - `--launch` (ou, no macOS, rodando como o atalho "Deepfake Studio
//!   Live.app"): abre o app direto, sem janela de instalação;
//! - `--install-headless`: instala pelo terminal, sem janela (CI e linha de
//!   comando). Sai com código 0 em sucesso.

mod download;
mod install;
mod launch;
mod models;
mod paths;
mod platform;
mod progress;
mod steps;

use progress::{Reporter, Step, StepState, StepUpdate};
use tauri::AppHandle;

#[tauri::command]
fn install_info() -> Result<install::InstallInfo, String> {
    install::info()
}

#[tauri::command]
async fn run_install(app: AppHandle) -> Result<install::InstallResult, String> {
    let rep = Reporter::new(app);
    install::run(&rep).await
}

/// Abre o app a partir da janela do instalador e fecha o instalador — deixar
/// a janela aberta atrás do app só confunde.
///
/// No macOS abre pelo atalho, para o pedido de câmera ser feito em nome dele
/// (ver platform::create_app_shortcuts); o próprio atalho mostra um erro se
/// o app não abrir. Sem atalho (ou no Windows), abre direto e espera a
/// janela aparecer, devolvendo o erro à UI se o app não abrir.
#[tauri::command]
async fn open_app(app: AppHandle) -> Result<(), String> {
    if platform::open_launcher_bundle()? {
        app.exit(0);
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(|| launch::launch_app(launch::AfterReady::Return))
        .await
        .map_err(|e| format!("falha interna ao abrir o app: {e}"))??;
    app.exit(0);
    Ok(())
}

/// Recria os atalhos de uma instalação que já existe. Chamado pela UI quando
/// abre já instalado: quem instalou antes de os atalhos existirem (ou os
/// apagou) os recebe sem reinstalar nada. Devolve a frase de onde achá-los.
#[tauri::command]
fn ensure_shortcuts() -> Result<String, String> {
    platform::create_app_shortcuts().map(|_| platform::SHORTCUT_HINT.to_string())
}

#[tauri::command]
fn open_log() -> Result<(), String> {
    let path = paths::install_log()?;
    if !path.exists() {
        return Err("ainda não há log de instalação".into());
    }
    platform::open_path(&path)
}

/// Modo launcher: abre o app e sai, sem criar nenhuma janela do instalador.
/// Um erro vira um alerta do sistema, porque neste modo não há UI nem
/// console para mostrá-lo.
fn launch_and_exit() -> ! {
    // No macOS o launcher fica vivo enquanto o app roda (ver AfterReady).
    let after_ready = if cfg!(target_os = "macos") {
        launch::AfterReady::WaitForExit
    } else {
        launch::AfterReady::Return
    };
    match launch::launch_app(after_ready) {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            let text = format!(
                "Não consegui abrir o aplicativo.\n\n{e}\n\nAbra o instalador do Deepfake Studio Live e clique em Instalar para reparar."
            );
            // No CI não há ninguém para fechar o alerta: o erro vai para o
            // terminal.
            if std::env::var("DSL_NO_DIALOGS").as_deref() == Ok("1") {
                platform::attach_parent_console();
                eprintln!("LAUNCH FAILED: {text}");
            } else {
                platform::show_error("Deepfake Studio Live", &text);
            }
            std::process::exit(1);
        }
    }
}

/// Modo `--install-headless`: a mesma instalação da janela, com o progresso
/// no terminal. Usado pelo CI para instalar do zero em máquinas limpas.
fn install_headless() -> ! {
    platform::attach_parent_console();
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("não consegui iniciar o runtime: {e}");
            std::process::exit(1);
        }
    };
    let rep = Reporter::terminal();
    match runtime.block_on(install::run(&rep)) {
        Ok(result) => {
            println!("INSTALL OK accelerator={} shortcuts={:?}", result.accelerator, result.shortcuts);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("INSTALL FAILED: {e}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--install-headless") {
        install_headless();
    }
    if args.iter().any(|a| a == "--launch") || platform::is_launcher_invocation() {
        launch_and_exit();
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            install_info,
            run_install,
            open_app,
            ensure_shortcuts,
            open_log
        ])
        .setup(|app| {
            // Estado inicial da lista de passos, para a UI não começar vazia.
            let rep = Reporter::new(app.handle().clone());
            for step in [Step::Python, Step::Dependencies, Step::AppCode, Step::Models, Step::VirtualCamera] {
                rep.update(StepUpdate {
                    step,
                    state: StepState::Pending,
                    message: String::new(),
                    fraction: None,
                    bytes: None,
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o instalador");
}
