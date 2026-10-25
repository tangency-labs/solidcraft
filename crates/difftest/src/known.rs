//! Known mismatches: cases that genuinely disagree with the C++ reference and are
//! tracked in checked-in lists, one file per unit,
//! `crates/difftest/known_mismatches/<unit id>.toml`, until the port is fixed.
//!
//! One file per unit (never one shared list) so parallel PRs touching different units
//! never conflict. A unit with no file has no known mismatches; delete the file when
//! its last entry goes.
//!
//! A listed case that fails its comparison is classified
//! [`CaseOutcome::KnownMismatch`](crate::runner::CaseOutcome::KnownMismatch): it is not a
//! pass, it keeps the unit from being green (the unit is at best `partial`), and it is
//! printed and written to the report like a failure. A listed case that does anything
//! else (passes, defers, or is no longer produced) fails the unit, so the list cannot go
//! stale. Never add a case here to get a unit green; every entry needs a
//! `plan/findings/<slug>.md` entry. Every entry's `unit` must equal the file name.
//!
//! An entry whose disagreement comes from the C library the C++ reference runs on (e.g.
//! glibc's allocator reusing a freed address under an upstream use-after-free) names it
//! in `libc`. On any other C library the entry does not apply: the case runs as an
//! ordinary case and must pass.
//!
//! ```toml
//! # crates/difftest/known_mismatches/curve.curve_p1.toml
//! [[mismatch]]
//! unit = "curve.curve_p1"
//! case = "gen0042"
//! why = "C++ succeeds with 0 points, Rust returns status_3 (plan/findings/...)"
//! # libc = "glibc"   # optional: only where the C++ reference runs on glibc
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::HarnessError;

/// Directory of the per-unit lists, relative to the repository root.
pub const KNOWN_MISMATCHES_DIR: &str = "crates/difftest/known_mismatches";

/// The former single shared list; it must not come back (one file per unit instead).
pub const RETIRED_SHARED_LIST: &str = "crates/difftest/known_mismatches.toml";

/// Path of `unit`'s list, relative to the repository root.
pub fn path_for(unit: &str) -> String {
    format!("{KNOWN_MISMATCHES_DIR}/{unit}.toml")
}

/// One listed case.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownMismatch {
    /// Unit id (must equal the file name).
    pub unit: String,
    /// Case name within the unit.
    pub case: String,
    /// Why it disagrees (C++ vs Rust status) and where it is recorded.
    pub why: String,
    /// The C library the disagreement depends on; `None` applies everywhere.
    #[serde(default)]
    pub libc: Option<Libc>,
}

impl KnownMismatch {
    /// Whether the entry applies to this build of the C++ reference.
    pub fn applies(&self) -> bool {
        self.libc.is_none_or(Libc::is_current)
    }
}

/// A C library a known mismatch can be restricted to ([`KnownMismatch::libc`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Libc {
    /// GNU libc (Linux `*-gnu` targets), whose allocator the CI reference uses.
    Glibc,
}

impl Libc {
    /// Whether the C++ reference in this process runs on this C library.
    pub fn is_current(self) -> bool {
        match self {
            Self::Glibc => cfg!(all(target_os = "linux", target_env = "gnu")),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    mismatch: Vec<KnownMismatch>,
}

/// Parses the text of `unit`'s list, rejecting entries for any other unit.
pub fn parse(text: &str, unit: &str) -> Result<Vec<KnownMismatch>, HarnessError> {
    let path = path_for(unit);
    let f: File = toml::from_str(text).map_err(|e| HarnessError::new(format!("{path}: {e}")))?;
    if let Some(m) = f.mismatch.iter().find(|m| m.unit != unit) {
        return Err(HarnessError::new(format!(
            "{path}: entry for case `{}` names unit `{}`; it belongs in {}",
            m.case,
            m.unit,
            path_for(&m.unit)
        )));
    }
    Ok(f.mismatch)
}

/// The entries of `unit` from `<root>/crates/difftest/known_mismatches/<unit>.toml`
/// (empty if the file does not exist).
pub fn for_unit(root: &Path, unit: &str) -> Result<Vec<KnownMismatch>, HarnessError> {
    if unit.is_empty() || unit.contains(['/', '\\']) || unit.starts_with('.') {
        return Err(HarnessError::new(format!(
            "{KNOWN_MISMATCHES_DIR}: `{unit}` is not a unit id"
        )));
    }
    let path = root.join(path_for(unit));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(HarnessError::new(format!("{}: {e}", path.display()))),
    };
    parse(&text, unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_rejects_typos_and_foreign_units() {
        let v = parse(
            "[[mismatch]]\nunit = \"a.b\"\ncase = \"c\"\nwhy = \"w\"\n",
            "a.b",
        )
        .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].unit.as_str(), v[0].case.as_str()), ("a.b", "c"));
        assert!(parse("", "a.b").unwrap().is_empty());
        assert!(parse(
            "[[mismatch]]\nunit = \"a.b\"\ncase = \"c\"\nwhy = \"w\"\nx = 1\n",
            "a.b"
        )
        .is_err());
        let e = parse(
            "[[mismatch]]\nunit = \"a.c\"\ncase = \"c\"\nwhy = \"w\"\n",
            "a.b",
        )
        .unwrap_err();
        assert!(e.0.contains("known_mismatches/a.c.toml"), "{}", e.0);
        assert!(for_unit(Path::new("."), "../x").is_err());
    }

    #[test]
    fn libc_restricted_entries_apply_only_on_that_libc() {
        let entry = |libc: &str| {
            parse(
                &format!("[[mismatch]]\nunit = \"a.b\"\ncase = \"c\"\nwhy = \"w\"\n{libc}"),
                "a.b",
            )
        };
        let any = entry("").unwrap();
        assert_eq!(any[0].libc, None);
        assert!(any[0].applies());
        let glibc = entry("libc = \"glibc\"\n").unwrap();
        assert_eq!(glibc[0].libc, Some(Libc::Glibc));
        assert_eq!(
            glibc[0].applies(),
            cfg!(all(target_os = "linux", target_env = "gnu"))
        );
        assert!(entry("libc = \"musl\"\n").is_err());
    }

    #[test]
    fn checked_in_lists_parse_and_match_their_file_names() {
        let root = crate::case::repo_root();
        assert!(for_unit(&root, "no.such.unit").unwrap().is_empty());
        assert!(
            !root.join(RETIRED_SHARED_LIST).exists(),
            "{RETIRED_SHARED_LIST} is retired: put entries in {KNOWN_MISMATCHES_DIR}/<unit id>.toml"
        );
        let units: Vec<String> = crate::plan::load(&root)
            .unwrap()
            .into_iter()
            .map(|u| u.id)
            .collect();
        let mut files: Vec<_> = std::fs::read_dir(root.join(KNOWN_MISMATCHES_DIR))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        assert!(!files.is_empty());
        for p in files {
            assert_eq!(
                p.extension().and_then(|e| e.to_str()),
                Some("toml"),
                "{p:?}"
            );
            let unit = p.file_stem().and_then(|s| s.to_str()).unwrap();
            assert!(units.iter().any(|u| u == unit), "{p:?}: no such unit");
            let v = for_unit(&root, unit).unwrap();
            assert!(!v.is_empty(), "{p:?}: empty list; delete the file");
        }
    }
}
