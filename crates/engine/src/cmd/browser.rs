//! Browser commands: renaming bodies, moving a sketch to another plane, and the browser's
//! groups and item order (kept in the design, so they are saved and undone like features).
//!
//! Browser items are keyed per component folder (`bodies`, `sketches`, `construction`): a body
//! by name, a sketch or construction plane by feature id. Expand All / Collapse All return the
//! tree keys the UI opens or closes; they do not change the design.

use std::collections::BTreeSet;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use solidcraft_doc::FeatureKind;

use super::CommandSpec;
use crate::params::{bad, bool_, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("body.rename", "Rename Body", rename_body).icon("rename").params("body: its name, name: the new name (features that use the body follow)"),
    // The import and failed operations offer it for a part the modelling kernel refuses.
    planned!("body.repair", "Repair Part", "8371")
        .icon("body")
        .params("planned: body: its name (a part the modelling kernel refuses: one its import marks as needing repair)"),
    CommandSpec::new("sketch.redefine", "Redefine Sketch Plane", redefine_sketch)
        .icon("plane")
        .params("sketch: id or name; plane: as sketch.create; frame?: as sketch.create; author_path?: exact owner occurrence ids; explicitly rebinds or freezes the plane while preserving geometry and dimension names"),
    CommandSpec::new("browser.group", "New Group", group)
        .icon("folder")
        .params("folder: bodies|sketches|construction; component?: id (default 0); name?; items?: [keys] (they leave other groups)"),
    CommandSpec::new("browser.ungroup", "Ungroup", ungroup).icon("folder").params("group: id (its items go back to the folder)"),
    CommandSpec::new("browser.rename_group", "Rename Group", rename_group).icon("rename").params("group: id, name"),
    CommandSpec::new("browser.move", "Move Browser Items", move_items).icon("reorder")
        .params("folder, component?, items: [keys]; group?: id (into it; omit to take them out of groups); before?: key (position in the group)"),
    CommandSpec::new("document.units", "Change Design Units", document_units)
        .icon("settings")
        .params("units: mm|cm|m|in|ft (display and unit-less parameters; values keep their meaning)"),
    CommandSpec::new("view.save", "New Named View", view_save)
        .icon("camera")
        .params("name?; camera: {target: [x,y,z], yaw, pitch, distance, fov?} (replaces a view of the same name)"),
    CommandSpec::new("view.rename", "Rename Named View", view_rename).icon("rename").params("view: name, name"),
    CommandSpec::new("view.delete", "Delete Named View", view_delete).icon("delete").params("view: name"),
    CommandSpec::new("view.list", "List Named Views", view_list).icon("list").noundo().doc_only(),
    CommandSpec::new("view.apply", "Restore Named View", view_apply).icon("camera").noundo().params(
        "view?: front|back|top|bottom|left|right|iso|home (frames the model) | a named view's name; or camera?: {target: [x,y,z], yaw, pitch, distance, fov?, roll?} (radians, mm; roll defaults to 0, must be finite and is clamped to ±π; up is refused: the up axis comes from the design settings) → camera. The viewport (and the MCP screenshot) moves there. View state: not saved, not an undo step",
    ),
    CommandSpec::new("browser.order", "Order Browser Items", order).icon("reorder").params("folder, component?, order: [keys] (the folder's items in display order)"),
    CommandSpec::new("browser.visibility", "Show/Hide", visibility).icon("eye").params(
        "visible: bool; items?: [sketch id or name | body name | construction name | joint name | O|X|Y|Z|XY|XZ|YZ]; \
         or joints?: [joint id or name] (their glyphs; they still constrain and move parts); \
         or features?: [feature id or name] (the bodies they made; path?: [occurrence ids] for only that placement's); \
         or folder?: bodies|sketches|construction|origin|joints|all (all of it; showing also shows its browser folders and groups, and `all` takes in the occurrence hides too); \
         or tree_folders?: [browser folder or group key: c0/bodies, o:4/sketches, c0/g7, c0/joints, c0/canvases] (the folder or group itself: its items keep their own state); \
         or paths?: [[occurrence ids]] (exact placements and descendants), isolate?: bool (requires paths and visible:true). Saved with the design and undone like any other change (the camera is not)",
    ),
    CommandSpec::new("browser.expand_all", "Expand All", expand_all).icon("folder").noundo().params(
        "path?: [occurrence ids] (that placement and its descendants); component?: id (that component's own folders); \
         omit both for the whole tree, including folders that start closed (Origin, Named Views, Features). \
         Display only: open/closed rows are not saved in the design and are not an undo step → open, open_by_default, closed_by_default",
    ),
    CommandSpec::new("browser.collapse_all", "Collapse All", collapse_all).icon("folder").noundo().params(
        "path?: [occurrence ids]; component?: id; omit both for the whole tree (the design row stays open so the first level remains visible). \
         Display only; not an undo step → open, open_by_default, closed_by_default",
    ),
];

const FOLDERS: [&str; 3] = ["bodies", "sketches", "construction"];

fn folder_of(p: &Value, cmd: &str) -> Result<String> {
    match str_(p, "folder") {
        Some(f) if FOLDERS.contains(&f) => Ok(f.to_string()),
        _ => Err(bad(cmd, "`folder` must be bodies, sketches or construction")),
    }
}

fn component_of(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let c = p.get("component").and_then(Value::as_u64).unwrap_or(0);
    if c != 0 && !s.doc.components.iter().any(|x| x.id == c) {
        return Err(bad(cmd, format!("no component {c}")));
    }
    Ok(c)
}

fn keys(p: &Value, k: &str, cmd: &str) -> Result<Vec<String>> {
    let v = match p.get(k) {
        Some(Value::Array(a)) => {
            a.iter().map(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n| n.to_string()))).collect::<Option<Vec<_>>>()
        }
        None => Some(Vec::new()),
        _ => None,
    };
    let v = v.ok_or_else(|| bad(cmd, format!("`{k}` must be a list of item keys")))?;
    if v.len() > 10_000 || v.iter().any(|x| x.is_empty() || x.len() > 256) {
        return Err(bad(cmd, format!("`{k}` has bad keys")));
    }
    Ok(v)
}

fn group_id(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    let id = p.get("group").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`group` must be a group id"))?;
    if s.doc.browser_groups.iter().any(|g| g.id == id) { Ok(id) } else { Err(bad(cmd, format!("no group {id}"))) }
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.group";
    let folder = folder_of(p, cmd)?;
    let component = component_of(s, p, cmd)?;
    let items = keys(p, "items", cmd)?;
    let d = s.doc_mut();
    let id = d.allocate_id()?;
    let name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.len() <= 128 => n.to_string(),
        Some(_) => return Err(bad(cmd, "`name` is too long")),
        None => format!("Group{}", d.browser_groups.iter().filter(|g| g.folder == folder).count() + 1),
    };
    for g in d.browser_groups.iter_mut().filter(|g| g.component == component && g.folder == folder) {
        g.items.retain(|k| !items.contains(k));
    }
    d.browser_groups.push(solidcraft_doc::BrowserGroup { id, name: name.clone(), component, folder, items });
    Ok(json!({ "group": id, "name": name }))
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_id(s, p, "browser.ungroup")?;
    s.doc_mut().browser_groups.retain(|g| g.id != id);
    Ok(json!({ "group": id }))
}

fn rename_group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.rename_group";
    let id = group_id(s, p, cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if let Some(g) = s.doc_mut().browser_groups.iter_mut().find(|g| g.id == id) {
        g.name = name.to_string();
    }
    Ok(json!({ "group": id, "name": name }))
}

fn move_items(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.move";
    let folder = folder_of(p, cmd)?;
    let component = component_of(s, p, cmd)?;
    let items = keys(p, "items", cmd)?;
    if items.is_empty() {
        return Err(bad(cmd, "`items` must list item keys"));
    }
    let to = match p.get("group") {
        Some(Value::Null) | None => None,
        Some(_) => Some(group_id(s, p, cmd)?),
    };
    if let Some(g) = to
        && !s.doc.browser_groups.iter().any(|x| x.id == g && x.component == component && x.folder == folder)
    {
        return Err(bad(cmd, "that group is in another folder"));
    }
    let before = str_(p, "before").map(str::to_string);
    let d = s.doc_mut();
    for g in d.browser_groups.iter_mut().filter(|g| g.component == component && g.folder == folder) {
        g.items.retain(|k| !items.contains(k));
        if Some(g.id) == to {
            let at = before.as_ref().and_then(|b| g.items.iter().position(|k| k == b)).unwrap_or(g.items.len());
            for (i, k) in items.iter().enumerate() {
                g.items.insert(at + i, k.clone());
            }
        }
    }
    Ok(json!({ "items": items, "group": to }))
}

fn order(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.order";
    let folder = folder_of(p, cmd)?;
    let component = component_of(s, p, cmd)?;
    let order = keys(p, "order", cmd)?;
    let key = format!("{component}/{folder}");
    let d = s.doc_mut();
    if order.is_empty() {
        d.browser_order.remove(&key);
    } else {
        d.browser_order.insert(key, order);
    }
    Ok(json!({}))
}

const ORIGIN: [&str; 7] = ["O", "X", "Y", "Z", "XY", "XZ", "YZ"];

/// A browser item whose visibility can change.
enum Shown {
    Sketch(u64),
    Body(String),
    /// Origin items and construction geometry, by name.
    Origin(String),
    /// An assembly joint's glyph, by joint id.
    Joint(u64),
}

fn is_construction(k: &FeatureKind) -> bool {
    matches!(k, FeatureKind::ConstructionPlane { .. } | FeatureKind::ConstructionAxis { .. } | FeatureKind::ConstructionPoint { .. })
}

/// The item `key` names; bodies are looked for in the placed model `world`.
fn shown_item(s: &Session, world: &solidcraft_doc::ModelState, key: &str) -> Option<Shown> {
    if let Some(f) = s.doc.find_feature(key) {
        if matches!(f.kind, FeatureKind::Sketch { .. }) {
            return Some(Shown::Sketch(f.id));
        }
        if is_construction(&f.kind) {
            return Some(Shown::Origin(f.name.clone()));
        }
    }
    if world.bodies.iter().any(|b| b.name == key) {
        return Some(Shown::Body(key.to_string()));
    }
    if let Some(j) = s.doc.assembly.joints.iter().find(|j| j.name == key) {
        return Some(Shown::Joint(j.id));
    }
    ORIGIN.iter().find(|o| o.eq_ignore_ascii_case(key)).map(|o| Shown::Origin(o.to_string()))
}

/// A browser folder or group key (`Visibility::hidden_folders`): `c0` or `o:<ids>`, then
/// `/bodies`, `/sketches`, `/construction`, `/joints`, `/canvases` or `/g<group id>`.
fn tree_folder_key(k: &str) -> bool {
    let Some((scope, last)) = k.rsplit_once('/') else { return false };
    let scope_ok =
        scope == "c0" || scope.strip_prefix("o:").is_some_and(|ids| ids.split('/').count() <= 64 && ids.split('/').all(|i| i.parse::<u64>().is_ok()));
    let last_ok =
        FOLDERS.contains(&last) || ["joints", "canvases"].contains(&last) || last.strip_prefix('g').is_some_and(|g| g.parse::<u64>().is_ok());
    k.len() <= 2048 && scope_ok && last_ok
}

