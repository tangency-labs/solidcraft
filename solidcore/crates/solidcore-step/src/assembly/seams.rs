//! Seam edges for faces that wind around a periodic surface without one. STEP lets a face on a
//! cylinder, cone or sphere be bounded by loops that go once around the axis (a band between
//! two circles, a cap bounded by one circle, a cone bounded by its base and a vertex loop at its
//! apex); schema 4 keeps every loop closed in the surface's (u, v), so such a face gets a seam:
//! a ruling line on a cylinder or cone, a meridian arc on a sphere, from a vertex of one loop to
//! the other loop (split there when it has no vertex at that angle) or to the pole the face
//! covers; on a surface of revolution, its profile turned to a quarter line (a multiple of a
//! quarter turn from the profile), where the revolution's form starts, the loops split there.
//! The loops merge into one: the first, the seam out, the second (or nothing at a pole), the seam
//! back. A face on a B-spline surface that closes on itself around u (its first and
//! last control columns the same) gets a seam the same way, along the surface's own boundary
//! column (u at the start of its knots), the loops split there where they have no vertex on it;
//! the period is the width of its knots. One closed around v only gets it along its boundary row
//! (v at the start of its knots). A torus face winding around its axis (a band between two
//! circles about it, a half torus bounded by its rims) gets a seam round the tube at a vertex's
//! angle, on the face's side of the tube; one winding round its tube (an elbow's wall) a circle
//! about the axis. A spline closed both ways is refused for now.
//!
//! A face bounded by a slit alone (a loop out along edges and straight back, of no (u, v) area)
//! on a closed surface is all of that surface less a cut of none: its slit is made its seams
//! ([`Assembly::complete_slits`]), so the loop bounds the surface's whole (u, v) domain. A slit that
//! is only one bound of a face with another loop encloses nothing of it either, and is left out
//! of the face ([`Assembly::drop_inner_slits`]).

use crate::vec::Vec3Geom as _;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::kernel::{CurveEval, Starts, SurfaceEval};
use solidcore::nurbs::math::gamma;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve as BCurve, Frame, Surface as BSurface, Vertex};

use super::{Assembly, Bound, EdgeB, R, index, unsupported};

/// Samples per edge when following a loop around the axis.
const SAMPLES: usize = 16;
/// How often a Bézier piece that reaches half a turn about an axis is halved for its parts'
/// arcs before its edge is taken to reach every angle.
const MAX_HALVINGS: u32 = 4;
/// Edges split to place seams, per solid.
const MAX_SPLITS: usize = 10_000;

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn wrap(x: f64) -> f64 {
    wrap_by(x, TAU)
}

/// `x` wrapped into (−period/2, period/2].
fn wrap_by(x: f64, period: f64) -> f64 {
    x - (x / period).round() * period
}

/// The turns a loop makes round a surface periodic in u, v or both, counted sample by sample.
pub(crate) struct Turns {
    periods: [Option<f64>; 2],
    /// The first and the last sample's parameters.
    first: Option<[f64; 2]>,
    last: Option<[f64; 2]>,
    /// Turns so far in each direction, a fraction of a period per step.
    total: [f64; 2],
}

impl Turns {
    pub(crate) fn new(periods: [Option<f64>; 2]) -> Self {
        Self { periods, first: None, last: None, total: [0.0; 2] }
    }

    /// The step from `l` to `q` as a fraction of each period, wrapped to the nearer way round.
    fn steps(&self, l: [f64; 2], q: [f64; 2]) -> [f64; 2] {
        let mut out = [0.0; 2];
        for (((o, period), q), l) in out.iter_mut().zip(self.periods).zip(q).zip(l) {
            if let Some(p) = period {
                *o = wrap_by(q - l, p) / p;
            }
        }
        out
    }

    /// Takes the next sample on the surface from `candidates`, each its parameters and its
    /// distance from the sample. Of those within `reach` (the edge's tolerance) of it, the loop
    /// continues on the one whose parameters step least round the surface from the last sample's,
    /// so a point within the tolerance of two parts of the surface (a profile folding back
    /// within the tolerance of itself) stays on the part the loop is on and the count cannot
    /// change by a period where the nearer of the two changes. Only where none is within reach,
    /// the loop having left the part it was on, does it take the nearest. A sample with no
    /// candidate is passed over (returns whether the sample was taken).
    pub(crate) fn step(&mut self, candidates: &[([f64; 2], f64)], reach: f64) -> bool {
        let size = |d: [f64; 2]| d[0].abs().max(d[1].abs());
        let within = candidates.iter().filter(|c| c.1 <= reach);
        let pick = match self.last {
            Some(l) => within.min_by(|a, b| size(self.steps(l, a.0)).total_cmp(&size(self.steps(l, b.0)))),
            None => within.min_by(|a, b| a.1.total_cmp(&b.1)),
        }
        .or_else(|| candidates.iter().min_by(|a, b| a.1.total_cmp(&b.1)));
        let Some(&(q, _)) = pick else { return false };
        self.add(q);
        self.first.get_or_insert(q);
        self.last = Some(q);
        true
    }

    fn add(&mut self, q: [f64; 2]) {
        if let Some(l) = self.last {
            let steps = self.steps(l, q);
            for (t, s) in self.total.iter_mut().zip(steps) {
                *t += s;
            }
        }
    }

    /// Closes a loop whose first and last samples are the same point (its start): steps from
    /// the last sample's parameters back to the first's. Round a closed path of parameters the
    /// steps add to whole turns, so a loop that started on one part of the surface within the
    /// tolerance of its point and ended on another is not left a fraction of a turn short.
    pub(crate) fn close(&mut self) {
        if let Some(f) = self.first {
            self.add(f);
            self.last = Some(f);
        }
    }

    /// Whole turns in u and in v.
    pub(crate) fn count(&self) -> [i64; 2] {
        self.total.map(|x| x.round() as i64)
    }
}

/// The middle of the widest stretch of angle `k` (0: longitude, 1: tube angle) that no sample
/// of `samples` lies in, when it is clear of them by more than four times `tol` at `radius`
/// (the smallest radius that angle is measured at); `None` otherwise.
fn widest_free(samples: &[[f64; 2]], k: usize, radius: f64, tol: f64) -> Option<f64> {
    let mut angles: Vec<f64> = samples.iter().filter_map(|q| q.get(k)).map(|a| a.rem_euclid(TAU)).collect();
    angles.sort_by(f64::total_cmp);
    let (first, last) = (*angles.first()?, *angles.last()?);
    let mut best = (first + TAU - last, last);
    for pair in angles.windows(2) {
        if let [a, b] = pair
            && b - a > best.0
        {
            best = (b - a, *a);
        }
    }
    let (gap, from) = best;
    (0.5 * gap * radius > 4.0 * tol).then(|| wrap(from + 0.5 * gap))
}

/// The path a slit runs out along: a loop that runs out along a chain of edges and straight back
/// along it, each edge used once each way, started at one of its two turns; `None` for any
/// other loop.
pub(crate) fn slit_path(es: &[(u32, bool)]) -> Option<Vec<(u32, bool)>> {
    let n = es.len();
    if n < 2 || !n.is_multiple_of(2) || !super::seam_only(es) {
        return None;
    }
    // A turn: a use followed by its edge the other way.
    let k = (0..n).find(|&k| matches!((es.get((k + n - 1) % n), es.get(k)), (Some(a), Some(b)) if a.0 == b.0))?;
    let ring: Vec<(u32, bool)> = es.iter().skip(k).chain(es.iter().take(k)).copied().collect();
    let (out, back) = ring.split_at(n / 2);
    back.iter().rev().zip(out).all(|(b, o)| b.0 == o.0 && b.1 != o.1).then(|| out.to_vec())
}

/// The axis frame of a surface of revolution: origin, unit axis, and the pole points at the
/// ends of its v range that have one (min v, max v).
struct Axis {
    origin: Vec3,
    z: Vec3,
    kind: Kind,
    poles: [Option<Vec3>; 2],
    /// The period around (2π, or a closed spline's knot width).
    period: f64,
}

/// How an edge of a seam-only loop sits on a pole-closed surface of revolution.
enum SeamEdgeKind {
    /// Runs monotonically in `v` along a meridian at longitude `u` (measured at its sample
    /// farthest from the axis, at squared radial distance `radial2`; `forward` = increasing `v`).
    Meridian { u: f64, radial2: f64, forward: bool, v_mid: f64 },
    /// Runs along a constant interior `v` strictly off both poles.
    Parallel,
    /// Neither a meridian nor an interior parallel.
    Other,
}

#[derive(Clone, PartialEq)]
enum Kind {
    Ruled,
    Sphere(f64),
    /// A torus, its ring's and tube's radii: a face winding around its axis (a band between two
    /// circles about it) gets a seam round the tube.
    Torus {
        major: f64,
        minor: f64,
    },
    /// A torus face winding round its tube (a band between two circles round the tube, an elbow's
    /// wall): its seam a circle about the axis, at a vertex's angle round the tube.
    TorusTube,
    /// A placed revolution with u the angle: its profile.
    Revolved(BCurve),
    /// A B-spline surface closed on itself in one parameter, `dir` (0: u, 1: v): its boundary
    /// line where that parameter's knots start (`start`), a curve in the other parameter, along
    /// which every seam runs (the column at the first u knot, or the row at the first v knot).
    Iso {
        start: f64,
        line: BCurve,
        dir: usize,
    },
}

impl Kind {
    /// The parameter a face's loops wind in around this frame (0: u, 1: v): v round a torus's
    /// tube and round a spline closed in v, u everywhere else.
    fn winds_in(&self) -> usize {
        usize::from(matches!(self, Kind::TorusTube | Kind::Iso { dir: 1, .. }))
    }
}

fn axis(s: &BSurface) -> Option<Axis> {
    let frame = |f: &Frame| {
        let (o, x, y) = (Vec3::from(f.origin), Vec3::from(f.x_axis), Vec3::from(f.y_axis));
        (o, x.cross(y))
    };
    match s {
        BSurface::Cylinder { frame: f, .. } => {
            let (origin, z) = frame(f);
            Some(Axis { origin, z, kind: Kind::Ruled, poles: [None, None], period: TAU })
        }
        BSurface::Cone { frame: f, radius_at_origin, half_angle } => {
            let (origin, z) = frame(f);
            let apex = origin + z * (-radius_at_origin / half_angle.tan());
            let poles = if *half_angle > 0.0 { [Some(apex), None] } else { [None, Some(apex)] };
            Some(Axis { origin, z, kind: Kind::Ruled, poles, period: TAU })
        }
        BSurface::Sphere { frame: f, radius } => {
            let (origin, z) = frame(f);
            Some(Axis { origin, z, kind: Kind::Sphere(*radius), poles: [Some(origin - z * *radius), Some(origin + z * *radius)], period: TAU })
        }
        BSurface::Torus { frame: f, major_radius, minor_radius, .. } => {
            let (origin, z) = frame(f);
            Some(Axis { origin, z, kind: Kind::Torus { major: *major_radius, minor: *minor_radius }, poles: [None, None], period: TAU })
        }
        BSurface::PlacedRevolution { profile, origin, axis, matrix, forward: false } if *matrix == IDENTITY => {
            let (origin, z) = (Vec3::from(*origin), Vec3::from(*axis));
            let c = CurveEval::new(profile).ok()?;
            let solidcore_record::brep::Natural::Range(a, b) = profile.natural() else { return None };
            // A profile end on the axis is a pole.
            let pole = |t: f64| {
                c.point(t).filter(|p| {
                    let d = *p - origin;
                    (d - z * d.dot(z)).len() <= 1e-9 * (1.0 + d.len())
                })
            };
            Some(Axis { origin, z, kind: Kind::Revolved((**profile).clone()), poles: [pole(a), pole(b)], period: TAU })
        }
        _ => None,
    }
}

/// The seam frame of a B-spline surface closed in one parameter only ([`Kind::Iso`]): its line at
/// the start of that parameter's knots (the column at the first u knot for a surface closed
/// around u, the row at the first v knot for one closed around v), and the points the other
/// parameter's ends collapse to (a pole), if they do.
fn spline_axis(s: &BSurface, eval: &SurfaceEval) -> Option<Axis> {
    let dom = eval.domain();
    let (dir, period) = match (dom.period[0], dom.period[1]) {
        (Some(p), None) => (0, p),
        (None, Some(p)) => (1, p),
        _ => return None,
    };
    let ((s0, s1), (o0, o1)) = if dir == 0 { (dom.range[0]?, dom.range[1]?) } else { (dom.range[1]?, dom.range[0]?) };
    // The control points at the start of the closed parameter, the knots clamped: the first
    // column (u index 0) or the first entry of every row (v index 0).
    let line = match (s, dir) {
        (BSurface::Spline { v_knots, points, .. }, 0) => BCurve::Spline { knots: v_knots.clone(), points: points.first()?.clone() },
        (BSurface::Spline { u_knots, points, .. }, _) => {
            BCurve::Spline { knots: u_knots.clone(), points: points.iter().map(|r| r.first().copied()).collect::<Option<_>>()? }
        }
        (BSurface::RationalSpline { v_knots, homogeneous, .. }, 0) => {
            BCurve::RationalSpline { knots: v_knots.clone(), homogeneous: homogeneous.first()?.clone() }
        }
        (BSurface::RationalSpline { u_knots, homogeneous, .. }, _) => {
            BCurve::RationalSpline { knots: u_knots.clone(), homogeneous: homogeneous.iter().map(|r| r.first().copied()).collect::<Option<_>>()? }
        }
        _ => return None,
    };
    let at = |along: f64, other: f64| if dir == 0 { eval.point([along, other]) } else { eval.point([other, along]) };
    // An end of the other parameter is a pole where the surface's points across the closed one
    // there are one point.
    let pole = |o: f64| {
        let p = at(s0, o)?;
        let across = (1..8).filter_map(|k| at(s0 + (s1 - s0) * k as f64 / 8.0, o)).map(|q| q.dist(p)).fold(0.0, f64::max);
        (across <= 1e-9 * (1.0 + p.len())).then_some(p)
    };
    Some(Axis {
        origin: Vec3::new(0.0, 0.0, 0.0),
        z: Vec3::new(0.0, 0.0, 1.0),
        kind: Kind::Iso { start: s0, line, dir },
        poles: [pole(o0), pole(o1)],
        period,
    })
}

const IDENTITY: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// `p` turned by `angle` about the axis through `o` along unit `k` (right-handed).
fn turned(p: Vec3, o: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    let x = p - o;
    o + x * c + k.cross(x) * s + k * (k.dot(x) * (1.0 - c))
}

/// A revolution's profile turned by `angle`: the meridian there, with the profile's parameter.
fn meridian(profile: &BCurve, o: Vec3, k: Vec3, angle: f64) -> R<BCurve> {
    let turn = |h: &[f64; 4]| {
        let w = h[3];
        let p = turned(Vec3::new(h[0] / w, h[1] / w, h[2] / w), o, k, angle);
        [p.x * w, p.y * w, p.z * w, w]
    };
    Ok(match profile {
        BCurve::Spline { knots, points } => {
            BCurve::Spline { knots: knots.clone(), points: points.iter().map(|p| arr(turned(Vec3::from(*p), o, k, angle))).collect() }
        }
        BCurve::RationalSpline { knots, homogeneous } => {
            BCurve::RationalSpline { knots: knots.clone(), homogeneous: homogeneous.iter().map(turn).collect() }
        }
        BCurve::Line { start, end } => {
            BCurve::Line { start: arr(turned(Vec3::from(*start), o, k, angle)), end: arr(turned(Vec3::from(*end), o, k, angle)) }
        }
        _ => return Err(unsupported("a revolution of this profile")),
    })
}

/// A B-spline surface closed around u (its first and last control columns the same, `period`
/// its knot width) started at `s` instead, exactly: `s` inserted to full multiplicity, then the
/// piece from `s` to the end followed by the piece from the start to `s`, moved on by a period.
/// The same points at parameters a period apart before `s`; `None` for another surface or an `s`
/// outside the open domain.
fn restarted(surface: &BSurface, s: f64, period: f64) -> Option<BSurface> {
    use solidcore::eval::pcurve::segment;
    let (u_knots, v_knots, rows): (&Vec<f64>, &Vec<f64>, Vec<Vec<[f64; 4]>>) = match surface {
        BSurface::Spline { u_knots, v_knots, points } => {
            (u_knots, v_knots, points.iter().map(|r| r.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()).collect())
        }
        BSurface::RationalSpline { u_knots, v_knots, homogeneous } => (u_knots, v_knots, homogeneous.clone()),
        _ => return None,
    };
    let n = rows.len();
    let p = u_knots.len().checked_sub(n + 1)?;
    let (u0, u1) = (*u_knots.get(p)?, *u_knots.get(n)?);
    if !(u0 < s && s < u1) || !((u1 - u0 - period).abs() <= 1e-9 * period.max(1.0)) {
        return None;
    }
    // Each v column of control points across u is a curve along u.
    let columns = rows.first()?.len();
    let column = |j: usize| rows.iter().map(|r| r.get(j).copied()).collect::<Option<Vec<[f64; 4]>>>();
    let mut knots: Option<Vec<f64>> = None;
    let mut out: Vec<Vec<[f64; 4]>> = Vec::with_capacity(columns);
    for j in 0..columns {
        let c = column(j)?;
        let (ka, a) = segment(p, u_knots, &c, s, u1).ok()?;
        let (kb, b) = segment(p, u_knots, &c, u0, s).ok()?;
        // The piece after the seam starts where the one before it ends (the closed surface's
        // last column is its first): joined there with multiplicity p.
        let mut k = ka.get(..ka.len() - 1)?.to_vec();
        k.extend(kb.get(p + 1..)?.iter().map(|x| x + period));
        let mut pts = a;
        pts.extend(b.get(1..)?.iter().copied());
        if k.len() != pts.len() + p + 1 || knots.as_ref().is_some_and(|x| *x != k) {
            return None;
        }
        knots = Some(k);
        out.push(pts);
    }
    let u_knots = knots?;
    // Back to rows along u.
    let rows: Vec<Vec<[f64; 4]>> =
        (0..out.first()?.len()).map(|i| out.iter().map(|c| c.get(i).copied()).collect::<Option<Vec<_>>>()).collect::<Option<_>>()?;
    Some(match surface {
        BSurface::Spline { .. } => BSurface::Spline {
            u_knots,
            v_knots: v_knots.clone(),
            points: rows.iter().map(|r| r.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect()).collect(),
        },
        _ => BSurface::RationalSpline { u_knots, v_knots: v_knots.clone(), homogeneous: rows },
    })
}

