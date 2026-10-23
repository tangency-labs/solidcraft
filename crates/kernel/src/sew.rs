//! Sewing a solid from face descriptions: each face is its boundary loops (lines, circular
//! arcs and Bézier chains, counter-clockwise about the outward normal; holes clockwise)
//! and its surface (planar, revolved from a line, extruded from an arc, or ruled between two
//! Bézier chains). Vertices and edges at the same places are shared, so the faces close into
//! one shell without booleans — for solids whose faces are known exactly, such as folded
//! sheet metal.
//!
//! The faces are assembled into a schema 4 record with exactly the topology the descriptions
//! give ([`assemble`]: shared edges, shells, regions, canonical order), which SMLib builds the
//! body from. No tolerance stitching is involved: two sheets of material meeting along one
//! geometric edge keep an edge each.

use std::collections::HashMap;
use std::sync::Arc;

use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::brep::{self as b, Curve2, pcurve};
use solidcraft_geom::{Ring, Vec3};

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// Most control points of one Bézier chain edge.
pub const MAX_CHAIN_POINTS: usize = 3 * 1024 + 1;
/// Highest degree of a Bézier chain's pieces.
pub const MAX_CHAIN_DEGREE: usize = 9;

/// A boundary edge.
#[derive(Clone, Debug, PartialEq)]
pub enum EdgeSpec {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// A circular arc from `a` to `b` through `mid`.
    Arc {
        a: Vec3,
        b: Vec3,
        mid: Vec3,
    },
    /// A chain of Bézier pieces of degree `degree` (1…[`MAX_CHAIN_DEGREE`]): `degree`·n + 1
    /// control points (n ≥ 1); the curve passes through `ctrl[0]`, `ctrl[degree]`, …
    /// `ctrl[degree·n]`. For curves with no exact line, arc or conic form, such as a font's
    /// outline (cubic) or a cut-out edge traced across a sheet metal bend.
    Chain {
        degree: usize,
        ctrl: Arc<[Vec3]>,
    },
}

impl EdgeSpec {
    fn ends(&self) -> (Vec3, Vec3) {
        match self {
            EdgeSpec::Line { a, b } | EdgeSpec::Arc { a, b, .. } => (*a, *b),
            EdgeSpec::Chain { ctrl, .. } => (ctrl.first().copied().unwrap_or(Vec3::ZERO), ctrl.last().copied().unwrap_or(Vec3::ZERO)),
        }
    }

    /// The way the edge leaves its start (`None` when that cannot be told).
    fn start_dir(&self) -> Option<Vec3> {
        let d = match self {
            EdgeSpec::Line { a, b } => *b - *a,
            // The circle through a, mid and b, inverted about a, is the line through the images
            // of mid and b, which runs along the circle's tangent at a.
            EdgeSpec::Arc { a, b, mid } => {
                let (m, e) = (*mid - *a, *b - *a);
                m * (1.0 / m.len2()) - e * (1.0 / e.len2())
            }
            EdgeSpec::Chain { ctrl, .. } => {
                let first = *ctrl.first()?;
                ctrl.iter().skip(1).map(|p| *p - first).find(|d| d.len() > 1e-12)?
            }
        };
        d.normalized()
    }

    /// The same edge, run the other way.
    pub fn reversed(&self) -> EdgeSpec {
        match self {
            EdgeSpec::Line { a, b } => EdgeSpec::Line { a: *b, b: *a },
            EdgeSpec::Arc { a, b, mid } => EdgeSpec::Arc { a: *b, b: *a, mid: *mid },
            EdgeSpec::Chain { degree, ctrl } => EdgeSpec::Chain { degree: *degree, ctrl: ctrl.iter().rev().copied().collect() },
        }
    }

    /// A point telling apart edges with the same ends, whichever way they run.
    fn mid_key(&self) -> Option<Key> {
        match self {
            EdgeSpec::Line { .. } => None,
            EdgeSpec::Arc { mid, .. } => Some(key(*mid)),
            EdgeSpec::Chain { ctrl, .. } => {
                // Summed in pairs from both ends, so that the chain run either way gives the very
                // same point: a font's control points are simple fractions, whose mean can fall
                // exactly halfway between two steps of the key, where summing in another order
                // would round it to the other one.
                let n = ctrl.len();
                let sum = (0..n.div_ceil(2)).fold(Vec3::ZERO, |s, i| match (ctrl.get(i), ctrl.get(n - 1 - i)) {
                    (Some(p), Some(q)) if i != n - 1 - i => s + (*p + *q),
                    (Some(p), _) => s + *p,
                    _ => s,
                });
                Some(key(sum * (1.0 / n.max(1) as f64)))
            }
        }
    }

    fn valid(&self) -> bool {
        match self {
            EdgeSpec::Line { a, b } => a.is_finite() && b.is_finite(),
            EdgeSpec::Arc { a, b, mid } => a.is_finite() && b.is_finite() && mid.is_finite(),
            EdgeSpec::Chain { degree, ctrl } => {
                (1..=MAX_CHAIN_DEGREE).contains(degree)
                    && ctrl.len() > *degree
                    && ctrl.len() <= MAX_CHAIN_POINTS
                    && (ctrl.len() - 1) % degree == 0
                    && ctrl.iter().all(|p| p.is_finite())
            }
        }
    }
}

/// A face's surface.
#[derive(Clone, Debug, PartialEq)]
pub enum SurfSpec {
    /// The plane of the boundary.
    Plane,
    /// The surface the line `a`→`b` sweeps turning `angle` about the axis.
    Revolved { a: Vec3, b: Vec3, axis_origin: Vec3, axis: Vec3, angle: f64 },
    /// The surface an arc sweeps moving along `dir`.
    Extruded { a: Vec3, b: Vec3, mid: Vec3, dir: Vec3 },
    /// The straight lines joining equal parameters of two Bézier chains of one degree with the
    /// same number of pieces, `a` and `b` running the same way; both chains lie on it as
    /// boundary edges.
    Ruled { a: EdgeSpec, b: EdgeSpec },
}

/// A face: boundary loops, surface, and a point on it with its outward normal (curved faces).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceSpec {
    pub loops: Vec<Vec<EdgeSpec>>,
    pub surface: SurfSpec,
    pub probe: Option<(Vec3, Vec3)>,
}

type Key = (i64, i64, i64);

fn key(p: Vec3) -> Key {
    let q = |x: f64| (x * 1e6).round() as i64;
    (q(p.x), q(p.y), q(p.z))
}

/// How near (mm) two edge ends weld into one vertex wherever they fall on the grid of [`key`].
/// The grid alone joins the ends in one of its cells, but not two ends a hair apart on either
/// side of a cell's border: a corner at x = 84.0764335 lies on a border, and the same corner
/// worked out again through a bend can land 1e-14 mm to the other side, leaving its face's loop
/// open. The planar booleans close loops over gaps up to 1e-6 mm, so the weld reaches as far.
const WELD: f64 = 1e-6;

/// The faces with every edge end within [`WELD`] of an end met earlier in a neighbouring cell of
/// the grid moved onto that end, so that [`key`] finds one vertex for both; `None` when no end
/// moves. Ends in one cell stay as they are (the grid already joins them).
fn welded(faces: &[FaceSpec]) -> Option<Vec<FaceSpec>> {
    // The end each cell's ends weld to: the first met in it, or a nearer one in a neighbour.
    let mut at: HashMap<Key, Vec3> = HashMap::new();
    let mut moves = false;
    for f in faces {
        // A ruled surface's chains end on the vertices of its boundary.
        let ruled: Vec<&EdgeSpec> = match &f.surface {
            SurfSpec::Ruled { a, b } => vec![a, b],
            _ => Vec::new(),
        };
        for e in f.loops.iter().flatten().chain(ruled) {
            let (a, b) = e.ends();
            for p in [a, b] {
                let k = key(p);
                if !p.is_finite() || at.contains_key(&k) {
                    continue;
                }
                let mut near = None;
                for (dx, dy, dz) in (-1i64..=1).flat_map(|x| (-1i64..=1).flat_map(move |y| (-1i64..=1).map(move |z| (x, y, z)))) {
                    let n = (k.0.saturating_add(dx), k.1.saturating_add(dy), k.2.saturating_add(dz));
                    if n != k
                        && let Some(q) = at.get(&n).filter(|q| q.dist(p) < WELD)
                    {
                        near = Some(*q);
                        break;
                    }
                }
                moves |= near.is_some();
                at.insert(k, near.unwrap_or(p));
            }
        }
    }
    if !moves {
        return None;
    }
    let weld = |p: Vec3| if p.is_finite() { at.get(&key(p)).copied().filter(|q| key(*q) != key(p)).unwrap_or(p) } else { p };
    let edge = |e: &EdgeSpec| match e {
        EdgeSpec::Line { a, b } => EdgeSpec::Line { a: weld(*a), b: weld(*b) },
        EdgeSpec::Arc { a, b, mid } => EdgeSpec::Arc { a: weld(*a), b: weld(*b), mid: *mid },
        EdgeSpec::Chain { degree, ctrl } => {
            let mut ctrl = ctrl.to_vec();
            let n = ctrl.len();
            for i in [0, n.saturating_sub(1)] {
                if let Some(p) = ctrl.get_mut(i) {
                    *p = weld(*p);
                }
            }
            EdgeSpec::Chain { degree: *degree, ctrl: ctrl.into() }
        }
    };
    Some(
        faces
            .iter()
            .map(|f| FaceSpec {
                loops: f.loops.iter().map(|l| l.iter().map(edge).collect()).collect(),
                surface: match &f.surface {
                    SurfSpec::Ruled { a, b } => SurfSpec::Ruled { a: edge(a), b: edge(b) },
                    s => s.clone(),
                },
                probe: f.probe,
            })
            .collect(),
    )
}

