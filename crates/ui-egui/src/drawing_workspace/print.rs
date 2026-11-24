//! Print a drawing (Drawing › Review › Output › Print, File › Print…, Ctrl+P in Drawing): the whole
//! drawing as one PDF, a page per sheet at its true paper size (`drawing.print`), opened in the
//! system's PDF viewer to print from there. The PDF asks the viewer to print at actual size
//! (`/PrintScaling /None`), and a note says to keep it at 100 %.
//!
//! - **Desktop:** the PDF is written off the window thread, as an export is
//!   ([`crate::file_load::Kind::ExportSheet`]), into a fresh folder of its own in the system's
//!   temporary folder, readable only by the user, then opened with the default app
//!   ([`crate::Services::open_path`]).
//! - **Browser:** the PDF is made at once and opened in a new tab ([`crate::Services::open_bytes`]).
//!
//! When nothing can open it, the status says so and where the file is: printing never fails
//! silently.

use serde_json::json;

use crate::SolveApp;

/// The PDF's file name: the drawing's name, or the design's.
fn file_name(app: &SolveApp) -> String {
    let drawing = app.session.drawing("drawing.print").ok().map(|d| d.name.clone()).filter(|n| !n.is_empty());
    let name = drawing.unwrap_or_else(|| app.session.doc.name.clone());
    // Only what every file system takes.
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    format!("{}.pdf", if safe.trim().is_empty() { "Drawing" } else { safe.trim() })
}

/// A new folder in the system's temporary folder for one print's PDF, readable only by this user
/// (on Unix): made fresh each time, never one another user or another print made first.
fn private_folder() -> std::io::Result<std::path::PathBuf> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let base = std::env::temp_dir();
    for _ in 0..64 {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stamp = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let folder = base.join(format!("SolidCraft print {}-{n}-{stamp:x}", std::process::id()));
        // Only Unix sets the folder's mode, so only Unix needs a mutable builder (an unused `mut`
        // fails the Windows lint).
        #[cfg(unix)]
        let builder = {
            let mut builder = std::fs::DirBuilder::new();
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            builder
        };
        #[cfg(not(unix))]
        let builder = std::fs::DirBuilder::new();
        match builder.create(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no fresh folder for the PDF"))
}

/// Starts printing the open drawing.
pub(crate) fn start(app: &mut SolveApp) {
    let name = file_name(app);
    if app.services.open_path.is_some() {
        let folder = match private_folder() {
            Ok(folder) => folder,
            Err(e) => {
                app.set_status(format!("{}: {e}", tr!("ui.drawing_print.failed")), true);
                return;
            }
        };
        let path = folder.join(name).to_string_lossy().into_owned();
        let mut request = crate::file_load::Request::new(&path, crate::file_load::Kind::ExportSheet);
        request.options = json!({"path": path}).as_object().cloned().unwrap_or_default();
        request.print = true;
        if let Err(error) = crate::file_load::begin_request(app, request) {
            app.set_status(error, true);
        }
        return;
    }
    // In a browser: made at once, its bytes as they are, then shown in a new tab.
    let bytes = solidcraft_engine::print_pdf(&mut app.session).map_err(|e| format!("{}: {e}", tr!("ui.drawing_print.failed")));
    match (bytes, &app.services.open_bytes) {
        (Ok(bytes), Some(open)) => match open(&name, "application/pdf", &bytes) {
            Ok(()) => app.set_status(tr!("ui.drawing_print.opened"), false),
            Err(e) => app.set_status(format!("{}: {e}", tr!("ui.drawing_print.failed")), true),
        },
        (Ok(_), None) => app.set_status(tr!("ui.drawing_print.unavailable"), true),
        (Err(e), _) => app.set_status(e, true),
    }
}

/// The PDF at `path` is written: opens it in the default viewer, and says what to do next, or why
/// it couldn't and where the file is.
pub(crate) fn written(app: &mut SolveApp, path: &str) {
    let opened = match &app.services.open_path {
        Some(open) => open(path),
        None => Err(tr!("ui.drawing_print.unavailable").into()),
    };
    match opened {
        Ok(()) => app.set_status(tr!("ui.drawing_print.opened"), false),
        Err(e) => app.set_status(tr!("ui.drawing_print.not_opened", path = path, reason = &e), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn wait(h: &mut Harness) {
        let began = std::time::Instant::now();
        loop {
            h.frames(1);
            if h.app.file_load.idle() {
                break;
            }
            assert!(began.elapsed().as_secs() < 30, "printing did not finish");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn drawing_open(h: &mut Harness) {
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":40,"width":20,"height":3})).unwrap();
        super::super::enter(&mut h.app, true).unwrap();
        h.frames(4);
        h.app.run("drawing.create", json!({})).unwrap();
        h.app.run("drawing.sheet.add", json!({})).unwrap();
        h.frames(4);
    }

    /// Print on the ribbon writes the whole drawing's PDF, a page per sheet, and hands it to the
    /// system's viewer; the status says to print at actual size.
    #[test]
    fn print_opens_the_drawings_pdf_in_the_viewer() {
        let mut h = Harness::new();
        drawing_open(&mut h);
        let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = opened.clone();
        h.app.services.open_path = Some(Box::new(move |path| {
            seen.lock().unwrap().push(path.to_string());
            Ok(())
        }));
        h.app.ui.tab = "DRAWING REVIEW".into();
        h.frames(4);
        h.step(&json!({"click":{"handle":"button:drawing.print"}})).unwrap();
        wait(&mut h);
        let paths = opened.lock().unwrap().clone();
        assert_eq!(paths.len(), 1, "{paths:?}");
        assert!(paths[0].ends_with(".pdf"), "{paths:?}");
        let bytes = std::fs::read(&paths[0]).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-") && text.contains("/Count 2") && text.contains("/PrintScaling /None"));
        assert_eq!(h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default(), tr!("ui.drawing_print.opened"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let folder = std::path::Path::new(&paths[0]).parent().unwrap();
            assert_eq!(std::fs::metadata(folder).unwrap().permissions().mode() & 0o777, 0o700, "a folder only this user reads");
        }
        let _ = std::fs::remove_file(&paths[0]);
        let _ = std::fs::remove_dir(std::path::Path::new(&paths[0]).parent().unwrap());
    }

    /// With no PDF viewer to open it, the status says so and names the file: never silent.
    #[test]
    fn a_pdf_nothing_can_open_says_where_it_is() {
        let mut h = Harness::new();
        drawing_open(&mut h);
        h.app.services.open_path = Some(Box::new(|_| Err("no application is set to open PDF files".into())));
        h.app.ui.tab = "DRAWING REVIEW".into();
        h.frames(4);
        start(&mut h.app);
        wait(&mut h);
        let said = h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(said.contains("no application is set to open PDF files") && said.contains(".pdf"), "{said}");
    }
}