fn visibility(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "browser.visibility";
    let visible = bool_(p, "visible").ok_or_else(|| bad(cmd, "`visible` must be true or false"))?;
    if let Some(value) = p.get("tree_folders") {
        if ["items", "folder", "joints", "paths"].iter().any(|k| p.get(*k).is_some()) {
            return Err(bad(cmd, "give `tree_folders`, `items`, `folder`, `joints` or `paths`, not several"));
        }
        let keys = value
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 10_000)
            .ok_or_else(|| bad(cmd, "`tree_folders` must list 1..10000 browser folder or group keys"))?;
        // Every key is checked before anything changes.
        let keys: Vec<String> = keys
            .iter()
            .map(|k| {
                k.as_str().filter(|k| tree_folder_key(k)).map(str::to_string).ok_or_else(|| {
                    bad(cmd, "a browser folder or group key is `<c0 | o:ids>/<folder | g<group id>>`, such as `c0/bodies` or `o:4/g7`")
                })
            })
            .collect::<Result<_>>()?;
        for k in &keys {
            s.visibility.hidden_folders.retain(|h| h != k);
            if !visible {
                s.visibility.hidden_folders.push(k.clone());
            }
        }
        s.revision += 1;
        return Ok(json!({"visible":visible,"changed":keys.len(),"visibility":s.visibility}));
    }
    if let Some(value) = p.get("paths") {
        if p.get("items").is_some() || p.get("folder").is_some() {
            return Err(bad(cmd, "give paths, items or folder, not several"));
        }
        let paths =
            value.as_array().filter(|a| !a.is_empty() && a.len() <= 10_000).ok_or_else(|| bad(cmd, "paths must list 1..10000 occurrence paths"))?;
        let paths: Vec<Vec<u64>> = paths
            .iter()
            .map(|value| {
                let path = value
                    .as_array()
                    .filter(|a| !a.is_empty() && a.len() <= 64)
                    .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
                    .ok_or_else(|| bad(cmd, "each path must list 1..64 occurrence ids"))?;
                s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
                Ok(path)
            })
            .collect::<Result<_>>()?;
        if bool_(p, "isolate").unwrap_or(false) {
            if !visible {
                return Err(bad(cmd, "isolate requires visible:true"));
            }
            let mut hidden = Vec::new();
            let mut ancestor_bodies = Vec::new();
            let mut stack = vec![Vec::<u64>::new()];
            let mut count = 0;
            while let Some(parent) = stack.pop() {
                for child in s.occurrence_children(&parent)? {
                    count += 1;
                    if count > 100_000 {
                        return Err(bad(cmd, "isolate exceeds 100000 placements"));
                    }
                    if paths.iter().any(|selected| child.path.starts_with(selected)) {
                        continue;
                    }
                    if paths.iter().any(|selected| selected.starts_with(&child.path)) {
                        ancestor_bodies.extend(child.bodies.into_iter().map(|b| b.world));
                        stack.push(child.path);
                    } else {
                        hidden.push(child.path);
                    }
                }
            }
            let mut root_bodies: Vec<String> =
                s.model.state().bodies.iter().filter(|b| s.doc.model_body_component(b) == 0).map(|b| b.name.clone()).collect();
            root_bodies.extend(ancestor_bodies);
            s.visibility.hidden_occurrences = hidden;
            s.visibility.hidden_bodies = root_bodies;
            s.revision += 1;
            return Ok(json!({"visible":true,"isolated":paths,"visibility":s.visibility}));
        }
        for path in &paths {
            s.visibility.hidden_occurrences.retain(|p| p != path);
            if !visible {
                s.visibility.hidden_occurrences.push(path.clone());
            }
        }
        s.revision += 1;
        return Ok(json!({"visible":visible,"changed":paths.len(),"visibility":s.visibility}));
    }
    let items: Vec<Shown> = match (str_(p, "folder"), p.get("items")) {
        _ if p.get("features").is_some() => {
            if ["items", "folder", "joints"].iter().any(|k| p.get(*k).is_some()) {
                return Err(bad(cmd, "give `features`, `items`, `folder` or `joints`, not several"));
            }
            feature_items(s, p, cmd)?
        }
        _ if p.get("joints").is_some() => {
            if p.get("items").is_some() || p.get("folder").is_some() {
                return Err(bad(cmd, "give `joints`, `items` or `folder`, not several"));
            }
            let list = p
                .get("joints")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty() && a.len() <= 10_000)
                .ok_or_else(|| bad(cmd, "`joints` must list 1..10000 joint ids or names"))?;
            list.iter().map(|j| super::joints::joint_id(s, Some(j), cmd).map(Shown::Joint)).collect::<Result<_>>()?
        }
        (Some(_), Some(_)) => return Err(bad(cmd, "give `items` or `folder`, not both")),
        (Some("bodies"), None) => s.world_state()?.bodies.iter().map(|b| Shown::Body(b.name.clone())).collect(),
        (Some("sketches"), None) => s.model.state().sketches.iter().map(|x| Shown::Sketch(x.feature)).collect(),
        (Some("construction"), None) => s.doc.features.iter().filter(|f| is_construction(&f.kind)).map(|f| Shown::Origin(f.name.clone())).collect(),
        (Some("origin"), None) => ORIGIN.iter().map(|o| Shown::Origin(o.to_string())).collect(),
        (Some("joints"), None) => s.doc.assembly.joints.iter().map(|j| Shown::Joint(j.id)).collect(),
        // Everything at once, for a single "show all" step: every body, sketch, construction
        // item, origin item and joint.
        (Some("all"), None) => {
            let mut all: Vec<Shown> = s.world_state()?.bodies.iter().map(|b| Shown::Body(b.name.clone())).collect();
            all.extend(s.model.state().sketches.iter().map(|x| Shown::Sketch(x.feature)));
            all.extend(s.doc.features.iter().filter(|f| is_construction(&f.kind)).map(|f| Shown::Origin(f.name.clone())));
            all.extend(ORIGIN.iter().map(|o| Shown::Origin(o.to_string())));
            all.extend(s.doc.assembly.joints.iter().map(|j| Shown::Joint(j.id)));
            all
        }
        (Some(f), None) => return Err(bad(cmd, format!("unknown folder `{f}` (bodies, sketches, construction, origin, joints, all)"))),
        (None, Some(_)) => {
            let keys = keys(p, "items", cmd)?;
            if keys.is_empty() {
                return Err(bad(cmd, "`items` must list sketches, bodies or construction geometry"));
            }
            // Every item is found before anything changes.
            let world = s.world_state()?;
            keys.iter()
                .map(|k| shown_item(s, &world, k).ok_or_else(|| bad(cmd, format!("no sketch, body, construction item or joint `{k}`"))))
                .collect::<Result<_>>()?
        }
        (None, None) => return Err(bad(cmd, "`items` or `folder` is required")),
    };
    // Showing all of a kind also shows the browser folders and groups that hold it.
    if visible && let Some(f) = str_(p, "folder") {
        if f == "all" {
            // Everything includes the folders and groups themselves.
            s.visibility.hidden_folders.clear();
        } else {
            let groups: Vec<String> = s.doc.browser_groups.iter().filter(|g| g.folder == f).map(|g| format!("/g{}", g.id)).collect();
            let own = format!("/{f}");
            s.visibility.hidden_folders.retain(|k| !k.ends_with(&own) && !groups.iter().any(|g| k.ends_with(g.as_str())));
        }
    }
    let v = &mut s.visibility;
    match str_(p, "folder") {
        Some("bodies" | "all") if visible => {
            v.hidden_bodies.clear();
            v.hidden_occurrences.clear();
        }
        // Showing a whole folder shows the folder too.
        Some("origin") if visible => v.origin_hidden = Some(false),
        _ => {}
    }
    for it in &items {
        // Each item ends in the state asked for, and asking for the state it already has leaves
        // the lists exactly as they were: no reordering, so a redundant hide shows up as no
        // change at all (no undo step, no unsaved changes).
        match it {
            Shown::Sketch(id) => {
                if visible {
                    v.hidden_sketches.retain(|x| x != id);
                    if !v.shown_sketches.contains(id) {
                        v.shown_sketches.push(*id);
                    }
                } else {
                    v.shown_sketches.retain(|x| x != id);
                    if !v.hidden_sketches.contains(id) {
                        v.hidden_sketches.push(*id);
                    }
                }
            }
            Shown::Body(n) => {
                if visible {
                    v.hidden_bodies.retain(|x| x != n);
                } else if !v.hidden_bodies.contains(n) {
                    v.hidden_bodies.push(n.clone());
                }
            }
            Shown::Origin(n) => {
                if visible {
                    v.hidden_origin.retain(|x| x != n);
                } else if !v.hidden_origin.contains(n) {
                    v.hidden_origin.push(n.clone());
                }
            }
            Shown::Joint(id) => {
                if visible {
                    v.hidden_joints.retain(|x| x != id);
                } else if !v.hidden_joints.contains(id) {
                    v.hidden_joints.push(*id);
                }
            }
        }
    }
    s.revision += 1;
    Ok(json!({ "visible": visible, "changed": items.len(), "visibility": s.visibility }))
}

/// Browser rows that start open (`c0`, Bodies, placements) vs closed (Origin, Features).
struct FoldKeys {
    open: BTreeSet<String>,
    closed: BTreeSet<String>,
}

enum FoldScope {
    Tree,
    Component(u64),
    Occurrence(Vec<u64>),
}

const MAX_FOLD_KEYS: usize = 10_000;
const MAX_FOLD_PLACEMENTS: usize = 100_000;

fn occurrence_fold_key(path: &[u64]) -> String {
    format!("o:{}", path.iter().map(u64::to_string).collect::<Vec<_>>().join("/"))
}

fn fold_path(s: &Session, p: &Value, cmd: &str) -> Result<Vec<u64>> {
    let value = p.get("path").ok_or_else(|| bad(cmd, "`path` is required"))?;
    let path = value
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 64)
        .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
        .ok_or_else(|| bad(cmd, "`path` must list 1..64 occurrence ids"))?;
    s.occurrence_placement(&path).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(path)
}

fn fold_scope(s: &Session, p: &Value, cmd: &str) -> Result<FoldScope> {
    let has_path = p.get("path").is_some();
    let has_component = p.get("component").is_some();
    if has_path && has_component {
        return Err(bad(cmd, "give `path` or `component`, not both"));
    }
    if has_path {
        return Ok(FoldScope::Occurrence(fold_path(s, p, cmd)?));
    }
    if has_component {
        let id = p.get("component").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`component` must be a component id"))?;
        if id != 0 && !s.doc.components.iter().any(|c| c.id == id) {
            return Err(bad(cmd, format!("no component {id}")));
        }
        return Ok(if id == 0 { FoldScope::Tree } else { FoldScope::Component(id) });
    }
    Ok(FoldScope::Tree)
}

fn folder_has_items(s: &Session, component: u64, folder: &str) -> bool {
    match folder {
        "bodies" => s.model.state().bodies.iter().any(|b| s.doc.model_body_component(b) == component),
        "sketches" => s.doc.features.iter().any(|f| f.component == component && matches!(f.kind, FeatureKind::Sketch { .. })),
        "construction" => s.doc.features.iter().any(|f| f.component == component && is_construction(&f.kind)),
        _ => false,
    }
}

fn has_native_features(s: &Session, component: u64) -> bool {
    s.doc.features.iter().any(|f| f.component == component && !matches!(f.kind, FeatureKind::Sketch { .. }))
}

fn collect_component_keys(s: &Session, prefix: &str, component: u64, root_extras: bool, keys: &mut FoldKeys) {
    keys.open.insert(prefix.to_string());
    for folder in FOLDERS {
        if folder_has_items(s, component, folder) {
            keys.open.insert(format!("{prefix}/{folder}"));
        }
    }
    for g in s.doc.browser_groups.iter().filter(|g| g.component == component) {
        keys.open.insert(format!("{prefix}/g{}", g.id));
    }
    if has_native_features(s, component) {
        keys.closed.insert(format!("{prefix}/features"));
    }
    let occurrence = prefix.starts_with("o:");
    for f in s.doc.features.iter().filter(|f| f.component == component) {
        if let FeatureKind::Sketch { sketch, .. } = &f.kind
            && !sketch.patterns.is_empty()
        {
            let key = if occurrence { format!("{prefix}/s:{}/patterns", f.id) } else { format!("s:{}/patterns", f.id) };
            keys.open.insert(key);
        }
    }
    if root_extras {
        keys.closed.insert("docsettings".into());
        keys.closed.insert("namedviews".into());
        keys.closed.insert("origin".into());
        if !s.doc.canvases.is_empty() {
            keys.open.insert("canvases".into());
        }
        if !s.doc.assembly.joints.is_empty() {
            keys.open.insert("c0/joints".into());
        }
        if s.section.is_some() {
            keys.open.insert("analysis".into());
        }
    }
}

