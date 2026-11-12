//! Test-only scanner behind every typed-errors ratchet: it finds the bare statuses
//! (`SolidError::sm_err(`, `SolidError::null_pointer(`, `SolidError::Upstream {`,
//! `SolidError::nl_ser(`) that an area's library code still builds.
//!
//! A file's library code is what is left once comments, string, raw-string and char
//! literals are blanked and every `#[cfg(test)]` (or `#[cfg(all(test, ..))]`) item is
//! removed, wherever it sits: a test module in the middle of a file, a test-only `fn` or
//! `impl`, or an out-of-line `mod x;` whose file is then skipped. `#![cfg(test)]` makes
//! the whole file test code. Blanking keeps every byte offset and line break, so the
//! braces of what is left still balance.

use std::ops::Range;
use std::path::{Path, PathBuf};

/// The constructions of a bare status.
const NEEDLES: [&str; 4] = [
    "SolidError::sm_err(",
    "SolidError::null_pointer(",
    "SolidError::Upstream {",
    "SolidError::nl_ser(",
];

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The ranges of `text` that hold comments or the contents of string, raw-string,
/// byte-string and char literals (not lifetimes).
fn literal_ranges(text: &str) -> Vec<Range<usize>> {
    let t = text.as_bytes();
    let n = t.len();
    let at = |i: usize| t.get(i).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = at(i);
        let after_ident = i > 0 && is_ident(at(i - 1));
        if c == b'/' && at(i + 1) == b'/' {
            let end = text[i..].find('\n').map_or(n, |k| i + k);
            out.push(i..end);
            i = end;
        } else if c == b'/' && at(i + 1) == b'*' {
            let (mut depth, mut j) = (1, i + 2);
            while j < n && depth > 0 {
                if at(j) == b'/' && at(j + 1) == b'*' {
                    depth += 1;
                    j += 2;
                } else if at(j) == b'*' && at(j + 1) == b'/' {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            out.push(i..j);
            i = j;
        } else if (c == b'r' || c == b'b') && !after_ident && raw_string_open(t, i).is_some() {
            let (open, hashes) = raw_string_open(t, i).unwrap_or((i, 0));
            let close = format!("\"{}", "#".repeat(hashes));
            let end = text[open..]
                .find(&close)
                .map_or(n, |k| open + k + close.len());
            out.push(open..end.saturating_sub(close.len()).max(open));
            i = end;
        } else if c == b'"' || (c == b'b' && at(i + 1) == b'"' && !after_ident) {
            let mut j = i + if c == b'b' { 2 } else { 1 };
            let start = j;
            while j < n && at(j) != b'"' {
                j += if at(j) == b'\\' { 2 } else { 1 };
            }
            out.push(start..j.min(n));
            i = j + 1;
        } else if c == b'\'' {
            match char_literal_end(text, i) {
                Some(end) => {
                    out.push(i + 1..end - 1);
                    i = end;
                }
                None => i += 1,
            }
        } else {
            i += 1;
        }
    }
    out
}

/// For a raw string `r#"`/`br#"` starting at `i`: the offset just past its opening quote
/// and the number of `#`s.
fn raw_string_open(t: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if t.get(j) == Some(&b'b') {
        j += 1;
    }
    if t.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = t[j..].iter().take_while(|&&b| b == b'#').count();
    j += hashes;
    (t.get(j) == Some(&b'"')).then_some((j + 1, hashes))
}

/// The offset just past a char literal starting with the `'` at `i`, or `None` for a
/// lifetime or label.
fn char_literal_end(text: &str, i: usize) -> Option<usize> {
    let rest = text.get(i + 1..)?;
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    let body = match first {
        '\\' => {
            let (_, kind) = chars.next()?;
            match kind {
                'x' => 4,
                'u' => 2 + rest[2..].find('}')? + 1,
                _ => 1 + kind.len_utf8(),
            }
        }
        '\'' | '\n' => return None,
        other => other.len_utf8(),
    };
    (rest.as_bytes().get(body) == Some(&b'\'')).then_some(i + 1 + body + 1)
}

/// `text` with the bytes in `ranges` replaced by spaces (line breaks kept).
fn blank(text: &str, ranges: &[Range<usize>]) -> String {
    let mut bytes = text.as_bytes().to_vec();
    for range in ranges {
        for b in bytes.get_mut(range.clone()).into_iter().flatten() {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The offset just past the closer matching the opener at `i` of `s`.
fn match_close(s: &[u8], mut i: usize) -> usize {
    let mut depth = 0usize;
    while let Some(&ch) = s.get(i) {
        match ch {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
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

/// The offset just past any attributes starting at or after `i`.
fn skip_attributes(s: &[u8], mut i: usize) -> usize {
    loop {
        while s.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if s.get(i) == Some(&b'#') && s.get(i + 1) == Some(&b'[') {
            i = match_close(s, i + 1);
        } else {
            return i;
        }
    }
}

/// The end of the item that starts at `i`, just after its `#[cfg(test)]`: past its
/// matching `}`, or past a `;`/`,` at depth 0, or where its container closes.
fn item_end(s: &[u8], i: usize) -> usize {
    let mut i = skip_attributes(s, i);
    let mut depth = 0usize;
    while let Some(&ch) = s.get(i) {
        match ch {
            b'{' if depth == 0 => return match_close(s, i),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => return i,
            b')' | b']' | b'}' => depth -= 1,
            b';' | b',' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    s.len()
}

/// Whether the attribute `#[..]`/`#![..]` (blanked code, `#` at `i`) is a test cfg, and
/// where it ends; `inner` is set for `#!`.
fn test_cfg_at(s: &[u8], i: usize) -> Option<(usize, bool)> {
    let inner = s.get(i + 1) == Some(&b'!');
    let open = i + 1 + usize::from(inner);
    if s.get(open) != Some(&b'[') {
        return None;
    }
    let end = match_close(s, open);
    let condensed: String = s
        .get(open + 1..end.saturating_sub(1))?
        .iter()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|&b| char::from(b))
        .collect();
    let test = condensed == "cfg(test)"
        || condensed == "cfg(all(test))"
        || condensed.starts_with("cfg(all(test,");
    test.then_some((end, inner))
}

/// The name of an out-of-line module declared by `item` (`mod x;`, with any visibility).
fn out_of_line_mod(item: &str) -> Option<String> {
    let mut rest = item.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if rest.starts_with('(') {
            rest = &rest[rest.find(')')? + 1..];
        }
        rest = rest.trim_start();
    }
    let rest = rest.strip_prefix("mod")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let name_len = rest.bytes().take_while(|&b| is_ident(b)).count();
    (name_len > 0 && rest[name_len..].trim() == ";").then(|| rest[..name_len].to_owned())
}

/// A file split into its library part and its test part.
struct Split {
    /// The ranges taken by test-only items.
    tests: Vec<Range<usize>>,
    /// The out-of-line modules the file declares test-only.
    test_mods: Vec<String>,
}

fn split(code: &str) -> Split {
    let s = code.as_bytes();
    let mut tests = Vec::new();
    let mut test_mods = Vec::new();
    let mut pos = 0;
    while let Some(k) = code[pos..].find('#') {
        let at = pos + k;
        match test_cfg_at(s, at) {
            Some((_, true)) => {
                return Split {
                    tests: std::iter::once(0..s.len()).collect(),
                    test_mods,
                };
            }
            Some((attr_end, false)) => {
                let end = item_end(s, attr_end);
                let item_start = skip_attributes(s, attr_end);
                if let Some(name) = code.get(item_start..end).and_then(out_of_line_mod) {
                    test_mods.push(name);
                }
                tests.push(at..end);
                pos = end.max(at + 1);
            }
            None => pos = at + 1,
        }
    }
    Split { tests, test_mods }
}

/// The library code of a source file: comments and literal contents blanked, and every
/// test-only item removed. This is what the needles are counted in.
pub(in crate::error) fn library_code(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let code = blank(&text, &literal_ranges(&text));
    let tests = split(&code).tests;
    blank(&code, &tests)
}

/// The library part of a source file with its comments and strings kept: every
/// test-only item removed, for checks on message text.
pub(in crate::error) fn library_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let code = blank(&text, &literal_ranges(&text));
    blank(&text, &split(&code).tests)
}

/// The bare statuses library code (from [`library_code`]) builds.
pub(in crate::error) fn bare_sites(code: &str) -> usize {
    NEEDLES
        .iter()
        .map(|needle| code.matches(needle).count())
        .sum()
}

/// The library files of an area, `src/<area>.rs` and everything under `src/<area>/`,
/// as (path relative to `src` with `/` separators, text with LF line ends), sorted by
/// path. Test files (`tests.rs`, `*_tests.rs`, anything under a `tests/` directory) and
/// the files of modules declared `#[cfg(test)] mod x;` are skipped. At least `least`
/// `.rs` files must be found.
pub(in crate::error) fn library_files(area: &str, least: usize) -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths: Vec<PathBuf> = vec![root.join(format!("{area}.rs"))];
    let mut dirs = vec![root.join(area)];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                paths.push(path);
            }
        }
    }
    assert!(paths.len() >= least, "{paths:?}");
    let mut files: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(path)
                .expect("read")
                .replace("\r\n", "\n");
            (relative, text)
        })
        .collect();
    files.sort();
    // the directories holding modules declared test-only
    let mut test_only: Vec<String> = Vec::new();
    for (relative, text) in &files {
        let stem = relative.trim_end_matches(".rs");
        let dir = match stem.strip_suffix("/mod") {
            Some(parent) => parent.to_owned(),
            None => stem.to_owned(),
        };
        let code = blank(text, &literal_ranges(text));
        for name in split(&code).test_mods {
            test_only.push(format!("{dir}/{name}"));
        }
    }
    files.retain(|(relative, _)| {
        let stem = relative.trim_end_matches(".rs");
        !(relative.ends_with("/tests.rs")
            || relative.ends_with("_tests.rs")
            || relative.contains("/tests/")
            || test_only
                .iter()
                .any(|m| stem == m || stem.starts_with(&format!("{m}/"))))
    });
    files
}

