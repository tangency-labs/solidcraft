//! Independent translation of cadmpeg's Apache-2.0 neutral Inventor part records.
//! Unsupported or incomplete history is refused as a whole so the caller can offer the
//! geometry import instead.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeStart, FeatureDefinition, FeatureOperation, LinearTermination, ParameterValue,
    PlanarProfileRef, ProfileRef, SketchProfileRegion,
};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput as Relation, SketchGeometryDefinition as Geometry, SketchLocus};
use cadmpeg_ir::{CadIr, SketchEntityId};
use solidcraft_doc::sketch::{ConstraintKind, CurveKind, Sketch};
use solidcraft_doc::{Direction, Document, Expr, Extent, FeatureKind, Operation, PlaneRef, ProfileSel, SketchId};
use solidcraft_geom::{Plane, Vec2, Vec3};

type ReadResult<T> = std::result::Result<T, String>;

/// Inventor part files (`.ipt`) have editable-history support; assemblies (`.iam`) use the
/// geometry importer.
pub fn is_inventor_part_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("ipt"))
}

/// Rebuild the fully decoded, supported part history as SolidCraft features.
/// cadmpeg supplies canonical millimetres/radians, not Inventor's stored centimetres.
pub fn import_inventor(bytes: &[u8], file: &str) -> crate::Result<(Document, Vec<String>)> {
    let result = solidcraft_kernel::guard("Inventor history import", || read(bytes, file).map_err(solidcraft_kernel::KernelError::Invalid));
    result.map_err(|e| crate::IoError::Invalid(format!("{file}: {e}")))
}

fn read(bytes: &[u8], file: &str) -> ReadResult<(Document, Vec<String>)> {
    if !is_inventor_part_path(file) {
        return Err("editable history requires an Inventor .ipt part".into());
    }
    let (_, decoded) = crate::native_cad::convert::decode(file, "ipt", bytes, &["inventor"], DecodeDepth::Full)?;
    // A partial design graph must not masquerade as a complete editable part.
    let mut warnings = Vec::new();
    for loss in &decoded.report().losses {
        let code = loss.code.to_string();
        if ["design.", "sketch.", "feature.", "parameter."].iter().any(|family| code.contains(family)) {
            return Err(format!("incomplete Inventor history: {code}: {}", loss.message));
        }
        warnings.push(format!("{code}: {}", loss.message));
    }
    let doc = rebuild(decoded.ir(), file)?;
    warnings.push("Feature extents retain decoded values; source parameter-to-feature bindings are not inferred.".into());
    Ok((doc, warnings))
}

