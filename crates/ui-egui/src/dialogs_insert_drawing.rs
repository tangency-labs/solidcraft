//! Insert SVG / Insert DXF: once the file is picked, the drawing is placed on the sketch (where
//! its middle goes, its size in millimetres or as a factor, and an angle) with a live preview,
//! and OK adds it (#2605, #3443).

use crate::ui_kit::BackingTooltipExt as _;
use serde_json::{Value, json};
use solidcraft_engine::geom::PlaneGeom as _;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row};

/// Largest scale the insert commands take.
pub(crate) const MAX_SCALE: f64 = 1e5;

/// The drawing being placed: the insert command, its file, and where, how big and how turned
/// it goes. `x` and `y` are where the drawing's middle lands; it scales and turns about it.
#[derive(Clone, Debug)]
pub struct Place {
    pub command: String,
    pub path: String,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    /// Degrees, counter-clockwise.
    pub angle: f64,
    /// The drawing's own width and height in the file, when it could be read.
    pub natural: Option<(f64, f64)>,
    /// SVG repair is explicit; this limit is in millimetres after placement scaling.
    pub svg: SvgOptions,
}

type DiagnosticReply = Arc<Mutex<Option<Result<Value, String>>>>;

/// The import form's transient options and diagnostic result. None is saved in the design.
#[derive(Clone, Debug)]
pub struct SvgOptions {
    pub repair: bool,
    pub review: bool,
    pub original_view: bool,
    repair_open: bool,
    pub tolerance: f64,
    draft_tolerance: f64,
    pub show_changes: bool,
    pub show_curves: bool,
    pub show_faces: bool,
    pub show_placement: bool,
    /// The single viewport status item for every SVG integrity fact; its ✕ hides it.
    pub show_integrity: bool,
    pub(crate) diagnostics: Arc<Vec<Diagnostic>>,
    pub(crate) selected: Option<usize>,
    requested: Option<Value>,
    reply: Option<DiagnosticReply>,
    error: Option<String>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            repair: false,
            review: false,
            original_view: false,
            repair_open: false,
            tolerance: 0.01,
            draft_tolerance: 0.01,
            show_changes: false,
            show_curves: true,
            show_faces: true,
            show_placement: true,
            show_integrity: true,
            diagnostics: Arc::new(Vec::new()),
            selected: None,
            requested: None,
            reply: None,
            error: None,
        }
    }
}

/// Display geometry from the diagnostic command; these polylines never drive modelling.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Diagnostic {
    pub path: usize,
    pub subpath: usize,
    pub segment: usize,
    pub kind: String,
    pub at: [f64; 2],
    pub highlight: Vec<[f64; 2]>,
    #[serde(default)]
    pub replacement_highlight: Vec<Vec<[f64; 2]>>,
    pub repaired: bool,
    pub max_deviation_mm: Option<f64>,
}

/// Open the placement for `path`, picked for `command` (`sketch.insert_svg` / `_dxf`).
pub fn open(app: &mut SolveApp, command: &str, path: String) {
    // Selecting the same path may mean its contents changed outside the application.
    app.preview.clear();
    let natural = app
        .session
        .execute("sketch.drawing_bounds", &json!({"path": path, "format": if command.ends_with("dxf") { "dxf" } else { "svg" }}))
        .ok()
        .and_then(|b| Some((b.get("width")?.as_f64()?, b.get("height")?.as_f64()?)))
        .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 1e-9 && *h > 1e-9);
    app.dialog = Some(Dialog::new(
        Kind::InsertDrawing(Place { command: command.to_string(), path, x: 0.0, y: 0.0, scale: 1.0, angle: 0.0, natural, svg: Default::default() }),
        Vec::new(),
    ));
    if command.ends_with("svg")
        && let Some((w, h)) = natural
    {
        let plane = app.session.active_sketch.and_then(|id| crate::canvas::profile_plane(app, id)).unwrap_or(solidcore_record::Plane::XY);
        let mut bounds = solidcraft_engine::geom::Aabb3::EMPTY;
        for (x, y) in [(-w * 0.5, -h * 0.5), (w * 0.5, -h * 0.5), (w * 0.5, h * 0.5), (-w * 0.5, h * 0.5)] {
            bounds.add(plane.to_world(solidcore_record::Vec2::new(x, y)));
        }
        app.animate_to(app.fitted_bounds(app.cam, &bounds));
    }
}

pub fn title(p: &Place) -> &'static str {
    if p.command.ends_with("dxf") { "INSERT DXF" } else { "INSERT SVG" }
}

pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    if p.command.ends_with("svg") {
        svg_rows(app, ui, p);
        return;
    }
    let name = std::path::Path::new(&p.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.path.clone());
    form_row(ui, "File", |ui| {
        ui.label(name).backing_on_hover_text(&p.path);
    });
    // Set the physical size first; placement remains available without crowding the first step.
    if let Some((nw, nh)) = p.natural {
        for (label, natural, handle) in [("Width", nw, "dialog:insert:width"), ("Height", nh, "dialog:insert:height")] {
            form_row(ui, label, |ui| {
                let mut mm = natural * p.scale;
                let r = crate::ui_kit::number_field(ui, &mut mm, 0.001..=natural * MAX_SCALE, 0.5, " mm");
                if r.changed() && mm.is_finite() && mm > 0.0 {
                    p.scale = (mm / natural).clamp(0.0001, MAX_SCALE);
                }
                publish_control(ui, handle, &r);
            });
        }
    }
    let placement = |ui: &mut egui::Ui, p: &mut Place| {
        form_row(ui, "Scale", |ui| {
            let r = crate::ui_kit::number_field(ui, &mut p.scale, 0.0001..=MAX_SCALE, 0.01, "");
            publish_control(ui, "dialog:insert:scale", &r);
        });
        for (label, v, handle) in [("X", &mut p.x, "dialog:insert:x"), ("Y", &mut p.y, "dialog:insert:y")] {
            form_row(ui, label, |ui| {
                let r = crate::ui_kit::number_field(ui, v, -1e9..=1e9, 0.5, " mm");
                publish_control(ui, handle, &r);
            });
        }
        form_row(ui, "Angle", |ui| {
            let r = crate::ui_kit::number_field(ui, &mut p.angle, -360.0..=360.0, 1.0, "\u{b0}");
            publish_control(ui, "dialog:insert:angle", &r);
        });
    };
    placement(ui, p);
}

/// Identity of the one status item that covers every SVG integrity fact.
pub(crate) const INTEGRITY_ID: &str = "svg_integrity";

/// What the SVG import's status item says: a short setting on the pill, the values in its key card.
pub(crate) struct Integrity {
    pub attention: bool,
    pub setting: String,
    pub rows: Vec<(String, String, crate::ui_kit::AssistantTone)>,
    pub view: String,
}

pub(crate) fn integrity(app: &SolveApp) -> Option<Integrity> {
    use crate::ui_kit::AssistantTone as Tone;
    let Kind::InsertDrawing(p) = &app.dialog.as_ref()?.kind else { return None };
    if !p.command.ends_with("svg") || !p.svg.show_integrity {
        return None;
    }
    let checking = p.svg.reply.is_some() || p.svg.requested.as_ref() != Some(&diagnostic_params(p));
    let outlines = p.svg.diagnostics.iter().map(|d| (d.path, d.subpath)).collect::<BTreeSet<_>>().len();
    let adjusted = p.svg.diagnostics.iter().filter(|d| d.repaired).map(|d| (d.path, d.subpath)).collect::<BTreeSet<_>>().len();
    let remaining = p.svg.diagnostics.iter().filter(|d| !d.repaired).count();
    let regions = region_count(app, p);
    let attention = p.svg.error.is_some() || is_stale(p) || remaining > 0;
    let curves = if p.svg.repair {
        solidcraft_ui_text::format("ui.svg_integrity_curves_adjusted", &[("outlines", &adjusted.to_string()), ("issues", &remaining.to_string())])
    } else {
        solidcraft_ui_text::format(
            "ui.svg_integrity_curves_unchanged",
            &[("issues", &p.svg.diagnostics.len().to_string()), ("outlines", &outlines.to_string())],
        )
    };
    let curves_tone = if checking {
        Tone::Working
    } else if p.svg.error.is_some() {
        Tone::Error
    } else if remaining > 0 {
        Tone::Warning
    } else if p.svg.repair && adjusted > 0 {
        Tone::Success
    } else {
        Tone::Neutral
    };
    let setting = if checking {
        solidcraft_ui_text::tr("ui.svg_checking").to_string()
    } else if attention {
        solidcraft_ui_text::format("ui.svg_integrity_issues", &[("count", &remaining.max(1).to_string())])
    } else {
        solidcraft_ui_text::tr("ui.svg_integrity_ready").to_string()
    };
    let (repair_value, repair_tone) = if p.svg.repair {
        (format!("{} mm", p.svg.tolerance), if is_stale(p) { Tone::Warning } else { Tone::Success })
    } else {
        (solidcraft_ui_text::tr("ui.svg_integrity_not_run").to_string(), Tone::Neutral)
    };
    let (regions_value, regions_tone) = match regions {
        Some(count) => (count.to_string(), if count > 0 { Tone::Success } else { Tone::Warning }),
        None => ("…".to_string(), Tone::Working),
    };
    let view = if p.svg.review {
        solidcraft_ui_text::tr("ui.svg_review_changes")
    } else if !p.svg.repair {
        solidcraft_ui_text::tr("ui.svg_integrity_original")
    } else if p.svg.original_view {
        solidcraft_ui_text::tr("ui.svg_before")
    } else {
        solidcraft_ui_text::tr("ui.svg_after")
    };
    Some(Integrity {
        attention,
        setting,
        rows: vec![
            (solidcraft_ui_text::tr("ui.svg_step_curves").to_string(), curves, curves_tone),
            (solidcraft_ui_text::tr("ui.svg_repair_section").to_string(), repair_value, repair_tone),
            (solidcraft_ui_text::tr("ui.svg_step_regions").to_string(), regions_value, regions_tone),
            (solidcraft_ui_text::tr("ui.svg_step_extrude").to_string(), solidcraft_ui_text::tr("ui.svg_not_checked").to_string(), Tone::Neutral),
        ],
        view: view.to_string(),
    })
}

