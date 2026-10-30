//! Original PAT inputs: no pattern library or output from another CAD product.
#![allow(clippy::unwrap_used, clippy::panic)]

use crate::{IoError, PatImportOptions, PatUnits, read_pat};
use solidcraft_geom::hatch::{self, Family, Mark, Pattern};
use solidcraft_geom::{Loop2, Region2, Vec2};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, SQRT_2};

const UNIT: &str = "*UNIT,Original unit probe\n90,1,-2,0.5,4,2,-1,0\n\n";
const CONTINUOUS: &str = "*LINES\n0,0,0,0,1\n\n";

fn read(text: &str) -> Vec<Pattern> {
    read_pat(text.as_bytes(), PatImportOptions::default()).unwrap()
}

fn family(text: &str, options: PatImportOptions) -> Family {
    let mut patterns = read_pat(text.as_bytes(), options).unwrap();
    assert_eq!(patterns.len(), 1);
    let mut pattern = patterns.remove(0);
    assert_eq!(pattern.families.len(), 1);
    pattern.families.remove(0)
}

fn invalid(bytes: &[u8], options: PatImportOptions) -> String {
    match read_pat(bytes, options) {
        Err(IoError::Invalid(message)) => message,
        other => panic!("expected a PAT input error, got {other:?}"),
    }
}

fn line_error(text: &str, line: usize) -> String {
    let message = invalid(text.as_bytes(), PatImportOptions::default());
    assert!(message.starts_with(&format!("PAT line {line}:")), "{message}");
    message
}

fn descriptor(fields: &[&str]) -> String {
    format!("*PROBE\n{}\n\n", fields.join(","))
}

#[test]
fn units_and_scale_convert_every_length_but_not_the_angle() {
    let defaults = PatImportOptions::default();
    assert!(matches!(defaults.units, PatUnits::Millimetres));
    assert_eq!(defaults.scale, 1.0);
    // Powers of two preserve the binary 25.4 conversion factor exactly. Expected
    // values are independent literals, not calls to the importer's conversion.
    for (units, scale, origin, delta, dashes) in [
        (PatUnits::Millimetres, 1.0, [1.0, -2.0], [0.5, 4.0], [2.0, -1.0, 0.0]),
        (PatUnits::Millimetres, 2.0, [2.0, -4.0], [1.0, 8.0], [4.0, -2.0, 0.0]),
        (PatUnits::Inches, 1.0, [25.4, -50.8], [12.7, 101.6], [50.8, -25.4, 0.0]),
        (PatUnits::Inches, 2.0, [50.8, -101.6], [25.4, 203.2], [101.6, -50.8, 0.0]),
    ] {
        let patterns = read_pat(UNIT.as_bytes(), PatImportOptions { units, scale }).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].name, "UNIT");
        assert_eq!(patterns[0].description, "Original unit probe");
        assert_eq!(patterns[0].families.len(), 1);
        let f = &patterns[0].families[0];
        assert_eq!(f.angle, FRAC_PI_2);
        assert_eq!([f.origin.x, f.origin.y], origin);
        assert_eq!([f.delta_x, f.delta_y], delta);
        assert_eq!(f.dashes, dashes);
    }
}

#[test]
fn local_deltas_signed_angles_and_scientific_notation_are_preserved() {
    let text = "*LOCAL\n -9e1 , 1e0 , -2e0 , 5e-1 , -4e0 , 2e0 , -1e0 , 0 \n\n";
    let f = family(text, PatImportOptions::default());
    assert_eq!(f.angle, -FRAC_PI_2);
    assert_eq!([f.origin.x, f.origin.y], [1.0, -2.0]);
    assert_eq!([f.delta_x, f.delta_y], [0.5, -4.0]);
    assert_eq!(f.dashes, [2.0, -1.0, 0.0]);
}

