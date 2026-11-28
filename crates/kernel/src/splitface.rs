//! Split Face and Split Body: divide chosen faces of a body along where a tool meets them (a
//! plane, another face's surface, or sketch curves swept along a direction), leaving the solid
//! unchanged, or cut a body in two along the tool. Edges the split line ends on are cut in every
//! face that uses them.
//!
//! A tool is a set of exact surface patches: schema 4 record surfaces, each over a parameter box,
//! unbounded where the tool is (a plane, a face's surface extended). A plane is one patch, so is a
//! face's surface; a sketch curve is one patch per segment swept along the direction (a line a
//! plane strip, an arc swept square to its sketch a cylinder strip, any other segment its exact
//! rational Bézier extruded), an open curve's ends continued by lines past the body. SMLib cuts
//! the faces where the patches meet them ([`solidcraft_smlib::Brep::split_faces`]), its own
//! intersection curves trimmed to each face, and slices a body with one patch
//! ([`solidcraft_smlib::Brep::split_body`]); the history is SMLib's own.
//!
//! Which side of a tool a point is on is its offset along the normal of the nearest patch
//! ([`ToolSide`]): each patch's surface is prepared once (its projection grid kept), never per
//! point. Every patch's normal points to the tool's positive side: a plane's normal, a face's
//! outward side, the left of a sketch curve (inside a closed one, counter-clockwise).

use solidcraft_geom::prelude::*;
use std::f64::consts::TAU;

use solidcore_record::brep::{self, Frame, Surface};
use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_geom::Aabb3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::tess::uv::Starts;

use crate::body::Body;
use crate::history::{EdgeOrigin, FaceOrigin, History};
use crate::{KernelError, Result, guard, record};

/// What splits the faces: a plane, another body's face, or sketch curves swept
/// ([`solidcraft_kernel_api::SplitTool`] over solidcore's body).
pub type SplitTool<'a> = solidcraft_kernel_api::SplitTool<'a, Body>;

/// Most segments a sketch tool may hold.
pub(crate) const MAX_SEGMENTS: usize = 4096;

fn invalid(what: impl Into<String>) -> KernelError {
    KernelError::Invalid(what.into())
}

fn failed(what: &str, e: impl crate::Cause) -> KernelError {
    KernelError::smlib(format!("SMLib {what}"), e)
}

fn a3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// One piece of a tool: a record surface over `domain` (`±∞` where the tool runs on without
/// end), `flip` when the tool's positive side is against the surface's normal.
#[derive(Clone, Debug)]
struct Patch {
    surface: Surface,
    domain: [[f64; 2]; 2],
    flip: bool,
}

const ALL: [f64; 2] = [f64::NEG_INFINITY, f64::INFINITY];

impl Patch {
    fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Frame {
        Frame { origin: a3(origin), x_axis: a3(x), y_axis: a3(y) }
    }

    /// The patch with every unbounded range closed where it has passed all of `reach`: the
    /// span of the box's corners in that parameter, which the surface is straight along (a
    /// plane's two, a cylinder's or a cone's height).
    fn bounded(&self, reach: &Aabb3) -> Result<Patch> {
        let corners: Vec<Vec3> = (0..8)
            .map(|k| {
                let pick = |bit: usize, lo: f64, hi: f64| if k & bit == 0 { lo } else { hi };
                Vec3::new(pick(1, reach.min.x, reach.max.x), pick(2, reach.min.y, reach.max.y), pick(4, reach.min.z, reach.max.z))
            })
            .collect();
        // The parameters of a corner along the surface's straight directions.
        let params = |c: Vec3| -> Option<[f64; 2]> {
            match &self.surface {
                Surface::Plane { origin, u_axis, v_axis } => {
                    let (du, dv, r) = (v3(*u_axis), v3(*v_axis), c - v3(*origin));
                    let (a, b, d) = (du.dot(du), du.dot(dv), dv.dot(dv));
                    let det = a * d - b * b;
                    (det.abs() > 0.0).then(|| [(d * du.dot(r) - b * dv.dot(r)) / det, (a * dv.dot(r) - b * du.dot(r)) / det])
                }
                Surface::Cylinder { frame, .. } | Surface::Cone { frame, .. } => {
                    let z = v3(frame.x_axis).cross(v3(frame.y_axis));
                    Some([0.0, (c - v3(frame.origin)).dot(z)])
                }
                _ => None,
            }
        };
        let mut domain = self.domain;
        for (k, range) in domain.iter_mut().enumerate() {
            if range.iter().all(|x| x.is_finite()) {
                continue;
            }
            let along: Option<Vec<f64>> = corners.iter().map(|c| params(*c).map(|p| p[k])).collect();
            let along = along.ok_or_else(|| invalid("not supported yet: a split tool unbounded on this surface"))?;
            let (lo, hi) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
            let pad = (hi - lo) * 0.05 + 1.0;
            *range = [range[0].max(lo - pad), range[1].min(hi + pad)];
            if !(range[0] < range[1]) {
                // The tool ends before the body: an empty patch, which cuts nothing.
                *range = [range[0], range[0]];
            }
        }
        Ok(Patch { domain, ..self.clone() })
    }
}

/// A box past the body (by its own size), which every unbounded patch is cut to.
fn reach_of(body: &Body) -> Aabb3 {
    let b = body.outline_bounds();
    let pad = Vec3::new(1.0, 1.0, 1.0) * (b.diagonal() + 1.0);
    Aabb3 { min: b.min - pad, max: b.max + pad }
}

/// The tool as patches, one group for each field (a sketch tool: one for each curve).
fn cutters(tool: &SplitTool, reach: &Aabb3) -> Result<Vec<Vec<Patch>>> {
    match tool {
        SplitTool::Plane(p) => {
            let (x, y) = (p.x.normalized(), p.normal().cross(p.x).normalized());
            let (Some(x), Some(y)) = (x, y) else { return Err(invalid("a split plane with no normal")) };
            let surface = Surface::Plane { origin: a3(p.origin), u_axis: a3(x), v_axis: a3(y) };
            Ok(vec![vec![Patch { surface, domain: [ALL, ALL], flip: false }]])
        }
        SplitTool::Face { body, face } => Ok(vec![vec![face_patch(body, *face)?]]),
        SplitTool::Curves { plane, curves, along } => {
            let sweep = sweep_of(plane, *along)?;
            if curves.iter().map(Vec::len).sum::<usize>() > MAX_SEGMENTS {
                return Err(invalid("too many split curves"));
            }
            curves.iter().map(|c| curve_patches(plane, c, sweep, reach)).collect()
        }
    }
}

/// The surface of face `face` of `body`, over its surface's whole parameter box (unbounded
/// where the surface is), facing out of the body.
fn face_patch(body: &Body, face: usize) -> Result<Patch> {
    body.require_brep("a split tool")?;
    let record = body.record();
    let f = record.faces.get(face).ok_or_else(|| invalid(format!("the tool body has no face {face}")))?;
    let forward = body.face_forward(face);
    // The surface's own parameter box, unbounded where it is: of `surface` itself, not of its
    // NURBS form (SMLib's cylinder is held on one, parameterised otherwise). A label (a round's
    // definition, a ball patch) has no (u, v) of its own: its face's NURBS form is the surface,
    // over its knot box, as a round's face was before it kept its definition.
    let surface = match &f.surface {
        Surface::Blend { .. } | Surface::Sweep { .. } | Surface::BallPatch { .. } => f.parameters(),
        s => s,
    };
    let dom = SurfaceEval::new(surface).map_err(|e| invalid(format!("the split tool's surface: {e}")))?.domain();
    let range = |k: usize| dom.range[k].map_or(ALL, |(a, b)| [a, b]);
    Ok(Patch { surface: surface.clone(), domain: [range(0), range(1)], flip: !forward })
}

/// The unit sweep direction of a sketch tool, on the sketch normal's side (`None`: along the
/// normal).
fn sweep_of(plane: &Plane, along: Option<Vec3>) -> Result<Vec3> {
    let n = plane.normal().normalized().ok_or_else(|| invalid("a sketch plane with no normal"))?;
    let Some(d) = along else { return Ok(n) };
    let d = d.normalized().filter(|d| d.is_finite()).ok_or_else(|| invalid("the projection direction must be a nonzero vector"))?;
    if d.dot(n).abs() < 1e-6 {
        return Err(invalid("the projection direction runs along the sketch plane"));
    }
    Ok(if d.dot(n) < 0.0 { -d } else { d })
}

/// One sketch curve swept along `sweep`: a patch for each segment, unbounded along the sweep,
/// and an open curve's ends continued along their tangents past `reach`. Each patch's normal
/// points to the curve's left.
fn curve_patches(plane: &Plane, segs: &[Seg2], sweep: Vec3, reach: &Aabb3) -> Result<Vec<Patch>> {
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(invalid("an empty split curve")) };
    let ok = |p: Vec2| p.x.is_finite() && p.y.is_finite();
    if !segs.iter().all(|s| ok(s.start()) && ok(s.end())) {
        return Err(invalid("split curves need finite segments"));
    }
    let closed = chain_closed(segs);
    let far = reach.diagonal() + (plane.origin - (reach.min + reach.max) * 0.5).len();
    let mut out = Vec::new();
    let line = |a: Vec2, b: Vec2, out: &mut Vec<Patch>| -> Result<()> {
        let (wa, wb) = (plane.to_world(a), plane.to_world(b));
        let d = wb - wa;
        let len = d.len();
        let d = d.normalized().ok_or_else(|| invalid("a zero-length split line"))?;
        // u along the sweep, v along the line: the normal sweep × line is the line's left.
        let surface = Surface::Plane { origin: a3(wa), u_axis: a3(sweep), v_axis: a3(d) };
        out.push(Patch { surface, domain: [ALL, [0.0, len]], flip: false });
        Ok(())
    };
    // An open curve runs on along its end tangents: an end line lengthened, else a line added.
    let mut segs = segs.to_vec();
    let (mut lead, mut tail) = (None, None);
    if !closed {
        let t0 = first.start_tangent().normalized().ok_or_else(|| invalid("a split curve with no tangent"))?;
        let t1 = last.end_tangent().normalized().ok_or_else(|| invalid("a split curve with no tangent"))?;
        let (s0, s1) = (first.start(), last.end());
        match segs.first_mut() {
            Some(Seg2::Line { a, .. }) => *a = s0 - t0 * far,
            _ => lead = Some((s0 - t0 * far, s0)),
        }
        match segs.last_mut() {
            Some(Seg2::Line { b, .. }) => *b = s1 + t1 * far,
            _ => tail = Some((s1, s1 + t1 * far)),
        }
    }
    if let Some((a, b)) = lead {
        line(a, b, &mut out)?;
    }
    let square = sweep.cross(plane.normal()).len() <= 1e-12;
    for s in &segs {
        match *s {
            Seg2::Line { a, b } => line(a, b, &mut out)?,
            Seg2::Arc { center, radius, start, sweep: turn } if square && radius.is_finite() && radius > 0.0 && turn.abs() <= TAU => {
                // Angles about the sketch normal from the sketch's x; a counter-clockwise arc has
                // its left inside, against the cylinder's outward normal.
                let n = plane.normal().normalized().ok_or_else(|| invalid("a sketch plane with no normal"))?;
                let surface = Surface::Cylinder { frame: Patch::frame(plane.to_world(center), plane.x, n.cross(plane.x)), radius };
                let (lo, hi) = if turn >= 0.0 { (start, start + turn) } else { (start + turn, start) };
                out.push(Patch { surface, domain: [[lo, hi], ALL], flip: turn > 0.0 });
            }
            ref s => {
                for (pts, w) in beziers(s)? {
                    out.push(extruded(plane, &pts, &w, sweep, reach)?);
                }
            }
        }
    }
    if let Some((a, b)) = tail {
        line(a, b, &mut out)?;
    }
    Ok(out)
}

