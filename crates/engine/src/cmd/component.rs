//! Components and occurrences: the design as a tree (see `solidcraft_doc` assembly docs).
//! New sketches and features go into the active component, authored in its frame; bodies
//! belong to their feature's component unless moved; occurrences place components (instances
//! share contents, "paste new" copies them) and can be grounded and moved.

use std::collections::HashMap;

use serde_json::{Value, json};
use solidcraft_doc::{Feature, FeatureKind, Mat, Occurrence};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, str_, string_list, vec3};
use crate::schema::component::{
    EditInPlaceParams, EditInPlaceResult, FinishEditParams, FinishEditResult, MoveFeaturesParams, MoveFeaturesResult, MoveSketchesToPartsParams,
    MoveSketchesToPartsResult, OccurrenceAlignParams, OccurrenceAlignResult, ReparentItemsParams, ReparentItemsResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const EDIT_IN_PLACE_CONTRACT: TypedContract<EditInPlaceParams> = TypedContract::new();
const FINISH_EDIT_CONTRACT: TypedContract<FinishEditParams> = TypedContract::new();
const MOVE_FEATURES_CONTRACT: TypedContract<MoveFeaturesParams> = TypedContract::new();
const MOVE_SKETCHES_TO_PARTS_CONTRACT: TypedContract<MoveSketchesToPartsParams> = TypedContract::new();
const REPARENT_ITEMS_CONTRACT: TypedContract<ReparentItemsParams> = TypedContract::new();

#[path = "component_align.rs"]
mod align;
#[path = "component_drag.rs"]
mod drag;
#[path = "component_foreign.rs"]
mod foreign;
#[path = "component_matrix.rs"]
pub(super) mod matrix;
#[path = "component_move_features.rs"]
mod move_features;
#[path = "component_placing.rs"]
pub(super) mod placing;
#[path = "component_properties.rs"]
pub(super) mod properties;
#[path = "component_reparent.rs"]
mod reparent;
#[path = "component_reparent_items.rs"]
mod reparent_items;
#[path = "component_sketches_to_parts.rs"]
mod sketches_to_parts;
#[path = "component_transfer.rs"]
mod transfer;

#[cfg(test)]
#[path = "component_history_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod history_tests;
#[cfg(test)]
#[path = "occurrence_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod occurrence_tests;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("component.cycle_color", "Cycle Component Color", cycle_color)
        .icon("colored_components").doc_only()
        .params("component: id or name; saves the next identification colour and enables Component Colors; shared by every instance, separate from appearances"),
    CommandSpec::new("component.hide_history", "Hide Component History", hide_history)
        .icon("eye").doc_only()
        .params("component: id or name; keeps the component's features in the design but leaves them out of the History bar (#10393)"),
    CommandSpec::new("component.show_history", "Show Component History", show_history)
        .icon("eye").doc_only()
        .params("component: id or name; puts a component's features back in the History bar after Hide Component History (#10393)"),
    CommandSpec::new("component.create", "New Component", new_component)
        .at("SOLID", "ASSEMBLE")
        .icon("component")
        .params("name?, parent?: component id or name (default: the active one); parent_path?: exact parent placement; activate?: bool (default true); grounded?: bool (default: true for the design's first part, false otherwise)"),
    CommandSpec::new("component.from_bodies", "Create Components from Bodies", from_bodies)
        .icon("component")
        .params("bodies: [names] → one new component per body, named after it"),
    CommandSpec::new("component.activate", "Activate Component", activate).icon("activate").noundo().params("component: id or name (0 or \"root\" for the design); occurrence?, path?: exact placement used to author shared geometry"),
    CommandSpec::new("component.edit_in_place", "Edit in Place", crate::checked!("component.edit_in_place", edit_in_place))
        .icon("edit")
        .noundo()
        .doc_only()
        .contract(&EDIT_IN_PLACE_CONTRACT)
        .output(output_schema_for::<EditInPlaceResult>)
        .params(
            "component: id or name of a linked component; path?: its placement (occurrence ids, default its first) → in the app \
             window: open the design it links in a tab of its own with the assembly drawn around it (component.finish_edit goes \
             back); fails when it is not linked, the assembly has no file yet, something is open or running in it, or the file is \
             missing, on a network location not allowed yet or open in another SolidCraft; asked during the agent's turn it opens \
             once the turn ends (queued: true); elsewhere it only checks",
        ),
    CommandSpec::new("component.finish_edit", "Finish Edit", crate::checked!("component.finish_edit", finish_edit))
        .icon("finish_edit")
        .noundo()
        .doc_only()
        .contract(&FINISH_EDIT_CONTRACT)
        .output(output_schema_for::<FinishEditResult>)
        .params(
            "no parameters → in the app window: go back from the linked design edited in place to its assembly, which reads it again; \
             fails while the design has unsaved changes (run file.save first) or something is open in it; asked during the agent's \
             turn it finishes once the turn ends (queued: true); elsewhere nothing changes",
        ),
    CommandSpec::new("component.list", "List Components", list).icon("list").noundo().params(
        "compact?: bool (counts of bodies, features, sketches and construction planes instead of their names; occurrences leave out \
         identity transforms; children as occurrence ids); bodies?: bool (with compact: body names, not their count)",
    ),
    CommandSpec::new("component.properties", "Component Physical Properties", properties::component)
        .icon("measure").noundo()
        .params("component?: id or name (default active); descendants?: bool (default true); about?: [x, y, z] (mm). Measures the first placement, or all placements for root, including pending moves; material-aware mass and center of mass in world coordinates; volume, density, mass, center of mass and inertia_g_mm2 (the inertia tensor about the body's center of mass, world axes, from its tessellation) only for closed, consistently oriented bodies; total.open_bodies_skipped counts open bodies excluded from volume/mass totals; total.first_moment_g_mm (mass × center of mass), inertia_g_mm2 about the total center of mass, principal_moments_g_mm2 (smallest first) and principal_axes (unit, right-handed); with about, total.inertia_about {point, inertia_g_mm2}"),
    CommandSpec::new("occurrence.properties", "Occurrence Physical Properties", properties::occurrence)
        .icon("measure").noundo()
        .params("occurrence: id or name; path?: [occurrence ids] (required if nested under multiple instances); descendants?: bool (default true); about?: [x, y, z] (mm). Physical properties of this placement in world coordinates, including pending moves; volume, density, mass, center of mass and inertia_g_mm2 only for closed, consistently oriented bodies; total.open_bodies_skipped counts open bodies excluded from volume/mass totals; total inertia as in component.properties"),
    CommandSpec::new("component.rename", "Rename Component", rename).icon("rename").params("component, name"),
    CommandSpec::new("component.delete", "Delete Component", delete).icon("delete").params("component: deletes it, its subcomponents, occurrences and features"),
    CommandSpec::new("component.unlock", "Unlock Contents", unlock).icon("rigid_group").params(
        "component: removes the rigid joints that hold its contents together (an imported assembly's rigid group), so its parts \
         move and take joints one by one → joints (the ids removed)",
    ),
    CommandSpec::new("component.move_bodies", "Move Bodies to Component", move_bodies).icon("move").params(
        "bodies: [names], component; component_path?: its placement to place them by (default: its only one, else the active \
         one; a component placed more than once gets them in every instance); from_path?: the placement of their component they \
         move from (default: its only one, else its first, where its bodies have their own names)",
    ),
    CommandSpec::new("component.copy_bodies", "Paste Bodies", copy_bodies).icon("paste").params(
        "bodies: [names, as the Browser shows them: a body seen in another placement of its component (`Cube (B:2)`) is copied \
         from there]; component?: where the copies go (default: the active one); component_path?: its placement to place them \
         by (default: its only one, else the active one; a component placed more than once gets them in every instance). Each \
         copy stays where its body is in the world; the copies' names come back as `bodies`",
    ),
    CommandSpec::new("component.paste_from_design", "Paste from Another Design", foreign::paste_from_design).icon("paste").params(
        "component?: where it goes (default: the active one); component_path?: its placement (default: the active one, else its \
         only one). Pastes what was copied in another design, at the same world coordinates: its bodies as imported bodies \
         (no history) in one Import feature, or its components as independent copies with their history, appearances, \
         joints and motion (as `component.paste_new`); the user parameters they use come along, renamed `w_2` when taken. \
         A component that uses what isn't in it (another body, sketch or parameter) is refused, saying what. Returns \
         `bodies` and `feature`, or the pasted components' `paths`",
    ),
    CommandSpec::new("component.move_sketches", "Move Sketches to Component", move_sketches)
        .icon("move")
        .params("sketches: [ids or names], component; the sketches keep their own coordinates (component.move_features keeps them where they are in the world), except that a sketch on an exactly attached face stays where it is on the face"),
    CommandSpec::new(
        "component.move_sketches.to_parts",
        "Move Sketches to Their Parts",
        crate::checked!("component.move_sketches.to_parts", sketches_to_parts::to_parts),
    )
    .icon("move")
    .contract(&MOVE_SKETCHES_TO_PARTS_CONTRACT)
    .output(output_schema_for::<MoveSketchesToPartsResult>)
    .params(
        "sketches?: [ids or names] (default: every sketch in the design) → moved, stayed, sketches [{sketch, name, component, part}], \
         kept [{sketch, name, code, reason}]. Each sketch whose plane is an exactly attached face of a part's body goes to that part, as \
         component.move_sketches moves it, and stays where it is on the face; the others (on an origin, construction or fixed \
         plane, on a body of the design itself, or already in their part) stay where they are. One undo step",
    ),
    CommandSpec::new("component.move_features", "Move Features to Component", crate::checked!("component.move_features", move_features::move_features))
        .icon("move")
        .contract(&MOVE_FEATURES_CONTRACT)
        .output(output_schema_for::<MoveFeaturesResult>)
        .params(
            "features: [ids or names] (sketches too); component?: where they go (default: the active one) → moved, component, bodies. \
             Ids, names and timeline order stay; the geometry stays where it is in the world; the bodies the features made move with \
             them. Refused while the timeline is rolled back, for a component placed more than once, and for a move that would break \
             a feature (nothing changes)",
        ),
    CommandSpec::new("browser.reparent", "Move to Component", crate::checked!("browser.reparent", reparent_items::reparent_items))
        .icon("move")
        .contract(&REPARENT_ITEMS_CONTRACT)
        .output(output_schema_for::<ReparentItemsResult>)
        .params(
            "Moves placements, features and bodies into a component. occurrences?: [ids or paths], features?: [ids or names], bodies?: [names], component?: where they go (default: the active \
             one), component_path?: its placement → component, occurrences, features, bodies (what moved). One undo step, all or \
             nothing: placements move first (taking what they hold), then features (taking the bodies they made), then the bodies \
             still elsewhere (a body transfer, which leaves its features behind)",
        ),
    CommandSpec::new("occurrence.copy", "Paste (Instance)", copy_occurrence).icon("copy")
        .params("component (or occurrence): what to place again; path?: validates selected occurrence; parent?: component (default: the same parent); translate?: [x,y,z]; in_place?: bool (default false: the selected transform within the parent; true: where the selected placement is in the world, translate in world axes; a parent placed more than once is placed by parent_path?, else the active placement, and every instance gets the copy). Copies the selected effective transform, including pending moves"),
    CommandSpec::new("component.paste_new", "Paste New", paste_new).icon("paste").params("component: independently copy its nested definition graph and history, materials and styles; occurrence?, path?: selected source placement; parent?: component the copy goes in (default: the source placement's; not the component or one inside it); in_place?, parent_path?: as occurrence.copy; translate?: [x,y,z]. Reassigned external bodies become base geometry; retained external links and omitted boundary joints are reported"),
    CommandSpec::new("occurrence.move", "Move Occurrence", move_occurrence).icon("move")
        .params("occurrence; path?: exact placement; space?: local|world (default local); translate?: [x,y,z]; axis?: [x,y,z], angle?: radians or \"30 deg\", origin?: [x,y,z]; matrix?: [12 or 16 numbers] (instead of translate/axis/angle/origin: a rigid move as a row-major 3x4 rotation-and-translation or 4x4 matrix, flat or as rows; one that is a rotation only up to rounding is orthonormalised, a scale, shear or mirror is refused); tolerance?: largest accepted |RᵀR − I| (default 1e-9, at most 1e-3); capture?: bool (default true; false keeps it pending until Capture Position); move_grounded?: bool (default false: a grounded occurrence refuses; true moves it and it stays grounded). Nested occurrence definitions remain shared"),
    CommandSpec::new("occurrence.drag", "Drag Occurrence", drag::drag).icon("move").params(
        "occurrence or path: exact placement; at: [x,y,z] world point on it; point: [x,y,z] where it goes. Moves the placement by \
         point - at (with any chain of joints it is in that is joined to nothing grounded) and captures its position; other pending \
         moves stay pending. Shared definitions move in all placements. Grounded parts and parts joints hold to something fixed are refused (joint.reach moves those)",
    ),
    CommandSpec::new("occurrence.align", "Align Occurrence", crate::checked!("occurrence.align", align::align))
        .icon("align")
        .contract(&ALIGN_CONTRACT)
        .output(output_schema_for::<OccurrenceAlignResult>)
        .params(
            "occurrence?, occurrences?, path?, paths?: placements to move together; from | from_face | from_plane; to | to_face | to_plane; \
             flip?: bool; capture?: bool (default true); move_grounded?: bool (default false). Moves only the chosen occurrence placements in \
             world coordinates without changing shared component geometry; a placement inside a component placed more than once moves in \
             every placement of it, and the reply's `shared` lists those paths",
        ),
    CommandSpec::new("occurrence.ground", "Ground", ground).icon("ground").params("occurrence; grounded?: bool (default toggles)"),
    CommandSpec::new("occurrence.reparent", "Move to Component", reparent::reparent).icon("move").params(
        "occurrence; path?: exact placement; parent: component id or name (0 or \"root\" for the design); parent_path?: the parent's \
         placement to place it by (default: its only one, else the active one; every instance of the parent gets it). The placement \
         keeps its world position; a move that would break a feature is refused",
    ),
    CommandSpec::new("component.capture_position", "Capture Position", capture).at("SOLID", "POSITION").icon("capture").params("keeps pending occurrence moves"),
    CommandSpec::new("component.revert_position", "Revert Position", revert).at("SOLID", "POSITION").icon("revert").noundo().params("occurrences?: [ids] (default all); drops selected pending occurrence moves"),
];

