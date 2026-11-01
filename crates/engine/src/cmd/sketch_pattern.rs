//! Associative sketch patterns. Editing retains ids at surviving instance indices.

use solidcraft_sketch::{SketchPattern, SketchPatternKind};

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
        let mut expressions = old.as_ref().map(|old| old.expressions.clone()).unwrap_or_default();
        for key in ["count", "count2", "spacing", "spacing2", "angle"] {
            if p.get(key).is_some() {
                expressions.remove(key);
            }
            if let Some(Value::String(e)) = p.get(key) {
                expressions.insert(key.into(), e.clone());
            }
        }
        for (key, expression) in &expressions {
            let kind = match key.as_str() {
                "count" | "count2" => solidcraft_doc::expr::Kind::Unitless,
                "angle" => solidcraft_doc::expr::Kind::Angle,
                _ => solidcraft_doc::expr::Kind::Length,
            };
            let value = doc.eval(expression, kind).map_err(|e| bad(cmd, format!("`{key}`: {e}")))?;
            params[key] = json!(if key == "angle" { value.to_degrees() } else { value });
        }
        if !circular
            && params.get("spacing2").is_none()
            && let Some(e) = expressions.get("spacing").cloned()
        {
            expressions.insert("spacing2".into(), e);
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
            let expression =
                expressions.remove("count").or_else(|| old.as_ref().and_then(|p| p.count_expression.clone()).filter(|_| p.get("count").is_none()));
            if let Some(expression) = &expression {
                let value = doc.eval(expression, solidcraft_doc::expr::Kind::Unitless)?;
                params["count"] = json!(SketchPattern::circular_quantity(value).map_err(|e| bad(cmd, format!("`count`: {e}")))?);
            }
            expression
        } else {
            None
        };
        let n = count(&params, "count", cmd)?;
        let (kind, center) = if circular {
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
            let alignment = alignment_param(&params, "alignment", cmd)?;
            let reversed = reversed_param(&params, "reversed", cmd)?;
            (SketchPatternKind::Circular { center: id, count: n, angle: total, alignment, reversed }, Some(center))
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
            )
        };
        let id = match &old {
            Some(old) => old.id.clone(),
            None => sk.fresh("pat")?,
        };
        let center_id = center.and_then(|i| sk.points.get(i)).map(|p| p.id.clone());
        let index = old_index.unwrap_or(sk.patterns.len());
        let pattern = SketchPattern {
            id: id.clone(),
            entities: entity_ids,
            kind: kind.clone(),
            instances: old.map(|old| old.instances).unwrap_or_default(),
            expressions,
            count_expression,
        };
        sk.patterns.insert(index.min(sk.patterns.len()), pattern);
        sk.update_pattern(index, kind)?;
        let pattern = sk.patterns.get(index).ok_or_else(|| bad(cmd, "missing sketch pattern"))?;
        let curves: Vec<_> = pattern.instances.iter().flat_map(|i| i.curves.iter().cloned()).collect();
        let constraints: Vec<_> = pattern.instances.iter().flat_map(|i| i.constraints.iter().cloned()).collect();
        Ok(json!({"pattern":id,"center":center_id,"curves":curves,"constraints":constraints}))
    })?;
    let mut out = out;
    if let Some(out) = out.as_object_mut() {
        out.insert("sketch".into(), info);
    }
    Ok(out)
}

fn count(p: &Value, k: &str, cmd: &str) -> Result<usize> {
    let n = num(p, k).ok_or_else(|| bad(cmd, format!("`{k}` must be a number or expression")))?;
    if !(1.0..=MAX_COPIES as f64).contains(&n) || n.fract() != 0.0 {
        return Err(bad(cmd, format!("`{k}` must be a positive integer from 1 to {MAX_COPIES}")));
    }
    Ok(n as usize)
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
