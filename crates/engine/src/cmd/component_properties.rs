//! Physical properties of an assembly placement, with original body material assignments.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcraft_doc::{Document, Mat, PhysicalOverride};
use solidcraft_geom::inertia::{self, Tensor3};
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

/// Principal moments this close (a share of the largest) count as equal: a round body's
/// tessellation tells its two equal moments apart by about 1e-4, which turns their axes at random.
const EQUAL_MOMENTS: f64 = 1e-3;

/// The point `about` asks for the inertia tensor about, if any.
fn about(p: &Value, cmd: &str) -> Result<Option<Vec3>> {
    match p.get("about") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => crate::params::vec3(v).map(Some).ok_or_else(|| bad(cmd, "`about` must be a point [x, y, z] in mm")),
    }
}

/// The 3 × 3 linear part of a placement, row by row: its turn, and any scale or mirror.
fn linear(m: &Mat) -> Tensor3 {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| m[j][i]))
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
    let mut out = properties(s, &doc, &path, descendants(p, cmd)?, about(p, cmd)?, cmd)?;
    out["component"] = json!(id);
    out["name"] = json!(name);
    if let Some(o) = doc.components.iter().find(|c| c.id == id).and_then(|c| c.physical_override) {
        out["physical_override"] = json!(o);
    }
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
    let mut out = properties(s, &doc, &path, descendants(p, cmd)?, about(p, cmd)?, cmd)?;
    out["occurrence"] = json!(id);
    out["name"] = json!(doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.as_str()));
    Ok(out)
}

pub(super) const PHYSICAL_OVERRIDE_CONTRACT: crate::schema::TypedContract<crate::schema::component::PhysicalOverrideParams> =
    crate::schema::TypedContract::new();

/// Set or clear what robot exports use for a component's own bodies.
pub(super) fn physical_override(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.physical_override";
    let id = super::component(s, p, "component", cmd)?;
    if id == 0 {
        return Err(bad(cmd, "the root design is not a component: move its bodies into a component to give them a mass override"));
    }
    let numbers = |key: &str| -> Option<Vec<f64>> { p.get(key)?.as_array()?.iter().map(Value::as_f64).collect() };
    let com = numbers("com").and_then(|v| <[f64; 3]>::try_from(v).ok());
    let inertia = numbers("inertia").and_then(|v| <[f64; 6]>::try_from(v).ok());
    if p.get("com").is_some_and(|v| !v.is_null()) && com.is_none() {
        return Err(bad(cmd, "`com` must be [x, y, z]"));
    }
    if p.get("inertia").is_some_and(|v| !v.is_null()) && inertia.is_none() {
        return Err(bad(cmd, "`inertia` must be [xx, yy, zz, xy, xz, yz]"));
    }
    if p.get("mass").is_some_and(|v| !v.is_null()) && crate::params::num(p, "mass").is_none() {
        return Err(bad(cmd, "`mass` must be a usable number"));
    }
    let saved = match crate::params::num(p, "mass") {
        Some(mass) => {
            let o = PhysicalOverride { mass, com, inertia };
            if !o.is_sane() {
                return Err(bad(
                    cmd,
                    format!(
                        "`mass` must be above 0 and at most {}, and `com` and `inertia` usable numbers, with the inertia's first three above 0",
                        PhysicalOverride::MAX_MASS
                    ),
                ));
            }
            if inertia.is_none() && !solidcraft_io::urdf::has_mass(&s.doc, &s.model.state(), id).map_err(|e| bad(cmd, e.to_string()))? {
                return Err(bad(cmd, "this component has no closed body to work the inertia out from: give `inertia` too"));
            }
            Some(o)
        }
        None if com.is_some() || inertia.is_some() => return Err(bad(cmd, "give `mass` with `com` or `inertia`")),
        None => None,
    };
    let Some(c) = s.doc_mut().components.iter_mut().find(|c| c.id == id) else { return Err(bad(cmd, format!("no component `{id}`"))) };
    c.physical_override = saved;
    Ok(json!({"component": id, "mass": saved.map(|o| o.mass)}))
}

