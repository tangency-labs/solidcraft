//! Unfold for Printing in the app: one tool box that shows one step at a time, and only what
//! that step needs. Step 1 takes the face the print will lie on and copies the body into a tab
//! of its own (`print.unfold.start`). Step 2 takes clicks on edges, each a `print.unfold.bend`
//! at once (so each undoes on its own), until every face hangs on the pattern; Next then makes
//! the surface model (`print.unfold.surface`). The view draws the bends dashed and the cuts
//! solid over the model for as long as the design unfolds a body.

use std::cell::RefCell;

use egui::Stroke;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::print_unfold::PrintUnfoldStep;
use solidcraft_engine::schema::print_unfold::PrintUnfoldState;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::selection::{EDGES, PLANAR_FACES, SelInput};
use crate::viewport::Proj;
use solidcraft_theme::Tokens;

/// The command the tool box belongs to.
pub const COMMAND: &str = "print.unfold.start";

/// The step the tool box shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Pu {
    /// Step 1: the face the print will lie on.
    Base,
    /// Step 2: the bends and cuts.
    Edges,
    /// Step 2 is done: the surface model is made, not laid flat yet.
    Surface,
    /// Step 3: the pattern laid flat, checked against a print bed. `printer` is the entry of
    /// the printer list (one past its end: a size typed in, `width` by `depth`).
    Flat { printer: usize, width: String, depth: String },
    /// Step 4: the flat pattern split into parts at bends.
    Split,
    /// Step 5: the wall thickness and the joints. `bends` and `cuts` are entries of their
    /// lists, `assign` the entry an edge clicked gets.
    Joints { wall: String, bends: usize, cuts: usize, assign: usize },
    /// Step 6 is done: the printable body is made.
    Done,
}

/// What a bend can be, what a cut can be, and what an edge of its own can be: the engine's
/// word and the message id of its name.
const BEND_KINDS: [(&str, &str); 3] = [("film", "ui.print_unfold_kind_film"), ("filament", "ui.fold_kind_filament"), ("press", "ui.fold_kind_press")];
const CUT_KINDS: [(&str, &str); 4] =
    [("butt", "ui.print_joint_butt"), ("mitre", "ui.print_joint_mitre"), ("filament", "ui.fold_kind_filament"), ("press", "ui.fold_kind_press")];
const OWN_KINDS: [(&str, &str); 4] = [
    ("default", "ui.print_unfold_kind_default"),
    ("plain", "ui.print_unfold_kind_plain"),
    ("filament", "ui.fold_kind_filament"),
    ("press", "ui.fold_kind_press"),
];

impl Pu {
    pub fn title(&self) -> &'static str {
        "UNFOLD FOR PRINTING"
    }
}

/// How many steps the tool box has so far.
const STEPS: &str = "6";

thread_local! {
    /// The unfolded print of the design last asked about, by the session and revision it was
    /// read at: the view asks every frame.
    static STATE: RefCell<Option<(u64, u64, Option<std::rc::Rc<PrintUnfoldState>>)>> = const { RefCell::new(None) };
    /// The session whose tool box is at step 4 (splitting): steps 3 and 4 work on the same flat
    /// pattern, so the design does not tell them apart.
    static SPLITTING: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// The session whose tool box is at step 5 (wall and joints), which also works on the flat
    /// pattern.
    static JOINING: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// The tool box asked to open again at the step the design is at now.
    static REOPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The state of the active design's unfolded print; none in a design that unfolds nothing.
pub fn state(app: &SolveApp) -> Option<std::rc::Rc<PrintUnfoldState>> {
    app.session.doc.print_unfold.as_ref()?;
    let key = (app.session.uid, app.session.revision);
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((uid, revision, state)) = slot.as_ref()
            && (*uid, *revision) == key
        {
            return state.clone();
        }
        let state = solidcraft_engine::cmd::print_unfold::state_of(&app.session, COMMAND).ok().map(std::rc::Rc::new);
        *slot = Some((key.0, key.1, state.clone()));
        state
    })
}

/// A wall thickness typed, in mm (any length above zero).
fn bed_mm_any(text: &str) -> Option<f64> {
    text.trim().trim_end_matches("mm").trim().replace(',', ".").parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 1000.0)
}

