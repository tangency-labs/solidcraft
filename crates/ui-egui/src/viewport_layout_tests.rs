use super::*;
use crate::scenario::Harness;
use crate::viewport::NavMode;
use serde_json::json;
use solidcraft_engine::render::StandardView;

fn bar_rect(canvas: Rect) -> Rect {
    crate::navigation_bar_view::measure(canvas, &Default::default(), crate::navigation::NavCaps::MODEL, |_| 0.0).rect
}

fn assert_navigator_shared_zoom(app: &crate::SolveApp, camera: Camera) {
    let context = format!(
        "host={}, measured={}, render={}, fov={}, up={:?}, viewport={:?}, shell_content={:?}",
        app.universal_panel_host_enabled(),
        app.universal_panel_frame().is_some(),
        crate::render_workspace::active(app),
        camera.fov,
        camera.up,
        app.viewport.rect,
        app.shell.content,
    );
    if crate::render_workspace::active(app) && app.universal_panel_frame().is_some() {
        let layout = app.viewport.layout.as_ref().expect(&context);
        let usable = crate::shell::host_usable(app).expect(&context);
        let picture = crate::render_workspace::host_picture_layout(app, layout, &usable).expect(&context);
        let region = picture.usable.as_ref().expect(&context);
        assert!(
            region
                .fit_rect_with_clearance(
                    1.0,
                    solidcraft_engine::panels::Point { x: picture.viewport.center().x, y: picture.viewport.center().y },
                    solidcraft_theme::Tokens::get().space_8,
                )
                .is_some(),
            "the Render fixture must leave clear picture room: {context}; free={:?}",
            region.free
        );
    }
    let bounds = Aabb3 { min: Vec3::new(120.0, -35.0, 5.0), max: Vec3::new(180.0, -5.0, 15.0) };
    let centre = bounds.center();
    let radius = bounds.diagonal() * 0.5;
    let offset = Vec3::new(radius, radius, radius);
    let sphere = Aabb3 { min: centre - offset, max: centre + offset };
    let mut front = if app.universal_panel_host_enabled() { camera } else { Camera::default() };
    front.set_view(StandardView::Front);
    let distance = app.try_fitted_bounds(front, &sphere).expect(&context).distance;
    let reference = app.navigator_fitted_bounds(front, &bounds).expect(&context);
    let mut directions = Vec::new();
    for view in
        [StandardView::Front, StandardView::Back, StandardView::Left, StandardView::Right, StandardView::Top, StandardView::Bottom, StandardView::Iso]
    {
        let mut requested = camera;
        requested.set_view(view);
        directions.push(requested);
    }
    for yaw in [-3.0, -1.0, 1.0, 3.0] {
        for pitch in [-1.0, 1.0] {
            directions.push(Camera {
                yaw: yaw * std::f64::consts::FRAC_PI_4,
                pitch: pitch * (1.0_f64 / 3.0_f64.sqrt()).asin(),
                roll: 0.17,
                ..camera
            });
        }
    }
    for requested in directions {
        let fitted = app.navigator_fitted_bounds(requested, &bounds).expect(&context);
        assert_eq!(fitted.distance.to_bits(), distance.to_bits(), "every navigator direction uses the Front probe's zoom");
        assert_eq!(
            (fitted.yaw, fitted.pitch, fitted.roll, fitted.up, fitted.fov),
            (requested.yaw, requested.pitch, requested.roll, requested.up, requested.fov)
        );
        if app.universal_panel_frame().is_some() {
            let layout = app.viewport.layout.as_ref().unwrap();
            let usable = crate::shell::host_usable(app).unwrap();
            let (projection, fov) = if crate::render_workspace::active(app) {
                (crate::render_workspace::host_picture_layout(app, layout, &usable).unwrap(), requested.fov)
            } else {
                (host_projection_layout(layout, &usable, layout.viewport).unwrap(), app.view_fov())
            };
            assert!((fitted.target - centre).dot(fitted.back()).abs() < 1e-8, "the contour offset must stay in the image plane");
            let viewport = projection.viewport;
            let project = |camera| {
                let effective = Camera { fov, ..camera };
                let (x, y, _) = effective.to_screen(centre, f64::from(viewport.width()), f64::from(viewport.height()), distance * 4.0).unwrap();
                pos2(viewport.left() + x as f32, viewport.top() + y as f32)
            };
            let focus = project(fitted);
            let reference_focus = project(reference);
            assert!((focus - reference_focus).length() < 1e-3, "every navigator direction keeps one screen centre: {focus:?} vs {reference_focus:?}");
            assert!(
                projection.usable.as_ref().unwrap().free.iter().any(|piece| crate::panel_runtime::egui_rect(*piece).contains(focus)),
                "the same object centre lands in the usable picture, although its world-space target offset rotates"
            );
        } else {
            assert_eq!(fitted.target, centre, "Classic and an unmeasured host keep the legacy centre");
        }
    }
}

fn navigator_layout_fixture(host: bool, measured: bool, render: bool) -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    if render {
        crate::render_workspace::enter(&mut h.app, true).unwrap();
        h.app.run("view.render_stage", json!({"stage":"frame"})).unwrap();
        h.app.run("render.output.size", json!({"width":400,"height":1600})).unwrap();
    }
    // Start from a coherent Classic composition, including the shell content
    // that anchors Render's picture, before optionally enabling the host.
    h.frames(4);
    if host {
        crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
        if measured {
            h.frames(4);
            if !render {
                let viewport = h.app.viewport.rect.unwrap();
                // Design has an asymmetric contour; Render keeps its real docks
                // and Frame controls so the authored picture remains available.
                h.app.viewport.host_windows =
                    Some(vec![Rect::from_min_max(pos2(viewport.left() + 550.0, viewport.top()), pos2(viewport.right(), viewport.bottom() - 300.0))]);
            }
        }
    }
    assert_eq!(h.app.universal_panel_host_enabled(), host);
    assert_eq!(h.app.universal_panel_frame().is_some(), measured);
    h
}

#[test]
fn navigator_front_probe_shares_zoom_in_classic_measured_and_unmeasured_hosts() {
    for (host, measured) in [(false, false), (true, false), (true, true)] {
        let mut h = navigator_layout_fixture(host, measured, false);
        for perspective in [false, true] {
            h.app.ui.perspective = perspective;
            h.app.ui.auto_ortho_faces = false;
            for up in [solidcraft_engine::render::UpAxis::Z, solidcraft_engine::render::UpAxis::Y] {
                assert_navigator_shared_zoom(&h.app, Camera { up, fov: 0.95, ..Default::default() });
            }
        }
    }
}

#[test]
fn navigator_render_keeps_classic_probe_and_host_authored_lens_and_picture() {
    for (host, measured) in [(false, false), (true, false), (true, true)] {
        let mut h = navigator_layout_fixture(host, measured, true);
        for fov in [0.0, 0.95] {
            // Host framing uses the authored lens even when Design preferences disagree.
            h.app.ui.perspective = fov == 0.0;
            h.app.ui.auto_ortho_faces = false;
            assert_ne!(h.app.view_fov(), fov);
            let layout = h.app.viewport.layout.as_ref().unwrap();
            let picture = crate::render_workspace::picture_layout(&h.app, layout);
            let outer = h.app.viewport.rect.unwrap();
            assert!(picture.viewport.width() < outer.width(), "the portrait frame must exercise a distinct projection");
            for up in [solidcraft_engine::render::UpAxis::Z, solidcraft_engine::render::UpAxis::Y] {
                assert_navigator_shared_zoom(&h.app, Camera { up, fov, ..Default::default() });
            }
        }
        if measured {
            h.app.run("view.render_stage", json!({"stage":"look"})).unwrap();
            h.frames(4);
            assert!(!crate::render_workspace::shot::overlay_visible(&h.app));
            let layout = h.app.viewport.layout.as_ref().unwrap();
            let usable = crate::shell::host_usable(&h.app).unwrap();
            let picture = crate::render_workspace::host_picture_layout(&h.app, layout, &usable).unwrap();
            let painted = crate::scenario::rect_of("render:product:frame").unwrap();
            assert!(painted.width() < h.app.viewport.rect.unwrap().width());
            assert_eq!(picture.viewport, painted, "host Fit and Centre must use the painted portrait even without guides");
        }
    }
}