/// The library files of an area that build bare statuses, with the number each builds,
/// sorted by path (see [`library_files`] for the files read).
pub(in crate::error) fn bare_statuses_in(area: &str, least: usize) -> Vec<(String, usize)> {
    library_files(area, least)
        .into_iter()
        .filter_map(|(relative, text)| {
            let bare = bare_sites(&library_code(&text));
            (bare > 0).then_some((relative, bare))
        })
        .collect()
}

/// Asserts that an area's library files build exactly the bare statuses listed
/// (path, count; sorted by path, every count above 0) and none elsewhere.
pub(in crate::error) fn assert_waiting(area: &str, least: usize, waiting: &[(&str, usize)]) {
    let expected: Vec<(String, usize)> = waiting
        .iter()
        .map(|(file, n)| ((*file).to_owned(), *n))
        .collect();
    assert_eq!(
        bare_statuses_in(area, least),
        expected,
        "bare statuses per {area} file"
    );
    for (file, n) in waiting {
        assert!(*n > 0, "{file} holds no bare status: remove its line");
    }
    assert!(
        waiting.windows(2).all(|w| w[0].0 < w[1].0),
        "the {area} waiting list is sorted by path, one file per line"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: &str = "SolidError::sm_err(OP)";

    /// Library code is counted however the test items interleave with it, and nothing
    /// in test items, comments or literals is.
    #[test]
    fn library_code_sees_past_every_test_item() {
        let fixture = format!(
            r##"
fn a() -> E {{ {B} }}

#[cfg(test)]
mod tests {{
    fn t() {{ {B}; let _ = '}}'; }}
}}

fn b() -> E {{ SolidError::null_pointer(OP) }}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod more_tests {{
    fn t() {{ {B} }}
}}

#[cfg(all(test, feature = "x"))]
fn helper() {{ {B} }}

#[cfg(test)]
impl Thing {{
    fn t() {{ {B} }}
}}

#[cfg(test)]
mod exact;

fn c() -> E {{
    // a comment: {B} and an unbalanced {{
    /* nested /* {B} */ still a comment }} */
    let s = "a string {B} with {{ and \" quote";
    let r = r#"raw {B} with "}}" inside"#;
    let o = '{{';
    let q = '\'';
    let l: &'static str = "";
    SolidError::Upstream {{ code: 1 }}
}}

#[cfg(test)]
mod last {{ fn t() {{ {B} }} }}

fn d() -> E {{ SolidError::nl_ser(OP) }}
"##
        );
        let code = library_code(&fixture);
        assert_eq!(bare_sites(&code), 4, "{code}");
        assert_eq!(bare_sites(&library_code(&fixture.replace('\n', "\r\n"))), 4);
        // braces still balance once literals and comments are blanked
        let opens = code.matches('{').count();
        assert_eq!(opens, code.matches('}').count(), "{code}");
        let code = fixture.replace("\r\n", "\n");
        let blanked = blank(&code, &literal_ranges(&code));
        assert_eq!(split(&blanked).test_mods, vec!["exact".to_owned()]);
        // the text form keeps comments and strings of library code only
        let text = library_text(&fixture);
        assert!(text.contains("a string SolidError::sm_err(OP)"));
        assert!(!text.contains("fn t()"));
    }

    /// An inner `#![cfg(test)]` makes the whole file test code.
    #[test]
    fn an_inner_test_cfg_drops_the_whole_file() {
        let fixture = format!("#![cfg(test)]\nfn a() -> E {{ {B} }}\n");
        assert_eq!(bare_sites(&library_code(&fixture)), 0);
    }

    /// Lifetimes and labels are not char literals; byte and escaped chars are.
    #[test]
    fn lifetimes_are_not_char_literals() {
        let fixture = format!(
            "fn a<'a>(x: &'a str) -> E {{ 'outer: loop {{ break 'outer; }} \
             let _ = (b'{{', '\\u{{7b}}', '\\x7b', 'é'); {B} }}\n"
        );
        let code = library_code(&fixture);
        assert_eq!(bare_sites(&code), 1, "{code}");
        assert_eq!(
            code.matches('{').count(),
            code.matches('}').count(),
            "{code}"
        );
    }
}
