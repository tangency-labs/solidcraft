//! A planar profile extruded along a direction as given (any length): straight, by the
//! linear sweep, or drafted, by the taper extrusion, whose height's sign picks the side
//! of the profile's plane the direction points to.
//!
//! Not upstream: SolidCraft's extrude entry (the direction made unit, the drafted
//! profile checked to be one closed loop square to it). Deviations: none (no upstream
//! counterpart). No recursion.

use crate::curve::composite_curve::OffsetCornerType;
use crate::curve::curve::p3::compute_crvs_normal;
use crate::curve::curve::Curve;
use crate::curve::path::{closed_loop, profile_square_to};
use crate::error::{Result, SolidError};
use crate::geom::certified::{dot, unit, unit_error, Bounded};
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::sweep::api_brep::{create_linear_sweep, create_taper_extrude_with_corner, TaperExtrude};

/// The operation name of every refusal of [`extrude`]'s own checks.
pub const OP: &str = "sweep::extrude::extrude";

/// A draft: its angle (degrees; the sign picks the side, as
/// [`create_taper_extrude_with_corner`] takes it) and its corner rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Draft {
    /// The angle in degrees.
    pub angle_deg: f64,
    /// How the walls meet at a corner.
    pub corner: OffsetCornerType,
}

/// The tolerances a drafted profile is checked with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtrudeTolerances {
    /// How far apart consecutive curves' ends (and the last's end and the first's
    /// start) may be for the profile to be one closed loop.
    pub loop_gap: f64,
    /// How far the profile may lie off the plane square to the direction, relative to
    /// its size (at least 1) ([`profile_square_to`]).
    pub parallel: f64,
}

