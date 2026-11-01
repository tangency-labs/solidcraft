use super::*;
use crate::scenario::{Harness, handle_at, rect_of};
use crate::shell::{LayoutMode, SHELL_GAP};

#[test]
fn clicked_browser_folders_toggle_visibility_with_v() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        for (name, x) in [("One", 0), ("Two", 20)] {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
        }
        let group =
            h.app.run("browser.group", json!({"component":0,"folder":"bodies","items":["One"],"name":"Group"})).unwrap()["group"].as_u64().unwrap();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":5})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("construct.plane.offset", json!({"base":"XY","offset":20})).unwrap();
        h.app.run("browser.visibility", json!({"items":["Two"],"visible":false})).unwrap();
        h.settle();
        for (label, key) in [
            ("Bodies", "c0/bodies".into()),
            ("Sketches", "c0/sketches".into()),
            ("Construction", "c0/construction".into()),
            ("Group (1)", format!("c0/g{group}")),
        ] {
            h.app.run("select.set", json!({"items":[Sel::Body {name:"One".into()}]})).unwrap();
            let before = h.app.session.visibility.clone();
            let mut hidden = before.clone();
            hidden.hidden_folders.push(key.clone());
            h.step(&json!({"click":{"handle":format!("row:{label}")}})).unwrap();
            h.step(&json!({"key":"V"})).unwrap();
            assert_eq!(h.app.session.visibility, hidden, "{mode:?}: V hides {label}, leaving individual items alone");
            h.step(&json!({"key":"V"})).unwrap();
            assert_eq!(h.app.session.visibility, before, "V shows the same folder again");
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.visibility, hidden);
            h.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(h.app.session.visibility, before);
            // Restore expansion so the later group row is still reachable.
            h.step(&json!({"click":{"handle":format!("row:{label}")}})).unwrap();
        }
    }
}

/// #10526: New Group from the root's context menu in a design with no bodies yet makes a group
/// the Browser shows, and a body made later can be dropped into it.
#[test]
fn a_new_group_in_a_folder_with_nothing_in_it_yet_shows() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.settle();
    assert!(handle_at("row:Bodies").is_none(), "no bodies, no Bodies folder");
    // What the root's New Group item runs.
    group_action(&mut h.app, "ui.newGroup", &json!({"component": 0}));
    h.settle();
    let group = h.app.session.doc.browser_groups.first().map(|g| (g.id, g.name.clone())).expect("New Group makes a group");
    assert!(handle_at(&format!("row:{} (0)", group.1)).is_some(), "the new group shows in the Browser");
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"operation":"new_body","body_name":"One"})).unwrap();
    h.settle();
    h.step(&json!({"drag":[{"handle":"row:One"},{"handle":format!("row:{} (0)", group.1)}]})).unwrap();
    h.settle();
    let items = h.app.session.doc.browser_group(group.0).map(|g| g.items.len()).unwrap_or(0);
    assert_eq!(items, 1, "the body went into the group");
}

/// #10005: F2 on a group picked in the Browser opens its Rename box, as for other rows, and the
/// typed name renames the group, at the top level and inside a component; a body selected
/// alongside does not take the rename from the clicked group.
#[test]
fn f2_on_a_clicked_browser_group_renames_the_group() {
    for inside_component in [false, true] {
        let (mut h, component) = if inside_component {
            let (h, component, _) = crate::test_support::component_box();
            (h, component)
        } else {
            let mut h = Harness::new();
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Body1"})).unwrap();
            (h, 0)
        };
        let body = h.app.session.model.state().bodies[0].name.clone();
        let group = h.app.run("browser.group", json!({"component":component,"folder":"bodies","items":[body],"name":"Group"})).unwrap()["group"]
            .as_u64()
            .unwrap();
        h.app.preferences.browser_bodies_folders = crate::prefs::BodiesFolders::Always;
        h.settle();
        let world_body = h.app.session.world_state().bodies[0].name.clone();
        h.app.run("select.set", json!({"items":[Sel::Body { name: world_body.clone() }]})).unwrap();
        h.step(&json!({"click":{"handle":"row:Group (1)"}})).unwrap();
        h.step(&json!({"key":"F2"})).unwrap();
        h.frames(2);
        let what = h.app.menu.rename.as_ref().map(|r| (r.what.clone(), r.text.clone()));
        assert_eq!(
            what,
            Some((crate::context_menu::RenameWhat::Group(group), "Group".to_string())),
            "component {inside_component}: F2 renames the group"
        );
        h.step(&json!({"text":"Fasteners"})).unwrap();
        h.frames(1);
        h.step(&json!({"key":"Enter"})).unwrap();
        h.frames(2);
        assert!(h.app.menu.rename.is_none());
        let names: Vec<&str> = h.app.session.doc.browser_groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Fasteners"], "component {inside_component}: Enter renames the group; status {:?}", h.app.status);
        assert!(h.app.session.world_state().body(&world_body).is_some(), "the body keeps its name");
    }
}

#[test]
fn folder_visibility_target_clears_when_the_user_moves_on() {
    for action in ["body click", "selection command", "empty click", "escape", "design tab", "model edit", "tool"] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"One"})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("select.set", json!({"items":[Sel::Body {name:"One".into()}]})).unwrap();
        h.settle();
        h.step(&json!({"click":{"handle":"row:Sketches"}})).unwrap();
        match action {
            "body click" => h.step(&json!({"click":{"handle":"row:One"}})).unwrap(),
            "selection command" => {
                // Explicitly picking the same body must also leave the folder target.
                h.app.run("select.set", json!({"items":[Sel::Body {name:"One".into()}]})).unwrap();
            }
            "empty click" => h.step(&json!({"click":[950,650]})).unwrap(),
            "escape" => h.step(&json!({"key":"Escape"})).unwrap(),
            "design tab" => {
                crate::documents::new_tab(&mut h.app);
                h.settle();
                crate::documents::switch(&mut h.app, 0);
                h.settle();
            }
            "model edit" => {
                h.app.run("sketch.create", json!({"plane":"XZ"})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
            }
            "tool" => {
                h.app.start("file.save_mesh");
                h.settle();
            }
            _ => panic!("unknown test action: {action}"),
        }
        h.step(&json!({"key":"V"})).unwrap();
        assert!(h.app.session.visibility.hidden_folders.is_empty(), "{action}: V must no longer target Sketches");
        if matches!(action, "body click" | "selection command") {
            assert!(!h.app.session.body_visible("One"), "{action}: normal selection shortcuts still work");
        }
    }
}

