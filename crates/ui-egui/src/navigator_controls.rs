//! The orbit and roll arrows, Home button and view command shared by the custom body navigator.
use super::*;
use crate::icons::navigation::{arc_arrow, arrow, curved_arrow};

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

/// The four orbit arrows: action, offset from the centre, the way the arrow points, tooltip
/// (in design units, scaled with the navigator).
const ORBIT_ARROWS: [(&str, egui::Vec2, egui::Vec2, &str); 4] = [
    ("left", vec2(-74.0, 0.0), vec2(1.0, 0.0), "Orbit left 90°"),
    ("right", vec2(74.0, 0.0), vec2(-1.0, 0.0), "Orbit right 90°"),
    ("up", vec2(0.0, -74.0), vec2(0.0, 1.0), "Orbit up 90°"),
    ("down", vec2(0.0, 74.0), vec2(0.0, -1.0), "Orbit down 90°"),
];
/// The side of an orbit arrow's square button, in design units.
const ORBIT_BUTTON: f32 = 16.0;
/// The two roll arrows: action, the button's offset and size, whether it curls clockwise, tooltip.
const ROLL_ARROWS: [(&str, egui::Vec2, egui::Vec2, bool, &str); 2] = [
    ("counterclockwise", vec2(42.0, -72.0), vec2(40.0, 20.0), false, "Rotate view counterclockwise 90°"),
    ("clockwise", vec2(72.0, -42.0), vec2(20.0, 40.0), true, "Rotate view clockwise 90°"),
];

/// The axes navigator's two roll arrows: action, the arc's span (radians, screen y down, so
/// negative is up), whether it curls clockwise, tooltip. They run along the outside of the compass
/// circle in its upper-right corner, the one part of the navigator's box with room outside the
/// circle, and meet at 45°.
const AXES_ROLL_ARROWS: [(&str, (f32, f32), bool, &str); 2] = [
    ("counterclockwise", (-0.80, -1.08), false, "Rotate view counterclockwise 90°"),
    ("clockwise", (-0.77, -0.49), true, "Rotate view clockwise 90°"),
];
/// How far outside the compass circle the axes navigator's roll arrows run.
const AXES_ROLL_GAP: f32 = 6.5;

/// The button of an axes navigator roll arrow spanning `span` round `c`: the box round its arc.
fn axes_roll_button(c: Pos2, span: (f32, f32)) -> Rect {
    let r = crate::view_compass::RADIUS + AXES_ROLL_GAP;
    let at = |a: f32| c + vec2(a.cos(), a.sin()) * r;
    Rect::from_two_pos(at(span.0), at(span.1)).expand(4.0)
}

/// `p` is on one of the axes navigator's roll arrows round the compass centred at `c`.
pub(super) fn on_axes_roll_arrow(c: Pos2, p: Pos2) -> bool {
    AXES_ROLL_ARROWS.iter().any(|(_, span, _, _)| axes_roll_button(c, *span).contains(p))
}

/// The axes navigator's two roll arrows round the compass centred at `c`: painted while it is `active`.
pub(super) fn axes_roll_arrows(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, active: bool, t: Tokens) {
    for (action, span, clockwise, tooltip) in AXES_ROLL_ARROWS {
        let (action, tooltip) = match app.preferences.invert_curved_roll_arrows.then(|| inverted_roll(action)).flatten() {
            Some(swapped) => (swapped, if swapped == "clockwise" { "Rotate view clockwise 90°" } else { "Rotate view counterclockwise 90°" }),
            None => (action, tooltip),
        };
        let hovered = control(app, ui, axes_roll_button(c, span), action, tooltip);
        if active {
            arc_arrow(painter, c, crate::view_compass::RADIUS + AXES_ROLL_GAP, span, clockwise, hovered, t, 0.75);
        }
    }
}

/// The four orbit arrows and the two roll arrows around a gizmo centred at `c`: painted while the gizmo is `active`.
#[allow(clippy::too_many_arguments)]
pub(super) fn arrows(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, scale: f32, active: bool, t: Tokens) {
    for (action, offset, direction, tooltip) in ORBIT_ARROWS {
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
        let hovered = control(app, ui, Rect::from_center_size(at, vec2(ORBIT_BUTTON, ORBIT_BUTTON) * scale), action, tooltip);
        if active {
            arrow(painter, at, direction * scale, hovered, t);
        }
    }
    roll_arrows(app, ui, painter, c, scale, active, t);
}

/// The two curved roll arrows at the top right of a gizmo centred at `c`: painted while the gizmo is `active`.
fn roll_arrows(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, scale: f32, active: bool, t: Tokens) {
    for (action, offset, size, clockwise, tooltip) in ROLL_ARROWS {
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
