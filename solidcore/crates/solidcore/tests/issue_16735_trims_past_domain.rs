//! Regression tests for #16735's closure where a face's trims and its integration disagree
//! (adapted from #15946's `issue_14170_volume_closure` by @bherbruck).
//!
//! `brep_volume` closes the faces' gaps with strips from each trim's image to its edge's
//! curve. The volume `int (z - O_z) n_z dA` of a closed surface is origin-free; where the
//! faces' own boundaries are not the curves the strips start on, the surface leaks and the
//! volume moves with the origin by `C_z O_z`, `C` the leak's vector area. Two ways:
//!
//! - A loop the trim boundary leaves open (`create_oriented_trim_boundary` fills a uv gap
//!   only past `|d|^2 > EFF_ZERO (1 + |p|)`, about 1e-6): the face's Green integral loses
//!   the gap's shadow down to its base. The loops are now closed with uv lines. A cube's
//!   top with its right trim moved 1e-6 holds 1000 about every origin and moved 1e3 up.
//! - A trim past the domain the surface's evaluation clamps to: clamped, its image is the
//!   domain edge's point but its derivative the domain edge's partials, so the face and
//!   the strips integrate different curves. The face and its strips are now integrated on
//!   the surface continued past its domain where that is certified. A plane trimmed past
//!   its analytic domain, and a bilinear B-spline top trimmed past its knots, hold their
//!   closed forms about every origin, where clamping misses closed-form amounts.
//!
//! The 2907c hull agrees about six origins to the accuracy.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{
    brep_properties, MassPropertyBehavior, PreciseProperties, PropertyAttributes,
};
use solidcore::math::det;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::loopuse;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, FaceId};

const ACCURACY: f64 = 1e-11;

fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k
}

fn properties(k: &Kernel, b: &mut Brep, origin: Point3) -> PreciseProperties {
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

/// The volume about `origin` holds `exact` within the accuracy.
fn holds(k: &Kernel, b: &mut Brep, origin: Point3, exact: f64) {
    let v = properties(k, b, origin).volume;
    let miss = (v - exact).abs();
    assert!(
        miss <= ACCURACY * exact.abs(),
        "about {origin:?}: {v} misses {exact} by {miss:e}"
    );
}

/// A 10 cube, turned `deg` about x, whose top plane's analytic domain is cut short of its
/// trims by `cut` of its width at its u end, and at its v end too with `both`
/// (`Plane::adjust_step_uv_domain`): the body, its top and the top's plane before the cut.
fn cut_cube(deg: f64, cut: f64, both: bool) -> (Kernel, Brep, FaceId, Plane) {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    if deg != 0.0 {
        api_rotate(
            &mut k,
            Some(TransformObject::Brep(&mut b)),
            Point3::ZERO,
            Vec3::X,
            deg,
        )
        .unwrap();
    }
    // the trims made first, so the top keeps them
    let _ = properties(&k, &mut b, Point3::ZERO);
    let top = top_face(&k, &b, deg);
    let SurfaceKind::Plane(plane) = b.face(top).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("the top is not a plane")
    };
    let before = plane.copy();
    let dom = plane.step_uv_domain();
    let cut_u = cut * (dom.u_max() - dom.u_min());
    let cut_v = if both {
        cut * (dom.v_max() - dom.v_min())
    } else {
        0.0
    };
    let short = Extent2d::from_bounds(
        dom.u_min(),
        dom.v_min(),
        dom.u_max() - cut_u,
        dom.v_max() - cut_v,
    );
    let SurfaceKind::Plane(plane) = b
        .face_mut(top)
        .unwrap()
        .surface
        .as_mut()
        .unwrap()
        .kind_mut()
    else {
        panic!("the top is not a plane")
    };
    plane.adjust_step_uv_domain(&short).unwrap();
    (k, b, top, before)
}

/// The cube's top found again after `cut_cube` or `open_cube` (the face whose middle is the
/// turned (5, 5, 10)).
fn top_face(k: &Kernel, b: &Brep, deg: f64) -> FaceId {
    let (c, s) = (det::cos(deg.to_radians()), det::sin(deg.to_radians()));
    let top_mid = Point3::new(5.0, 5.0 * c - 10.0 * s, 5.0 * s + 10.0 * c);
    faces(b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let face = b.face(f).unwrap();
            let surface = face.surface.as_ref().unwrap();
            surface
                .evaluate_point(k, face.uv_domain.mid())
                .is_ok_and(|p| p.distance(top_mid) < 1e-9)
        })
        .unwrap()
}

