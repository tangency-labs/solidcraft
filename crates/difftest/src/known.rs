//! Known mismatches: cases that genuinely disagree with the C++ reference and are
//! tracked in one checked-in list, `crates/difftest/known_mismatches.toml`, until the
//! port is fixed.
//!
//! A listed case that fails its comparison is classified
//! [`CaseOutcome::KnownMismatch`](crate::runner::CaseOutcome::KnownMismatch): it is not a
//! pass, it keeps the unit from being green (the unit is at best `partial`), and it is
//! printed and written to the report like a failure. A listed case that does anything
//! else (passes, defers, or is no longer produced) fails the unit, so the list cannot go
//! stale. Never add a case here to get a unit green; every entry needs a
//! `plan/findings.md` entry.
//!
//! ```toml
//! [[mismatch]]
//! unit = "curve.curve_p1"
//! case = "gen0042"
//! why = "C++ succeeds with 0 points, Rust returns status_3 (plan/findings.md: ...)"
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::HarnessError;

/// Path of the list, relative to the repository root.
pub const KNOWN_MISMATCHES: &str = "crates/difftest/known_mismatches.toml";

/// One listed case.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownMismatch {
    /// Unit id.
    pub unit: String,
    /// Case name within the unit.
    pub case: String,
    /// Why it disagrees (C++ vs Rust status) and where it is recorded.
    pub why: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    mismatch: Vec<KnownMismatch>,
}

/// Parses the list's text.
pub fn parse(text: &str) -> Result<Vec<KnownMismatch>, HarnessError> {
    let f: File =
        toml::from_str(text).map_err(|e| HarnessError::new(format!("{KNOWN_MISMATCHES}: {e}")))?;
    Ok(f.mismatch)
}

/// The entries of `unit` from `<root>/crates/difftest/known_mismatches.toml` (empty if
/// the file does not exist).
pub fn for_unit(root: &Path, unit: &str) -> Result<Vec<KnownMismatch>, HarnessError> {
    let path = root.join(KNOWN_MISMATCHES);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(HarnessError::new(format!("{}: {e}", path.display()))),
    };
    Ok(parse(&text)?
        .into_iter()
        .filter(|m| m.unit == unit)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_rejects_typos() {
        let v = parse("[[mismatch]]\nunit = \"a.b\"\ncase = \"c\"\nwhy = \"w\"\n").unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].unit.as_str(), v[0].case.as_str()), ("a.b", "c"));
        assert!(parse("").unwrap().is_empty());
        assert!(parse("[[mismatch]]\nunit = \"a\"\ncase = \"c\"\nwhy = \"w\"\nx = 1\n").is_err());
    }

    #[test]
    fn checked_in_list_parses() {
        let root = crate::case::repo_root();
        assert!(for_unit(&root, "no.such.unit").unwrap().is_empty());
    }
}
