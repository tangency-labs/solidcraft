//! A bare B-spline net beside a local operation's edge or at its vertex (a surface with no
//! analytic label and no definition), and where it meets planes, quadrics and other nets,
//! enclosed for [`super::local_extension_reach`] (not upstream).
//!
//! A net is its own surface over its knots and, past a side the offset executive extends,
//! the extension it builds (`Continuity::Ruled`, definitions-and-certificates.md §6.1;
//! `surface/extend.rs`): each row across the side continues by the homogeneous line
//! `H_n + s D`, `D = H_v + μ H` with one `μ` for the side, so past `v = v1` the surface is
//! `H(u, v1) + s′ H_v(u, v1)` in homogeneous coordinates, `s′ = s / (1 + s μ)` the same for
//! every row: a line through `G(u, v1)` along `G_v` at each `u`. Its corner past two sides,
//! the first side's extension extended again, is `H + s′ H_u + t′ H_v + s′ t′ H_uv` at the
//! box's corner, on the rows' common parameters again. That holds where the rows across each
//! side end in one weight ratio (every polynomial net, a rational extrusion's or
//! revolution's); where they do not, the line ends at a finite point and past it the
//! extension takes the product form `S + s S_v`, or is refused and left short, so nothing
//! past such a side is enclosed here. A closed direction is not extended: the
//! net runs on round its seam. A side collapsed to a point (a pole) is not extended either,
//! and nothing past it is enclosed.
//!
//! A meeting is proved by Krawczyk's operator (definitions-and-certificates.md §4.3): a box
//! `Y` about Newton's root `ỹ` with `K(Y) = ỹ − C F(ỹ) + (I − C F′(Y)) (Y − ỹ)` strictly
//! inside it holds a root of `F` [Krawczyk 1969; Moore 1977], whatever the edge parameter
//! in the enclosures `F` is taken over. `F` is continuous across the sides and knot lines a
//! box crosses (the extension is C1 across its side; a net is at least continuous across a
//! knot line), and smooth on each piece, and `F′(Y)` is the hull of the pieces' enclosures
//! of its partials (from either side on a line): along the segment from `ỹ` to any point of
//! `Y`, `F` is the integral of its derivative, which lies in that hull wherever it exists,
//! so the mean-value form `F(y) ∈ F(ỹ) + F′(Y) (y − ỹ)` holds [Neumaier 1990, §2.1]. At most
//! [`KRAWCZYK_STEPS`] steps run.

use std::cell::Cell;

use crate::blend::fillet_section::certificate::{inverse4, nurbs_partials_over_box};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::station::Nurbs;
use crate::error::{Result, SolidError};
use crate::geom::vector3d::{sqrt, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::surface::implicit::Implicit;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};

/// The most Krawczyk steps one meeting gets.
const KRAWCZYK_STEPS: usize = 8;

/// The most Newton steps for the root a Krawczyk box is centred on.
const NEWTON_STEPS: usize = 40;

/// The grid each parameter of a net is searched on for Newton's first guess (a guess only:
/// every meeting is proved by its Krawczyk box).
const GUESS_GRID: u32 = 8;

/// An enclosed point or vector.
pub(super) type I3 = [Ivl; 3];

fn add(a: I3, b: I3) -> I3 {
    let [x, y, z] = a;
    let [p, q, r] = b;
    [x + p, y + q, z + r]
}

fn sub(a: I3, b: I3) -> I3 {
    let [x, y, z] = a;
    let [p, q, r] = b;
    [x - p, y - q, z - r]
}

fn scale(a: I3, s: Ivl) -> I3 {
    let [x, y, z] = a;
    [x * s, y * s, z * s]
}

fn dot(a: I3, b: I3) -> Ivl {
    let [x, y, z] = a;
    let [p, q, r] = b;
    x * p + y * q + z * r
}

