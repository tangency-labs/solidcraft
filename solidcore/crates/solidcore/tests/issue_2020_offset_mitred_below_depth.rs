//! #2020 (WR-615): mitred rounds on a moved face, the top moved below the rounds' depth.
//! A 40 x 30 x 10 box has its four top edges rounded r 3 (mitred at the top corners) and
//! its top offset by `d` (`OffsetFacesExecutive::offset_faces`): the top goes to
//! z = 10 + d and the rounds' axes to z = 7 + d. Below d = -7 the axes go under the
//! floor, and the rounds meet it.
//!
//! The volume is analytic: each section at height z is the 34 x 24 top moved out by
//! `w(z) = sqrt(9 - (z - z_axis)^2)` (3 below the axis), its corners mitred, so
//! V = integral of (34 + 2w)(24 + 2w) dz over 0..10 + d.
//!
//! With SolidCraft's flags (183 `UpstreamFixes`, among them the fork's cliff-overflow
//! and side-edge run-on fixes), every depth builds. Below the floor the rails meet the
//! side edges' lines past the moved side edges' lower ends; each rail's search runs the
//! side edge on along its line as far as the rail's base surface's domain is extended
//! (#2769). Bounded by the edge's own length alone (where the trace's reach was not set,
//! a face an offset had widened), one rail at a corner was found and the other not, and
//! the corner's bevel adjustment read the unsolved rail's crossing as parameter 0 (d =
//! -9.5, a put-back refused); at -9 the rail met the run-on exactly at the end of its
//! widened interval, and the put-back was refused as the fork's since 779cb9b. -9 now
//! builds the fork's 2fc375f body (6 faces), with the closed form's volume to 8e-12: the
//! shrink keeps the rounds' exact trims (WR-700), where the fork's 1006.320632128887
//! through SolidCraft's capi is 4.8e-9 over it. With the flags off, the put-back fails at -7.5 (a corner edge) and at -9
//! (a rollover). Each such failure is now a typed refusal,
//! `OffsetFacesRefusal::NotPutBack`, where it had none.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::offset::offset_faces::{OffsetFacesExecutive, OffsetFacesRefusal};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::plane::Plane;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

/// The fork's volume at d = -9 before 779cb9b (2fc375f, `smlib_offset_faces`): 6 faces.
const FORK_VOLUME_D9: f64 = 1006.320632128887;

/// The edge whose midpoint is `p`.
fn edge_through(b: &Brep, p: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let a = vertex::point(b, s.unwrap()).unwrap();
            let c = vertex::point(b, t.unwrap()).unwrap();
            (0.5 * (a.x + c.x) - p.x).abs() < 1e-6
                && (0.5 * (a.y + c.y) - p.y).abs() < 1e-6
                && (0.5 * (a.z + c.z) - p.z).abs() < 1e-6
        })
        .unwrap()
}

