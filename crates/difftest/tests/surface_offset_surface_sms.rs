//! Non-C++ tests of the production offset-surface SMS decoder.

pub use difftest::HarnessError;

#[path = "../src/units/fixtures/surface_offset_surface_sms.rs"]
mod sms;

use sms::{knot_count, parse_sms, ParsedSrf};
use solidcore::nurbs::math::NOW;
use std::fmt::Write;

const PATH: &str = "offset-reader.sms";

fn rejected(text: &str, expected: &str) {
    let result = parse_sms(text, PATH);
    assert!(result.is_err(), "accepted {text}");
    if let Err(error) = result {
        let error = error.to_string();
        assert!(error.starts_with(&format!("{PATH}: ")), "{error}");
        assert!(error.contains(expected), "{error}");
    }
}

fn asymmetric(flag: &str, rational: bool) -> String {
    let mut text = format!("1 2 1 2 {flag}");
    for (i, weight) in ["-2", "0", "-0", "0.25", "1e100", "-1.7976931348623157e308"]
        .iter()
        .enumerate()
    {
        assert!(write!(text, " {i} {} -0", i + 10).is_ok());
        if rational {
            assert!(write!(text, " {weight}").is_ok());
        }
    }
    text.push_str(" -0 0 1 1 -2 -2 0 1 3 3");
    text
}

fn bits(actual: f64, expected: f64) {
    assert_eq!(actual.to_bits(), expected.to_bits());
}

#[test]
fn asymmetric_polynomial_keeps_v_fastest_order_and_sentinel() {
    let parsed = parse_sms(&asymmetric("0", false), PATH).unwrap();
    assert_eq!((parsed.p, parsed.q), (1, 2));
    assert_eq!(parsed.net.len(), 2);
    for (i, point) in parsed.net.iter().flatten().enumerate() {
        bits(point[0], i as f64);
        bits(point[1], (i + 10) as f64);
        bits(point[2], -0.0);
        bits(point[3], NOW);
    }
    assert!(parsed.net.iter().all(|row| row.len() == 3));
    assert_eq!(parsed.u, [-0.0, 0.0, 1.0, 1.0]);
    bits(parsed.u[0], -0.0);
    assert_eq!(parsed.v, [-2.0, -2.0, 0.0, 1.0, 3.0, 3.0]);
}

#[test]
fn asymmetric_rational_is_already_weighted_and_never_rescaled() {
    let parsed = parse_sms(&asymmetric("1", true), PATH).unwrap();
    assert_eq!((parsed.p, parsed.q), (1, 2));
    assert_eq!(parsed.net.len(), 2);
    let weights = [-2.0, 0.0, -0.0, 0.25, 1e100, NOW];
    for (i, point) in parsed.net.iter().flatten().enumerate() {
        bits(point[0], i as f64);
        bits(point[1], (i + 10) as f64);
        bits(point[2], -0.0);
        bits(point[3], weights[i]);
    }
    assert_eq!(parsed.u.len(), 4);
    assert_eq!(parsed.v.len(), 6);
}

#[test]
fn flags_retain_numeric_nonzero_semantics() {
    for flag in ["0", "-0", "0.0", "-0e10"] {
        let parsed = parse_sms(&asymmetric(flag, false), PATH).unwrap();
        bits(parsed.net[0][0][3], NOW);
    }
    for flag in ["1", "-1", "0.5", "-0.25", "NaN", "inf", "-inf"] {
        let parsed = parse_sms(&asymmetric(flag, true), PATH).unwrap();
        bits(parsed.net[0][0][3], -2.0);
        assert_eq!(parsed.v.len(), 6);
    }
}

#[test]
fn nonfinite_payload_is_not_geometry_validated() {
    let parsed = parse_sms("0 0 0 0 NaN NaN inf -inf NaN inf NaN -inf -0", PATH).unwrap();
    assert!(parsed.net[0][0][0].is_nan());
    bits(parsed.net[0][0][1], f64::INFINITY);
    bits(parsed.net[0][0][2], f64::NEG_INFINITY);
    assert!(parsed.net[0][0][3].is_nan());
    bits(parsed.u[0], f64::INFINITY);
    assert!(parsed.u[1].is_nan());
    bits(parsed.v[0], f64::NEG_INFINITY);
    bits(parsed.v[1], -0.0);
}

#[test]
fn numeric_tails_are_ignored_but_lexical_tails_are_not() {
    let text = asymmetric("0", false);
    let parsed = parse_sms(&format!("{text} 42 NaN inf -0"), PATH).unwrap();
    assert_eq!(parsed.v, [-2.0, -2.0, 0.0, 1.0, 3.0, 3.0]);
    rejected(&format!("{text} invalid"), "invalid float");
}

#[test]
fn eager_bad_tail_precedes_all_semantic_failures() {
    for prefix in ["", "-1", "0 0 0", "0 0 0 0 0", "0 0 1e100 0 0"] {
        rejected(&format!("{prefix} invalid"), "invalid float");
    }
    let (gap, large) = overflow_values();
    for text in [
        format!("{gap} 0 {large:.0} 0 0 invalid"),
        format!("0 {gap} 0 {large:.0} 0 invalid"),
    ] {
        rejected(&text, "invalid float");
    }
}

