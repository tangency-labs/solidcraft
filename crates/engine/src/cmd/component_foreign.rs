//! Pasting what was copied in another design (`Session::foreign_copy`). Bodies come as an
//! imported body each, without their history (that stays with the design they were made in), at
//! the same world coordinates as in their design, in one Import base feature in the target
//! component. Components come as a Paste New does: an independent copy of each with what it
//! holds, its history, appearances, joints and motion, where it is in the world.

use super::*;

pub(super) fn paste_from_design(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.paste_from_design";
    let copy = s.foreign_copy.clone().ok_or_else(|| bad(cmd, "nothing was copied in another design"))?;
    if copy.bodies.is_empty() && copy.placed.is_empty() {
        return Err(bad(cmd, "nothing was copied in another design"));
    }
    let to = if p.get("component").is_some() { component(s, p, "component", cmd)? } else { s.active_component };
    let given = match p.get("component_path") {
        None => None,
        Some(v) => Some(
            v.as_array()
                .filter(|a| a.len() <= 64)
                .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
                .ok_or_else(|| bad(cmd, "`component_path` must list 0..64 occurrence ids"))?,
        ),
    };
    let path = match given {
        Some(path) => path,
        None if to == s.active_component => s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(to)),
        None => placing::placement_of(s, &s.placed_document(), to, None, "component_path", cmd)?.0,
    };
    let frame = s.placed_document().path_transform(to, &path).ok_or_else(|| bad(cmd, "that path is not a placement of the target"))?;
    let into_target = solidcraft_doc::mat_inverse(&frame).ok_or_else(|| bad(cmd, "the target's frame is not invertible"))?;
    if copy.bodies.is_empty() {
        return paste_components(s, &copy, to, path, into_target, p);
    }
    // Each body at its world coordinates in its design, in the target's frame, named as in its
    // component (a body seen in another placement without that placement) where that name is
    // free here, else numbered.
    let state = s.model.state();
    let mut names: Vec<String> = Vec::new();
    let mut shapes = Vec::new();
    for (i, seen) in copy.bodies.iter().enumerate() {
        let body = copy.world.body(seen).ok_or_else(|| bad(cmd, format!("no body `{seen}` in {}", copy.design)))?;
        let name = copy.names.get(i).unwrap_or(seen);
        let taken = |n: &str| state.body(n).is_some() || names.iter().any(|x| x == n);
        let new = if taken(name) { solidcraft_doc::copy_name(name, taken) } else { name.clone() };
        shapes.push(solidcraft_kernel::transform_matrix(&body.body, into_target)?);
        names.push(new);
    }
    drop(state);
    let list: Vec<solidcraft_kernel::ExportBody> =
        names.iter().zip(&shapes).map(|(name, body)| solidcraft_kernel::ExportBody { name: name.clone(), body, color: body.color() }).collect();
    let header = solidcraft_kernel::StepHeader { organization: "SolidCraft".into(), ..Default::default() };
    let step = solidcraft_kernel::step_export_bodies(&list, &header)?;
    let mut scratch = s.scratch();
    let kind = FeatureKind::Import { file: format!("{} (pasted)", copy.design), step: step.into(), components: Vec::new(), skip: Default::default() };
    // Named as any import is ("Import1"); the design they came from stays on it as its file.
    let id = scratch.doc_mut().add_feature(kind, None)?;
    let feature = scratch.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    if let Some(f) = scratch.doc_mut().feature_mut(id) {
        f.component = to;
        f.body_names = names.clone();
    }
    scratch.refresh();
    if let Some(error) = scratch.model.result(id).and_then(|r| r.error.as_ref()) {
        return Err(bad(cmd, format!("the pasted bodies didn't import: {error}")));
    }
    scratch.selection = names.iter().map(|name| crate::Sel::Body { name: name.clone() }).collect();
    s.accept_computed(scratch);
    Ok(json!({"bodies": names, "feature": feature, "component": to}))
}

/// Independent copies of the components copied in another design, each with what it holds, in
/// `to` (placed by `path`; `into_target` takes world coordinates into it), where each is in the
/// world; one step to undo. They are selected.
fn paste_components(
    s: &mut Session,
    copy: &crate::ForeignCopy,
    to: u64,
    path: Vec<u64>,
    into_target: solidcraft_doc::Mat,
    p: &Value,
) -> Result<Value> {
    let origin = crate::cmd::component_clone::Source::copied(copy);
    let mut computed: Option<Session> = None;
    let mut made = Vec::new();
    for (component, occurrence, world) in &copy.placed {
        let source_occurrence = copy.doc.occurrences.iter().find(|o| o.id == *occurrence).cloned();
        let transform = solidcraft_doc::mat_mul(&into_target, world);
        if !super::valid_position(&transform) {
            return Err(bad("component.paste_from_design", "copy transform exceeds supported numeric range"));
        }
        let target = computed.as_ref().unwrap_or(s);
        let (next, out) = crate::cmd::component_clone::build(target, &origin, *component, source_occurrence, transform, to, Some(path.clone()), p)?;
        if let Some(id) = out.get("occurrence").and_then(Value::as_u64) {
            made.push(path.iter().copied().chain([id]).collect::<Vec<u64>>());
        }
        computed = Some(next);
    }
    let Some(mut scratch) = computed else { return Err(bad("component.paste_from_design", "the copied components are gone")) };
    scratch.selection = made.iter().map(|path| crate::Sel::Occurrence { path: path.clone() }).collect();
    s.accept_computed(scratch);
    Ok(json!({"paths": made, "component": to}))
}

#[cfg(test)]
#[path = "component_foreign_tests.rs"]
mod tests;
