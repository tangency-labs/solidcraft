//! New points on an existing fit spline (#6533): a fit point inserted where the curve already
//! runs, or a new first or last point the spline runs on to.
//!
//! The spline keeps its curve index and every point it had (the new point is appended to
//! `Sketch::points`), so constraints on its points and on the curve stay as they were.
//!
//! Control point splines take no new points yet: their knots are uniform, so one more control
//! point re-spreads every span and the whole curve moves, wherever the point goes.

use solidcraft_geom::Vec2;

use crate::model::{ConstraintKind, CurveKind, Result, Sketch, SketchError};

/// Closer than this (relative to the spline's size) to one of its points, a new point would
/// double it.
const SAME_POINT: f64 = 1e-6;

impl Sketch {
    /// Insert a fit point into spline `ci` where the curve passes nearest to `at`, and return its
    /// index. The point lies on the curve, which still runs through it and through all its old
    /// points; between them the curve shifts slightly, as interpolation through one more point
    /// does.
    pub fn insert_spline_point(&mut self, ci: usize, at: Vec2) -> Result<usize> {
        let pts = self.editable_spline(ci)?;
        if !at.is_finite() {
            return Err(SketchError::Invalid("the position must be finite".into()));
        }
        let pos: Vec<Vec2> = pts.iter().map(|i| self.point(*i).ok_or_else(|| SketchError::Unknown(format!("point {i}")))).collect::<Result<_>>()?;
        // A fit spline has one exact piece between each pair of neighbouring fit points.
        let pieces = self.exact_segs(ci).filter(|p| !p.is_empty() && p.len() + 1 == pos.len());
        let pieces = pieces.ok_or_else(|| SketchError::Invalid("the spline has no exact pieces to place a point on".into()))?;
        let (piece, new) = pieces
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let q = seg.point_at(seg.closest_param(at));
                (i, q, q.dist(at))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, q, _)| (i, q))
            .ok_or_else(|| SketchError::Invalid("the spline has no exact pieces to place a point on".into()))?;
        let size = spread(&pos);
        if [pos.get(piece), pos.get(piece + 1)].into_iter().flatten().any(|p| p.dist(new) <= SAME_POINT * size) {
            return Err(SketchError::Invalid("there is already a spline point there".into()));
        }
        let p = self.add_point(new, None)?;
        if let Some(CurveKind::Spline { pts, .. }) = self.curves.get_mut(ci).map(|c| &mut c.kind) {
            pts.insert(piece + 1, p);
        }
        Ok(p)
    }

    /// Add `to` to fit spline `ci` as its new first (`at_start`) or last point, and return its
    /// index: the curve runs on from its old end through it. The old end becomes an inner point,
    /// so it must not be joined to anything (another curve, or a constraint on that point or a
    /// tangency of the spline).
    pub fn extend_spline(&mut self, ci: usize, to: Vec2, at_start: bool) -> Result<usize> {
        let pts = self.editable_spline(ci)?;
        if matches!(self.curves.get(ci).map(|c| &c.kind), Some(CurveKind::Spline { periodic: true, .. })) {
            return Err(SketchError::Invalid("a closed spline has no end to extend".into()));
        }
        if !to.is_finite() {
            return Err(SketchError::Invalid("the position must be finite".into()));
        }
        let end = if at_start { pts.first() } else { pts.last() }.copied().ok_or_else(|| SketchError::Invalid("empty spline".into()))?;
        let joined = self.curves.iter().enumerate().any(|(i, c)| i != ci && c.kind.uses(end))
            || self.constraints.iter().any(|k| {
                let (points, curves) = k.kind.indices();
                points.contains(&end) || (curves.contains(&ci) && matches!(k.kind, ConstraintKind::Tangent { .. } | ConstraintKind::Smooth { .. }))
            });
        if joined {
            return Err(SketchError::Invalid("that end of the spline is joined to other geometry; add the point inside the spline instead".into()));
        }
        let pos: Vec<Vec2> = pts.iter().filter_map(|i| self.point(*i)).collect();
        if pos.iter().any(|p| p.dist(to) <= SAME_POINT * spread(&pos)) {
            return Err(SketchError::Invalid("there is already a spline point there".into()));
        }
        let p = self.add_point(to, None)?;
        if let Some(CurveKind::Spline { pts, .. }) = self.curves.get_mut(ci).map(|c| &mut c.kind) {
            if at_start {
                pts.insert(0, p);
            } else {
                pts.push(p);
            }
        }
        Ok(p)
    }

    /// The points of spline `ci`, when it may take another one.
    fn editable_spline(&self, ci: usize) -> Result<Vec<usize>> {
        let curve = self.curves.get(ci).ok_or_else(|| SketchError::Unknown(format!("curve {ci}")))?;
        let CurveKind::Spline { pts, control, .. } = &curve.kind else {
            return Err(SketchError::WrongKind(curve.id.clone(), "only a spline takes another point".into()));
        };
        if *control {
            return Err(SketchError::Invalid(
                "not supported yet: adding a point to a control point spline (it would reshape the whole curve)".into(),
            ));
        }
        if curve.link.is_some() {
            return Err(SketchError::Invalid("a projected spline follows its source; it takes no points of its own".into()));
        }
        if self.freeform_offsets.iter().any(|o| o.output == curve.id) {
            return Err(SketchError::Invalid("an offset spline follows its source; break the offset association first".into()));
        }
        if self.freeform_offsets.iter().any(|o| o.source == curve.id) {
            return Err(SketchError::Invalid(
                "not supported yet: adding a point to a spline that has an offset; break the offset association first".into(),
            ));
        }
        if self.patterns.iter().any(|p| p.contains(&curve.id)) {
            return Err(SketchError::Invalid("not supported yet: adding a point to a patterned spline".into()));
        }
        if pts.len() >= crate::curves::MAX_SPLINE_POINTS {
            return Err(SketchError::Invalid(format!("a spline has at most {} points", crate::curves::MAX_SPLINE_POINTS)));
        }
        Ok(pts.clone())
    }
}

/// The size of a set of points (its bounding box diagonal, at least 1 mm).
fn spread(pos: &[Vec2]) -> f64 {
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in pos {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    if lo.is_finite() && hi.is_finite() { lo.dist(hi).max(1.0) } else { 1.0 }
}
