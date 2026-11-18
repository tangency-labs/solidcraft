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
use crate::blend::fillet_section::interval::{cmax, cmin, Ivl};
use crate::blend::fillet_section::offset_box::{
    offset_crossed_knot_jumps, offset_partials_over_parts,
};
use crate::blend::fillet_section::series::{v_add, v_hull, v_scale, v_sub, V3, VZ};
use crate::blend::fillet_section::station::{as_b_spline, knot, span, Nurbs, Partials};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::ellipse::{Ellipse, EllipseOptions};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::{Extent2d, SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use crate::geom::solution_array::SolutionObject;
use crate::geom::tol::ScaledZero as _;
use crate::intersect::surface_drop_lib::p3::DropSolveContext;
use crate::kernel::Kernel;
use crate::math::{Point2, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use crate::nurbs::math::{NOW, NOZ};
use crate::surface::implicit::{curve_lies_on, AnalyticSupport, Implicit};
use crate::surface::surface::p1::SurfaceIntersectContext;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceIntersection, SurfaceKind};
use std::borrow::Cow;

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
                let ivl = common_trim_interval(k, &[&piece, &piece1, &piece2], ivl, OP)?;
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
    curves: &[&Curve],
    ivl: Extent1d,
    op: &'static str,
) -> Result<Extent1d> {
    let mut ivl = ivl;
    for _ in 0..TRIM_SNAP_ROUNDS {
        // a round in which no curve snaps the interval: every one of them accepts it
        let mut snapped = false;
        for &c in curves {
            let before = ivl;
            c.copy(k)?.trim(k, &mut ivl, TrimMode::IntervalOnly)?;
            snapped |= ivl != before;
        }
        if !snapped {
            return Ok(ivl);
        }
    }
    // the curves' snaps disagree: knots of theirs at one parameter a few ulps apart, each
    // snapping the end to its own. The first curve's snap is the interval; each other one
    // is reparametrised onto it ([`trim_unsnapped`])
    let first = curves
        .first()
        .ok_or_else(|| SolidError::port_invariant(op, "a common trim interval of no curves"))?;
    first.copy(k)?.trim(k, &mut ivl, TrimMode::IntervalOnly)?;
    Ok(ivl)
}

/// `curve` trimmed ([`TrimMode::Notify`]) to `ivl`, an interval [`common_trim_interval`]
/// gave for it. Where its own trim snaps an end to a knot of its own within the trim's snap
/// tolerance of `ivl` (100 scaled zeros, the same parameter as the other curves' knot there,
/// a few ulps apart), a B-spline curve is reparametrised linearly onto `ivl`, every point of
/// it kept, so it ends at the same parameters as the others.
///
/// # Errors
///
/// Those of the trim and the reparametrisation; a port invariant when the trim snaps `ivl`
/// farther than that, or the curve is no B-spline.
pub(crate) fn trim_unsnapped(
    k: &Kernel,
    curve: &mut Curve,
    ivl: Extent1d,
    op: &'static str,
) -> Result<()> {
    let mut trimmed = ivl;
    curve.trim(k, &mut trimmed, TrimMode::Notify)?;
    if trimmed == ivl {
        return Ok(());
    }
    let near = |a: f64, b: f64| (a - b).abs() < 100.0 * b.scaled_zero();
    if near(trimmed.min(), ivl.min())
        && near(trimmed.max(), ivl.max())
        && curve.reparam_bspline_to(&ivl)?
    {
        return Ok(());
    }
    Err(SolidError::port_invariant(
        op,
        "a trim snapped the interval its snap left unchanged",
    ))
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
        reason: "a UV curve's lift is not certified within the tolerance of its 3D curve",
    }
}

/// The refusal of a lift proved strictly farther than `tol` from its 3D curve.
const fn lift_too_far() -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason: "a UV curve's lift is farther than the tolerance from its 3D curve",
    }
}

