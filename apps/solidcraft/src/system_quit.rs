//! The system asks SolidCraft to stop (#7345): a shutdown or restart (systemd sends SIGTERM), the
//! session ending (SIGHUP), or Ctrl+C in a terminal (SIGINT). Their default action kills the
//! process on the spot, so `on_exit` never ran, the crash journal stayed behind and the next start
//! reported a crash. Now the first such signal closes the app as a forced quit does: unsaved
//! designs are kept for recovery, the session ends cleanly and nothing is reported. A second one
//! takes the signal's default action at once, so a window thread that is stuck can't keep the
//! process from stopping (that run is then reported as the unclean exit it is).

use std::sync::atomic::{AtomicBool, Ordering};

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

/// Set by the first signal; the window thread closes the app when it sees it.
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether the system has asked the app to stop.
pub fn requested() -> bool {
    REQUESTED.load(Ordering::SeqCst)
}

/// Listen for the signals on a thread of their own. `wake` runs on that thread after the first
/// one, to have the window thread draw a frame (an idle app draws none) and see [`requested`].
pub fn install(wake: impl Fn() + Send + 'static) -> std::io::Result<()> {
    let mut signals = Signals::new([SIGTERM, SIGHUP, SIGINT])?;
    std::thread::Builder::new().name("system quit".into()).spawn(move || {
        for signal in signals.forever() {
            if REQUESTED.swap(true, Ordering::SeqCst) {
                // Asked again: stop now, as the signal would have without us.
                let _ = signal_hook::low_level::emulate_default_handler(signal);
            } else {
                log::info!("signal {signal}: closing, unsaved changes kept for recovery");
                wake();
            }
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use signal_hook::consts::SIGTERM;

    const CHILD: &str = "SOLIDCRAFT_SYSTEM_QUIT_CHILD";
    const SIGNALS: &str = "SOLIDCRAFT_SYSTEM_QUIT_SIGNALS";

    /// The child half: crash reporting and the handler as the app installs them, then the number
    /// of SIGTERMs `SOLIDCRAFT_SYSTEM_QUIT_SIGNALS` says. After the first, it ends the session as
    /// the app's `on_exit` does once the window has closed.
    #[test]
    fn child() {
        let Some(dir) = std::env::var_os(CHILD) else { return };
        let build = solidcraft_engine::crash::Build { version: "9.9.9", commit: "abc1234", built: "2026-10-27" };
        let fresh = solidcraft_engine::crash::install(PathBuf::from(dir), build);
        println!("FRESH {}", fresh.len());
        solidcraft_engine::crash::phase(solidcraft_engine::crash::RUNNING);
        let (woke, wait) = std::sync::mpsc::channel();
        super::install(move || {
            let _ = woke.send(());
        })
        .unwrap();
        let count: usize = std::env::var(SIGNALS).ok().and_then(|n| n.parse().ok()).unwrap_or(0);
        if count > 0 {
            signal_hook::low_level::raise(SIGTERM).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).expect("the first signal wakes the window thread");
            assert!(super::requested());
        }
        if count > 1 {
            signal_hook::low_level::raise(SIGTERM).unwrap();
            // The second signal ends the process; this only runs if it didn't.
            std::thread::sleep(Duration::from_secs(10));
            println!("STILL RUNNING");
        }
        solidcraft_engine::crash::end_session();
    }

    fn run(dir: &Path, signals: usize) -> std::process::Output {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "system_quit::tests::child", "--test-threads=1", "--nocapture"])
            .env(CHILD, dir)
            .env(SIGNALS, signals.to_string())
            .output()
            .unwrap()
    }

    fn fresh_reports(out: &std::process::Output) -> Option<usize> {
        // The test harness prints "test …child ... " before it on the same line.
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout.lines().find_map(|l| l.split_once("FRESH ")).and_then(|(_, n)| n.split_whitespace().next()?.parse().ok())
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-system-quit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_sigterm_closes_cleanly_and_the_next_start_reports_nothing() {
        let dir = scratch("once");
        let out = run(&dir, 1);
        assert!(out.status.success(), "{out:?}");
        let next = run(&dir, 0);
        assert!(next.status.success(), "{next:?}");
        assert_eq!(fresh_reports(&next), Some(0), "{next:?}");
        assert!(solidcraft_engine::crash::list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_sigterm_stops_the_app_at_once_and_counts_as_unclean() {
        let dir = scratch("twice");
        let out = run(&dir, 2);
        assert_eq!(out.status.signal(), Some(SIGTERM), "{out:?}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("STILL RUNNING"), "{out:?}");
        let next = run(&dir, 0);
        assert_eq!(fresh_reports(&next), Some(1), "{next:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
