//! The spelling of a text in a DXF file, from what a person typed. One line of at most 256
//! characters is a TEXT value; anything longer, or with a line break, is MTEXT: group 3 chunks of
//! exactly 250 bytes, then a group 1 tail of fewer. Nothing is cut. Another control character
//! becomes a space; a character beyond U+FFFF is refused with its position (AC1015 writes `\U+XXXX`,
//! which has four digits). Written by @0xlillith (#5321).

use std::fmt::Write as _;

use crate::{IoError, Result};

const TEXT_CHAR_LIMIT: usize = 256;
const MTEXT_CHUNK_BYTES: usize = 250;

/// The value groups of one text entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EncodedText {
    /// TEXT's group 1 value.
    Text(String),
    /// MTEXT's group 3 chunks, then its group 1 tail.
    MText(Vec<(i32, String)>),
}

fn invalid(reason: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("DXF text: {reason}"))
}

fn is_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r')
}

/// Refuse a character beyond U+FFFF, naming it and its position; nothing is replaced. The
/// message is the writer's own (its callers name the entity), so it carries no prefix.
pub(super) fn writable(source: &str) -> Result<()> {
    match source.chars().enumerate().find(|(_, ch)| u32::from(*ch) > 0xffff) {
        Some((index, ch)) => Err(IoError::Invalid(format!(
            "character {ch:?} (U+{:04X}) at position {} cannot be written as DXF AC1015 text; only characters through U+FFFF are supported",
            u32::from(ch),
            index + 1
        ))),
        None => Ok(()),
    }
}

/// TEXT or MTEXT for `source`, encoded, within `max_encoded_bytes` (the value bytes only).
pub(super) fn encode_text(source: &str, max_encoded_bytes: usize) -> Result<EncodedText> {
    writable(source)?;
    if !source.contains(is_break) && source.chars().count() <= TEXT_CHAR_LIMIT {
        return Ok(EncodedText::Text(encode(source, false, max_encoded_bytes)?));
    }
    let encoded = encode(source, true, max_encoded_bytes)?;
    let mut groups = Vec::new();
    groups.try_reserve(encoded.len() / MTEXT_CHUNK_BYTES + 1).map_err(|error| invalid(format!("cannot reserve MTEXT chunks: {error}")))?;
    // The encoded value is ASCII, so every byte offset is a character boundary.
    let mut rest = encoded.as_str();
    while rest.len() >= MTEXT_CHUNK_BYTES {
        let (chunk, tail) = rest.split_at(MTEXT_CHUNK_BYTES);
        groups.push((3, chunk.to_string()));
        rest = tail;
    }
    groups.push((1, rest.to_string()));
    Ok(EncodedText::MText(groups))
}

/// The encoded bytes of one character, and how it is written.
fn spelling(ch: char, mtext: bool) -> (usize, Spelling) {
    match ch {
        '\n' | '\r' if mtext => (2, Spelling::Literal("\\P")),
        c if c.is_control() => (1, Spelling::Literal(" ")),
        '%' if mtext => (7, Spelling::Unicode),
        '\\' | '{' | '}' if mtext => (2, Spelling::Escaped),
        '%' => (3, Spelling::Literal("%%%")),
        '°' if !mtext => (3, Spelling::Literal("%%d")),
        '\\' => (7, Spelling::Unicode),
        c if !c.is_ascii() => (7, Spelling::Unicode),
        _ => (1, Spelling::Plain),
    }
}

enum Spelling {
    Plain,
    Literal(&'static str),
    Escaped,
    Unicode,
}

/// The characters of `source`, with `\r\n` taken as one line break in MTEXT.
fn characters(source: &str, mtext: bool) -> impl Iterator<Item = char> + '_ {
    let mut chars = source.chars().peekable();
    std::iter::from_fn(move || {
        let ch = chars.next()?;
        if mtext && ch == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
        }
        Some(ch)
    })
}

