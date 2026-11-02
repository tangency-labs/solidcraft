//! WR-533 (the fork's tangency-labs/smlib@f11d0ac, SolidCraft's `boolean_regression`
//! `lip_swept_flush_round_a_corner_joins`) through a boolean (#1954): a 50 x 40 x 10 block
//! with its corners rounded r 4.5, and a 1 x 1 lip swept along half its top rim, flush with
//! it, round two corners, united. This is the fork's capi test `lip_swept_flush_round_a_corner`
//! built with the same calls (`smlib_extrude` is a linear sweep; `smlib_sweep` joins the path
//! into one B-spline and sweeps the square along it with the profile kept where it lies).
//!
//! The sweep's walls are B-splines approximating the swept square; its outer bottom edge lies
//! on the block's rim arc within tolerance and crosses it twice, 0.05 apart, at the arc's
//! middle. The union splits the rim arc there. Upstream's `UpdateEdges` moves the coincident
//! edge definition along the whole arc to the piece under its midpoint, the 0.05 piece, whose
//! vertices are not its ends, and the imprint finds no vertex for them. With
//! `Kernel::fixes.boolean_splits_definition_across_split` the definition is split with its
//! edge, and the union is the block and the lip side by side.
//!
//! On a default kernel the port refuses the union in `make_faceuse` (1001) where the C++
//! returned 1006. With `proto_vertex_miss_is_false` (WR-73) also on, the union succeeds with
//! or without this flag, with the same body, so the test keeps every other flag off.

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
use solidcore::error::SolidError;
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

#[test]
fn wr_533_lip_swept_flush_round_a_corner_joins_only_with_the_fix() {
    let want = closed_form();
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.boolean_splits_definition_across_split = fix;
        let a = block(&mut k);
        let b = lip(&mut k);
        let joined = boolean_union(&mut k, a, b);
        if !fix {
            // upstream: the definition across the split point lands on the 0.05 piece, and
            // the faces round it cannot be closed
            assert!(
                matches!(
                    joined,
                    Err(SolidError::Upstream {
                        operation: "boolean::merge::p2::make_faceuse",
                        code: 1001,
                        ..
                    })
                ),
                "{:?}",
                joined.map(|r| r.face_list().len())
            );
            continue;
        }
        let mut r = joined.unwrap();
        let got = volume(&k, &mut r);
        let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
        eprintln!("volume {got}, the block and the lip {want}");
        assert!(check.is_valid(), "{:?}", check.lines());
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "volume {got}, the block and the lip {want}"
        );
    }
}

/// The fix's observable at the topology level (#1954): with the definition split with its
/// edge, the union is a body of the block's and the lip's faces, edges and vertices. Off,
/// the union produces no body to count (the test above pins its refusal). Pins the exact
/// counts, so a result that agrees in volume but leaves the rim arc and the lip's edge
/// joined differently still fails.
#[test]
fn wr_533_lip_swept_flush_round_a_corner_has_the_block_and_lip_topology() {
    let mut k = Kernel::default();
    k.fixes.boolean_splits_definition_across_split = true;
    let a = block(&mut k);
    let b = lip(&mut k);
    let mut r = boolean_union(&mut k, a, b).unwrap();
    let counts = (
        r.face_list().len(),
        r.edge_list().len(),
        r.vertex_list().len(),
    );
    let got = volume(&k, &mut r);
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    eprintln!("faces, edges, vertices {counts:?}, volume {got}");
    assert_eq!(counts, (22, 50, 30));
    assert!(
        (got - closed_form()).abs() <= 1e-6 * closed_form(),
        "volume {got}, the block and the lip {}",
        closed_form()
    );
    assert!(check.is_valid(), "{:?}", check.lines());
    assert_eq!(check.assert_valid, Some(true));
}