fn point(v: Vec3) -> I3 {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

fn middle(a: I3) -> Vec3 {
    let [x, y, z] = a;
    Vec3::new(x.mid(), y.mid(), z.mid())
}

fn of_v3(v: crate::blend::fillet_section::series::V3<Ivl>) -> I3 {
    [v.x, v.y, v.z]
}

/// An upper bound on the length of every vector in `a`.
pub(super) fn length_bound(a: I3) -> f64 {
    let [x, y, z] = a;
    round_up(sqrt(round_up(
        x.mag() * x.mag() + y.mag() * y.mag() + z.mag() * z.mag(),
    )))
}

/// A net's point and first partials over a box of its parameters, enclosed.
#[derive(Debug, Clone, Copy)]
pub(super) struct Jet {
    pub(super) p: I3,
    pub(super) su: I3,
    pub(super) sv: I3,
}

impl Jet {
    fn hull(self, o: Self) -> Self {
        let h = |a: I3, b: I3| {
            let [x, y, z] = a;
            let [p, q, r] = b;
            [x.hull(p), y.hull(q), z.hull(r)]
        };
        Self {
            p: h(self.p, o.p),
            su: h(self.su, o.su),
            sv: h(self.sv, o.sv),
        }
    }
}

/// Where a part of a box lies in one direction of a net: on its knots (`[a, b]`), or past
/// the side at `at` by `d` (the extension's ruled part).
#[derive(Debug, Clone, Copy)]
enum Part {
    On(f64, f64),
    Past { at: f64, d: Ivl },
}

/// A bare B-spline net (module docs).
#[derive(Debug, Clone)]
pub(super) struct Net {
    cn: Nurbs,
    /// The homogeneous net as two polynomial ones: its points `H` and its weights `W` (as
    /// the first coordinate), whose partials give the extension's.
    h: Nurbs,
    w: Nurbs,
    /// The knots' range: `[u0, u1]`, `[v0, v1]`.
    range: [(f64, f64); 2],
    /// Whether the net is closed in u, in v.
    closed: [bool; 2],
    /// Whether each side (`BoxSide::ALL` order) is extended as enclosed here: the executive
    /// extends it, and its rows end in one weight ratio (module docs).
    extends: [bool; 4],
}

impl Net {
    /// `surface` as a bare net: a B-spline surface, or an extrusion (its net is made from
    /// its generator's exact form), with no analytic label and no definition (a blend's or
    /// sweep's net is only its approximation); `None` for any other.
    ///
    /// # Errors
    ///
    /// Those of the net's form and of the closure and side queries; a cancel.
    pub(super) fn of(k: &Kernel, surface: &Surface) -> Result<Option<Self>> {
        if !matches!(
            surface.kind(),
            SurfaceKind::BSpline(_) | SurfaceKind::SurfOfExtrusion(_)
        ) || crate::surface::implicit::AnalyticSupport::of(surface).is_some()
            || surface.procedural_blend().is_some()
            || surface.procedural_sweep().is_some()
        {
            return Ok(None);
        }
        let Some(form) = crate::blend::fillet_section::station::as_b_spline(k, surface)? else {
            return Ok(None);
        };
        let cn = match Nurbs::of(&form) {
            Ok(cn) => cn,
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            Err(_) => return Ok(None),
        };
        // the box's sides are the end knots and its boundary rows the end control rows only
        // where both ends are clamped (multiplicity `p + 1`), and the net is continuous across
        // every inner knot (multiplicity at most `p`), as the mean-value form needs
        if !(clamped_and_continuous(&cn.u, cn.p) && clamped_and_continuous(&cn.v, cn.q)) {
            return Ok(None);
        }
        let dom = surface.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (dom.min(), dom.max());
        let mut closed = [false; 2];
        for (c, param) in closed.iter_mut().zip([SurfParamType::U, SurfParamType::V]) {
            *c = surface.is_closed(k, &dom, param, None, false)?.0;
        }
        let mut extends = [false; 4];
        for (e, side) in extends.iter_mut().zip(BoxSide::ALL) {
            let direction_closed = if side.is_u() { closed[0] } else { closed[1] };
            // (as the executive's extension decides: a closed direction and a collapsed
            // side are left)
            *e = !direction_closed && surface.side_diameter(k, &dom, side)?.lower > 0.0;
        }
        let weight = |i: usize, j: usize| {
            let at = i
                .checked_mul(cn.nv)?
                .checked_add(j)?
                .checked_mul(4)?
                .checked_add(3)?;
            cn.pw.get(at).copied()
        };
        // whether every row's end weight ratio `w_(n-1) / w_n` across a side is one number
        let one_ratio = |ends: &dyn Fn(usize) -> Option<(f64, f64)>, rows: usize| {
            let mut first: Option<f64> = None;
            for r in 0..rows {
                let Some((inner, end)) = ends(r) else {
                    return false;
                };
                let ratio = inner / end;
                #[allow(clippy::float_cmp)] // one ratio, as the extension takes it
                let same = first.is_none_or(|f| f == ratio);
                if !(ratio.is_finite() && same) {
                    return false;
                }
                first = Some(ratio);
            }
            true
        };
        let (lu, lv) = (cn.nu.saturating_sub(1), cn.nv.saturating_sub(1));
        let uniform = [
            one_ratio(&|j| Some((weight(1, j)?, weight(0, j)?)), cn.nv),
            one_ratio(
                &|j| Some((weight(lu.checked_sub(1)?, j)?, weight(lu, j)?)),
                cn.nv,
            ),
            one_ratio(&|i| Some((weight(i, 1)?, weight(i, 0)?)), cn.nu),
            one_ratio(
                &|i| Some((weight(i, lv.checked_sub(1)?)?, weight(i, lv)?)),
                cn.nu,
            ),
        ];
        // past the reach of rows whose end weight ratios differ the extension takes the
        // product form, or is refused and left short: such a side is not enclosed
        for (e, &one) in extends.iter_mut().zip(&uniform) {
            *e &= one;
        }
        let polynomial = |coordinates: &dyn Fn(&[f64]) -> [f64; 3]| {
            let mut pw = Vec::new();
            for c in cn.pw.chunks_exact(4) {
                let [x, y, z] = coordinates(c);
                pw.extend_from_slice(&[x, y, z, 1.0]);
            }
            Nurbs { pw, ..cn.clone() }
        };
        let h = polynomial(&|c| match c {
            [x, y, z, _] => [*x, *y, *z],
            _ => [f64::NAN; 3],
        });
        let w = polynomial(&|c| match c {
            [_, _, _, w] => [*w, 0.0, 0.0],
            _ => [f64::NAN; 3],
        });
        Ok(Some(Self {
            cn,
            h,
            w,
            range: [(lo.x, hi.x), (lo.y, hi.y)],
            closed,
            extends,
        }))
    }

    /// The parts of `[a, b]` in direction `d` (0 for u): on the knots, past each side the
    /// executive extends, or, in a closed direction, round the seam (at most one period);
    /// `None` past a side that is not extended.
    fn parts(&self, d: usize, a: f64, b: f64) -> Option<Vec<Part>> {
        let (r0, r1) = *self.range.get(d)?;
        let closed = *self.closed.get(d)?;
        let (low, high) = (*self.extends.get(2 * d)?, *self.extends.get(2 * d + 1)?);
        if !(a <= b && a.is_finite() && b.is_finite()) {
            return None;
        }
        let mut out = Vec::new();
        if closed {
            let period = r1 - r0;
            if b - a > period {
                return None;
            }
            if a < r0 {
                out.push(Part::On(a + period, r1));
            }
            let (c0, c1) = (a.max(r0), b.min(r1));
            if c0 <= c1 {
                out.push(Part::On(c0, c1));
            }
            if b > r1 {
                out.push(Part::On(r0, b - period));
            }
            let in_range = out
                .iter()
                .all(|p| matches!(*p, Part::On(x, y) if x >= r0 && y <= r1 && x <= y));
            return in_range.then_some(out);
        }
        if a < r0 {
            if !low {
                return None;
            }
            out.push(Part::Past {
                at: r0,
                d: Ivl::new(a - r0, b.min(r0) - r0),
            });
        }
        let (c0, c1) = (a.max(r0), b.min(r1));
        if c0 <= c1 {
            out.push(Part::On(c0, c1));
        }
        if b > r1 {
            if !high {
                return None;
            }
            out.push(Part::Past {
                at: r1,
                d: Ivl::new(a.max(r1) - r1, b - r1),
            });
        }
        Some(out)
    }

    /// `cn`'s partials `a + b <= lk` over `[ua, ub] × [va, vb]` on its knots, enclosed;
    /// `None` where they are not (a weight not proved positive).
    fn partials(
        cn: &Nurbs,
        (ua, ub): (f64, f64),
        (va, vb): (f64, f64),
        lk: usize,
    ) -> Result<Option<crate::blend::fillet_section::station::Partials<Ivl>>> {
        // (continuous across every crossed knot line, a net's points are all the mean-value
        // form needs: their partials are enclosed from either side, module docs)
        match nurbs_partials_over_box(cn, ua, ub, va, vb, lk, 0) {
            Ok(p) => Ok(Some(p)),
            Err(e @ SolidError::Cancelled { .. }) => Err(e),
            Err(_) => Ok(None),
        }
    }

    /// The point and first partials of the net and its extension (module docs) over
    /// `u × v`, enclosed; `None` where they are not (past a side not extended, past two
    /// sides one of which ends its rows in more than one weight ratio, or as
    /// [`Net::partials`]).
    ///
    /// # Errors
    ///
    /// A cancel.
    pub(super) fn jet(&self, u: Ivl, v: Ivl) -> Result<Option<Jet>> {
        let (Some(us), Some(vs)) = (self.parts(0, u.lo, u.hi), self.parts(1, v.lo, v.hi)) else {
            return Ok(None);
        };
        let mut out: Option<Jet> = None;
        for &pu in &us {
            for &pv in &vs {
                let Some(jet) = self.part_jet(pu, pv)? else {
                    return Ok(None);
                };
                out = Some(out.map_or(jet, |h| h.hull(jet)));
            }
        }
        Ok(out)
    }

    /// The jet over one part of a box ([`Net::parts`]): on the knots, the net's own; past a
    /// side, or two, the extension's (module docs), from the homogeneous net's partials at
    /// the side, `H + Δu H_u + Δv H_v + Δu Δv H_uv` and its partials (`Δ` zero along a side
    /// the part is on), projected.
    fn part_jet(&self, pu: Part, pv: Part) -> Result<Option<Jet>> {
        let at = |p: Part| match p {
            Part::On(a, b) => (a, b),
            Part::Past { at, .. } => (at, at),
        };
        // how far past its side the part is
        let past = |p: Part| match p {
            Part::On(..) => None,
            Part::Past { d, .. } => Some(d),
        };
        let (du, dv) = (past(pu), past(pv));
        if du.is_none() && dv.is_none() {
            let Some(g) = Self::partials(&self.cn, at(pu), at(pv), 1)? else {
                return Ok(None);
            };
            return Ok(Some(Jet {
                p: of_v3(g.at(0, 0)),
                su: of_v3(g.at(1, 0)),
                sv: of_v3(g.at(0, 1)),
            }));
        }
        let (Some(h), Some(w)) = (
            Self::partials(&self.h, at(pu), at(pv), 2)?,
            Self::partials(&self.w, at(pu), at(pv), 2)?,
        ) else {
            return Ok(None);
        };
        let zero = Ivl::from(0.0);
        let (eu, ev) = (du.unwrap_or(zero), dv.unwrap_or(zero));
        let extend = |x: &crate::blend::fillet_section::station::Partials<Ivl>| {
            let (x, xu, xv, xuv) = (
                of_v3(x.at(0, 0)),
                of_v3(x.at(1, 0)),
                of_v3(x.at(0, 1)),
                of_v3(x.at(1, 1)),
            );
            (
                add(
                    add(x, scale(xu, eu)),
                    add(scale(xv, ev), scale(xuv, eu * ev)),
                ),
                add(xu, scale(xuv, ev)),
                add(xv, scale(xuv, eu)),
            )
        };
        let (hp, hu, hv) = extend(&h);
        let ([wp, _, _], [wu, _, _], [wv, _, _]) = extend(&w);
        if wp.lo <= 0.0 || wp.lo.is_nan() {
            return Ok(None);
        }
        let over = Ivl::from(1.0) / wp;
        let p = scale(hp, over);
        let quotient = |hd: I3, wd: Ivl| scale(sub(hd, scale(p, wd)), over);
        Ok(Some(Jet {
            p,
            su: quotient(hu, wu),
            sv: quotient(hv, wv),
        }))
    }

    /// The point and partials at `(u, v)`, as floating point numbers (for Newton's
    /// steps); `None` where [`Net::jet`] encloses nothing there.
    fn at(&self, u: f64, v: f64) -> Result<Option<(Vec3, Vec3, Vec3)>> {
        Ok(self
            .jet(Ivl::from(u), Ivl::from(v))?
            .map(|j| (middle(j.p), middle(j.su), middle(j.sv))))
    }

    /// The parameters on a grid over the knots whose point is nearest `x` (Newton's first
    /// guess only).
    fn nearest_on_grid(&self, x: Vec3) -> Result<Option<(f64, f64)>> {
        self.nearest_on_grid_by(&|p| (p - x).length())
    }

    /// The parameters on a grid over the knots whose point is nearest the line `x0 + τ u`
    /// (`u` a unit vector), and that point's `τ`: Newton's first guess for the line's meeting
    /// (the point nearest the line's foot can be where the net is tangent to it, a tangent
    /// edge's, where Newton's step is not defined).
    fn nearest_to_line(&self, x0: Vec3, u: Vec3) -> Result<Option<[f64; 3]>> {
        let off = |p: Vec3| {
            let d = p - x0;
            (d - u * d.dot(u)).length()
        };
        let Some((a, b)) = self.nearest_on_grid_by(&off)? else {
            return Ok(None);
        };
        Ok(self.at(a, b)?.map(|(p, _, _)| [a, b, (p - x0).dot(u)]))
    }

    /// The parameters on a grid over the knots where `distance` of the point is least.
    fn nearest_on_grid_by(&self, distance: &dyn Fn(Vec3) -> f64) -> Result<Option<(f64, f64)>> {
        let [(u0, u1), (v0, v1)] = self.range;
        let mut best: Option<(f64, (f64, f64))> = None;
        for i in 0..=GUESS_GRID {
            for j in 0..=GUESS_GRID {
                let u = u0 + (u1 - u0) * f64::from(i) / f64::from(GUESS_GRID);
                let v = v0 + (v1 - v0) * f64::from(j) / f64::from(GUESS_GRID);
                let Some((p, _, _)) = self.at(u, v)? else {
                    continue;
                };
                let d = distance(p);
                if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, (u, v)));
                }
            }
        }
        Ok(best.map(|(_, uv)| uv))
    }
}

