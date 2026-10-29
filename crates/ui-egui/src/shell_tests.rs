use super::*;
use crate::scenario::{Harness, rect_of};
use serde_json::json;

#[test]
fn modes_are_transient_and_available_to_control_and_scenarios() {
    let mut h = Harness::new();
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    let prefs = serde_json::to_value(&h.app.preferences).unwrap();
    let doc = h.app.session.doc.to_json();
    h.step(&json!({"call":"ui.set", "params":{"layout":"docked"}})).unwrap();
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    h.step(&json!({"call":"ui.set", "params":{"layout":"invalid"},"fail":true})).unwrap();
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    h.call("ui.set", json!({"layout":"floating"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    assert_eq!(h.app.session.doc.to_json(), doc);
    assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), prefs);
}

#[test]
fn both_modes_keep_their_geometry_and_fit_clear_of_covered_rects() {
    for mode in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        let viewport = h.app.viewport.rect.unwrap();
        let workspace = rect_of("ribbon:workspace").unwrap();
        let toolbar = rect_of("toolbar").unwrap();
        if mode == LayoutMode::Floating {
            let ribbon = rect_of("ribbon:card").unwrap();
            assert_eq!(viewport.left(), 0.0);
            assert_eq!(viewport.top(), toolbar::APP_BAR_HEIGHT);
            assert_eq!(workspace.left(), SHELL_GAP);
            assert_eq!(workspace.top(), viewport.top() + SHELL_GAP);
            assert_eq!(ribbon.left() - workspace.right(), SHELL_GAP);
            assert_eq!(ribbon.right(), viewport.right() - SHELL_GAP);
            assert!(!covered(&h.app, pos2(4.0, workspace.center().y)));
            assert!(!covered(&h.app, pos2(workspace.right() + 2.0, workspace.center().y)));
            assert!(covered(&h.app, workspace.center()));
            assert!(covered(&h.app, ribbon.center()));
        } else {
            assert_eq!(toolbar.top(), toolbar::APP_BAR_HEIGHT);
            assert_eq!(viewport.top(), toolbar.bottom());
            assert_eq!(toolbar.height(), toolbar::RIBBON_HEIGHT);
            assert_eq!(workspace.left(), 0.0);
            assert_eq!(covered_rects(&h.app), vec![h.app.shell.history.unwrap(), rect_of("browser:card").unwrap()]);
        }
        h.app.fit_view();
        h.settle();
        let layout = h.app.viewport.layout.as_ref().unwrap();
        for covered in covered_rects(&h.app) {
            assert!(!layout.drawable.intersect(covered).is_positive(), "{layout:?} {covered:?}");
        }
        let projection = crate::viewport::projection(&h.app, viewport);
        for point in [solidcraft_engine::geom::Vec3::ZERO, solidcraft_engine::geom::Vec3::new(40.0, 30.0, 20.0)] {
            assert!(layout.drawable.contains(projection.to_screen(point).unwrap()));
        }
        h.app.start("solid.extrude");
        h.settle();
        if mode == LayoutMode::Floating {
            assert!(h.app.viewport.dialog_rect.unwrap().top() >= h.app.viewport.layout.as_ref().unwrap().axis_view.bottom());
        }
    }
}

#[test]
fn cards_block_model_hover_wheel_and_drag_but_the_gap_remains_viewport() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let ribbon = rect_of("ribbon:card").unwrap();
    // Blank space at the right, away from the search and command buttons.
    let at = pos2(ribbon.right() - 12.0, ribbon.bottom() - 28.0);
    let camera = h.app.cam;
    let doc = h.app.session.doc.to_json();
    h.call("ui.move", json!({"x":at.x,"y":at.y}));
    assert!(h.app.viewport.hover.is_none());
    assert!(h.app.viewport.mouse.is_none());
    h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
    h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x-30.0,"y1":at.y+15.0,"button":"middle","steps":6}));
    assert_eq!(h.app.cam, camera);
    assert!(h.app.viewport.boxsel.is_none());
    assert_eq!(h.app.session.doc.to_json(), doc);
    let gap = pos2(4.0, ribbon.center().y);
    h.call("ui.move", json!({"x":gap.x,"y":gap.y}));
    assert_eq!(h.app.viewport.mouse, Some(gap));
}

