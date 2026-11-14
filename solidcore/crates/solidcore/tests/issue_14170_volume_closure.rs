//! Regression tests for SolidCraft #14170: a whole-body volume asked at 1e-11 missed its
//! accuracy by up to 300 times, by an amount that depends on the origin it is integrated
//! about.
//!
//! The cause is not cancellation about a far origin: closed bodies far from the origin are
//! exact. It is a flux surface that does not close. Where two faces' boundaries along an
//! edge are apart, the volume `int (z - O_z) n_z dA` over the faces changes with the origin
//! by `C_z O_z`, `C` the open surface's vector area, whatever the accuracy (the 2907c hull:
//! gaps up to 4.7e-6, `C_z` 9.8e-5, 1.77e-2 between the world origin and a point on it).
//! `brep_properties` now closes the faces with the strips across their gaps (as the
//! certified enclosure does) and reports a proved bound on what is still open
//! (`PreciseProperties::closure`):
//!
//! - A box with its top face tilted about one of its edges (its trims kept): the faces and
//!   strips bound the box plus a prism, `a b h + a b dz`, `dz` the top's rise at its middle.
//!   The volume holds that about the world origin, the box's middle and a point 1e4 away.
//! - A box and a cylinder translated far from the origin hold their closed forms, with a
//!   closure bound of the order of rounding (their faces meet).
//! - The 2907c hull's volume about the issue's six origins agrees within the accuracy plus
//!   twice the closure bound.
//! - A loop the trim boundary leaves open (a cube's top with its right trim moved 1e-6 off
//!   its corners, as the `issue_1476` shell's wall in its own test) is integrated closed:
//!   the cube holds its 1000 about every origin and moved 1e3 up, the strip along the moved
//!   edge carries its closed-form flux, and its enclosure holds the estimate.
//! - A plane trimmed past its analytic domain (a tilted cube's top cut short of its trims),
//!   and a bilinear B-spline top cut short of its knots, are integrated on their surfaces
//!   continued past the domain, as the closure builds them: the cube's 1000 and the patch
//!   box's `1000 + 2500 eps` about every origin, where clamping misses closed-form amounts.
//! - A fin inside the material refuses the closure, naming itself
//!   (`ClosureRefusal::NotOneMaterialSide`). (An edge whose sides do not pair widens the
//!   bound instead: `mass::closure`'s unit test.)
#![allow(clippy::unwrap_used, clippy::panic)]

use solidcore::blend::fillet_section::interval::Ivl;
use solidcore::boolean::api_brep::boolean_merge;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::closure::{ClosureRefusal, ClosureRefused};
use solidcore::mass::integrator::properties::{
    brep_properties, ClosureBound, MassPropertyBehavior, PreciseProperties, PropertyAttributes,
    VolumeClosure,
};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_plane};
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

/// The closure bound of a closed body's properties.
fn bound(p: &PreciseProperties) -> ClosureBound {
    match p.closure {
        VolumeClosure::Bounded(c) => c,
        other => panic!("not closed: {other:?}"),
    }
}

/// The volume about `origin` holds `exact` within the accuracy and its closure bound.
fn holds(k: &Kernel, b: &mut Brep, origin: Point3, exact: f64) -> PreciseProperties {
    let p = properties(k, b, origin);
    let bound = bound(&p).volume;
    let miss = (p.volume - exact).abs();
    assert!(
        miss <= ACCURACY * exact.abs() + bound,
        "about {origin:?}: {} misses {exact} by {miss:e} (bound {bound:e})",
        p.volume
    );
    p
}

/// The volume's cap bound is its closed form: the caps' area at most `L^2 / 4` times
/// `max |f|` over them, `f = z - O_z` (the volume's field), every cap point within `L` of a
/// point on the body: at most `(R + L) L^2 / 4`, `R` the body box's greatest `|z - O_z|`,
/// the right side in outward-rounded intervals (its upper end, no slack).
fn assert_cap_formula(k: &Kernel, b: &mut Brep, origin: Point3, p: &PreciseProperties) {
    let c = bound(p);
    let bbox = calculate_bounding_box(k, b, BoundingBoxOptions::default()).unwrap();
    let oz = Ivl::from(origin.z);
    let r = (Ivl::from(bbox.max().z) - oz).hull(Ivl::from(bbox.min().z) - oz);
    let l = Ivl::from(c.open_length);
    let formula = ((Ivl::from(r.mag()) + l) * l * l / Ivl::from(4.0)).hi;
    assert!(
        c.volume >= 0.0 && c.volume <= formula,
        "{} above (R + L) L^2 / 4 = {formula:e} (R {r:?}, L {:e})",
        c.volume,
        c.open_length
    );
}

