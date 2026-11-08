//! tangency-labs/solidcore#420 (WR-421): a chamfer as wide as the corner rounds it runs
//! round. `CalcTorusFilletGeom` only handled a circular section, so a linear one went to
//! the offset trace, which collapses to a point where the neighbouring round's offset
//! reaches its axis. `fixes.fillet_chamfer_cone_on_arc` revolves the linear section into a
//! cone. The case is the fork's capi test: a 40 x 40 x 20 plate with its vertical edges
//! rounded R1, its bottom loop chamfered 1.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The midpoint of edge `e`.
fn midpoint(k: &Kernel, b: &Brep, e: EdgeId) -> Point3 {
    let ivl = edge::interval(b, e).unwrap();
    let curve = edge::curve(b, e).unwrap().unwrap();
    curve
        .evaluate(k, 0.5 * (ivl.min() + ivl.max()), 0, true, false)
        .unwrap()[0]
}

/// The edges whose midpoint satisfies `keep`.
fn edges_where(k: &Kernel, b: &Brep, keep: impl Fn(Point3) -> bool) -> Vec<EdgeId> {
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    edges
        .into_iter()
        .filter(|&e| keep(midpoint(k, b, e)))
        .collect()
}

/// The plate's volume after its corners are rounded, after its bottom loop is chamfered,
/// and the chamfer's result.
fn chamfered_plate(fix: bool) -> (f64, f64, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = fix;
    let mut plate = create_box(&mut k, Point3::new(-20.0, -20.0, 0.0), 40.0, 40.0, 20.0).unwrap();
    let corners = edges_where(&k, &plate, |m| (m.z - 10.0).abs() < 1e-6);
    fillet_edges(&mut k, &mut plate, &corners, 1.0, 1, 1, 1.0).unwrap();
    let rounded = volume(&k, &mut plate);
    let bottom = edges_where(&k, &plate, |m| m.z.abs() < 1e-6);
    let r = fillet_edges(&mut k, &mut plate, &bottom, 1.0, 0, 1, 1.0);
    (rounded, volume(&k, &mut plate), r)
}

#[test]
fn the_chamfer_runs_round_the_corner_rounds_with_the_fix() {
    // each straight side loses a 1 x 1 triangle along 38, each corner the triangle turned
    // a quarter about the axis: (pi / 2) / 3
    let removed = 4.0 * 38.0 * 0.5 + 4.0 * (PI / 2.0) / 3.0;

    let (rounded, after, r) = chamfered_plate(true);
    r.unwrap();
    assert!(
        (after - (rounded - removed)).abs() < 1e-7 * rounded,
        "{rounded} {after}"
    );

    // upstream traces the offset intersection, which collapses to a point, and fails
    let (_, _, r) = chamfered_plate(false);
    assert!(r.is_err(), "upstream should fail: {r:?}");
}

// ---- #2486: a chamfer on a cylinder against a plane by its definition ---------------------

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_executive::{FitKind, NoRoomReason};
use solidcore::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use solidcore::surface::procedural_blend::convert::{rebuild, to_record};
use solidcore::surface::procedural_blend::{BlendRef, ProceduralBlend};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::SolidError;

/// The section generator and rounds (and chamfers) as their definitions.
fn definition_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_round_as_definition = true;
    k
}

/// A cylinder of radius 5 and height 10 on the origin, its top rim chamfered by `size`:
/// the result, the body and whether support 1 is the top plane.
fn chamfered_rim(size: ChamferSize) -> (Kernel, Brep, bool, solidcore::Result<()>) {
    let mut k = definition_kernel();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    let rim = edges_where(&k, &b, |m| (m.z - 10.0).abs() < 1e-9);
    assert_eq!(rim.len(), 1);
    let u0 = edge::blend_edgeuse(&k, &mut b, rim[0]).unwrap().unwrap();
    let f = eu1::face(&b, u0).unwrap().unwrap();
    let plane_first = matches!(
        b.face(f).unwrap().surface.as_ref().unwrap().kind(),
        SurfaceKind::Plane(_)
    );
    let r = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers(&k, &rim, &[size]).unwrap();
        s.do_fillet(&mut k)
    };
    b.refresh_cached_topology();
    (k, b, plane_first, r)
}

/// The faces of `b` that are blends by their definitions, as their procedural blends
/// (none is exact here: a chamfer on a cylinder is certified).
fn definitions(b: &Brep) -> Vec<ProceduralBlend> {
    b.face_list()
        .iter()
        .filter_map(
            |&f| match b.face(f).unwrap().surface.as_ref()?.procedural_blend()? {
                BlendRef::Procedural(pb) => Some(pb.clone()),
                BlendRef::Exact { .. } => panic!("a chamfer on a cylinder is certified"),
                BlendRef::Corner { .. } | BlendRef::Draft { .. } => None,
            },
        )
        .collect()
}