#[test]
fn floating_sketch_palette_and_compass_share_measured_clearance() {
    for style in crate::prefs::NavigatorStyle::ALL {
        for (width, height, scale) in [(1400.0, 900.0, 1.0), (700.0, 720.0, 2.0)] {
            let mut h = Harness::new();
            h.app.preferences.navigator_style = style;
            h.call("ui.resize", json!({"width":width,"height":height}));
            h.call("ui.prefs", json!({"ui_scale":scale}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
            h.settle();
            h.app.fit_view();
            h.settle();
            let layout = h.app.viewport.layout.as_ref().unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!(palette.top() >= layout.axis_view.bottom(), "{style:?} {palette:?} {layout:?}");
            assert!(layout.viewport.expand(1.0).contains_rect(palette), "{palette:?} {layout:?}");
            assert!(palette.contains(crate::scenario::handle_at("sketch_palette:look_at").unwrap()));
            let proj = crate::viewport::projection(&h.app, layout.viewport);
            for point in [solidcraft_engine::geom::Vec3::ZERO, solidcraft_engine::geom::Vec3::new(20.0, 0.0, 0.0)] {
                assert!(layout.drawable.contains(proj.to_screen(point).unwrap()), "{layout:?}");
            }
            if width > 1000.0 {
                let content = layout.content;
                h.app.start("construct.plane.offset");
                h.settle();
                let dialog = h.app.viewport.dialog_rect.unwrap();
                let expected_top = (content.top() + crate::viewport::AXIS_VIEW_CLEARANCE).min((content.bottom() - 360.0).max(content.top()));
                assert!((dialog.left() - content.left()).abs() < 1.0);
                assert!((dialog.top() - expected_top).abs() < 1.0, "left dialogs retain their existing top: {dialog:?}");
            }
        }
    }
}

#[test]
fn short_floating_palette_stays_in_viewport() {
    for style in crate::prefs::NavigatorStyle::ALL {
        for height in [475, 385, 300] {
            let mut h = Harness::new();
            h.app.preferences.navigator_style = style;
            h.call("ui.resize", json!({"width":700,"height":height}));
            h.call("ui.prefs", json!({"ui_scale":2.0}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            // Main's open/untied-end feedback also consumes footer space.
            for points in [[[0, 0], [10, 0]], [[10, 0], [10, 10]], [[10, 10], [0, 10]], [[0, 10], [0, 0]], [[20, 0], [30, 5]]] {
                h.app.run("sketch.line", json!({"points":points,"infer":false,"connect":false})).unwrap();
            }
            // Include the extra Feature Options section in the tiny-height case.
            if height == 300 {
                h.app.start("sketch.rectangle.two_point");
            }
            h.settle();
            let viewport = h.app.viewport.rect.unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!(
                viewport.contains_rect(palette),
                "{style:?}, height={height}: {palette:?} extends beyond {viewport:?}, right={:.9}, bottom={:.9}",
                palette.right(),
                palette.bottom()
            );
            let compass = h.app.viewport.layout.as_ref().unwrap().axis_view;
            if compass.is_positive() {
                assert!(palette.top() < compass.bottom(), "short panels reclaim compass space");
            }
            let action = crate::scenario::handle_at("sketch_palette:look_at").unwrap();
            if !palette.contains(action) {
                // Once reclaiming is insufficient, the whole palette scrolls to its footer.
                h.call("ui.scroll", json!({"x":palette.center().x,"y":palette.center().y,"delta":-1200}));
                h.frames(20);
            }
            let action = crate::scenario::handle_at("sketch_palette:look_at").unwrap();
            assert!(palette.contains(action), "footer must be reachable: {style:?}, height={height}, {action:?} in {palette:?}");
            let finish = crate::scenario::handle_at("sketch_palette:finish").unwrap();
            h.call("ui.click", json!({"x":finish.x,"y":finish.y}));
            assert!(h.app.session.active_sketch.is_none(), "Finish Sketch must work after scrolling");
        }
    }
}

#[test]
fn selection_drag_released_over_a_card_leaves_no_rubber_band() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.settle();
    let ribbon = rect_of("ribbon:card").unwrap();
    let viewport = h.app.viewport.rect.unwrap();
    h.call(
        "ui.drag",
        json!({"x0":viewport.right()-20.0,"y0":viewport.bottom()-100.0,
        "x1":ribbon.right()-12.0,"y1":ribbon.bottom()-28.0,"steps":12}),
    );
    assert!(h.app.viewport.boxsel.is_none());
    assert!(h.app.session.selection.is_empty());
}

#[test]
fn floating_browser_clears_cards_shields_input_and_can_dock_again() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for timeline_left in [false, true] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.app.preferences.timeline_left = timeline_left;
            h.app.preferences.browser_float = true;
            h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
            h.settle();
            let browser = rect_of("browser:float").unwrap();
            let toolbar = rect_of("toolbar").unwrap();
            assert!(browser.top() >= toolbar.bottom(), "{mode:?}: {browser:?} must clear {toolbar:?}");
            assert!(crate::scenario::handle_at("row:Untitled").is_some());
            assert!(crate::scenario::handle_at("left-browser-rail").is_some());
            assert!(covered(&h.app, browser.center()));
            let viewport = h.app.viewport.rect.unwrap();
            // A left-side history still takes a column in Docked mode.
            if mode == LayoutMode::Floating || !timeline_left {
                assert_eq!(viewport.left(), 0.0);
            } else {
                assert!(viewport.left() > 0.0);
            }
            if mode == LayoutMode::Floating {
                assert_eq!(viewport.top(), toolbar::APP_BAR_HEIGHT);
                assert!(!browser.intersect(rect_of("ribbon:card").unwrap()).is_positive());
            }
            h.app.fit_view();
            h.settle();
            assert!(!h.app.viewport.layout.as_ref().unwrap().drawable.intersect(browser).is_positive());
            let camera = h.app.cam;
            let doc = h.app.session.doc.to_json();
            let at = browser.center();
            h.call("ui.move", json!({"x":at.x,"y":at.y}));
            assert!(h.app.viewport.hover.is_none());
            assert!(h.app.viewport.mouse.is_none());
            h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
            h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x+20.0,"y1":at.y+10.0,"button":"middle","steps":6}));
            assert_eq!(h.app.cam, camera);
            assert_eq!(h.app.session.doc.to_json(), doc);
            // The browser remains interactive above the shielded viewport.
            h.step(&json!({"click":{"chevron":"Origin"}})).unwrap();
            assert!(crate::browser::drawn_rows().iter().any(|(name, _, _, open)| name == "Origin" && *open == Some(true)));
            h.app.preferences.browser_float = false;
            h.settle();
            assert!(h.app.shell.browser.is_some());
            assert!(rect_of("browser:float").is_none());
            assert!(rect_of("browser:card").is_some());
            // The saved width follows both the floating card and the docked column.
            let rail = crate::scenario::handle_at("left-browser-rail").unwrap();
            let width = h.app.preferences.browser_width;
            h.call("ui.drag", json!({"x0":rail.x,"y0":rail.y,"x1":rail.x+60.0,"y1":rail.y,"steps":6}));
            assert!(h.app.preferences.browser_width > width + 30.0);
        }
    }
}

#[test]
fn browser_and_history_cards_block_input_while_their_gaps_and_space_below_browser_do_not() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let browser = rect_of("browser:card").unwrap();
    let history = rect_of("history:card").unwrap();
    let viewport = h.app.viewport.rect.unwrap();
    assert_eq!(history.left(), viewport.left() + SHELL_GAP);
    assert_eq!(history.right(), viewport.right() - SHELL_GAP);
    assert_eq!(history.bottom(), viewport.bottom() - SHELL_GAP);
    let layout = h.app.viewport.layout.as_ref().unwrap();
    assert!(layout.navigation.bottom() < history.top());
    assert!(layout.axis_view.top() >= rect_of("ribbon:card").unwrap().bottom());
    for r in [browser, history] {
        assert!(covered(&h.app, r.center()));
        assert!(!layout.drawable.intersect(r).is_positive());
    }
    for at in [pos2(browser.left() + 2.0, browser.top() + 2.0), pos2(history.right() - 30.0, history.center().y)] {
        let camera = h.app.cam;
        h.call("ui.move", json!({"x":at.x,"y":at.y}));
        assert!(h.app.viewport.mouse.is_none());
        assert!(h.app.viewport.hover.is_none());
        h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
        h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x+10.0,"y1":at.y+2.0,"button":"middle","steps":6}));
        assert_eq!(h.app.cam, camera);
    }
    for at in [pos2(3.0, browser.center().y), pos2(browser.center().x, browser.bottom() + SHELL_GAP), pos2(3.0, history.center().y)] {
        assert!(!covered(&h.app, at));
        h.call("ui.move", json!({"x":at.x,"y":at.y}));
        assert_eq!(h.app.viewport.mouse, Some(at));
    }
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.settle();
    assert!(h.app.shell.browser.is_none() && h.app.shell.history.is_none());
    assert!(!covered(&h.app, browser.center()));
    assert!(!covered(&h.app, history.center()));
}

