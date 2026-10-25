//! Non-CPP coverage of the production query.tess SMS reader.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

#[path = "../src/units/fixtures/query_tess_sms.rs"]
mod query_tess_sms;

use query_tess_sms::{parse, ParsedSurface};
use solidcore::nurbs::math::NOW;
use std::fmt::Write;

fn bits(actual: f64, expected: f64) {
    assert_eq!(actual.to_bits(), expected.to_bits());
}

fn minimal(flag: &str) -> String {
    let weight = if flag.parse::<f64>().unwrap() == 0.0 {
        ""
    } else {
        " -0"
    };
    format!("0 0 0 0 {flag} -0 2 -3{weight} -0 1 0 1")
}

#[test]
fn polynomial_scientific_headers_and_signed_zero() {
    let s = parse("-0 0e8 -0 0e-3 -0 -0 2 -3 -0 1 0 1").unwrap();
    assert_eq!((s.p, s.q), (0, 0));
    assert_eq!((s.net.len(), s.net[0].len()), (1, 1));
    bits(s.net[0][0][0], -0.0);
    bits(s.net[0][0][3], NOW);
    bits(s.u[0], -0.0);
    assert_eq!(s.v, [0.0, 1.0]);
}

#[test]
fn raw_flags_and_homogeneous_weights_are_not_normalized() {
    for flag in ["0", "-0", "1", "2", "-1", "0.5", "NaN", "inf", "-inf"] {
        let s = parse(&minimal(flag)).unwrap();
        bits(s.net[0][0][0], -0.0);
        bits(s.net[0][0][1], 2.0);
        bits(s.net[0][0][2], -3.0);
        bits(
            s.net[0][0][3],
            if flag.parse::<f64>().unwrap() == 0.0 {
                NOW
            } else {
                -0.0
            },
        );
    }
}

#[test]
fn asymmetric_v_fastest_net_and_distinct_degrees() {
    let mut text = "1 2 3 1 2".to_owned();
    for i in 0..6 {
        write!(text, " {} {} {} {}", i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 3).unwrap();
    }
    text.push_str(" 30 31 32 33 34 35 40 41 42 43 44");
    let s = parse(&text).unwrap();
    assert_eq!((s.p, s.q), (3, 1));
    assert_eq!((s.net.len(), s.net[0].len()), (2, 3));
    for (i, point) in s.net.iter().flatten().enumerate() {
        for (j, value) in point.iter().enumerate() {
            bits(*value, (4 * i + j) as f64);
        }
    }
    assert_eq!(s.u, [30., 31., 32., 33., 34., 35.]);
    assert_eq!(s.v, [40., 41., 42., 43., 44.]);
}

#[test]
fn coordinates_weights_and_knots_remain_unrestricted() {
    let s = parse("0 0 0 0 1 NaN inf -inf NaN -inf NaN inf -0").unwrap();
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
    assert!(s.net[0][0][3].is_nan());
    assert_eq!(s.u[0], f64::NEG_INFINITY);
    assert!(s.u[1].is_nan());
    assert_eq!(s.v[0], f64::INFINITY);
    bits(s.v[1], -0.0);
}

#[test]
fn every_polynomial_and_rational_prefix_is_truncated() {
    for flag in ["0", "1"] {
        let text = minimal(flag);
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(parse(&tokens[..end].join(" ")).unwrap_err(), "truncated");
        }
        assert!(parse(&text).is_ok());
    }
}

#[test]
fn counts_validate_in_header_order() {
    for position in 0..4 {
        for invalid in ["-1", "0.5", "NaN", "inf", "-inf"] {
            let mut header = vec!["0"; position];
            header.push(invalid);
            let error = parse(&header.join(" ")).unwrap_err();
            assert_eq!(
                error,
                format!("bad count {}", invalid.parse::<f64>().unwrap())
            );
        }
    }
    assert_eq!(parse("-1 NaN").unwrap_err(), "bad count -1");
}

fn exclusive() -> f64 {
    2.0_f64.powi(usize::BITS as i32)
}

fn largest_count() -> f64 {
    if usize::BITS <= 53 {
        exclusive() - 1.0
    } else {
        f64::from_bits(exclusive().to_bits() - 1)
    }
}

#[test]
fn exclusive_usize_boundary_rejected_in_every_count_position() {
    for position in 0..4 {
        for value in [exclusive(), exclusive() * 2.0, f64::MAX] {
            let mut header = vec!["0".to_owned(); position];
            header.push(value.to_string());
            assert_eq!(
                parse(&header.join(" ")).unwrap_err(),
                format!("bad count {value}")
            );
        }
    }
}

#[test]
fn largest_in_range_count_is_not_rejected_as_bad_count() {
    for position in 0..4 {
        let mut header = vec!["0".to_owned(); position];
        header.push(largest_count().to_string());
        assert_eq!(parse(&header.join(" ")).unwrap_err(), "truncated");
    }
}