#[test]
fn counts_have_exclusive_usize_bounds_and_ordered_validation() {
    let upper = format!("{:.0}", 2.0_f64.powi(usize::BITS as i32));
    for value in ["-1", "0.5", "NaN", "inf", "-inf", "1e100", &upper] {
        for position in 0..4 {
            let mut header = ["0"; 5];
            header[position] = value;
            rejected(&header.join(" "), "bad count");
        }
    }
    rejected("-1 -2 -3 -4", "bad count -1");
    rejected("0 -2 -3 -4", "bad count -2");
    rejected("0 0 -3 -4", "bad count -3");
    rejected("0 0 0 -4", "bad count -4");
    let parsed = parse_sms("-0 0.0 0e0 -0e2 0 1 2 3 0 1 0 1", PATH).unwrap();
    assert_eq!((parsed.p, parsed.q), (0, 0));
    bits(parsed.net[0][0][3], NOW);
}

#[test]
fn every_header_and_payload_prefix_is_path_bearing_truncation() {
    for rational in [false, true] {
        let text = asymmetric(if rational { "1" } else { "0" }, rational);
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            rejected(&tokens[..end].join(" "), "truncated");
        }
        assert!(parse_sms(&text, PATH).is_ok());
    }
}

#[test]
fn checked_knot_helper_covers_both_additions_without_large_storage() {
    assert_eq!(knot_count(2, 3, PATH, "u").unwrap(), 7);
    assert_eq!(
        knot_count(usize::MAX - 2, 0, PATH, "v").unwrap(),
        usize::MAX
    );
    for axis in ["u", "v"] {
        for (index, degree) in [(usize::MAX, 1), (usize::MAX - 1, 0)] {
            let error = knot_count(index, degree, PATH, axis)
                .unwrap_err()
                .to_string();
            assert_eq!(error, format!("{PATH}: {axis} knot count overflow"));
        }
    }
}

// Both values are exactly representable on 32/64-bit hosts. Only <=2049
// points are supplied to reach the real parser's arithmetic positions.
fn overflow_values() -> (usize, f64) {
    let gap = if usize::BITS > 53 { 2048 } else { 1 };
    (gap, 2.0_f64.powi(usize::BITS as i32) - gap as f64)
}

#[test]
fn net_then_u_then_v_error_precedence_and_reachable_overflows() {
    let (gap, large) = overflow_values();
    let points = " 0 0 0".repeat(gap + 1);
    let u_header = format!("{gap} 0 {large:.0} 0 0");
    rejected(&u_header, "truncated");
    rejected(&format!("{u_header}{points}"), "u knot count overflow");
    let v_header = format!("0 {gap} 0 {large:.0} 0");
    rejected(&v_header, "truncated");
    rejected(&format!("{v_header}{points}"), "truncated");
    rejected(&format!("{v_header}{points} 0"), "truncated");
    rejected(&format!("{v_header}{points} 0 1"), "v knot count overflow");
    rejected(&format!("{u_header}{points} invalid"), "invalid float");
    rejected(&format!("{v_header}{points} 0 1 invalid"), "invalid float");
}

#[test]
fn huge_representable_headers_require_real_data_before_growth() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 1);
    for text in [
        format!("{huge:.0} {huge:.0} 0 0 0"),
        format!("{huge:.0} 0 {huge:.0} 0 0"),
        format!("0 {huge:.0} 0 {huge:.0} 1"),
        format!("0 0 {huge:.0} 0 0 0 0 0"),
        format!("0 0 0 {huge:.0} 0 0 0 0 0 1"),
    ] {
        rejected(&text, "truncated");
    }
}

fn assert_asset_bits(parsed: &ParsedSrf, values: &[f64], n: usize, m: usize) {
    let mut payload = values.iter().skip(5).copied();
    assert_eq!(parsed.net.len(), n + 1);
    for row in &parsed.net {
        assert_eq!(row.len(), m + 1);
        for point in row {
            for coordinate in point.iter().take(3) {
                assert_eq!(Some(coordinate.to_bits()), payload.next().map(f64::to_bits));
            }
            assert_eq!(point.last().map(|w| w.to_bits()), Some(NOW.to_bits()));
        }
    }
    for knot in parsed.u.iter().chain(&parsed.v) {
        assert_eq!(Some(knot.to_bits()), payload.next().map(f64::to_bits));
    }
    assert!(payload.next().is_none());
}

#[test]
fn three_allowlisted_assets_keep_exact_token_and_bit_census() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles");
    for (file, n, m, p, q, tokens) in [
        ("offset_nurb.sms", 5, 1, 3, 1, 55),
        ("Surface.sms", 13, 1, 3, 1, 111),
        ("DropCurveSurface.sms", 1, 1, 1, 1, 25),
    ] {
        let path = root.join(file);
        if !path.exists() {
            eprintln!("SKIP optional upstream asset: {}", path.display());
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let values: Vec<f64> = text
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(values.len(), tokens);
        assert_eq!(&values[..5], &[n as f64, m as f64, p as f64, q as f64, 0.0]);
        let parsed = parse_sms(&text, &path.to_string_lossy()).unwrap();
        assert_eq!((parsed.p, parsed.q), (p, q));
        assert_eq!(parsed.u.len(), n + p + 2);
        assert_eq!(parsed.v.len(), m + q + 2);
        assert_asset_bits(&parsed, &values, n, m);
        eprintln!("CENSUS {file}: {tokens} tokens, 0 trailing, bit-identical payload");
    }
}