/// The ✕ on the status item ends it, as for every status item; reopening the panel brings it back.
pub(crate) fn hide_integrity(app: &mut SolveApp) {
    if let Some(Kind::InsertDrawing(p)) = app.dialog.as_mut().map(|d| &mut d.kind) {
        p.svg.show_integrity = false;
    }
}

/// The comparison changes the preview and import geometry without discarding its proposal.
pub(crate) fn repair_enabled(p: &Place) -> bool {
    p.svg.repair && !p.svg.original_view
}

pub(crate) fn import_label(p: &Place) -> &'static str {
    if repair_enabled(p) { "ui.svg_import_repaired" } else { "ui.svg_import_original" }
}

pub(crate) fn repair_open(p: &Place) -> bool {
    p.svg.repair_open || p.svg.repair
}

pub(crate) fn is_stale(p: &Place) -> bool {
    p.svg.repair && (p.svg.draft_tolerance != p.svg.tolerance || p.svg.requested.as_ref() != Some(&diagnostic_params(p)))
}

pub(crate) fn placement_visible(p: &Place) -> bool {
    p.svg.show_placement && !p.svg.review && !repair_open(p)
}

/// Diagnostics must describe the current proposal; the dialog also checks its geometry preview.
pub(crate) fn preview_ready(p: &Place) -> bool {
    !p.command.ends_with("svg")
        || (p.svg.repair && p.svg.original_view && commands(p).is_ok())
        || (commands(p).is_ok()
            && p.svg.reply.is_none()
            && p.svg.error.is_none()
            && (!repair_enabled(p) || p.svg.draft_tolerance == p.svg.tolerance)
            && p.svg.requested.as_ref() == Some(&diagnostic_params(p)))
}

/// A command in a fixed, centred frame: moving and turning its overlay do not reread the file.
fn diagnostic_params(p: &Place) -> Value {
    let mut params = json!({"path": p.path, "scale": p.scale, "align": "center"});
    if p.svg.repair {
        params["repair_tolerance"] = json!(p.svg.tolerance);
    }
    params
}

fn diagnostic_reply(options: &mut SvgOptions, result: Result<Value, String>) {
    options.reply = None;
    match result {
        Ok(value) => {
            let parsed = value
                .get("diagnostics")
                .cloned()
                .ok_or_else(|| solidcraft_ui_text::tr("ui.svg_diagnostics_failed").to_string())
                .and_then(|v| serde_json::from_value::<Vec<Diagnostic>>(v).map_err(|e| e.to_string()));
            match parsed {
                Ok(diagnostics) => {
                    options.diagnostics = Arc::new(diagnostics);
                    options.selected = None;
                    options.error = None;
                }
                Err(error) => options.error = Some(error),
            }
        }
        Err(error) => options.error = Some(error),
    }
}

/// Check on a scratch session, keeping the file and command work off the native window thread.
fn refresh_diagnostics(app: &SolveApp, ui: &egui::Ui, p: &mut Place) {
    let params = diagnostic_params(p);
    if let Some(slot) = p.svg.reply.as_ref() {
        let reply = slot.lock().map(|mut slot| slot.take()).unwrap_or_else(|_| Some(Err(solidcraft_ui_text::tr("ui.svg_diagnostics_failed").to_string())));
        if let Some(reply) = reply {
            if p.svg.requested.as_ref() == Some(&params) {
                diagnostic_reply(&mut p.svg, reply);
            } else {
                // A placement edit overtook this reply. Only the newest parameters are shown.
                p.svg.reply = None;
            }
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    if p.svg.requested.as_ref() != Some(&params) {
        p.svg.diagnostics = Arc::new(Vec::new());
        p.svg.selected = None;
        p.svg.error = None;
        // Keep one worker per dialog; edits made during a check replace its next request.
        if p.svg.reply.is_some() {
            return;
        }
        p.svg.requested = Some(params.clone());
        #[cfg(not(target_arch = "wasm32"))]
        if app.apply_off_thread.is_some() {
            let slot: DiagnosticReply = Default::default();
            let out = slot.clone();
            let mut session = app.session.scratch();
            let given = params.clone();
            let ctx = ui.ctx().clone();
            let scope = solidcraft_engine::io::vfs::current_scope();
            let job = std::thread::Builder::new().name("SVG diagnostics".into()).spawn(move || {
                let reply =
                    solidcraft_engine::io::vfs::scoped(scope, || session.execute("sketch.svg_diagnostics", &given).map_err(|e| e.to_string()));
                if let Ok(mut slot) = out.lock() {
                    *slot = Some(reply);
                }
                ctx.request_repaint();
            });
            if job.is_ok() {
                p.svg.reply = Some(slot);
            } else {
                p.svg.error = Some(solidcraft_ui_text::tr("ui.svg_diagnostics_failed").to_string());
            }
        } else {
            diagnostic_reply(&mut p.svg, app.session.scratch().execute("sketch.svg_diagnostics", &params).map_err(|e| e.to_string()));
        }
        #[cfg(target_arch = "wasm32")]
        diagnostic_reply(&mut p.svg, app.session.scratch().execute("sketch.svg_diagnostics", &params).map_err(|e| e.to_string()));
    }
}

pub(crate) fn diagnostic_kind(kind: &str) -> &'static str {
    match kind {
        "attached_loop" => "ui.svg_diagnostic_attached_loop",
        "backtracking_curve" => "ui.svg_diagnostic_backtracking",
        "short_tangent" => "ui.svg_diagnostic_short_tangent",
        "closed_cubic" => "ui.svg_diagnostic_closed_cubic",
        "shared_boundary" => "ui.svg_diagnostic_shared_boundary",
        "unpainted_boundary" => "ui.svg_diagnostic_unpainted_boundary",
        "outline_approximation" => "ui.svg_diagnostic_outline_approximation",
        _ => "ui.svg_diagnostic_other",
    }
}

fn deviation_text(bound: f64) -> String {
    if bound > 0.0 && bound < 1e-4 { format!("{bound:.3e}") } else { format!("{bound:.6}") }
}

#[derive(Clone, Debug)]
struct OutlineGroup {
    path: usize,
    subpath: usize,
    repaired: bool,
    segments: Vec<usize>,
}

fn outline_groups(diagnostics: &[Diagnostic]) -> Vec<OutlineGroup> {
    let mut grouped = BTreeMap::<(bool, usize, usize), Vec<usize>>::new();
    for (index, issue) in diagnostics.iter().enumerate() {
        grouped.entry((issue.repaired, issue.path, issue.subpath)).or_default().push(index);
    }
    grouped.into_iter().map(|((repaired, path, subpath), segments)| OutlineGroup { path, subpath, repaired, segments }).collect()
}

fn use_original(p: &mut Place) {
    p.svg.repair = false;
    p.svg.original_view = false;
    p.svg.review = false;
    p.svg.repair_open = false;
    p.svg.selected = None;
    p.svg.show_changes = false;
}

fn preview_repair(p: &mut Place) {
    p.svg.repair = true;
    p.svg.tolerance = p.svg.draft_tolerance;
    p.svg.original_view = false;
    p.svg.repair_open = true;
    p.svg.review = false;
    p.svg.selected = None;
    p.svg.show_changes = false;
}

fn publish_control(ui: &egui::Ui, handle: &str, response: &egui::Response) {
    solidcraft_ui_registry::publish_rect(handle, response.rect);
    if response.enabled() && ui.is_rect_visible(response.rect) && ui.clip_rect().contains_rect(response.rect) {
        solidcraft_ui_registry::publish_handle(handle, response.rect.center());
    }
}

fn svg_heading(ui: &mut egui::Ui, title: &str) {
    crate::ui_kit::section_heading_with_size(ui, title, solidcraft_theme::Tokens::get().font_small);
}

fn status_pill(ui: &mut egui::Ui, label: &str, tone: crate::ui_kit::AssistantTone) -> egui::Response {
    let t = solidcraft_theme::Tokens::get();
    crate::ui_kit::assistant_badge_with_metrics(ui, label, tone, crate::ui_kit::BadgeMetrics::compact(&t))
}

fn sized_button(ui: &mut egui::Ui, label: &str, style: crate::ui_kit::Button) -> egui::Response {
    let t = solidcraft_theme::Tokens::get();
    let before = ui.style().clone();
    ui.spacing_mut().interact_size.y = t.form_button_height;
    ui.spacing_mut().button_padding = egui::vec2(t.space_10, t.space_3);
    let widgets = &mut ui.style_mut().visuals.widgets;
    for visual in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
        visual.corner_radius = egui::CornerRadius::same(t.control_radius);
    }
    let text = egui::RichText::new(label).font(egui::FontId::proportional(t.font_label));
    let response = crate::ui_kit::button(ui, text, style);
    ui.set_style(before);
    response
}

fn sized_number(ui: &mut egui::Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64, suffix: &str, width: f32) -> egui::Response {
    let t = solidcraft_theme::Tokens::get();
    ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
        ui.spacing_mut().interact_size = egui::vec2(width, t.control_height);
        ui.spacing_mut().text_edit_width = width;
        ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
        ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(t.font_body));
        crate::ui_kit::number_field(ui, value, range, speed, suffix)
    })
    .inner
}

fn publish_button(ui: &mut egui::Ui, label: &str, handle: &str) -> bool {
    let response = sized_button(ui, label, crate::ui_kit::Button::Normal);
    publish_control(ui, handle, &response);
    response.clicked()
}

fn region_count(app: &SolveApp, p: &Place) -> Option<usize> {
    commands(p)
        .is_ok_and(|commands| crate::preview::matches_commands(app, &commands))
        .then(|| app.preview.sketches.iter().map(|preview| preview.sketch.profiles.len()).sum())
}

fn tone_colours(t: &solidcraft_theme::Tokens, tone: crate::ui_kit::AssistantTone) -> (egui::Color32, egui::Color32) {
    match tone {
        crate::ui_kit::AssistantTone::Success => (t.form_success, t.form_success_fill),
        crate::ui_kit::AssistantTone::Warning => (t.warning, t.warning.gamma_multiply(0.12)),
        crate::ui_kit::AssistantTone::Error => (t.form_error, t.form_error_fill),
        crate::ui_kit::AssistantTone::Working => (t.selected_ink, t.accent_soft),
        crate::ui_kit::AssistantTone::Neutral => (t.form_dim, t.dialog_footer),
    }
}

