//! A surface extended past one side of its parameter box by a distance measured across that
//! side, exactly: [`Surface::extended_by_distance`].
//!
//! Not upstream: upstream's extension (`CreateExtendedSurface` on `N_SrfExtendByDist`) estimates
//! its distance, fits a rational curvature-continuous extension by least squares and ends by
//! removing knots within a tolerance, which can move the source's own part. It is not ported;
//! every surface extension in the kernel is this exact one.
//!
//! **The construction.** The surface's B-spline form (an analytic kind's only once proved on
//! its label, [`AnalyticSupport::check`]) is worked on its homogeneous net `(H, w)`, one row of
//! control points along the extended direction at a time:
//!
//! - **Natural:** the boundary span's polynomial continued (on the homogeneous rows, so a
//!   rational row continues as the same rational curve: a conic stays on its conic).
//! - **Tangent:** the boundary piece mirrored: the extension at `b + s` is `L C(b - s)`, `L` the
//!   linear map of `R^4` that fixes the hyperplane through the end point lifted from the
//!   Euclidean mirror plane (normal to the row's last leg) and reverses the row's homogeneous end
//!   derivative. For a polynomial row it is the Euclidean mirror; on every row the homogeneous
//!   surface is C1 across the side, so the surface is (where `w > 0`). A row whose end has no
//!   tangent (its last leg is no longer than its rounding: a pole) is mirrored through its end
//!   point, `2 P_n - C(b - s)`, which is C1 on the homogeneous row too. An extension longer than
//!   the surface's parameter range mirrors in steps, each step the last.
//! - **Curvature:** the Tangent construction with the extension's second row solved from the C2
//!   condition on the homogeneous rows (linear, in closed form): the homogeneous surface is C2
//!   across the side, so the surface is curvature continuous.
//! - **Ruled:** each row continued past its end by the homogeneous line `H_n + s D`, `D = (p
//!   / h) (rho H_n - H_(n-1))` (`h` the last span's length), with ONE `rho` for every row:
//!   the rows' largest end weight ratio `w_(n-1) / w_n` (1 for polynomial rows). It is
//!   written as one degree-`p` span by degree elevation of the line, joined at multiplicity
//!   `p` (the joint point the source's own). A line in homogeneous space projects to a
//!   straight line, so at a fixed parameter along the side the result is straight in `s`:
//!   ruled. Across the side the homogeneous derivative is `H_u + mu H` with one `mu = (p /
//!   h) (rho - 1)` for the whole side, the same tangent as `H_u` once projected: the surface
//!   is C1 there exactly (`S_s = (D W - H D_w) / W^2 = S_u`), on polynomial and rational
//!   rows alike, its direction the side's end tangent. Every weight is `w_n (1 + (i e / h)
//!   (rho - rho_j)) >= w_n`. Where every row has the same ratio (every polynomial row; a
//!   rational net whose weights are a product along and across the side, as a revolution's
//!   or an extrusion's) the weights stay `w_n`: each row is the 3D segment `P_n + s T`
//!   along its end tangent `T = C'(b)`, equally spaced, reaching any distance. Where the
//!   ratios differ (a variable round's sections) a row with a ratio below the largest has
//!   growing weights and its line ends at a finite point: the length across saturates, a
//!   bounded reach. Past that reach (an ask this form refuses as not exact, #2878) the
//!   extension is the ruled surface `S + s S_u` itself, in the homogeneous form `(H W + s
//!   (H_u W - H W_u), W^2)` (`H`, `W` the side's homogeneous points and weight along it,
//!   `H_u`, `W_u` their derivatives along the rows): its weight does not change along `s`,
//!   so every row is straight and reaches any distance, C1 at the joint. It is of degree
//!   `2q` along the side, so the source is written in that form too, its homogeneous points
//!   times `W`: the same surface and parameters, with [`Extended::source`] the rounding of
//!   that product, not 0. Where that form has no B-spline (not clamped across the side, a
//!   knot across it of full multiplicity, a degree `2q` past [`MAX_DEGREE`]) or does not
//!   reach the ask either, the refusal stands, naming the longest distance found.
//!
//! **Knots.** The source's net and knots are kept bit for bit (but for the Ruled product
//! form past its reach, above): the end knot stays at multiplicity `p` as the joint, and the extension's
//! spans follow it. No knot is removed.
//!
//! **Weights.** Every weight control point of the result is positive, which proves `w > 0` on
//! it. An extension whose weights are not is refused ([`SolidError::ExtendRefused`]) naming a
//! distance that works: the length of an ask, backing off from the longest positive one found
//! by bisection, whose length was measured (asking for it succeeds). Positivity need not be
//! monotone in the ask, so it is a long workable distance, not proved the longest.
//!
//! **The distance.** The extension parameter is solved (bracketed, then Illinois false
//! position) so that the length across the side at its middle
//! ([`Surface::length_across_side`]) is the distance: until the length's certified enclosure
//! lies within `max(1e-9 d, its truncation and rounding parts and the stored net's
//! resolution)` of `d`, as the measure reports them (the resolution: how far one ulp of the
//! stored points' coordinates moves the length, [`Across::resolution`]; no net of floats is
//! nearer to `d` than that, about `2.7e-14` at a part 230 from the origin: a stop allowance,
//! since stopping a length solve tighter than one ulp of the stored net can move the length
//! is not resolvable; the certificate is unchanged), with the truncation (series cutoff) part
//! at most `1e-9 d` and the rounding part at
//! most `1e-6 d` (owner); the extension's own rounding bound ([`Extended::extension`]) is a
//! rounding part too, held to `1e-6 d`. An extension whose length at `d` cannot be measured
//! so (a leaf that did not converge, a rounding part past `1e-6 d`, an extension bound past
//! it, as a Curvature mirror's, whose C2 rows carry the rounding of the rows before them) is
//! refused as not exact ([`ExtendRefusal::NotExact`]), naming the longest distance found
//! whose length does meet it (by bisection on the extension parameter: that need not be
//! monotone either, so it is a long workable distance, not proved the longest).
//!
//! **Analytic kinds.** Natural on a plane, cone, sphere or torus is the same analytic widened
//! ([`Surface::continued`]). So is Tangent, Curvature or Ruled across a side whose B-spline form
//! is of degree 1 across it (a plane; a cone or cylinder along its rulings): a straight row's
//! mirror, its C2 condition and its tangent line are its own continuation.
//!
//! **Blends.** A procedural blend is extended by its definition, not its net (`blend`).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::cell::Cell;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::scaled_zero2;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_PARAM;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::bezier::product_matrix;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::gamma;
use crate::nurbs::math::{CPoint, RealMatrix, NOW};
use crate::nurbs::srf_basic::{Direction, Surface as Net};
use crate::nurbs::srf_tool::p1::insert_knot;
use crate::surface::implicit::AnalyticSupport;
use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN};
use crate::surface::side::{Across, BoxSide};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::extend::Surface::extended_by_distance";

/// The relative accuracy the across length is measured to: each leaf's polygon within this
/// of its chord, so the truncation part is at most this of the length, inside [`STOP`].
const REL: f64 = 0.5 * STOP;
/// The stop on the across length, relative to the distance: the truncation part must reach
/// it, and the enclosure lie within it of the distance where it is narrower.
const STOP: f64 = 1e-9;
/// The most the rounding part of the across length may be, relative to the distance.
const ROUNDING: f64 = 1e-6;
/// Asks that scale the extension parameter until it brackets the distance.
const BRACKET_TRIES: usize = 60;
/// Illinois steps inside the bracket.
const SOLVE_TRIES: usize = 200;
/// Asks backing off from the longest with positive weights, to name the longest distance
/// proved to work.
const BACK_OFFS: usize = 30;
/// Halvings that find the longest extension with positive weights.
const BISECTIONS: usize = 60;

/// Growth steps of [`Local::least_step`] (each closes the rounding shortfall; two do).
const LEAST_STEP_TRIES: usize = 8;
/// A budget on one Tangent or Curvature extension's mirror steps: a limit on its size, not on
/// any convergence. The step count follows from the ask: about `log2(e / range) + 1` for
/// Tangent (each step mirrors the whole piece, doubling it), `ceil(e / h)` for Curvature (one
/// span of the last span's length `h` a step).
const MAX_STEPS: usize = 4096;

/// How a surface continues past its side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Continuity {
    /// The surface itself, continued: its boundary span's polynomial.
    Natural,
    /// Tangent continuous: the boundary piece mirrored.
    Tangent,
    /// Curvature continuous: the mirror with its second row from the C2 condition.
    Curvature,
    /// Tangent continuous and ruled: each row continued by a homogeneous line along its end
    /// tangent (a straight segment in 3D), a degree-`p` span by degree elevation of the line.
    Ruled,
}

/// Why an extension is refused ([`SolidError::ExtendRefused`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtendRefusal {
    /// A weight of the extension would not be positive within the distance asked; the refusal
    /// names the longest distance whose weights are.
    WeightVanishes,
    /// A sphere's latitude would run past its pole; the refusal names the arc to the pole.
    PastPole,
    /// A cone would run past its apex; the refusal names the slant length to it, less its
    /// rounding.
    PastApex,
    /// A sweep or tube angle would run over a full turn; the refusal names the arc to the
    /// full turn.
    OverATurn,
    /// The length across the side at the distance could not be measured to the stop (a leaf
    /// too short to cut again before it converged, a rounding part or an extension bound past
    /// `1e-6` of the distance): the refusal names the longest distance found that is, if
    /// any.
    NotExact,
}

