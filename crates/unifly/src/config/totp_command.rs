//! TOTP codes from a profile's `totp_command`.
//!
//! The command runs directly, without a shell, every time a login needs a
//! code. That is what lets an MFA session renew itself long after startup,
//! where a code read once from a flag or an environment variable is spent.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use secrecy::SecretString;
use tracing::warn;
use unifly_api::TotpProvider;

/// How long a `totp_command` may run before it is killed and treated as
/// having no code: room for a password manager to unlock, short enough that
/// a hung command does not stall a login for long.
pub const TOTP_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Far more than any code needs; output past this is not read.
const MAX_OUTPUT_BYTES: u64 = 1024;

/// How often a running command is checked for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Wrap `argv` (the program, then its arguments) as a [`TotpProvider`].
pub fn provider(argv: Vec<String>) -> TotpProvider {
    TotpProvider::new(move || run(&argv, TOTP_COMMAND_TIMEOUT))
}

/// Run `argv` and return its trimmed stdout as the code.
///
/// A command that cannot start, exits non-zero, outlives `timeout`, or
/// prints nothing yields `None`. Failures are logged by program name only:
/// neither the arguments nor the output reach a log line.
fn run(argv: &[String], timeout: Duration) -> Option<SecretString> {
    let (program, arguments) = argv.split_first()?;
    let deadline = Instant::now() + timeout;
    let mut child = match Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            warn!(%error, %program, "totp_command could not be started");
            return None;
        }
    };
    let Some(stdout) = child.stdout.take() else {
        stop(&mut child);
        return None;
    };

    // Drain stdout on its own thread, so neither a full pipe nor something
    // the command spawned that keeps the pipe open can hold us past the
    // deadline.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let read = stdout.take(MAX_OUTPUT_BYTES).read_to_end(&mut output);
        let _ = tx.send(read.map(|_| output));
    });

    let status = wait_until(&mut child, deadline, program, timeout)?;
    if !status.success() {
        warn!(%program, %status, "totp_command failed");
        return None;
    }
    let output = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            warn!(%error, %program, "totp_command output could not be read");
            return None;
        }
        Err(_) => {
            warn!(%program, "totp_command exited but its output never closed");
            return None;
        }
    };
    let Ok(output) = String::from_utf8(output) else {
        warn!(%program, "totp_command printed something other than text");
        return None;
    };
    let code = output.trim();
    if code.is_empty() {
        warn!(%program, "totp_command printed no code");
        return None;
    }
    Some(SecretString::from(code.to_owned()))
}

/// Wait for `child` to exit, killing it if it is still running at
/// `deadline`.
fn wait_until(
    child: &mut Child,
    deadline: Instant,
    program: &str,
    timeout: Duration,
) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(error) => {
                warn!(%error, %program, "totp_command could not be waited on");
                stop(child);
                return None;
            }
        }
        let now = Instant::now();
        if now >= deadline {
            warn!(%program, ?timeout, "totp_command timed out and was stopped");
            stop(child);
            return None;
        }
        std::thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

/// Kill `child` and reap it so it does not linger as a zombie.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::run;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[cfg(unix)]
    #[test]
    fn trimmed_stdout_is_the_code() {
        use secrecy::ExposeSecret;

        let code = run(
            &argv(&["sh", "-c", "printf '  246810\\n'"]),
            Duration::from_secs(5),
        )
        .expect("the command prints a code");
        assert_eq!(code.expose_secret(), "246810");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_command_yields_no_code() {
        let code = run(
            &argv(&["sh", "-c", "echo 246810; exit 3"]),
            Duration::from_secs(5),
        );
        assert!(code.is_none(), "a non-zero exit must not count");
    }

    #[cfg(unix)]
    #[test]
    fn blank_output_yields_no_code() {
        let code = run(
            &argv(&["sh", "-c", "printf ' \\n'"]),
            Duration::from_secs(5),
        );
        assert!(code.is_none(), "whitespace is not a code");
    }

    #[cfg(unix)]
    #[test]
    fn an_overrunning_command_is_stopped_and_yields_no_code() {
        let started = std::time::Instant::now();
        let code = run(
            &argv(&["sh", "-c", "echo 246810; exec sleep 30"]),
            Duration::from_millis(200),
        );
        assert!(code.is_none(), "a timed-out command must not count");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the timeout was not enforced: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_missing_program_yields_no_code() {
        let code = run(
            &argv(&["/nonexistent/unifly-totp-helper"]),
            Duration::from_secs(5),
        );
        assert!(code.is_none());
    }

    #[test]
    fn an_empty_command_yields_no_code() {
        assert!(run(&[], Duration::from_secs(5)).is_none());
    }
}
