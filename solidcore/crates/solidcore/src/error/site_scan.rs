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
//!   or in the pattern of a `matches!`, with any delimiter, up to its guard: a variant in
//!   the guard is a value, and counts). A `use` that imports a counted variant, or renames
//!   or glob-imports its enum, fails the scan: what it brings in would not be seen;
//! - a call of a helper the scan names (its own definition is not a site, nor is what its
//!   body builds; a [`Builder::Method`]'s definition is a method of `SolidError`, so a free
//!   `fn` of the same name is a local wrapper);
//! - an occurrence of a text the scan names (the bare statuses, counted as text);
//! - a call of a local *wrapper*: a `let` closure, a private `fn` or a `macro_rules!` that
//!   builds one of the above in the way the scan's [`Wrappers`] says. The wrapper's body is
//!   not a site; each call of it in its scope is. For a [`Wrappers::Thin`] wrapper, so is
//!   each use of a closure's or a `fn`'s name as a value (`ok_or_else(fail)`, as an argument
//!   or a result; a field, a method or a path segment of the same name is not one). A
//!   [`Wrappers::Phrase`] wrapper takes its phrase as a parameter, so its name passed as a
//!   value carries no phrase of the site's own and is not counted there. The scope is the rest of the
//!   enclosing block for a closure and the file for a `fn` or a macro, less what follows a
//!   `let` that binds the closure's or the `fn`'s name again, within that `let`'s block. A
//!   macro is always a wrapper when its body builds one, whatever its body holds: each call
//!   expands it.
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
    /// Whether a `fn` of the helper's name (at `at` in `code`) is the helper's definition: a
    /// [`Builder::Method`]'s is a method of `SolidError`, so a free `fn` of the same name is a
    /// local wrapper.
    fn defines(self, code: &str, at: usize) -> bool {
        !matches!(self, Self::Method(..)) || in_solid_error_impl(code, at)
    }

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
        let name = &code[i..ne];
        defs.push(Def {
            kind: Kind::Closure,
            name: name.to_owned(),
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
            if s.get(bang) != Some(&b'!') || !matches!(s.get(open), Some(b'(' | b'[' | b'{')) {
                return None;
            }
            let close = match_close(s, open);
            let comma = top_level(s, open + 1..close, |k| s.get(k) == Some(&b','))?;
            // the pattern ends at a guard: a variant in the guard is a value
            let guard = top_level(s, comma + 1..close, |k| {
                code[k..].starts_with("if")
                    && !s.get(k.wrapping_sub(1)).is_some_and(|&b| is_ident(b))
                    && !s.get(k + 2).is_some_and(|&b| is_ident(b))
            });
            Some(comma..guard.unwrap_or(close))
        })
        .collect()
}

/// Whether a `let` between `from` and `at` binds `name` again in a block that `at` is in:
/// a call there is of the new binding, not the closure's.
fn rebound(code: &str, name: &str, from: usize, at: usize) -> bool {
    let s = code.as_bytes();
    words(&code[from..at], "let").into_iter().any(|k| {
        let k = from + k;
        let mut n = skip_ws(s, k + 3);
        if words(&code[n..at], "mut").first() == Some(&0) {
            n = skip_ws(s, n + 3);
        }
        code[n..].starts_with(name) && ident_end(s, n) == n + name.len() && block_end(s, k) >= at
    })
}

/// Whether `at` lies in a `use` declaration.
fn in_use(code: &str, at: usize) -> bool {
    words(&code[..at], "use")
        .last()
        .is_some_and(|&u| !code[u..at].contains(';'))
}

/// The 1-based line of `at`.
fn line_of(code: &str, at: usize) -> usize {
    1 + code[..at].matches('\n').count()
}

/// Whether the `fn` named at `at` is a method in an `impl` of `SolidError`.
fn in_solid_error_impl(code: &str, at: usize) -> bool {
    let s = code.as_bytes();
    words(&code[..at], "impl").into_iter().rev().any(|i| {
        code[i..at].find('{').is_some_and(|k| {
            let open = i + k;
            code[i..open].contains("SolidError") && match_close(s, open) > at
        })
    })
}