/// An extended surface and its certified bounds.
#[derive(Debug, Clone)]
pub struct Extended {
    /// The extension: the same analytic surface widened, or a B-spline surface.
    pub surface: Surface,
    /// (a) How far the result over the source's box is from the source: 0, proved (the
    /// result's net and knots there are the source's, bit for bit; an analytic's label and
    /// form are the source's). Ruled past the reach of rows whose end weight ratios differ
    /// writes the source in its product form (module docs): the same surface and parameters,
    /// this the certified 3D bound of that product's rounding.
    pub source: f64,
    /// How far the extension's part past the side is from the construction's definition (the
    /// continued polynomial, the mirror, the C2 row): a certified bound on the 3D distance,
    /// from the rounding of making its net. 0 for an analytic widened, its label exact.
    pub extension: f64,
    /// (b) The length across the side, at its middle, from the side to the result's end: its
    /// certified enclosure and the enclosure's parts.
    pub across: Across,
    /// The face box `dom` on the result's parameters (an analytic widened may reparametrise).
    pub dom: Extent2d,
    /// The side of the result's parameter box each side of the source's is on, in
    /// [`BoxSide::ALL`]'s order (an analytic widened may reverse a direction).
    pub sides: [BoxSide; 4],
}

impl Extended {
    /// The side of the result's box that side `s` of the source's is on.
    #[must_use]
    pub fn side_of(&self, s: BoxSide) -> BoxSide {
        BoxSide::ALL
            .iter()
            .position(|&x| x == s)
            .and_then(|i| self.sides.get(i))
            .copied()
            .unwrap_or(s)
    }
}

impl Surface {
    /// Whether `dom` is exactly the box of the surface's NURBS form, as
    /// [`Surface::extended_by_distance`] needs a spline's face box to be: every bound equal.
    ///
    /// # Errors
    ///
    /// Those of the natural box query.
    pub fn is_whole_box(&self, k: &Kernel, dom: &Extent2d) -> Result<bool> {
        let n = self.natural_uv_domain_with_nurb(k)?;
        Ok(n.min().x == dom.min().x
            && n.min().y == dom.min().y
            && n.max().x == dom.max().x
            && n.max().y == dom.max().y)
    }
}

impl Surface {
    /// The surface continued `distance` past side `side` of its face box `dom` (in its NURBS
    /// parameters) as `how` says, exactly (module docs): the result with (a) its bound against
    /// the source and (b) its across length's enclosure, within `max(1e-9 distance, the
    /// enclosure's width)` of `distance`, its truncation part at most `1e-9 distance` and its
    /// rounding part at most `1e-6 distance`. A B-spline extension is past the side of the
    /// surface's B-spline form, which `dom`'s side must be (the face its whole box there).
    ///
    /// `None` where the distance is below the side's resolution ([`below_resolution`]): the
    /// shortest extension the side's knot can carry is proved past it, so no extension is
    /// made and the surface is used as it is. The same rule holds for an analytic widened on
    /// its own parameters (Natural, or straight across the side) where the face box ends at
    /// its form's end.
    ///
    /// # Errors
    ///
    /// - `InvalidInput`: a distance not finite and positive; a side not the form's end; a
    ///   form moved off its analytic label; a form not clamped at the side;
    /// - `Unsupported`: a kind with no exact B-spline form here (a curve bounded, STEP or
    ///   in-volume surface); an offset surface is the offset of its base extended
    ///   (`offset_extended`), with its base's errors; a procedural blend is extended by its
    ///   definition (`blend`: across its section continued, along its spine by a span of its
    ///   construction), refused for a G^n section across, a net cut across or turned, or a
    ///   side that is not its definition's end station;
    /// - [`SolidError::ExtendRefused`]: a weight would not be positive, with a distance whose
    ///   weights are ([`ExtendRefusal::WeightVanishes`]); the length at the distance not
    ///   measurable to the stop, with the longest distance found that is
    ///   ([`ExtendRefusal::NotExact`]);
    /// - `NotConverged`: the distance not bracketed or not met;
    /// - those of [`Surface::continued`], [`Surface::length_across_side`] and knot insertion.
    pub fn extended_by_distance(
        &self,
        k: &Kernel,
        dom: &Extent2d,
        side: BoxSide,
        how: Continuity,
        distance: f64,
    ) -> Result<Option<Extended>> {
        if !(distance.is_finite() && distance > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "the distance is not finite and positive",
            ));
        }
        if let SurfaceKind::OffsetSurface(o) = self.kind() {
            return offset_extended(k, o, dom, side, how, distance);
        }
        if let SurfaceKind::ProceduralBlend(pb) = self.kind() {
            return blend::blend_extended(k, pb, dom, side, distance);
        }
        let analytic = AnalyticSupport::of(self).is_some();
        let form = crate::blend::fillet_section::station::as_b_spline(k, self)?.ok_or(
            SolidError::Unsupported {
                operation: OP,
                reason: "the surface has no exact B-spline form",
            },
        )?;
        let net = form.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
        let degree = if side.is_u() { net.p() } else { net.q() };
        let source = Local::of(net, side)?;
        let fixed = side.at(dom);
        let end = if side.is_high() {
            source.end()?
        } else {
            -source.end()?
        };
        let made = |product: bool, e: f64| -> Result<Option<Made>> {
            let grown = if product {
                Some(source.ruled_product(e)?).filter(Local::is_positive)
            } else {
                source.extended(how, e)?
            };
            let Some(grown) = grown else {
                return Ok(None);
            };
            // a net that overflowed is no extension to measure: unproved, not refused
            if !grown.is_finite() {
                return Ok(Some((None, f64::INFINITY)));
            }
            let bound = grown.bound();
            let out = grown.surface(side)?;
            Ok(Some((
                Some(Surface::from(BSplineSurface::from_gw_nurb(&out))),
                bound,
            )))
        };
        // a length past the measure's leaf budget is no measurement, as a leaf not converged
        let across =
            |m: &Option<Surface>| match m.as_ref().map(|s| s.length_across_side(k, dom, side, REL))
            {
                None | Some(Err(SolidError::NotConverged { .. })) => Ok(None),
                Some(Ok(a)) => Ok(Some(a)),
                Some(Err(e)) => Err(e),
            };
        if analytic && (how == Continuity::Natural || degree == 1) {
            // the one rule on every path: a distance the side's least step is proved past
            // is below its resolution, the side left as it is (where the face box ends at
            // the form's end, the step its knot can carry; inside the form the widening
            // moves the box alone and has no such step)
            if fixed == end && below_least_step(&|e| made(false, e), &across, &source, distance)? {
                return Ok(None);
            }
            // proved once, by the continuation
            return widened(k, self, dom, side, distance).map(Some);
        }
        if analytic {
            AnalyticSupport::check(k, self)?;
        }
        if fixed != end {
            return Err(SolidError::invalid_input(
                OP,
                "the side is not the end of the surface's B-spline form",
            ));
        }
        let solved = match solve(&|e| made(false, e), &across, &source, distance) {
            // past the rows' reach: the ruled surface itself (#2878), or the refusal naming
            // the longer of the two forms' distances
            Err(SolidError::ExtendRefused {
                refusal: ExtendRefusal::NotExact,
                distance: named,
                ..
            }) if how == Continuity::Ruled && source.has_product_form() => {
                match solve(&|e| made(true, e), &across, &source, distance) {
                    Err(SolidError::ExtendRefused {
                        refusal: ExtendRefusal::NotExact,
                        distance: reached,
                        ..
                    }) => Err(not_exact(match (named, reached) {
                        (Some(x), Some(y)) => Some(x.max(y)),
                        (x, y) => x.or(y),
                    })),
                    other => other,
                }
            }
            other => other,
        };
        let Some(((surface, extension), enclosure)) = solved? else {
            return Ok(None);
        };
        let surface = surface.ok_or_else(op_err)?;
        // a bound that is not finite certifies nothing of the extension (its net's rounding
        // ran away, as near a vanishing weight): not exact (the solve met none such)
        if !extension.is_finite() {
            return Err(not_exact(None));
        }
        // (a): the source's net and knots, bit for bit, over its box; the Ruled product form
        // writes the source of degree 2 q across, within its rounding bound (#2878)
        let SurfaceKind::BSpline(b) = surface.kind() else {
            return Err(SolidError::port_invariant(
                OP,
                "the extension is not a B-spline",
            ));
        };
        let out = b.gw_nurb().ok_or_else(|| SolidError::sm_err(OP))?;
        let regraded = if side.is_u() {
            out.q() != net.q()
        } else {
            out.p() != net.p()
        };
        if !regraded && !keeps(net, out, side)? {
            return Err(SolidError::port_invariant(
                OP,
                "the extension moved the source's net",
            ));
        }
        Ok(Some(Extended {
            surface,
            source: if regraded {
                source.product_source_bound()?
            } else {
                0.0
            },
            extension,
            across: enclosure,
            dom: *dom,
            sides: BoxSide::ALL,
        }))
    }
}

/// An offset surface extended: the offset, by the same distance, of its base extended past
/// the same side by the same rule ([`Surface::extended_by_distance`] on the base, over the
/// same parameters: an offset surface is parametrised by its base). Exact by construction:
/// the result is the offset surface of that base, so its bounds are the base's: (a) over the
/// source's box (0, or the base's own for the Ruled product form), the extension's
/// construction bound, and the length across the side measured on the base (the offset's own
/// length across differs from it by the offset's curvature term, which this does not
/// measure).
fn offset_extended(
    k: &Kernel,
    o: &OffsetSurface,
    dom: &Extent2d,
    side: BoxSide,
    how: Continuity,
    distance: f64,
) -> Result<Option<Extended>> {
    let Some(base) = o
        .base_surface()
        .extended_by_distance(k, dom, side, how, distance)?
    else {
        return Ok(None);
    };
    let surface = Surface::from(OffsetSurface::new(
        k,
        o.offset_distance(),
        base.surface,
        true,
        SS_UNKNOWN,
    )?);
    Ok(Some(Extended { surface, ..base }))
}