impl Assembly<'_> {
    /// Start every B-spline surface closed around u that a face lies across the start of, without
    /// winding round it, in the middle of the widest stretch of u the face leaves free, or at the
    /// face's own seam where it goes once round bounded by one: the face's pcurves then run inside
    /// the surface's knots, as a kernel's trims have to (one crossing the start had its pcurve
    /// clamped there, mm off its edge). Exact ([`restarted`]). Returns how many surfaces were
    /// started anew.
    pub(crate) fn start_splines_clear_of_faces(&mut self) -> R<usize> {
        let mut turned = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            if !matches!(face.surface, BSurface::Spline { .. } | BSurface::RationalSpline { .. }) {
                continue;
            }
            let eval = SurfaceEval::new(&face.surface).map_err(|e| e.to_string())?;
            let dom = eval.domain();
            let (Some(period), None, Some((u0, _))) = (dom.period[0], dom.period[1], dom.range[0]) else { continue };
            let poles: Vec<Vec3> = spline_axis(&face.surface, &eval).iter().flat_map(|ax| ax.poles.iter().flatten().copied()).collect();
            // How far along u each loop of the face reaches, followed round from its first point
            // (unwrapped across the start); a face winding round is the seams' to handle. The
            // widest loop bounds the face, with the seam it has: an edge it uses twice.
            let mut widest: Option<(f64, f64, Option<u32>)> = None;
            let mut winds = false;
            for l in &face.loops {
                let Bound::Edges(es) = l else { continue };
                winds |= self.winding(&eval, es, [Some(period), None], &poles)?[0] != 0;
                let seam = es.iter().enumerate().find_map(|(i, &(e, _))| es.get(i + 1..)?.iter().any(|&(f, _)| f == e).then_some(e));
                let mut last: Option<f64> = None;
                let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                for &(e, _) in es {
                    let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                    let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                    let [a, b] = edge.interval;
                    for k in 0..=SAMPLES {
                        let t = a + (b - a) * k as f64 / SAMPLES as f64;
                        let Some(q) = c.point(t).and_then(|p| eval.project(p, None, f64::INFINITY)) else { continue };
                        let u = last.map_or(q[0], |x| x + wrap_by(q[0] - x, period));
                        (lo, hi) = (lo.min(u), hi.max(u));
                        last = Some(u);
                    }
                }
                if hi - lo > widest.map_or(f64::NEG_INFINITY, |(a, b, _)| b - a) {
                    widest = Some((lo, hi, seam));
                }
            }
            let Some((lo, hi, seam)) = widest else { continue };
            // Across the start: a whole number of periods from it lies inside the reach, clear of
            // its ends (a face that only meets the start there runs inside the knots already).
            let start = u0 + ((hi - u0) / period).floor() * period;
            let clear = 1e-9 * period;
            let crossing = hi - start > clear && start - lo > clear;
            if winds {
                continue;
            }
            let at = if hi - lo < period {
                if !crossing {
                    continue;
                }
                // The middle of the stretch the face leaves free.
                let free = hi + 0.5 * (period - (hi - lo));
                u0 + (free - u0).rem_euclid(period)
            } else if let Some(e) = seam {
                // A whole turn bounded by its own seam (the edges are followed along their
                // curves, so a band's reach is two turns): started at the seam where that is off
                // the start (a band round a hole as a general placement writes it, #18779), so
                // the seam's two uses are the knots' two ends, a period apart. Off the start,
                // both were given the seam's one u inside the knots, and the band enclosed no
                // (u, v) area.
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                let [a, b] = edge.interval;
                let Some(q) = c.point(0.5 * (a + b)).and_then(|p| eval.project(p, None, f64::INFINITY)) else { continue };
                let at = u0 + (q[0] - u0).rem_euclid(period);
                if at - u0 <= clear || u0 + period - at <= clear {
                    continue;
                }
                at
            } else {
                continue;
            };
            let Some(surface) = restarted(&face.surface, at, period) else { continue };
            if let Some(f) = self.faces.get_mut(fi) {
                f.surface = surface;
                f.extent = None;
            }
            turned += 1;
        }
        Ok(turned)
    }

    /// Give every face that winds around its surface without a seam one. Returns how many
    /// seams were added.
    pub(crate) fn add_seams(&mut self) -> R<usize> {
        let mut added = 0;
        let mut splits = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let surface = face.surface.clone();
            let natural = super::natural_periods(&surface);
            let eval = SurfaceEval::new(&surface).map_err(|e| e.to_string())?;
            // Periodic as the surface is, or as a spline closed on itself is (its knot width).
            let dom = eval.domain().period;
            let periods = [(natural[0], dom[0]), (natural[1], dom[1])].map(|(n, p)| if n { Some(TAU) } else { p });
            if periods.iter().all(Option::is_none) {
                continue;
            }
            let found = axis(&surface).or_else(|| spline_axis(&surface, &eval));
            let poles: Vec<Vec3> = found.iter().flat_map(|ax| ax.poles.iter().flatten().copied()).collect();
            let mut up = Vec::new();
            let mut down = Vec::new();
            // On a torus a face may wind round its tube instead, and on a spline closed in v it
            // can only wind that way: its loops turn in v, not u. Counted as turns about the frame.
            let v_turns = matches!(surface, BSurface::Torus { .. }) || found.as_ref().is_some_and(|ax| ax.kind.winds_in() == 1);
            let mut tube = false;
            for (li, l) in face.loops.iter().enumerate() {
                let Bound::Edges(es) = l else { continue };
                let mut w = self.winding(&eval, es, periods, &poles)?;
                if v_turns && w[0] == 0 && w[1].abs() == 1 {
                    tube = true;
                    w = [w[1], 0];
                }
                if w[1] != 0 || w[0].abs() > 1 {
                    return Err(format!("face #{}: {}", face.origin, unsupported("a loop winding around a surface more than once or in v")));
                }
                // A loop through a pole turns once round the axis and closes there, where every
                // angle is one point: a cone's base circle with its seam out to the apex and
                // back. It has its seam; another one doubled it, a sliver the face crossed.
                if w[0] != 0 && found.as_ref().is_some_and(|ax| self.through_pole(es, ax)) {
                    continue;
                }
                match w[0] {
                    1 => up.push(li),
                    -1 => down.push(li),
                    _ => {}
                }
            }
            if up.is_empty() && down.is_empty() {
                continue;
            }
            let step = face.origin;
            let found = match found {
                Some(ax) if tube && matches!(ax.kind, Kind::Torus { .. }) => Some(Axis { kind: Kind::TorusTube, ..ax }),
                f => f,
            };
            let Some(ax) = found else {
                return Err(format!("face #{step}: {}", unsupported("a face winding around a surface with no seam frame")));
            };
            // The face lies to the left of its loops in (u, v): above a loop running +u, and on
            // the side of lower u of a loop running +v.
            let (left_of_up, left_of_down) = if ax.kind.winds_in() == 1 { (ax.poles[0], ax.poles[1]) } else { (ax.poles[1], ax.poles[0]) };
            match (up.as_slice(), down.as_slice()) {
                ([l], []) => self.pole_seam(fi, *l, &ax, left_of_up, &eval)?,
                ([], [l]) => self.pole_seam(fi, *l, &ax, left_of_down, &eval)?,
                ([a], [b]) => self.band_seam(fi, *a, *b, &ax, &eval, &mut splits)?,
                _ => return Err(format!("face #{step}: {}", unsupported("a face with several loops around its axis"))),
            }
            added += 1;
        }
        Ok(added)
    }

    /// Split every edge that passes through a pole of a face it bounds (a sphere's, a revolved
    /// face's on its axis) inside its interval: a great circle from the equator over the pole to a
    /// vertex just past it. On the face's parameters the pole is a line, along which the edge's
    /// pcurve would have to jump half a turn; split there, the jump is at a vertex on the pole,
    /// where a loop may run along the pole's line. Both pieces of a split edge are looked at again:
    /// a great circle through both poles (a hemisphere's rim on a sphere whose axis lies in the
    /// rim's plane) is split at each, the second pole lying on the piece the first split made.
    /// Returns how many splits were made.
    pub(crate) fn split_at_poles(&mut self) -> R<usize> {
        let mut splits = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let Some(ax) = axis(&face.surface) else { continue };
            let poles: Vec<Vec3> = ax.poles.iter().flatten().copied().collect();
            if poles.is_empty() {
                continue;
            }
            let mut edges: Vec<u32> =
                face.loops.iter().filter_map(|l| if let Bound::Edges(es) = l { Some(es) } else { None }).flatten().map(|u| u.0).collect();
            edges.sort_unstable();
            edges.dedup();
            for e in edges {
                // The pieces still to look at: the edge, then each piece a split leaves.
                let mut pieces = vec![e];
                while let Some(piece) = pieces.pop() {
                    for pole in &poles {
                        let Some(t) = self.through_at(piece, *pole)? else { continue };
                        splits += 1;
                        if splits > MAX_SPLITS {
                            return Err(format!("more than {MAX_SPLITS} edges split at poles"));
                        }
                        // `split_edge` keeps the part before `t` as `piece` and appends the rest.
                        let rest = index(self.edges.len())?;
                        let v = self.split_edge(piece, t)?;
                        // The new vertex is the pole, as the face's parameters have it.
                        if let Some(x) = self.vertices.get_mut(v as usize) {
                            let gap = Vec3::from(x.point).dist(*pole);
                            x.point = arr(*pole);
                            x.tolerance = x.tolerance.max(1.5 * gap);
                        }
                        // Either piece may pass through another pole; neither through this one,
                        // which is now an end of both (`through_at` takes no pole at an end).
                        pieces.extend([rest, piece]);
                        break;
                    }
                }
            }
        }
        Ok(splits)
    }

    /// Where edge `e` passes through `pole` inside its interval (within the tolerance, away from
    /// its ends), if it does: its nearest point to the pole, refined by golden-section search.
    fn through_at(&self, e: u32, pole: Vec3) -> R<Option<f64>> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let [lo, hi] = edge.interval;
        let reach = self.tol.max(edge.tolerance);
        let d = |t: f64| c.point(t).map_or(f64::INFINITY, |p| p.dist(pole));
        let step = (hi - lo) / SAMPLES as f64;
        let k = (0..=SAMPLES).min_by(|&i, &j| d(lo + step * i as f64).total_cmp(&d(lo + step * j as f64))).unwrap_or(0);
        let (mut a, mut b) = (lo + step * k.saturating_sub(1) as f64, (lo + step * (k + 1) as f64).min(hi));
        let g = 0.5 * (5f64.sqrt() - 1.0);
        for _ in 0..100 {
            let (x, y) = (b - g * (b - a), a + g * (b - a));
            if d(x) < d(y) {
                b = y;
            } else {
                a = x;
            }
        }
        let t = 0.5 * (a + b);
        let ends = [lo, hi].map(|s| c.point(s).map_or(0.0, |p| p.dist(pole)));
        Ok((d(t) <= reach && ends.iter().all(|x| *x > reach) && t > lo && t < hi).then_some(t))
    }

    /// Turns (in u, in v) a loop makes around a periodic surface. Points on one of the surface's
    /// `poles` are passed over: every angle round the axis is the pole, so the parameter found
    /// there is any, and a loop running in to the pole and out again turned as the points either
    /// side of it say (a cap split along a meridian, one half's loop from the rim to the pole and
    /// back, does not wind round the axis).
    ///
    /// Each point is taken on the surface by [`Turns::step`], from two candidates: the point found
    /// from the last point's parameters and the one found by searching the whole surface. A
    /// search from the last point alone can settle on another part of the surface, near the
    /// point but not under it (a fit-point spline's wiggles millimetres away), and a loop with its
    /// seam would count a turn it does not make.
    fn winding(&self, eval: &SurfaceEval, es: &[(u32, bool)], periods: [Option<f64>; 2], poles: &[Vec3]) -> R<[i64; 2]> {
        let mut turns = Turns::new(periods);
        let starts = Starts::default();
        // Whether the loop's start was taken both as its first sample and as its last.
        let mut closed = true;
        for (i, &(e, forward)) in es.iter().enumerate() {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            let reach = self.tol.max(edge.tolerance);
            for k in 0..=SAMPLES {
                let end = (i == 0 && k == 0) || (i + 1 == es.len() && k == SAMPLES);
                let s = k as f64 / SAMPLES as f64;
                let t = if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s };
                let p = c.point(t).ok_or("edge point")?;
                if poles.iter().any(|q| p.dist(*q) <= reach) {
                    closed &= !end;
                    continue;
                }
                let off = |q: Option<[f64; 2]>| q.and_then(|q| eval.point(q).map(|s| (q, s.dist(p))));
                let from_last = off(eval.project(p, turns.last, f64::INFINITY));
                let searched = off(eval.project_with(&starts, p, None, f64::INFINITY));
                let taken = turns.step(&[from_last, searched].into_iter().flatten().collect::<Vec<_>>(), reach);
                closed &= taken || !end;
            }
        }
        if closed {
            turns.close();
        }
        Ok(turns.count())
    }

    /// A vertex of the loop is on one of the axis's poles (within its tolerance).
    fn through_pole(&self, es: &[(u32, bool)], ax: &Axis) -> bool {
        es.iter().any(|&u| {
            self.start(u)
                .ok()
                .and_then(|v| self.vertices.get(v as usize))
                .is_some_and(|x| ax.poles.iter().flatten().any(|p| Vec3::from(x.point).dist(*p) <= self.tol.max(x.tolerance)))
        })
    }

    /// The vertex where use `k` of a loop starts.
    fn start(&self, (e, forward): (u32, bool)) -> R<u32> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        Ok(if forward { edge.vertices[0] } else { edge.vertices[1] })
    }

    fn vpoint(&self, v: u32) -> R<Vec3> {
        self.vertices.get(v as usize).map(|v| Vec3::from(v.point)).ok_or_else(|| "vertex reference".to_string())
    }

    /// Edge loop `li` of face `fi`, rotated to start at its use `k`.
    fn rotated(&self, fi: usize, li: usize, k: usize) -> R<Vec<(u32, bool)>> {
        let Some(Bound::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        Ok(es.iter().skip(k).chain(es.iter().take(k)).copied().collect())
    }

    /// A seam edge from vertex `a` to vertex `b` on the surface: a ruling line, a meridian arc
    /// of a sphere, or a revolution's profile turned to their angle. Returns the edge and
    /// whether it runs from `a` to `b`. With `pole`, `b` is the surface's pole there (a vertex
    /// within its tolerance of it): the seam ends on the pole itself, where every ruling or
    /// meridian meets, and [`Self::new_edge`] raises `b`'s tolerance to cover its gap. A seam to
    /// the point as written, a hair off the pole, would leave the surface there and swing round
    /// the axis as it neared it.
    fn seam_edge(&mut self, a: u32, b: u32, ax: &Axis, eval: &SurfaceEval, check: bool, pole: Option<Vec3>) -> R<(u32, bool)> {
        let pa = self.vpoint(a)?;
        let pb = match pole {
            Some(p) => p,
            None => self.vpoint(b)?,
        };
        if pa.dist(pb) <= self.tol {
            return Err("a seam of no length".into());
        }
        let (curve, interval, along) = match &ax.kind {
            Kind::Ruled => (BCurve::Line { start: arr(pa), end: arr(pb) }, [0.0, 1.0], true),
            Kind::Sphere(r) => {
                // The sphere's own meridian through the end further from the axis: angle 0 on the
                // equator, π/2 at the upper pole, so its angle is the surface's latitude and its
                // quarters the surface's. Its frame is unit and square to rounding, as the
                // record's check asks ([`crate::kernel::square_frame`]): one projection off the
                // axis leaves along it the rounding of the whole offset, which a short remainder
                // (an end near a pole) makes many ulps of its direction.
                let (da, db) = (pa - ax.origin, pb - ax.origin);
                let (ra, rb) = (da - ax.z * da.dot(ax.z), db - ax.z * db.dot(ax.z));
                let (z, x) = crate::kernel::square_frame(ax.z, if ra.len() >= rb.len() { da } else { db }).ok_or("a seam along the sphere's axis")?;
                let latitude = |d: Vec3| d.dot(z).atan2(d.dot(x));
                let (ta, tb) = (latitude(da), latitude(db));
                if !(ta.abs() <= 0.5 * PI + 1e-9 && tb.abs() <= 0.5 * PI + 1e-9) {
                    return Err(unsupported("a seam between loops at opposite angles"));
                }
                let along = ta < tb;
                (
                    BCurve::Circle { frame: Frame { origin: arr(ax.origin), x_axis: arr(x), y_axis: arr(z) }, radius: *r },
                    if along { [ta, tb] } else { [tb, ta] },
                    along,
                )
            }
            Kind::Torus { major, minor } => {
                // The tube circle at `a`'s angle about the axis: centred on the ring there, angle
                // 0 outward and π/2 up the axis, so its angle is the surface's tube angle. The
                // seam runs from the loop the face lies above (`a`'s, running +u) up the tube to
                // `b`: the face's side of the circle, whichever half that is. Its frame is
                // unit and square to rounding, as the sphere's meridian's.
                let da = pa - ax.origin;
                let (z, x) = crate::kernel::square_frame(ax.z, da).ok_or("a torus seam on its axis")?;
                let centre = ax.origin + x * *major;
                let tube = |p: Vec3| {
                    let d = p - centre;
                    d.dot(z).atan2(d.dot(x))
                };
                let (ta, tb) = (tube(pa), tube(pb));
                let up = (tb - ta).rem_euclid(TAU);
                if !(up > 1e-9) {
                    return Err("a seam of no length".into());
                }
                (BCurve::Circle { frame: Frame { origin: arr(centre), x_axis: arr(x), y_axis: arr(z) }, radius: *minor }, [ta, ta + up], true)
            }
            Kind::TorusTube => {
                // The circle about the axis through `a`: angle 0 at `a`, growing with the
                // torus's u. The face lies to the left of its loops in (u, v), so from the loop
                // running round the tube up (+v) it lies towards −u: the seam runs from `a` back
                // round the axis to `b`, the edge from `b` to `a`.
                let da = pa - ax.origin;
                let (z, x) = crate::kernel::square_frame(ax.z, da).ok_or("a torus seam on its axis")?;
                let h = da.dot(z);
                let r = (da - z * h).len();
                let y = z.cross(x);
                let db = pb - ax.origin;
                let back = (-(db.dot(y).atan2(db.dot(x)))).rem_euclid(TAU);
                if !(back > 1e-9) {
                    return Err("a seam of no length".into());
                }
                let centre = ax.origin + z * h;
                (BCurve::Circle { frame: Frame { origin: arr(centre), x_axis: arr(x), y_axis: arr(y) }, radius: r }, [-back, 0.0], false)
            }
            Kind::Iso { line, dir, .. } => {
                // Each vertex's parameter along the line, the one the surface is not closed in
                // (a pole's from its end of that range).
                let across = 1 - dir;
                let Some((v0, v1)) = eval.domain().range.get(across).copied().flatten() else {
                    return Err("a closed spline with no range across it".into());
                };
                let v_of = |p: Vec3| -> R<f64> {
                    Ok(match ax.poles {
                        [Some(q), _] if q.dist(p) <= self.tol => v0,
                        [_, Some(q)] if q.dist(p) <= self.tol => v1,
                        _ => eval.project(p, None, f64::INFINITY).and_then(|q| q.get(across).copied()).ok_or("a seam vertex off its surface")?,
                    })
                };
                let (va, vb) = (v_of(pa)?, v_of(pb)?);
                let along = va < vb;
                (line.clone(), if along { [va, vb] } else { [vb, va] }, along)
            }
            Kind::Revolved(profile) => {
                // a's angle; each vertex's profile parameter (a pole's from the profile's end).
                let qa = eval.project(pa, None, f64::INFINITY).ok_or("a seam vertex off its surface")?;
                let qb = eval.project(pb, Some(qa), f64::INFINITY).ok_or("a seam vertex off its surface")?;
                let solidcore_record::brep::Natural::Range(lo, hi) = profile.natural() else { return Err("a periodic profile".into()) };
                let vb = match ax.poles {
                    [Some(p), _] if p.dist(pb) <= self.tol => lo,
                    [_, Some(p)] if p.dist(pb) <= self.tol => hi,
                    _ => qb[1],
                };
                let curve = meridian(profile, ax.origin, ax.z, qa[0])?;
                let along = qa[1] < vb;
                (curve, if along { [qa[1], vb] } else { [vb, qa[1]] }, along)
            }
        };
        if !(interval[0] < interval[1]) {
            return Err("a degenerate seam".into());
        }
        // On the surface: its middle within the tolerance (a ruling, a meridian).
        let c = CurveEval::new(&curve).map_err(|m| m.to_string())?;
        if check {
            let m = c.point(0.5 * (interval[0] + interval[1])).ok_or("seam point")?;
            let gap = |p: Vec3| eval.project(p, None, f64::INFINITY).and_then(|q| eval.point(q)).map_or(f64::INFINITY, |s| s.dist(p));
            // As far off as its ends are, at most: a vertex on an edge the file has a little off
            // the surface (a spline rim 0.3 um off its cone, in a file stating 0.01 um) puts the
            // seam's middle off by no more, and the seam's tolerance covers it as any edge's does
            // (`measure_edges`). Vertices at different angles leave the chord between them far
            // more than that inside the surface.
            let allowed = self.tol.max(gap(pa).max(gap(pb)).min(super::MAX_TOLERANCE));
            if !(gap(m) <= allowed) {
                return Err(unsupported("a seam off its surface: loops with no vertices at one angle"));
            }
        }
        let i = self.new_edge(curve, interval, if along { [a, b] } else { [b, a] })?;
        Ok((i, along))
    }

    /// A new edge on `curve` over `interval` between `vertices`, each vertex's tolerance raised to
    /// cover its gap to the curve's end.
    fn new_edge(&mut self, curve: BCurve, interval: [f64; 2], vertices: [u32; 2]) -> R<u32> {
        let c = CurveEval::new(&curve).map_err(|m| m.to_string())?;
        for (t, v) in interval.into_iter().zip(vertices) {
            let gap = c.point(t).ok_or("seam point")?.dist(self.vpoint(v)?);
            if let Some(x) = self.vertices.get_mut(v as usize) {
                x.tolerance = x.tolerance.max(1.5 * gap);
            }
        }
        let i = index(self.edges.len())?;
        self.edges.push(EdgeB {
            curve,
            interval,
            vertices,
            tolerance: self.tol,
            pcurve_tolerance: self.pcurve_tolerance(),
            form: None,
            origin: None,
        });
        Ok(i)
    }

    /// Leave out of each face its bounds that are slits (a loop out along edges and straight back,
    /// of no (u, v) area) when the face has a bound that is not one, and no other face uses the
    /// slit's edges: the slit lies inside the face and takes nothing from it, but the portable
    /// B-rep refuses a loop of no area, and with it the face and the solid. Returns how many
    /// bounds were left out.
    pub(crate) fn drop_inner_slits(&mut self) -> usize {
        let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for face in &self.faces {
            for l in &face.loops {
                if let Bound::Edges(es) = l {
                    for (e, _) in es {
                        *uses.entry(*e).or_default() += 1;
                    }
                }
            }
        }
        let alone = |l: &Bound| match l {
            Bound::Edges(es) => es.len() >= 2 && slit_path(es).is_some() && es.iter().all(|(e, _)| uses.get(e).copied() == Some(2)),
            Bound::Vertex(_) => false,
        };
        let mut dropped = 0;
        for face in &mut self.faces {
            let slit: Vec<bool> = face.loops.iter().enumerate().map(|(i, l)| face.outer != Some(i) && alone(l)).collect();
            let count = slit.iter().filter(|s| **s).count();
            // A face of slits alone is a whole closed surface ([`Assembly::complete_slits`]).
            if count == 0 || count == face.loops.len() {
                continue;
            }
            // The marked outer bound keeps its place among those that stay.
            face.outer = face.outer.map(|o| o - slit.iter().take(o).filter(|s| **s).count());
            let mut at = 0;
            face.loops.retain(|_| {
                at += 1;
                !slit.get(at - 1).copied().unwrap_or(false)
            });
            dropped += count;
        }
        dropped
    }

    /// Complete every face bounded by a slit alone into the whole of its surface. A slit is a
    /// loop that runs out along a chain of edges and straight back ([`slit_path`]): it encloses
    /// no (u, v) area, yet on a closed surface it bounds all of it less a cut of none (a ball
    /// whose one face is bounded by part of a meridian, out and back). Schema 4 keeps that face
    /// as the surface's whole (u, v) domain, closed by its seams, so the slit is made one: on a
    /// surface of revolution closed at both poles (a sphere, a revolution whose profile ends on
    /// its axis, a spline closed round u that collapses at both v ends), the meridian the slit
    /// lies along is run on from its ends to the poles, out and back; on a torus, the circle the
    /// slit lies along (round the tube or round the axis) is closed and the other circle through
    /// its start is the second seam. Returns how many faces were completed.
    pub(crate) fn complete_slits(&mut self) -> R<usize> {
        let mut completed = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let [Bound::Edges(es)] = face.loops.as_slice() else { continue };
            let Some(path) = slit_path(es) else { continue };
            let (step, surface) = (face.origin, face.surface.clone());
            let (merged, seated) = match &surface {
                BSurface::Torus { frame, major_radius, .. } => (self.torus_slit(&path, frame, *major_radius)?, None),
                _ => match self.closed_at_poles(&path, &surface)? {
                    // An open surface: the slit bounds nothing, and is refused for that later.
                    None => continue,
                    Some(Some(m)) => (Some(m), None),
                    // A sphere's frame is the writer's choice (any right-handed frame about its
                    // centre is the same surface, its natural normal outward): one whose
                    // meridians miss the slit has its pole put at the slit's start instead, its x
                    // axis through the slit's middle.
                    Some(None) => match &surface {
                        BSurface::Sphere { frame, radius } => match self.seated(&path, frame, *radius)? {
                            Some(s) => (self.closed_at_poles(&path, &s)?.flatten(), Some(s)),
                            None => (None, None),
                        },
                        _ => (None, None),
                    },
                },
            };
            let merged = merged.ok_or_else(|| format!("face #{step}: {}", unsupported("a face bounded by a slit off its surface's seam lines")))?;
            let Some(face) = self.faces.get_mut(fi) else { continue };
            if let Some(s) = seated {
                face.surface = s;
                face.extent = None;
            }
            if let Some(Bound::Edges(es)) = face.loops.first_mut() {
                *es = merged;
            }
            completed += 1;
        }
        Ok(completed)
    }

    /// Complete every face on a torus that is the outside of its loops into the torus less what
    /// they enclose (#17930). A loop runs counter-clockwise about the face's normal round the
    /// region it bounds; on a torus, which is closed both ways, a loop that winds round neither
    /// way (it encloses a patch) and runs clockwise bounds all the rest of the torus. A full
    /// torus written as two faces sharing one loop, a patch and the rest round it, has the
    /// rest so: read as the patch turned over, the solid came in with both faces on the patch
    /// and no volume. Turning such a face over is still right where the file has its loop the
    /// wrong way round, so a face is completed only where a face it shares an edge with lies on
    /// a torus facing the same way: then the patch is that face's, and this one is the rest. That
    /// torus may be another: two rings joined into one body are each the torus less the disc of
    /// it inside the other (#20550). The face gets the torus's two seams (a circle round the axis and one round the
    /// tube, through a new vertex, in the middle of the widest stretches of longitude and tube
    /// angle its loops keep clear of, where its torus is turned to start) as its outer bound,
    /// and its loops become its holes. A face on a sphere is completed the same way
    /// ([`Self::complete_sphere_outside`]). Returns how many faces were completed.
    pub(crate) fn complete_outsides(&mut self) -> R<usize> {
        let mut completed = 0;
        // The faces each edge bounds, so a face's neighbours are found without a pass over all.
        let mut users: HashMap<u32, Vec<usize>> = HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for l in &f.loops {
                if let Bound::Edges(es) = l {
                    for (e, _) in es {
                        users.entry(*e).or_default().push(fi);
                    }
                }
            }
        }
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            if let BSurface::Sphere { frame, radius } = face.surface.clone() {
                if self.complete_sphere_outside(fi, &frame, radius, &users)? {
                    completed += 1;
                }
                continue;
            }
            let BSurface::Torus { frame, major_radius, minor_radius, tube_start } = face.surface.clone() else { continue };
            let loops: Vec<Vec<(u32, bool)>> = face
                .loops
                .iter()
                .map(|l| match l {
                    Bound::Edges(es) if !es.is_empty() && !super::seam_only(es) => Some(es.clone()),
                    _ => None,
                })
                .collect::<Option<_>>()
                .unwrap_or_default();
            if loops.is_empty() {
                continue;
            }
            let o = Vec3::from(frame.origin);
            let (x, y) = (Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
            let z = x.cross(y);
            // Each loop's samples as (longitude, tube angle) about the torus, unwrapped along it.
            let angles = |p: Vec3| -> [f64; 2] {
                let d = p - o;
                let h = d.dot(z);
                let rho = (d - z * h).len();
                [d.dot(y).atan2(d.dot(x)), h.atan2(rho - major_radius)]
            };
            let mut samples: Vec<[f64; 2]> = Vec::new();
            let mut outside = true;
            for l in &loops {
                let mut path: Vec<[f64; 2]> = Vec::new();
                for &(e, forward) in l {
                    let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                    let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                    let [lo, hi] = edge.interval;
                    for k in 0..SAMPLES {
                        let s = k as f64 / SAMPLES as f64;
                        let t = if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s };
                        let q = angles(c.point(t).ok_or("edge point")?);
                        samples.push(q);
                        let q = match path.last() {
                            Some(l) => [l[0] + wrap(q[0] - l[0]), l[1] + wrap(q[1] - l[1])],
                            None => q,
                        };
                        path.push(q);
                    }
                }
                let (Some(first), Some(last)) = (path.first(), path.last()) else { continue };
                // Back to the start: a loop that winds round the torus doesn't enclose a patch.
                let back = [last[0] + wrap(first[0] - last[0]), last[1] + wrap(first[1] - last[1])];
                if (back[0] - first[0]).abs() > PI || (back[1] - first[1]).abs() > PI {
                    outside = false;
                    break;
                }
                // Twice the signed area in (longitude, tube angle): the torus's natural normal is
                // ∂u × ∂v, out of the tube, and the loops are kept about it.
                let mut twice = 0.0;
                for (a, b) in path.iter().zip(path.iter().skip(1).chain(std::iter::once(&back))) {
                    twice += a[0] * b[1] - b[0] * a[1];
                }
                if !(twice < 0.0) {
                    outside = false;
                    break;
                }
            }
            // The patch is a face of a torus facing the same way: this torus, or another (a union
            // of two rings is the torus of each less the part inside the other, so each face's
            // loop bounds a patch of the other's torus). Not a sphere: a sphere's face is completed
            // only beside a sphere, so a torus beside a sphere would be completed alone and the
            // pair would close as the torus less the sphere; such a pair stays refused.
            let same = |s: &BSurface| matches!(s, BSurface::Torus { .. });
            if !outside || !self.patch_beside(fi, &loops, &users, same) {
                continue;
            }
            // The seams go through the middle of the widest stretch of longitude and of tube
            // angle the loops leave free (each clear of them by more than the tolerance), and
            // the torus is turned to start there, as a whole torus starts at its seams.
            let (Some(u), Some(w)) =
                (widest_free(&samples, 0, major_radius - minor_radius, self.tol), widest_free(&samples, 1, minor_radius, self.tol))
            else {
                continue;
            };
            let Some(turned) = crate::kernel::form::turned(&face.surface, u).and_then(|s| crate::kernel::form::tube_turned(&s, w - tube_start))
            else {
                continue;
            };
            let bound = self.torus_seams(&frame, major_radius, minor_radius, u, w)?;
            let Some(face) = self.faces.get_mut(fi) else { continue };
            face.surface = turned;
            face.loops.insert(0, bound);
            face.outer = Some(0);
            face.extent = None;
            completed += 1;
        }
        Ok(completed)
    }

    /// The two full-circle seams bound one period in each direction, about the natural normal.
    pub(crate) fn torus_seams(&mut self, frame: &Frame, major_radius: f64, minor_radius: f64, u: f64, w: f64) -> R<Bound> {
        let o = Vec3::from(frame.origin);
        let (x, y) = (Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let z = x.cross(y);
        let radial = x * u.cos() + y * u.sin();
        let ring_radius = major_radius + minor_radius * w.cos();
        let p = o + radial * ring_radius + z * (minor_radius * w.sin());
        let v = index(self.vertices.len())?;
        self.vertices.push(Vertex { point: arr(p), tolerance: self.tol });
        // Round the axis the way u runs, round the tube the way v runs: the torus's whole
        // (u, v) box, counter-clockwise.
        let ring = BCurve::Circle {
            frame: Frame { origin: arr(o + z * (minor_radius * w.sin())), x_axis: arr(radial), y_axis: arr(z.cross(radial)) },
            radius: ring_radius,
        };
        let tube_x = radial * w.cos() + z * w.sin();
        let tube_y = z * w.cos() - radial * w.sin();
        let tube = BCurve::Circle {
            frame: Frame { origin: arr(o + radial * major_radius), x_axis: arr(tube_x), y_axis: arr(tube_y) },
            radius: minor_radius,
        };
        let ring = self.new_edge(ring, [0.0, TAU], [v, v])?;
        let tube = self.new_edge(tube, [0.0, TAU], [v, v])?;
        Ok(Bound::Edges(vec![(ring, true), (tube, true), (ring, false), (tube, false)]))
    }

    /// Turn each torus face whose bounds hold a seam round the axis (a full circle about it,
    /// used both ways) off its tube start so the tube starts there, and one whose bounds hold a
    /// seam round the tube off its frame's x axis so the frame starts there: a face's (u, v) box
    /// on its form is read from its seams, and a seam away from where the torus starts in that
    /// direction gave a box short of the whole torus, so a torus written whole round a hole
    /// read back with part of it missing. STEP's torus starts its tube at the outer equator, so
    /// a file's seam round the axis elsewhere is common. Returns how many faces were turned.
    pub(crate) fn seat_torus_seams(&mut self) -> R<usize> {
        let mut seated = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let BSurface::Torus { frame, major_radius, tube_start, .. } = &face.surface else { continue };
            let (major_radius, tube_start) = (*major_radius, *tube_start);
            let o = Vec3::from(frame.origin);
            let (x, y) = (Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
            let z = x.cross(y);
            let mut ways: std::collections::HashMap<u32, [bool; 2]> = std::collections::HashMap::new();
            for l in &face.loops {
                if let Bound::Edges(es) = l {
                    for (e, forward) in es {
                        if let Some(w) = ways.entry(*e).or_default().get_mut(usize::from(*forward)) {
                            *w = true;
                        }
                    }
                }
            }
            let (mut u_at, mut w_at) = (None, None);
            for (e, used) in &ways {
                if *used != [true, true] {
                    continue;
                }
                let Some(edge) = self.edges.get(*e as usize) else { continue };
                let BCurve::Circle { frame: cf, radius } = &edge.curve else { continue };
                if !((edge.interval[1] - edge.interval[0]) >= TAU * (1.0 - 1e-9)) {
                    continue;
                }
                let (c, cx) = (Vec3::from(cf.origin), Vec3::from(cf.x_axis));
                let n = cx.cross(Vec3::from(cf.y_axis));
                let p = c + cx * *radius;
                let d = p - o;
                if n.cross(z).len() <= 1e-9 {
                    // Round the axis: its tube angle.
                    let h = d.dot(z);
                    w_at = Some(h.atan2((d - z * h).len() - major_radius));
                } else if n.dot(z).abs() <= 1e-9 {
                    // Round the tube: its longitude.
                    let dc = c - o;
                    u_at = Some(dc.dot(y).atan2(dc.dot(x)));
                }
            }
            let mut surface = face.surface.clone();
            let mut turned = false;
            if let Some(u) = u_at.filter(|u| wrap(*u).abs() > 1e-12)
                && let Some(s) = crate::kernel::form::turned(&surface, u)
            {
                surface = s;
                turned = true;
            }
            if let Some(w) = w_at.filter(|w| wrap(*w - tube_start).abs() > 1e-12)
                && let Some(s) = crate::kernel::form::tube_turned(&surface, wrap(w - tube_start))
            {
                surface = s;
                turned = true;
            }
            if turned && let Some(f) = self.faces.get_mut(fi) {
                f.surface = surface;
                f.extent = None;
                seated += 1;
            }
        }
        Ok(seated)
    }

    /// A face sharing an edge with one of `loops` (of face `fi`; `users` the faces each edge
    /// bounds) lies on a surface `same` takes for face `fi`'s, and faces the same way as face `fi`.
    fn patch_beside(&self, fi: usize, loops: &[Vec<(u32, bool)>], users: &HashMap<u32, Vec<usize>>, same: impl Fn(&BSurface) -> bool) -> bool {
        let Some(forward) = self.faces.get(fi).map(|f| f.forward) else { return false };
        loops
            .iter()
            .flatten()
            .filter_map(|u| users.get(&u.0))
            .flatten()
            .any(|&gi| gi != fi && self.faces.get(gi).is_some_and(|g| g.forward == forward && same(&g.surface)))
    }

    /// Complete face `fi`, on the sphere about `frame` of `radius`, when it is the outside of its
    /// loop, into the sphere less what the loop encloses (#15046), as [`Self::complete_outsides`]
    /// does a torus face. A loop that winds round neither the axis nor a pole encloses a patch,
    /// and run clockwise about the sphere's outward normal it bounds all the rest of the sphere.
    /// A ball written as two faces sharing one loop, a cap and the rest round it, has the rest
    /// so, and read as the cap turned over it came in with both faces on the cap and no mass.
    /// As on a torus, the face is completed only where a face it shares an edge with lies on the
    /// same sphere facing the same way. Its sphere is turned (any frame about its centre is the
    /// same surface) to put the upper pole inside the patch, towards the loop's mean point, and
    /// its x axis through the loop's first vertex: the loop then runs once round the axis the
    /// way u falls, so the face covers the lower pole, and gets its seam to it as a cap bounded
    /// by one circle does ([`Self::add_seams`]). Returns whether it was completed.
    fn complete_sphere_outside(&mut self, fi: usize, frame: &Frame, radius: f64, users: &HashMap<u32, Vec<usize>>) -> R<bool> {
        let Some(face) = self.faces.get(fi) else { return Ok(false) };
        let [Bound::Edges(l)] = face.loops.as_slice() else { return Ok(false) };
        if l.is_empty() || super::seam_only(l) {
            return Ok(false);
        }
        let l = l.clone();
        let o = Vec3::from(frame.origin);
        let (x, y) = (Vec3::from(frame.x_axis), Vec3::from(frame.y_axis));
        let z = x.cross(y);
        // The loop's samples: their points, and as (longitude, latitude) on the sphere, the
        // longitude unwrapped along it.
        let (mut points, mut path): (Vec<Vec3>, Vec<[f64; 2]>) = (Vec::new(), Vec::new());
        for &(e, forward) in &l {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            for k in 0..SAMPLES {
                let s = k as f64 / SAMPLES as f64;
                let t = if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s };
                let p = c.point(t).ok_or("edge point")?;
                points.push(p);
                let d = p - o;
                let h = d.dot(z);
                let q = [d.dot(y).atan2(d.dot(x)), h.atan2((d - z * h).len())];
                let q = match path.last() {
                    Some(l) => [l[0] + wrap(q[0] - l[0]), q[1]],
                    None => q,
                };
                path.push(q);
            }
        }
        let (Some(first), Some(last)) = (path.first(), path.last()) else { return Ok(false) };
        // Back to the start: a loop that winds round the axis encloses a pole, not a patch.
        let back = [last[0] + wrap(first[0] - last[0]), first[1]];
        if (back[0] - first[0]).abs() > PI {
            return Ok(false);
        }
        // Twice the signed area in (longitude, latitude): the sphere's natural normal is
        // ∂u × ∂v, outward, and the loop is kept about it.
        let mut twice = 0.0;
        for (a, b) in path.iter().zip(path.iter().skip(1).chain(std::iter::once(&back))) {
            twice += a[0] * b[1] - b[0] * a[1];
        }
        let tol = self.tol;
        let same =
            |s: &BSurface| matches!(s, BSurface::Sphere { frame: f, radius: r } if Vec3::from(f.origin).dist(o) <= tol && (r - radius).abs() <= tol);
        if !(twice < 0.0) || !self.patch_beside(fi, std::slice::from_ref(&l), users, same) {
            return Ok(false);
        }
        // The new axis, towards the loop's mean point, and the x axis through its first vertex.
        let mean = points.iter().fold(Vec3::ZERO, |s, p| s + (*p - o)) * (1.0 / points.len() as f64);
        let Some(up) = mean.normalized() else { return Ok(false) };
        let start = self.vpoint(self.start(*l.first().ok_or("empty loop")?)?)? - o;
        let Some(ex) = (start - up * start.dot(up)).normalized() else { return Ok(false) };
        let ey = up.cross(ex);
        // About it, the loop runs once round the way u falls (whole turns, as [`Turns`] counts
        // them): else the patch is not one this turn makes a cap of. One through a pole is
        // `add_seams`'s to close there.
        let mut turned = 0.0;
        let mut prev: Option<f64> = None;
        for p in points.iter().chain(points.first()) {
            let d = *p - o;
            let a = d.dot(ey).atan2(d.dot(ex));
            if let Some(b) = prev {
                turned += wrap(a - b);
            }
            prev = Some(a);
        }
        if (turned / TAU).round() as i64 != -1 {
            return Ok(false);
        }
        let seated = BSurface::Sphere { frame: Frame { origin: frame.origin, x_axis: arr(ex), y_axis: arr(ey) }, radius };
        let Some(face) = self.faces.get_mut(fi) else { return Ok(false) };
        face.surface = seated;
        face.extent = None;
        Ok(true)
    }

    /// The slit made the meridian seam of `surface` when it is a surface of revolution closed
    /// at both poles ([`Assembly::meridian_slit`]); `None` for any other surface, or for a closed
    /// meridian slit on an analytic/revolved horn torus (coincident poles) left for
    /// [`Assembly::split_at_poles`] and [`Assembly::reduce_pole_closed_seams`].
    fn closed_at_poles(&mut self, path: &[(u32, bool)], surface: &BSurface) -> R<Option<Option<Vec<(u32, bool)>>>> {
        let eval = SurfaceEval::new(surface).map_err(|e| e.to_string())?;
        match axis(surface).or_else(|| spline_axis(surface, &eval)) {
            // Meridians run in v: a spline closed in v has none.
            Some(ax) if ax.kind.winds_in() == 0 && ax.poles.iter().all(Option::is_some) => {
                if !matches!(ax.kind, Kind::Iso { .. })
                    && let [Some(south), Some(north)] = ax.poles
                    && south.dist(north) <= self.tol
                    && let (Some(&first), Some(&last)) = (path.first(), path.last())
                    && self.start(first)? == self.end(last)?
                    && self.on_meridian(path, &ax, &eval)?
                {
                    return Ok(None);
                }
                Ok(Some(self.meridian_slit(path, &ax, &eval)?))
            }
            _ => Ok(None),
        }
    }

    /// Normalize seam-only loops on pole-closed surfaces of revolution (notably full horn tori,
    /// where a STEP `TOROIDAL_SURFACE` with `R = r` carries both a meridian seam and an interior
    /// parallel seam such as the outer equator). On the `PlacedRevolution` form `v` runs from
    /// pole to pole, so an interior parallel circle is a redundant cut inside `(v_min, v_max)`
    /// that pushes lifted pcurves outside the form's knot range. Dropping interior parallel seam
    /// edges used solely by that loop, ordering the remaining meridian edges from the lower pole
    /// to the upper and back, and aligning `u = 0` with the meridian gives a clean pole-to-pole
    /// seam loop covering the whole surface without an internal pole. Returns how many loops were
    /// normalized.
    pub(crate) fn reduce_pole_closed_seams(&mut self) -> R<usize> {
        let mut edge_uses: HashMap<u32, usize> = HashMap::new();
        for face in &self.faces {
            for l in &face.loops {
                if let Bound::Edges(es) = l {
                    for &(e, _) in es {
                        *edge_uses.entry(e).or_default() += 1;
                    }
                }
            }
        }
        let mut reduced = 0;
        for fi in 0..self.faces.len() {
            let Some(surface) = self.faces.get(fi).map(|f| f.surface.clone()) else { continue };
            let BSurface::PlacedRevolution { profile, origin, axis: rev_axis, matrix, forward } = &surface else { continue };
            let Some(ax) = axis(&surface) else { continue };
            let [Some(south), Some(north)] = ax.poles else { continue };
            let eval = SurfaceEval::new(&surface).map_err(|e| e.to_string())?;
            let Some((v_min, v_max)) = eval.domain().range.get(1).copied().flatten().filter(|(a, b)| a < b) else {
                continue;
            };
            let pv = eval.domain().period.get(1).copied().flatten().filter(|&p| p > 0.0);
            let wrap_v = |v: f64| pv.map_or(v, |p| v_min + (v - v_min).rem_euclid(p));
            let loop_count = self.faces.get(fi).map_or(0, |f| f.loops.len());
            for li in 0..loop_count {
                let Some(Bound::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else {
                    continue;
                };
                if !super::seam_only(es) || !es.iter().all(|(e, _)| edge_uses.get(e).copied() == Some(2)) {
                    continue;
                }
                let (mut unique, mut meridians, mut has_parallel, mut valid) = (Vec::new(), Vec::new(), false, true);
                for &(e, _) in es {
                    if !unique.contains(&e) {
                        unique.push(e);
                    }
                }
                for &e in &unique {
                    match self.classify_pole_closed_seam_edge(e, &ax, &eval, (v_min, v_max))? {
                        SeamEdgeKind::Meridian { u, radial2, forward, v_mid } => meridians.push((e, forward, v_mid, u, radial2)),
                        SeamEdgeKind::Parallel => has_parallel = true,
                        SeamEdgeKind::Other => {
                            valid = false;
                            break;
                        }
                    }
                }
                let Some(&(_, _, _, u_meridian, _)) = valid.then(|| meridians.iter().max_by(|a, b| a.4.total_cmp(&b.4))).flatten() else {
                    continue;
                };
                meridians.sort_by(|a, b| a.2.total_cmp(&b.2));
                let up: Vec<(u32, bool)> = meridians.iter().map(|&(e, fwd, _, _, _)| (e, fwd)).collect();
                let (Some(&first), Some(&last)) = (up.first(), up.last()) else { continue };
                if !self.on_meridian_at(&up, &ax, &eval, u_meridian)? || !self.at(self.start(first)?, south)? || !self.at(self.end(last)?, north)? {
                    continue;
                }
                let mut chained = true;
                for pair in up.windows(2) {
                    if let [a, b] = pair {
                        let mid = self.end(*a)?;
                        if mid != self.start(*b)? || self.at(mid, south)? || self.at(mid, north)? {
                            chained = false;
                            break;
                        }
                    }
                }
                if !chained {
                    continue;
                }
                let needs_turn = wrap(u_meridian).abs() > 1e-12;
                let mc = if needs_turn {
                    let Ok(r) = meridian(profile, ax.origin, ax.z, u_meridian) else { continue };
                    r
                } else {
                    (**profile).clone()
                };
                let Ok(mc_eval) = CurveEval::new(&mc) else { continue };
                // Re-parameterize the private meridian seam edges onto the revolution's own
                // profile so off-quarter-grid STEP curves lift to an exact linear pcurve.
                let (mut segs, mut prev) = (Vec::new(), v_min);
                for (idx, &(e, fwd)) in up.iter().enumerate() {
                    let (va, vb) = (self.start((e, fwd))?, self.end((e, fwd))?);
                    let next = if idx + 1 == up.len() {
                        Some(v_max)
                    } else {
                        eval.project(self.vpoint(vb)?, None, f64::INFINITY).map(|q| wrap_v(q[1])).filter(|&t| t > prev && t < v_max)
                    };
                    let Some(tb) = next.filter(|&t| t > prev) else {
                        segs.clear();
                        break;
                    };
                    let (mut gaps, mut ends_ok) = ([0.0; 2], true);
                    for ((slot, t), v) in gaps.iter_mut().zip([prev, tb]).zip([va, vb]) {
                        let Some(pt) = mc_eval.point(t) else {
                            ends_ok = false;
                            break;
                        };
                        let gap = pt.dist(self.vpoint(v)?);
                        if !(super::VERTEX_GAP_MARGIN * gap <= super::MAX_TOLERANCE) {
                            ends_ok = false;
                            break;
                        }
                        *slot = gap;
                    }
                    if !ends_ok {
                        segs.clear();
                        break;
                    }
                    segs.push((e, [va, vb], [prev, tb], gaps));
                    prev = tb;
                }
                if segs.len() != up.len() {
                    continue;
                }
                let up_oriented: Vec<(u32, bool)> = up.iter().map(|&(e, _)| (e, true)).collect();
                let mut down = up_oriented.clone();
                super::reverse(&mut down);
                let mut merged = down.clone();
                merged.extend(&up_oriented);
                let mut up_first = up_oriented;
                up_first.extend(down);
                let edges_changed = segs.iter().any(|&(e, vs, iv, _)| {
                    self.edges.get(e as usize).is_none_or(|edge| edge.curve != mc || edge.interval != iv || edge.vertices != vs)
                });
                if !has_parallel && !needs_turn && !edges_changed && (*es == merged || *es == up_first) {
                    continue;
                }
                for (e, vs, iv, gaps) in segs {
                    for (gap, v) in gaps.into_iter().zip(vs) {
                        if let Some(x) = self.vertices.get_mut(v as usize) {
                            x.tolerance = x.tolerance.max(super::VERTEX_GAP_MARGIN * gap);
                        }
                    }
                    if let Some(edge) = self.edges.get_mut(e as usize) {
                        (edge.curve, edge.interval, edge.vertices, edge.form) = (mc.clone(), iv, vs, None);
                    }
                }
                if let Some(face) = self.faces.get_mut(fi) {
                    if needs_turn {
                        let turned = BSurface::PlacedRevolution {
                            profile: Box::new(mc),
                            origin: *origin,
                            axis: *rev_axis,
                            matrix: *matrix,
                            forward: *forward,
                        };
                        (face.surface, face.extent) = (turned, None);
                    }
                    if let Some(Bound::Edges(slot)) = face.loops.get_mut(li) {
                        *slot = merged;
                    }
                    reduced += 1;
                }
                break;
            }
        }
        Ok(reduced)
    }

    /// Classifies an edge of a seam-only loop on a pole-closed surface as lying along a meridian
    /// (strictly monotone in `v`, recording its longitude at the sample farthest from the axis),
    /// along an interior constant-`v` parallel off both poles, or neither.
    fn classify_pole_closed_seam_edge(&self, e: u32, ax: &Axis, eval: &SurfaceEval, (v_min, v_max): (f64, f64)) -> R<SeamEdgeKind> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let ([lo, hi], reach) = (edge.interval, self.tol.max(edge.tolerance));
        let mut samples = Vec::new();
        for k in 0..=SAMPLES {
            let t = lo + (hi - lo) * k as f64 / SAMPLES as f64;
            let p = c.point(t).ok_or("edge point")?;
            let at_pole = ax.poles.iter().flatten().any(|q| q.dist(p) <= reach);
            samples.push((p, at_pole));
        }

        // First check whether all non-pole samples lie on a single meridian (reading its angle at
        // the sample farthest from the axis) and move strictly monotonically in `v`.
        let pv = eval.domain().period.get(1).copied().flatten().filter(|&p| p > 0.0);
        let wrap_v = |v: f64| pv.map_or(v, |p| v_min + (v - v_min).rem_euclid(p));
        let radial_dist2 = |p: Vec3| {
            let d = p - ax.origin;
            (d - ax.z * d.dot(ax.z)).len2()
        };
        let farthest = samples
            .iter()
            .filter(|&&(_, at_pole)| !at_pole)
            .max_by(|(a, _), (b, _)| radial_dist2(*a).total_cmp(&radial_dist2(*b)))
            .and_then(|&(p, _)| eval.project(p, None, f64::INFINITY).map(|q| (q[0], radial_dist2(p))));
        let mut vs = Vec::new();
        let mut on_meridian = farthest.is_some();
        if let Some((u, _)) = farthest {
            for &(p, at_pole) in &samples {
                if at_pole {
                    continue;
                }
                let Some(q) = eval.project(p, None, f64::INFINITY) else {
                    on_meridian = false;
                    break;
                };
                let v = wrap_v(q[1]);
                if !eval.point([u, v]).is_some_and(|m| m.dist(p) <= reach) {
                    on_meridian = false;
                    break;
                }
                vs.push(v);
            }
        }
        if let Some((u, radial2)) = farthest
            && on_meridian
            && vs.len() >= 2
        {
            let inc = vs.windows(2).all(|w| matches!(w, [a, b] if a < b));
            let dec = vs.windows(2).all(|w| matches!(w, [a, b] if a > b));
            if inc || dec {
                let v_mid = vs.get(vs.len() / 2).copied().ok_or("meridian midpoint")?;
                return Ok(SeamEdgeKind::Meridian { u, radial2, forward: inc, v_mid });
            }
        }

        // Otherwise check whether all samples lie off both poles on a single interior parallel
        // `v = v_const` strictly inside `(v_min, v_max)`.
        if samples.iter().any(|&(_, at_pole)| at_pole) {
            return Ok(SeamEdgeKind::Other);
        }
        let mut v_const: Option<f64> = None;
        for &(p, _) in &samples {
            let Some(q) = eval.project(p, None, f64::INFINITY) else {
                return Ok(SeamEdgeKind::Other);
            };
            let v = *v_const.get_or_insert(wrap_v(q[1]));
            if !(v > v_min && v < v_max) || !eval.point([q[0], v]).is_some_and(|m| m.dist(p) <= reach) {
                return Ok(SeamEdgeKind::Other);
            }
        }
        Ok(if v_const.is_some() { SeamEdgeKind::Parallel } else { SeamEdgeKind::Other })
    }

    /// The sphere about `frame`'s centre with its upper pole at the slit's start and its x axis
    /// through the middle of the slit's middle edge; `None` where those coincide.
    fn seated(&self, path: &[(u32, bool)], frame: &Frame, radius: f64) -> R<Option<BSurface>> {
        let centre = Vec3::from(frame.origin);
        let first = *path.first().ok_or("empty slit")?;
        let Some(z) = (self.vpoint(self.start(first)?)? - centre).normalized() else { return Ok(None) };
        let &(e, _) = path.get(path.len() / 2).ok_or("empty slit")?;
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let middle =
            CurveEval::new(&edge.curve).map_err(|m| m.to_string())?.point(0.5 * (edge.interval[0] + edge.interval[1])).ok_or("edge point")?;
        let off = middle - centre;
        let Some(x) = (off - z * off.dot(z)).normalized() else { return Ok(None) };
        Ok(Some(BSurface::Sphere { frame: Frame { origin: frame.origin, x_axis: arr(x), y_axis: arr(z.cross(x)) }, radius }))
    }

    /// The vertex use `u` ends at.
    fn end(&self, (e, forward): (u32, bool)) -> R<u32> {
        self.start((e, !forward))
    }

    /// Vertex `v` is at `p`, within its tolerance.
    fn at(&self, v: u32, p: Vec3) -> R<bool> {
        let x = self.vertices.get(v as usize).ok_or("vertex reference")?;
        Ok(Vec3::from(x.point).dist(p) <= self.tol.max(x.tolerance))
    }

    /// Every point of the path's edges lies on one meridian of the surface (the spline's
    /// boundary column for [`Kind::Iso`]), the poles apart.
    fn on_meridian(&self, path: &[(u32, bool)], ax: &Axis, eval: &SurfaceEval) -> R<bool> {
        let u_ref = match ax.kind {
            Kind::Iso { start, .. } => Some(start),
            _ => {
                // Choose the reference longitude at the non-pole sample farthest from the axis so
                // near-pole rounding is not magnified across the meridian.
                let mut best: Option<(f64, Vec3)> = None;
                for &(e, _) in path {
                    let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                    let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                    let ([lo, hi], reach) = (edge.interval, self.tol.max(edge.tolerance));
                    for k in 0..=SAMPLES {
                        let p = c.point(lo + (hi - lo) * k as f64 / SAMPLES as f64).ok_or("edge point")?;
                        if ax.poles.iter().flatten().any(|q| q.dist(p) <= reach) {
                            continue;
                        }
                        let d = p - ax.origin;
                        let r2 = (d - ax.z * d.dot(ax.z)).len2();
                        if best.is_none_or(|(b, _)| r2 > b) {
                            best = Some((r2, p));
                        }
                    }
                }
                best.and_then(|(_, p)| eval.project(p, None, f64::INFINITY).map(|q| q[0]))
            }
        };
        let Some(u) = u_ref else { return Ok(false) };
        self.on_meridian_at(path, ax, eval, u)
    }

    /// Every non-pole sample of `path`'s edges lies on meridian `u` within each edge's tolerance.
    fn on_meridian_at(&self, path: &[(u32, bool)], ax: &Axis, eval: &SurfaceEval, u: f64) -> R<bool> {
        for &(e, _) in path {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let ([lo, hi], reach) = (edge.interval, self.tol.max(edge.tolerance));
            for k in 0..=SAMPLES {
                let p = c.point(lo + (hi - lo) * k as f64 / SAMPLES as f64).ok_or("edge point")?;
                if ax.poles.iter().flatten().any(|q| q.dist(p) <= reach) {
                    continue;
                }
                let Some(q) = eval.project(p, None, f64::INFINITY) else { return Ok(false) };
                if !eval.point([u, q[1]]).is_some_and(|m| m.dist(p) <= reach) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// A slit along a meridian of a surface closed at both poles, run on to them: the meridian
    /// from the lower pole to the upper (the slit's path, with a seam edge from each end of it
    /// that is not at its pole), then back. `None` when the slit is off every meridian.
    fn meridian_slit(&mut self, path: &[(u32, bool)], ax: &Axis, eval: &SurfaceEval) -> R<Option<Vec<(u32, bool)>>> {
        let (Some(south), Some(north)) = (ax.poles[0], ax.poles[1]) else { return Ok(None) };
        if !self.on_meridian(path, ax, eval)? {
            return Ok(None);
        }
        // How far up the meridian a vertex is: a pole's from its end, else its v.
        let height = |s: &Self, v: u32| -> R<f64> {
            Ok(if s.at(v, south)? {
                f64::NEG_INFINITY
            } else if s.at(v, north)? {
                f64::INFINITY
            } else {
                eval.project(s.vpoint(v)?, None, f64::INFINITY).ok_or("a slit vertex off its surface")?[1]
            })
        };
        let (first, last) = (*path.first().ok_or("empty slit")?, *path.last().ok_or("empty slit")?);
        let (ha, hb) = (height(self, self.start(first)?)?, height(self, self.end(last)?)?);
        if !(ha != hb) {
            return Ok(None);
        }
        // Up the meridian.
        let mut up = path.to_vec();
        if ha > hb {
            super::reverse(&mut up);
        }
        let (a, b) = (self.start(*up.first().ok_or("empty slit")?)?, self.end(*up.last().ok_or("empty slit")?)?);
        let pole_vertex = |s: &mut Self, p: Vec3| -> R<u32> {
            let v = index(s.vertices.len())?;
            s.vertices.push(Vertex { point: arr(p), tolerance: s.tol });
            Ok(v)
        };
        let mut merged = Vec::with_capacity(2 * up.len() + 4);
        if !self.at(a, south)? {
            let s = pole_vertex(self, south)?;
            let (seam, along) = self.seam_edge(a, s, ax, eval, true, None)?;
            merged.push((seam, !along));
        }
        merged.extend(up);
        if !self.at(b, north)? {
            let n = pole_vertex(self, north)?;
            merged.push(self.seam_edge(b, n, ax, eval, true, None)?);
        }
        let mut back = merged.clone();
        super::reverse(&mut back);
        merged.extend(back);
        Ok(Some(merged))
    }

    /// A slit along a circle of a torus (round its tube, or round its axis) made the torus's two
    /// seams: that circle closed from the slit's end round to its start, the way the slit runs,
    /// and the other circle through the start; the loop runs round the first, the second, the
    /// first back and the second back. `None` when the slit is off both circles.
    fn torus_slit(&mut self, path: &[(u32, bool)], frame: &Frame, major: f64) -> R<Option<Vec<(u32, bool)>>> {
        let (o, z) = (Vec3::from(frame.origin), Vec3::from(frame.x_axis).cross(Vec3::from(frame.y_axis)));
        let (first, last) = (*path.first().ok_or("empty slit")?, *path.last().ok_or("empty slit")?);
        let (a, b) = (self.start(first)?, self.end(last)?);
        let (pa, pb) = (self.vpoint(a)?, self.vpoint(b)?);
        let off = pa - o;
        let h = off.dot(z);
        let Some(radial) = (off - z * h).normalized() else { return Ok(None) };
        // The circles through the slit's start, as centre and unit normal: round the tube in the
        // plane through the axis, and round the axis at the start's height.
        let tube = (o + radial * major, radial.cross(z));
        let ring = (o + z * h, z);
        let along = |s: &Self, (c, n): (Vec3, Vec3)| -> R<bool> {
            let r = (pa - c).len();
            for &(e, _) in path {
                let edge = s.edges.get(e as usize).ok_or("edge reference")?;
                let curve = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
                let ([lo, hi], reach) = (edge.interval, s.tol.max(edge.tolerance));
                for k in 0..=SAMPLES {
                    let d = curve.point(lo + (hi - lo) * k as f64 / SAMPLES as f64).ok_or("edge point")? - c;
                    if !(d.dot(n).abs() <= reach && (d.len() - r).abs() <= reach) {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        };
        let (circle, other) = if along(self, tube)? {
            (tube, ring)
        } else if along(self, ring)? {
            (ring, tube)
        } else {
            return Ok(None);
        };
        let mut round = path.to_vec();
        if a != b {
            // On from the slit's end the way it arrives there, from a point of its last edge
            // just before.
            let (c, n) = circle;
            let edge = self.edges.get(last.0 as usize).ok_or("edge reference")?;
            let [lo, hi] = edge.interval;
            let t = if last.1 { hi - (hi - lo) / SAMPLES as f64 } else { lo + (hi - lo) / SAMPLES as f64 };
            let before = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?.point(t).ok_or("edge point")?;
            let Some(x) = (pb - c).normalized() else { return Ok(None) };
            let y = n.cross(x);
            let y = if (pb - before).dot(y) >= 0.0 { y } else { -y };
            let d = pa - c;
            let theta = d.dot(y).atan2(d.dot(x)).rem_euclid(TAU);
            let radius = (pb - c).len();
            if !(theta * radius > self.tol) {
                return Ok(None);
            }
            let rest = BCurve::Circle { frame: Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(y) }, radius };
            round.push((self.new_edge(rest, [0.0, theta], [b, a])?, true));
        }
        let (c, n) = other;
        let Some(x) = (pa - c).normalized() else { return Ok(None) };
        let full = BCurve::Circle { frame: Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(n.cross(x)) }, radius: (pa - c).len() };
        let second = self.new_edge(full, [0.0, TAU], [a, a])?;
        let mut back = round.clone();
        super::reverse(&mut back);
        let mut merged = round;
        merged.push((second, true));
        merged.extend(back);
        merged.push((second, false));
        Ok(Some(merged))
    }

    /// Join loop `li` of face `fi` to the pole the face covers: seam out and back.
    fn pole_seam(&mut self, fi: usize, li: usize, ax: &Axis, pole: Option<Vec3>, eval: &SurfaceEval) -> R<()> {
        let step = self.faces.get(fi).map_or(0, |f| f.origin);
        let Some(p) = pole else {
            return Err(format!("face #{step}: a face around an open surface of revolution with one bound"));
        };
        let Some(Bound::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        let es = es.clone();
        // Start at a vertex off the pole.
        let mut k = None;
        for (i, u) in es.iter().enumerate() {
            if self.vpoint(self.start(*u)?)?.dist(p) > self.tol {
                k = Some(i);
                break;
            }
        }
        let k = k.ok_or_else(|| format!("face #{step}: a loop around the axis on its pole"))?;
        let k = match ax.kind {
            Kind::Iso { start, .. } => self.seam_start(fi, li, start, ax, eval)?,
            Kind::Ruled | Kind::Sphere(_) => self.once_start(fi, li, ax)?,
            // A revolution's seam lies on a quarter line, where its form starts ([`Self::band_seam`]):
            // the one at its profile.
            Kind::Revolved(_) => self.seam_start(fi, li, 0.0, ax, eval)?,
            _ => k,
        };
        let ring = self.rotated(fi, li, k)?;
        let a = self.start(*ring.first().ok_or("empty loop")?)?;
        // The face's vertex loop at the pole, if it has one, else a new vertex there.
        let face = self.faces.get(fi).ok_or("face reference")?;
        let at_pole = face.loops.iter().enumerate().find_map(|(i, l)| match l {
            Bound::Vertex(v) => self.vertices.get(*v as usize).filter(|x| Vec3::from(x.point).dist(p) <= self.tol.max(x.tolerance)).map(|_| (i, *v)),
            Bound::Edges(_) => None,
        });
        let b = match at_pole {
            Some((_, v)) => v,
            None => {
                let v = index(self.vertices.len())?;
                self.vertices.push(Vertex { point: arr(p), tolerance: self.tol });
                v
            }
        };
        let (seam, along) = self.seam_edge(a, b, ax, eval, false, Some(p))?;
        let mut merged = ring;
        merged.push((seam, along));
        merged.push((seam, !along));
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        if let Some(l) = face.loops.get_mut(li) {
            *l = Bound::Edges(merged);
        }
        if let Some((i, _)) = at_pole {
            remove_loop(face, i, li);
        }
        Ok(())
    }

    /// The use of loop `li` of face `fi` (a cap round a cone's or a sphere's pole) where its seam
    /// to the pole starts: at an angle about the axis the loop passes only once
    /// ([`Assembly::blocked_angles`]: every piece's angle runs one way there and no other piece
    /// reaches it), so the loop followed from the seam covers exactly one turn. A loop that turns
    /// back partway round (an edge across a drill point cut by another) covers the angles it
    /// turns back over three times; a seam among them left it more than a turn wide, which reads
    /// as a face winding round its axis. The first vertex at such an angle, else the loop split
    /// in the middle of the widest range of them. Where the pieces leave no angle known to be
    /// passed once (a piece whose hull reaches round the axis), the first vertex off the pole, as
    /// every cap had it before.
    fn once_start(&mut self, fi: usize, li: usize, ax: &Axis) -> R<usize> {
        let Some(Bound::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        let es = es.clone();
        let blocked = self.blocked_angles(fi, li, li, ax)?.ok_or("a cap's loop winds round its axis")?;
        let frame = Perpendicular::new(ax);
        let off_pole = |s: &Self, v: u32| -> R<bool> {
            let x = s.vertices.get(v as usize).ok_or("vertex reference")?;
            Ok(!ax.poles.iter().flatten().any(|p| Vec3::from(x.point).dist(*p) <= s.tol.max(x.tolerance)))
        };
        let mut first = None;
        for (k, u) in es.iter().enumerate() {
            let v = self.start(*u)?;
            if !off_pole(self, v)? {
                continue;
            }
            if first.is_none() {
                first = Some((k, v));
            }
            if blocked.clear(frame.angle(self.vpoint(v)?)) {
                return Ok(k);
            }
        }
        let (k, _) = first.ok_or("a loop around the axis on its pole")?;
        let Some(middle) = blocked.widest_clear() else { return Ok(k) };
        // No blocked arc reaches that angle, so the loop passes it on one piece, whose angle runs
        // one way and spans under half a turn: the kernel solves where it reaches the angle on
        // the piece itself. Sampling the edge instead missed it near a pole, where the angle
        // turns fast along the edge (a radian between samples on an edge 0.6 mm from a sphere's
        // pole).
        for &(e, _) in &es {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let Some((form, _)) = nurbs_form(&edge.curve, edge.interval) else { continue };
            let form = CurveEval::new(&form).map_err(|m| m.to_string())?;
            for piece in frame.pieces(&edge.curve, edge.interval, self.tol) {
                if piece.runs.is_none() || (Blocked { arcs: vec![piece.arc] }).clear(middle) {
                    continue;
                }
                let Some((true, p, x)) = ruling_crossing(&frame, &form, piece.on, middle)? else { continue };
                let [e0, e1] = edge.vertices;
                if let Some(v) = [e0, e1].into_iter().find(|&v| self.vpoint(v).is_ok_and(|q| q.dist(p) <= self.tol)) {
                    return self.use_at(fi, li, v);
                }
                let Some(t) = self.edge_parameter(e, x, p)? else { continue };
                let v = self.split_edge(e, t)?;
                return self.use_at(fi, li, v);
            }
        }
        Err(unsupported("a loop round a pole that misses its seam's angle"))
    }

    /// Vertex `vb`, which the file puts a hair off the ruling through vertex `va` (within the
    /// tolerance, as two loops' start vertices written to their own rounding are), moved onto that
    /// ruling at its own height. The seam is then the ruling itself, an isoline of the surface,
    /// with a vertex at each end. Left as the line between the two points, it slants across the
    /// surface's seam by that hair, so the vertex that is off the seam line is a vertex "near the
    /// seam" to SMLib's check ("Face Near missing Seam"), which the isoline's end vertices are
    /// not. The vertex's tolerance is raised to cover its move and the ends of the edges that
    /// already meet it, as a new edge's covers its gap.
    /// A vertex already on the ruling (to 1e-10 mm), or one the surface cannot place there, stays as it is.
    fn seat_on_ruling(&mut self, va: u32, vb: u32, d: usize, eval: &SurfaceEval) -> R<()> {
        let (pa, pb) = (self.vpoint(va)?, self.vpoint(vb)?);
        let Some(qa) = eval.project(pa, None, f64::INFINITY) else { return Ok(()) };
        let Some(mut q) = eval.project(pb, Some(qa), f64::INFINITY) else { return Ok(()) };
        let (Some(&ua), Some(slot)) = (qa.get(d), q.get_mut(d)) else { return Ok(()) };
        // Along the ruling's own angle, by the short way round when the two sit either side of
        // the surface's period.
        *slot += wrap(ua - *slot);
        let Some(on) = eval.point(q) else { return Ok(()) };
        let gap = on.dist(pb);
        if !(gap > 1e-10 && gap <= self.tol) {
            return Ok(());
        }
        // The edges already ending at the vertex keep their ends where they were: its tolerance
        // covers each end's distance from where the vertex now is, as `new_edge` covers a new one's.
        let mut widest = 1.5 * gap;
        for e in self.edges.iter().filter(|e| e.vertices.contains(&vb)) {
            let c = CurveEval::new(&e.curve).map_err(|m| m.to_string())?;
            for (t, v) in e.interval.into_iter().zip(e.vertices) {
                if v == vb {
                    let end = c.point(t).ok_or("edge end")?;
                    widest = widest.max(1.5 * end.dist(on));
                }
            }
        }
        let x = self.vertices.get_mut(vb as usize).ok_or("vertex reference")?;
        x.point = arr(on);
        x.tolerance = x.tolerance.max(widest);
        Ok(())
    }

    /// Join loops `la` (running +u) and `lb` (running −u) of face `fi` by a seam.
    fn band_seam(&mut self, fi: usize, la: usize, lb: usize, ax: &Axis, eval: &SurfaceEval, splits: &mut usize) -> R<()> {
        // The parameter the loops wind in: u, or v round a torus's tube.
        let d = ax.kind.winds_in();
        let angle = |s: &Self, v: u32| -> R<f64> {
            let p = s.vpoint(v)?;
            eval.project(p, None, f64::INFINITY).and_then(|q| q.get(d).copied()).ok_or_else(|| "a vertex off its surface".to_string())
        };
        let (Some(Bound::Edges(a_es)), Some(Bound::Edges(b_es))) =
            (self.faces.get(fi).and_then(|f| f.loops.get(la)), self.faces.get(fi).and_then(|f| f.loops.get(lb)))
        else {
            return Err("loop reference".into());
        };
        let (a_es, b_es) = (a_es.clone(), b_es.clone());
        // Angles agree within the tolerance at the radius they are at.
        let close = |s: &Self, va: u32, vb: u32| -> R<bool> {
            let (ua, ub) = (angle(s, va)?, angle(s, vb)?);
            let r = match &ax.kind {
                // Round the tube the angle is the tube's, at the tube's radius at most.
                Kind::TorusTube => match &s.faces.get(fi).map(|f| &f.surface) {
                    Some(BSurface::Torus { minor_radius, .. }) => *minor_radius,
                    _ => 0.0,
                },
                _ => {
                    let p = s.vpoint(va)? - ax.origin;
                    (p - ax.z * p.dot(ax.z)).len()
                }
            };
            Ok(wrap(ua - ub).abs() * r <= s.tol)
        };
        // A face that winds round its axis more than a turn (a thread's crest between its two
        // helices, run on into the plain cylinder past the thread's end): one of its loops,
        // followed from its start, goes round more than once, so it reaches every angle more than
        // once and no angle is clear of it; its seam is found along the ruling instead.
        let mut wound = false;
        if ax.kind == Kind::Ruled {
            for li in [la, lb] {
                wound |= self.loops_extent(fi, Some(li))?.is_some_and(crate::kernel::form::winds);
            }
        }
        if wound {
            return self.wound_band_seam(fi, la, lb, ax, eval, splits);
        }
        if let Kind::Iso { start, .. } = ax.kind {
            // Both loops start on the spline's boundary column, split there where they don't.
            let i = self.seam_start(fi, la, start, ax, eval)?;
            let j = self.seam_start(fi, lb, start, ax, eval)?;
            *splits += 2;
            if *splits > MAX_SPLITS {
                return Err(format!("more than {MAX_SPLITS} edges split for seams"));
            }
            return self.join_by_seam(fi, la, lb, i, j, ax, eval);
        }
        // The seam must be an edge of the face as it stands: longer than its tolerance (an edge
        // no smaller than its tube, Edge 7), and meeting no other loop of the face (one it crossed
        // was cut in two, and the face lost). The angles about the axis those loops can reach
        // are blocked; the seam runs from the first vertex of the two loops (pairs at one angle
        // first, then loop A's, then loop B's) at a clear angle where the band is wide enough,
        // else from the middle of the widest clear range, both loops split there.
        let blocked = self.blocked_angles(fi, la, lb, ax)?;
        let frame = Perpendicular::new(ax);
        let first = self.start(*a_es.first().ok_or("empty loop")?)?;
        // The surface's parameter `d` at an angle about the axis: offset from the frame's angle
        // by that of A's first vertex (both measured right-handed about the axis).
        let offset = angle(self, first)? - frame.angle(self.vpoint(first)?);
        if let Kind::Revolved(_) = ax.kind {
            // A revolution's form starts on the quarter line its seam lies in (`brep::form`: a
            // whole turn is the four quarters from the one it starts in), and its frame cannot
            // be turned to start at a seam elsewhere (`frames`): a seam off a quarter line leaves
            // its second use past the form's end. Its seam is the meridian on the first quarter
            // line no other loop reaches, both loops starting there, split where they don't.
            let start = (0..4u8)
                .map(|q| f64::from(q) * FRAC_PI_2)
                .find(|u| blocked.as_ref().is_none_or(|b| b.clear(u - offset)))
                .ok_or_else(|| unsupported("a band round a revolved surface whose other loops reach each of its quarter lines"))?;
            let i = self.seam_start(fi, la, start, ax, eval)?;
            let j = self.seam_start(fi, lb, start, ax, eval)?;
            *splits += 2;
            if *splits > MAX_SPLITS {
                return Err(format!("more than {MAX_SPLITS} edges split for seams"));
            }
            return self.join_by_seam(fi, la, lb, i, j, ax, eval);
        }
        let clear = |s: &Self, v: u32| -> R<bool> {
            let a = frame.angle(s.vpoint(v)?);
            Ok(blocked.as_ref().is_none_or(|b| b.clear(a)))
        };
        let long = |s: &Self, va: u32, pb: Vec3| -> R<bool> { Ok(s.vpoint(va)?.dist(pb) > s.tol) };
        let mut pair = None;
        'search: for (i, ua) in a_es.iter().enumerate().take(100) {
            let va = self.start(*ua)?;
            for (j, ub) in b_es.iter().enumerate().take(100) {
                let vb = self.start(*ub)?;
                if close(self, va, vb)? && clear(self, va)? && long(self, va, self.vpoint(vb)?)? {
                    pair = Some((i, j));
                    break 'search;
                }
            }
        }
        if let Some((i, j)) = pair {
            if ax.kind == Kind::Ruled {
                let (va, vb) = (self.start(*a_es.get(i).ok_or("loop reference")?)?, self.start(*b_es.get(j).ok_or("loop reference")?)?);
                self.seat_on_ruling(va, vb, d, eval)?;
            }
            return self.join_by_seam(fi, la, lb, i, j, ax, eval);
        }
        // A vertex of one loop and where the ruling (or meridian) through it meets the other.
        let mut from_vertex = None;
        'vertex: for (from, onto) in [(la, lb), (lb, la)] {
            let (mine, theirs) = if from == la { (&a_es, &b_es) } else { (&b_es, &a_es) };
            for (k, u) in mine.iter().enumerate().take(100) {
                let v = self.start(*u)?;
                if !clear(self, v)? {
                    continue;
                }
                let target = angle(self, v)?;
                if let Some((e, t, p)) = self.meets(theirs, d, target, ax.period, eval)?
                    && long(self, v, p)?
                {
                    from_vertex = Some((from, onto, k, e, t));
                    break 'vertex;
                }
            }
        }
        *splits += 1;
        if *splits > MAX_SPLITS {
            return Err(format!("more than {MAX_SPLITS} edges split for seams"));
        }
        let (i, j) = match from_vertex {
            Some((from, onto, k, e, t)) => {
                let v = self.split_edge(e, t)?;
                let at = self.use_at(fi, onto, v)?;
                if from == la { (k, at) } else { (at, k) }
            }
            None => {
                let middle = blocked
                    .as_ref()
                    .and_then(Blocked::widest_clear)
                    .ok_or_else(|| unsupported("a band whose other loops leave its seam no angle"))?;
                let target = middle + offset;
                let (ea, ta, pa) = self.meets(&a_es, d, target, ax.period, eval)?.ok_or_else(|| unsupported("a band whose bounds share no angle"))?;
                let (eb, tb, pb) = self.meets(&b_es, d, target, ax.period, eval)?.ok_or_else(|| unsupported("a band whose bounds share no angle"))?;
                // Both points at the clear angle chosen: the surface's parameter runs the way
                // the angle does.
                if !(blocked.as_ref().is_none_or(|b| b.clear(frame.angle(pa)) && b.clear(frame.angle(pb)))) {
                    return Err(unsupported("a band whose seam's angle meets its other loops"));
                }
                if !(pa.dist(pb) > self.tol) {
                    return Err("a seam of no length".into());
                }
                *splits += 1;
                let va = self.split_edge(ea, ta)?;
                let vb = self.split_edge(eb, tb)?;
                (self.use_at(fi, la, va)?, self.use_at(fi, lb, vb)?)
            }
        };
        self.join_by_seam(fi, la, lb, i, j, ax, eval)
    }

    /// The seam of band face `fi` on a cylinder or cone when the face winds round its axis more
    /// than a turn, between its loops `la` (running +u, the face above it along the axis) and `lb`
    /// (running −u, the face below it). Every angle meets the loops more than once, so the seam is
    /// placed on the ruling itself: from a vertex of one loop where the loop runs its own way on
    /// both sides (so the face lies along the ruling towards the other loop), the ruling's
    /// crossings with every loop of the face are found on the Bézier pieces of their edges' NURBS
    /// forms, and the seam runs to the nearest, which must be on the other loop. The open segment
    /// between then meets no loop, so it lies in the face: the regions off the face beyond a loop
    /// are bounded by that loop alone. A piece whose angle is not monotone, or one that comes within
    /// its pad of the ruling's angle without crossing it between its ends, may meet the ruling
    /// anywhere over the heights [`meets`] gives it, none of which may be on the seam. A ruling
    /// that crosses the other loop within a piece's pad of a joint between two of its pieces (a knot
    /// of a helix, the next quarter of a circle) has the piece across the joint come that near
    /// without crossing: where both run one way round the axis the loop passes the ruling once
    /// there, and that piece is the loop going on from the seam's end, no matter unless it reaches
    /// back along the seam from there by more than the tolerance. Vertices are tried in loop order,
    /// `la`'s first.
    fn wound_band_seam(&mut self, fi: usize, la: usize, lb: usize, ax: &Axis, eval: &SurfaceEval, splits: &mut usize) -> R<()> {
        let frame = Perpendicular::new(ax);
        let height = |p: Vec3| (p - ax.origin).dot(ax.z);
        // Each loop's uses in order: the use, its edge's NURBS form, and the form's pieces along
        // the loop (reversed, their angle's way with them, on a use against its edge).
        let face = self.faces.get(fi).ok_or("face reference")?;
        let mut loops: Vec<(usize, Vec<WoundUse>)> = Vec::new();
        let mut points: Vec<Vec3> = Vec::new();
        for (li, l) in face.loops.iter().enumerate() {
            let es = match l {
                Bound::Edges(es) => es,
                Bound::Vertex(v) => {
                    points.push(self.vpoint(*v)?);
                    continue;
                }
            };
            let mut uses = Vec::new();
            for &(e, forward) in es {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let form = nurbs_form(&edge.curve, edge.interval).map(|(c, _)| c);
                let mut pieces = frame.pieces(&edge.curve, edge.interval, self.tol);
                if !forward {
                    pieces.reverse();
                    for p in &mut pieces {
                        p.runs = p.runs.map(|r| -r);
                    }
                }
                let form = match form {
                    Some(c) => Some(CurveEval::new(&c).map_err(|m| m.to_string())?),
                    None => None,
                };
                uses.push(WoundUse { edge: e, forward, form, pieces });
            }
            loops.push((li, uses));
        }
        // Candidate starts: (loop, use) whose start vertex has the loop's way on both sides.
        let mut starts = Vec::new();
        for (from, way) in [(la, 1i8), (lb, -1i8)] {
            let Some((_, uses)) = loops.iter().find(|(li, _)| *li == from) else { return Err("loop reference".into()) };
            for k in 0..uses.len().min(100) {
                let before = uses.get(k.checked_sub(1).unwrap_or(uses.len().saturating_sub(1))).and_then(|u| u.pieces.last());
                let after = uses.get(k).and_then(|u| u.pieces.first());
                if before.is_some_and(|p| p.runs == Some(way)) && after.is_some_and(|p| p.runs == Some(way)) {
                    starts.push((from, k));
                }
            }
        }
        for (from, k) in starts {
            let (onto, up) = if from == la { (lb, 1.0) } else { (la, -1.0) };
            let Some((_, uses)) = loops.iter().find(|(li, _)| *li == from) else { continue };
            let Some(u) = uses.get(k) else { continue };
            let v = self.start((u.edge, u.forward))?;
            let pv = self.vpoint(v)?;
            let theta = frame.angle(pv);
            // Signed distance along the ruling from the vertex, towards the other loop.
            let along = |h: f64| up * (h - height(pv));
            let before = k.checked_sub(1).unwrap_or(uses.len().saturating_sub(1));
            // The nearest crossing on the other loop (distance, edge, its point), and the nearest
            // reach of anything else along the ruling (from the vertex on, or straddling it).
            let mut nearest: Option<(f64, u32, Vec3, f64)> = None;
            let mut other = f64::INFINITY;
            // The other loop's pieces counted along it: the place and way of the one its nearest
            // crossing is on, those that may meet the ruling without crossing it (place, way,
            // reach), and how many it has.
            let mut crossed: Option<(usize, Option<i8>)> = None;
            let mut doubtful: Vec<(usize, Option<i8>, f64)> = Vec::new();
            let mut count = 0usize;
            for &p in &points {
                let d = along(height(p));
                if !(Blocked { arcs: vec![frame.point_arc(p, self.tol)] }).clear(theta) && d > -self.tol {
                    other = other.min(d.max(0.0));
                }
            }
            for (li, luses) in &loops {
                let mut place = 0usize;
                for (ui, w) in luses.iter().enumerate() {
                    let last = w.pieces.len().saturating_sub(1);
                    for (pi, piece) in w.pieces.iter().enumerate() {
                        let here = place;
                        place += 1;
                        // The vertex's own pieces meet the ruling there only (monotone, under half
                        // a turn).
                        if *li == from && ((ui == k && pi == 0) || (ui == before && pi == last)) {
                            continue;
                        }
                        match meets(&frame, &w.form, piece, theta, height, along, self.tol)? {
                            Meets::Clear => {}
                            Meets::Once { d, p, x } if *li == onto => {
                                // Behind the vertex it is no matter, unless it touches it.
                                if d > 0.0 {
                                    if nearest.is_none_or(|(n, ..)| d < n) {
                                        nearest = Some((d, w.edge, p, x));
                                        crossed = Some((here, piece.runs));
                                    }
                                } else if d > -self.tol {
                                    other = 0.0;
                                }
                            }
                            // Another loop's crossing, at its one point.
                            Meets::Once { d, .. } => {
                                other = other.min(if d > -self.tol { d.max(0.0) } else { f64::INFINITY });
                            }
                            // The other loop's own: judged once its nearest crossing is known.
                            Meets::Hull(Some(reach)) if *li == onto => doubtful.push((here, piece.runs, reach)),
                            Meets::Hull(reach) => other = other.min(reach.unwrap_or(f64::INFINITY)),
                        }
                    }
                }
                if *li == onto {
                    count = place;
                }
            }
            let Some((d, e, p, x)) = nearest else { continue };
            for (place, runs, reach) in doubtful {
                // The piece next along the loop to the one crossed, both running one way round
                // the axis: their angle is monotone through the joint between them, so the loop
                // passes the ruling once, at the crossing, and this piece is the loop going on
                // from there. It is on the seam only where it reaches back along it.
                let goes_on = crossed.is_some_and(|(at, way)| {
                    let apart = at.abs_diff(place);
                    way.is_some() && way == runs && (apart == 1 || apart.checked_add(1) == Some(count))
                });
                if !(goes_on && reach > d - self.tol) {
                    other = other.min(reach);
                }
            }
            // Anything else at the crossing or before it: another loop, a doubtful piece, or a
            // crossing of the other loop's own short of where the seam would end.
            if !(d > self.tol) || other <= d + self.tol {
                continue;
            }
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let [e0, e1] = edge.vertices;
            let at_vertex = [e0, e1].into_iter().find(|&x| self.vpoint(x).is_ok_and(|q| q.dist(p) <= self.tol));
            let vb = match at_vertex {
                Some(x) => x,
                None => {
                    let Some(t) = self.edge_parameter(e, x, p)? else { continue };
                    *splits += 1;
                    if *splits > MAX_SPLITS {
                        return Err(format!("more than {MAX_SPLITS} edges split for seams"));
                    }
                    self.split_edge(e, t)?
                }
            };
            let (i, j) =
                if from == la { (self.use_at(fi, la, v)?, self.use_at(fi, lb, vb)?) } else { (self.use_at(fi, la, vb)?, self.use_at(fi, lb, v)?) };
            return self.join_by_seam(fi, la, lb, i, j, ax, eval);
        }
        Err(unsupported("a band winding round its axis whose rulings from its vertices meet another loop first"))
    }

    /// Join loops `la` and `lb` of face `fi`, from their uses `i` and `j`, by a seam between
    /// the vertices those uses start at: loop A, the seam out, loop B, the seam back.
    #[allow(clippy::too_many_arguments)]
    fn join_by_seam(&mut self, fi: usize, la: usize, lb: usize, i: usize, j: usize, ax: &Axis, eval: &SurfaceEval) -> R<()> {
        let a_ring = self.rotated(fi, la, i)?;
        let b_ring = self.rotated(fi, lb, j)?;
        let va = self.start(*a_ring.first().ok_or("empty loop")?)?;
        let vb = self.start(*b_ring.first().ok_or("empty loop")?)?;
        let (seam, along) = self.seam_edge(va, vb, ax, eval, true, None)?;
        let mut merged = a_ring;
        merged.push((seam, along));
        merged.extend(b_ring);
        merged.push((seam, !along));
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        if let Some(l) = face.loops.get_mut(la) {
            *l = Bound::Edges(merged);
        }
        remove_loop(face, lb, la);
        Ok(())
    }

    /// The parameter inside edge `e`'s interval where its curve reaches the value `target` of
    /// the surface's parameter `d` (0: u, an angle about the axis; 1: v), if it does, further
    /// than the tolerance from both of its ends.
    fn crossing(&self, e: u32, d: usize, target: f64, period: f64, eval: &SurfaceEval) -> R<Option<f64>> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let [lo, hi] = edge.interval;
        let at = d;
        let d = |t: f64| {
            c.point(t).and_then(|p| eval.project(p, None, f64::INFINITY)).and_then(|q| q.get(at).copied()).map(|x| wrap_by(x - target, period))
        };
        // A jump across the half period is the wrap, not a crossing.
        let near = period / TAU;
        let n = 64;
        let mut prev: Option<(f64, f64)> = None;
        for k in 0..=n {
            let t = lo + (hi - lo) * k as f64 / n as f64;
            let Some(x) = d(t) else { continue };
            if let Some((t0, x0)) = prev
                && x0.signum() != x.signum()
                && x0.abs() < near
                && x.abs() < near
            {
                let (mut a, mut b, mut fa) = (t0, t, x0);
                for _ in 0..100 {
                    let m = 0.5 * (a + b);
                    let Some(fm) = d(m) else { break };
                    if fm.signum() == fa.signum() {
                        a = m;
                        fa = fm;
                    } else {
                        b = m;
                    }
                }
                let mut t = 0.5 * (a + b);
                // The solve runs through the surface's projection, whose parameter carries its
                // own rounding, so a crossing it lands a rounding away from where the edge's own
                // form has a knot is read as that knot exactly. A knot is the crossing where it
                // lies on the seam line itself, within the record's tolerance (the check a
                // vertex of the loop is read by), and clear of the edge's ends by more than
                // that tolerance along it, as the refined root itself must be; the nearest such
                // knot takes the split, and without one the root keeps it. The split vertex
                // then lies where the form has it, and the trims and the pcurves made beside
                // the split agree with it exactly; a rounding apart, a loop that runs a whole
                // turn of a form that closes comes off the pcurve maker a rounding wider than
                // the form's range, and the clamp that then puts its end on the range leaves
                // the rounding as a gap the record's check names. A spline's knots are its
                // own; a circle's form is four quarter arcs, its knots the quarter turns of
                // its parameter.
                let inside = |k: f64| {
                    k > lo
                        && k < hi
                        && c.point(k).zip(c.point(lo)).is_some_and(|(p, q)| p.dist(q) > self.tol)
                        && c.point(k).zip(c.point(hi)).is_some_and(|(p, q)| p.dist(q) > self.tol)
                };
                let on_seam = |k: f64| {
                    let Some(p) = c.point(k) else { return false };
                    let Some(mut q) = eval.project(p, None, f64::INFINITY) else { return false };
                    if let Some(x) = q.get_mut(at) {
                        *x = target;
                    }
                    eval.point(q).is_some_and(|s| s.dist(p) <= self.tol)
                };
                let candidates: Vec<f64> = match &edge.curve {
                    BCurve::Spline { knots, .. } | BCurve::RationalSpline { knots, .. } => knots.to_vec(),
                    BCurve::Circle { .. } => {
                        let first = (lo / FRAC_PI_2).floor() as i64;
                        let last = (hi / FRAC_PI_2).ceil() as i64;
                        (first..last).map(|k| k as f64 * FRAC_PI_2).collect()
                    }
                    _ => Vec::new(),
                };
                let root = t;
                t = candidates
                    .into_iter()
                    .filter(|k| inside(*k) && on_seam(*k))
                    .min_by(|x, y| (x - root).abs().total_cmp(&(y - root).abs()))
                    .unwrap_or(root);
                // Inside the edge by more than the tolerance along it, measured in length: a split
                // nearer an end would leave an edge no longer than its tolerance (the end vertex
                // is then the one on the line, within the tolerance).
                let clear = |end: f64| c.point(t).zip(c.point(end)).is_some_and(|(p, q)| p.dist(q) > self.tol);
                return Ok((t > lo && t < hi && clear(lo) && clear(hi)).then_some(t));
            }
            prev = Some((t, x));
        }
        Ok(None)
    }

    /// The first edge of `es` that crosses the value `target` of the surface's parameter `d`
    /// inside its interval: the edge, the crossing's parameter on it and its point.
    fn meets(&self, es: &[(u32, bool)], d: usize, target: f64, period: f64, eval: &SurfaceEval) -> R<Option<(u32, f64, Vec3)>> {
        for &(e, _) in es {
            if let Some(t) = self.crossing(e, d, target, period, eval)? {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let p = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?.point(t).ok_or("edge point")?;
                return Ok(Some((e, t, p)));
            }
        }
        Ok(None)
    }

    /// The index of the use of loop `li` of face `fi` that starts at vertex `v`.
    fn use_at(&self, fi: usize, li: usize, v: u32) -> R<usize> {
        let Some(Bound::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        for (k, u) in es.iter().enumerate() {
            if self.start(*u)? == v {
                return Ok(k);
            }
        }
        Err("the split vertex is not on its loop".into())
    }

    /// The angles about the axis a seam of face `fi` between its loops `la` and `lb` must not
    /// take. A seam is an edge of the face only where it meets each of the two loops once and no
    /// other loop at all, and that is decided on the Bézier pieces of the edges' NURBS forms:
    /// each piece lies within the angles of its control points (the convex hull; a hull clear of
    /// the axis spans less than half a turn about it), and its angle runs one way where the
    /// Bernstein coefficients of its angular speed all have one sign. Blocked: every piece of the
    /// other loops; of the two loops, a piece whose angle is not monotone, and where two pieces
    /// reach the same angles (save two pieces one after the other running the same way, which
    /// meet at their shared end only). Each blocked arc is widened by its rounding and by the
    /// tolerance at the piece's smallest radius (`tol / r`), so a vertex that lies off its curve's
    /// end by a rounding, or a loop within the tolerance of the seam, is not clear. `None` round a
    /// torus's tube, where the loops wind in v and the angle about the axis does not part them.
    fn blocked_angles(&self, fi: usize, la: usize, lb: usize, ax: &Axis) -> R<Option<Blocked>> {
        if ax.kind.winds_in() != 0 {
            return Ok(None);
        }
        let frame = Perpendicular::new(ax);
        let face = self.faces.get(fi).ok_or("face reference")?;
        let mut arcs = Vec::new();
        for (li, l) in face.loops.iter().enumerate() {
            let es = match l {
                Bound::Vertex(v) => {
                    // A vertex loop at a pole (a cone's tip) is every angle at once: a seam to the
                    // pole ends there and crosses nothing.
                    let x = self.vertices.get(*v as usize).ok_or("vertex reference")?;
                    if !ax.poles.iter().flatten().any(|p| Vec3::from(x.point).dist(*p) <= self.tol.max(x.tolerance)) {
                        arcs.push(frame.point_arc(self.vpoint(*v)?, self.tol));
                    }
                    continue;
                }
                Bound::Edges(es) => es,
            };
            // The loop's pieces in its order, each with the way its angle runs along the loop.
            let mut pieces: Vec<Piece> = Vec::new();
            for &(e, forward) in es {
                let edge = self.edges.get(e as usize).ok_or("edge reference")?;
                let mut own = frame.pieces(&edge.curve, edge.interval, self.tol);
                if !forward {
                    own.reverse();
                    for p in &mut own {
                        p.runs = p.runs.map(|r| -r);
                    }
                }
                pieces.extend(own);
            }
            if li != la && li != lb {
                arcs.extend(pieces.iter().map(Piece::blocked));
                continue;
            }
            let n = pieces.len();
            arcs.extend(pieces.iter().filter(|p| p.runs.is_none()).map(Piece::blocked));
            for (i, j) in overlapping_pairs(&pieces) {
                let (Some(p), Some(q)) = (pieces.get(i), pieces.get(j)) else { continue };
                let next = j == i + 1 || (i == 0 && j + 1 == n);
                if next && p.runs.is_some() && p.runs == q.runs {
                    continue;
                }
                arcs.extend(overlap(p.arc, q.arc).into_iter().map(|a| padded(a, p.pad.max(q.pad))));
            }
        }
        Ok(Some(Blocked { arcs }))
    }

    /// The use of loop `li` of face `fi` that starts on the closed spline's boundary line at
    /// `start` (the column at u = `start`, or the row at v = `start` for one closed in v), or on a
    /// revolution's meridian at angle `start`: a vertex there within the tolerance (or one whose
    /// foot on the surface is there, within the vertex's own distance from the surface), else one
    /// made by splitting the loop's edge that crosses it.
    fn seam_start(&mut self, fi: usize, li: usize, start: f64, ax: &Axis, eval: &SurfaceEval) -> R<usize> {
        let Some(Bound::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        let es = es.clone();
        let d = ax.kind.winds_in();
        // A vertex is on the line within the tolerance, or its foot on the surface is within
        // the vertex's own distance from the surface of it (at most the largest tolerance the
        // record takes): a fitted surface a little off its edges puts each loop's vertex that
        // far off every line of it, the seam's too, and the loop then crosses the line within
        // that distance of the vertex, too near it for a split. The seam starts on the line at
        // the vertex's place along it, the vertex's tolerance raised to cover the gap, as a new
        // edge's covers its ends.
        let on_seam = |s: &Self, v: u32| -> R<bool> {
            let p = s.vpoint(v)?;
            let Some(q) = eval.project(p, None, f64::INFINITY) else { return Ok(false) };
            let on = if d == 0 { [start, q[1]] } else { [q[0], start] };
            let Some(c) = eval.point(on) else { return Ok(false) };
            if c.dist(p) <= s.tol {
                return Ok(true);
            }
            // Measured on the surface: from the vertex's foot on it to the line.
            let Some(foot) = eval.point(q) else { return Ok(false) };
            let reach = s.tol.max(foot.dist(p)).min(super::MAX_TOLERANCE);
            Ok(c.dist(foot) <= reach)
        };
        for (k, u) in es.iter().enumerate() {
            if on_seam(self, self.start(*u)?)? {
                return Ok(k);
            }
        }
        for &(e, _) in &es {
            if let Some(t) = self.crossing(e, d, start, ax.period, eval)? {
                let v = self.split_edge(e, t)?;
                let Some(Bound::Edges(now)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
                for (k, u) in now.iter().enumerate() {
                    if self.start(*u)? == v {
                        return Ok(k);
                    }
                }
                return Err("the split vertex is not on its loop".into());
            }
        }
        Err(unsupported("a loop around a closed surface that never reaches the line its seam runs on"))
    }

    /// The parameter inside edge `e` of point `p`, found at parameter `x` of the edge's NURBS
    /// form ([`nurbs_form`]). A spline is its own form; a line's, circle's or ellipse's point is
    /// found on the edge's own curve, where it is the one nearest. `None` where that is not
    /// within the tolerance of `p` or not inside the edge's interval.
    fn edge_parameter(&self, e: u32, x: f64, p: Vec3) -> R<Option<f64>> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let [lo, hi] = edge.interval;
        let t = match &edge.curve {
            BCurve::Spline { .. } | BCurve::RationalSpline { .. } => x,
            curve => {
                let c = CurveEval::new(curve).map_err(|m| m.to_string())?;
                let Some(t) = crate::topology::project(&c, lo, hi, &[], p) else { return Ok(None) };
                if !c.point(t).is_some_and(|q| q.dist(p) <= self.tol) {
                    return Ok(None);
                }
                t
            }
        };
        Ok((lo < t && t < hi).then_some(t))
    }

    /// Split edge `e` at `t` inside its interval: it keeps [min, t], a new edge takes
    /// [t, max], and every loop using it uses both. Returns the new vertex.
    pub(crate) fn split_edge(&mut self, e: u32, t: f64) -> R<u32> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let p = c.point(t).ok_or("split point")?;
        let (curve, [lo, hi], [a, b], tolerance) = (edge.curve.clone(), edge.interval, edge.vertices, edge.tolerance);
        let (pcurve_tolerance, origin) = (edge.pcurve_tolerance, edge.origin);
        if !(t > lo && t < hi) {
            return Err("a split outside its edge".into());
        }
        let v = index(self.vertices.len())?;
        self.vertices.push(Vertex { point: arr(p), tolerance: self.tol });
        let e2 = index(self.edges.len())?;
        if let Some(x) = self.edges.get_mut(e as usize) {
            x.interval = [lo, t];
            x.vertices = [a, v];
        }
        self.edges.push(EdgeB { curve, interval: [t, hi], vertices: [v, b], tolerance, pcurve_tolerance, form: None, origin });
        for f in &mut self.faces {
            for l in &mut f.loops {
                let Bound::Edges(es) = l else { continue };
                if !es.iter().any(|u| u.0 == e) {
                    continue;
                }
                let mut out = Vec::with_capacity(es.len() + 1);
                for &(x, forward) in es.iter() {
                    if x != e {
                        out.push((x, forward));
                    } else if forward {
                        out.extend([(e, true), (e2, true)]);
                    } else {
                        out.extend([(e2, false), (e, false)]);
                    }
                }
                *es = out;
            }
        }
        Ok(v)
    }
}

/// Remove loop `gone` from a face whose loop `kept` absorbed it, keeping its outer loop marked.
fn remove_loop(face: &mut super::FaceB, gone: usize, kept: usize) {
    if gone >= face.loops.len() {
        return;
    }
    face.loops.remove(gone);
    face.outer = match face.outer {
        Some(o) if o == gone => Some(if kept > gone { kept - 1 } else { kept }),
        Some(o) if o > gone => Some(o - 1),
        o => o,
    };
}

/// A curve's NURBS form over `interval`, and the interval on it: a spline is its own, a line,
/// circle or ellipse its form ([`crate::kernel::form::curve`]).
fn nurbs_form(curve: &BCurve, interval: [f64; 2]) -> Option<(BCurve, [f64; 2])> {
    match curve {
        BCurve::Spline { .. } | BCurve::RationalSpline { .. } => Some((curve.clone(), interval)),
        other => crate::kernel::form::curve(other, interval),
    }
}

/// Directions square to an axis, to measure angles about it.
pub(crate) struct Perpendicular {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
}

impl Perpendicular {
    fn new(ax: &Axis) -> Self {
        Self::about(ax.origin, ax.z)
    }

    /// Directions square to the axis through `origin` along unit `z`.
    pub(crate) fn about(origin: Vec3, z: Vec3) -> Self {
        let pick = if z.x.abs() <= z.y.abs() && z.x.abs() <= z.z.abs() {
            Vec3::new(1.0, 0.0, 0.0)
        } else if z.y.abs() <= z.z.abs() {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };
        let x = (pick - z * pick.dot(z)).normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        Perpendicular { origin, x, y: z.cross(x) }
    }

    /// `p`'s angle about the axis, right-handed.
    fn angle(&self, p: Vec3) -> f64 {
        let d = p - self.origin;
        d.dot(self.y).atan2(d.dot(self.x))
    }

    /// The Bézier pieces of a curve's NURBS form on `interval` (a line, circle or ellipse on its
    /// form, [`crate::kernel::form::curve`]): each piece's arc of angles and the way its angle
    /// runs, a piece that reaches half a turn in halves ([`MAX_HALVINGS`]); the whole turn where a
    /// hull is still not clear of the axis.
    pub(crate) fn pieces(&self, curve: &BCurve, interval: [f64; 2], tol: f64) -> Vec<Piece> {
        let whole =
            || vec![Piece { arc: [0.0, TAU], pad: 0.0, runs: None, on: interval, heights: [f64::NEG_INFINITY, f64::INFINITY], points: Vec::new() }];
        let Some((form, [a, b])) = nurbs_form(curve, interval) else { return whole() };
        let (knots, homogeneous): (&Vec<f64>, Vec<[f64; 4]>) = match &form {
            BCurve::Spline { knots, points } => (knots, points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect()),
            BCurve::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
            _ => return whole(),
        };
        let Some(degree) = knots.len().checked_sub(homogeneous.len() + 1) else { return whole() };
        let (Some(&k0), Some(&k1)) = (knots.get(degree), knots.get(homogeneous.len())) else { return whole() };
        // The interval within the knots (a file's end parameter a rounding past them).
        let (a, b) = (a.min(b).max(k0), a.max(b).min(k1));
        if !(a < b) || degree == 0 {
            return whole();
        }
        // The knot spans inside the interval: one Bézier piece each.
        let mut cuts = vec![a];
        cuts.extend(knots.iter().copied().filter(|k| *k > a && *k < b));
        cuts.push(b);
        cuts.dedup();
        // A piece that reaches half a turn about the axis (a closed spline of two pieces, each
        // half way round: a cylinder's end as some exporters write it) has no arc of its own:
        // halved, exactly, until each part has.
        let mut todo: Vec<(f64, f64, u32)> = cuts.windows(2).rev().filter_map(|w| if let &[s, t] = w { Some((s, t, 0)) } else { None }).collect();
        let mut out = Vec::new();
        while let Some((s, t, halvings)) = todo.pop() {
            let Ok((_, piece)) = solidcore::eval::pcurve::segment(degree, knots, &homogeneous, s, t) else { return whole() };
            if piece.len() != degree + 1 || piece.iter().any(|h| !(h[3] > 0.0)) {
                return whole();
            }
            let points: Vec<Vec3> = piece.iter().map(|h| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])).collect();
            let Some((arc, pad)) = self.span(&points, tol) else {
                let middle = 0.5 * (s + t);
                if halvings >= MAX_HALVINGS || !(s < middle && middle < t) {
                    return whole();
                }
                todo.push((middle, t, halvings + 1));
                todo.push((s, middle, halvings + 1));
                continue;
            };
            // The homogeneous coordinates square to the axis: the angle of the piece's points.
            let flat: Vec<[f64; 2]> = piece
                .iter()
                .map(|h| {
                    let d = Vec3::new(h[0], h[1], h[2]) - self.origin * h[3];
                    [d.dot(self.x), d.dot(self.y)]
                })
                .collect();
            let z = self.x.cross(self.y);
            let height = |p: &Vec3| (*p - self.origin).dot(z);
            let heights = points.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], p| [lo.min(height(p)), hi.max(height(p))]);
            out.push(Piece { arc, pad, runs: angular_sign(&flat), on: [s, t], heights, points: piece });
        }
        out
    }

    /// The arc of angles `[from, to]` (to − from under π) that points spanning less than half a
    /// turn reach, measured from the first, and how far to widen it where it blocks a seam: its
    /// rounding (each angle's `atan2` and coordinates, `γ` of the operations at the points'
    /// distance over their radius) and `tol` at the smallest radius the points' hull reaches (the
    /// least radius times the cosine of half the arc, as their convex hull lies within the wedge).
    /// `None` when a point is not finite or on the axis, or they reach half a turn.
    pub(crate) fn span(&self, points: &[Vec3], tol: f64) -> Option<([f64; 2], f64)> {
        let first = *points.first()?;
        let a0 = self.angle(first);
        let (mut lo, mut hi, mut least, mut far) = (0.0f64, 0.0f64, f64::INFINITY, 0.0f64);
        for p in points {
            if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
                return None;
            }
            let d = *p - self.origin;
            let r = d.dot(self.x).hypot(d.dot(self.y));
            if !(r > 0.0) {
                return None;
            }
            let t = wrap(self.angle(*p) - a0);
            if !t.is_finite() {
                return None;
            }
            (lo, hi, least, far) = (lo.min(t), hi.max(t), least.min(r), far.max(d.len() / r));
        }
        if !(hi - lo < PI) {
            return None;
        }
        let r = least * (0.5 * (hi - lo)).cos();
        let pad = gamma(16) * (PI + far) + if tol > 0.0 { tol / r } else { 0.0 };
        (pad.is_finite() && r > 0.0).then_some(([a0 + lo, a0 + hi], pad))
    }

    /// A vertex loop's point as a blocked arc: its angle, widened as [`Self::span`] widens one.
    fn point_arc(&self, p: Vec3, tol: f64) -> [f64; 2] {
        match self.span(&[p], tol) {
            Some((arc, pad)) => padded(arc, pad),
            None => [0.0, TAU],
        }
    }
}

