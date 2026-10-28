//! Abrir o app instalado.
//!
//! O launcher faz o mínimo: roda o Python da instalação com cwd em app/ e
//! deixa o run.py cuidar do resto (inclusive registrar as DLLs do CUDA no
//! Windows — sem isso o onnxruntime cai para CPU em silêncio). O que não pode
//! faltar é o cwd: sem ele o run.py calcula project_root errado.
//!
//! "Aberto" significa a janela ter aparecido: o app grava um arquivo quando
//! mostra a janela principal (DSL_READY_FILE — ver modules/ui.py). Antes,
//! bastava o processo existir depois de 800 ms; um app que morria durante a
//! carga (que leva de 5 a 30 s) ou que desistia de abrir com código 0 ainda
//! era dado como "iniciado", e a pessoa ficava olhando para a tela sem nada.

use std::fs::File;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Até quando esperar a janela. Passado isso, considera aberto — uma máquina
/// lenta na primeira abertura não deve ganhar um erro falso — e para de
/// esperar.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// O que fazer depois que a janela apareceu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AfterReady {
    /// Devolve o controle (o app segue sozinho).
    Return,
    /// Fica esperando o app fechar. No macOS é o launcher do atalho: o
    /// pedido de câmera do Python é feito em nome de quem o abriu, e quem o
    /// abriu precisa continuar vivo para isso valer.
    WaitForExit,
}

pub fn launch_app(after_ready: AfterReady) -> Result<u32, String> {
    let app_dir = crate::paths::app_dir()?;
    let python = crate::paths::python_exe()?;
    let entry = app_dir.join("run.py");

    if !python.exists() {
        return Err(format!(
            "o Python da instalação não está em {} — rode a instalação de novo",
            python.display()
        ));
    }
    if !entry.exists() {
        return Err(format!("run.py não está em {} — instalação incompleta", entry.display()));
    }

    // Ajustes que dependem do sistema (no Windows, o `._pth` do embeddable,
    // reaplicado a cada abertura para cobrir instalações antigas).
    crate::platform::prepare_launch(&crate::paths::python_dir()?, &app_dir)?;

    // stdout/stderr vão para um arquivo: sem console, uma exceção antes de a
    // janela abrir não deixaria rastro nenhum.
    let log_path = crate::paths::app_log()?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("não consegui criar {}: {e}", parent.display()))?;
    }
    let stdout_file = File::create(&log_path)
        .map_err(|e| format!("não consegui criar {}: {e}", log_path.display()))?;
    let stderr_file = stdout_file
        .try_clone()
        .map_err(|e| format!("não consegui preparar o log: {e}"))?;

    let ready = crate::paths::ready_file()?;
    let _ = std::fs::remove_file(&ready);

    let mut command = Command::new(&python);
    command
        .arg(&entry)
        // cwd em app/: run.py deriva project_root do próprio caminho, mas o
        // resto do projeto (switch_states.json, models/) é relativo ao cwd.
        .current_dir(&app_dir)
        .env("DSL_READY_FILE", &ready)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    crate::platform::hide_console(&mut command);

    let mut child = command.spawn().map_err(|e| format!("não consegui iniciar o app: {e}"))?;
    let pid = child.id();

    let started = Instant::now();
    loop {
        if ready.exists() {
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            // Terminou antes de mostrar a janela: é falha, qualquer que seja
            // o código de saída.
            return Err(format!(
                "o aplicativo fechou antes de abrir a janela (código {}).\n{}",
                status.code().map(|c| c.to_string()).unwrap_or_else(|| "desconhecido".into()),
                log_tail(&log_path)
            ));
        }
        if started.elapsed() > READY_TIMEOUT {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    if after_ready == AfterReady::WaitForExit {
        let _ = child.wait();
    }
    Ok(pid)
}

/// As últimas linhas do log do app, para a mensagem de erro.
fn log_tail(path: &std::path::Path) -> String {
    let log = std::fs::read_to_string(path).unwrap_or_default();
    let tail: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).rev().take(20).collect();
    if tail.is_empty() {
        "(sem saída capturada)".to_string()
    } else {
        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
    }
}

/// Pergunta ao app instalado qual execution provider o onnxruntime realmente
/// consegue usar.
///
/// Existe porque a queda para CPU é silenciosa: sem checar, uma instalação
/// com CUDA quebrado parece bem-sucedida e o usuário só percebe pela
/// lentidão. Roda o Python da instalação com o mesmo preâmbulo de DLLs do
/// run.py (sem efeito fora do Windows) e devolve a lista de providers.
pub fn detect_execution_provider() -> Result<String, String> {
    let app_dir = crate::paths::app_dir()?;
    let python = crate::paths::python_exe()?;
    if !python.exists() {
        return Err("instalação não encontrada".into());
    }

    let probe = r#"
import os, sys, json
root = os.getcwd()
for sp in (os.path.join(sys.prefix, "Lib", "site-packages"), os.path.join(root, "venv", "Lib", "site-packages")):
    dirs = []
    torch_lib = os.path.join(sp, "torch", "lib")
    if os.path.isdir(torch_lib):
        dirs.append(torch_lib)
    nvidia = os.path.join(sp, "nvidia")
    if os.path.isdir(nvidia):
        for pkg in os.listdir(nvidia):
            b = os.path.join(nvidia, pkg, "bin")
            if os.path.isdir(b):
                dirs.append(b)
    for d in dirs:
        os.environ["PATH"] = d + os.pathsep + os.environ["PATH"]
        try:
            os.add_dll_directory(d)
        except (OSError, AttributeError):
            pass
try:
    import onnxruntime
    providers = onnxruntime.get_available_providers()
    # get_available_providers() só lista o que o pacote traz, não o que
    # carrega: com cuBLAS/cuDNN ausentes o CUDA aparece na lista e falha na
    # primeira sessão, e o app roda em CPU. Carregar a DLL do provider
    # reproduz essa falha aqui, em vez de descobri-la pela lentidão.
    if "CUDAExecutionProvider" in providers:
        import ctypes
        capi = os.path.join(os.path.dirname(onnxruntime.__file__), "capi")
        try:
            ctypes.WinDLL(os.path.join(capi, "onnxruntime_providers_cuda.dll"))
        except OSError:
            providers = [p for p in providers if p != "CUDAExecutionProvider"]
    print(json.dumps(providers))
except Exception as exc:
    print(json.dumps({"error": str(exc)}))
"#;

    let mut command = Command::new(&python);
    command.arg("-c").arg(probe).current_dir(&app_dir);
    crate::platform::hide_console(&mut command);

    let output = command.output().map_err(|e| format!("não consegui checar o acelerador: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("checagem falhou: {}", stderr.trim()));
    }
    Ok(stdout)
}

/// Nome curto do acelerador a partir da lista de providers.
pub fn accelerator_name(providers: &str) -> &'static str {
    if providers.contains("CUDAExecutionProvider") {
        "CUDA"
    } else if providers.contains("CoreMLExecutionProvider") {
        "CoreML"
    } else if providers.contains("DmlExecutionProvider") {
        "DirectML"
    } else {
        "CPU"
    }
}
