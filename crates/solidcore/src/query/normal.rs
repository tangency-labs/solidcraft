//! A surface's unit normal and point with proved bounds (not upstream).
//!
//! The kernel's evaluators return a normal and a point in floating point with no statement
//! of their error. [`surface_normal`] returns the evaluator's normal (bit for bit, so
//! callers that compare it see the same numbers) together with a proved bound on its
//! distance from the exact unit normal of the surface S as stored at the given (u, v);
//! [`surface_point`] encloses the exact point S(u, v).
//!
//! **What S is, per kind.** S is the surface the kernel's evaluator defines, in the
//! evaluator's own parameters:
//! - a B-spline, cone, torus, revolution, extrusion and ball patch is its control net (an
//!   exact class holds its analytic kind at bound 0 on that net), and so is a surface
//!   carrying an exact round's definition (its net is the exact surface, bound 0);
//! - a plane is its placement: `origin + u s_u X + v s_v Y`, with the unit normal
//!   `X x Y / |X x Y|` its evaluator returns, `(u, v)` clamped to its domain as its
//!   evaluator clamps it;
//! - a sphere is its net, with a normal by convention: the exact sphere's normal through
//!   the net's point, `(S - centre) / |S - centre|` (the radial direction its evaluator
//!   returns), reversed where the sphere's reversal flag is set (`swap_uv != inside_out`).
//!   That is not the exact normal of the rounded net, which leans from it at the scale of
//!   the net's rounding; the bound is against the radial normal;
//! - a procedural blend is its definition, not its net: its normal and point are enclosed
//!   on the definition ([`crate::blend::fillet_section::definition_box`]), whether or not
//!   a net scope makes the evaluator answer from the net, so a value taken from the net is
//!   measured against S and its bound covers the net's distance from S too. A definition
//!   that module does not enclose (only the ball's circle of one radius between NURBS
//!   faces is) is refused. The faces the definition rolls on are enclosed piece by piece
//!   over the feet's boxes (`nurbs_partials_over_box`: cut at the knot lines a box crosses,
//!   each part on its own piece, the hull), so no station invariant is assumed: a foot may
//!   cross a face's knot line (at a station, or on a refitted net) and the enclosure is
//!   still of S, certified to the face's continuity there. A point is refused only where a
//!   crossed knot line's continuity is below the order the feet need (second partials and
//!   one more than the normal's); the leader likewise, piece by piece. **Dependency:** the
//!   faces' partials (`nurbs_partials_over_piece`) come from `ders_basis::<Ivl>`, which
//!   rounds each knot difference to a double before making it an interval (solidcore#2559);
//!   until that is fixed the enclosure holds on the divide's spare width, not by
//!   construction, so a bound on a procedural blend is as rigorous as #2559's fix makes it;
//! - a curve-bounded surface is its basis surface's.
//!
//! Refused ([`SolidError::Unsupported`]): a STEP-parameterised surface, a surface in a
//! volume and an offset surface (its evaluator returns its base's normal inside the base's
//! domain, which is the offset's only where the offset does not fold; nothing here proves
//! that).
//!
//! **The sense.** The exact normal's sense is the evaluator's convention for the kind, never
//! a choice made from the value being measured: for a net (and a definition), the sense of
//! `S_u x S_v`, which the evaluator's normal takes; for a plane its axis; for a sphere the
//! radial direction (the convention above) times its reversal flag. A value in the other sense is measured as
//! about 2 away and refused. Where the enclosure of `S_u x S_v` (or of `S - centre`) holds
//! 0, a pole or a collapsed side, there is no sense to prove and the point is refused as
//! [`SolidError::Degenerate`].
//!
//! **The enclosures.** A net's homogeneous point and first partials by the B-spline basis
//! (the NURBS book's A2.2 and the derivative recurrence) in rounded interval arithmetic
//! ([`crate::blend::fillet_section::interval`]), every knot difference enclosed too. For a
//! rational net `S = H / W`, `S_u x S_v = (H_u W - H W_u) x (H_v W - H W_v) / W^4`, so the
//! unit normal is that of the numerators' cross product, `W^4` being positive wherever the
//! cross product is proved non-zero.
//!
//! Deviations: none (no upstream counterpart). No recursion beyond the fixed curve-bounded
//! hops, bounded by the surface nesting limit.

use crate::blend::fillet_section::definition_box::{definition_partials_piecewise, gn_def_face_of};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::station::{as_b_spline, Nurbs};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec3};
use crate::nurbs::math::NOW;
use crate::nurbs::srf_basic::Surface as Net;
use crate::surface::plane::Plane;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "query::normal::surface_normal";

/// The surface nesting the curve-bounded hops may follow.
const MAX_HOPS: usize = 16;

