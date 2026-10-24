//! Sewing a solid from face descriptions: each face is its boundary loops (lines, circular
//! arcs and Bézier chains, counter-clockwise about the outward normal; holes clockwise)
//! and its surface (planar, revolved from a line, extruded from an arc, or ruled between two
//! Bézier chains). Vertices and edges at the same places are shared, so the faces close into
//! one shell without booleans — for solids whose faces are known exactly, such as folded
//! sheet metal.

use std::collections::HashMap;
use std::sync::Arc;

use mt::builder;
use solidcraft_geom::{Ring, Vec3};
use truck_modeling as mt;

use crate::body::{Body, Solid, p3, v3};
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

    /// A shared point and oriented tangent for comparing faces at a contact edge.
    /// Reversing an arc keeps the point and reverses the tangent, including semicircles.
    fn contact_frame(&self) -> Option<(Vec3, Vec3)> {
        match self {
            EdgeSpec::Line { a, b } => Some((*a, (*b - *a).normalized()?)),
            EdgeSpec::Arc { a, b, mid } => {
                let centre = circumcentre(*a, *mid, *b)?;
                let normal = (*mid - *a).cross(*b - *a).normalized()?;
                Some((*mid, normal.cross(*mid - centre).normalized()?))
            }
            EdgeSpec::Chain { .. } => None,
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

#[derive(Default)]
struct Cache {
    /// Vertices by point and fan (see [`corner_fans`]).
    verts: HashMap<(Key, usize), mt::Vertex>,
    edges: HashMap<(Key, usize, Key, usize, Option<Key>, usize), mt::Edge>,
}

impl Cache {
    fn vertex(&mut self, p: Vec3, fan: usize) -> mt::Vertex {
        self.verts.entry((key(p), fan)).or_insert_with(|| builder::vertex(p3(p))).clone()
    }
    /// The edge `e` on boundary sheet `sheet`, between the vertices of fans `fans` at its ends.
    fn edge(&mut self, e: &EdgeSpec, sheet: usize, (fa, fb): (usize, usize)) -> Result<mt::Edge> {
        let (a, b) = e.ends();
        let mid = e.mid_key();
        let (ka, kb) = (key(a), key(b));
        if let Some(found) = self.edges.get(&(ka, fa, kb, fb, mid, sheet)) {
            return Ok(found.clone());
        }
        if let Some(found) = self.edges.get(&(kb, fb, ka, fa, mid, sheet)) {
            return Ok(found.inverse());
        }
        let (va, vb) = (self.vertex(a, fa), self.vertex(b, fb));
        let made = match e {
            EdgeSpec::Line { .. } => builder::line(&va, &vb),
            EdgeSpec::Arc { mid, .. } => arc_edge(&va, &vb, *mid),
            EdgeSpec::Chain { .. } => {
                let curve = self.chain(e)?;
                mt::Edge::try_new(&va, &vb, mt::Curve::BSplineCurve(curve)).map_err(|e| KernelError::Failed(format!("sew: chain edge: {e}")))?
            }
        };
        self.edges.insert((ka, fa, kb, fb, mid, sheet), made.clone());
        Ok(made)
    }

    /// A Bézier chain as a B-spline of its degree (inner knots as many times as the degree)
    /// ending on the shared vertices.
    fn chain(&mut self, e: &EdgeSpec) -> Result<mt::BSplineCurve<mt::Point3>> {
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
        let mut pts: Vec<mt::Point3> = ctrl.iter().map(|p| p3(*p)).collect();
        let (a, b) = e.ends();
        if let Some(first) = pts.first_mut() {
            *first = self.vertex(a, 0).point();
        }
        if let Some(last) = pts.last_mut() {
            *last = self.vertex(b, 0).point();
        }
        mt::BSplineCurve::try_new(mt::KnotVec::from(knots), pts).map_err(|e| KernelError::Failed(format!("sew: chain curve: {e}")))
    }
}

/// Sew the faces into a closed solid.
pub fn sew(faces: &[FaceSpec]) -> Result<Body> {
    sew_impl(faces, false, false, None).map(|(body, _)| body)
}

/// Sew the faces into a closed solid that may touch itself: where four faces meet along a
/// straight edge (a cut-out's corner on the outline of a plate), each two of them get an edge of
/// their own (see [`touching_sheets`]), and where faces meet at a point in fans joined by no edge
/// (a hole's corner on its face's outline), each fan gets a vertex of its own.
pub fn sew_touching(faces: &[FaceSpec]) -> Result<Body> {
    sew_impl(faces, true, false, None).map(|(body, _)| body)
}

/// Separate boundary sheets at a shared geometric edge without moving it.
/// Optional transient sources follow only unchanged produced face identities.
/// Also reports the face each face of the body was built from (by its place in `faces`; `None`
/// when that could not be followed).
/// Union construction opts into circular contacts to distinguish edge-only joins from
/// numerical failures. Other operations retain their existing boundary-sheet construction.
pub(crate) fn sew_touching_with_sources(
    faces: &[FaceSpec],
    sources: &[Option<crate::FaceSource>],
    circular_contacts: bool,
) -> Result<(Body, Option<Vec<usize>>)> {
    if sources.len() != faces.len() {
        return Err(KernelError::Invalid("sew source alignment".into()));
    }
    sew_impl(faces, true, circular_contacts, Some(sources))
}

type EdgeUse = (usize, usize, usize);

/// Below this angle (radians) two faces leave an edge the same way: tangent faces, ordered by
/// their curvature.
const SAME_WAY: f64 = 1e-6;

/// Where more than two faces meet along a line or circular arc (bodies touching along an edge, or a wall
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
fn touching_sheets(faces: &[FaceSpec], circular_contacts: bool) -> Result<(HashMap<EdgeUse, usize>, HashMap<Corner, usize>)> {
    let mut uses: HashMap<(Key, Key), Vec<(EdgeUse, Vec3, Vec3)>> = HashMap::new();
    let mut arcs: HashMap<(Key, Key, Key), Vec<(EdgeUse, Vec3, Vec3)>> = HashMap::new();
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
                } else if circular_contacts && let EdgeSpec::Arc { a, b, mid } = e {
                    let (ka, kb, km) = (key(*a), key(*b), key(*mid));
                    let k = if ka < kb { (ka, kb, km) } else { (kb, ka, km) };
                    arcs.entry(k).or_default().push(((fi, li, ei), *a, *b));
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
    for (ends, group) in arcs.iter().filter(|(_, g)| g.len() > 2) {
        let [material, empty] = edge_sheets(faces, group)?;
        contacts.push(((ends.0, ends.1), material, empty, false));
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
    let (Some((u0, a0, b0)), 4) = (group.first(), group.len()) else { return Err(fail("unsupported edge contact")) };
    let edge = faces.get(u0.0).and_then(|f| f.loops.get(u0.1)).and_then(|lp| lp.get(u0.2));
    let (_, along) = edge.and_then(EdgeSpec::contact_frame).ok_or_else(|| fail("a degenerate contact"))?;
    let t = if key(*a0) < key(*b0) { along } else { -along };
    // Each face: its use, where the use starts, the way into it, its turn, its material side, and
    // the angle of the way into it from the first face's.
    let mut rays = Vec::with_capacity(4);
    let mut reference = None;
    for (u, a, _) in group {
        let face = faces.get(u.0).ok_or_else(|| fail("edge contact"))?;
        let (into, turn, ccw) = leaving(face, *u, t).ok_or_else(|| fail("unsupported edge contact"))?;
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

/// How a face leaves the line or circular arc it bounds (its use `u`), seen along the edge's
/// direction `t`: the way into the face square to the edge, how fast it turns from that way (its
/// curvature, positive counter-clockwise about `t`; 0 for a plane or a ruled wall), and whether
/// its material is on its counter-clockwise side. None for other faces, and for an extruded wall
/// the edge is neither a ruling nor a circular section of.
fn leaving(face: &FaceSpec, u: EdgeUse, t: Vec3) -> Option<(Vec3, f64, bool)> {
    let edge = face.loops.get(u.1)?.get(u.2)?;
    let (at, along) = edge.contact_frame()?;
    // The outward normal along the edge, and the centre of curvature (with its curvature).
    let (outward, centre) = match &face.surface {
        SurfSpec::Plane => (plane_normal(face)?, None),
        SurfSpec::Extruded { a: sa, b: sb, mid, dir } => {
            let k = dir.normalized()?;
            let centre = circumcentre(*sa, *mid, *sb)?;
            let radial = |p: Vec3| {
                let r = p - centre;
                r - k * r.dot(k)
            };
            let r = radial(at);
            let (p, out) = face.probe?;
            let side = if radial(p).dot(out) < 0.0 { -1.0 } else { 1.0 };
            let curvature = match edge {
                EdgeSpec::Line { .. } if along.dot(k).abs() >= 1.0 - 1e-9 => Some((centre, 1.0 / r.len())),
                // Across a circular section the wall leaves along its straight extrusion
                // direction. Its normal section here has no curvature.
                EdgeSpec::Arc { a, b, mid } if (*mid - *a).cross(*b - *a).normalized()?.dot(k).abs() > 1.0 - 1e-9 => None,
                _ => return None,
            };
            (r.normalized()? * side, curvature)
        }
        // A ruled wall (a cut-out's wall across a bend, ruled between its traces on the bend's
        // two faces), standing on the edge: its normal there is square to the edge and to the way
        // the next edge leaves the edge's end into the wall, as a wall's top or bottom edge leaves
        // its upright one.
        SurfSpec::Ruled { .. } if matches!(edge, EdgeSpec::Line { .. }) => {
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
        Some((c, curvature)) if (c - at).dot(left) > 0.0 => curvature,
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
/// gets its own vertex, so that a face's loops share no vertex (truck's faces need that) and
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

fn faces_connected(faces: &[mt::Face]) -> bool {
    let mut by_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            by_edge.entry(e.id()).or_default().push(i);
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![0];
    while let Some(i) = todo.pop() {
        if seen.insert(i)
            && let Some(f) = faces.get(i)
        {
            for e in f.edge_iter() {
                if let Some(adjacent) = by_edge.get(&e.id()) {
                    todo.extend(adjacent.iter().copied().filter(|j| !seen.contains(j)));
                }
            }
        }
    }
    seen.len() == faces.len()
}

/// Keep disconnected closed boundaries as distinct shells, preserving orientation.
fn closed_components(faces: Vec<mt::Face>) -> Result<Vec<mt::Shell>> {
    const MAX_EDGE_USES: usize = 500_000;
    const MAX_COMPONENTS: usize = 1024;
    let fail = |s: &str| KernelError::Invalid(format!("sew: {s}"));
    let mut by_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    let mut edge_uses = 0usize;
    for (i, face) in faces.iter().enumerate() {
        for edge in face.edge_iter() {
            edge_uses = edge_uses.saturating_add(1);
            if edge_uses > MAX_EDGE_USES {
                return Err(fail("edge budget exceeded"));
            }
            let uses = by_edge.entry(edge.id()).or_default();
            if uses.len() >= 2 {
                return Err(fail("nonmanifold edge contact"));
            }
            uses.push(i);
        }
    }
    if by_edge.values().any(|uses| uses.len() != 2) {
        return Err(fail("the faces do not close"));
    }
    let mut seen = vec![false; faces.len()];
    let mut vertices = HashMap::new();
    let mut components = Vec::new();
    for first in 0..faces.len() {
        if seen.get(first).copied().unwrap_or(true) {
            continue;
        }
        if components.len() >= MAX_COMPONENTS {
            return Err(fail("component budget exceeded"));
        }
        let component = components.len();
        let mut todo = vec![first];
        if let Some(slot) = seen.get_mut(first) {
            *slot = true;
        }
        let mut indices = Vec::new();
        while let Some(i) = todo.pop() {
            let face = faces.get(i).ok_or_else(|| fail("missing component face"))?;
            indices.push(i);
            for vertex in face.vertex_iter() {
                if vertices.insert(vertex.id(), component).is_some_and(|previous| previous != component) {
                    return Err(fail("bodies touch only at a vertex"));
                }
            }
            for edge in face.edge_iter() {
                let adjacent = by_edge.get(&edge.id()).ok_or_else(|| fail("missing component edge"))?;
                for j in adjacent {
                    let slot = seen.get_mut(*j).ok_or_else(|| fail("missing adjacent face"))?;
                    if !*slot {
                        *slot = true;
                        todo.push(*j);
                    }
                }
            }
        }
        indices.sort_unstable();
        let component_faces =
            indices.into_iter().map(|i| faces.get(i).cloned().ok_or_else(|| fail("missing component face"))).collect::<Result<Vec<_>>>()?;
        components.push(component_faces.into());
    }
    Ok(components)
}

/// A contact's edge sheets can terminate in separate vertex fans. Give each fan
/// its own vertex at the same coordinates so the boundary remains a manifold.
fn split_vertex_fans(faces: Vec<mt::Face>) -> Result<Vec<mt::Face>> {
    let mut by_vertex: HashMap<mt::VertexID, Vec<usize>> = HashMap::new();
    let mut by_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    let mut vertices = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for v in f.vertex_iter() {
            vertices.insert(v.id(), v.clone());
            by_vertex.entry(v.id()).or_default().push(i);
        }
        for e in f.edge_iter() {
            by_edge.entry(e.id()).or_default().push(i);
        }
    }
    let mut replacements: HashMap<(mt::VertexID, usize), mt::Vertex> = HashMap::new();
    for (id, members) in &mut by_vertex {
        members.sort_unstable();
        members.dedup();
        let mut seen = std::collections::HashSet::new();
        let mut fans = Vec::new();
        for start in members.iter().copied() {
            if seen.contains(&start) {
                continue;
            }
            let mut todo = vec![start];
            let mut fan = Vec::new();
            while let Some(i) = todo.pop() {
                if !seen.insert(i) {
                    continue;
                }
                fan.push(i);
                if let Some(f) = faces.get(i) {
                    for e in f.edge_iter().filter(|e| e.absolute_front().id() == *id || e.absolute_back().id() == *id) {
                        if let Some(adjacent) = by_edge.get(&e.id()) {
                            todo.extend(adjacent.iter().copied().filter(|j| !seen.contains(j)));
                        }
                    }
                }
            }
            fans.push(fan);
        }
        if fans.len() > 1 {
            let original = vertices.get(id).ok_or_else(|| KernelError::Failed("sew: missing contact vertex".into()))?;
            for fan in fans {
                let vertex = builder::vertex(original.point());
                for i in fan {
                    replacements.insert((*id, i), vertex.clone());
                }
            }
        }
    }
    let mut edges = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            edges.entry(e.id()).or_insert_with(|| {
                let (a, b) = (e.absolute_front(), e.absolute_back());
                let a = replacements.get(&(a.id(), i)).unwrap_or(a);
                let b = replacements.get(&(b.id(), i)).unwrap_or(b);
                mt::Edge::new(a, b, e.curve())
            });
        }
    }
    faces
        .into_iter()
        .map(|f| {
            let wires: Vec<mt::Wire> = f
                .absolute_boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .map(|e| {
                            let rebuilt = edges.get(&e.id()).ok_or_else(|| KernelError::Failed("sew: missing contact edge".into()))?;
                            Ok(if e.front() == e.absolute_front() { rebuilt.clone() } else { rebuilt.inverse() })
                        })
                        .collect::<Result<Vec<_>>>()
                        .map(Into::into)
                })
                .collect::<Result<_>>()?;
            let mut rebuilt = mt::Face::try_new(wires, f.surface()).map_err(|e| KernelError::Failed(format!("sew: contact face: {e}")))?;
            if !f.orientation() {
                rebuilt.invert();
            }
            Ok(rebuilt)
        })
        .collect()
}

fn sew_impl(
    faces: &[FaceSpec],
    touching: bool,
    circular_contacts: bool,
    sources: Option<&[Option<crate::FaceSource>]>,
) -> Result<(Body, Option<Vec<usize>>)> {
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
    for f in faces {
        for l in &f.loops {
            for e in l {
                let (a, b) = e.ends();
                // Ends that weld to one vertex would make truck panic on a zero-length edge (#268).
                if !e.valid() || !a.is_finite() || !b.is_finite() || a.dist(b) < 1e-9 || key(a) == key(b) {
                    return Err(KernelError::Invalid("sew: a degenerate edge".into()));
                }
            }
        }
    }
    guard("sew", || {
        // Separate fans at a point only where contact is expected (as for edge sheets); a plain
        // sew refuses bodies touching at a vertex.
        let (sheets, fans) = if touching { touching_sheets(faces, circular_contacts)? } else { (HashMap::new(), HashMap::new()) };
        let fan = |c: Corner| fans.get(&c).copied().unwrap_or(0);
        let mut cache = Cache::default();
        let mut out: Vec<mt::Face> = Vec::with_capacity(faces.len());
        for (fi, f) in faces.iter().enumerate() {
            let wires: Vec<mt::Wire> = f
                .loops
                .iter()
                .enumerate()
                .map(|(li, l)| {
                    let n = l.len().max(1);
                    l.iter()
                        .enumerate()
                        .map(|(ei, e)| {
                            cache.edge(e, sheets.get(&(fi, li, ei)).copied().unwrap_or(0), (fan((fi, li, ei)), fan((fi, li, (ei + 1) % n))))
                        })
                        .collect::<Result<Vec<_>>>()
                        .map(Into::into)
                })
                .collect::<Result<_>>()?;
            let face = match &f.surface {
                SurfSpec::Plane => match builder::try_attach_plane(&wires) {
                    Ok(face) => face,
                    // truck's fit loses thin faces away from the origin: see `plane_about_the_mean`.
                    Err(e) => {
                        let plane = plane_about_the_mean(&wires).ok_or_else(|| KernelError::Failed(format!("sew: face {fi}: {e}")))?;
                        mt::Face::try_new(wires, mt::Surface::Plane(plane)).map_err(|e| KernelError::Failed(format!("sew: face {fi}: {e}")))?
                    }
                },
                SurfSpec::Revolved { a, b, axis_origin, axis, angle } => {
                    let k = axis.normalized().ok_or_else(|| KernelError::Invalid("sew: axis".into()))?;
                    let (va, vb) = (builder::vertex(p3(*a)), builder::vertex(p3(*b)));
                    let swept: mt::Shell = builder::rsweep(&builder::line(&va, &vb), p3(*axis_origin), v3(k), mt::Rad(*angle));
                    let tmp = swept.face_iter().next().cloned().ok_or_else(|| KernelError::Failed("sew: revolved surface".into()))?;
                    curved(tmp.oriented_surface(), wires, f.probe, fi)?
                }
                SurfSpec::Extruded { a, b, mid, dir } => {
                    let (va, vb) = (builder::vertex(p3(*a)), builder::vertex(p3(*b)));
                    let tmp: mt::Face = builder::tsweep(&arc_edge(&va, &vb, *mid), v3(*dir));
                    curved(tmp.oriented_surface(), wires, f.probe, fi)?
                }
                SurfSpec::Ruled { a, b } => {
                    let (ca, cb) = (cache.chain(a)?, cache.chain(b)?);
                    if ca.degree() != cb.degree() || ca.control_points().len() != cb.control_points().len() {
                        return Err(KernelError::Invalid("sew: ruled chains differ in pieces".into()));
                    }
                    let surface = mt::Surface::BSplineSurface(mt::BSplineSurface::homotopy(ca, cb));
                    curved(surface, wires, f.probe, fi)?
                }
            };
            out.push(face);
        }
        let source_ids: HashMap<_, _> = out
            .iter()
            .enumerate()
            .filter_map(|(i, f)| sources.and_then(|s| s.get(i)).and_then(Option::as_ref).map(|s| (f.id(), (f.orientation(), s))))
            .collect();
        if !sheets.is_empty() && !faces_connected(&out) {
            if sheets.keys().any(|u| matches!(faces.get(u.0).and_then(|f| f.loops.get(u.1)).and_then(|lp| lp.get(u.2)), Some(EdgeSpec::Arc { .. }))) {
                return Err(KernelError::CircularEdgeContact);
            }
            return Err(KernelError::Invalid("sew: bodies touch only along an edge".into()));
        }
        if !sheets.is_empty() {
            out = split_vertex_fans(out)?;
        }
        // Faces keep their identity into the solid (grouped into shells) and the body.
        let built: HashMap<mt::FaceID, usize> = out.iter().enumerate().map(|(i, f)| (f.id(), i)).collect();
        let shells = closed_components(out)?;
        let solid = Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("sew: the faces do not close: {e}")))?;
        let from = solid.face_iter().map(|f| built.get(&f.id()).copied()).collect();
        let body = Body::new(solid)?;
        let mapped =
            body.solid.face_iter().map(|f| source_ids.get(&f.id()).filter(|(o, _)| *o == f.orientation()).map(|(_, s)| (*s).clone())).collect();
        Ok((body.with_face_sources(sources.map(|_| std::sync::Arc::new(mapped))), from))
    })
}

/// Below this sagitta-to-half-chord ratio an arc is built as its exact rational quadratic from
/// its ends and midpoint. truck's three-point arc (circumcentre, then a turn about it) misses
/// its own ends on very flat arcs: one 12 mm long with a 0.08 µm sagitta (a 225 m radius) ends
/// about 1e-6 mm off its vertex, and a planar face holding it far from the origin could not be
/// given a plane (its wire's points no longer closed).
const FLAT_ARC: f64 = 1e-2;

/// The arc from `a` to `b` through `mid`: the exact rational quadratic for a flat arc, truck's
/// three-point arc otherwise (and should the curve be refused, which its exact ends make
/// impossible).
fn arc_edge(a: &mt::Vertex, b: &mt::Vertex, mid: Vec3) -> mt::Edge {
    let (pa, pb) = (a.point(), b.point());
    let (pa, pb) = (Vec3::new(pa.x, pa.y, pa.z), Vec3::new(pb.x, pb.y, pb.z));
    flat_arc(pa, pb, mid)
        .and_then(|curve| mt::Edge::try_new(a, b, mt::Curve::NurbsCurve(curve)).ok())
        .unwrap_or_else(|| builder::circle_arc(a, b, p3(mid)))
}

/// A flat arc (see [`FLAT_ARC`]) through its midpoint `mid` as one rational quadratic: the middle
/// control point on the line from the chord's midpoint through `mid`, 2 / (1 − t²) as far, weight
/// (1 − t²) / (1 + t²), where t is the sagitta over the half chord. None for a deeper arc, or
/// when `mid` is not halfway along the chord.
fn flat_arc(a: Vec3, b: Vec3, mid: Vec3) -> Option<mt::NurbsCurve<mt::Vector4>> {
    let m = (a + b) * 0.5;
    let half = a.dist(b) / 2.0;
    let chord = (b - a).normalized()?;
    let bow = mid - m;
    let t = bow.len() / half;
    if !(half > 1e-9) || !(t < FLAT_ARC) || bow.dot(chord).abs() > 1e-9 * half.max(1.0) {
        return None;
    }
    let w = (1.0 - t * t) / (1.0 + t * t);
    let apex = m + bow * (2.0 / (1.0 - t * t));
    let h = |p: Vec3, w: f64| mt::Vector4::new(p.x * w, p.y * w, p.z * w, w);
    Some(mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), vec![h(a, 1.0), h(apex, w), h(b, 1.0)])))
}

