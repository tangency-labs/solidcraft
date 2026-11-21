//! Opening a file in the program the system opens its kind with (a drawing's PDF in the PDF
//! viewer, to print it): `open` on macOS, `xdg-open` on Linux and the BSDs, `cmd /C start` on
//! Windows. Each says when nothing could open the file, so the window never says it opened.
//!
//! The window never waits on a viewer: an opener can stay until the viewer it started closes, so
//! it is watched for a moment only. One that fails does so at once (nothing opens the file, or the
//! file is gone); one still running, or done without an error, has started a viewer.

/// Opens `path` with the program the system opens its kind with, or says why it couldn't.
pub fn open_with_default_app(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).is_file() {
        return Err(format!("{path} isn't there"));
    }
    let mut command = opener(path)?;
    let mut child = command.spawn().map_err(|e| format!("{}: {e}", command.get_program().to_string_lossy()))?;
    let until = std::time::Instant::now() + std::time::Duration::from_millis(400);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(solidcraft_ui_text::tr("ui.drawing_print.no_viewer").to_string()),
            Ok(None) if std::time::Instant::now() < until => std::thread::sleep(std::time::Duration::from_millis(20)),
            Ok(None) => {
                std::thread::spawn(move || child.wait());
                return Ok(());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The command that opens `path`. On Windows `start` is a `cmd` built-in: its first quoted
/// argument is the window title (empty here), and the path goes quoted after it. A path holding
/// a quote can't be quoted for `cmd`, so it is refused rather than mangled.
#[cfg(target_os = "windows")]
fn opener(path: &str) -> Result<std::process::Command, String> {
    use std::os::windows::process::CommandExt;
    if path.contains('"') {
        return Err(format!("{path} can't be opened: its name holds a quote"));
    }
    let mut command = std::process::Command::new("cmd");
    command.raw_arg(format!("/C start \"\" \"{path}\""));
    // No console window flashes up.
    command.creation_flags(0x08000000);
    Ok(command)
}

#[cfg(not(target_os = "windows"))]
fn opener(path: &str) -> Result<std::process::Command, String> {
    let mut command = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" });
    command.arg(path);
    Ok(command)
}