impl Net {
    /// The sides (`BoxSide::ALL` order) whose boundary row lies on the plane `plane` within
    /// `tolerance`: every control point of the row within it of the plane
    /// ([`super::plane_distance`], a certified bound), so the row, in their convex hull
    /// (weights positive), is too.
    pub(super) fn rows_on(&self, plane: super::Tangent, tolerance: f64) -> Vec<usize> {
        let point = |i: usize, j: usize| {
            let at = i.checked_mul(self.cn.nv)?.checked_add(j)?.checked_mul(4)?;
            match *self.cn.pw.get(at..at.checked_add(4)?)? {
                [x, y, z, w] if w > 0.0 => Some(Vec3::new(x / w, y / w, z / w)),
                _ => None,
            }
        };
        let (lu, lv) = (self.cn.nu.saturating_sub(1), self.cn.nv.saturating_sub(1));
        let on = |row: Vec<(usize, usize)>| {
            row.into_iter().all(|(i, j)| {
                point(i, j).is_some_and(|x| super::plane_distance(x, plane) <= tolerance)
            })
        };
        [
            on((0..self.cn.nv).map(|j| (0, j)).collect()),
            on((0..self.cn.nv).map(|j| (lu, j)).collect()),
            on((0..self.cn.nu).map(|i| (i, 0)).collect()),
            on((0..self.cn.nu).map(|i| (i, lv)).collect()),
        ]
        .iter()
        .enumerate()
        .filter_map(|(side, &yes)| yes.then_some(side))
        .collect()
    }

