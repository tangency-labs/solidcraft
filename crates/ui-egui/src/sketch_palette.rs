//! The Sketch Palette: a panel docked at the right while a sketch is being edited, like a
//! command dialog. Feature Options: the modes of the creation tool that is running, as icon
//! buttons. Options: the line type of the selected curves (normal, construction, centerline),
//! Look At, and the display toggles kept with the sketch (grid, snap, slice, profiles, points,
//! dimensions, constraints, projected geometry, construction geometry, 3D sketch); the sketch's
//! degrees of freedom. Finish Sketch sits at the bottom, beside the ribbon's Exit group. Toggles run `sketch.options`, line types
//! `sketch.construction` / `sketch.centerline`.
//!
//! The line type is also a drawing mode: with nothing selected, Construction (or X) makes what
//! is drawn next construction geometry until it is switched back.

use std::cell::Cell;

use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;
use crate::theme::Tokens;

thread_local! {
    /// The drawing line type (0 normal, 1 construction, 2 centerline) and the sketch it is for.
    static MODE: Cell<(u8, Option<u64>)> = const { Cell::new((0, None)) };
}

/// Option keys and existing localized labels. The five primary design rows are selected below;
/// the remaining display options live in More. Point/profile labels share the dialog IDs.
pub const TOGGLES: [(&str, &str); 10] = [
    ("grid", "Sketch Grid"),
    ("snap", "Snap"),
    ("slice", "Slice"),
    ("show_profile", "ui.profile"),
    ("show_points", "ui.points"),
    ("show_dimensions", "Dimensions"),
    ("show_constraints", "Constraints"),
    ("show_projected", "Projected Geometries"),
    ("show_construction", "Construction Geometries"),
    ("sketch_3d", "3D Sketch"),
];

/// Does the sketch hold any geometry of its own: a drawn curve or a 3D curve? Projected curves
/// and the origin are held in place and do not count.
pub fn has_geometry(sketch: &solidcraft_engine::sketch::Sketch) -> bool {
    !sketch.curves.is_empty() || !sketch.wires.is_empty()
}

/// Nothing the user added: no curve, 3D curve, constraint or dimension, pattern, projection,
/// text or offset, and no point but the fixed origin.
pub fn is_blank(sketch: &solidcraft_engine::sketch::Sketch) -> bool {
    !has_geometry(sketch)
        && sketch.constraints.is_empty()
        && sketch.patterns.is_empty()
        && sketch.links.is_empty()
        && sketch.offsets.is_empty()
        && sketch.freeform_offsets.is_empty()
        && sketch.points.iter().all(|p| p.id == "origin")
}

/// The degrees-of-freedom line: "No geometry yet" (an empty sketch only has its fixed origin),
/// "Fully constrained" or "N degrees of freedom".
pub fn dof_text(dof: usize, ok: bool, has_geometry: bool) -> String {
    if !ok {
        "Constraints conflict".into()
    } else if !has_geometry {
        "No geometry yet".into()
    } else if dof == 0 {
        "Fully constrained".into()
    } else if dof == 1 {
        "1 degree of freedom".into()
    } else {
        format!("{dof} degrees of freedom")
    }
}

/// The line type new curves get in the sketch being edited (0 normal, 1 construction,
/// 2 centerline, 3 infinite); a new sketch starts normal.
pub fn mode(app: &SolveApp) -> u8 {
    let (m, sk) = MODE.with(Cell::get);
    if sk.is_some() && sk == app.session.active_sketch { m } else { 0 }
}

/// Switch the drawing line type (with a status hint).
pub fn set_mode(app: &mut SolveApp, m: u8) {
    MODE.with(|c| c.set((m, app.session.active_sketch)));
    let hint = match m {
        1 => "Construction mode: new curves are construction geometry (X or the palette's Normal to leave)",
        2 => "Centerline mode: new lines are centerlines",
        3 => "Infinite mode: new lines are infinite construction lines",
        _ => "Normal mode",
    };
    app.set_status(hint, false);
}

/// X / the toolbar's Construction: convert the selected curves, or with nothing selected
/// switch construction mode on or off.
pub fn toggle_construction(app: &mut SolveApp) {
    let curves = selected_curves(app);
    if curves.is_empty() && app.session.active_sketch.is_none() {
        app.set_status("Construction mode is for drawing in a sketch: edit a sketch first", false);
    } else if curves.is_empty() {
        let m = if mode(app) == 1 { 0 } else { 1 };
        set_mode(app, m);
    } else {
        let _ = app.run("sketch.construction", json!({ "curves": curves }));
    }
}

/// Curves a drawing command just made take the drawing line type.
pub fn apply_mode(app: &mut SolveApp, id: &str, result: &Value) {
    if solidcraft_engine::cmd::construction_line_mode(id).is_some() {
        return;
    }
    let m = mode(app);
    if m == 0 || !solidcraft_engine::find_command(id).is_some_and(|c| c.tab == "SKETCH" && c.panel == "CREATE") {
        return;
    }
    let curves: Vec<String> = result["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
    if curves.is_empty() {
        return;
    }
    let lines: Vec<String> = if m == 2 || m == 3 {
        let st = app.session.model.state();
        let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
        curves
            .iter()
            .filter(|c| {
                ss.sketch
                    .curve_index(c)
                    .and_then(|i| ss.sketch.curves.get(i))
                    .is_some_and(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Line { .. }))
            })
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let rest: Vec<String> = curves.iter().filter(|c| !lines.contains(c)).cloned().collect();
    // Part of the drawing's own undo step.
    let depth = app.session.undo.len();
    if !lines.is_empty() {
        let line_type = if m == 3 { "sketch.infinite" } else { "sketch.centerline" };
        let _ = app.session.execute(line_type, &json!({ "curves": lines, "value": true }));
    }
    if !rest.is_empty() {
        let _ = app.session.execute("sketch.construction", &json!({ "curves": rest, "value": true }));
    }
    app.session.undo.truncate(depth);
}

/// Selected curves of the active sketch.
fn selected_curves(app: &SolveApp) -> Vec<String> {
    app.session.selection.iter().filter_map(|s| if let Sel::SketchCurve { id, .. } = s { Some(id.clone()) } else { None }).collect()
}

/// The line types among the selected curves of the active sketch, indexed like the palette's
/// buttons (0 normal, 1 construction, 2 centerline, 3 infinite); a mixed selection marks each of
/// its types.
fn selected_linetypes(app: &SolveApp) -> [bool; 4] {
    let mut types = [false; 4];
    let curves = selected_curves(app);
    if curves.is_empty() {
        return types;
    }
    let st = app.session.world_state();
    let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return types };
    for c in curves.iter().filter_map(|id| ss.sketch.curve_index(id).and_then(|i| ss.sketch.curves.get(i))) {
        let k = if c.infinite {
            3
        } else if c.centerline {
            2
        } else if c.construction {
            1
        } else {
            0
        };
        if let Some(t) = types.get_mut(k) {
            *t = true;
        }
    }
    types
}

