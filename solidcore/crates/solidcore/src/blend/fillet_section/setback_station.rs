//! The station of a chamfer whose sizes are setbacks held at every station of its edge (not
//! upstream): the feet on the two supports, as pure functions of the edge's jet and the
//! supports' partials. Nothing builds such a chamfer yet: the record kind that holds one
//! (`ChamferKindRecord::Setbacks`, `RadiusRecord::Edge` in
//! [`crate::surface::procedural_blend::record`]) is refused when read, and these functions are
//! the station its tracer and certificate will solve.
//!
//! **The station.** At a parameter s of the edge C, foot i is the point S of support i with
//!
//! `|S − C(s)| = d_i` and `(S − C(s)) · C′(s) = 0`:
//!
//! a straight line of length `d_i` from the edge, in the plane square to the edge (a setback).
//! The chamfer's section there is the chord between the two feet. Two distances give two
//! independent 2 × 2 systems in each support's (u, v) ([`setback_foot`]). A chord `c` couples
//! them through one setback `d`, with `|F_1 − F_2| = c` ([`chord_setback`]). Newton starts at
//! `C + d t`, `t` the unit tangent into the face square to the edge ([`setback_tangent`],
//! [`setback_start`]), not on the edge itself, where `|S − C|` has no derivative. Between
//! planes the feet are those of today's offsets chamfer, so the two kinds agree there.
//!
//! **What the definition keeps.** The leader is the edge, extended past its ends and stored
//! as written (a spline edge's extension is then the stored curve, not a rule applied again
//! on reading). The radius is no ball (`RadiusRecord::Edge`): no offsets, no law. The seeds
//! are each foot's (u, v) over s, the stations' Newton starts. Each support's side is the side
//! of it, along its normal at the edge station, on which the wedge's interior lies: the side
//! today's chamfer's ball sits on.
//!
//! **The three definitions a setback chamfer is to be built under** (its tracer and
//! certificate, which apply them, are not built yet).
//! - **Crossing.** Neighbouring stations' planes meet on the line `(X − C) · C″ = |C′|²` of
//!   each plane (the derivative in s of `(X − C) · C′ = 0`): the polar line, `1/κ` from the
//!   edge along its principal normal n (a rim's axis). A foot past it lies on stations that
//!   cross. The chamfer will be refused only where it is not proved, by enclosure over the span,
//!   that `(S − C) · C″ < |C′|²`, that is `κ (S − C) · n < 1` (equal since
//!   `(S − C) · C′ = 0`, and so the same under any reparametrisation of the edge). A straight
//!   edge (`C″ = 0` in arc length) is no special case. The condition is linear in the point,
//!   so where both feet meet it the whole chord does, and there the section's map is regular
//!   (the foot moves along the edge at `S_s · C′ = |C′|² − (S − C) · C″ > 0`). Feet on the
//!   convex side of the edge never cross: a chamfer of 6 around a hole of radius 5 is not
//!   refused by it.
//! - **The foot.** Foot i is the first transversal crossing of support i's section with the
//!   circle of radius `d_i` about the edge point, counted from the edge, with a proof that no
//!   nearer root exists. A tangency, a double root, a section that never reaches `d_i`
//!   (`|d k| ≥ 2` on a circle of curvature k), or a foot past the polar line does not fit:
//!   the asked chamfer is too large there.
//! - **Ends and corners.** Stations past an edge's ends will stand on the stored extended
//!   leader. Corners will keep today's patches between the rails' ends. Rails are per edge, so a tangent
//!   chain's one wedge (`chain_wedge`) has no part in a setback chamfer.
//!
//! **The net's bound (an affine lemma).** Where, at each s, the net B and the definition S are
//! both affine across the section on the same map t ∈ [0, 1], with B's ends on the rails' nets
//! `B_1(s)`, `B_2(s)` and S's at the feet `F_1(s)`, `F_2(s)`, then
//! `B − S = (1 − t)(B_1 − F_1) + t (B_2 − F_2)`, a convex function of t in norm, so
//! `max_t |B − S| = max_i |B_i − F_i|`:
//! the net's bound is the rails' larger bound. The lemma has two hypotheses the certificate
//! checks rather than assumes: the net is of degree 1 across with equal weights in each
//! cross-section (a rational net with unequal weights there is projective in t, not affine),
//! and the net's s along the rails is the definition's s through the stored map.
//!
//! The certified enclosures of these systems over an s-span (Krawczyk's operator on the foot
//! system with s as an interval, the first-crossing exclusion, the crossing condition by
//! enclosure) are not here yet; the functions below are the point solves they start from.
//! The tests check them against the closed form on a circle section of curvature k,
//! `F = C + d (cos β t + sin β w)`, `β = asin(d k / 2)`.
//!
//! Deviations: not the fork's (no counterpart). Every loop has a fixed count
//! ([`NEWTON_STEPS`] Newton steps for a foot, as many for the chord's setback), no recursion.