/// How a Bézier piece of a loop meets the ruling at angle `theta` that a seam would run along
/// ([`Assembly::wound_band_seam`]).
pub(crate) enum Meets {
    /// Not at all: the ruling's angle is clear of the piece's, padded.
    Clear,
    /// At one point, `d` along the ruling from the vertex: `p`, at parameter `x` of the edge's form.
    Once { d: f64, p: Vec3, x: f64 },
    /// Anywhere over the heights of the piece's hull, or of the hull of the part of it that comes
    /// within its pad of the ruling's angle, from this far along the ruling (none where that hull
    /// lies behind the vertex by more than the tolerance).
    Hull(Option<f64>),
}

/// How `piece`, with its use's NURBS `form`, meets the ruling at angle `theta`, `height` giving a
/// point's height along the axis and `along` a height's distance along the ruling. A monotone
/// piece under half a turn meets it once where its angle reaches `theta` between its ends. One
/// within its pad of the ruling's angle without crossing it between its ends (a near miss), and
/// one whose angle is not monotone, may meet it anywhere over the heights of their hull: the
/// angle of the nearer end says nothing of where in between the piece comes near. A near miss
/// whose angle is monotone comes within its pad only between its nearer end and where its angle
/// is the pad away, past which it runs farther off: it may meet the ruling over the heights of
/// that part's hull alone ([`Piece::heights_between`]), the whole piece's where the part cannot
/// be cut out or the piece stays within its pad.
pub(crate) fn meets(
    frame: &Perpendicular,
    form: &Option<CurveEval>,
    piece: &Piece,
    theta: f64,
    height: impl Fn(Vec3) -> f64,
    along: impl Fn(f64) -> f64,
    tol: f64,
) -> R<Meets> {
    if (Blocked { arcs: vec![piece.blocked()] }).clear(theta) {
        return Ok(Meets::Clear);
    }
    let crossing = match (piece.runs, form) {
        (Some(_), Some(c)) => ruling_crossing(frame, c, piece.on, theta)?,
        _ => None,
    };
    Ok(match crossing {
        Some((true, p, x)) => Meets::Once { d: along(height(p)), p, x },
        near => {
            let within = match (near, form) {
                (Some((false, end, x)), Some(c)) => {
                    // Where the angle is the pad away from the ruling's, on the nearer end's side.
                    let side = wrap(frame.angle(end) - theta);
                    match ruling_crossing(frame, c, piece.on, theta + piece.pad.copysign(side))? {
                        Some((true, _, leaves)) => piece.heights_between(x, leaves, &height),
                        _ => None,
                    }
                }
                _ => None,
            };
            let [lo, hi] = within.unwrap_or(piece.heights).map(along);
            let (lo, hi) = (lo.min(hi), lo.max(hi));
            Meets::Hull((hi > -tol).then_some(lo.max(0.0)))
        }
    })
}

