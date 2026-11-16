//! Test-only ratchet on the blend sites that build a *phrase cause*: a
//! [`BlendFailure`](super::blend::BlendFailure) that reads as neither a defect nor a
//! refusal ([`BlendFailure::is_phrase`](super::blend::BlendFailure::is_phrase)). The
//! count may only fall: each site moves to a finer cause, or to a `StageFailed` built on
//! purpose, and the PR that converts it lowers its file's count in [`PINNED`].
//!
//! **What counts as one site**, in the library code of `solidcore` and `solidcore-ops`
//! (comments, string contents and every `#[cfg(test)]` item blanked, test files skipped:
//! [`super::bare_scan::crate_library_files`]):
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

use super::bare_scan::{crate_library_files, is_ident, library_code, match_close};
use std::ops::Range;
use std::path::Path;

/// The phrase variants of `BlendFailure`.
const VARIANTS: [&str; 9] = [
    "Failed",
    "Missing",
    "InvalidValue",
    "Inconsistent",
    "BadIntersections",
    "NotConverged",
    "OutsideDomain",
    "NotWithinTolerance",
    "PassedOn",
];

/// The `SolidError` helpers that build a phrase cause.
const HELPERS: [&str; 3] = ["blend_failed", "blend_missing", "blend_invalid_value"];

/// One site that builds a phrase cause: its line, and what it calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::error) struct Site {
    /// The 1-based line.
    pub line: usize,
    /// The constructor, helper or wrapper (with what it wraps) the site calls.
    pub via: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fn,
    Closure,
    Macro,
}

/// A local definition that may wrap a phrase constructor.
#[derive(Debug)]
struct Def {
    kind: Kind,
    name: String,
    /// Its parameter names (a macro's without the `$`).
    params: Vec<String>,
    /// Where its body is.
    body: Range<usize>,
    /// Where it can be called.
    scope: Range<usize>,
    /// Whether a `fn` is visible outside its file.
    public: bool,
    /// Where its name is (a `fn` call there is the definition).
    at: usize,
}

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    while s.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

fn ident_end(s: &[u8], mut i: usize) -> usize {
    while s.get(i).is_some_and(|&b| is_ident(b)) {
        i += 1;
    }
    i
}

/// The offsets of `word` in `code` as a whole identifier.
fn words(code: &str, word: &str) -> Vec<usize> {
    let s = code.as_bytes();
    code.match_indices(word)
        .map(|(i, _)| i)
        .filter(|&i| {
            (i == 0 || !s.get(i - 1).is_some_and(|&b| is_ident(b)))
                && !s.get(i + word.len()).is_some_and(|&b| is_ident(b))
        })
        .collect()
}

/// The identifier that ends just before `i` (whitespace skipped), if any.
fn ident_before(code: &str, i: usize) -> &str {
    let s = code.as_bytes();
    let mut end = i;
    while end > 0 && s.get(end - 1).is_some_and(u8::is_ascii_whitespace) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && s.get(start - 1).is_some_and(|&b| is_ident(b)) {
        start -= 1;
    }
    &code[start..end]
}

/// The top-level comma-separated pieces of the group opening at `open`.
fn pieces(code: &str, open: usize) -> Vec<String> {
    let s = code.as_bytes();
    let close = match_close(s, open);
    let inner = code.get(open + 1..close.saturating_sub(1)).unwrap_or("");
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (k, b) in inner.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(inner[start..k].trim().to_owned());
                start = k + 1;
            }
            _ => {}
        }
    }
    out.push(inner[start..].trim().to_owned());
    out.retain(|p| !p.is_empty());
    out
}

/// The names bound by parameter pieces (`mut x: T`, `x`), by position as a call passes
/// them: a `self` receiver is left out, and a pattern binds `""`.
fn param_names(pieces: &[String]) -> Vec<String> {
    pieces
        .iter()
        .filter_map(|p| {
            let p = p.strip_prefix("mut ").map_or(p.as_str(), str::trim_start);
            let receiver = p.trim_start_matches(['&', ' ']);
            let receiver = receiver
                .strip_prefix("mut ")
                .unwrap_or(receiver)
                .trim_start();
            if receiver.starts_with("self") && ident_end(receiver.as_bytes(), 0) == 4 {
                return None;
            }
            let n = ident_end(p.as_bytes(), 0);
            Some(p[..n].to_owned())
        })
        .collect()
}