/// Natural on an analytic: the same surface widened, its face box on its own parameters and
/// the across length measured there.
fn widened(
    k: &Kernel,
    s: &Surface,
    dom: &Extent2d,
    side: BoxSide,
    distance: f64,
) -> Result<Extended> {
    let grown = s.continued(k, dom, side, distance)?;
    let map = |uv: Point2| -> Result<Point2> {
        grown.convert_uv_from_step_to_nurbs(k, s.convert_uv_from_nurbs_to_step(k, uv)?)
    };
    let (a, b) = (map(dom.min())?, map(dom.max())?);
    let on = Extent2d::from_bounds(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
    // the side each side of the face maps to: the one of the same direction nearer its middle
    let mut sides = BoxSide::ALL;
    for (to, from) in sides.iter_mut().zip(BoxSide::ALL) {
        let mid = map(from.point(dom, 0.5))?;
        let at = if from.is_u() { mid.x } else { mid.y };
        let (lo, hi) = if from.is_u() {
            (BoxSide::UMin, BoxSide::UMax)
        } else {
            (BoxSide::VMin, BoxSide::VMax)
        };
        *to = if (at - hi.at(&on)).abs() <= (at - lo.at(&on)).abs() {
            hi
        } else {
            lo
        };
    }
    let to = BoxSide::ALL
        .iter()
        .position(|&x| x == side)
        .and_then(|i| sides.get(i))
        .copied()
        .ok_or_else(op_err)?;
    let across = grown.length_across_side(k, &on, to, REL)?;
    // the widened's length across is the distance, by its closed form: checked as any
    // extension's is
    if !matches!(says(Some(&across), 0.0, distance), Says::Met) {
        return Err(not_exact(None));
    }
    Ok(Extended {
        surface: grown,
        source: 0.0,
        extension: 0.0,
        across,
        dom: on,
        sides,
    })
}

/// Whether the across length `b` of an extension whose construction bound is `extension` is
/// measured to the stop at the distance `d`: every leaf converged, its truncation part at most
/// `1e-9 d`, and its rounding part and the extension's rounding bound each at most `1e-6 d`
/// (owner).
fn exact(b: &Across, extension: f64, d: f64) -> bool {
    b.converged
        && b.truncation <= STOP * d
        && b.rounding <= ROUNDING * d
        && extension <= ROUNDING * d
}

/// What an ask's across length proves of the distance `d`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Says {
    /// Measured to the stop, its enclosure within `max(1e-9 d, its parts)` of `d`.
    Met,
    /// Proved short of `d` (its enclosure below it), by its middle less `d`.
    Short(f64),
    /// Proved past `d` (its enclosure above it), by its middle less `d`.
    Past(f64),
    /// Its enclosure holds `d` but it is not measured to the stop, or it is no measurement
    /// (no length, a leaf not converged, an extension bound not finite).
    Unproved,
}

/// How near `d` an across length `b` counts as at `d`: the stop, or the enclosure's parts and
/// the stored net's own resolution of the length (no net of floats is nearer to a length than
/// that, [`Across::resolution`]) where wider. Stopping tighter than one ulp of the net can move
/// the length is not resolvable; the certificate (the enclosure, `exact`, the 1e-6 d rounding
/// criterion) is unchanged.
fn stop_allowance(b: &Across, d: f64) -> f64 {
    (STOP * d).max(b.truncation + b.rounding + b.resolution)
}

/// What the across length `b` (none: no measurement) of an extension whose bound is
/// `extension` proves of the distance `d`.
fn says(b: Option<&Across>, extension: f64, d: f64) -> Says {
    let Some(b) = b else {
        return Says::Unproved;
    };
    if !b.converged || !extension.is_finite() {
        return Says::Unproved;
    }
    let (lower, upper) = (b.enclosure.lower, b.enclosure.upper);
    let tol = stop_allowance(b, d);
    if exact(b, extension, d) && lower >= d - tol && upper <= d + tol {
        Says::Met
    } else if upper < d {
        Says::Short(b.enclosure.mid() - d)
    } else if lower > d {
        Says::Past(b.enclosure.mid() - d)
    } else {
        Says::Unproved
    }
}

/// The distance an ask works for, when its length is measured to the stop at its own lower
/// bound: asking for that distance is met by this very extension.
fn workable(b: Option<&Across>, extension: f64) -> Option<f64> {
    let b = b?;
    let l = b.enclosure.lower;
    (l > 0.0 && matches!(says(Some(b), extension, l), Says::Met)).then_some(l)
}

/// The one rule every exact path applies at a side's least step (owner, 2026-11-05): the side
/// extended by [`Local::least_step`], the shortest extension its knot can carry (the kernel's
/// parameter resolution, about `1.2e-8` of the parameter), measures `at_least` across with
/// construction bound `extension`. A distance `d` that extension is proved past is below the
/// resolution: no extension can be built for it, and the honest answer is none, the side left
/// unchanged ([`SideExtension::Unchanged`]). A distance it does not prove past is extended
/// as asked (at that step when it meets it).
///
/// [`SideExtension::Unchanged`]: sides::SideExtension::Unchanged
pub(super) fn below_resolution(at_least: Option<&Across>, extension: f64, d: f64) -> bool {
    matches!(says(at_least, extension, d), Says::Past(_))
}

/// Whether the side's least step ([`Local::least_step`]), made and measured, is proved
/// past `d` ([`below_resolution`]).
fn below_least_step(
    made: &dyn Fn(f64) -> Result<Option<Made>>,
    across: &dyn Fn(&Option<Surface>) -> Result<Option<Across>>,
    source: &Local,
    d: f64,
) -> Result<bool> {
    Ok(match made(source.least_step()?)? {
        Some(s) => below_resolution(across(&s.0)?.as_ref(), s.1, d),
        None => false,
    })
}

fn not_exact(distance: Option<f64>) -> SolidError {
    SolidError::ExtendRefused {
        operation: OP,
        refusal: ExtendRefusal::NotExact,
        distance,
    }
}