use super::station::{surface_partials, vec3, STATION_STEP};
use crate::error::Result;
use crate::geom::krawczyk::{inverse_of_columns2, NEWTON_STEPS};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3};
use crate::surface::surface::Surface;

/// The edge at one station: its point `C(s)` and its first and second derivatives in its own
/// parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeJet {
    /// `C(s)`.
    pub point: Point3,
    /// `C′(s)`.
    pub d1: Vec3,
    /// `C″(s)`.
    pub d2: Vec3,
}

/// A foot of a setback chamfer on its support: its parameters, its point, and how it moves
/// as the setback grows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetbackFoot {
    /// The foot's (u, v) on the support.
    pub uv: Point2,
    /// The foot `S(u, v)`.
    pub point: Point3,
    /// `∂S/∂d`: the foot's motion per unit of setback at this station (the implicit function
    /// theorem on the foot's system).
    pub along: Vec3,
}

/// The feet of a chord at one station, and the equal setback that puts them `chord` apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChordFeet {
    /// The equal setback.
    pub d: f64,
    /// The feet on supports 1 and 2.
    pub feet: [SetbackFoot; 2],
}

/// The support's point and first partials at `uv`.
fn support_jet(k: &Kernel, support: &Surface, uv: Point2) -> Result<(Point3, Vec3, Vec3)> {
    let p = surface_partials(k, support, uv, 1)?;
    Ok((vec3(p.at(0, 0)), vec3(p.at(1, 0)), vec3(p.at(0, 1))))
}

/// `m x` for a 2 × 2 matrix by rows.
fn times(m: &[[f64; 2]; 2], x: [f64; 2]) -> [f64; 2] {
    [
        m[0][0] * x[0] + m[0][1] * x[1],
        m[1][0] * x[0] + m[1][1] * x[1],
    ]
}

/// The unit tangent `t` into the face at the edge point: square to `C′` in the support's
/// tangent plane at `edge_uv` (the edge point's parameters on the support), on the side of
/// `into` (any vector pointing from the edge into the face). `None` where the support is
/// singular there, `C′` is normal to it, or `into` lies in the plane of `C′` and the normal.
///
/// # Errors
///
/// Those of the support's evaluation.
pub fn setback_tangent(
    k: &Kernel,
    support: &Surface,
    edge_uv: Point2,
    edge: &EdgeJet,
    into: Vec3,
) -> Result<Option<Vec3>> {
    let (_, su, sv) = support_jet(k, support, edge_uv)?;
    let b = su.cross(sv).cross(edge.d1);
    let length = b.length();
    if !(length.is_finite() && length > 0.0) {
        return Ok(None);
    }
    let t = b * (1.0 / length);
    let side = t.dot(into);
    Ok(if !side.is_finite() || side == 0.0 {
        None
    } else if side > 0.0 {
        Some(t)
    } else {
        Some(-t)
    })
}

/// The (u, v) of `C + d t` on the support to first order, Newton's start for the foot at
/// setback `d`: `edge_uv + d (JᵀJ)⁻¹ Jᵀ t`, `J = [S_u S_v]` at `edge_uv`. `None` where the
/// support is singular there or `d` is not finite and positive.
///
/// # Errors
///
/// Those of the support's evaluation.
pub fn setback_start(
    k: &Kernel,
    support: &Surface,
    edge_uv: Point2,
    t: Vec3,
    d: f64,
) -> Result<Option<Point2>> {
    if !(d.is_finite() && d > 0.0) {
        return Ok(None);
    }
    let (_, su, sv) = support_jet(k, support, edge_uv)?;
    let gram = [[su.dot(su), su.dot(sv)], [su.dot(sv), sv.dot(sv)]];
    let Some(inverse) = inverse_of_columns2(&gram) else {
        return Ok(None);
    };
    let [du, dv] = times(&inverse, [d * su.dot(t), d * sv.dot(t)]);
    let start = Point2::new(edge_uv.x + du, edge_uv.y + dv);
    Ok((start.x.is_finite() && start.y.is_finite()).then_some(start))
}