/// A length typed for the bed, in mm.
fn bed_mm(text: &str) -> Option<f64> {
    text.trim().trim_end_matches("mm").trim().replace(',', ".").parse::<f64>().ok().filter(|v| (10.0..=5000.0).contains(v))
}

/// The tool box for the step the active design is at.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    if id != COMMAND {
        return None;
    }
    let printers = solidcraft_engine::cmd::print_unfold::PRINTERS;
    Some(match app.session.doc.print_unfold.as_ref().map(|u| u.step) {
        None => (Kind::PrintUnfold(Pu::Base), vec![SelInput::new("Base Face", PLANAR_FACES, false).tip("ui.print_unfold_base_tip")]),
        Some(PrintUnfoldStep::Edges) => (Kind::PrintUnfold(Pu::Edges), vec![SelInput::new("Edge", EDGES, true).tip("ui.print_unfold_edge_tip")]),
        Some(PrintUnfoldStep::Surface) => (Kind::PrintUnfold(Pu::Surface), Vec::new()),
        Some(PrintUnfoldStep::Solid) => (Kind::PrintUnfold(Pu::Done), Vec::new()),
        Some(PrintUnfoldStep::Flat) if JOINING.with(std::cell::Cell::get) == Some(app.session.uid) => {
            let s = state(app);
            let at = |kinds: &[(&str, &str)], now: Option<&str>| kinds.iter().position(|k| Some(k.0) == now).unwrap_or(0);
            let joints = Pu::Joints {
                wall: s.as_ref().map_or_else(|| "2 mm".to_string(), |s| s.wall.clone()),
                bends: at(&BEND_KINDS, s.as_ref().map(|s| s.bend_joint.as_str())),
                cuts: at(&CUT_KINDS, s.as_ref().map(|s| s.cut_joint.as_str())),
                assign: 2,
            };
            (Kind::PrintUnfold(joints), vec![SelInput::new("Edge", EDGES, true).tip("ui.print_unfold_joint_tip")])
        }
        Some(PrintUnfoldStep::Flat) if SPLITTING.with(std::cell::Cell::get) == Some(app.session.uid) => {
            (Kind::PrintUnfold(Pu::Split), vec![SelInput::new("Edge", EDGES, true).tip("ui.print_unfold_split_tip")])
        }
        Some(PrintUnfoldStep::Flat) => {
            // The bed chosen before, else a common one.
            let bed = app.session.doc.print_unfold.as_ref().and_then(|u| u.bed.clone());
            let (printer, width, depth) = match &bed {
                Some(b) => {
                    (printers.iter().position(|p| p.0 == b.name).unwrap_or(printers.len()), format!("{} mm", b.width), format!("{} mm", b.depth))
                }
                None => (1, "220 mm".to_string(), "220 mm".to_string()),
            };
            (Kind::PrintUnfold(Pu::Flat { printer, width, depth }), Vec::new())
        }
    })
}

/// The primary and the closing action of a step.
pub fn action_labels(k: &Pu) -> (&'static str, &'static str) {
    match k {
        Pu::Base => ("Next", "Cancel"),
        Pu::Edges | Pu::Surface | Pu::Flat { .. } | Pu::Split => ("Next", "Close"),
        Pu::Joints { .. } => ("Make Printable Body", "Close"),
        Pu::Done => ("Back", "Close"),
    }
}

/// Whether the step's conditions are met, so its primary action is offered.
pub fn ready(app: &SolveApp, k: &Pu, inputs: &[SelInput]) -> bool {
    match k {
        Pu::Base => matches!(inputs.first().map(|i| i.items.as_slice()), Some([Sel::Face { .. }])),
        Pu::Edges => state(app).is_some_and(|s| s.complete),
        Pu::Surface | Pu::Done => true,
        // A wall as thick as the joints chosen need.
        Pu::Joints { wall, .. } => bed_mm_any(wall).zip(state(app)).is_some_and(|(w, s)| w + 1e-9 >= s.min_wall),
        // Faces that lie on each other can't be printed: split until none do.
        Pu::Split => state(app).is_some_and(|s| s.overlapping.is_empty()),
        // A bed is chosen. A pattern that does not fit it may still go on: it can be split.
        Pu::Flat { .. } => state(app).is_some_and(|s| s.bed.is_some()),
    }
}

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn note(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.add(egui::Label::new(egui::RichText::new(text).color(t.text_dim).size(t.font_body)).wrap());
}

