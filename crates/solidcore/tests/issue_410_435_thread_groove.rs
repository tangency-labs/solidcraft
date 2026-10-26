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
//! The groove is a 60-degree trapezoid swept along a helix about the shaft's axis, a
//! pitch past each end of the shaft, as the SolidCraft rig's M33x3.5 (case 2193) and
//! M44x1 (case 2209) threads. `SmFace` test #34 is not ported, so the symptom measured is
//! its cause: how far each edge's end lies from its vertex, against the vertex tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::{create_helix, create_line_segment, HelixParams};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::sweep::api_brep::create_curve_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel};

/// An external thread: a shaft of radius `r` and length `len` minus a groove of pitch `p`,
/// `depth` deep, with a root flat of width `flat`.
fn threaded_shaft(k: &mut Kernel, p: f64, r: f64, len: f64, depth: f64, flat: f64) -> Brep {
    let shaft = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), r, len).unwrap();
    let z0 = -p;
    let height = len + 2.0 * p;
    let root = r - depth;
    let helix = create_helix(&HelixParams {
        origin: Point3::new(0.0, 0.0, z0),
        height,
        radius_start: root,
        radius_end: root,
        num_turns: height / p,
        right_handed: true,
        tolerance: 1e-4,
    })
    .unwrap();
    let helix = Curve::from(helix);
    let outer = r + 0.05;
    let half_root = flat / 2.0;
    let half_outer = half_root + (outer - root) * 30.0_f64.to_radians().tan();
    let corners = [
        Point3::new(root, 0.0, z0 - half_root),
        Point3::new(outer, 0.0, z0 - half_outer),
        Point3::new(outer, 0.0, z0 + half_outer),
        Point3::new(root, 0.0, z0 + half_root),
    ];
    let sides: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(corners[i], corners[(i + 1) % 4]).unwrap()))
        .collect();
    let profile: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    let options = SweepOptions {
        cap_ends: true,
        ..SweepOptions::default()
    };
    let groove = create_curve_sweep(k, &profile, Some(&helix), None, None, Some(&options)).unwrap();
    boolean_difference(k, shaft, groove.brep).unwrap()
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

/// M44x1 on 4 mm (SolidCraft case 2209): an edge ends 9.8e-6 from its vertex, over half the
/// vertex's tolerance, which SMLib's check reports; with the fix every edge ends within
/// 2e-6 of its vertex.
#[test]
fn issue_435_m44x1_edges_end_at_their_vertices() {
    let cut = |trim, extend| {
        cut(1.0, 22.0, 4.0, 0.61, 0.25, |k| {
            k.fixes.imprint_trims_edge_to_proto_vertex = trim;
            k.fixes.imprint_extends_edge_to_proto_vertex = extend;
        })
    };
    let (gap, tol) = cut(false, false);
    assert!(gap > tol / 2.0, "upstream: gap {gap:e}, tolerance {tol:e}");
    for (trim, extend) in [(true, false), (true, true)] {
        let (gap, tol) = cut(trim, extend);
        assert!(gap < 2e-6, "gap {gap:e}, tolerance {tol:e}");
    }
}

/// M33x3.5 on 14 mm (SolidCraft case 2193): the flank/end-face curves cross the rim at
/// 3.3 degrees.
#[test]
fn issue_435_m33x3_5_edges_end_at_their_vertices() {
    let cut = |trim| {
        cut(3.5, 16.5, 14.0, 2.15, 0.875, |k| {
            k.fixes.imprint_trims_edge_to_proto_vertex = trim;
            k.fixes.imprint_extends_edge_to_proto_vertex = trim;
        })
    };
    let (gap, tol) = cut(false);
    assert!(gap > tol / 2.0, "upstream: gap {gap:e}, tolerance {tol:e}");
    let (gap, tol) = cut(true);
    assert!(gap < 2e-6, "gap {gap:e}, tolerance {tol:e}");
}