#[test]
fn host_picture_projection_clips_real_controls_and_refuses_invalid_domains() {
    use crate::panel_runtime::{egui_rect, logical_rect, resolve_usable_region};
    let viewport = Rect::from_min_max(pos2(0.0, 34.0), pos2(1400.0, 950.0));
    let picture = Rect::from_min_max(pos2(420.0, 160.0), pos2(980.0, 860.0));
    let outside = Rect::from_min_size(pos2(0.0, 34.0), vec2(1400.0, 100.0));
    let crossing = Rect::from_min_size(pos2(300.0, 300.0), vec2(220.0, 120.0));
    let usable = resolve_usable_region(logical_rect(viewport), &[logical_rect(outside), logical_rect(crossing)]).unwrap();
    let mut layout = Layout::host_with_axis(viewport, &usable, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, Rect::NOTHING, false);
    layout.navigation = Rect::from_min_size(pos2(900.0, 800.0), vec2(200.0, 40.0));
    let projection = host_projection_layout(&layout, &usable, picture).unwrap();
    let region = projection.usable.as_ref().unwrap();
    assert_eq!(projection.viewport, picture);
    assert_eq!(egui_rect(region.domain), picture);
    assert_eq!(region.obstacles.len(), 2, "only real intersections remain");
    assert!(region.obstacles.contains(&logical_rect(crossing.intersect(picture))));
    assert!(region.obstacles.contains(&logical_rect(layout.navigation.intersect(picture))));
    assert!(region.obstacles.iter().all(|rect| picture.contains_rect(egui_rect(*rect))));
    assert_eq!(layout.viewport, viewport, "preparing a fit never changes the canvas");
    for invalid in [
        Rect::NOTHING,
        Rect::from_min_size(picture.min, vec2(0.0, 1.0)),
        viewport.expand(1.0),
        Rect::from_min_size(pos2(f32::NAN, 0.0), vec2(10.0, 10.0)),
    ] {
        assert!(host_projection_layout(&layout, &usable, invalid).is_none());
    }
}

#[test]
fn host_picture_fit_keeps_product_lens_and_padded_corners_in_the_picture_contour() {
    use crate::panel_runtime::{egui_rect, logical_rect, resolve_usable_region};
    let viewport = Rect::from_min_max(pos2(0.0, 34.0), pos2(1400.0, 950.0));
    // A 40 px inset leaves 1320 × 836 px: portrait 1:4 and landscape 12:5 frames.
    for size in [vec2(209.0, 836.0), vec2(1320.0, 550.0)] {
        let picture = Rect::from_center_size(viewport.center(), size);
        let obstacle = Rect::from_min_size(picture.min, picture.size() * 0.25);
        let usable = resolve_usable_region(logical_rect(viewport), &[logical_rect(obstacle)]).unwrap();
        let layout = Layout::host_with_axis(viewport, &usable, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, Rect::NOTHING, false);
        let projection = host_projection_layout(&layout, &usable, picture).unwrap();
        let region = projection.usable.as_ref().unwrap();
        for fov in [0.0, 0.95] {
            for bounds in [
                Aabb3 { min: Vec3::new(-60.0, -15.0, -2.0), max: Vec3::new(60.0, 15.0, 2.0) },
                Aabb3 { min: Vec3::ZERO, max: Vec3::new(100.0, 0.0, 0.0) },
            ] {
                let camera = Camera { fov, ..Default::default() };
                let fitted = fit_host_bounds(camera, &bounds, &projection, region, camera.fov, 1.0).unwrap();
                assert_eq!(fitted.fov, fov);
                let radius = (bounds.diagonal() + bounds.center().dist(fitted.target)).max(fitted.half_height() * 2.0).max(10.0);
                let mut footprint = Rect::NOTHING;
                for x in [bounds.min.x, bounds.max.x] {
                    for y in [bounds.min.y, bounds.max.y] {
                        for z in [bounds.min.z, bounds.max.z] {
                            let (x, y, depth) =
                                fitted.to_screen(Vec3::new(x, y, z), f64::from(picture.width()), f64::from(picture.height()), radius).unwrap();
                            assert!((0.0..=1.0).contains(&depth));
                            footprint.extend_with(pos2(picture.left() + x as f32, picture.top() + y as f32));
                        }
                    }
                }
                assert!(
                    region.contains_rect(logical_rect(footprint.expand(solidcraft_theme::Tokens::get().space_4))),
                    "{footprint:?} in {:?}",
                    egui_rect(region.domain)
                );
            }
        }
    }
}

#[test]
fn host_picture_centring_keeps_distance_and_selected_focus_at_the_product_lens() {
    use crate::panel_runtime::{logical_rect, resolve_usable_region};
    let viewport = Rect::from_min_max(pos2(0.0, 34.0), pos2(1200.0, 900.0));
    let picture = Rect::from_min_max(pos2(420.0, 180.0), pos2(780.0, 800.0));
    let obstacle = Rect::from_min_size(picture.min, vec2(100.0, 180.0));
    let usable = resolve_usable_region(logical_rect(viewport), &[logical_rect(obstacle)]).unwrap();
    let layout = Layout::host_with_axis(viewport, &usable, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, Rect::NOTHING, false);
    let projection = host_projection_layout(&layout, &usable, picture).unwrap();
    let safe = projection.usable.as_ref().unwrap();
    let bounds = Aabb3 { min: Vec3::new(-20.0, -10.0, 0.0), max: Vec3::new(20.0, 10.0, 0.0) };
    for fov in [0.0, 0.95] {
        for distance in [2.0, 500.0] {
            let camera = Camera { distance, fov, target: bounds.center(), ..Default::default() };
            let centred = centre_host_camera(camera, Some(&bounds), &projection, safe, camera.fov).unwrap();
            assert_eq!(centred.distance.to_bits(), distance.to_bits());
            assert_eq!(centred.fov, fov);
            let (x, y, _) = centred.to_screen(camera.target, f64::from(picture.width()), f64::from(picture.height()), 2000.0).unwrap();
            let focus = pos2(picture.left() + x as f32, picture.top() + y as f32);
            assert!(picture.contains(focus) && !obstacle.contains(focus));
        }
    }
    let tiny = Rect::from_min_size(picture.min, vec2(4.0, 4.0));
    let projection = host_projection_layout(&layout, &usable, tiny).unwrap();
    assert!(fit_host_bounds(Camera::default(), &bounds, &projection, projection.usable.as_ref().unwrap(), 0.0, 1.0).is_none());
}