fn arr(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The vertices and edges of the faces being sewn, shared by key.
#[derive(Default)]
struct Builder {
    vertices: Vec<[f64; 3]>,
    /// Vertices by point and fan (see [`corner_fans`]).
    verts: HashMap<(Key, usize), u32>,
    edges: Vec<PartEdge>,
    by_key: HashMap<(Key, usize, Key, usize, Option<Key>, usize), u32>,
}

impl Builder {
    fn vertex(&mut self, p: Vec3, fan: usize) -> Result<u32> {
        if let Some(v) = self.verts.get(&(key(p), fan)) {
            return Ok(*v);
        }
        let v = u32::try_from(self.vertices.len()).map_err(|_| KernelError::Invalid("sew: vertex budget exceeded".into()))?;
        self.vertices.push(arr(p));
        self.verts.insert((key(p), fan), v);
        Ok(v)
    }

    fn point(&self, v: u32) -> Vec3 {
        self.vertices.get(v as usize).copied().map_or(Vec3::ZERO, v3)
    }

    /// Where the vertex at `p` is (the first point met in its cell), `p` itself before any.
    fn at(&self, p: Vec3) -> Vec3 {
        self.verts.get(&(key(p), 0)).map_or(p, |v| self.point(*v))
    }

    /// The edge `e` on boundary sheet `sheet`, between the vertices of fans `fans` at its ends,
    /// and whether `e` runs along it.
    fn edge(&mut self, e: &EdgeSpec, sheet: usize, (fa, fb): (usize, usize)) -> Result<(u32, bool)> {
        let (a, b) = e.ends();
        let mid = e.mid_key();
        let (ka, kb) = (key(a), key(b));
        if let Some(found) = self.by_key.get(&(ka, fa, kb, fb, mid, sheet)) {
            return Ok((*found, true));
        }
        if let Some(found) = self.by_key.get(&(kb, fb, ka, fa, mid, sheet)) {
            return Ok((*found, false));
        }
        let (va, vb) = (self.vertex(a, fa)?, self.vertex(b, fb)?);
        let (pa, pb) = (self.point(va), self.point(vb));
        let (curve, interval) = match e {
            EdgeSpec::Line { .. } => (b::Curve::Line { start: arr(pa), end: arr(pb) }, [0.0, 1.0]),
            EdgeSpec::Arc { mid, .. } => arc_curve(pa, pb, *mid)?,
            EdgeSpec::Chain { .. } => {
                let (knots, points) = chain(e, pa, pb)?;
                let n = knots.last().copied().unwrap_or(1.0);
                (b::Curve::Spline { knots, points }, [0.0, n])
            }
        };
        let id = u32::try_from(self.edges.len()).map_err(|_| KernelError::Invalid("sew: edge budget exceeded".into()))?;
        self.edges.push(PartEdge { curve, interval, vertices: [va, vb] });
        self.by_key.insert((ka, fa, kb, fb, mid, sheet), id);
        Ok((id, true))
    }

    /// The surface of face `fi` and whether its natural normal points outward; on a ruled face,
    /// each use's exact pcurve is filled in.
    fn surface(&self, fi: usize, f: &FaceSpec, loops: &mut [Vec<PartUse>], tol: f64) -> Result<(b::Surface, bool)> {
        let fail = |m: &str| KernelError::Invalid(format!("sew: face {fi}: {m}"));
        match &f.surface {
            SurfSpec::Plane => plane_of(f).map(|s| (s, true)).ok_or_else(|| fail("cannot attach a plane to a wire that is not on one plane")),
            SurfSpec::Revolved { a, b, axis_origin, axis, angle: _ } => {
                let s = revolved(*a, *b, *axis_origin, *axis).map_err(|m| fail(&m))?;
                Ok((s.clone(), outward(&s, f.probe)))
            }
            SurfSpec::Extruded { a, b, mid, dir } => {
                let c = circumcentre(*a, *mid, *b).ok_or_else(|| fail("an arc through points in line"))?;
                let n = (*mid - *a).cross(*b - *mid).normalized().ok_or_else(|| fail("an arc through points in line"))?;
                let z = dir.normalized().ok_or_else(|| fail("an extrusion of no length"))?;
                if n.cross(z).len() > 1e-9 {
                    return Err(fail("not supported yet: an arc wall extruded askew to its arc's axis"));
                }
                let x = (*a - c).normalized().ok_or_else(|| fail("an arc of no radius"))?;
                let y = z.cross(x).normalized().ok_or_else(|| fail("an arc of no radius"))?;
                let frame = b::Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(y) };
                let s = b::Surface::Cylinder { frame, radius: a.dist(c) };
                Ok((s.clone(), outward(&s, f.probe)))
            }
            SurfSpec::Ruled { a, b } => {
                let (ka, pa) = chain(a, self.at(a.ends().0), self.at(a.ends().1))?;
                let (kb, pb) = chain(b, self.at(b.ends().0), self.at(b.ends().1))?;
                let (EdgeSpec::Chain { degree: da, .. }, EdgeSpec::Chain { degree: db, .. }) = (a, b) else {
                    return Err(KernelError::Invalid("sew: not a Bézier chain".into()));
                };
                if da != db || pa.len() != pb.len() || ka != kb {
                    return Err(KernelError::Invalid("sew: ruled chains differ in pieces".into()));
                }
                let points = pa.iter().zip(&pb).map(|(p, q)| vec![*p, *q]).collect();
                let s = b::Surface::Spline { u_knots: ka.clone(), v_knots: vec![0.0, 0.0, 1.0, 1.0], points };
                for u in loops.iter_mut().flatten() {
                    let e = self.edges.get(u.edge as usize).ok_or_else(|| fail("edge"))?;
                    u.given = Some(ruled_pcurve(e, *da, &ka, [&pa, &pb], tol).ok_or_else(|| fail("not supported yet: an edge across a ruled face"))?);
                }
                Ok((s.clone(), outward(&s, f.probe)))
            }
        }
    }
}

/// A Bézier chain as a B-spline of its degree (inner knots as many times as the degree) ending on
/// `a` and `b` (its vertices).
fn chain(e: &EdgeSpec, a: Vec3, b: Vec3) -> Result<(Vec<f64>, Vec<[f64; 3]>)> {
    let EdgeSpec::Chain { degree, ctrl } = e else { return Err(KernelError::Invalid("sew: not a Bézier chain".into())) };
    if !e.valid() {
        return Err(KernelError::Invalid("sew: a Bézier chain needs degree·n + 1 finite control points".into()));
    }
    let d = *degree;
    let n = (ctrl.len() - 1) / d;
    let mut knots = vec![0.0; d + 1];
    for i in 1..n {
        knots.extend(std::iter::repeat_n(i as f64, d));
    }
    knots.extend(std::iter::repeat_n(n as f64, d + 1));
    let mut pts: Vec<[f64; 3]> = ctrl.iter().map(|p| arr(*p)).collect();
    if let Some(first) = pts.first_mut() {
        *first = arr(a);
    }
    if let Some(last) = pts.last_mut() {
        *last = arr(b);
    }
    Ok((knots, pts))
}

/// The exact pcurve of an edge of a ruled face `(1 − v)·A(u) + v·B(u)` (`rows` its chains A and
/// B, of `degree` on `knots`): a chain lying on A or B either way round is `(u(t), 0 or 1)` with
/// `u` affine in its parameter, and a line from a corner of A to the corner of B at the same `u`
/// is a ruling, `(u, t)`. `None` for any other edge.
fn ruled_pcurve(e: &PartEdge, degree: usize, knots: &[f64], rows: [&[[f64; 3]]; 2], tol: f64) -> Option<Curve2> {
    let near = |p: &[f64; 3], q: &[f64; 3]| v3(*p).dist(v3(*q)) <= tol;
    let n = *knots.last()?;
    match &e.curve {
        b::Curve::Spline { knots: ek, points } if ek.as_slice() == knots => {
            for (v, row) in [0.0, 1.0].into_iter().zip(rows) {
                if row.len() != points.len() {
                    continue;
                }
                let forward = points.iter().zip(row).all(|(p, q)| near(p, q));
                let backward = points.iter().zip(row.iter().rev()).all(|(p, q)| near(p, q));
                if forward || backward {
                    // Control points at the Greville abscissae give the affine u(t) exactly.
                    let points = (0..points.len())
                        .map(|i| {
                            let xi = (1..=degree).map(|r| knots.get(i + r).copied().unwrap_or(0.0)).sum::<f64>() / degree as f64;
                            [if forward { xi } else { n - xi }, v]
                        })
                        .collect();
                    return Some(Curve2::Spline { knots: knots.to_vec(), points });
                }
            }
            None
        }
        b::Curve::Line { start, end } => {
            let corner = |p: &[f64; 3]| -> Option<[f64; 2]> {
                for (v, row) in [0.0, 1.0].into_iter().zip(rows) {
                    if row.first().is_some_and(|q| near(p, q)) {
                        return Some([0.0, v]);
                    }
                    if row.last().is_some_and(|q| near(p, q)) {
                        return Some([n, v]);
                    }
                }
                None
            };
            let (s, z) = (corner(start)?, corner(end)?);
            (s[0] == z[0] && s[1] != z[1]).then(|| Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![s, z] })
        }
        _ => None,
    }
}

/// The plane of a face's boundary: through the mean of its control points (arcs' midpoints and
/// chains' control points included), its normal the one the outer loop runs counter-clockwise
/// about, summed about that mean (about a far origin the rounding of the sum grows with the
/// square of the distance, and turns a thin face's short normal). None when the boundary has no
/// area or leaves the plane by more than 10⁻⁶ mm.
fn plane_of(f: &FaceSpec) -> Option<b::Surface> {
    let points = |e: &EdgeSpec| -> Vec<Vec3> {
        match e {
            EdgeSpec::Line { a, b } => vec![*a, *b],
            EdgeSpec::Arc { a, b, mid } => vec![*a, *mid, *b],
            EdgeSpec::Chain { ctrl, .. } => ctrl.to_vec(),
        }
    };
    let all: Vec<Vec3> = f.loops.iter().flatten().flat_map(points).collect();
    let mean = all.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / all.len().max(1) as f64);
    let n = f.loops.first()?.iter().fold(Vec3::ZERO, |n, e| {
        let p = points(e);
        p.iter().zip(p.iter().skip(1)).fold(n, |n, (a, b)| n + (*a - mean).cross(*b - mean))
    });
    let n = n.normalized()?;
    if !mean.is_finite() || all.iter().any(|p| ((*p - mean).dot(n)).abs() > 1e-6) {
        return None;
    }
    let t = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = (t - n * t.dot(n)).normalized()?;
    let v = n.cross(u).normalized()?;
    Some(b::Surface::Plane { origin: arr(mean), u_axis: arr(u), v_axis: arr(v) })
}

/// The surface the line `a`→`b` sweeps turning about the axis: a cylinder (the line along the
/// axis), a plane (square to it) or a cone (in a plane through it). A line skew to the axis
/// sweeps a hyperboloid, which has no exact form here.
fn revolved(a: Vec3, b: Vec3, origin: Vec3, axis: Vec3) -> std::result::Result<b::Surface, String> {
    let k = axis.normalized().ok_or("the axis has no direction")?;
    let foot = |p: Vec3| origin + k * (p - origin).dot(k);
    let (ra, rb) = (a - foot(a), b - foot(b));
    let along = (b - a).dot(k);
    let scale = ra.len().max(rb.len());
    if !(scale > 1e-12) {
        return Err("a line along the axis sweeps no surface".into());
    }
    if ra.cross(rb).len() > 1e-9 * scale * scale || ra.dot(rb) < 0.0 {
        return Err("not supported yet: a face revolved from a line skew to its axis or across it".into());
    }
    let x = if ra.len() >= rb.len() { ra } else { rb }.normalized().ok_or("a line on the axis")?;
    let frame = |z: Vec3| -> std::result::Result<b::Frame, String> {
        let y = z.cross(x).normalized().ok_or("frame")?;
        Ok(b::Frame { origin: arr(foot(a)), x_axis: arr(x), y_axis: arr(y) })
    };
    if along.abs() <= 1e-12 * (b - a).len() {
        let y = k.cross(x).normalized().ok_or("frame")?;
        return Ok(b::Surface::Plane { origin: arr(foot(a)), u_axis: arr(x), v_axis: arr(y) });
    }
    let slope = (rb.len() - ra.len()) / along;
    if slope.abs() <= 1e-12 {
        return Ok(b::Surface::Cylinder { frame: frame(k)?, radius: ra.len() });
    }
    // The cone widens along its z.
    let z = if slope > 0.0 { k } else { k * -1.0 };
    Ok(b::Surface::Cone { frame: frame(z)?, radius_at_origin: ra.len(), half_angle: slope.abs().atan() })
}