#[test]
fn left_history_and_browser_share_the_left_edge_without_moving_right_controls() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        h.app.preferences.timeline_left = true;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"First"})).unwrap();
        h.settle();
        assert!(rect_of("history:card").is_none());
        let history = rect_of("history:vertical").unwrap();
        let browser = rect_of("browser:card").unwrap();
        assert!((browser.left() - history.right() - Container::for_layout(mode, Surface::Panel).gap()).abs() < 1.0);
        assert!(crate::scenario::handle_at("timeline:First").is_some());
        let viewport = h.app.viewport.rect.unwrap();
        let content = content_rect(&h.app, viewport);
        assert_eq!(viewport.left(), if mode == LayoutMode::Floating { 0.0 } else { crate::ui_kit::history_width(true) });
        assert_eq!(content.left(), browser.right() + Container::for_layout(mode, Surface::Panel).gap());
        assert_eq!(content.bottom(), viewport.bottom());
        if mode == LayoutMode::Floating {
            assert!(covered(&h.app, history.center()));
            assert!(covered(&h.app, browser.center()));
            let gap_x = (history.right() + browser.left()) / 2.0;
            assert!(covered(&h.app, pos2(gap_x, history.center().y)), "the seam pill shields viewport input");
            assert!(!covered(&h.app, pos2(gap_x, history.top() + 4.0)), "the rest of the shell gap remains available");
        }
    }
}

#[test]
fn assistant_floats_without_changing_projection_and_shields_the_model() {
    for dark in [false, true] {
        for side in ["left", "right"] {
            let mut h = crate::scenario::Harness::new();
            h.app.home.open = false;
            h.call("ui.resize", serde_json::json!({"width":1400,"height":1000}));
            h.call("ui.set", serde_json::json!({"dark":dark,"layout":"floating"}));
            h.frames(4);
            let projection = h.app.viewport.rect.expect("viewport");
            let original_controls = h.app.viewport.layout.clone().expect("layout");
            h.call("ui.agent_chat", serde_json::json!({"open":true,"picker":false,"side":side,"width":440}));
            h.frames(6);
            let card = crate::scenario::rect_of("agent_chat:panel").expect("floating Assistant");
            assert_eq!(h.app.viewport.rect, Some(projection), "opening the card preserves projection");
            assert!(card.left() > projection.left() && card.right() < projection.right());
            assert!(card.bottom() < projection.bottom());
            let content = h.app.shell.content.expect("content");
            assert!((card.top() - content.top()).abs() < 1.0, "full-height top: {card:?} {content:?}");
            assert!((card.bottom() - content.bottom()).abs() < 1.0, "full-height bottom: {card:?} {content:?}");
            assert!(covered(&h.app, card.center()), "chat shields viewport input");
            if side == "right" {
                let layout = h.app.viewport.layout.as_ref().expect("layout");
                assert!(!card.intersect(layout.axis_view).is_positive(), "card clears compass");
                assert!(!card.intersect(layout.navigation).is_positive(), "card clears navigation");
                assert!(layout.axis_view.right() < original_controls.axis_view.right(), "compass moves left");
                assert!(layout.navigation.center().x < original_controls.navigation.center().x, "navigation moves left");
            } else {
                let browser = h.app.shell.browser.expect("browser card");
                assert!(card.right() < browser.left(), "left Assistant precedes browser");
                assert!((card.top() - browser.top()).abs() < 1.0, "Assistant and browser share the top margin");
            }
            let camera = h.app.cam;
            let at = card.center();
            h.call("ui.move", json!({"x":at.x,"y":at.y}));
            h.call("ui.scroll", json!({"x":at.x,"y":at.y,"delta":120}));
            h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x-30.0,"y1":at.y+15.0,"button":"middle","steps":6}));
            assert_eq!(h.app.cam, camera, "chat consumes wheel and orbit input");
            assert!(h.app.viewport.boxsel.is_none());
            h.app.start("solid.extrude");
            h.frames(5);
            let feature = h.app.viewport.dialog_rect.expect("feature panel");
            let beside = crate::scenario::rect_of("agent_chat:panel").expect("Assistant beside feature");
            assert!(!beside.intersect(feature).is_positive(), "feature and Assistant coexist: {beside:?} {feature:?}");
            assert!(
                (beside.min - card.min).length() < 1.0 && (beside.max - card.max).length() < 1.0,
                "opening a tool must not move Assistant: {card:?} -> {beside:?}"
            );
            if side == "right" {
                assert!(feature.right() + SHELL_GAP <= beside.left() + 1.0, "tool stays inside Assistant's rail");
            }
            h.app.dialog = None;
            h.app.tool = None;
            h.frames(3);
            h.call("ui.agent_chat", serde_json::json!({"minimized":true}));
            h.frames(4);
            let folded = crate::scenario::rect_of("agent_chat:panel").expect("floating folded card");
            assert!(folded.width() < 60.0 && folded.height() < 120.0);
            assert!(covered(&h.app, folded.center()));
            assert_eq!(h.app.viewport.rect, Some(projection));
            let folded_controls = h.app.viewport.layout.as_ref().expect("layout");
            assert_eq!(folded_controls.axis_view, original_controls.axis_view);
            if side == "right" {
                assert_eq!(folded_controls.navigation, original_controls.navigation);
            } else {
                let shift = (folded.width() + SHELL_GAP) / 2.0;
                assert!((folded_controls.navigation.center().x - original_controls.navigation.center().x - shift).abs() < 1.0);
            }
            h.call("ui.agent_chat", serde_json::json!({"open":false}));
            h.frames(3);
            assert!(!covered(&h.app, card.center()), "closed chat leaves no stale obstruction");
        }
    }
}

