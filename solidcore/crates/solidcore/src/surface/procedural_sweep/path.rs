//! The path motion of a [`ProceduralSweep`](super::ProceduralSweep) (#2572, slice 2, not
//! upstream): the profile carried along a path in the path's rotation-minimizing frame, and
//! the certified distance of the net `sweep_scale` fits to it.
//!
//! **The definition.** `S(w, t) = T(t) + x(t) C_x(w) + y(t) C_y(w) + z(t) C_z(w)`, `T` the
//! path, `C` the profile in the frame's own coordinates and `t` the path's parameter. The
//! frame is a property of the path alone, its rotation-minimizing (parallel transport)
//! frame: `x = T' / |T'|`, `z` the unit normal that turns with `x` and no further, `y = z x
//! x`, started from the unit normal the sweep starts from. It does not depend on how the
//! sweep sampled the path. In closed form where the path is made of
//!
//! - lines: the frame is constant;
//! - circular arcs: the frame turns about the arc's axis `n` by the angle the tangent has
//!   turned from the span's start, `z = R_n(phi) z_0`, `phi = atan2((x_0 x x) . n, x_0 . x)`;
//!
//! each knot span of the path being one or the other, and the frame at a span's start the
//! previous span's frame at its end (the joints are G1). A span is classified by sampling
//! it at 12 parameters (a line to `1e-11` of the path's size for degree up to 3, a circle for
//! a conic); the frame itself is the exact closed form (a rigid rotation of the span's start frame), so the classification only decides which closed form a span has, and a span that is a line or an arc within that tolerance is taken as one. A path with any other span (a
//! general spline: the rotation-minimizing equation has no closed form there, and its
//! certified enclosure is not here yet), or a closed one whose transported frame does
//! not close, has no definition: the #2505 ledger's `path-spline` and `path-closed`.
//!
//! **The map.** The fitted net uses the trajectory parameters at every section, with
//! derivatives in that same parameter. Contiguous pieces join without a parameter shift,
//! so the map from the fitted net to the path is the identity at the traced stations.
//!
//! **The bound.** Every section of the net is the profile's control points moved by one
//! motion, in the same rational basis `R_i(u)` (non-negative, summing to 1) for every `v`,
//! so with `P_i(v)` the net's `i`-th control point as a curve of `v` and `S_i(v) = T + R C_i`
//! the same point of `S`,
//! `B(u, v) - S(u, v) = sum_i R_i(u) (P_i(v) - S_i(v))`: a convex combination, and
//! `|B - S| <= max_i max_v |P_i(v) - S_i(v)|`. Each `f_i = P_i - S_i` is bounded over a cell
//! of `v` about its middle `c`, half width `h`, by its Taylor expansion,
//! `|f_i| <= |f_i(c)| + |f_i'(c)| h + (M_P + M_S) h^2 / 2`, `M_P` the largest `|P_i''|` in the
//! cell (its second derivative's control points, a B-spline's convex hull) and `M_S` that of
//! `S_i''`: `(dt/dv)^2 (|T''| + |C_i| (phi'' + phi'^2))` on an arc, `|T''|` on a line, with
//! `phi' <= |T'| / r`, `phi'' <= |T''| / r` and `|T'|`, `|T''|` bounded over the path's span
//! from the control polygons of its derivatives (the path is `N / w`, quotient rule). Cells
//! never cross a knot of the net, of the path or a node, where `P_i''` or `S_i''` jump, and
//! are split until the second order term is small or the budget [`MAX_CELLS`] is spent (the
//! bound is then looser, never wrong).
//!
//! The floor adds a forward error bound of evaluating both in floating point, `gamma_K`
//! times the sizes of the numbers, and a slack of `2^-30` relative on the cell bounds (they
//! are computed in doubles).
//!
//! Deviations: none (no upstream counterpart). No recursion (an explicit stack of cells).

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector3d::Vec3;
use crate::nurbs::b_spline_curve::p1::KnotType;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::gamma;
use crate::nurbs::srf_adv::p6::SweepTrace;

use crate::blend::fillet_section::interval::round_up;

const OP_CREATE: &str = "surface::procedural_sweep::PathMotion::create";
const OP_EVAL: &str = "surface::procedural_sweep::PathMotion::frame";

/// The most cells the bound is split into.
pub const MAX_CELLS: usize = 400_000;

/// The remainder of a cell, relative to the part's size, below which the bound of a fitted net
/// is not refined further (the goal of #2723, kept for the fitted nets: they are far from
/// their definition, so a finer enclosure would only cost cells).
const FITTED_GOAL: f64 = 1e-7;

/// The same for a net built from its definition, which is that definition to rounding, so the
/// enclosure is all that is left of the bound: the bound is about the goal, `1e-8 (1 + big)`,
/// which is at most a tenth of the kernel's tolerance (`ZONE_TOL_3D = 1e-5`) for parts up to
/// about a hundred mm across; a larger part gets a looser bound and, above the tolerance,
/// keeps the fit ([`crate::primitive::primitive_creation`]'s `curve_sweep`).
const EXACT_GOAL: f64 = 1e-8;

/// The deepest a cell is halved.
const MAX_DEPTH: u32 = 40;

/// Slack on the doubles' cell bounds, relative.
const SLACK: f64 = 1.0 / 1_073_741_824.0;

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP_CREATE, reason)
}

/// A B-spline curve of 3D points, unrounded evaluation and its derivatives: the net's
/// control columns and the path's derivatives.
#[derive(Debug, Clone)]
struct Spline {
    degree: usize,
    knots: Vec<f64>,
    ctrl: Vec<Vec3>,
}

impl Spline {
    /// The spline through `ctrl` over the full `knots` (`ctrl.len() + degree + 1` of them,
    /// ascending).
    fn new(degree: usize, knots: Vec<f64>, ctrl: Vec<Vec3>) -> Result<Self> {
        let ok = !ctrl.is_empty()
            && knots.len() == ctrl.len() + degree + 1
            && knots.windows(2).all(|w| matches!(w, [a, b] if a <= b))
            && knots.iter().all(|k| k.is_finite())
            && ctrl
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
        if ok {
            Ok(Self {
                degree,
                knots,
                ctrl,
            })
        } else {
            Err(bad(
                "a spline's knots and control points are finite and fit",
            ))
        }
    }

