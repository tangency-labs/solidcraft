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
    pub symbol: Option<solidcraft_engine::geom::weld::Weld>,
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<crate::selection::SelInput>)> {
    (id == "weld.create").then(|| {
        let mut state = WeldDialog::default();
        state.panel.number = next_number(app);
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
    let mut size = serde_json::Map::new();
    for (key, value) in [
        ("a", &p.size.a),
        ("z", &p.size.z),
        ("z1", &p.size.z1),
        ("h", &p.size.h),
        ("s", &p.size.s),
        ("d", &p.size.d),
        ("fill", &p.size.fill),
        ("collar", &p.size.collar),
        ("t", &p.size.t),
        ("w", &p.size.w),
    ] {
        if !value.trim().is_empty() {
            size.insert(key.into(), json!(value));
        }
    }
    let weld_type =
        p.weld_type.info().label.strip_prefix("ui.weld.type.").ok_or_else(|| solidcraft_ui_text::tr("ui.weld.create.invalid_type").to_string())?;
    Ok(json!({
        "weld_type":weld_type, "picks":{}, "size":size,
        "face":match p.face {Face::Flat=>"flat",Face::Convex=>"convex",Face::Concave=>"concave"},
        "size_by_leg":p.size_by_leg,
        "blend_toes":p.blend_toes,
        "sheet_picked":p.sheet_picked,
        "tangent_chain":p.tangent_chain,
        "flip_side":p.flip_side,
        "spans":p.spans,
        "both_sides":p.both_sides,
        "staggered":p.staggered,
        "sharp_mitre":p.sharp_mitre,
        "merge_crossing":p.merge_crossing,
        "different_halves":p.different_halves,
        "tack":p.tack,
        "field_weld":p.field_weld,
        "all_around":p.all_around,
        "passes_shown":p.passes_shown,
        "penetration_shown":p.penetration_shown,
        "root_side":p.root_side,
        "ends":p.ends,
        "process":p.process,
        "texture":p.texture,
        "spot_look":p.spot_look,
        "reference_plane":p.reference_plane,
        "pattern_count":p.pattern_count,
        "pattern_pitch":p.pattern_pitch,
        "edge_distance":p.edge_distance,
        "density":p.density,
        "overlap":p.overlap,
        "symbol":state.symbol
    }))
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

/// The existing panel owns its kit backing; the host only supplies floating placement and scrolling.
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
    let width = t.form_card_width + 2.0 * (t.space_24 + t.space_16);
    let position = egui::pos2((vp.right() - width - t.space_16).max(vp.left()), vp.top() + t.space_16);
    let response = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Feature)
        .area(egui::Id::unique("weld_create_panel"))
        .default_pos(position)
        .movable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height((vp.height() - t.space_24).max(t.control_height))
                .show(ui, |ui| {
                    let event =
                        if let Kind::Weld(state) = &mut d.kind { crate::weld_panel::show(ui, &mut state.panel, &t) } else { WeldPanelEvent::None };
                    crate::dialogs::error_rows(app, ctx, &mut d, ui);
                    event
                })
                .inner
        });
    app.viewport.dialog_rect = Some(response.response.rect);
    crate::scenario::publish_rect("dialog:window", response.response.rect);
    let requested = if std::mem::take(&mut d.confirm) { WeldPanelEvent::Done } else { response.inner };
    if event(app, &mut d, requested) {
        app.dialog = Some(d);
    }
}

#[cfg(test)]
#[path = "dialogs_weld_tests.rs"]
mod tests;
