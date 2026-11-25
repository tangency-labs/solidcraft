//! A weld bead between two separate bodies (#15130): `FootBlend::of_faces` reads each foot's
//! side from its own face (the side along which the face holds a step from the root) and
//! refuses faces that do not meet in an inside corner, and `FootBlend::toes_on_bodies`
//! proves the strip from the root to each toe on its own body's face over the whole joint. A
//! plate standing on a plate (T joint), a plate lapped on a plate, a rod standing on a plate,
//! a plate standing on a pocket's floor (a face whose outer use runs against its surface) and
//! one on a cavity's floor (a face whose outer side is a void inside the body), each two
//! bodies, 6 mm legs: the flat bead's volume is the closed form, its edges lie in the joint's
//! corner, and both bodies are the same after, in their file bytes and in memory. Small legs
//! on the tee are proved too, and so is a rod sunk into the plate, whose face's edge past the
//! root is in the leg's reach but off the strip. A toe in a notch of the base, a hole in the
//! base between the stations, a pocket in the rod's face on the strip, and a web too low for
//! the leg (a station with no toe) are refused with the leg that fits; two plates butted flush, and a face that leaves the joint
//! away from the corner, meet in no corner; a joint off one of the faces, or over a hole in
//! it, the same face twice, and a face on a region the kernel is unsure of are refused.
//!
//! The tolerances: a bead's edges lie within [`WELD_FIT_TOLERANCE`] of the exact toes and
//! root (its section face is fitted to it); the plates' beads are planes, on which the
//! volume's Gauss-Kronrod rule is exact, so their volume is held to the accuracy asked
//! (relative); the rod's bead's section face is within the deviation the fit certifies of the
//! exact cone (`FootBlend::section_bound`; the fit tolerance where none is certified), so its
//! volume is within that deviation times the face's area (Pappus) and the accuracy asked. A
//! leg's fit is the largest leg that holds, less the halving's bracket `1e-7 (1 + hi)` and the
//! face's tolerance (the fit named keeps the toe on the face itself), and at most the face's
//! zone tolerance (`1e-5`, a toe within it of the boundary is on the face) or the strip's
//! margin (the fit tolerance, at the toe) past the distance to the boundary.

#![allow(clippy::unwrap_used, clippy::panic)] // test

use solidcore::blend::foot_blend::{FootBlend, WELD_FIT_TOLERANCE};
use solidcore::curve::curve::Curve;
use solidcore::error::{ErrorKind, FitMeasure, SolidError};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_curves::{create_circle, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep::p1::tessellate_boundaries;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edge;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};

const LEG: f64 = 6.0;

/// How far a bead's edges may lie from the exact toes and root: the fit tolerance.
const SLACK: f64 = WELD_FIT_TOLERANCE;

/// The volume's accuracy asked, relative.
const ACCURACY: f64 = 1e-9;

/// The halving's bracket on a leg's fit near `leg` (`FootBlend::toes_on_bodies`).
fn bracket(leg: f64) -> f64 {
    1e-7 * (1.0 + leg)
}

/// The face's zone tolerance: a toe within it of the face's boundary is on the face.
const ZONE: f64 = 1e-5;

fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn block(k: &mut Kernel, at: [f64; 3], size: [f64; 3]) -> Brep {
    create_box(
        k,
        Point3::new(at[0], at[1], at[2]),
        size[0],
        size[1],
        size[2],
    )
    .unwrap()
}