/// The refusal of a surface whose B-spline net is only an approximation of its definition.
pub(crate) const APPROXIMATE_NET_REASON: &str =
    "a UV curve on a surface whose NURBS net is an approximation";

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// A part's enclosure ([`certified_lift`]): the bound on the lift's distance over the part,
/// and a proved lower bound on it at the part's middle.
struct LiftPart {
    bound: f64,
    at_middle: f64,
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

/// Whether `e` is the certificate's own `SM_ERR` (a wide box's non-positive weight): an
/// [`SolidError::Upstream`], or a typed failure that replaced one (its
/// [`SolidError::upstream_status`], #13790).
fn is_certificate_sm_err(e: &SolidError) -> bool {
    e.upstream_status() == Some(crate::error::SM_ERR)
        && e.operation() == "blend::fillet_section::certificate"
}

/// Converts only `nurbs_partials_over_piece`'s wide-box non-positive-weight `SM_ERR` into
/// `Ok(None)` so the caller subdivides the span; all other errors (`PortInvariant`,
/// `Unsupported`, `Cancelled`, `Unported`) propagate untouched.
fn partials_or_subdivide(r: Result<Partials<Ivl>>) -> Result<Option<Partials<Ivl>>> {
    match r {
        Ok(p) => Ok(Some(p)),
        // the status, not the variant: a typed failure that replaced the `Upstream` (#13790)
        Err(e) if is_certificate_sm_err(&e) => Ok(None),
        Err(e) => Err(e),
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

fn clamp_ivl(x: Ivl, lo: f64, hi: f64) -> Ivl {
    if x.lo.is_nan() || x.hi.is_nan() {
        return Ivl::whole();
    }
    Ivl::new(cmax(cmin(x.lo, hi), lo), cmax(cmin(x.hi, hi), lo))
}

fn ivl_inside(x: Ivl, lo: f64, hi: f64) -> bool {
    x.lo >= lo && x.hi <= hi
}

fn ivl_disjoint(x: Ivl, lo: f64, hi: f64) -> bool {
    x.hi < lo || x.lo > hi
}

/// Returns `true` when `s` (or an `OffsetSurface` base / extended base within nesting
/// depth 4) evaluates a procedural definition whose B-spline net is only a positional
/// approximation (`ProceduralBlend`, `ProceduralSweep`), so net-derivative bounds do not
/// bound the definition's derivatives. Iterative (`stack`): no recursion.
pub(crate) fn surface_uses_approximate_net(s: &Surface) -> bool {
    let mut stack = vec![(s, 0_usize)];
    while let Some((cur, depth)) = stack.pop() {
        if depth > 4 || cur.procedural_sweep().is_some() {
            return true;
        }
        match cur.kind() {
            SurfaceKind::ProceduralBlend(_) => return true,
            SurfaceKind::OffsetSurface(off) => {
                stack.push((off.base_surface(), depth + 1));
                let ext_approx = off.with_extended_base_surface(|e| {
                    e.is_some_and(|es| {
                        depth + 1 > 4
                            || es.procedural_sweep().is_some()
                            || matches!(
                                es.kind(),
                                SurfaceKind::ProceduralBlend(_) | SurfaceKind::OffsetSurface(_)
                            )
                    })
                });
                if ext_approx {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Returns the exact rational B-spline representation of `s` when `s`'s point and
/// derivative evaluations equal that NURBS net's everywhere on its natural domain (`eps = 0`),
/// or `None` when `s` has no exact NURBS representation (`ProceduralBlend`,
/// `ProceduralSweep`, power-basis `BSplineSurfaceForm::Polynomial`, `WR-713` asymmetric
/// unbounded inside-out `Cone`, `OffsetSurface`, `CurveBounded`, `Step`, `SrfInVolume`).
pub(crate) fn exact_nurbs_surface<'a>(
    k: &Kernel,
    s: &'a Surface,
) -> Result<Option<Cow<'a, BSplineSurface>>> {
    if s.procedural_sweep().is_some() || matches!(s.kind(), SurfaceKind::ProceduralBlend(_)) {
        return Ok(None);
    }
    if let SurfaceKind::Cone(c) = s.kind_with_nurb(k)? {
        if !c.rev.is_bounded()? && c.inside_out && !k.fixes.cone_unbounded_parameterized_as_nurb {
            let step_ivl = c.rev.anal_uv_domain.v_interval();
            let nurb_dom = c.rev.base().natural_uv_domain();
            let nurb_ivl = if c.rev.swap_uv {
                nurb_dom.u_interval()
            } else {
                nurb_dom.v_interval()
            };
            if nurb_ivl.min().to_bits() != (-step_ivl.max()).to_bits()
                || nurb_ivl.max().to_bits() != (-step_ivl.min()).to_bits()
            {
                return Ok(None);
            }
        }
    }
    Ok(as_b_spline(k, s)?.filter(|b| b.form() != BSplineSurfaceForm::Polynomial))
}

/// Builds a [`NurbsCurve`] directly from `bs.gw_nurb()`'s raw homogeneous `CPoint`s
/// (along with its natural domain, whether out-of-bounds clamping applies, and its 2D
/// control-polygon bounding box on `natural_interval()`), avoiding `control_polygon()`'s
/// `(wx / w) * w` rational round-trip rounding loss.
pub(crate) fn exact_nurbs_curve_data(
    bs: &BSplineCurve,
    is_2d: bool,
) -> Result<(NurbsCurve, Extent1d, bool, Point2, Point2)> {
    let bad = || SolidError::Unsupported {
        operation: OP,
        reason: "a B-spline curve without a valid positive-weight NURBS net",
    };
    let nurb = bs.gw_nurb().ok_or_else(bad)?;
    let p = nurb.degree();
    let cpts = nurb.cpts();
    let n = cpts.len();
    let u = nurb.knots().to_vec();
    if p < 1 || n <= p || u.len() != n.checked_add(p + 1).ok_or_else(bad)? {
        return Err(bad());
    }
    if u.iter().any(|k| !k.is_finite()) {
        return Err(bad());
    }
    for (&a, &b) in u.iter().zip(u.iter().skip(1)) {
        if b < a {
            return Err(bad());
        }
    }
    let u0 = *u.get(p).ok_or_else(bad)?;
    let um = *u.get(n).ok_or_else(bad)?;
    if u0 >= um {
        return Err(bad());
    }

    let rational = nurb.is_rational();
    let mut pw = Vec::new();
    let (mut u_min, mut u_max) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut v_min, mut v_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for cp in cpts {
        if (cp.w != NOW) != rational {
            return Err(bad());
        }
        let w = if rational { cp.w } else { 1.0 };
        let z = if is_2d || cp.z == NOZ { 0.0 } else { cp.z };
        if !(w.is_finite() && w > 0.0 && cp.x.is_finite() && cp.y.is_finite() && z.is_finite()) {
            return Err(bad());
        }
        let (ux, vy) = if rational {
            (
                Ivl::from(cp.x) / Ivl::from(w),
                Ivl::from(cp.y) / Ivl::from(w),
            )
        } else {
            (Ivl::from(cp.x), Ivl::from(cp.y))
        };
        if !(ux.lo.is_finite() && ux.hi.is_finite() && vy.lo.is_finite() && vy.hi.is_finite()) {
            return Err(bad());
        }
        u_min = u_min.min(ux.lo);
        u_max = u_max.max(ux.hi);
        v_min = v_min.min(vy.lo);
        v_max = v_max.max(vy.hi);
        pw.extend_from_slice(&[cp.x, cp.y, z, w]);
    }
    if !(u_min <= u_max && v_min <= v_max) {
        return Err(bad());
    }
    Ok((
        NurbsCurve { p, n, u, pw },
        bs.natural_interval(),
        !bs.is_out_of_bounds_enabled(),
        Point2::new(u_min, v_min),
        Point2::new(u_max, v_max),
    ))
}

fn scale_exact_zero(v: V3<Ivl>, s: Ivl) -> V3<Ivl> {
    if s.lo.is_finite() && s.hi.is_finite() {
        let comp = |c: Ivl| {
            if c.lo == 0.0 && c.hi == 0.0 {
                IZ
            } else {
                c * s
            }
        };
        V3 {
            x: comp(v.x),
            y: comp(v.y),
            z: comp(v.z),
        }
    } else {
        v_scale(v, s)
    }
}

fn add_exact_zero(a: V3<Ivl>, b: V3<Ivl>) -> V3<Ivl> {
    let comp = |ac: Ivl, bc: Ivl| {
        if bc.lo == 0.0 && bc.hi == 0.0 {
            ac
        } else if ac.lo == 0.0 && ac.hi == 0.0 {
            bc
        } else {
            ac + bc
        }
    };
    V3 {
        x: comp(a.x, b.x),
        y: comp(a.y, b.y),
        z: comp(a.z, b.z),
    }
}

enum LineStepMode {
    Unbounded {
        inside_out: bool,
    },
    ZeroLength,
    Bounded {
        n_min: f64,
        n_max: f64,
        n_len: Ivl,
        a_min: f64,
        a_len: Ivl,
        inside_out: bool,
    },
}

struct LineEval {
    p0: V3<Ivl>,
    dir: V3<Ivl>,
    scale: Ivl,
    d1: V3<Ivl>,
    mode: LineStepMode,
}

impl LineEval {
    fn of(l: &crate::curve::line::Line, is_2d: bool) -> Result<Self> {
        let bad = || SolidError::Unsupported {
            operation: OP,
            reason: "a line curve with non-finite or degenerate parameterization",
        };
        let pt = l.line_point();
        let v = l.line_vector();
        let sc = l.line_scale();
        let pz = if is_2d || pt.z == NOZ { 0.0 } else { pt.z };
        let vz = if is_2d || v.z == NOZ { 0.0 } else { v.z };
        if !(pt.x.is_finite()
            && pt.y.is_finite()
            && pz.is_finite()
            && v.x.is_finite()
            && v.y.is_finite()
            && vz.is_finite()
            && sc.is_finite())
        {
            return Err(bad());
        }
        let p0 = V3 {
            x: Ivl::from(pt.x),
            y: Ivl::from(pt.y),
            z: Ivl::from(pz),
        };
        let dir = V3 {
            x: Ivl::from(v.x),
            y: Ivl::from(v.y),
            z: Ivl::from(vz),
        };
        let scale = Ivl::from(sc);
        let (mode, dstep_dt) = if l.is_bounded() {
            let nurb_ivl = l.natural_interval();
            if !(nurb_ivl.min().is_finite() && nurb_ivl.max().is_finite()) {
                return Err(bad());
            }
            if crate::geom::tol::is_zero(nurb_ivl.length()) {
                (LineStepMode::ZeroLength, Ivl::from(1.0))
            } else {
                let n_len = Ivl::from(nurb_ivl.max()) - Ivl::from(nurb_ivl.min());
                if !n_len.is_positive() {
                    return Err(bad());
                }
                let anal_ivl = l.step_interval();
                if !(anal_ivl.min().is_finite() && anal_ivl.max().is_finite()) {
                    return Err(bad());
                }
                let a_len = Ivl::from(anal_ivl.max()) - Ivl::from(anal_ivl.min());
                let rate = a_len / n_len;
                if !(rate.lo.is_finite() && rate.hi.is_finite()) {
                    return Err(bad());
                }
                let deriv = if l.inside_out() { IZ - rate } else { rate };
                (
                    LineStepMode::Bounded {
                        n_min: nurb_ivl.min(),
                        n_max: nurb_ivl.max(),
                        n_len,
                        a_min: anal_ivl.min(),
                        a_len,
                        inside_out: l.inside_out(),
                    },
                    deriv,
                )
            }
        } else {
            let inside_out = l.inside_out();
            (
                LineStepMode::Unbounded { inside_out },
                Ivl::from(if inside_out { -1.0 } else { 1.0 }),
            )
        };
        let d1 = scale_exact_zero(dir, dstep_dt * scale);
        Ok(Self {
            p0,
            dir,
            scale,
            d1,
            mode,
        })
    }

    fn eval_point(&self, t: Ivl) -> V3<Ivl> {
        let step = match self.mode {
            LineStepMode::Unbounded { inside_out } => {
                if inside_out {
                    IZ - t
                } else {
                    t
                }
            }
            LineStepMode::ZeroLength => t,
            LineStepMode::Bounded {
                n_min,
                n_max,
                n_len,
                a_min,
                a_len,
                inside_out,
            } => {
                let num = if inside_out {
                    Ivl::from(n_max) - t
                } else {
                    t - Ivl::from(n_min)
                };
                Ivl::from(a_min) + (num / n_len) * a_len
            }
        };
        add_exact_zero(self.p0, scale_exact_zero(self.dir, step * self.scale))
    }
}

enum LiftCurve {
    Line(LineEval),
    Nurbs {
        nc: NurbsCurve,
        dom: Extent1d,
        clamp_to_dom: bool,
        is_2d: bool,
        uv_min: Point2,
        uv_max: Point2,
    },
}

impl LiftCurve {
    fn of(curve: &Curve, lo: f64, hi: f64, is_2d: bool) -> Result<Self> {
        if let CurveKind::Line(l) = curve.kind() {
            return Ok(Self::Line(LineEval::of(l, is_2d)?));
        }
        let bs = match curve.kind() {
            CurveKind::BSpline(b) => b,
            CurveKind::Ellipse(e) => e.base(),
            CurveKind::Hyperbola(h) => h.base(),
            CurveKind::Parabola(p) => p.base(),
            _ => {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a curve that is not of a line or B-spline kind",
                });
            }
        };
        let (nc, dom, clamp_to_dom, uv_min, uv_max) = exact_nurbs_curve_data(bs, is_2d)?;
        let active_lo = if clamp_to_dom {
            cmax(lo, dom.min())
        } else {
            lo
        };
        let active_hi = if clamp_to_dom {
            cmin(hi, dom.max())
        } else {
            hi
        };
        let mut s = nc.p + 1;
        while s < nc.n {
            let Some(&kv) = nc.u.get(s) else {
                break;
            };
            let mut end = s + 1;
            while end < nc.n && nc.u.get(end) == Some(&kv) {
                end += 1;
            }
            if end - s > nc.p && kv > active_lo && kv < active_hi {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a curve across an interior knot where its net is not continuous",
                });
            }
            s = end;
        }
        Ok(Self::Nurbs {
            nc,
            dom,
            clamp_to_dom,
            is_2d,
            uv_min,
            uv_max,
        })
    }

    fn push_cuts(&self, lo: f64, hi: f64, cuts: &mut Vec<f64>) {
        if let Self::Nurbs { nc, dom, .. } = self {
            for bnd in [dom.min(), dom.max()] {
                if bnd > lo && bnd < hi {
                    cuts.push(bnd);
                }
            }
            cuts.extend(nc.u.iter().copied().filter(|&t| t > lo && t < hi));
        }
    }

    fn uv_bounds(&self, lo: f64, hi: f64) -> Result<(Point2, Point2)> {
        let p_bounds = |p: V3<Ivl>| -> Result<(Point2, Point2)> {
            if !(p.x.lo.is_finite()
                && p.x.hi.is_finite()
                && p.y.lo.is_finite()
                && p.y.hi.is_finite())
            {
                return Err(lift_not_certified());
            }
            Ok((Point2::new(p.x.lo, p.y.lo), Point2::new(p.x.hi, p.y.hi)))
        };
        match self {
            Self::Line(le) => p_bounds(le.eval_point(Ivl::new(lo, hi))),
            Self::Nurbs {
                dom,
                clamp_to_dom,
                uv_min,
                uv_max,
                ..
            } => {
                if *clamp_to_dom || (lo >= dom.min() && hi <= dom.max()) {
                    Ok((*uv_min, *uv_max))
                } else {
                    let [p, _, _] = self.derivs(Ivl::new(lo, hi), 0)?;
                    p_bounds(p)
                }
            }
        }
    }

    fn derivs(&self, t: Ivl, order: usize) -> Result<[V3<Ivl>; 3]> {
        let missing =
            || SolidError::port_invariant(OP, "a curve's derivative over a part is missing");
        match self {
            Self::Line(le) => Ok([le.eval_point(t), le.d1, VZ]),
            Self::Nurbs {
                nc,
                dom,
                clamp_to_dom,
                is_2d,
                uv_min,
                uv_max,
            } => {
                let clamp_hull = |p: V3<Ivl>, in_hull: bool| -> V3<Ivl> {
                    if *is_2d {
                        if in_hull {
                            V3 {
                                x: clamp_ivl(p.x, uv_min.x, uv_max.x),
                                y: clamp_ivl(p.y, uv_min.y, uv_max.y),
                                z: IZ,
                            }
                        } else {
                            V3 {
                                x: p.x,
                                y: p.y,
                                z: IZ,
                            }
                        }
                    } else {
                        p
                    }
                };
                if *clamp_to_dom && (t.hi <= dom.min() || t.lo >= dom.max()) {
                    let bnd = if t.hi <= dom.min() {
                        dom.min()
                    } else {
                        dom.max()
                    };
                    let d = curve_derivs_over(nc, Ivl::from(bnd), 0, None)?;
                    let d0 = d.first().copied().ok_or_else(missing)?;
                    return Ok([clamp_hull(d0, true), VZ, VZ]);
                }
                let (tc, in_hull) = if *clamp_to_dom {
                    (clamp_ivl(t, dom.min(), dom.max()), true)
                } else {
                    (t, ivl_inside(t, dom.min(), dom.max()))
                };
                let d = curve_derivs_over(nc, tc, order, None)?;
                let d0 = d.first().copied().ok_or_else(missing)?;
                let d1 = if order >= 1 {
                    d.get(1).copied().ok_or_else(missing)?
                } else {
                    VZ
                };
                let d2 = if order >= 2 {
                    d.get(2).copied().ok_or_else(missing)?
                } else {
                    VZ
                };
                Ok([clamp_hull(d0, in_hull), d1, d2])
            }
        }
    }
}

#[derive(Clone, Copy)]
enum NurbsOobMode {
    /// `BSplineSurface::evaluate_point` evaluates end knot spans past `dom` without clamping.
    ExtrapolateEndSpans,
    /// `Cone::evaluate_point` wraps its sweep coordinate via `polar` outside `dom`, while its
    /// linear generator coordinate extrapolates when `linear_oob` and clamps otherwise.
    Cone { swap_uv: bool, linear_oob: bool },
}

#[derive(Clone, Copy)]
struct OffsetDomCfg {
    allow_ext: bool,
    periodic: [bool; 2],
    singularities: u32,
}

enum LiftSurface {
    Plane {
        origin: V3<Ivl>,
        su: V3<Ivl>,
        sv: V3<Ivl>,
        step_dom: Extent2d,
        oob: bool,
    },
    Nurbs {
        net: Nurbs,
        dom: Extent2d,
        oob_mode: NurbsOobMode,
    },
    Offset {
        d: Ivl,
        base_net: Nurbs,
        base_dom: Extent2d,
        cfg: OffsetDomCfg,
        ext: Option<(Nurbs, Extent2d)>,
    },
}

struct CurveSpanEval {
    w0: V3<Ivl>,
    w1: V3<Ivl>,
    w2: V3<Ivl>,
    wm0: V3<Ivl>,
    wm1: V3<Ivl>,
    c0: V3<Ivl>,
    c1: V3<Ivl>,
    c2: V3<Ivl>,
    cm0: V3<Ivl>,
    cm1: V3<Ivl>,
    dt: Ivl,
    half_r2: Ivl,
}

fn second_order_taylor(
    s_t: &Partials<Ivl>,
    s_m: &Partials<Ivl>,
    ev: &CurveSpanEval,
) -> (V3<Ivl>, V3<Ivl>) {
    let g_m = v_sub(s_m.at(0, 0), ev.cm0);
    let dg_m = v_sub(
        v_add(
            v_scale(s_m.at(1, 0), ev.wm1.x),
            v_scale(s_m.at(0, 1), ev.wm1.y),
        ),
        ev.cm1,
    );
    let (u1, v1) = (ev.w1.x, ev.w1.y);
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
            v_add(
                v_scale(s_t.at(1, 0), ev.w2.x),
                v_scale(s_t.at(0, 1), ev.w2.y),
            ),
        ),
        ev.c2,
    );
    let g = v_add(v_add(g_m, v_scale(dg_m, ev.dt)), v_scale(ddg, ev.half_r2));
    (g, g_m)
}

