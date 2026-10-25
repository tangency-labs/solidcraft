//! Exercise the exact runner-private SMS parser without C++.

use difftest::HarnessError;
use solidcore::nurbs::math::NOW;

#[path = "../src/units/fixtures/surface_surface_p5_sms.rs"]
mod sms;

const PATH: &str = "fixture/Surface.sms";
const POLY: &str = "0 0 0 0 0 1 2 3 0 1 0 1";

#[allow(clippy::unwrap_used)]
fn parse(text: &str) -> sms::ParsedSurface {
    sms::parse_sms(PATH, text).unwrap()
}

#[allow(clippy::panic)]
fn error(text: &str) -> String {
    match sms::parse_sms(PATH, text) {
        Ok(_) => panic!("unexpected success"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn polynomial_uses_now() {
    let s = parse(POLY);
    assert_eq!((s.p, s.q), (0, 0));
    assert_eq!(s.net, vec![vec![[1.0, 2.0, 3.0, NOW]]]);
    assert_eq!(s.u, [0.0, 1.0]);
    assert_eq!(s.v, [0.0, 1.0]);
}

#[test]
fn nonsquare_net_is_v_fastest_and_knots_keep_order() {
    let s = parse("1 2 1 0 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 21 22 23 24 31 32 33 34");
    assert_eq!((s.p, s.q), (1, 0));
    assert_eq!(s.net.len(), 2);
    assert_eq!(
        s.net[0],
        [
            [1.0, 2.0, 3.0, NOW],
            [4.0, 5.0, 6.0, NOW],
            [7.0, 8.0, 9.0, NOW]
        ]
    );
    assert_eq!(s.net[1][2], [16.0, 17.0, 18.0, NOW]);
    assert_eq!(s.u, [21.0, 22.0, 23.0, 24.0]);
    assert_eq!(s.v, [31.0, 32.0, 33.0, 34.0]);
}

#[test]
fn rational_points_stay_homogeneous() {
    let s = parse("0 0 0 0 1 2 4 6 2 0 1 0 1");
    assert_eq!(s.net[0][0], [2.0, 4.0, 6.0, 2.0]);
}

#[test]
fn every_nonzero_flag_is_rational() {
    for flag in ["-1", "0.5", "NaN", "inf", "-inf"] {
        assert_eq!(
            parse(&format!("0 0 0 0 {flag} 2 4 6 2 0 1 0 1")).net[0][0][3],
            2.0
        );
    }
    assert_eq!(parse("0 0 0 0 -0 1 2 3 0 1 0 1").net[0][0][3], NOW);
}

#[test]
fn signed_zero_survives() {
    let s = parse("-0 -0 -0 -0 1 -0 0 -0 -0 -0 1 0 -0");
    for value in [
        s.net[0][0][0],
        s.net[0][0][2],
        s.net[0][0][3],
        s.u[0],
        s.v[1],
    ] {
        assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
    }
}

#[test]
fn coordinates_and_knots_are_not_validated() {
    let s = parse("0 0 0 0 1 NaN inf -inf -2 9 -9 NaN inf");
    assert!(s.net[0][0][0].is_nan());
    assert_eq!(s.net[0][0][1], f64::INFINITY);
    assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
    assert_eq!(s.net[0][0][3], -2.0);
    assert_eq!(s.u, [9.0, -9.0]);
    assert!(s.v[0].is_nan());
}

#[test]
fn invalid_counts_in_each_header_position() {
    for bad in ["-1", "0.5", "NaN", "inf", "-inf"] {
        for index in 0..4 {
            let mut header = ["0"; 5];
            header[index] = bad;
            assert!(error(&header.join(" ")).contains("bad count"));
        }
    }
}

#[test]
fn exclusive_usize_bound_is_rejected() {
    let bound = 2.0_f64.powi(usize::BITS as i32);
    for bad in [bound, bound * 2.0, f64::MAX] {
        for index in 0..4 {
            let mut header = vec!["0".to_owned(); 5];
            header[index] = bad.to_string();
            assert!(error(&header.join(" ")).contains("bad count"));
        }
    }
    assert!(error("0 0 18446744073709551616 0 0 0 0 0").contains("bad count"));
}

fn largest_count() -> usize {
    let bound = 2.0_f64.powi(usize::BITS as i32);
    f64::from_bits(bound.to_bits() - 1) as usize
}

#[test]
fn huge_representable_counts_fail_on_actual_missing_data() {
    let huge = largest_count();
    for text in [
        format!("{huge} 0 0 0 0"),
        format!("0 {huge} 0 0 0"),
        format!("0 0 {huge} 0 0 0 0 0"),
        format!("0 0 0 {huge} 0 0 0 0 0 1"),
    ] {
        assert!(error(&text).ends_with("truncated"));
    }
}

#[test]
fn independent_u_and_v_checked_additions() {
    let huge = largest_count();
    let gap = usize::MAX - huge;
    // Cover overflow in n+degree, and independently in (n+degree)+2.
    for extent in [gap + 1, gap] {
        for v_axis in [false, true] {
            let mut text = if v_axis {
                format!("0 {extent} 0 {huge} 0")
            } else {
                format!("{extent} 0 {huge} 0 0")
            };
            for _ in 0..=extent {
                text.push_str(" 0 0 0");
            }
            if v_axis {
                text.push_str(" 0 1");
            }
            assert!(error(&text).ends_with("knot count overflow"));
        }
    }
}

#[test]
fn every_prefix_of_valid_bodies_is_truncated() {
    for text in [POLY, "0 0 0 0 1 2 4 6 2 0 1 0 1"] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert!(error(&tokens[..end].join(" ")).ends_with("truncated"));
        }
        parse(text);
    }
}

#[test]
fn net_and_u_truncation_precede_later_knot_overflow() {
    let huge = largest_count();
    let gap = usize::MAX - huge;
    // U overflow is possible, but not examined until the net is consumed.
    assert!(error(&format!("{gap} 0 {huge} 0 0")).ends_with("truncated"));
    // V overflow is possible, but not examined until all U knots are consumed.
    let mut text = format!("0 {gap} 0 {huge} 0");
    for _ in 0..=gap {
        text.push_str(" 0 0 0");
    }
    assert!(error(&text).ends_with("truncated"));
    text.push_str(" 0");
    assert!(error(&text).ends_with("truncated"));
    text.push_str(" 1");
    assert!(error(&text).ends_with("knot count overflow"));
}

#[test]
fn eager_bad_tail_precedes_header_or_body_errors() {
    for text in [POLY, "-1", "0 0 0 0 0", ""] {
        let err = error(&format!("{text} garbage"));
        assert!(err.starts_with(PATH));
        assert!(err.contains("invalid float literal"));
    }
}

#[test]
fn numeric_tail_is_ignored() {
    let s = parse(&format!("{POLY} 99 NaN inf -0"));
    assert_eq!(s.net, parse(POLY).net);
    assert_eq!(s.u, [0.0, 1.0]);
    assert_eq!(s.v, [0.0, 1.0]);
}

#[test]
fn parse_errors_include_path() {
    for text in ["", "-1", "garbage"] {
        assert!(error(text).starts_with(&format!("{PATH}: ")));
    }
}

#[test]
fn filesystem_errors_include_path() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/no-such-p5-sms-directory/Surface.sms"
    );
    let err = match sms::read_sms(path) {
        Ok(_) => panic!("unexpected file"),
        Err(e) => e.to_string(),
    };
    assert!(err.starts_with(&format!("{path}: ")));
}