#[test]
fn folder_visibility_shortcut_does_not_take_dialog_body_picks_or_chevrons() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"One"})).unwrap();
    h.settle();
    h.step(&json!({"click":{"chevron":"Bodies"}})).unwrap();
    h.step(&json!({"key":"V"})).unwrap();
    assert!(!h.app.session.folder_hidden("c0/bodies"), "the fold arrow does not pick a visibility target");
    h.app.start("file.save_mesh");
    h.settle();
    h.step(&json!({"click":{"handle":"row:Bodies"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![Sel::Body { name: "One".into() }]);
    h.step(&json!({"key":"V"})).unwrap();
    assert!(!h.app.session.folder_hidden("c0/bodies"), "a body input consumes the folder click");
}

#[test]
fn save_mesh_add_all_and_add_visible_bodies_fill_the_bodies_input() {
    let mut h = Harness::new();
    for (name, x) in [("One", 0), ("Two", 20), ("Three", 40)] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
    }
    h.app.run("browser.visibility", json!({"items":["Two"],"visible":false})).unwrap();
    h.app.start("file.save_mesh");
    h.settle();
    for (button, expected) in [("Add all bodies", vec!["One", "Two", "Three"]), ("Add visible bodies", vec!["One", "Three"])] {
        let handle = crate::scenario::handles("dialog:button:").into_iter().find(|(n, _)| n.ends_with(&format!(":{button}")));
        let (handle, _) = handle.unwrap_or_else(|| panic!("{button} is shown"));
        h.step(&json!({"click":{"handle":handle}})).unwrap();
        let d = h.app.dialog.as_ref().unwrap();
        let crate::dialogs::Kind::SaveMesh(k) = &d.kind else { panic!("Save as Mesh is open") };
        let commands = crate::dialogs_save_mesh::commands(k, &d.inputs).unwrap();
        assert_eq!(commands[0].1["bodies"], json!(expected), "{button}");
        assert!(h.app.session.selection.is_empty(), "the buttons do not change the global selection");
    }
}

#[test]
fn browser_body_clicks_fill_and_toggle_save_mesh_inputs() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        for (name, x) in [("One", 0), ("Two", 20)] {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
        }
        h.app.start("file.save_mesh");
        h.settle();
        for (clicked, expected) in [("One", vec!["One"]), ("Two", vec!["One", "Two"]), ("One", vec!["Two"])] {
            h.step(&json!({"click":{"handle":format!("row:{clicked}")}})).unwrap();
            let d = h.app.dialog.as_ref().unwrap();
            let crate::dialogs::Kind::SaveMesh(k) = &d.kind else { panic!("Save as Mesh is open") };
            let commands = crate::dialogs_save_mesh::commands(k, &d.inputs).unwrap();
            assert_eq!(commands[0].1["bodies"], json!(expected), "{mode:?}: clicked {clicked}");
            assert!(crate::dialogs_save_mesh::ready(&d.inputs));
            assert!(h.app.session.selection.is_empty(), "dialog picks do not change the global selection");
        }
        h.step(&json!({"click":{"handle":"row:Two"}})).unwrap();
        assert!(!crate::dialogs_save_mesh::ready(&h.app.dialog.as_ref().unwrap().inputs));
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"click":{"handle":"row:One"}})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::Body { name: "One".into() }]);
    }
}

