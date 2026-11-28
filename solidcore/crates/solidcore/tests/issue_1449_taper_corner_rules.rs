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
use solidcore::math::det;
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
    let o = h * det::tan(10.0_f64.to_radians());
    let s = if outward { 1.0 } else { -1.0 };
    h * (a * b + s * o * (a + b) + 4.0 * o * o / 3.0)
}

/// The strip's closed solid, the hip roof: at height z the section is the rectangle
/// (a - 2 t z) x (b - 2 t z), t = tan 10, until it closes at z* = b / (2 t), so
/// V = integral (a - 2 t z)(b - 2 t z) dz = b^2 (3 a - b) / (12 t).
fn strip_volume() -> f64 {
    let (a, b) = (20.0_f64, 2.0_f64);
    let t = det::tan(10.0_f64.to_radians());
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
    let angle = det::atan(0.1_f64).to_degrees();
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
    // at a corner of a short wall, the walls that close
    let corners = [p(0.0, 0.0), p(20.0, 0.0), p(20.0, 2.0), p(0.0, 2.0)];
    assert!(
        corners.iter().any(|c| c.distance(refusal.corner) <= 1e-12),
        "{refusal:?}"
    );
}

/// A 3-4-5 triangle drafted 45 degrees in: every wall's section shrinks to the incentre
/// (1, 1) at the height of the inradius, 1 (tan 45 = 1), so the closed taper is the
/// pyramid of volume 6 * 1 / 3 = 2 with 4 faces and 4 vertices.
#[test]
fn a_triangle_closes_at_its_incentre() {
    let mut k = kernel();
    let curves = polygon(&[p(0.0, 0.0), p(4.0, 0.0), p(0.0, 3.0)]);
    let TaperExtrude::Solid(mut b) =
        taper(&mut k, &curves, 45.0, OffsetCornerType::LinearExtension)
    else {
        panic!("refused");
    };
    assert!(is_manifold_solid(&b).unwrap());
    assert_eq!((b.face_list().len(), b.vertex_list().len()), (4, 4));
    let apex = b
        .vertex_list()
        .iter()
        .map(|&v| b.vertex(v).unwrap().point)
        .fold(
            Vec3::new(0.0, 0.0, -1.0),
            |a, q| if q.z > a.z { q } else { a },
        );
    assert!(apex.distance(Vec3::new(1.0, 1.0, 1.0)) <= 1e-9, "{apex:?}");
    assert_rel(volume(&k, &mut b), 2.0, 1e-9);
}

/// A rectangle 2 x (2 + d) drafted 45 degrees in closes at height 1: its long walls meet
/// along a ridge of length d. A ridge well over the tolerance is built as one (6 vertices);
/// one far below it is the apex of a pyramid (5 vertices). The volume is the hip roof's,
/// b^2 (3a - b) / 12 with a = 2 + d, b = 2 (tan 45 = 1), either way.
#[test]
fn a_ridge_closes_as_a_ridge_and_a_point_as_an_apex() {
    for (d, vertices) in [(1e-3, 6), (1e-12, 5)] {
        let mut k = kernel();
        let a = 2.0 + d;
        let curves = polygon(&[p(0.0, 0.0), p(a, 0.0), p(a, 2.0), p(0.0, 2.0)]);
        let TaperExtrude::Solid(mut b) =
            taper(&mut k, &curves, 45.0, OffsetCornerType::LinearExtension)
        else {
            panic!("d {d}: refused");
        };
        assert!(is_manifold_solid(&b).unwrap(), "d {d}");
        assert_eq!(
            (b.face_list().len(), b.vertex_list().len()),
            (5, vertices),
            "d {d}"
        );
        assert_rel(volume(&k, &mut b), 4.0 * (3.0 * a - 2.0) / 12.0, 1e-9);
    }
}