/// Whether the surface's natural normal at the probe point faces the probe's outward
/// direction (true without a probe, or where the point cannot be found on the surface).
fn outward(surface: &b::Surface, probe: Option<(Vec3, Vec3)>) -> bool {
    let Some((p, out)) = probe else { return true };
    let Ok(s) = SurfaceEval::new(surface) else { return true };
    let Some((_, su, sv)) = s.project(p, None, f64::INFINITY).and_then(|uv| s.derivs(uv[0], uv[1])) else { return true };
    su.cross(sv).dot(out) >= 0.0
}

/// Below this sagitta-to-half-chord ratio an arc is the exact rational quadratic from its ends
/// and midpoint, rather than a circle about its circumcentre: the centre of an arc 12 mm long
/// with a 0.08 µm sagitta (a 225 m radius) is found to about 1e-6 mm, and the circle about it
/// would miss the arc's own ends by as much.
const FLAT_ARC: f64 = 1e-2;

/// The arc from `a` to `b` through `mid` and its interval: a circle (angle from `a`), or for a
/// flat arc one rational quadratic: the middle control point on the line from the chord's
/// midpoint through `mid`, 2 / (1 − t²) as far, weight (1 − t²) / (1 + t²), where t is the
/// sagitta over the half chord.
fn arc_curve(a: Vec3, b: Vec3, mid: Vec3) -> Result<(b::Curve, [f64; 2])> {
    let fail = || KernelError::Invalid("sew: an arc through points in line".into());
    let m = (a + b) * 0.5;
    let half = a.dist(b) / 2.0;
    let chord = (b - a).normalized().ok_or_else(fail)?;
    let bow = mid - m;
    let t = bow.len() / half;
    if half > 1e-9 && t < FLAT_ARC && bow.dot(chord).abs() <= 1e-9 * half.max(1.0) {
        let w = (1.0 - t * t) / (1.0 + t * t);
        let apex = m + bow * (2.0 / (1.0 - t * t));
        let homogeneous = vec![[a.x, a.y, a.z, 1.0], [apex.x * w, apex.y * w, apex.z * w, w], [b.x, b.y, b.z, 1.0]];
        return Ok((b::Curve::RationalSpline { knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], homogeneous }, [0.0, 1.0]));
    }
    let c = circumcentre(a, mid, b).ok_or_else(fail)?;
    let n = (mid - a).cross(b - mid).normalized().ok_or_else(fail)?;
    let x = (a - c).normalized().ok_or_else(fail)?;
    let y = n.cross(x).normalized().ok_or_else(fail)?;
    let angle = |p: Vec3| {
        let r = p - c;
        r.dot(y).atan2(r.dot(x)).rem_euclid(std::f64::consts::TAU)
    };
    let end = angle(b);
    if !(end > 0.0) {
        return Err(fail());
    }
    Ok((b::Curve::Circle { frame: b::Frame { origin: arr(c), x_axis: arr(x), y_axis: arr(y) }, radius: a.dist(c) }, [0.0, end]))
}

/// Sew the faces into a closed solid.
pub fn sew(faces: &[FaceSpec]) -> Result<Body> {
    sew_impl(faces, false, None).map(|(body, _)| body)
}

/// Sew the faces into a closed solid that may touch itself: where four faces meet along a
/// straight edge (a cut-out's corner on the outline of a plate), each two of them get an edge of
/// their own (see [`touching_sheets`]), and where faces meet at a point in fans joined by no edge
/// (a hole's corner on its face's outline), each fan gets a vertex of its own.
pub fn sew_touching(faces: &[FaceSpec]) -> Result<Body> {
    sew_impl(faces, true, None).map(|(body, _)| body)
}

/// Separate boundary sheets at a shared geometric edge without moving it.
/// Optional transient sources follow only unchanged produced face identities.
/// Also reports the face each face of the body was built from (by its place in `faces`; `None`
/// when that could not be followed).
pub(crate) fn sew_touching_with_sources(faces: &[FaceSpec], sources: &[Option<crate::FaceSource>]) -> Result<(Body, Option<Vec<usize>>)> {
    if sources.len() != faces.len() {
        return Err(KernelError::Invalid("sew source alignment".into()));
    }
    sew_impl(faces, true, Some(sources))
}

type EdgeUse = (usize, usize, usize);

/// Below this angle (radians) two faces leave an edge the same way: tangent faces, ordered by
/// their curvature.
const SAME_WAY: f64 = 1e-6;

/// Where more than two faces meet along a straight edge (bodies touching along a line, or a wall
/// of no thickness), which faces bound each sheet of it (each sheet gets its own edge), and the
/// vertex fans that follow from that (see [`corner_fans`]). Only edges on planes, on the walls
/// of extruded arcs and on ruled walls are looked at.
///
/// Each sheet bounds a wedge of material, so that pieces of material meeting along an edge stay
/// apart, except where that would make a face touch itself: a hole touching the outline from
/// inside (tangent, or at a corner) leaves its caps an outline and a hole meeting at the edge's
/// end, which with the material kept apart would share a vertex, and a face can't have that.
/// The material there goes round the hole, and the edges ending at such points get sheets that
/// bound the empty wedges instead: the hole's wall on either side is one sheet, the outline's
/// the other.
fn touching_sheets(faces: &[FaceSpec]) -> Result<(HashMap<EdgeUse, usize>, HashMap<Corner, usize>)> {
    let mut uses: HashMap<(Key, Key), Vec<(EdgeUse, Vec3, Vec3)>> = HashMap::new();
    for (fi, face) in faces.iter().enumerate() {
        if !matches!(face.surface, SurfSpec::Plane | SurfSpec::Extruded { .. } | SurfSpec::Ruled { .. }) {
            continue;
        }
        for (li, lp) in face.loops.iter().enumerate() {
            for (ei, e) in lp.iter().enumerate() {
                if let EdgeSpec::Line { a, b } = e {
                    let (ka, kb) = (key(*a), key(*b));
                    let k = if ka < kb { (ka, kb) } else { (kb, ka) };
                    uses.entry(k).or_default().push(((fi, li, ei), *a, *b));
                }
            }
        }
    }
    // Each contact edge: its ends, both ways of pairing its faces, and whether the second is used.
    let mut contacts = Vec::new();
    for (ends, group) in uses.iter().filter(|(_, g)| g.len() > 2) {
        let [material, empty] = edge_sheets(faces, group)?;
        contacts.push((*ends, material, empty, false));
    }
    if contacts.is_empty() {
        return Ok((HashMap::new(), corner_fans(faces, &HashMap::new())));
    }
    let sheets_of = |contacts: &[((Key, Key), [[EdgeUse; 2]; 2], [[EdgeUse; 2]; 2], bool)]| -> HashMap<EdgeUse, usize> {
        let mut sheets = HashMap::new();
        for (_, material, empty, round) in contacts {
            for (sheet, pair) in (if *round { empty } else { material }).iter().enumerate() {
                for u in pair {
                    sheets.insert(*u, sheet);
                }
            }
        }
        sheets
    };
    // Each pass turns at least one edge round, and none back.
    loop {
        let sheets = sheets_of(&contacts);
        let fans = corner_fans(faces, &sheets);
        let touching = self_touching(faces, &fans);
        let mut turned = false;
        for (ends, _, _, round) in &mut contacts {
            if !*round && (touching.contains(&ends.0) || touching.contains(&ends.1)) {
                *round = true;
                turned = true;
            }
        }
        if !turned {
            return Ok((sheets, fans));
        }
    }
}

/// The points where a face has two corners at one vertex (its loops would touch there).
fn self_touching(faces: &[FaceSpec], fans: &HashMap<Corner, usize>) -> std::collections::HashSet<Key> {
    let mut seen = std::collections::HashSet::new();
    let mut out = std::collections::HashSet::new();
    for (fi, f) in faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            for (ei, e) in l.iter().enumerate() {
                let at = key(e.ends().0);
                if !seen.insert((at, fans.get(&(fi, li, ei)).copied().unwrap_or(0), fi)) {
                    out.insert(at);
                }
            }
        }
    }
    out
}

/// The two ways of making sheets of an edge four faces meet along, as the pairs of faces
/// bounding each: around the wedges of material between them, and around the empty ones.
///
/// Seen along the edge, each face leaves it as a curve; going round the edge, material and empty
/// wedges between them alternate. Faces are ordered by the way they leave the edge and, where
/// they leave it the same way (tangent faces), by how fast they turn from it: the order of
/// points a short way into each face, as in 2D loop chaining (`prism::tightest_left`).
fn edge_sheets(faces: &[FaceSpec], group: &[(EdgeUse, Vec3, Vec3)]) -> Result<[[[EdgeUse; 2]; 2]; 2]> {
    let fail = |s: &str| KernelError::Invalid(format!("sew: {s}"));
    let (Some((_, a0, b0)), 4) = (group.first(), group.len()) else { return Err(fail("unsupported edge contact")) };
    let t = (if key(*a0) < key(*b0) { *b0 - *a0 } else { *a0 - *b0 }).normalized().ok_or_else(|| fail("a degenerate contact"))?;
    // Each face: its use, where the use starts, the way into it, its turn, its material side, and
    // the angle of the way into it from the first face's.
    let mut rays = Vec::with_capacity(4);
    let mut reference = None;
    for (u, a, b) in group {
        let face = faces.get(u.0).ok_or_else(|| fail("edge contact"))?;
        let (into, turn, ccw) = leaving(face, *u, *a, *b, t).ok_or_else(|| fail("unsupported edge contact"))?;
        let r: Vec3 = *reference.get_or_insert(into);
        let angle = r.cross(into).dot(t).atan2(r.dot(into)).rem_euclid(std::f64::consts::TAU);
        rays.push((*u, key(*a), turn, ccw, if angle > std::f64::consts::TAU - SAME_WAY { 0.0 } else { angle }));
    }
    rays.sort_by(|p, q| p.4.total_cmp(&q.4));
    // Faces leaving the same way, by their turn (a total order on each, which a comparison with
    // a tolerance is not).
    for i in 1..rays.len() {
        let mut j = i;
        while let (Some(p), Some(q)) = (rays.get(j.wrapping_sub(1)), rays.get(j)) {
            if q.4 - p.4 > SAME_WAY || p.2 <= q.2 {
                break;
            }
            rays.swap(j - 1, j);
            j -= 1;
        }
    }
    let rays = Ring::new(rays);
    let wedges: Vec<_> = rays.edges().collect();
    // Each wedge's faces must use the edge in opposite directions, and be told apart (faces
    // leaving the same way and turning alike lie on each other).
    let same_way = |p: f64, q: f64| {
        let gap = (q - p).rem_euclid(std::f64::consts::TAU);
        gap <= SAME_WAY || gap >= std::f64::consts::TAU - SAME_WAY
    };
    let alike = |p: f64, q: f64| (p - q).abs() <= 1e-9 * p.abs().max(q.abs()).max(1e-9);
    if wedges.iter().any(|(p, q)| p.3 == q.3 || p.1 == q.1 || (same_way(p.4, q.4) && alike(p.2, q.2))) {
        return Err(fail("ambiguous edge contact"));
    }
    let pairs = |material: bool| -> Result<[[EdgeUse; 2]; 2]> {
        let mut it = wedges.iter().filter(|(p, _)| p.3 == material).map(|(p, q)| [p.0, q.0]);
        match (it.next(), it.next()) {
            (Some(x), Some(y)) => Ok([x, y]),
            _ => Err(fail("ambiguous edge contact")),
        }
    };
    Ok([pairs(true)?, pairs(false)?])
}

