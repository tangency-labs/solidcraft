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
//! linear in the record. Every distance is held to the record's own tolerance:
//! an edge's for its pcurves, an edge's and its vertex's together where they meet, a vertex's on
//! its surface. A point that cannot be evaluated is an error, never a panic.
//!
//! Origin: SolidCraft's `crates/geom/src/brep/geometry.rs` (MIT OR Apache-2.0), moved here with
//! its history (RFC 0003). It reads the record through a plain view of it ([`Edge`], [`Vertex`],
//! [`Face`]) in the kernel's evaluation types, which `solidcore-ops`' `record_check::agreement`
//! builds from a record.
use super::vec::{v3, VecExt};
use super::{Curve, Curve2, CurveEval, Surface, SurfaceEval};

/// An edge as the check reads it: the curve it is parameterised on (its NURBS form where it has
/// one), its interval there, its vertices (into the vertex list) and its tolerance.
#[derive(Clone, Copy, Debug)]
pub struct Edge<'a> {
    /// The curve the edge's interval is on.
    pub curve: &'a Curve,
    /// The edge's interval.
    pub interval: [f64; 2],
    /// Its start and end vertices.
    pub vertices: [u32; 2],
    /// Its tolerance.
    pub tolerance: f64,
}

/// A vertex: its point and tolerance.
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    /// The point.
    pub point: [f64; 3],
    /// Its tolerance.
    pub tolerance: f64,
}

/// One oriented edge of an edge loop: its edge (into the edge list), whether the loop runs along
/// it, and its pcurve.
#[derive(Clone, Copy, Debug)]
pub struct Use<'a> {
    /// Into the edge list.
    pub edge: u32,
    /// Whether the loop runs along the edge.
    pub forward: bool,
    /// The use's pcurve.
    pub pcurve: &'a Curve2,
}

/// A face's loop: a single vertex, or edge uses.
#[derive(Clone, Debug)]
pub enum Loop<'a> {
    /// A vertex loop (into the vertex list).
    Vertex(u32),
    /// An edge loop.
    Edges(Vec<Use<'a>>),
}

/// A face: the surface its parameters are on (its NURBS form where it has one), and its loops.
#[derive(Clone, Debug)]
pub struct Face<'a> {
    /// The surface the face's (u, v) are on.
    pub surface: &'a Surface,
    /// Its loops.
    pub loops: Vec<Loop<'a>>,
}

/// A record as the check reads it.
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    /// The edges.
    pub edges: &'a [Edge<'a>],
    /// The vertices.
    pub vertices: &'a [Vertex],
    /// The faces.
    pub faces: &'a [Face<'a>],
}

/// Where a record disagrees with itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
    /// `edges[edge]`.
    Edge(u32),
    /// `faces[face]`.
    Face(u32),
    /// `faces[face].loops[loop].edges[position]`.
    Use {
        /// The face.
        face: u32,
        /// The loop.
        r#loop: u32,
        /// The use's position in the loop.
        position: u32,
    },
    /// `faces[face].loops[loop]`, a vertex loop.
    VertexLoop {
        /// The face.
        face: u32,
        /// The loop.
        r#loop: u32,
    },
    /// `faces[face].loops[loop]`, an edge loop.
    Loop {
        /// The face.
        face: u32,
        /// The loop.
        r#loop: u32,
    },
}

impl std::fmt::Display for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Record::Edge(e) => write!(f, "edge {e}"),
            Record::Face(i) => write!(f, "face {i}"),
            Record::Use {
                face,
                r#loop,
                position,
            } => write!(f, "face {face} loop {loop} use {position}"),
            Record::VertexLoop { face, r#loop } | Record::Loop { face, r#loop } => {
                write!(f, "face {face} loop {loop}")
            }
        }
    }
}

/// What disagrees.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Problem {
    /// Two points that should coincide are `gap` mm apart, more than `tolerance`.
    #[error("{what} by {gap} mm (tolerance {tolerance} mm)")]
    Off {
        /// What should coincide.
        what: &'static str,
        /// How far apart.
        gap: f64,
        /// The tolerance.
        tolerance: f64,
    },
    /// A pcurve starts `gap` in a periodic parameter from where the use before it ends: on
    /// another period.
    #[error("pcurve starts {gap} from the end of the use before it, on another period (period {period})")]
    Period {
        /// The jump.
        gap: f64,
        /// The period.
        period: f64,
    },
    /// An edge loop's pcurves enclose no (u, v) area, within rounding of the loop's size: its
    /// face has no surface to it (a seam's two uses on one parameter line).
    #[error("loop encloses no (u, v) area (signed area {area})")]
    Empty {
        /// The signed area.
        area: f64,
    },
    /// An edge loop runs the wrong way round in (u, v), seen from the surface's natural normal:
    /// no loop of the face runs counter-clockwise (`outer`: the largest runs clockwise), or a
    /// second one does (an inner loop, the smaller of them).
    #[error("{} loop runs {} in (u, v) (signed area {area})", if *outer { "outer" } else { "inner" }, if *outer { "clockwise" } else { "counter-clockwise" })]
    Turn {
        /// Whether it is the outer loop.
        outer: bool,
        /// The signed area.
        area: f64,
    },
    /// A point of the record cannot be evaluated (an unsupported kind, a non-finite value).
    #[error("{0}")]
    Unevaluable(String),
}

