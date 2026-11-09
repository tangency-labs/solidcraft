//! tangency-labs/solidcore#1449: `SmApiCreateTaperExtrude`'s draft-corner rules (SolidCraft
//! fork, tangency-labs/smlib 54148db and e8f23e0).
//!
//! - `OffsetCornerType::CurveExtension` (WR-520): each wall continues as itself (a line's
//!   plane, an arc's cone) to meet its neighbour along their exact seam, and a corner that
//!   cannot be closed is refused with why and where. It is a new corner rule, so selecting
//!   it is the opt-in.
//! - `Kernel::fixes.taper_tangent_corner_planes` (WR-521, off by default): linear-extension
//!   corners of a profile of lines and arcs continue the walls as their tangent planes
//!   through the same construction (a drafted rectangle has 6 faces, not 14) and refuse a
//!   corner they cannot close. Off, the result is SMLib's.
//!
//! The issue's repro is a strip 20 x 2 drafted 10 degrees inward over a height of 10: the
//! long walls' offsets (10 tan 10 = 1.76) meet across the short walls (half-width 1) before
//! the far end. SMLib builds a 5-face closed solid (a ridge, no far cap); the fork refuses
//! it (`SM_TCR_WALL_CLOSES`) at the first corner of a short wall.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::sweep::api_brep::{
    create_taper_extrude, create_taper_extrude_with_corner, OffsetCornerType, TaperCornerRefusal,
    TaperExtrude,
};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::{Brep, SolidError, Vec3};

fn p(x: f64, y: f64) -> Vec3 {
    Vec3::new(x, y, 0.0)
}

fn polygon(pts: &[Vec3]) -> Vec<Curve> {
    (0..pts.len())
        .map(|i| {
            Curve::from(
                Line::create_line_segment(3, pts[i], pts[(i + 1) % pts.len()], None).unwrap(),
            )
        })
        .collect()
}

/// The issue's strip, 20 x 2.
fn strip() -> Vec<Curve> {
    polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 2.0), p(0.0, 2.0)])
}

/// A rectangle 20 x 10.
fn rectangle() -> Vec<Curve> {
    polygon(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0), p(0.0, 10.0)])
}

/// A half disc of radius 20: its diameter and an `SmCircle` arc over the top.
fn half_disc(k: &Kernel) -> Vec<Curve> {
    let line = Line::create_line_segment(3, p(-20.0, 0.0), p(20.0, 0.0), None).unwrap();
    let arc = Ellipse::circle_from_3_points(k, p(20.0, 0.0), p(0.0, 20.0), p(-20.0, 0.0), 3, false)
        .unwrap();
    vec![Curve::from(line), Curve::from(arc)]
}

fn kernel(fix: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.taper_tangent_corner_planes = fix;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-6, &attributes).unwrap()
}

fn taper(
    k: &mut Kernel,
    curves: &[Curve],
    draft_deg: f64,
    corner: OffsetCornerType,
) -> TaperExtrude {
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude_with_corner(k, &refs, 10.0, draft_deg, 3, corner).unwrap()
}

/// The taper of `curves`, a closed manifold solid: its face count and volume.
fn built(
    k: &mut Kernel,
    curves: &[Curve],
    draft_deg: f64,
    corner: OffsetCornerType,
) -> (usize, f64) {
    let result = taper(k, curves, draft_deg, corner);
    solid(k, result)
}

/// A closed manifold solid: its face count and volume.
fn solid(k: &Kernel, result: TaperExtrude) -> (usize, f64) {
    let TaperExtrude::Solid(mut b) = result else {
        panic!("refused: {result:?}");
    };
    assert!(is_manifold_solid(&b).unwrap());
    (b.face_list().len(), volume(k, &mut b))
}

fn assert_rel(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * a.abs().max(b.abs()), "{a} vs {b}");
}

/// A rectangle a x b with planar walls drafted by offset o over the height h:
/// h (ab + s o (a + b) + 4 o^2 / 3), s = +1 outward, -1 inward.
fn drafted_rectangle_volume(outward: bool) -> f64 {
    let (a, b, h) = (20.0_f64, 10.0_f64, 10.0_f64);
    let o = h * 10.0_f64.to_radians().tan();
    let s = if outward { 1.0 } else { -1.0 };
    h * (a * b + s * o * (a + b) + 4.0 * o * o / 3.0)
}

#[test]
fn the_tangent_corner_planes_fix_is_off_by_default() {
    assert!(!Kernel::default().fixes.taper_tangent_corner_planes);
}

/// The issue's repro without the field: SMLib's 5-face solid, through both entry points.
#[test]
fn without_the_fix_the_strip_is_smlibs_ridge() {
    let mut k = kernel(false);
    let curves = strip();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let b = create_taper_extrude(&mut k, &refs, 10.0, 10.0, 3, false).unwrap();
    assert!(is_manifold_solid(&b).unwrap() && b.face_list().len() == 5);
    let (faces, _) = built(&mut k, &curves, 10.0, OffsetCornerType::LinearExtension);
    assert_eq!(faces, 5);
}

/// The issue's repro with the field: refused, the wall closes at the first corner of a
/// short wall; the boolean entry point reports it as `SM_ERR_INVALID_INPUT`.
#[test]
fn with_the_fix_the_strip_is_refused_where_its_short_wall_closes() {
    let mut k = kernel(true);
    let curves = strip();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let err = create_taper_extrude(&mut k, &refs, 10.0, 10.0, 3, false).unwrap_err();
    assert!(matches!(err, SolidError::InvalidInput { .. }), "{err}");
    let TaperExtrude::Refused(refusal) =
        taper(&mut k, &curves, 10.0, OffsetCornerType::LinearExtension)
    else {
        panic!("built");
    };
    assert_eq!(refusal.reason, TaperCornerRefusal::WallCloses);
    assert_eq!(refusal.corner, p(20.0, 0.0));
    // fillet corners are not affected by the field
    let b = create_taper_extrude(&mut k, &refs, 10.0, 10.0, 3, true);
    assert!(b.is_ok(), "{b:?}");
}

