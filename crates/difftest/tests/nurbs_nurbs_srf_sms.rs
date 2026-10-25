//! Regression tests for the exact SMS decoder used by `nurbs.nurbs_srf`.

pub use difftest::HarnessError;
#[path = "../src/units/fixtures/nurbs_nurbs_srf_sms.rs"]
mod sms;

use sms::parse_sms;
use solidcore::nurbs::math::NOW;

const PATH: &str = "fixtures/example.sms";

fn error(text: &str) -> String {
    let result = parse_sms(text, PATH);
    assert!(result.is_err(), "unexpected valid SMS: {text}");
    let message = result.map_or_else(|error| error.to_string(), |_| String::new());
    assert!(message.contains(PATH), "{message}");
    message
}

fn record(rational: bool) -> Vec<String> {
    // 2 rows, 3 columns, intentionally distinct degrees and knot lengths.
    let mut tokens: Vec<String> = ["1", "2", "0", "1", if rational { "1" } else { "0" }]
        .into_iter()
        .map(str::to_owned)
        .collect();
    for i in 0..6 {
        for j in 0..if rational { 4 } else { 3 } {
            tokens.push((10 * i + j).to_string());
        }
    }
    tokens.extend(["100", "101", "102", "200", "201", "202", "203", "204"].map(str::to_owned));
    tokens
}

#[test]
fn nonsquare_polynomial_order_and_now() {
    let s = parse_sms(&record(false).join(" \n\t"), PATH).unwrap();
    assert_eq!((s.p, s.q), (0, 1));
    assert_eq!(s.net.len(), 2);
    for (i, row) in s.net.iter().enumerate() {
        assert_eq!(row.len(), 3);
        for (j, point) in row.iter().enumerate() {
            let x = (10 * (i * 3 + j)) as f64;
            assert_eq!(*point, [x, x + 1.0, x + 2.0, NOW]);
        }
    }
    assert_eq!(s.u, [100.0, 101.0, 102.0]);
    assert_eq!(s.v, [200.0, 201.0, 202.0, 203.0, 204.0]);
}