fn warn(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get();
    ui.add(egui::Label::new(egui::RichText::new(text).color(t.warning).size(t.font_body)).wrap());
}

fn count_row(ui: &mut egui::Ui, label: &str, value: String) {
    crate::dialogs::form_row(ui, label, |ui| {
        ui.label(value);
    });
}

fn step_note(ui: &mut egui::Ui, step: &str) {
    note(ui, &solidcraft_ui_text::format("ui.print_unfold_step", &[("step", step), ("of", STEPS)]));
}

/// The parts of the flat pattern, each with the rectangle round it and whether it fits the bed.
fn part_rows(app: &SolveApp, ui: &mut egui::Ui) {
    let Some(s) = state(app) else { return };
    for (i, part) in s.parts.iter().enumerate() {
        let size = solidcraft_ui_text::format(
            "ui.print_unfold_part_size",
            &[("long", &format!("{:.1}", part.long)), ("short", &format!("{:.1}", part.short))],
        );
        let fits = match part.fits {
            Some(true) => solidcraft_ui_text::tr("ui.print_unfold_fits"),
            Some(false) => solidcraft_ui_text::tr("ui.print_unfold_fits_not"),
            None => "",
        };
        count_row(ui, &solidcraft_ui_text::format("ui.print_unfold_part", &[("n", &(i + 1).to_string())]), format!("{size}  {fits}"));
    }
    if s.parts.iter().any(|p| p.fits == Some(false)) {
        warn(ui, solidcraft_ui_text::tr("ui.print_unfold_no_fit"));
    }
    if s.parts.iter().any(|p| p.overlaps > 0) {
        warn(ui, solidcraft_ui_text::tr("ui.print_unfold_overlap"));
    }
}

/// A button back to the bends and cuts, which the flat pattern is made from.
fn edit_edges_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::dialogs::form_row(ui, "", |ui| {
        if crate::ui_kit::button(ui, solidcraft_ui_text::tr("cmd.print_unfold_edit"), crate::ui_kit::Button::Small).clicked() {
            match app.run("print.unfold.edit", json!({})) {
                Ok(_) => {
                    SPLITTING.with(|c| c.set(None));
                    JOINING.with(|c| c.set(None));
                    REOPEN.with(|c| c.set(true));
                }
                Err(e) => app.set_status(e, true),
            }
        }
    });
}

