//! Atalhos "Deepfake Studio Live" (Área de Trabalho e Menu Iniciar).
//!
//! O único atalho que existia era o do instalador: depois de instalar, o
//! usuário abria a janela de instalação de novo só para clicar em "Abrir".
//! Estes atalhos chamam este mesmo executável com `--launch`, que abre o app
//! direto, sem janela de instalação (ver main::launch_and_exit). Assim não
//! há um segundo binário para distribuir nem para manter em sincronia.

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Nome do atalho, que é o nome que o usuário vê e procura.
const SHORTCUT_NAME: &str = "Deepfake Studio Live";

/// Cria (ou recria) os atalhos. Idempotente: rodar de novo só reescreve o
/// mesmo arquivo, então serve também para consertar um atalho apagado.
///
/// Devolve as pastas onde criou, para a UI poder dizer onde achar.
pub fn create_app_shortcuts() -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("não achei o executável: {e}"))?;
    let exe_dir = exe
        .parent()
        .ok_or("o executável não tem pasta")?
        .to_path_buf();

    // O caminho vai por variável de ambiente, nunca interpolado no script:
    // um perfil de usuário com aspas ou `$` no nome quebraria (ou
    // reinterpretaria) a linha de comando do PowerShell.
    let script = r#"
$ErrorActionPreference = 'Stop'
$shell = New-Object -ComObject WScript.Shell
$made = @()
foreach ($dir in @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('Programs'))) {
    if (-not $dir) { continue }
    $link = Join-Path $dir ($env:DSL_NAME + '.lnk')
    $s = $shell.CreateShortcut($link)
    $s.TargetPath = $env:DSL_TARGET
    $s.Arguments = '--launch'
    $s.WorkingDirectory = $env:DSL_WORKDIR
    $s.IconLocation = $env:DSL_TARGET + ',0'
    $s.Description = 'Troca de rosto ao vivo com câmera virtual'
    $s.Save()
    $made += $dir
}
$made -join "`n"
"#;

    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("DSL_NAME", SHORTCUT_NAME)
        .env("DSL_TARGET", &exe)
        .env("DSL_WORKDIR", &exe_dir);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|e| format!("não consegui rodar o PowerShell: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "falha ao criar os atalhos: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}