#[test]
fn a_tilted_top_is_closed_by_its_strips_about_every_origin() {
    let mut k = kernel();
    let (o, a, w, h) = (Point3::new(270.0, -130.0, 340.0), 180.0, 120.0, 240.0);
    let mut b = create_box(&mut k, o, a, w, h).unwrap();
    // the trims made first, so the tilted top keeps them
    let _ = properties(&k, &mut b, Point3::ZERO);
    let top = faces(&b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            s.evaluate_point(&k, face.uv_domain.mid())
                .is_ok_and(|p| (p.z - (o.z + h)).abs() < 1e-9)
        })
        .unwrap();
    // tilted by 1e-7 about its edge x = o.x: its far edge rises by a sin(1e-7)
    let mut tilt = Axis2Placement::default();
    tilt.rotate_about_axis_at_point(
        1e-7,
        Point3::new(o.x, o.y, o.z + h),
        Vec3::new(0.0, -1.0, 0.0),
    );
    b.face_mut(top)
        .unwrap()
        .surface
        .as_mut()
        .unwrap()
        .transform(&k, &tilt, None)
        .unwrap();
    let face = b.face(top).unwrap();
    let mid = face
        .surface
        .as_ref()
        .unwrap()
        .evaluate_point(&k, face.uv_domain.mid())
        .unwrap();
    let dz = mid.z - (o.z + h);
    assert!(dz > 0.0 && (dz - 0.5 * a * 1e-7).abs() < 1e-12, "{dz}");
    let exact = a * w * h + a * w * dz;
    let middle = Point3::new(o.x + a / 2.0, o.y + w / 2.0, o.z + h / 2.0);
    for origin in [Point3::ZERO, middle, middle + Vec3::new(1e4, -1e4, 1e4)] {
        let p = holds(&k, &mut b, origin, exact);
        assert_cap_formula(&k, &mut b, origin, &p);
        assert!(bound(&p).volume < ACCURACY * exact, "{p:?}");
    }
}

