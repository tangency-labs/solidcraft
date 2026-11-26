//! Persistent identity of selectable source openings. No display geometry decides identity.
use super::*;
use std::collections::BTreeSet;

pub(super) fn body_key(body: &str, path: &[u64]) -> PatternMemberKey {
    PatternMemberKey::Stable(PatternMemberIdentity::Body { body: body.into(), path: path.to_vec() })
}

pub(super) fn hole_keys(st: &ModelState, src: &Feature) -> Vec<Option<PatternMemberKey>> {
    let FeatureKind::Hole { points, .. } = &src.kind else { return Vec::new() };
    match points {
        Some(points) => {
            let Some(ss) = st.sketch(points.sketch.0) else { return Vec::new() };
            points
                .ids
                .iter()
                .take(if points.single { 1 } else { usize::MAX })
                .filter(|id| ss.sketch.points.iter().any(|point| &point.id == *id))
                .take(10_000)
                .map(|id| Some(PatternMemberKey::Stable(PatternMemberIdentity::SketchPoint { sketch: points.sketch, point: id.clone() })))
                .collect()
        }
        None => vec![Some(PatternMemberKey::Stable(PatternMemberIdentity::Single))],
    }
}

fn role_curves(lineage: &crate::lineage::Lineage, curves: &mut BTreeSet<String>) {
    for operand in &lineage.operands {
        match operand.as_ref() {
            crate::lineage::Operand::Made(faces) => {
                curves.extend(faces.iter().flatten().filter_map(|role| role.strip_prefix("side:").map(str::to_owned)));
            }
            crate::lineage::Operand::Stage(stage) => role_curves(&stage.lineage, curves),
            _ => {}
        }
    }
}

pub(super) fn source_keys(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, src: &Feature) -> Result<Vec<Option<PatternMemberKey>>> {
    match &src.kind {
        FeatureKind::Hole { .. } => Ok(hole_keys(st, src)),
        FeatureKind::Fillet { .. } | FeatureKind::Chamfer { .. } | FeatureKind::OffsetFace { .. } | FeatureKind::Thread { .. } => {
            Ok(vec![Some(PatternMemberKey::Stable(PatternMemberIdentity::Single))])
        }
        FeatureKind::Move { copy: true, .. } | FeatureKind::FrameMember { .. } => {
            Ok(st.bodies.iter().filter(|body| body.feature == src.id).map(|body| Some(body_key(&body.name, &[]))).collect())
        }
        FeatureKind::Rib { .. } => Ok(vec![None; more::rib_tools(vals, st, src, None)?.len()]),
        _ => {
            let moved = pattern_source_at_faces(doc, vals, st, st, src)?;
            let tools = feature_tools_with_constructor(doc, vals, moved.as_ref().unwrap_or(src), st, &mut None, &mut Vec::new(), &mut None, false)?;
            Ok(tool_keys(st, src, &tools))
        }
    }
}

/// Exact constructor roles must match one complete source boundary. Partial intersection
/// fragments cannot identify a profile persistently, even when their curve set is currently
/// unique. Distinct tools must also have distinct identities before either can be selected.
pub(super) fn tool_keys(st: &ModelState, src: &Feature, tools: &[Traced]) -> Vec<Option<PatternMemberKey>> {
    let sketch = match &src.kind {
        FeatureKind::Extrude { sketch, thin: None, .. } | FeatureKind::Revolve { sketch, .. } | FeatureKind::Sweep { sketch, .. } => Some(*sketch),
        _ => None,
    };
    if let Some(sketch) = sketch {
        let Some(ss) = st.sketch(sketch.0) else { return vec![None; tools.len()] };
        let mut signatures = BTreeMap::<BTreeSet<String>, usize>::new();
        for profile in &ss.profiles {
            if let Some((outer, holes)) = profile.whole_curve_boundaries() {
                let curves = outer.into_iter().chain(holes.into_iter().flatten()).collect();
                *signatures.entry(curves).or_default() += 1;
            }
        }
        let mut keys: Vec<_> = tools
            .iter()
            .map(|tool| {
                let mut curves = BTreeSet::new();
                role_curves(tool.lineage.as_ref()?, &mut curves);
                if curves.is_empty() || curves.len() > 512 || curves.iter().any(|curve| curve.len() > 512) {
                    return None;
                }
                (signatures.get(&curves) == Some(&1))
                    .then(|| PatternMemberKey::Stable(PatternMemberIdentity::Profile { sketch, curves: curves.into_iter().collect() }))
            })
            .collect();
        let mut counts = BTreeMap::new();
        for key in keys.iter().flatten() {
            *counts.entry(key.clone()).or_insert(0usize) += 1;
        }
        for key in &mut keys {
            if key.as_ref().is_some_and(|key| counts.get(key).is_some_and(|count| *count > 1)) {
                *key = None;
            }
        }
        return keys;
    }
    // Only constructors that semantically produce one whole tool have a singleton identity.
    // A profile constructor must never acquire it simply because one profile remains.
    if tools.len() == 1
        && matches!(
            src.kind,
            FeatureKind::Box { .. }
                | FeatureKind::Cylinder { .. }
                | FeatureKind::Sphere { .. }
                | FeatureKind::Cone { .. }
                | FeatureKind::Torus { .. }
                | FeatureKind::Loft { .. }
                | FeatureKind::Pipe { .. }
                | FeatureKind::Coil { sketch: None, .. }
        )
    {
        vec![Some(PatternMemberKey::Stable(PatternMemberIdentity::Single))]
    } else {
        vec![None; tools.len()]
    }
}

