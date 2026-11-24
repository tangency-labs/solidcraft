//! A design's history as a command script (#886): `timeline.export_script` writes the steps
//! that rebuild the design (`solidcraft-cli eval` / `run` replay them), and `timeline.add`
//! appends one recorded feature, as the design file holds it, so every feature kind replays
//! exactly: same ids, names, parameter names and stored edge and face names. Trimming the
//! script after a step gives the design as it was at that step.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use solidcraft_doc::config::Column;
use solidcraft_doc::{Document, Feature, FeatureKind, Param, expr};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::timeline_clipboard::{AddParams, AddResult, ExportScriptParams, ExportScriptResult, Script, ScriptAbout, ScriptStep};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const ADD_CONTRACT: TypedContract<AddParams> = TypedContract::new();
const EXPORT_SCRIPT_CONTRACT: TypedContract<ExportScriptParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("timeline.add", "Add Recorded Feature", crate::checked!("timeline.add", add))
        .icon("new")
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<AddResult>)
        .params(
        "feature: a feature as the design file and timeline.inspect hold it ({type, name?, id?, …}; the name is kept even when another \
         feature has it, the id is kept and refused when another feature has it, so later steps can refer to them); params?: [{name, expression, unit?, comment?}] (the model parameters its sketch \
         dimensions use); the feature goes at the marker → feature, name, error, warning",
    ),
    CommandSpec::new("timeline.export_script", "Export Command Script", crate::checked!("timeline.export_script", export_script))
        .icon("export")
        .noundo()
        .doc_only()
        .contract(&EXPORT_SCRIPT_CONTRACT)
        .output(output_schema_for::<ExportScriptResult>)
        .params(
        "path?: write the script to this file → script: {commands: [...]} that rebuilds the design (solidcraft-cli eval/run), \
         steps, path (as given), absolute_path (with path: the file written as an absolute path, left out when it can't be worked out), bytes (with path: the size of the file written), omitted: what the script leaves out (appearances, folders, views, configurations a replay cannot rebuild, print-face assignments); components and assemblies \
         are not supported yet",
    ),
];

/// Most model parameters one `timeline.add` step may bring (a sketch's dimensions).
const MAX_STEP_PARAMS: usize = solidcraft_sketch::MAX_CONSTRAINTS;

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.add";
    let raw = p
        .get("feature")
        .and_then(Value::as_object)
        .ok_or_else(|| bad(cmd, "`feature` must be a feature object, as a design file or timeline.inspect has it"))?;
    let mut raw = raw.clone();
    if let Some(Value::Object(def)) = raw.remove("definition") {
        for (k, v) in def {
            raw.entry(k).or_insert(v);
        }
    }
    for meta in ["omitted", "inputs", "error", "warning", "boolean_diagnostics", "rolled_back", "type_name"] {
        raw.remove(meta);
    }
    if let Some(kind_str) = raw.get("type").and_then(Value::as_str)
        && let Some(canonical) = FeatureKind::type_name_to_kind_key(kind_str)
    {
        raw.insert("type".into(), json!(canonical));
    }
    resolve_legacy_names(&mut raw, &s.doc);
    let feature = recorded_feature(cmd, &raw)?;
    let params = step_params(cmd, p.get("params"))?;
    let id = s.doc_mut().insert_feature(feature, params)?;
    s.refresh();
    let name = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    let r = s.model.result(id);
    let reply = AddResult {
        feature: id,
        name,
        error: r.and_then(|r| r.error.clone()),
        warning: r.and_then(|r| r.warning.clone()),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// timeline.add keeps accepting legacy name-keyed pattern and mirror seeds and construction
/// plane names (#1134): a script recorded before these references were keyed by id resolves
/// them against the design as it stands, exactly the way opening a saved design migrates them
/// (`format::pending::feature_id_refs`) — a name that finds no feature keeps id 0, which
/// dangles at evaluation with the same warning or error it always gave.
fn resolve_legacy_names(raw: &mut Map<String, Value>, doc: &Document) {
    let kind = raw.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    // A pattern's or mirror's seeds: each name becomes the feature's id.
    if matches!(kind.as_str(), "pattern" | "mirror")
        && let Some(Value::Array(seeds)) = raw.get_mut("features")
    {
        for seed in seeds.iter_mut() {
            if let Value::String(name) = seed {
                let name = name.clone();
                *seed = json!(doc.seed_feature(&name).map(|f| f.id).unwrap_or(0));
            }
        }
    }
    // Construction plane references, everywhere but a revolve's axis: an
    // `AxisRef::Construction` has the same shape but names a construction axis, which is
    // still keyed by name.
    let skip_axis = matches!(kind.as_str(), "revolve" | "surface_revolve");
    for (key, v) in raw.iter_mut() {
        if !(skip_axis && key == "axis") {
            resolve_plane_refs(v, doc);
        }
    }
}

/// Rewrite every `{"type": "construction", "name": …}` plane reference in `v` to carry the
/// feature's id, resolved by `Document::find_feature`; nothing is id 0.
fn resolve_plane_refs(v: &mut Value, doc: &Document) {
    match v {
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("construction") {
                if let Some(name) = o.get("name").and_then(Value::as_str) {
                    let name = name.to_string();
                    let id = doc.find_feature(&name).map(|f| f.id).unwrap_or(0);
                    o.remove("name");
                    o.insert("feature".into(), json!(id));
                }
                return;
            }
            for (_, x) in o.iter_mut() {
                resolve_plane_refs(x, doc);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| resolve_plane_refs(x, doc)),
        _ => {}
    }
}

/// The feature, through the checks a design file gets when it opens (numbers, sketch sizes, 3D
/// curves, face attachments…), so a script step can hold nothing a design could not.
fn recorded_feature(cmd: &str, raw: &Map<String, Value>) -> Result<Feature> {
    let mut raw = raw.clone();
    // A hand-written step may leave out the id and the name: they are then given new ones.
    raw.entry("id").or_insert(json!(0));
    raw.entry("name").or_insert(json!(""));
    // Opening fills in missing input names with the next free `dN` of the one-feature design
    // below; the real design names them instead (`Document::insert_feature`).
    let inputs = match raw.get("param_names") {
        None => Vec::new(),
        Some(Value::Array(a)) if a.len() <= 1000 => {
            a.iter().map(|n| n.as_str().map(str::to_string)).collect::<Option<Vec<_>>>().ok_or_else(|| bad(cmd, "`param_names` must be names"))?
        }
        Some(_) => return Err(bad(cmd, "`param_names` must be a list of names")),
    };
    let design = json!({
        "format": solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT),
        "name": "",
        "params": [],
        "features": [Value::Object(raw)],
    });
    let mut d = Document::from_json(&design.to_string()).map_err(|e| bad(cmd, format!("invalid feature: {e}")))?;
    let mut f = d.features.pop().ok_or_else(|| bad(cmd, "invalid feature"))?;
    f.param_names = inputs;
    Ok(f)
}