#[test]
fn render_compass_avoids_the_navigation_bar_on_either_inspector_side() {
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 140.0));
    for style in [crate::prefs::NavigatorStyle::Axes, crate::prefs::NavigatorStyle::Cube] {
        for inspector_left in [false, true] {
            for bar_left in [false, true] {
                let navigation = Rect::from_min_size(pos2(if bar_left { 8.0 } else { 312.0 }, 100.0), vec2(80.0, 32.0));
                let make = if inspector_left { Layout::product_left } else { Layout::product };
                let layout = make(viewport, viewport, &[], 1.0, style, egui::Vec2::ZERO, navigation);
                assert!(!layout.axis_view.intersect(navigation).is_positive());
                if inspector_left != bar_left {
                    assert!(layout.axis_view.is_positive(), "a bar on the opposite side must leave the compass visible");
                }
                if layout.axis_view.is_positive() {
                    let edge = if inspector_left { layout.axis_view.left() - viewport.left() } else { viewport.right() - layout.axis_view.right() };
                    assert!(edge.abs() < 0.1, "a compact compass must stay on the Inspector's side");
                }
            }
        }
    }
}

fn layout(viewport: Rect, docks: &[Rect], scale: f32, style: crate::prefs::NavigatorStyle, visible: bool) -> Layout {
    let navigation = if visible {
        crate::navigation_bar_view::measure(viewport, &Default::default(), crate::navigation::NavCaps::MODEL, |_| 0.0).rect
    } else {
        Rect::NOTHING
    };
    Layout::with_content(viewport, viewport, docks, scale, style, egui::Vec2::ZERO, navigation)
}

#[test]
fn render_focus_compass_keeps_clear_of_an_exit_chip_on_either_side() {
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
    let content = Rect::from_min_max(pos2(0.0, 104.0), viewport.max);
    let navigation = crate::navigation_bar_view::measure(content, &Default::default(), crate::navigation::NavCaps::RENDER, |_| 0.0).rect;
    for x in [8.0, 1250.0] {
        let chip = Rect::from_min_size(pos2(x, content.top()), vec2(142.0, 40.0));
        let layout = Layout::product(viewport, content, &[chip], 1.0, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, navigation);
        assert!(layout.axis_view.is_positive() && layout.navigation.is_positive());
        assert!(!layout.axis_view.intersect(chip).is_positive());
        assert!(content.contains_rect(layout.axis_view));
        assert!(content.contains_rect(layout.navigation));
    }
}

#[test]
fn thin_rotated_bounds_fit_using_the_original_full_projection() {
    for (width, height, scale) in [(1400.0, 900.0, 1.0), (700.0, 450.0, 2.0), (700.0, 360.0, 2.0), (900.0, 720.0, 1.0)] {
        let vp = Rect::from_min_max(pos2(250.0, crate::toolbar::APP_BAR_HEIGHT + crate::toolbar::RIBBON_HEIGHT), pos2(width, height - 30.0));
        let dock = Rect::from_min_max(pos2(width - 205.0, vp.top()), vp.max);
        let layout = layout(vp, &[dock], scale, crate::prefs::NavigatorStyle::Axes, true);
        assert!(layout.drawable.width() >= 80.0 && layout.drawable.height() >= 30.0, "{layout:?}");
        assert!(layout.axis_view.is_positive() && layout.navigation.is_positive(), "{layout:?}");
        assert!(!layout.drawable.intersect(dock).is_positive());
        assert!(!layout.axis_view.intersect(layout.navigation).is_positive());
        // Parallel, the default angle, and the ends of the Field of view preference (#5364).
        for fov in [0.0, 0.6, 15f64.to_radians(), 90f64.to_radians()] {
            for view in [StandardView::Top, StandardView::Iso, StandardView::Right] {
                let mut cam = Camera::default();
                cam.set_view(view);
                let bounds = Aabb3 { min: Vec3::new(-30.0, -1.0, -2.0), max: Vec3::new(70.0, 2.0, 3.0) };
                let fit = fit_bounds(cam, &bounds, &layout, fov).unwrap();
                assert_eq!((cam.yaw, cam.pitch, cam.up), (fit.yaw, fit.pitch, fit.up));
                assert_eq!(fit_bounds(fit, &bounds, &layout, fov).unwrap(), fit);
                let mut effective = fit;
                effective.fov = fov;
                let radius = (bounds.diagonal() * 0.5 + bounds.center().dist(fit.target)).max(effective.half_height() * 2.0).max(10.0);
                for x in [bounds.min.x, bounds.max.x] {
                    for y in [bounds.min.y, bounds.max.y] {
                        for z in [bounds.min.z, bounds.max.z] {
                            let (sx, sy, depth) =
                                effective.to_screen(Vec3::new(x, y, z), f64::from(vp.width()), f64::from(vp.height()), radius).unwrap();
                            assert!((0.0..=1.0).contains(&depth));
                            assert!(layout.drawable.contains(pos2(vp.left() + sx as f32, vp.top() + sy as f32)));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn measured_palette_fit_exposes_endpoints_and_layout_changes_preserve_manual_view() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.settle();
        for (scale, width, height) in [(1.0, 1400.0, 900.0), (2.0, 700.0, 475.0), (2.0, 700.0, 385.0), (1.0, 900.0, 720.0)] {
            h.call("ui.prefs", json!({"ui_scale":scale}));
            h.call("ui.resize", json!({"width":width,"height":height}));
            h.settle();
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            h.app.fit_view();
            h.settle();
            let layout = h.app.viewport.layout.as_ref().unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!(layout.drawable.width() >= 80.0 && layout.drawable.height() >= 30.0, "{layout:?}");
            // Floating's card margins leave less content height at 385 px. The compass
            // intentionally disappears when it cannot fit above the bottom navigation.
            assert!(
                layout.axis_view.is_positive()
                    || layout.content.height()
                        < crate::viewport::axis_view_size(h.app.preferences.navigator_style) + layout.navigation.height() + 11.0,
                "{layout:?}"
            );
            assert!(layout.navigation.is_positive(), "{layout:?}");
            assert!(palette.width() <= 260.0, "{palette:?}");
            let projection = crate::viewport::projection(&h.app, layout.viewport);
            for point in [Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0)] {
                let p = projection.to_screen(point).unwrap();
                assert!(layout.drawable.contains(p), "endpoint {p:?} outside {layout:?}");
                assert!(!palette.contains(p));
                assert!(!layout.axis_view.contains(p));
                assert!(!layout.navigation.contains(p));
            }
            let fitted = h.app.cam;
            h.app.fit_view();
            h.settle();
            assert_eq!(h.app.cam, fitted);
            h.app.cancel_view_animation();
            h.app.cam.pan(13.0, -19.0, 500.0);
            let manual = h.app.cam;
            h.call("ui.resize", json!({"width":width+20.0,"height":height}));
            h.settle();
            assert_eq!(h.app.cam, manual);
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!(h.app.session.undo.len(), undo);
        }
    }
}

#[test]
fn hostile_or_empty_fit_keeps_the_existing_camera_valid() {
    let cam = Camera::default();
    let measured = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)), &[], 1.0, crate::prefs::NavigatorStyle::Axes, true);
    assert!(fit_bounds(cam, &Aabb3::EMPTY, &measured, 0.0).is_none());
    let bad = Aabb3 { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::new(1.0, 1.0, 1.0) };
    assert!(fit_bounds(cam, &bad, &measured, 0.6).is_none());
    let mut app = crate::SolveApp::new(Default::default(), Default::default());
    let tiny = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(5.0, 5.0)), &[], 1.0, crate::prefs::NavigatorStyle::Axes, true);
    app.viewport.layout = Some(tiny);
    let bounds = Aabb3 { min: Vec3::ZERO, max: Vec3::new(20.0, 1.0, 1.0) };
    assert_eq!(app.fitted_bounds(cam, &bounds), cam);
}