#[test]
fn browser_body_clicks_respect_the_active_dialog_input() {
    let mut h = Harness::new();
    for (name, x) in [("One", 0), ("Two", 20)] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
    }
    h.app.start("solid.combine");
    h.app.dialog.as_mut().unwrap().preview_enabled = false;
    h.settle();
    h.step(&json!({"click":{"handle":"row:One"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().active, 1, "a target pick advances to Tool Bodies");
    h.step(&json!({"click":{"handle":"row:Two"}})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "One".into() }]);
    assert_eq!(d.inputs[1].items, vec![Sel::Body { name: "Two".into() }]);
    h.call("ui.dialogInput", json!({"index":0}));
    h.step(&json!({"click":{"handle":"row:Two"}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![Sel::Body { name: "Two".into() }], "a single input replaces its pick");
    h.step(&json!({"key":"Escape"})).unwrap();
    h.app.start("solid.fillet");
    h.settle();
    h.step(&json!({"click":{"handle":"row:One"}})).unwrap();
    assert!(h.app.dialog.as_ref().unwrap().inputs.iter().all(|i| i.items.is_empty()), "a body is not an edge");
    assert!(h.app.session.selection.is_empty(), "an incompatible pick does not leak into the global selection");
}

#[test]
fn browser_card_shrinks_and_scrolls_without_covering_history() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.settle();
    let short = rect_of("browser:card").unwrap();
    let history = rect_of("history:card").unwrap();
    assert_eq!(short.left(), SHELL_GAP);
    assert_eq!(short.width(), 250.0);
    assert_eq!(short.top(), rect_of("ribbon:card").unwrap().bottom() + SHELL_GAP);
    assert!(short.bottom() < history.top() - SHELL_GAP);
    assert!(drawn_rows().iter().all(|(_, rect, _, _)| rect.height() == 26.0));

    for i in 0..35 {
        h.app.run("sketch.create", json!({"plane":"XY", "name":format!("Sketch{i}")})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.settle();
    let tall = rect_of("browser:card").unwrap();
    assert!(tall.height() > short.height());
    assert!((tall.bottom() - (history.top() - SHELL_GAP)).abs() < 1.0, "{tall:?} {history:?}");
    assert!(h.app.viewport.rect.unwrap().contains_rect(tall));
    assert!(handle_at("row:Sketch34").is_none(), "last row starts below the scroll clip");
    let camera = h.app.cam;
    h.call("ui.scroll", json!({"x":tall.center().x,"y":tall.center().y,"delta":-1800}));
    h.settle();
    assert!(handle_at("row:Sketch34").is_some(), "scrolling must reveal the last row");
    assert_eq!(h.app.cam, camera);
    assert_eq!(rect_of("browser:card").unwrap(), tall);

    h.call("ui.set", json!({"layout":"docked"}));
    h.settle();
    assert_eq!(rect_of("browser:card").unwrap().left(), 0.0);
    assert_eq!(h.app.viewport.rect.unwrap().left(), 0.0);
    assert!(drawn_rows().iter().all(|(_, rect, _, _)| rect.height() == 26.0));
    h.call("ui.set", json!({"layout":"floating"}));
    h.app.tree.collapsed.insert("c0".into());
    h.settle();
    assert!(rect_of("browser:card").unwrap().height() < short.height());
}

#[test]
fn selected_row_paints_tab_soft_fill_and_accent_text_in_both_layouts() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for mode in [LayoutMode::Floating, LayoutMode::Docked] {
            for tab in ["MODEL", "SKETCH", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER"] {
                let ctx = crate::fonts::test_context();
                let mut app = SolveApp::new(Default::default(), Default::default());
                app.ui.layout = mode;
                app.ui.tab = tab.into();
                crate::theme::TabAccent::publish(&app, &ctx);
                let accent = crate::theme::TabAccent::for_tab(&Tokens::get(), tab);
                let mut row_rect = Rect::NOTHING;
                let out = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
                    let row = Row { selected: true, label: "Selected body", dim: true, color: Some(Tokens::get().error), ..Default::default() };
                    row_rect = draw_row(ui, egui::Id::new("selected-row"), &row).rect.unwrap();
                });
                assert_eq!(row_rect.height(), 26.0);
                assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == row_rect && r.fill == accent.soft)));
                assert!(
                    out.shapes
                        .iter()
                        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Selected body" && t.fallback_color == accent.ink))
                );
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn card_and_column_keep_body_eyes_menus_and_group_drops() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        for (name, x) in [("One", 0), ("Two", 20)] {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
        }
        let group =
            h.app.run("browser.group", json!({"component":0,"folder":"bodies","items":["One"],"name":"Group"})).unwrap()["group"].as_u64().unwrap();
        h.settle();
        h.step(&json!({"click":{"handle":"eye:Two"}})).unwrap();
        assert!(h.app.session.visibility.hidden_bodies.iter().any(|name| name == "Two"));
        h.step(&json!({"click":{"handle":"eye:Two"}})).unwrap();
        assert!(!h.app.session.visibility.hidden_bodies.iter().any(|name| name == "Two"));
        h.step(&json!({"click":{"handle":"row:Two"},"button":"right"})).unwrap();
        assert!(matches!(&h.app.menu.target, crate::context_menu::Target::Body { name } if name == "Two"));
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"drag":[{"handle":"row:Two"},{"handle":"row:Group (1)"}]})).unwrap();
        h.settle();
        let group = h.app.session.doc.browser_groups.iter().find(|g| g.id == group).unwrap();
        assert!(group.has_item("Two"), "body drop into the folder failed in {mode:?}");
    }
}

/// A second click on a Document Settings entry closes the window it opened, the way that
/// window's own × does, and a third opens it again (#4150): the Units menu, Preferences (a change
/// made there is put back), Parameters (a value typed but not applied is dropped) and Design
/// Properties (nothing typed there is saved).
#[test]
fn a_second_click_on_a_document_settings_entry_closes_its_window() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        h.app.run("parameters.add", json!({"name":"width","expression":"10 mm"})).unwrap();
        h.settle();
        h.step(&json!({"click":{"chevron":"Document Settings"}})).unwrap();
        h.settle();
        // The Units, Precision and Parameters labels carry their values.
        let row = |start: &str| {
            let (_, rect, _, _) = drawn_rows().into_iter().find(|(label, ..)| label.starts_with(start)).unwrap();
            json!({"click":[rect.center().x, rect.center().y]})
        };

        h.step(&row("Units")).unwrap();
        assert!(h.app.viewport.context_menu.is_some() && h.app.menu.target == Target::Units, "{mode:?}: the first click opens the Units menu");
        h.step(&row("Units")).unwrap();
        h.frames(2);
        assert!(h.app.viewport.context_menu.is_none(), "{mode:?}: the second click closes it instead of reopening it");
        h.step(&row("Units")).unwrap();
        assert!(h.app.viewport.context_menu.is_some(), "{mode:?}: and a third opens it again");
        h.step(&json!({"key":"Escape"})).unwrap();

        h.step(&row("Precision")).unwrap();
        h.frames(2);
        assert!(h.app.prefs_window.open, "{mode:?}: the first click opens Preferences");
        let decimals = h.app.preferences.angle_decimals;
        h.app.preferences.angle_decimals = decimals + 2;
        h.step(&row("Precision")).unwrap();
        h.frames(2);
        assert!(!h.app.prefs_window.open, "{mode:?}: the second click closes it");
        assert_eq!(h.app.preferences.angle_decimals, decimals, "{mode:?}: and puts back what changed there, as × does");
        h.step(&row("Precision")).unwrap();
        h.frames(2);
        assert!(h.app.prefs_window.open, "{mode:?}: a third opens it again");
        h.step(&json!({"key":"Escape"})).unwrap();

        h.step(&row("Parameters")).unwrap();
        h.frames(2);
        assert!(h.app.params.is_some(), "{mode:?}: the first click opens Parameters");
        h.step(&json!({"click":{"handle":"param:width"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"25 mm"})).unwrap();
        h.step(&row("Parameters")).unwrap();
        h.frames(2);
        assert!(h.app.params.is_none(), "{mode:?}: the second click closes it");
        assert_eq!(h.app.session.doc.param("width").unwrap().expr, "10 mm", "{mode:?}: and drops a value typed but not applied");
        h.step(&row("Parameters")).unwrap();
        assert!(h.app.params.is_some(), "{mode:?}: a third opens it again");
        h.app.params = None;

        h.step(&row("Design Properties")).unwrap();
        assert!(h.app.design_properties.is_some(), "{mode:?}: the first click opens Design Properties");
        h.app.design_properties.as_mut().unwrap().fields[0] = "Typed".into();
        h.frames(2);
        h.step(&row("Design Properties")).unwrap();
        assert!(h.app.design_properties.is_none(), "{mode:?}: the second click closes it");
        assert_eq!(h.app.session.doc.metadata.project(), None, "{mode:?}: and saves nothing typed there");
        h.step(&row("Design Properties")).unwrap();
        assert!(h.app.design_properties.is_some(), "{mode:?}: a third opens it again");
    }
}

