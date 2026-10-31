//! Keep unsaved work in this browser (#10220): the decisions, in plain Rust (tested natively).
//!
//! With Preferences › General › "Keep unsaved work in this browser instead of asking when the tab
//! closes" on, each open design with unsaved changes has a copy in the browser's private storage:
//! `keep-<id>.sc` (the design) and `keep-<id>.json` (its name, its file and when it last
//! changed). The copy is written shortly after each change ([`due`]) and again when the page is
//! hidden, so it is already current when the tab closes; the page then doesn't ask through the
//! browser's "leave site?" box ([`asks_before_unload`]). A design that is saved, closed or
//! discarded has its copy removed. The next visit offers the copies newer than their design's
//! last save ([`fate`]) in SolidCraft's own window, answered with [`plan`].
//!
//! An id is `<page>-<n>`: the page that wrote it (each page load has its own) and the design's
//! number in that page. A page holds a browser lock named after it while it runs, so another tab
//! never offers, or removes, the copies of a page that is still open.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use serde_json::{Value, json};

/// The start of every kept file's name.
pub const PREFIX: &str = "keep-";
/// The browser lock a page holds while it runs: this, then its page id.
pub const LOCK_PREFIX: &str = "solidcraft.keep.";
/// A copy is written once the design has been still this long (ms)…
pub const QUIET_MS: f64 = 800.0;
/// …or at the latest this long after the first change it doesn't hold yet (ms).
pub const MAX_WAIT_MS: f64 = 4000.0;
/// The most copies offered at once (the rest wait for the next visit).
pub const MAX_OFFERED: usize = 32;
const MAX_ID: usize = 64;
const MAX_NAME: usize = 256;
const MAX_PATH: usize = 4096;
/// Copies' metadata is small: a damaged or hostile one is not read.
pub const MAX_META_BYTES: usize = 64 * 1024;

/// Ids are short and safe in a file name: ASCII letters, digits and dashes, with a page part.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && page_of(id).is_some()
}

/// The page that wrote copy `id`.
pub fn page_of(id: &str) -> Option<&str> {
    id.rsplit_once('-').map(|(page, _)| page).filter(|p| !p.is_empty())
}

/// This page's id, from the time it started (ms since 1970) and a random number in 0..1:
/// letters and digits only.
pub fn page_id(now_ms: f64, random: f64) -> String {
    let now = if now_ms.is_finite() && now_ms > 0.0 { now_ms.min(1e15) as u64 } else { 0 };
    let random = if random.is_finite() { (random.clamp(0.0, 1.0) * f64::from(0xff_ffff)) as u64 } else { 0 };
    format!("{now:x}{random:06x}")
}

/// The copy of design `uid` (`Session::uid`) open in page `page`.
pub fn entry_id(page: &str, uid: u64) -> String {
    format!("{page}-{uid}")
}

/// The stored file names of copy `id`: its design and its metadata.
pub fn design_file(id: &str) -> String {
    format!("{PREFIX}{id}.{}", solidcraft_engine::io::DESIGN_EXT)
}
pub fn meta_file(id: &str) -> String {
    format!("{PREFIX}{id}.json")
}

/// Which part of a copy a stored file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Design,
    Meta,
}

/// A stored file's copy id and part; `None` for every other file.
pub fn parse_name(name: &str) -> Option<(&str, Part)> {
    let rest = name.strip_prefix(PREFIX)?;
    let (id, part) = if let Some(id) = rest.strip_suffix(".json") {
        (id, Part::Meta)
    } else {
        (rest.strip_suffix(solidcraft_engine::io::DESIGN_EXT)?.strip_suffix('.')?, Part::Design)
    };
    valid_id(id).then_some((id, part))
}

/// A kept copy, as its metadata describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    /// The design's name.
    pub name: String,
    /// Its file (`None`: never saved).
    pub path: Option<String>,
    /// When the design last changed (ms since 1970).
    pub changed_at: f64,
}