/// The first offset in `range` at the top level of its group (no bracket open) where
/// `hit` holds.
fn top_level(s: &[u8], range: Range<usize>, hit: impl Fn(usize) -> bool) -> Option<usize> {
    let mut depth = 0usize;
    range.into_iter().find(|&k| match s.get(k) {
        Some(b'(' | b'[' | b'{') => {
            depth += 1;
            false
        }
        Some(b')' | b']' | b'}') => {
            depth = depth.saturating_sub(1);
            false
        }
        _ => depth == 0 && hit(k),
    })
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
                // a construction is matched by its path: one the scan would not see is refused
                for at in words(code, enum_name) {
                    let after = skip_ws(s, at + enum_name.len());
                    let renamed = words(&code[after..], "as").first() == Some(&0);
                    let glob = code[after..].starts_with("::")
                        && code[skip_ws(s, after + 2)..].starts_with('*');
                    assert!(
                        !(in_use(code, at) && (renamed || glob)),
                        "{file}:{}: a `use` imports `{enum_name}` {}; the site scan matches \
                         `{enum_name}::<variant>` paths only, so name it in full",
                        line_of(code, at),
                        if renamed {
                            "under another name"
                        } else {
                            "with a glob"
                        },
                    );
                }
                let needle = format!("{enum_name}::{variant}");
                for (at, _) in code.match_indices(&needle) {
                    let end = at + needle.len();
                    if (at > 0 && s.get(at - 1).is_some_and(|&b| is_ident(b)))
                        || s.get(end).is_some_and(|&b| is_ident(b))
                        || in_matches.iter().any(|r| r.contains(&at))
                    {
                        continue;
                    }
                    // an import is no construction, and the short name it brings in would
                    // not be seen
                    assert!(
                        !in_use(code, at),
                        "{file}:{}: a `use` imports `{needle}`; the site scan matches the full \
                         path only, so build it as `{needle}`",
                        line_of(code, at),
                    );
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

/// The calls of the wrapper `def` in its scope, and with `values` the uses of its name as a
/// value; `param` is the parameter it passes on as the phrase (`None` for a macro: every
/// argument).
fn wrapper_calls(
    code: &str,
    def: &Def,
    param: Option<usize>,
    wraps: &str,
    values: bool,
) -> Vec<Occurrence> {
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
                    // a `let` of the same name hides the `fn` for the rest of its block
                    if code[..at].ends_with("SolidError::")
                        || ident_before(code, at) == "fn"
                        || rebound(code, &def.name, 0, at)
                    {
                        return None;
                    }
                    skip_ws(s, after)
                }
                Kind::Closure => {
                    if matches!(s.get(at.wrapping_sub(1)), Some(b'.' | b':'))
                        || rebound(code, &def.name, def.scope.start, at)
                    {
                        return None;
                    }
                    skip_ws(s, after)
                }
            };
            if matches!(s.get(open), Some(b'(' | b'[' | b'{')) {
                let args = pieces(code, open);
                return Some(Occurrence {
                    at,
                    phrase: match param {
                        Some(k) => args.into_iter().skip(k).take(1).collect(),
                        None => args,
                    },
                    via: via.clone(),
                    wrapped: true,
                    pattern: false,
                });
            }
            // the name passed as a value (`ok_or_else(fail)`): each such use is a site too
            (values && def.kind != Kind::Macro && is_value_use(code, at, def.name.len())).then(
                || Occurrence {
                    at,
                    phrase: Vec::new(),
                    via: via.clone(),
                    wrapped: true,
                    pattern: false,
                },
            )
        })
        .collect()
}