#[test]
fn browser_header_folds_the_floating_card_to_its_header_and_back() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.settle();
    let open = rect_of("browser:card").unwrap();
    assert!(!h.app.preferences.browser_collapsed);
    let at = handle_at("browser:header").unwrap();
    h.call("ui.click", json!({"x":at.x,"y":at.y}));
    h.settle();
    let folded = rect_of("browser:card").unwrap();
    assert!(h.app.preferences.browser_collapsed);
    assert!(folded.height() < 60.0 && folded.height() < open.height(), "{folded:?} {open:?}");
    assert_eq!(folded.top(), open.top());
    assert!(drawn_rows().is_empty() || handle_at("row:Plate").is_none());
    h.call("ui.click", json!({"x":at.x,"y":at.y}));
    h.settle();
    assert!(!h.app.preferences.browser_collapsed);
    assert!((rect_of("browser:card").unwrap().height() - open.height()).abs() < 1.0);
}

/// A folded browser stays folded in the next run, as the toolbar's pins do (#8692).
#[test]
fn the_folded_browser_is_remembered_across_runs() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    h.settle();
    let open = rect_of("browser:card").unwrap();
    let at = handle_at("browser:header").unwrap();
    h.call("ui.click", json!({"x":at.x,"y":at.y}));
    h.settle();
    assert!(h.app.preferences.browser_collapsed);
    let saved = h.app.prefs();

    let mut next = Harness::new();
    next.app.load_prefs(&saved);
    assert!(next.app.preferences.browser_collapsed, "the fold round-trips through the settings");
    next.call("ui.resize", json!({"width":1400,"height":900}));
    next.settle();
    let folded = rect_of("browser:card").unwrap();
    assert!(folded.height() < 60.0 && folded.height() < open.height(), "the next run starts folded: {folded:?}");
    let at = handle_at("browser:header").unwrap();
    next.call("ui.click", json!({"x":at.x,"y":at.y}));
    next.settle();
    let mut after = SolveApp::new(Default::default(), Default::default());
    after.load_prefs(&next.app.prefs());
    assert!(!after.preferences.browser_collapsed, "unfolding is remembered too");
}

/// Reveal on hover (#8692): the browser is out of sight until the pointer comes over the strip at
/// its left edge or its place, stays while the pointer is inside it or its menu is open, and
/// never folds, even if it was folded before.
#[test]
fn a_browser_that_reveals_on_hover_shows_and_hides_with_the_pointer() {
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"operation":"new_body","body_name":"One"})).unwrap();
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.settle();
        let shown = rect_of("browser:card").unwrap();
        h.app.preferences.browser_collapsed = true;
        h.app.preferences.browser_show = BrowserShow::Hover;
        let away = json!({"x":900,"y":450});
        h.call("ui.move", away.clone());
        h.settle();
        assert!(h.app.shell.browser_hidden, "{mode:?}: out of sight while the pointer is elsewhere");
        let card = rect_of("browser:card").unwrap();
        assert!((card.height() - shown.height()).abs() < 1.0, "{mode:?}: the card keeps its unfolded place: {card:?} {shown:?}");

        let edge = handle_at("browser:edge").unwrap();
        h.call("ui.move", json!({"x":edge.x,"y":edge.y}));
        h.settle();
        assert!(!h.app.shell.browser_hidden, "{mode:?}: the left edge reveals it");
        let inside = rect_of("browser:card").unwrap().center();
        h.call("ui.move", json!({"x":inside.x,"y":inside.y}));
        h.settle();
        assert!(!h.app.shell.browser_hidden, "{mode:?}: it stays while the pointer is inside");
        h.call("ui.move", away.clone());
        h.settle();
        assert!(h.app.shell.browser_hidden, "{mode:?}: leaving hides it again");

        h.call("ui.move", json!({"x":inside.x,"y":inside.y}));
        h.settle();
        let header = handle_at("browser:header").unwrap();
        h.call("ui.click", json!({"x":header.x,"y":header.y}));
        h.settle();
        assert!(rect_of("browser:card").unwrap().height() > 60.0, "{mode:?}: the header does not fold it");
        h.step(&json!({"click":{"handle":"row:One"},"button":"right"})).unwrap();
        assert!(h.app.viewport.context_menu.is_some(), "{mode:?}: the row's menu opens");
        h.call("ui.move", away.clone());
        h.settle();
        assert!(!h.app.shell.browser_hidden, "{mode:?}: its menu keeps it shown");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.call("ui.move", away.clone());
        h.settle();
        assert!(h.app.shell.browser_hidden, "{mode:?}: closing the menu away from it hides it");

        h.app.preferences.browser_show = BrowserShow::Shown;
        h.settle();
        assert!(!h.app.shell.browser_hidden);
        assert!(rect_of("browser:card").unwrap().height() < 60.0, "{mode:?}: Shown folds it again as it was left");
        let mut restored = SolveApp::new(Default::default(), Default::default());
        h.app.preferences.browser_show = BrowserShow::Hover;
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.browser_show, BrowserShow::Hover);
    }
}

