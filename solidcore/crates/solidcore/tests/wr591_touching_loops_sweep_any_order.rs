//! WR-591 at operation level (`Kernel::fixes.loops_go_on_round_the_vertex`, the fork's
//! tangency-labs/smlib@9209e76 and b0ee3f9): the record's 10 x 6 outline and the
//! triangular hole whose corner touches it at (0, 3), as eight lines in the record's 24
//! orders (none, the outline or the hole reversed, every rotation of the list), swept 2
//! along +z with caps by `create_linear_sweep`. The caps go through
//! `create_plane_with_3d_curves`, so through `order_curves_into_loops` (#2423), with
//! `join_touching_loops` (WR-405) on as SolidCraft runs it.
//!
//! The fork has this case in one order: tangency-labs/smlib@1f2b0c4, capi test
//! `capi/tests/sweeps.cpp` "extrude plate notched at a corner" (`smlib_extrude` of the same
//! plate and hole, 2 up: volume 118, the level 2 check clean, 10 faces of one loop each,
//! two regions). These tests add the other 23 orders.
//!
//! Without the flag the cap plane is refused in 15 of the 24 orders, and the capped sweep
//! returns the cap's failure (upstream returned its eight side faces as an open skin). With it the plane is one loop of all eight
//! lines (area 59) in every order, and the swept body has ten faces, every edge manifold
//! and the right area, the plate less the hole, volume 118, in every order. In 9 of them
//! (rotations 4, 6 and 7 of each variant) the closed shell got no solid region (one void
//! region, volume 0, though `check_brep` passed it) until the sweep's stitch glued its
//! coincident lamina edges as neighbours round their line, a separate defect after the
//! loop ordering (#2506; see the WR-591 addendum of 2026-11-03):
//! [`wr591_on_sweep_is_the_plate_less_the_hole_in_every_order`].

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1::{edges, faces};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge::is_manifold;
use solidcore::{Kernel, Point3, Vec3};

const OUTLINE: [[f64; 2]; 5] = [[0.0, 0.0], [10.0, 0.0], [10.0, 6.0], [0.0, 6.0], [0.0, 3.0]];
const HOLE: [[f64; 2]; 3] = [[0.0, 3.0], [2.0, 3.5], [2.0, 2.5]];

/// The plate less the hole: (60 - 1) x 2.
const VOLUME: f64 = 118.0;

/// Both caps (2 x 59) and the side faces: the perimeters, 32 and 1 + 2 sqrt(4.25),
/// times 2.
fn area() -> f64 {
    2.0 * 59.0 + 2.0 * (32.0 + 1.0 + 2.0 * 4.25_f64.sqrt())
}

/// The orders whose cap plane is refused without the flag: (variant, rotation).
const REFUSED_OFF: [(usize, usize); 15] = [
    (0, 5),
    (0, 6),
    (0, 7),
    (1, 4),
    (1, 5),
    (1, 6),
    (1, 7),
    (2, 0),
    (2, 1),
    (2, 2),
    (2, 3),
    (2, 4),
    (2, 5),
    (2, 6),
    (2, 7),
];

/// The rotations whose closed body got no solid region with the flag before #2506's
/// stitch fix, in every variant.
const NO_SOLID_ROTATIONS: [usize; 3] = [4, 6, 7];

fn p(q: [f64; 2]) -> Point3 {
    Point3::new(q[0], q[1], 0.0)
}

/// The lines of a closed polygon, each reversed (and in reverse order of travel)
/// when `reversed`.
fn polygon(points: &[[f64; 2]], reversed: bool) -> Vec<(Point3, Point3)> {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p(points[i]), p(points[(i + 1) % n]));
            if reversed {
                (b, a)
            } else {
                (a, b)
            }
        })
        .collect()
}

/// The record's 24 input orders: the variant (none, outline or hole reversed) and the
/// list rotated by `turn`.
fn orders() -> Vec<(usize, usize, Vec<Curve>)> {
    let mut out = Vec::new();
    for variant in 0..3 {
        let mut lines = polygon(&OUTLINE, variant == 1);
        lines.extend(polygon(&HOLE, variant == 2));
        let n = lines.len();
        for turn in 0..n {
            let curves = (0..n)
                .map(|k| {
                    let (a, b) = lines[(k + turn) % n];
                    Curve::from(create_line_segment(a, b).unwrap())
                })
                .collect();
            out.push((variant, turn, curves));
        }
    }
    out
}

fn kernel(round: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.join_touching_loops = true;
    k.fixes.loops_go_on_round_the_vertex = round;
    k
}

/// The cap plane's loops: the outer loop counts, the loop counts and the signed area
/// the ordered curves enclose, or `None` when the plane is refused.
fn cap_loops(k: &Kernel, curves: &[Curve]) -> Option<(Vec<usize>, Vec<usize>, f64)> {
    let plane = Surface::create_plane_with_3d_curves(k, curves, 1e-6).ok()?;
    let mut area = 0.0;
    for (&c, &orient) in plane.ordered_curves.iter().zip(&plane.orients) {
        let e = curves[c].ends(k).unwrap();
        let (a, b) = if orient == OrientType::Same {
            (e.start, e.end)
        } else {
            (e.end, e.start)
        };
        area += 0.5 * (a.x * b.y - b.x * a.y);
    }
    Some((plane.outer_loop_counts, plane.loop_counts, area))
}