    /// Where the boundary row of `side` (`BoxSide::ALL` order), continued along the
    /// extension past the box, crosses the plane `(origin, normal)`: an upper bound on the
    /// distance from `p` of a crossing proved by Krawczyk's test on `normal · (G − origin) = 0`
    /// along the row (module docs), or `None`.
    ///
    /// # Errors
    ///
    /// A cancel.
    pub(super) fn row_meets(
        &self,
        side: usize,
        origin: Vec3,
        normal: Vec3,
        p: Vec3,
    ) -> Result<Option<f64>> {
        let [(u0, u1), (v0, v1)] = self.range;
        // the row's fixed parameter, and whether it runs along u
        let (fixed, along_u) = match side {
            0 => (u0, false),
            1 => (u1, false),
            2 => (v0, true),
            3 => (v1, true),
            _ => return Ok(None),
        };
        let (n, o) = (point(normal), point(origin));
        let jet = |t: Ivl| -> Result<Option<Jet>> {
            if along_u {
                self.jet(t, Ivl::from(fixed))
            } else {
                self.jet(Ivl::from(fixed), t)
            }
        };
        let numbers = |t: f64| -> Result<Option<(Vec3, Vec3, Vec3)>> {
            if along_u {
                self.at(t, fixed)
            } else {
                self.at(fixed, t)
            }
        };
        let at = |y: [f64; 1]| -> Result<Linearised<1>> {
            let [t] = y;
            Ok(numbers(t)?.map(|(x, su, sv)| {
                let d = if along_u { su } else { sv };
                ([(x - origin).dot(normal)], [[normal.dot(d)]])
            }))
        };
        // Newton's start: the row's point nearest the plane, on a grid along it (the point
        // nearest the vertex can be where the row is tangent to the plane, a tangent edge's,
        // where Newton's step is not defined)
        let (r0, r1) = if along_u { (u0, u1) } else { (v0, v1) };
        let mut start: Option<(f64, f64)> = None;
        for i in 0..=GUESS_GRID {
            let t = r0 + (r1 - r0) * f64::from(i) / f64::from(GUESS_GRID);
            if let Some((x, _, _)) = numbers(t)? {
                let d = (x - origin).dot(normal).abs();
                if d.is_finite() && start.is_none_or(|(b, _)| d < b) {
                    start = Some((d, t));
                }
            }
        }
        let Some((_, t)) = start else {
            return Ok(None);
        };
        let inverse = |j: &[[f64; 1]; 1]| {
            let [[a]] = *j;
            (a.is_finite() && a != 0.0).then(|| [[1.0 / a]])
        };
        let Some(root) = newton([t], &at, &inverse)? else {
            return Ok(None);
        };
        let Some((_, j)) = at(root)? else {
            return Ok(None);
        };
        let Some(c) = inverse(&j) else {
            return Ok(None);
        };
        let value = |y: [Ivl; 1]| -> Result<Option<[Ivl; 1]>> {
            let [t] = y;
            Ok(jet(t)?.map(|g| [dot(n, sub(g.p, o))]))
        };
        let jacobian = |y: [Ivl; 1]| -> Result<Option<[[Ivl; 1]; 1]>> {
            let [t] = y;
            Ok(jet(t)?.map(|g| [[dot(n, if along_u { g.su } else { g.sv })]]))
        };
        let Some([t]) = krawczyk(root, c, &value, &jacobian)? else {
            return Ok(None);
        };
        Ok(jet(t)?.map(|g| length_bound(sub(g.p, point(p)))))
    }
}

/// Whether the full knot vector `knots` of degree `p` is clamped at both ends (its first
/// and last `p + 1` knots equal) and has no inner knot of multiplicity above `p`.
fn clamped_and_continuous(knots: &[f64], p: usize) -> bool {
    let Some(ends) = p.checked_add(1) else {
        return false;
    };
    let (Some(head), Some(tail)) = (
        knots.get(..ends),
        knots.len().checked_sub(ends).and_then(|i| knots.get(i..)),
    ) else {
        return false;
    };
    #[allow(clippy::float_cmp)] // knots repeated as stored
    let flat = |run: &[f64]| run.windows(2).all(|w| matches!(w, [a, b] if a == b));
    if !(flat(head) && flat(tail)) || knots.len() < 2 * ends {
        return false;
    }
    // an inner knot's multiplicity: a run of more than `p` equal knots
    let inner = knots
        .get(ends..knots.len().saturating_sub(ends))
        .unwrap_or(&[]);
    let mut run = 0_usize;
    let mut last = f64::NAN;
    for &x in inner {
        #[allow(clippy::float_cmp)] // knots repeated as stored
        let same = x == last;
        run = if same { run + 1 } else { 1 };
        last = x;
        if run > p {
            return false;
        }
    }
    true
}