/// The extension parameter solved for the distance `d`: bracketed by scaling the ask by `d`
/// over what it gave, then Illinois false position on the enclosures' middles. An ask whose
/// weights are not positive bounds the bracket; when even the longest positive one runs short,
/// the extension is refused naming a distance that works. An ask whose length holds `d` but
/// is not measured to the stop ([`Says::Unproved`]) bounds the bracket too, when a shorter
/// ask measured to the stop reaches `d`; otherwise the extension is refused as not exact,
/// naming the longest such ask's distance.
///
/// No ask is below the shortest extension the side's knot can carry
/// ([`Local::least_step`], for every continuity's new end knot): every halving and
/// scaling stops there. A `d` that step is proved past is below the side's resolution
/// ([`below_resolution`]): `None`, no extension. At that step a length not proved short of
/// `d` otherwise meets it, with its own measured enclosure and bound.
fn solve(
    made: &dyn Fn(f64) -> Result<Option<Made>>,
    across: &dyn Fn(&Option<Surface>) -> Result<Option<Across>>,
    source: &Local,
    d: f64,
) -> Result<Option<(Made, Across)>> {
    if below_least_step(made, across, source, d)? {
        return Ok(None);
    }
    let least = source.least_step()?;
    // the first ask: the parameter range times d over the source's length across it
    let range = source.range()?;
    let whole = made(range)?;
    let mut ask = match &whole {
        Some(s) => {
            let l = across(&s.0)?.map_or(0.0, |b| b.enclosure.mid());
            if l > 0.0 && l.is_finite() {
                (range * d / l).max(least)
            } else {
                range
            }
        }
        None => range,
    };
    // the longest ask below `e` found measured to the stop at its own length, by halving
    // (`made` and the measure at each), stopping early at one that reaches d: its ask, its
    // length's lower bound and its middle
    let longest_workable = |e: f64| -> Result<Option<(f64, f64, f64)>> {
        let (mut lo, mut hi) = (0.0_f64, e);
        let mut found: Option<(f64, f64, f64)> = None;
        for _ in 0..BISECTIONS {
            let m = (0.5 * (lo + hi)).max(least);
            if !(m > lo && m < hi) {
                break;
            }
            let at = match made(m)? {
                Some(s) => {
                    let b = across(&s.0)?;
                    workable(b.as_ref(), s.1).map(|l| (l, b.map_or(0.0, |b| b.enclosure.mid())))
                }
                None => None,
            };
            if let Some((l, mid)) = at {
                lo = m;
                found = Some((m, l, mid));
                if l >= d {
                    break;
                }
            } else {
                hi = m;
            }
        }
        Ok(found)
    };
    // the longest ask known to give positive weights, and the shortest known not to
    let (good, bad) = (Cell::new(0.0_f64), Cell::new(f64::INFINITY));
    // an ask, or the longest positive one below it when its weights are not: the ask made,
    // its surface and its across length
    let try_ask = |e: f64| -> Result<Option<Asked>> {
        let e = if e >= bad.get() {
            0.5 * (good.get() + bad.get())
        } else {
            e
        }
        .max(least);
        if let Some(s) = made(e)? {
            good.set(good.get().max(e));
            let b = across(&s.0)?;
            return Ok(Some((e, s, b)));
        }
        bad.set(e);
        // the longest positive ask, by halving between it and the last good one
        let (mut lo, mut hi) = (good.get(), e);
        for _ in 0..BISECTIONS {
            let m = (0.5 * (lo + hi)).max(least);
            if !(m > lo && m < hi) {
                break;
            }
            if made(m)?.is_some() {
                lo = m;
            } else {
                hi = m;
            }
        }
        good.set(lo);
        bad.set(hi);
        let longest = if lo > 0.0 { made(lo)? } else { None };
        let Some(s) = longest else {
            return Err(SolidError::ExtendRefused {
                operation: OP,
                refusal: ExtendRefusal::WeightVanishes,
                distance: None,
            });
        };
        let b = across(&s.0)?;
        if matches!(says(b.as_ref(), s.1, d), Says::Met | Says::Past(_)) {
            return Ok(Some((lo, s, b)));
        }
        // not proved to reach d at the longest positive ask: near the failing weight the net
        // runs far from the curve and its length may not be measurable there. The asks
        // backing off from it are measured: one measured to the stop that reaches d bounds
        // the solve from above (so asking for a distance this names succeeds); else the
        // refusal names the longest workable one. Weight positivity need not be monotone in
        // the ask, so this is a long workable distance, not proved the longest.
        let mut longest = workable(b.as_ref(), s.1);
        let mut reaching: Option<Asked> = None;
        let mut back = 0.5;
        for _ in 0..BACK_OFFS {
            let ask = (lo * (1.0 - back)).max(least);
            if let Some(s) = made(ask)? {
                let b = across(&s.0)?;
                if let Some(l) = workable(b.as_ref(), s.1) {
                    longest = Some(longest.map_or(l, |x| x.max(l)));
                    let tighter = reaching
                        .as_ref()
                        .and_then(|r| r.2.as_ref())
                        .is_none_or(|r| l < r.enclosure.lower);
                    // reaching d: past it, or meeting it to the stop (a back-off from a
                    // longest positive ask a few ulps off the one a distance was named
                    // from measures that distance to the stop, not always past it)
                    let meets = matches!(says(b.as_ref(), s.1, d), Says::Met);
                    if (l >= d || meets) && tighter {
                        reaching = Some((ask, s, b));
                    }
                }
            }
            back *= 0.5;
        }
        if let Some(r) = reaching {
            return Ok(Some(r));
        }
        // none of the back-offs measured: the longest workable ask below the longest positive
        if longest.is_none() {
            if let Some((m, l, _)) = longest_workable(lo)? {
                if l >= d {
                    if let Some(s) = made(m)? {
                        let b = across(&s.0)?;
                        return Ok(Some((m, s, b)));
                    }
                }
                longest = Some(l);
            }
        }
        Err(SolidError::ExtendRefused {
            operation: OP,
            refusal: ExtendRefusal::WeightVanishes,
            distance: longest,
        })
    };
    // an ask whose length is not measured to the stop: the longest ask below it that is, by
    // halving (`made` and the measure at each). Reaching d, it bounds the bracket from above;
    // short of d, the extension is refused naming its distance.
    let unproved = |e: f64| -> Result<(f64, f64)> {
        match longest_workable(e)? {
            Some((m, l, mid)) if l >= d => Ok((m, mid - d)),
            other => Err(not_exact(other.map(|(_, l, _)| l))),
        }
    };
    // (ask, f) running short of d and past it
    let (mut short, mut past): (Try, Try) = (None, None);
    // the longest short ask's distance measured to the stop: what a length that saturates
    // short of d reaches
    let mut reach: Option<f64> = None;
    let mut overshoot = 1e-6;
    for _ in 0..BRACKET_TRIES {
        let Some((made_ask, s, b)) = try_ask(ask)? else {
            break;
        };
        ask = made_ask;
        let f = match says(b.as_ref(), s.1, d) {
            Says::Met => return finish(s, b),
            Says::Past(_) | Says::Unproved if ask <= least && reaches(b.as_ref(), s.1, d) => {
                return finish(s, b)
            }
            Says::Short(f) => {
                short = Some((ask, f));
                if let Some(l) = workable(b.as_ref(), s.1) {
                    reach = Some(reach.map_or(l, |r| r.max(l)));
                }
                f
            }
            Says::Past(f) => {
                past = Some((ask, f));
                f
            }
            Says::Unproved => {
                let (x, f) = unproved(ask)?;
                past = Some((x, f));
                ask = x;
                f
            }
        };
        if short.is_some() && past.is_some() {
            break;
        }
        // scaled by d over what it gave, overshooting by a margin that doubles each try so a
        // ratio creeping to 1 still crosses d
        let got = f + d;
        overshoot *= 2.0;
        let push = 1.0 + overshoot;
        ask = if got > 0.0 {
            if got < d {
                ask * d / got * push
            } else {
                ask * d / got / push
            }
        } else {
            2.0 * ask
        };
        if !ask.is_finite() {
            break;
        }
    }
    let (Some((mut lo, mut flo)), Some((mut hi, mut fhi))) = (short, past) else {
        // every ask short of d, the length saturating (a ruled extension whose rows' end
        // weight ratios differ): refused, naming the longest reached
        if past.is_none() && reach.is_some() {
            return Err(not_exact(reach));
        }
        return Err(SolidError::NotConverged {
            operation: OP,
            iterations: u32::try_from(BRACKET_TRIES).unwrap_or(u32::MAX),
        });
    };
    let mut moved = Moved::Neither;
    for _ in 0..SOLVE_TRIES {
        let x = hi - fhi * (hi - lo) / (fhi - flo);
        let x = if x > lo.min(hi) && x < lo.max(hi) {
            x
        } else {
            0.5 * (lo + hi)
        };
        if !(x > lo.min(hi) && x < lo.max(hi)) {
            break;
        }
        let Some((x, s, b)) = try_ask(x)? else {
            break;
        };
        let (x, f) = match says(b.as_ref(), s.1, d) {
            Says::Met => return finish(s, b),
            Says::Short(f) | Says::Past(f) => (x, f),
            Says::Unproved => unproved(x)?,
        };
        if f < 0.0 {
            (lo, flo) = (x, f);
            if moved == Moved::Lo {
                fhi *= 0.5;
            }
            moved = Moved::Lo;
        } else {
            (hi, fhi) = (x, f);
            if moved == Moved::Hi {
                flo *= 0.5;
            }
            moved = Moved::Hi;
        }
    }
    Err(SolidError::NotConverged {
        operation: OP,
        iterations: u32::try_from(SOLVE_TRIES).unwrap_or(u32::MAX),
    })
}

/// Whether the across length `b` of an extension whose bound is `extension` is measured
/// (every leaf converged, the bound finite) and proved to reach `d`: its enclosure's lower
/// end at `d` within its [`stop_allowance`]. At the side's least step a length
/// that reaches `d` is the shortest the side carries; one whose enclosure merely holds `d`
/// may be short of it, and is not accepted there.
fn reaches(b: Option<&Across>, extension: f64, d: f64) -> bool {
    b.is_some_and(|b| {
        b.converged && extension.is_finite() && b.enclosure.lower >= d - stop_allowance(b, d)
    })
}

/// A met ask's extension and its length (met implies a length).
fn finish(s: Made, b: Option<Across>) -> Result<Option<(Made, Across)>> {
    let b = b.ok_or_else(op_err)?;
    Ok(Some((s, b)))
}

/// An ask made: the ask, its extension and its across length (none: not measured).
type Asked = (f64, Made, Option<Across>);

/// Which end of the Illinois bracket moved last.
#[derive(PartialEq)]
enum Moved {
    Neither,
    Lo,
    Hi,
}

/// An extension made: its surface (none when its net overflowed) and the bound on its part
/// past the side.
type Made = (Option<Surface>, f64);

/// An ask and its across length's middle less the distance.
type Try = Option<(f64, f64)>;

/// A homogeneous point `(w x, w y, w z, w)`.
type H = [f64; 4];

fn lin(a: f64, x: H, b: f64, y: H) -> H {
    [
        a * x[0] + b * y[0],
        a * x[1] + b * y[1],
        a * x[2] + b * y[2],
        a * x[3] + b * y[3],
    ]
}

fn sub(x: H, y: H) -> H {
    lin(1.0, x, -1.0, y)
}

fn dot4(x: H, y: H) -> f64 {
    x[0] * y[0] + x[1] * y[1] + x[2] * y[2] + x[3] * y[3]
}

/// A surface's net seen from the side it is extended past: rows of homogeneous points along
/// the extended direction, the side at the rows' high end (a low side reversed: the knots
/// negated, the rows read backwards, both exact).
#[derive(Debug, Clone)]
struct Local {
    rows: Vec<Vec<H>>,
    knots: Vec<f64>,
    p: usize,
    rational: bool,
    /// A bound on how far each extension point's homogeneous coordinates are from the exact
    /// construction's (0 for the source's own points, which are kept bit for bit).
    err: f64,
    /// A bound on how far each extension point's weight is from the exact construction's:
    /// 0 on a polynomial net (its weights are 1 in the construction and stored as none, so
    /// the surface's are exactly 1 whatever the rows' fourth coordinate rounded to); on a
    /// rational one [`Self::err`] where a construction does not bound the weights alone.
    err_w: f64,
    /// The degree across the rows (the other direction's): `q + 1` consecutive rows make a
    /// point of the surface.
    q: usize,
    /// The 3D bound of a construction made in a local frame ([`Self::natural`]), which
    /// [`Self::bound`] returns in place of the one from [`Self::err`].
    frame: Option<f64>,
    /// The knots across the rows (the other direction's), of degree [`Self::q`].
    across: Vec<f64>,
}

fn op_err() -> SolidError {
    SolidError::port_invariant(OP, "an index past the net")
}

impl Local {
    fn of(net: &Net, side: BoxSide) -> Result<Self> {
        let cpts = net.cpts();
        let rational = net.is_rational();
        let h = |c: &CPoint| -> H {
            if c.w == NOW {
                [c.x, c.y, c.z, 1.0]
            } else {
                [c.x, c.y, c.z, c.w]
            }
        };
        let (q, across) = if side.is_u() {
            (net.q(), net.v_knots())
        } else {
            (net.p(), net.u_knots())
        };
        let (rows, knots, p): (Vec<Vec<H>>, &[f64], usize) = if side.is_u() {
            let cols = cpts.first().map_or(0, Vec::len);
            let rows = (0..cols)
                .map(|j| cpts.iter().filter_map(|r| r.get(j)).map(h).collect())
                .collect();
            (rows, net.u_knots(), net.p())
        } else {
            let rows = cpts.iter().map(|r| r.iter().map(h).collect()).collect();
            (rows, net.v_knots(), net.q())
        };
        let mut l = Self {
            rows,
            knots: knots.to_vec(),
            p,
            rational,
            err: 0.0,
            err_w: 0.0,
            q,
            frame: None,
            across: across.to_vec(),
        };
        if !side.is_high() {
            l.reverse();
        }
        if l.p == 0 || l.rows.iter().any(|r| r.len() <= l.p) {
            return Err(SolidError::invalid_input(
                OP,
                "the net is too small to extend",
            ));
        }
        // clamped at the side: the end knot p + 1 times
        let b = l.end()?;
        let clamped = l.knots.iter().rev().take(l.p + 1).all(|&x| x == b);
        if !clamped {
            return Err(SolidError::invalid_input(
                OP,
                "the surface's form is not clamped at the side",
            ));
        }
        Ok(l)
    }