/// Addresses and semantic source identities available in the current layout. Group choices
/// are independent of the current source tools, so suppressing a source does not erase them.
pub fn pattern_member_catalogue(
    doc: &Document,
    st: &ModelState,
    component: u64,
    pattern: &PatternKind,
    features: &[FeatureId],
    bodies: &[crate::BodyName],
    placements: &[Vec<u64>],
) -> Result<crate::PatternCatalogue> {
    let vals = doc.evaluation_values();
    let transforms = pattern_transforms(doc, &vals, st, component, pattern)?;
    let slots = pattern_slots(&vals, &transforms, Some(pattern), FeatureId::DANGLING, true)?;
    let groups = slots.iter().map(|slot| crate::PatternGroupChoice { number: slot.number, address: slot.address(0) }).collect();
    const MAX_CHOICES: usize = 100_000;
    let mut members = Vec::new();
    let mut unavailable_members = 0usize;
    if !bodies.is_empty() {
        if slots.len().checked_mul(bodies.len()).is_none_or(|count| count > MAX_CHOICES) {
            return Err(DocError::Invalid("too many selectable pattern members (at most 100000)".into()));
        }
        for slot in &slots {
            for (index, body) in bodies.iter().enumerate().filter(|(_, body)| st.body(body).is_some()) {
                let key = body_key(body, placements.get(index).map_or(&[], Vec::as_slice));
                members.push(crate::PatternMemberChoice {
                    member: PatternMember(slot.address(0), FeatureId::DANGLING, key),
                    index,
                    number: slot.number,
                });
            }
        }
    } else {
        if slots.len().checked_mul(features.len()).is_none_or(|count| count > MAX_CHOICES) {
            return Err(DocError::Invalid("too many selectable pattern members (at most 100000)".into()));
        }
        for (src, copies, _) in copy_plan(doc, &vals, st, component, features, &transforms, Some(pattern), true, Some(MAX_CHOICES))? {
            if copies.is_empty() {
                continue;
            }
            let keys = source_keys(doc, &vals, st, src)?;
            if copies.len().checked_mul(keys.len()).and_then(|count| count.checked_add(members.len())).is_none_or(|count| count > MAX_CHOICES) {
                return Err(DocError::Invalid("too many selectable pattern members (at most 100000)".into()));
            }
            unavailable_members = unavailable_members.saturating_add(copies.len().saturating_mul(keys.iter().filter(|key| key.is_none()).count()));
            for copy in &copies {
                for (index, key) in keys.iter().enumerate() {
                    if let Some(key) = key.as_ref().filter(|key| copy.keeps(Some(key))) {
                        members.push(crate::PatternMemberChoice {
                            member: PatternMember(copy.slot.address(0), FeatureId(src.id), key.clone()),
                            index,
                            number: copy.slot.number,
                        });
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    members.retain(|choice| seen.insert(choice.member.clone()));
    Ok(crate::PatternCatalogue { groups, members, unavailable_members })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use solidcraft_sketch::Sketch;

    #[test]
    fn distinct_tools_with_the_same_whole_profile_identity_are_not_selectable() {
        let mut doc = Document::new("duplicate source identity");
        let mut sketch = Sketch::new();
        sketch.add_circle(Vec2::ZERO, 2.0, None, Some("circle")).unwrap();
        let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let source = doc
            .add_feature(
                serde_json::from_value(
                    serde_json::json!({"type":"extrude","sketch":sketch,"profiles":{"type":"all"},"extent":{"distance":"2"},"operation":"new_body"}),
                )
                .unwrap(),
                None,
            )
            .unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|result| result.error.is_none()));
        let state = model.state();
        let source = doc.feature(source).unwrap();
        let (values, _) = doc.param_values();
        let mut tools = feature_tools_with_constructor(&doc, &values, source, &state, &mut None, &mut Vec::new(), &mut None, false).unwrap();
        assert_eq!(tools.len(), 1);
        assert!(tool_keys(&state, source, &tools)[0].is_some(), "an ordinary complete circle remains selectable");
        // Two separated constructor outputs carry the same profile provenance. Without a
        // further source identity, choosing either must not omit both tools silently.
        let mut second = tools[0].clone();
        second.body = kernel::transform_matrix(&second.body, translation(Vec3::new(20.0, 0.0, 0.0))).unwrap();
        second.recipe = None;
        tools.push(second);
        assert!(tool_keys(&state, source, &tools).iter().all(Option::is_none));
    }
}
