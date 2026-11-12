//! Delete (the Delete and Backspace keys, and every menu's Delete item): whatever is selected in
//! the viewport or the browser goes in one undoable step through `selection.delete`. When it
//! takes other features with it or makes some fail, a confirmation lists them first.

use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::doc::KeptResults;

use crate::SolveApp;
use crate::ui_kit;

/// Longest list of names the confirmation shows before "and n more".
const MAX_LISTED: usize = 20;

/// Whether this item is a face a fillet or chamfer made.
///
/// Deleting one takes that blend off -- see `selection.delete` -- so the feature that goes is
/// the thing that was asked for, not a surprise to be confirmed first. Pressing Delete on a
/// round is how a round is taken off, and a card in the way of it every time would make the
/// quickest thing in the modeller the slowest. What went is said afterwards, and Ctrl+Z is
/// there, which is the right trade for an action this direct.
fn is_blend_face(app: &SolveApp, item: &Value) -> bool {
    if item["type"] != "face" {
        return false;
    }
    let (Some(body), Some(index)) = (item["body"].as_str(), item["index"].as_u64()) else {
        return false;
    };
    crate::dialogs::blend_feature(&app.session, body, index as usize).is_some()
}

/// A delete waiting for confirmation: its parameters and the dry run's report.
#[derive(Clone, Debug)]
pub struct Pending {
    pub params: Value,
    pub report: Value,
    /// What the dry run computed ahead, for the delete to find (#9917).
    pub kept: Option<Arc<KeptResults>>,
}

/// What Delete acts on now: the selection (bodies not locked), plus the components picked in
/// the browser (their occurrences).
pub fn selection_params(app: &SolveApp) -> Value {
    use solidcraft_engine::Sel;
    let mut items: Vec<Sel> = Vec::new();
    for s in app.session.selection.iter().filter(|s| !matches!(s, Sel::Body { name } if app.session.visibility.locked_bodies.contains(name))) {
        // A MidPoint glyph shared by lines whose midpoints meet removes every constraint it
        // shows, and the point they shared when nothing else holds it.
        let group = match s {
            Sel::SketchConstraint { id } => crate::sketch_tools::active_glyph_deletion(app, id),
            _ => vec![s.clone()],
        };
        for g in group {
            if !items.contains(&g) {
                items.push(g);
            }
        }
    }
    let occurrences: Vec<u64> = app.tree.picked_components.iter().filter_map(|c| app.session.doc.occurrence_of(*c).map(|o| o.id)).collect();
    json!({ "items": items, "occurrences": occurrences, "canvases": app.tree.picked_canvases, "joints": app.tree.picked_joints })
}

fn is_empty(p: &Value) -> bool {
    ["items", "occurrences", "components", "canvases", "joints"].iter().all(|k| p.get(*k).and_then(Value::as_array).is_none_or(|a| a.is_empty()))
}

fn shared_occurrences(app: &SolveApp, params: &Value) -> bool {
    let explicit = params.get("occurrences").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64);
    let selected = params
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "occurrence")
        .filter_map(|item| item["path"].as_array()?.last()?.as_u64());
    explicit.chain(selected).any(|id| app.session.occurrence_id_scope_count(id).is_ok_and(|n| n > 1))
}

/// Delete what is selected (the Delete key). A selected sketch dimension or constraint glyph
/// goes first, as before.
pub fn delete_selection(app: &mut SolveApp) {
    if crate::sketch3d::delete_selected(app) || crate::sketch_tools::delete_glyph(app) {
        return;
    }
    let mut params = selection_params(app);
    // In Edit Joint with nothing else selected, Delete means the joint being edited (#3749).
    if is_empty(&params)
        && let Some(id) = edited_joint(app)
    {
        params["joints"] = json!([id]);
    }
    request(app, params);
}

/// Whether Delete has nothing to act on: nothing selected, and no joint open in Edit Joint.
pub fn nothing_to_delete(app: &SolveApp) -> bool {
    is_empty(&selection_params(app)) && edited_joint(app).is_none()
}

