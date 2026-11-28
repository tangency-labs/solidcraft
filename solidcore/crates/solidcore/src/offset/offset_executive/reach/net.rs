//! A bare B-spline net beside a local operation's edge or at its vertex (a surface with no
//! analytic label and no definition: a B-spline surface, an extrusion or a surface of
//! revolution), and where it meets planes, quadrics and other nets,
//! enclosed for [`super::local_extension_reach`] (not upstream).
//!
//! A net is its own surface over its knots and, past a side the offset executive extends,
//! the extension it builds (`Continuity::Ruled`, definitions-and-certificates.md §6.1;
//! `surface/extend.rs`), side after side in `BoxSide::ALL`'s order, the v sides on the
//! surface the u sides left. Each row across a side continues as the line its end is tangent
//! to: past a side the surface is the set of the lines `G + λ G_u` from the side (`G` and
//! its partials taken at the side), whichever of the executive's two forms is built there,
//! the homogeneous lines `H + s (H_u + μ H)` (`s = Δu`, one `μ` along the side) or, past
//! their reach, the product form `S + s S_u`. Projected, the lines are `G + σ G_u` with
//! `σ = s / (1 + s κ)`, `κ = (p / h) C / W ≥ 0` the lines' weight rate
//! (`C = Σ N_j (ρ w_n − w_(n−1))`, `W = Σ N_j w_n` along the side, [`Lines`]), and the
//! product form is `κ = 0`: where the rows end in one weight ratio `κ` is 0 and the two are
//! one. Which is built
//! depends on the distance asked, which the reach does not know, so both are enclosed, `κ`
//! taken over its hull with 0. Past a v side the lines start from what the u sides left,
//! `E + Δv E_v` at that side (`E` the net on its knots, `G + σ G_u` past a u side): either
//! form runs each line from its point of the side along that point's tangent, only at another
//! pace along it, so the lines are one surface on other parameters (as far as the form built
//! reaches; with no form that reaches, the extension is refused and left short, and the
//! meeting found is not reached: the operation fails, it builds nothing wrong). The meeting
//! is a point, not a parameter, so these parameters serve. At a corner past a u and a
//! v side, `E_v = G_v + σ G_uv + σ_v G_u` carries the u side's parameters, so the corner
//! differs between the two forms; each is enclosed. A closed direction is not extended: the
//! net runs on round its seam. A side collapsed to a point (a pole) is not extended either,
//! and nothing past it is enclosed.
//!
//! A surface of revolution is the net of its generator turned and weighted, exactly. The
//! executive leaves its sides round the axis as they are (`SideExtension::AroundAxis`); past
//! them its end span continued is the surface turned on (a rational quadratic piece
//! continued stays on its conic, the circle each row's point sweeps), which Krawczyk's boxes
//! may straddle. A meeting is taken only where the box proved to hold it is within the
//! B-rep's tolerance of the side as built ([`Net::jet_as_built`]): one past the turn's end
//! by more is not on the executive's surfaces, and is not proved. Its partials are enclosed
//! about a point near the box ([`Net::jet_near`]): its rows' weights vary round the axis,
//! and taken about the origin the quotient's enclosure grows with the point's distance from
//! it.
//!
//! A meeting is proved by Krawczyk's operator (definitions-and-certificates.md §4.3): a box
//! `Y` about Newton's root `ỹ` with `K(Y) = ỹ − C F(ỹ) + (I − C F′(Y)) (Y − ỹ)` strictly
//! inside it holds a root of `F` [Krawczyk 1969; Moore 1977], whatever the edge parameter
//! in the enclosures `F` is taken over. `F` is continuous across the sides and knot lines a
//! box crosses (the extension is C1 across its side; a net is at least continuous across a
//! knot line), and smooth on each piece, and `F′(Y)` is the hull of the pieces' enclosures
//! of its partials (from either side on a line): along the segment from `ỹ` to any point of
//! `Y`, `F` is the integral of its derivative, which lies in that hull wherever it exists,
//! so the mean-value form `F(y) ∈ F(ỹ) + F′(Y) (y − ỹ)` holds [Neumaier 1990, §2.1]. Taken
//! over enclosures that hold both forms past a u side, `K(Y)` holds each form's own, so a box
//! it proves holds a root of whichever is built. At most
//! [`crate::geom::krawczyk::KRAWCZYK_STEPS`] steps run ([`crate::geom::krawczyk::krawczyk`]).

use super::quadric::value_and_gradient as quadric_value_and_gradient;
use std::cell::Cell;

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, knot_cuts, nurbs_partials_over_box, nurbs_partials_over_piece,
    nurbs_partials_over_piece_about, partials_hull, NurbsCurve,
};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::station::{span, Nurbs};
use crate::error::{Result, SolidError};
use crate::geom::krawczyk::{
    inverse_of_columns2, inverse_of_columns3, inverse_of_columns4, krawczyk, newton, Linearised,
    NEWTON_STEPS,
};
use crate::geom::vector3d::{sqrt, Vec3};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::surface::implicit::Implicit;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};

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

fn cross(a: I3, b: I3) -> I3 {
    let [x, y, z] = a;
    let [p, q, r] = b;
    [y * r - z * q, z * p - x * r, x * q - y * p]
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

/// Where a part of a box lies in one direction of a net: on its knots (`[a, b]`), past
/// the side at `at` by `d` (the extension's ruled part), or past a side the executive leaves
/// on the net's own end span continued (`[a, b]`, beyond the side at `at`: a revolution's
/// side round its axis, [`Net::continues`]).
#[derive(Debug, Clone, Copy)]
enum Part {
    On(f64, f64),
    Past { at: f64, d: Ivl },
    Beyond { at: f64, a: f64, b: f64 },
}

/// A bare B-spline net (module docs).
#[derive(Debug, Clone)]
pub(super) struct Net {
    cn: Nurbs,
    /// The knots' range: `[u0, u1]`, `[v0, v1]`.
    range: [(f64, f64); 2],
    /// Whether the net is closed in u, in v.
    closed: [bool; 2],
    /// Whether the executive extends each side (`BoxSide::ALL` order).
    extends: [bool; 4],
    /// Whether the surface continues past each side the executive leaves as the net's own
    /// end span continued (`BoxSide::ALL` order): a revolution's sides round its axis, where
    /// that span is its generator turned on (a rational quadratic piece continued stays on
    /// its conic, the circle the row sweeps). Only what is within [`Net::tolerance`] of the
    /// side as built is taken there ([`Net::jet_as_built`]).
    continues: [bool; 4],
    /// How far from the surface as built a point past a continued side may be: the B-rep's
    /// tolerance, within which the mate and the intersection identify points.
    tolerance: f64,
    /// The weights of the homogeneous lines past each u side (low, high), from which the
    /// extension's parameters there follow (module docs); `None` where they are not proved
    /// positive.
    lines: [Option<Lines>; 2],
    /// The signed distance the surface lies off the net along the net's unit normal, for an
    /// offset surface of the net (`O = G + d n`, `n = G_u × G_v / |G_u × G_v|`, the side
    /// `OffsetSurface` moves a positive distance to); `None` for the net itself. The
    /// executive extends an offset surface as the offset of its extended base
    /// (`surface/extend.rs`, `offset_extended`), so past a side `O` is the offset of the
    /// extension above ([`Net::offset_jet`]).
    offset: Option<f64>,
    /// The points of the stations ([`Net::nearest_on_knots`]) the first time they are asked
    /// for: the same for every point looked for, so enclosed once, not once per piece of an
    /// edge (a thread's flank winds many turns: its stations are thousands, and on every
    /// piece of every edge they were enclosed again).
    stations: std::sync::OnceLock<Vec<((f64, f64), Vec3)>>,
}

/// The homogeneous lines `H + s (H_u + μ H)` the executive continues the rows by past a u
/// side (`surface/extend.rs`, Ruled), as their weight's rate against the side's weight:
/// `κ = (p / h) C / W ≥ 0` along the side, with `C = Σ N_j (ρ w_n − w_(n−1))` and
/// `W = Σ N_j w_n` the curve `(C, W)` over the v knots (`ρ` the rows' largest end weight
/// ratio, `h` the end span, `p` the degree in u).
#[derive(Debug, Clone)]
struct Lines {
    curve: NurbsCurve,
    scale: Ivl,
}

/// Which extension past a u side a jet encloses ([`Net::jet`]): either one the executive
/// may build, or the homogeneous lines alone (for Newton's steps and the tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Either,
    Lines,
    /// The product form alone (`κ = 0`), for the tests.
    #[cfg(test)]
    Product,
}

impl Net {
    /// `surface` as a bare net: a B-spline surface, an extrusion (its net is made from its
    /// generator's exact form) or a surface of revolution (its net is its generator turned
    /// and weighted, exactly), with no analytic label and no definition (a blend's or sweep's
    /// net is only its approximation); or an offset surface of such a net ([`Net::offset`]);
    /// `None` for any other. A revolution's sides round its
    /// axis are not extended, as the executive leaves them (`SideExtension::AroundAxis`):
    /// past them the net's end span continued is enclosed only within `tolerance` of the side
    /// ([`Net::continues`]).
    ///
    /// # Errors
    ///
    /// Those of the net's form and of the closure and side queries; a cancel.
    pub(super) fn of(k: &Kernel, surface: &Surface, tolerance: f64) -> Result<Option<Self>> {
        // an offset surface of a bare net: the net, moved along its normal (an offset of an
        // offset, which upstream evaluates by adding the distances, is not taken)
        if let SurfaceKind::OffsetSurface(o) = surface.kind() {
            let d = o.offset_distance();
            let base = o.base_surface();
            if !d.is_finite() || matches!(base.kind(), SurfaceKind::OffsetSurface(_)) {
                return Ok(None);
            }
            // (the net only bounds the reach; the offset surface is built by `create_offset_surface`)
            let Some(net) = Self::of_bare(k, base, tolerance, true)? else {
                return Ok(None);
            };
            // the offset is a surface only where the base's normal runs on across its knots
            if !offset_is_continuous(&net.cn, d, tolerance) {
                return Ok(None);
            }
            return Ok(Some(Self {
                offset: Some(d),
                ..net
            }));
        }
        Self::of_bare(k, surface, tolerance, false)
    }