fn clamp_singular_uv(mut ux: Ivl, mut vy: Ivl, dom: &Extent2d, singularities: u32) -> (Ivl, Ivl) {
    let (u0, u1) = (dom.min().x, dom.max().x);
    let (v0, v1) = (dom.min().y, dom.max().y);
    if (singularities & SS_UMIN) != 0 {
        ux = Ivl::new(cmax(ux.lo, u0), cmax(ux.hi, u0));
    }
    if (singularities & SS_VMIN) != 0 {
        vy = Ivl::new(cmax(vy.lo, v0), cmax(vy.hi, v0));
    }
    if (singularities & SS_UMAX) != 0 {
        ux = Ivl::new(cmin(ux.lo, u1), cmin(ux.hi, u1));
    }
    if (singularities & SS_VMAX) != 0 {
        vy = Ivl::new(cmin(vy.lo, v1), cmin(vy.hi, v1));
    }
    (ux, vy)
}

fn periodic_or_in_dom_parts(
    knots: &[f64],
    p: usize,
    n: usize,
    x: Ivl,
    dmin: f64,
    dmax: f64,
    is_periodic: bool,
) -> Result<Option<Vec<(f64, f64, usize)>>> {
    if ivl_inside(x, dmin, dmax) {
        return net_parts(knots, p, n, x).map(Some);
    }
    if !is_periodic {
        return Ok(None);
    }
    let len = dmax - dmin;
    if !(len > 0.0 && x.hi - x.lo < len && x.lo >= dmin - len && x.hi <= dmax + len) {
        return Ok(None);
    }
    let mut parts = Vec::new();
    if x.lo < dmin {
        let w_lo = cmax(
            (Ivl::from(dmax) + (Ivl::from(x.lo) - Ivl::from(dmin))).lo,
            dmin,
        );
        let w_hi = if x.hi < dmin {
            cmin(
                (Ivl::from(dmax) + (Ivl::from(x.hi) - Ivl::from(dmin))).hi,
                dmax,
            )
        } else {
            dmax
        };
        if w_lo <= w_hi {
            parts.extend(net_parts(knots, p, n, Ivl::new(w_lo, w_hi))?);
        }
    }
    let in_lo = cmax(x.lo, dmin);
    let in_hi = cmin(x.hi, dmax);
    if in_lo <= in_hi && x.hi >= dmin && x.lo <= dmax {
        parts.extend(net_parts(knots, p, n, Ivl::new(in_lo, in_hi))?);
    }
    if x.hi > dmax {
        let w_lo = if x.lo > dmax {
            cmax(
                (Ivl::from(dmin) + (Ivl::from(x.lo) - Ivl::from(dmax))).lo,
                dmin,
            )
        } else {
            dmin
        };
        let w_hi = cmin(
            (Ivl::from(dmin) + (Ivl::from(x.hi) - Ivl::from(dmax))).hi,
            dmax,
        );
        if w_lo <= w_hi {
            parts.extend(net_parts(knots, p, n, Ivl::new(w_lo, w_hi))?);
        }
    }
    Ok((!parts.is_empty()).then_some(parts))
}

