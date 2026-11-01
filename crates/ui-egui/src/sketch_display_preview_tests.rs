use super::{Tool, polygon_outline, preview_curve};
use crate::scenario::Harness;
use crate::viewport::Proj;
use serde_json::{Value, json};
use solidcraft_engine::geom::{Plane, Vec2, Vec3};
use solidcraft_engine::render::{Camera, Mat4};
use solidcraft_engine::sketch::Sketch;

fn tool(command: &str, points: &[[f64; 2]]) -> Tool {
    let mut tool = Tool::for_command(command).unwrap();
    tool.pts = points.iter().map(|p| (Vec2::new(p[0], p[1]), None)).collect();
    tool
}
fn spans(sk: &Sketch) -> Vec<solidcraft_engine::geom::Seg2> {
    sk.exact_segs(0).unwrap_or_else(|| sk.segs(0))
}
fn projection(scale: f64) -> Proj {
    Proj {
        cam: Camera::default(),
        vp: Mat4([[scale / 700.0, 0.0, 0.0, 0.0], [0.0, scale / 450.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.5, 1.0]]),
        rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)),
    }
}
fn screen_error(paths: &[Vec<Vec3>], point: Vec3, proj: &Proj, ppp: f32) -> f32 {
    let point = proj.to_screen(point).unwrap();
    paths
        .iter()
        .flat_map(|part| part.windows(2))
        .map(|pair| {
            let a = proj.to_screen(pair[0]).unwrap();
            let b = proj.to_screen(pair[1]).unwrap();
            let ab = b - a;
            let t = if ab.length_sq() > 0.0 { ((point - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
            point.distance(a + ab * t) * ppp
        })
        .fold(f32::INFINITY, f32::min)
}

#[test]
fn pixel_preview_temporary_curves_match_committed_geometry_and_sampling() {
    let cases: Vec<(&str, Vec<[f64; 2]>, [f64; 2], Option<f64>, Option<f64>, Value)> = vec![
        ("sketch.circle.center", vec![[0., 0.]], [1., 0.], None, None, json!({"center":[0,0],"radius":1})),
        ("sketch.circle.two_point", vec![[-1., 0.]], [1., 0.], None, None, json!({"p0":[-1,0],"p1":[1,0]})),
        // #1358: the whole line, both sides of the middle.
        ("sketch.line.midpoint", vec![[0., 0.]], [1., 0.5], None, None, json!({"mid":[0,0],"end":[1,0.5]})),
        ("sketch.circle.three_point", vec![[1., 0.], [-1., 0.]], [0., 1.], None, None, json!({"p0":[1,0],"p1":[-1,0],"p2":[0,1]})),
        ("sketch.arc.three_point", vec![[1., 0.], [-1., 0.]], [0., -1.], None, None, json!({"start":[1,0],"end":[-1,0],"through":[0,-1]})),
        (
            "sketch.arc.center_point",
            vec![[0., 0.], [1., 0.]],
            [0., -1.],
            Some(-std::f64::consts::FRAC_PI_2),
            None,
            json!({"center":[0,0],"start":[1,0],"sweep":-90}),
        ),
        ("sketch.ellipse", vec![[0., 0.], [1., 0.]], [0., 0.05], None, None, json!({"center":[0,0],"major":[1,0],"minor":[0,0.05]})),
        ("sketch.conic", vec![[0., 0.], [1., 0.]], [0.5, 0.5], None, None, json!({"start":[0,0],"end":[1,0],"apex":[0.5,0.5]})),
        // A typed Rho previews the curve the third click is about to commit, not the default
        // parabola: an ellipse below 0.5, a hyperbola above it.
        ("sketch.conic", vec![[0., 0.], [1., 0.]], [0.5, 0.5], None, Some(0.2), json!({"start":[0,0],"end":[1,0],"apex":[0.5,0.5],"rho":0.2})),
        ("sketch.conic", vec![[0., 0.], [1., 0.]], [0.5, 0.5], None, Some(0.85), json!({"start":[0,0],"end":[1,0],"apex":[0.5,0.5],"rho":0.85})),
        (
            "sketch.spline.fit_point",
            vec![[0., 0.], [0.3, 0.8], [0.6, -0.8]],
            [1., 0.],
            None,
            None,
            json!({"points":[[0,0],[0.3,0.8],[0.6,-0.8],[1,0]]}),
        ),
        (
            "sketch.spline.control_point_5",
            vec![[0., 0.], [0.2, 0.9], [0.4, -0.9], [0.6, 0.9], [0.8, -0.9]],
            [1., 0.],
            None,
            None,
            json!({"points":[[0,0],[0.2,0.9],[0.4,-0.9],[0.6,0.9],[0.8,-0.9],[1,0]]}),
        ),
        (
            "sketch.spline.fit_point",
            vec![[0., 0.], [1., 0.], [0.5, 1.]],
            [0., 0.],
            None,
            None,
            json!({"points":[[0,0],[1,0],[0.5,1]],"periodic":true}),
        ),
    ];
    for (command, points, hover, sweep, rho, params) in cases {
        let tool = tool(command, &points);
        let preview = preview_curve(&tool, Vec2::new(hover[0], hover[1]), sweep, rho, None).unwrap();
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run(command, params).unwrap();
        let state = h.app.session.world_state().unwrap();
        let committed = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        let a = spans(&preview);
        let b = spans(committed);
        assert_eq!(a.len(), b.len(), "{command}");
        for (a, b) in a.iter().zip(&b) {
            for i in 0..=50 {
                assert!(a.point_at(i as f64 / 50.0).dist(b.point_at(i as f64 / 50.0)) < 1e-8, "{command}");
            }
        }
        for ppp in [1.0, 1.5, 2.0] {
            for scale in [2.0, 20.0, 220.0, 1000.0] {
                let proj = projection(scale / f64::from(ppp));
                let temporary = crate::sketch_display::curve_parts(&preview, 0, &Plane::XY, &proj, ppp);
                let permanent = crate::sketch_display::curve_parts(committed, 0, &Plane::XY, &proj, ppp);
                assert_eq!(temporary, permanent, "{command}");
                assert!(temporary.iter().map(Vec::len).sum::<usize>() <= 65_536);
                let mut visible = 0;
                for segment in &a {
                    for i in 0..=300 {
                        let point = Plane::XY.to_world(segment.point_at(i as f64 / 300.0));
                        if !proj.rect.contains(proj.to_screen(point).unwrap()) {
                            continue;
                        }
                        visible += 1;
                        let error = screen_error(&temporary, point, &proj, ppp);
                        assert!(error <= 1.0, "{command}, scale{scale}, ppp{ppp}: {error}");
                    }
                }
                if scale <= 220.0 {
                    assert_eq!(visible, a.len() * 301, "the low/normal zoom fixture must test the entire curve: {command}");
                }
            }
        }
    }
}

#[test]
fn pixel_preview_fully_offscreen_circle_is_clipped_without_false_connections() {
    let curve = preview_curve(&tool("sketch.circle.center", &[[0., 0.]]), Vec2::X, None, None, None).unwrap();
    let proj = projection(1000.0);
    // The viewport half diagonal is about 832px: a 1000px circle never enters it.
    assert!((proj.rect.width() * 0.5).hypot(proj.rect.height() * 0.5) < 1000.0);
    assert!(crate::sketch_display::curve_parts(&curve, 0, &Plane::XY, &proj, 1.0).is_empty());
}

#[test]
fn pixel_preview_rejects_degenerate_and_unbounded_input_without_changing_document() {
    assert!(preview_curve(&tool("sketch.circle.center", &[[0., 0.]]), Vec2::new(f64::NAN, 0.), None, None, None).is_none());
    assert!(preview_curve(&tool("sketch.arc.three_point", &[[0., 0.], [1., 0.]]), Vec2::new(2., 0.), None, None, None).is_none());
    let points = vec![[0., 0.]; 500];
    assert!(preview_curve(&tool("sketch.spline.fit_point", &points), Vec2::X, None, None, None).is_none());
}

#[test]
fn polygon_preview_squares_have_the_expected_rotation_and_size() {
    let cases = [
        ("sketch.polygon.inscribed", [10.0, 0.0], [[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0], [0.0, -10.0], [10.0, 0.0]]),
        ("sketch.polygon.circumscribed", [10.0, 0.0], [[10.0, 10.0], [-10.0, 10.0], [-10.0, -10.0], [10.0, -10.0], [10.0, 10.0]]),
        ("sketch.polygon.edge", [20.0, 0.0], [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0], [0.0, 0.0]]),
    ];
    for (command, hover, expected) in cases {
        let t = tool(command, &[[0.0, 0.0]]);
        let outline = polygon_outline(&t, Vec2::new(hover[0], hover[1]), 4.0).unwrap();
        assert_eq!(outline.len(), expected.len());
        for (actual, expected) in outline.iter().zip(expected) {
            assert!(actual.dist(Vec2::new(expected[0], expected[1])) < 1e-9, "{command}: {actual:?}");
        }
    }
}

#[test]
fn polygon_preview_rejects_invalid_or_unbounded_input() {
    let t = tool("sketch.polygon.inscribed", &[[0.0, 0.0]]);
    for sides in [0.0, 2.0, 3.5, 201.0, f64::INFINITY, f64::NAN] {
        assert!(polygon_outline(&t, Vec2::X, sides).is_none());
    }
    assert!(polygon_outline(&t, Vec2::ZERO, 6.0).is_none());
    assert!(polygon_outline(&t, Vec2::new(f64::NAN, 0.0), 6.0).is_none());
    assert_eq!(polygon_outline(&t, Vec2::X, 200.0).unwrap().len(), 201);
    let t = tool("sketch.polygon.inscribed", &[[f64::INFINITY, 0.0]]);
    assert!(polygon_outline(&t, Vec2::X, 6.0).is_none());
}

#[test]
fn slot_preview_matches_every_committed_side_and_cap() {
    let cases = [
        ("sketch.slot.center_to_center", vec![[0., 0.], [40., 0.]], [20., 8.], json!({"p0":[0,0],"p1":[40,0],"width":16})),
        // Free pointer input retains the final click's minimum width.
        ("sketch.slot.center_to_center", vec![[0., 0.], [40., 0.]], [20., 0.], json!({"p0":[0,0],"p1":[40,0],"width":0.001})),
        ("sketch.slot.overall", vec![[0., 0.], [40., 0.]], [20., -8.], json!({"p0":[0,0],"p1":[40,0],"width":16})),
        ("sketch.slot.center_point", vec![[0., 0.], [20., 0.]], [10., 8.], json!({"center":[0,0],"end":[20,0],"width":16})),
        ("sketch.slot.arc_center", vec![[0., 0.], [20., 0.], [0., 20.]], [0., 24.], json!({"center":[0,0],"start":[20,0],"end":[0,20],"width":8})),
        (
            "sketch.slot.arc_three_point",
            vec![[20., 0.], [0., 20.], [-20., 0.]],
            [-20., 4.],
            json!({"start":[20,0],"through":[0,20],"end":[-20,0],"width":8}),
        ),
        // A sloping, reversed axis and the opposite side of a three-point centre arc.
        ("sketch.slot.center_to_center", vec![[30., 40.], [0., 0.]], [8., -6.], json!({"p0":[30,40],"p1":[0,0],"width":20})),
        (
            "sketch.slot.arc_three_point",
            vec![[-20., 0.], [0., 20.], [20., 0.]],
            [20., -4.],
            json!({"start":[-20,0],"through":[0,20],"end":[20,0],"width":8}),
        ),
    ];
    for (command, points, hover, params) in cases {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let preview = preview_curve(&tool(command, &points), Vec2::new(hover[0], hover[1]), None, None, None)
            .unwrap_or_else(|| panic!("{command}: no slot outline before the width click"));
        let ctx = egui::Context::default();
        let proj = projection(2.0);
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() }, |ui| {
            super::paint_preview_curve(preview.clone(), &Plane::XY, ui.painter(), &proj, egui::Stroke::new(1.5, egui::Color32::WHITE), 0);
        });
        output.textures_delta.clear();
        assert_eq!(
            output.shapes.iter().filter(|shape| matches!(shape.shape, egui::Shape::Path(_))).count(),
            4,
            "{command}: paint all four outline curves"
        );
        h.app.run(command, params).unwrap();
        let state = h.app.session.world_state().unwrap();
        let committed = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert_eq!(preview.curves.len(), committed.curves.len(), "{command}");
        for index in 0..committed.curves.len() {
            let a = preview.exact_segs(index).unwrap_or_else(|| preview.segs(index));
            let b = committed.exact_segs(index).unwrap_or_else(|| committed.segs(index));
            assert_eq!(a.len(), b.len(), "{command}, curve {index}");
            assert_eq!(preview.curves[index].construction, committed.curves[index].construction);
            for (a, b) in a.iter().zip(&b) {
                for i in 0..=20 {
                    assert!(a.point_at(f64::from(i) / 20.0).dist(b.point_at(f64::from(i) / 20.0)) < 1e-7, "{command}, curve {index}");
                }
            }
        }
    }
}