impl Entry {
    pub fn to_json(&self) -> String {
        json!({"name": self.name, "path": self.path, "changed_at": self.changed_at}).to_string()
    }

    /// Read copy `id`'s metadata. Anything malformed, oversized or not finite is no copy.
    pub fn parse(id: &str, bytes: &[u8]) -> Option<Entry> {
        if !valid_id(id) || bytes.len() > MAX_META_BYTES {
            return None;
        }
        let v: Value = serde_json::from_slice(bytes).ok()?;
        let name = v.get("name")?.as_str()?.chars().take(MAX_NAME).collect::<String>();
        let path = match v.get("path") {
            None | Some(Value::Null) => None,
            Some(p) => Some(p.as_str().filter(|p| !p.is_empty() && p.len() <= MAX_PATH)?.to_string()),
        };
        let changed_at = v.get("changed_at")?.as_f64().filter(|t| t.is_finite() && *t >= 0.0)?;
        Some(Entry { id: id.to_string(), name, path, changed_at })
    }
}

/// Whether a copy that last changed at `changed_at` holds work its design's file doesn't:
/// the file was last written at `saved_at` (ms since 1970; `None` or unknown: never saved, or
/// the time can't be read, so the copy is kept rather than lost).
pub fn newer_than_save(changed_at: f64, saved_at: Option<f64>) -> bool {
    match saved_at {
        Some(saved) if saved.is_finite() && saved > 0.0 => changed_at > saved,
        _ => true,
    }
}

/// What the next visit does with a copy it finds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// Offer it: "Restore unsaved design…?".
    Offer,
    /// Its design was saved after it: remove it without asking.
    Drop,
    /// Its page is still open (another tab): leave it alone.
    Leave,
}

/// The copy `entry`, whose design's file was last written at `saved_at`, while the pages
/// `live` are open.
pub fn fate(entry: &Entry, live: &[String], saved_at: Option<f64>) -> Fate {
    if page_of(&entry.id).is_some_and(|p| live.iter().any(|l| l == p)) {
        Fate::Leave
    } else if newer_than_save(entry.changed_at, saved_at) {
        Fate::Offer
    } else {
        Fate::Drop
    }
}

/// Copies to offer first: the most recently changed, at most [`MAX_OFFERED`].
pub fn order_offered(entries: &mut Vec<Entry>) {
    entries.sort_by(|a, b| b.changed_at.total_cmp(&a.changed_at).then_with(|| a.id.cmp(&b.id)));
    entries.truncate(MAX_OFFERED);
}

/// Whether a design's copy is due to be written at `now`: the design last changed at
/// `last_change`, and the copy has lacked a change since `first_unwritten` (all ms).
pub fn due(now: f64, first_unwritten: f64, last_change: f64) -> bool {
    now - last_change >= QUIET_MS || now - first_unwritten >= MAX_WAIT_MS
}

/// Whether leaving the page asks through the browser's "leave site?" box: there is unsaved work,
/// and it isn't all already kept in the browser (with the preference off it never is).
pub fn asks_before_unload(persists: bool, keep: bool, any_unsaved: bool, all_kept: bool) -> bool {
    persists && any_unsaved && !(keep && all_kept)
}

/// An answer in the "Restore unsaved design?" window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Open the copy, with its unsaved changes, as the design it was (saving goes to its file).
    Restore,
    /// Open the design's file as last saved; the copy goes.
    OpenSaved,
    /// The copy goes.
    Discard,
}

/// What an answer does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Open the copy (and, once it is open, give it back its name and file).
    pub open_copy: bool,
    /// Open this saved file.
    pub open_saved: Option<String>,
    /// Remove the copy.
    pub drop_copy: bool,
}