#[test]
fn assistant_left_dock_precedes_history_and_drag_hints_match_the_result() {
    for layout in ["floating", "docked"] {
        for (names, stacked) in [(false, false), (true, false), (true, true)] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_under_browser = stacked;
            h.app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
            h.call("ui.resize", json!({"width":1600,"height":1000}));
            h.call("ui.set", json!({"layout":layout}));
            h.call("ui.agent_chat", json!({"open":true,"side":"right","width":440,"picker":false}));
            h.frames(6);
            let projection = h.app.viewport.rect;
            for left in [true, false] {
                let grip = crate::scenario::handle_at("agent_chat:grip").expect("drag grip");
                let target = pos2(if left { 100.0 } else { 1500.0 }, grip.y);
                h.app.synthetic.push(egui::Event::PointerMoved(grip));
                h.frame();
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: grip,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frame();
                h.app.synthetic.push(egui::Event::PointerMoved(target));
                h.frames(2);
                let hint = rect_of("agent_chat:dock_hint").expect("visible dock target");
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frames(6);
                let card = h.app.agent_chat.panel_rect.expect("docked Assistant");
                assert_eq!(h.app.preferences.agent_chat_side, if left { "left" } else { "right" });
                assert!((hint.min - card.min).length() < 1.0 && (hint.max - card.max).length() < 1.0, "{layout} preview {hint:?} matches {card:?}");
                if layout == "floating" {
                    assert_eq!(h.app.viewport.rect, projection, "docking keeps model projection");
                }
                if left {
                    let history = h.app.shell.history.expect("vertical history");
                    let browser = h.app.shell.browser.expect("browser");
                    assert!(card.right() <= history.left(), "Assistant precedes history: {card:?} {history:?}");
                    if stacked {
                        assert!((history.left() - browser.left()).abs() < 1.0);
                        assert!((history.width() - browser.width()).abs() < 1.0);
                        assert!(browser.bottom() <= history.top(), "History stays below Browser");
                    } else {
                        assert!(history.right() <= browser.left(), "history precedes browser");
                    }
                }
            }
            h.call("ui.agent_chat", json!({"side":"left","minimized":true}));
            h.frames(6);
            let strip = h.app.agent_chat.drop_rect.expect("minimized Assistant");
            assert!(strip.right() <= h.app.shell.history.expect("history").left());
            h.call("ui.agent_chat", json!({"open":false}));
            h.frames(6);
            assert!(h.app.shell.history.expect("history returns to outer edge").left() < strip.right());
        }
    }
}

#[test]
fn assistant_right_edge_survives_palette_tool_resize_and_minimize() {
    for layout in ["floating", "docked"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1800,"height":1000}));
        h.call("ui.set", json!({"layout":layout}));
        h.call("ui.agent_chat", json!({"open":true,"side":"right","width":380,"picker":false}));
        h.frames(5);
        let right = h.app.agent_chat.panel_rect.unwrap().right();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(6);
        for width in [380.0, 480.0] {
            h.call("ui.agent_chat", json!({"width":width}));
            h.frames(6);
            let chat = h.app.agent_chat.panel_rect.unwrap();
            let palette = h.app.viewport.palette_rect.unwrap();
            assert!((chat.right() - right).abs() < 1.0, "{layout}: Assistant stays outside the palette");
            assert!(
                palette.right() + Container::for_layout(h.app.ui.layout, Surface::Panel).gap() <= chat.left() + 1.0,
                "{layout}: {palette:?}, {chat:?}"
            );
        }
        h.call("ui.agent_chat", json!({"minimized":true}));
        h.frames(5);
        let strip = h.app.agent_chat.drop_rect.unwrap();
        let palette = h.app.viewport.palette_rect.unwrap();
        assert!((strip.right() - right).abs() < 1.0);
        assert!(palette.right() <= strip.left(), "palette clears the minimized Assistant too");
        h.call("ui.agent_chat", json!({"open":false}));
        h.frames(5);
        assert!(h.app.viewport.palette_rect.unwrap().right() > palette.right(), "closing Assistant reclaims its rail");
    }
}

#[test]
fn tool_sheets_drag_over_browser_and_history_but_clear_protected_panels() {
    for side in ["left", "right"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1800,"height":1000}));
        h.call("ui.set", json!({"layout":"floating"}));
        h.call("ui.agent_chat", json!({"open":true,"side":side,"width":380,"picker":false}));
        h.frames(5);
        h.app.start("sketch.create");
        h.frames(5);
        let chat = h.app.agent_chat.panel_rect.unwrap();
        let browser = h.app.shell.browser.unwrap();
        let history = h.app.shell.history.unwrap();
        let ribbon = h.app.shell.cards[1];
        let workspace = h.app.shell.cards[0];
        for target in [browser.center(), history.center(), ribbon.center(), workspace.center(), egui::pos2(-1000.0, 400.0), egui::pos2(3000.0, 400.0)]
        {
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":target.x,"y1":target.y,"steps":10}));
            h.frames(4);
            let sheet = h.app.viewport.dialog_rect.unwrap();
            assert!(!sheet.intersect(chat).is_positive(), "{side}: {sheet:?} overlaps Assistant {chat:?}");
            assert!(!sheet.intersect(ribbon).is_positive(), "sheet clears ribbon");
            assert!(!sheet.intersect(workspace).is_positive(), "sheet clears workspace selector");
            if target == browser.center() {
                assert!(sheet.intersect(browser).is_positive(), "sheet can cover Browser");
                let layer = h.ctx.layer_id_at(sheet.intersect(browser).center()).unwrap();
                assert_eq!(layer.id, egui::Id::new(("sc_dialog", "CREATE SKETCH")), "sheet receives input above Browser");
            } else if target == history.center() {
                assert!(sheet.intersect(history).is_positive(), "sheet can cover History");
            }
        }
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.dialog.is_none(), "dragged sheet remains interactive");
    }
}

#[test]
fn tool_sheets_stay_above_browser_and_history_after_panel_clicks() {
    for dock in ["bottom", "left", "under_browser"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1800,"height":1000}));
        h.call("ui.set", json!({"layout":"floating"}));
        h.app.run("view.history_layout", json!({"dock":dock})).unwrap();
        h.app.start("solid.extrude");
        h.settle();
        for panel in [h.app.shell.browser.unwrap(), h.app.shell.history.unwrap()] {
            let title = crate::scenario::handle_at("dialog:title").unwrap();
            let target = panel.center();
            h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":target.x,"y1":target.y,"steps":10}));
            let sheet = h.app.viewport.dialog_rect.unwrap();
            let overlap = sheet.intersect(panel);
            assert!(overlap.is_positive(), "{dock}: the dialog overlaps the panel");
            let exposed = panel.left_top() + egui::vec2(10.0, 10.0);
            assert!(!sheet.contains(exposed), "the panel click is outside the dialog");
            h.call("ui.click", json!({"x":exposed.x,"y":exposed.y}));
            let layer = h.ctx.layer_id_at(overlap.center()).unwrap();
            assert_eq!(layer.id, egui::Id::new(("sc_dialog", "EXTRUDE")), "{dock}: clicking a panel must not cover the dialog");
            let panel_layer = h.ctx.layer_id_at(exposed).unwrap();
            h.call("ui.click", json!({"x":1700,"y":500}));
            assert_eq!(h.ctx.layer_id_at(exposed), Some(panel_layer), "clicking the model view must not cover the panel");
        }
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.dialog.is_none(), "{dock}: the overlapping dialog remains clickable");
    }
}