/// The foot at setback `d` on the support: Newton from `start` on
///
/// `F(u, v) = [(|S − C|² − d²) / 2, (S − C) · C′]`,
///
/// whose Jacobian has the columns `[(S − C) · S_u, S_u · C′]` and `[(S − C) · S_v, S_v · C′]`,
/// for at most [`NEWTON_STEPS`] steps, stopping once a step in each parameter x is at most
/// [`STATION_STEP`] `(1 + |x|)`: the step only, where the offsets' station solve also asks for
/// its residual within the tolerance. `None` where a step is not defined (a singular
/// Jacobian: a tangency of the section with the circle of radius `d`) or the steps do not
/// settle. The root is the one Newton reaches from `start`; that it is the
/// first crossing from the edge is the enclosure's to prove (module docs).
///
/// # Errors
///
/// Those of the support's evaluation.
pub fn setback_foot(
    k: &Kernel,
    support: &Surface,
    edge: &EdgeJet,
    d: f64,
    start: Point2,
) -> Result<Option<SetbackFoot>> {
    if !(d.is_finite() && d > 0.0) {
        return Ok(None);
    }
    let mut uv = start;
    for _ in 0..NEWTON_STEPS {
        let (s, su, sv) = support_jet(k, support, uv)?;
        let g = s - edge.point;
        let f = [0.5 * (g.dot(g) - d * d), g.dot(edge.d1)];
        let columns = [[g.dot(su), su.dot(edge.d1)], [g.dot(sv), sv.dot(edge.d1)]];
        let Some(inverse) = inverse_of_columns2(&columns) else {
            return Ok(None);
        };
        let [du, dv] = times(&inverse, f);
        if !(du.is_finite() && dv.is_finite()) {
            return Ok(None);
        }
        uv = Point2::new(uv.x - du, uv.y - dv);
        if du.abs() <= STATION_STEP * (1.0 + uv.x.abs())
            && dv.abs() <= STATION_STEP * (1.0 + uv.y.abs())
        {
            return foot_at(k, support, edge, d, uv);
        }
    }
    Ok(None)
}

/// The foot at `uv`, the root of the foot's system for setback `d`, with its motion in d:
/// `∂(u, v)/∂d = J⁻¹ (d, 0)` (the system's partial in d is `(−d, 0)`), so
/// `∂S/∂d = S_u ∂u/∂d + S_v ∂v/∂d`.
fn foot_at(
    k: &Kernel,
    support: &Surface,
    edge: &EdgeJet,
    d: f64,
    uv: Point2,
) -> Result<Option<SetbackFoot>> {
    let (s, su, sv) = support_jet(k, support, uv)?;
    let g = s - edge.point;
    let columns = [[g.dot(su), su.dot(edge.d1)], [g.dot(sv), sv.dot(edge.d1)]];
    let Some(inverse) = inverse_of_columns2(&columns) else {
        return Ok(None);
    };
    let [du, dv] = times(&inverse, [d, 0.0]);
    let along = su * du + sv * dv;
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    Ok((finite(s) && finite(along)).then_some(SetbackFoot {
        uv,
        point: s,
        along,
    }))
}

