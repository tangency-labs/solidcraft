//! The level-0 helpers of the topology `AssertValid` walk (#1419, WP2) on concrete
//! bodies: `SmBSplineCurve::AssertNoPeriodicJump`, `SmEdgeuse::IsEdgeuseSideInSurface`
//! and `HasDogLegNearPole`, `SmLoopuse::IsLoopuseSideInSurface`,
//! `SmVertex::CheckPointers` and `sm_AssertTestFace37`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep as sm;
use solidcore::curve::curve::Curve;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::extent2d::{Extent2d, SS_NONE, SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::p3::assert_no_periodic_jump_of;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p2::{has_dog_leg_near_pole, is_edgeuse_side_in_surface};
use solidcore::topology::face::p4::assert_test_face37;
use solidcore::topology::loopuse::is_loopuse_side_in_surface;
use solidcore::topology::vertex::check_pointers;
use solidcore::topology::{Brep, EdgeuseOwner, VertexuseOwner};

/// A degree-1 UV polyline through `pts`.
fn polyline(pts: &[[f64; 2]]) -> BSplineCurve {
    let cps: Vec<Point3> = pts.iter().map(|p| Point3::new(p[0], p[1], 0.0)).collect();
    let n = cps.len();
    let knots: Vec<f64> = (0..n).map(|i| i as f64).collect();
    let mut mults = vec![1; n];
    mults[0] = 2;
    mults[n - 1] = 2;
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: 1,
        control_points: &cps,
        form: BSplineCurveForm::PolylineForm,
        knot_multiplicities: &mults,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap()
}

#[test]
fn assert_no_periodic_jump_flags_a_control_polygon_crossing_the_period() {
    let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    let ok = Curve::from(polyline(&[[0.1, 0.0], [0.95, 0.5], [0.9, 1.0]]));
    let jump = Curve::from(polyline(&[[0.0, 0.0], [0.95, 0.5], [0.9, 1.0]]));
    let jump_v = Curve::from(polyline(&[[0.5, 0.0], [0.5, 0.95], [0.4, 1.0]]));
    let two = Curve::from(polyline(&[[0.0, 0.0], [1.0, 1.0]]));
    assert!(assert_no_periodic_jump_of(&ok, true, true, &dom).unwrap());
    assert!(!assert_no_periodic_jump_of(&jump, true, false, &dom).unwrap());
    // the jump is in U: not closed in U is no jump
    assert!(assert_no_periodic_jump_of(&jump, false, true, &dom).unwrap());
    assert!(!assert_no_periodic_jump_of(&jump_v, false, true, &dom).unwrap());
    assert!(assert_no_periodic_jump_of(&jump, false, false, &dom).unwrap());
    // two control points are assumed not to jump
    assert!(assert_no_periodic_jump_of(&two, true, true, &dom).unwrap());
    // scaled domain: the same polygon in a 1e3-scaled domain
    let big = Extent2d::from_bounds(0.0, 0.0, 1e3, 1e3);
    let jump_big = Curve::from(polyline(&[[0.0, 0.0], [950.0, 500.0], [900.0, 1e3]]));
    assert!(!assert_no_periodic_jump_of(&jump_big, true, false, &big).unwrap());
    // a 3D curve is never checked
    let line3 = Curve::from(
        BSplineCurve::create_line_segment(3, Point3::ZERO, Point3::new(1.0, 0.0, 0.0)).unwrap(),
    );
    assert!(assert_no_periodic_jump_of(&line3, true, true, &dom).unwrap());
}

/// Whether the edge use lies on its face's upward face use (its loops run
/// counter-clockwise around the face in UV).
fn on_upward_faceuse(b: &Brep, eu: solidcore::topology::EdgeuseId) -> bool {
    let fu = eu1::faceuse(b, eu).unwrap().unwrap();
    let f = b.faceuse(fu).unwrap().face.unwrap();
    b.face(f).unwrap().faceuse == Some(fu)
}

fn all_checks_pass(k: &mut Kernel, b: &mut Brep) {
    for v in b.vertices().ids().collect::<Vec<_>>() {
        assert!(check_pointers(b, v).unwrap(), "vertex {v:?}");
    }
    for eu in b.edgeuses().ids().collect::<Vec<_>>() {
        if !matches!(b.edgeuse(eu).unwrap().owner, EdgeuseOwner::Loopuse(_)) {
            continue;
        }
        assert!(
            !has_dog_leg_near_pole(k, b, eu).unwrap().has_dog_leg,
            "edgeuse {eu:?}"
        );
        if !on_upward_faceuse(b, eu) {
            continue;
        }
        assert!(
            is_edgeuse_side_in_surface(k, b, eu, 6, None, None)
                .unwrap()
                .in_surface,
            "edgeuse {eu:?}"
        );
    }
    // the loops' primary loop uses (what SmLoop::AssertValid test 2 checks)
    for l in b.loops().ids().collect::<Vec<_>>() {
        let lu = b.loop_(l).unwrap().loopuse.unwrap();
        assert!(
            is_loopuse_side_in_surface(k, b, lu, None, None)
                .unwrap()
                .in_surface
        );
    }
    for f in b.faces().ids().collect::<Vec<_>>() {
        assert!(assert_test_face37(k, b, f).unwrap(), "face {f:?}");
    }
}

#[test]
fn valid_primitives_pass_every_level0_helper() {
    let mut k = Kernel::default();
    for scale in [1e-3, 1.0, 1e3] {
        let mut boxed =
            create_box(&mut k, Point3::ZERO, 2.0 * scale, 3.0 * scale, 4.0 * scale).unwrap();
        all_checks_pass(&mut k, &mut boxed);
        let mut sphere = create_sphere(&mut k, Point3::new(1.0, 2.0, 3.0), 5.0 * scale).unwrap();
        all_checks_pass(&mut k, &mut sphere);
        let mut cyl = create_cylinder(&mut k, Point3::ZERO, 2.0 * scale, 7.0 * scale).unwrap();
        all_checks_pass(&mut k, &mut cyl);
    }
}

#[test]
fn seam_edgeuses_run_on_a_natural_boundary() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 2.0, 7.0).unwrap();
    let mut on_boundary = 0;
    for eu in b.edgeuses().ids() {
        if !matches!(b.edgeuse(eu).unwrap().owner, EdgeuseOwner::Loopuse(_))
            || !on_upward_faceuse(&b, eu)
        {
            continue;
        }
        let side = is_edgeuse_side_in_surface(&k, &b, eu, 6, None, None).unwrap();
        assert!(side.in_surface);
        on_boundary += usize::from(side.on_natural_boundary);
        // forcing the periodic flags changes nothing for a valid body
        let forced = is_edgeuse_side_in_surface(&k, &b, eu, 6, Some(true), Some(false)).unwrap();
        assert!(forced.in_surface);
    }
    assert!(
        on_boundary > 0,
        "no edge use of the cylinder runs on its domain boundary"
    );
}