/// What the capped sweep built.
#[derive(Debug)]
struct Swept {
    faces: usize,
    /// Every edge has two faces on both sides.
    edges_manifold: bool,
    manifold_solid: bool,
    area: f64,
    volume: f64,
    check: Vec<String>,
}

fn sweep(k: &mut Kernel, curves: &[Curve]) -> Swept {
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut b = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true).unwrap();
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    Swept {
        faces: faces(&b, None).unwrap().len(),
        edges_manifold: edges(&b, None)
            .unwrap()
            .into_iter()
            .all(|e| is_manifold(&b, e).unwrap()),
        manifold_solid: is_manifold_solid(&b).unwrap(),
        area: brep_area(k, &mut b, 1e-6).unwrap(),
        volume: brep_volume(k, &mut b, 1e-6, &attributes).unwrap(),
        check: check_brep(k, &mut b, CheckLevel::Level2).unwrap().lines(),
    }
}

/// The plate less the hole: ten faces, a manifold solid of volume 118, the check clean.
fn is_plate(s: &Swept) -> bool {
    s.faces == 10
        && s.edges_manifold
        && s.manifold_solid
        && (s.area - area()).abs() < 1e-9
        && (s.volume - VOLUME).abs() < 1e-9
        && s.check.is_empty()
}

/// The defect WR-591 is about, pinned while the flag is opt-in, not a result to keep:
/// without the flag the cap plane is refused in 15 orders (the loop ordering ends on a
/// curve it already used, or runs round the hole the wrong way). Upstream's capped sweep
/// still returned `Ok`, the eight side faces as an open skin; the fork returns the cap's
/// failure (c8f6a31, the curve `CreateLinearSweep`'s `SER(
/// pBrep->CreatePlanarFacesWith3DCurves(m_pRegion, cr3DCurves, dTol, sStartFaces) )`).
/// In the other orders the plane is the one loop. This test goes when the flag becomes
/// the default.
#[test]
fn wr591_without_the_flag_the_capped_sweep_refuses() {
    assert!(!Kernel::default().fixes.loops_go_on_round_the_vertex);
    let mut refused = Vec::new();
    for (variant, turn, curves) in orders() {
        let mut k = kernel(false);
        match cap_loops(&k, &curves) {
            None => {
                refused.push((variant, turn));
                let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
                let e = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 2.0, true)
                    .expect_err("the cap is refused");
                assert_eq!(
                    e.sm_status(),
                    Some(1001),
                    "variant {variant}, turn {turn}: {e:?}"
                );
            }
            Some((outer, loops, enclosed)) => {
                assert_eq!(
                    (outer, loops),
                    (vec![1], vec![8]),
                    "variant {variant}, turn {turn}"
                );
                assert!((enclosed.abs() - 59.0).abs() < 1e-12, "{enclosed}");
            }
        }
    }
    assert_eq!(refused, REFUSED_OFF);
}

/// With the flag the cap plane is one loop of all eight lines, enclosing 59, in every
/// order, and the sweep closes: ten faces, every edge manifold, the area of the plate
/// less the hole.
#[test]
fn wr591_on_cap_plane_is_one_loop_in_every_order() {
    for (variant, turn, curves) in orders() {
        let mut k = kernel(true);
        let (outer, loops, enclosed) = cap_loops(&k, &curves)
            .unwrap_or_else(|| panic!("variant {variant}, turn {turn}: plane refused"));
        assert_eq!(
            (outer, loops),
            (vec![1], vec![8]),
            "variant {variant}, turn {turn}"
        );
        assert!(
            (enclosed.abs() - 59.0).abs() < 1e-12,
            "variant {variant}, turn {turn}: {enclosed}"
        );
        let s = sweep(&mut k, &curves);
        assert!(
            s.faces == 10 && s.edges_manifold && (s.area - area()).abs() < 1e-9,
            "variant {variant}, turn {turn}: {s:?}"
        );
    }
}

/// With the flag the 15 orders outside [`NO_SOLID_ROTATIONS`] give the plate less the
/// hole: a manifold solid of volume 118, the check clean.
#[test]
fn wr591_on_sweep_is_the_plate_less_the_hole() {
    let mut bad = Vec::new();
    for (variant, turn, curves) in orders() {
        if NO_SOLID_ROTATIONS.contains(&turn) {
            continue;
        }
        let s = sweep(&mut kernel(true), &curves);
        if !is_plate(&s) {
            bad.push((variant, turn, s));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// The plate less the hole in all 24 orders (#2506). Four side faces end on the vertical
/// line through the corner (0, 3), so the sweep's stitch finds four coincident lamina
/// edges there; it must glue each to a neighbour round the line, the two walls across
/// the outside and the two hole faces across the hole, or each wall to the hole face
/// next to it. The regression test of that fix: without it, in rotations 4, 6 and 7 of
/// every variant the stitch glues each wall to the far hole face: the two glued edges
/// cross, the faces' two sides join into one shell, and the closed body gets one void
/// region and no solid one (volume 0), though `check_brep` `Level2` passes it.
#[test]
fn wr591_on_sweep_is_the_plate_less_the_hole_in_every_order() {
    let mut bad = Vec::new();
    for (variant, turn, curves) in orders() {
        let s = sweep(&mut kernel(true), &curves);
        if !is_plate(&s) {
            bad.push((variant, turn, s));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