/// Between the two: a ridge shorter than the B-rep's tolerance but longer than the closure
/// bound (`tol s / (s + H)`, half the tolerance at 45 degrees over a height of 10 with an
/// offset of 10) is neither one apex nor two vertices, so it is refused where its first
/// short wall closes rather than built with a sub-tolerance ridge.
#[test]
fn a_ridge_shorter_than_the_tolerance_is_refused() {
    let mut k = kernel();
    // the B-rep's tolerance, from the 1e-3 case that builds: the taper's walls are closed
    // to that same tolerance (both the kernel's default B-rep tolerance)
    let curves = polygon(&[p(0.0, 0.0), p(2.001, 0.0), p(2.001, 2.0), p(0.0, 2.0)]);
    let TaperExtrude::Solid(b) = taper(&mut k, &curves, 45.0, OffsetCornerType::LinearExtension)
    else {
        panic!("refused");
    };
    let tol = b.tolerance();
    let a = 2.0 + 0.75 * tol;
    let curves = polygon(&[p(0.0, 0.0), p(a, 0.0), p(a, 2.0), p(0.0, 2.0)]);
    let result = taper(&mut k, &curves, 45.0, OffsetCornerType::LinearExtension);
    let TaperExtrude::Refused(refusal) = result else {
        panic!("built: {result:?}");
    };
    assert_eq!(refusal.reason, TaperCornerRefusal::WallCloses);
    let corners = [p(0.0, 0.0), p(a, 0.0), p(a, 2.0), p(0.0, 2.0)];
    assert!(
        corners.iter().any(|c| c.distance(refusal.corner) <= 1e-12),
        "{refusal:?}"
    );
}

/// A quarter disc of radius 10 (its two radii and an `SmCircle` arc) drafted 45 degrees in:
/// it is tangential (its inscribed circle touches both radii and the arc), so the three
/// walls' sections, the arc's on its cone, shrink to the inscribed circle's centre, on the
/// bisector at `R / (1 + sin 45)` from the corner, at the height of its radius
/// `rho = R sin 45 / (1 + sin 45)` (4.14), drafted over 6.
#[test]
fn a_quarter_disc_closes_at_its_inscribed_centre() {
    let mut k = kernel();
    let r = 10.0;
    let a = Line::create_line_segment(3, p(0.0, 0.0), p(r, 0.0), None).unwrap();
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let arc =
        Ellipse::circle_from_3_points(&k, p(r, 0.0), p(r * h, r * h), p(0.0, r), 3, false).unwrap();
    let c = Line::create_line_segment(3, p(0.0, r), p(0.0, 0.0), None).unwrap();
    let curves = [Curve::from(a), Curve::from(arc), Curve::from(c)];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    // over 6, past the apex, before the arc's cone reaches its axis at 10
    let result = create_taper_extrude_with_corner(
        &mut k,
        &refs,
        6.0,
        45.0,
        3,
        OffsetCornerType::LinearExtension,
    )
    .unwrap();
    let TaperExtrude::Solid(mut b) = result else {
        panic!("refused: {result:?}");
    };
    assert!(is_manifold_solid(&b).unwrap());
    let lines = solidcore::topology::brep::validation::check_brep(
        &mut k,
        &mut b,
        solidcore::topology::brep::validation::CheckLevel::Level2,
    )
    .unwrap()
    .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let rho = r * h / (1.0 + h);
    let along = r / (1.0 + h);
    let want = Vec3::new(along * h, along * h, rho);
    let apex = b
        .vertex_list()
        .iter()
        .map(|&v| b.vertex(v).unwrap().point)
        .fold(
            Vec3::new(0.0, 0.0, -1.0),
            |a, q| if q.z > a.z { q } else { a },
        );
    assert!(apex.distance(want) <= 1e-9 * r, "{apex:?} vs {want:?}");
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
    let o = det::tan(3.0_f64.to_radians());
    let area = |t: f64| {
        let (r, d) = (20.0 + t * o, t * o);
        std::f64::consts::PI * r * r / 2.0 + d * (r * r - d * d).sqrt() + r * r * det::asin(d / r)
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
/// SMLib's offset method. (A B-spline that is a straight line is a line, #16639.)
#[test]
fn a_free_form_corner_is_not_exact() {
    use solidcore::nurbs::b_spline_curve::p1::{
        BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
    };
    let mut k = kernel();
    // the rectangle with its bottom side a quadratic B-spline bowed out by 0.25
    let mut curves = rectangle();
    curves[0] = Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &[p(0.0, 0.0), p(10.0, -0.5), p(20.0, 0.0)],
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[3, 3],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap(),
    );
    let TaperExtrude::Refused(refusal) =
        taper(&mut k, &curves, -10.0, OffsetCornerType::CurveExtension)
    else {
        panic!("built");
    };
    assert_eq!(refusal.reason, TaperCornerRefusal::NotExact);
    let (faces, _) = built(&mut k, &curves, -10.0, OffsetCornerType::LinearExtension);
    assert_eq!(faces, 14);
}

/// #16639: the rectangle with its bottom side a straight B-spline segment drafts as the
/// rectangle of lines under both corner rules (the curvature rule refused it as not exact):
/// 6 faces, 12 edges, 8 vertices, the prismatoid's volume.
#[test]
fn a_straight_spline_side_drafts_as_its_line() {
    use solidcore::nurbs::b_spline_curve::p1::BSplineCurve;
    let expected = drafted_rectangle_volume(true);
    let mut curves = rectangle();
    curves[0] =
        Curve::from(BSplineCurve::create_line_segment(3, p(0.0, 0.0), p(20.0, 0.0)).unwrap());
    for corner in [
        OffsetCornerType::LinearExtension,
        OffsetCornerType::CurveExtension,
    ] {
        let mut k = kernel();
        let TaperExtrude::Solid(b) = taper(&mut k, &curves, -10.0, corner) else {
            panic!("{corner:?}: refused");
        };
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            (6, 12, 8),
            "{corner:?}"
        );
        let (_, v) = built(&mut k, &curves, -10.0, corner);
        assert_rel(v, expected, 1e-9);
    }
}

