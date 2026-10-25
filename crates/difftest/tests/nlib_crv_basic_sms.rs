//! Regression tests for the runner's actual SMS decoder, without C++ linkage.

use difftest::HarnessError;
use solidcore::nurbs::math::NOW;
use std::fmt::Write;

#[path = "../src/units/fixtures/nlib_crv_basic_sms.rs"]
mod sms;

const PATH: &str = "fixtures/example.sms";

fn failure(text: &str) -> String {
    let result = sms::decode(text, PATH);
    assert!(result.is_err(), "unexpected success: {text}");
    result.err().map_or_else(String::new, |e| e.to_string())
}

fn assert_error(text: &str, message: &str) {
    assert_eq!(failure(text), format!("{PATH}: {message}"));
}

fn surface_text(flag: &str, rational: bool) -> String {
    let mut text = format!("1 2 1 2 {flag}");
    for point in 0..6 {
        let x = if point == 0 {
            "-0".to_owned()
        } else {
            (point * 10).to_string()
        };
        let _ = write!(text, " {x} {} {}", point * 10 + 1, point * 10 + 2);
        if rational {
            let _ = write!(text, " {}", -(point + 3));
        }
    }
    text.push_str(" 100 101 102 103 200 201 202 203 204 205");
    text
}

#[test]
fn rectangular_net_rational_flags_and_raw_coordinate_order() {
    for flag in ["0", "-0", "1", "-1", "0.5", "-0.5", "NaN", "inf", "-inf"] {
        let rational = flag.parse::<f64>().unwrap() != 0.0;
        let s = sms::decode(&surface_text(flag, rational), PATH).unwrap();
        assert_eq!((s.p, s.q), (1, 2));
        assert_eq!(s.net.len(), 2);
        for (i, row) in s.net.iter().enumerate() {
            assert_eq!(row.len(), 3);
            for (j, point) in row.iter().enumerate() {
                let index = (i * 3 + j) as f64;
                assert_eq!(
                    *point,
                    [
                        index * 10.0,
                        index * 10.0 + 1.0,
                        index * 10.0 + 2.0,
                        if rational { -(index + 3.0) } else { NOW }
                    ]
                );
            }
        }
        assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(s.u, [100.0, 101.0, 102.0, 103.0]);
        assert_eq!(s.v, [200.0, 201.0, 202.0, 203.0, 204.0, 205.0]);
    }
}

#[test]
fn floating_counts_signed_zero_and_unrestricted_degrees() {
    let s = sms::decode(
        "-0.0 0e0 2e0 3.0 -0 -0 2 3 10 11 12 13 20 21 22 23 24",
        PATH,
    )
    .unwrap();
    assert_eq!((s.p, s.q), (2, 3));
    assert_eq!(s.net, [vec![[-0.0, 2.0, 3.0, NOW]]]);
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u, [10.0, 11.0, 12.0, 13.0]);
    assert_eq!(s.v, [20.0, 21.0, 22.0, 23.0, 24.0]);
}

#[test]
fn invalid_nonfinite_and_exclusive_counts_in_each_position() {
    let exclusive = 2.0_f64.powi(usize::BITS as i32).to_string();
    for value in [
        "-1", "0.5", "NaN", "inf", "-inf", "1e300", "1e999", &exclusive,
    ] {
        for index in 0..4 {
            let mut header = ["0", "0", "0", "0", "0"];
            header[index] = value;
            assert_error(
                &header.join(" "),
                &format!("bad count {}", value.parse::<f64>().unwrap()),
            );
        }
    }
}

#[test]
fn huge_headers_with_tiny_bodies_are_preflighted() {
    let huge = 2.0_f64.powi((usize::BITS - 4) as i32).to_string();
    for index in 0..4 {
        let mut header = ["0", "0", "0", "0", "0"];
        header[index] = &huge;
        assert_error(&header.join(" "), "truncated");
        assert_error(&format!("{} 1 2 3", header.join(" ")), "truncated");
    }
}

