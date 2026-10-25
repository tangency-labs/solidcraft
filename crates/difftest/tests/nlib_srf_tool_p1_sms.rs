//! Regression tests for the runner's actual SMS decoder, without C++ linkage.

use difftest::HarnessError;
use solidcore::nurbs::math::NOW;

#[path = "../src/units/fixtures/nlib_srf_tool_p1_sms.rs"]
mod sms;

const PATH: &str = "fixtures/example.sms";
const POLY: &str = "0 1 0 1 0  -0 2 3  4 5 6  10 11  20 21 22 23";
const RATIONAL: &str = "0 1 0 1 1  -0 2 3 -4  5 6 7 8  10 11  20 21 22 23";

fn failure(text: &str) -> String {
    let result = sms::decode(text, PATH);
    assert!(result.is_err(), "unexpected success: {text}");
    result.err().map_or_else(String::new, |e| e.to_string())
}

fn assert_error(text: &str, message: &str) {
    let e = failure(text);
    assert!(e.contains(PATH), "{e}");
    assert!(e.contains(message), "{e}");
}

#[test]
fn polynomial_preserves_shape_order_and_now() {
    let s = sms::decode(POLY, PATH).unwrap();
    assert_eq!((s.p, s.q), (0, 1));
    assert_eq!(
        s.net,
        vec![vec![[-0.0, 2.0, 3.0, NOW], [4.0, 5.0, 6.0, NOW]]]
    );
    assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(s.u, [10.0, 11.0]);
    assert_eq!(s.v, [20.0, 21.0, 22.0, 23.0]);
}

#[test]
fn rational_keeps_homogeneous_coordinates_and_all_nonzero_flags() {
    for flag in ["1", "-1", "0.5", "NaN", "inf", "-inf"] {
        let text = RATIONAL.replacen("0 1 0 1 1", &format!("0 1 0 1 {flag}"), 1);
        let s = sms::decode(&text, PATH).unwrap();
        assert_eq!((s.p, s.q), (0, 1));
        assert_eq!(
            s.net,
            vec![vec![[-0.0, 2.0, 3.0, -4.0], [5.0, 6.0, 7.0, 8.0]]]
        );
        assert_eq!(s.net[0][0][0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(s.u, [10.0, 11.0]);
        assert_eq!(s.v, [20.0, 21.0, 22.0, 23.0]);
    }
}

#[test]
fn nonsquare_net_is_v_fastest() {
    let s = sms::decode("1 0 0 0 0 1 2 3 4 5 6 10 11 12 20 21", PATH).unwrap();
    assert_eq!(
        s.net,
        vec![vec![[1.0, 2.0, 3.0, NOW]], vec![[4.0, 5.0, 6.0, NOW]]]
    );
    assert_eq!(s.u, [10.0, 11.0, 12.0]);
    assert_eq!(s.v, [20.0, 21.0]);
}

#[test]
fn scientific_counts_signed_zero_and_unrestricted_degrees() {
    let text = POLY.replacen("0 1 0 1 0", "-0 1e0 -0.0 1.0 -0", 1);
    let s = sms::decode(&text, PATH).unwrap();
    assert_eq!(s.net[0][1], [4.0, 5.0, 6.0, NOW]);
    let s = sms::decode("0 0 2 3 0 1 2 3 0 1 2 3 4 5 6 7 8", PATH).unwrap();
    assert_eq!((s.p, s.q), (2, 3));
    assert_eq!(s.u, [0.0, 1.0, 2.0, 3.0]);
    assert_eq!(s.v, [4.0, 5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn every_count_rejects_invalid_and_unrepresentable_values() {
    let exclusive = 2.0_f64.powi(usize::BITS as i32).to_string();
    for value in [
        "-1", "0.5", "NaN", "inf", "-inf", "1e300", "1e999", &exclusive,
    ] {
        for index in 0..4 {
            let mut header = ["0", "0", "0", "0", "0"];
            header[index] = value;
            assert_error(&header.join(" "), "bad count");
        }
    }
}

#[test]
fn huge_representable_counts_with_short_bodies_do_not_allocate() {
    let huge = 2.0_f64.powi((usize::BITS - 4) as i32).to_string();
    for index in 0..4 {
        let mut header = ["0", "0", "0", "0", "0"];
        header[index] = &huge;
        assert_error(&header.join(" "), "truncated");
        assert_error(&format!("{} 1 2 bad", header.join(" ")), "invalid float");
    }
}

#[test]
fn checked_knot_net_coordinate_and_total_payload_arithmetic() {
    let power = |bits| 2.0_f64.powi(bits).to_string();
    let half = power(usize::BITS as i32 - 1);
    let quarter = power(usize::BITS as i32 - 2);
    let square = power((usize::BITS / 2) as i32);
    let exclusive = 2.0_f64.powi(usize::BITS as i32);
    let largest = f64::from_bits(exclusive.to_bits() - 1).floor();
    // Compose MAX from two valid counts to reach the final knot-count +2.
    let remainder = (exclusive - largest - 1.0).to_string();
    let largest = largest.to_string();
    for header in [
        format!("{half} 0 {half} 0 0"),         // u knots
        format!("0 {half} 0 {half} 0"),         // v knots
        format!("{square} {square} 0 0 0"),     // net product
        format!("{half} 0 0 0 0"),              // polynomial coordinate product
        format!("{quarter} 0 0 0 1"),           // rational coordinate product
        format!("{quarter} 0 0 0 0"),           // total payload (products individually fit)
        format!("{largest} 0 {remainder} 0 0"), // u final +2
        format!("0 {largest} 0 {remainder} 0"), // v final +2
    ] {
        assert_error(&header, "count overflow");
    }
    // On 32-bit targets MAX itself is exactly representable, exercising +1.
    if usize::BITS <= f64::MANTISSA_DIGITS {
        for header in [
            format!("{} 0 0 0 0", usize::MAX),
            format!("0 {} 0 0 0", usize::MAX),
        ] {
            assert_error(&header, "count overflow");
        }
    }
}

#[test]
fn every_truncation_point_preserves_path_and_first_numeric_error() {
    for text in [POLY, RATIONAL] {
        let tokens: Vec<_> = text.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_error(&tokens[..end].join(" "), "truncated");
            for bad in 0..end {
                let mut prefix = tokens[..end].to_vec();
                prefix[bad] = "bad";
                assert_error(&prefix.join(" "), "invalid float");
            }
        }
        for bad in 0..tokens.len() {
            let mut body = tokens.clone();
            body[bad] = "bad";
            assert_error(&body.join(" "), "invalid float");
        }
    }
}

#[test]
fn trailing_numeric_and_nonnumeric_junk_is_never_parsed() {
    for text in [POLY, RATIONAL] {
        let original = sms::decode(text, PATH).unwrap();
        for tail in [" 99 100", " junk", " 99 NaN junk"] {
            let s = sms::decode(&format!("{text}{tail}"), PATH).unwrap();
            assert_eq!(s.net, original.net);
            assert_eq!(s.u, original.u);
            assert_eq!(s.v, original.v);
        }
    }
}
