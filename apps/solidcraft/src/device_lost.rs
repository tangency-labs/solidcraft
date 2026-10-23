//! The graphics device was lost after the window opened: the driver was reset (on Windows a TDR,
//! "Display driver stopped responding and has recovered") or the GPU went away (#4172). A lost
//! device can't draw again, so the window can't show anything more, not even a message. The
//! painter stops drawing (`egui_wgpu::DeviceLost`); the app then closes, keeping unsaved changes
//! for recovery as a forced quit does, and starts again ([`restart`]), and the new start says what
//! happened ([`note`]). A device lost again soon after such a restart starts the next one on the
//! software renderer, which doesn't depend on the graphics driver.

use crate::startup;

/// Set on the start after a lost device: when the device was lost (seconds since 1970).
pub const LOST_ENV: &str = "SOLIDCRAFT_DEVICE_LOST";

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

/// What the start after a lost device tells the user, `software` when it runs on the software
/// renderer.
pub fn note(software: bool) -> String {
    let mut note = String::from(
        "The graphics device was reset (the graphics driver stopped responding), so SolidCraft closed and started again. Unsaved changes were kept for recovery.",
    );
    if software {
        note.push_str(" This start uses software rendering, which doesn't depend on the graphics driver.");
    }
    note
}

/// Start SolidCraft again with the same arguments, the loss stamped at `lost_at`, on the software
/// renderer when `software`.
pub fn restart(software: bool, lost_at: u64) {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1)).env(LOST_ENV, lost_at.to_string());
    if software {
        command.env(startup::RENDERER_ENV, "cpu");
    }
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
        assert!(super::note(true).contains("software rendering"));
        assert!(!super::note(false).contains("software rendering"));
        assert!(super::note(false).contains("kept for recovery"));
    }
}
