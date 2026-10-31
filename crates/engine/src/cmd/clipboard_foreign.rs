//! Independent sketch features pasted from the immutable clipboard of another design.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcraft_doc::{Document, Feature, FeatureKind, Mat, PlaneRef};

use crate::params::{bad, vec3};
use crate::{Result, Sel, Session};

const CMD: &str = "timeline.paste";

/// References to geometry outside the copied sketches must never resolve against coincident
/// IDs or names in the destination. Fixed and expression-placed planes are self-contained.
fn independent_plane(plane: &PlaneRef, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(bad(CMD, "copied plane nesting exceeds 64"));
    }
    match plane {
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } => Ok(()),
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } | PlaneRef::AtAngle { base, line: None, .. } => {
            independent_plane(base, depth + 1)
        }
        _ => Err(bad(CMD, "the copied sketch's plane depends on model geometry; convert it to a fixed plane before pasting into another design")),
    }
}

fn independent_links(sketch: &solidcraft_sketch::Sketch, features: &BTreeSet<u64>, moved: bool) -> Result<()> {
    use solidcraft_sketch::LinkSource as L;
    for link in &sketch.links {
        let supported = match &link.source {
            L::Text { .. } | L::SketchAxis { .. } => true,
            L::SketchCurve { sketch, .. } | L::SketchPoint { sketch, .. } => features.contains(sketch),
            L::Origin | L::Axis { .. } => !moved,
            L::Plane { name } => !moved && solidcraft_geom::Plane::named(name).is_some(),
            _ => false,
        };
        if !supported {
            return Err(bad(
                CMD,
                "the copied sketch has external projected geometry; unlink it or copy its source sketches before pasting into another design",
            ));
        }
    }
    Ok(())
}

fn free_parameter_name(base: &str, taken: &mut BTreeSet<String>) -> Result<String> {
    for index in 1..=solidcraft_doc::MAX_PARAMS + 1 {
        let name = if index == 1 { base.to_string() } else { format!("{base}_{index}") };
        if Document::param_name_problem(&name).is_none() && taken.insert(name.clone()) {
            return Ok(name);
        }
    }
    Err(bad(CMD, format!("no free name for copied parameter {base}")))
}

fn model_parameter_name(taken: &mut BTreeSet<String>) -> Result<String> {
    for index in 1..=taken.len() + 1 {
        let name = format!("d{index}");
        if taken.insert(name.clone()) {
            return Ok(name);
        }
    }
    Err(bad(CMD, "no free model parameter name"))
}

/// Bring the complete expression dependency graph, including dimensions driven by another
/// dimension. Use the source definitions' units, never a destination parameter with the same name.
fn parameters(target: &mut Document, source: &Document, features: &[Feature], mapped: &mut BTreeMap<String, String>) -> Result<()> {
    let definitions: BTreeMap<_, _> = source.param_defs().into_iter().map(|p| (p.name.clone(), p)).collect();
    let mut pending: Vec<String> = features.iter().flat_map(|f| f.kind.expressions()).flat_map(solidcraft_doc::expr::references).collect();
    let mut texts = BTreeSet::new();
    for feature in features {
        if let FeatureKind::Sketch { sketch, .. } = &feature.kind {
            pending.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
            let text_and_points = sketch
                .links
                .iter()
                .filter_map(|l| match &l.source {
                    solidcraft_sketch::LinkSource::Text { exprs, .. } => Some(exprs.list()),
                    _ => None,
                })
                .flatten()
                .chain(sketch.points.iter().filter_map(|p| p.exprs.as_deref()).flat_map(solidcraft_sketch::PointExprs::list));
            pending.extend(text_and_points.flat_map(|(_, e)| solidcraft_doc::expr::references(e)));
            for equation in &sketch.equations {
                pending
                    .extend(equation.list().into_iter().flat_map(|(_, e)| solidcraft_doc::expr::references(e)).filter(|n| n != equation.variable()));
            }
            texts.extend(sketch.text_word_exprs().into_iter().flat_map(|e| solidcraft_doc::text_params::references(e)));
        }
    }
    let mut used = BTreeSet::new();
    let mut work = 0;
    while let Some(name) = pending.pop() {
        work += 1;
        if work > 100_000 {
            return Err(bad(CMD, "copied parameter references exceed work limit"));
        }
        if used.insert(name.clone()) {
            let p = definitions.get(&name).ok_or_else(|| bad(CMD, format!("copied sketch uses missing parameter {name}")))?;
            pending.extend(solidcraft_doc::expr::references(&p.expr));
        }
    }
    if target.params.len() + target.text_params.len() + used.len() + texts.len() > solidcraft_doc::MAX_PARAMS {
        return Err(bad(CMD, "too many copied parameters"));
    }
    let mut taken: BTreeSet<_> = target.param_defs().into_iter().map(|p| p.name).chain(target.text_params.iter().map(|p| p.name.clone())).collect();
    for name in &used {
        if mapped.contains_key(name) {
            continue;
        }
        let definition = definitions.get(name).ok_or_else(|| bad(CMD, "missing copied parameter definition"))?;
        let model = source.param(name).is_none_or(|p| p.model);
        let new = if model {
            let new = model_parameter_name(&mut taken)?;
            target.params.push(solidcraft_doc::Param {
                name: new.clone(),
                expr: definition.expr.clone(),
                unit: definition.unit.clone(),
                comment: String::new(),
                model: true,
            });
            new
        } else {
            let new = free_parameter_name(name, &mut taken)?;
            let mut p = source.param(name).cloned().ok_or_else(|| bad(CMD, "missing copied user parameter"))?;
            p.name = new.clone();
            target.params.push(p);
            new
        };
        mapped.insert(name.clone(), new);
    }
    for name in texts {
        let mut p = source.text_param(&name).cloned().ok_or_else(|| bad(CMD, format!("copied sketch uses missing text parameter {name}")))?;
        p.name = free_parameter_name(&name, &mut taken)?;
        mapped.insert(name, p.name.clone());
        target.text_params.push(p);
    }
    for (old, new) in mapped.iter() {
        if let Some(definition) = definitions.get(old)
            && let Some(p) = target.params.iter_mut().find(|p| p.name == *new)
        {
            p.expr = solidcraft_doc::clone_expression(&definition.expr, mapped);
            p.comment = source.param(old).map(|p| p.comment.clone()).or_else(|| source.param_comments.get(old).cloned()).unwrap_or_default();
        }
        if let Some(comment) = source.param_comments.get(old) {
            target.param_comments.insert(new.clone(), comment.clone());
        }
    }
    Ok(())
}

