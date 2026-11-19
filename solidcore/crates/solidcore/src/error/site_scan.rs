//! Test-only scanner behind the per-file site counts of the typed-errors work: the sites
//! in library code that build a phrase cause ([`super::phrase_sites`] for the blend, each
//! area's own error file for the others), an unreviewed stage failure
//! ([`super::stage_sites`]) or a bare status ([`super::bare_scan`]). Each count is pinned per
//! file and only falls; the conversion guide (`docs/failure-codes-and-reproduction.md` §6)
//! says how a site is converted.
//!
//! A [`Scan`] names what builds a site ([`Builder`]) and which local definitions are
//! wrappers ([`Wrappers`]). **What counts as one site**, in a file's library code (comments,
//! string contents and every `#[cfg(test)]` item blanked: [`super::bare_scan::library_code`]):
//! - a construction of a variant the scan names, by any path ending in `<Enum>::<variant>`;
//!   a pattern is not one (a variant followed by `=>`, `|`, `if` or `=`, binding `_` or `..`,
//!   or in the pattern of a `matches!`);
//! - a call of a helper the scan names (its own definition is not a site, nor is what its
//!   body builds);
//! - an occurrence of a text the scan names (the bare statuses, counted as text);
//! - a call of a local *wrapper*: a `let` closure, a private `fn` or a `macro_rules!` that
//!   builds one of the above in the way the scan's [`Wrappers`] says. The wrapper's body is
//!   not a site; each call of it in its scope is (the rest of the enclosing block for a
//!   closure, the file for a `fn` or a macro).
//!
//! Under [`Wrappers::Phrase`], a wrapper `fn` visible outside its file fails the scan, since
//! its calls elsewhere would not be seen: make it private, or list it as a
//! [`Builder::Shared`] helper of the files that call it.

use super::bare_scan::{crate_library_files, is_ident, library_code, match_close};
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

