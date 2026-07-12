use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const IO_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) struct ProcessLimits {
    pub(super) timeout: Duration,
    pub(super) stdout_bytes: usize,
    pub(super) stderr_bytes: usize,
}

pub(super) struct ProcessOutput {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) stdout_truncated: bool,
    pub(super) stderr_truncated: bool,
}

#[derive(Debug)]
pub(super) enum ProcessError {
    Start(std::io::Error),
    Wait(std::io::Error),
    Timeout,
    ProcessIdUnavailable,
    InputUnavailable,
    OutputUnavailable,
    Input(std::io::Error),
    Drain,
}

pub(super) fn run_process(
    command: &mut Command,
    input: Option<&[u8]>,
    limits: ProcessLimits,
) -> Result<ProcessOutput, ProcessError> {
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = spawn_process(command)?;
    let Ok(process_group) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ProcessError::ProcessIdUnavailable);
    };
    let process_group = Pid::from_raw(process_group);

    let input_result = input.map(|input| {
        let Some(mut stdin) = child.stdin.take() else {
            terminate_process_group(&mut child, process_group);
            return Err(ProcessError::InputUnavailable);
        };
        let input = input.to_vec();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = sender.send(stdin.write_all(&input));
        });
        Ok(receiver)
    });
    let input_result = input_result.transpose()?;

    let Some(stdout) = child.stdout.take() else {
        terminate_process_group(&mut child, process_group);
        return Err(ProcessError::OutputUnavailable);
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_process_group(&mut child, process_group);
        return Err(ProcessError::OutputUnavailable);
    };
    let stdout_result = read_capped_async(stdout, limits.stdout_bytes);
    let stderr_result = read_capped_async(stderr, limits.stderr_bytes);

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limits.timeout => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                terminate_process_group(&mut child, process_group);
                return Err(ProcessError::Timeout);
            }
            Err(error) => {
                terminate_process_group(&mut child, process_group);
                return Err(ProcessError::Wait(error));
            }
        }
    };
    // A formatter may leave descendants holding inherited pipes open after its main process exits.
    let _ = killpg(process_group, Signal::SIGKILL);

    let input_error = if let Some(input_result) = input_result {
        input_result
            .recv_timeout(IO_DRAIN_TIMEOUT)
            .map_err(|_| ProcessError::Drain)?
            .err()
    } else {
        None
    };
    let (stdout, stdout_truncated) = receive_output(stdout_result)?;
    let (stderr, stderr_truncated) = receive_output(stderr_result)?;
    if status.success()
        && let Some(error) = input_error
    {
        return Err(ProcessError::Input(error));
    }
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

fn spawn_process(command: &mut Command) -> Result<std::process::Child, ProcessError> {
    for attempt in 0..3 {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 2 => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(ProcessError::Start(error)),
        }
    }
    unreachable!("bounded spawn loop always returns")
}

fn terminate_process_group(child: &mut std::process::Child, process_group: Pid) {
    let _ = killpg(process_group, Signal::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

type OutputReceiver = mpsc::Receiver<std::io::Result<(Vec<u8>, bool)>>;

fn read_capped_async(reader: impl Read + Send + 'static, limit: usize) -> OutputReceiver {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(read_capped(reader, limit));
    });
    receiver
}

fn receive_output(receiver: OutputReceiver) -> Result<(Vec<u8>, bool), ProcessError> {
    receiver
        .recv_timeout(IO_DRAIN_TIMEOUT)
        .map_err(|_| ProcessError::Drain)?
        .map_err(ProcessError::Wait)
}

fn read_capped(mut reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0_u8; 16 * 1024];
    let mut truncated = false;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let available = limit.saturating_sub(retained.len());
        let retained_count = available.min(count);
        retained.extend_from_slice(&buffer[..retained_count]);
        truncated |= retained_count < count;
    }
    Ok((retained, truncated))
}

pub(super) fn stderr_message(output: &ProcessOutput) -> String {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.stderr_truncated {
        format!("{message}\n[stderr truncated]").trim().to_owned()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn captures_success_output_and_bounded_errors() {
        let directory = test_directory("output");
        let script = write_script(
            &directory,
            "process",
            "read value\nprintf '%s' \"$value\"\ni=0\nwhile [ $i -lt 100 ]; do printf x >&2; i=$((i + 1)); done\n",
        );
        let mut command = Command::new("/bin/sh");
        command.arg(script);
        let output = run_process(
            &mut command,
            Some(b"formatted\n"),
            ProcessLimits {
                timeout: Duration::from_secs(1),
                stdout_bytes: 64,
                stderr_bytes: 16,
            },
        )
        .unwrap();

        assert!(output.status.success());
        assert_eq!(output.stdout, b"formatted");
        assert!(!output.stdout_truncated);
        assert_eq!(output.stderr.len(), 16);
        assert!(output.stderr_truncated);
        assert!(stderr_message(&output).ends_with("[stderr truncated]"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn timeout_kills_the_entire_process_group() {
        let directory = test_directory("timeout");
        let marker = directory.join("descendant-survived");
        let script = write_script(
            &directory,
            "timeout",
            "marker=$1\n(sleep 0.2; printf survived > \"$marker\") &\nsleep 5\n",
        );
        let mut command = Command::new("/bin/sh");
        command.arg(script).arg(&marker);

        assert!(matches!(
            run_process(
                &mut command,
                None,
                ProcessLimits {
                    timeout: Duration::from_millis(50),
                    stdout_bytes: 16,
                    stderr_bytes: 16,
                },
            ),
            Err(ProcessError::Timeout)
        ));
        thread::sleep(Duration::from_millis(350));
        assert!(!marker.exists());
        let _ = fs::remove_dir_all(directory);
    }

    fn test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kosmos-formatter-process-{name}-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn write_script(directory: &Path, name: &str, body: &str) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }
}
