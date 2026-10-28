//! A instalação de ponta a ponta — usada pela janela do instalador e pelo
//! modo `--install-headless` (CI e linha de comando), exatamente igual nos
//! dois.

use crate::launch;
use crate::models;
use crate::paths;
use crate::platform;
use crate::progress::{Reporter, Step};
use crate::steps;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// O que a UI recebe quando a instalação termina.
#[derive(Serialize)]
pub struct InstallResult {
    /// "CUDA", "CoreML", "DirectML", "CPU" ou "desconhecido".
    pub accelerator: String,
    /// Onde os atalhos foram criados (vazio se falhou).
    pub shortcuts: Vec<String>,
    /// Frase dizendo onde achar o app (vazia se os atalhos falharam).
    pub shortcut_hint: String,
}

#[derive(Serialize)]
pub struct InstallInfo {
    pub installed: bool,
    pub root: String,
    /// "windows" ou "macos", para a UI ajustar textos.
    pub os: &'static str,
    /// Bytes que a instalação precisa, para a UI avisar antes de começar.
    pub required_bytes: u64,
    pub free_bytes: Option<u64>,
    pub models_bytes: u64,
}

/// Estado atual da instalação.
///
/// O Python existir só prova que o passo 1 rodou. "Instalado" exige um
/// pacote que só aparece depois do `pip install -r` terminar, todos os
/// modelos completos (sem eles o app abre e trava no primeiro "Start live")
/// e, com placa NVIDIA, as bibliotecas CUDA (sem elas o app roda ~17x mais
/// devagar, em CPU, sem erro à vista).
pub fn info() -> Result<InstallInfo, String> {
    let root = paths::root()?;
    let python_dir = paths::python_dir()?;
    let deps_marker = platform::site_packages(&python_dir).join("onnxruntime");
    let installed = deps_marker.exists()
        && paths::app_dir()?.join("run.py").exists()
        && models::all_present(&paths::models_dir()?)
        && platform::gpu_libs_ready(&python_dir);
    Ok(InstallInfo {
        installed,
        root: root.to_string_lossy().to_string(),
        os: if cfg!(target_os = "macos") { "macos" } else { "windows" },
        required_bytes: steps::required_bytes(),
        free_bytes: steps::free_disk_bytes().ok(),
        models_bytes: models::total_bytes(),
    })
}

/// Mensagem de falha para a tela. O erro técnico (saída do pip, códigos do
/// sistema…) só faz sentido para suporte, então vai inteiro para o log e a
/// tela diz em qual parte deu errado e onde ver os detalhes.
fn friendly_failure(rep: &Reporter, step: Step, technical: &str) -> String {
    rep.log(&format!("[{}] erro técnico: {technical}", step.label()));
    let part = match step {
        Step::Python => "configurar o ambiente",
        Step::AppCode => "instalar os arquivos do aplicativo",
        Step::Dependencies => "instalar os componentes",
        Step::Models => "baixar os modelos de IA",
        Step::VirtualCamera => "ativar a câmera virtual",
    };
    let network = ["timed out", "dns", "connection", "conex", "network", "rede", "http"]
        .iter()
        .any(|k| technical.to_lowercase().contains(k));
    let hint = if network {
        " Verifique a conexão com a internet e clique em Tentar de novo."
    } else {
        " Clique em Tentar de novo; o que já foi baixado é aproveitado."
    };
    format!("Não foi possível {part}.{hint} Detalhes em Ver log.")
}

fn fail(rep: &Reporter, step: Step, technical: String) -> String {
    let msg = friendly_failure(rep, step, &technical);
    rep.failed(step, &msg);
    msg
}