pub(crate) fn rebuild(ir: &CadIr, file: &str) -> ReadResult<Document> {
    if ir.model.features.is_empty() || ir.model.features.len() > solidcraft_doc::MAX_FEATURES {
        return Err("no complete supported feature history, or too many features".into());
    }
    if ir.model.configurations.len() > 1 || !ir.model.occurrences.is_empty() {
        return Err("multiple model states and assembly histories are not supported".into());
    }
    let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("Import");
    let mut doc = Document::new(stem);
    parameters(ir, &mut doc)?;
    let component = doc.add_component(Some(stem), 0).map_err(|e| e.to_string())?;
    let mut ordered: Vec<_> = ir.model.features.iter().collect();
    ordered.sort_by_key(|f| f.ordinal);
    let mut seen = BTreeSet::new();
    let mut ordinals = BTreeSet::new();
    let mut sketches = BTreeMap::new();
    let mut bodies = 0usize;
    for feature in ordered {
        if !seen.insert(feature.id.as_str()) || !ordinals.insert(feature.ordinal) {
            return Err("duplicate feature identity or history position".into());
        }
        if feature.dependencies.iter().any(|id| id == &feature.id || !seen.contains(id.as_str())) {
            return Err(format!("feature {:?} has unresolved or forward dependencies", feature.name));
        }
        let suppressed = feature.suppressed.ok_or("feature suppression state is unresolved")?;
        let FeatureDefinition::Operation(operation) = feature.evaluation.definition() else {
            return Err("feature post-processing is not supported".into());
        };
        let kind = match operation {
            FeatureOperation::TreeNode { .. } => continue,
            FeatureOperation::Sketch { sketch } => {
                let id = sketch.id().ok_or("sketch geometry is unresolved")?;
                if sketches.contains_key(id.as_str()) {
                    return Err("a sketch occurs more than once in the history".into());
                }
                let source = ir.model.sketches.iter().find(|s| &s.id == id).ok_or("sketch is missing")?;
                let (plane, sketch, curves) = sketch_model(ir, source)?;
                let index = doc
                    .add_feature(FeatureKind::Sketch { plane: PlaneRef::Custom { plane }, sketch: sketch.clone() }, feature.name.as_deref())
                    .map_err(|e| e.to_string())?;
                let imported = doc.feature_mut(index).ok_or("created sketch is missing")?;
                imported.component = component;
                imported.suppressed = suppressed;
                sketches.insert(id.as_str(), (index, plane, sketch, curves));
                continue;
            }
            FeatureOperation::Extrude {
                profile,
                direction,
                start,
                extent,
                op,
                solid,
                inner_wire_taper,
                face_maker,
                length_along_profile_normal,
                allow_multi_profile_faces,
            } => {
                if *solid != Some(true)
                    || inner_wire_taper.is_some()
                    || face_maker.is_some()
                    || length_along_profile_normal.is_some()
                    || allow_multi_profile_faces.is_some()
                {
                    return Err("extrusion solid state or face-building options are unsupported or unresolved".into());
                }
                let (source, selected) = selected_profiles(ir, profile)?;
                let (sketch_id, plane, sketch, curves) = sketches.get(source.id.as_str()).ok_or("extrusion uses a sketch later in the history")?;
                let mut loops = Vec::new();
                for index in selected {
                    let chain = source.profiles.get(index).ok_or("profile index is out of range")?;
                    let mut names = Vec::new();
                    for usage in chain {
                        names.push(curves.get(usage.entity.as_str()).ok_or("profile uses unknown or point-only geometry")?.clone());
                    }
                    if names.is_empty() {
                        return Err("profile is empty".into());
                    }
                    loops.push(names);
                }
                // A picked outer loop's holes come with it; a hole loop of its own would fill the
                // hole in, so the pick is grouped against the sketch's own profiles.
                let found = solidcraft_sketch::find_profiles_checked(sketch);
                if found.incomplete {
                    return Err("the sketch's profile search was cut short".into());
                }
                let sets: Vec<BTreeSet<String>> = loops.iter().map(|l| l.iter().cloned().collect()).collect();
                let profiles = group_profile_selection(&found.profiles, &sets)?;
                let operation = match op {
                    BooleanOp::NewBody => Operation::NewBody,
                    BooleanOp::Join if bodies <= 1 => Operation::Join,
                    BooleanOp::Cut if bodies == 1 => Operation::Cut,
                    BooleanOp::Intersect if bodies == 1 => Operation::Intersect,
                    _ => return Err("extrusion Boolean targets are unresolved or ambiguous".into()),
                };
                if !suppressed && (*op == BooleanOp::NewBody || bodies == 0) {
                    bodies += 1;
                }
                FeatureKind::Extrude {
                    sketch: SketchId(*sketch_id),
                    profiles,
                    extent: extrusion_extent(plane, direction, start, extent)?,
                    operation,
                    targets: Vec::new(),
                    thin: None,
                }
            }
            _ => return Err(format!("feature {:?} is outside the supported editable history subset", feature.name)),
        };
        let index = doc.add_feature(kind, feature.name.as_deref()).map_err(|e| e.to_string())?;
        let imported = doc.feature_mut(index).ok_or("created feature is missing")?;
        imported.component = component;
        imported.suppressed = suppressed;
    }
    if bodies == 0 {
        return Err("history contains no active solid-producing feature".into());
    }
    Ok(doc)
}