/// What `answer` does to copy `entry`; `saved_exists`: its file can still be opened. `None`: the
/// answer isn't offered for it (Open saved version of a design never saved, or gone).
pub fn plan(answer: Answer, entry: &Entry, saved_exists: bool) -> Option<Plan> {
    match answer {
        Answer::Restore => Some(Plan { open_copy: true, open_saved: None, drop_copy: true }),
        Answer::OpenSaved => {
            let path = entry.path.clone().filter(|_| saved_exists)?;
            Some(Plan { open_copy: false, open_saved: Some(path), drop_copy: true })
        }
        Answer::Discard => Some(Plan { open_copy: false, open_saved: None, drop_copy: true }),
    }
}

/// The answers the window offers for `entry`, in order.
pub fn answers(entry: &Entry, saved_exists: bool) -> Vec<Answer> {
    [Answer::Restore, Answer::OpenSaved, Answer::Discard].into_iter().filter(|a| plan(*a, entry, saved_exists).is_some()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, changed_at: f64, path: Option<&str>) -> Entry {
        Entry { id: id.into(), name: "Bracket".into(), path: path.map(str::to_string), changed_at }
    }

    #[test]
    fn copies_are_named_by_page_and_design_and_nothing_else_reads_as_one() {
        let page = page_id(1_700_000_000_000.0, 0.5);
        assert!(page.chars().all(|c| c.is_ascii_hexdigit()));
        let id = entry_id(&page, 7);
        assert!(valid_id(&id));
        assert_eq!(page_of(&id), Some(page.as_str()));
        assert_eq!(parse_name(&design_file(&id)), Some((id.as_str(), Part::Design)));
        assert_eq!(parse_name(&meta_file(&id)), Some((id.as_str(), Part::Meta)));
        for other in ["part.sc", "recovery.sc", "recovery.json", "keep-.sc", "keep-x.sc", "keep-a/b-1.sc", "keep-a-1.step", "keep-a-1sc"] {
            assert_eq!(parse_name(other), None, "{other}");
        }
        // Hostile clocks still give a usable id.
        for (now, random) in [(f64::NAN, f64::NAN), (f64::INFINITY, -1.0), (-5.0, 9.0)] {
            assert!(valid_id(&entry_id(&page_id(now, random), 1)));
        }
    }

    #[test]
    fn metadata_round_trips_and_hostile_metadata_is_no_copy() {
        let e = entry("a1-3", 1234.0, Some("/opfs/Bracket.sc"));
        assert_eq!(Entry::parse("a1-3", e.to_json().as_bytes()), Some(e.clone()));
        let never_saved = entry("a1-4", 99.0, None);
        assert_eq!(Entry::parse("a1-4", never_saved.to_json().as_bytes()), Some(never_saved));
        for bad in [
            "",
            "[]",
            r#"{"name":"x","changed_at":"soon"}"#,
            r#"{"name":"x","changed_at":-1}"#,
            r#"{"name":"x","changed_at":1e999}"#,
            r#"{"name":"x","path":7,"changed_at":1}"#,
            r#"{"name":"x","path":"","changed_at":1}"#,
            r#"{"changed_at":1}"#,
        ] {
            assert_eq!(Entry::parse("a1-3", bad.as_bytes()), None, "{bad}");
        }
        assert_eq!(Entry::parse("../x", e.to_json().as_bytes()), None);
        assert_eq!(Entry::parse("a1-3", &vec![b' '; MAX_META_BYTES + 1]), None);
        let long = json!({"name": "n".repeat(10_000), "changed_at": 1}).to_string();
        assert_eq!(Entry::parse("a1-3", long.as_bytes()).map(|e| e.name.len()), Some(MAX_NAME));
    }

    #[test]
    fn a_copy_newer_than_the_last_save_is_offered_an_older_one_dropped_an_open_pages_left() {
        let e = entry("p1-1", 2000.0, Some("/opfs/Bracket.sc"));
        // Changed after the file was last written: offered.
        assert_eq!(fate(&e, &[], Some(1000.0)), Fate::Offer);
        // Saved since (in this tab or another): nothing to offer.
        assert_eq!(fate(&e, &[], Some(2000.0)), Fate::Drop);
        assert_eq!(fate(&e, &[], Some(3000.0)), Fate::Drop);
        // Never saved, or the save time can't be read: kept rather than lost.
        for saved in [None, Some(0.0), Some(f64::NAN), Some(f64::INFINITY), Some(-4.0)] {
            assert_eq!(fate(&e, &[], saved), Fate::Offer, "{saved:?}");
        }
        // Its page is still open in another tab: neither offered nor removed.
        assert_eq!(fate(&e, &["p1".into()], Some(1000.0)), Fate::Leave);
        assert_eq!(fate(&e, &["p1".into()], Some(3000.0)), Fate::Leave);
        assert_eq!(fate(&e, &["p2".into()], Some(1000.0)), Fate::Offer);
    }

    #[test]
    fn the_newest_copies_are_offered_first_and_at_most_a_windowful() {
        let mut list: Vec<Entry> = (0..40).map(|i| entry(&format!("p-{i}"), f64::from(i), None)).collect();
        order_offered(&mut list);
        assert_eq!(list.len(), MAX_OFFERED);
        assert_eq!(list.first().map(|e| e.id.as_str()), Some("p-39"));
    }

    #[test]
    fn a_copy_is_written_once_the_design_is_still_or_after_the_longest_wait() {
        // Still changing, not waited long: not yet.
        assert!(!due(1000.0, 900.0, 900.0));
        // Still for QUIET_MS: due.
        assert!(due(900.0 + QUIET_MS, 900.0, 900.0));
        // Changing all the time: due after MAX_WAIT_MS anyway.
        assert!(!due(5000.0, 2000.0, 4900.0));
        assert!(due(2000.0 + MAX_WAIT_MS, 2000.0, 2000.0 + MAX_WAIT_MS - 1.0));
    }

    #[test]
    fn the_browser_asks_before_unload_only_without_a_current_copy() {
        // The preference off: exactly as before, unsaved work asks.
        assert!(asks_before_unload(true, false, true, true));
        assert!(asks_before_unload(true, false, true, false));
        // On, with every unsaved design kept: no browser box.
        assert!(!asks_before_unload(true, true, true, true));
        // On, but a copy still being written (or failed): the browser still asks.
        assert!(asks_before_unload(true, true, true, false));
        // Nothing unsaved, or an embedded page that keeps nothing: never.
        assert!(!asks_before_unload(true, true, false, false));
        assert!(!asks_before_unload(true, false, false, false));
        assert!(!asks_before_unload(false, false, true, false));
    }

    #[test]
    fn restore_open_saved_and_discard_each_do_one_thing_and_always_remove_the_copy() {
        let saved = entry("p-1", 5.0, Some("/opfs/Bracket.sc"));
        assert_eq!(plan(Answer::Restore, &saved, true), Some(Plan { open_copy: true, open_saved: None, drop_copy: true }));
        assert_eq!(
            plan(Answer::OpenSaved, &saved, true),
            Some(Plan { open_copy: false, open_saved: Some("/opfs/Bracket.sc".into()), drop_copy: true })
        );
        assert_eq!(plan(Answer::Discard, &saved, true), Some(Plan { open_copy: false, open_saved: None, drop_copy: true }));
        assert_eq!(answers(&saved, true), [Answer::Restore, Answer::OpenSaved, Answer::Discard]);
        // A design never saved, or whose file is gone, has no saved version to open.
        let never = entry("p-2", 5.0, None);
        assert_eq!(plan(Answer::OpenSaved, &never, true), None);
        assert_eq!(plan(Answer::OpenSaved, &saved, false), None);
        assert_eq!(answers(&never, false), [Answer::Restore, Answer::Discard]);
    }
}
