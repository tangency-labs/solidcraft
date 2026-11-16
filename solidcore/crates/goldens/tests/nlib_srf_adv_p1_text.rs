//! Non-C++ tests exercising the exact helper used by the runner adapters.

#[path = "../src/units/fixtures/nlib_srf_adv_p1_text.rs"]
mod text;

use solidcore::nurbs::math::{NOW, NOZ};
use text::{parse_smc, parse_sms};

const PATH: &str = "fixture/path";
const SMS: &str = "0 1 0 1 0  1 2 3 4 5 6  0 1  0 0 1 1";
const SMC: &str = "1 1 0 3  1 2 3 4 5 6  0 0 1 1";

fn replace(source: &str, index: usize, value: &str) -> String {
    source
        .split_whitespace()
        .enumerate()
        .map(|(i, token)| if i == index { value } else { token })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn sms_nonsquare_nonrational_v_fast() {
    let s = parse_sms(PATH, SMS).unwrap();
    assert_eq!((s.p, s.q), (0, 1));
    assert_eq!(s.net, vec![vec![[1., 2., 3., NOW], [4., 5., 6., NOW]]]);
    assert_eq!(s.u, [0., 1.]);
    assert_eq!(s.v, [0., 0., 1., 1.]);
    let s = parse_sms(PATH, "1 0 1 0 0 1 2 3 4 5 6 0 0 1 1 0 1").unwrap();
    assert_eq!(
        s.net,
        vec![vec![[1., 2., 3., NOW]], vec![[4., 5., 6., NOW]]]
    );
}

#[test]
fn sms_homogeneous_coordinates_and_permissive_rational_flag() {
    for flag in ["1", "-1", "0.5", "NaN", "inf"] {
        let source = format!("0 1 0 1 {flag} 2 4 6 2 9 12 15 3 0 1 0 0 1 1");
        let s = parse_sms(PATH, &source).unwrap();
        assert_eq!(s.net, vec![vec![[2., 4., 6., 2.], [9., 12., 15., 3.]]]);
    }
}

#[test]
fn smc_2d_3d_and_rational_preserve_coordinates() {
    let c = parse_smc(PATH, SMC).unwrap();
    assert_eq!(c.p, 1);
    assert_eq!(c.cpts, [[1., 2., 3., NOW], [4., 5., 6., NOW]]);
    assert_eq!(c.u, [0., 0., 1., 1.]);
    for (dim, point, expected) in [
        (2, "2 4", [2., 4., NOZ, NOW]),
        (3, "2 4 6", [2., 4., 6., NOW]),
    ] {
        let c = parse_smc(PATH, &format!("0 0 0 {dim} {point} 0 1")).unwrap();
        assert_eq!(c.cpts, [expected]);
    }
    for flag in ["1", "-1", "0.5", "NaN", "inf"] {
        for (dim, point, expected) in [
            (2, "2 4 2", [2., 4., NOZ, 2.]),
            (3, "2 4 6 2", [2., 4., 6., 2.]),
        ] {
            let c = parse_smc(PATH, &format!("0 0 {flag} {dim} {point} 0 1")).unwrap();
            assert_eq!(c.cpts, [expected]);
        }
    }
}

#[test]
fn smc_non3_dimensions_keep_original_two_coordinate_branch() {
    let large = 2.0_f64.powi(usize::BITS as i32 - 1);
    for dim in [0., 1., 2., 4., large] {
        for (flag, point, w) in [("0", "2 4", NOW), ("1", "2 4 2", 2.)] {
            let c = parse_smc(PATH, &format!("0 0 {flag} {dim} {point} 0 1")).unwrap();
            assert_eq!(c.cpts, [[2., 4., NOZ, w]]);
        }
    }
}

#[test]
fn every_count_rejects_bad_nonfinite_and_exclusive_upper_bound() {
    let exclusive = 2.0_f64.powi(usize::BITS as i32).to_string();
    for bad in ["-1", "0.5", "NaN", "inf", "-inf", "1e300", &exclusive] {
        for index in 0..4 {
            let e = parse_sms(PATH, &replace(SMS, index, bad)).unwrap_err();
            assert!(e.starts_with("fixture/path: bad count"), "{e}");
        }
        for index in [0, 1, 3] {
            let e = parse_smc(PATH, &replace(SMC, index, bad)).unwrap_err();
            assert!(e.starts_with("fixture/path: bad count"), "{e}");
        }
    }
}

#[test]
fn negative_zero_counts_and_flags_are_accepted() {
    let s = parse_sms(PATH, "-0 -0 -0 -0 -0 1 2 3 0 1 0 1").unwrap();
    assert_eq!(s.net, vec![vec![[1., 2., 3., NOW]]]);
    let c = parse_smc(PATH, "-0 -0 -0 -0 1 2 0 1").unwrap();
    assert_eq!(c.cpts, [[1., 2., NOZ, NOW]]);
}

#[test]
fn no_new_degree_or_geometry_validation() {
    let s = parse_sms(PATH, "0 0 4 5 0 NaN inf -inf 0 0 0 0 0 1 0 0 0 0 0 0 1").unwrap();
    assert_eq!((s.p, s.q), (4, 5));
    assert_eq!((s.u.len(), s.v.len()), (6, 7));
    let point = s.net.first().unwrap().first().unwrap();
    assert!(point.first().unwrap().is_nan());
    let c = parse_smc(PATH, "0 4 0 3 NaN inf -inf 0 0 0 0 0 1").unwrap();
    assert_eq!(c.p, 4);
    assert_eq!(c.u.len(), 6);
    assert!(c.cpts.first().unwrap().first().unwrap().is_nan());
}

#[test]
fn huge_representable_headers_preflight_without_allocating() {
    let huge = 2.0_f64.powi(usize::BITS as i32 - 2).to_string();
    for index in 0..4 {
        let e = parse_sms(PATH, &replace(SMS, index, &huge)).unwrap_err();
        assert!(
            e == "fixture/path: truncated" || e == "fixture/path: count overflow",
            "{e}"
        );
    }
    for index in [0, 1] {
        let e = parse_smc(PATH, &replace(SMC, index, &huge)).unwrap_err();
        assert!(
            e == "fixture/path: truncated" || e == "fixture/path: count overflow",
            "{e}"
        );
    }
}

#[test]
fn checked_knot_point_stride_and_total_payload_overflow() {
    let half = 2.0_f64.powi(usize::BITS as i32 - 1);
    let quarter = 2.0_f64.powi(usize::BITS as i32 - 2);
    for source in [
        format!("{half} 0 {half} 0 0"), // u knots
        format!("0 {half} 0 {half} 0"), // v knots
        format!("{half} {half} 0 0 0"), // row * column
        format!("{half} 0 0 0 0"),      // point * stride
        format!("{quarter} 0 0 0 0"),   // points + knots
    ] {
        assert_eq!(
            parse_sms(PATH, &source).unwrap_err(),
            "fixture/path: count overflow"
        );
    }
    for source in [
        format!("{half} {half} 0 3"),
        format!("{half} 0 0 3"),
        format!("{quarter} 0 1 3"),
    ] {
        assert_eq!(
            parse_smc(PATH, &source).unwrap_err(),
            "fixture/path: count overflow"
        );
    }
}

#[test]
fn truncation_at_every_header_point_and_knot_token() {
    for source in [SMS, "0 0 0 0 1 2 4 6 2 0 1 0 1"] {
        let tokens: Vec<_> = source.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(
                parse_sms(PATH, &tokens[..end].join(" ")).unwrap_err(),
                "fixture/path: truncated"
            );
        }
    }
    for source in [SMC, "0 0 1 2 2 4 2 0 1", "0 0 1 3 2 4 6 2 0 1"] {
        let tokens: Vec<_> = source.split_whitespace().collect();
        for end in 0..tokens.len() {
            assert_eq!(
                parse_smc(PATH, &tokens[..end].join(" ")).unwrap_err(),
                "fixture/path: truncated"
            );
        }
    }
}

