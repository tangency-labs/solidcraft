//! How far a fillet solver's trace extends its base surfaces' domains: the extension
//! factor's share, and the reach across the edge that SolidCraft's fork lets a trace
//! have so that a blend wider than its faces is traced as far as its rails.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork
//! (tangency-labs/smlib, rev `68fb9162`) adds `m_dTraceReach`, `m_abTraceAcross`,
//! `SetTraceReach` and `SmFilletSolver::GetDomainExtension` (`SmFilletSolver.h`,
//! `SmFilletSolver.cpp`) in commit `a9277fd` (WR-609). They are used only with the
//! opt-in `Kernel::fixes.fillet_trace_reaches_rails`: with it off the reach stays `0`
//! and [`FilletSolver::domain_extension`] is the extension factor times the domain's
//! size, as upstream's `SetupOffsetExtension` computes it.
//!
//! | Rust                               | Upstream (fork)                      |
//! |------------------------------------|--------------------------------------|
//! | [`FilletSolver::domain_extension`] | `SmFilletSolver::GetDomainExtension` |
//! | [`FilletSolver::set_trace_reach`]  | `SmFilletSolver::SetTraceReach`      |
//!
//! Not the fork's: [`FilletSolver::domain_extension`] ends the extension of a direction
//! the surface turns back on itself in where its two sides' continuations meet
//! (`closing_extension`), whatever the factor and the reach ask: past that meeting the
//! continued surface holds points twice. The fork's function is
//! `FilletSolver::reaching_extension`, unchanged.
//!
//! Deviations: the fork tells which of the solver's base surfaces `pSurf` is by its
//! address. The port's callers pass copies of the base surfaces, so a surface is taken
//! for a solver's base surface when it is that object or shares its cache slot, or
//! else when the base surface has the same natural domain and gives bit-identical
//! points at the five parameter points the extension evaluates (the four ends and
//! the middle of the domain's middle lines); as in the fork, the last of the two that
//! matches wins. A failed evaluation gives the factor's extension, as the fork's does,
//! except that [`SolidError::Unported`](crate::error::SolidError::Unported) propagates. There is no recursion.

use super::p1::FilletSolver;
use crate::error::Result;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::{Point2, Vec2};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO_SQRT};
use crate::surface::surface::Surface;

