//! Docked and floating command dialogs keep their size, their actions and their dragged place
//! (`frame::drag_dock`, `frame::offset_anchor`) in the running app.

#[test]
fn feature_forms_use_available_height_and_keep_actions_visible() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        use crate::scenario::Harness;
        use serde_json::json;
        for (width, height, scale) in [(1400.0, 900.0, 1.0), (1400.0, 950.0, 2.0)] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.app.preferences.ui_scale = scale;
            h.call("ui.resize", json!({"width":width / scale,"height":height / scale}));
            h.app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            h.step(&json!({"start":"solid.extrude"})).unwrap();
            h.frames(10);
            let dialog = h.app.viewport.dialog_rect.unwrap();
            let dialog_content = crate::scenario::count_of("dialog:body:content_height").unwrap();
            let viewport = h.app.viewport.rect.unwrap();
            assert!(dialog.height() > 230.0, "scale={scale}: {dialog:?}");
            assert!(viewport.expand(1.0).contains_rect(dialog), "scale={scale}: {dialog:?} outside {viewport:?}");
            assert!(h.widget("Distance").is_some(), "distance must be visible without scrolling");
            if scale == 1.0 {
                assert!(h.widget("Operation").is_some(), "operation must be visible without scrolling");
            }
            assert!(dialog.contains(crate::scenario::handle_at("dialog:OK").unwrap()));
            assert!(h.widget("Cancel").is_some(), "cancel must be visible without scrolling");
            h.step(&json!({"widget":"Cancel"})).unwrap();
            assert!(h.app.dialog.is_none());
            h.step(&json!({"start":"solid.shell"})).unwrap();
            h.frames(8);
            let short = h.app.viewport.dialog_rect.unwrap();
            let short_content = crate::scenario::count_of("dialog:body:content_height").unwrap();
            assert!(short_content < dialog_content, "short forms have less content even when both need scrolling: {short_content}, {dialog_content}");
            assert!(short.height() <= dialog.height(), "short forms cannot grow taller: {short:?}, {dialog:?}");
            let t = solidcraft_theme::Tokens::get();
            let chrome = f64::from(t.form_header_height + t.form_footer_height + t.form_gap_y * 2.0 + t.space_2);
            if short_content + chrome < f64::from(dialog.height()) {
                assert!(short.height() < dialog.height(), "unconstrained short forms must shrink: {short:?}, {dialog:?}");
            }
            assert!(viewport.expand(1.0).contains_rect(short));
            assert!(h.widget("Cancel").is_some());
            h.step(&json!({"widget":"Cancel"})).unwrap();
            h.step(&json!({"start":"solid.hole"})).unwrap();
            if let Some(crate::dialogs::Dialog { kind: crate::dialogs::Kind::Hole { kind, opts, .. }, .. }) = h.app.dialog.as_mut() {
                *kind = 1;
                opts.tapped = true;
            }
            h.frames(8);
            let long = h.app.viewport.dialog_rect.unwrap();
            assert!(viewport.expand(1.0).contains_rect(long), "long form clipped: {long:?}");
            assert!(h.widget("Cancel").is_some(), "long-form actions must stay visible");
            assert!(long.contains(crate::scenario::handle_at("dialog:OK").unwrap()));
            h.step(&json!({"widget":"Cancel"})).unwrap();
            assert!(h.app.dialog.is_none());
        }
    }
}

