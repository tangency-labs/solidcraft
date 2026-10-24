//! Associative sketch patterns. Editing retains ids at surviving instance indices.

use solidcraft_sketch::{PatternInstance, SketchPattern, SketchPatternKind};

use super::*;
use crate::cmd::{alignment_param, reversed_param};

pub(super) fn run(s: &mut Session, p: &Value, circular: bool) -> Result<Value> {
    let cmd = if circular { "sketch.pattern.circular" } else { "sketch.pattern.rectangular" };
    let (out, info) = edit(s, p, cmd, true, |sk, doc| {
        if sk.patterns.len() >= solidcraft_sketch::MAX_CONSTRAINTS {
            return Err(bad(cmd, "too many sketch patterns"));
        }
        let old_index = str_(p, "pattern").and_then(|id| sk.patterns.iter().position(|x| x.id == id));
        let old = if let Some(id) = str_(p, "pattern") {
            let i = sk.patterns.iter().position(|x| x.id == id).ok_or_else(|| bad(cmd, "unknown sketch pattern"))?;
            Some(sk.patterns.remove(i))
        } else {
            None
        };
        let mut params = p.clone();
        if let Some(old) = &old {
            if circular != matches!(old.kind, SketchPatternKind::Circular { .. }) {
                return Err(bad(cmd, "use the existing pattern's type"));
            }
            let defaults = match &old.kind {
                SketchPatternKind::Circular { center, count, angle, alignment, reversed } => {
                    json!({"entities":old.entities,"center":center,"count":old.count_expression.as_ref().map_or_else(|| json!(count), |e| json!(e)),"angle":angle,"alignment":alignment,"reversed":reversed})
                }
                SketchPatternKind::Rectangular {
                    dir,
                    dir2,
                    direction,
                    direction2,
                    count,
                    spacing,
                    count2,
                    spacing2,
                    alignment,
                    alignment2,
                    reversed,
                    reversed2,
                } => {
                    json!({"entities":old.entities,"dir":dir,"dir2":dir2,"direction":direction,"direction2":direction2,"count":count,"spacing":spacing,"count2":count2,"spacing2":spacing2,"alignment":alignment,"alignment2":alignment2,"reversed":reversed,"reversed2":reversed2})
                }
            };
            if let (Some(params), Some(defaults)) = (params.as_object_mut(), defaults.as_object()) {
                for (key, value) in defaults {
                    params.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        let (mut cs, mut ps) = entities(sk, &params, cmd)?;
        cs.sort_unstable();
        cs.dedup();
        ps.sort_unstable();
        ps.dedup();
        let entity_ids: Vec<String> = cs
            .iter()
            .filter_map(|i| sk.curves.get(*i).map(|c| c.id.clone()))
            .chain(ps.iter().filter_map(|i| sk.points.get(*i).map(|p| p.id.clone())))
            .collect();
        if let Some(old) = &old {
            if entity_ids != old.entities {
                return Err(bad(cmd, "the seed entities of an existing pattern cannot be changed"));
            }
            if old.instances.len() >= MAX_COPIES {
                return Err(bad(cmd, "too many stored pattern instances"));
            }
            if old.instances.iter().any(|instance| {
                instance.curves.iter().any(|id| sk.curve_index(id).is_none())
                    || instance.points.iter().any(|(p, q)| sk.resolve_point(p).is_none() || sk.resolve_point(q).is_none())
                    || instance.constraints.iter().any(|id| !sk.constraints.iter().any(|c| c.id == *id))
            }) {
                return Err(bad(cmd, "the pattern has deleted geometry or relations; undo the deletion before editing it"));
            }
        }
        let count_expression = if circular {
            match params.get("count") {
                Some(Value::String(expression)) => {
                    let expression = expression.trim().to_string();
                    let value = doc.eval(&expression, solidcraft_doc::expr::Kind::Unitless)?;
                    params["count"] = json!(SketchPattern::circular_quantity(value)?);
                    Some(expression)
                }
                _ => None,
            }
        } else {
            None
        };
        let n = count(&params, "count", cmd)?;
        if count_expression.is_some() && n < 2 {
            return Err(bad(cmd, "circular pattern Quantity must be 2…500"));
        }
        let (kind, center, slots) = if circular {
            let total = if params.get("angle").is_some() { num(&params, "angle").ok_or_else(|| bad(cmd, "`angle` must be finite"))? } else { 360.0 };
            if !(total.abs() > 1e-9 && total.abs() <= 360.0) {
                return Err(bad(cmd, "`angle` must be nonzero and at most 360 degrees"));
            }
            let center = match params.get("center") {
                Some(Value::String(id)) => sk.resolve_point(id).ok_or_else(|| bad(cmd, "unknown pattern center point"))?,
                Some(value) => {
                    let pos = vec2(value).ok_or_else(|| bad(cmd, "`center` must be [x,y] or a point ref"))?;
                    // A centre placed in empty space is a real point that can subsequently
                    // be made coincident with a circle centre or dimensioned.
                    sk.add_point(pos, None)?
                }
                None => return Err(bad(cmd, "missing `center`")),
            };
            let id = sk.points.get(center).ok_or_else(|| bad(cmd, "missing center point"))?.id.clone();
            let full = (total.abs() - 360.0).abs() < 1e-9;
            let step = total / if full { n as f64 } else { (n.max(2) - 1) as f64 };
            let alignment = alignment_param(&params, "alignment", cmd)?;
            let reversed = reversed_param(&params, "reversed", cmd)?;
            (
                SketchPatternKind::Circular { center: id, count: n, angle: total, alignment, reversed },
                Some(center),
                (1..n).map(|i| ([i, 0], (step * alignment.multiplier(i, reversed)).to_radians(), Vec2::ZERO)).collect::<Vec<_>>(),
            )
        } else {
            let spacing = num(&params, "spacing").ok_or_else(|| bad(cmd, "`spacing` must be finite"))?;
            let count2 = if params.get("count2").is_some() { count(&params, "count2", cmd)? } else { 1 };
            let spacing2 = if params.get("spacing2").is_some() {
                num(&params, "spacing2").ok_or_else(|| bad(cmd, "`spacing2` must be finite"))?
            } else {
                spacing
            };
            if n * count2 > MAX_COPIES {
                return Err(bad(cmd, "too many copies"));
            }
            let dir = match params.get("dir") {
                Some(value) => vec2(value).and_then(Vec2::normalized).ok_or_else(|| bad(cmd, "`dir` must be a finite nonzero vector"))?,
                None => Vec2::X,
            };
            let dir2 = match params.get("dir2") {
                Some(value) if !value.is_null() => {
                    Some(vec2(value).and_then(Vec2::normalized).ok_or_else(|| bad(cmd, "`dir2` must be a finite nonzero vector"))?)
                }
                _ => None,
            };
            let direction = direction_ref(sk, &params, "direction", cmd)?;
            let direction2 = direction_ref(sk, &params, "direction2", cmd)?;
            let alignment = alignment_param(&params, "alignment", cmd)?;
            let alignment2 = alignment_param(&params, "alignment2", cmd)?;
            let reversed = reversed_param(&params, "reversed", cmd)?;
            let reversed2 = reversed_param(&params, "reversed2", cmd)?;
            let first = line_direction(sk, direction.as_deref()).unwrap_or(dir);
            let second = line_direction(sk, direction2.as_deref()).or(dir2).unwrap_or_else(|| first.perp());
            let slots = (0..n)
                .flat_map(|i| {
                    (0..count2).filter_map(move |j| {
                        (i != 0 || j != 0).then_some((
                            [i, j],
                            0.0,
                            first * (spacing * alignment.multiplier(i, reversed)) + second * (spacing2 * alignment2.multiplier(j, reversed2)),
                        ))
                    })
                })
                .collect();
            (
                SketchPatternKind::Rectangular {
                    dir,
                    dir2,
                    direction,
                    direction2,
                    count: n,
                    spacing,
                    count2,
                    spacing2,
                    alignment,
                    alignment2,
                    reversed,
                    reversed2,
                },
                None,
                slots,
            )
        };
        let id = match &old {
            Some(old) => old.id.clone(),
            None => sk.fresh("pat")?,
        };
        let center_id = center.and_then(|i| sk.points.get(i)).map(|p| p.id.clone());
        let mut existing = old.map(|old| old.instances).unwrap_or_default();
        // Remove retired instances first: all following indices are resolved by stable id.
        for instance in existing.iter().filter(|instance| !slots.iter().any(|(index, _, _)| *index == instance.index)) {
            let curves: Vec<_> = instance.curves.iter().filter_map(|id| sk.curve_index(id)).collect();
            sk.constraints.retain(|c| !instance.constraints.contains(&c.id));
            sk.remove_curves(&curves);
            let points: Vec<_> = instance.points.iter().filter_map(|(_, id)| sk.resolve_point(id)).collect();
            sk.remove_points(&points);
        }
        let mut instances = Vec::new();
        let center = center_id.as_ref().and_then(|id| sk.resolve_point(id));
        if center_id.is_some() && center.is_none() {
            return Err(bad(cmd, "the center belongs to an instance removed by this edit"));
        }
        if !circular {
            // A direction can have belonged to an instance retired above. Resolve it again
            // before creating relations so an edit never silently substitutes a vector.
            direction_ref(sk, &params, "direction", cmd)?;
            direction_ref(sk, &params, "direction2", cmd)?;
        }
        let mut curves = Vec::new();
        let mut constraints = Vec::new();
        for (index, angle, offset) in slots {
            let (base_offset, directions) = linear_displacement(sk, &kind, index, offset);
            let c = center.and_then(|i| sk.point(i)).unwrap_or(Vec2::ZERO);
            let transform = |p| rotate(c, angle)(p) + offset;
            let instance = if let Some(i) = existing.iter().position(|instance| instance.index == index) {
                let instance = existing.remove(i);
                for cid in &instance.constraints {
                    if let Some(constraint) = sk.constraints.iter_mut().find(|constraint| constraint.id == *cid)
                        && let ConstraintKind::PatternPoint { center: slot, angle: a, offset: off, directions: dirs, .. } = &mut constraint.kind
                    {
                        *slot = center;
                        *a = angle;
                        *off = base_offset;
                        *dirs = directions;
                    }
                }
                for (seed, copy) in &instance.points {
                    let pos = sk.resolve_point(seed).and_then(|i| sk.point(i)).ok_or_else(|| bad(cmd, "missing seed point"))?;
                    if let Some(point) = sk.resolve_point(copy).and_then(|i| sk.points.get_mut(i)) {
                        point.pos = transform(pos);
                    }
                }
                instance
            } else {
                let cs: Vec<_> = entity_ids.iter().filter_map(|id| sk.curve_index(id)).collect();
                let ps: Vec<_> = entity_ids.iter().filter(|id| sk.curve_index(id).is_none()).filter_map(|id| sk.resolve_point(id)).collect();
                let (made, map) = copy_through(sk, &cs, &ps, &transform, false, &|_| false, 1.0)?;
                let mut cons = Vec::new();
                for (p, q) in &map {
                    cons.push(add_c(sk, ConstraintKind::PatternPoint { p: *p, q: *q, center, angle, offset: base_offset, directions })?);
                }
                for (a, b) in cs.iter().zip(&made) {
                    if matches!(sk.curves.get(*a).map(|c| &c.kind), Some(CurveKind::Circle { .. } | CurveKind::Ellipse { .. })) {
                        cons.push(add_c(sk, ConstraintKind::PatternRadius { a: *a, b: *b })?);
                    }
                }
                PatternInstance {
                    index,
                    curves: ids_of(sk, &made),
                    points: map.iter().filter_map(|(p, q)| Some((sk.points.get(*p)?.id.clone(), sk.points.get(*q)?.id.clone()))).collect(),
                    constraints: cons,
                }
            };
            curves.extend(instance.curves.iter().cloned());
            constraints.extend(instance.constraints.iter().cloned());
            instances.push(instance);
        }
        let center_id = center.and_then(|i| sk.points.get(i)).map(|p| p.id.clone());
        let pattern = SketchPattern { id: id.clone(), entities: entity_ids, kind, count_expression, instances };
        if let Some(i) = old_index {
            sk.patterns.insert(i.min(sk.patterns.len()), pattern);
        } else {
            sk.patterns.push(pattern);
        }
        Ok(json!({"pattern":id,"center":center_id,"curves":curves,"constraints":constraints}))
    })?;
    let mut out = out;
    if let Some(out) = out.as_object_mut() {
        out.insert("sketch".into(), info);
    }
    Ok(out)
}

fn direction_ref(sk: &Sketch, p: &Value, key: &str, cmd: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) if line_direction(sk, Some(id)).is_some() => Ok(Some(id.clone())),
        _ => Err(bad(cmd, format!("`{key}` must reference a nondegenerate sketch line"))),
    }
}

fn line_direction(sk: &Sketch, id: Option<&str>) -> Option<Vec2> {
    let Shape::Line { a, b } = sk.shape(sk.curve_index(id?)?)? else { return None };
    (b - a).normalized()
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