#[test]
fn numeric_tails_are_ignored_but_malformed_tails_are_eager_errors() {
    assert_eq!(
        parse_sms(PATH, SMS),
        parse_sms(PATH, &format!("{SMS} 99 NaN inf"))
    );
    assert_eq!(
        parse_smc(PATH, SMC),
        parse_smc(PATH, &format!("{SMC} 99 NaN inf"))
    );
    let float_error = format!("{PATH}: {}", "malformed".parse::<f64>().unwrap_err());
    for source in [SMS, "-1 0 0 0 0", "0 0", "NaN", "1e300"] {
        assert_eq!(
            parse_sms(PATH, &format!("{source} malformed")).unwrap_err(),
            float_error
        );
    }
    for source in [SMC, "-1 0 0 3", "0 0", "NaN", "1e300"] {
        assert_eq!(
            parse_smc(PATH, &format!("{source} malformed")).unwrap_err(),
            float_error
        );
    }
}

#[test]
fn header_conversion_order_is_preserved() {
    assert_eq!(
        parse_sms(PATH, "-1 0.5 0 0 0").unwrap_err(),
        "fixture/path: bad count -1"
    );
    assert_eq!(
        parse_smc(PATH, "0 -1 NaN 0.5").unwrap_err(),
        "fixture/path: bad count -1"
    );
    assert_eq!(
        parse_smc(PATH, "0 0 NaN -1").unwrap_err(),
        "fixture/path: bad count -1"
    );
}
