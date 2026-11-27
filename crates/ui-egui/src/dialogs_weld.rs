//! Adapter from the local weld editor to saved, undoable weld graphic commands.
use crate::{
    SolveApp,
    dialogs::{Dialog, Kind},
    weld_panel::{Face, Picks, WeldPanelEvent, WeldPanelState},
};
use serde_json::{Value, json};
use solidcraft_theme::Tokens;

#[derive(Clone, Debug, Default)]
pub struct WeldDialog {
    pub panel: WeldPanelState,
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<crate::selection::SelInput>)> {
    (id == "weld.create").then(|| {
        let mut state = WeldDialog::default();
        state.panel.number = next_number(app);
        state.panel.symbol_standard = if solidcraft_engine::doc::weld::SYMBOL_STANDARDS.contains(&app.preferences.weld_symbol_standard.as_str()) {
            app.preferences.weld_symbol_standard.clone()
        } else {
            "iso2553_2019_a".into()
        };
        (Kind::Weld(Box::new(state)), Vec::new())
    })
}
fn next_number(app: &SolveApp) -> String {
    format!("W{}", app.session.doc.welds.iter().map(|w| w.number).max().unwrap_or(0).saturating_add(1))
}

/// Chip strings are display labels, not stable picks. Never turn them into fabricated references.
pub fn params(state: &WeldDialog) -> Result<Value, String> {
    let p = &state.panel;
    if [&p.picks.line, &p.picks.groove, &p.picks.point, &p.picks.hole, &p.picks.ring, &p.picks.area].iter().any(|p| !p.is_empty()) {
        return Err(solidcraft_ui_text::tr("ui.weld.create.local_picks").into());
    }
    let weld_type =
        p.weld_type.info().label.strip_prefix("ui.weld.type.").ok_or_else(|| solidcraft_ui_text::tr("ui.weld.create.invalid_type").to_string())?;
    let mut size = serde_json::Map::new();
    for (key, value) in [
        ("a", &p.size.a),
        ("z", &p.size.z),
        ("z1", &p.size.z1),
        ("z2", &p.size.z2),
        ("h", &p.size.h),
        ("s", &p.size.s),
        ("d", &p.size.d),
        ("n", &p.size.n),
        ("l", &p.size.l),
        ("e", &p.size.e),
        ("t", &p.size.t),
        ("w", &p.size.w),
    ] {
        use crate::weld_panel::WeldType;
        let visible = match p.weld_type {
            WeldType::Fillet => key == if p.size_by_leg { "z" } else { "a" } || matches!(key, "z1" | "z2" | "s"),
            WeldType::Butt => key != "s" || p.partial_penetration,
            WeldType::Tack => key == "l" || (key == "e" && p.tack_mode == 0) || (key == "n" && p.tack_mode == 1),
            WeldType::Seam | WeldType::Surfacing | WeldType::Plug | WeldType::Stud | WeldType::Spot | WeldType::Edge => true,
        };
        if visible && solidcraft_engine::doc::weld::size_keys(weld_type).contains(&key) && !value.trim().is_empty() {
            size.insert(key.into(), json!(value));
        }
    }
    let ends = ["square", "tapered", "rounded"].get(p.ends).copied();
    let spot_look = ["marks", "ball", "marks_ball", "marks_pin", "pin", "dent"].get(p.spot_look).copied();
    let root = ["as_welded", "melt_through", "backing_run"].get(p.root).copied().unwrap_or("as_welded");
    let tack_mode = ["pitch", "count", "points"].get(p.tack_mode).copied().unwrap_or("pitch");
    let mut result = json!({
        "weld_type":weld_type, "picks":{}, "size":size,
        "symbol_standard":p.symbol_standard,
        "groove":solidcraft_engine::doc::weld::GROOVES.get(p.groove).copied().unwrap_or("v"),
        "penetration":if p.partial_penetration {"partial"} else {"full"},
        "root":root,
        "tack_mode":tack_mode,
        "root_gap":(!p.root_gap.trim().is_empty()).then_some(&p.root_gap),
        "add_fillet":p.add_fillet,
        "face":match p.face {Face::Flat=>"flat",Face::Convex=>"convex",Face::Concave=>"concave"},
        "size_by_leg":p.size_by_leg,
        "blend_toes":p.blend_toes,
        "sheet_picked":p.sheet_picked,
        "tangent_chain":p.tangent_chain,
        "spans":if p.spans {
            let count = p.span_count.trim().parse::<u32>().map_err(|_| solidcraft_ui_text::tr("ui.weld.create.invalid_spans").to_string())?;
            Some(json!({"count":count,"length_mm":p.span_length,"pitch_mm":p.span_pitch}))
        } else {None},
        "both_sides":p.both_sides,
        "staggered":p.staggered,
        "sharp_mitre":p.sharp_mitre,
        "merge_crossing":p.merge_crossing,
        "different_halves":p.different_halves,
        "field_weld":p.field_weld,
        "all_around":p.all_around,
        "passes_shown":p.passes_shown,
        "penetration_shown":p.penetration_shown,
        "side":if p.side == 1 {"other"} else {"arrow"},
        "ends":ends,
        "process":p.process,
        "spot_look":spot_look,
        "reference_plane":"sketch",
        "density":(!p.density.trim().is_empty()).then_some(&p.density),
        "overlap":(!p.overlap.trim().is_empty()).then_some(&p.overlap),
        "symbol":crate::weld_panel::symbol::record(p).ok()
    });
    if let Some(object) = result.as_object_mut() {
        object.retain(|key, value| key == "symbol" || !value.is_null());
    }
    Ok(result)
}