#[test]
fn tool_sheet_dropdowns_and_preferences_still_open_above_the_sheet() {
    for layout in ["floating", "compact"] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":layout}));
        h.app.start("solid.extrude");
        h.settle();
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":700,"y1":350,"steps":10}));
        h.step(&json!({"widget":"One Side"})).unwrap();
        h.step(&json!({"widget":"Symmetric"})).unwrap();
        assert!(matches!(h.app.dialog.as_ref().unwrap().kind, crate::dialogs::Kind::Extrude { direction: 2, .. }));
        let sheet = h.app.viewport.dialog_rect.unwrap();
        h.call("ui.prefs", json!({"open":"General"}));
        let preferences = rect_of("dialog:window").unwrap();
        let overlap = sheet.intersect(preferences);
        assert!(overlap.is_positive(), "Preferences overlaps the feature dialog");
        assert_eq!(h.ctx.layer_id_at(overlap.center()).unwrap().id, egui::Id::new("sc_prefs"), "Preferences receives input above the sheet");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(!h.app.prefs_window.open && h.app.dialog.is_some(), "Cancel closes Preferences, leaving the feature dialog open");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert!(h.app.dialog.is_none());
    }
}

#[test]
fn compact_layout_is_saved_and_survives_cancel_apply_defaults_and_bad_input() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.start("solid.extrude");
    h.settle();
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let camera = h.app.cam;
    h.call("ui.prefs", json!({"open":"Interface"}));
    crate::prefs::reveal_preference(&mut h, "dialog:Workspace:Compact");
    h.step(&json!({"click":{"handle":"dialog:Workspace:Compact"}})).unwrap();
    h.call("ui.prefs", json!({"button":"ok"}));
    h.settle();
    assert!(h.app.dialog.is_some(), "changing layout keeps the active tool");
    assert_eq!(h.app.preferences.layout, LayoutMode::Docked);
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    assert_eq!(h.app.cam, camera);
    h.call("engine.execute", json!({"command":"view.layout", "params":{"mode":"compact"}}));
    assert_eq!(h.app.cam, camera, "programmatic layout changes never auto-fit");
    assert_eq!(h.app.session.doc.to_json(), design);
    assert_eq!(h.app.session.undo.len(), undo);
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(restored.ui.layout, LayoutMode::Docked);
    assert_eq!(restored.preferences.layout, LayoutMode::Docked);
    h.call("ui.prefs", json!({"open":"Interface","layout":"floating"}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    h.call("ui.prefs", json!({"open":"Interface","layout":"floating"}));
    h.call("ui.prefs", json!({"button":"apply"}));
    h.call("ui.prefs", json!({"layout":"compact"}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    h.call("ui.prefs", json!({"open":"Interface","layout":"compact"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    assert_eq!(h.app.preferences.layout, LayoutMode::Floating);
    h.call("ui.prefs", json!({"button":"ok"}));
    assert!(crate::prefs::set(&mut h.app, &json!({"layout":"invalid"})).is_err());
    assert_eq!(h.app.ui.layout, LayoutMode::Floating);
    restored.load_prefs(r#"{"preferences":{"layout":"invalid","default_units":"in"}}"#);
    assert_eq!(restored.ui.layout, LayoutMode::Floating);
    assert_eq!(restored.preferences.default_units, "in");
    assert_eq!(crate::prefs::lenient(&json!({})).layout, LayoutMode::Floating);
}

#[test]
fn compact_chrome_is_flush_and_browser_hugs_content_in_both_themes() {
    for dark in [false, true] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"layout":"compact"}));
        h.call("ui.set", json!({"dark":dark}));
        h.settle();
        let bar = rect_of("toolbar").unwrap();
        let browser = rect_of("browser:card").unwrap();
        let workspace = rect_of("ribbon:workspace").unwrap();
        assert_eq!(workspace.left(), bar.left());
        assert_eq!(workspace.top(), bar.top());
        assert_eq!(h.app.shell.cards.get(1).unwrap().left(), workspace.right(), "one joined top bar");
        assert_eq!(browser.left(), 0.0);
        assert_eq!(browser.top(), bar.bottom(), "square, flush Browser/ribbon join");
        assert!(browser.height() < h.app.viewport.rect.unwrap().height() / 2.0, "Browser hugs its contents");
        let compact = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Panel);
        assert_eq!(compact.gap(), 0.0);
        let frame = compact.frame(&h.ctx, crate::theme::Tokens::get().panel, crate::frame::Side::Left);
        assert_eq!(frame.shadow, egui::Shadow::NONE);
        assert_eq!(frame.corner_radius, egui::CornerRadius::ZERO);
        h.app.start("solid.extrude");
        h.settle();
        let dialog = h.app.viewport.dialog_rect.unwrap();
        assert!((dialog.right() - h.app.viewport.rect.unwrap().right()).abs() < 1.0, "{dialog:?}");
        let before = dialog;
        h.step(&json!({"drag":[{"handle":"dialog:title"},[700,300]],"steps":6})).unwrap();
        h.settle();
        let moved = h.app.viewport.dialog_rect.unwrap();
        assert!(moved.right() < before.right() - 100.0, "compact tool sheets detach from the right edge");
        let feature = crate::ui_kit::Container::new(&h.ctx, crate::ui_kit::Surface::Feature);
        assert_eq!(feature.frame(&h.ctx, crate::theme::Tokens::get().dialog_body, crate::frame::Side::Right).shadow, egui::Shadow::NONE);
        h.app.run("view.layout", json!({"mode":"floating"})).unwrap();
        h.settle();
        assert_eq!(rect_of("browser:card").unwrap().left(), SHELL_GAP);
        assert!(h.app.dialog.is_some());
    }
}

#[test]
fn two_collapsed_compact_headers_leave_live_viewport_below_their_column() {
    for dark in [false, true] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"layout":"compact"}));
        h.call("ui.set", json!({"dark":dark}));
        h.app.run("view.history_layout", json!({"dock":"under_browser"})).unwrap();
        h.app.preferences.timeline_left_browser_collapsed = true;
        h.app.preferences.timeline_collapsed = true;
        h.frames(12);
        let browser = h.app.shell.browser.unwrap();
        let history = h.app.shell.history.unwrap();
        let below = egui::pos2(browser.center().x, history.bottom() + 40.0);
        assert!(h.app.viewport.rect.unwrap().contains(below));
        assert!(!covered(&h.app, below));
        assert!(covered(&h.app, browser.center()));
        assert!(covered(&h.app, history.center()));
        h.call("ui.move", json!({"x":below.x,"y":below.y}));
        assert_eq!(h.app.viewport.mouse, Some(below), "the vacated column remains live viewport");
    }
}