#[test]
fn continuous_dots_and_gap_only_families_keep_distinct_meanings() {
    assert!(family(CONTINUOUS, PatImportOptions::default()).dashes.is_empty());
    assert_eq!(read(CONTINUOUS)[0].description, "");
    for dashes in ["0,-1", "2,-1,0", "-2,-1"] {
        let text = format!("*DASHES\n0,0,0,0,1,{dashes}\n\n");
        let f = family(&text, PatImportOptions::default());
        let expected: &[f64] = match dashes {
            "0,-1" => &[0.0, -1.0],
            "2,-1,0" => &[2.0, -1.0, 0.0],
            _ => &[-2.0, -1.0],
        };
        assert_eq!(f.dashes, expected);
    }
    for dashes in ["0", "0,-0", "0,0,0,0,0,0"] {
        line_error(&format!("*ZERO\n0,0,0,0,1,{dashes}\n\n"), 2);
    }
    for spacing in ["0", "-0", "0.0", "-0e12"] {
        line_error(&format!("*ZERO\n0,0,0,0,{spacing}\n\n"), 2);
    }
}

#[test]
fn small_nonzero_spacing_and_dashes_are_not_snapped_to_zero() {
    let text = "*SMALL\n0,0,0,0,0.00000095367431640625,0.00000095367431640625,-1\n\n";
    let f = family(text, PatImportOptions::default());
    assert_eq!(f.delta_y, 0.00000095367431640625);
    assert_eq!(f.dashes, [0.00000095367431640625, -1.0]);
}

#[test]
fn comments_whitespace_crlf_and_family_order_are_supported() {
    let text = "; original test data\n\n*ORDER, First, second ; ignored\n\t0,0,0,0,1 ; family one\n\
                90,2,3,4,-5,2,-1\n \t\n; trailing comment\n";
    for input in [text.to_owned(), text.replace('\n', "\r\n")] {
        let patterns = read(&input);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].description, "First, second");
        assert_eq!(patterns[0].families.len(), 2);
        assert_eq!(patterns[0].families[0].angle, 0.0);
        assert_eq!(patterns[0].families[0].delta_y, 1.0);
        assert_eq!(patterns[0].families[1].angle, FRAC_PI_2);
        assert_eq!(patterns[0].families[1].delta_x, 4.0);
        assert_eq!(patterns[0].families[1].delta_y, -5.0);
    }
}

#[test]
fn duplicate_names_preserve_each_definition_case_and_order() {
    let text = "*Same,First\n0,0,0,0,1\n*Same,Second\n0,0,0,0,2\n*same,Third\n0,0,0,0,3\n\n";
    let patterns = read(text);
    assert_eq!(patterns.len(), 3);
    for (p, (name, description, spacing)) in patterns.iter().zip([("Same", "First", 1.0), ("Same", "Second", 2.0), ("same", "Third", 3.0)]) {
        assert_eq!(p.name, name);
        assert_eq!(p.description, description);
        assert_eq!(p.families.len(), 1);
        assert_eq!(p.families[0].delta_y, spacing);
    }
}

#[test]
fn names_follow_the_ascii_grammar_and_thirty_one_character_limit() {
    for name in ["A_1-$".to_owned(), "9x".to_owned(), "A".repeat(31)] {
        assert_eq!(read(&format!("*{name}\n0,0,0,0,1\n\n"))[0].name, name);
    }
    for name in ["", "has space", "bad/name", "bad.name", "_first", "-first", "$first", "é", "A\tB", "A\0B"] {
        line_error(&format!("*{name}\n0,0,0,0,1\n\n"), 1);
    }
    line_error(&format!("*{}\n0,0,0,0,1\n\n", "A".repeat(32)), 1);
}

#[test]
fn unicode_descriptions_comments_and_one_initial_bom_are_supported() {
    let text = "\u{feff}*UTF8,Rayado café, 漢字\n0,0,0,0,1 ; comentario válido\n\n";
    let patterns = read(text);
    assert_eq!(patterns[0].name, "UTF8");
    assert_eq!(patterns[0].description, "Rayado café, 漢字");
    assert_eq!(patterns[0].families[0].delta_y, 1.0);
    line_error(&format!("\u{feff}\u{feff}{CONTINUOUS}"), 1);
    line_error("*A\n0,0,0,0,1\n\u{feff}*B\n0,0,0,0,2\n\n", 3);
}

