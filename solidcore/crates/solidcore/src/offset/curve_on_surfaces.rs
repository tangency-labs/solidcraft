//! Whether a curve is proved to lie within a distance of two surfaces: the check
//! `MateEdges` makes before it keeps an offset edge whose ends match the mated intersection
//! (not upstream).
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`curve_on_surfaces`]    | none     |
//!
//! **The bound.** For a curve `C` and a surface `S`, split the curve's parameter range into
//! segments `[a, b]`, each inside one polynomial piece of `C`. At both ends take any surface
//! point `S(w_a)`, `S(w_b)` (the surface's closest point; any point gives an upper bound) and
//! join them in the surface's parameters by the straight line `w(t)`. The distance of `C(t)`
//! from `S` is at most `|g(t)|`, `g = C - S(w)`, and linear interpolation of `g` misses it
//! by at most `h^2 / 8 sup |g''|` (`h = b - a`): so the distance is at most
//! `max(|g(a)|, |g(b)|) + h^2 / 8 sup |g''|` on the segment. With `w' = (du, dv) / h`
//! constant, `g'' = C'' - (S_uu du^2 + 2 S_uv du dv + S_vv dv^2) / h^2`. `C(a)`, `C(b)`,
//! `C''` and the surface's point and second partials are enclosed in rounded interval
//! arithmetic ([`curve_derivs_over`], [`partials_over`]) over the segment and the parameter
//! box of `w`. A segment over the distance is cut in two, to [`MAX_DEPTH`] halvings.
//!
//! **Offsets.** For an offset `O = S + d n` of a B-spline net (`n = N / |N|`,
//! `N = S_u x S_v`), `S` is replaced by `O`: its point by the net's point plus `d N / |N|`
//! (enclosed by interval division, `|N|` proved positive), and its second derivative along
//! `w` by the net's plus `d n''`, bounded by `|n''| <= 2 |N''| / |N| + 5 |N'|^2 / |N|^2`
//! (from `n'' = N''/r - 2 N' r'/r^2 + N (2 r'^2 / r^3 - r''/r^2)`, `r = |N|`, `|r'| <= |N'|`,
//! `|r''| <= (|N'|^2 + r |N''|) / r`), `N'` and `N''` along `w` from the net's partials to
//! the third order. An offset is checked only over its net's own domain, where it is that
//! formula (outside it the offset may evaluate an extension).
//!
//! The interpolation bound needs `g` once continuously differentiable: the segments are cut
//! at the curve's knots, and a surface with an interior knot of multiplicity at least its
//! degree (a crease in its parameters; for an offset, one less) is not checked.
//!
//! Deviations: none (no upstream counterpart). Only a B-spline curve on B-spline surfaces or
//! offsets of them is checked; anything else is not proved (`false`). Recursion: none (an
//! explicit stack, at most [`MAX_DEPTH`] halvings of each knot span).

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Nurbs;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::mass::enclosure::partials_over;
use crate::query::api_queries::surface_closest_point;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;

const OP: &str = "offset::curve_on_surfaces";

/// The most halvings of one knot span of the curve.
pub const MAX_DEPTH: u32 = 24;

/// The first segments a knot span is cut into.
const FIRST_SEGMENTS: usize = 16;

/// A surface the check handles: a B-spline net, or an offset of one by `d` over the net's
/// domain.
struct Checked {
    net: Nurbs,
    offset: Option<(f64, Extent2d)>,
}

impl Checked {
    fn of(k: &Kernel, s: &Surface) -> Option<Self> {
        match s.kind() {
            SurfaceKind::BSpline(b) => Some(Self {
                net: Nurbs::of(b).ok()?,
                offset: None,
            }),
            SurfaceKind::OffsetSurface(o) => {
                let SurfaceKind::BSpline(b) = o.base_surface().kind() else {
                    return None;
                };
                let d = o.offset_distance();
                let domain = o.base_surface().natural_uv_domain_with_nurb(k).ok()?;
                d.is_finite().then_some(())?;
                Some(Self {
                    net: Nurbs::of(b).ok()?,
                    offset: Some((d, domain)),
                })
            }
            _ => None,
        }
    }

    /// Whether the box lies where the bound holds.
    fn covers(&self, u: Ivl, v: Ivl) -> bool {
        self.offset.as_ref().is_none_or(|(_, dom)| {
            u.lo >= dom.u_min() && u.hi <= dom.u_max() && v.lo >= dom.v_min() && v.hi <= dom.v_max()
        })
    }
}