    /// The derivative, a spline of one degree less (of degree 0 and zero for a constant).
    fn derivative(&self) -> Self {
        let p = self.degree;
        if p == 0 {
            return Self {
                degree: 0,
                knots: self.knots.clone(),
                ctrl: vec![Vec3::ZERO; self.ctrl.len()],
            };
        }
        let factor = f64::from(u32::try_from(p).unwrap_or(u32::MAX));
        let ctrl = self
            .ctrl
            .windows(2)
            .enumerate()
            .map(|(i, w)| match w {
                [a, b] => {
                    let lo = self.knots.get(i + 1).copied().unwrap_or(0.0);
                    let hi = self.knots.get(i + p + 1).copied().unwrap_or(0.0);
                    if hi > lo {
                        (*b - *a) * (factor / (hi - lo))
                    } else {
                        Vec3::ZERO
                    }
                }
                _ => Vec3::ZERO,
            })
            .collect();
        let knots = self
            .knots
            .get(1..self.knots.len().saturating_sub(1))
            .map(<[f64]>::to_vec)
            .unwrap_or_default();
        Self {
            degree: p - 1,
            knots,
            ctrl,
        }
    }

    /// The knot span `m`, `degree <= m < ctrl.len()`, holding `t` (clamped to the domain).
    fn span(&self, t: f64) -> usize {
        let p = self.degree;
        let n = self.ctrl.len();
        let m = self
            .knots
            .partition_point(|&k| k <= t)
            .saturating_sub(1)
            .max(p);
        m.min(n.saturating_sub(1))
    }

    /// The point at `t` (de Boor).
    fn eval(&self, t: f64) -> Vec3 {
        let p = self.degree;
        let m = self.span(t);
        let mut d: Vec<Vec3> = (0..=p)
            .map(|j| {
                (m + j)
                    .checked_sub(p)
                    .and_then(|i| self.ctrl.get(i))
                    .copied()
                    .unwrap_or(Vec3::ZERO)
            })
            .collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + m - p;
                let lo = self.knots.get(i).copied().unwrap_or(0.0);
                let hi = self.knots.get(i + p + 1 - r).copied().unwrap_or(lo);
                let alpha = if hi > lo { (t - lo) / (hi - lo) } else { 0.0 };
                let prev = d.get(j - 1).copied().unwrap_or(Vec3::ZERO);
                let cur = d.get(j).copied().unwrap_or(Vec3::ZERO);
                if let Some(slot) = d.get_mut(j) {
                    *slot = prev * (1.0 - alpha) + cur * alpha;
                }
            }
        }
        d.get(p).copied().unwrap_or(Vec3::ZERO)
    }

    /// The control points that carry the span holding `t`.
    fn local(&self, t: f64) -> &[Vec3] {
        let m = self.span(t);
        self.ctrl
            .get(m.saturating_sub(self.degree)..=m)
            .unwrap_or(&[])
    }

    /// An upper bound of `|S(x)|` over the span holding `t` (convex hull).
    fn largest(&self, t: f64) -> f64 {
        self.local(t).iter().fold(0.0_f64, |m, p| m.max(p.length()))
    }

    /// The domain.
    fn domain(&self) -> (f64, f64) {
        let p = self.degree;
        let lo = self.knots.get(p).copied().unwrap_or(0.0);
        let hi = self.knots.get(self.ctrl.len()).copied().unwrap_or(lo);
        (lo, hi)
    }
}

/// The full knot vector of unique knots and multiplicities.
fn full_knots(unique: &[f64], mult: &[usize]) -> Vec<f64> {
    unique
        .iter()
        .zip(mult)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m))
        .collect()
}

/// A frame of the definition at a parameter, with its first derivatives in `t`.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// The path's point.
    pub point: Vec3,
    /// The path's first derivative.
    pub tangent: Vec3,
    /// The frame's axes `x`, `y`, `z`.
    pub axes: [Vec3; 3],
    /// Their derivatives in `t`.
    pub rates: [Vec3; 3],
}

impl Frame {
    /// The frame's image of the profile's `c`: `T + x c_x + y c_y + z c_z`.
    pub fn place(&self, c: Vec3) -> Vec3 {
        let [x, y, z] = self.axes;
        self.point + x * c.x + y * c.y + z * c.z
    }

    /// The rate of that image in `t`.
    pub fn rate(&self, c: Vec3) -> Vec3 {
        let [x, y, z] = self.rates;
        self.tangent + x * c.x + y * c.y + z * c.z
    }

    /// The image's rotation of a profile vector `c`.
    pub fn turn(&self, c: Vec3) -> Vec3 {
        let [x, y, z] = self.axes;
        x * c.x + y * c.y + z * c.z
    }

    /// The rate of the rotation of `c`.
    pub fn turn_rate(&self, c: Vec3) -> Vec3 {
        let [x, y, z] = self.rates;
        x * c.x + y * c.y + z * c.z
    }
}

/// What one knot span of the path is: the rotation-minimizing frame has a closed form on
/// each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpanKind {
    /// A straight line: the frame is constant.
    Line,
    /// A circular arc of `radius` in the plane square to the unit `axis`: the frame turns
    /// about the axis by the angle the tangent has turned.
    Arc {
        /// The unit normal of the arc's plane.
        axis: Vec3,
        /// The arc's radius.
        radius: f64,
    },
}

/// One knot span of the path, with the frame it starts in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// The span's parameters.
    pub from: f64,
    /// The span's end.
    pub to: f64,
    /// What the span is.
    pub kind: SpanKind,
    /// The unit tangent at its start.
    pub x0: Vec3,
    /// The frame's unit normal at its start, carried from the previous span's end.
    pub z0: Vec3,
}

