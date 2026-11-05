//! tangency-labs/solidcore#2358 (the fork's smlib#124): the junction rig's boss, a cylinder
//! R6 standing 2 into a 60 x 60 x 10 plate, tilted about y, its root rounded (WR-697).
//!
//! Tilted 20 degrees the cylinder's bottom cap rises out of the plate on the -x side, by
//! 6 sin 20 - 2 cos 20 = 0.173 at its top: a wedge of air under the cap. The root edge's
//! round ends at the two vertices where the root ellipse meets the cap's chord on the
//! plate, Ends of mixed convexity, on the cap: its rail on the wall must end on the rim,
//! which it does only while r <= r* = (6 sin 20 - 2 cos 20) / (1 - sin 20) = 0.262524174.
//! Past it the round's rail passes over the rim: a concave overflow, refused
//! (`SM_FILERR_NO_ROOM`) naming the largest radius whose rail still ends on the rim, less
//! the part's tolerance (1e-5): 0.262514174 (the fork's 54a4ee7, `BallTouchingAt` and
//! `RefuseEndOverflowingSideEdge`).
//!
//! Inputs and expectations are the fork's capi test `scenario_junction_boss_tilt_20_root_round`
//! (ce52279); the values are the fork at 18e96f1 (smlib#124 at 619c2f0) run on the same
//! inputs (`SmApiCreateBox`, `SmApiCreateCylinder` rotated and translated,
//! `SmApiBooleanUnion`, `SmFilletExecutive` with a circular section at constant radius,
//! continuity 2, keep features): at r 1.5 status 1001, `SM_FILERR_NO_ROOM`, fit
//! 0.262514174 and that reason; at 0.25 and 0.2625 built (11 faces, -0.576293097954 and
//! -0.635789172127 removed: the round adds material).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::{EntityRef, SolidError};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::cutting::PlaneCutter;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::face::p2 as face;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::{edge, vertex, Brep, EdgeId, FaceId};

/// Every opt-in fix on, with the blend corner as given, except the two the fork does not
/// have that move this body's measure: the exact trims of the cylinder's sections by the
/// plate (`cylinder_plane_section_trims_exact`) and the integrator's split at surface knots
/// (`precise_properties_split_at_surface_knots`). With them on the union measures 3.6e-5
/// less (38601.117929445747 against the fork's 38601.117965259655), and the removed
/// volumes move by 1.7e-4.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        *f.flag_mut(name).unwrap() = true;
    }
    f.cylinder_plane_section_trims_exact = false;
    f.precise_properties_split_at_surface_knots = false;
    k.fixes = f;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The plate with the boss tilted `deg` about y, its foot 2 into the plate, its cylinder
/// first turned `seam` degrees about its axis (moving its seam; 0 leaves it).
fn boss_seam(k: &mut Kernel, deg: f64, seam: f64) -> Brep {
    let a = deg.to_radians();
    let plate = create_box(k, Point3::new(-30.0, -30.0, -10.0), 60.0, 60.0, 10.0).unwrap();
    let mut cyl = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 6.0, 25.0).unwrap();
    let origin = Point3::new(0.0, 0.0, 0.0);
    if seam != 0.0 {
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut cyl)),
            origin,
            Vec3::Z,
            seam,
        )
        .unwrap();
    }
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut cyl)),
        origin,
        Vec3::Y,
        deg,
    )
    .unwrap();
    api_translate(
        k,
        Some(TransformObject::Brep(&mut cyl)),
        Vec3::new(-2.0 * a.sin(), 0.0, -2.0 * a.cos()),
    )
    .unwrap();
    boolean_union(k, plate, cyl).unwrap()
}

/// The root ellipse's edges: on the plate's top, their middles on the cylinder.
fn root(k: &Kernel, b: &Brep, deg: f64) -> Vec<EdgeId> {
    let a = deg.to_radians();
    let axis = Vec3::new(a.sin(), 0.0, a.cos());
    let foot = Point3::new(-2.0 * a.sin(), 0.0, -2.0 * a.cos());
    let mut out = Vec::new();
    for &e in b.edge_list() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let iv = edge::interval(b, e).unwrap();
        let p0 = c.evaluate_point(k, iv.min()).unwrap();
        let p1 = c.evaluate_point(k, iv.max()).unwrap();
        let pm = c.evaluate_point(k, 0.5 * (iv.min() + iv.max())).unwrap();
        if p0.z.abs() > 1e-6 || p1.z.abs() > 1e-6 || pm.z.abs() > 1e-6 {
            continue;
        }
        let d = pm - foot;
        let q = d - axis * d.dot(axis);
        if (q.length() - 6.0).abs() < 1e-6 {
            out.push(e);
        }
    }
    out
}