fn walk_fold(s: &Session, start: &[u64], root_extras: bool, keys: &mut FoldKeys, cmd: &str) -> Result<()> {
    let mut stack = vec![start.to_vec()];
    let mut count = 0usize;
    while let Some(path) = stack.pop() {
        count += 1;
        if count > MAX_FOLD_PLACEMENTS {
            return Err(bad(cmd, "expand/collapse exceeds 100000 placements"));
        }
        let component = if path.is_empty() { 0 } else { s.occurrence_placement(&path)?.component };
        let prefix = if path.is_empty() { "c0".to_string() } else { occurrence_fold_key(&path) };
        collect_component_keys(s, &prefix, component, root_extras && path.is_empty(), keys);
        for child in s.occurrence_children(&path)? {
            if child.path.len() > 64 {
                continue;
            }
            stack.push(child.path);
        }
    }
    Ok(())
}

fn fold_tree(s: &mut Session, p: &Value, open: bool, cmd: &str) -> Result<Value> {
    let scope = fold_scope(s, p, cmd)?;
    let mut keys = FoldKeys { open: BTreeSet::new(), closed: BTreeSet::new() };
    match &scope {
        FoldScope::Component(id) => collect_component_keys(s, &format!("c{id}"), *id, false, &mut keys),
        FoldScope::Tree => walk_fold(s, &[], true, &mut keys, cmd)?,
        FoldScope::Occurrence(path) => walk_fold(s, path, false, &mut keys, cmd)?,
    }
    if !open && matches!(scope, FoldScope::Tree) {
        keys.open.remove("c0");
    }
    if keys.open.len().saturating_add(keys.closed.len()) > MAX_FOLD_KEYS {
        return Err(bad(cmd, "expand/collapse exceeds 10000 browser rows"));
    }
    Ok(json!({
        "open": open,
        "open_by_default": keys.open.into_iter().collect::<Vec<_>>(),
        "closed_by_default": keys.closed.into_iter().collect::<Vec<_>>(),
    }))
}

fn expand_all(s: &mut Session, p: &Value) -> Result<Value> {
    fold_tree(s, p, true, "browser.expand_all")
}

fn collapse_all(s: &mut Session, p: &Value) -> Result<Value> {
    fold_tree(s, p, false, "browser.collapse_all")
}

/// `features` (and `path`) of `browser.visibility`: the bodies those features made (#5099).
fn feature_items(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Shown>> {
    let keys = keys(p, "features", cmd)?;
    if keys.is_empty() {
        return Err(bad(cmd, "`features` must list feature ids or names"));
    }
    let mut ids = Vec::with_capacity(keys.len());
    let mut component = None;
    for k in &keys {
        let f = s.doc.find_feature(k).ok_or_else(|| bad(cmd, format!("no feature `{k}`")))?;
        if matches!(f.kind, FeatureKind::Sketch { .. }) {
            return Err(bad(cmd, format!("`{k}` is a sketch: give it in `items`")));
        }
        ids.push(f.id);
        component = Some(f.component);
    }
    let path = match p.get("path") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let path = v
                .as_array()
                .filter(|a| a.len() <= 64)
                .and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
                .ok_or_else(|| bad(cmd, "`path` must list at most 64 occurrence ids"))?;
            let placement = s.doc.placements().into_iter().find(|(_, at, _)| *at == path).ok_or_else(|| bad(cmd, "no placement at `path`"))?;
            if ids.iter().any(|id| s.doc.feature(*id).is_some_and(|f| f.component != placement.0)) {
                return Err(bad(cmd, "`path` must be a placement of the features' component"));
            }
            Some((component.unwrap_or(placement.0), path))
        }
    };
    let bodies = s.feature_bodies(&ids, path.as_ref().map(|(c, at)| (*c, at.as_slice())));
    if bodies.is_empty() {
        return Err(bad(cmd, "those features made no bodies that are in the design now (a feature that changes bodies shows and hides with them)"));
    }
    Ok(bodies.into_iter().map(Shown::Body).collect())
}

fn document_units(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "document.units";
    let u = str_(p, "units").ok_or_else(|| bad(cmd, "`units` is required"))?;
    if !["mm", "cm", "m", "in", "ft"].contains(&u) {
        return Err(bad(cmd, "`units` must be mm, cm, m, in or ft"));
    }
    let old = s.doc.units.clone();
    if old == u {
        return Ok(json!({ "units": u, "kept": [] }));
    }
    // What the geometry reads: every feature's length inputs.
    let matched_configuration = s.doc.configuration_matches().then(|| s.doc.configs.active.clone()).flatten();
    let before = s.doc.length_inputs();
    let solved = s.solved_sketches();
    s.doc_mut().units = u.to_string();
    // Unit-less user parameters holding a bare number read it in the design's units where they
    // stand for lengths. If any input moved, those numbers keep the old unit, written out.
    let same = |a: &[Option<f64>]| {
        a.iter().zip(&before).filter(|(x, y)| matches!((x, y), (Some(x), Some(y)) if (x - y).abs() <= 1e-9 * y.abs().max(1.0))).count()
    };
    let after = s.doc.length_inputs();
    let mut kept = Vec::new();
    if same(&after) < before.len() {
        let bare: Vec<(String, String)> = s
            .doc
            .params
            .iter()
            .filter(|p| !p.model && p.unit.is_empty() && solidcraft_doc::expr::is_literal(&p.expr) && p.expr.trim().parse::<f64>().is_ok())
            .map(|p| (p.name.clone(), p.expr.trim().to_string()))
            .collect();
        for (name, e) in bare {
            let now = same(&s.doc.length_inputs());
            let probe = {
                let mut d = (*s.doc).clone();
                let _ = d.change_param(&name, &format!("{e} {old}"), Some(&old), None);
                same(&d.length_inputs())
            };
            // Only when writing the old unit puts more inputs back where they were.
            if probe > now {
                s.doc_mut().change_param(&name, &format!("{e} {old}"), Some(&old), None)?;
                kept.push(name);
            }
        }
    }
    s.doc_mut().preserve_configuration_units(&old, &kept);
    if let Some(row) = matched_configuration {
        s.doc_mut().apply_configuration(&row)?;
    }
    s.settle_value_edit(&solved)?;
    Ok(json!({ "units": u, "kept": kept }))
}

fn view_save(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.save";
    let c = p.get("camera").ok_or_else(|| bad(cmd, "`camera` is required"))?;
    let mut v: solidcraft_doc::NamedView = serde_json::from_value(c.clone()).map_err(|e| bad(cmd, format!("bad camera: {e}")))?;
    let ok = [v.target.x, v.target.y, v.target.z, v.yaw, v.pitch, v.distance, v.fov].iter().all(|x| x.is_finite()) && v.distance > 0.0;
    if !ok {
        return Err(bad(cmd, "the camera must be finite with a positive distance"));
    }
    let n = s.doc.named_views.len();
    v.name = match str_(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.len() <= 128 => n.to_string(),
        Some(_) => return Err(bad(cmd, "`name` is too long")),
        None => (n + 1..).map(|k| format!("Named View{k}")).find(|x| !s.doc.named_views.iter().any(|v| v.name == *x)).unwrap_or_default(),
    };
    if n >= 1000 {
        return Err(bad(cmd, "too many named views"));
    }
    let name = v.name.clone();
    let d = s.doc_mut();
    d.named_views.retain(|x| x.name != name);
    d.named_views.push(v);
    Ok(json!({ "view": name }))
}

fn view_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.apply";
    let cam = match (str_(p, "view"), p.get("camera")) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `view` or `camera`, not both")),
        (None, None) => return Err(bad(cmd, "`view` (a name) or `camera` is required")),
        (None, Some(c)) => {
            if c.get("up").is_some() {
                return Err(bad(cmd, "camera `up` is not supported: the up axis comes from the design settings"));
            }
            let roll = match c.get("roll") {
                Some(value) => {
                    value.as_f64().filter(|r| r.is_finite()).ok_or_else(|| bad(cmd, "camera `roll` must be a finite number in radians"))?
                }
                None => 0.0,
            };
            let v: solidcraft_doc::NamedView = serde_json::from_value(c.clone()).map_err(|e| bad(cmd, format!("bad camera: {e}")))?;
            crate::render::Camera { roll: roll.clamp(-std::f64::consts::PI, std::f64::consts::PI), ..camera_of(&v) }
        }
        // Standard view names come first, as in the browser's Named Views folder.
        (Some(name), None) => match crate::render::StandardView::parse(name) {
            Some(sv) => {
                let mut c = crate::view::home_camera(s)?;
                c.set_view(sv);
                c.fit(&crate::view::bounds(s)?);
                c
            }
            None => s.doc.named_views.iter().find(|v| v.name == name).map(camera_of).ok_or_else(|| bad(cmd, format!("no named view `{name}`")))?,
        },
    };
    if !cam.is_valid() || !cam.fov.is_finite() || cam.fov < 0.0 {
        return Err(bad(cmd, "the camera must be finite with a positive distance"));
    }
    s.camera = Some(cam);
    Ok(json!({ "camera": cam }))
}

/// The orbit camera of a named view (Z up, no roll).
fn camera_of(v: &solidcraft_doc::NamedView) -> crate::render::Camera {
    crate::render::Camera { target: v.target, yaw: v.yaw, pitch: v.pitch, distance: v.distance, fov: v.fov, ..Default::default() }
}

fn view_index(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let k = str_(p, "view").ok_or_else(|| bad(cmd, "`view` must be a view name"))?;
    s.doc.named_views.iter().position(|v| v.name == k).ok_or_else(|| bad(cmd, format!("no named view `{k}`")))
}

fn view_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.rename";
    let i = view_index(s, p, cmd)?;
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if s.doc.named_views.iter().enumerate().any(|(j, v)| j != i && v.name == name) {
        return Err(bad(cmd, format!("another view is called `{name}`")));
    }
    if let Some(v) = s.doc_mut().named_views.get_mut(i) {
        v.name = name.to_string();
    }
    Ok(json!({ "view": name }))
}

fn view_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let i = view_index(s, p, "view.delete")?;
    let v = s.doc_mut().named_views.remove(i);
    Ok(json!({ "deleted": v.name }))
}

fn view_list(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(json!({ "views": s.doc.named_views }))
}

/// The keys whose strings name bodies, alone or in a list (`body`, `targets`…), at any depth of a
/// feature: its own fields and the face, edge and link references inside it.
const BODY_KEYS: [&str; 5] = ["body", "bodies", "target", "targets", "tools"];

/// Replace the body name `old` with `new` where a string names a body (the value of one of
/// [`BODY_KEYS`]); other strings equal to it (sketch text, plane and feature names, expressions)
/// stay. True when something changed.
fn replace_body_name(v: &mut Value, old: &str, new: &str, names_body: bool, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match v {
        Value::String(s) if names_body && s == old => {
            *s = new.to_string();
            true
        }
        Value::Array(a) => a.iter_mut().fold(false, |acc, x| replace_body_name(x, old, new, names_body, depth + 1) | acc),
        Value::Object(o) => o.iter_mut().fold(false, |acc, (k, x)| replace_body_name(x, old, new, BODY_KEYS.contains(&k.as_str()), depth + 1) | acc),
        _ => false,
    }
}

/// A frame member feature's bodies by line id, as its own result made them: what pins each
/// member's name to its line. `None` for any other feature, or when they don't pair up.
fn member_pins(s: &Session, fid: u64) -> Option<BTreeMap<String, String>> {
    let f = s.doc.feature(fid)?;
    let result = s.model.result(fid)?;
    let lines = solidcraft_doc::frame::member_line_ids(&result.state, &f.kind).ok()?;
    let made: Vec<String> = result.state.bodies.iter().filter(|b| b.feature == fid).map(|b| b.name.clone()).collect();
    (lines.len() == made.len()).then(|| lines.into_iter().zip(made).collect())
}

/// Whether the `i`-th new body of `f` is numbered by the body counter (`BodyN`): it has no stored
/// name of its own and is not numbered after the feature's first name (`Ribs (4)`).
pub(crate) fn counted(f: &solidcraft_doc::Feature, i: usize) -> bool {
    let first = f.body_names.first().is_some_and(|n| !n.trim().is_empty());
    f.body_names.get(i).is_none_or(|n| n.trim().is_empty()) && (f.shell_piece_order || i < f.body_names.len() || !first)
}