/// A path and the rotation-minimizing frame it carries the profile in (module docs).
#[derive(Debug, Clone)]
pub struct PathMotion {
    path: BSplineCurve,
    /// The path's knot spans, ascending, each a line or an arc.
    spans: Vec<Span>,
    /// The net's parameter across the sweep and the path's, at each section, both strictly
    /// ascending.
    nodes: Vec<(f64, f64)>,
}

impl PathMotion {
    /// The path.
    #[must_use]
    pub const fn path(&self) -> &BSplineCurve {
        &self.path
    }

    /// The path's knot spans: what each is and the frame it starts in.
    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// The net's parameter and the path's at each placed section, both ascending: the path's
    /// `t` at a net parameter in between is interpolated linearly.
    #[must_use]
    pub fn nodes(&self) -> &[(f64, f64)] {
        &self.nodes
    }

    /// The path's `t` at the net's parameter `v` (clamped to the nodes) and `dt / dv`, at a
    /// node the segment that starts there.
    #[must_use]
    pub fn path_parameter(&self, v: f64) -> (f64, f64) {
        self.path_parameter_side(v, false)
    }

    /// [`PathMotion::path_parameter`], at a node the segment that ends there when `left` (a face
    /// ending at a joint is read with its own side).
    #[must_use]
    pub fn path_parameter_side(&self, v: f64, left: bool) -> (f64, f64) {
        let (Some(&(v0, t0)), Some(&(v1, t1))) = (self.nodes.first(), self.nodes.last()) else {
            return (v, 1.0);
        };
        // beyond the ends the parameter is held, with the slope of the end segment
        let last = self.nodes.len().saturating_sub(2);
        let i = if v > v0 {
            self.nodes
                .partition_point(|&(x, _)| if left { x < v } else { x <= v })
                .saturating_sub(1)
                .min(last)
        } else {
            0
        };
        let held = if !(v > v0) {
            Some(t0)
        } else if !(v < v1) {
            Some(t1)
        } else {
            None
        };
        match (self.nodes.get(i), self.nodes.get(i + 1)) {
            (Some(&(va, ta)), Some(&(vb, tb))) if vb > va => {
                let m = (tb - ta) / (vb - va);
                (held.unwrap_or(ta + m * (v - va)), m)
            }
            _ => (v, 1.0),
        }
    }

    /// The span holding the path's parameter `t`.
    fn span_at(&self, t: f64) -> Option<&Span> {
        self.spans.get(
            self.spans
                .partition_point(|s| s.from <= t)
                .saturating_sub(1),
        )
    }

    /// The frame at the path's parameter `t` (clamped to the path's domain): the
    /// rotation-minimizing frame of the path, which depends on the path alone.
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the path's tangent vanishes; those of evaluating the path.
    pub fn frame(&self, t: f64) -> Result<Frame> {
        self.frame_side(t, false)
    }

    /// [`PathMotion::frame`], at a joint the span that ends there when `left` (a face ending
    /// at the joint is read with its own side).
    ///
    /// # Errors
    ///
    /// Those of [`PathMotion::frame`].
    pub fn frame_side(&self, t: f64, left: bool) -> Result<Frame> {
        // `from_left = true` is the curve's `Side::Left`, which at a knot gives the derivatives of
        // the span that starts there (at `t = 1` of a line then an arc: the arc's; the test
        // `the_definitions_partials_at_the_joints_are_right_sided` holds this), `false` those of
        // the span that ends there, and at the domain's end those of the last span
        let d = self.path.evaluate(t, 2, !left, false)?;
        let (Some(&point), Some(&t1), Some(&t2)) = (d.first(), d.get(1), d.get(2)) else {
            return Err(SolidError::invalid_input(
                OP_EVAL,
                "the path has no derivatives",
            ));
        };
        let speed = t1.length();
        if !(speed.is_finite() && speed > 0.0) {
            return Err(SolidError::invalid_input(
                OP_EVAL,
                "the path's tangent vanishes",
            ));
        }
        let x = t1 * (1.0 / speed);
        let dx = (t2 - x * t2.dot(x)) * (1.0 / speed);
        let j = if left {
            self.spans.partition_point(|s| s.to < t)
        } else {
            self.spans
                .partition_point(|s| s.from <= t)
                .saturating_sub(1)
        }
        .min(self.spans.len().saturating_sub(1));
        let span = self
            .spans
            .get(j)
            .ok_or_else(|| SolidError::invalid_input(OP_EVAL, "the path has no spans"))?;
        // the frame is the exact closed form (a rigid rotation of the span's start frame), so
        // it is orthonormal and its rates are those of its own angle
        let (x, z, dx, dz) = match span.kind {
            SpanKind::Line => (span.x0, span.z0, Vec3::ZERO, Vec3::ZERO),
            SpanKind::Arc { axis, .. } => {
                // turned by the angle the tangent has turned about the axis
                let angle = span.x0.cross(x).dot(axis).atan2(span.x0.dot(x));
                let rate = axis.dot(x.cross(dx));
                let (xe, ze) = (
                    rodrigues(span.x0, axis, angle),
                    rodrigues(span.z0, axis, angle),
                );
                (xe, ze, axis.cross(xe) * rate, axis.cross(ze) * rate)
            }
        };
        let y = z.cross(x);
        let dy = dz.cross(x) + z.cross(dx);
        Ok(Frame {
            point,
            tangent: t1,
            axes: [x, y, z],
            rates: [dx, dy, dz],
        })
    }

    /// Moves the path with its net: `rotate_n_move` after `scale` (a uniform one); the
    /// profile, in the frame's own coordinates, takes the scale only (the caller's).
    ///
    /// # Errors
    ///
    /// Those of the path's transform.
    pub(crate) fn transform(
        &mut self,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
    ) -> Result<()> {
        self.path.transform(rotate_n_move, scale)?;
        let factor = scale.map_or(1.0, |s| s.x);
        for s in &mut self.spans {
            s.x0 = rotate_n_move.transform_vector(s.x0);
            s.z0 = rotate_n_move.transform_vector(s.z0);
            if let SpanKind::Arc { axis, radius } = &mut s.kind {
                *axis = rotate_n_move.transform_vector(*axis);
                *radius *= factor;
            }
        }
        Ok(())
    }