fn parameters(ir: &CadIr, doc: &mut Document) -> ReadResult<()> {
    if ir.model.parameters.len() > solidcraft_doc::MAX_PARAMS {
        return Err("too many Inventor parameters".into());
    }
    let mut pending: BTreeMap<_, _> = ir.model.parameters.iter().map(|p| (p.id.as_str(), p)).collect();
    if pending.len() != ir.model.parameters.len() {
        return Err("duplicate parameter identity".into());
    }
    let mut names = BTreeSet::new();
    let mut loaded = BTreeSet::new();
    while !pending.is_empty() {
        let ready = pending
            .iter()
            .find(|(_, p)| p.dependencies.iter().all(|d| loaded.contains(d.as_str())))
            .map(|(id, _)| *id)
            .ok_or("parameter graph has a cycle or missing dependency")?;
        let p = pending.remove(ready).ok_or("parameter disappeared")?;
        if p.owner.is_some() || !names.insert(p.name.as_str()) {
            return Err("scoped or duplicate parameter name".into());
        }
        // The decoded value is canonical (mm / radians); `eval` in the parameter's kind agrees.
        let (unit, expected) = match &p.value {
            Some(ParameterValue::Length(v)) => ("mm", v.get()),
            Some(ParameterValue::Angle(v)) => ("deg", v.get()),
            Some(ParameterValue::Real(v)) => ("", v.get()),
            _ => return Err(format!("parameter {} has unsupported or unresolved units", p.name)),
        };
        doc.set_param(&p.name, &p.expression, Some(unit), None).map_err(|e| e.to_string())?;
        let value = doc.eval(&p.name, solidcraft_doc::expr::Kind::from_unit(unit)).map_err(|e| e.to_string())?;
        if !value.is_finite() || (value - expected).abs() > 1e-8 * expected.abs().max(1.0) {
            return Err(format!("parameter {} expression does not match its decoded value", p.name));
        }
        loaded.insert(ready);
    }
    Ok(())
}

fn selected_profiles<'a>(ir: &'a CadIr, profile: &ProfileRef) -> ReadResult<(&'a cadmpeg_ir::Sketch, Vec<usize>)> {
    let planar = profile.planar().ok_or("spatial sketch profiles are not supported")?;
    let (id, indices) = match planar {
        // The whole sketch: every closed loop of its profile table.
        PlanarProfileRef::Sketch(id) => (id, None),
        PlanarProfileRef::SketchProfiles { sketch, profiles } => (sketch, Some(profiles.iter().map(|i| *i as usize).collect())),
        PlanarProfileRef::SketchRegions { sketch, regions } => {
            let mut indices = Vec::new();
            for region in regions.iter() {
                let SketchProfileRegion::Loops { loops } = region else {
                    return Err("trimmed atomic profile regions are not supported".into());
                };
                indices.push(loops.outer() as usize);
                indices.extend(loops.holes().iter().map(|i| *i as usize));
            }
            (sketch, Some(indices))
        }
        _ => return Err("profile selection is unresolved or unsupported".into()),
    };
    let sketch = ir.model.sketches.iter().find(|s| &s.id == id).ok_or("selected sketch is missing")?;
    let selected = indices.unwrap_or_else(|| (0..sketch.profiles.len()).collect());
    if selected.is_empty() {
        return Err("selected profile has no closed loops".into());
    }
    let unique: BTreeSet<_> = selected.iter().collect();
    if unique.len() != selected.len() {
        return Err("profile repeats a loop".into());
    }
    if selected.iter().any(|i| *i >= sketch.profiles.len()) {
        return Err("profile index is out of range".into());
    }
    Ok((sketch, selected))
}

