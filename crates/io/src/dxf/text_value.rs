//! TEXT and MTEXT value decoding: what a person typed, from the value a file holds.
//! Caller supplies raw values and encoded/decoded byte caps. Output is never rescanned.
//! MTEXT's group 3 chunks and group 1 tail are joined before decoding once, because an
//! escape may straddle a chunk boundary. Written by @0xlillith (#5321).

use crate::{IoError, Result};

fn invalid(reason: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("DXF text value: {reason}"))
}

fn push(out: &mut String, ch: char, max_bytes: usize) -> Result<()> {
    let needed = out.len().checked_add(ch.len_utf8()).ok_or_else(|| invalid("decoded size overflow"))?;
    if needed > max_bytes {
        return Err(invalid("decoded byte budget exceeded"));
    }
    out.try_reserve(ch.len_utf8()).map_err(|error| invalid(format!("cannot reserve decoded text: {error}")))?;
    out.push(ch);
    Ok(())
}

fn cif(chars: &mut impl Iterator<Item = char>) -> Result<char> {
    if chars.next() != Some('+') {
        return Err(invalid("malformed CIF Unicode prefix"));
    }
    let mut code = 0_u32;
    for _ in 0..4 {
        let digit = chars.next().and_then(|ch| ch.to_digit(16)).ok_or_else(|| invalid("CIF Unicode escape requires four hexadecimal digits"))?;
        code = code * 16 + digit;
    }
    let ch = char::from_u32(code).ok_or_else(|| invalid("CIF Unicode escape is not a scalar value; surrogate pairs are unsupported"))?;
    if ch.is_control() {
        return Err(invalid("not supported yet: a control character encoded as CIF"));
    }
    Ok(ch)
}

fn percent(chars: &mut impl Iterator<Item = char>, mtext: bool) -> Result<char> {
    match chars.next() {
        Some('%') if !mtext => Ok('%'),
        Some('d') => Ok('°'),
        Some('p') => Ok('±'),
        Some('c') => Ok('\u{2205}'),
        _ => Err(invalid("not supported yet: percent formatting code")),
    }
}

fn decode(value: &str, mtext: bool, max_decoded_bytes: usize) -> Result<String> {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        let decoded = match ch {
            // A paragraph is admitted only through MTEXT's explicit escape below.
            c if c.is_control() => return Err(invalid("not supported yet: raw text control character")),
            '\\' => match chars.peek().copied() {
                Some('U') => {
                    chars.next();
                    cif(&mut chars)?
                }
                Some('\\' | '{' | '}') if mtext => chars.next().ok_or_else(|| invalid("incomplete MTEXT escape"))?,
                Some('P') if mtext => {
                    chars.next();
                    '\n'
                }
                _ if mtext => return Err(invalid("not supported yet: MTEXT formatting escape")),
                _ => '\\',
            },
            '%' if chars.peek() == Some(&'%') => {
                chars.next();
                percent(&mut chars, mtext)?
            }
            '{' | '}' if mtext => return Err(invalid("not supported yet: MTEXT formatting group")),
            c => c,
        };
        push(&mut out, decoded, max_decoded_bytes)?;
    }
    Ok(out)
}

/// TEXT's untrimmed group 1 value.
pub(super) fn decode_text_value(value: &str, max_encoded_bytes: usize, max_decoded_bytes: usize) -> Result<String> {
    if value.len() > max_encoded_bytes {
        return Err(invalid("encoded byte budget exceeded"));
    }
    decode(value, false, max_decoded_bytes)
}

/// MTEXT's group 3 chunks, then its one group 1 tail, in stored order, joined and decoded once.
pub(super) fn decode_mtext_values<'a>(
    values: impl IntoIterator<Item = (i32, &'a str)>,
    max_encoded_bytes: usize,
    max_decoded_bytes: usize,
    max_chunks: usize,
) -> Result<String> {
    let mut joined = String::new();
    let mut tail_seen = false;
    let mut count = 0_usize;
    for (code, value) in values {
        count = count.checked_add(1).ok_or_else(|| invalid("MTEXT chunk count overflow"))?;
        if count > max_chunks {
            return Err(invalid("MTEXT chunk budget exceeded"));
        }
        if tail_seen {
            return Err(invalid("MTEXT text field follows its group 1 tail"));
        }
        match code {
            3 => {}
            1 => tail_seen = true,
            _ => return Err(invalid("MTEXT value combiner accepts groups 1 and 3 only")),
        }
        let needed = joined.len().checked_add(value.len()).ok_or_else(|| invalid("MTEXT encoded size overflow"))?;
        if needed > max_encoded_bytes {
            return Err(invalid("MTEXT encoded byte budget exceeded"));
        }
        joined.try_reserve(value.len()).map_err(|error| invalid(format!("cannot reserve MTEXT text: {error}")))?;
        joined.push_str(value);
    }
    if !tail_seen {
        return Err(invalid("MTEXT is missing its group 1 tail"));
    }
    decode(&joined, true, max_decoded_bytes)
}

#[cfg(test)]
mod tests {
    use super::{decode_mtext_values, decode_text_value};

