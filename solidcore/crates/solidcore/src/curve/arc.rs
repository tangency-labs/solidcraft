//! Circular arcs by their definition: a centre, an axis, a reference direction, a radius
//! and start and end angles (radians, counter-clockwise about the axis from the
//! reference), as a profile or path of a sweep gives them. [`Ellipse::arc`] makes the
//! circle of a definition; [`arcs_on_one_circle`] and [`curves_meet`] are the certified
//! queries a profile's arcs are compared with; [`join_arcs`] makes the one arc of a run
//! of arcs on one circle.
//!
//! The circle of a definition is the one its placement names
//! ([`ArcDefinition::placement`]): the centre, the unit axis, and the reference's unit
//! component square to the axis ([`Axis2Placement::set_step_canonical`]), so the circle
//! lies in the plane square to the axis by construction. The arc is an exact class: the
//! circle keeps its analytic kind (an [`Ellipse`] of the circle class, its frame, radius
//! and angular domain beside its rational quadratic net).
//! TODO(d0b44726f): its bound 0 is claimed from that label alone until the net is proved
//! on the circle's plane and sphere by `surface::implicit::curve_on_quadrics` (PR 1,
//! layer-ops-convert d0b44726f); after the rebase onto it, [`Ellipse::arc`] proves it.
//!
//! Not upstream: SolidCraft's sweep profiles (`arc`, `FlatArc`, `one_circle`,
//! `angle_on` and the arc join of its one-wall-per-circle rule). Deviations: none (no
//! upstream counterpart). No recursion.

use std::f64::consts::{PI, TAU};

use crate::curve::curve::Curve;
use crate::curve::ellipse::Ellipse;
use crate::curve::path::PathPiece;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::certified::{
    difference, distance, dot, gamma, scaled, unit, unit_error, Bounded, Verdict,
};
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;

const OP_ARC: &str = "curve::arc::Ellipse::arc";
const OP_DEFINITION: &str = "curve::arc::ArcDefinition::placement";
const OP_JOIN: &str = "curve::arc::join_arcs";

/// The smallest radius, and the smallest sweep, of an arc.
pub const MIN_ARC: f64 = 1e-9;

/// How far from square to its axis a reference direction may be, as the cosine.
pub const SQUARE_TOL: f64 = 1e-9;

/// The error of each axis of an arc's placement against the exact frame of its
/// definition, as a vector length: the axis made unit (`2 e`, `e` the
/// [`unit_error`] of a component), the cross with the reference (`2 e + 4 gamma_2` of
/// the reference's length, about the cross's), made unit (`2 x` that, plus `2 e`): `Y`
/// within `6 e + 8 gamma_2`; the cross of `Y` with the axis (their errors and
/// `4 gamma_2`) made unit again: `X` within `18 e + 24 gamma_2`. Counted as `32 e`.
#[must_use]
pub fn frame_error() -> f64 {
    32.0 * unit_error()
}

/// A circular arc by its definition: the circle about `centre` of `radius` in the plane
/// square to `axis`, from angle `start` to `end` (radians, counter-clockwise about the
/// axis from `reference`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcDefinition {
    /// The centre.
    pub centre: Point3,
    /// The axis (any length; made unit where used).
    pub axis: Vec3,
    /// The direction of angle 0, square to the axis (any length; its component square
    /// to the axis made unit where used).
    pub reference: Vec3,
    /// The radius.
    pub radius: f64,
    /// The start angle, radians.
    pub start: f64,
    /// The end angle, radians.
    pub end: f64,
}

impl ArcDefinition {
    /// `end - start`.
    #[must_use]
    pub fn sweep(&self) -> f64 {
        self.end - self.start
    }