/// Tiny or odd window sizes never panic: the cards leave out what cannot fit.
#[test]
fn tiny_floating_windows_never_panic() {
    for (w, h) in [(100, 60), (240, 90), (320, 140), (700, 150), (700, 300), (1400, 120), (180, 800)] {
        for scale in [1.0, 2.0] {
            let mut harness = Harness::new();
            harness.call("ui.resize", json!({"width": w, "height": h}));
            harness.call("ui.prefs", json!({"ui_scale": scale}));
            harness.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
            harness.settle();
            for mode in [LayoutMode::Floating, LayoutMode::Docked] {
                harness.app.ui.layout = mode;
                harness.settle();
            }
        }
    }
}

/// Both shell layouts share one content-hugging tree and one saved width.
/// The translucency preference never creates a second browser renderer.
#[test]
fn one_browser_keeps_its_resized_width_across_layouts_and_overlay_preferences() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    for mode in [LayoutMode::Floating, LayoutMode::Docked] {
        for overlay in [false, true] {
            h.app.ui.layout = mode;
            h.app.preferences.browser_float = overlay;
            h.app.preferences.browser_width = 310.0;
            h.settle();
            assert!(rect_of("browser:card").is_some());
            assert_eq!(crate::scenario::handles("browser:header").len(), 1, "only one browser header");
            assert_eq!(drawn_rows().iter().filter(|(name, ..)| name == "Untitled").count(), 1, "only one tree");
            assert!((rect_of("browser:card").unwrap().width() - 310.0).abs() < 1.0);
            assert_eq!(h.app.viewport.rect.unwrap().left(), 0.0);
            let rail = handle_at("left-browser-rail").unwrap();
            h.call("ui.drag", json!({"x0":rail.x,"y0":rail.y,"x1":rail.x+60.0,"y1":rail.y,"steps":6}));
            h.settle();
            let width = h.app.preferences.browser_width;
            assert!(width > 340.0 && width <= MAX_BROWSER_WIDTH, "{mode:?}, overlay={overlay}: {width}");
            h.app.ui.layout = LayoutMode::Docked;
            h.app.preferences.browser_float = false;
            h.settle();
            assert!((rect_of("browser:card").unwrap().width() - width).abs() < 1.0, "compact keeps the saved width");
        }
    }
}
#[test]
fn browser_title_uses_a_single_semibold_run() {
    let ctx = crate::fonts::test_context();
    let mut app = SolveApp::new(Default::default(), Default::default());
    let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
        ui.set_width(250.0);
        browser_header(&mut app, ui, true);
    });
    let title = crate::i18n::tr("ui.browser_title");
    let titles: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(text) if text.galley.text() == title => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0].galley.job.sections[0].format.font_id, crate::fonts::semibold(Tokens::get().font_heading));
}

#[test]
fn resizing_browser_card_and_column_persists_width_including_floating_preference() {
    for (mode, float, left_history) in [
        (LayoutMode::Floating, false, false),
        (LayoutMode::Docked, false, false),
        (LayoutMode::Docked, true, false),
        (LayoutMode::Floating, true, true),
        (LayoutMode::Docked, true, true),
    ] {
        let mut h = Harness::new();
        h.app.ui.layout = mode;
        h.app.preferences.browser_float = float;
        h.app.preferences.timeline_left = left_history;
        h.call("ui.resize", json!({"width":1400,"height":900}));
        h.settle();
        let before = h.app.viewport.rect.unwrap();
        let at = handle_at("left-browser-rail").unwrap();
        h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x+80.0,"y1":at.y,"steps":8}));
        h.settle();
        let width = h.app.preferences.browser_width;
        assert!(width > crate::prefs::DEFAULT_BROWSER_WIDTH + 40.0, "{mode:?}, float={float}, width={width}");
        assert!((rect_of("browser:card").unwrap().width() - width).abs() < 1.0);
        assert_eq!(h.app.viewport.rect.unwrap(), before, "browser resizing leaves the projection intact");
        let mut restored = SolveApp::new(Default::default(), Default::default());
        restored.load_prefs(&h.app.prefs());
        assert_eq!(restored.preferences.browser_width, width);
        assert_eq!(restored.preferences.browser_float, float);
    }
}

#[test]
fn browser_width_rejects_hostile_preferences_and_tiny_windows_still_fit() {
    for width in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(browser_width(width), crate::prefs::DEFAULT_BROWSER_WIDTH);
    }
    assert_eq!(browser_width(-100.0), MIN_BROWSER_WIDTH);
    assert_eq!(browser_width(f32::MAX), MAX_BROWSER_WIDTH);
    let available = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 300.0));
    for top_gap in [0.0, crate::shell::SHELL_GAP] {
        let bounds = panel_bounds(available, None, MAX_BROWSER_WIDTH, top_gap);
        assert!(available.contains_rect(bounds));
    }
}

#[test]
fn a_long_body_name_stays_inside_the_card_and_ends_in_an_ellipsis() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    let name = "W".repeat(200);
    h.app.run("solid.box", json!({"body_name":name, "operation":"new", "length":10, "width":10, "height":10})).unwrap();
    h.settle();
    let card = rect_of("browser:card").unwrap();
    let label = rect_of(&format!("row_label:{name}")).expect("the body row is drawn");
    assert!(label.right() <= card.right(), "{label:?} spills out of {card:?}");
    assert!(label.width() > 100.0, "the name keeps the room it has: {label:?}");
}