#[test]
fn huge_net_sizes_fail_from_available_payload_without_allocation() {
    let large = largest_count();
    for (n, m) in [
        (large, 0.0),
        (0.0, large),
        (large, large),
        (large / 2.0, 1.0),
    ] {
        assert_eq!(
            parse(&format!("{n} {m} 0 0 1 1 2 3 4")).unwrap_err(),
            "truncated"
        );
    }
}

// The gap below 2^BITS is small (2048 on 64-bit). This net reaches knot
// arithmetic with a valid count, using only a few thousand actual input numbers.
fn knot_overflow_input(v_axis: bool, complete_net: bool, complete_u: bool) -> String {
    let degree = largest_count() as usize;
    let last = usize::MAX - degree;
    assert!(last <= 2048);
    let (n, m, p, q) = if v_axis {
        (0, last, 0, degree)
    } else {
        (last, 0, degree, 0)
    };
    let mut text = format!("{n} {m} {p} {q} 0");
    if complete_net {
        for _ in 0..=last {
            text.push_str(" 1 2 3");
        }
    }
    if complete_u {
        text.push_str(" 0 1");
    }
    text
}

#[test]
fn checked_u_and_v_knot_additions_fail_softly() {
    assert_eq!(
        parse(&knot_overflow_input(false, true, false)).unwrap_err(),
        "count overflow"
    );
    assert_eq!(
        parse(&knot_overflow_input(true, true, true)).unwrap_err(),
        "count overflow"
    );
}

#[test]
fn net_eof_precedes_u_overflow_and_u_eof_precedes_v_overflow() {
    assert_eq!(
        parse(&knot_overflow_input(false, false, false)).unwrap_err(),
        "truncated"
    );
    assert_eq!(
        parse(&knot_overflow_input(true, false, false)).unwrap_err(),
        "truncated"
    );
    assert_eq!(
        parse(&knot_overflow_input(true, true, false)).unwrap_err(),
        "truncated"
    );
    let mut one_u = knot_overflow_input(true, true, false);
    one_u.push_str(" 0");
    assert_eq!(parse(&one_u).unwrap_err(), "truncated");
}

#[test]
fn numeric_tails_ignored_but_lexical_tails_win_all_earlier_errors() {
    let s = parse(&format!("{} NaN inf -1 999", minimal("0"))).unwrap();
    assert_eq!(s.u.len(), 2);
    assert_eq!(s.v.len(), 2);
    let lexical = "bad".parse::<f64>().unwrap_err().to_string();
    for text in [
        minimal("0"),
        "-1".to_owned(),
        "0".to_owned(),
        format!("{}", exclusive()),
        knot_overflow_input(false, true, false),
    ] {
        assert_eq!(parse(&format!("{text} bad")).unwrap_err(), lexical);
    }
}

// Compare every stored bit directly against the file's numeric tokens, not a
// second parser implementation. Missing optional upstream data is reported.
fn fixture_bits(text: &str, s: &ParsedSurface) -> usize {
    let nums: Vec<f64> = text
        .split_whitespace()
        .map(|t| t.parse().unwrap())
        .collect();
    let (n, m, p, q) = (
        nums[0] as usize,
        nums[1] as usize,
        nums[2] as usize,
        nums[3] as usize,
    );
    assert_eq!((s.p, s.q), (p, q));
    assert_eq!(s.net.len(), n + 1);
    let rational = nums[4] != 0.0;
    let mut tokens = nums[5..].iter().copied();
    let mut compared = 0;
    for row in &s.net {
        assert_eq!(row.len(), m + 1);
        for point in row {
            for value in point.iter().take(3) {
                bits(*value, tokens.next().unwrap());
                compared += 1;
            }
            bits(
                point[3],
                if rational {
                    tokens.next().unwrap()
                } else {
                    NOW
                },
            );
            compared += 1;
        }
    }
    assert_eq!(s.u.len(), n + p + 2);
    assert_eq!(s.v.len(), m + q + 2);
    for value in s.u.iter().chain(&s.v) {
        bits(*value, tokens.next().unwrap());
        compared += 1;
    }
    compared
}

#[test]
fn available_fixture_bit_census() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles");
    let mut files = 0;
    let mut total = 0;
    for name in ["Surface.sms", "DropCurveSurface.sms", "offset_nurb.sms"] {
        let path = root.join(name);
        if !path.exists() {
            eprintln!("unavailable SMS fixture: {}", path.display());
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let s = parse(&text).unwrap();
        let count = fixture_bits(&text, &s);
        eprintln!("SMS bit census: {name}: {count} values");
        files += 1;
        total += count;
    }
    eprintln!("SMS bit census: {files}/3 files, {total} values");
}