/// Each kind of chamfer of the cylinder's top rim is the chord by its definition, its
/// sizes kept, certified; the body loses the triangle of legs a (along the top) and b
/// (down the side) turned about the axis at its centroid's radius 5 - a/3; the record
/// round trips and re-certifies.
#[test]
fn issue_2486_rim_chamfers_on_a_cylinder_are_chords_by_their_definitions() {
    let kinds = [
        ChamferSize::Distances([1.0, 1.0]),
        ChamferSize::Distances([1.0, 1.5]),
        ChamferSize::DistanceAngle {
            distance: 1.2,
            angle: PI / 3.0,
        },
    ];
    for size in kinds {
        let (k, mut b, plane_first, r) = chamfered_rim(size);
        r.unwrap();
        let defs = definitions(&b);
        assert_eq!(defs.len(), 1, "{size:?}: one chamfer face");
        let pb = &defs[0];
        assert_eq!(pb.order(), 0, "{size:?}");
        assert_eq!(pb.chamfer().unwrap().size, size, "{size:?}");
        assert!(
            pb.bound() > 0.0 && pb.bound() < 1e-5,
            "{size:?}: {}",
            pb.bound()
        );
        let d = ChamferDefinition::new(size, 0.5 * PI).unwrap().setbacks();
        let [a, bb] = if plane_first { d } else { [d[1], d[0]] };
        let want = PI * 25.0 * 10.0 - 2.0 * PI * (5.0 - a / 3.0) * a * bb / 2.0;
        let v = volume(&k, &mut b);
        eprintln!("{size:?}: bound {:e}, volume {v} want {want}", pb.bound());
        assert!((v - want).abs() <= 1e-6 * want, "{size:?}: {v} vs {want}");
        let rec = to_record(&k, pb).unwrap();
        let back = rebuild(&k, &rec).unwrap();
        assert!(back.same_definition(&k, pb).unwrap(), "{size:?}");
        assert_eq!(to_record(&k, &back).unwrap(), rec);
    }
}

/// A chamfer whose offset into the cylinder is wider than the cylinder folds: refused as
/// before, `OffsetFolds`, naming the size that fits.
#[test]
fn issue_2486_a_rim_chamfer_whose_offset_folds_is_refused_naming_its_fit() {
    // 5.5 along the top: the offset into the cylinder (radius 5) is 5.5
    let (_, _, plane_first, _) = chamfered_rim(ChamferSize::Distances([1.0, 1.0]));
    let d = if plane_first { [5.5, 1.0] } else { [1.0, 5.5] };
    let (_, _, _, r) = chamfered_rim(ChamferSize::Distances(d));
    eprintln!("{r:?}");
    let err = r.unwrap_err();
    let SolidError::FilletRefused { reason, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(reason, Some(NoRoomReason::OffsetFolds));
    assert!(fit.size > 0.0 && fit.kind == FitKind::Scale, "{fit:?}");
}

// ---- #2546: a chamfer just short of the corner rounds' radius ------------------------------

use solidcore::topology::brep::validation::{check_brep, CheckLevel};

/// The cube of #9522 (x, y -15..15, z 0..30) with its four vertical edges rounded R5, then
/// its bottom loop of four lines and four arcs chamfered `d` with
/// `fillet_chamfer_cone_on_arc`: the kernel, the body and the chamfer's result.
fn chamfered_rounded_cube(d: f64) -> (Kernel, Brep, solidcore::Result<()>) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = true;
    let mut cube = create_box(&mut k, Point3::new(-15.0, -15.0, 0.0), 30.0, 30.0, 30.0).unwrap();
    let corners = edges_where(&k, &cube, |m| (m.z - 15.0).abs() < 1e-6);
    assert_eq!(corners.len(), 4);
    fillet_edges(&mut k, &mut cube, &corners, 5.0, 1, 1, 1.0).unwrap();
    let bottom = edges_where(&k, &cube, |m| m.z.abs() < 1e-6);
    assert_eq!(bottom.len(), 8);
    let r = fillet_edges(&mut k, &mut cube, &bottom, d, 0, 1, 1.0);
    (k, cube, r)
}

/// The closed form: the rounded cube less the chamfer, by the coarea formula
/// `int_0^d (d - s) (80 + 2 pi (5 - s)) ds` (the bottom face's inward offset by s has
/// perimeter 80 + 2 pi (5 - s), the chamfer's height there is d - s).
fn chamfered_rounded_cube_volume(d: f64) -> f64 {
    (900.0 - (4.0 - PI) * 25.0) * 30.0 - (40.0 * d * d + PI * d * d * (5.0 - d / 3.0))
}

/// The chamfer built: one valid manifold solid of the closed-form volume, 18 faces (the
/// box's six, four rounds, four plane and four cone chamfers).
fn assert_builds(d: f64) {
    let (mut k, mut b, r) = chamfered_rounded_cube(d);
    r.unwrap_or_else(|e| panic!("d {d}: {e}"));
    assert_eq!(b.face_list().len(), 18, "d {d}");
    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "d {d}: {:?}", check.lines());
    assert!(
        solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap(),
        "d {d}"
    );
    let want = chamfered_rounded_cube_volume(d);
    let v = volume(&k, &mut b);
    assert!((v - want).abs() < 1e-9 * want, "d {d}: {v} vs {want}");
}

