//! The dimension value window (Preferences › Sketch › Dimension values: In a window, #7246).
//! Placing a dimension with the Dimension tool, or double-clicking one, opens a small window at
//! its text, kept inside the view, instead of the box on the dimension:
//! - the value field, which takes what the box takes (numbers, units, expressions, parameter
//!   names, `w = 20`; a click on another dimension inserts its name);
//! - arrows that step a plain number by the step preferences: a length by `dimension_step_mm`,
//!   written in the unit the value is written in (none: the design's), an angle by
//!   `dimension_step_deg`. Up/Down and the mouse wheel over the field step it too (Shift: ten
//!   steps, Alt: a tenth, as in the other value fields). An expression is left as typed;
//! - a check (OK, also Enter) and a cross (Cancel, also Esc).
//!
//! While it is open the sketch shows the value: what it moves is drawn in the preview colour, and
//! why OK would refuse it shows under the field. Nothing reaches the design until OK, which
//! applies through the box's own path (`edit_box`), so undo, driven dimensions and refusals are
//! the same as there. Cancel drops this edit only: a new dimension is not added, and the
//! Dimension tool stays on for the next one. A click elsewhere drops it as well, as it does the
//! box.
//!
//! With the window the Dimension tool places a new dimension's text first (`begin_live` keeps the
//! place-first flow), and the window opens on the placed text: a window following the pointer
//! could never be reached.

use super::*;
use crate::prefs::{DIMENSION_STEP_DEG, DIMENSION_STEP_MM, DimensionInput};
use solidcraft_engine::doc::expr::{Kind, unit_info};

/// The window's area.
const AREA: &str = "sc_dim_window";

/// The window is on (Preferences › Sketch).
pub(super) fn on(app: &SolveApp) -> bool {
    app.preferences.dimension_input == DimensionInput::Window
}

/// What a step adds to a length (millimetres) and to an angle (degrees). A saved value outside
/// what Preferences accepts (a hand edit) steps by the default.
fn steps(app: &SolveApp) -> (f64, f64) {
    let or_default = |value: f64, accepted: std::ops::RangeInclusive<f64>| if accepted.contains(&value) { value } else { 1.0 };
    (or_default(app.preferences.dimension_step_mm, DIMENSION_STEP_MM), or_default(app.preferences.dimension_step_deg, DIMENSION_STEP_DEG))
}

/// Whether a dimension's value is a length or an angle, from its parameter's unit.
fn kind(doc: &solidcraft_engine::doc::Document, param: &str) -> Kind {
    doc.param(param).and_then(|p| unit_info(&p.unit)).map_or(Kind::Length, |(kind, _)| kind)
}

/// `text` moved by `steps` steps (negative: down) of `step_mm` (a length) or `step_deg` (an
/// angle). A plain number keeps the unit it is written in (none: `design_unit` for a length,
/// degrees for an angle) and its decimal mark, gaining the decimal places the step needs in that
/// unit. `None` for anything else: an expression is left as typed.
pub(crate) fn stepped(text: &str, steps: f64, kind: Kind, design_unit: &str, step_mm: f64, step_deg: f64) -> Option<String> {
    crate::value_step::shifted(text, |written| {
        let unit = match written {
            "" if kind == Kind::Angle => "deg",
            "" => design_unit,
            "\"" | "″" => "in",
            "'" | "′" => "ft",
            unit => unit,
        };
        let (unit_kind, per_unit) = unit_info(unit)?;
        let step = match (kind, unit_kind) {
            (Kind::Length, Kind::Length) => step_mm,
            (Kind::Angle, Kind::Angle) => step_deg.to_radians(),
            _ => return None,
        };
        let amount = step / per_unit * steps;
        (amount.is_finite() && per_unit > 0.0).then(|| (amount, places(amount)))
    })
}

/// The fewest decimal places, at most six, that write `amount` as it is.
fn places(amount: f64) -> usize {
    let a = amount.abs();
    (0..6)
        .find(|p| {
            let scale = 10f64.powi(*p);
            ((a * scale).round() / scale - a).abs() <= a * 1e-9
        })
        .map_or(6, |p| p as usize)
}