/// `curves` extruded `distance` along `direction` (any length): straight without a
/// draft (the linear sweep), or drafted with one (the taper extrusion), capped at both
/// ends with `cap`. A drafted profile must be proved one closed loop (to
/// `tol.loop_gap`) lying in the plane square to the direction (every control point
/// within `tol.parallel` of the profile's size of it, [`profile_square_to`]); the height
/// is `distance` towards the side the direction points to, read from the sign of the
/// dot of [`compute_crvs_normal`]'s normal (as a given vector) with the direction, which
/// must be proved away from 0.
///
/// # Errors
///
/// `InvalidInput` (operation [`OP`]) for a direction without a usable length, a
/// distance not finite and positive, a drafted profile not proved one closed loop in
/// the plane square to the direction, or one whose normal is not proved along it; those of [`create_linear_sweep`],
/// [`compute_crvs_normal`] and [`create_taper_extrude_with_corner`].
pub fn extrude(
    k: &mut Kernel,
    curves: &[Curve],
    direction: Vec3,
    distance: f64,
    draft: Option<Draft>,
    cap: bool,
    tol: &ExtrudeTolerances,
) -> Result<TaperExtrude> {
    let Some(dir) = unit(direction) else {
        return Err(SolidError::invalid_input(
            OP,
            "a direction without a usable length",
        ));
    };
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a distance not finite and positive",
        ));
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let Some(draft) = draft else {
        return create_linear_sweep(k, &refs, dir, distance, cap).map(TaperExtrude::Solid);
    };
    if !closed_loop(k, curves, tol.loop_gap)?.is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the profile is not proved one closed loop",
        ));
    }
    if !profile_square_to(curves, dir, tol.parallel)?.is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the profile is not proved square to the direction",
        ));
    }
    // Only the side: the taper extrusion's own normal, made unit and taken as a given
    // vector, must be proved along the direction one way or the other (its dot with it
    // away from 0 by a half), so its sign is the side.
    let Some(normal) = compute_crvs_normal(k, curves).ok().and_then(unit) else {
        return Err(SolidError::invalid_input(OP, "the profile has no normal"));
    };
    let along = dot(normal, dir, unit_error(), unit_error());
    let away = Bounded {
        value: along.value.abs(),
        ..along
    };
    if !away.above(0.5).is_yes() {
        return Err(SolidError::invalid_input(
            OP,
            "the profile's normal is not along the direction",
        ));
    }
    let height = if along.value > 0.0 {
        distance
    } else {
        -distance
    };
    create_taper_extrude_with_corner(
        k,
        &refs,
        height,
        draft.angle_deg,
        if cap { 3 } else { 0 },
        draft.corner,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::vector3d::Point3;
    use crate::math::det;
    use crate::topology::brep::p2::is_manifold_solid;

    const TOL: ExtrudeTolerances = ExtrudeTolerances {
        loop_gap: 1e-6,
        parallel: 1e-9,
    };

    fn square() -> Vec<Curve> {
        let p = [
            Point3::ZERO,
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ];
        (0..4)
            .map(|i| Curve::from(Line::create_line_segment(3, p[i], p[(i + 1) % 4], None).unwrap()))
            .collect()
    }

    fn bounds(k: &Kernel, b: &crate::topology::Brep) -> (Point3, Point3) {
        let e = crate::query::api_queries::brep_bounding_box(k, b, true).unwrap();
        (e.min(), e.max())
    }

    #[test]
    fn a_long_direction_extrudes_the_distance() {
        let mut k = Kernel::default();
        let c = square();
        let TaperExtrude::Solid(b) =
            extrude(&mut k, &c, Vec3::new(0.0, 0.0, 7.0), 3.0, None, true, &TOL).unwrap()
        else {
            panic!()
        };
        assert!(is_manifold_solid(&b).unwrap());
        let (lo, hi) = bounds(&k, &b);
        assert!((lo - Point3::ZERO).length() < 1e-9);
        assert!((hi - Point3::new(2.0, 2.0, 3.0)).length() < 1e-9);
    }

    #[test]
    fn a_draft_goes_the_way_the_direction_points() {
        let mut k = Kernel::default();
        let draft = Draft {
            angle_deg: 10.0,
            corner: OffsetCornerType::LinearExtension,
        };
        for z in [5.0, -5.0] {
            let c = square();
            let TaperExtrude::Solid(b) = extrude(
                &mut k,
                &c,
                Vec3::new(0.0, 0.0, z),
                1.0,
                Some(draft),
                true,
                &TOL,
            )
            .unwrap() else {
                panic!()
            };
            let (lo, hi) = bounds(&k, &b);
            if z > 0.0 {
                assert!((hi.z - 1.0).abs() < 1e-9 && lo.z.abs() < 1e-9);
            } else {
                assert!((lo.z + 1.0).abs() < 1e-9 && hi.z.abs() < 1e-9);
            }
        }
    }

    #[test]
    fn a_draft_across_the_profile_or_an_open_profile_is_refused() {
        let mut k = Kernel::default();
        let draft = Some(Draft {
            angle_deg: 10.0,
            corner: OffsetCornerType::LinearExtension,
        });
        let c = square();
        let refused = |r: Result<TaperExtrude>| matches!(r, Err(SolidError::InvalidInput { operation, .. }) if operation == OP);
        assert!(refused(extrude(
            &mut k,
            &c,
            Vec3::new(0.0, 1e-3, 1.0),
            1.0,
            draft,
            true,
            &TOL
        )));
        assert!(refused(extrude(
            &mut k,
            &c[..3],
            Vec3::Z,
            1.0,
            draft,
            true,
            &TOL
        )));
        assert!(refused(extrude(
            &mut k,
            &c,
            Vec3::ZERO,
            1.0,
            None,
            true,
            &TOL
        )));
        assert!(refused(extrude(&mut k, &c, Vec3::Z, 0.0, None, true, &TOL)));
        // Across by 1e-6 radians: 1 - cos is 5e-13, within 1e-9, but the square's corners
        // lie up to 2.8e-6 off the plane square to the direction.
        assert!(refused(extrude(
            &mut k,
            &c,
            Vec3::new(0.0, 1e-6, 1.0),
            1.0,
            draft,
            true,
            &TOL
        )));
        assert!(profile_square_to(&c, Vec3::Z, 1e-9).unwrap().is_yes());
    }

    #[test]
    fn a_profile_with_a_short_cubic_extrudes_to_a_valid_solid() {
        // #17735: a traced outline's cubic 0.025 long, about 14 from the origin, closed by
        // its chord. The wall swept from it is a surface of extrusion whose origin is the
        // cubic's point at 0.45678; the check's search for that point on the cubic stopped
        // 2e-11 short of it, beyond the check's 1.5e-11, and refused the solid.
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::CPoint;
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        let p = [
            Point3::new(6.505, 14.3, 0.0),
            Point3::new(6.5084, 14.2944, 0.0),
            Point3::new(6.5134, 14.2944, 0.0),
            Point3::new(6.53, 14.3, 0.0),
        ];
        let pw = p.iter().map(|&q| CPoint::from_point(q)).collect();
        let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let cubic = BSplineCurve::from_nurb(NurbCurve::new(pw, 3, knots).unwrap(), 3, false);
        let c = vec![
            Curve::from(cubic),
            Curve::from(Line::create_line_segment(3, p[3], p[0], None).unwrap()),
        ];
        let mut k = Kernel::default();
        let TaperExtrude::Solid(mut b) =
            extrude(&mut k, &c, Vec3::Z, 1.0, None, true, &TOL).unwrap()
        else {
            panic!()
        };
        assert!(is_manifold_solid(&b).unwrap());
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(check.is_valid(), "{:?}", check.lines());
    }

    #[test]
    fn an_uncapped_profile_with_a_short_segment_keeps_every_wall() {
        // #18515: a 10 x 10 loop whose top side ends in a segment `len` long, extruded 1.2
        // with no caps. Each curve sweeps one wall, so the sheet has a wall per curve, a
        // bottom and a top edge per wall, a side edge per profile vertex, and a vertex at
        // each end of a side edge. The two edges of the short wall's profile copies are
        // `len` long. The lengths lie between the stitch's first tolerance (2e-5) and its
        // second (4e-4), where the second pass squeezed both edges out of the sheet.
        use crate::topology::brep::validation::{check_brep, CheckLevel};
        use crate::topology::{edge, vertex};
        for len in [3e-5, 1.7e-4, 3.9e-4] {
            let p = [
                Point3::ZERO,
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(10.0, 10.0, 0.0),
                Point3::new(len, 10.0, 0.0),
                Point3::new(0.0, 10.0, 0.0),
            ];
            for closed in [true, false] {
                let n = if closed { 5 } else { 4 };
                let c: Vec<Curve> = (0..n)
                    .map(|i| {
                        Curve::from(
                            Line::create_line_segment(3, p[i], p[(i + 1) % 5], None).unwrap(),
                        )
                    })
                    .collect();
                let mut k = Kernel::default();
                let TaperExtrude::Solid(mut b) =
                    extrude(&mut k, &c, Vec3::Z, 1.2, None, false, &TOL).unwrap()
                else {
                    panic!()
                };
                // the open profile's five points, like the closed one's, give five sides
                assert_eq!(
                    (
                        b.face_list().len(),
                        b.edge_list().len(),
                        b.vertex_list().len()
                    ),
                    (n, 2 * n + 5, 10),
                    "len {len}, closed {closed}: faces, edges, vertices"
                );
                let short = b
                    .edge_list()
                    .iter()
                    .filter(|&&e| {
                        let s = edge::start_vertex(&b, e).unwrap().unwrap();
                        let o = edge::other_vertex(&b, e, Some(s)).unwrap().unwrap();
                        let d = vertex::point(&b, s)
                            .unwrap()
                            .distance(vertex::point(&b, o).unwrap());
                        (d - len).abs() <= 1e-12
                    })
                    .count();
                assert_eq!(
                    short, 2,
                    "len {len}, closed {closed}: the short wall's edges"
                );
                let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
                assert!(
                    check.is_valid(),
                    "len {len}, closed {closed}: {:?}",
                    check.lines()
                );
            }
        }
    }

    /// #18514: a rounded rectangle 2a x 2b with corners of radius r about (+-(a - r),
    /// +-(b - r)), counter-clockwise from its bottom line, lines and quarter arcs.
    fn rounded_rectangle(k: &Kernel, a: f64, b: f64, r: f64) -> Vec<Curve> {
        use crate::curve::arc::ArcDefinition;
        use crate::curve::path::PathPiece;
        use std::f64::consts::FRAC_PI_2;
        let line = |x0: f64, y0: f64, x1: f64, y1: f64| {
            Curve::from(
                Line::create_line_segment(
                    3,
                    Point3::new(x0, y0, 0.0),
                    Point3::new(x1, y1, 0.0),
                    None,
                )
                .unwrap(),
            )
        };
        let arc = |cx: f64, cy: f64, quarter: f64| {
            let d = ArcDefinition {
                centre: Point3::new(cx, cy, 0.0),
                axis: Vec3::Z,
                reference: Vec3::X,
                radius: r,
                start: quarter * FRAC_PI_2,
                end: (quarter + 1.0) * FRAC_PI_2,
            };
            PathPiece::of_arc(k, &d).unwrap().curve().clone()
        };
        vec![
            line(-a + r, -b, a - r, -b),
            arc(a - r, -b + r, -1.0),
            line(a, -b + r, a, b - r),
            arc(a - r, b - r, 0.0),
            line(a - r, b, -a + r, b),
            arc(-a + r, b - r, 1.0),
            line(-a, b - r, -a, -b + r),
            arc(-a + r, -b + r, 2.0),
        ]
    }

    #[test]
    fn a_rounded_rectangle_drafted_slightly_inward_keeps_every_corner_wall() {
        // #18514: the issue's rounded rectangle, 0.5 degrees inward. At these heights the
        // offset method lost the walls of two corner arcs: their offsets' mid points lie
        // exactly the offset (about 0.017) from their arcs, and the classifier dropped
        // them where that distance, computed about 111 from the origin, rounded below
        // `offset * (1 - 1e-12)`. The sections are the profile offset inward by
        // `s = z tan 0.5 degrees`: lines moved in by s, arcs of radius r - s about the same
        // centres, so the area is `(2a - 2s)(2b - 2s) - (4 - pi)(r - s)^2` and the volume
        // its integral over the height. Every height builds a wall along each of the four
        // lines and four arcs and two caps: 10 faces, 24 edges, 16 vertices.
        use crate::mass::integrator::properties::brep_volume_default;
        use crate::topology::brep::outward_solid_failure;
        let (a, b, r) = (111.48254626441849, 41.48254626441848, 8.982546264418483);
        let draft = Some(Draft {
            angle_deg: 0.5,
            corner: OffsetCornerType::CurveExtension,
        });
        let t = det::tan(0.5_f64.to_radians());
        let (w, h2) = (2.0 * a, 2.0 * b);
        let c4 = 4.0 - std::f64::consts::PI;
        let volume = |h: f64| {
            h * (w * h2 - c4 * r * r)
                + t * h * h / 2.0 * (-2.0 * (w + h2) + 2.0 * c4 * r)
                + t * t * h * h * h / 3.0 * std::f64::consts::PI
        };
        for height in [2.0, 2.01, 2.5] {
            let mut k = Kernel::default();
            let c = rounded_rectangle(&k, a, b, r);
            let TaperExtrude::Solid(mut body) =
                extrude(&mut k, &c, Vec3::Z, height, draft, true, &TOL).unwrap()
            else {
                panic!("{height}: refused")
            };
            let counts = (
                body.face_list().len(),
                body.edge_list().len(),
                body.vertex_list().len(),
            );
            assert_eq!(counts, (10, 24, 16), "{height}");
            assert!(is_manifold_solid(&body).unwrap(), "{height}");
            assert_eq!(outward_solid_failure(&mut k, &mut body).unwrap(), None);
            let v = brep_volume_default(&k, &mut body, 1e-9).unwrap();
            let exact = volume(height);
            assert!(
                (v - exact).abs() <= 1e-7 * exact,
                "{height}: {v} vs {exact}"
            );
        }
    }
}
