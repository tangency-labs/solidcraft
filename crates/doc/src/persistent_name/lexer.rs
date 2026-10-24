//! The lexer: a name's text as tokens.

/// A plain decimal number: digits only, no leading zero (so it writes back the same).
pub(super) fn number<T: std::str::FromStr>(d: &str) -> Option<T> {
    let ok = !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) && (d.len() == 1 || !d.starts_with('0'));
    if ok { d.parse().ok() } else { None }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// `F<n>` before a `:`.
    Feature(u64),
    /// Text between separators.
    Word,
    Colon,
    Pipe,
    Comma,
    /// `#[`
    Open,
    /// `]`
    Close,
    /// `#<n>`
    Hash(u32),
    /// `~<n>`
    Tie(u32),
    /// `@F<n>.<k>`
    Copy(u64, u32),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Tok {
    pub(super) kind: Kind,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// The digits of `s` from byte `i` as a number, and where they end.
fn digits_at<T: std::str::FromStr>(s: &str, i: usize) -> Option<(T, usize)> {
    let rest = s.get(i..)?;
    let n = rest.bytes().take_while(u8::is_ascii_digit).count();
    Some((number(rest.get(..n)?)?, i + n))
}

/// The separator token starting at byte `i`, if one does.
fn separator_at(s: &str, i: usize) -> Option<Tok> {
    let b = s.as_bytes();
    let tok = |kind, end| Some(Tok { kind, start: i, end });
    match b.get(i)? {
        b':' => tok(Kind::Colon, i + 1),
        b'|' => tok(Kind::Pipe, i + 1),
        b',' => tok(Kind::Comma, i + 1),
        b']' => tok(Kind::Close, i + 1),
        b'#' if b.get(i + 1) == Some(&b'[') => tok(Kind::Open, i + 2),
        b'#' => digits_at(s, i + 1).and_then(|(k, end)| tok(Kind::Hash(k), end)),
        b'~' => digits_at(s, i + 1).and_then(|(k, end)| tok(Kind::Tie(k), end)),
        b'@' if b.get(i + 1) == Some(&b'F') => {
            let (feature, dot) = digits_at(s, i + 2)?;
            if b.get(dot) != Some(&b'.') {
                return None;
            }
            let (instance, end) = digits_at(s, dot + 1)?;
            tok(Kind::Copy(feature, instance), end)
        }
        _ => None,
    }
}

pub(super) fn lex(s: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if let Some(t) = separator_at(s, i) {
            toks.push(t);
            i = t.end;
            continue;
        }
        // `F<n>` right before a `:` names a feature.
        if s.as_bytes().get(i) == Some(&b'F')
            && let Some((n, end)) = digits_at::<u64>(s, i + 1)
            && s.as_bytes().get(end) == Some(&b':')
        {
            toks.push(Tok { kind: Kind::Feature(n), start: i, end });
            i = end;
            continue;
        }
        let start = i;
        while i < s.len() && separator_at(s, i).is_none() {
            i += s.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8);
        }
        toks.push(Tok { kind: Kind::Word, start, end: i });
    }
    toks
}