/// The level 10 cube with its top's right trim (the pcurve at the domain's largest u) moved
/// `delta` further in u: the loop is open by `delta` at both its right corners, within the
/// trim boundary's gap test (`|d|^2 <= EFF_ZERO (1 + |p|)`). The body.
fn open_cube(delta: f64) -> (Kernel, Brep) {
    use solidcore::topology::edgeuse::p1::{
        faceuse_loopuses, loopuse_edgeuses, set_uv_trim_curve, uv_trim_curve, UvTrimCurveArg,
    };
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    // the trims made first, so the top keeps them
    let _ = properties(&k, &mut b, Point3::ZERO);
    let top = top_face(&k, &b, 0.0);
    let SurfaceKind::Plane(plane) = b.face(top).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("the top is not a plane")
    };
    let u_max = plane.step_uv_domain().u_max();
    let up = b.face(top).unwrap().faceuse.unwrap();
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
    (k, b)
}

/// The open cube: integrated open, the top would lose the shadow of the gap at its far v
/// end down to its base, `(10 - O_z) delta 10`. Its loop closed, the top is the cube's top
/// moved `delta` in u, and the strip along the moved edge folds it back to the side: the
/// cube's 1000 about every origin. Moved out (`delta` 1e-6), the trim also stands past the
/// plane's domain, and the face is integrated on the plane continued past it; moved in
/// (`delta` -1e-6), it stays inside.
#[test]
fn an_open_loop_is_integrated_closed() {
    for delta in [1e-6, -1e-6] {
        let (k, mut b) = open_cube(delta);
        let middle = Point3::new(5.0, 5.0, 5.0);
        for origin in [
            middle,
            middle + Vec3::new(0.0, 0.0, 100.0),
            middle - Vec3::new(0.0, 0.0, 100.0),
            Point3::ZERO,
        ] {
            holds(&k, &mut b, origin, 1000.0);
        }
    }
}

/// The open cube moved 1e3 up keeps its volume about the world origin; with the loop
/// integrated open it moved by the shadow's area per unit of z (1e-5, so 1e-2 here).
#[test]
fn an_open_loop_is_origin_independent() {
    for delta in [1e-6, -1e-6] {
        let (mut k, mut b) = open_cube(delta);
        let here = properties(&k, &mut b, Point3::ZERO).volume;
        api_translate(
            &mut k,
            Some(TransformObject::Brep(&mut b)),
            Vec3::new(0.0, 0.0, 1e3),
        )
        .unwrap();
        let there = properties(&k, &mut b, Point3::ZERO).volume;
        let allowed = 2.0 * ACCURACY * 1000.0 * 1e3;
        assert!(
            (here - there).abs() <= allowed,
            "delta {delta:e}: {here} moved 1e3 up: {there}"
        );
    }
}

/// The cube turned 30 degrees about x (its top tilted), its top's domain cut 1e-2 of its
/// width short of its trims at both ends. Clamped to the domain, as upstream's face
/// integration evaluates it, the top's flux misses about 2.2e-2. The body's volume
/// integrates the top on the plane continued past its domain, as its strips are built: the
/// cube's 1000 about every origin.
#[test]
fn a_plane_trimmed_past_its_domain_is_integrated_unclamped() {
    let (k, mut b, _, _) = cut_cube(30.0, 1e-2, true);
    let (c, s) = (
        det::cos(30.0_f64.to_radians()),
        det::sin(30.0_f64.to_radians()),
    );
    let middle = Point3::new(5.0, 5.0 * c - 5.0 * s, 5.0 * s + 5.0 * c);
    for origin in [
        middle,
        middle + Vec3::new(0.0, 0.0, 100.0),
        middle - Vec3::new(0.0, 0.0, 100.0),
    ] {
        holds(&k, &mut b, origin, 1000.0);
    }
}

/// A box whose top is a bilinear B-spline patch `z = 10 + eps (u - u0) (v - v0)` over the
/// top plane's own parameters, its u knots cut short of the trims by `cut`: the NURB path,
/// where the evaluation clamps to the knots' range. The patch continued is the same
/// bilinear map, so the closed body's volume is `1000 + 2500 eps` (scales 1; the side
/// walls' strips to the raised top edges are vertical, `n_z = 0`). Clamped, the top misses
/// `eps cut^2 / 2 * 50`, 0.25 here.
#[test]
fn a_nurb_trimmed_past_its_knots_is_integrated_unclamped() {
    nurb_trimmed_past_its_knots(false);
}

