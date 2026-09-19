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

use std::fs::File;
use std::process::{Command, Stdio};
use std::time::Duration;

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

    // stdout/stderr do processo iam para lugar nenhum: CREATE_NO_WINDOW
    // suprime o console, e sem um Stdio explícito o Rust também não os
    // captura. Se run.py levantar uma exceção antes de a janela do Qt
    // abrir — import faltando, erro do onnxruntime na inicialização — o
    // processo morre e não sobra rastro nenhum de por quê, nem para o
    // usuário nem para quem for depurar depois. Redireciona para um
    // arquivo em vez de Stdio::piped(): o app roda solto após este
    // comando retornar, então não há como (nem por quê) manter um handle
    // vivo lendo a saída.
    let log_path = crate::paths::logs_dir()?.join("app.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("não consegui criar {}: {e}", parent.display()))?;
    }
    let stdout_file = File::create(&log_path)
        .map_err(|e| format!("não consegui criar {}: {e}", log_path.display()))?;
    let stderr_file = stdout_file
        .try_clone()
        .map_err(|e| format!("não consegui preparar o log: {e}"))?;

    let mut command = Command::new(&python);
    command.arg(&entry);
    // cwd em app/: run.py deriva project_root do próprio caminho, mas o
    // resto do projeto (switch_states.json, models/) é relativo ao cwd.
    //
    // app/ entrar em sys.path (pra `from modules import ...` resolver) não
    // é responsabilidade daqui — nem current_dir() nem PYTHONPATH bastam
    // com o Python embeddable, porque o `._pth` dele ignora os dois. Ver
    // steps::fix_pth_restrictions, chamada uma vez durante a instalação,
    // que resolve isso editando o próprio `._pth`.
    command.current_dir(&app_dir);
    command.stdout(Stdio::from(stdout_file));
    command.stderr(Stdio::from(stderr_file));

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|e| format!("não consegui iniciar o app: {e}"))?;

    // Um crash na inicialização (import faltando, exceção antes da janela
    // abrir) normalmente acontece em menos de um segundo. Uma pausa curta
    // aqui troca isso por um erro imediato e legível em vez de deixar o
    // usuário olhando pra tela achando que "não fez nada".
    std::thread::sleep(Duration::from_millis(800));
    if let Ok(Some(status)) = child.try_wait() {
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let tail: Vec<&str> = log.lines().rev().take(20).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(format!(
            "o aplicativo fechou logo após abrir (código {}).\n{}",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "desconhecido".into()),
            if tail.is_empty() { "(sem saída capturada)".to_string() } else { tail }
        ));
    }

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
    //
    // Duas pastas são checadas, igual ao run.py real (run.py:14-15): não há
    // mais venv/ desde que a instalação passou a usar o Python embeddable
    // direto (ver paths::venv_python), então sys.prefix/Lib/site-packages é
    // onde os pacotes realmente estão — mas o run.py também verifica
    // venv/Lib/site-packages, sem quebrar se não existir, então este probe
    // faz o mesmo em vez de assumir qual dos dois é o caminho real.
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
