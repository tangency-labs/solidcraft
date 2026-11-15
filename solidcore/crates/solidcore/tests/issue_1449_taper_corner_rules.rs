//! tangency-labs/solidcore#1449: `SmApiCreateTaperExtrude`'s draft-corner rules (SolidCraft
//! fork, tangency-labs/smlib 54148db and e8f23e0).
//!
//! - `OffsetCornerType::CurveExtension` (WR-520): each wall continues as itself (a line's
//!   plane, an arc's cone) to meet its neighbour along their exact seam, and a corner that
//!   cannot be closed is refused with why and where. It is a new corner rule, so selecting
//!   it is the opt-in.
//! - Linear-extension corners (WR-521; once the opt-in `taper_tangent_corner_planes`, now
//!   the only path): a profile of lines and arcs continues the walls as their tangent planes
//!   through the same construction (a drafted rectangle has 6 faces, not SMLib's 14) and
//!   refuses a corner they cannot close.
//!
//! The issue's repro is a strip 20 x 2 drafted 10 degrees inward over a height of 10: the
//! long walls' offsets (10 tan 10 = 1.76) meet across the short walls (half-width 1) before
//! the far end. SMLib builds a 5-face closed solid (a ridge, no far cap); the fork refused
//! it (`SM_TCR_WALL_CLOSES`) at the first corner of a short wall. Both rules now close it into
//! its ridge (#17265): the short walls close at their apexes, the long walls meet along the
//! ridge between them, and there no section is left.
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
use solidcore::{Brep, Vec3};

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

fn kernel() -> Kernel {
    Kernel::new(&KernelOptions::default())
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

/// The strip's closed solid, the hip roof: at height z the section is the rectangle
/// (a - 2 t z) x (b - 2 t z), t = tan 10, until it closes at z* = b / (2 t), so
/// V = integral (a - 2 t z)(b - 2 t z) dz = b^2 (3 a - b) / (12 t).
fn strip_volume() -> f64 {
    let (a, b) = (20.0_f64, 2.0_f64);
    let t = 10.0_f64.to_radians().tan();
    b * b * (3.0 * a - b) / (12.0 * t)
}

/// The issue's repro (#17265): the tangent rule closes the strip into its ridge, a 5-face
/// closed solid (two triangles, two trapezoids and the cap at the profile) of the hip roof's
/// volume, through both entry points; it refused it as `WallCloses`. Fillet corners build
/// their own solid.
#[test]
fn the_tangent_rule_closes_the_strip_into_its_ridge() {
    let mut k = kernel();
    let curves = strip();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut b = create_taper_extrude(&mut k, &refs, 10.0, 10.0, 3, false).unwrap();
    assert!(is_manifold_solid(&b).unwrap() && b.face_list().len() == 5);
    assert_rel(volume(&k, &mut b), strip_volume(), 1e-9);
    let (faces, v) = built(&mut k, &curves, 10.0, OffsetCornerType::LinearExtension);
    assert_eq!(faces, 5);
    assert_rel(v, strip_volume(), 1e-9);
    let b = create_taper_extrude(&mut k, &refs, 10.0, 10.0, 3, true);
    assert!(b.is_ok(), "{b:?}");
}

/// The curvature-keeping rule closes the strip the same way (lines: the same walls).
#[test]
fn curve_extension_closes_the_strip_into_its_ridge() {
    let mut k = kernel();
    let (faces, v) = built(&mut k, &strip(), 10.0, OffsetCornerType::CurveExtension);
    assert_eq!(faces, 5);
    assert_rel(v, strip_volume(), 1e-9);
}

/// The strip drafted so that its long walls meet just at the far end: the far end would
/// pinch to the ridge, so it is still refused where the short wall closes.
#[test]
fn a_ridge_on_the_far_end_is_still_refused() {
    let mut k = kernel();
    let curves = strip();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    // 10 tan(angle) = 1, the half-width
    let angle = 0.1_f64.atan().to_degrees();
    let TaperExtrude::Refused(refusal) = create_taper_extrude_with_corner(
        &mut k,
        &refs,
        10.0,
        angle,
        3,
        OffsetCornerType::LinearExtension,
    )
    .unwrap() else {
        panic!("built");
    };
    assert_eq!(refusal.reason, TaperCornerRefusal::WallCloses);
}

/// A rectangle drafted outward: SMLib's tangent rule gave 14 faces (two flat corner
/// triangles at each corner, coplanar with the walls beside them); the tangent rule, and
/// the curvature-keeping rule, meet the four walls at corner edges (6 faces). The volume is
/// the same exact prismatoid.
#[test]
fn a_drafted_rectangle_has_six_faces() {
    let expected = drafted_rectangle_volume(true);
    let mut k = kernel();
    let (faces, v) = built(
        &mut k,
        &rectangle(),
        -10.0,
        OffsetCornerType::LinearExtension,
    );
    assert_eq!(faces, 6);
    assert_rel(v, expected, 1e-9);

    let mut k = kernel();
    let (faces, v) = built(
        &mut k,
        &rectangle(),
        -10.0,
        OffsetCornerType::CurveExtension,
    );
    assert_eq!(faces, 6);
    assert_rel(v, expected, 1e-9);

    // inward there is no gap at the corners: every rule gives the same 6 faces
    let mut k = kernel();
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

/// A half disc drafted outward: the curvature-keeping rule continues the arc's cone to
/// the diameter's plane (4 faces, the exact volume); the tangent rule continues the
/// cone's ends as flat pieces (6 faces, the volume WR-402's fix gives, 6555.9139).
#[test]
fn a_half_disc_keeps_its_cone_or_gets_tangent_planes() {
    let mut k = kernel();
    let d = half_disc(&k);
    let (faces, v) = built(&mut k, &d, -3.0, OffsetCornerType::CurveExtension);
    assert_eq!(faces, 4);
    assert_rel(v, half_disc_cone_volume(), 1e-6);

    let (faces, v) = built(&mut k, &d, -3.0, OffsetCornerType::LinearExtension);
    assert_eq!(faces, 6);
    assert_rel(v, 6555.9139, 1e-7);
}

/// A profile curve that is neither a line nor an arc has no exact wall: the
/// curvature-keeping rule refuses it at its start, and the tangent rule leaves it to
/// SMLib's offset method.
#[test]
fn a_free_form_corner_is_not_exact() {
    use solidcore::nurbs::b_spline_curve::p1::BSplineCurve;
    let mut k = kernel();
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