/// The joint the open Edit Joint dialog edits.
fn edited_joint(app: &SolveApp) -> Option<u64> {
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(crate::dialogs::Kind::Assembly(crate::dialogs_assembly::Asm::Joint(f))) => f.editing,
        _ => None,
    }
}

/// Why a joint cannot be deleted now (the history it changes is busy), if it cannot.
fn joint_delete_blocked(app: &SolveApp) -> Option<&'static str> {
    if app.dialog.is_some() {
        Some("Close the open dialog before deleting a joint")
    } else if app.tool.is_some() {
        Some("Finish the active tool before deleting a joint")
    } else if app.computing_since().is_some() {
        Some("Wait for the design to finish computing before deleting a joint")
    } else if app.file_load.busy() {
        Some("Wait for the file to finish loading before deleting a joint")
    } else {
        None
    }
}

/// Delete `params` (as `selection.delete` takes them): at once, or after a confirmation when
/// other features go with it or would fail. The dry run that finds out evaluates the history
/// after what is deleted, so it may compute off the window thread (#2678).
pub fn request(app: &mut SolveApp, params: Value) {
    let joints: Vec<u64> = params.get("joints").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
    if !joints.is_empty() {
        // Deleting just the joint Edit Joint edits closes the dialog first, as its Cancel does.
        let only_edited = edited_joint(app).is_some_and(|id| joints == [id])
            && ["items", "occurrences", "components", "canvases"]
                .iter()
                .all(|k| params.get(*k).and_then(Value::as_array).is_none_or(|a| a.is_empty()));
        if only_edited && app.applying.is_none() && app.tool.is_none() && app.computing_since().is_none() && !app.file_load.busy() {
            crate::dialogs::cancel(app);
        }
        if let Some(why) = joint_delete_blocked(app) {
            app.set_status(why, true);
            return;
        }
    }
    if is_empty(&params) {
        return;
    }
    let mut dry = params.clone();
    dry["dry_run"] = json!(true);
    app.check_delete(vec![("selection.delete".to_string(), dry)], crate::deferred::AfterCheck::Selection(params));
}

/// [`request`] once its dry run's report is in.
pub(crate) fn checked(app: &mut SolveApp, params: Value, report: Result<Value, String>, kept: Option<Arc<KeptResults>>) {
    let report = match report {
        Ok(r) => r,
        Err(e) => {
            app.set_status(e.replace("selection.delete: ", ""), true);
            return;
        }
    };
    let asked = params
        .get("items")
        .and_then(Value::as_array)
        .map_or(0, |a| a.iter().filter(|i| i["type"] == "feature" || i["type"] == "plane" || is_blend_face(app, i)).count());
    let extra = report["deleted"].as_array().map_or(0, Vec::len) > asked;
    let fails = report["would_fail"].as_array().is_some_and(|a| !a.is_empty());
    let shared = shared_occurrences(app, &params);
    if extra || fails || shared {
        app.menu.confirm = Some(Pending { params, report, kept });
    } else {
        run(app, &params, kept);
    }
}

/// Delete for real. With the history rolled back it builds the history after the marker to see
/// what the delete breaks: the dry run built it ahead, off the window thread, and `kept` it.
fn run(app: &mut SolveApp, params: &Value, kept: Option<Arc<KeptResults>>) {
    match app.run_with_kept(kept, |app| app.run("selection.delete", params.clone())) {
        Ok(r) => {
            app.tree.picked_components.clear();
            app.tree.picked_canvases.clear();
            app.tree.picked_joints.clear();
            if let Some(s) = r["skipped"].as_array().and_then(|a| a.first()).and_then(Value::as_str) {
                app.set_status(format!("deleted; {s}"), false);
            } else if let Some(names) = took_features(params, &r) {
                // A face that took a blend off: name it, because what went is not the face that
                // was picked but the feature behind it, and one fillet may have rounded several
                // edges that all come back sharp together.
                app.set_status(format!("deleted {names}"), false);
            }
        }
        Err(e) => app.set_status(e.replace("selection.delete: ", ""), true),
    }
}