/// How a face leaves the straight edge `a`→`b` it bounds (its use `u`), seen along the edge's
/// direction `t`: the way into the face square to the edge, how fast it turns from that way (its
/// curvature, positive counter-clockwise about `t`; 0 for a plane or a ruled wall), and whether
/// its material is on its counter-clockwise side. None for other faces, and for an extruded wall
/// the edge is not a ruling of.
fn leaving(face: &FaceSpec, u: EdgeUse, a: Vec3, b: Vec3, t: Vec3) -> Option<(Vec3, f64, bool)> {
    let along = (b - a).normalized()?;
    // The outward normal along the edge, and the centre of curvature (with its curvature).
    let (outward, centre) = match &face.surface {
        SurfSpec::Plane => (plane_normal(face)?, None),
        SurfSpec::Extruded { a: sa, b: sb, mid, dir } => {
            let k = dir.normalized()?;
            if along.dot(k).abs() < 1.0 - 1e-9 {
                return None;
            }
            let centre = circumcentre(*sa, *mid, *sb)?;
            let radial = |p: Vec3| {
                let r = p - centre;
                r - k * r.dot(k)
            };
            let r = radial(a);
            let (p, out) = face.probe?;
            let side = if radial(p).dot(out) < 0.0 { -1.0 } else { 1.0 };
            (r.normalized()? * side, Some((centre, 1.0 / r.len())))
        }
        // A ruled wall (a cut-out's wall across a bend, ruled between its traces on the bend's
        // two faces), standing on the edge: its normal there is square to the edge and to the way
        // the next edge leaves the edge's end into the wall, as a wall's top or bottom edge leaves
        // its upright one.
        SurfSpec::Ruled { .. } => {
            let lp = face.loops.get(u.1)?;
            let v = lp.get((u.2 + 1) % lp.len())?.start_dir()?;
            (along.cross(v - along * v.dot(along)).normalized()?, None)
        }
        _ => return None,
    };
    // A face's loops run counter-clockwise about its outward normal: it lies on their left.
    let into = outward.cross(along).normalized()?;
    let left = t.cross(into);
    let turn = match centre {
        None => 0.0,
        Some((c, curvature)) if (c - a).dot(left) > 0.0 => curvature,
        Some((_, curvature)) => -curvature,
    };
    turn.is_finite().then_some((into, turn, left.dot(outward) < 0.0))
}

/// The unit normal a planar face's outer loop runs counter-clockwise about (through its arcs'
/// midpoints and chains' control points, so that a loop of one line and one arc has one).
fn plane_normal(face: &FaceSpec) -> Option<Vec3> {
    let n = face.loops.first()?.iter().fold(Vec3::ZERO, |n, e| match e {
        EdgeSpec::Line { a, b } => n + a.cross(*b),
        EdgeSpec::Arc { a, b, mid } => n + a.cross(*mid) + mid.cross(*b),
        EdgeSpec::Chain { ctrl, .. } => ctrl.iter().zip(ctrl.iter().skip(1)).fold(n, |n, (p, q)| n + p.cross(*q)),
    });
    n.normalized()
}

/// The centre of the circle through three points; None when they are in line.
fn circumcentre(a: Vec3, b: Vec3, c: Vec3) -> Option<Vec3> {
    let (u, v) = (b - a, c - a);
    let w = u.cross(v);
    let d = 2.0 * w.dot(w);
    if !(d > 1e-300) {
        return None;
    }
    let p = a + (v.cross(w) * u.dot(u) + w.cross(u) * v.dot(v)) * (1.0 / d);
    p.is_finite().then_some(p)
}

/// A loop corner: face, loop, and the edge of the loop that starts there.
type Corner = (usize, usize, usize);

/// Where the faces meeting at a point form several fans, joined to each other by no edge (a hole
/// touching its face's outline at one point; two caps touching at a corner), the fan each corner
/// there is in, from 1 for every fan past the first (corners not listed are in fan 0). Each fan
/// gets its own vertex, so that a face's loops share no vertex and
/// every vertex of the solid is a manifold one.
fn corner_fans(faces: &[FaceSpec], sheets: &HashMap<EdgeUse, usize>) -> HashMap<Corner, usize> {
    type EdgeId = (Key, Key, Option<Key>, usize);
    let id = |fi: usize, li: usize, ei: usize, e: &EdgeSpec| -> EdgeId {
        let (a, b) = e.ends();
        let (ka, kb) = (key(a), key(b));
        let sheet = sheets.get(&(fi, li, ei)).copied().unwrap_or(0);
        if ka <= kb { (ka, kb, e.mid_key(), sheet) } else { (kb, ka, e.mid_key(), sheet) }
    };
    // Every corner at each point, with the two edges it lies between.
    let mut at: HashMap<Key, Vec<(Corner, [EdgeId; 2])>> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for (li, l) in f.loops.iter().enumerate() {
            let n = l.len();
            for (ei, e) in l.iter().enumerate() {
                let Some(prev) = l.get((ei + n - 1) % n.max(1)) else { continue };
                let p = id(fi, li, (ei + n - 1) % n.max(1), prev);
                at.entry(key(e.ends().0)).or_default().push(((fi, li, ei), [p, id(fi, li, ei, e)]));
            }
        }
    }
    let mut out = HashMap::new();
    for corners in at.values().filter(|c| c.len() > 2) {
        let mut parent: Vec<usize> = (0..corners.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while let Some(&p) = parent.get(i) {
                if p == i {
                    break;
                }
                let up = parent.get(p).copied().unwrap_or(p);
                if let Some(slot) = parent.get_mut(i) {
                    *slot = up;
                }
                i = p;
            }
            i
        }
        let mut by_edge: HashMap<&EdgeId, usize> = HashMap::new();
        for (k, (_, edges)) in corners.iter().enumerate() {
            for e in edges {
                match by_edge.get(e) {
                    Some(&other) => {
                        let (a, b) = (root(&mut parent, k), root(&mut parent, other));
                        if let Some(slot) = parent.get_mut(a) {
                            *slot = b;
                        }
                    }
                    None => {
                        by_edge.insert(e, k);
                    }
                }
            }
        }
        let mut fan_of_root: HashMap<usize, usize> = HashMap::new();
        for (k, (corner, _)) in corners.iter().enumerate() {
            let r = root(&mut parent, k);
            let next = fan_of_root.len();
            let fan = *fan_of_root.entry(r).or_insert(next);
            if fan > 0 {
                out.insert(*corner, fan);
            }
        }
    }
    out
}

/// The parent of `i` in a union-find forest, with path halving.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let up = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = up;
        }
        i = p;
    }
    i
}

fn join(parent: &mut [usize], a: usize, b: usize) {
    let (a, b) = (root(parent, a), root(parent, b));
    if let Some(slot) = parent.get_mut(a) {
        *slot = b;
    }
}

/// The faces using each edge, a face once per use.
fn edge_users(edges: usize, faces: &[PartFace]) -> Result<Vec<Vec<usize>>> {
    let mut users: Vec<Vec<usize>> = vec![Vec::new(); edges];
    for (fi, f) in faces.iter().enumerate() {
        for u in f.loops.iter().flatten() {
            users.get_mut(u.edge as usize).ok_or_else(|| KernelError::Failed("sew: edge reference".into()))?.push(fi);
        }
    }
    Ok(users)
}

/// The connected groups of faces (joined through shared edges), each ascending, and each face's
/// group.
fn components(faces: usize, users: &[Vec<usize>]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut parent: Vec<usize> = (0..faces).collect();
    for u in users {
        for w in u.windows(2) {
            if let [a, b] = w {
                join(&mut parent, *a, *b);
            }
        }
    }
    let mut group_of_root: HashMap<usize, usize> = HashMap::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut of = vec![0; faces];
    for f in 0..faces {
        let r = root(&mut parent, f);
        let next = groups.len();
        let g = *group_of_root.entry(r).or_insert(next);
        if g == next {
            groups.push(Vec::new());
        }
        if let (Some(group), Some(slot)) = (groups.get_mut(g), of.get_mut(f)) {
            group.push(f);
            *slot = g;
        }
    }
    (groups, of)
}

/// A contact's edge sheets can end in separate vertex fans: each fan gets its own vertex at the
/// same point, so that every vertex of the boundary is a manifold one. A fan is the faces round
/// the vertex joined through the edges that end there.
fn split_vertex_fans(vertices: &mut Vec<[f64; 3]>, edges: &mut [PartEdge], faces: &[PartFace]) -> Result<()> {
    let users = edge_users(edges.len(), faces)?;
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    for (e, edge) in edges.iter().enumerate() {
        for v in edge.vertices {
            if let Some(list) = at.get_mut(v as usize)
                && list.last() != Some(&e)
            {
                list.push(e);
            }
        }
    }
    for (v, incident) in at.iter().enumerate() {
        // Faces round the vertex, joined by each incident edge.
        let mut parent: HashMap<usize, usize> = HashMap::new();
        let mut local: Vec<usize> = Vec::new();
        let mut index = |f: usize, local: &mut Vec<usize>| -> usize {
            *parent.entry(f).or_insert_with(|| {
                local.push(local.len());
                local.len() - 1
            })
        };
        let mut edge_face = Vec::with_capacity(incident.len());
        for &e in incident {
            let fs: Vec<usize> = users.get(e).map(|u| u.iter().map(|f| index(*f, &mut local)).collect()).unwrap_or_default();
            for w in fs.windows(2) {
                if let [a, b] = w {
                    join(&mut local, *a, *b);
                }
            }
            edge_face.push((e, fs.first().copied()));
        }
        let mut fan_of_root: HashMap<usize, u32> = HashMap::new();
        for (e, f) in edge_face {
            let Some(f) = f else { continue };
            let r = root(&mut local, f);
            let fan = match fan_of_root.get(&r) {
                Some(fan) => *fan,
                None => {
                    // The first fan keeps the vertex; each other gets a copy.
                    let fan = if fan_of_root.is_empty() {
                        v as u32
                    } else {
                        let p = vertices.get(v).copied().ok_or_else(|| KernelError::Failed("sew: vertex".into()))?;
                        vertices.push(p);
                        u32::try_from(vertices.len() - 1).map_err(|_| KernelError::Invalid("sew: vertex budget exceeded".into()))?
                    };
                    fan_of_root.insert(r, fan);
                    fan
                }
            };
            if let Some(edge) = edges.get_mut(e) {
                for end in &mut edge.vertices {
                    if *end as usize == v {
                        *end = fan;
                    }
                }
            }
        }
    }
    Ok(())
}

