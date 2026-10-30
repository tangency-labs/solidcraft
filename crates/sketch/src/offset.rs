//! Associative offsets keep their output entity ids while their source curves move.
use crate::{ConstraintKind, CurveKind, Shape, Sketch, SketchError, intersections};
use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OffsetChain {
    pub sources: Vec<(String, bool)>,
    pub outputs: Vec<String>,
    pub distance: f64,
    pub closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology: Option<crate::offset_topology::OffsetTopology>,
}

impl OffsetChain {
    /// Includes reserved corner roles that may temporarily disappear from the boundary.
    pub fn owns_output(&self, output: &str) -> bool {
        self.outputs.iter().any(|id| id == output) || self.topology.as_ref().is_some_and(|t| t.roles.iter().any(|r| r.output == output))
    }
}

impl Sketch {
    /// The signed distance of the association that owns an output (including retired roles).
    pub fn offset_distance(&self, output: &str) -> Option<f64> {
        self.offsets
            .iter()
            .find(|o| o.owns_output(output))
            .map(|o| o.distance)
            .or_else(|| self.freeform_offsets.iter().find(|o| o.output == output).map(|o| o.distance))
    }

    /// A Parallel between an offset line and the source it follows (#4464). The association
    /// keeps them parallel already, and the output's points are held during a solve, so solving
    /// it would only pin the source to the output's current direction. It drives again once
    /// Break releases the output.
    pub(crate) fn follows_its_source(&self, kind: &ConstraintKind) -> bool {
        let ConstraintKind::Parallel { a, b } = *kind else {
            return false;
        };
        let (Some(a), Some(b)) = (self.curves.get(a).map(|c| c.id.as_str()), self.curves.get(b).map(|c| c.id.as_str())) else {
            return false;
        };
        self.offsets.iter().any(|o| {
            o.sources.iter().enumerate().any(|(i, (source, _))| {
                let output = match &o.topology {
                    Some(t) => t.roles.get(i).map(|r| r.output.as_str()),
                    None => o.outputs.get(i).map(String::as_str),
                };
                output.is_some_and(|output| (source == a && output == b) || (source == b && output == a))
            })
        })
    }

    /// An arc an offset made round its source's own centre (#6569). Its ends are held while the
    /// sketch solves, so its own condition, both ends as far from the centre, only tied that
    /// centre (and the source with it) to the middle of the held ends: a slot offset by 1 mm
    /// could no longer be moved by the dimension that places it. The source's own arc keeps the
    /// centre, and the offset is rebuilt from the source after the solve.
    pub(crate) fn offset_arc_follows_source(&self, curve: usize) -> bool {
        if self.offsets.is_empty() {
            return false;
        }
        let Some(arc) = self.curves.get(curve) else { return false };
        let CurveKind::Arc { c: centre, .. } = arc.kind else { return false };
        let output = |id: &str| self.offsets.iter().any(|o| o.owns_output(id));
        output(&arc.id) && self.curves.iter().enumerate().any(|(k, other)| k != curve && other.kind.uses(centre) && !output(&other.id))
    }

    /// A circle an offset made (#9264). The offset sets its radius from the source's after the
    /// solve, so the solve holds that radius instead of counting it as a free unknown.
    pub(crate) fn offset_circle_follows_source(&self, curve: usize) -> bool {
        let Some(circle) = self.curves.get(curve) else { return false };
        matches!(circle.kind, CurveKind::Circle { .. }) && self.offsets.iter().any(|o| o.owns_output(&circle.id))
    }

