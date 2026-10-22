//! Revisions window: the open design's in-file saves.
//!
//! File view, the File menu, and `ui.start` of `doc.revisions` open it. `engine.execute` of
//! `doc.revisions` still returns the JSON list and does not open this window. This is not the
//! feature timeline. Restore and Open read-only call `doc.restore`. A snapshot is the parametric
//! model at that save. The newest 16 revisions can keep one.

use serde_json::{Value, json};
use solidcraft_engine::doc::document_history::{DocumentHistory, MAX_SNAPSHOTS};

use crate::SolveApp;
use crate::i18n;
use crate::theme::Tokens;

/// Open window state. Absent means the window is closed.
pub struct RevisionsPanel {
    selected: Option<u64>,
    error: Option<String>,
}

/// One revision row. Snapshots stay in the document; this only keeps what the row draws.
struct Row {
    revision: u64,
    format_version: u32,
    action: String,
    has_snapshot: bool,
    /// 0 is the newest entry.
    rank: usize,
    feature_count: Option<usize>,
    /// Feature names, filled for the selected row only.
    features: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapshotState {
    Restorable,
    /// A save whose model was pruned, or never stored.
    Dropped,
    /// An `opened` entry from a file that predates snapshots.
    OlderFile,
}

/// Open the window. A second open leaves the current selection in place.
pub fn open(app: &mut SolveApp) {
    if app.revisions.is_none() {
        app.revisions = Some(RevisionsPanel { selected: None, error: None });
    }
}

/// Draw the window when it is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.revisions.is_none() {
        return;
    }
    let selected = app.revisions.as_ref().and_then(|panel| panel.selected);
    let history = &app.session.doc.document_history;
    let rows = rows_of(history, selected);
    let save_count = history.revision;
    let format_version = history.format_version;
    let readonly = app.session.revision_view;
    let Some(mut state) = app.revisions.take() else { return };
    if state.selected.is_some_and(|revision| !rows.iter().any(|row| row.revision == revision)) {
        state.selected = None;
    }

    let mut action: Option<&'static str> = None;
    let title = app.tr("ui.revisions.title");
    // One and a half cards, the kit window default. Another command closes this window, so it
    // is not on screen beside a command dialog.
    let closed = crate::ui_kit::Window::new(egui::Id::new("sc_revisions"), "restore", title).cards(1.5).show_body(ctx, |ui| {
        draw_header(ui, save_count, format_version, &rows, readonly);
        ui.add_space(4.0);
        if rows.is_empty() {
            ui.label(i18n::tr("ui.revisions.empty"));
        } else {
            draw_rows(ui, &mut state, &rows);
        }
        if let Some(error) = &state.error {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(error).color(Tokens::get().error));
        }
        ui.separator();
        let can_restore = rows.iter().any(|row| state.selected == Some(row.revision) && row.has_snapshot);
        let restore_tip = if state.selected.is_none() {
            i18n::tr("ui.revisions.select_first")
        } else if can_restore {
            i18n::tr("ui.revisions.restore_tip")
        } else {
            i18n::tr("ui.revisions.no_model")
        };
        let readonly_tip = if can_restore { i18n::tr("ui.revisions.readonly_tip") } else { restore_tip };
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let restore = crate::ui_kit::button(ui, i18n::tr("ui.revisions.restore"), crate::ui_kit::Button::Enabled(can_restore))
                    .on_hover_text(restore_tip);
                crate::scenario::publish_handle("revisions:restore", restore.rect.center());
                if restore.clicked() {
                    action = Some("working");
                }
                let readonly = crate::ui_kit::button(ui, i18n::tr("ui.revisions.open_readonly"), crate::ui_kit::Button::Enabled(can_restore))
                    .on_hover_text(readonly_tip);
                crate::scenario::publish_handle("revisions:readonly", readonly.rect.center());
                if readonly.clicked() {
                    action = Some("readonly");
                }
            });
        });
    });

    if let Some(mode) = action
        && let Some(revision) = state.selected
    {
        match restore_revision(app, revision, mode) {
            Ok(()) => state.error = None,
            Err(error) => {
                app.set_status(error.clone(), true);
                state.error = Some(error);
            }
        }
    }
    if !closed {
        app.revisions = Some(state);
    }
}

