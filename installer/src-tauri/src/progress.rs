//! Progresso e log.
//!
//! Cada passo emite eventos para o frontend e grava a mesma coisa em
//! logs/install.log. O log importa: quando uma instalação falha na máquina de
//! outra pessoa, é a única evidência do que aconteceu.

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use tauri::{AppHandle, Emitter};

/// Em qual dos cinco passos estamos. O frontend usa isto para marcar a lista.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Python,
    Dependencies,
    AppCode,
    Models,
    VirtualCamera,
}

impl Step {
    pub fn label(self) -> &'static str {
        match self {
            Step::Python => "Python",
            Step::Dependencies => "Dependências",
            Step::AppCode => "Aplicativo",
            Step::Models => "Modelos de IA",
            Step::VirtualCamera => "Câmera virtual",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    Pending,
    Running,
    Done,
    /// Falhou, mas o app continua utilizável. Hoje só a câmera virtual:
    /// sem ela não há saída para Zoom/Discord, mas o resto funciona.
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepUpdate {
    pub step: Step,
    pub state: StepState,
    /// O que está acontecendo agora, em linguagem de gente.
    pub message: String,
    /// 0.0–1.0 quando o passo sabe medir (download), None quando não sabe.
    pub fraction: Option<f64>,
    /// Bytes baixados / total, para mostrar "412 MB de 958 MB".
    pub bytes: Option<(u64, u64)>,
}

#[derive(Clone)]
pub struct Reporter {
    app: AppHandle,
}

impl Reporter {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    pub fn update(&self, update: StepUpdate) {
        self.log(&format!(
            "[{}] {:?}: {}",
            update.step.label(),
            update.state,
            update.message
        ));
        // Um erro de emit significa que a janela sumiu; a instalação
        // continua e o log guarda o resto.
        let _ = self.app.emit("install:step", &update);
    }

    pub fn running(&self, step: Step, message: impl Into<String>) {
        self.update(StepUpdate {
            step,
            state: StepState::Running,
            message: message.into(),
            fraction: None,
            bytes: None,
        });
    }

    pub fn progress(&self, step: Step, message: impl Into<String>, done: u64, total: u64) {
        let fraction = if total > 0 {
            Some((done as f64 / total as f64).clamp(0.0, 1.0))
        } else {
            None
        };
        self.update(StepUpdate {
            step,
            state: StepState::Running,
            message: message.into(),
            fraction,
            bytes: Some((done, total)),
        });
    }

    pub fn done(&self, step: Step, message: impl Into<String>) {
        self.update(StepUpdate {
            step,
            state: StepState::Done,
            message: message.into(),
            fraction: Some(1.0),
            bytes: None,
        });
    }

    pub fn degraded(&self, step: Step, message: impl Into<String>) {
        self.update(StepUpdate {
            step,
            state: StepState::Degraded,
            message: message.into(),
            fraction: None,
            bytes: None,
        });
    }

    pub fn failed(&self, step: Step, message: impl Into<String>) {
        self.update(StepUpdate {
            step,
            state: StepState::Failed,
            message: message.into(),
            fraction: None,
            bytes: None,
        });
    }

    /// Append no install.log. Silencioso em erro de I/O de propósito: não
    /// conseguir escrever o log nunca deve derrubar a instalação.
    pub fn log(&self, line: &str) {
        let Ok(path) = crate::paths::install_log() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "{stamp} {line}");
        }
    }
}