#[test]
fn host_fit_uses_the_actual_contour_and_model_extent_with_the_full_projection() {
    use crate::panel_runtime::{egui_rect, logical_rect, resolve_usable_region};
    let viewport = Rect::from_min_max(pos2(0.0, crate::toolbar::APP_BAR_HEIGHT), pos2(1400.0, 1000.0));
    let obstacles = [
        Rect::from_min_max(viewport.min, pos2(1400.0, 130.0)),
        Rect::from_min_size(pos2(0.0, 130.0), vec2(260.0, 170.0)),
        Rect::from_min_size(pos2(1180.0, 130.0), vec2(220.0, 300.0)),
        Rect::from_min_size(pos2(500.0, 340.0), vec2(220.0, 160.0)),
    ];
    let usable = resolve_usable_region(logical_rect(viewport), &obstacles.map(logical_rect)).unwrap();
    assert!(usable.free.iter().any(|piece| egui_rect(*piece).contains(pos2(100.0, 700.0))), "the short-panel gap is part of the retained contour");
    for style in [crate::prefs::NavigatorStyle::Axes, crate::prefs::NavigatorStyle::Cube] {
        let navigation = bar_rect(viewport);
        let layout = Layout::host(viewport, &usable, style, egui::Vec2::ZERO, navigation);
        assert_eq!(layout.viewport, viewport);
        assert!(layout.navigation.is_positive() && layout.axis_view.is_positive());
        for control in [layout.navigation, layout.axis_view] {
            assert!(usable.contains_rect(logical_rect(control)));
            assert!(obstacles.iter().all(|obstacle| !control.intersect(*obstacle).is_positive()));
        }
        assert!(!layout.axis_view.expand(solidcraft_theme::Tokens::get().space_4).intersect(layout.navigation).is_positive());
        for bounds in [
            Aabb3 { min: Vec3::new(-90.0, -5.0, -2.0), max: Vec3::new(110.0, 5.0, 2.0) },
            Aabb3 { min: Vec3::new(-5.0, -90.0, -2.0), max: Vec3::new(5.0, 110.0, 2.0) },
        ] {
            for view in [StandardView::Top, StandardView::Iso] {
                for fov in [0.0, 0.6] {
                    let mut cam = Camera::default();
                    cam.set_view(view);
                    let fitted = fit_host_bounds(cam, &bounds, &layout, &usable, fov, 1.0).unwrap();
                    assert_eq!((fitted.yaw, fitted.pitch, fitted.up), (cam.yaw, cam.pitch, cam.up));
                    assert_eq!(fit_host_bounds(fitted, &bounds, &layout, &usable, fov, 1.0).unwrap(), fitted);
                    let mut effective = fitted;
                    effective.fov = fov;
                    let radius = (bounds.diagonal() * 0.5 + bounds.center().dist(fitted.target)).max(effective.half_height() * 2.0).max(10.0);
                    let mut projected = Rect::NOTHING;
                    for x in [bounds.min.x, bounds.max.x] {
                        for y in [bounds.min.y, bounds.max.y] {
                            for z in [bounds.min.z, bounds.max.z] {
                                let (sx, sy, depth) = effective
                                    .to_screen(Vec3::new(x, y, z), f64::from(viewport.width()), f64::from(viewport.height()), radius)
                                    .unwrap();
                                assert!((0.0..=1.0).contains(&depth));
                                projected.extend_with(pos2(viewport.left() + sx as f32, viewport.top() + sy as f32));
                            }
                        }
                    }
                    assert!(viewport.contains_rect(projected));
                    assert!(
                        obstacles.iter().all(|obstacle| !projected.intersect(*obstacle).is_positive()),
                        "the entire model footprint must clear short panels and floating holes: {projected:?}"
                    );
                    assert!([layout.navigation, layout.axis_view, layout.confirm].iter().all(|control| !projected.intersect(*control).is_positive()));
                }
            }
        }
    }
}

#[test]
fn host_fit_keeps_thin_and_degenerate_model_footprints_padded_at_both_scales() {
    use crate::panel_runtime::{logical_rect, resolve_usable_region};
    for scale in [1.0, 2.0] {
        let ctx = crate::fonts::test_context();
        ctx.set_pixels_per_point(scale);
        crate::test_support::frame(
            &ctx,
            egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 1000.0))), focused: true, ..Default::default() },
            |ui| {
                assert_eq!(ui.ctx().pixels_per_point(), scale);
                let viewport = Rect::from_min_max(pos2(0.0, crate::toolbar::APP_BAR_HEIGHT), ui.ctx().content_rect().max);
                let obstacles = [
                    Rect::from_min_max(viewport.min, pos2(1400.0, 130.0)),
                    Rect::from_min_size(pos2(0.0, 130.0), vec2(260.0, 170.0)),
                    Rect::from_min_size(pos2(1180.0, 130.0), vec2(220.0, 300.0)),
                    Rect::from_min_size(pos2(500.0, 340.0), vec2(220.0, 160.0)),
                ];
                let usable = resolve_usable_region(logical_rect(viewport), &obstacles.map(logical_rect)).unwrap();
                let layout = Layout::host(viewport, &usable, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, bar_rect(viewport));
                let all_obstacles = obstacles
                    .iter()
                    .copied()
                    .chain([layout.navigation, layout.axis_view, layout.confirm].into_iter().filter(|rect| rect.is_positive()))
                    .map(logical_rect)
                    .collect::<Vec<_>>();
                let safe = resolve_usable_region(logical_rect(viewport), &all_obstacles).unwrap();
                for extent in [Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, 100.0, 0.0), Vec3::new(100.0, 1e-9, 0.0), Vec3::new(1e-9, 100.0, 0.0)] {
                    let bounds = Aabb3 { min: extent * -0.5, max: extent * 0.5 };
                    for fov in [0.0, 0.6] {
                        let mut cam = Camera::default();
                        cam.set_view(StandardView::Top);
                        let fitted = fit_host_bounds(cam, &bounds, &layout, &usable, fov, 1.0)
                            .expect("a line or near-flat extent must still fit with clearance");
                        assert_eq!((fitted.yaw, fitted.pitch, fitted.up, fitted.fov), (cam.yaw, cam.pitch, cam.up, cam.fov));
                        let mut effective = fitted;
                        effective.fov = fov;
                        let radius = (bounds.diagonal() * 0.5 + bounds.center().dist(fitted.target)).max(effective.half_height() * 2.0).max(10.0);
                        let mut projected = Rect::NOTHING;
                        for x in [bounds.min.x, bounds.max.x] {
                            for y in [bounds.min.y, bounds.max.y] {
                                for z in [bounds.min.z, bounds.max.z] {
                                    let (sx, sy, depth) = effective
                                        .to_screen(Vec3::new(x, y, z), f64::from(viewport.width()), f64::from(viewport.height()), radius)
                                        .unwrap();
                                    assert!((0.0..=1.0).contains(&depth));
                                    projected.extend_with(pos2(viewport.left() + sx as f32, viewport.top() + sy as f32));
                                }
                            }
                        }
                        assert!(projected.is_finite());
                        let padding = solidcraft_theme::Tokens::get().space_8;
                        let padded = projected.expand(padding - 0.01);
                        assert!(
                            safe.contains_rect(logical_rect(padded)),
                            "{scale}x thin extent {extent:?} needs its full projected clearance: {padded:?}"
                        );
                        assert!(all_obstacles.iter().all(|obstacle| !padded.intersect(crate::panel_runtime::egui_rect(*obstacle)).is_positive()));
                    }
                }
            },
        );
    }
}

