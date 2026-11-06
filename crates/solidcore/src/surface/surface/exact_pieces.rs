//! The intersection of two exact surfaces over their parameter domains, as the pieces of
//! each exact curve inside both domains with their UV curves.
//!
//! Not upstream. An analytic special case of [`Surface::global_surface_intersect`] (a
//! sphere or cone with a plane, two spheres, ...) gives its exact line or conic made from
//! the analytic surfaces' numbers, clipped at most to an analytic domain, and no UV curves
//! (upstream's `AssociateOutputs`). Upstream's offset never meets them there: it intersects
//! fitted B-spline extensions, whose marched curves are bounded by the domains and carry UV
//! curves. A surface extended exactly keeps its analytic kind, so the special case answers,
//! and a curve may run across the other surface's seam or past its domain.
//! [`SurfaceIntersection::clip_exact_curves`] gives such a curve as the marched intersection
//! gives its boundary curves (`AddBoundaryCurve`): dropped and trimmed onto each surface over
//! its domain (the drop trace, which breaks the curve at the domain's boundaries and seams),
//! each overlap of the two surfaces' pieces one piece of the exact curve with its two UV
//! curves. A closed curve that crosses a seam or a domain boundary is first turned to start
//! at a crossing, so that its pieces end only at crossings and each is given once.
//!
//! What is checked, and how: each piece's control points are on both analytic surfaces to
//! the rounding of evaluating their implicit equations ([`curve_lies_on`]; not covering the
//! rounding of the knot insertion or the trim that made the piece). Each UV curve, as
//! stored after the trims, is lifted onto its surface's net (every analytic surface is its
//! net, and the drop gives UV curves in the net's parameters) and the distance of the lift
//! from the piece at the same parameter is enclosed in rounded intervals over the whole
//! piece ([`certified_lift`]): accepted when the enclosure is within the tolerance, which
//! is then the piece's deviation. Anything else is refused with its typed error.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_section::certificate::{
    curve_derivs_over, nurbs_partials_over_piece, partials_hull, NurbsCurve,
};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, knot, span, Nurbs, Partials};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::SolutionObject;
use crate::intersect::surface_drop_lib::p3::DropSolveContext;
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::surface::implicit::{curve_lies_on, AnalyticSupport, Implicit};
use crate::surface::surface::p1::SurfaceIntersectContext;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceIntersection};

/// The operation [`SurfaceIntersection::clip_exact_curves`] names in its errors.
pub const OP: &str = "surface::surface::SurfaceIntersection::clip_exact_curves";

impl SurfaceIntersection {
    /// Each curve of this intersection of `surface` over `uv_domain` with `other` over
    /// `other_uv_domain` that came without its UV curves (an analytic special case's exact
    /// curve) replaced, in place, by its pieces inside both domains, each with its UV
    /// curves on both surfaces (module docs); its type is each piece's, and each piece's
    /// deviation is the certified bound on its UV curves' lifts ([`certified_lift`]).
    /// Each piece is given once, and ends only where the curve crosses a seam or a domain
    /// boundary of either surface: a closed curve crossing one is first turned to start at
    /// such a crossing ([`turned_to`]), so its own parameter origin splits nothing; a
    /// closed curve crossing none is one closed piece. A curve inside
    /// neither domain gives no piece. Curves with both UV curves and degenerate curves (a
    /// touching point) are kept as they are.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unsupported`] for a curve without UV curves whose surfaces are not
    /// both analytic, a piece not on both ([`curve_lies_on`]), a UV curve whose lift is
    /// not certified within `approx_tol_3d` of its piece ([`certified_lift`]), or a closed
    /// curve to be turned that is not an ellipse; [`SolidError::PortInvariant`] for a drop
    /// whose pieces overlap or a turned ellipse that does not start at the crossing; those
    /// of the drops, the trims, the check and the certificate.
    pub fn clip_exact_curves(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        approx_tol_3d: f64,
    ) -> Result<()> {
        self.clip_exact_curves_with_context::<NoObject>(
            k,
            None,
            surface,
            uv_domain,
            other,
            other_uv_domain,
            approx_tol_3d,
        )
    }

    /// [`SurfaceIntersection::clip_exact_curves`] whose drops run under the caller's
    /// face-aware cache `context` (the one the intersection ran under): its `handles`
    /// name `surface` and `other` in their caches, so the drop onto a face's surface
    /// traces on that face's cache (`smsurf_GetSurfaceCache` hands a face-owned surface
    /// its face's cache).
    ///
    /// # Errors
    ///
    /// Those of [`SurfaceIntersection::clip_exact_curves`] and of the context's cache
    /// preparation.
    #[allow(clippy::too_many_arguments)]
    pub fn clip_exact_curves_with_context<O: SolutionObject>(
        &mut self,
        k: &Kernel,
        mut context: Option<SurfaceIntersectContext<'_, O>>,
        surface: &Surface,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        approx_tol_3d: f64,
    ) -> Result<()> {
        let old = core::mem::take(self);
        let mut uv1 = old.surface1_uv_curves.into_iter();
        let mut uv2 = old.surface2_uv_curves.into_iter();
        let mut types = old.curve_types.into_iter();
        let mut deviations = old.deviations.into_iter();
        let pair = Pair {
            surface,
            uv_domain,
            other,
            other_uv_domain,
            approx_tol_3d,
        };
        for curve in old.curves_3d {
            let (a, b) = (uv1.next().flatten(), uv2.next().flatten());
            let (Some(curve_type), Some(deviation)) = (types.next(), deviations.next()) else {
                return Err(SolidError::port_invariant(
                    OP,
                    "fewer curve types or deviations than curves",
                ));
            };
            let keep = (a.is_some() && b.is_some()) || curve.is_degenerate(k, EFF_ZERO, None)?;
            if keep {
                self.curves_3d.push(curve);
                self.surface1_uv_curves.push(a);
                self.surface2_uv_curves.push(b);
                self.curve_types.push(curve_type);
                self.deviations.push(deviation);
                continue;
            }
            let implicits = [implicit_of(surface)?, implicit_of(other)?];
            let mut drops = pair.drops(k, &curve, context.as_mut())?;
            let mut curve = curve;
            if let Some(t) = drops.first_inner_break(&curve.natural_interval()) {
                if is_closed_curve(k, &curve, approx_tol_3d)? {
                    curve = turned_to(k, &curve, t, approx_tol_3d)?;
                    drops = pair.drops(k, &curve, context.as_mut())?;
                }
            }
            for (ivl, p1, p2) in drops.overlaps() {
                let mut piece = curve.copy(k)?;
                let mut piece1 = p1.copy(k)?;
                let mut piece2 = p2.copy(k)?;
                // each trim may snap the interval to a knot of its curve: the three are
                // trimmed to the one interval none of them snaps, so the 3D piece and its
                // UV curves end at the same parameters
                let ivl = common_trim_interval(k, [&piece, &piece1, &piece2], ivl, OP)?;
                for c in [&mut piece, &mut piece1, &mut piece2] {
                    trim_unsnapped(k, c, ivl, OP)?;
                }
                for implicit in &implicits {
                    if !on_implicit(&piece, implicit)? {
                        return Err(SolidError::Unsupported {
                            operation: OP,
                            reason: "a piece of an exact intersection curve is not on both \
                                     analytic surfaces",
                        });
                    }
                }
                let lift1 = certified_lift(k, surface, &piece1, &piece, ivl, approx_tol_3d)?;
                let lift2 = certified_lift(k, other, &piece2, &piece, ivl, approx_tol_3d)?;
                self.curves_3d.push(piece);
                self.surface1_uv_curves.push(Some(piece1));
                self.surface2_uv_curves.push(Some(piece2));
                self.curve_types.push(curve_type);
                self.deviations.push(lift1.max(lift2));
            }
        }
        Ok(())
    }
}