/// The loop selection as SolidCraft profile picks: the outermost picked profiles, each carried
/// with the sketch's holes in it. A picked loop that is a hole of another picked profile is
/// absorbed (it is not a second solid); a picked loop that is not a closed profile of the sketch
/// at all, or a picked outer whose profile holes were not all picked, is refused rather than
/// imported wrong.
fn group_profile_selection(found: &[solidcraft_sketch::Profile], selected: &[BTreeSet<String>]) -> ReadResult<ProfileSel> {
    let profiles: Vec<(BTreeSet<String>, Vec<BTreeSet<String>>)> = found
        .iter()
        .map(|p| (p.outer_curves.iter().cloned().collect(), p.hole_curves.iter().map(|h| h.iter().cloned().collect()).collect()))
        .collect();
    let is_outer = |loop_set: &BTreeSet<String>| profiles.iter().any(|(outer, _)| outer == loop_set);
    let is_hole_of = |loop_set: &BTreeSet<String>| profiles.iter().any(|(_, holes)| holes.iter().any(|h| h == loop_set));
    // Absorbed when it is a hole of a profile whose own outer was picked too.
    let absorbed = |loop_set: &BTreeSet<String>| {
        profiles.iter().filter(|(_, holes)| holes.iter().any(|h| h == loop_set)).any(|(outer, _)| selected.iter().any(|s| s == outer))
    };
    let outers: Vec<&BTreeSet<String>> = selected.iter().filter(|l| is_outer(l) && !absorbed(l)).collect();
    if outers.is_empty() {
        return Err("the selected loops contain no outer loop".into());
    }
    for loop_set in selected {
        if is_outer(loop_set) && !absorbed(loop_set) {
            continue;
        }
        if is_hole_of(loop_set) && absorbed(loop_set) {
            continue;
        }
        return Err("a selected loop is not a closed profile of the sketch or a hole in one".into());
    }
    // A picked outer carries the sketch's profile holes: refuse a pick that leaves some out, or
    // the hole would silently fill in.
    for loop_set in &outers {
        let (_, holes) = profiles.iter().find(|(outer, _)| outer == *loop_set).ok_or("picked profile disappeared")?;
        for hole in holes {
            if !selected.iter().any(|s| s == hole) {
                return Err("a selected outer loop's holes were not all selected".into());
            }
        }
    }
    let loops: Vec<Vec<String>> = outers.iter().map(|o| (*o).iter().cloned().collect()).collect();
    Ok(ProfileSel::Curves { loops })
}

fn extrusion_extent(plane: &Plane, direction: &ExtrudeDirection, start: &ExtrudeStart, source: &ExtrudeExtent) -> ReadResult<Extent> {
    let sign = match direction {
        ExtrudeDirection::ProfileNormal {} => 1.0,
        ExtrudeDirection::ReversedProfileNormal {} => -1.0,
        ExtrudeDirection::Explicit { vector, .. } => {
            let v = vector.get();
            let v = Vec3::new(v.x, v.y, v.z).normalized().ok_or("invalid extrusion direction")?;
            let dot = v.dot(plane.normal());
            if (dot.abs() - 1.0).abs() > 1e-8 {
                return Err("oblique extrusion direction is not supported".into());
            }
            dot.signum()
        }
        _ => return Err("extrusion direction is unresolved".into()),
    };
    let (first, second, symmetric) = match source {
        ExtrudeExtent::OneSided { side } => (side, None, false),
        ExtrudeExtent::Symmetric { side } => (side, None, true),
        ExtrudeExtent::TwoSided { first, second } => (first, Some(second), false),
    };
    if first.draft.is_some() || second.is_some_and(|s| s.draft.is_some()) {
        return Err("drafted extrusion history is not supported".into());
    }
    let (distance, through_all) = match first.termination {
        LinearTermination::Blind { length } => (length.get(), false),
        LinearTermination::ThroughAll {} if second.is_none() => (1.0, true),
        _ => return Err("extrusion termination is unresolved or unsupported".into()),
    };
    let distance2 = match second {
        Some(side) => match side.termination {
            LinearTermination::Blind { length } if length.get() > 0.0 => Some(Expr::from(length.get().to_string())),
            _ => return Err("second extrusion termination is unsupported".into()),
        },
        None => None,
    };
    let start_offset = match start {
        ExtrudeStart::ProfilePlane {} => None,
        // The source measures the offset along the travel direction; SolidCraft's start offset
        // runs along the sketch normal whatever the extrusion direction, so it flips when the
        // direction reverses. A symmetric extent has no travel direction: it keeps the offset.
        ExtrudeStart::OffsetProfilePlane { offset } => {
            let factor = if symmetric { 1.0 } else { sign };
            Some(Expr::from((factor * offset.get()).to_string()))
        }
        _ => return Err("extrusion start is unresolved or unsupported".into()),
    };
    if distance <= 0.0 {
        return Err("extrusion travel must be positive".into());
    }
    Ok(Extent {
        distance: Expr::from(distance.to_string()),
        direction: if symmetric {
            Direction::Symmetric
        } else if sign < 0.0 {
            Direction::Negative
        } else {
            Direction::Positive
        },
        convention: solidcraft_doc::ExtrudeConvention::TotalDistance,
        distance2,
        start_offset,
        through_all,
        taper: None,
        to: None,
        to_offset: None,
        from: None,
    })
}