#[test]
fn explicit_host_centring_preserves_zoom_and_only_falls_back_to_the_selected_focus() {
    use crate::panel_runtime::{egui_rect, logical_rect, resolve_usable_region};
    let viewport = Rect::from_min_max(pos2(0.0, crate::toolbar::APP_BAR_HEIGHT), pos2(1000.0, 700.0));
    let obstacles = [Rect::from_min_size(pos2(0.0, viewport.top()), vec2(260.0, 220.0)), Rect::from_min_size(pos2(450.0, 300.0), vec2(180.0, 160.0))];
    let usable = resolve_usable_region(logical_rect(viewport), &obstacles.map(logical_rect)).unwrap();
    let layout = Layout::host(viewport, &usable, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, bar_rect(viewport));
    let safe = layout.usable.as_ref().unwrap();
    let bounds = Aabb3 { min: Vec3::new(0.0, 0.0, 10.0), max: Vec3::new(40.0, 30.0, 10.0) };
    for fov in [0.0, 0.6] {
        for distance in [2.0, 500.0] {
            let mut camera = Camera::default();
            camera.set_view(StandardView::Top);
            camera.target = Vec3::new(20.0, 15.0, 10.0);
            camera.distance = distance;
            camera.fov = fov;
            let focus = camera.target;
            let centred = centre_host_camera(camera, Some(&bounds), &layout, &usable, fov).unwrap();
            assert_eq!(centred.distance.to_bits(), camera.distance.to_bits(), "centring must never silently zoom out an oversized selection");
            assert_eq!(centred.fov.to_bits(), camera.fov.to_bits());
            assert_eq!((centred.yaw, centred.pitch, centred.roll, centred.up), (camera.yaw, camera.pitch, camera.roll, camera.up));
            let radius = (bounds.diagonal().max(camera.half_height()).max(10.0)) * 2.0;
            let project = |point| {
                let (x, y, _) = centred.to_screen(point, f64::from(viewport.width()), f64::from(viewport.height()), radius).unwrap();
                pos2(viewport.left() + x as f32, viewport.top() + y as f32)
            };
            let selected = project(focus);
            assert!(safe.free.iter().any(|piece| egui_rect(*piece).contains(selected)));
            assert!(obstacles.iter().all(|obstacle| !obstacle.contains(selected)));
            if distance == 500.0 {
                let mut footprint = Rect::NOTHING;
                for x in [bounds.min.x, bounds.max.x] {
                    for y in [bounds.min.y, bounds.max.y] {
                        footprint.extend_with(project(Vec3::new(x, y, 10.0)));
                    }
                }
                assert!(safe.contains_rect(logical_rect(footprint)), "when the current zoom fits, the complete selected footprint lands safely");
            }
        }
    }
}

#[test]
fn host_normal_to_keeps_distance_and_selection_while_generic_view_follow_remains_exact() {
    for scale in [1.0, 2.0] {
        for perspective in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.look_at_anim = false;
            h.call("ui.resize", json!({"width":1400,"height":900}));
            h.app.run("solid.box", json!({"length":40,"width":30,"height":10})).unwrap();
            h.settle();
            h.ctx.set_pixels_per_point(scale);
            crate::panel_runtime::enable_host_specimen(&mut h.app, "docks").unwrap();
            h.frames(3);
            let body = h.app.session.model.state().bodies[0].clone();
            let face = (0..body.body.face_count()).find(|index| body.body.planar_face(*index).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
            let focus = Vec3::new(20.0, 15.0, 10.0);
            h.app.run("select.set", json!({"items":[{"type":"face","body":body.name,"index":face,"point":focus}]})).unwrap();
            let selection = h.app.session.selection.clone();
            h.app.ui.perspective = perspective;
            h.app.cam.distance = 2.0;
            h.app.cam.fov = if perspective { 0.6 } else { 0.0 };
            h.app.cam.target = Vec3::new(-50.0, -50.0, 0.0);
            let distance = h.app.cam.distance.to_bits();
            let lens = h.app.cam.fov.to_bits();
            h.app.start("view.normal_to");
            h.settle();
            assert_eq!(h.app.cam.distance.to_bits(), distance);
            assert_eq!(h.app.cam.fov.to_bits(), lens);
            assert!(h.app.cam.back().dist(Vec3::Z) < 1e-6);
            assert_eq!(h.app.session.selection, selection);
            let viewport = h.app.viewport.rect.unwrap();
            let point = crate::viewport::projection(&h.app, viewport).to_screen(focus).unwrap();
            assert!(
                h.app
                    .viewport
                    .layout
                    .as_ref()
                    .unwrap()
                    .usable
                    .as_ref()
                    .unwrap()
                    .free
                    .iter()
                    .any(|piece| crate::panel_runtime::egui_rect(*piece).contains(point)),
                "an oversized selection keeps zoom while placing its selected focus safely"
            );
            let mut requested = h.app.cam;
            requested.target = Vec3::new(123.0, 456.0, 7.0);
            requested.distance = 17.0;
            h.app.session.camera = Some(requested);
            h.app.follow_view_request(false);
            h.frames(2);
            assert_eq!(h.app.cam, requested, "generic apply/replay must not acquire an implicit centring offset");
        }
    }
}

#[test]
fn legacy_usable_space_does_not_silently_ignore_obstacles_after_sixteen() {
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let mut obstacles = (0..20).map(|index| Rect::from_min_size(pos2(index as f32 * 40.0, 0.0), vec2(20.0, 20.0))).collect::<Vec<_>>();
    let late = Rect::from_min_size(pos2(820.0, 0.0), vec2(180.0, 180.0));
    obstacles.push(late);
    let room = free_rect(viewport, &obstacles);
    assert!(room.is_positive() && room.area() > 100000.0);
    assert!(obstacles.iter().all(|obstacle| !room.intersect(*obstacle).is_positive()));
    for scale in [1.0, 2.0] {
        let layout =
            Layout::with_content(viewport, viewport, &obstacles, scale, crate::prefs::NavigatorStyle::Axes, egui::Vec2::ZERO, bar_rect(viewport));
        assert_eq!(layout.viewport, viewport);
        assert!(layout.drawable.is_positive());
        assert!(obstacles.iter().all(|obstacle| !layout.drawable.intersect(*obstacle).is_positive()));
        assert_eq!(layout.navigation, bar_rect(viewport), "legacy callers retain their supplied measured navigation policy");
    }
}

#[test]
fn fit_home_animation_and_manual_takeover_use_one_layout_solution() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":1})).unwrap();
        h.settle();
        for view in ["fit", "home"] {
            h.app.preferences.look_at_anim = false;
            h.app.animate_view(view);
            h.settle();
            let immediate = h.app.cam;
            h.app.cam.pan(50.0, 10.0, 400.0);
            h.app.preferences.look_at_anim = true;
            h.app.animate_view(view);
            h.settle();
            assert_eq!(h.app.cam, immediate);
        }
        h.app.fit_view();
        h.app.cancel_view_animation();
        h.app.cam.pan(7.0, 13.0, 400.0);
        let takeover = h.app.cam;
        h.settle();
        assert_eq!(h.app.cam, takeover);
    }
}

