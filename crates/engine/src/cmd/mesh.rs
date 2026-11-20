//! Imported mesh bodies: parametric reconstruction, and a stack of sections sketched through
//! one.

use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;
use solidcraft_geom::PlaneGeom;

use super::{
    CommandSpec,
    features::{add_feature, check_expr},
    not_in_sketch,
};
use crate::schema::features::FeatureResult;
use crate::schema::mesh::{MeshConvertParams, MeshSectionStackParams, MeshSectionStackResult, SkippedSection};
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    EngineError, Result, Sel, Session,
    params::{bad, bool_, expr, str_},
};

const MESH_CONVERT_CONTRACT: TypedContract<MeshConvertParams> = TypedContract::new();
const SECTION_STACK_CONTRACT: TypedContract<MeshSectionStackParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mesh.convert", "Convert Mesh", crate::checked!("mesh.convert", convert))
        .icon("import")
        .contract(&MESH_CONVERT_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm): rebuild a closed mesh as an editable solid — a constant-section part as planar and cylindrical faces, any other closed mesh as the faceted solid its triangles describe — with a parametric timeline feature in the mesh body's owning component (the active component must be that component or an ancestor; otherwise activate the named component first); the active component is unchanged; name?"),
    CommandSpec::new("mesh.section_stack", "Section Stack", crate::checked!("mesh.section_stack", section_stack))
        .icon("fit_section")
        .enabled(not_in_sketch)
        .contract(&SECTION_STACK_CONTRACT)
        .output(output_schema_for::<MeshSectionStackResult>)
        .params("body?: mesh body name (default: the selected mesh body); plane: as sketch.create's plane; spacing: length expression (> 0); from?, to?: length expressions, distances along the plane's normal (default: the body's reach along it); max_sections?: 1…1000 (default 100); extrude?: bool (default false: extrude each section's material to the next plane and join the slabs into one body); name? (default \"Section\"): a sketch of fitted lines, arcs and circles on each plane that meets the mesh, in one timeline folder, as one undo step → sketches, planes, empty, slabs, bodies, skipped?, group"),
];

/// How far inside the body's reach along the normal the first and last planes stay: a plane
/// exactly on a flat end face cuts every triangle of that face, which has no section.
const STACK_EDGE: f64 = 1e-3;
/// A last plane this close to the end plane moves onto it, so the end isn't cut twice a hair
/// apart (also at most a tenth of the spacing).
const STACK_HAIR: f64 = 0.01;
/// A slab thinner than this at the end of the stack is not made.
const THINNEST_SLAB: f64 = 0.01;
/// Below this, two planes are the same plane.
const SAME: f64 = 1e-9;
/// Each section walks every triangle of the mesh (about 0.12 s per section for 100,000 triangles),
/// so the default keeps a stack on a large scan to seconds; up to [`MOST_SECTIONS`] on request.
const DEFAULT_MAX_SECTIONS: u32 = 100;
const MOST_SECTIONS: u32 = 1000;
const MAX_NAME: usize = 100;
/// How Fit Curves to Mesh Section refuses a plane its body doesn't cross
/// (`solidcraft_doc::project`'s message for a link that resolves to nothing).
const MISSES_THE_MESH: &str = "projects to nothing on this sketch plane";

