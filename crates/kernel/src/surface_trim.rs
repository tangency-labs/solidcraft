//! Trim: a surface body cut where a tool meets it (a plane, another body's face, or sketch
//! curves swept onto it), keeping the side of a point or the regions points pick; and Mutual
//! Trim: two surfaces cut by each other, each keeping its picked side, extended to meet first
//! when one stops short of the other.

use solidcore_record::Vec3;
use std::collections::BTreeSet;

use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::splitface::{SplitTool, ToolSide};
use crate::surface_extend::{ExtendLimit, ExtendMode, ExtendOptions};
use crate::{KernelError, Result};

/// Most points one trim picks with.
const MAX_PICKS: usize = 10_000;

/// What a trim keeps.
#[derive(Clone, Copy, Debug)]
pub enum TrimKeep<'a> {
    /// Every piece on the side of the tool this point is on. A tool of several sketch curves
    /// has no one side: the region the point picks is kept.
    Side(Vec3),
    /// The regions the points pick (a region: the pieces that hang together once the tool has
    /// cut the body, each picked by the piece nearest a point).
    Regions(&'a [Vec3]),
}

impl TrimKeep<'_> {
    fn points(&self) -> &[Vec3] {
        match self {
            TrimKeep::Side(p) => std::slice::from_ref(p),
            TrimKeep::Regions(at) => at,
        }
    }

    fn check(&self) -> Result<()> {
        let at = self.points();
        if at.is_empty() {
            return Err(KernelError::Invalid("pick what to keep".into()));
        }
        if at.len() > MAX_PICKS {
            return Err(KernelError::Invalid("too many points to keep".into()));
        }
        if at.iter().any(|p| !p.is_finite()) {
            return Err(KernelError::Invalid("a point to keep is not finite".into()));
        }
        Ok(())
    }
}

/// A surface body trimmed by a tool: its faces are cut where the tool meets them and the
/// pieces on the side of `keep` are kept. The result's history is over `[body]`.
pub fn trim(body: &Body, tool: &SplitTool, keep: Vec3) -> Result<Body> {
    trim_keep(body, tool, TrimKeep::Side(keep))
}

/// A surface body trimmed by a tool, keeping what `keep` says. Each kept face is the face it is
/// a piece of (its history is over `[body]`).
///
/// The body is split where the tool meets it ([`crate::splitface::split_any`]), each piece's side
/// read at a point inside it ([`face_sides`]), and the pieces not kept are taken away ([`crate::remove_faces`]); both carry their history, so the kept
/// faces are the body's own, cut.
pub fn trim_keep(body: &Body, tool: &SplitTool, keep: TrimKeep) -> Result<Body> {
    body.require_brep("trim")?;
    keep.check()?;
    let all: Vec<usize> = (0..body.face_count()).collect();
    let (split, _) = crate::splitface::split_any(body, &all, tool)?;
    let kept: Vec<bool> = match keep {
        TrimKeep::Side(p) if crate::splitface::field_count(tool)? == 1 => {
            let side = ToolSide::of(tool, body)?;
            let want = side.at(p);
            if !want.is_finite() {
                return Err(KernelError::Invalid("the side of the tool the point is on can't be told".into()));
            }
            face_sides(&split, &side)?.into_iter().map(|x| (x >= 0.0) == (want >= 0.0)).collect()
        }
        _ => {
            let picked = nearest_faces(&split, keep.points())?;
            let (region, count) = crate::splitface::regions(&split, tool)?;
            let mut wanted = vec![false; count];
            for f in &picked {
                if let Some(w) = region.get(*f).and_then(|r| wanted.get_mut(*r)) {
                    *w = true;
                }
            }
            region.iter().map(|r| wanted.get(*r).copied().unwrap_or(false)).collect()
        }
    };
    if !kept.iter().any(|k| *k) {
        return Err(KernelError::Invalid("nothing is left on that side of the tool".into()));
    }
    let dropped: Vec<usize> = kept.iter().enumerate().filter(|(_, k)| !**k).map(|(i, _)| i).collect();
    let trimmed = if dropped.is_empty() { split.clone() } else { crate::remove_faces(&split, &dropped)? };
    // The kept pieces are the split's own; its history carries them back to `body`.
    Ok(crate::record::after(trimmed, &split, &crate::record::counts(&[body])))
}

/// Most bodies one [`stop_at_bodies`] cuts with.
const MAX_STOP_BODIES: usize = 10_000;