fn draw_header(ui: &mut egui::Ui, save_count: u64, format_version: u32, rows: &[Row], readonly: Option<u64>) {
    let t = Tokens::get();
    let count = save_count.to_string();
    ui.label(i18n::format("ui.revisions.save_count", &[("count", &count)]));
    let version = format_version.to_string();
    ui.label(egui::RichText::new(i18n::format("ui.revisions.format", &[("version", &version)])).small().color(t.text_dim));
    let kept = MAX_SNAPSHOTS.to_string();
    ui.label(egui::RichText::new(i18n::format("ui.revisions.cap", &[("count", &kept)])).small().color(t.text_dim));
    if !rows.is_empty() {
        let considered = rows.iter().filter(|row| row.rank < MAX_SNAPSHOTS).count();
        let restorable = rows.iter().filter(|row| row.rank < MAX_SNAPSHOTS && row.has_snapshot).count();
        let kept = restorable.to_string();
        let considered = considered.to_string();
        ui.label(i18n::format("ui.revisions.restorable", &[("kept", &kept), ("considered", &considered)]));
        ui.label(egui::RichText::new(i18n::tr("ui.revisions.oldest")).small().color(t.text_dim));
    }
    if let Some(revision) = readonly {
        let revision = revision.to_string();
        ui.label(i18n::format("ui.revisions.readonly", &[("revision", &revision)]));
    }
}

fn draw_rows(ui: &mut egui::Ui, state: &mut RevisionsPanel, rows: &[Row]) {
    let t = Tokens::get();
    for row in rows {
        let selected = state.selected == Some(row.revision);
        let state_kind = snapshot_state(&row.action, row.has_snapshot);
        let status = status_text(state_kind, row.rank);
        let revision = row.revision.to_string();
        let background = ui.painter().add(egui::Shape::Noop);
        let inner = ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    let label = i18n::format("ui.revisions.row", &[("revision", &revision)]);
                    ui.label(egui::RichText::new(label).strong());
                    let version = row.format_version.to_string();
                    let action = action_label(&row.action);
                    ui.label(
                        egui::RichText::new(i18n::format(
                            "ui.revisions.meta",
                            &[("revision", &revision), ("action", &action), ("version", &version)],
                        ))
                        .small()
                        .color(t.text_dim),
                    );
                    ui.label(egui::RichText::new(status).small().color(status_color(state_kind, t)));
                    if selected && let Some(total) = row.feature_count {
                        ui.label(feature_line(total));
                        for name in &row.features {
                            ui.label(egui::RichText::new(name).small());
                        }
                        if row.features.len() < total {
                            let more = (total - row.features.len()).to_string();
                            ui.label(egui::RichText::new(i18n::format("ui.revisions.features.more", &[("count", &more)])).small().color(t.text_dim));
                        }
                    }
                });
                ui.add_space(6.0);
            });
            ui.add_space(6.0);
        });
        let rect = inner.response.rect;
        let fill = if selected { t.hover } else { egui::Color32::TRANSPARENT };
        ui.painter().set(background, egui::epaint::RectShape::filled(rect, 4u8, fill));
        let response = ui.interact(rect, ui.id().with(("revision-row", row.revision)), egui::Sense::click());
        crate::scenario::publish_handle(&format!("revisions:{}", row.revision), response.rect.center());
        if response.clicked() {
            state.selected = Some(row.revision);
            state.error = None;
        }
        ui.add_space(2.0);
    }
}

fn restore_revision(app: &mut SolveApp, revision: u64, mode: &str) -> Result<(), String> {
    app.run("doc.restore", json!({"revision": revision, "mode": mode}))?;
    app.dialog = None;
    app.tool = None;
    app.pre_sketch_marker = None;
    app.params = None;
    app.menu.props = None;
    crate::dim_view::cancel();
    let save_count = app.session.doc.document_history.revision;
    let revision = revision.to_string();
    let count = save_count.to_string();
    let message = if mode == "readonly" {
        i18n::format("ui.revisions.opened_readonly", &[("revision", &revision)])
    } else {
        i18n::format("ui.revisions.restored", &[("revision", &revision), ("count", &count)])
    };
    app.set_status(message, false);
    Ok(())
}

fn rows_of(history: &DocumentHistory, selected: Option<u64>) -> Vec<Row> {
    let count = history.entries.len();
    let mut rows = Vec::with_capacity(count);
    for (index, entry) in history.entries.iter().enumerate() {
        let chosen = selected == Some(entry.revision);
        rows.push(Row {
            revision: entry.revision,
            format_version: entry.format_version,
            action: entry.action.clone(),
            has_snapshot: entry.snapshot.is_some(),
            rank: count.saturating_sub(index).saturating_sub(1),
            feature_count: feature_count(entry.snapshot.as_ref()),
            features: if chosen { feature_names(entry.snapshot.as_ref()) } else { Vec::new() },
        });
    }
    rows
}

