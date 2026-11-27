//! Colours as text, once: `#rrggbb`, or `#rrggbbaa` with a straight (not premultiplied) alpha, as CSS Color 4 and 3MF
//! write them. Theme files, the colour field, 3MF files and appearance replies all read and write through here.
//!
//! The digits are checked before they are read: `u8::from_str_radix` alone would take a leading `+` (`"+f"` is 15).
//! Nothing is trimmed: a text field keeps a space as typed, and a file reader trims first if its format allows it.

/// The colour `text` names: `#` and exactly 6 or 8 hex digits, in either case. Nothing else reads: no spaces, no sign,
/// no CSS short form.
pub fn parse(text: &str) -> Option<([u8; 3], Option<u8>)> {
    parse_digits(text.strip_prefix('#')?)
}

/// [`parse`] without the `#`, for a reader that lets it be left out.
pub fn parse_digits(digits: &str) -> Option<([u8; 3], Option<u8>)> {
    if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| digits.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok());
    let alpha = if digits.len() == 8 { Some(byte(6)?) } else { None };
    Some(([byte(0)?, byte(2)?, byte(4)?], alpha))
}

/// `#rrggbb`, lower case.
pub fn format([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `#rrggbbaa`, lower case.
pub fn format_alpha([r, g, b]: [u8; 3], alpha: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}{alpha:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_writes_and_reads_back() {
        for v in 0..=255u8 {
            for rgb in [[v, 0, 0], [0, v, 0], [0, 0, v]] {
                assert_eq!(parse(&format(rgb)), Some((rgb, None)), "{rgb:?}");
            }
            assert_eq!(parse(&format_alpha([1, 2, 3], v)), Some(([1, 2, 3], Some(v))), "{v}");
        }
        assert_eq!(format([10, 27, 44]), "#0a1b2c");
        assert_eq!(format_alpha([255, 0, 16], 128), "#ff001080");
    }

    #[test]
    fn only_a_hash_and_six_or_eight_hex_digits_read() {
        assert_eq!(parse("#0a1B2c"), Some(([10, 27, 44], None)));
        assert_eq!(parse("#FFFFFF"), Some(([255, 255, 255], None)));
        assert_eq!(parse("#1b2C3DfF"), Some(([27, 44, 61], Some(255))));
        assert_eq!(parse("#00000000"), Some(([0, 0, 0], Some(0))));
        for text in [
            "",
            "#",
            "#12345",
            "#1234567",
            "#123456fff",
            "123456",
            "##123456",
            "#12345g",
            "#gggggg",
            "#+102fe",
            "#0102+e",
            "#+f0000",
            "#-10000",
            "#\u{e9}1234",
            "#123456 ",
            " #123456",
            "#123456\n",
            "# 123456",
            "#112233ffignored",
            "#fff",
            "#ffff",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn digits_without_the_hash() {
        assert_eq!(parse_digits("0a1b2c"), Some(([10, 27, 44], None)));
        assert_eq!(parse_digits("0a1b2c80"), Some(([10, 27, 44], Some(128))));
        for digits in ["#0a1b2c", "+a1b2c", "0a1b2", " 0a1b2c"] {
            assert_eq!(parse_digits(digits), None, "{digits:?}");
        }
    }
}
