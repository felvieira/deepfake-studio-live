#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Instalador do Deepfake Studio Live.
//!
//! Instala em %LOCALAPPDATA%\DeepLiveCam: Python embeddable, venv com as
//! dependências, o código do app, os modelos ONNX e o driver da câmera
//! virtual. Depois abre o app.
//!
//! Dois cuidados que não são óbvios e estão documentados onde importam:
//! o venv precisa ficar dentro de app/ (paths::venv_dir) e a queda para CPU
//! do onnxruntime é silenciosa (launch::detect_execution_provider).

mod download;
mod launch;
mod models;
mod paths;
mod progress;
mod steps;

use progress::{Reporter, Step, StepState, StepUpdate};
use serde::Serialize;
use std::path::PathBuf;
use tauri::AppHandle;

#[derive(Serialize)]
struct InstallInfo {
    installed: bool,
    root: String,
    /// Bytes que a instalação precisa, para a UI avisar antes de começar.
    required_bytes: u64,
    free_bytes: Option<u64>,
    models_bytes: u64,
}

#[tauri::command]
fn install_info() -> Result<InstallInfo, String> {
    let root = paths::root()?;
    let installed = paths::venv_python()?.exists() && paths::app_dir()?.join("run.py").exists();
    Ok(InstallInfo {
        installed,
        root: root.to_string_lossy().to_string(),
        required_bytes: steps::required_bytes(),
        free_bytes: steps::free_disk_bytes().ok(),
        models_bytes: models::total_bytes(),
    })
}

/// De onde sai o código do app.
///
/// Hoje: a cópia do repositório ao lado do instalador, o que serve para uso
/// próprio e para desenvolvimento. Quando for distribuir, este é o ponto em
/// que se troca por um download do Release de
/// github.com/felvieira/deepfake-studio-live — repo público, logo sem token.
fn source_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Em dev o binário fica em installer/src-tauri/target/debug/.
    let mut candidate = exe.parent().map(|p| p.to_path_buf());
    while let Some(dir) = candidate {
        if dir.join("run.py").exists() && dir.join("requirements.txt").exists() {
            return Ok(dir);
        }
        candidate = dir.parent().map(|p| p.to_path_buf());
    }
    Err("não encontrei o código do aplicativo para instalar".into())
}

#[tauri::command]
async fn run_install(app: AppHandle) -> Result<String, String> {
    let rep = Reporter::new(app.clone());
    rep.log("=== instalação iniciada ===");

    // Espaço em disco ANTES de baixar qualquer byte.
    if let Ok(free) = steps::free_disk_bytes() {
        let needed = steps::required_bytes();
        if free < needed {
            let msg = format!(
                "Espaço insuficiente: {:.1} GB livres, {:.1} GB necessários",
                free as f64 / 1e9,
                needed as f64 / 1e9
            );
            rep.failed(Step::Python, &msg);
            return Err(msg);
        }
    }

    std::fs::create_dir_all(paths::root()?).map_err(|e| e.to_string())?;

    let client = reqwest::Client::builder()
        .user_agent("DeepfakeStudioLive-Installer")
        .build()
        .map_err(|e| format!("não consegui iniciar o cliente HTTP: {e}"))?;

    // 1. Python
    if let Err(e) = steps::ensure_python(&client, &rep).await {
        rep.failed(Step::Python, &e);
        return Err(e);
    }

    // 2. Código do app — antes das dependências, porque o venv mora dentro
    //    de app/ e o requirements.txt vem daqui.
    //
    //    Rodando de dentro do repositório (desenvolvimento), copia dali.
    //    Numa máquina limpa não há repositório, e aí baixa o Release — que
    //    é público, então não precisa de token.
    let result = match source_dir() {
        Ok(source) => {
            rep.log(&format!("usando código local de {}", source.display()));
            steps::ensure_app_code(&rep, &source)
        }
        Err(_) => {
            rep.log("sem código local; baixando do Release");
            steps::fetch_app_code(&client, &rep).await
        }
    };
    if let Err(e) = result {
        rep.failed(Step::AppCode, &e);
        return Err(e);
    }

    // 3. Dependências
    if let Err(e) = steps::ensure_dependencies(&rep) {
        rep.failed(Step::Dependencies, &e);
        return Err(e);
    }

    // 4. Modelos
    if let Err(e) = steps::ensure_models(&client, &rep).await {
        rep.failed(Step::Models, &e);
        return Err(e);
    }

    // 5. Câmera virtual — degradada em vez de fatal.
    steps::ensure_virtual_camera(&rep)?;

    // Checa o acelerador: a queda para CPU não dá erro, então sem isto uma
    // instalação com CUDA quebrado parece perfeita e só se revela na
    // lentidão.
    let accelerator = match launch::detect_execution_provider() {
        Ok(providers) => {
            rep.log(&format!("providers disponíveis: {providers}"));
            if providers.contains("CUDAExecutionProvider") {
                "CUDA".to_string()
            } else if providers.contains("DmlExecutionProvider") {
                "DirectML".to_string()
            } else {
                "CPU".to_string()
            }
        }
        Err(e) => {
            rep.log(&format!("não consegui checar o acelerador: {e}"));
            "desconhecido".to_string()
        }
    };

    rep.log("=== instalação concluída ===");
    Ok(accelerator)
}

#[tauri::command]
fn open_app() -> Result<u32, String> {
    launch::launch_app()
}

#[tauri::command]
fn open_log() -> Result<(), String> {
    let path = paths::install_log()?;
    if !path.exists() {
        return Err("ainda não há log de instalação".into());
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .spawn()
        .map_err(|e| format!("não consegui abrir o log: {e}"))?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            install_info,
            run_install,
            open_app,
            open_log
        ])
        .setup(|app| {
            // Estado inicial da lista de passos, para a UI não começar vazia.
            let rep = Reporter::new(app.handle().clone());
            for step in [
                Step::Python,
                Step::Dependencies,
                Step::AppCode,
                Step::Models,
                Step::VirtualCamera,
            ] {
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
