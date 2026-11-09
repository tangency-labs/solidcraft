//! The agent as a child process (desktop only): its stdout is read on a thread and handed to the
//! panel line by line, its stdin written on another so a slow agent never blocks a frame.
//!
//! Agents start helpers of their own (Node adapters start the CLI they wrap, which starts the MCP
//! servers it was given), so stopping only the program we started would leave those running. The
//! agent therefore gets a process group of its own on Unix (on Windows, `taskkill /T` walks its
//! tree), and [`AgentProcess::shutdown`] ends the whole group: stdin is closed first (an ACP agent
//! ends at end of input), then the group is asked to stop, then stopped. Everything is addressed
//! by the ids of the processes we started, never by name.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[cfg(unix)]
use nix::sys::signal::{Signal, killpg};
#[cfg(unix)]
use nix::unistd::Pid;

/// Longest line read from the agent; a longer one is dropped.
const MAX_LINE: usize = 8 << 20;
/// Lines of the agent's stderr kept to explain a failed start.
const STDERR_LINES: usize = 40;
/// How long an agent gets to end by itself once its input is closed, and then once asked to stop.
const GRACE: Duration = Duration::from_millis(1200);

pub enum FromAgent {
    Line(String),
    /// stdout closed: the agent ended. Its last stderr lines, for the panel.
    Exited(Vec<String>),
}

pub struct AgentProcess {
    child: ProcessHandle,
    to_agent: Option<Sender<String>>,
    pub from_agent: Receiver<FromAgent>,
}

/// Ends the process from another thread than the one that talks to it.
#[derive(Clone)]
pub struct ProcessHandle(std::sync::Arc<std::sync::Mutex<Option<Child>>>);

impl ProcessHandle {
    pub fn pid(&self) -> Option<u32> {
        self.0.lock().ok().and_then(|c| c.as_ref().map(Child::id))
    }
    /// End it and everything it started; `wait`: block until done.
    pub fn shutdown(&self, wait: bool) {
        let Some(child) = self.0.lock().ok().and_then(|mut c| c.take()) else { return };
        if wait {
            end_tree(child);
        } else {
            std::thread::spawn(move || end_tree(child));
        }
    }
}

/// Start the agent in a process group of its own (Unix) or without a console window (Windows).
fn detach_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up for a console agent.
        cmd.creation_flags(0x0800_0000);
    }
}

/// Signal the agent's whole process group (`signal`: `TERM` or `KILL`). The group's id is the
/// agent's own process id, as it was started with `process_group(0)`.
#[cfg(unix)]
fn signal_group(pgid: u32, signal: &str) {
    if pgid <= 1 {
        return;
    }
    let Ok(pgid) = i32::try_from(pgid) else { return };
    let signal = match signal {
        "KILL" => Signal::SIGKILL,
        _ => Signal::SIGTERM,
    };
    // The group is signalled with the system call, not with a `kill` program: busybox' `kill`
    // takes no `--` and signals nothing, and the shell's builtin parses the group differently
    // from one shell to the next (#4587).
    let _ = killpg(Pid::from_raw(pgid), signal);
}

/// Windows: end the agent and every process it started.
#[cfg(windows)]
fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Wait up to `limit` for the child to end; whether it did.
fn wait_for(child: &mut Child, limit: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => return false,
            Err(_) => return true,
        }
    }
}

/// End the agent and everything it started (see the module note). Blocks for at most about
/// three seconds.
fn end_tree(mut child: Child) {
    let pid = child.id();
    // Its input is already closed: a well-behaved agent ends now and stops its own helpers.
    let ended = wait_for(&mut child, GRACE);
    #[cfg(unix)]
    {
        // Helpers can outlive the agent: the group is signalled whether or not it ended. The
        // group id stays reserved while any member lives, so it can't name someone else's.
        signal_group(pid, "TERM");
        if !ended {
            wait_for(&mut child, GRACE);
        }
        signal_group(pid, "KILL");
    }
    #[cfg(windows)]
    {
        if !ended {
            kill_tree(pid);
        }
    }
    #[cfg(not(any(unix, windows)))]
    let _ = (pid, ended);
    let _ = child.kill();
    let _ = child.wait();
}