/// The rows under the step's pick field. In steps 2 and 4 every edge clicked acts at once, and
/// leaves the field again.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Pu, inputs: &mut [SelInput]) -> bool {
    let mut enter = false;
    match k {
        Pu::Base => {
            step_note(ui, "1");
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_base_note"));
        }
        Pu::Edges => {
            let picked: Vec<Sel> = inputs.first_mut().map(|i| std::mem::take(&mut i.items)).unwrap_or_default();
            for sel in picked {
                let Sel::Edge { point, .. } = sel else { continue };
                match app.run("print.unfold.bend", json!({"edge": [point.x, point.y, point.z]})) {
                    Ok(reply) => {
                        let id = match reply["changed"].as_str() {
                            Some("moved") => "ui.print_unfold_moved",
                            Some("removed") => "ui.print_unfold_removed",
                            _ => "ui.print_unfold_added",
                        };
                        app.set_status(solidcraft_ui_text::tr(id), false);
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
            step_note(ui, "2");
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_edges_note"));
            if let Some(s) = state(app) {
                let on = solidcraft_ui_text::format("ui.print_unfold_faces_on", &[("on", &s.attached.to_string()), ("all", &s.faces.to_string())]);
                count_row(ui, solidcraft_ui_text::tr("ui.print_unfold_faces"), on);
                count_row(ui, solidcraft_ui_text::tr("ui.print_unfold_bends"), s.bends.len().to_string());
                count_row(ui, solidcraft_ui_text::tr("ui.print_unfold_cuts"), s.cuts.len().to_string());
                if !s.complete {
                    note(ui, solidcraft_ui_text::tr("ui.print_unfold_missing_note"));
                    note(ui, solidcraft_ui_text::tr("ui.print_unfold_incomplete"));
                }
            }
        }
        Pu::Surface => {
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_surface_note"));
            edit_edges_row(app, ui);
        }
        Pu::Flat { printer, width, depth } => {
            step_note(ui, "3");
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_flat_note"));
            let printers = solidcraft_engine::cmd::print_unfold::PRINTERS;
            let custom = solidcraft_ui_text::tr("ui.print_unfold_custom_bed").to_string();
            let mut names: Vec<&str> = printers.iter().map(|p| p.0).collect();
            names.push(&custom);
            crate::ui_kit::choice_row(ui, solidcraft_ui_text::tr("ui.print_unfold_printer"), "pu_printer", printer, &names);
            let chosen = match printers.get(*printer) {
                Some((name, w, d)) => Some((name.to_string(), *w, *d)),
                None => {
                    enter |= crate::ui_kit::expr_row(ui, solidcraft_ui_text::tr("ui.print_unfold_bed_width"), width);
                    enter |= crate::ui_kit::expr_row(ui, solidcraft_ui_text::tr("ui.print_unfold_bed_depth"), depth);
                    bed_mm(width).zip(bed_mm(depth)).map(|(w, d)| (String::new(), w, d))
                }
            };
            if let Some((name, w, d)) = chosen {
                let same = state(app)
                    .and_then(|s| s.bed.clone())
                    .is_some_and(|b| b.name == name && (b.width - w).abs() < 1e-9 && (b.depth - d).abs() < 1e-9);
                if !same && let Err(e) = app.run("print.unfold.bed", json!({"name": name, "width": w, "depth": d})) {
                    app.set_status(e, true);
                }
                count_row(
                    ui,
                    solidcraft_ui_text::tr("ui.print_unfold_bed"),
                    solidcraft_ui_text::format("ui.print_unfold_part_size", &[("long", &format!("{w:.0}")), ("short", &format!("{d:.0}"))]),
                );
            }
            part_rows(app, ui);
            edit_edges_row(app, ui);
        }
        Pu::Split => {
            let picked: Vec<Sel> = inputs.first_mut().map(|i| std::mem::take(&mut i.items)).unwrap_or_default();
            for sel in picked {
                let Sel::Edge { point, .. } = sel else { continue };
                match app.run("print.unfold.split", json!({"edge": [point.x, point.y, point.z]})) {
                    Ok(reply) => {
                        let id = if reply["changed"] == "joined" { "ui.print_unfold_joined" } else { "ui.print_unfold_split_done" };
                        app.set_status(solidcraft_ui_text::tr(id), false);
                    }
                    Err(e) => app.set_status(e, true),
                }
            }
            step_note(ui, "4");
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_split_note"));
            part_rows(app, ui);
            crate::dialogs::form_row(ui, "", |ui| {
                if crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.print_unfold_to_bed"), crate::ui_kit::Button::Small).clicked() {
                    SPLITTING.with(|c| c.set(None));
                    REOPEN.with(|c| c.set(true));
                }
            });
            edit_edges_row(app, ui);
        }
        Pu::Joints { wall, bends, cuts, assign } => {
            let own = OWN_KINDS.get(*assign).map_or("default", |k| k.0);
            let picked: Vec<Sel> = inputs.first_mut().map(|i| std::mem::take(&mut i.items)).unwrap_or_default();
            for sel in picked {
                let Sel::Edge { point, .. } = sel else { continue };
                if let Err(e) = app.run("print.unfold.joint", json!({"edge": [point.x, point.y, point.z], "kind": own})) {
                    app.set_status(e, true);
                }
            }
            step_note(ui, "5");
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_joints_note"));
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_rigid_note"));
            if let Some(s) = state(app)
                && s.min_wall > 0.0
            {
                let text = solidcraft_ui_text::format("ui.print_unfold_min_wall", &[("wall", &format!("{:.1}", s.min_wall))]);
                if bed_mm_any(wall).is_some_and(|w| w + 1e-9 < s.min_wall) {
                    warn(ui, &text);
                } else {
                    note(ui, &text);
                }
            }
            enter |= crate::ui_kit::expr_row(ui, solidcraft_ui_text::tr("ui.print_wall"), wall);
            let names = |kinds: &[(&str, &str)]| -> Vec<String> { kinds.iter().map(|k| solidcraft_ui_text::tr(k.1).to_string()).collect() };
            let (bend_names, cut_names, own_names) = (names(&BEND_KINDS), names(&CUT_KINDS), names(&OWN_KINDS));
            crate::ui_kit::choice_row(ui, solidcraft_ui_text::tr("ui.print_unfold_bends"), "pu_bend_kind", bends, &refs(&bend_names));
            crate::ui_kit::choice_row(ui, solidcraft_ui_text::tr("ui.print_unfold_cuts"), "pu_cut_kind", cuts, &refs(&cut_names));
            crate::ui_kit::choice_row(ui, solidcraft_ui_text::tr("ui.print_unfold_own_kind"), "pu_own_kind", assign, &refs(&own_names));
            // What the rows say goes to the design at once.
            if let Some(s) = state(app) {
                let (b, c) = (BEND_KINDS.get(*bends).map_or("film", |k| k.0), CUT_KINDS.get(*cuts).map_or("butt", |k| k.0));
                let typed = wall.trim();
                if (s.bend_joint != b || s.cut_joint != c || (!typed.is_empty() && s.wall != typed))
                    && let Err(e) =
                        app.run("print.unfold.joints", json!({"wall": if typed.is_empty() { s.wall.as_str() } else { typed }, "bends": b, "cuts": c}))
                {
                    app.set_status(e, true);
                }
                let own: std::collections::BTreeSet<usize> = s.flat_bends.iter().chain(&s.flat_cuts).filter(|e| e.own).map(|e| e.edge).collect();
                count_row(ui, solidcraft_ui_text::tr("ui.print_unfold_own_count"), own.len().to_string());
            }
            crate::dialogs::form_row(ui, "", |ui| {
                if crate::ui_kit::button(ui, solidcraft_ui_text::tr("ui.print_unfold_to_split"), crate::ui_kit::Button::Small).clicked() {
                    JOINING.with(|c| c.set(None));
                    SPLITTING.with(|c| c.set(Some(app.session.uid)));
                    REOPEN.with(|c| c.set(true));
                }
            });
        }
        Pu::Done => {
            step_note(ui, "6");
            note(ui, solidcraft_ui_text::tr("ui.print_unfold_done_note"));
        }
    }
    enter
}

/// The step's primary action.
pub fn run(app: &mut SolveApp, k: &Pu, inputs: &[SelInput]) -> Result<(), String> {
    match k {
        Pu::Base => {
            let Some(Sel::Face { body, point, .. }) = inputs.first().and_then(|i| i.items.first()) else {
                return Err(solidcraft_ui_text::tr("ui.print_unfold_base_tip").to_string());
            };
            app.run(COMMAND, json!({"body": body, "face": [point.x, point.y, point.z]})).map(|_| ())
        }
        // The surface model is made and laid flat in one go.
        Pu::Edges | Pu::Surface => {
            SPLITTING.with(|c| c.set(None));
            JOINING.with(|c| c.set(None));
            app.run("print.unfold.flatten", json!({})).map(|_| ())?;
            app.animate_view("top");
            app.fit_view();
            Ok(())
        }
        Pu::Flat { .. } => {
            SPLITTING.with(|c| c.set(Some(app.session.uid)));
            Ok(())
        }
        Pu::Split => {
            SPLITTING.with(|c| c.set(None));
            JOINING.with(|c| c.set(Some(app.session.uid)));
            Ok(())
        }
        Pu::Joints { .. } => {
            app.run("print.unfold.make", json!({})).map(|_| ())?;
            JOINING.with(|c| c.set(None));
            app.fit_view();
            Ok(())
        }
        Pu::Done => {
            app.run("print.unfold.unmake", json!({})).map(|_| ())?;
            JOINING.with(|c| c.set(Some(app.session.uid)));
            Ok(())
        }
    }
}

/// Open the tool box again when a button in it changed the step (called once a frame).
pub fn tick(app: &mut SolveApp) {
    if REOPEN.with(|c| c.replace(false)) {
        crate::dialogs::cancel(app);
        app.start(COMMAND);
        app.fit_view();
    }
}

/// `print.unfold.start` in the app: the design that unfolds the body opens in a tab of its own,
/// and the design the body came from stays as it is.
pub fn start_in_tab(app: &mut SolveApp, params: &Value) -> Result<Value, String> {
    let body = params["body"].as_str().ok_or("print.unfold.start: give `body`")?;
    let face =
        params.get("face").and_then(solidcraft_engine::params::vec3).ok_or("print.unfold.start: `face` must be [x, y, z] of numbers, in mm")?;
    let fresh = solidcraft_engine::cmd::print_unfold::unfold_design(&app.session, body, face).map_err(|e| e.to_string())?;
    let reply = solidcraft_engine::cmd::print_unfold::state_of(&fresh, COMMAND).map_err(|e| e.to_string())?;
    crate::documents::open_tab(app, fresh)?;
    serde_json::to_value(reply).map_err(|e| e.to_string())
}

/// The bends (dashed) and the cuts (solid) of the active design's unfolded print, over the
/// model; laid flat, also the rectangle round each part, in the colour of whether it fits.
pub fn overlay(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(state) = state(app) else { return };
    let t = Tokens::get();
    let clip = painter.clip_rect();
    let point = |p: &[f64; 3]| proj.to_screen(solidcore_record::Vec3::new(p[0], p[1], p[2])).filter(|p| p.is_finite());
    if state.step == "flat" {
        for part in &state.parts {
            let colour = match part.fits {
                Some(true) => t.form_success,
                Some(false) => t.form_error,
                None => t.text_dim,
            };
            for i in 0..4 {
                let (a, b) = (part.corners[i], part.corners[(i + 1) % 4]);
                if let (Some(a), Some(b)) = (point(&[a[0], a[1], 0.0]), point(&[b[0], b[1], 0.0])) {
                    painter.extend(crate::ui_kit::dashed_line(&[a, b], clip, Stroke::new(1.5, colour), 4.0, 4.0));
                }
            }
        }
        for outline in &state.overlapping {
            let n = outline.len();
            for i in 0..n {
                if let (Some(a), Some(b)) = (outline.get(i).and_then(point), outline.get((i + 1) % n).and_then(point)) {
                    painter.line_segment([a, b], Stroke::new(4.0, t.form_error));
                }
            }
        }
        // A filament joint is green and a press fit red, on bends (dashed) and cuts alike.
        let joint = |kind: &str, plain: egui::Color32| match kind {
            "filament" => (t.form_success, 4.0),
            "press" => (t.axis_x, 4.0),
            _ => (plain, 2.0),
        };
        for edge in &state.flat_cuts {
            if let (Some(a), Some(b)) = (point(&edge.start), point(&edge.end)) {
                let (colour, width) = joint(&edge.joint, t.warning);
                painter.line_segment([a, b], Stroke::new(width, colour));
            }
        }
        for edge in &state.flat_bends {
            if let (Some(a), Some(b)) = (point(&edge.start), point(&edge.end)) {
                let (colour, width) = joint(&edge.joint, t.accent);
                painter.extend(crate::ui_kit::dashed_line(&[a, b], clip, Stroke::new(width.max(3.0), colour), 9.0, 6.0));
            }
        }
        return;
    }
    // The faces not on the pattern yet stand out: they are what is left to do.
    for outline in &state.missing {
        let n = outline.len();
        for i in 0..n {
            if let (Some(a), Some(b)) = (outline.get(i).and_then(point), outline.get((i + 1) % n).and_then(point)) {
                painter.line_segment([a, b], Stroke::new(4.0, t.form_error));
            }
        }
    }
    for edge in &state.cuts {
        if let (Some(a), Some(b)) = (point(&edge.start), point(&edge.end)) {
            painter.line_segment([a, b], Stroke::new(2.0, t.warning));
        }
    }
    for edge in &state.bends {
        if let (Some(a), Some(b)) = (point(&edge.start), point(&edge.end)) {
            painter.extend(crate::ui_kit::dashed_line(&[a, b], clip, Stroke::new(3.0, t.accent), 9.0, 6.0));
        }
    }
}

thread_local! {
    /// The session of an unfolded print whose body changed in the design it was copied from:
    /// its user is asked what to do.
    static CHANGED: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Whether `copy` unfolds a body copied from the active design.
fn copied_from_active(app: &SolveApp, copy: &solidcraft_engine::Session) -> bool {
    copy.doc.print_unfold.as_ref().and_then(|u| u.source.as_ref()).is_some_and(|source| {
        (source.session != 0 && source.session == app.session.uid) || (source.path.is_some() && source.path == app.session.path)
    })
}

/// After a command changed the active design: a body that another tab unfolds for printing and
/// that no longer measures what it did when it was copied makes the app ask what to do.
pub fn source_changed(app: &mut SolveApp) {
    let mut changed = None;
    for slot in app.docs.slots.iter().flatten() {
        let copy = &slot.session;
        if !copied_from_active(app, copy) {
            continue;
        }
        let Some(source) = copy.doc.print_unfold.as_ref().and_then(|u| u.source.as_ref()) else { continue };
        let now = app
            .session
            .world_state()
            .ok()
            .and_then(|world| world.body(&source.body).and_then(|b| solidcraft_engine::cmd::print_unfold::stamp_of(b).ok()));
        if !now.is_some_and(|now| now.same(&source.stamp)) {
            changed = Some(copy.uid);
        }
    }
    if changed.is_some() {
        CHANGED.with(|c| c.set(changed));
    }
}

/// What the user chose for an unfolded print whose body changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Changed {
    /// Start the unfolding again from the changed body.
    Discard,
    /// Take the change of the body back.
    Undo,
}

/// The tab that unfolds the changed body, while the question about it is open.
pub fn changed_copy(app: &SolveApp) -> Option<usize> {
    let uid = CHANGED.with(std::cell::Cell::get)?;
    app.docs.slots.iter().position(|slot| slot.as_ref().is_some_and(|slot| slot.session.uid == uid))
}

/// Act on the answer.
pub fn answer_changed(app: &mut SolveApp, answer: Changed) {
    let copy = changed_copy(app);
    CHANGED.with(|c| c.set(None));
    let Some(i) = copy else { return };
    match answer {
        Changed::Undo => {
            if let Err(e) = app.run("edit.undo", json!({})) {
                app.set_status(e, true);
            }
        }
        Changed::Discard => {
            let source =
                app.docs.slots.get(i).and_then(|s| s.as_ref()).and_then(|s| s.session.doc.print_unfold.as_ref()).and_then(|u| u.source.clone());
            let Some(source) = source else { return };
            match solidcraft_engine::cmd::print_unfold::unfold_design(&app.session, &source.body, source.face) {
                Ok(fresh) => {
                    if let Some(Some(slot)) = app.docs.slots.get_mut(i) {
                        slot.session.replace_design(fresh);
                    }
                    app.set_status(solidcraft_ui_text::tr("ui.print_unfold_restarted"), false);
                }
                // Its base face is gone: the tab keeps the old copy, which no longer follows.
                Err(e) => {
                    if let Some(Some(slot)) = app.docs.slots.get_mut(i)
                        && let Some(source) = slot.session.doc_mut().print_unfold.as_mut().and_then(|u| u.source.as_mut())
                    {
                        source.session = 0;
                        source.path = None;
                    }
                    app.set_status(solidcraft_ui_text::format("ui.print_unfold_restart_failed", &[("why", &e.to_string())]), true);
                }
            }
        }
    }
}

/// Ask what to do about an unfolded print whose body changed in the design it came from.
pub fn prompt(app: &mut SolveApp, ctx: &egui::Context) {
    tick(app);
    if CHANGED.with(std::cell::Cell::get).is_none() {
        return;
    }
    if changed_copy(app).is_none() {
        CHANGED.with(|c| c.set(None));
        return;
    }
    let (closed, action) = crate::ui_kit::Confirm::new(
        egui::Id::unique("sc_print_unfold_changed"),
        "status_warning",
        solidcraft_ui_text::tr("ui.print_unfold_changed_title"),
    )
    .tone(crate::ui_kit::Tone::Warning)
    .actions(solidcraft_ui_text::tr("ui.print_unfold_discard"), solidcraft_ui_text::tr("ui.print_unfold_undo_change"), None)
    .show(ctx, true, |ui| {
        crate::ui_kit::text(ui, solidcraft_ui_text::tr("ui.print_unfold_changed"), crate::ui_kit::Text::Report("print_unfold_changed", false));
    });
    match action {
        crate::ui_kit::Action::Ok => answer_changed(app, Changed::Discard),
        crate::ui_kit::Action::Cancel => answer_changed(app, Changed::Undo),
        _ if closed => answer_changed(app, Changed::Undo),
        _ => {}
    }
}

#[cfg(test)]
#[path = "dialogs_print_unfold_tests.rs"]
mod tests;
