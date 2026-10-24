//! Ctrl/Cmd+V in the chat's text box: files copied in the file manager attach as files, a picture
//! on the clipboard attaches as a picture, and text pastes as text.
//!
//! egui-winit answers a paste shortcut by reading the clipboard's *text*: with text it sends a
//! `Paste` event, without it nothing at all, not even the key press. So a clipboard that holds
//! only a picture or only files (a screenshot, files copied in Explorer) is seen through the V
//! key coming up (Shift+Insert likewise), which is still sent. Files and a picture are read with
//! arboard, which gives a file list on Windows, macOS and Linux (X11 and Wayland data-control).
//! Gap: macOS delivers no key-up while Cmd is held, so there a picture-only or files-only paste
//! doesn't arrive (text and files that come with text do); the + menu's "Paste a picture" works.

use std::path::{Path, PathBuf};

use super::SolveApp;

/// A picture on the clipboard: RGBA rows, as the system gives them.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// What the system clipboard can give besides text; read only when a paste needs it.
pub trait Clipboard {
    fn files(&mut self) -> Vec<PathBuf>;
    fn picture(&mut self) -> Option<Picture>;
}

/// The system's clipboard.
pub struct System;

impl Clipboard for System {
    #[cfg(not(target_os = "android"))]
    fn files(&mut self) -> Vec<PathBuf> {
        arboard::Clipboard::new().and_then(|mut c| c.get().file_list()).unwrap_or_default()
    }
    /// Android: arboard has no backend here, and the platform clipboard is not reachable from
    /// this crate. Text still pastes (egui-winit carries it); files and pictures do not.
    #[cfg(target_os = "android")]
    fn files(&mut self) -> Vec<PathBuf> {
        Vec::new()
    }
    #[cfg(not(target_os = "android"))]
    fn picture(&mut self) -> Option<Picture> {
        let i = arboard::Clipboard::new().and_then(|mut c| c.get_image()).ok()?;
        Some(Picture { width: i.width, height: i.height, rgba: i.bytes.into_owned() })
    }
    #[cfg(target_os = "android")]
    fn picture(&mut self) -> Option<Picture> {
        None
    }
}

/// What a paste does.
pub enum Paste {
    /// Leave it to the text box.
    Text,
    Files(Vec<PathBuf>),
    Picture(Picture),
    Nothing,
}

/// Decide a paste. `text` is the clipboard's text when the shortcut brought some. Files win over
/// text (file managers put the files' names on the clipboard as text too); text wins over a
/// picture (a spreadsheet's cells come with a picture of them).
pub fn decide(text: Option<&str>, clip: &mut dyn Clipboard) -> Paste {
    let files = clip.files();
    if !files.is_empty() {
        return Paste::Files(files);
    }
    if let Some(text) = text {
        let listed = files_in_text(text);
        return if listed.is_empty() { Paste::Text } else { Paste::Files(listed) };
    }
    clip.picture().map_or(Paste::Nothing, Paste::Picture)
}

/// The files a text names, when it is a list of `file://` links to files that exist (what some
/// file managers put on the clipboard) and nothing else.
pub fn files_in_text(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let Some(path) = line.strip_prefix("file://").and_then(uri_path) else { return Vec::new() };
        if !path.is_file() {
            return Vec::new();
        }
        out.push(path);
    }
    out
}

/// The path of a `file://` link's rest (`/home/a%20b`, `localhost/home/a`, `/C:/Users/a`).
fn uri_path(rest: &str) -> Option<PathBuf> {
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut raw = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = rest.get(i + 1..i + 3)?;
            raw.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            raw.push(b);
            i += 1;
        }
    }
    let s = String::from_utf8(raw).ok()?;
    // A Windows drive: `/C:/Users` is `C:/Users`.
    let drive = s.as_bytes();
    let s = if drive.get(2) == Some(&b':') && drive.get(1).is_some_and(u8::is_ascii_alphabetic) { s.get(1..)?.to_string() } else { s };
    Some(Path::new(&s).to_path_buf())
}

/// How long after a text paste the V key coming up still belongs to it.
const SAME_PASTE_SECS: f64 = 1.5;

