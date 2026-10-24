//! Physical properties of an assembly placement, with original body material assignments.
use serde_json::{Value, json};
use solidcraft_doc::{Document, Mat};
use solidcraft_geom::{Aabb3, Vec3};

use crate::params::bad;
use crate::{Result, Session};

fn descendants(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("descendants") {
        None => Ok(true),
        Some(Value::Bool(v)) => Ok(*v),
        Some(_) => Err(bad(cmd, "`descendants` must be a boolean")),
    }
}

fn placed_doc(s: &Session) -> Document {
    let mut doc = (*s.doc).clone();
    for occurrence in &mut doc.occurrences {
        if let Some(transform) = s.pending_moves.get(&occurrence.id) {
            occurrence.transform = *transform;
        }
    }
    doc
}

/// A property total must cover the entire assembly. The display traversal intentionally
/// truncates hostile designs, so use an explicit error instead of accepting its prefix.
pub(in crate::cmd) fn placements(doc: &Document, cmd: &str) -> Result<Vec<(u64, Vec<u64>, Mat)>> {
    let mut out = vec![(0, Vec::new(), solidcraft_doc::IDENTITY)];
    let mut stack = vec![(0, Vec::<u64>::new(), solidcraft_doc::IDENTITY)];
    let mut children: std::collections::BTreeMap<u64, Vec<_>> = std::collections::BTreeMap::new();
    for occurrence in &doc.occurrences {
        children.entry(occurrence.parent).or_default().push(occurrence);
    }
    while let Some((component, path, transform)) = stack.pop() {
        for occurrence in children.get(&component).into_iter().flatten() {
            if path.len() >= 64 || out.len() >= 100_000 {
                return Err(bad(cmd, "assembly exceeds physical-properties traversal limits (64 levels or 100000 placements)"));
            }
            let mut child_path = path.clone();
            child_path.push(occurrence.id);
            let world = solidcraft_doc::mat_mul(&transform, &occurrence.transform);
            out.push((occurrence.component, child_path.clone(), world));
            stack.push((occurrence.component, child_path, world));
        }
    }
    Ok(out)
}

pub(super) fn component(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.properties";
    let id = if p.get("component").is_some() { super::component(s, p, "component", cmd)? } else { s.active_component };
    let doc = placed_doc(s);
    let path = doc.first_path(id);
    let name = if id == 0 {
        doc.name.clone()
    } else {
        doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).ok_or_else(|| bad(cmd, "no active component"))?
    };
    let mut out = properties(s, &doc, &path, descendants(p, cmd)?, cmd)?;
    out["component"] = json!(id);
    out["name"] = json!(name);
    Ok(out)
}

pub(super) fn occurrence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.properties";
    let id = super::occurrence(s, p, cmd)?;
    let doc = placed_doc(s);
    let path = if p.get("path").is_some() {
        super::occurrence_path(s, p, id, cmd)?
    } else {
        let paths: Vec<Vec<u64>> =
            placements(&doc, cmd)?.into_iter().filter(|(_, path, _)| path.last() == Some(&id)).map(|(_, path, _)| path).collect();
        match paths.as_slice() {
            [path] => path.clone(),
            [] => return Err(bad(cmd, "occurrence has no placement")),
            _ => return Err(bad(cmd, "occurrence has multiple placements; supply its `path`")),
        }
    };
    let mut out = properties(s, &doc, &path, descendants(p, cmd)?, cmd)?;
    out["occurrence"] = json!(id);
    out["name"] = json!(doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.as_str()));
    Ok(out)
}

fn properties(s: &Session, doc: &Document, selected: &[u64], descendants: bool, cmd: &str) -> Result<Value> {
    let state = s.model.state();
    let mut bodies = Vec::new();
    let (mut volume, mut area, mut mass) = (0.0, 0.0, 0.0);
    let mut moment = Vec3::ZERO;
    let mut bounds = Aabb3::EMPTY;
    let (mut faces, mut edges, mut vertices) = (0, 0, 0);
    let mut placement_count = 0;
    let selected_placements = if selected.is_empty() {
        if descendants { placements(doc, cmd)? } else { vec![(0, Vec::new(), solidcraft_doc::IDENTITY)] }
    } else {
        let placement = s.occurrence_placement(selected).map_err(|e| bad(cmd, e.to_string()))?;
        let mut out = vec![(placement.component, selected.to_vec(), placement.world)];
        let mut stack = if descendants { out.clone() } else { Vec::new() };
        while let Some((component, path, transform)) = stack.pop() {
            for occurrence in doc.occurrences.iter().filter(|o| o.parent == component) {
                if path.len() >= 64 || out.len() >= 100_000 {
                    return Err(bad(cmd, "assembly exceeds physical-properties traversal limits (64 levels or 100000 placements)"));
                }
                let mut child = path.clone();
                child.push(occurrence.id);
                let world = solidcraft_doc::mat_mul(&transform, &occurrence.transform);
                out.push((occurrence.component, child.clone(), world));
                stack.push((occurrence.component, child, world));
            }
        }
        out
    };
    for (component, path, transform) in selected_placements {
        placement_count += 1;
        for body in state.bodies.iter().filter(|b| doc.model_body_component(b) == component) {
            let transform: Mat = doc.model_body_offset(body).map(|offset| solidcraft_doc::mat_mul(&transform, offset)).unwrap_or(transform);
            let placed = solidcraft_kernel::transform_matrix(&body.body, transform)?;
            let measured = solidcraft_kernel::measure(&placed)?;
            let density = doc.density(&body.name);
            let body_mass = measured.volume * density / 1000.0;
            volume += measured.volume;
            area += measured.area;
            mass += body_mass;
            moment += measured.centroid * body_mass;
            bounds = bounds.union(&measured.bbox);
            faces += measured.merged.faces;
            edges += measured.merged.edges;
            vertices += measured.merged.vertices;
            bodies.push(json!({"name": body.name, "component": component, "path": path,
                "material": doc.body_material(&body.name),
                "density_g_cm3": density, "mass_g": body_mass, "volume_mm3": measured.volume,
                "area_mm2": measured.area, "center_of_mass": measured.centroid,
                "bbox": {"min": measured.bbox.min, "max": measured.bbox.max}}));
        }
    }
    if !(volume.is_finite() && area.is_finite() && mass.is_finite() && moment.is_finite()) {
        return Err(bad(cmd, "physical properties exceed supported numeric range"));
    }
    let centroid = if mass > 0.0 { Some(moment / mass) } else { None };
    if centroid.is_some_and(|point| !point.is_finite()) {
        return Err(bad(cmd, "center of mass exceeds supported numeric range"));
    }
    let bbox = if bodies.is_empty() { Value::Null } else { json!({"min": bounds.min, "max": bounds.max}) };
    Ok(json!({"path": selected, "coordinate_frame": "world", "descendants": descendants,
        "placement_count": placement_count, "body_count": bodies.len(), "bodies": bodies,
        "total": {"volume_mm3": volume, "area_mm2": area, "mass_g": mass,
            "center_of_mass": centroid, "bbox": bbox, "faces": faces, "edges": edges, "vertices": vertices}}))
}

#[cfg(test)]
#[path = "component_properties_tests.rs"]
mod tests;