/// How many rounds [`common_trim_interval`] snaps the interval in before it is refused.
const TRIM_SNAP_ROUNDS: usize = 4;

/// `ivl` snapped by each of `curves`' trims in turn ([`TrimMode::IntervalOnly`]: each may
/// move an end onto a knot of its own), round after round until no trim moves it: the one
/// interval all of them trim to. A 3D curve and its UV curves trimmed in turn with one
/// interval passed on (`ivl` snapped by each trim given to the next) end at different
/// parameters wherever a later trim snaps, so an edge's interval (its 3D curve's) is not
/// its UV curves'; trimmed each to this interval ([`trim_unsnapped`]), they end together.
/// Also used by `OffsetGeometryCreation::mate_edges`, whose intersection curves may be
/// pieces of exact curves ([`SurfaceIntersection::clip_exact_curves`]): a conic's trim
/// snaps onto its NURB's knots, its UV curves' onto theirs. `op` names the caller.
///
/// # Errors
///
/// Those of the trims; a port invariant when the snaps still move it after
/// [`TRIM_SNAP_ROUNDS`] rounds (two curves pulling an end onto knots of their own).
pub(crate) fn common_trim_interval(
    k: &Kernel,
    curves: [&Curve; 3],
    ivl: Extent1d,
    op: &'static str,
) -> Result<Extent1d> {
    let mut ivl = ivl;
    for _ in 0..TRIM_SNAP_ROUNDS {
        let start = ivl;
        for c in curves {
            c.copy(k)?.trim(k, &mut ivl, TrimMode::IntervalOnly)?;
        }
        if ivl == start {
            return Ok(ivl);
        }
    }
    Err(SolidError::port_invariant(
        op,
        "the trims of a piece and its UV curves snap its interval to different knots",
    ))
}

/// `curve` trimmed ([`TrimMode::Notify`]) to `ivl`, an interval [`common_trim_interval`]
/// gave for it: its trim snaps nothing.
///
/// # Errors
///
/// Those of the trim; a port invariant when the trim snaps `ivl` all the same.
pub(crate) fn trim_unsnapped(
    k: &Kernel,
    curve: &mut Curve,
    ivl: Extent1d,
    op: &'static str,
) -> Result<()> {
    let mut trimmed = ivl;
    curve.trim(k, &mut trimmed, TrimMode::Notify)?;
    if trimmed != ivl {
        return Err(SolidError::port_invariant(
            op,
            "a trim snapped the interval its snap left unchanged",
        ));
    }
    Ok(())
}

/// The two surfaces of [`SurfaceIntersection::clip_exact_curves`] over their domains.
struct Pair<'a> {
    surface: &'a Surface,
    uv_domain: &'a Extent2d,
    other: &'a Surface,
    other_uv_domain: &'a Extent2d,
    approx_tol_3d: f64,
}

/// A curve's UV pieces on each surface of a [`Pair`], each list in parameter order and
/// without overlaps.
struct Drops {
    on1: Vec<Curve>,
    on2: Vec<Curve>,
}

impl Pair<'_> {
    /// `curve` dropped and trimmed onto both surfaces over their domains, under the
    /// caller's face-aware `context` when given.
    fn drops<O: SolutionObject>(
        &self,
        k: &Kernel,
        curve: &Curve,
        mut context: Option<&mut SurfaceIntersectContext<'_, O>>,
    ) -> Result<Drops> {
        let [h1, h2] = context.as_ref().map_or([None, None], |c| c.handles);
        let on1 = self.drop_onto(
            k,
            self.surface,
            self.uv_domain,
            curve,
            context.as_mut().map(|c| DropSolveContext {
                solver: &mut *c.solver,
                handle: h1,
            }),
        )?;
        let on2 = self.drop_onto(
            k,
            self.other,
            self.other_uv_domain,
            curve,
            context.map(|c| DropSolveContext {
                solver: &mut *c.solver,
                handle: h2,
            }),
        )?;
        Ok(Drops { on1, on2 })
    }

    /// `curve`'s UV pieces on `surface` over `domain`, in parameter order (each is
    /// certified on its surface after the trims, [`certified_lift`]). With a `context`
    /// (the face-aware service and the surface's handle in it), a face's surface traces
    /// on its face's cache, built by the context, and the trace's drops run under it.
    fn drop_onto<O: SolutionObject>(
        &self,
        k: &Kernel,
        surface: &Surface,
        domain: &Extent2d,
        curve: &Curve,
        context: Option<DropSolveContext<'_, O>>,
    ) -> Result<Vec<Curve>> {
        let mut from_context;
        let (cache, context) = match context {
            Some(DropSolveContext { solver, handle }) => {
                let cache = if surface.face()?.is_some() {
                    from_context = solver.build_surface_cache(k, surface, handle)?;
                    Some(&mut from_context)
                } else {
                    None
                };
                (cache, Some(DropSolveContext { solver, handle }))
            }
            None => (None, None),
        };
        let on = surface.drop_and_trim_curve_in_context::<NoObject, O>(
            k,
            cache,
            domain,
            curve,
            &curve.natural_interval(),
            self.approx_tol_3d,
            true,
            true,
            None,
            context,
        )?;
        let mut pieces = on.uv_curves;
        pieces.sort_by(|a, b| {
            a.natural_interval()
                .min()
                .total_cmp(&b.natural_interval().min())
        });
        // the trace breaks the curve at the crossings: consecutive pieces at most meet
        let mut ends = pieces.iter().map(Curve::natural_interval);
        if let Some(mut before) = ends.next() {
            for ivl in ends {
                if ivl.min() < before.max() {
                    return Err(SolidError::port_invariant(
                        OP,
                        "the drop of an exact intersection curve gave overlapping pieces",
                    ));
                }
                before = ivl;
            }
        }
        Ok(pieces)
    }
}