/// A use of a loop round a wound band ([`Assembly::wound_band_seam`]): its edge and way, its
/// edge's NURBS form ready to evaluate, and the form's pieces along the loop.
struct WoundUse {
    edge: u32,
    forward: bool,
    form: Option<CurveEval>,
    pieces: Vec<Piece>,
}

/// Where the ruling at angle `theta` meets a Bézier piece (parameters `on` of the NURBS form `c`)
/// whose angle is monotone and spans under half a turn: `(true, point, parameter)` where the
/// angle reaches `theta` between the piece's ends, else `(false, end, parameter)` with the end
/// whose angle is nearer: the ruling passes within the piece's pad of it, somewhere over its
/// hull's heights. The geometry is the kernel's ([`crate::kernel::angle_crossing`]).
fn ruling_crossing(frame: &Perpendicular, c: &CurveEval, on: [f64; 2], theta: f64) -> R<Option<(bool, Vec3, f64)>> {
    Ok(crate::kernel::angle_crossing(c, frame.origin, frame.x, frame.y, on, theta))
}

/// A Bézier piece of a loop's edge: the arc of angles about the axis it lies within, and the
/// way its angle runs along the loop (1 up, -1 down; `None` where it is not certainly one way).
pub(crate) struct Piece {
    arc: [f64; 2],
    /// How far its arc is widened where it blocks a seam ([`Perpendicular::span`]).
    pad: f64,
    runs: Option<i8>,
    /// Its parameters on the curve's NURBS form ([`Perpendicular::pieces`]), lowest first.
    on: [f64; 2],
    /// The heights along the axis its control points reach (its hull's), lowest first.
    heights: [f64; 2],
    /// Its Bézier control points on `on`, homogeneous with positive weights (none for an edge
    /// taken to reach the whole turn).
    points: Vec<[f64; 4]>,
}