/// A segment as rational Bézier pieces (control points and weights), exactly: an arc in pieces
/// of a quarter turn at most.
fn beziers(s: &Seg2) -> Result<Vec<(Vec<Vec2>, Vec<f64>)>> {
    Ok(match *s {
        Seg2::Line { a, b } => vec![(vec![a, b], vec![1.0; 2])],
        Seg2::Arc { center, radius, start, sweep } => {
            if !(radius.is_finite() && radius > 0.0 && sweep.is_finite() && sweep.abs() <= TAU && sweep != 0.0) {
                return Err(invalid("a split arc needs a positive radius and a turn of at most one revolution"));
            }
            let n = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            let h = sweep / (2 * n) as f64;
            let at = |t: f64| center + Vec2::new(t.cos(), t.sin()) * radius;
            (0..n)
                .map(|i| {
                    let t0 = start + 2.0 * h * i as f64;
                    let apex = center + Vec2::new((t0 + h).cos(), (t0 + h).sin()) * (radius / h.cos());
                    (vec![at(t0), apex, at(t0 + 2.0 * h)], vec![1.0, h.cos(), 1.0])
                })
                .collect()
        }
        Seg2::Cubic { p0, p1, p2, p3 } => vec![(vec![p0, p1, p2, p3], vec![1.0; 4])],
        Seg2::Conic { a, apex, b, w } => vec![(vec![a, apex, b], vec![1.0, w, 1.0])],
        Seg2::Bezier { n, p } => {
            let pts = p.get(..=usize::from(n)).ok_or_else(|| invalid("Bézier degree"))?.to_vec();
            let w = vec![1.0; pts.len()];
            vec![(pts, w)]
        }
    })
}

/// A rational Bézier piece in `plane` swept along `sweep` across `reach`, exactly: a spline of
/// degree one along the sweep (u) and the piece's along the curve (v), so its normal is the
/// curve's left.
fn extruded(plane: &Plane, pts: &[Vec2], w: &[f64], sweep: Vec3, reach: &Aabb3) -> Result<Patch> {
    let n = plane.normal().normalized().ok_or_else(|| invalid("a sketch plane with no normal"))?;
    // The sweep heights that take the sketch plane through the whole box.
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for k in 0..8 {
        let pick = |bit: usize, a: f64, b: f64| if k & bit == 0 { a } else { b };
        let c = Vec3::new(pick(1, reach.min.x, reach.max.x), pick(2, reach.min.y, reach.max.y), pick(4, reach.min.z, reach.max.z));
        let h = (c - plane.origin).dot(n) / sweep.dot(n);
        (lo, hi) = (lo.min(h), hi.max(h));
    }
    let degree = pts.len().saturating_sub(1);
    let knots = |d: usize| std::iter::repeat_n(0.0, d + 1).chain(std::iter::repeat_n(1.0, d + 1)).collect::<Vec<f64>>();
    let homogeneous: Vec<Vec<[f64; 4]>> = [lo, hi]
        .iter()
        .map(|h| {
            pts.iter()
                .zip(w)
                .map(|(p, wt)| {
                    let q = plane.to_world(*p) + sweep * *h;
                    [q.x * wt, q.y * wt, q.z * wt, *wt]
                })
                .collect()
        })
        .collect();
    let surface = Surface::RationalSpline { u_knots: knots(1), v_knots: knots(degree), homogeneous };
    Ok(Patch { surface, domain: [[0.0, 1.0], [0.0, 1.0]], flip: false })
}

/// One patch ready to measure points against: its surface prepared once.
struct Measured {
    eval: SurfaceEval,
    starts: Starts,
    domain: [[f64; 2]; 2],
    sign: f64,
}

impl Measured {
    fn new(p: &Patch) -> Result<Measured> {
        let eval = SurfaceEval::new(&p.surface).map_err(|e| invalid(format!("the split tool's surface: {e}")))?;
        Ok(Measured { eval, starts: Starts::default(), domain: p.domain, sign: if p.flip { -1.0 } else { 1.0 } })
    }

    /// The nearest point of the patch to `p` (its parameters kept in the patch's box, a
    /// periodic one taken round to it), and `p`'s offset along the patch's side there.
    fn near(&self, p: Vec3) -> Option<(f64, f64)> {
        let uv = self.eval.project_with(&self.starts, p, None, f64::INFINITY)?;
        let period = self.eval.domain().period;
        let mut q = [0.0; 2];
        for k in 0..2 {
            let [lo, hi] = self.domain[k];
            let mut x = uv[k];
            if let Some(t) = period[k] {
                x = lo + (x - lo).rem_euclid(t);
                // Past the end of a strip: whichever end is nearer round the turn.
                if x > hi && x - hi > lo + t - x {
                    x -= t;
                }
            }
            q[k] = x.clamp(lo.min(hi), hi.max(lo));
        }
        let (at, su, sv) = self.eval.derivs(q[0], q[1])?;
        let n = su.cross(sv).normalized()?;
        Some(((p - at).len(), (p - at).dot(n) * self.sign))
    }
}

/// Which side of a tool a point is on, the tool's surfaces prepared once for many points.
pub(crate) struct ToolSide(Vec<Measured>);

impl ToolSide {
    /// The side field of `tool` (one field: a sketch tool with one curve), its patches reaching
    /// past `near`.
    pub(crate) fn of(tool: &SplitTool, near: &Body) -> Result<ToolSide> {
        let mut groups = cutters(tool, &reach_of(near))?;
        match (groups.pop(), groups.is_empty()) {
            (Some(g), true) => Ok(ToolSide(g.iter().map(Measured::new).collect::<Result<_>>()?)),
            _ => Err(invalid("one tool at a time")),
        }
    }

    /// The side of `p`: its sign says which, zero on the tool (NaN where it can't be told): the
    /// offset from the nearest patch along its side.
    pub(crate) fn at(&self, p: Vec3) -> f64 {
        self.0.iter().filter_map(|m| m.near(p)).min_by(|a, b| a.0.total_cmp(&b.0)).map_or(f64::NAN, |(_, side)| side)
    }
}

/// Whether a sketch curve's chain of segments closes on itself (its end on its start).
fn chain_closed(segs: &[Seg2]) -> bool {
    match (segs.first(), segs.last()) {
        (Some(first), Some(last)) => (first.start() - last.end()).len() <= 1e-9 * (1.0 + first.start().len()),
        _ => false,
    }
}

/// What one field of a split tool is, which says what its two sides are called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A plane: the side its normal points to, or the other.
    Plane,
    /// Another body's face: outside that body (the face's outward side), or inside.
    Face,
    /// A sketch curve that closes on itself: inside it, or outside.
    Loop,
    /// An open sketch curve: on its left (looking down the sketch normal), or its right.
    Curve,
}

/// The pieces of a Split Face told apart by the tool: what each field of the tool is, and for
/// each face asked about, which side of each field it lies on.
#[derive(Clone, Debug, PartialEq)]
pub struct PieceSides {
    pub fields: Vec<FieldKind>,
    /// By face asked about, then by field: `Some(true)` on the field's first side (a plane's
    /// normal side, outside a tool face, inside a closed curve, left of an open one), `Some(false)`
    /// on the other, `None` where it can't be told.
    pub sides: Vec<Vec<Option<bool>>>,
}

/// Which side of each field of `tool` each of the faces `faces` of `split` (a body `tool` split
/// with [`split_faces`]) lies on: the tool's field at a point inside the face, on its surface and
/// inside its trims (SMLib's `CalculateAnInternalPoint`). Each piece of a split face lies wholly
/// on one side of every field, so one point says which; a face that lies on a field says nothing.
pub fn split_piece_sides(split: &Body, faces: &[usize], tool: &SplitTool) -> Result<PieceSides> {
    let fields: Vec<ToolSide> = cutters(tool, &reach_of(split))?
        .iter()
        .map(|g| g.iter().map(Measured::new).collect::<Result<Vec<_>>>().map(ToolSide))
        .collect::<Result<_>>()?;
    // Each patch's positive side is a sketch curve's left: inside a counter-clockwise loop,
    // outside a clockwise one.
    let kinds: Vec<(FieldKind, bool)> = match tool {
        SplitTool::Plane(_) => vec![(FieldKind::Plane, true)],
        SplitTool::Face { .. } => vec![(FieldKind::Face, true)],
        SplitTool::Curves { curves, .. } => curves
            .iter()
            .map(|c| if chain_closed(c) { (FieldKind::Loop, solidcore_record::Loop2 { segs: c.clone() }.is_ccw()) } else { (FieldKind::Curve, true) })
            .collect(),
    };
    if kinds.len() != fields.len() {
        return Err(invalid("a split tool's fields and curves differ"));
    }
    let points = inner_points(split)?;
    let tol = (split.size() * 1e-6).max(1e-9);
    let sides = faces
        .iter()
        .map(|f| {
            let p = points.get(*f).copied().flatten();
            fields
                .iter()
                .zip(&kinds)
                .map(|(field, (_, ahead))| {
                    let side = field.at(p?);
                    (side.is_finite() && side.abs() > tol).then_some((side > 0.0) == *ahead)
                })
                .collect()
        })
        .collect();
    Ok(PieceSides { fields: kinds.into_iter().map(|(k, _)| k).collect(), sides })
}

/// A point inside each face of `body` (in its face order), on its surface and inside its trims
/// (SMLib's `CalculateAnInternalPoint`); `None` for a face SMLib numbers outside the record.
pub(crate) fn inner_points(body: &Body) -> Result<Vec<Option<Vec3>>> {
    let (sm, order) = crate::native::smlib_body_ordered(body)?;
    let order = order.ok_or_else(|| KernelError::Failed("SMLib numbers the body's faces differently from its record".into()))?;
    let points = sm.face_points().map_err(|e| KernelError::smlib("SMLib: a point inside each face", e))?;
    let mut out = vec![None; body.face_count()];
    for (i, p) in points.iter().enumerate() {
        let face = if order.faces.is_empty() { i } else { order.faces.get(i).copied().unwrap_or(usize::MAX) };
        if let Some(slot) = out.get_mut(face) {
            *slot = Some(Vec3::new(p[0], p[1], p[2]));
        }
    }
    Ok(out)
}

/// How many fields a tool is (a sketch tool: one per curve).
pub(crate) fn field_count(tool: &SplitTool) -> Result<usize> {
    Ok(match tool {
        SplitTool::Curves { curves, .. } => curves.len(),
        _ => 1,
    })
}

/// The regions of a body cut by a tool: for each face (in face order), the region it lies in,
/// and how many there are. Faces are in one region when they meet at an edge the tool does not
/// run along; an edge along the tool (a split line) parts them.
pub(crate) fn regions(split: &Body, tool: &SplitTool) -> Result<(Vec<usize>, usize)> {
    let record = split.record();
    let fields: Vec<ToolSide> = cutters(tool, &reach_of(split))?
        .iter()
        .map(|g| g.iter().map(Measured::new).collect::<Result<Vec<_>>>().map(ToolSide))
        .collect::<Result<_>>()?;
    let tol = (split.size() * 2e-3).max(1e-4);
    let along = |e: &brep::Edge| -> bool {
        let Ok(c) = CurveEval::new(e.parameters()) else { return false };
        let [t0, t1] = e.interval;
        let at: Vec<Vec3> = [0.25, 0.5, 0.75].iter().filter_map(|s| c.point(t0 + (t1 - t0) * s)).collect();
        at.len() == 3 && fields.iter().any(|g| at.iter().all(|p| g.at(*p).abs() <= tol))
    };
    let n = record.faces.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while let Some(&p) = parent.get(i).filter(|p| **p != i) {
            i = p;
        }
        i
    }
    for e in &record.edges {
        if along(e) {
            continue;
        }
        let faces: Vec<usize> = e.uses.iter().map(|u| u.face as usize).collect();
        for pair in faces.windows(2) {
            let (a, b) = (root(&mut parent, pair[0]), root(&mut parent, pair[1]));
            if let Some(slot) = parent.get_mut(a.max(b)) {
                *slot = a.min(b);
            }
        }
    }
    let mut ids = std::collections::BTreeMap::new();
    let region: Vec<usize> = (0..n)
        .map(|i| {
            let r = root(&mut parent, i);
            let next = ids.len();
            *ids.entry(r).or_insert(next)
        })
        .collect();
    Ok((region, ids.len()))
}

