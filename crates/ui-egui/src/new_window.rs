//! Tabs that break out into new windows: a design leaves this window for another process of this
//! app, started on its saved file. Two processes may run side by side: the only guard is the
//! design lock, which warns (`open_elsewhere`). The new process is given no `--control` port, so
//! it never competes for this one's.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;

/// How far below the tab bar (points) a drag must go before it tears the tab off.
const TEAR_OFF_DISTANCE: f32 = 40.0;

/// The view names `--view` accepts, one per `ViewKind`.
const VIEWS: [&str; 5] = ["model", "sketch", "sheet", "render", "animation"];

/// The view the process was asked to start on, until the window takes it.
static START_VIEW: Mutex<Option<String>> = Mutex::new(None);

/// The arguments that start another SolidCraft on `design`, on `view` when it names one
/// (`--view render`). The flag goes first so a design path can never be read as one.
pub fn command_line(exe: &Path, design: &Path, view: Option<&str>) -> Vec<OsString> {
    let mut line = vec![exe.as_os_str().to_owned()];
    if let Some(view) = view.and_then(parse_view) {
        line.push("--view".into());
        line.push(view.into());
    }
    line.push(design.as_os_str().to_owned());
    line
}

/// The view a `--view` value names (any case), or `None` for anything else.
pub fn parse_view(value: &str) -> Option<&'static str> {
    let value = value.trim();
    VIEWS.iter().copied().find(|name| name.eq_ignore_ascii_case(value))
}

/// Remember the view the command line asked to start on (an unknown name is ignored).
pub fn set_start_view(value: &str) {
    if let (Some(view), Ok(mut slot)) = (parse_view(value), START_VIEW.lock()) {
        *slot = Some(view.to_string());
    }
}

/// The view the command line asked to start on, once: the window calls it after the design loads.
pub fn take_start_view() -> Option<String> {
    START_VIEW.lock().ok().and_then(|mut slot| slot.take())
}

/// Whether a design must be saved before it can move to a new window: it has no file yet, or it
/// has unsaved changes the new window would not see.
pub fn needs_save_first(path: Option<&str>, dirty: bool) -> bool {
    dirty || path.is_none_or(|p| p.trim().is_empty())
}

/// Whether dragging a tab to `pointer` tears it off the bar: well below it, not just off its edge.
pub fn drag_out_is_tear_off(pointer: egui::Pos2, bar: egui::Rect) -> bool {
    pointer.y > bar.max.y + TEAR_OFF_DISTANCE
}

/// Start another SolidCraft on the saved `design` (on `view` when it names one), detached so it
/// outlives this window. The error is `ui.new_window.unsupported` in the browser, otherwise the
/// system's reason; the caller shows it with `ui.new_window.failed`.
pub fn open_in_new_window(design: &Path, view: Option<&str>) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (design, view);
        Err("ui.new_window.unsupported".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let line = command_line(&exe, design, view);
        let mut command = std::process::Command::new(&exe);
        command.args(&line[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        // Reap it when it ends, so it never lingers as a zombie while this window lives.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_puts_view_before_design() {
        let line = command_line(Path::new("sc"), Path::new("a b.sc"), Some("Render"));
        assert_eq!(line, ["sc", "--view", "render", "a b.sc"].map(OsString::from));
    }

    #[test]
    fn command_line_drops_unknown_view() {
        let line = command_line(Path::new("sc"), Path::new("a.sc"), Some("--sample"));
        assert_eq!(line, ["sc", "a.sc"].map(OsString::from));
        assert_eq!(command_line(Path::new("sc"), Path::new("a.sc"), None).len(), 2);
    }

    #[test]
    fn parse_view_knows_every_kind() {
        for name in VIEWS {
            assert_eq!(parse_view(&name.to_uppercase()), Some(name));
        }
        assert_eq!(parse_view(" sketch "), Some("sketch"));
        assert_eq!(parse_view(""), None);
        assert_eq!(parse_view("sketch2"), None);
    }

    #[test]
    fn start_view_is_taken_once() {
        set_start_view("nonsense");
        assert_eq!(take_start_view(), None);
        set_start_view("Render");
        assert_eq!(take_start_view().as_deref(), Some("render"));
        assert_eq!(take_start_view(), None);
    }

    #[test]
    fn unsaved_designs_save_first() {
        assert!(needs_save_first(None, false));
        assert!(needs_save_first(Some(" "), false));
        assert!(needs_save_first(Some("a.sc"), true));
        assert!(!needs_save_first(Some("a.sc"), false));
    }

    #[test]
    fn tear_off_needs_a_clear_drop_below_the_bar() {
        let bar = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 40.0));
        assert!(!drag_out_is_tear_off(egui::pos2(100.0, 60.0), bar));
        assert!(!drag_out_is_tear_off(egui::pos2(100.0, -50.0), bar));
        assert!(drag_out_is_tear_off(egui::pos2(100.0, 90.0), bar));
    }
}