/// What OK would make of an existing dimension's typed value, worked out once per text: the
/// sketch it gives, or the refusal OK would show. It runs OK's own command on a scratch copy of
/// the session, so nothing reaches the design.
struct Existing {
    doc: Arc<solidcraft_engine::doc::Document>,
    param: String,
    expr: String,
    sketch: Option<Arc<solidcraft_engine::doc::SolvedSketch>>,
    error: Option<String>,
}

thread_local! {
    static EXISTING: RefCell<Option<Existing>> = const { RefCell::new(None) };
    /// Where the open window sits, for which parameter, and the frame it was last drawn in. It
    /// stays where it opened while it is open, so its arrows stay under the pointer while the
    /// stepped value moves the dimension's text.
    static ANCHOR: RefCell<Option<(String, u64, Pos2)>> = const { RefCell::new(None) };
}

/// Forget the worked-out value (the edit closed).
pub(super) fn forget() {
    EXISTING.with(|e| *e.borrow_mut() = None);
}

fn existing(app: &SolveApp, param: &str, expr: &str) -> (Option<Arc<solidcraft_engine::doc::SolvedSketch>>, Option<String>) {
    EXISTING.with(|cell| {
        let mut cell = cell.borrow_mut();
        let current = cell.as_ref().is_some_and(|x| Arc::ptr_eq(&x.doc, &app.session.doc) && x.param == param && x.expr == expr);
        if !current {
            let expression = crate::length_input::expression(&app.session.doc, expr, kind(&app.session.doc, param));
            let mut scratch = app.session.scratch();
            let (sketch, error) = match scratch.execute("parameters.change", &json!({"name": param, "expression": expression})) {
                Ok(_) => (app.session.active_sketch.and_then(|id| scratch.world_state().sketches.iter().find(|s| s.feature == id).cloned()), None),
                Err(e) => (None, Some(shown_error("parameters.change", &e.to_string()))),
            };
            *cell = Some(Existing { doc: app.session.doc.clone(), param: param.to_string(), expr: expr.to_string(), sketch, error });
        }
        cell.as_ref().map(|x| (x.sketch.clone(), x.error.clone())).unwrap_or_default()
    })
}

/// Draw what the open window's value moves in the sketch, in the preview colour, over the sketch
/// as it is. A value OK would refuse shows nothing.
pub(super) fn paint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !on(app) {
        return;
    }
    let Some((param, expr, draft)) = EDIT.with(|edit| {
        let edit = edit.borrow();
        let e = edit.as_ref()?;
        let draft = e.draft.as_ref().map(|d| (d.sketch, d.preview.clone(), d.error.is_some()));
        Some((e.param.clone(), e.expr.clone(), draft))
    }) else {
        return;
    };
    let Some(sketch) = app.session.active_sketch else { return };
    let after = match draft {
        Some((id, preview, false)) => preview.sketches.iter().find(|s| s.feature == id).cloned(),
        Some((_, _, true)) => None,
        None => existing(app, &param, &expr).0,
    };
    let state = app.session.world_state();
    let changes = after.and_then(|after| crate::preview::sketch_changes(state.sketch(sketch), &after));
    let mut budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
    let curves = changes.map_or(0, |changes| crate::preview::paint_sketch(&changes, painter, proj, &mut budget));
    crate::scenario::publish_count("dimwin:preview_curves", curves as f64);
}

/// Where the window goes: where it already is while it stays open, else under the dimension's
/// text `at` (over it where the view has no room below); always inside the view's free area.
/// `size` is the window's as drawn last frame.
fn place(app: &SolveApp, ctx: &egui::Context, param: &str, at: Pos2, size: egui::Vec2) -> Pos2 {
    let frame = ctx.cumulative_frame_nr();
    let view = app.viewport.layout.as_ref().map(|layout| layout.drawable).filter(|r| r.is_finite() && r.is_positive());
    ANCHOR.with(|anchor| {
        let mut anchor = anchor.borrow_mut();
        let open = anchor.as_ref().filter(|(open, drawn, _)| open == param && drawn.saturating_add(1) >= frame).map(|x| x.2);
        let mut pos = open.unwrap_or_else(|| {
            let gap = Tokens::get().space_12;
            let below = Pos2::new(at.x - size.x * 0.5, at.y + gap);
            if view.is_some_and(|rect| below.y + size.y > rect.bottom()) { Pos2::new(below.x, at.y - gap - size.y) } else { below }
        });
        if let Some(rect) = view {
            pos.x = pos.x.clamp(rect.left(), (rect.right() - size.x).max(rect.left()));
            pos.y = pos.y.clamp(rect.top(), (rect.bottom() - size.y).max(rect.top()));
        }
        *anchor = Some((param.to_string(), frame, pos));
        pos
    })
}