/// The features a delete of faces alone took with it, named, where it took any.
fn took_features(params: &Value, report: &Value) -> Option<String> {
    let items = params.get("items").and_then(Value::as_array)?;
    if items.is_empty() || !items.iter().all(|i| i["type"] == "face") {
        return None;
    }
    let names: Vec<&str> = report["deleted"].as_array()?.iter().filter_map(Value::as_str).collect();
    (!names.is_empty()).then(|| names.join(", "))
}

/// Answer the confirmation (also `ui.confirm`): delete, or keep everything.
pub fn confirm(app: &mut SolveApp, accept: bool) -> bool {
    let Some(p) = app.menu.confirm.take() else { return false };
    if accept {
        run(app, &p.params, p.kept);
    }
    true
}

/// The confirmation card.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(p) = app.menu.confirm.clone() else { return };
    let list = |k: &str| -> Vec<String> {
        p.report[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
    };
    let (deleted, fails) = (list("deleted"), list("would_fail"));
    let shared = shared_occurrences(app, &p.params);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_delete_confirm"), "delete", crate::i18n::tr_label("Delete"))
        .tone(ui_kit::Tone::Danger)
        .actions("Delete", "Cancel", None)
        .show(ctx, true, |ui| {
            if shared {
                ui_kit::text(
                    ui,
                    crate::i18n::tr_label("This child is shared. Deleting it removes it from every copy of its parent."),
                    ui_kit::Text::Report("shared", false),
                );
            }
            for (key, heading, names, error) in
                [("deleted", "These features will be deleted:", &deleted, false), ("fails", "These features will fail:", &fails, true)]
            {
                if names.is_empty() {
                    continue;
                }
                ui_kit::text(ui, crate::i18n::tr_label(heading), ui_kit::Text::Report(key, error));
                for name in names.iter().take(MAX_LISTED) {
                    ui_kit::text(ui, &format!("  • {name}"), ui_kit::Text::Report(&format!("{key}:{name}"), error));
                }
                if names.len() > MAX_LISTED {
                    let more = (names.len() - MAX_LISTED).to_string();
                    ui_kit::text(
                        ui,
                        &crate::i18n::format("delete.and_more", &[("count", &more)]),
                        ui_kit::Text::Report(&format!("{key}:more"), error),
                    );
                }
            }
        });
    match action {
        ui_kit::Action::Ok => drop(confirm(app, true)),
        ui_kit::Action::Cancel => drop(confirm(app, false)),
        _ if closed => drop(confirm(app, false)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

    /// Delete on a fillet's own face takes the fillet off at once, with no card in the way, and
    /// says which feature went -- because what goes is not the face that was picked.
    #[test]
    fn delete_on_a_round_takes_the_round_off_without_asking() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 40, "body_name": "B"})).unwrap();
        h.app.run("solid.fillet", json!({"edges": [[40, 20, 40]], "radius": 6})).unwrap();
        let fillet = h.app.session.doc.features.iter().find(|f| f.name.starts_with("Fillet")).expect("a fillet").name.clone();

        // The face the round made, found by its name rather than by counting.
        let body = h.app.session.model.state().body("B").cloned().expect("the body");
        let names = solidcraft_engine::doc::naming::face_names(&body);
        let index = names.iter().position(|n| solidcraft_engine::doc::persistent_name::is_blend(n)).expect("a blend face");
        // A point on it: the round's surface, level with the edge it replaced.
        let d = 40.0 - 6.0 + 6.0 / 2.0f64.sqrt();
        h.app.run("select.set", json!({"items": [{"type": "face", "body": "B", "index": index, "point": [d, 20.0, d]}]})).unwrap();

        crate::delete::delete_selection(&mut h.app);
        assert!(h.app.menu.confirm.is_none(), "a round should go without a confirmation card");
        assert!(h.app.session.doc.find_feature(&fillet).is_none(), "the fillet should be gone from the history");
        assert_eq!(h.app.session.doc.features.len(), 1, "only the box should be left");

        // And it is one undo step back to the rounded part.
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(h.app.session.doc.find_feature(&fillet).is_some(), "undo should bring the fillet back");
    }
}