#[test]
fn measured_navigator_targets_home_and_drag_work_beside_the_palette_at_hidpi() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        for style in crate::prefs::NavigatorStyle::ALL {
            for (scale, width, height) in [(1.0, 900.0, 720.0), (2.0, 700.0, 475.0), (2.0, 700.0, 385.0)] {
                let mut h = Harness::new();
                h.call("ui.set", json!({"layout":mode}));
                h.call("ui.prefs", json!({"navigator_style":style}));
                h.app.preferences.look_at_anim = false;
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":1})).unwrap();
                h.call("ui.prefs", json!({"ui_scale":scale}));
                h.call("ui.resize", json!({"width":width,"height":height}));
                h.app.fit_view();
                h.settle();
                let baseline = h.app.session.doc.to_json();
                let undo = h.app.session.undo.len();
                h.call("ui.view", json!({"view":"front"}));
                let layout = h.app.viewport.layout.as_ref().unwrap().clone();
                let palette = h.app.viewport.palette_rect.unwrap();
                if !layout.axis_view.is_positive() {
                    // Floating's smaller content at tiny heights cannot fit the compass;
                    // the navigation bar takes priority in both layouts.
                    assert!(layout.content.height() < crate::viewport::axis_view_size(style) + layout.navigation.height() + 11.0, "{layout:?}");
                    assert!(layout.navigation.is_positive());
                    assert!(crate::scenario::rect_of("axis_view:outer").is_none());
                    assert_eq!(h.app.session.doc.to_json(), baseline);
                    assert_eq!(h.app.session.undo.len(), undo);
                    continue;
                }
                let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
                let home = crate::scenario::rect_of("axis_view:home").unwrap();
                assert_eq!(outer, layout.axis_view);
                assert!(outer.contains_rect(home));
                assert!(layout.viewport.contains_rect(outer));
                assert_eq!(outer.right_top(), layout.content.right_top());
                if outer.intersect(palette).is_positive() {
                    // Short palettes may cover the fixed anchor; covered controls never steal input.
                    let covered = outer.intersect(palette).center();
                    let camera = h.app.cam;
                    h.call("ui.click", json!({"x":covered.x,"y":covered.y}));
                    assert_eq!(h.app.cam, camera);
                    assert!(h.app.viewport.axis_drag.is_none());
                    assert_eq!(crate::scenario::rect_of("axis_view:outer").unwrap(), outer);
                    assert_eq!(h.app.session.doc.to_json(), baseline);
                    assert_eq!(h.app.session.undo.len(), undo);
                    continue;
                }
                assert!(!layout.navigation.intersect(outer).is_positive());
                // Custom without a body draws the cube.
                if style != crate::prefs::NavigatorStyle::Axes {
                    let front = crate::scenario::handle_at("navigation_cube:front").unwrap();
                    assert!(outer.contains(front));
                    let up = crate::scenario::handle_at("navigation_cube:control:up").unwrap();
                    h.call("ui.click", json!({"x":up.x,"y":up.y}));
                    assert!(h.app.cam.back().dist(Vec3::Z) < 1e-5);
                    let top = crate::scenario::handle_at("navigation_cube:top").unwrap();
                    h.call("ui.click", json!({"x":top.x,"y":top.y}));
                    assert!(h.app.cam.back().dist(Vec3::Z) < 1e-5);
                } else {
                    let z = crate::scenario::handle_at("axis_view:z").unwrap();
                    assert!(outer.contains(z));
                    h.call("ui.click", json!({"x":z.x,"y":z.y}));
                    assert!(h.app.cam.back().dist(Vec3::Z) < 1e-5);
                    let z = crate::scenario::handle_at("axis_view:z").unwrap();
                    h.call("ui.click", json!({"x":z.x,"y":z.y}));
                    assert!(h.app.cam.back().dist(-Vec3::Z) < 1e-5);
                }
                h.call("ui.view", json!({"view":"front"}));
                h.app.viewport.nav = Some(crate::viewport::NavMode::Pan);
                let before = h.app.cam;
                let c = h.app.viewport.layout.as_ref().unwrap().axis_view.center();
                h.call("ui.drag", json!({"x0":c.x,"y0":c.y,"x1":c.x+24.0,"y1":c.y+8.0,"steps":8}));
                assert!((h.app.cam.yaw - before.yaw - 24.0 * 0.008).abs() < 1e-6);
                assert!((h.app.cam.pitch - before.pitch - 8.0 * 0.008).abs() < 1e-6);
                assert_eq!(h.app.cam.target, before.target);
                assert_eq!(h.app.cam.distance, before.distance);
                assert!(h.app.viewport.axis_drag.is_none());
                assert!(h.app.viewport.boxsel.is_none());
                let home = crate::scenario::handle_at("axis_view:home").unwrap();
                h.call("ui.click", json!({"x":home.x,"y":home.y}));
                let mut iso = h.app.cam;
                iso.set_view(StandardView::Iso);
                assert_eq!((h.app.cam.yaw, h.app.cam.pitch), (iso.yaw, iso.pitch));
                assert_eq!(h.app.session.doc.to_json(), baseline);
                assert_eq!(h.app.session.undo.len(), undo);
                assert!(h.app.session.selection.is_empty());
            }
        }
    }
}

#[test]
fn immediate_standard_view_cancels_fit_before_the_measured_frame() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        let baseline = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.app.cam.set_view(StandardView::Iso);
        h.app.fit_view();
        assert!(h.app.pending_fit.is_some(), "Fit awaits the measured overlay frame");
        let mut front = h.app.cam;
        front.set_view(StandardView::Front);
        h.call("ui.view", json!({"view":"front"}));
        h.settle();
        assert!(h.app.pending_fit.is_none());
        assert_eq!(h.app.cam, front, "the later frame must keep the explicitly requested view");
        assert_eq!(h.app.session.doc.to_json(), baseline);
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

#[test]
fn navigation_anchors_ignore_positioned_resized_and_covering_dialogs() {
    for (width, height, scale) in [(1400.0, 900.0, 1.0), (700.0, 450.0, 2.0), (700.0, 360.0, 2.0)] {
        let vp = Rect::from_min_max(pos2(250.0, crate::toolbar::APP_BAR_HEIGHT + crate::toolbar::RIBBON_HEIGHT), pos2(width, height - 30.0));
        let baseline = layout(vp, &[], scale, crate::prefs::NavigatorStyle::Axes, true);
        assert!(baseline.axis_view.is_positive() && baseline.navigation.is_positive());
        assert_eq!(baseline.axis_view.right_top(), vp.right_top());
        assert_eq!(baseline.navigation.center().x, vp.center().x);
        assert_eq!(baseline.navigation.bottom(), vp.bottom() - 7.0);
        let cases = [
            vec![Rect::from_min_max(pos2(width - 220.0, vp.top() + 120.0), vp.max)],
            vec![Rect::from_min_size(vp.min, vec2(210.0, 170.0))],
            vec![Rect::from_center_size(vp.center(), vec2(250.0, 180.0)), Rect::from_min_size(vp.min, vec2(70.0, 120.0))],
            vec![vp],
        ];
        for docks in cases {
            let measured = layout(vp, &docks, scale, crate::prefs::NavigatorStyle::Axes, true);
            assert_eq!(measured.axis_view, baseline.axis_view, "{docks:?}");
            assert_eq!(measured.navigation, baseline.navigation, "{docks:?}");
            assert!(vp.contains_rect(measured.axis_view));
            assert!(vp.contains_rect(measured.navigation));
        }
    }
}

#[test]
fn opening_different_command_dialogs_keeps_published_navigation_positions() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        let axis = crate::scenario::rect_of("axis_view:outer").unwrap();
        let nav = crate::scenario::rect_of("nav:outer").unwrap();
        let camera = h.app.cam;
        let baseline = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        for command in ["solid.extrude", "solid.fillet", "inspect.measure"] {
            h.app.start(command);
            h.settle();
            assert!(h.app.dialog.is_some(), "{command}");
            assert!(h.app.viewport.dialog_rect.is_some(), "dialog must be measured");
            assert_eq!(crate::scenario::rect_of("axis_view:outer").unwrap(), axis, "{command}");
            assert_eq!(crate::scenario::rect_of("nav:outer").unwrap(), nav, "{command}");
            assert_eq!(h.app.cam, camera);
            let pan = crate::scenario::handle_at("nav:slot:pan").unwrap();
            assert!(!h.app.viewport.dialog_rect.unwrap().contains(pan));
            h.app.viewport.nav = None;
            h.call("ui.click", json!({"x":pan.x,"y":pan.y}));
            assert_eq!(h.app.viewport.nav, Some(crate::viewport::NavMode::Pan), "fixed navigation stays usable beside {command}");
            h.app.dialog = None;
            h.settle();
            assert_eq!(crate::scenario::rect_of("axis_view:outer").unwrap(), axis);
            assert_eq!(crate::scenario::rect_of("nav:outer").unwrap(), nav);
        }
        assert_eq!(h.app.session.doc.to_json(), baseline);
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

