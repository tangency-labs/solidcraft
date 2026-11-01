//! The runs and folders opened and folded in the History (the bar and the History beside the
//! browser) belong to the design they were chosen in, not to every design. A design keeps its
//! choices in its tab while it is open and, once it has a file, between runs too: they are
//! remembered by file in the preferences for the [`DESIGNS_MAX`] designs used most recently. A
//! design opened, or made afresh, starts from the choices remembered for its file, or none.
//!
//! The choices are remembered as they are made, so a design replaced in its tab (opening another
//! file there) has already left its own behind. A save gives the design's choices to the file it
//! was saved as, so a new design or a copy saved under another name keeps what it showed.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::SolveApp;

/// Designs whose choices are remembered between runs; the least recently used is forgotten first.
const DESIGNS_MAX: usize = 256;
/// Choices of one design read from the preferences.
const CHOICES_MAX: usize = 10_000;
/// Keys the History once saved for all designs at once, with the browser's folders.
const SHARED_ONCE: [&str; 2] = [crate::timeline_fold::KEY, crate::timeline_folders::KEY];

/// One design's choices: the History keys of the runs and folders opened and folded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Folds {
    open: BTreeSet<String>,
    folded: BTreeSet<String>,
}

impl Folds {
    /// The choice made for `key`: opened (true), folded (false), or none.
    fn get(&self, key: &str) -> Option<bool> {
        if self.open.contains(key) {
            Some(true)
        } else if self.folded.contains(key) {
            Some(false)
        } else {
            None
        }
    }

    fn set(&mut self, key: &str, open: bool) {
        self.open.remove(key);
        self.folded.remove(key);
        if open {
            self.open.insert(key.to_string());
        } else {
            self.folded.insert(key.to_string());
        }
    }

    fn is_empty(&self) -> bool {
        self.open.is_empty() && self.folded.is_empty()
    }

    fn read(v: &Value) -> Self {
        let keys = |k: &str| -> BTreeSet<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().take(CHOICES_MAX).filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        Folds { open: keys("open"), folded: keys("folded") }
    }
}

/// The choices of designs with a file, by file, the most recently used last.
#[derive(Debug, Default)]
pub(crate) struct Remembered(Vec<(String, Folds)>);

impl Remembered {
    fn get(&self, path: &str) -> Option<&Folds> {
        self.0.iter().find(|(p, _)| p == path).map(|(_, folds)| folds)
    }

    /// Remember `folds` for `path` as the most recently used (forget it when no choice is made).
    fn put(&mut self, path: &str, folds: &Folds) {
        self.0.retain(|(p, _)| p != path);
        if !folds.is_empty() {
            self.0.push((path.to_string(), folds.clone()));
        }
        let over = self.0.len().saturating_sub(DESIGNS_MAX);
        self.0.drain(..over);
    }
}

/// Whether `key` shows open in the active design: as last chosen there, else `unless_chosen`.
pub(crate) fn is_open(app: &SolveApp, key: &str, unless_chosen: bool) -> bool {
    app.tree.history_folds.get(key).unwrap_or(unless_chosen)
}

/// Open or fold `key` in the active design, remembered for its file when it has one.
pub(crate) fn set_open(app: &mut SolveApp, key: &str, open: bool) {
    app.tree.history_folds.set(key, open);
    remember(app);
}

/// Remember the active design's choices for its file: as they change, and after a save gave it
/// a file (or another one).
pub(crate) fn remember(app: &mut SolveApp) {
    if let Some(path) = app.session.path.as_deref() {
        app.tree.history_remembered.put(path, &app.tree.history_folds);
    }
}

/// Take up the choices remembered for the design just put in the active tab: its file's, or none.
pub(crate) fn load(app: &mut SolveApp) {
    app.tree.history_folds = app.session.path.as_deref().and_then(|p| app.tree.history_remembered.get(p)).cloned().unwrap_or_default();
    remember(app);
}

/// The remembered choices, for the preferences.
pub(crate) fn prefs(app: &SolveApp) -> Value {
    app.tree.history_remembered.0.iter().map(|(path, folds)| json!({"path": path, "open": folds.open, "folded": folds.folded})).collect()
}