/// Whether the identifier of `len` bytes at `at` stands as a value of its own, an argument
/// or a result (`f(name)`, `f(x, name)`, `name;`): not a field, a method or a path segment of
/// something else, nor a binding, a parameter or a struct field's name.
fn is_value_use(code: &str, at: usize, len: usize) -> bool {
    let s = code.as_bytes();
    let next = skip_ws(s, at + len);
    let before = code[..at].trim_end();
    !before.ends_with('.')
        && !before.ends_with("::")
        && !before.ends_with('|')
        && !matches!(
            ident_before(code, at),
            "fn" | "let"
                | "mut"
                | "struct"
                | "enum"
                | "const"
                | "static"
                | "type"
                | "use"
                | "as"
                | "ref"
        )
        && matches!(s.get(next), Some(b')' | b',' | b';'))
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

/// Whether the definition `def` (in `file`'s `code`) is one of the scan's helpers.
fn is_helper_def(scan: &Scan, file: &str, code: &str, def: &Def) -> bool {
    def.kind == Kind::Fn
        && scan.builders.iter().any(|b| {
            b.helper_in(file)
                .is_some_and(|(name, _)| name == def.name && b.defines(code, def.at))
        })
}

/// The sites in a file's library code (from [`library_code`]) that `scan` counts, sorted
/// by line. Panics on a wrapper `fn` visible outside the file.
pub(in crate::error) fn sites_in(scan: &Scan, code: &str, file: &str) -> Vec<Site> {
    let defs = definitions(code);
    let mut queue = base_occurrences(scan, code, file);
    // per definition: not yet a wrapper, or a wrapper with calls, or one never called
    let mut wrapped: Vec<Option<bool>> = vec![None; defs.len()];
    let mut sites = Vec::new();
    let is_helper = |d: usize| is_helper_def(scan, file, code, &defs[d]);
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
                    let values = scan.wrappers == Wrappers::Thin;
                    let calls = wrapper_calls(code, &defs[d], param, &occ.via, values);
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

/// What differs between the files with a site (sites, by path) and the `pinned` counts
/// (path, count), as a report naming `what` is counted, or `None` when they agree.
fn pin_report(
    what: &str,
    found: &[(String, Vec<Site>)],
    pinned: &[(&str, usize)],
) -> Option<String> {
    use std::fmt::Write as _;
    let counts: Vec<(&str, usize)> = found.iter().map(|(f, s)| (f.as_str(), s.len())).collect();
    if counts == pinned {
        return None;
    }
    let mut report = String::new();
    let pinned_total: usize = pinned.iter().map(|(_, n)| n).sum();
    let found_total: usize = counts.iter().map(|(_, n)| n).sum();
    writeln!(report, "{what}: pinned {pinned_total}, found {found_total}").unwrap();
    if pinned_total == found_total {
        writeln!(
            report,
            "the total is unchanged: if code moved between files, update both pins"
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
    Some(report)
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
    if let Some(mut report) = pin_report(what, found, pinned) {
        writeln!(report, "the pinned list, as found:").unwrap();
        for (file, sites) in found {
            writeln!(report, "    (\"{file}\", {}),", sites.len()).unwrap();
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

/// The pins in the folder `src/error/pins/<kind>/` of this crate, as (path, count) sorted
/// by path. A pin is one file, `<path>.pin`, at the library file's path relative to
/// `crates/` (`pins/phrase/solidcore/src/blend/hcr.rs.pin` pins
/// `solidcore/src/blend/hcr.rs`), holding its count as a decimal number. One file per pin,
/// so changes that lower different files' counts touch different files and merge cleanly.
/// Panics on any other file in the folder, or a pin that is not a number above 0.
pub(in crate::error) fn pins(kind: &str) -> Vec<(String, usize)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/error/pins")
        .join(kind);
    let mut pins = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("read pins dir") {
            let path = entry.expect("pins dir entry").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .expect("under the pins dir")
                .to_string_lossy()
                .replace('\\', "/");
            let Some(file) = relative.strip_suffix(".pin") else {
                panic!("{}: a pin is named `<path>.pin`", path.display());
            };
            let text = std::fs::read_to_string(&path).expect("read pin");
            let n = match text.trim().parse::<usize>() {
                Ok(n) if n > 0 => n,
                _ => panic!(
                    "{}: holds {:?}; a pin holds its file's count, above 0 (a file whose \
                     count reaches 0 loses its pin)",
                    path.display(),
                    text.trim()
                ),
            };
            pins.push((file.to_owned(), n));
        }
    }
    pins.sort();
    pins
}

/// Panics, with a report of what changed and the pin files to write, unless the files with
/// a site (sites, by path) are exactly the pins in `src/error/pins/<kind>/` ([`pins`]).
/// `what` names the count in the report.
pub(in crate::error) fn assert_pins(what: &str, found: &[(String, Vec<Site>)], kind: &str) {
    use std::fmt::Write as _;
    let pins = pins(kind);
    let pinned: Vec<(&str, usize)> = pins.iter().map(|(f, n)| (f.as_str(), *n)).collect();
    if let Some(mut report) = pin_report(what, found, &pinned) {
        writeln!(
            report,
            "the pins to change, in solidcore/crates/solidcore/src/error/pins/{kind}/:"
        )
        .unwrap();
        for (file, pin) in &pinned {
            if !found.iter().any(|(f, _)| f == file) {
                writeln!(report, "    delete {file}.pin (was {pin})").unwrap();
            }
        }
        for (file, sites) in found {
            let n = sites.len();
            match pinned.iter().find(|(f, _)| f == file) {
                Some((_, pin)) if *pin == n => {}
                Some((_, pin)) => writeln!(report, "    {file}.pin: {pin} -> {n}").unwrap(),
                None => writeln!(report, "    create {file}.pin holding {n}").unwrap(),
            }
        }
        panic!("{report}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Scan = Scan {
        builders: &[
            Builder::Variant("CurveFailure", "StageFailed"),
            Builder::Method("curve_stage_failed", None),
        ],
        wrappers: Wrappers::Thin,
    };

    const GENERIC: Scan = Scan {
        builders: &[Builder::Variant("Reason", "Failed")],
        wrappers: Wrappers::Phrase,
    };

    fn lines(scan: &Scan, code: &str) -> Vec<(usize, String)> {
        sites_in(scan, &library_code(code), "fixture")
            .into_iter()
            .map(|s| (s.line, s.via))
            .collect()
    }

    /// A `matches!` pattern is no construction with any delimiter, up to its guard; a variant
    /// in the guard is a value.
    #[test]
    fn a_matches_pattern_is_excluded_with_any_delimiter_up_to_its_guard() {
        let fixture = "
fn a(e: &F) -> bool { matches! { e, CurveFailure::StageFailed } }
fn b(e: &F) -> bool { matches![e, CurveFailure::StageFailed] || matches!(e, CurveFailure::StageFailed) }
fn c(e: &F) -> bool { matches!(e, Some(_) if accepts(CurveFailure::StageFailed)) }
";
        assert_eq!(
            lines(&STAGE, fixture),
            vec![(4, "CurveFailure::StageFailed".to_owned())]
        );
    }

    /// A closure's calls stop where a `let` binds its name again, within that `let`'s block.
    #[test]
    fn a_closure_shadowed_by_a_new_binding_is_not_called_after_it() {
        let fixture = "
fn c() {
    let err = || CurveFailure::StageFailed;
    let _ = err();
    {
        let err = || ();
        err();
    }
    let _ = err();
    let err = || ();
    err();
}
";
        assert_eq!(
            lines(&STAGE, fixture),
            vec![
                (4, "err (wraps CurveFailure::StageFailed)".to_owned()),
                (9, "err (wraps CurveFailure::StageFailed)".to_owned()),
            ]
        );
    }

    /// A wrapper's name passed as a value is a site, as a call is; a field, a method or a
    /// path segment of the same name is not.
    #[test]
    fn a_wrapper_passed_by_name_is_a_site() {
        let fixture = "
fn stage() -> E { SolidError::curve(OP, CurveFailure::StageFailed) }
fn a(x: Option<u8>, y: Option<u8>) -> Result<u8> {
    let fail = || SolidError::curve(OP, CurveFailure::StageFailed);
    x.ok_or_else(fail)?;
    y.ok_or_else(stage)?;
    let _ = (t.fail, t.fail(), m::fail, Thing { fail: 1 }, t.stage, m::stage);
    Err(stage())
}
";
        let lines = lines(&STAGE, fixture);
        let wraps = |name: &str| format!("{name} (wraps CurveFailure::StageFailed)");
        assert_eq!(
            lines,
            vec![(5, wraps("fail")), (6, wraps("stage")), (8, wraps("stage"))]
        );
    }

    /// A `let` of a wrapper `fn`'s name hides the `fn` for the rest of its block.
    #[test]
    fn a_let_of_the_same_name_hides_a_wrapper_fn() {
        let fixture = "
fn missing() -> E { SolidError::curve(OP, CurveFailure::StageFailed) }
fn a() -> Result<()> {
    {
        let missing = || SolidError::invalid_input(OP, \"x\");
        let _ = missing();
    }
    Err(missing())
}
";
        assert_eq!(
            lines(&STAGE, fixture),
            vec![(8, "missing (wraps CurveFailure::StageFailed)".to_owned())]
        );
    }

    /// A free `fn` named as a `SolidError` method is a local wrapper, counted at its calls;
    /// the method's own body is not a site.
    #[test]
    fn a_free_fn_named_as_a_helper_is_a_wrapper() {
        let fixture = "
fn curve_stage_failed(op: &'static str) -> E { SolidError::curve(op, CurveFailure::StageFailed) }
fn g() { let _ = (curve_stage_failed(OP), curve_stage_failed(OP)); }
impl crate::SolidError {
    pub(crate) fn curve_stage_failed(op: &'static str, s: i32) -> Self {
        Failure::new(op, CurveFailure::StageFailed).with_status(s).into_error()
    }
}
fn h() { let _ = SolidError::curve_stage_failed(OP, 1); }
";
        let wraps = "curve_stage_failed (wraps CurveFailure::StageFailed)".to_owned();
        assert_eq!(
            lines(&STAGE, fixture),
            vec![
                (3, wraps.clone()),
                (3, wraps),
                (9, "curve_stage_failed".to_owned()),
            ]
        );
    }

    /// A counted enum imported under another name would hide its constructions: the scan
    /// refuses it.
    #[test]
    #[should_panic(expected = "under another name")]
    fn an_enum_imported_under_another_name_fails_the_scan() {
        let _ = lines(
            &GENERIC,
            "use crate::error::Reason as R;\nfn x() -> E { Failure::new(OP, R::Failed(\"x\")).into_error() }\n",
        );
    }

    /// A glob import of a counted enum's variants fails the scan too.
    #[test]
    #[should_panic(expected = "with a glob")]
    fn an_enum_glob_import_fails_the_scan() {
        let _ = lines(&STAGE, "use crate::error::curve::CurveFailure::*;\n");
    }

    /// An import of a counted variant is no construction, and the short name it brings in
    /// would not be seen: the scan refuses it.
    #[test]
    #[should_panic(expected = "a `use` imports `CurveFailure::StageFailed`")]
    fn a_variant_import_fails_the_scan() {
        let _ = lines(
            &STAGE,
            "use crate::error::curve::CurveFailure::StageFailed;\nfn x(e: F) -> bool { e == StageFailed }\n",
        );
    }

    /// An enum imported under its own name is neither a site nor refused.
    #[test]
    fn a_plain_import_is_neither_a_site_nor_refused() {
        let fixture = "
use crate::error::{curve::CurveFailure, Reason};
fn x() -> E { Failure::new(OP, Reason::Failed(\"x\")).into_error() }
";
        assert_eq!(lines(&STAGE, fixture), Vec::new());
        assert_eq!(
            lines(&GENERIC, fixture),
            vec![(3, "Reason::Failed".to_owned())]
        );
    }
}
