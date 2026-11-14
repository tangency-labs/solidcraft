//! WR-73 and #1954: a proto-vertex miss returns `SM_SUCCESS` (read as TRUE), so the
//! boolean's proto-vertex cleanup stops at the first proto edge and never tests the
//! remaining ones. `RemoveCoincidentNoEdgeProtoVertex` (SmProtoTopology.cpp:6925) is
//! reached only from that cleanup (`boolean::topology_intersector::p2`), so the public
//! union is where the guard's outcome shows.
//!
//! The body is the fork's capi `lip_swept_flush_round_a_corner` case, built with the same
//! public calls: a 50 x 40 x 10 block with r 4.5 corners (a linear sweep of its rim), and
//! a 1 x 1 lip swept along half its top rim, flush with it, round two corners. The
//! sweep's outer bottom edge crosses the block's rim arc twice, 0.05 apart. In the
//! boolean's cleanup the proto vertices lie off the first proto edge's curve but inside
//! later ones.
//!
//! With every other `Kernel::fixes` flag default (the two outcomes below were those of the
//! fitted sweep; a sweep along lines and arcs is now built to its definition, flush with the
//! rim, so the second crossing is gone and the union builds with the flag off as well):
//! - off (upstream): every miss returns TRUE, the cleanup breaks at the first proto edge,
//!   and the union is refused in `boolean::merge::p2::make_faceuse` with 1001;
//! - on: a miss returns FALSE, the cleanup goes on to the remaining proto edges, removes
//!   the coincident edgeless vertices, and the union is the block and the lip, 22 faces,
//!   50 edges, 30 vertices.
//!
//! Measured twice with `cargo +1.98.0 test -p solidcore --test
//! wr73_proto_vertex_miss_is_false`; both runs identical. The record is
//! `plan/upstream-issues/WR-73--addendum-2026-11-02-public-boolean-reachability.md`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::sweep::api_brep::{create_curve_sweep, create_linear_sweep};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The line from `a` to `b`.
fn segment(a: Point3, b: Point3) -> Line {
    Line::create_line_segment(3, a, b, None).unwrap()
}

/// The line from `a` to `b` (x, y) at height `z`.
fn line(a: (f64, f64), b: (f64, f64), z: f64) -> Line {
    segment(Point3::new(a.0, a.1, z), Point3::new(b.0, b.1, z))
}

/// The arc of radius `r` round `c` at height `z`, about +z (`nz` 1) or -z (`nz` -1), from
/// angle `a0` to `a1` (radians from +x, counter-clockwise about its axis), its parameter
/// starting at `a0`, as the capi builds it.
fn arc(k: &Kernel, c: (f64, f64), z: f64, nz: f64, r: f64, a0: f64, a1: f64) -> Ellipse {
    // the capi's x0 = ref cos a0 + (axis x ref) sin a0 and y0 = axis x x0, with ref +x
    let x0 = Vec3::new(a0.cos(), nz * a0.sin(), 0.0);
    let y0 = Vec3::new(-a0.sin(), nz * a0.cos(), 0.0);
    let frame = Axis2Placement::new(Point3::new(c.0, c.1, z), x0, y0);
    let degrees = Extent1d::from_min_max(0.0, (a1 - a0).to_degrees());
    Ellipse::create_canonical_circle(k, &frame, r, Some(&degrees), None).unwrap()
}