const ALIGN_CONTRACT: TypedContract<OccurrenceAlignParams> = TypedContract::new();

fn key(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn component(s: &Session, p: &Value, k: &str, cmd: &str) -> Result<u64> {
    let kk = key(p.get(k)).ok_or_else(|| bad(cmd, format!("`{k}` must be a component id or name")))?;
    s.doc.find_component(&kk).ok_or_else(|| bad(cmd, format!("no component `{kk}`")))
}

fn occurrence(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let kk = key(p.get("occurrence")).ok_or_else(|| bad(cmd, "`occurrence` must be an id or name"))?;
    s.doc.occurrences.iter().find(|o| o.id.to_string() == kk || o.name == kk).map(|o| o.id).ok_or_else(|| bad(cmd, format!("no occurrence `{kk}`")))
}

fn translation(t: Vec3) -> Mat {
    solidcraft_doc::rigid(t, Vec3::ZERO, Vec3::Z, 0.0)
}

fn move_vector(p: &Value, key: &str, default: Vec3, coordinate: bool, cmd: &str) -> Result<Vec3> {
    let v = match p.get(key) {
        None => default,
        Some(value) => vec3(value).ok_or_else(|| bad(cmd, format!("{key} must contain three finite numbers")))?,
    };
    if !v.is_finite() || (coordinate && [v.x, v.y, v.z].iter().any(|x| x.abs() > 1e9)) {
        return Err(bad(cmd, format!("{key} exceeds supported numeric range")));
    }
    Ok(v)
}

pub(super) fn valid_position(m: &Mat) -> bool {
    let column = |i: usize| Vec3::new(m[i][0], m[i][1], m[i][2]);
    let (x, y, z) = (column(0), column(1), column(2));
    m.iter().flatten().all(|x| x.is_finite())
        && m[3][..3].iter().all(|x| x.abs() <= 1e9)
        && [x, y, z].iter().all(|axis| (axis.dot(*axis) - 1.0).abs() < 1e-6)
        && x.dot(y).abs() < 1e-6
        && x.dot(z).abs() < 1e-6
        && y.dot(z).abs() < 1e-6
        && x.cross(y).dot(z) > 0.0
        && [m[0][3], m[1][3], m[2][3]].iter().all(|x| x.abs() < 1e-9)
        && (m[3][3] - 1.0).abs() < 1e-9
}

pub(super) fn new_component(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.create";
    if let Some(params) = p.as_object()
        && let Some(key) = params.keys().find(|key| !matches!(key.as_str(), "name" | "parent" | "parent_path" | "activate" | "grounded"))
    {
        let hint = if params.contains_key("bodies") { "; use component.move_bodies or component.from_bodies to move bodies" } else { "" };
        return Err(bad(cmd, format!("unknown parameter `{key}`; accepted parameters: name, parent, parent_path, activate, grounded{hint}")));
    }
    let parent = if p.get("parent").is_some() { component(s, p, "parent", cmd)? } else { s.active_component };
    let parent_path = if let Some(value) = p.get("parent_path") {
        let path = value
            .as_array()
            .filter(|a| a.len() < 64)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .ok_or_else(|| bad(cmd, "parent_path must list up to 63 occurrence ids"))?;
        let component = if path.is_empty() { 0 } else { s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?.component };
        if component != parent {
            return Err(bad(cmd, "parent_path belongs to another component"));
        }
        path
    } else if parent == s.active_component {
        s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(parent))
    } else {
        s.doc.first_path(parent)
    };
    if parent_path.len() >= 64 {
        return Err(bad(cmd, "new component exceeds occurrence depth limit"));
    }
    let grounded = match p.get("grounded") {
        None => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(cmd, "grounded must be true or false"))?),
    };
    let id = s.doc_mut().add_component(str_(p, "name"), parent)?;
    if let Some(grounded) = grounded
        && let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.component == id)
    {
        o.grounded = grounded;
    }
    if p.get("activate").and_then(Value::as_bool).unwrap_or(true) {
        let mut path = parent_path;
        if let Some(o) = s.doc.occurrence_of(id) {
            path.push(o.id);
        }
        s.active_component = id;
        s.active_occurrence_path = Some(path);
    }
    let name = s.doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
    let occ = s.doc.occurrence_of(id);
    Ok(json!({"component": id, "name": name, "parent": parent, "occurrence": occ.map(|o| o.id), "grounded": occ.is_some_and(|o| o.grounded)}))
}