    fn reverse(&mut self) {
        for r in &mut self.rows {
            r.reverse();
        }
        self.knots = self.knots.iter().rev().map(|x| -x).collect();
    }

    fn end(&self) -> Result<f64> {
        self.knots.last().copied().ok_or_else(op_err)
    }

    fn start(&self) -> Result<f64> {
        self.knots.first().copied().ok_or_else(op_err)
    }

    fn range(&self) -> Result<f64> {
        Ok(self.end()? - self.start()?)
    }

    /// The shortest extension the side's end knot `b` can carry (its last span `[c, b]`).
    /// Every continuity's new end knot is `b + e` (the mirror's `2b - (b - e)` too), and it
    /// must survive the knot fixup every B-spline surface's knots go through
    /// ([`BSplineSurface::from_gw_nurb`], [`fixup_knot_vector`] at [`EFF_ZERO_PARAM`]): a
    /// knot nearer its neighbour than the ends' scaled zero is merged onto it (the new span
    /// gone, the end knot repeated past `p + 1`), and a span shorter than `EFF_ZERO_PARAM`
    /// is spread, moving the joint knot `b` (the source's net no longer kept). The span as
    /// the fixup computes it, `fl(b + e) - b`, is at least both. `e` is also at least
    /// `(b - c) EPSILON`, so Natural's de Casteljau parameter `1 + e / (b - c)` is past 1.
    /// A solver asks nothing shorter. A target this step is proved past is below the side's
    /// resolution: no extension ([`below_resolution`]).
    ///
    /// [`fixup_knot_vector`]: crate::nurbs::nurbs_crv::fixup_knot_vector
    pub(super) fn least_step(&self) -> Result<f64> {
        let a = self.start()?;
        let b = self.end()?;
        let c = self.before_end(0)?;
        if !(a.is_finite() && b.is_finite() && b > c) {
            return Err(op_err());
        }
        let mut e = EFF_ZERO_PARAM.max((b - c) * f64::EPSILON);
        for _ in 0..LEAST_STEP_TRIES {
            let end = b + e;
            let span = end - b;
            // the fixup's own tests, on the knots as they will be (the ends' scaled zero
            // is even in sign, so a low side's negated knots give the same)
            let need = EFF_ZERO_PARAM.max(scaled_zero2(&a, &end));
            if span >= need && 1.0 + e / (b - c) > 1.0 {
                return Ok(e);
            }
            // short by rounding (or by the end's own scaled zero): grow by the shortfall,
            // and at least by the rounding of `b + e`
            e += (need - span).max(end.abs() * f64::EPSILON);
        }
        Err(op_err())
    }

    /// The knot `back` places before the end's `p + 1` copies (`u_n` for 0).
    fn before_end(&self, back: usize) -> Result<f64> {
        self.knots
            .iter()
            .rev()
            .nth(self.p + 1 + back)
            .copied()
            .ok_or_else(op_err)
    }

    /// As a B-spline net.
    fn surface(mut self, side: BoxSide) -> Result<Net> {
        if !side.is_high() {
            self.reverse();
        }
        let c = |x: &H| {
            if self.rational {
                CPoint::new(x[0], x[1], x[2], x[3])
            } else {
                CPoint::new(x[0], x[1], x[2], NOW)
            }
        };
        let across = std::mem::take(&mut self.across);
        if side.is_u() {
            let len = self.rows.first().map_or(0, Vec::len);
            let net: Vec<Vec<CPoint>> = (0..len)
                .map(|i| self.rows.iter().filter_map(|r| r.get(i)).map(c).collect())
                .collect();
            Net::new(net, self.p, self.q, self.knots, across)
        } else {
            let net: Vec<Vec<CPoint>> = self
                .rows
                .iter()
                .map(|r| r.iter().map(c).collect())
                .collect();
            Net::new(net, self.q, self.p, across, self.knots)
        }
    }

    /// As a net extended in its own u, for knot insertion.
    fn as_net(&self) -> Result<Net> {
        let len = self.rows.first().map_or(0, Vec::len);
        let net: Vec<Vec<CPoint>> = (0..len)
            .map(|i| {
                self.rows
                    .iter()
                    .filter_map(|r| r.get(i))
                    .map(|x| CPoint::new(x[0], x[1], x[2], x[3]))
                    .collect()
            })
            .collect();
        let cols = self.rows.len();
        // a linear other direction over [0, 1] when the rows are two or more, else constant
        let (q, v) = if cols >= 2 {
            let mut v = vec![0.0; cols + 2];
            // clamped linear: 0, 0, uniform interior, 1, 1
            for (i, x) in v.iter_mut().enumerate() {
                *x = (i.saturating_sub(1)).min(cols - 1) as f64;
            }
            (1, v)
        } else {
            (0, vec![0.0, 1.0])
        };
        Net::new(net, self.p, q, self.knots.clone(), v)
    }

    /// The rows with `t` inserted until it has multiplicity `p`, from a copy (knot insertion:
    /// exact for the curve, to rounding for its points).
    fn with_knot(&self, t: f64) -> Result<Self> {
        let mult = self.knots.iter().filter(|&&x| x == t).count();
        if mult >= self.p {
            return Ok(self.clone());
        }
        let net = insert_knot(&self.as_net()?, t, self.p - mult, Direction::U)?;
        let cpts = net.cpts();
        let cols = self.rows.len();
        let rows = (0..cols)
            .map(|j| {
                cpts.iter()
                    .filter_map(|r| r.get(j))
                    .map(|c| [c.x, c.y, c.z, c.w])
                    .collect()
            })
            .collect();
        // each inserted point a convex combination of two, `p - mult` times over: the
        // points' rounding, gamma_(3 (p - mult)) of the largest coordinate, on top of theirs
        let err = self.err + gamma(3 * (self.p - mult)) * self.size();
        // the weights the same way, of the largest weight: a weight's combination is made of
        // weights alone
        let err_w = if self.rational {
            self.err_w + gamma(3 * (self.p - mult)) * self.weight_size()
        } else {
            0.0
        };
        Ok(Self {
            rows,
            knots: net.u_knots().to_vec(),
            p: self.p,
            rational: self.rational,
            err,
            err_w,
            q: self.q,
            frame: None,
            across: self.across.clone(),
        })
    }

    /// Whether every coordinate is finite.
    fn is_finite(&self) -> bool {
        self.rows.iter().flatten().flatten().all(|x| x.is_finite())
    }

    /// The largest weight (0 on a polynomial net).
    fn weight_size(&self) -> f64 {
        if !self.rational {
            return 0.0;
        }
        self.rows
            .iter()
            .flatten()
            .fold(0.0_f64, |m, x| m.max(x[3].abs()))
    }

    /// The largest homogeneous coordinate.
    fn size(&self) -> f64 {
        self.rows
            .iter()
            .flatten()
            .flatten()
            .fold(0.0_f64, |m, x| m.max(x.abs()))
    }

    /// A bound on the 3D distance of the surface of these rows from the exact construction's,
    /// from the homogeneous bounds [`Self::err`] and [`Self::err_w`]: `S = sum N H / sum N
    /// w`, so a change of at most `err` in each point coordinate and `err_w` in each weight
    /// moves it at most `(sqrt 3 err + |S| err_w) / w_min` (a polynomial net's weights are
    /// exactly 1: `sqrt 3 err`, whatever the part's position).
    fn bound(&self) -> f64 {
        if let Some(b) = self.frame {
            return b;
        }
        if self.err == 0.0 && self.err_w == 0.0 {
            return 0.0;
        }
        let (mut wmin, mut pmax) = (f64::INFINITY, 0.0_f64);
        for x in self.rows.iter().flatten() {
            let w = if self.rational { x[3] } else { 1.0 };
            wmin = wmin.min(w);
            pmax = pmax.max((x[0].abs() + x[1].abs() + x[2].abs()) / w.abs());
        }
        let err_w = if self.rational { self.err_w } else { 0.0 };
        // the rounding of this bound itself, a few ulps
        (2.0 * self.err + pmax * err_w) / wmin * (1.0 + gamma(8))
    }

    /// The rounding of the Ruled product form ([`Self::ruled_product`]), relative to the
    /// product's size: `2 gamma_(6 i) + gamma_(q + 6)`, from the `i` insertions of the Bezier
    /// cut, a convex combination each (`gamma_6` of the larger, on each factor), then the
    /// Bernstein product's sum of at most `q + 1` terms, its coefficient of exact integers
    /// rounding three times, each term twice more.
    fn product_rounding(&self) -> Result<f64> {
        let inserted: usize = interior_knots(&self.across, self.q)?
            .iter()
            .map(|&(_, mult)| self.q.saturating_sub(mult))
            .sum();
        Ok(2.0 * gamma(6 * inserted) + gamma(self.q + 6))
    }

    /// Whether every weight is positive.
    fn is_positive(&self) -> bool {
        !self.rational
            || self
                .rows
                .iter()
                .flatten()
                .all(|x| x[3] > 0.0 && x[3].is_finite())
    }

    /// Whether Ruled has a form past the rows' reach: their end weight ratios differ past
    /// rounding (equal ratios reach any distance) and the product form has a B-spline.
    fn has_product_form(&self) -> bool {
        if !self.rational || self.q == 0 || 2 * self.q > MAX_DEGREE {
            return false;
        }
        let ratios = || self.rows.iter().map(|r| end_ratio(r).unwrap_or(f64::NAN));
        let rho = ratios().fold(f64::NEG_INFINITY, f64::max);
        let least = ratios().fold(f64::INFINITY, f64::min);
        rho - least > gamma(4) * rho.abs() && interior_knots(&self.across, self.q).is_ok()
    }