    /// [`Net::of`] for a surface that is no offset surface. A swept wall's definition
    /// ([`Surface::procedural_sweep`]) rides on its net, which is only within the
    /// definition's bound of the swept surface, so the net itself stands for a wall only
    /// where the net is what is used: `as_offset_base`, for the base of an offset surface,
    /// which the executive offsets and extends as that net (the offset keeps no definition)
    /// whatever the definition beside it says.
    ///
    /// # Errors
    ///
    /// As [`Net::of`].
    fn of_bare(
        k: &Kernel,
        surface: &Surface,
        tolerance: f64,
        as_offset_base: bool,
    ) -> Result<Option<Self>> {
        if !matches!(
            surface.kind(),
            SurfaceKind::BSpline(_)
                | SurfaceKind::SurfOfExtrusion(_)
                | SurfaceKind::SurfOfRevolution(_)
        ) || crate::surface::implicit::AnalyticSupport::of(surface).is_some()
            || surface.procedural_blend().is_some()
            || (!as_offset_base && surface.procedural_sweep().is_some())
        {
            return Ok(None);
        }
        let Some(form) = crate::blend::fillet_section::station::as_b_spline(k, surface)? else {
            return Ok(None);
        };
        let cn = match Nurbs::of(&form) {
            Ok(cn) => cn,
            Err(e @ (SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })) => {
                return Err(e)
            }
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
        let mut continues = [false; 4];
        for ((e, c), side) in extends.iter_mut().zip(&mut continues).zip(BoxSide::ALL) {
            let direction_closed = if side.is_u() { closed[0] } else { closed[1] };
            if direction_closed || surface.side_diameter(k, &dom, side)?.lower <= 0.0 {
                continue;
            }
            // (as the executive's extension decides: a closed direction, a collapsed side
            // and a revolution's side round its axis are left; the last continues as its
            // net's end span, round the axis)
            if crate::surface::extend::round_the_axis(surface, side) == Some(true) {
                *c = true;
            } else {
                *e = true;
            }
        }
        let lu = cn.nu.saturating_sub(1);
        let lines = [
            Self::lines(&cn, 0, 1, cn.u.get(cn.p + 1).map(|k| k - lo.x)),
            Self::lines(
                &cn,
                lu,
                lu.saturating_sub(1),
                cn.u.len()
                    .checked_sub(cn.p + 2)
                    .and_then(|i| cn.u.get(i))
                    .map(|k| hi.x - k),
            ),
        ];
        Ok(Some(Self {
            cn,
            range: [(lo.x, hi.x), (lo.y, hi.y)],
            closed,
            extends,
            continues,
            tolerance: if tolerance.is_finite() {
                tolerance.max(0.0)
            } else {
                0.0
            },
            lines,
            offset: None,
            stations: std::sync::OnceLock::new(),
        }))
    }

    /// The lines past the u side whose rows end at column `end`, the column before it
    /// `inner`, its end span `h` long ([`Lines`]), with `ρ` and `ρ w_n − w_(n−1)` in the
    /// floating point the executive forms them in (`extend.rs`, `ruled`); `None` where a
    /// weight or `h` is not positive.
    fn lines(cn: &Nurbs, end: usize, inner: usize, h: Option<f64>) -> Option<Lines> {
        let weight = |i: usize, j: usize| -> Option<f64> {
            let at = i
                .checked_mul(cn.nv)?
                .checked_add(j)?
                .checked_mul(4)?
                .checked_add(3)?;
            cn.pw.get(at).copied().filter(|w| *w > 0.0)
        };
        let ends = (0..cn.nv)
            .map(|j| Some((weight(end, j)?, weight(inner, j)?)))
            .collect::<Option<Vec<_>>>()?;
        let rho = ends
            .iter()
            .fold(f64::NEG_INFINITY, |m, (wn, wn1)| m.max(wn1 / wn));
        let h = h.filter(|h| *h > 0.0)?;
        if !rho.is_finite() {
            return None;
        }
        #[allow(clippy::cast_precision_loss)] // a degree
        let p = cn.p as f64;
        let pw = ends
            .iter()
            .flat_map(|&(wn, wn1)| [rho * wn - wn1, wn, 0.0, 1.0])
            .collect();
        Some(Lines {
            curve: NurbsCurve {
                p: cn.q,
                n: cn.nv,
                u: cn.v.clone(),
                pw,
            },
            scale: Ivl::from(p) / Ivl::from(h),
        })
    }