fn from_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    transfer::from_bodies(s, p)
}

/// The app window opens the edit in place from what this returns; here the component and its
/// placement are only checked.
fn edit_in_place(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.edit_in_place";
    let id = component(s, p, "component", cmd)?;
    if !s.doc.links.get(&id).is_some_and(|link| link.step_names.is_none()) {
        return Err(bad(cmd, "that component is not linked to a design"));
    }
    let path = match p.get("path") {
        Some(_) => serde_json::from_value::<Vec<u64>>(p["path"].clone()).map_err(|_| bad(cmd, "`path` must be occurrence ids"))?,
        None => s.doc.first_path(id),
    };
    if s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?.component != id {
        return Err(bad(cmd, "`path` is a placement of another component"));
    }
    serde_json::to_value(EditInPlaceResult { component: id, path, ..Default::default() }).map_err(|e| crate::EngineError::Other(e.to_string()))
}

/// The app window finishes an edit in place (it knows the part's tab and its assembly); here
/// there is none, so nothing changes.
fn finish_edit(_s: &mut Session, _p: &Value) -> Result<Value> {
    serde_json::to_value(FinishEditResult::default()).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn activate(s: &mut Session, p: &Value) -> Result<Value> {
    let id = component(s, p, "component", "component.activate")?;
    let path = if p.get("path").is_some() || p.get("occurrence").is_some() {
        let occurrence = occurrence(s, p, "component.activate")?;
        let path = occurrence_path(s, p, occurrence, "component.activate")?;
        if s.occurrence_placement(&path)?.component != id {
            return Err(bad("component.activate", "path belongs to another component"));
        }
        Some(path)
    } else {
        None
    };
    s.active_component = id;
    s.active_occurrence_path = path;
    s.revision += 1;
    Ok(json!({ "active": id }))
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let doc = &s.doc;
    // Compact: what a large assembly needs to be found around in, without every name and matrix.
    let compact = bool_(p, "compact").unwrap_or(false);
    let names = |v: Vec<&str>| if compact { json!(v.len()) } else { json!(v) };
    let body_names = !compact || bool_(p, "bodies").unwrap_or(false);
    let placement = |o: &Occurrence| {
        if !compact {
            return json!(o);
        }
        let mut v = json!({"id": o.id, "name": o.name, "parent": o.parent, "grounded": o.grounded});
        if o.transform.iter().flatten().zip(solidcraft_doc::IDENTITY.iter().flatten()).any(|(a, b)| (a - b).abs() > 1e-12) {
            v["transform"] = json!(o.transform);
        }
        v
    };
    // Each component's bodies, features, occurrences and children, grouped once rather than
    // searched for every component (thousands of each in a design with many components).
    let mut bodies_of: HashMap<u64, Vec<&str>> = HashMap::new();
    for b in &st.bodies {
        bodies_of.entry(doc.model_body_component(b)).or_default().push(b.name.as_str());
    }
    let mut features_of: HashMap<u64, Vec<&Feature>> = HashMap::new();
    for f in &doc.features {
        features_of.entry(f.component).or_default().push(f);
    }
    let (mut occurrences_of, mut children_of): (HashMap<u64, Vec<&Occurrence>>, HashMap<u64, Vec<&Occurrence>>) = Default::default();
    for o in &doc.occurrences {
        occurrences_of.entry(o.component).or_default().push(o);
        children_of.entry(o.parent).or_default().push(o);
    }
    let node = |id: u64, name: &str, parent: Option<u64>| {
        let bodies = bodies_of.get(&id).cloned().unwrap_or_default();
        let features = features_of.get(&id).map(Vec::as_slice).unwrap_or_default();
        let named = |keep: fn(&FeatureKind) -> bool| -> Vec<&str> { features.iter().filter(|f| keep(&f.kind)).map(|f| f.name.as_str()).collect() };
        let sketches = named(|k| matches!(k, FeatureKind::Sketch { .. }));
        let construction = named(|k| matches!(k, FeatureKind::ConstructionPlane { .. }));
        let features = named(|k| !matches!(k, FeatureKind::Sketch { .. } | FeatureKind::ConstructionPlane { .. }));
        let occurrences: Vec<Value> = occurrences_of.get(&id).into_iter().flatten().map(|o| placement(o)).collect();
        let children: Vec<Value> = children_of.get(&id).into_iter().flatten().map(|o| if compact { json!(o.id) } else { json!(o) }).collect();
        let bodies = if body_names { json!(bodies) } else { json!(bodies.len()) };
        json!({"id": id, "name": name, "parent": parent, "bodies": bodies, "features": names(features), "sketches": names(sketches),
            "construction": names(construction), "occurrences": occurrences, "children": children})
    };
    let mut out = vec![node(0, &doc.name, None)];
    for c in &doc.components {
        out.push(node(c.id, &c.name, Some(c.parent)));
    }
    Ok(json!({"active": s.active_component, "components": out, "pending_moves": s.pending_moves.keys().collect::<Vec<_>>()}))
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.rename";
    let id = component(s, p, "component", cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    let d = s.doc_mut();
    if id == 0 {
        d.name = name.to_string();
    } else if let Some(c) = d.components.iter_mut().find(|c| c.id == id) {
        let old = std::mem::replace(&mut c.name, name.to_string());
        // Placements still named after the component ("Old:2") follow it (#1665); others keep theirs.
        for o in d.occurrences.iter_mut().filter(|o| o.component == id) {
            let k = o.name.strip_prefix(old.as_str()).and_then(|r| r.strip_prefix(':'));
            if let Some(k) = k.filter(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit())) {
                o.name = format!("{name}:{k}");
            }
        }
    }
    Ok(json!({"component": id, "name": name}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.delete";
    let id = component(s, p, "component", cmd)?;
    if id == 0 {
        return Err(bad(cmd, "the design itself can't be deleted"));
    }
    let removed = s.doc_mut().remove_component_tree(id).map_err(|e| match e {
        solidcraft_doc::DocError::Invalid(msg) => bad(cmd, msg),
        e => e.into(),
    })?;
    let gone = removed.components;
    if gone.contains(&s.active_component) {
        s.active_component = 0;
    }
    Ok(json!({"deleted": gone, "features": removed.features}))
}

fn unlock(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.unlock";
    let id = component(s, p, "component", cmd)?;
    if id == 0 {
        return Err(bad(cmd, "pick a component, not the design"));
    }
    let gone = s.contents_joints(id);
    if gone.is_empty() {
        return Err(bad(cmd, "nothing holds this component's contents together"));
    }
    let doc = s.doc_mut();
    doc.assembly.joints.retain(|j| !gone.contains(&j.id));
    for group in &mut doc.timeline_groups {
        group.joints.retain(|id| !gone.contains(id));
    }
    doc.assembly.links.retain(|l| !gone.contains(&l.a) && !gone.contains(&l.b));
    s.visibility.hidden_joints.retain(|h| !gone.contains(h));
    Ok(json!({"component": id, "joints": gone}))
}

fn move_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    transfer::move_bodies(s, p)
}

/// Copies of bodies in a component (Ctrl+C, Ctrl+V on bodies): each body is copied in place in
/// its own component, then the copies move to the target keeping their place in the world, as
/// `component.move_bodies` keeps it. One step to undo.
fn copy_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.copy_bodies";
    let to = if p.get("component").is_some() { component(s, p, "component", cmd)? } else { s.active_component };
    let names = string_list(cmd, p, "bodies")?;
    if names.is_empty() || names.len() > 1000 {
        return Err(bad(cmd, "`bodies` must list 1…1000 bodies"));
    }
    let state = s.model.state();
    // Bodies by their component, and the placement they are copied as: a body is named as its
    // component's first placement shows it, and one seen in another placement (`Cube (B:2)`) is
    // that body, copied from where that placement shows it.
    let mut groups: std::collections::BTreeMap<(u64, Option<Vec<u64>>), Vec<String>> = std::collections::BTreeMap::new();
    let mut placed_bodies = None;
    for name in &names {
        if let Some(body) = state.body(name) {
            groups.entry((s.doc.model_body_component(body), None)).or_default().push(name.clone());
            continue;
        }
        let placed = placed_bodies.get_or_insert_with(|| s.doc.body_frames_with_source(&state));
        let (_, source, component, path, _) = placed.iter().find(|(shown, ..)| shown == name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
        groups.entry((*component, Some(path.clone()))).or_default().push(source.clone());
    }
    drop(state);
    let mut scratch = s.scratch();
    let mut copies = Vec::new();
    for ((from, from_path), bodies) in groups {
        // The copy is made where its body is, so it starts out in the same place.
        scratch.active_component = from;
        scratch.active_occurrence_path = None;
        let mut made = string_list(cmd, &scratch.execute("solid.move", &json!({"bodies": bodies, "copy": true}))?, "created_bodies")?;
        // Each copy is its body's name numbered as all copies are (`Cube (2)`, `Cube (2)(2)`): the
        // move names a copy of `Cube (2)` `Cube (2) (2)`.
        if made.len() == bodies.len() {
            for (copy, original) in made.iter_mut().zip(&bodies) {
                let state = scratch.model.state();
                let name = solidcraft_doc::copy_name(original, |n| n != copy.as_str() && state.body(n).is_some());
                drop(state);
                if name != *copy {
                    scratch.execute("body.rename", &json!({"body": copy, "name": name}))?;
                    *copy = name;
                }
            }
        }
        // The active placement again: a target placed more than once is placed by it.
        scratch.active_component = s.active_component;
        scratch.active_occurrence_path = s.active_occurrence_path.clone();
        if from != to && !made.is_empty() {
            let mut moved = json!({"bodies": made, "component": to});
            if let Some(path) = p.get("component_path") {
                moved["component_path"] = path.clone();
            }
            if let Some(path) = from_path {
                moved["from_path"] = json!(path);
            }
            transfer::move_bodies(&mut scratch, &moved)?;
        }
        copies.extend(made);
    }
    // The steps above are this command's one undo step.
    scratch.undo.clear();
    scratch.selection = copies.iter().map(|name| crate::Sel::Body { name: name.clone() }).collect();
    s.accept_computed(scratch);
    Ok(json!({"bodies": copies, "component": to}))
}

pub(crate) fn transfer_constructor_result(s: &mut Session, p: &Value) -> Result<Value> {
    transfer::move_result(s, p)
}

fn move_sketches(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.move_sketches";
    let to = component(s, p, "component", cmd)?;
    let keys = string_list(cmd, p, "sketches")?;
    let mut ids = Vec::new();
    for k in &keys {
        match s.doc.find_feature(k) {
            Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => ids.push(f.id),
            _ => return Err(bad(cmd, format!("no sketch `{k}`"))),
        }
    }
    if ids.is_empty() {
        return Err(bad(cmd, "`sketches` must list sketches"));
    }
    for id in &ids {
        sketches_to_parts::reauthor(s, *id, to, cmd)?;
        if let Some(f) = s.doc_mut().feature_mut(*id) {
            f.component = to;
        }
    }
    Ok(json!({"sketches": ids, "component": to}))
}

pub(super) fn copy_occurrence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.copy";
    let selected = if p.get("occurrence").is_some() {
        let o = occurrence(s, p, cmd)?;
        Some(o)
    } else {
        None
    };
    let comp = if let Some(id) = selected {
        s.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.component).unwrap_or(0)
    } else {
        component(s, p, "component", cmd)?
    };
    let src = if let Some(id) = selected { s.doc.occurrences.iter().find(|o| o.id == id) } else { s.doc.occurrence_of(comp) }
        .cloned()
        .ok_or_else(|| bad(cmd, "that component has no occurrence"))?;
    if p.get("path").is_some() {
        occurrence_path(s, p, src.id, cmd)?;
    }
    let parent = if p.get("parent").is_some() { component(s, p, "parent", cmd)? } else { src.parent };
    let t = move_vector(p, "translate", Vec3::ZERO, true, cmd)?;
    let m = if bool_(p, "in_place") == Some(true) {
        placing::in_place(s, &occurrence_path(s, p, src.id, cmd)?, parent, placing::parent_path(p, cmd)?.as_deref(), t, cmd)?
    } else {
        let base = s.pending_moves.get(&src.id).unwrap_or(&src.transform);
        solidcraft_doc::mat_mul(&translation(t), base)
    };
    if !valid_position(&m) {
        return Err(bad(cmd, "copy transform exceeds supported numeric range"));
    }
    let id = s.doc_mut().add_occurrence(comp, parent, m)?;
    Ok(json!({"occurrence": id, "component": comp}))
}