/// Set the line type of the selected curves: 0 normal, 1 construction, 2 centerline, 3 infinite
/// (the last two for lines).
pub fn set_linetype(app: &mut SolveApp, kind: u8) {
    let curves = selected_curves(app);
    if curves.is_empty() {
        app.set_status("Select sketch curves first", false);
        return;
    }
    let lines: Vec<String> = {
        let st = app.session.world_state();
        let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
        curves
            .iter()
            .filter(|id| {
                ss.sketch
                    .curve_index(id)
                    .and_then(|i| ss.sketch.curves.get(i))
                    .is_some_and(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Line { .. }))
            })
            .cloned()
            .collect()
    };
    let r = match kind {
        0 => {
            if !lines.is_empty() {
                let _ = app.run("sketch.centerline", json!({ "curves": lines, "value": false }));
            }
            app.run("sketch.construction", json!({ "curves": curves, "value": false }))
        }
        1 => {
            if !lines.is_empty() {
                let _ = app.run("sketch.infinite", json!({ "curves": lines, "value": false }));
            }
            app.run("sketch.construction", json!({ "curves": curves, "value": true }))
        }
        3 if lines.is_empty() => Err("an infinite line must be a line".to_string()),
        3 => app.run("sketch.infinite", json!({ "curves": lines, "value": true })),
        _ if lines.is_empty() => Err("a centerline must be a line".to_string()),
        _ => app.run("sketch.centerline", json!({ "curves": lines, "value": true })),
    };
    if let Err(e) = r {
        app.set_status(e, true);
    }
}

/// The palette's content width in `vp`; it gives way to the viewport on narrow views.
/// The line types, in the order of the mode and of the selection flags: key, icon master and name.
const LINE_TYPES: [(&str, &str, &str); 4] = [
    ("normal", "normal", "Normal"),
    ("construction", "construction", "Construction"),
    ("centerline", "centerline", "Centerline"),
    ("infinite", "infinite", "Infinite"),
];

fn width(vp: egui::Rect) -> f32 {
    let t = Tokens::get();
    t.sketch_palette_width.min((vp.width() * 0.46 - t.space_16).max(t.space_24 * 5.0))
}

/// The viewport the palette docks into.
fn viewport(app: &SolveApp, ctx: &egui::Context) -> egui::Rect {
    crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())))
}

/// Is a sketch being edited? That is the palette's own condition.
pub fn editing(app: &SolveApp) -> bool {
    app.session.active_sketch.and_then(|s| app.session.world_state().sketch(s).map(|_| ())).is_some()
}

/// Has the viewport room for the palette and a command dialog side by side? A narrow view has
/// not, and there the dialog keeps the right edge on its own.
pub fn fits_beside_a_dialog(app: &SolveApp, ctx: &egui::Context) -> bool {
    let vp = viewport(app, ctx);
    vp.width() >= width(vp) + crate::frame::DIALOG_MIN_W + 40.0
}

/// Is the palette on screen? It stays for as long as a sketch is being edited, a command dialog
/// open or not: the dialog takes the left edge of the viewport (or the left column) and the
/// palette keeps the right.
pub fn visible(app: &SolveApp, ctx: &egui::Context) -> bool {
    editing(app) && ((app.dialog.is_none() && app.tree.redefine.is_none()) || fits_beside_a_dialog(app, ctx) || crate::dialog_column::holds(app))
}

/// The creation family of the tool that is running, and the exact variant it is running, when
/// that family has more than one mode to switch between.
///
/// Family membership comes from the declared table in `sketch_families`, never from the shape of
/// the id. `sketch.line` and `sketch.line.midpoint` are one family although only one of them
/// carries a variant suffix, and `sketch.ellipse`, `sketch.conic` and `sketch.text` carry no
/// family at all. The table also fixes the order the modes are shown in, which a derived rule
/// cannot express.
///
/// A tool outside a family has nothing to switch to, so the section stays hidden rather than
/// showing a single button that does nothing. The palette's vertical space is already budgeted
/// down to the footer, so an inert row would cost the Options list real height.
fn active_feature(app: &SolveApp) -> Option<(&'static crate::sketch_families::Family, &'static str)> {
    let id = app.tool.as_ref()?.cmd;
    let family = crate::sketch_families::family(id)?;
    (family.commands.len() > 1).then_some((family, id))
}

fn section_alias(name: &str, title: &str) {
    if let Some(at) = crate::scenario::handle_at(&format!("dialog:section:{title}")) {
        crate::scenario::publish_handle(name, at);
    }
}