/// `smos_Max` of two doubles (the first when it is larger, else the second).
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles (the first when it is smaller, else the second).
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// The four points `GetDomainExtension` evaluates: the ends of the domain's middle
/// lines, `[u min, u max, v min, v max]`.
fn domain_ends(k: &Kernel, surface: &Surface, domain: &Extent2d) -> Result<Option<[Point3; 4]>> {
    let mid = domain.mid();
    let uvs = [
        Point2::new(domain.min.x, mid.y),
        Point2::new(domain.max.x, mid.y),
        Point2::new(mid.x, domain.min.y),
        Point2::new(mid.x, domain.max.y),
    ];
    let mut ends = [Point3::UNDEF; 4];
    for (end, uv) in ends.iter_mut().zip(uvs) {
        match surface.evaluate_point(k, uv) {
            Ok(p) => *end = p,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(ends))
}

/// How far past its two ends, in its parameter, a parameter line can be continued before
/// the continuations meet. The line leaves its low end at `low` (its point and its first
/// derivative there) against that derivative and its high end at `high` along it; the
/// two tangent lines are nearest each other the lengths `s` and `t` on from the ends.
/// Where both are ahead, the line turns back on itself (more than half a turn between
/// its ends: an arc most of the way round its axis), and continued further than that
/// the two continuations cross, so the extended line holds a point twice. The lengths
/// are `((B - A) x b) . n / n . n` and `((B - A) x a) . n / n . n` with `n = a x b`, the
/// form that stays determined as the directions become parallel (a half turn, where the
/// lines meet at infinity); each is taken to the parameter by its end's own speed, and
/// the lesser is returned. `None` where the tangent lines do not both run on towards
/// their meeting (the line does not turn back: a straight line, an arc of at most half
/// a turn), where they are parallel, or where an end has no direction; and for a closed
/// line, its ends one point (no farther apart than a direction of no length in 3D is
/// told by, `EFF_ZERO_SQRT`): its direction is periodic, not extended to a meeting, and
/// what rounding leaves between its ends has no meeting to measure.
fn closing_extension(low: (Point3, Vec3), high: (Point3, Vec3)) -> Option<f64> {
    let (a_pnt, a_der) = low;
    let (b_pnt, b_der) = high;
    let (a_speed, b_speed) = (a_der.length(), b_der.length());
    // (`!( x > 0.0 )`: a NaN speed has no direction either)
    let directed = a_speed > 0.0 && b_speed > 0.0;
    if !directed {
        return None;
    }
    let w = b_pnt - a_pnt;
    let open = w.length() > EFF_ZERO_SQRT;
    if !open {
        return None;
    }
    let a = a_der * (-1.0 / a_speed);
    let b = b_der * (1.0 / b_speed);
    let n = a.cross(b);
    let nn = n.dot(n);
    let crossing = nn > 0.0;
    if !crossing {
        return None;
    }
    let s = w.cross(b).dot(n) / nn;
    let t = w.cross(a).dot(n) / nn;
    let ahead = s > 0.0 && t > 0.0;
    if !ahead {
        return None;
    }
    Some(smos_min(s / a_speed, t / b_speed))
}

/// [`closing_extension`] of the middle lines of `domain` on `surface`, `[u, v]`.
fn closing_extensions(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
) -> Result<[Option<f64>; 2]> {
    let mid = domain.mid();
    let end = |uv: Point2, low: bool| surface.evaluate_1st_derivatives(k, uv, !low, !low);
    let (u_low, u_low_du, _) = end(Point2::new(domain.min.x, mid.y), true)?;
    let (u_high, u_high_du, _) = end(Point2::new(domain.max.x, mid.y), false)?;
    let (v_low, _, v_low_dv) = end(Point2::new(mid.x, domain.min.y), true)?;
    let (v_high, _, v_high_dv) = end(Point2::new(mid.x, domain.max.y), false)?;
    Ok([
        closing_extension((u_low, u_low_du), (u_high, u_high_du)),
        closing_extension((v_low, v_low_dv), (v_high, v_high_dv)),
    ])
}

/// Whether `candidate` stands for the solver's base surface `base` (the fork compares
/// addresses; see the module's deviations).
fn is_same_surface(
    k: &Kernel,
    base: &Surface,
    candidate: &Surface,
    domain: &Extent2d,
    candidate_ends: &[Point3; 4],
) -> Result<bool> {
    if std::ptr::eq(base, candidate) || base.shares_cache_slot_with(candidate) {
        return Ok(true);
    }
    let same_domain = match (
        base.natural_uv_domain_with_nurb(k),
        candidate.natural_uv_domain_with_nurb(k),
    ) {
        (Ok(a), Ok(b)) => a.min == b.min && a.max == b.max,
        (Err(e), _) | (_, Err(e)) if e.is_unported_or_cancelled() => return Err(e),
        _ => false,
    };
    if !same_domain {
        return Ok(false);
    }
    let Some(base_ends) = domain_ends(k, base, domain)? else {
        return Ok(false);
    };
    if base_ends != *candidate_ends {
        return Ok(false);
    }
    let mid = domain.mid();
    match (
        base.evaluate_point(k, mid),
        candidate.evaluate_point(k, mid),
    ) {
        (Ok(a), Ok(b)) => Ok(a == b),
        (Err(e), _) | (_, Err(e)) if e.is_unported_or_cancelled() => Err(e),
        _ => Ok(false),
    }
}

impl FilletSolver {
    /// `SetTraceReach(dReach, crabAcross)` (fork, WR-609): how far (3D) the trace's base
    /// surface domains reach past their own at least, across the edge (`0` = by the
    /// extension factor alone), and which parameter directions of each base surface
    /// (`[surface][u, v]`) run across the edge.
    pub fn set_trace_reach(&mut self, reach: f64, across: [[bool; 2]; 2]) {
        self.trace_reach = reach;
        self.trace_across = across;
    }

    /// How far the trace extends a base surface's domain past `domain`, in each parameter
    /// direction: the fork's `GetDomainExtension` (`FilletSolver::reaching_extension`),
    /// and in a direction the surface turns back on itself in, no further than where its
    /// two sides' continuations meet (`closing_extension`, on the domain's middle line).
    ///
    /// Not the fork's: the factor and the reach are lengths chosen without regard to
    /// where the surface's other side is. An arc most of the way round its axis, grown
    /// by either, is carried across the turn it leaves open and on over itself: the
    /// B-spline's own continuation names points the surface already holds by a second
    /// parameter pair, and the offset's tangent extension is a second sheet over them. A
    /// section seeded there is solved on that sheet, not on the face. Up to the meeting,
    /// the continued line is still one line, each of its points held once. (A surface
    /// whose offset continues as itself closes its turn there, and wraps:
    /// `surface::offset_surface::closed_turn`.) A closed direction is periodic
    /// and keeps its extension; so does a `None` surface, which has no line to measure.
    ///
    /// # Errors
    ///
    /// Those of `FilletSolver::reaching_extension` and of the surface's evaluation at
    /// the ends of the domain's middle lines.
    pub fn domain_extension(
        &self,
        k: &Kernel,
        surface: Option<&Surface>,
        domain: &Extent2d,
    ) -> Result<Vec2> {
        let mut size = self.reaching_extension(k, surface, domain)?;
        let Some(surface) = surface else {
            return Ok(size);
        };
        let [closing_u, closing_v] = closing_extensions(k, surface, domain)?;
        if let Some(closing) = closing_u {
            size.x = smos_min(size.x, closing);
        }
        if let Some(closing) = closing_v {
            size.y = smos_min(size.y, closing);
        }
        Ok(size)
    }

    /// `GetDomainExtension(pSurf, crDomain)` (fork, WR-609): how far the trace extends
    /// a base surface's domain past `domain`, in each parameter direction, by its factor
    /// and its reach.
    ///
    /// The extension factor times the domain's size (upstream), and in the directions
    /// that run across the edge ([`FilletSolver::set_trace_reach`]) at least as far as
    /// the trace's reach in 3D: the size in parameters scaled by the domain's length in
    /// 3D along that direction, through its middle. Along the edge it keeps the
    /// factor's. A direction of no length in 3D keeps the factor's. `surface` (`None`
    /// for the fork's `NULL`) need not be one of the solver's base surfaces; one that
    /// is not takes the reach in both directions.
    ///
    /// # Errors
    ///
    /// [`crate::error::SolidError::Unported`] from the evaluations; other failures give
    /// the factor's extension, as the fork's does.
    fn reaching_extension(
        &self,
        k: &Kernel,
        surface: Option<&Surface>,
        domain: &Extent2d,
    ) -> Result<Vec2> {
        let mut size: Vec2 = self.surface_extension_factor * domain.size();
        let Some(surface) = surface else {
            return Ok(size);
        };
        // `!( m_dTraceReach > 0.0 )`: a NaN reach returns too
        let reaches = self.trace_reach > 0.0;
        if !reaches {
            return Ok(size);
        }
        let Some(ends) = domain_ends(k, surface, domain)? else {
            return Ok(size);
        };
        // which of the solver's base surfaces this is: its directions across the edge
        let mut which = None;
        for (index, slot) in self.surfaces.iter().enumerate() {
            if let Some(slot) = slot {
                if is_same_surface(k, slot.surface.base_surface(), surface, domain, &ends)? {
                    which = Some(index);
                }
            }
        }
        let across = which
            .and_then(|index| self.trace_across.get(index))
            .copied()
            .unwrap_or([true, true]);
        let [across_u, across_v] = across;
        let uv_size = domain.size();
        let [u_min, u_max, v_min, v_max] = ends;
        let d3d_u = u_min.distance(u_max);
        let d3d_v = v_min.distance(v_max);
        if across_u && d3d_u > EFF_ZERO_SQRT {
            size.x = smos_max(size.x, self.trace_reach * uv_size.x / d3d_u);
        }
        if across_v && d3d_v > EFF_ZERO_SQRT {
            size.y = smos_max(size.y, self.trace_reach * uv_size.y / d3d_v);
        }
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_surface::BSplineSurface;

    fn plane(_k: &Kernel) -> Surface {
        Surface::from(
            BSplineSurface::create_bilinear_surface(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Point3::new(0.0, 4.0, 0.0),
                Point3::new(10.0, 4.0, 0.0),
            )
            .unwrap(),
        )
    }

    #[test]
    fn no_reach_is_the_factor_times_the_size() {
        let k = Kernel::default();
        let s = FilletSolver::new(1e-6, 1e-3, 1e-6);
        let surface = plane(&k);
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let size = s.domain_extension(&k, Some(&surface), &domain).unwrap();
        let expect = 0.5 * domain.size();
        assert_eq!((size.x, size.y), (expect.x, expect.y));
        assert_eq!(s.domain_extension(&k, None, &domain).unwrap(), expect);
    }

    #[test]
    fn a_reach_grows_only_the_directions_across() {
        let k = Kernel::default();
        let mut s = FilletSolver::new(1e-6, 1e-3, 1e-6);
        let surface = plane(&k);
        let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let factor = 0.5 * domain.size();
        // a surface that is none of the solver's: both directions get the reach
        s.set_trace_reach(40.0, [[false, true], [true, true]]);
        let size = s.domain_extension(&k, Some(&surface), &domain).unwrap();
        let ends = domain_ends(&k, &surface, &domain).unwrap().unwrap();
        let (du, dv) = (ends[0].distance(ends[1]), ends[2].distance(ends[3]));
        assert_eq!(size.x, 40.0 * domain.size().x / du);
        assert_eq!(size.y, 40.0 * domain.size().y / dv);
        assert!(size.x > factor.x && size.y > factor.y);
    }

    /// A cylinder of radius `radius` about the z axis from height 0 to 4: its arc of `turn`
    /// degrees from the x axis, `spans` equal rational quadratic spans over u from 0 to 1
    /// (a B-spline, with no analytic kind).
    fn cylinder_arc(radius: f64, turn: f64, spans: usize) -> Surface {
        use crate::nurbs::b_spline_curve::KnotType;
        use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};
        let step = (turn / spans as f64).to_radians();
        let mid_weight = (0.5 * step).cos();
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for i in 0..=2 * spans {
            let angle = 0.5 * step * i as f64;
            let (r, w) = if i % 2 == 0 {
                (radius, 1.0)
            } else {
                (radius / mid_weight, mid_weight)
            };
            for z in [0.0, 4.0] {
                points.push(Point3::new(r * angle.cos(), r * angle.sin(), z));
                weights.push(w);
            }
        }
        let u_knots: Vec<f64> = (0..=spans).map(|i| i as f64 / spans as f64).collect();
        let mut u_mults = vec![2; spans + 1];
        u_mults[0] = 3;
        u_mults[spans] = 3;
        Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 2,
                v_degree: 1,
                control_points: &points,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &u_mults,
                v_multiplicities: &[2, 2],
                u_knots: &u_knots,
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&weights),
                uv_domain: None,
            })
            .unwrap(),
        )
    }

    /// How far round the axis, in degrees, the arc's own continuation has gone past its low
    /// side at `u` below its domain (the B-spline evaluated out of bounds, as a drop over the
    /// trace's domain evaluates it).
    fn turned_past_the_low_side(k: &Kernel, arc: &Surface, u: f64) -> f64 {
        use crate::surface::surface::SurfaceKind;
        let mut continued = arc.clone();
        match continued.kind_mut() {
            SurfaceKind::BSpline(b) => *b.out_of_bounds_enabled_mut() = true,
            _ => panic!("the arc is a B-spline"),
        }
        let p = continued.evaluate_point(k, Point2::new(u, 0.5)).unwrap();
        // the low side is on the x axis and the arc runs anticlockwise from it
        (-p.y).atan2(p.x).to_degrees().rem_euclid(360.0)
    }

    #[test]
    fn a_line_that_does_not_turn_back_has_no_meeting() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let at = |px: f64, py: f64| Point3::new(px, py, 0.0);
        // a straight line; a quarter turn; a half turn (its tangent lines parallel)
        assert_eq!(
            closing_extension((at(0.0, 0.0), x), (at(5.0, 0.0), x)),
            None
        );
        assert_eq!(
            closing_extension((at(1.0, 0.0), y), (at(0.0, 1.0), -x)),
            None
        );
        assert_eq!(
            closing_extension((at(1.0, 0.0), y), (at(-1.0, 0.0), -y)),
            None
        );
        // a closed line: its ends one point, to rounding, whatever its directions there
        assert_eq!(
            closing_extension((at(1.0, 0.0), y), (at(1.0, 0.0), y)),
            None
        );
        let skew = Vec3::new(1e-16, 1.0, 3e-16);
        assert_eq!(
            closing_extension((at(1.0, 0.0), y), (at(1.0, 2e-15), skew)),
            None
        );
        // a straight line whose ends' directions differ by rounding
        let (up, down) = (Vec3::new(1.0, 1e-17, 0.0), Vec3::new(1.0, 0.0, -2e-17));
        assert_eq!(
            closing_extension((at(0.0, 0.0), up), (at(5.0, 0.0), down)),
            None
        );
        // an end with no direction
        assert_eq!(
            closing_extension((at(0.0, 0.0), Vec3::ZERO), (at(0.0, 1.0), x)),
            None
        );
    }

    #[test]
    fn three_quarters_of_a_turn_meets_a_radius_past_its_ends() {
        // the unit circle from (1, 0) anticlockwise to (0, -1), at speeds 2 and 4: the
        // tangent lines x = 1 and y = -1 meet at (1, -1), a radius from either end
        let low = (Point3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0));
        let high = (Point3::new(0.0, -1.0, 0.0), Vec3::new(4.0, 0.0, 0.0));
        let closing = closing_extension(low, high).unwrap();
        assert!((closing - 0.25).abs() < 1e-15, "{closing}");
    }

    #[test]
    fn an_arc_past_half_a_turn_is_extended_to_where_its_sides_meet() {
        let k = Kernel::default();
        let mut s = FilletSolver::new(1e-6, 1e-3, 1e-6);
        let arc = cylinder_arc(10.0, 300.0, 4);
        let domain = arc.natural_uv_domain_with_nurb(&k).unwrap();
        // the tangent lines of the sides meet 10 tan 30 from each, and a span of 75 degrees
        // over a quarter of the domain leaves its end at 8 sin 37.5 radians per unit of u
        let meeting = 30.0_f64.to_radians().tan() / (8.0 * 37.5_f64.to_radians().sin());
        let size = s.domain_extension(&k, Some(&arc), &domain).unwrap();
        assert!((size.x - meeting).abs() < 1e-13, "{} {meeting}", size.x);
        // along the axis the cylinder is straight: the factor's
        assert_eq!(size.y, 0.5);
        // a reach is stopped there too
        s.set_trace_reach(40.0, [[true, true], [true, true]]);
        let reached = s.domain_extension(&k, Some(&arc), &domain).unwrap();
        assert_eq!(reached.x, size.x);
        assert!(reached.y > size.y);
    }

    #[test]
    fn an_arc_of_at_most_half_a_turn_keeps_its_extension() {
        let k = Kernel::default();
        for (turn, spans) in [(90.0, 1), (180.0, 2), (179.999_999, 2)] {
            let mut s = FilletSolver::new(1e-6, 1e-3, 1e-6);
            let arc = cylinder_arc(10.0, turn, spans);
            let domain = arc.natural_uv_domain_with_nurb(&k).unwrap();
            let size = s.domain_extension(&k, Some(&arc), &domain).unwrap();
            assert_eq!((size.x, size.y), (0.5, 0.5), "{turn}");
            s.set_trace_reach(40.0, [[true, true], [true, true]]);
            let size = s.domain_extension(&k, Some(&arc), &domain).unwrap();
            let ends = domain_ends(&k, &arc, &domain).unwrap().unwrap();
            assert_eq!(size.x, 40.0 / ends[0].distance(ends[1]), "{turn}");
        }
    }

    /// An arc that leaves 23.6 degrees of its turn open (a chamfer's cone round most of a
    /// boss): grown by the factor alone, its own continuation crosses the gap and runs on
    /// over the arc, so parameters past the side name points the arc already holds; grown
    /// to where its sides meet, it stays inside the gap.
    #[test]
    fn an_arc_most_of_the_way_round_is_not_extended_over_itself() {
        let k = Kernel::default();
        let gap = 23.6;
        let arc = cylinder_arc(15.0, 360.0 - gap, 5);
        let domain = arc.natural_uv_domain_with_nurb(&k).unwrap();
        let mut s = FilletSolver::new(1e-6, 1e-3, 1e-6);
        s.set_trace_reach(24.8, [[true, false], [true, true]]);
        let fork = s.reaching_extension(&k, Some(&arc), &domain).unwrap();
        assert!(turned_past_the_low_side(&k, &arc, domain.min.x - fork.x) > gap);
        let factor = 0.5 * domain.size().x;
        assert!(turned_past_the_low_side(&k, &arc, domain.min.x - factor) > gap);
        let size = s.domain_extension(&k, Some(&arc), &domain).unwrap();
        assert!(size.x < factor);
        let turned = turned_past_the_low_side(&k, &arc, domain.min.x - size.x);
        assert!(turned > 0.0 && turned < 0.5 * gap + 1.0, "{turned}");
    }
}