#[test]
fn available_surface_fixture_matches_original_layout_bitwise() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Surface.sms");
    if !path.is_file() {
        eprintln!("Surface.sms bit census unavailable: {}", path.display());
        return;
    }
    let text = std::fs::read_to_string(&path).unwrap();
    let values: Vec<f64> = text
        .split_whitespace()
        .map(|token| token.parse().unwrap())
        .collect();
    // This reference only handles the trusted fixture, never malformed headers.
    for value in &values[..4] {
        assert!(value.is_finite() && *value >= 0.0 && *value < 10000.0);
        assert_eq!(value.fract(), 0.0);
    }
    let mut it = values.into_iter();
    let n = it.next().unwrap() as usize;
    let m = it.next().unwrap() as usize;
    let p = it.next().unwrap() as usize;
    let q = it.next().unwrap() as usize;
    let rat = it.next().unwrap() != 0.0;
    let mut expected = Vec::new();
    for _ in 0..=n {
        for _ in 0..=m {
            expected.extend([
                it.next().unwrap().to_bits(),
                it.next().unwrap().to_bits(),
                it.next().unwrap().to_bits(),
                if rat { it.next().unwrap() } else { NOW }.to_bits(),
            ]);
        }
    }
    for _ in 0..n + p + 2 + m + q + 2 {
        expected.push(it.next().unwrap().to_bits());
    }
    let actual = sms::read_sms(path.to_str().unwrap()).unwrap();
    assert_eq!((actual.p, actual.q), (p, q));
    assert_eq!(actual.net.len(), n + 1);
    assert!(actual.net.iter().all(|row| row.len() == m + 1));
    assert_eq!(actual.u.len(), n + p + 2);
    assert_eq!(actual.v.len(), m + q + 2);
    let bits: Vec<_> = actual
        .net
        .iter()
        .flatten()
        .flatten()
        .chain(&actual.u)
        .chain(&actual.v)
        .map(|value| value.to_bits())
        .collect();
    assert_eq!(bits, expected);
    eprintln!(
        "Surface.sms bit census: {}x{} points, degrees {p}/{q}, U/V knots {}/{}, {} matching f64 bit patterns",
        n + 1,
        m + 1,
        actual.u.len(),
        actual.v.len(),
        bits.len()
    );
}