impl Piece {
    /// The arc the piece blocks: its own, widened by its pad.
    pub(crate) fn blocked(&self) -> [f64; 2] {
        padded(self.arc, self.pad)
    }

    /// The heights (`height` of a point, lowest first) the part of the piece between parameters
    /// `a` and `b` of its form reaches: those of that part's own control points, the piece cut
    /// there as the kernel cuts a spline ([`solidcore::eval::pcurve::segment`], the piece being
    /// a Bézier curve on `on`). `None` where the piece has no control points or the part is
    /// empty or cannot be cut out.
    fn heights_between(&self, a: f64, b: f64, height: impl Fn(Vec3) -> f64) -> Option<[f64; 2]> {
        let degree = self.points.len().checked_sub(1).filter(|d| *d > 0)?;
        let [s, t] = self.on;
        let (a, b) = (a.min(b).max(s), a.max(b).min(t));
        if !(a < b) {
            return None;
        }
        let knots: Vec<f64> = std::iter::repeat_n(s, degree + 1).chain(std::iter::repeat_n(t, degree + 1)).collect();
        let (_, part) = solidcore::eval::pcurve::segment(degree, &knots, &self.points, a, b).ok()?;
        if part.len() != degree + 1 || part.iter().any(|h| !(h[3] > 0.0)) {
            return None;
        }
        let heights = part.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], h| {
            let z = height(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
            [lo.min(z), hi.max(z)]
        });
        heights.iter().all(|z| z.is_finite()).then_some(heights)
    }
}