/// #2546 (WR-740): a chamfer just short of the rounds' radius builds. Each arc's chamfer
/// is the cone about its round's axis whose apex is 5 - d below the bottom face, and its
/// ball's centre is 5 - d from that axis: the corners' search for where the rails meet the
/// rounds' seams starts from that centre's closest point on the arc, which upstream's
/// point/curve Newton step did not find within about R / 100 of the arc's centre (a bare
/// status 1001).
#[test]
fn issue_2546_chamfers_just_short_of_the_rounds_build_at_the_closed_form() {
    for d in [4.99, 4.999, 4.9995] {
        assert_builds(d);
    }
    // and exactly as wide as the rounds, the cones' apexes on the rounds' axes
    assert_builds(5.0);
}

/// The blend's approximation tolerance on this part (its solvers' `approx_tol_3d`): two
/// of them are the resolution within which a rail's two end vertices are one.
const APPROX_TOL: f64 = 1.6e-4;

/// #2546 (WR-739): closer to the rounds' radius, the bottom face's corner arcs (the cone
/// chamfers' rails there, radius 5 - d over a quarter turn, their ends sqrt(2) (5 - d)
/// apart) come within the part's resolution 2 tol: the chamfer is refused before the
/// corners, naming the largest size whose arcs the part resolves, 5 - 2 tol / sqrt(2), and
/// that exactly 5 builds. Below that limit it builds; at d 4.9998 it built a body of no
/// volume, and above it failed with a bare 1001.
#[test]
fn issue_2546_chamfers_within_the_resolution_of_the_rounds_are_refused_naming_the_limit() {
    let limit = 5.0 - 2.0 * APPROX_TOL / std::f64::consts::SQRT_2;
    for d in [4.9998, 4.9999, 4.999999, limit + 1e-7] {
        let (_, _, r) = chamfered_rounded_cube(d);
        let err = r.unwrap_err();
        let text = err.to_string();
        let SolidError::FilletRefused { reason, fit, .. } = err else {
            panic!("d {d}: {err:?}");
        };
        assert_eq!(reason, Some(NoRoomReason::RailBelowResolution), "d {d}");
        assert_eq!(fit.kind, FitKind::Radius, "d {d}");
        assert!(
            (fit.size - limit).abs() < 1e-12,
            "d {d}: {} vs {limit}",
            fit.size
        );
        assert!(text.contains(&format!("{}", fit.size)), "d {d}: {text}");
    }
    // the session's note names both sizes
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = true;
    let mut cube = create_box(&mut k, Point3::new(-15.0, -15.0, 0.0), 30.0, 30.0, 30.0).unwrap();
    let corners = edges_where(&k, &cube, |m| (m.z - 15.0).abs() < 1e-6);
    fillet_edges(&mut k, &mut cube, &corners, 5.0, 1, 1, 1.0).unwrap();
    let bottom = edges_where(&k, &cube, |m| m.z.abs() < 1e-6);
    let comment = {
        let mut s = FilletSession::new(&mut k, &mut cube).unwrap();
        s.prepare_constant_radius_edges(&mut k, &bottom, 4.9999, 0, 1, 1.0)
            .unwrap();
        assert!(s.do_fillet(&mut k).is_err());
        s.error_info().comment.clone()
    };
    assert!(
        comment.contains(
            "The part resolves the rail below 4.999774, and at exactly 5 it closes to a point"
        ),
        "{comment}"
    );
    // just below the limit it builds
    assert_builds(limit - 1e-7);
}

/// #2546 (WR-739): past the rounds' radius the cones' apexes are above the bottom face and
/// the corner arcs' radius 5 - d is negative: each rail runs on the far side of its round's
/// axis, the section through the axis. That is no chamfer of the corner, whether the part
/// resolves the arc or not (just past 5, within the resolution; at 5.1, beyond it, where a
/// body of more volume than at 5 was built). Refused, naming 5, the largest size on the
/// edge's side of the axis, which builds.
#[test]
fn issue_2546_chamfers_past_the_rounds_radius_are_refused_naming_it() {
    for d in [5.0001, 5.0002, 5.1] {
        let (_, _, r) = chamfered_rounded_cube(d);
        let err = r.unwrap_err();
        let SolidError::FilletRefused { reason, fit, .. } = err else {
            panic!("d {d}: {err:?}");
        };
        assert_eq!(reason, Some(NoRoomReason::RailPastAxis), "d {d}");
        assert_eq!(fit.kind, FitKind::Radius, "d {d}");
        assert!((fit.size - 5.0).abs() < 1e-12, "d {d}: {}", fit.size);
    }
}
