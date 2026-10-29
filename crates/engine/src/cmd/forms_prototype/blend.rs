//! Blender interchange is pure Rust and uses the same transactional Form editor.
use super::*;
use crate::doc::forms::{Definition, Placement};
use crate::geom::subdivision::primitives::Parameters;
use solidcraft_io::blend::{Import, Object};
use std::collections::BTreeSet;
mod surface;

fn error(e: impl ToString) -> EngineError {
    EngineError::Other(e.to_string())
}
fn path(path: &str) -> Result<()> {
    if path.trim().is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(error("choose a bounded Blender file path"));
    }
    Ok(())
}
fn summaries(objects: &[Object]) -> Vec<params::BlendObject> {
    objects
        .iter()
        .map(|o| params::BlendObject { name: o.name.clone(), points: o.cage.points.len(), faces: o.cage.faces.len(), levels: o.levels })
        .collect()
}
fn definition(object: &Object, file: &str) -> Definition {
    let mut cage = object.cage.clone();
    let (min, max) = cage.points.iter().fold(
        (
            crate::geom::Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            crate::geom::Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(a, b), &p| {
            (crate::geom::Vec3::new(a.x.min(p.x), a.y.min(p.y), a.z.min(p.z)), crate::geom::Vec3::new(b.x.max(p.x), b.y.max(p.y), b.z.max(p.z)))
        },
    );
    let center = (min + max) * 0.5;
    for p in &mut cage.points {
        *p -= center;
    }
    let size = max - min;
    Definition {
        imported: Some(Box::new(crate::doc::forms::Imported { file: file.into(), object: object.name.clone() })),
        cage,
        placement: Placement { origin: center, ..Default::default() },
        parameters: Parameters {
            dimensions: [size.x.clamp(0.1, 10000.), size.y.clamp(0.1, 10000.), size.z.clamp(0.1, 10000.)],
            segments: [1; 3],
            ..Default::default()
        },
        symmetry: None,
        hierarchy: None,
        matching: None,
        profile: None,
        pipe: None,
    }
}
pub(super) fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::BlendImport = parse(p)?;
    path(&args.path)?;
    if s.active_sketch.is_some() {
        return Err(error("finish the sketch before importing Forms"));
    }
    if s.forms_prototype
        .as_ref()
        .is_some_and(|f| f.has_tool_preview() || f.editing_feature.is_some() || !f.parts.is_empty() || f.consumed_features().next().is_some())
    {
        return Err(error("finish or cancel the current Form and staged objects before importing"));
    }
    if args.objects.len() > 16 || args.objects.iter().any(|n| n.is_empty() || n.len() > 256) {
        return Err(error("select at most 16 bounded object names"));
    }
    let bytes = solidcraft_io::vfs::read(&args.path, solidcraft_io::blend::MAX_BLEND_BYTES).map_err(error)?;
    let Import { mut objects, rejected } = solidcraft_io::blend::read(&bytes).map_err(error)?;
    if !args.objects.is_empty() {
        let names: BTreeSet<_> = args.objects.iter().collect();
        if names.len() != args.objects.len() || names.iter().any(|n| objects.iter().filter(|o| &o.name == *n).count() != 1) {
            return Err(error("each selected object must identify one supported mesh"));
        }
        objects.retain(|o| names.contains(&o.name));
    }
    if objects.is_empty() {
        return Err(error(format!("no supported quad control cages: {}", rejected.join("; "))));
    }
    let definitions = objects.iter().map(|o| (o.name.clone(), definition(o, &args.path), o.levels)).collect();
    let mut next = crate::forms_prototype::Form::from_import(definitions)?;
    next.authoring_component = s.active_component;
    next.authoring_path = s.active_occurrence_path.clone();
    if let Some(current) = &mut s.forms_prototype {
        current.replace_import(next);
    } else {
        s.forms_prototype = Some(next);
    }
    serde_json::to_value(params::BlendReply { path: args.path, objects: summaries(&objects), rejected, active: true, bytes: None, surface: None })
        .map_err(error)
}
fn object(name: String, definition: &Definition, levels: usize) -> Result<Object> {
    if definition.profile.is_some() {
        return Err(error("Form Surface guide grids are not Catmull-Clark cages; export their CAD shape instead"));
    }
    if definition.matching.is_some() {
        return Err(error("a matched Form needs its constrained surface baked before Blender cage export"));
    }
    if definition.hierarchy.is_some() {
        return Err(error("materialize local refinement before exporting its control cage"));
    }
    let mut cage = definition.cage.clone();
    for point in &mut cage.points {
        *point = definition.placement.point(*point);
    }
    Ok(Object { name, cage, levels })
}
fn cages(s: &Session, args: &params::BlendExport) -> Result<Vec<Object>> {
    let mut objects = Vec::new();
    if args.features.is_empty()
        && let Some(f) = &s.forms_prototype
    {
        if f.has_tool_preview() {
            return Err(error("apply or cancel the Form tool before exporting"));
        }
        let name = args
            .name
            .clone()
            .or_else(|| f.editing_feature.and_then(|id| s.doc.feature(id).map(|v| v.name.clone())))
            .or_else(|| f.imported.as_ref().map(|v| v.object.clone()))
            .unwrap_or_else(|| "Form".into());
        objects.push(object(name, &f.definition(), f.levels)?);
        for part in f.parts.values() {
            objects.push(object(part.name.clone(), &part.definition, f.levels)?);
        }
    } else {
        let ids: BTreeSet<_> = args.features.iter().copied().collect();
        if ids.len() != args.features.len() {
            return Err(error("duplicate Form feature ids"));
        }
        if ids.iter().any(|&id| !s.doc.feature(id).is_some_and(|f| matches!(f.kind, crate::doc::FeatureKind::Form { .. }))) {
            return Err(error("select saved Form features"));
        }
        for feature in &s.doc.features {
            if !ids.is_empty() && !ids.contains(&feature.id) {
                continue;
            }
            if let crate::doc::FeatureKind::Form { definition, .. } = &feature.kind {
                let mut definition = definition.as_ref().clone();
                definition.placement =
                    Placement::from_matrix(&crate::doc::mat_mul(&s.doc.component_transform(feature.component), &definition.placement.matrix()))?;
                objects.push(object(feature.name.clone(), &definition, 2)?);
            }
        }
    }
    Ok(objects)
}