/// The curvature-keeping rule refuses the strip with or without the field.
#[test]
fn curve_extension_refuses_the_strip() {
    for fix in [false, true] {
        let mut k = kernel(fix);
        let TaperExtrude::Refused(refusal) =
            taper(&mut k, &strip(), 10.0, OffsetCornerType::CurveExtension)
        else {
            panic!("built");
        };
        assert_eq!(refusal.reason, TaperCornerRefusal::WallCloses);
        assert_eq!(refusal.corner, p(20.0, 0.0));
    }
}

/// A rectangle drafted outward: SMLib's tangent rule gives 14 faces (two flat corner
/// triangles at each corner); with the field, and with the curvature-keeping rule, the
/// four walls meet at corner edges (6 faces). The volume is the same exact prismatoid.
#[test]
fn a_drafted_rectangle_has_six_faces_with_the_fix() {
    let expected = drafted_rectangle_volume(true);
    let mut k = kernel(false);
    let (faces, v) = built(
        &mut k,
        &rectangle(),
        -10.0,
        OffsetCornerType::LinearExtension,
    );
    assert_eq!(faces, 14);
    assert_rel(v, expected, 1e-9);

    let mut k = kernel(true);
    let (faces, v) = built(
        &mut k,
        &rectangle(),
        -10.0,
        OffsetCornerType::LinearExtension,
    );
    assert_eq!(faces, 6);
    assert_rel(v, expected, 1e-9);

    for fix in [false, true] {
        let mut k = kernel(fix);
        let (faces, v) = built(
            &mut k,
            &rectangle(),
            -10.0,
            OffsetCornerType::CurveExtension,
        );
        assert_eq!(faces, 6);
        assert_rel(v, expected, 1e-9);
    }

    // inward there is no gap at the corners: every rule gives the same 6 faces
    let mut k = kernel(true);
    let (faces, v) = built(
        &mut k,
        &rectangle(),
        10.0,
        OffsetCornerType::LinearExtension,
    );
    assert_eq!(faces, 6);
    assert_rel(v, drafted_rectangle_volume(false), 1e-9);
}

/// The volume of the half disc drafted outward with its walls kept: at height t the
/// section is the disc of radius 20 + t tan 3 cut by the line y = -t tan 3 (the plane
/// wall), so its area is pi r^2 / 2 + d sqrt(r^2 - d^2) + r^2 asin(d / r); Simpson's rule
/// over the height.
fn half_disc_cone_volume() -> f64 {
    let o = 3.0_f64.to_radians().tan();
    let area = |t: f64| {
        let (r, d) = (20.0 + t * o, t * o);
        std::f64::consts::PI * r * r / 2.0 + d * (r * r - d * d).sqrt() + r * r * (d / r).asin()
    };
    let n = 200;
    let h = 10.0 / f64::from(n);
    let mut sum = area(0.0) + area(10.0);
    for i in 1..n {
        sum += area(f64::from(i) * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// A half disc drafted outward: SMLib's tangent rule does not close (WR-402, without
/// `taper_pivot_nearest_corner`); the curvature-keeping rule continues the arc's cone to
/// the diameter's plane (4 faces, the exact volume); the tangent planes fix continues the
/// cone's ends as flat pieces (6 faces, the volume WR-402's fix gives, 6555.9139).
#[test]
fn a_half_disc_keeps_its_cone_or_gets_tangent_planes() {
    let mut k = kernel(false);
    let d = half_disc(&k);
    let TaperExtrude::Solid(b) = taper(&mut k, &d, -3.0, OffsetCornerType::LinearExtension) else {
        panic!("refused");
    };
    assert!(!is_manifold_solid(&b).unwrap());

    let (faces, v) = built(&mut k, &d, -3.0, OffsetCornerType::CurveExtension);
    assert_eq!(faces, 4);
    assert_rel(v, half_disc_cone_volume(), 1e-6);

    let mut k = kernel(true);
    let (faces, v) = built(&mut k, &d, -3.0, OffsetCornerType::LinearExtension);
    assert_eq!(faces, 6);
    assert_rel(v, 6555.9139, 1e-7);
}

/// A profile curve that is neither a line nor an arc has no exact wall: the
/// curvature-keeping rule refuses it at its start, and the tangent planes fix leaves it to
/// SMLib's offset method.
#[test]
fn a_free_form_corner_is_not_exact() {
    use solidcore::nurbs::b_spline_curve::p1::BSplineCurve;
    let mut k = kernel(true);
    // the rectangle with its bottom side as a plain B-spline
    let mut curves = rectangle();
    curves[0] =
        Curve::from(BSplineCurve::create_line_segment(3, p(0.0, 0.0), p(20.0, 0.0)).unwrap());
    let TaperExtrude::Refused(refusal) =
        taper(&mut k, &curves, -10.0, OffsetCornerType::CurveExtension)
    else {
        panic!("built");
    };
    assert_eq!(refusal.reason, TaperCornerRefusal::NotExact);
    let (faces, _) = built(&mut k, &curves, -10.0, OffsetCornerType::LinearExtension);
    assert_eq!(faces, 14);
}
