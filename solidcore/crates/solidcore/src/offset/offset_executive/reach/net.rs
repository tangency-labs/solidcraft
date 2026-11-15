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
    /// net is only its approximation); `None` for any other. A revolution's sides round its
    /// axis are not extended, as the executive leaves them (`SideExtension::AroundAxis`):
    /// past them the net's end span continued is enclosed only within `tolerance` of the side
    /// ([`Net::continues`]).
    ///
    /// # Errors
    ///
    /// Those of the net's form and of the closure and side queries; a cancel.
    pub(super) fn of(k: &Kernel, surface: &Surface, tolerance: f64) -> Result<Option<Self>> {
        if !matches!(
            surface.kind(),
            SurfaceKind::BSpline(_)
                | SurfaceKind::SurfOfExtrusion(_)
                | SurfaceKind::SurfOfRevolution(_)
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
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
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
                        Err(e @ SolidError::Cancelled { .. }) => return Err(e),
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
            Err(e @ SolidError::Cancelled { .. }) => Err(e),
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
    /// `tolerance` ([`Net::rows_through`]).
    fn rows_on(&self, plane: super::Tangent, tolerance: f64) -> Vec<usize> {
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
    Ok(net
        .jet_as_built(u, v, p)?
        .map(|jet| length_bound(sub(jet.p, point(p)))))
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
