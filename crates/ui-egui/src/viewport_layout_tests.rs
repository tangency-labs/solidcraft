use super::*;
use crate::scenario::Harness;
use crate::viewport::NavMode;
use serde_json::json;
use solidcraft_engine::render::StandardView;

#[test]
fn thin_rotated_bounds_fit_using_the_original_full_projection() {
    for (width, height, scale) in [(1400.0, 900.0, 1.0), (700.0, 450.0, 2.0), (700.0, 360.0, 2.0), (900.0, 720.0, 1.0)] {
        let vp = Rect::from_min_max(pos2(250.0, crate::toolbar::APP_BAR_HEIGHT + crate::toolbar::RIBBON_HEIGHT), pos2(width, height - 30.0));
        let dock = Rect::from_min_max(pos2(width - 205.0, vp.top()), vp.max);
        let layout = Layout::new(vp, &[dock], scale, crate::prefs::NavigatorStyle::Axes);
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
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
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
    let layout = Layout::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)), &[], 1.0, crate::prefs::NavigatorStyle::Axes);
    assert!(fit_bounds(cam, &Aabb3::EMPTY, &layout, 0.0).is_none());
    let bad = Aabb3 { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::new(1.0, 1.0, 1.0) };
    assert!(fit_bounds(cam, &bad, &layout, 0.6).is_none());
    let mut app = crate::SolveApp::new(Default::default(), Default::default());
    let tiny = Layout::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(5.0, 5.0)), &[], 1.0, crate::prefs::NavigatorStyle::Axes);
    app.viewport.layout = Some(tiny);
    let bounds = Aabb3 { min: Vec3::ZERO, max: Vec3::new(20.0, 1.0, 1.0) };
    assert_eq!(app.fitted_bounds(cam, &bounds), cam);
}

#[test]
fn fit_home_animation_and_manual_takeover_use_one_layout_solution() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
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
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
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
                if style == crate::prefs::NavigatorStyle::Cube {
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
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
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
        let baseline = Layout::new(vp, &[], scale, crate::prefs::NavigatorStyle::Axes);
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
            let measured = Layout::new(vp, &docks, scale, crate::prefs::NavigatorStyle::Axes);
            assert_eq!(measured.axis_view, baseline.axis_view, "{docks:?}");
            assert_eq!(measured.navigation, baseline.navigation, "{docks:?}");
            assert!(vp.contains_rect(measured.axis_view));
            assert!(vp.contains_rect(measured.navigation));
        }
    }
}

#[test]
fn opening_different_command_dialogs_keeps_published_navigation_positions() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
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
            let pan = pos2(nav.left() + 5.0 + 2.0 * 30.0 + 13.0, nav.top() + 15.0);
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

#[test]
fn tiny_viewports_never_overlap_navigation_and_axis_view() {
    for width in [39.0, 40.0, 90.0, 120.0, 160.0, 310.0] {
        for height in [30.0, 80.0, 130.0, 160.0, 240.0] {
            let vp = Rect::from_min_size(pos2(20.0, 30.0), vec2(width, height));
            let clear = Layout::new(vp, &[], 1.0, crate::prefs::NavigatorStyle::Axes);
            let covered = Layout::new(vp, &[vp], 1.0, crate::prefs::NavigatorStyle::Axes);
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
    let nav = crate::scenario::rect_of("nav:outer").unwrap();
    let column = match mode {
        NavMode::Orbit => 0,
        NavMode::Pan => 2,
        NavMode::Zoom => 3,
    };
    let p = pos2(nav.left() + 5.0 + column as f32 * 30.0 + 13.0, nav.top() + 15.0);
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