/// The plane face whose vertices are all at height `z`.
fn top(b: &Brep, z: f64) -> FaceId {
    *b.face_list()
        .iter()
        .find(|&&f| {
            face2::surface(b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
                && solidcore::topology::face::p4::edges(b, f, None, false, None)
                    .unwrap()
                    .into_iter()
                    .all(|e| {
                        let (s, _) = edge::start_end_vertices(b, e).unwrap();
                        (vertex::point(b, s.unwrap()).unwrap().z - z).abs() < 1e-6
                    })
        })
        .unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The analytic volume after the top moved by `d` (see the module docs).
fn analytic(d: f64) -> f64 {
    let top = 10.0 + d;
    let axis = top - 3.0;
    // below the axis the walls are the box's: w = 3
    let below = axis.max(0.0);
    let mut v = 40.0 * 30.0 * below;
    // over the axis: 816 L + 116 integral(w) + 4 integral(w^2), u = z - axis
    let (u0, u1) = (below - axis, top - axis);
    let w_int = |u: f64| 0.5 * (u * (9.0 - u * u).max(0.0).sqrt() + 9.0 * (u / 3.0).asin());
    let w2_int = |u: f64| 9.0 * u - u * u * u / 3.0;
    v += 816.0 * (u1 - u0) + 116.0 * (w_int(u1) - w_int(u0)) + 4.0 * (w2_int(u1) - w2_int(u0));
    v
}

/// The rounded box with its top moved by `d`: the B-rep, the executive and the result.
fn moved(k: &mut Kernel, d: f64) -> (Brep, OffsetFacesExecutive, solidcore::Result<()>) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let edges: Vec<EdgeId> = [
        Point3::new(20.0, 0.0, 10.0),
        Point3::new(20.0, 30.0, 10.0),
        Point3::new(0.0, 15.0, 10.0),
        Point3::new(40.0, 15.0, 10.0),
    ]
    .into_iter()
    .map(|p| edge_through(&b, p))
    .collect();
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    let t = top(&b, 10.0);
    let mut ex = OffsetFacesExecutive::new();
    let r = ex.offset_faces(k, &mut b, &[t], d);
    (b, ex, r)
}

#[test]
fn analytic_volume_matches_the_issue() {
    // the issue's table and its -9 derivation (816 + 116 * 1.54873 + 4 * 8/3)
    for (d, v) in [
        (-6.9, 3459.955684),
        (-7.0, 3339.955684),
        (-7.5, 2740.931296),
        (-8.0, 2147.845376),
        (-9.0, 1006.320628),
        (-9.5, 476.105890),
    ] {
        assert!(
            (analytic(d) - v).abs() < 1e-5,
            "d {d}: {} vs {v}",
            analytic(d)
        );
    }
}

#[test]
fn solidcraft_flags_build_the_depths_below_the_rounds() {
    for (d, faces) in [(-6.9, 10), (-7.0, 6), (-7.5, 6), (-8.0, 6), (-9.5, 6)] {
        let k = &mut Kernel::default();
        solidcraft_flags(k);
        let (mut b, ex, r) = moved(k, d);
        assert!(
            r.is_ok(),
            "d {d}: {r:?}: {:?} {}",
            ex.refusal(),
            ex.reason()
        );
        assert_eq!(ex.refusal(), OffsetFacesRefusal::None, "d {d}");
        assert!(brep_is_manifold_solid(&b).unwrap(), "d {d}");
        assert_eq!(b.face_list().len(), faces, "d {d}");
        let v = volume(k, &mut b);
        let want = analytic(d);
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "d {d}: V {v} vs analytic {want}"
        );
    }
}

/// At -9 the rails meet the side edges' lines 3 below the moved side edges' lower ends
/// (z = -2). With the run-on bounded by the base surface's domain as the solve extends it
/// (#2769), each rail's search reaches that point, not only the one whose solver's trace
/// reach was set: it builds, as the fork's 2fc375f did.
#[test]
fn solidcraft_flags_build_minus_nine_as_the_fork() {
    let k = &mut Kernel::default();
    solidcraft_flags(k);
    let (mut b, ex, r) = moved(k, -9.0);
    assert!(r.is_ok(), "{r:?}: {:?} {}", ex.refusal(), ex.reason());
    assert_eq!(ex.refusal(), OffsetFacesRefusal::None);
    assert!(brep_is_manifold_solid(&b).unwrap());
    assert_eq!(b.face_list().len(), 6);
    let v = volume(k, &mut b);
    assert!(
        (v - analytic(-9.0)).abs() <= 1e-10 * analytic(-9.0),
        "V {v}"
    );
    assert!((v - FORK_VOLUME_D9).abs() <= 1e-8 * FORK_VOLUME_D9, "V {v}");
}

#[test]
fn flags_off_build_the_depths_above_the_floor() {
    for (d, faces) in [(-6.9, 10), (-7.0, 6)] {
        let k = &mut Kernel::default();
        let (mut b, ex, r) = moved(k, d);
        assert!(
            r.is_ok(),
            "d {d}: {r:?}: {:?} {}",
            ex.refusal(),
            ex.reason()
        );
        assert_eq!(b.face_list().len(), faces, "d {d}");
        let v = volume(k, &mut b);
        let want = analytic(d);
        assert!(
            (v - want).abs() <= 1e-6 * want,
            "d {d}: V {v} vs analytic {want}"
        );
    }
}

/// SolidCraft's `UpstreamFixes` preset (183 flags).
fn solidcraft_flags(k: &mut Kernel) {
    k.fixes.analytic_copy_keeps_parameterization = true;
    k.fixes.classify_edge_interval = true;
    k.fixes.curve_contacts_at_curve_ends = true;
    k.fixes.curve_loop_normal_from_area = true;
    k.fixes.face_tight_box_searches_trim = true;
    k.fixes.refine_cc_range_ends = true;
    k.fixes.simple_coincidence_crossing_is_point = true;
}