/// A leg on a foot (#18452, the foot of a lettering "Omega"): the foot's top runs left into a
/// sharp notch, where a curve (a parabola, so the tangent rule takes SMLib's offset method)
/// leaves it at 27.5 degrees, 0.83 from the leg's far side, x = 0.
fn leg_on_a_foot() -> Vec<Curve> {
    use solidcore::nurbs::b_spline_curve::p1::{
        BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
    };
    let notch = p(0.83, 0.875);
    let parabola = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 2,
        control_points: &[notch, p(1.9, 1.465), p(2.8, 3.0)],
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let line = |a, b| Curve::from(Line::create_line_segment(3, a, b, None).unwrap());
    vec![
        line(p(0.0, 0.0), p(3.5, 0.0)),
        line(p(3.5, 0.0), p(3.5, 0.94)),
        line(p(3.5, 0.94), notch),
        Curve::from(parabola),
        line(p(2.8, 3.0), p(0.0, 3.0)),
        line(p(0.0, 3.0), p(0.0, 0.0)),
    ]
}

/// The tangent rule continues the notch's walls as a long wedge, which reaches the leg's
/// far wall partway down a 45 degree draft 0.3 deep: SMLib's offset method built an open
/// shell there. It is refused where the walls meet, on the far wall (x = z at 45 degrees);
/// a 20 degree draft, whose wedge stops short of it, builds.
#[test]
fn walls_meeting_across_the_offset_method_are_refused_where_they_meet() {
    let mut k = kernel();
    let curves = leg_on_a_foot();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let result = create_taper_extrude_with_corner(
        &mut k,
        &refs,
        0.3,
        45.0,
        3,
        OffsetCornerType::LinearExtension,
    )
    .unwrap();
    let TaperExtrude::Refused(refusal) = result else {
        panic!("built: {result:?}");
    };
    assert_eq!(refusal.reason, TaperCornerRefusal::WallCloses);
    let at = refusal.corner;
    assert!((at.x - at.z).abs() <= 1e-5, "{at:?}");
    assert!(
        at.z > 0.0 && at.z < 0.3 && at.y > 0.0 && at.y < 0.875,
        "{at:?}"
    );

    let result = create_taper_extrude_with_corner(
        &mut k,
        &refs,
        0.3,
        20.0,
        3,
        OffsetCornerType::LinearExtension,
    )
    .unwrap();
    solid(&k, result);
}