#[test]
fn typed_small_slot_width_preview_matches_the_final_click() {
    for (command, length) in [("sketch.slot.center_to_center", 40.0), ("sketch.slot.overall", 40.0), ("sketch.slot.overall", 0.0002)] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let mut t = tool(command, &[[0., 0.], [length, 0.]]);
        t.dims = crate::sketch_dims::boxes_for(command, 2);
        t.dims[0].value = "0.0002 / 2".into();
        t.dims[0].locked = true;
        t.hover = Some((Vec2::new(length * 0.5, 8.), None));
        crate::sketch_dims::validate(&h.app, &t).unwrap();

        // Zoom into the first cap: the accepted typed width is smaller than the pointer's
        // minimum width, and must still produce the same visible outline as the final click.
        let ctx = egui::Context::default();
        let proj = projection(2_000_000.0);
        let input = || egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() };
        let mut output = ctx.run_ui(input(), |ui| super::preview(&h.app, &t, ui.painter(), &proj));
        output.textures_delta.clear();
        let paths = |output: egui::FullOutput| {
            output
                .shapes
                .into_iter()
                .filter_map(|s| match s.shape {
                    egui::Shape::Path(path) => Some(path.points),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let preview = paths(output);
        assert!(h.app.session.world_state().unwrap().sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.curves.is_empty());

        let click = crate::sketch_dims::placed(&h.app, &t, t.hover.as_ref().unwrap());
        h.app.tool = Some(t);
        super::place(&mut h.app, click);
        let state = h.app.session.world_state().unwrap();
        let committed = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert_eq!(committed.curves.len(), 4, "{command}: final click commits the typed width");
        let solidcraft_engine::geom::Seg2::Arc { radius, .. } = committed.segs(1)[0] else { panic!("missing slot cap") };
        assert!((radius - 0.00005).abs() < 1e-10, "{command}: the typed width determines the cap radius");
        let mut output = ctx.run_ui(input(), |ui| {
            super::paint_preview_curve(committed.clone(), &Plane::XY, ui.painter(), &proj, egui::Stroke::new(1.5, egui::Color32::WHITE), 0);
        });
        output.textures_delta.clear();
        let expected = paths(output);
        assert!(!expected.is_empty(), "{command}: the zoomed cap must be visible");
        assert_eq!(preview.len(), expected.len(), "{command}: preview and committed paths");
        for (preview, expected) in preview.iter().zip(&expected) {
            assert_eq!(preview.len(), expected.len(), "{command}: sampled path vertices");
            for (preview, expected) in preview.iter().zip(expected) {
                assert!(preview.distance(*expected) < 0.1, "{command}: preview {preview:?}, committed {expected:?}");
            }
        }
    }
}

#[test]
fn slot_preview_rejects_invalid_extents() {
    let cases = [
        ("sketch.slot.center_to_center", vec![[0., 0.], [0., 0.]], [0., 8.]),
        ("sketch.slot.overall", vec![[0., 0.], [10., 0.]], [5., 5.]),
        ("sketch.slot.overall", vec![[0., 0.], [10., 0.]], [5., 8.]),
        ("sketch.slot.arc_center", vec![[0., 0.], [20., 0.], [0., 20.]], [0., 40.]),
        ("sketch.slot.arc_center", vec![[0., 0.], [0., 20.], [0., 0.]], [0., 24.]),
        ("sketch.slot.arc_three_point", vec![[0., 0.], [10., 0.], [20., 0.]], [20., 4.]),
    ];
    for (command, points, hover) in cases {
        assert!(preview_curve(&tool(command, &points), Vec2::new(hover[0], hover[1]), None, None, None).is_none(), "{command}");
    }
    let t = tool("sketch.slot.center_to_center", &[[0., 0.], [40., 0.]]);
    for hover in [Vec2::new(f64::NAN, 0.), Vec2::new(0., f64::INFINITY), Vec2::new(f64::MAX, f64::MAX)] {
        assert!(preview_curve(&t, hover, None, None, None).is_none());
    }
    for width in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(preview_curve(&t, Vec2::new(20., 8.), None, None, Some(width)).is_none());
    }
}