impl Drops {
    /// The first end of a piece on either surface inside `interval`: where the curve
    /// crosses a seam or a domain boundary other than at its own ends. An end nearer an
    /// end of `interval` than the drop's own break distance (`SM_EFF_ZERO_SQRT / 100`,
    /// below which `FindBoundaryStartPoints` takes a crossing for the end) is the end.
    fn first_inner_break(&self, interval: &Extent1d) -> Option<f64> {
        let near = EFF_ZERO_SQRT / 100.0;
        self.on1
            .iter()
            .chain(&self.on2)
            .flat_map(|c| {
                let ivl = c.natural_interval();
                [ivl.min(), ivl.max()]
            })
            .filter(|&t| t > interval.min() + near && t < interval.max() - near)
            .min_by(f64::total_cmp)
    }

    /// Each overlap of a piece on the first surface with one on the second, once, in
    /// parameter order: a sweep over both ordered lists.
    fn overlaps(&self) -> Vec<(Extent1d, &Curve, &Curve)> {
        let mut out = Vec::new();
        let (mut i, mut j) = (0, 0);
        while let (Some(p1), Some(p2)) = (self.on1.get(i), self.on2.get(j)) {
            let (i1, i2) = (p1.natural_interval(), p2.natural_interval());
            let (lo, hi) = (i1.min().max(i2.min()), i1.max().min(i2.max()));
            if hi > lo {
                out.push((Extent1d::from_min_max(lo, hi), p1, p2));
            }
            // the piece that ends first meets nothing further on the other list
            if i1.max() <= i2.max() {
                i += 1;
            } else {
                j += 1;
            }
        }
        out
    }
}

/// Whether `curve` is closed: an ellipse all the way round, or another curve whose ends
/// are within `tol` ([`Curve::is_closed`]).
fn is_closed_curve(k: &Kernel, curve: &Curve, tol: f64) -> Result<bool> {
    match curve.kind() {
        // the full turn as the ellipse's own largest domain takes it
        CurveKind::Ellipse(e) => Ok(e.step_interval().length() > 360.0 - EFF_ZERO),
        _ => curve.is_closed(k, &curve.natural_interval(), tol),
    }
}

/// The closed `curve` (an ellipse all the way round) turned to start at its parameter
/// `t`: the same ellipse (placement, radii, class, sense) over the full turn from the
/// angle of `t`, so its points are the same to rounding and its parameter origin is at
/// `t`.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a closed curve that is not an ellipse;
/// [`SolidError::PortInvariant`] for a turned ellipse without NURB or not starting
/// within `tol` of the curve's point at `t`; those of the angle and the constructor.
fn turned_to(k: &Kernel, curve: &Curve, t: f64, tol: f64) -> Result<Curve> {
    let CurveKind::Ellipse(e) = curve.kind() else {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a closed exact intersection curve crossing a seam or domain boundary \
                     that is not an ellipse",
        });
    };
    let angle = e.convert_t_from_nurbs_to_step(k, t)?;
    // an inside-out ellipse runs from its largest angle down
    let domain = if e.inside_out() {
        Extent1d::from_min_max(angle - 360.0, angle)
    } else {
        Extent1d::from_min_max(angle, angle + 360.0)
    };
    let opts = EllipseOptions {
        dim: e.base().dim(),
        nurb: None,
        inside_out: e.inside_out(),
    };
    let at = e.position();
    let (o, x, y) = (at.origin(), at.x_axis(), at.y_axis());
    let turned = if e.is_circle_class() {
        Ellipse::new_circle(k, o, x, y, domain, e.x_radius(), &opts)?
    } else {
        Ellipse::new(k, o, x, y, domain, e.x_radius(), e.y_radius(), &opts)?
    };
    if turned.base().nurb.is_none() {
        return Err(SolidError::port_invariant(
            OP,
            "a turned exact intersection ellipse has no NURB",
        ));
    }
    let turned = Curve::from(turned);
    let start = turned.evaluate_point(k, turned.natural_interval().min())?;
    if start.distance(curve.evaluate_point(k, t)?) > tol {
        return Err(SolidError::port_invariant(
            OP,
            "a turned exact intersection ellipse does not start at the crossing",
        ));
    }
    Ok(turned)
}

/// The implicit equation of an analytic `surface`.
fn implicit_of(surface: &Surface) -> Result<Implicit> {
    AnalyticSupport::of(surface)
        .map(|s| s.implicit())
        .ok_or(SolidError::Unsupported {
            operation: OP,
            reason: "an intersection curve without UV curves on a surface that is not analytic",
        })
}

/// Whether `curve`, a B-spline kind, lies on `implicit` ([`curve_lies_on`]).
fn on_implicit(curve: &Curve, implicit: &Implicit) -> Result<bool> {
    match curve.kind() {
        CurveKind::BSpline(b) => curve_lies_on(b, implicit),
        CurveKind::Line(l) => curve_lies_on(&*l.base_with_nurb()?, implicit),
        CurveKind::Ellipse(e) => curve_lies_on(e.base(), implicit),
        CurveKind::Hyperbola(h) => curve_lies_on(h.base(), implicit),
        CurveKind::Parabola(p) => curve_lies_on(p.base(), implicit),
        _ => Err(SolidError::Unsupported {
            operation: OP,
            reason: "an exact intersection curve that is not of a B-spline kind",
        }),
    }
}

/// The deepest a part of [`certified_lift`] is halved before the lift is refused.
const LIFT_MAX_DEPTH: usize = 52;

/// The most parts [`certified_lift`] encloses before the lift is refused.
const LIFT_MAX_PARTS: usize = 1 << 16;

/// The refusal of a lift [`certified_lift`] cannot bound within the tolerance.
const fn lift_not_certified() -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason: "a UV curve's lift is not certified within the tolerance of its exact \
                 intersection curve",
    }
}