fn encode(source: &str, mtext: bool, max_encoded_bytes: usize) -> Result<String> {
    let mut needed = 0_usize;
    for ch in characters(source, mtext) {
        needed = needed.checked_add(spelling(ch, mtext).0).ok_or_else(|| invalid("encoded size overflow"))?;
    }
    if needed > max_encoded_bytes {
        return Err(invalid("encoded text exceeds the byte budget"));
    }
    let mut out = String::new();
    out.try_reserve(needed).map_err(|error| invalid(format!("cannot reserve encoded text: {error}")))?;
    for ch in characters(source, mtext) {
        match spelling(ch, mtext).1 {
            Spelling::Plain => out.push(ch),
            Spelling::Literal(text) => out.push_str(text),
            Spelling::Escaped => {
                out.push('\\');
                out.push(ch);
            }
            Spelling::Unicode => write!(out, "\\U+{:04X}", u32::from(ch)).map_err(|error| invalid(format!("encoding failed: {error}")))?,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{EncodedText, encode_text};

    fn text(source: &str) -> String {
        match encode_text(source, 1 << 20).unwrap() {
            EncodedText::Text(value) => value,
            other => panic!("{other:?}"),
        }
    }

    fn mtext(source: &str) -> Vec<(i32, String)> {
        match encode_text(source, 1 << 20).unwrap() {
            EncodedText::MText(groups) => groups,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn one_line_of_at_most_256_characters_is_text_and_longer_or_two_lines_is_mtext() {
        for n in [0, 1, 255, 256] {
            assert_eq!(text(&"x".repeat(n)), "x".repeat(n));
        }
        assert_eq!(text(&"中".repeat(256)), "\\U+4E2D".repeat(256));
        assert_eq!(mtext(&"x".repeat(257)), vec![(3, "x".repeat(250)), (1, "x".repeat(7))]);
        for source in ["one\ntwo", "one\r\ntwo", "one\rtwo"] {
            assert_eq!(mtext(source), vec![(1, "one\\Ptwo".to_string())], "{source:?}");
        }
    }

    #[test]
    fn typed_percent_degree_and_unicode_escapes_stay_what_was_typed() {
        assert_eq!(text("% %%d ° \\U+0041 {}"), "%%% %%%%%%d %%d \\U+005CU+0041 {}");
        assert_eq!(text("\u{ffff}"), "\\U+FFFF");
        assert_eq!(text("B1 UP 90° R1"), "B1 UP 90%%d R1");
        let groups = mtext("% %%d ° \\P {x}\nnext\r\nlast\rend");
        assert_eq!(groups, vec![(1, "\\U+0025 \\U+0025\\U+0025d \\U+00B0 \\\\P \\{x\\}\\Pnext\\Plast\\Pend".to_string())]);
    }

    #[test]
    fn mtext_chunks_are_250_bytes_with_one_shorter_tail() {
        for (n, expected) in [(257, vec![(3, 250), (1, 7)]), (500, vec![(3, 250), (3, 250), (1, 0)]), (501, vec![(3, 250), (3, 250), (1, 1)])] {
            let groups = mtext(&"x".repeat(n));
            assert_eq!(groups.iter().map(|(code, value)| (*code, value.len())).collect::<Vec<_>>(), expected);
        }
        for (n, expected) in [(249, vec![(1, 249)]), (250, vec![(3, 250), (1, 0)]), (251, vec![(3, 250), (1, 1)])] {
            let groups = mtext(&format!("{}\n", "x".repeat(n - 2)));
            assert_eq!(groups.iter().map(|(code, value)| (*code, value.len())).collect::<Vec<_>>(), expected, "{n} encoded bytes");
        }
    }

    #[test]
    fn an_escape_may_straddle_two_mtext_chunks() {
        let groups = mtext(&format!("{}中z\n", "a".repeat(248)));
        assert_eq!(groups, vec![(3, format!("{}\\U", "a".repeat(248))), (1, "+4E2Dz\\P".to_string())]);
    }

    #[test]
    fn a_control_character_becomes_a_space_in_both_entities() {
        for ch in ['\0', '\t', '\u{1f}', '\u{7f}', '\u{85}'] {
            assert_eq!(text(&format!("a{ch}b")), "a b");
            assert_eq!(mtext(&format!("a{ch}b\nc")), vec![(1, "a b\\Pc".to_string())]);
        }
    }

    #[test]
    fn a_character_beyond_u_ffff_is_refused_with_its_position_even_past_256() {
        for (source, position) in [("𠀋".to_string(), 1), (format!("{}𝑥", "x".repeat(300)), 301)] {
            let error = encode_text(&source, 0).unwrap_err().to_string();
            assert!(error.contains(&format!("position {position}")) && error.contains("U+"), "{error}");
        }
    }

    #[test]
    fn the_byte_budget_counts_the_encoded_value_and_is_inclusive() {
        assert_eq!(encode_text("°", 3).unwrap(), EncodedText::Text("%%d".into()));
        assert!(encode_text("°", 2).is_err());
        assert_eq!(encode_text("%\n", 9).unwrap(), EncodedText::MText(vec![(1, "\\U+0025\\P".into())]));
        assert!(encode_text("%\n", 8).is_err());
        assert_eq!(encode_text("", 0).unwrap(), EncodedText::Text(String::new()));
    }
}