/// Restore the choices saved with [`prefs`] (`v`: the preferences), and drop the History's keys
/// from the browser's folders, where they were once saved for all designs.
pub(crate) fn load_prefs(app: &mut SolveApp, v: &Value) {
    let entries = v.get("history_folds").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let mut remembered = Remembered::default();
    for entry in entries.iter().skip(entries.len().saturating_sub(DESIGNS_MAX)) {
        if let Some(path) = entry.get("path").and_then(Value::as_str) {
            remembered.put(path, &Folds::read(entry));
        }
    }
    app.tree.history_remembered = remembered;
    let shared = |key: &String| SHARED_ONCE.iter().any(|k| key.starts_with(k));
    app.tree.expanded.retain(|key| !shared(key));
    app.tree.collapsed.retain(|key| !shared(key));
    load(app);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DESIGNS_MAX, Folds, Remembered};
    use crate::SolveApp;

    fn new_app() -> SolveApp {
        SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default())
    }

    /// An empty folder of its own for one test's designs.
    fn folder(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-history-folds-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    fn design(app: &mut SolveApp, dir: &std::path::Path, name: &str) -> String {
        let path = dir.join(name).to_string_lossy().into_owned();
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).expect("a box");
        app.run("file.save_as", json!({"path": path})).expect("saved");
        path
    }

    fn open(app: &mut SolveApp, path: &str) {
        crate::documents::prepare_open(app);
        app.run("doc.open", json!({"path": path})).expect("opened");
    }

    /// A choice made in one design stays with it: another design, open in its own tab or opened
    /// later in the same tab, starts without it; going back to the first design shows it again.
    #[test]
    fn a_choice_stays_with_the_design_it_was_made_in() {
        let dir = folder("stays");
        let mut app = new_app();
        let first = design(&mut app, &dir, "first.solidcraft");
        super::set_open(&mut app, "timeline_group:7", true);
        assert!(super::is_open(&app, "timeline_group:7", false));

        crate::documents::new_design(&mut app);
        assert!(!super::is_open(&app, "timeline_group:7", false), "a new design in its own tab");
        let second = design(&mut app, &dir, "second.solidcraft");
        super::set_open(&mut app, "timeline_group:7", false);
        assert!(!super::is_open(&app, "timeline_group:7", true));

        crate::documents::switch(&mut app, 0);
        assert!(super::is_open(&app, "timeline_group:7", false), "back in the first design's tab");
        app.run("doc.open", json!({"path": second})).expect("the second design, in this tab");
        assert!(!super::is_open(&app, "timeline_group:7", true), "the second design's own choice");
        app.run("doc.open", json!({"path": first})).expect("the first design again");
        assert!(super::is_open(&app, "timeline_group:7", false), "remembered while it was replaced");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The choices of designs with a file outlast the run (in the preferences); the History's keys
    /// once saved for all designs with the browser's folders no longer apply to any design.
    #[test]
    fn choices_are_remembered_by_file_between_runs() {
        let dir = folder("between-runs");
        let mut app = new_app();
        let first = design(&mut app, &dir, "first.solidcraft");
        super::set_open(&mut app, "timeline_component:3:9", true);
        crate::documents::new_design(&mut app);
        let second = design(&mut app, &dir, "second.solidcraft");
        let mut prefs: serde_json::Value = serde_json::from_str(&app.prefs()).expect("preferences");
        prefs["browser_expanded"] = json!(["c0/bodies", "timeline_group:8", "timeline_component:1:2"]);

        let mut next = new_app();
        next.load_prefs(&prefs.to_string());
        assert_eq!(next.tree.expanded.iter().collect::<Vec<_>>(), ["c0/bodies"], "the browser's own folders stay");
        assert!(!super::is_open(&next, "timeline_group:8", false));
        open(&mut next, &second);
        assert!(!super::is_open(&next, "timeline_component:3:9", false), "not chosen in the second design");
        open(&mut next, &first);
        assert!(super::is_open(&next, "timeline_component:3:9", false), "chosen in the first design");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Saving gives the choices to the file saved as: a new design keeps what it showed once it
    /// has a file, and so does a copy saved under another name, while the original keeps its own.
    #[test]
    fn a_save_keeps_the_choices_under_the_new_file() {
        let dir = folder("save");
        let mut app = new_app();
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).expect("a box");
        super::set_open(&mut app, "timeline_group:4", true);
        let first = dir.join("first.solidcraft").to_string_lossy().into_owned();
        crate::save_job::start(&mut app, "file.save_as", json!({"path": first}));
        crate::save_job::tick(&mut app, true);
        assert_eq!(app.session.path.as_deref(), Some(first.as_str()));
        super::set_open(&mut app, "timeline_group:5", true);
        let copy = dir.join("copy.solidcraft").to_string_lossy().into_owned();
        app.run("file.save_as", json!({"path": copy})).expect("saved as a copy");
        super::set_open(&mut app, "timeline_group:6", true);

        let remembered = &app.tree.history_remembered;
        let open = |path: &str, key: &str| remembered.get(path).and_then(|f| f.get(key)) == Some(true);
        assert!(open(&first, "timeline_group:4") && open(&first, "timeline_group:5"));
        assert!(!open(&first, "timeline_group:6"), "chosen after the copy was saved");
        assert!(open(&copy, "timeline_group:4") && open(&copy, "timeline_group:6"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Only the designs used most recently are remembered, and a design whose choices are all
    /// gone is forgotten.
    #[test]
    fn the_least_recently_used_designs_are_forgotten() {
        let mut remembered = Remembered::default();
        let mut folds = Folds::default();
        folds.set("timeline_group:1", true);
        for i in 0..=DESIGNS_MAX {
            remembered.put(&format!("{i}.solidcraft"), &folds);
        }
        assert_eq!(remembered.0.len(), DESIGNS_MAX);
        assert!(remembered.get("0.solidcraft").is_none(), "the least recently used");
        remembered.put("1.solidcraft", &Folds::default());
        assert!(remembered.get("1.solidcraft").is_none(), "no choice left");
    }
}
