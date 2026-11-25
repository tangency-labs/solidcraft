//! tangency-labs/solidcore#2479: a pcurve's end points lie in its face surface's
//! parameter domain. `SmFace::SwapUV` mirrored each UV trim about `u = v` through a
//! rounded matrix, so every face `StitchAndOrient` (or any other reversal) swaps had trims
//! an ulp past its domain's bounds: a unit box's three swapped faces end at
//! `v = -2.2e-16`. The trims are now transposed exactly (WR-729). The scan ([`support::pcurve_domain_scan`]) walks every face's
//! loops; the cases are the operations whose out-of-domain ends were all made there.
//! The filleted box bottom is surveyed, not asserted: one of its round's trims ends an ulp
//! past `u = 1` from the fillet's own trim
//! (`plan/findings/blend.fillet_executive--box-bottom-round-trim-ends-an-ulp-past-its-domain.md`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "support/pcurve_domain_scan.rs"]
mod support;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::primitive::api_curves::{
    create_analytic_circle, create_ellipse, create_line_segment,
};
use solidcore::primitive::api_primitives::{
    create_box, create_cone, create_cylinder, create_sphere, create_torus,
};
use solidcore::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use solidcore::topology::edgeuse::p1::{mate, set_uv_trim_curve, uv_trim_curve, UvTrimCurveArg};
use solidcore::topology::face::p1::swap_uv;
use solidcore::topology::face::p2::loops;
use solidcore::topology::loopuse::loop_edgeuses;
use solidcore::topology::{edge, EdgeId, EdgeuseId};
use solidcore::{Brep, FaceId, Kernel, Point3, Vec3};

/// The edges whose midpoint satisfies `keep`.
fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            keep(c.evaluate_point(k, 0.5 * (ivl.min() + ivl.max())).unwrap())
        })
        .collect()
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