/// An upper bound, enclosed, on `|S(uv(t)) - C(t)|` over `ivl`: `S` is `surface`'s net
/// ([`as_b_spline`], [`Nurbs::of`]; past its knots, the analytic continuation of its end
/// spans, as [`Nurbs`] evaluates), `uv` a UV curve on it and `C` the 3D `curve`, both in
/// the same parameter. `ivl` is cut at the inner knots of `uv` and `C`, so each part lies
/// on one polynomial piece of both, and each part is enclosed ([`lift_over`]) and halved
/// until its bound is at most `tol`. The bound returned is the largest part's.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a surface without a net, a curve not of a B-spline
/// kind, a lift proved farther than `tol` at a part's middle, or a part still above `tol`
/// after [`LIFT_MAX_DEPTH`] halvings or past [`LIFT_MAX_PARTS`] parts;
/// [`SolidError::PortInvariant`] for an empty `ivl`; those of the enclosures.
fn certified_lift(
    k: &Kernel,
    surface: &Surface,
    uv: &Curve,
    curve: &Curve,
    ivl: Extent1d,
    tol: f64,
) -> Result<f64> {
    let (lo, hi) = (ivl.min(), ivl.max());
    let has_length = lo < hi;
    if !has_length {
        return Err(SolidError::port_invariant(
            OP,
            "an exact intersection piece over an empty interval",
        ));
    }
    let bs = as_b_spline(k, surface)?.ok_or(SolidError::Unsupported {
        operation: OP,
        reason: "a UV curve on a surface without a NURBS net",
    })?;
    let net = Nurbs::of(&bs)?;
    let (uvn, cn) = (nurbs_curve_of(uv)?, nurbs_curve_of(curve)?);
    let mut cuts = vec![lo, hi];
    cuts.extend(uvn.u.iter().chain(&cn.u).filter(|&&t| t > lo && t < hi));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut todo: Vec<(f64, f64, usize)> = cuts
        .windows(2)
        .rev()
        .filter_map(|w| match *w {
            [a, b] => Some((a, b, 0)),
            _ => None,
        })
        .collect();
    let (mut parts, mut worst) = (0_usize, 0.0_f64);
    while let Some((a, b, depth)) = todo.pop() {
        parts += 1;
        if parts > LIFT_MAX_PARTS {
            return Err(lift_not_certified());
        }
        let part = lift_over(&net, &uvn, &cn, a, b)?;
        if part.bound <= tol {
            worst = worst.max(part.bound);
            continue;
        }
        if part.at_middle > tol {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a UV curve's lift is farther than the tolerance from its exact \
                         intersection curve",
            });
        }
        let m = 0.5 * (a + b);
        let halves = m > a && m < b;
        if depth >= LIFT_MAX_DEPTH || !halves {
            return Err(lift_not_certified());
        }
        todo.push((m, b, depth + 1));
        todo.push((a, m, depth + 1));
    }
    Ok(worst)
}

/// A part's enclosure ([`lift_over`]): the bound on the lift's distance over the part,
/// and a proved lower bound on it at the part's middle.
struct LiftPart {
    bound: f64,
    at_middle: f64,
}

/// `g(t) = S(uv(t)) - C(t)` over `[a, b]` (inside one polynomial piece of `uv` and of `C`,
/// [`certified_lift`]), enclosed in rounded intervals with `m` the middle and `r` the
/// half-width. Where the box `uv([a, b])` crosses no inner knot line of the net, `g` is
/// `C^2` there and its second-order Taylor form holds: `g(t)` is in
/// `g(m) + g'(m) [-r, r] + g''([a, b]) [0, r^2] / 2`, with
/// `g'' = S_uu u'^2 + 2 S_uv u'v' + S_vv v'^2 + S_u u'' + S_v v'' - C''`. Where it crosses
/// one (a circle's net has `C^0` double knots), `g` is continuous and `C^1` on each side,
/// and its mean-value form holds: `g(t)` is in `g(m) + g'([a, b]) [-r, r]`, with each
/// partial enclosed piece by piece and hulled ([`net_partials`]). The bound is the norm of
/// the enclosure's magnitudes, rounded up.
///
/// # Errors
///
/// [`SolidError::PortInvariant`] for a missing derivative; those of
/// [`curve_derivs_over`], [`net_partials`] and [`net_parts`].
fn lift_over(net: &Nurbs, uvn: &NurbsCurve, cn: &NurbsCurve, a: f64, b: f64) -> Result<LiftPart> {
    let m = 0.5 * (a + b);
    let t = Ivl::new(a, b);
    let dt = t - Ivl::from(m);
    let r = Ivl::from(dt.mag());
    let missing = || SolidError::port_invariant(OP, "a curve's derivative over a part is missing");
    let uv_t = curve_derivs_over(uvn, t, 2, None)?;
    let c_t = curve_derivs_over(cn, t, 2, None)?;
    let uv_m = curve_derivs_over(uvn, Ivl::from(m), 1, None)?;
    let c_m = curve_derivs_over(cn, Ivl::from(m), 1, None)?;
    let at = |v: &[V3<Ivl>], i: usize| v.get(i).copied().ok_or_else(missing);
    let (w0, w1, w2) = (at(&uv_t, 0)?, at(&uv_t, 1)?, at(&uv_t, 2)?);
    let (c1, c2) = (at(&c_t, 1)?, at(&c_t, 2)?);
    let (wm0, wm1) = (at(&uv_m, 0)?, at(&uv_m, 1)?);
    let (cm0, cm1) = (at(&c_m, 0)?, at(&c_m, 1)?);
    let pu = net_parts(&net.u, net.p, net.nu, w0.x)?;
    let pv = net_parts(&net.v, net.q, net.nv, w0.y)?;
    let (g, g_m) = if one_piece(&pu) && one_piece(&pv) {
        let s_t = net_partials(net, &pu, &pv, 2)?;
        // uv(m) lies in uv([a, b]), so on the same pieces: each one's polynomial is the
        // net there
        let pm = |parts: &[(f64, f64, usize)], x: Ivl| -> Vec<(f64, f64, usize)> {
            parts.iter().map(|&(_, _, s)| (x.lo, x.hi, s)).collect()
        };
        let s_m = net_partials(net, &pm(&pu, wm0.x), &pm(&pv, wm0.y), 1)?;
        let g_m = v_sub(s_m.at(0, 0), cm0);
        let dg_m = v_sub(
            v_add(v_scale(s_m.at(1, 0), wm1.x), v_scale(s_m.at(0, 1), wm1.y)),
            cm1,
        );
        let (u1, v1) = (w1.x, w1.y);
        let two = Ivl::from(2.0);
        let ddg = v_sub(
            v_add(
                v_add(
                    v_add(
                        v_scale(s_t.at(2, 0), u1 * u1),
                        v_scale(s_t.at(1, 1), two * u1 * v1),
                    ),
                    v_scale(s_t.at(0, 2), v1 * v1),
                ),
                v_add(v_scale(s_t.at(1, 0), w2.x), v_scale(s_t.at(0, 1), w2.y)),
            ),
            c2,
        );
        let half_r2 = Ivl::new(0.0, (r * r * Ivl::from(0.5)).hi);
        let g = v_add(v_add(g_m, v_scale(dg_m, dt)), v_scale(ddg, half_r2));
        (g, g_m)
    } else {
        let s_t = net_partials(net, &pu, &pv, 1)?;
        let s_m = net_partials(
            net,
            &net_parts(&net.u, net.p, net.nu, wm0.x)?,
            &net_parts(&net.v, net.q, net.nv, wm0.y)?,
            0,
        )?;
        let g_m = v_sub(s_m.at(0, 0), cm0);
        let dg = v_sub(
            v_add(v_scale(s_t.at(1, 0), w1.x), v_scale(s_t.at(0, 1), w1.y)),
            c1,
        );
        (v_add(g_m, v_scale(dg, dt)), g_m)
    };
    Ok(LiftPart {
        bound: norm_above(g),
        at_middle: norm_below(g_m),
    })
}

