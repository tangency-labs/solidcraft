//! How far a Join's shared section stands off the second blend, from the corner's geometry
//! at its vertex: the side of the second blend's end the section lies on decides whether
//! that gap is of first or of second order in the corner's angle (#13911).
//!
//! Upstream: none (solidcore-native; the fork's Join takes no bound, #16627 added the
//! first-order one, [`super::join_within_xsect_tolerance_deg`], in #16681).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`JoinBlend`]            | none     |
//! | [`EdgeBend`]             | none     |
//! | [`join_tolerance_deg`]   | none     |
//! | [`overlap_gap`]          | none     |
//! | [`rail_curvature`]       | none     |
//! | [`sagitta`]              | none     |
//!
//! A Join ends both blends on one section, the first blend's, in the plane `S` through the
//! corner's vertex `v` square to its edge (`FilletBrep::calc_join_section`). On the face the
//! two blends share, that section's foot is `F = v + R1 b1`: `b1` the first edge's binormal
//! into the face, `R1` the first blend's reach along the face (on the upright walls this
//! module takes, the blend's offset off the wall). Let `t2` be the second
//! edge's direction leaving `v`, and `beta` the angle the two edges make in the face
//! (`cos beta = -t1 . t2` for `t1` the first edge's direction leaving `v`).
//!
//! - **Behind** (`b1 . t2 < 0`): `F` lies `R1 sin beta` before the second blend's own end,
//!   where its surface does not reach, so the vertex stands that far off it: first order,
//!   #16627's bound (`2 size tan(a / 2)`, MEASURED there as `o a` on the concave corners
//!   of its plates, where the section lies behind).
//! - **Over** (`b1 . t2 >= 0`): `F` lies over the second blend, whose rail (its surface's
//!   foot on the shared face) crosses the line `S` cuts from the face at `v + lambda b1`.
//!   The gap is `|R1 - lambda|`, bounded by [`overlap_gap`]: with equal reaches and straight
//!   edges `R (sec beta - 1)`, second order (at the 3.4e-3 degrees of #13911, `1.8e-9 R`).
//!
//! **The bound** (PROVEN under the model, which the code verifies). Work in the shared face, a plane, with
//! `v` the origin, `x` along `t2` and `y` along `b2` (the second edge's binormal into the
//! face). The face is a plane and `b_i` is `t_i` turned a quarter turn in it toward the face,
//! so `b1` and `b2` make the angle `beta` the edges make, and on the over side
//! `b1 = (sin beta, cos beta)`: `S` cuts the face in the points `lambda (sin beta, cos beta)`.
//! The second rail leaves `(0, R2)` along `x`. Let `K2` bound the second edge's curvature
//! along the arc the rail is taken over; the rail, its offset by `R2` in the plane, has
//! curvature at most `kr = K2 / (1 - R2 K2)` there (an offset's curvature is `k / (1 - R2 k)`
//! for the edge's signed curvature `k`, `|k| <= K2`, `R2 K2 < 1`). A unit-speed plane curve
//! of curvature at most `kr` turns by at most `kr s` over arc `s`, so while `kr s <= pi / 2`
//! its `x` grows at least as `sin(kr s) / kr` and its `y` moves at most `(1 - cos(kr s)) /
//! kr`: at abscissa `x` (with `kr x <= 1`) the rail stands within `sagitta(kr, x) = (1 -
//! sqrt(1 - kr^2 x^2)) / kr = kr x^2 / (1 + sqrt(1 - kr^2 x^2))` of `y = R2`, and has run at
//! most `asin(kr x) / kr`, which for `kr x <= 1/2` is at most `1.05 x`. Take
//! `X = 2 R tan beta` (`R` the larger reach) with `tan beta <= 1/2` and `kr X <= 1/2`: the
//! sagitta is then at most `X <= R`, so over `0 <= x <= X` the rail lies in the band
//! `|y - R2| <= w`, `w = sagitta(kr, X) + e` (`e` the rail's shift off the edge's offset,
//! below; refused unless `w <= R`). At `x = 0` the rail (`y = R2 > 0`) is above the line
//! (`y = 0`), and at `x = X` it is below it (the line stands at `X / tan beta = 2R >= R2 +
//! w`). It therefore crosses the line at some `x <= X`, where `lambda cos beta = R2 + d`,
//! `|d| <= w`: `|lambda - R2 /
//! cos beta| <= sec beta w`, and `|R1 - lambda| <= |R1 - R2 / cos beta| + sec beta w` (the
//! shifts `e` are carried as the reaches' bounds). The order of the two blends is not known here, so both are taken:
//! [`overlap_gap`] is the larger of the two `|R_i - R_j / cos beta|` plus `sec beta` times the
//! larger sagitta. The rail runs at most `1.05 X` along its arc and its edge at most `1.05 X
//! / (1 - R K)` (the rail's speed is `1 - R k` times the edge's), so the curvature bound is
//! needed only over that much of the edge next to `v`: [`EdgeBend`] carries a bound and the
//! certified length of the edge it holds over, and a bound over less is refused.
//!
//! **The model** (verified). The corner's ring is three edges: the two blended ones and
//! one side edge, the two walls' common edge (a four-edge ring, or a side edge between
//! other faces, falls back: the bound below covers no other side edge). The shared face is
//! a plane of unit normal `n`, and each blend's other face is a wall ruled along `n`
//! ([`ruling_tilt`]), tilted from it by at most `delta` (`sin delta` certified, at most
//! [`MAX_TILT`]), both as it evaluates and as its B-spline net:
//! - its net (a plane's, a cylinder's, an extrusion's or a B-spline's own) is of degree 1
//!   across with two control points, each rung's points of one weight and every rung proved
//!   to point the same way along `n`. For a fixed parameter along, such a net is `A + t B`
//!   with `B` a positive combination of the rungs, so every ruling lies within `delta` of
//!   `n`;
//! - and the kinds that evaluate points from their analytic form are certified on it too: a
//!   plane's normal within `sin delta` of square to `n`, a cone only of the cylinder class
//!   (semi-angle 0) with its axis within `delta` of `n`, an extrusion's vector within
//!   `delta` of `n`.
//!
//! An upright wall (`delta = 0`) is the cylinder over the edge along `n`, and a blend between it and the
//! plane is the same in every plane square to the edge: its section's foot on the plane
//! stands its offset `oB` off the wall from the edge, and on the wall its offset `oA` off
//! the plane down the ruling. So the rail is the edge's offset in the plane by the
//! constant `R = oB`. A tilted wall stands within `h tan(delta)` of the upright one at
//! depth `h`; the distance to a surface is 1-Lipschitz in the surface (Hausdorff), and the
//! distance to the upright wall grows at unit rate along `b`, so each section's foot moves
//! by at most that much. The depth is bounded (PROVEN, any `delta` below a quarter turn):
//! the section's centre (a round's ball centre, the corner of a chamfer's two offset
//! planes) stands `oA` below the plane; its foot on the plane is at depth 0, and its foot
//! on the wall is the centre moved `oB` along the wall's normal, which is within `delta` of
//! square to `n`, so at depth `oA + oB sin delta <= oA + oB`; a round's arc between its feet
//! stays within its ball, at depth at most `oA + oB`. So `h <= oA + oB`, and the shift is
//! taken as `e = 2 (oA + oB) tan(delta)` (twice that), with `tan delta <= sin delta / (1 -
//! sin delta)`. On #13911 `delta` is about `2.4e-18`.
//!
//! **The second joint vertex** (PROVEN under the model). The Join's section ends on the
//! first wall too. With both walls upright through `v`, the line `v + t n` lies in both:
//! the first section's plane contains `n` (it is square to `t1`, which lies in the plane),
//! so its foot on the first wall is `v - oA1 n`, on that line. The second blend's rail on
//! its wall is the second edge moved `oA2` down `n` and passes through `v - oA2 n`, on the
//! same line. So the vertex there lies on the second wall and within `|oA1 - oA2|` of the
//! second blend. For tilted walls each lies within its shift `e` of its upright wall over
//! that depth (above), so the vertex moves by at most `e1` off the common line and the second
//! blend's foot by at most `e2`, and each wall's shift is added twice. [`MAX_TILT`] is a
//! conservative refusal cap, not a limit of this argument: walls tilted past it (drafted
//! walls) fall back to #16627's bound.
//!
//! That the check's vertex gap is the larger of these two distances is ARGUED as the
//! over/behind split above. MEASURED on `kinked_plate` (wr697): `sec(kink) - 1` to `1e-4`.
//!
//! **Curvature bounds** ([`EdgeBend`], PROVEN): a line 0; an ellipse of radii `a >= b`
//! `a / b^2`, its largest; a polynomial B-spline, over the knot span at `v`, `max |C''| /
//! min |C'|^2` (the curvature is `|C' x C''| / |C'|^3`), with `max |C''|` at most the
//! largest of the span's second-hodograph control points and `|C'| >= C' . u >= min (Q_i .
//! u)` for `u` the unit tangent at `v` and `Q_i` the span's first-hodograph control points
//! (the B-spline basis is non-negative and sums to 1); a B-spline stored with every weight
//! exactly 1 is that polynomial. Any other rational B-spline or other kind is refused (no
//! bound is derived for it).
//!
//! **Rounding** (PROVEN for this module's arithmetic, [`crate::geom::certified`]): every
//! unit vector carries `unit_error` (a rung's its difference's `gamma_1`), the cosine and
//! sine of `beta` and of each rung's tilt their [`crate::geom::certified::dot`] and
//! `cross_length` bounds (the reach is the solver's offset, given), the reaches' quotients by
//! `cos beta` their error through `1 / (cos beta - e_cos)`, and the bend term is evaluated
//! from upper bounds of every input, each product, quotient and root rounded up by its
//! `gamma`. The gap is compared with the budget by [`crate::geom::certified::Bounded::at_most`],
//! so a Join is taken only where the exact gap of the evaluated jets is proved below it.
//! The jets themselves (the curves' derivatives and the faces' normals as evaluated) are
//! taken as given; their evaluation error is not bounded here, as in #16627's bound. The
//! budget keeps #16627's margin for the check's own rounding.
//!
//! Deviations: none (no upstream counterpart).

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignored, Result};
use crate::geom::certified::{
    above, cross_length, dot, gamma, unit, unit_error, unit_error_of, Bounded,
};
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::{Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::face::p2 as face2;
use crate::topology::{edge, Brep, EdgeuseId, FaceId, VertexId};

/// One blend of a corner as [`join_tolerance_deg`] reads it: its edge use and, for each of
/// its two faces, the face and the blend's offset off it (the solver's offset surfaces).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JoinBlend {
    /// The blended edge use.
    pub eu: EdgeuseId,
    /// The blend's two faces with its offset off each (an offset's absolute distance).
    pub faces: [(Option<FaceId>, f64); 2],
}

