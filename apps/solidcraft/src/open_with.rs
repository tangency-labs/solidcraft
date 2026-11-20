//! Opening a file in the program the system opens its kind with (a drawing's PDF in the PDF
//! viewer, to print it): `open` on macOS, `xdg-open` on Linux and the BSDs, `ShellExecuteW` on
//! Windows. Each says when nothing could open the file, so the window never says it opened.
//!
//! The window never waits on a viewer: `xdg-open` can stay until the viewer it started closes, so
//! it is watched for a moment only. One that fails does so at once (nothing opens the file); one
//! still running has started a viewer. `ShellExecuteW` returns once the program is started, and
//! its result names what went wrong (no program for the file's kind, access denied, …); calling
//! it has no safe API, hence the `unsafe_code` allowance on [`shell_open`].

/// Opens `path` with the program the system opens its kind with, or says why it couldn't.
pub fn open_with_default_app(path: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        shell_open(path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let mut child = std::process::Command::new(opener).arg(path).spawn().map_err(|e| format!("{opener}: {e}"))?;
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
                Err(e) => return Err(format!("{opener}: {e}")),
            }
        }
    }
}

/// `path` opened with the `open` verb of its file kind's association.
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
fn shell_open(path: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(path));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call; no window, parameters
    // or directory are passed (null is allowed for each). The call only starts a program.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    // Above 32 is success; at or below it, the error (SE_ERR_NOASSOC is 31).
    match result as isize {
        code if code > 32 => Ok(()),
        31 => Err(solidcraft_ui_text::tr("ui.drawing_print.no_viewer").to_string()),
        code => Err(format!("Windows couldn't open it (error {code})")),
    }
}