pub async fn run(rep: &Reporter) -> Result<InstallResult, String> {
    rep.log(&format!(
        "=== instalação iniciada (instalador {}, {}) ===",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    ));

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

    // 1. Ambiente (Python)
    if let Err(e) = steps::ensure_python(&client, rep).await {
        return Err(fail(rep, Step::Python, e));
    }

    // 2. Código do app — antes das dependências, porque o requirements.txt
    //    vem daqui. Rodando de dentro do repositório (desenvolvimento, CI),
    //    copia dali; numa máquina limpa baixa o Release, que é público.
    let result = match source_dir() {
        Ok(source) => {
            rep.log(&format!("usando código local de {}", source.display()));
            steps::ensure_app_code(rep, &source)
        }
        Err(_) => {
            rep.log("sem código local; baixando do Release");
            steps::fetch_app_code(&client, rep).await
        }
    };
    if let Err(e) = result {
        return Err(fail(rep, Step::AppCode, e));
    }

    // 3. Componentes (dependências)
    if let Err(e) = steps::ensure_dependencies(&client, rep).await {
        return Err(fail(rep, Step::Dependencies, e));
    }

    // 4. Modelos
    if let Err(e) = steps::ensure_models(&client, rep).await {
        return Err(fail(rep, Step::Models, e));
    }

    // 5. Câmera virtual — degradada em vez de fatal.
    steps::ensure_virtual_camera(rep)?;

    // A queda para CPU não dá erro; sem checar, uma instalação com a
    // aceleração quebrada parece perfeita e só se revela na lentidão.
    let accelerator = match launch::detect_execution_provider() {
        Ok(providers) => {
            rep.log(&format!("providers disponíveis: {providers}"));
            launch::accelerator_name(&providers).to_string()
        }
        Err(e) => {
            rep.log(&format!("não consegui checar o acelerador: {e}"));
            "desconhecido".to_string()
        }
    };

    // Atalhos que abrem o app direto. Falhar aqui não invalida a instalação:
    // o app funciona, só o caminho até ele é menos conveniente.
    let (shortcuts, shortcut_hint) = match platform::create_app_shortcuts() {
        Ok(dirs) => {
            rep.log(&format!("atalhos criados em: {}", dirs.join(", ")));
            (dirs, platform::SHORTCUT_HINT.to_string())
        }
        Err(e) => {
            rep.log(&format!("não consegui criar os atalhos: {e}"));
            (Vec::new(), String::new())
        }
    };

    rep.log(&format!("=== instalação concluída (acelerador: {accelerator}) ==="));
    Ok(InstallResult { accelerator, shortcuts, shortcut_hint })
}

/// De onde sai o código do app.
///
/// Procura um checkout subindo a partir do executável: rodando de dentro do
/// repositório (desenvolvimento, CI), instala o código local, o que evita
/// baixar de novo o que já está no disco e testa mudanças ainda não
/// publicadas. Numa máquina limpa nada é encontrado e quem assume é
/// steps::fetch_app_code, que baixa o Release.
fn source_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    find_checkout(exe.parent())
}

/// Separada de source_dir() para poder ser testada sem depender de onde o
/// binário de teste está.
fn find_checkout(start: Option<&Path>) -> Result<PathBuf, String> {
    let mut candidate = start.map(|p| p.to_path_buf());
    while let Some(dir) = candidate {
        if dir.join("run.py").exists() && dir.join("requirements.txt").exists() {
            return Ok(dir);
        }
        candidate = dir.parent().map(|p| p.to_path_buf());
    }
    Err("não encontrei o código do aplicativo para instalar".into())
}

#[cfg(test)]
mod tests {
    use super::find_checkout;

    /// Um executável dentro do repositório acha o checkout subindo os
    /// diretórios — é o que faz o instalador usar o código local em vez de
    /// rebaixar o Release.
    #[test]
    fn finds_checkout_from_nested_dir() {
        let tmp = std::env::temp_dir().join(format!("dlc-test-checkout-{}", std::process::id()));
        let nested = tmp.join("installer").join("target").join("release");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(tmp.join("run.py"), "").unwrap();
        std::fs::write(tmp.join("requirements.txt"), "").unwrap();

        let found = find_checkout(Some(&nested)).expect("deveria achar o checkout");
        assert_eq!(found, tmp);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Fora do repositório não acha nada, e o instalador cai para o Release.
    #[test]
    fn no_checkout_outside_repo() {
        let tmp = std::env::temp_dir().join(format!("dlc-test-empty-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(find_checkout(Some(&tmp)).is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// run.py sozinho não basta: sem requirements.txt o passo das
    /// dependências falharia depois.
    #[test]
    fn partial_checkout_is_not_enough() {
        let tmp = std::env::temp_dir().join(format!("dlc-test-partial-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("run.py"), "").unwrap();
        assert!(find_checkout(Some(&tmp)).is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