/// A bound on an edge's curvature next to the corner's vertex, and a lower bound on the
/// length of edge from the vertex over which it holds (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeBend {
    /// At least the edge's curvature over [`Self::arc`] from the vertex.
    pub curvature: f64,
    /// At most the length of edge, from the vertex, the bound holds over.
    pub arc: f64,
}

/// The blend's geometry at the corner's vertex on the face both blends share.
#[derive(Clone, Copy, Debug)]
struct AtVertex {
    /// The edge's unit direction leaving the vertex.
    out: Vec3,
    /// The edge's unit binormal into the shared face there.
    binormal: Vec3,
    /// The blend's reach along the shared face, its offset off the wall, with how far the
    /// rail may stand off the edge's offset by it (the wall's tilt).
    reach: Bounded,
    /// The blend's offset off the shared face: its section's foot on the wall that far
    /// down the wall's ruling through the vertex.
    down: f64,
    /// The blend's other face, the wall.
    wall: FaceId,
    /// The edge's curvature bound next to the vertex.
    bend: EdgeBend,
}

/// `x` rounded up past `n` roundings, for a non-negative `x`.
fn up(x: f64, n: usize) -> f64 {
    x * (1.0 + gamma(n))
}

/// At least the curvature of a rail offset `reach` in the plane from an edge whose
/// curvature is at most `curvature`: `curvature / (1 - reach curvature)`, rounded up. `None`
/// where `reach curvature >= 1` (the offset may fold: no bound), or for a non-finite or
/// negative input.
#[must_use]
pub fn rail_curvature(curvature: f64, reach: f64) -> Option<f64> {
    if !(curvature.is_finite() && reach.is_finite() && curvature >= 0.0 && reach >= 0.0) {
        return None;
    }
    let rk = up(reach * curvature, 1);
    if !above(1.0, rk) {
        return None;
    }
    // 1 - rk rounds once, down by at most gamma_1 of it
    let lower = (1.0 - rk) * (1.0 - gamma(2));
    Some(up(curvature / lower, 1))
}