    /// How far the path reaches from the origin.
    pub(crate) fn reach(&self) -> Result<f64> {
        let c = self.path.canonical()?;
        Ok(c.control_points
            .iter()
            .fold(0.0_f64, |m, p| m.max(p.length())))
    }

    /// The motion of a net `net` of the profile `profile` along `path`, as the sweep placed
    /// the sections in `trace`; with its certified bound `|B - S| <= bound` and whether the
    /// net's `(u, v)` are the sweep's `(v, u)`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a net that is not the profile's sections moved along the path (its
    /// control points per section, its weights or its knots differ), a closed path whose
    /// transported frame does not close, a path
    /// with a span that is neither a line nor a circular arc (the frame of a general spline
    /// has no closed form yet: the #2505 ledger's `path-spline`) or a vanishing tangent or a
    /// joint that is not G1, a section whose net parameter is not found or a bound that is
    /// not finite.
    pub(crate) fn create(
        profile: &BSplineCurve,
        path: &BSplineCurve,
        trace: &SweepTrace,
        net: &BSplineSurface,
    ) -> Result<(Self, f64, bool)> {
        let (path, spans) = path_spans(path, trace.start_normal)?;
        let mut motion = Self {
            path,
            spans,
            nodes: Vec::new(),
        };
        let columns = NetView::read(profile, net)?;
        motion.nodes = motion.find_nodes(&columns, &trace.pieces)?;
        let bound = motion.certify(&columns, FITTED_GOAL)?;
        Ok((motion, bound, columns.swapped))
    }

    /// The motion of the profile along `path` and the net that is its definition exactly,
    /// where the path is made of lines and circular arcs: under the rotation-minimizing frame
    /// each control point of the profile traces a line or a circular arc per span, which is
    /// a rational quadratic (or linear) B-spline in the path's own parameter, so the net is
    /// the tensor product of those curves and the profile's, with knots at the path's joints.
    /// Its parameter across the sweep is the path's `t` (the map is the identity at the
    /// joints), its first parameter the path's and its second the profile's. Returns the
    /// motion, the net, and the bound certified against it (the rounding of building and
    /// evaluating it, unless the path's own parametrisation is not the standard one of an
    /// arc, which the bound then shows).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a closed path whose transported frame does not close,
    /// a span that is neither a line nor an arc, a joint
    /// that is not G1, an arc of half a turn or more in one span or a bound that is not
    /// finite; those of reading the curves.
    pub(crate) fn create_exact(
        profile: &BSplineCurve,
        path: &BSplineCurve,
        start_normal: Vec3,
    ) -> Result<(Self, BSplineSurface, f64)> {
        let (path, spans) = path_spans(path, start_normal)?;
        let mut nodes: Vec<(f64, f64)> = spans.iter().map(|s| (s.from, s.from)).collect();
        if let Some(last) = spans.last() {
            nodes.push((last.to, last.to));
        }
        let motion = Self { path, spans, nodes };
        let weights = bezier_weights(&motion.path)?;
        let net = motion.exact_net(profile, &weights)?;
        let view = NetView::read(profile, &net)?;
        let bound = motion.certify(&view, EXACT_GOAL)?;
        Ok((motion, net, bound))
    }