#[test]
fn closed_bodies_far_from_the_origin_are_exact() {
    for off in [Vec3::new(270.0, -130.0, 340.0), Vec3::new(3e4, -2e4, 4e4)] {
        let mut k = kernel();
        let o = Point3::ZERO + off;
        let mut bx = create_box(&mut k, o, 180.0, 120.0, 240.0).unwrap();
        let mut cy = create_cylinder(&mut k, o, 90.0, 240.0).unwrap();
        for origin in [Point3::ZERO, o] {
            let p = holds(&k, &mut bx, origin, 180.0 * 120.0 * 240.0);
            // the faces meet (what is open is rounding), and the bound is its formula
            assert_cap_formula(&k, &mut bx, origin, &p);
            holds(&k, &mut cy, origin, PI * 90.0 * 90.0 * 240.0);
        }
    }
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
    let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
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
/// trim boundary's gap test (`|d|^2 <= EFF_ZERO (1 + |p|)`), so the face loop integrates it
/// open. The body, the top and its plane.
fn open_cube(delta: f64) -> (Kernel, Brep, FaceId, Plane) {
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
    let plane = plane.copy();
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
    // the top's right trim
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
    (k, b, top, plane)
}

/// The open cube (#14170: as the `issue_1476` shell's wall, open by 1.5e-6 at two corners):
/// integrated open, the top would lose the shadow of the gap at its far v end down to its
/// base, `delta` by its v extent: `(10 - O_z) delta 10` (the top level, its scales 1). Its
/// loop closed, the top is the cube's top moved `delta` in u, and the strip along the moved
/// edge folds it back to the side: the strips' volume is that strip's flux,
/// `-(10 - O_z) delta 10`, and the volume is the cube's 1000 about every origin, both to the
/// accuracy at the cube's size. Moved out (`delta` 1e-6), the trim also stands past the
/// plane's domain, and the face is integrated on the plane continued past it; moved in
/// (`delta` -1e-6), it stays inside.
#[test]
fn an_open_loop_is_integrated_closed() {
    for delta in [1e-6, -1e-6] {
        let (k, mut b, _, plane) = open_cube(delta);
        let scale = plane.uv_scale();
        let dom = plane.step_uv_domain();
        let shadow = delta * scale.x.abs() * (dom.v_max() - dom.v_min()) * scale.y.abs();
        let middle = Point3::new(5.0, 5.0, 5.0);
        for origin in [
            middle,
            middle + Vec3::new(0.0, 0.0, 100.0),
            middle - Vec3::new(0.0, 0.0, 100.0),
            Point3::ZERO,
        ] {
            let p = holds(&k, &mut b, origin, 1000.0);
            assert_cap_formula(&k, &mut b, origin, &p);
            let c = bound(&p);
            let want = -(10.0 - origin.z) * shadow;
            assert!(
                (c.strip_volume - want).abs() <= ACCURACY * 1000.0,
                "delta {delta:e} about {origin:?}: strips {:e} for the moved edge's {want:e}",
                c.strip_volume
            );
        }
    }
}

/// The open cube moved 1e3 up keeps its volume about the world origin; with the loop
/// integrated open it moved by the shadow's area per unit of z (1e-5, so 1e-2 here).
#[test]
fn an_open_loop_is_origin_independent() {
    for delta in [1e-6, -1e-6] {
        open_loop_moved_up(delta);
    }
}

/// [`an_open_loop_is_origin_independent`] for one `delta`.
fn open_loop_moved_up(delta: f64) {
    let (mut k, mut b, _, _) = open_cube(delta);
    let here = properties(&k, &mut b, Point3::ZERO);
    api_translate(
        &mut k,
        Some(TransformObject::Brep(&mut b)),
        Vec3::new(0.0, 0.0, 1e3),
    )
    .unwrap();
    let there = properties(&k, &mut b, Point3::ZERO);
    let allowed = bound(&here).volume + bound(&there).volume + 2.0 * ACCURACY * 1000.0 * 1e3;
    assert!(
        (here.volume - there.volume).abs() <= allowed,
        "{} moved 1e3 up: {}",
        here.volume,
        there.volume
    );
}

/// The open cube's certified sign and enclosure agree with its estimate, which integrates the
/// loop closed as the enclosure does: the sign is positive, and the enclosure at 1e-6 holds
/// the volume (within the estimate's accuracy).
#[test]
fn an_open_loop_s_enclosure_holds_its_estimate() {
    use solidcore::mass::enclosure::{enclose_volume, volume_sign, VolumeSign};
    let (k, mut b, _, _) = open_cube(1e-6);
    assert!(matches!(
        volume_sign(&k, &mut b).unwrap(),
        VolumeSign::Positive(_)
    ));
    let e = enclose_volume(&k, &mut b, 1e-6).unwrap();
    let v = properties(&k, &mut b, Point3::new(5.0, 5.0, 5.0)).volume;
    let slack = ACCURACY * 1000.0;
    assert!(
        e.lo - slack <= v && v <= e.hi + slack,
        "{v} outside [{}, {}]",
        e.lo,
        e.hi
    );
}

/// The cube turned 30 degrees about x (its top tilted), its top's domain cut 1e-2 of its
/// width short of its trims at both ends. Clamped to the domain, as upstream's face
/// integration evaluates it (`face_properties`), the top keeps its plane's area element and
/// takes its fields at the clamped points, so its flux misses
/// `n_z int (z(p) - z(clamp p)) dA` over the part cut off:
/// `n_z s_u s_v (z_u cut_u^2 / 2 L_v + z_v cut_v^2 / 2 L_u)` (`z` linear on the plane, `z_u`,
/// `z_v` its rates in u and v, `L` the trims' extents; 2.2e-2 here). The body's volume
/// integrates the top on the plane continued past its domain, as the closure builds it: the
/// cube's 1000 about every origin.
#[test]
fn a_plane_trimmed_past_its_domain_is_integrated_unclamped() {
    use solidcore::mass::integrator::properties::face_properties;
    use solidcore::topology::face::p2::PPF_ALL;
    let cut = 1e-2;
    let (k, mut b, top, plane) = cut_cube(30.0, cut, true);
    let dom = plane.step_uv_domain();
    let scale = plane.uv_scale();
    let (lu, lv) = (dom.u_max() - dom.u_min(), dom.v_max() - dom.v_min());
    let (cut_u, cut_v) = (cut * lu, cut * lv);
    let at = |u: f64, v: f64| plane.evaluate_point_fast(Point2::new(u, v)).z;
    let (u0, v0) = (dom.u_min(), dom.v_min());
    let (z_u, z_v) = (at(u0 + 1.0, v0) - at(u0, v0), at(u0, v0 + 1.0) - at(u0, v0));
    let n_z = 30.0_f64.to_radians().cos();
    let missed = n_z
        * scale.x.abs()
        * scale.y.abs()
        * (z_u * cut_u * cut_u / 2.0 * lv + z_v * cut_v * cut_v / 2.0 * lu);
    let (c, s) = (30.0_f64.to_radians().cos(), 30.0_f64.to_radians().sin());
    let middle = Point3::new(5.0, 5.0 * c - 5.0 * s, 5.0 * s + 5.0 * c);
    let top_mid_z = 5.0 * s + 10.0 * c;
    for origin in [
        middle,
        middle + Vec3::new(0.0, 0.0, 100.0),
        middle - Vec3::new(0.0, 0.0, 100.0),
    ] {
        let p = holds(&k, &mut b, origin, 1000.0);
        assert_cap_formula(&k, &mut b, origin, &p);
        // the top clamped, through either use: the whole top's flux, its area 100 at its
        // middle's height, less what clamping misses
        let whole = 100.0 * n_z * (top_mid_z - origin.z);
        let up = b.face(top).unwrap().faceuse.unwrap();
        let orientation = b.faceuse(up).unwrap().orientation;
        let clamped = face_properties(
            &k,
            &mut b,
            top,
            orientation,
            ACCURACY,
            origin,
            100.0,
            0.0,
            PPF_ALL,
        )
        .unwrap()
        .volume;
        assert!(
            (clamped.abs() - (whole - missed).abs()).abs() <= ACCURACY * 1000.0,
            "about {origin:?}: the clamped top {clamped} for {}",
            whole - missed
        );
    }
}

/// A box whose top is a bilinear B-spline patch `z = 10 + eps (u - u0) (v - v0)` over the
/// top plane's own parameters (its point at `(u, v)` the plane's, raised), its u knots cut
/// short of the trims by `cut`: the NURB path, where the evaluation clamps to the knots'
/// range. The patch continued is the same bilinear map, so the closed body's volume is
/// `1000 + eps int int (u - u0)(v - v0) du dv = 1000 + 2500 eps` (scales 1; the side walls'
/// strips to the raised top edges are vertical, `n_z = 0`). Clamped, the top misses
/// `eps int int (u - u1)^+ (v - v0) = eps cut^2 / 2 * 50`, 0.25 here. The volume is held to
/// that closed form at the accuracy alone: the closure's cap bound covers the raised edges'
/// openings, up to 108 here, and would hide the 0.25.
#[test]
fn a_nurb_trimmed_past_its_knots_is_integrated_unclamped() {
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
        weights: None,
        uv_domain: None,
    })
    .unwrap();
    b.face_mut(top).unwrap().surface = Some(Surface::from(patch));
    let want = 1000.0 + eps * 2500.0;
    for origin in [
        Point3::new(5.0, 5.0, 5.0),
        Point3::new(5.0, 5.0, 105.0),
        Point3::new(5.0, 5.0, -95.0),
    ] {
        let p = properties(&k, &mut b, origin);
        assert!(
            (p.volume - want).abs() <= ACCURACY * want,
            "about {origin:?}: {} for {want} (clamped: {})",
            p.volume,
            want - eps * cut * cut / 2.0 * 50.0
        );
    }
}

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
    let mut worst = 0.0_f64;
    for origin in origins {
        let p = properties(&k, &mut b, origin);
        lo = lo.min(p.volume);
        hi = hi.max(p.volume);
        worst = worst.max(bound(&p).volume);
    }
    // each within the accuracy and its bound of the closed surface's volume
    assert!(
        hi - lo <= 2.0 * (ACCURACY * hi + worst),
        "spread {:e}, bound {worst:e}",
        hi - lo
    );
}

/// A fin inside the material (one region on both sides) cannot be part of a closed
/// surface: the closure is refused, naming it, and the volume's accuracy is not proved.
#[test]
fn a_fin_refuses_the_closure_and_names_itself() {
    let mut k = kernel();
    let a = create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0).unwrap();
    let fin = create_plane(&mut k, Point3::new(0.25, 0.5, 1.5), 0.5, 0.5).unwrap();
    let mut b = boolean_merge(&mut k, a, fin).unwrap();
    let p = properties(&k, &mut b, Point3::ZERO);
    match p.closure {
        VolumeClosure::Refused(ClosureRefused {
            face: Some(f),
            why: ClosureRefusal::NotOneMaterialSide,
        }) => {
            let (x, y) = solidcore::topology::face::p2::faceuses(&b, f).unwrap();
            let region = |u: Option<solidcore::topology::FaceuseId>| {
                let sh = b.faceuse(u.unwrap()).unwrap().shell.unwrap();
                b.shell(sh).unwrap().region.unwrap()
            };
            assert_eq!(region(x), region(y), "the refused face is the fin");
        }
        other => panic!("{other:?}"),
    }
}