/// At least how far a plane curve of curvature at most `curvature` stands off its tangent
/// at abscissa `x` along it: `k x^2 / (1 + sqrt(1 - k^2 x^2))` (module docs), rounded up.
/// `None` past `k x = 1/2` or for a non-finite or negative input.
#[must_use]
pub fn sagitta(curvature: f64, x: f64) -> Option<f64> {
    if !(curvature.is_finite() && x.is_finite() && curvature >= 0.0 && x >= 0.0) {
        return None;
    }
    let kx = up(curvature * x, 1);
    if kx.is_nan() || kx > 0.5 {
        return None;
    }
    let num = up(kx * x, 1);
    // the square rounds up, so 1 - it and its root are below their exact values but for
    // their own rounding: gamma_4 down in all, and the sum once more
    let den = (1.0 + (1.0 - up(kx * kx, 1)).sqrt()) * (1.0 - gamma(5));
    Some(up(num / den, 1))
}

/// The bound on the gap where the shared section lies over the second blend (module docs):
/// the larger `|R_i - R_j / cos beta|` plus `sec beta` times the larger [`sagitta`] of the
/// rails over `X = 2 R tan beta`, each rail's curvature from its edge's [`EdgeBend`] by
/// [`rail_curvature`], certified for its own rounding. Each reach's bound is how far the
/// rail may stand off the edge's offset by it (a wall's shift, module docs), carried into
/// the gap. `None` where `cos beta` is not proved positive, `tan beta > 1/2`, a reach is
/// not proved positive, a rail's curvature or sagitta is refused, the band (sagitta and shift) exceeds
/// the larger reach, or an edge's bound holds over less than the `1.05 X / (1 - R K)` of
/// edge the rail runs over.
#[must_use]
pub fn overlap_gap(
    reaches: [Bounded; 2],
    cos_beta: Bounded,
    sin_beta: Bounded,
    bends: [EdgeBend; 2],
) -> Option<Bounded> {
    let given = [reaches[0], reaches[1], cos_beta, sin_beta];
    if !given
        .iter()
        .all(|x| x.value.is_finite() && x.bound.is_finite() && x.bound >= 0.0)
    {
        return None;
    }
    let [r1, r2] = reaches;
    // the crossing argument needs each reach proved positive (`y = R2 > 0` at `x = 0`)
    if !(r1.value - r1.bound > 0.0 && r2.value - r2.bound > 0.0 && sin_beta.value >= 0.0) {
        return None;
    }
    let cos_lo = cos_beta.value - cos_beta.bound;
    if !above(cos_lo, 0.0) {
        return None;
    }
    // r / cos beta, within (e_r + q e_cos) / (cos beta - e_cos) and its rounding
    let quotient = |r: Bounded| {
        let q = r.value / cos_beta.value;
        (
            q,
            up((r.bound + q * cos_beta.bound) / cos_lo, 3) + gamma(1) * q,
        )
    };
    let (q1, e1) = quotient(r1);
    let (q2, e2) = quotient(r2);
    let a12 = (r1.value - q2).abs();
    let a21 = (r2.value - q1).abs();
    let apart = a12.max(a21);
    let apart_bound =
        up(r1.bound + e2 + gamma(1) * a12, 2).max(up(r2.bound + e1 + gamma(1) * a21, 2));
    // the bend, from upper bounds of every input
    let reach_up = up((r1.value + r1.bound).max(r2.value + r2.bound), 1);
    let tan_up = up((sin_beta.value + sin_beta.bound) / cos_lo, 2);
    if tan_up.is_nan() || tan_up > 0.5 {
        return None;
    }
    let x = up(2.0 * reach_up * tan_up, 2);
    let mut sag = 0.0_f64;
    for (bend, r) in bends.iter().zip([r1, r2]) {
        let r_up = up(r.value + r.bound, 1);
        let kr = rail_curvature(bend.curvature, r_up)?;
        sag = sag.max(sagitta(kr, x)?);
        // the edge the rail runs over: 1.05 X / (1 - R K)
        let shrink = (1.0 - up(r_up * bend.curvature, 1)) * (1.0 - gamma(2));
        if !(shrink > 0.0 && bend.arc >= up(1.05 * x / shrink, 3)) {
            return None;
        }
    }
    // the band the rail lies in, sagitta and shift, within the larger reach (module docs)
    let band = up(sag + r1.bound.max(r2.bound), 1);
    if band.is_nan() || band > reach_up {
        return None;
    }
    let bend = up(up(1.0 / cos_lo, 1) * sag, 1);
    let value = apart + bend;
    let bound = apart_bound * (1.0 + gamma(1)) + gamma(1) * value;
    value.is_finite().then_some(Bounded { value, bound })
}