    /// The circle's placement: origin the centre, Z the unit axis, X the unit component
    /// of the reference square to the axis ([`Axis2Placement::set_step_canonical`]),
    /// each axis within [`frame_error`] of the definition's exact frame.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a centre, axis or reference not finite, an axis or reference
    /// without a length, or a reference not proved square to the axis to
    /// [`SQUARE_TOL`].
    pub fn placement(&self) -> Result<Axis2Placement> {
        let c = self.centre;
        if ![c.x, c.y, c.z].iter().all(|x| x.is_finite()) {
            return Err(SolidError::invalid_input(OP_DEFINITION, "not finite"));
        }
        let (Some(k), Some(r)) = (unit(self.axis), unit(self.reference)) else {
            return Err(SolidError::invalid_input(
                OP_DEFINITION,
                "axis or reference without a length",
            ));
        };
        // |k . r| of two made unit vectors.
        let square = dot(k, r, unit_error(), unit_error());
        let square = Bounded {
            value: square.value.abs(),
            ..square
        };
        if !square.at_most(SQUARE_TOL).is_yes() {
            return Err(SolidError::invalid_input(
                OP_DEFINITION,
                "reference not square to the axis",
            ));
        }
        let mut placement = Axis2Placement::IDENTITY;
        placement.set_step_canonical(c, self.axis, self.reference)?;
        Ok(placement)
    }

    /// The point at angle `t` of the circle (radians from the reference).
    ///
    /// # Errors
    ///
    /// Those of [`ArcDefinition::placement`].
    pub fn point_at(&self, t: f64) -> Result<Point3> {
        Ok(self.point_at_bounded(t, 0.0)?.0)
    }

    /// The point at angle `t` (given within `t_bound` of the exact angle), with a bound
    /// on its distance from the exact circle's point at the exact angle: the placement's
    /// axes ([`frame_error`] each, times the radius), `r cos t` and `r sin t` (the libm's
    /// two roundings and the product's, `gamma_3 r` each), the point's three products
    /// and sums (`gamma_4` of each coordinate's terms, at most `|centre| + 2 r`), the
    /// angle's own error (`r t_bound`); `sqrt 3 < 2` from a coordinate to the vector.
    ///
    /// # Errors
    ///
    /// Those of [`ArcDefinition::placement`].
    pub fn point_at_bounded(&self, t: f64, t_bound: f64) -> Result<(Point3, f64)> {
        let placement = self.placement()?;
        let r = self.radius;
        let p = placement.transform_point(Point3::new(r * t.cos(), r * t.sin(), 0.0));
        let c = self.centre;
        let size = c.x.abs().max(c.y.abs()).max(c.z.abs()) + 2.0 * r.abs();
        let bound = (2.0 * gamma(4) * size
            + 2.0 * gamma(3) * r.abs()
            + 2.0 * r.abs() * frame_error()
            + r.abs() * t_bound)
            * (1.0 + gamma(2));
        Ok((p, bound))
    }

    /// The angle of `p` about the circle's axis, from its reference (radians, in
    /// `[0, 2 pi)`).
    ///
    /// # Errors
    ///
    /// Those of [`ArcDefinition::placement`].
    pub fn angle_of(&self, p: Point3) -> Result<f64> {
        let placement = self.placement()?;
        let d = p - self.centre;
        let t = d.dot(placement.y_axis()).atan2(d.dot(placement.x_axis()));
        if t >= 0.0 {
            return Ok(t);
        }
        // A tiny negative angle rounds up to 2 pi: it is 0 to within its own size.
        let up = t + TAU;
        Ok(if up < TAU { up } else { 0.0 })
    }
}

