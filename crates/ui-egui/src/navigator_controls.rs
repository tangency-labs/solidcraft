//! The orbit and roll arrows, Home button and view command shared by the custom body navigator.
use super::*;
use crate::icons::navigation::{arrow, curved_arrow};

pub(super) const SIZE: f32 = 112.0;
// Paint and picking use the same original coordinate space, scaled as a group.
pub(super) const DESIGN_SIZE: f32 = 164.0;
pub(super) fn navigate(app: &mut SolveApp, action: Option<&str>, dir: Option<Vec3>) {
    // Same as arrow-key steps (#8912): turn from where a glide in flight is going, so rapid
    // clicks add full turns instead of restarting from a mid-animation camera.
    let from = app.cam_anim.map_or(app.cam, |a| a.to);
    let mut params = json!({"camera": from});
    if let Some(action) = action {
        params["action"] = json!(action);
    }
    if let Some(d) = dir {
        params["direction"] = json!([d.x, d.y, d.z]);
    }
    // Preferences › Navigation: a view chosen on the navigator also frames the whole model (#10091).
    if app.preferences.navigator_fit {
        params["fit"] = json!(true);
    }
    let _ = app.run("view.navigate", params);
}

fn control(app: &mut SolveApp, ui: &mut egui::Ui, bounds: Rect, action: &str, tooltip: &str) -> bool {
    crate::scenario::publish_handle(&format!("navigator:control:{action}"), bounds.center());
    let response = ui.interact(bounds, ui.scope_id().with(("navigator", action)), Sense::click()).backing_on_hover_text(tooltip);
    let hover = response.hovered();
    if response.clicked() {
        navigate(app, Some(action), None);
    }
    // The control takes the right-click from the viewport under it: open the gizmo's menu.
    if response.secondary_clicked()
        && app.dialog.is_none()
        && let Some(at) = response.interact_pointer_pos()
    {
        crate::context_menu::open_for(app, at, crate::context_menu::Target::ViewGizmo);
    }
    hover
}

/// Preferences › Navigation: "Invert the rotation arrows" (#10254). With it on, an
/// arrow orbits the camera the way the arrow points (camera-centric)
/// instead of rotating the view toward the side the arrow sits on, so left and right, and up and
/// down, trade places. The tooltip names the action actually performed. The curved roll arrows
/// are not inverted: they name the rotation they perform, not a direction.
fn inverted_arrow(action: &str) -> Option<&'static str> {
    match action {
        "left" => Some("right"),
        "right" => Some("left"),
        "up" => Some("down"),
        "down" => Some("up"),
        _ => None,
    }
}

/// Preferences › Navigation: "Invert the curved roll arrows" (#10483). With it on, the two
/// curved arrows at the navigator's corner roll the view the other way within the screen plane: the
/// clockwise arrow rolls counterclockwise and the counterclockwise arrow rolls clockwise. The
/// arrow keeps its curl; the tooltip names the roll actually performed.
fn inverted_roll(action: &str) -> Option<&'static str> {
    match action {
        "clockwise" => Some("counterclockwise"),
        "counterclockwise" => Some("clockwise"),
        _ => None,
    }
}

/// The Home button at the top left of a gizmo centred at `c`, painted while it is `active`.
pub(super) fn home(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, scale: f32, active: bool, t: Tokens) {
    let home = Rect::from_center_size(c + vec2(-65.0, -65.0) * scale, vec2(26.0, 26.0) * scale);
    crate::scenario::publish_target("axis_view:home", home);
    let home_hover = control(app, ui, home, "home", "Home: isometric view and fit model");
    if active {
        icons::paint_icon(painter, home.shrink(4.0 * scale), "home", f32::from((if home_hover { t.accent } else { t.navigator_face }).a()) / 255.0);
    }
}

/// The four orbit arrows and the two roll arrows around a gizmo centred at `c`: painted while the gizmo is `active`.
#[allow(clippy::too_many_arguments)]
pub(super) fn arrows(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, scale: f32, active: bool, t: Tokens) {
    let (distance, button_size) = (74.0, 16.0);
    for (action, offset, direction, tooltip) in [
        ("left", vec2(-distance, 0.0), vec2(1.0, 0.0), "Orbit left 90°"),
        ("right", vec2(distance, 0.0), vec2(-1.0, 0.0), "Orbit right 90°"),
        ("up", vec2(0.0, -distance), vec2(0.0, 1.0), "Orbit up 90°"),
        ("down", vec2(0.0, distance), vec2(0.0, -1.0), "Orbit down 90°"),
    ] {
        let (action, tooltip) = match app.preferences.invert_navigator_arrows.then(|| inverted_arrow(action)).flatten() {
            Some(swapped) => (
                swapped,
                match swapped {
                    "right" => "Orbit right 90°",
                    "left" => "Orbit left 90°",
                    "down" => "Orbit down 90°",
                    _ => "Orbit up 90°",
                },
            ),
            None => (action, tooltip),
        };
        let at = c + offset * scale;
        let hovered = control(app, ui, Rect::from_center_size(at, vec2(button_size, button_size) * scale), action, tooltip);
        if active {
            arrow(painter, at, direction * scale, hovered, t);
        }
    }
    for (action, offset, size, clockwise, tooltip) in [
        ("counterclockwise", vec2(42.0, -72.0), vec2(40.0, 20.0), false, "Rotate view counterclockwise 90°"),
        ("clockwise", vec2(72.0, -42.0), vec2(20.0, 40.0), true, "Rotate view clockwise 90°"),
    ] {
        let (action, tooltip) = match app.preferences.invert_curved_roll_arrows.then(|| inverted_roll(action)).flatten() {
            Some(swapped) => (swapped, if swapped == "clockwise" { "Rotate view clockwise 90°" } else { "Rotate view counterclockwise 90°" }),
            None => (action, tooltip),
        };
        let hovered = control(app, ui, Rect::from_center_size(c + offset * scale, size * scale), action, tooltip);
        if active {
            curved_arrow(painter, c + vec2(42.0, -42.0) * scale, clockwise, hovered, t, scale);
        }
    }
}