/// Count the sketch's independent input coordinates and radii before constraints. The solver
/// stores remaining DOF but no total: fixed/projected inputs are constants, not free variables.
/// Arc endpoints already supply their radii through coordinates, as in the solver's layout.
fn unconstrained_total(sketch: &solidcraft_engine::sketch::Sketch) -> usize {
    let coordinates = sketch.points.iter().filter(|p| !p.fixed && p.link.is_none()).count().saturating_mul(2);
    let radii = sketch
        .curves
        .iter()
        .filter(|c| {
            !c.fixed
                && c.link.is_none()
                && matches!(c.kind, solidcraft_engine::sketch::CurveKind::Circle { .. } | solidcraft_engine::sketch::CurveKind::Ellipse { .. })
        })
        .count();
    coordinates.saturating_add(radii)
}

fn constrained_fraction(remaining: usize, total: usize, solved: bool) -> f32 {
    if !solved {
        0.0
    } else if total == 0 {
        if remaining == 0 { 1.0 } else { 0.0 }
    } else {
        1.0 - remaining.min(total) as f32 / total as f32
    }
}

/// Draw the sketch options card, retaining the command paths used by the original palette.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    app.viewport.palette_rect = None;
    let _language = crate::i18n::scope(&app.preferences.language);
    if !visible(app, ctx) {
        return;
    }
    let (opts, name, remaining, solved, total, geometry, open_ends, untied_ends) = {
        let st = app.session.world_state();
        let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
        (
            ss.sketch.view.clone(),
            ss.name.clone(),
            ss.report.dof,
            ss.report.ok(),
            unconstrained_total(&ss.sketch),
            has_geometry(&ss.sketch),
            ss.ends.open.len(),
            ss.ends.untied.len(),
        )
    };
    let vp = viewport(app, ctx);
    if !vp.is_finite() || !vp.is_positive() {
        return;
    }
    let gap = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Palette).gap();
    let bottom = vp.bottom().min(ctx.content_rect().bottom()) - gap;
    let top = crate::shell::panel_top(app, vp, bottom, crate::frame::Side::Right);
    let frame = crate::ui_kit::Panel::palette_frame(ctx, app.ui.layout);
    let anchor = egui::pos2(vp.right() - gap - frame.stroke.width, top + frame.stroke.width);
    let shown = |key: &str| match key {
        "grid" => !opts.hide_grid,
        "snap" => !opts.no_snap,
        "slice" => opts.slice,
        "show_profile" => !opts.hide_profile,
        "show_points" => !opts.hide_points,
        "show_dimensions" => !opts.hide_dimensions,
        "show_constraints" => !opts.hide_constraints,
        "show_projected" => !opts.hide_projected,
        "show_construction" => !opts.hide_construction,
        "sketch_3d" => opts.three_d,
        _ => false,
    };
    let mut set: Option<(String, bool)> = None;
    let mut linetype = None;
    let mut construction = false;
    let mut look = false;
    let mut finish = false;
    let mut tie = false;
    let mut start = None;
    let feature = active_feature(app);
    let has_curves = !selected_curves(app).is_empty();
    let selected_types = if has_curves { selected_linetypes(app) } else { std::array::from_fn(|i| mode(app) == i as u8) };
    let panel = crate::ui_kit::Panel::new(
        app.tr_label("ui.palette_sketch_options"),
        egui::Id::new("sc_sketch_palette"),
        anchor,
        vp,
        crate::frame::Side::Right,
        frame,
    )
    .width(width(vp));
    let response = panel.window(ctx).show(ctx, |ui| {
        panel.palette(ui, bottom, top, |ui, part| match part {
            crate::ui_kit::Part::Header => {
                let status = if !solved || !geometry || remaining == 0 {
                    crate::i18n::tr_dyn(&dof_text(remaining, solved, geometry))
                } else {
                    crate::i18n::format(if remaining == 1 { "sketch.dof_left_one" } else { "sketch.dof_left" }, &[("count", &remaining.to_string())])
                };
                crate::ui_kit::status_header(
                    ui,
                    "sketch_palette",
                    &name,
                    crate::ui_kit::StatusHeader {
                        icon: "sketch",
                        text: &status,
                        complete: solved && remaining == 0 && geometry,
                        fraction: constrained_fraction(remaining, total, solved && geometry),
                    },
                );
            }
            crate::ui_kit::Part::Body => {
                let t = crate::theme::Tokens::get();
                if open_ends > 0 {
                    let text = crate::i18n::format("sketch.open_ends", &[("count", &open_ends.to_string())]);
                    ui.label(egui::RichText::new(text).size(t.font_small).color(t.warning)).on_hover_text(app.tr("ui.open_ends_tip"));
                }
                if untied_ends > 0 {
                    ui.horizontal(|ui| {
                        let text = crate::i18n::format("sketch.untied_ends", &[("count", &untied_ends.to_string())]);
                        ui.label(egui::RichText::new(text).size(t.font_small).color(t.warning)).on_hover_text(app.tr("ui.untied_ends_tip"));
                        let button =
                            crate::ui_kit::button(ui, app.tr("ui.tie_ends"), crate::ui_kit::Button::Normal).on_hover_text(app.tr("ui.tie_ends_tip"));
                        crate::scenario::publish_handle("sketch_palette:tie_ends", button.rect.center());
                        tie = button.clicked();
                    });
                }

                if let Some((family, current)) = feature {
                    let section = crate::ui_kit::section(ui, "feature", "Feature Options", true, |ui| {
                        let choices: Vec<_> = family
                            .commands
                            .iter()
                            .filter_map(|id| solidcraft_engine::find_command(id))
                            .map(|spec| crate::ui_kit::ToolChoice {
                                key: spec.id,
                                icon: spec.icon,
                                label: app.tr_label(spec.label),
                                enabled: spec.info(&app.session).enabled,
                            })
                            .collect();
                        let section = ui.vertical(|ui| crate::ui_kit::tool_choices(ui, app.tr_label(family.label), current, &choices));
                        crate::scenario::publish_rect("sketch_palette:feature_options", section.response.rect);
                        section.inner
                    });
                    start = section.flatten();
                    section_alias("sketch_palette:feature_fold", "Feature Options");
                }
                crate::ui_kit::row(ui, "Line Type", |ui| {
                    // Normal, Construction, Centerline and the Infinite line, as icons. With a
                    // selection a click converts it; without, it picks the mode new curves use.
                    let segments = LINE_TYPES.map(|(key, icon, name)| {
                        let name = app.tr_label(name);
                        let tip = if has_curves {
                            crate::i18n::format("sketch.make_selection", &[("linetype", name)])
                        } else {
                            crate::i18n::format("sketch.draw_curves", &[("linetype", name)])
                        };
                        crate::ui_kit::IconSegment { key, icon, tip }
                    });
                    let mut selected = selected_types.iter().position(|on| *on).unwrap_or(usize::MAX);
                    if crate::ui_kit::segmented_icons(ui, &mut selected, &segments, &selected_types) {
                        linetype = u8::try_from(selected).ok();
                    }
                });
                // In words: the selection's line types (several for a mixed selection), or the
                // mode new curves are drawn in.
                let names: Vec<&str> =
                    LINE_TYPES.iter().zip(selected_types).filter(|(_, on)| *on).map(|((_, _, name), _)| app.tr_label(name)).collect();
                crate::ui_kit::caption(ui, "Line Type", &names.join(", "));
                for key in ["show_constraints", "show_dimensions", "show_points", "snap"] {
                    if let Some((_, label)) = TOGGLES.iter().find(|(k, _)| *k == key)
                        && crate::ui_kit::checkbox_row(ui, key, label, shown(key))
                    {
                        set = Some((key.to_string(), !shown(key)));
                    }
                }
                construction = crate::ui_kit::checkbox_row(
                    ui,
                    "construction_mode",
                    "ui.palette_construction_mode",
                    selected_types.get(1).copied().unwrap_or(false),
                );
                crate::ui_kit::section(ui, "more", "ui.palette_more", false, |ui| {
                    for (key, label) in TOGGLES {
                        if !["show_constraints", "show_dimensions", "show_points", "snap"].contains(&key)
                            && crate::ui_kit::checkbox_row(ui, key, label, shown(key))
                        {
                            set = Some((key.to_string(), !shown(key)));
                        }
                    }
                });
                section_alias("sketch_palette:options_fold", "ui.palette_more");
            }
            crate::ui_kit::Part::Footer => {
                let button = crate::ui_kit::wide_primary_button(ui, "Finish Sketch");
                crate::scenario::publish_rect("sketch_palette:finish", button.rect);
                crate::scenario::publish_handle("sketch_palette:finish", button.rect.center());
                finish = button.clicked();
                let (at, slice) = crate::ui_kit::button_pair(
                    ui,
                    "sketch_palette",
                    [false, opts.slice],
                    [
                        crate::ui_kit::PairButton { key: "look_at", label: "ui.look_at", icon: Some("lookat") },
                        crate::ui_kit::PairButton { key: "slice", label: "Slice", icon: None },
                    ],
                );
                look = at;
                if slice {
                    set = Some(("slice".into(), !opts.slice));
                }
            }
        });
    });
    app.viewport.palette_rect = response.map(|r| r.response.rect.expand(frame.stroke.width));
    if let Some(rect) = app.viewport.palette_rect {
        crate::scenario::publish_rect("sketch_palette:outer", rect);
    }
    if let Some((key, on)) = set {
        let mut params = serde_json::Map::new();
        params.insert(key, Value::Bool(on));
        if let Err(error) = app.run("sketch.options", Value::Object(params)) {
            app.set_status(error, true);
        }
    }
    if let Some(kind) = linetype {
        if has_curves {
            set_linetype(app, kind);
        } else {
            set_mode(app, kind);
        }
        // The button click would otherwise leave the shape's dimension box without the keyboard.
        crate::sketch_dims::restore_box_focus(app, ctx);
    }
    if construction {
        toggle_construction(app);
    }
    if let Some(id) = start {
        app.start(id);
    }
    if tie && let Err(error) = app.run("sketch.loose_ends.tie", json!({})) {
        app.set_status(error, true);
    }
    if look {
        crate::dialogs::look_at_sketch_again(app);
    }
    if finish {
        app.finish_sketch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_uses_design_dimensions_and_all_options_still_run_commands() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        let palette = h.app.viewport.palette_rect.unwrap();
        assert!(
            (palette.width() - Tokens::get().sketch_palette_width).abs() < 1.0,
            "palette {palette:?}, width {}",
            Tokens::get().sketch_palette_width
        );
        let look = crate::scenario::rect_of("sketch_palette:look_at").unwrap();
        let slice = crate::scenario::rect_of("sketch_palette:slice").unwrap();
        assert_eq!(look.size(), slice.size());
        assert_eq!(look.height(), Tokens::get().form_button_height);
        assert_eq!(crate::scenario::rect_of("sketch_palette:meter").unwrap().height(), Tokens::get().space_6);
        assert!(crate::scenario::handle_at("sketch_palette:toggle:grid").is_none());
        let more = crate::scenario::handle_at("sketch_palette:options_fold").unwrap();
        h.call("ui.click", json!({"x":more.x,"y":more.y}));
        let active = h.app.session.active_sketch.unwrap();
        for (key, _) in TOGGLES {
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            let at = crate::scenario::handle_at(&format!("sketch_palette:toggle:{key}")).unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            assert_ne!(h.app.session.doc.to_json(), before, "{key}");
            assert_eq!(h.app.session.undo.len(), undo + 1, "{key}");
            assert_eq!(h.app.session.active_sketch, Some(active));
            h.app.run("edit.undo", json!({})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.to_json(), before, "{key}");
        }
        let construction = crate::scenario::handle_at("sketch_palette:toggle:construction_mode").unwrap();
        h.call("ui.click", json!({"x":construction.x,"y":construction.y}));
        assert_eq!(mode(&h.app), 1);
        h.call("ui.click", json!({"x":construction.x,"y":construction.y}));
        assert_eq!(mode(&h.app), 0);
        let infinite = crate::scenario::handle_at("dialog:Line Type:infinite").expect("the Infinite segment is drawn");
        h.call("ui.click", json!({"x":infinite.x,"y":infinite.y}));
        assert_eq!(mode(&h.app), 3);
        set_mode(&mut h.app, 0);
    }

    #[test]
    fn meter_uses_input_variables_and_solver_remaining_dof() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[10,10],"radius":5})).unwrap();
        h.settle();
        let state = h.app.session.world_state();
        let sketch = state.sketch(h.app.session.active_sketch.unwrap()).unwrap();
        assert_eq!(unconstrained_total(&sketch.sketch), 3);
        assert_eq!(sketch.report.dof, 3);
        assert_eq!(crate::scenario::count_of("sketch_palette:constrained_fraction"), Some(0.0));
        assert_eq!(constrained_fraction(2, 14, true), 1.0 - 2.0 / 14.0);
        assert_eq!(constrained_fraction(0, 0, true), 1.0);
        assert_eq!(constrained_fraction(0, 14, false), 0.0);
        assert_eq!(constrained_fraction(20, 14, true), 0.0);
    }

    #[test]
    fn ribbon_exit_finishes_the_active_sketch() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        assert!(crate::scenario::rect_of("ribbon:section:SKETCH:FINISH SKETCH").is_some());
        let at = crate::scenario::handle_at("button:sketch.finish").unwrap();
        h.call("ui.click", json!({"x":at.x,"y":at.y}));
        assert!(h.app.session.active_sketch.is_none());
    }

    #[test]
    fn palette_has_a_finish_sketch_button_that_finishes_the_sketch() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        let at = crate::scenario::handle_at("sketch_palette:finish").expect("the palette ends in Finish Sketch");
        let look = crate::scenario::rect_of("sketch_palette:look_at").expect("Look At | Slice stay");
        let slice = crate::scenario::rect_of("sketch_palette:slice").unwrap();
        let finish = crate::scenario::rect_of("sketch_palette:finish").unwrap();
        assert_eq!((finish.left(), finish.right()), (look.left(), slice.right()), "Finish {finish:?} shares the pair's inset");
        assert_eq!(finish.height(), Tokens::get().form_button_height);
        assert_eq!(look.top() - finish.bottom(), 2.0 * Tokens::get().space_6, "same spacing as the pair");
        assert!(crate::scenario::handle_at("button:sketch.finish").is_some(), "and so does the ribbon's Finish");
        h.call("ui.click", json!({"x":at.x,"y":at.y}));
        assert!(h.app.session.active_sketch.is_none());
    }

    #[test]
    fn construction_shortcut_keeps_the_drawing_tool_and_commits_its_line_type() {
        for command in ["sketch.line", "sketch.rectangle.two_point", "sketch.circle.center"] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            set_mode(&mut h.app, 0);
            h.step(&json!({"start":command})).unwrap();
            h.step(&json!({"key":"X"})).unwrap();
            assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(command));
            assert_eq!(mode(&h.app), 1);
            h.step(&json!({"click":{"sketch":[0,0]}})).unwrap();
            h.step(&json!({"move":{"sketch":[20,10]}})).unwrap();
            let before = h.app.tool.clone().unwrap();
            let history = h.app.session.undo.len();
            // The toolbar takes the same start path, even while a dimension box has focus.
            h.app.start("sketch.construction");
            h.app.start("sketch.construction");
            let after = h.app.tool.as_ref().unwrap();
            assert_eq!(after.cmd, before.cmd);
            assert_eq!(after.pts, before.pts);
            assert_eq!(after.dims_stage, before.dims_stage);
            assert_eq!(after.dims.iter().map(|d| &d.value).collect::<Vec<_>>(), before.dims.iter().map(|d| &d.value).collect::<Vec<_>>());
            assert_eq!(h.app.session.undo.len(), history);
            h.step(&json!({"click":{"sketch":[20,10]}})).unwrap();
            let state = h.app.session.world_state();
            let sketch = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
            assert!(!sketch.curves.is_empty(), "{command}");
            assert!(sketch.curves.iter().all(|c| c.construction), "{command}");
            h.app.run("edit.undo", json!({})).unwrap();
            assert!(h.app.session.world_state().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.is_empty());
            set_mode(&mut h.app, 0);
        }
    }

    #[test]
    fn palette_footer_and_scrolled_lower_option_remain_reachable_at_scale_and_resize() {
        for (scale, height) in [(1.0, 900.0), (2.0, 900.0), (2.0, 720.0)] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":1})).unwrap();
            h.call("ui.resize", json!({"width":1400,"height":height}));
            h.call("ui.prefs", json!({"ui_scale":scale}));
            h.settle();
            let at = crate::scenario::handle_at("sketch_palette:options_fold").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            let baseline = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            let vp = h.app.viewport.rect.unwrap();
            let footer = crate::scenario::rect_of("sketch_palette:look_at").unwrap();
            assert!(vp.contains_rect(footer), "scale{scale} height{height}: {footer:?} outside {vp:?}");
            let options = crate::scenario::rect_of("sketch_palette:options").unwrap();
            assert!(vp.contains_rect(options));
            h.call("ui.scroll", json!({"x":options.center().x,"y":options.center().y,"delta":-1000}));
            h.settle();
            assert_eq!(h.app.session.doc.to_json(), baseline);
            assert_eq!(h.app.session.undo.len(), undo);
            let [x, y] = h.widget("3D Sketch").unwrap();
            assert!(options.contains(egui::pos2(x as f32, y as f32)), "lower option must be visible after scrolling");
            h.call("ui.click", json!({"x":x,"y":y}));
            h.settle();
            assert!(h.app.session.world_state().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.view.three_d);
            assert_eq!(h.app.session.undo.len(), undo + 1);
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), baseline);
            h.settle();
            let at = crate::scenario::handle_at("sketch_palette:finish").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            h.settle();
            assert!(h.app.session.active_sketch.is_none(), "Finish Sketch must work at scale{scale} height{height}");
            assert_eq!(h.app.session.doc.to_json(), baseline);
        }
    }

    /// A command dialog opened while sketching used to hide the palette outright. The two now
    /// share the viewport, the dialog at the left edge and the palette at the right, and a view
    /// too narrow for both keeps the dialog alone.
    #[test]
    fn a_dialog_opened_while_sketching_keeps_the_palette_beside_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.settle();
        assert!(h.app.viewport.palette_rect.is_some(), "the palette is there while sketching");
        h.app.start("inspect.measure");
        h.settle();
        assert!(h.app.dialog.is_some(), "the command opens a dialog");
        let palette = h.app.viewport.palette_rect.expect("the palette stays on screen while a dialog is open");
        let dialog = h.app.viewport.dialog_rect.unwrap();
        assert!(!palette.intersect(dialog).is_positive(), "palette {palette:?} overlaps dialog {dialog:?}");
        assert!(dialog.left() < palette.left(), "the dialog takes the left edge: {dialog:?}, {palette:?}");
        let finish = crate::scenario::rect_of("sketch_palette:look_at").expect("Look At stays reachable");
        assert!(palette.contains_rect(finish));
        // Neither panel ever covers the other, however narrow the view gets.
        for width in [1200.0, 1000.0, 860.0, 720.0, 600.0] {
            h.call("ui.resize", json!({"width":width,"height":800}));
            h.settle();
            if let (Some(p), Some(d)) = (h.app.viewport.palette_rect, h.app.viewport.dialog_rect) {
                assert!(!p.intersect(d).is_positive(), "width {width}: palette {p:?} overlaps dialog {d:?}");
            }
            assert!(h.app.dialog.is_some(), "width {width}: the dialog stays open");
        }
    }

    #[test]
    fn linetype_buttons_follow_the_selection_and_fall_back_to_the_mode() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let curve = |r: Value| r["curves"][0].as_str().unwrap().to_string();
        let normal = curve(h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"infer":false,"connect":false})).unwrap());
        let cons = curve(h.app.run("sketch.line", json!({"points":[[0,10],[20,10]],"infer":false,"connect":false})).unwrap());
        let center = curve(h.app.run("sketch.line", json!({"points":[[0,20],[20,20]],"infer":false,"connect":false})).unwrap());
        h.app.run("sketch.construction", json!({"curves":[cons],"value":true})).unwrap();
        h.app.run("sketch.centerline", json!({"curves":[center],"value":true})).unwrap();
        // Which of Normal, Construction, Centerline and Infinite the palette shows pressed.
        let pressed = |h: &mut crate::scenario::Harness, ids: &[&String]| {
            h.app.session.selection = ids.iter().map(|id| Sel::SketchCurve { id: (*id).clone(), sketch: None }).collect();
            h.settle();
            let tree = h.output.accesskit_update.as_ref().unwrap();
            ["normal", "construction", "centerline", "infinite"]
                .map(|label| tree.nodes.iter().any(|(_, n)| n.label() == Some(label) && n.toggled() == Some(egui::accesskit::Toggled::True)))
        };
        assert_eq!(pressed(&mut h, &[&normal]), [true, false, false, false]);
        assert_eq!(pressed(&mut h, &[&cons]), [false, true, false, false]);
        assert_eq!(pressed(&mut h, &[&center]), [false, false, true, false]);
        // A mixed selection shows each of its line types.
        assert_eq!(pressed(&mut h, &[&normal, &center]), [true, false, true, false]);
        // Nothing selected: the drawing mode.
        assert_eq!(pressed(&mut h, &[]), [true, false, false, false]);
        set_mode(&mut h.app, 1);
        assert_eq!(pressed(&mut h, &[]), [false, true, false, false]);
        set_mode(&mut h.app, 3);
        assert_eq!(pressed(&mut h, &[]), [false, false, false, true]);
        set_mode(&mut h.app, 1);
        // A selection overrides the mode until it is cleared.
        assert_eq!(pressed(&mut h, &[&normal]), [true, false, false, false]);
        assert_eq!(pressed(&mut h, &[]), [false, true, false, false]);
        // The line types are also named in words under the icons.
        assert!(crate::scenario::rect_of("dialog:caption:Line Type").is_some_and(|r| r.height() > 0.0));
    }

    #[test]
    fn look_at_is_one_label_and_an_icon_button_that_animates_the_camera() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        // Point the camera somewhere clearly not looking straight down the sketch plane, so the
        // click's effect is unambiguous regardless of where `sketch.create` happened to leave
        // it. `Harness::call` settles any camera animation to completion before it returns, so
        // checking `cam_anim` right after the click (as an earlier version of this test did)
        // never sees it: the animation is always finished and cleared by then. Check where the
        // camera actually ended up instead.
        h.app.cam.yaw = 1.0;
        h.app.cam.pitch = 0.2;
        h.settle();
        // The row's own label is the only place the words "Look At" are drawn; the control
        // next to it is an icon button with no visible text of its own.
        assert!(h.widget("Look At").is_some(), "the row label must still be reachable by name");
        let at = crate::scenario::handle_at("sketch_palette:look_at").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        let (yaw, pitch) = (h.app.cam.yaw, h.app.cam.pitch);
        assert!((pitch - std::f64::consts::FRAC_PI_2).abs() < 0.01, "must look straight down the XY sketch plane: pitch={pitch}");
        assert!(yaw.abs() < 0.01, "must align with the sketch's own x axis: yaw={yaw}");
    }

    #[test]
    fn status_counts_loose_ends_and_tie_closes_the_untied_ones() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        // A square of separate lines (closed by position only) and one lone line (two open ends).
        let c = [[0, 0], [10, 0], [10, 10], [0, 10]];
        for i in 0..4 {
            h.app.run("sketch.line", json!({"points": [c[i], c[(i + 1) % 4]], "infer": false, "connect": false})).unwrap();
        }
        h.app.run("sketch.line", json!({"points": [[20, 0], [30, 5]], "infer": false, "connect": false})).unwrap();
        h.settle();
        assert!(h.widget("Open 2").is_some(), "the status counts the open ends");
        assert!(h.widget("Untied 4").is_some(), "the status counts the untied ends");
        assert!(h.widget("Tie").is_some(), "the Tie button shows its label, not its catalog key");
        let at = crate::scenario::handle_at("sketch_palette:tie_ends").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        assert!(h.widget("Untied 4").is_none(), "Tie constrains every untied end");
        assert!(h.widget("Open 2").is_some(), "open ends have nothing to tie to");
        let r = h.app.run("sketch.loose_ends", json!({})).unwrap();
        assert!(r["untied"].as_array().is_some_and(Vec::is_empty), "{r}");
    }

    #[test]
    fn options_disclosure_triangle_folds_and_unfolds_the_section() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        assert!(h.widget("Sketch Grid").is_none(), "More is collapsed by default");
        let at = crate::scenario::handle_at("sketch_palette:options_fold").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        assert!(h.widget("Sketch Grid").is_some(), "the triangle must unfold More");
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        assert!(h.widget("Sketch Grid").is_none(), "the triangle must collapse More again");
    }

    #[test]
    fn toggle_row_label_and_checkbox_both_flip_the_option() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        let active = h.app.session.active_sketch.unwrap();
        let slice_on = |h: &mut crate::scenario::Harness| h.app.session.world_state().sketch(active).unwrap().sketch.view.slice;
        assert!(!slice_on(&mut h));
        // Clicking the row's label (not just the checkbox) toggles the option, same as before
        // the checkbox and the label became separate widgets in their own column.
        let [x, y] = h.widget("Slice").unwrap();
        h.call("ui.click", json!({"x": x, "y": y}));
        h.settle();
        assert!(slice_on(&mut h), "clicking the \"Slice\" label must toggle it on");
    }

    /// The modes the Feature Options row draws pressed, in family order.
    fn pressed_modes(h: &crate::scenario::Harness, family: &crate::sketch_families::Family) -> Vec<&'static str> {
        let tree = h.output.accesskit_update.as_ref().unwrap();
        family
            .commands
            .iter()
            .copied()
            .filter(|id| {
                let label = solidcraft_engine::find_command(id).unwrap().label;
                tree.nodes.iter().any(|(_, n)| n.label() == Some(label) && n.toggled() == Some(egui::accesskit::Toggled::True))
            })
            .collect()
    }

    #[test]
    fn feature_options_show_every_mode_of_the_running_tool_and_switch_between_them() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.start("sketch.circle.center");
        h.settle();
        let circle = crate::sketch_families::family("sketch.circle.center").unwrap();
        assert_eq!(circle.commands.len(), 5, "the Circle family is the five circle modes");
        for id in circle.commands {
            assert!(crate::scenario::handle_at(&format!("sketch_palette:feature:{id}")).is_some(), "{id} needs a mode button");
        }
        assert!(h.widget("Circle").is_some(), "the row names the family");
        assert_eq!(pressed_modes(&h, circle), ["sketch.circle.center"]);
        let at = crate::scenario::handle_at("sketch_palette:feature:sketch.circle.three_tangent").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.circle.three_tangent"));
        assert_eq!(pressed_modes(&h, circle), ["sketch.circle.three_tangent"]);
        // Another family replaces the row with its own modes.
        h.app.start("sketch.rectangle.two_point");
        h.settle();
        let rect = crate::sketch_families::family("sketch.rectangle.two_point").unwrap();
        assert_eq!(rect.commands.len(), 4);
        assert_eq!(pressed_modes(&h, rect), ["sketch.rectangle.two_point"]);
        assert!(crate::scenario::handle_at("sketch_palette:feature:sketch.circle.center").is_none(), "only the running family is shown");
    }

    #[test]
    fn switching_mode_mid_draw_restarts_the_tool_and_leaves_the_design_alone() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.start("sketch.rectangle.three_point");
        h.step(&json!({"click":{"sketch":[0,0]}})).unwrap();
        h.settle();
        assert_eq!(h.app.tool.as_ref().map(|t| t.pts.len()), Some(1), "one corner is placed");
        let baseline = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let at = crate::scenario::handle_at("sketch_palette:feature:sketch.rectangle.two_point").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        let tool = h.app.tool.as_ref().unwrap();
        assert_eq!(tool.cmd, "sketch.rectangle.two_point");
        assert!(tool.pts.is_empty(), "the half-drawn rectangle is dropped, not carried into the new mode");
        assert_eq!(h.app.session.doc.to_json(), baseline, "switching modes never changes the design");
        assert_eq!(h.app.session.undo.len(), undo, "switching modes adds no undo step");
    }

    #[test]
    fn feature_options_stay_hidden_without_a_tool_that_has_other_modes() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        assert!(crate::scenario::rect_of("sketch_palette:feature_options").is_none(), "no creation tool, no section");
        // Ellipse has no siblings to switch to, so a section would hold one inert button.
        h.app.start("sketch.ellipse");
        h.settle();
        assert!(crate::sketch_families::family("sketch.ellipse").is_none());
        assert!(crate::scenario::rect_of("sketch_palette:feature_options").is_none(), "a tool outside a family shows no section");
        h.app.start("sketch.arc.three_point");
        h.settle();
        assert!(crate::scenario::rect_of("sketch_palette:feature_options").is_some(), "a family with siblings shows the section");
        // Finishing the tool takes the section away again.
        h.step(&json!({"key":"Escape"})).unwrap();
        h.settle();
        assert!(h.app.tool.is_none());
        assert!(crate::scenario::rect_of("sketch_palette:feature_options").is_none());
    }

    #[test]
    fn feature_options_disclosure_triangle_folds_and_unfolds_the_section() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.start("sketch.slot.center_to_center");
        h.settle();
        let button = "sketch_palette:feature:sketch.slot.arc_center";
        assert!(crate::scenario::handle_at(button).is_some());
        let at = crate::scenario::handle_at("sketch_palette:feature_fold").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        assert!(crate::scenario::handle_at(button).is_none(), "the triangle must fold the section");
        assert!(h.widget("Constraints").is_some(), "folding Feature Options leaves primary options visible");
        let at = crate::scenario::handle_at("sketch_palette:feature_fold").unwrap();
        h.call("ui.click", json!({"x": at.x, "y": at.y}));
        h.settle();
        assert!(crate::scenario::handle_at(button).is_some(), "the triangle must unfold it again");
    }

    #[test]
    fn dof_readout() {
        assert_eq!(dof_text(0, true, true), "Fully constrained");
        assert_eq!(dof_text(1, true, true), "1 degree of freedom");
        assert_eq!(dof_text(4, true, true), "4 degrees of freedom");
        assert_eq!(dof_text(4, false, true), "Constraints conflict");
        assert_eq!(dof_text(0, true, false), "No geometry yet");
    }

    /// #2906: a new sketch has no degree of freedom (only the fixed origin), which is not
    /// "Fully constrained"; the palette says there is no geometry until a curve is drawn.
    #[test]
    fn an_empty_sketch_is_not_reported_fully_constrained() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.settle();
        let line = |h: &crate::scenario::Harness| {
            let st = h.app.session.world_state();
            let ss = h.app.session.active_sketch.and_then(|s| st.sketch(s)).unwrap();
            dof_text(ss.report.dof, ss.report.ok(), has_geometry(&ss.sketch))
        };
        assert_eq!(line(&h), "No geometry yet");
        assert!(h.widget("No geometry yet").is_some(), "the kit header shows the empty-sketch status");
        assert!(h.widget("Fully constrained").is_none());
        assert_eq!(crate::scenario::count_of("sketch_palette:constrained_fraction"), Some(0.0));
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,5]})).unwrap();
        h.settle();
        assert_ne!(line(&h), "No geometry yet", "a drawn rectangle is geometry");
    }

    /// One Esc cancels the tool and keeps the sketch open; two quick presses finish it (the
    /// sketch is kept and listed in the timeline), also when the first press cancelled a tool.
    #[test]
    fn escape_cancels_then_a_quick_second_escape_finishes_the_sketch() {
        let apart = |h: &mut crate::scenario::Harness| h.frames(60);
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.settle();
        h.step(&json!({"start":"sketch.line"})).unwrap();
        assert!(h.app.tool.is_some());
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.tool.is_none(), "one Esc puts the tool down");
        apart(&mut h);
        h.step(&json!({"key":"Escape"})).unwrap();
        apart(&mut h);
        assert!(h.app.session.active_sketch.is_some(), "single presses keep the sketch open");

        // A quick double press finishes and keeps the sketch.
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_none(), "a double Esc finishes the sketch");
        assert_eq!(h.app.ui.tab, "MODEL");
        assert!(crate::scenario::handle_at("timeline:Sketch1").is_some(), "the sketch is kept");
        let sketch = h.app.session.execute("sketch.inspect", &json!({"sketch": 1})).unwrap();
        assert_eq!(sketch["curves"].as_array().map(Vec::len), Some(4), "{sketch}");

        // With a tool active: the first press cancels it, the second finishes.
        h.step(&json!({"click": {"handle": "timeline:Sketch1"}, "double": true})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_some());
        apart(&mut h);
        h.step(&json!({"start":"sketch.circle.center"})).unwrap();
        assert!(h.app.tool.is_some());
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.settle();
        assert!(h.app.tool.is_none() && h.app.session.active_sketch.is_none());
        assert!(crate::scenario::handle_at("timeline:Sketch1").is_some());
        // Outside a sketch a double Esc only clears, as before.
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        assert!(h.app.session.active_sketch.is_none());
    }

    /// A double Esc on a sketch left blank takes it away (no empty sketch in the browser or the
    /// timeline, one undo step) and leaves the view alone; one line is enough to keep it.
    #[test]
    fn a_quick_double_escape_discards_a_blank_sketch_and_keeps_one_with_a_line() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(3);
        let undo = h.app.session.undo.len();
        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_some());
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.settle();
        crate::scenario::check(&mut h, &json!({"sketching": false, "camera": {"yaw_deg": 0, "pitch_deg": 90}})).unwrap();
        assert_eq!(h.app.ui.tab, "MODEL");
        assert!(h.app.session.doc.features.is_empty(), "the blank sketch is gone");
        assert!(crate::scenario::handles("timeline:").iter().all(|(n, _)| !n.contains("Sketch")));
        assert!(crate::scenario::handle_at("row:Sketch1").is_none());
        assert!(h.app.session.undo.len() <= undo + 2, "creating and discarding are at most two steps");

        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.step(&json!({"click": {"plane": "XY"}})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [30, 10]]})).unwrap();
        h.settle();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.settle();
        assert!(h.app.session.active_sketch.is_none());
        assert_eq!(h.app.session.doc.features.len(), 1, "a sketch with a line is kept");
        assert!(crate::scenario::handles("timeline:").iter().any(|(n, _)| n.contains("Sketch")));
    }
}
