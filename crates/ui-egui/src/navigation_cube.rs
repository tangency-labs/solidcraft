//! Original, code-drawn navigation cube with chamfered edges and corners: every one of its
//! 26 views (6 faces, 12 edges, 8 corners) is one flat facet that picks it.
use super::*;
use crate::icons::navigation::{
    CURVED_ARROW_RADIUS, Facet, ORBIT_ARROW_WING, arrow, curved_arrow, curved_arrow_bounds, curved_arrow_contains, paint_cube,
};
use solidcraft_engine::geom::Ring;

pub(super) const SIZE: f32 = 112.0;
// Paint and picking use the same original coordinate space, scaled as a group.
pub(super) const DESIGN_SIZE: f32 = 164.0;
const HALF: f64 = 40.0;
// How deep an edge chamfer bites into its two faces, and how far a corner chamfer reaches
// along each face edge, as fractions of the half side.
const EDGE_CUT: f64 = 0.2;
const CORNER_CUT: f64 = 0.36;
// Half side of a face's octagon, and where its cut corners start.
const FACE: f64 = 1.0 - EDGE_CUT;
const CUT: f64 = FACE - CORNER_CUT;

fn camera(app: &SolveApp) -> Camera {
    let mut cam = app.cam;
    if !crate::render_workspace::active(app) {
        cam.fov = app.view_fov();
    }
    cam
}

fn inverse_focal_length(cam: &Camera) -> f64 {
    // The field of view spans the whole navigator, including the margin around its cube.
    let half_height = f64::from(DESIGN_SIZE) / (2.0 * HALF);
    if cam.fov.is_finite() { (cam.fov.clamp(0.0, std::f64::consts::FRAC_PI_2) * 0.5).tan() / half_height } else { 0.0 }
}

fn project(cam: &Camera, v: Vec3, scale: f64) -> egui::Vec2 {
    let (r, u, b) = cam.basis();
    let v = cam.from_frame(v);
    // A unit cube has its own camera, independent of model size and zoom. Place its eye
    // one unit beyond the focal distance: the front face keeps its size beside the arrows.
    // At the maximum 90-degree field of view the eye is still outside the cube's sphere,
    // so every vertex stays in front of it. Zero fov gives the original parallel projection.
    let denominator = 1.0 + inverse_focal_length(cam) * (1.0 - v.dot(b));
    vec2((v.dot(r) * scale / denominator) as f32, -(v.dot(u) * scale / denominator) as f32)
}

/// Local projected axes at a label's centre; a direction alone has no perspective position.
fn project_direction(cam: &Camera, at: Vec3, direction: Vec3) -> egui::Vec2 {
    let (r, u, b) = cam.basis();
    let inverse_focal = inverse_focal_length(cam);
    let direction = cam.from_frame(direction);
    let denominator = 1.0 + inverse_focal * (1.0 - cam.from_frame(at).dot(b));
    let parallel = vec2(direction.dot(r) as f32, -direction.dot(u) as f32);
    (parallel + project(cam, at, 1.0) * (inverse_focal * direction.dot(b)) as f32) / denominator as f32
}