/// Where along each cut edge [`stop_at_bodies`] checks which side of it a kept piece lies on,
/// as fractions of the edge's interval.
const SIDE_SAMPLES: [f64; 7] = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875];

/// The walls of a surface extrude (`walls`, swept along `along`) cut where they first run into
/// `bodies`: each body's faces exactly as they are, bounded where they end and never extended
/// (SMLib's imprint of each body in turn). The pieces kept are those joined to the walls'
/// start, without crossing an edge a body cut: `starts` are corners of the walls at their
/// start, one on each chain. Mesh bodies and bodies apart from the walls cut nothing.
///
/// Each kept piece must lie on the start side of every cut edge that bounds it, so it stops
/// where it first meets a body all along. A piece that runs on past a body beside it (a body
/// covering only part of a wall, the wall running past its side) is refused: keeping it would
/// keep wall above that body.
///
/// The second value is whether any body cut the walls; where none did they come back whole.
/// Each kept face is the face of `walls` it is a piece of: the history is over `[walls]`, as
/// [`trim_keep`]'s.
pub fn stop_at_bodies(walls: &Body, bodies: &[&Body], starts: &[Vec3], along: Vec3) -> Result<(Body, bool)> {
    const NAME: &str = "SMLib imprint of the bodies in the walls' way";
    let _span = solidcraft_profile::span("kernel", "stop_at_bodies");
    walls.require_brep("trim")?;
    TrimKeep::Regions(starts).check()?;
    if bodies.len() > MAX_STOP_BODIES {
        return Err(KernelError::Invalid(format!("too many bodies in the walls' way (at most {MAX_STOP_BODIES})")));
    }
    let along = along.normalized().filter(|a| a.is_finite()).ok_or_else(|| KernelError::Invalid("the walls' direction has no length".into()))?;
    let failed = |e: solidcraft_smlib::Error| KernelError::smlib(NAME, e);
    let lost = |what: &str| KernelError::Failed(format!("{NAME}: {what}"));
    let (mut cur, order) = crate::native::smlib_body_ordered(walls)?;
    let order = order.ok_or_else(|| lost("SMLib numbers the walls' faces unlike their record"))?;
    let [faces, edges, _] = cur.counts().map_err(failed)?;
    // Each SMLib face of `cur`: the face of `walls` it is a piece of. Each SMLib edge: whether
    // a body's cut made it (or a piece of it).
    let canonical = |list: &[usize], i: usize| if list.is_empty() { Some(i) } else { list.get(i).copied() };
    let mut face_of: Vec<usize> =
        (0..faces).map(|i| canonical(&order.faces, i)).collect::<Option<_>>().ok_or_else(|| lost("a face is not numbered"))?;
    let mut cut = vec![false; edges];
    let mut met = false;
    for b in bodies {
        if b.is_mesh() || crate::apart(walls, b)? {
            continue;
        }
        let tool = b.smlib()?.tagged(1).map_err(failed)?;
        let Some(r) = cur.tagged(0).map_err(failed)?.imprint(&tool).map_err(failed)? else { continue };
        let raw = r.history().map_err(failed)?;
        face_of = raw
            .faces
            .iter()
            .map(|f| f.origins.iter().find(|(k, _)| *k == 0).and_then(|(_, i)| face_of.get(*i).copied()))
            .collect::<Option<_>>()
            .ok_or_else(|| lost("a piece comes from no face of the walls"))?;
        // New where the faces met; a piece of a cut edge, or of the body's own edge, is a cut too.
        cut = raw
            .edges
            .iter()
            .map(|e| e.origins.is_empty() || e.origins.iter().any(|&(k, i)| k != 0 || cut.get(i).copied().unwrap_or(false)))
            .collect();
        cur = r;
        met = true;
    }
    if !met {
        return Ok((walls.clone(), false));
    }
    let (split, order) = crate::native::body_from_smlib_ordered(&cur)?;
    let order = order.ok_or_else(|| lost("the imprinted walls cannot be numbered"))?;
    let split = split.with_color(walls.color());
    let rec = split.record();
    let n = rec.faces.len();
    // In the record's order: the walls' face each face is a piece of, and the edges cut.
    let mut from = vec![None; n];
    for (i, f) in face_of.iter().enumerate() {
        let slot = canonical(&order.faces, i).and_then(|c| from.get_mut(c)).ok_or_else(|| lost("a face lies outside the walls"))?;
        *slot = Some(*f);
    }
    let from: Vec<usize> = from.into_iter().collect::<Option<_>>().ok_or_else(|| lost("a face of the imprint is not numbered"))?;
    let mut made = vec![false; rec.edges.len()];
    for i in cut.iter().enumerate().filter(|(_, c)| **c).map(|(i, _)| i) {
        if let Some(slot) = canonical(&order.edges, i).and_then(|k| made.get_mut(k)) {
            *slot = true;
        }
    }
    for p in &order.pieces {
        if cut.get(p.edge).copied().unwrap_or(false)
            && let Some(slot) = made.get_mut(p.canonical)
        {
            *slot = true;
        }
    }
    // Regions: the faces that hang together across edges no cut made.
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &[usize], mut i: usize) -> usize {
        while let Some(&p) = parent.get(i).filter(|p| **p != i) {
            i = p;
        }
        i
    }
    for (e, edge) in rec.edges.iter().enumerate() {
        if made.get(e).copied().unwrap_or(true) {
            continue;
        }
        let faces: Vec<usize> = edge.uses.iter().map(|u| u.face as usize).collect();
        for pair in faces.windows(2) {
            let (a, b) = (root(&parent, pair[0]), root(&parent, pair[1]));
            if let Some(slot) = parent.get_mut(a.max(b)) {
                *slot = a.min(b);
            }
        }
    }
    // The regions holding a start: those of the faces whose edges end at a start corner.
    let accept = rec.edges.iter().map(|e| e.tolerance).fold(rec.tolerance, f64::max).max(split.size() * 1e-9) * 10.0;
    let at_start = |v: u32| rec.vertices.get(v as usize).is_some_and(|v| starts.iter().any(|s| crate::body::v3(v.point).dist(*s) <= accept));
    let mut wanted = BTreeSet::new();
    for edge in rec.edges.iter().filter(|e| e.vertices.iter().any(|v| at_start(*v))) {
        wanted.extend(edge.uses.iter().map(|u| root(&parent, u.face as usize)));
    }
    if wanted.is_empty() {
        return Err(lost("no piece of the walls holds their start"));
    }
    let kept: Vec<bool> = (0..n).map(|f| wanted.contains(&root(&parent, f))).collect();
    // A kept piece lies on the start side of each cut edge bounding it: into the face, across
    // the edge, it runs back against `along`, never on with it.
    for (f, face) in rec.faces.iter().enumerate().filter(|(f, _)| kept.get(*f).copied().unwrap_or(false)) {
        let surface = SurfaceEval::new(face.parameters()).map_err(|e| lost(&format!("a face's surface: {e}")))?;
        let uses = face.loops.iter().flat_map(|l| match l {
            solidcore_record::brep::Loop::Edges { edges } => edges.as_slice(),
            solidcore_record::brep::Loop::Vertex { .. } => &[],
        });
        for u in uses.filter(|u| made.get(u.edge as usize).copied().unwrap_or(false)) {
            let Some(edge) = rec.edges.get(u.edge as usize) else { continue };
            let curve = CurveEval::new(edge.parameters()).map_err(|e| lost(&format!("a cut edge's curve: {e}")))?;
            let [t0, t1] = edge.interval;
            for s in SIDE_SAMPLES {
                let t = t0 + (t1 - t0) * s;
                let (Some(p), Some(d)) = (curve.point(t), crate::blend::curve_derivative(edge.parameters(), t)) else { continue };
                let Some([su, sv]) = surface.project(p, None, accept).and_then(|uv| surface.derivs(uv[0], uv[1])).map(|(_, su, sv)| [su, sv]) else {
                    continue;
                };
                // The face lies to the left of its loop seen from its surface's natural normal.
                let ahead = if u.forward { d } else { d * -1.0 };
                let Some(into) = su.cross(sv).cross(ahead).normalized() else { continue };
                if into.dot(along) > 1e-6 {
                    return Err(KernelError::Invalid(
                        "not supported yet: part of a wall runs on past the side of a body in its way (the body covers only part of the wall)".into(),
                    ));
                }
            }
        }
    }
    let dropped: Vec<usize> = kept.iter().enumerate().filter(|(_, k)| !**k).map(|(i, _)| i).collect();
    let faces: Vec<_> = from.iter().map(|f| crate::record::from_face(0, *f, crate::history::Change::Kept)).collect();
    let split = crate::record::attach(split, &crate::record::counts(&[walls]), Some(faces), &[]);
    let trimmed = if dropped.is_empty() { split.clone() } else { crate::remove_faces(&split, &dropped)? };
    Ok((crate::record::after(trimmed, &split, &crate::record::counts(&[walls])), true))
}

