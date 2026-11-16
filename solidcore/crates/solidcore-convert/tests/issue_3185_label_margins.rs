//! (#3185) An analytic label is kept on its NURBS form when the kernel's analytic surface,
//! carrying the label's numbers, evaluates as the form within the rounding
//! `AnalyticSupport::build` derives term by term (`surface::implicit`'s `evaluates_as`):
//! [`AnalyticSupport::parametrisation_margin`] is the largest ratio of distance to bound over
//! the form's Greville points, at most 1 where the label stands.
//!
//! - The corpus labels #3137's parity run refused under the old constant `gamma(16 (p + q +
//!   2)) size`, pinned to their margins under the derived bound. The four torus labels
//!   stand (0.01 to 0.49); the cylinders (b94 1.23, b43 1.40, the 2903 rims 1.52 and 2.03)
//!   do not. Since #16297 a stored torus support supplies both converters' circles and
//!   their angular intervals, as well as the analytic evaluator's circles. Previously
//!   recognition's independently fitted generator and polar converter remained installed:
//!   b46's margin 1.69 measured their disagreement with the label, not a bad label.
//!   These are stored-label checks, not measurements of `AnalyticSupport::derive`.
//! - A torus the kernel makes at the groove's sizes stands on its own form, and the same label
//!   turned about its axis by four times what the bound allows is refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr,
    clippy::disallowed_methods
)] // test (the turn's cos and sin)

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::surface::from_parameters::from_parameters;
use solidcore::surface::implicit::AnalyticSupport;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore_record::brep::Brep as Record;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../solidcore/tests/kernel_cases")
}

/// The corpus's kernel, its fixes on (as the parity run and SolidCraft run it).
fn kernel() -> Kernel {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("fixes.json")).unwrap())
            .unwrap();
    let mut fixes = UpstreamFixes::default();
    for flag in v.as_array().unwrap() {
        *fixes.flag_mut(flag.as_str().unwrap()).unwrap() = true;
    }
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes;
    k
}