fn direction_label(d: Vec3) -> String {
    [(d.z, "BOTTOM", "TOP"), (d.y, "FRONT", "BACK"), (d.x, "LEFT", "RIGHT")]
        .into_iter()
        .filter_map(|(v, neg, pos)| {
            if v < 0.0 {
                Some(neg)
            } else if v > 0.0 {
                Some(pos)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The view's name in the program's language; automation handles keep `direction_label`.
///
/// Each translated word arrives in display order already, but egui has no bidirectional layout, so
/// the joined words are put in display order as a whole: in Arabic or Hebrew the first word ends up
/// at the right. This assumes each heading is one word, as the shipped ones are; text without
/// right-to-left letters comes back unchanged.
fn view_name(d: Vec3) -> String {
    let joined = direction_label(d)
        .split(" · ")
        .map(|part| crate::i18n::tr(&format!("ui.heading_{}", part.to_ascii_lowercase())).to_string())
        .collect::<Vec<_>>()
        .join(" · ");
    crate::bidi::visual(&joined).into_owned()
}

fn facets(cam: &Camera, c: Pos2, half: f64) -> Vec<Facet> {
    if !cam.is_valid() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let inverse_focal = inverse_focal_length(cam);
    let inverse_eye_distance = inverse_focal / (1.0 + inverse_focal);
    let mut add = |d: Vec3, normal: Vec3, mut vertices: Vec<Vec3>, axes: Option<(Vec3, Vec3)>| {
        let depth = cam.from_frame(normal).dot(cam.back());
        // In perspective a facet must look toward the finite eye, not merely toward the
        // parallel view direction. Face planes are one unit from the origin, chamfer planes
        // farther, so each facet turns away at its own angle.
        if depth <= normal.dot(vertices[0]) * inverse_eye_distance + 1e-5 {
            return;
        }
        let centre = vertices.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) / vertices.len() as f64;
        let projected_centre = project(cam, centre, half);
        let at = c + projected_centre;
        vertices.sort_by(|a, b| {
            let a = project(cam, *a, half) - projected_centre;
            let b = project(cam, *b, half) - projected_centre;
            a.y.atan2(a.x).total_cmp(&b.y.atan2(b.x))
        });
        // Neighbouring facets meet at an angle, so every boundary is a visible crease.
        let outer_edges = vec![true; vertices.len()];
        let points = vertices.into_iter().map(|v| c + project(cam, v, half)).collect();
        out.push(Facet {
            direction: d,
            points,
            outer_edges,
            centre: at,
            label: view_name(d),
            text_axes: axes.map(|(r, u)| (project_direction(cam, centre, r), -project_direction(cam, centre, u))),
            depth,
        });
    };
    let faces = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (-Vec3::X, -Vec3::Y, Vec3::Z),
        (Vec3::Y, -Vec3::X, Vec3::Z),
        (-Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (-Vec3::Z, Vec3::X, -Vec3::Y),
    ];
    // Each face is an octagon: its square with the four corners cut off.
    for (n, r, u) in faces {
        let corners = [(FACE, CUT), (CUT, FACE)].into_iter().flat_map(|(a, b)| [(a, b), (a, -b), (-a, b), (-a, -b)]);
        add(n, n, corners.map(|(a, b)| n + r * a + u * b).collect(), Some((r, u)));
    }
    // Two perpendicular faces share an edge chamfer, three share a corner chamfer.
    let normals = faces.map(|(n, ..)| n);
    for (i, &a) in normals.iter().enumerate() {
        for (j, &b) in normals.iter().enumerate().skip(i + 1).filter(|(_, b)| a.dot(**b) == 0.0) {
            let along = a.cross(b);
            let edge = [(a, b), (b, a)].into_iter().flat_map(|(p, q)| [p + q * FACE + along * CUT, p + q * FACE - along * CUT]);
            add(a + b, (a + b) / (a + b).len(), edge.collect(), None);
            for &c in normals.iter().skip(j + 1).filter(|c| a.dot(**c) == 0.0 && b.dot(**c) == 0.0) {
                let corner = [(a, b, c), (a, c, b), (b, a, c), (b, c, a), (c, a, b), (c, b, a)].map(|(p, q, s)| p + q * FACE + s * CUT);
                add(a + b + c, (a + b + c) / (a + b + c).len(), corner.to_vec(), None);
            }
        }
    }
    out.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    out
}

fn contains(points: &Ring<Pos2>, p: Pos2) -> bool {
    if !p.is_finite() || points.len() < 3 {
        return false;
    }
    let mut sign = 0.0_f32;
    for (a, b) in points.edges() {
        let edge = *b - *a;
        let offset = p - *a;
        let cross = edge.x * offset.y - edge.y * offset.x;
        if cross.abs() > 1e-3 {
            if sign != 0.0 && sign.signum() != cross.signum() {
                return false;
            }
            sign = cross;
        }
    }
    sign != 0.0
}

fn hit(faces: &[Facet], p: Pos2) -> Option<usize> {
    faces.iter().enumerate().rev().find(|(_, f)| contains(&f.points, p)).map(|(i, _)| i)
}

pub(super) fn drag_origin(app: &SolveApp, centre: Pos2, pointer: Pos2, scale: f32) -> bool {
    hit(&facets(&camera(app), centre, HALF * f64::from(scale)), pointer).is_some()
}

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
    // Preferences › Navigation: a view chosen on the cube also frames the whole model (#10091).
    if app.preferences.navigator_fit {
        params["fit"] = json!(true);
    }
    let _ = app.run("view.navigate", params);
}

fn control(app: &mut SolveApp, ui: &mut egui::Ui, bounds: Rect, action: &str, tooltip: &str) -> bool {
    control_in(app, ui, bounds, bounds.center(), |_| true, action, tooltip)
}

/// A control whose target is the part of `bounds` that `inside` accepts (a curved arrow is no
/// rectangle): only a pointer on that part hovers it, shows its tooltip, clicks or right-clicks it.
/// `handle` is the point automation clicks.
fn control_in(app: &mut SolveApp, ui: &mut egui::Ui, bounds: Rect, handle: Pos2, inside: impl Fn(Pos2) -> bool, action: &str, tooltip: &str) -> bool {
    crate::scenario::publish_handle(&format!("navigation_cube:control:{action}"), handle);
    let response = ui.interact(bounds, ui.scope_id().with(("navigation_cube", action)), Sense::click());
    let over = response.hover_pos().is_none_or(&inside);
    let response = if over { response.backing_on_hover_text(tooltip) } else { response };
    let hover = response.hovered() && over;
    let pressed_inside = response.interact_pointer_pos().is_none_or(&inside);
    if response.clicked() && pressed_inside {
        navigate(app, Some(action), None);
    }
    // The control takes the right-click from the viewport under it: open the gizmo's menu.
    if response.secondary_clicked()
        && pressed_inside
        && app.dialog.is_none()
        && let Some(at) = response.interact_pointer_pos()
    {
        crate::context_menu::open_for(app, at, crate::context_menu::Target::ViewGizmo);
    }
    hover
}

/// Preferences › Navigation: "Invert the view cube rotation arrows" (#10254). With it on, an
/// arrow orbits the camera the way the arrow points (camera-centric, as in Fusion and Inventor)
/// instead of rotating the cube toward the side the arrow sits on, so left and right, and up and
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

/// The orbit an arrow beside the cube's `side` performs, its tooltip, and the screen direction
/// it points: always inward at the cube, whichever orbit the preference above gives it.
fn orbit_arrow(side: &'static str, inverted: bool) -> (&'static str, &'static str, egui::Vec2) {
    let action = if inverted { inverted_arrow(side).unwrap_or(side) } else { side };
    let tooltip = match action {
        "left" => "Orbit left 90°",
        "right" => "Orbit right 90°",
        "up" => "Orbit up 90°",
        _ => "Orbit down 90°",
    };
    let inward = match side {
        "left" => vec2(1.0, 0.0),
        "right" => vec2(-1.0, 0.0),
        "up" => vec2(0.0, 1.0),
        _ => vec2(0.0, -1.0),
    };
    (action, tooltip, inward)
}

/// Preferences › Navigation: "Invert the curved roll arrows" (#10483). With it on, the two
/// curved arrows above the cube roll the view the other way within the screen plane: the
/// clockwise arrow rolls counterclockwise and the counterclockwise arrow rolls clockwise. The
/// arrow keeps its curl; the tooltip names the roll actually performed.
fn inverted_roll(action: &str) -> Option<&'static str> {
    match action {
        "clockwise" => Some("counterclockwise"),
        "counterclockwise" => Some("clockwise"),
        _ => None,
    }
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let outer = axis_rect(app, rect);
    if !outer.is_positive() {
        app.viewport.axis_drag = None;
        crate::scenario::publish_count("axis_view_highlights", 0.0);
        return;
    }
    crate::scenario::publish_rect("axis_view:outer", outer);
    let scale = outer.width() / DESIGN_SIZE;
    let c = axis_center(app, rect);
    let orbiting = axis_view_orbit(app, ui, rect);
    let active = orbiting || ui.rect_contains_pointer(outer);
    let t = Tokens::get().navigation_cube(active);
    crate::scenario::publish_count("navigation_cube:active", f64::from(u8::from(active)));
    let faces = facets(&camera(app), c, HALF * f64::from(scale));
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| ui.is_enabled() && ui.rect_contains_pointer(rect) && !crate::shell::covered(app, *p));
    let hovered = if orbiting { None } else { hover.and_then(|p| hit(&faces, p)) };
    let painter = ui.painter_at(outer);
    paint_cube(&painter, &faces, hovered, t, true, scale);
    for (i, face) in faces.iter().enumerate() {
        if faces.iter().take(i).any(|f| f.direction == face.direction) {
            continue;
        }
        let pieces: Vec<_> = faces.iter().filter(|f| f.direction == face.direction).collect();
        let centre = pieces.iter().fold(egui::Vec2::ZERO, |sum, f| sum + f.centre.to_vec2()) / pieces.len() as f32;
        crate::scenario::publish_handle(
            &format!("navigation_cube:{}", direction_label(face.direction).to_ascii_lowercase().replace(" · ", "_")),
            centre.to_pos2(),
        );
        // Keep principal-axis handles for existing automation, using only visible faces.
        let world = app.cam.from_frame(face.direction);
        let axis = match (world.x, world.y, world.z) {
            (1.0, 0.0, 0.0) => Some("x"),
            (-1.0, 0.0, 0.0) => Some("-x"),
            (0.0, 1.0, 0.0) => Some("y"),
            (0.0, -1.0, 0.0) => Some("-y"),
            (0.0, 0.0, 1.0) => Some("z"),
            (0.0, 0.0, -1.0) => Some("-z"),
            _ => None,
        };
        if let Some(axis) = axis {
            crate::scenario::publish_handle(&format!("axis_view:{axis}"), centre.to_pos2());
        }
    }
    crate::scenario::publish_count("axis_view_highlights", f64::from(u8::from(hovered.is_some())));
    if let Some(face) = hovered.and_then(|i| faces.get(i)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.interact(Rect::from_center_size(hover.unwrap_or(face.centre), vec2(2.0, 2.0)), ui.scope_id().with("navigation_cube_tip"), Sense::hover())
            .backing_on_hover_text(crate::i18n::format(
                if app.preferences.free_orbit { "ui.navigation_cube_view_free" } else { "ui.navigation_cube_view" },
                &[("view", &face.label)],
            ));
        if ui.input(|i| i.pointer.primary_clicked()) {
            navigate(app, None, Some(face.direction));
        }
    }
    let square_on = faces.iter().filter(|f| f.text_axes.is_some()).count() == 1;
    arrows(app, ui, &painter, c, scale, active, square_on, t);
    home(app, ui, &painter, c, scale, active, t);
    let fit = Rect::from_center_size(c + vec2(67.0, 67.0) * scale, vec2(22.0, 22.0) * scale);
    let fit_hover = control(app, ui, fit, "fit", "Fit model in view");
    if active {
        painter.circle(fit.center(), 6.0 * scale, if fit_hover { t.accent_soft } else { t.cube_face }, Stroke::new(1.0, t.cube_border));
    }
    // Small world-axis triad below the cube; it follows the selected up-axis frame.
    let origin = c + vec2(-60.0, 52.0) * scale;
    for (d, color, label) in [(Vec3::X, t.axis_x, "X"), (Vec3::Y, t.axis_y, "Y"), (Vec3::Z, t.axis_z, "Z")] {
        let (r, u, _) = app.cam.basis();
        let tip = origin + vec2((d.dot(r) * 17.0) as f32, -(d.dot(u) * 17.0) as f32) * scale;
        painter.line_segment([origin, tip], Stroke::new(1.5, color));
        painter.text(tip, Align2::CENTER_CENTER, label, FontId::proportional(crate::theme::Tokens::get().font_triad), color);
    }
}