/// Handle the same events for the live host and headless lifecycle tests.
pub(crate) fn event(app: &mut SolveApp, d: &mut Dialog, event: WeldPanelEvent) -> bool {
    match event {
        WeldPanelEvent::Cancel => return false,
        WeldPanelEvent::Done | WeldPanelEvent::NewBead => {
            let Kind::Weld(state) = &d.kind else { return true };
            let result = params(state).and_then(|p| app.run("weld.create", p));
            match result {
                Ok(_) if event == WeldPanelEvent::Done => return false,
                Ok(_) => {
                    if let Kind::Weld(state) = &mut d.kind {
                        state.panel.picks = Picks::default();
                        state.panel.notices.clear();
                        state.panel.number = next_number(app);
                    }
                    d.error = None;
                }
                Err(error) => d.refuse(app, error),
            }
        }
        WeldPanelEvent::DefineSymbol => d.refuse(app, app.tr("ui.weld.create.symbol_unavailable").into()),
        WeldPanelEvent::Pick(_) | WeldPanelEvent::PickAgain(_) => d.refuse(app, app.tr("ui.weld.create.local_picks").into()),
        _ => {}
    }
    true
}

/// Use the same Panel placement and surface as the other tool dialogs.
pub fn show(app: &mut SolveApp, ctx: &egui::Context, mut d: Dialog) {
    // Undo or a programmatic create can change the next number while the form is open.
    if let Kind::Weld(state) = &mut d.kind {
        state.panel.number = next_number(app);
    }
    let t = Tokens::get();
    let vp = crate::agent_chat::tool_space(app, crate::shell::content_rect(app, app.viewport.rect.unwrap_or_else(|| ctx.content_rect())));
    if !vp.is_finite() || !vp.is_positive() {
        app.dialog = Some(d);
        return;
    }
    let placement = crate::shell::editor_space(app, vp);
    let side = crate::frame::Side::Right;
    let top = crate::shell::panel_top(app, placement, placement.bottom(), side);
    let drag_bounds = crate::shell::tool_drag_rect(app, vp);
    let docked = egui::pos2(side.anchor_x(placement), top);
    let moved = crate::frame::dock_offset(ctx, side.dialog_offset_key());
    let anchor = crate::frame::offset_anchor(docked, moved, drag_bounds, side);
    let frame = crate::ui_kit::Panel::feature_frame(ctx, app.ui.layout, side);
    let panel = crate::ui_kit::Panel::new(app.tr("ui.weld.title"), egui::Id::unique("weld_create_panel"), anchor, drag_bounds, side, frame)
        .width(t.form_card_width + 2.0 * (t.space_24 + t.space_16));
    let frame = panel.positioned_frame(ctx);
    let gap = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Feature).gap();
    let size = egui::vec2(
        (panel.width.min(drag_bounds.width()) - frame.total_margin().sum().x).max(0.0),
        (drag_bounds.bottom() - panel.bounded_anchor(ctx).y - gap - frame.total_margin().sum().y).max(0.0),
    );
    let mut painted = false;
    let mut local = if let Kind::Weld(state) = &mut d.kind { std::mem::take(&mut state.panel) } else { WeldPanelState::default() };
    let response = panel.window(ctx).fixed_size(size).show(ctx, |ui| {
        painted = ui.is_visible() && !ui.is_sizing_pass();
        let title = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(size.x, t.form_header_height));
        crate::frame::drag_dock(ui, title, side.dialog_offset_key(), moved, panel.bounded_anchor(ctx) - docked);
        crate::weld_panel::show_contents(ui, &mut local, &t, size.y, |ui| crate::dialogs::error_rows(app, ctx, &mut d, ui))
    });
    if let Kind::Weld(state) = &mut d.kind {
        state.panel = local;
    }
    app.viewport.dialog_rect = response.as_ref().filter(|_| painted).map(|r| r.response.rect);
    if let Some(rect) = app.viewport.dialog_rect {
        crate::scenario::publish_rect("dialog:window", rect);
        crate::scenario::publish_rect("dialog:outer", rect);
        crate::scenario::publish_target("dialog:card", rect);
    }
    let requested = if std::mem::take(&mut d.confirm) { WeldPanelEvent::Done } else { response.and_then(|r| r.inner).unwrap_or_default() };
    if event(app, &mut d, requested) {
        app.dialog = Some(d);
    }
}

#[cfg(test)]
#[path = "dialogs_weld_tests.rs"]
mod tests;