fn body(case: &str, id: &str) -> Record {
    let gz = corpus().join(format!("{case}.bodies.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: Value = serde_json::from_str(&text).unwrap();
    serde_json::from_value(records[id].clone()).unwrap()
}

fn placement(f: &solidcore::eval::Frame) -> Axis2Placement {
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    Axis2Placement::from_stored(
        Point3::new(f.origin[0], f.origin[1], f.origin[2]),
        v(f.x_axis),
        v(f.y_axis),
    )
}

/// A corpus face's label as the kernel's support, and its NURBS form.
fn label_and_form(
    k: &Kernel,
    case: &str,
    id: &str,
    face: usize,
) -> (
    AnalyticSupport,
    solidcore::nurbs::b_spline_surface::BSplineSurface,
) {
    let r = body(case, id);
    let f = &r.faces[face];
    let nurbs = solidcore_convert::surface(f.nurbs.as_ref().unwrap()).unwrap();
    let (s, _) = from_parameters(k, &nurbs, f.domain).unwrap();
    let SurfaceKind::BSpline(form) = s.kind() else {
        panic!("{case} {id} {face}: the form is not a B-spline")
    };
    let support = match solidcore_convert::surface(&f.surface).unwrap() {
        solidcore::eval::Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            ..
        } => AnalyticSupport::Torus {
            placement: placement(&frame),
            major_radius,
            minor_radius,
        },
        solidcore::eval::Surface::Cylinder { frame, radius } => AnalyticSupport::Cylinder {
            placement: placement(&frame),
            radius,
        },
        other => panic!("{case} {id} {face}: {other:?}"),
    };
    (support, form.copy())
}

/// (case, body, face, margin with the stored support's converters, whether the label stands).
///
/// Test change (#16297): installing the support's circles in both converters changes the
/// four torus margins without changing their labels, forms or the rounding bound. A separate
/// 60-digit rational evaluation of 1089 points on each recorded face gives maximum distance
/// to its labelled torus of 1.65e-14 mm (b6), 3.20e-15 (b46), 2.22e-13 (b85/15) and 2.46e-13
/// (b85/21). The b85 labels retain their small geometric discrepancies; their changed
/// margins still stand. The deliberately turned label below must still be refused.
const CASES: &[(&str, &str, usize, f64, bool)] = &[
    (
        "sweeps/3897-sweep-ring-along-a-short-arc-joined-to-a-collar",
        "b6",
        1,
        0.0163, // formerly 0.0201 with the independently fitted converters
        true,
    ),
    (
        "sweeps/4280-circular-pattern-of-sweep-cut-groove-on-hub",
        "b46",
        1,
        0.0098, // formerly 1.6943: the stored label now keeps its own converters
        true,
    ),
    (
        "sweeps/4280-circular-pattern-of-sweep-cut-groove-on-hub",
        "b85",
        15,
        0.4881, // formerly 0.4995; this face's label still stands
        true,
    ),
    (
        "sweeps/4280-circular-pattern-of-sweep-cut-groove-on-hub",
        "b85",
        21,
        0.4154, // formerly 0.4245; checked separately from face 15
        true,
    ),
    (
        "booleans/3456-deep-cut-through-drilled-taper-leaves-base",
        "b94",
        3,
        1.2339,
        false,
    ),
    (
        "crashes/2243-step-export-flanged-enclosure",
        "b43",
        1,
        1.4008,
        false,
    ),
    ("fillets/2903-original-rim-chamfer", "b0", 1, 1.5243, false),
    ("fillets/2903-original-rim-chamfer", "b0", 3, 2.0275, false),
    ("fillets/2903-original-rim-round", "b0", 1, 1.5243, false),
    ("fillets/2903-original-rim-round", "b0", 3, 2.0275, false),
];

/// Each corpus label's margin, pinned to 1 % of its measured value, on its side of 1, and
/// `build` agreeing with it.
#[test]
fn the_corpus_labels_stand_or_fall_by_the_derived_bound() {
    let k = kernel();
    let mut report = String::new();
    let mut wrong = Vec::new();
    for &(case, id, face, pinned, stands) in CASES {
        let (support, form) = label_and_form(&k, case, id, face);
        let m = support.parametrisation_margin(&k, &form).unwrap().unwrap();
        let _ = writeln!(report, "{case} {id} face {face}: margin {m:.4}");
        let built = support.build(&k, form).is_ok();
        if (m <= 1.0) != stands || built != stands || (m - pinned).abs() > 0.01 * pinned {
            wrong.push(format!(
                "{case} {id} face {face}: margin {m} (pinned {pinned}, stands {stands}), build {built}"
            ));
        }
    }
    eprintln!("{report}");
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// A torus at the groove's sizes (#3185's b46: centre (19.375, -33.558, 37.5), R 33.657, r 2,
/// a 43.6 degree sweep of the whole tube), made by the kernel: its own label stands on its
/// form; turned about its axis by four times the turn the bound allows (the margin is linear
/// in a small turn: a turn of `t` reads `m(t)`, so `4 t / m(t)` reads 4), it is refused.
#[test]
fn a_torus_label_turned_past_its_bound_is_refused() {
    let k = kernel();
    let o = Point3::new(19.374_999_999_999_975, -33.558_484_396_647_01, 37.5);
    let x = Vec3::new(
        -0.427_099_277_807_218_7,
        0.739_757_649_038_078_7,
        -0.519_946_946_895_745_4,
    );
    let y = Vec3::new(
        0.259_973_473_447_872_36,
        -0.450_287_264_631_874_2,
        -0.854_198_555_614_438_5,
    );
    let domain = Extent2d::from_bounds(0.0, -180.0, 43.602_818_972_703_645, 180.0);
    let made = Torus::new(
        &k,
        o,
        x,
        y,
        domain,
        33.657_280_044_590_65,
        2.0,
        TorusOptions::default(),
    )
    .unwrap();
    let s = Surface::from(made);
    let form = as_b_spline(&k, &s).unwrap().unwrap().into_owned();
    let own = AnalyticSupport::of(&s).unwrap();
    let m0 = own.parametrisation_margin(&k, &form).unwrap().unwrap();
    assert!(m0 <= 1.0, "own label: {m0}");
    assert!(own.build(&k, form.copy()).is_ok());
    let turned = |t: f64| {
        let AnalyticSupport::Torus {
            placement,
            major_radius,
            minor_radius,
        } = own
        else {
            panic!()
        };
        let (c, sn) = (t.cos(), t.sin());
        let (px, py) = (placement.x_axis(), placement.y_axis());
        AnalyticSupport::Torus {
            placement: Axis2Placement::from_stored(
                placement.origin(),
                px * c + py * sn,
                py * c - px * sn,
            ),
            major_radius,
            minor_radius,
        }
    };
    let probe = 1e-11;
    let m1 = turned(probe)
        .parametrisation_margin(&k, &form)
        .unwrap()
        .unwrap();
    assert!(m1 > 1.0, "a turn of {probe} rad reads {m1}");
    let four = 4.0 * probe / m1;
    let m4 = turned(four)
        .parametrisation_margin(&k, &form)
        .unwrap()
        .unwrap();
    eprintln!("own {m0:.4}; turn {probe:e}: {m1:.3}; turn {four:e}: {m4:.3}");
    assert!(m4 > 1.0, "a turn of {four:e} rad reads {m4}");
    assert!(turned(four).build(&k, form).is_err());
}