/// Whether the parts of one direction of a box ([`net_parts`]) are on one piece of the
/// net: one part, or a point on a knot line given with the spans on both sides (the net
/// restricted to the line is the same function from either side).
fn one_piece(parts: &[(f64, f64, usize)]) -> bool {
    parts.len() == 1 || parts.iter().all(|&(a, b, _)| a == b)
}

/// The parts of `x` (one direction of a box) between the inner knots of `knots` (degree
/// `p`, `n` control points) strictly inside it, each with the span whose polynomial is the
/// net on it ([`span`]: past the knots' range, the end span continued). A point on an
/// inner knot is given twice, with the span on each side.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for an `x` that is not an interval (a NaN end), or one
/// across an inner knot where the net is not continuous (a multiplicity above `p`).
fn net_parts(knots: &[f64], p: usize, n: usize, x: Ivl) -> Result<Vec<(f64, f64, usize)>> {
    // a NaN end fails it too
    let is_interval = x.lo <= x.hi;
    if !is_interval {
        return Err(lift_not_certified());
    }
    let (first, last) = (knot(knots, p as isize), knot(knots, n as isize));
    let inner = |y: f64| y > first && y < last;
    if x.lo == x.hi && inner(x.lo) && knots.contains(&x.lo) {
        let after = span(knots, p, n, x.lo);
        let mut before = after.checked_sub(1).ok_or_else(lift_not_certified)?;
        while before > p && knot(knots, before as isize) >= knot(knots, before as isize + 1) {
            before -= 1;
        }
        return Ok(vec![(x.lo, x.hi, before), (x.lo, x.hi, after)]);
    }
    let mut at = vec![x.lo];
    for &y in knots {
        if inner(y) && y > x.lo && y < x.hi && at.last().is_none_or(|&l| l < y) {
            if knots.iter().filter(|&&z| z == y).count() > p {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a UV curve across a knot line where its surface's net is not \
                             continuous",
                });
            }
            at.push(y);
        }
    }
    at.push(x.hi);
    Ok(at
        .windows(2)
        .filter_map(|w| match *w {
            [a, b] => Some((a, b, span(knots, p, n, 0.5 * (a + b)))),
            _ => None,
        })
        .collect())
}

