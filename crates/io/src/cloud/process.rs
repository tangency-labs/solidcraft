//! Bounded native Git subprocesses. Process-wrap owns platform process groups and job objects.
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use process_wrap::std::{ChildWrapper, CommandWrap};

type Result<T> = std::io::Result<T>;

struct Child(Box<dyn ChildWrapper>);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn read(pipe: impl Read, limit: usize, overflow: &AtomicBool) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.take(limit.saturating_add(1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        overflow.store(true, Ordering::Relaxed);
    }
    Ok(bytes)
}

pub(super) fn run(mut command: Command, input: &[u8], cancel: impl Fn() -> bool, timeout: Duration, limit: usize) -> Result<Vec<u8>> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut command = CommandWrap::from(command);
    #[cfg(unix)]
    command.wrap(process_wrap::std::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::std::JobObject);
    let child = Child(command.spawn()?);
    std::thread::scope(|scope| {
        // The guard is owned inside the scope so an early error kills the group before
        // scoped threads are joined. Git, SSH, hooks, and helpers share that lifetime.
        let mut child = child;
        let stdin = child.0.stdin().take();
        let stdout = child.0.stdout().take().ok_or_else(|| std::io::Error::other("missing output pipe"))?;
        let stderr = child.0.stderr().take().ok_or_else(|| std::io::Error::other("missing error pipe"))?;
        let overflow = Arc::new(AtomicBool::new(false));
        let out_overflow = overflow.clone();
        let err_overflow = overflow.clone();
        let out = std::thread::Builder::new().name("cloud-git-output".into()).spawn_scoped(scope, move || read(stdout, limit, &out_overflow))?;
        let err = std::thread::Builder::new().name("cloud-git-error".into()).spawn_scoped(scope, move || read(stderr, limit, &err_overflow))?;
        let write = std::thread::Builder::new().name("cloud-git-input".into()).spawn_scoped(scope, move || {
            if let Some(mut stdin) = stdin {
                stdin.write_all(input)?;
            }
            Ok::<_, std::io::Error>(())
        })?;
        let started = Instant::now();
        let status = loop {
            if cancel() {
                return Err(std::io::Error::other("cancelled"));
            }
            if started.elapsed() >= timeout {
                return Err(std::io::Error::other("timed out"));
            }
            if overflow.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("output exceeded its safety limit"));
            }
            if let Some(status) = child.0.try_wait()?
                && out.is_finished()
                && err.is_finished()
                && write.is_finished()
            {
                break status;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // Always join every worker even when one reports an error or a panic.
        let output = out.join();
        let errors = err.join();
        let written = write.join();
        let output = output.map_err(|_| std::io::Error::other("output worker stopped"))??;
        errors.map_err(|_| std::io::Error::other("error worker stopped"))??;
        written.map_err(|_| std::io::Error::other("input worker stopped"))??;
        if output.len() > limit || overflow.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("output exceeded its safety limit"));
        }
        if !status.success() {
            return Err(std::io::Error::other("subprocess failed"));
        }
        Ok(output)
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn large_stdout_and_stderr_cannot_deadlock() {
        let start = Instant::now();
        assert!(run(shell("yes x"), b"", || false, Duration::from_secs(3), 1024).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(run(shell("yes x >&2"), b"", || false, Duration::from_secs(3), 1024).is_err());
    }

    #[test]
    fn cancellation_kills_descendants_holding_pipes() {
        let start = Instant::now();
        let result = run(shell("sleep 30 & wait"), b"", || start.elapsed() > Duration::from_millis(50), Duration::from_secs(3), 1024);
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn deadline_kills_descendants_holding_pipes() {
        let start = Instant::now();
        assert!(run(shell("sleep 30 & wait"), b"", || false, Duration::from_millis(50), 1024).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stdin_and_both_output_pipes_are_drained() {
        let bytes = vec![b'x'; 100_000];
        assert_eq!(run(shell("cat; printf diagnostic >&2"), &bytes, || false, Duration::from_secs(3), 200_000).unwrap(), bytes);
    }
}