#[test]
fn physical_line_limit_counts_characters_before_removing_comments() {
    for character in ['x', 'é', '界'] {
        let description = character.to_string().repeat(77);
        let header = format!("*T,{description}");
        assert_eq!(header.chars().count(), 80);
        for ending in ["\n", "\r\n"] {
            let text = format!("{header}{ending}0,0,0,0,1{ending}{ending}");
            assert_eq!(read(&text)[0].description, description);
        }
        for count in [78, 79] {
            let header = format!("*T,{}", character.to_string().repeat(count));
            assert!(matches!(header.chars().count(), 81 | 82));
            line_error(&format!("{header}\n0,0,0,0,1\n\n"), 1);
        }
        let comment = format!(";{}", character.to_string().repeat(79));
        assert_eq!(comment.chars().count(), 80);
        assert_eq!(read(&format!("{comment}\n{CONTINUOUS}")).len(), 1);
        line_error(&format!("{comment}{character}\n{CONTINUOUS}"), 1);
    }
    let line = format!("0,0,0,0,1;{}", "x".repeat(70));
    assert_eq!(line.len(), 80);
    assert_eq!(read(&format!("*T\n{line}\n\n")).len(), 1);
    line_error(&format!("*T\n{line}x\n\n"), 2);
}

#[test]
fn invalid_utf8_and_bare_carriage_returns_report_physical_lines() {
    let bytes = b"; original\n\n*TEXT,broken \xff\n0,0,0,0,1\n\n";
    let message = invalid(bytes, PatImportOptions::default());
    assert!(message.starts_with("PAT line 3:"), "{message}");
    line_error("*A\n0,0,0,0,1\r\n\r\n*B\r0,0,0,0,2\n\n", 4);
    line_error("*A\n0,0,0,0,1\0\n\n", 2);
    line_error("*A,embedded\tcontrol\n0,0,0,0,1\n\n", 1);
}

#[test]
fn a_real_blank_line_after_the_final_family_is_required() {
    for text in ["*A\n0,0,0,0,1\n\n", "*A\n0,0,0,0,1\n \t\n", "*A\n0,0,0,0,1\n \t", "*A\n0,0,0,0,1\n\n; after blank"] {
        assert_eq!(read(text).len(), 1);
        assert_eq!(read(&text.replace('\n', "\r\n")).len(), 1);
    }
    for text in ["*A\n0,0,0,0,1", "*A\n0,0,0,0,1\n", "*A\r\n0,0,0,0,1\r\n"] {
        line_error(text, 3);
    }
    line_error("*A\n0,0,0,0,1\n; only a comment\n", 4);
    line_error("*A\n0,0,0,0,1\n\n90,0,0,0,2\n", 5);
    line_error("*A\n0,0,0,0,1\n\n*B\n0,0,0,0,2\n", 6);
    assert_eq!(read("*A\n0,0,0,0,1\n; comment\n\n").len(), 1);
}

#[test]
fn empty_input_and_patterns_without_families_are_errors() {
    for text in ["", "\n", " \t\r\n", "; comments only\n\n"] {
        let message = invalid(text.as_bytes(), PatImportOptions::default());
        assert!(message.starts_with("PAT "), "{message}");
    }
    line_error("0,0,0,0,1\n\n", 1);
    line_error("*EMPTY\n\n", 1);
    line_error("; comment\n*EMPTY\n*FULL\n0,0,0,0,1\n\n", 2);
}

#[test]
fn malformed_descriptor_errors_keep_physical_line_numbers_and_no_partial_result() {
    for line in ["0,0,0,1", "0,0,0,0,1,", "0,0,,0,1", "0,0,0,0,1x", "0,0,0,0,2mm", "0,0,0,0,1+2", "0,0,0,0,١"] {
        let text = format!("; original\n\n*BAD\n{line}\n\n");
        line_error(&text, 4);
        line_error(&text.replace('\n', "\r\n"), 4);
        line_error(&format!("*GOOD\n0,0,0,0,1\n*BAD\n{line}\n\n"), 4);
    }
}

#[test]
fn six_dashes_are_accepted_and_seven_are_refused() {
    let f = family("*SIX\n0,0,0,0,1,1,-1,0,2,-2,0\n\n", PatImportOptions::default());
    assert_eq!(f.dashes, [1.0, -1.0, 0.0, 2.0, -2.0, 0.0]);
    line_error("*SEVEN\n0,0,0,0,1,1,-1,0,2,-2,0,1\n\n", 2);
}

#[test]
fn every_numeric_field_rejects_nonfinite_and_overflowed_input() {
    for index in 0..8 {
        for bad in ["NaN", "inf", "-inf", "1e309", "-1e309"] {
            let mut fields = ["90", "1", "-2", "0.5", "4", "2", "-1", "0"];
            fields[index] = bad;
            line_error(&descriptor(&fields), 2);
        }
    }
}

