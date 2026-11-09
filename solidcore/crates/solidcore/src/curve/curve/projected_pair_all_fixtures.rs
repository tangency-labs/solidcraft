//! Original affine All fixture regressions for #13698.

use crate::curve::curve::Curve;
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::curve::line::{BoundedLineOptions, Line};
use crate::geom::extent1d::Extent1d;
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::nurbs_crv::allocate_nurb_curve;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Debug)]
struct Handle;
impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn vector(v: &Value) -> Vec3 {
    Vec3::new(number(&v[0]), number(&v[1]), number(&v[2]))
}
fn interval(v: &Value) -> Extent1d {
    Extent1d::from_min_max(number(&v[0]), number(&v[1]))
}
fn curve(k: &Kernel, v: &Value) -> Curve {
    if let Some(v) = v.get("line") {
        return Curve::from(Line::bounded(
            vector(&v["p"]),
            vector(&v["v"]),
            &interval(&v["domain"]),
            &BoundedLineOptions {
                scale: number(&v["scale"]),
                inside_out: v["inside_out"].as_bool().unwrap(),
                ..Default::default()
            },
        ));
    }
    if let Some(v) = v.get("ellipse") {
        return Curve::from(
            Ellipse::new(
                k,
                vector(&v["c"]),
                vector(&v["x"]),
                vector(&v["y"]),
                interval(&v["domain"]),
                number(&v["radii"][0]),
                number(&v["radii"][1]),
                &EllipseOptions::default(),
            )
            .unwrap(),
        );
    }
    let v = &v["bspline"];
    let points = v["cpts"].as_array().unwrap();
    let knots = v["u"].as_array().unwrap();
    let mut net = allocate_nurb_curve(
        points.len() - 1,
        v["p"].as_u64().unwrap() as usize,
        knots.len() - 1,
    )
    .unwrap();
    for (dst, src) in net.cpts_mut().iter_mut().zip(points) {
        dst.x = number(&src[0]);
        dst.y = number(&src[1]);
        dst.z = number(&src[2]);
        dst.w = number(&src[3]);
    }
    for (dst, src) in net.knots_mut().iter_mut().zip(knots) {
        *dst = number(src);
    }
    let mut spline = BSplineCurve::from_nurb(net, v["dim"].as_u64().unwrap(), false);
    spline.set_out_of_bounds_enabled(v["oob"].as_bool().unwrap());
    Curve::from(spline)
}

#[test]
fn original_affine_all_cases_have_complete_near_best_corner_sets() {
    let cases: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plan/findings/diagnostics/projected_pair_all_cases_3060.json"
    )))
    .unwrap();
    for (name, expected, count) in [
        ("gen2050", 40.001231972795864, 2),
        ("gen2187", 6.310837322899526, 1),
        ("gen2585", 0.996226875922088, 2),
    ] {
        let input = &cases
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap()["inputs"];
        let k = Kernel::default();
        let a = curve(&k, &input["c1"]);
        let b = curve(&k, &input["c2"]);
        let tol = number(&input["tol"]);
        assert!(
            super::supports(
                [&a, &b],
                [interval(&input["ivl1"]), interval(&input["ivl2"])]
            ),
            "{name}: original affine case must select certified path"
        );
        let result = super::solve::<Handle>(
            &k,
            [&a, &b],
            [interval(&input["ivl1"]), interval(&input["ivl2"])],
            vector(&input["vecs"][0]),
            tol,
            [None, None],
            None,
        )
        .unwrap();
        assert_eq!(result.len(), count, "{name}");
        assert!(
            (result.as_slice()[0].start.solution_value - expected).abs() <= tol,
            "{name}"
        );
        let normal = vector(&input["vecs"][0]);
        for answer in result.as_slice() {
            let [s, t] = [answer.start.parameters[0], answer.start.parameters[1]];
            assert!(a.natural_interval().contains_value(s, 0.0));
            assert!(b.natural_interval().contains_value(t, 0.0));
            assert!(expected - answer.start.solution_value <= tol);
            // The reported value is the native distance of the returned pair.
            let d = a.evaluate_point(&k, s).unwrap() - b.evaluate_point(&k, t).unwrap();
            let measured = normal.cross(d).cross(normal).length();
            assert_eq!(answer.start.solution_value, measured, "{name}: ({s}, {t})");
        }
        if count == 2 {
            assert_ne!(
                result.as_slice()[0].start.parameters,
                result.as_slice()[1].start.parameters
            );
        }
        // Both returned pairs, as recorded in projected_pair_native_witnesses_3060.jsonl.
        let pairs: &[[f64; 2]] = match name {
            "gen2050" => &[
                [0.42689846142600174, 0.30573599521598394],
                [0.30573599521598394, 0.42689846142600174],
            ],
            "gen2585" => &[
                [0.6795950883084139, 1.0610357492278748],
                [0.15819072816225468, 3.977500125051104],
            ],
            _ => &[],
        };
        for (answer, pair) in result.as_slice().iter().zip(pairs) {
            assert_eq!(
                [answer.start.parameters[0], answer.start.parameters[1]],
                *pair,
                "{name}"
            );
        }
    }
}