/// #5086: while a docked dialog's title is dragged, the pointer can outrun the dialog; the
/// model under it must not highlight until the drag ends.
#[test]
fn dragging_a_docked_dialog_blocks_viewport_hover() {
    use crate::scenario::Harness;
    use serde_json::json;

    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout": mode}));
        h.call("ui.resize", json!({"width": 2400, "height": 900}));
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.call("ui.view", json!({"view": "fit"}));
        h.app.ui.pick_bodies = true;
        let p = h.app.viewport.rect.unwrap().center();
        h.call("ui.move", json!({"x": p.x, "y": p.y}));
        assert!(h.app.viewport.hover.is_some(), "the uncovered model must highlight");
        h.step(&json!({"start": "parameters.change"})).unwrap();
        h.frames(6);
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        assert!(!h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_params_dialog"))).unwrap().contains(p));
        h.call("ui.move", json!({"x": title.x, "y": title.y}));
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: title,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.frame();
        h.app.synthetic.push(egui::Event::PointerMoved(p));
        h.frame();
        assert!(h.ctx.dragged_id().is_some(), "the title must own the drag");
        assert!(!h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_params_dialog"))).unwrap().contains(p), "the pointer must outrun the dialog");
        assert!(h.app.viewport.hover.is_none(), "dragging a dialog must block model highlights");
        assert!(h.app.viewport.mouse.is_none(), "the viewport must not receive the drag pointer");
        h.frames(2);
        assert!(h.app.viewport.hover.is_none(), "a stationary drag must still block highlights");
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.frame();
        h.app.params = None;
        h.frames(2);
        assert!(h.app.viewport.hover.is_some(), "hover must return after the dialog closes");
    }
}

/// #1335: a dialog keeps its size while its Select chip is hovered, and its title band
/// drags it; reopening keeps its position.
#[test]
fn a_docked_dialog_keeps_its_size_under_the_pointer_and_moves_by_its_title() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        use crate::scenario::Harness;
        use serde_json::json;
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.call("ui.resize", json!({"width":1400.0,"height":900.0}));
        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.frames(6);
        let rest = h.app.viewport.dialog_rect.unwrap();
        let chip = crate::scenario::handle_at("dialog:Plane").unwrap();
        h.call("ui.move", json!({"x": chip.x, "y": chip.y}));
        h.frames(6);
        let hovered = h.app.viewport.dialog_rect.unwrap();
        assert!((hovered.size() - rest.size()).length() < 0.01, "hovering resized the dialog: {rest:?} -> {hovered:?}");
        // Drag the published title band, independent of similarly labelled toolbar controls.
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        h.call("ui.drag", json!({"x0": title.x, "y0": title.y, "x1": title.x - 300.0, "y1": title.y + 120.0, "steps": 8}));
        h.frames(4);
        let moved = h.app.viewport.dialog_rect.unwrap();
        assert!((moved.min - rest.min - egui::vec2(-300.0, 120.0)).length() < 2.0, "{rest:?} -> {moved:?}");
        assert!(h.app.dialog.is_some(), "dragging the title keeps the dialog open");
        // Dragged far off, it stays on screen.
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        h.call("ui.drag", json!({"x0": title.x, "y0": title.y, "x1": title.x + 3000.0, "y1": title.y + 3000.0, "steps": 8}));
        h.frames(4);
        let viewport = h.app.viewport.rect.unwrap();
        assert!(viewport.contains(h.app.viewport.dialog_rect.unwrap().min));
        let last = h.app.viewport.dialog_rect.unwrap();
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        h.step(&json!({"start":"sketch.create"})).unwrap();
        h.frames(6);
        assert!((h.app.viewport.dialog_rect.unwrap().min - last.min).length() < 0.01, "reopening keeps the dragged position");
    }
}
#[test]
fn command_dialogs_keep_the_dragged_position_after_closing() {
    use crate::scenario::Harness;
    use serde_json::json;
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut h = Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.call("ui.resize", json!({"width":1400.0,"height":900.0}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"distance":10})).unwrap();
        h.step(&json!({"start":"solid.extrude"})).unwrap();
        h.frames(6);
        let rest = h.app.viewport.dialog_rect.unwrap();
        let title = crate::scenario::handle_at("dialog:title").unwrap();
        h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x-300.0,"y1":title.y+60.0,"steps":8}));
        h.frames(4);
        let moved = h.app.viewport.dialog_rect.unwrap();
        assert!((moved.min - rest.min - egui::vec2(-300.0, 60.0)).length() < 2.0);
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(4);
        assert!(h.app.dialog.is_none());
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(6);
        let fillet = h.app.viewport.dialog_rect.unwrap();
        assert!((fillet.right_top() - moved.right_top()).length() < 2.0, "Extrude: {moved:?}, Fillet: {fillet:?}");
        // A smaller window keeps the kept place on screen.
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(4);
        h.call("ui.resize", json!({"width":700.0,"height":500.0}));
        h.step(&json!({"start":"solid.fillet"})).unwrap();
        h.frames(6);
        let viewport = h.app.viewport.rect.unwrap();
        assert!(viewport.contains(h.app.viewport.dialog_rect.unwrap().min), "{viewport:?}");
    }
}