/// The values of a struct expression's fields (`what: x`, or shorthand `what`).
fn field_values(pieces: &[String]) -> Vec<String> {
    pieces
        .iter()
        .map(|p| {
            let n = ident_end(p.as_bytes(), 0);
            match p[n..].trim_start().strip_prefix(':') {
                Some(value) if !value.starts_with(':') => value.trim().to_owned(),
                _ => p.clone(),
            }
        })
        .collect()
}

/// The offset just past the `>` closing the generics opening at `i`.
fn angle_close(s: &[u8], mut i: usize) -> usize {
    let mut depth = 0usize;
    while let Some(&c) = s.get(i) {
        match c {
            b'-' if s.get(i + 1) == Some(&b'>') => i += 1,
            b'<' => depth += 1,
            b'>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    s.len()
}

/// From `i`: the offset of the `;` ending the statement at depth 0, or of the closer of
/// the enclosing group.
fn statement_end(s: &[u8], mut i: usize) -> usize {
    let mut depth = 0usize;
    while let Some(&c) = s.get(i) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' | b';' if depth == 0 => return i,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    s.len()
}

/// The offset of the closer of the group enclosing `i`.
fn block_end(s: &[u8], mut i: usize) -> usize {
    loop {
        i = statement_end(s, i);
        match s.get(i) {
            Some(b';') => i += 1,
            _ => return i,
        }
    }
}

/// The `fn`s, `let` closures and `macro_rules!` of `code` that may wrap a constructor.
fn definitions(code: &str) -> Vec<Def> {
    let s = code.as_bytes();
    let mut defs = Vec::new();
    for at in words(code, "fn") {
        let i = skip_ws(s, at + 2);
        let ne = ident_end(s, i);
        if ne == i {
            continue;
        }
        let mut j = skip_ws(s, ne);
        if s.get(j) == Some(&b'<') {
            j = skip_ws(s, angle_close(s, j));
        }
        if s.get(j) != Some(&b'(') {
            continue;
        }
        let params = param_names(&pieces(code, j));
        let mut k = match_close(s, j);
        let mut depth = 0usize;
        let mut body = None;
        while let Some(&c) = s.get(k) {
            match c {
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b'{' if depth == 0 => {
                    body = Some(k..match_close(s, k));
                    break;
                }
                b';' if depth == 0 => break,
                _ => {}
            }
            k += 1;
        }
        let Some(body) = body else { continue };
        let item_start = code[..at].rfind([';', '{', '}']).map_or(0, |p| p + 1);
        let public = !words(&code[item_start..at], "pub").is_empty();
        defs.push(Def {
            kind: Kind::Fn,
            name: code[i..ne].to_owned(),
            params,
            body,
            scope: 0..s.len(),
            public,
            at: i,
        });
    }
    for at in words(code, "let") {
        let mut i = skip_ws(s, at + 3);
        if words(&code[i..], "mut").first() == Some(&0) {
            i = skip_ws(s, i + 3);
        }
        let ne = ident_end(s, i);
        if ne == i {
            continue;
        }
        let mut j = skip_ws(s, ne);
        if s.get(j) != Some(&b'=') || matches!(s.get(j + 1), Some(b'=' | b'>')) {
            continue;
        }
        j = skip_ws(s, j + 1);
        if words(&code[j..], "move").first() == Some(&0) {
            j = skip_ws(s, j + 4);
        }
        if s.get(j) != Some(&b'|') || s.get(j + 1) == Some(&b'|') {
            continue;
        }
        let Some(close) = code[j + 1..].find('|').map(|k| j + 1 + k) else {
            continue;
        };
        let split: Vec<String> = code[j + 1..close]
            .split(',')
            .map(|p| p.trim().to_owned())
            .collect();
        let end = statement_end(s, close + 1);
        defs.push(Def {
            kind: Kind::Closure,
            name: code[i..ne].to_owned(),
            params: param_names(&split),
            body: close + 1..end,
            scope: end..block_end(s, end),
            public: false,
            at: i,
        });
    }
    for at in words(code, "macro_rules") {
        let mut i = at + "macro_rules".len();
        if s.get(i) != Some(&b'!') {
            continue;
        }
        i = skip_ws(s, i + 1);
        let ne = ident_end(s, i);
        let open = skip_ws(s, ne);
        if ne == i || !matches!(s.get(open), Some(b'(' | b'[' | b'{')) {
            continue;
        }
        let body = open..match_close(s, open);
        let mut params = Vec::new();
        for (k, _) in code[body.clone()].match_indices('$') {
            let start = body.start + k + 1;
            let end = ident_end(s, start);
            if end > start && s.get(skip_ws(s, end)) == Some(&b':') {
                params.push(code[start..end].to_owned());
            }
        }
        defs.push(Def {
            kind: Kind::Macro,
            name: code[i..ne].to_owned(),
            params,
            scope: body.end..s.len(),
            body,
            public: false,
            at: i,
        });
    }
    defs
}

/// The ranges holding the pattern of a `matches!`.
fn matches_patterns(code: &str) -> Vec<Range<usize>> {
    let s = code.as_bytes();
    words(code, "matches")
        .into_iter()
        .filter_map(|at| {
            let bang = at + "matches".len();
            let open = skip_ws(s, bang + 1);
            if s.get(bang) != Some(&b'!') || s.get(open) != Some(&b'(') {
                return None;
            }
            let close = match_close(s, open);
            let mut depth = 0usize;
            (open + 1..close).find_map(|k| match s.get(k) {
                Some(b'(' | b'[' | b'{') => {
                    depth += 1;
                    None
                }
                Some(b')' | b']' | b'}') => {
                    depth = depth.saturating_sub(1);
                    None
                }
                Some(b',') if depth == 0 => Some(k..close),
                _ => None,
            })
        })
        .collect()
}

/// Whether what follows the group ending at `end` makes it a pattern.
fn is_pattern_tail(s: &[u8], end: usize) -> bool {
    let mut i = end;
    loop {
        i = skip_ws(s, i);
        if matches!(s.get(i), Some(b')' | b']')) {
            i += 1;
        } else {
            break;
        }
    }
    match (s.get(i), s.get(i + 1)) {
        (Some(b'='), Some(b'>')) => true,
        (Some(b'='), next) => next != Some(&b'='),
        (Some(b'|'), next) => next != Some(&b'|'),
        (Some(b'i'), Some(b'f')) => !s.get(i + 2).is_some_and(|&b| is_ident(b)),
        _ => false,
    }
}

/// An occurrence of a constructor: where, the argument it takes the phrase from (every
/// argument of a macro), and what it calls.
struct Occurrence {
    at: usize,
    phrase: Vec<String>,
    via: String,
}

/// The occurrences of the variants and helpers in `code`, patterns left out.
fn base_occurrences(code: &str) -> Vec<Occurrence> {
    let s = code.as_bytes();
    let in_matches = matches_patterns(code);
    let mut out = Vec::new();
    for variant in VARIANTS {
        let needle = format!("BlendFailure::{variant}");
        for (at, _) in code.match_indices(&needle) {
            let end = at + needle.len();
            if (at > 0 && s.get(at - 1).is_some_and(|&b| is_ident(b)))
                || s.get(end).is_some_and(|&b| is_ident(b))
                || in_matches.iter().any(|r| r.contains(&at))
            {
                continue;
            }
            let open = skip_ws(s, end);
            // the phrase is a tuple variant's field, or `PassedOn`'s `what`
            let (args, phrase, tail) = match s.get(open) {
                Some(b'(') => {
                    let args = pieces(code, open);
                    let phrase = args.iter().take(1).cloned().collect();
                    (args, phrase, match_close(s, open))
                }
                Some(b'{') => {
                    let fields = pieces(code, open);
                    let phrase = fields
                        .iter()
                        .filter(|f| f.starts_with("what") && ident_end(f.as_bytes(), 0) == 4)
                        .flat_map(|f| field_values(std::slice::from_ref(f)))
                        .collect();
                    (field_values(&fields), phrase, match_close(s, open))
                }
                _ => (Vec::new(), Vec::new(), end),
            };
            if args.iter().any(|a| a == "_" || a == "..") || is_pattern_tail(s, tail) {
                continue;
            }
            out.push(Occurrence {
                at,
                phrase,
                via: needle.clone(),
            });
        }
    }
    for helper in HELPERS {
        for at in words(code, helper) {
            if ident_before(code, at) == "fn" {
                continue;
            }
            let open = skip_ws(s, at + helper.len());
            // `(operation, phrase)`
            let phrase = if s.get(open) == Some(&b'(') {
                pieces(code, open).into_iter().skip(1).take(1).collect()
            } else {
                Vec::new()
            };
            out.push(Occurrence {
                at,
                phrase,
                via: helper.to_owned(),
            });
        }
    }
    out
}

/// The calls of the wrapper `def` in its scope; `param` is the parameter it passes on as
/// the phrase (`None` for a macro: every argument).
fn wrapper_calls(code: &str, def: &Def, param: Option<usize>, wraps: &str) -> Vec<Occurrence> {
    let s = code.as_bytes();
    let via = format!("{} (wraps {wraps})", def.name);
    words(code, &def.name)
        .into_iter()
        .filter(|&at| def.scope.contains(&at) && at != def.at)
        .filter_map(|at| {
            let after = at + def.name.len();
            let open = match def.kind {
                Kind::Macro => {
                    if s.get(after) != Some(&b'!') {
                        return None;
                    }
                    skip_ws(s, after + 1)
                }
                Kind::Fn => {
                    if code[..at].ends_with("SolidError::") || ident_before(code, at) == "fn" {
                        return None;
                    }
                    skip_ws(s, after)
                }
                Kind::Closure => {
                    if matches!(s.get(at.wrapping_sub(1)), Some(b'.' | b':')) {
                        return None;
                    }
                    skip_ws(s, after)
                }
            };
            matches!(s.get(open), Some(b'(' | b'[' | b'{')).then(|| {
                let args = pieces(code, open);
                Occurrence {
                    at,
                    phrase: match param {
                        Some(k) => args.into_iter().skip(k).take(1).collect(),
                        None => args,
                    },
                    via: via.clone(),
                }
            })
        })
        .collect()
}

/// The sites in a file's library code (from [`library_code`]) that build a phrase
/// cause, sorted by line. Panics on a wrapper `fn` visible outside the file.
pub(in crate::error) fn sites_in(code: &str, file: &str) -> Vec<Site> {
    let defs = definitions(code);
    let mut queue = base_occurrences(code);
    let mut wrapped = vec![false; defs.len()];
    let mut sites = Vec::new();
    while let Some(occ) = queue.pop() {
        // the innermost enclosing definition that has the phrase as a parameter
        let mut enclosing: Vec<usize> = (0..defs.len())
            .filter(|&d| defs[d].body.contains(&occ.at))
            .collect();
        enclosing.sort_by_key(|&d| std::cmp::Reverse(defs[d].body.start));
        let forwards_to = enclosing.into_iter().find_map(|d| {
            let def = &defs[d];
            occ.phrase.iter().find_map(|a| match def.kind {
                Kind::Macro => a
                    .strip_prefix('$')
                    .is_some_and(|p| def.params.iter().any(|q| q == p))
                    .then_some((d, None)),
                Kind::Fn | Kind::Closure => {
                    def.params.iter().position(|q| q == a).map(|k| (d, Some(k)))
                }
            })
        });
        match forwards_to {
            Some((d, _))
                if defs[d].kind == Kind::Fn && HELPERS.contains(&defs[d].name.as_str()) => {}
            Some((d, param)) => {
                if !wrapped[d] {
                    wrapped[d] = true;
                    assert!(
                        !defs[d].public,
                        "{file}:{}: `{}` passes on the phrase of {} and is visible outside \
                         its file; the phrase-site scan counts only calls in its own file",
                        1 + code[..occ.at].matches('\n').count(),
                        defs[d].name,
                        occ.via
                    );
                    queue.extend(wrapper_calls(code, &defs[d], param, &occ.via));
                }
            }
            None => sites.push(Site {
                line: 1 + code[..occ.at].matches('\n').count(),
                via: occ.via,
            }),
        }
    }
    sites.sort_by(|a, b| (a.line, &a.via).cmp(&(b.line, &b.via)));
    sites
}

/// The library files of `solidcore` and `solidcore-ops` that build phrase causes, with
/// their sites, as (path relative to `crates/`, sites), sorted by path.
pub(in crate::error) fn phrase_sites() -> Vec<(String, Vec<Site>)> {
    let solidcore = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ops = solidcore.join("../solidcore-ops");
    let mut out = Vec::new();
    for (name, dir, least) in [("solidcore", solidcore, 400), ("solidcore-ops", &*ops, 20)] {
        for (relative, text) in crate_library_files(dir, least) {
            let file = format!("{name}/src/{relative}");
            let sites = sites_in(&library_code(&text), &file);
            if !sites.is_empty() {
                out.push((file, sites));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Every library file that builds a phrase cause, with its exact number of sites (see the
/// module docs for what counts). Converting a site lowers its file's count here in the
/// same PR, and a file that reaches 0 loses its line. The count never rises: a new
/// failure gets a classed cause. Code that moves between files moves its sites: the PR
/// lowers one file's count and raises the other's by the same number, and the total stays
/// (the failure says so). Per file, as the bare-status ratchets are (`assert_waiting`), so
/// a rise hidden by a conversion in another file still fails.
#[rustfmt::skip] // one file per line, so conversions in parallel merge cleanly
const PINNED: &[(&str, usize)] = &[
    ("solidcore/src/blend/blend_corner.rs", 5),
    ("solidcore/src/blend/blend_corner/ball.rs", 15),
    ("solidcore/src/blend/blend_corner/chamfer.rs", 8),
    ("solidcore/src/blend/blend_corner/end.rs", 9),
    ("solidcore/src/blend/blend_corner/mixed.rs", 25),
    ("solidcore/src/blend/blend_corner/mixed/algebra.rs", 1),
    ("solidcore/src/blend/blend_corner/setback.rs", 27),
    ("solidcore/src/blend/blend_corner/swept.rs", 7),
    ("solidcore/src/blend/blend_setback.rs", 29),
    ("solidcore/src/blend/feature_executive.rs", 32),
    ("solidcore/src/blend/fillet_cliff_overflow.rs", 56),
    ("solidcore/src/blend/fillet_corner/p1.rs", 46),
    ("solidcore/src/blend/fillet_corner/p2.rs", 17),
    ("solidcore/src/blend/fillet_corner/p3.rs", 25),
    ("solidcore/src/blend/fillet_corner/topology.rs", 7),
    ("solidcore/src/blend/fillet_corner/topology/ball.rs", 7),
    ("solidcore/src/blend/fillet_corner/topology/closed_turn.rs", 2),
    ("solidcore/src/blend/fillet_corner/topology/concave_4x2.rs", 2),
    ("solidcore/src/blend/fillet_corner/topology/concave_nxn.rs", 2),
    ("solidcore/src/blend/fillet_corner/topology/end_corners.rs", 4),
    ("solidcore/src/blend/fillet_corner/topology/end_smooth.rs", 8),
    ("solidcore/src/blend/fillet_corner/topology/join.rs", 19),
    ("solidcore/src/blend/fillet_corner/topology/mixed.rs", 9),
    ("solidcore/src/blend/fillet_corner/topology/mixed_3x2.rs", 5),
    ("solidcore/src/blend/fillet_corner/topology/nx1_end.rs", 1),
    ("solidcore/src/blend/fillet_corner/topology/swept_turn.rs", 8),
    ("solidcore/src/blend/fillet_custom_solver.rs", 20),
    ("solidcore/src/blend/fillet_exact.rs", 19),
    ("solidcore/src/blend/fillet_exact/rational.rs", 23),
    ("solidcore/src/blend/fillet_executive/api.rs", 1),
    ("solidcore/src/blend/fillet_executive/p1.rs", 17),
    ("solidcore/src/blend/fillet_executive/p2.rs", 35),
    ("solidcore/src/blend/fillet_executive/p3.rs", 7),
    ("solidcore/src/blend/fillet_executive/session.rs", 43),
    ("solidcore/src/blend/fillet_executive/session/ball_regular.rs", 8),
    ("solidcore/src/blend/fillet_executive/session/band_cuts.rs", 13),
    ("solidcore/src/blend/fillet_executive/session/cliff_overflow.rs", 21),
    ("solidcore/src/blend/fillet_executive/session/concave_nxn.rs", 1),
    ("solidcore/src/blend/fillet_executive/session/corner_marks.rs", 1),
    ("solidcore/src/blend/fillet_executive/session/ease.rs", 49),
    ("solidcore/src/blend/fillet_executive/session/finish.rs", 4),
    ("solidcore/src/blend/fillet_executive/session/fit_size.rs", 20),
    ("solidcore/src/blend/fillet_executive/session/insertion/geometry.rs", 2),
    ("solidcore/src/blend/fillet_executive/session/insertion/geometry/intersector.rs", 1),
    ("solidcore/src/blend/fillet_executive/session/insertion/geometry/topology_objects.rs", 1),
    ("solidcore/src/blend/fillet_executive/session/insertion/live.rs", 3),
    ("solidcore/src/blend/fillet_executive/session/keep_features.rs", 16),
    ("solidcore/src/blend/fillet_executive/session/law_corners.rs", 9),
    ("solidcore/src/blend/fillet_executive/session/original_trim.rs", 1),
    ("solidcore/src/blend/fillet_executive/session/outline_rims.rs", 4),
    ("solidcore/src/blend/fillet_executive/session/piecewise.rs", 13),
    ("solidcore/src/blend/fillet_executive/session/surface.rs", 4),
    ("solidcore/src/blend/fillet_executive/session/tangent_rollover.rs", 5),
    ("solidcore/src/blend/fillet_executive/session/trace_reach.rs", 3),
    ("solidcore/src/blend/fillet_executive/session/trim_at_creases.rs", 34),
    ("solidcore/src/blend/fillet_executive/session/two_offsets.rs", 18),
    ("solidcore/src/blend/fillet_geom/p1.rs", 62),
    ("solidcore/src/blend/fillet_geom/p2.rs", 112),
    ("solidcore/src/blend/fillet_geom/p3.rs", 47),
    ("solidcore/src/blend/fillet_geom/p4.rs", 120),
    ("solidcore/src/blend/fillet_geom/p5.rs", 117),
    ("solidcore/src/blend/fillet_geom/p6.rs", 88),
    ("solidcore/src/blend/fillet_geom/surface_plane_curve.rs", 13),
    ("solidcore/src/blend/fillet_geom/tangent_end_rib.rs", 19),
    ("solidcore/src/blend/fillet_intersector.rs", 13),
    ("solidcore/src/blend/fillet_outline.rs", 3),
    ("solidcore/src/blend/fillet_overflow.rs", 31),
    ("solidcore/src/blend/fillet_removal.rs", 5),
    ("solidcore/src/blend/fillet_removal/ring.rs", 5),
    ("solidcore/src/blend/fillet_section/certificate.rs", 13),
    ("solidcore/src/blend/fillet_section/continuation.rs", 2),
    ("solidcore/src/blend/fillet_section/definition.rs", 6),
    ("solidcore/src/blend/fillet_section/definition_box.rs", 12),
    ("solidcore/src/blend/fillet_section/feet_tm.rs", 6),
    ("solidcore/src/blend/fillet_section/section.rs", 27),
    ("solidcore/src/blend/fillet_section/series.rs", 4),
    ("solidcore/src/blend/fillet_section/station.rs", 15),
    ("solidcore/src/blend/fillet_solver/curve_blend_sih.rs", 14),
    ("solidcore/src/blend/fillet_solver/p1.rs", 25),
    ("solidcore/src/blend/fillet_solver/p1/rails.rs", 7),
    ("solidcore/src/blend/fillet_solver/p2.rs", 13),
    ("solidcore/src/blend/fillet_solver/p3.rs", 25),
    ("solidcore/src/blend/fillet_solver/p4.rs", 38),
    ("solidcore/src/blend/fillet_solver/p5.rs", 7),
    ("solidcore/src/blend/fillet_solver/p5/end_sections_fit.rs", 13),
    ("solidcore/src/blend/fillet_solver/p5/rails_fit.rs", 25),
    ("solidcore/src/blend/fillet_solver/p5/recovery.rs", 6),
    ("solidcore/src/blend/fillet_solver/surfaces.rs", 2),
    ("solidcore/src/blend/fillet_standard_solver/p1.rs", 32),
    ("solidcore/src/blend/fillet_standard_solver/p2.rs", 28),
    ("solidcore/src/blend/fillet_standard_solver/surface_surface.rs", 2),
    ("solidcore/src/blend/fillet_standard_solver/touch_circle.rs", 1),
    ("solidcore/src/blend/fillet_tangent_rollover.rs", 2),
    ("solidcore/src/blend/hcr.rs", 2),
    ("solidcore/src/blend/identify_fillet.rs", 4),
    ("solidcore/src/blend/law_corners.rs", 7),
    ("solidcore/src/blend/round_room.rs", 1),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    /// The number of blend sites that build a phrase cause only falls.
    #[test]
    fn the_phrase_cause_sites_only_shrink() {
        let found = phrase_sites();
        let counts: Vec<(&str, usize)> = found.iter().map(|(f, s)| (f.as_str(), s.len())).collect();
        if counts != PINNED {
            let mut report = String::new();
            let pinned_total: usize = PINNED.iter().map(|(_, n)| n).sum();
            let found_total: usize = counts.iter().map(|(_, n)| n).sum();
            writeln!(
                report,
                "phrase-cause sites: pinned {pinned_total}, found {found_total}"
            )
            .unwrap();
            if pinned_total == found_total {
                writeln!(
                    report,
                    "the total is unchanged: if code moved between files, update both lines"
                )
                .unwrap();
            }
            let mut files: Vec<&str> = PINNED.iter().map(|(f, _)| *f).collect();
            files.extend(counts.iter().map(|(f, _)| *f));
            files.sort_unstable();
            files.dedup();
            for file in files {
                let pinned = PINNED
                    .iter()
                    .find(|(f, _)| *f == file)
                    .map_or(0, |(_, n)| *n);
                let sites = found.iter().find(|(f, _)| f == file).map(|(_, s)| s);
                let n = sites.map_or(0, Vec::len);
                if pinned != n {
                    writeln!(report, "{file}: pinned {pinned}, found {n}").unwrap();
                    for site in sites.into_iter().flatten() {
                        writeln!(report, "    {file}:{}  {}", site.line, site.via).unwrap();
                    }
                }
            }
            writeln!(report, "the pinned list, as found:").unwrap();
            for (file, n) in &counts {
                writeln!(report, "    (\"{file}\", {n}),").unwrap();
            }
            panic!("{report}");
        }
        for (file, n) in PINNED {
            assert!(*n > 0, "{file} builds no phrase cause: remove its line");
        }
        assert!(
            PINNED.windows(2).all(|w| w[0].0 < w[1].0),
            "PINNED is sorted by path, one file per line"
        );
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