fn sew_impl(faces: &[FaceSpec], touching: bool, sources: Option<&[Option<crate::FaceSource>]>) -> Result<(Body, Option<Vec<usize>>)> {
    if faces.len() < 4 || faces.len() > 200_000 {
        return Err(KernelError::Invalid("a solid needs 4…200000 faces".into()));
    }
    let mut edge_uses = 0usize;
    for f in faces {
        for l in &f.loops {
            edge_uses = edge_uses.saturating_add(l.len().max(1));
            if edge_uses > 500_000 {
                return Err(KernelError::Invalid("sew: edge budget exceeded".into()));
            }
        }
    }
    let welded = welded(faces);
    let faces = welded.as_deref().unwrap_or(faces);
    let (mut lo, mut hi) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for f in faces {
        for l in &f.loops {
            if l.is_empty() {
                return Err(KernelError::Invalid("sew: an empty loop".into()));
            }
            for e in l {
                let (a, b) = e.ends();
                // Ends that weld to one vertex make an edge of no length.
                if !e.valid() || !a.is_finite() || !b.is_finite() || a.dist(b) < 1e-9 || key(a) == key(b) {
                    return Err(KernelError::Invalid("sew: a degenerate edge".into()));
                }
                lo = lo.min(a).min(b);
                hi = hi.max(a).max(b);
            }
        }
    }
    // The record's working tolerance: 10⁻⁶ of the solid's size, at least 10⁻⁵ mm (the tolerance
    // of a fitted face, `fill::face_tolerance`).
    let tol = (lo.dist(hi) * 1e-6).max(1e-5);
    guard("sew", || {
        // Separate fans at a point only where contact is expected (as for edge sheets); a plain
        // sew refuses bodies touching at a vertex.
        let (sheets, fans) = if touching { touching_sheets(faces)? } else { (HashMap::new(), HashMap::new()) };
        let fan = |c: Corner| fans.get(&c).copied().unwrap_or(0);
        let mut built = Builder::default();
        let mut parts: Vec<Vec<Vec<PartUse>>> = Vec::with_capacity(faces.len());
        for (fi, f) in faces.iter().enumerate() {
            let mut loops = Vec::with_capacity(f.loops.len());
            for (li, l) in f.loops.iter().enumerate() {
                let n = l.len().max(1);
                let mut uses = Vec::with_capacity(l.len());
                for (ei, e) in l.iter().enumerate() {
                    let sheet = sheets.get(&(fi, li, ei)).copied().unwrap_or(0);
                    let (edge, forward) = built.edge(e, sheet, (fan((fi, li, ei)), fan((fi, li, (ei + 1) % n))))?;
                    uses.push(PartUse { edge, forward, given: None });
                }
                loops.push(uses);
            }
            parts.push(loops);
        }
        let mut out: Vec<PartFace> = Vec::with_capacity(faces.len());
        for (fi, (f, mut loops)) in faces.iter().zip(parts).enumerate() {
            let (surface, forward) = built.surface(fi, f, &mut loops, tol)?;
            out.push(PartFace { surface, forward, loops });
        }
        let Builder { mut vertices, mut edges, .. } = built;
        let users = edge_users(edges.len(), &out)?;
        if !sheets.is_empty() && components(out.len(), &users).0.len() > 1 {
            return Err(KernelError::Invalid("sew: bodies touch only along an edge".into()));
        }
        if !sheets.is_empty() {
            split_vertex_fans(&mut vertices, &mut edges, &out)?;
        }
        // Each edge bounds two faces, and separate bodies share no vertex.
        if users.iter().any(|u| u.len() > 2) {
            return Err(KernelError::Invalid("sew: nonmanifold edge contact".into()));
        }
        if users.iter().any(|u| u.len() != 2) {
            return Err(KernelError::Invalid("sew: the faces do not close".into()));
        }
        let (groups, group_of) = components(out.len(), &users);
        if groups.len() > b::MAX_SHELLS {
            return Err(KernelError::Invalid("sew: component budget exceeded".into()));
        }
        let mut vertex_group: Vec<Option<usize>> = vec![None; vertices.len()];
        for (e, edge) in edges.iter().enumerate() {
            let Some(&g) = users.get(e).and_then(|u| u.first()).and_then(|f| group_of.get(*f)) else { continue };
            for v in edge.vertices {
                let slot = vertex_group.get_mut(v as usize).ok_or_else(|| KernelError::Failed("sew: vertex reference".into()))?;
                if slot.is_some_and(|other| other != g) {
                    return Err(KernelError::Invalid("sew: bodies touch only at a vertex".into()));
                }
                *slot = Some(g);
            }
        }
        let made = assemble(Parts { vertices, edges, faces: out, tolerance: tol }, true)?;
        let smlib = solidcraft_smlib::Brep::from_record(&made.brep, true).map_err(|e| KernelError::Failed(format!("sew: SMLib: {e}")))?;
        let (body, order) = crate::native::body_from_smlib_ordered(&smlib)?;
        // The face of `faces` each face of the body was built from: record face `c` is SMLib's
        // face `c`, which the order places in the body.
        let from = order.and_then(|o| {
            let mut from = vec![None; body.face_count()];
            for (spec, &c) in made.faces.iter().enumerate() {
                let j = if o.faces.is_empty() { c } else { *o.faces.get(c)? };
                *from.get_mut(j)? = Some(spec);
            }
            from.into_iter().collect::<Option<Vec<usize>>>()
        });
        let mapped = sources.map(|s| {
            let mapped: Vec<Option<crate::FaceSource>> = match &from {
                Some(from) => from.iter().map(|i| s.get(*i).cloned().flatten()).collect(),
                None => vec![None; body.face_count()],
            };
            Arc::new(mapped)
        });
        Ok((body.with_face_sources(mapped), from))
    })
}

/// One face to [`assemble`].
pub(crate) struct PartFace {
    pub surface: b::Surface,
    /// The surface's natural normal points out of the material (on a sheet: to its front).
    pub forward: bool,
    /// The loops, counter-clockwise about the face's outward normal, the outer one first.
    pub loops: Vec<Vec<PartUse>>,
}

/// One use of an edge in a loop: whether the loop runs along it, and its exact pcurve where the
/// caller holds one (on the edge's interval, in its direction).
pub(crate) struct PartUse {
    pub edge: u32,
    pub forward: bool,
    pub given: Option<Curve2>,
}

pub(crate) struct PartEdge {
    pub curve: b::Curve,
    pub interval: [f64; 2],
    pub vertices: [u32; 2],
}

/// Faces on shared edges and vertices, to be made a record; `tolerance` is every vertex's and
/// edge's, and the record's.
pub(crate) struct Parts {
    pub vertices: Vec<[f64; 3]>,
    pub edges: Vec<PartEdge>,
    pub faces: Vec<PartFace>,
    pub tolerance: f64,
}

/// An assembled record, with the canonical index of each face of the parts and of each edge
/// (`None` for an edge no face uses).
pub(crate) struct Assembled {
    pub brep: b::Brep,
    pub faces: Vec<usize>,
    pub edges: Vec<Option<usize>>,
}

