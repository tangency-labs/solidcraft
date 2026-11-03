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

// ---- #2486: chamfers as their definitions, the chord of order 0 -------------------------

use solidcore::blend::fillet_executive::api::chamfer_fillet;
use solidcore::math::Vec3;
use solidcore::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use solidcore::surface::procedural_blend::convert::{exact_rebuild, exact_to_record};
use solidcore::surface::procedural_blend::exact::ExactClass;
use solidcore::surface::procedural_blend::BlendRef;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;

/// The section generator and rounds (and chamfers) as their definitions.
fn definition_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_round_as_definition = true;
    k
}

/// The box [0, 20] x [0, 20] x [0, 10]'s top front edge, along x at y = 0, z = 10.
fn top_front_edge(b: &Brep) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let s = vertex::point(b, s.unwrap()).unwrap();
            let t = vertex::point(b, t.unwrap()).unwrap();
            [s, t]
                .iter()
                .all(|q| q.y.abs() < 1e-9 && (q.z - 10.0).abs() < 1e-9)
        })
        .unwrap()
}

/// The surfaces of `b`'s faces that are blends by their definitions.
fn definitions(b: &Brep) -> Vec<Surface> {
    b.face_list()
        .iter()
        .filter_map(|&f| {
            let s = b.face(f).unwrap().surface.as_ref()?;
            s.procedural_blend().map(|_| s.clone())
        })
        .collect()
}

/// Whether support 1 of the box's top front edge's chamfer is the top face (z = 10), else
/// the front face (y = 0): the blend edge use's face.
fn first_support_is_top(k: &Kernel, b: &mut Brep, e: EdgeId) -> bool {
    let u0 = edge::blend_edgeuse(k, b, e).unwrap().unwrap();
    let f = eu1::face(b, u0).unwrap().unwrap();
    let s = b.face(f).unwrap().surface.as_ref().unwrap();
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let p = s.evaluate_point(k, d.evaluate(0.5, 0.5)).unwrap();
    let q = s.evaluate_point(k, d.evaluate(0.2, 0.7)).unwrap();
    (p.z - 10.0).abs() < 1e-9 && (q.z - 10.0).abs() < 1e-9
}

/// The box's top front edge chamfered by `size` through `prepare_chamfers`: the body,
/// its kernel, and the setbacks (along the top face, along the front face).
fn chamfered_box(size: ChamferSize) -> (Kernel, Brep, [f64; 2]) {
    let mut k = definition_kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 10.0).unwrap();
    let e = top_front_edge(&b);
    let top_first = first_support_is_top(&k, &mut b, e);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers(&k, &[e], &[size]).unwrap();
        s.create_corners(&k, None).unwrap();
        for id in s.fillet_corners().unwrap() {
            s.set_corner_bevel(id, true).unwrap();
        }
        s.do_filleting(&mut k, None).unwrap();
    }
    b.refresh_cached_topology();
    let d = ChamferDefinition::new(size, 0.5 * std::f64::consts::PI)
        .unwrap()
        .setbacks();
    let setbacks = if top_first { d } else { [d[1], d[0]] };
    (k, b, setbacks)
}

/// The largest distance of a 9 x 9 grid of the chamfer face's surface from the plane
/// through the box's top front edge's rails, `a` along the top face and `b` along the
/// front face.
fn off_the_chamfer_plane(k: &Kernel, s: &Surface, [a, bb]: [f64; 2]) -> f64 {
    // in the y-z plane the rails are (a, 10) and (0, 10 - b): normal (b, -a)
    let n = Vec3::new(0.0, bb, -a).unitize().unwrap();
    let o = Point3::new(0.0, a, 10.0);
    let d = s.natural_uv_domain_with_nurb(k).unwrap();
    let mut off = 0.0f64;
    for i in 0..=8 {
        for j in 0..=8 {
            let p = s
                .evaluate_point(k, d.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0))
                .unwrap();
            off = off.max((p - o).dot(n).abs());
        }
    }
    off
}

/// The chord's definition on a chamfer face between planes: its exact round, the chord
/// class of order 0 at a bound of 0.
fn chord_of(s: &Surface) -> solidcore::surface::procedural_blend::exact::ExactRound {
    let blend = s.procedural_blend().unwrap();
    assert_eq!(blend.order(), 0);
    assert_eq!(blend.bound(), 0.0);
    let BlendRef::Exact { round, .. } = blend else {
        panic!("a chord between planes is exact: {blend:?}");
    };
    assert_eq!(round.class(), ExactClass::Chord);
    round.clone()
}

/// Each kind of chamfer between the box's planes keeps its definition, the chord (order
/// 0) of its sizes, exact (bound 0); its face is the chamfer's plane, every point on it to
/// rounding, and the body loses the prism `a b / 2 * 20`. The definition round trips
/// through its record and re-certifies.
#[test]
fn issue_2486_chamfers_between_planes_are_chords_by_their_definitions() {
    let kinds = [
        ChamferSize::Distances([1.5, 1.5]),
        ChamferSize::Distances([1.0, 2.0]),
        ChamferSize::DistanceAngle {
            distance: 2.0,
            angle: std::f64::consts::PI / 6.0,
        },
    ];
    for size in kinds {
        let (k, mut b, setbacks) = chamfered_box(size);
        let defs = definitions(&b);
        assert_eq!(defs.len(), 1, "{size:?}: one chamfer face");
        let s = &defs[0];
        let round = chord_of(s);
        let kept = round.chamfer().unwrap();
        assert_eq!(kept.size, size, "{size:?}");
        assert!((kept.wedge - 0.5 * std::f64::consts::PI).abs() < 1e-14);
        let off = off_the_chamfer_plane(&k, s, setbacks);
        assert!(off <= 1e-13, "{size:?}: {off}");
        let want = 4000.0 - setbacks[0] * setbacks[1] / 2.0 * 20.0;
        assert_close(volume(&k, &mut b), want);
        // the record round trips, and the net re-certifies
        let rec = exact_to_record(&k, s).unwrap();
        let back = exact_rebuild(&k, &rec).unwrap();
        assert_eq!(chord_of(&back).chamfer(), Some(kept));
        assert_eq!(exact_to_record(&k, &back).unwrap(), rec);
        // sizes that do not give its offsets are refused on rebuild
        let mut off_sizes = rec.clone();
        if let Some(c) = off_sizes.chamfer.as_mut() {
            c.wedge *= 0.9;
        }
        assert!(exact_rebuild(&k, &off_sizes).is_err(), "{size:?}");
    }
}

/// `SmApiChamferFillet`'s chord between the cube's planes: every chamfer face keeps the
/// chord 1 as its definition, exact, and the cube has its closed-form volume with bevelled
/// corners: twelve prisms of legs 1/sqrt 2 less the corners' overlaps.
#[test]
fn issue_2486_chord_chamfers_of_a_cube_are_chords_by_their_definitions() {
    let mut k = definition_kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    chamfer_fillet(&mut k, &mut b, 1.0).unwrap();
    b.refresh_cached_topology();
    let defs = definitions(&b);
    assert_eq!(defs.len(), 12);
    for s in &defs {
        let round = chord_of(s);
        assert_eq!(round.chamfer().unwrap().size, ChamferSize::Chord(1.0));
    }
    // twelve prisms of 20 x d^2 / 2, d = 1/sqrt 2, each corner giving back 3 d^3 / 4
    assert_close(volume(&k, &mut b), 7940.0 + 1.5 * std::f64::consts::SQRT_2);
}
