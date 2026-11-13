//! Quantise a gesture's exact translation delta in its frozen constraint frame.
use super::*;
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
#[derive(Clone, Copy)]
enum Frame {
    Axis(Vec3),
    Plane([Vec3; 2]),
}
pub(super) struct Gesture {
    increment: f64,
    frame: Frame,
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
fn axis(direction: Vec3) -> Result<Frame, String> {
    let direction = direction.normalized().filter(|d| d.is_finite()).ok_or_else(invalid)?;
    Ok(Frame::Axis(direction))
}
fn plane(x: Vec3, y: Vec3) -> Result<Frame, String> {
    let x = x.normalized().filter(|v| v.is_finite()).ok_or_else(invalid)?;
    let y = y.normalized().filter(|v| v.is_finite()).ok_or_else(invalid)?;
    if x.dot(y).abs() > 1e-8 {
        return Err(invalid());
    }
    Ok(Frame::Plane([x, y]))
}
pub(super) fn pointer(app: &SolveApp, proj: &Proj, constraint: Option<Vec3>) -> Result<Option<Gesture>, String> {
    let Some(increment) = step(app)? else { return Ok(None) };
    let frame = if let Some(direction) = constraint {
        axis(direction)?
    } else {
        let (right, up, _) = proj.cam.basis();
        plane(right, up)?
    };
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
        crate::gizmo::Handle::Axis(id) => axis(*dirs.get(id).ok_or_else(invalid)?)?,
        crate::gizmo::Handle::Plane(id) => {
            if id > 2 {
                return Err(invalid());
            }
            plane(dirs[(id + 1) % 3], dirs[(id + 2) % 3])?
        }
        crate::gizmo::Handle::Ball => {
            // Match gizmo::on_plane, including off-centre perspective projection.
            let at = proj.to_screen(center).ok_or_else(invalid)?;
            let normal = proj.ray(at).1.normalized().ok_or_else(invalid)?;
            let (right, _, _) = proj.cam.basis();
            let x = (right - normal * right.dot(normal)).normalized().ok_or_else(invalid)?;
            plane(x, x.cross(normal))?
        }
        crate::gizmo::Handle::Ring(_) => return Ok(None),
    };
    Ok(Some(Gesture { increment, frame }))
}
pub(super) fn apply(gesture: Option<&Gesture>, delta: Vec3, free: bool) -> Result<Vec3, String> {
    if !delta.is_finite() {
        return Err(invalid());
    }
    let Some(gesture) = gesture.filter(|_| !free) else { return Ok(delta) };
    let quantise = |distance: f64| -> Result<f64, String> {
        let count = distance / gesture.increment;
        // Above this count, adjacent integer increments cannot be represented reliably.
        if !count.is_finite() || count.abs() > 4_503_599_627_370_496.0 {
            return Err(invalid());
        }
        let snapped = count.round() * gesture.increment;
        if !snapped.is_finite() {
            return Err(invalid());
        }
        Ok(snapped)
    };
    let result = match gesture.frame {
        Frame::Axis(axis) => axis * quantise(delta.dot(axis))?,
        Frame::Plane([x, y]) => x * quantise(delta.dot(x))? + y * quantise(delta.dot(y))?,
    };
    if !result.is_finite() {
        return Err(invalid());
    }
    Ok(result)
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