#[test]
fn every_length_is_bounded_after_scale_and_unit_conversion() {
    for index in 1..8 {
        for sign in [1.0, -1.0] {
            let limit = (sign * 50000.0).to_string();
            let beyond = (sign * 50001.0).to_string();
            let mut fields = ["0", "1", "-2", "0.5", "4", "2", "-1", "0"];
            fields[index] = &limit;
            let f = family(&descriptor(&fields), PatImportOptions { units: PatUnits::Millimetres, scale: 2.0 });
            let lengths = [f.origin.x, f.origin.y, f.delta_x, f.delta_y, f.dashes[0], f.dashes[1], f.dashes[2]];
            assert_eq!(lengths[index - 1], sign * 100000.0);
            fields[index] = &beyond;
            let message = invalid(descriptor(&fields).as_bytes(), PatImportOptions { units: PatUnits::Millimetres, scale: 2.0 });
            assert!(message.starts_with("PAT line 2:"), "{message}");
        }
        let mut fields = ["0", "1", "-2", "0.5", "4", "2", "-1", "0"];
        fields[index] = "2048";
        let f = family(&descriptor(&fields), PatImportOptions { units: PatUnits::Inches, scale: 1.0 });
        let lengths = [f.origin.x, f.origin.y, f.delta_x, f.delta_y, f.dashes[0], f.dashes[1], f.dashes[2]];
        assert_eq!(lengths[index - 1], 52019.2);
        fields[index] = "4096";
        let message = invalid(descriptor(&fields).as_bytes(), PatImportOptions { units: PatUnits::Inches, scale: 1.0 });
        assert!(message.starts_with("PAT line 2:"), "{message}");
    }
}

#[test]
fn the_angle_bound_is_in_radians_and_is_independent_of_length_units() {
    for units in [PatUnits::Millimetres, PatUnits::Inches] {
        for angle in ["57000000", "-57000000"] {
            let f = family(&format!("*ANGLE\n{angle},0,0,0,1\n\n"), PatImportOptions { units, scale: 2.0 });
            assert!(f.angle.abs() > 900000.0 && f.angle.abs() < 1000000.0);
            assert_eq!(f.angle.is_sign_negative(), angle.starts_with('-'));
        }
        for angle in ["58000000", "-58000000"] {
            let message = invalid(format!("*ANGLE\n{angle},0,0,0,1\n\n").as_bytes(), PatImportOptions { units, scale: 2.0 });
            assert!(message.starts_with("PAT line 2:"), "{message}");
        }
    }
}

#[test]
fn invalid_scales_and_overflowed_conversion_are_refused() {
    for scale in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let message = invalid(UNIT.as_bytes(), PatImportOptions { units: PatUnits::Millimetres, scale });
        assert!(message.starts_with("PAT import:"), "{message}");
    }
    let message = invalid(UNIT.as_bytes(), PatImportOptions { units: PatUnits::Inches, scale: f64::MAX });
    assert!(message.starts_with("PAT import:"), "{message}");
    let message = invalid(UNIT.as_bytes(), PatImportOptions { units: PatUnits::Millimetres, scale: f64::MAX });
    assert!(message.starts_with("PAT line 2:"), "{message}");
}

#[test]
fn conversion_must_not_turn_nonzero_input_into_zero() {
    for index in 1..7 {
        let mut fields = ["0", "1", "1", "1", "1", "1", "-1"];
        fields[index] = "0.5";
        let message = invalid(descriptor(&fields).as_bytes(), PatImportOptions { units: PatUnits::Millimetres, scale: f64::from_bits(1) });
        assert!(message.starts_with("PAT line 2:"), "{message}");
    }
    line_error("*ANGLE\n5e-324,0,0,0,1\n\n", 2);
}

