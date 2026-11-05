//! Subprocess configuration and clean termination.
//!
//! Background language servers and ACP agents must never attach to the host
//! terminal or flash console windows on Windows. All OS-specific flags are
//! confined to this module.

use std::time::Duration;

/// Configure a synchronous command for background execution.
pub fn configure_background_command(command: &mut std::process::Command) {
    configure_std_command(command);
}

/// Configure a Tokio command for background execution.
pub fn configure_tokio_command(command: &mut tokio::process::Command) {
    configure_tokio(command);
}

/// Terminate a Tokio child and reap it, escalating to a hard kill after
/// `grace` elapses without exit.
pub async fn terminate_child(child: &mut tokio::process::Child, grace: Duration) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    if let Err(error) = child.start_kill() {
        tracing::warn!(%error, "failed to signal child process");
    }
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        if let Err(error) = child.kill().await {
            tracing::warn!(%error, "failed to hard-kill child process");
        }
    }
}

#[cfg(unix)]
fn configure_std_command(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_std_command(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn configure_std_command(_command: &mut std::process::Command) {}

#[cfg(unix)]
fn configure_tokio(command: &mut tokio::process::Command) {
    command.process_group(0);
}

#[cfg(windows)]
fn configure_tokio(command: &mut tokio::process::Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn configure_tokio(_command: &mut tokio::process::Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn short_lived_child_is_reaped_without_termination() {
        let mut command = tokio::process::Command::new("true");
        configure_tokio_command(&mut command);
        let mut child = command.spawn().expect("spawn true");
        terminate_child(&mut child, Duration::from_millis(250)).await;
    }
}