/// The check or the cross in `rect`, its icon in colour as the view's own OK and Cancel draw it
/// (`view_confirm`): hover, the pointing hand, its tooltip, and its handle `key` for scenarios and
/// the control channel. True when clicked.
fn button(ui: &mut egui::Ui, rect: Rect, key: &str, tip: &str, icon: &str) -> bool {
    let t = Tokens::get();
    let response = ui.interact(rect, ui.id().with(key), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    crate::icons::paint_icon(ui.painter(), rect.shrink(t.space_4), icon, 1.0);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    crate::scenario::publish_target(key, rect);
    response.on_hover_text(tip).clicked()
}

/// Draw the window for the open edit `e` at its dimension's text `at` (screen) and answer it:
/// `Some(true)` applies it (the check, Enter), `Some(false)` drops it; the second value says
/// whether a new dimension's picks go too (the cross, Esc), as Esc in the box does.
pub(super) fn show(app: &mut SolveApp, ctx: &egui::Context, e: &mut Edit, at: Pos2) -> (Option<bool>, bool) {
    let t = Tokens::get();
    let id = egui::Id::new("sc_dim_edit");
    let area = egui::Id::new(AREA);
    let (kind, error) = match &e.draft {
        Some(draft) => (kind(&draft.preview_doc, &e.param), draft.error.clone()),
        None => (kind(&app.session.doc, &e.param), existing(app, &e.param, &e.expr).1),
    };
    let (step_mm, step_deg) = steps(app);
    let unit = crate::length_input::unit(&app.session.doc).to_string();
    let step = |text: &str, steps: f64| stepped(text, steps, kind, &unit, step_mm, step_deg);
    let step_text = if kind == Kind::Angle {
        format!("{:.*}°", usize::from(app.preferences.angle_decimals), step_deg)
    } else {
        crate::prefs::format_length(step_mm, &app.session.doc.units, app.preferences.length_decimals)
    };
    let last = ctx.memory(|m| m.area_rect(area));
    let width = t.space_24 * 8.0;
    let guess = egui::vec2(width + t.space_6 * 2.0, t.control_height * 2.0 + t.space_6 * 3.0);
    let anchor = place(app, ctx, &e.param, at, last.map_or(guess, |r| r.size()));
    // A click on the window (an arrow) takes the keyboard from its field, as any click away does;
    // the field takes it back instead of closing.
    let on_window = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| last.is_some_and(|r| r.contains(p))));
    let mut answer: Option<bool> = None;
    let mut escaped = false;
    let mut by = 0.0;
    Container::new(ctx, Surface::Canvas).area(area).fixed_pos(anchor).order(egui::Order::Foreground).show(ctx, |ui| {
        Container::popup_frame(ui.style()).fill(t.dialog_body).inner_margin(t.space_6).show(ui, |ui| {
            crate::ui_kit::scope(ui, |ui| {
                ui.set_width(width);
                // The parameter's name, then the check and the cross.
                let (title, _) = ui.allocate_exact_size(egui::vec2(width, t.control_height), egui::Sense::hover());
                let name = crate::i18n::format("dialog.dimension_name", &[("name", &e.param)]);
                ui.scope_builder(egui::UiBuilder::new().max_rect(title).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                    crate::ui_kit::text(ui, &name, crate::ui_kit::Text::Inline).on_hover_text(crate::i18n::tr("dialog.dimension_insert_hint"));
                });
                let side = t.control_height;
                let cancel = Rect::from_min_size(egui::pos2(title.right() - side, title.top()), egui::vec2(side, side));
                let ok = cancel.translate(egui::vec2(-(side + t.space_2), 0.0));
                if button(ui, ok, "dimwin:ok", crate::i18n::tr("ui.heading_ok"), "finish") {
                    answer = Some(true);
                }
                if button(ui, cancel, "dimwin:cancel", crate::i18n::tr("ui.cancel"), "close") {
                    answer = Some(false);
                    escaped = true;
                }
                // The value, with its arrows at its right (the kit's own up and down arrows).
                let (row, _) = ui.allocate_exact_size(egui::vec2(width, t.control_height), egui::Sense::hover());
                let arrows = 2.0 * (t.form_close_size - t.space_4) + t.space_2;
                let (r, up, down) = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                        ui.spacing_mut().item_spacing.x = t.space_2;
                        let field = egui::vec2((row.width() - arrows - t.space_2).max(t.space_24), t.control_height);
                        let r = ui.allocate_ui(field, |ui| crate::ui_kit::cell_editor(ui, id, &mut e.expr, "", false)).inner;
                        crate::scenario::publish_target("dimwin:value", r.rect);
                        if r.has_focus() {
                            e.selection = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range());
                        }
                        if crate::expression_complete::complete(ui, &r, &mut e.expr) {
                            e.selection = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range());
                        }
                        let can_step = step(&e.expr, 1.0).is_some();
                        let tip = |id: &str| {
                            if can_step {
                                crate::i18n::format(id, &[("step", &step_text)])
                            } else {
                                crate::i18n::tr("dialog.dimension_step_expression").to_string()
                            }
                        };
                        let up = crate::ui_kit::compact_button(ui, crate::ui_kit::MOVE_UP, &tip("dialog.dimension_step_up"), can_step, false);
                        crate::scenario::publish_target("dimwin:up", up.rect);
                        let down = crate::ui_kit::compact_button(ui, crate::ui_kit::MOVE_DOWN, &tip("dialog.dimension_step_down"), can_step, false);
                        crate::scenario::publish_target("dimwin:down", down.rect);
                        (r, up.clicked(), down.clicked())
                    })
                    .inner;
                if up {
                    by += 1.0;
                }
                if down {
                    by -= 1.0;
                }
                // Up/Down in the field and the wheel over it or its arrows step too.
                if r.has_focus() {
                    by += ui.input(crate::value_step::key_tenths) as f64 / 10.0;
                }
                let wheel = id.with("dimwin_wheel");
                if ui.rect_contains_pointer(row) {
                    by += crate::value_step::wheel_tenths(ui, wheel) as f64 / 10.0;
                    crate::navigation::claim_wheel(ui.ctx());
                } else {
                    ui.data_mut(|d| d.remove::<f32>(wheel));
                }
                if let Some(error) = &error {
                    crate::ui_kit::text(ui, error, crate::ui_kit::Text::Report("dimension_error", true));
                }
                if e.fresh == FRESH {
                    select_all(ctx, id, e);
                }
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let field = if r.lost_focus() && enter {
                    Some(true)
                } else if e.fresh > 0 || on_window || by != 0.0 {
                    if !r.has_focus() {
                        r.request_focus();
                    }
                    None
                } else if r.lost_focus() {
                    Some(false)
                } else {
                    None
                };
                answer = answer.or(field);
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    answer = Some(false);
                    escaped = true;
                }
            });
        });
    });
    if let Some(rect) = ctx.memory(|m| m.area_rect(area)) {
        crate::scenario::publish_rect("dimwin", rect);
    }
    if by != 0.0
        && answer.is_none()
        && let Some(stepped) = step(&e.expr, by)
    {
        e.expr = stepped;
        // The whole new value is selected, so typing replaces it.
        select_all(ctx, id, e);
        ctx.request_repaint();
    }
    if answer.is_some() {
        forget();
    }
    (answer, escaped)
}

/// Select the whole value in the field, so typing replaces it.
fn select_all(ctx: &egui::Context, id: egui::Id, e: &mut Edit) {
    let n = e.expr.chars().count();
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
    e.selection = state.cursor.char_range();
    state.store(ctx, id);
}
