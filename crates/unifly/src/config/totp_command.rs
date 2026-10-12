//! TOTP codes from a profile's `totp_command`.
//!
//! The command runs directly, without a shell, every time a login needs a
//! code. That is what lets an MFA session renew itself long after startup,
//! where a code read once from a flag or an environment variable is spent.

use std::process::Stdio;
use std::time::Duration;

use secrecy::SecretString;
use tokio::io::AsyncReadExt;
use tokio::time::Instant;
use tracing::warn;
use unifly_api::TotpProvider;

/// How long a `totp_command` may run before it is killed and treated as
/// having no code: room for a password manager to unlock, short enough that
/// a hung command does not stall a login for long.
pub const TOTP_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to keep reading after the command exits, for output still in
/// flight, before giving up on a pipe that something it started holds open.
const OUTPUT_GRACE: Duration = Duration::from_millis(100);

/// Far more than any code needs; output past this is not read.
const MAX_OUTPUT_BYTES: usize = 1024;

/// Wrap `argv` (the program, then its arguments) as a [`TotpProvider`].
pub fn provider(argv: Vec<String>) -> TotpProvider {
    TotpProvider::new(move || run(&argv, TOTP_COMMAND_TIMEOUT))
}

/// Run `argv` and return its trimmed stdout as the code.
///
/// A command that cannot start, exits non-zero, outlives `timeout`, or
/// prints nothing yields `None`. Failures are logged by program name only:
/// neither the arguments nor the output reach a log line.
///
/// It runs on a private single-threaded runtime, so reading its output is
/// an async read that is simply dropped when time runs out. A blocking read
/// on a thread could not be abandoned, and would stay stuck for as long as
/// anything the command started held the pipe open.
///
/// One narrow limit remains on Windows, where tokio itself reads a child's
/// pipe with a blocking read that dropping it does not cancel. The call
/// still returns on time (see [`block_on_detached`]), but a helper that
/// leaves a background process holding its stdout leaves one thread
/// blocked until that process exits. Stopping such processes there would
/// take Job Objects.
fn run(argv: &[String], timeout: Duration) -> Option<SecretString> {
    block_on_detached(run_async(argv, timeout)).flatten()
}

/// Drive `future` to completion on a private current-thread runtime, then
/// shut the runtime down without waiting for its blocking tasks. Dropping
/// it normally would wait for every one of them, including a blocking pipe
/// read that only ends when whatever holds the pipe open exits. `None` when
/// no runtime could be built.
fn block_on_detached<F: std::future::Future>(future: F) -> Option<F::Output> {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            warn!(%error, "totp_command could not get a runtime to run on");
            return None;
        }
    };
    let output = runtime.block_on(future);
    runtime.shutdown_background();
    Some(output)
}

async fn run_async(argv: &[String], timeout: Duration) -> Option<SecretString> {
    let (program, arguments) = argv.split_first()?;
    let deadline = Instant::now() + timeout;
    let mut command = tokio::process::Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // Its own process group, so stopping it reaches everything it started.
    #[cfg(unix)]
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            warn!(%error, %program, "totp_command could not be started");
            return None;
        }
    };
    // Declared after `child`, so it is dropped first, while the command's
    // pid still names the group.
    let mut group = GroupReaper::armed(child.id());
    let mut stdout = child.stdout.take()?;

    // Read while waiting, so a command never stalls on a full pipe.
    let mut output = Vec::new();
    let mut chunk = [0_u8; 256];
    let mut pipe = Pipe::Open;
    let status = loop {
        tokio::select! {
            status = child.wait() => break status,
            read = stdout.read(&mut chunk), if pipe == Pipe::Open => {
                pipe = take_chunk(&read, &chunk, &mut output);
            }
            () = tokio::time::sleep_until(deadline) => {
                warn!(%program, ?timeout, "totp_command timed out and was stopped");
                group.stop();
                let _ = child.kill().await;
                return None;
            }
        }
    };

    // It has exited, so whatever it printed is already in the pipe. Drain
    // that, but only until the pipe closes or a short grace passes:
    // something it started may hold the pipe open indefinitely.
    if pipe == Pipe::Open {
        let _ = tokio::time::timeout(OUTPUT_GRACE, async {
            while pipe == Pipe::Open {
                pipe = take_chunk(&stdout.read(&mut chunk).await, &chunk, &mut output);
            }
        })
        .await;
    }
    drop(stdout);
    // End of file means every writer closed the pipe: nothing the command
    // started still holds it, so a helper that properly detached (a cache
    // daemon with its output redirected) is left running. Anything still
    // holding the pipe, a read error, or a failed wait stops the group.
    if pipe == Pipe::Closed && status.is_ok() {
        group.disarm();
    }
    drop(group);

    let status = match status {
        Ok(status) => status,
        Err(error) => {
            warn!(%error, %program, "totp_command could not be waited on");
            return None;
        }
    };
    if !status.success() {
        warn!(%program, %status, "totp_command failed");
        return None;
    }
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