#[test]
fn decimal_underflow_is_not_mistaken_for_an_authored_zero() {
    for index in 0..7 {
        for underflow in ["1e-999", "-1e-999"] {
            let mut fields = ["0", "1", "1", "1", "1", "1", "-1"];
            fields[index] = underflow;
            line_error(&descriptor(&fields), 2);
        }
    }
    let text = "*ZEROS\n0e999,0e999,-0e999,0e999,1,0e999,-1\n\n";
    let f = family(text, PatImportOptions::default());
    assert_eq!(f.angle, 0.0);
    assert_eq!([f.origin.x, f.origin.y, f.delta_x, f.delta_y], [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(f.dashes, [0.0, -1.0]);
}

fn padded_file(bytes: usize) -> Vec<u8> {
    let padding = bytes - CONTINUOUS.len();
    let mut text = format!(";{}\n", "p".repeat(79)).repeat(padding / 81);
    match padding % 81 {
        0 => {}
        1 => text.push('\n'),
        remainder => text.push_str(&format!(";{}\n", "p".repeat(remainder - 2))),
    }
    text.push_str(CONTINUOUS);
    assert_eq!(text.len(), bytes);
    assert!(text.lines().all(|line| line.len() <= 80));
    text.into_bytes()
}

#[test]
fn one_mebibyte_is_accepted_and_one_more_byte_is_refused_before_decoding() {
    let patterns = read_pat(&padded_file(1_048_576), PatImportOptions::default()).unwrap();
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].name, "LINES");
    for bytes in [padded_file(1_048_577), vec![0xff; 1_048_577]] {
        let message = invalid(&bytes, PatImportOptions::default());
        assert!(message.starts_with("PAT import:"), "{message}");
    }
    let mut with_bom = vec![0xef, 0xbb, 0xbf];
    with_bom.extend(padded_file(1_048_573));
    assert_eq!(with_bom.len(), 1_048_576);
    assert_eq!(read_pat(&with_bom, PatImportOptions::default()).unwrap().len(), 1);
    with_bom.push(b'\n');
    let message = invalid(&with_bom, PatImportOptions::default());
    assert!(message.starts_with("PAT import:"), "{message}");

    let mut unicode = format!(";{}\n", "é".repeat(39)).repeat(13_108);
    unicode.push_str(CONTINUOUS);
    assert!(unicode.len() > 1_048_576 && unicode.chars().count() < 1_048_576);
    assert!(unicode.lines().all(|line| line.chars().count() <= 80));
    let message = invalid(unicode.as_bytes(), PatImportOptions::default());
    assert!(message.starts_with("PAT import:"), "{message}");
}

fn patterns_file(count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        text.push_str(&format!("*P{i:04}\n0,0,0,0,1\n"));
    }
    text.push('\n');
    text
}

#[test]
fn one_thousand_patterns_is_inclusive_and_excess_is_not_truncated() {
    let patterns = read(&patterns_file(1000));
    assert_eq!(patterns.len(), 1000);
    assert_eq!(patterns[0].name, "P0000");
    assert_eq!(patterns[999].name, "P0999");
    line_error(&patterns_file(1001), 2001);
}

#[test]
fn sixty_four_families_is_a_per_pattern_limit() {
    let mut text = String::new();
    for name in ["FIRST", "SECOND"] {
        text.push_str(&format!("*{name}\n"));
        for spacing in 1..=64 {
            text.push_str(&format!("0,0,0,0,{spacing}\n"));
        }
    }
    text.push('\n');
    let patterns = read(&text);
    assert_eq!(patterns.len(), 2);
    for pattern in patterns {
        assert_eq!(pattern.families.len(), 64);
        for (family, expected) in pattern.families.iter().zip(1..=64) {
            assert_eq!(family.delta_y, f64::from(expected));
        }
    }
    line_error(&format!("*TOO_MANY\n{}\n", "0,0,0,0,1\n".repeat(65)), 66);
}

#[test]
fn bounded_byte_mutations_return_errors_or_complete_valid_patterns() {
    let input = UNIT.as_bytes();
    for end in 0..=input.len() {
        if let Ok(patterns) = read_pat(&input[..end], PatImportOptions::default()) {
            assert!(!patterns.is_empty());
            for pattern in patterns {
                pattern.validate().unwrap();
            }
        }
    }
    for index in 0..input.len() {
        for byte in [0, 0xff, b',', b';', b'\n'] {
            let mut changed = input.to_vec();
            changed[index] = byte;
            if let Ok(patterns) = read_pat(&changed, PatImportOptions::default()) {
                assert!(!patterns.is_empty());
                for pattern in patterns {
                    pattern.validate().unwrap();
                }
            }
        }
    }
}