fn paste_new(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.paste_new";
    let comp = component(s, p, "component", cmd)?;
    if comp == 0 {
        return Err(bad(cmd, "pick a component, not the design"));
    }
    let src = s.doc.components.iter().find(|c| c.id == comp).cloned().ok_or_else(|| bad(cmd, "component"))?;
    let occ = if p.get("occurrence").is_some() {
        let id = occurrence(s, p, cmd)?;
        if p.get("path").is_some() {
            occurrence_path(s, p, id, cmd)?;
        }
        let occurrence = s.doc.occurrences.iter().find(|o| o.id == id).cloned().ok_or_else(|| bad(cmd, "no occurrence"))?;
        if occurrence.component != comp {
            return Err(bad(cmd, "occurrence belongs to another component"));
        }
        Some(occurrence)
    } else {
        s.doc.occurrence_of(comp).cloned()
    };
    let t = move_vector(p, "translate", Vec3::ZERO, true, cmd)?;
    let parent = if p.get("parent").is_some() { component(s, p, "parent", cmd)? } else { occ.as_ref().map(|o| o.parent).unwrap_or(src.parent) };
    // A copy of a component holds what the component holds: not a place for it to go.
    if s.doc.inside_component(parent, comp) {
        return Err(bad(cmd, format!("can't paste a copy of {} inside itself", src.name)));
    }
    let transform = match occ.as_ref().filter(|_| bool_(p, "in_place") == Some(true)) {
        Some(o) => placing::in_place(s, &occurrence_path(s, p, o.id, cmd)?, parent, placing::parent_path(p, cmd)?.as_deref(), t, cmd)?,
        None => {
            let base = occ.as_ref().map(|o| s.pending_moves.get(&o.id).copied().unwrap_or(o.transform)).unwrap_or(solidcraft_doc::IDENTITY);
            solidcraft_doc::mat_mul(&translation(t), &base)
        }
    };
    if !valid_position(&transform) {
        return Err(bad(cmd, "copy transform exceeds supported numeric range"));
    }
    super::component_clone::paste(s, comp, occ, transform, parent, p)
}