/// SMLib's surfaces of the tool's patches, in the tool's order, each reaching past `reach`.
fn smlib_tools(groups: &[Vec<Patch>], reach: &Aabb3) -> Result<Vec<solidcraft_smlib::ssi::Surface>> {
    let mut out = Vec::new();
    for p in groups.iter().flatten() {
        let b = p.bounded(reach)?;
        if b.domain.iter().any(|r| !(r[0] < r[1])) {
            continue;
        }
        out.push(solidcraft_smlib::ssi::Surface::from_record(&b.surface, b.domain).map_err(|e| failed("split tool", e))?);
    }
    Ok(out)
}

/// A result checked by SMLib and brought back with its history over `[body]`.
pub(crate) fn brought_back(name: &str, body: &Body, ob: &solidcraft_smlib::history::Order, r: &solidcraft_smlib::Brep) -> Result<(Body, History)> {
    let check = r.check(solidcraft_smlib::CheckLevel::All).map_err(|e| failed(name, e))?;
    if !check.is_valid() {
        return Err(failed(name, format!("SMLib's check refuses the result: {check}")));
    }
    let (out, or) = crate::native::body_from_smlib_ordered(r)?;
    let or = or.ok_or_else(|| failed(name, "the result's record is numbered unlike SMLib's body"))?;
    let h = r.history().map_err(|e| failed(name, e))?.history(std::slice::from_ref(ob), &or).map_err(|e| failed(name, e))?;
    let out = out.with_color(body.color()).with_split_lines(body.split_lines().cloned());
    Ok((out, h))
}

/// `body` in SMLib, tagged as operand 0, with its record's order.
pub(crate) fn tagged(name: &str, body: &Body) -> Result<(solidcraft_smlib::Brep, solidcraft_smlib::history::Order)> {
    let (sb, ob) = crate::native::smlib_body_ordered(body)?;
    let ob = ob.ok_or_else(|| failed(name, "SMLib numbers the body's faces unlike its record"))?;
    Ok((sb.tagged(0).map_err(|e| failed(name, e))?, ob))
}