/// A length in mm for a name or a message: at most three decimals, without trailing zeros.
fn mm(v: f64) -> String {
    let t = format!("{v:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" { "0".to_string() } else { t.to_string() }
}

/// The planes' distances along the normal: `from` and `to` (either way round), each kept inside
/// `[reach[0] + STACK_EDGE, reach[1] - STACK_EDGE]`, then every `step` from the lower one, the
/// upper one always included. Each distance is `lo + k * step`, never a running sum. A last
/// multiple closer to the end than [`STACK_HAIR`] (or a tenth of the step) moves onto the end
/// instead of adding a plane a hair from it, and ends that close are one plane. More than `most`
/// planes is refused before any is made, naming the spacing that fits.
pub(crate) fn stack_offsets(from: f64, to: f64, step: f64, reach: [f64; 2], most: usize) -> std::result::Result<Vec<f64>, String> {
    let [low, high] = reach;
    if ![from, to, step, low, high].iter().all(|v| v.is_finite()) {
        return Err("the stack's numbers must be finite".into());
    }
    if step <= 0.0 {
        return Err("`spacing` must be more than 0".into());
    }
    let (inner_low, inner_high) = (low + STACK_EDGE, high - STACK_EDGE);
    if inner_high < inner_low {
        return Err(format!("the body is only {} mm thick along the plane's normal: there is nothing to cut", mm(high - low)));
    }
    let keep = |v: f64| v.max(inner_low).min(inner_high);
    let (lo, hi) = if from <= to { (keep(from), keep(to)) } else { (keep(to), keep(from)) };
    // Ends closer than the hair are one plane: two sections a hair apart only draw the same
    // sketch twice.
    if hi - lo <= SAME || hi - lo < STACK_HAIR.min(step / 10.0) {
        return Ok(vec![lo]);
    }
    let multiples = ((hi - lo) / step + SAME).floor();
    // The planes the multiples make, and the end plane after them (counted in floats first, so a
    // tiny spacing is refused before anything is allocated).
    let count = multiples + 2.0;
    let too_many = |n: f64| -> String {
        let fits = if most > 1 { ((hi - lo) / (most as f64 - 1.0) * 1000.0).ceil() / 1000.0 } else { f64::INFINITY };
        let advice = if fits.is_finite() {
            format!("a spacing of at least {} mm fits", mm(fits))
        } else {
            "give `from` and `to` the same value for one section".to_string()
        };
        format!(
            "{} sections from {} mm to {} mm every {} mm are more than `max_sections` ({most}): {advice}",
            if n.is_finite() && n < 1e15 { format!("{n}") } else { "too many".to_string() },
            mm(lo),
            mm(hi),
            mm(step)
        )
    };
    if !count.is_finite() || count - 1.0 > most as f64 {
        return Err(too_many(count - 1.0));
    }
    // At most `most + 1` here, so the cast is exact.
    let n = multiples as usize;
    let mut out: Vec<f64> = (0..=n).map(|k| lo + k as f64 * step).collect();
    let last = out.last().copied().unwrap_or(lo);
    let gap = hi - last;
    if gap > SAME {
        if out.len() > 1 && gap < STACK_HAIR.min(step / 10.0) {
            if let Some(l) = out.last_mut() {
                *l = hi;
            }
        } else {
            out.push(hi);
        }
    }
    if out.len() > most {
        return Err(too_many(out.len() as f64));
    }
    Ok(out)
}

/// How far each section is extruded to make the slabs, for planes at `ats` (ascending) every
/// `step`, the body ending at `far`: each slab runs to the next plane, the last one a full `step`
/// but never past `far`. When the next-to-last slab is under half a step (the end plane came a
/// hair after a multiple), it runs on through the last one's height and the end section is only
/// drawn (`None`); so is an end slab under [`THINNEST_SLAB`]. One plane makes no slab.
pub(crate) fn stack_extrudes(ats: &[f64], step: f64, far: f64) -> Vec<Option<f64>> {
    let n = ats.len();
    let mut out: Vec<Option<f64>> = vec![None; n];
    if n < 2 {
        return out;
    }
    for (slab, pair) in out.iter_mut().zip(ats.windows(2)) {
        if let [a, b] = pair {
            *slab = Some(b - a);
        }
    }
    let last_at = ats.last().copied().unwrap_or(far);
    let mut last = Some((far - last_at).min(step).max(0.0));
    if n > 2
        && let (Some(Some(before)), Some(end)) = (out.get(n - 2).copied(), last)
        && before < 0.5 * step
    {
        if let Some(slot) = out.get_mut(n - 2) {
            *slot = Some(before + end);
        }
        last = None;
    }
    if last.is_some_and(|d| d < THINNEST_SLAB) {
        last = None;
    }
    if let Some(slot) = out.last_mut() {
        *slot = last;
    }
    out
}

/// The outer curve ids of a sketch's material profiles: those at an even depth, where a profile
/// whose outer loop is another profile's hole is one level deeper than that profile (a disc
/// filling a hole is depth 1; an island inside the hole, depth 2). Ids only: which loop lies in
/// which is the profile finder's.
fn material_outlines(profiles: &[solidcraft_sketch::Profile]) -> Vec<Vec<String>> {
    let sorted = |ids: &[String]| {
        let mut v = ids.to_vec();
        v.sort();
        v
    };
    let outers: Vec<Vec<String>> = profiles.iter().map(|p| sorted(&p.outer_curves)).collect();
    // The profile whose hole each profile's outer loop is, if any.
    let parent: Vec<Option<usize>> = outers
        .iter()
        .enumerate()
        .map(|(i, outer)| profiles.iter().enumerate().position(|(j, q)| j != i && q.hole_curves.iter().any(|h| sorted(h) == *outer)))
        .collect();
    profiles
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            let mut depth = 0usize;
            let mut at = *i;
            // At most one step per profile: a cycle (which the finder never makes) ends the walk.
            for _ in 0..profiles.len() {
                match parent.get(at).copied().flatten() {
                    Some(up) => {
                        depth += 1;
                        at = up;
                    }
                    None => break,
                }
            }
            depth.is_multiple_of(2)
        })
        .map(|(_, p)| p.outer_curves.clone())
        .collect()
}