pub(super) fn occurrence_path(s: &Session, p: &Value, id: u64, cmd: &str) -> Result<Vec<u64>> {
    let path = if let Some(value) = p.get("path") {
        value
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 64)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .ok_or_else(|| bad(cmd, "`path` must list 1..64 occurrence ids"))?
    } else {
        let o = s.doc.occurrences.iter().find(|o| o.id == id).ok_or_else(|| bad(cmd, "no occurrence"))?;
        let mut path = s.doc.first_path(o.parent);
        // A world frame requires an explicit containing placement when any ancestor is instanced.
        for ancestor in &path {
            if let Some(a) = s.doc.occurrences.iter().find(|a| a.id == *ancestor)
                && s.doc.occurrences.iter().filter(|x| x.component == a.component).count() > 1
            {
                return Err(bad(cmd, "occurrence has multiple placements; supply its `path`"));
            }
        }
        path.push(id);
        path
    };
    if path.last() != Some(&id) {
        return Err(bad(cmd, "path and occurrence disagree"));
    }
    s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(path)
}

fn move_occurrence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.move";
    let id = occurrence(s, p, cmd)?;
    let o = s.doc.occurrences.iter().find(|o| o.id == id).cloned().ok_or_else(|| bad(cmd, "occurrence"))?;
    if o.grounded && bool_(p, "move_grounded") != Some(true) {
        return Err(bad(
            cmd,
            format!("{} is grounded: unground it first (occurrence.ground; a design's first part is grounded when it is made)", o.name),
        ));
    }
    if let Some(mut group) = s.held_by_rigid_group(id) {
        // The outermost component the group holds it in moves it.
        for _ in 0..64 {
            match s.doc.occurrence_of(group).and_then(|o| s.held_by_rigid_group(o.id)) {
                Some(up) => group = up,
                None => break,
            }
        }
        let name = s.doc.components.iter().find(|c| c.id == group).map(|c| c.name.clone()).unwrap_or_default();
        return Err(bad(cmd, format!("{} is held in {name} by a rigid group: move {name}, or Unlock Contents (component.unlock) first", o.name)));
    }
    let delta = if let Some(m) = p.get("matrix") {
        if let Some(k) = ["translate", "axis", "angle", "origin"].into_iter().find(|k| p.get(*k).is_some()) {
            return Err(bad(cmd, format!("give either matrix or {k}, not both")));
        }
        matrix::rigid(m, matrix::tolerance(p, cmd)?, cmd)?
    } else {
        let t = move_vector(p, "translate", Vec3::ZERO, true, cmd)?;
        let axis = move_vector(p, "axis", Vec3::Z, false, cmd)?;
        let angle = match p.get("angle") {
            Some(Value::String(e)) => s.doc.eval(e, solidcraft_doc::expr::Kind::Angle).map_err(|e| bad(cmd, e.to_string()))?,
            None => 0.0,
            Some(value) => value.as_f64().ok_or_else(|| bad(cmd, "angle must be a number or angle expression"))?,
        };
        if !(t.is_finite() && axis.is_finite() && angle.is_finite()) {
            return Err(bad(cmd, "non-finite move"));
        }
        if angle != 0.0 && axis.normalized().is_none() {
            return Err(bad(cmd, "rotation axis must have a finite nonzero length"));
        }
        let origin = move_vector(p, "origin", Vec3::ZERO, true, cmd)?;
        solidcraft_doc::rigid(t, origin, axis, angle)
    };
    let space = match p.get("space") {
        None => "local",
        Some(Value::String(space)) => space.as_str(),
        _ => return Err(bad(cmd, "space must be local or world")),
    };
    if !matches!(space, "local" | "world") {
        return Err(bad(cmd, "space must be local or world"));
    }
    let path = if p.get("path").is_some() || space == "world" { Some(occurrence_path(s, p, id, cmd)?) } else { None };
    let delta = if space == "world" {
        let placement = s.occurrence_placement(path.as_deref().unwrap_or(&[]))?;
        let inverse = solidcraft_doc::mat_inverse(&placement.parent_world).ok_or_else(|| bad(cmd, "parent frame is singular"))?;
        solidcraft_doc::mat_mul(&solidcraft_doc::mat_mul(&inverse, &delta), &placement.parent_world)
    } else {
        delta
    };
    let cur = if let Some(value) = p.get("base_transform") {
        let base: Mat = serde_json::from_value(value.clone()).map_err(|_| bad(cmd, "base_transform must be a 4x4 affine matrix"))?;
        if !valid_position(&base)
            || base[0][3].abs() > 1e-12
            || base[1][3].abs() > 1e-12
            || base[2][3].abs() > 1e-12
            || (base[3][3] - 1.0).abs() > 1e-12
            || solidcraft_doc::mat_inverse(&base).is_none()
        {
            return Err(bad(cmd, "base_transform must be finite, affine and invertible"));
        }
        base
    } else {
        s.pending_moves.get(&id).copied().unwrap_or(o.transform)
    };
    let next = solidcraft_doc::mat_mul(&delta, &cur);
    if !valid_position(&next) {
        return Err(bad(cmd, "move transform exceeds supported numeric range"));
    }
    if bool_(p, "capture").unwrap_or(true) {
        s.pending_moves.remove(&id);
        if let Some(x) = s.doc_mut().occurrences.iter_mut().find(|x| x.id == id) {
            x.transform = next;
        }
    } else {
        s.pending_moves.insert(id, next);
        s.revision += 1;
    }
    Ok(json!({"occurrence": id, "transform": next, "pending": !bool_(p, "capture").unwrap_or(true)}))
}