    /// The net of [`PathMotion::create_exact`].
    fn exact_net(
        &self,
        profile: &BSplineCurve,
        spans_weights: &[[f64; 3]],
    ) -> Result<BSplineSurface> {
        let prof = profile.canonical()?;
        if prof.dim != 3 {
            return Err(bad("a profile is a 3D curve"));
        }
        let arcs = self
            .spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::Arc { .. }));
        let degree = if arcs { 2 } else { 1 };
        let pw = |i: usize| prof.weights.get(i).copied().unwrap_or(1.0);
        // per profile control point, the control points along the sweep, and their weights
        let mut omega: Vec<f64> = Vec::new();
        let mut along: Vec<Vec<Vec3>> = vec![Vec::new(); prof.control_points.len()];
        for (k, span) in self.spans.iter().enumerate() {
            let start = self.frame_side(span.from, false)?;
            let end = self.frame_side(span.to, true)?;
            let arc: Option<(Vec3, Vec3, f64)> = match span.kind {
                SpanKind::Line => None,
                SpanKind::Arc { axis, radius } => {
                    let turned = span
                        .x0
                        .cross(end.axes[0])
                        .dot(axis)
                        .atan2(span.x0.dot(end.axes[0]));
                    if !(turned.abs() < std::f64::consts::PI - 1e-6 && turned.abs() > 0.0) {
                        return Err(bad("an arc span turns less than half a turn"));
                    }
                    let centre = start.point + start.rates[0] * (radius / start.rates[0].length());
                    let half = 0.5 * turned;
                    Some((centre, axis, half))
                }
            };
            // the path span's own weights (scaled to join the one before) keep its parametrisation
            let [w0, w1, w2] = spans_weights.get(k).copied().unwrap_or([1.0; 3]);
            let scale = if k == 0 {
                1.0 / w0
            } else {
                omega.last().copied().unwrap_or(1.0) / w0
            };
            if k == 0 {
                omega.push(w0 * scale);
            }
            if degree == 2 {
                omega.push(w1 * scale);
            }
            omega.push(w2 * scale);
            for (col, c) in along.iter_mut().zip(&prof.control_points) {
                let (a, b) = (start.place(*c), end.place(*c));
                if k == 0 {
                    col.push(a);
                }
                if degree == 2 {
                    col.push(match arc {
                        None => (a + b) * 0.5,
                        Some((centre, axis, half)) => {
                            let on_axis = centre + axis * (a - centre).dot(axis);
                            on_axis + rodrigues(a - on_axis, axis, half) * (1.0 / half.cos())
                        }
                    });
                }
                col.push(b);
            }
        }
        // the net: first parameter along the sweep, second along the profile
        let count = omega.len();
        let mut points = Vec::with_capacity(count * prof.control_points.len());
        let mut weights = Vec::with_capacity(points.capacity());
        for (j, w) in omega.iter().enumerate() {
            for (i, col) in along.iter().enumerate() {
                points.push(
                    col.get(j)
                        .copied()
                        .ok_or_else(|| bad("a column is short"))?,
                );
                weights.push(w * pw(i));
            }
        }
        let mut u_knots: Vec<f64> = self.spans.iter().map(|s| s.from).collect();
        u_knots.extend(self.spans.last().map(|s| s.to));
        let mut u_mult = vec![degree; u_knots.len()];
        if let Some(first) = u_mult.first_mut() {
            *first = degree + 1;
        }
        if let Some(last) = u_mult.last_mut() {
            *last = degree + 1;
        }
        let rational = weights.iter().any(|w| (w - 1.0).abs() > 1e-15);
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: degree,
            v_degree: prof.degree,
            control_points: &points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &u_mult,
            v_multiplicities: &prof.knot_multiplicities,
            u_knots: &u_knots,
            v_knots: &prof.knots,
            knot_type: KnotType::Unspecified,
            weights: rational.then_some(&weights[..]),
            uv_domain: None,
        })
    }

    /// Sweep skins interpolate at the trajectory parameters and preserve the supplied
    /// derivatives in that parameter. Adjacent pieces have contiguous domains, so joining
    /// them adds no parameter offset: the net-to-path map is the identity.
    fn find_nodes(&self, cols: &NetView, pieces: &[Vec<f64>]) -> Result<Vec<(f64, f64)>> {
        let domain = self.path.natural_interval();
        if pieces.is_empty() || cols.domain != (domain.min(), domain.max()) {
            return Err(bad(
                "a net's parameter across the sweep is not the path's parameter",
            ));
        }
        let mut nodes: Vec<(f64, f64)> = Vec::new();
        for piece in pieces {
            if piece.len() < 2 {
                return Err(bad("a piece has at least two sections"));
            }
            for &t in piece {
                match nodes.last() {
                    Some(&(_, previous)) if t == previous => {}
                    Some(&(_, previous)) if !t.is_finite() || t <= previous => {
                        return Err(bad("the net's sections are not in the path's order"));
                    }
                    _ => nodes.push((t, t)),
                }
            }
        }
        Ok(nodes)
    }

    /// The certified bound (module docs).
    /// `goal_rel`: the remainder of a cell, relative to `1 + big` (the largest coordinate
    /// met), below which the cell is not split.
    fn certify(&self, cols: &NetView, goal_rel: f64) -> Result<f64> {
        let bounds = PathBounds::new(&self.path)?;
        let (v0, v1) = cols.domain;
        let to_v = |t: f64| -> Option<f64> {
            let i = self
                .nodes
                .partition_point(|&(_, x)| x <= t)
                .saturating_sub(1)
                .min(self.nodes.len().saturating_sub(2));
            match (self.nodes.get(i), self.nodes.get(i + 1)) {
                (Some(&(va, ta)), Some(&(vb, tb))) if tb > ta => {
                    Some(va + (vb - va) * (t - ta) / (tb - ta))
                }
                _ => None,
            }
        };
        // cells never cross a knot of the net, a node, a station or a knot of the path
        let mut cuts: Vec<f64> = cols.unique_knots.clone();
        cuts.extend(self.nodes.iter().map(|n| n.0));
        cuts.extend(bounds.unique_knots.iter().filter_map(|&k| to_v(k)));
        cuts.retain(|&v| v >= v0 && v <= v1);
        cuts.push(v0);
        cuts.push(v1);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() <= 1e-14 * (1.0 + b.abs()));
        // the sizes the floor and the goal are relative to
        let reach = cols.profile.iter().fold(0.0_f64, |m, c| m.max(c.length()));
        let big = cols
            .columns
            .iter()
            .flat_map(|c| c.pts.ctrl.iter())
            .fold(0.0_f64, |m, p| m.max(p.length()))
            .max(self.reach()? + reach);
        let goal = goal_rel * (1.0 + big);
        let mut worst = 0.0_f64;
        let mut cells = 0_usize;
        for w in cuts.windows(2) {
            let &[a, b] = w else { continue };
            if !(b > a) {
                continue;
            }
            let mut stack: Vec<(f64, f64, u32)> = vec![(a, b, 0)];
            while let Some((lo, hi, depth)) = stack.pop() {
                cells += 1;
                let c = 0.5 * (lo + hi);
                let h = 0.5 * (hi - lo);
                let (t, m) = self.path_parameter(c);
                let frame = self.frame(t)?;
                // the path's derivative bounds over the cell's span, and what the frame's
                // turn adds: on an arc `R'' C` is at most `|C| (phi'' + phi'^2)` for the
                // angle `phi` of the tangent, `phi' <= |T'| / r` and `phi'' <= |T''| / r`
                let (d1, d2, _) = bounds.derivative_bounds(t);
                let turn = match self.span_at(t).map(|s| s.kind) {
                    Some(SpanKind::Arc { radius, .. }) => {
                        d2 / radius + (d1 / radius) * (d1 / radius)
                    }
                    _ => 0.0,
                };
                let can_split = depth < MAX_DEPTH && cells + stack.len() < MAX_CELLS;
                let mut cell = 0.0_f64;
                let mut split = false;
                for (col, cc) in cols.columns.iter().zip(&cols.profile) {
                    let f = col.eval(c) - frame.place(*cc);
                    let df = col.first(c) - frame.rate(*cc) * m;
                    let mp = col.second_bound(c);
                    let ms = m * m * (d2 + cc.length() * turn);
                    let rem = 0.5 * (mp + ms) * h * h;
                    if rem > goal && can_split {
                        split = true;
                        break;
                    }
                    cell = cell.max(f.length() + df.length() * h + rem);
                }
                if split {
                    stack.push((c, hi, depth + 1));
                    stack.push((lo, c, depth + 1));
                } else {
                    worst = worst.max(cell);
                }
            }
        }
        let floor = rounding_floor(big, reach);
        let bound = round_up(worst * (1.0 + SLACK) + floor);
        if bound.is_finite() && bound > 0.0 {
            Ok(bound)
        } else {
            Err(bad(
                "the net's distance from the definition is not certified finite",
            ))
        }
    }
}

