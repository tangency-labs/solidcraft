//! Trim: a surface body cut where a tool meets it (a plane, another body's face or complete
//! bounded sheet, or sketch curves swept onto it or dropped square onto it), keeping the side
//! of a point or the regions points pick; and Mutual Trim: two surfaces cut by each other, each
//! keeping its picked side, extended to meet first when one stops short of the other.

use solidcore_record::{Plane, Seg2, Vec3};
use solidcraft_geom::brep::eval::CurveEval;
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

/// Trim by the complete bounded cutting sheet, keeping the connected regions picked by
/// `keep`. A multi-face sheet has no single global side: `Side` picks its connected region,
/// as it does for a tool of several sketch curves. The result's history is over `[body]`.
pub fn trim_sheet_keep(body: &Body, sheet: &Body, keep: TrimKeep) -> Result<Body> {
    body.require_brep("trim")?;
    sheet.require_brep("a trim tool")?;
    keep.check()?;
    if std::ptr::eq(body, sheet) {
        return Err(KernelError::Invalid("a surface cannot trim itself".into()));
    }
    if !body.is_surface() || !sheet.is_surface() {
        return Err(KernelError::Invalid("a whole-sheet trim takes two surface bodies".into()));
    }
    crate::guard("trim sheet", || {
        let (target, order) = crate::splitface::tagged("trim sheet", body)?;
        let tool = crate::native::smlib_body(sheet)?;
        let points: Vec<[f64; 3]> = keep.points().iter().map(|p| [p.x, p.y, p.z]).collect();
        let result = target.trim_sheet(&tool, &points).map_err(|e| KernelError::smlib("trim sheet", e))?;
        let (mut out, history) = crate::splitface::brought_back("trim sheet", body, &order, &result)?;
        out.regularization_protected = true;
        Ok(out.with_checked_history(Some(history), &crate::record::counts(&[body])))
    })
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

/// The walls of a surface extrude (`walls`, swept along `along`) cut where they first run into
/// `bodies`, as solidcore-ops does it ([`solidcore_ops::stop::stop_at_bodies`]): each body's
/// faces exactly as they are, bounded where they end and never extended, imprinted on the walls
/// in turn. The pieces kept are those joined to the walls' start, without crossing an edge a
/// body cut: `starts` are corners of the walls at their start, one on each chain. Mesh bodies
/// and bodies apart from the walls cut nothing.
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
    const NAME: &str = "the imprint of the bodies in the walls' way";
    let _span = solidcraft_profile::span("kernel", "stop_at_bodies");
    walls.require_brep("trim")?;
    TrimKeep::Regions(starts).check()?;
    let no_form = || KernelError::Failed(format!("{NAME}: a body has no solidcore form"));
    let form = walls.ops_body().ok_or_else(no_form)?;
    let (ops, order) = (&form.0, &form.1);
    let mut forms = Vec::new();
    for b in bodies {
        if b.is_mesh() || crate::apart(walls, b)? {
            continue;
        }
        forms.push(b.ops_body().ok_or_else(no_form)?);
    }
    let tools: Vec<&solidcore_ops::Body> = forms.iter().map(|f| &f.0).collect();
    if tools.is_empty() {
        return Ok((walls.clone(), false));
    }
    let starts: Vec<[f64; 3]> = starts.iter().map(|s| [s.x, s.y, s.z]).collect();
    let stopped = stopped(NAME, || solidcore_ops::stop::stop_at_bodies(ops, order, &tools, &starts, [along.x, along.y, along.z]))?;
    match stopped {
        Some(stopped) => Ok((pieces(stopped, walls)?, true)),
        None => Ok((walls.clone(), false)),
    }
}

/// `long`, the surface body `original` extended past some of its edges, cut where it first runs
/// into `target`, as solidcore-ops does it ([`solidcore_ops::stop::stop_extension`]): the target's
/// faces exactly as they are, never extended. The pieces kept are those joined to `inside`, points
/// inside the original's faces. An extension part of which never meets the target, or that runs on
/// past a side of it, is refused. The history is over `[long]`, as [`stop_at_bodies`]'s.
pub(crate) fn stop_extension(long: &Body, original: &Body, inside: &[Vec3], target: &Body) -> Result<Body> {
    const NAME: &str = "the imprint of the body the surface extends up to";
    let _span = solidcraft_profile::span("kernel", "stop_extension");
    if target.is_mesh() {
        return Err(KernelError::Invalid("not supported yet: extending a surface up to a mesh body (pick a solid or surface body)".into()));
    }
    let missed = || KernelError::Invalid("part of the extension never meets the body it would extend up to".into());
    if crate::apart(long, target)? {
        return Err(missed());
    }
    let no_form = || KernelError::Failed(format!("{NAME}: a body has no solidcore form"));
    let (form, before, tool) =
        (long.ops_body().ok_or_else(no_form)?, original.ops_body().ok_or_else(no_form)?, target.ops_body().ok_or_else(no_form)?);
    let inside: Vec<[f64; 3]> = inside.iter().map(|p| [p.x, p.y, p.z]).collect();
    let stopped = stopped(NAME, || solidcore_ops::stop::stop_extension(&form.0, &form.1, &before.0, &inside, &tool.0))?;
    pieces(stopped.ok_or_else(missed)?, long)
}

