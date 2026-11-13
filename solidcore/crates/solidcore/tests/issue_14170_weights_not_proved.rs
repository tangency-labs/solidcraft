//! Regression test for SolidCraft #14170: a face trimmed past the domain its surface's
//! evaluation clamps to is integrated on the surface continued past it only where that
//! continuation is certified. Here it is not: a rational surface whose weights fall to and
//! below 0 past its knots. The face is integrated clamped, as upstream integrates it, and the
//! closure's bound is refused by name (`ClosureRefusal::WeightsNotProved`); the body's other
//! gaps are still closed, the strips not touching that face added to its volume.
#![allow(clippy::unwrap_used, clippy::panic)]

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::closure::{ClosureRefusal, ClosureRefused};
use solidcore::mass::integrator::properties::{
    brep_properties, face_properties, MassPropertyBehavior, PropertyAttributes, VolumeClosure,
};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p1::faces;
use solidcore::topology::edgeuse::p1::faceuse_region;
use solidcore::topology::Brep;
use solidcore::topology::FaceId;

const ACCURACY: f64 = 1e-11;

fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k
}

fn properties(
    k: &Kernel,
    b: &mut Brep,
    origin: Point3,
) -> solidcore::mass::integrator::properties::PreciseProperties {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_properties(
        k,
        b,
        ACCURACY,
        origin,
        MassPropertyBehavior::default(),
        &attrs,
    )
    .unwrap()
}

/// The face of `b` whose middle is at height `z`.
fn face_at(k: &Kernel, b: &Brep, z: f64) -> FaceId {
    faces(b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let face = b.face(f).unwrap();
            let surface = face.surface.as_ref().unwrap();
            surface
                .evaluate_point(k, face.uv_domain.mid())
                .is_ok_and(|p| (p.z - z).abs() < 1e-9)
        })
        .unwrap()
}

/// A 10 cube whose top is a rational bilinear patch over the top plane's own parameters,
/// its u knots `[u0, u0 + 9]` cut 1 short of the trims, its weights 1 at `u0` and 0.05 at the
/// cut. Continued past the cut the weight falls on linearly, to -0.056 at the trims' end, so
/// the enclosure cannot prove it positive there. With `delta` not 0, its bottom's trim at the
/// bottom plane's largest u moved `delta` further in u (as `issue_14170_volume_closure`'s
/// open cube moves its top's): the bottom's loop is open by `delta` at two corners, and the
/// edge there has a gap strip, between the bottom and a side, neither of them the top. The
/// kernel, the body, the top, and the volume the clamped top gives about the cube's middle
/// (`5, 5, 5`), in closed form: the top's evaluation clamps its uv to the knots, the point
/// and partials both, so past the cut it is the cut's line with its partials there, where
/// `S_u` is `w(u0) / w(u1) = 20` times its mean over the patch; with `f = 5` on the top and
/// the bottom, the sides' flux 0: `5 (s (9 + 20 (u_max - u1)) (v1 - v0) + 100)`, `s` the top
/// plane's uv scales (`(u_max - u1)` the 1 the cut leaves).
fn weighted_cube(delta: f64) -> (Kernel, Brep, FaceId, f64) {
    use solidcore::topology::edgeuse::p1::{
        faceuse_loopuses, loopuse_edgeuses, set_uv_trim_curve, uv_trim_curve, UvTrimCurveArg,
    };
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    // the trims made first, so the faces keep them
    let _ = properties(&k, &mut b, Point3::ZERO);
    let top = face_at(&k, &b, 10.0);
    let SurfaceKind::Plane(plane) = b.face(top).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("the top is not a plane")
    };
    let (plane, dom, scale) = (plane.copy(), plane.step_uv_domain(), plane.uv_scale());
    let (u0, v0, v1) = (dom.u_min(), dom.v_min(), dom.v_max());
    let u1 = u0 + 9.0;
    let s = (scale.x * scale.y).abs();
    let clamped = 5.0 * (s * ((u1 - u0) + 20.0 * (dom.u_max() - u1)) * (v1 - v0) + 100.0);
    let at = |u: f64, v: f64| plane.evaluate_point_fast(Point2::new(u, v));
    let corners = [at(u0, v0), at(u0, v1), at(u1, v0), at(u1, v1)];
    let patch = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 1,
        v_degree: 1,
        control_points: &corners,
        form: BSplineSurfaceForm::default(),
        u_multiplicities: &[2, 2],
        v_multiplicities: &[2, 2],
        u_knots: &[u0, u1],
        v_knots: &[v0, v1],
        knot_type: KnotType::default(),
        weights: Some(&[1.0, 1.0, 0.05, 0.05]),
        uv_domain: None,
    })
    .unwrap();
    b.face_mut(top).unwrap().surface = Some(Surface::from(patch));
    let bottom = face_at(&k, &b, 0.0);
    let SurfaceKind::Plane(plane) = b.face(bottom).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("the bottom is not a plane")
    };
    let u_max = plane.step_uv_domain().u_max();
    if delta != 0.0 {
        let up = b.face(bottom).unwrap().faceuse.unwrap();
        let mut moved = None;
        for lu in faceuse_loopuses(&b, up, false, None).unwrap() {
            for eu in loopuse_edgeuses(&b, lu).unwrap() {
                let Some(c) = uv_trim_curve(&b, eu).unwrap().0 else {
                    continue;
                };
                let iv = c.natural_interval();
                let (a, z) = (
                    c.evaluate_point(&k, iv.min()).unwrap(),
                    c.evaluate_point(&k, iv.max()).unwrap(),
                );
                if moved.is_none() && a.x == u_max && z.x == u_max {
                    let mut c = c.clone();
                    let mut shift = Axis2Placement::default();
                    shift.translate(Vec3::new(delta, 0.0, 0.0));
                    c.transform(&k, &shift, None).unwrap();
                    moved = Some((eu, c));
                }
            }
        }
        let (eu, c) = moved.unwrap();
        set_uv_trim_curve(
            &mut b,
            eu,
            UvTrimCurveArg::New(Box::new(c)),
            0.0,
            true,
            None,
        )
        .unwrap();
    }
    (k, b, top, clamped)
}

