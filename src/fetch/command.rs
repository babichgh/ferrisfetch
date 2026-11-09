//! Bounded execution for optional system-information tools.
//!
//! Drain both pipes concurrently to avoid deadlocks. Never join pipe readers:
//! descendants can inherit pipes even after the direct child has exited. On
//! Unix a private process group lets cleanup kill those descendants too.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(2);
const OUTPUT_LIMIT: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Io,
    Timeout,
    OutputLimit,
    Exit,
}

/// Returns stdout only for a successful command within the time/output limits.
/// Each of stdout and stderr is limited to 1 MiB. Errors remain silent so callers
/// can try another backend or omit unavailable information.
pub(crate) fn output(program: &str, args: &[&str]) -> Option<String> {
    run(program, args, TIMEOUT, OUTPUT_LIMIT)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

struct RunningChild(Child);

impl Drop for RunningChild {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = rustix::process::Pid::from_raw(self.0.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        // Also covers non-Unix platforms and a failed group signal. Always reap
        // the direct child, including on setup, read, timeout, and exit errors.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read_limited(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(count) => {
                if count > limit.saturating_sub(bytes.len()) {
                    return Err(Failure::OutputLimit);
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Failure::Io),
        }
    }
}

fn run(program: &str, args: &[&str], timeout: Duration, limit: usize) -> Result<Vec<u8>, Failure> {
    let start = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = RunningChild(command.spawn().map_err(|_| Failure::Io)?);
    let stdout = child.0.stdout.take().ok_or(Failure::Io)?;
    let stderr = child.0.stderr.take().ok_or(Failure::Io)?;
    // Keep the original sender alive while polling so a disconnected channel
    // cannot turn the loop into a busy wait when both pipes close before exit.
    let (sender, receiver) = mpsc::channel();
    let tx = sender.clone();
    std::thread::Builder::new()
        .name("fetch-stdout".into())
        .spawn(move || {
            let _ = tx.send((true, read_limited(stdout, limit)));
        })
        .map_err(|_| Failure::Io)?;
    let tx = sender.clone();
    std::thread::Builder::new()
        .name("fetch-stderr".into())
        .spawn(move || {
            let _ = tx.send((false, read_limited(stderr, limit)));
        })
        .map_err(|_| Failure::Io)?;

    let mut stdout = None;
    let mut stderr_done = false;
    let mut status = None;
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(Failure::Timeout);
        }
        if status.is_none() {
            status = child.0.try_wait().map_err(|_| Failure::Io)?;
        }
        if let Some(status) = status {
            if !status.success() {
                return Err(Failure::Exit);
            }
            if stderr_done && let Some(bytes) = stdout.take() {
                return Ok(bytes);
            }
        }
        match receiver.recv_timeout(remaining.min(POLL_INTERVAL)) {
            Ok((is_stdout, result)) => {
                let bytes = result?;
                if is_stdout {
                    stdout = Some(bytes);
                } else {
                    stderr_done = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Failure::Io),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_limit_is_exact() {
        assert_eq!(read_limited(&b"abcd"[..], 4), Ok(b"abcd".to_vec()));
        assert_eq!(read_limited(&b"abcde"[..], 4), Err(Failure::OutputLimit));
        assert_eq!(read_limited(&b""[..], 0), Ok(Vec::new()));
        assert_eq!(read_limited(&b"x"[..], 0), Err(Failure::OutputLimit));
    }

    #[test]
    fn missing_command_is_unavailable() {
        assert!(output("/ferrisfetch-test/nonexistent/tool", &[]).is_none());
    }

    // A shell is used only as a controlled test fixture, never by detectors.
    #[cfg(unix)]
    fn shell(script: &str, timeout: Duration, limit: usize) -> Result<Vec<u8>, Failure> {
        run("/bin/sh", &["-c", script], timeout, limit)
    }

    #[cfg(unix)]
    #[test]
    fn successful_command_keeps_stdout_and_accepts_stderr() {
        assert_eq!(
            shell("printf hello; printf warning >&2", TIMEOUT, OUTPUT_LIMIT),
            Ok(b"hello".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn locale_is_fixed_and_stdin_is_closed() {
        assert_eq!(
            shell(
                "printf '%s|%s|' \"$LC_ALL\" \"$LANG\"; if read -r value; then printf open; else printf closed; fi",
                TIMEOUT,
                OUTPUT_LIMIT,
            ),
            Ok(b"C|C|closed".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn arguments_are_not_interpreted_as_shell_code() {
        assert_eq!(
            run(
                "/bin/sh",
                &["-c", "printf '%s' \"$1\"", "fixture", "$(exit 1); *"],
                TIMEOUT,
                OUTPUT_LIMIT
            ),
            Ok(b"$(exit 1); *".to_vec())
        );
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_and_signal_are_unavailable() {
        assert_eq!(
            shell("printf partial; exit 7", TIMEOUT, OUTPUT_LIMIT),
            Err(Failure::Exit)
        );
        assert_eq!(
            shell("kill -TERM $$", TIMEOUT, OUTPUT_LIMIT),
            Err(Failure::Exit)
        );
    }

    #[cfg(unix)]
    #[test]
    fn excessive_stdout_and_stderr_are_rejected() {
        for script in [
            "while :; do printf 123456789; done",
            "while :; do printf 123456789 >&2; done",
        ] {
            // Closing a full pipe can deliver SIGPIPE before the parent sees
            // the reader's limit error; either outcome must reject the output.
            assert!(matches!(
                shell(script, TIMEOUT, 1024),
                Err(Failure::OutputLimit | Failure::Exit)
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn both_pipes_are_drained_without_deadlocking() {
        assert_eq!(
            shell(
                "i=0; while [ \"$i\" -lt 20000 ]; do printf x; printf y >&2; i=$((i + 1)); done",
                Duration::from_secs(10),
                20000,
            ),
            Ok(vec![b'x'; 20000])
        );
    }

    #[cfg(unix)]
    #[test]
    fn hanging_command_times_out_even_with_closed_pipes() {
        assert_eq!(
            shell(
                "exec 1>&- 2>&-; exec sleep 30",
                Duration::from_millis(100),
                OUTPUT_LIMIT
            ),
            Err(Failure::Timeout)
        );
    }

    #[cfg(unix)]
    #[test]
    fn inherited_pipes_do_not_prevent_timeout() {
        assert_eq!(
            shell(
                "sleep 30 & exit 0",
                Duration::from_millis(100),
                OUTPUT_LIMIT
            ),
            Err(Failure::Timeout)
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_kills_and_reaps_the_child() {
        use rustix::process::{Pid, test_kill_process};
        use std::os::unix::process::CommandExt;

        let child = Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = Pid::from_raw(child.id() as i32).unwrap();
        drop(RunningChild(child));
        assert_eq!(test_kill_process(pid), Err(rustix::io::Errno::SRCH));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cleanup_also_terminates_background_children() {
        let stdout = shell(
            "sleep 30 </dev/null >/dev/null 2>&1 & printf '%s' \"$!\"",
            TIMEOUT,
            OUTPUT_LIMIT,
        )
        .unwrap();
        let pid: u32 = String::from_utf8(stdout).unwrap().parse().unwrap();
        // Group signals are asynchronous. An orphan may remain as a zombie
        // until PID 1 reaps it, but it must no longer be running.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return,
                Ok(stat)
                    if stat
                        .rsplit_once(") ")
                        .is_some_and(|(_, tail)| tail.starts_with(['Z', 'X'])) =>
                {
                    return;
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                if let Some(pid) = rustix::process::Pid::from_raw(pid as i32) {
                    let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                }
                panic!("background child {pid} survived group cleanup");
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}
