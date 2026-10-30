//! TEXT-only value decoding; no MTEXT helper or registration substitute.
//! Caller supplies raw group1 and encoded/decoded byte caps. Output is not rescanned.

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

fn percent(chars: &mut impl Iterator<Item = char>) -> Result<char> {
    match chars.next() {
        Some('%') => Ok('%'),
        Some('d') => Ok('°'),
        Some('p') => Ok('±'),
        Some('c') => Ok('\u{2205}'),
        _ => Err(invalid("not supported yet: percent formatting code")),
    }
}

pub(super) fn decode_text_value(value: &str, max_encoded_bytes: usize, max_decoded_bytes: usize) -> Result<String> {
    if value.len() > max_encoded_bytes {
        return Err(invalid("encoded byte budget exceeded"));
    }
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        let decoded = match ch {
            c if c.is_control() => return Err(invalid("not supported yet: raw text control character")),
            '\\' if chars.peek() == Some(&'U') => {
                chars.next();
                cif(&mut chars)?
            }
            '%' if chars.peek() == Some(&'%') => {
                chars.next();
                percent(&mut chars)?
            }
            c => c,
        };
        push(&mut out, decoded, max_decoded_bytes)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::decode_text_value;

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
}