fn rename_body(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "body.rename";
    let old = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must be a body name"))?.to_string();
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if name == old {
        return Ok(json!({"body": old, "name": name}));
    }
    let previous = s.doc.clone();
    let st = s.model.state();
    let b = st.body(&old).ok_or_else(|| bad(cmd, format!("no body `{old}`")))?;
    if st.body(name).is_some() {
        return Err(bad(cmd, format!("another body is called `{name}`")));
    }
    // Remove keeps a name even after its body leaves the current state. Reusing it would
    // redirect that feature to this body on rebuild (#6042). Inactive removals must keep
    // their targets too, for when the history marker advances or they are unsuppressed.
    if let Some(f) = s.doc.features.iter().find(|f| matches!(&f.kind, FeatureKind::Remove { bodies } if bodies.iter().any(|b| b == name))) {
        return Err(bad(cmd, format!("`{name}` is still used by Remove feature `{}`; choose a different body name", f.name)));
    }
    // The body is the k-th new body of its feature; its siblings keep the names they have now
    // (unpinned pattern copies would otherwise be renumbered into the freed name, #265). That
    // includes the siblings whose stored name is shared: the solids of a STEP file with one label
    // are stored as `X`, `X`, `X` and built as `X`, `X (2)`, `X (3)`, so the stored names are set
    // to the numbered ones, or `X (3)` would turn into `X (2)` once the second solid is renamed.
    // Read the feature's own result so siblings removed by later features keep their names too.
    let fid = b.feature;
    let made = s.model.result(fid).map_or(&st.bodies, |r| &r.state.bodies);
    let siblings: Vec<String> = made.iter().filter(|x| x.feature == fid).map(|x| x.name.clone()).collect();
    let k = siblings.iter().position(|n| *n == old).unwrap_or(0);
    // A body numbered by the counter (`BodyN`) gives its number back once it has a name of its
    // own, and every counted body after it would move down one (`Body3` becoming `Body1`) and
    // break what names it. Those later features keep the names they made too (#1109): each one's
    // own result holds them, also for bodies a later feature joins away.
    // Bodies later features name after this one (`Top (1)`, a split's other piece) keep their
    // names too: they would otherwise follow it, and break what names them (#3510).
    let derived = format!("{old} (");
    let at = s.doc.features.iter().position(|f| f.id == fid);
    let counts = at.and_then(|at| s.doc.features.get(at)).is_some_and(|f| counted(f, k));
    let later: Vec<(u64, Vec<String>)> = match at {
        Some(at) => s
            .doc
            .features
            .iter()
            .skip(at + 1)
            .filter_map(|f| {
                let made: Vec<String> = s.model.result(f.id)?.state.bodies.iter().filter(|x| x.feature == f.id).map(|x| x.name.clone()).collect();
                let keep = (counts && (0..made.len()).any(|i| counted(f, i))) || made.iter().any(|n| n.starts_with(&derived));
                keep.then_some((f.id, made))
            })
            .collect(),
        None => Vec::new(),
    };
    // Frame members are named by their lines (#4003): each keeps the name it has now on its
    // line, whichever lines come and go later, and the renamed one takes its new name.
    let later_pins: Vec<Option<BTreeMap<String, String>>> = later.iter().map(|(id, _)| member_pins(s, *id)).collect();
    let own_pins = member_pins(s, fid).and_then(|mut pins| {
        let line = pins.iter().find(|(_, n)| **n == old).map(|(line, _)| line.clone())?;
        pins.insert(line, name.to_string());
        Some(pins)
    });
    let d = s.doc_mut();
    for ((id, made), pins) in later.iter().zip(later_pins) {
        if let Some(f) = d.feature_mut(*id) {
            if let (Some(pins), FeatureKind::FrameMember { names, .. }) = (pins, &mut f.kind) {
                *names = pins;
                f.body_names.clear();
            } else {
                f.pin_body_names(made);
            }
        }
    }
    let f = d.feature_mut(fid).ok_or_else(|| bad(cmd, "the body's feature is gone"))?;
    if let (Some(pins), FeatureKind::FrameMember { names, .. }) = (own_pins, &mut f.kind) {
        *names = pins;
        f.body_names.clear();
    } else {
        // An Import keeps its names by the file's body index, its skipped bodies' too (#2175).
        let slots = f.body_name_slots(siblings.len().max(k + 1));
        if let Some(end) = slots.last().map(|s| s + 1).filter(|end| f.body_names.len() < *end) {
            f.body_names.resize(end, String::new());
        }
        for (slot, current) in slots.iter().zip(&siblings) {
            if let Some(stored) = f.body_names.get_mut(*slot) {
                stored.clone_from(current);
            }
        }
        if let Some(stored) = slots.get(k).and_then(|slot| f.body_names.get_mut(*slot)) {
            *stored = name.to_string();
        }
    }
    // Later features refer to bodies by name.
    let mut failed = None;
    for f in d.features.iter_mut().filter(|f| f.id != fid) {
        // The bodies a join/cut/intersect may change (a primitive's `targets`) are kept beside
        // the feature's kind, not in it (#1108).
        for b in f.participants.iter_mut().filter(|b| **b == old) {
            *b = name.to_string();
        }
        if let FeatureKind::Sketch { plane, .. } = &mut f.kind {
            solidcraft_doc::sheet::rename_plane_sheet_body(plane, &old, name);
        }
        if let FeatureKind::Sketch { plane, sketch } = &mut f.kind
            && let Some(anchor) = super::sketch::face::anchor_mut(plane)
            && anchor.target.body == old
        {
            let ids = std::collections::BTreeMap::from([(anchor.target.origin_feature, anchor.target.origin_feature)]);
            let bodies = std::collections::BTreeMap::from([(old.clone(), name.to_string())]);
            anchor.remap_target(&ids, &bodies)?;
            for link in &mut sketch.links {
                if link.auto_face && matches!(&link.source,solidcraft_sketch::LinkSource::Face {body,..} if *body==old) {
                    link.face_name = Some(anchor.target.name.clone());
                }
            }
        }
        let Ok(mut v) = serde_json::to_value(&f.kind) else { continue };
        if replace_body_name(&mut v, &old, name, false, 0) {
            match serde_json::from_value(v) {
                Ok(k) => f.kind = k,
                Err(e) => failed = Some(e.to_string()),
            }
        }
    }
    if let Some(e) = failed {
        return Err(bad(cmd, e));
    }
    d.rename_body_keys(&old, name);
    for b in s.visibility.locked_bodies.iter_mut().filter(|b| **b == old) {
        *b = name.to_string();
    }
    s.remember_body_rename(&previous, &old, name);
    Ok(json!({"body": old, "name": name}))
}