/// The equal setback whose feet on `supports` are `chord` apart, and the feet: Newton in d on
/// `g(d) = |F_1(d) − F_2(d)|² − chord²`, `g′(d) = 2 (F_1 − F_2) · (∂F_1/∂d − ∂F_2/∂d)`, each
/// foot solved by [`setback_foot`] from the last one's parameters. It starts at the straight
/// sections' setback `chord / (2 sin(α / 2))`, α the angle between the faces' tangents `t`
/// (each from [`setback_tangent`]), from the edge points' parameters `edge_uvs` through
/// [`setback_start`]. At most [`NEWTON_STEPS`] steps, stopping once a step is at most
/// [`STATION_STEP`] `(1 + d)`. `None` where a foot is not found, the setback leaves the
/// positive numbers, the steps do not settle, or the feet's distance is not growing with d at
/// the root (`g′ ≤ 0`: past the branch on which a chord has one setback). This is a point
/// solve: a `Some` proves nothing and a `None` refuses nothing; that the root is on the
/// growing branch, and the only one there, is the enclosure's to prove.
///
/// # Errors
///
/// Those of the supports' evaluation.
pub fn chord_setback(
    k: &Kernel,
    supports: [&Surface; 2],
    edge: &EdgeJet,
    edge_uvs: [Point2; 2],
    tangents: [Vec3; 2],
    chord: f64,
) -> Result<Option<ChordFeet>> {
    if !(chord.is_finite() && chord > 0.0) {
        return Ok(None);
    }
    let [t1, t2] = tangents;
    let half = 0.5 * t1.dot(t2).clamp(-1.0, 1.0).acos();
    let mut d = chord / (2.0 * half.sin());
    let mut starts = [Point2::ZERO; 2];
    for ((start, (support, uv)), t) in starts
        .iter_mut()
        .zip(supports.iter().zip(edge_uvs))
        .zip(tangents)
    {
        let Some(s) = setback_start(k, support, uv, t, d)? else {
            return Ok(None);
        };
        *start = s;
    }
    for _ in 0..NEWTON_STEPS {
        let Some(feet) = chord_feet(k, supports, edge, d, starts)? else {
            return Ok(None);
        };
        let [f1, f2] = feet;
        let gap = f1.point - f2.point;
        let g = gap.dot(gap) - chord * chord;
        let slope = 2.0 * gap.dot(f1.along - f2.along);
        if !(slope.is_finite() && slope > 0.0) {
            return Ok(None);
        }
        let step = g / slope;
        let next = d - step;
        if !(next.is_finite() && next > 0.0) {
            return Ok(None);
        }
        d = next;
        starts = [f1.uv, f2.uv];
        if step.abs() <= STATION_STEP * (1.0 + d) {
            let Some(feet) = chord_feet(k, supports, edge, d, starts)? else {
                return Ok(None);
            };
            let [f1, f2] = feet;
            let gap = f1.point - f2.point;
            return Ok((gap.dot(f1.along - f2.along) > 0.0).then_some(ChordFeet { d, feet }));
        }
    }
    Ok(None)
}