/// The inverse of the 3 × 3 matrix of columns `c`, as rows; `None` where it is singular in
/// floating point.
fn inverse3(c: [Vec3; 3]) -> Option<[[f64; 3]; 3]> {
    let [a, b, d] = c;
    let det = a.dot(b.cross(d));
    if !(det.is_finite() && det != 0.0) {
        return None;
    }
    let rows = [b.cross(d), d.cross(a), a.cross(b)].map(|r| [r.x / det, r.y / det, r.z / det]);
    rows.iter().flatten().all(|x| x.is_finite()).then_some(rows)
}

/// Krawczyk's test about `mid` with the approximate inverse `c` (rows), `F` enclosed at a
/// point box by `value` and `F′` over a box by `jacobian` (columns): the box proved to hold
/// a root (module docs), or `None` where none is within [`KRAWCZYK_STEPS`] steps.
///
/// # Errors
///
/// Those of `value` and `jacobian` (a cancel).
fn krawczyk<const N: usize>(
    mid: [f64; N],
    c: [[f64; N]; N],
    value: &dyn Fn([Ivl; N]) -> Result<Option<[Ivl; N]>>,
    jacobian: &dyn Fn([Ivl; N]) -> Result<Option<[[Ivl; N]; N]>>,
) -> Result<Option<[Ivl; N]>> {
    let Some(f) = value(mid.map(Ivl::from))? else {
        return Ok(None);
    };
    let times = |row: &[f64; N], x: &[Ivl; N]| {
        row.iter()
            .zip(x)
            .fold(Ivl::from(0.0), |s, (&r, &v)| s + Ivl::from(r) * v)
    };
    let cf: [Ivl; N] = c.map(|row| times(&row, &f));
    // the first box: twice Newton's step from the middle, and a little more
    let mut radius = cf.map(|x| 2.0 * x.mag());
    for (r, m) in radius.iter_mut().zip(&mid) {
        *r = round_up(*r + 1e-12 * (1.0 + m.abs()));
    }
    for _ in 0..KRAWCZYK_STEPS {
        let mut y = mid.map(Ivl::from);
        for ((yi, &m), &r) in y.iter_mut().zip(&mid).zip(&radius) {
            if !r.is_finite() {
                return Ok(None);
            }
            *yi = Ivl::new(m - r, m + r);
        }
        let Some(j) = jacobian(y)? else {
            return Ok(None);
        };
        let mut k = y;
        for (i, ((ki, row), (&m, &cfi))) in
            k.iter_mut().zip(&c).zip(mid.iter().zip(&cf)).enumerate()
        {
            let mut v = Ivl::from(m) - cfi;
            for (jj, (col, (&yj, &mj))) in j.iter().zip(y.iter().zip(&mid)).enumerate() {
                // (I − C F′(Y))_ij, formed before it multiplies Y − ỹ
                let identity = Ivl::from(if i == jj { 1.0 } else { 0.0 });
                v = v + (identity - times(row, col)) * (yj - Ivl::from(mj));
            }
            *ki = v;
        }
        let inside = k.iter().zip(&y).all(|(a, b)| a.lo > b.lo && a.hi < b.hi);
        if inside {
            let mut out = k;
            for (o, b) in out.iter_mut().zip(&y) {
                *o = Ivl::new(o.lo.max(b.lo), o.hi.min(b.hi));
            }
            return Ok(Some(out));
        }
        for ((r, ki), &m) in radius.iter_mut().zip(&k).zip(&mid) {
            let reach = (ki.lo - m).abs().max((ki.hi - m).abs());
            *r = round_up(1.5 * reach.max(*r) + 1e-12 * (1.0 + m.abs()));
        }
    }
    Ok(None)
}

/// `F` at a point and its columns there, as numbers ([`newton`]).
type Linearised<const N: usize> = Option<([f64; N], [[f64; N]; N])>;

/// A square matrix's inverse (rows) from its columns, `None` where it is singular.
type Inverse<const N: usize> = dyn Fn(&[[f64; N]; N]) -> Option<[[f64; N]; N]>;

/// Newton's root of `F` from `start` (`at` gives `F` and its columns there): the root, or
/// `None` where a step is not defined or the steps do not settle.
fn newton<const N: usize>(
    start: [f64; N],
    at: &dyn Fn([f64; N]) -> Result<Linearised<N>>,
    inverse: &Inverse<N>,
) -> Result<Option<[f64; N]>> {
    let mut y = start;
    for _ in 0..NEWTON_STEPS {
        let Some((f, j)) = at(y)? else {
            return Ok(None);
        };
        let Some(c) = inverse(&j) else {
            return Ok(None);
        };
        let mut settled = true;
        for (yi, row) in y.iter_mut().zip(&c) {
            let step: f64 = row.iter().zip(&f).map(|(r, x)| r * x).sum();
            if !step.is_finite() {
                return Ok(None);
            }
            settled &= step.abs() <= 1e-14 * (1.0 + yi.abs());
            *yi -= step;
        }
        if settled {
            return Ok(Some(y));
        }
    }
    Ok(None)
}

/// The inverse (rows) of a 3 × 3 matrix given by columns.
fn inverse_of_columns3(j: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    inverse3(j.map(|[x, y, z]| Vec3::new(x, y, z)))
}