/// The volume's closure is refused by name, on the top.
fn refused_on(p: &solidcore::mass::integrator::properties::PreciseProperties, top: FaceId) {
    match p.closure {
        VolumeClosure::Refused(ClosureRefused {
            face: Some(f),
            why: ClosureRefusal::WeightsNotProved,
        }) => assert_eq!(f, top),
        other => panic!("{other:?}"),
    }
}

/// The faces' flux alone, each face integrated clamped through its material use, as
/// upstream's face integration takes it (`face_properties`), in the face loop's order and
/// with its estimated area (the body box's largest size squared over 100).
fn faces_flux(k: &Kernel, b: &mut Brep, origin: Point3) -> f64 {
    let size = solidcore::query::brep::calculate_bounding_box(
        k,
        &mut *b,
        solidcore::query::brep::BoundingBoxOptions::default(),
    )
    .unwrap()
    .size();
    let estimated = size.z.max(size.x.max(size.y));
    let estimated = estimated * estimated / 100.0;
    let mut flux = 0.0;
    for f in faces(b, None).unwrap() {
        let up = b.face(f).unwrap().faceuse.unwrap();
        let mate = b.faceuse(up).unwrap().mate.unwrap();
        let material = [up, mate]
            .into_iter()
            .find(|&fu| {
                faceuse_region(b, fu).unwrap().is_some_and(|r| {
                    solidcore::mass::integrator::properties::region_material(b, r).unwrap()
                })
            })
            .unwrap();
        let orientation = b.faceuse(material).unwrap().orientation;
        flux += face_properties(k, b, f, orientation, ACCURACY, origin, estimated, 0.0, 0x3f)
            .unwrap()
            .volume;
    }
    flux
}

/// Closed, the weighted cube's volume is its faces' flux, the clamped top's closed form: its
/// edges meet, so its strips (those not touching the top) add rounding only, within the
/// accuracy at its size.
#[test]
fn a_rational_face_past_its_knots_with_weights_not_proved_is_integrated_clamped() {
    let (k, mut b, top, clamped) = weighted_cube(0.0);
    let origin = Point3::new(5.0, 5.0, 5.0);
    let p = properties(&k, &mut b, origin);
    refused_on(&p, top);
    let flux = faces_flux(&k, &mut b, origin);
    let tol = ACCURACY * clamped;
    assert!(
        (p.volume - flux).abs() <= tol,
        "{} vs the faces' {flux}",
        p.volume
    );
    assert!(
        (p.volume - clamped).abs() <= tol,
        "{} vs the closed form {clamped}",
        p.volume
    );
}

/// The weighted cube with its bottom's loop open by `delta`: the top's refusal stops only the
/// top's continuation and its strips. The bottom's loop is still integrated closed and the
/// strip along its moved edge, which folds the bottom's moved part back to the side, still
/// added, so the volume is the closed cube's closed form (the case above). Without that strip
/// it would be off by the bottom's shadow, `|0 - O_z| delta 10 = 5e-5`.
#[test]
fn a_weights_refusal_keeps_the_other_faces_gaps_closed() {
    let origin = Point3::new(5.0, 5.0, 5.0);
    for delta in [1e-6, -1e-6] {
        let (k, mut b, top, clamped) = weighted_cube(delta);
        let p = properties(&k, &mut b, origin);
        refused_on(&p, top);
        assert!(
            (p.volume - clamped).abs() <= ACCURACY * clamped,
            "delta {delta:e}: {} for the closed form {clamped}",
            p.volume
        );
    }
}