/// Both feet at setback `d`, each from its start.
fn chord_feet(
    k: &Kernel,
    supports: [&Surface; 2],
    edge: &EdgeJet,
    d: f64,
    starts: [Point2; 2],
) -> Result<Option<[SetbackFoot; 2]>> {
    let [s1, s2] = supports;
    let Some(f1) = setback_foot(k, s1, edge, d, starts[0])? else {
        return Ok(None);
    };
    let Some(f2) = setback_foot(k, s2, edge, d, starts[1])? else {
        return Ok(None);
    };
    Ok(Some([f1, f2]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    //! The station against the closed forms on circle sections: a foot at straight
    //! distance d along a section of curvature k, from the edge along t with w the normal
    //! toward the other support, is `C + d (cos β t + sin β w)`, `β = asin(d k / 2)`.
    use super::*;
    use crate::blend::fillet_section::station::as_b_spline;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec2;
    use crate::surface::plane::Plane;
    use crate::surface::sphere::{Sphere, SphereOptions};

    const R: f64 = 5.0;

    /// The flat face z = 0 of the hemisphere.
    fn flat() -> Surface {
        Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-20.0, -20.0, 20.0, 20.0),
        ))
    }

    /// The ball of radius 5 about the origin.
    fn ball(k: &Kernel) -> Surface {
        let mut sp = Sphere::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(-30.0, -80.0, 120.0, 80.0),
            R,
            SphereOptions::default(),
        )
        .unwrap();
        sp.make_nurb(k).unwrap();
        Surface::from(sp)
    }

    /// The (u, v) of `p` on `s` (a test's inversion: a grid, then Gauss-Newton).
    fn uv_of(k: &Kernel, s: &Surface, p: Point3) -> Point2 {
        let dom = as_b_spline(k, s).unwrap().unwrap().natural_uv_domain();
        let (lo, hi) = (dom.min, dom.max);
        let mut best = (f64::INFINITY, lo);
        for i in 0..=40 {
            for j in 0..=40 {
                let uv = Point2::new(
                    lo.x + (hi.x - lo.x) * f64::from(i) / 40.0,
                    lo.y + (hi.y - lo.y) * f64::from(j) / 40.0,
                );
                let (q, _, _) = support_jet(k, s, uv).unwrap();
                let e = (q - p).length();
                if e < best.0 {
                    best = (e, uv);
                }
            }
        }
        let mut uv = best.1;
        for _ in 0..60 {
            let (q, su, sv) = support_jet(k, s, uv).unwrap();
            let r = p - q;
            let gram = [[su.dot(su), su.dot(sv)], [su.dot(sv), sv.dot(sv)]];
            let [du, dv] = times(&inverse_of_columns2(&gram).unwrap(), [su.dot(r), sv.dot(r)]);
            uv = Point2::new(uv.x + du, uv.y + dv);
        }
        let (q, _, _) = support_jet(k, s, uv).unwrap();
        assert!((q - p).length() < 1e-12, "{p:?} not on the support");
        uv
    }

    /// The rim `C(s) = R (cos s, sin s, 0)`.
    fn rim(s: f64) -> EdgeJet {
        let (sn, cs) = s.sin_cos();
        EdgeJet {
            point: Point3::new(R * cs, R * sn, 0.0),
            d1: Vec3::new(-R * sn, R * cs, 0.0),
            d2: Vec3::new(-R * cs, -R * sn, 0.0),
        }
    }

    /// The foot on a circle (or line, k = 0) section, in closed form.
    fn section_foot(c: Point3, t: Vec3, w: Vec3, k: f64, d: f64) -> Point3 {
        let (sb, cb) = (0.5 * d * k).asin().sin_cos();
        c + (t * cb + w * sb) * d
    }

    fn close(a: Point3, b: Point3, tol: f64) -> bool {
        (a - b).length() <= tol
    }

    /// The hemisphere of radius 5 at 2 on the flat and 1 on the ball: the flat foot at radius
    /// 3, the ball's at radius 4.9 and height 3 sqrt(11) / 10, at every station.
    #[test]
    fn a_hemisphere_rim_s_feet_are_its_closed_forms() {
        let k = Kernel::default();
        let (fl, bl) = (flat(), ball(&k));
        for s in [0.0_f64, 0.3, 0.75, 1.2] {
            let e = rim(s);
            let out = Vec3::new(s.cos(), s.sin(), 0.0);
            let flat_foot = out * 3.0;
            let ball_foot = out * 4.9 + Vec3::Z * (3.0 * 11.0_f64.sqrt() / 10.0);
            for (support, into, d, want) in
                [(&fl, -out, 2.0, flat_foot), (&bl, Vec3::Z, 1.0, ball_foot)]
            {
                let uv = uv_of(&k, support, e.point);
                let t = setback_tangent(&k, support, uv, &e, into).unwrap().unwrap();
                let start = setback_start(&k, support, uv, t, d).unwrap().unwrap();
                let foot = setback_foot(&k, support, &e, d, start).unwrap().unwrap();
                assert!(close(t, into, 1e-12), "{t:?}");
                assert!(close(foot.point, want, 1e-12), "s {s}: {foot:?} {want:?}");
                // the section's closed form gives the same foot
                let k_sec = if d == 1.0 { 1.0 / R } else { 0.0 };
                let section = section_foot(e.point, into, -out, k_sec, d);
                assert!(close(foot.point, section, 1e-12), "{section:?}");
                // the foot's motion in d: the section's tangent at the foot, scaled so that
                // the distance from the edge grows at rate 1
                let g = foot.point - e.point;
                assert!((g.dot(foot.along) / d - 1.0).abs() < 1e-10, "{foot:?}");
                assert!(foot.along.dot(e.d1).abs() < 1e-10, "{foot:?}");
            }
        }
    }

    /// A chord on the hemisphere rim, a right angle with one circle of curvature `k = 1/5`:
    /// equal setbacks d put the feet `c = sqrt(2 d² − k d³)` apart.
    #[test]
    fn a_chord_s_setback_on_the_hemisphere_rim() {
        let k = Kernel::default();
        let (fl, bl) = (flat(), ball(&k));
        for s in [0.0_f64, 0.9] {
            let e = rim(s);
            let out = Vec3::new(s.cos(), s.sin(), 0.0);
            let uvs = [uv_of(&k, &fl, e.point), uv_of(&k, &bl, e.point)];
            let t1 = setback_tangent(&k, &fl, uvs[0], &e, -out).unwrap().unwrap();
            let t2 = setback_tangent(&k, &bl, uvs[1], &e, Vec3::Z)
                .unwrap()
                .unwrap();
            for d in [0.3_f64, 1.0, 1.5, 3.0] {
                let c = (2.0 * d * d - d * d * d / R).sqrt();
                let got = chord_setback(&k, [&fl, &bl], &e, uvs, [t1, t2], c)
                    .unwrap()
                    .unwrap();
                assert!(
                    (got.d - d).abs() <= 1e-12 * (1.0 + d),
                    "s {s} d {d}: {got:?}"
                );
                let [f1, f2] = got.feet;
                assert!(close(f1.point, e.point - out * d, 1e-11), "{f1:?}");
                let want = section_foot(e.point, Vec3::Z, -out, 1.0 / R, d);
                assert!(close(f2.point, want, 1e-11), "{f2:?}");
            }
            // past the largest chord the rim's sections give (c² = 2d² − d³/5 peaks at
            // d = 20/3, c² = 800/27 < 36): no setback on the growing branch
            assert!(chord_setback(&k, [&fl, &bl], &e, uvs, [t1, t2], 6.0)
                .unwrap()
                .is_none());
            assert!(chord_setback(&k, [&fl, &bl], &e, uvs, [t1, t2], f64::NAN)
                .unwrap()
                .is_none());
        }
    }

    /// Between two planes the setbacks are straight lines on them: today's feet.
    #[test]
    fn between_planes_the_feet_are_the_straight_setbacks() {
        let k = Kernel::default();
        // a 60 degree wedge along the x axis: z = 0 (into +y) and the plane through x at 60
        let a = std::f64::consts::FRAC_PI_3;
        let dir = Vec3::new(0.0, a.cos(), a.sin());
        let p1 = flat();
        let p2 = Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            dir,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-20.0, -20.0, 20.0, 20.0),
        ));
        let e = EdgeJet {
            point: Point3::new(1.5, 0.0, 0.0),
            d1: Vec3::new(2.0, 0.0, 0.0),
            d2: Vec3::ZERO,
        };
        for (support, into, d) in [(&p1, Vec3::Y, 1.5), (&p2, dir, 2.0)] {
            let uv = uv_of(&k, support, e.point);
            let t = setback_tangent(&k, support, uv, &e, into).unwrap().unwrap();
            let start = setback_start(&k, support, uv, t, d).unwrap().unwrap();
            let foot = setback_foot(&k, support, &e, d, start).unwrap().unwrap();
            assert!(close(foot.point, e.point + into * d, 1e-13), "{foot:?}");
        }
        // a chord of 1 on the wedge: equal setbacks of 1 / (2 sin 30) = 1
        let uvs = [uv_of(&k, &p1, e.point), uv_of(&k, &p2, e.point)];
        let ts = [Vec3::Y, dir];
        let got = chord_setback(&k, [&p1, &p2], &e, uvs, ts, 1.0)
            .unwrap()
            .unwrap();
        assert!((got.d - 1.0).abs() < 1e-14, "{got:?}");
    }

    /// Hostile setbacks and a tangent edge give no foot, never a panic.
    #[test]
    fn hostile_inputs_give_no_foot() {
        let k = Kernel::default();
        let fl = flat();
        let e = rim(0.0);
        let uv = uv_of(&k, &fl, e.point);
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(setback_foot(&k, &fl, &e, d, uv).unwrap().is_none());
            assert!(setback_start(&k, &fl, uv, -Vec3::X, d).unwrap().is_none());
        }
        // an edge normal to the support has no tangent into it
        let normal = EdgeJet { d1: Vec3::Z, ..e };
        assert!(setback_tangent(&k, &fl, uv, &normal, -Vec3::X)
            .unwrap()
            .is_none());
        // past the ball's diameter there is no foot at all
        let bl = ball(&k);
        let ub = uv_of(&k, &bl, e.point);
        let t = setback_tangent(&k, &bl, ub, &e, Vec3::Z).unwrap().unwrap();
        let start = setback_start(&k, &bl, ub, t, 11.0).unwrap().unwrap();
        assert!(setback_foot(&k, &bl, &e, 11.0, start).unwrap().is_none());
    }
}
