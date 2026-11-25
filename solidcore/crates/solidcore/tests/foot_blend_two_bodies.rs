//! A weld bead between two separate bodies (#15130): `FootBlend::of_faces` reads each foot's
//! side from the other face's outward normal along a joint curve, and
//! `FootBlend::toes_on_bodies` checks each toe on its own body's face. A plate standing on a
//! plate (T joint), a plate lapped on a plate and a rod standing on a plate, each two bodies,
//! 6 mm legs: the flat bead's volume is the closed form, its edges lie in the joint's corner,
//! and both bodies are the same after, in their file bytes and in memory. A leg past the base
//! plate's end is refused with the leg that fits; two plates butted flush meet in no corner;
//! a joint off one of the faces is refused.

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
        |p| (38.95..=45.05).contains(&p.x) && (9.95..=16.05).contains(&p.z),
        1e-9,
    );
    assert!(inside, "the bead left the corner");
    assert!((v - 1800.0).abs() <= 1e-6 * 1800.0, "{v}");
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
        |p| (33.95..=40.05).contains(&p.y) && (9.95..=16.05).contains(&p.z),
        1e-9,
    );
    assert!(inside, "the bead left the corner");
    assert!((v - 1800.0).abs() <= 1e-6 * 1800.0, "{v}");
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
            (14.95..=21.05).contains(&r) && (9.95..=16.05).contains(&p.z)
        },
        1e-6,
    );
    assert!(inside, "the bead left the corner");
    let pappus = 18.0 * 2.0 * std::f64::consts::PI * 17.0;
    assert!((v - pappus).abs() <= 1e-4 * pappus, "{v} against {pappus}");
}

/// The base runs 45 past the web's face: a leg of 50 along it is refused, naming the base's
/// top, with the leg that fits (the halving's last bracket at 45, a toe held to the face's
/// tolerance), which passes; both bodies unchanged.
#[test]
fn a_leg_past_the_other_body_s_face_is_refused_with_the_leg_that_fits() {
    let mut k = kernel();
    let (web, side, base, top) = tee(&mut k);
    let joint = line([45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let before = state(&k, &web, &base);
    let blend = FootBlend::of_faces(&k, &web, side, &base, top, &joint).unwrap();
    let err = blend
        .toes_on_bodies(&k, [&web, &base], [LEG, 50.0], &[])
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::FilletTooLarge, "{err}");
    assert!(format!("{err:?}").contains("LegPastFace"), "{err:?}");
    let fit = err.fit().unwrap();
    assert_eq!(fit.measure, FitMeasure::Leg, "{err}");
    assert!(
        (45.0 - 4.7e-6..=45.0 + 1e-4).contains(&fit.size),
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