/// The schema 4 record of `parts`: each face its pcurves (the caller's exact ones where given,
/// else `pcurve::pcurves`) and parameter box; the faces grouped into shells through shared edges;
/// closed shells (every edge used twice in them) enclosing material, or a cavity where the
/// faces' outward normals enclose negative volume, each cavity in the solid around it; open
/// shells (sheets) in the infinite region; everything in canonical order and validated.
/// `solid`: every shell must be closed.
pub(crate) fn assemble(parts: Parts, solid: bool) -> Result<Assembled> {
    let fail = |m: String| KernelError::Invalid(format!("sew: {m}"));
    let Parts { vertices, edges, faces, tolerance: tol } = parts;
    let nf = faces.len();
    if nf == 0 {
        return Err(fail("no faces".into()));
    }
    // The loops as the record keeps them: relative to the surface's natural normal.
    let loops: Vec<Vec<Vec<(u32, bool, Option<&Curve2>)>>> = faces
        .iter()
        .map(|f| {
            f.loops
                .iter()
                .map(|l| {
                    if f.forward {
                        l.iter().map(|u| (u.edge, u.forward, u.given.as_ref())).collect()
                    } else {
                        l.iter().rev().map(|u| (u.edge, !u.forward, u.given.as_ref())).collect()
                    }
                })
                .collect()
        })
        .collect();
    // Every circle and ellipse edge on its NURBS form (an edge the caller gave a pcurve for keeps
    // the parameter that pcurve is on).
    let given_on: Vec<bool> = {
        let mut g = vec![false; edges.len()];
        for u in faces.iter().flat_map(|f| f.loops.iter().flatten()).filter(|u| u.given.is_some()) {
            if let Some(slot) = g.get_mut(u.edge as usize) {
                *slot = true;
            }
        }
        g
    };
    // A circle on a cylinder, cone, sphere or torus about its axis is labelled on that face's
    // frame, so its form lies on the face's quarter grid and its pcurve there is a straight line.
    let mut frame_of: Vec<Option<&b::Frame>> = vec![None; edges.len()];
    for f in &faces {
        let (b::Surface::Cylinder { frame, .. }
        | b::Surface::Cone { frame, .. }
        | b::Surface::Sphere { frame, .. }
        | b::Surface::Torus { frame, .. }) = &f.surface
        else {
            continue;
        };
        for u in f.loops.iter().flatten() {
            if let Some(slot) = frame_of.get_mut(u.edge as usize).filter(|s| s.is_none()) {
                *slot = Some(frame);
            }
        }
    }
    let labels: Vec<(b::Curve, [f64; 2])> = edges
        .iter()
        .zip(&frame_of)
        .map(|(e, frame)| frame.and_then(|f| b::form::on_frame(&e.curve, e.interval, f, tol)).unwrap_or_else(|| (e.curve.clone(), e.interval)))
        .collect();
    let forms: Vec<Option<(b::Curve, [f64; 2])>> = labels
        .iter()
        .zip(&given_on)
        .map(|((curve, interval), given)| match curve {
            b::Curve::Circle { .. } | b::Curve::Ellipse { .. } if !given => {
                b::form::curve(curve, *interval).ok_or_else(|| fail("an arc of more than one turn".into())).map(Some)
            }
            _ => Ok(None),
        })
        .collect::<Result<_>>()?;
    // Each face on its NURBS form: a cylinder, cone, sphere or torus over the extent its label's
    // pcurves give, then every pcurve again on the forms, exactly.
    let mut pcs: Vec<Vec<Vec<Curve2>>> = Vec::with_capacity(nf);
    let mut domains = Vec::with_capacity(nf);
    let mut face_forms: Vec<Option<b::Surface>> = Vec::with_capacity(nf);
    for (fi, (f, ls)) in faces.iter().zip(&loops).enumerate() {
        let failed = |e: pcurve::Error| KernelError::Failed(format!("sew: face {fi}: {e}"));
        let form = match f.surface {
            b::Surface::Cylinder { .. } | b::Surface::Cone { .. } | b::Surface::Sphere { .. } | b::Surface::Torus { .. } => {
                let label = pcurve::pcurves(&f.surface, &uses_of(&edges, &forms, ls, false, tol)?, tol).map_err(failed)?;
                let extent = domain_of(&f.surface, &label).ok_or_else(|| fail(format!("face {fi} has no area in its parameters")))?;
                Some(b::form::surface(&f.surface, extent).ok_or_else(|| fail(format!("face {fi}: more than one turn")))?.0)
            }
            _ => None,
        };
        let on = form.as_ref().unwrap_or(&f.surface);
        let pc = pcurve::pcurves(on, &uses_of(&edges, &forms, ls, true, tol)?, tol).map_err(failed)?;
        domains.push(domain_of(on, &pc).ok_or_else(|| fail(format!("face {fi} has no area in its parameters")))?);
        pcs.push(pc);
        face_forms.push(form);
    }
    let users = edge_users(edges.len(), &faces)?;
    let (groups, _) = components(nf, &users);
    if groups.len() > b::MAX_SHELLS {
        return Err(fail("component budget exceeded".into()));
    }
    let closed: Vec<bool> = groups
        .iter()
        .map(|g| {
            g.iter()
                .flat_map(|f| faces.get(*f).into_iter().flat_map(|f| f.loops.iter().flatten()))
                .all(|u| users.get(u.edge as usize).is_some_and(|x| x.len() >= 2))
        })
        .collect();
    if solid && closed.iter().any(|c| !c) {
        return Err(fail("the faces do not close".into()));
    }
    // Closed shells: the volume their outward normals enclose, positive for a solid's outside.
    let flux_of = |g: &[usize], field: &dyn Fn(Vec3) -> Vec3| -> Result<f64> {
        let mut sum = 0.0;
        for &f in g {
            let (Some(face), Some(pc), Some(ls), Some(dom)) = (faces.get(f), pcs.get(f), loops.get(f), domains.get(f)) else { continue };
            let on = face_forms.get(f).and_then(Option::as_ref).unwrap_or(&face.surface);
            let s = SurfaceEval::new(on).map_err(|e| KernelError::Failed(format!("sew: face {f}: {e}")))?;
            let uses: Vec<Vec<(&Curve2, bool)>> = pc.iter().zip(ls).map(|(c, l)| c.iter().zip(l).map(|(c, u)| (c, u.1)).collect()).collect();
            let flux = face_flux(&s, dom[1][0], &uses, field).ok_or_else(|| KernelError::Failed(format!("sew: face {f} cannot be evaluated")))?;
            sum += if face.forward { flux } else { -flux };
        }
        Ok(sum)
    };
    let mut volume = vec![0.0; groups.len()];
    for (k, g) in groups.iter().enumerate() {
        if closed.get(k).copied().unwrap_or(false) {
            let v = flux_of(g, &|p| p * (1.0 / 3.0))?;
            if !(v.abs() > 0.0) || !v.is_finite() {
                return Err(fail("a closed shell encloses no volume".into()));
            }
            if let Some(slot) = volume.get_mut(k) {
                *slot = v;
            }
        }
    }
    let outers: Vec<usize> = (0..groups.len()).filter(|k| volume.get(*k).is_some_and(|v| *v > 0.0)).collect();
    // Each cavity in the smallest solid around it (by the winding number of one of its points).
    let mut cavities: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
    for k in (0..groups.len()).filter(|k| volume.get(*k).is_some_and(|v| *v < 0.0)) {
        let around = if let [only] = outers.as_slice() {
            Some(*only)
        } else {
            let q = groups
                .get(k)
                .and_then(|g| g.first())
                .and_then(|f| faces.get(*f))
                .and_then(|f| f.loops.first()?.first())
                .and_then(|u| edges.get(u.edge as usize))
                .and_then(|e| vertices.get(e.vertices[0] as usize))
                .map(|p| v3(*p))
                .ok_or_else(|| fail("cavity".into()))?;
            let field = move |p: Vec3| {
                let d = p - q;
                let r = d.len();
                d * (1.0 / (4.0 * std::f64::consts::PI * r * r * r))
            };
            let mut best: Option<(f64, usize)> = None;
            for &o in &outers {
                let w = flux_of(groups.get(o).map_or(&[][..], Vec::as_slice), &field)?;
                let v = volume.get(o).copied().unwrap_or(0.0);
                if (w - 1.0).abs() < 0.5 && best.is_none_or(|(b, _)| v < b) {
                    best = Some((v, o));
                }
            }
            best.map(|(_, o)| o)
        };
        let o = around.ok_or_else(|| fail("a cavity lies in no solid".into()))?;
        cavities.get_mut(o).ok_or_else(|| fail("cavity".into()))?.push(k);
    }
    // The record as its parts stand (shell k is group k, face i part face i, edges and vertices
    // as given), then put in canonical order.
    let index = |i: usize| u32::try_from(i).map_err(|_| fail("index".into()));
    let open: Vec<usize> = (0..groups.len()).filter(|k| !closed.get(*k).copied().unwrap_or(false)).collect();
    let mut regions = vec![b::Region { shells: outers.iter().chain(&open).map(|g| index(*g)).collect::<Result<_>>()?, void: false }];
    for &o in &outers {
        let mut shells = vec![index(o)?];
        for &c in cavities.get(o).into_iter().flatten() {
            shells.push(index(c)?);
        }
        regions.push(b::Region { shells, void: false });
    }
    for &o in &outers {
        for &c in cavities.get(o).into_iter().flatten() {
            regions.push(b::Region { shells: vec![index(c)?], void: true });
        }
    }
    let shells = groups
        .iter()
        .map(|g| {
            let faces = g
                .iter()
                .map(|&f| Ok(b::OrientedFace { face: index(f)?, forward: faces.get(f).is_some_and(|x| x.forward) }))
                .collect::<Result<_>>()?;
            Ok(b::Shell::Faces { faces, wires: Vec::new() })
        })
        .collect::<Result<_>>()?;
    let mut record_faces = Vec::with_capacity(nf);
    for (f, ((face, ls), (pc, domain))) in faces.iter().zip(&loops).zip(pcs.iter().zip(&domains)).enumerate() {
        let mut record_loops = Vec::with_capacity(ls.len());
        for (l, c) in ls.iter().zip(pc) {
            let oriented = l.iter().zip(c).map(|(&(edge, forward, _), pcurve)| b::OrientedEdge { edge, forward, pcurve: pcurve.clone() }).collect();
            record_loops.push(b::Loop::Edges { edges: oriented });
        }
        if record_loops.is_empty() {
            return Err(fail(format!("face {f} has no loop")));
        }
        let nurbs = face_forms.get(f).cloned().flatten();
        record_faces.push(b::Face { nurbs, surface: face.surface.clone(), domain: *domain, loops: record_loops });
    }
    let record_edges = edges
        .iter()
        .zip(forms.iter().zip(&labels))
        .map(|(e, (form, (label, label_interval)))| {
            let (nurbs, interval) = match form {
                Some((c, i)) => (Some(c.clone()), *i),
                None => (None, *label_interval),
            };
            b::Edge { nurbs, curve: label.clone(), interval, vertices: e.vertices, tolerance: tol, uses: Vec::new() }
        })
        .collect();
    let record_vertices = vertices.iter().map(|point| b::Vertex { point: *point, tolerance: tol }).collect();
    let loose = b::Brep { version: b::VERSION, tolerance: tol, regions, shells, faces: record_faces, edges: record_edges, vertices: record_vertices };
    let part = loose.canonical().map_err(|e| KernelError::Failed(format!("sew: the record is invalid: {e}")))?;
    let mut face_new = vec![usize::MAX; nf];
    for (c, &f) in part.faces.iter().enumerate() {
        if let Some(slot) = face_new.get_mut(f as usize) {
            *slot = c;
        }
    }
    let mut edge_new = vec![None; edges.len()];
    for (c, &e) in part.edges.iter().enumerate() {
        if let Some(slot) = edge_new.get_mut(e as usize) {
            *slot = Some(c);
        }
    }
    Ok(Assembled { brep: part.brep, faces: face_new, edges: edge_new })
}

/// A face's parameter box: around its pcurves' control points (a B-spline lies in their hull),
/// held inside the surface's natural domain (one turn where periodic). None when it has no
/// width in either direction.
/// A face's edge uses for `pcurve::pcurves`: on each edge's NURBS form where it has one and
/// `formed`, else on its curve as given.
fn uses_of<'a>(
    edges: &'a [PartEdge],
    forms: &'a [Option<(b::Curve, [f64; 2])>],
    ls: &[Vec<(u32, bool, Option<&'a Curve2>)>],
    formed: bool,
    tol: f64,
) -> Result<Vec<Vec<pcurve::EdgeUse<'a>>>> {
    ls.iter()
        .map(|l| {
            l.iter()
                .map(|&(e, forward, given)| {
                    let edge = edges.get(e as usize).ok_or_else(|| KernelError::Invalid("sew: edge reference".into()))?;
                    let (curve, interval) = match forms.get(e as usize).and_then(Option::as_ref) {
                        Some((c, i)) if formed => (c, *i),
                        _ => (&edge.curve, edge.interval),
                    };
                    Ok(pcurve::EdgeUse { edge: e, curve, interval, forward, tolerance: tol, given })
                })
                .collect()
        })
        .collect()
}

fn domain_of(surface: &b::Surface, pcs: &[Vec<Curve2>]) -> Option<[[f64; 2]; 2]> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for c in pcs.iter().flatten() {
        let points: Vec<[f64; 2]> = match c {
            Curve2::Spline { points, .. } => points.clone(),
            Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
        };
        for p in points {
            for d in 0..2 {
                lo[d] = lo[d].min(p[d]);
                hi[d] = hi[d].max(p[d]);
            }
        }
    }
    let natural = surface.natural();
    let mut out = [[0.0; 2]; 2];
    for d in 0..2 {
        let (mut a, mut z) = (lo[d], hi[d]);
        match natural[d] {
            b::Natural::Periodic => z = z.min(a + std::f64::consts::TAU),
            b::Natural::Range(n0, n1) => {
                a = a.max(n0);
                z = z.min(n1);
            }
        }
        if !(a < z) {
            return None;
        }
        out[d] = [a, z];
    }
    Some(out)
}