#[test]
fn checked_shape_knot_coordinate_and_total_arithmetic() {
    let power = |bits| 2.0_f64.powi(bits).to_string();
    let half = power(usize::BITS as i32 - 1);
    let quarter = power(usize::BITS as i32 - 2);
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    let largest = f64::from_bits(exclusive.to_bits() - 1).floor();
    let remainder = (exclusive - largest - 1.0).to_string();
    for header in [
        format!("{half} {half} 0 0 0"),         // rows * columns
        format!("{half} 0 {half} 0 0"),         // n + p
        format!("0 {half} 0 {half} 0"),         // m + q
        format!("{largest} 0 {remainder} 0 0"), // final u knot +2
        format!("0 {largest} 0 {remainder} 0"), // final v knot +2
        format!("{half} 0 0 0 0"),              // points * 3
        format!("{quarter} 0 0 0 1"),           // points * 4
        format!("{quarter} 0 0 0 0"),           // coordinates + u knots
        format!("0 {quarter} 0 0 0"),           // coordinates + u knots + v knots
    ] {
        assert_error(&header, "count overflow");
    }
    // On 32-bit targets usize::MAX itself is exactly representable.
    if usize::BITS <= f64::MANTISSA_DIGITS {
        assert_error(&format!("{} 0 0 0 0", usize::MAX), "count overflow");
        assert_error(&format!("0 {} 0 0 0", usize::MAX), "count overflow");
    }
}

#[test]
fn every_header_coordinate_and_knot_prefix_is_truncated() {
    for flag in ["0", "1"] {
        let text = surface_text(flag, flag == "1");
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_error(&tokens[..end].join(" "), "truncated");
        }
        assert!(sms::decode(&text, PATH).is_ok());
    }
}

#[test]
fn eager_bad_tail_wins_and_reports_first_lexical_error() {
    let lexical = "bad".parse::<f64>().unwrap_err().to_string();
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    let half = exclusive / 2.0;
    for prefix in [
        String::new(),
        "-1".to_owned(),
        "0".to_owned(),
        "0 0 0 0".to_owned(),
        "0 0 0 0 0 1".to_owned(),
        surface_text("1", true),
        format!("{exclusive} 0 0 0 0"),
        format!("{half} {half} 0 0 0"),
    ] {
        assert_error(&format!("{prefix} bad also_bad"), &lexical);
    }
    let first = "1e".parse::<f64>().unwrap_err().to_string();
    assert_error("-1 1e bad", &first);
}

#[test]
fn ordered_header_errors_precede_later_missing_or_invalid_fields() {
    for (text, message) in [
        ("-1", "bad count -1"),
        ("0 -2", "bad count -2"),
        ("0 0 -3", "bad count -3"),
        ("0 0 0 -4", "bad count -4"),
        ("-1 -2 -3 -4", "bad count -1"),
        ("0 -2 -3 -4", "bad count -2"),
        ("0 0 -3 -4", "bad count -3"),
        ("0 0 0 0", "truncated"),
    ] {
        assert_error(text, message);
    }
}

#[test]
fn numeric_tails_and_nonfinite_payload_are_preserved() {
    let plain = sms::decode(&surface_text("0", false), PATH).unwrap();
    let tail = sms::decode(
        &format!("{} 999 NaN inf -inf", surface_text("0", false)),
        PATH,
    )
    .unwrap();
    assert_eq!(plain.net, tail.net);
    assert_eq!(plain.u, tail.u);
    assert_eq!(plain.v, tail.v);
    for (flag, weight) in [("0", ""), ("1", "-0 ")] {
        let s = sms::decode(
            &format!("0 0 0 0 {flag} NaN inf -inf {weight}NaN inf -inf -0"),
            PATH,
        )
        .unwrap();
        assert!(s.net[0][0][0].is_nan());
        assert_eq!(s.net[0][0][1], f64::INFINITY);
        assert_eq!(s.net[0][0][2], f64::NEG_INFINITY);
        assert_eq!(
            s.net[0][0][3].to_bits(),
            if flag == "1" {
                (-0.0_f64).to_bits()
            } else {
                NOW.to_bits()
            }
        );
        assert!(s.u[0].is_nan());
        assert_eq!(s.u[1], f64::INFINITY);
        assert_eq!(s.v[0], f64::NEG_INFINITY);
        assert_eq!(s.v[1].to_bits(), (-0.0_f64).to_bits());
    }
}