#[test]
fn ribbon_labels_are_saved_independently_and_icon_only_commands_remain_reachable() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"layout":mode}));
        h.settle();
        let labelled = rect_of("toolbar").unwrap();
        h.call("ui.prefs", json!({"ribbon_labels":false}));
        h.settle();
        let icons = rect_of("toolbar").unwrap();
        assert!(icons.height() < labelled.height());
        assert_eq!(h.app.ui.layout, mode);
        let command = rect_of("button:sketch.create").unwrap();
        assert!(icons.contains_rect(command));
        h.step(&json!({"click":{"handle":"button:sketch.create"}})).unwrap();
        assert!(h.app.dialog.is_some(), "the icon still starts Create Sketch");
        let mut restored = crate::SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert!(!restored.preferences.ribbon_labels);
        assert_eq!(restored.ui.layout, mode);
        h.call("ui.prefs", json!({"open":"Interface","ribbon_labels":true}));
        h.call("ui.prefs", json!({"button":"cancel"}));
        assert!(!h.app.preferences.ribbon_labels);
    }
    assert!(crate::prefs::lenient(&json!({})).ribbon_labels);
}

#[test]
fn embed_keeps_model_panels_without_application_chrome() {
    let mut h = Harness::new();
    h.app.embedded = true;
    h.app.home.show(h.app.session.revision);
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    assert!(!h.app.home.open);
    assert_eq!(h.app.ui.layout, LayoutMode::Docked);
    assert!(h.app.ui.show_browser && h.app.ui.show_timeline);
    assert!(rect_of("toolbar").is_none());
    assert!(rect_of("ribbon:workspace").is_none());
    assert!(rect_of("browser:card").is_some());
    assert!(h.app.viewport.rect.is_some());
}

/// A design with a finished sketch nothing uses yet, so Extrude opens ready for OK.
fn sketched(h: &mut Harness) {
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
}

/// Dialogs › In the left column (#6707): Extrude takes the place of the browser and the History
/// at the left, at the column's width and never narrower than its card, in both layouts and
/// every History dock. The view keeps its size, the dialog is covered, the content area starts
/// beside it, and OK, Cancel or Esc bring the column back exactly as it was.
#[test]
fn a_dialog_in_the_left_column_takes_the_place_of_browser_and_history() {
    let card = crate::theme::Tokens::get().form_card_width;
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for (dock, close) in [("bottom", "Cancel"), ("left", "Escape"), ("under_browser", "OK")] {
            let what = format!("{mode:?}, History {dock}, {close}");
            let mut h = Harness::new();
            h.call("ui.resize", json!({"width":1400,"height":900}));
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.prefs", json!({"dialog_place":"left_column"}));
            h.app.run("view.history_layout", json!({"dock":dock})).unwrap();
            sketched(&mut h);
            h.settle();
            let browser = h.app.shell.browser.expect("browser");
            let history = h.app.shell.history.expect("history");
            let viewport = h.app.viewport.rect.unwrap();
            let prefs = serde_json::to_value(&h.app.preferences).unwrap();
            let column = if dock == "bottom" { browser } else { browser.union(history) };
            h.step(&json!({"start":"solid.extrude"})).unwrap();
            h.settle();
            let dialog = h.app.viewport.dialog_rect.expect("dialog");
            assert_eq!(rect_of("dialog:outer"), Some(dialog), "{what}");
            let place = rect_of("dialog:column").expect("the column is published");
            assert!((dialog.left() - column.left()).abs() <= 1.0 && (place.left() - column.left()).abs() <= 1.0, "{what}: {dialog:?} {column:?}");
            assert!((dialog.top() - column.top()).abs() <= 1.0, "{what}: {dialog:?} {column:?}");
            // The card's outline lies outside its response rect, one stroke at each side.
            assert!((dialog.width() - column.width().max(card)).abs() <= 2.5, "{what}: {dialog:?} {column:?}");
            assert!(dialog.bottom() <= place.bottom() + 1.0, "{what}: {dialog:?} {place:?}");
            assert!(rect_of("browser:card").is_none() && h.app.shell.browser.is_none(), "{what}: the browser is not drawn");
            if dock == "bottom" {
                assert_eq!(h.app.shell.history, Some(history), "{what}: the bottom strip stays");
                assert!(dialog.bottom() < history.top(), "{what}");
            } else {
                assert!(rect_of("history:vertical").is_none(), "{what}: the History is not drawn");
            }
            assert_eq!(h.app.viewport.rect, Some(viewport), "{what}: the view keeps its size");
            assert!(covered(&h.app, dialog.center()), "{what}");
            assert!(content_rect(&h.app, viewport).left() >= dialog.right(), "{what}");
            assert!(h.widget("Distance").is_some(), "{what}: the inputs are there");
            match close {
                "Escape" => h.step(&json!({"key":"Escape"})).unwrap(),
                other => h.step(&json!({"widget":other})).unwrap(),
            }
            h.settle();
            assert!(h.app.dialog.is_none(), "{what}");
            assert_eq!(h.app.session.doc.features.len(), if close == "OK" { 2 } else { 1 }, "{what}: OK adds the extrude");
            let back = h.app.shell.browser.expect("the browser is back");
            assert_eq!((back.x_range(), back.top()), (browser.x_range(), browser.top()), "{what}");
            assert_eq!(h.app.shell.history, Some(history), "{what}: the History is back as it was");
            assert_eq!(serde_json::to_value(&h.app.preferences).unwrap(), prefs, "{what}: no width or fold changed");
            assert!(rect_of("dialog:column").is_none(), "{what}");
        }
    }
}

/// With no column at the left (the browser hidden, or translucent over the view) a dialog in
/// the left column docks at the left edge at its own width, above the bottom History strip.
#[test]
fn without_a_left_column_a_dialog_docks_at_the_left_edge_at_its_own_width() {
    let card = crate::theme::Tokens::get().form_card_width;
    for (mode, browser) in [(LayoutMode::Floating, "hidden"), (LayoutMode::Docked, "hidden"), (LayoutMode::Floating, "translucent")] {
        let what = format!("{mode:?}, browser {browser}");
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":mode}));
        h.call("ui.prefs", json!({"dialog_place":"left_column","browser_float":browser == "translucent"}));
        h.app.ui.show_browser = browser != "hidden";
        sketched(&mut h);
        h.settle();
        let history = h.app.shell.history.expect("history");
        let viewport = h.app.viewport.rect.unwrap();
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        h.settle();
        let dialog = h.app.viewport.dialog_rect.expect("dialog");
        let gap = Container::for_layout(mode, Surface::Panel).gap();
        assert!((dialog.left() - (viewport.left() + gap)).abs() <= 1.0, "{what}: {dialog:?} {viewport:?}");
        assert!((dialog.width() - card).abs() <= 2.5, "{what}: {dialog:?}");
        assert!(dialog.bottom() < history.top(), "{what}");
        assert!(rect_of("browser:card").is_none(), "{what}");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        h.settle();
        assert_eq!(rect_of("browser:card").is_some(), browser == "translucent", "{what}");
    }
}

