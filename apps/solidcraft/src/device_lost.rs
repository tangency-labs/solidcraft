//! The graphics device was lost after the window opened: the driver was reset (on Windows a TDR,
//! "Display driver stopped responding and has recovered") or the GPU went away (#4172). A lost
//! device can't draw again, so the window can't show anything more, not even a message. The
//! painter stops drawing (`egui_wgpu::DeviceLost`); the app then closes, keeping unsaved changes
//! for recovery as a forced quit does, and starts again ([`restart`]), and the new start says what
//! happened ([`note`]). A device lost again soon after such a restart starts the next one on the
//! software renderer, which doesn't depend on the graphics driver.
//!
//! The old process ends without tearing down its GPU objects ([`end`]): on a lost NVIDIA device
//! destroying the window's swap chain busy-waits in the driver and never returns (#9044).

use crate::startup;

/// Set on the start after a lost device: when the device was lost (seconds since 1970).
pub const LOST_ENV: &str = "SOLIDCRAFT_DEVICE_LOST";

/// Set on the start after a lost device: why it was lost, as wgpu said it. The old process's
/// session journal, which recorded it, is gone by then.
pub const REASON_ENV: &str = "SOLIDCRAFT_DEVICE_LOST_REASON";

/// The longest reason passed on ([`REASON_ENV`]) and shown, in characters.
const REASON_CHARS: usize = 300;

/// How long the process may take to end ([`end`]) before it is aborted.
const END_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// A loss this recent (seconds) counts for the start after it: its note, and a second loss
/// within it starts on the software renderer. Inherited by later starts, an older stamp is
/// ignored.
const RECENT_SECS: u64 = 600;

/// Seconds since 1970.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether `stamp` ([`LOST_ENV`]) names a loss at most [`RECENT_SECS`] before `now`.
pub fn recent(stamp: Option<&str>, now: u64) -> bool {
    stamp.and_then(|s| s.trim().parse::<u64>().ok()).is_some_and(|at| at <= now && now - at <= RECENT_SECS)
}

/// Whether the start after a lost device uses the software renderer: when this launch already
/// followed a recent loss (the driver keeps losing it), or ran on it anyway.
pub fn restart_on_software(after_recent_loss: bool, cpu_renderer: bool) -> bool {
    after_recent_loss || cpu_renderer
}

/// `reason` on one line, at most [`REASON_CHARS`] long; None when there is nothing to say.
pub fn reason(reason: Option<&str>) -> Option<String> {
    let line = reason?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then(|| line.chars().take(REASON_CHARS).collect())
}

/// What the start after a lost device tells the user, `software` when it runs on the software
/// renderer, with what the driver reported ([`REASON_ENV`]).
pub fn note(software: bool, reason: Option<&str>) -> String {
    let mut note = String::from(
        "The graphics device was reset (the graphics driver stopped responding), so SolidCraft closed and started again. Unsaved changes were kept for recovery.",
    );
    if software {
        note.push_str(" This start uses software rendering, which doesn't depend on the graphics driver.");
    }
    if let Some(reason) = self::reason(reason) {
        note.push_str(&format!(" The graphics driver reported: {reason}"));
    }
    note
}

/// The command that starts `exe` again with `args`, the loss stamped at `lost_at` and its
/// `reason`, on the software renderer when `software`.
fn command(
    exe: std::path::PathBuf,
    args: impl IntoIterator<Item = std::ffi::OsString>,
    software: bool,
    lost_at: u64,
    reason: &str,
) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    // The notice of a hang that restarted this launch isn't the next one's (`App::watch_for_a_hang`).
    command.args(args).env(LOST_ENV, lost_at.to_string()).env_remove(startup::HUNG_ENV);
    match self::reason(Some(reason)) {
        Some(reason) => command.env(REASON_ENV, reason),
        // Not one inherited from an earlier loss.
        None => command.env_remove(REASON_ENV),
    };
    if software {
        command.env(startup::RENDERER_ENV, "cpu");
    }
    command
}