fn eval_offset_allow_ext_oob(
    net: &Nurbs,
    d: Ivl,
    dom: &Extent2d,
    cfg: OffsetDomCfg,
    ev: &CurveSpanEval,
) -> Result<LiftPart> {
    let (ux, vy) = clamp_singular_uv(ev.w0.x, ev.w0.y, dom, cfg.singularities);
    let (umx, umy) = clamp_singular_uv(ev.wm0.x, ev.wm0.y, dom, cfg.singularities);
    if let (Some(pu), Some(pv), Some(pum), Some(pvm)) = (
        periodic_or_in_dom_parts(
            &net.u,
            net.p,
            net.nu,
            ux,
            dom.min().x,
            dom.max().x,
            cfg.periodic[0],
        )?,
        periodic_or_in_dom_parts(
            &net.v,
            net.q,
            net.nv,
            vy,
            dom.min().y,
            dom.max().y,
            cfg.periodic[1],
        )?,
        periodic_or_in_dom_parts(
            &net.u,
            net.p,
            net.nu,
            umx,
            dom.min().x,
            dom.max().x,
            cfg.periodic[0],
        )?,
        periodic_or_in_dom_parts(
            &net.v,
            net.q,
            net.nv,
            umy,
            dom.min().y,
            dom.max().y,
            cfg.periodic[1],
        )?,
    ) {
        let Some(s_m) = partials_or_subdivide(offset_partials_over_parts(net, d, &pum, &pvm, 0))?
        else {
            return Ok(LiftPart {
                bound: f64::INFINITY,
                at_middle: 0.0,
            });
        };
        let g_m = v_sub(s_m.at(0, 0), ev.cm0);
        let bound = partials_or_subdivide(offset_partials_over_parts(net, d, &pu, &pv, 0))?
            .map_or(f64::INFINITY, |s_t| norm_above(v_sub(s_t.at(0, 0), ev.c0)));
        return Ok(LiftPart {
            bound,
            at_middle: norm_below(g_m),
        });
    }
    if (!cfg.periodic[0] && ivl_disjoint(umx, dom.min().x, dom.max().x))
        || (!cfg.periodic[1] && ivl_disjoint(umy, dom.min().y, dom.max().y))
    {
        return Err(lift_not_certified());
    }
    let umx_c = clamp_ivl(umx, dom.min().x, dom.max().x);
    let umy_c = clamp_ivl(umy, dom.min().y, dom.max().y);
    let pum = net_parts(&net.u, net.p, net.nu, umx_c)?;
    let pvm = net_parts(&net.v, net.q, net.nv, umy_c)?;
    let at_mid = partials_or_subdivide(offset_partials_over_parts(net, d, &pum, &pvm, 0))?
        .map_or(0.0, |s_m| norm_below(v_sub(s_m.at(0, 0), ev.cm0)));
    Ok(LiftPart {
        bound: f64::INFINITY,
        at_middle: at_mid,
    })
}

fn eval_offset_on_net(
    net: &Nurbs,
    d: Ivl,
    dom: &Extent2d,
    cfg: OffsetDomCfg,
    ev: &CurveSpanEval,
) -> Result<LiftPart> {
    let (ux, vy) = (ev.w0.x, ev.w0.y);
    let (umx, umy) = (ev.wm0.x, ev.wm0.y);
    let in_dom =
        ivl_inside(ux, dom.min().x, dom.max().x) && ivl_inside(vy, dom.min().y, dom.max().y);
    if !in_dom {
        if cfg.allow_ext {
            return eval_offset_allow_ext_oob(net, d, dom, cfg, ev);
        }
        let ux_c = clamp_ivl(ux, dom.min().x, dom.max().x);
        let vy_c = clamp_ivl(vy, dom.min().y, dom.max().y);
        let umx_c = clamp_ivl(umx, dom.min().x, dom.max().x);
        let umy_c = clamp_ivl(umy, dom.min().y, dom.max().y);
        let pu = net_parts(&net.u, net.p, net.nu, ux_c)?;
        let pv = net_parts(&net.v, net.q, net.nv, vy_c)?;
        let pum = net_parts(&net.u, net.p, net.nu, umx_c)?;
        let pvm = net_parts(&net.v, net.q, net.nv, umy_c)?;
        let Some(s_m) = partials_or_subdivide(offset_partials_over_parts(net, d, &pum, &pvm, 0))?
        else {
            return Ok(LiftPart {
                bound: f64::INFINITY,
                at_middle: 0.0,
            });
        };
        let g_m = v_sub(s_m.at(0, 0), ev.cm0);
        let bound = partials_or_subdivide(offset_partials_over_parts(net, d, &pu, &pv, 0))?
            .map_or(f64::INFINITY, |s_t| norm_above(v_sub(s_t.at(0, 0), ev.c0)));
        return Ok(LiftPart {
            bound,
            at_middle: norm_below(g_m),
        });
    }

    let pu = net_parts(&net.u, net.p, net.nu, ux)?;
    let pv = net_parts(&net.v, net.q, net.nv, vy)?;
    if let ([(_, _, ku)], [(_, _, kv)]) = (pu.as_slice(), pv.as_slice()) {
        let Some(s_m) = partials_or_subdivide(offset_partials_over_parts(
            net,
            d,
            &[(umx.lo, umx.hi, *ku)],
            &[(umy.lo, umy.hi, *kv)],
            1,
        ))?
        else {
            return Ok(LiftPart {
                bound: f64::INFINITY,
                at_middle: 0.0,
            });
        };
        let g_m = v_sub(s_m.at(0, 0), ev.cm0);
        let bound = partials_or_subdivide(offset_partials_over_parts(net, d, &pu, &pv, 2))?
            .map_or(f64::INFINITY, |s_t| {
                norm_above(second_order_taylor(&s_t, &s_m, ev).0)
            });
        return Ok(LiftPart {
            bound,
            at_middle: norm_below(g_m),
        });
    }

    let pum = net_parts(&net.u, net.p, net.nu, umx)?;
    let pvm = net_parts(&net.v, net.q, net.nv, umy)?;
    let Some(s_m) = partials_or_subdivide(offset_partials_over_parts(net, d, &pum, &pvm, 0))?
    else {
        return Ok(LiftPart {
            bound: f64::INFINITY,
            at_middle: 0.0,
        });
    };
    let g_m = v_sub(s_m.at(0, 0), ev.cm0);
    let mono_u = ev.w1.x.lo > 0.0 || ev.w1.x.hi < 0.0;
    let mono_v = ev.w1.y.lo > 0.0 || ev.w1.y.hi < 0.0;
    let jump_opt = match offset_crossed_knot_jumps(net, d, &pu, &pv, mono_u, mono_v) {
        Ok(j) => Some(j),
        Err(e) if is_certificate_sm_err(&e) => None,
        Err(e) => return Err(e),
    };
    let bound = match jump_opt {
        Some(Some(jump)) => partials_or_subdivide(offset_partials_over_parts(net, d, &pu, &pv, 1))?
            .map_or(f64::INFINITY, |s_t| {
                let dg = v_sub(
                    v_add(
                        v_scale(s_t.at(1, 0), ev.w1.x),
                        v_scale(s_t.at(0, 1), ev.w1.y),
                    ),
                    ev.c1,
                );
                norm_above(v_add(v_add(g_m, v_scale(dg, ev.dt)), jump))
            }),
        Some(None) => partials_or_subdivide(offset_partials_over_parts(net, d, &pu, &pv, 0))?
            .map_or(f64::INFINITY, |s_t| norm_above(v_sub(s_t.at(0, 0), ev.c0))),
        None => f64::INFINITY,
    };
    Ok(LiftPart {
        bound,
        at_middle: norm_below(g_m),
    })
}

fn lift_plane_surface(pl: &crate::surface::plane::Plane) -> Result<LiftSurface> {
    let o = pl.position().origin();
    let ax = pl.position().x_axis();
    let ay = pl.position().y_axis();
    let sc = pl.uv_scale();
    if !(o.x.is_finite()
        && o.y.is_finite()
        && o.z.is_finite()
        && ax.x.is_finite()
        && ax.y.is_finite()
        && ax.z.is_finite()
        && ay.x.is_finite()
        && ay.y.is_finite()
        && ay.z.is_finite()
        && sc.x.is_finite()
        && sc.y.is_finite())
    {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a plane surface with non-finite frame or scale",
        });
    }
    let sx = Ivl::from(sc.x);
    let sy = Ivl::from(sc.y);
    Ok(LiftSurface::Plane {
        origin: V3 {
            x: Ivl::from(o.x),
            y: Ivl::from(o.y),
            z: Ivl::from(o.z),
        },
        su: V3 {
            x: sx * Ivl::from(ax.x),
            y: sx * Ivl::from(ax.y),
            z: sx * Ivl::from(ax.z),
        },
        sv: V3 {
            x: sy * Ivl::from(ay.x),
            y: sy * Ivl::from(ay.y),
            z: sy * Ivl::from(ay.z),
        },
        step_dom: pl.step_uv_domain(),
        oob: pl.is_out_of_bounds_enabled(),
    })
}

