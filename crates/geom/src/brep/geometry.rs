//! Geometric agreement of a record (docs/native-brep.md, "Validation and trust"): what a kernel
//! assumes when it builds topology from the records without checking them itself. Each edge's
//! curve meets its vertices at its interval's ends; each oriented edge's pcurve, lifted through
//! its face's surface, lies on the edge's curve at the interval's ends and middle, and continues
//! the use before it without jumping a period; each edge loop's pcurves enclose (u, v) area,
//! one loop of each face counter-clockwise about the natural normal (the outer) and the rest
//! clockwise; each vertex loop's vertex lies on its surface.
//!
//! A spot check, not a certificate: a fixed number of evaluations per record (three points of
//! each edge, [`AREA_SAMPLES`] of each use, one projection per vertex loop), so the work is
//! linear in the item budget [`Brep::validate`] caps. Every distance is held to the record's own tolerance:
//! an edge's for its pcurves, an edge's and its vertex's together where they meet, a vertex's on
//! its surface. A point that cannot be evaluated is an error, never a panic.
use super::eval::{CurveEval, SurfaceEval};
use super::{Brep, Loop};
use crate::Vec3;

/// Where a record disagrees with itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
    /// `edges[edge]`.
    Edge(u32),
    /// `faces[face]`.
    Face(u32),
    /// `faces[face].loops[loop].edges[position]`.
    Use { face: u32, r#loop: u32, position: u32 },
    /// `faces[face].loops[loop]`, a vertex loop.
    VertexLoop { face: u32, r#loop: u32 },
    /// `faces[face].loops[loop]`, an edge loop.
    Loop { face: u32, r#loop: u32 },
}

impl std::fmt::Display for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Record::Edge(e) => write!(f, "edge {e}"),
            Record::Face(i) => write!(f, "face {i}"),
            Record::Use { face, r#loop, position } => write!(f, "face {face} loop {loop} use {position}"),
            Record::VertexLoop { face, r#loop } | Record::Loop { face, r#loop } => write!(f, "face {face} loop {loop}"),
        }
    }
}

/// What disagrees.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Problem {
    /// Two points that should coincide are `gap` mm apart, more than `tolerance`.
    #[error("{what} by {gap} mm (tolerance {tolerance} mm)")]
    Off { what: &'static str, gap: f64, tolerance: f64 },
    /// A pcurve starts `gap` in a periodic parameter from where the use before it ends: on
    /// another period.
    #[error("pcurve starts {gap} from the end of the use before it, on another period (period {period})")]
    Period { gap: f64, period: f64 },
    /// An edge loop's pcurves enclose no (u, v) area, within rounding of the loop's size: its
    /// face has no surface to it (a seam's two uses on one parameter line).
    #[error("loop encloses no (u, v) area (signed area {area})")]
    Empty { area: f64 },
    /// An edge loop runs the wrong way round in (u, v), seen from the surface's natural normal:
    /// no loop of the face runs counter-clockwise (`outer`: the largest runs clockwise), or a
    /// second one does (an inner loop, the smaller of them).
    #[error("{} loop runs {} in (u, v) (signed area {area})", if *outer { "outer" } else { "inner" }, if *outer { "clockwise" } else { "counter-clockwise" })]
    Turn { outer: bool, area: f64 },
    /// A point of the record cannot be evaluated (an unsupported kind, a non-finite value).
    #[error("{0}")]
    Unevaluable(String),
}

/// A record whose geometry disagrees with its topology, and where.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("portable B-rep: {record}: {problem}")]
pub struct Mismatch {
    pub record: Record,
    pub problem: Problem,
}

type Result<T> = std::result::Result<T, Mismatch>;

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}
fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
fn unevaluable(record: Record, what: impl std::fmt::Display) -> Mismatch {
    Mismatch { record, problem: Problem::Unevaluable(what.to_string()) }
}
/// `gap` within `tolerance` (a non-finite gap never is).
fn within(record: Record, what: &'static str, gap: f64, tolerance: f64) -> Result<()> {
    if gap <= tolerance { Ok(()) } else { Err(Mismatch { record, problem: Problem::Off { what, gap, tolerance } }) }
}

