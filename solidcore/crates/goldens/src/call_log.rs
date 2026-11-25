//! A log of every status write whose `GOLDENS_TYPED=1` value differs from its legacy value,
//! for checking the re-bless entry by entry. Scaffolding for the switch to kinds: it goes
//! with the `legacy` arguments and [`crate::status::typed`] once the goldens record kinds.
//!
//! Off unless `GOLDENS_CALL_LOG=<dir>` is set; then each case's distinct differing writes are
//! appended to `<dir>/<unit>.log` when the case ends, one line each:
//! `case<TAB>helper<TAB>file:line<TAB>legacy<TAB>typed`. A write outside a case (a fixture
//! built before the loop) is logged to `_outside.log` under case `?`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::io::Write;
use std::panic::Location;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Environment variable naming the directory the logs go to.
pub const CALL_LOG_ENV: &str = "GOLDENS_CALL_LOG";

thread_local! {
    static CASE: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static CALLS: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

fn dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| std::env::var_os(CALL_LOG_ENV).map(PathBuf::from))
        .as_ref()
}

/// True when `GOLDENS_CALL_LOG` is set.
pub fn active() -> bool {
    dir().is_some()
}

/// Starts logging the writes of `case` of `unit` (nothing unless [`active`]).
pub fn begin(unit: &str, case: &str) {
    if !active() {
        return;
    }
    CASE.with(|c| *c.borrow_mut() = Some((unit.to_owned(), case.to_owned())));
    CALLS.with(|c| c.borrow_mut().clear());
}

/// Ends the case [`begin`] started and appends its writes to the unit's log.
pub fn end() {
    if !active() {
        return;
    }
    let Some((unit, case)) = CASE.with(|c| c.borrow_mut().take()) else {
        return;
    };
    flush(&unit, &case);
}

fn flush(unit: &str, case: &str) {
    let calls = CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    let Some(dir) = dir() else {
        return;
    };
    if calls.is_empty() {
        return;
    }
    let mut text = String::new();
    for call in calls {
        text.push_str(case);
        text.push('\t');
        text.push_str(&call);
        text.push('\n');
    }
    let path = dir.join(format!("{unit}.log"));
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if let Err(e) = written {
        eprintln!("call log: {}: {e}", path.display());
    }
}

/// Notes one write (only when the two values differ). Callers check [`active`] first.
pub(crate) fn note(helper: &str, at: &Location<'_>, legacy: &dyn Debug, typed: &dyn Debug) {
    let legacy = format!("{legacy:?}");
    let typed = format!("{typed:?}");
    if legacy == typed {
        return;
    }
    let line = format!("{helper}\t{}:{}\t{legacy}\t{typed}", at.file(), at.line());
    let in_case = CASE.with(|c| c.borrow().is_some());
    CALLS.with(|c| c.borrow_mut().insert(line));
    if !in_case {
        flush("_outside", "?");
    }
}
