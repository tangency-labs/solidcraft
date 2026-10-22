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
        let errors = errors.map_err(|_| std::io::Error::other("error worker stopped"))??;
        written.map_err(|_| std::io::Error::other("input worker stopped"))??;
        if output.len() > limit || overflow.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("output exceeded its safety limit"));
        }
        if !status.success() {
            let diagnostic = diagnostic_tail(&errors);
            return Err(std::io::Error::other(if diagnostic.is_empty() {
                "subprocess failed".into()
            } else {
                format!("subprocess failed: {diagnostic}")
            }));
        }
        Ok(output)
    })
}

/// Sanitize before taking the tail so a truncated URL cannot expose its userinfo.
/// This is shared by Git, setup, and credential-helper subprocess failures. Stdout
/// is deliberately excluded because helpers can return passwords there.
fn diagnostic_tail(bytes: &[u8]) -> String {
    let text: String = String::from_utf8_lossy(bytes).chars().filter(|c| !c.is_control() || c.is_whitespace()).collect();
    let mut redacted = String::new();
    let mut remaining = text.as_str();
    while let Some((prefix, suffix)) = remaining.split_once("://") {
        redacted.push_str(prefix);
        redacted.push_str("://");
        let end = suffix.find(|c: char| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '<' | '>')).unwrap_or(suffix.len());
        let authority = suffix.get(..end).unwrap_or_default();
        if let Some((_, host)) = authority.rsplit_once('@') {
            redacted.push_str("[redacted]@");
            redacted.push_str(host);
        } else {
            redacted.push_str(authority);
        }
        remaining = suffix.get(end..).unwrap_or_default();
    }
    redacted.push_str(remaining);
    let text: String = redacted.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let mut start = text.len().saturating_sub(512);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text.get(start..).unwrap_or_default().trim().into()
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
    fn failed_commands_report_only_bounded_redacted_stderr() {
        let error = run(
            shell("printf stdout-secret; printf 'Permission denied (publickey)\\nhttps://user:password@example.test/repo\\033' >&2; exit 1"),
            b"",
            || false,
            Duration::from_secs(3),
            8192,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Permission denied (publickey)"));
        assert!(error.contains("https://[redacted]@example.test/repo"));
        assert!(!error.contains("stdout-secret") && !error.contains("password") && !error.contains("user:"));
        assert!(!error.chars().any(char::is_control));
        assert_eq!(run(shell("exit 1"), b"", || false, Duration::from_secs(3), 8192).unwrap_err().to_string(), "subprocess failed");

        let long_secret = "secret".repeat(300);
        let diagnostic = diagnostic_tail(format!("https://user:{long_secret}@example.test/repo ssh://git:pa'ss@host/repo").as_bytes());
        assert_eq!(diagnostic, "https://[redacted]@example.test/repo ssh://[redacted]@host/repo");
        let diagnostic = diagnostic_tail(format!("{} denied", "界".repeat(600)).as_bytes());
        assert!(diagnostic.len() <= 512 && diagnostic.ends_with(" denied"));
        assert!(diagnostic_tail(b"\n\t\x1b").is_empty());
        assert!(diagnostic_tail(b"bad UTF-8: \xff").contains('\u{fffd}'));
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