/// OK and Cancel in the view's corner (#6709): the pair holds the content's top right corner and
/// the axis view or cube moves down below it, clear of the pair and the navigation bar; without
/// the pair (or without room for it) nothing moves.
#[test]
fn the_corner_pair_takes_the_top_right_and_the_view_navigator_moves_below_it() {
    let corner = vec2(92.0, 48.0);
    for style in [crate::prefs::NavigatorStyle::Axes, crate::prefs::NavigatorStyle::Cube] {
        for (width, height) in [(1400.0, 900.0), (700.0, 450.0), (310.0, 240.0), (160.0, 160.0), (90.0, 50.0)] {
            let vp = Rect::from_min_size(pos2(20.0, 30.0), vec2(width, height));
            let content = Rect::from_min_max(pos2(vp.left() + width * 0.25, vp.top()), vp.max);
            let bar = bar_rect(content);
            let plain = Layout::with_content(vp, content, &[], 1.0, style, egui::Vec2::ZERO, bar);
            let held = Layout::with_content(vp, content, &[], 1.0, style, corner, bar);
            assert!(!plain.confirm.is_positive(), "{plain:?}");
            assert_eq!(held.navigation, plain.navigation, "the navigation bar never moves");
            assert!(!held.axis_view.intersect(held.navigation).is_positive(), "{held:?}");
            if !held.confirm.is_positive() {
                assert_eq!(held.axis_view, plain.axis_view, "no room for the pair: {held:?}");
                continue;
            }
            assert_eq!(held.confirm.size(), corner);
            assert!(content.contains_rect(held.confirm), "{held:?}");
            assert_eq!((held.confirm.right(), held.confirm.top()), (content.right() - 8.0, content.top() + 8.0));
            assert!(!held.drawable.intersect(held.confirm).is_positive(), "fit keeps the model clear of it");
            if held.axis_view.is_positive() {
                assert!(held.axis_view.top() >= held.confirm.bottom(), "{style:?} {width}x{height}: {held:?}");
                assert_eq!(held.axis_view.right(), content.right());
                assert!(content.contains_rect(held.axis_view));
            }
        }
    }
    let roomy = Layout::with_content(
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0)),
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0)),
        &[],
        1.0,
        crate::prefs::NavigatorStyle::Axes,
        corner,
        bar_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0))),
    );
    assert!(roomy.confirm.is_positive() && roomy.axis_view.is_positive(), "{roomy:?}");
}

/// The pair in the running app: shown only with the preference and while a dialog is open, the
/// axis view below it, and the pointer over it reaching neither picking nor navigation.
#[test]
fn the_corner_pair_follows_the_dialog_and_keeps_the_pointer_from_the_model() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        let axis = crate::scenario::rect_of("axis_view:outer").unwrap();
        h.app.start("solid.fillet");
        h.settle();
        assert!(crate::scenario::rect_of("view:confirm").is_none(), "off by default");
        assert_eq!(crate::scenario::rect_of("axis_view:outer"), Some(axis));
        h.call("ui.prefs", json!({"view_confirm_corner": true}));
        h.settle();
        let pair = crate::scenario::rect_of("view:confirm").expect("the pair while a dialog is open");
        let moved = crate::scenario::rect_of("axis_view:outer").unwrap();
        assert!(moved.top() >= pair.bottom() && moved.right() == axis.right(), "{mode:?}: {moved:?} below {pair:?}");
        assert!(crate::shell::covered(&h.app, pair.center()), "one of the shell's covered rects");
        let dialog = h.app.viewport.dialog_rect.unwrap();
        assert!(!dialog.intersects(pair), "{mode:?}: {dialog:?} / {pair:?}");
        let (ok, cancel) = (crate::scenario::rect_of("view:confirm:ok").unwrap(), crate::scenario::rect_of("view:confirm:cancel").unwrap());
        assert!(pair.contains_rect(ok) && pair.contains_rect(cancel) && ok.right() < cancel.left(), "the check, then the cross in the corner");
        assert!(ok.width() >= 40.0 && ok.height() >= 40.0, "large enough to hit easily");
        h.step(&json!({"move":[pair.center().x, pair.center().y]})).unwrap();
        assert_eq!(h.app.viewport.mouse, None, "{mode:?}: the model under the pair never sees the pointer");
        h.app.dialog = None;
        h.settle();
        assert!(crate::scenario::rect_of("view:confirm").is_none(), "gone with the dialog");
        assert_eq!(crate::scenario::rect_of("axis_view:outer"), Some(axis), "{mode:?}: the axis view returns");
    }
}

#[test]
fn tiny_viewports_never_overlap_navigation_and_axis_view() {
    for width in [39.0, 40.0, 90.0, 120.0, 160.0, 310.0] {
        for height in [30.0, 80.0, 130.0, 160.0, 240.0] {
            let vp = Rect::from_min_size(pos2(20.0, 30.0), vec2(width, height));
            let clear = layout(vp, &[], 1.0, crate::prefs::NavigatorStyle::Axes, true);
            let covered = layout(vp, &[vp], 1.0, crate::prefs::NavigatorStyle::Axes, true);
            assert_eq!(clear.axis_view, covered.axis_view);
            assert_eq!(clear.navigation, covered.navigation);
            assert!(!clear.axis_view.intersect(clear.navigation).is_positive());
            for control in [clear.axis_view, clear.navigation] {
                if control.is_positive() {
                    assert!(vp.contains_rect(control), "{clear:?}");
                }
            }
        }
    }
}

#[test]
fn floating_fit_uses_the_measured_browser_and_history_cards_at_each_size() {
    for (width, height) in [(1400, 900), (900, 720), (700, 475)] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":width,"height":height}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        h.app.fit_view();
        h.settle();
        let layout = h.app.viewport.layout.as_ref().unwrap();
        for name in ["browser:card", "history:card"] {
            let card = crate::scenario::rect_of(name).unwrap();
            assert!(layout.viewport.contains_rect(card), "{name}: {card:?} {layout:?}");
            assert!(!layout.drawable.intersect(card).is_positive(), "{name}: {card:?} {layout:?}");
        }
        let projection = crate::viewport::projection(&h.app, layout.viewport);
        for x in [0.0, 40.0] {
            for y in [0.0, 30.0] {
                for z in [0.0, 20.0] {
                    assert!(layout.drawable.contains(projection.to_screen(Vec3::new(x, y, z)).unwrap()), "{layout:?}");
                }
            }
        }
    }
}

fn click_nav_mode(h: &mut Harness, mode: NavMode) {
    let slot = match mode {
        NavMode::Orbit => "orbit",
        NavMode::Pan => "pan",
        NavMode::Zoom => "zoom",
    };
    let p = crate::scenario::handle_at(&format!("nav:slot:{slot}")).unwrap();
    h.call("ui.click", json!({"x":p.x,"y":p.y}));
}