/// The path's derivative bounds over a span (module docs): `T = N / w` for the homogeneous
/// numerator `N = w P` and the weight `w`, so
/// `|T'| <= (|N'| + |T| |w'|) / w`, `|T''| <= (|N''| + 2 D1 |w'| + |T| |w''|) / w` and
/// `|T'''| <= (|N'''| + 3 D2 |w'| + 3 D1 |w''| + |T| |w'''|) / w`, each of `N^(k)`, `w^(k)`
/// at most the largest of its control points over the span (convex hull), `w` at least the
/// smallest weight, `|T|` at most the largest `|P|`.
struct PathBounds {
    n: [Spline; 4],
    w: [Spline; 4],
    p: Spline,
    unique_knots: Vec<f64>,
}

impl PathBounds {
    fn new(path: &BSplineCurve) -> Result<Self> {
        let pc = path.canonical()?;
        let knots = full_knots(&pc.knots, &pc.knot_multiplicities);
        let weight = |i: usize| pc.weights.get(i).copied().unwrap_or(1.0);
        if (0..pc.control_points.len()).any(|i| !(weight(i) > 0.0 && weight(i).is_finite())) {
            return Err(bad("a path's weights are positive"));
        }
        let numerator: Vec<Vec3> = pc
            .control_points
            .iter()
            .enumerate()
            .map(|(i, &p)| p * weight(i))
            .collect();
        let weights: Vec<Vec3> = (0..pc.control_points.len())
            .map(|i| Vec3::new(weight(i), 0.0, 0.0))
            .collect();
        let chain = |s: Spline| {
            let s1 = s.derivative();
            let s2 = s1.derivative();
            let s3 = s2.derivative();
            [s, s1, s2, s3]
        };
        Ok(Self {
            n: chain(Spline::new(pc.degree, knots.clone(), numerator)?),
            w: chain(Spline::new(pc.degree, knots.clone(), weights)?),
            p: Spline::new(pc.degree, knots, pc.control_points)?,
            unique_knots: pc.knots,
        })
    }

    /// Upper bounds of `|T'|`, `|T''|` and `|T'''|` over the span holding `t`.
    fn derivative_bounds(&self, t: f64) -> (f64, f64, f64) {
        let wmin = self.w.first().map_or(0.0, |w| {
            w.local(t).iter().fold(f64::INFINITY, |m, p| m.min(p.x))
        });
        let tmax = self.p.largest(t);
        let nk = |k: usize| self.n.get(k).map_or(f64::INFINITY, |s| s.largest(t));
        let wk = |k: usize| self.w.get(k).map_or(f64::INFINITY, |s| s.largest(t));
        if !(wmin > 0.0) {
            return (f64::INFINITY, f64::INFINITY, f64::INFINITY);
        }
        let d1 = (nk(1) + tmax * wk(1)) / wmin;
        let d2 = (nk(2) + 2.0 * d1 * wk(1) + tmax * wk(2)) / wmin;
        let d3 = (nk(3) + 3.0 * d2 * wk(1) + 3.0 * d1 * wk(2) + tmax * wk(3)) / wmin;
        (d1, d2, d3)
    }
}

/// The net's control points grouped as the sweep's columns: one spline in `v` per control
/// point of the profile.
struct NetView {
    columns: Vec<Column>,
    /// The profile's control points, in the frame's coordinates.
    profile: Vec<Vec3>,
    unique_knots: Vec<f64>,
    domain: (f64, f64),
    swapped: bool,
}

impl NetView {
    /// Reads `net` as the profile's control points swept along one parameter, in the same
    /// rational basis (module docs).
    fn read(profile: &BSplineCurve, net: &BSplineSurface) -> Result<Self> {
        let pc = profile.canonical()?;
        if pc.dim != 3 {
            return Err(bad("a profile is a 3D curve"));
        }
        let sc = net.canonical()?;
        let grid = net.control_point_net()?;
        let (nu, nv) = (grid.u_count, grid.v_count);
        let sections = pc.control_points.len();
        let same = |a: &[f64], b: &[f64]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| (x - y).abs() <= 1e-12 * (1.0 + x.abs()))
        };
        let (swapped, along_knots, along_mult, along_degree, along) = if nu == sections
            && sc.u_degree == pc.degree
            && same(&sc.u_knots, &pc.knots)
            && sc.u_multiplicities == pc.knot_multiplicities
        {
            (false, &sc.v_knots, &sc.v_multiplicities, sc.v_degree, nv)
        } else if nv == sections
            && sc.v_degree == pc.degree
            && same(&sc.v_knots, &pc.knots)
            && sc.v_multiplicities == pc.knot_multiplicities
        {
            (true, &sc.u_knots, &sc.u_multiplicities, sc.u_degree, nu)
        } else {
            return Err(bad("a net's sections are not the profile's control points"));
        };
        let at = |i: usize, j: usize| {
            let idx = if swapped { j * nv + i } else { i * nv + j };
            (
                grid.points.get(idx).copied(),
                grid.weights.get(idx).copied(),
            )
        };
        let pw = |i: usize| pc.weights.get(i).copied().unwrap_or(1.0);
        let full = full_knots(along_knots, along_mult);
        // the weights are the profile's times one weight per control point along the sweep
        // (the same for every section): `omega`
        let weight = |i: usize, j: usize| at(i, j).1.unwrap_or(1.0);
        let mut omega = Vec::with_capacity(along);
        for j in 0..along {
            omega.push(weight(0, j) / pw(0));
        }
        if omega.iter().any(|o| !(o.is_finite() && *o > 0.0)) {
            return Err(bad("a net's weights are positive"));
        }
        for i in 0..sections {
            for (j, o) in omega.iter().enumerate() {
                let r = weight(i, j) / pw(i);
                if !((r - o).abs() <= 1e-12 * o.abs()) {
                    return Err(bad("a net's weights are not the profile's"));
                }
            }
        }
        let o0 = omega.first().copied().unwrap_or(1.0);
        let rational = omega.iter().any(|o| (o - o0).abs() > 1e-12 * o0.abs());
        let mut columns = Vec::with_capacity(sections);
        for i in 0..sections {
            let mut ctrl = Vec::with_capacity(along);
            for j in 0..along {
                let (Some(p), _) = at(i, j) else {
                    return Err(bad("a net's control points are missing"));
                };
                ctrl.push(p);
            }
            columns.push(Column::new(
                Spline::new(along_degree, full.clone(), ctrl)?,
                rational.then_some(&omega[..]),
            )?);
        }
        let domain = columns
            .first()
            .map(|c| c.pts.domain())
            .ok_or_else(|| bad("a net has sections"))?;
        Ok(Self {
            columns,
            profile: pc.control_points,
            unique_knots: along_knots.clone(),
            domain,
            swapped,
        })
    }
}

