//! Exercise the actual SMS decoder without linking the C++ reference.

use difftest::HarnessError;
use solidcore::nurbs::math::NOW;

#[path = "../src/units/fixtures/nlib_srf_tool_p2_sms.rs"]
mod sms;

const PATH: &str = "fixtures/surface.sms";
const POLY: &str =
    "1 2 1 0 0  -0 1 2  3 4 5  6 7 8  9 10 11  12 13 14  15 16 17  0 0 1 1  -0 0.5 1 1";
const RATIONAL: &str = "0 1 0 1 1  2 4 6 2  -3 -6 -9 -3  0 1  0 0 1 1";

fn error(text: &str) -> String {
    let result = sms::parse_sms(text, PATH);
    assert!(result.is_err(), "unexpected valid SMS: {text}");
    result.map_or_else(|error| error.to_string(), |_| String::new())
}

#[test]
fn polynomial_nonsquare_exact_order_and_signed_zero() {
    let s = sms::parse_sms(POLY, PATH).unwrap();
    assert_eq!((s.p, s.q), (1, 0));
    assert_eq!(s.net.len(), 2);
    assert_eq!(s.net[0].len(), 3);
    for (index, point) in s.net.iter().flatten().enumerate() {
        assert_eq!(
            *point,
            [
                (index * 3) as f64,
                (index * 3 + 1) as f64,
                (index * 3 + 2) as f64,
                NOW
            ]
        );
    }
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.v, [-0.0, 0.5, 1.0, 1.0]);
    assert_eq!(s.v[0].to_bits(), (-0.0_f64).to_bits());
}

#[test]
fn rational_coordinates_are_not_divided_or_reordered() {
    let s = sms::parse_sms(RATIONAL, PATH).unwrap();
    assert_eq!((s.p, s.q), (0, 1));
    assert_eq!(
        s.net,
        [vec![[2.0, 4.0, 6.0, 2.0], [-3.0, -6.0, -9.0, -3.0]]]
    );
    assert_eq!(s.u, [0.0, 1.0]);
    assert_eq!(s.v, [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn counts_accept_exponents_signed_zero_and_unrestricted_degrees() {
    let s = sms::parse_sms("-0 0e2 2e0 3E0 -0 1 2 3 0 0 1 1 0 0 0 1 1", PATH).unwrap();
    assert_eq!((s.p, s.q), (2, 3));
    assert_eq!(s.net, [vec![[1.0, 2.0, 3.0, NOW]]]);
    assert_eq!(s.u.len(), 4);
    assert_eq!(s.v.len(), 5);
}

#[test]
fn every_count_rejects_invalid_nonfinite_and_exclusive_limit() {
    let limit = 2.0_f64.powi(usize::BITS as i32).to_string();
    for field in 0..4 {
        for bad in ["-1", "0.5", "NaN", "inf", "-inf", "1e300", limit.as_str()] {
            let mut header = ["0", "0", "0", "0", "0"];
            header[field] = bad;
            let message = error(&header.join(" "));
            assert!(
                message.starts_with(&format!("{PATH}: bad count ")),
                "{message}"
            );
        }
    }
}

#[test]
fn huge_representable_counts_fail_without_header_allocation() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 2).to_string();
    for field in 0..4 {
        let mut header = ["0", "0", "0", "0", "0"];
        header[field] = &huge;
        let message = error(&header.join(" "));
        assert!(
            message.ends_with(": truncated") || message.ends_with(": SMS size overflow"),
            "{message}"
        );
    }
}

#[test]
fn checked_knot_net_coordinate_and_token_totals() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1).to_string();
    let quarter = 2.0_f64.powi(usize::BITS as i32 - 2).to_string();
    for header in [
        format!("{half} 0 {half} 0 0"),       // u knots
        format!("0 {half} 0 {half} 0"),       // v knots
        format!("{half} 2 0 0 0"),            // net product
        format!("{quarter} 0 0 0 1"),         // coordinate product
        format!("{quarter} 0 {quarter} 0 0"), // payload total
    ] {
        assert_eq!(error(&header), format!("{PATH}: SMS size overflow"));
    }
}

#[test]
fn every_truncated_prefix_reports_path() {
    for text in [POLY, RATIONAL] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(
                error(&tokens[..end].join(" ")),
                format!("{PATH}: truncated")
            );
        }
    }
}

#[test]
fn consumed_bad_numbers_precede_later_truncation() {
    let tokens: Vec<_> = RATIONAL.split_whitespace().collect();
    let parse_error = "bad".parse::<f64>().unwrap_err().to_string();
    for index in 0..tokens.len() {
        let mut damaged = tokens.clone();
        damaged[index] = "bad";
        for end in index + 1..=tokens.len() {
            assert_eq!(
                error(&damaged[..end].join(" ")),
                format!("{PATH}: {parse_error}")
            );
        }
    }
    assert_eq!(
        error("1000000 0 0 0 0 bad"),
        format!("{PATH}: {parse_error}")
    );
}

#[test]
fn numeric_and_nonnumeric_trailing_tokens_are_ignored() {
    for text in [POLY, RATIONAL] {
        let s = sms::parse_sms(&format!("{text} 42 NaN rubbish 1e999"), PATH).unwrap();
        let original = sms::parse_sms(text, PATH).unwrap();
        assert_eq!(s.net, original.net);
        assert_eq!(s.u, original.u);
        assert_eq!(s.v, original.v);
    }
}

#[test]
fn rational_flag_retains_numeric_nonzero_semantics() {
    for flag in ["-2", "0.5", "NaN", "inf", "-inf"] {
        let text = format!("0 1 0 1 {flag} 2 4 6 2 -3 -6 -9 -3 0 1 0 0 1 1");
        let s = sms::parse_sms(&text, PATH).unwrap();
        assert_eq!(s.net[0][0], [2.0, 4.0, 6.0, 2.0]);
    }
    let s = sms::parse_sms("0 0 0 0 0 NaN inf -inf 0 1 0 1", PATH).unwrap();
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
}
