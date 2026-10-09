//! Progresso e log.
//!
//! Cada passo emite eventos para o frontend e grava a mesma coisa em
//! logs/install.log. O log importa: quando uma instalação falha na máquina de
//! outra pessoa, é a única evidência do que aconteceu.

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
    throttle: Arc<Mutex<Throttle>>,
}

/// Estado do limitador de eventos. Um download de 300 MB entrega milhares de
/// chunks; emitir e gravar uma linha de log por chunk deixava o install.log
/// com centenas de milhares de caracteres de "Baixando…" repetido e
/// inundava o frontend. Aqui só passa o que muda de forma visível.
struct Throttle {
    last_emit: Instant,
    last_decile: i32,
    last_step: Option<Step>,
}

impl Reporter {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            throttle: Arc::new(Mutex::new(Throttle {
                last_emit: Instant::now() - Duration::from_secs(1),
                last_decile: -1,
                last_step: None,
            })),
        }
    }

    /// Emite sem gravar no log (o chamador decide o que merece uma linha).
    fn emit(&self, update: &StepUpdate) {
        // Um erro de emit significa que a janela sumiu; a instalação
        // continua e o log guarda o resto.
        let _ = self.app.emit("install:step", update);
    }

    /// Uma linha de detalhe do que está acontecendo AGORA dentro de uma
    /// etapa longa (ex.: cada pacote que o pip baixa). Vai inteira para o
    /// log; para a tela, no máximo ~8 por segundo.
    pub fn detail(&self, step: Step, message: impl Into<String>) {
        let message = message.into();
        self.log(&format!("[{}] {}", step.label(), message));
        let mut t = self.throttle.lock().unwrap();
        if t.last_emit.elapsed() < Duration::from_millis(120) {
            return;
        }
        t.last_emit = Instant::now();
        drop(t);
        self.emit(&StepUpdate {
            step,
            state: StepState::Running,
            message,
            fraction: None,
            bytes: None,
        });
    }

    pub fn update(&self, update: StepUpdate) {
        self.log(&format!(
            "[{}] {:?}: {}",
            update.step.label(),
            update.state,
            update.message
        ));
        self.emit(&update);
        // Uma mudança de estado/etapa zera o limitador de progresso.
        if let Ok(mut t) = self.throttle.lock() {
            t.last_decile = -1;
            t.last_step = Some(update.step);
        }
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
        let message = message.into();
        let decile = fraction.map(|f| (f * 10.0) as i32).unwrap_or(-1);

        let mut t = self.throttle.lock().unwrap();
        let finished = total > 0 && done >= total;
        let step_changed = t.last_step != Some(step);
        let decile_changed = decile != t.last_decile;
        // A tela recebe no máximo ~7 atualizações por segundo, mas nunca
        // perde a primeira, a última nem a troca de etapa.
        let emit_now =
            finished || step_changed || t.last_emit.elapsed() >= Duration::from_millis(150);
        if decile_changed || step_changed {
            // O log só ganha uma linha a cada 10%.
            self.log(&format!(
                "[{}] {} ({}%)",
                step.label(),
                message,
                (fraction.unwrap_or(0.0) * 100.0) as i32
            ));
        }
        t.last_decile = decile;
        t.last_step = Some(step);
        if emit_now {
            t.last_emit = Instant::now();
        }
        drop(t);
        if emit_now {
            self.emit(&StepUpdate {
                step,
                state: StepState::Running,
                message,
                fraction,
                bytes: Some((done, total)),
            });
        }
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