#[test]
fn a_uv_trim_curve_turning_along_a_pole_is_a_dog_leg() {
    let mut k = Kernel::default();
    let mut b = create_sphere(&mut k, Point3::ZERO, 5.0).unwrap();
    // an edge use of the sphere face with its UV trim curve holder
    let (eu, holder) = b
        .edgeuses()
        .ids()
        .find_map(|eu| match eu1::uv_trim_curve(&b, eu).unwrap() {
            (Some(_), Some(h)) => Some((eu, h)),
            _ => None,
        })
        .expect("a sphere edge use with a UV trim curve");
    let f = eu1::face(&b, eu).unwrap().unwrap();
    let surface = b.face(f).unwrap().surface.clone().unwrap();
    let tol = b.face(f).unwrap().tolerance;
    let sing = surface
        .get_singularities_of_face(&k, tol, false, false)
        .unwrap()
        .singular;
    assert_ne!(sing, SS_NONE);
    let d = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let (u0, u1, v0, v1) = (d.u_min(), d.u_max(), d.v_min(), d.v_max());
    let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let (lu, lv) = ((u1 - u0) / 10.0, (v1 - v0) / 10.0);
    // down to the pole, along it in U, back up: a turn at the pole
    let pts = if sing & SS_VMIN != 0 {
        [[um, vm], [um, v0], [um + lu, v0], [um + lu, vm]]
    } else if sing & SS_VMAX != 0 {
        [[um, vm], [um, v1], [um + lu, v1], [um + lu, vm]]
    } else if sing & SS_UMIN != 0 {
        [[um, vm], [u0, vm], [u0, vm + lv], [um, vm + lv]]
    } else {
        assert_ne!(sing & SS_UMAX, 0);
        [[um, vm], [u1, vm], [u1, vm + lv], [um, vm + lv]]
    };
    b.edgeuse_mut(holder).unwrap().uv_trim_curve = Some(Curve::from(polyline(&pts)));
    let dog = has_dog_leg_near_pole(&k, &b, eu).unwrap();
    // a U-direction pole run (VMIN/VMAX) turns at control point 1; a U pole is a V run,
    // which the U-direction test does not see
    if sing & (SS_VMIN | SS_VMAX) != 0 {
        assert!(dog.has_dog_leg);
        assert_eq!(dog.index, Some(1));
    }
    // a straight trim across the domain away from the poles is no dog leg
    b.edgeuse_mut(holder).unwrap().uv_trim_curve = Some(Curve::from(polyline(&[
        [um, vm],
        [um + lu, vm + lv],
        [um + 2.0 * lu, vm],
    ])));
    assert!(!has_dog_leg_near_pole(&k, &b, eu).unwrap().has_dog_leg);
}