#[derive(Clone, Copy)]
enum Target {
    Point(usize),
    Curve(usize),
}

fn sketch_model(ir: &CadIr, source: &cadmpeg_ir::Sketch) -> ReadResult<(Plane, Sketch, BTreeMap<String, String>)> {
    let (origin, normal, u) = source.placement.resolved().ok_or("sketch plane is unresolved")?;
    let origin = origin.get();
    let normal = normal.get();
    let u = u.get();
    let normal = Vec3::new(normal.x, normal.y, normal.z);
    let u = Vec3::new(u.x, u.y, u.z);
    let plane = Plane::new(Vec3::new(origin.x, origin.y, origin.z), u, normal.cross(u)).ok_or("invalid sketch plane")?;
    let mut sketch = Sketch::new();
    let mut targets = BTreeMap::new();
    let mut curves = BTreeMap::new();
    let entities: Vec<_> = ir.model.sketch_entities.iter().filter(|e| e.sketch == source.id).collect();
    for entity in entities {
        if targets.contains_key(entity.id().as_str()) {
            return Err("duplicate sketch entity".into());
        }
        let target = match entity.geometry.definition().to_raw() {
            Geometry::Point { position } => Target::Point(sketch.add_point(Vec2::new(position.u, position.v), None).map_err(|e| e.to_string())?),
            Geometry::Line { start, end } => {
                Target::Curve(sketch.add_line(Vec2::new(start.u, start.v), Vec2::new(end.u, end.v), None, None, None).map_err(|e| e.to_string())?)
            }
            Geometry::Circle { center, radius } => {
                Target::Curve(sketch.add_circle(Vec2::new(center.u, center.v), radius.get(), None, None).map_err(|e| e.to_string())?)
            }
            _ => return Err("unsupported or unresolved sketch geometry".into()),
        };
        if let Target::Curve(i) = target {
            let curve = sketch.curves.get_mut(i).ok_or("created sketch curve is missing")?;
            curve.construction = entity.construction;
            curves.insert(entity.id().as_str().to_string(), curve.id.clone());
        }
        targets.insert(entity.id().as_str(), target);
    }
    for constraint in ir.model.sketch_constraints.iter().filter(|c| c.sketch == source.id) {
        if constraint.active == Some(false) {
            continue;
        }
        if constraint.active.is_none() {
            return Err("sketch constraint activity is unresolved".into());
        }
        // A reference dimension measures without driving: no stored expression for it.
        let reference = constraint.driving == Some(false);
        let mut driving = None;
        let kind = match constraint.definition.kind() {
            Relation::Disabled {} => continue,
            Relation::Horizontal { entity } => ConstraintKind::Horizontal { l: curve(&targets, entity)? },
            Relation::Vertical { entity } => ConstraintKind::Vertical { l: curve(&targets, entity)? },
            Relation::Parallel { first, second } => ConstraintKind::Parallel { a: curve(&targets, first)?, b: curve(&targets, second)? },
            Relation::Perpendicular { first, second } => ConstraintKind::Perpendicular { a: curve(&targets, first)?, b: curve(&targets, second)? },
            Relation::Equal { first, second } => ConstraintKind::Equal { a: curve(&targets, first)?, b: curve(&targets, second)? },
            Relation::Radius { entity, parameter } | Relation::Diameter { entity, parameter } => {
                let p = ir.model.parameters.iter().find(|p| &p.id == parameter).ok_or("dimension parameter is missing")?;
                let Some(ParameterValue::Length(value)) = &p.value else {
                    return Err("dimension is not a length".into());
                };
                driving = (!reference).then(|| p.name.clone());
                let c = curve(&targets, entity)?;
                if matches!(constraint.definition.kind(), Relation::Radius { .. }) {
                    ConstraintKind::Radius { c, value: value.get() }
                } else {
                    ConstraintKind::Diameter { c, value: value.get() }
                }
            }
            Relation::Distance { entities, parameter } if entities.len() == 1 => {
                let entity = entities.first().ok_or("length dimension has no entity")?;
                let p = ir.model.parameters.iter().find(|p| &p.id == parameter).ok_or("dimension parameter is missing")?;
                let Some(ParameterValue::Length(value)) = &p.value else {
                    return Err("dimension is not a length".into());
                };
                driving = (!reference).then(|| p.name.clone());
                ConstraintKind::Length { l: curve(&targets, entity)?, value: value.get() }
            }
            Relation::CoincidentLoci { loci } => {
                let first = loci.first().ok_or("empty coincidence relation")?;
                let p = locus(&sketch, &targets, first)?;
                for other in loci.iter().skip(1) {
                    let q = locus(&sketch, &targets, other)?;
                    if p != q {
                        sketch.add_constraint(ConstraintKind::Coincident { p, q }, None).map_err(|e| e.to_string())?;
                    }
                }
                continue;
            }
            _ => return Err("unsupported sketch constraint; editable import would lose a relation".into()),
        };
        if driving.is_some() && constraint.driving.is_none() {
            return Err("dimension driving/reference state is unresolved".into());
        }
        let id = sketch.add_constraint(kind, driving).map_err(|e| e.to_string())?;
        if let Some(imported) = sketch.constraints.iter_mut().find(|c| c.id == id) {
            imported.driven = reference;
        }
    }
    Ok((plane, sketch, curves))
}