/// A small secondary button (24 high) for the actions under a readiness step.
fn small_button(ui: &mut egui::Ui, label: &str, handle: &str) -> bool {
    small_button_tip(ui, label, None, handle)
}

fn small_button_tip(ui: &mut egui::Ui, label: &str, tip: Option<&str>, handle: &str) -> bool {
    let t = solidcraft_theme::Tokens::get();
    let before = ui.style().clone();
    ui.spacing_mut().interact_size.y = t.space_24;
    ui.spacing_mut().button_padding = egui::vec2(t.space_8, t.space_2);
    let widgets = &mut ui.style_mut().visuals.widgets;
    for visual in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
        visual.corner_radius = egui::CornerRadius::same(t.control_radius);
    }
    let text = egui::RichText::new(label).font(egui::FontId::proportional(t.font_small));
    let response = crate::ui_kit::button(ui, text, crate::ui_kit::Button::Normal);
    ui.set_style(before);
    publish_control(ui, handle, &response);
    if let Some(tip) = tip {
        response.clone().backing_on_hover_text(tip);
    }
    response.clicked()
}

/// A tinted notice: icon, a title in the tone's ink and a line of detail, then optional actions.
fn svg_banner(ui: &mut egui::Ui, tone: crate::ui_kit::AssistantTone, glyph: &str, title: &str, body: &str, actions: impl FnOnce(&mut egui::Ui)) {
    let t = solidcraft_theme::Tokens::get();
    let (ink, fill) = tone_colours(&t, tone);
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(t.radius_md))
        .inner_margin(egui::Margin::symmetric(t.space_12 as i8, t.space_10 as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(egui::RichText::new(glyph).color(ink).font(crate::ui_kit::type_face::semibold(t.font_label)));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = t.space_2;
                    ui.add(egui::Label::new(egui::RichText::new(title).color(ink).font(crate::ui_kit::type_face::semibold(t.font_label))).wrap());
                    if !body.is_empty() {
                        crate::ui_kit::text(ui, body, crate::ui_kit::Text::Hint("svg_banner"));
                    }
                    actions(ui);
                });
            });
        });
}

fn svg_file_card(app: &mut SolveApp, ui: &mut egui::Ui, p: &Place) {
    let t = solidcraft_theme::Tokens::get();
    svg_heading(ui, app.tr("ui.svg_file_section"));
    let name = std::path::Path::new(&p.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.path.clone());
    let destination = app.session.active_sketch.and_then(|id| app.session.doc.feature(id)).map(|feature| feature.name.clone());
    let (value, tip) = match &destination {
        Some(name) => (
            solidcraft_ui_text::format("ui.svg_destination_active", &[("name", name)]),
            solidcraft_ui_text::format("ui.svg_destination_active_note", &[("name", name)]),
        ),
        None => (app.tr("ui.svg_destination_xy").to_string(), app.tr("ui.svg_destination_new_note").to_string()),
    };
    let mut change = false;
    crate::ui_kit::settings_card_with_inset(ui, egui::vec2(t.space_12, t.space_6), |ui| {
        let top = ui.horizontal_top(|ui| {
            let side = t.space_24 + t.space_8;
            let (icon, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            ui.painter().rect_filled(icon, t.radius_md, t.dialog_footer);
            crate::icons::paint_icon(ui.painter(), egui::Rect::from_center_size(icon.center(), egui::Vec2::splat(t.space_16)), "sketch_svg", 1.0);
            let text_width = (ui.available_width() - t.space_24 * 3.0 - t.space_8).max(t.space_24);
            ui.allocate_ui_with_layout(egui::vec2(text_width, side), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(egui::Label::new(egui::RichText::new(&name).font(crate::ui_kit::type_face::semibold(t.font_label))).truncate());
                ui.add(egui::Label::new(egui::RichText::new(&p.path).monospace().size(t.font_small).color(t.form_dim)).truncate())
                    .backing_on_hover_text(&p.path);
            });
            change = small_button(ui, app.tr("ui.svg_change_file"), "dialog:insert:svg:file");
        });
        solidcraft_ui_registry::publish_rect("svg:metrics:file", top.response.rect);
        ui.add(egui::Separator::default().spacing(t.space_6));
        let row = ui.horizontal(|ui| {
            ui.label(egui::RichText::new(app.tr("ui.svg_destination")).size(t.font_small).color(t.form_dim));
            let (info, response) = ui.allocate_exact_size(egui::Vec2::splat(t.space_14), egui::Sense::hover());
            ui.painter().circle_stroke(info.center(), info.width() * 0.5 - 1.0, egui::Stroke::new(1.0, t.form_dim));
            ui.painter().text(info.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(t.font_small - 2.0), t.form_dim);
            response.backing_on_hover_text(&tip);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&value).size(t.font_small).color(t.form_ink)).truncate());
            });
        });
        solidcraft_ui_registry::publish_rect("svg:metrics:destination", row.response.rect);
    });
    if change {
        let command = p.command.clone();
        app.pick_open_for(&["svg"], move |app, path| {
            if let Some(path) = path {
                open(app, &command, path);
            }
        });
    }
}

fn size_field(ui: &mut egui::Ui, p: &mut Place, natural: f64, width: bool, field_width: f32) {
    let mut mm = natural * p.scale;
    let response = sized_number(ui, &mut mm, 0.001..=natural * MAX_SCALE, 0.5, " mm", field_width);
    if response.changed() && mm.is_finite() && mm > 0.0 {
        p.scale = (mm / natural).clamp(0.0001, MAX_SCALE);
    }
    publish_control(ui, if width { "dialog:insert:width" } else { "dialog:insert:height" }, &response);
}

fn svg_size(app: &SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    let t = solidcraft_theme::Tokens::get();
    svg_heading(ui, app.tr("ui.svg_size_section"));
    if let Some((width, height)) = p.natural {
        let lock = t.space_24;
        let field_width = ((ui.available_width() - lock - t.space_8 * 2.0) / 2.0).max(t.space_24 * 3.0).floor();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            for (is_width, natural, label) in [(true, width, "Width"), (false, height, "Height")] {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = t.space_2;
                    ui.label(egui::RichText::new(solidcraft_ui_text::tr_label(label)).size(t.font_small).color(t.form_dim));
                    size_field(ui, p, natural, is_width, field_width);
                });
                if is_width {
                    ui.vertical(|ui| {
                        ui.add_space(t.space_16 + t.space_2);
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(lock, t.control_height), egui::Sense::hover());
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "↔", egui::FontId::proportional(t.font_label), t.form_dim);
                        response.backing_on_hover_text(app.tr("ui.svg_aspect_locked"));
                    });
                }
            }
        });
    }
    let summary = solidcraft_ui_text::format(
        "ui.svg_placement_summary",
        &[("x", &format!("{:.2}", p.x)), ("y", &format!("{:.2}", p.y)), ("angle", &format!("{:.1}", p.angle)), ("scale", &format!("{:.2}", p.scale))],
    );
    crate::ui_kit::quiet_disclosure(ui, "svg_placement", &summary, false, false, |ui| {
        for (label, value, handle) in [("X", &mut p.x, "dialog:insert:x"), ("Y", &mut p.y, "dialog:insert:y")] {
            form_row(ui, label, |ui| {
                let r = crate::ui_kit::number_field(ui, value, -1e9..=1e9, 0.5, " mm");
                publish_control(ui, handle, &r);
            });
        }
        form_row(ui, "Angle", |ui| {
            let r = crate::ui_kit::number_field(ui, &mut p.angle, -360.0..=360.0, 1.0, "\u{b0}");
            publish_control(ui, "dialog:insert:angle", &r);
        });
        form_row(ui, "Scale", |ui| {
            let r = crate::ui_kit::number_field(ui, &mut p.scale, 0.0001..=MAX_SCALE, 0.01, "");
            publish_control(ui, "dialog:insert:scale", &r);
        });
    });
    if let Some(rect) = crate::scenario::rect_of(&format!("dialog:section:{summary}")) {
        solidcraft_ui_registry::publish_rect("dialog:insert:placement_summary", rect);
        if ui.is_rect_visible(rect) && ui.clip_rect().contains_rect(rect) {
            solidcraft_ui_registry::publish_handle("dialog:insert:placement_summary", rect.center());
        }
    }
}

/// One step of the readiness stepper: a badge on a rail, the title with its status pill, a summary
/// and the step's own actions.
#[allow(clippy::too_many_arguments)]
fn ladder_row(
    ui: &mut egui::Ui,
    title: &str,
    state: &str,
    summary: &str,
    tone: crate::ui_kit::AssistantTone,
    glyph: &str,
    part: &str,
    last: bool,
    with_actions: bool,
    actions: impl FnOnce(&mut egui::Ui),
) {
    let t = solidcraft_theme::Tokens::get();
    let (ink, fill) = tone_colours(&t, tone);
    let rail = ui.painter().add(egui::Shape::Noop);
    let row = ui.horizontal_top(|ui| {
        let (badge, _) = ui.allocate_exact_size(egui::Vec2::splat(t.space_24), egui::Sense::hover());
        solidcraft_ui_registry::publish_rect(&format!("svg:metrics:badge:{part}"), badge);
        ui.painter().circle_filled(badge.center(), badge.width() * 0.5, fill);
        ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, glyph, crate::ui_kit::type_face::semibold(t.font_label), ink);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = t.space_2;
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).font(crate::ui_kit::type_face::semibold(t.font_label))).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let pill = status_pill(ui, state, tone);
                    solidcraft_ui_registry::publish_rect(&format!("svg:metrics:pill:{part}"), pill.rect);
                });
            });
            crate::ui_kit::text(ui, summary, crate::ui_kit::Text::Hint("svg_readiness_row"));
            if with_actions {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = t.space_6;
                    actions(ui);
                });
            }
        });
        badge
    });
    if !last {
        let badge = row.inner;
        ui.painter().set(
            rail,
            egui::Shape::line_segment(
                [egui::pos2(badge.center().x, badge.bottom() + t.space_3), egui::pos2(badge.center().x, row.response.rect.bottom() + t.space_10)],
                egui::Stroke::new(t.space_2, t.dialog_rule),
            ),
        );
        ui.add_space(t.space_10);
    }
}

