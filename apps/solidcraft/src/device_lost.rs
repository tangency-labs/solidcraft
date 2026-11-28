//! The graphics device was lost after the window opened: the driver was reset (on Windows a TDR,
//! "Display driver stopped responding and has recovered") or the GPU went away (#4172). A lost
//! device can't draw again, so the window can't show anything more, not even a message. The
//! window device's lost callback marks it (`solidcraft_viewport_gpu::DeviceLost`); the app then closes, keeping unsaved changes
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

/// Set to `"1"` on the start after an out-of-memory device loss, classified from the full
/// untruncated reason in the parent before [`REASON_ENV`] is trimmed to [`REASON_CHARS`].
pub const OOM_ENV: &str = "SOLIDCRAFT_DEVICE_LOST_OOM";

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

/// Whether `reason` reports that graphics or system memory ran out rather than a driver crash or
/// reset: switching to the software renderer (which puts GPU buffers in system RAM) would only
/// make memory exhaustion worse (#23651).
pub fn is_out_of_memory(reason: Option<&str>) -> bool {
    let Some(reason) = reason else { return false };
    let normalized = reason.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    normalized.contains("out of memory")
        || normalized.contains("outofmemory")
        || normalized.contains("out_of_memory")
        || normalized.contains("out_of_device_memory")
        || normalized.contains("out_of_host_memory")
}

/// Whether a restarted launch followed an out-of-memory loss, preferring the parent's
/// untruncated classification ([`OOM_ENV`]) and falling back to [`is_out_of_memory`] on
/// [`REASON_ENV`].
pub fn was_out_of_memory(oom_flag: Option<&str>, reason: Option<&str>) -> bool {
    oom_flag.is_some_and(|v| v.trim() == "1") || is_out_of_memory(reason)
}

/// Whether the start after a lost device uses the software renderer: when this launch already
/// followed a recent loss (the driver keeps losing it), or ran on it anyway — unless the device
/// was lost because memory ran out, where software rendering would worsen memory exhaustion
/// (#23651).
pub fn restart_on_software(after_recent_loss: bool, cpu_renderer: bool, reason: Option<&str>) -> bool {
    if is_out_of_memory(reason) {
        return false;
    }
    after_recent_loss || cpu_renderer
}

/// `reason` on one line, at most [`REASON_CHARS`] long; None when there is nothing to say.
pub fn reason(reason: Option<&str>) -> Option<String> {
    let line = reason?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then(|| line.chars().take(REASON_CHARS).collect())
}

/// What the start after a lost device tells the user, `software` when it runs on the software
/// renderer, `out_of_memory` when memory ran out (decided before [`REASON_ENV`] truncation), with
/// what the driver reported ([`REASON_ENV`]) and the designs whose unsaved changes the lost launch
/// couldn't keep ([`startup::UNKEPT_ENV`]).
pub fn note(software: bool, out_of_memory: bool, reason: Option<&str>, unkept: Option<&str>) -> String {
    let unsaved = startup::unsaved_clause(unkept, "unsaved changes were kept for recovery");
    let mut first = unsaved.chars();
    let unsaved = first.next().map(|c| c.to_uppercase().chain(first).collect::<String>()).unwrap_or_default();
    let cause =
        if out_of_memory { "Graphics or system memory ran out" } else { "The graphics device was reset (the graphics driver stopped responding)" };
    let mut note = format!("{cause}, so SolidCraft closed and started again. {unsaved}.");
    if software {
        note.push_str(" This start uses software rendering, which doesn't depend on the graphics driver.");
    }
    if let Some(reason) = self::reason(reason) {
        note.push_str(&format!(" The graphics driver reported: {reason}"));
    }
    note
}

/// The command that starts `exe` again with `args`, the loss stamped at `lost_at` and its
/// `reason`, on the software renderer when `software`, naming the designs whose unsaved changes
/// weren't kept (`unkept`, [`startup::UNKEPT_ENV`]).
fn command(
    exe: std::path::PathBuf,
    args: impl IntoIterator<Item = std::ffi::OsString>,
    software: bool,
    lost_at: u64,
    reason: &str,
    unkept: &[String],
) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    // The notice of a hang that restarted this launch isn't the next one's (`App::watch_for_a_hang`).
    command.args(args).env(LOST_ENV, lost_at.to_string());
    match self::reason(Some(reason)) {
        Some(reason) => command.env(REASON_ENV, reason),
        // Not one inherited from an earlier loss.
        None => command.env_remove(REASON_ENV),
    };
    if is_out_of_memory(Some(reason)) {
        command.env(OOM_ENV, "1");
    } else {
        command.env_remove(OOM_ENV);
    }
    match startup::unkept(unkept) {
        Some(designs) => command.env(startup::UNKEPT_ENV, designs),
        None => command.env_remove(startup::UNKEPT_ENV),
    };
    if software {
        command.env(startup::RENDERER_ENV, "cpu");
    }
    command
}

