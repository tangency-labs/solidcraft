//! Quantise a gesture's exact translation delta in its frozen constraint frame.
use super::*;
use solidcore_ops::form_transform::SnapFrame;
use solidcraft_ui_kit::BackingTooltipExt as _;

pub(super) struct State {
    pub enabled: bool,
    pub increment: String,
}
impl Default for State {
    fn default() -> Self {
        Self { enabled: false, increment: "1 mm".into() }
    }
}
pub(super) struct Gesture {
    increment: f64,
    frame: SnapFrame,
}

fn invalid() -> String {
    solidcraft_ui_text::tr("forms.translation_snap.invalid").to_owned()
}
fn step(app: &SolveApp) -> Result<Option<f64>, String> {
    if !app.forms_prototype.translation_snap.enabled {
        return Ok(None);
    }
    let value =
        controls::number(app, &app.forms_prototype.translation_snap.increment, solidcraft_engine::doc::expr::Kind::Length).ok_or_else(invalid)?;
    if !(0.000001..=100_000.0).contains(&value) {
        return Err(invalid());
    }
    Ok(Some(value))
}
pub(super) fn pointer(app: &SolveApp, proj: &Proj, constraint: Option<Vec3>) -> Result<Option<Gesture>, String> {
    let Some(increment) = step(app)? else { return Ok(None) };
    let frame = if let Some(direction) = constraint {
        SnapFrame::axis(direction)
    } else {
        let (right, up, _) = proj.cam.basis();
        SnapFrame::plane([right, up])
    }
    .ok_or_else(invalid)?;
    Ok(Some(Gesture { increment, frame }))
}
pub(super) fn gizmo(
    app: &SolveApp,
    proj: &Proj,
    center: Vec3,
    dirs: [Vec3; 3],
    handle: crate::gizmo::Handle,
    translation: bool,
) -> Result<Option<Gesture>, String> {
    if !translation {
        return Ok(None);
    }
    let Some(increment) = step(app)? else { return Ok(None) };
    let frame = match handle {
        crate::gizmo::Handle::Axis(id) => SnapFrame::axis(*dirs.get(id).ok_or_else(invalid)?),
        crate::gizmo::Handle::Plane(id) => {
            if id > 2 {
                return Err(invalid());
            }
            SnapFrame::plane([dirs[(id + 1) % 3], dirs[(id + 2) % 3]])
        }
        crate::gizmo::Handle::Ball => {
            // Match gizmo::on_plane, including off-centre perspective projection.
            let at = proj.to_screen(center).ok_or_else(invalid)?;
            let basis = crate::gizmo::view_plane_basis(proj, at).ok_or_else(invalid)?;
            SnapFrame::plane(basis)
        }
        crate::gizmo::Handle::Ring(_) => return Ok(None),
    }
    .ok_or_else(invalid)?;
    Ok(Some(Gesture { increment, frame }))
}
pub(super) fn apply(gesture: Option<&Gesture>, delta: Vec3, free: bool) -> Result<Vec3, String> {
    if !delta.is_finite() {
        return Err(invalid());
    }
    let Some(gesture) = gesture.filter(|_| !free) else { return Ok(delta) };
    gesture.frame.quantise(delta, gesture.increment).ok_or_else(invalid)
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let active = app.forms_prototype.drag.is_some() || app.forms_prototype.gizmo_drag.is_some();
    ui.add_enabled_ui(!active, |ui| {
        let label = app.tr("forms.translation_snap.enabled").to_owned();
        let help = app.tr("forms.translation_snap.help").to_owned();
        let response = crate::ui_kit::checkbox(ui, &label, &mut app.forms_prototype.translation_snap.enabled).backing_on_hover_text(help);
        crate::scenario::publish_handle("forms_translation_snap", response.rect.center());
        if app.forms_prototype.translation_snap.enabled {
            let label = app.tr("forms.translation_snap.increment").to_owned();
            crate::ui_kit::expr_row(ui, &label, &mut app.forms_prototype.translation_snap.increment);
            if let Err(error) = step(app) {
                ui.colored_label(Tokens::get().warning, error);
            }
        }
    });
}