    /// [`Extended::source`] of the Ruled product form, a bound on the 3D distance of the
    /// source written in that form from the source, as [`Self::bound`]: `err (sqrt 3 + |S|) /
    /// w_min`, with the form's weights at least the least weight times the least end weight
    /// (Bezier points are convex combinations of net points, and a product's Bernstein
    /// coefficients convex combinations of its factors' products).
    fn product_source_bound(&self) -> Result<f64> {
        let a = self.product_rounding()?;
        // each coordinate of a product is at most `s w`, `s` the largest of `H`, `w` of `W`
        let w = self
            .rows
            .iter()
            .filter_map(|r| r.last())
            .fold(0.0_f64, |m, x| m.max(x[3].abs()));
        let m = self.size() * w;
        let (mut wmin, mut pmax) = (f64::INFINITY, 0.0_f64);
        for x in self.rows.iter().flatten() {
            wmin = wmin.min(x[3]);
            pmax = pmax.max((x[0].abs() + x[1].abs() + x[2].abs()) / x[3].abs());
        }
        let wend = self
            .rows
            .iter()
            .filter_map(|r| r.last())
            .fold(f64::INFINITY, |m, x| m.min(x[3]));
        Ok(a * m * (2.0 + pmax) / (wmin * wend) * (1.0 + gamma(8)))
    }