/// The Home button at the top left of a gizmo centred at `c`, painted while it is `active`.
pub(super) fn home(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, scale: f32, active: bool, t: Tokens) {
    let home = Rect::from_center_size(c + vec2(-69.0, -69.0) * scale, vec2(24.0, 24.0) * scale);
    crate::scenario::publish_target("axis_view:home", home);
    let home_hover = control(app, ui, home, "home", "Home: isometric view and fit model");
    if active {
        icons::paint_icon(painter, home.shrink(1.0 * scale), "home", f32::from((if home_hover { t.accent } else { t.cube_face }).a()) / 255.0);
    }
}

/// The four orbit arrows and the two roll arrows around a gizmo centred at `c`: nearer and larger
/// while a cube face is square on, painted while the gizmo is `active`.
#[allow(clippy::too_many_arguments)]
pub(super) fn arrows(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, c: Pos2, scale: f32, active: bool, square_on: bool, t: Tokens) {
    let distance = if square_on { 54.0 } else { 74.0 };
    // The target holds the whole drawn triangle, wings included, so it is at least as wide.
    let button_size = 2.0 * ORBIT_ARROW_WING;
    for (side, offset) in
        [("left", vec2(-distance, 0.0)), ("right", vec2(distance, 0.0)), ("up", vec2(0.0, -distance)), ("down", vec2(0.0, distance))]
    {
        let (action, tooltip, direction) = orbit_arrow(side, app.preferences.invert_navigator_arrows);
        let at = c + offset * scale;
        let hovered = control(app, ui, Rect::from_center_size(at, vec2(button_size, button_size) * scale), action, tooltip);
        if active {
            arrow(painter, at, direction * scale, hovered, t);
        }
    }
    // Two arcs above the cube, left of the top arrow curling counterclockwise and right of it
    // clockwise; angles in degrees on the screen, y down.
    for (action, from, to, tooltip) in
        [("counterclockwise", -110.0_f32, -155.0_f32, "Rotate view counterclockwise 90°"), ("clockwise", -70.0, -25.0, "Rotate view clockwise 90°")]
    {
        let (action, tooltip) = match app.preferences.invert_curved_roll_arrows.then(|| inverted_roll(action)).flatten() {
            Some(swapped) => (swapped, if swapped == "clockwise" { "Rotate view clockwise 90°" } else { "Rotate view counterclockwise 90°" }),
            None => (action, tooltip),
        };
        let (from, to) = (from.to_radians(), to.to_radians());
        let middle = (from + to) / 2.0;
        let at = c + vec2(middle.cos(), middle.sin()) * (CURVED_ARROW_RADIUS * scale);
        let target = curved_arrow_bounds(c, from, to, scale);
        let hovered = control_in(app, ui, target, at, |p| curved_arrow_contains(c, from, to, scale, p), action, tooltip);
        if active {
            curved_arrow(painter, c, from, to, hovered, t, scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::navigation::CURVED_ARROW_HEAD_LENGTH;
    #[cfg(test)]
    use crate::icons::navigation::{LABEL_PIXELS, face_text};
    use solidcraft_engine::render::{StandardView, UpAxis};

    fn paint_frame(ctx: &egui::Context, paint: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let mut output = ctx.run_ui(egui::RawInput::default(), paint);
        // These tests inspect shapes without uploading textures to a renderer.
        output.textures_delta.clear();
        output
    }

    #[test]
    fn cube_projection_foreshortens_depth_only_in_perspective() {
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut cam = Camera { up, ..Default::default() };
            cam.set_view(StandardView::Front);
            let near = Vec3::new(0.5, -1.0, 0.0);
            let far = Vec3::new(0.5, 1.0, 0.0);
            assert_eq!(project(&cam, near, HALF), project(&cam, far, HALF));
            for fov in [0.2618, 0.6, std::f64::consts::FRAC_PI_2] {
                cam.fov = fov;
                let front = project(&cam, near, HALF);
                let back = project(&cam, far, HALF);
                assert!(back.x < front.x, "perspective must make the far edge smaller: {up:?}, {fov}");
                assert!((front.x - HALF as f32 * 0.5).abs() < 1e-4, "the front face must retain its size beside the orbit arrows");
                let focal_distance = f64::from(DESIGN_SIZE) / (2.0 * HALF * (fov * 0.5).tan());
                let ratio = focal_distance / (focal_distance + 2.0);
                assert!((f64::from(back.x / front.x) - ratio).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn perspective_toggle_changes_the_cube_and_its_faces_still_navigate() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "look_at_anim":false}));
        h.call("ui.view", json!({"view":"iso"}));
        let orthographic = crate::scenario::handle_at("navigation_cube:front").unwrap();
        h.call("ui.set", json!({"perspective":true}));
        let fov = camera(&h.app).fov;
        assert!(fov > 0.0);
        let p = crate::scenario::handle_at("navigation_cube:front").unwrap();
        assert_ne!(p, orthographic, "the drawn cube must follow the viewport's projection selection");
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert_eq!(h.app.cam.aligned_standard_view(1e-5), Some(StandardView::Front));
        assert_eq!(camera(&h.app).fov, fov);
        h.call("ui.set", json!({"autoOrthoFaces":true}));
        assert_eq!(camera(&h.app).fov, 0.0, "automatic orthographic axis views also apply to the cube");
        h.call("ui.view", json!({"view":"iso"}));
        assert_eq!(camera(&h.app).fov, fov);
    }

    #[test]
    fn perspective_cube_culls_sides_facing_away_and_ignores_model_framing() {
        let cam = Camera { yaw: -0.1, pitch: 0.1, fov: 0.6, ..Default::default() };
        let faces = facets(&cam, Pos2::ZERO, HALF);
        assert_eq!(faces.len(), 9, "only the front face and its ring of chamfers face this finite eye");
        assert!(faces.iter().all(|face| face.direction.y == -1.0));
        let points = |cam: &Camera| facets(cam, Pos2::ZERO, HALF).into_iter().flat_map(|face| face.points).collect::<Vec<_>>();
        let expected = points(&cam);
        for distance in [1e-3, 100.0, 1e12] {
            assert_eq!(points(&Camera { distance, target: Vec3::new(123.0, -456.0, 789.0), ..cam }), expected);
        }
        let parallel = Camera { fov: 0.0, ..cam };
        for fov in [f64::NAN, f64::INFINITY, -1.0] {
            assert_eq!(points(&Camera { fov, ..cam }), points(&parallel));
        }
    }

    /// Each orbit arrow points inward at the cube in both settings. By default it shows the way
    /// the cube turns, so the camera moves against it; with "Invert the view cube rotation arrows"
    /// the actions trade sides and the camera moves the way the arrow points.
    #[test]
    fn orbit_arrows_point_at_the_cube_and_the_preference_picks_cube_or_camera() {
        let mut h = crate::scenario::Harness::new();
        for inverted in [false, true] {
            for (side, outward) in [("left", vec2(-1.0, 0.0)), ("right", vec2(1.0, 0.0)), ("up", vec2(0.0, -1.0)), ("down", vec2(0.0, 1.0))] {
                let (action, _, direction) = orbit_arrow(side, inverted);
                let mut from = Camera::default();
                from.set_view(StandardView::Front);
                let to: Camera = serde_json::from_value(
                    h.app.session.execute("view.navigate", &json!({"camera": from, "action": action})).unwrap()["camera"].clone(),
                )
                .unwrap();
                let (r, u, _) = from.basis();
                let moved = to.back() - from.back();
                let on_screen = vec2(moved.dot(r) as f32, -moved.dot(u) as f32).normalized();
                let along = if inverted { 1.0 } else { -1.0 };
                assert!(on_screen.dot(direction) * along > 0.99, "{side} (inverted {inverted}): drawn {direction:?}, camera moves {on_screen:?}");
                assert!(direction.dot(outward) < -0.99, "{side} (inverted {inverted}): the arrow points at the cube");
            }
        }
    }

    /// A chamfer's plane lies farther from the centre than a face's, so from a finite eye it
    /// turns away at a different angle: every facet is drawn exactly when it faces the eye.
    #[test]
    fn perspective_cube_draws_exactly_the_facets_that_face_the_eye() {
        let mut directions = Vec::new();
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if (x, y, z) != (0, 0, 0) {
                        directions.push(Vec3::new(f64::from(x), f64::from(y), f64::from(z)));
                    }
                }
            }
        }
        // A facet's plane passes through a + b·FACE + c·CUT for its one, two or three axes.
        let plane_distance = |d: Vec3| {
            let axes = [d.x, d.y, d.z].iter().filter(|v| **v != 0.0).count();
            ([1.0, FACE, CUT][..axes].iter().sum::<f64>()) / (axes as f64).sqrt()
        };
        let mut culled = 0;
        for up in [UpAxis::Z, UpAxis::Y] {
            for fov in [0.2618, 0.6, std::f64::consts::FRAC_PI_2] {
                for step in 0..48 {
                    let (yaw, pitch) = (f64::from(step) * 0.37, (f64::from(step) * 0.53).sin() * 1.4);
                    let cam = Camera { up, yaw, pitch, fov, ..Default::default() };
                    let eye_distance = 1.0 + 1.0 / inverse_focal_length(&cam);
                    let drawn: Vec<Vec3> = facets(&cam, Pos2::ZERO, HALF).iter().map(|f| f.direction).collect();
                    for &d in &directions {
                        let margin = cam.from_frame(d / d.len()).dot(cam.back()) * eye_distance - plane_distance(d);
                        if margin.abs() < 1e-4 {
                            continue;
                        }
                        if margin < 0.0 && cam.from_frame(d / d.len()).dot(cam.back()) > 0.0 {
                            culled += 1;
                        }
                        assert_eq!(drawn.contains(&d), margin > 0.0, "{up:?} fov {fov} yaw {yaw} pitch {pitch}: {}", direction_label(d));
                    }
                }
            }
        }
        assert!(culled > 0, "the sweep must reach facets that only perspective hides");
    }

    #[test]
    fn cube_targets_are_six_faces_twelve_edges_and_eight_corners_in_both_up_frames() {
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut directions = Vec::new();
            for view in [StandardView::Front, StandardView::Back, StandardView::Top, StandardView::Bottom, StandardView::Left, StandardView::Right] {
                let mut cam = Camera { up, ..Default::default() };
                cam.set_view(view);
                for facet in facets(&cam, Pos2::ZERO, HALF) {
                    if !directions.contains(&facet.direction) {
                        directions.push(facet.direction);
                    }
                }
            }
            let rank = |d: &&Vec3| [d.x, d.y, d.z].into_iter().filter(|v| *v != 0.0).count();
            assert_eq!([1, 2, 3].map(|n| directions.iter().filter(|d| rank(d) == n).count()), [6, 12, 8]);
        }
    }

    #[test]
    fn chamfers_around_a_square_on_face_click_their_edge_and_corner_views() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        h.app.preferences.look_at_anim = false;
        for up in ["z", "y"] {
            h.call("ui.prefs", json!({"up_axis":up}));
            for (name, direction) in
                [("top_front", Vec3::Z - Vec3::Y), ("front_right", Vec3::X - Vec3::Y), ("bottom_front_left", -Vec3::Z - Vec3::Y - Vec3::X)]
            {
                h.call("ui.view", json!({"view":"front"}));
                let p = crate::scenario::handle_at(&format!("navigation_cube:{name}")).unwrap();
                h.call("ui.click", json!({"x":p.x,"y":p.y}));
                let expected = h.app.cam.from_frame(direction.normalized().unwrap());
                assert!(h.app.cam.back().dist(expected) < 2e-6, "{up} {name}");
            }
        }
    }

    #[test]
    fn hovering_cube_sections_activates_then_leaving_restores_idle_without_navigation() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        let camera = h.app.cam;
        let design = h.app.session.doc.to_json();
        let viewport = h.app.viewport.rect.unwrap();
        for name in ["front", "right", "top_front_right"] {
            let p = crate::scenario::handle_at(&format!("navigation_cube:{name}")).unwrap();
            h.call("ui.move", json!({"x":p.x,"y":p.y}));
            assert_eq!(crate::scenario::count_of("navigation_cube:active"), Some(1.0));
            assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(1.0));
            h.call("ui.move", json!({"x":viewport.center().x,"y":viewport.center().y}));
            assert_eq!(crate::scenario::count_of("navigation_cube:active"), Some(0.0));
            assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(0.0));
        }
        // Reveal auxiliary controls even before the pointer touches a facet.
        let p = crate::scenario::handle_at("navigation_cube:control:home").unwrap();
        h.call("ui.move", json!({"x":p.x,"y":p.y}));
        assert_eq!(crate::scenario::count_of("navigation_cube:active"), Some(1.0));
        assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(0.0));
        assert_eq!(h.app.cam, camera);
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(h.app.session.undo.is_empty());
    }

    #[test]
    fn hovered_face_and_corner_have_a_pale_blue_opaque_fill_in_both_themes() {
        let ctx = crate::fonts::test_context();
        let faces = facets(&Camera::default(), pos2(100.0, 100.0), HALF);
        for t in [Tokens::light(), Tokens::dark()] {
            assert_ne!(t.navigation_cube(false).cube_face, t.cube_face);
            for name in ["FRONT", "RIGHT", "TOP · FRONT", "TOP · FRONT · RIGHT"] {
                let hovered = faces.iter().position(|f| f.label == name).unwrap();
                let output = paint_frame(&ctx, |ui| {
                    paint_cube(ui.painter(), &faces, Some(hovered), t.navigation_cube(true), false, 1.0);
                });
                let mesh = output.shapes.iter().find_map(|s| if let Shape::Mesh(m) = &s.shape { Some(m) } else { None }).unwrap();
                let highlighted: Vec<_> = mesh.vertices.iter().filter(|v| v.color == t.cube_hover).collect();
                let pieces: Vec<_> = faces.iter().filter(|f| f.direction == faces[hovered].direction).collect();
                assert_eq!(
                    highlighted.len(),
                    pieces.iter().map(|f| f.points.len() + 1).sum::<usize>(),
                    "highlight the same target on adjacent flat faces"
                );
                assert!(highlighted.iter().all(|v| pieces.iter().any(|f| contains(&f.points, v.pos))));
                assert!(mesh.vertices.iter().all(|v| v.color.a() == 255));
                assert!(output.shapes.iter().any(|s| matches!(&s.shape, Shape::LineSegment { stroke, .. } if stroke.color == t.accent)));
            }
        }
    }

    #[test]
    fn principal_views_show_the_face_ringed_by_its_four_edge_and_four_corner_chamfers() {
        let ctx = crate::fonts::test_context();
        for up in [UpAxis::Z, UpAxis::Y] {
            for view in [StandardView::Front, StandardView::Back, StandardView::Top, StandardView::Bottom, StandardView::Left, StandardView::Right] {
                let mut cam = Camera { up, ..Default::default() };
                cam.set_view(view);
                let centre = pos2(100.0, 100.0);
                let faces = facets(&cam, centre, HALF);
                let rank = |f: &&Facet| [f.direction.x, f.direction.y, f.direction.z].into_iter().filter(|v| *v != 0.0).count();
                assert_eq!([1, 2, 3].map(|n| faces.iter().filter(|f| rank(f) == n).count()), [1, 4, 4], "{up:?} {view:?}");
                assert_eq!(faces.iter().filter(|f| f.text_axes.is_some()).count(), 1, "only the face carries a label");
                let bounds = Rect::from_points(&faces.iter().flat_map(|f| f.points.iter().copied()).collect::<Vec<_>>());
                let expected = Rect::from_center_size(centre, vec2(80.0, 80.0));
                assert!(
                    bounds.min.distance(expected.min) < 1e-4 && bounds.max.distance(expected.max) < 1e-4,
                    "the chamfers reach the cube's outline"
                );
                let output = paint_frame(&ctx, |ui| {
                    paint_cube(ui.painter(), &faces, None, Tokens::light(), false, 1.0);
                });
                let seams = output
                    .shapes
                    .iter()
                    .filter(|s| matches!(&s.shape, Shape::LineSegment { stroke, .. } if stroke.color == Tokens::light().cube_border));
                // The octagon's 8 sides, then 3 more per edge strip and per corner hexagon.
                assert_eq!(seams.count(), 32, "every facet boundary is drawn once");
            }
        }
    }

    #[test]
    fn projected_facets_pick_their_own_centres_in_all_views_and_up_frames() {
        for up in [UpAxis::Z, UpAxis::Y] {
            for view in [
                StandardView::Front,
                StandardView::Back,
                StandardView::Top,
                StandardView::Bottom,
                StandardView::Left,
                StandardView::Right,
                StandardView::Iso,
            ] {
                let mut cam = Camera { up, ..Default::default() };
                cam.set_view(view);
                for roll in [0.0, 0.7, std::f64::consts::FRAC_PI_2] {
                    cam.roll = roll;
                    for fov in [0.0, 0.2618, 0.6, std::f64::consts::FRAC_PI_2] {
                        cam.fov = fov;
                        let faces = facets(&cam, Pos2::ZERO, HALF);
                        assert!(!faces.is_empty());
                        for (i, face) in faces.iter().enumerate() {
                            assert_eq!(hit(&faces, face.centre), Some(i), "{up:?} {view:?} {roll} {fov} {}", face.label);
                            assert!(face.points.iter().all(|p| p.is_finite() && p.to_vec2().length() <= (HALF * 3.0_f64.sqrt()) as f32 + 1e-4));
                        }
                        assert!(hit(&faces, pos2(80.0, 80.0)).is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn face_labels_use_normalized_font_atlas_coordinates() {
        let ctx = crate::fonts::test_context();
        let output = paint_frame(&ctx, |ui| {
            let faces = facets(&Camera::default(), pos2(100.0, 100.0), HALF);
            for face in &faces {
                face_text(ui.painter(), face, Tokens::light().cube_text, Tokens::get().font_heading);
            }
        });
        let meshes: Vec<_> = output.shapes.iter().filter_map(|shape| if let Shape::Mesh(mesh) = &shape.shape { Some(mesh) } else { None }).collect();
        assert_eq!(meshes.len(), 3, "the isometric view must show three face labels");
        for mesh in meshes {
            assert!(!mesh.vertices.is_empty());
            assert!(mesh.vertices.iter().all(|v| (0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y)));
        }
    }

    #[test]
    fn face_labels_follow_the_program_language_and_fit_their_face() {
        let ctx = crate::fonts::test_context();
        for catalog in crate::i18n::LOCALES {
            let _language = crate::i18n::scope(catalog.id);
            for (view, id) in [
                (StandardView::Front, "ui.heading_front"),
                (StandardView::Back, "ui.heading_back"),
                (StandardView::Top, "ui.heading_top"),
                (StandardView::Bottom, "ui.heading_bottom"),
                (StandardView::Left, "ui.heading_left"),
                (StandardView::Right, "ui.heading_right"),
            ] {
                let mut cam = Camera::default();
                cam.set_view(view);
                let faces = facets(&cam, pos2(100.0, 100.0), HALF);
                let face = faces.iter().find(|f| f.text_axes.is_some()).unwrap();
                assert_eq!(face.label, crate::i18n::tr(id), "{} {id}", catalog.id);
                let output = paint_frame(&ctx, |ui| face_text(ui.painter(), face, Tokens::light().cube_text, 14.0));
                for shape in &output.shapes {
                    if let Shape::Mesh(mesh) = &shape.shape {
                        assert!(mesh.vertices.iter().all(|v| contains(&face.points, v.pos)), "{} {id}: the label overflows its face", catalog.id);
                    }
                }
            }
        }
        let _german = crate::i18n::scope("de");
        let mut labels: Vec<_> =
            facets(&Camera::default(), Pos2::ZERO, HALF).into_iter().filter(|f| f.text_axes.is_some()).map(|f| f.label).collect();
        labels.sort();
        assert_eq!(labels, ["OBEN", "RECHTS", "VORNE"]);
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "language":"de"}));
        assert!(crate::scenario::handle_at("navigation_cube:front").is_some(), "automation handles stay language-neutral");
        assert!(crate::scenario::handle_at("navigation_cube:top_front_right").is_some());
    }

    #[test]
    fn label_uvs_follow_font_atlas_growth_later_in_the_same_frame() {
        let ctx = crate::fonts::test_context();
        let mut pixel_uv = Pos2::ZERO;
        let mut glyph_vertex = 0;
        let mut early_size = [0; 2];
        let output = paint_frame(&ctx, |ui| {
            let mut cam = Camera::default();
            cam.set_view(StandardView::Top);
            let faces = facets(&cam, pos2(100.0, 100.0), HALF);
            let face = faces.iter().find(|f| f.label == "TOP" && f.text_axes.is_some()).unwrap();
            let galley = ui.painter().layout_no_wrap(face.label.clone(), FontId::proportional(14.0), Tokens::light().cube_text);
            let (index, vertex) = galley.rows.first().unwrap().visuals.mesh.vertices.iter().enumerate().find(|(_, v)| v.uv.y > 1.0).unwrap();
            glyph_vertex = index;
            pixel_uv = vertex.uv;
            face_text(ui.painter(), face, Tokens::light().cube_text, Tokens::get().font_heading);
            early_size = ctx.fonts(|f| f.font_image_size());
            // Other widgets can allocate new glyphs after the cube has been drawn.
            let text: String = (32_u8..127).map(char::from).collect();
            let _ = ui.painter().layout_no_wrap(text, FontId::proportional(48.0), Color32::WHITE);
        });
        let size = ctx.fonts(|f| f.font_image_size());
        assert!(size[1] > early_size[1], "the reproduction must grow the font atlas");
        let vertex = output.shapes.iter().find_map(|s| if let Shape::Mesh(m) = &s.shape { m.vertices.get(glyph_vertex) } else { None }).unwrap();
        assert!(output.shapes.iter().all(|s| !matches!(&s.shape, Shape::Mesh(m) if m.texture_id == LABEL_PIXELS)));
        assert!((vertex.uv.x * size[0] as f32 - pixel_uv.x).abs() < 1e-3);
        assert!((vertex.uv.y * size[1] as f32 - pixel_uv.y).abs() < 1e-3);
    }

    #[test]
    fn rotating_cube_facets_stay_opaque_and_inside_the_projected_geometry() {
        let ctx = crate::fonts::test_context();
        for up in [UpAxis::Z, UpAxis::Y] {
            for view in [StandardView::Top, StandardView::Bottom, StandardView::Front, StandardView::Back, StandardView::Left, StandardView::Right] {
                let from = Camera { up, ..Default::default() };
                let mut to = from;
                to.set_view(view);
                for step in 0..=60 {
                    let cam = Camera::interpolate(&from, &to, f64::from(step) / 60.0);
                    let faces = facets(&cam, pos2(100.0, 100.0), HALF);
                    let bounds = Rect::from_points(&faces.iter().flat_map(|f| f.points.iter().copied()).collect::<Vec<_>>());
                    let output = paint_frame(&ctx, |ui| {
                        paint_cube(ui.painter(), &faces, Some(0), Tokens::light(), false, 1.0);
                    });
                    let fills: Vec<_> = output.shapes.iter().filter_map(|s| if let Shape::Mesh(m) = &s.shape { Some(m) } else { None }).collect();
                    assert_eq!(fills.len(), 1);
                    let mesh = fills[0];
                    assert!(mesh.is_valid());
                    assert!(mesh.vertices.iter().all(|v| v.color.a() == 255 && v.pos.is_finite() && bounds.contains(v.pos)));
                }
            }
        }
    }

    #[test]
    fn control_navigation_keeps_its_transition_in_a_single_body_design() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        h.frames(4);
        let cam = h.app.cam;
        let result = h.call("engine.execute", json!({"command":"view.navigate","params":{"camera":cam,"direction":[0,0,1]}}));
        assert_eq!(result["ok"], true);
        assert!(h.app.cam.back().dist(Vec3::Z) < 1e-6, "automatic model fitting must not cancel the navigation command");
        assert_eq!(h.app.cam.target, cam.target);
        assert_eq!(h.app.cam.distance, cam.distance);
    }

    /// #10091: with Preferences › Navigation "Fit the model after a navigator view change" on, a
    /// cube face, corner or arrow frames the whole model from the new direction, as Home does;
    /// off (the default) the view turns in place, keeping its target and zoom.
    #[test]
    fn cube_views_frame_the_model_when_the_navigation_preference_asks() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "look_at_anim":false}));
        assert!(!h.app.preferences.navigator_fit, "off by default");
        h.app.run("solid.box", json!({"corner":[100,0,0],"length":20,"width":20,"height":20})).unwrap();
        h.settle();
        let undo = h.app.session.undo.len();
        let away = |h: &mut crate::scenario::Harness| {
            h.call("ui.view", json!({"view":"iso"}));
            h.app.cam.pan(600.0, 400.0, 500.0);
            h.app.cam.distance *= 4.0;
            h.frames(2);
            h.app.cam
        };
        let from = away(&mut h);
        let p = crate::scenario::handle_at("navigation_cube:front").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        h.frames(2);
        assert!(h.app.cam.back().dist(-Vec3::Y) < 1e-6);
        assert_eq!(h.app.cam.target, from.target, "off: the target stays");
        assert_eq!(h.app.cam.distance, from.distance, "off: the zoom stays");
        h.call("ui.prefs", json!({"navigator_fit":true}));
        for handle in ["navigation_cube:front", "navigation_cube:top_front_right", "navigation_cube:control:right"] {
            let from = away(&mut h);
            let p = crate::scenario::handle_at(handle).unwrap();
            h.call("ui.click", json!({"x":p.x,"y":p.y}));
            h.frames(2);
            if handle == "navigation_cube:front" {
                assert!(h.app.cam.back().dist(-Vec3::Y) < 1e-6);
            }
            assert!(h.app.cam.target.dist(Vec3::new(110.0, 10.0, 10.0)) < 1e-6, "{handle}: looks at the middle of the model, {:?}", h.app.cam.target);
            assert!(h.app.cam.distance < from.distance / 2.0, "{handle}: {} vs {}", h.app.cam.distance, from.distance);
        }
        // Home, then a face, a corner or an arrow: the zoom and the centre stay; only the
        // direction changes.
        let click = |h: &mut crate::scenario::Harness, handle: &str| {
            let p = crate::scenario::handle_at(handle).unwrap_or_else(|| panic!("{handle} is not on the cube"));
            h.call("ui.click", json!({"x":p.x,"y":p.y}));
            h.frames(2);
        };
        click(&mut h, "navigation_cube:control:home");
        let home = h.app.cam;
        for handle in ["navigation_cube:front", "navigation_cube:top", "navigation_cube:top_front_right", "navigation_cube:control:right"] {
            click(&mut h, "navigation_cube:control:home");
            click(&mut h, handle);
            assert!((h.app.cam.distance - home.distance).abs() < 1e-6, "{handle}: {} vs Home's {}", h.app.cam.distance, home.distance);
            assert!(h.app.cam.target.dist(home.target) < 1e-6, "{handle}: {:?} vs Home's {:?}", h.app.cam.target, home.target);
        }
        assert_eq!(h.app.session.undo.len(), undo, "a view change edits nothing");
    }

    /// #10254: with Preferences › Navigation "Invert the view cube rotation arrows" on, the
    /// arrow on the left orbits the camera the way it points (right), and the other arrows swap
    /// the same way; the curved roll arrows keep their action.
    #[test]
    fn inverted_arrows_orbit_the_way_they_point() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "look_at_anim":false}));
        assert!(!h.app.preferences.invert_navigator_arrows, "off by default");
        let turn = |h: &mut crate::scenario::Harness, action: &str| -> Camera {
            h.call("ui.view", json!({"view":"front"}));
            h.frames(2);
            serde_json::from_value(h.app.session.execute("view.navigate", &json!({"camera": h.app.cam, "action": action})).unwrap()["camera"].clone())
                .unwrap()
        };
        let click = |h: &mut crate::scenario::Harness, handle: &str| {
            h.call("ui.view", json!({"view":"front"}));
            h.frames(2);
            let p = crate::scenario::handle_at(&format!("navigation_cube:control:{handle}")).unwrap();
            h.call("ui.click", json!({"x":p.x,"y":p.y}));
            h.frames(2);
            h.app.cam
        };
        let on_the_left = |handle: &str| {
            let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
            crate::scenario::handle_at(&format!("navigation_cube:control:{handle}")).unwrap().x < outer.center().x
        };
        // Off (the default): the arrow on the left runs the "left" orbit, as before.
        assert!(on_the_left("left"), "off: the left action sits on the cube's left side");
        assert_eq!(click(&mut h, "left"), turn(&mut h, "left"));
        assert_eq!(click(&mut h, "up"), turn(&mut h, "up"));
        h.call("ui.prefs", json!({"invert_navigator_arrows":true}));
        h.frames(2);
        // On: the arrow on the left now runs the "right" orbit, and the sides trade places.
        assert!(on_the_left("right"), "on: the arrow on the left runs the right orbit");
        assert_eq!(click(&mut h, "right"), turn(&mut h, "right"));
        assert_eq!(click(&mut h, "left"), turn(&mut h, "left"));
        assert_eq!(click(&mut h, "down"), turn(&mut h, "down"));
        // The curved roll arrows are not inverted.
        h.call("ui.view", json!({"view":"front"}));
        h.frames(2);
        let camera = h.app.cam;
        let p = crate::scenario::handle_at("navigation_cube:control:clockwise").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!((h.app.cam.roll - camera.roll - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "clockwise still rolls clockwise");
    }

    /// An orbit arrow is hit where it is drawn, wings included, whether a cube face is square on
    /// (the arrows sit nearer) or not.
    #[test]
    fn the_wings_of_an_orbit_arrow_are_clickable() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "look_at_anim":false}));
        for (view, distance) in [("front", 54.0_f32), ("iso", 74.0)] {
            for (side, offset) in [("left", vec2(-1.0, 0.0)), ("right", vec2(1.0, 0.0)), ("up", vec2(0.0, -1.0)), ("down", vec2(0.0, 1.0))] {
                let (_, _, inward) = orbit_arrow(side, false);
                let across = vec2(-inward.y, inward.x);
                for wing in [-1.0_f32, 1.0] {
                    h.call("ui.view", json!({"view":view}));
                    h.frames(2);
                    let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
                    let scale = outer.width() / DESIGN_SIZE;
                    // A wing tip, a little inside the drawn corner (see `arrow`).
                    let tip = offset * distance - inward * 6.0 + across * wing * (ORBIT_ARROW_WING - 0.6);
                    let p = outer.center() + tip * scale;
                    let before = h.app.cam.back();
                    h.call("ui.click", json!({"x":p.x,"y":p.y}));
                    h.frames(2);
                    assert!(h.app.cam.back().dist(before) > 1e-3, "{view} {side} wing {wing}: the click did not orbit");
                }
            }
        }
    }

    /// A roll arrow is hit where it is drawn: its far end and its arrowhead roll the view too,
    /// not only the middle of the band.
    #[test]
    fn the_whole_drawn_roll_arrow_is_clickable() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "look_at_anim":false}));
        let quarter = std::f64::consts::FRAC_PI_2;
        let rolled = |delta: f64, way: f64| (delta - way).rem_euclid(std::f64::consts::TAU) < 1e-6;
        // Each arrow, with the angles it is drawn between in `arrows`, and the roll it performs.
        for (name, from, to, way) in [("counterclockwise", -110.0_f32, -155.0_f32, -quarter), ("clockwise", -70.0, -25.0, quarter)] {
            let (from, to) = (from.to_radians(), to.to_radians());
            let along = (to - from).signum();
            let head_angle = CURVED_ARROW_HEAD_LENGTH / CURVED_ARROW_RADIUS;
            // Near the start of the band, and in the arrowhead just short of its tip.
            for (where_, angle) in [("start", from + along * 0.03), ("head", to + along * (head_angle - 0.05))] {
                h.call("ui.view", json!({"view":"front"}));
                h.frames(2);
                let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
                let radius = CURVED_ARROW_RADIUS * outer.width() / DESIGN_SIZE;
                let p = outer.center() + vec2(angle.cos(), angle.sin()) * radius;
                let before = h.app.cam.roll;
                h.call("ui.click", json!({"x":p.x,"y":p.y}));
                h.frames(2);
                assert!(rolled(h.app.cam.roll - before, way), "{name} {where_}: roll went {} -> {}", before, h.app.cam.roll);
            }
        }
    }

    /// The pointer target follows the drawn band and head, and stops there.
    #[test]
    fn a_roll_arrow_contains_its_band_and_head_and_nothing_else() {
        let centre = pos2(200.0, 300.0);
        let (from, to) = ((-110.0_f32).to_radians(), (-155.0_f32).to_radians());
        let along = (to - from).signum();
        let head_angle = CURVED_ARROW_HEAD_LENGTH / CURVED_ARROW_RADIUS;
        for scale in [1.0, 2.5] {
            let at = |angle: f32, radius: f32| centre + vec2(angle.cos(), angle.sin()) * (radius * scale);
            let inside = |p| curved_arrow_contains(centre, from, to, scale, p);
            let middle = (from + to) / 2.0;
            assert!(inside(at(middle, CURVED_ARROW_RADIUS)), "the middle of the band, scale {scale}");
            assert!(inside(at(from + along * 0.02, CURVED_ARROW_RADIUS)), "its start, scale {scale}");
            assert!(inside(at(to + along * (head_angle - 0.05), CURVED_ARROW_RADIUS)), "its head, scale {scale}");
            assert!(!inside(at(from - along * 0.3, CURVED_ARROW_RADIUS)), "before its start, scale {scale}");
            assert!(!inside(at(to + along * (head_angle + 0.3), CURVED_ARROW_RADIUS)), "past its tip, scale {scale}");
            assert!(!inside(at(middle, CURVED_ARROW_RADIUS * 0.5)), "towards the cube, scale {scale}");
            assert!(!inside(at(middle, CURVED_ARROW_RADIUS * 1.3)), "outside the band, scale {scale}");
        }
    }

    /// A tooltip that names an edge or a corner joins two or three translated words. Right-to-left
    /// catalogs are stored in display order word by word, so the joined label is put in display
    /// order as a whole (the first word at the right); other languages read as they are joined.
    #[test]
    fn joined_view_names_follow_the_script_direction() {
        let top_front = Vec3::Z - Vec3::Y;
        for (language, rtl) in [("he", true), ("ar", true), ("en", false), ("de", false)] {
            let _language = crate::i18n::scope(language);
            let logical = ["ui.heading_top", "ui.heading_front"].map(|id| crate::i18n::tr(id).to_string()).join(" · ");
            let name = view_name(top_front);
            if rtl {
                assert_eq!(name, crate::bidi::visual(&logical), "{language}");
                assert_ne!(name, logical, "{language}: the words must change places");
            } else {
                assert_eq!(name, logical, "{language}");
            }
        }
    }

    /// #10483: with Preferences › Navigation "Invert the curved roll arrows" on, the two curved
    /// arrows trade rolls: the clockwise arrow rolls counterclockwise, and the counterclockwise
    /// arrow rolls clockwise.
    #[test]
    fn inverted_curved_arrows_roll_the_other_way() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube", "look_at_anim":false}));
        assert!(!h.app.preferences.invert_curved_roll_arrows, "off by default");
        let roll_at = |h: &mut crate::scenario::Harness, p: egui::Pos2| -> f64 {
            h.call("ui.view", json!({"view":"front"}));
            h.frames(2);
            let camera = h.app.cam;
            h.call("ui.click", json!({"x":p.x,"y":p.y}));
            h.frames(2);
            h.app.cam.roll - camera.roll
        };
        let quarter = std::f64::consts::FRAC_PI_2;
        let turn = std::f64::consts::TAU;
        // The engine keeps roll in [0, τ), so compare quarter-turns modulo a full turn.
        let is_a_quarter_turn = |delta: f64, way: f64| (delta - way).rem_euclid(turn) < 1e-6 || (way - delta).rem_euclid(turn) < 1e-6;
        let top = crate::scenario::handle_at("navigation_cube:control:counterclockwise").unwrap();
        let right = crate::scenario::handle_at("navigation_cube:control:clockwise").unwrap();
        // Off (the default): each arrow rolls the view the way it curls.
        assert!(is_a_quarter_turn(roll_at(&mut h, right), quarter), "off: the clockwise arrow rolls clockwise");
        assert!(is_a_quarter_turn(roll_at(&mut h, top), -quarter), "off: the counterclockwise arrow rolls counterclockwise");
        h.call("ui.prefs", json!({"invert_curved_roll_arrows":true}));
        h.frames(2);
        // On: the same two arrows trade rolls.
        assert!(is_a_quarter_turn(roll_at(&mut h, right), -quarter), "on: the clockwise arrow rolls counterclockwise");
        assert!(is_a_quarter_turn(roll_at(&mut h, top), quarter), "on: the counterclockwise arrow rolls clockwise");
        // The handles name the roll actually performed, and trade places with it.
        assert_eq!(crate::scenario::handle_at("navigation_cube:control:clockwise").unwrap(), top);
    }

    #[test]
    fn cube_navigation_eases_through_intermediate_views_and_can_be_interrupted() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        assert!(h.app.preferences.look_at_anim);
        for (action, direction) in [(None, Some(Vec3::Z)), (Some("right"), None), (Some("clockwise"), None), (Some("home"), None)] {
            let from = h.app.cam;
            navigate(&mut h.app, action, direction);
            let anim = h.app.cam_anim.expect("cube navigation must animate");
            assert_eq!(h.app.cam, from, "starting a transition must not jump the camera");
            let mut previous = from;
            for frame in 1..=60 {
                let now = anim.start + anim.duration * f64::from(frame) / 60.0 + if frame == 60 { 1e-9 } else { 0.0 };
                let (cam, done) = anim.sample(now);
                let angle = |a: Vec3, b: Vec3| a.dot(b).clamp(-1.0, 1.0).acos().to_degrees();
                assert!(angle(previous.back(), cam.back()).max(angle(previous.basis().0, cam.basis().0)) < 10.0);
                assert_eq!(done, frame == 60);
                if frame == 30 {
                    assert_ne!(cam.basis(), from.basis());
                    assert_ne!(cam.basis(), anim.to.basis());
                }
                previous = cam;
            }
            h.app.cam = anim.sample(anim.start + anim.duration / 2.0).0;
            let intermediate = h.app.cam;
            let first_to = anim.to;
            navigate(&mut h.app, Some("left"), None);
            let next = h.app.cam_anim.expect("retargeted glide");
            assert_eq!(next.from, intermediate, "the glide continues from the camera as it is");
            assert_ne!(next.to.basis(), first_to.basis(), "the destination is a further turn from the in-flight target");
            h.app.cam = next.to;
            h.app.cancel_view_animation();
        }
    }

    /// #8912: two quick Right clicks retarget from the first glide's destination, not from mid-flight.
    #[test]
    fn rapid_cube_turns_compose_from_the_in_flight_target() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        assert!(h.app.preferences.look_at_anim);
        navigate(&mut h.app, Some("right"), None);
        let first = h.app.cam_anim.expect("first turn");
        let first_to = first.to;
        let mid = first.sample(first.start + first.duration / 2.0).0;
        h.app.cam = mid;
        navigate(&mut h.app, Some("right"), None);
        let second = h.app.cam_anim.expect("retargeted turn");
        let composed: Camera = serde_json::from_value(
            h.app.session.execute("view.navigate", &json!({"camera": first_to, "action": "right"})).unwrap()["camera"].clone(),
        )
        .unwrap();
        let from_mid: Camera =
            serde_json::from_value(h.app.session.execute("view.navigate", &json!({"camera": mid, "action": "right"})).unwrap()["camera"].clone())
                .unwrap();
        assert!(second.to.back().dist(composed.back()) < 1e-6, "second click turns from the in-flight target");
        assert!(second.to.back().dist(from_mid.back()) > 1e-3, "not from the mid-glide camera");
        assert!(second.from.back().dist(mid.back()) < 1e-6, "ease still starts where the camera is");
    }

    /// A view-cube click glides, but a mouse orbit started at once cancels the glide: the camera
    /// then follows the pointer one to one, and nothing keeps easing after the pointer stops.
    #[test]
    fn a_mouse_orbit_right_after_a_cube_click_follows_the_pointer_one_to_one() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        h.frames(4);
        let p = crate::scenario::handle_at("navigation_cube:top").unwrap();
        let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        h.app.synthetic.extend([egui::Event::PointerMoved(p), button(true), button(false)]);
        h.frames(3);
        assert!(h.app.cam_anim.is_some(), "the click glides");
        let mut expected = h.app.cam;
        h.call("ui.drag", json!({"x0":800,"y0":500,"x1":850,"y1":520,"button":"middle","shift":true,"steps":20}));
        assert!(h.app.cam_anim.is_none(), "the orbit cancels the glide");
        expected.orbit(50.0, 20.0);
        // The glide may step once, in the frame the orbit starts and cancels it.
        assert!((h.app.cam.yaw - expected.yaw).abs() < 0.03 && (h.app.cam.pitch - expected.pitch).abs() < 0.03, "{:?} vs {:?}", h.app.cam, expected);
        let done = h.app.cam;
        h.frames(30);
        assert_eq!(h.app.cam, done, "nothing keeps easing after the pointer stops");
    }

    #[test]
    fn cube_faces_corners_roll_and_home_drive_the_shared_command() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        h.app.preferences.look_at_anim = false;
        let before = h.app.session.doc.to_json();
        for (name, direction) in [
            ("top", Vec3::Z),
            ("front", -Vec3::Y),
            ("right", Vec3::X),
            ("top_front", Vec3::Z - Vec3::Y),
            ("front_right", Vec3::X - Vec3::Y),
            ("top_front_right", Vec3::Z - Vec3::Y + Vec3::X),
        ] {
            h.call("ui.view", json!({"view":"iso"}));
            let p = crate::scenario::handle_at(&format!("navigation_cube:{name}")).unwrap();
            let scale = crate::scenario::rect_of("axis_view:outer").unwrap().width() / DESIGN_SIZE;
            let offsets = if name == "top_front_right" {
                vec![vec2(0.0, 0.0), vec2(8.0, 0.0), vec2(-8.0, 0.0), vec2(0.0, 8.0), vec2(0.0, -8.0)]
            } else {
                vec![vec2(0.0, 0.0)]
            };
            for offset in offsets {
                let offset = offset * scale;
                h.call("ui.view", json!({"view":"iso"}));
                h.call("ui.click", json!({"x":p.x+offset.x,"y":p.y+offset.y}));
                assert!(h.app.cam.back().dist(direction.normalized().unwrap()) < 2e-6, "{name} {offset:?}");
                assert!(h.app.session.selection.is_empty());
            }
        }
        let camera = h.app.cam;
        let p = crate::scenario::handle_at("navigation_cube:control:clockwise").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!((h.app.cam.roll - camera.roll - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(h.app.cam.back().dist(camera.back()) < 1e-6);
        let p = crate::scenario::handle_at("navigation_cube:control:home").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!(h.app.cam.back().dist(Camera::default().back()) < 1e-6);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert!(h.app.session.undo.is_empty());
    }
}