/// The polynomial B-spline `c`'s [`EdgeBend`] curvature over its knot span at `param` (on
/// the side toward the edge's interior), with the span's far end: module docs. `None` for a
/// rational B-spline (other than one whose weights are all exactly 1), a degree-0 one, a span it cannot find, or a tangent along which a
/// hodograph point does not lead.
fn spline_curvature(
    c: &BSplineCurve,
    param: f64,
    at_start: bool,
    tangent: Vec3,
) -> Result<Option<(f64, f64)>> {
    let p = c.degree()?;
    let Ok(p32) = u32::try_from(p) else {
        return Ok(None);
    };
    if p == 0 {
        return Ok(None);
    }
    let cpts = c.control_points()?;
    // a weight of exactly 1 everywhere is a polynomial stored homogeneously
    if cpts.iter().any(|q| q.w != NOW && q.w != 1.0) {
        return Ok(None);
    }
    let (unique, mult) = c.knots(None)?;
    let mut knots = Vec::new();
    for (t, m) in unique.iter().zip(&mult) {
        for _ in 0..*m {
            knots.push(*t);
        }
    }
    let Some(last) = cpts.len().checked_sub(1) else {
        return Ok(None);
    };
    // the span [t_j, t_j+1] next to the vertex, toward the edge
    let j = if at_start {
        knots.iter().rposition(|&t| t <= param).map(|j| j.min(last))
    } else {
        knots.iter().rposition(|&t| t < param)
    };
    let Some(j) = j else { return Ok(None) };
    let (Some(&lo), Some(&hi)) = (knots.get(j), knots.get(j + 1)) else {
        return Ok(None);
    };
    let inside = if at_start {
        lo <= param && param < hi
    } else {
        lo < param && param <= hi
    };
    if !(inside && j >= p && j <= last) {
        return Ok(None);
    }
    let point = |i: usize| {
        cpts.get(i)
            .map(|c| Vec3::new(c.x, c.y, if c.z == NOZ { 0.0 } else { c.z }))
    };
    let knot = |i: usize| knots.get(i).copied();
    let pf = f64::from(p32);
    // the first hodograph's points Q_i = p (P_i+1 - P_i) / (t_i+p+1 - t_i+1), i = j-p..j-1:
    // the difference of exact doubles rounds once relative to itself, the knot difference
    // once, the scale and quotient twice: gamma_4 of |Q| per component
    let mut hodo = Vec::new();
    for i in (j - p)..j {
        let (Some(a), Some(b), Some(t0), Some(t1)) =
            (point(i), point(i + 1), knot(i + 1), knot(i + p + 1))
        else {
            return Ok(None);
        };
        let dt = t1 - t0;
        if !above(dt, 0.0) {
            return Ok(None);
        }
        let q = (b - a) * (pf / dt);
        hodo.push((q, gamma(4) * 2.0 * q.length()));
    }
    // |C'| >= C' . u / |u| >= min (Q_i . u) / |u|, each dot within its rounding and Q's error
    let u_len = 1.0 + 2.0 * unit_error();
    let mut lead = f64::INFINITY;
    for &(q, e) in &hodo {
        let along = q.dot(tangent) - (e * u_len + gamma(3) * q.length() * u_len);
        lead = lead.min(along);
    }
    let lead = lead * (1.0 - gamma(2)) / u_len;
    if !above(lead, 0.0) {
        return Ok(None);
    }
    // the second hodograph's points (p - 1) (Q_i+1 - Q_i) / (t_i+p+1 - t_i+2), i = j-p..j-2
    let mut most = 0.0_f64;
    for (n, w) in hodo.windows(2).enumerate() {
        let i = j - p + n;
        let ([(qa, ea), (qb, eb)], Some(t0), Some(t1)) = (w, knot(i + 2), knot(i + p + 1)) else {
            return Ok(None);
        };
        let dt = t1 - t0;
        if !above(dt, 0.0) {
            return Ok(None);
        }
        let scale = (pf - 1.0) / dt;
        let r = (*qb - *qa) * scale;
        let e = up(
            scale * (ea + eb + gamma(1) * (qa.length() + qb.length()) * 2.0),
            3,
        ) + gamma(4) * 2.0 * r.length();
        most = most.max(up(r.length(), 5) + e);
    }
    let far = if at_start { hi } else { lo };
    Ok(Some((up(most / up(lead * lead, 1), 1), far)))
}