/// Points of each use's pcurve that its loop's (u, v) area is taken over.
pub const AREA_SAMPLES: usize = 16;
/// Signed area this small against the square of the loop's (u, v) extent is none.
const EMPTY_AREA: f64 = 1e-9;

const PCURVE_POINTS: [&str; 3] =
    ["pcurve start lifts off its edge's curve", "pcurve middle lifts off its edge's curve", "pcurve end lifts off its edge's curve"];

impl Brep {
    /// Check that the record's geometry agrees with its topology, within its tolerances (module
    /// docs). Call on a record [`Brep::validate`] accepted: this checks geometry, not structure,
    /// though a malformed record is refused, never a panic.
    pub fn check_geometry(&self) -> Result<()> {
        // Each edge's curve at its interval's start, middle and end, shared by all its uses.
        let mut points = Vec::with_capacity(self.edges.len());
        for (i, e) in self.edges.iter().enumerate() {
            let at = Record::Edge(index(i));
            let curve = CurveEval::new(&e.curve).map_err(|err| unevaluable(at, err))?;
            let [t0, t1] = e.interval;
            let p = [t0, 0.5 * (t0 + t1), t1].map(|t| curve.point(t).ok_or(t));
            let p = match p {
                [Ok(a), Ok(m), Ok(z)] => [a, m, z],
                [Err(t), ..] | [_, Err(t), _] | [.., Err(t)] => return Err(unevaluable(at, format!("the curve has no point at {t}"))),
            };
            for (end, what) in [(0, "curve start misses its start vertex"), (1, "curve end misses its end vertex")] {
                let v = e.vertices.get(end).and_then(|&v| self.vertices.get(v as usize)).ok_or_else(|| unevaluable(at, "edge vertices"))?;
                let q = if end == 0 { p[0] } else { p[2] };
                within(at, what, q.dist(v3(v.point)), v.tolerance + e.tolerance)?;
            }
            points.push(p);
        }
        for (fi, f) in self.faces.iter().enumerate() {
            let face = index(fi);
            let surface = SurfaceEval::new(&f.surface).map_err(|err| unevaluable(Record::Face(face), err))?;
            let period = surface.domain().period;
            // Each edge loop's signed (u, v) area.
            let mut areas: Vec<(u32, f64)> = Vec::with_capacity(f.loops.len());
            for (li, l) in f.loops.iter().enumerate() {
                let r#loop = index(li);
                match l {
                    Loop::Vertex { vertex } => {
                        let at = Record::VertexLoop { face, r#loop };
                        let v = self.vertices.get(*vertex as usize).ok_or_else(|| unevaluable(at, "vertex reference"))?;
                        let p = v3(v.point);
                        let on = surface.project(p, None, f64::INFINITY).and_then(|q| surface.point(q));
                        let gap = on.map_or(f64::INFINITY, |s| s.dist(p));
                        within(at, "vertex lies off its face's surface", gap, v.tolerance)?;
                    }
                    Loop::Edges { edges } => {
                        // Where each use starts and ends in (u, v), in the loop's direction.
                        let mut ends: Vec<[[f64; 2]; 2]> = Vec::with_capacity(edges.len());
                        for (position, u) in edges.iter().enumerate() {
                            let at = Record::Use { face, r#loop, position: index(position) };
                            let i = u.edge as usize;
                            let (e, p) = self.edges.get(i).zip(points.get(i)).ok_or_else(|| unevaluable(at, "edge reference"))?;
                            let [t0, t1] = e.interval;
                            let mut uv = [[0.0; 2]; 3];
                            for (k, t) in [t0, 0.5 * (t0 + t1), t1].into_iter().enumerate() {
                                let q = u.pcurve.eval(t).ok_or_else(|| unevaluable(at, format!("the pcurve has no point at {t}")))?;
                                let s = surface.point(q).ok_or_else(|| unevaluable(at, format!("the surface has no point at {q:?}")))?;
                                let (Some(c), Some(what), Some(slot)) = (p.get(k), PCURVE_POINTS.get(k), uv.get_mut(k)) else {
                                    return Err(unevaluable(at, "pcurve sample"));
                                };
                                within(at, what, s.dist(*c), e.tolerance)?;
                                *slot = q;
                            }
                            ends.push(if u.forward { [uv[0], uv[2]] } else { [uv[2], uv[0]] });
                        }
                        // Each use continues the one before it (the last closes the loop), except
                        // along a direction that collapses there (a pole).
                        for (position, pair) in ends.iter().enumerate() {
                            let before = ends.get(position.checked_sub(1).unwrap_or(ends.len().saturating_sub(1)));
                            let (Some([_, end]), [start, _]) = (before, pair) else { continue };
                            let pole = surface.pole(*start);
                            for d in 0..2 {
                                let (Some(Some(p)), Some(false)) = (period.get(d), pole.get(d)) else { continue };
                                let gap = (start[d] - end[d]).abs();
                                if !(gap < 0.5 * p) {
                                    let record = Record::Use { face, r#loop, position: index(position) };
                                    return Err(Mismatch { record, problem: Problem::Period { gap, period: *p } });
                                }
                            }
                        }
                        // The signed area the loop encloses (positive for the outer loop). Where
                        // uses meet at a pole the ring runs straight along the pole's line.
                        let at = Record::Loop { face, r#loop };
                        let mut ring: Vec<[f64; 2]> = Vec::with_capacity(edges.len().saturating_mul(AREA_SAMPLES));
                        for u in edges {
                            let [t0, t1] = self.edges.get(u.edge as usize).ok_or_else(|| unevaluable(at, "edge reference"))?.interval;
                            for k in 0..AREA_SAMPLES {
                                let f = k as f64 / AREA_SAMPLES as f64;
                                let t = if u.forward { t0 + (t1 - t0) * f } else { t1 - (t1 - t0) * f };
                                ring.push(u.pcurve.eval(t).ok_or_else(|| unevaluable(at, format!("the pcurve has no point at {t}")))?);
                            }
                        }
                        let area = signed_area(&ring);
                        let extent = (0..2)
                            .map(|d| {
                                let (lo, hi) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[d]), b.max(q[d])));
                                hi - lo
                            })
                            .fold(0.0, f64::max);
                        if !(area.abs() > EMPTY_AREA * extent * extent) {
                            return Err(Mismatch { record: at, problem: Problem::Empty { area } });
                        }
                        areas.push((r#loop, area));
                    }
                }
            }
            // One loop runs counter-clockwise, the outer; every other one clockwise. Which is
            // which comes from the areas, not the loops' order: the truck adapter writes a face's
            // wires in truck's order, which need not put the outer one first.
            let by_size = |a: &(u32, f64), b: &(u32, f64)| a.1.abs().total_cmp(&b.1.abs());
            let ccw: Vec<(u32, f64)> = areas.iter().copied().filter(|a| a.1 > 0.0).collect();
            let wrong = match ccw.len() {
                0 => areas.into_iter().max_by(by_size).map(|(l, area)| (l, true, area)),
                1 => None,
                _ => ccw.into_iter().min_by(by_size).map(|(l, area)| (l, false, area)),
            };
            if let Some((r#loop, outer, area)) = wrong {
                return Err(Mismatch { record: Record::Loop { face, r#loop }, problem: Problem::Turn { outer, area } });
            }
        }
        Ok(())
    }
}

/// Signed area of a closed polygon in (u, v): positive counter-clockwise.
fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    0.5 * (0..n).filter_map(|i| Some((ring.get(i)?, ring.get((i + 1) % n)?))).map(|(p, q)| p[0] * q[1] - q[0] * p[1]).sum::<f64>()
}