/// The root of the boss tilted `deg` rounded `r`: the volume it removes and the face count.
fn rounded(deg: f64, r: f64) -> Result<(f64, usize), SolidError> {
    rounded_seam(deg, 0.0, r).map(|(v, n, _)| (v, n))
}

/// As [`rounded`] with the seam turned `seam`; also the body's vertices.
fn rounded_seam(deg: f64, seam: f64, r: f64) -> Result<(f64, usize, Vec<Point3>), SolidError> {
    let mut k = kernel();
    let mut b = boss_seam(&mut k, deg, seam);
    let before = volume(&k, &mut b);
    let es = root(&k, &b, deg);
    // as the fork's: two at tilt 20, one (the seam turned onto the chord side) at tilt 40
    assert_eq!(es.len(), if seam == 0.0 { 2 } else { 1 });
    fillet_edges(&mut k, &mut b, &es, r, 1, 2, 1.0)?;
    let points = b
        .vertex_list()
        .iter()
        .map(|&v| vertex::point(&b, v).unwrap())
        .collect();
    Ok((before - volume(&k, &mut b), b.face_list().len(), points))
}

/// The bound r*, closed form.
fn bound() -> f64 {
    let a = 20.0_f64.to_radians();
    (6.0 * a.sin() - 2.0 * a.cos()) / (1.0 - a.sin())
}

#[test]
fn the_bound_is_the_closed_form() {
    assert!((bound() - 0.262_524_174).abs() <= 1e-9, "{}", bound());
}

