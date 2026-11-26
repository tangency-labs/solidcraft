//! Test-only ratchet on the blend sites that build a *phrase cause*: a
//! [`BlendFailure`](super::blend::BlendFailure) that reads as neither a defect nor a
//! refusal ([`BlendFailure::is_phrase`](super::blend::BlendFailure::is_phrase)). The
//! count may only fall: each site moves to a finer cause, a refusal, or a
//! [`BlendFailure::Defect`](super::blend::BlendFailure::Defect) with its reason, and the PR
//! that converts it lowers its file's pin (see [`PINS`]). The other areas' phrase causes
//! are counted the same way, each area's list in its own error file; how to convert a site
//! is the conversion guide (`docs/failure-codes-and-reproduction.md` §6).
//!
//! **What counts as one site**, in the library code of `solidcore` and `solidcore-ops`
//! (comments, string contents and every `#[cfg(test)]` item blanked, test files skipped:
//! [`super::bare_scan::crate_library_files`]), as [`super::site_scan`] counts it for
//! [`BLEND`]:
//! - a construction of a phrase variant, `BlendFailure::Failed`, `Missing`,
//!   `InvalidValue`, `Inconsistent`, `BadIntersections`, `NotConverged`, `OutsideDomain`,
//!   `NotWithinTolerance` or `PassedOn`, by any path ending in `BlendFailure::<variant>`;
//! - a call of a helper that builds one: `blend_failed(`, `blend_missing(`,
//!   `blend_invalid_value(` (their own definitions are not sites);
//! - a call of a local *wrapper*: a `let` closure, a private `fn` or a `macro_rules!` that
//!   passes one of its own parameters on as the phrase of one of the above (a variant's
//!   field or `PassedOn`'s `what`, a helper's second argument, the same parameter of a
//!   wrapper, any argument of a macro). Passing on only the operation does not make a
//!   wrapper: that construction is one site. The wrapper's body is not a site; each call of it in its scope is (the rest of
//!   the enclosing block for a closure, the file for a `fn` or a macro). Wrappers of
//!   wrappers resolve the same way.
//!
//! Not counted: the variant definitions and their `Self::` matches in `error/blend.rs`,
//! patterns (a variant followed by `=>`, `|`, `if` or `=`, binding `_` or `..`, or in the
//! pattern of a `matches!`), and comments, strings and tests. A closure that builds a
//! fixed phrase (`let nul = || SolidError::blend_missing(OP, "..")`) is one site however
//! often it runs. A wrapper `fn` visible outside its file fails the scan, since its calls
//! elsewhere would not be seen.

use super::bare_scan::library_code;
use super::site_scan::{self, crate_sites, Builder, Scan, Site, Wrappers};

/// The blend's phrase causes: the phrase variants of `BlendFailure` and the `SolidError`
/// helpers that build one (their phrase is the second argument).
pub(in crate::error) const BLEND: Scan = Scan {
    builders: &[
        Builder::Variant("BlendFailure", "Failed"),
        Builder::Variant("BlendFailure", "Missing"),
        Builder::Variant("BlendFailure", "InvalidValue"),
        Builder::Variant("BlendFailure", "Inconsistent"),
        Builder::Variant("BlendFailure", "BadIntersections"),
        Builder::Variant("BlendFailure", "NotConverged"),
        Builder::Variant("BlendFailure", "OutsideDomain"),
        Builder::Variant("BlendFailure", "NotWithinTolerance"),
        Builder::Variant("BlendFailure", "PassedOn"),
        Builder::Helper("blend_failed", Some(1)),
        Builder::Helper("blend_missing", Some(1)),
        Builder::Helper("blend_invalid_value", Some(1)),
    ],
    wrappers: Wrappers::Phrase,
};

/// The sites in a file's library code (from [`library_code`]) that build a blend phrase
/// cause, sorted by line. Panics on a wrapper `fn` visible outside the file.
pub(in crate::error) fn sites_in(code: &str, file: &str) -> Vec<Site> {
    site_scan::sites_in(&BLEND, code, file)
}

/// The library files of `solidcore` and `solidcore-ops` that build blend phrase causes,
/// with their sites, as (path relative to `crates/`, sites), sorted by path.
pub(in crate::error) fn phrase_sites() -> Vec<(String, Vec<Site>)> {
    crate_sites(&BLEND)
}