/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]`.
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];

/// Pieces each pcurve is integrated in, and how many Gauss panels cover a v range.
const FLUX_PIECES: usize = 32;
const FLUX_PANELS: usize = 4;

/// The flux of `field` through a face, `∫∫ field(S)·(S_u × S_v) du dv` over the (u, v) region its
/// loops (record convention, each use its pcurve and direction) bound, by Green's theorem:
/// `∮ −G du` with `G(u, v) = ∫ field·N ds` from `v0` to `v`. A quadrature for the sign of a
/// shell's volume (`field = p / 3`) or a winding number, which only has to be right to well
/// under a half: no geometry is made from it. Loops round a periodic direction need no seam
/// edge, as a seam's two runs at constant u add nothing to `∮ G du`.
fn face_flux(s: &SurfaceEval, v0: f64, loops: &[Vec<(&Curve2, bool)>], field: &dyn Fn(Vec3) -> Vec3) -> Option<f64> {
    let g = |u: f64, v: f64| -> Option<f64> {
        let (mut sum, h) = (0.0, (v - v0) / FLUX_PANELS as f64);
        for panel in 0..FLUX_PANELS {
            let a = v0 + h * panel as f64;
            for (x, w) in GAUSS4 {
                let t = a + 0.5 * h * (x + 1.0);
                let (p, su, sv) = s.derivs(u, t)?;
                sum += 0.5 * h * w * field(p).dot(su.cross(sv));
            }
        }
        sum.is_finite().then_some(sum)
    };
    let mut total = 0.0;
    for (c, forward) in loops.iter().flatten() {
        let [t0, t1] = c.interval()?;
        let at = |k: f64| {
            let f = k / FLUX_PIECES as f64;
            // Exactly the ends at f = 0 and 1, whatever the interval's rounding.
            c.eval(if *forward { t0 * (1.0 - f) + t1 * f } else { t1 * (1.0 - f) + t0 * f })
        };
        for k in 0..FLUX_PIECES {
            let (a, m, z) = (at(k as f64)?, at(k as f64 + 0.5)?, at(k as f64 + 1.0)?);
            total -= g(m[0], m[1])? * (z[0] - a[0]);
        }
    }
    total.is_finite().then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> FaceSpec {
        FaceSpec {
            loops: vec![vec![EdgeSpec::Line { a, b }, EdgeSpec::Line { a: b, b: c }, EdgeSpec::Line { a: c, b: d }, EdgeSpec::Line { a: d, b: a }]],
            surface: SurfSpec::Plane,
            probe: None,
        }
    }

    fn box_faces(origin: Vec3, size: Vec3) -> Vec<FaceSpec> {
        let p = |x: f64, y: f64, z: f64| origin + Vec3::new(x * size.x, y * size.y, z * size.z);
        let (a, b, c, d) = (p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.));
        let (e, f, g, h) = (p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.));
        vec![quad(a, d, c, b), quad(e, f, g, h), quad(a, b, f, e), quad(b, c, g, f), quad(c, d, h, g), quad(d, a, e, h)]
    }

    /// A chain's key is the same whichever way it runs. These control points (a joining stroke
    /// of a script font) have their mean exactly halfway between two steps of the key, and
    /// summed in order one way and the other they rounded to different steps: a wall and the cap
    /// beside it, reading the curve opposite ways, got two edges and the solid did not close.
    #[test]
    fn a_chain_has_one_key_both_ways() {
        let p = |x: f64, y: f64| Vec3::new(x, y, 40.0);
        let ctrl = [
            p(34.74439583333333, 11.397236111111111),
            p(35.066196759259256, 11.722652777777778),
            p(35.32833796296296, 11.885361111111111),
            p(35.53081944444444, 11.885361111111111),
        ];
        let forward = EdgeSpec::Chain { degree: 3, ctrl: ctrl.to_vec().into() };
        assert_eq!(forward.mid_key(), forward.reversed().mid_key());
    }

    #[test]
    fn disconnected_closed_faces_keep_every_material_component() {
        let mut faces = box_faces(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        faces.extend(box_faces(Vec3::X * 10.0, Vec3::new(2.0, 3.0, 4.0)));
        for reverse in [false, true] {
            if reverse {
                faces.reverse();
            }
            let body = sew(&faces).unwrap();
            assert_eq!(body.shell_count(), 2);
            assert!(body.validity().is_empty());
            assert!((crate::measure(&body).unwrap().volume - 48.0).abs() < 1e-9);
            let lumps = body.lumps().unwrap();
            assert_eq!(lumps.len(), 2);
            assert!(lumps.iter().all(|b| (crate::measure(b).unwrap().volume - 24.0).abs() < 1e-9));
            let mesh = body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
            assert!(mesh.contains(Vec3::new(1.0, 1.0, 1.0)));
            assert!(mesh.contains(Vec3::new(11.0, 1.0, 1.0)));
            assert!(!mesh.contains(Vec3::new(5.0, 1.0, 1.0)));
            let imported = crate::step_import(&crate::step_export(&[&body], "sewn pieces").unwrap()).unwrap();
            assert!(imported.warnings.is_empty());
            let volume: f64 = imported.bodies.iter().map(|b| crate::measure(&b.body).unwrap().volume).sum();
            assert!((volume - 48.0).abs() < 1e-6);
        }
    }

    #[test]
    fn sewing_preserves_inward_cavity_orientation_and_refuses_vertex_contact() {
        let outer = box_faces(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        let mut inner = box_faces(Vec3::new(0.5, 0.75, 1.0), Vec3::new(1.0, 1.5, 2.0));
        for face in &mut inner {
            for lp in &mut face.loops {
                lp.reverse();
                for edge in lp {
                    if let EdgeSpec::Line { a, b } = edge {
                        std::mem::swap(a, b);
                    }
                }
            }
        }
        let cavity = sew(&outer.iter().chain(&inner).cloned().collect::<Vec<_>>()).unwrap();
        assert_eq!(cavity.shell_count(), 2);
        assert_eq!(cavity.lumps().unwrap().len(), 1);
        assert!((crate::measure(&cavity).unwrap().volume - 21.0).abs() < 1e-9);
        assert!(!cavity.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().contains(Vec3::new(1.0, 1.5, 2.0)));
        let touching = box_faces(Vec3::new(2.0, 3.0, 4.0), Vec3::new(2.0, 3.0, 4.0));
        assert!(sew(&outer.iter().chain(&touching).cloned().collect::<Vec<_>>()).unwrap_err().to_string().contains("vertex"));
        let mut hostile = outer.clone();
        hostile[0].loops = vec![Vec::new(); 500_001];
        assert!(sew(&hostile).unwrap_err().to_string().contains("budget"));
    }

    #[test]
    fn flat_arcs_far_from_the_origin_keep_their_ends_and_their_faces_planes() {
        // A 12 x 30 x 2 slab 1000 mm out whose y = 0 side bows 0.00008 mm (a 225 m radius).
        for (o, bow) in [(Vec3::new(1000.0, 500.0, 0.0), 8e-5), (Vec3::new(-1238.4, 4.0, 14.0), 8e-5), (Vec3::ZERO, 0.5)] {
            let p = |x: f64, y: f64, z: f64| o + Vec3::new(x, y, z);
            let (a, b, c, d) = (p(0., 0., 0.), p(12., 0., 0.), p(12., 30., 0.), p(0., 30., 0.));
            let up = Vec3::new(0., 0., 2.);
            let (e, f, g, h) = (a + up, b + up, c + up, d + up);
            let (m0, m1) = (p(6., -bow, 0.), p(6., -bow, 2.));
            let line = |a, b| EdgeSpec::Line { a, b };
            let plane = |l: Vec<EdgeSpec>| FaceSpec { loops: vec![l], surface: SurfSpec::Plane, probe: None };
            let wall = FaceSpec {
                loops: vec![vec![EdgeSpec::Arc { a, b, mid: m0 }, line(b, f), EdgeSpec::Arc { a: f, b: e, mid: m1 }, line(e, a)]],
                surface: SurfSpec::Extruded { a, b, mid: m0, dir: up },
                probe: Some((p(6., -bow, 1.), Vec3::new(0., -1., 0.))),
            };
            let faces = vec![
                plane(vec![line(a, d), line(d, c), line(c, b), EdgeSpec::Arc { a: b, b: a, mid: m0 }]),
                plane(vec![EdgeSpec::Arc { a: e, b: f, mid: m1 }, line(f, g), line(g, h), line(h, e)]),
                wall,
                quad(b, c, g, f),
                quad(c, d, h, g),
                quad(d, a, e, h),
            ];
            let body = sew(&faces).unwrap_or_else(|e| panic!("{o:?} {bow}: {e}"));
            assert!(body.validity().is_empty(), "{o:?}");
            // The circular segment added to the slab (the measure follows flat arcs by chords).
            let r = (36.0 + bow * bow) / (2.0 * bow);
            let half = (6.0 / r).asin();
            let segment = r * r * (half - half.sin() * half.cos());
            let want = (360.0 + segment) * 2.0;
            let got = crate::measure(&body).unwrap().volume;
            assert!((got - want).abs() < 1e-4 * want, "{o:?} {bow}: {got} vs {want}");
        }
    }

    #[test]
    fn a_sewn_box() {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let (a, b, c, d) = (p(0., 0., 0.), p(2., 0., 0.), p(2., 3., 0.), p(0., 3., 0.));
        let (e, f, g, h) = (p(0., 0., 4.), p(2., 0., 4.), p(2., 3., 4.), p(0., 3., 4.));
        let faces = vec![quad(a, d, c, b), quad(e, f, g, h), quad(a, b, f, e), quad(b, c, g, f), quad(c, d, h, g), quad(d, a, e, h)];
        let body = sew(&faces).unwrap();
        let m = crate::measure(&body).unwrap();
        assert!((m.volume - 24.0).abs() < 1e-9, "{}", m.volume);
        assert_eq!(body.face_count(), 6);
        assert!(sew(&faces[..5]).is_err());
    }

    /// A 2.0000005 x 3 x 4 box: x = 2.0000005 mm is a border between two cells of the 1e-6 mm
    /// grid vertices are found on. The bottom face's edges meet at its corner (x, 3, 0) 2e-9 mm
    /// to either side of the border, as an arc's end and the end of the line it meets can: the
    /// ends weld and the box closes. Ends 3e-6 mm apart stay apart.
    #[test]
    fn edge_ends_a_hair_apart_across_a_grid_border_weld() {
        let x = 2.0000005;
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let (a, b, c, d) = (p(0., 0., 0.), p(x, 0., 0.), p(x, 3., 0.), p(0., 3., 0.));
        let (e, f, g, h) = (p(0., 0., 4.), p(x, 0., 4.), p(x, 3., 4.), p(0., 3., 4.));
        let line = |a, b| EdgeSpec::Line { a, b };
        let faces = |into: Vec3, from: Vec3| {
            let bottom = FaceSpec { loops: vec![vec![line(a, d), line(d, into), line(from, b), line(b, a)]], surface: SurfSpec::Plane, probe: None };
            vec![bottom, quad(e, f, g, h), quad(a, b, f, e), quad(b, c, g, f), quad(c, d, h, g), quad(d, a, e, h)]
        };
        let (lo, hi) = (p(x - 2e-9, 3., 0.), p(x + 2e-9, 3., 0.));
        assert_ne!(key(lo), key(hi));
        for (into, from) in [(lo, hi), (hi, lo), (lo, c), (hi, c)] {
            let body = sew(&faces(into, from)).unwrap_or_else(|e| panic!("{into:?} {from:?}: {e}"));
            assert!(body.validity().is_empty(), "{:?}", body.validity());
            let v = crate::measure(&body).unwrap().volume;
            assert!((v - x * 12.0).abs() < 1e-6, "{v}");
        }
        assert!(sew(&faces(p(x - 2e-6, 3., 0.), p(x + 1e-6, 3., 0.))).is_err());
    }

    /// A 4 × 3 × 2 box whose +x wall bulges out as the cubic (4,0) (5,0) (5,3) (4,3), given as a
    /// chain of its two halves at the bottom (`z = 0`) and the top, the wall ruled between them.
    fn bulged_box(bottom: EdgeSpec, top: EdgeSpec) -> Vec<FaceSpec> {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let (a, b, c, d) = (p(0., 0., 0.), p(4., 0., 0.), p(4., 3., 0.), p(0., 3., 0.));
        let (e, f, g, h) = (p(0., 0., 2.), p(4., 0., 2.), p(4., 3., 2.), p(0., 3., 2.));
        let line = |a: Vec3, b: Vec3| EdgeSpec::Line { a, b };
        let flat = |edges: Vec<EdgeSpec>| FaceSpec { loops: vec![edges], surface: SurfSpec::Plane, probe: None };
        vec![
            flat(vec![line(a, d), line(d, c), bottom.reversed(), line(b, a)]),
            flat(vec![line(e, f), top.clone(), line(g, h), line(h, e)]),
            quad(a, b, f, e),
            quad(c, d, h, g),
            quad(d, a, e, h),
            FaceSpec {
                loops: vec![vec![bottom.clone(), line(c, g), top.reversed(), line(f, b)]],
                surface: SurfSpec::Ruled { a: bottom, b: top },
                probe: Some((p(4.75, 1.5, 1.0), Vec3::X)),
            },
        ]
    }

    /// The bulge's cubic split at its middle (de Casteljau), at height `z`.
    fn bulge(z: f64) -> EdgeSpec {
        let ctrl = [(4.0, 0.0), (4.5, 0.0), (4.75, 0.75), (4.75, 1.5), (4.75, 2.25), (4.5, 3.0), (4.0, 3.0)];
        EdgeSpec::Chain { degree: 3, ctrl: ctrl.iter().map(|(x, y)| Vec3::new(*x, *y, z)).collect() }
    }

    /// The bulge's cubic raised to degree 5, (4,0) (4.6,0) (4.9,0.9) (4.9,2.1) (4.6,3) (4,3), and
    /// split at its middle (de Casteljau), at height `z`: the same curve as a quintic chain.
    fn quintic_bulge(z: f64) -> EdgeSpec {
        let raised = [(4.0, 0.0), (4.6, 0.0), (4.9, 0.9), (4.9, 2.1), (4.6, 3.0), (4.0, 3.0)].map(|(x, y)| Vec3::new(x, y, z));
        let (mut left, mut right, mut w) = (Vec::new(), Vec::new(), raised.to_vec());
        while let (Some(first), Some(last)) = (w.first().copied(), w.last().copied()) {
            left.push(first);
            right.push(last);
            w = w.windows(2).map(|p| (p[0] + p[1]) * 0.5).collect();
        }
        right.reverse();
        left.extend(right.into_iter().skip(1));
        EdgeSpec::Chain { degree: 5, ctrl: left.into() }
    }

    #[test]
    fn a_ruled_wall_between_bezier_chains_closes_the_solid() {
        let body = sew(&bulged_box(bulge(0.0), bulge(2.0))).unwrap();
        assert_eq!(body.face_count(), 6);
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        // x = 4 + 3t(1 − t), y = 3(3t² − 2t³): the bulge adds ∫ (x − 4) dy = 54 ∫ t²(1 − t)² dt
        // = 1.8 to the 4 × 3 section; the solid is that section 2 high.
        let m = crate::measure(&body).unwrap();
        assert!((m.volume - 27.6).abs() < 2e-3, "{}", m.volume);
        assert!((m.bbox.max.x - 4.75).abs() < 1e-3, "{:?}", m.bbox);
    }

    #[test]
    fn a_ruled_wall_between_quintic_chains_closes_the_same_solid() {
        let EdgeSpec::Chain { ctrl, .. } = quintic_bulge(0.0) else { panic!() };
        assert_eq!(ctrl.len(), 11);
        let body = sew(&bulged_box(quintic_bulge(0.0), quintic_bulge(2.0))).unwrap();
        assert_eq!(body.face_count(), 6);
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        let m = crate::measure(&body).unwrap();
        assert!((m.volume - 27.6).abs() < 2e-3, "{}", m.volume);
        assert!((m.bbox.max.x - 4.75).abs() < 1e-3, "{:?}", m.bbox);
    }

    #[test]
    fn hostile_bezier_chains_are_refused() {
        let chain = |ctrl: Vec<Vec3>| EdgeSpec::Chain { degree: 3, ctrl: ctrl.into() };
        let EdgeSpec::Chain { ctrl, .. } = bulge(2.0) else { panic!() };
        // Not 3n + 1 control points; a control point that is not a number; too many pieces.
        let mut nan = ctrl.to_vec();
        nan[2].x = f64::NAN;
        let long: Vec<Vec3> = (0..MAX_CHAIN_POINTS + 3).map(|i| Vec3::new(4.0, 3.0 * i as f64 / (MAX_CHAIN_POINTS + 2) as f64, 2.0)).collect();
        for top in [chain(ctrl[..6].to_vec()), chain(ctrl[..3].to_vec()), chain(Vec::new()), chain(nan), chain(long)] {
            assert!(sew(&bulged_box(bulge(0.0), top)).is_err());
        }
        // A ruled wall needs chains with the same number of pieces.
        let whole = chain(vec![Vec3::new(4.0, 0.0, 2.0), Vec3::new(5.0, 0.0, 2.0), Vec3::new(5.0, 3.0, 2.0), Vec3::new(4.0, 3.0, 2.0)]);
        let e = sew(&bulged_box(bulge(0.0), whole)).unwrap_err().to_string();
        assert!(e.contains("ruled chains differ"), "{e}");
        // Degree 0, or above the highest, or a count of control points that doesn't fit the degree
        // (11 control points are no chain of quartics).
        let EdgeSpec::Chain { ctrl: fifth, .. } = quintic_bulge(2.0) else { panic!() };
        let degree = |degree: usize, ctrl: &[Vec3]| EdgeSpec::Chain { degree, ctrl: ctrl.to_vec().into() };
        // `n` control points bulging out from (4, 0) to (4, 3) at height `z`.
        let arch = |n: usize, z: f64| -> Vec<Vec3> {
            let at = |i: usize| i as f64 / (n - 1) as f64;
            (0..n).map(|i| Vec3::new(4.0 + (std::f64::consts::PI * at(i)).sin(), 3.0 * at(i), z)).collect()
        };
        for top in [degree(0, &ctrl), degree(MAX_CHAIN_DEGREE + 1, &arch(MAX_CHAIN_DEGREE + 2, 2.0)), degree(4, &fifth)] {
            assert!(sew(&bulged_box(bulge(0.0), top)).is_err());
        }
        // A ruled wall needs chains of one degree: 16 control points are five cubic pieces or
        // three quintic ones.
        let e = sew(&bulged_box(degree(3, &arch(16, 0.0)), degree(5, &arch(16, 2.0)))).unwrap_err().to_string();
        assert!(e.contains("ruled chains differ"), "{e}");
    }

    #[test]
    fn thin_faces_away_from_the_origin_get_their_planes() {
        // Slabs 30 x 2 mm and 0.00013 to 0.1 mm thick, away from the origin and turned off the
        // axes. A plane fitted by summing the cross products of a boundary's points about a far
        // centre turns a thin face's short normal so far that a 0.00013 x 30 mm face at
        // (-300, -200, 5) lies 2e-6 mm out of it; summed about the boundary's mean it lies
        // within 1e-14 mm.
        let turned = |about_z: f64, about_x: f64| {
            let (sz, cz) = about_z.sin_cos();
            let (sx, cx) = about_x.sin_cos();
            // The x, y and z axes turned about x, then about z.
            (Vec3::new(cz, sz, 0.0), Vec3::new(-sz * cx, cz * cx, sx), Vec3::new(sz * sx, -cz * sx, cx))
        };
        // A box on the corner `o` with the edges `u`, `v` and `w` (right-handed).
        let skew_box = |o: Vec3, u: Vec3, v: Vec3, w: Vec3| {
            let p = |x: f64, y: f64, z: f64| o + u * x + v * y + w * z;
            let (a, b, c, d) = (p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.));
            let (e, f, g, h) = (p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.));
            vec![quad(a, d, c, b), quad(e, f, g, h), quad(a, b, f, e), quad(b, c, g, f), quad(c, d, h, g), quad(d, a, e, h)]
        };
        for o in [Vec3::new(-300.0, -200.0, 5.0), Vec3::new(2500.0, -1800.0, 40.0), Vec3::ZERO] {
            for width in [1.3e-4, 1e-3, 0.1] {
                for (about_z, about_x) in [(0.0, 0.0), (0.0, 0.45), (0.9, 0.0), (-0.65, 0.45), (2.2, 1.1)] {
                    let tag = format!("{o:?}, {width} wide, turned {about_z} and {about_x}");
                    let (u, v, w) = turned(about_z, about_x);
                    let body = sew(&skew_box(o, u * width, v * 2.0, w * 30.0)).unwrap_or_else(|e| panic!("{tag}: {e}"));
                    assert!(body.validity().is_empty() && body.shell_count() == 1 && body.face_count() == 6, "{tag}: {:?}", body.validity());
                    // (The measure sums about the origin: it is good to about 1e-14 of the
                    // distance cubed, which three metres out is more than the thinnest slabs hold.)
                    let (got, want) = (crate::measure(&body).unwrap().volume, width * 60.0);
                    assert!((got - want).abs() < 1e-5 * want + 1e-14 * o.len().powi(3) + 1e-9, "{tag}: {got} vs {want}");
                }
            }
        }
        // A boundary that leaves its plane is refused as before: one corner of a face lifted by
        // a thousandth of a millimetre, on a cube and on a sliver.
        for size in [Vec3::new(10.0, 10.0, 10.0), Vec3::new(10.0, 2e-4, 10.0)] {
            let mut warped = box_faces(Vec3::new(500.0, 300.0, 0.0), size);
            if let EdgeSpec::Line { b, .. } = &mut warped[1].loops[0][1] {
                b.z += 1e-3;
            }
            if let EdgeSpec::Line { a, .. } = &mut warped[1].loops[0][2] {
                a.z += 1e-3;
            }
            let e = sew(&warped).unwrap_err().to_string();
            assert!(e.contains("face 1") && e.contains("not on one plane"), "{e}");
        }
        // So is one with no area.
        let mut flat = box_faces(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0));
        let p = Vec3::new(0.0, 0.0, 10.0);
        flat[1] = quad(p, p + Vec3::X * 3.0, p + Vec3::X * 7.0, p + Vec3::X * 10.0);
        assert!(sew(&flat).unwrap_err().to_string().contains("not on one plane"));
    }
}