impl AgentProcess {
    /// Start `argv` (program first) in `cwd`; `wake` is called whenever a line arrives.
    /// `env_remove`: variables the agent must not inherit.
    pub fn spawn(
        argv: &[String],
        cwd: &std::path::Path,
        env_remove: &[&str],
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<AgentProcess, String> {
        let Some((program, args)) = argv.split_first() else { return Err("no agent command".into()) };
        let mut cmd = super::discover::command_for(std::path::Path::new(program));
        cmd.args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for v in env_remove {
            cmd.env_remove(v);
        }
        detach_group(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
        let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            end_tree(child);
            return Err("the agent's pipes could not be opened".into());
        };
        let (to_agent, rx) = channel::<String>();
        std::thread::spawn(move || {
            for line in rx {
                if stdin.write_all(line.as_bytes()).and_then(|()| stdin.write_all(b"\n")).and_then(|()| stdin.flush()).is_err() {
                    break;
                }
            }
            // The sender is gone: stdin closes here, which is how an ACP agent learns to end.
        });
        let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
        let tail_w = tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Ok(mut t) = tail_w.lock() {
                    if t.len() >= STDERR_LINES {
                        t.pop_front();
                    }
                    t.push_back(line.chars().take(400).collect());
                }
            }
        });
        let (tx, from_agent) = channel::<FromAgent>();
        let wake = std::sync::Arc::new(wake);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut buf = Vec::new();
                match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if buf.len() > MAX_LINE => {
                        // Skip the rest of an oversized line.
                        let mut rest = Vec::new();
                        if reader.read_until(b'\n', &mut rest).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {
                        if tx.send(FromAgent::Line(String::from_utf8_lossy(&buf).into_owned())).is_err() {
                            return;
                        }
                        wake();
                    }
                }
            }
            // Give stderr a moment to deliver the agent's last words.
            std::thread::sleep(Duration::from_millis(100));
            let lines = tail.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default();
            let _ = tx.send(FromAgent::Exited(lines));
            wake();
        });
        Ok(AgentProcess { child: ProcessHandle(std::sync::Arc::new(std::sync::Mutex::new(Some(child)))), to_agent: Some(to_agent), from_agent })
    }

    pub fn send(&self, msg: &serde_json::Value) {
        if let Some(t) = &self.to_agent {
            let _ = t.send(msg.to_string());
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.pid()
    }

    pub fn handle(&self) -> ProcessHandle {
        self.child.clone()
    }

    /// End the agent and its helpers. `wait`: block until they are gone (quitting the app);
    /// otherwise the ending runs on a thread of its own and this returns at once.
    pub fn shutdown(&mut self, wait: bool) {
        self.to_agent = None;
        self.child.shutdown(wait);
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        self.shutdown(false);
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    fn alive(pid: u32) -> bool {
        // `kill -0` checks that the process exists without touching it. A zombie still counts
        // on Linux, so /proc's state is checked there too.
        #[cfg(target_os = "linux")]
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            return !stat.rsplit(')').next().is_some_and(|s| s.trim_start().starts_with('Z'));
        }
        Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }

    /// Start a fake agent that leaves helpers behind, read the helpers' ids from it, end it.
    fn helpers_after_shutdown(script: &str) -> Vec<u32> {
        let dir = std::env::temp_dir();
        let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string()];
        let mut p = AgentProcess::spawn(&argv, &dir, &[], || {}).unwrap();
        let mut pids = vec![p.pid().unwrap()];
        while pids.len() < 3 {
            match p.from_agent.recv_timeout(Duration::from_secs(10)).unwrap() {
                FromAgent::Line(l) => pids.push(l.trim().parse().unwrap()),
                FromAgent::Exited(tail) => panic!("the fake agent ended early: {tail:?}"),
            }
        }
        assert!(pids.iter().all(|&pid| alive(pid)), "all running before the shutdown: {pids:?}");
        p.shutdown(true);
        // Orphaned helpers are reaped by init a moment after they end.
        let start = Instant::now();
        while pids.iter().any(|&pid| alive(pid)) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        pids.into_iter().filter(|&pid| alive(pid)).collect()
    }

    /// No orphans: an agent that started helpers (as Node adapters start the CLI they wrap and
    /// its MCP servers) is ended with all of them, whether it ends politely at end of input or
    /// ignores both its input and the request to stop.
    #[test]
    fn stopping_the_agent_leaves_no_helpers_running() {
        // A polite agent: ends when its input closes; its helpers would live on by themselves.
        let polite = "sleep 300 & echo $!; sleep 300 & echo $!; cat >/dev/null";
        assert_eq!(helpers_after_shutdown(polite), Vec::<u32>::new(), "polite agent");
        // A stubborn one: ignores end of input and TERM (so do its helpers).
        let stubborn = "trap '' TERM; (trap '' TERM; sleep 300) & echo $!; (trap '' TERM; sleep 300) & echo $!; while :; do sleep 1; done";
        assert_eq!(helpers_after_shutdown(stubborn), Vec::<u32>::new(), "stubborn agent");
    }

    /// Dropping the process (closing the panel's connection any other way) ends it too.
    #[test]
    fn dropping_the_process_ends_its_group() {
        let argv = vec!["sh".to_string(), "-c".to_string(), "sleep 300 & echo $!; wait".to_string()];
        let p = AgentProcess::spawn(&argv, &std::env::temp_dir(), &[], || {}).unwrap();
        let leader = p.pid().unwrap();
        let FromAgent::Line(l) = p.from_agent.recv_timeout(Duration::from_secs(10)).unwrap() else { panic!("no helper id") };
        let helper: u32 = l.trim().parse().unwrap();
        drop(p);
        let start = Instant::now();
        while (alive(helper) || alive(leader)) && start.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!alive(helper) && !alive(leader), "leader {leader} or helper {helper} still running");
    }
}