    /// The extension by the parameter `e`, or `None` when a weight is not positive.
    fn extended(&self, how: Continuity, e: f64) -> Result<Option<Self>> {
        if !(e.is_finite() && e > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "the extension is not positive",
            ));
        }
        // a new end knot the knot fixup merges onto the side's or spreads (a span of no
        // length, or the joint moved), whatever the continuity
        if e < self.least_step()? {
            return Err(SolidError::invalid_input(
                OP,
                "the extension is shorter than the side's knot can carry",
            ));
        }
        let grown = match how {
            Continuity::Natural => self.natural(e)?,
            Continuity::Ruled => self.ruled(e)?,
            Continuity::Tangent | Continuity::Curvature => {
                let mut cur = self.clone();
                let mut left = e;
                let mut steps = 0;
                while left > 0.0 {
                    steps += 1;
                    if steps > MAX_STEPS {
                        return Err(SolidError::Unsupported {
                            operation: OP,
                            reason: "the extension needs more mirror steps than its budget",
                        });
                    }
                    // Curvature mirrors at most the last Bezier span a step, so its C2 point is
                    // that span's end and the extension is continuous in e; Tangent mirrors
                    // the whole piece (a prefix family in e either way)
                    let most = if how == Continuity::Curvature {
                        cur.end()? - cur.before_end(0)?
                    } else {
                        cur.range()?
                    };
                    // a remainder shorter than the side's knot can carry would be a sliver
                    // span the knot fixup merges or spreads (the joint moved, the net not
                    // kept): what is left is split in two instead, where both halves can be
                    // at least that long (a step shorter than it mirrors a span the net
                    // already has)
                    let least = cur.least_step()?;
                    let mut step = left.min(most);
                    let rest = left - step;
                    if rest > 0.0 && rest < least && left >= 2.0 * least {
                        step = 0.5 * left;
                    }
                    cur = cur.mirrored(step, how == Continuity::Curvature)?;
                    left -= step;
                }
                cur
            }
        };
        Ok(Some(grown).filter(Self::is_positive))
    }

    /// The source's rows and knots, the end knot down to multiplicity `p` as the joint, then
    /// the extension's points `ext[j]` (after the joint point) and knots `knots` (after the
    /// joint).
    fn joined(&self, ext: Vec<Vec<H>>, knots: &[f64]) -> Result<Self> {
        let keep = self.knots.len().checked_sub(1).ok_or_else(op_err)?;
        let mut all: Vec<f64> = self.knots.iter().take(keep).copied().collect();
        all.extend_from_slice(knots);
        let rows = self
            .rows
            .iter()
            .zip(ext)
            .map(|(r, x)| r.iter().copied().chain(x).collect())
            .collect();
        Ok(Self {
            rows,
            knots: all,
            p: self.p,
            rational: self.rational,
            err: self.err,
            err_w: self.err_w,
            q: self.q,
            frame: None,
            across: self.across.clone(),
        })
    }

    /// Natural: the last span's Bezier points (from a copy with its start knot at multiplicity
    /// `p`) continued past `b` by de Casteljau at `t = 1 + e / (b - c)`: the points
    /// `blossom(1^(p-i), t^i)` are the polynomial's Bezier points over `[b, b + e]`.
    ///
    /// In a local frame: each row is translated by its own origin `O_j` (its end point), `H'
    /// = H - w O_j` (the weights unchanged), continued, and its new points translated back,
    /// `H = H' + w O_j`; the source's rows are kept as they are. Translation commutes with the
    /// continuation (both are linear in the homogeneous points and keep the weights), so the
    /// construction is the same; only its rounding changes. The continuation amplifies its
    /// input's and its own rounding by `a^p` (`a = |1 - t| + |t|`), which is now relative to
    /// the rows' local size, not to their absolute coordinates; the translations round once
    /// each, the way back not amplified. The 3D bound ([`Self::bound`]) is then that of the
    /// local construction ([`Self::frame`]).
    fn natural(&self, e: f64) -> Result<Self> {
        let b = self.end()?;
        let c = self.before_end(0)?;
        let p = self.p;
        // each row's origin: its end point (any point would do: it is exact by choice)
        let origins: Vec<[f64; 3]> = self
            .rows
            .iter()
            .map(|r| {
                let x = r.last().ok_or_else(op_err)?;
                let w = if self.rational { x[3] } else { 1.0 };
                if !(w.is_finite() && w > 0.0) {
                    return Err(SolidError::invalid_input(
                        OP,
                        "a weight that is not positive",
                    ));
                }
                Ok([x[0] / w, x[1] / w, x[2] / w])
            })
            .collect::<Result<_>>()?;
        // the rows translated: one product and one difference a coordinate
        let mut local = self.clone();
        let mut moved = 0.0_f64;
        for (r, o) in local.rows.iter_mut().zip(&origins) {
            for x in r.iter_mut() {
                let w = x[3];
                for (v, &oc) in x.iter_mut().zip(o) {
                    moved = moved.max(v.abs() + (w * oc).abs());
                    *v -= w * oc;
                }
            }
        }
        local.err = self.err + gamma(2) * moved;
        let span = if c > local.start()? {
            local.with_knot(c)?
        } else {
            local.clone()
        };
        let t = 1.0 + e / (b - c);
        let ext_local: Vec<Vec<H>> = span
            .rows
            .iter()
            .map(|r| {
                let mut level: Vec<H> = r.iter().rev().take(p + 1).rev().copied().collect();
                let mut out = Vec::new();
                while level.len() > 1 {
                    level = level
                        .iter()
                        .zip(level.iter().skip(1))
                        .map(|(x, y)| lin(1.0 - t, *x, t, *y))
                        .collect();
                    if let Some(&last) = level.last() {
                        out.push(last);
                    }
                }
                out
            })
            .collect();
        // de Casteljau past the span: each level `(1 - t) x + t y` scales the errors and the
        // points by `a = |1 - t| + |t|` and rounds by gamma_3 of that, over p levels (the
        // rounding of t itself only reparametrises the continuation, its points stay on it)
        let a = (1.0 - t).abs() + t.abs();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let amp = a.powi(p as i32);
        let err_local = amp * (span.err + gamma(3 * p + 3) * span.size());
        // the weights' own: not translated, combined of weights alone (none on a polynomial
        // net, its weights stored as exactly 1)
        let err_local_w = if self.rational {
            amp * (span.err_w + gamma(3 * p + 3) * span.weight_size())
        } else {
            0.0
        };
        // the local frame's extent: its points' largest l1 size over their weight, the
        // smallest weight (the source's rows and the new ones), and the new rows translated
        // back, each coordinate one product and one sum
        let (mut wmin, mut pmax, mut back) = (f64::INFINITY, 0.0_f64, 0.0_f64);
        for x in local
            .rows
            .iter()
            .flatten()
            .chain(ext_local.iter().flatten())
        {
            let w = if self.rational { x[3] } else { 1.0 };
            wmin = wmin.min(w);
            pmax = pmax.max((x[0].abs() + x[1].abs() + x[2].abs()) / w.abs());
        }
        let ext: Vec<Vec<H>> = ext_local
            .into_iter()
            .zip(&origins)
            .map(|(r, o)| {
                r.into_iter()
                    .map(|mut x| {
                        let w = x[3];
                        for (v, &oc) in x.iter_mut().zip(o) {
                            back = back.max(v.abs() + (w * oc).abs());
                            *v += w * oc;
                        }
                        x
                    })
                    .collect()
            })
            .collect();
        // rows of different origins mix at a point: the q + 1 rows of a span across, so the
        // largest l1 distance between two origins within q + 1 consecutive rows
        let mut diam = 0.0_f64;
        for (i, oi) in origins.iter().enumerate() {
            for oj in origins.iter().skip(i + 1).take(self.q) {
                let dist = (oi[0] - oj[0]).abs() + (oi[1] - oj[1]).abs() + (oi[2] - oj[2]).abs();
                diam = diam.max(dist);
            }
        }
        let knots = vec![b + e; p + 1];
        let mut out = self.joined(ext, &knots)?;
        out.err = err_local;
        out.err_w = err_local_w;
        // `S = sum N (H' + w O_j) / sum N w`: an error of at most `err_local` in each local
        // point coordinate and `err_local_w` in each weight moves it at most `sqrt 3
        // err_local + |S - O_j| err_local_w`, with `|S - O_j| <= pmax + diam` for every row j
        // at the point; the way back's rounding `gamma_2 back` in each coordinate moves it at
        // most `sqrt 3` of that over the weight
        out.frame = Some(
            ((2.0 * err_local + (pmax + diam) * err_local_w) * (1.0 + gamma(4))
                + 2.0 * gamma(2) * back)
                / wmin
                * (1.0 + gamma(8)),
        );
        Ok(out)
    }

    /// Ruled: each row continued by the homogeneous line `H_n + s D`, `D = (p / h) (rho
    /// H_n - H_(n-1))` with one `rho`, the rows' largest `w_(n-1) / w_n` (module docs),
    /// written by degree elevation over `[b, b + e]`: the points `H_n + (i e / h) (rho H_n -
    /// H_(n-1))`, all four coordinates. Its bound: the points' rounding.
    fn ruled(&self, e: f64) -> Result<Self> {
        let b = self.end()?;
        let h = b - self.before_end(0)?;
        let p = self.p;
        #[allow(clippy::cast_precision_loss)]
        let pf = p as f64;
        let c = e / h;
        let mut ends = Vec::new();
        for row in &self.rows {
            let mut back = row.iter().rev();
            let (Some(&pn), Some(&pn1)) = (back.next(), back.next()) else {
                return Err(op_err());
            };
            ends.push((pn, pn1));
        }
        // one rho for every row (any one keeps the surface C1); the largest keeps every
        // weight at least its row's end weight: w_n (1 + (i e / h) (rho - rho_j))
        let rho = if self.rational {
            ends.iter()
                .fold(f64::NEG_INFINITY, |m, (pn, pn1)| m.max(pn1[3] / pn[3]))
        } else {
            1.0
        };
        let (mut err, mut err_w, mut ext) = (0.0_f64, 0.0_f64, Vec::new());
        for (pn, pn1) in ends {
            let leg = lin(rho, pn, -1.0, pn1);
            let q: Vec<H> = (1..=p)
                .map(|i| {
                    #[allow(clippy::cast_precision_loss)]
                    let t = c * i as f64;
                    lin(1.0, pn, t, leg)
                })
                .collect();
            // rounding: the leg (a product and a difference), t (two products) and the point
            // (a product and a sum), gamma_6 of the terms' magnitudes
            let m = |x: H| x.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
            err = err.max(gamma(6) * (m(pn) + c * pf * (rho.abs() * m(pn) + m(pn1))));
            // the weight coordinate the same way, of the weights alone
            err_w = err_w
                .max(gamma(6) * (pn[3].abs() + c * pf * (rho.abs() * pn[3].abs() + pn1[3].abs())));
            ext.push(q);
        }
        let knots = vec![b + e; p + 1];
        let mut out = self.joined(ext, &knots)?;
        out.err = self.err.max(err);
        out.err_w = if self.rational {
            self.err_w.max(err_w)
        } else {
            0.0
        };
        Ok(out)
    }

    /// Ruled past the reach of rows whose end weight ratios differ (#2878): the ruled surface
    /// `S + s S_u` past the joint, in the homogeneous form `(H W + s (H_u W - H W_u), W^2)`
    /// (module docs), and the source written in that form, its homogeneous points times `W`.
    /// The curves across are cut into Bezier pieces and multiplied piece by piece, so the
    /// result is of degree `2 q` across. The weights past the joint are the joint's, bit for
    /// bit. The caller has checked [`Self::has_product_form`].
    ///
    /// Its bound: with `a` of [`Self::product_rounding`] and `m = s w` (`s` the largest
    /// homogeneous coordinate, `w` the largest weight of the last two columns), the joint's
    /// `H W` is within `a m`, and `|H_u|`, `|W_u|` are at most `2 p / h` of `s`, `w`, so `X2 =
    /// H_u W - H W_u` is at most `4 p m / h` within `a + gamma_4` of that (`H_u`'s and `X2`'s
    /// differences); the point `X1 + (i e / p) X2` adds `gamma_4` (`t`, its product and sum).
    fn ruled_product(&self, e: f64) -> Result<Self> {
        let b = self.end()?;
        let h = b - self.before_end(0)?;
        let (p, q) = (self.p, self.q);
        #[allow(clippy::cast_precision_loss)]
        let pf = p as f64;
        let n = self.rows.first().map_or(0, Vec::len);
        if n < 2 || q == 0 {
            return Err(op_err());
        }
        let joints = interior_knots(&self.across, q)?;
        let column = |i: usize| -> Result<Vec<H>> {
            self.rows
                .iter()
                .map(|r| r.get(i).copied().ok_or_else(op_err))
                .collect()
        };
        let end = column(n - 1)?;
        let before = column(n - 2)?;
        let derivative: Vec<H> = end
            .iter()
            .zip(&before)
            .map(|(x, y)| lin(pf / h, *x, -pf / h, *y))
            .collect();
        let hu = bezier_pieces(&derivative, &self.across, q)?;
        let end = bezier_pieces(&end, &self.across, q)?;
        let w: Vec<f64> = end.iter().map(|x| x[3]).collect();
        let wu: Vec<f64> = hu.iter().map(|x| x[3]).collect();
        let pm = product_matrix(q, q)?;
        let written = (0..n)
            .map(|i| times(&bezier_pieces(&column(i)?, &self.across, q)?, &w, &pm, q))
            .collect::<Result<Vec<Vec<H>>>>()?;
        // past the joint: X1 + s X2, X1 = H W (the last column), X2 = H_u W - H W_u, whose
        // weight is exactly 0
        let x1 = written.last().ok_or_else(op_err)?;
        let x2 = times(&hu, &w, &pm, q)?
            .iter()
            .zip(&times(&end, &wu, &pm, q)?)
            .map(|(x, y)| {
                let d = sub(*x, *y);
                [d[0], d[1], d[2], 0.0]
            })
            .collect::<Vec<H>>();
        let ext: Vec<Vec<H>> = x1
            .iter()
            .zip(&x2)
            .map(|(x, y)| {
                (1..=p)
                    .map(|i| {
                        #[allow(clippy::cast_precision_loss)]
                        let t = e * i as f64 / pf;
                        lin(1.0, *x, t, *y)
                    })
                    .collect()
            })
            .collect();
        let rows = (0..x1.len())
            .map(|j| {
                written
                    .iter()
                    .map(|c| c.get(j).copied().ok_or_else(op_err))
                    .collect::<Result<Vec<H>>>()
            })
            .collect::<Result<Vec<Vec<H>>>>()?;
        let a = self.product_rounding()? + gamma(8);
        let wmax = self
            .rows
            .iter()
            .flat_map(|r| r.iter().rev().take(2))
            .fold(0.0_f64, |m, x| m.max(x[3].abs()));
        let m = self.size() * wmax;
        let form = Self {
            rows,
            knots: self.knots.clone(),
            p,
            rational: true,
            err: 0.0,
            err_w: 0.0,
            q: 2 * q,
            frame: None,
            across: product_knots(&self.across, &joints, q)?,
        };
        let mut out = form.joined(ext, &vec![b + e; p + 1])?;
        out.err = self.err * wmax + a * m * (1.0 + 4.0 * pf * e / h);
        // the bound holds every homogeneous coordinate, the weights' included
        out.err_w = out.err;
        Ok(out)
    }

    /// One mirror step by `e <= range`: the piece over `[b - e, b]` (from a copy with `b - e`
    /// at multiplicity `p`) mapped by each row's `L` and reversed; for Curvature its second
    /// row then from the C2 condition.
    fn mirrored(&self, e: f64, curvature: bool) -> Result<Self> {
        let b = self.end()?;
        let a = self.start()?;
        let p = self.p;
        let refl = b - e;
        let refl = if refl < a { a } else { refl };
        let piece_src = if refl > a {
            self.with_knot(refl)?
        } else {
            self.clone()
        };
        let inner: Vec<f64> = self
            .knots
            .iter()
            .copied()
            .filter(|&x| x > refl && x < b)
            .collect();
        let len = inner.len() + p + 1;
        // the extension's knots after the joint: the inner knots mirrored, then the new end
        let mut knots: Vec<f64> = inner.iter().rev().map(|&x| 2.0 * b - x).collect();
        knots.extend(std::iter::repeat_n(2.0 * b - refl, p + 1));
        let mut ext = Vec::new();
        let mut err = 0.0_f64;
        for (row, src) in self.rows.iter().zip(&piece_src.rows) {
            let (Some(&pn), Some(&pn1)) = (row.last(), row.iter().rev().nth(1)) else {
                return Err(op_err());
            };
            let piece: Vec<H> = src.iter().rev().take(len).copied().collect();
            let (map, amp, round) = mirror(pn, pn1);
            // the piece read from b backwards, without the joint point; the first after it
            // on the last leg's line, so its image is exactly `2 pn - x` (L fixes pn and
            // reverses the leg): the tangent across the joint carries no rounding of L
            let mut q = Vec::new();
            for (i, x) in piece.iter().skip(1).enumerate() {
                let m = x.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
                if i == 0 {
                    q.push(lin(2.0, pn, -1.0, *x));
                    let pm = pn.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
                    err = err.max(piece_src.err + gamma(2) * (2.0 * pm + m));
                } else {
                    q.push(map(*x));
                    err = err.max(amp * piece_src.err + round * m);
                }
            }
            ext.push(q);
        }
        if curvature && p >= 2 {
            // the C2 condition across the joint on the homogeneous rows
            let h1 = b - self.before_end(0)?;
            let h2 = b - self.before_end(1)?;
            let (Some(&v1), Some(&v2)) = (knots.first(), knots.get(1)) else {
                return Err(op_err());
            };
            let (g1, g2) = (v1 - b, v2 - b);
            for (row, q) in self.rows.iter().zip(ext.iter_mut()) {
                let mut back = row.iter().rev();
                let (Some(&pn), Some(&pn1), Some(&pn2)) = (back.next(), back.next(), back.next())
                else {
                    return Err(op_err());
                };
                let Some(&q1) = q.first() else {
                    return Err(op_err());
                };
                // Q2 = Q1 + g2 [(g1 / h1) ((Pn - Pn1) / h1 - (Pn1 - Pn2) / h2) + (Q1 - Pn) / g1]
                let second = lin(1.0 / h1, sub(pn, pn1), -1.0 / h2, sub(pn1, pn2));
                let step = lin(g1 / h1, second, 1.0 / g1, sub(q1, pn));
                if let Some(q2) = q.get_mut(1) {
                    *q2 = lin(1.0, q1, g2, step);
                }
                // its rounding: gamma_12 of its terms' magnitudes, and q1's and the rows'
                // errors carried by the formula's coefficients
                let m = [pn, pn1, pn2, q1]
                    .iter()
                    .flatten()
                    .fold(0.0_f64, |m, c| m.max(c.abs()));
                let gain = 1.0 + g2 * (2.0 * g1 / h1 * (1.0 / h1 + 1.0 / h2) + 2.0 / g1);
                err = err.max(gain * (err + self.err) + gamma(12) * gain * m);
            }
        }
        let mut out = self.joined(ext, &knots)?;
        out.err = self.err.max(err);
        // the mirror's map mixes the weight into the points: its weights bounded by `err`
        out.err_w = if self.rational { out.err } else { 0.0 };
        Ok(out)
    }
}