    #[test]
    fn literals_percent_cif_and_decode_once_retain_text_semantics() {
        assert_eq!(decode_text_value("%%% %%d %%p %%c \\U+FFFF", 4096, 4096).unwrap(), "% ° ± \u{2205} \u{ffff}");
        assert_eq!(decode_text_value("  \\U+0025\\U+0025d \\U+005CU+0041 \\P  ", 4096, 4096).unwrap(), "  %%d \\U+0041 \\P  ");
        assert_eq!(decode_text_value("trailing\\", 4096, 4096).unwrap(), "trailing\\");
    }

    #[test]
    fn malformed_styles_surrogates_and_controls_still_refuse() {
        for value in [
            "\\U+", "\\U+XYZ0", "\\U+123", "\\U0041", "\\U+D800", "\\U+DC00", "%%", "%%q", "%%u", "%%123", "a\rb", "a\nb", "a\tb", "\\U+0000",
            "\\U+000A", "\\U+0085",
        ] {
            assert!(decode_text_value(value, 4096, 4096).is_err(), "{value}");
        }
    }

    #[test]
    fn encoded_and_decoded_budgets_are_separate_utf8_byte_bounds() {
        assert_eq!(decode_text_value("\\U+FFFF", 7, 3).unwrap(), "\u{ffff}");
        assert!(decode_text_value("\\U+FFFF", 6, 3).is_err());
        assert!(decode_text_value("\\U+FFFF", 7, 2).is_err());
        assert_eq!(decode_text_value("", 0, 0).unwrap(), "");
        // The general TEXT value decoder still preserves newer Unicode; the
        // current writer-compatible entity adapter separately refuses non-BMP.
        assert_eq!(decode_text_value("中𠀋", 7, 7).unwrap(), "中𠀋");
    }

    #[test]
    fn mtext_literals_and_paragraphs_decode_once_without_style_interpretation() {
        let fields = [(1, "\\U+0025 \\U+0025\\U+0025d \\U+00B0 \\\\P \\{x\\}\\Pnext")];
        assert_eq!(decode_mtext_values(fields, 4096, 4096, 1).unwrap(), "% %%d ° \\P {x}\nnext");
        assert_eq!(decode_mtext_values([(1, "\\\\U+0041")], 4096, 4096, 1).unwrap(), "\\U+0041");
    }

    #[test]
    fn mtext_joins_an_escape_across_a_250_byte_boundary_before_decoding() {
        let head = format!("{}\\U", "a".repeat(248));
        assert_eq!(head.len(), 250);
        assert_eq!(decode_mtext_values([(3, head.as_str()), (1, "+4E2Dz")], 256, 252, 2).unwrap(), format!("{}中z", "a".repeat(248)));
        let head = format!("{}\\", "a".repeat(249));
        assert_eq!(decode_mtext_values([(3, head.as_str()), (1, "\\P")], 4096, 4096, 2).unwrap(), format!("{}\\P", "a".repeat(249)));
        assert_eq!(decode_mtext_values([(3, "\\U+002"), (1, "5\\U+0025d")], 4096, 4096, 2).unwrap(), "%%d");
    }

    #[test]
    fn mtext_keeps_an_empty_tail_and_the_spaces_between_chunks() {
        let full = "x".repeat(250);
        assert_eq!(decode_mtext_values([(3, full.as_str()), (1, "")], 250, 250, 2).unwrap(), full);
        assert_eq!(decode_mtext_values([(1, "")], 0, 0, 1).unwrap(), "");
        assert_eq!(decode_mtext_values([(3, "  one "), (1, " two  ")], 4096, 4096, 2).unwrap(), "  one  two  ");
    }

    #[test]
    fn mtext_formatting_and_malformed_escapes_are_refused_by_name() {
        for value in ["\\", "\\H2x;Hi", "{styled}", "%%%", "\\~", "\\Lline"] {
            assert!(decode_mtext_values([(1, value)], 4096, 4096, 1).is_err(), "{value}");
        }
        for value in ["a\rb", "a\nb", "a\tb", "\\U+0000", "\\U+000A", "\\U+000D", "\\U+0085"] {
            assert!(decode_mtext_values([(1, value)], 4096, 4096, 1).is_err(), "{value}");
        }
    }

    #[test]
    fn mtext_budgets_are_inclusive_and_chunks_must_end_with_one_tail() {
        assert_eq!(decode_mtext_values([(3, "\\U+"), (1, "FFFF")], 7, 3, 2).unwrap(), "\u{ffff}");
        assert!(decode_mtext_values([(3, "\\U+"), (1, "FFFF")], 6, 3, 2).is_err());
        assert!(decode_mtext_values([(3, "\\U+"), (1, "FFFF")], 7, 2, 2).is_err());
        assert!(decode_mtext_values([(3, "\\U+"), (1, "FFFF")], 7, 3, 1).is_err());
        assert!(decode_mtext_values([(1, "")], 0, 0, 0).is_err());
        assert!(decode_mtext_values([], 4096, 4096, 8).is_err());
        assert!(decode_mtext_values([(3, "text")], 4096, 4096, 8).is_err());
        assert!(decode_mtext_values([(1, "a"), (1, "b")], 4096, 4096, 8).is_err());
        assert!(decode_mtext_values([(1, "a"), (3, "b")], 4096, 4096, 8).is_err());
        assert!(decode_mtext_values([(8, "LAYER"), (1, "a")], 4096, 4096, 8).is_err());
    }
}