#[test]
fn raw_rational_flags_and_homogeneous_coordinates() {
    for flag in ["1", "-1", "2.5", "NaN", "inf", "-inf"] {
        let mut tokens = record(true);
        tokens[4] = flag.into();
        let s = parse_sms(&tokens.join(" "), PATH).unwrap();
        assert_eq!(s.net[0][0], [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(s.net[1][2], [50.0, 51.0, 52.0, 53.0]);
        assert_eq!(s.u, [100.0, 101.0, 102.0]);
        assert_eq!(s.v, [200.0, 201.0, 202.0, 203.0, 204.0]);
    }
}

#[test]
fn signed_zero_counts_flag_and_payload() {
    let s = parse_sms("-0 0e9 -0 0E0 -0 -0 0 -0 -0 0 0 -0", PATH).unwrap();
    assert_eq!((s.p, s.q), (0, 0));
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.net[0][0][2].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.net[0][0][3], NOW);
    assert_eq!(s.u[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.v[1].to_bits(), (-0.0_f64).to_bits());
}

#[test]
fn nonfinite_geometry_and_unvalidated_knots_weights() {
    let s = parse_sms("0 0 0 0 1 NaN inf -inf -0 9 -9 NaN inf", PATH).unwrap();
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
    assert_eq!(s.net[0][0][3].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u, [9.0, -9.0]);
    assert!(s.v[0].is_nan());
    assert_eq!(s.v[1], f64::INFINITY);
    for weight in ["0", "-2", "NaN", "inf", "-inf"] {
        let s = parse_sms(&format!("0 0 0 0 1 2 3 4 {weight} 0 0 0 0"), PATH).unwrap();
        assert_eq!(&s.net[0][0][..3], &[2.0, 3.0, 4.0]);
        let expected: f64 = weight.parse().unwrap();
        assert_eq!(s.net[0][0][3].to_bits(), expected.to_bits());
    }
}

#[test]
fn every_prefix_truncation() {
    for rational in [false, true] {
        let tokens = record(rational);
        for end in 0..tokens.len() {
            assert!(
                error(&tokens[..end].join(" ")).contains("truncated"),
                "prefix {end}"
            );
        }
        assert!(parse_sms(&tokens.join(" "), PATH).is_ok());
    }
}

#[test]
fn first_bad_consumed_float_beats_later_missing_or_bad_tokens() {
    for rational in [false, true] {
        let tokens = record(rational);
        for at in 0..tokens.len() {
            let mut bad = tokens.clone();
            bad[at] = "broken".into();
            for end in at + 1..=tokens.len() {
                let message = error(&bad[..end].join(" "));
                assert!(
                    message.contains("invalid float"),
                    "position {at}, end {end}: {message}"
                );
            }
            if at + 1 < bad.len() {
                bad[at + 1] = "also-broken".into();
                assert!(error(&bad.join(" ")).contains("invalid float"));
            }
        }
    }
}

#[test]
fn all_numeric_and_malformed_tails_are_ignored() {
    for rational in [false, true] {
        let original = record(rational).join(" ");
        let expected = parse_sms(&original, PATH).unwrap();
        for tail in [
            "junk",
            "0 1 2",
            "NaN inf -inf",
            "1e999 broken NaN",
            "0 nope 9",
        ] {
            let actual = parse_sms(&format!("{original} {tail}"), PATH).unwrap();
            assert_eq!(actual.net, expected.net);
            assert_eq!((actual.p, actual.q), (expected.p, expected.q));
            assert_eq!(actual.u, expected.u);
            assert_eq!(actual.v, expected.v);
        }
    }
}

#[test]
fn invalid_counts_are_ordered_before_later_headers() {
    let bound = 2.0_f64.powi(usize::BITS as i32);
    for slot in 0..4 {
        for bad in [
            "-1".into(),
            "0.5".into(),
            "NaN".into(),
            "inf".into(),
            "-inf".into(),
            "1e100".into(),
            bound.to_string(),
            (bound * 2.0).to_string(),
        ] {
            let mut tokens = vec!["0".to_owned(); slot];
            tokens.push(bad);
            assert!(error(&tokens.join(" ")).contains("bad count"));
            tokens.push("broken".into());
            assert!(error(&tokens.join(" ")).contains("bad count"));
        }
    }
    assert!(error("broken -1 NaN").contains("invalid float"));
}

#[test]
fn huge_representable_counts_do_not_allocate_from_headers() {
    let bound = 2.0_f64.powi(usize::BITS as i32);
    let largest = if usize::BITS > 53 {
        f64::from_bits(bound.to_bits() - 1)
    } else {
        bound - 1.0
    };
    for slot in 0..4 {
        let mut tokens = vec!["0".to_owned(); 5];
        tokens[slot] = largest.to_string();
        let message = error(&tokens.join(" "));
        assert!(!message.contains("bad count"), "{message}");
        assert!(
            message.contains("overflow") || message.contains("truncated"),
            "{message}"
        );
        tokens[slot] = "1000000".into();
        assert!(error(&tokens.join(" ")).contains("truncated"));
        tokens.push("broken".into());
        assert!(error(&tokens.join(" ")).contains("invalid float"));
    }
}

#[test]
fn checked_sizes_reject_overflow_without_payload() {
    let bound = 2.0_f64.powi(usize::BITS as i32);
    // Exact powers of two are representable on every host. Cover knot sums,
    // net product, polynomial/rational stride, and cumulative payload addition.
    let half = bound / 2.0;
    for (n, m, p, q, flag) in [
        (half, 0.0, half, 0.0, 0),
        (0.0, half, 0.0, half, 0),
        (half, half, 0.0, 0.0, 0),
        (half, 0.0, 0.0, 0.0, 0),
        (half / 2.0, 0.0, 0.0, 0.0, 1),
        (half / 2.0, 0.0, 0.0, 0.0, 0),
    ] {
        assert!(error(&format!("{n} {m} {p} {q} {flag}")).contains("overflow"));
    }
}

#[test]
fn header_errors_precede_size_checks() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 1);
    assert!(error(&format!("{huge} 0 0 0 broken")).contains("invalid float"));
    assert!(error(&format!("{huge} 0 0 0")).contains("truncated"));
    assert!(error(&format!("{huge} -1 broken")).contains("bad count"));
}