const DIAGONAL_PAT: &str = "*R45,Original 45 degree lines\n45,0,0,0,5\n\n";
// Same analytic fixture and comparison tolerance as geom's hatch/clip_tests.rs;
// this is not a clipping, snapping or import-admission tolerance.
const DIAGONAL_TOL_MM: f64 = 1e-9;

fn paper_rectangle(width: f64, height: f64) -> Region2 {
    let outer = Loop2::polygon(&[Vec2::ZERO, Vec2::new(width, 0.0), Vec2::new(width, height), Vec2::new(0.0, height)]);
    Region2 { outer, holes: vec![] }
}

fn imported_diagonal_output(region: &Region2, options: PatImportOptions, spacing: f64) -> Vec<Mark> {
    let direct = Pattern {
        name: "R45".into(),
        description: "Original 45 degree lines".into(),
        families: vec![Family { angle: FRAC_PI_4, origin: Vec2::ZERO, delta_x: 0.0, delta_y: spacing, dashes: vec![] }],
    };
    let imported = read_pat(DIAGONAL_PAT.as_bytes(), options).unwrap();
    assert_eq!(imported, vec![direct.clone()]);
    let patterns_before = imported.clone();
    let region_before = region.clone();
    let marks = hatch::generate(&imported[0], region).unwrap();
    assert_eq!(marks, hatch::generate(&direct, region).unwrap());
    assert_eq!(imported, patterns_before);
    assert_eq!(region, &region_before);
    marks
}

fn diagonal_oracle(with_hole: bool) -> Vec<[f64; 4]> {
    let r = SQRT_2;
    // Original 40x20 mm oracle: y-x=5*sqrt(2)*k, k=-5..2.
    // The hole [10,20] x [5,15] splits only members -2, -1 and 0.
    if with_hole {
        vec![
            [25.0 * r, 0.0, 40.0, 40.0 - 25.0 * r],
            [20.0 * r, 0.0, 40.0, 40.0 - 20.0 * r],
            [15.0 * r, 0.0, 40.0, 40.0 - 15.0 * r],
            [10.0 * r, 0.0, 5.0 + 10.0 * r, 5.0],
            [20.0, 20.0 - 10.0 * r, 20.0 + 10.0 * r, 20.0],
            [5.0 * r, 0.0, 5.0 + 5.0 * r, 5.0],
            [20.0, 20.0 - 5.0 * r, 20.0 + 5.0 * r, 20.0],
            [0.0, 0.0, 10.0, 10.0],
            [15.0, 15.0, 20.0, 20.0],
            [0.0, 5.0 * r, 20.0 - 5.0 * r, 20.0],
            [0.0, 10.0 * r, 20.0 - 10.0 * r, 20.0],
        ]
    } else {
        vec![
            [25.0 * r, 0.0, 40.0, 40.0 - 25.0 * r],
            [20.0 * r, 0.0, 40.0, 40.0 - 20.0 * r],
            [15.0 * r, 0.0, 40.0, 40.0 - 15.0 * r],
            [10.0 * r, 0.0, 20.0 + 10.0 * r, 20.0],
            [5.0 * r, 0.0, 20.0 + 5.0 * r, 20.0],
            [0.0, 0.0, 20.0, 20.0],
            [0.0, 5.0 * r, 20.0 - 5.0 * r, 20.0],
            [0.0, 10.0 * r, 20.0 - 10.0 * r, 20.0],
        ]
    }
}

fn assert_diagonal_marks(marks: &[Mark], expected: &[[f64; 4]], total_length: f64) {
    assert_eq!(marks.len(), expected.len());
    let mut total = 0.0;
    for mark in marks {
        let Mark::Line { a, b } = *mark else { panic!("continuous PAT produced a dot") };
        assert!(a.is_finite() && b.is_finite() && a != b);
        total += a.dist(b);
    }
    // Consume the full multiset; order and endpoint direction are immaterial.
    let mut unmatched = marks.to_vec();
    for &[ax, ay, bx, by] in expected {
        let c = Vec2::new(ax, ay);
        let d = Vec2::new(bx, by);
        let found = unmatched.iter().position(|mark| match *mark {
            Mark::Line { a, b } => {
                (a.dist(c) <= DIAGONAL_TOL_MM && b.dist(d) <= DIAGONAL_TOL_MM) || (a.dist(d) <= DIAGONAL_TOL_MM && b.dist(c) <= DIAGONAL_TOL_MM)
            }
            Mark::Dot { .. } => false,
        });
        let Some(index) = found else { panic!("missing segment {c:?} -> {d:?}; remaining {unmatched:?}") };
        unmatched.swap_remove(index);
    }
    assert!(unmatched.is_empty());
    assert!((total - total_length).abs() <= DIAGONAL_TOL_MM, "actual length {total}, expected {total_length}");
}

