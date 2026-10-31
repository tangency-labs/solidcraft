//! The Crash Reports window (Help > Crash Reports…): the reports `solidcraft_engine::crash`
//! keeps on this computer, each with a reviewed issue draft. It opens by itself after a crash.

use solidcraft_engine::crash::{self, Summary};

use crate::SolveApp;
use crate::ui_kit;

#[derive(Default)]
pub struct CrashUi {
    pub open: bool,
    /// The crash folder (default: the engine's, `crash::default_dir`).
    pub folder: Option<std::path::PathBuf>,
    reports: Vec<Summary>,
    dir: Option<String>,
    note: String,
}

impl CrashUi {
    /// Cached report count; rendering preferences never reads the filesystem.
    pub(crate) fn saved_count(&self) -> usize {
        self.reports.len()
    }

    /// Read the reports again (when the window opens or changes; never every frame).
    pub fn refresh(&mut self) {
        let dir = self.dir_path();
        self.reports = dir.as_deref().map(crash::list).unwrap_or_default();
        self.dir = dir.map(|d| d.to_string_lossy().into_owned());
    }
}

impl CrashUi {
    fn dir_path(&self) -> Option<std::path::PathBuf> {
        self.folder.clone().or_else(crash::default_dir)
    }
}

fn kind_label(kind: &str) -> &'static str {
    crate::i18n::tr(match kind {
        "command_panic" => "crash.kind_command",
        "kernel_panic_recovered" => "crash.kind_kernel",
        "unclean_exit" => "crash.kind_unclean",
        _ => "crash.kind_internal",
    })
}

fn report_time(r: &Summary) -> String {
    let time = r.time.replace('T', " ");
    let time = time.trim_end_matches('Z');
    if r.kind == "unclean_exit" { crate::i18n::format("crash.detected_time", &[("time", time)]) } else { format!("{time} UTC") }
}

fn report_heading(r: &Summary) -> String {
    let time = report_time(r);
    let times = if r.count > 1 { format!("  ×{}", r.count) } else { String::new() };
    format!("{time}  {}{times}", kind_label(&r.kind))
}

/// The host found these new reports at startup: tell the user about real crashes (recovered
/// kernel errors stay quiet; they are listed in the window).
pub fn announce(app: &mut SolveApp, fresh: &[Summary]) {
    let crashes = fresh.iter().filter(|r| r.kind != "kernel_panic_recovered").count();
    if crashes == 0 {
        return;
    }
    app.crashes.open = true;
    app.crashes.refresh();
    let locale = crate::i18n::Locale::for_language(&app.preferences.language);
    app.set_status(locale.text("crash.previous_session"), true);
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.crashes.open {
        return;
    }
    let mut report: Option<String> = None;
    let mut delete: Option<Option<String>> = None;
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_crash_reports"), "bug_report", crate::i18n::tr_label("Crash Reports"))
        .cards(2.0)
        .actions("", crate::i18n::tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui.spacing_mut().item_spacing.y = crate::theme::Tokens::get().space_10;
            let intro = ui_kit::text(ui, crate::i18n::tr("crash.intro"), ui_kit::Text::Hint("intro"));
            crate::scenario::publish_rect("dialog:note:intro", intro.rect);
            if let Some(d) = app.crashes.dir.clone() {
                // A long path is cut short (hover shows it all): it must not widen the card.
                ui_kit::path_chip(ui, &d, |ui| {
                    if ui_kit::icon_label_button(ui, crate::i18n::tr("crash.copy_folder"), "copy", true, false).clicked() {
                        ctx.copy_text(d.clone());
                    }
                });
            }
            // The list/empty-box top border replaces the old divider; retain its geometry alias.
            crate::scenario::publish_rect(
                "dialog:divider:reports",
                egui::Rect::from_min_size(ui.next_widget_position(), egui::vec2(ui.available_width(), 1.0)),
            );
            if app.crashes.reports.is_empty() {
                ui_kit::empty_box(ui, "none", crate::i18n::tr_label("No crash reports."));
            }
            if !app.crashes.reports.is_empty() {
                ui_kit::report_list(
                    ui,
                    "reports",
                    crate::theme::Tokens::get().control_height * 10.0 + crate::theme::Tokens::get().space_10 * 4.0,
                    |ui| {
                        let last = app.crashes.reports.len().saturating_sub(1);
                        for (index, r) in app.crashes.reports.iter().enumerate() {
                            let title = report_heading(r);
                            ui.push_id(&r.id, |ui| {
                                ui.spacing_mut().item_spacing.y = crate::theme::Tokens::get().space_6;
                                ui.add_space(crate::theme::Tokens::get().space_10);
                                ui_kit::report_header(ui, kind_label(&r.kind), r.count, &report_time(r), &title, |ui| {
                                    if ui_kit::bin_button(ui, crate::i18n::tr("ui.delete")).clicked() {
                                        delete = Some(Some(r.id.clone()));
                                    }
                                    ui_kit::icon_label_button(ui, crate::i18n::tr("crash.copy_path"), "copy", true, false)
                                        .clicked()
                                        .then(|| ctx.copy_text(r.path.clone()));
                                    if ui_kit::icon_label_button(ui, crate::i18n::tr("crash.report"), "bug_report", true, false).clicked() {
                                        report = Some(r.id.clone());
                                    }
                                });
                                let mut detail = r.command.clone().unwrap_or_default();
                                if r.replayable {
                                    let sep = if detail.is_empty() { "" } else { " · " };
                                    detail = format!("{detail}{sep}{}", crate::i18n::tr("crash.replayable"));
                                }
                                if !detail.is_empty() {
                                    ui_kit::text(ui, &detail, ui_kit::Text::MonoNote(&format!("detail:{}", r.id)));
                                }
                                ui_kit::text(ui, &r.message, ui_kit::Text::Report(&format!("message:{}", r.id), false));
                                if r.kind == "unclean_exit" {
                                    ui_kit::text(ui, crate::i18n::tr("crash.unclean_explanation"), ui_kit::Text::Note(&format!("unclean:{}", r.id)));
                                }
                                ui.add_space(crate::theme::Tokens::get().space_6);
                                if index != last {
                                    ui_kit::report_divider(ui, &format!("end:{}", r.id));
                                }
                            });
                        }
                    },
                );
            }
            if !app.crashes.note.is_empty() {
                ui_kit::text(ui, &app.crashes.note, ui_kit::Text::Note("note"));
            }
            if ui_kit::icon_label_button(ui, crate::i18n::tr_label("Delete all"), "delete", !app.crashes.reports.is_empty(), true).clicked() {
                delete = Some(None);
            }
        });
    app.crashes.open = !(closed || action == ui_kit::Action::Cancel);
    if let Some(id) = report
        && let Some(dir) = app.crashes.dir_path()
    {
        match crash::read(&dir, &id) {
            Ok(v) => crate::issue_report::begin_crash(app, ctx, &v),
            Err(e) => app.crashes.note = e,
        }
    }
    if let Some(which) = delete
        && let Some(dir) = app.crashes.dir_path()
    {
        let n = crash::clear(&dir, which.as_deref());
        app.crashes.note = crate::i18n::format("crash.deleted", &[("count", &n.to_string())]);
        app.crashes.refresh();
    }
}

