//! Original, code-drawn navigation cube with chamfered edges and corners: every one of its
//! 26 views (6 faces, 12 edges, 8 corners) is one flat facet that picks it.
use super::*;
use crate::icons::navigation::{Facet, arrow, curved_arrow, paint_cube};
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

fn project(cam: &Camera, v: Vec3, scale: f64) -> egui::Vec2 {
    let (r, u, _) = cam.basis();
    let v = cam.from_frame(v);
    vec2((v.dot(r) * scale) as f32, -(v.dot(u) * scale) as f32)
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
fn view_name(d: Vec3) -> String {
    direction_label(d)
        .split(" · ")
        .map(|part| crate::i18n::tr(&format!("ui.heading_{}", part.to_ascii_lowercase())).to_string())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn facets(cam: &Camera, c: Pos2, half: f64) -> Vec<Facet> {
    if !cam.is_valid() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut add = |d: Vec3, normal: Vec3, mut vertices: Vec<Vec3>, axes: Option<(Vec3, Vec3)>| {
        let depth = cam.from_frame(normal).dot(cam.back());
        if depth <= 1e-5 {
            return;
        }
        let centre = vertices.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) / vertices.len() as f64;
        let at = c + project(cam, centre, half);
        vertices.sort_by(|a, b| {
            let a = project(cam, *a - centre, half);
            let b = project(cam, *b - centre, half);
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
            text_axes: axes.map(|(r, u)| (project(cam, r, 1.0), -project(cam, u, 1.0))),
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

pub(super) fn drag_origin(cam: &Camera, centre: Pos2, pointer: Pos2, scale: f32) -> bool {
    hit(&facets(cam, centre, HALF * f64::from(scale)), pointer).is_some()
}

fn navigate(app: &mut SolveApp, action: Option<&str>, dir: Option<Vec3>) {
    let mut params = json!({"camera":app.cam});
    if let Some(action) = action {
        params["action"] = json!(action);
    }
    if let Some(d) = dir {
        params["direction"] = json!([d.x, d.y, d.z]);
    }
    let _ = app.run("view.navigate", params);
}

fn control(app: &mut SolveApp, ui: &mut egui::Ui, bounds: Rect, action: &str, tooltip: &str) -> bool {
    crate::scenario::publish_handle(&format!("navigation_cube:control:{action}"), bounds.center());
    let response = ui.interact(bounds, ui.id().with(("navigation_cube", action)), Sense::click()).on_hover_text(tooltip);
    let hover = response.hovered();
    if response.clicked() {
        navigate(app, Some(action), None);
    }
    hover
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
    let faces = facets(&app.cam, c, HALF * f64::from(scale));
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
        ui.interact(Rect::from_center_size(hover.unwrap_or(face.centre), vec2(2.0, 2.0)), ui.id().with("navigation_cube_tip"), Sense::hover())
            .on_hover_text(crate::i18n::format(
                if app.preferences.free_orbit { "ui.navigation_cube_view_free" } else { "ui.navigation_cube_view" },
                &[("view", &face.label)],
            ));
        if ui.input(|i| i.pointer.primary_clicked()) {
            navigate(app, None, Some(face.direction));
        }
    }
    let square_on = faces.iter().filter(|f| f.text_axes.is_some()).count() == 1;
    let (distance, button_size) = if square_on { (54.0, 20.0) } else { (74.0, 16.0) };
    for (action, offset, direction, tooltip) in [
        ("left", vec2(-distance, 0.0), vec2(1.0, 0.0), "Orbit left 90°"),
        ("right", vec2(distance, 0.0), vec2(-1.0, 0.0), "Orbit right 90°"),
        ("up", vec2(0.0, -distance), vec2(0.0, 1.0), "Orbit up 90°"),
        ("down", vec2(0.0, distance), vec2(0.0, -1.0), "Orbit down 90°"),
    ] {
        let at = c + offset * scale;
        let hovered = control(app, ui, Rect::from_center_size(at, vec2(button_size, button_size) * scale), action, tooltip);
        if active {
            arrow(&painter, at, direction * scale, hovered, t);
        }
    }
    for (action, offset, size, clockwise, tooltip) in [
        ("counterclockwise", vec2(42.0, -72.0), vec2(40.0, 20.0), false, "Rotate view counterclockwise 90°"),
        ("clockwise", vec2(72.0, -42.0), vec2(20.0, 40.0), true, "Rotate view clockwise 90°"),
    ] {
        let hovered = control(app, ui, Rect::from_center_size(c + offset * scale, size * scale), action, tooltip);
        if active {
            curved_arrow(&painter, c + vec2(42.0, -42.0) * scale, clockwise, hovered, t, scale);
        }
    }
    let home = Rect::from_center_size(c + vec2(-65.0, -65.0) * scale, vec2(26.0, 26.0) * scale);
    crate::scenario::publish_target("axis_view:home", home);
    let home_hover = control(app, ui, home, "home", "Home: isometric view and fit model");
    if active {
        icons::paint_icon(&painter, home.shrink(4.0 * scale), "home", f32::from((if home_hover { t.accent } else { t.cube_face }).a()) / 255.0);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
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
                    let faces = facets(&cam, Pos2::ZERO, HALF);
                    assert!(!faces.is_empty());
                    for (i, face) in faces.iter().enumerate() {
                        assert_eq!(hit(&faces, face.centre), Some(i), "{up:?} {view:?} {roll} {}", face.label);
                    }
                    assert!(hit(&faces, pos2(80.0, 80.0)).is_none());
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
            navigate(&mut h.app, Some("left"), None);
            assert_eq!(h.app.cam_anim.unwrap().from, intermediate, "another click starts where the camera is");
            h.app.cam = anim.to;
            h.app.cancel_view_animation();
        }
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