/// The edge `curve`'s [`EdgeBend`] next to its end at `param` (`at_start`: its interval's
/// start) within `ivl`, or `None` for a kind no bound is derived for (module docs).
fn edge_bend(
    k: &Kernel,
    curve: &Curve,
    ivl: &Extent1d,
    param: f64,
    at_start: bool,
    tangent: Vec3,
) -> Result<Option<EdgeBend>> {
    let other = if at_start { ivl.max() } else { ivl.min() };
    let (curvature, far) = match curve.kind() {
        CurveKind::Line(_) => (0.0, other),
        CurveKind::Ellipse(el) => {
            let (a, b) = (el.x_radius().abs(), el.y_radius().abs());
            let (big, small) = (a.max(b), a.min(b));
            if !above(small, EFF_ZERO) {
                return Ok(None);
            }
            (up(big / up(small * small, 1), 1), other)
        }
        CurveKind::BSpline(c) => {
            let Some((curvature, end)) = spline_curvature(c, param, at_start, tangent)? else {
                return Ok(None);
            };
            let far = if at_start {
                end.min(other)
            } else {
                end.max(other)
            };
            (curvature, far)
        }
        _ => return Ok(None),
    };
    if !curvature.is_finite() {
        return Ok(None);
    }
    let span = Extent1d::from_min_max(param.min(far), param.max(far));
    let Some(length) = ignored(curve.length_bound(k, &span, f64::MAX))? else {
        return Ok(None);
    };
    Ok(Some(EdgeBend {
        curvature,
        arc: length.lower,
    }))
}

/// The largest tilt a wall's rulings may make with `normal`, the sine of the angle rounded
/// up, where `surface` is ruled along it both as it evaluates and as its B-spline net (a
/// plane's, a cylinder's, an extrusion's or a B-spline's own): the net of degree 1 with two
/// control points across, each rung's two points of one weight, every rung proved to point
/// the same way along `normal`; a plane's normal, a cylinder's axis or an extrusion's vector
/// certified too (module docs). `None` otherwise, for a cone that is not of the cylinder
/// class, or past [`MAX_TILT`].
fn ruling_tilt(surface: &Surface, normal: Vec3) -> Result<Option<f64>> {
    let ue = unit_error();
    // the analytic form's tilt, for the kinds that evaluate points from it
    let along = |v: Vec3| unit(v).map(|u| cross_length(u, normal, ue, ue));
    let (net, analytic) = match surface.kind() {
        SurfaceKind::Plane(p) => {
            let Some(m) = unit(p.is_planar().1) else {
                return Ok(None);
            };
            // the plane holds `normal` within the sine of `|m . normal|`
            let d = dot(m, normal, ue, ue);
            (p.base(), Some(d.value.abs() + d.bound))
        }
        SurfaceKind::Cone(c) => {
            if !c.is_cylinder_class() || c.semi_angle_deg != 0.0 {
                return Ok(None);
            }
            let Some(sine) = along(c.rev.position().z_axis()) else {
                return Ok(None);
            };
            (c.rev.base(), Some(sine.value + sine.bound))
        }
        SurfaceKind::SurfOfExtrusion(e) => {
            let Some(sine) = along(e.extrusion_vector()) else {
                return Ok(None);
            };
            (e.base(), Some(sine.value + sine.bound))
        }
        SurfaceKind::BSpline(s) => (s, None),
        _ => return Ok(None),
    };
    let Some(net_tilt) = net_tilt(net, normal)? else {
        return Ok(None);
    };
    let tilt = up(net_tilt.max(analytic.unwrap_or(0.0)), 1);
    Ok((tilt <= MAX_TILT).then_some(tilt))
}

/// The tilt (a sine, rounded up) of a B-spline net's rungs from `normal` ([`ruling_tilt`]).
/// The net is read u-major, row by row, as `BSplineSurface::control_points` returns it.
fn net_tilt(net: &BSplineSurface, normal: Vec3) -> Result<Option<f64>> {
    let (nu, nv, points) = net.control_points()?;
    // a net linear both ways (a plane's) is tried both ways: the rungs across v, then
    // across u
    if nv == 2 && net.degree(SurfParamType::V) == 1 && points.len() == 2 * nu {
        let rungs = points.chunks(2).filter_map(|c| match c {
            [a, b] => Some((*a, *b)),
            _ => None,
        });
        if let Some(tilt) = rungs_tilt(rungs, points.len() / 2, normal) {
            return Ok(Some(tilt));
        }
    }
    if nu == 2 && net.degree(SurfParamType::U) == 1 && points.len() == 2 * nv {
        let (Some(first), Some(second)) = (points.get(..nv), points.get(nv..)) else {
            return Ok(None);
        };
        let rungs = first.iter().zip(second).map(|(a, b)| (*a, *b));
        return Ok(rungs_tilt(rungs, nv, normal));
    }
    Ok(None)
}

