use super::{FORMULA_STARTS, NEEDS_QUOTES, field, quoted, unguard};

#[test]
fn a_field_a_spreadsheet_would_run_gets_an_apostrophe() {
    for (text, written) in [
        ("=1+1", "'=1+1"),
        ("+2 for the fit", "'+2 for the fit"),
        ("-5 mm shim", "'-5 mm shim"),
        ("@SUM(1,1)", "\"'@SUM(1,1)\""),
        ("＝1+1", "'＝1+1"),
        ("＋2", "'＋2"),
        ("－5", "'－5"),
        ("＠x", "'＠x"),
        ("\tx", "\"'\tx\""),
        ("\rx", "\"'\rx\""),
        ("\nx", "\"'\nx\""),
        // Already an apostrophe, then a formula: one more, so the import gives back the original.
        ("'=x", "''=x"),
        ("''-x", "'''-x"),
        // Spaces before a formula count: a spreadsheet may take them off.
        (" =x", "' =x"),
        (" -5", "' -5"),
        ("' @x", "'' @x"),
    ] {
        assert_eq!(field(text), written, "{text:?}");
        assert_eq!(unguard(written.trim_matches('"')), text, "{written:?}");
    }
}

#[test]
fn a_whole_plain_number_stays_a_number() {
    for number in ["-5", "+5", "-0", "0", "1e-9", "-1.5E+3", ".5", "-.5", "5.", "+1e5", "-12.000000000"] {
        assert_eq!(field(number), number, "{number:?}");
    }
    // Not plain numbers: Rust's parse takes the words, spaces aren't a number's, and one too large isn't finite.
    for text in ["-inf", "+inf", "-infinity", "-NaN", "+nan", "-", "+", "--5", "-5 mm", "-5 ", "-1e400", "-0x10", "-5,5"] {
        assert!(field(text).trim_start_matches('"').starts_with('\''), "{text:?}: {}", field(text));
    }
}

#[test]
fn other_fields_are_left_as_they_are() {
    for text in ["", "Plate", "PL-100", "a=b", "x+1", "'quoted' word", "'", "''", "5 mm", " 5", "é=", "μ-5"] {
        assert_eq!(field(text), text, "{text:?}");
        assert_eq!(unguard(text), text, "{text:?}");
    }
}

#[test]
fn a_field_with_a_separator_a_quote_or_a_break_is_quoted() {
    assert_eq!(field("Plate, 3 mm"), "\"Plate, 3 mm\"");
    assert_eq!(field("one; two"), "\"one; two\"");
    assert_eq!(field("Pin \"short\""), "\"Pin \"\"short\"\"\"");
    assert_eq!(field("a\tb"), "\"a\tb\"");
    assert_eq!(field("line one\r\nline two"), "\"line one\r\nline two\"");
    assert_eq!(quoted("Plate"), "\"Plate\"");
    assert_eq!(quoted("=A1"), "\"'=A1\"");
    assert_eq!(quoted("-5"), "\"-5\"");
}

/// Random fields from an alphabet heavy in what matters: every formula start, apostrophes, quotes,
/// separators, spaces, digits and the letters of `inf` and `NaN`. Each one reads back exactly, never starts
/// a formula once written unless it is a whole number, and is quoted whenever it holds a separator.
#[test]
fn fuzz_every_field_reads_back_and_never_starts_a_formula() {
    let mut alphabet: Vec<char> = "'''\"\";,.eE0123456789 infaNxé".chars().collect();
    alphabet.extend(FORMULA_STARTS);
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..50_000 {
        let n = (rnd() % 12) as usize;
        let text: String = (0..n).filter_map(|_| alphabet.get((rnd() % alphabet.len() as u64) as usize)).collect();
        let written = field(&text);
        let inner = match written.strip_prefix('"').and_then(|w| w.strip_suffix('"')) {
            Some(inner) => inner.replace("\"\"", "\""),
            None => written.clone(),
        };
        assert_eq!(unguard(&inner), text, "{text:?} written as {written:?}");
        let bare = inner.trim_start_matches(' ');
        assert!(!bare.starts_with(FORMULA_STARTS) || text.parse::<f64>().is_ok_and(f64::is_finite), "{text:?} written as {written:?}");
        assert!(!text.contains(NEEDS_QUOTES) || written.starts_with('"'), "{text:?} written as {written:?}");
        assert_eq!(quoted(&text), format!("\"{}\"", inner.replace('"', "\"\"")), "{text:?}");
    }
}