fn ground(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.ground";
    let id = occurrence(s, p, cmd)?;
    if p.get("path").is_some() {
        occurrence_path(s, p, id, cmd)?;
    }
    let want = bool_(p, "grounded");
    let o = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id).ok_or_else(|| bad(cmd, "occurrence"))?;
    o.grounded = want.unwrap_or(!o.grounded);
    let g = o.grounded;
    Ok(json!({"occurrence": id, "grounded": g}))
}

fn capture(s: &mut Session, _p: &Value) -> Result<Value> {
    let moves = std::mem::take(&mut s.pending_moves);
    let n = moves.len();
    for (id, m) in moves {
        if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
            o.transform = m;
        }
    }
    s.revision += 1;
    Ok(json!({ "captured": n }))
}

fn revert(s: &mut Session, p: &Value) -> Result<Value> {
    let before = s.pending_moves.len();
    if let Some(value) = p.get("occurrences") {
        let ids = value
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 10_000)
            .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
            .ok_or_else(|| bad("component.revert_position", "occurrences must list 1..10000 ids"))?;
        if ids.iter().any(|id| !s.doc.occurrences.iter().any(|o| o.id == *id)) {
            return Err(bad("component.revert_position", "no occurrence"));
        }
        for id in ids {
            s.pending_moves.remove(&id);
        }
    } else {
        s.pending_moves.clear();
    }
    let n = before - s.pending_moves.len();
    s.revision += 1;
    Ok(json!({ "reverted": n }))
}