/// The block: its rim (lines and r 4.5 corner arcs) at z = 0 extruded 10 up.
fn block(k: &mut Kernel) -> Brep {
    let rim = [
        Curve::from(line((20.5, -20.0), (-20.5, -20.0), 0.0)),
        Curve::from(arc(k, (-20.5, -15.5), 0.0, 1.0, 4.5, PI, 1.5 * PI)),
        Curve::from(line((-25.0, -15.5), (-25.0, 15.5), 0.0)),
        Curve::from(arc(k, (-20.5, 15.5), 0.0, 1.0, 4.5, 0.5 * PI, PI)),
        Curve::from(line((-20.5, 20.0), (20.5, 20.0), 0.0)),
        Curve::from(arc(k, (20.5, 15.5), 0.0, 1.0, 4.5, 0.0, 0.5 * PI)),
        Curve::from(line((25.0, 15.5), (25.0, -15.5), 0.0)),
        Curve::from(arc(k, (20.5, -15.5), 0.0, 1.0, 4.5, 1.5 * PI, 2.0 * PI)),
    ];
    let refs: Vec<Option<&Curve>> = rim.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// The lip: the 1 x 1 square in the plane x = 20.5 from (y, z) = (-20, 10) to (-19, 11),
/// swept along the top rim from (20.5, -20) round two corners to (20.5, 20), the path
/// joined into one B-spline and the square kept where it lies.
fn lip(k: &mut Kernel) -> Brep {
    let corners = [(-20.0, 10.0), (-19.0, 10.0), (-19.0, 11.0), (-20.0, 11.0)];
    let square: Vec<Curve> = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(segment(
                Point3::new(20.5, a.0, a.1),
                Point3::new(20.5, b.0, b.1),
            ))
        })
        .collect();
    let lines = [
        line((20.5, -20.0), (-20.5, -20.0), 10.0),
        line((-25.0, -15.5), (-25.0, 15.5), 10.0),
        line((-20.5, 20.0), (20.5, 20.0), 10.0),
    ];
    let arcs = [
        arc(k, (-20.5, -15.5), 10.0, -1.0, 4.5, 0.5 * PI, PI),
        arc(k, (-20.5, 15.5), 10.0, -1.0, 4.5, PI, 1.5 * PI),
    ];
    let mut parts = [
        lines[0].base_with_nurb().unwrap().into_owned(),
        arcs[0].base().clone(),
        lines[1].base_with_nurb().unwrap().into_owned(),
        arcs[1].base().clone(),
        lines[2].base_with_nurb().unwrap().into_owned(),
    ];
    let path = Curve::from(BSplineCurve::create_by_joining(&mut parts, None).unwrap());
    let refs: Vec<Option<&Curve>> = square.iter().map(Some).collect();
    let options = SweepOptions {
        cap_ends: true,
        profile_at_path_start: true,
        ..SweepOptions::default()
    };
    create_curve_sweep(k, &refs, Some(&path), None, None, Some(&options))
        .unwrap()
        .brep
}

/// The volume of `b` at a 1e-7 absolute integration tolerance.
fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The lip sits on the block, so the union is the sum: the block
/// (50 x 40 - (4 - pi) 4.5^2) 10 and the lip 41 + 31 + 41 + 2 pi (4.5^2 - 3.5^2) / 4.
fn closed_form() -> f64 {
    let block = (2000.0 - (4.0 - PI) * 4.5 * 4.5) * 10.0;
    let lip = 41.0 + 31.0 + 41.0 + 2.0 * PI * (4.5 * 4.5 - 3.5 * 3.5) / 4.0;
    block + lip
}

/// The proto-vertex miss decides the union: with the flag off the first miss ends the
/// cleanup at the first proto edge and the union is refused; with it on the cleanup tests
/// every proto edge, removes the coincident edgeless vertices and joins the bodies.
/// It pins the exact face, edge and vertex counts, the volume and the check, so a body that
/// merely looks right (same volume, a different rim split) still fails.
#[test]
fn wr73_proto_vertex_miss_is_false_lip_union_builds() {
    // The miss is FALSE (the default), so the caller goes on to the other proto edges.
    let mut k = Kernel::default();
    assert!(k.fixes.proto_vertex_miss_is_false);
    let a = block(&mut k);
    let b = lip(&mut k);
    let mut joined = boolean_union(&mut k, a, b).unwrap();
    let counts = (
        joined.face_list().len(),
        joined.edge_list().len(),
        joined.vertex_list().len(),
    );
    assert_eq!(counts, (22, 50, 30));
    let got = volume(&k, &mut joined);
    let want = closed_form();
    assert!(
        (got - want).abs() <= 1e-9 * want,
        "volume {got}, the block and the lip {want}"
    );
    let check = check_brep(&mut k, &mut joined, CheckLevel::Level2).unwrap();
    assert_eq!(check.assert_valid, Some(true));
    assert!(check.lines().is_empty(), "{:?}", check.lines());
}