    /// Endpoints of a normal-distance annotation, evaluated from its source curve.
    pub fn offset_dimension_points(&self, output: &str) -> Option<(Vec2, Vec2)> {
        let (source, forward, distance) = self
            .offsets
            .iter()
            .find(|o| o.owns_output(output))
            .and_then(|o| o.sources.first().map(|(id, forward)| (id.as_str(), *forward, o.distance)))
            .or_else(|| self.freeform_offsets.iter().find(|o| o.output == output).map(|o| (o.source.as_str(), o.forward, o.distance)))?;
        let index = self.curve_index(source)?;
        let segment = self.exact_segs(index).unwrap_or_else(|| self.segs(index)).into_iter().next()?;
        let p = segment.point_at(0.5);
        let tangent = (segment.point_at(0.5001) - segment.point_at(0.4999)).normalized()?;
        Some((p, p + tangent.perp() * if forward { distance } else { -distance }))
    }
    /// Removing an output removes its relationship. A partial analytic chain
    /// deletion releases the surviving outputs, while retained dependents keep
    /// their source references so the command's refresh refuses broken sources.
    pub(crate) fn prune_offsets_for_removed_curves(&mut self, which: &[usize]) {
        let removed = which.iter().filter_map(|i| self.curves.get(*i).map(|curve| curve.id.clone())).collect::<std::collections::BTreeSet<_>>();
        if removed.is_empty() {
            return;
        }
        let released = self
            .offsets
            .iter()
            .filter(|offset| offset.outputs.iter().any(|id| removed.contains(id)))
            .flat_map(|offset| offset.outputs.iter().cloned().chain(offset.topology.iter().flat_map(|t| t.roles.iter().map(|r| r.output.clone()))))
            .chain(self.freeform_offsets.iter().filter(|offset| removed.contains(&offset.output)).map(|offset| offset.output.clone()))
            .collect::<std::collections::BTreeSet<_>>();
        self.offsets.retain(|offset| !offset.outputs.iter().any(|id| removed.contains(id)));
        self.freeform_offsets.retain(|offset| !removed.contains(&offset.output));
        self.constraints.retain(|c| !matches!(&c.kind, ConstraintKind::OffsetDistance { output, .. } if released.contains(output)));
        let points = self
            .curves
            .iter()
            .filter(|curve| released.contains(&curve.id))
            .flat_map(|curve| curve.kind.point_ids())
            .collect::<std::collections::BTreeSet<_>>();
        for index in points {
            if index != 0
                && self.curves.iter().all(|curve| !curve.kind.uses(index) || released.contains(&curve.id))
                && let Some(point) = self.points.get_mut(index)
            {
                point.fixed = false;
            }
        }
    }

    pub fn refresh_offsets(&mut self) -> Result<(), SketchError> {
        if self.offsets.is_empty() && self.freeform_offsets.is_empty() {
            return Ok(());
        }
        let mut candidate = self.clone();
        let mut driven = std::collections::BTreeSet::new();
        for constraint in &self.constraints {
            if let ConstraintKind::OffsetDistance { output, value, reverse } = &constraint.kind {
                self.validate(&constraint.kind)?;
                let analytic = candidate.offsets.iter().position(|o| o.owns_output(output));
                let freeform = candidate.freeform_offsets.iter().position(|o| o.output == *output);
                let identity = analytic
                    .or_else(|| freeform.map(|i| candidate.offsets.len() + i))
                    .ok_or_else(|| SketchError::Invalid("missing offset dimension association".into()))?;
                if constraint.driven || !driven.insert(identity) {
                    return Err(SketchError::Invalid("an offset must have exactly one driving distance".into()));
                }
                let distance = if *reverse { -*value } else { *value };
                if let Some(offset) = analytic.and_then(|i| candidate.offsets.get_mut(i)) {
                    offset.distance = distance;
                }
                if let Some(offset) = freeform.and_then(|i| candidate.freeform_offsets.get_mut(i)) {
                    offset.distance = distance;
                }
            }
        }
        candidate.refresh_analytic_offsets()?;
        candidate.refresh_freeform_offsets()?;
        *self = candidate;
        Ok(())
    }