fn curve(targets: &BTreeMap<&str, Target>, id: &SketchEntityId) -> ReadResult<usize> {
    match targets.get(id.as_str()) {
        Some(Target::Curve(i)) => Ok(*i),
        _ => Err("constraint curve is missing".into()),
    }
}

fn locus(sketch: &Sketch, targets: &BTreeMap<&str, Target>, source: &SketchLocus) -> ReadResult<usize> {
    match source {
        SketchLocus::Entity(id) => match targets.get(id.as_str()) {
            Some(Target::Point(i)) => Ok(*i),
            _ => Err("constraint locus is not a point".into()),
        },
        SketchLocus::Start(id) => match sketch.curves.get(curve(targets, id)?).map(|c| &c.kind) {
            Some(CurveKind::Line { a, .. }) => Ok(*a),
            _ => Err("unsupported start locus".into()),
        },
        SketchLocus::End(id) => match sketch.curves.get(curve(targets, id)?).map(|c| &c.kind) {
            Some(CurveKind::Line { b, .. }) => Ok(*b),
            _ => Err("unsupported end locus".into()),
        },
        SketchLocus::Center(id) => match sketch.curves.get(curve(targets, id)?).map(|c| &c.kind) {
            Some(CurveKind::Circle { c, .. }) => Ok(*c),
            _ => Err("unsupported center locus".into()),
        },
    }
}
