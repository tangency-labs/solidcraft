//! Stored parameters and stable entity ids for editing an existing sketch pattern.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec2;

use crate::{ConstraintKind, Curve, CurveKind, Sketch, SketchError};

/// Placement relative to the original, along a pattern direction's oriented line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternAlignment {
    Left,
    Center,
    #[default]
    Right,
}

impl PatternAlignment {
    /// Slot zero is always the original. Center alternates sides to retain stable copy ids.
    pub fn multiplier(self, index: usize, reversed: bool) -> f64 {
        let value = match self {
            Self::Left => -(index as f64),
            Self::Right => index as f64,
            Self::Center if index.is_multiple_of(2) => -((index / 2) as f64),
            Self::Center => index.div_ceil(2) as f64,
        };
        if reversed { -value } else { value }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SketchPatternKind {
    Circular {
        center: String,
        count: usize,
        angle: f64,
        #[serde(default)]
        alignment: PatternAlignment,
        #[serde(default)]
        reversed: bool,
    },
    Rectangular {
        dir: Vec2,
        #[serde(default)]
        dir2: Option<Vec2>,
        #[serde(default)]
        direction: Option<String>,
        #[serde(default)]
        direction2: Option<String>,
        count: usize,
        spacing: f64,
        count2: usize,
        spacing2: f64,
        #[serde(default)]
        alignment: PatternAlignment,
        #[serde(default)]
        alignment2: PatternAlignment,
        #[serde(default)]
        reversed: bool,
        #[serde(default)]
        reversed2: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternInstance {
    pub index: [usize; 2],
    pub curves: Vec<String>,
    /// Seed point id and instance point id, in the order used to create the copy.
    pub points: Vec<(String, String)>,
    pub constraints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPattern {
    pub id: String,
    pub entities: Vec<String>,
    pub kind: SketchPatternKind,
    /// Authored circular Quantity expression; absent for legacy and fixed-count patterns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_expression: Option<String>,
    pub instances: Vec<PatternInstance>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub expressions: std::collections::BTreeMap<String, String>,
}

impl SketchPattern {
    /// Resolve an evaluated circular Quantity with the dialog's whole-number tolerance.
    pub fn circular_quantity(value: f64) -> Result<usize, SketchError> {
        let count = value.round();
        if !value.is_finite() || !(1.0..=500.0).contains(&value) || count < 2.0 || (value - count).abs() > 1e-9 {
            return Err(SketchError::Invalid("circular pattern Quantity must be a whole number from 2 to 500".into()));
        }
        Ok(count as usize)
    }

    pub fn contains(&self, entity: &str) -> bool {
        self.entities.iter().any(|id| id == entity)
            || self
                .instances
                .iter()
                .any(|instance| instance.curves.iter().any(|id| id == entity) || instance.points.iter().any(|(_, id)| id == entity))
            || matches!(&self.kind, SketchPatternKind::Circular { center, .. } if center == entity)
            || matches!(&self.kind, SketchPatternKind::Rectangular { direction, direction2, .. }
                if direction.as_deref() == Some(entity) || direction2.as_deref() == Some(entity))
    }
}

impl Sketch {
    /// Rebuild a circular Quantity while retaining surviving copy ids.
    pub fn resize_circular_pattern(&mut self, id: &str, count: usize) -> Result<(), SketchError> {
        if !(2..=500).contains(&count) {
            return Err(SketchError::Invalid("circular pattern Quantity must be 2…500".into()));
        }
        self.validate_patterns()?;
        let index = self.patterns.iter().position(|p| p.id == id).ok_or_else(|| SketchError::Invalid("missing sketch pattern".into()))?;
        let mut kind = self.patterns.get(index).ok_or_else(|| SketchError::Invalid("missing sketch pattern".into()))?.kind.clone();
        let SketchPatternKind::Circular { count: previous, .. } = &mut kind else {
            return Err(SketchError::Invalid("Quantity expression requires a circular pattern".into()));
        };
        if *previous == count {
            return Ok(());
        }
        *previous = count;
        self.update_pattern(index, kind)
    }

    /// Rebuild a pattern after its driving expressions change, retaining surviving copy ids.
    pub fn update_pattern(&mut self, index: usize, kind: SketchPatternKind) -> Result<(), SketchError> {
        let old = self.patterns.get(index).cloned().ok_or_else(|| SketchError::Invalid("missing sketch pattern".into()))?;
        let check_count = |n: usize| {
            if !(1..=500).contains(&n) { Err(SketchError::Invalid("pattern count must be a positive integer from 1 to 500".into())) } else { Ok(()) }
        };
        match &kind {
            SketchPatternKind::Circular { count, angle, .. } => {
                check_count(*count)?;
                if !angle.is_finite() || angle.abs() <= 1e-9 || angle.abs() > 360.0 {
                    return Err(SketchError::Invalid("pattern angle must be nonzero and at most 360 degrees".into()));
                }
            }
            SketchPatternKind::Rectangular { count, count2, spacing, spacing2, .. } => {
                check_count(*count)?;
                check_count(*count2)?;
                if count.checked_mul(*count2).is_none_or(|n| n > 500) {
                    return Err(SketchError::Invalid("pattern counts must make at most 500 instances total".into()));
                }
                if ![spacing, spacing2].iter().all(|v| v.is_finite() && v.abs() <= crate::MAX_COORD) {
                    return Err(SketchError::Invalid("pattern spacing must be finite and in range".into()));
                }
            }
        }
        self.patterns.remove(index);
        let entity_ids = old.entities.clone();
        let (center, slots) = match &kind {
            SketchPatternKind::Circular { center, count, angle, alignment, reversed } => {
                let center = self.resolve_point(center).ok_or_else(|| SketchError::Invalid("missing pattern center".into()))?;
                let full = (angle.abs() - 360.0).abs() < 1e-9;
                let step = angle / if full { *count as f64 } else { (count.max(&2) - 1) as f64 };
                (
                    Some(center),
                    (1..*count).map(|i| ([i, 0], (step * alignment.multiplier(i, *reversed)).to_radians(), Vec2::ZERO)).collect::<Vec<_>>(),
                )
            }
            SketchPatternKind::Rectangular { count, count2, .. } => {
                (None, (0..*count).flat_map(|i| (0..*count2).filter_map(move |j| (i != 0 || j != 0).then_some(([i, j], 0.0, Vec2::ZERO)))).collect())
            }
        };
        let center_id = center.and_then(|i| self.points.get(i)).map(|p| p.id.clone());
        let mut existing = old.instances.clone();
        // Remove retired instances first: all following indices are resolved by stable id.
        for instance in existing.iter().filter(|instance| !slots.iter().any(|(index, _, _)| *index == instance.index)) {
            let curves: Vec<_> = instance.curves.iter().filter_map(|id| self.curve_index(id)).collect();
            self.constraints.retain(|c| !instance.constraints.contains(&c.id));
            self.remove_curves(&curves);
            let points: Vec<_> = instance.points.iter().filter_map(|(_, id)| self.resolve_point(id)).collect();
            self.remove_points(&points);
        }
        if let SketchPatternKind::Rectangular { direction, direction2, .. } = &kind {
            for id in direction.iter().chain(direction2) {
                if self.curve_index(id).is_none() {
                    return Err(SketchError::Invalid("the direction belongs to an instance removed by this edit".into()));
                }
            }
        }
        let mut instances = Vec::new();
        let center = center_id.as_ref().and_then(|id| self.resolve_point(id));
        if center_id.is_some() && center.is_none() {
            return Err(SketchError::Invalid("the center belongs to an instance removed by this edit".into()));
        }
        for (index, angle, offset) in slots {
            let (base_offset, directions) = linear_displacement(self, &kind, index, offset);
            let c = center.and_then(|i| self.point(i)).unwrap_or(Vec2::ZERO);
            let mut displacement = base_offset;
            for (line, distance) in directions.into_iter().flatten() {
                if let Some(crate::Shape::Line { a, b }) = self.shape(line)
                    && let Some(u) = (b - a).normalized()
                {
                    displacement += u * distance.x + u.perp() * distance.y;
                }
            }
            let transform = |p| rotate(c, angle)(p) + displacement;
            let instance = if let Some(i) = existing.iter().position(|instance| instance.index == index) {
                let instance = existing.remove(i);
                for cid in &instance.constraints {
                    if let Some(constraint) = self.constraints.iter_mut().find(|constraint| constraint.id == *cid)
                        && let ConstraintKind::PatternPoint { center: slot, angle: a, offset: off, directions: dirs, .. } = &mut constraint.kind
                    {
                        *slot = center;
                        *a = angle;
                        *off = base_offset;
                        *dirs = directions;
                    }
                }
                for (seed, copy) in &instance.points {
                    let pos =
                        self.resolve_point(seed).and_then(|i| self.point(i)).ok_or_else(|| SketchError::Invalid("missing seed point".into()))?;
                    if let Some(point) = self.resolve_point(copy).and_then(|i| self.points.get_mut(i)) {
                        let at = transform(pos);
                        if !crate::model::in_range(at) {
                            return Err(SketchError::Invalid("pattern point is out of range".into()));
                        }
                        point.pos = at;
                    }
                }
                instance
            } else {
                let cs: Vec<_> = entity_ids.iter().filter_map(|id| self.curve_index(id)).collect();
                let ps: Vec<_> = entity_ids.iter().filter(|id| self.curve_index(id).is_none()).filter_map(|id| self.resolve_point(id)).collect();
                let (made, map) = copy_through(self, &cs, &ps, &transform)?;
                let mut cons = Vec::new();
                for (p, q) in &map {
                    cons.push(
                        self.add_constraint(ConstraintKind::PatternPoint { p: *p, q: *q, center, angle, offset: base_offset, directions }, None)?,
                    );
                }
                for (a, b) in cs.iter().zip(&made) {
                    if matches!(self.curves.get(*a).map(|c| &c.kind), Some(CurveKind::Circle { .. } | CurveKind::Ellipse { .. })) {
                        cons.push(self.add_constraint(ConstraintKind::PatternRadius { a: *a, b: *b }, None)?);
                    }
                }
                PatternInstance {
                    index,
                    curves: made.iter().filter_map(|i| self.curves.get(*i).map(|c| c.id.clone())).collect(),
                    points: map.iter().filter_map(|(p, q)| Some((self.points.get(*p)?.id.clone(), self.points.get(*q)?.id.clone()))).collect(),
                    constraints: cons,
                }
            };
            instances.push(instance);
        }

        self.patterns.insert(index.min(self.patterns.len()), SketchPattern { kind, instances, ..old });
        self.follow_pattern_seeds();
        Ok(())
    }

    /// Start an explicit Move/Scale of seed geometry with its copies already transformed.
    /// Interactive drags instead leave the ordinary relations to the held-point solver,
    /// which also supports dragging an instance back through its seed.
    pub fn follow_pattern_seeds(&mut self) {
        let constraint_indices: std::collections::BTreeMap<_, _> = self.constraints.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
        for pattern in &self.patterns {
            for instance in &pattern.instances {
                for cid in &instance.constraints {
                    let Some(kind) = constraint_indices.get(cid.as_str()).and_then(|i| self.constraints.get(*i)).map(|c| c.kind.clone()) else {
                        continue;
                    };
                    match kind {
                        crate::ConstraintKind::PatternPoint { p, q, center, angle, mut offset, directions } => {
                            for (l, distance) in directions.into_iter().flatten() {
                                if let Some(crate::Shape::Line { a, b }) = self.shape(l)
                                    && let Some(u) = (b - a).normalized()
                                {
                                    offset += u * distance.x + u.perp() * distance.y;
                                }
                            }
                            let Some(seed) = self.point(p) else { continue };
                            let c = center.and_then(|c| self.point(c)).unwrap_or(Vec2::ZERO);
                            let d = seed - c;
                            let (sin, cos) = angle.sin_cos();
                            if let Some(copy) = self.points.get_mut(q) {
                                copy.pos = c + Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos) + offset;
                            }
                        }
                        crate::ConstraintKind::PatternRadius { a, b } => {
                            let r = match self.curves.get(a).map(|c| &c.kind) {
                                Some(CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. }) => Some(*r),
                                _ => None,
                            };
                            if let (Some(r), Some(CurveKind::Circle { r: copy, .. } | CurveKind::Ellipse { r: copy, .. })) =
                                (r, self.curves.get_mut(b).map(|c| &mut c.kind))
                            {
                                *copy = r;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        self.refresh_pattern_shapes();
    }
    /// Deleting a seed, instance or relation breaks that pattern's association. Remaining
    /// geometry stays in the sketch, without stale metadata that could resurrect a deletion.
    pub fn prune_patterns(&mut self) {
        if self.patterns.is_empty() {
            return;
        }
        let curves: std::collections::HashSet<_> = self.curves.iter().map(|c| c.id.as_str()).collect();
        let points: std::collections::HashSet<_> = self.points.iter().map(|p| p.id.as_str()).collect();
        let constraint_ids: std::collections::HashSet<_> = self.constraints.iter().map(|c| c.id.as_str()).collect();
        let retired: Vec<_> = self
            .patterns
            .iter()
            .filter(|pattern| {
                pattern.entities.iter().any(|id| !curves.contains(id.as_str()) && !points.contains(id.as_str()))
                    || matches!(&pattern.kind, SketchPatternKind::Circular { center, .. } if !points.contains(center.as_str()))
                    || matches!(&pattern.kind, SketchPatternKind::Rectangular { direction, direction2, .. }
                        if direction.iter().chain(direction2).any(|id| !curves.contains(id.as_str())))
                    || pattern.instances.iter().any(|instance| {
                        instance.curves.iter().any(|id| !curves.contains(id.as_str()))
                            || instance.points.iter().any(|(p, q)| !points.contains(p.as_str()) || !points.contains(q.as_str()))
                            || instance.constraints.iter().any(|id| !constraint_ids.contains(id.as_str()))
                    })
            })
            .map(|pattern| pattern.id.clone())
            .collect();
        let constraints: std::collections::BTreeSet<_> =
            self.patterns.iter().filter(|p| retired.contains(&p.id)).flat_map(|p| &p.instances).flat_map(|i| i.constraints.iter().cloned()).collect();
        self.patterns.retain(|p| !retired.contains(&p.id));
        self.constraints.retain(|c| !constraints.contains(&c.id));
    }

    /// Conic rho is an authored shape parameter rather than a solver unknown. Like its
    /// defining points, the copies follow the seed's shape when that parameter is edited.
    pub(crate) fn refresh_pattern_shapes(&mut self) {
        if self.patterns.is_empty() || !self.curves.iter().any(|c| matches!(c.kind, CurveKind::Conic { .. } | CurveKind::Arc { .. })) {
            return;
        }
        // This runs before every solve, drag frames included. Ids resolve through maps built
        // once here: a linear search per copy cost patterns × copies × seeds × curves string
        // comparisons, which dominated dragging anything in a sketch with a large pattern.
        // Only curve shapes change below, so the indices stay valid throughout.
        let (sources, copies): (Vec<Vec<usize>>, Vec<Vec<Vec<Option<usize>>>>) = {
            let curves = first_index(self.curves.iter().map(|c| c.id.as_str()));
            self.patterns
                .iter()
                .map(|pattern| {
                    (
                        pattern.entities.iter().filter_map(|id| curves.get(id.as_str()).copied()).collect(),
                        pattern.instances.iter().map(|i| i.curves.iter().map(|id| curves.get(id.as_str()).copied()).collect()).collect(),
                    )
                })
                .unzip()
        };
        let points = first_index(self.points.iter().map(|p| p.id.as_str()));
        for ((pattern, sources), copies) in self.patterns.iter().zip(&sources).zip(&copies) {
            for (instance, copies) in pattern.instances.iter().zip(copies) {
                for (seed, copy) in sources.iter().zip(copies) {
                    if let Some(CurveKind::Conic { rho, .. }) = self.curves.get(*seed).map(|c| &c.kind)
                        && let Some(copy) = *copy
                    {
                        let rho = *rho;
                        if let Some(CurveKind::Conic { rho: slot, .. }) = self.curves.get_mut(copy).map(|c| &mut c.kind) {
                            *slot = rho;
                        }
                    }
                    let Some(source) = self.curves.get(*seed).cloned() else { continue };
                    if let CurveKind::Arc { c, a, b } = source.kind {
                        let mapped = |p: usize| {
                            self.points
                                .get(p)
                                .and_then(|p| instance.points.iter().find(|(id, _)| *id == p.id))
                                .and_then(|(_, id)| points.get(id.as_str()).copied())
                        };
                        if let (Some(c), Some(a), Some(b), Some(copy)) = (mapped(c), mapped(a), mapped(b), *copy)
                            && let Some(curve) = self.curves.get_mut(copy)
                        {
                            curve.kind = CurveKind::Arc { c, a, b };
                            curve.reversed = source.reversed;
                        }
                    }
                }
            }
        }
    }

    pub fn validate_patterns(&self) -> Result<(), SketchError> {
        if self.patterns.len() > crate::MAX_CONSTRAINTS {
            return Err(SketchError::TooLarge);
        }
        let mut ids = std::collections::BTreeSet::new();
        for pattern in &self.patterns {
            if pattern.expressions.len() > 5
                || pattern.expressions.iter().any(|(key, expression)| {
                    !matches!(key.as_str(), "count" | "count2" | "spacing" | "spacing2" | "angle")
                        || expression.trim().is_empty()
                        || expression.len() > 4096
                })
            {
                return Err(SketchError::Invalid("invalid sketch pattern expression".into()));
            }
            if pattern.count_expression.as_ref().is_some_and(|e| e.trim().is_empty() || e.len() > 4096)
                || (pattern.count_expression.is_some() && !matches!(pattern.kind, SketchPatternKind::Circular { .. }))
            {
                return Err(SketchError::Invalid("invalid circular pattern Quantity expression".into()));
            }
            if pattern.id.is_empty()
                || pattern.id.len() > 512
                || !ids.insert(&pattern.id)
                || pattern.entities.is_empty()
                || pattern.entities.len() > crate::MAX_CURVES + crate::MAX_POINTS
                || pattern.instances.len() >= 500
            {
                return Err(SketchError::Invalid("invalid sketch pattern".into()));
            }
            let valid_count = |n| (1..=500).contains(&n);
            let finite = |v: f64| v.is_finite() && v.abs() <= crate::MAX_COORD;
            let valid = match &pattern.kind {
                SketchPatternKind::Circular { count, angle, center, .. } => {
                    valid_count(*count) && angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= 360.0 && self.resolve_point(center).is_some()
                }
                SketchPatternKind::Rectangular { count, count2, spacing, spacing2, dir, dir2, direction, direction2, .. } => {
                    valid_count(*count)
                        && valid_count(*count2)
                        && count.checked_mul(*count2).is_some_and(|n| n <= 500)
                        && finite(*spacing)
                        && finite(*spacing2)
                        && finite(dir.x)
                        && finite(dir.y)
                        && (dir.len() - 1.0).abs() < 1e-6
                        && dir2.is_none_or(|d| finite(d.x) && finite(d.y) && (d.len() - 1.0).abs() < 1e-6)
                        && direction.iter().chain(direction2).all(|id| {
                            self.curve_index(id).is_some_and(|l| matches!(self.curves.get(l).map(|c| &c.kind), Some(CurveKind::Line { .. })))
                        })
                }
            };
            if !valid {
                return Err(SketchError::Invalid("invalid sketch pattern settings".into()));
            }
            let (n1, n2) = match pattern.kind {
                SketchPatternKind::Circular { count, .. } => (count, 1),
                SketchPatternKind::Rectangular { count, count2, .. } => (count, count2),
            };
            if pattern.instances.len() != n1 * n2 - 1 {
                return Err(SketchError::Invalid("incomplete sketch pattern".into()));
            }
            let mut slots = std::collections::BTreeSet::new();
            for instance in &pattern.instances {
                let [i, j] = instance.index;
                let valid_id = |id: &String| !id.is_empty() && id.len() <= 512;
                if i >= n1
                    || j >= n2
                    || (i == 0 && j == 0)
                    || !slots.insert(instance.index)
                    || instance.points.len() > crate::MAX_POINTS
                    || instance.curves.len() > crate::MAX_CURVES
                    || instance.constraints.len() > crate::MAX_CONSTRAINTS
                    || instance.curves.iter().any(|id| !valid_id(id) || self.curve_index(id).is_none())
                    || instance
                        .points
                        .iter()
                        .any(|(p, q)| !valid_id(p) || !valid_id(q) || self.point_index(p).is_none() || self.point_index(q).is_none())
                    || instance.constraints.iter().any(|id| {
                        !valid_id(id)
                            || !self.constraints.iter().any(|c| {
                                c.id == *id
                                    && matches!(c.kind, crate::ConstraintKind::PatternPoint { .. } | crate::ConstraintKind::PatternRadius { .. })
                            })
                    })
                {
                    return Err(SketchError::Invalid("invalid sketch pattern instance".into()));
                }
            }
        }
        for constraint in &self.constraints {
            if matches!(constraint.kind, crate::ConstraintKind::PatternPoint { .. } | crate::ConstraintKind::PatternRadius { .. }) {
                self.validate(&constraint.kind)?;
            }
        }
        Ok(())
    }
}

/// Keep sampled vector directions compatible with older designs, while picked lines are
/// solver references. The default second direction follows the first line's perpendicular.
fn linear_displacement(sk: &Sketch, kind: &SketchPatternKind, [i, j]: [usize; 2], fallback: Vec2) -> (Vec2, [Option<(usize, Vec2)>; 2]) {
    let SketchPatternKind::Rectangular { dir, dir2, direction, direction2, spacing, spacing2, alignment, alignment2, reversed, reversed2, .. } = kind
    else {
        return (fallback, [None; 2]);
    };
    let mut offset = Vec2::ZERO;
    let mut directions = [None; 2];
    let d1 = *spacing * alignment.multiplier(i, *reversed);
    let d2 = *spacing2 * alignment2.multiplier(j, *reversed2);
    if let Some(l) = direction.as_ref().and_then(|id| sk.curve_index(id)) {
        directions[0] = Some((l, Vec2::new(d1, if dir2.is_none() && direction2.is_none() { d2 } else { 0.0 })));
    } else {
        offset += *dir * d1;
        if dir2.is_none() && direction2.is_none() {
            offset += dir.perp() * d2;
        }
    }
    if let Some(l) = direction2.as_ref().and_then(|id| sk.curve_index(id)) {
        directions[1] = Some((l, Vec2::new(d2, 0.0)));
    } else if let Some(dir2) = dir2 {
        offset += *dir2 * d2;
    }
    (offset, directions)
}

fn involved_points(sk: &Sketch, cs: &[usize], ps: &[usize]) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    for c in cs {
        if let Some(k) = sk.curves.get(*c).map(|c| &c.kind) {
            v.extend(k.point_ids());
        }
    }
    v.extend(ps);
    let mut out = Vec::new();
    for q in v {
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

/// Copy seed curves and their points, returning curves and the original-to-copy point map.
fn copy_through(sk: &mut Sketch, cs: &[usize], ps: &[usize], f: &dyn Fn(Vec2) -> Vec2) -> Result<(Vec<usize>, Vec<(usize, usize)>), SketchError> {
    if sk.curves.len().saturating_add(cs.len()) > crate::MAX_CURVES {
        return Err(crate::SketchError::TooLarge);
    }
    let pts = involved_points(sk, cs, ps);
    let mut map: Vec<(usize, usize)> = Vec::new();
    for q in &pts {
        let pos = sk.point(*q).unwrap_or_default();
        let n = sk.add_point(f(pos), None)?;
        map.push((*q, n));
    }
    let m = |q: usize| map.iter().find(|x| x.0 == q).map(|x| x.1).unwrap_or(q);
    let mut made = Vec::new();
    for c in cs {
        let Some(cu) = sk.curves.get(*c).cloned() else { continue };
        let mut kind = cu.kind.clone();
        kind.map_points(&m);
        let prefix = match &kind {
            CurveKind::Ellipse { .. } => "e",
            CurveKind::Spline { .. } => "s",
            CurveKind::Conic { .. } => "n",
            CurveKind::Line { .. } => "l",
            CurveKind::Circle { .. } => "c",
            CurveKind::Arc { .. } => "a",
        };
        let id = sk.fresh(prefix)?;
        // Copies retain the seed line's extent.
        let infinite = cu.infinite && matches!(kind, CurveKind::Line { .. });
        sk.curves.push(Curve {
            id,
            kind,
            construction: cu.construction,
            reversed: cu.reversed,
            link: None,
            centerline: false,
            fixed: false,
            infinite,
        });
        made.push(sk.curves.len() - 1);
    }
    Ok((made, map))
}

fn rotate(c: Vec2, ang: f64) -> impl Fn(Vec2) -> Vec2 {
    move |q: Vec2| {
        let d = q - c;
        let (s, co) = ang.sin_cos();
        c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
    }
}

/// Each id's first index, as `position` finds it.
fn first_index<'a>(ids: impl Iterator<Item = &'a str>) -> std::collections::HashMap<&'a str, usize> {
    let mut map = std::collections::HashMap::new();
    for (i, id) in ids.enumerate() {
        map.entry(id).or_insert(i);
    }
    map
}