/// Whether `curve` over `interval` is proved within `tol` of each of `surfaces` (module
/// docs). `false` where it is not proved: the kinds are not checked, a closest point is not
/// found, an enclosure fails or a segment stays over `tol` after [`MAX_DEPTH`] halvings.
///
/// # Errors
///
/// [`SolidError::Cancelled`](crate::error::SolidError::Cancelled) when the kernel's cancel
/// token fires (polled once per segment); none from the checks themselves.
pub fn curve_on_surfaces(
    k: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    surfaces: [&Surface; 2],
    tol: f64,
) -> Result<bool> {
    let CurveKind::BSpline(bs) = curve.kind() else {
        return Ok(false);
    };
    let Ok(cn) = NurbsCurve::of(bs) else {
        return Ok(false);
    };
    if !(tol > 0.0 && interval.min().is_finite() && interval.max().is_finite()) {
        return Ok(false);
    }
    for s in surfaces {
        let Some(c) = Checked::of(k, s) else {
            return Ok(false);
        };
        if creased(&c) || !on_surface(k, &cn, curve, s, &c, interval, tol)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether the net has an interior knot of multiplicity at least its degree (one less for
/// an offset, whose normal takes one more derivative) in either direction.
fn creased(c: &Checked) -> bool {
    let less = usize::from(c.offset.is_some());
    let crease = |knots: &[f64], p: usize| {
        let limit = p.saturating_sub(less);
        let interior = knots
            .get(p + 1..knots.len().saturating_sub(p + 1))
            .unwrap_or(&[]);
        if interior.is_empty() {
            return false;
        }
        if limit <= 1 {
            return true;
        }
        let mut run = 1;
        for w in interior.windows(2) {
            if let [a, b] = w {
                if a == b {
                    run += 1;
                    if run >= limit {
                        return true;
                    }
                } else {
                    run = 1;
                }
            }
        }
        false
    };
    crease(&c.net.u, c.net.p) || crease(&c.net.v, c.net.q)
}

/// A surface parameter of the point nearest `p`'s, `None` where none is found.
fn foot(k: &Kernel, s: &Surface, t: f64, curve: &Curve) -> Option<Point2> {
    let p = curve.evaluate_point(k, t).ok()?;
    let uv = *surface_closest_point(k, s, p).ok()?.uvs.first()?;
    (uv.x.is_finite() && uv.y.is_finite()).then_some(uv)
}

type IV3 = V3<Ivl>;

/// An upper bound of `|v|`, rounding included.
fn norm_up(v: IV3) -> f64 {
    let (x, y, z) = (v.x.mag(), v.y.mag(), v.z.mag());
    round_up(round_up(round_up(x * x + y * y) + z * z).sqrt())
}

/// A lower bound of `|v|`, rounding included (0 where it is not proved positive).
fn norm_down(v: IV3) -> f64 {
    let sq = |c: Ivl| {
        if c.lo > 0.0 {
            round_down(c.lo * c.lo)
        } else if c.hi < 0.0 {
            round_down(c.hi * c.hi)
        } else {
            0.0
        }
    };
    let s = round_down(round_down(sq(v.x) + sq(v.y)) + sq(v.z));
    if s > 0.0 {
        round_down(s.sqrt())
    } else {
        0.0
    }
}

fn add(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn sub(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn scale(a: IV3, c: Ivl) -> IV3 {
    V3 {
        x: a.x * c,
        y: a.y * c,
        z: a.z * c,
    }
}

fn cross(a: IV3, b: IV3) -> IV3 {
    V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// An upper bound of `|C(t) - O(w)|`, `None` where an enclosure fails.
fn gap_at(cn: &NurbsCurve, c: &Checked, t: f64, w: Point2) -> Option<f64> {
    let (u, v) = (Ivl::from(w.x), Ivl::from(w.y));
    if !c.covers(u, v) {
        return None;
    }
    let cv = *curve_derivs_over(cn, Ivl::from(t), 0, None).ok()?.first()?;
    let order = usize::from(c.offset.is_some());
    let p = partials_over(&c.net, u, v, order).ok().flatten()?;
    let mut sp = p.at(0, 0);
    if let Some((d, _)) = c.offset {
        let n = cross(p.at(1, 0), p.at(0, 1));
        if norm_down(n) <= 0.0 {
            return None;
        }
        let len = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
        let unit = V3 {
            x: n.x / len,
            y: n.y / len,
            z: n.z / len,
        };
        sp = add(sp, scale(unit, Ivl::from(d)));
    }
    let g = norm_up(sub(cv, sp));
    g.is_finite().then_some(g)
}

/// An upper bound of `h^2 / 8 sup |g''|` over the segment (module docs).
fn bend(cn: &NurbsCurve, c: &Checked, a: f64, b: f64, wa: Point2, wb: Point2) -> Option<f64> {
    let (u, v) = (
        Ivl::new(wa.x.min(wb.x), wa.x.max(wb.x)),
        Ivl::new(wa.y.min(wb.y), wa.y.max(wb.y)),
    );
    if !c.covers(u, v) {
        return None;
    }
    let c2 = *curve_derivs_over(cn, Ivl::new(a, b), 2, None)
        .ok()?
        .get(2)?;
    let order = if c.offset.is_some() { 3 } else { 2 };
    let p = partials_over(&c.net, u, v, order).ok().flatten()?;
    let (du, dv) = (
        Ivl::from(wb.x) - Ivl::from(wa.x),
        Ivl::from(wb.y) - Ivl::from(wa.y),
    );
    let h = Ivl::from(b) - Ivl::from(a);
    let two = Ivl::from(2.0);
    // second derivative along w of a field F with partials F_u, F_v and second ones, times
    // h^2: F_uu du^2 + 2 F_uv du dv + F_vv dv^2
    let quad = |uu: IV3, uv: IV3, vv: IV3| {
        add(
            add(scale(uu, du * du), scale(uv, two * du * dv)),
            scale(vv, dv * dv),
        )
    };
    let s2 = quad(p.at(2, 0), p.at(1, 1), p.at(0, 2));
    let g = sub(scale(c2, h * h), s2);
    let mut extra = 0.0;
    if let Some((dist, _)) = c.offset {
        let (su, sv) = (p.at(1, 0), p.at(0, 1));
        let r = norm_down(cross(su, sv));
        if r <= 0.0 {
            return None;
        }
        // first and second derivatives of N = S_u x S_v along w, times h and h^2
        let su1 = add(scale(p.at(2, 0), du), scale(p.at(1, 1), dv));
        let sv1 = add(scale(p.at(1, 1), du), scale(p.at(0, 2), dv));
        let n1 = add(cross(su1, sv), cross(su, sv1));
        let su2 = quad(p.at(3, 0), p.at(2, 1), p.at(1, 2));
        let sv2 = quad(p.at(2, 1), p.at(1, 2), p.at(0, 3));
        let n2 = add(
            add(cross(su2, sv), scale(cross(su1, sv1), two)),
            cross(su, sv2),
        );
        let (n1m, n2m) = (norm_up(n1), norm_up(n2));
        // |n''| h^2 <= 2 |N''| h^2 / r + 5 (|N'| h)^2 / r^2
        let nn = round_up(
            round_up(2.0 * n2m / r) + round_up(5.0 * round_up(n1m * n1m) / round_up(r * r)),
        );
        extra = round_up(dist.abs() * nn);
    }
    let bound = round_up(round_up(norm_up(g) + extra) / 8.0);
    bound.is_finite().then_some(bound)
}

/// Whether the curve is proved within `tol` of one surface over `interval`.
fn on_surface(
    k: &Kernel,
    cn: &NurbsCurve,
    curve: &Curve,
    s: &Surface,
    c: &Checked,
    interval: Extent1d,
    tol: f64,
) -> Result<bool> {
    // the knot spans inside the interval
    let (lo, hi) = (interval.min(), interval.max());
    let mut cuts = vec![lo];
    for &u in &cn.u {
        if u > lo && u < hi && cuts.last().is_some_and(|&l| u > l) {
            cuts.push(u);
        }
    }
    cuts.push(hi);
    for span in cuts.windows(2) {
        let [a0, b0] = span else { continue };
        let mut stack: Vec<(f64, f64, u32)> = Vec::new();
        for i in (0..FIRST_SEGMENTS).rev() {
            let f = |j: usize| {
                if j == FIRST_SEGMENTS {
                    *b0
                } else {
                    a0 + (b0 - a0) * (j as f64) / (FIRST_SEGMENTS as f64)
                }
            };
            stack.push((f(i), f(i + 1), 0));
        }
        while let Some((a, b, depth)) = stack.pop() {
            k.check_cancelled(OP)?;
            let (Some(wa), Some(wb)) = (foot(k, s, a, curve), foot(k, s, b, curve)) else {
                return Ok(false);
            };
            let ok = match (gap_at(cn, c, a, wa), gap_at(cn, c, b, wb)) {
                (Some(ga), Some(gb)) if ga.max(gb) > tol => return Ok(false),
                (Some(ga), Some(gb)) => {
                    bend(cn, c, a, b, wa, wb).is_some_and(|e| round_up(ga.max(gb) + e) <= tol)
                }
                _ => return Ok(false),
            };
            if ok {
                continue;
            }
            let m = 0.5 * (a + b);
            if depth >= MAX_DEPTH || !(a < m && m < b) {
                return Ok(false);
            }
            stack.push((m, b, depth + 1));
            stack.push((a, m, depth + 1));
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::b_spline_surface::p1::BSplineSurface;
    use crate::nurbs::math::CPoint;

    /// The plane z = `z` over `[0, 10]^2` as a bilinear B-spline in `[0, 1]^2`.
    fn plane(z: f64) -> Surface {
        let pw = vec![
            vec![
                CPoint::new(0.0, 0.0, z, 1.0),
                CPoint::new(0.0, 10.0, z, 1.0),
            ],
            vec![
                CPoint::new(10.0, 0.0, z, 1.0),
                CPoint::new(10.0, 10.0, z, 1.0),
            ],
        ];
        Surface::from(
            BSplineSurface::from_data(1, 1, 1, 1, 3, 3, &pw, &[0., 0., 1., 1.], &[0., 0., 1., 1.])
                .expect("plane"),
        )
    }

    /// A cubic arc in z = `z` from (1, 1) to (9, 9), bulging to y = 6 at its middle.
    fn arc(z: f64) -> Curve {
        let pts = [
            CPoint::new(1.0, 1.0, z, 1.0),
            CPoint::new(3.0, 6.0, z, 1.0),
            CPoint::new(7.0, 9.0, z, 1.0),
            CPoint::new(9.0, 9.0, z, 1.0),
        ];
        Curve::from(
            BSplineCurve::from_data(3, 3, 7, 3, &pts, &[0., 0., 0., 0., 1., 1., 1., 1.])
                .expect("arc"),
        )
    }

    #[test]
    fn a_curve_in_both_planes_is_proved_and_one_off_them_is_not() {
        let k = Kernel::default();
        let (a, b) = (plane(0.0), plane(0.0));
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        assert!(curve_on_surfaces(&k, &arc(0.0), ivl, [&a, &b], 1e-6).expect("check"));
        // 1e-3 off one of them
        let c = plane(1e-3);
        assert!(!curve_on_surfaces(&k, &arc(0.0), ivl, [&a, &c], 1e-6).expect("check"));
        // within a looser distance it is proved again
        assert!(curve_on_surfaces(&k, &arc(0.0), ivl, [&a, &c], 2e-3).expect("check"));
    }

    /// An offset of a net is checked through its normal: the arc in z = 0.5 lies on the
    /// plane offset by 0.5, not on it offset by 0.501.
    #[test]
    fn an_offset_surface_is_checked_through_its_normal() {
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let off =
            |d: f64| Surface::from(OffsetSurface::new(&k, d, plane(0.0), true, 0).expect("offset"));
        let (a, b) = (off(0.5), off(0.501));
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        assert!(curve_on_surfaces(&k, &arc(0.5), ivl, [&a, &a], 1e-6).expect("check"));
        assert!(!curve_on_surfaces(&k, &arc(0.5), ivl, [&a, &b], 1e-6).expect("check"));
    }

    /// A biquadratic bump over `[0, 1]^2` and its iso-curve u = 1/2, a quadratic B-spline
    /// lying on it exactly (control points the rows' 1/4, 1/2, 1/4 blend).
    fn bump_and_iso() -> (Surface, Curve) {
        let z = [[0.0, 0.3, 0.0], [0.4, 1.0, 0.2], [0.0, 0.5, 0.1]];
        let p = |i: usize, j: usize| {
            [
                i as f64 * 5.0,
                j as f64 * 4.0,
                z.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0) * 3.0,
            ]
        };
        let pw: Vec<Vec<CPoint>> = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| {
                        let q = p(i, j);
                        CPoint::new(q[0], q[1], q[2], 1.0)
                    })
                    .collect()
            })
            .collect();
        let knots = [0., 0., 0., 1., 1., 1.];
        let s = Surface::from(
            BSplineSurface::from_data(2, 2, 2, 2, 5, 5, &pw, &knots, &knots).expect("bump"),
        );
        let iso: Vec<CPoint> = (0..3)
            .map(|j| {
                let (a, b, c) = (p(0, j), p(1, j), p(2, j));
                let m = |k: usize| 0.25 * a[k] + 0.5 * b[k] + 0.25 * c[k];
                CPoint::new(m(0), m(1), m(2), 1.0)
            })
            .collect();
        let c = Curve::from(BSplineCurve::from_data(3, 2, 5, 2, &iso, &knots).expect("iso"));
        (s, c)
    }

    /// On a curved net the check runs through its second partials (`bend`'s S'' term), and
    /// on an offset of it through the normal's second derivative: the iso-curve is on the
    /// net, and 1e-4 off its offset by 1e-4.
    #[test]
    fn a_curve_on_a_curved_net_and_near_its_offset_is_checked() {
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let (s, c) = bump_and_iso();
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        assert!(curve_on_surfaces(&k, &c, ivl, [&s, &s], 1e-6).expect("check"));
        let off = Surface::from(OffsetSurface::new(&k, 1e-4, s.clone(), true, 0).expect("offset"));
        assert!(curve_on_surfaces(&k, &c, ivl, [&s, &off], 1e-3).expect("check"));
        assert!(!curve_on_surfaces(&k, &c, ivl, [&s, &off], 1e-5).expect("check"));
    }
}