/// Start SolidCraft again with the same arguments, the loss stamped at `lost_at` and its `reason`,
/// on the software renderer when `software`.
pub fn restart(software: bool, lost_at: u64, reason: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut command = command(exe, std::env::args_os().skip(1), software, lost_at, reason);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Not tied to this process: its own process group, no console.
        command.creation_flags(0x0000_0200 | 0x0000_0008);
    }
    if let Err(e) = command.spawn() {
        log::error!("graphics: starting again after the device was lost: {e}");
    }
}

/// End the process now, without tearing down what is left on the lost device: the window's swap
/// chain, the viewport's buffers and pipelines. On NVIDIA's Vulkan driver destroying the swap
/// chain of a lost device busy-waits forever, which left the old window frozen at 100% of a core
/// beside the new one (#9044). Should the exit itself get stuck in a library's exit handler, the
/// process is aborted after [`END_GRACE`].
pub fn end() -> ! {
    let watchdog = std::thread::Builder::new().name("solidcraft-exit".into()).spawn(|| {
        std::thread::sleep(END_GRACE);
        eprintln!("SolidCraft graphics: still closing after the device was lost; aborting");
        std::process::abort();
    });
    if let Err(e) = watchdog {
        log::warn!("graphics: no exit watchdog: {e}");
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    /// Only a recent, well-formed stamp counts: a later start that inherited it says nothing.
    #[test]
    fn only_a_recent_loss_counts() {
        let now = 1_000_000;
        assert!(super::recent(Some("999990"), now));
        assert!(super::recent(Some(" 999400 "), now));
        assert!(!super::recent(Some("999399"), now), "older than ten minutes");
        assert!(!super::recent(Some("1000001"), now), "from the future");
        assert!(!super::recent(Some("soon"), now));
        assert!(!super::recent(None, now));
    }

    /// The first loss starts again on the GPU; a second one soon after, on the software renderer.
    #[test]
    fn a_second_loss_in_a_row_starts_on_the_software_renderer() {
        assert!(!super::restart_on_software(false, false));
        assert!(super::restart_on_software(true, false));
        assert!(super::restart_on_software(false, true));
        assert!(super::note(true, None).contains("software rendering"));
        assert!(!super::note(false, None).contains("software rendering"));
        assert!(super::note(false, None).contains("kept for recovery"));
    }

    /// The start after a loss is told why (#9044): the old process's journal held the reason, and
    /// it is deleted as that process closes.
    #[test]
    fn the_next_start_is_told_why_the_device_was_lost() {
        let exe = std::path::PathBuf::from("solidcraft");
        let args = ["--control".into(), "7979".into()];
        let command = super::command(exe.clone(), args, true, 1_000, "Lost: Device lost\n  (vkQueueSubmit)");
        let env = |command: &std::process::Command, name: &str| {
            command.get_envs().find(|(k, _)| *k == name).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(env(&command, super::LOST_ENV), Some(Some("1000".into())));
        assert_eq!(env(&command, super::REASON_ENV), Some(Some("Lost: Device lost (vkQueueSubmit)".into())), "on one line");
        assert_eq!(env(&command, crate::startup::RENDERER_ENV), Some(Some("cpu".into())));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--control", "7979"]);
        // No reason: none inherited from an earlier loss either.
        let command = super::command(exe, Vec::new(), false, 1_000, "  ");
        assert_eq!(env(&command, super::REASON_ENV), Some(None));
        assert_eq!(env(&command, crate::startup::RENDERER_ENV), None);

        let note = super::note(false, Some("Lost: Device lost"));
        assert!(note.ends_with("The graphics driver reported: Lost: Device lost"), "{note}");
        assert!(!super::note(false, Some(" \n ")).contains("reported"));
        let long = "x".repeat(1_000);
        assert_eq!(super::reason(Some(&long)).map(|r| r.chars().count()), Some(300));
    }
}