/// Tilted 20 degrees and rounded 1.5: refused, naming r* less the part's tolerance.
#[test]
fn tilt_20_is_refused_naming_the_largest_radius() {
    let err = rounded(20.0, 1.5).unwrap_err();
    let SolidError::FilletRefused { code, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(code, 1001);
    assert!((fit.size - 0.262_514_174).abs() <= 1e-9, "{}", fit.size);
    assert!(fit.size < bound() && fit.size >= bound() - 0.02);
}

/// The radii below the bound build, as the fork's: the removed volumes agree to 2.5e-9
/// (-0.576293096943 and -0.635789174601 here; the union to 4e-8 of 38601.12).
#[test]
fn tilt_20_below_the_bound_builds() {
    for (r, removed) in [(0.25, -0.576_293_097_954), (0.2625, -0.635_789_172_127)] {
        let (got, faces) = rounded(20.0, r).unwrap();
        assert!((got - removed).abs() <= 5e-9, "{r}: {got}");
        assert_eq!(faces, 11);
    }
}

/// Tilted 40 degrees with the cylinder's seam turned 125.13 degrees, onto the cap's chord
/// side (the fork's capi seam of 0.6131 rad past the quarter turn), rounded 1.5: each wall
/// rail ends on its side edge at the seam's vertex and is continued across it (63c7962).
/// The +y End's rim edge stops at the seam's vertex short of the rail's end, which lies on
/// the next rim edge; the End's edge on the rim joins the two (a circle's arcs: their
/// B-splines). The fork: 10 faces, -33.432934277429 removed, the rails' ends at
/// (-4.176377478, +-4.664695271, 0.893582223). Pinned here at -33.432934235592: the side
/// plane extended for the round is now an exactly widened plane (the fork's path handed the
/// round the plane's fitted B-spline), and the faces, rails and rounds are exact. The move,
/// 4.2e-8, is 1.1e-12 of the part's volume (38601): rounding at the part's scale, so the
/// value is re-pinned with the same 5e-9.
#[test]
fn tilt_40_rail_continued_across_the_seam_vertex() {
    let (got, faces, points) = rounded_seam(40.0, 125.13, 1.5).unwrap();
    assert!((got - -33.432_934_235_592).abs() <= 5e-9, "{got}");
    assert_eq!(faces, 10);
    for y in [-4.664_695_271, 4.664_695_271] {
        let want = Point3::new(-4.176_377_478, y, 0.893_582_223);
        assert!(
            points.iter().any(|p| p.distance(want) <= 1e-8),
            "{want:?} not among {points:?}"
        );
    }
}

/// Tilted 20 degrees with the seam turned 170 degrees, into the cap's overhang, which splits
/// the rim arc there: still refused naming r* less the part's tolerance, the most over both
/// edges of the split arc (`SideEdgeChainTo`, 63c7962), and that size builds (its +y End's
/// rail ends past the seam's vertex). The fork: 1001, fit 0.262514174; at 0.262514174, 10
/// faces and -0.636117736700 removed. Here -0.636117743787: the union measures 7.7e-9 more
/// than the fork's (38601.117955469737 against 38601.117955462076), the result 1.5e-8 more
/// (4e-13 relative).
#[test]
fn tilt_20_seam_over_the_rim_refused_on_the_whole_rim() {
    let err = rounded_seam(20.0, 170.0, 1.5).unwrap_err();
    let SolidError::FilletRefused { code, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(code, 1001);
    assert!((fit.size - 0.262_514_174).abs() <= 1e-9, "{}", fit.size);
    let (got, faces, _) = rounded_seam(20.0, 170.0, 0.262_514_174).unwrap();
    assert!((got - -0.636_117_736_700).abs() <= 1e-8, "{got}");
    assert_eq!(faces, 10);
}

/// The wall of [`boss_seam`] split by the plane through its generators at 120 and 240
/// degrees from the cap's frame x, as the capi's `smlib_split_faces` cuts a face: the
/// wall's section by the plane over its UV domain, merged into it through the plane
/// (`MergeCurvesOnSurface`, here [`PlaneCutter::merge_intersection`] on the wall alone).
/// Two more faces.
fn wall_split(k: &mut Kernel, b: &mut Brep, deg: f64) {
    let a = deg.to_radians();
    let axis = Vec3::new(a.sin(), 0.0, a.cos());
    let ux = Vec3::new(a.cos(), 0.0, -a.sin());
    let walls: Vec<FaceId> = b
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            matches!(
                face::surface(b, f).unwrap().map(Surface::kind),
                Some(SurfaceKind::Cone(_))
            )
        })
        .collect();
    assert_eq!(walls.len(), 1);
    let before = b.face_list().len();
    let origin = Point3::new(-2.0 * a.sin(), 0.0, -2.0 * a.cos()) - ux * 3.0;
    let normal = Vec3::Y.cross(axis).unitize().unwrap();
    let mut cutter = PlaneCutter::new(origin, normal);
    let tol = b.tolerance();
    let editing = b.editing_enabled();
    b.set_editing_enabled(true);
    let doing = k.context.doing_boolean();
    k.context.set_doing_boolean(true);
    {
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        let curve_ref = |e: EdgeId, _type_code: i64| notify_ref(EntityRef::Edge(e));
        cutter
            .merge_intersection(k, b, &mut env, &curve_ref, tol, Some(&walls))
            .unwrap();
    }
    k.context.set_doing_boolean(doing);
    b.set_editing_enabled(editing);
    assert_eq!(b.face_list().len(), before + 2);
}

/// Tilted 40 degrees, the seam turned 125.13 degrees, the wall split by a real edge at 120
/// and 240 degrees (the fork's capi test
/// `scenario_junction_boss_tilt_40_wall_split_short_of_the_rail_end_refused`, ad30d68), and
/// rounded 1.5: the rail ending past the split's vertex is not continued across it (a real
/// edge between two faces is a corner, 63c7962), and the round is refused naming the radius
/// whose rail ends at 120 degrees, (3 sin 40 - 2 cos 40) / (1 - sin 40 / 2) = 0.583952734,
/// less the part's tolerance. The fork at 18e96f1, the same split
/// (`CreatePlanarSectionCurves` and `MergeCurvesOnSurface` through the plane) and round:
/// 1001, `SM_FILERR_NO_ROOM`, fit 0.583942733779.
#[test]
fn tilt_40_wall_split_short_of_the_rail_end_refused() {
    let a = 40.0_f64.to_radians();
    let r120 = (3.0 * a.sin() - 2.0 * a.cos()) / (1.0 - 0.5 * a.sin());
    let mut k = kernel();
    let mut b = boss_seam(&mut k, 40.0, 125.13);
    wall_split(&mut k, &mut b, 40.0);
    let es = root(&k, &b, 40.0);
    let err = fillet_edges(&mut k, &mut b, &es, 1.5, 1, 2, 1.0).unwrap_err();
    let SolidError::FilletRefused { code, fit, .. } = err else {
        panic!("{err:?}");
    };
    assert_eq!(code, 1001);
    let want = r120 - 1e-5;
    assert!(
        (fit.size - 0.583_942_733_779).abs() <= 1e-9,
        "{} ({want})",
        fit.size
    );
}