/// The model parameters of a `timeline.add` step. The reader the command's contract uses too.
pub(crate) fn step_params(cmd: &str, v: Option<&Value>) -> Result<Vec<Param>> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let list = v.as_array().filter(|a| a.len() <= MAX_STEP_PARAMS).ok_or_else(|| bad(cmd, "`params` must be a list of parameters"))?;
    let text = |p: &Value, k: &str| match p.get(k) {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) if k == "expression" => n.as_f64().filter(|x| x.is_finite()).map(|x| x.to_string()),
        Some(_) => None,
    };
    let mut out = Vec::with_capacity(list.len());
    for p in list {
        let name = str_(p, "name").ok_or_else(|| bad(cmd, "each parameter needs a `name`"))?;
        let expr =
            text(p, "expression").filter(|e| !e.trim().is_empty()).ok_or_else(|| bad(cmd, format!("parameter `{name}` needs an `expression`")))?;
        let unit = text(p, "unit").ok_or_else(|| bad(cmd, format!("parameter `{name}`: `unit` must be text")))?;
        let comment = text(p, "comment").ok_or_else(|| bad(cmd, format!("parameter `{name}`: `comment` must be text")))?;
        out.push(Param { name: name.trim().to_string(), expr: expr.trim().to_string(), unit: unit.trim().to_string(), comment, model: true });
    }
    Ok(out)
}

fn export_script(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.export_script";
    let state = s.model.state();
    let (commands, omitted) = script_steps(&s.doc, &|body| state.body(body).is_some(), s.path.as_deref())?;
    let steps = commands.len();
    let script = Script {
        about: ScriptAbout {
            design: s.doc.name.clone(),
            format: solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT),
            replay: "solidcraft-cli eval script.json (timeline.add steps need a build that reads this design format)".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        commands,
    };
    let path = str_(p, "path");
    let mut bytes = None;
    if let Some(path) = path {
        let text = serde_json::to_string_pretty(&script).map_err(|e| EngineError::Other(e.to_string()))?;
        solidcraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        bytes = Some(text.len());
    } else if p.get("path").is_some_and(|v| !v.is_null()) {
        return Err(bad(cmd, "`path` must be a file path"));
    }
    // `path` echoes the call, as it did before #18838 (#19248). `absolute_path` names the file in full:
    // a relative path resolves against this process's folder, which an agent talking to the
    // app over MCP can't see. It is left out where it can't be worked out: with no such folder
    // (browser storage), or when the folder's name isn't valid UTF-8. On Linux and macOS a ".."
    // stays in it: taking it out by the text alone names another file when the folder before it
    // is a link. Windows removes it, and resolves the path the same way when opening it.
    let absolute_path = path.and_then(|p| std::path::absolute(p).ok()).and_then(|a| a.to_str().map(str::to_string));
    let path = path.map(str::to_string);
    serde_json::to_value(ExportScriptResult { omitted, path, absolute_path, bytes, script, steps, param_warnings: Vec::new() })
        .map_err(|e| bad(cmd, e.to_string()))
}

fn step(command: &str, params: Value) -> Value {
    serde_json::to_value(ScriptStep { command: command.to_string(), params }).unwrap_or_default()
}

fn not_supported(what: &str) -> EngineError {
    EngineError::Other(format!("not supported yet: a command script of a design with {what} (#886)"))
}

