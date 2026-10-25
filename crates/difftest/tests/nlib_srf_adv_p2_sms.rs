//! SMS reader regressions runnable without linking the C++ reference.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use difftest::HarnessError;
use solidcore::nurbs::math::NOW;

#[path = "../src/units/fixtures/nlib_srf_adv_p2_sms.rs"]
mod sms;

const PATH: &str = "fixtures/example.sms";
const POLYNOMIAL: &str = "1 2 1 2 0
    1 2 3 4 5 6 7 8 9
    10 11 12 13 14 15 16 17 18
    0 0 1 1
    0 0 0 1 1 1";
const RATIONAL: &str = "0 1 0 1 1
    2 4 6 2 -3 -6 -9 -3
    0 1
    0 0 1 1";

fn error(text: &str) -> String {
    match sms::parse_sms(text, PATH) {
        Ok(_) => unreachable!("expected a parse error"),
        Err(e) => {
            let message = e.to_string();
            assert!(message.contains(PATH), "{message}");
            message
        }
    }
}

#[test]
fn each_count_rejects_malformed_and_exclusive_limit() {
    let limit = 2.0_f64.powi(usize::BITS as i32).to_string();
    for at in 0..4 {
        for invalid in ["-1", "0.5", "NaN", "inf", "-inf", "1e999", &limit] {
            let mut header = ["0", "0", "0", "0", "0"];
            header[at] = invalid;
            assert!(error(&header.join(" ")).contains("bad count"));
        }
        let mut header = ["0", "0", "0", "0", "0"];
        header[at] = "not-a-number";
        assert!(!error(&header.join(" ")).contains("truncated"));
    }
}

#[test]
fn huge_representable_counts_do_not_allocate_from_header() {
    let huge = (2.0_f64.powi(usize::BITS as i32 - 4)).to_string();
    for at in 0..4 {
        let mut header = ["0", "0", "0", "0", "0"];
        header[at] = &huge;
        assert!(error(&header.join(" ")).contains("truncated"));
    }
    let limit = 2.0_f64.powi(usize::BITS as i32);
    let largest = if usize::BITS <= 53 {
        limit - 1.0
    } else {
        f64::from_bits(limit.to_bits() - 1)
    };
    for at in 0..4 {
        let largest = largest.to_string();
        let mut header = ["0", "0", "0", "0", "0"];
        header[at] = &largest;
        let message = error(&header.join(" "));
        assert!(!message.contains("bad count"), "{message}");
        assert!(message.contains("overflow") || message.contains("truncated"));
    }
}

#[test]
fn checked_size_arithmetic() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    // U/V knot addition, net multiplication, coordinate multiplication, then
    // payload additions (overflow at U, and separately at V).
    for header in [
        format!("{half} 0 {half} 0 0"),
        format!("0 {half} 0 {half} 0"),
        format!("{half} {half} 0 0 0"),
        format!("{half} 0 0 0 0"),
        format!("{} 0 {half} 0 1", half / 4.0),
        format!("0 0 {half} {half} 0"),
    ] {
        assert!(error(&header).contains("SMS size overflow"), "{header}");
    }
    // On 64-bit hosts usize::MAX is not representable as f64, so row/column
    // addition overflow is unreachable after count validation there.
    if usize::BITS <= 53 {
        let max = 2.0_f64.powi(usize::BITS as i32) - 1.0;
        assert!(error(&format!("{max} 0 0 0 0")).contains("overflow"));
        assert!(error(&format!("0 {max} 0 0 0")).contains("overflow"));
    }
}

#[test]
fn every_header_point_weight_and_knot_truncation() {
    for text in [POLYNOMIAL, RATIONAL] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert!(error(&tokens[..end].join(" ")).contains("truncated"));
        }
        assert!(sms::parse_sms(text, PATH).is_ok());
    }
}

#[test]
fn exact_nonsquare_polynomial_layout_and_now_weights() {
    let s = sms::parse_sms(POLYNOMIAL, PATH).unwrap();
    assert_eq!((s.p, s.q), (1, 2));
    assert_eq!(
        s.net,
        vec![
            vec![
                [1.0, 2.0, 3.0, NOW],
                [4.0, 5.0, 6.0, NOW],
                [7.0, 8.0, 9.0, NOW]
            ],
            vec![
                [10.0, 11.0, 12.0, NOW],
                [13.0, 14.0, 15.0, NOW],
                [16.0, 17.0, 18.0, NOW]
            ],
        ]
    );
    assert_eq!(s.u, [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.v, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn homogeneous_coordinates_and_all_nonzero_rational_flags() {
    for flag in ["1", "-1", "2.5", "NaN", "inf", "-inf"] {
        let text = RATIONAL.replacen("0 1 0 1 1", &format!("0 1 0 1 {flag}"), 1);
        let s = sms::parse_sms(&text, PATH).unwrap();
        assert_eq!((s.p, s.q), (0, 1));
        assert_eq!(
            s.net,
            vec![vec![[2.0, 4.0, 6.0, 2.0], [-3.0, -6.0, -9.0, -3.0]]]
        );
        assert_eq!(s.u, [0.0, 1.0]);
        assert_eq!(s.v, [0.0, 0.0, 1.0, 1.0]);
    }
}

#[test]
fn signed_zero_scientific_counts_and_unrestricted_numeric_payload() {
    let s = sms::parse_sms("-0 0e0 +0.0 -0e3 -0 NaN inf -0 -inf 1 0 1", PATH).unwrap();
    assert_eq!((s.p, s.q), (0, 0));
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.net[0][0][3], NOW);
    assert_eq!(s.u, [f64::NEG_INFINITY, 1.0]);
    assert_eq!(s.v, [0.0, 1.0]);
    let text = POLYNOMIAL.replacen("1 2 1 2", "1e0 2.0 1.0e0 +2e0", 1);
    assert!(sms::parse_sms(&text, PATH).is_ok());
}

#[test]
fn consumed_bad_numbers_precede_truncation() {
    for text in [POLYNOMIAL, RATIONAL] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for at in 4..tokens.len() {
            let mut prefix = tokens[..=at].to_vec();
            prefix[at] = "bad-number";
            let message = error(&prefix.join(" "));
            assert!(!message.contains("truncated"), "at {at}: {message}");
            let mut full = tokens.clone();
            full[at] = "bad-number";
            assert_eq!(error(&full.join(" ")), message);
        }
    }
    assert!(!error("1000000 0 0 0 0 1 bad-number").contains("truncated"));
}

#[test]
fn numeric_and_nonnumeric_trailing_tokens_are_ignored() {
    for text in [POLYNOMIAL, RATIONAL] {
        let expected = sms::parse_sms(text, PATH).unwrap();
        for tail in ["42 -3 NaN", "invalid trailing text", "42 invalid 1e999"] {
            let s = sms::parse_sms(&format!("{text} {tail}"), PATH).unwrap();
            assert_eq!((s.p, s.q), (expected.p, expected.q));
            assert_eq!(s.net, expected.net);
            assert_eq!(s.u, expected.u);
            assert_eq!(s.v, expected.v);
        }
    }
}
