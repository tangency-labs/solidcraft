//! Seam edges for faces that wind around a periodic surface without one. STEP lets a face on a
//! cylinder, cone or sphere be bounded by loops that go once around the axis (a band between
//! two circles, a cap bounded by one circle, a cone bounded by its base and a vertex loop at its
//! apex); schema 4 keeps every loop closed in the surface's (u, v), so such a face gets a seam:
//! a ruling line on a cylinder or cone, a meridian arc on a sphere, from a vertex of one loop to
//! the other loop (split there when it has no vertex at that angle) or to the pole the face
//! covers; on a surface of revolution, its profile turned to the vertex's angle. The loops merge into one: the first, the seam out, the second (or nothing at a pole),
//! the seam back. A face on a B-spline surface that closes on itself around u (its first and
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
//! ([`Builder::complete_slits`]), so the loop bounds the surface's whole (u, v) domain. A slit that
//! is only one bound of a face with another loop encloses nothing of it either, and is left out
//! of the face ([`Builder::drop_inner_slits`]).

use crate::vec::Vec3Geom as _;
use std::f64::consts::{PI, TAU};

use crate::kernel::{CurveEval, SurfaceEval};
use solidcore::nurbs::math::gamma;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve as BCurve, Frame, Surface as BSurface, Vertex};

use super::{Builder, EdgeB, LoopB, R, index, unsupported};

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

