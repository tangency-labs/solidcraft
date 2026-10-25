//! Non-C++ regression tests of the production SMS decoder (not the SMC reader).

pub use difftest::HarnessError;
#[path = "../src/units/fixtures/nlib_srf_adv_p3_sms.rs"]
mod fixtures;

use fixtures::parse_sms;
use solidcore::nurbs::math::NOW;
use std::fmt::Write;

const PATH: &str = "fixture.sms";

fn error(text: &str) -> String {
    let result = parse_sms(text, PATH);
    assert!(result.is_err(), "expected rejection: {text}");
    result.err().map(|e| e.to_string()).unwrap_or_default()
}

fn tiny(flag: &str) -> String {
    let rational = !matches!(flag, "0" | "-0");
    format!(
        "0 0 0 0 {flag} 2 -4 6 {}0 1 2 3",
        if rational { "-2 " } else { "" }
    )
}

#[test]
fn nonsquare_weighted_v_fastest_and_unequal_degrees() {
    let mut text = String::from("1 2 0 1 1 ");
    let mut expected = Vec::new();
    for row in 0..2 {
        let mut points = Vec::new();
        for col in 0..3 {
            let x = f64::from(10 * row + col);
            let w = if col == 0 { 0.0 } else { -2.0 };
            write!(text, "{x} {} {} {w} ", x + 1.0, x + 2.0).unwrap();
            points.push([x, x + 1.0, x + 2.0, w]);
        }
        expected.push(points);
    }
    text.push_str("10 11 12 20 21 22 23 24");
    let s = parse_sms(&text, PATH).unwrap();
    assert_eq!(s.net, expected);
    assert_eq!((s.p, s.q), (0, 1));
    assert_eq!(s.u, [10.0, 11.0, 12.0]);
    assert_eq!(s.v, [20.0, 21.0, 22.0, 23.0, 24.0]);
}

#[test]
fn raw_rational_flags_and_polynomial_now() {
    for flag in ["0", "-0", "1", "-1", "0.5", "NaN", "inf", "-inf"] {
        let s = parse_sms(&tiny(flag), PATH).unwrap();
        let w = if flag.parse::<f64>().unwrap() == 0.0 {
            NOW
        } else {
            -2.0
        };
        assert_eq!(s.net, vec![vec![[2.0, -4.0, 6.0, w]]]);
        assert_eq!((s.p, s.q), (0, 0));
    }
}

