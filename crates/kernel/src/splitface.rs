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

use std::f64::consts::TAU;

use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::{self, Frame, Surface};
use solidcraft_geom::tess::uv::Starts;
use solidcraft_geom::{Aabb3, Plane, Seg2, UnitVec3, Vec2, Vec3};

use crate::body::Body;
use crate::history::{EdgeOrigin, FaceOrigin, History};
use crate::{KernelError, Result, guard, record};

/// What splits the faces.
#[derive(Clone, Debug)]
pub enum SplitTool<'a> {
    /// A plane (unbounded).
    Plane(Plane),
    /// The surface of another body's face (`face` in that body's face order), extended.
    Face { body: &'a Body, face: usize },
    /// Sketch curves (chains of segments in `plane`) swept along `along` (none: the plane's
    /// normal); each curve splits on its own, an open one extended past its ends.
    Curves { plane: Plane, curves: Vec<Vec<Seg2>>, along: Option<Vec3> },
}

/// Most segments a sketch tool may hold.
const MAX_SEGMENTS: usize = 4096;

fn invalid(what: impl Into<String>) -> KernelError {
    KernelError::Invalid(what.into())
}

fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib {what}: {e}"))
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
    let record = crate::native::brep_record(body)?;
    let f = record.faces.get(face).ok_or_else(|| invalid(format!("the tool body has no face {face}")))?;
    let forward = record
        .shells
        .iter()
        .find_map(|s| match s {
            brep::Shell::Faces { faces, .. } => faces.iter().find(|o| o.face as usize == face).map(|o| o.forward),
            _ => None,
        })
        .ok_or_else(|| invalid(format!("face {face} of the tool body lies in no shell")))?;
    // The surface's own parameter box, unbounded where it is.
    let dom = SurfaceEval::new(&f.surface).map_err(|e| invalid(format!("the split tool's surface: {e}")))?.domain();
    let range = |k: usize| dom.range[k].map_or(ALL, |(a, b)| [a, b]);
    Ok(Patch { surface: f.surface.clone(), domain: [range(0), range(1)], flip: !forward })
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
    let closed = (first.start() - last.end()).len() <= 1e-9 * (1.0 + first.start().len());
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

/// Which side of a tool a point is on.
pub(crate) fn side_of_point(tool: &SplitTool, near: &Body, p: Vec3) -> Result<f64> {
    Ok(ToolSide::of(tool, near)?.at(p))
}

/// How many fields a tool is (a sketch tool: one per curve).
pub(crate) fn field_count(tool: &SplitTool) -> Result<usize> {
    Ok(match tool {
        SplitTool::Curves { curves, .. } => curves.len(),
        _ => 1,
    })
}

/// A plane or circular cylinder behind a face's surface, facing as the face does.
pub(crate) enum Analytic {
    Plane { n: Vec3 },
    Cylinder { o: Vec3, axis: UnitVec3, r: f64, out: f64 },
}

