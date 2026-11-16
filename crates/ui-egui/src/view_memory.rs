//! What each saved design's views were when it was last closed, kept in the user's own data
//! folder (never in the design file) so "Reopen a design's views next time it opens" can bring
//! them back. A small map from a design's path to its [`ViewSet`] saved form, newest last.

use std::path::Path;

use serde_json::{Value, json};

use crate::design_views::{ViewKind, ViewSet};

/// The file in the data folder.
const FILE: &str = "view-memory.json";
/// How many designs are remembered; the least recently closed go first.
const CAP: usize = 200;
/// A file larger than this is ignored (it is not ours).
const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The remembered designs, oldest first. Anything unreadable gives none.
fn read(dir: &Path) -> Vec<(String, Value)> {
    let file = dir.join(FILE);
    if std::fs::metadata(&file).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        return Vec::new();
    }
    let Ok(Value::Array(items)) = std::fs::read(&file).map_err(drop).and_then(|bytes| serde_json::from_slice(&bytes).map_err(drop)) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, Value)> =
        items.into_iter().filter_map(|item| Some((item.get("path")?.as_str()?.to_owned(), item.get("views")?.clone()))).collect();
    let skip = entries.len().saturating_sub(CAP);
    entries.drain(..skip);
    entries
}

/// Remember `views` (a [`ViewSet::to_json`]) for the design saved at `path`, in the folder `dir`.
pub fn remember(dir: &Path, path: &str, views: Value) -> std::io::Result<()> {
    let mut entries = read(dir);
    entries.retain(|(known, _)| known != path);
    entries.push((path.to_owned(), views));
    let skip = entries.len().saturating_sub(CAP);
    entries.drain(..skip);
    let list: Vec<Value> = entries.into_iter().map(|(path, views)| json!({"path": path, "views": views})).collect();
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&temp, Value::Array(list).to_string())?;
    std::fs::rename(&temp, dir.join(FILE))
}

/// What was remembered for the design saved at `path` in the folder `dir`.
pub fn recall(dir: &Path, path: &str) -> Option<Value> {
    read(dir).into_iter().rev().find(|(known, _)| known == path).map(|(_, views)| views)
}

/// The views of a remembered `saved` set that `can_show` accepts (given each one's kind and
/// subject), as a set of their own; the closed ones are not brought back. None when the saved form
/// is not a set or no view is left.
pub fn restorable(saved: &Value, can_show: impl Fn(ViewKind, Option<&str>) -> bool) -> Option<ViewSet> {
    let mut set = saved.clone();
    let views = set.get_mut("views")?.as_array_mut()?;
    views.retain(|view| {
        let kind = view.get("kind").and_then(Value::as_str).and_then(ViewKind::from_key);
        kind.is_some_and(|kind| can_show(kind, view.get("subject").and_then(Value::as_str)))
    });
    if views.is_empty() {
        return None;
    }
    set.as_object_mut()?.remove("closed");
    ViewSet::from_json(&set)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-view-memory-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn set_with_sketch() -> ViewSet {
        let mut set = ViewSet::new();
        set.open(ViewKind::Sketch, Some("7".into()), true);
        set.open(ViewKind::Sheet, Some("d1".into()), true);
        set
    }

    #[test]
    fn a_design_gets_back_the_views_it_was_closed_with_and_the_newest_wins() {
        let dir = folder("roundtrip");
        assert_eq!(recall(&dir, "a.scd"), None, "nothing yet");
        remember(&dir, "a.scd", set_with_sketch().to_json()).unwrap();
        remember(&dir, "b.scd", ViewSet::new().to_json()).unwrap();
        assert_eq!(ViewSet::from_json(&recall(&dir, "a.scd").unwrap()), Some(set_with_sketch()));
        remember(&dir, "a.scd", ViewSet::new().to_json()).unwrap();
        assert_eq!(ViewSet::from_json(&recall(&dir, "a.scd").unwrap()), Some(ViewSet::new()));
        assert_eq!(recall(&dir, "c.scd"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_two_hundred_latest_designs_are_kept() {
        let dir = folder("cap");
        for n in 0..CAP + 5 {
            remember(&dir, &format!("{n}.scd"), ViewSet::new().to_json()).unwrap();
        }
        assert_eq!(read(&dir).len(), CAP);
        assert_eq!(recall(&dir, "0.scd"), None, "the oldest went");
        assert!(recall(&dir, &format!("{}.scd", CAP + 4)).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_or_foreign_file_is_ignored_and_replaced() {
        let dir = folder("hostile");
        std::fs::create_dir_all(&dir).unwrap();
        for text in ["", "not json", "{\"path\": 1}", "[1, null, {\"path\": 3}, {\"path\": \"a.scd\"}]"] {
            std::fs::write(dir.join(FILE), text).unwrap();
            assert_eq!(recall(&dir, "a.scd"), None, "{text}");
        }
        remember(&dir, "a.scd", ViewSet::new().to_json()).unwrap();
        assert!(recall(&dir, "a.scd").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restoring_drops_views_that_cannot_be_shown_and_stale_json() {
        let saved = set_with_sketch().to_json();
        let all = restorable(&saved, |_, _| true).unwrap();
        assert_eq!(all.len(), 3);
        let some = restorable(&saved, |kind, subject| kind == ViewKind::Model || (kind == ViewKind::Sketch && subject == Some("7"))).unwrap();
        assert_eq!(some.views().iter().map(|view| view.kind).collect::<Vec<_>>(), [ViewKind::Model, ViewKind::Sketch]);
        assert_eq!(some.active().kind, ViewKind::Model, "the active view was dropped: the first one shows");
        assert!(restorable(&saved, |_, _| false).is_none(), "nothing left");
        for hostile in [json!(null), json!(5), json!({"views": 3}), json!({"views": [{"kind": 4}]}), json!({"views": []})] {
            assert!(restorable(&hostile, |_, _| true).is_none(), "{hostile}");
        }
    }
}