/// The state of the command's stdout pipe.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pipe {
    /// Still open: something may write more.
    Open,
    /// End of file: every process holding the write end has closed it.
    Closed,
    /// A read failed; whether anything still holds it is unknown.
    Failed,
}

/// Record one read: append its bytes to `output` up to the output limit,
/// discarding the rest. Reading goes on past the limit so that end of file
/// can still be seen.
fn take_chunk(read: &std::io::Result<usize>, chunk: &[u8], output: &mut Vec<u8>) -> Pipe {
    match *read {
        Ok(0) => Pipe::Closed,
        Err(_) => Pipe::Failed,
        Ok(len) => {
            let room = MAX_OUTPUT_BYTES.saturating_sub(output.len());
            output.extend_from_slice(chunk.get(..len.min(room)).unwrap_or_default());
            Pipe::Open
        }
    }
}

/// Stops the command's process group when dropped, unless disarmed. A run
/// disarms it only after a clean exit whose stdout reached end of file, so
/// every other way out (a timeout, a failed wait or read, something still
/// holding the pipe, an early return) leaves nothing behind that the
/// command started in its group.
struct GroupReaper {
    group: Option<u32>,
    armed: bool,
}

impl GroupReaper {
    fn armed(group: Option<u32>) -> Self {
        Self { group, armed: true }
    }