/// The pins of the phrase-cause sites: every library file that builds a phrase cause has
/// one file in `src/error/pins/phrase/`, `<path>.pin` holding its exact number of sites
/// (see the module docs for what counts; [`site_scan::pins`] for the layout). Converting a
/// site lowers its file's pin in the same PR, and a file that reaches 0 loses its pin. The
/// count never rises: a new failure gets a classed cause. Code that moves between files
/// moves its sites: the PR lowers one pin and raises the other by the same number, and the
/// total stays (the failure says so). Per file, as the bare-status ratchets are
/// (`assert_waiting`), so a rise hidden by a conversion in another file still fails. One
/// file per pin, so conversions in different files never touch the same line.
const PINS: &str = "phrase";

#[cfg(test)]
mod tests {
    use super::*;

    /// The number of blend sites that build a phrase cause only falls.
    #[test]
    fn the_phrase_cause_sites_only_shrink() {
        site_scan::assert_pins("phrase-cause sites", &phrase_sites(), PINS);
    }

    fn lines(code: &str) -> Vec<(usize, String)> {
        sites_in(&library_code(code), "fixture")
            .into_iter()
            .map(|s| (s.line, s.via))
            .collect()
    }

    /// Constructions, helper calls and wrapper calls count; patterns, definitions,
    /// comments, strings and tests don't.
    #[test]
    fn the_scan_counts_sites_not_patterns_or_wrappers() {
        let fixture = r#"
fn a() -> E { SolidError::blend(OP, BlendFailure::Failed("x")) }
fn b() -> E { SolidError::blend_missing(OP, "y") }
fn c(e: &E) -> bool { matches!(e.blend_failure(), Some(BlendFailure::Missing(_))) }
fn d(f: F) -> i32 { match f { BlendFailure::Failed("p") | BlendFailure::Missing("q") => 1, _ => 0 } }
fn e(f: F) { if let Some(BlendFailure::NotConverged(w)) = f { } }
// SolidError::blend_failed(OP, "comment")
fn s() -> &'static str { "SolidError::blend_failed(OP, x)" }
fn wrap(what: &'static str) -> E { SolidError::blend_failed(OP, what) }
fn g() { wrap("one"); let _ = Thing::wrap("two"); }
fn h() {
    let nul = |what: &'static str| SolidError::blend_missing(OP, what);
    nul("three");
    x.ok_or_else(|| nul("four"));
}
fn i() { nul("not in scope"); let fixed = || SolidError::blend_failed(OP, "five"); }
macro_rules! null { ($what:literal) => { SolidError::blend_missing(OP, $what) }; }
fn j() { null!("six"); }
fn outer(reason: &'static str) -> E { wrap(reason) }
fn k() { outer("seven"); SolidError::wrap(OP); }
fn l() -> E { SolidError::blend(OP, BlendFailure::PassedOn { what: "eight", status: 3 }) }
pub fn nth(v: &[u8], op: &'static str) -> R { v.first().ok_or_else(|| SolidError::blend_missing(op, "nine")) }
fn m() { nth(&[], OP); }
pub(crate) fn blend_failed(op: &'static str, what: &'static str) -> E {
    E::blend(op, BlendFailure::Failed(what))
}
#[cfg(test)]
mod tests { fn t() { SolidError::blend_failed(OP, "test"); } }
"#;
        assert_eq!(
            lines(fixture),
            vec![
                (2, "BlendFailure::Failed".to_owned()),
                (3, "blend_missing".to_owned()),
                (10, "wrap (wraps blend_failed)".to_owned()),
                (10, "wrap (wraps blend_failed)".to_owned()),
                (13, "nul (wraps blend_missing)".to_owned()),
                (14, "nul (wraps blend_missing)".to_owned()),
                (16, "blend_failed".to_owned()),
                (18, "null (wraps blend_missing)".to_owned()),
                (20, "outer (wraps wrap (wraps blend_failed))".to_owned()),
                (21, "BlendFailure::PassedOn".to_owned()),
                (22, "blend_missing".to_owned()),
            ]
        );
    }

    /// A wrapper visible outside its file fails the scan: its calls elsewhere are unseen.
    #[test]
    #[should_panic(expected = "visible outside its file")]
    fn a_public_wrapper_fails_the_scan() {
        let _ =
            lines("pub fn fail(what: &'static str) -> E { SolidError::blend_failed(OP, what) }\n");
    }
}
