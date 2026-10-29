//! Evaluate into a scratch document: exporting never finishes or changes the
//! user's live Form, history, source references, or staged companion objects.
use super::*;
use solidcraft_io::blend::SurfaceObject;

fn bodies(s: &Session, state: &crate::doc::ModelState, name: Option<&str>) -> Vec<SurfaceObject> {
    let world = crate::doc::world_state(&s.doc, state);
    world
        .bodies
        .iter()
        .enumerate()
        .map(|(i, b)| SurfaceObject {
            name: if let Some(name) = name { if i == 0 { name.into() } else { format!("{name} ({})", i + 1) } } else { b.name.clone() },
            mesh: b.mesh(),
        })
        .collect()
}

pub(super) fn objects(s: &Session, args: &params::BlendExport) -> Result<(Vec<SurfaceObject>, Vec<String>)> {
    let mut scratch = s.scratch();
    let mut ids = args.features.clone();
    let active = ids.is_empty() && s.forms_prototype.is_some();
    let mut primary_name = None;
    if active {
        let form = s.forms_prototype.as_ref().ok_or_else(|| error("no Form is being edited"))?;
        if form.has_tool_preview() {
            return Err(error("apply or cancel the Form tool before exporting"));
        }
        primary_name = Some(
            args.name
                .clone()
                .or_else(|| form.editing_feature.and_then(|id| s.doc.feature(id).map(|f| f.name.clone())))
                .or_else(|| form.imported.as_ref().map(|source| source.object.clone()))
                .unwrap_or_else(|| "Form".into()),
        );
        let mut params = json!({});
        if let Some(tolerance) = args.tolerance {
            params["tolerance"] = json!(tolerance);
        }
        if let Some(operation) = &args.operation {
            params["operation"] = json!(operation);
        }
        if let Some(targets) = &args.targets {
            params["targets"] = json!(targets);
        }
        let reply = scratch.execute("form.prototype.commit", &params)?;
        ids.push(reply["feature"].as_u64().ok_or_else(|| error("the evaluated Form has no feature identity"))?);
        if let Some(parts) = reply["parts"].as_array() {
            for part in parts {
                ids.push(part.as_u64().ok_or_else(|| error("invalid staged Form identity"))?);
            }
        }
    } else if args.tolerance.is_some() || args.operation.is_some() || args.targets.is_some() {
        return Err(error("fitting options apply only to an active Form"));
    }
    if ids.is_empty() {
        return Ok((bodies(&scratch, &scratch.model.state(), None), Vec::new()));
    }
    let mut objects = Vec::new();
    let mut warnings = Vec::new();
    for (index, id) in ids.into_iter().enumerate() {
        let feature = scratch.doc.feature(id).ok_or_else(|| error("select existing export features"))?;
        let result = scratch.model.result(id).ok_or_else(|| error("the selected feature has not been evaluated"))?;
        if let Some(message) = &result.error {
            return Err(error(format!("{}: {message}", feature.name)));
        }
        if result.skipped || result.stale {
            return Err(error(format!("{} is not currently evaluated", feature.name)));
        }
        if let Some(message) = &result.warning {
            warnings.push(format!("{}: {message}", feature.name));
        }
        let before = scratch.model.state_before(id);
        let mut changed = result.state.as_ref().clone();
        changed
            .bodies
            .retain(|body| body.feature == id || before.bodies.iter().find(|old| old.name == body.name).is_none_or(|old| !old.body.same(&body.body)));
        if changed.bodies.is_empty() {
            return Err(error(format!("{} produces no exportable surface", feature.name)));
        }
        let name = if active && index == 0 { primary_name.as_deref() } else { Some(feature.name.as_str()) };
        objects.extend(bodies(&scratch, &changed, name));
    }
    Ok((objects, warnings))
}