fn readiness_ladder(app: &SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    use crate::ui_kit::AssistantTone as Tone;
    let checking = p.svg.reply.is_some() || p.svg.requested.as_ref() != Some(&diagnostic_params(p));
    let outlines = p.svg.diagnostics.iter().map(|d| (d.path, d.subpath)).collect::<BTreeSet<_>>().len();
    let adjusted = p.svg.diagnostics.iter().filter(|d| d.repaired).map(|d| (d.path, d.subpath)).collect::<BTreeSet<_>>().len();
    let remaining = p.svg.diagnostics.iter().filter(|d| !d.repaired).count();
    let summary = if p.svg.repair {
        solidcraft_ui_text::format("ui.svg_curves_adjusted", &[("outlines", &adjusted.to_string()), ("issues", &remaining.to_string())])
    } else {
        solidcraft_ui_text::format("ui.svg_curves_unchanged", &[("issues", &p.svg.diagnostics.len().to_string()), ("outlines", &outlines.to_string())])
    };
    solidcraft_ui_registry::publish_count("svg_diagnostics", p.svg.diagnostics.len() as f64);
    solidcraft_ui_registry::publish_count("svg_diagnostic_outlines", outlines as f64);
    solidcraft_ui_registry::publish_count("svg_readiness_steps", 3.0);
    solidcraft_ui_registry::publish_count("svg_extrude_checked", 0.0);
    svg_heading(ui, app.tr("ui.svg_readiness"));
    let top = ui.cursor().top();
    let (state, tone, glyph) = if checking {
        ("ui.svg_checking", Tone::Working, "…")
    } else if p.svg.error.is_some() {
        ("ui.svg_failed_title", Tone::Error, "×")
    } else if p.svg.repair && adjusted > 0 {
        ("ui.svg_adjusted", if remaining > 0 { Tone::Warning } else { Tone::Success }, "✓")
    } else {
        ("ui.svg_checked", if remaining > 0 { Tone::Warning } else { Tone::Neutral }, if remaining > 0 { "!" } else { "—" })
    };
    let curves_summary = if checking { app.tr("ui.svg_checking_curves").to_string() } else { summary };
    let (review_label, repair_handle) = (
        app.tr(if p.svg.repair { "ui.svg_review_changes" } else { "ui.svg_review_issues" }),
        if repair_open(p) { "dialog:insert:svg:repair_open" } else { "dialog:insert:svg:repair" },
    );
    let can_review = !checking && !p.svg.diagnostics.is_empty();
    ladder_row(ui, app.tr("ui.svg_step_curves"), app.tr(state), &curves_summary, tone, glyph, "curves", false, true, |ui| {
        if can_review && small_button(ui, review_label, "dialog:insert:svg:review") {
            p.svg.review = true;
            p.svg.selected = None;
        }
        if small_button(ui, app.tr("ui.svg_repair_action"), repair_handle) {
            p.svg.repair_open = true;
        }
    });
    if let Some(count) = region_count(app, p) {
        let state = solidcraft_ui_text::format("ui.svg_regions_count", &[("count", &count.to_string())]);
        let summary = solidcraft_ui_text::format("ui.svg_regions_detected", &[("count", &count.to_string())]);
        ladder_row(ui, app.tr("ui.svg_step_regions"), &state, &summary, Tone::Neutral, "—", "regions", false, false, |_| {});
        solidcraft_ui_registry::publish_count("svg_readiness_regions", count as f64);
    } else {
        ladder_row(
            ui,
            app.tr("ui.svg_step_regions"),
            app.tr("ui.svg_not_checked"),
            app.tr("ui.svg_faces_checking"),
            Tone::Working,
            "…",
            "regions",
            false,
            false,
            |_| {},
        );
    }
    ladder_row(
        ui,
        app.tr("ui.svg_step_extrude"),
        app.tr("ui.svg_not_checked"),
        app.tr("ui.svg_extrusion_not_checked"),
        Tone::Neutral,
        "—",
        "extrude",
        true,
        false,
        |_| {},
    );
    let bottom = ui.cursor().top();
    solidcraft_ui_registry::publish_rect(
        "svg:metrics:readiness",
        egui::Rect::from_min_max(egui::pos2(ui.max_rect().left(), top), egui::pos2(ui.max_rect().right(), bottom)),
    );
}

fn publish_action(ui: &mut egui::Ui, label: &str, handle: &str) -> bool {
    let response = sized_button(ui, label, crate::ui_kit::Button::Link);
    publish_control(ui, handle, &response);
    response.clicked()
}

fn display_toggle(
    ui: &mut egui::Ui,
    key: &str,
    handle: &str,
    label: &str,
    count: &str,
    on: &mut bool,
    swatch: egui::Color32,
    shape: crate::ui_kit::ChoiceChipSwatchShape,
    enabled: bool,
) {
    let response = crate::ui_kit::swatch_checkbox(ui, label, count, on, swatch, shape, enabled);
    solidcraft_ui_registry::publish_rect(&format!("svg:metrics:chip:{key}"), response.rect);
    publish_control(ui, handle, &response);
}

fn viewport_chips(app: &SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    use crate::ui_kit::ChoiceChipSwatchShape as Shape;
    let t = solidcraft_theme::Tokens::get();
    svg_heading(ui, app.tr("ui.svg_show_viewport"));
    let regions = region_count(app, p).map(|count| count.to_string()).unwrap_or_else(|| "…".to_string());
    let diagnostics_current = p.svg.reply.is_none() && p.svg.error.is_none() && p.svg.requested.as_ref() == Some(&diagnostic_params(p));
    let problems = if diagnostics_current { p.svg.diagnostics.iter().filter(|d| !d.repaired).count().to_string() } else { "…".to_string() };
    let enabled = !p.svg.review && !repair_open(p);
    let gap = t.space_12;
    let column = ((ui.available_width() - gap) / 2.0).floor();
    let height = t.control_height;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.allocate_ui_with_layout(egui::vec2(column, height * 2.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            display_toggle(
                ui,
                "curves",
                "dialog:insert:svg:curves",
                app.tr("ui.svg_chip_curves"),
                "",
                &mut p.svg.show_curves,
                t.svg_curve,
                Shape::Line,
                true,
            );
            display_toggle(
                ui,
                "regions",
                "dialog:insert:svg:faces",
                app.tr("ui.svg_toggle_regions"),
                &regions,
                &mut p.svg.show_faces,
                t.svg_region_fill.to_opaque(),
                Shape::Square,
                true,
            );
        });
        ui.allocate_ui_with_layout(egui::vec2(column, height * 2.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            display_toggle(
                ui,
                "problems",
                "dialog:insert:svg:changes",
                app.tr("ui.svg_toggle_problems"),
                &problems,
                &mut p.svg.show_changes,
                t.warning,
                Shape::Dot,
                true,
            );
            display_toggle(
                ui,
                "placement",
                "dialog:insert:svg:placement",
                app.tr("ui.svg_chip_placement"),
                "",
                &mut p.svg.show_placement,
                t.svg_repair_segment,
                Shape::Square,
                enabled,
            );
        });
    });
}

fn state_banner(app: &mut SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    use crate::ui_kit::AssistantTone as Tone;
    if let Some(reason) = p.svg.error.clone() {
        let mut retry = false;
        svg_banner(ui, Tone::Error, "×", app.tr("ui.svg_failed_title"), &reason, |ui| {
            ui.horizontal_wrapped(|ui| {
                if small_button(ui, app.tr("ui.svg_try_default"), "dialog:insert:svg:try_default") {
                    p.svg.draft_tolerance = 0.01;
                    preview_repair(p);
                    retry = true;
                }
                if small_button(ui, app.tr("ui.svg_retry"), "dialog:insert:svg:retry") {
                    retry = true;
                }
            });
        });
        ui.add_space(t_gap());
        if retry {
            p.svg.requested = None;
            app.preview.retry_svg();
        }
    } else if is_stale(p) {
        svg_banner(ui, Tone::Warning, "!", app.tr("ui.svg_stale_title"), app.tr("ui.svg_stale_detail"), |_| {});
        ui.add_space(t_gap());
    } else if p.svg.reply.is_some() {
        svg_banner(ui, Tone::Working, "…", app.tr("ui.svg_checking_curves"), "", |_| {});
        ui.add_space(t_gap());
    }
}

/// The space between the panel's sections.
fn t_gap() -> f32 {
    solidcraft_theme::Tokens::get().space_8
}

fn proposal_comparison(app: &SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    if !p.svg.repair {
        return;
    }
    svg_heading(ui, app.tr("ui.svg_compare_section"));
    crate::ui_kit::row(ui, "", |ui| {
        let mut selected = usize::from(!p.svg.original_view);
        if crate::ui_kit::segmented(ui, &mut selected, &[app.tr("ui.svg_before"), app.tr("ui.svg_after")], Default::default()) {
            p.svg.original_view = selected == 0;
        }
    });
    crate::ui_kit::text(ui, app.tr("ui.svg_compare_coupled_note"), crate::ui_kit::Text::Note("svg_compare"));
}

