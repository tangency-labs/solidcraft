//! The desktop app's watch on the open design's linked designs (Link Parameters): when a linked
//! design changes on disk, goes missing or can't be read, the Parameters button and the
//! Browser's Parameters row get the "needs attention" mark. Nothing is updated by itself; the
//! user decides when to run Update.
//!
//! Every [`POLL`] a thread looks at each linked file's length and modification time, and only
//! when one moved does it read the files and compare their values (`linked_design_states`, the
//! rule `parameters.links` uses). The window never touches the disk, so a slow or syncing cloud
//! folder can't stall it. The browser has no files to watch: there the mark never shows.

use solidcraft_engine::LinkedState;
use solidcraft_engine::doc::LinkedDesign;

use crate::SolveApp;

/// How often the linked files are looked at.
#[cfg(not(target_arch = "wasm32"))]
const POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// A file's identity for change detection: its length and modification time.
type Stamp = Option<(u64, std::time::SystemTime)>;

/// What the watch knows about the open design's linked designs.
#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct LinkedWatch {
    /// The linked designs and their files' stamps as last checked.
    checked: Vec<LinkedDesign>,
    stamps: Vec<Stamp>,
    next_poll: Option<web_time::Instant>,
    job: Option<std::sync::mpsc::Receiver<(Vec<Stamp>, Option<Vec<LinkedState>>)>>,
    /// The linked designs that need attention, as last worked out.
    pub stale: Vec<LinkedState>,
    /// How many times the files were read and compared (for the tests).
    pub reads: u32,
}

impl LinkedWatch {
    /// What the mark says when hovered: each linked design that needs attention and why.
    pub fn reason(&self) -> Option<String> {
        if self.stale.is_empty() {
            return None;
        }
        let lines: Vec<String> = self
            .stale
            .iter()
            .map(|s| {
                format!("{}: {}", crate::params_dialog::design_title(&s.file), crate::i18n::tr_label(crate::params_dialog::state_label(s.state)))
            })
            .collect();
        Some(lines.join("\n"))
    }
}

/// Once a frame: collect a finished check, start the next when due, and keep the mark current.
pub fn step(app: &mut SolveApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let w = &mut app.linked_watch;
        if let Some(rx) = &w.job {
            match rx.try_recv() {
                Ok((stamps, states)) => {
                    w.stamps = stamps;
                    if let Some(states) = states {
                        w.reads += 1;
                        let stale: Vec<LinkedState> = states.into_iter().filter(LinkedState::needs_attention).collect();
                        if stale != w.stale {
                            w.stale = stale;
                            // The Parameters window lists the states again.
                            if let Some(d) = app.params.as_mut() {
                                d.relist();
                            }
                        }
                    }
                    app.linked_watch.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => app.linked_watch.job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let designs = &app.session.doc.linked_designs;
        let w = &mut app.linked_watch;
        if designs.is_empty() {
            w.checked.clear();
            w.stamps.clear();
            w.stale.clear();
        } else {
            let now = web_time::Instant::now();
            let changed = w.checked != *designs;
            if w.job.is_none() && (changed || w.next_poll.is_none_or(|t| now >= t)) {
                w.next_poll = Some(now + POLL);
                // Values just read into the design are compared again, whatever the stamps say.
                let before = if changed { Vec::new() } else { w.stamps.clone() };
                w.checked = designs.clone();
                let list = designs.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let wake = ctx.clone();
                let spawned = std::thread::Builder::new().name("linked-designs".into()).spawn(move || {
                    let stamps: Vec<Stamp> = list.iter().map(|d| stamp(&d.path)).collect();
                    let states = (stamps != before).then(|| solidcraft_engine::linked_design_states(&list));
                    if tx.send((stamps, states)).is_ok() {
                        wake.request_repaint();
                    }
                });
                if spawned.is_ok() {
                    w.job = Some(rx);
                }
            }
            ctx.request_repaint_after(POLL);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = ctx;
    match app.linked_watch.reason() {
        Some(why) => app.attention.insert("parameters.change", why),
        None => app.attention.remove("parameters.change"),
    };
}

#[cfg(not(target_arch = "wasm32"))]
fn stamp(path: &str) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use serde_json::json;

    /// Frames until the watch has read and compared the files `reads` times in all.
    fn until_read(h: &mut crate::scenario::Harness, reads: u32) {
        h.app.linked_watch.next_poll = None;
        for _ in 0..500 {
            h.frames(1);
            if h.app.linked_watch.reads >= reads && h.app.linked_watch.job.is_none() {
                h.frames(2);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the linked designs were not checked");
    }

    /// The Parameters button gets the mark when a linked design changes on disk or goes
    /// missing, and loses it once the design is updated; nothing updates by itself.
    #[test]
    fn a_changed_or_missing_linked_design_marks_the_parameters_button() {
        let dir = std::env::temp_dir().join(format!("solidcraft-linked-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("standard.solidcraft");
        let mut standard = solidcraft_engine::Session::default();
        standard.execute("parameters.add", &json!({"name": "Hole", "expression": "8.2 mm"})).unwrap();
        standard.execute("file.save", &json!({"path": file})).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.link_design", json!({"path": file})).unwrap();
        until_read(&mut h, 1);
        assert!(h.app.attention.is_empty(), "{:?}", h.app.attention);
        assert!(crate::scenario::handle_at("attention:parameters.change").is_none());
        // The standard changes on disk: the mark appears, with why; the value stays.
        standard.execute("parameters.change", &json!({"name": "Hole", "expression": "8.5 mm"})).unwrap();
        standard.execute("file.save", &json!({"path": file})).unwrap();
        until_read(&mut h, 2);
        let why = h.app.attention.get("parameters.change").cloned().unwrap_or_default();
        assert!(why.contains("standard") && why.contains("Changed since last update"), "{why}");
        assert!(crate::scenario::handle_at("attention:parameters.change").is_some(), "the ribbon button shows the mark");
        assert_eq!(h.app.session.doc.linked_param("Hole").map(|(_, p)| p.expr.clone()).as_deref(), Some("8.2 mm"));
        // Updating takes the mark away.
        h.app.run("parameters.update_links", json!({})).unwrap();
        until_read(&mut h, 3);
        assert!(h.app.attention.is_empty(), "{:?}", h.app.attention);
        // A linked design that goes missing is marked too.
        std::fs::remove_file(&file).unwrap();
        until_read(&mut h, 4);
        let why = h.app.attention.get("parameters.change").cloned().unwrap_or_default();
        assert!(why.contains("File not found"), "{why}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