#[test]
fn floating_count_spellings_and_unrestricted_degrees() {
    let s = parse_sms("-0 0e9 3.0 4e0 -0 1 2 3 0 1 2 3 4 5 6 7 8 9 10", PATH).unwrap();
    assert_eq!((s.p, s.q), (3, 4));
    assert_eq!(s.u, [0.0, 1.0, 2.0, 3.0, 4.0]);
    assert_eq!(s.v, [5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
}

#[test]
fn signed_zero_and_nonfinite_payloads_survive() {
    let s = parse_sms("0 0 0 0 1 -0 NaN inf -inf -0 inf NaN -inf", PATH).unwrap();
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert!(s.net[0][0][1].is_nan());
    assert_eq!(s.net[0][0][2], f64::INFINITY);
    assert_eq!(s.net[0][0][3], f64::NEG_INFINITY);
    assert_eq!(s.u[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u[1], f64::INFINITY);
    assert!(s.v[0].is_nan());
    assert_eq!(s.v[1], f64::NEG_INFINITY);
    let s = parse_sms("0 0 0 0 1 0 0 0 -0 0 0 0 -0", PATH).unwrap();
    assert_eq!(s.net[0][0][3].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.v[1].to_bits(), (-0.0_f64).to_bits());
}

#[test]
fn every_prefix_is_truncated() {
    for flag in ["0", "1"] {
        let text = tiny(flag);
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(error(&tokens[..end].join(" ")), "fixture.sms: truncated");
        }
        assert!(parse_sms(&text, PATH).is_ok());
    }
}

#[test]
fn counts_are_finite_integral_nonnegative_and_exclusively_bounded() {
    let bound = 2.0_f64.powi(usize::BITS as i32);
    for value in [
        "NaN".to_owned(),
        "inf".to_owned(),
        "-inf".to_owned(),
        "-1".to_owned(),
        "0.5".to_owned(),
        format!("{bound:.0}"),
        format!("{:.0}", bound * 2.0),
    ] {
        for slot in 0..4 {
            let mut header = vec!["0".to_owned(); 5];
            header[slot] = value.clone();
            assert_eq!(
                error(&header.join(" ")),
                format!("fixture.sms: bad count {}", value.parse::<f64>().unwrap())
            );
        }
    }
}

#[test]
fn ordered_header_errors_and_eager_lexical_errors() {
    for text in ["-1", "0 -1", "0 0 -1", "0 0 0 -1"] {
        assert_eq!(error(text), "fixture.sms: bad count -1");
    }
    let lexical = format!("fixture.sms: {}", "bad-tail".parse::<f64>().unwrap_err());
    let huge = 2.0_f64.powi((usize::BITS - 1) as i32);
    for text in [
        "-1 bad-tail".to_owned(),
        "0 bad-tail".to_owned(),
        format!("{huge:.0} {huge:.0} 0 0 0 bad-tail"),
        format!("{} bad-tail", tiny("1")),
    ] {
        assert_eq!(error(&text), lexical);
    }
}

#[test]
fn huge_headers_and_checked_arithmetic_before_geometry_growth() {
    let huge = 2.0_f64.powi((usize::BITS - 4) as i32);
    for slot in 0..4 {
        let mut header = vec!["0".to_owned(); 5];
        header[slot] = format!("{huge:.0}");
        assert_eq!(error(&header.join(" ")), "fixture.sms: truncated");
    }
    let half = 2.0_f64.powi((usize::BITS - 1) as i32);
    // n+p / m+q, rows*cols, points*stride, payload additions.
    for header in [
        format!("{half:.0} 0 {half:.0} 0 0"),
        format!("0 {half:.0} 0 {half:.0} 0"),
        format!("{half:.0} 2 0 0 0"),
        format!("{half:.0} 0 0 0 1"),
        format!("{:.0} 0 {half:.0} 0 0", half / 4.0),
    ] {
        assert_eq!(error(&header), "fixture.sms: count overflow", "{header}");
    }
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    let largest = if usize::BITS <= 53 {
        usize::MAX as f64
    } else {
        f64::from_bits(exclusive.to_bits() - 1)
    };
    for header in [
        format!("{largest:.0} 0 2048 0 0"),
        format!("0 {largest:.0} 0 2048 0"),
    ] {
        assert_eq!(error(&header), "fixture.sms: count overflow");
    }
    if usize::BITS == 64 {
        // The sum fits (usize::MAX); only the final knot-count +2 overflows.
        for header in [
            format!("2047 0 {largest:.0} 0 0"),
            format!("0 2047 0 {largest:.0} 0"),
        ] {
            assert_eq!(error(&header), "fixture.sms: count overflow");
        }
    }
    // The last +2 and row/column +1 overflow are separately reachable on 32-bit.
    if usize::BITS <= 53 {
        for header in [
            format!("{largest:.0} 0 0 0 0"),
            format!("0 {largest:.0} 0 0 0"),
            format!("0 0 {largest:.0} 0 0"),
            format!("0 0 0 {largest:.0} 0"),
        ] {
            assert_eq!(error(&header), "fixture.sms: count overflow");
        }
    }
}

#[test]
fn numeric_tails_are_ignored_bit_for_bit() {
    for flag in ["0", "1"] {
        let text = tiny(flag);
        let a = parse_sms(&text, PATH).unwrap();
        let b = parse_sms(&format!("{text} -0 NaN inf -inf 123"), PATH).unwrap();
        assert_eq!(a.net, b.net);
        assert_eq!((a.p, a.q, a.u, a.v), (b.p, b.q, b.u, b.v));
    }
}

#[test]
fn available_sms_asset_storage_bit_census() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles");
    let mut present = 0;
    for name in ["Surface.sms", "offset_nurb.sms", "DropCurveSurface.sms"] {
        let path = root.join(name);
        if !path.exists() {
            eprintln!("SMS census missing: {}", path.display());
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let s = parse_sms(&text, path.to_str().unwrap()).unwrap();
        let numbers: Vec<f64> = text
            .split_whitespace()
            .map(|t| t.parse().unwrap())
            .collect();
        let rational = numbers[4] != 0.0;
        let mut stored = Vec::new();
        for row in &s.net {
            for point in row {
                stored.extend(
                    point[..if rational { 4 } else { 3 }]
                        .iter()
                        .map(|v| v.to_bits()),
                );
                if !rational {
                    assert_eq!(point[3].to_bits(), NOW.to_bits());
                }
            }
        }
        stored.extend(s.u.iter().chain(&s.v).map(|v| v.to_bits()));
        assert_eq!(s.net.len(), numbers[0] as usize + 1);
        assert!(s.net.iter().all(|row| row.len() == numbers[1] as usize + 1));
        assert_eq!((s.p, s.q), (numbers[2] as usize, numbers[3] as usize));
        assert_eq!(s.u.len(), numbers[0] as usize + s.p + 2);
        assert_eq!(s.v.len(), numbers[1] as usize + s.q + 2);
        assert_eq!(
            stored,
            numbers[5..5 + stored.len()]
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        eprintln!(
            "SMS census present: {name}, {} payload values bit-identical",
            stored.len()
        );
        present += 1;
    }
    eprintln!("SMS census: {present}/3 present (no SMC or C++ coverage)");
}
