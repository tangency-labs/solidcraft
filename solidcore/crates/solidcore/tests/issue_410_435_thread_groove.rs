//! tangency-labs/solidcore#435 and #410 (WR-452, WR-453): a cylinder minus a helical thread
//! groove. Where the groove's flank meets an end face the flank/plane curve crosses the
//! shaft's rim at a grazing angle, so the proto vertex there lies a few tolerances off the
//! curve's end, which the edge then ran past (`SmFace` test #34, "Face has partially
//! Coincident Edges") or stopped short of (a vertex gap over its tolerance). Upstream
//! `SmEdgeDefinition::Imprint` makes the edge over the curve's whole interval anyway. The
//! opt-in `Kernel::fixes.imprint_trims_edge_to_proto_vertex` moves the interval's ends to
//! the curve's point nearest each proto vertex, and
//! `Kernel::fixes.imprint_extends_edge_to_proto_vertex` continues a curve that stops short
//! of its vertex to it.
//!
//! The groove is a 60-degree trapezoid under a screw motion about the shaft's axis (sections
//! in meridian planes, sixteen a turn, skinned cubically and capped), run a pitch past each
//! end of the shaft, as the SolidCraft rig's M33x3.5 (case 2193) and M44x1 (case 2209)
//! threads; the construction is that of `issue_435_screw_thread_groove.rs` (#1626). (`create_curve_sweep`
//! along a helix is not a thread: its frame twists about 21 degrees a turn.) `SmFace`
//! test #34 is not measured here; the symptom measured is its cause: how far each edge's
//! end lies from its vertex, against the vertex tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use std::f64::consts::PI;

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel};

/// An external thread: a shaft of radius `r` and length `len` minus a groove of pitch `p`,
/// `depth` deep, with a root flat of width `flat`.
fn threaded_shaft(k: &mut Kernel, p: f64, r: f64, len: f64, depth: f64, flat: f64) -> Brep {
    let shaft = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), r, len).unwrap();
    let root = r - depth;
    let outer = r + 0.05;
    let half_root = flat / 2.0;
    let half_outer = half_root + (outer - root) * 30.0_f64.to_radians().tan();
    let section = [
        (root, -half_root),
        (outer, -half_outer),
        (outer, half_outer),
        (root, half_root),
    ];
    let groove = screw_groove(k, &section, p, -p, (len + 2.0 * p) / p, 16);
    boolean_difference(k, shaft, groove).unwrap()
}

/// The groove: the `section`'s (radius, axial offset) corners moved by a screw motion of `p`
/// a turn, from `z0`, over `turns` turns, `per_turn` sections a turn, skinned with degree 3
/// and capped.
fn screw_groove(
    k: &mut Kernel,
    section: &[(f64, f64)],
    p: f64,
    z0: f64,
    turns: f64,
    per_turn: usize,
) -> Brep {
    let n = (turns * per_turn as f64).round() as usize;
    let mut curves = Vec::new();
    for i in 0..=n {
        let turn = i as f64 / per_turn as f64;
        let (s, c) = (2.0 * PI * turn).sin_cos();
        let pts: Vec<Point3> = section
            .iter()
            .map(|&(r, z)| Point3::new(r * c, r * s, z0 + z + p * turn))
            .collect();
        for j in 0..pts.len() {
            let side = create_line_segment(pts[j], pts[(j + 1) % pts.len()]).unwrap();
            curves.push(Curve::from(side));
        }
    }
    let counts = vec![section.len(); n + 1];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_skin_primitive(k, &counts, &refs, 3, true)
        .unwrap()
        .brep
}

/// The largest distance of an edge's end from its vertex, and that vertex's tolerance.
fn worst_vertex_gap(k: &Kernel, b: &Brep) -> (f64, f64) {
    let (mut worst, mut tol) = (0.0_f64, 0.0);
    for e in p1::edges(b, None).unwrap() {
        let Some(curve) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let interval = edge::interval(b, e).unwrap();
        let ends = [
            (edge::start_vertex(b, e).unwrap(), interval.min()),
            (edge::end_vertex(b, e).unwrap(), interval.max()),
        ];
        for (v, t) in ends {
            let Some(v) = v else { continue };
            let gap = curve
                .evaluate_point(k, t)
                .unwrap()
                .distance(vertex::point(b, v).unwrap());
            if gap > worst {
                (worst, tol) = (gap, vertex::tolerance(b, v).unwrap());
            }
        }
    }
    (worst, tol)
}

/// The worst gap of the cut with the fixes set as `set` says.
fn cut(p: f64, r: f64, len: f64, depth: f64, flat: f64, set: impl Fn(&mut Kernel)) -> (f64, f64) {
    let mut k = Kernel::default();
    set(&mut k);
    let b = threaded_shaft(&mut k, p, r, len, depth, flat);
    worst_vertex_gap(&k, &b)
}

#[test]
fn issue_435_edge_end_fixes_are_off_by_default() {
    let k = Kernel::default();
    assert!(!k.fixes.imprint_trims_edge_to_proto_vertex);
    assert!(!k.fixes.imprint_extends_edge_to_proto_vertex);
}

/// M44x1 on 4 mm (SolidCraft case 2209). On the helix-swept groove an edge ended 9.8e-6
/// from its vertex; on the screw-motion groove the worst edge ends 3.54e-6 from its vertex,
/// under half the vertex's tolerance, as the C++ reference gives on the same body (which
/// passes SMLib's check there). The fixes leave that edge as it is.
#[test]
fn issue_435_m44x1_edges_end_at_their_vertices() {
    let cut = |trim, extend| {
        cut(1.0, 22.0, 4.0, 0.61, 0.25, |k| {
            k.fixes.imprint_trims_edge_to_proto_vertex = trim;
            k.fixes.imprint_extends_edge_to_proto_vertex = extend;
        })
    };
    for (trim, extend) in [(false, false), (true, false), (true, true)] {
        let (gap, tol) = cut(trim, extend);
        assert!(
            gap < tol / 2.0 && (gap - 3.5414e-6).abs() < 1e-9,
            "trim {trim}, extend {extend}: gap {gap:e}, tolerance {tol:e}"
        );
    }
}

/// M33x3.5 on 14 mm (SolidCraft case 2193): the flank/end-face curves cross the rim at a
/// grazing angle and an edge ends 1.52e-5 from its vertex, over the vertex's tolerance (the
/// C++ reference gives 1.52342e-5 on the same body and fails `SmBrep` test #0 and `SmFace`
/// test #34 twice each); with the fixes every edge ends within 2e-6 of its vertex.
#[test]
fn issue_435_m33x3_5_edges_end_at_their_vertices() {
    let cut = |trim| {
        cut(3.5, 16.5, 14.0, 2.15, 0.875, |k| {
            k.fixes.imprint_trims_edge_to_proto_vertex = trim;
            k.fixes.imprint_extends_edge_to_proto_vertex = trim;
        })
    };
    let (gap, tol) = cut(false);
    assert!(gap > tol, "upstream: gap {gap:e}, tolerance {tol:e}");
    let (gap, tol) = cut(true);
    assert!(gap < 2e-6, "gap {gap:e}, tolerance {tol:e}");
}