/// The floor of a path sweep's bound, a forward error bound of evaluating the net and the
/// definition in doubles: `gamma_96` times four times the sizes of the numbers (`size` the
/// largest coordinate met, `reach` the profile's). The one function [`PathMotion::certify`] and
/// [`ProceduralSweep::floor`] both take.
pub(crate) fn rounding_floor(size: f64, reach: f64) -> f64 {
    gamma(96) * (size + reach) * 4.0
}

/// `v` turned right-handed about the unit `axis` by `angle` (Rodrigues): the one rotation the
/// sweeps' frames use (this module's arcs, `sweep::stations`' pieces and corners).
pub(crate) fn rodrigues(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    v * angle.cos() + axis.cross(v) * angle.sin() + axis * (axis.dot(v) * (1.0 - angle.cos()))
}

/// Two unit tangents meet G1 where they are at most this far apart (docs/procedural-sweep.md
/// section 3.1): the one rule for a path's joints, here and in `sweep::stations`.
pub(crate) const G1_TANGENTS: f64 = 1e-8;

/// The unit tangent of `path` at `t`, from the left or the right.
fn unit_tangent(path: &BSplineCurve, t: f64, from_left: bool) -> Result<Vec3> {
    let d = path.evaluate(t, 1, from_left, false)?;
    let t1 = d.get(1).copied().unwrap_or(Vec3::ZERO);
    let l = t1.length();
    if l.is_finite() && l > 0.0 {
        Ok(t1 * (1.0 / l))
    } else {
        Err(bad("the path's tangent vanishes"))
    }
}

/// What the knot span `[from, to]` of `path` of degree `degree` is, by 12 samples: a line
/// (collinear within `1e-11` of the path's size; any degree up to 3 is then that line) or a
/// circular arc (degree up to 2, so a conic, which 12 points on a circle make that circle).
fn classify(path: &BSplineCurve, from: f64, to: f64, degree: usize) -> Result<SpanKind> {
    const N: u32 = 11;
    let mut pts = Vec::with_capacity(12);
    for i in 0..=N {
        let t = from + (to - from) * f64::from(i) / f64::from(N);
        pts.push(path.evaluate_point(t)?);
    }
    let size = pts.iter().fold(0.0_f64, |m, p| m.max(p.length()));
    let tol = 1e-11 * (1.0 + size);
    let (Some(&a), Some(&m), Some(&c)) = (pts.first(), pts.get(5), pts.last()) else {
        return Err(bad("a span has samples"));
    };
    let chord = c - a;
    let length = chord.length();
    if !(length > tol) {
        return Err(bad("a span is a point or closed"));
    }
    let dir = chord * (1.0 / length);
    let straight = pts
        .iter()
        .all(|&p| (p - a - dir * (p - a).dot(dir)).length() <= tol);
    if straight && degree <= 3 {
        return Ok(SpanKind::Line);
    }
    let (u, v) = (m - a, c - a);
    let w = u.cross(v);
    let w2 = w.dot(w);
    if degree > 2 || !(w2.sqrt() > tol * length) {
        return Err(bad(
            "a span is neither a line nor a circular arc: the frame of a general spline has no closed form",
        ));
    }
    let center = a + (w.cross(u) * v.dot(v) + v.cross(w) * u.dot(u)) * (0.5 / w2);
    let radius = (center - a).length();
    let axis = w * (1.0 / w2.sqrt());
    let circle = pts.iter().all(|&p| {
        ((p - center).length() - radius).abs() <= tol && (p - center).dot(axis).abs() <= tol
    });
    if circle && radius.is_finite() && radius > tol {
        Ok(SpanKind::Arc { axis, radius })
    } else {
        Err(bad(
            "a span is neither a line nor a circular arc: the frame of a general spline has no closed form",
        ))
    }
}

/// The spans of `path` between its unique `knots`, each classified, with the frame carried
/// from the start normal across the joints (which must be G1).
fn spans_of(
    path: &BSplineCurve,
    knots: &[f64],
    degree: usize,
    start_normal: Vec3,
) -> Result<Vec<Span>> {
    let mut spans: Vec<Span> = Vec::new();
    for w in knots.windows(2) {
        let &[from, to] = w else { continue };
        if !(to > from) {
            continue;
        }
        let kind = classify(path, from, to, degree)?;
        let x0 = unit_tangent(path, from, false)?;
        let z_in = match spans.last() {
            None => start_normal,
            Some(prev) => {
                let x_end = unit_tangent(path, prev.to, true)?;
                if (x_end - x0).length() > G1_TANGENTS {
                    return Err(bad("a joint of the path is not G1"));
                }
                match prev.kind {
                    SpanKind::Line => prev.z0,
                    SpanKind::Arc { axis, .. } => {
                        let angle = prev.x0.cross(x_end).dot(axis).atan2(prev.x0.dot(x_end));
                        rodrigues(prev.z0, axis, angle)
                    }
                }
            }
        };
        let u = z_in - x0 * z_in.dot(x0);
        let n = u.length();
        if !(n.is_finite() && n > 0.5) {
            return Err(bad("the frame's normal is not square to the path"));
        }
        spans.push(Span {
            from,
            to,
            kind,
            x0,
            z0: u * (1.0 / n),
        });
    }
    if spans.is_empty() {
        return Err(bad("a path has spans"));
    }
    Ok(spans)
}

