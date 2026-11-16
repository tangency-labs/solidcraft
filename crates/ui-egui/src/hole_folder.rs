//! Editing every hole in a History folder at once. A folder that holds only holes (and the folders
//! inside it) opens the Hole dialog filled in from its first hole, with nothing to pick: what OK
//! changes there (size, depth, type, thread, …) goes to each hole, and the rest of each hole (its
//! place first of all) stays its own. Each hole is redefined with `timeline.redefine`, as editing
//! it on its own does.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::dialogs::Dialog;

/// Where the folder dialog keeps the folder and its holes, and the parameters it opened with.
const FOLDER: &str = "hole_folder";
const OPENED: &str = "hole_folder_opened";

/// The holes of folder `id`, in History order, when it (with the folders inside it) holds holes
/// only and no joints.
pub(crate) fn holes(app: &SolveApp, id: u64) -> Option<Vec<u64>> {
    let doc = &app.session.doc;
    let mut pending = vec![id];
    let mut seen = std::collections::BTreeSet::new();
    let mut ids = Vec::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let group = doc.timeline_groups.iter().find(|group| group.id == id)?;
        if !group.joints.is_empty() {
            return None;
        }
        for feature in &group.features {
            if !matches!(doc.feature(*feature)?.kind, solidcraft_engine::doc::FeatureKind::Hole { .. }) {
                return None;
            }
            ids.push(*feature);
        }
        pending.extend(doc.timeline_groups.iter().filter(|child| child.parent == Some(id)).map(|child| child.id));
    }
    ids.sort_by_key(|feature| doc.feature_index(*feature));
    (!ids.is_empty()).then_some(ids)
}

/// Whether `d` edits a folder of holes.
pub(crate) fn active(d: &Dialog) -> bool {
    d.extra.contains_key(FOLDER)
}

/// How many holes the open folder dialog edits, and the folder's name.
pub(crate) fn summary(app: &SolveApp, d: &Dialog) -> Option<(usize, String)> {
    let folder = d.extra.get(FOLDER)?;
    let id = folder.get("group").and_then(Value::as_u64)?;
    let count = folder.get("holes").and_then(Value::as_array).map_or(0, Vec::len);
    let name = app.session.doc.timeline_groups.iter().find(|group| group.id == id).map(|group| group.name.clone())?;
    Some((count, name))
}

/// Open the Hole dialog for every hole in folder `id`.
pub(crate) fn open(app: &mut SolveApp, id: u64) -> Result<(), String> {
    let ids = holes(app, id).ok_or("the folder holds something other than holes")?;
    let first = *ids.first().ok_or("the folder holds no holes")?;
    crate::dialogs::cancel(app);
    let mut d = crate::dialogs::for_feature(app, first, None).ok_or("the hole can't be edited here")?;
    d.editing = None;
    d.inputs.clear();
    d.active = 0;
    for key in ["direction", "position_exprs", "all_points"] {
        d.extra.remove(key);
    }
    d.extra.insert(FOLDER.into(), json!({"group": id, "holes": ids}));
    let opened = hole_params(&crate::dialogs::apply_commands(app, &d)?)?;
    d.extra.insert(OPENED.into(), opened);
    app.dialog = Some(d);
    Ok(())
}

/// The parameters of the one `solid.hole` command among `cmds`.
fn hole_params(cmds: &[(String, Value)]) -> Result<Value, String> {
    match cmds {
        [(cmd, params)] if cmd == "solid.hole" => Ok(params.clone()),
        _ => Err("the Hole dialog made no hole".into()),
    }
}

/// The folder dialog's commands: `edited` is the one hole its fields make (with no place). Each
/// hole of the folder is redefined from its own parameters with the fields that differ from those
/// the dialog opened with; none at all before the dialog has opened.
pub(crate) fn commands(app: &SolveApp, d: &Dialog, edited: Vec<(String, Value)>) -> Result<Vec<(String, Value)>, String> {
    let Some(opened) = d.extra.get(OPENED) else { return Ok(edited) };
    let edited = hole_params(&edited)?;
    let (Some(opened), Some(edited)) = (opened.as_object(), edited.as_object()) else { return Err("the hole's fields are not a map".into()) };
    let changed: Vec<(&String, Option<&Value>)> =
        opened.keys().chain(edited.keys()).filter(|key| opened.get(*key) != edited.get(*key)).map(|key| (key, edited.get(key))).collect();
    let ids: Vec<u64> = d
        .extra
        .get(FOLDER)
        .and_then(|folder| folder.get("holes"))
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for id in ids {
        let one = crate::dialogs::for_feature(app, id, None).ok_or("a hole in the folder can't be edited")?;
        let Some((cmd, mut redefine)) = crate::dialogs::apply_commands(app, &one)?.into_iter().next() else { continue };
        let Some(params) = redefine.get_mut("params").and_then(Value::as_object_mut) else { continue };
        // A hole at a point keeps the point it was made at: its edit dialog finds the face there
        // in the model before the hole (History rolled back to it), which this one is not.
        if let Some(solidcraft_engine::doc::FeatureKind::Hole { points: None, position, at, .. }) = app.session.doc.feature(id).map(|f| &f.kind) {
            let place = match at {
                Some(exprs) => json!(exprs.iter().map(|e| e.as_str()).collect::<Vec<_>>()),
                None => json!([position.0.x, position.0.y, position.0.z]),
            };
            params.insert("position".into(), place);
        }
        for (key, value) in &changed {
            match value {
                Some(value) => {
                    params.insert((*key).clone(), (*value).clone());
                }
                None => {
                    params.remove(*key);
                }
            }
        }
        out.push((cmd, redefine));
    }
    Ok(out)
}

/// The line the folder dialog shows over its fields.
pub(crate) fn note(app: &SolveApp, d: &Dialog, ui: &mut egui::Ui) {
    if let Some((count, name)) = summary(app, d) {
        let text = solidcraft_ui_text::format("hole_folder.applies_to", &[("count", &count.to_string()), ("folder", &name)]);
        ui.add(egui::Label::new(egui::RichText::new(text).color(solidcraft_theme::Tokens::get().text_dim)).wrap());
    }
}