/// A unit vector with a proved bound: the exact unit vector is within `bound` of
/// `value` (Euclidean).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitVector {
    /// The evaluated vector.
    pub value: Vec3,
    /// A proved bound on `|value - exact|`, finite, in `[0, 1)`.
    pub bound: f64,
}

/// The sense a normal is asked in: the surface's own (its evaluator's), or the opposite one
/// (a face used against its surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sense {
    /// The surface's sense.
    Surface,
    /// The opposite sense.
    Reversed,
}

/// The kernel's unit normal of `surface` at `uv` (`Surface::evaluate_normal`, from the
/// left on a knot) with a proved bound on its distance from the exact unit normal of S
/// at `uv` in the surface's sense (module docs).
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a kind with no enclosure or a bound that cannot be
/// proved below 1; [`SolidError::Degenerate`] at a point with no normal; those of the
/// evaluator.
pub fn surface_normal(k: &Kernel, surface: &Surface, uv: Point2) -> Result<UnitVector> {
    use crate::nurbs::knots::Side;
    let value = surface.evaluate_normal(k, uv, Side::Left, Side::Left)?;
    certify(k, surface, uv, value, Sense::Surface)
}

/// Proves how close `value` is to the exact unit normal of S at `uv` in `sense` (module
/// docs). `value` need not be the evaluator's: it is any vector to be measured against
/// the exact normal. A value nearer the other sense's normal is refused, not turned.
///
/// # Errors
///
/// As [`surface_normal`], without the evaluator's; [`SolidError::InvalidInput`] for a
/// non-finite value or `uv`.
pub fn certify(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    value: Vec3,
    sense: Sense,
) -> Result<UnitVector> {
    if !(value.x.is_finite() && value.y.is_finite() && value.z.is_finite()) {
        return Err(SolidError::invalid_input(OP, "non-finite normal"));
    }
    if !(uv.x.is_finite() && uv.y.is_finite()) {
        return Err(SolidError::invalid_input(OP, "non-finite (u, v)"));
    }
    let n = exact_normal(k, surface, uv)?;
    let s = Ivl::from(match sense {
        Sense::Surface => 1.0,
        Sense::Reversed => -1.0,
    });
    let d = |v: f64, c: Ivl| square(Ivl::from(v) - s * c);
    let bound = round_up(
        (d(value.x, n[0]) + d(value.y, n[1]) + d(value.z, n[2]))
            .sqrt()
            .hi,
    );
    if bound.partial_cmp(&1.0) != Some(core::cmp::Ordering::Less) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "the value is not within 1 of the exact normal in the sense asked",
        });
    }
    Ok(UnitVector {
        value,
        bound: if bound > 0.0 { bound } else { 0.0 },
    })
}

/// The exact point S(`uv`) of `surface` (module docs), each coordinate enclosed.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a kind with no enclosure or `uv` outside the net;
/// [`SolidError::InvalidInput`] for a non-finite `uv`; [`SolidError::Degenerate`] where
/// the net's weight is not proved positive.
pub fn surface_point(k: &Kernel, surface: &Surface, uv: Point2) -> Result<[Ivl; 3]> {
    if !(uv.x.is_finite() && uv.y.is_finite()) {
        return Err(SolidError::invalid_input(OP, "non-finite (u, v)"));
    }
    match exact_of(k, surface)? {
        Exact::Net(net) | Exact::Sphere { net, .. } => net_jet(net, uv)?.point(),
        Exact::Plane(p) => Ok(plane_point(p, uv)),
        Exact::Definition(s) => {
            let (p, _, _) = definition_jet(k, s, uv, 0)?;
            Ok(p)
        }
    }
}

