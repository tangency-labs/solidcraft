//! New points on an existing fit spline (#6533): a fit point inserted where the curve already
//! runs, or a new first or last point the spline runs on to. One point taken out of a spline
//! (#10783). Also a spline's direction reversed (#8224).
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

    /// Take point `p` out of spline `ci` (#10783), and return whether the point itself went too.
    /// The spline runs through (or over) its other points, and keeps its curve index, so
    /// constraints on the curve and on its other points stay. The point goes with the constraints
    /// on it, unless another curve uses it (it stays, for that curve) or it is the origin.
    ///
    /// The spline keeps at least 2 points (a closed one 3 besides its closing point, which stays).
    /// An end of an open spline joined to other geometry stays, as for [`Sketch::extend_spline`]:
    /// the spline would no longer end where it is joined.
    pub fn remove_spline_point(&mut self, ci: usize, p: usize) -> Result<bool> {
        let curve = self.curves.get(ci).ok_or_else(|| SketchError::Unknown(format!("curve {ci}")))?;
        let CurveKind::Spline { pts, periodic, .. } = &curve.kind else {
            return Err(SketchError::WrongKind(curve.id.clone(), "only a spline has points to remove".into()));
        };
        let (pts, periodic) = (pts.clone(), *periodic);
        if curve.link.is_some() {
            return Err(SketchError::Invalid("a projected spline follows its source; its points can't be removed".into()));
        }
        if self.freeform_offsets.iter().any(|o| o.output == curve.id) {
            return Err(SketchError::Invalid("an offset spline follows its source; break the offset association first".into()));
        }
        if self.freeform_offsets.iter().any(|o| o.source == curve.id) {
            return Err(SketchError::Invalid(
                "not supported yet: removing a point from a spline that has an offset; break the offset association first".into(),
            ));
        }
        if self.patterns.iter().any(|pattern| pattern.contains(&curve.id)) {
            return Err(SketchError::Invalid("not supported yet: removing a point from a patterned spline".into()));
        }
        let at = match pts.iter().enumerate().filter(|(_, q)| **q == p).map(|(i, _)| i).collect::<Vec<_>>().as_slice() {
            [] => return Err(SketchError::Invalid(format!("that point isn't one of {}'s points", curve.id))),
            [at] => *at,
            _ => return Err(SketchError::Invalid("that point closes the spline; remove another one".into())),
        };
        let last = pts.len().saturating_sub(1);
        if periodic && (at == 0 || at == last) {
            return Err(SketchError::Invalid("a closed spline's closing point stays; remove another one".into()));
        }
        if pts.len() < if periodic { 5 } else { 3 } {
            return Err(SketchError::Invalid(format!("a spline keeps at least {} points", if periodic { 3 } else { 2 })));
        }
        if !periodic && (at == 0 || at == last) {
            let joined = self.curves.iter().enumerate().any(|(i, c)| i != ci && c.kind.uses(p))
                || self.constraints.iter().any(|k| {
                    let (points, curves) = k.kind.indices();
                    points.contains(&p) || (curves.contains(&ci) && matches!(k.kind, ConstraintKind::Tangent { .. } | ConstraintKind::Smooth { .. }))
                });
            if joined {
                return Err(SketchError::Invalid(
                    "that end of the spline is joined to other geometry or held by a constraint; remove those first".into(),
                ));
            }
        }
        let mut left = pts;
        left.remove(at);
        if periodic {
            let pos: Vec<Vec2> =
                left.iter().map(|i| self.point(*i).ok_or_else(|| SketchError::Unknown(format!("point {i}")))).collect::<Result<_>>()?;
            crate::periodic::validate_periodic_points(&pos)?;
        }
        if let Some(CurveKind::Spline { pts, .. }) = self.curves.get_mut(ci).map(|c| &mut c.kind) {
            *pts = left;
        }
        // The origin (point 0) is never removed.
        if p == 0 || self.point_used_by_curve(p) {
            return Ok(false);
        }
        self.keep_ties_through(&[p]);
        self.remove_points(&[p]);
        Ok(true)
    }

    /// Reverse spline `ci`'s direction: its last point becomes its first and the curve runs the
    /// other way along the same path. Its points are reversed in place, so every point, and every
    /// constraint on them and on the curve, stays. The shape doesn't change: a fit spline's chord
    /// lengths and natural ends, a control point spline's uniform clamped knots and a closed
    /// spline's cyclic fit (whose closing point repeats its first) read the same both ways.
    pub fn reverse_spline(&mut self, ci: usize) -> Result<()> {
        let curve = self.curves.get(ci).ok_or_else(|| SketchError::Unknown(format!("curve {ci}")))?;
        if !matches!(curve.kind, CurveKind::Spline { .. }) {
            return Err(SketchError::WrongKind(curve.id.clone(), "only a spline can be reversed".into()));
        }
        if curve.link.is_some() {
            return Err(SketchError::Invalid("a projected spline follows its source; it can't be reversed on its own".into()));
        }
        let id = curve.id.as_str();
        // An offset's side is taken along its sources' directions.
        let offset = self.freeform_offsets.iter().any(|o| o.source == id || o.output == id)
            || self.offsets.iter().any(|o| o.sources.iter().any(|(source, _)| source == id) || o.owns_output(id));
        if offset {
            return Err(SketchError::Invalid("the spline is part of an offset; break the offset association first".into()));
        }
        if let Some(CurveKind::Spline { pts, .. }) = self.curves.get_mut(ci).map(|c| &mut c.kind) {
            pts.reverse();
        }
        Ok(())
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