/// The side of the tool each face of `body` (in its face order) lies on: the tool's field at a
/// point inside the face, on its surface and inside its trims (SMLib's
/// `CalculateAnInternalPoint`). Once the tool has cut the body each piece lies on one side, so
/// one such point says which.
fn face_sides(body: &Body, side: &ToolSide) -> Result<Vec<f64>> {
    let (sm, order) = crate::native::smlib_body_ordered(body)?;
    let order = order.ok_or_else(|| KernelError::Failed("trim: SMLib numbers the pieces' faces differently from their record".into()))?;
    let points = sm.face_points().map_err(|e| KernelError::smlib("SMLib trim: a point inside each piece", e))?;
    let mut sides = vec![f64::NAN; points.len()];
    for (i, p) in points.iter().enumerate() {
        let face = if order.faces.is_empty() { i } else { order.faces.get(i).copied().unwrap_or(usize::MAX) };
        let slot = sides.get_mut(face).ok_or_else(|| KernelError::Failed("trim: a piece's face lies outside the body".into()))?;
        *slot = side.at(Vec3::new(p[0], p[1], p[2]));
    }
    if sides.iter().any(|x| !x.is_finite()) {
        return Err(KernelError::Failed("trim: the side of a piece can't be told".into()));
    }
    Ok(sides)
}