#[test]
fn collapsed_rail_shares_header_hover_inset_and_expands_from_its_whole_card() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for collapsed in [false, true] {
            let ctx = crate::fonts::test_context();
            ctx.global_style_mut(|style| style.animation_time = 0.0);
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.ui.tab = "SURFACE".into();
            app.preferences.timeline_left = true;
            app.preferences.timeline_left_browser_collapsed = collapsed;
            crate::theme::TabAccent::publish(&app, &ctx);
            let bounds = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 700.0));
            let draw = |app: &mut SolveApp, events| {
                crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(bounds), events, ..Default::default() }, |ui| {
                    browser(app, ui);
                })
            };
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            let card = rect_of("browser:card").unwrap();
            let header = rect_of("browser:header").unwrap();
            // Hover at the bottom margin, far away from the disclosure and rotated title.
            let at = if collapsed { card.center_bottom() - vec2(0.0, 2.0) } else { header.center() };
            draw(&mut app, vec![egui::Event::PointerMoved(at)]);
            let output = draw(&mut app, vec![egui::Event::PointerMoved(at)]);
            assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
            let accent = crate::theme::TabAccent::current(&ctx);
            let mut hover = None;
            for shape in &output.shapes {
                crate::test_support::each_shape(&shape.shape, &mut |shape| {
                    if let egui::Shape::Rect(rect) = shape
                        && rect.fill == accent.soft
                    {
                        hover = Some(rect.clone());
                    }
                });
            }
            let hover =
                hover.unwrap_or_else(|| panic!("dark={dark}, collapsed={collapsed}, at={at:?}, accent={accent:?}, shapes={:?}", output.shapes));
            let inset = CARD_INSET + 1.0; // shared frame's inner margin and border
            assert_eq!(hover.rect, if collapsed { card.shrink(inset) } else { header });
            assert_eq!(hover.rect.left() - card.left(), inset);
            assert_eq!(card.right() - hover.rect.right(), inset);
            assert_eq!(hover.corner_radius, egui::CornerRadius::same(Tokens::get().radius_lg.saturating_sub(CARD_INSET as u8)));
            if collapsed {
                draw(&mut app, crate::test_support::pointer(at, true));
                draw(&mut app, crate::test_support::pointer(at, false));
                assert!(!app.preferences.timeline_left_browser_collapsed, "the bottom margin expands the rail");
            }
        }
    }
    crate::theme::set_dark(previous);
}

/// #6907: a mesh body's row has its own icon and tooltip, so it can be told from a solid one.
#[test]
fn mesh_and_solid_body_rows_have_different_icons_and_tooltips() {
    use solidcraft_engine::doc::{Document, FeatureKind, MeshData};
    use solidcraft_engine::geom::Vec3;
    let solid = solidcraft_engine::kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
    let mesh = solid.tessellate(0.01).unwrap();
    let mut doc = Document::default();
    doc.add_feature(
        FeatureKind::MeshImport {
            file: "box.stl".into(),
            meshes: vec![MeshData {
                name: "Mesh".into(),
                positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: mesh.triangles.iter().flatten().copied().collect(),
                color: None,
            }],
        },
        Some("Imported mesh"),
    )
    .unwrap();
    let mut h = Harness::new();
    h.app.session = solidcraft_engine::Session::new(doc);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"operation":"new_body","body_name":"Solid"})).unwrap();
    h.settle();
    let rows = entries(&h.app, 0, "bodies");
    let row = |name: &str| rows.iter().find(|e| e.key == name).unwrap_or_else(|| panic!("no {name} row in {:?}", body_rows(&h.app)));
    assert_eq!((row("Mesh").icon, row("Mesh").tip), ("mesh_body", Some("ui.body_kind_mesh")));
    assert_eq!((row("Solid").icon, row("Solid").tip), ("body", Some("ui.body_kind_solid")));
    assert_eq!(crate::i18n::tr("ui.body_kind_mesh"), "Mesh body");
    assert_eq!(crate::i18n::tr("ui.body_kind_solid"), "Solid body");
    // Its own drawing in the mesh colours, not the solid body's grey cube.
    let icon = crate::icon_svg::resolve("mesh_body");
    assert!(!icon.placeholder);
    assert_ne!(icon.master, crate::icon_svg::resolve("body").master);
    assert_eq!(icon.accent, crate::icon_svg::accent_index("mesh"));
}