fn lift_offset_surface(
    k: &Kernel,
    surface: &Surface,
    off: &crate::surface::offset_surface::OffsetSurface,
    uv_min: Point2,
    uv_max: Point2,
) -> Result<LiftSurface> {
    let unsupported_base = || SolidError::Unsupported {
        operation: OP,
        reason: "an offset surface without an exact NURBS base net",
    };
    if off.allow_extension() {
        for corner in [
            uv_min,
            Point2::new(uv_min.x, uv_max.y),
            Point2::new(uv_max.x, uv_min.y),
            uv_max,
        ] {
            let _ = surface.evaluate_point(k, corner);
        }
    }
    let (base, d) = if let SurfaceKind::OffsetSurface(inner) = off.base_surface().kind() {
        if matches!(inner.base_surface().kind(), SurfaceKind::OffsetSurface(_)) {
            return Err(unsupported_base());
        }
        (
            inner.base_surface(),
            Ivl::from(off.offset_distance()) + Ivl::from(inner.offset_distance()),
        )
    } else {
        (off.base_surface(), Ivl::from(off.offset_distance()))
    };
    if !(d.lo.is_finite() && d.hi.is_finite()) {
        return Err(unsupported_base());
    }
    if surface_uses_approximate_net(base) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: APPROXIMATE_NET_REASON,
        });
    }
    let base_dom = base.natural_uv_domain_with_nurb(k)?;
    let bs_base = exact_nurbs_surface(k, base)?.ok_or_else(unsupported_base)?;
    let base_net = Nurbs::of(&bs_base).map_err(|e| match e {
        e if e.upstream_status().is_some() => unsupported_base(),
        other => other,
    })?;
    let ext_srf = off
        .with_extended_base_surface(|e| e.map(Surface::clone_for_evaluation))
        .transpose()?;
    let ext = match ext_srf {
        Some(e_srf) => {
            if surface_uses_approximate_net(&e_srf) {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: APPROXIMATE_NET_REASON,
                });
            }
            let bs_ext = exact_nurbs_surface(k, &e_srf)?.ok_or_else(unsupported_base)?;
            let ext_net = Nurbs::of(&bs_ext).map_err(|e| match e {
                e if e.upstream_status().is_some() => unsupported_base(),
                other => other,
            })?;
            let ext_dom = e_srf.natural_uv_domain_with_nurb(k)?;
            Some((ext_net, ext_dom))
        }
        None => None,
    };
    let [pu, pv] = off.is_periodic();
    Ok(LiftSurface::Offset {
        d,
        base_net,
        base_dom,
        cfg: OffsetDomCfg {
            allow_ext: off.allow_extension(),
            periodic: [pu != 0, pv != 0],
            singularities: off.singularities(),
        },
        ext,
    })
}

fn eval_plane_part(
    origin: V3<Ivl>,
    su: V3<Ivl>,
    sv: V3<Ivl>,
    step_dom: &Extent2d,
    oob: bool,
    ev: &CurveSpanEval,
) -> LiftPart {
    let (ux, vy) = (ev.w0.x, ev.w0.y);
    let (umx, umy) = (ev.wm0.x, ev.wm0.y);
    let in_dom = ivl_inside(ux, step_dom.min().x, step_dom.max().x)
        && ivl_inside(vy, step_dom.min().y, step_dom.max().y);
    if oob || in_dom {
        let s_m_0 = v_add(origin, v_add(v_scale(su, umx), v_scale(sv, umy)));
        let g_m = v_sub(s_m_0, ev.cm0);
        let dg_m = v_sub(v_add(v_scale(su, ev.wm1.x), v_scale(sv, ev.wm1.y)), ev.cm1);
        let ddg = v_sub(v_add(v_scale(su, ev.w2.x), v_scale(sv, ev.w2.y)), ev.c2);
        let g = v_add(v_add(g_m, v_scale(dg_m, ev.dt)), v_scale(ddg, ev.half_r2));
        LiftPart {
            bound: norm_above(g),
            at_middle: norm_below(g_m),
        }
    } else {
        let umx_c = clamp_ivl(umx, step_dom.min().x, step_dom.max().x);
        let umy_c = clamp_ivl(umy, step_dom.min().y, step_dom.max().y);
        let s_m_0 = v_add(origin, v_add(v_scale(su, umx_c), v_scale(sv, umy_c)));
        let g_m = v_sub(s_m_0, ev.cm0);
        let su_eff = if ux.hi <= step_dom.min().x || ux.lo >= step_dom.max().x {
            VZ
        } else {
            v_hull(su, VZ)
        };
        let sv_eff = if vy.hi <= step_dom.min().y || vy.lo >= step_dom.max().y {
            VZ
        } else {
            v_hull(sv, VZ)
        };
        let dg = v_sub(
            v_add(v_scale(su_eff, ev.w1.x), v_scale(sv_eff, ev.w1.y)),
            ev.c1,
        );
        LiftPart {
            bound: norm_above(v_add(g_m, v_scale(dg, ev.dt))),
            at_middle: norm_below(g_m),
        }
    }
}

fn eval_nurbs_part(
    net: &Nurbs,
    dom: &Extent2d,
    oob_mode: NurbsOobMode,
    ev: &CurveSpanEval,
) -> Result<LiftPart> {
    let (ux, vy) = (ev.w0.x, ev.w0.y);
    let (umx, umy) = (ev.wm0.x, ev.wm0.y);
    let in_u = ivl_inside(ux, dom.min().x, dom.max().x);
    let in_v = ivl_inside(vy, dom.min().y, dom.max().y);
    if !(in_u && in_v) {
        if let NurbsOobMode::Cone {
            swap_uv,
            linear_oob,
        } = oob_mode
        {
            let (sweep_in, sweep_mid, smin, smax) = if swap_uv {
                (in_v, umy, dom.min().y, dom.max().y)
            } else {
                (in_u, umx, dom.min().x, dom.max().x)
            };
            if !sweep_in {
                if ivl_disjoint(sweep_mid, smin, smax) {
                    return Err(lift_not_certified());
                }
                let umx_c = clamp_ivl(umx, dom.min().x, dom.max().x);
                let umy_c = clamp_ivl(umy, dom.min().y, dom.max().y);
                let pum = net_parts(&net.u, net.p, net.nu, umx_c)?;
                let pvm = net_parts(&net.v, net.q, net.nv, umy_c)?;
                let at_mid = partials_or_subdivide(net_partials(net, &pum, &pvm, 0))?
                    .map_or(0.0, |s_m| norm_below(v_sub(s_m.at(0, 0), ev.cm0)));
                return Ok(LiftPart {
                    bound: f64::INFINITY,
                    at_middle: at_mid,
                });
            }
            if !linear_oob {
                let ux_c = clamp_ivl(ux, dom.min().x, dom.max().x);
                let vy_c = clamp_ivl(vy, dom.min().y, dom.max().y);
                let umx_c = clamp_ivl(umx, dom.min().x, dom.max().x);
                let umy_c = clamp_ivl(umy, dom.min().y, dom.max().y);
                let pu = net_parts(&net.u, net.p, net.nu, ux_c)?;
                let pv = net_parts(&net.v, net.q, net.nv, vy_c)?;
                let pum = net_parts(&net.u, net.p, net.nu, umx_c)?;
                let pvm = net_parts(&net.v, net.q, net.nv, umy_c)?;
                let Some(s_m) = partials_or_subdivide(net_partials(net, &pum, &pvm, 0))? else {
                    return Ok(LiftPart {
                        bound: f64::INFINITY,
                        at_middle: 0.0,
                    });
                };
                let g_m = v_sub(s_m.at(0, 0), ev.cm0);
                let bound = partials_or_subdivide(net_partials(net, &pu, &pv, 1))?.map_or(
                    f64::INFINITY,
                    |s_t| {
                        let su = if ux.hi <= dom.min().x || ux.lo >= dom.max().x {
                            VZ
                        } else if in_u {
                            s_t.at(1, 0)
                        } else {
                            v_hull(s_t.at(1, 0), VZ)
                        };
                        let sv = if vy.hi <= dom.min().y || vy.lo >= dom.max().y {
                            VZ
                        } else if in_v {
                            s_t.at(0, 1)
                        } else {
                            v_hull(s_t.at(0, 1), VZ)
                        };
                        let dg = v_sub(v_add(v_scale(su, ev.w1.x), v_scale(sv, ev.w1.y)), ev.c1);
                        norm_above(v_add(g_m, v_scale(dg, ev.dt)))
                    },
                );
                return Ok(LiftPart {
                    bound,
                    at_middle: norm_below(g_m),
                });
            }
        }
    }

    let pu = net_parts(&net.u, net.p, net.nu, ux)?;
    let pv = net_parts(&net.v, net.q, net.nv, vy)?;
    if one_piece(&pu) && one_piece(&pv) {
        let pm = |parts: &[(f64, f64, usize)], x: Ivl| -> Vec<(f64, f64, usize)> {
            parts.iter().map(|&(_, _, s)| (x.lo, x.hi, s)).collect()
        };
        let Some(s_m) = partials_or_subdivide(net_partials(net, &pm(&pu, umx), &pm(&pv, umy), 1))?
        else {
            return Ok(LiftPart {
                bound: f64::INFINITY,
                at_middle: 0.0,
            });
        };
        let g_m = v_sub(s_m.at(0, 0), ev.cm0);
        let bound = partials_or_subdivide(net_partials(net, &pu, &pv, 2))?
            .map_or(f64::INFINITY, |s_t| {
                norm_above(second_order_taylor(&s_t, &s_m, ev).0)
            });
        Ok(LiftPart {
            bound,
            at_middle: norm_below(g_m),
        })
    } else {
        let pum = net_parts(&net.u, net.p, net.nu, umx)?;
        let pvm = net_parts(&net.v, net.q, net.nv, umy)?;
        let Some(s_m) = partials_or_subdivide(net_partials(net, &pum, &pvm, 0))? else {
            return Ok(LiftPart {
                bound: f64::INFINITY,
                at_middle: 0.0,
            });
        };
        let g_m = v_sub(s_m.at(0, 0), ev.cm0);
        let bound =
            partials_or_subdivide(net_partials(net, &pu, &pv, 1))?.map_or(f64::INFINITY, |s_t| {
                let dg = v_sub(
                    v_add(
                        v_scale(s_t.at(1, 0), ev.w1.x),
                        v_scale(s_t.at(0, 1), ev.w1.y),
                    ),
                    ev.c1,
                );
                norm_above(v_add(g_m, v_scale(dg, ev.dt)))
            });
        Ok(LiftPart {
            bound,
            at_middle: norm_below(g_m),
        })
    }
}