#[test]
fn check_pointers_fails_on_a_broken_mate_link() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
    let v = b.vertices().ids().next().unwrap();
    assert!(check_pointers(&b, v).unwrap());
    let vu = b.vertex(v).unwrap().vertexuses[0];
    let VertexuseOwner::Edgeuse(Some(eu)) = b.vertexuse(vu).unwrap().owner else {
        panic!("box vertex use is not an edge vertex use");
    };
    let lu = eu1::loopuse(&b, eu).unwrap().unwrap();
    b.loopuse_mut(lu).unwrap().mate = None;
    assert!(!check_pointers(&b, v).unwrap());
}

#[test]
fn face37_fails_when_the_outer_loop_is_not_first() {
    let mut k = Kernel::default();
    let block = create_box(&mut k, Point3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let pin = create_cylinder(&mut k, Point3::new(10.0, 10.0, -5.0), 3.0, 30.0).unwrap();
    let mut b = sm::boolean(&mut k, block, pin, BooleanOperation::Difference)
        .unwrap()
        .result;
    let holed: Vec<_> = b
        .faces()
        .ids()
        .filter(|&f| {
            solidcore::topology::face::p2::loops(&b, f, false, None)
                .unwrap()
                .len()
                == 2
        })
        .collect();
    assert_eq!(holed.len(), 2, "the pierced box has two faces with a hole");
    for &f in &holed {
        assert!(assert_test_face37(&k, &mut b, f).unwrap());
    }
    // the hole listed first: the square is then "inside" the circle, which it is not
    let f = holed[0];
    let fu = b.face(f).unwrap().faceuse.unwrap();
    b.faceuse_mut(fu).unwrap().loopuses.reverse();
    assert!(!assert_test_face37(&k, &mut b, f).unwrap());
}

#[test]
fn the_downward_face_use_of_a_box_face_has_its_side_outside_the_surface() {
    // the mate face use's loops run clockwise in UV, so an edge use along a plane's
    // domain side has its inside side out of the domain
    let mut k = Kernel::default();
    let b = create_box(&mut k, Point3::ZERO, 2.0, 3.0, 4.0).unwrap();
    let mut outside = 0;
    for eu in b.edgeuses().ids() {
        if on_upward_faceuse(&b, eu) {
            continue;
        }
        let side = is_edgeuse_side_in_surface(&k, &b, eu, 6, None, None).unwrap();
        if !side.in_surface {
            assert!(side.on_natural_boundary);
            outside += 1;
        }
    }
    assert!(outside > 0);
    // the loop use reports the first failing edge use
    let lu = b
        .loopuses()
        .ids()
        .find(|&lu| {
            !is_loopuse_side_in_surface(&k, &b, lu, None, None)
                .unwrap()
                .in_surface
        })
        .expect("a box loop use with its side outside");
    let side = is_loopuse_side_in_surface(&k, &b, lu, None, None).unwrap();
    let eus = solidcore::topology::loopuse::edgeuses(&b, lu, None, None).unwrap();
    let first_bad = eus
        .iter()
        .position(|&eu| {
            !is_edgeuse_side_in_surface(&k, &b, eu, 6, None, None)
                .unwrap()
                .in_surface
        })
        .unwrap();
    assert_eq!(side.edgeuse_index, first_bad);
    assert!(side.on_natural_boundary);
}
