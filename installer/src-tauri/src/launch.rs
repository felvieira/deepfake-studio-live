//! Abrir o app instalado.
//!
//! O cuidado aqui é não estragar o que run.py faz sozinho. Ele monta o
//! ambiente CUDA no começo da execução (add_dll_directory para
//! venv/Lib/site-packages/nvidia/*/bin), porque o Python 3.8+ ignora o PATH
//! para dependências nativas de módulos de extensão. Se esse trabalho não
//! acontecer, o onnxruntime não acha cuDNN/cuBLAS e cai para CPU **em
//! silêncio**: o app abre, funciona e fica lento, sem mensagem de erro.
//!
//! Por isso o launcher faz o mínimo: roda o python do venv, com cwd em app/,
//! e deixa o run.py cuidar do resto. O que não pode faltar é o cwd — sem ele
//! o run.py calcula project_root errado.

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Não abrir janela de console junto com a GUI.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn launch_app() -> Result<u32, String> {
    let app_dir = crate::paths::app_dir()?;
    let python = crate::paths::venv_python()?;
    let entry = app_dir.join("run.py");

    if !python.exists() {
        return Err(format!(
            "o Python da instalação não está em {} — rode a instalação de novo",
            python.display()
        ));
    }
    if !entry.exists() {
        return Err(format!(
            "run.py não está em {} — instalação incompleta",
            entry.display()
        ));
    }

    let mut command = Command::new(&python);
    command.arg("run.py");
    // cwd em app/: run.py deriva project_root do próprio caminho, mas o
    // resto do projeto (switch_states.json, models/) é relativo ao cwd.
    command.current_dir(&app_dir);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let child = command
        .spawn()
        .map_err(|e| format!("não consegui iniciar o app: {e}"))?;

    Ok(child.id())
}

/// Pergunta ao app instalado qual execution provider o onnxruntime realmente
/// carregou.
///
/// Existe porque a queda para CPU é silenciosa: sem checar, uma instalação
/// com CUDA quebrado parece bem-sucedida e o usuário só percebe pela
/// lentidão. Roda o python do venv com o mesmo preâmbulo de DLLs do run.py
/// e devolve a lista de providers disponíveis.
pub fn detect_execution_provider() -> Result<String, String> {
    let app_dir = crate::paths::app_dir()?;
    let python = crate::paths::venv_python()?;
    if !python.exists() {
        return Err("instalação não encontrada".into());
    }

    // Reproduz o registro de DLLs do run.py antes de importar onnxruntime —
    // importar direto daria um falso negativo, já que o preâmbulo do run.py
    // é justamente o que torna os providers de GPU carregáveis.
    let probe = r#"
import os, sys, json
root = os.getcwd()
sp = os.path.join(root, "venv", "Lib", "site-packages")
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
    print(json.dumps(onnxruntime.get_available_providers()))
except Exception as exc:
    print(json.dumps({"error": str(exc)}))
"#;

    let mut command = Command::new(&python);
    command.arg("-c").arg(probe).current_dir(&app_dir);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|e| format!("não consegui checar o acelerador: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("checagem falhou: {}", stderr.trim()));
    }
    Ok(stdout)
}