/// What builds a site.
#[derive(Debug, Clone, Copy)]
pub(in crate::error) enum Builder {
    /// A construction of the variant (enum, variant) by any path ending in
    /// `<enum>::<variant>`; a pattern is not one. Its phrase is a tuple variant's first field
    /// or a struct variant's `what`.
    Variant(&'static str, &'static str),
    /// A call of the `SolidError` helper named, by its name alone; its phrase is the
    /// argument at the index (`None`: it takes none). Its definition is not a site.
    Helper(&'static str, Option<usize>),
    /// A call of the `SolidError` helper named, as `SolidError::<name>` or `Self::<name>` (so
    /// a local `fn` of the same name is not it); its phrase is the argument at the index.
    /// Its definition is not a site.
    Method(&'static str, Option<usize>),
    /// A text that builds one wherever it stands in library code (a bare status's
    /// constructor). Counted as text: a pattern that names it counts too.
    Text(&'static str),
    /// A wrapper shared with the files under a path (path relative to `crates/`, without
    /// `.rs`: the module's file and its children's), named, with its phrase at the index: a
    /// helper there. A wrapper `fn` visible outside its file is listed so, or fails the scan.
    Shared(&'static str, &'static str, Option<usize>),
}

impl Builder {
    /// The helper this builder names in `file`, with its phrase argument.
    fn helper_in(self, file: &str) -> Option<(&'static str, Option<usize>)> {
        match self {
            Self::Helper(name, phrase) | Self::Method(name, phrase) => Some((name, phrase)),
            Self::Shared(path, name, phrase)
                if file
                    .strip_prefix(path)
                    .is_some_and(|r| r == ".rs" || r.starts_with('/')) =>
            {
                Some((name, phrase))
            }
            _ => None,
        }
    }
}

/// Which local definitions are wrappers, whose calls count in place of their body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::error) enum Wrappers {
    /// One that passes one of its own parameters on as the phrase (a variant's phrase, a
    /// helper's phrase argument, the same parameter of a wrapper, any argument of a macro).
    /// Passing on only the operation does not make a wrapper, and a closure that builds a
    /// fixed phrase is one site however often it runs. Wrappers of wrappers resolve the same
    /// way. The phrase scans use it.
    Phrase,
    /// One whose body is one expression, with no statement and no control flow (`let`,
    /// `if`, `match`, `loop`, `while`, `for`, `return`), that builds one directly (a
    /// macro: any body that builds one), whatever it passes on: `fn sm_err() -> SolidError {
    /// SolidError::sm_err(OP) }`, `|v| v.ok_or_else(|| SolidError::null_pointer(OP))`. A
    /// definition whose one expression only calls a wrapper is not one: its call is the site.
    /// A `fn` visible outside its file is not one: its body is the site (its callers
    /// elsewhere are not seen), as is the body of a wrapper never called. The bare-status and
    /// stage-failure scans use it, as what these build carries no phrase to tell one call
    /// from another.
    Thin,
}

/// A count of sites: what builds one, and which definitions are wrappers.
#[derive(Debug, Clone, Copy)]
pub(in crate::error) struct Scan {
    /// What builds a site.
    pub builders: &'static [Builder],
    /// Which definitions are wrappers.
    pub wrappers: Wrappers,
}

/// One site: its line, and what it calls.
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

/// A local definition that may wrap a constructor.
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
    /// Whether its body is one expression with no control flow ([`Wrappers::Thin`]).
    thin: bool,
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

/// Whether `body` (a `fn`'s block, or a closure's body after its `|..|`) is one expression
/// with no statement and no control flow.
fn is_thin(code: &str, body: Range<usize>) -> bool {
    let s = code.as_bytes();
    let mut inner = body.clone();
    let start = skip_ws(s, body.start);
    let open = if code[start..body.end].starts_with("->") {
        code[start..body.end].find('{').map(|k| start + k)
    } else {
        (s.get(start) == Some(&b'{')).then_some(start)
    };
    if let Some(open) = open {
        let close = match_close(s, open);
        if code[close..body.end].trim().is_empty() {
            inner = open + 1..close.saturating_sub(1);
        }
    }
    let text = &code[inner.clone()];
    let mut depth = 0usize;
    for b in text.bytes() {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return false,
            _ => {}
        }
    }
    ["let", "if", "match", "loop", "while", "for", "return"]
        .iter()
        .all(|w| words(text, w).is_empty())
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
            thin: is_thin(code, body.clone()),
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
        if s.get(j) != Some(&b'|') {
            continue;
        }
        // `||`: a closure without parameters
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
            thin: is_thin(code, close + 1..end),
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
            thin: true,
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
/// argument of a macro), what it calls, whether it is a call of a wrapper, and whether it
/// is a pattern (a [`Builder::Text`] counts in one, where it stands).
struct Occurrence {
    at: usize,
    phrase: Vec<String>,
    via: String,
    wrapped: bool,
    pattern: bool,
}

/// The occurrences of the scan's builders in `code`, patterns left out, in the order of
/// the builders.
fn base_occurrences(scan: &Scan, code: &str, file: &str) -> Vec<Occurrence> {
    let s = code.as_bytes();
    let in_matches = matches_patterns(code);
    let mut out = Vec::new();
    for builder in scan.builders {
        match *builder {
            Builder::Variant(enum_name, variant) => {
                let needle = format!("{enum_name}::{variant}");
                for (at, _) in code.match_indices(&needle) {
                    let end = at + needle.len();
                    if (at > 0 && s.get(at - 1).is_some_and(|&b| is_ident(b)))
                        || s.get(end).is_some_and(|&b| is_ident(b))
                        || in_matches.iter().any(|r| r.contains(&at))
                    {
                        continue;
                    }
                    let open = skip_ws(s, end);
                    // the phrase is a tuple variant's field, or a struct variant's `what`
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
                                .filter(|f| {
                                    f.starts_with("what") && ident_end(f.as_bytes(), 0) == 4
                                })
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
                        wrapped: false,
                        pattern: false,
                    });
                }
            }
            Builder::Helper(..) | Builder::Method(..) | Builder::Shared(..) => {
                let Some((helper, phrase_at)) = builder.helper_in(file) else {
                    continue;
                };
                let qualified = matches!(builder, Builder::Method(..));
                for at in words(code, helper) {
                    if ident_before(code, at) == "fn"
                        || (qualified
                            && !(code[..at].ends_with("SolidError::")
                                || code[..at].ends_with("Self::")))
                    {
                        continue;
                    }
                    let open = skip_ws(s, at + helper.len());
                    let phrase = match phrase_at {
                        Some(k) if s.get(open) == Some(&b'(') => {
                            pieces(code, open).into_iter().skip(k).take(1).collect()
                        }
                        _ => Vec::new(),
                    };
                    out.push(Occurrence {
                        at,
                        phrase,
                        via: helper.to_owned(),
                        wrapped: false,
                        pattern: false,
                    });
                }
            }
            Builder::Text(text) => {
                for (at, _) in code.match_indices(text) {
                    let open = (at + text.len()).saturating_sub(1);
                    let group = matches!(s.get(open), Some(b'(' | b'{'));
                    let pattern = in_matches.iter().any(|r| r.contains(&at))
                        || (group
                            && (pieces(code, open)
                                .iter()
                                .any(|a| a == "_" || a == ".." || a.ends_with(".."))
                                || is_pattern_tail(s, match_close(s, open))));
                    out.push(Occurrence {
                        at,
                        phrase: Vec::new(),
                        via: text.trim_end_matches(['(', '{', ' ']).to_owned(),
                        wrapped: false,
                        pattern,
                    });
                }
            }
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
                    wrapped: true,
                    pattern: false,
                }
            })
        })
        .collect()
}

