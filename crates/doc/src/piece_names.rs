//! The grammar of piece names: a face split into pieces is named `<face>#[<faces around it>]`
//! (the faces only that piece touches), `#k` after that when nothing tells pieces apart, and
//! designs saved before format 14 numbered pieces `<face>#k`. Parsing never panics on hostile
//! names: an unbalanced or empty qualifier is simply not one.

/// The face a piece name is a piece of (one qualifier off), `None` when it is not a piece.
pub fn parent_piece(n: &str) -> Option<&str> {
    piece_split(n).map(|(stem, _)| stem)
}

/// A piece name's stem and the faces its qualifier lists (`F2:top`, [`F2:side:l4`, `F4:side:l4`]
/// for `F2:top#[F2:side:l4,F4:side:l4]`; none for an older design's numbered piece).
pub(crate) fn piece_parts(n: &str) -> Option<(&str, Vec<&str>)> {
    let (stem, around) = piece_split(n)?;
    Some((stem, around.map(split_top_level).unwrap_or_default()))
}

/// `n` as stem and qualifier contents: `stem#[…]`, `stem#[…]#k` (a numbered tie) or `stem#k` (an
/// older design's numbered piece, no contents).
fn piece_split(n: &str) -> Option<(&str, Option<&str>)> {
    let mut rest = n;
    let mut numbered = false;
    if let Some(i) = rest.rfind('#')
        && let Some(digits) = rest.get(i + 1..)
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        rest = rest.get(..i)?;
        numbered = true;
    }
    if let Some(inner_end) = rest.len().checked_sub(1).filter(|_| rest.ends_with(']')) {
        let open = matching_open(rest, inner_end)?;
        let stem = rest.get(..open.checked_sub(1)?)?;
        if rest.get(open - 1..open) == Some("#") && !stem.is_empty() {
            return Some((stem, rest.get(open + 1..inner_end)));
        }
    }
    (numbered && !rest.is_empty()).then_some((rest, None))
}

/// The index of the `[` matching the `]` at byte `close`.
fn matching_open(s: &str, close: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    for i in (0..=close).rev() {
        match bytes.get(i)? {
            b']' => depth += 1,
            b'[' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `s` split at the commas outside brackets (a qualifier's list of faces).
fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut from) = (0usize, 0usize);
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.extend(s.get(from..i).filter(|x| !x.is_empty()));
                from = i + 1;
            }
            _ => {}
        }
    }
    out.extend(s.get(from..).filter(|x| !x.is_empty()));
    out
}

/// An edge name without piece qualifiers: its own number (`~k`) and its faces' (`#[…]`, and the
/// numbered pieces of older designs), so pieces of a split edge, or of an edge between split
/// faces, match.
pub(crate) fn strip_edge_piece(n: &str) -> String {
    strip_qualifiers(strip_edge_number(n))
}

/// A name without any face's piece qualifiers (`#[…]`, and the numbered pieces of older designs),
/// wherever they are in it: the face a piece name is a piece of, with every face it names
/// unqualified too. An edge's own number (`~k`) stays.
pub(crate) fn strip_qualifiers(n: &str) -> String {
    let mut out = String::with_capacity(n.len());
    let bytes = n.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i) == Some(&b'#') {
            match bytes.get(i + 1) {
                Some(b'[') => {
                    // Skip the bracketed qualifier (and a tie number after it).
                    let mut depth = 0usize;
                    let mut j = i + 1;
                    while j < bytes.len() {
                        match bytes.get(j) {
                            Some(b'[') => depth += 1,
                            Some(b']') => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    i = j + 1;
                    continue;
                }
                Some(d) if d.is_ascii_digit() => {
                    i += 1;
                    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    continue;
                }
                _ => {}
            }
        }
        // Copy one whole character.
        let ch = n.get(i..).and_then(|rest| rest.chars().next());
        match ch {
            Some(c) => {
                out.push(c);
                i += c.len_utf8();
            }
            None => break,
        }
    }
    out
}

/// An edge name without its own number (`~k`).
pub(crate) fn strip_edge_number(n: &str) -> &str {
    match n.rfind('~') {
        Some(i) if n.get(i + 1..).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())) => n.get(..i).unwrap_or(n),
        _ => n,
    }
}