/// The same with the patch's out-of-bounds flag already on: that flag alone does not
/// continue the face integration's evaluation (it clamps unless the integration's own
/// continuation is on), so the continuation is still turned on, and the flag is left on.
#[test]
fn a_nurb_trimmed_past_its_knots_with_out_of_bounds_on_is_integrated_unclamped() {
    nurb_trimmed_past_its_knots(true);
}

/// [`a_nurb_trimmed_past_its_knots_is_integrated_unclamped`], the patch's out-of-bounds
/// flag set to `out_of_bounds` first.
fn nurb_trimmed_past_its_knots(out_of_bounds: bool) {
    use solidcore::nurbs::b_spline_curve::KnotType;
    use solidcore::nurbs::b_spline_surface::{
        BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
    };
    use solidcore::surface::surface::Surface;
    let (eps, cut) = (1e-2, 1.0);
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0).unwrap();
    let _ = properties(&k, &mut b, Point3::ZERO);
    let top = top_face(&k, &b, 0.0);
    let SurfaceKind::Plane(plane) = b.face(top).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("the top is not a plane")
    };
    let (plane, dom) = (plane.copy(), plane.step_uv_domain());
    let (u0, v0, v1) = (dom.u_min(), dom.v_min(), dom.v_max());
    let u1 = dom.u_max() - cut;
    let raised = |u: f64, v: f64| {
        let rise = eps * (u - u0) * (v - v0);
        plane.evaluate_point_fast(Point2::new(u, v)) + Vec3::new(0.0, 0.0, rise)
    };
    let corners = [
        raised(u0, v0),
        raised(u0, v1),
        raised(u1, v0),
        raised(u1, v1),
    ];
    let mut patch = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 1,
        v_degree: 1,
        control_points: &corners,
        form: BSplineSurfaceForm::default(),
        u_multiplicities: &[2, 2],
        v_multiplicities: &[2, 2],
        u_knots: &[u0, u1],
        v_knots: &[v0, v1],
        knot_type: KnotType::default(),
        weights: None,
        uv_domain: None,
    })
    .unwrap();
    *patch.out_of_bounds_enabled_mut() = out_of_bounds;
    b.face_mut(top).unwrap().surface = Some(Surface::from(patch));
    let want = 1000.0 + eps * 2500.0;
    for origin in [
        Point3::new(5.0, 5.0, 5.0),
        Point3::new(5.0, 5.0, 105.0),
        Point3::new(5.0, 5.0, -95.0),
    ] {
        let v = properties(&k, &mut b, origin).volume;
        assert!(
            (v - want).abs() <= ACCURACY * want,
            "about {origin:?}: {v} for {want} (clamped: {})",
            want - eps * cut * cut / 2.0 * 50.0
        );
    }
    let SurfaceKind::BSpline(s) = b.face(top).unwrap().surface.as_ref().unwrap().kind() else {
        panic!("the top is not the patch")
    };
    assert_eq!(
        s.is_out_of_bounds_enabled(),
        out_of_bounds,
        "the flag is restored"
    );
}

/// The 2907c hull (#14170: gaps up to 4.7e-6, its faces' flux alone 1.77e-2 apart between
/// the world origin and a point on it) agrees about the issue's six origins to the
/// accuracy.
#[test]
fn the_2907c_hull_agrees_about_every_origin() {
    let mut k = kernel();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let body = include_bytes!("data/issue_2907c_hull_rim.smb").to_vec();
    let mut b = read_brep_from_file(&mut k, Some(body), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    let mid = calculate_bounding_box(&k, &mut b, BoundingBoxOptions::default())
        .unwrap()
        .mid();
    let chain = Point3::new(470.5, 0.0, 538.1);
    let origins = [
        chain,
        chain + Vec3::new(50.0, 0.0, 0.0),
        chain + Vec3::new(0.0, 30.0, 0.0),
        chain - Vec3::new(0.0, 0.0, 50.0),
        mid,
        Point3::ZERO,
    ];
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for origin in origins {
        let v = properties(&k, &mut b, origin).volume;
        lo = lo.min(v);
        hi = hi.max(v);
    }
    assert!(hi - lo <= 2.0 * ACCURACY * hi, "spread {:e}", hi - lo);
}