/// The edge of `b` nearest `p`.
fn edge_at(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The face of `b` that the edges nearest `p` and `q` share.
fn face_at(k: &Kernel, b: &Brep, p: [f64; 3], q: [f64; 3]) -> FaceId {
    let a = edge::faces(b, edge_at(k, b, p), None).unwrap();
    let c = edge::faces(b, edge_at(k, b, q), None).unwrap();
    let shared: Vec<FaceId> = a.into_iter().filter(|f| c.contains(f)).collect();
    assert_eq!(shared.len(), 1, "{p:?} {q:?}");
    *shared.first().unwrap()
}

fn smb(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &|e| b.owner_identity(e),
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// Both bodies as file bytes and as they are in memory.
fn state(k: &Kernel, a: &Brep, b: &Brep) -> (Vec<u8>, Vec<u8>, String, String) {
    (smb(k, a), smb(k, b), format!("{a:?}"), format!("{b:?}"))
}

fn volume(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::from(
        create_line_segment(Point3::new(a[0], a[1], a[2]), Point3::new(b[0], b[1], b[2])).unwrap(),
    )
}

/// The bead of `face1` of `body1` and `face2` of `body2` along `joint`, 6 mm legs, flat, its
/// toes checked on both bodies; both bodies asserted the same after. Its volume, and whether
/// every point of its edges passes `corner`.
fn bead(
    k: &mut Kernel,
    (body1, face1): (&Brep, FaceId),
    (body2, face2): (&Brep, FaceId),
    joint: &Curve,
    corner: impl Fn(Point3) -> bool,
    accuracy: f64,
) -> (f64, bool) {
    let before = state(k, body1, body2);
    let blend = FootBlend::of_faces(k, body1, face1, body2, face2, joint).unwrap();
    blend
        .toes_on_bodies(k, [body1, body2], [LEG, LEG], &[])
        .unwrap();
    let mut body = blend.body(k, [LEG, LEG], 0.0, WELD_FIT_TOLERANCE).unwrap();
    assert!(state(k, body1, body2) == before, "a part changed");
    let points = tessellate_boundaries(k, &body, 2.0).unwrap().points;
    let inside = !points.is_empty() && points.iter().all(|&p| corner(p));
    (volume(k, &mut body, accuracy), inside)
}

/// A web 10 thick and 50 high standing at x = 45 on a base 100 by 100, 10 thick: the web's
/// face at x = 45 and the base's top.
fn tee(k: &mut Kernel) -> (Brep, FaceId, Brep, FaceId) {
    let base = block(k, [0.0; 3], [100.0, 100.0, 10.0]);
    let web = block(k, [45.0, 0.0, 10.0], [10.0, 100.0, 50.0]);
    let side = face_at(k, &web, [45.0, 50.0, 10.0], [45.0, 0.0, 35.0]);
    let top = face_at(k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    (web, side, base, top)
}

#[test]
fn a_plate_standing_on_a_plate_gets_its_bead_in_the_corner() {
    let mut k = kernel();
    let (web, side, base, top) = tee(&mut k);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let (v, inside) = bead(
        &mut k,
        (&web, side),
        (&base, top),
        &joint,
        |p| {
            (39.0 - SLACK..=45.0 + SLACK).contains(&p.x)
                && (10.0 - SLACK..=16.0 + SLACK).contains(&p.z)
        },
        ACCURACY,
    );
    assert!(inside, "the bead left the corner");
    assert!((v - 1800.0).abs() <= ACCURACY * 1800.0, "{v}");
}

#[test]
fn a_plate_lapped_on_a_plate_gets_its_bead_in_the_corner() {
    let mut k = kernel();
    let lower = block(&mut k, [0.0; 3], [100.0, 50.0, 10.0]);
    let upper = block(&mut k, [0.0, 40.0, 10.0], [100.0, 50.0, 10.0]);
    let front = face_at(&k, &upper, [50.0, 40.0, 10.0], [0.0, 40.0, 15.0]);
    let top = face_at(&k, &lower, [50.0, 0.0, 10.0], [0.0, 25.0, 10.0]);
    let joint = line([0.0, 40.0, 10.0], [100.0, 40.0, 10.0]);
    let (v, inside) = bead(
        &mut k,
        (&upper, front),
        (&lower, top),
        &joint,
        |p| {
            (34.0 - SLACK..=40.0 + SLACK).contains(&p.y)
                && (10.0 - SLACK..=16.0 + SLACK).contains(&p.z)
        },
        ACCURACY,
    );
    assert!(inside, "the bead left the corner");
    assert!((v - 1800.0).abs() <= ACCURACY * 1800.0, "{v}");
}

/// The rod's foot is a closed joint: the bead runs round it in one piece of the foot blend
/// (built in four segments), its volume the triangle's 18 mm² swept round radius 17 (Pappus).
#[test]
fn a_rod_standing_on_a_plate_gets_its_bead_round_its_foot() {
    let mut k = kernel();
    let plate = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let rod = create_cylinder(&mut k, Point3::new(50.0, 50.0, 10.0), 15.0, 50.0).unwrap();
    let side = face_at(&k, &rod, [65.0, 50.0, 10.0], [65.0, 50.0, 60.0]);
    let top = face_at(&k, &plate, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = Curve::from(create_circle(Point3::new(50.0, 50.0, 10.0), 15.0).unwrap());
    let (v, inside) = bead(
        &mut k,
        (&rod, side),
        (&plate, top),
        &joint,
        |p| {
            let r = ((p.x - 50.0).powi(2) + (p.y - 50.0).powi(2)).sqrt();
            (15.0 - SLACK..=21.0 + SLACK).contains(&r)
                && (10.0 - SLACK..=16.0 + SLACK).contains(&p.z)
        },
        1e-6,
    );
    assert!(inside, "the bead left the corner");
    let tau = 2.0 * std::f64::consts::PI;
    let pappus = 18.0 * tau * 17.0;
    // the section face: the hypotenuse, 6 sqrt 2, swept round its middle's radius 18; the
    // face's net within the deviation the fit certifies of the exact cone (else within the fit
    // tolerance), so the volume within that deviation times the face's area, and the
    // integration's accuracy asked
    let face = 6.0 * 2.0_f64.sqrt() * tau * 18.0;
    let blend = FootBlend::of_faces(&k, &rod, side, &plate, top, &joint).unwrap();
    let deviation = blend
        .section_bound(&k, [LEG, LEG], 0.0, WELD_FIT_TOLERANCE)
        .unwrap()
        .unwrap_or(WELD_FIT_TOLERANCE);
    let bound = deviation * face + 1e-6 * pappus;
    assert!(
        (v - pappus).abs() <= bound,
        "{v} against {pappus}, {bound} (deviation {deviation})"
    );
}

/// A notch cut through the base behind the web (x from 20 to 30, y from 40 to 60): a leg of
/// 20 along the base puts the toes in it, off the base's top but on its plane. Refused,
/// naming no face (two bodies), with the largest leg whose toes stay on it, 15 (to the
/// notch's side at x = 30), which passes; both bodies unchanged.
#[test]
fn a_toe_in_a_notch_of_the_other_body_s_face_is_refused_with_the_leg_that_fits() {
    use solidcore::boolean::api_brep::boolean_difference;
    let mut k = kernel();
    let (web, side, base, _) = tee(&mut k);
    let notch = block(&mut k, [20.0, 40.0, -1.0], [10.0, 20.0, 12.0]);
    let base = boolean_difference(&mut k, base, notch).unwrap();
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let before = state(&k, &web, &base);
    let blend = FootBlend::of_faces(&k, &web, side, &base, top, &joint).unwrap();
    let err = blend
        .toes_on_bodies(&k, [&web, &base], [LEG, 20.0], &[])
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::FilletTooLarge, "{err}");
    assert!(format!("{err:?}").contains("LegPastFace"), "{err:?}");
    let fit = err.fit().unwrap();
    assert_eq!(fit.measure, FitMeasure::Leg, "{err}");
    assert!(
        fit.size >= 15.0 - bracket(15.0) - ZONE && fit.size <= 15.0 + ZONE + SLACK,
        "{}",
        fit.size
    );
    blend
        .toes_on_bodies(&k, [&web, &base], [LEG, fit.size], &[])
        .unwrap();
    assert!(state(&k, &web, &base) == before, "a part changed");
}

/// Two plates butted flush, tops level: the joint lies on both tops, but no foot heads to
/// the other face's outer side, so there is no corner for a bead.
#[test]
fn plates_butted_flush_meet_in_no_corner() {
    let mut k = kernel();
    let a = block(&mut k, [0.0; 3], [50.0, 100.0, 10.0]);
    let b = block(&mut k, [50.0, 0.0, 0.0], [50.0, 100.0, 10.0]);
    let ta = face_at(&k, &a, [25.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let tb = face_at(&k, &b, [75.0, 0.0, 10.0], [100.0, 50.0, 10.0]);
    let joint = line([50.0, 0.0, 10.0], [50.0, 100.0, 10.0]);
    let got = FootBlend::of_faces(&k, &a, ta, &b, tb, &joint);
    assert!(
        matches!(&got, Err(SolidError::InvalidInput { .. }))
            && format!("{got:?}").contains("inside corner"),
        "{got:?}"
    );
}

/// The web's face at x = 45 and the base's bottom: the joint along the web's foot is not on
/// the bottom.
#[test]
fn a_joint_off_one_of_the_faces_is_refused() {
    let mut k = kernel();
    let (web, side, base, _) = tee(&mut k);
    let bottom = face_at(&k, &base, [50.0, 0.0, 0.0], [0.0, 50.0, 0.0]);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let got = FootBlend::of_faces(&k, &web, side, &base, bottom, &joint);
    assert!(
        matches!(&got, Err(SolidError::InvalidInput { .. }))
            && format!("{got:?}").contains("does not lie on both faces"),
        "{got:?}"
    );
}

/// A web 10 thick standing on a pocket's floor: a base 100 by 100, 10 thick, with a pocket
/// 60 by 60, 5 deep (its floor at z = 5, x and y from 20 to 80), the web at x = 45 over the
/// pocket's length. The floor is the cutter's face, its outer use against its surface: the
/// face's outward normal is the surface's turned. Read unturned, the web's foot would head
/// into the floor's material and the corner be refused. The bead is 18 by 60.
#[test]
fn a_plate_standing_on_a_pocket_floor_gets_its_bead_in_the_corner() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::edgeuse::p1::faceuse_orientation;
    use solidcore::topology::face::p2::outer_faceuse;
    let mut k = kernel();
    let base = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let cutter = block(&mut k, [20.0, 20.0, 5.0], [60.0, 60.0, 6.0]);
    let base = boolean_difference(&mut k, base, cutter).unwrap();
    let web = block(&mut k, [45.0, 20.0, 5.0], [10.0, 60.0, 30.0]);
    let side = face_at(&k, &web, [45.0, 50.0, 5.0], [45.0, 20.0, 20.0]);
    let floor = face_at(&k, &base, [50.0, 20.0, 5.0], [20.0, 50.0, 5.0]);
    let fu = outer_faceuse(&base, floor).unwrap().unwrap();
    assert_eq!(
        faceuse_orientation(&base, fu).unwrap(),
        OrientType::Opposite,
        "the floor's outer use runs with its surface: not the case this test is for"
    );
    let joint = line([45.0, 20.0, 5.0], [45.0, 80.0, 5.0]);
    let (v, inside) = bead(
        &mut k,
        (&web, side),
        (&base, floor),
        &joint,
        |p| {
            (39.0 - SLACK..=45.0 + SLACK).contains(&p.x)
                && (5.0 - SLACK..=11.0 + SLACK).contains(&p.z)
        },
        ACCURACY,
    );
    assert!(inside, "the bead left the corner");
    assert!((v - 1080.0).abs() <= ACCURACY * 1080.0, "{v}");
}

/// A hole through the base (x from 40 to 44, y from 41 to 49) under the bead's strip on it,
/// between the stations (every 10 along the joint) where the toes and the sides are read:
/// the toes at x = 39 are on the base's top and the root beside it, but the strip runs over
/// the hole. Refused by the strip's proof, naming no face (two bodies), with the largest leg
/// whose strip stays on the top: 1, to the hole's side at x = 44; that leg passes.
#[test]
fn a_hole_under_the_strip_between_the_stations_is_refused_with_the_leg_that_fits() {
    use solidcore::boolean::api_brep::boolean_difference;
    let mut k = kernel();
    let (web, side, base, _) = tee(&mut k);
    let hole = block(&mut k, [40.0, 41.0, -1.0], [4.0, 8.0, 12.0]);
    let base = boolean_difference(&mut k, base, hole).unwrap();
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let before = state(&k, &web, &base);
    let blend = FootBlend::of_faces(&k, &web, side, &base, top, &joint).unwrap();
    let err = blend
        .toes_on_bodies(&k, [&web, &base], [LEG, LEG], &[])
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::FilletTooLarge, "{err}");
    let debug = format!("{err:?}");
    assert!(
        debug.contains("LegPastFace") && debug.contains("face: None"),
        "{debug}"
    );
    let fit = err.fit().unwrap();
    assert_eq!(fit.measure, FitMeasure::Leg, "{err}");
    assert!(
        fit.size >= 1.0 - bracket(1.0) - ZONE && fit.size <= 1.0 + ZONE + SLACK,
        "{}",
        fit.size
    );
    blend
        .toes_on_bodies(&k, [&web, &base], [LEG, fit.size], &[])
        .unwrap();
    assert!(state(&k, &web, &base) == before, "a part changed");
}

/// The hole under the web's foot (x from 40 to 50, y from 40 to 60): the joint crosses it,
/// so the base's top holds neither side of the root there.
#[test]
fn a_joint_over_a_hole_in_one_of_the_faces_is_refused() {
    use solidcore::boolean::api_brep::boolean_difference;
    let mut k = kernel();
    let (web, side, base, _) = tee(&mut k);
    let hole = block(&mut k, [40.0, 40.0, -1.0], [10.0, 20.0, 12.0]);
    let base = boolean_difference(&mut k, base, hole).unwrap();
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let got = FootBlend::of_faces(&k, &web, side, &base, top, &joint);
    assert!(
        matches!(&got, Err(SolidError::InvalidInput { .. }))
            && format!("{got:?}").contains("does not lie on both faces"),
        "{got:?}"
    );
}

/// A block hanging beside the base (x from 100 to 110, z from -40 to 10), its face at x =
/// 100 meeting the base's top along the top's edge: that face leaves the joint downward, away
/// from the top's outer side, so the two faces meet in an outside corner (the base's own edge
/// continued), not an inside one.
#[test]
fn a_face_leaving_the_joint_away_from_the_corner_is_refused() {
    let mut k = kernel();
    let base = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let hang = block(&mut k, [100.0, 0.0, -40.0], [10.0, 100.0, 50.0]);
    let face = face_at(&k, &hang, [100.0, 50.0, 10.0], [100.0, 0.0, -15.0]);
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = line([100.0, 0.0, 10.0], [100.0, 100.0, 10.0]);
    let got = FootBlend::of_faces(&k, &hang, face, &base, top, &joint);
    assert!(
        matches!(&got, Err(SolidError::InvalidInput { .. }))
            && format!("{got:?}").contains("inside corner"),
        "{got:?}"
    );
}

/// The base's top as both faces.
#[test]
fn the_same_face_twice_is_refused() {
    let mut k = kernel();
    let (_, _, base, top) = tee(&mut k);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let got = FootBlend::of_faces(&k, &base, top, &base, top, &joint);
    assert!(
        matches!(&got, Err(SolidError::InvalidInput { .. }))
            && format!("{got:?}").contains("the same face twice"),
        "{got:?}"
    );
}

/// A web on the floor of a cavity: a block 100 by 100, 50 high, with a closed cavity 60 by 60,
/// 30 high inside it (its floor at z = 10), a web 10 thick at x = 45 over the cavity's length.
/// The floor's outer use bounds the cavity, a void that is not the infinite region: it has
/// an outer side. The bead is 18 by 60.
#[test]
fn a_plate_standing_on_a_cavity_floor_gets_its_bead_in_the_corner() {
    use solidcore::boolean::api_brep::boolean_difference;
    let mut k = kernel();
    let block_ = block(&mut k, [0.0; 3], [100.0, 100.0, 50.0]);
    let cavity = block(&mut k, [20.0, 20.0, 10.0], [60.0, 60.0, 30.0]);
    let base = boolean_difference(&mut k, block_, cavity).unwrap();
    let web = block(&mut k, [45.0, 20.0, 10.0], [10.0, 60.0, 20.0]);
    let side = face_at(&k, &web, [45.0, 50.0, 10.0], [45.0, 20.0, 20.0]);
    let floor = face_at(&k, &base, [50.0, 20.0, 10.0], [20.0, 50.0, 10.0]);
    let joint = line([45.0, 20.0, 10.0], [45.0, 80.0, 10.0]);
    let (v, inside) = bead(
        &mut k,
        (&web, side),
        (&base, floor),
        &joint,
        |p| {
            (39.0 - SLACK..=45.0 + SLACK).contains(&p.x)
                && (10.0 - SLACK..=16.0 + SLACK).contains(&p.z)
        },
        ACCURACY,
    );
    assert!(inside, "the bead left the corner");
    assert!((v - 1080.0).abs() <= ACCURACY * 1080.0, "{v}");
}

/// Small legs on the tee, down to a tenth of a millimetre (the smallest a body builds is far
/// below): the strip's proof starts from the edges' and the joint's spans and halves only
/// where it needs to, so it finishes as the leg shrinks.
#[test]
fn small_legs_on_the_tee_are_proved_on_the_faces() {
    let mut k = kernel();
    let (web, side, base, top) = tee(&mut k);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let blend = FootBlend::of_faces(&k, &web, side, &base, top, &joint).unwrap();
    for leg in [0.5, 0.2, 0.1] {
        blend
            .toes_on_bodies(&k, [&web, &base], [leg, leg], &[])
            .unwrap_or_else(|e| panic!("leg {leg}: {e}"));
    }
}

/// The cavity's floor again, with the cavity's region marked unsure (`UNSURE`, 2): the
/// floor's outer use is on a region the kernel cannot classify, so `of_faces` refuses it
/// (`UnsureRegion`) rather than reading it as void or as material.
#[test]
fn a_face_on_a_region_the_kernel_is_unsure_of_is_refused() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::topology::brep::p1::regions;
    let mut k = kernel();
    let block_ = block(&mut k, [0.0; 3], [100.0, 100.0, 50.0]);
    let cavity = block(&mut k, [20.0, 20.0, 10.0], [60.0, 60.0, 30.0]);
    let mut base = boolean_difference(&mut k, block_, cavity).unwrap();
    let infinite = base.infinite_region();
    let mut changed = 0;
    for r in regions(&base, None).unwrap() {
        if Some(r) != infinite && base.region(r).unwrap().is_void() == 1 {
            base.region_mut(r).unwrap().set_is_void(2);
            changed += 1;
        }
    }
    assert_eq!(changed, 1, "the cavity is the one void besides the outside");
    let web = block(&mut k, [45.0, 20.0, 10.0], [10.0, 60.0, 20.0]);
    let side = face_at(&k, &web, [45.0, 50.0, 10.0], [45.0, 20.0, 20.0]);
    let floor = face_at(&k, &base, [50.0, 20.0, 10.0], [20.0, 50.0, 10.0]);
    let joint = line([45.0, 20.0, 10.0], [45.0, 80.0, 10.0]);
    let got = FootBlend::of_faces(&k, &web, side, &base, floor, &joint);
    assert!(
        matches!(&got, Err(SolidError::UnsureRegion { .. })),
        "{got:?}"
    );
}

/// A web only 3 high on the base, legs 6: up the web's face the foot runs off its surface at
/// every station, so no toe is found there. Refused as the build refuses it (`toes_on_bodies`
/// takes `body`'s refusal for a station with no toe), with the largest leg whose foot is
/// found, 3 (the web's top), less the halving's bracket. Both faces are planes: no strip is
/// enclosed here (`FootBlend::strip_cover`).
#[test]
fn a_station_with_no_toe_is_refused_with_the_leg_that_fits() {
    let mut k = kernel();
    let base = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let web = block(&mut k, [45.0, 0.0, 10.0], [10.0, 100.0, 3.0]);
    let side = face_at(&k, &web, [45.0, 50.0, 10.0], [45.0, 0.0, 11.5]);
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let blend = FootBlend::of_faces(&k, &web, side, &base, top, &joint).unwrap();
    let err = blend
        .toes_on_bodies(&k, [&web, &base], [LEG, LEG], &[])
        .unwrap_err();
    let fit = err.fit().unwrap_or_else(|| panic!("no fit: {err:?}"));
    assert_eq!(fit.measure, FitMeasure::Leg, "{err}");
    assert!(
        fit.size >= 3.0 - bracket(3.0) && fit.size <= 3.0 + ZONE,
        "{}",
        fit.size
    );
}

/// A rod sunk 2 into the plate (its foot at z = 8, below the plate's top): the rod's face goes
/// on past the joint on the side away from the bead, and its bottom edge lies 2 below the root,
/// inside the leg's reach but not on the strip. On a curved support that edge is told off the
/// strip by the strip's enclosure (`FootBlend::strip_cover`), not refused.
#[test]
fn an_edge_of_a_curved_face_beyond_the_root_is_not_on_the_strip() {
    let mut k = kernel();
    let plate = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let rod = create_cylinder(&mut k, Point3::new(50.0, 50.0, 8.0), 15.0, 52.0).unwrap();
    let side = face_at(&k, &rod, [65.0, 50.0, 8.0], [65.0, 50.0, 60.0]);
    let top = face_at(&k, &plate, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = Curve::from(create_circle(Point3::new(50.0, 50.0, 10.0), 15.0).unwrap());
    let blend = FootBlend::of_faces(&k, &rod, side, &plate, top, &joint).unwrap();
    blend
        .toes_on_bodies(&k, [&rod, &plate], [LEG, LEG], &[])
        .unwrap();
}

/// A pocket cut into the rod's face on the strip (x from 48 to 52, y from 62 to 68, z from 11 to
/// 14): its edges on the rod's face, fitted where the pocket's walls meet the cylinder, lie on the
/// strip between 1 and 4 from the root. On a curved support the strip's enclosure clears only an
/// edge outside it, so these stay on it and the strip is refused, naming no face (two bodies), with
/// a leg that fits: at most 1 past the margins, to the pocket's lower edge, and at least 1 less the
/// strip's margin (the fit tolerance), where the proof on that fitted edge, an arc beside the
/// joint's arc, may stop deciding; that leg passes.
#[test]
fn a_pocket_in_a_curved_face_on_the_strip_is_refused_with_the_leg_that_fits() {
    use solidcore::boolean::api_brep::boolean_difference;
    let mut k = kernel();
    let plate = block(&mut k, [0.0; 3], [100.0, 100.0, 10.0]);
    let rod = create_cylinder(&mut k, Point3::new(50.0, 50.0, 10.0), 15.0, 50.0).unwrap();
    let pocket = block(&mut k, [48.0, 62.0, 11.0], [4.0, 6.0, 3.0]);
    let rod = boolean_difference(&mut k, rod, pocket).unwrap();
    let side = face_at(&k, &rod, [35.0, 50.0, 10.0], [35.0, 50.0, 60.0]);
    let top = face_at(&k, &plate, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let joint = Curve::from(create_circle(Point3::new(50.0, 50.0, 10.0), 15.0).unwrap());
    let before = state(&k, &rod, &plate);
    let blend = FootBlend::of_faces(&k, &rod, side, &plate, top, &joint).unwrap();
    let err = blend
        .toes_on_bodies(&k, [&rod, &plate], [LEG, LEG], &[])
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::FilletTooLarge, "{err}");
    let debug = format!("{err:?}");
    assert!(
        debug.contains("LegPastFace") && debug.contains("face: None"),
        "{debug}"
    );
    let fit = err.fit().unwrap();
    assert_eq!(fit.measure, FitMeasure::Leg, "{err}");
    assert!(
        fit.size >= 1.0 - SLACK - ZONE && fit.size <= 1.0 + ZONE + SLACK,
        "{}",
        fit.size
    );
    blend
        .toes_on_bodies(&k, [&rod, &plate], [fit.size, LEG], &[])
        .unwrap();
    assert!(state(&k, &rod, &plate) == before, "a part changed");
}