#[test]
fn browser_occurrence_clicks_fill_align_components_input() {
    let mut h = Harness::new();
    let comp = h.app.run("component.create", json!({"name": "Extrusion", "grounded": false})).unwrap()["component"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 40, "body_name": "Beam"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    let second = h.app.run("occurrence.copy", json!({"component": comp, "translate": [30, 0, 0]})).unwrap()["occurrence"].as_u64().unwrap();

    h.app.start("solid.align");
    {
        let d = h.app.dialog.as_mut().unwrap();
        d.kind = crate::dialogs::Kind::Align { flip: false, components: true, axes: [true; 3], angle: String::new() };
        d.inputs[0].label = "Components";
        d.active = 0;
    }
    h.settle();

    let handle = format!("row:occurrence:{second}");
    h.step(&json!({"click": {"handle": &handle}})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs[0].items, vec![Sel::Occurrence { path: vec![second] }]);
    assert!(h.app.session.selection.is_empty(), "dialog picks do not change global selection");

    // Clicking the same occurrence row again toggles it off in the multi-select Components input.
    h.step(&json!({"click": {"handle": &handle}})).unwrap();
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
}

/// #8718: the eyes form one column at the rows' right end, right of each name and radio at every
/// depth, and a click on one hides without activating; the preference puts them back by the fold arrow.
#[test]
fn row_eyes_line_up_at_the_right_end_unless_the_preference_keeps_them_left() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Top"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Part"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"body_name":"Inner"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.settle();
    let placement = format!("occurrence:{part}");
    let eye = |name: &str| handle_at(&format!("eye:{name}")).unwrap_or_else(|| panic!("eye:{name}"));
    let label = |name: &str| rect_of(&format!("row_label:{name}")).unwrap_or_else(|| panic!("row_label:{name}"));
    let activate = || handle_at(&format!("activate:{placement}")).unwrap();
    let card = rect_of("browser:card").unwrap();
    let column = eye(&placement).x;
    for name in ["Bodies", "Top", "Inner", placement.as_str()] {
        assert!((eye(name).x - column).abs() < 0.5, "{name}: {} is off the column at {column}", eye(name).x);
    }
    for name in ["Bodies", "Top", "Inner"] {
        assert!(eye(name).x > label(name).right(), "{name}: the eye is right of the name");
    }
    assert!(column > activate().x + 40.0 && column > card.right() - 40.0, "the eye sits at the row's end, apart from the radio");
    assert!(eye("Inner").y > eye(&placement).y + 10.0, "the body sits under its placement");
    let indent = |name: &str| handle_at(&format!("indent:{name}")).unwrap_or_else(|| panic!("indent:{name}")).x;
    assert!((indent("Inner") - indent("Part:1") - super::INDENT).abs() < 0.5, "one level deeper, one indent further right");
    assert!((label("Inner").left() - label("Top").left()).abs() < 0.5, "bodies at one depth start their names together");
    // Its tip shows by the eye, not under the name.
    h.step(&json!({"move":{"handle":"eye:Top"}})).unwrap();
    h.frames(60);
    h.frame();
    let mut tip = None;
    for clipped in &h.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |s| {
            if let egui::Shape::Text(t) = s
                && t.galley.text() == "Hide"
            {
                tip = Some(t.pos.x + t.galley.rect.center().x);
            }
        });
    }
    let tip = tip.expect("the eye's Hide tip");
    assert!(tip > label("Top").right(), "the tip at {tip} is by the eye at {}", eye("Top").x);

    // Between the fold arrow and the radio, a click neither folds nor activates.
    let (fold, radio) = (handle_at(&format!("fold:{placement}")).unwrap(), activate());
    assert!(radio.x - fold.x >= 24.0, "the radio keeps clear of the fold arrow: {fold:?} {radio:?}");
    for dx in [11.0, 14.0] {
        h.frames(40);
        h.step(&json!({"click":[fold.x + dx, fold.y]})).unwrap();
        h.settle();
        assert!(handle_at("indent:Inner").is_some(), "{dx}: the placement stays open");
        assert_eq!(h.app.session.active_component, 0, "{dx}: nothing is activated");
    }
    h.frames(40);
    h.step(&json!({"click":{"handle":format!("eye:{placement}")}})).unwrap();
    assert!(h.app.session.occurrence_hidden(&[part]), "the placement's eye hides it");
    h.step(&json!({"click":{"handle":"eye:Top"}})).unwrap();
    assert!(h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Top"), "the body's eye hides it");
    assert_eq!(h.app.session.active_component, 0, "an eye click activates nothing");
    assert!(h.app.session.active_occurrence_path.is_none());

    h.call("ui.prefs", json!({"browser_eye_left": true}));
    h.settle();
    assert!(eye("Top").x < label("Top").left() && eye(&placement).x < activate().x, "beside the fold arrow, as before");
    assert!((eye("Inner").x - eye(&placement).x - super::INDENT).abs() < 0.5, "one level deeper, one indent further right");
    h.step(&json!({"click":{"handle":"eye:Top"}})).unwrap();
    assert!(!h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Top"), "the eye still shows it");
    assert_eq!(h.app.session.active_component, 0);
}

/// #8718: with the rows overflowing the card, an eye at the rows' end stays clear of the scroll
/// bar laid over them: a click on it hides the sketch and scrolls nothing.
#[test]
fn an_eye_at_the_row_end_takes_its_click_from_the_scroll_bar() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    for i in 0..35 {
        h.app.run("sketch.create", json!({"plane":"XY", "name":format!("Sketch{i}")})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.settle();
    assert!(handle_at("row:Sketch34").is_none(), "the rows overflow the card");
    let card = rect_of("browser:card").unwrap();
    let eye = handle_at("eye:Sketch3").unwrap();
    let row = handle_at("row:Sketch3").unwrap();
    let bar = h.ctx.global_style().spacing.scroll.bar_width;
    let row_rect = drawn_rows().into_iter().find(|r| r.0 == "Sketch3").unwrap().1;
    assert!(card.contains_rect(row_rect));
    let half = (Tokens::get().space_16 + Tokens::get().space_3) / 2.0;
    assert!(eye.x + half <= row_rect.right() - bar, "the eye at {} is left of the bar along {}", eye.x, row_rect.right());
    // The bar widens under the pointer before the click.
    h.step(&json!({"move":[row_rect.right() - 3.0, eye.y]})).unwrap();
    h.frames(30);
    // The eye's edge nearest the bar.
    h.step(&json!({"click":[eye.x + half - 1.0, eye.y]})).unwrap();
    h.settle();
    assert_eq!(h.app.session.visibility.hidden_sketches.len(), 1, "the eye hides its sketch");
    assert_eq!(handle_at("row:Sketch3"), Some(row), "the list did not scroll");
}

/// Dragging a group's row onto the middle of another group's row nests it; onto the top of a row
/// puts it before that group. A group cannot be dropped into one of its own groups.
#[test]
fn a_group_dragged_onto_another_group_nests_in_it_or_goes_before_it() {
    let mut h = Harness::new();
    let ids: Vec<u64> = ["Alpha", "Beta", "Gamma"]
        .into_iter()
        .enumerate()
        .map(|(i, name)| {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[i as f64 * 20.0,0,0],"body_name":name})).unwrap();
            h.app.run("browser.group", json!({"component":0,"folder":"bodies","items":[name],"name":name})).unwrap()["group"].as_u64().unwrap()
        })
        .collect();
    let (alpha, beta, gamma) = (ids[0], ids[1], ids[2]);
    h.settle();
    let drag = |h: &mut Harness, from: &str, to: &str, dy: f32| {
        let (from, to) = (handle_at(from).unwrap(), handle_at(to).unwrap());
        assert_eq!(h.call("ui.drag", json!({"x0":from.x,"y0":from.y,"x1":to.x,"y1":to.y + dy,"steps":12}))["ok"], true);
        h.settle();
    };
    let parent = |h: &Harness, id: u64| h.app.session.doc.browser_group(id).unwrap().parent;
    drag(&mut h, "row:Alpha (1)", "row:Beta (1)", 0.0);
    assert_eq!(parent(&h, alpha), Some(beta));
    drag(&mut h, "row:Beta (2)", "row:Alpha (1)", 0.0);
    assert_eq!(parent(&h, beta), None, "a group does not go into its own group");
    drag(&mut h, "row:Gamma (1)", "row:Beta (2)", -10.0);
    assert_eq!(h.app.session.doc.ordered_entries(0, "bodies", None), [BrowserEntry::Group(gamma), BrowserEntry::Group(beta)]);
}