/// The wrapper `occ` is built in, if any: the definition, and the parameter it passes on as
/// the phrase (`None` for a macro, or for a [`Wrappers::Thin`] wrapper).
fn forwards_to(scan: &Scan, defs: &[Def], occ: &Occurrence) -> Option<(usize, Option<usize>)> {
    let mut enclosing: Vec<usize> = (0..defs.len())
        .filter(|&d| defs[d].body.contains(&occ.at))
        .collect();
    enclosing.sort_by_key(|&d| std::cmp::Reverse(defs[d].body.start));
    match scan.wrappers {
        // the innermost enclosing definition that has the phrase as a parameter
        Wrappers::Phrase => enclosing.into_iter().find_map(|d| {
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
        }),
        // the innermost enclosing definition, when it is thin and builds it directly
        Wrappers::Thin => enclosing
            .first()
            .filter(|&&d| {
                defs[d].thin
                    && !defs[d].public
                    && !occ.pattern
                    && (!occ.wrapped || defs[d].kind == Kind::Macro)
            })
            .map(|&d| (d, None)),
    }
}

/// The helpers among the scan's builders, in `file`.
fn helpers<'a>(scan: &'a Scan, file: &'a str) -> impl Iterator<Item = &'static str> + 'a {
    scan.builders
        .iter()
        .filter_map(move |b| b.helper_in(file).map(|(name, _)| name))
}