/// The plane or circular cylinder face `face` of `body` lies on (a B-spline whose control net is
/// flat is a plane), with the face's orientation; none for other surfaces.
pub(crate) fn analytic_face(body: &Body, face: usize) -> Result<Option<Analytic>> {
    let p = face_patch(body, face)?;
    let sign = if p.flip { -1.0 } else { 1.0 };
    let flat = |pts: &[Vec3]| -> Option<Vec3> {
        let o = *pts.first()?;
        let size = pts.iter().map(|q| q.dist(o)).fold(0.0, f64::max);
        let n = pts.iter().flat_map(|a| pts.iter().map(move |b| (*a - o).cross(*b - o))).max_by(|a, b| a.len().total_cmp(&b.len()))?.normalized()?;
        pts.iter().all(|q| (*q - o).dot(n).abs() <= size * 1e-9).then_some(n)
    };
    Ok(match &p.surface {
        Surface::Plane { u_axis, v_axis, .. } => v3(*u_axis).cross(v3(*v_axis)).normalized().map(|n| Analytic::Plane { n: n * sign }),
        Surface::PlaneThrough { origin, u_point, v_point } => {
            (v3(*u_point) - v3(*origin)).cross(v3(*v_point) - v3(*origin)).normalized().map(|n| Analytic::Plane { n: n * sign })
        }
        Surface::Cylinder { frame, radius } => {
            v3(frame.x_axis).cross(v3(frame.y_axis)).unit().map(|axis| Analytic::Cylinder { o: v3(frame.origin), axis, r: *radius, out: sign })
        }
        Surface::Spline { points, .. } => {
            let pts: Vec<Vec3> = points.iter().flatten().map(|q| v3(*q)).collect();
            // The net's normal, turned to the surface's own.
            let m = Measured::new(&p)?;
            flat(&pts).and_then(|n| {
                let (_, su, sv) = m.eval.derivs(p.domain[0][0], p.domain[1][0])?;
                let own = su.cross(sv);
                Some(Analytic::Plane { n: if own.dot(n) < 0.0 { -n } else { n } * sign })
            })
        }
        _ => None,
    })
}

/// Which side of a tool each face of `body` lies on (in face order): the field at the centre of
/// the face's triangle where it is largest, so well inside the face.
pub(crate) fn face_sides(body: &Body, tool: &SplitTool) -> Result<Vec<f64>> {
    let side = ToolSide::of(tool, body)?;
    let mesh = body.mesh(&crate::MeshTolerance::chord((body.size() * 2e-3).max(1e-4)))?;
    let mut best = vec![0.0f64; body.face_count()];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, b, c]) = mesh.tri(t) else { continue };
        let x = side.at((a + b + c) * (1.0 / 3.0));
        if let Some(slot) = best.get_mut(*f as usize)
            && x.is_finite()
            && x.abs() > slot.abs()
        {
            *slot = x;
        }
    }
    Ok(best)
}