/// What a restart starts: `exe`, or the AppImage it runs from. Inside an AppImage `exe` lies in
/// the mount the AppImage's runtime made for this process ($APPDIR), which this process closing
/// may take away; the AppImage file ($APPIMAGE) mounts its own, as an update's start does
/// (`update::InstallKind::AppImage`).
fn executable(
    exe: std::path::PathBuf,
    appimage: Option<&str>,
    appdir: Option<&str>,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let appimage = appimage.map(str::trim).filter(|a| !a.is_empty()).map(std::path::PathBuf::from);
    let inside = appdir.map(str::trim).filter(|d| !d.is_empty()).is_some_and(|dir| exe.starts_with(dir));
    match appimage {
        Some(image) if inside && is_file(&image) => image,
        _ => exe,
    }
}

/// Start SolidCraft again with the same arguments, the loss stamped at `lost_at` and its `reason`,
/// on the software renderer when `software`, naming the designs whose unsaved changes weren't
/// kept (`unkept`).
pub fn restart(software: bool, lost_at: u64, reason: &str, unkept: &[String]) {
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = executable(exe, std::env::var("APPIMAGE").ok().as_deref(), std::env::var("APPDIR").ok().as_deref(), std::path::Path::is_file);
    let mut command = command(exe, std::env::args_os().skip(1), software, lost_at, reason, unkept);
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
        solidcraft_engine::note!("SolidCraft graphics: still closing after the device was lost; aborting");
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
        assert!(!super::restart_on_software(false, false, None));
        assert!(super::restart_on_software(true, false, None));
        assert!(super::restart_on_software(false, true, None));
        assert!(super::restart_on_software(true, false, Some("Lost: Device lost (vkQueueSubmit)")));
        assert!(super::note(true, false, None, None).contains("software rendering"));
        assert!(!super::note(false, false, None, None).contains("software rendering"));
        assert!(super::note(false, false, None, None).contains("Unsaved changes were kept for recovery."));
        // A design the lost launch couldn't keep is named, not promised (#3321).
        let note = super::note(false, false, None, Some("Bracket: autosave is off"));
        assert!(note.contains("Some unsaved changes couldn't be kept for recovery (Bracket: autosave is off)."), "{note}");
        assert!(!note.contains("were kept"), "{note}");
    }

    /// Out-of-memory device losses are recognized across driver/wgpu phrasings, never force
    /// software rendering (which would allocate GPU buffers in system RAM), and tell the user
    /// memory ran out instead of blaming a hung graphics driver (#23651).
    #[test]
    fn out_of_memory_does_not_force_software_rendering_or_blame_driver() {
        for oom in [
            "Out of memory",
            "DeviceLost: Out of memory",
            "GPU out of memory",
            "OutOfMemory",
            "DXGI_ERROR_DEVICE_REMOVED (E_OUTOFMEMORY)",
            "VK_ERROR_OUT_OF_DEVICE_MEMORY",
            "VK_ERROR_OUT_OF_HOST_MEMORY",
            "Out\n  of  Memory",
        ] {
            assert!(super::is_out_of_memory(Some(oom)), "expected OOM for {oom:?}");
            assert!(!super::restart_on_software(true, false, Some(oom)), "must not switch to CPU for {oom:?}");
            assert!(!super::restart_on_software(false, true, Some(oom)), "must not force CPU for {oom:?}");
            assert!(!super::restart_on_software(true, true, Some(oom)), "must not force CPU for {oom:?}");
        }
        for other in [None, Some(""), Some("   "), Some("Lost: Device lost (vkQueueSubmit)"), Some("DXGI_ERROR_DEVICE_HUNG")] {
            assert!(!super::is_out_of_memory(other), "unexpected OOM for {other:?}");
        }

        let note = super::note(false, true, Some("DeviceLost: Out of memory"), None);
        assert!(note.contains("memory"), "{note}");
        assert!(!note.contains("the graphics driver stopped responding"), "{note}");
        assert!(!note.contains("software rendering"), "{note}");
        assert!(note.contains("Unsaved changes were kept for recovery."), "{note}");
        assert!(note.ends_with("The graphics driver reported: DeviceLost: Out of memory"), "{note}");

        let unkept_note = super::note(false, true, Some("Out of memory"), Some("Bracket: autosave is off"));
        assert!(unkept_note.contains("Some unsaved changes couldn't be kept for recovery (Bracket: autosave is off)."), "{unkept_note}");
        assert!(!unkept_note.contains("the graphics driver stopped responding"), "{unkept_note}");

        // A long OOM reason whose tail is truncated past REASON_CHARS is still classified as OOM
        // via OOM_ENV set by the parent on the full reason.
        let env = |command: &std::process::Command, name: &str| {
            command.get_envs().find(|(k, _)| *k == name).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        let long_oom = format!("{} Out of memory", "x".repeat(350));
        let oom_cmd = super::command(std::path::PathBuf::from("solidcraft"), Vec::new(), false, 1_000, &long_oom, &[]);
        let truncated_reason = env(&oom_cmd, super::REASON_ENV).flatten();
        let oom_flag = env(&oom_cmd, super::OOM_ENV).flatten();
        assert_eq!(oom_flag.as_deref(), Some("1"));
        assert!(!super::is_out_of_memory(truncated_reason.as_deref()), "tail was truncated");
        assert!(super::was_out_of_memory(oom_flag.as_deref(), truncated_reason.as_deref()));
        let long_note = super::note(false, true, truncated_reason.as_deref(), None);
        assert!(long_note.starts_with("Graphics or system memory ran out"), "{long_note}");

        let driver_cmd = super::command(std::path::PathBuf::from("solidcraft"), Vec::new(), false, 1_000, "Lost: Device lost", &[]);
        assert_eq!(env(&driver_cmd, super::OOM_ENV), Some(None));
    }

    /// The start after a loss is told why (#9044): the old process's journal held the reason, and
    /// it is deleted as that process closes.
    #[test]
    fn the_next_start_is_told_why_the_device_was_lost() {
        let exe = std::path::PathBuf::from("solidcraft");
        let args = ["--control".into(), "7979".into()];
        let unkept = ["Bracket: the recovery folder is read-only".to_string()];
        let command = super::command(exe.clone(), args, true, 1_000, "Lost: Device lost\n  (vkQueueSubmit)", &unkept);
        let env = |command: &std::process::Command, name: &str| {
            command.get_envs().find(|(k, _)| *k == name).map(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(env(&command, super::LOST_ENV), Some(Some("1000".into())));
        assert_eq!(env(&command, super::REASON_ENV), Some(Some("Lost: Device lost (vkQueueSubmit)".into())), "on one line");
        assert_eq!(env(&command, crate::startup::RENDERER_ENV), Some(Some("cpu".into())));
        assert_eq!(env(&command, crate::startup::UNKEPT_ENV), Some(Some(unkept[0].clone())));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--control", "7979"]);
        // No reason, every design kept: none inherited from an earlier loss either.
        let command = super::command(exe, Vec::new(), false, 1_000, "  ", &[]);
        assert_eq!(env(&command, super::REASON_ENV), Some(None));
        assert_eq!(env(&command, crate::startup::UNKEPT_ENV), Some(None));
        assert_eq!(env(&command, crate::startup::RENDERER_ENV), None);

        let note = super::note(false, false, Some("Lost: Device lost"), None);
        assert!(note.ends_with("The graphics driver reported: Lost: Device lost"), "{note}");
        assert!(!super::note(false, false, Some(" \n "), None).contains("reported"));
        let long = "x".repeat(1_000);
        assert_eq!(super::reason(Some(&long)).map(|r| r.chars().count()), Some(300));
    }

    /// Inside an AppImage the restart starts the AppImage file, which mounts its own copy, not
    /// the binary in this process's mount; anywhere else, or with a stale $APPIMAGE inherited
    /// from another program, this executable.
    #[test]
    fn a_restart_from_an_appimage_starts_the_appimage() {
        use std::path::{Path, PathBuf};
        let mounted = PathBuf::from("/tmp/.mount_SolidCxyz/usr/bin/solidcraft");
        let image = Path::new("/home/u/SolidCraft.AppImage");
        let pick = |exe: &PathBuf, appimage, appdir| super::executable(exe.clone(), appimage, appdir, |p: &Path| p == image);
        assert_eq!(pick(&mounted, Some("/home/u/SolidCraft.AppImage"), Some("/tmp/.mount_SolidCxyz")), image);
        assert_eq!(pick(&mounted, Some("/home/u/Gone.AppImage"), Some("/tmp/.mount_SolidCxyz")), mounted, "no such file");
        assert_eq!(pick(&mounted, None, Some("/tmp/.mount_SolidCxyz")), mounted);
        let installed = PathBuf::from("/usr/bin/solidcraft");
        assert_eq!(pick(&installed, Some("/home/u/SolidCraft.AppImage"), Some("/tmp/.mount_Other")), installed, "not inside that AppImage");
        assert_eq!(pick(&installed, Some("/home/u/SolidCraft.AppImage"), None), installed);
    }
}