/// What S is for one surface (module docs).
enum Exact<'a> {
    /// The control net.
    Net(&'a Net),
    /// A plane's placement.
    Plane(&'a Plane),
    /// A sphere's net, its centre and whether its normal is reversed.
    Sphere {
        net: &'a Net,
        centre: crate::math::Point3,
        reversed: bool,
    },
    /// A procedural blend's definition.
    Definition(&'a Surface),
}

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

fn degenerate() -> SolidError {
    SolidError::Degenerate {
        operation: OP,
        entity: None,
    }
}

/// What S is for `surface`, following curve-bounded surfaces to their basis.
fn exact_of<'a>(k: &Kernel, surface: &'a Surface) -> Result<Exact<'a>> {
    let net = |b: &'a crate::nurbs::b_spline_surface::BSplineSurface| {
        b.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))
    };
    let mut s = surface;
    for _ in 0..MAX_HOPS {
        let b = match s.kind_with_nurb(k)? {
            SurfaceKind::BSpline(b) => b,
            SurfaceKind::Plane(p) => return Ok(Exact::Plane(p)),
            SurfaceKind::Cone(c) => c.rev().base(),
            SurfaceKind::Sphere(c) => {
                return Ok(Exact::Sphere {
                    net: net(c.base().base())?,
                    centre: c.base().position().origin(),
                    reversed: c.base().swap_uv() != c.inside_out(),
                })
            }
            SurfaceKind::Torus(c) => c.base().base(),
            SurfaceKind::SurfOfRevolution(c) => c.base(),
            SurfaceKind::SurfOfExtrusion(c) => c.base(),
            SurfaceKind::ProceduralBlend(_) => return Ok(Exact::Definition(s)),
            SurfaceKind::BallPatch(c) => c.net(),
            SurfaceKind::CurveBounded(c) => {
                s = c.basis();
                continue;
            }
            SurfaceKind::OffsetSurface(_) => {
                return Err(unsupported(
                    "an offset surface's normal is its base's only where it does not fold",
                ))
            }
            SurfaceKind::Step(_) | SurfaceKind::SrfInVolume(_) => {
                return Err(unsupported("no control net to enclose the surface on"))
            }
        };
        return Ok(Exact::Net(net(b)?));
    }
    Err(unsupported("surface nesting too deep"))
}

/// The exact unit normal of S at `uv` in the surface's sense, each component enclosed.
fn exact_normal(k: &Kernel, surface: &Surface, uv: Point2) -> Result<[Ivl; 3]> {
    match exact_of(k, surface)? {
        Exact::Net(net) => {
            let j = net_jet(net, uv)?;
            unit(cross(j.tangent_u(), j.tangent_v()))
        }
        Exact::Plane(p) => {
            let (x, y) = (p.position().x_axis(), p.position().y_axis());
            unit(cross(ivl3(x), ivl3(y)))
        }
        Exact::Sphere {
            net,
            centre,
            reversed,
        } => {
            let pt = net_jet(net, uv)?.point()?;
            let c = ivl3(centre);
            let radial = [pt[0] - c[0], pt[1] - c[1], pt[2] - c[2]];
            let n = unit(radial)?;
            Ok(if reversed { n.map(neg) } else { n })
        }
        Exact::Definition(s) => {
            let (_, su, sv) = definition_jet(k, s, uv, 1)?;
            unit(cross(su, sv))
        }
    }
}

fn neg(x: Ivl) -> Ivl {
    Ivl::from(0.0) - x
}