#[test]
fn escape_ends_nav_bar_modes_and_left_drag_stops_moving_the_camera() {
    for mode in [NavMode::Pan, NavMode::Orbit, NavMode::Zoom] {
        let mut h = Harness::new();
        h.settle();
        click_nav_mode(&mut h, mode);
        assert_eq!(h.app.viewport.nav, Some(mode));
        click_nav_mode(&mut h, mode);
        assert_eq!(h.app.viewport.nav, None, "clicking the active button still toggles it off");
        click_nav_mode(&mut h, mode);
        let p = h.app.viewport.layout.as_ref().unwrap().drawable.center();
        let drag = json!({"x0":p.x,"y0":p.y,"x1":p.x+40.0,"y1":p.y+25.0,"steps":8});
        let before = h.app.cam;
        h.call("ui.drag", drag.clone());
        assert_ne!(h.app.cam, before, "{mode:?}: the active mode moves the camera");
        h.step(&json!({"key":"Escape"})).unwrap();
        assert_eq!(h.app.viewport.nav, None, "{mode:?}");
        let stopped = h.app.cam;
        h.call("ui.drag", drag);
        assert_eq!(h.app.cam, stopped, "{mode:?}: Escape restores normal left-drag behavior");
    }
}

#[test]
fn escape_ends_navigation_before_cancelling_a_dialog() {
    let mut h = Harness::new();
    h.app.start("inspect.measure");
    h.settle();
    assert!(h.app.dialog.is_some());
    click_nav_mode(&mut h, NavMode::Pan);
    assert_eq!(h.app.viewport.nav, Some(NavMode::Pan));
    h.step(&json!({"key":"Escape"})).unwrap();
    assert_eq!(h.app.viewport.nav, None);
    assert!(h.app.dialog.is_some(), "ending navigation keeps the dialog open");
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_none(), "Escape without navigation still cancels the dialog");
}

#[test]
fn escape_ends_navigation_before_sketch_tools_and_does_not_count_as_a_finish_press() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[10,0]]})).unwrap();
    h.app.start("sketch.line");
    h.settle();
    let sketch = h.app.session.active_sketch;
    click_nav_mode(&mut h, NavMode::Pan);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert_eq!(h.app.viewport.nav, None);
    assert!(h.app.tool.is_some(), "ending navigation leaves the sketch tool active");
    assert!(h.app.last_escape.is_none());
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.tool.is_none(), "Escape without navigation still cancels the tool");
    assert_eq!(h.app.session.active_sketch, sketch);
    // Cancel navigation with no tool: the next Escape must not finish the sketch.
    click_nav_mode(&mut h, NavMode::Pan);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert_eq!(h.app.session.active_sketch, sketch);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert_eq!(h.app.session.active_sketch, sketch);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.session.active_sketch.is_none(), "two ordinary quick Escapes still finish the sketch");
}

/// Home and Fit put the model in the middle of the view. The floating controls take room out of
/// it, so the model is drawn smaller and kept clear of them, but the room they leave over moves it
/// towards the middle of the view rather than the middle of the gap between them; a panel moves
/// the middle itself (#9195).
#[test]
fn a_fit_centres_the_model_in_the_view_and_is_moved_only_by_panels() {
    let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 1000.0));
    let bounds = Aabb3 { min: Vec3::new(-50.0, -50.0, -50.0), max: Vec3::new(50.0, 50.0, 50.0) };
    let centre = bounds.center();
    let on_screen = |cam: Camera, p: Vec3| {
        let radius = (bounds.diagonal() * 0.5 + centre.dist(cam.target)).max(cam.half_height() * 2.0).max(10.0);
        let (x, y, _) = cam.to_screen(p, f64::from(viewport.width()), f64::from(viewport.height()), radius).unwrap();
        pos2(viewport.left() + x as f32, viewport.top() + y as f32)
    };
    for style in [crate::prefs::NavigatorStyle::Axes, crate::prefs::NavigatorStyle::Cube] {
        // The compass and the navigation bar, and nothing else.
        let bare = layout(viewport, &[], 1.0, style, true);
        let cam = fit_bounds(Camera::default(), &bounds, &bare, 0.0).expect("framed");
        for x in [bounds.min.x, bounds.max.x] {
            for y in [bounds.min.y, bounds.max.y] {
                for z in [bounds.min.z, bounds.max.z] {
                    let p = on_screen(cam, Vec3::new(x, y, z));
                    assert!(bare.drawable.contains(p), "{style:?}: the model is clear of the controls: {p:?} {bare:?}");
                }
            }
        }
        let middle = bare.middle.center();
        let gap = bare.drawable.center().distance(middle);
        let off = on_screen(cam, centre).distance(middle);
        assert!(off + 1.0 < gap, "{style:?}: nearer the middle of the view than the middle of the gap: {off} vs {gap}");
        // A panel down the left takes that part of the view away, so the middle moves right: the
        // camera's target moves the other way, against its right vector.
        let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(320.0, 1000.0));
        let docked = layout(viewport, &[panel], 1.0, style, true);
        let moved = fit_bounds(Camera::default(), &bounds, &docked, 0.0).expect("framed");
        let right = moved.basis().0;
        assert!((moved.target - centre).dot(right) < -1.0, "{style:?}: a panel must move the model clear of it: {:?}", moved.target);
        // It is drawn smaller with the controls shown than with the whole view to itself.
        let empty = Layout::with_content(viewport, viewport, &[], 1.0, style, egui::Vec2::ZERO, Rect::NOTHING);
        let roomy = fit_bounds(Camera::default(), &bounds, &empty, 0.0).expect("framed");
        assert!(roomy.distance <= cam.distance + 1e-9, "{style:?}: controls must not make the model larger");
    }
}

/// #9943: dragging the orbit gizmo toward the window edge must not stall once the cursor hits
/// that edge: the drag warps the cursor back to the window centre and keeps orbiting while the
/// button is held.
#[test]
fn gizmo_orbit_keeps_tracking_after_the_cursor_hits_the_window_edge() {
    let mut h = Harness::new();
    h.call("ui.view", json!({"view": "front"}));
    h.settle();
    let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
    let c = outer.center();
    let home = h.ctx.viewport_rect().center();
    let edge = h.ctx.viewport_rect().max.x - 2.0;
    assert!(c.x < edge - 20.0 && home.x < edge - 20.0, "the gizmo and the centre sit clear of the edge");
    let press = |pressed: bool, at: egui::Pos2| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.app.synthetic.push(egui::Event::PointerMoved(c));
    h.app.synthetic.push(press(true, c));
    // Drag right until the cursor reaches the window's right edge.
    for k in 1..=3 {
        h.app.synthetic.push(egui::Event::PointerMoved(pos2((c.x + 40.0 * k as f32).min(edge), c.y)));
    }
    h.frames(8);
    assert!(h.app.viewport.axis_drag.is_some(), "the drag is still held");
    assert_eq!(h.app.viewport.axis_warp, Some(home), "the cursor was warped back to the window centre");
    // The OS echoes the warp: the pointer reappears at the centre without moving the view.
    let before = h.app.cam;
    h.app.synthetic.push(egui::Event::PointerMoved(home));
    h.frames(4);
    assert_eq!(h.app.cam, before, "the warp echo must not jump the view");
    assert!(h.app.viewport.axis_warp.is_none(), "the echo clears the pending warp");
    // The user keeps dragging right from the centre: the orbit keeps tracking past the edge.
    let yaw = h.app.cam.yaw;
    for k in 1..=5 {
        h.app.synthetic.push(egui::Event::PointerMoved(pos2(home.x + 30.0 * k as f32, home.y)));
    }
    h.frames(8);
    assert!((h.app.cam.yaw - yaw - 150.0 * 0.008).abs() < 1e-3, "the orbit kept tracking past the edge: {:?}", h.app.cam);
    // Releasing ends the drag cleanly.
    h.app.synthetic.push(press(false, pos2(home.x + 150.0, home.y)));
    h.frames(4);
    assert!(h.app.viewport.axis_drag.is_none());
    assert!(h.app.viewport.axis_warp.is_none());
}