    fn refresh_analytic_offsets(&mut self) -> Result<(), SketchError> {
        for index in 0..self.offsets.len() {
            if self.refresh_convex_offset(index)? {
                continue;
            }
            let offset = self.offsets.get(index).cloned().ok_or_else(|| SketchError::Invalid("missing offset chain".into()))?;
            if offset.sources.len() != offset.outputs.len() || offset.sources.len() > 10_000 || !offset.distance.is_finite() {
                return Err(SketchError::Invalid("damaged offset chain".into()));
            }
            let bad = || SketchError::Invalid("an offset source vanished or changed shape".into());
            let mut pieces = Vec::new();
            for (id, forward) in &offset.sources {
                let i = self.curve_index(id).ok_or_else(bad)?;
                let kind = &self.curves.get(i).ok_or_else(bad)?.kind;
                let piece = match *kind {
                    CurveKind::Line { a, b } => {
                        let (a, b) = (self.point(a).ok_or_else(bad)?, self.point(b).ok_or_else(bad)?);
                        let (a, b) = if *forward { (a, b) } else { (b, a) };
                        let n = (b - a).normalized().ok_or_else(bad)?.perp() * offset.distance;
                        (Shape::Line { a: a + n, b: b + n }, a + n, b + n)
                    }
                    CurveKind::Arc { c, a, b } => {
                        let (c, a, b) = (self.point(c).ok_or_else(bad)?, self.point(a).ok_or_else(bad)?, self.point(b).ok_or_else(bad)?);
                        let r = c.dist(a) - if *forward { offset.distance } else { -offset.distance };
                        if r <= 1e-9 {
                            return Err(SketchError::Invalid("the offset collapses an arc".into()));
                        }
                        let a = c + (a - c).normalized().ok_or_else(bad)? * r;
                        let b = c + (b - c).normalized().ok_or_else(bad)? * r;
                        let (a, b) = if *forward { (a, b) } else { (b, a) };
                        (Shape::Round { c, r, start: 0.0, sweep: std::f64::consts::TAU }, a, b)
                    }
                    CurveKind::Circle { c, r } => {
                        let c = self.point(c).ok_or_else(bad)?;
                        let r = r - offset.distance;
                        if r <= 1e-9 {
                            return Err(SketchError::Invalid("the offset collapses a circle".into()));
                        }
                        let out = offset.outputs.first().and_then(|id| self.curve_index(id)).ok_or_else(bad)?;
                        if let Some(crate::Curve { kind: CurveKind::Circle { r: target, .. }, .. }) = self.curves.get_mut(out) {
                            *target = r;
                        } else {
                            return Err(bad());
                        }
                        let _ = c;
                        continue;
                    }
                    _ => return Err(bad()),
                };
                pieces.push(piece);
            }
            if pieces.is_empty() {
                continue;
            }
            let joint = |i: usize, j: usize| -> Result<Vec2, SketchError> {
                let (x, y) = (pieces.get(i).ok_or_else(bad)?, pieces.get(j).ok_or_else(bad)?);
                if x.2.dist(y.1) < 1e-6 {
                    return Ok(x.2);
                }
                let guess = (x.2 + y.1) * 0.5;
                intersections(&x.0, &y.0)
                    .into_iter()
                    .min_by(|a, b| a.dist(guess).total_cmp(&b.dist(guess)))
                    .ok_or_else(|| SketchError::Invalid("the offset curves no longer meet".into()))
            };
            let n = pieces.len();
            let mut joints = vec![if offset.closed { joint(n - 1, 0)? } else { pieces.first().ok_or_else(bad)?.1 }];
            for i in 0..n.saturating_sub(1) {
                joints.push(joint(i, i + 1)?);
            }
            if !offset.closed {
                joints.push(pieces.last().ok_or_else(bad)?.2);
            }
            for (i, id) in offset.outputs.iter().enumerate() {
                let a = *joints.get(i).ok_or_else(bad)?;
                let b = *joints.get(if offset.closed { (i + 1) % n } else { i + 1 }).ok_or_else(bad)?;
                let shape = &pieces.get(i).ok_or_else(bad)?.0;
                if let Shape::Line { a: p, b: q } = shape
                    && (b - a).dot(*q - *p) <= 1e-9
                {
                    return Err(SketchError::Invalid("the offset curves would cross".into()));
                }
                let ci = self.curve_index(id).ok_or_else(bad)?;
                let kind = self.curves.get(ci).ok_or_else(bad)?.kind.clone();
                let (pa, pb) = match kind {
                    CurveKind::Line { a, b } => (a, b),
                    CurveKind::Arc { a, b, .. } => {
                        if offset.sources.get(i).is_some_and(|x| x.1) {
                            (a, b)
                        } else {
                            (b, a)
                        }
                    }
                    _ => return Err(bad()),
                };
                for (pi, pos) in [(pa, a), (pb, b)] {
                    if let Some(p) = self.points.get_mut(pi) {
                        p.pos = pos;
                        p.fixed = true;
                    }
                }
            }
        }
        Ok(())
    }
}