/// A solidcore-ops stop run under this call's cancellation, its errors as this crate's.
fn stopped<T>(name: &str, stop: impl FnOnce() -> solidcore_ops::Result<T>) -> Result<T> {
    match solidcraft_smlib::stop::token() {
        Some(token) => solidcore_ops::cancel::within(&token, stop),
        None => stop(),
    }
    .map_err(|e| match e {
        solidcore_ops::Error::Unsupported(_) | solidcore_ops::Error::Argument => KernelError::Invalid(e.to_string()),
        // a panic the session caught stays the kernel's internal error, as SMLib's do
        solidcore_ops::Error::Internal(m) => KernelError::Internal(format!("{name}: {m}")),
        e => KernelError::Failed(format!("{name}: {e}")),
    })
}

/// What a stop kept of `walls`: each face the face of `walls` it is a piece of.
fn pieces(stopped: solidcore_ops::stop::Stopped, walls: &Body) -> Result<Body> {
    let kept = Body::from_record(stopped.record)?.with_color(walls.color());
    let faces = stopped.from.iter().map(|f| crate::record::from_face(0, *f, crate::history::Change::Kept)).collect();
    Ok(crate::record::attach(kept, &crate::record::counts(&[walls]), Some(faces), &[]))
}

/// A surface body trimmed along sketch curves (chains of segments in `plane`) dropped square
/// onto it: each point of a curve goes to its foot on a face's surface (the nearest point of
/// that surface, along its normal there), wherever the sketch lies, and each chain cuts the
/// faces it bounds a region on (solidcore's `trim::trim_by_dropped_curves`); a chain that bounds
/// no region on any face is refused, never left out. Dropped
/// curves have no one side, so every point of `keep` picks the region of the face nearest it,
/// as it does for a tool of several sketch curves. Each kept face is the face it is a piece of
/// (the result's history is over `[body]`).
///
/// The curves are dropped within a millionth of the body's size of their true feet (the
/// tolerance of every curve SMLib makes on a body's faces).
pub fn trim_dropped_keep(body: &Body, plane: &Plane, curves: &[Vec<Seg2>], keep: TrimKeep) -> Result<Body> {
    const NAME: &str = "trim";
    body.require_brep(NAME)?;
    keep.check()?;
    let segments: usize = curves.iter().map(Vec::len).sum();
    if segments == 0 || curves.iter().any(Vec::is_empty) {
        return Err(KernelError::Invalid("no curves to trim with".into()));
    }
    if segments > crate::splitface::MAX_SEGMENTS {
        return Err(KernelError::Invalid("too many trim curves".into()));
    }
    let dropped: Vec<Vec<_>> =
        curves.iter().map(|chain| chain.iter().map(|s| crate::smlib_sweeps::curve(plane, s)).collect::<Result<_>>()).collect::<Result<_>>()?;
    let points: Vec<[f64; 3]> = keep.points().iter().map(|p| [p.x, p.y, p.z]).collect();
    crate::guard(NAME, || {
        let (target, order) = crate::splitface::tagged(NAME, body)?;
        let result = target.trim_by_dropped_curves(&dropped, &points, crate::smlib_sweeps::curve_tolerance(body)).map_err(dropped_refusal)?;
        let (mut out, history) = crate::splitface::brought_back(NAME, body, &order, &result)?;
        // An authored boundary: later rebuilds keep it.
        out.regularization_protected = true;
        Ok(out.with_checked_history(Some(history), &crate::record::counts(&[body])))
    })
}