impl Ellipse {
    /// The circle of an arc's definition ([`ArcDefinition::placement`]), its parameter
    /// starting at the arc's start: the placement's axes turned by `start` in their
    /// plane, its angular domain `[0, sweep]` in degrees.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a radius or angles not finite, a radius not above
    /// [`MIN_ARC`], or a sweep not in `(MIN_ARC, 2 pi]` (to a relative `1e-12`); those
    /// of [`ArcDefinition::placement`] and [`Ellipse::create_canonical_circle`].
    pub fn arc(k: &Kernel, arc: &ArcDefinition) -> Result<Self> {
        if ![arc.radius, arc.start, arc.end]
            .iter()
            .all(|x| x.is_finite())
        {
            return Err(SolidError::invalid_input(OP_ARC, "not finite"));
        }
        let placement = arc.placement()?;
        let sweep = arc.end - arc.start;
        let radius_ok = arc.radius > MIN_ARC;
        let sweep_ok = sweep > MIN_ARC && sweep <= TAU * (1.0 + 1e-12);
        if !(radius_ok && sweep_ok) {
            return Err(SolidError::invalid_input(
                OP_ARC,
                "radius or sweep out of range",
            ));
        }
        // The circle's own parameter starts at the arc's start: the kernel takes
        // intervals in degrees within [-360, 360]. Both axes stay in the placement's
        // plane.
        let (x, y) = (placement.x_axis(), placement.y_axis());
        let (cos, sin) = (arc.start.cos(), arc.start.sin());
        let frame = Axis2Placement::new(arc.centre, x * cos + y * sin, y * cos - x * sin);
        let degrees = Extent1d::from_min_max(0.0, (sweep * 180.0 / PI).min(360.0));
        Self::create_canonical_circle(k, &frame, arc.radius, Some(&degrees), None)
    }
}

/// Whether two arcs lie on one circle, and whether they share a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnOneCircle {
    /// The centres and radii within the tolerance, the axes the same way round and
    /// within it at the rim.
    pub circle: Verdict,
    /// The references within the tolerance at the rim (of the first arc's radius):
    /// the arcs measure their angles from one direction.
    pub frame: Verdict,
}

/// Whether arcs `a` and `b` lie on one circle to `tol`: their centres and radii within
/// it, and their axes the same way round and within it at the rim (`2 R sin(theta / 2)`,
/// `theta` the angle between them, `R` the larger radius); and whether their
/// placements' references are within it at the rim. Each comparison is certified
/// ([`Verdict`]): a quantity within its rounding bound of `tol` is
/// [`Verdict::Undecided`].
///
/// # Errors
///
/// Those of [`ArcDefinition::placement`].
pub fn arcs_on_one_circle(a: &ArcDefinition, b: &ArcDefinition, tol: f64) -> Result<OnOneCircle> {
    let (pa, pb) = (a.placement()?, b.placement()?);
    // The placements' Z axes, each the cross of its X and Y: within twice the frame error
    // and the cross's own rounding (4 gamma_2) of the exact unit axis.
    let (ka, kb) = (pa.z_axis(), pb.z_axis());
    let ez = 2.0 * frame_error() + 4.0 * gamma(2);
    let centres = distance(a.centre, b.centre).at_most(tol);
    let radii = difference(a.radius, b.radius).at_most(tol);
    let same_way = dot(ka, kb, ez, ez).above(0.0);
    // The rim's move between the two planes: 2 R sin(theta / 2) = R |kb - ka|, each axis
    // within ez, the difference rounding once per component (2 gamma_1 over the vector),
    // then the length's rounding.
    let turn = (kb - ka).length();
    let rim = Bounded {
        value: turn,
        bound: (2.0 * ez + 2.0 * gamma(1)) * (1.0 + gamma(5)) + gamma(5) * turn,
    };
    let tilt = scaled(rim, a.radius.max(b.radius)).at_most(tol);
    // |X_b - X_a| of the placements' references: each within the frame error, the
    // difference rounding once per component (2 gamma_1 over the vector), then the
    // length's rounding.
    let gap = (pb.x_axis() - pa.x_axis()).length();
    let references = Bounded {
        value: gap,
        bound: (2.0 * frame_error() + 2.0 * gamma(1)) * (1.0 + gamma(5)) + gamma(5) * gap,
    };
    Ok(OnOneCircle {
        circle: centres.and(radii).and(same_way).and(tilt),
        frame: scaled(references, a.radius).at_most(tol),
    })
}

/// Whether the end of curve `a` meets the start of curve `b` to `tol`: the distance
/// between their stored end points, certified.
///
/// # Errors
///
/// Those of [`Curve::ends`].
pub fn curves_meet(k: &Kernel, a: &Curve, b: &Curve, tol: f64) -> Result<Verdict> {
    let (ea, eb) = (a.ends(k)?, b.ends(k)?);
    Ok(distance(ea.end, eb.start).at_most(tol))
}