/// The sites in a file's library code (from [`library_code`]) that `scan` counts, sorted
/// by line. Panics on a wrapper `fn` visible outside the file.
pub(in crate::error) fn sites_in(scan: &Scan, code: &str, file: &str) -> Vec<Site> {
    let defs = definitions(code);
    let mut queue = base_occurrences(scan, code, file);
    // per definition: not yet a wrapper, or a wrapper with calls, or one never called
    let mut wrapped: Vec<Option<bool>> = vec![None; defs.len()];
    let mut sites = Vec::new();
    let is_helper =
        |d: usize| defs[d].kind == Kind::Fn && helpers(scan, file).any(|h| h == defs[d].name);
    while let Some(occ) = queue.pop() {
        let innermost = (0..defs.len())
            .filter(|&d| defs[d].body.contains(&occ.at))
            .max_by_key(|&d| defs[d].body.start);
        // what a helper's own body builds is the helper, not a site
        if scan.wrappers == Wrappers::Thin && innermost.is_some_and(is_helper) {
            continue;
        }
        match forwards_to(scan, &defs, &occ) {
            Some((d, _)) if is_helper(d) => {}
            // a thin wrapper never called is counted at its body, as dead code still builds
            Some((d, _)) if scan.wrappers == Wrappers::Thin && wrapped[d] == Some(false) => {
                sites.push(Site {
                    line: 1 + code[..occ.at].matches('\n').count(),
                    via: occ.via,
                });
            }
            Some((d, param)) => {
                if wrapped[d].is_none() {
                    assert!(
                        !defs[d].public,
                        "{file}:{}: `{}` wraps {} and is visible outside its file; the \
                         site scan counts only calls in its own file",
                        1 + code[..occ.at].matches('\n').count(),
                        defs[d].name,
                        occ.via
                    );
                    let param = match scan.wrappers {
                        Wrappers::Phrase => param,
                        Wrappers::Thin => None,
                    };
                    let calls = wrapper_calls(code, &defs[d], param, &occ.via);
                    wrapped[d] = Some(!calls.is_empty());
                    if calls.is_empty() && scan.wrappers == Wrappers::Thin {
                        sites.push(Site {
                            line: 1 + code[..occ.at].matches('\n').count(),
                            via: occ.via,
                        });
                    }
                    queue.extend(calls);
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

/// The library code of every library file of `solidcore` and `solidcore-ops`, as (path
/// relative to `crates/`, library code), sorted by path; read once.
fn crate_sources() -> &'static [(String, String)] {
    static SOURCES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let solidcore = Path::new(env!("CARGO_MANIFEST_DIR"));
        let ops = solidcore.join("../solidcore-ops");
        let mut out = Vec::new();
        for (name, dir, least) in [("solidcore", solidcore, 400), ("solidcore-ops", &*ops, 20)] {
            for (relative, text) in crate_library_files(dir, least) {
                out.push((format!("{name}/src/{relative}"), library_code(&text)));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    })
}

/// The library files of `solidcore` and `solidcore-ops` with a site `scan` counts, with
/// their sites, as (path relative to `crates/`, sites), sorted by path.
pub(in crate::error) fn crate_sites(scan: &Scan) -> Vec<(String, Vec<Site>)> {
    crate_sources()
        .iter()
        .filter_map(|(file, code)| {
            let sites = sites_in(scan, code, file);
            (!sites.is_empty()).then(|| (file.clone(), sites))
        })
        .collect()
}

/// Panics, with a report of what changed, unless the files with a site `scan` counts
/// (sites, by path) are exactly `pinned` (path, count; sorted by path, one file per line,
/// every count above 0). `what` names the count in the report.
pub(in crate::error) fn assert_pinned(
    what: &str,
    found: &[(String, Vec<Site>)],
    pinned: &[(&str, usize)],
) {
    use std::fmt::Write as _;
    let counts: Vec<(&str, usize)> = found.iter().map(|(f, s)| (f.as_str(), s.len())).collect();
    if counts != pinned {
        let mut report = String::new();
        let pinned_total: usize = pinned.iter().map(|(_, n)| n).sum();
        let found_total: usize = counts.iter().map(|(_, n)| n).sum();
        writeln!(report, "{what}: pinned {pinned_total}, found {found_total}").unwrap();
        if pinned_total == found_total {
            writeln!(
                report,
                "the total is unchanged: if code moved between files, update both lines"
            )
            .unwrap();
        }
        let mut files: Vec<&str> = pinned.iter().map(|(f, _)| *f).collect();
        files.extend(counts.iter().map(|(f, _)| *f));
        files.sort_unstable();
        files.dedup();
        for file in files {
            let pin = pinned
                .iter()
                .find(|(f, _)| *f == file)
                .map_or(0, |(_, n)| *n);
            let sites = found.iter().find(|(f, _)| f == file).map(|(_, s)| s);
            let n = sites.map_or(0, Vec::len);
            if pin != n {
                writeln!(report, "{file}: pinned {pin}, found {n}").unwrap();
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
    for (file, n) in pinned {
        assert!(*n > 0, "{file} has no {what}: remove its line");
    }
    assert!(
        pinned.windows(2).all(|w| w[0].0 < w[1].0),
        "the {what} list is sorted by path, one file per line"
    );
}