/// `arc` widened by `pad` at both ends; the whole turn where that reaches round.
fn padded([a, b]: [f64; 2], pad: f64) -> [f64; 2] {
    if !(b - a + 2.0 * pad < TAU) {
        return [0.0, TAU];
    }
    [a - pad, b + pad]
}

/// The sign of the angular speed of the planar Bézier curve with homogeneous control points
/// `a` (x·w, y·w about the axis; the angle of the point is the angle of these): `A × A'`, a
/// polynomial of degree 2n − 1 whose Bernstein coefficients are
/// `Σ_{i+j=k} C(n,i) C(n−1,j) / C(2n−1,k) · n (a_i × (a_{j+1} − a_j))`. All of one strict sign:
/// the angle runs that way over the whole piece. A coefficient's sign counts only where it
/// exceeds its rounding, `γ_K` of the sum of its terms' magnitudes (a ruling's coefficients
/// are 0, computed as a rounding either way): otherwise `None`.
pub(crate) fn angular_sign(a: &[[f64; 2]]) -> Option<i8> {
    let n = a.len().checked_sub(1).filter(|n| *n > 0)?;
    let binomial = |n: usize, k: usize| -> f64 { (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64) };
    // Each coefficient: n + 1 terms of a binomial ratio (3n operations at most), a difference, a
    // cross product and the sum.
    let bound = gamma(6 * n + 8);
    let mut sign = 0i8;
    for k in 0..2 * n {
        let (mut c, mut size) = (0.0, 0.0);
        for i in 0..=n.min(k) {
            let j = k - i;
            if j > n - 1 {
                continue;
            }
            let (Some(&ai), Some(&aj), Some(&aj1)) = (a.get(i), a.get(j), a.get(j + 1)) else { return None };
            let f = binomial(n, i) * binomial(n - 1, j) / binomial(2 * n - 1, k) * n as f64;
            let (d0, d1) = (aj1[0] - aj[0], aj1[1] - aj[1]);
            c += f * (ai[0] * d1 - ai[1] * d0);
            size += f * (ai[0].abs() * (aj1[1].abs() + aj[1].abs()) + ai[1].abs() * (aj1[0].abs() + aj[0].abs()));
        }
        if !(c.is_finite() && size.is_finite()) {
            return None;
        }
        let s = if c > bound * size {
            1
        } else if c < -bound * size {
            -1
        } else {
            return None;
        };
        if sign != 0 && s != sign {
            return None;
        }
        sign = s;
    }
    Some(sign)
}