fn eval_offset_part(
    d: Ivl,
    base_net: &Nurbs,
    base_dom: &Extent2d,
    cfg: OffsetDomCfg,
    ext: Option<&(Nurbs, Extent2d)>,
    ev: &CurveSpanEval,
) -> Result<LiftPart> {
    let (ux, vy) = (ev.w0.x, ev.w0.y);
    let (umx, umy) = (ev.wm0.x, ev.wm0.y);
    let in_base = ivl_inside(ux, base_dom.min().x, base_dom.max().x)
        && ivl_inside(vy, base_dom.min().y, base_dom.max().y);
    if in_base {
        return eval_offset_on_net(base_net, d, base_dom, cfg, ev);
    }
    if let Some((ext_net, ext_dom)) = ext {
        let outside_base = ivl_disjoint(ux, base_dom.min().x, base_dom.max().x)
            || ivl_disjoint(vy, base_dom.min().y, base_dom.max().y);
        if outside_base {
            return eval_offset_on_net(ext_net, d, ext_dom, cfg, ev);
        }
        let part_ext = eval_offset_on_net(ext_net, d, ext_dom, cfg, ev)?;
        let ux_b = clamp_ivl(ux, base_dom.min().x, base_dom.max().x);
        let vy_b = clamp_ivl(vy, base_dom.min().y, base_dom.max().y);
        let pu_b = net_parts(&base_net.u, base_net.p, base_net.nu, ux_b)?;
        let pv_b = net_parts(&base_net.v, base_net.q, base_net.nv, vy_b)?;
        let bound_base =
            partials_or_subdivide(offset_partials_over_parts(base_net, d, &pu_b, &pv_b, 0))?
                .map_or(f64::INFINITY, |qb| norm_above(v_sub(qb.at(0, 0), ev.c0)));
        let mid_in_base = ivl_inside(umx, base_dom.min().x, base_dom.max().x)
            && ivl_inside(umy, base_dom.min().y, base_dom.max().y);
        let mid_out_base = ivl_disjoint(umx, base_dom.min().x, base_dom.max().x)
            || ivl_disjoint(umy, base_dom.min().y, base_dom.max().y);
        let at_mid = if mid_in_base {
            let pum = net_parts(&base_net.u, base_net.p, base_net.nu, umx)?;
            let pvm = net_parts(&base_net.v, base_net.q, base_net.nv, umy)?;
            partials_or_subdivide(offset_partials_over_parts(base_net, d, &pum, &pvm, 0))?
                .map_or(0.0, |q| norm_below(v_sub(q.at(0, 0), ev.cm0)))
        } else if mid_out_base {
            part_ext.at_middle
        } else {
            0.0
        };
        return Ok(LiftPart {
            bound: cmax(part_ext.bound, bound_base),
            at_middle: at_mid,
        });
    }
    eval_offset_on_net(base_net, d, base_dom, cfg, ev)
}

impl LiftSurface {
    fn of(k: &Kernel, surface: &Surface, uv_min: Point2, uv_max: Point2) -> Result<Self> {
        if surface_uses_approximate_net(surface) {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: APPROXIMATE_NET_REASON,
            });
        }
        if let SurfaceKind::Plane(pl) = surface.kind() {
            return lift_plane_surface(pl);
        }
        if let SurfaceKind::OffsetSurface(off) = surface.kind() {
            return lift_offset_surface(k, surface, off, uv_min, uv_max);
        }
        let unsupported_srf = || SolidError::Unsupported {
            operation: OP,
            reason: "a UV curve on a surface without an exact lift enclosure",
        };
        let bs = exact_nurbs_surface(k, surface)?.ok_or_else(unsupported_srf)?;
        let net = Nurbs::of(&bs).map_err(|e| match e {
            e if e.upstream_status().is_some() => unsupported_srf(),
            other => other,
        })?;
        let dom = surface.natural_uv_domain_with_nurb(k)?;
        let oob_mode = match surface.kind() {
            SurfaceKind::Cone(c) => NurbsOobMode::Cone {
                swap_uv: c.rev().swap_uv,
                linear_oob: c.rev().is_out_of_bounds_enabled(),
            },
            _ => NurbsOobMode::ExtrapolateEndSpans,
        };
        Ok(Self::Nurbs { net, dom, oob_mode })
    }

    fn lift_part(&self, ev: &CurveSpanEval) -> Result<LiftPart> {
        match self {
            Self::Plane {
                origin,
                su,
                sv,
                step_dom,
                oob,
            } => Ok(eval_plane_part(*origin, *su, *sv, step_dom, *oob, ev)),
            Self::Nurbs { net, dom, oob_mode } => eval_nurbs_part(net, dom, *oob_mode, ev),
            Self::Offset {
                d,
                base_net,
                base_dom,
                cfg,
                ext,
            } => eval_offset_part(*d, base_net, base_dom, *cfg, ext.as_ref(), ev),
        }
    }
}