/// The Assistant docked on the left keeps the outside edge: the column starts beside it, where
/// the browser was.
#[test]
fn a_dialog_in_the_left_column_starts_beside_the_assistant() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1800,"height":1000}));
    h.call("ui.set", json!({"layout":"floating"}));
    h.call("ui.prefs", json!({"dialog_place":"left_column"}));
    h.call("ui.agent_chat", json!({"open":true,"side":"left","width":380,"picker":false}));
    h.frames(5);
    let browser = h.app.shell.browser.unwrap();
    h.app.start("sketch.create");
    h.frames(5);
    let chat = h.app.agent_chat.panel_rect.unwrap();
    let dialog = h.app.viewport.dialog_rect.unwrap();
    assert!((dialog.left() - browser.left()).abs() <= 1.0, "{dialog:?} {browser:?}");
    assert!(!dialog.intersect(chat).is_positive(), "{dialog:?} {chat:?}");
    let title = crate::scenario::handle_at("dialog:title").unwrap();
    h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x + 300.0,"y1":title.y + 200.0,"steps":10}));
    h.frames(4);
    assert_eq!(h.app.viewport.dialog_rect, Some(dialog), "a dialog in the column stays in its place");
}

/// The Sketch Palette keeps the right edge while a sketch dialog holds the left column, also on
/// a view too narrow for both side by side; editing a feature reopens its dialog in the column
/// and Cancel rolls the marker back.
#[test]
fn the_sketch_palette_stays_and_editing_a_feature_opens_in_the_left_column() {
    for width in [1400.0, 900.0] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":width,"height":900}));
        h.call("ui.prefs", json!({"dialog_place":"left_column"}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
        h.settle();
        let palette = rect_of("sketch_palette:outer").expect("palette");
        let browser = h.app.shell.browser.unwrap();
        h.step(&json!({"start":"inspect.measure"})).unwrap();
        h.settle();
        let dialog = h.app.viewport.dialog_rect.expect("measure");
        let kept = rect_of("sketch_palette:outer").expect("the palette stays on screen");
        assert!((kept.right() - palette.right()).abs() <= 1.0 && (kept.top() - palette.top()).abs() <= 1.0, "width {width}: {kept:?} {palette:?}");
        assert!((dialog.left() - browser.left()).abs() <= 1.0 && !dialog.intersect(palette).is_positive(), "width {width}: {dialog:?}");
        h.step(&json!({"widget":"Close"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":10})).unwrap();
        h.settle();
        let extrude = h.app.session.doc.features.last().unwrap().id;
        let marker = h.app.session.doc.marker;
        h.app.edit_feature(extrude);
        h.settle();
        assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()));
        assert_ne!(h.app.session.doc.marker, marker, "editing rolls the marker to the feature");
        let edit = h.app.viewport.dialog_rect.expect("edit dialog");
        assert!((edit.left() - browser.left()).abs() <= 1.0, "width {width}: {edit:?}");
        assert!(h.app.shell.browser.is_none());
        h.step(&json!({"widget":"Cancel"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, marker);
        assert!(h.app.shell.browser.is_some());
    }
}

/// OK and Cancel in the view's corner (#6709) with a dialog in the left column: the pair keeps
/// the view's top right, beside the column's content area and clear of the dialog; its check
/// applies the dialog and its cross cancels, and either way the column comes back.
#[test]
fn the_view_corner_answers_a_dialog_in_the_left_column() {
    for how in ["ok", "cancel"] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.prefs", json!({"dialog_place":"left_column","view_confirm_corner":true}));
        sketched(&mut h);
        h.settle();
        let browser = h.app.shell.browser.expect("browser");
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        h.settle();
        let dialog = h.app.viewport.dialog_rect.expect("dialog");
        let pair = rect_of("view:confirm").expect("the pair in the view's corner");
        let content = content_rect(&h.app, h.app.viewport.rect.unwrap());
        assert!((dialog.left() - browser.left()).abs() <= 1.0, "{how}: the dialog holds the column: {dialog:?}");
        assert!(content.contains_rect(pair) && pair.left() > content.center().x, "{how}: {pair:?} in the top right of {content:?}");
        assert!(!pair.intersects(dialog), "{how}: {pair:?} {dialog:?}");
        assert!(covered(&h.app, pair.center()), "{how}");
        assert_eq!(crate::scenario::count_of("view:confirm:ok:enabled"), Some(1.0), "{how}");
        h.step(&json!({"click":{"handle":format!("view:confirm:{how}")}})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none(), "{how}: the dialog closes");
        assert_eq!(h.app.session.doc.features.len(), if how == "ok" { 2 } else { 1 }, "{how}: the check makes the extrude");
        assert!(h.app.shell.browser.is_some_and(|b| b.x_range() == browser.x_range()), "{how}: the column comes back");
    }
}

/// Dialogs is an Interface preference: set from the page or the control channel, saved,
/// refused when unknown, and Cancel and Restore Defaults put it back. A dialog already open
/// moves with it.
#[test]
fn dialog_place_is_saved_and_survives_cancel_defaults_and_bad_input() {
    use crate::dialog_column::DialogPlace;
    let mut h = Harness::new();
    sketched(&mut h);
    h.step(&json!({"start":"solid.extrude"})).unwrap();
    h.settle();
    let right = h.app.viewport.dialog_rect.unwrap();
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::Edge);
    h.call("ui.prefs", json!({"open":"Interface"}));
    crate::prefs::reveal_preference(&mut h, "dialog:Dialogs:In the left column");
    h.step(&json!({"click":{"handle":"dialog:Dialogs:In the left column"}})).unwrap();
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::LeftColumn);
    h.call("ui.prefs", json!({"button":"ok"}));
    h.settle();
    let left = h.app.viewport.dialog_rect.unwrap();
    assert!(left.left() < right.left() && h.app.shell.browser.is_none(), "the open dialog moves to the column: {left:?}");
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert_eq!(restored.preferences.dialog_place, DialogPlace::LeftColumn);
    h.call("ui.prefs", json!({"open":"Interface","dialog_place":"edge"}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::LeftColumn);
    h.call("ui.prefs", json!({"open":"Interface"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::Edge);
    h.call("ui.prefs", json!({"button":"ok"}));
    assert!(crate::prefs::set(&mut h.app, &json!({"dialog_place":"top"})).is_err());
    assert_eq!(h.app.preferences.dialog_place, DialogPlace::Edge);
    restored.load_prefs(r#"{"preferences":{"dialog_place":"top","default_units":"in"}}"#);
    assert_eq!(restored.preferences.dialog_place, DialogPlace::Edge);
    assert_eq!(restored.preferences.default_units, "in");
}

/// Every text the last frame painted.
fn painted_texts(h: &Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

#[test]
fn status_bar_spans_the_window_below_history_in_both_layouts_and_is_covered() {
    for mode in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        h.settle();
        assert!(rect_of("status:bar").is_none() && h.app.shell.status_bar.is_none(), "off by default");
        let viewport_without = h.app.viewport.rect.unwrap();
        let covered_without = covered_rects(&h.app);
        h.call("ui.prefs", json!({"status_bar": true}));
        h.settle();
        let bar = rect_of("status:bar").unwrap();
        assert_eq!(h.app.shell.status_bar, Some(bar));
        assert_eq!((bar.left(), bar.right(), bar.bottom()), (0.0, 1400.0, 900.0), "{mode:?}: full width at the window's bottom: {bar:?}");
        assert_eq!(bar.height(), crate::status_bar::HEIGHT);
        let viewport = h.app.viewport.rect.unwrap();
        assert!(viewport.bottom() <= bar.top() + 0.5, "{mode:?}: the view ends above the bar: {viewport:?} {bar:?}");
        assert_eq!(viewport.bottom(), viewport_without.bottom() - bar.height(), "{mode:?}: the view gives up only the bar's height");
        let history = h.app.shell.history.unwrap();
        assert!(history.bottom() <= bar.top() + 0.5, "{mode:?}: History sits above the bar: {history:?} {bar:?}");
        let rects = covered_rects(&h.app);
        assert!(rects.contains(&bar), "{mode:?}: the bar is covered: {rects:?}");
        assert_eq!(rects.len(), covered_without.len() + 1, "{mode:?}: {rects:?} {covered_without:?}");
        assert!(covered(&h.app, bar.center()));
        h.call("ui.prefs", json!({"status_bar": false}));
        h.settle();
        assert!(rect_of("status:bar").is_none() && h.app.shell.status_bar.is_none());
        assert_eq!(h.app.viewport.rect.unwrap(), viewport_without);
    }
}

#[test]
fn status_bar_names_what_is_edited_and_the_sketch_state_and_replaces_the_views_corner_text() {
    use crate::status_bar::SketchState;
    let mut h = Harness::new();
    let corner = |h: &Harness| painted_texts(h).into_iter().filter(|t| t.starts_with("Sketch1 — ")).collect::<Vec<_>>();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.settle();
    assert_eq!(corner(&h), ["Sketch1 — fully constrained"], "without the bar the state stays in the view's corner");
    h.call("ui.prefs", json!({"status_bar": true}));
    h.settle();
    let contents = crate::status_bar::contents(&h.app);
    assert_eq!(contents.editing.as_deref(), Some("Editing Sketch1"));
    assert_eq!(contents.sketch, Some(SketchState::Empty), "a blank sketch is not called fully constrained");
    let rectangle = h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})).unwrap();
    h.settle();
    let contents = crate::status_bar::contents(&h.app);
    let Some(SketchState::Under(dof)) = contents.sketch else { panic!("{contents:?}") };
    assert!(dof > 0);
    let texts = painted_texts(&h);
    assert!(corner(&h).is_empty(), "with the bar on, the view's corner shows no state: {texts:?}");
    assert!(texts.contains(&format!("Under constrained ({dof} degrees of freedom)")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Editing Sketch1") && texts.iter().any(|t| t == "mm"), "{texts:?}");
    assert!(crate::scenario::handle_at("status:editing").unwrap().x < crate::scenario::handle_at("status:sketch").unwrap().x);
    assert!(crate::scenario::handle_at("status:sketch").unwrap().x < crate::scenario::handle_at("status:units").unwrap().x);
    // Fully constrained: the rectangle fixed in place.
    h.app.run("sketch.constraint.fix", json!({"entities": rectangle["curves"]})).unwrap();
    h.settle();
    assert_eq!(crate::status_bar::contents(&h.app).sketch, Some(SketchState::Full));
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance":10})).unwrap();
    h.settle();
    let idle = crate::status_bar::contents(&h.app);
    assert_eq!((idle.editing, idle.sketch, idle.units.as_str()), (None, None, "mm"), "idle: only the units");
    assert!(crate::scenario::handle_at("status:editing").is_none() && crate::scenario::handle_at("status:sketch").is_none());
    // A feature's dialog, editing it.
    let extrude = h.app.session.doc.features.iter().find(|f| f.name == "Extrude1").map(|f| f.id).unwrap();
    h.app.edit_feature(extrude);
    h.settle();
    assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_some()));
    assert_eq!(crate::status_bar::contents(&h.app).editing.as_deref(), Some("Editing Extrude1"));
    // In another language the text is translated, and so is a default feature name.
    let _ja = crate::i18n::scope("ja");
    assert_eq!(crate::status_bar::contents(&h.app).editing, Some(format!("編集中: {}", crate::i18n::tr_name("Extrude1"))));
    assert_eq!(crate::i18n::tr_name("Sketch1"), "スケッチ1");
}

#[test]
fn status_bar_units_open_the_units_menu_above_the_bar_and_change_units_in_one_undo_step() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.call("ui.prefs", json!({"status_bar": true}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.settle();
    let undo = h.app.session.undo.len();
    let button = rect_of("status:units").unwrap();
    assert!(rect_of("status:bar").unwrap().contains_rect(button));
    h.step(&json!({"click":{"handle":"status:units"}})).unwrap();
    h.settle();
    assert_eq!(h.app.menu.target, crate::context_menu::Target::Units);
    let at = h.app.viewport.context_menu.unwrap();
    assert_eq!(at, button.right_top());
    let menu = crate::context_menu::describe(&h.app);
    let labels: Vec<_> = menu["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_string()).collect();
    assert_eq!(labels, crate::prefs::UNITS);
    // The list opens above the bar, inside the window.
    let list = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_context_menu"))).unwrap();
    assert!(list.bottom() <= button.top() + 0.5 && list.top() >= 0.0, "{list:?} {button:?}");
    assert_eq!(h.app.session.undo.len(), undo, "opening the menu changes nothing");
    h.call("ui.menuPick", json!({"item":"in"}));
    h.settle();
    assert_eq!(h.app.session.doc.units, "in");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.viewport.context_menu.is_none());
    assert_eq!(crate::status_bar::contents(&h.app).units, "in");
    h.app.run("edit.undo", json!({})).unwrap();
    h.settle();
    assert_eq!(h.app.session.doc.units, "mm");
    // A second click on the button closes the menu it opened.
    h.step(&json!({"click":{"handle":"status:units"}})).unwrap();
    h.settle();
    assert!(h.app.viewport.context_menu.is_some());
    h.step(&json!({"click":{"handle":"status:units"}})).unwrap();
    h.settle();
    assert!(h.app.viewport.context_menu.is_none());
}

#[test]
fn status_bar_stays_out_of_the_embed() {
    let mut h = Harness::new();
    h.app.embedded = true;
    h.app.preferences.status_bar = true;
    h.settle();
    assert!(!crate::status_bar::shown(&h.app));
    assert!(rect_of("status:bar").is_none() && h.app.shell.status_bar.is_none());
}