#[test]
fn imported_diagonal_generates_all_eight_analytic_rectangle_segments() {
    let region = paper_rectangle(40.0, 20.0);
    let marks = imported_diagonal_output(&region, PatImportOptions::default(), 5.0);
    assert_diagonal_marks(&marks, &diagonal_oracle(false), 220.0 * SQRT_2 - 150.0);
}

#[test]
fn imported_diagonal_keeps_all_eleven_segments_and_leaves_the_hole_unchanged() {
    let mut region = paper_rectangle(40.0, 20.0);
    region.holes.push(Loop2::polygon(&[Vec2::new(10.0, 5.0), Vec2::new(10.0, 15.0), Vec2::new(20.0, 15.0), Vec2::new(20.0, 5.0)]));
    let marks = imported_diagonal_output(&region, PatImportOptions::default(), 5.0);
    assert_diagonal_marks(&marks, &diagonal_oracle(true), 185.0 * SQRT_2 - 120.0);
}

#[test]
fn imported_units_and_scale_reach_generation_once_in_paper_millimetres() {
    // Similarity of the same original rectangle oracle, not expected geometry
    // calculated from the imported Pattern or generated marks. Fixed paper
    // regions expose missing/repeated conversion. There is no view-scale API.
    for (options, width, height, spacing, factor) in [
        (PatImportOptions { units: PatUnits::Millimetres, scale: 2.0 }, 80.0, 40.0, 10.0, 2.0),
        (PatImportOptions { units: PatUnits::Inches, scale: 0.5 }, 508.0, 254.0, 63.5, 12.7),
    ] {
        let region = paper_rectangle(width, height);
        let marks = imported_diagonal_output(&region, options, spacing);
        let expected: Vec<_> = diagonal_oracle(false).into_iter().map(|line| line.map(|coordinate| coordinate * factor)).collect();
        assert_diagonal_marks(&marks, &expected, (220.0 * SQRT_2 - 150.0) * factor);
    }
}

#[test]
fn imported_decimal_spacing_reaches_generation_after_one_unit_conversion() {
    // Original PAT records. Independent binary64 ratios after conversion:
    // 0.1 = 3602879701896397 / 2^55; 1.5875 = 3574732204225331 / 2^51.
    for (text, options, spacing, numerator, denominator, half_height, last) in [
        (
            "*SC_DECIMAL,Original decimal spacing\n0,0,0,0,0.2\n\n",
            PatImportOptions { units: PatUnits::Millimetres, scale: 0.5 },
            0.1,
            3_602_879_701_896_397i128,
            36_028_797_018_963_968.0,
            0.95,
            9i64,
        ),
        (
            "*SC_DECIMAL,Original decimal spacing\n0,0,0,0,0.125\n\n",
            PatImportOptions { units: PatUnits::Inches, scale: 0.5 },
            1.5875,
            3_574_732_204_225_331i128,
            2_251_799_813_685_248.0,
            5.0,
            3i64,
        ),
    ] {
        let imported = read_pat(text.as_bytes(), options).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].families[0].delta_y, spacing);
        let before = imported.clone();
        let region = Region2 {
            outer: Loop2::polygon(&[
                Vec2::new(-1.0, -half_height),
                Vec2::new(1.0, -half_height),
                Vec2::new(1.0, half_height),
                Vec2::new(-1.0, half_height),
            ]),
            holes: vec![],
        };
        let saved_region = region.clone();
        let marks = hatch::generate(&imported[0], &region).unwrap();
        let expected: Vec<_> = (-last..=last)
            .map(|k| {
                let y = (i128::from(k) * numerator) as f64 / denominator;
                Mark::Line { a: Vec2::new(-1.0, y), b: Vec2::new(1.0, y) }
            })
            .collect();
        assert_eq!(marks, expected);
        assert_eq!(marks.len(), if last == 9 { 19 } else { 7 });
        assert_eq!(imported, before);
        assert_eq!(region, saved_region);
    }
}