fn svg_rows(app: &mut SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    refresh_diagnostics(app, ui, p);
    if p.svg.review {
        if publish_action(ui, app.tr(if p.svg.repair { "ui.svg_back_setup" } else { "ui.svg_back_to_import" }), "dialog:insert:svg:back") {
            p.svg.review = false;
            p.svg.selected = None;
            return;
        }
        proposal_comparison(app, ui, p);
        state_banner(app, ui, p);
        review_rows(app, ui, p);
        viewport_chips(app, ui, p);
        return;
    }
    if repair_open(p) {
        // While repairing, the file and size shrink to one summary line (reference board 3) so
        // the readiness ladder, tolerance and comparison stay in view. Expanding it brings the
        // full setup back, and nothing about the placement changes either way.
        let name = std::path::Path::new(&p.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.path.clone());
        let size = p.natural.map(|(w, h)| format!(" · {:.1} × {:.1} mm", w * p.scale, h * p.scale)).unwrap_or_default();
        let summary = format!("{name}{size}");
        crate::ui_kit::quiet_disclosure(ui, "svg_setup_collapsed", &summary, true, false, |ui| {
            svg_file_card(app, ui, p);
            svg_size(app, ui, p);
        });
        if let Some(rect) = crate::scenario::rect_of(&format!("dialog:section:{summary}")) {
            solidcraft_ui_registry::publish_rect("svg:metrics:setup_summary", rect);
        }
    } else {
        svg_file_card(app, ui, p);
        ui.add_space(t_gap());
        svg_size(app, ui, p);
    }
    ui.add_space(t_gap());
    readiness_ladder(app, ui, p);
    ui.add_space(t_gap());
    state_banner(app, ui, p);
    if repair_open(p) {
        let t = solidcraft_theme::Tokens::get();
        svg_heading(ui, app.tr("ui.svg_repair_section"));
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = t.space_2;
                ui.label(egui::RichText::new(app.tr("ui.svg_max_change")).size(t.font_small).color(t.form_dim));
                let width = t.space_24 * 5.0 + t.space_8;
                let response = sized_number(ui, &mut p.svg.draft_tolerance, 1e-9..=10.0, 0.001, " mm", width);
                publish_control(ui, "dialog:insert:svg:tolerance", &response);
            });
            ui.vertical(|ui| {
                ui.add_space(t.space_16 + t.space_2);
                if publish_button(ui, app.tr("ui.svg_preview_repair"), "dialog:insert:svg:repair") {
                    preview_repair(p);
                }
            });
        });
        if p.svg.repair && preview_ready(p) && region_count(app, p).is_some() {
            let text = solidcraft_ui_text::format("ui.svg_preview_up_to_date", &[("tolerance", &p.svg.tolerance.to_string())]);
            ui.add_space(t.space_6);
            ui.label(egui::RichText::new(format!("✓ {text}")).size(t.font_small).color(t.form_success));
        }
        crate::ui_kit::section(ui, "svg_repair_details", app.tr("ui.svg_technical_details"), false, |ui| {
            crate::ui_kit::text(ui, app.tr("ui.svg_repair_explanation"), crate::ui_kit::Text::Hint("svg_repair"));
        });
    }
    proposal_comparison(app, ui, p);
    if p.svg.repair && publish_action(ui, app.tr("ui.svg_use_original"), "dialog:insert:svg:original") {
        use_original(p);
    }
    viewport_chips(app, ui, p);
}

/// One outline in the review list: its name, how many changes it has, and its status.
fn outline_row(ui: &mut egui::Ui, name: &str, count: &str, state: &str, tone: crate::ui_kit::AssistantTone, selected: bool) -> egui::Response {
    let t = solidcraft_theme::Tokens::get();
    let height = t.space_24 * 2.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    if selected {
        ui.painter().rect(rect, t.control_radius, t.accent_soft, egui::Stroke::new(1.0, t.form_primary), egui::StrokeKind::Inside);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    let mut inner = ui.new_child(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(t.space_10, t.space_4))).layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    inner.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(egui::Label::new(egui::RichText::new(name).font(crate::ui_kit::type_face::semibold(t.font_label))).truncate());
        ui.label(egui::RichText::new(count).size(t.font_small).color(t.form_dim));
    });
    inner.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        status_pill(ui, state, tone);
    });
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, ui.is_enabled(), selected, name));
    response
}

fn review_rows(app: &mut SolveApp, ui: &mut egui::Ui, p: &mut Place) {
    use crate::ui_kit::AssistantTone as Tone;
    let t = solidcraft_theme::Tokens::get();
    let groups = outline_groups(&p.svg.diagnostics);
    solidcraft_ui_registry::publish_count("svg_review_groups", groups.len() as f64);
    crate::ui_kit::text(ui, app.tr("ui.svg_review_guidance"), crate::ui_kit::Text::Hint("svg_review"));
    let mut rows = Vec::new();
    for repaired in [false, true] {
        let indices = groups.iter().enumerate().filter_map(|(index, group)| (group.repaired == repaired).then_some(index)).collect::<Vec<_>>();
        if !indices.is_empty() {
            rows.push((repaired, None));
            rows.extend(indices.into_iter().map(|index| (repaired, Some(index))));
        }
    }
    let row_height = t.space_24 * 2.0 + t.space_4;
    egui::ScrollArea::vertical().id_salt("svg_outline_review").max_height(row_height * 4.5).show_rows(ui, row_height, rows.len(), |ui, range| {
        for row in range {
            let Some((repaired, index)) = rows.get(row) else { continue };
            let Some(index) = *index else {
                let count = groups.iter().filter(|group| group.repaired == *repaired).count();
                let heading = solidcraft_ui_text::format(
                    if *repaired { "ui.svg_review_adjusted" } else { "ui.svg_review_unresolved" },
                    &[("count", &count.to_string())],
                );
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::hover());
                let ink = if *repaired { t.form_dim } else { t.warning };
                ui.painter().text(
                    egui::pos2(rect.left() + t.space_10, rect.bottom() - t.space_6),
                    egui::Align2::LEFT_BOTTOM,
                    heading.to_uppercase(),
                    crate::ui_kit::type_face::semibold(t.font_small),
                    ink,
                );
                continue;
            };
            let Some(group) = groups.get(index) else { continue };
            let name = solidcraft_ui_text::format("ui.svg_review_outline", &[("path", &group.path.to_string()), ("subpath", &group.subpath.to_string())]);
            let count = solidcraft_ui_text::format("ui.svg_review_change_count", &[("count", &group.segments.len().to_string())]);
            let selected = p.svg.selected.is_some_and(|selected| group.segments.contains(&selected));
            let (state, tone) =
                if group.repaired { (app.tr("ui.svg_adjusted"), Tone::Success) } else { (app.tr("ui.svg_unresolved"), Tone::Warning) };
            let r = outline_row(ui, &name, &count, state, tone, selected);
            publish_control(ui, &format!("dialog:insert:svg:group:{index}"), &r);
            if let Some(first) = group.segments.first() {
                publish_control(ui, &format!("dialog:insert:svg:issue:{first}"), &r);
            }
            if r.clicked() {
                p.svg.selected = group.segments.first().copied();
            }
        }
    });
    let Some(index) = p.svg.selected else { return };
    let Some(group) = groups.iter().find(|g| g.segments.contains(&index)) else { return };
    let position = group.segments.iter().position(|i| *i == index).unwrap_or(0);
    let mut next = index;
    ui.add_space(t_gap());
    ui.horizontal(|ui| {
        let shown =
            solidcraft_ui_text::format("ui.svg_review_segment", &[("position", &(position + 1).to_string()), ("count", &group.segments.len().to_string())]);
        ui.label(egui::RichText::new(shown).font(crate::ui_kit::type_face::semibold(t.font_label)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_4;
            ui.add_enabled_ui(position + 1 < group.segments.len(), |ui| {
                if small_button_tip(ui, "›", Some(app.tr("ui.svg_next")), "dialog:insert:svg:next") {
                    next = group.segments.get(position + 1).copied().unwrap_or(index);
                }
            });
            ui.add_enabled_ui(position > 0, |ui| {
                if small_button_tip(ui, "‹", Some(app.tr("ui.svg_previous")), "dialog:insert:svg:previous") {
                    next = group.segments.get(position.saturating_sub(1)).copied().unwrap_or(index);
                }
            });
        });
    });
    p.svg.selected = Some(next);
    let Some(issue) = p.svg.diagnostics.get(next) else { return };
    ui.label(egui::RichText::new(app.tr(diagnostic_kind(&issue.kind))).font(crate::ui_kit::type_face::semibold(t.font_label)));
    if let Some(bound) = issue.max_deviation_mm {
        let bound = solidcraft_ui_text::format(
            "ui.svg_review_move",
            &[("deviation", &format!("≤ {}", deviation_text(bound))), ("tolerance", &p.svg.tolerance.to_string())],
        );
        crate::ui_kit::text(ui, &bound, crate::ui_kit::Text::Note("svg_bound"));
        let count = issue.replacement_highlight.iter().map(|chain| chain.len().saturating_sub(1)).sum::<usize>();
        let detail = if count == 0 {
            app.tr("ui.svg_review_removed").to_string()
        } else {
            solidcraft_ui_text::format("ui.svg_review_replaced_by", &[("count", &count.to_string())])
        };
        crate::ui_kit::text(ui, &detail, crate::ui_kit::Text::Note("svg_replacement"));
    }
    ui.add_space(t.space_6);
    if small_button(ui, app.tr("ui.svg_locate_segment"), "dialog:insert:svg:locate") {
        locate_diagnostic(app, p, issue);
    }
    crate::ui_kit::text(ui, app.tr("ui.svg_change_legend"), crate::ui_kit::Text::Hint("svg_legend"));
    crate::ui_kit::section(ui, "svg_technical", app.tr("ui.svg_technical_details"), false, |ui| {
        let location = solidcraft_ui_text::format(
            "ui.svg_diagnostic_location",
            &[
                ("path", &issue.path.to_string()),
                ("subpath", &issue.subpath.to_string()),
                ("segment", &issue.segment.to_string()),
                ("kind", app.tr(diagnostic_kind(&issue.kind))),
            ],
        );
        crate::ui_kit::text(ui, &location, crate::ui_kit::Text::Hint("svg_issue"));
        if issue.repaired && issue.replacement_highlight.is_empty() {
            crate::ui_kit::text(ui, app.tr("ui.svg_segment_removed"), crate::ui_kit::Text::Note("svg_removed"));
        }
    });
}

fn locate_diagnostic(app: &mut SolveApp, place: &Place, issue: &Diagnostic) {
    let plane = crate::insert_handles::placement_plane(app);
    let mut bounds = solidcraft_engine::geom::Aabb3::EMPTY;
    for point in std::iter::once(&issue.at).chain(&issue.highlight).chain(issue.replacement_highlight.iter().flatten()) {
        if let Some(point) = crate::insert_handles::diagnostic_world(place, &plane, *point) {
            bounds.add(point);
        }
    }
    if !bounds.is_empty() {
        let margin = bounds.diagonal().max(place.svg.tolerance) * 0.1;
        let camera = app.fitted_bounds(app.cam, &bounds.inflated(margin));
        app.animate_to(camera);
    }
}