/// The steps that rebuild `doc` on an empty design, and what they leave out. `has_body` says
/// whether the design has a body of that name now, for the columns of the configuration table,
/// and `own` is the design's own file, for the paths of linked designs.
pub(crate) fn script_steps(doc: &Document, has_body: &dyn Fn(&str) -> bool, own: Option<&str>) -> Result<(Vec<Value>, Vec<String>)> {
    if doc.components.iter().any(|c| c.parent != 0) {
        return Err(not_supported("nested components"));
    }
    if !doc.body_components.is_empty()
        || !doc.body_offsets.is_empty()
        || doc.features.iter().any(|f| matches!(f.kind, FeatureKind::BodyTransfer { .. }))
    {
        return Err(not_supported("bodies moved between components"));
    }
    let mut omitted = Vec::new();
    for (empty, what) in [
        (doc.appearances.is_empty(), "appearances (colours of bodies and faces)"),
        (doc.canvases.is_empty(), "canvases (reference images)"),
        (doc.point_clouds.is_empty(), "point clouds"),
        (doc.print_faces.is_empty(), "print-face assignments (body.print_face)"),
        (doc.named_views.is_empty(), "named views"),
        (doc.browser_groups.is_empty() && doc.browser_order.is_empty(), "Browser folders and order"),
        (doc.timeline_groups.is_empty(), "timeline folders"),
        (doc.visibility.is_empty(), "hidden items"),
    ] {
        if !empty {
            omitted.push(what.to_string());
        }
    }

    let mut out = Vec::new();
    if doc.units != "mm" {
        out.push(step("document.units", json!({"units": doc.units})));
    }
    // Design properties, in one step. Keys `document.metadata.set` would refuse (a well-known
    // key holding something other than text, a key another tool wrote outside the rules) are
    // listed as left out instead of failing the replay.
    let mut properties = solidcraft_doc::Metadata::default();
    let mut odd = Vec::new();
    for (key, value) in &doc.metadata.0 {
        if properties.set(solidcraft_doc::metadata::Owner::Design, key, value.clone()).is_err() {
            odd.push(key.as_str());
        }
    }
    if !properties.is_empty() {
        out.push(step("document.metadata.set", json!({"values": properties})));
    }
    if !odd.is_empty() {
        omitted.push(format!("design properties document.metadata.set can't set ({})", odd.join(", ")));
    }
    for rule in &doc.sheet.rules {
        let mut p = serde_json::to_value(rule).map_err(|e| EngineError::Other(e.to_string()))?;
        if doc.sheet.active.as_deref() == Some(rule.name.as_str())
            && let Some(o) = p.as_object_mut()
        {
            o.insert("active".into(), json!(true));
        }
        out.push(step("sheet.manage_rules", p));
    }
    if let Some(active) = doc.sheet.active.as_deref().filter(|a| !doc.sheet.rules.iter().any(|r| r.name == *a)) {
        out.push(step("sheet.manage_rules", json!({"name": active, "active": true})));
    }
    for rule in &doc.plastic.rules {
        out.push(step("plastic.manage_rules", serde_json::to_value(rule).map_err(|e| EngineError::Other(e.to_string()))?));
    }
    if let Some(active) = &doc.plastic.active {
        out.push(step("plastic.manage_rules", json!({"name": active, "active": true})));
    }
    for m in &doc.user_materials {
        let mut p = json!({"name": m.name, "density_g_cm3": m.density});
        if let (Some(look), Some(o)) = (&m.appearance, p.as_object_mut()) {
            // A colour of its own carries the material's name; otherwise it is a library look.
            if look.name == m.name {
                o.insert("color".into(), json!(look.color));
            } else {
                o.insert("appearance".into(), json!(look.name));
            }
            o.insert("opacity".into(), json!(look.opacity));
        }
        out.push(step("material.create", p));
    }

    // ---- assembly: components, occurrences and features, in id order ----
    // One id counter numbers features, components, occurrences and joints, so
    // replaying every allocation-bearing step at its original id position
    // reproduces the design's ids exactly: features keep their own ids (and
    // the persistent names that embed them) and their component references
    // stay valid. Features whose timeline order differs from their id order
    // break the interleaving and are refused explicitly; joint ids travel
    // last and are referenced by name, so their position is free.
    // Each sketch brings the model parameters of its dimensions.
    let models: BTreeMap<&str, &Param> = doc.params.iter().filter(|p| p.model).map(|p| (p.name.as_str(), p)).collect();
    let mut owned: BTreeSet<&str> = BTreeSet::new();
    let mut brings: BTreeMap<u64, Vec<&Param>> = BTreeMap::new();
    for f in &doc.features {
        if let FeatureKind::Sketch { sketch, .. } = &f.kind {
            for name in sketch.dimension_params().into_iter().map(String::as_str) {
                if let Some(p) = models.get(name)
                    && owned.insert(name)
                {
                    brings.entry(f.id).or_default().push(p);
                }
            }
        }
    }
    if let Some(p) = models.keys().find(|n| !owned.contains(*n)) {
        return Err(not_supported(&format!("model parameter `{p}` outside a sketch dimension")));
    }

    // Standard parts rebuild themselves from their library entry at the
    // part's step: their generated features are skipped below and come back
    // with parts.insert, and their seat joints replay through the joint pass
    // like every other joint.
    let mut skipped: BTreeSet<u64> = BTreeSet::new();
    let mut part_built: BTreeMap<u64, usize> = BTreeMap::new();
    for (comp, part) in &doc.parts {
        let fresh = rebuilt_part(doc, *comp, part, &brings)?;
        for f in &fresh {
            skipped.insert(f.id);
        }
        part_built.insert(*comp, fresh.len());
    }

    // A linked design rebuilds its Import feature from the file at its step.
    for (comp, link) in &doc.links {
        let name = doc.components.iter().find(|c| c.id == *comp).map(|c| c.name.clone()).unwrap_or_default();
        match doc.features.iter().find(|f| f.id == link.feature) {
            Some(f) if f.component == *comp && matches!(f.kind, FeatureKind::Import { .. }) && !f.suppressed => {
                skipped.insert(link.feature);
            }
            Some(f) if f.suppressed => {
                return Err(not_supported(&format!("a linked design whose import feature is suppressed (`{name}`)")));
            }
            _ => {
                return Err(not_supported(&format!("a linked design whose import feature is missing or changed (`{name}`)")));
            }
        }
    }

    enum Event<'e> {
        Create(&'e solidcraft_doc::Component),
        Copy(&'e solidcraft_doc::Occurrence),
        Feature(&'e Feature),
    }
    let mut events: Vec<(u64, Event<'_>)> = Vec::new();
    // A stream that interleaves features with component, occurrence and
    // constructor steps is walked in id order, so a timeline reordered
    // relative to creation order breaks the interleaving and is refused. A
    // features-only stream keeps the timeline's own order.
    let interleaved = !doc.components.is_empty() || !doc.occurrences.is_empty();
    let mut last_feature_id = 0u64;
    for f in &doc.features {
        if interleaved && f.id < last_feature_id {
            return Err(not_supported("a timeline reordered relative to creation order"));
        }
        last_feature_id = f.id;
        if !skipped.contains(&f.id) {
            events.push((f.id, Event::Feature(f)));
        }
    }
    for c in &doc.components {
        events.push((c.id, Event::Create(c)));
    }
    for o in &doc.occurrences {
        if !doc.components.iter().any(|c| c.id == o.component && c.id.saturating_add(1) == o.id) {
            events.push((o.id, Event::Copy(o)));
        }
    }
    if interleaved {
        events.sort_by_key(|(id, _)| *id);
    }
    let mut sim_occ: Vec<(u64, String)> = Vec::new();

    // User parameters go in as soon as everything they refer to exists, so each step sees the
    // values the design has (parameters are global; the order only has to let each one evaluate).
    let mut known: BTreeSet<String> = BTreeSet::new();
    let mut pending: Vec<&Param> = doc.params.iter().filter(|p| !p.model).collect();
    let flush = |pending: &mut Vec<&Param>, known: &mut BTreeSet<String>, out: &mut Vec<Value>| {
        while let Some(i) = pending.iter().position(|p| expr::references(&p.expr).iter().all(|r| known.contains(r))) {
            let p = pending.remove(i);
            let mut params = json!({"name": p.name, "expression": p.expr, "unit": p.unit});
            if let Some(o) = params.as_object_mut() {
                if !p.comment.is_empty() {
                    o.insert("comment".into(), json!(p.comment));
                }
                if doc.favorites.contains(&p.name) {
                    o.insert("favorite".into(), json!(true));
                }
            }
            out.push(step("parameters.add", params));
            known.insert(p.name.clone());
        }
    };
    let mut sim_next = 1u64;
    // The ids a replay gives the components and occurrences (deleted steps
    // leave gaps its allocator does not cross), and the identification colour
    // number its component creations hand out.
    let mut comp_map: BTreeMap<u64, u64> = BTreeMap::new();
    let mut occ_map: BTreeMap<u64, u64> = BTreeMap::new();
    let mut color_index = 0u64;
    for (_id, event) in &events {
        match event {
            Event::Feature(f) => {
                sim_next = sim_next.max(f.id.saturating_add(1));
                flush(&mut pending, &mut known, &mut out);
                // The feature keeps its id; its references follow the items to
                // the ids the replay gives them.
                let mut feature = serde_json::to_value(f).map_err(|e| EngineError::Other(e.to_string()))?;
                remap_replay_refs(&mut feature, &comp_map, &occ_map)?;
                let mut params = json!({"feature": feature});
                if let (Some(list), Some(o)) = (brings.get(&f.id), params.as_object_mut()) {
                    let list: Vec<Value> = list
                        .iter()
                        .map(|p| {
                            let mut v = json!({"name": p.name, "expression": p.expr, "unit": p.unit});
                            if let (false, Some(o)) = (p.comment.is_empty(), v.as_object_mut()) {
                                o.insert("comment".into(), json!(p.comment));
                            }
                            v
                        })
                        .collect();
                    o.insert("params".into(), Value::Array(list));
                    known.extend(brings.get(&f.id).into_iter().flatten().map(|p| p.name.clone()));
                }
                known.extend(f.param_names.iter().filter(|n| !n.is_empty()).cloned());
                out.push(step("timeline.add", params));
            }
            Event::Create(c) => {
                let name = replay_occurrence_name(doc, &sim_occ, c.id);
                sim_occ.push((c.id, name.clone()));
                // The first placement comes with the component, under the name
                // the naming rule gives it: a design that names it otherwise
                // (or has none) cannot come back the same.
                let first = doc.occurrences.iter().find(|o| o.id == c.id.saturating_add(1));
                match first {
                    Some(o) if o.component == c.id && o.name == name => {}
                    Some(o) if o.name != name => {
                        return Err(not_supported(&format!(
                            "an occurrence the replay would name differently (`{}` would become `{}`)",
                            o.name, name
                        )));
                    }
                    _ => return Err(not_supported(&format!("a component whose first placement is missing or another's (`{}`)", c.name))),
                }
                let predicted_color = solidcraft_doc::component_color::automatic_color(color_index);
                color_index = color_index.saturating_add(1);
                if doc.component_identification_color(c.id) != predicted_color {
                    omitted.push(format!("identification colour of `{}`", c.name));
                }
                if let Some(part) = doc.parts.get(&c.id) {
                    // A part rebuilds its features at the ids its own steps
                    // allocate, so the counter must sit at the component's id:
                    // steps that left gaps before the part would renumber what
                    // it rebuilds, and a hand-made numbering cannot be asked
                    // for. The builder's features are skipped above.
                    let built = part_built.get(&c.id).copied().unwrap_or(0);
                    if sim_next != c.id {
                        return Err(not_supported(&format!("a standard part whose rebuilt features would not keep their ids (`{}`)", c.name)));
                    }
                    let mut o = Map::new();
                    o.insert("family".into(), json!(part.family));
                    o.insert("size".into(), json!(part.size));
                    if let Some(length) = part.length {
                        o.insert("length".into(), json!(length));
                    }
                    o.insert("name".into(), json!(c.name));
                    out.push(step("parts.insert", Value::Object(o)));
                    comp_map.insert(c.id, c.id);
                    occ_map.insert(c.id.saturating_add(1), c.id.saturating_add(1));
                    sim_next = c.id.saturating_add(2 + built as u64);
                    // The builder names its inputs again at replay: a user
                    // parameter that refers to one flushes after the part.
                    for f in doc.features.iter().filter(|f| f.id > c.id.saturating_add(1) && f.id <= c.id.saturating_add(1 + built as u64)) {
                        known.extend(f.param_names.iter().filter(|n| !n.is_empty()).cloned());
                    }
                } else if let Some(link) = doc.links.get(&c.id) {
                    // A linked design comes in through its own constructor,
                    // which reads the file again and rebuilds the Import
                    // feature at the id its steps allocate. The file is named
                    // as the replay resolves it: against its own folder, not
                    // wherever the replay happens to run.
                    if sim_next != c.id || link.feature != c.id.saturating_add(2) {
                        return Err(not_supported(&format!("a linked design whose rebuilt feature would not keep its id (`{}`)", c.name)));
                    }
                    let file = super::design_link::absolute(&super::design_link::resolve(own, &link.path));
                    let mut o = Map::new();
                    o.insert("path".into(), json!(file.to_string_lossy()));
                    o.insert("name".into(), json!(c.name));
                    out.push(step("component.insert_design", Value::Object(o)));
                    comp_map.insert(c.id, c.id);
                    occ_map.insert(c.id.saturating_add(1), c.id.saturating_add(1));
                    sim_next = c.id.saturating_add(3);
                    omitted.push(format!("linked design `{}`: the file `{}` must be present at replay", c.name, file.to_string_lossy()));
                    if let Some(f) = doc.features.iter().find(|f| f.id == link.feature) {
                        known.extend(f.param_names.iter().filter(|n| !n.is_empty()).cloned());
                    }
                } else {
                    let mut params = json!({"name": c.name, "grounded": false, "activate": false});
                    if let Some(o) = params.as_object_mut()
                        && c.parent != 0
                    {
                        let parent = doc.components.iter().find(|p| p.id == c.parent).map(|p| p.name.clone()).unwrap_or_default();
                        o.insert("parent".into(), json!(parent));
                    }
                    out.push(step("component.create", params));
                    comp_map.insert(c.id, sim_next);
                    occ_map.insert(c.id.saturating_add(1), sim_next.saturating_add(1));
                    sim_next = sim_next.saturating_add(2);
                }
                place_occurrence(doc, c.id.saturating_add(1), &name, &mut out);
                let (metadata, odd) = component_metadata_step(c)?;
                if let Some(step) = metadata {
                    out.push(step);
                }
                if !odd.is_empty() {
                    omitted.push(format!("part properties of `{}` component.metadata.set can't set ({})", c.name, odd.join(", ")));
                }
            }
            Event::Copy(o) => {
                let name = replay_occurrence_name(doc, &sim_occ, o.component);
                sim_occ.push((o.component, name.clone()));
                if o.name != name {
                    return Err(not_supported(&format!("an occurrence the replay would name differently (`{}` would become `{}`)", o.name, name)));
                }
                occ_map.insert(o.id, sim_next);
                sim_next = sim_next.saturating_add(1);
                let source = doc
                    .occurrences
                    .iter()
                    .find(|s| s.component == o.component && s.id.saturating_add(0) < o.id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                out.push(step("occurrence.copy", json!({"occurrence": source})));
                place_occurrence(doc, o.id, &name, &mut out);
            }
        }
    }
    flush(&mut pending, &mut known, &mut out);
    if let Some(p) = pending.first() {
        return Err(EngineError::Other(format!(
            "parameter `{}` refers to a parameter this design does not define (or to itself): fix it before exporting a script",
            p.name
        )));
    }

    // ---- assembly: named origins, joints, motion links ----
    // Joints come after placements (a move after a joint can be re-solved) and
    // after parameters (joint expressions evaluate at creation). Values are
    // stored in radians and millimetres; commands read degrees and millimetres.
    for origin in &doc.assembly.origins {
        if let Some(arg) = joint_origin_arg(&origin.origin, doc)? {
            let mut params = json!({"name": origin.name});
            if let (Some(o), Some(k)) = (params.as_object_mut(), arg.as_object()) {
                for (key, v) in k {
                    o.insert(key.clone(), v.clone());
                }
            }
            out.push(step("joint.origin", params));
        } else {
            omitted.push(format!("named joint origin `{}` (its snap is outside this export)", origin.name));
        }
    }
    for j in &doc.assembly.joints {
        if j.constraint.is_some() {
            return Err(not_supported(&format!("a joint made as an assembly constraint (`{}`)", j.name)));
        }
        let kind = match j.kind {
            solidcraft_doc::joints::JointKind::Rigid => "rigid",
            solidcraft_doc::joints::JointKind::Revolute => "revolute",
            solidcraft_doc::joints::JointKind::Slider => "slider",
            solidcraft_doc::joints::JointKind::Cylindrical => "cylindrical",
            solidcraft_doc::joints::JointKind::PinSlot => "pin_slot",
            solidcraft_doc::joints::JointKind::Planar => "planar",
            solidcraft_doc::joints::JointKind::Ball => "ball",
            solidcraft_doc::joints::JointKind::Parallel => "parallel",
            solidcraft_doc::joints::JointKind::Perpendicular => "perpendicular",
            solidcraft_doc::joints::JointKind::Angle => "angle",
            solidcraft_doc::joints::JointKind::PointOnPlane => "point_on_plane",
            solidcraft_doc::joints::JointKind::PointOnLine => "point_on_line",
            solidcraft_doc::joints::JointKind::LineOnPlane => "line_on_plane",
        };
        let a = joint_origin_arg(&j.a, doc)?;
        let b = joint_origin_arg(&j.b, doc)?;
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(not_supported(&format!("joint `{}` uses a snap this export cannot express", j.name))),
        };
        let mut o = Map::new();
        o.insert("type".into(), json!(kind));
        o.insert("a".into(), a);
        o.insert("b".into(), b);
        if !j.name.is_empty() {
            o.insert("name".into(), json!(j.name));
        }
        if j.flip {
            o.insert("flip".into(), json!(true));
        }
        // The offset and the rest angle travel as they are stored, zero
        // included: a value left out would be re-inferred at creation from
        // wherever the parts sit (`least_turn_angle`), not restored.
        match (&j.exprs.offset_x, j.offset_xy[0]) {
            (Some(e), _) => {
                o.insert("offset_x".into(), json!(e));
            }
            (None, v) if v.abs() > 1e-12 => {
                o.insert("offset_x".into(), json!(v));
            }
            _ => {}
        }
        match (&j.exprs.offset_y, j.offset_xy[1]) {
            (Some(e), _) => {
                o.insert("offset_y".into(), json!(e));
            }
            (None, v) if v.abs() > 1e-12 => {
                o.insert("offset_y".into(), json!(v));
            }
            _ => {}
        }
        match (&j.exprs.offset, j.offset) {
            (Some(e), _) => {
                o.insert("offset".into(), json!(e));
            }
            (None, v) => {
                o.insert("offset".into(), json!(v));
            }
        }
        match (&j.exprs.angle, j.angle) {
            (Some(e), _) => {
                o.insert("angle".into(), json!(e));
            }
            (None, v) => {
                o.insert("angle".into(), json!(v.to_degrees()));
            }
        }
        // Values travel explicitly (degrees for angles, millimetres for
        // distances; expression strings verbatim). A joint's values are
        // anchored at its snap mate, not at its creation pose, so creating it
        // where the parts are with the stored values re-solves to the stored
        // pose and keeps them; leaving them out would start it at zero.
        let values: Vec<Value> = j
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| match j.exprs.values.get(i).and_then(|e| e.as_ref()) {
                Some(e) => json!(e),
                None if j.kind.is_angle(i) => json!(v.to_degrees()),
                None => json!(v),
            })
            .collect();
        if !values.is_empty() {
            o.insert("values".into(), Value::Array(values));
        }
        out.push(step("joint.create", Value::Object(o)));
        if j.limits.iter().any(|l| l.is_some()) {
            for (i, l) in j.limits.iter().enumerate() {
                if let Some((min, max)) = l {
                    let (min, max) = if j.kind.is_angle(i) { (min.to_degrees(), max.to_degrees()) } else { (*min, *max) };
                    out.push(step("joint.limits", json!({"joint": j.name, "index": i, "min": min, "max": max})));
                }
            }
        }
        if j.suppressed {
            out.push(step("joint.edit", json!({"joint": j.name, "suppressed": true})));
        }
        if j.captured.is_some() {
            omitted.push(format!("captured position of joint `{}`", j.name));
        }
    }
    for link in &doc.assembly.links {
        let ja = doc.assembly.joints.iter().find(|j| j.id == link.a);
        let jb = doc.assembly.joints.iter().find(|j| j.id == link.b);
        let (Some(ja), Some(jb)) = (ja, jb) else {
            return Err(not_supported("a motion link on a joint this design does not keep"));
        };
        let (a, b) = (ja.name.clone(), jb.name.clone());
        match &link.coupling {
            None => {
                let mut o = Map::new();
                o.insert("a".into(), json!(a));
                o.insert("b".into(), json!(b));
                // The given ratio is in b's units per a's (mm per degree,
                // degrees per mm); the stored one is per radian. The offset is
                // in b's units, degrees where b's linked value turns.
                let unit = |angle: bool| if angle { 1f64.to_radians() } else { 1.0 };
                let angle_a = ja.kind.is_angle(link.ia);
                let angle_b = jb.kind.is_angle(link.ib);
                let scale = unit(angle_b) / unit(angle_a);
                o.insert("ratio".into(), json!(link.ratio / scale));
                if link.offset != 0.0 {
                    if angle_b {
                        o.insert("offset".into(), json!(link.offset.to_degrees()));
                    } else {
                        o.insert("offset".into(), json!(link.offset));
                    }
                }
                if link.ia != 0 {
                    o.insert("ia".into(), json!(link.ia));
                }
                if link.ib != 0 {
                    o.insert("ib".into(), json!(link.ib));
                }
                out.push(step("joint.motion_link", Value::Object(o)));
            }
            Some(coupling) => return Err(not_supported(&format!("a {} coupling on joints `{a}` and `{b}`", coupling_kind(coupling),))),
        }
    }
    if doc.assembly.contact_enabled || doc.assembly.contact_all || !doc.assembly.contact_sets.is_empty() {
        omitted.push("contact sets".to_string());
    }
    if !doc.assembly.studies.is_empty() {
        omitted.push("motion studies".to_string());
    }
    if !doc.assembly.traces.is_empty() {
        omitted.push("motion traces".to_string());
    }
    if !doc.assembly.exploded.is_empty() {
        omitted.push("exploded views".to_string());
    }

    // Comments and favourites of feature inputs and dimensions.
    for (name, comment) in &doc.param_comments {
        out.push(step("parameters.comment", json!({"name": name, "comment": comment})));
    }
    let users: BTreeSet<&str> = doc.params.iter().filter(|p| !p.model).map(|p| p.name.as_str()).collect();
    for name in doc.favorites.iter().filter(|n| !users.contains(n.as_str())) {
        out.push(step("parameters.favorite", json!({"name": name, "favorite": true})));
    }

    // Materials and plastic rules of bodies, once the bodies exist. The
    // default material is no assignment (material.assign removes one) and
    // comes back with whatever inherited it, so it travels as nothing.
    let mut by_material: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (body, material) in &doc.materials {
        if *material == solidcraft_doc::DEFAULT_MATERIAL {
            continue;
        }
        by_material.entry(material.as_str()).or_default().push(body.as_str());
    }
    for (material, bodies) in by_material {
        out.push(step("material.assign", json!({"bodies": bodies, "material": material})));
    }
    let mut by_rule: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (body, rule) in &doc.plastic.assigned {
        by_rule.entry(rule.as_str()).or_default().push(body.as_str());
    }
    for (rule, bodies) in by_rule {
        out.push(step("plastic.assign_rule", json!({"bodies": bodies, "rule": rule})));
    }

    // The configuration table, once the features, parameters and bodies its columns name exist.
    if !doc.configs.is_empty() {
        match config_steps(doc, has_body, &mut omitted) {
            Ok(steps) => out.extend(steps),
            Err(why) => omitted.push(format!("configurations ({why})")),
        }
    }

    if let Some(m) = doc.marker.filter(|m| *m < doc.features.len()) {
        out.push(step("timeline.rollback", json!({"position": m})));
    }
    Ok((out, omitted))
}

/// The steps that rebuild the configuration table: its columns, its rows, its rules, then the
/// active row. Row ids are not kept (a row is found by its name). `Err` says why the table
/// can't be written, and then none of it is: a replay that stops at one of its steps is worse
/// than a table listed as left out.
fn config_steps(doc: &Document, has_body: &dyn Fn(&str) -> bool, omitted: &mut Vec<String>) -> std::result::Result<Vec<Value>, String> {
    let table = &doc.configs;
    let mut out = Vec::new();
    // The parameters the script creates: the design's own and the inputs the features name. Text
    // parameters are not among them.
    let known = doc.all_param_exprs();
    for column in &table.columns {
        let params = match column {
            Column::Param(name) => {
                if !known.iter().any(|(known, _, _)| known == name) {
                    return Err(format!("a column for parameter `{name}`, which the script does not create"));
                }
                json!({"param": name})
            }
            Column::Suppress(id) => {
                // `config.column` finds the feature by name, so the name has to pick out this one.
                let name = doc.feature(*id).map(|f| f.name.as_str()).filter(|n| doc.find_feature(n).is_some_and(|f| f.id == *id));
                json!({"feature": name.ok_or("a suppression column whose feature a name does not pick out")?})
            }
            // `config.column` finds the body in the design as the replay has built it by then. A
            // body that is not there now (its feature is suppressed) is not there then either.
            Column::Material(body) | Column::Appearance(body) if !has_body(body) => {
                let kind = if matches!(column, Column::Material(_)) { "material" } else { "appearance" };
                return Err(format!("a {kind} column for body `{body}`, which is not in the design as it is now"));
            }
            Column::Material(body) => json!({"material": body}),
            Column::Appearance(body) => json!({"appearance": body}),
            Column::ComponentAppearance(_) | Column::FaceAppearance(_) => return Err("component or face appearance columns".into()),
        };
        out.push(step("config.column", params));
    }
    let mut names = BTreeSet::new();
    for row in &table.rows {
        let name = row.name.as_str();
        if name.is_empty() || name.trim() != name || name.len() > 128 || !names.insert(name) || row.cells.len() != table.columns.len() {
            return Err(format!("row `{name}` that config.row can't add by name"));
        }
        let values: Map<String, Value> = table.columns.iter().zip(&row.cells).map(|(c, v)| (doc.config_key(c), json!(v))).collect();
        out.push(step("config.row", json!({"name": name, "values": values})));
    }
    if !table.rules.is_empty() {
        // `config.rule` makes every row pass the rules, so a stored row they refuse (a parameter
        // changed after the row was made) would stop the replay there.
        for row in &table.rows {
            if doc.clone().apply_configuration(row.id).is_err() {
                return Err(format!("row `{}`, which the rules refuse", row.name));
            }
        }
        let rules = serde_json::to_value(&table.rules).map_err(|e| e.to_string())?;
        out.push(step("config.rule", json!({"rules": rules})));
    }
    // Activating a row sets the design to it, so only a design that still matches its row can
    // have it activated without changing.
    if let Some(active) = &table.active {
        if doc.configuration_matches() {
            out.push(step("config.activate", json!({"row": active})));
        } else {
            omitted.push("the active configuration (the design no longer matches its row)".to_string());
        }
    }
    Ok(out)
}

/// The occurrence name the replay will generate: the component name, a colon,
/// and the first number that is neither taken by a surviving occurrence of
/// the component nor by any occurrence's name at all.
fn replay_occurrence_name(doc: &Document, sim: &[(u64, String)], comp: u64) -> String {
    let cname = doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
    let next = sim.iter().filter(|(c, _)| *c == comp).count() + 1;
    let taken = |k: usize| {
        let name = format!("{cname}:{k}");
        sim.iter().any(|(_, n)| *n == name)
    };
    let k = std::iter::once(next).chain(1..).find(|k| !taken(*k)).unwrap_or(next);
    format!("{cname}:{k}")
}

/// Rewrite the component and occurrence references one exported feature carries
/// to the ids a replay gives them (`components`, `occurrences`: the design's id
/// to the replay's). A feature keeps its own id. Every reference the maps do not
/// know is an error: it would dangle at replay. A path deeper than one level is
/// a nested occurrence path, which this export cannot predict and refuses.
pub(crate) fn remap_replay_refs(value: &mut Value, components: &BTreeMap<u64, u64>, occurrences: &BTreeMap<u64, u64>) -> Result<()> {
    fn mapped(map: &BTreeMap<u64, u64>, id: u64) -> Result<u64> {
        if id == 0 {
            return Ok(0);
        }
        map.get(&id).copied().ok_or_else(|| not_supported("a reference to an item this design does not have"))
    }
    fn path(value: &mut Value, occurrences: &BTreeMap<u64, u64>) -> Result<()> {
        let Some(list) = value.as_array_mut() else {
            return Ok(());
        };
        // An occurrence path is a list of ids; a pattern's or sweep's path of
        // curve names is not one.
        if !list.iter().all(|v| v.is_u64()) {
            return Ok(());
        }
        if list.len() > 1 {
            let shown = serde_json::to_string(list).unwrap_or_default();
            return Err(not_supported(&format!("a nested occurrence path ({shown})")));
        }
        for slot in list.iter_mut() {
            let id = slot.as_u64().ok_or_else(|| not_supported("a reference to an item this design does not have"))?;
            *slot = json!(mapped(occurrences, id)?);
        }
        Ok(())
    }
    fn datum(s: &mut String, occurrences: &BTreeMap<u64, u64>) -> Result<()> {
        let Some(mut d) = solidcraft_doc::construct::DatumRef::parse_key(s) else {
            return Ok(());
        };
        if d.path.len() > 1 {
            return Err(not_supported(&format!("a nested occurrence path (`{s}`)")));
        }
        for id in &mut d.path {
            *id = mapped(occurrences, *id)?;
        }
        *s = d.key();
        Ok(())
    }
    match value {
        Value::Object(o) => {
            for (key, v) in o.iter_mut() {
                match key.as_str() {
                    "component" => {
                        if let Value::Number(n) = v {
                            let id = n.as_u64().ok_or_else(|| not_supported("a reference to an item this design does not have"))?;
                            *v = json!(mapped(components, id)?);
                        }
                    }
                    "path" | "author_path" if v.is_array() => path(v, occurrences)?,
                    _ => remap_replay_refs(v, components, occurrences)?,
                }
            }
        }
        Value::Array(a) => {
            for v in a.iter_mut() {
                remap_replay_refs(v, components, occurrences)?;
            }
        }
        Value::String(s) => datum(s, occurrences)?,
        _ => {}
    }
    Ok(())
}

/// A joint snap as the joint commands take it: world coordinates (flat
/// assemblies make the occurrence transform the world placement), converted
/// from the component-local snap. `None` when the snap is outside this export.
fn joint_origin_arg(origin: &solidcraft_doc::joints::JointOrigin, doc: &Document) -> Result<Option<Value>> {
    use solidcraft_doc::joints::Snap;
    if !origin.inside.is_empty() {
        return Err(not_supported("a nested occurrence path (an origin on another copy of a sub-assembly)"));
    }
    let occ = doc.occurrences.iter().find(|o| o.id == origin.occurrence).map(|o| o.name.clone()).unwrap_or_default();
    if occ.is_empty() {
        return Ok(None);
    }
    let m = doc.occurrences.iter().find(|o| o.id == origin.occurrence).map(|o| o.transform).unwrap_or([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let world = |p: solidcore_record::Vec3| -> [f64; 3] {
        let p = solidcraft_doc::apply_point(&m, p);
        [p.x, p.y, p.z]
    };
    let dir = |d: solidcore_record::Vec3| -> [f64; 3] {
        let d = solidcraft_doc::apply_vector(&m, d);
        [d.x, d.y, d.z]
    };
    let mut arg = Map::new();
    arg.insert("occurrence".into(), json!(occ));
    // A face or circle's x reference as the joint commands take it: a
    // direction or a face pick in world coordinates. A named face with no pick
    // travels as its name; one with a pick travels as the pick alone, which
    // names the face again at replay and keeps the pick.
    let xref = |x: &solidcraft_doc::joints::XRef| -> Value {
        use solidcraft_doc::joints::XRef;
        match x {
            XRef::Dir(d) => json!(dir(*d)),
            XRef::Face { face, pick: None } => json!(face),
            XRef::Face { pick: Some(p), .. } => json!({"face": world(*p)}),
            XRef::Reference { reference, reverse } => json!({"reference":reference,"reverse":reverse}),
            XRef::Planar { planar_face, reverse } => json!({"planar_face":planar_face,"reverse":reverse}),
        }
    };
    match &origin.snap {
        Snap::Frame { origin: o, z, x } => {
            let mut frame = Map::new();
            frame.insert("origin".into(), json!(world(*o)));
            frame.insert("z".into(), json!(dir(*z)));
            if let Some(x) = x {
                frame.insert("x".into(), json!(dir(*x)));
            }
            arg.insert("frame".into(), Value::Object(frame));
        }
        Snap::Point { point, z, x, on } => {
            // On a sketch point: by the sketch's name and the point's id, so the replayed joint
            // follows the point too (#21815).
            match on.as_ref().and_then(|on| doc.feature(on.sketch).map(|f| (f.name.clone(), on.point.clone()))) {
                Some((sketch, id)) => {
                    arg.insert("sketch".into(), json!(sketch));
                    arg.insert("point".into(), json!(id));
                }
                None => {
                    arg.insert("point".into(), json!(world(*point)));
                }
            }
            if let Some(z) = z {
                arg.insert("z".into(), json!(dir(*z)));
            }
            if let Some(x) = x {
                arg.insert("x".into(), json!(dir(*x)));
            }
        }
        Snap::CircleCenter { pick, end, x, .. } => {
            arg.insert("circle".into(), json!(world(*pick)));
            if let Some(end) = end {
                arg.insert("end".into(), json!(end));
            }
            if let Some(x) = x {
                arg.insert("x".into(), xref(x));
            }
        }
        Snap::FaceCenter { pick, x, .. } => {
            arg.insert("face".into(), json!(world(*pick)));
            if let Some(x) = x {
                arg.insert("x".into(), xref(x));
            }
        }
        _ => return Ok(None),
    }
    Ok(Some(Value::Object(arg)))
}

/// The coupling kind name, for the refusal message.
fn coupling_kind(coupling: &solidcraft_doc::joints::Coupling) -> &'static str {
    use solidcraft_doc::joints::Coupling;
    match coupling {
        Coupling::Gear { .. } => "gear",
        Coupling::RackPinion { .. } => "rack-and-pinion",
        Coupling::Screw { .. } => "screw",
        Coupling::Cam(_) => "cam",
    }
}

/// A placed occurrence: the move to its stored transform, then its grounding.
/// The move wants the occurrence free: a design's first part is grounded when
/// it is made, so it is ungrounded first and grounded again after.
fn place_occurrence(doc: &Document, id: u64, name: &str, out: &mut Vec<Value>) {
    let Some(o) = doc.occurrences.iter().find(|o| o.id == id) else {
        return;
    };
    if o.grounded {
        out.push(step("occurrence.ground", json!({"occurrence": name, "grounded": false})));
    }
    let m = o.transform;
    // Row-major: the stored columns read out across the rows.
    let to_matrix: Vec<f64> = (0..3).flat_map(|row| m.iter().map(move |col| col[row])).collect();
    out.push(step("occurrence.move", json!({"occurrence": name, "space": "local", "to_matrix": to_matrix})));
    if o.grounded {
        out.push(step("occurrence.ground", json!({"occurrence": name, "grounded": true})));
    }
}

/// A standard part rebuilt at the part's own ids by `parts.insert`, in a
/// scratch session whose next id is the component's and whose names are the
/// ones the replay will have taken by then (the surviving features below the
/// part's ids, with the input parameter names they carry). The rebuilt
/// features must equal the design's own exactly: anything else is builder
/// output someone hand-edited, deleted or suppressed, which the replay would
/// silently replace with the library's, so the export refuses it instead. The
/// features the builder made are returned: the timeline loop skips them and
/// the part's step rebuilds them.
fn rebuilt_part(doc: &Document, comp: u64, part: &solidcraft_doc::StandardPart, brings: &BTreeMap<u64, Vec<&Param>>) -> Result<Vec<Feature>> {
    let name = doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).unwrap_or_default();
    let what = |why: &str| not_supported(&format!("a standard part whose builder output was {why} (`{name}`)"));
    let mut scratch = Session::default();
    {
        let d = scratch.doc_mut();
        d.next_id = comp;
        for f in doc.features.iter().filter(|f| f.id < comp) {
            d.features.push(Feature { suppressed: true, ..f.clone() });
        }
        // The parameters in use below the part: its own come with its builder.
        let earlier: BTreeSet<&str> = brings.range(..comp).flat_map(|(_, ps)| ps.iter().map(|p| p.name.as_str())).collect();
        d.params = doc.params.iter().filter(|p| !p.model || earlier.contains(p.name.as_str())).cloned().collect();
    }
    let mut o = Map::new();
    o.insert("family".into(), json!(part.family));
    o.insert("size".into(), json!(part.size));
    if let Some(length) = part.length {
        o.insert("length".into(), json!(length));
    }
    o.insert("name".into(), json!(name));
    scratch.execute("parts.insert", &Value::Object(o)).map_err(|_| what("not rebuildable from its library entry"))?;
    // The export runs nested inside its own command, where a command pins no
    // body names; the replay's parts.insert is outermost and pins them, as the
    // design's own insert did.
    scratch.pin_body_names();
    let fresh: Vec<Feature> = scratch.doc.features.iter().filter(|f| f.id > comp.saturating_add(1)).cloned().collect();
    for f in &fresh {
        match doc.features.iter().find(|g| g.id == f.id) {
            Some(g) if g == f => {}
            Some(g) if g.suppressed => return Err(what("suppressed")),
            Some(_) => return Err(what("hand-edited")),
            None => return Err(what("deleted")),
        }
    }
    Ok(fresh)
}

/// A component's part properties and free-text properties as one
/// `component.metadata.set` step (the constructors seed defaults the design
/// may have edited), with the keys the command would refuse listed out. The
/// well-known keys the constructors seed and the design dropped are cleared.
fn component_metadata_step(c: &solidcraft_doc::Component) -> Result<(Option<Value>, Vec<String>)> {
    use solidcraft_doc::metadata::{BOM_KEYS, BOM_TYPE, Owner};
    let mut values = Map::new();
    let mut odd = Vec::new();
    for (key, value) in &c.metadata.0 {
        let mut probe = solidcraft_doc::Metadata::default();
        if probe.set(Owner::Component, key, value.clone()).is_err() {
            odd.push(key.clone());
        } else {
            values.insert(key.clone(), value.clone());
        }
    }
    for key in BOM_KEYS.iter().chain([&BOM_TYPE]) {
        if !c.metadata.0.contains_key(*key) {
            values.insert((*key).to_string(), Value::Null);
        }
    }
    if values.is_empty() && c.properties.is_empty() {
        return Ok((None, odd));
    }
    let mut params = Map::new();
    params.insert("component".into(), json!(c.name));
    params.insert("values".into(), Value::Object(values));
    if !c.properties.is_empty() {
        params.insert("properties".into(), json!(c.properties));
    }
    Ok((Some(step("component.metadata.set", Value::Object(params))), odd))
}