/// An upper bound, enclosed in outward-rounded [`Ivl`] arithmetic, on
/// `sup_{t in ivl} |surface(uv(t)) - curve(t)|` (`docs/definitions-and-certificates.md`
/// §4.4, §7.5).
///
/// Supports analytic planes, surfaces with an exact NURBS net ([`exact_nurbs_surface`],
/// including cones and cylinders with periodic/clamped domain semantics), and procedural
/// [`OffsetSurface`](crate::surface::offset_surface::OffsetSurface)s over an exact base
/// ([`offset_partials_over_parts`], [`offset_crossed_knot_jumps`]), paired with line,
/// conic or B-spline UV and 3D curves. `ivl` is cut at the domain bounds and inner knots
/// of `uv` and `curve`, so each part lies on one polynomial piece of both, and each part
/// is enclosed and halved until its bound is at most `tol`. Returns the largest certified
/// part bound over `ivl`.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a surface whose NURBS net is only an approximation
/// (`ProceduralBlend`, `ProceduralSweep`), an unsupported surface or curve kind, a
/// non-finite or negative `tol`, a lift proved farther than `tol` at an endpoint or a
/// part's middle, or a part still above `tol` after [`LIFT_MAX_DEPTH`] halvings or past
/// [`LIFT_MAX_PARTS`] parts; [`SolidError::PortInvariant`] for an empty or non-finite
/// `ivl`; those of the enclosures.
pub(crate) fn certified_lift(
    k: &Kernel,
    surface: &Surface,
    uv: &Curve,
    curve: &Curve,
    ivl: Extent1d,
    tol: f64,
) -> Result<f64> {
    let (lo, hi) = (ivl.min(), ivl.max());
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolidError::port_invariant(
            OP,
            "a UV curve lift over an empty or non-finite interval",
        ));
    }
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a UV curve lift tolerance that is negative or non-finite",
        });
    }
    let uv_c = LiftCurve::of(uv, lo, hi, true)?;
    let c3_c = LiftCurve::of(curve, lo, hi, false)?;
    let (uv_min, uv_max) = uv_c.uv_bounds(lo, hi)?;
    let srf = LiftSurface::of(k, surface, uv_min, uv_max)?;

    let eval_span = |a: f64, b: f64| -> Result<LiftPart> {
        let m = 0.5 * (a + b);
        let t = Ivl::new(a, b);
        let dt = t - Ivl::from(m);
        let r = Ivl::from(dt.mag());
        let half_r2 = Ivl::new(0.0, (r * r * Ivl::from(0.5)).hi);
        let [w0, w1, w2] = uv_c.derivs(t, 2)?;
        let [c0, c1, c2] = c3_c.derivs(t, 2)?;
        let [wm0, wm1, _] = uv_c.derivs(Ivl::from(m), 1)?;
        let [cm0, cm1, _] = c3_c.derivs(Ivl::from(m), 1)?;
        srf.lift_part(&CurveSpanEval {
            w0,
            w1,
            w2,
            wm0,
            wm1,
            c0,
            c1,
            c2,
            cm0,
            cm1,
            dt,
            half_r2,
        })
    };

    for end_t in [lo, hi] {
        let end_part = eval_span(end_t, end_t)?;
        if end_part.at_middle > tol {
            return Err(lift_too_far());
        }
    }

    let mut cuts = vec![lo, hi];
    uv_c.push_cuts(lo, hi, &mut cuts);
    c3_c.push_cuts(lo, hi, &mut cuts);
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
        let part = eval_span(a, b)?;
        if part.bound <= tol {
            worst = worst.max(part.bound);
            continue;
        }
        if part.at_middle > tol {
            return Err(lift_too_far());
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
        let common = common_trim_interval(&k, &[&piece, &uv1, &uv2], ivl, OP).unwrap();
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

        let common = common_trim_interval(&k, &[&c3, &uv1, &uv2], ivl, OP).unwrap();
        assert_eq!((common.min(), common.max()), (start, end));
        for c in [&c3, &uv1, &uv2] {
            let mut c = c.copy(&k).unwrap();
            trim_unsnapped(&k, &mut c, common, OP).unwrap();
            assert_eq!(c.natural_interval(), common);
        }
        // a curve whose own trim snaps an end within the snap tolerance (its knot 2 ulps
        // past 0.5) is reparametrised onto the interval asked: it ends there, at the point
        // of its own knot, every point of it kept
        let mut snapping = uv2.copy(&k).unwrap();
        trim_unsnapped(&k, &mut snapping, ivl, OP).unwrap();
        assert_eq!(snapping.natural_interval(), ivl);
        let at = |c: &Curve, t: f64| c.evaluate_point(&k, t).unwrap();
        assert_eq!(at(&snapping, 0.5), at(&uv2, end));
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
        let SurfaceKind::Plane(p) = plane.kind() else {
            panic!("a plane")
        };
        let nurbs = Surface::from(p.base().clone());
        let err = ssi
            .clip_exact_curves(&k, &sphere, &d1, &nurbs, &d2, 1e-6)
            .unwrap_err();
        assert!(matches!(err, SolidError::Unsupported { operation: OP, .. }));
    }

    #[test]
    fn certified_lift_checks_supremum_on_plane_bspline_and_offset() {
        use crate::curve::line::Line;
        use crate::math::Point3;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NlSurface;
        use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};

        let k = Kernel::default();
        let tol = 1e-5;
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let uvc = Curve::from(Line::from_points(
            Point3::new(0.0, 0.5, NOZ),
            Point3::new(1.0, 0.5, NOZ),
            2,
        ));

        // 1. Plane z = 0 with cubic `c3(t) = (t, 0.5, (9/4) * peak * 3 t (1 - t)^2)`:
        //    peaks at `t = 1/3` (`peak`), while `g(0) = 0`, `g(0.5) = (27/32) * peak`, `g(1) = 0`.
        let make_bumped_line = |peak: f64| -> Curve {
            let z1 = 2.25 * peak;
            let pw = vec![
                CPoint::new(0.0, 0.5, 0.0, NOW),
                CPoint::new(1.0 / 3.0, 0.5, z1, NOW),
                CPoint::new(2.0 / 3.0, 0.5, 0.0, NOW),
                CPoint::new(1.0, 0.5, 0.0, NOW),
            ];
            let nl = NlCurve::new(pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
            Curve::from(BSplineCurve::from_nurb(nl, 3, false))
        };
        let plane = Surface::from(Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap());
        let bound_ok =
            certified_lift(&k, &plane, &uvc, &make_bumped_line(0.9 * tol), ivl, tol).unwrap();
        assert!((0.9 * tol..=tol).contains(&bound_ok), "{bound_ok}");
        let bumped_over = make_bumped_line(1.1 * tol);
        // Endpoints and midpoint are all strictly below `tol` (`0`, `0.928 * tol`, `0`),
        // yet the supremum at `t = 1/3` is `1.1 * tol > tol`.
        assert!(bumped_over.evaluate_point(&k, 0.5).unwrap().z < tol);
        let err_over = certified_lift(&k, &plane, &uvc, &bumped_over, ivl, tol).unwrap_err();
        assert!(matches!(
            err_over,
            SolidError::Unsupported { operation: OP, .. }
        ));

        // 2. Curved quadratic B-spline surface `S(u, v) = (u, v, 0.5 u^2)` (`u, v in [0, 1]`)
        //    and 3. procedural `OffsetSurface` at distance `d = 0.4` over `S`.
        let srf_cpts = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(0.5, 0.0, 0.0, NOW),
                CPoint::new(0.5, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(1.0, 0.0, 0.5, NOW),
                CPoint::new(1.0, 1.0, 0.5, NOW),
            ],
        ];
        let nl_srf = NlSurface::new(
            srf_cpts,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let curved_srf = Surface::from(BSplineSurface::from_nurb(nl_srf, false));
        let make_curved_c3 = |peak: f64| -> Curve {
            let z1 = 2.25 * peak;
            let pw = vec![
                CPoint::new(0.0, 0.5, 0.0, NOW),
                CPoint::new(1.0 / 3.0, 0.5, z1, NOW),
                CPoint::new(2.0 / 3.0, 0.5, 1.0 / 6.0, NOW),
                CPoint::new(1.0, 0.5, 0.5, NOW),
            ];
            let nl = NlCurve::new(pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
            Curve::from(BSplineCurve::from_nurb(nl, 3, false))
        };
        assert!(
            certified_lift(&k, &curved_srf, &uvc, &make_curved_c3(0.9 * tol), ivl, tol).is_ok()
        );
        assert!(matches!(
            certified_lift(&k, &curved_srf, &uvc, &make_curved_c3(1.15 * tol), ivl, tol),
            Err(SolidError::Unsupported { operation: OP, .. })
        ));

        let offset_srf =
            Surface::from(OffsetSurface::new(&k, 0.4, curved_srf, true, SS_UNKNOWN).unwrap());
        let dom = offset_srf.natural_uv_domain_with_nurb(&k).unwrap();
        let p0 = offset_srf
            .evaluate_point(&k, Point2::new(0.5, 0.0))
            .unwrap();
        let p1 = offset_srf
            .evaluate_point(&k, Point2::new(0.5, 1.0))
            .unwrap();
        let exact_3d = Curve::from(Line::from_points(p0, p1, 3));
        let dropped = offset_srf
            .drop_curve::<NoObject>(&k, &dom, &exact_3d, &ivl, tol, false, None, true)
            .unwrap();
        let dropped_uv = dropped.uv_curves.into_iter().next().unwrap();
        assert!(certified_lift(&k, &offset_srf, &dropped_uv, &exact_3d, ivl, tol).is_ok());

        let z_bump = 2.25 * 1.15 * tol;
        let bumped_pw = vec![
            CPoint::new(p0.x, p0.y, p0.z, NOW),
            CPoint::new(
                (2.0 * p0.x + p1.x) / 3.0,
                (2.0 * p0.y + p1.y) / 3.0,
                (2.0 * p0.z + p1.z) / 3.0 + z_bump,
                NOW,
            ),
            CPoint::new(
                (p0.x + 2.0 * p1.x) / 3.0,
                (p0.y + 2.0 * p1.y) / 3.0,
                (p0.z + 2.0 * p1.z) / 3.0,
                NOW,
            ),
            CPoint::new(p1.x, p1.y, p1.z, NOW),
        ];
        let bumped_nl =
            NlCurve::new(bumped_pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
        let bumped_off_3d = Curve::from(BSplineCurve::from_nurb(bumped_nl, 3, false));
        assert!(matches!(
            certified_lift(&k, &offset_srf, &dropped_uv, &bumped_off_3d, ivl, tol),
            Err(SolidError::Unsupported { operation: OP, .. })
        ));
    }

    #[test]
    fn certified_lift_refuses_procedural_blend_and_offset_of_blend() {
        use crate::blend::fillet_section::recertify::net_of_definition;
        use crate::blend::fillet_section::section::GnSectionFsg;
        use crate::curve::line::Line;
        use crate::math::Point3;
        use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
        use crate::surface::procedural_blend::{BlendDefinitionInput, ProceduralBlend};

        let k = Kernel::default();

        let (l, r) = (10.0, 2.0);
        let o = Point3::new(0.0, 0.0, 0.0);
        let bilinear = |p: [Point3; 4]| {
            BSplineSurface::create_bilinear_surface(p[0], p[1], p[2], p[3]).unwrap()
        };
        let supp_a = Surface::from(bilinear([
            o,
            Point3::new(2.0 * l, 0.0, 0.0),
            Point3::new(0.0, 0.0, l),
            Point3::new(2.0 * l, 0.0, l),
        ]));
        let supp_b = Surface::from(bilinear([
            o,
            Point3::new(2.0 * l, 0.0, 0.0),
            Point3::new(0.0, l, 0.0),
            Point3::new(2.0 * l, l, 0.0),
        ]));
        let toy = bilinear([
            o,
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ]);
        let seg = |a: Point3, b: Point3| BSplineCurve::create_line_segment(3, a, b).unwrap();
        let leader = seg(Point3::new(0.0, r, r), Point3::new(l, r, r));
        let seed = seg(Point3::new(0.0, r / l, 0.0), Point3::new(0.5, r / l, 0.0));
        let input = BlendDefinitionInput {
            supports: [&supp_a, &supp_b],
            sides: [-1.0, 1.0],
            offsets: [r, r],
            law: None,
            order: 2,
            thumbweight: 0.5,
            leader: &leader,
            seeds: [&seed, &seed],
            bound: 1e-3,
            chamfer: None,
        };
        let valid = |n: u32, w: f64| GnSectionFsg::is_valid(n as usize, w);
        let uncert = ProceduralBlend::create(&k, &toy, &input, valid).unwrap();
        let (net, bound) = net_of_definition(&k, &uncert, 1e-12).unwrap();
        let pb = ProceduralBlend::create(&k, &net, &BlendDefinitionInput { bound, ..input }, valid)
            .unwrap();
        let pb_srf = Surface::from(pb);
        let dom = pb_srf.natural_uv_domain_with_nurb(&k).unwrap();
        let (u0, u1) = (dom.min().x, dom.max().x);
        let vm = 0.5 * (dom.min().y + dom.max().y);

        let p0 = pb_srf.evaluate_point(&k, Point2::new(u0, vm)).unwrap();
        let p1 = pb_srf.evaluate_point(&k, Point2::new(u1, vm)).unwrap();
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let c3 = Curve::from(Line::from_points(p0, p1, 3));
        let uvc = Curve::from(Line::from_points(
            Point3::new(u0, vm, NOZ),
            Point3::new(u1, vm, NOZ),
            2,
        ));

        let net_srf = Surface::from(net);
        assert!(certified_lift(&k, &net_srf, &uvc, &c3, ivl, 1e-3).is_ok());
        assert!(matches!(
            certified_lift(&k, &pb_srf, &uvc, &c3, ivl, 1e-3),
            Err(SolidError::Unsupported {
                operation: OP,
                reason: APPROXIMATE_NET_REASON,
            })
        ));

        let off_pb = Surface::from(OffsetSurface::new(&k, 0.2, pb_srf, true, SS_UNKNOWN).unwrap());
        let q0 = off_pb.evaluate_point(&k, Point2::new(u0, vm)).unwrap();
        let q1 = off_pb.evaluate_point(&k, Point2::new(u1, vm)).unwrap();
        let c3_off = Curve::from(Line::from_points(q0, q1, 3));
        assert!(matches!(
            certified_lift(&k, &off_pb, &uvc, &c3_off, ivl, 1e-3),
            Err(SolidError::Unsupported {
                operation: OP,
                reason: APPROXIMATE_NET_REASON,
            })
        ));
    }

    #[test]
    fn certified_lift_outward_rounding_unbounded_cone_and_depth_cap() {
        use crate::curve::line::Line;
        use crate::math::Point3;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::CPoint;
        use crate::nurbs::srf_basic::Surface as NlSurface;
        use crate::surface::cone::Cone;
        use crate::surface::offset_surface::OffsetSurface;

        let k = Kernel::default();
        let shift = 1e6_f64;
        let tol = 1e-5_f64;
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let srf_cpts = vec![
            vec![
                CPoint::new(shift, shift, shift, NOW),
                CPoint::new(shift, shift + 1.0, shift, NOW),
            ],
            vec![
                CPoint::new(shift + 0.5, shift, shift, NOW),
                CPoint::new(shift + 0.5, shift + 1.0, shift, NOW),
            ],
            vec![
                CPoint::new(shift + 1.0, shift, shift + 0.5, NOW),
                CPoint::new(shift + 1.0, shift + 1.0, shift + 0.5, NOW),
            ],
        ];
        let nl_srf = NlSurface::new(
            srf_cpts,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        let shifted_srf = Surface::from(BSplineSurface::from_nurb(nl_srf, false));
        let uvc = Curve::from(Line::from_points(
            Point3::new(0.0, 0.5, NOZ),
            Point3::new(1.0, 0.5, NOZ),
            2,
        ));
        let make_shifted_c3 = |peak: f64| -> Curve {
            let pw = vec![
                CPoint::new(shift, shift + 0.5, shift, NOW),
                CPoint::new(shift + 1.0 / 3.0, shift + 0.5, shift + 2.25 * peak, NOW),
                CPoint::new(shift + 2.0 / 3.0, shift + 0.5, shift + 1.0 / 6.0, NOW),
                CPoint::new(shift + 1.0, shift + 0.5, shift + 0.5, NOW),
            ];
            let nl = NlCurve::new(pw, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap();
            Curve::from(BSplineCurve::from_nurb(nl, 3, false))
        };
        assert!(certified_lift(
            &k,
            &shifted_srf,
            &uvc,
            &make_shifted_c3(0.9 * tol),
            ivl,
            tol
        )
        .is_ok());
        assert!(matches!(
            certified_lift(
                &k,
                &shifted_srf,
                &uvc,
                &make_shifted_c3(1.15 * tol),
                ivl,
                tol
            ),
            Err(SolidError::Unsupported { operation: OP, .. })
        ));

        // `WR-713`: asymmetric unbounded inside-out `Cone` without `cone_unbounded_parameterized_as_nurb`
        let cone_srf = Surface::from(
            Cone::new_unbounded(&k, Point3::ZERO, Vec3::X, Vec3::Y, 5.0, 30.0, false, true)
                .unwrap(),
        );
        assert!(exact_nurbs_surface(&k, &cone_srf).unwrap().is_none());
        let mut k_fixed = Kernel::default();
        k_fixed.fixes.cone_unbounded_parameterized_as_nurb = true;
        let cone_fixed = Surface::from(
            Cone::new_unbounded(
                &k_fixed,
                Point3::ZERO,
                Vec3::X,
                Vec3::Y,
                5.0,
                30.0,
                false,
                true,
            )
            .unwrap(),
        );
        assert!(exact_nurbs_surface(&k_fixed, &cone_fixed)
            .unwrap()
            .is_some());

        // Rational curve homogeneous-to-Cartesian rounding and `LIFT_MAX_DEPTH` cap refusal:
        // with weight `3.0`, adjacent homogeneous coordinates `wx_a` and `wx_b` (`+1` ULP)
        // near `1e6` map to the identical `canonical()` float via `x * (1.0 / 3.0)`, while
        // `exact_nurbs_curve_data` preserves raw `gw_nurb()` homogeneous `CPoint`s. Below
        // `Ivl`'s rounding width at `x ~ 3.33e5` (`tol = 1e-11`), subdivision reaches
        // `LIFT_MAX_DEPTH` and refuses with `lift_not_certified()`.
        let step_ulp = |x: f64| f64::from_bits(x.to_bits() + 1);
        let plane = Surface::from(Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap());
        let w = 3.0_f64;
        let wx_a = step_ulp(1_000_000.0);
        let wx_b = step_ulp(wx_a);
        assert_eq!(wx_a * (1.0 / w), wx_b * (1.0 / w));
        let rat_3d = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![CPoint::new(wx_a, 0.0, 0.0, w), CPoint::new(wx_a, w, 0.0, w)],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        ));
        let rat_uv = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![CPoint::new(wx_b, 0.0, NOZ, w), CPoint::new(wx_b, w, NOZ, w)],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            2,
            false,
        ));
        let (CurveKind::BSpline(b3), CurveKind::BSpline(b2)) = (rat_3d.kind(), rat_uv.kind())
        else {
            panic!("expected rational B-spline curves");
        };
        assert_eq!(
            b3.canonical().unwrap().control_points,
            b2.canonical().unwrap().control_points,
        );
        assert_ne!(
            exact_nurbs_curve_data(b3, false).unwrap().0.pw,
            exact_nurbs_curve_data(b2, true).unwrap().0.pw,
        );
        assert_eq!(
            certified_lift(&k, &plane, &rat_uv, &rat_3d, ivl, 1e-11).unwrap_err(),
            lift_not_certified(),
        );
        assert!(certified_lift(&k, &plane, &rat_uv, &rat_3d, ivl, 1e-8).is_ok());

        // Boundary iso-line (`u = 0`) on `OffsetSurface` with `allow_extension = true`, and
        // end-span extrapolation past `[0, 1]` on `BSplineSurface`.
        let unit_patch = BSplineSurface::from_nurb(
            NlSurface::new(
                vec![
                    vec![
                        CPoint::new(0.0, 0.0, 0.0, NOW),
                        CPoint::new(0.0, 1.0, 0.0, NOW),
                    ],
                    vec![
                        CPoint::new(1.0, 0.0, 0.0, NOW),
                        CPoint::new(1.0, 1.0, 0.0, NOW),
                    ],
                ],
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap(),
            true,
        );
        let bsp_srf = Surface::from(unit_patch);
        let mut off = OffsetSurface::new(&k, 2.0, bsp_srf.clone(), true, 0).unwrap();
        off.set_allow_extension(true);
        let off_srf = Surface::from(off);
        let iso_u0 = Curve::from(Line::from_points(
            Point3::new(0.0, 0.0, NOZ),
            Point3::new(0.0, 1.0, NOZ),
            2,
        ));
        let off_edge_ok = Curve::from(Line::from_points(
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(0.0, 1.0, 2.0),
            3,
        ));
        let off_edge_bad = Curve::from(Line::from_points(
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(0.0, 1.0, 2.001),
            3,
        ));
        assert!(certified_lift(&k, &off_srf, &iso_u0, &off_edge_ok, ivl, 1e-12).is_ok());
        assert_eq!(
            certified_lift(&k, &off_srf, &iso_u0, &off_edge_bad, ivl, 1e-12).unwrap_err(),
            lift_too_far(),
        );
        let uv_extrap = Curve::from(Line::from_points(
            Point3::new(0.5, -0.25, NOZ),
            Point3::new(0.5, 1.25, NOZ),
            2,
        ));
        let c3_extrap = Curve::from(Line::from_points(
            Point3::new(0.5, -0.25, 0.0),
            Point3::new(0.5, 1.25, 0.0),
            3,
        ));
        assert!(certified_lift(&k, &bsp_srf, &uv_extrap, &c3_extrap, ivl, 1e-12).is_ok());
    }

    /// #13790: a typed `SM_ERR` that replaced the certificate's `Upstream` subdivides the
    /// span as the `Upstream` does.
    #[test]
    fn certificate_sm_err_reads_the_status_of_a_typed_failure() {
        use crate::error::replaced_upstream_samples::{native_failures, replaced_upstream};
        let op = "blend::fillet_section::certificate";
        assert!(is_certificate_sm_err(&SolidError::sm_err(op)));
        assert!(matches!(
            partials_or_subdivide(Err(SolidError::sm_err(op))),
            Ok(None)
        ));
        for typed in replaced_upstream(op, crate::error::SM_ERR, None) {
            assert!(is_certificate_sm_err(&typed), "{typed:?}");
            assert!(matches!(partials_or_subdivide(Err(typed)), Ok(None)));
        }
        for other in replaced_upstream(op, 1006, None)
            .into_iter()
            .chain(native_failures(op, crate::error::SM_ERR))
        {
            assert!(!is_certificate_sm_err(&other), "{other:?}");
            assert!(partials_or_subdivide(Err(other)).is_err());
        }
    }
}