/// The command OK runs (and the preview shows).
pub fn commands(p: &Place) -> Result<Vec<(String, Value)>, String> {
    if !(p.x.is_finite() && p.y.is_finite()) {
        return Err("the position must be a number".into());
    }
    if !(p.scale.is_finite() && p.scale > 1e-9 && p.scale <= MAX_SCALE) {
        return Err("the scale must be above 0".into());
    }
    if !p.angle.is_finite() {
        return Err("the angle must be a number".into());
    }
    let mut params = json!({"path": p.path, "at": [p.x, p.y], "scale": p.scale, "angle": p.angle, "align": "center"});
    if p.command.ends_with("svg") && repair_enabled(p) {
        if !(p.svg.tolerance.is_finite() && (1e-9..=10.0).contains(&p.svg.tolerance)) {
            return Err(solidcraft_ui_text::tr("ui.svg_repair_tolerance_invalid").to_string());
        }
        params["repair_tolerance"] = json!(p.svg.tolerance);
    }
    Ok(vec![(p.command.clone(), params)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    fn reveal_control(h: &mut Harness, handle: &str) {
        for _ in 0..32 {
            let rect = crate::scenario::rect_of(handle).unwrap_or_else(|| panic!("{handle} is not drawn"));
            let viewport = crate::scenario::rect_of("dialog:body:viewport").expect("dialog scroll body");
            if viewport.contains_rect(rect) && handle_at(handle).is_some() {
                return;
            }
            let point = egui::pos2(viewport.left() + viewport.width() * 0.4, viewport.center().y);
            h.call("ui.scroll", json!({"x":point.x,"y":point.y,"delta":if rect.top() < viewport.top() {100} else {-100}}));
            for _ in 0..120 {
                if !h.ctx.input(|input| input.is_scrolling()) {
                    break;
                }
                h.frame();
            }
            h.frames(2);
        }
        panic!("could not scroll {handle} into the dialog body");
    }

    fn click_control(h: &mut Harness, handle: &str) {
        reveal_control(h, handle);
        crate::test_support::click(h, handle);
    }

    fn svg_file(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-insert-place-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><rect x="0" y="0" width="20" height="10"/></svg>"#).unwrap();
        path.to_string_lossy().to_string()
    }

    fn lines(h: &Harness) -> Vec<(solidcore_record::Vec2, solidcore_record::Vec2)> {
        let st = h.app.session.world_state().unwrap();
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        (0..sk.curves.len())
            .filter_map(|i| match sk.shape(i) {
                Some(solidcraft_engine::sketch::Shape::Line { a, b }) => Some((a, b)),
                _ => None,
            })
            .collect()
    }

    /// Insert SVG asks for the file, then places it: X, Y and Scale move and size the drawing
    /// before OK adds it, in one undo step.
    #[test]
    fn a_picked_svg_is_placed_and_scaled_before_it_is_added() {
        let mut h = Harness::new();
        let path = svg_file("plate.svg");
        let picked = path.clone();
        h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(Some(picked.clone())))));
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        h.app.start("sketch.insert_svg");
        h.frames(3);
        let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no placement: {:?}", h.app.status) };
        assert_eq!((p.path.as_str(), p.x, p.y, p.scale, p.angle, p.natural), (path.as_str(), 0.0, 0.0, 1.0, 0.0, Some((20.0, 10.0))));
        assert!(handle_at("dialog:insert:width").is_some(), "the physical size is drawn");
        assert!(
            (handle_at("dialog:insert:width").unwrap().y - handle_at("dialog:insert:height").unwrap().y).abs() < 2.0,
            "linked size fields share one row"
        );
        assert!(lines(&h).is_empty(), "nothing is added before OK");
        // The drawing is previewed where it would go.
        h.step(&json!({"until": {"count": {"name": "sketch_preview_curves", "value": 4}}})).unwrap();
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (p.x, p.y, p.scale) = (5.0, -3.0, 2.0);
        }
        h.frames(3);
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let pts: Vec<_> = lines(&h).into_iter().flat_map(|(a, b)| [a, b]).collect();
        assert_eq!(pts.len(), 8, "the rectangle's four lines");
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.x), u.max(p.x)));
        let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.y), u.max(p.y)));
        assert!((hi_x - lo_x - 40.0).abs() < 1e-6 && (hi_y - lo_y - 20.0).abs() < 1e-6, "twice its size: {lo_x}..{hi_x} × {lo_y}..{hi_y}");
        assert!(((lo_x + hi_x) / 2.0 - 5.0).abs() < 1e-6, "its middle is at X: {lo_x}..{hi_x}");
        assert!(((lo_y + hi_y) / 2.0 + 3.0).abs() < 1e-6, "its middle is at Y: {lo_y}..{hi_y}");
        h.app.run("edit.undo", json!({})).unwrap();
        h.settle();
        assert!(lines(&h).is_empty(), "one undo removes the drawing");
    }

    /// The width in millimetres sets the scale, and the angle turns the drawing about its middle.
    #[test]
    fn a_width_and_an_angle_size_and_turn_the_drawing_about_its_middle() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("turned.svg"));
        h.frames(2);
        assert!(handle_at("dialog:insert:width").is_some(), "physical size is visible");
        assert!(handle_at("dialog:insert:angle").is_none(), "placement starts collapsed");
        click_control(&mut h, "dialog:insert:placement_summary");
        assert!(handle_at("dialog:insert:angle").is_some(), "rotation is available on request");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            let (nw, _) = p.natural.unwrap();
            // 40 mm wide, as the Width field sets it, turned a quarter, middle at (5, -3).
            p.scale = 40.0 / nw;
            (p.x, p.y, p.angle) = (5.0, -3.0, 90.0);
        }
        h.frames(3);
        h.step(&json!({"key": "Enter"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        let pts: Vec<_> = lines(&h).into_iter().flat_map(|(a, b)| [a, b]).collect();
        let (lo_x, hi_x) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.x), u.max(p.x)));
        let (lo_y, hi_y) = pts.iter().fold((f64::MAX, f64::MIN), |(l, u), p| (l.min(p.y), u.max(p.y)));
        assert!((hi_x - lo_x - 20.0).abs() < 1e-6 && (hi_y - lo_y - 40.0).abs() < 1e-6, "turned: {lo_x}..{hi_x} x {lo_y}..{hi_y}");
        assert!(((lo_x + hi_x) / 2.0 - 5.0).abs() < 1e-6 && ((lo_y + hi_y) / 2.0 + 3.0).abs() < 1e-6, "about its middle");
    }

    /// The drawing's handles in the viewport edit the same values: a corner scales about the
    /// opposite one, the box moves, and the knob turns.
    #[test]
    fn the_handles_in_the_viewport_scale_move_and_turn_the_drawing() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("handles.svg"));
        h.frames(60);
        h.app.cam.distance = 200.0;
        h.frames(4);
        let place = |h: &Harness| match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
            Some(Kind::InsertDrawing(p)) => p,
            other => panic!("no placement: {other:?}"),
        };
        // The corner at (+10, +5) dragged to (+30, +15): twice the size, the corner at (-10, -5) stays.
        h.step(&json!({"drag": [{"handle": "insert:handle:3"}, {"world": [30, 15, 0]}]})).unwrap();
        h.frames(3);
        let p = place(&h);
        assert!((p.scale - 2.0).abs() < 0.05, "scaled: {}", p.scale);
        assert!((p.x - 10.0).abs() < 0.5 && (p.y - 5.0).abs() < 0.5, "about the far corner: {}, {}", p.x, p.y);
        // The box dragged by its middle by (+5, -4).
        let (x, y) = (p.x, p.y);
        h.step(&json!({"drag": [{"world": [x, y, 0]}, {"world": [x + 5.0, y - 4.0, 0]}]})).unwrap();
        h.frames(3);
        let p = place(&h);
        assert!((p.x - x - 5.0).abs() < 0.5 && (p.y - y + 4.0).abs() < 0.5, "moved: {}, {}", p.x, p.y);
        // The knob above the box, dragged to the left of the middle: a quarter turn.
        h.step(&json!({"drag": [{"handle": "insert:handle:5"}, {"world": [p.x - 30.0, p.y, 0]}]})).unwrap();
        h.frames(3);
        assert!((place(&h).angle - 90.0).abs() < 2.0, "turned: {}", place(&h).angle);
    }

    /// Cancel adds nothing; a bad scale is refused.
    #[test]
    fn cancelling_the_placement_adds_nothing() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.settle();
        open(&mut h.app, "sketch.insert_svg", svg_file("cancel.svg"));
        h.frames(2);
        let bad = Place {
            command: "sketch.insert_svg".into(),
            path: "x.svg".into(),
            x: 0.0,
            y: 0.0,
            scale: 0.0,
            angle: 0.0,
            natural: None,
            svg: Default::default(),
        };
        assert!(commands(&bad).is_err());
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert!(lines(&h).is_empty(), "nothing added");
    }
    fn evidence(h: &mut Harness, name: &str) {
        let Ok(dir) = std::env::var("SOLIDCRAFT_SVG_DIAGNOSTICS_EVIDENCE") else { return };
        let ctx = crate::fonts::test_context();
        let mut textures = std::collections::HashMap::new();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        for frame in 0..6 {
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| h.app.ui(ui));
            crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
            output.textures_delta.clear();
            if frame == 5 {
                let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                std::fs::create_dir_all(&dir).unwrap();
                image.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
                let rects = crate::scenario::rects("")
                    .into_iter()
                    .filter(|(key, _)| {
                        key.starts_with("svg:metrics:")
                            || key.starts_with("dialog:insert:")
                            || key.starts_with("dialog:body:")
                            || matches!(
                                key.as_str(),
                                "dialog:window" | "dialog:header" | "dialog:footer" | "dialog:close" | "dialog:ok" | "dialog:cancel"
                            )
                    })
                    .map(|(key, rect)| (key, json!({"x":rect.left(),"y":rect.top(),"width":rect.width(),"height":rect.height()})))
                    .collect::<serde_json::Map<_, _>>();
                let t = solidcraft_theme::Tokens::get();
                let measurements = json!({"canvas":{"width":1440,"height":900,"pixels_per_point":output.pixels_per_point},
                    "fonts":{"heading":t.font_small,"button":t.font_label,"number":t.font_body,"chip":t.font_caption},"rects":rects});
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}.json")), serde_json::to_vec_pretty(&measurements).unwrap()).unwrap();
            }
        }
    }

    /// The SVG opt-in layout keeps intrinsic controls at the reference dimensions in both destinations.
    #[test]
    fn svg_panel_control_metrics_fit_the_reference_at_1440_by_900() {
        for active in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            if active {
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            }
            open(&mut h.app, "sketch.insert_svg", svg_file(if active { "metrics-active.svg" } else { "metrics-new.svg" }));
            let ctx = crate::fonts::test_context();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
            for frame in 0..8 {
                let mut output = ctx
                    .run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| h.app.ui(ui));
                output.textures_delta.clear();
            }
            let close = |actual: f32, expected: f32, key: &str| {
                assert!((actual - expected).abs() <= 1.0, "{key}: expected {expected}, actual {actual}, active sketch {active}");
            };
            close(crate::scenario::rect_of("dialog:header").unwrap().height(), 48.0, "header");
            close(crate::scenario::rect_of("dialog:footer").unwrap().height(), 60.0, "footer");
            close(crate::scenario::rect_of("dialog:close").unwrap().width(), 28.0, "close target");
            // 380 with the 1 px border on each side.
            close(crate::scenario::rect_of("dialog:outer").unwrap().width(), 378.0, "panel");
            let width = crate::scenario::rect_of("dialog:insert:width").unwrap();
            let height = crate::scenario::rect_of("dialog:insert:height").unwrap();
            close(width.width(), height.width(), "equal size fields");
            assert!(width.width() >= 84.0, "size fields share the panel width: {width:?}");
            for rect in [width, height] {
                close(rect.height(), 28.0, "size field");
            }
            for key in ["curves", "regions", "extrude"] {
                let badge = crate::scenario::rect_of(&format!("svg:metrics:badge:{key}")).unwrap();
                close(badge.width(), 24.0, key);
                close(badge.height(), 24.0, key);
                close(crate::scenario::rect_of(&format!("svg:metrics:pill:{key}")).unwrap().height(), 20.0, key);
            }
            for key in ["curves", "regions", "problems", "placement"] {
                close(crate::scenario::rect_of(&format!("svg:metrics:chip:{key}")).unwrap().height(), 28.0, key);
            }
            close(crate::scenario::rect_of("dialog:insert:svg:file").unwrap().height(), 24.0, "change file");
            let card = crate::scenario::rect_of("svg:metrics:file").unwrap();
            assert!((card.height() - 32.0).abs() <= 2.0, "file row height: {card:?}");
            assert!(crate::scenario::rect_of("svg:metrics:destination").is_some(), "the destination row stays in the file card");
            evidence(&mut h, if active { "svg-metrics-active" } else { "svg-metrics-new" });
        }
    }

    /// One status item at the top of the viewport covers every SVG integrity fact; ✕ hides it.
    #[test]
    fn svg_import_has_one_integrity_status_item_with_its_values_in_the_key() {
        let mut h = Harness::new();
        h.app.home.open = false;
        open(&mut h.app, "sketch.insert_svg", svg_file("integrity.svg"));
        h.frames(60);
        assert_eq!(crate::scenario::count_of("viewport_status:count"), Some(1.0), "one pill, not one per fact");
        let facts = integrity(&h.app).expect("integrity facts");
        assert_eq!(facts.rows.len(), 4, "curves, repair, regions, extrusion");
        assert!(crate::scenario::rect_of("viewport_status:svg_integrity:item").is_some());
        crate::test_support::click(&mut h, "viewport_status:svg_integrity:key");
        h.frames(4);
        assert!(crate::scenario::rect_of("viewport_status:card").is_some(), "the key opens the card with the values");
        hide_integrity(&mut h.app);
        h.frames(3);
        assert!(integrity(&h.app).is_none());
    }

    /// Inspect and repair use the same placement, and neither is applied before OK.
    #[test]
    fn svg_issues_are_located_and_repair_is_explicit_and_bounded() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        h.settle();
        h.app.cam.distance = 10.0;
        let path = svg_file("backtracking.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0 C-0.1 0 1 1 2 0 L2 2 L0 2 Z"/></svg>"#).unwrap();
        let before = h.app.session.doc.clone();
        open(&mut h.app, "sketch.insert_svg", path);
        h.frames(3);
        let place = |h: &Harness| match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
            Some(Kind::InsertDrawing(p)) => p,
            other => panic!("no SVG placement: {other:?}"),
        };
        let initial = place(&h);
        evidence(&mut h, "svg-diagnostics-original");
        assert!(!initial.svg.repair);
        assert_eq!(initial.svg.diagnostics.len(), 1);
        assert_eq!(initial.svg.diagnostics[0].kind, "backtracking_curve");
        assert_eq!((initial.svg.diagnostics[0].path, initial.svg.diagnostics[0].subpath, initial.svg.diagnostics[0].segment), (1, 1, 1));
        assert!(handle_at("dialog:insert:svg:repair").is_some());
        assert!(initial.svg.selected.is_none());
        assert!(!initial.svg.review && !initial.svg.show_changes);
        assert!(handle_at("dialog:insert:svg:issue:0").is_none(), "review details stay out of the initial import");
        assert!(handle_at("insert:svg:highlight:0").is_none(), "the initial preview is clean");
        assert_eq!(crate::scenario::count_of("sketch_preview_point_markers"), Some(0.0));
        assert!(preview_ready(&initial));
        assert!(commands(&initial).unwrap()[0].1.get("repair_tolerance").is_none(), "original curves are the default");
        assert!(std::sync::Arc::ptr_eq(&before, &h.app.session.doc), "diagnosing never changes the design");
        // Moving/turning repositions the highlight; it does not reread the SVG.
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (p.x, p.y, p.angle) = (5.0, -3.0, 30.0);
        }
        h.frames(2);
        assert!(Arc::ptr_eq(&initial.svg.diagnostics, &place(&h).svg.diagnostics));
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.draft_tolerance = 0.1;
            preview_repair(p);
        }
        h.frames(2);
        assert!(place(&h).svg.selected.is_none(), "a repair reply does not select or annotate a segment");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.review = true;
            p.svg.selected = Some(0);
        }
        h.frames(2);
        let repaired = place(&h);
        h.app.cam.target = solidcore_record::Vec3::new(repaired.x, repaired.y, 0.0);
        evidence(&mut h, "svg-diagnostics-repair");
        assert_eq!(commands(&repaired).unwrap()[0].1["repair_tolerance"], json!(0.1));
        assert!(repaired.svg.diagnostics.iter().all(|d| d.repaired && d.max_deviation_mm.is_some_and(|d| d <= 0.1)));
        assert!(crate::scenario::count_of("svg_original_overlays").is_some_and(|count| count >= 1.0), "the original curve remains dashed");
        assert!(crate::scenario::count_of("svg_repaired_overlays").is_some_and(|count| count >= 1.0), "the replacement is also visible");
        assert!(handle_at("insert:svg:changed:0").is_some(), "the changed location is marked");
        assert!(handle_at("dialog:insert:svg:locate").is_some());
        assert!(std::sync::Arc::ptr_eq(&before, &h.app.session.doc), "repair remains a preview until OK");
        let mut invalid = repaired;
        for tolerance in [f64::NAN, f64::INFINITY, -1.0, 0.0, 11.0] {
            invalid.svg.tolerance = tolerance;
            assert!(commands(&invalid).is_err());
        }
        h.step(&json!({"key": "Escape"})).unwrap();
        h.settle();
        assert!(lines(&h).is_empty());
    }
    /// A normal import may start without a sketch; the future XY plane still has an overlay.
    #[test]
    fn svg_diagnostics_highlight_before_a_sketch_is_created() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        h.settle();
        h.app.cam.distance = 10.0;
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        let path = svg_file("fresh-document.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0 C-0.1 0 1 1 2 0 L2 2 L0 2 Z"/></svg>"#).unwrap();
        assert!(h.app.session.active_sketch.is_none());
        open(&mut h.app, "sketch.insert_svg", path);
        h.frames(3);
        assert!(h.app.session.active_sketch.is_none(), "the import stays a preview");
        assert!(Arc::ptr_eq(&before, &h.app.session.doc), "diagnosing and previewing do not create a sketch");
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(handle_at("insert:svg:highlight:0").is_none(), "no issues are marked before review");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.review = true;
            p.svg.selected = Some(0);
        }
        h.frames(2);
        assert!(handle_at("dialog:insert:svg:issue:0").is_some(), "the outline is listed during review");
        assert!(handle_at("insert:svg:highlight:0").is_some(), "review highlights the selected issue on the future XY sketch");
        assert!(handle_at("insert:handle:3").is_none(), "placement handles do not compete with review");
    }

    #[test]
    fn very_small_repair_bounds_are_shown_without_rounding_to_zero() {
        assert_eq!(deviation_text(1e-9), "1.000e-9");
        assert_eq!(deviation_text(2.5e-7), "2.500e-7");
        assert_eq!(deviation_text(0.001), "0.001000");
    }
    /// Only the closed part of a drawing becomes a shaded region, before creating a sketch.
    #[test]
    fn svg_preview_shades_closed_regions_and_leaves_open_outlines_unfilled() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        h.settle();
        h.app.cam.distance = 30.0;
        let before = h.app.session.doc.clone();
        let path = svg_file("closed-and-open.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0 L10 0 L10 10 L0 10 Z"/><path d="M12 0 L15 0 L15 3"/></svg>"#).unwrap();
        open(&mut h.app, "sketch.insert_svg", path);
        h.step(&json!({"until":{"count":{"name":"sketch_preview_closed_profiles","value":1}}})).unwrap();
        assert_eq!(crate::scenario::count_of("sketch_preview_closed_profiles"), Some(1.0));
        assert_eq!(crate::scenario::count_of("sketch_preview_shaded_profiles"), Some(1.0));
        assert!(crate::scenario::count_of("sketch_preview_fill_triangles").is_some_and(|count| count > 0.0));
        assert!(h.app.session.active_sketch.is_none());
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
        evidence(&mut h, "svg-closed-region-preview");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.show_faces = false;
        }
        h.frames(2);
        assert_eq!(crate::scenario::count_of("sketch_preview_closed_profiles"), Some(1.0), "turning off shading does not change the found profile");
        assert_eq!(crate::scenario::count_of("sketch_preview_shaded_profiles"), Some(0.0));
        assert_eq!(crate::scenario::count_of("sketch_preview_fill_triangles"), Some(0.0));
    }
    /// The initial view stays clean, repair is explicitly previewed, and comparison is reversible.
    #[test]
    fn svg_import_stages_group_outlines_and_compare_without_moving_the_camera() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        let path = svg_file("staged.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0 C-0.1 0 1 1 2 0 C1.9 -0.1 3 -1 4 0 L4 2 L0 2 Z"/></svg>"#).unwrap();
        let before = h.app.session.doc.clone();
        open(&mut h.app, "sketch.insert_svg", path);
        h.frames(60);
        let place = |h: &Harness| match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
            Some(Kind::InsertDrawing(p)) => p,
            other => panic!("no staged SVG import: {other:?}"),
        };
        let initial = place(&h);
        assert!(!initial.svg.repair && !initial.svg.review && !initial.svg.show_changes);
        assert!(initial.svg.selected.is_none());
        assert!(initial.svg.diagnostics.len() >= 2);
        assert_eq!(crate::scenario::count_of("svg_diagnostic_outlines"), Some(1.0), "segments are summarised as one outline");
        assert_eq!(crate::scenario::count_of("sketch_preview_point_markers"), Some(0.0));
        assert!(handle_at("dialog:insert:x").is_none(), "placement is collapsed");
        assert!(handle_at("dialog:insert:width").is_some(), "setup shows file and size in full before repair");
        assert!(handle_at("dialog:insert:scale").is_none(), "width and height are primary");
        assert!(handle_at("dialog:insert:svg:issue:0").is_none());
        let camera = h.app.cam;
        // First reveal the tolerance, then explicitly compute a proposal.
        click_control(&mut h, "dialog:insert:svg:repair");
        assert!(!place(&h).svg.repair);
        assert!(handle_at("dialog:insert:svg:tolerance").is_some());
        h.frames(2);
        assert!(handle_at("dialog:insert:width").is_none(), "file and size collapse to one summary row while repairing");
        assert!(crate::scenario::rect_of("svg:metrics:setup_summary").is_some(), "the summary row stays");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.draft_tolerance = 0.1;
        }
        click_control(&mut h, "dialog:insert:svg:repair");
        h.frames(3);
        let repaired = place(&h);
        assert!(repaired.svg.repair && !repaired.svg.original_view);
        assert!(repaired.svg.selected.is_none(), "a completed proposal does not enter technical review");
        assert_eq!(import_label(&repaired), "ui.svg_import_repaired");
        assert_eq!(commands(&repaired).unwrap()[0].1["repair_tolerance"], json!(0.1));
        let proposal = repaired.svg.diagnostics.clone();
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.original_view = true;
        }
        h.frames(3);
        let original = place(&h);
        assert!(commands(&original).unwrap()[0].1.get("repair_tolerance").is_none());
        assert!(Arc::ptr_eq(&proposal, &original.svg.diagnostics), "comparison retains its proposal");
        assert_eq!(h.app.cam, camera, "Before/After keeps exactly the same camera");
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.original_view = false;
        }
        h.frames(3);
        click_control(&mut h, "dialog:insert:svg:review");
        assert!(place(&h).svg.review && place(&h).svg.selected.is_none());
        assert_eq!(crate::scenario::count_of("svg_review_groups"), Some(1.0));
        click_control(&mut h, "dialog:insert:svg:group:0");
        let selected = place(&h).svg.selected.unwrap();
        assert!(handle_at(&format!("insert:svg:highlight:{selected}")).is_some());
        click_control(&mut h, "dialog:insert:svg:next");
        assert_ne!(place(&h).svg.selected, Some(selected));
        click_control(&mut h, "dialog:insert:svg:back");
        assert!(!place(&h).svg.review && place(&h).svg.selected.is_none());
        if let Some(Kind::InsertDrawing(p)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            p.svg.draft_tolerance = 0.2;
        }
        h.frames(2);
        assert!(!preview_ready(&place(&h)), "an edited tolerance needs an explicit preview");
        assert_eq!(commands(&place(&h)).unwrap()[0].1["repair_tolerance"], json!(0.1));
        click_control(&mut h, "dialog:insert:svg:original");
        h.frames(3);
        assert!(!place(&h).svg.repair);
        assert_eq!(import_label(&place(&h)), "ui.svg_import_original");
        assert!(commands(&place(&h)).unwrap()[0].1.get("repair_tolerance").is_none());
        assert!(Arc::ptr_eq(&before, &h.app.session.doc), "every stage remains a reversible preview");
    }

    #[test]
    fn changing_the_svg_file_preserves_the_design_and_cancel_preserves_the_form() {
        let mut h = Harness::new();
        h.app.home.open = false;
        let first = svg_file("first.svg");
        let second = svg_file("second.svg");
        open(&mut h.app, "sketch.insert_svg", first.clone());
        h.frames(3);
        let before = h.app.session.doc.clone();
        h.app.services.pick_open = Some(Box::new(move |_| Box::pin(std::future::ready(None))));
        click_control(&mut h, "dialog:insert:svg:file");
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::InsertDrawing(p)) if p.path == first));
        let replacement = second.clone();
        h.app.services.pick_open = Some(Box::new(move |extensions| {
            assert_eq!(extensions, ["svg"]);
            Box::pin(std::future::ready(Some(replacement.clone())))
        }));
        click_control(&mut h, "dialog:insert:svg:file");
        h.frames(3);
        assert!(
            matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::InsertDrawing(p)) if p.path == second && p.svg.selected.is_none() && !p.svg.repair)
        );
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    }
    #[test]
    fn svg_reselecting_a_modified_file_refreshes_closed_regions() {
        let mut h = Harness::new();
        h.app.home.open = false;
        let path = svg_file("reselected.svg");
        open(&mut h.app, "sketch.insert_svg", path.clone());
        h.step(&json!({"until":{"count":{"name":"sketch_preview_closed_profiles","value":1}}})).unwrap();
        assert_eq!(crate::scenario::count_of("sketch_preview_closed_profiles"), Some(1.0));
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="10mm" viewBox="0 0 20 10"><path d="M0 0 L20 0 L20 10"/></svg>"#,
        )
        .unwrap();
        open(&mut h.app, "sketch.insert_svg", path);
        h.step(&json!({"until":{"count":{"name":"sketch_preview_closed_profiles","value":0}}})).unwrap();
        assert_eq!(crate::scenario::count_of("sketch_preview_closed_profiles"), Some(0.0));
        assert!(h.app.session.active_sketch.is_none());
    }
    /// Curve adjustment, detected regions and extrusion readiness remain independent facts.
    #[test]
    fn svg_readiness_ladder_does_not_promote_a_repair_to_an_extrusion_check() {
        let mut h = Harness::new();
        h.app.home.open = false;
        let path = svg_file("readiness.svg");
        std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0 C3 0 7 0 10 0 L10 10 L0 10 Z"/><path d="M12 0 L14 0 L14 3"/></svg>"#).unwrap();
        let before = h.app.session.doc.clone();
        open(&mut h.app, "sketch.insert_svg", path);
        h.step(&json!({"until":{"count":{"name":"svg_readiness_regions","value":1}}})).unwrap();
        assert_eq!(crate::scenario::count_of("svg_readiness_steps"), Some(3.0));
        assert_eq!(crate::scenario::count_of("svg_extrude_checked"), Some(0.0));
        for name in ["curves", "faces", "changes", "placement"] {
            let handle = format!("dialog:insert:svg:{name}");
            reveal_control(&mut h, &handle);
            assert!(handle_at(&handle).is_some(), "the {name} chip is visible after scrolling");
            let control = crate::scenario::rect_of(&handle).unwrap();
            let body = crate::scenario::rect_of("dialog:body:viewport").unwrap();
            assert!(control.left() >= body.left() && control.right() <= body.right(), "{name} must fit the panel width: {control:?} in {body:?}");
        }
        let state = |h: &Harness| match h.app.dialog.as_ref().map(|d| d.kind.clone()) {
            Some(Kind::InsertDrawing(p)) => p,
            other => panic!("no readiness panel: {other:?}"),
        };
        assert!(state(&h).svg.show_curves && state(&h).svg.show_faces && state(&h).svg.show_placement);
        assert!(!state(&h).svg.show_changes && state(&h).svg.selected.is_none());
        click_control(&mut h, "dialog:insert:svg:curves");
        assert!(!state(&h).svg.show_curves);
        click_control(&mut h, "dialog:insert:svg:curves");
        click_control(&mut h, "dialog:insert:svg:faces");
        assert!(!state(&h).svg.show_faces);
        assert_eq!(crate::scenario::count_of("svg_readiness_regions"), Some(1.0), "shading does not decide which profiles exist");
        click_control(&mut h, "dialog:insert:svg:faces");
        click_control(&mut h, "dialog:insert:svg:placement");
        assert!(!placement_visible(&state(&h)));
        click_control(&mut h, "dialog:insert:svg:placement");
        click_control(&mut h, "dialog:insert:svg:repair");
        assert!(repair_open(&state(&h)) && !state(&h).svg.repair);
        assert!(!placement_visible(&state(&h)), "the repair editor hides placement before a proposal exists");
        click_control(&mut h, "dialog:insert:svg:repair");
        h.step(&json!({"until":{"count":{"name":"svg_readiness_regions","value":1}}})).unwrap();
        assert!(state(&h).svg.repair);
        assert!(state(&h).svg.diagnostics.iter().any(|d| d.repaired));
        assert_eq!(crate::scenario::count_of("svg_readiness_regions"), Some(1.0), "the separate open path did not become a face");
        assert_eq!(crate::scenario::count_of("svg_extrude_checked"), Some(0.0), "a repaired curve is not a proven extrusion");
        let labels = h
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label == h.app.tr("ui.svg_step_extrude")));
        assert!(labels.iter().any(|label| label == h.app.tr("ui.svg_not_checked")));
        evidence(&mut h, "svg-readiness-p0");
        assert!(Arc::ptr_eq(&before, &h.app.session.doc));
    }

    #[test]
    fn choosing_before_keeps_original_import_available_when_a_proposal_is_stale_or_failed() {
        let mut p = Place {
            command: "sketch.insert_svg".into(),
            path: svg_file("before-original.svg"),
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            angle: 0.0,
            natural: Some((20.0, 10.0)),
            svg: Default::default(),
        };
        p.svg.repair = true;
        p.svg.draft_tolerance = 0.2;
        p.svg.tolerance = 0.01;
        p.svg.error = Some("a proposal could not be made".into());
        p.svg.reply = Some(Default::default());
        assert!(!preview_ready(&p));
        p.svg.original_view = true;
        assert!(preview_ready(&p), "the dialog separately verifies that this raw geometry preview is current");
        assert!(commands(&p).unwrap()[0].1.get("repair_tolerance").is_none());
        assert_eq!(import_label(&p), "ui.svg_import_original");
    }
}