/// A length key: `None` when absent, else its value in mm, refused when it doesn't evaluate as a
/// finite length.
fn length(s: &Session, p: &Value, k: &str, cmd: &str) -> Result<Option<f64>> {
    let Some(e) = expr(p, k) else {
        return match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => Err(bad(cmd, format!("`{k}` must be a length: a number or an expression such as \"2 mm\""))),
        };
    };
    let v = s.doc.eval(&e, Kind::Length).map_err(|err| bad(cmd, format!("{k}: {err}")))?;
    if v.is_finite() { Ok(Some(v)) } else { Err(bad(cmd, format!("`{k}` must be a finite length"))) }
}

fn section_stack(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.section_stack";
    let state = s.model.state();
    let given = target(s, p, cmd)?;
    let body = s.own_body_name(&given).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    if !source.body.is_mesh() {
        return Err(bad(cmd, "select a mesh body"));
    }
    // The sketches go into the active component, and its frame is the one the body's box and the
    // plane are read in.
    let owner = s.doc.model_body_component(source);
    if owner != s.active_component {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first: the sections are sketched in the mesh body's own component")));
    }
    let plane_value = match p.get("plane") {
        None | Some(Value::Null) => return Err(bad(cmd, "`plane` is required: the first plane of the stack, as sketch.create takes it")),
        Some(v) => v.clone(),
    };
    let spacing = length(s, p, "spacing", cmd)?.ok_or_else(|| bad(cmd, "`spacing` is required: the distance between sections"))?;
    if spacing <= 0.0 {
        return Err(bad(cmd, "`spacing` must be more than 0"));
    }
    let most = match p.get("max_sections") {
        None | Some(Value::Null) => DEFAULT_MAX_SECTIONS,
        Some(v) => match v.as_u64() {
            Some(n) if (1..=u64::from(MOST_SECTIONS)).contains(&n) => n as u32,
            _ => return Err(bad(cmd, format!("`max_sections` must be a whole number from 1 to {MOST_SECTIONS}"))),
        },
    };
    let extrude = match p.get("extrude") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "extrude").ok_or_else(|| bad(cmd, "`extrude` must be true or false"))?,
    };
    let name = match p.get("name") {
        None | Some(Value::Null) => "Section".to_string(),
        Some(Value::String(n)) if !n.trim().is_empty() && n.trim().chars().count() <= MAX_NAME => n.trim().to_string(),
        Some(_) => return Err(bad(cmd, format!("`name` must be 1…{MAX_NAME} characters"))),
    };
    // The first plane, as the sketches will have it, and the body's reach along its normal.
    let base = super::sketch::plane_ref(s, &json!({ "plane": plane_value }), cmd)?;
    let (vals, _) = s.doc.param_values();
    let plane = s.doc.resolve_plane_in(&vals, &base, 0, s.active_component, Some(state.as_ref())).map_err(|e| bad(cmd, e.to_string()))?;
    let bounds = source.body.exact_bounds().map_err(|e| bad(cmd, format!("the mesh body's size: {e}")))?;
    let v3 = |v: solidcore_record::Vec3| [v.x, v.y, v.z];
    let reach = solidcore_ops::affine::span_along(v3(bounds.min), v3(bounds.max), v3(plane.origin), v3(plane.normal()))
        .map_err(|_| bad(cmd, "the mesh body's reach along the plane's normal is not a finite length"))?;
    let from = length(s, p, "from", cmd)?.unwrap_or(reach[0]);
    let to = length(s, p, "to", cmd)?.unwrap_or(reach[1]);
    let offsets = stack_offsets(from, to, spacing, reach, most as usize).map_err(|why| bad(cmd, why))?;
    let n = offsets.len();

    let mut made: Vec<(u64, f64)> = Vec::new();
    let mut empty = 0usize;
    for (i, at) in offsets.iter().enumerate() {
        let sketch_name = format!("{name} {} of {n} ({} mm)", i + 1, mm(*at));
        let created =
            run(s, "sketch.create", &json!({"plane": plane_value, "offset": format!("{at} mm"), "project_edges": false, "name": sketch_name}))?;
        let sketch = created.get("sketch").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("a section's sketch was not made".into()))?;
        let fitted = match run(s, "sketch.fit_curves_to_section", &json!({ "body": body })) {
            Ok(r) => r.get("curves").and_then(Value::as_array).is_some_and(|c| !c.is_empty()),
            // Fit Curves refuses a plane the mesh doesn't cross: here that plane is only empty.
            Err(e) if e.to_string().contains(MISSES_THE_MESH) => false,
            Err(e) => return Err(e),
        };
        run(s, "sketch.finish", &json!({}))?;
        if fitted {
            made.push((sketch, *at));
        } else {
            // The plane missed the mesh: no sketch for it.
            run(s, "timeline.delete", &json!({ "feature": sketch }))?;
            empty += 1;
        }
    }
    if made.is_empty() {
        return Err(bad(cmd, format!("none of the {n} planes meets the mesh body `{body}`: check `plane`, `from` and `to`")));
    }

    let mut slabs = Vec::new();
    let mut bodies: Vec<String> = Vec::new();
    let mut skipped = Vec::new();
    if extrude {
        let ats: Vec<f64> = made.iter().map(|(_, at)| *at).collect();
        let heights = stack_extrudes(&ats, spacing, reach[1] - STACK_EDGE);
        for ((sketch, _), height) in made.iter().zip(heights) {
            let Some(height) = height else {
                skipped.push(SkippedSection { sketch: *sketch, reason: "the stack's end: drawn, not extruded".into() });
                continue;
            };
            // Read from the sketch itself: the model is evaluated once the whole stack is made.
            let outlines =
                super::sketch_store::read(&s.doc, *sketch).map(|sk| material_outlines(&solidcraft_sketch::find_profiles(sk))).unwrap_or_default();
            if outlines.is_empty() {
                skipped.push(SkippedSection { sketch: *sketch, reason: "no closed outline to extrude".into() });
                continue;
            }
            let profiles: Vec<Value> = outlines.iter().map(|ids| json!({ "curves": ids })).collect();
            let mut call = json!({"sketch": sketch, "profiles": profiles, "distance": format!("{height} mm")});
            if bodies.is_empty() {
                call["operation"] = json!("new");
                call["body_name"] = json!(format!("{body} slabs"));
            } else {
                call["operation"] = json!("join");
                call["targets"] = json!(bodies);
            }
            let r = run(s, "solid.extrude", &call)?;
            let names = |k: &str| -> Vec<String> {
                r.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
            };
            let removed = names("removed_bodies");
            bodies.retain(|b| !removed.contains(b));
            for b in names("created_bodies") {
                if !bodies.contains(&b) {
                    bodies.push(b);
                }
            }
            if let Some(f) = r.get("feature").and_then(Value::as_u64) {
                slabs.push(f);
            }
        }
    }

    let mut features: Vec<u64> = made.iter().map(|(id, _)| *id).collect();
    features.extend(slabs.iter().copied());
    features.sort_unstable();
    let folder_name: String = format!("{name} stack ({body})").chars().take(128).collect();
    let grouped = run(s, "timeline.group", &json!({"name": folder_name, "features": features}))?;
    let reply = MeshSectionStackResult {
        sketches: made.iter().map(|(id, _)| *id).collect(),
        planes: n,
        empty,
        slabs,
        bodies,
        skipped,
        group: grouped.get("group").and_then(Value::as_u64),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// Another command, run inside this one (one undo step for the whole stack).
fn run(s: &mut Session, id: &str, p: &Value) -> Result<Value> {
    let spec = super::find_command(id).ok_or_else(|| EngineError::Other(format!("`{id}` is missing")))?;
    (spec.run)(s, p)
}

/// The mesh body to convert: `body`, else the one selected body.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<String> {
    if let Some(name) = str_(p, "body").filter(|b| !b.trim().is_empty()) {
        return Ok(name.to_owned());
    }
    let selected: Vec<&str> = s.selection.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.as_str()) } else { None }).collect();
    match selected.as_slice() {
        [name] => Ok((*name).to_owned()),
        [] => Err(bad(cmd, "select a mesh body")),
        _ => Err(bad(cmd, "select one mesh body")),
    }
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.convert";
    let state = s.model.state();
    let given = target(s, p, cmd)?;
    // The selection and the Browser name a body of a further placement as the world shows it
    // (`Mesh (Scan:2)`): that name means the body it shows, which the conversion edits in its
    // component, as `appearance.assign` reads it (#9740).
    let body = s.own_body_name(&given).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    if !source.body.is_mesh() {
        return Err(bad(cmd, "select a mesh body"));
    }
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0.05 mm".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    // Conversion edits the component definition, independent of its occurrence placement.
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::MeshConvert { body: body.into(), tolerance });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    result
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