/// The inverse (rows) of a 4 × 4 matrix given by columns.
fn inverse_of_columns4(j: &[[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let mut rows = [[0.0_f64; 4]; 4];
    for (jj, col) in j.iter().enumerate() {
        for (row, &x) in rows.iter_mut().zip(col) {
            if let Some(r) = row.get_mut(jj) {
                *r = x;
            }
        }
    }
    let inv = inverse4(&rows).ok()?;
    inv.iter().flatten().all(|x| x.is_finite()).then_some(inv)
}

/// A line `x0 + τ u` (`u` a unit vector, both enclosed over a piece of an edge's
/// parameters; `mid` the same at the piece's middle, as numbers) meeting the net: the
/// enclosure of `τ` at a meeting proved for every parameter of the piece, by Krawczyk's
/// test on `G(u, v) − x0 − τ u = 0` (module docs), or `None`. `guess` carries the last root
/// found from one call to the next (Newton's start; else the grid point nearest the line). The
/// start only steers Newton to a root for Krawczyk's box: a carried start from another piece
/// that leads nowhere is replaced by the grid's once, and what is proved is decided by the box
/// alone.
///
/// # Errors
///
/// A cancel.
pub(super) fn line_meets(
    net: &Net,
    line: (I3, I3),
    mid: (Vec3, Vec3),
    guess: &Cell<Option<[f64; 3]>>,
) -> Result<Option<Ivl>> {
    let (x0, u) = line;
    let (mx0, mu) = mid;
    let at = |y: [f64; 3]| -> Result<Linearised<3>> {
        let [a, b, tau] = y;
        Ok(net.at(a, b)?.map(|(p, su, sv)| {
            let f = p - mx0 - mu * tau;
            (
                [f.x, f.y, f.z],
                [
                    [su.x, su.y, su.z],
                    [sv.x, sv.y, sv.z],
                    [-mu.x, -mu.y, -mu.z],
                ],
            )
        }))
    };
    let start = match guess.get() {
        Some(g) => g,
        None => match net.nearest_to_line(mx0, mu)? {
            Some(g) => g,
            None => return Ok(None),
        },
    };
    let mut root = newton(start, &at, &inverse_of_columns3)?;
    if root.is_none() && guess.get().is_some() {
        if let Some(g) = net.nearest_to_line(mx0, mu)? {
            root = newton(g, &at, &inverse_of_columns3)?;
        }
    }
    let Some(root) = root else {
        return Ok(None);
    };
    guess.set(Some(root));
    let Some((_, j)) = at(root)? else {
        return Ok(None);
    };
    let Some(c) = inverse_of_columns3(&j) else {
        return Ok(None);
    };
    let value = |y: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
        let [a, b, tau] = y;
        Ok(net.jet(a, b)?.map(|jet| sub(sub(jet.p, x0), scale(u, tau))))
    };
    let minus_u = scale(u, Ivl::from(-1.0));
    let jacobian = |y: [Ivl; 3]| -> Result<Option<[[Ivl; 3]; 3]>> {
        let [a, b, _] = y;
        Ok(net.jet(a, b)?.map(|jet| [jet.su, jet.sv, minus_u]))
    };
    Ok(krawczyk(root, c, &value, &jacobian)?.map(|[_, _, tau]| tau))
}

/// Where the plane `(origin, normal)` and the nets `a` and `b` meet, near the vertex `p`:
/// an upper bound on the distance from `p` of a meeting proved by Krawczyk's test on
/// `A(ua, va) − B(ub, vb) = 0`, `normal · (A − origin) = 0` (module docs), or `None`.
///
/// # Errors
///
/// A cancel.
pub(super) fn plane_and_nets_meet(
    a: &Net,
    b: &Net,
    origin: Vec3,
    normal: Vec3,
    p: Vec3,
) -> Result<Option<f64>> {
    let at = |y: [f64; 4]| -> Result<Linearised<4>> {
        let [ua, va, ub, vb] = y;
        let (Some((pa, au, av)), Some((pb, bu, bv))) = (a.at(ua, va)?, b.at(ub, vb)?) else {
            return Ok(None);
        };
        let d = pa - pb;
        let h = (pa - origin).dot(normal);
        let col = |s: Vec3, t: Vec3| [s.x - t.x, s.y - t.y, s.z - t.z, normal.dot(s)];
        Ok(Some((
            [d.x, d.y, d.z, h],
            [
                col(au, Vec3::ZERO),
                col(av, Vec3::ZERO),
                [-bu.x, -bu.y, -bu.z, 0.0],
                [-bv.x, -bv.y, -bv.z, 0.0],
            ],
        )))
    };
    let (Some(ga), Some(gb)) = (a.nearest_on_grid(p)?, b.nearest_on_grid(p)?) else {
        return Ok(None);
    };
    let Some(root) = newton([ga.0, ga.1, gb.0, gb.1], &at, &inverse_of_columns4)? else {
        return Ok(None);
    };
    let Some((_, j)) = at(root)? else {
        return Ok(None);
    };
    let Some(c) = inverse_of_columns4(&j) else {
        return Ok(None);
    };
    let n = point(normal);
    let o = point(origin);
    let value = |y: [Ivl; 4]| -> Result<Option<[Ivl; 4]>> {
        let [ua, va, ub, vb] = y;
        let (Some(ja), Some(jb)) = (a.jet(ua, va)?, b.jet(ub, vb)?) else {
            return Ok(None);
        };
        let [dx, dy, dz] = sub(ja.p, jb.p);
        Ok(Some([dx, dy, dz, dot(n, sub(ja.p, o))]))
    };
    let jacobian = |y: [Ivl; 4]| -> Result<Option<[[Ivl; 4]; 4]>> {
        let [ua, va, ub, vb] = y;
        let (Some(ja), Some(jb)) = (a.jet(ua, va)?, b.jet(ub, vb)?) else {
            return Ok(None);
        };
        let zero = Ivl::from(0.0);
        let col = |s: I3, on_plane: Ivl| {
            let [x, y, z] = s;
            [x, y, z, on_plane]
        };
        let minus = Ivl::from(-1.0);
        Ok(Some([
            col(ja.su, dot(n, ja.su)),
            col(ja.sv, dot(n, ja.sv)),
            col(scale(jb.su, minus), zero),
            col(scale(jb.sv, minus), zero),
        ]))
    };
    let Some([ua, va, _, _]) = krawczyk(root, c, &value, &jacobian)? else {
        return Ok(None);
    };
    Ok(a.jet(ua, va)?.map(|jet| length_bound(sub(jet.p, point(p)))))
}

/// The implicit function `F` of a cone (a cylinder) or sphere at `x`, and its gradient,
/// enclosed: a cone's `|P v|² − (r + t z)²` (`v = x − origin`, `z = v·a`, `P` the projection
/// off the axis), gradient `2 P v − 2 (r + t z) t a`; a sphere's `|v|² − r²`, gradient `2 v`.
/// `None` for any other kind.
fn quadric_value_and_gradient(q: &Implicit, x: I3) -> Option<(Ivl, I3)> {
    let two = Ivl::from(2.0);
    match *q {
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => {
            let a = point(axis);
            let v = sub(x, point(origin));
            let z = dot(v, a);
            let pv = sub(v, scale(a, z));
            let t = Ivl::from(tan_angle);
            let r = Ivl::from(radius) + t * z;
            Some((
                dot(pv, pv) - r * r,
                sub(scale(pv, two), scale(a, two * r * t)),
            ))
        }
        Implicit::Sphere { centre, radius } => {
            let v = sub(x, point(centre));
            let r = Ivl::from(radius);
            Some((dot(v, v) - r * r, scale(v, two)))
        }
        _ => None,
    }
}

/// The inverse (rows) of a 2 × 2 matrix given by columns.
fn inverse_of_columns2(j: &[[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let [[a, c], [b, d]] = *j;
    let det = a * d - b * c;
    if !(det.is_finite() && det != 0.0) {
        return None;
    }
    let inv = [[d / det, -b / det], [-c / det, a / det]];
    inv.iter().flatten().all(|x| x.is_finite()).then_some(inv)
}

/// Where the plane `(origin, normal)`, the quadric `q` and the net meet, near the vertex
/// `p`: an upper bound on the distance from `p` of a meeting proved by Krawczyk's test on
/// `normal · (N(u, v) − origin) = 0`, `F(N(u, v)) = 0` (`F` the quadric's implicit
/// function, [`quadric_value_and_gradient`]; module docs), or `None`.
///
/// # Errors
///
/// A cancel.
pub(super) fn plane_quadric_and_net_meet(
    net: &Net,
    q: &Implicit,
    origin: Vec3,
    normal: Vec3,
    p: Vec3,
) -> Result<Option<f64>> {
    let at = |y: [f64; 2]| -> Result<Linearised<2>> {
        let [u, v] = y;
        let Some((x, su, sv)) = net.at(u, v)? else {
            return Ok(None);
        };
        let Some((f, g)) = quadric_value_and_gradient(q, point(x)) else {
            return Ok(None);
        };
        let g = middle(g);
        Ok(Some((
            [(x - origin).dot(normal), f.mid()],
            [[normal.dot(su), g.dot(su)], [normal.dot(sv), g.dot(sv)]],
        )))
    };
    let Some(guess) = net.nearest_on_grid(p)? else {
        return Ok(None);
    };
    let Some(root) = newton([guess.0, guess.1], &at, &inverse_of_columns2)? else {
        return Ok(None);
    };
    let Some((_, j)) = at(root)? else {
        return Ok(None);
    };
    let Some(c) = inverse_of_columns2(&j) else {
        return Ok(None);
    };
    let (n, o) = (point(normal), point(origin));
    let value = |y: [Ivl; 2]| -> Result<Option<[Ivl; 2]>> {
        let [u, v] = y;
        let Some(jet) = net.jet(u, v)? else {
            return Ok(None);
        };
        Ok(quadric_value_and_gradient(q, jet.p).map(|(f, _)| [dot(n, sub(jet.p, o)), f]))
    };
    let jacobian = |y: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
        let [u, v] = y;
        let Some(jet) = net.jet(u, v)? else {
            return Ok(None);
        };
        Ok(quadric_value_and_gradient(q, jet.p).map(|(_, g)| {
            [
                [dot(n, jet.su), dot(g, jet.su)],
                [dot(n, jet.sv), dot(g, jet.sv)],
            ]
        }))
    };
    let Some([u, v]) = krawczyk(root, c, &value, &jacobian)? else {
        return Ok(None);
    };
    Ok(net.jet(u, v)?.map(|jet| length_bound(sub(jet.p, point(p)))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::nurbs::srf_basic::Surface as Patch;

    /// The quarter of the cylinder r 2 about z from (2, 0) to (0, 2), z from 0 to 3, as its
    /// bare rational net: u along the arc (degree 2), v along the axis (degree 1), both on
    /// [0, 1].
    fn quarter_cylinder() -> Surface {
        let w = core::f64::consts::FRAC_1_SQRT_2;
        let row = |x: f64, y: f64, w: f64| {
            vec![
                CPoint::new(x * w, y * w, 0.0, w),
                CPoint::new(x * w, y * w, 3.0 * w, w),
            ]
        };
        let net = vec![row(2.0, 0.0, 1.0), row(2.0, 2.0, w), row(0.0, 2.0, 1.0)];
        let patch = Patch::new(
            net,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_gw_nurb(&patch))
    }

    /// The plane x = `x` as a bilinear net over y in [−5, 5], z in [−5, 5].
    fn plane_net_at_x(x: f64) -> Surface {
        let row = |y: f64| vec![CPoint::new(x, y, -5.0, NOW), CPoint::new(x, y, 5.0, NOW)];
        let patch = Patch::new(
            vec![row(-5.0), row(5.0)],
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_gw_nurb(&patch))
    }

    fn net(k: &Kernel, s: &Surface) -> Net {
        Net::of(k, s).unwrap().unwrap()
    }

    fn holds(x: Ivl, want: f64) -> bool {
        x.lo <= want && want <= x.hi && x.hi - x.lo <= 1e-12
    }

    /// Past the top (v = 1) the quarter cylinder runs on along its rulings, on the cylinder:
    /// `H + s′ H_v` with `H_v` the ruling (its weight's rate 0), so v = 1.25 is z = 3.75 at
    /// radius 2. Past the arc's end (u = 1, at (0, 2)) it is the tangent line along −x there,
    /// y = 2, and past both its corner is their plane y = 2, above z = 3. Each point is
    /// enclosed tightly about its closed form.
    #[test]
    fn past_its_sides_a_net_runs_on_its_tangent_lines_and_past_a_corner_their_plane() {
        let k = Kernel::default();
        let n = net(&k, &quarter_cylinder());
        let up = n.jet(Ivl::from(0.5), Ivl::from(1.25)).unwrap().unwrap();
        let [x, y, z] = up.p;
        assert!(holds(z, 3.75), "{z:?}");
        let radius = (x * x + y * y).sqrt();
        assert!(
            holds(radius, 2.0) || (radius.lo - 2.0).abs() < 1e-12,
            "{radius:?}"
        );
        let along = n.jet(Ivl::from(1.5), Ivl::from(0.5)).unwrap().unwrap();
        let [x, y, z] = along.p;
        assert!(holds(y, 2.0) && holds(z, 1.5) && x.hi < 0.0, "{along:?}");
        let corner = n.jet(Ivl::from(1.5), Ivl::from(1.5)).unwrap().unwrap();
        let [x, y, z] = corner.p;
        assert!(holds(y, 2.0) && z.lo > 3.0 && x.hi < 0.0, "{corner:?}");
        // the jets agree across the side (C1): the extension's partials at v = 1 are the
        // net's there
        let on = n.jet(Ivl::from(0.5), Ivl::from(1.0)).unwrap().unwrap();
        let past = n
            .jet(Ivl::from(0.5), Ivl::new(1.0, 1.0 + 1e-9))
            .unwrap()
            .unwrap();
        for (a, b) in on.sv.iter().zip(&past.sv) {
            assert!((a.mid() - b.mid()).abs() < 1e-6, "{on:?} {past:?}");
        }
    }

    /// The point of the model's extension (module docs) nearest `x`, by Gauss–Newton on the
    /// model's parameters from `(u, v)`, and its distance from `x`.
    fn model_distance(n: &Net, x: Vec3, (mut u, mut v): (f64, f64)) -> f64 {
        for _ in 0..60 {
            let (p, su, sv) = n.at(u, v).unwrap().unwrap();
            let r = p - x;
            let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (g, h) = (su.dot(r), sv.dot(r));
            let det = a * c - b * b;
            u -= (c * g - b * h) / det;
            v -= (a * h - b * g) / det;
        }
        (n.at(u, v).unwrap().unwrap().0 - x).length()
    }

    /// The model of the extension past the sides and the corner (module docs) is the surface
    /// the executive builds: the quarter cylinder extended `Ruled` by 1.5 on every side
    /// (`extended_sides_keeping_parameters`, as the offset executive asks), its points past
    /// the top, past the arc's end and past both lie on the model's extension within
    /// rounding, though the two are on different parameters past the box (`s′ = s / (1 + s
    /// μ)`).
    #[test]
    fn the_extension_model_is_the_ruled_extension_the_executive_builds() {
        use crate::surface::extend::Continuity;
        let k = Kernel::default();
        let source = quarter_cylinder();
        let n = net(&k, &source);
        let built = source
            .extended_sides_keeping_parameters(&k, &BoxSide::ALL, Continuity::Ruled, 1.5)
            .unwrap()
            .surface;
        let dom = built.natural_uv_domain_with_nurb(&k).unwrap();
        let (lo, hi) = (dom.min(), dom.max());
        let mut checked = 0;
        for i in 0..=8 {
            for j in 0..=8 {
                let u = lo.x + (hi.x - lo.x) * f64::from(i) / 8.0;
                let v = lo.y + (hi.y - lo.y) * f64::from(j) / 8.0;
                let past_u = !(0.0..=1.0).contains(&u);
                let past_v = !(0.0..=1.0).contains(&v);
                if !(past_u || past_v) {
                    continue;
                }
                let x = built
                    .evaluate_point(&k, crate::geom::vector2d::Point2::new(u, v))
                    .unwrap();
                let x = Vec3::new(x.x, x.y, x.z);
                let d = model_distance(&n, x, (u, v));
                assert!(d < 1e-9, "({u}, {v}): {d}");
                checked += 1;
            }
        }
        // past the arc's two ends, the top and the bottom, and the four corners
        assert!(checked >= 32, "{checked}");
    }

    /// A line through the axis at z 1.5 along (1, 1, 0) / √2 meets the cylinder 2 along it;
    /// the line x = −1, z = 1.5 along +y from y = 0 meets the extension past the arc's end,
    /// the plane y = 2, 2 along it.
    #[test]
    fn a_line_meets_a_net_and_its_extension_where_the_closed_forms_say() {
        let k = Kernel::default();
        let n = net(&k, &quarter_cylinder());
        let h = core::f64::consts::FRAC_1_SQRT_2;
        let guess = Cell::new(None);
        let (x0, u) = (Vec3::new(0.0, 0.0, 1.5), Vec3::new(h, h, 0.0));
        let tau = line_meets(&n, (point(x0), point(u)), (x0, u), &guess)
            .unwrap()
            .unwrap();
        assert!(
            tau.lo <= 2.0 && 2.0 <= tau.hi && tau.hi - tau.lo < 1e-9,
            "{tau:?}"
        );
        let guess = Cell::new(None);
        let (x0, u) = (Vec3::new(-1.0, 0.0, 1.5), Vec3::new(0.0, 1.0, 0.0));
        let tau = line_meets(&n, (point(x0), point(u)), (x0, u), &guess)
            .unwrap()
            .unwrap();
        assert!(
            tau.lo <= 2.0 && 2.0 <= tau.hi && tau.hi - tau.lo < 1e-9,
            "{tau:?}"
        );
    }

    /// The plane z = 2, the quarter cylinder and the plane x = 1 as a net meet at
    /// (1, √3, 2); and the plane x = 1 with the cylinder and the sphere about (0, 0, 1.5) of
    /// radius √5 at (1, √3, 2.5). Each bound holds the distance from `p` within rounding.
    #[test]
    fn a_plane_meets_two_nets_and_a_net_and_a_quadric_where_the_closed_forms_say() {
        let k = Kernel::default();
        let (a, b) = (net(&k, &quarter_cylinder()), net(&k, &plane_net_at_x(1.0)));
        let p = Vec3::new(1.0, 1.5, 2.0);
        let want = 3.0_f64.sqrt() - 1.5;
        let found = plane_and_nets_meet(&a, &b, Vec3::new(0.0, 0.0, 2.0), Vec3::Z, p)
            .unwrap()
            .unwrap();
        assert!(found >= want && found <= want + 1e-9, "{found} vs {want}");
        let sphere = Implicit::Sphere {
            centre: Vec3::new(0.0, 0.0, 1.5),
            radius: 5.0_f64.sqrt(),
        };
        let p = Vec3::new(1.0, 1.5, 2.5);
        let found = plane_quadric_and_net_meet(&a, &sphere, Vec3::new(1.0, 0.0, 0.0), Vec3::X, p)
            .unwrap()
            .unwrap();
        assert!(found >= want && found <= want + 1e-9, "{found} vs {want}");
    }
}