/// Help > Crash Reports…
pub fn open(app: &mut SolveApp) {
    app.crashes.open = true;
    app.crashes.note.clear();
    app.crashes.refresh();
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    fn folder(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-crash-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let id = "20261012-162341-bb60791ed6692f1d";
        std::fs::create_dir_all(d.join(id)).unwrap();
        let report = json!({
            "format": "solidcraft-crash/1", "id": id, "kind": "unclean_exit", "count": 2,
            "first_seen": "2026-10-12T16:23:41Z", "message": "SolidCraft ended without a clean exit while running `solid.fillet`",
            "location": "unclean exit in solid.fillet", "thread": "(unknown)", "backtrace": [],
            "command": {"command": "solid.fillet", "params": {"radius": 2}},
            "recent_commands": [{"command": "solid.box", "ok": true}],
            "app": {"version": "0.3.2", "commit": "abc1234", "built": "2026-10-12"},
            "system": {"os": "linux", "arch": "x86_64", "environment": "Renderer: test"},
        });
        std::fs::write(d.join(id).join("report.json"), report.to_string()).unwrap();
        std::fs::write(d.join(id).join("replay.json"), r#"{"commands":[]}"#).unwrap();
        d
    }

    #[test]
    fn an_old_unclean_exit_is_shown_as_detected_on_a_later_launch() {
        let d = folder("old-session");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.crashes.folder = Some(d.clone());
        let id = "20261012-162341-bb60791ed6692f1d";
        let path = d.join(id).join("report.json");
        let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        report["message"] = json!("SolidCraft 0.3.2 (started 2026-10-01T10:00:00Z) ended without a clean exit between commands");
        report["command"] = serde_json::Value::Null;
        std::fs::write(&path, report.to_string()).unwrap();
        super::announce(&mut h.app, &solidcraft_engine::crash::list(&d));
        h.frames(6);
        assert!(h.app.crashes.open, "unclean exits still alert the user");
        let status = &h.app.status.as_ref().unwrap().0;
        assert!(status.contains("earlier SolidCraft session"), "{status}");
        assert!(status.contains("detected at startup"), "{status}");
        assert!(h.widget("Detected 2026-10-12 16:23:41 UTC  Closed unexpectedly  ×2").is_some());
        assert!(h.widget(report["message"].as_str().unwrap()).is_some(), "keep the original session start visible");
        assert!(h.widget(crate::i18n::tr("crash.unclean_explanation")).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_panic_keeps_its_event_time_without_the_unclean_exit_explanation() {
        let d = folder("panic-time");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.crashes.folder = Some(d.clone());
        let path = d.join("20261012-162341-bb60791ed6692f1d").join("report.json");
        let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        report["kind"] = json!("command_panic");
        std::fs::write(&path, report.to_string()).unwrap();
        super::announce(&mut h.app, &solidcraft_engine::crash::list(&d));
        h.frames(6);
        assert!(h.app.crashes.open);
        assert!(h.widget("2026-10-12 16:23:41 UTC  Internal error in a command  ×2").is_some());
        assert!(h.widget(crate::i18n::tr("crash.unclean_explanation")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_crash_report_opens_a_reviewed_issue_draft_and_can_be_deleted() {
        let d = folder("draft");
        let mut h = crate::scenario::Harness::new();
        h.app.crashes.folder = Some(d.clone());
        let fresh = solidcraft_engine::crash::list(&d);
        assert_eq!(fresh.len(), 1);
        super::announce(&mut h.app, &fresh);
        assert!(h.app.crashes.open, "a real crash opens the window");
        assert!(h.app.status.as_ref().is_some_and(|s| s.2), "and says so");
        h.frames(4);
        h.step(&json!({"widget": "Report…"})).unwrap();
        h.frames(2);
        let draft = h.app.issue_report.draft.as_ref().expect("Report… opens the draft");
        assert!(draft.title.starts_with("Crash: SolidCraft ended without a clean exit"), "{}", draft.title);
        for want in [
            "while running `solid.fillet`",
            "\"radius\": 2",
            "solid.box",
            "solidcraft-cli crashes replay 20261012-162341-bb60791ed6692f1d",
            "Renderer: test",
            "Recorded 2 time(s)",
        ] {
            assert!(draft.body.contains(want), "missing {want:?} in {}", draft.body);
        }
        assert!(!draft.body.contains(&d.to_string_lossy().into_owned()), "the draft names no local paths");
        h.app.issue_report.draft = None;
        h.frames(2);
        h.step(&json!({"widget": "Delete all"})).unwrap();
        h.frames(2);
        assert!(solidcraft_engine::crash::list(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_long_folder_path_does_not_widen_the_window_past_its_title_bar() {
        let d = folder(&format!("{}-long", "very-long-folder-name-".repeat(6)));
        let mut h = crate::scenario::Harness::new();
        h.app.crashes.folder = Some(d.clone());
        super::open(&mut h.app);
        h.frames(6);
        let window = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_crash_reports"))).unwrap();
        // The frame's margins come on top of the content's maximum width.
        assert!(window.width() <= 760.0 + 40.0, "{} wide", window.width());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn recovered_kernel_errors_do_not_interrupt() {
        let mut h = crate::scenario::Harness::new();
        let quiet = solidcraft_engine::crash::Summary {
            id: "x".into(),
            kind: "kernel_panic_recovered".into(),
            time: String::new(),
            message: String::new(),
            command: None,
            count: 1,
            version: String::new(),
            path: String::new(),
            replayable: false,
        };
        super::announce(&mut h.app, &[quiet]);
        assert!(!h.app.crashes.open);
    }

    #[test]
    fn inventory_actions_details_empty_state_and_keyboard_remain_reachable() {
        let d = folder("inventory");
        let mut h = crate::scenario::Harness::new();
        h.app.crashes.folder = Some(d.clone());
        super::open(&mut h.app);
        h.frames(5);
        for handle in [
            "dialog:close",
            "dialog:cancel",
            "dialog:Close",
            "dialog:button::Copy folder path",
            "dialog:button::Report…",
            "dialog:button::Copy path",
            "dialog:button::Delete",
            "dialog:button::Delete all",
        ] {
            assert!(crate::scenario::handle_at(handle).is_some(), "missing {handle}");
        }
        for rect in [
            "dialog:note:intro",
            "dialog:divider:reports",
            "dialog:list:reports",
            "dialog:note:detail:20261012-162341-bb60791ed6692f1d",
            "dialog:report:message:20261012-162341-bb60791ed6692f1d",
        ] {
            assert!(crate::scenario::rect_of(rect).is_some(), "missing {rect}");
        }
        let title = "Detected 2026-10-12 16:23:41 UTC  Closed unexpectedly  ×2";
        assert!(crate::scenario::handle_at(&format!("dialog:cell:{title}")).is_some());
        assert!(crate::scenario::handle_at(&format!("dialog:cell:{}", d.display())).is_some());
        h.step(&json!({"click":{"handle":"dialog:button::Copy folder path"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:button::Copy path"}})).unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.crashes.open, "Enter has no primary action");
        h.step(&json!({"click":{"handle":"dialog:button::Delete"}})).unwrap();
        h.frames(3);
        assert!(h.app.crashes.reports.is_empty());
        assert!(crate::scenario::rect_of("dialog:note:none").is_some());
        assert!(crate::scenario::rect_of("dialog:note:note").is_some());
        h.step(&json!({"click":{"handle":"dialog:button::Delete all"}})).unwrap();
        assert!(h.app.crashes.reports.is_empty());
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(!h.app.crashes.open);
        let _ = std::fs::remove_dir_all(d);
    }
}