/// The pairs `(i, j)`, `i < j`, of pieces whose arcs may share an angle ([`overlap`]), found by a
/// sweep over the arcs' starts instead of every pair: each arc (under half a turn) is laid on the
/// line from its start taken in `[0, 2π)`, and once more a turn before and after, so two arcs
/// that meet across a whole turn meet there too; sorted by start, an arc meets those still open
/// when it starts (an end at its start included). Every pair [`overlap`] finds is among them; the
/// work is the sort and the pairs, not the square of the pieces.
fn overlapping_pairs(pieces: &[Piece]) -> Vec<(usize, usize)> {
    let mut laid: Vec<(f64, f64, usize)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let [a, b] = p.arc;
        let start = a.rem_euclid(TAU);
        if !(start.is_finite() && (b - a).is_finite()) {
            continue;
        }
        for shift in [-TAU, 0.0, TAU] {
            laid.push((start + shift, start + shift + (b - a), i));
        }
    }
    laid.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut open: Vec<(f64, usize)> = Vec::new();
    let mut pairs = std::collections::BTreeSet::new();
    for (start, end, i) in laid {
        open.retain(|(e, _)| *e >= start);
        for &(_, j) in &open {
            if i != j {
                pairs.insert((i.min(j), i.max(j)));
            }
        }
        open.push((end, i));
    }
    pairs.into_iter().collect()
}