/// Whether `p` lies within `tol` of the point of `circle` at angle `t` (given within
/// `t_bound`), certified: the distance's rounding plus
/// [`ArcDefinition::point_at_bounded`]'s.
///
/// # Errors
///
/// Those of [`ArcDefinition::placement`].
pub fn at_angle(
    circle: &ArcDefinition,
    t: f64,
    t_bound: f64,
    p: Point3,
    tol: f64,
) -> Result<Verdict> {
    let (q, bound) = circle.point_at_bounded(t, t_bound)?;
    let d = distance(p, q);
    Ok(Bounded {
        value: d.value,
        bound: d.bound + bound,
    }
    .at_most(tol))
}

/// The running angles of a run of arcs on the frame of the first: the first's start
/// plus the sweeps so far, each with the bound on its rounding (`gamma_{i + 2}` of the
/// sizes summed: each sweep's subtraction and the sums').
fn running(run: &[&ArcDefinition], start: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let (mut t, mut size) = (start, start.abs());
    for (i, a) in run.iter().enumerate() {
        let s = a.sweep();
        t += s;
        size += s.abs();
        let n = i.saturating_add(2);
        out.push((t, gamma(n) * size));
    }
    out
}

/// How a run of arcs is joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// An open run: the one arc from the first piece's start through every piece's
    /// sweep.
    Open,
    /// A run round the whole circle: the full circle, its seam at angle 0 of the first
    /// piece's frame.
    ClosedAtFrameStart,
    /// A run round the whole circle: the full circle, its seam at the middle of the
    /// longest piece (the first of those longer than the others by no more than the
    /// tolerance at the rim).
    ClosedAtMiddleOfLongest,
}

