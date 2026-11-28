//! The Crash Reports window (Help > Crash Reports…): the reports `solidcraft_engine::crash`
//! keeps on this computer, each with a reviewed issue draft. It opens by itself after a crash.
//! After a graphics failure (a lost device, a start that died in the graphics driver) it also
//! offers to start again on another graphics API ([`retry_api`]), the Graphics preference's
//! choice, through the same restart as Preferences › Graphics.

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
    /// The graphics API this start offers to try after a graphics failure ([`retry_api`]).
    pub retry: Option<&'static str>,
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
    solidcraft_ui_text::tr(match kind {
        "command_panic" => "crash.kind_command",
        "kernel_panic_recovered" => "crash.kind_kernel",
        "unclean_exit" => "crash.kind_unclean",
        "device_lost" => "crash.kind_device_lost",
        _ => "crash.kind_internal",
    })
}

fn report_time(r: &Summary) -> String {
    let time = r.time.replace('T', " ");
    let time = time.trim_end_matches('Z');
    if r.kind == "unclean_exit" { solidcraft_ui_text::format("crash.detected_time", &[("time", time)]) } else { format!("{time} UTC") }
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
    // The API this start draws with on a hardware adapter (safe mode's software one doesn't count).
    let software = app.help.gpu.as_deref().is_some_and(|g| g.trim_end().ends_with("Cpu)")) || app.blur.software_adapter();
    let running = app.services.graphics_backend.filter(|_| app.viewport.gpu.is_some() && !software);
    app.crashes.retry = retry_api(fresh, crate::prefs::GRAPHICS_BACKENDS, running);
    app.crashes.open = true;
    app.crashes.refresh();
    let locale = solidcraft_ui_text::Locale::for_language(&app.preferences.language);
    app.set_status(locale.text("crash.previous_session"), true);
}