fn ivl3(v: Vec3) -> [Ivl; 3] {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

fn cross(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `c / |c|`; [`SolidError::Degenerate`] where the enclosure of `|c|` holds 0.
fn unit(c: [Ivl; 3]) -> Result<[Ivl; 3]> {
    let len = (square(c[0]) + square(c[1]) + square(c[2])).sqrt();
    if !len.is_positive() || !len.hi.is_finite() {
        return Err(degenerate());
    }
    Ok([c[0] / len, c[1] / len, c[2] / len])
}

/// A plane's point at `uv`, clamped to its domain as [`Plane::evaluate_point`] clamps it.
fn plane_point(p: &Plane, uv: Point2) -> [Ivl; 3] {
    let d = p.step_uv_domain();
    let uv = if !p.is_out_of_bounds_enabled() && !d.contains_point(uv, 0.0) {
        d.clamp_point(uv)
    } else {
        uv
    };
    let pos = p.position();
    let (o, x, y) = (ivl3(pos.origin()), ivl3(pos.x_axis()), ivl3(pos.y_axis()));
    let scale = p.uv_scale();
    let (u, v) = (
        Ivl::from(uv.x) * Ivl::from(scale.x),
        Ivl::from(uv.y) * Ivl::from(scale.y),
    );
    [
        o[0] + u * x[0] + v * y[0],
        o[1] + u * x[1] + v * y[1],
        o[2] + u * x[2] + v * y[2],
    ]
}

/// A procedural blend's definition at `uv` to total order `lk` (0 or 1): its point and,
/// for order 1, its first partials, enclosed.
fn definition_jet(
    k: &Kernel,
    s: &Surface,
    uv: Point2,
    lk: usize,
) -> Result<([Ivl; 3], [Ivl; 3], [Ivl; 3])> {
    let refused = || unsupported("a blend's definition that is not enclosed at this point");
    let mut d = gn_def_face_of(k, s).map_err(|e| {
        if e.is_unported_or_cancelled() {
            e
        } else {
            unsupported("a blend's definition that is not enclosed (only the ball's circle of one radius between NURBS faces is)")
        }
    })?;
    let bs = as_b_spline(k, s)?.ok_or_else(|| SolidError::sm_err(OP))?;
    let net = Nurbs::of(&bs)?;
    let ku = span(&net.u, net.p, net.nu.saturating_sub(1), uv.x)
        .ok_or_else(|| unsupported("(u, v) outside the blend's net"))?;
    let kv = span(&net.v, net.q, net.nv.saturating_sub(1), uv.y)
        .ok_or_else(|| unsupported("(u, v) outside the blend's net"))?;
    let p = definition_partials_piecewise(k, &mut d, &net, ku, kv, uv.x, uv.x, uv.y, uv.y, lk)
        .map_err(|e| {
            if e.is_unported_or_cancelled() {
                e
            } else {
                refused()
            }
        })?;
    let v = |a, b| {
        let x = p.at(a, b);
        [x.x, x.y, x.z]
    };
    Ok((v(0, 0), v(1, 0), v(0, 1)))
}

/// A net's homogeneous point `[x, y, z, w]` and first partials at a point, enclosed.
struct NetJet {
    h: [Ivl; 4],
    hu: [Ivl; 4],
    hv: [Ivl; 4],
    rational: bool,
}

impl NetJet {
    /// `S = H / W`; [`SolidError::Degenerate`] where `W` is not proved positive.
    fn point(&self) -> Result<[Ivl; 3]> {
        let w = self.h[3];
        if !w.is_positive() {
            return Err(degenerate());
        }
        let p = [self.h[0] / w, self.h[1] / w, self.h[2] / w];
        if p.iter().all(|c| c.lo.is_finite() && c.hi.is_finite()) {
            Ok(p)
        } else {
            Err(degenerate())
        }
    }

    /// `W^2 S_u = H_u W - H W_u` (`H_u` alone when not rational).
    fn tangent_u(&self) -> [Ivl; 3] {
        self.tangent(self.hu)
    }

    /// `W^2 S_v`.
    fn tangent_v(&self) -> [Ivl; 3] {
        self.tangent(self.hv)
    }

    fn tangent(&self, d: [Ivl; 4]) -> [Ivl; 3] {
        let h = self.h;
        if self.rational {
            [
                d[0] * h[3] - h[0] * d[3],
                d[1] * h[3] - h[1] * d[3],
                d[2] * h[3] - h[2] * d[3],
            ]
        } else {
            [d[0], d[1], d[2]]
        }
    }
}

/// `x^2`, enclosed without the sign dependency of `x * x`.
fn square(x: Ivl) -> Ivl {
    if x.lo >= 0.0 || x.hi <= 0.0 {
        x * x
    } else {
        let m = x.mag();
        Ivl::new(0.0, (Ivl::from(m) * Ivl::from(m)).hi)
    }
}

/// The knot span `k` with `knots[k] < x <= knots[k + 1]` (from the left), or the first
/// non-empty span at the start, among the spans `p..=n` of a clamped net.
fn span(knots: &[f64], p: usize, n: usize, x: f64) -> Option<usize> {
    let lo = *knots.get(p)?;
    let hi = *knots.get(n + 1)?;
    if !(x >= lo && x <= hi) {
        return None;
    }
    (p..=n).find(|&k| match (knots.get(k), knots.get(k + 1)) {
        (Some(&a), Some(&b)) => a < b && (x <= b) && (x > a || a == lo),
        _ => false,
    })
}

/// The non-zero basis functions `N_{k-p..=k, p}` at `x` in span `k` and their first
/// derivatives, enclosed (A2.2 for degrees `p - 1` and `p`; `N' = p (N_{i,p-1} /
/// (U_{i+p} - U_i) - N_{i+1,p-1} / (U_{i+p+1} - U_{i+1}))`).
fn basis(knots: &[f64], k: usize, p: usize, x: f64) -> Option<(Vec<Ivl>, Vec<Ivl>)> {
    let u = |i: usize| knots.get(i).copied().map(Ivl::from);
    let x = Ivl::from(x);
    // N_{k-d..=k, d} for d = 0..=p, keeping degree p - 1
    let mut n = vec![Ivl::from(1.0)];
    let mut lower = Vec::new();
    for j in 1..=p {
        if j == p {
            lower.clone_from(&n);
        }
        let mut next = vec![Ivl::from(0.0); j + 1];
        let mut saved = Ivl::from(0.0);
        for r in 0..j {
            // N_{k-j+1+r, j-1} spreads over U_{k+1+r} - U_{k+1-j+r}
            let denom = u(k + 1 + r)? - u((k + 1 + r).checked_sub(j)?)?;
            if !denom.is_positive() {
                return None;
            }
            let temp = *n.get(r)? / denom;
            let right = u(k + 1 + r)? - x;
            let left = x - u((k + 1 + r).checked_sub(j)?)?;
            *next.get_mut(r)? = saved + right * temp;
            saved = left * temp;
        }
        *next.get_mut(j)? = saved;
        n = next;
    }
    let mut d = vec![Ivl::from(0.0); p + 1];
    if p > 0 {
        let pf = Ivl::from(f64::from(u32::try_from(p).ok()?));
        for (j, dj) in d.iter_mut().enumerate() {
            // i = k - p + j; lower index of N_{i,p-1} is j - 1, of N_{i+1,p-1} is j
            let i = (k + j).checked_sub(p)?;
            let mut v = Ivl::from(0.0);
            if j >= 1 {
                let denom = u(i + p)? - u(i)?;
                v = v + *lower.get(j - 1)? / denom;
            }
            if j < p {
                let denom = u(i + p + 1)? - u(i + 1)?;
                v = v - *lower.get(j)? / denom;
            }
            *dj = pf * v;
        }
    }
    Some((n, d))
}

/// `net`'s homogeneous point and first partials at `uv`, enclosed.
fn net_jet(net: &Net, uv: Point2) -> Result<NetJet> {
    let refuse = unsupported;
    let (p, q) = net.degrees();
    let (uk, vk) = (net.u_knots(), net.v_knots());
    let (ku, kv) = (
        span(uk, p, net.n(), uv.x).ok_or_else(|| refuse("(u, v) outside the net"))?,
        span(vk, q, net.m(), uv.y).ok_or_else(|| refuse("(u, v) outside the net"))?,
    );
    let ((nu, du), (nv, dv)) = (
        basis(uk, ku, p, uv.x).ok_or_else(|| refuse("degenerate knots"))?,
        basis(vk, kv, q, uv.y).ok_or_else(|| refuse("degenerate knots"))?,
    );
    let zero = Ivl::from(0.0);
    // homogeneous point and partials: [x, y, z, w]
    let (mut h, mut hu, mut hv) = ([zero; 4], [zero; 4], [zero; 4]);
    let mut rational = false;
    for (a, (bu, bdu)) in nu.iter().zip(&du).enumerate() {
        for (b, (bv, bdv)) in nv.iter().zip(&dv).enumerate() {
            let (i, j) = (
                (ku + a)
                    .checked_sub(p)
                    .ok_or_else(|| refuse("degenerate knots"))?,
                (kv + b)
                    .checked_sub(q)
                    .ok_or_else(|| refuse("degenerate knots"))?,
            );
            let c = net.cpt(i, j)?;
            let w = if c.w == NOW {
                1.0
            } else {
                rational = true;
                c.w
            };
            let (f, fu, fv) = (*bu * *bv, *bdu * *bv, *bu * *bdv);
            let coords = [c.x, c.y, c.z, w];
            for (((x, h), hu), hv) in coords.iter().zip(&mut h).zip(&mut hu).zip(&mut hv) {
                let x = Ivl::from(*x);
                *h = *h + f * x;
                *hu = *hu + fu * x;
                *hv = *hv + fv * x;
            }
        }
    }
    Ok(NetJet {
        h,
        hu,
        hv,
        rational,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::math::Point3;
    use crate::surface::ball_patch::BallPatch;
    use crate::surface::curve_bounded_surface::CurveBoundedSurface;
    use crate::surface::offset_surface::OffsetSurface;
    use crate::surface::sphere::Sphere;
    use crate::surface::surf_of_extrusion::SurfOfExtrusion;
    use crate::surface::surf_of_revolution::SurfOfRevolution;
    use crate::topology::FaceId;

    fn faces_of(b: &crate::topology::Brep) -> Vec<FaceId> {
        crate::query::api_queries::get_faces(b).unwrap()
    }

    /// A point inside `s`'s natural domain, off its middle and off its knots.
    fn inside(k: &Kernel, s: &Surface) -> Point2 {
        let d = s.natural_uv_domain_with_nurb(k).unwrap();
        Point2::new(
            d.min().x + 0.537 * (d.max().x - d.min().x),
            d.min().y + 0.419 * (d.max().y - d.min().y),
        )
    }

    /// The normal at `uv` is proved within `tight` and the closed form `exact` (in the
    /// surface's sense) is within its bound; the opposite vector is refused in the
    /// surface's sense and proved, with the same bound, in the reversed one.
    fn check_at(k: &Kernel, s: &Surface, uv: Point2, exact: Vec3, tight: f64) -> UnitVector {
        let n = surface_normal(k, s, uv).unwrap();
        let off = (n.value - exact).length();
        assert!(off <= n.bound + 1e-15, "off {off} bound {}", n.bound);
        assert!(n.bound <= tight, "bound {}", n.bound);
        let flipped = certify(k, s, uv, -n.value, Sense::Surface);
        assert!(
            matches!(flipped, Err(SolidError::Unsupported { .. })),
            "{flipped:?}"
        );
        let r = certify(k, s, uv, -n.value, Sense::Reversed).unwrap();
        assert_eq!(r.bound, n.bound);
        // the point: S(uv) enclosed, the evaluator's point inside its box widened by rounding
        let p = surface_point(k, s, uv).unwrap();
        let e = s.evaluate_point(k, uv).unwrap();
        for (c, x) in p.iter().zip([e.x, e.y, e.z]) {
            let slack = 1e-12 * (1.0 + x.abs());
            assert!(c.lo - slack <= x && x <= c.hi + slack, "{c:?} vs {x}");
            assert!(c.hi - c.lo <= 1e-12 * (1.0 + x.abs()), "{c:?}");
        }
        n
    }

    #[test]
    fn a_box_faces_normals_are_their_axes_in_their_sense() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        for f in faces_of(&b) {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let SurfaceKind::Plane(p) = s.kind() else {
                panic!("a box face is a plane")
            };
            // the plane's normal is its placement's axis, X x Y: one of the box's axes
            let z = p.position().z_axis();
            assert_eq!(
                [z.x.abs(), z.y.abs(), z.z.abs()]
                    .iter()
                    .filter(|&&v| v == 1.0)
                    .count(),
                1
            );
            check_at(&k, s, inside(&k, s), z, 1e-14);
        }
    }

    #[test]
    fn a_sphere_normal_is_radial_times_its_reversal_flag() {
        let mut k = Kernel::default();
        let c = Point3::new(1.0, -2.0, 0.5);
        let b = crate::primitive::api_primitives::create_sphere(&mut k, c, 3.0).unwrap();
        for f in faces_of(&b) {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let SurfaceKind::Sphere(sp) = s.kind() else {
                panic!("a sphere's face")
            };
            let sigma = if sp.base().swap_uv() == sp.inside_out() {
                1.0
            } else {
                -1.0
            };
            let uv = inside(&k, s);
            let p = s.evaluate_point(&k, uv).unwrap();
            check_at(&k, s, uv, (p - c) * (sigma / 3.0), 1e-12);
        }
    }

    #[test]
    fn an_inside_out_sphere_normal_points_in() {
        // the reversal flag decides the sense: the inside-out sphere's normal is inward,
        // and the outward one is refused in its sense
        let k = Kernel::default();
        let c = Point3::new(0.5, 0.25, -1.0);
        let place = Axis2Placement::new(c, Vec3::X, Vec3::Y);
        for (r, sigma) in [(2.0, 1.0), (-2.0, -1.0)] {
            let s = Surface::from(Sphere::create_canonical(&k, &place, r).unwrap());
            let uv = inside(&k, &s);
            let p = s.evaluate_point(&k, uv).unwrap();
            let outward = (p - c) / 2.0;
            check_at(&k, &s, uv, outward * sigma, 1e-12);
            let o = certify(&k, &s, uv, outward, Sense::Surface);
            assert_eq!(o.is_ok(), sigma > 0.0, "{r}: {o:?}");
        }
    }

    #[test]
    fn a_pole_is_proved_on_a_sphere_and_refused_on_its_net() {
        // the sphere's normal is radial, so it holds at its poles; the same net as a ball
        // patch takes S_u x S_v, which vanishes there: refused as degenerate, not guessed
        let k = Kernel::default();
        let c = Point3::new(0.0, 0.0, 1.0);
        let sp =
            Sphere::create_canonical(&k, &Axis2Placement::new(c, Vec3::X, Vec3::Y), 1.0).unwrap();
        let ball = Surface::from(BallPatch::create(sp.base().base(), c, 1.0).unwrap());
        let sp = Surface::from(sp);
        let d = sp.natural_uv_domain_with_nurb(&k).unwrap();
        let sides = [
            Point2::new(d.min().x, 0.5 * (d.min().y + d.max().y)),
            Point2::new(d.max().x, 0.5 * (d.min().y + d.max().y)),
            Point2::new(0.5 * (d.min().x + d.max().x), d.min().y),
            Point2::new(0.5 * (d.min().x + d.max().x), d.max().y),
        ];
        let mut poles = 0;
        for uv in sides {
            let p = sp.evaluate_point(&k, uv).unwrap();
            if (p.x - c.x).abs() > 1e-12 || (p.y - c.y).abs() > 1e-12 {
                continue;
            }
            poles += 1;
            let n = surface_normal(&k, &sp, uv).unwrap();
            assert!(n.bound <= 1e-12, "{n:?}");
            assert!((n.value.z.abs() - 1.0).abs() <= n.bound, "{n:?}");
            let on_net = surface_normal(&k, &ball, uv);
            assert!(
                matches!(on_net, Err(SolidError::Degenerate { .. })),
                "{on_net:?}"
            );
        }
        assert_eq!(poles, 2);
    }

    #[test]
    fn a_cylinder_normal_is_radial_to_its_axis() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 2.0, 5.0)
            .unwrap();
        for f in faces_of(&b) {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let uv = inside(&k, s);
            let n = surface_normal(&k, s, uv).unwrap();
            let p = s.evaluate_point(&k, uv).unwrap();
            let e = if matches!(s.kind(), SurfaceKind::Plane(_)) {
                Vec3::new(0.0, 0.0, n.value.z.signum())
            } else {
                Vec3::new(p.x / 2.0, p.y / 2.0, 0.0) * (n.value.x * p.x + n.value.y * p.y).signum()
            };
            check_at(&k, s, uv, e, 1e-12);
        }
    }

    #[test]
    fn a_cone_side_normal_and_its_apex() {
        // a cone of base radius 2, apex 4 up: the side's normal leans out by the half angle,
        // and the apex has none
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cone(&mut k, Point3::ZERO, 2.0, 0.0, 4.0)
            .unwrap();
        let mut sides = 0;
        for f in faces_of(&b) {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let SurfaceKind::Cone(_) = s.kind_with_nurb(&k).unwrap() else {
                continue;
            };
            sides += 1;
            let uv = inside(&k, s);
            let n = surface_normal(&k, s, uv).unwrap();
            let p = s.evaluate_point(&k, uv).unwrap();
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            // radial (x, y)/rho times cos, plus z times sin, of the half angle atan(2/4)
            let len = 20.0_f64.sqrt();
            let out = Vec3::new(p.x / rho * 4.0 / len, p.y / rho * 4.0 / len, 2.0 / len);
            let e = if n.value.dot(out) > 0.0 { out } else { -out };
            check_at(&k, s, uv, e, 1e-12);
            // the apex: the side of the domain whose points are all at z = 4
            let d = s.natural_uv_domain_with_nurb(&k).unwrap();
            let ends = [
                Point2::new(0.5 * (d.min().x + d.max().x), d.min().y),
                Point2::new(0.5 * (d.min().x + d.max().x), d.max().y),
                Point2::new(d.min().x, 0.5 * (d.min().y + d.max().y)),
                Point2::new(d.max().x, 0.5 * (d.min().y + d.max().y)),
            ];
            let apex = ends
                .into_iter()
                .find(|&uv| (s.evaluate_point(&k, uv).unwrap().z - 4.0).abs() < 1e-12)
                .unwrap();
            assert!(matches!(
                surface_normal(&k, s, apex),
                Err(SolidError::Degenerate { .. })
            ));
        }
        assert_eq!(sides, 1);
    }

    #[test]
    fn a_torus_normal_points_from_its_tube_centre() {
        let mut k = Kernel::default();
        let b =
            crate::primitive::api_primitives::create_torus(&mut k, Point3::ZERO, 4.0, 1.0).unwrap();
        for f in faces_of(&b) {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let uv = inside(&k, s);
            let n = surface_normal(&k, s, uv).unwrap();
            let p = s.evaluate_point(&k, uv).unwrap();
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            let out = p - Point3::new(4.0 * p.x / rho, 4.0 * p.y / rho, 0.0);
            let e = if n.value.dot(out) > 0.0 { out } else { -out };
            check_at(&k, s, uv, e, 1e-12);
        }
    }

    /// A line generator from `a` to `b` with its NURB made.
    fn line(a: Point3, b: Point3) -> Curve {
        let mut l = Line::from_points(a, b, 3);
        l.make_nurb().unwrap();
        Curve::from(l)
    }

    #[test]
    fn a_revolution_and_an_extrusion_of_a_line() {
        // a line at x = 1 parallel to z swept about z: the cylinder of radius 1, its normal
        // radial; the line from the origin to (1, 0, 1) extruded along y: the plane
        // x - z = 0, its normal (1, 0, -1) / sqrt 2 in one sense or the other
        let k = Kernel::default();
        let gen = line(Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 2.0));
        let rev = Surface::from(
            SurfOfRevolution::create_canonical(&k, gen, Vec3::ZERO, Vec3::Z, false).unwrap(),
        );
        assert!(matches!(rev.kind(), SurfaceKind::SurfOfRevolution(_)));
        let uv = inside(&k, &rev);
        let n = surface_normal(&k, &rev, uv).unwrap();
        let p = rev.evaluate_point(&k, uv).unwrap();
        let out = Vec3::new(p.x, p.y, 0.0);
        check_at(&k, &rev, uv, out * n.value.dot(out).signum(), 1e-12);
        let ext = Surface::from(
            SurfOfExtrusion::create_canonical(
                &k,
                line(Point3::ZERO, Point3::new(1.0, 0.0, 1.0)),
                Vec3::Y,
            )
            .unwrap(),
        );
        assert!(matches!(ext.kind(), SurfaceKind::SurfOfExtrusion(_)));
        let uv = inside(&k, &ext);
        let n = surface_normal(&k, &ext, uv).unwrap();
        let e = Vec3::new(1.0, 0.0, -1.0) / 2.0_f64.sqrt();
        check_at(&k, &ext, uv, e * n.value.dot(e).signum(), 1e-12);
    }

    #[test]
    fn a_curve_bounded_surface_is_its_basis() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_sphere(&mut k, Point3::ZERO, 2.0).unwrap();
        let s = b.face(faces_of(&b)[0]).unwrap().surface.clone().unwrap();
        let cb = Surface::from(CurveBoundedSurface::new(s.clone(), vec![], vec![], true).unwrap());
        let uv = inside(&k, &s);
        let a = surface_normal(&k, &s, uv).unwrap();
        let c = surface_normal(&k, &cb, uv).unwrap();
        assert_eq!(a, c);
        assert_eq!(
            surface_point(&k, &s, uv).unwrap(),
            surface_point(&k, &cb, uv).unwrap()
        );
    }

    #[test]
    fn a_ball_patch_normal_is_radial_in_its_net_s_sense() {
        // the sphere's net as a ball patch: the same net, its sense the net's S_u x S_v
        let k = Kernel::default();
        let c = Point3::new(0.0, 1.0, 0.0);
        let sp =
            Sphere::create_canonical(&k, &Axis2Placement::new(c, Vec3::X, Vec3::Y), 1.5).unwrap();
        let s = Surface::from(BallPatch::create(sp.base().base(), c, 1.5).unwrap());
        assert!(matches!(s.kind(), SurfaceKind::BallPatch(_)));
        let uv = inside(&k, &s);
        let n = surface_normal(&k, &s, uv).unwrap();
        let p = s.evaluate_point(&k, uv).unwrap();
        let out = (p - c) / 1.5;
        check_at(&k, &s, uv, out * n.value.dot(out).signum(), 1e-12);
    }

    #[test]
    fn an_offset_surface_and_a_value_off_the_normal_are_refused() {
        let k = Kernel::default();
        let sp = Surface::from(
            Sphere::create_canonical(
                &k,
                &Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Y),
                1.0,
            )
            .unwrap(),
        );
        let uv = inside(&k, &sp);
        let off = Surface::from(OffsetSurface::new(&k, 0.5, sp.clone(), true, 0).unwrap());
        assert!(matches!(
            surface_normal(&k, &off, uv),
            Err(SolidError::Unsupported { .. })
        ));
        // a value at right angles to the normal is about sqrt 2 off: refused either way
        let n = surface_normal(&k, &sp, uv).unwrap().value;
        let other = n.cross(Vec3::Z).unitize().unwrap();
        for sense in [Sense::Surface, Sense::Reversed] {
            assert!(certify(&k, &sp, uv, other, sense).is_err());
        }
        assert!(certify(&k, &sp, uv, Vec3::new(f64::NAN, 0.0, 0.0), Sense::Surface).is_err());
    }

    #[test]
    fn a_moved_normal_is_measured_by_how_far_it_moved() {
        // certify a value turned by a known angle off the exact normal: the bound covers
        // it and is no smaller than the chord
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
            .unwrap();
        let f = faces_of(&b)[0];
        let s = b.face(f).unwrap().surface.as_ref().unwrap();
        let uv = inside(&k, s);
        let n = surface_normal(&k, s, uv).unwrap();
        let t = 1e-3_f64;
        let other = if n.value.x.abs() < 0.5 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let moved = n.value * t.cos() + other * t.sin();
        let m = certify(&k, s, uv, moved, Sense::Surface).unwrap();
        let chord = 2.0 * (t / 2.0).sin();
        assert!(m.bound >= chord * (1.0 - 1e-9), "{} vs {chord}", m.bound);
        assert!(m.bound <= chord + 1e-12, "{} vs {chord}", m.bound);
    }

    #[test]
    fn a_rounded_net_normal_on_a_rational_patch_is_enclosed() {
        // the sphere's net is rational: its knots and weights not powers of two
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_sphere(&mut k, Point3::ZERO, 1.0 / 3.0)
            .unwrap();
        for f in faces_of(&b) {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let uv = inside(&k, s);
            let n = surface_normal(&k, s, uv).unwrap();
            let p = s.evaluate_point(&k, uv).unwrap();
            let out = p * 3.0;
            check_at(&k, s, uv, out * n.value.dot(out).signum(), 1e-12);
        }
    }
}