/// The active component's frame from world coordinates (for new features' geometry).
pub fn to_active_frame(s: &Session, kind: &mut FeatureKind) {
    // Commands whose picks `frames` maps are already in their frames.
    if s.active_component == 0 || crate::frames::mapped() {
        return;
    }
    let m = s.active_component_frame();
    if solidcraft_doc::is_identity(&m) {
        return;
    }
    if let Some(inv) = solidcraft_doc::mat_inverse(&m) {
        kind.transform_geometry(&inv);
    }
}

fn cycle_color(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.cycle_color";
    let id = component(s, p, "component", cmd)?;
    let next = solidcraft_doc::component_color::next_color(s.doc.component_identification_color(id));
    let c = s.doc_mut().components.iter_mut().find(|c| c.id == id).ok_or_else(|| bad(cmd, "choose a component below the design root"))?;
    c.identification_color = Some(next);
    s.colored_components = Some(());
    Ok(json!({"component": id, "color": next}))
}

/// Mark a component's features out of the History bar, or back in (#10393). The features still
/// rebuild; only the History bar leaves them out.
fn set_history_hidden(s: &mut Session, p: &Value, hidden: bool) -> Result<Value> {
    let cmd = if hidden { "component.hide_history" } else { "component.show_history" };
    let id = component(s, p, "component", cmd)?;
    let c = s.doc_mut().components.iter_mut().find(|c| c.id == id).ok_or_else(|| bad(cmd, "choose a component below the design root"))?;
    c.history_hidden = hidden;
    Ok(json!({"component": id, "history_hidden": hidden}))
}

fn hide_history(s: &mut Session, p: &Value) -> Result<Value> {
    set_history_hidden(s, p, true)
}

fn show_history(s: &mut Session, p: &Value) -> Result<Value> {
    set_history_hidden(s, p, false)
}
