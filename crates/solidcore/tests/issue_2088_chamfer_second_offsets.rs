//! #2088 (the fork's tangency-labs/smlib `f653ac9`, `pOptSecondRadii` on
//! `SmFilletExecutive::SetFilletParameters`): a chamfer with a second offset per edge goes
//! through `SetFilletParameters`, so with `fillet_corner_per_vertex_fan` (WR-665) it reaches
//! the vertex-fan split as a fillet does.
//!
//! The part is WR-665's up-down joined squares: the cube [-10, 10]^3 less the octants
//! [0, 10]^3 and [-10, 0]^3 (V = 6000). The two concave edges on the line x = y = 0
//! (z -10..0 and z 0..10) meet at the origin, a vertex of two fans of faces. A linear
//! constant-radius blend with offsets a and b on a right-angled concave edge of length 10
//! adds a triangular prism of legs a and b: `a * b / 2 * 10`. So both edges chamfered
//! 1 x 1 give 6010 (SMLib at the fork's 26cc451: 6009.999999999998) and 1 x 2 give 6020.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel, Point3, SolidError};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    b.refresh_cached_topology();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

/// The cube [-10, 10]^3 less the octants [0, 10]^3 and [-10, 0]^3.
fn up_down_squares(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let cube = create_box(k, p(-10.0, -10.0, -10.0), 20.0, 20.0, 20.0).unwrap();
    let up = create_box(k, p(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let down = create_box(k, p(-10.0, -10.0, -10.0), 10.0, 10.0, 10.0).unwrap();
    let once = boolean_difference(k, cube, up).unwrap();
    boolean_difference(k, once, down).unwrap()
}

/// The two edges on the line x = y = 0, sorted.
fn axis_edges(b: &Brep) -> Vec<EdgeId> {
    let on_axis = |q: Point3| q.x.abs() < 1e-9 && q.y.abs() < 1e-9;
    let mut out: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let s = vertex::point(b, s.unwrap()).unwrap();
            let t = vertex::point(b, t.unwrap()).unwrap();
            on_axis(s) && on_axis(t) && s.distance(t) > 1e-9
        })
        .collect();
    out.sort();
    out
}

/// Chamfers the axis edges `which` with first offset 1 and second offset `second`
/// through `SetFilletParameters`' second radii; `Err` is the fillet error code.
fn chamfer(per_fan: bool, which: &[usize], second: f64) -> Result<f64, i32> {
    let mut k = Kernel::default();
    k.fixes.fillet_corner_per_vertex_fan = per_fan;
    let mut b = up_down_squares(&mut k);
    let all = axis_edges(&b);
    assert_eq!(all.len(), 2);
    let ids: Vec<EdgeId> = which.iter().map(|&i| all[i]).collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius_with_second(
        &mut k,
        &ids,
        &[1.0],
        &vec![second; ids.len()],
        &[FilletSurfaceGeneratorType::Linear],
        1,
        1.0,
    )
    .unwrap();
    if s.do_fillet(&mut k).is_err() {
        return Err(s.error_info().code);
    }
    drop(s);
    Ok(volume(&k, &mut b))
}

fn assert_close(v: f64, want: f64) {
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
}

/// The issue's case: both axis edges chamfered 1 x 1. Without the flag the corner at the
/// origin fails (`SM_FILERR_MAKE_TOPOLOGY`, code 3, as #2088 reports); with it each fan
/// is its own corner.
#[test]
fn issue_2088_joined_squares_chamfer_1x1() {
    assert_eq!(chamfer(false, &[0, 1], 1.0), Err(3));
    assert_close(chamfer(true, &[0, 1], 1.0).unwrap(), 6010.0);
}

/// Unequal setbacks, 1 x 2: each prism is 1 * 2 / 2 * 10.
#[test]
fn issue_2088_joined_squares_chamfer_1x2() {
    assert_eq!(chamfer(false, &[0, 1], 2.0), Err(3));
    assert_close(chamfer(true, &[0, 1], 2.0).unwrap(), 6020.0);
}

/// WR-665's one-edge variant as a two-distance chamfer: one fan blended.
#[test]
fn issue_2088_one_axis_edge_chamfer() {
    for which in [0, 1] {
        assert_close(chamfer(true, &[which], 1.0).unwrap(), 6005.0);
        assert_close(chamfer(true, &[which], 2.0).unwrap(), 6010.0);
    }
}

/// The fork's size check: a second-radius list that is not one per edge is invalid input.
#[test]
fn issue_2088_second_radii_size_mismatch() {
    let mut k = Kernel::default();
    let mut b = up_down_squares(&mut k);
    let ids = axis_edges(&b);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    let err = s
        .prepare_per_edge_constant_radius_with_second(
            &mut k,
            &ids,
            &[1.0],
            &[1.0],
            &[FilletSurfaceGeneratorType::Linear],
            1,
            1.0,
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            SolidError::Upstream {
                code: solidcore::error::SM_ERR_INVALID_INPUT,
                ..
            }
        ),
        "{err:?}"
    );
}