fn feature_count(snapshot: Option<&Value>) -> Option<usize> {
    snapshot.and_then(|value| value.get("features")).and_then(Value::as_array).map(Vec::len)
}

fn feature_names(snapshot: Option<&Value>) -> Vec<String> {
    let Some(features) = snapshot.and_then(|value| value.get("features")).and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for feature in features.iter().take(12) {
        let Some(name) = feature.get("name").and_then(Value::as_str) else { continue };
        if !name.is_empty() {
            names.push(name.to_string());
        }
    }
    names
}

fn snapshot_state(action: &str, has_snapshot: bool) -> SnapshotState {
    if has_snapshot {
        SnapshotState::Restorable
    } else if action == "opened" {
        SnapshotState::OlderFile
    } else {
        SnapshotState::Dropped
    }
}

fn status_text(state: SnapshotState, rank: usize) -> String {
    let count = MAX_SNAPSHOTS.to_string();
    match state {
        SnapshotState::Restorable => i18n::tr("ui.revisions.snapshot.restorable").to_string(),
        SnapshotState::OlderFile => i18n::tr("ui.revisions.snapshot.older").to_string(),
        SnapshotState::Dropped if rank >= MAX_SNAPSHOTS => i18n::format("ui.revisions.snapshot.dropped", &[("count", &count)]),
        SnapshotState::Dropped => i18n::tr("ui.revisions.snapshot.none").to_string(),
    }
}

fn status_color(state: SnapshotState, tokens: Tokens) -> egui::Color32 {
    match state {
        SnapshotState::Restorable => tokens.accent,
        SnapshotState::Dropped | SnapshotState::OlderFile => tokens.text_dim,
    }
}

fn action_label(action: &str) -> String {
    match action {
        "saved" => i18n::tr("ui.revisions.action.saved").to_string(),
        "opened" => i18n::tr("ui.revisions.action.opened").to_string(),
        other => other.to_string(),
    }
}