/// One control point of the profile swept along the net's parameter: the curve `P_i(v)` of
/// the net's `i`-th column, polynomial or, when the net's weights vary along the sweep, a
/// quotient `N / W` of a numerator and a weight (module docs).
#[derive(Debug, Clone)]
struct Column {
    /// The Euclidean control points.
    pts: Spline,
    /// The first and second derivative of `pts`.
    poly: [Spline; 2],
    /// The numerator `N = W P` and the weight `W` (its `x`) with their first and second
    /// derivatives, for a column that is rational along the sweep.
    rat: Option<([Spline; 3], [Spline; 3])>,
}

impl Column {
    /// The column through `pts`, rational with the weights `omega` along the sweep if given.
    fn new(pts: Spline, omega: Option<&[f64]>) -> Result<Self> {
        let d1 = pts.derivative();
        let d2 = d1.derivative();
        let rat = match omega {
            None => None,
            Some(o) => {
                let n = pts
                    .ctrl
                    .iter()
                    .zip(o)
                    .map(|(p, &w)| *p * w)
                    .collect::<Vec<_>>();
                let w = o
                    .iter()
                    .map(|&w| Vec3::new(w, 0.0, 0.0))
                    .collect::<Vec<_>>();
                let chain = |s: Spline| {
                    let s1 = s.derivative();
                    let s2 = s1.derivative();
                    [s, s1, s2]
                };
                Some((
                    chain(Spline::new(pts.degree, pts.knots.clone(), n)?),
                    chain(Spline::new(pts.degree, pts.knots.clone(), w)?),
                ))
            }
        };
        Ok(Self {
            pts,
            poly: [d1, d2],
            rat,
        })
    }

    /// The point at `v`.
    fn eval(&self, v: f64) -> Vec3 {
        match &self.rat {
            None => self.pts.eval(v),
            Some(([n, ..], [w, ..])) => n.eval(v) * (1.0 / w.eval(v).x),
        }
    }

    /// The first derivative at `v`: `(N' - P W') / W` for a quotient.
    fn first(&self, v: f64) -> Vec3 {
        match &self.rat {
            None => self.poly[0].eval(v),
            Some(([_, n1, _], [w, w1, _])) => {
                let wv = w.eval(v).x;
                (n1.eval(v) - self.eval(v) * w1.eval(v).x) * (1.0 / wv)
            }
        }
    }

    /// An upper bound of `|P''|` over the span holding `v`: the quotient rule on the control
    /// polygons' hulls, as for the path ([`PathBounds`]).
    fn second_bound(&self, v: f64) -> f64 {
        match &self.rat {
            None => self.poly[1].largest(v),
            Some(([_, n1, n2], [w, w1, w2])) => {
                let wmin = w.local(v).iter().fold(f64::INFINITY, |m, p| m.min(p.x));
                if !(wmin > 0.0) {
                    return f64::INFINITY;
                }
                let pmax = self.pts.largest(v);
                let d1 = (n1.largest(v) + pmax * w1.largest(v)) / wmin;
                (n2.largest(v) + 2.0 * d1 * w1.largest(v) + pmax * w2.largest(v)) / wmin
            }
        }
    }
}

/// A copy of a 3D path and its spans, with a closing frame when the path is closed:
/// the checks both ways of making a motion ([`PathMotion::create`], [`PathMotion::create_exact`]) share.
fn path_spans(path: &BSplineCurve, start_normal: Vec3) -> Result<(BSplineCurve, Vec<Span>)> {
    let pc = path.canonical()?;
    if pc.dim != 3 {
        return Err(bad("a path is a 3D curve"));
    }
    let closed = match (pc.control_points.first(), pc.control_points.last()) {
        (Some(a), Some(b)) => (*a - *b).length() <= 1e-9 * (1.0 + a.length()),
        _ => true,
    };
    let mut path = path.copy();
    path.set_out_of_bounds_enabled(false);
    let spans = spans_of(&path, &pc.knots, pc.degree, start_normal)?;
    let motion = PathMotion {
        path,
        spans,
        nodes: Vec::new(),
    };
    if closed {
        // A closed path needs no frame correction when parallel transport returns
        // the entire frame to its start. Otherwise the fitted sweep's distributed
        // correction is a different motion and cannot use this definition.
        let domain = motion.path.natural_interval();
        let start = motion.frame_side(domain.min(), false)?;
        let end = motion.frame_side(domain.max(), true)?;
        if start.axes.iter().zip(&end.axes).any(|(a, b)| {
            let gap = (*a - *b).length();
            !gap.is_finite() || gap > G1_TANGENTS
        }) {
            return Err(bad("a closed path's transported frame does not close"));
        }
    }
    Ok((motion.path, motion.spans))
}

/// The weights `[w0, w1, w2]` of each knot span of `path` as a quadratic rational Bezier
/// (the path refined so every interior knot has multiplicity 2); `[1; 3]` for every span of a
/// path of another degree (its lines are taken at the linear parametrisation, which the
/// bound shows where it does not hold).
fn bezier_weights(path: &BSplineCurve) -> Result<Vec<[f64; 3]>> {
    let pc = path.canonical()?;
    let spans = pc.knots.len().saturating_sub(1);
    if pc.degree != 2 {
        return Ok(vec![[1.0; 3]; spans]);
    }
    let mut refined = path.clone();
    let extra: Vec<f64> = pc
        .knots
        .iter()
        .zip(&pc.knot_multiplicities)
        .skip(1)
        .take(spans.saturating_sub(1))
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, 2_usize.saturating_sub(m)))
        .collect();
    refined.insert_knots(&extra)?;
    let rc = refined.canonical()?;
    (0..spans)
        .map(|k| {
            let w = |j: usize| rc.weights.get(2 * k + j).copied().unwrap_or(1.0);
            let ws = [w(0), w(1), w(2)];
            if ws.iter().all(|x| x.is_finite() && *x > 0.0) {
                Ok(ws)
            } else {
                Err(bad("a path's weights are positive"))
            }
        })
        .collect()
}