/// The tilt (a sine, rounded up) of `count` rungs from `normal` where every rung's two
/// points share a weight and every rung is proved to point the same way along it, at most
/// [`MAX_TILT`]; `None` otherwise.
fn rungs_tilt(
    rungs: impl Iterator<Item = (CPoint, CPoint)>,
    count: usize,
    normal: Vec3,
) -> Option<f64> {
    let ue = unit_error();
    let mut tilt = 0.0_f64;
    let mut side = 0.0_f64;
    let mut seen = 0usize;
    for (a, b) in rungs {
        seen += 1;
        let weighted = (a.w == NOW && b.w == NOW) || (a.w == b.w && above(a.w, 0.0));
        if !weighted || a.z == NOZ || b.z == NOZ {
            return None;
        }
        // homogeneous points of one weight: the rung's direction; each difference of two
        // doubles rounds once, within gamma_1 of itself
        let d = Vec3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let length = d.length();
        let du = unit(d)?;
        let ed = unit_error_of(gamma(1) * up(length, 5), length);
        let along = dot(du, normal, ed, ue);
        let this = if along.value - along.bound > 0.0 {
            1.0
        } else if along.value + along.bound < 0.0 {
            -1.0
        } else {
            return None;
        };
        if side != 0.0 && this != side {
            return None;
        }
        side = this;
        let sine = cross_length(du, normal, ed, ue);
        tilt = tilt.max(sine.value + sine.bound);
    }
    let tilt = up(tilt, 1);
    (seen > 0 && seen == count && tilt <= MAX_TILT).then_some(tilt)
}

/// The largest ruling tilt (a sine) [`ruling_tilt`] takes: a conservative refusal cap,
/// chosen, not derived (the shift argument holds for any tilt below a quarter turn, module
/// docs). It keeps the over-side Join to walls upright to rounding, as #13911's are (about
/// `2.4e-18`); walls tilted further (drafted walls) fall back to #16627's bound.
pub const MAX_TILT: f64 = 1e-9;

/// The blend's [`AtVertex`] on `shared`, the plane of unit normal `normal`, or `None` where
/// it cannot be read or the model does not hold (no edge use of the blend's edge on that
/// face, a degenerate tangent or binormal, an other face not a wall ruled along `normal`,
/// an edge without a curvature bound: module docs).
fn at_vertex(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    blend: &JoinBlend,
    shared: FaceId,
    normal: Vec3,
) -> Result<Option<AtVertex>> {
    let Some(e) = eu1::edge(b, blend.eu)? else {
        return Ok(None);
    };
    let ivl = edge::interval(b, e)?;
    let at_start = edge::start_vertex(b, e)? == Some(vertex);
    let param = if at_start { ivl.min() } else { ivl.max() };
    let Some(curve) = edge::curve(b, e)?.cloned() else {
        return Ok(None);
    };
    let derivs = curve.evaluate(k, param, 1, !at_start, true)?;
    let Some(&d1) = derivs.get(1) else {
        return Ok(None);
    };
    if !above(d1.length(), EFF_ZERO) {
        return Ok(None);
    }
    let Ok(tangent) = d1.unitize() else {
        return Ok(None);
    };
    let out = if at_start { tangent } else { -tangent };

    // the edge use on the shared face and the one on the blend's other face
    let Some(radial) = eu1::radial(b, blend.eu)? else {
        return Ok(None);
    };
    let (on_shared, on_other) = if eu1::face(b, blend.eu)? == Some(shared) {
        (blend.eu, radial)
    } else if eu1::face(b, radial)? == Some(shared) {
        (radial, blend.eu)
    } else {
        return Ok(None);
    };
    let opts = BinormalOptions::default();
    let s = evaluate_binormal(k, b, on_shared, param, false, &opts)?;
    let Ok(binormal) = s.binormal.unitize() else {
        return Ok(None);
    };
    let other_face = eu1::face(b, on_other)?;
    let offset_on = |face: Option<FaceId>| {
        blend
            .faces
            .iter()
            .find(|(f, _)| f.is_some() && *f == face)
            .map(|&(_, o)| o)
    };
    let (Some(o_shared), Some(o_other), Some(wall)) =
        (offset_on(Some(shared)), offset_on(other_face), other_face)
    else {
        return Ok(None);
    };
    // the other face a wall ruled along the shared plane's normal (module docs)
    let Some(surface) = face2::surface(b, wall)? else {
        return Ok(None);
    };
    let Some(tilt) = ruling_tilt(surface, normal)? else {
        return Ok(None);
    };
    // the wall stands within `h tan(delta)` of the upright one over the edge at depth `h`,
    // taken over `h <= 2 (o_shared + o_other)` (module docs)
    let (o_shared, o_other) = (o_shared.abs(), o_other.abs());
    let shift = up(2.0 * (o_shared + o_other) * tilt / (1.0 - tilt), 4);
    if !(o_other.is_finite() && o_shared.is_finite() && shift.is_finite()) {
        return Ok(None);
    }
    let Some(bend) = edge_bend(k, &curve, &ivl, param, at_start, tangent)? else {
        return Ok(None);
    };
    Ok(Some(AtVertex {
        out,
        binormal,
        reach: Bounded {
            value: o_other,
            bound: shift,
        },
        down: o_shared,
        wall,
        bend,
    }))
}

