//! Non-C++ tests of the exact SMS helper used by the p1 runner.

#![allow(clippy::unwrap_used)]

use difftest::HarnessError;
use solidcore::nurbs::math::NOW;

#[path = "../src/units/fixtures/nurbs_b_spline_surface_p1_sms.rs"]
mod sms;

const PATH: &str = "fixture.sms";
const POLY: &str = "1 2 1 0 0 -0 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 -0 0 1 1 2 3 4 5";
const RAT: &str = "0 1 0 0 1 2 4 6 2 -3 -6 -9 -3 0 1 2 3 4";

fn parse(text: &str) -> sms::ParsedSms {
    sms::parse_sms(text, PATH).unwrap()
}

fn error(text: &str) -> String {
    sms::parse_sms(text, PATH).unwrap_err().to_string()
}

fn bits(s: &sms::ParsedSms) -> Vec<u64> {
    s.net
        .iter()
        .flatten()
        .flatten()
        .chain(&s.u)
        .chain(&s.v)
        .map(|v| v.to_bits())
        .collect()
}

fn malformed_error() -> String {
    format!("{PATH}: {}", "bad".parse::<f64>().unwrap_err())
}

#[test]
fn nonsquare_polynomial_preserves_v_fast_order_and_now() {
    let s = parse(POLY);
    assert_eq!((s.p, s.q), (1, 0));
    assert_eq!((s.net.len(), s.net[0].len()), (2, 3));
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    for (i, point) in s.net.iter().flatten().enumerate() {
        assert_eq!(point[1], (i * 3 + 2) as f64);
        assert_eq!(point[2], (i * 3 + 3) as f64);
        assert_eq!(point[3].to_bits(), NOW.to_bits());
    }
    assert_eq!(s.u, [-0.0, 0.0, 1.0, 1.0]);
    assert_eq!(s.u[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.v, [2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn rational_values_are_stored_undivided() {
    let s = parse(RAT);
    assert_eq!(s.net, [[[2.0, 4.0, 6.0, 2.0], [-3.0, -6.0, -9.0, -3.0]]]);
    assert_eq!(s.u, [0.0, 1.0]);
    assert_eq!(s.v, [2.0, 3.0, 4.0]);
}

#[test]
fn raw_rational_flags_keep_original_coordinate_width() {
    for flag in ["-2", "0.5", "NaN", "inf", "-inf", "1"] {
        let input = RAT.replacen("0 1 0 0 1", &format!("0 1 0 0 {flag}"), 1);
        assert_eq!(bits(&parse(&input)), bits(&parse(RAT)), "{flag}");
    }
    for flag in ["0", "-0", "0.0"] {
        let input = POLY.replacen("1 2 1 0 0", &format!("1 2 1 0 {flag}"), 1);
        assert_eq!(bits(&parse(&input)), bits(&parse(POLY)), "{flag}");
    }
}

#[test]
fn nonfinite_payload_and_signed_zero_are_not_geometry_validated() {
    let s = parse("0 0 0 0 NaN NaN inf -inf -0 -inf NaN -0 inf");
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
    assert_eq!(s.net[0][0][3].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u[0], f64::NEG_INFINITY);
    assert!(s.u[1].is_nan());
    assert_eq!(s.v[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.v[1], f64::INFINITY);
}

#[test]
fn integral_float_counts_and_unrestricted_degrees() {
    let input = POLY.replacen("1 2 1 0 0", "1.0 2e0 1e0 -0.0 0", 1);
    assert_eq!(bits(&parse(&input)), bits(&parse(POLY)));
    let s = parse("-0 0.0 3e0 2.0 0 1 2 3 0 1 2 3 4 5 6 7 8");
    assert_eq!((s.p, s.q, s.u.len(), s.v.len()), (3, 2, 5, 4));
}

#[test]
fn invalid_counts_at_every_header_position() {
    let exclusive = 2.0_f64.powi(usize::BITS as i32).to_string();
    for bad in ["-1", "0.5", "NaN", "inf", "-inf", "1e300", &exclusive] {
        for position in 0..4 {
            let mut tokens = ["0"; 5];
            tokens[position] = bad;
            let message = error(&tokens.join(" "));
            assert!(
                message.starts_with(&format!("{PATH}: bad count ")),
                "{message}"
            );
        }
    }
}

#[test]
fn header_errors_remain_sequential() {
    assert_eq!(error("-1 bad"), format!("{PATH}: bad count -1"));
    assert_eq!(error("0 bad -1"), malformed_error());
    assert_eq!(error("0 0 0 -1 bad"), format!("{PATH}: bad count -1"));
    assert_eq!(error("0 0 0 0 bad"), malformed_error());
}

#[test]
fn checked_shape_arithmetic_and_tiny_bodies() {
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    let below = f64::from_bits(exclusive.to_bits() - 1).floor();
    let half = exclusive / 2.0;
    let root = 2.0_f64.powi((usize::BITS / 2) as i32);
    let overflow = format!("{PATH}: SMS size overflow");
    // knot addition, points, coordinates, then total addition.
    for input in [
        format!("{half} 0 {half} 0 0"),
        format!("0 {half} 0 {half} 0"),
        format!("{root} {root} 0 0 0"),
        format!("{} 0 0 0 1", half / 2.0),
        format!("{} 0 0 0 0", half / 2.0 - 1.0),
        format!("{below} 0 0 0 0"),
    ] {
        assert_eq!(error(&input), overflow, "{input}");
    }
    for body in ["", " 1", " 1 2 3", " NaN inf -inf"] {
        assert_eq!(
            error(&format!("1000000000 0 0 0 0{body}")),
            format!("{PATH}: truncated")
        );
    }
    assert_eq!(error("1000000000 0 0 0 0 bad"), malformed_error());
}

#[test]
fn every_prefix_truncation_is_reported_without_output_allocation() {
    for sample in [POLY, RAT] {
        let tokens: Vec<_> = sample.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(
                error(&tokens[..end].join(" ")),
                format!("{PATH}: truncated"),
                "prefix {end}"
            );
        }
    }
}

#[test]
fn malformed_consumed_numbers_precede_later_eof() {
    for sample in [POLY, RAT] {
        let tokens: Vec<_> = sample.split_whitespace().collect();
        for position in 0..tokens.len() {
            let mut input = tokens.clone();
            input[position] = "bad";
            assert_eq!(
                error(&input.join(" ")),
                malformed_error(),
                "complete {position}"
            );
            assert_eq!(
                error(&input[..=position].join(" ")),
                malformed_error(),
                "short {position}"
            );
        }
    }
}

#[test]
fn all_unused_tail_types_are_ignored_lazily() {
    for sample in [POLY, RAT] {
        let expected = bits(&parse(sample));
        for tail in [
            "bad",
            "1 2 -0 1e300",
            "NaN inf -inf",
            "# comment",
            "0.5 bad NaN inf 99",
        ] {
            assert_eq!(
                bits(&parse(&format!("{sample} {tail}"))),
                expected,
                "{tail}"
            );
        }
        let whitespace = sample
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("\t\r\n ");
        assert_eq!(bits(&parse(&whitespace)), expected);
    }
}

#[test]
fn available_fixture_asset_bit_census() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles");
    let mut checked = 0;
    for (file, rows, cols, p, q, u, v, count, bytes) in [
        ("Surface.sms", 14, 2, 3, 1, 18, 4, 111, 2094),
        ("offset_nurb.sms", 6, 2, 3, 1, 10, 4, 55, 988),
        ("DropCurveSurface.sms", 2, 2, 1, 1, 4, 4, 25, 430),
    ] {
        let path = root.join(file);
        if !path.exists() {
            eprintln!("SKIP unavailable asset: {}", path.display());
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let s = sms::parse_sms(&text, &path.to_string_lossy()).unwrap();
        assert_eq!(
            (s.net.len(), s.net[0].len(), s.p, s.q, s.u.len(), s.v.len()),
            (rows, cols, p, q, u, v)
        );
        assert_eq!(text.len(), bytes);
        assert_eq!(text.split_whitespace().count(), count);
        // Independent allocation-free numeric iterator, not the unsafe old reader.
        let mut payload = text
            .split_whitespace()
            .skip(5)
            .map(|t| t.parse::<f64>().unwrap().to_bits());
        for point in s.net.iter().flatten() {
            for coordinate in &point[..3] {
                assert_eq!(Some(coordinate.to_bits()), payload.next());
            }
            assert_eq!(point[3].to_bits(), NOW.to_bits());
        }
        for knot in s.u.iter().chain(&s.v) {
            assert_eq!(Some(knot.to_bits()), payload.next());
        }
        assert_eq!(payload.next(), None);
        checked += 1;
        eprintln!(
            "asset bit census: {file}: {rows}x{cols}, u/v={u}/{v}, tokens={count}, bytes={bytes}"
        );
    }
    eprintln!("asset bit census: {checked}/3 available assets checked");
}