/// For each point, the face of `body` nearest it (over one tessellation).
fn nearest_faces(body: &Body, at: &[Vec3]) -> Result<Vec<usize>> {
    let m = body.mesh(&crate::MeshTolerance::chord(body.size() * 2e-3))?;
    at.iter()
        .map(|p| {
            let mut best: Option<(f64, u32)> = None;
            for (t, f) in m.triangles.iter().zip(&m.tri_face) {
                let Some([a, b, c]) = m.tri(t) else { continue };
                let d = ((a + b + c) / 3.0).dist(*p).min(a.dist(*p)).min(b.dist(*p)).min(c.dist(*p));
                if best.is_none_or(|(x, _)| d < x) {
                    best = Some((d, *f));
                }
            }
            best.map(|(_, f)| f as usize).ok_or_else(|| KernelError::Invalid("no face there".into()))
        })
        .collect()
}

/// Two surface bodies trimmed by each other: `a` cut where `b`'s surface (extended) meets it,
/// keeping what `keep_a` says, and `b` by `a`'s, keeping `keep_b`. A surface that stops short of
/// the other is extended to meet it when `extend` is set (naturally, its nearest boundary edge
/// up to the other's surface), else that is an error; one that ends on the other is kept whole.
/// Each body must be one face (its surface is the other's tool). The results' histories are over
/// `[a]` and `[b]`.
pub fn mutual_trim(a: &Body, b: &Body, keep_a: TrimKeep, keep_b: TrimKeep, extend: bool) -> Result<(Body, Body)> {
    for (x, name) in [(a, "first"), (b, "second")] {
        x.require_brep("trim")?;
        if !x.is_surface() {
            return Err(KernelError::Invalid(format!("the {name} body is a solid: a mutual trim takes two surfaces")));
        }
        if x.face_count() != 1 {
            return Err(KernelError::Invalid(format!(
                "not supported yet: a mutual trim of a surface of more than one face (the {name} body has {}; trim it by one face instead)",
                x.face_count()
            )));
        }
    }
    keep_a.check()?;
    keep_b.check()?;
    let ta = cut_by(a, &SplitTool::Face { body: b, face: 0 }, keep_a, extend, "first")?;
    let tb = cut_by(b, &SplitTool::Face { body: a, face: 0 }, keep_b, extend, "second")?;
    Ok((ta, tb))
}

/// `x` trimmed by `tool` where the tool crosses it; whole where it ends on the tool; extended to
/// the tool first (when `extend`) where it stops short of it.
fn cut_by(x: &Body, tool: &SplitTool, keep: TrimKeep, extend: bool, name: &str) -> Result<Body> {
    let side = ToolSide::of(tool, x)?;
    let size = x.size();
    let touch = size * 1e-6 + 1e-9;
    let m = x.mesh(&crate::MeshTolerance::chord(size * 2e-3))?;
    let (mut lo, mut hi, mut near) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY);
    for p in &m.positions {
        let v = side.at(*p);
        if !v.is_finite() {
            return Err(KernelError::Failed("the other surface cannot be evaluated over this one".into()));
        }
        lo = lo.min(v);
        hi = hi.max(v);
        near = near.min(v.abs());
    }
    if lo < -touch && hi > touch {
        return trim_keep(x, tool, keep);
    }
    if near <= touch {
        return Ok(crate::boolean_record::operand(x, 0, &crate::record::counts(&[x])));
    }
    if !extend {
        return Err(KernelError::Invalid(format!("the {name} surface does not reach the other one: extend it to meet")));
    }
    let edge = nearest_boundary_edge(x, &side)?;
    let opts = ExtendOptions { mode: ExtendMode::Natural, limit: ExtendLimit::UpTo(tool), neighbours: true };
    crate::extend_surface(x, &[edge], &opts)
}

