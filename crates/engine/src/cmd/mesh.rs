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
use crate::schema::mesh::{MeshConvertParams, MeshReduceParams, MeshReduceResult, MeshSectionStackParams, MeshSectionStackResult, SkippedSection};
use crate::schema::{TypedContract, output_schema_for};
use crate::{
    EngineError, Result, Sel, Session,
    params::{bad, bool_, expr, str_},
};

const MESH_CONVERT_CONTRACT: TypedContract<MeshConvertParams> = TypedContract::new();
const MESH_REDUCE_CONTRACT: TypedContract<MeshReduceParams> = TypedContract::new();
const SECTION_STACK_CONTRACT: TypedContract<MeshSectionStackParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("mesh.convert", "Convert Mesh", crate::checked!("mesh.convert", convert)).begins_edit()
        .icon("import")
        .contract(&MESH_CONVERT_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body?: mesh body name (default: the selected mesh body); tolerance?: length expression (default 0.05 mm): rebuild a closed mesh as an editable solid — a constant-section part as planar and cylindrical faces, any other closed mesh as the faceted solid its triangles describe — with a parametric timeline feature in the mesh body's owning component (the active component must be that component or an ancestor; otherwise activate the named component first); the active component is unchanged; name?"),
    CommandSpec::new("mesh.reduce", "Reduce Mesh", crate::checked!("mesh.reduce", reduce))
        .icon("mesh_reduce")
        .contract(&MESH_REDUCE_CONTRACT)
        .output(output_schema_for::<MeshReduceResult>)
        .params("body?: mesh body name (default: selected body); exactly one of triangle_count: positive integer or percentage: retained percent strictly between 0 and 100; name?: feature name. Creates a separate mesh in the source component, preserving the original. Reports actual triangle count and warns when the target cannot be reached."),
    CommandSpec::new("mesh.section_stack", "Section Stack", crate::checked!("mesh.section_stack", section_stack))
        .icon("fit_section")
        .enabled(not_in_sketch)
        .contract(&SECTION_STACK_CONTRACT)
        .output(output_schema_for::<MeshSectionStackResult>)
        .params("body?: mesh body name (default: the selected mesh body); plane: as sketch.create's plane; spacing: length expression (> 0); from?, to?: length expressions, distances along the plane's normal (default: the body's reach along it); max_sections?: 1…1000 (default 100); extrude?: bool (default false: extrude each section's material to the next plane, each slab a body of its own); name? (default \"Section\"): a sketch of fitted lines, arcs and circles on each plane that meets the mesh, in one timeline folder, as one undo step → sketches, planes, empty, slabs, bodies, skipped?, group"),
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

use super::board_import::mm;

/// The planes' distances along the normal: `from` and `to` (either way round), each kept inside
/// `[reach[0] + STACK_EDGE, reach[1] - STACK_EDGE]`, then every `step` from the lower one, the
/// upper one always included. Each distance is `lo + k * step`, never a running sum. A last
/// multiple closer to the end than [`STACK_HAIR`] (or a tenth of the step) moves onto the end
/// instead of adding a plane a hair from it, and ends that close are one plane. More than `most`
/// planes is refused before any is made, naming the spacing that fits; so are ends wholly
/// outside the reach, which would cut none of the body.
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
    if from.max(to) < low || from.min(to) > high {
        return Err(format!(
            "`from` and `to` ({} mm to {} mm) lie outside the mesh body, which reaches from {} mm to {} mm along the plane's normal",
            mm(from.min(to)),
            mm(from.max(to)),
            mm(low),
            mm(high)
        ));
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

/// The outer curve ids of a sketch's material profiles, by the rule an extrude takes when it is
/// given none ([`super::features::material_profiles`]). Read from the sketch, not the model: the
/// model is evaluated once the whole stack is made. A section's fitted outlines never cross, so it
/// has no enclosed gaps.
fn material_outlines(profiles: &[solidcraft_sketch::Profile]) -> Vec<Vec<String>> {
    super::features::material_profiles(profiles, &[]).into_iter().map(|p| p.outer_curves.clone()).collect()
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
    // The sketches go into the active component. A body of that component is measured where it
    // is defined; with the design's top level active, a body of a component inside it (where
    // Insert Mesh puts a scan) is measured where it is placed, the frame the top level's sketches
    // are in, as Fit Curves reads it.
    let owner = s.doc.model_body_component(source);
    let at_top = s.active_component == 0 && s.active_occurrence_path.is_none();
    let measured: solidcraft_kernel::Body = if owner == s.active_component {
        source.body.clone()
    } else if at_top && s.doc.component_within(owner, s.active_component) {
        let world = s.world_state()?;
        let placed: Vec<&solidcraft_doc::ModelBody> =
            world.bodies.iter().filter(|b| s.own_body_name(&b.name).as_deref() == Some(body.as_str())).collect();
        match placed.as_slice() {
            [one] => one.body.clone(),
            _ => {
                let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or("its component");
                return Err(bad(cmd, format!("the mesh body is placed {} times: activate component '{name}' to cut it there", placed.len())));
            }
        }
    } else {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(
            cmd,
            format!("activate component '{name}' (or the design's top level) first: the sections are sketched in the active component"),
        ));
    };
    let bounds = measured.exact_bounds().map_err(|e| bad(cmd, format!("the mesh body's size: {e}")))?;
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
    let v3 = |v: solidcore_record::Vec3| [v.x, v.y, v.z];
    let reach = solidcraft_kernel::services::affine::span_along(v3(bounds.min), v3(bounds.max), v3(plane.origin), v3(plane.normal()))
        .map_err(|_| bad(cmd, "the mesh body's reach along the plane's normal is not a finite length"))?;
    let from = length(s, p, "from", cmd)?.unwrap_or(reach[0]);
    let to = length(s, p, "to", cmd)?.unwrap_or(reach[1]);
    let offsets = stack_offsets(from, to, spacing, reach, most as usize).map_err(|why| bad(cmd, why))?;
    let n = offsets.len();
    // Each triangle's span along the normal. A plane crosses the mesh where a triangle has
    // corners both sides of it, and meets it where triangles touch it from both sides (a plane
    // on a ring of vertices, or on a shoulder's face). The body's box reaches further than the
    // mesh along a slanted normal, and a mesh of separate pieces has gaps between them.
    let normal = plane.normal();
    let (positions, triangles) = measured.triangle_mesh().ok_or_else(|| bad(cmd, "select a mesh body"))?;
    let height = |k: u32| {
        positions.get(k as usize).map(|q| (q.x - plane.origin.x) * normal.x + (q.y - plane.origin.y) * normal.y + (q.z - plane.origin.z) * normal.z)
    };
    let spans: Vec<(f64, f64)> = triangles
        .iter()
        .filter_map(|t| {
            let (a, b, c) = (height(t[0])?, height(t[1])?, height(t[2])?);
            Some((a.min(b).min(c), a.max(b).max(c)))
        })
        .collect();
    let hair = (reach[1] - reach[0]).abs().max(1.0) * SAME;
    let crosses = |at: f64| spans.iter().any(|(lo, hi)| *lo < at - hair && *hi > at + hair);
    let meets = |at: f64| {
        spans.iter().any(|(lo, hi)| *lo <= at + hair && *hi > at + hair) && spans.iter().any(|(lo, hi)| *hi >= at - hair && *lo < at - hair)
    };
    // The sketches' names, the first of `name`, `name 2`, `name 3`… none of whose names is taken
    // (a second stack of the same planes).
    let label = |prefix: &str, i: usize, at: f64| format!("{prefix} {} of {n} ({} mm)", i + 1, mm(at));
    let taken = |prefix: &str| offsets.iter().enumerate().any(|(i, at)| s.doc.features.iter().any(|f| f.name == label(prefix, i, *at)));
    let prefix = std::iter::once(name.clone())
        .chain((2..=MOST_SECTIONS as usize).map(|k| format!("{name} {k}")))
        .find(|p| !taken(p))
        .ok_or_else(|| bad(cmd, format!("every name from `{name}` to `{name} {MOST_SECTIONS}` is taken: give the stack another `name`")))?;

    // The sketches made, with the index of their plane.
    let mut made: Vec<(u64, usize)> = Vec::new();
    let mut empty = 0usize;
    for (i, at) in offsets.iter().enumerate() {
        if !meets(*at) {
            empty += 1;
            continue;
        }
        let sketch_name = label(&prefix, i, *at);
        let created =
            run(s, "sketch.create", &json!({"plane": plane_value, "offset": format!("{at} mm"), "project_edges": false, "name": sketch_name}))?;
        let sketch = created.get("sketch").and_then(Value::as_u64).ok_or_else(|| EngineError::Other("a section's sketch was not made".into()))?;
        let fitted = match run(s, "sketch.fit_curves_to_section", &json!({ "body": body })) {
            Ok(r) => r.get("curves").and_then(Value::as_array).is_some_and(|c| !c.is_empty()),
            // No triangle crosses this plane: it only touches the mesh where two pieces meet,
            // which may have no section.
            Err(_) if !crosses(*at) => false,
            Err(e) => return Err(e),
        };
        run(s, "sketch.finish", &json!({}))?;
        if fitted {
            made.push((sketch, i));
        } else {
            // The plane only touched the mesh: no sketch for it.
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
        // Every plane's slab, empty planes included: a section's slab runs to the next plane,
        // whether or not that plane met the mesh, never on through the gap after it.
        let heights = stack_extrudes(&offsets, spacing, reach[1] - STACK_EDGE);
        // A plane that missed the mesh ends the slab below it, even where a thin last slab
        // would otherwise run on through it.
        let met: std::collections::BTreeSet<usize> = made.iter().map(|(_, i)| *i).collect();
        // Slab bodies are named `<body> slab N`, N the first number no body has yet (a second
        // stack on the same mesh carries on from the first).
        let mut taken: std::collections::BTreeSet<String> = state.bodies.iter().map(|b| b.name.clone()).collect();
        let mut number = 0usize;
        for (sketch, i) in &made {
            let Some(mut height) = heights.get(*i).copied().flatten() else {
                skipped.push(SkippedSection { sketch: *sketch, reason: "the stack's end: drawn, not extruded".into() });
                continue;
            };
            if let (Some(at), Some(next)) = (offsets.get(*i), offsets.get(i + 1))
                && !met.contains(&(i + 1))
            {
                height = height.min(next - at);
            }
            // Read from the sketch itself: the model is evaluated once the whole stack is made.
            let outlines =
                super::sketch_store::read(&s.doc, *sketch).map(|sk| material_outlines(&solidcraft_sketch::find_profiles(sk))).unwrap_or_default();
            if outlines.is_empty() {
                skipped.push(SkippedSection { sketch: *sketch, reason: "no closed outline to extrude".into() });
                continue;
            }
            let profiles: Vec<Value> = outlines.iter().map(|ids| json!({ "curves": ids })).collect();
            let mut call = json!({"sketch": sketch, "profiles": profiles, "distance": format!("{height} mm")});
            // Each slab is a body of its own. Neighbouring slabs meet on faces whose outlines,
            // fitted section by section, differ by a hair: joining them asks the boolean to
            // merge nearly coincident faces, which fails on ordinary parts.
            call["operation"] = json!("new");
            let slab_name = loop {
                number += 1;
                let name = format!("{body} slab {number}");
                if !taken.contains(&name) {
                    break name;
                }
            };
            taken.insert(slab_name.clone());
            call["body_name"] = json!(slab_name);
            // A slab that cannot be built (a scan's outline crossing itself) is left out, with
            // why: the other sections and slabs stand.
            let (doc, model) = (s.doc.clone(), s.model.clone());
            let r = match run(s, "solid.extrude", &call) {
                Ok(r) => r,
                Err(e) => {
                    (s.doc, s.model) = (doc, model);
                    skipped.push(SkippedSection { sketch: *sketch, reason: format!("its slab could not be built: {e}") });
                    continue;
                }
            };
            for b in r.get("created_bodies").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                bodies.push(b.to_string());
            }
            if let Some(f) = r.get("feature").and_then(Value::as_u64) {
                slabs.push(f);
            }
        }
    }

    let mut features: Vec<u64> = made.iter().map(|(id, _)| *id).collect();
    features.extend(slabs.iter().copied());
    features.sort_unstable();
    let folder_name: String = format!("{prefix} stack ({body})").chars().take(128).collect();
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
    super::find_command(id).ok_or_else(|| EngineError::Other(format!("`{id}` is missing")))?.run_nested(s, p)
}

/// The mesh body to convert: `body`, else the one selected body.
fn target(s: &Session, p: &Value, cmd: &str) -> Result<String> {
    if let Some(name) = str_(p, "body").filter(|b| !b.trim().is_empty()) {
        return Ok(name.to_owned());
    }
    let selected: Vec<String> = s.selected_items()?.into_iter().filter_map(|x| if let Sel::Body { name } = x { Some(name) } else { None }).collect();
    match selected.as_slice() {
        [name] => Ok(name.clone()),
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

fn reduce(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "mesh.reduce";
    let given = target(s, p, cmd)?;
    let body = s.own_body_name(&given).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let state = s.model.state();
    let source = state.body(&body).ok_or_else(|| bad(cmd, format!("no body `{given}`")))?;
    let original = source.body.triangle_mesh().ok_or_else(|| bad(cmd, "select a mesh body"))?.1.len();
    let owner = s.doc.model_body_component(source);
    if !s.doc.component_within(owner, s.active_component) {
        let name = s.doc.components.iter().find(|c| c.id == owner).map(|c| c.name.as_str()).unwrap_or(s.doc.name.as_str());
        return Err(bad(cmd, format!("activate component '{name}' first")));
    }
    let count_value = p.get("triangle_count").filter(|v| !v.is_null());
    let percent_value = p.get("percentage").filter(|v| !v.is_null());
    let (triangle_count, percentage, target) = match (count_value, percent_value) {
        (Some(value), None) => {
            let count = value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| bad(cmd, "triangle_count must be a positive integer"))?;
            (Some(count), None, count)
        }
        (None, Some(value)) => {
            let percent = value
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.0 && *n < 100.0)
                .ok_or_else(|| bad(cmd, "percentage must be finite and strictly between 0 and 100"))?;
            (None, Some(percent), ((original as f64 * percent / 100.0).floor() as usize).max(1))
        }
        _ => return Err(bad(cmd, "specify exactly one of triangle_count or percentage")),
    };
    if target >= original {
        return Err(bad(cmd, format!("target {target} must be less than the original {original} triangles")));
    }
    let active_component = std::mem::replace(&mut s.active_component, owner);
    let active_occurrence_path = s.active_occurrence_path.take();
    let result = add_feature(s, p, FeatureKind::MeshReduce { body: body.into(), triangle_count, percentage });
    s.active_component = active_component;
    s.active_occurrence_path = active_occurrence_path;
    let mut result = result?;
    let name = result["created_bodies"]
        .as_array()
        .and_then(|names| names.first())
        .and_then(Value::as_str)
        .ok_or_else(|| bad(cmd, "reduction did not create a body"))?;
    let achieved = s
        .model
        .state()
        .body(name)
        .and_then(|b| b.body.triangle_mesh())
        .map(|(_, t)| t.len())
        .ok_or_else(|| bad(cmd, "reduction did not create a mesh"))?;
    result["original_triangles"] = original.into();
    result["target_triangles"] = target.into();
    result["achieved_triangles"] = achieved.into();
    Ok(result)
}

#[cfg(test)]
#[path = "mesh_tests.rs"]
mod tests;