fn text_expression(source: &str, mapped: &BTreeMap<String, String>) -> String {
    let mut result = source.to_string();
    let mut temporary = Vec::new();
    for (i, (old, new)) in mapped.iter().enumerate() {
        let mut token = format!("__paste_text_{i}");
        while source.contains(&token) || mapped.keys().chain(mapped.values()).any(|name| name == &token) {
            token.push('_');
        }
        result = solidcraft_doc::text_params::rename_reference(&result, old, &token);
        temporary.push((token, new));
    }
    for (token, new) in temporary {
        result = solidcraft_doc::text_params::rename_reference(&result, &token, new);
    }
    result
}

pub(super) fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let copy = s.foreign_copy.clone().ok_or_else(|| bad(CMD, "nothing was copied in another design"))?;
    if copy.features.is_empty() || copy.features.len() > 1000 {
        return Err(bad(CMD, "copy 1…1000 sketches before pasting into another design"));
    }
    let component = match p.get("component") {
        None => s.active_component,
        Some(v) => {
            let key = match v {
                Value::String(name) => name.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Err(bad(CMD, "`component` must be a component id or name")),
            };
            s.doc.find_component(&key).ok_or_else(|| bad(CMD, format!("no component `{key}`")))?
        }
    };
    let path = match p.get("component_path") {
        Some(v) => v
            .as_array()
            .filter(|a| a.len() <= 64)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .ok_or_else(|| bad(CMD, "`component_path` must list 0..64 occurrence ids"))?,
        None if component == s.active_component => s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(component)),
        None => super::component::placing::placement_of(s, &s.placed_document(), component, None, "component_path", CMD)?.0,
    };
    let placed = s.placed_document();
    let frame = placed.path_transform(component, &path).ok_or_else(|| bad(CMD, "that path is not a placement of the target"))?;
    let into_target = solidcraft_doc::mat_inverse(&frame).ok_or_else(|| bad(CMD, "the target's frame is not invertible"))?;
    drop(placed);
    let mut shift = solidcraft_doc::IDENTITY;
    if let Some(t) = p.get("translate") {
        let t = vec3(t).ok_or_else(|| bad(CMD, "`translate` must be [x, y, z]"))?;
        shift[3] = [t.x, t.y, t.z, 1.];
    }
    let world_shift = solidcraft_doc::mat_mul(&frame, &solidcraft_doc::mat_mul(&shift, &into_target));
    let mut placed_source = (*copy.doc).clone();
    for occurrence in &mut placed_source.occurrences {
        if let Some(m) = copy.pending_moves.get(&occurrence.id) {
            occurrence.transform = *m;
        }
    }
    let selected: BTreeSet<_> = copy.features.iter().map(|f| f.id).collect();
    let mut transforms: BTreeMap<u64, Mat> = BTreeMap::new();
    for feature in &copy.features {
        let FeatureKind::Sketch { plane, sketch } = &feature.kind else {
            return Err(bad(CMD, "only sketch features can be pasted between designs; copy a component to include other feature history"));
        };
        independent_plane(plane, 0)?;
        let source_frame = placed_source
            .path_transform(feature.component, &placed_source.first_path(feature.component))
            .ok_or_else(|| bad(CMD, "the copied sketch's component placement is missing"))?;
        let transform = solidcraft_doc::mat_mul(&shift, &solidcraft_doc::mat_mul(&into_target, &source_frame));
        independent_links(sketch, &selected, !solidcraft_doc::is_identity(&transform))?;
        transforms.insert(feature.id, transform);
    }
    let mut scratch = s.scratch();
    let mut mapped = BTreeMap::new();
    let mut ids = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut made = Vec::new();
    for feature in &copy.features {
        let taken = |name: &str| scratch.doc.features.iter().any(|f| f.name == name);
        let name = if taken(&feature.name) { solidcraft_doc::copy_name(&feature.name, taken) } else { feature.name.clone() };
        let id = scratch.doc_mut().add_feature(feature.kind.clone(), Some(&name))?;
        let mut taken: BTreeSet<_> =
            scratch.doc.param_defs().into_iter().map(|p| p.name).chain(scratch.doc.text_params.iter().map(|p| p.name.clone())).collect();
        let text_names: BTreeSet<_> = scratch.doc.text_params.iter().map(|p| p.name.clone()).collect();
        if let Some(added) = scratch.doc_mut().feature_mut(id) {
            for name in &mut added.param_names {
                if text_names.contains(name) {
                    *name = model_parameter_name(&mut taken)?;
                }
            }
        }
        if let Some(added) = scratch.doc.feature(id) {
            mapped.extend(feature.param_names.iter().cloned().zip(added.param_names.iter().cloned()));
        }
        ids.insert(feature.id, id);
        names.insert(feature.name.clone(), name.clone());
        made.push(json!({"id": id, "name": name, "from": feature.name}));
    }
    parameters(scratch.doc_mut(), &copy.doc, &copy.features, &mut mapped)?;
    let source_values = copy.doc.param_values().0;
    for original in &copy.features {
        let mut feature = original.clone();
        let transform = transforms.get(&original.id).ok_or_else(|| bad(CMD, "missing copied sketch transform"))?;
        // Nonliteral unitless length inputs are interpreted in their design's display units.
        // Make that unit explicit before moving the expressions to a design with other units.
        if copy.doc.units != s.doc.units {
            for (_, e, kind) in feature.kind.inputs_mut() {
                if kind == solidcraft_doc::expr::Kind::Length
                    && !solidcraft_doc::expr::is_literal(e)
                    && let Some((solidcraft_doc::expr::Kind::Length, scale)) = solidcraft_doc::expr::unit_info(copy.doc.units.trim())
                    && solidcraft_doc::expr::eval_with(e, &|name| {
                        source_values.get(name).copied().ok_or_else(|| solidcraft_doc::DocError::Expr(format!("unknown parameter {name}")))
                    })?
                    .unitless()
                {
                    *e = format!("({e}) * {scale} mm").into();
                }
            }
        }
        feature.remap_clone(&ids, &names, &BTreeMap::new(), &mapped)?;
        if let FeatureKind::Sketch { plane, sketch } = &mut feature.kind {
            // 3D wires carry their own coordinate space, independently of the sketch plane.
            for wire in &mut sketch.wires {
                let matrix = if wire.space == solidcraft_sketch::WireSpace::Component { transform } else { &world_shift };
                solidcraft_doc::map_wire(wire, matrix);
            }
            // Patterns and text words have expressions outside FeatureKind's numeric inputs.
            for pattern in &mut sketch.patterns {
                for e in pattern.count_expression.iter_mut().chain(pattern.expressions.values_mut()) {
                    *e = solidcraft_doc::clone_expression(e, &mapped);
                }
            }
            for e in sketch.text_word_exprs_mut() {
                *e = text_expression(e, &mapped);
            }
            if !solidcraft_doc::is_identity(transform) {
                super::clipboard::explicit_planes(plane);
            }
        }
        feature.kind.transform_geometry(transform);
        let id = *ids.get(&original.id).ok_or_else(|| bad(CMD, "missing copied sketch id"))?;
        let added = scratch.doc_mut().feature_mut(id).ok_or_else(|| bad(CMD, "missing pasted sketch"))?;
        added.kind = feature.kind;
        added.component = component;
        added.suppressed = original.suppressed;
    }
    scratch.refresh();
    for id in ids.values() {
        if let Some(error) = scratch.model.result(*id).and_then(|r| r.error.as_ref()) {
            return Err(bad(CMD, format!("the pasted sketch did not rebuild: {error}")));
        }
    }
    scratch.selection = ids.values().map(|id| Sel::Feature { id: *id }).collect();
    s.accept_computed(scratch);
    Ok(json!({"pasted": made, "errors": [], "notices": []}))
}