/// A dropped-curve trim's refusal in the user's words: curves that divide no face, and what
/// solidcore refuses with a reason of its own (a curve that lands on no face, a point to keep
/// on a cut line); anything else as SMLib's failure.
fn dropped_refusal(e: solidcraft_smlib::Error) -> KernelError {
    use solidcore_ops::Error as Ops;
    match e {
        solidcraft_smlib::Error::Refused(Ops::CurveDoesNotCut) => KernelError::Invalid(
            "a sketch curve, projected along the surface's normal, doesn't divide the surface: each curve must close, reach the surface's edges or end on another curve"
                .into(),
        ),
        solidcraft_smlib::Error::Refused(Ops::Unsupported(why)) => KernelError::Invalid(why.to_string()),
        e => KernelError::smlib("SMLib trim", e),
    }
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
    use crate::PointClass::{OnBoundary, Outside};
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

    /// A bent cutter crosses the square along y=3 for x=0..5 and y=3+0.4(x-5)
    /// for x=5..10. Its lower region is 5*3 + 5*(3+5)/2 = 35, upper 65.
    #[test]
    fn whole_sheet_trim_uses_every_face_with_its_boundaries() {
        let square = rect(&Plane::XY, 0.0, 0.0, 10.0, 10.0);
        let chain =
            vec![Seg2::Line { a: Vec2::new(-5.0, 3.0), b: Vec2::new(5.0, 3.0) }, Seg2::Line { a: Vec2::new(5.0, 3.0), b: Vec2::new(15.0, 7.0) }];
        let cutter = extrude_sheet(&Plane::XY, &[chain], -5.0, 5.0).unwrap();
        assert_eq!(cutter.face_count(), 2);
        for (point, expected) in [(Vec3::new(2.0, 1.0, 0.0), 35.0), (Vec3::new(2.0, 9.0, 0.0), 65.0)] {
            let trimmed = trim_sheet_keep(&square, &cutter, TrimKeep::Side(point)).unwrap();
            assert!(rel(area(&trimmed), expected) < 1e-9, "{} vs {expected}", area(&trimmed));
            let history = trimmed.history().unwrap();
            assert_eq!(history.operands, 1);
            assert!(history.faces.iter().all(|f| matches!(f.origins.as_slice(), [FaceOrigin::Face { operand: 0, face: 0 }])));
        }
        // In reverse, the original join remains traversable above the horizontal cut.
        let across = rect(&Plane::XY, -10.0, -10.0, 20.0, 20.0);
        let top = trim_sheet_keep(&cutter, &across, TrimKeep::Side(Vec3::new(2.0, 3.0, 2.0))).unwrap();
        assert_eq!(top.face_count(), 2);
        let expected = 5.0 * (10.0 + 116.0_f64.sqrt());
        assert!(rel(area(&top), expected) < 1e-9, "{} vs {expected}", area(&top));
    }

    /// A roof of two faces cuts the four walls of a closed square extrusion. Its height
    /// is 5 for x=0..5 and 5+0.2(x-5) for x=5..10. The two y walls contribute
    /// 2*(5*5+5*(5+6)/2), the x walls 10*5+10*6: lower area 215, upper 400-215=185.
    #[test]
    fn whole_sheet_trim_keeps_regions_across_a_closed_targets_seams() {
        let outline = Loop2::polygon(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]);
        let walls = extrude_sheet(&Plane::XY, &[outline.segs], 0.0, 10.0).unwrap();
        let xz = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
        let roof =
            vec![Seg2::Line { a: Vec2::new(-5.0, 5.0), b: Vec2::new(5.0, 5.0) }, Seg2::Line { a: Vec2::new(5.0, 5.0), b: Vec2::new(15.0, 7.0) }];
        let cutter = extrude_sheet(&xz, &[roof], -15.0, 5.0).unwrap();
        assert_eq!((walls.face_count(), cutter.face_count()), (4, 2));
        for (point, expected) in [(Vec3::new(0.0, 5.0, 2.0), 215.0), (Vec3::new(0.0, 5.0, 8.0), 185.0)] {
            let trimmed = trim_sheet_keep(&walls, &cutter, TrimKeep::Side(point)).unwrap();
            assert_eq!(trimmed.face_count(), 4);
            assert!(rel(area(&trimmed), expected) < 1e-9, "{} vs {expected}", area(&trimmed));
        }
    }

    /// The cubic continuation has x=4+12u, y=3+12u²-8u³ and is tangent to the line.
    /// At x=10, u=1/2, so the lower area is 4*3 +
    /// 12*[3u+4u³-2u⁴] from 0 to 1/2 = 34.5; the complement is 65.5.
    #[test]
    fn whole_sheet_trim_follows_a_spline_and_its_tangent_line() {
        let square = rect(&Plane::XY, 0.0, 0.0, 10.0, 10.0);
        let chain = vec![
            Seg2::Line { a: Vec2::new(-5.0, 3.0), b: Vec2::new(4.0, 3.0) },
            Seg2::Cubic { p0: Vec2::new(4.0, 3.0), p1: Vec2::new(8.0, 3.0), p2: Vec2::new(12.0, 7.0), p3: Vec2::new(16.0, 7.0) },
        ];
        let cutter = extrude_sheet(&Plane::XY, &[chain], -5.0, 5.0).unwrap();
        assert_eq!(cutter.face_count(), 2);
        // area() exposes an estimated quadrature accuracy, not a certified error bound.
        // Instead, certify the retained trim against polynomial points at u=1/8,1/4,3/8
        // and points on each side at x=7, where the exact boundary is y=3.625.
        let boundary = [Vec3::new(5.5, 3.171875, 0.0), Vec3::new(7.0, 3.625, 0.0), Vec3::new(8.5, 4.265625, 0.0)];
        let sides = [Vec3::new(7.0, 3.5, 0.0), Vec3::new(7.0, 3.75, 0.0)];
        for (point, expected) in [(Vec3::new(2.0, 1.0, 0.0), [OnBoundary, Outside]), (Vec3::new(2.0, 9.0, 0.0), [Outside, OnBoundary])] {
            let trimmed = trim_sheet_keep(&square, &cutter, TrimKeep::Side(point)).unwrap();
            assert_eq!(trimmed.face_count(), 1);
            assert_eq!(trimmed.classify_points(&boundary).unwrap(), [OnBoundary; 3]);
            assert_eq!(trimmed.classify_points(&sides).unwrap(), expected);
        }
        // In reverse, z=0 halves the straight extrusion: the same generatrix length L
        // gives original area 10L and kept area 5L, exactly half, with both faces retained.
        let across = rect(&Plane::XY, -10.0, -10.0, 20.0, 20.0);
        let top = trim_sheet_keep(&cutter, &across, TrimKeep::Side(Vec3::new(0.0, 3.0, 2.0))).unwrap();
        assert_eq!(top.face_count(), 2);
        for (x, y) in [(0.0, 3.0), (7.0, 3.625)] {
            let points = [Vec3::new(x, y, -2.0), Vec3::new(x, y, 0.0), Vec3::new(x, y, 2.0), Vec3::new(x, y, 5.0)];
            assert_eq!(top.classify_points(&points).unwrap(), [Outside, OnBoundary, OnBoundary, OnBoundary]);
        }
        let (lo, hi) = top.ops_body().unwrap().0.bounds().unwrap();
        let tolerance = top.record().tolerance;
        assert!(lo[2].abs() <= tolerance && (hi[2] - 5.0).abs() <= tolerance, "{lo:?}..{hi:?}");
    }

    /// A spline tool face (an extruded cubic: its surface ends where the face does, so there is
    /// nothing more of it to extend along) that runs 10 into the 40 square and stops is refused
    /// for not reaching across, not as a miss (#19927). Extruded on across the square, the same
    /// cubic trims it along x = 22.25, where the cubic crosses z = 0 (its middle, by symmetry:
    /// (20 + 3 * 23 + 3 * 23 + 20) / 8), keeping 22.25 * 40 = 890 of the square.
    #[test]
    fn a_tool_face_that_stops_inside_the_surface_is_refused_for_not_reaching_across() {
        let square = rect(&Plane::XY, 0.0, 0.0, 40.0, 40.0);
        let cubic = vec![Seg2::Cubic { p0: Vec2::new(20.0, -10.0), p1: Vec2::new(23.0, -3.0), p2: Vec2::new(23.0, 3.0), p3: Vec2::new(20.0, 10.0) }];
        let keep = Vec3::new(5.0, 20.0, 0.0);
        let short = extrude_sheet(&Plane::XZ, std::slice::from_ref(&cubic), -10.0, 10.0).unwrap();
        let e = trim(&square, &SplitTool::Face { body: &short, face: 0 }, keep).unwrap_err().to_string();
        assert!(e.contains("the tool face doesn't reach across the surface, even extended"), "{e}");
        let long = extrude_sheet(&Plane::XZ, &[cubic], -10.0, 50.0).unwrap();
        let cut = trim(&square, &SplitTool::Face { body: &long, face: 0 }, keep).unwrap();
        assert!(rel(area(&cut), 890.0) < 1e-6, "{}", area(&cut));
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

    /// The slot |x| ≤ 3 (a straight run z = 12…28 and round ends) sketched in the plane y = 20
    /// and dropped square onto a half cylinder (radius 10 about Z, y ≥ 0, 40 high). A point
    /// (x, 20, z) of the sketch has its foot on the cylinder at the angle atan2(20, x), at its own
    /// height, so a width 2w of the slot lands on the arc 2R·atan(w/d): the straight run covers
    /// 16 · 2R·atan(a/d), and the two round ends (w = √(a² − t²)) together
    /// 2πR·(√(a² + d²) − d), since ∫₀ᵃ atan(√(a² − t²)/d) dt = (π/2)(√(a² + d²) − d). The piece
    /// inside is their sum, 61.7033…, the piece outside the rest of 10π · 40. The sketch's own
    /// normal would carry the slot onto the wider arc 2R·asin(a/R) instead
    /// ([`a_slot_projected_onto_a_cylinder`]).
    ///
    /// The areas are SMLib's integrals over the trimmed faces (to a relative 1e-8). The slot's
    /// straight sides drop onto rulings exactly; its round ends lie within the curve tolerance
    /// (a millionth of the body's size, which is under 45) of their true drop and are no longer
    /// than the arcs they come from, 2πa together: the area is within 45e-6 · 2πa < 1e-3 of the
    /// closed form.
    #[test]
    fn a_slot_dropped_square_onto_a_cylinder() {
        let (r, a, d): (f64, f64, f64) = (10.0, 3.0, 20.0);
        let half = extrude_sheet(&Plane::XY, &[vec![Seg2::Arc { center: Vec2::ZERO, radius: r, start: 0.0, sweep: PI }]], 0.0, 40.0).unwrap();
        let slot = vec![
            Seg2::Line { a: Vec2::new(a, 12.0), b: Vec2::new(a, 28.0) },
            Seg2::Arc { center: Vec2::new(0.0, 28.0), radius: a, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(-a, 28.0), b: Vec2::new(-a, 12.0) },
            Seg2::Arc { center: Vec2::new(0.0, 12.0), radius: a, start: PI, sweep: PI },
        ];
        let front = Plane { origin: Vec3::new(0.0, d, 0.0), y: Vec3::Z, ..Plane::XZ };
        let inside = 32.0 * r * (a / d).atan() + 2.0 * PI * r * ((a * a + d * d).sqrt() - d);
        let all = PI * r * 40.0;
        let bound = 45e-6 * 2.0 * PI * a;
        let (on, beside) = (Vec3::new(0.0, r, 20.0), Vec3::new(r, 0.0, 20.0));
        let kept = trim_dropped_keep(&half, &front, std::slice::from_ref(&slot), TrimKeep::Side(on)).unwrap();
        assert_eq!(kept.face_count(), 1);
        assert!((area(&kept) - inside).abs() < bound, "{} vs {inside}", area(&kept));
        let rest = trim_dropped_keep(&half, &front, std::slice::from_ref(&slot), TrimKeep::Side(beside)).unwrap();
        assert!((area(&rest) - (all - inside)).abs() < bound, "{} vs {}", area(&rest), all - inside);
        // Both regions picked: the whole wall, in two faces.
        let both = trim_dropped_keep(&half, &front, std::slice::from_ref(&slot), TrimKeep::Regions(&[on, beside])).unwrap();
        assert_eq!(both.face_count(), 2);
        assert!(rel(area(&both), all) < 1e-9, "{}", area(&both));
        // Each piece is the cylinder's face, cut.
        for t in [&kept, &rest, &both] {
            let h = t.history().unwrap();
            assert_eq!(h.operands, 1);
            assert!(h.faces.iter().all(|f| matches!(f.origins.as_slice(), [FaceOrigin::Face { operand: 0, face: 0 }])), "{h:?}");
        }
        // A point on the slot's own edge picks neither side; a line that ends inside the wall
        // bounds nothing.
        let reach = (a * a + d * d).sqrt();
        let edge = Vec3::new(r * a / reach, r * d / reach, 20.0);
        let e = trim_dropped_keep(&half, &front, std::slice::from_ref(&slot), TrimKeep::Side(edge)).unwrap_err().to_string();
        assert!(e.contains("pick inside the region to keep"), "{e}");
        let slit = vec![vec![Seg2::Line { a: Vec2::new(-a, 20.0), b: Vec2::new(a, 20.0) }]];
        let e = trim_dropped_keep(&half, &front, &slit, TrimKeep::Side(on)).unwrap_err().to_string();
        assert!(e.contains("doesn't divide the surface"), "{e}");
        assert!(trim_dropped_keep(&half, &front, &[], TrimKeep::Side(on)).is_err());
        // A curve behind the half cylinder (y = -20 from the axis) has its feet on the half that
        // is not there: it lands on no face.
        let behind = Plane { origin: Vec3::new(0.0, -d, 0.0), y: Vec3::Z, ..Plane::XZ };
        let e = trim_dropped_keep(&half, &behind, std::slice::from_ref(&slot), TrimKeep::Side(on)).unwrap_err().to_string();
        assert!(e.contains("drops onto no face"), "{e}");
    }

    /// Two sketch lines that cross (x = 10 and y = 10, sketched 5 above the 20 × 20 square)
    /// dropped square onto it cut it into four squares of 100. Each line is its own chain, and
    /// the second splits the first's edge where they cross: a point keeps its own square alone,
    /// two points keep two. With them, a segment that ends inside the square bounds nothing:
    /// the trim is refused, not made without it.
    #[test]
    fn crossing_lines_dropped_square_part_four_regions() {
        let sq = rect(&Plane::XY, 0.0, 0.0, 20.0, 20.0);
        let above = Plane { origin: Vec3::new(0.0, 0.0, 5.0), ..Plane::XY };
        let lines = vec![
            vec![Seg2::Line { a: Vec2::new(10.0, -5.0), b: Vec2::new(10.0, 25.0) }],
            vec![Seg2::Line { a: Vec2::new(-5.0, 10.0), b: Vec2::new(25.0, 10.0) }],
        ];
        for p in [Vec3::new(5.0, 5.0, 0.0), Vec3::new(15.0, 5.0, 0.0), Vec3::new(15.0, 15.0, 0.0), Vec3::new(5.0, 15.0, 0.0)] {
            let kept = trim_dropped_keep(&sq, &above, &lines, TrimKeep::Side(p)).unwrap();
            assert!(kept.face_count() == 1 && rel(area(&kept), 100.0) < 1e-9, "{p:?}: {}", area(&kept));
        }
        let two = trim_dropped_keep(&sq, &above, &lines, TrimKeep::Regions(&[Vec3::new(5.0, 5.0, 0.0), Vec3::new(15.0, 15.0, 0.0)])).unwrap();
        assert!(two.face_count() == 2 && rel(area(&two), 200.0) < 1e-9, "{}", area(&two));
        let mut with_slit = lines;
        with_slit.push(vec![Seg2::Line { a: Vec2::new(2.0, 3.0), b: Vec2::new(4.0, 3.0) }]);
        let e = trim_dropped_keep(&sq, &above, &with_slit, TrimKeep::Side(Vec3::new(5.0, 5.0, 0.0))).unwrap_err().to_string();
        assert!(e.contains("doesn't divide the surface"), "{e}");
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

    /// The normal of each face of `b`, from its display triangles' winding (each face is flat).
    fn normals(b: &Body) -> Vec<Vec3> {
        b.faces(0.01).unwrap().iter().map(|f| f.plane_normal.unwrap()).collect()
    }

    /// A wall (the line y = 0 from x = 0 to 10, swept 10 up z) turned over with Reverse Normal
    /// trims at x = 5 as the wall does, keeping half its 100 (#13640), and the half kept faces as
    /// the turned wall did. Its normal is ±y, the other way from the wall's.
    #[test]
    fn a_turned_wall_trims_and_keeps_its_side() {
        let wall = extrude_sheet(&Plane::XY, &[vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }]], 0.0, 10.0).unwrap();
        let turned = crate::reverse_faces(&wall, &[]).unwrap();
        let [n] = normals(&turned)[..] else { panic!("one face") };
        let [m] = normals(&wall)[..] else { panic!("one face") };
        assert!(n.dot(m) < -1.0 + 1e-9 && n.y.abs() > 1.0 - 1e-9, "{n:?} vs {m:?}");
        let cut = SplitTool::Plane(Plane::new(Vec3::new(5.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap());
        let half = trim(&turned, &cut, Vec3::new(1.0, 0.0, 5.0)).unwrap();
        assert!(rel(area(&half), 50.0) < 1e-9, "{}", area(&half));
        assert!(normals(&half).iter().all(|k| k.dot(n) > 1.0 - 1e-9), "{:?} vs {n:?}", normals(&half));
        assert!(measure(&half).unwrap().bbox.max.x < 5.0 + 1e-9);
    }
}