    /// `κ` and `κ_v` of the lines past the u side `side` (`BoxSide::ALL` order) over `v`
    /// ([`Lines`]): the lines' own for [`Form::Lines`], and hulled with 0 for
    /// [`Form::Either`], as `κ = 0` is the executive's other extension there (module docs);
    /// `None` where they are not enclosed.
    fn kappa(&self, side: usize, v: Ivl, form: Form) -> Result<Option<(Ivl, Ivl)>> {
        let Some(lines) = self.lines.get(side).and_then(Option::as_ref) else {
            return Ok(None);
        };
        let ders = match curve_derivs_over(&lines.curve, v, 1, None) {
            Ok(d) => d,
            Err(e @ (SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })) => {
                return Err(e)
            }
            Err(_) => return Ok(None),
        };
        let (Some(d0), Some(d1)) = (ders.first(), ders.get(1)) else {
            return Ok(None);
        };
        let (c, w, cv, wv) = (d0.x, d0.y, d1.x, d1.y);
        if w.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Ok(None);
        }
        let kappa = lines.scale * c / w;
        let rate = lines.scale * (cv * w - c * wv) / (w * w);
        let with_zero = |x: Ivl| match form {
            Form::Lines => x,
            Form::Either => x.hull(Ivl::from(0.0)),
            #[cfg(test)]
            Form::Product => Ivl::from(0.0),
        };
        let (kappa, rate) = (with_zero(kappa), with_zero(rate));
        let finite = |x: Ivl| x.lo.is_finite() && x.hi.is_finite();
        Ok((finite(kappa) && finite(rate)).then_some((kappa, rate)))
    }

    /// `κ`, `κ_v` and `κ_vv` of the lines past the u side `side` over `v`, as
    /// [`Net::kappa`] takes the first two: with `(C, W)` the lines' curve and `c` its scale,
    /// `κ = c C / W`, `κ_v = c (C' W − C W') / W²` and
    /// `κ_vv = c ((C'' W − C W'') / W² − 2 W' (C' W − C W') / W³)`, each hulled with 0 for
    /// [`Form::Either`] (the product form's `κ` is 0 at every `v`, and so are its
    /// derivatives).
    ///
    /// # Errors
    ///
    /// A cancel.
    fn kappa2(&self, side: usize, v: Ivl, form: Form) -> Result<Option<[Ivl; 3]>> {
        let Some(lines) = self.lines.get(side).and_then(Option::as_ref) else {
            return Ok(None);
        };
        let ders = match curve_derivs_over(&lines.curve, v, 2, None) {
            Ok(d) => d,
            Err(e @ (SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })) => {
                return Err(e)
            }
            Err(_) => return Ok(None),
        };
        let (Some(d0), Some(d1), Some(d2)) = (ders.first(), ders.get(1), ders.get(2)) else {
            return Ok(None);
        };
        let (c, w, cv, wv, cvv, wvv) = (d0.x, d0.y, d1.x, d1.y, d2.x, d2.y);
        if w.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Ok(None);
        }
        let numerator = cv * w - c * wv;
        let kappa = lines.scale * c / w;
        let rate = lines.scale * numerator / (w * w);
        let rate2 = lines.scale
            * ((cvv * w - c * wvv) / (w * w) - Ivl::from(2.0) * wv * numerator / (w * w * w));
        let with_zero = |x: Ivl| match form {
            Form::Lines => x,
            Form::Either => x.hull(Ivl::from(0.0)),
            #[cfg(test)]
            Form::Product => Ivl::from(0.0),
        };
        let out = [kappa, rate, rate2].map(with_zero);
        Ok(out
            .iter()
            .all(|x| x.lo.is_finite() && x.hi.is_finite())
            .then_some(out))
    }

    /// The parts of `[a, b]` in direction `d` (0 for u): on the knots, past each side the
    /// executive extends, or, in a closed direction, round the seam (at most one period, from
    /// wherever it starts: moved by whole turns onto the knots, each part past an end by one
    /// more); `None` past a side that is not extended.
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
            if period.is_nan() || period <= 0.0 || b - a > period {
                return None;
            }
            // a box starting off the knots moved by whole turns to start on them (a
            // parameter and the one a period on are one point), the move rounded outward:
            // Newton's steps from a root carried past the seam can land a period or more away
            let (a, b) = if r0 <= a && a < r1 {
                (a, b)
            } else {
                let turns = Ivl::from(((a - r0) / period).floor());
                let moved = Ivl::new(a, b) - turns * (Ivl::from(r1) - Ivl::from(r0));
                (moved.lo, moved.hi)
            };
            if !(a <= b && a.is_finite() && b.is_finite()) {
                return None;
            }
            // each part of the box past an end is taken round the seam by one period, that
            // part alone: a box wholly past an end (a point among them, as Newton's steps
            // and Krawczyk's middle may be) is its own copy, not the whole way from the
            // seam to it
            // the period as an interval, so each wrapped end is rounded outward
            let wrap = Ivl::from(r1) - Ivl::from(r0);
            if a < r0 {
                let hi = if b < r0 {
                    (Ivl::from(b) + wrap).hi.min(r1)
                } else {
                    r1
                };
                out.push(Part::On((Ivl::from(a) + wrap).lo.min(hi), hi));
            }
            let (c0, c1) = (a.max(r0), b.min(r1));
            if c0 <= c1 {
                out.push(Part::On(c0, c1));
            }
            if b > r1 {
                let lo = if a > r1 {
                    (Ivl::from(a) - wrap).lo.max(r0)
                } else {
                    r0
                };
                out.push(Part::On(lo, (Ivl::from(b) - wrap).hi.max(lo)));
            }
            let in_range = out
                .iter()
                .all(|p| matches!(*p, Part::On(x, y) if x >= r0 && y <= r1 && x <= y));
            return in_range.then_some(out);
        }
        let (low_on, high_on) = (*self.continues.get(2 * d)?, *self.continues.get(2 * d + 1)?);
        if a < r0 {
            if low {
                out.push(Part::Past {
                    at: r0,
                    d: Ivl::new(a - r0, b.min(r0) - r0),
                });
            } else if low_on {
                // (from the side on: the part's partials bound the way back to it)
                out.push(Part::Beyond { at: r0, a, b: r0 });
            } else {
                return None;
            }
        }
        let (c0, c1) = (a.max(r0), b.min(r1));
        if c0 <= c1 {
            out.push(Part::On(c0, c1));
        }
        if b > r1 {
            if high {
                out.push(Part::Past {
                    at: r1,
                    d: Ivl::new(a.max(r1) - r1, b - r1),
                });
            } else if high_on {
                out.push(Part::Beyond { at: r1, a: r1, b });
            } else {
                return None;
            }
        }
        Some(out)
    }

    /// `cn`'s partials `a + b <= lk` over `[ua, ub] × [va, vb]` on its knots (or past an end,
    /// on the end span continued), enclosed; those of `S − about` where `about` is given
    /// ([`nurbs_partials_over_piece_about`]); `None` where they are not (a weight not proved
    /// positive).
    fn partials(
        cn: &Nurbs,
        (ua, ub): (f64, f64),
        (va, vb): (f64, f64),
        lk: usize,
        about: Option<Vec3>,
    ) -> Result<Option<crate::blend::fillet_section::station::Partials<Ivl>>> {
        // (continuous across every crossed knot line, a net's points are all the mean-value
        // form needs: their partials are enclosed from either side, module docs)
        let found = if about.is_none()
            && Self::within(&cn.u, cn.p, cn.nu, ua, ub)
            && Self::within(&cn.v, cn.q, cn.nv, va, vb)
        {
            nurbs_partials_over_box(cn, ua, ub, va, vb, lk, 0)
        } else {
            // past an end the end span's polynomial continued ([`Part::Beyond`]), or about a
            // point: each direction's pieces (cut at the knot lines as
            // `nurbs_partials_over_box` cuts them) taken in turn and hulled
            let (Some(us), Some(vs)) = (
                Self::pieces(&cn.u, cn.p, cn.nu, ua, ub),
                Self::pieces(&cn.v, cn.q, cn.nv, va, vb),
            ) else {
                return Ok(None);
            };
            let mut out: Option<crate::blend::fillet_section::station::Partials<Ivl>> = None;
            for &(a0, a1, ku) in &us {
                for &(b0, b1, kv) in &vs {
                    let found = match about {
                        Some(o) => nurbs_partials_over_piece_about(
                            cn,
                            ku,
                            kv,
                            a0,
                            a1,
                            b0,
                            b1,
                            lk,
                            [o.x, o.y, o.z],
                        ),
                        None => nurbs_partials_over_piece(cn, ku, kv, a0, a1, b0, b1, lk),
                    };
                    let p = match found {
                        Ok(p) => p,
                        Err(e @ (SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })) => {
                            return Err(e)
                        }
                        Err(_) => return Ok(None),
                    };
                    out = Some(match out {
                        None => p,
                        Some(h) => partials_hull(h, &p, lk),
                    });
                }
            }
            return Ok(out);
        };
        match found {
            Ok(p) => Ok(Some(p)),
            Err(e @ (SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })) => Err(e),
            Err(_) => Ok(None),
        }
    }

    /// Whether `[lo, hi]` lies on the knots `knots` (degree `p`, `n` control points).
    fn within(knots: &[f64], p: usize, n: usize, lo: f64, hi: f64) -> bool {
        let (Some(&first), Some(&last)) = (knots.get(p), knots.get(n)) else {
            return false;
        };
        lo >= first && hi <= last
    }

    /// The polynomial pieces `[lo, hi]` lies on, with their spans: cut at the knot lines on
    /// the knots, the end span past an end; `None` for a range across an end (a box's parts
    /// never are, [`Net::parts`]) or not ordered.
    fn pieces(
        knots: &[f64],
        p: usize,
        n: usize,
        lo: f64,
        hi: f64,
    ) -> Option<Vec<(f64, f64, usize)>> {
        let (&first, &last) = (knots.get(p)?, knots.get(n)?);
        if lo.is_nan() || hi.is_nan() || lo > hi {
            return None;
        }
        if lo >= first && hi <= last {
            return knot_cuts(knots, p, n, lo, hi, 0).ok();
        }
        if lo >= last {
            return Some(vec![(lo, hi, span(knots, p, n, last))]);
        }
        if hi <= first {
            return Some(vec![(lo, hi, span(knots, p, n, first))]);
        }
        None
    }

    /// The point and first partials of the net and its extension (module docs) over
    /// `u × v`, enclosed for either extension the executive may build past a u side;
    /// `None` where they are not (past a side not extended, or as [`Net::partials`] and
    /// [`Net::kappa`]).
    ///
    /// # Errors
    ///
    /// A cancel.
    pub(super) fn jet(&self, u: Ivl, v: Ivl) -> Result<Option<Jet>> {
        self.jet_as(u, v, Form::Either, None)
    }

    /// [`Net::jet`], its partials enclosed about `near` (a point near the box: Krawczyk's
    /// middle), where the net's weights vary round an axis ([`Net::centred`]): the same
    /// surface, its partials far narrower than when taken about the origin.
    ///
    /// # Errors
    ///
    /// A cancel.
    pub(super) fn jet_near(&self, u: Ivl, v: Ivl, near: Vec3) -> Result<Option<Jet>> {
        self.jet_as(u, v, Form::Either, self.centred(near))
    }

    /// The point a revolution's partials are enclosed about ([`Net::jet_near`]): `near`
    /// for a net that continues round an axis ([`Net::continues`]), whose rows' weights vary
    /// round it and whose points lie far from the origin along it, where the quotient's
    /// enclosure of a partial takes the point's distance from the origin times its weights'
    /// spread; `None` (about the origin, as before) for every other net.
    fn centred(&self, near: Vec3) -> Option<Vec3> {
        (self.continues.iter().any(|&c| c)
            && near.x.is_finite()
            && near.y.is_finite()
            && near.z.is_finite())
        .then_some(near)
    }

    /// [`Net::jet`] for the extensions past the u sides `form` names.
    fn jet_as(&self, u: Ivl, v: Ivl, form: Form, about: Option<Vec3>) -> Result<Option<Jet>> {
        let (Some(us), Some(vs)) = (self.parts(0, u.lo, u.hi), self.parts(1, v.lo, v.hi)) else {
            return Ok(None);
        };
        let mut out: Option<Jet> = None;
        for &pu in &us {
            for &pv in &vs {
                let Some(jet) = self.part_jet(pu, pv, form, about)? else {
                    return Ok(None);
                };
                out = Some(out.map_or(jet, |h| h.hull(jet)));
            }
        }
        Ok(out)
    }

    /// The jet over one part of a box ([`Net::parts`]): on the knots, the net's own; past a
    /// u side, `E = G + σ G_u` with `σ = Δu / (1 + |Δu| κ)` ([`Net::kappa`]); past a v side,
    /// `E + Δv E_v` at that side (module docs), each with its partials, from the net's
    /// partials at the sides (`Δ` zero along a side the part is on).
    fn part_jet(&self, pu: Part, pv: Part, form: Form, about: Option<Vec3>) -> Result<Option<Jet>> {
        self.part_jet_of(pu, pv, form, about, self.offset)
    }

    /// [`Net::part_jet`] of the net moved by `offset` ([`Net::offset`]; `None` for the net
    /// itself, whatever its own offset).
    fn part_jet_of(
        &self,
        pu: Part,
        pv: Part,
        form: Form,
        about: Option<Vec3>,
        offset: Option<f64>,
    ) -> Result<Option<Jet>> {
        if let Some(d) = offset {
            return self.offset_jet(pu, pv, form, about, d);
        }
        let at = |p: Part| match p {
            Part::On(a, b) | Part::Beyond { a, b, .. } => (a, b),
            Part::Past { at, .. } => (at, at),
        };
        // how far past its side the part is, and that side's index (`BoxSide::ALL`)
        let past = |p: Part, d: usize| match p {
            Part::On(..) | Part::Beyond { .. } => None,
            Part::Past { at, d: by } => {
                let low = self.range.get(d).is_some_and(|&(r0, _)| at <= r0);
                Some((by, if low { 2 * d } else { 2 * d + 1 }))
            }
        };
        let (du, dv) = (past(pu, 0), past(pv, 1));
        let lk = if du.is_none() && dv.is_none() { 1 } else { 2 };
        let Some(g) = Self::partials(&self.cn, at(pu), at(pv), lk, about)? else {
            return Ok(None);
        };
        // (the point taken back from `S − about`)
        let x = match about {
            Some(o) => add(of_v3(g.at(0, 0)), point(o)),
            None => of_v3(g.at(0, 0)),
        };
        let (xu, xv) = (of_v3(g.at(1, 0)), of_v3(g.at(0, 1)));
        if lk == 1 {
            let jet = Jet {
                p: x,
                su: xu,
                sv: xv,
            };
            return Ok(Some(jet));
        }
        let xuv = of_v3(g.at(1, 1));
        let (zero, one) = (Ivl::from(0.0), Ivl::from(1.0));
        // σ and its partials `σ_u`, `σ_v`, `σ_uv`: (0, 1, 0, 0) on the u knots
        let (sigma, sigma_u, sigma_v, sigma_uv) = match du {
            None => (zero, one, zero, zero),
            Some((d, side)) => {
                let (v0, v1) = at(pv);
                let Some((kappa, rate)) = self.kappa(side, Ivl::new(v0, v1), form)? else {
                    return Ok(None);
                };
                // `|Δu|`: past the low side Δu ≤ 0
                let far = if side == 0 { zero - d } else { d };
                let q = one + far * kappa;
                if q.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                    return Ok(None);
                }
                let q2 = q * q;
                (
                    d / q,
                    one / q2,
                    zero - d * far * rate / q2,
                    zero - Ivl::from(2.0) * far * rate / (q2 * q),
                )
            }
        };
        let ev = dv.map_or(zero, |(d, _)| d);
        // the extension past the u side (or the net, on its knots) and its partials at v
        let e = add(x, scale(xu, sigma));
        let eu = scale(xu, sigma_u);
        let ev_ = add(add(xv, scale(xuv, sigma)), scale(xu, sigma_v));
        let euv = add(scale(xuv, sigma_u), scale(xu, sigma_uv));
        let jet = Jet {
            p: add(e, scale(ev_, ev)),
            su: add(eu, scale(euv, ev)),
            sv: ev_,
        };
        Ok(Some(jet))
    }

    /// The jet of the offset `O = E + d n` over one part of a box ([`Net::offset`]), `E` the
    /// net or its extension there as [`Net::part_jet`] takes it and `n = N / |N|`,
    /// `N = E_u × E_v`: `O_u = E_u + d n_u` with `n_u = (N_u − n (n·N_u)) / |N|` and
    /// `N_u = E_uu × E_v + E_u × E_uv` (the same in v), so `E`'s second partials are enclosed
    /// ([`Net::second_order`]). `None` where they are not, or where `|N|` is not proved
    /// positive (a pole of the net, where the offset has no normal to move along).
    fn offset_jet(
        &self,
        pu: Part,
        pv: Part,
        form: Form,
        about: Option<Vec3>,
        d: f64,
    ) -> Result<Option<Jet>> {
        let Some([x, xu, xv, xuu, xuv, xvv]) = self.second_order(pu, pv, form, about)? else {
            return Ok(None);
        };
        let n = cross(xu, xv);
        let length = dot(n, n).sqrt();
        if !(length.lo > 0.0 && length.hi.is_finite()) {
            return Ok(None);
        }
        let unit = scale(n, Ivl::from(1.0) / length);
        let nu = add(cross(xuu, xv), cross(xu, xuv));
        let nv = add(cross(xuv, xv), cross(xu, xvv));
        // the unit normal's partials: the normal's, less their part along it, over its length
        let along = |m: I3| scale(sub(m, scale(unit, dot(unit, m))), Ivl::from(1.0) / length);
        let d = Ivl::from(d);
        Ok(Some(Jet {
            p: add(x, scale(unit, d)),
            su: add(xu, scale(along(nu), d)),
            sv: add(xv, scale(along(nv), d)),
        }))
    }

    /// The point and the first and second partials `[E, E_u, E_v, E_uu, E_uv, E_vv]` of the
    /// net and its extension over one part of a box ([`Net::part_jet`]'s `E`, the point
    /// taken back from `E − about`). On the knots (or beyond a continued side) they are the
    /// net's own. Past a u side, `E = G + σ G_u` from the net's partials `G` at the side, with
    /// `σ = s / q`, `q = 1 + |s| κ(v)` (`s = Δu`, [`Net::kappa`] with its first two
    /// derivatives), whose partials are, `ε` the sign of `s` (`|s| = ε s`):
    ///
    /// ```text
    /// σ_s = 1 / q²,                    σ_ss = −2 ε κ / q³,
    /// σ_v = −s |s| κ_v / q²,           σ_sv = −2 |s| κ_v / q³,
    /// σ_vv = −s |s| κ_vv / q² + 2 s |s|² κ_v² / q³,
    /// σ_ssv = −2 ε κ_v / q³ + 6 ε |s| κ κ_v / q⁴,
    /// ```
    ///
    /// so `E_s = σ_s G_u`, `E_v = G_v + σ_v G_u + σ G_uv`, `E_ss = σ_ss G_u`,
    /// `E_sv = σ_sv G_u + σ_s G_uv` and `E_vv = G_vv + σ_vv G_u + 2 σ_v G_uv + σ G_uvv`. Past
    /// a v side, `F = E + t E_v` at that side (`t = Δv`): `F_u = E_u + t E_uv`, `F_t = E_v`,
    /// `F_uu = E_uu + t E_uuv`, `F_ut = E_uv`, `F_tt = 0`, with `E_uuv` the net's own on the
    /// u knots and `σ_ssv G_u + σ_ss G_uv` past a u side.
    ///
    /// # Errors
    ///
    /// A cancel.
    #[allow(clippy::many_single_char_names)] // the partials' names
    fn second_order(
        &self,
        pu: Part,
        pv: Part,
        form: Form,
        about: Option<Vec3>,
    ) -> Result<Option<[I3; 6]>> {
        let at = |p: Part| match p {
            Part::On(a, b) | Part::Beyond { a, b, .. } => (a, b),
            Part::Past { at, .. } => (at, at),
        };
        // how far past its side the part is, and that side's index (`BoxSide::ALL`)
        let past = |p: Part, d: usize| match p {
            Part::On(..) | Part::Beyond { .. } => None,
            Part::Past { at, d: by } => {
                let low = self.range.get(d).is_some_and(|&(r0, _)| at <= r0);
                Some((by, if low { 2 * d } else { 2 * d + 1 }))
            }
        };
        let (du, dv) = (past(pu, 0), past(pv, 1));
        // the third partials `G_uvv` (past a u side) and `G_uuv` (past a v side) are asked
        // only where a part is past a side
        let lk = if du.is_none() && dv.is_none() { 2 } else { 3 };
        let Some(g) = Self::partials(&self.cn, at(pu), at(pv), lk, about)? else {
            return Ok(None);
        };
        let x = match about {
            Some(o) => add(of_v3(g.at(0, 0)), point(o)),
            None => of_v3(g.at(0, 0)),
        };
        let [xu, xv, xuu, xuv, xvv] =
            [(1, 0), (0, 1), (2, 0), (1, 1), (0, 2)].map(|(a, b)| of_v3(g.at(a, b)));
        let (xuuv, xuvv) = (of_v3(g.at(2, 1)), of_v3(g.at(1, 2)));
        let (zero, one) = (Ivl::from(0.0), Ivl::from(1.0));
        // `E` and its partials, with `E_uuv` for a part past a v side
        let (e, eu, ev, euu, euv, evv, euuv) = match du {
            None => (x, xu, xv, xuu, xuv, xvv, xuuv),
            Some((s, side)) => {
                let (v0, v1) = at(pv);
                let Some([kappa, rate, rate2]) = self.kappa2(side, Ivl::new(v0, v1), form)? else {
                    return Ok(None);
                };
                // `ε` and `|s|`: past the low side `s ≤ 0`
                let sign = if side == 0 { Ivl::from(-1.0) } else { one };
                let far = sign * s;
                let q = one + far * kappa;
                if q.lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                    return Ok(None);
                }
                let (q2, two) = (q * q, Ivl::from(2.0));
                let q3 = q2 * q;
                let sigma = s / q;
                let sigma_s = one / q2;
                let sigma_v = zero - s * far * rate / q2;
                let sigma_ss = zero - two * sign * kappa / q3;
                let sigma_sv = zero - two * far * rate / q3;
                let sigma_vv = zero - s * far * rate2 / q2 + two * s * far * far * rate * rate / q3;
                let sigma_ssv = zero - two * sign * rate / q3
                    + Ivl::from(6.0) * sign * far * kappa * rate / (q3 * q);
                (
                    add(x, scale(xu, sigma)),
                    scale(xu, sigma_s),
                    add(add(xv, scale(xu, sigma_v)), scale(xuv, sigma)),
                    scale(xu, sigma_ss),
                    add(scale(xu, sigma_sv), scale(xuv, sigma_s)),
                    add(
                        add(add(xvv, scale(xu, sigma_vv)), scale(xuv, two * sigma_v)),
                        scale(xuvv, sigma),
                    ),
                    add(scale(xu, sigma_ssv), scale(xuv, sigma_ss)),
                )
            }
        };
        Ok(Some(match dv {
            None => [e, eu, ev, euu, euv, evv],
            Some((t, _)) => [
                add(e, scale(ev, t)),
                add(eu, scale(euv, t)),
                ev,
                add(euu, scale(euuv, t)),
                euv,
                [zero; 3],
            ],
        }))
    }

    /// [`Net::jet`] over a box proved to hold a meeting, where that meeting must be on the
    /// surface as built: `None` also where a part of the box past a continued side
    /// ([`Part::Beyond`]) is not within [`Net::tolerance`] of the side ([`Net::near_as_built`]).
    /// Krawczyk's boxes are taken over the continuation (the net's end span continued is the
    /// surface's own, a revolution turned on), and only the box it proves is asked to be
    /// built: a meeting past the side by more than the tolerance is one the executive's
    /// surfaces do not hold.
    ///
    /// # Errors
    ///
    /// A cancel.
    pub(super) fn jet_as_built(&self, u: Ivl, v: Ivl, near: Vec3) -> Result<Option<Jet>> {
        let about = self.centred(near);
        let (Some(us), Some(vs)) = (self.parts(0, u.lo, u.hi), self.parts(1, v.lo, v.hi)) else {
            return Ok(None);
        };
        let mut out: Option<Jet> = None;
        for &pu in &us {
            for &pv in &vs {
                let Some(jet) = self.part_jet(pu, pv, Form::Either, about)? else {
                    return Ok(None);
                };
                if !self.near_as_built(pu, pv, jet) {
                    return Ok(None);
                }
                out = Some(out.map_or(jet, |h| h.hull(jet)));
            }
        }
        Ok(out)
    }

    /// Whether a part's points past a continued side ([`Part::Beyond`]) are each within
    /// [`Net::tolerance`] of the surface as built: the point at `t` beyond the side at `s`
    /// differs from the one at `s` (the same other parameter) by the integral of its partial
    /// in that direction, so by at most `|t − s|` times that partial's length bound over the
    /// part (`jet`, the mean-value form), rounded up. Parts on the knots or past an extended
    /// side hold.
    fn near_as_built(&self, pu: Part, pv: Part, jet: Jet) -> bool {
        let off = |p: Part, partial: I3| match p {
            Part::Beyond { at, a, b } => {
                let width = round_up((a - at).abs().max((b - at).abs()));
                round_up(width * length_bound(partial))
            }
            Part::On(..) | Part::Past { .. } => 0.0,
        };
        let (du, dv) = (off(pu, jet.su), off(pv, jet.sv));
        // (a NaN fails)
        du <= self.tolerance && dv <= self.tolerance
    }

    /// The point and partials at `(u, v)`, as floating point numbers (for Newton's
    /// steps), past a u side on the homogeneous lines ([`Form::Lines`]); `None` where
    /// [`Net::jet`] encloses nothing there.
    fn at(&self, u: f64, v: f64) -> Result<Option<(Vec3, Vec3, Vec3)>> {
        Ok(self
            .jet_as(Ivl::from(u), Ivl::from(v), Form::Lines, None)?
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

    /// The net's data as bits, for an edge's bound to be taken again from its other face
    /// ([`super::EdgeBounds`]): the offset, the degrees and counts, the knots, the
    /// homogeneous control points, which sides are extended or continued, and the tolerance.
    /// Two nets with the same bits are the same surface, extended alike.
    pub(super) fn bits(&self) -> Vec<u64> {
        let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        let mut out = vec![3, self.offset.map_or(u64::MAX, f64::to_bits)];
        out.extend([self.cn.p, self.cn.q, self.cn.nu, self.cn.nv].map(count));
        for list in [&self.cn.u, &self.cn.v, &self.cn.pw] {
            out.push(count(list.len()));
            out.extend(list.iter().map(|x| x.to_bits()));
        }
        out.extend(
            self.extends
                .iter()
                .chain(&self.continues)
                .map(|&b| u64::from(b)),
        );
        out.push(self.tolerance.to_bits());
        out
    }

    /// The parameters of the point of the net itself (not its offset, [`Net::offset`]) on
    /// its knots nearest `x`: Gauss–Newton on `(G − x) · G_u = (G − x) · G_v = 0` from
    /// `start`, kept where it ends within the net's tolerance of `x`, else from the point
    /// nearest `x` among the net's knots and span middles ([`Net::nearest_on_knots`]).
    /// Newton's start only, for a point of the edge, which lies on the net: an offset's
    /// meeting there is the one near those parameters (an offset is parametrised as its
    /// net), where a start from the knots alone may lie a turn away on a thread's flank.
    ///
    /// # Errors
    ///
    /// A cancel.
    fn foot(&self, x: Vec3, start: Option<[f64; 2]>) -> Result<Option<[f64; 2]>> {
        let [(u0, u1), (v0, v1)] = self.range;
        let on = |u: f64, v: f64| -> Result<Option<(Vec3, Vec3, Vec3)>> {
            let (Some(us), Some(vs)) = (self.parts(0, u, u), self.parts(1, v, v)) else {
                return Ok(None);
            };
            let (Some(&pu), Some(&pv)) = (us.first(), vs.first()) else {
                return Ok(None);
            };
            Ok(self
                .part_jet_of(pu, pv, Form::Lines, None, None)?
                .map(|j| (middle(j.p), middle(j.su), middle(j.sv))))
        };
        let descend = |[mut u, mut v]: [f64; 2]| -> Result<Option<([f64; 2], f64)>> {
            for _ in 0..NEWTON_STEPS {
                let Some((g, gu, gv)) = on(u, v)? else {
                    return Ok(None);
                };
                let r = g - x;
                let (a, b, c) = (gu.dot(gu), gu.dot(gv), gv.dot(gv));
                let det = a * c - b * b;
                if !(det.is_finite() && det > 0.0) {
                    break;
                }
                let (e, f) = (r.dot(gu), r.dot(gv));
                let (nu, nv) = (
                    (u - (c * e - b * f) / det).max(u0).min(u1),
                    (v - (a * f - b * e) / det).max(v0).min(v1),
                );
                #[allow(clippy::float_cmp)] // a step that changes nothing
                let still = nu == u && nv == v;
                (u, v) = (nu, nv);
                if still {
                    break;
                }
            }
            Ok(on(u, v)?.map(|(g, _, _)| ([u, v], (g - x).length())))
        };
        if let Some(s) = start {
            if let Some((uv, d)) = descend(s)? {
                if d <= self.tolerance {
                    return Ok(Some(uv));
                }
            }
        }
        let Some((u, v)) = self.nearest_on_knots(x)? else {
            return Ok(None);
        };
        Ok(descend([u, v])?.map(|(uv, _)| uv))
    }

    /// The parameters whose point is nearest `x` among the net's knots and the middles of
    /// its spans, every pair of the two directions' (Newton's first guess only: a net that
    /// winds several turns, a thread's flank, meets a section once a turn, and a start on
    /// the grid's coarser cells may lead to another turn's meeting).
    ///
    /// # Errors
    ///
    /// A cancel.
    fn nearest_on_knots(&self, x: Vec3) -> Result<Option<(f64, f64)>> {
        let stations = |knots: &[f64], (r0, r1): (f64, f64)| {
            let mut out: Vec<f64> = Vec::new();
            for w in knots.windows(2) {
                if let [a, b] = *w {
                    if a < b && a >= r0 && b <= r1 {
                        out.extend([a, 0.5 * (a + b), b]);
                    }
                }
            }
            out.dedup();
            out
        };
        if self.stations.get().is_none() {
            let [ru, rv] = self.range;
            let (us, vs) = (stations(&self.cn.u, ru), stations(&self.cn.v, rv));
            let mut points = Vec::new();
            for &u in &us {
                for &v in &vs {
                    if let Some((p, _, _)) = self.at(u, v)? {
                        points.push(((u, v), p));
                    }
                }
            }
            // (a thread that got here first stored the same points)
            let _ = self.stations.set(points);
        }
        let Some(points) = self.stations.get() else {
            return Ok(None);
        };
        let mut best: Option<(f64, (f64, f64))> = None;
        for &(uv, p) in points {
            let d = (p - x).length();
            if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, uv));
            }
        }
        Ok(best.map(|(_, uv)| uv))
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
    /// `tolerance` and passes the vertex `p` within it: the row is then where the plane and
    /// the net meet at that vertex, the corner's curve there. On the plane: every control
    /// point of the row within `tolerance` of it ([`super::plane_distance`], a certified
    /// bound), so the row, in their convex hull (weights positive), is too. Past the vertex:
    /// a point of the row, enclosed, within `tolerance` of `p` ([`Net::row_near`]).
    ///
    /// # Errors
    ///
    /// A cancel.
    pub(super) fn rows_through(
        &self,
        plane: super::Tangent,
        tolerance: f64,
        p: Vec3,
    ) -> Result<Vec<usize>> {
        let mut out = Vec::new();
        for side in self.rows_on(plane, tolerance) {
            if self.row_near(side, p)?.is_some_and(|d| d <= tolerance) {
                out.push(side);
            }
        }
        Ok(out)
    }

    /// The row of `side` (`BoxSide::ALL` order) as its fixed parameter, whether it runs
    /// along u, and its parameter range.
    fn row(&self, side: usize) -> Option<(f64, bool, (f64, f64))> {
        let [(u0, u1), (v0, v1)] = self.range;
        Some(match side {
            0 => (u0, false, (v0, v1)),
            1 => (u1, false, (v0, v1)),
            2 => (v0, true, (u0, u1)),
            3 => (v1, true, (u0, u1)),
            _ => return None,
        })
    }

    /// An upper bound on the distance from `p` of the row of `side` over its range: the
    /// enclosed distance of one point of it, found by Gauss–Newton from the grid point
    /// nearest `p` (a choice only: any point of the row bounds the distance from above).
    ///
    /// # Errors
    ///
    /// A cancel.
    fn row_near(&self, side: usize, p: Vec3) -> Result<Option<f64>> {
        let Some((fixed, along_u, (r0, r1))) = self.row(side) else {
            return Ok(None);
        };
        let at = |t: f64| -> Result<Option<(Vec3, Vec3, Vec3)>> {
            if along_u {
                self.at(t, fixed)
            } else {
                self.at(fixed, t)
            }
        };
        let mut best: Option<(f64, f64)> = None;
        for i in 0..=GUESS_GRID {
            let t = r0 + (r1 - r0) * f64::from(i) / f64::from(GUESS_GRID);
            if let Some((x, _, _)) = at(t)? {
                let d = (x - p).length();
                if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, t));
                }
            }
        }
        let Some((_, mut t)) = best else {
            return Ok(None);
        };
        for _ in 0..NEWTON_STEPS {
            let Some((x, su, sv)) = at(t)? else {
                return Ok(None);
            };
            let d = if along_u { su } else { sv };
            let dd = d.dot(d);
            if !(dd.is_finite() && dd > 0.0) {
                break;
            }
            t = (t - (x - p).dot(d) / dd).max(r0).min(r1);
        }
        let jet = if along_u {
            self.jet(Ivl::from(t), Ivl::from(fixed))?
        } else {
            self.jet(Ivl::from(fixed), Ivl::from(t))?
        };
        Ok(jet.map(|g| length_bound(sub(g.p, point(p)))))
    }

    /// The sides (`BoxSide::ALL` order) whose boundary row lies on `plane` within
    /// `tolerance` ([`Net::rows_through`]); none for an offset ([`Net::offset`]), whose rows
    /// are not the net's control rows.
    fn rows_on(&self, plane: super::Tangent, tolerance: f64) -> Vec<usize> {
        // an offset's rows are not the net's control rows
        if self.offset.is_some() {
            return Vec::new();
        }
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
    /// along the row (module docs), or `None`. The caller has proved the row passes `p`
    /// ([`Net::rows_through`]).
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
        let Some((fixed, along_u, (r0, r1))) = self.row(side) else {
            return Ok(None);
        };
        let (n, o) = (point(normal), point(origin));
        let jet = |t: Ivl| -> Result<Option<Jet>> {
            if along_u {
                self.jet_near(t, Ivl::from(fixed), p)
            } else {
                self.jet_near(Ivl::from(fixed), t, p)
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
        let built = if along_u {
            self.jet_as_built(t, Ivl::from(fixed), p)?
        } else {
            self.jet_as_built(Ivl::from(fixed), t, p)?
        };
        Ok(built.map(|g| length_bound(sub(g.p, point(p)))))
    }
}

/// Whether the offset of `cn` by `distance` has no jump across the base's inner knots of
/// multiplicity `p`, where the base is only C0: there its normal, which the offset moves
/// along, may turn, and the offset then jumps by `distance` times the turn, which the
/// certificates' mean-value enclosures do not hold across. The base's tangent directions
/// either side of such a knot line, at every control column, must agree within an angle
/// whose offset moves a point by at most `tolerance` (and at most 1e-3 radian): a G1 join,
/// such as one circle's arcs make, passes, a crease does not. A knot of lower multiplicity
/// is C1 and its normal runs on.
#[allow(clippy::float_cmp)] // a distance of exactly nought is no offset
fn offset_is_continuous(cn: &Nurbs, distance: f64, tolerance: f64) -> bool {
    if distance == 0.0 {
        return true;
    }
    let limit = (tolerance / distance.abs()).min(1e-3);
    // the homogeneous control point `(i, j)` (u-major)
    let point = |i: usize, j: usize| -> Option<[f64; 4]> {
        let at = i.checked_mul(cn.nv)?.checked_add(j)?.checked_mul(4)?;
        match cn.pw.get(at..at.checked_add(4)?)? {
            [x, y, z, w] => Some([*x, *y, *z, *w]),
            _ => None,
        }
    };
    joins_at_c0_knots(&cn.u, cn.p, cn.nv, &point, limit)
        && joins_at_c0_knots(&cn.v, cn.q, cn.nu, &|k, c| point(c, k), limit)
}

/// [`offset_is_continuous`] along one direction: `at(k, c)` is the control point `k` along
/// the direction in the column `c` of `across`. At a knot of multiplicity `p` the surface
/// passes through control point `k` (the one before the knot's first position), and its
/// one-sided derivative of the weighted points either side is along the step to the
/// neighbour (the surface's, `D_xyz − S D_w` for the step `D` of the homogeneous points and
/// the surface's point `S`, up to a positive factor).
#[allow(clippy::float_cmp)] // knots repeated as stored
fn joins_at_c0_knots(
    knots: &[f64],
    p: usize,
    across: usize,
    at: &dyn Fn(usize, usize) -> Option<[f64; 4]>,
    limit: f64,
) -> bool {
    let end = knots.len().saturating_sub(p.saturating_add(1));
    let mut first = p.saturating_add(1);
    while first < end {
        let Some(&knot) = knots.get(first) else {
            return false;
        };
        let mut run = 1_usize;
        while knots.get(first.saturating_add(run)) == Some(&knot) {
            run = run.saturating_add(1);
        }
        if run == p {
            let Some(k) = first.checked_sub(1) else {
                return false;
            };
            for c in 0..across {
                let (Some(before), Some(mid), Some(after)) = (
                    k.checked_sub(1).and_then(|b| at(b, c)),
                    at(k, c),
                    at(k.saturating_add(1), c),
                ) else {
                    return false;
                };
                let step = |from: [f64; 4], to: [f64; 4]| -> Option<[f64; 3]> {
                    if mid[3] <= 0.0 {
                        return None;
                    }
                    let s = [mid[0] / mid[3], mid[1] / mid[3], mid[2] / mid[3]];
                    let w = to[3] - from[3];
                    Some([
                        (to[0] - from[0]) - s[0] * w,
                        (to[1] - from[1]) - s[1] * w,
                        (to[2] - from[2]) - s[2] * w,
                    ])
                };
                let (Some(a), Some(b)) = (step(before, mid), step(mid, after)) else {
                    return false;
                };
                let len = |v: [f64; 3]| sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
                let cross = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                let along = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let (la, lb) = (len(a), len(b));
                if !(la > 0.0 && lb > 0.0 && along > 0.0 && len(cross) <= limit * la * lb) {
                    return false;
                }
            }
        }
        first = first.saturating_add(run);
    }
    true
}

/// Whether the full knot vector `knots` of degree `p` is clamped at both ends (its first
/// and last knots each exactly `p + 1` times: a longer run ends the surface on its last
/// control row alone, at the end knot, where a net clamped `p + 1` times ends on the row
/// before) and has no inner knot of multiplicity above `p`.
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
    // each end exactly `p + 1` times: the knot after the first run and the one before the
    // last differ from them
    let after = knots.get(ends);
    let before = ends
        .checked_add(1)
        .and_then(|e| knots.len().checked_sub(e))
        .and_then(|i| knots.get(i));
    #[allow(clippy::float_cmp)] // knots repeated as stored
    let longer = after == head.first() || before == tail.last();
    if longer {
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
    // an offset's meeting from the line's foot's own parameters on the net ([`Net::foot`]),
    // the grid's point nearest the line may lie a turn away on a thread's flank
    let first = |net: &Net| -> Result<Option<[f64; 3]>> {
        if net.offset.is_some() {
            if let Some([u, v]) = net.foot(mx0, None)? {
                return Ok(net.at(u, v)?.map(|(p, _, _)| [u, v, (p - mx0).dot(mu)]));
            }
        }
        net.nearest_to_line(mx0, mu)
    };
    let start = match guess.get() {
        Some(g) => g,
        None => match first(net)? {
            Some(g) => g,
            None => return Ok(None),
        },
    };
    let mut root = newton(start, &at, &inverse_of_columns3)?;
    if root.is_none() && guess.get().is_some() {
        if let Some(g) = first(net)? {
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
        Ok(net
            .jet_near(a, b, mx0)?
            .map(|jet| sub(sub(jet.p, x0), scale(u, tau))))
    };
    let minus_u = scale(u, Ivl::from(-1.0));
    let jacobian = |y: [Ivl; 3]| -> Result<Option<[[Ivl; 3]; 3]>> {
        let [a, b, _] = y;
        Ok(net
            .jet_near(a, b, mx0)?
            .map(|jet| [jet.su, jet.sv, minus_u]))
    };
    let Some([a, b, tau]) = krawczyk(root, c, &value, &jacobian)? else {
        return Ok(None);
    };
    Ok(net.jet_as_built(a, b, mx0)?.map(|_| tau))
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
        let (Some(ja), Some(jb)) = (a.jet_near(ua, va, p)?, b.jet_near(ub, vb, p)?) else {
            return Ok(None);
        };
        let [dx, dy, dz] = sub(ja.p, jb.p);
        Ok(Some([dx, dy, dz, dot(n, sub(ja.p, o))]))
    };
    let jacobian = |y: [Ivl; 4]| -> Result<Option<[[Ivl; 4]; 4]>> {
        let [ua, va, ub, vb] = y;
        let (Some(ja), Some(jb)) = (a.jet_near(ua, va, p)?, b.jet_near(ub, vb, p)?) else {
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
    let Some([ua, va, ub, vb]) = krawczyk(root, c, &value, &jacobian)? else {
        return Ok(None);
    };
    if b.jet_as_built(ub, vb, p)?.is_none() {
        return Ok(None);
    }
    Ok(a.jet_as_built(ua, va, p)?
        .map(|jet| length_bound(sub(jet.p, point(p)))))
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
    // an offset's meeting from the vertex's own parameters on the net (the grid's nearest
    // point may lie a turn away on a thread's flank, [`Net::foot`])
    let guess = if net.offset.is_some() {
        net.foot(p, None)?.map(|[u, v]| (u, v))
    } else {
        net.nearest_on_grid(p)?
    };
    let Some(guess) = guess else {
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
        let Some(jet) = net.jet_near(u, v, p)? else {
            return Ok(None);
        };
        Ok(quadric_value_and_gradient(q, jet.p).map(|(f, _)| [dot(n, sub(jet.p, o)), f]))
    };
    let jacobian = |y: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
        let [u, v] = y;
        let Some(jet) = net.jet_near(u, v, p)? else {
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
    let Some(jet) = net.jet_as_built(u, v, p)? else {
        return Ok(None);
    };
    // the quadric's own nappe, as the meetings of two quadrics and a net: a cone's squared
    // equation holds on both
    Ok(super::quadric::on_nappe(q, jet.p).then(|| length_bound(sub(jet.p, point(p)))))
}

/// Where the quadrics `a` and `b` and the net meet, near the vertex `p`: an upper bound on
/// the distance from `p` of a meeting proved by Krawczyk's test on `F_a(N(u, v)) = 0`,
/// `F_b(N(u, v)) = 0` (each quadric's implicit function, [`quadric_value_and_gradient`];
/// module docs), on both quadrics' own nappes ([`super::quadric::on_nappe`]) and on the
/// surface as built, or `None`. Newton starts from the vertex's parameters on the net
/// ([`Net::foot`]).
///
/// # Errors
///
/// A cancel.
pub(super) fn quadrics_and_net_meet(
    net: &Net,
    a: &Implicit,
    b: &Implicit,
    p: Vec3,
) -> Result<Option<f64>> {
    let at = |y: [f64; 2]| -> Result<Linearised<2>> {
        let [u, v] = y;
        let Some((x, su, sv)) = net.at(u, v)? else {
            return Ok(None);
        };
        let (Some((fa, ga)), Some((fb, gb))) = (
            quadric_value_and_gradient(a, point(x)),
            quadric_value_and_gradient(b, point(x)),
        ) else {
            return Ok(None);
        };
        let (ga, gb) = (middle(ga), middle(gb));
        Ok(Some((
            [fa.mid(), fb.mid()],
            [[ga.dot(su), gb.dot(su)], [ga.dot(sv), gb.dot(sv)]],
        )))
    };
    let Some(start) = net.foot(p, None)? else {
        return Ok(None);
    };
    let Some(root) = newton(start, &at, &inverse_of_columns2)? else {
        return Ok(None);
    };
    let Some((_, j)) = at(root)? else {
        return Ok(None);
    };
    let Some(c) = inverse_of_columns2(&j) else {
        return Ok(None);
    };
    let both = |x: I3| quadric_value_and_gradient(a, x).zip(quadric_value_and_gradient(b, x));
    let value = |y: [Ivl; 2]| -> Result<Option<[Ivl; 2]>> {
        let [u, v] = y;
        let Some(jet) = net.jet_near(u, v, p)? else {
            return Ok(None);
        };
        Ok(both(jet.p).map(|((fa, _), (fb, _))| [fa, fb]))
    };
    let jacobian = |y: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
        let [u, v] = y;
        let Some(jet) = net.jet_near(u, v, p)? else {
            return Ok(None);
        };
        Ok(both(jet.p).map(|((_, ga), (_, gb))| {
            [
                [dot(ga, jet.su), dot(gb, jet.su)],
                [dot(ga, jet.sv), dot(gb, jet.sv)],
            ]
        }))
    };
    let Some([u, v]) = krawczyk(root, c, &value, &jacobian)? else {
        return Ok(None);
    };
    let Some(jet) = net.jet_as_built(u, v, p)? else {
        return Ok(None);
    };
    let on = super::quadric::on_nappe(a, jet.p) && super::quadric::on_nappe(b, jet.p);
    Ok(on.then(|| length_bound(sub(jet.p, point(p)))))
}

/// A piece of an edge, enclosed over its parameters `t`: the point `p` and its rate `p'`,
/// the tangent `τ` (the normal section's normal) and its rate `τ'`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Moving {
    pub(super) p: I3,
    pub(super) p_rate: I3,
    pub(super) t: I3,
    pub(super) t_rate: I3,
}

/// Where the quadric `q` and the net meet in the normal sections of a piece of an edge, the
/// planes through the edge point `p(t)` across its tangent `τ(t)` (`over` the piece, `mid`
/// its middle `t_m` as point enclosures, `h` the range of `t − t_m`): certified upper and
/// lower bounds on the distance from `p(t)` of a meeting proved in every section of the
/// piece, or `None`.
///
/// The meeting solves `F(u, v; t) = (τ · (N − p), Q(N)) = 0` (`N` the net at `(u, v)`, `Q` the
/// quadric's implicit function, [`quadric_value_and_gradient`]; module docs). Along an edge
/// the root moves with `t`, while the net's parameter along the edge is barely tied by `Q`
/// (the net runs along the quadric there), so a box over all the roots of a piece is long
/// in that parameter, and its Jacobian, enclosed over it, too wide for Krawczyk's test. The
/// root's first-order motion is taken out instead: with `ỹ` Newton's root at `t_m`, `C` the
/// inverse of `F′` there and `s = −C ∂F/∂t` (`∂F/∂t = (τ' · (N − p) − τ · p', 0)` at fixed
/// `(u, v)`), the unknown is `δ` in `(u, v) = ỹ + s (t − t_m) + δ`, and for every `t` of the
/// piece a root `δ(t)` is proved in one small box by Krawczyk's operator: `F` at `δ = 0`
/// enclosed by its mean-value form in `t`, `F(t_m) + (dF/dt) h` with
/// `dF/dt = (τ' · (N − p) + τ · (N_u s_u + N_v s_v − p'), ∇Q · (N_u s_u + N_v s_v))` over
/// the box `B = ỹ + s h`, and `F′` over `B + δ`. The meeting is on the quadric's own nappe
/// ([`super::quadric::on_nappe`]) and on the surface as built ([`Net::jet_as_built`]). Its
/// offset from the edge point is, by the mean-value form from `ỹ`,
/// `N(ỹ) − p(t_m) + (N_u s_u + N_v s_v − p') h + N_u δ_u + N_v δ_v`, the partials over the
/// hull of `ỹ` and the proved box; the upper bound is that vector's length, the lower its
/// least length at `t_m` (the value that steers the halving).
///
/// Newton starts from the parameters of the edge point `p(t_m)` on the net ([`Net::foot`],
/// `guess` carrying them from one piece to the next): the meeting nearest the edge, where a
/// start from anywhere else may reach a meeting a turn away on a thread's flank. It only
/// steers Newton, and what is proved is decided by the box alone.
///
/// # Errors
///
/// A cancel.
#[allow(clippy::too_many_lines)] // the moving root's set-up, its proof and its distance
pub(super) fn section_quadric_meets(
    net: &Net,
    q: &Implicit,
    over: &Moving,
    mid: &Moving,
    h: Ivl,
    guess: &Cell<Option<[f64; 2]>>,
) -> Result<Option<(f64, f64)>> {
    let (mp, mt) = (middle(mid.p), middle(mid.t));
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
            [(x - mp).dot(mt), f.mid()],
            [[mt.dot(su), g.dot(su)], [mt.dot(sv), g.dot(sv)]],
        )))
    };
    // Newton from the edge point's own parameters on the net (the meeting nearest it)
    let Some(start) = net.foot(mp, guess.get())? else {
        return Ok(None);
    };
    guess.set(Some(start));
    let Some(root) = newton(start, &at, &inverse_of_columns2)? else {
        return Ok(None);
    };
    let Some((x, _, _)) = net.at(root[0], root[1])? else {
        return Ok(None);
    };
    let Some((_, j)) = at(root)? else {
        return Ok(None);
    };
    let Some(c) = inverse_of_columns2(&j) else {
        return Ok(None);
    };
    // the root's rate along the edge, `s = −C ∂F/∂t` (a choice of centre only: any `s`
    // keeps the proof valid)
    let f_t = (x - mp).dot(middle(mid.t_rate)) - mt.dot(middle(mid.p_rate));
    let [[c00, _], [c10, _]] = c;
    let rate = [-c00 * f_t, -c10 * f_t];
    if !rate.iter().all(|r| r.is_finite()) {
        return Ok(None);
    }
    let [ru, rv] = rate.map(Ivl::from);
    let [u0, v0] = root.map(Ivl::from);
    let (bu, bv) = (u0 + ru * h, v0 + rv * h);
    // `F` at `t_m` from the root's own point, and its rate over the piece
    let (Some(at_root), Some(over_b)) = (net.jet_near(u0, v0, mp)?, net.jet_near(bu, bv, mp)?)
    else {
        return Ok(None);
    };
    let (Some((q_root, _)), Some((_, g_b))) = (
        quadric_value_and_gradient(q, at_root.p),
        quadric_value_and_gradient(q, over_b.p),
    ) else {
        return Ok(None);
    };
    let moves = add(scale(over_b.su, ru), scale(over_b.sv, rv));
    let f_mid = [dot(mid.t, sub(at_root.p, mid.p)), q_root];
    let f_rate = [
        dot(over.t_rate, sub(over_b.p, over.p)) + dot(over.t, sub(moves, over.p_rate)),
        dot(g_b, moves),
    ];
    let [f0, f1] = f_mid;
    let [d0, d1] = f_rate;
    let at_zero = [f0 + d0 * h, f1 + d1 * h];
    let value = |_: [Ivl; 2]| -> Result<Option<[Ivl; 2]>> { Ok(Some(at_zero)) };
    let jacobian = |y: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
        let [du, dv] = y;
        let Some(jet) = net.jet_near(bu + du, bv + dv, mp)? else {
            return Ok(None);
        };
        Ok(quadric_value_and_gradient(q, jet.p).map(|(_, g)| {
            [
                [dot(over.t, jet.su), dot(g, jet.su)],
                [dot(over.t, jet.sv), dot(g, jet.sv)],
            ]
        }))
    };
    let Some([du, dv]) = krawczyk([0.0; 2], c, &value, &jacobian)? else {
        return Ok(None);
    };
    let (fu, fv) = (bu + du, bv + dv);
    let Some(met) = net.jet_as_built(fu, fv, mp)? else {
        return Ok(None);
    };
    if !super::quadric::on_nappe(q, met.p) {
        return Ok(None);
    }
    // the partials over the hull of the root and the proved box
    let Some(hull) = net.jet_near(fu.hull(u0), fv.hull(v0), mp)? else {
        return Ok(None);
    };
    let along = sub(add(scale(hull.su, ru), scale(hull.sv, rv)), over.p_rate);
    let off = add(
        add(sub(at_root.p, mid.p), scale(along, h)),
        add(scale(hull.su, du), scale(hull.sv, dv)),
    );
    let upper = length_bound(off);
    let least = sub(at_root.p, mid.p).map(Ivl::mig);
    let lower = sqrt(least.iter().map(|x| x * x).sum::<f64>());
    Ok(upper.is_finite().then_some((upper, lower)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bilinear net over three rows in u and two in v whose middle row is at height
    /// `middle`: a plane at 0, a crease in u at its knot otherwise (degree 1: the knot of
    /// multiplicity 1 is C0).
    fn creased(middle: f64) -> Nurbs {
        let mut pw = Vec::new();
        for (x, z) in [(0.0, 0.0), (1.0, middle), (2.0, 0.0)] {
            for y in [0.0, 1.0] {
                pw.extend([x, y, z, 1.0]);
            }
        }
        Nurbs {
            p: 1,
            q: 1,
            nu: 3,
            nv: 2,
            u: vec![0.0, 0.0, 1.0, 2.0, 2.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        }
    }

    /// An offset of a base whose normal turns across a C0 knot jumps there, and is not
    /// taken as a net; a flat base, or a distance of nought, is.
    #[test]
    fn an_offset_is_continuous_only_across_joins_of_the_base_that_keep_its_normal() {
        assert!(offset_is_continuous(&creased(0.0), 0.5, 1e-6));
        assert!(!offset_is_continuous(&creased(0.75), 0.5, 1e-6));
        assert!(offset_is_continuous(&creased(0.75), 0.0, 1e-6));
    }
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

    /// A saddle as a bilinear net: corners (0, 0, 0), (10, 0, 0), (0, 10, −5), (10, 10, 5),
    /// so `z = v (10 u − 5)`: the plane z = 0 holds its row v = 0 (y = 0) and its line
    /// u = ½ (x = 5).
    fn saddle() -> Surface {
        let row =
            |x: f64, z1: f64| vec![CPoint::new(x, 0.0, 0.0, NOW), CPoint::new(x, 10.0, z1, NOW)];
        let patch = Patch::new(
            vec![row(0.0, -5.0), row(10.0, 5.0)],
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_gw_nurb(&patch))
    }

    /// A boundary row on a plane is the corner's only where it passes the corner's vertex:
    /// the saddle's row v = 0 lies on z = 0, which also holds its line x = 5; at a vertex on
    /// that line, (5, 8, 0), the row is no curve of the corner and is not taken, at a vertex
    /// on the row, (3, 0, 0) or its end (0, 0, 0), it is (`BoxSide::VMin`, 2).
    #[test]
    fn a_row_on_the_plane_is_the_corners_only_through_its_vertex() {
        let k = Kernel::default();
        let n = net(&k, &saddle());
        let plane = super::super::Tangent {
            point: Vec3::ZERO,
            normal: Vec3::Z,
            raw: Vec3::Z,
        };
        assert!(n
            .rows_through(plane, 1e-6, Vec3::new(5.0, 8.0, 0.0))
            .unwrap()
            .is_empty());
        assert_eq!(
            n.rows_through(plane, 1e-6, Vec3::new(3.0, 0.0, 0.0))
                .unwrap(),
            vec![2]
        );
        assert_eq!(n.rows_through(plane, 1e-6, Vec3::ZERO).unwrap(), vec![2]);
    }

    /// Newton's start for Krawczyk's box settles wherever the part lies: a quarter cylinder
    /// of radius 0.002 about (1000, 1000), z 1000 to 1000.003, whose points round at about
    /// 1e-13, while its parameters move 500 per unit length. The line through its axis at
    /// z 1000.0015 along (1, 1, 0) / √2 meets it 0.002 along.
    #[test]
    fn a_small_part_far_from_the_origin_is_met() {
        let k = Kernel::default();
        let (c, r, h) = (1000.0, 0.002, 0.003);
        let w = core::f64::consts::FRAC_1_SQRT_2;
        let row = |x: f64, y: f64, w: f64| {
            vec![
                CPoint::new((c + x) * w, (c + y) * w, c * w, w),
                CPoint::new((c + x) * w, (c + y) * w, (c + h) * w, w),
            ]
        };
        let patch = Patch::new(
            vec![row(r, 0.0, 1.0), row(r, r, w), row(0.0, r, 1.0)],
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let n = net(&k, &Surface::from(BSplineSurface::from_gw_nurb(&patch)));
        let guess = Cell::new(None);
        let (x0, u) = (Vec3::new(c, c, c + h / 2.0), Vec3::new(w, w, 0.0));
        let tau = line_meets(&n, (point(x0), point(u)), (x0, u), &guess)
            .unwrap()
            .unwrap();
        assert!(
            tau.lo <= r && r <= tau.hi && tau.hi - tau.lo < 1e-9,
            "{tau:?}"
        );
    }

    /// The plane x = `x` as a bilinear net over y in [−5, 5], z in [−5, 5].
    fn plane_net_at_x(x: f64) -> Surface {
        plane_net_at_height(x, 0.0)
    }

    fn plane_net_at_height(x: f64, height: f64) -> Surface {
        let row = |y: f64| {
            vec![
                CPoint::new(x, y, height - 5.0, NOW),
                CPoint::new(x, y, height + 5.0, NOW),
            ]
        };
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
        Net::of(k, s, 0.0).unwrap().unwrap()
    }

    fn holds(x: Ivl, want: f64) -> bool {
        x.lo <= want && want <= x.hi && x.hi - x.lo <= 1e-12
    }

    /// Past the top (v = 1) the quarter cylinder runs on along its rulings, on the cylinder:
    /// `G + (v − 1) G_v` with `G_v` the ruling, so v = 1.25 is z = 3.75 at
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

    /// The distance of `x`, the built extension's point at `(u, v)`, from the model's
    /// extension of `form` (module docs): on the v knots the model's point at `(u, v)` (past
    /// a u side the parameters are the construction's own); past a v side, from the line
    /// `E + λ E_v` the model runs from that side at `u` (the built extension runs the same
    /// line at another pace).
    fn model_distance(n: &Net, form: Form, x: Vec3, (u, v): (f64, f64)) -> f64 {
        let at = |u: f64, v: f64| {
            let j = n
                .jet_as(Ivl::from(u), Ivl::from(v), form, None)
                .unwrap()
                .unwrap();
            (middle(j.p), middle(j.sv))
        };
        let [_, (v0, v1)] = n.range;
        if (v0..=v1).contains(&v) {
            return (at(u, v).0 - x).length();
        }
        let (e, ev) = at(u, if v < v0 { v0 } else { v1 });
        let r = x - e;
        (r - ev * (r.dot(ev) / ev.dot(ev))).length()
    }

    /// The points of `source` extended `Ruled` by `distance` on every side
    /// (`extended_sides_keeping_parameters`, as the offset executive asks) past its unit
    /// box, on a 9 × 9 grid over the result's box, each with its distance from the model's
    /// extension of `form`.
    fn built_against_model(source: &Surface, distance: f64, form: Form) -> Vec<((f64, f64), f64)> {
        use crate::surface::extend::Continuity;
        let k = Kernel::default();
        let n = net(&k, source);
        let built = source
            .extended_sides_keeping_parameters(&k, &BoxSide::ALL, Continuity::Ruled, distance)
            .unwrap()
            .surface;
        let dom = built.natural_uv_domain_with_nurb(&k).unwrap();
        let (lo, hi) = (dom.min(), dom.max());
        let mut out = Vec::new();
        for i in 0..=8 {
            for j in 0..=8 {
                let u = lo.x + (hi.x - lo.x) * f64::from(i) / 8.0;
                let v = lo.y + (hi.y - lo.y) * f64::from(j) / 8.0;
                if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
                    continue;
                }
                let x = built
                    .evaluate_point(&k, crate::geom::vector2d::Point2::new(u, v))
                    .unwrap();
                let x = Vec3::new(x.x, x.y, x.z);
                out.push(((u, v), model_distance(&n, form, x, (u, v))));
            }
        }
        out
    }

    /// The model of the extension past the sides and the corner (module docs) is the surface
    /// the executive builds: the quarter cylinder extended by 1.5, whose rows end in one
    /// weight ratio, and the uneven bump extended by 1, whose rows do not (the homogeneous
    /// lines past its u sides, their corners with the v sides carrying the lines'
    /// parameters), and by 200, past the lines' reach (the product form past its u sides,
    /// `κ = 0`): every point past the sides and the corners lies on the model within
    /// rounding.
    #[test]
    fn the_extension_model_is_the_ruled_extension_the_executive_builds() {
        let cases = [
            (quarter_cylinder(), 1.5, Form::Lines),
            (uneven_bump(), 1.0, Form::Lines),
            (uneven_bump(), 200.0, Form::Product),
        ];
        for (source, distance, form) in cases {
            let found = built_against_model(&source, distance, form);
            for ((u, v), d) in &found {
                assert!(*d < 1e-9, "({u}, {v}): {d}");
            }
            // past the two ends in u, the two in v, and the four corners
            assert!(found.len() >= 32, "{}", found.len());
        }
        // the two forms' corners differ there: the lines' model misses the product form's
        let lines = built_against_model(&uneven_bump(), 200.0, Form::Lines);
        let most = lines.iter().map(|(_, d)| *d).fold(0.0, f64::max);
        assert!(most > 1e-6, "{most}");
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

    /// A bump 10 × 10 over the square, biquadratic, whose rows end in different weight
    /// ratios across every side (as a variable round's): middle weights 0.8, 0.9, 0.7 along
    /// u, so the ratios across u = 0 are 0.8, 0.9, 0.7 and across v = 0 are 1, 1.125, 1.
    fn uneven_bump() -> Surface {
        let w = [[1.0, 1.0, 1.0], [0.8, 0.9, 0.7], [1.0, 1.0, 1.0]];
        let z = [[0.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 0.0]];
        let net = (0..3_u8)
            .map(|i| {
                (0..3_u8)
                    .map(|j| {
                        let (x, y) = (5.0 * f64::from(i), 5.0 * f64::from(j));
                        let (a, b) = (usize::from(i), usize::from(j));
                        let (z, w) = (z[a][b], w[a][b]);
                        CPoint::new(x * w, y * w, z * w, w)
                    })
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let patch = Patch::new(net, 2, 2, knots.clone(), knots).unwrap();
        Surface::from(BSplineSurface::from_gw_nurb(&patch))
    }

    /// A quadratic strip given the knots 0, 0, 0, 1 − 1e-13, 1, 1, 1 in u: a span narrower
    /// than the knots' scaled zero, which the stored surface takes as a repeat, so its u knots
    /// are 0, 0, 0, a, a, a, a (`a = 1 − 1e-13`), the end knot four times for degree 2. Its
    /// rows run at z = 0 but the last, at z = 1.
    fn strip_with_a_sliver_of_a_span() -> Surface {
        let row = |x: f64, z: f64| vec![CPoint::new(x, 0.0, z, NOW), CPoint::new(x, 10.0, z, NOW)];
        let patch = Patch::new(
            vec![row(0.0, 0.0), row(5.0, 0.0), row(10.0, 0.0), row(10.0, 1.0)],
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0 - 1e-13, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Surface::from(BSplineSurface::from_gw_nurb(&patch))
    }

    /// A net is its surface at its sides: the strip ends on its last row (z = 1 at the end
    /// of its domain), while the rows before it, which a net clamped `p + 1` times would end
    /// on, are at z = 0. Where the net is taken at all, what it encloses at the side holds
    /// the surface's point there.
    #[test]
    fn a_net_is_taken_only_where_it_ends_on_its_surfaces_side() {
        let k = Kernel::default();
        let s = strip_with_a_sliver_of_a_span();
        let side = s.natural_uv_domain_with_nurb(&k).unwrap().max().x;
        let Some(n) = Net::of(&k, &s, 0.0).unwrap() else {
            return;
        };
        for v in [0.0, 0.5, 1.0] {
            let want = s
                .evaluate_point(&k, crate::geom::vector2d::Point2::new(side, v))
                .unwrap();
            assert!((want.z - 1.0).abs() < 1e-9, "{want:?}");
            let Some(jet) = n.jet(Ivl::from(side), Ivl::from(v)).unwrap() else {
                continue;
            };
            let [x, y, z] = jet.p;
            let holds = |i: Ivl, w: f64| i.lo <= w && w <= i.hi;
            assert!(
                holds(x, want.x) && holds(y, want.y) && holds(z, want.z),
                "at v {v}: {:?} against {want:?}",
                jet.p
            );
        }
    }

    /// The net point nearest a point among the stations (the knots and span middles) is looked
    /// for on every piece of an edge ([`Net::foot`]'s fallback), where a thread's flank has
    /// thousands of stations: they are enclosed once, on the first look, and held, so a
    /// reach over a many-turn thread does not enclose the whole grid again on each piece of
    /// each edge (#15715: a one-face Offset Faces of a thread's root ran for hours).
    #[test]
    fn the_stations_nearest_a_point_are_enclosed_once_and_held() {
        let k = Kernel::default();
        let n = net(&k, &quarter_cylinder());
        assert!(n.stations.get().is_none());
        // the middle of the arc at the top, then its start at the bottom
        let top = core::f64::consts::SQRT_2;
        let first = n
            .nearest_on_knots(Vec3::new(top, top, 3.0))
            .unwrap()
            .unwrap();
        assert!(
            (first.0 - 0.5).abs() < 1e-9 && (first.1 - 1.0).abs() < 1e-9,
            "{first:?}"
        );
        let held = n.stations.get().map(|s| (s.len(), s.as_ptr()));
        assert_eq!(
            held.map(|h| h.0),
            Some(9),
            "3 stations along the arc, 3 up the axis"
        );
        let second = n
            .nearest_on_knots(Vec3::new(2.0, 0.0, 0.0))
            .unwrap()
            .unwrap();
        assert!(second.0.abs() < 1e-9 && second.1.abs() < 1e-9, "{second:?}");
        assert_eq!(n.stations.get().map(|s| (s.len(), s.as_ptr())), held);
    }

    /// A line along the normal through the bump's corner (u, v) = (1, 0), from 0.5 below it,
    /// meets it at the corner, 0.5 along: Krawczyk's box about the corner runs past both sides
    /// there, whose rows end in different weight ratios, and is enclosed all the same.
    #[test]
    fn a_line_through_the_corner_of_a_net_with_uneven_end_weights_meets_it() {
        let k = Kernel::default();
        let n = net(&k, &uneven_bump());
        let (x, su, sv) = n.at(1.0, 0.0).unwrap().unwrap();
        let normal = su.cross(sv).unitize().unwrap();
        let x0 = x - normal * 0.5;
        let guess = Cell::new(None);
        let tau = line_meets(&n, (point(x0), point(normal)), (x0, normal), &guess)
            .unwrap()
            .unwrap();
        assert!(
            tau.lo <= 0.5 + 1e-12 && 0.5 - 1e-12 <= tau.hi && tau.hi - tau.lo < 1e-9,
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
    /// The shared cone evaluator preserves the previous projected expression's value
    /// and gradient enclosures exactly for cardinal unit axes, even at large heights.
    /// The expanded v.v-z² form loses that sharpness by subtracting large squares.
    #[test]
    fn a_cardinal_cones_enclosures_match_the_previous_projected_form() {
        use super::super::{quadric, Di, Dv};
        for axis in [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::Z] {
            for height in [0.0, 1000.0, -1e6, 1e6, 1e8] {
                for slope in [0.0, 0.02, -0.05] {
                    let q = Implicit::Cone {
                        origin: Vec3::ZERO,
                        axis,
                        radius: 2.0,
                        tan_angle: slope,
                    };
                    let centre = axis * height + Vec3::new(1.0, 1.5, 0.5);
                    let x = [centre.x, centre.y, centre.z].map(|c| Ivl::new(c - 1e-4, c + 1e-4));
                    let [xx, yy, zz] = x.map(|v| Di {
                        v,
                        d: Ivl::from(0.0),
                    });
                    let v = Dv {
                        x: xx,
                        y: yy,
                        z: zz,
                    }
                    .sub(Dv::constant(Vec3::ZERO));
                    let a = Dv::constant(axis);
                    let z = v.dot(a);
                    let t = Di::constant(slope);
                    let r = Di::constant(2.0) + t * z;
                    let pv = v.sub(a.scale(z));
                    let old_f = (pv.dot(pv) - r * r).v;
                    let old_g = pv
                        .scale(Di::constant(2.0))
                        .sub(a.scale(Di::constant(2.0) * r * t));
                    let (f, g) = quadric::value_and_gradient(&q, x).unwrap();
                    let bits = |v: Ivl| (v.lo.to_bits(), v.hi.to_bits());
                    assert_eq!(bits(f), bits(old_f));
                    assert_eq!(g.map(bits), [old_g.x.v, old_g.y.v, old_g.z.v].map(bits));
                }
            }
        }
    }

    /// A plane/net x = 1, a cylinder r = 2 and the plane z = H meet at
    /// (1, √3, H), distance √3-1.5 from (1, 1.5, H), independently of H.
    /// This exercises the existing plane/quadric/net corner certificate itself.
    #[test]
    fn a_plane_quadric_net_corner_still_certifies_at_large_axial_heights() {
        let k = Kernel::default();
        let q = Implicit::Cone {
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            radius: 2.0,
            tan_angle: 0.0,
        };
        let want = 3.0_f64.sqrt() - 1.5;
        for height in [0.0, -1e6, 1e6, 1000.0] {
            let n = net(&k, &plane_net_at_height(1.0, height));
            let p = Vec3::new(1.0, 1.5, height);
            let found = plane_quadric_and_net_meet(&n, &q, Vec3::new(0.0, 0.0, height), Vec3::Z, p)
                .unwrap()
                .unwrap_or_else(|| panic!("H={height}: the corner was not certified"));
            assert!(
                found >= want && found <= want + 1e-8,
                "H={height}: {found} vs {want}"
            );
        }
    }
}