/// The net's partials to total order `lk` over every part of the box `pu` x `pv`
/// ([`net_parts`]), each on its own piece ([`nurbs_partials_over_piece`]), hulled
/// ([`partials_hull`]).
///
/// # Errors
///
/// Those of [`nurbs_partials_over_piece`]; a port invariant for a box without parts.
fn net_partials(
    net: &Nurbs,
    pu: &[(f64, f64, usize)],
    pv: &[(f64, f64, usize)],
    lk: usize,
) -> Result<Partials<Ivl>> {
    let mut out: Option<Partials<Ivl>> = None;
    for &(ua, ub, ku) in pu {
        for &(va, vb, kv) in pv {
            let p = nurbs_partials_over_piece(net, ku, kv, ua, ub, va, vb, lk)?;
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.ok_or_else(|| SolidError::port_invariant(OP, "a box of a UV curve without parts"))
}

/// The NURBS data of `curve`, a B-spline kind (as [`on_implicit`] takes it).
fn nurbs_curve_of(curve: &Curve) -> Result<NurbsCurve> {
    match curve.kind() {
        CurveKind::BSpline(b) => NurbsCurve::of(b),
        CurveKind::Line(l) => NurbsCurve::of(&*l.base_with_nurb()?),
        CurveKind::Ellipse(e) => NurbsCurve::of(e.base()),
        CurveKind::Hyperbola(h) => NurbsCurve::of(h.base()),
        CurveKind::Parabola(p) => NurbsCurve::of(p.base()),
        _ => Err(SolidError::Unsupported {
            operation: OP,
            reason: "a curve of an exact intersection piece that is not of a B-spline kind",
        }),
    }
}

/// `a + b`, componentwise.
fn v_add(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

/// `a - b`, componentwise.
fn v_sub(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `a s`, componentwise.
fn v_scale(a: V3<Ivl>, s: Ivl) -> V3<Ivl> {
    V3 {
        x: a.x * s,
        y: a.y * s,
        z: a.z * s,
    }
}

/// An upper bound on the length of every vector in `v`, rounded up (infinite or NaN for an
/// undefined enclosure, which no tolerance passes).
fn norm_above(v: V3<Ivl>) -> f64 {
    let m = |x: Ivl| Ivl::from(x.mag());
    let (x, y, z) = (m(v.x), m(v.y), m(v.z));
    (x * x + y * y + z * z).sqrt().hi
}

/// A lower bound on the length of every vector in `v`, rounded down (0 where it holds 0).
fn norm_below(v: V3<Ivl>) -> f64 {
    let m = |x: Ivl| Ivl::from(x.mig());
    let (x, y, z) = (m(v.x), m(v.y), m(v.z));
    let s = x * x + y * y + z * z;
    if s.lo.is_nan() {
        return 0.0;
    }
    s.sqrt().lo
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Of a piece and its UV curves, only the last trimmed has a knot a rounding past the
    /// interval's end, which its trim snaps the end onto. The three are trimmed to the one
    /// interval none of them snaps, the snapped one, so they end at the same parameter
    /// (trimmed in turn with one interval passed on, the first two ended at the unsnapped
    /// end).
    #[test]
    fn a_later_trims_knot_snap_is_given_to_the_curves_trimmed_before() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let k = Kernel::default();
        let knot = 0.5 + 2.0 * f64::EPSILON;
        let line = |dim: u64, knots: Vec<f64>, n: usize| {
            let pts = (0..n)
                .map(|i| CPoint::new(i as f64, 1.0, 0.0, NOW))
                .collect();
            Curve::from(BSplineCurve::from_nurb(
                NlCurve::new(pts, 1, knots).unwrap(),
                dim,
                false,
            ))
        };
        let piece = line(3, vec![0.0, 0.0, 1.0, 1.0], 2);
        let uv1 = line(2, vec![0.0, 0.0, 1.0, 1.0], 2);
        let uv2 = line(2, vec![0.0, 0.0, knot, 1.0, 1.0], 3);
        let ivl = Extent1d::from_min_max(0.25, 0.5);
        let common = common_trim_interval(&k, [&piece, &uv1, &uv2], ivl, OP).unwrap();
        assert_eq!((common.min(), common.max()), (0.25, knot));
        let ends: Vec<f64> = [piece, uv1, uv2]
            .into_iter()
            .map(|mut c| {
                let mut at = common;
                c.trim(&k, &mut at, TrimMode::Notify).unwrap();
                assert_eq!(at, common);
                c.natural_interval().max()
            })
            .collect();
        assert_eq!(ends, vec![knot; 3]);
    }

    /// `mate_edges`' order: the 3D curve trimmed first has no knot near the interval, one
    /// UV curve a knot a rounding past its start, the other a rounding past its end. Each
    /// trim snaps its own end, so trimmed in turn with one interval passed on the 3D curve
    /// keeps both unsnapped ends and the first UV curve the unsnapped end: the edge's
    /// interval would not be its UV curves'. Trimmed to the common interval
    /// ([`trim_unsnapped`]) all three end together, and a trim to an interval its curve
    /// still snaps is refused.
    #[test]
    fn a_3d_curve_and_its_uv_curves_trimmed_to_the_common_interval_end_together() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let k = Kernel::default();
        let start = 0.25 + 2.0 * f64::EPSILON;
        let end = 0.5 + 2.0 * f64::EPSILON;
        let line = |dim: u64, knots: Vec<f64>, n: usize| {
            let pts = (0..n)
                .map(|i| CPoint::new(i as f64, 1.0, 0.0, NOW))
                .collect();
            Curve::from(BSplineCurve::from_nurb(
                NlCurve::new(pts, 1, knots).unwrap(),
                dim,
                false,
            ))
        };
        let c3 = line(3, vec![0.0, 0.0, 1.0, 1.0], 2);
        let uv1 = line(2, vec![0.0, 0.0, start, 1.0, 1.0], 3);
        let uv2 = line(2, vec![0.0, 0.0, end, 1.0, 1.0], 3);
        let ivl = Extent1d::from_min_max(0.25, 0.5);

        // in turn, one interval passed on: the 3D curve ends where no UV curve does
        let mut passed = ivl;
        let in_turn: Vec<Extent1d> = [&c3, &uv1, &uv2]
            .into_iter()
            .map(|c| {
                let mut c = c.copy(&k).unwrap();
                c.trim(&k, &mut passed, TrimMode::Notify).unwrap();
                c.natural_interval()
            })
            .collect();
        assert_eq!(
            (in_turn[0].min(), in_turn[0].max()),
            (0.25, 0.5),
            "the 3D curve keeps the unsnapped ends"
        );
        assert_eq!((in_turn[2].min(), in_turn[2].max()), (start, end));

        let common = common_trim_interval(&k, [&c3, &uv1, &uv2], ivl, OP).unwrap();
        assert_eq!((common.min(), common.max()), (start, end));
        for c in [&c3, &uv1, &uv2] {
            let mut c = c.copy(&k).unwrap();
            trim_unsnapped(&k, &mut c, common, OP).unwrap();
            assert_eq!(c.natural_interval(), common);
        }
        let mut snapping = uv2.copy(&k).unwrap();
        assert!(matches!(
            trim_unsnapped(&k, &mut snapping, ivl, OP),
            Err(SolidError::PortInvariant { .. })
        ));
    }
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector2d::Vec2;
    use crate::geom::vector3d::Vec3;
    use crate::surface::plane::Plane;
    use crate::surface::sphere::Sphere;

    /// A whole sphere of radius 5 about the origin, its seam the half plane `y = 0`,
    /// `x > 0` (`u` 0 and 1).
    fn sphere(k: &Kernel) -> Surface {
        let frame = Axis2Placement::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        Surface::from(Sphere::create_canonical(k, &frame, 5.0).expect("sphere"))
    }

    fn plane(origin: Vec3, x: Vec3, y: Vec3) -> Surface {
        Surface::from(Plane::create_canonical(&Axis2Placement::new(origin, x, y)).expect("plane"))
    }

    /// The sphere and a plane through `(-0.5, 0, 0)` tilted about `y`, which cuts it in a
    /// circle round the sphere's axis (the axis meets the plane at `z = -5/3`) that starts
    /// off the seam and crosses it once, at `z` about `-4.9`.
    fn sphere_and_plane(k: &Kernel) -> (Surface, Surface) {
        let tilted = plane(
            Vec3::new(-0.5, 0.0, 0.0),
            Vec3::new(-0.3, 0.0, 1.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        (sphere(k), tilted)
    }

    /// The plane `z = 2`: its circle on the sphere starts on the seam.
    fn plane_z2() -> Surface {
        plane(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
    }

    fn intersect(
        k: &Kernel,
        s: &Surface,
        o: &Surface,
    ) -> (SurfaceIntersection, Extent2d, Extent2d) {
        let d2 = o.natural_uv_domain_with_nurb(k).expect("domain");
        intersect_over(k, s, o, d2)
    }

    fn intersect_over(
        k: &Kernel,
        s: &Surface,
        o: &Surface,
        d2: Extent2d,
    ) -> (SurfaceIntersection, Extent2d, Extent2d) {
        let d1 = s.natural_uv_domain_with_nurb(k).expect("domain");
        let ssi = s
            .global_surface_intersect(k, &d1, o, &d2, [true, true], Some(1e-6), None)
            .expect("ssi");
        (ssi, d1, d2)
    }

    /// Whether `uv` is on a side of `d`.
    fn on_side(uv: Vec2, d: &Extent2d) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + b.abs());
        near(uv.x, d.u_min())
            || near(uv.x, d.u_max())
            || near(uv.y, d.v_min())
            || near(uv.y, d.v_max())
    }

    /// The pieces of the special case's one exact circle of `sphere` with `other` over
    /// `d2`, checked: each piece's UV curves are on the piece (sampled) and the piece is
    /// on both surfaces ([`curve_lies_on`]); no two pieces overlap; each end of a piece
    /// is a crossing (on a side of either domain) unless `closed_free`, when the circle
    /// crosses nothing and is one closed piece. The pieces' 3D ends, in order.
    fn checked_pieces(
        k: &Kernel,
        sphere: &Surface,
        other: &Surface,
        d2: Extent2d,
        closed_free: bool,
    ) -> Vec<(Vec3, Vec3)> {
        let (mut ssi, d1, d2) = intersect_over(k, sphere, other, d2);
        // the special case: one whole exact circle, no UV curves
        assert_eq!(ssi.curves_3d.len(), 1);
        assert!(ssi.surface1_uv_curves[0].is_none() && ssi.surface2_uv_curves[0].is_none());
        ssi.clip_exact_curves(k, sphere, &d1, other, &d2, 1e-6)
            .expect("clip");
        let mut ends = Vec::new();
        let mut intervals = Vec::new();
        for (i, piece) in ssi.curves_3d.iter().enumerate() {
            let ivl = piece.natural_interval();
            let uv1 = ssi.surface1_uv_curves[i]
                .as_ref()
                .expect("uv on the sphere");
            let uv2 = ssi.surface2_uv_curves[i].as_ref().expect("uv on the other");
            // the deviation is the certified bound: within the tolerance, and at least
            // every sampled distance of the lifts from the piece
            let deviation = ssi.deviations[i];
            assert!(deviation <= 1e-6, "deviation {deviation}");
            for j in 0..=200 {
                let t = ivl.evaluate(f64::from(j) / 200.0);
                let p = piece.evaluate_point(k, t).expect("point");
                for (uv, s) in [(uv1, sphere), (uv2, other)] {
                    let q = uv.evaluate_point(k, t).expect("uv");
                    let on = s.evaluate_point(k, Vec2::new(q.x, q.y)).expect("on");
                    let d = (on - p).length();
                    assert!(d <= deviation + 1e-12, "{d} above the bound {deviation}");
                }
            }
            for j in 0..=8 {
                let t = ivl.evaluate(f64::from(j) / 8.0);
                let p = piece.evaluate_point(k, t).expect("point");
                for (uv, s) in [(uv1, sphere), (uv2, other)] {
                    let q = uv.evaluate_point(k, t).expect("uv");
                    let on = s.evaluate_point(k, Vec2::new(q.x, q.y)).expect("on");
                    assert!((on - p).length() <= 1e-6, "{} off", (on - p).length());
                }
            }
            for s in [sphere, other] {
                assert!(on_implicit(piece, &implicit_of(s).unwrap()).unwrap());
            }
            for t in [ivl.min(), ivl.max()] {
                let (a, b) = (
                    uv1.evaluate_point(k, t).expect("uv"),
                    uv2.evaluate_point(k, t).expect("uv"),
                );
                let crossing =
                    on_side(Vec2::new(a.x, a.y), &d1) || on_side(Vec2::new(b.x, b.y), &d2);
                assert!(
                    crossing || closed_free,
                    "piece {i} ends at {t}, not at a crossing"
                );
            }
            intervals.push(ivl);
            ends.push((
                piece.evaluate_point(k, ivl.min()).expect("start"),
                piece.evaluate_point(k, ivl.max()).expect("end"),
            ));
        }
        for (i, a) in intervals.iter().enumerate() {
            for b in intervals.iter().skip(i + 1) {
                assert!(a.max() <= b.min() || b.max() <= a.min(), "pieces overlap");
            }
        }
        ends
    }

    fn same(a: Vec3, b: Vec3) -> bool {
        (a - b).length() <= 1e-12 * 5.0
    }

    #[test]
    fn a_circle_across_the_seam_once_is_one_closed_piece_from_the_seam() {
        let k = Kernel::default();
        let (sphere, tilted) = sphere_and_plane(&k);
        let d2 = tilted.natural_uv_domain_with_nurb(&k).expect("domain");
        let ends = checked_pieces(&k, &sphere, &tilted, d2, false);
        // one seam-to-seam piece, all the way round (not split at the circle's own start)
        assert_eq!(ends.len(), 1);
        let (start, end) = ends[0];
        assert!(same(start, end));
        assert!(start.y.abs() <= 1e-9 && start.x > 0.0 && start.z < -4.8);
    }

    #[test]
    fn a_circle_starting_on_the_seam_is_one_piece_given_once() {
        let k = Kernel::default();
        let sphere = sphere(&k);
        let h = plane_z2();
        let d2 = h.natural_uv_domain_with_nurb(&k).expect("domain");
        let ends = checked_pieces(&k, &sphere, &h, d2, false);
        assert_eq!(ends.len(), 1);
        let (start, end) = ends[0];
        assert!(same(start, end));
        assert!(same(start, Vec3::new(21.0_f64.sqrt(), 0.0, 2.0)));
    }

    #[test]
    fn a_circle_starting_on_the_seam_over_a_quadrant_is_its_quarter_once() {
        let k = Kernel::default();
        let sphere = sphere(&k);
        let h = plane_z2();
        let quadrant = Extent2d::from_points(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
        let ends = checked_pieces(&k, &sphere, &h, quadrant, false);
        assert_eq!(ends.len(), 1);
        let r = 21.0_f64.sqrt();
        let (start, end) = ends[0];
        assert!(same(start, Vec3::new(r, 0.0, 2.0)), "{start:?}");
        assert!(same(end, Vec3::new(0.0, r, 2.0)), "{end:?}");
    }

    #[test]
    fn a_circle_across_the_seam_twice_is_two_pieces_between_the_crossings() {
        let k = Kernel::default();
        let sphere = sphere(&k);
        // the plane x = 1: a circle off the axis, across the seam at z = +-sqrt(24)
        let v = plane(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let d2 = v.natural_uv_domain_with_nurb(&k).expect("domain");
        let ends = checked_pieces(&k, &sphere, &v, d2, false);
        assert_eq!(ends.len(), 2);
        let z = 24.0_f64.sqrt();
        let (top, bottom) = (Vec3::new(1.0, 0.0, z), Vec3::new(1.0, 0.0, -z));
        // consecutive: the one's end the other's start, each a seam crossing
        assert!(same(ends[0].1, ends[1].0) && same(ends[1].1, ends[0].0));
        for (a, b) in ends {
            assert!((same(a, top) && same(b, bottom)) || (same(a, bottom) && same(b, top)));
        }
    }

    #[test]
    fn a_circle_crossing_no_seam_is_one_closed_piece() {
        let k = Kernel::default();
        let sphere = sphere(&k);
        // the plane y = 1: a circle on the side of the sphere away from its seam
        let w = plane(
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        );
        let d2 = w.natural_uv_domain_with_nurb(&k).expect("domain");
        let ends = checked_pieces(&k, &sphere, &w, d2, true);
        assert_eq!(ends.len(), 1);
        assert!(same(ends[0].0, ends[0].1));
    }

    /// The drop itself: a circle starting on the sphere's seam, on a `v` iso line, drops
    /// to that iso line once (the seam's other point is the same point of it).
    #[test]
    fn the_drop_of_an_iso_circle_starting_on_the_seam_gives_it_once() {
        let k = Kernel::default();
        let sphere = sphere(&k);
        let h = plane_z2();
        let (ssi, d1, _) = intersect(&k, &sphere, &h);
        let circle = &ssi.curves_3d[0];
        let on = sphere
            .drop_and_trim_curve::<NoObject>(
                &k,
                &d1,
                circle,
                &circle.natural_interval(),
                1e-6,
                true,
                true,
                None,
            )
            .expect("drop");
        assert_eq!(on.uv_curves.len(), 1);
    }

    /// A full circular edge (a hemisphere's base circle `z = 2` when its base plane moves
    /// there) is covered by one piece: trimming that piece to the closed edge's whole span
    /// gives the circle, so the edge is mated.
    #[test]
    fn a_full_circular_edge_is_mateable_on_its_one_piece() {
        let k = Kernel::default();
        let sphere = sphere(&k);
        let h = plane_z2();
        let (mut ssi, d1, d2) = intersect(&k, &sphere, &h);
        let whole = ssi.curves_3d[0].natural_interval();
        ssi.clip_exact_curves(&k, &sphere, &d1, &h, &d2, 1e-6)
            .expect("clip");
        assert_eq!(ssi.curves_3d.len(), 1);
        let mut piece = ssi.curves_3d[0].copy(&k).expect("copy");
        let mut span = piece.natural_interval();
        assert!(span == whole, "{span:?}");
        piece
            .trim(&k, &mut span, TrimMode::Notify)
            .expect("trimmed to the closed edge");
        let r = 21.0_f64.sqrt();
        for j in 0..=16 {
            let p = piece
                .evaluate_point(&k, span.evaluate(f64::from(j) / 16.0))
                .expect("p");
            assert!(((p - Vec3::new(0.0, 0.0, 2.0)).length() - r).abs() <= 1e-12 * 5.0);
            assert!((p.z - 2.0).abs() <= 1e-12 * 5.0);
        }
    }

    #[test]
    fn a_circle_moved_off_its_sphere_is_refused() {
        let k = Kernel::default();
        let (sphere, plane) = sphere_and_plane(&k);
        let (ssi, _, _) = intersect(&k, &sphere, &plane);
        let circle = &ssi.curves_3d[0];
        let on = implicit_of(&sphere).unwrap();
        assert!(on_implicit(circle, &on).unwrap());
        // the same sphere 1e-9 larger: the circle is not on it
        let Implicit::Sphere { centre, radius } = on else {
            panic!("a sphere")
        };
        let moved = Implicit::Sphere {
            centre,
            radius: radius + 1e-9,
        };
        assert!(!on_implicit(circle, &moved).unwrap());
    }

    /// The flat net `S(u, v) = (u, v, 0)` over the unit square, the line `C(t) = (t, 1/2,
    /// 0)` over `[0, 1]`, and a cubic UV curve `(t, 1/2 + 3 c t (1 - t))`: its lift is
    /// `3 c t (1 - t)` from the line, largest (`3 c / 4`) at `t = 1/2`.
    fn bumped_lift(k: &Kernel, c: f64) -> Result<f64> {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW};
        use crate::nurbs::srf_basic::Surface as NlSurface;
        let net = (0..2)
            .map(|i| {
                (0..2)
                    .map(|j| CPoint::new(f64::from(i), f64::from(j), 0.0, NOW))
                    .collect()
            })
            .collect();
        let unit = vec![0.0, 0.0, 1.0, 1.0];
        let flat = Surface::from(BSplineSurface::from_nurb(
            NlSurface::new(net, 1, 1, unit.clone(), unit.clone()).unwrap(),
            false,
        ));
        let line = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(0.0, 0.5, 0.0, NOW),
                    CPoint::new(1.0, 0.5, 0.0, NOW),
                ],
                1,
                unit,
            )
            .unwrap(),
            3,
            false,
        ));
        let vs = [0.5, 0.5 + c, 0.5 + c, 0.5];
        let uv = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
                    .into_iter()
                    .zip(vs)
                    .map(|(u, v)| CPoint::new(u, v, 0.0, NOW))
                    .collect(),
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            2,
            false,
        ));
        certified_lift(k, &flat, &uv, &line, Extent1d::from_min_max(0.0, 1.0), 1e-6)
    }

    #[test]
    fn a_lift_off_its_curve_between_the_ends_is_refused() {
        let k = Kernel::default();
        // on the line at both ends, 1e-5 off it in the middle
        let err = bumped_lift(&k, 4.0 / 3.0 * 1e-5).unwrap_err();
        assert!(matches!(err, SolidError::Unsupported { operation: OP, .. }));
    }

    #[test]
    fn a_lift_within_the_tolerance_is_bounded_above_its_largest_distance() {
        let k = Kernel::default();
        let bound = bumped_lift(&k, 4.0 / 3.0 * 0.25e-6).unwrap();
        assert!((0.25e-6..=1e-6).contains(&bound), "{bound}");
    }

    /// The rounding the enclosure carries for this fixture: one part `[0, 1]` (no inner
    /// knots; its bound is under the tolerance at once), its Taylor form (`lift_over`)
    /// with `r = 1/2`, every interval operation widened by `2.3e-16 |x|` on each side
    /// (`round_down`, `round_up`), exact or not. The UV cubic's `u''([0, 1])`, from its
    /// Taylor polynomial at `1/2` with third-derivative basis values up to 18 in size, is
    /// about `4.4e-13` wide, times `r^2 / 2 = 1/8`: about `5.7e-14` in x, with `g(m)`
    /// (`1.5e-14`) and `g'(m) [-r, r]` (`2.7e-14`); y the same way, from `v''`. That
    /// gives x `9.89e-14`, y `5.38e-14`, and a norm of `1.1253786587070681e-13`: this
    /// operation sequence replayed in IEEE doubles gives that bound to the bit. The
    /// lift's true distance is the rounding of the control points `1/3` and `2/3`, under
    /// `1e-16`.
    const EXACT_LIFT_ROUNDING: f64 = 1.1253786587070681e-13;

    #[test]
    fn an_exact_lift_is_bounded_by_rounding() {
        let k = Kernel::default();
        let bound = bumped_lift(&k, 0.0).unwrap();
        assert!((0.0..=EXACT_LIFT_ROUNDING).contains(&bound), "{bound}");
    }

    #[test]
    fn a_curve_on_a_surface_that_is_not_analytic_is_refused() {
        let k = Kernel::default();
        let (sphere, plane) = sphere_and_plane(&k);
        let (mut ssi, d1, d2) = intersect(&k, &sphere, &plane);
        // the plane's own B-spline net: the same points, no analytic kind
        let crate::surface::surface::SurfaceKind::Plane(p) = plane.kind() else {
            panic!("a plane")
        };
        let nurbs = Surface::from(p.base().clone());
        let err = ssi
            .clip_exact_curves(&k, &sphere, &d1, &nurbs, &d2, 1e-6)
            .unwrap_err();
        assert!(matches!(err, SolidError::Unsupported { operation: OP, .. }));
    }
}