fn feature_line(total: usize) -> String {
    if total == 0 {
        i18n::tr("ui.revisions.features.none").to_string()
    } else if total == 1 {
        i18n::tr("ui.revisions.features.one").to_string()
    } else {
        let count = total.to_string();
        i18n::format("ui.revisions.features.count", &[("count", &count)])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::doc::document_history::{DocumentHistory, MAX_SNAPSHOTS};

    use super::open;
    use crate::scenario::Harness;

    fn save_snapshot(app: &mut crate::SolveApp) {
        let snapshot = DocumentHistory::model_snapshot(&app.session.design());
        app.session.doc_mut().document_history.record_save_with(snapshot);
    }

    #[test]
    fn start_opens_the_window_and_execute_does_not() {
        let mut h = Harness::new();
        h.app.run("doc.revisions", json!({})).unwrap();
        assert!(h.app.revisions.is_none(), "execute lists revisions and does not open the window");
        h.step(&json!({"start": "doc.revisions"})).unwrap();
        assert!(h.app.revisions.is_some(), "ui.start opens the window");
        let listed = h.app.run("doc.revisions", json!({})).unwrap();
        assert_eq!(listed["revision"].as_u64(), Some(0));
        assert!(listed["entries"].as_array().is_some_and(|entries| entries.is_empty()));
        assert!(h.app.revisions.is_some(), "execute leaves the window open");
        h.frames(2);
        assert!(h.widget("No revisions yet. Save the design to record the first one.").is_some());
        assert!(h.widget("Current save count: 0").is_some());

        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.revisions = None;
        h.step(&json!({"call": "ui.home", "params": {"open": true}})).unwrap();
        h.step(&json!({"click": {"handle": "home:revisions"}})).unwrap();
        assert!(h.app.revisions.is_some(), "File view, Revisions opens the window");
        assert!(!h.app.home.open);
    }

    #[test]
    fn restore_rolls_the_working_model_back_and_read_only_opens_the_later_save() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10, "name": "First"})).unwrap();
        let first = h.app.session.doc.features.first().map(|feature| feature.id);
        save_snapshot(&mut h.app);
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "name": "Second"})).unwrap();
        save_snapshot(&mut h.app);
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert_eq!(h.app.session.doc.document_history.revision, 2);

        open(&mut h.app);
        h.frames(2);
        assert!(h.widget("Current save count: 2").is_some());
        assert!(h.widget("Restorable snapshots: 2 of the newest 2.").is_some());
        assert!(h.widget("Restorable snapshot").is_some());

        h.step(&json!({"widget": "Revision 1"})).unwrap();
        h.frames(2);
        assert!(h.widget("1 feature in this snapshot.").is_some(), "selecting a revision shows the features stored in it");
        assert!(h.widget("First").is_some());
        h.step(&json!({"widget": "Restore"})).unwrap();
        h.frames(1);
        assert_eq!(h.app.session.doc.features.len(), 1);
        assert_eq!(h.app.session.doc.features.first().map(|feature| feature.id), first);
        assert_eq!(h.app.session.doc.document_history.revision, 2, "restore keeps the save count");
        assert!(h.app.session.revision_view.is_none());
        assert!(h.app.revisions.is_some());
        let volume = h.app.run("inspect.measure", json!({})).unwrap();
        assert!((volume["total"]["volume_mm3"].as_f64().unwrap() - 8000.0).abs() < 1e-6, "{volume}");

        h.step(&json!({"widget": "Revision 2"})).unwrap();
        h.frames(2);
        assert!(h.widget("2 features in this snapshot.").is_some());
        h.step(&json!({"widget": "Open read-only"})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert_eq!(h.app.session.revision_view, Some(2));
        assert!(h.widget("Revision 2 is open read-only. Edits and Save stay off until you restore a working model or use Save As.").is_some());
        assert!(h.app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).is_err());
        let later = h.app.run("inspect.measure", json!({})).unwrap();
        assert!((later["total"]["volume_mm3"].as_f64().unwrap() - 9000.0).abs() < 1e-6, "{later}");
    }

    #[test]
    fn dropped_snapshot_and_older_file_say_so() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        for _ in 0..MAX_SNAPSHOTS + 1 {
            save_snapshot(&mut h.app);
        }
        assert!(h.app.session.doc.document_history.entries.first().is_some_and(|entry| entry.snapshot.is_none()));
        assert_eq!(h.app.session.doc.document_history.entries.iter().filter(|entry| entry.snapshot.is_some()).count(), MAX_SNAPSHOTS);
        open(&mut h.app);
        h.frames(2);
        let dropped = format!("Snapshot dropped. This revision is older than the newest {MAX_SNAPSHOTS} that can keep one.");
        assert!(h.widget(&dropped).is_some(), "a pruned revision says the snapshot was dropped");
        assert!(h.widget(&format!("Restorable snapshots: {MAX_SNAPSHOTS} of the newest {MAX_SNAPSHOTS}.")).is_some());
        let features = h.app.session.doc.features.len();
        let revision = h.app.session.doc.document_history.revision;
        h.step(&json!({"widget": "Revision 1"})).unwrap();
        h.frames(1);
        h.step(&json!({"widget": "Restore"})).unwrap();
        assert_eq!(h.app.session.doc.features.len(), features, "a dropped snapshot cannot be restored");
        assert_eq!(h.app.session.doc.document_history.revision, revision);

        let mut older = Harness::new();
        older.app.session.doc_mut().document_history = DocumentHistory::migrated(1);
        open(&mut older.app);
        older.frames(2);
        assert!(older.widget("No snapshot. This revision records an older file that did not store one.").is_some());
        assert!(older.widget("Current save count: 0").is_some());
        assert!(older.widget("Format solidcraft/1").is_some());
        older.step(&json!({"widget": "Revision 0"})).unwrap();
        older.frames(1);
        older.step(&json!({"widget": "Restore"})).unwrap();
        assert!(older.app.session.doc.features.is_empty());
        assert_eq!(older.app.session.doc.document_history.revision, 0);
    }

    #[test]
    fn a_worker_save_adopts_the_revision_onto_the_open_session() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Plate.solidcraft");
        assert!(crate::save_job::start(&mut h.app, "file.save_as", json!({"path": path.to_string_lossy()})));
        h.frame();
        h.settle();
        assert!(h.app.saving.is_none());
        assert_eq!(h.app.session.doc.document_history.revision, 1);
        assert!(h.app.session.doc.document_history.entries.first().is_some_and(|entry| entry.snapshot.is_some()));
        assert!(!h.app.session.is_dirty());
    }
}