/// The graphics API to offer after the graphics failed in a run these new reports are about: the
/// first of `choices` (the Graphics preference's, for this platform) that is not the API that
/// failed, nor the one this start already draws with on the GPU (`running`), in the order that
/// most often works where another failed: Direct3D 12 on Windows, then Vulkan; OpenGL on Linux.
/// None without a graphics failure, or with nothing else to try (macOS has Metal alone).
pub fn retry_api(fresh: &[Summary], choices: &[&'static str], running: Option<&str>) -> Option<&'static str> {
    let failed = fresh.iter().find(|r| r.graphics_failure)?;
    let failed_api = failed.renderer.as_deref().and_then(crash::graphics).and_then(|g| g.api_id());
    let order: &[&str] = if choices.contains(&"dx12") { &["dx12", "vulkan", "gl"] } else { &["gl", "vulkan"] };
    order
        .iter()
        .filter_map(|want| choices.iter().find(|c| *c == want).copied())
        .find(|api| Some(*api) != failed_api && Some(*api) != running)
}

/// The note and button offering [`CrashUi::retry`]: saves the choice as Preferences › Graphics
/// does, then restarts through the normal unsaved-design prompts. Nothing when this launch can't
/// restart itself or a developer override fixes the API.
fn retry_offer(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(api) = app.crashes.retry else { return };
    if crate::prefs::check_graphics_restart(app).is_err() {
        return;
    }
    let name = crate::prefs::graphics_backend_label(api);
    let note = solidcraft_ui_text::format("crash.graphics_retry_note", &[("api", &name)]);
    ui_kit::text(ui, &note, ui_kit::Text::Note("graphics_retry"));
    let label = solidcraft_ui_text::format("crash.graphics_retry", &[("api", &name)]);
    let response = ui_kit::icon_label_button(ui, &label, "refresh", true, false);
    crate::scenario::publish_target("crash:graphics_retry", response.rect);
    if response.clicked() {
        let started = crate::prefs::set_graphics_backend(app, api).and_then(|()| crate::prefs::request_graphics_restart(app));
        match started {
            Ok(()) => app.crashes.retry = None,
            Err(e) => app.crashes.note = e,
        }
    }
}

pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.crashes.open {
        return;
    }
    let mut report: Option<String> = None;
    let mut delete: Option<Option<String>> = None;
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_crash_reports"), "bug_report", solidcraft_ui_text::tr_label("Crash Reports"))
        .cards(2.0)
        .actions("", solidcraft_ui_text::tr("ui.close"), None)
        .show(ctx, true, |ui| {
            ui.spacing_mut().item_spacing.y = solidcraft_theme::Tokens::get().space_10;
            let intro = ui_kit::text(ui, solidcraft_ui_text::tr("crash.intro"), ui_kit::Text::Hint("intro"));
            crate::scenario::publish_rect("dialog:note:intro", intro.rect);
            retry_offer(app, ui);
            if let Some(d) = app.crashes.dir.clone() {
                // A long path is cut short (hover shows it all): it must not widen the card.
                ui_kit::path_chip(ui, &d, |ui| {
                    if ui_kit::icon_label_button(ui, solidcraft_ui_text::tr("crash.copy_folder"), "copy", true, false).clicked() {
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
                ui_kit::empty_box(ui, "none", solidcraft_ui_text::tr_label("No crash reports."));
            }
            if !app.crashes.reports.is_empty() {
                ui_kit::report_list(
                    ui,
                    "reports",
                    solidcraft_theme::Tokens::get().control_height * 10.0 + solidcraft_theme::Tokens::get().space_10 * 4.0,
                    |ui| {
                        let last = app.crashes.reports.len().saturating_sub(1);
                        for (index, r) in app.crashes.reports.iter().enumerate() {
                            let title = report_heading(r);
                            ui.push_id(&r.id, |ui| {
                                ui.spacing_mut().item_spacing.y = solidcraft_theme::Tokens::get().space_6;
                                ui.add_space(solidcraft_theme::Tokens::get().space_10);
                                ui_kit::report_header(ui, kind_label(&r.kind), r.count, &report_time(r), &title, |ui| {
                                    if ui_kit::bin_button(ui, solidcraft_ui_text::tr("ui.delete")).clicked() {
                                        delete = Some(Some(r.id.clone()));
                                    }
                                    ui_kit::icon_label_button(ui, solidcraft_ui_text::tr("crash.copy_path"), "copy", true, false)
                                        .clicked()
                                        .then(|| ctx.copy_text(r.path.clone()));
                                    if ui_kit::icon_label_button(ui, solidcraft_ui_text::tr("crash.report"), "bug_report", true, false).clicked() {
                                        report = Some(r.id.clone());
                                    }
                                });
                                let mut detail = r.command.clone().unwrap_or_default();
                                if r.replayable {
                                    let sep = if detail.is_empty() { "" } else { " · " };
                                    detail = format!("{detail}{sep}{}", solidcraft_ui_text::tr("crash.replayable"));
                                }
                                if !detail.is_empty() {
                                    ui_kit::text(ui, &detail, ui_kit::Text::MonoNote(&format!("detail:{}", r.id)));
                                }
                                ui_kit::text(ui, &r.message, ui_kit::Text::Report(&format!("message:{}", r.id), false));
                                if r.kind == "unclean_exit" {
                                    ui_kit::text(
                                        ui,
                                        solidcraft_ui_text::tr("crash.unclean_explanation"),
                                        ui_kit::Text::Note(&format!("unclean:{}", r.id)),
                                    );
                                }
                                ui.add_space(solidcraft_theme::Tokens::get().space_6);
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
            if ui_kit::icon_label_button(ui, solidcraft_ui_text::tr_label("Delete all"), "delete", !app.crashes.reports.is_empty(), true).clicked() {
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
        app.crashes.note = solidcraft_ui_text::format("crash.deleted", &[("count", &n.to_string())]);
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
        assert!(h.widget(solidcraft_ui_text::tr("crash.unclean_explanation")).is_some());
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
        assert!(h.widget(solidcraft_ui_text::tr("crash.unclean_explanation")).is_none());
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

    fn summary(kind: &str, renderer: Option<&str>, graphics_failure: bool) -> solidcraft_engine::crash::Summary {
        solidcraft_engine::crash::Summary {
            id: "20261127-100000-0000000000000001".into(),
            kind: kind.into(),
            time: "2026-11-27T10:00:00Z".into(),
            message: "The graphics device was lost".into(),
            command: None,
            count: 1,
            version: "0.51.0".into(),
            path: String::new(),
            replayable: false,
            renderer: renderer.map(str::to_string),
            graphics_failure,
        }
    }

    /// After a graphics failure the next start offers the API that most often works instead: never
    /// the one that failed, nor the one it already draws with; nothing after other crashes, or
    /// where there is no other API.
    #[test]
    fn a_graphics_failure_offers_another_graphics_api() {
        let windows = &["auto", "dx12", "vulkan", "gl"];
        let linux = &["auto", "vulkan", "gl"];
        let vulkan = Some("Renderer: Vulkan Intel(R) UHD Graphics 620 (IntegratedGpu, driver Intel 31.0)");
        let dx12 = Some("Renderer: Dx12 Intel(R) UHD Graphics 620 (IntegratedGpu, driver Intel 31.0)");
        let lost = [summary("device_lost", vulkan, true)];
        assert_eq!(super::retry_api(&lost, windows, None), Some("dx12"));
        assert_eq!(super::retry_api(&lost, linux, None), Some("gl"));
        assert_eq!(super::retry_api(&[summary("unclean_exit", dx12, true)], windows, None), Some("vulkan"), "D3D12 itself failed");
        assert_eq!(super::retry_api(&lost, windows, Some("dx12")), Some("gl"), "already on D3D12 hardware: the next one");
        assert_eq!(super::retry_api(&[summary("unclean_exit", None, true)], windows, None), Some("dx12"), "the failed API unknown");
        assert_eq!(super::retry_api(&[summary("unclean_exit", vulkan, false)], windows, None), None, "not a graphics failure");
        assert_eq!(super::retry_api(&lost, &["auto", "metal"], None), None, "macOS has nothing else");
    }

    /// The offer is a note and a button in the window that opens after the failure; the button
    /// saves the API as Preferences › Graphics does and restarts. Without the failure, no offer.
    #[test]
    fn the_offer_after_a_graphics_failure_saves_the_api_and_restarts() {
        let d = std::env::temp_dir().join(format!("solidcraft-crash-ui-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.crashes.folder = Some(d.clone());
        std::fs::write(d.join("graphics.json"), br#"{"backend":"auto"}"#).unwrap();
        h.app.services.graphics_path = Some(d.join("graphics.json").to_string_lossy().into_owned());
        h.app.services.graphics_restart = Some(("solidcraft".into(), Vec::new()));
        let want = super::retry_api(
            &[summary("device_lost", Some("Renderer: Vulkan Test GPU (IntegratedGpu, driver 1)"), true)],
            crate::prefs::GRAPHICS_BACKENDS,
            None,
        );
        super::announce(&mut h.app, &[summary("device_lost", Some("Renderer: Vulkan Test GPU (IntegratedGpu, driver 1)"), true)]);
        assert_eq!(h.app.crashes.retry, want);
        h.frames(4);
        let Some(api) = want else {
            // macOS: Metal alone, nothing to offer.
            assert!(crate::scenario::rect_of("crash:graphics_retry").is_none());
            let _ = std::fs::remove_dir_all(&d);
            return;
        };
        let label = format!("Start with {}", crate::prefs::graphics_backend_label(api));
        assert!(h.widget(&label).is_some(), "the button names the API");
        h.step(&json!({"widget": label})).unwrap();
        h.frames(2);
        assert_eq!(h.app.preferences.graphics_backend, api);
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("graphics.json")).unwrap()).unwrap();
        assert_eq!(saved["backend"], json!(api), "saved as Preferences › Graphics saves it");
        assert!(crate::update::take_launch(&mut h.app).is_some(), "and the restart is under way");
        // Another crash, not the graphics: no offer.
        let mut h = crate::scenario::Harness::new();
        h.app.crashes.folder = Some(d.clone());
        h.app.services.graphics_restart = Some(("solidcraft".into(), Vec::new()));
        super::announce(&mut h.app, &[summary("command_panic", None, false)]);
        assert_eq!(h.app.crashes.retry, None);
        h.frames(4);
        assert!(crate::scenario::rect_of("crash:graphics_retry").is_none());
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
            renderer: None,
            graphics_failure: false,
        };
        super::announce(&mut h.app, &[quiet]);
        assert!(!h.app.crashes.open);
    }

    #[test]
    fn inventory_actions_details_empty_state_and_keyboard_remain_reachable() {
        let d = folder("inventory");
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.language = "en".into();
        h.app.home.open = false;
        h.app.crashes.folder = Some(d.clone());
        let assert_general_count = |h: &mut crate::scenario::Harness, count: usize| {
            // The fixture folder was assigned after startup; programmatic Preferences uses the cached inventory.
            h.app.crashes.refresh();
            for theme in ["light", "dark"] {
                h.call("ui.prefs", json!({"open":"General", "theme":theme}));
                crate::prefs::reveal_preference(h, "prefs:crash_count");
                assert_eq!(h.app.crashes.saved_count(), count);
                assert!(h.widget(&format!("{count} saved")).is_some());
                let badge = crate::scenario::rect_of("prefs:crash_count").unwrap();
                assert!((badge.height() - 18.0).abs() < 0.5);
                assert!(crate::scenario::rect_of("prefs:page_body").unwrap().contains_rect(badge));
                let t = solidcraft_theme::Tokens::get();
                let fill = if count == 0 { t.dialog_footer } else { t.pref_note_fill };
                assert!(
                    h.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect == badge && rect.fill == fill)),
                    "{theme}: count tone reflects reports waiting"
                );
                let buttons: Vec<_> = h
                    .output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.role() == egui::accesskit::Role::Button && node.label() == Some(h.app.tr("prefs.crash_reports")))
                    .collect();
                assert_eq!(buttons.len(), 1);
                assert!(!buttons[0].1.is_disabled(), "opening reports stays available even at zero");
                h.step(&json!({"click":{"handle":"prefs:cancel"}})).unwrap();
            }
        };
        assert_general_count(&mut h, 1);
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
        assert_general_count(&mut h, 0);
        let _ = std::fs::remove_dir_all(d);
    }
}