/// A record whose geometry disagrees with its topology, and where.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("portable B-rep: {record}: {problem}")]
pub struct Mismatch {
    /// Where.
    pub record: Record,
    /// What.
    pub problem: Problem,
}

type Result<T> = std::result::Result<T, Mismatch>;

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}
fn unevaluable(record: Record, what: impl std::fmt::Display) -> Mismatch {
    Mismatch {
        record,
        problem: Problem::Unevaluable(what.to_string()),
    }
}
/// A use's pcurve at its edge's parameter `t`. The pcurve's interval is its edge's to within
/// rounding ([`Brep::validate`], `PCURVE_INTERVAL`), so an edge parameter just past it is read at
/// the pcurve's end.
fn on_pcurve(pc: &Curve2, t: f64) -> Option<[f64; 2]> {
    let [a, b] = pc.interval()?;
    super::pcurve::eval(pc, t.max(a.min(b)).min(a.max(b)))
}

/// `gap` within `tolerance` (a non-finite gap never is).
fn within(record: Record, what: &'static str, gap: f64, tolerance: f64) -> Result<()> {
    if gap <= tolerance {
        Ok(())
    } else {
        Err(Mismatch {
            record,
            problem: Problem::Off {
                what,
                gap,
                tolerance,
            },
        })
    }
}

/// Points of each use's pcurve that its loop's (u, v) area is taken over.
pub const AREA_SAMPLES: usize = 16;
/// Signed area this small against the square of the loop's (u, v) extent is none.
const EMPTY_AREA: f64 = 1e-9;

const PCURVE_POINTS: [&str; 3] = [
    "pcurve start lifts off its edge's curve",
    "pcurve middle lifts off its edge's curve",
    "pcurve end lifts off its edge's curve",
];