fn redefine_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.redefine";
    let key = match p.get("sketch") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`sketch` must be an id or name")),
    };
    let id = match s.doc.find_feature(&key) {
        Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => f.id,
        _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
    };
    if p.get("plane").is_none() {
        return Err(bad(cmd, "`plane` is required"));
    }
    let feature = s.doc.feature(id).ok_or_else(|| bad(cmd, "sketch disappeared"))?;
    let component = feature.component;
    let index = s.doc.feature_index(id).ok_or_else(|| bad(cmd, "sketch disappeared"))?;
    let author_path = super::sketch::author_path_for_rebind(s, feature, p, cmd)?;
    let frame = super::sketch::plane_frame(p, cmd)?;
    let placement = solidcraft_doc::EmbossFace::component_frame(&s.placed_document(), component, &author_path)?;
    let marker = s.doc.marker;
    let mut baseline = s.scratch();
    baseline.doc_mut().marker = Some(index + 1);
    baseline.refresh();
    let visible_geometry = baseline.model.state().sketch(id).map(|resolved| resolved.sketch.clone());
    let mut trial = s.scratch();
    trial.active_component = component;
    trial.active_occurrence_path = (!author_path.is_empty()).then_some(author_path);
    // A new attachment may only depend on geometry preceding this sketch.
    trial.doc_mut().marker = Some(index);
    trial.refresh();
    let old_anchor = trial
        .doc
        .feature(id)
        .and_then(|feature| match &feature.kind {
            FeatureKind::Sketch { plane, .. } => super::sketch::face::anchor(plane),
            _ => None,
        })
        .cloned();
    let plane = super::sketch::plane_ref(&trial, p, cmd)?;
    if super::sketch::face::is_face(p) {
        super::sketch::face::capture_positions(&mut trial, &plane)?;
    }
    if let Some(anchor) = old_anchor
        && super::sketch::face::anchor(&plane)
            .is_none_or(|next| next.target != anchor.target || next.component != anchor.component || next.path != anchor.path)
    {
        let sketch = trial.doc_mut().sketch_mut(id)?;
        if let Some(mut visible) = visible_geometry {
            let constraints: std::collections::BTreeMap<_, _> =
                sketch.constraints.iter().map(|constraint| (constraint.id.clone(), constraint.clone())).collect();
            for constraint in &mut visible.constraints {
                if let Some(original) = constraints.get(&constraint.id) {
                    if std::mem::discriminant(&constraint.kind) != std::mem::discriminant(&original.kind) {
                        return Err(bad(cmd, "a dimension role changed during border refresh; repair it explicitly before rebinding"));
                    }
                    // Keep evaluated entity indices after linked topology rebuilds.
                    constraint.param = original.param.clone();
                    constraint.driven = original.driven;
                    constraint.text = original.text;
                    if let Some(value) = original.kind.value() {
                        constraint.kind.set_value(value);
                    }
                }
            }
            *sketch = visible;
        }
        let affected: Vec<_> = sketch
            .links
            .iter()
            .filter(|link| {
                link.auto_face
                    && link.face_name.as_ref() == Some(&anchor.target.name)
                    && matches!(&link.source,solidcraft_sketch::LinkSource::Face {body,..} if *body==anchor.target.body)
            })
            .map(|link| link.id.clone())
            .collect();
        for link in affected {
            sketch.break_link(&link)?;
        }
    }
    if let Some(FeatureKind::Sketch { plane: slot, .. }) = trial.doc_mut().feature_mut(id).map(|feature| &mut feature.kind) {
        *slot = plane;
    }
    trial.doc_mut().marker = Some(index + 1);
    trial.refresh();
    if let Some(error) = trial.model.result(id).and_then(|result| result.error.as_ref()) {
        return Err(bad(cmd, error.clone()));
    }
    trial.doc_mut().marker = marker;
    trial.refresh();
    s.doc = trial.doc;
    s.pending_moves = trial.pending_moves;

    let attachment = s.doc.feature(id).and_then(|feature| match &feature.kind {
        FeatureKind::Sketch { plane, .. } => Some(if super::sketch::face::anchor(plane).is_some() {
            "exact_face"
        } else if solidcraft_doc::sheet::plane_sheet_face(plane).is_some() {
            "sheet_face"
        } else {
            "fixed"
        }),
        _ => None,
    });
    let mut out = json!({ "sketch": id,"attachment":attachment });
    super::sketch::frame_reply(&mut out, frame, &placement);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn names(s: &Session) -> Vec<String> {
        s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
    }

    #[test]
    fn body_rename_preserves_plastic_rules_and_current_visibility_through_history() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Plate"}));
        run(&mut s, "plastic.manage_rules", json!({"name": "Housing", "draft": "3 deg"}));
        run(&mut s, "plastic.assign_rule", json!({"bodies": ["Plate"], "rule": "Housing"}));
        run(&mut s, "browser.visibility", json!({"items": ["Plate"], "visible": false}));
        run(&mut s, "body.rename", json!({"body": "Plate", "name": "Base"}));
        assert_eq!(run(&mut s, "plastic.rules", json!({}))["assigned"], json!({"Base": "Housing"}));
        assert!(!s.body_visible("Base"));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(run(&mut s, "plastic.rules", json!({}))["assigned"], json!({"Plate": "Housing"}));
        assert!(!s.body_visible("Plate"), "undo must keep the body hidden");
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.body_visible("Base"));
        // Showing a body is its own undo step; the rename keeps the correct hidden name on
        // either side of that step, and neither operation loses the assigned rule.
        run(&mut s, "browser.visibility", json!({"items": ["Base"], "visible": true}));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(names(&s), ["Base"]);
        assert!(!s.body_visible("Base"));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(names(&s), ["Plate"]);
        assert!(!s.body_visible("Plate"));
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.body_visible("Base"));
        run(&mut s, "edit.redo", json!({}));
        assert!(s.body_visible("Base"));
        run(&mut s, "browser.visibility", json!({"items": ["Base"], "visible": false}));
        let mut saved = (*s.doc).clone();
        saved.visibility = s.visibility.clone();
        let reopened = Session::new(serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap());
        assert_eq!(reopened.doc.plastic.assigned.get("Base").map(String::as_str), Some("Housing"));
        assert!(!reopened.body_visible("Base"));
    }

    #[test]
    fn body_rename_undo_keeps_hidden_bodies_with_downstream_users_hidden() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Peg"}));
        run(&mut s, "solid.move", json!({"bodies": ["Peg"], "translate": [0, 0, 5]}));
        run(&mut s, "browser.visibility", json!({"items": ["Peg"], "visible": false}));
        run(&mut s, "body.rename", json!({"body": "Peg", "name": "Stud"}));
        run(&mut s, "edit.undo", json!({}));
        assert!(!s.body_visible("Peg"));
        assert_eq!(s.visibility.hidden_bodies, ["Peg"]);
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.body_visible("Stud"));
    }

    #[test]
    fn show_all_bodies_clears_stale_hidden_names() {
        for folder in ["bodies", "all"] {
            let mut s = Session::default();
            s.visibility.hidden_bodies.push("Old body name".into());
            run(&mut s, "browser.visibility", json!({"folder": folder, "visible": true}));
            assert!(s.visibility.hidden_bodies.is_empty(), "{folder}");
        }
    }

    #[test]
    fn computed_body_rename_keeps_the_latest_visibility_and_undo_mapping() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Peg"}));
        run(&mut s, "solid.move", json!({"bodies": ["Peg"], "translate": [0, 0, 5]}));
        let mut computed = s.scratch();
        run(&mut computed, "body.rename", json!({"body": "Peg", "name": "Stud"}));
        // The user hides the body while the rename is being evaluated.
        run(&mut s, "browser.visibility", json!({"items": ["Peg"], "visible": false}));
        s.accept_computed(computed);
        assert!(!s.body_visible("Stud"));
        assert_eq!(s.visibility.hidden_bodies, ["Stud"]);
        run(&mut s, "edit.undo", json!({}));
        assert!(!s.body_visible("Peg"));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.body_visible("Peg"), "the concurrent hide remains its own undo step");
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.body_visible("Peg"));
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.body_visible("Stud"));
    }

    #[test]
    fn renamed_body_keeps_its_users() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        let [a, b] = [names(&s)[0].clone(), names(&s)[1].clone()];
        run(&mut s, "solid.move", json!({"bodies": [b.clone()], "translate": [0, 0, 5]}));
        run(&mut s, "body.rename", json!({"body": b, "name": "Lid"}));
        assert_eq!(names(&s), vec![a.clone(), "Lid".to_string()]);
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "the move still finds its body");
        // Duplicate names and unknown bodies are refused; undo restores the old name.
        assert!(s.execute("body.rename", &json!({"body": "Lid", "name": a})).is_err());
        assert!(s.execute("body.rename", &json!({"body": "Nope", "name": "X"})).is_err());
        assert!(s.execute("body.rename", &json!({"body": "Lid", "name": " "})).is_err());
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(names(&s)[1], b);
    }

    #[test]
    fn renaming_a_body_to_a_removed_name_preserves_the_design() {
        for removed_name in ["Base", "Body1"] {
            for remove_first in [false, true] {
                let mut s = Session::default();
                let plate = json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"});
                if !remove_first {
                    run(&mut s, "solid.box", plate.clone());
                }
                let mut cube = json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "operation": "new"});
                if removed_name == "Base" {
                    cube["body_name"] = json!(removed_name);
                }
                run(&mut s, "solid.box", cube);
                run(&mut s, "solid.remove", json!({"bodies": [removed_name], "name": "RemoveCube"}));
                if remove_first {
                    run(&mut s, "solid.box", plate);
                }
                assert_eq!(names(&s), ["Plate"]);
                let before = serde_json::to_value(&*s.doc).unwrap();
                let undo = s.undo.len();
                let revision = s.revision;
                let result = s.execute("body.rename", &json!({"body": "Plate", "name": removed_name}));
                assert!(result.is_err(), "rename must not steal {removed_name} from RemoveCube: {result:?}; bodies: {:?}", names(&s));
                let error = result.unwrap_err().to_string();
                assert!(error.contains(removed_name) && error.contains("RemoveCube"), "{error}");
                assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
                assert_eq!(s.undo.len(), undo, "a refused rename adds no undo entry");
                assert_eq!(s.revision, revision);
                assert_eq!(names(&s), ["Plate"]);
                let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
                assert!((volume - 12000.0).abs() < 1e-6);

                // A fresh name still works, including after a full rebuild from saved data.
                run(&mut s, "body.rename", json!({"body": "Plate", "name": "Lid"}));
                run(&mut s, "edit.undo", json!({}));
                assert_eq!(names(&s), ["Plate"]);
                run(&mut s, "edit.redo", json!({}));
                let mut fresh = Session::new(serde_json::from_value(serde_json::to_value(&*s.doc).unwrap()).unwrap());
                fresh.refresh();
                assert_eq!(names(&fresh), ["Lid"]);
                assert!(fresh.model.results.iter().all(|r| r.error.is_none()));
            }
        }
    }

    #[test]
    fn renaming_a_body_cannot_take_an_inactive_removals_name() {
        for rollback in [false, true] {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
            run(
                &mut s,
                "solid.box",
                json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "operation": "new", "body_name": "Base", "name": "Cube"}),
            );
            run(&mut s, "solid.remove", json!({"bodies": ["Base"], "name": "RemoveCube"}));
            if rollback {
                run(&mut s, "timeline.rollback", json!({"position": 1}));
            } else {
                run(&mut s, "timeline.suppress", json!({"features": ["Cube", "RemoveCube"], "suppressed": true}));
            }
            assert_eq!(names(&s), ["Plate"]);
            let before = serde_json::to_value(&*s.doc).unwrap();
            let error = s.execute("body.rename", &json!({"body": "Plate", "name": "Base"})).unwrap_err().to_string();
            assert!(error.contains("RemoveCube"), "{error}");
            assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
            if rollback {
                run(&mut s, "timeline.rollback", json!({"position": "end"}));
            } else {
                run(&mut s, "timeline.suppress", json!({"features": ["Cube", "RemoveCube"], "suppressed": false}));
            }
            assert_eq!(names(&s), ["Plate"]);
            assert!(s.model.results.iter().all(|r| r.error.is_none()));
        }
    }

    #[test]
    fn renaming_a_body_keeps_a_later_sibling_removal() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        for x in [0, 20, 40] {
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [x, 0], "p1": [x + 10, 10]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5, "operation": "new", "name": "Extrude1"}));
        assert_eq!(names(&s), ["Body1", "Body2", "Body3"]);
        run(&mut s, "solid.remove", json!({"bodies": ["Body3"]}));
        assert_eq!(names(&s), ["Body1", "Body2"]);

        run(&mut s, "body.rename", json!({"body": "Body1", "name": "Ext"}));
        assert_eq!(s.doc.find_feature("Extrude1").unwrap().body_names, ["Ext", "Body2", "Body3"]);
        assert_eq!(names(&s), ["Ext", "Body2"]);
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "the remove still finds Body3");

        let mut fresh = Session::new((*s.doc).clone());
        fresh.refresh();
        assert_eq!(names(&fresh), ["Ext", "Body2"]);
        assert!(fresh.model.results.iter().all(|r| r.error.is_none()), "the remove also works after a full evaluation");
    }

    /// #3510: each piece of a split is renamed on its own. Renaming the split body leaves the
    /// other piece, and the features that use it, as they were.
    #[test]
    fn split_pieces_are_renamed_on_their_own() {
        let ok = |s: &Session| s.model.results.iter().all(|r| r.error.is_none());
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, -5], "body_name": "Top"}));
        run(&mut s, "solid.split_body", json!({"body": "Top", "plane": "XY"}));
        assert_eq!(names(&s), vec!["Top", "Top (1)"]);
        run(&mut s, "solid.move", json!({"bodies": ["Top (1)"], "translate": [0, 0, -5]}));
        run(&mut s, "body.rename", json!({"body": "Top", "name": "Upper"}));
        assert_eq!(names(&s), vec!["Upper", "Top (1)"]);
        assert!(ok(&s), "the move still finds the other piece");
        run(&mut s, "body.rename", json!({"body": "Top (1)", "name": "Lower"}));
        assert_eq!(names(&s), vec!["Upper", "Lower"]);
        assert!(ok(&s), "the move follows the renamed piece");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(names(&s), vec!["Upper", "Top (1)"]);
    }

    /// Renaming one half of a split leaves the other half's name, also once a later feature
    /// joins that other half away.
    #[test]
    fn renaming_one_half_of_a_split_leaves_the_other() {
        let ok = |s: &Session| s.model.results.iter().all(|r| r.error.is_none());
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, -5]}));
        run(&mut s, "solid.split_body", json!({"body": "Body1", "plane": "XY"}));
        assert_eq!(names(&s), ["Body1", "Body1 (1)"]);
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [10, 0, -5], "operation": "new", "body_name": "Tab"}));
        run(&mut s, "solid.combine", json!({"target": "Tab", "tools": ["Body1 (1)"], "operation": "join"}));
        assert_eq!(names(&s), ["Body1", "Tab"]);

        run(&mut s, "body.rename", json!({"body": "Body1", "name": "Upper"}));
        assert_eq!(names(&s), ["Upper", "Tab"]);
        assert!(ok(&s), "the join still finds the other half");
        let mut fresh = Session::new((*s.doc).clone());
        fresh.refresh();
        assert_eq!(names(&fresh), ["Upper", "Tab"]);
        assert!(ok(&fresh), "the join also works after a full evaluation");
    }

    /// #1108: a primitive's `targets` are its participants, kept beside its kind; a rename
    /// follows them, as it follows the names inside the kind (a combine's target and tools).
    #[test]
    fn renamed_body_keeps_the_primitives_that_target_it() {
        let mut s = Session::default();
        let total = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "name": "Block", "body_name": "A"}));
        run(&mut s, "solid.cylinder", json!({"diameter": 5, "height": 12, "base": [5, 5, -1], "operation": "cut", "targets": ["A"], "name": "Hole"}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "operation": "new", "body_name": "Tab"}));
        run(&mut s, "solid.combine", json!({"target": "A", "tools": ["Tab"], "operation": "join", "name": "Joined"}));
        let want = total(&mut s);
        run(&mut s, "body.rename", json!({"body": "A", "name": "B"}));
        assert!(
            s.model.results.iter().all(|r| r.error.is_none()),
            "{:?}",
            s.model.results.iter().filter_map(|r| r.error.clone()).collect::<Vec<_>>()
        );
        assert_eq!(s.doc.find_feature("Hole").unwrap().participants, vec!["B".to_string()]);
        assert!((total(&mut s) - want).abs() < 1e-6);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.find_feature("Hole").unwrap().participants, vec!["A".to_string()]);
    }

    /// #1109: renaming counted bodies (`Body1`, `Body2` of one pattern) leaves the counted bodies
    /// of later features as they were (`Body3`, `Body4`), and what names them keeps working.
    #[test]
    fn renaming_counted_bodies_keeps_later_counted_names() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "center": [0, 0, 2.5], "name": "B1", "body_name": "First"}));
        run(&mut s, "solid.pattern.rectangular", json!({"features": ["B1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 10, "name": "P1"}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "center": [0, 20, 2.5], "name": "B2", "body_name": "Second"}));
        run(&mut s, "solid.pattern.rectangular", json!({"features": ["B2"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 10, "name": "P2"}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "center": [10, 25, 2.5], "operation": "new", "name": "Bar"}));
        let before = names(&s);
        assert!(["Body1", "Body2", "Body3", "Body4", "Body5"].iter().all(|n| before.contains(&n.to_string())), "{before:?}");
        run(&mut s, "solid.combine", json!({"target": "Body3", "tools": ["Body5"], "operation": "join", "name": "Joined"}));
        // Renaming a body that already has a name of its own stores nothing elsewhere.
        let stored = s.doc.find_feature("P2").unwrap().body_names.clone();
        run(&mut s, "body.rename", json!({"body": "Second", "name": "Row"}));
        assert_eq!(s.doc.find_feature("P2").unwrap().body_names, stored);
        run(&mut s, "body.rename", json!({"body": "Body1", "name": "FirstCopyA"}));
        run(&mut s, "body.rename", json!({"body": "Body2", "name": "FirstCopyB"}));
        assert!(
            s.model.results.iter().all(|r| r.error.is_none()),
            "{:?}",
            s.model.results.iter().filter_map(|r| r.error.clone()).collect::<Vec<_>>()
        );
        let after = names(&s);
        assert!(after.contains(&"Body3".to_string()) && after.contains(&"Body4".to_string()), "{after:?}");
        assert!(!after.contains(&"Body1".to_string()) && !after.contains(&"Body5".to_string()), "{after:?}");
        run(&mut s, "edit.undo", json!({}));
        assert!(names(&s).contains(&"Body2".to_string()) && names(&s).contains(&"Body4".to_string()), "{:?}", names(&s));
    }

    /// #3044: a rename changes names only, so it, its undo and its redo evaluate nothing: every
    /// body keeps its geometry and display mesh, and the model is the one evaluating the renamed
    /// design gives.
    #[test]
    fn renaming_a_body_evaluates_nothing() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "center": [20, 0, 2.5], "operation": "new", "body_name": "B"}));
        run(&mut s, "appearance.assign", json!({"bodies": ["A"], "appearance": "Aluminum - Satin"}));
        let meshes = |s: &Session| s.model.state().bodies.iter().map(|b| std::sync::Arc::as_ptr(&b.mesh())).collect::<Vec<_>>();
        let shown = meshes(&s);
        s.evaluations = 0;
        run(&mut s, "body.rename", json!({"body": "A", "name": "Plate"}));
        assert_eq!(s.evaluations, 0, "a rename evaluates nothing");
        assert_eq!(names(&s), vec!["Plate".to_string(), "B".to_string()]);
        assert_eq!(meshes(&s), shown, "the bodies keep their meshes");
        assert!(s.doc.appearances.bodies.contains_key("Plate"));
        // The same model as evaluating the renamed design.
        let mut fresh = Session::new((*s.doc).clone());
        fresh.refresh();
        assert_eq!(names(&fresh), names(&s));
        s.refresh();
        assert_eq!(s.model.last_recomputed, 0, "the next evaluation reuses every result");
        s.evaluations = 0;
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(names(&s), vec!["A".to_string(), "B".to_string()]);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(names(&s), vec!["Plate".to_string(), "B".to_string()]);
        assert_eq!(s.evaluations, 0, "undo and redo of a rename evaluate nothing");
        assert_eq!(meshes(&s), shown);
        // A body a later feature uses is renamed in that feature too: that is evaluated.
        run(&mut s, "solid.combine", json!({"target": "Plate", "tools": ["B"], "operation": "join", "keep_tools": true}));
        s.evaluations = 0;
        run(&mut s, "body.rename", json!({"body": "B", "name": "Tool"}));
        assert!(s.evaluations > 0);
        assert!(names(&s).contains(&"Tool".to_string()));
    }

    #[test]
    fn renaming_a_body_leaves_other_text_with_its_name_alone() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        let body = names(&s)[0].clone();
        run(&mut s, "solid.move", json!({"bodies": [body.clone()], "translate": [0, 0, 5]}));
        // Sketch text that happens to spell the body's name is not a reference to it.
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Label"}));
        run(&mut s, "sketch.text", json!({"text": body.clone(), "at": [30, 30], "height": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "body.rename", json!({"body": body.clone(), "name": "Lid"}));
        let kind = |name: &str| serde_json::to_value(&s.doc.find_feature(name).unwrap().kind).unwrap();
        let label = kind("Label").to_string();
        assert!(label.contains(&format!("\"{body}\"")) && !label.contains("\"Lid\""), "{label}");
        let moved = s.doc.features.iter().find_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Move { bodies, .. } => Some(bodies.clone()),
            _ => None,
        });
        assert_eq!(moved, Some(vec![solidcraft_doc::BodyName::from("Lid")]), "the move follows the rename");
        assert!(s.model.results.iter().all(|r| r.error.is_none()));
    }

    #[test]
    fn redefined_sketch_moves_its_extrude() {
        let mut s = Session::default();
        let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 5}));
        let zmax = |s: &mut Session| run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"]["max"][2].as_f64().unwrap();
        assert!((zmax(&mut s) - 5.0).abs() < 1e-6);
        run(&mut s, "sketch.redefine", json!({"sketch": sk, "plane": "XZ"}));
        let z = zmax(&mut s);
        assert!(z.abs() < 1e-6, "positive sketch Y on XZ extends toward -Z: {z}");
        let ymin = run(&mut s, "inspect.measure", json!({}))["bodies"][0]["bbox"]["min"][2].as_f64().unwrap();
        assert!((ymin + 20.0).abs() < 1e-6);
        assert!(s.execute("sketch.redefine", &json!({"sketch": sk, "plane": "nope"})).is_err());
        assert!(s.execute("sketch.redefine", &json!({"sketch": 999, "plane": "XY"})).is_err());
    }

    #[test]
    fn design_units_change_without_moving_geometry() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5}));
        let vol = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
        let v0 = vol(&mut s);
        run(&mut s, "document.units", json!({"units": "in"}));
        assert_eq!(s.doc.units, "in");
        assert!((vol(&mut s) - v0).abs() < 1e-6, "feature inputs keep their millimetres");
        assert!(s.execute("document.units", &json!({"units": "furlong"})).is_err());
        // A unit-less parameter used as a length keeps its millimetres; a plain count stays a number.
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "w", "expression": "5", "unit": ""}));
        run(&mut s, "parameters.add", json!({"name": "n", "expression": "3", "unit": ""}));
        run(&mut s, "solid.box", json!({"length": "w", "width": 10, "height": 5}));
        let b = s.model.state().bodies[0].name.clone();
        run(&mut s, "solid.pattern.rectangular", json!({"bodies": [b], "dir1": [1, 0, 0], "count1": "n", "spacing1": 20}));
        let v0 = vol(&mut s);
        let r = run(&mut s, "document.units", json!({"units": "cm"}));
        assert!((vol(&mut s) - v0).abs() < 1e-6, "{r}");
        assert!(s.doc.param("n").is_some_and(|p| p.expr == "3"), "the count stays bare");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.units, "mm");
    }

    #[test]
    fn a_bare_width_keeps_its_inches_when_the_design_goes_metric() {
        let mut s = Session::default();
        run(&mut s, "document.units", json!({"units": "in"}));
        run(&mut s, "parameters.add", json!({"name": "w", "expression": "2", "unit": ""}));
        run(&mut s, "solid.box", json!({"length": 10, "width": "w", "height": 5}));
        let width = |s: &mut Session| {
            let b = &run(s, "inspect.measure", json!({}))["bodies"][0]["bbox"];
            b["max"][1].as_f64().unwrap() - b["min"][1].as_f64().unwrap()
        };
        assert!((width(&mut s) - 50.8).abs() < 1e-6, "2 in an inch design");
        let r = run(&mut s, "document.units", json!({"units": "mm"}));
        assert!((width(&mut s) - 50.8).abs() < 1e-6, "still 50.8 mm: {r}");
        assert_eq!(r["kept"], json!(["w"]));
        assert_eq!(s.doc.param("w").map(|p| p.expr.as_str()), Some("2 in"));
    }

    #[test]
    fn applying_a_named_view_or_camera_moves_the_view() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10}));
        run(&mut s, "view.save", json!({"name": "Photo", "camera": {"target": [1, 2, 3], "yaw": 0.5, "pitch": -0.6, "distance": 150}}));
        let undo = s.undo.len();
        assert!(s.camera.is_none());
        let r = run(&mut s, "view.apply", json!({"view": "Photo"}));
        let c = s.camera.expect("a camera for the view");
        assert_eq!((c.target, c.yaw, c.pitch, c.distance), (solidcraft_geom::Vec3::new(1.0, 2.0, 3.0), 0.5, -0.6, 150.0));
        assert_eq!(r["camera"]["yaw"], 0.5);
        // A standard view looks from its direction and frames the model.
        run(&mut s, "view.apply", json!({"view": "top"}));
        let c = s.camera.expect("a camera for the top view");
        assert!((c.pitch - std::f64::consts::FRAC_PI_2).abs() < 1e-3 && (c.target.x - 20.0).abs() < 1e-6, "{c:?}");
        let r = run(&mut s, "view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 1, "pitch": 0.2, "distance": 80, "fov": 0.5, "roll": 1.0}}));
        assert_eq!(s.camera.map(|c| (c.yaw, c.distance, c.fov, c.roll)), Some((1.0, 80.0, 0.5, 1.0)));
        assert_eq!(r["camera"]["roll"], 1.0);
        for (roll, expected) in [(f64::MAX, std::f64::consts::PI), (-f64::MAX, -std::f64::consts::PI)] {
            let r = run(&mut s, "view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "roll": roll}}));
            assert_eq!(r["camera"]["roll"], expected);
            assert_eq!(s.camera.map(|c| c.roll), Some(expected));
        }
        assert_eq!(s.undo.len(), undo, "view state, not an undo step");
        for p in [
            json!({}),
            json!({"view": "Nope"}),
            json!({"view": "Photo", "camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 1}}),
            json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0}}),
            json!({"camera": "x"}),
        ] {
            assert!(s.execute("view.apply", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn applying_a_camera_rejects_up() {
        let mut s = Session::default();
        run(&mut s, "view.apply", json!({"view": "front"}));
        let before = serde_json::to_value(s.camera).unwrap();
        let err = s.execute("view.apply", &json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "up": "y"}})).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("up") && message.contains("design") && message.contains("settings"), "{message}");
        assert_eq!(serde_json::to_value(s.camera).unwrap(), before);
    }

    #[test]
    fn applying_a_camera_rejects_invalid_roll() {
        let mut s = Session::default();
        run(&mut s, "view.apply", json!({"view": "front"}));
        let before = serde_json::to_value(s.camera).unwrap();
        // Non-finite floating-point values serialize as null in JSON.
        for roll in [json!(f64::NAN), json!(f64::INFINITY), json!(f64::NEG_INFINITY), json!("NaN"), json!(true)] {
            let err =
                s.execute("view.apply", &json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "roll": roll}})).unwrap_err();
            assert!(err.to_string().contains("roll"), "{err}");
            assert_eq!(serde_json::to_value(s.camera).unwrap(), before);
        }
    }

    #[test]
    fn named_views_are_saved_renamed_and_deleted() {
        let mut s = Session::default();
        let cam = json!({"target": [1, 2, 3], "yaw": 0.5, "pitch": 0.3, "distance": 100});
        assert_eq!(run(&mut s, "view.save", json!({"camera": cam}))["view"], "Named View1");
        run(&mut s, "view.save", json!({"camera": cam, "name": "Detail"}));
        run(&mut s, "view.rename", json!({"view": "Named View1", "name": "Overview"}));
        assert!(s.execute("view.rename", &json!({"view": "Overview", "name": "Detail"})).is_err());
        let back: solidcraft_doc::Document = serde_json::from_str(&serde_json::to_string(&*s.doc).unwrap()).unwrap();
        assert_eq!(back.named_views.len(), 2);
        assert_eq!(back.named_views[0].target.z, 3.0);
        run(&mut s, "view.delete", json!({"view": "Detail"}));
        assert_eq!(run(&mut s, "view.list", json!({}))["views"].as_array().unwrap().len(), 1);
        for p in [json!({}), json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": -1}}), json!({"camera": "x"})] {
            assert!(s.execute("view.save", &p).is_err(), "{p}");
        }
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.named_views.len(), 2);
    }

    #[test]
    fn the_sketch_of_an_edited_feature_is_shown_without_changing_visibility() {
        for (command, params) in [("solid.extrude", json!({"distance": 5})), ("solid.revolve", json!({"axis": "y"}))] {
            let mut s = Session::default();
            let sketch = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 0], "p1": [4, 3]}));
            run(&mut s, "sketch.finish", json!({}));
            // Another sketch between them: the marker of the edit is not on the edited one's sketch.
            run(&mut s, "sketch.create", json!({"plane": "XZ"}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 0], "p1": [4, 3]}));
            run(&mut s, "sketch.finish", json!({}));
            let mut params = params;
            params["sketch"] = json!(sketch);
            run(&mut s, command, params);
            run(&mut s, "timeline.roll_to", json!({"position": 2}));
            for explicitly in [None, Some(true), Some(false)] {
                if let Some(visible) = explicitly {
                    run(&mut s, "browser.visibility", json!({"items": [sketch], "visible": visible}));
                }
                let before = crate::view::sketch_shown(&s, sketch);
                assert_eq!(before, explicitly == Some(true), "{command} {explicitly:?}");
                let visibility = s.visibility.clone();
                s.feature_edit_sketch = Some(sketch);
                assert!(crate::view::sketch_shown(&s, sketch), "{command} {explicitly:?}");
                assert!(crate::view::sketch_shown(&s.scratch(), sketch), "{command} {explicitly:?}");
                s.feature_edit_sketch = None;
                assert_eq!(crate::view::sketch_shown(&s, sketch), before, "{command} {explicitly:?}");
                assert_eq!(s.visibility, visibility);
            }
        }
    }

    #[test]
    fn helper_sketches_hide_and_show() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        let floor = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [50, 0]], "construction": true}));
        run(&mut s, "sketch.finish", json!({}));
        let drain = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.point", json!({"point": [25, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Floor"}));
        let body = s.model.state().bodies[0].name.clone();
        let drain_name = s.doc.feature(drain).unwrap().name.clone();
        let cam = solidcraft_render::Camera::default();
        let lines = |s: &Session| crate::view::scene(s, &cam).unwrap().lines.len();
        let shown = |s: &mut Session| {
            let d = run(s, "document.inspect", json!({}));
            d["sketches"].as_array().unwrap().iter().filter(|x| x["visible"] == true).count()
        };
        let (n0, undo0, doc0) = (lines(&s), s.undo.len(), s.doc.clone());
        assert_eq!(shown(&mut s), 2);
        // By id and by name; the view loses their lines, the design still doesn't change: hiding
        // is an undo step of its own (#2219), and the step carries no document change.
        let r = run(&mut s, "browser.visibility", json!({"items": [floor, drain_name], "visible": false}));
        assert_eq!(r["changed"], 2);
        assert_eq!(shown(&mut s), 0);
        assert!(lines(&s) < n0);
        assert!(std::sync::Arc::ptr_eq(&s.doc, &doc0), "view state, not a design change");
        assert_eq!(s.undo.len(), undo0 + 1, "hiding is one undo step");
        run(&mut s, "browser.visibility", json!({"folder": "sketches", "visible": true}));
        assert_eq!(lines(&s), n0);
        // The browser's Sketches folder hidden: sketches shown one by one aren't drawn either,
        // only the sketch being edited.
        assert!(s.visibility.shown_sketches.contains(&floor));
        run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/sketches"], "visible": false}));
        assert!([floor, drain].iter().all(|id| !crate::view::sketch_shown(&s, *id)));
        s.active_sketch = Some(floor);
        assert!(crate::view::sketch_shown(&s, floor) && !crate::view::sketch_shown(&s, drain));
        s.active_sketch = None;
        run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/sketches"], "visible": true}));
        // Bodies (following a rename) and construction planes.
        run(&mut s, "browser.visibility", json!({"items": [body.clone(), "Floor", "xy"], "visible": false}));
        assert!(crate::view::scene(&s, &cam).unwrap().meshes.is_empty());
        assert_eq!(s.visibility.hidden_origin, vec!["Floor".to_string(), "XY".to_string()]);
        run(&mut s, "body.rename", json!({"body": body, "name": "Tray"}));
        assert_eq!(s.visibility.hidden_bodies, vec!["Tray".to_string()]);
        let d = run(&mut s, "document.inspect", json!({}));
        assert_eq!(d["bodies"][0]["visible"], false);
        // Hostile input is refused and changes nothing.
        let before = s.visibility.clone();
        for p in [
            json!({"items": [floor]}),
            json!({"items": [floor], "visible": "no"}),
            json!({"items": [floor, "Nope"], "visible": false}),
            json!({"items": [], "visible": false}),
            json!({"items": "Floor", "visible": false}),
            json!({"folder": "sketches", "items": [floor], "visible": false}),
            json!({"folder": "nope", "visible": false}),
            json!({"visible": false}),
        ] {
            assert!(s.execute("browser.visibility", &p).is_err(), "{p}");
        }
        assert_eq!(s.visibility, before);
    }

    /// #2219: hiding is a step of its own, so Ctrl+Z brings the body back instead of undoing the
    /// command before the hide, and Ctrl+Y hides it again.
    #[test]
    fn hiding_a_body_is_undone_and_redone() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        s.mark_saved();
        let cam = solidcraft_render::Camera::default();
        let drawn = |s: &Session| crate::view::scene(s, &cam).unwrap().meshes.len();
        assert!(drawn(&s) > 0, "the box is drawn");
        assert!(!s.is_dirty());
        let steps = s.undo.len();

        // Hide: one step, no document change, the view loses the body, the design has changes.
        let doc = s.doc.clone();
        run(&mut s, "browser.visibility", json!({"items": ["Box"], "visible": false}));
        assert_eq!(s.undo.len(), steps + 1, "hiding is one undo step");
        assert!(std::sync::Arc::ptr_eq(&s.doc, &doc), "hiding leaves the document alone");
        assert!(!s.body_visible("Box") && drawn(&s) == 0);
        assert!(s.is_dirty(), "hiding is a change to save");

        // Undo: the hide is what goes back, not the box the hide came after.
        let r = run(&mut s, "edit.undo", json!({}));
        assert_eq!(r["undone"], "Show/Hide", "{r}");
        assert!(s.body_visible("Box") && drawn(&s) > 0);
        assert!(s.model.state().bodies.iter().any(|b| b.name == "Box"), "the box itself stays");
        assert!(!s.is_dirty(), "back to what the file holds");

        // Redo hides it again, and undoing twice more leaves the design as it was.
        run(&mut s, "edit.redo", json!({}));
        assert!(!s.body_visible("Box") && s.is_dirty());
        run(&mut s, "edit.undo", json!({}));
        assert!(s.body_visible("Box") && !s.is_dirty());

        // Hiding a whole folder is one step too, and undoing it brings the body back.
        run(&mut s, "browser.visibility", json!({"folder": "bodies", "visible": false}));
        assert!(!s.body_visible("Box"));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.body_visible("Box") && s.visibility.is_empty() && !s.is_dirty());

        // Hiding or showing what is already so changes nothing: no step, and the lists are not
        // even reordered, so the design is not left with unsaved changes either.
        let steps = s.undo.len();
        run(&mut s, "browser.visibility", json!({"items": ["Box"], "visible": true}));
        assert_eq!(s.undo.len(), steps, "showing a body that is already shown is no change");
        run(&mut s, "browser.visibility", json!({"items": ["Box"], "visible": false}));
        run(&mut s, "browser.visibility", json!({"items": ["Box"], "visible": false}));
        assert_eq!(s.undo.len(), steps + 1, "the first hide is one step, hiding it again is none");
        assert_eq!(s.visibility.hidden_bodies, vec!["Box".to_string()], "no duplicate, no reordering");
        assert!(s.is_dirty(), "the first hide is still a change to save");
        run(&mut s, "browser.visibility", json!({"items": ["Box"], "visible": true}));
        assert_eq!(s.undo.len(), steps + 2, "showing it again is one step");
        assert!(s.visibility.hidden_bodies.is_empty() && !s.is_dirty());

        // `all` shows everything (folders and occurrence hides included) as one step.
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [30, 0, 0], "operation": "new", "body_name": "Second"}));
        run(&mut s, "browser.visibility", json!({"folder": "bodies", "visible": false}));
        run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/bodies"], "visible": false}));
        assert_eq!(s.visibility.hidden_bodies.len(), 2);
        assert_eq!(s.visibility.hidden_folders, vec!["c0/bodies".to_string()]);
        let steps = s.undo.len();
        run(&mut s, "browser.visibility", json!({"folder": "all", "visible": true}));
        assert!(s.visibility.is_empty(), "all shows everything: {:?}", s.visibility);
        assert_eq!(s.undo.len(), steps + 1, "one step for all of it");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.visibility.hidden_bodies.len(), 2, "and one undo puts it back");
        assert_eq!(s.visibility.hidden_folders, vec!["c0/bodies".to_string()]);
    }

    /// A design file can carry any id, the largest a `u64` holds too: a new group then has no id
    /// to take. That is an error, not an overflow (a panic where overflow is checked, a duplicate
    /// id where it is not).
    #[test]
    fn a_new_group_after_the_largest_id_is_an_error_not_an_overflow() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        let a = names(&s)[0].clone();
        run(&mut s, "browser.group", json!({"folder": "bodies", "items": [a.clone()]}));
        s.doc_mut().browser_groups[0].id = u64::MAX;
        let result = s.execute("browser.group", &json!({"folder": "bodies", "items": [a]}));
        assert!(!matches!(result, Err(crate::EngineError::Internal(..))), "panicked inside the command: {result:?}");
        assert!(result.is_err(), "no id is left to give: {result:?}");
        assert_eq!(s.doc.browser_groups.len(), 1, "nothing was added");
    }

    #[test]
    fn exhausted_shared_ids_refuse_a_group_without_changing_the_design() {
        let mut s = Session::default();
        s.doc_mut().next_id = u64::MAX;
        let before = s.doc.to_json();
        let result = s.execute("browser.group", &json!({"folder": "bodies"}));
        assert!(result.is_err() && !matches!(result, Err(crate::EngineError::Internal(..))), "{result:?}");
        assert_eq!(s.doc.to_json(), before);
    }

    #[test]
    fn deleted_group_ids_are_not_reused_after_reopening() {
        let mut s = Session::default();
        let params = json!({"folder": "bodies"});
        let first = run(&mut s, "browser.group", params.clone())["group"].as_u64().unwrap();
        run(&mut s, "browser.ungroup", json!({"group": first}));
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        *s.doc_mut() = doc;
        let second = run(&mut s, "browser.group", params)["group"].as_u64().unwrap();
        assert!(second > first, "deleted group {first} was reused as {second}");
        assert!(s.execute("browser.rename_group", &json!({"group": first, "name": "Stale reference"})).is_err());
    }

    #[test]
    fn the_browsers_view_is_saved_with_the_design() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        let sketch = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [30, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        let body = names(&s)[0].clone();
        s.mark_saved();
        let undo = s.undo.len();
        assert!(crate::view::sketch_shown(&s, sketch));
        // The folders' eyes, a sketch's profiles and dimensions and a lock: view state to save.
        let v = &mut s.visibility;
        v.origin_hidden = Some(true);
        v.hidden_folders.push("c0/sketches".into());
        v.hidden_profiles.push(sketch);
        v.shown_dimensions.push(sketch);
        v.locked_bodies.push(body.clone());
        assert!(s.is_dirty() && s.undo.len() == undo, "a change to save, not an undo step");
        assert!(!crate::view::sketch_shown(&s, sketch), "the hidden Sketches folder hides an unused sketch");
        run(&mut s, "body.rename", json!({"body": body, "name": "Locked"}));
        assert_eq!(s.visibility.locked_bodies, vec!["Locked".to_string()]);
        // Saved and opened again, the design brings all of it back, with nothing to save.
        let text = s.design().to_json();
        let opened = Session::new(solidcraft_doc::Document::from_json(&text).unwrap());
        assert_eq!(opened.visibility, s.visibility);
        assert!(!opened.is_dirty());
        assert!(!crate::view::sketch_shown(&opened, sketch));
        // Showing a whole folder shows the folder itself.
        run(&mut s, "browser.visibility", json!({"folder": "origin", "visible": true}));
        run(&mut s, "browser.visibility", json!({"folder": "sketches", "visible": true}));
        assert_eq!((s.visibility.origin_hidden, s.visibility.hidden_folders.is_empty()), (Some(false), true));
        // A design that never touched the Origin's eye leaves it to the app.
        let plain = solidcraft_doc::Document::from_json(&Session::default().design().to_json()).unwrap();
        assert!(plain.visibility.is_empty() && plain.visibility.origin_hidden.is_none());
    }

    #[test]
    fn groups_hold_items_and_follow_renames() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        let [a, b] = [names(&s)[0].clone(), names(&s)[1].clone()];
        let g = run(&mut s, "browser.group", json!({"folder": "bodies", "items": [a.clone()]}))["group"].as_u64().unwrap();
        assert_eq!(s.doc.browser_groups[0].name, "Group1");
        run(&mut s, "browser.move", json!({"folder": "bodies", "items": [b.clone()], "group": g, "before": a.clone()}));
        assert_eq!(s.doc.browser_groups[0].items, vec![b.clone(), a.clone()]);
        run(&mut s, "browser.rename_group", json!({"group": g, "name": "Fasteners"}));
        run(&mut s, "body.rename", json!({"body": a, "name": "Bolt"}));
        assert_eq!(s.doc.browser_groups[0].items, vec![b.clone(), "Bolt".to_string()]);
        // Out of the group, then a folder order; saved with the design.
        run(&mut s, "browser.move", json!({"folder": "bodies", "items": [b.clone()]}));
        assert_eq!(s.doc.browser_groups[0].items, vec!["Bolt".to_string()]);
        run(&mut s, "browser.order", json!({"folder": "bodies", "order": [b.clone(), "Bolt"]}));
        let saved: solidcraft_doc::Document = serde_json::from_str(&serde_json::to_string(&*s.doc).unwrap()).unwrap();
        assert_eq!(saved.browser_groups, s.doc.browser_groups);
        assert_eq!(saved.browser_order["0/bodies"], vec![b, "Bolt".to_string()]);
        run(&mut s, "browser.ungroup", json!({"group": g}));
        assert!(s.doc.browser_groups.is_empty());
        // Hostile input is refused.
        for (c, p) in [
            ("browser.group", json!({"folder": "nope"})),
            ("browser.group", json!({"folder": "bodies", "component": 99})),
            ("browser.group", json!({"folder": "bodies", "items": "x"})),
            ("browser.move", json!({"folder": "bodies", "items": []})),
            ("browser.move", json!({"folder": "bodies", "items": ["x"], "group": 42})),
            ("browser.ungroup", json!({"group": -1})),
            ("browser.rename_group", json!({"group": g, "name": "x"})),
        ] {
            assert!(s.execute(c, &p).is_err(), "{c} {p}");
        }
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.browser_groups.len(), 1, "ungroup undoes");
    }

    #[test]
    fn hidden_folders_and_groups_keep_their_items_state() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
        let [a, b, c] = [names(&s)[0].clone(), names(&s)[1].clone(), names(&s)[2].clone()];
        let shown = |s: &Session| [&a, &b, &c].map(|n| s.body_visible(n));
        run(&mut s, "browser.visibility", json!({"items": [a.clone()], "visible": false}));

        // Hiding the folder hides every body; their own state is kept.
        run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/bodies"], "visible": false}));
        assert_eq!(shown(&s), [false; 3]);
        assert_eq!(s.visibility.hidden_bodies, vec![a.clone()]);
        // Showing a body in a hidden folder doesn't show the folder, nor draw the body.
        run(&mut s, "browser.visibility", json!({"items": [b.clone()], "visible": true}));
        assert!(s.folder_hidden("c0/bodies") && !s.body_visible(&b));
        // Showing the folder brings back what was shown before.
        run(&mut s, "browser.visibility", json!({"tree_folders": ["c0/bodies"], "visible": true}));
        assert_eq!(shown(&s), [false, true, true]);
        // Hiding every body leaves the folder shown.
        run(&mut s, "browser.visibility", json!({"items": [a.clone(), b.clone(), c.clone()], "visible": false}));
        assert!(!s.folder_hidden("c0/bodies"), "hiding every item doesn't hide the folder");
        run(&mut s, "browser.visibility", json!({"items": [b.clone(), c.clone()], "visible": true}));

        // A group the same way.
        let g = run(&mut s, "browser.group", json!({"folder": "bodies", "items": [a.clone(), b.clone()]}))["group"].as_u64().unwrap();
        let key = format!("c0/g{g}");
        run(&mut s, "browser.visibility", json!({"tree_folders": [key], "visible": false}));
        assert_eq!(shown(&s), [false, false, true]);
        run(&mut s, "browser.visibility", json!({"tree_folders": [key], "visible": true}));
        assert_eq!(shown(&s), [false, true, true]);
        run(&mut s, "browser.visibility", json!({"items": [a.clone(), b.clone()], "visible": false}));
        assert!(!s.folder_hidden(&key), "hiding every item doesn't hide the group");

        // Saved with the design.
        run(&mut s, "browser.visibility", json!({"tree_folders": [key, "o:4/sketches"], "visible": false}));
        let v: solidcraft_doc::Visibility = serde_json::from_value(serde_json::to_value(&s.visibility).unwrap()).unwrap();
        assert_eq!(v.hidden_folders, vec![key.clone(), "o:4/sketches".to_string()]);
        // Showing every body also shows the folders and groups that hold bodies.
        run(&mut s, "browser.visibility", json!({"folder": "bodies", "visible": true}));
        assert_eq!(shown(&s), [true; 3]);
        assert_eq!(s.visibility.hidden_folders, vec!["o:4/sketches".to_string()]);

        // Hostile keys are refused, and nothing changes.
        for p in [
            json!({"tree_folders": [], "visible": false}),
            json!({"tree_folders": "c0/bodies", "visible": false}),
            json!({"tree_folders": ["bodies"], "visible": false}),
            json!({"tree_folders": ["c1/bodies"], "visible": false}),
            json!({"tree_folders": ["o:/bodies"], "visible": false}),
            json!({"tree_folders": ["o:x/bodies"], "visible": false}),
            json!({"tree_folders": ["c0/gx"], "visible": false}),
            json!({"tree_folders": ["c0/nope"], "visible": false}),
            json!({"tree_folders": [7], "visible": false}),
            json!({"tree_folders": ["c0/bodies"], "items": [c.clone()], "visible": false}),
            json!({"tree_folders": [format!("o:{}1/bodies", "1/".repeat(70))], "visible": false}),
        ] {
            assert!(s.execute("browser.visibility", &p).is_err(), "{p}");
        }
        assert_eq!(s.visibility.hidden_folders, vec!["o:4/sketches".to_string()]);
    }

    fn strs(v: &Value, k: &str) -> Vec<String> {
        v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }

    #[test]
    fn expand_and_collapse_all_return_tree_keys_and_are_not_undoable() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        let g = run(&mut s, "browser.group", json!({"folder": "bodies"}))["group"].as_u64().unwrap();
        let created = run(&mut s, "component.create", json!({"name": "Part"}));
        let occ = created["occurrence"].as_u64().unwrap();
        let n = s.undo.len();

        let expanded = run(&mut s, "browser.expand_all", json!({}));
        assert_eq!(expanded["open"], true);
        let open = strs(&expanded, "open_by_default");
        let closed = strs(&expanded, "closed_by_default");
        assert!(open.contains(&"c0".into()), "{open:?}");
        assert!(open.contains(&"c0/bodies".into()), "{open:?}");
        assert!(open.contains(&format!("c0/g{g}")), "{open:?}");
        assert!(open.contains(&"c0/sketches".into()), "{open:?}");
        assert!(open.contains(&format!("o:{occ}")), "{open:?}");
        assert!(closed.contains(&"origin".into()) && closed.contains(&"namedviews".into()) && closed.contains(&"docsettings".into()), "{closed:?}");
        assert!(closed.contains(&"c0/features".into()), "{closed:?}");
        assert_eq!(s.undo.len(), n, "display-only: no undo step");

        let collapsed = run(&mut s, "browser.collapse_all", json!({}));
        assert_eq!(collapsed["open"], false);
        let open = strs(&collapsed, "open_by_default");
        assert!(!open.contains(&"c0".into()), "the design row stays visible: {open:?}");
        assert!(open.contains(&"c0/bodies".into()) && open.contains(&format!("o:{occ}")), "{open:?}");

        let below = run(&mut s, "browser.collapse_all", json!({"path": [occ]}));
        let open = strs(&below, "open_by_default");
        assert!(open.contains(&format!("o:{occ}")), "{open:?}");
        assert!(!open.contains(&"c0/bodies".into()), "{open:?}");
        assert!(strs(&below, "closed_by_default").iter().all(|k| !k.starts_with("c0") && k != "origin"), "{below}");

        let only = run(&mut s, "browser.expand_all", json!({"component": created["component"]}));
        let open = strs(&only, "open_by_default");
        assert!(open.iter().any(|k| k.starts_with('c') && k != "c0" && !k.starts_with("c0/")), "{open:?}");
        assert!(!open.contains(&format!("o:{occ}")), "{open:?}");
    }

    #[test]
    fn expand_and_collapse_all_refuse_hostile_scope() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        let created = run(&mut s, "component.create", json!({"name": "Part"}));
        let occ = created["occurrence"].as_u64().unwrap();
        for (cmd, p) in [
            ("browser.expand_all", json!({"path": []})),
            ("browser.expand_all", json!({"path": "o:1"})),
            ("browser.expand_all", json!({"path": [999]})),
            ("browser.expand_all", json!({"component": "root"})),
            ("browser.collapse_all", json!({"component": 99})),
            ("browser.collapse_all", json!({"path": [occ], "component": 0})),
            (
                "browser.expand_all",
                json!({"path": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65]}),
            ),
        ] {
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        }
        assert!(s.execute("browser.expand_all", &json!({})).is_ok());
    }

    /// #5099: a feature's bodies show and hide by the feature, in every placement or one.
    #[test]
    fn a_feature_shows_and_hides_the_bodies_it_made() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "solid.sphere", json!({"diameter": 4, "center": [30, 0, 0]}));
        // A join makes no body of its own.
        run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 20, "operation": "join"}));
        let hide = |s: &mut Session, p: Value| run(s, "browser.visibility", p)["visibility"]["hidden_bodies"].clone();
        assert_eq!(hide(&mut s, json!({"visible": false, "features": ["Sphere1"]})), json!(["Body2"]));
        assert_eq!(hide(&mut s, json!({"visible": true, "features": ["Sphere1"]})), json!([]));
        for (p, why) in [
            (json!({"visible": false, "features": ["Box2"]}), "made no bodies"),
            (json!({"visible": false, "features": ["Nope"]}), "no feature"),
            (json!({"visible": false, "features": ["Box1"], "items": ["Body2"]}), "not several"),
        ] {
            let e = s.execute("browser.visibility", &p).expect_err("refused").to_string();
            assert!(e.contains(why), "{p}: {e}");
        }

        // A component placed twice: every instance's body, or one placement's.
        let first = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}))["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 40, 0]}));
        let second = run(&mut s, "occurrence.copy", json!({"occurrence": first}))["occurrence"].as_u64().unwrap();
        let all = hide(&mut s, json!({"visible": false, "features": ["Box3"]}));
        assert_eq!(all.as_array().unwrap().len(), 2, "{all}");
        hide(&mut s, json!({"visible": true, "features": ["Box3"]}));
        let one = hide(&mut s, json!({"visible": false, "features": ["Box3"], "path": [second]}));
        assert_eq!(one.as_array().unwrap().len(), 1, "{one}");
        assert!(all.as_array().unwrap().contains(&one[0]) && one[0] != all[0], "the copy's body: {one} of {all}");
        let e = s.execute("browser.visibility", &json!({"visible": false, "features": ["Sphere1"], "path": [first]})).expect_err("other component");
        assert!(e.to_string().contains("component"), "{e}");
    }
}