/// The one arc of a run of arcs on one circle, end to end, on the first piece's circle
/// (its definition, turned to the run's start): open from the first piece's start
/// through the sum of their sweeps, or the full circle with its seam as `join` says.
/// The circle of [`Ellipse::arc`]. Returns the definition and its curve.
///
/// Every piece is compared with the first, so nothing drifts along the run: each on
/// the first's circle ([`arcs_on_one_circle`]) and each piece's end at the first's
/// circle's point at the running angle ([`at_angle`]), both to `tol`; consecutive
/// pieces (round the run for a closed join) end to end ([`curves_meet`]). A closed run
/// turns once: the sum of its sweeps is proved within half a turn of a whole turn.
/// TODO(d0b44726f): the joined arc's bound 0 rests on [`Ellipse::arc`]'s label until
/// the rebase onto d0b44726f.
///
/// # Errors
///
/// `InvalidInput` for an empty run, a piece without an arc's definition, or a run not
/// proved as above; those of [`Ellipse::arc`].
pub fn join_arcs(
    k: &Kernel,
    run: &[PathPiece],
    join: Join,
    tol: f64,
) -> Result<(ArcDefinition, Ellipse)> {
    let refuse = |why| Err(SolidError::invalid_input(OP_JOIN, why));
    let mut arcs = Vec::new();
    for p in run {
        let Some(a) = p.arc() else {
            return refuse("a piece without an arc's definition");
        };
        arcs.push(a);
    }
    let Some(&first) = arcs.first() else {
        return refuse("an empty run");
    };
    let angles = running(&arcs, first.start);
    for (i, (p, a)) in run.iter().zip(&arcs).enumerate() {
        let Some(&(t, t_bound)) = angles.get(i) else {
            return refuse("a run out of range");
        };
        let end = p.curve().ends(k)?.end;
        let on = arcs_on_one_circle(first, a, tol)?.circle;
        if !(on.is_yes() && at_angle(first, t, t_bound, end, tol)?.is_yes()) {
            return refuse("a piece not proved on the first's circle where the run reaches");
        }
        // The next piece round the run (the first after the last, for a closed join).
        let next = if i + 1 < run.len() {
            run.get(i + 1)
        } else if join != Join::Open {
            run.first()
        } else {
            None
        };
        if let Some(q) = next {
            if !curves_meet(k, p.curve(), q.curve(), tol)?.is_yes() {
                return refuse("pieces not proved end to end");
            }
        }
    }
    let Some(&(total, total_bound)) = angles.last() else {
        return refuse("an empty run");
    };
    let sweep = total - first.start;
    if join != Join::Open {
        let turn = Bounded {
            value: (sweep - TAU).abs(),
            bound: total_bound + gamma(2) * (total.abs() + first.start.abs() + TAU),
        };
        if !turn.at_most(PI).is_yes() {
            return refuse("a closed run that does not turn once");
        }
    }
    let (start, end) = match join {
        Join::Open => (first.start, total),
        Join::ClosedAtFrameStart => (0.0, TAU),
        Join::ClosedAtMiddleOfLongest => {
            // The first piece unless another is proved longer at the rim than the
            // tolerance: pieces equal to within it keep the earliest.
            let mut longest = 0;
            for (i, a) in arcs.iter().enumerate() {
                let Some(l) = arcs.get(longest) else {
                    return refuse("a run out of range");
                };
                let longer = Bounded {
                    value: a.sweep() - l.sweep(),
                    bound: gamma(2) * (a.sweep().abs() + l.sweep().abs()),
                };
                if scaled(longer, first.radius).above(tol).is_yes() {
                    longest = i;
                }
            }
            let (Some(l), before) = (arcs.get(longest), longest.checked_sub(1)) else {
                return refuse("a run out of range");
            };
            let from = match before.and_then(|j| angles.get(j)) {
                Some(&(t, _)) => t,
                None => first.start,
            };
            let seam = from + 0.5 * l.sweep();
            (seam, seam + TAU)
        }
    };
    let joined = ArcDefinition {
        start,
        end,
        ..*first
    };
    let curve = Ellipse::arc(k, &joined)?;
    Ok((joined, curve))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(start: f64, end: f64) -> ArcDefinition {
        ArcDefinition {
            centre: Point3::new(1.0, 2.0, 3.0),
            axis: Vec3::new(0.0, 0.0, 2.0),
            reference: Vec3::new(3.0, 0.0, 0.0),
            radius: 2.0,
            start,
            end,
        }
    }

    fn near(p: Point3, q: Point3) -> bool {
        (p - q).length() < 1e-12
    }

    fn pieces(k: &Kernel, defs: &[ArcDefinition]) -> Vec<PathPiece> {
        defs.iter()
            .map(|a| PathPiece::of_arc(k, a).unwrap())
            .collect()
    }

    #[test]
    fn an_arc_starts_and_ends_at_its_angles_on_its_circle() {
        let k = Kernel::default();
        let a = def(0.5 * PI, PI);
        let e = Ellipse::arc(&k, &a).unwrap();
        let ends = e.base().ends().unwrap();
        // The centre plus r (cos t, sin t, 0).
        assert!(near(ends.start, Point3::new(1.0, 4.0, 3.0)));
        assert!(near(ends.end, Point3::new(-1.0, 2.0, 3.0)));
        let iv = e.base().natural_interval();
        for i in 0..=8 {
            let t = iv.min() + (iv.max() - iv.min()) * f64::from(i) / 8.0;
            let p = e.base().evaluate_point(t).unwrap();
            assert!(((p - a.centre).length() - 2.0).abs() < 1e-12);
            assert!((p.z - 3.0).abs() < 1e-12);
            let angle = a.angle_of(p).unwrap();
            assert!((0.5 * PI - 1e-12..=PI + 1e-12).contains(&angle));
        }
        assert!(e.is_circle_class());
        assert!(near(
            a.point_at(0.75 * PI).unwrap(),
            Point3::new(1.0 - 2.0_f64.sqrt(), 2.0 + 2.0_f64.sqrt(), 3.0)
        ));
    }

    #[test]
    fn a_reference_off_square_is_projected_into_the_plane_of_the_axis() {
        // Accepted (1 - cos within SQUARE_TOL), but the circle lies in the plane square to
        // the axis, not tilted by the reference's axial component (5e-7 at this radius
        // without the projection).
        let k = Kernel::default();
        let a = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::Z,
            reference: Vec3::new(1.0, 0.0, 5e-10),
            radius: 1000.0,
            start: 0.3,
            end: 0.3 + TAU,
        };
        let e = Ellipse::arc(&k, &a).unwrap();
        let iv = e.base().natural_interval();
        for i in 0..=16 {
            let t = iv.min() + (iv.max() - iv.min()) * f64::from(i) / 16.0;
            let p = e.base().evaluate_point(t).unwrap();
            assert!(p.z.abs() < 1e-12, "{p:?}");
            assert!(a.point_at(t).unwrap().z.abs() < 1e-12);
        }
    }

    #[test]
    fn a_point_s_bound_covers_its_rounding() {
        let a = def(0.0, 1.0);
        let (p, bound) = a.point_at_bounded(1.0, 0.0).unwrap();
        let q = Point3::new(1.0 + 2.0 * 1.0_f64.cos(), 2.0 + 2.0 * 1.0_f64.sin(), 3.0);
        assert!((p - q).length() <= bound && bound < 1e-12);
        // An angle given within 1e-6: the bound grows by r 1e-6.
        let (_, wide) = a.point_at_bounded(1.0, 1e-6).unwrap();
        assert!(wide >= 2e-6);
    }

    #[test]
    fn an_angle_just_below_a_whole_turn_is_in_range() {
        let a = ArcDefinition {
            centre: Point3::ZERO,
            ..def(0.0, 1.0)
        };
        // atan2 gives -1e-20, which rounds to 2 pi when moved up a turn.
        let p = Point3::new(1.0, -1e-20, 0.0);
        let t = a.angle_of(p).unwrap();
        assert!((0.0..TAU).contains(&t), "{t}");
    }

    #[test]
    fn a_full_turn_closes() {
        let k = Kernel::default();
        let e = Ellipse::arc(&k, &def(1.0, 1.0 + TAU)).unwrap();
        let ends = e.base().ends().unwrap();
        assert!(near(ends.start, ends.end));
        assert!(near(ends.start, def(0.0, 1.0).point_at(1.0).unwrap()));
    }

    #[test]
    fn bad_definitions_are_refused() {
        let k = Kernel::default();
        let bad = [
            ArcDefinition {
                axis: Vec3::ZERO,
                ..def(0.0, 1.0)
            },
            ArcDefinition {
                reference: Vec3::new(1.0, 0.0, 1e-6),
                ..def(0.0, 1.0)
            },
            ArcDefinition {
                axis: Vec3::new(f64::NAN, 0.0, 1.0),
                ..def(0.0, 1.0)
            },
            ArcDefinition {
                radius: 0.0,
                ..def(0.0, 1.0)
            },
            def(1.0, 1.0),
            def(0.0, TAU + 1e-6),
            def(0.0, f64::NAN),
        ];
        for a in bad {
            assert!(
                matches!(Ellipse::arc(&k, &a), Err(SolidError::InvalidInput { .. })),
                "{a:?}"
            );
        }
        // No axis: no point and no angle either (no fallback to the given axis).
        let none = ArcDefinition {
            axis: Vec3::ZERO,
            ..def(0.0, 1.0)
        };
        assert!(none.point_at(0.5).is_err());
        assert!(none.angle_of(Point3::ZERO).is_err());
    }

    #[test]
    fn pieces_of_one_circle_lie_on_it_and_share_its_frame() {
        let tol = 1e-6;
        let one = arcs_on_one_circle(&def(0.0, 1.0), &def(1.0, 2.0), tol).unwrap();
        assert_eq!(one.circle, Verdict::Yes);
        assert_eq!(one.frame, Verdict::Yes);
        // Another reference: the same circle, another frame.
        let turned = ArcDefinition {
            reference: Vec3::new(0.0, 1.0, 0.0),
            ..def(0.0, 1.0)
        };
        let other = arcs_on_one_circle(&def(0.0, 1.0), &turned, tol).unwrap();
        assert_eq!(other.circle, Verdict::Yes);
        assert_eq!(other.frame, Verdict::No);
    }

    #[test]
    fn a_moved_arc_is_on_another_circle() {
        let tol = 1e-6;
        let a = def(0.0, 1.0);
        let moved = |d: Vec3| ArcDefinition {
            centre: a.centre + d,
            ..a
        };
        let far = arcs_on_one_circle(&a, &moved(Vec3::new(2e-6, 0.0, 0.0)), tol).unwrap();
        assert_eq!(far.circle, Verdict::No);
        // A centre exactly the tolerance away is not decided.
        let edge = ArcDefinition {
            centre: Point3::ZERO,
            ..a
        };
        let at = ArcDefinition {
            centre: Point3::new(tol, 0.0, 0.0),
            ..a
        };
        assert_eq!(
            arcs_on_one_circle(&edge, &at, tol).unwrap().circle,
            Verdict::Undecided
        );
        let bigger = ArcDefinition {
            radius: 2.0 + 2e-6,
            ..a
        };
        assert_eq!(
            arcs_on_one_circle(&a, &bigger, tol).unwrap().circle,
            Verdict::No
        );
        let reversed = ArcDefinition {
            axis: Vec3::new(0.0, 0.0, -1.0),
            ..a
        };
        assert_eq!(
            arcs_on_one_circle(&a, &reversed, tol).unwrap().circle,
            Verdict::No
        );
        // Tilted by 1e-6 radians: 2e-6 at the rim.
        let tilted = ArcDefinition {
            axis: Vec3::new(1e-6, 0.0, 1.0),
            reference: Vec3::new(1.0, 0.0, -1e-6),
            ..a
        };
        assert_eq!(
            arcs_on_one_circle(&a, &tilted, tol).unwrap().circle,
            Verdict::No
        );
    }

    #[test]
    fn the_tilt_is_measured_by_the_rim_s_move() {
        // Axes 60 degrees apart, radius 1: the rim moves 2 sin 30 = 1, more than the
        // 0.9 allowed, though R sin 60 = 0.87 is not.
        let a = ArcDefinition {
            centre: Point3::ZERO,
            axis: Vec3::Z,
            reference: Vec3::X,
            radius: 1.0,
            start: 0.0,
            end: 1.0,
        };
        let (s, c) = (PI / 3.0).sin_cos();
        let b = ArcDefinition {
            axis: Vec3::new(0.0, s, c),
            ..a
        };
        assert_eq!(arcs_on_one_circle(&a, &b, 0.9).unwrap().circle, Verdict::No);
        assert_eq!(
            arcs_on_one_circle(&a, &b, 1.1).unwrap().circle,
            Verdict::Yes
        );
    }

    #[test]
    fn an_open_run_joins_into_one_arc() {
        let k = Kernel::default();
        let run = pieces(&k, &[def(0.25, 1.0), def(1.0, 2.5)]);
        let (joined, e) = join_arcs(&k, &run, Join::Open, 1e-6).unwrap();
        assert!((joined.start - 0.25).abs() < 1e-12);
        assert!((joined.sweep() - 2.25).abs() < 1e-12);
        let ends = e.base().ends().unwrap();
        assert!(near(ends.start, def(0.0, 1.0).point_at(0.25).unwrap()));
        assert!(near(ends.end, def(0.0, 1.0).point_at(2.5).unwrap()));
    }

    #[test]
    fn a_closed_run_is_the_full_circle_with_its_seam_where_asked() {
        let k = Kernel::default();
        let run = pieces(&k, &[def(0.0, 1.0), def(1.0, 2.5), def(2.5, TAU)]);
        let (at_zero, _) = join_arcs(&k, &run, Join::ClosedAtFrameStart, 1e-6).unwrap();
        assert_eq!((at_zero.start, at_zero.end), (0.0, TAU));
        let (mid, e) = join_arcs(&k, &run, Join::ClosedAtMiddleOfLongest, 1e-6).unwrap();
        // The longest is 2.5 .. 2 pi: its middle.
        let seam = 0.5 * (2.5 + TAU);
        assert!((mid.start - seam).abs() < 1e-12);
        let ends = e.base().ends().unwrap();
        assert!(near(ends.start, ends.end));
        assert!(near(ends.start, def(0.0, 1.0).point_at(seam).unwrap()));
    }

    #[test]
    fn equal_pieces_put_the_seam_in_the_first() {
        // Four quarters, the third longer by a few ulps (as computed angles give): equal
        // within the tolerance, so the seam stays in the first, whatever the rounding.
        let k = Kernel::default();
        let q = 0.5 * PI;
        let ends = [0.0, q, 2.0 * q, 3.0 * q + 4e-15, TAU];
        let defs: Vec<ArcDefinition> = ends.windows(2).map(|w| def(w[0], w[1])).collect();
        let run = pieces(&k, &defs);
        let (mid, _) = join_arcs(&k, &run, Join::ClosedAtMiddleOfLongest, 1e-6).unwrap();
        assert_eq!(mid.start, 0.5 * q);
        // One longer at the rim than the tolerance: its middle.
        let ends = [0.0, q, 2.0 * q, 3.0 * q + 1e-3, TAU];
        let defs: Vec<ArcDefinition> = ends.windows(2).map(|w| def(w[0], w[1])).collect();
        let run = pieces(&k, &defs);
        let (mid, _) = join_arcs(&k, &run, Join::ClosedAtMiddleOfLongest, 1e-6).unwrap();
        assert!((mid.start - (2.0 * q + 0.5 * (q + 1e-3))).abs() < 1e-15);
    }

    #[test]
    fn a_run_off_one_circle_or_apart_is_refused() {
        let k = Kernel::default();
        let moved = ArcDefinition {
            centre: Point3::new(1.0, 2.0, 3.1),
            ..def(1.0, 2.0)
        };
        let gap = def(1.1, 2.0);
        for second in [moved, gap] {
            let run = pieces(&k, &[def(0.0, 1.0), second]);
            assert!(matches!(
                join_arcs(&k, &run, Join::Open, 1e-6),
                Err(SolidError::InvalidInput { .. })
            ));
        }
        assert!(join_arcs(&k, &[], Join::Open, 1e-6).is_err());
        // A piece that is a curve, not an arc's definition.
        let arc = Curve::from(Ellipse::arc(&k, &def(1.0, 2.0)).unwrap());
        let run = [
            PathPiece::of_arc(&k, &def(0.0, 1.0)).unwrap(),
            PathPiece::of_curve(arc),
        ];
        assert!(join_arcs(&k, &run, Join::Open, 1e-6).is_err());
    }

    #[test]
    fn a_run_drifting_piece_by_piece_is_refused() {
        // Each centre 0.9e-6 from the one before, tol 1e-6: consecutive pieces agree, but
        // the sixth is 4.5e-6 off the first's circle.
        let k = Kernel::default();
        let defs: Vec<ArcDefinition> = (0..6)
            .map(|i| ArcDefinition {
                centre: Point3::new(0.9e-6 * f64::from(i), 0.0, 0.0),
                axis: Vec3::Z,
                reference: Vec3::X,
                radius: 1.0,
                start: f64::from(i) * 0.5,
                end: f64::from(i + 1) * 0.5,
            })
            .collect();
        let run = pieces(&k, &defs);
        assert!(matches!(
            join_arcs(&k, &run, Join::Open, 1e-6),
            Err(SolidError::InvalidInput { .. })
        ));
        // The first two alone are within the tolerance.
        assert!(join_arcs(&k, &run[..2], Join::Open, 1e-6).is_ok());
    }

    #[test]
    fn a_closed_run_of_two_turns_is_refused() {
        // Four half arcs meet end to end round the run, but turn twice.
        let k = Kernel::default();
        let defs = [
            def(0.0, PI),
            def(PI, TAU),
            def(TAU, 3.0 * PI),
            def(3.0 * PI, 2.0 * TAU),
        ];
        let run = pieces(&k, &defs);
        for join in [Join::ClosedAtFrameStart, Join::ClosedAtMiddleOfLongest] {
            assert!(matches!(
                join_arcs(&k, &run, join, 1e-6),
                Err(SolidError::InvalidInput { .. })
            ));
        }
        // Two halves turn once.
        assert!(join_arcs(&k, &run[..2], Join::ClosedAtFrameStart, 1e-6).is_ok());
    }
}