/// The map of the mirror at a row's end `pn` with the point `pn1` before it: the linear map of
/// `R^4` fixing the hyperplane `n . x = 0` (`n = (w_n nu, -H_n . nu)`, `nu = w_(n-1) H_n -
/// w_n H_(n-1)`, the last leg scaled by `w_n w_(n-1)`, so the hyperplane holds `pn` and lifts
/// the Euclidean plane through its point normal to the leg) and reversing `D = pn - pn1`:
/// `x - 2 D (n . x) / (n . D)`, with `n . D = nu . nu`. A leg no longer than its rounding
/// leaves no tangent: the point reflection `2 pn - x` there.
fn mirror(pn: H, pn1: H) -> (impl Fn(H) -> H, f64, f64) {
    let (wn, w1) = (pn[3], pn1[3]);
    let nu = [
        w1 * pn[0] - wn * pn1[0],
        w1 * pn[1] - wn * pn1[1],
        w1 * pn[2] - wn * pn1[2],
    ];
    let rounding = |i: usize| {
        let (a, b) = (
            pn.get(i).copied().unwrap_or(0.0),
            pn1.get(i).copied().unwrap_or(0.0),
        );
        gamma(3) * ((w1 * a).abs() + (wn * b).abs())
    };
    let tangent = nu.iter().enumerate().any(|(i, x)| x.abs() > rounding(i));
    let nd = nu[0] * nu[0] + nu[1] * nu[1] + nu[2] * nu[2];
    let n: H = [
        wn * nu[0],
        wn * nu[1],
        wn * nu[2],
        -(pn[0] * nu[0] + pn[1] * nu[1] + pn[2] * nu[2]),
    ];
    let d = sub(pn, pn1);
    let reflect = tangent && nd > 0.0;
    // how much L can grow an error (`1 + 2 |D| |n|_1 / nd` in the largest coordinate), and its
    // rounding per unit of the point's largest coordinate: the map's own (a 4-term dot, a
    // division, a product and a difference) and that of n and nd, gamma_12 of the same gain
    let (amp, round) = if reflect {
        let dm = d.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
        let n1: f64 = n.iter().map(|c| c.abs()).sum();
        let gain = 1.0 + 2.0 * dm * n1 / nd;
        (gain, gamma(12) * gain)
    } else {
        (1.0, gamma(2) * 3.0)
    };
    let map = move |x: H| {
        if reflect {
            lin(1.0, x, -2.0 * dot4(n, x) / nd, d)
        } else {
            lin(2.0, pn, -1.0, x)
        }
    };
    (map, amp, round)
}

/// The row's `w_(n-1) / w_n`.
fn end_ratio(row: &[H]) -> Option<f64> {
    let mut back = row.iter().rev();
    let (pn, pn1) = (back.next()?, back.next()?);
    Some(pn1[3] / pn[3])
}

/// (#2878) The distinct interior knots of `knots` with their multiplicities, for the curves
/// across cut into continuous Bezier pieces of degree `q`: refused unless clamped at both ends
/// with no interior knot past multiplicity `q`, as [`Local::has_product_form`] checks first.
fn interior_knots(knots: &[f64], q: usize) -> Result<Vec<(f64, usize)>> {
    let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
        return Err(op_err());
    };
    let copies = |t: f64| knots.iter().filter(|&&x| x == t).count();
    if copies(lo) != q + 1 || copies(hi) != q + 1 {
        return Err(SolidError::invalid_input(
            OP,
            "the surface's form is not clamped across the side",
        ));
    }
    let mut out: Vec<(f64, usize)> = Vec::new();
    for &t in knots.iter().filter(|&&x| x > lo && x < hi) {
        match out.last_mut() {
            Some((x, mult)) if *x == t => *mult += 1,
            _ => out.push((t, 1)),
        }
    }
    if out.iter().any(|&(_, mult)| mult > q) {
        return Err(SolidError::invalid_input(
            OP,
            "the surface's form is not continuous across the side",
        ));
    }
    Ok(out)
}

/// (#2878) The curve of degree `q` over the clamped `knots` cut into its Bezier pieces: their
/// points in order, `q + 1` a piece, consecutive pieces sharing their joint point.
fn bezier_pieces(curve: &[H], knots: &[f64], q: usize) -> Result<Vec<H>> {
    let pw = curve
        .iter()
        .map(|x| CPoint::new(x[0], x[1], x[2], x[3]))
        .collect();
    let mut out = Vec::new();
    for (i, piece) in Curve::new(pw, q, knots.to_vec())?
        .decompose_bez()?
        .iter()
        .enumerate()
    {
        let points = piece.cpts().iter().copied().map(CPoint::to_array);
        out.extend(points.skip(usize::from(i > 0)));
    }
    Ok(out)
}

/// (#2878) The product of the curve `a` and the scalar curve `w`, both in Bezier pieces of
/// degree `q` ([`bezier_pieces`]), in Bezier pieces of degree `2 q`; `pm` is the Bernstein
/// [`product_matrix`] of `q` and `q`.
fn times(a: &[H], w: &[f64], pm: &RealMatrix, q: usize) -> Result<Vec<H>> {
    let span = a.len().saturating_sub(1);
    let (Some(0), Some(pieces)) = (span.checked_rem(q), span.checked_div(q)) else {
        return Err(op_err());
    };
    if a.is_empty() || a.len() != w.len() {
        return Err(op_err());
    }
    let mut out = Vec::new();
    for piece in 0..pieces {
        let base = piece * q;
        // a joint is the previous piece's last point
        for k in usize::from(piece > 0)..=2 * q {
            let mut c = [0.0_f64; 4];
            for i in k.saturating_sub(q)..=k.min(q) {
                let (Some(&ai), Some(&wj)) = (a.get(base + i), w.get(base + k - i)) else {
                    return Err(op_err());
                };
                c = lin(1.0, c, pm.get(k, i)? * wj, ai);
            }
            out.push(c);
        }
    }
    Ok(out)
}

/// (#2878) The knots across of [`times`]' result: the ends `2 q + 1` times, each interior
/// knot `joints` gives `2 q` times, from the clamped `knots`.
fn product_knots(knots: &[f64], joints: &[(f64, usize)], q: usize) -> Result<Vec<f64>> {
    let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
        return Err(op_err());
    };
    let mut out = vec![lo; 2 * q + 1];
    for &(t, _) in joints {
        out.extend(std::iter::repeat_n(t, 2 * q));
    }
    out.extend(std::iter::repeat_n(hi, 2 * q + 1));
    Ok(out)
}

/// Whether `out` holds `net`'s points and knots over its box, bit for bit, on side `side`'s
/// direction, and `net`'s other knots.
fn keeps(net: &Net, out: &Net, side: BoxSide) -> Result<bool> {
    let (ka, kb, oa, ob) = if side.is_u() {
        (net.u_knots(), out.u_knots(), net.v_knots(), out.v_knots())
    } else {
        (net.v_knots(), out.v_knots(), net.u_knots(), out.u_knots())
    };
    if oa != ob {
        return Ok(false);
    }
    // the source's knots but its end's last copy, at the kept end of the result's
    let keep = ka.len().checked_sub(1).ok_or_else(op_err)?;
    let shift = kb.len().checked_sub(ka.len()).ok_or_else(op_err)?;
    let knots_kept = if side.is_high() {
        ka.iter().take(keep).eq(kb.iter().take(keep))
    } else {
        ka.iter().skip(1).eq(kb.iter().skip(shift + 1))
    };
    if !knots_kept {
        return Ok(false);
    }
    let (a, b) = (net.cpts(), out.cpts());
    let same = |x: &CPoint, y: &CPoint| x.x == y.x && x.y == y.y && x.z == y.z && x.w == y.w;
    let points_kept = if side.is_u() {
        let shift = b.len().checked_sub(a.len()).ok_or_else(op_err)?;
        let skip = if side.is_high() { 0 } else { shift };
        a.iter()
            .zip(b.iter().skip(skip))
            .all(|(ra, rb)| ra.len() == rb.len() && ra.iter().zip(rb).all(|(x, y)| same(x, y)))
    } else {
        a.iter().zip(&b).all(|(ra, rb)| {
            let shift = rb.len().saturating_sub(ra.len());
            let skip = if side.is_high() { 0 } else { shift };
            ra.iter().zip(rb.iter().skip(skip)).all(|(x, y)| same(x, y))
        })
    };
    Ok(points_kept && a.len() <= b.len())
}

mod analytic;
mod ball;
mod blend;
mod sides;
pub use sides::{ExtendedSides, SideExtension};

#[cfg(test)]
mod tests;