/// The angles two arcs both reach (none, one arc, or two where they meet round both ends).
fn overlap(p: [f64; 2], q: [f64; 2]) -> Vec<[f64; 2]> {
    let w1 = p[1] - p[0];
    let s = (q[0] - p[0]).rem_euclid(TAU);
    let e = s + (q[1] - q[0]);
    let mut out = Vec::new();
    for shift in [0.0, -TAU] {
        let (lo, hi) = ((s + shift).max(0.0), (e + shift).min(w1));
        if lo <= hi {
            out.push([p[0] + lo, p[0] + hi]);
        }
    }
    out
}

/// Arcs of angle about an axis that a face's other loops can reach ([`Assembly::blocked_angles`]).
pub(crate) struct Blocked {
    pub(crate) arcs: Vec<[f64; 2]>,
}

impl Blocked {
    /// `angle` is outside every arc.
    pub(crate) fn clear(&self, angle: f64) -> bool {
        self.arcs.iter().all(|[a, b]| (angle - a).rem_euclid(TAU) > b - a)
    }

    /// The middle of the widest range of angles no arc reaches, if any is left (with no arcs,
    /// the frame's angle 0).
    fn widest_clear(&self) -> Option<f64> {
        if self.arcs.is_empty() {
            return Some(0.0);
        }
        // The arcs from 0 up, merged where they overlap; the gaps between them round the turn.
        let mut arcs: Vec<[f64; 2]> = self
            .arcs
            .iter()
            .map(|[a, b]| {
                let s = a.rem_euclid(TAU);
                [s, s + (b - a)]
            })
            .collect();
        arcs.sort_by(|x, y| x[0].total_cmp(&y[0]));
        let mut merged: Vec<[f64; 2]> = Vec::new();
        for [a, b] in arcs {
            match merged.last_mut() {
                Some(last) if a <= last[1] => last[1] = last[1].max(b),
                _ => merged.push([a, b]),
            }
        }
        // The last arc may run past a turn over the first ones.
        while merged.len() > 1 {
            let (Some(last), Some(head)) = (merged.last().copied(), merged.first().copied()) else { break };
            if last[1] - TAU < head[0] {
                break;
            }
            merged.pop();
            if let Some(h) = merged.first_mut() {
                *h = [last[0] - TAU, h[1].max(last[1] - TAU)];
            }
        }
        let first = *merged.first()?;
        let mut best: Option<(f64, f64)> = None;
        for (k, m) in merged.iter().enumerate() {
            let next = merged.get(k + 1).map_or(first[0] + TAU, |n| n[0]);
            let width = next - m[1];
            if width > 0.0 && best.is_none_or(|(w, _)| width > w) {
                best = Some((width, m[1] + 0.5 * width));
            }
        }
        best.map(|(_, middle)| middle)
    }
}