/// The paste shortcut for the focused text box this frame. Call before the box is drawn: a paste
/// that is files is taken out of the input, so the box doesn't type their names.
pub fn handle(app: &mut SolveApp, ui: &egui::Ui) {
    let now = ui.input(|i| i.time);
    let (text, released) = ui.input(|i| {
        let text = i.events.iter().find_map(|e| if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None });
        let released = i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key, pressed: false, modifiers, .. }
                if (*key == egui::Key::V && modifiers.command) || (*key == egui::Key::Insert && modifiers.shift))
        });
        (text, released)
    });
    let after_text = app.agent_chat.text_pasted_at.is_some_and(|at| (0.0..SAME_PASTE_SECS).contains(&(now - at)));
    if text.is_none() && !(released && !after_text) {
        if released {
            app.agent_chat.text_pasted_at = None;
        }
        return;
    }
    let paste = decide(text.as_deref(), &mut System);
    if text.is_some() {
        app.agent_chat.text_pasted_at = if matches!(paste, Paste::Text) { Some(now) } else { None };
    } else {
        app.agent_chat.text_pasted_at = None;
    }
    apply(app, ui, paste, text.is_some());
}

/// Do what [`decide`] said.
fn apply(app: &mut SolveApp, ui: &egui::Ui, paste: Paste, had_text: bool) {
    match paste {
        Paste::Text | Paste::Nothing => {}
        Paste::Files(paths) => {
            if had_text {
                ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Paste(_))));
            }
            super::attach_dropped(app, paths.into_iter());
        }
        Paste::Picture(p) => {
            if let Err(e) = super::attach_rgba(app, p.width, p.height, &p.rgba) {
                app.agent_chat.notice(e, true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        files: Vec<PathBuf>,
        picture: Option<(usize, usize)>,
    }

    impl Clipboard for Fake {
        fn files(&mut self) -> Vec<PathBuf> {
            self.files.clone()
        }
        fn picture(&mut self) -> Option<Picture> {
            self.picture.map(|(width, height)| Picture { width, height, rgba: vec![255; width * height * 4] })
        }
    }

    #[test]
    fn a_picture_alone_attaches_as_a_picture() {
        let mut clip = Fake { picture: Some((2, 2)), ..Fake::default() };
        assert!(matches!(decide(None, &mut clip), Paste::Picture(p) if (p.width, p.height) == (2, 2)));
        assert!(matches!(decide(None, &mut Fake::default()), Paste::Nothing), "an empty clipboard does nothing");
    }

    #[test]
    fn copied_files_attach_even_when_their_names_came_as_text() {
        let files = vec![PathBuf::from("/a/part.step"), PathBuf::from("/a/brief.pdf")];
        let mut clip = Fake { files: files.clone(), picture: Some((1, 1)) };
        assert!(matches!(decide(None, &mut clip), Paste::Files(f) if f == files), "files alone");
        assert!(matches!(decide(Some("part.step\nbrief.pdf"), &mut clip), Paste::Files(f) if f == files), "with their names as text");
    }

    #[test]
    fn text_pastes_as_text_even_with_a_picture_beside_it() {
        let mut clip = Fake { picture: Some((1, 1)), ..Fake::default() };
        assert!(matches!(decide(Some("wall 2 mm"), &mut clip), Paste::Text));
    }

    #[test]
    fn a_text_of_file_links_attaches_the_files_but_other_text_does_not() {
        let d = std::env::temp_dir().join(format!("solidcraft-paste-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let spaced = d.join("my part.stl");
        std::fs::write(&spaced, b"solid").unwrap();
        let plain = d.join("brief.md");
        std::fs::write(&plain, b"x").unwrap();
        // A proper file URL on every system: `file:///tmp/a%20b`, `file:///C:/Users/a%20b`.
        let link = |p: &Path| {
            let path = p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/").replace(' ', "%20");
            format!("file://{}{path}", if path.starts_with('/') { "" } else { "/" })
        };
        let list = format!("# copied\n{}\r\n{}\n", link(&spaced), link(&plain));
        assert_eq!(files_in_text(&list), vec![spaced.clone(), plain.clone()]);
        assert!(matches!(decide(Some(&list), &mut Fake::default()), Paste::Files(f) if f.len() == 2));
        assert!(files_in_text(&format!("{}\nsee also", link(&plain))).is_empty(), "a line that is not a link: it is text");
        assert!(files_in_text(&link(&d.join("missing.png"))).is_empty(), "a file that isn't there: it is text");
        assert!(files_in_text(&link(&d)).is_empty(), "a folder is not a file");
        assert!(files_in_text("https://example.com/a.png").is_empty());
        assert!(files_in_text("file://host/share/a.png").is_empty(), "another machine's file");
        assert!(files_in_text("file:///a/%zz").is_empty(), "a bad escape");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn links_to_windows_drives_lose_the_leading_slash() {
        assert_eq!(uri_path("/C:/Users/a%20b/part.step"), Some(PathBuf::from("C:/Users/a b/part.step")));
        assert_eq!(uri_path("localhost/home/a"), Some(PathBuf::from("/home/a")));
        assert_eq!(uri_path("relative"), None);
    }
}
