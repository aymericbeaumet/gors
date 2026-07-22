use std::fs;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) struct RunningCommand {
    child: Child,
    stdout_file: tempfile::NamedTempFile,
    stderr_file: tempfile::NamedTempFile,
    started: Instant,
}

impl RunningCommand {
    pub(super) fn started(&self) -> Instant {
        self.started
    }
}

pub(super) fn spawn_command_abortable(
    mut command: Command,
    abort: &AtomicBool,
) -> Result<Option<RunningCommand>, String> {
    if abort.load(Ordering::SeqCst) {
        return Ok(None);
    }
    let stdout_file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let stderr_file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    command
        .stdout(Stdio::from(
            stdout_file.reopen().map_err(|e| e.to_string())?,
        ))
        .stderr(Stdio::from(
            stderr_file.reopen().map_err(|e| e.to_string())?,
        ));
    let child = command.spawn().map_err(|e| e.to_string())?;
    Ok(Some(RunningCommand {
        child,
        stdout_file,
        stderr_file,
        started: Instant::now(),
    }))
}

pub(super) fn wait_command_output_abortable(
    mut running: RunningCommand,
    abort: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<Option<Output>, String> {
    loop {
        if abort.load(Ordering::SeqCst) {
            let _ = running.child.kill();
            let _ = running.child.wait();
            return Ok(None);
        }
        if let Some(status) = running.child.try_wait().map_err(|e| e.to_string())? {
            return Ok(Some(Output {
                status,
                stdout: fs::read(running.stdout_file.path()).map_err(|e| e.to_string())?,
                stderr: fs::read(running.stderr_file.path()).map_err(|e| e.to_string())?,
            }));
        }
        if let Some(timeout) = timeout
            && running.started.elapsed() >= timeout
        {
            let _ = running.child.kill();
            let _ = running.child.wait();
            return Err(format!("command timed out after {timeout:?}"));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn command_output_abortable(
    command: Command,
    abort: &AtomicBool,
    timeout: Option<Duration>,
) -> Result<Option<Output>, String> {
    let Some(running) = spawn_command_abortable(command, abort)? else {
        return Ok(None);
    };
    wait_command_output_abortable(running, abort, timeout)
}

pub fn command_output_with_timeout(command: Command, timeout: Duration) -> Result<Output, String> {
    let abort = AtomicBool::new(false);
    command_output_abortable(command, &abort, Some(timeout))?
        .ok_or_else(|| "command was cancelled unexpectedly".to_string())
}