/// The angle (degrees) within which the faces across the corner's side edges may meet for
/// its two blends to Join: the solvers' tangency tolerance `tangency_tol_deg` where the
/// shared face is a plane, both other faces are walls ruled along its normal, the shared
/// section is proved to lie over the second blend either way round, and both joint
/// vertices' gaps ([`overlap_gap`] on the plane, the walls' offsets apart on their common
/// ruling) are proved below the intersection tolerance `xsect_tol` (less #16627's margin
/// for the check's own rounding), else `behind_deg`,
/// #16627's bound ([`super::join_within_xsect_tolerance_deg`]). `behind_deg` too where the
/// two blends share no single face, the ring `all_eus` is not three edges whose unblended
/// one is the two walls' common edge, or any quantity cannot be read or bounded (module
/// docs).
///
/// # Errors
///
/// Those of the topology queries, the curve evaluation, `evaluate_binormal` and the
/// certified edge length.
pub fn join_tolerance_deg(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    blends: [&JoinBlend; 2],
    all_eus: &[EdgeuseId],
    xsect_tol: f64,
    tangency_tol_deg: f64,
    behind_deg: f64,
) -> Result<f64> {
    if !(xsect_tol.is_finite() && xsect_tol > 0.0) {
        return Ok(behind_deg);
    }
    let [first, second] = blends;
    // the one face both blends roll on
    let shared: Vec<FaceId> = first
        .faces
        .iter()
        .filter_map(|&(f, _)| f)
        .filter(|f| second.faces.iter().any(|&(g, _)| g == Some(*f)))
        .collect();
    let [shared] = shared.as_slice() else {
        return Ok(behind_deg);
    };
    // a plane: its bending is never a term (module docs)
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, *shared)?.map(Surface::kind) else {
        return Ok(behind_deg);
    };
    let Some(normal) = unit(plane.is_planar().1) else {
        return Ok(behind_deg);
    };
    let (Some(one), Some(two)) = (
        at_vertex(k, b, vertex, first, *shared, normal)?,
        at_vertex(k, b, vertex, second, *shared, normal)?,
    ) else {
        return Ok(behind_deg);
    };
    // the ring: three edges, the unblended one the two walls' common edge (module docs)
    if all_eus.len() != 3 || one.wall == two.wall {
        return Ok(behind_deg);
    }
    let blended = [eu1::edge(b, first.eu)?, eu1::edge(b, second.eu)?];
    let mut side = None;
    for &eu in all_eus {
        if blended.contains(&eu1::edge(b, eu)?) {
            continue;
        }
        if side.is_some() {
            return Ok(behind_deg);
        }
        side = Some(eu);
    }
    let Some(side) = side else {
        return Ok(behind_deg);
    };
    let Some(radial) = eu1::radial(b, side)? else {
        return Ok(behind_deg);
    };
    let faces = [eu1::face(b, side)?, eu1::face(b, radial)?];
    let walls = [Some(one.wall), Some(two.wall)];
    if !(faces == walls || faces == [walls[1], walls[0]]) {
        return Ok(behind_deg);
    }
    // over the other blend, either way round, proved (a NaN is not)
    let ue = unit_error();
    let over = |x: Bounded| x.value - x.bound >= 0.0;
    if !(over(dot(one.binormal, two.out, ue, ue)) && over(dot(two.binormal, one.out, ue, ue))) {
        return Ok(behind_deg);
    }
    let along = dot(one.out, two.out, ue, ue);
    let cos_beta = Bounded {
        value: -along.value,
        bound: along.bound,
    };
    let sin_beta = cross_length(one.out, two.out, ue, ue);
    let Some(gap) = overlap_gap(
        [one.reach, two.reach],
        cos_beta,
        sin_beta,
        [one.bend, two.bend],
    ) else {
        return Ok(behind_deg);
    };
    // #16627's margin for the check's own rounding
    let eps = f64::EPSILON;
    let reach = (one.reach.value + one.reach.bound).max(two.reach.value + two.reach.bound);
    let budget = xsect_tol * (1.0 - 16.0 * eps) - 8.0 * eps * reach;
    // the second joint vertex, on the walls' common ruling through the vertex: the two
    // sections' feet there stand `|o1 - o2|` apart, and each wall within its shift of the
    // upright one (module docs)
    let apart = (one.down - two.down).abs();
    let walls = apart + 2.0 * (one.reach.bound + two.reach.bound);
    let wall_gap = Bounded {
        value: walls,
        bound: gamma(4) * walls,
    };
    if gap.at_most(budget).is_yes() && wall_gap.at_most(budget).is_yes() {
        Ok(tangency_tol_deg)
    } else {
        Ok(behind_deg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(value: f64) -> Bounded {
        Bounded { value, bound: 0.0 }
    }

    fn straight(arc: f64) -> EdgeBend {
        EdgeBend {
            curvature: 0.0,
            arc,
        }
    }

    fn close(got: f64, want: f64) -> bool {
        (got - want).abs() <= 1e-14 * want.abs()
    }

    // These check that the code computes the formulas of the module docs, rounded up: the
    // bound itself is proved there.
    #[test]
    fn the_overlap_gap_computes_its_formula() {
        let beta = 5.9e-5_f64;
        let (c, s) = (exact(beta.cos()), exact(beta.sin()));
        // equal reaches, straight edges: R (sec beta - 1)
        let gap = overlap_gap([exact(1.0); 2], c, s, [straight(1.0); 2]).unwrap();
        let want = 1.0 / beta.cos() - 1.0;
        assert!(gap.value >= want && gap.value <= want + 1e-15, "{gap:?}");
        assert!(gap.bound < 1e-14, "{gap:?}");
        // a rail of an edge of curvature 0.625 at reach 1: kr = 0.625 / 0.375, the sagitta
        // over X = 2 tan beta, times sec beta
        let bent = EdgeBend {
            curvature: 0.625,
            arc: 1.0,
        };
        let gap = overlap_gap([exact(1.0); 2], c, s, [bent, straight(1.0)]).unwrap();
        let kr = 0.625 / 0.375;
        let x = 2.0 * beta.tan();
        let sag = kr * x * x / (1.0 + (1.0 - kr * kr * x * x).sqrt());
        let want = 1.0 / beta.cos() - 1.0 + sag / beta.cos();
        assert!(
            gap.value >= want && close(gap.value, want),
            "{gap:?} {want}"
        );
        // unequal reaches stand apart by their difference: first order
        let gap = overlap_gap(
            [exact(1.0), exact(0.9)],
            exact(1.0),
            exact(0.0),
            [straight(1.0); 2],
        )
        .unwrap();
        assert!((gap.value - 0.1).abs() < 1e-15);
        // the inputs' bounds carry through
        let wide = Bounded {
            value: 1.0,
            bound: 1e-12,
        };
        let gap = overlap_gap([wide, exact(1.0)], c, s, [straight(1.0); 2]).unwrap();
        assert!(gap.bound >= 1e-12, "{gap:?}");
    }

    #[test]
    fn the_sagitta_exceeds_the_quadratic_term() {
        // k x^2 / (1 + sqrt(1 - k^2 x^2)) > k x^2 / 2, by 7% near k x = 1/2
        let sag = sagitta(1.0, 0.499).unwrap();
        assert!(
            close(
                sag,
                0.499 * 0.499 / (1.0 + (1.0 - 0.499_f64 * 0.499).sqrt())
            ),
            "{sag}"
        );
        assert!(sag > 0.5 * 0.499 * 0.499 * 1.07);
        assert_eq!(sagitta(1.0, 0.51), None);
        assert_eq!(sagitta(f64::NAN, 0.1), None);
        assert_eq!(sagitta(-1.0, 0.1), None);
        assert_eq!(sagitta(0.0, 1e6), Some(0.0));
    }

    #[test]
    fn the_overlap_gap_refuses_what_it_cannot_bound() {
        let one = [exact(1.0); 2];
        let flat = [straight(1.0); 2];
        assert_eq!(overlap_gap(one, exact(0.0), exact(1.0), flat), None);
        assert_eq!(overlap_gap(one, exact(-0.5), exact(0.8), flat), None);
        assert_eq!(
            overlap_gap([exact(f64::NAN), exact(1.0)], exact(1.0), exact(0.0), flat),
            None
        );
        assert_eq!(
            overlap_gap([exact(1.0), exact(-1.0)], exact(1.0), exact(0.0), flat),
            None
        );
        // a zero reach, or one not proved positive: the crossing argument needs `R2 > 0`
        assert_eq!(
            overlap_gap([exact(1.0), exact(0.0)], exact(1.0), exact(0.0), flat),
            None
        );
        let unsure_reach = Bounded {
            value: 1e-12,
            bound: 2e-12,
        };
        assert_eq!(
            overlap_gap([exact(1.0), unsure_reach], exact(1.0), exact(0.0), flat),
            None
        );
        assert_eq!(overlap_gap(one, exact(f64::NAN), exact(0.0), flat), None);
        // a cosine not proved positive
        let unsure = Bounded {
            value: 1e-20,
            bound: 1e-19,
        };
        assert_eq!(overlap_gap(one, unsure, exact(0.0), flat), None);
        // tan beta past 1/2
        assert_eq!(overlap_gap(one, exact(0.8), exact(0.6), flat), None);
        // a rail that may fold at its reach, or bent too much over its run
        let folded = EdgeBend {
            curvature: 1.0,
            arc: 1.0,
        };
        assert_eq!(
            overlap_gap(one, exact(1.0), exact(0.0), [folded, straight(1.0)]),
            None
        );
        let tight = EdgeBend {
            curvature: 0.9,
            arc: 1.0,
        };
        assert_eq!(
            overlap_gap(one, exact(0.9), exact(0.4), [tight, straight(1.0)]),
            None
        );
        // a curvature bound over less edge than the rail runs: 1.05 X
        let beta = 1e-3_f64;
        let x = 2.0 * beta.tan();
        let (c, s) = (exact(beta.cos()), exact(beta.sin()));
        assert_eq!(
            overlap_gap(one, c, s, [straight(1.04 * x), straight(1.0)]),
            None
        );
        assert!(overlap_gap(one, c, s, [straight(1.06 * x), straight(1.0)]).is_some());
        assert_eq!(
            overlap_gap(
                one,
                exact(1.0),
                exact(0.0),
                [
                    EdgeBend {
                        curvature: f64::INFINITY,
                        arc: 1.0
                    },
                    straight(1.0)
                ]
            ),
            None
        );
    }

    #[test]
    fn a_rail_s_curvature_bound_follows_its_offset() {
        assert_eq!(rail_curvature(0.0, 1.0), Some(0.0));
        assert!(close(rail_curvature(0.25, 1.0).unwrap(), 0.25 / 0.75));
        assert!(rail_curvature(0.25, 1.0).unwrap() >= 0.25 / 0.75);
        assert!(close(rail_curvature(0.5, 1.0).unwrap(), 1.0));
        assert_eq!(rail_curvature(1.0, 1.0), None);
        assert_eq!(rail_curvature(f64::NAN, 1.0), None);
        assert_eq!(rail_curvature(0.1, -1.0), None);
        assert_eq!(rail_curvature(-0.1, 1.0), None);
    }
}