    fn stop(&self) {
        kill_group(self.group);
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for GroupReaper {
    fn drop(&mut self) {
        if self.armed {
            self.stop();
        }
    }
}

/// Send SIGKILL to the command's process group (its leader's pid), which
/// takes down anything it started that did not leave the group.
#[cfg(unix)]
fn kill_group(group: Option<u32>) {
    use rustix::process::{Pid, Signal, kill_process_group};

    let Some(pid) = group
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
    else {
        return;
    };
    // An empty group (everything already exited) is not an error here.
    let _ = kill_process_group(pid, Signal::KILL);
}

/// Without process groups, only the command itself is stopped (by
/// `kill_on_drop` and the explicit kill on timeout).
#[cfg(not(unix))]
fn kill_group(_group: Option<u32>) {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{block_on_detached, run};

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    /// Whether the process `pid` still exists, reaped or not. A killed
    /// orphan is reaped by init shortly after, so callers poll.
    #[cfg(unix)]
    fn alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    fn wait_for_exit(pid: &str) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while alive(pid) {
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
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

    /// A command that exits while something it backgrounded holds stdout
    /// open: the code it printed still counts, the call returns promptly
    /// instead of waiting on the open pipe, and the straggler is stopped.
    #[cfg(unix)]
    #[test]
    fn a_backgrounded_descendant_holding_stdout_is_stopped() {
        use secrecy::ExposeSecret;

        let dir = tempfile::tempdir().expect("temp dir");
        let pid_file = dir.path().join("descendant.pid");
        let script = format!(
            "sleep 86400 & echo $! > '{}'; printf 123456",
            pid_file.display()
        );

        let started = std::time::Instant::now();
        let code = run(&argv(&["sh", "-c", &script]), Duration::from_secs(5))
            .expect("the code printed before exiting counts");
        assert_eq!(code.expose_secret(), "123456");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "waited on the held pipe: {:?}",
            started.elapsed()
        );

        let pid = std::fs::read_to_string(&pid_file).expect("descendant pid");
        assert!(wait_for_exit(pid.trim()), "the descendant {pid} still runs");
    }

    /// Output past the limit stops the read early; that must not skip the
    /// cleanup. A helper that prints more than the limit, leaves a process
    /// holding stdout, and exits still leaves no survivor.
    #[cfg(unix)]
    #[test]
    fn output_past_the_limit_does_not_skip_cleanup() {
        let dir = tempfile::tempdir().expect("temp dir");
        let pid_file = dir.path().join("descendant.pid");
        let script = format!(
            "sleep 86400 & echo $! > '{}'; printf '%0{}d' 0",
            pid_file.display(),
            super::MAX_OUTPUT_BYTES * 2
        );

        let started = std::time::Instant::now();
        let _ = run(&argv(&["sh", "-c", &script]), Duration::from_secs(5));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "waited on the held pipe: {:?}",
            started.elapsed()
        );

        let pid = std::fs::read_to_string(&pid_file).expect("descendant pid");
        assert!(wait_for_exit(pid.trim()), "the descendant {pid} still runs");
    }

    /// A helper that properly detaches a background process (its output
    /// redirected, so our pipe still reaches end of file) keeps it: that is
    /// how a password manager's cache daemon survives between renewals.
    #[cfg(unix)]
    #[test]
    fn a_detached_background_process_is_left_running() {
        use secrecy::ExposeSecret;

        let dir = tempfile::tempdir().expect("temp dir");
        let pid_file = dir.path().join("daemon.pid");
        let script = format!(
            "sleep 30 >/dev/null 2>&1 & echo $! > '{}'; printf 123456",
            pid_file.display()
        );

        let code = run(&argv(&["sh", "-c", &script]), Duration::from_secs(5))
            .expect("the command prints a code");
        assert_eq!(code.expose_secret(), "123456");

        let pid = std::fs::read_to_string(&pid_file).expect("daemon pid");
        let pid = pid.trim();
        let survived = alive(pid);
        // Clean up before asserting, so a failure leaves nothing behind.
        let _ = std::process::Command::new("kill").arg(pid).status();
        assert!(wait_for_exit(pid), "could not clean up {pid}");
        assert!(survived, "the detached process {pid} was killed");
    }

    /// On timeout the whole process group goes, not just the command.
    #[cfg(unix)]
    #[test]
    fn a_timeout_stops_the_whole_process_group() {
        let dir = tempfile::tempdir().expect("temp dir");
        let pid_file = dir.path().join("descendant.pid");
        let script = format!(
            "sleep 86400 & echo $! > '{}'; exec sleep 86400",
            pid_file.display()
        );

        let started = std::time::Instant::now();
        let code = run(&argv(&["sh", "-c", &script]), Duration::from_millis(300));
        assert!(code.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));

        let pid = std::fs::read_to_string(&pid_file).expect("descendant pid");
        assert!(wait_for_exit(pid.trim()), "the descendant {pid} still runs");
    }

    /// A blocking task still running when the command is done (tokio's
    /// pipe read on Windows, held open by a leftover process) must not
    /// hold up the call: the runtime is shut down without waiting for it.
    #[test]
    fn an_outstanding_blocking_read_does_not_hold_up_the_call() {
        let started = std::time::Instant::now();
        let value = block_on_detached(async {
            drop(tokio::task::spawn_blocking(|| {
                std::thread::sleep(Duration::from_secs(5));
            }));
            7
        });
        assert_eq!(value, Some(7));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "waited for the blocking task: {:?}",
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