/// The regions of a body cut by a tool: for each face (in face order), the region it lies in,
/// and how many there are. Faces are in one region when they meet at an edge the tool does not
/// run along; an edge along the tool (a split line) parts them.
pub(crate) fn regions(split: &Body, tool: &SplitTool) -> Result<(Vec<usize>, usize)> {
    let record = crate::native::brep_record(split)?;
    let fields: Vec<ToolSide> = cutters(tool, &reach_of(split))?
        .iter()
        .map(|g| g.iter().map(Measured::new).collect::<Result<Vec<_>>>().map(ToolSide))
        .collect::<Result<_>>()?;
    let tol = (split.size() * 2e-3).max(1e-4);
    let along = |e: &brep::Edge| -> bool {
        let Ok(c) = CurveEval::new(&e.curve) else { return false };
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

/// The record's position of SMLib's face `i` (identity when `order` is empty).
fn canonical(order: &[usize], i: usize) -> Option<usize> {
    if order.is_empty() { Some(i) } else { order.get(i).copied() }
}

/// A result checked by SMLib and brought back with its history over `[body]`.
fn brought_back(name: &str, body: &Body, ob: &solidcraft_smlib::history::Order, r: &solidcraft_smlib::Brep) -> Result<(Body, History)> {
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
fn tagged(name: &str, body: &Body) -> Result<(solidcraft_smlib::Brep, solidcraft_smlib::history::Order)> {
    let (sb, ob) = crate::native::smlib_body_ordered(body)?;
    let ob = ob.ok_or_else(|| failed(name, "SMLib numbers the body's faces unlike its record"))?;
    Ok((sb.tagged(0).map_err(|e| failed(name, e))?, ob))
}

/// Split the given faces (indices in `body`'s face order) where the tool meets them. The solid
/// is unchanged. A listed face the tool misses is an error naming it. The result's history is
/// over `[body]`: each piece of a split face is Split from it, every other face Kept.
pub fn split_faces(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<Body> {
    split_faces_with_map(body, faces, tool).map(|(b, _)| b)
}

/// Like [`split_faces`], with the body face each face of the result came from (in face order).
pub fn split_faces_with_map(body: &Body, faces: &[usize], tool: &SplitTool) -> Result<(Body, Vec<usize>)> {
    let (b, map, made) = split(body, faces, tool)?;
    if let Some((missed, _)) = faces.iter().zip(&made).find(|(_, m)| **m == 0) {
        return Err(invalid(format!("the tool does not cross face {missed}")));
    }
    Ok((b, map))
}

/// Split where the tool meets the listed faces (missing some is fine; missing all is an error),
/// with the body face each face of the result came from.
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
        let (r, made) = sb.split_faces(&at, &refs).map_err(|e| failed(NAME, e))?.ok_or_else(missed)?;
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

/// The edges of `split` that no edge of the body was before (its split lines), as polylines
/// through their exact points.
fn new_lines(split: &Body, h: &History) -> Result<Vec<Vec<Vec3>>> {
    let record = crate::native::brep_record(split)?;
    let mut out = Vec::new();
    for (e, prov) in record.edges.iter().zip(&h.edges) {
        if prov.origins.iter().any(|o| matches!(o, EdgeOrigin::Edge { .. })) {
            continue;
        }
        let c = CurveEval::new(&e.curve).map_err(|err| failed("split face", err))?;
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
/// (extended), or one sketch line swept along its direction. Every part is closed with the piece
/// of the tool's surface inside the body; parts on the tool's back come first, each side's in
/// face order. A body the tool misses comes back whole. Each part's history is over `[body]`:
/// its faces the body's (or pieces of them), the section new.
pub fn split_body(body: &Body, tool: &SplitTool) -> Result<Vec<Body>> {
    const NAME: &str = "split body";
    body.require_brep(NAME)?;
    let reach = reach_of(body);
    let groups = cutters(tool, &reach)?;
    let [group] = groups.as_slice() else { return Err(invalid("a body is split by a plane or a face")) };
    let [patch] = group.as_slice() else { return Err(invalid("a body is split by a plane, a face or one sketch line")) };
    let counts = record::counts(&[body]);
    let whole = || crate::boolean_record::operand(body, 0, &counts);
    let Some(surface) = smlib_tools(std::slice::from_ref(group), &reach)?.pop() else { return Ok(vec![whole()]) };
    guard(NAME, || {
        let (sb, ob) = tagged(NAME, body)?;
        let Some((back, front)) = sb.split_body(&surface).map_err(|e| failed(NAME, e))? else { return Ok(vec![whole()]) };
        // SMLib's sides are the surface's; the tool's are the face's.
        let sides = if patch.flip { [front, back] } else { [back, front] };
        let (mut parts, mut cut) = (Vec::new(), false);
        for side in &sides {
            let (part, h) = brought_back(NAME, body, &ob, side)?;
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure};

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
        // Without the record the export joins the top's pieces again (as it does for pieces a
        // boolean left).
        assert_eq!(round_trip_faces(&s.clone().with_split_lines(None)), 6);
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
        // The half side face splits at z = 5: a quarter and three quarters of its area.
        let mut sides: Vec<f64> = s.faces(0.01).unwrap().iter().filter(|f| f.plane_normal.is_none()).map(|f| f.area).collect();
        sides.sort_by(f64::total_cmp);
        let half = std::f64::consts::PI * 10.0 * 20.0;
        assert_eq!(sides.len(), 3);
        assert!(rel(sides[0], half / 4.0) < 1e-3 && rel(sides[1], half * 0.75) < 1e-3 && rel(sides[2], half) < 1e-3, "{sides:?}");
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

    fn volumes(parts: &[Body]) -> Vec<f64> {
        let mut v: Vec<f64> = parts.iter().map(|b| measure(b).unwrap().volume).collect();
        v.sort_by(f64::total_cmp);
        v
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
        let record = crate::native::brep_record(&s).unwrap();
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
}