/// Check that the record's geometry agrees with its topology, within its tolerances (module
/// docs). Call on a record its structural validation accepted: this checks geometry, not
/// structure, though a malformed record is refused, never a panic.
///
/// # Errors
/// The first record that disagrees, and how.
pub fn check(brep: &View<'_>) -> Result<()> {
    // Each edge's curve at its interval's start, middle and end, shared by all its uses.
    let mut points = Vec::new();
    for (i, e) in brep.edges.iter().enumerate() {
        let at = Record::Edge(index(i));
        let curve = CurveEval::new(e.curve).map_err(|err| unevaluable(at, err))?;
        let [t0, t1] = e.interval;
        let p = [t0, 0.5 * (t0 + t1), t1].map(|t| curve.point(t).ok_or(t));
        let p = match p {
            [Ok(a), Ok(m), Ok(z)] => [a, m, z],
            [Err(t), ..] | [_, Err(t), _] | [.., Err(t)] => {
                return Err(unevaluable(at, format!("the curve has no point at {t}")))
            }
        };
        for (end, what) in [
            (0, "curve start misses its start vertex"),
            (1, "curve end misses its end vertex"),
        ] {
            let v = e
                .vertices
                .get(end)
                .and_then(|&v| brep.vertices.get(v as usize))
                .ok_or_else(|| unevaluable(at, "edge vertices"))?;
            let q = if end == 0 { p[0] } else { p[2] };
            within(at, what, q.dist(v3(v.point)), v.tolerance + e.tolerance)?;
        }
        points.push(p);
    }
    for (fi, f) in brep.faces.iter().enumerate() {
        let face = index(fi);
        let surface =
            SurfaceEval::new(f.surface).map_err(|err| unevaluable(Record::Face(face), err))?;
        let period = surface.domain().period;
        // Each edge loop's signed (u, v) area.
        let mut areas: Vec<(u32, f64)> = Vec::new();
        for (li, l) in f.loops.iter().enumerate() {
            let r#loop = index(li);
            match l {
                Loop::Vertex(vertex) => {
                    let at = Record::VertexLoop { face, r#loop };
                    let v = brep
                        .vertices
                        .get(*vertex as usize)
                        .ok_or_else(|| unevaluable(at, "vertex reference"))?;
                    let p = v3(v.point);
                    let on = surface
                        .project(p, None, f64::INFINITY)
                        .and_then(|q| surface.point(q));
                    let gap = on.map_or(f64::INFINITY, |s| s.dist(p));
                    within(at, "vertex lies off its face's surface", gap, v.tolerance)?;
                }
                Loop::Edges(edges) => {
                    // Where each use starts and ends in (u, v), in the loop's direction.
                    let mut ends: Vec<[[f64; 2]; 2]> = Vec::new();
                    for (position, u) in edges.iter().enumerate() {
                        let at = Record::Use {
                            face,
                            r#loop,
                            position: index(position),
                        };
                        let i = u.edge as usize;
                        let (e, p) = brep
                            .edges
                            .get(i)
                            .zip(points.get(i))
                            .ok_or_else(|| unevaluable(at, "edge reference"))?;
                        let [t0, t1] = e.interval;
                        let mut uv = [[0.0; 2]; 3];
                        for (k, t) in [t0, 0.5 * (t0 + t1), t1].into_iter().enumerate() {
                            let q = on_pcurve(u.pcurve, t).ok_or_else(|| {
                                unevaluable(at, format!("the pcurve has no point at {t}"))
                            })?;
                            let s = surface.point(q).ok_or_else(|| {
                                unevaluable(at, format!("the surface has no point at {q:?}"))
                            })?;
                            let (Some(c), Some(what), Some(slot)) =
                                (p.get(k), PCURVE_POINTS.get(k), uv.get_mut(k))
                            else {
                                return Err(unevaluable(at, "pcurve sample"));
                            };
                            within(at, what, s.dist(*c), e.tolerance)?;
                            *slot = q;
                        }
                        ends.push(if u.forward {
                            [uv[0], uv[2]]
                        } else {
                            [uv[2], uv[0]]
                        });
                    }
                    // Each use continues the one before it (the last closes the loop), except
                    // along a direction that collapses there (a pole): at the vertex where
                    // they meet, to within the vertex's tolerance.
                    for (position, pair) in ends.iter().enumerate() {
                        let before = ends.get(
                            position
                                .checked_sub(1)
                                .unwrap_or_else(|| ends.len().saturating_sub(1)),
                        );
                        let (Some([_, end]), [start, _]) = (before, pair) else {
                            continue;
                        };
                        let reach = edges
                            .get(position)
                            .and_then(|u| {
                                brep.edges.get(u.edge as usize).map(|e| {
                                    if u.forward {
                                        e.vertices[0]
                                    } else {
                                        e.vertices[1]
                                    }
                                })
                            })
                            .and_then(|v| brep.vertices.get(v as usize))
                            .map_or(0.0, |v| v.tolerance);
                        let pole = super::pcurve::pole_by_speed(
                            &surface,
                            &surface.domain(),
                            *start,
                            reach,
                        );
                        for d in 0..2 {
                            let (Some(Some(p)), Some(false)) = (period.get(d), pole.get(d)) else {
                                continue;
                            };
                            let (Some(a), Some(b)) = (start.get(d), end.get(d)) else {
                                continue;
                            };
                            let gap = (a - b).abs();
                            #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN gap is a jump
                            if !(gap < 0.5 * p) {
                                let record = Record::Use {
                                    face,
                                    r#loop,
                                    position: index(position),
                                };
                                return Err(Mismatch {
                                    record,
                                    problem: Problem::Period { gap, period: *p },
                                });
                            }
                        }
                    }
                    // The signed area the loop encloses (positive for the outer loop). Where
                    // uses meet at a pole the ring runs straight along the pole's line.
                    let at = Record::Loop { face, r#loop };
                    let mut ring: Vec<[f64; 2]> = Vec::new();
                    for u in edges {
                        let [t0, t1] = brep
                            .edges
                            .get(u.edge as usize)
                            .ok_or_else(|| unevaluable(at, "edge reference"))?
                            .interval;
                        for k in 0..AREA_SAMPLES {
                            let f = k as f64 / AREA_SAMPLES as f64;
                            let t = if u.forward {
                                t0 + (t1 - t0) * f
                            } else {
                                t1 - (t1 - t0) * f
                            };
                            ring.push(on_pcurve(u.pcurve, t).ok_or_else(|| {
                                unevaluable(at, format!("the pcurve has no point at {t}"))
                            })?);
                        }
                    }
                    let area = signed_area(&ring);
                    let extent = (0..2)
                        .map(|d| {
                            let (lo, hi) = ring.iter().fold(
                                (f64::INFINITY, f64::NEG_INFINITY),
                                |(a, b), q| {
                                    let x = q.get(d).copied().unwrap_or(f64::NAN);
                                    (a.min(x), b.max(x))
                                },
                            );
                            hi - lo
                        })
                        .fold(0.0, f64::max);
                    #[allow(clippy::neg_cmp_op_on_partial_ord)] // a NaN area is empty
                    if !(area.abs() > EMPTY_AREA * extent * extent) {
                        return Err(Mismatch {
                            record: at,
                            problem: Problem::Empty { area },
                        });
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
            0 => areas
                .into_iter()
                .max_by(by_size)
                .map(|(l, area)| (l, true, area)),
            1 => None,
            _ => ccw
                .into_iter()
                .min_by(by_size)
                .map(|(l, area)| (l, false, area)),
        };
        if let Some((r#loop, outer, area)) = wrong {
            return Err(Mismatch {
                record: Record::Loop { face, r#loop },
                problem: Problem::Turn { outer, area },
            });
        }
    }
    Ok(())
}

/// Signed area of a closed polygon in (u, v): positive counter-clockwise.
fn signed_area(ring: &[[f64; 2]]) -> f64 {
    0.5 * ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(p, q)| p[0] * q[1] - q[0] * p[1])
        .sum::<f64>()
}