/// The path a slit runs out along: a loop that runs out along a chain of edges and straight back
/// along it, each edge used once each way, started at one of its two turns; `None` for any
/// other loop.
pub(super) fn slit_path(es: &[(u32, bool)]) -> Option<Vec<(u32, bool)>> {
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

impl Builder<'_, '_> {
    /// Start every B-spline surface closed around u that a face lies across the start of, without
    /// winding round it, in the middle of the widest stretch of u the face leaves free: the face's
    /// pcurves then run inside the surface's knots, as a kernel's trims have to (one crossing the
    /// start had its pcurve clamped there, mm off its edge). Exact ([`restarted`]). Returns how
    /// many surfaces were started anew.
    pub(super) fn start_splines_clear_of_faces(&mut self) -> R<usize> {
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
            // widest loop bounds the face.
            let mut widest: Option<(f64, f64)> = None;
            let mut winds = false;
            for l in &face.loops {
                let LoopB::Edges(es) = l else { continue };
                winds |= self.winding(&eval, es, [Some(period), None], &poles)?[0] != 0;
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
                if hi - lo > widest.map_or(f64::NEG_INFINITY, |(a, b)| b - a) {
                    widest = Some((lo, hi));
                }
            }
            let Some((lo, hi)) = widest else { continue };
            // Across the start: a whole number of periods from it lies inside the reach, clear of
            // its ends (a face that only meets the start there runs inside the knots already).
            let start = u0 + ((hi - u0) / period).floor() * period;
            let clear = 1e-9 * period;
            let crossing = hi - start > clear && start - lo > clear;
            if winds || !crossing || !(hi - lo < period) {
                continue;
            }
            // The middle of the stretch the face leaves free.
            let free = hi + 0.5 * (period - (hi - lo));
            let at = u0 + (free - u0).rem_euclid(period);
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
    pub(super) fn add_seams(&mut self) -> R<usize> {
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
                let LoopB::Edges(es) = l else { continue };
                let mut w = self.winding(&eval, es, periods, &poles)?;
                if v_turns && w[0] == 0 && w[1].abs() == 1 {
                    tube = true;
                    w = [w[1], 0];
                }
                if w[1] != 0 || w[0].abs() > 1 {
                    return Err(format!("face #{}: {}", face.step, unsupported("a loop winding around a surface more than once or in v")));
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
            let step = face.step;
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
    /// where a loop may run along the pole's line. Returns how many edges were split.
    pub(super) fn split_at_poles(&mut self) -> R<usize> {
        let mut splits = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let Some(ax) = axis(&face.surface) else { continue };
            let poles: Vec<Vec3> = ax.poles.iter().flatten().copied().collect();
            if poles.is_empty() {
                continue;
            }
            let mut edges: Vec<u32> =
                face.loops.iter().filter_map(|l| if let LoopB::Edges(es) = l { Some(es) } else { None }).flatten().map(|u| u.0).collect();
            edges.sort_unstable();
            edges.dedup();
            for e in edges {
                for pole in &poles {
                    let Some(t) = self.through_at(e, *pole)? else { continue };
                    splits += 1;
                    if splits > MAX_SPLITS {
                        return Err(format!("more than {MAX_SPLITS} edges split at poles"));
                    }
                    let v = self.split_edge(e, t)?;
                    // The new vertex is the pole, as the face's parameters have it.
                    if let Some(x) = self.vertices.get_mut(v as usize) {
                        let gap = Vec3::from(x.point).dist(*pole);
                        x.point = arr(*pole);
                        x.tolerance = x.tolerance.max(1.5 * gap);
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
    fn winding(&self, eval: &SurfaceEval, es: &[(u32, bool)], periods: [Option<f64>; 2], poles: &[Vec3]) -> R<[i64; 2]> {
        let mut last: Option<[f64; 2]> = None;
        let mut total = [0.0; 2];
        for &(e, forward) in es {
            let edge = self.edges.get(e as usize).ok_or("edge reference")?;
            let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
            let [lo, hi] = edge.interval;
            let reach = self.tol.max(edge.tolerance);
            for k in 0..=SAMPLES {
                let s = k as f64 / SAMPLES as f64;
                let t = if forward { lo + (hi - lo) * s } else { hi - (hi - lo) * s };
                let p = c.point(t).ok_or("edge point")?;
                if poles.iter().any(|q| p.dist(*q) <= reach) {
                    continue;
                }
                let Some(q) = eval.project(p, last, f64::INFINITY) else { continue };
                if let Some(l) = last {
                    for (((t, period), q), l) in total.iter_mut().zip(periods).zip(q).zip(l) {
                        if let Some(p) = period {
                            *t += wrap_by(q - l, p) / p;
                        }
                    }
                }
                last = Some(q);
            }
        }
        Ok(total.map(|x| x.round() as i64))
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
        let Some(LoopB::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        Ok(es.iter().skip(k).chain(es.iter().take(k)).copied().collect())
    }

    /// A seam edge from vertex `a` to vertex `b` on the surface: a ruling line, a meridian arc
    /// of a sphere, or a revolution's profile turned to their angle. Returns the edge and
    /// whether it runs from `a` to `b`.
    fn seam_edge(&mut self, a: u32, b: u32, ax: &Axis, eval: &SurfaceEval, check: bool) -> R<(u32, bool)> {
        let (pa, pb) = (self.vpoint(a)?, self.vpoint(b)?);
        if pa.dist(pb) <= self.tol {
            return Err("a seam of no length".into());
        }
        let (curve, interval, along) = match &ax.kind {
            Kind::Ruled => (BCurve::Line { start: arr(pa), end: arr(pb) }, [0.0, 1.0], true),
            Kind::Sphere(r) => {
                // The sphere's own meridian through the end further from the axis: angle 0 on the
                // equator, π/2 at the upper pole, so its angle is the surface's latitude and its
                // quarters the surface's.
                let (da, db) = (pa - ax.origin, pb - ax.origin);
                let (ra, rb) = (da - ax.z * da.dot(ax.z), db - ax.z * db.dot(ax.z));
                let x = if ra.len() >= rb.len() { ra } else { rb }.normalized().ok_or("a seam along the sphere's axis")?;
                let latitude = |d: Vec3| d.dot(ax.z).atan2(d.dot(x));
                let (ta, tb) = (latitude(da), latitude(db));
                if !(ta.abs() <= 0.5 * PI + 1e-9 && tb.abs() <= 0.5 * PI + 1e-9) {
                    return Err(unsupported("a seam between loops at opposite angles"));
                }
                let along = ta < tb;
                (
                    BCurve::Circle { frame: Frame { origin: arr(ax.origin), x_axis: arr(x), y_axis: arr(ax.z) }, radius: *r },
                    if along { [ta, tb] } else { [tb, ta] },
                    along,
                )
            }
            Kind::Torus { major, minor } => {
                // The tube circle at `a`'s angle about the axis: centred on the ring there, angle
                // 0 outward and π/2 up the axis, so its angle is the surface's tube angle. The
                // seam runs from the loop the face lies above (`a`'s, running +u) up the tube to
                // `b`: the face's side of the circle, whichever half that is.
                let da = pa - ax.origin;
                let x = (da - ax.z * da.dot(ax.z)).normalized().ok_or("a torus seam on its axis")?;
                let centre = ax.origin + x * *major;
                let tube = |p: Vec3| {
                    let d = p - centre;
                    d.dot(ax.z).atan2(d.dot(x))
                };
                let (ta, tb) = (tube(pa), tube(pb));
                let up = (tb - ta).rem_euclid(TAU);
                if !(up > 1e-9) {
                    return Err("a seam of no length".into());
                }
                (BCurve::Circle { frame: Frame { origin: arr(centre), x_axis: arr(x), y_axis: arr(ax.z) }, radius: *minor }, [ta, ta + up], true)
            }
            Kind::TorusTube => {
                // The circle about the axis through `a`: angle 0 at `a`, growing with the
                // torus's u. The face lies to the left of its loops in (u, v), so from the loop
                // running round the tube up (+v) it lies towards −u: the seam runs from `a` back
                // round the axis to `b`, the edge from `b` to `a`.
                let da = pa - ax.origin;
                let h = da.dot(ax.z);
                let radial = da - ax.z * h;
                let r = radial.len();
                let x = radial.normalized().ok_or("a torus seam on its axis")?;
                let y = ax.z.cross(x);
                let db = pb - ax.origin;
                let back = (-(db.dot(y).atan2(db.dot(x)))).rem_euclid(TAU);
                if !(back > 1e-9) {
                    return Err("a seam of no length".into());
                }
                let centre = ax.origin + ax.z * h;
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
        self.edges.push(EdgeB { curve, interval, vertices, tolerance: self.tol, form: None });
        Ok(i)
    }

    /// Leave out of each face its bounds that are slits (a loop out along edges and straight back,
    /// of no (u, v) area) when the face has a bound that is not one, and no other face uses the
    /// slit's edges: the slit lies inside the face and takes nothing from it, but the portable
    /// B-rep refuses a loop of no area, and with it the face and the solid. Returns how many
    /// bounds were left out.
    pub(super) fn drop_inner_slits(&mut self) -> usize {
        let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for face in &self.faces {
            for l in &face.loops {
                if let LoopB::Edges(es) = l {
                    for (e, _) in es {
                        *uses.entry(*e).or_default() += 1;
                    }
                }
            }
        }
        let alone = |l: &LoopB| match l {
            LoopB::Edges(es) => es.len() >= 2 && slit_path(es).is_some() && es.iter().all(|(e, _)| uses.get(e).copied() == Some(2)),
            LoopB::Vertex(_) => false,
        };
        let mut dropped = 0;
        for face in &mut self.faces {
            let slit: Vec<bool> = face.loops.iter().enumerate().map(|(i, l)| face.outer != Some(i) && alone(l)).collect();
            let count = slit.iter().filter(|s| **s).count();
            // A face of slits alone is a whole closed surface ([`Builder::complete_slits`]).
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
    pub(super) fn complete_slits(&mut self) -> R<usize> {
        let mut completed = 0;
        for fi in 0..self.faces.len() {
            let Some(face) = self.faces.get(fi) else { continue };
            let [LoopB::Edges(es)] = face.loops.as_slice() else { continue };
            let Some(path) = slit_path(es) else { continue };
            let (step, surface) = (face.step, face.surface.clone());
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
            if let Some(LoopB::Edges(es)) = face.loops.first_mut() {
                *es = merged;
            }
            completed += 1;
        }
        Ok(completed)
    }

    /// The slit made the meridian seam of `surface` when it is a surface of revolution closed
    /// at both poles ([`Builder::meridian_slit`]); `None` for any other surface.
    fn closed_at_poles(&mut self, path: &[(u32, bool)], surface: &BSurface) -> R<Option<Option<Vec<(u32, bool)>>>> {
        let eval = SurfaceEval::new(surface).map_err(|e| e.to_string())?;
        match axis(surface).or_else(|| spline_axis(surface, &eval)) {
            // Meridians run in v: a spline closed in v has none.
            Some(ax) if ax.kind.winds_in() == 0 && ax.poles.iter().all(Option::is_some) => Ok(Some(self.meridian_slit(path, &ax, &eval)?)),
            _ => Ok(None),
        }
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
        let mut u_ref = match ax.kind {
            Kind::Iso { start, .. } => Some(start),
            _ => None,
        };
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
                let u = *u_ref.get_or_insert(q[0]);
                if !eval.point([u, q[1]]).is_some_and(|m| m.dist(p) <= reach) {
                    return Ok(false);
                }
            }
        }
        Ok(u_ref.is_some())
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
            let (seam, along) = self.seam_edge(a, s, ax, eval, true)?;
            merged.push((seam, !along));
        }
        merged.extend(up);
        if !self.at(b, north)? {
            let n = pole_vertex(self, north)?;
            merged.push(self.seam_edge(b, n, ax, eval, true)?);
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
        let step = self.faces.get(fi).map_or(0, |f| f.step);
        let Some(p) = pole else {
            return Err(format!("face #{step}: a face around an open surface of revolution with one bound"));
        };
        let Some(LoopB::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
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
            _ => k,
        };
        let ring = self.rotated(fi, li, k)?;
        let a = self.start(*ring.first().ok_or("empty loop")?)?;
        // The face's vertex loop at the pole, if it has one, else a new vertex there.
        let face = self.faces.get(fi).ok_or("face reference")?;
        let at_pole = face.loops.iter().enumerate().find_map(|(i, l)| match l {
            LoopB::Vertex(v) => self.vertices.get(*v as usize).filter(|x| Vec3::from(x.point).dist(p) <= self.tol.max(x.tolerance)).map(|_| (i, *v)),
            LoopB::Edges(_) => None,
        });
        let b = match at_pole {
            Some((_, v)) => v,
            None => {
                let v = index(self.vertices.len())?;
                self.vertices.push(Vertex { point: arr(p), tolerance: self.tol });
                v
            }
        };
        let (seam, along) = self.seam_edge(a, b, ax, eval, false)?;
        let mut merged = ring;
        merged.push((seam, along));
        merged.push((seam, !along));
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        if let Some(l) = face.loops.get_mut(li) {
            *l = LoopB::Edges(merged);
        }
        if let Some((i, _)) = at_pole {
            remove_loop(face, i, li);
        }
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
        let (Some(LoopB::Edges(a_es)), Some(LoopB::Edges(b_es))) =
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

    /// Join loops `la` and `lb` of face `fi`, from their uses `i` and `j`, by a seam between
    /// the vertices those uses start at: loop A, the seam out, loop B, the seam back.
    #[allow(clippy::too_many_arguments)]
    fn join_by_seam(&mut self, fi: usize, la: usize, lb: usize, i: usize, j: usize, ax: &Axis, eval: &SurfaceEval) -> R<()> {
        let a_ring = self.rotated(fi, la, i)?;
        let b_ring = self.rotated(fi, lb, j)?;
        let va = self.start(*a_ring.first().ok_or("empty loop")?)?;
        let vb = self.start(*b_ring.first().ok_or("empty loop")?)?;
        let (seam, along) = self.seam_edge(va, vb, ax, eval, true)?;
        let mut merged = a_ring;
        merged.push((seam, along));
        merged.extend(b_ring);
        merged.push((seam, !along));
        let face = self.faces.get_mut(fi).ok_or("face reference")?;
        if let Some(l) = face.loops.get_mut(la) {
            *l = LoopB::Edges(merged);
        }
        remove_loop(face, lb, la);
        Ok(())
    }

    /// The parameter inside edge `e`'s interval where its curve reaches the value `target` of
    /// the surface's parameter `d` (0: u, an angle about the axis; 1: v), if it does.
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
                let t = 0.5 * (a + b);
                let eps = 1e-9 * (hi - lo);
                return Ok((t > lo + eps && t < hi - eps).then_some(t));
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
        let Some(LoopB::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
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
                LoopB::Vertex(v) => {
                    arcs.push(frame.point_arc(self.vpoint(*v)?, self.tol));
                    continue;
                }
                LoopB::Edges(es) => es,
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
            for (i, p) in pieces.iter().enumerate() {
                if p.runs.is_none() {
                    arcs.push(p.blocked());
                }
                for (j, q) in pieces.iter().enumerate().skip(i + 1) {
                    let next = j == i + 1 || (i == 0 && j + 1 == n);
                    if next && p.runs.is_some() && p.runs == q.runs {
                        continue;
                    }
                    arcs.extend(overlap(p.arc, q.arc).into_iter().map(|a| padded(a, p.pad.max(q.pad))));
                }
            }
        }
        Ok(Some(Blocked { arcs }))
    }

    /// The use of loop `li` of face `fi` that starts on the closed spline's boundary line at
    /// `start` (the column at u = `start`, or the row at v = `start` for one closed in v): a vertex there
    /// within the tolerance, else one made by splitting the loop's edge that crosses it.
    fn seam_start(&mut self, fi: usize, li: usize, start: f64, ax: &Axis, eval: &SurfaceEval) -> R<usize> {
        let Some(LoopB::Edges(es)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
        let es = es.clone();
        let d = ax.kind.winds_in();
        let on_seam = |s: &Self, v: u32| -> R<bool> {
            let p = s.vpoint(v)?;
            let Some(q) = eval.project(p, None, f64::INFINITY) else { return Ok(false) };
            let on = if d == 0 { [start, q[1]] } else { [q[0], start] };
            Ok(eval.point(on).is_some_and(|c| c.dist(p) <= s.tol))
        };
        for (k, u) in es.iter().enumerate() {
            if on_seam(self, self.start(*u)?)? {
                return Ok(k);
            }
        }
        for &(e, _) in &es {
            if let Some(t) = self.crossing(e, d, start, ax.period, eval)? {
                let v = self.split_edge(e, t)?;
                let Some(LoopB::Edges(now)) = self.faces.get(fi).and_then(|f| f.loops.get(li)) else { return Err("loop reference".into()) };
                for (k, u) in now.iter().enumerate() {
                    if self.start(*u)? == v {
                        return Ok(k);
                    }
                }
                return Err("the split vertex is not on its loop".into());
            }
        }
        Err(unsupported("a loop around a closed spline that never reaches its boundary line"))
    }

    /// Split edge `e` at `t` inside its interval: it keeps [min, t], a new edge takes
    /// [t, max], and every loop using it uses both. Returns the new vertex.
    fn split_edge(&mut self, e: u32, t: f64) -> R<u32> {
        let edge = self.edges.get(e as usize).ok_or("edge reference")?;
        let c = CurveEval::new(&edge.curve).map_err(|m| m.to_string())?;
        let p = c.point(t).ok_or("split point")?;
        let (curve, [lo, hi], [a, b], tolerance) = (edge.curve.clone(), edge.interval, edge.vertices, edge.tolerance);
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
        self.edges.push(EdgeB { curve, interval: [t, hi], vertices: [v, b], tolerance, form: None });
        for f in &mut self.faces {
            for l in &mut f.loops {
                let LoopB::Edges(es) = l else { continue };
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

/// Directions square to an axis, to measure angles about it.
pub(super) struct Perpendicular {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
}

impl Perpendicular {
    fn new(ax: &Axis) -> Self {
        Self::about(ax.origin, ax.z)
    }

    /// Directions square to the axis through `origin` along unit `z`.
    pub(super) fn about(origin: Vec3, z: Vec3) -> Self {
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
    pub(super) fn pieces(&self, curve: &BCurve, interval: [f64; 2], tol: f64) -> Vec<Piece> {
        let whole = || vec![Piece { arc: [0.0, TAU], pad: 0.0, runs: None }];
        let form = match curve {
            BCurve::Spline { .. } | BCurve::RationalSpline { .. } => Some((curve.clone(), interval)),
            other => crate::kernel::form::curve(other, interval),
        };
        let Some((form, [a, b])) = form else { return whole() };
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
            out.push(Piece { arc, pad, runs: angular_sign(&flat) });
        }
        out
    }

    /// The arc of angles `[from, to]` (to − from under π) that points spanning less than half a
    /// turn reach, measured from the first, and how far to widen it where it blocks a seam: its
    /// rounding (each angle's `atan2` and coordinates, `γ` of the operations at the points'
    /// distance over their radius) and `tol` at the smallest radius the points' hull reaches (the
    /// least radius times the cosine of half the arc, as their convex hull lies within the wedge).
    /// `None` when a point is not finite or on the axis, or they reach half a turn.
    pub(super) fn span(&self, points: &[Vec3], tol: f64) -> Option<([f64; 2], f64)> {
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

/// A Bézier piece of a loop's edge: the arc of angles about the axis it lies within, and the
/// way its angle runs along the loop (1 up, -1 down; `None` where it is not certainly one way).
pub(super) struct Piece {
    arc: [f64; 2],
    /// How far its arc is widened where it blocks a seam ([`Perpendicular::span`]).
    pad: f64,
    runs: Option<i8>,
}

impl Piece {
    /// The arc the piece blocks: its own, widened by its pad.
    pub(super) fn blocked(&self) -> [f64; 2] {
        padded(self.arc, self.pad)
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
pub(super) fn angular_sign(a: &[[f64; 2]]) -> Option<i8> {
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

/// Arcs of angle about an axis that a face's other loops can reach ([`Builder::blocked_angles`]).
pub(super) struct Blocked {
    pub(super) arcs: Vec<[f64; 2]>,
}

impl Blocked {
    /// `angle` is outside every arc.
    pub(super) fn clear(&self, angle: f64) -> bool {
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