/// Split the given faces (indices in `body`'s face order) where the tool meets them, with the
/// body face each face of the result came from (in face order). The solid is unchanged. A listed
/// face the tool misses is an error naming it; a tool that runs into the faces and ends inside
/// them, cutting none through, is an error saying so. The result's history is over `[body]`: each
/// piece of a split face is Split from it, every other face Kept.
pub fn split_faces_with_map(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<(Body, Vec<usize>)> {
    let _span = solidcraft_profile::span("kernel", "split_faces_with_map");
    let (b, map, made) = split(body, faces, tool)?;
    if let Some((missed, _)) = faces.iter().zip(&made).find(|(_, m)| **m == 0) {
        return Err(invalid(format!("the tool does not cross face {missed}")));
    }
    Ok((b, map))
}

/// Split where the tool meets the listed faces (missing some is fine; missing all is an error,
/// and so is stopping inside them without cutting any through), with the body face each face of
/// the result came from.
pub(crate) fn split_any(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<(Body, Vec<usize>)> {
    split(body, faces, tool).map(|(b, map, _)| (b, map))
}

/// The split, the body face each result face came from, and for each listed face how many faces
/// it became besides itself.
fn split(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<(Body, Vec<usize>, Vec<usize>)> {
    const NAME: &str = "split face";
    body.require_brep(NAME)?;
    let total = body.face_count();
    if faces.is_empty() || faces.iter().any(|f| *f >= total) {
        return Err(invalid("no such face"));
    }
    if faces.iter().enumerate().any(|(i, f)| faces.get(..i).is_some_and(|before| before.contains(f))) {
        return Err(invalid("a face is listed twice"));
    }
    let reach = reach_of(body);
    let tools = smlib_tools(&cutters(tool, &reach)?, &reach)?;
    let missed = || invalid("the tool does not cross the selected faces");
    if tools.is_empty() {
        return Err(missed());
    }
    guard(NAME, || {
        let (sb, ob) = tagged(NAME, body)?;
        let at: Vec<usize> = faces
            .iter()
            .map(|f| if ob.faces.is_empty() { Some(*f) } else { ob.faces.iter().position(|c| c == f) })
            .collect::<Option<_>>()
            .ok_or_else(|| failed(NAME, "a listed face is not in SMLib's body"))?;
        let refs: Vec<&solidcraft_smlib::ssi::Surface> = tools.iter().collect();
        let (r, made) = sb
            .split_faces(&at, &refs)
            .map_err(|e| if matches!(e, solidcraft_smlib::Error::ToolStopsShort) { stops_short(tool) } else { failed(NAME, e) })?
            .ok_or_else(missed)?;
        let (out, h) = brought_back(NAME, body, &ob, &r)?;
        let map = h
            .faces
            .iter()
            .map(|f| f.origins.iter().find_map(|o| if let FaceOrigin::Face { face, .. } = o { Some(*face) } else { None }))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| failed(NAME, "a face of the result comes from no face of the body"))?;
        // The new edges, remembered so that export keeps the pieces apart.
        let mut lines: Vec<Vec<Vec3>> = body.split_lines().map(|l| l.to_vec()).unwrap_or_default();
        lines.extend(new_lines(&out, &h)?);
        let mut out = out.with_split_lines(Some(std::sync::Arc::new(lines))).with_checked_history(Some(h), &record::counts(&[body]));
        // An authored boundary: later rebuilds keep it.
        out.regularization_protected = true;
        Ok((out, map, made))
    })
}

/// The refusal for a tool that runs into the faces and ends inside them, so it cuts nothing off.
/// A face tool is already taken over its surface's whole extent ([`face_patch`]) and an open
/// sketch curve past its ends, so extending them is no help.
fn stops_short(tool: &SplitTool) -> KernelError {
    invalid(match tool {
        SplitTool::Face { .. } => "the tool face doesn't reach across the surface, even extended",
        SplitTool::Curves { .. } => "the sketch curves don't reach across the surface, even extended",
        SplitTool::Plane(_) => "the plane doesn't reach across the surface",
    })
}

/// The edges of `split` that no edge of the body was before (its split lines), as polylines
/// through their exact points.
fn new_lines(split: &Body, h: &History) -> Result<Vec<Vec<Vec3>>> {
    let record = split.record();
    let mut out = Vec::new();
    for (e, prov) in record.edges.iter().zip(&h.edges) {
        if prov.origins.iter().any(|o| matches!(o, EdgeOrigin::Edge { .. })) {
            continue;
        }
        let c = CurveEval::new(e.parameters()).map_err(|err| failed("split face", err))?;
        let n = if matches!(e.curve, brep::Curve::Line { .. }) { 1 } else { 64 };
        let [t0, t1] = e.interval;
        let pts: Option<Vec<Vec3>> = (0..=n).map(|k| c.point(t0 + (t1 - t0) * k as f64 / n as f64)).collect();
        if let Some(pts) = pts.filter(|p| p.iter().all(|q| q.is_finite())) {
            out.push(pts);
        }
    }
    Ok(out)
}

/// Split a body into the parts on either side of a tool: a plane, another face's surface
/// (extended; a face of a surface body joined tangentially to others: their smooth sheet as it
/// is, and where that does not cut the body into closed solids, the whole sheet as it is), or
/// one sketch line swept along its direction. Every part is closed with the piece of the
/// tool's surface inside the body; parts on the tool's back come first, each side's in
/// face order. A body the tool misses comes back whole, and so does one it only nicks: a face
/// that ends inside the body leaves no part that is a closed solid. Each part's history is over
/// `[body]`: its faces the body's (or pieces of them), the section new.
pub fn split_body(body: &Body, tool: &SplitTool) -> Result<Vec<Body>> {
    let _span = solidcraft_profile::span("kernel", "split_body");
    body.require_brep(SPLIT_BODY)?;
    let SplitTool::Face { body: sheet, face } = tool else { return split_by(body, tool) };
    let smooth = smooth_sheet(sheet, *face)?;
    let parts = if smooth.len() > 1 {
        split_by_sheet(body, sheet, *face, &smooth)?
    } else {
        match split_by(body, tool) {
            Ok(p) if closed_solids(&p).unwrap_or(false) => p,
            Ok(p) if sheet.is_surface() => match split_by_sheet(body, sheet, *face, &smooth) {
                Ok(sheet_parts) if closed_solids(&sheet_parts).unwrap_or(false) => sheet_parts,
                _ => p,
            },
            Err(e) if sheet.is_surface() => match split_by_sheet(body, sheet, *face, &smooth) {
                Ok(sheet_parts) if closed_solids(&sheet_parts).unwrap_or(false) => sheet_parts,
                _ => return Err(e),
            },
            Ok(p) => p,
            Err(e) => return Err(e),
        }
    };
    // The picked face and its smooth neighbours are the tool where they cut the body into closed
    // solids. Where they do not (they end inside it, or miss it), the whole sheet the face
    // belongs to is, across any crease between its faces (#17250): a loft between two spline
    // curves meets itself at creases under a degree, and its picked face alone stops short.
    let whole = whole_sheet(sheet, *face)?;
    if whole.len() == smooth.len() || closed_solids(&parts)? {
        return Ok(parts);
    }
    split_by_sheet(body, sheet, *face, &whole)
}

/// Split a body with faces of another body exactly as they are, bounded where they end and never
/// extended (#9305): `faces` (in `sheet`'s face order) and `face` are the tool, and `face` gives
/// the sides (parts on its back first, as [`split_body`]). The faces must join into one sheet:
/// picks in separate pieces are refused. Faces that do not cut the body into closed solids (they
/// end inside it, or miss it) are refused, so the body never comes back with a slit. Each part's history is over `[body]`.
pub fn split_body_by_faces(body: &Body, sheet: &Body, face: usize, faces: &[usize]) -> Result<Vec<Body>> {
    let _span = solidcraft_profile::span("kernel", "split_body_by_faces");
    body.require_brep(SPLIT_BODY)?;
    sheet.require_brep("a split tool")?;
    let mut picked: Vec<usize> = faces.iter().copied().chain([face]).collect();
    picked.sort_unstable();
    picked.dedup();
    if let Some(missing) = picked.iter().find(|f| **f >= sheet.face_count()) {
        return Err(invalid(format!("the tool body has no face {missing}")));
    }
    let parts = split_by_sheet(body, sheet, face, &picked)?;
    if !closed_solids(&parts)? {
        return Err(invalid("the tool faces do not cut the body through"));
    }
    Ok(parts)
}

/// Split a body with every face of the surface body `sheet` exactly as it is, never extended,
/// where that cuts it through (#13303): `face` gives the sides, as [`split_body_by_faces`].
/// `None` when the sheet does not cut the body into closed solids (it misses it, or ends inside
/// it): one of several surface bodies may cross only some of the pieces the others leave, and
/// the body never comes back with a slit. Each part's history is over `[body]`.
pub fn split_body_by_sheet_where_it_cuts(body: &Body, sheet: &Body, face: usize) -> Result<Option<Vec<Body>>> {
    let _span = solidcraft_profile::span("kernel", "split_body_by_sheet_where_it_cuts");
    body.require_brep(SPLIT_BODY)?;
    sheet.require_brep("a split tool")?;
    if !sheet.is_surface() {
        return Err(invalid("a solid cannot split as a whole surface body"));
    }
    if face >= sheet.face_count() {
        return Err(invalid(format!("the tool body has no face {face}")));
    }
    let faces: Vec<usize> = (0..sheet.face_count()).collect();
    let parts = split_by_sheet(body, sheet, face, &faces)?;
    Ok(closed_solids(&parts)?.then_some(parts))
}

/// Whether `parts` are two or more parts, each one closed manifold solid (SMLib's exact
/// topological test): a split that cut the body through.
fn closed_solids(parts: &[Body]) -> Result<bool> {
    if parts.len() < 2 {
        return Ok(false);
    }
    for part in parts {
        if !part.smlib()?.is_manifold_solid().map_err(|e| failed(SPLIT_BODY, e))? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// [`split_body`] by `tool` as one surface: a plane, a face's surface extended, or one sketch
/// line swept along its direction.
fn split_by(body: &Body, tool: &SplitTool) -> Result<Vec<Body>> {
    let reach = reach_of(body);
    let groups = cutters(tool, &reach)?;
    let [group] = groups.as_slice() else { return Err(invalid("a body is split by a plane or a face")) };
    let [patch] = group.as_slice() else { return Err(invalid("a body is split by a plane, a face or one sketch line")) };
    let Some(surface) = smlib_tools(std::slice::from_ref(group), &reach)?.pop() else {
        return Ok(vec![crate::boolean_record::operand(body, 0, &record::counts(&[body]))]);
    };
    sliced(body, patch.flip, |sb| sb.split_body(&surface))
}

/// `tool`, a prism through the surface of `face` of `target` (run on past it), cut at that
/// surface (extended) where it crosses it the way it does at `near`, a point of the face
/// ([`split_at_facing_side`]: on a cylinder the half the face lies on, #20389), keeping the
/// pieces that start at `start`: the planar faces square to
/// `plane`'s normal at that height along it. Whether the start lies in air or inside the
/// target's material does not matter, and a piece never passes the face.
///
/// The decision is the exact on-face test, made on the split itself: `Some(pieces)` when the
/// profile's section lies wholly on the face (no kept piece has its own end cap at `far`, the
/// section is not empty, and every edge of it lies on the face's surface and inside its trim,
/// [`solidcore_ops::cap::edges_lie_on_face`]); those pieces are the result (empty: no piece
/// starts at `start`). `None` when it does not, and the caller builds the body trim instead. It
/// is not a failed attempt: one split is built, once, and answers both.
///
/// The two heights are told apart by a tolerance of the body's own, never more than a quarter of
/// the distance between them.
pub fn face_split_if_on_face(tool: &Body, target: &Body, face: usize, near: Vec3, plane: &Plane, start: f64, far: f64) -> Result<Option<Vec<Body>>> {
    let target_form = target.ops_body().ok_or_else(|| invalid("the To object is not a solid body"))?;
    let (ops_target, order) = &*target_form;
    let n = plane.normal();
    let mut out = Vec::new();
    let travel = if far > start { n } else { n * -1.0 };
    for part in split_at_facing_side(tool, target, face, near, travel)? {
        let own = part.record().tolerance;
        let cut = solidcore_ops::cap::PrismCut {
            origin: a3(plane.origin),
            normal: a3(n),
            start,
            far,
            cap_tolerance: own.min(0.25 * (far - start).abs()),
            section_tolerance: own.max(target.record().tolerance),
        };
        let planar: Vec<_> = (0..part.face_count())
            .filter_map(|i| part.planar_face(i))
            .map(|p| solidcore_ops::cap::PlanarFace { origin: a3(p.origin), normal: a3(p.normal()) })
            .collect();
        let part_form = part.ops_body().ok_or_else(|| invalid("a piece of the prism is not a solid body"))?;
        let (ops_part, part_order) = &*part_form;
        match solidcore_ops::cap::prism_part(ops_target, order, face, ops_part, part_order, &planar, &section_edges(&part), &cut)
            .map_err(|e| KernelError::Failed(format!("split prism: {e}")))?
        {
            solidcore_ops::cap::PrismPart::Dropped => {}
            solidcore_ops::cap::PrismPart::ReachesPast => return Ok(None),
            solidcore_ops::cap::PrismPart::Kept => out.push(part),
        }
    }
    Ok(Some(out))
}

/// `tool`, a prism run along `travel`, cut at the surface of face `face` of `sheet` where it
/// crosses that surface the way it does at `near` (a point of the face): the pieces, as
/// [`split_body`] gives them. A cylinder, extended, meets a line along the travel twice, once on
/// each half of it either side of its axis, and only the half the face's pick lies on ends the
/// tool there (#20389): the tool is cut by that half, its long edges carried on tangentially
/// along the travel (back the way it came where the tool leaves the cylinder at the pick, on
/// ahead where it goes in), a smooth sheet a line along the travel crosses once at most; the
/// sheet reaches past the tool every way. Every other surface (and a travel along the axis, or a
/// pick on the cylinder's edge as seen along the travel) splits at the face's surface extended,
/// as [`split_body`] does.
pub fn split_at_facing_side(tool: &Body, sheet: &Body, face: usize, near: Vec3, travel: Vec3) -> Result<Vec<Body>> {
    let whole = SplitTool::Face { body: sheet, face };
    let Some(crate::FaceSurface::Cylinder { origin, axis, radius }) = crate::face_surface(sheet, face) else {
        return split_body(tool, &whole);
    };
    let b = tool.exact_bounds()?;
    let Some(t) = solidcore_ops::facing_trough::facing_trough(a3(origin), a3(axis.vec()), radius, a3(near), a3(travel), a3(b.min), a3(b.max)) else {
        return split_body(tool, &whole);
    };
    let v3 = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    let v2 = |[x, y]: [f64; 2]| Vec2::new(x, y);
    // The frame across the axis: x along the travel, its normal along the axis.
    let plane = Plane { origin: v3(t.origin), x: v3(t.x), y: v3(t.y) };
    let chain = vec![
        Seg2::Line { a: v2(t.wing_top), b: v2(t.top) },
        Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: t.radius, start: t.start, sweep: t.sweep },
        Seg2::Line { a: v2(t.bottom), b: v2(t.wing_bottom) },
    ];
    let trough = crate::extrude_sheet(&plane, &[chain], -t.reach, t.reach)?;
    // Named by its round face: were the sheet not taken whole, that face's surface extended
    // is the whole cylinder again, never a plane of the sheet.
    let round = (0..trough.face_count())
        .find(|&f| matches!(crate::face_surface(&trough, f), Some(crate::FaceSurface::Cylinder { .. })))
        .ok_or_else(|| invalid("the sheet across the round has no round face"))?;
    split_body(tool, &SplitTool::Face { body: &trough, face: round })
}

/// `tool`, a prism run on past the surface of `face` of `target`, cut at that surface carried on
/// past the face's edges (a plane without end, a cylinder, cone, sphere or torus along its
/// surface), keeping the pieces that start at `start`: no on-face test, as the whole profile ends
/// on the extended surface. `None` when a kept piece still has its end cap at `far`: the
/// surface does not cross the prism's path over some of the profile (a cone's apex side, a plane
/// running along the path), so that part has no end.
///
/// The two heights are told apart by the body's own tolerance, never more than a quarter of the
/// distance between them, as in [`face_split_if_on_face`].
pub fn face_split_extended(tool: &Body, target: &Body, face: usize, plane: &Plane, start: f64, far: f64) -> Result<Option<Vec<Body>>> {
    let mut out = Vec::new();
    for part in split_body(tool, &SplitTool::Face { body: target, face })? {
        if !has_cap_at(&part, plane, start, far - start) {
            continue;
        }
        if has_cap_at(&part, plane, far, far - start) {
            return Ok(None);
        }
        out.push(part);
    }
    Ok(Some(out))
}

/// Whether `body` has a planar face square to `plane`'s normal at `height` along it (a prism's
/// cap there), by the body's own tolerance never over a quarter of `span`
/// ([`solidcore_ops::cap::is_square_plane_at_height`]).
pub fn has_cap_at(body: &Body, plane: &Plane, height: f64, span: f64) -> bool {
    let tol = body.record().tolerance.min(0.25 * span.abs());
    let n = a3(plane.normal());
    (0..body.face_count())
        .filter_map(|i| body.planar_face(i))
        .any(|p| solidcore_ops::cap::is_square_plane_at_height(a3(p.origin), a3(p.normal()), a3(plane.origin), n, height, tol))
}

/// The record edges of the section a split made (the faces with no origin in the body it
/// split).
fn section_edges(part: &Body) -> Vec<usize> {
    let Some(history) = part.history() else { return Vec::new() };
    let record = part.record();
    let mut edges = std::collections::BTreeSet::new();
    for (face, prov) in history.faces.iter().enumerate() {
        if !prov.origins.is_empty() {
            continue;
        }
        for l in record.faces.get(face).map(|f| f.loops.as_slice()).unwrap_or_default() {
            if let brep::Loop::Edges { edges: used } = l {
                edges.extend(used.iter().map(|e| e.edge as usize));
            }
        }
    }
    edges.into_iter().collect()
}

const SPLIT_BODY: &str = "split body";

/// `body` cut in two by `cut` (SMLib's slice of its tagged copy, giving the parts behind and in
/// front of the tool's surface), the parts in the tool's order of sides (`flip`: the tool's side
/// is its surface's back), each solid of a part its own body. The body whole where the tool
/// cuts nothing.
fn sliced(
    body: &Body,
    flip: bool,
    cut: impl FnOnce(&solidcraft_smlib::Brep) -> solidcraft_smlib::Result<Option<(solidcraft_smlib::Brep, solidcraft_smlib::Brep)>>,
) -> Result<Vec<Body>> {
    let counts = record::counts(&[body]);
    let whole = || crate::boolean_record::operand(body, 0, &counts);
    guard(SPLIT_BODY, || {
        let (sb, ob) = tagged(SPLIT_BODY, body)?;
        let Some((back, front)) = cut(&sb).map_err(|e| failed(SPLIT_BODY, e))? else { return Ok(vec![whole()]) };
        // SMLib's sides are the surface's; the tool's are the face's.
        let sides = if flip { [front, back] } else { [back, front] };
        let (mut parts, mut cut) = (Vec::new(), false);
        for side in &sides {
            let (part, h) = brought_back(SPLIT_BODY, body, &ob, side)?;
            // The section comes from no face of the body.
            cut |= h.faces.iter().any(|f| f.origins.is_empty());
            let part = part.with_checked_history(Some(h), &counts);
            let lumps = part.lumps()?;
            if lumps.len() < 2 {
                parts.push(part);
                continue;
            }
            for lump in lumps {
                let h = crate::boolean_record::through(&lump, 0, part.history());
                parts.push(lump.with_checked_history(h, &counts));
            }
        }
        Ok(if cut { parts } else { vec![whole()] })
    })
}

/// The faces of the surface body `sheet` joined to its face `face` tangentially, through edges
/// along which the faces' normals agree all along ([`crate::tangent_edge`], within the one angle
/// for smooth, [`crate::smooth_edges::ANGLE`]), and through those faces' tangent neighbours in
/// turn: the smooth surface the face is part of, in face order. Only `face` for a closed body
/// (a solid's face splits with its own surface extended).
fn smooth_sheet(sheet: &Body, face: usize) -> Result<Vec<usize>> {
    sheet.require_brep("a split tool")?;
    let r = sheet.record();
    if r.faces.get(face).is_none() {
        return Err(invalid(format!("the tool body has no face {face}")));
    }
    if !r.edges.iter().any(|e| e.uses.len() == 1) {
        return Ok(vec![face]);
    }
    let sides = crate::surfaces::face_sides(r);
    let sign = |f: usize| sides.get(f).map(|forward| if *forward { 1.0 } else { -1.0 });
    let mut joined = std::collections::BTreeSet::from([face]);
    let mut todo = vec![face];
    while let Some(f) = todo.pop() {
        let Some(this) = r.faces.get(f) else { continue };
        for l in &this.loops {
            let brep::Loop::Edges { edges } = l else { continue };
            for u in edges {
                let e = u.edge as usize;
                let Some([a, b]) = r.edges.get(e).map(|edge| edge.uses.as_slice()) else { continue };
                let g = if a.face as usize == f { b.face } else { a.face } as usize;
                if g == f || joined.contains(&g) {
                    continue;
                }
                if crate::tangent_edge::tangent_all_along(r, sign, e, crate::smooth_edges::ANGLE)? {
                    joined.insert(g);
                    todo.push(g);
                }
            }
        }
    }
    Ok(joined.into_iter().collect())
}

/// The faces of the surface body `sheet` joined to its face `face`, edge to edge, and through
/// those faces' neighbours in turn: the whole sheet the face is part of, in face order, however
/// its faces meet (smoothly or at a crease, #17250). Only `face` for a closed body.
fn whole_sheet(sheet: &Body, face: usize) -> Result<Vec<usize>> {
    sheet.require_brep("a split tool")?;
    let r = sheet.record();
    if r.faces.get(face).is_none() {
        return Err(invalid(format!("the tool body has no face {face}")));
    }
    if !r.edges.iter().any(|e| e.uses.len() == 1) {
        return Ok(vec![face]);
    }
    let mut joined = std::collections::BTreeSet::from([face]);
    let mut todo = vec![face];
    while let Some(f) = todo.pop() {
        let Some(this) = r.faces.get(f) else { continue };
        for l in &this.loops {
            let brep::Loop::Edges { edges } = l else { continue };
            for u in edges {
                let Some([a, b]) = r.edges.get(u.edge as usize).map(|edge| edge.uses.as_slice()) else { continue };
                let g = if a.face as usize == f { b.face } else { a.face } as usize;
                if joined.insert(g) {
                    todo.push(g);
                }
            }
        }
    }
    Ok(joined.into_iter().collect())
}

/// `body` cut in two by the faces `faces` of the surface body `sheet` as they are (a smooth sheet
/// of several faces, or a whole one, bounded where they end), its sides those of `sheet`'s face
/// `face`: SMLib's slice by the sheet ([`solidcraft_smlib::Brep::split_body_by_sheet`]). A sheet that does not
/// cross the body cuts nothing.
fn split_by_sheet(body: &Body, sheet: &Body, face: usize, faces: &[usize]) -> Result<Vec<Body>> {
    let picked: Vec<(usize, bool)> = faces.iter().map(|f| (*f, false)).collect();
    let c = crate::surfaces::Parts::of(sheet.record(), &picked)?.canonical()?;
    let at = c.faces.iter().position(|p| faces.get(*p) == Some(&face)).ok_or_else(|| failed(SPLIT_BODY, "the tool face is not in its sheet"))?;
    let (tool, order) = crate::native::smlib_record_ordered(&c.brep)?;
    let order = order.ok_or_else(|| failed(SPLIT_BODY, "SMLib numbers the tool sheet's faces unlike its record"))?;
    // SMLib's face `i` is the record's `order.faces[i]` (the same where none is given).
    let k = if order.faces.is_empty() { Some(at) } else { order.faces.iter().position(|f| *f == at) };
    let k = k.ok_or_else(|| failed(SPLIT_BODY, "the tool face has no SMLib face"))?;
    // SMLib's import gives each tool face its record's side in its surface.
    sliced(body, false, |sb| sb.split_body_by_sheet(&tool, k))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure, split_faces};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    fn top(b: &Body, z: f64) -> usize {
        b.faces(0.01).unwrap().iter().position(|f| (f.centroid.z - z).abs() < 1e-6 && f.plane_normal.is_some()).unwrap()
    }

    #[test]
    fn plane_splits_a_box_top() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let m0 = measure(&b).unwrap();
        let pl = Plane::new(Vec3::new(4.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let s = split_faces(&b, &[top(&b, 30.0)], &SplitTool::Plane(pl)).unwrap();
        assert_eq!(s.face_count(), 7);
        let m = measure(&s).unwrap();
        assert!(rel(m.volume, m0.volume) < 1e-9 && rel(m.area, m0.area) < 1e-9);
        let mut tops: Vec<f64> = s.faces(0.01).unwrap().iter().filter(|f| (f.centroid.z - 30.0).abs() < 1e-6).map(|f| f.area).collect();
        tops.sort_by(f64::total_cmp);
        assert!((tops[0] - 80.0).abs() < 1e-6 && (tops[1] - 120.0).abs() < 1e-6, "{tops:?}");
        // The side faces the line ends on gained a vertex but kept their area.
        assert!(s.faces(0.01).unwrap().iter().all(|f| f.area > 0.0));
        // Later features work on the split body: a hole through the split top.
        let pin = cylinder(Vec3::new(5.0, 10.0, -1.0), Vec3::Z, 2.0, 40.0).unwrap();
        let holed = crate::boolean(&s, &pin, crate::BoolOp::Cut).unwrap().unwrap();
        let v = measure(&holed).unwrap().volume;
        assert!(rel(v, 6000.0 - std::f64::consts::PI * 4.0 * 30.0) < 1e-3, "{v}");
    }

    /// Faces after a STEP round trip.
    fn round_trip_faces(b: &Body) -> usize {
        let ex = crate::ExportBody { name: "B".into(), body: b, color: None };
        let text = crate::step_export_bodies(&[ex], &crate::StepHeader::default()).unwrap();
        let imp = crate::step_import(&text).unwrap();
        imp.bodies.iter().map(|x| x.body.face_count()).sum()
    }

    #[test]
    fn export_keeps_faces_split_on_purpose() {
        // A box top split by a plane, and a cylinder's side split by a plane across its axis.
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let pl = Plane::new(Vec3::new(4.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let s = split_faces(&b, &[top(&b, 30.0)], &SplitTool::Plane(pl)).unwrap();
        assert_eq!(s.face_count(), 7);
        assert_eq!(round_trip_faces(&s), 7);
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let side = c.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_none()).unwrap();
        let mid = Plane::new(Vec3::new(0.0, 0.0, 5.0), Vec3::X, Vec3::Y).unwrap();
        let cs = split_faces(&c, &[side], &SplitTool::Plane(mid)).unwrap();
        let n = cs.face_count();
        assert!(n > c.face_count());
        // The pieces stay apart; the cylinder's incidental seam pieces still join as before.
        assert_eq!(round_trip_faces(&cs), round_trip_faces(&c) + 1);
        // Without the record (no operation drops it: only Split Face's own removal, which
        // rebuilds without the split) the pieces are incidental: the file has what the body
        // has, and the next operation joins them as it joins any pieces on one surface (SMLib's
        // booleans at their end, the local modifiers through the heal).
        let unrecorded = s.clone().with_split_lines(None);
        assert_eq!(round_trip_faces(&unrecorded), unrecorded.face_count());
        let joined = crate::boolean_record::healed(&unrecorded).unwrap();
        assert_eq!((joined.face_count(), round_trip_faces(&joined)), (6, 6));
        // A split after a split keeps both lines.
        let pl2 = Plane::new(Vec3::new(0.0, 12.0, 0.0), Vec3::X, Vec3::Z).unwrap();
        let tops: Vec<usize> =
            s.faces(0.01).unwrap().iter().enumerate().filter(|(_, f)| (f.centroid.z - 30.0).abs() < 1e-6).map(|(i, _)| i).collect();
        let s2 = split_faces(&s, &tops, &SplitTool::Plane(pl2)).unwrap();
        assert_eq!(s2.face_count(), 9);
        assert_eq!(round_trip_faces(&s2), 9);
    }

    #[test]
    fn sketch_curve_splits_a_cylinder_side_and_a_circle_splits_a_top() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let m0 = measure(&c).unwrap();
        let side = c.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_none()).unwrap();
        // A horizontal line seen from the front (XZ sketch) at z = 5, swept along y.
        let xz = Plane { y: Vec3::Z, ..Plane::XZ };
        let tool = SplitTool::Curves { plane: xz, curves: vec![vec![Seg2::Line { a: Vec2::new(-20.0, 5.0), b: Vec2::new(20.0, 5.0) }]], along: None };
        let s = split_faces(&c, &[side], &tool).unwrap();
        let m = measure(&s).unwrap();
        // (Measured on meshes: curved faces agree to about 1e-4.)
        assert!(rel(m.volume, m0.volume) < 2e-4 && rel(m.area, m0.area) < 2e-4, "{} {} {} {}", m.volume, m0.volume, m.area, m0.area);
        // The side (one face round the whole turn) splits at z = 5: a quarter and three quarters
        // of its area.
        let mut sides: Vec<f64> = s.faces(0.01).unwrap().iter().filter(|f| f.plane_normal.is_none()).map(|f| f.area).collect();
        sides.sort_by(f64::total_cmp);
        let whole = std::f64::consts::TAU * 10.0 * 20.0;
        assert_eq!(sides.len(), 2);
        assert!(rel(sides[0], whole / 4.0) < 1e-3 && rel(sides[1], whole * 0.75) < 1e-3, "{sides:?}");
        assert!(s.face_count() > c.face_count());
        // A smaller cylinder's side splits the top in a ring and a disc.
        let pin = cylinder(Vec3::ZERO, Vec3::Z, 4.0, 40.0).unwrap();
        let pin_side = pin.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_none()).unwrap();
        let s = split_faces(&c, &[top(&c, 20.0)], &SplitTool::Face { body: &pin, face: pin_side }).unwrap();
        let m = measure(&s).unwrap();
        assert!(rel(m.volume, m0.volume) < 2e-4 && rel(m.area, m0.area) < 2e-4, "{} {}", m.area, m0.area);
        let mut tops: Vec<f64> = s.faces(0.01).unwrap().iter().filter(|f| (f.centroid.z - 20.0).abs() < 1e-6).map(|f| f.area).collect();
        tops.sort_by(f64::total_cmp);
        let pi = std::f64::consts::PI;
        // (Face areas come from meshes: within their sag, which depends on where seams lie.)
        assert!(rel(tops[0], pi * 16.0) < 2e-3 && rel(tops[1], pi * 84.0) < 2e-3, "{tops:?}");
    }

    /// A plane through a hole in a plate's top: the top splits in two, each piece holding
    /// half of the hole's rim.
    #[test]
    fn plane_through_a_hole_splits_the_face() {
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
        let b = crate::boolean(&plate, &pin, crate::BoolOp::Cut).unwrap().unwrap();
        let m0 = measure(&b).unwrap();
        let pl = Plane::new(Vec3::new(20.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let s = split_faces(&b, &[top(&b, 10.0)], &SplitTool::Plane(pl)).unwrap();
        let m = measure(&s).unwrap();
        assert!(rel(m.volume, m0.volume) < 1e-4 && rel(m.area, m0.area) < 1e-4);
        let tops: Vec<f64> = s.faces(0.01).unwrap().iter().filter(|f| (f.centroid.z - 10.0).abs() < 1e-6).map(|f| f.area).collect();
        let half = (1200.0 - std::f64::consts::PI * 25.0) / 2.0;
        assert_eq!(tops.len(), 2, "{tops:?}");
        assert!(tops.iter().all(|a| rel(*a, half) < 1e-3), "{tops:?}");
    }

    /// Cubic spans of a spline, each with its x control points evenly spaced (so x runs linearly
    /// along it and y is a cubic in x), meeting tangent, extruded from z = -5 to 15.
    fn spans(ys: &[[f64; 4]]) -> Body {
        let chain = ys
            .iter()
            .enumerate()
            .map(|(i, y)| {
                let x0 = -3.0 + 6.0 * i as f64;
                let p = |k: usize| Vec2::new(x0 + 2.0 * k as f64, y[k]);
                Seg2::Cubic { p0: p(0), p1: p(1), p2: p(2), p3: p(3) }
            })
            .collect();
        crate::extrude_sheet(&Plane::XY, &[chain], -5.0, 15.0).unwrap()
    }

    /// The face of `b` farthest to -x (by its centroid).
    fn leftmost(b: &Body) -> usize {
        let faces = b.faces(0.01).unwrap();
        (0..faces.len()).min_by(|i, j| faces[*i].centroid.x.total_cmp(&faces[*j].centroid.x)).unwrap()
    }

    /// Split Body by a face of a smooth sheet cuts with the whole sheet (#3830): three spans
    /// meeting tangent cross the cube 0…10, the face of the first alone does not. The part
    /// under the spline is 10 ∫ y dx over x 0…10 (y a cubic in x on each span: Gauss–Legendre at
    /// three points is exact). Two spans that end inside the cube cut nothing.
    #[test]
    fn a_face_of_a_smooth_sheet_splits_with_the_whole_sheet() {
        let ys = [[5.0, 5.0, 4.0, 4.0], [4.0, 4.0, 6.0, 6.0], [6.0, 6.0, 5.0, 5.0]];
        let bern = |y: &[f64; 4], t: f64| {
            let s = 1.0 - t;
            y[0] * s * s * s + 3.0 * y[1] * s * s * t + 3.0 * y[2] * s * t * t + y[3] * t * t * t
        };
        // ∫ y dx over the span's t0…t1 (dx = 6 dt).
        let integral = |y: &[f64; 4], t0: f64, t1: f64| {
            let (m, h) = (0.5 * (t0 + t1), 0.5 * (t1 - t0));
            let g = (0.6f64).sqrt();
            6.0 * h * (5.0 * bern(y, m - h * g) + 8.0 * bern(y, m) + 5.0 * bern(y, m + h * g)) / 9.0
        };
        let under = 10.0 * (integral(&ys[0], 0.5, 1.0) + integral(&ys[1], 0.0, 1.0) + integral(&ys[2], 0.0, 1.0 / 6.0));
        let sheet = spans(&ys);
        assert_eq!(sheet.face_count(), 3);
        let first = leftmost(&sheet);
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let parts = split_body(&cube, &SplitTool::Face { body: &sheet, face: first }).unwrap();
        let v = volumes(&parts);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(rel(v[0], under) < 1e-6 && rel(v[1], 1000.0 - under) < 1e-6, "{v:?} vs {under}");

        let short = spans(&ys[..2]);
        let first = leftmost(&short);
        let nicked = split_body(&cube, &SplitTool::Face { body: &short, face: first }).unwrap();
        let info: Vec<_> = nicked.iter().map(|b| (measure(b).unwrap().volume, b.face_count(), b.lumps().unwrap().len())).collect();
        assert_eq!(nicked.len(), 1, "{info:?}");
    }

    /// A face that ends inside the body only nicks it: Split Body leaves the body whole, never a
    /// lamina and a body with a slit, neither of them closed (#17192). One span alone runs over
    /// x -3…3 and ends inside the cube 0…10.
    #[test]
    fn a_face_that_ends_inside_the_body_leaves_it_whole() {
        let sheet = spans(&[[5.0, 5.0, 4.0, 4.0]]);
        assert_eq!(sheet.face_count(), 1);
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let parts = split_body(&cube, &SplitTool::Face { body: &sheet, face: 0 }).unwrap();
        let info: Vec<_> = parts.iter().map(|b| (measure(b).unwrap().volume, b.face_count())).collect();
        assert_eq!(parts.len(), 1, "{info:?}");
        assert!(rel(measure(&parts[0]).unwrap().volume, 1000.0) < 1e-9, "{info:?}");
        assert_eq!(parts[0].face_count(), 6, "{info:?}");
    }

    /// A single-face open sheet that crosses the body cuts it through into closed solids (#18891).
    #[test]
    fn a_single_face_sheet_that_crosses_the_body_splits_it() {
        use crate::{PathPiece, PointPath, SweepControl};
        let d = 20.0 / std::f64::consts::SQRT_2;
        let wavy = vec![
            Vec3::new(20.0, 0.0, 9.0),
            Vec3::new(d, d, 4.0),
            Vec3::new(0.0, 20.0, 9.0),
            Vec3::new(-d, d, 4.0),
            Vec3::new(-20.0, 0.0, 9.0),
            Vec3::new(-d, -d, 4.0),
            Vec3::new(0.0, -20.0, 9.0),
            Vec3::new(d, -d, 4.0),
            Vec3::new(20.0, 0.0, 9.0),
        ];
        let path = PointPath { pieces: vec![PathPiece::Spline { fit: wavy, reversed: false }] };
        let flat_at = Plane { origin: Vec3::new(0.0, 0.0, 9.0), x: Vec3::X, y: Vec3::Y };
        let radial = vec![Seg2::Line { a: Vec2::new(15.0, 0.0), b: Vec2::new(25.0, 0.0) }];
        let wall = crate::sweep_sheet_point_path_with(&flat_at, &[radial], &path, &SweepControl::default()).unwrap();
        assert_eq!(wall.face_count(), 1);
        let cube = crate::box_solid(Vec3::new(18.0, -2.0, 7.0), Vec3::new(22.0, 2.0, 11.0)).unwrap();
        let parts = split_body(&cube, &SplitTool::Face { body: &wall, face: 0 }).unwrap();
        assert_eq!(parts.len(), 2, "parts: {:?}", parts.len());
        assert!(closed_solids(&parts).unwrap());
    }

    /// A face of a creased sheet that cuts the body through on its own keeps cutting alone: a flat
    /// face along y = 5 from x = -3 to 5, then one rising 1 in 50 (a crease of about 1.15°) to
    /// x = 13, extruded from z = -5 to 15. The first face's plane, extended, cuts the cube 0…10
    /// into two closed halves of 500, as before #17250, not by the whole sheet (502.5 under it).
    #[test]
    fn a_face_of_a_creased_sheet_that_cuts_through_alone_keeps_its_cut() {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let chain = vec![Seg2::Line { a: p(-3.0, 5.0), b: p(5.0, 5.0) }, Seg2::Line { a: p(5.0, 5.0), b: p(13.0, 5.16) }];
        let sheet = crate::extrude_sheet(&Plane::XY, &[chain], -5.0, 15.0).unwrap();
        assert_eq!(sheet.face_count(), 2);
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let v = volumes(&split_body(&cube, &SplitTool::Face { body: &sheet, face: leftmost(&sheet) }).unwrap());
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().all(|v| rel(*v, 500.0) < 1e-9), "{v:?}");
    }

    /// Split Body by picked faces as they are (#9305), against the picked face extended. The sheet
    /// is a line at y = 5 from x = -3 to 5, then a line rising 1 in 2 to (13, 9), extruded from
    /// z = -5 to 15; it crosses the cube 0…10 with a crease of about 26.6° at x = 5. The first
    /// face extended is the plane y = 5: halves of 500. Both faces as they are leave under them
    /// 10 · (5 · 5 + ∫ (5 + (x - 5) / 2) dx over x 5…10) = 10 · (25 + 25 + 6.25) = 562.5, and
    /// 437.5 above. The first face alone ends inside the cube at x = 5, so it is refused, and a
    /// face number the sheet does not have is refused.
    #[test]
    fn picked_faces_of_a_bent_sheet_split_as_they_are_not_extended() {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let chain = vec![Seg2::Line { a: p(-3.0, 5.0), b: p(5.0, 5.0) }, Seg2::Line { a: p(5.0, 5.0), b: p(13.0, 9.0) }];
        let sheet = crate::extrude_sheet(&Plane::XY, &[chain], -5.0, 15.0).unwrap();
        assert_eq!(sheet.face_count(), 2);
        let first = leftmost(&sheet);
        let second = 1 - first;
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let extended = volumes(&split_body(&cube, &SplitTool::Face { body: &sheet, face: first }).unwrap());
        assert!(extended.len() == 2 && extended.iter().all(|v| rel(*v, 500.0) < 1e-9), "{extended:?}");
        for (face, faces) in [(first, vec![second]), (second, vec![first]), (first, vec![0, 1])] {
            let parts = split_body_by_faces(&cube, &sheet, face, &faces).unwrap();
            let v = volumes(&parts);
            assert!(v.len() == 2 && rel(v[0], 437.5) < 1e-9 && rel(v[1], 562.5) < 1e-9, "{face} {faces:?}: {v:?}");
            assert!(parts.iter().all(|b| b.smlib().unwrap().is_manifold_solid().unwrap()));
        }
        let e = split_body_by_faces(&cube, &sheet, first, &[]).unwrap_err().to_string();
        assert!(e.contains("do not cut the body through"), "{e}");
        assert!(split_body_by_faces(&cube, &sheet, first, &[7]).is_err());
    }

    /// One of several surface bodies splitting a body cuts the pieces it crosses and leaves the
    /// others (#13303). The bent sheet of the test above cuts the cube 0…10 into 437.5 and 562.5.
    /// A flat sheet at y = 5 that ends at x = 5, inside the cube, only nicks it, and a flat sheet
    /// at y = 20 misses it: both leave it alone (`None`), never with a slit. A solid is refused.
    #[test]
    fn a_whole_surface_body_splits_where_it_cuts_and_leaves_what_it_misses() {
        let p = |x: f64, y: f64| Vec2::new(x, y);
        let bent = vec![Seg2::Line { a: p(-3.0, 5.0), b: p(5.0, 5.0) }, Seg2::Line { a: p(5.0, 5.0), b: p(13.0, 9.0) }];
        let bent = crate::extrude_sheet(&Plane::XY, &[bent], -5.0, 15.0).unwrap();
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let parts = split_body_by_sheet_where_it_cuts(&cube, &bent, 0).unwrap().unwrap();
        let v = volumes(&parts);
        assert!(v.len() == 2 && rel(v[0], 437.5) < 1e-9 && rel(v[1], 562.5) < 1e-9, "{v:?}");
        assert!(parts.iter().all(|b| b.smlib().unwrap().is_manifold_solid().unwrap()));
        for line in [Seg2::Line { a: p(-3.0, 5.0), b: p(5.0, 5.0) }, Seg2::Line { a: p(-3.0, 20.0), b: p(13.0, 20.0) }] {
            let sheet = crate::extrude_sheet(&Plane::XY, &[vec![line]], -5.0, 15.0).unwrap();
            assert!(split_body_by_sheet_where_it_cuts(&cube, &sheet, 0).unwrap().is_none());
        }
        assert!(split_body_by_sheet_where_it_cuts(&cube, &cube, 0).is_err());
    }

    /// A face of a creased sheet whose smooth part ends inside the body cuts with the whole sheet
    /// (#17250, the loft of #17192): two spans meeting tangent from x = -3 to 9, then a line rising
    /// 1 in 50 from their end (a crease of about 1.15°) to x = 13, extruded from z = -5 to 15. The
    /// spans alone end inside the cube 0…10; the whole sheet cuts it into two closed solids, the
    /// part under it 10 ∫ y dx over x 0…10 (y a cubic in x on each span: Gauss–Legendre at three
    /// points is exact; the line's ∫ over 9…10 is 6.01).
    #[test]
    fn a_face_of_a_creased_sheet_that_stops_short_splits_with_the_whole_sheet() {
        let ys = [[5.0, 5.0, 4.0, 4.0], [4.0, 4.0, 6.0, 6.0]];
        let bern = |y: &[f64; 4], t: f64| {
            let s = 1.0 - t;
            y[0] * s * s * s + 3.0 * y[1] * s * s * t + 3.0 * y[2] * s * t * t + y[3] * t * t * t
        };
        // ∫ y dx over the span's t0…t1 (dx = 6 dt).
        let integral = |y: &[f64; 4], t0: f64, t1: f64| {
            let (m, h) = (0.5 * (t0 + t1), 0.5 * (t1 - t0));
            let g = (0.6f64).sqrt();
            6.0 * h * (5.0 * bern(y, m - h * g) + 8.0 * bern(y, m) + 5.0 * bern(y, m + h * g)) / 9.0
        };
        let under = 10.0 * (integral(&ys[0], 0.5, 1.0) + integral(&ys[1], 0.0, 1.0) + 6.01);
        let mut chain: Vec<Seg2> = ys
            .iter()
            .enumerate()
            .map(|(i, y)| {
                let x0 = -3.0 + 6.0 * i as f64;
                let p = |k: usize| Vec2::new(x0 + 2.0 * k as f64, y[k]);
                Seg2::Cubic { p0: p(0), p1: p(1), p2: p(2), p3: p(3) }
            })
            .collect();
        chain.push(Seg2::Line { a: Vec2::new(9.0, 6.0), b: Vec2::new(13.0, 6.08) });
        let sheet = crate::extrude_sheet(&Plane::XY, &[chain], -5.0, 15.0).unwrap();
        assert_eq!(sheet.face_count(), 3);
        let cube = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let parts = split_body(&cube, &SplitTool::Face { body: &sheet, face: leftmost(&sheet) }).unwrap();
        let v = volumes(&parts);
        assert_eq!(v.len(), 2, "{v:?}");
        // The part under the sheet is the smaller: 485.725 of 1000.
        assert!(rel(v[0], under) < 1e-6 && rel(v[1], 1000.0 - under) < 1e-6, "{v:?} vs {under}");
        assert!(closed_solids(&parts).unwrap(), "{v:?}");
    }

    fn volumes(parts: &[Body]) -> Vec<f64> {
        let mut v: Vec<f64> = parts.iter().map(|b| measure(b).unwrap().volume).collect();
        v.sort_by(f64::total_cmp);
        v
    }

    /// A cylinder r 5 along z through a sphere r 15 on its axis, split by the sphere's face:
    /// the core inside the sphere and the two ends outside it. The sphere is closed, so its face
    /// has no edge to extend past; the split takes the sphere as it is (SMLib refused it with
    /// status 1001, and Extrude To a sphere fell back to its old trim).
    #[test]
    fn a_cylinder_through_a_sphere_splits_by_its_face_into_core_and_ends() {
        let rod = cylinder(Vec3::new(0.0, 0.0, -30.0), Vec3::Z, 5.0, 60.0).unwrap();
        let ball = crate::sphere(Vec3::ZERO, 15.0).unwrap();
        let face = ball.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_none()).unwrap();
        let parts = split_body(&rod, &SplitTool::Face { body: &ball, face }).unwrap();
        let v = volumes(&parts);
        let pi = std::f64::consts::PI;
        // The rod inside the sphere: twice the integral of sqrt(225 - r^2) over the r 5 disc.
        let core = 4.0 * pi / 3.0 * (15.0_f64.powi(3) - 200.0_f64.powf(1.5));
        let whole = pi * 25.0 * 60.0;
        let end = (whole - core) / 2.0;
        assert!(v.len() == 3, "{v:?}");
        assert!(rel(v[0], end) < 1e-4 && rel(v[1], end) < 1e-4 && rel(v[2], core) < 1e-4, "{v:?}, core {core}, ends {end}");
        // The same rod across the sphere, its poles outside the rod: the same parts.
        let across = cylinder(Vec3::new(-30.0, 0.0, 0.0), Vec3::X, 5.0, 60.0).unwrap();
        let v = volumes(&split_body(&across, &SplitTool::Face { body: &ball, face }).unwrap());
        assert!(v.len() == 3, "{v:?}");
        assert!(rel(v[0], end) < 1e-4 && rel(v[1], end) < 1e-4 && rel(v[2], core) < 1e-4, "{v:?}, core {core}, ends {end}");
    }

    /// A round along a curved edge keeps its definition (`Surface::Blend`), which has no (u, v)
    /// of its own, and is a split tool by its face's NURBS form. A cylinder r 20, 20 tall, its
    /// bottom rim rounded r 5: the round is the torus of major radius 15 and tube radius 5 about
    /// (0, 0, 5). Shelled 2 in from the top, the cavity's round is that torus with tube radius 3.
    /// A tube r 16 to 17, 20 tall, split by either round: below the torus of tube radius r it
    /// holds 2 pi times the integral of (5 - sqrt(r^2 - u^2)) (15 + u) over u = rho - 15 in
    /// [1, 2]. Both splits were refused (#23158): "the split tool's surface: not supported yet:
    /// a blend's label".
    #[test]
    fn a_tube_splits_by_a_round_on_a_cylinder_rim_and_by_the_round_of_its_shell() {
        let pi = std::f64::consts::PI;
        let disc = cylinder(Vec3::ZERO, Vec3::Z, 20.0, 20.0).unwrap();
        let rounded = crate::fillet(&disc, &[Vec3::new(20.0, 0.0, 0.0)], 5.0).unwrap();
        let shelled = crate::shell(&rounded, &[Vec3::new(0.0, 0.0, 20.0)], 2.0).unwrap();
        let outer = cylinder(Vec3::ZERO, Vec3::Z, 17.0, 20.0).unwrap();
        let bore = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 16.0, 22.0).unwrap();
        let tube = crate::boolean(&outer, &bore, crate::BoolOp::Cut).unwrap().unwrap();
        let whole = pi * (289.0 - 256.0) * 20.0;
        // The integral of sqrt(r^2 - u^2) and of u sqrt(r^2 - u^2).
        let f = |r: f64, u: f64| u / 2.0 * (r * r - u * u).sqrt() + r * r / 2.0 * (u / r).asin();
        let g = |r: f64, u: f64| -(r * r - u * u).powf(1.5) / 3.0;
        let below = |r: f64| 2.0 * pi * (5.0 * 16.5 - (g(r, 2.0) - g(r, 1.0)) - 15.0 * (f(r, 2.0) - f(r, 1.0)));
        // The round's face of each body: the face at the point (17.5, 0, z) of its torus.
        let round_of = |b: &Body, r: f64| {
            let z = 5.0 - (r * r - 2.5f64 * 2.5).sqrt();
            let i = b.face_at(Vec3::new(17.5, 0.0, z), 1e-6).unwrap();
            assert!(matches!(b.record().faces[i].surface, Surface::Blend { .. }), "face {i} of the r {r} round keeps its definition");
            i
        };
        for (b, r) in [(&rounded, 5.0), (&shelled, 3.0)] {
            let v = volumes(&split_body(&tube, &SplitTool::Face { body: b, face: round_of(b, r) }).unwrap());
            let low = below(r);
            assert!(
                v.len() == 2 && rel(v[0], low.min(whole - low)) < 1e-6 && rel(v[1], low.max(whole - low)) < 1e-6,
                "r {r}: {v:?}, below {low}, whole {whole}"
            );
        }
    }

    #[test]
    fn split_body_by_plane_and_by_a_face() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let pl = Plane::new(Vec3::new(4.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let v = volumes(&split_body(&b, &SplitTool::Plane(pl)).unwrap());
        assert!(v.len() == 2 && rel(v[0], 2400.0) < 1e-9 && rel(v[1], 3600.0) < 1e-9, "{v:?}");
        // A plate with a hole split through the hole: two equal halves.
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
        let holed = crate::boolean(&plate, &pin, crate::BoolOp::Cut).unwrap().unwrap();
        let pl = Plane::new(Vec3::new(20.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let v = volumes(&split_body(&holed, &SplitTool::Plane(pl)).unwrap());
        let half = (12000.0 - std::f64::consts::PI * 250.0) / 2.0;
        assert!(v.len() == 2 && rel(v[0], half) < 1e-4 && rel(v[1], half) < 1e-4, "{v:?}");
        // A box split by a cylinder's side: the core and the rest.
        let tool = cylinder(Vec3::new(5.0, 10.0, -5.0), Vec3::Z, 4.0, 40.0).unwrap();
        let side = tool.faces(0.01).unwrap().iter().position(|f| f.plane_normal.is_none()).unwrap();
        let v = volumes(&split_body(&b, &SplitTool::Face { body: &tool, face: side }).unwrap());
        let core = std::f64::consts::PI * 16.0 * 30.0;
        assert!(v.len() == 2 && rel(v[0], core) < 1e-3 && rel(v[1], 6000.0 - core) < 1e-3, "{v:?}");
        // A plane that misses gives the body back.
        let far = Plane::new(Vec3::new(50.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        assert_eq!(split_body(&b, &SplitTool::Plane(far)).unwrap().len(), 1);
    }

    /// Sketch arcs split exactly (a circular edge), a listed face the tool misses is named, and
    /// the map gives each new face its source.
    #[test]
    fn sketch_arc_splits_exactly_and_misses_are_named() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let t = top(&b, 30.0);
        let arc = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 6.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 };
        let plane = Plane { origin: Vec3::new(0.0, 0.0, 30.0), ..Plane::XY };
        let tool = SplitTool::Curves { plane, curves: vec![vec![arc]], along: None };
        let (s, map) = split_faces_with_map(&b, &[t], &tool).unwrap();
        assert_eq!(map.len(), s.face_count());
        assert_eq!(map.iter().filter(|o| **o == t).count(), 2);
        let mut tops: Vec<f64> = s.faces(0.001).unwrap().iter().filter(|f| (f.centroid.z - 30.0).abs() < 1e-6).map(|f| f.area).collect();
        tops.sort_by(f64::total_cmp);
        let quarter = std::f64::consts::PI * 36.0 / 4.0;
        // (Face areas come from meshes.)
        assert!(rel(tops[0], quarter) < 1e-3 && rel(tops[1], 200.0 - quarter) < 1e-3, "{tops:?}");
        let record = s.record();
        assert!(record.edges.iter().any(|e| matches!(e.curve, brep::Curve::Circle { .. } | brep::Curve::RationalSpline { .. })));
        // A listed face the tool misses is named: the plane z = 10 crosses the sides, not the top.
        let mid = Plane { origin: Vec3::new(0.0, 0.0, 10.0), ..Plane::XY };
        let side = b.faces(0.01).unwrap().iter().position(|f| (f.centroid.x - 10.0).abs() < 1e-6).unwrap();
        assert!(split_faces(&b, &[side], &SplitTool::Plane(mid)).is_ok());
        let e = split_faces(&b, &[side, t], &SplitTool::Plane(mid)).unwrap_err().to_string();
        assert!(e.contains(&format!("face {t}")), "{e}");
    }

    #[test]
    fn a_tool_that_misses_is_an_error() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let pl = Plane::new(Vec3::new(50.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        assert!(split_faces(&b, &[0], &SplitTool::Plane(pl)).is_err());
        assert!(split_faces(&b, &[99], &SplitTool::Plane(Plane::XY)).is_err());
    }

    /// A wall (the line y = 0 from x = 0 to 10, swept 10 up z) turned over with Reverse Normal
    /// splits at x = 5 into two halves of 50 (#13640), each facing as the turned wall did: the
    /// other way from the wall.
    #[test]
    fn a_turned_wall_splits_and_its_pieces_keep_its_side() {
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let wall = crate::extrude_sheet(&Plane::XY, &[line], 0.0, 10.0).unwrap();
        let turned = crate::reverse_faces(&wall, &[]).unwrap();
        let normal = |b: &Body| b.faces(0.01).unwrap().iter().map(|f| f.plane_normal.unwrap()).collect::<Vec<_>>();
        let [n] = normal(&turned)[..] else { panic!("one face") };
        let [m] = normal(&wall)[..] else { panic!("one face") };
        assert!(n.dot(m) < -1.0 + 1e-9, "{n:?} vs {m:?}");
        let pl = Plane::new(Vec3::new(5.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let s = split_faces(&turned, &[0], &SplitTool::Plane(pl)).unwrap();
        assert_eq!(s.face_count(), 2);
        assert!(s.faces(0.01).unwrap().iter().all(|f| rel(f.area, 50.0) < 1e-9));
        assert!(normal(&s).iter().all(|k| k.dot(n) > 1.0 - 1e-9), "{:?} vs {n:?}", normal(&s));
    }

    /// A box 10 × 20 × 30 split by a wall at x = 4 (a sheet past the box on every side) comes
    /// apart into 2400 and 3600, the part behind the wall's face first; split by the same wall
    /// turned over with Reverse Normal, the parts come in the other order (#13640).
    #[test]
    fn a_turned_tool_sheet_splits_a_body_with_its_sides_swapped() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let line = vec![Seg2::Line { a: Vec2::new(4.0, -5.0), b: Vec2::new(4.0, 25.0) }];
        let wall = crate::extrude_sheet(&Plane::XY, &[line], -5.0, 35.0).unwrap();
        let turned = crate::reverse_faces(&wall, &[]).unwrap();
        let v = |tool: &Body| split_body_by_faces(&b, tool, 0, &[0]).unwrap().iter().map(|p| measure(p).unwrap().volume).collect::<Vec<_>>();
        let (a, z) = (v(&wall), v(&turned));
        assert!(a.len() == 2 && z.len() == 2, "{a:?} {z:?}");
        assert!(rel(a[0] + a[1], 6000.0) < 1e-9 && (rel(a[0], 2400.0) < 1e-9 || rel(a[0], 3600.0) < 1e-9), "{a:?}");
        assert!(rel(z[0], a[1]) < 1e-9 && rel(z[1], a[0]) < 1e-9, "{a:?} then {z:?}");
    }

    /// #20389: a bar run along the travel up to a quarter of a cylinder (axis z through (30, 20),
    /// radius 10, the quarter toward +x +y) is cut where it crosses the cylinder the way it does at
    /// the pick, not where it first meets the cylinder extended. The bar covers y 22…28 and z 0…4,
    /// where the cylinder spans x = 30 ∓ √(100 − (y − 20)²). From x = −10 along +x it leaves the
    /// cylinder at the quarter: the piece at the start runs to the far crossing,
    /// 4 · (6 · 40 + ∫₂⁸ √(100 − u²) du). From x = 60 along −x it goes in there: the piece at the
    /// start runs to the near crossing, 4 · (6 · 30 − ∫₂⁸ √(100 − u²) du).
    #[test]
    fn a_bar_is_cut_where_it_crosses_the_cylinder_as_at_the_pick() {
        let quarter = vec![Seg2::Arc { center: Vec2::new(30.0, 20.0), radius: 10.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 }];
        let sheet = crate::extrude_sheet(&Plane::XY, &[quarter], -5.0, 10.0).unwrap();
        let h = 10.0 / 2f64.sqrt();
        let near = Vec3::new(30.0 + h, 20.0 + h, 2.0);
        let primitive = |u: f64| u / 2.0 * (100.0 - u * u).sqrt() + 50.0 * (u / 10.0).asin();
        let segment = primitive(8.0) - primitive(2.0);
        for (from, to, travel, want) in [(-10.0, 60.0, Vec3::X, 4.0 * (240.0 + segment)), (60.0, -10.0, -Vec3::X, 4.0 * (180.0 - segment))] {
            let bar = crate::box_solid(Vec3::new(f64::min(from, to), 22.0, 0.0), Vec3::new(f64::max(from, to), 28.0, 4.0)).unwrap();
            let parts = split_at_facing_side(&bar, &sheet, 0, near, travel).unwrap();
            // The piece at the start: the one reaching `from`.
            let start = parts
                .iter()
                .find(|p| {
                    let b = p.exact_bounds().unwrap();
                    (b.min.x - from).abs() < 1e-6 || (b.max.x - from).abs() < 1e-6
                })
                .unwrap();
            let got = measure(start).unwrap().volume;
            assert!(rel(got, want) < 1e-6, "from {from}: {got} vs {want}");
        }
    }

    /// #21864: a split-in pocket whose tangent spline outline merges a degree-2 vertex during
    /// pre-offset healing must remain separate from the raised face regardless of loop direction
    /// (both forward and reversed outlines must cut the same 4 mm recess instead of filling it).
    #[test]
    fn offset_face_keeps_split_pocket_regardless_of_spline_outline_direction() {
        use solidcore_record::{Loop2, Region2};
        use std::f64::consts::TAU;

        // Unit checks for `on_split_line` across joined/merged recorded split lines.
        let l1 = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0)];
        let l2 = vec![Vec3::new(5.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)];
        let l3 = vec![Vec3::new(10.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 0.0)];
        let keep_lines = vec![l1, l2, l3];
        // Edge spanning l1 + l2 (merged degree-2 vertex at x = 5): every sample lies on the union.
        let merged_pts =
            [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.5, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0), Vec3::new(7.5, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)];
        assert!(crate::heal::on_split_line(&merged_pts, &keep_lines));
        // Diagonal chord bridging l2 and l3 across the corner (10, 0): interior samples miss both.
        let chord_pts =
            [Vec3::new(5.0, 0.0, 0.0), Vec3::new(6.25, 2.5, 0.0), Vec3::new(7.5, 5.0, 0.0), Vec3::new(8.75, 7.5, 0.0), Vec3::new(10.0, 10.0, 0.0)];
        assert!(!crate::heal::on_split_line(&chord_pts, &keep_lines));
        assert!(!crate::heal::on_split_line(&[], &keep_lines));
        assert!(!crate::heal::on_split_line(&merged_pts, &[]));

        let outer = Loop2 { segs: vec![Seg2::Arc { center: Vec2::ZERO, radius: 101.0, start: 0.0, sweep: TAU }] };
        let inner = Loop2 { segs: vec![Seg2::Arc { center: Vec2::ZERO, radius: 99.0, start: 0.0, sweep: TAU }] };
        let tube = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![inner] }], 0.0, 400.0).unwrap().remove(0);
        let f_pos = tube.face_within(Vec3::new(0.0, 99.0, 10.0), 1.0).unwrap().unwrap();
        let f_neg = tube.face_within(Vec3::new(0.0, -99.0, 10.0), 1.0).unwrap().unwrap();
        let mut faces = vec![f_pos];
        if f_neg != f_pos {
            faces.push(f_neg);
        }
        let pl_y = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
        let tube = split_faces(&tube, &faces, &SplitTool::Plane(pl_y)).unwrap();

        let p = |x: f64, y: f64| Vec2::new(x, y);
        let forward = vec![
            Seg2::Line { a: p(-8.44878, 378.5), b: p(-3.799174, 378.5) },
            Seg2::Cubic { p0: p(-3.799174, 378.5), p1: p(-2.442599, 378.499905), p2: p(-1.254548, 377.587947), p3: p(-0.902724, 376.277972) },
            Seg2::Cubic { p0: p(-0.902724, 376.277972), p1: p(-0.550956, 374.96748), p2: p(-1.122935, 373.583376), p3: p(-2.297185, 372.903494) },
            Seg2::Cubic { p0: p(-2.297185, 372.903494), p1: p(-3.848691, 372.005326), p2: p(-5.399984, 371.106791), p3: p(-6.950631, 370.207141) },
            Seg2::Cubic { p0: p(-6.950631, 370.207141), p1: p(-7.875272, 369.670732), p2: p(-9.015685, 369.668575), p3: p(-9.940929, 370.204788) },
            Seg2::Cubic { p0: p(-9.940929, 370.204788), p1: p(-10.867204, 370.741687), p2: p(-11.433462, 371.734105), p3: p(-11.433499, 372.803648) },
            Seg2::Line { a: p(-11.433499, 372.803648), b: p(-11.433499, 375.5) },
            Seg2::Cubic { p0: p(-11.433499, 375.5), p1: p(-11.433488, 376.294081), p2: p(-11.120363, 377.057985), p3: p(-10.56026, 377.62132) },
            Seg2::Cubic { p0: p(-10.56026, 377.62132), p1: p(-10.001249, 378.183525), p2: p(-9.241693, 378.499988), p3: p(-8.44878, 378.5) },
        ];
        let reversed: Vec<Seg2> = forward
            .iter()
            .rev()
            .map(|s| match *s {
                Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
                Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: p3, p1: p2, p2: p1, p3: p0 },
                other => other,
            })
            .collect();

        // Precomputed unit vector for the sketch plane X axis (unit normalization of (-0.382683, 0.92388, 0)).
        const SKETCH_X: Vec3 = Vec3::new(-0.382_682_898_036_223, 0.923_879_753_837_282, 0.0);
        // Precomputed plain offset-tube volume: pi * (101^2 - 95^2) * 400 = 470_400 * pi mm^3.
        const PLAIN_TUBE_VOL: f64 = 1_477_805.184_292_861_8;

        let sk_plane = Plane::new(Vec3::ZERO, SKETCH_X, Vec3::Z).unwrap();
        let target = tube.face_within(Vec3::new(91.464074, 37.88566, 10.0), 1.0).unwrap().unwrap();
        let picks = [Vec3::new(91.464074, 37.88566, 10.0), Vec3::new(91.464074, -37.88566, 10.0)];

        let run_case = |segs: Vec<Seg2>| {
            let split = split_faces(&tube, &[target], &SplitTool::Curves { plane: sk_plane, curves: vec![segs], along: None }).unwrap();
            // Even a second heal after degree-2 vertex merging must keep the pocket separate.
            let healed_once = crate::boolean_record::healed(&split).unwrap().with_split_lines(split.splits.clone());
            let healed_twice = crate::boolean_record::healed(&healed_once).unwrap().with_split_lines(split.splits.clone());
            assert_eq!(healed_twice.face_count(), split.face_count());
            let raised = crate::offset_faces(&split, &picks, 4.0).unwrap();
            let vol = raised.volume_at(1e-6).unwrap();
            let raised_healed = crate::offset_faces(&healed_twice, &picks, 4.0).unwrap();
            let healed_vol = raised_healed.volume_at(1e-6).unwrap();
            assert_eq!(raised_healed.face_count(), raised.face_count());
            assert!((healed_vol - vol).abs() < 1e-3, "repeated-heal offset volume ({healed_vol:.3}) must match split offset volume ({vol:.3})");
            (raised.face_count(), vol)
        };

        let (fwd_faces, fwd_vol) = run_case(forward);
        let (rev_faces, rev_vol) = run_case(reversed);
        assert!(fwd_vol < PLAIN_TUBE_VOL - 200.0, "forward pocket was filled: {fwd_vol:.3} vs plain {PLAIN_TUBE_VOL:.3}");
        assert!((fwd_vol - 1_477_519.984).abs() < 0.05, "forward volume {fwd_vol:.3}");
        assert!((fwd_vol - rev_vol).abs() < 1e-3, "forward ({fwd_vol:.3}) and reversed ({rev_vol:.3}) volumes must match");
        assert!(fwd_faces > 6 && rev_faces > 6, "recess walls must exist: fwd={fwd_faces}, rev={rev_faces}");
    }
}