/// A point in the middle of the boundary edge of `x` nearest the tool: of the record's edges
/// that one face uses once, the one whose middle (half-way along its interval) is nearest.
fn nearest_boundary_edge(x: &Body, side: &ToolSide) -> Result<Vec3> {
    let record = crate::native::brep_record(x)?;
    let mut best: Option<(f64, Vec3)> = None;
    for e in record.edges.iter().filter(|e| e.uses.len() == 1) {
        let curve = CurveEval::new(e.parameters()).map_err(|err| KernelError::Failed(format!("trim: an edge's curve: {err}")))?;
        let Some(mid) = curve.point(0.5 * (e.interval[0] + e.interval[1])) else { continue };
        let d = side.at(mid).abs();
        if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, mid));
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| KernelError::Invalid("the surface has no boundary edge to extend".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{Change, FaceOrigin};
    use crate::{extrude_sheet, measure, patch_region, stitch};
    use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
    use std::f64::consts::PI;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-12)
    }

    fn rect(plane: &Plane, u0: f64, v0: f64, u1: f64, v1: f64) -> Body {
        let outer = Loop2::polygon(&[Vec2::new(u0, v0), Vec2::new(u1, v0), Vec2::new(u1, v1), Vec2::new(u0, v1)]);
        patch_region(plane, &Region2 { outer, holes: vec![] }).unwrap()
    }

    fn area(b: &Body) -> f64 {
        measure(b).unwrap().area
    }

    /// A U-shaped sheet (legs x = 0 and x = 10, 10 high, a base along y = 0, 10 deep in z) cut
    /// by the plane y = 5: one side keeps both legs' tops, one region pick only one of them, two
    /// picks both again.
    #[test]
    fn regions_keep_what_the_points_pick() {
        let chain = vec![
            Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, 0.0) },
            Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) },
            Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 10.0) },
        ];
        let u = extrude_sheet(&Plane::XY, &[chain], 0.0, 10.0).unwrap();
        let cut = SplitTool::Plane(Plane::new(Vec3::new(0.0, 5.0, 0.0), Vec3::X, Vec3::Z).unwrap());
        let side = trim(&u, &cut, Vec3::new(0.0, 8.0, 5.0)).unwrap();
        assert!(rel(area(&side), 100.0) < 1e-9, "{}", area(&side));
        let one = trim_keep(&u, &cut, TrimKeep::Regions(&[Vec3::new(0.0, 8.0, 5.0)])).unwrap();
        assert!(rel(area(&one), 50.0) < 1e-9, "{}", area(&one));
        let both = trim_keep(&u, &cut, TrimKeep::Regions(&[Vec3::new(0.0, 8.0, 5.0), Vec3::new(10.0, 9.0, 2.0)])).unwrap();
        assert!(rel(area(&both), 100.0) < 1e-9, "{}", area(&both));
        // The base side is one region: its three faces hang together.
        let base = trim_keep(&u, &cut, TrimKeep::Regions(&[Vec3::new(5.0, 0.0, 5.0)])).unwrap();
        assert!(rel(area(&base), 200.0) < 1e-9 && base.face_count() == 3, "{}", area(&base));
        // Each kept face is a piece of the face it was cut from.
        let h = one.history().unwrap();
        assert_eq!(h.operands, 1);
        assert!(
            h.faces.iter().all(|f| f.change == Change::Modified && matches!(f.origins.as_slice(), [FaceOrigin::Face { operand: 0, .. }])),
            "{h:?}"
        );
    }

    /// A 10 × 10 square cut by two sketch lines (x = 3 and x = 7): a side is no answer for two
    /// curves, so a point picks its region; two picks keep the two outer strips.
    #[test]
    fn several_curves_cut_regions() {
        let sq = rect(&Plane::XY, 0.0, 0.0, 10.0, 10.0);
        let lines = vec![
            vec![Seg2::Line { a: Vec2::new(3.0, -5.0), b: Vec2::new(3.0, 15.0) }],
            vec![Seg2::Line { a: Vec2::new(7.0, -5.0), b: Vec2::new(7.0, 15.0) }],
        ];
        let tool = SplitTool::Curves { plane: Plane { origin: Vec3::new(0.0, 0.0, 5.0), ..Plane::XY }, curves: lines, along: None };
        let mid = trim(&sq, &tool, Vec3::new(5.0, 5.0, 0.0)).unwrap();
        assert!(rel(area(&mid), 40.0) < 1e-9, "{}", area(&mid));
        let outer = trim_keep(&sq, &tool, TrimKeep::Regions(&[Vec3::new(1.0, 5.0, 0.0), Vec3::new(9.0, 5.0, 0.0)])).unwrap();
        assert!(rel(area(&outer), 60.0) < 1e-9 && outer.face_count() == 2, "{}", area(&outer));
    }

    /// A line sketched 10 above a 20 × 20 sheet at x = 5, swept along (1, 0, −1): it meets the
    /// sheet at x = 15, so the side of x = 0 keeps 15 × 20. A direction along the sketch plane
    /// is refused.
    #[test]
    fn curves_swept_aslant() {
        let sq = rect(&Plane::XY, 0.0, 0.0, 20.0, 20.0);
        let plane = Plane { origin: Vec3::new(0.0, 0.0, 10.0), ..Plane::XY };
        let curves = vec![vec![Seg2::Line { a: Vec2::new(5.0, -5.0), b: Vec2::new(5.0, 25.0) }]];
        let tool = SplitTool::Curves { plane, curves: curves.clone(), along: Some(Vec3::new(1.0, 0.0, -1.0)) };
        let t = trim(&sq, &tool, Vec3::new(1.0, 5.0, 0.0)).unwrap();
        assert!(rel(area(&t), 300.0) < 1e-9, "{}", area(&t));
        let flat = SplitTool::Curves { plane, curves, along: Some(Vec3::X) };
        assert!(trim(&sq, &flat, Vec3::new(1.0, 5.0, 0.0)).is_err());
    }

    /// The slot |x| ≤ 3 (a straight run z = 12…28 and round ends) sketched in front of a half
    /// cylinder (radius 10 about Z, y ≥ 0, 40 high) and swept along the sketch's normal: the
    /// piece inside it is ∫∫ R / √(R² − x²) dx dz over the slot (the straight run 16 · 2R·asin(3/R),
    /// the two round ends one disc), the piece outside the rest of 10π · 40.
    #[test]
    fn a_slot_projected_onto_a_cylinder() {
        let r: f64 = 10.0;
        let half = extrude_sheet(&Plane::XY, &[vec![Seg2::Arc { center: Vec2::ZERO, radius: r, start: 0.0, sweep: PI }]], 0.0, 40.0).unwrap();
        let a: f64 = 3.0;
        let slot = vec![
            Seg2::Line { a: Vec2::new(a, 12.0), b: Vec2::new(a, 28.0) },
            Seg2::Arc { center: Vec2::new(0.0, 28.0), radius: a, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(-a, 28.0), b: Vec2::new(-a, 12.0) },
            Seg2::Arc { center: Vec2::new(0.0, 12.0), radius: a, start: PI, sweep: PI },
        ];
        let front = Plane { y: Vec3::Z, ..Plane::XZ };
        let tool = SplitTool::Curves { plane: front, curves: vec![slot], along: None };
        // The round ends: x = a·sin φ, the disc's height 2a·cos φ, dx = a·cos φ dφ (Simpson).
        let n = 4000;
        let f = |phi: f64| 2.0 * a * a * phi.cos().powi(2) * r / (r * r - (a * phi.sin()).powi(2)).sqrt();
        let h = PI / n as f64;
        let ends = (0..=n)
            .map(|k| {
                let w = if k == 0 || k == n {
                    1.0
                } else if k % 2 == 1 {
                    4.0
                } else {
                    2.0
                };
                w * f(-PI / 2.0 + h * k as f64)
            })
            .sum::<f64>()
            * h
            / 3.0;
        let inside = 16.0 * 2.0 * r * (a / r).asin() + ends;
        let on = Vec3::new(0.0, r, 20.0);
        let kept = trim(&half, &tool, on).unwrap();
        eprintln!("slot on the cylinder: {} measured, {inside} analytic", area(&kept));
        // (Measured on meshes.)
        assert!(rel(area(&kept), inside) < 1e-3, "{} vs {inside}", area(&kept));
        let rest = trim(&half, &tool, Vec3::new(r, 0.0, 20.0)).unwrap();
        let all = PI * r * 40.0;
        assert!(rel(area(&rest), all - inside) < 1e-3, "{} vs {}", area(&rest), all - inside);
        let picked = trim_keep(&half, &tool, TrimKeep::Regions(&[on])).unwrap();
        assert!(rel(area(&picked), area(&kept)) < 1e-9);
    }

    /// Two 20 × 10 sheets crossing at right angles (z = 0 and x = 0, both y = 0…10) trimmed by
    /// each other, each keeping its x > 0 / z > 0 half, and stitched: an L of two faces sharing
    /// their edge, 200 in area.
    #[test]
    fn crossing_sheets_trim_each_other_into_an_l() {
        let a = rect(&Plane::XY, -10.0, 0.0, 10.0, 10.0);
        let yz = Plane::new(Vec3::ZERO, Vec3::Y, Vec3::Z).unwrap();
        let b = rect(&yz, 0.0, -10.0, 10.0, 10.0);
        let (ta, tb) = mutual_trim(&a, &b, TrimKeep::Side(Vec3::new(5.0, 5.0, 0.0)), TrimKeep::Side(Vec3::new(0.0, 5.0, 5.0)), false).unwrap();
        assert!(rel(area(&ta), 100.0) < 1e-9 && rel(area(&tb), 100.0) < 1e-9);
        let l = stitch(&[&ta, &tb], 0.0).unwrap();
        assert_eq!((l.face_count(), l.edge_count()), (2, 7), "one shared edge");
        assert!(rel(area(&l), 200.0) < 1e-9);
        assert!(l.is_surface());
        let bb = measure(&l).unwrap().bbox;
        assert!(bb.min.x.abs() < 1e-9 && bb.min.z.abs() < 1e-9 && (bb.max.x - 10.0).abs() < 1e-9 && (bb.max.z - 10.0).abs() < 1e-9, "{bb:?}");
        // The pieces are the sheets' faces.
        for t in [&ta, &tb] {
            let h = t.history().unwrap();
            assert!(matches!(h.faces[0].origins.as_slice(), [FaceOrigin::Face { operand: 0, face: 0 }]) && h.faces[0].change == Change::Modified);
        }
    }

    /// A sheet that stops 3 short of the other (x = 0, z = 3…10) is refused until extended to
    /// meet it; extended, the two still make the L.
    #[test]
    fn a_short_sheet_is_extended_to_meet() {
        let a = rect(&Plane::XY, -10.0, 0.0, 10.0, 10.0);
        let yz = Plane::new(Vec3::ZERO, Vec3::Y, Vec3::Z).unwrap();
        let b = rect(&yz, 0.0, 3.0, 10.0, 10.0);
        let (ka, kb) = (TrimKeep::Side(Vec3::new(5.0, 5.0, 0.0)), TrimKeep::Side(Vec3::new(0.0, 5.0, 5.0)));
        let e = mutual_trim(&a, &b, ka, kb, false).unwrap_err().to_string();
        assert!(e.contains("extend"), "{e}");
        let (ta, tb) = mutual_trim(&a, &b, ka, kb, true).unwrap();
        assert!(rel(area(&ta), 100.0) < 1e-9 && rel(area(&tb), 100.0) < 1e-6, "{} {}", area(&ta), area(&tb));
        let l = stitch(&[&ta, &tb], 0.0).unwrap();
        assert_eq!((l.face_count(), l.edge_count()), (2, 7));
    }

    #[test]
    fn hostile_trims_are_refused() {
        let a = rect(&Plane::XY, 0.0, 0.0, 10.0, 10.0);
        let cut = SplitTool::Plane(Plane::new(Vec3::new(3.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap());
        assert!(trim_keep(&a, &cut, TrimKeep::Regions(&[])).is_err());
        assert!(trim_keep(&a, &cut, TrimKeep::Regions(&[Vec3::new(f64::NAN, 0.0, 0.0)])).is_err());
        assert!(trim(&a, &cut, Vec3::new(f64::INFINITY, 0.0, 0.0)).is_err());
        let many = vec![Vec3::ZERO; MAX_PICKS + 1];
        assert!(trim_keep(&a, &cut, TrimKeep::Regions(&many)).is_err());
        let solid = crate::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        assert!(mutual_trim(&a, &solid, TrimKeep::Side(Vec3::ZERO), TrimKeep::Side(Vec3::ZERO), true).is_err());
        let chain =
            vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(5.0, 0.0) }, Seg2::Line { a: Vec2::new(5.0, 0.0), b: Vec2::new(5.0, 5.0) }];
        let two = extrude_sheet(&Plane::XY, &[chain], 0.0, 5.0).unwrap();
        let e = mutual_trim(&a, &two, TrimKeep::Side(Vec3::ZERO), TrimKeep::Side(Vec3::ZERO), true).unwrap_err().to_string();
        assert!(e.contains("not supported yet"), "{e}");
    }

    /// A wall at y = 0 from x = 0 to 10, swept from z = 0 to 50, stops where it first runs into
    /// a body, each body's faces as they are (#16537, Surface Extrude To Next). The areas are
    /// worked out from the shapes: 10 wide, up to the first body's underside.
    #[test]
    fn walls_stop_at_the_first_body_in_their_way() {
        let line = || vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let wall = extrude_sheet(&Plane::XY, &[line()], 0.0, 50.0).unwrap();
        let starts = [Vec3::ZERO];
        let up = Vec3::new(0.0, 0.0, 1.0);
        let top = |b: &Body| b.exact_bounds().unwrap().max.z;
        // A slab across the wall from z = 20 to 22: 10 x 20 is left, ending at z = 20.
        let slab = crate::box_solid(Vec3::new(-1.0, -1.0, 20.0), Vec3::new(11.0, 1.0, 22.0)).unwrap();
        let (kept, met) = stop_at_bodies(&wall, &[&slab], &starts, up).unwrap();
        assert!(met && rel(area(&kept), 200.0) < 1e-9 && (top(&kept) - 20.0).abs() < 1e-9, "{} {}", area(&kept), top(&kept));
        // Each kept face is a piece of the wall's one face.
        let h = kept.history().unwrap();
        assert!(h.faces.iter().all(|f| f.origins == [FaceOrigin::Face { operand: 0, face: 0 }]), "{h:?}");
        // A surface body across it at z = 8 is nearer, whichever body comes first: 10 x 8.
        let sheet = rect(&Plane { origin: Vec3::new(0.0, 0.0, 8.0), ..Plane::XY }, -1.0, -1.0, 11.0, 1.0);
        for bodies in [[&slab, &sheet], [&sheet, &slab]] {
            let (kept, met) = stop_at_bodies(&wall, &bodies, &starts, up).unwrap();
            assert!(met && rel(area(&kept), 80.0) < 1e-9 && (top(&kept) - 8.0).abs() < 1e-9, "{}", area(&kept));
        }
        // Swept the other way, down to z = -50, the slab and the sheet are behind it: a slab
        // from z = -32 to -30 stops it at z = -30, 10 x 30.
        let down = extrude_sheet(&Plane::XY, &[line()], -50.0, 0.0).unwrap();
        let under = crate::box_solid(Vec3::new(-1.0, -1.0, -32.0), Vec3::new(11.0, 1.0, -30.0)).unwrap();
        let (kept, met) = stop_at_bodies(&down, &[&slab, &sheet, &under], &starts, up * -1.0).unwrap();
        assert!(met && rel(area(&kept), 300.0) < 1e-9, "{}", area(&kept));
        // A body out of the way cuts nothing: the walls come back whole.
        let aside = crate::box_solid(Vec3::new(40.0, 40.0, 0.0), Vec3::new(50.0, 50.0, 10.0)).unwrap();
        let (kept, met) = stop_at_bodies(&wall, &[&aside], &starts, up).unwrap();
        assert!(!met && rel(area(&kept), 500.0) < 1e-9);
        // A block over the left half only, from z = 20 to 22, under a sheet across at z = 30:
        // the left half would stop at z = 20, but the piece beside the block also runs on over
        // it up to z = 30. That is refused, never kept.
        let half = crate::box_solid(Vec3::new(-1.0, -1.0, 20.0), Vec3::new(5.0, 1.0, 22.0)).unwrap();
        let high = rect(&Plane { origin: Vec3::new(0.0, 0.0, 30.0), ..Plane::XY }, -1.0, -1.0, 11.0, 1.0);
        let e = stop_at_bodies(&wall, &[&half, &high], &starts, up).unwrap_err().to_string();
        assert!(e.contains("not supported yet"), "{e}");
        // Hostile input is refused, not a crash.
        assert!(stop_at_bodies(&wall, &[&slab], &[], up).is_err());
        assert!(stop_at_bodies(&wall, &[&slab], &starts, Vec3::ZERO).is_err());
        assert!(stop_at_bodies(&wall, &[&slab], &[Vec3::new(f64::NAN, 0.0, 0.0)], up).is_err());
    }
}