type Case = (&'static str, fn(&mut Kernel) -> Brep);

/// The operations of the regression: primitives, booleans through seams, fillets and
/// chamfers on planar and curved faces, a sweep and a revolve.
const CASES: &[Case] = &[
    ("unit box", |k| {
        create_box(k, p(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap()
    }),
    ("offset box", |k| {
        create_box(k, p(-5.0, 3.0, 7.0), 10.0, 20.0, 30.0).unwrap()
    }),
    ("cylinder", |k| {
        create_cylinder(k, p(1.0, 2.0, 3.0), 3.0, 10.0).unwrap()
    }),
    ("cone", |k| {
        create_cone(k, p(0.0, 0.0, 0.0), 4.0, 1.0, 6.0).unwrap()
    }),
    ("sphere", |k| {
        create_sphere(k, p(0.0, 0.0, 0.0), 5.0).unwrap()
    }),
    ("torus", |k| {
        create_torus(k, p(0.0, 0.0, 0.0), 6.0, 2.0).unwrap()
    }),
    ("box minus a cylinder through it", |k| {
        let block = create_box(k, p(0.0, 0.0, 0.0), 40.0, 40.0, 20.0).unwrap();
        let hole = create_cylinder(k, p(20.0, 20.0, -1.0), 3.0, 22.0).unwrap();
        boolean_difference(k, block, hole).unwrap()
    }),
    ("box minus a sphere across its seam", |k| {
        let block = create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let ball = create_sphere(k, p(10.0, 5.0, 5.0), 3.0).unwrap();
        boolean_difference(k, block, ball).unwrap()
    }),
    ("box minus a cone", |k| {
        let block = create_box(k, p(0.0, 0.0, 0.0), 20.0, 20.0, 10.0).unwrap();
        let cone = create_cone(k, p(10.0, 10.0, -1.0), 4.0, 2.0, 12.0).unwrap();
        boolean_difference(k, block, cone).unwrap()
    }),
    ("box minus a torus", |k| {
        let block = create_box(k, p(-10.0, -10.0, 0.0), 20.0, 20.0, 5.0).unwrap();
        let ring = create_torus(k, p(0.0, 0.0, 5.0), 6.0, 2.0).unwrap();
        boolean_difference(k, block, ring).unwrap()
    }),
    ("cylinder union a crescent across its seam", |k| {
        let small = create_cylinder(k, p(10.0, 0.0, 0.0), 6.0, 10.0).unwrap();
        let big = create_cylinder(k, p(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
        let crescent = boolean_difference(k, small, big).unwrap();
        let cylinder = create_cylinder(k, p(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
        boolean_union(k, cylinder, crescent).unwrap()
    }),
    ("filleted cylinder rim", |k| {
        let mut c = create_cylinder(k, p(0.0, 0.0, 0.0), 5.0, 8.0).unwrap();
        let rim = edges_where(k, &c, |m| (m.z - 8.0).abs() < 1e-6);
        fillet_edges(k, &mut c, &rim, 1.0, 1, 1, 1.0).unwrap();
        c
    }),
    ("extruded circle", |k| {
        let circle = Curve::from(create_analytic_circle(k, p(0.0, 0.0, 0.0), 4.0).unwrap());
        create_linear_sweep(k, &[Some(&circle)], Vec3::Z, 7.0, true).unwrap()
    }),
    ("revolved rectangle", |k| {
        let corners = [
            p(2.0, 0.0, 0.0),
            p(5.0, 0.0, 0.0),
            p(5.0, 0.0, 4.0),
            p(2.0, 0.0, 4.0),
        ];
        let sides: Vec<Curve> = (0..4)
            .map(|i| Curve::from(create_line_segment(corners[i], corners[(i + 1) % 4]).unwrap()))
            .collect();
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        create_rotational_sweep(k, &refs, Point3::ZERO, Vec3::Z, 360.0, true).unwrap()
    }),
];

/// Cases with another producer left: the old chamfer's faces (`fillet_edges` with
/// cross-section 0) end an ulp past `v = 1`, and so does one trim of the
/// box bottom's rounds (cross-section 1) at `u = 1`, which the face shrink keeps since it
/// keeps a trim that keeps the surface's map (WR-700; it removed and projected
/// them again before; recorded in
/// `plan/findings/blend.fillet_executive--box-bottom-round-trim-ends-an-ulp-past-its-domain.md`).
/// That code is being replaced by the generic blend (AGENTS.md §4b), so they are surveyed,
/// not asserted.
const SURVEY_ONLY: &[Case] = &[
    ("chamfered box top", |k| {
        let mut plate = create_box(k, p(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
        let top = edges_where(k, &plate, |m| (m.z - 10.0).abs() < 1e-6);
        fillet_edges(k, &mut plate, &top, 1.5, 0, 1, 1.0).unwrap();
        plate
    }),
    ("filleted box bottom", |k| {
        let mut tray = create_box(k, p(0.0, 0.0, 0.0), 25.0, 25.0, 10.0).unwrap();
        let bottom = edges_where(k, &tray, |m| m.z.abs() < 1e-6);
        fillet_edges(k, &mut tray, &bottom, 2.0, 1, 1, 1.0).unwrap();
        tray
    }),
];

/// The out-of-domain ends of every case of `cases`.
fn scan(cases: &[Case]) -> Vec<(&'static str, Vec<String>)> {
    cases
        .iter()
        .map(|&(name, build)| {
            let mut k = Kernel::default();
            let b = build(&mut k);
            (name, support::report(&k, &b))
        })
        .collect()
}

#[test]
fn every_pcurve_ends_in_its_face_domain() {
    let found: Vec<_> = scan(CASES)
        .into_iter()
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

/// The counts the PR quotes (`--ignored --nocapture`).
#[test]
#[ignore = "survey output"]
fn survey() {
    let all: Vec<Case> = CASES.iter().chain(SURVEY_ONLY).copied().collect();
    for (name, lines) in scan(&all) {
        println!("{name}: {}", lines.len());
        for l in &lines {
            println!("  {l}");
        }
    }
}

/// The bits of `c`'s NURB control points' weighted u, v and z and weight: a B-spline's own
/// NURB, a line's made as `SwapUV` makes it, an ellipse's made beforehand by the test.
fn nurb_uvw(c: &Curve) -> Vec<[u64; 4]> {
    let bits = |n: &solidcore::nurbs::crv_basic::Curve| -> Vec<[u64; 4]> {
        n.cpts()
            .iter()
            .map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), p.w.to_bits()])
            .collect()
    };
    match c.kind() {
        CurveKind::BSpline(s) => bits(s.gw_nurb().expect("a B-spline trim with its NURB")),
        CurveKind::Line(l) => bits(
            l.base_with_nurb()
                .unwrap()
                .gw_nurb()
                .expect("a line trim's NURB"),
        ),
        CurveKind::Ellipse(e) => bits(e.base().gw_nurb().expect("an ellipse trim with its NURB")),
        other => panic!("a trim that is neither a B-spline, a line nor an ellipse: {other:?}"),
    }
}

/// Every trim of `f`'s loops (its upward loop uses), by edge use.
fn face_trims(b: &Brep, f: FaceId) -> Vec<(EdgeuseId, Vec<[u64; 4]>)> {
    let mut out = Vec::new();
    for l in loops(b, f, false, None).unwrap() {
        for eu in loop_edgeuses(b, l).unwrap() {
            if let Some(c) = uv_trim_curve(b, eu).unwrap().0 {
                out.push((eu, nurb_uvw(c)));
            }
        }
    }
    out
}

/// A box whose first face's first trim is an ellipse (`SwapUV`'s conic arm). The ellipse
/// is not on the face's boundary; `SwapUV` only transposes it.
fn box_with_ellipse_trim(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, p(-5.0, 3.0, 7.0), 10.0, 20.0, 30.0).unwrap();
    let f = b.faces().iter().map(|(f, _)| f).next().unwrap();
    let l = loops(&b, f, false, None).unwrap()[0];
    let eu = loop_edgeuses(&b, l).unwrap()[0];
    let mut e = create_ellipse(k, p(0.3, 0.7, 0.0), 0.25, 0.125).unwrap();
    e.make_nurb(k).unwrap();
    set_uv_trim_curve(
        &mut b,
        eu,
        UvTrimCurveArg::New(Box::new(Curve::from(e))),
        0.0,
        true,
        None,
    )
    .unwrap();
    b
}

/// The bits of a weighted `z` once its curve is 3D: a 2D NURB's [`NOZ`] becomes `0`, as
/// upstream's mirror makes it (`N_CrvTransform` on a 2D curve).
fn made_3d(z: u64) -> u64 {
    if f64::from_bits(z) == NOZ {
        0.0f64.to_bits()
    } else {
        z
    }
}

/// WR-729: `SmFace::SwapUV` transposes each trim exactly. After a swap, the trim set on each
/// edge use's mate has its old NURB's control points with `u` and `v` swapped, bit for bit
/// (the weight unchanged, a 2D trim made 3D at `z = 0`), and a second swap gives every trim back bit for bit.
/// Upstream's mirror through the rounded matrix (`I - 2nn^T`, `-2.2e-16` on the diagonal)
/// fails both: it moves each coordinate by up to an ulp (`v = 0` became `-2.2e-16`). The
/// cylinder's cap circles are rational and the third body's ellipse takes the conic arm.
#[test]
fn swap_uv_transposes_every_trim_bit_for_bit() {
    let bodies: [fn(&mut Kernel) -> Brep; 3] = [
        |k| create_box(k, p(-5.0, 3.0, 7.0), 10.0, 20.0, 30.0).unwrap(),
        |k| create_cylinder(k, p(1.0, 2.0, 3.0), 2.5, 7.0).unwrap(),
        box_with_ellipse_trim,
    ];
    let (mut checked, mut rational, mut ellipses) = (0, 0, 0);
    for build in bodies {
        let mut k = Kernel::default();
        let mut b = build(&mut k);
        let faces: Vec<FaceId> = b.faces().iter().map(|(f, _)| f).collect();
        for f in faces {
            let before = face_trims(&b, f);
            assert!(!before.is_empty());
            for (eu, _) in &before {
                let c = uv_trim_curve(&b, *eu).unwrap().0.unwrap();
                ellipses += usize::from(matches!(c.kind(), CurveKind::Ellipse(_)));
            }
            swap_uv(&k, &mut b, f).unwrap();
            for (eu, cpts) in &before {
                let m = mate(&b, *eu).unwrap().unwrap();
                let swapped = nurb_uvw(uv_trim_curve(&b, m).unwrap().0.unwrap());
                let want: Vec<[u64; 4]> = cpts
                    .iter()
                    .map(|&[u, v, z, w]| [v, u, made_3d(z), w])
                    .collect();
                assert_eq!(swapped, want, "face {f:?} edge use {eu:?}");
                checked += 1;
                rational += usize::from(cpts.iter().any(|q| {
                    let w = f64::from_bits(q[3]);
                    w != NOW && w != 1.0
                }));
            }
            swap_uv(&k, &mut b, f).unwrap();
            let made_3d_before: Vec<_> = before
                .iter()
                .map(|(eu, c)| {
                    let c: Vec<[u64; 4]> = c
                        .iter()
                        .map(|&[u, v, z, w]| [u, v, made_3d(z), w])
                        .collect();
                    (*eu, c)
                })
                .collect();
            assert_eq!(
                face_trims(&b, f),
                made_3d_before,
                "face {f:?}: swapped twice"
            );
        }
    }
    assert_eq!(checked, 54, "{checked} trims checked");
    assert_eq!(rational, 3, "{rational} rational trims checked");
    assert_eq!(ellipses, 1, "{ellipses} ellipse trims checked");
}