/// A curved face on `surf`, turned so its normal at the probe points outward.
fn curved(mut surf: mt::Surface, wires: Vec<mt::Wire>, probe: Option<(Vec3, Vec3)>, fi: usize) -> Result<mt::Face> {
    use mt::{ParametricSurface3D, SearchParameter};
    if let Some((p, outward)) = probe
        && let Some((u, v)) = surf.search_parameter(p3(p), None, 100)
    {
        let n = surf.normal(u, v);
        if Vec3::new(n.x, n.y, n.z).dot(outward) < 0.0 {
            surf = mt::Invertible::inverse(&surf);
        }
    }
    mt::Face::try_new(wires, surf).map_err(|e| KernelError::Failed(format!("sew: face {fi}: {e}")))
}

/// The plane of a boundary that truck's fit (`builder::try_attach_plane`) refused: truck's fit
/// again, on the boundary moved so that the mean of its control points is the origin, and the
/// plane moved back. None when truck refuses that one too (the boundary is not flat within
/// truck's 1e-6, or has no area).
///
/// truck sums the cross products of consecutive control points about a centre, and takes the
/// points' sum for that centre (it divides by the number of wires, not of points): eight times as
/// far from the origin as a rectangle of four lines. The sum is the same about any centre; its
/// rounding is not. It grows with the square of the centre's distance, and on a thin face, whose
/// normal is short, turns the normal far enough for truck's own flatness test to fail: truck
/// finds a 0.00013 x 30 mm face 360 mm from the origin 2e-6 mm out of the plane it fitted. With
/// the centre at the origin the same face is 1e-14 mm out at most.
fn plane_about_the_mean(wires: &[mt::Wire]) -> Option<mt::Plane> {
    let (mut sum, mut count) = (Vec3::ZERO, 0usize);
    for p in wires.iter().flat_map(|w| w.edge_iter()).flat_map(|e| e.oriented_curve().lift_up().control_points().clone()) {
        sum += Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w);
        count += 1;
    }
    let mean = sum * (1.0 / count.max(1) as f64);
    if !mean.is_finite() {
        return None;
    }
    let moved: Vec<mt::Wire> = wires.iter().map(|w| builder::transformed(w, mt::Matrix4::from_translation(v3(-mean)))).collect();
    match builder::try_attach_plane(&moved).ok()?.surface() {
        mt::Surface::Plane(plane) => Some(mt::Transformed::transformed(&plane, mt::Matrix4::from_translation(v3(mean)))),
        _ => None,
    }
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
            let mesh = body.tessellate(0.01).unwrap();
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
        assert!(!cavity.tessellate(0.01).unwrap().contains(Vec3::new(1.0, 1.5, 2.0)));
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
        // axes. truck's plane fit sums the cross products of a boundary's points about a centre
        // and takes the points' sum for it, eight times as far from the origin as a rectangle;
        // on the thin faces its normal then comes out so far off that truck finds a
        // 0.00013 x 30 mm face at (-300, -200, 5) 2e-6 mm out of its own plane and refuses it:
        // `cannot attach a plane to a wire that is not on one plane`. Nine of these 45 slabs
        // have such a face. (Thinner slabs meet another limit, at the origin too: a face
        // 0.00003 mm wide and askew to the axes is sewn but gets no triangles.)
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