pub(super) fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let args: params::BlendExport = parse(p)?;
    path(&args.path)?;
    if args.features.len() > 1024 || args.name.as_ref().is_some_and(|n| n.is_empty() || n.len() > 256 || n.contains('\0')) {
        return Err(error("export names and feature count exceed their bounds"));
    }
    if args.tolerance.is_some_and(|v| !v.is_finite() || !(0.000001..=1.).contains(&v)) || !args.surface && args.tolerance.is_some() {
        return Err(error("surface fitting tolerance must be 0.000001..1 mm"));
    }
    if !args.surface && (args.operation.is_some() || args.targets.is_some()) {
        return Err(error("CAD operations apply only to surface export"));
    }
    if args.features.iter().copied().collect::<BTreeSet<_>>().len() != args.features.len() {
        return Err(error("duplicate export feature ids"));
    }
    solidcraft_io::vfs::check(&args.path).map_err(error)?;
    let (bytes, objects, rejected) = if args.surface {
        let (objects, warnings) = surface::objects(s, &args)?;
        let summaries = objects
            .iter()
            .map(|o| params::BlendObject { name: o.name.clone(), points: o.mesh.positions.len(), faces: o.mesh.triangles.len(), levels: 0 })
            .collect();
        (solidcraft_io::blend::write_surfaces(&objects).map_err(error)?, summaries, warnings)
    } else {
        let objects = cages(s, &args)?;
        let summaries = summaries(&objects);
        (solidcraft_io::blend::write(&objects).map_err(error)?, summaries, Vec::new())
    };
    #[cfg(not(target_arch = "wasm32"))]
    solidcraft_io::write_atomic(std::path::Path::new(&args.path), &bytes, true).map_err(error)?;
    #[cfg(target_arch = "wasm32")]
    solidcraft_io::vfs::write(&args.path, &bytes).map_err(error)?;
    serde_json::to_value(params::BlendReply {
        path: args.path,
        objects,
        rejected,
        active: s.forms_prototype.is_some(),
        surface: Some(args.surface),
        bytes: Some(bytes.len()),
    })
    .map_err(error)
}