fn properties(s: &Session, doc: &Document, selected: &[u64], descendants: bool, about: Option<Vec3>, cmd: &str) -> Result<Value> {
    let state = s.model.state();
    let mut bodies = Vec::new();
    let (mut volume, mut area, mut mass) = (0.0, 0.0, 0.0);
    let mut moment = Vec3::ZERO;
    let mut bounds = Aabb3::EMPTY;
    let (mut faces, mut edges, mut vertices) = (0, 0, 0);
    let mut placement_count = 0;
    let mut skipped = 0;
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
    // How each closed body's volume spreads about its centre of mass (#8095): one tessellation
    // however often it is placed, which each placement then turns.
    let components: BTreeSet<u64> = selected_placements.iter().map(|(component, _, _)| *component).collect();
    let spread_bodies: Vec<usize> = (0..state.bodies.len())
        .filter(|&i| {
            state.bodies.get(i).is_some_and(|b| components.contains(&doc.model_body_component(b)) && crate::cmd::inspect::encloses_volume(&b.body))
        })
        .collect();
    let spreads = solidcraft_kernel::map_on_threads(&spread_bodies, 16, |&i| {
        state.bodies.get(i).and_then(|b| solidcraft_io::mass_properties(&b.body).ok().flatten()).and_then(|m| m.gyration())
    });
    let spread: BTreeMap<usize, Tensor3> = spread_bodies.into_iter().zip(spreads).filter_map(|(i, g)| Some((i, g.flatten()?))).collect();
    // Closed bodies' (mass, centre of mass, inertia about it), all in world axes.
    let mut parts: Vec<(f64, Vec3, Option<Tensor3>)> = Vec::new();
    for (component, path, transform) in selected_placements {
        placement_count += 1;
        for (index, body) in state.bodies.iter().enumerate().filter(|(_, b)| doc.model_body_component(b) == component) {
            let transform: Mat = doc.model_body_offset(body).map(|offset| solidcraft_doc::mat_mul(&transform, offset)).unwrap_or(transform);
            let placed = solidcraft_kernel::transform_matrix(&body.body, transform)?;
            let measured = solidcraft_kernel::measure(&placed)?;
            let density = doc.density(&body.name);
            let body_mass = measured.volume * density / 1000.0;
            let closed = crate::cmd::inspect::encloses_volume(&placed);
            // The mesh's spread with the exact mass, so the tensor agrees with the mass shown.
            let body_inertia = spread
                .get(&index)
                .filter(|_| closed)
                .map(|g| inertia::from_gyration(&inertia::transformed(g, &linear(&transform)), body_mass))
                .filter(|i| i.iter().flatten().all(|x| x.is_finite()));
            if closed {
                volume += measured.volume;
                mass += body_mass;
                moment += measured.centroid * body_mass;
                parts.push((body_mass, measured.centroid, body_inertia));
            } else {
                skipped += 1;
            }
            area += measured.area;
            bounds = bounds.union(&measured.bbox);
            faces += measured.merged.faces;
            edges += measured.merged.edges;
            vertices += measured.merged.vertices;
            bodies.push(json!({"name": body.name, "component": component, "path": path,
                "material": doc.body_material(&body.name),
                "closed": closed, "density_g_cm3": closed.then_some(density),
                "mass_g": closed.then_some(body_mass), "volume_mm3": closed.then_some(measured.volume),
                "area_mm2": measured.area, "center_of_mass": closed.then_some(measured.centroid),
                "inertia_g_mm2": body_inertia,
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
    // About the centre of mass: each body's own tensor, plus its mass at its distance from that
    // centre (parallel axes). Unknown when any closed body's mesh did not close.
    let at_centre = centroid.and_then(|c| {
        parts.iter().try_fold([[0.0f64; 3]; 3], |sum, (m, at, i)| {
            let i = inertia::shifted(&(*i)?, *m, *at - c);
            Some([0, 1, 2].map(|r| [0, 1, 2].map(|k| sum[r][k] + i[r][k])))
        })
    });
    let at_centre = at_centre.filter(|i| i.iter().flatten().all(|x| x.is_finite()));
    let principal = at_centre.and_then(|i| inertia::principal(&i, EQUAL_MOMENTS));
    let bbox = if bodies.is_empty() { Value::Null } else { json!({"min": bounds.min, "max": bounds.max}) };
    let mut out = json!({"path": selected, "coordinate_frame": "world", "descendants": descendants,
        "placement_count": placement_count, "body_count": bodies.len(), "bodies": bodies,
        "total": {"open_bodies_skipped": skipped, "volume_mm3": volume, "area_mm2": area, "mass_g": mass,
            "center_of_mass": centroid, "first_moment_g_mm": moment, "inertia_g_mm2": at_centre,
            "principal_moments_g_mm2": principal.map(|(moments, _)| moments), "principal_axes": principal.map(|(_, axes)| axes),
            "bbox": bbox, "faces": faces, "edges": edges, "vertices": vertices}});
    if let Some(point) = about {
        let shifted =
            centroid.zip(at_centre).map(|(c, i)| inertia::shifted(&i, mass, point - c)).filter(|i| i.iter().flatten().all(|x| x.is_finite()));
        out["total"]["inertia_about"] = json!({"point": point, "inertia_g_mm2": shifted});
    }
    Ok(out)
}

#[cfg(test)]
#[path = "component_properties_tests.rs"]
mod tests;