/// Three bodies with their eyes in the Browser, the first one's eye hidden or shown per the test.
fn three_bodies(h: &mut Harness) {
    for (name, x) in [("One", 0), ("Two", 20), ("Three", 40)] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
    }
    h.settle();
}

fn body_hidden(h: &Harness, name: &str) -> bool {
    h.app.session.visibility.hidden_bodies.iter().any(|b| b == name)
}

/// #9680: a press on an eye that drags over other eyes sets each eye it passes to the first eye's
/// new state, and the whole drag is one undo step.
#[test]
fn eye_drag_sets_every_eye_it_passes_in_one_undo_step() {
    for eye_left in [false, true] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        if eye_left {
            h.call("ui.prefs", json!({"browser_eye_left": true}));
            h.settle();
        }
        let undo = h.app.session.undo.len();
        h.step(&json!({"drag":[{"handle":"eye:One"},{"handle":"eye:Three"}],"steps":40})).unwrap();
        h.settle();
        for name in ["One", "Two", "Three"] {
            assert!(body_hidden(&h, name), "eye_left {eye_left}: {name} is hidden by the drag");
        }
        assert_eq!(h.app.session.undo.len(), undo + 1, "eye_left {eye_left}: the drag is one undo step");
        h.app.run("edit.undo", json!({})).unwrap();
        for name in ["One", "Two", "Three"] {
            assert!(!body_hidden(&h, name), "eye_left {eye_left}: one undo shows {name} again");
        }
    }
}

/// #9680: an eye already in the target state stays as it is: a drag from a shown eye over a hidden
/// one hides both, and undo brings back the hidden one as it was.
#[test]
fn eye_drag_leaves_an_eye_already_in_the_target_state_alone() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    h.app.run("browser.visibility", json!({"items":["Two"],"visible":false})).unwrap();
    h.settle();
    h.step(&json!({"drag":[{"handle":"eye:One"},{"handle":"eye:Three"}],"steps":40})).unwrap();
    h.settle();
    for name in ["One", "Two", "Three"] {
        assert!(body_hidden(&h, name), "{name} is hidden: the hidden eye is not flipped back");
    }
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(!body_hidden(&h, "One") && !body_hidden(&h, "Three"), "undo shows the two the drag hid");
    assert!(body_hidden(&h, "Two"), "undo keeps the eye hidden before the drag");
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(!body_hidden(&h, "Two"), "the hide before the drag is its own undo step");
}

/// #9680: a plain click on an eye still toggles that one row, as one undo step.
#[test]
fn a_plain_eye_click_still_toggles_one_row_as_one_undo_step() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"eye:Two"}})).unwrap();
    h.settle();
    assert!(body_hidden(&h, "Two") && !body_hidden(&h, "One") && !body_hidden(&h, "Three"), "only Two is hidden");
    assert_eq!(h.app.session.undo.len(), undo + 1, "the click is one undo step");
}

/// #9680: a drag over the eyes that rests near the rows' bottom edge scrolls them, and the eyes
/// the rows bring under the pointer take the drag's state too.
#[test]
fn eye_drag_near_the_rows_bottom_edge_scrolls_them() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1400,"height":900}));
    for i in 0..35 {
        h.app.run("sketch.create", json!({"plane":"XY", "name":format!("Sketch{i}")})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
    }
    h.settle();
    assert!(handle_at("row:Sketch34").is_none(), "the rows overflow the card");
    let card = rect_of("browser:card").unwrap();
    let eye = handle_at("eye:Sketch0").unwrap();
    h.call("ui.drag", json!({"x0": eye.x, "y0": eye.y, "x1": eye.x, "y1": card.bottom() - 10.0, "steps": 20, "hold_end": 240}));
    h.settle();
    assert!(handle_at("row:Sketch34").is_some(), "the drag scrolled the list down to its last row");
    assert!(h.app.session.visibility.hidden_sketches.len() > 10, "the eyes the drag reached are hidden");
}

/// #9680: a drag from the Origin's eye over the plane eyes is one undo step of its own, and the show
/// before it stays a step: one undo takes back only the drag. The Origin's eye is not an undo step (a
/// click on it is not either), so that one undo leaves it as the drag set it.
#[test]
fn eye_drag_from_the_origin_is_its_own_undo_step_and_leaves_an_earlier_show_alone() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"One"})).unwrap();
    h.app.run("browser.visibility", json!({"items":["One"],"visible":false})).unwrap();
    h.app.run("browser.visibility", json!({"items":["One"],"visible":true})).unwrap();
    h.app.run("browser.expand_all", json!({})).unwrap();
    h.settle();
    let undo = h.app.session.undo.len();
    h.step(&json!({"drag":[{"handle":"eye:Origin"},{"handle":"eye:YZ"}],"steps":40})).unwrap();
    h.settle();
    let origin_hidden = |h: &Harness| h.app.session.visibility.origin_hidden == Some(true);
    assert!(origin_hidden(&h) && h.app.session.visibility.hidden_origin.iter().any(|k| k == "YZ"), "the drag hid the Origin and its planes");
    assert_eq!(h.app.session.undo.len(), undo + 1, "the drag is one undo step");
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(!body_hidden(&h, "One"), "one undo takes back only the drag: the show before it stays");
    assert!(h.app.session.visibility.hidden_origin.is_empty(), "the planes the drag hid are shown again");
    assert!(origin_hidden(&h), "the Origin's eye is not an undo step: it keeps what the drag set");
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(body_hidden(&h, "One"), "the show is its own undo step: undoing it hides One again");
}

/// #9953: hiding or showing a sketch or body dynamically dims/undims the row without needing a full row rebuild.
#[test]
fn hiding_or_showing_body_or_sketch_updates_row_dimming() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"One"})).unwrap();
    h.app.run("browser.expand_all", json!({})).unwrap();
    h.settle();

    // Toggle body hidden via eye click
    h.step(&json!({"click":{"handle":"eye:One"}})).unwrap();
    h.settle();
    assert!(body_hidden(&h, "One"));

    // Toggle body shown again via eye click
    h.step(&json!({"click":{"handle":"eye:One"}})).unwrap();
    h.settle();
    assert!(!body_hidden(&h, "One"));
}
