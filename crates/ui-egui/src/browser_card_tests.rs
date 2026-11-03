use super::*;
use crate::scenario::{Harness, handle_at, rect_of};
use crate::semantic_content::{self, ContentOwner, ContentRequest};
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
        // The group's row sits inside a placement, which starts closed (#9274).
        h.app.preferences.browser_components_open = true;
        let body = h.app.session.model.state().bodies[0].name.clone();
        let group = h.app.run("browser.group", json!({"component":component,"folder":"bodies","items":[body],"name":"Group"})).unwrap()["group"]
            .as_u64()
            .unwrap();
        h.app.preferences.browser_bodies_folders = crate::prefs::BodiesFolders::Always;
        h.settle();
        let world_body = crate::open_meshing::session_world(&h.app.session).bodies[0].name.clone();
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
        assert!(crate::open_meshing::session_world(&h.app.session).body(&world_body).is_some(), "the body keeps its name");
    }
}

/// #10654: a refused inline rename keeps the editor open and says why, instead of closing
/// silently and leaving the old name.
#[test]
fn a_refused_inline_rename_keeps_the_editor_and_shows_the_error() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"A"})).unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"B"})).unwrap();
    h.app.preferences.browser_bodies_folders = crate::prefs::BodiesFolders::Always;
    h.settle();
    h.app.menu.rename = Some(crate::context_menu::Rename {
        what: crate::context_menu::RenameWhat::Body("B".into()),
        text: "A".into(),
        at: egui::Pos2::ZERO,
        focused: true,
        inline: true,
    });
    h.frames(2);
    h.step(&json!({"key":"Enter"})).unwrap();
    h.frames(2);
    assert!(h.app.menu.rename.is_some(), "the editor stays open after a refused rename");
    let (text, _, error) = h.app.status.clone().expect("the refusal is shown");
    assert!(error && text.contains("another body is called"), "{text}");
    assert!(h.app.session.world_state().unwrap().body("B").is_some(), "the body keeps its name");
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
                    row_rect = draw_row(ui, egui::Id::unique("selected-row"), &row).rect.unwrap();
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
    use solidcore_record::Vec3;
    use solidcraft_engine::doc::{Document, FeatureKind, MeshData};
    let solid = solidcraft_engine::kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
    let mesh = solid.mesh(&solidcraft_engine::kernel::MeshTolerance::chord(0.01)).unwrap();
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
    // These rows are read with every placement open, as placements started before #9274.
    h.app.preferences.browser_components_open = true;
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

/// Draw content without either panel host. Changing submission order does not change IDs,
/// screen placement or the input stream, so these exercise the adapters' shared boundary.
fn semantic_frame(h: &mut Harness, owners: &[ContentOwner], events: Vec<egui::Event>, modifiers: egui::Modifiers) {
    semantic_frame_switch(h, owners, events, modifiers, None);
}

fn semantic_frame_switch(h: &mut Harness, owners: &[ContentOwner], events: Vec<egui::Event>, modifiers: egui::Modifiers, tab: Option<usize>) {
    semantic_frame_transitions(h, owners, events, modifiers, tab, None);
}

fn semantic_frame_transitions(
    h: &mut Harness,
    owners: &[ContentOwner],
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    tab: Option<usize>,
    after_content: Option<usize>,
) {
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 900.0))),
        focused: true,
        events: std::iter::once(egui::Event::ModifiersChanged(modifiers)).chain(events).collect(),
        ..Default::default()
    };
    h.ctx
        .run_ui(raw, |ui| {
            crate::scenario::clear_handles();
            publish_row_prefs(&h.app, ui.ctx());
            eye_drag_frame(&mut h.app, ui.ctx());
            semantic_content::begin(&mut h.app, ui.ctx());
            if let Some(tab) = tab {
                crate::documents::switch(&mut h.app, tab);
            }
            for &owner in owners {
                let x = if owner == ContentOwner::BROWSER { 10.0 } else { 450.0 };
                let mut content = ui.new_child(
                    egui::UiBuilder::new().id_salt(("semantic-test", owner)).max_rect(Rect::from_min_size(pos2(x, 10.0), vec2(400.0, 850.0))),
                );
                let request = if owner == ContentOwner::HISTORY {
                    ContentRequest::ActiveBodiesOrigin
                } else {
                    ContentRequest::Browser { shrink_height: false }
                };
                semantic_content::show(&mut h.app, &mut content, owner, request);
            }
            if let Some(tab) = after_content {
                crate::documents::switch(&mut h.app, tab);
            }
            semantic_content::finalize(&mut h.app, ui.ctx());
            // App paths may converge after an early return; finalization must be idempotent.
            semantic_content::finalize(&mut h.app, ui.ctx());
            if ui.ctx().data_mut(|data| data.remove_temp::<bool>(egui::Id::unique("semantic-test-discard"))).unwrap_or(false) {
                ui.ctx().request_discard("repeat semantic layout after input");
            }
        })
        .textures_delta
        .clear();
}

fn semantic_at(owner: ContentOwner, label: &str) -> Pos2 {
    semantic_content::output(owner).rows.into_iter().find(|(name, ..)| name == label).unwrap().1.center()
}

fn semantic_control(owner: ContentOwner, handle: &str) -> Pos2 {
    let rows = semantic_content::output(owner).rows;
    crate::scenario::handles(handle)
        .into_iter()
        .find(|(name, point)| name == handle && rows.iter().any(|(_, rect, ..)| rect.contains(*point)))
        .unwrap()
        .1
}

fn semantic_button(at: Pos2, button: egui::PointerButton, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::PointerButton { pos: at, button, pressed, modifiers }
}

fn semantic_click(h: &mut Harness, owners: &[ContentOwner], at: Pos2, modifiers: egui::Modifiers) {
    semantic_frame(h, owners, vec![egui::Event::PointerMoved(at)], modifiers);
    semantic_frame(h, owners, vec![semantic_button(at, egui::PointerButton::Primary, true, modifiers)], modifiers);
    semantic_frame(h, owners, vec![semantic_button(at, egui::PointerButton::Primary, false, modifiers)], modifiers);
}

#[test]
fn history_bodies_and_origin_work_with_browser_hidden() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    h.app.ui.show_browser = false;
    h.app.preferences.timeline_left = true;
    h.app.preferences.timeline_left_width = 260.0;
    h.app.preferences.timeline_origin_bodies = true;
    h.app.tree.expanded.insert("origin".into());
    h.frames(4);
    assert!(drawn_rows().is_empty() && drawn_occurrences().is_empty());
    let body = semantic_at(ContentOwner::HISTORY, "One");
    h.step(&json!({"move":[body.x, body.y]})).unwrap();
    assert_eq!(h.app.viewport.hover_bodies, ["One"]);
    h.step(&json!({"click":[body.x, body.y]})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Body { name: "One".into() }]);
    let eye = semantic_control(ContentOwner::HISTORY, "eye:One");
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":[eye.x, eye.y]})).unwrap();
    assert!(body_hidden(&h, "One"));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let plane = semantic_at(ContentOwner::HISTORY, "XY");
    h.step(&json!({"move":[plane.x, plane.y]})).unwrap();
    assert_eq!(h.app.viewport.hover_plane.as_deref(), Some("XY"));
    h.step(&json!({"click":[plane.x, plane.y]})).unwrap();
    assert_eq!(h.app.session.selection, [Sel::Plane { name: "XY".into() }]);
    h.step(&json!({"click":[plane.x, plane.y],"button":"right"})).unwrap();
    assert_eq!(h.app.menu.target, Target::OriginPlane { name: "XY".into() });
    crate::context_menu::close(&mut h.app);
    h.frames(2);
    let body = semantic_at(ContentOwner::HISTORY, "Two");
    h.step(&json!({"click":[body.x, body.y],"button":"right"})).unwrap();
    assert_eq!(h.app.menu.target, Target::Body { name: "Two".into() });
    crate::context_menu::close(&mut h.app);
    h.app.run("select.clear", json!({})).unwrap();
    h.app.start("solid.combine");
    h.frames(2);
    for name in ["One", "Two"] {
        let body = semantic_at(ContentOwner::HISTORY, name);
        h.step(&json!({"click":[body.x, body.y]})).unwrap();
    }
    let dialog = h.app.dialog.as_ref().unwrap();
    assert_eq!(dialog.inputs[0].items, [Sel::Body { name: "One".into() }]);
    assert_eq!(dialog.inputs[1].items, [Sel::Body { name: "Two".into() }]);
    assert!(h.app.session.selection.is_empty(), "tool picks do not leak into document selection");
}

#[test]
fn semantic_owner_submission_order_preserves_hover_selection_picks_and_single_visibility_command() {
    for owners in [[ContentOwner::BROWSER, ContentOwner::HISTORY], [ContentOwner::HISTORY, ContentOwner::BROWSER]] {
        for owner in [ContentOwner::BROWSER, ContentOwner::HISTORY] {
            let mut h = Harness::new();
            three_bodies(&mut h);
            for _ in 0..3 {
                semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
            }
            let body = semantic_at(owner, "One");
            semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(body)], egui::Modifiers::NONE);
            assert_eq!(h.app.viewport.hover_bodies, ["One"], "{owners:?}, {owner:?}");
            let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
            semantic_click(&mut h, &owners, body, ctrl);
            assert_eq!(h.app.session.selection, [Sel::Body { name: "One".into() }], "Ctrl must toggle exactly once");
            let eye = semantic_control(owner, "eye:Two");
            let undo = h.app.session.undo.len();
            semantic_click(&mut h, &owners, eye, egui::Modifiers::NONE);
            assert_eq!(h.app.session.undo.len(), undo + 1, "visibility command runs once");
            assert!(body_hidden(&h, "Two"));
            h.app.run("select.clear", json!({})).unwrap();
            h.app.start("solid.combine");
            for name in ["One", "Three"] {
                let body = semantic_at(owner, name);
                semantic_click(&mut h, &owners, body, egui::Modifiers::NONE);
            }
            let dialog = h.app.dialog.as_ref().unwrap();
            assert_eq!(dialog.inputs[0].items, [Sel::Body { name: "One".into() }]);
            assert_eq!(dialog.inputs[1].items, [Sel::Body { name: "Three".into() }]);
            assert!(h.app.session.selection.is_empty());
        }
    }
}

#[test]
fn absent_semantic_owners_expire_geometry_and_hover_without_losing_selection_or_folds() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    h.app.tree.expanded.insert("origin".into());
    let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let body = semantic_at(ContentOwner::HISTORY, "One");
    semantic_click(&mut h, &owners, body, egui::Modifiers::NONE);
    assert_eq!(h.app.viewport.hover_bodies, ["One"]);
    let selected = h.app.session.selection.clone();
    let folds = (h.app.tree.collapsed.clone(), h.app.tree.expanded.clone());
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    assert!(h.app.viewport.hover_bodies.is_empty() && h.app.viewport.hover_plane.is_none());
    for owner in owners {
        let output = semantic_content::output(owner);
        assert!(output.rows.is_empty() && output.occurrences.is_empty());
    }
    assert_eq!(h.app.session.selection, selected);
    assert_eq!((h.app.tree.collapsed.clone(), h.app.tree.expanded.clone()), folds);
    // Unrelated viewport contributors must survive frames with no semantic presentations.
    h.app.viewport.hover_bodies = vec!["external contributor".into()];
    h.app.viewport.hover_plane = Some("external plane".into());
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    assert_eq!(h.app.viewport.hover_bodies, ["external contributor"]);
    assert_eq!(h.app.viewport.hover_plane.as_deref(), Some("external plane"));
    // Egui hit-tests the previous pass: register the reappearing rows before moving to one.
    semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    let body = semantic_at(ContentOwner::HISTORY, "One");
    semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(body)], egui::Modifiers::NONE);
    assert!(
        h.app.viewport.hover_bodies.contains(&"external contributor".into()) && h.app.viewport.hover_bodies.contains(&"One".into()),
        "hover contributors: {:?}",
        h.app.viewport.hover_bodies
    );
    let plane = semantic_at(ContentOwner::HISTORY, "XY");
    semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(plane)], egui::Modifiers::NONE);
    assert_eq!(h.app.viewport.hover_plane.as_deref(), Some("XY"));
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    assert_eq!(h.app.viewport.hover_bodies, ["external contributor"]);
    assert_eq!(h.app.viewport.hover_plane.as_deref(), Some("external plane"));
}

#[test]
fn semantic_shift_and_ctrl_ranges_use_each_owners_anchor_and_complete_previous_order() {
    for owners in [[ContentOwner::BROWSER, ContentOwner::HISTORY], [ContentOwner::HISTORY, ContentOwner::BROWSER]] {
        for (owner, other) in [(ContentOwner::BROWSER, ContentOwner::HISTORY), (ContentOwner::HISTORY, ContentOwner::BROWSER)] {
            let mut h = Harness::new();
            three_bodies(&mut h);
            for _ in 0..3 {
                semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
            }
            let first = semantic_at(owner, "One");
            semantic_click(&mut h, &owners, first, egui::Modifiers::NONE);
            let unrelated = semantic_at(other, "Three");
            semantic_click(&mut h, &owners, unrelated, egui::Modifiers::NONE);
            let middle = semantic_at(owner, "Two");
            semantic_click(&mut h, &owners, middle, egui::Modifiers { shift: true, ..Default::default() });
            assert_eq!(h.app.session.selection, [Sel::Body { name: "One".into() }, Sel::Body { name: "Two".into() }]);
            // Start below the target, so the current order is incomplete at the Shift click.
            let last = semantic_at(owner, "Three");
            semantic_click(&mut h, &owners, last, egui::Modifiers::NONE);
            let first = semantic_at(owner, "One");
            semantic_click(&mut h, &owners, first, egui::Modifiers { shift: true, ctrl: true, ..Default::default() });
            let names: BTreeSet<_> = h.app.session.selection.iter().filter_map(|selection| picked_body(selection).map(str::to_owned)).collect();
            assert_eq!(names, BTreeSet::from(["One".into(), "Two".into(), "Three".into()]));
        }
    }
}

#[test]
fn browser_body_reveal_survives_history_only_and_unrelated_presentations() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    reveal_body(&mut h.app, "Three");
    for owners in [&[ContentOwner::HISTORY][..], &[][..], &[ContentOwner::HISTORY][..]] {
        semantic_frame(&mut h, owners, Vec::new(), egui::Modifiers::NONE);
        assert_eq!(h.app.tree.reveal.as_deref(), Some("Three"));
    }
    h.app.ui.show_browser = true;
    h.app.ui.show_timeline = false;
    h.app.preferences.browser_collapsed = true;
    h.frames(3);
    assert_eq!(h.app.tree.reveal.as_deref(), Some("Three"), "collapsed Browser does not acknowledge a row");
    h.app.preferences.browser_collapsed = false;
    for _ in 0..4 {
        semantic_frame(&mut h, &[ContentOwner::HISTORY, ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
    }
    assert!(h.app.tree.reveal.is_none());
    assert!(drawn_rows().iter().any(|(label, ..)| label == "Three"));
}

#[test]
fn history_reveal_survives_hidden_and_nonmatching_content_in_all_existing_compositions() {
    for (left, combined) in [(false, false), (true, false), (false, true)] {
        let mut h = Harness::new();
        h.app.home.open = false;
        let feature = h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"Target","body_name":"Body"})).unwrap()["feature"]
            .as_u64()
            .unwrap();
        h.app.preferences.timeline_left = left;
        h.app.preferences.history_in_browser = combined;
        h.app.ui.show_timeline = false;
        h.app.ui.show_browser = false;
        h.app.tree.timeline_reveal = Some(feature);
        h.frames(3);
        assert_eq!(h.app.tree.timeline_reveal, Some(feature));
        semantic_frame(&mut h, &[ContentOwner::HISTORY], Vec::new(), egui::Modifiers::NONE);
        assert_eq!(h.app.tree.timeline_reveal, Some(feature), "Bodies/Origin is not a feature presentation");
        if combined {
            h.app.ui.show_browser = true;
            h.app.preferences.browser_collapsed = false;
            h.app.tree.collapsed.insert("c0/history".into());
            h.frames(3);
            assert_eq!(h.app.tree.timeline_reveal, Some(feature), "a folded combined History retains the request");
            h.app.tree.collapsed.remove("c0/history");
        } else {
            h.app.ui.show_timeline = true;
        }
        h.frames(8);
        assert!(h.app.tree.timeline_reveal.is_none(), "left={left}, combined={combined}");
    }
}

#[test]
fn native_feature_reveal_waits_for_its_intended_browser_composition() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Body"})).unwrap();
    let id = h.app.session.doc.features[0].id;
    reveal_feature(&mut h.app, id, "Body");
    h.app.preferences.history_in_browser = true;
    h.frames(3);
    assert!(h.app.tree.reveal_feature.is_some(), "combined History does not consume a native Features request");
    h.app.preferences.history_in_browser = false;
    h.frames(4);
    assert!(h.app.tree.reveal_feature.is_none());
}

#[test]
fn eye_capture_disappearance_finishes_one_undo_group_and_does_not_transfer_to_another_owner() {
    for owner in [ContentOwner::BROWSER, ContentOwner::HISTORY] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
        for _ in 0..3 {
            semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
        }
        let first = semantic_control(owner, "eye:One");
        let undo = h.app.session.undo.len();
        semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(first)], egui::Modifiers::NONE);
        semantic_frame(
            &mut h,
            &owners,
            vec![semantic_button(first, egui::PointerButton::Primary, true, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        );
        let middle = semantic_control(owner, "eye:Two");
        for at in [first + vec2(0.0, 7.0), middle] {
            semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(at)], egui::Modifiers::NONE);
        }
        assert!(h.ctx.data(|data| data.get_temp::<bool>(crate::ui_kit::eye_drag_id())).is_some());
        assert!(body_hidden(&h, "One") && body_hidden(&h, "Two") && !body_hidden(&h, "Three"));
        let other = if owner == ContentOwner::BROWSER { ContentOwner::HISTORY } else { ContentOwner::BROWSER };
        let other_eye = semantic_control(other, "eye:Three");
        semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
        assert!(h.ctx.data(|data| data.get_temp::<bool>(crate::ui_kit::eye_drag_id())).is_none());
        assert!(h.app.tree.eye_press.is_none());
        assert_eq!(h.app.session.undo.len(), undo + 1, "disappearance closes the existing drag group");
        semantic_frame(&mut h, &[other], vec![egui::Event::PointerMoved(other_eye)], egui::Modifiers::NONE);
        assert!(!body_hidden(&h, "Three"), "the vanished owner's eye target cannot transfer to another presentation");
        semantic_frame(
            &mut h,
            &owners,
            vec![semantic_button(other_eye, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        );
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(!body_hidden(&h, "One") && !body_hidden(&h, "Two") && !body_hidden(&h, "Three"));
    }
}

#[test]
fn shared_body_origin_rows_keep_their_geometry_labels_and_composition() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    h.app.tree.expanded.insert("origin".into());
    let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let browser = semantic_content::output(ContentOwner::BROWSER);
    let history = semantic_content::output(ContentOwner::HISTORY);
    let labels: Vec<_> = history.rows.iter().map(|(label, ..)| label.as_str()).collect();
    assert_eq!(labels, ["Bodies", "One", "Two", "Three", "Origin", "O", "X", "Y", "Z", "XY", "XZ", "YZ"]);
    for (label, rect, fold, open) in history.rows {
        let (_, browser_rect, browser_fold, browser_open) = browser.rows.iter().find(|(name, ..)| *name == label).unwrap();
        assert_eq!(rect.height(), browser_rect.height(), "{label}: existing shared row height");
        assert_eq!(fold.is_some(), browser_fold.is_some(), "{label}: existing disclosure composition");
        assert_eq!(open, *browser_open, "{label}: canonical fold state");
    }
    assert_eq!(drawn_rows(), browser.rows, "legacy automation remains Browser-only");
}

#[test]
fn semantic_drop_runs_once_in_either_owner_submission_order() {
    for owners in [[ContentOwner::BROWSER, ContentOwner::HISTORY], [ContentOwner::HISTORY, ContentOwner::BROWSER]] {
        for owner in [ContentOwner::BROWSER, ContentOwner::HISTORY] {
            let mut h = Harness::new();
            three_bodies(&mut h);
            let group = h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":"Destination"})).unwrap()["group"].as_u64().unwrap();
            for _ in 0..3 {
                semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
            }
            let from = semantic_at(owner, "One");
            let to = semantic_at(owner, "Destination (0)");
            let undo = h.app.session.undo.len();
            semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(from)], egui::Modifiers::NONE);
            semantic_frame(
                &mut h,
                &owners,
                vec![semantic_button(from, egui::PointerButton::Primary, true, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            for at in [from + vec2(8.0, 0.0), to] {
                semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(at)], egui::Modifiers::NONE);
            }
            semantic_frame(
                &mut h,
                &owners,
                vec![semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            assert!(h.app.session.doc.browser_group(group).unwrap().has_item("One"));
            assert_eq!(h.app.session.undo.len(), undo + 1, "{owners:?}, {owner:?}: exactly one drop command");
            h.app.run("edit.undo", json!({})).unwrap();
            assert!(!h.app.session.doc.browser_group(group).unwrap().has_item("One"));
        }
    }
}

#[test]
fn disappearing_semantic_drag_owner_releases_only_its_payload_and_widget_capture() {
    for owner in [ContentOwner::BROWSER, ContentOwner::HISTORY] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        for _ in 0..3 {
            semantic_frame(&mut h, &[owner], Vec::new(), egui::Modifiers::NONE);
        }
        let from = semantic_at(owner, "One");
        semantic_frame(&mut h, &[owner], vec![egui::Event::PointerMoved(from)], egui::Modifiers::NONE);
        semantic_frame(
            &mut h,
            &[owner],
            vec![semantic_button(from, egui::PointerButton::Primary, true, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        );
        semantic_frame(&mut h, &[owner], vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))], egui::Modifiers::NONE);
        assert!(egui::DragAndDrop::has_payload_of_type::<Drag>(&h.ctx));
        semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
        assert!(!egui::DragAndDrop::has_any_payload(&h.ctx));
        assert!(h.ctx.dragged_id().is_none());
        assert!(semantic_content::output(owner).rows.is_empty());
    }
}

#[test]
fn pending_semantic_reveals_expire_when_their_targets_are_removed() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Body"})).unwrap();
    let feature = h.app.session.doc.features[0].id;
    let other = h
        .app
        .run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"operation":"new_body","body_name":"Other"}))
        .unwrap()["feature"]
        .as_u64()
        .unwrap();
    h.app.run("timeline.suppress", json!({"feature":other})).unwrap();
    reveal_body(&mut h.app, "Body");
    reveal_feature(&mut h.app, feature, "Body");
    h.app.tree.reveal = Some("Body".into());
    h.app.tree.timeline_reveal = Some(feature);
    h.app.run("selection.delete", json!({"items":[Sel::Feature { id: feature }]})).unwrap();
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    assert!(h.app.tree.reveal.is_none() && h.app.tree.reveal_feature.is_none() && h.app.tree.timeline_reveal.is_none());
}

#[test]
fn semantic_row_commands_are_consumed_once_across_layout_repeats() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let at = semantic_control(ContentOwner::HISTORY, "eye:One");
    let undo = h.app.session.undo.len();
    semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(at)], egui::Modifiers::NONE);
    semantic_frame(&mut h, &owners, vec![semantic_button(at, egui::PointerButton::Primary, true, egui::Modifiers::NONE)], egui::Modifiers::NONE);
    h.ctx.data_mut(|data| data.insert_temp(egui::Id::unique("semantic-test-discard"), true));
    semantic_frame(&mut h, &owners, vec![semantic_button(at, egui::PointerButton::Primary, false, egui::Modifiers::NONE)], egui::Modifiers::NONE);
    assert!(body_hidden(&h, "One"), "a repeated layout cannot toggle the eye back on");
    assert_eq!(h.app.session.undo.len(), undo + 1);
}

#[test]
fn invisible_browser_content_keeps_its_reveal_and_publishes_no_geometry() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    reveal_body(&mut h.app, "Three");
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 900.0))), ..Default::default() };
    h.ctx
        .run_ui(raw, |ui| {
            semantic_content::begin(&mut h.app, ui.ctx());
            ui.set_opacity(0.0);
            semantic_content::show(&mut h.app, ui, ContentOwner::BROWSER, ContentRequest::Browser { shrink_height: false });
            semantic_content::finalize(&mut h.app, ui.ctx());
        })
        .textures_delta
        .clear();
    assert_eq!(h.app.tree.reveal.as_deref(), Some("Three"));
    assert!(drawn_rows().is_empty() && drawn_occurrences().is_empty());
}

#[test]
fn combined_history_reveal_opens_and_acknowledges_the_features_placement() {
    let (mut h, _, occurrence) = crate::test_support::component_box();
    let feature = h.app.session.model.state().body("Body1").unwrap().feature;
    h.app.preferences.history_in_browser = true;
    h.app.tree.close_row(&occurrence_key(&[occurrence]));
    reveal_in_history(&mut h.app, feature);
    h.app.tree.timeline_reveal = Some(feature);
    h.frames(6);
    assert!(is_open(&h.app, &occurrence_key(&[occurrence]), true));
    assert!(h.app.tree.reveal_feature.is_none() && h.app.tree.timeline_reveal.is_none());
    let name = &h.app.session.doc.feature(feature).unwrap().name;
    assert!(drawn_rows().iter().any(|(label, ..)| label == name));
}

#[test]
fn browser_editors_have_one_app_owned_host_when_both_presentations_are_hidden() {
    let (mut h, _, occurrence) = crate::test_support::component_box();
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    h.app.tree.occurrence_move = Some(OccMove::new(occurrence));
    h.frames(4);
    assert!(drawn_rows().is_empty());
    let fields = crate::scenario::handles("occurrence-move:distance:0");
    assert_eq!(
        fields.iter().filter(|(name, _)| name == "occurrence-move:distance:0").count(),
        1,
        "the editor is hosted once despite both panels being hidden"
    );
    assert!(h.app.tree.occurrence_move.as_ref().unwrap().baseline.is_some());
}

#[test]
fn body_reveal_waits_through_rollback_until_its_creator_is_presented_again() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    reveal_body(&mut h.app, "Three");
    h.app.run("timeline.roll_to", json!({"position":1})).unwrap();
    for _ in 0..3 {
        semantic_frame(&mut h, &[ContentOwner::HISTORY, ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
        assert_eq!(h.app.tree.reveal.as_deref(), Some("Three"), "rollback is not removal");
    }
    h.app.run("timeline.roll_to", json!({})).unwrap();
    for _ in 0..4 {
        semantic_frame(&mut h, &[ContentOwner::HISTORY, ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
    }
    assert!(h.app.tree.reveal.is_none());
}

/// Make an actual second document tab with identical semantic names and IDs. Return to A;
/// tests can switch during an input frame without rebuilding or rendering B first.
fn matching_semantic_tab(h: &mut Harness) -> (usize, String, usize) {
    let original = h.app.docs.active;
    let incoming = solidcraft_engine::Session::new((*h.app.session.doc).clone());
    let original_session = h.app.session.uid;
    crate::documents::open_tab(&mut h.app, incoming).unwrap();
    assert_ne!(h.app.session.uid, original_session);
    let tab = h.app.docs.active;
    let document = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    crate::documents::switch(&mut h.app, original);
    (tab, document, undo)
}

fn semantic_drag_start(h: &mut Harness, owners: &[ContentOwner], from: Pos2) {
    semantic_frame(h, owners, vec![egui::Event::PointerMoved(from)], egui::Modifiers::NONE);
    semantic_frame(h, owners, vec![semantic_button(from, egui::PointerButton::Primary, true, egui::Modifiers::NONE)], egui::Modifiers::NONE);
    semantic_frame(h, owners, vec![egui::Event::PointerMoved(from + vec2(10.0, 0.0))], egui::Modifiers::NONE);
}

fn semantic_occurrence_at(owner: ContentOwner, occurrence: u64) -> Pos2 {
    semantic_content::output(owner).occurrences.into_iter().find(|(path, ..)| path.last() == Some(&occurrence)).unwrap().2.center()
}

#[test]
fn switching_documents_cancels_body_payloads_before_matching_incoming_groups_can_drop() {
    for after_begin in [false, true] {
        for owner in [ContentOwner::BROWSER, ContentOwner::HISTORY] {
            let mut h = Harness::new();
            three_bodies(&mut h);
            h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":"Destination"})).unwrap();
            let (tab, document, undo) = matching_semantic_tab(&mut h);
            let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
            for _ in 0..3 {
                semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
            }
            let from = semantic_at(owner, "One");
            let to = semantic_at(owner, "Destination (0)");
            semantic_drag_start(&mut h, &owners, from);
            let outgoing = h.app.session.uid;
            let payload = egui::DragAndDrop::payload::<Drag>(&h.ctx).unwrap();
            assert_eq!(payload.source.session, outgoing);
            let stale_payload = (*payload).clone();
            if !after_begin {
                crate::documents::switch(&mut h.app, tab);
            }
            semantic_frame_switch(
                &mut h,
                &owners,
                vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
                after_begin.then_some(tab),
            );
            assert_ne!(h.app.session.uid, outgoing);
            assert_eq!(h.app.session.doc.to_json(), document);
            assert_eq!(h.app.session.undo.len(), undo);
            assert!(!egui::DragAndDrop::has_payload_of_type::<Drag>(&h.ctx));
            assert!(h.ctx.dragged_id().is_none());
            // Even a delayed payload reintroduced after cancellation must fail provenance
            // validation; matching body/group names cannot make it belong to the new tab.
            egui::DragAndDrop::set_payload(&h.ctx, stale_payload);
            semantic_frame(
                &mut h,
                &owners,
                vec![semantic_button(to, egui::PointerButton::Primary, true, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            semantic_frame(
                &mut h,
                &owners,
                vec![semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            assert_eq!(h.app.session.doc.to_json(), document);
            assert_eq!(h.app.session.undo.len(), undo);
        }
    }
}

#[test]
fn switching_documents_cancels_occurrence_payloads_with_matching_component_and_path_ids() {
    for after_begin in [false, true] {
        let (mut h, _, occurrence) = crate::test_support::component_box();
        let destination = h.app.run("component.create", json!({"name":"Destination"})).unwrap()["occurrence"].as_u64().unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        let (tab, document, undo) = matching_semantic_tab(&mut h);
        let owners = [ContentOwner::BROWSER];
        for _ in 0..3 {
            semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
        }
        let from = semantic_occurrence_at(ContentOwner::BROWSER, occurrence);
        let to = semantic_occurrence_at(ContentOwner::BROWSER, destination);
        semantic_drag_start(&mut h, &owners, from);
        assert!(egui::DragAndDrop::has_payload_of_type::<OccurrenceDrag>(&h.ctx));
        if !after_begin {
            crate::documents::switch(&mut h.app, tab);
        }
        semantic_frame_switch(
            &mut h,
            &owners,
            vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            after_begin.then_some(tab),
        );
        assert_eq!(h.app.session.doc.to_json(), document);
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(!egui::DragAndDrop::has_payload_of_type::<OccurrenceDrag>(&h.ctx));
        assert!(h.ctx.dragged_id().is_none());
    }
}

#[test]
fn switching_documents_clears_combined_history_drag_even_when_feature_ids_match() {
    for after_begin in [false, true] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        h.app.preferences.history_in_browser = true;
        let source = h.app.session.doc.features[2].id;
        let target = h.app.session.doc.features[0].id;
        let (tab, document, undo) = matching_semantic_tab(&mut h);
        let owners = [ContentOwner::BROWSER];
        for _ in 0..3 {
            semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
        }
        let from = semantic_control(ContentOwner::BROWSER, &format!("row:browser-feature:c0:{source}"));
        let to = semantic_control(ContentOwner::BROWSER, &format!("row:browser-feature:c0:{target}"));
        semantic_drag_start(&mut h, &owners, from);
        assert!(!h.app.tree.content.owner(ContentOwner::BROWSER).unwrap().feature_drags.is_empty());
        if !after_begin {
            crate::documents::switch(&mut h.app, tab);
        }
        semantic_frame_switch(
            &mut h,
            &owners,
            vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            after_begin.then_some(tab),
        );
        assert_eq!(h.app.session.doc.to_json(), document);
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.tree.content.owner(ContentOwner::BROWSER).unwrap().feature_drags.is_empty());
        assert!(h.ctx.dragged_id().is_none());
    }
}

#[test]
fn document_switch_drops_shift_anchors_without_changing_the_incoming_document_or_undo() {
    for owner in [ContentOwner::BROWSER, ContentOwner::HISTORY] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        let (tab, document, undo) = matching_semantic_tab(&mut h);
        let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
        for _ in 0..3 {
            semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
        }
        let at = semantic_at(owner, "One");
        semantic_click(&mut h, &owners, at, egui::Modifiers::NONE);
        assert!(h.app.tree.content.owner(owner).unwrap().anchor.is_some());
        semantic_frame_switch(&mut h, &owners, Vec::new(), egui::Modifiers::NONE, Some(tab));
        assert!(h.app.tree.content.owner(owner).unwrap().anchor.is_none());
        let at = semantic_at(owner, "Two");
        semantic_click(&mut h, &owners, at, egui::Modifiers { shift: true, ..Default::default() });
        assert_eq!(h.app.session.selection, [Sel::Body { name: "Two".into() }], "A's One anchor cannot extend B's selection");
        assert_eq!(h.app.session.doc.to_json(), document);
        assert_eq!(h.app.session.undo.len(), undo);
    }
}

#[test]
fn switching_documents_finishes_eye_undo_group_in_the_outgoing_session_only() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let original = h.app.docs.active;
    let (tab, document, undo_b) = matching_semantic_tab(&mut h);
    let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let undo_a = h.app.session.undo.len();
    let one = semantic_control(ContentOwner::BROWSER, "eye:One");
    let two = semantic_control(ContentOwner::BROWSER, "eye:Two");
    semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(one)], egui::Modifiers::NONE);
    semantic_frame(&mut h, &owners, vec![semantic_button(one, egui::PointerButton::Primary, true, egui::Modifiers::NONE)], egui::Modifiers::NONE);
    for point in [one + vec2(0.0, 7.0), two] {
        semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(point)], egui::Modifiers::NONE);
    }
    assert!(body_hidden(&h, "One") && body_hidden(&h, "Two"));
    semantic_frame_switch(&mut h, &owners, Vec::new(), egui::Modifiers::NONE, Some(tab));
    let three = semantic_control(ContentOwner::BROWSER, "eye:Three");
    semantic_frame(&mut h, &owners, vec![egui::Event::PointerMoved(three)], egui::Modifiers::NONE);
    semantic_frame(&mut h, &owners, vec![semantic_button(three, egui::PointerButton::Primary, false, egui::Modifiers::NONE)], egui::Modifiers::NONE);
    assert_eq!(h.app.session.doc.to_json(), document);
    assert_eq!(h.app.session.undo.len(), undo_b);
    assert!(!body_hidden(&h, "One") && !body_hidden(&h, "Two") && !body_hidden(&h, "Three"));
    crate::documents::switch(&mut h.app, original);
    assert_eq!(h.app.session.undo.len(), undo_a + 1, "only the outgoing session receives the merged drag undo group");
    h.app.run("edit.undo", json!({})).unwrap();
    assert!(!body_hidden(&h, "One") && !body_hidden(&h, "Two"));
}

#[test]
fn switching_semantic_documents_preserves_unrelated_payload_and_capture() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let (tab, _, _) = matching_semantic_tab(&mut h);
    let owners = [ContentOwner::BROWSER];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let from = semantic_at(ContentOwner::BROWSER, "One");
    semantic_drag_start(&mut h, &owners, from);
    let external = egui::Id::unique("external capture");
    egui::DragAndDrop::set_payload(&h.ctx, "external payload".to_string());
    h.ctx.set_dragged_id(external);
    crate::documents::switch(&mut h.app, tab);
    assert_eq!(egui::DragAndDrop::payload::<String>(&h.ctx).as_deref().map(String::as_str), Some("external payload"));
    assert_eq!(h.ctx.dragged_id(), Some(external));
}

#[test]
fn hidden_body_or_group_drag_source_cannot_drop_through_a_surviving_history_target() {
    for group_source in [false, true] {
        for keep_source in [false, true] {
            let mut h = Harness::new();
            three_bodies(&mut h);
            if group_source {
                h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":"Source","items":["One"]})).unwrap();
            }
            let destination =
                h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":"Destination"})).unwrap()["group"].as_u64().unwrap();
            let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
            for _ in 0..3 {
                semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
            }
            let from = semantic_at(ContentOwner::BROWSER, if group_source { "Source (1)" } else { "One" });
            let to = semantic_at(ContentOwner::HISTORY, "Destination (0)");
            let document = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            semantic_drag_start(&mut h, &owners, from);
            assert!(egui::DragAndDrop::has_payload_of_type::<Drag>(&h.ctx));
            let presented = if keep_source { &owners[..] } else { &[ContentOwner::HISTORY][..] };
            semantic_frame(
                &mut h,
                presented,
                vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            if keep_source {
                assert_ne!(h.app.session.doc.to_json(), document, "cross-owner drops remain valid while both owners are presented");
                assert_eq!(h.app.session.undo.len(), undo + 1);
                if !group_source {
                    assert!(h.app.session.doc.browser_group(destination).unwrap().has_item("One"));
                }
            } else {
                assert_eq!(h.app.session.doc.to_json(), document);
                assert_eq!(h.app.session.undo.len(), undo);
            }
            assert!(!egui::DragAndDrop::has_payload_of_type::<Drag>(&h.ctx));
            assert!(h.ctx.dragged_id().is_none());
        }
    }
}

#[test]
fn hidden_occurrence_source_cannot_reparent_or_reorder_through_a_surviving_adapter() {
    for reorder in [false, true] {
        for keep_source in [false, true] {
            let (mut h, _, occurrence) = crate::test_support::component_box();
            let destination = h.app.run("component.create", json!({"name":"Destination"})).unwrap()["occurrence"].as_u64().unwrap();
            h.app.run("component.activate", json!({"component":0})).unwrap();
            let adapter = ContentOwner(2);
            let owners = [ContentOwner::BROWSER, adapter];
            for _ in 0..3 {
                semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
            }
            // Move the later placement before the earlier one for a real reorder change.
            let (source, target) = if reorder { (destination, occurrence) } else { (occurrence, destination) };
            let from = semantic_occurrence_at(ContentOwner::BROWSER, source);
            let mut to = semantic_occurrence_at(adapter, target);
            if reorder {
                to.y -= crate::ui_kit::TREE_ROW_HEIGHT * 0.4;
            }
            let document = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            semantic_drag_start(&mut h, &owners, from);
            assert!(egui::DragAndDrop::has_payload_of_type::<OccurrenceDrag>(&h.ctx));
            let presented = if keep_source { &owners[..] } else { std::slice::from_ref(&adapter) };
            semantic_frame(
                &mut h,
                presented,
                vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            if keep_source {
                assert_ne!(h.app.session.doc.to_json(), document);
                assert_eq!(h.app.session.undo.len(), undo + 1);
            } else {
                assert_eq!(h.app.session.doc.to_json(), document);
                assert_eq!(h.app.session.undo.len(), undo);
            }
            assert!(!egui::DragAndDrop::has_payload_of_type::<OccurrenceDrag>(&h.ctx));
            assert!(h.ctx.dragged_id().is_none());
        }
    }
}

#[test]
fn session_switch_after_target_rows_render_discards_their_already_queued_drop() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":"Destination"})).unwrap();
    let (tab, document, undo) = matching_semantic_tab(&mut h);
    let owners = [ContentOwner::BROWSER, ContentOwner::HISTORY];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let from = semantic_at(ContentOwner::BROWSER, "One");
    let to = semantic_at(ContentOwner::HISTORY, "Destination (0)");
    semantic_drag_start(&mut h, &owners, from);
    semantic_frame_transitions(
        &mut h,
        &owners,
        vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
        None,
        Some(tab),
    );
    assert_eq!(h.app.session.doc.to_json(), document);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(semantic_content::output(ContentOwner::BROWSER).rows.is_empty());
    assert!(semantic_content::output(ContentOwner::HISTORY).rows.is_empty());
}

#[test]
fn semantic_entrypoints_reject_replaced_sessions_even_without_a_documents_hook() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    h.app.run("browser.group", json!({"component":0,"folder":"bodies","name":"Destination"})).unwrap();
    let incoming = solidcraft_engine::Session::new((*h.app.session.doc).clone());
    let document = incoming.doc.to_json();
    let undo = incoming.undo.len();
    let owners = [ContentOwner::BROWSER];
    for _ in 0..3 {
        semantic_frame(&mut h, &owners, Vec::new(), egui::Modifiers::NONE);
    }
    let from = semantic_at(ContentOwner::BROWSER, "One");
    semantic_click(&mut h, &owners, from, egui::Modifiers::NONE);
    let to = semantic_at(ContentOwner::BROWSER, "Destination (0)");
    semantic_drag_start(&mut h, &owners, from);
    assert!(egui::DragAndDrop::has_payload_of_type::<Drag>(&h.ctx));
    h.app.session.replace_design(incoming);
    semantic_frame(
        &mut h,
        &owners,
        vec![egui::Event::PointerMoved(to), semantic_button(to, egui::PointerButton::Primary, false, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert_eq!(h.app.session.doc.to_json(), document);
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.tree.content.owner(ContentOwner::BROWSER).unwrap().anchor.is_none());
    assert!(!egui::DragAndDrop::has_payload_of_type::<Drag>(&h.ctx));
    assert!(h.ctx.dragged_id().is_none());
}

fn reveal_browser_at_height(h: &mut Harness, height: f32) {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))), ..Default::default() };
    h.ctx
        .run_ui(raw, |ui| {
            semantic_content::begin(&mut h.app, ui.ctx());
            let mut content = ui.new_child(
                egui::UiBuilder::new().id_salt("reveal-document-test").max_rect(Rect::from_min_size(pos2(10.0, 10.0), vec2(400.0, height))),
            );
            semantic_content::show(&mut h.app, &mut content, ContentOwner::BROWSER, ContentRequest::Browser { shrink_height: false });
            semantic_content::finalize(&mut h.app, ui.ctx());
        })
        .textures_delta
        .clear();
}

fn assert_no_pending_reveals(h: &Harness) {
    assert!(h.app.tree.reveal.is_none());
    assert!(h.app.tree.reveal_source.is_none());
    assert!(h.app.tree.reveal_feature.is_none());
    assert!(h.app.tree.timeline_reveal.is_none());
    assert!(h.app.tree.view_pick.is_none());
}

fn queue_all_document_reveals(h: &mut Harness) -> u64 {
    let feature = h.app.session.model.state().body("Three").unwrap().feature;
    reveal_body(&mut h.app, "Three");
    reveal_feature(&mut h.app, feature, "Three");
    // Keep both independently pending presentations, with the body's real provenance from
    // reveal_body. Public request fields also remain supported for existing adapters.
    h.app.tree.reveal = Some("Three".into());
    request_timeline_reveal(&mut h.app, feature);
    h.app.tree.view_pick = Some(feature);
    h.app.ui.show_browser = false;
    h.app.ui.show_timeline = false;
    feature
}

#[test]
fn body_reveals_park_with_each_document_and_cannot_scroll_a_matching_incoming_browser() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let original = h.app.docs.active;
    let original_uid = h.app.session.uid;
    let (tab, _, _) = matching_semantic_tab(&mut h);
    crate::documents::switch(&mut h.app, tab);
    for _ in 0..3 {
        reveal_browser_at_height(&mut h, 120.0);
    }
    let root = h.app.session.doc.name.clone();
    let incoming_root = semantic_at(ContentOwner::BROWSER, &root);
    assert!(!drawn_rows().iter().any(|(label, ..)| label == "Three"), "the target starts outside the scroll viewport");
    crate::documents::switch(&mut h.app, original);
    reveal_body(&mut h.app, "Three");
    h.app.ui.show_browser = false;
    h.app.tree.view_pick = Some(h.app.session.doc.features[2].id);
    semantic_frame_transitions(&mut h, &[ContentOwner::HISTORY], Vec::new(), egui::Modifiers::NONE, None, Some(tab));
    assert_no_pending_reveals(&h);
    for _ in 0..8 {
        reveal_browser_at_height(&mut h, 120.0);
    }
    assert_eq!(semantic_at(ContentOwner::BROWSER, &root), incoming_root, "B must not scroll in response to A's same-named body");
    reveal_body(&mut h.app, "Two");
    h.app.ui.show_browser = false;
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    assert_eq!(h.app.tree.reveal.as_deref(), Some("Two"));
    crate::documents::switch(&mut h.app, original);
    assert_eq!(h.app.tree.reveal.as_deref(), Some("Three"));
    assert_eq!(h.app.tree.reveal_source.as_ref().unwrap().session, original_uid);
    assert!(h.app.tree.view_pick.is_none());
    for _ in 0..30 {
        reveal_browser_at_height(&mut h, 120.0);
    }
    assert!(h.app.tree.reveal.is_none(), "A acknowledges its restored target only after scrolling presents it");
    crate::documents::switch(&mut h.app, tab);
    assert_eq!(h.app.tree.reveal.as_deref(), Some("Two"), "B's independent request survives A's acknowledgment");
    for _ in 0..30 {
        reveal_browser_at_height(&mut h, 120.0);
    }
    assert!(h.app.tree.reveal.is_none());
}

#[test]
fn native_feature_reveals_restore_only_in_their_document_even_when_feature_ids_match() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let original = h.app.docs.active;
    let target_a = h.app.session.doc.features[2].id;
    let target_b = h.app.session.doc.features[0].id;
    let (tab, _, _) = matching_semantic_tab(&mut h);
    reveal_feature(&mut h.app, target_a, "Three");
    h.app.ui.show_browser = false;
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    semantic_frame_switch(&mut h, &[ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE, Some(tab));
    assert_no_pending_reveals(&h);
    assert!(h.app.session.doc.feature(target_a).is_some(), "matching numeric IDs do exist in B");
    reveal_feature(&mut h.app, target_b, "One");
    h.app.ui.show_browser = false;
    crate::documents::switch(&mut h.app, original);
    assert_eq!(h.app.tree.reveal_feature.as_ref().map(|target| target.id), Some(target_a));
    for _ in 0..4 {
        semantic_frame(&mut h, &[ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
    }
    assert!(h.app.tree.reveal_feature.is_none());
    crate::documents::switch(&mut h.app, tab);
    assert_eq!(h.app.tree.reveal_feature.as_ref().map(|target| target.id), Some(target_b));
    for _ in 0..4 {
        semantic_frame(&mut h, &[ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
    }
    assert!(h.app.tree.reveal_feature.is_none());
}

#[test]
fn timeline_reveals_remain_per_document_in_bottom_vertical_and_combined_history() {
    for (left, combined) in [(false, false), (true, false), (false, true)] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        let original = h.app.docs.active;
        let target_a = h.app.session.doc.features[2].id;
        let target_b = h.app.session.doc.features[0].id;
        let (tab, _, _) = matching_semantic_tab(&mut h);
        h.app.preferences.timeline_left = left;
        h.app.preferences.history_in_browser = combined;
        if combined {
            reveal_in_history(&mut h.app, target_a);
        }
        request_timeline_reveal(&mut h.app, target_a);
        h.app.tree.view_pick = Some(target_a);
        h.app.ui.show_timeline = false;
        h.app.ui.show_browser = false;
        h.frames(2);
        semantic_frame_switch(&mut h, &[], Vec::new(), egui::Modifiers::NONE, Some(tab));
        assert_no_pending_reveals(&h);
        h.app.ui.show_timeline = true;
        h.app.ui.show_browser = combined;
        h.frames(4);
        assert!(h.app.tree.timeline_reveal.is_none());
        request_timeline_reveal(&mut h.app, target_b);
        h.app.ui.show_timeline = false;
        h.app.ui.show_browser = false;
        h.frames(2);
        semantic_frame_switch(&mut h, &[], Vec::new(), egui::Modifiers::NONE, Some(original));
        assert_eq!(h.app.tree.timeline_reveal, Some(target_a));
        assert!(h.app.tree.view_pick.is_none());
        if combined {
            assert_eq!(h.app.tree.reveal_feature.as_ref().map(|target| target.id), Some(target_a));
        }
        h.app.ui.show_timeline = true;
        h.app.ui.show_browser = combined;
        h.frames(8);
        assert!(h.app.tree.timeline_reveal.is_none(), "left={left}, combined={combined}: A acknowledges its own request");
        assert!(h.app.tree.reveal_feature.is_none());
        crate::documents::switch(&mut h.app, tab);
        assert_eq!(h.app.tree.timeline_reveal, Some(target_b));
        h.frames(8);
        assert!(h.app.tree.timeline_reveal.is_none(), "B acknowledges its independent request");
    }
}

#[test]
fn closing_a_document_destroys_its_parked_reveals_and_clears_stale_view_pick() {
    for active in [false, true] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        let original = h.app.docs.active;
        let (tab, _, _) = matching_semantic_tab(&mut h);
        queue_all_document_reveals(&mut h);
        if !active {
            crate::documents::switch(&mut h.app, tab);
        }
        crate::documents::close(&mut h.app, original, true);
        assert_eq!(h.app.docs.count(), 1);
        assert_no_pending_reveals(&h);
        let replacement = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        crate::documents::open_tab(&mut h.app, replacement).unwrap();
        assert_no_pending_reveals(&h);
        for _ in 0..3 {
            semantic_frame(&mut h, &[ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
        }
        assert_no_pending_reveals(&h);
    }
}

#[test]
fn last_close_blank_adoption_and_direct_replacement_discard_outgoing_reveal_requests() {
    for replacement in ["last-close", "blank-adopt", "direct"] {
        let mut h = Harness::new();
        three_bodies(&mut h);
        let incoming = solidcraft_engine::Session::new((*h.app.session.doc).clone());
        queue_all_document_reveals(&mut h);
        match replacement {
            "last-close" => {
                let original = h.app.docs.active;
                crate::documents::close(&mut h.app, original, true);
                assert_no_pending_reveals(&h);
                crate::documents::adopt(&mut h.app, incoming);
                assert_no_pending_reveals(&h);
            }
            "blank-adopt" => {
                h.app.session.replace_design(solidcraft_engine::Session::default());
                crate::documents::adopt(&mut h.app, incoming);
                assert_no_pending_reveals(&h);
            }
            _ => h.app.session.replace_design(incoming),
        }
        for _ in 0..3 {
            semantic_frame(&mut h, &[ContentOwner::BROWSER], Vec::new(), egui::Modifiers::NONE);
        }
        assert_no_pending_reveals(&h);
        assert!(h.app.session.model.state().body("Three").is_some(), "the incoming target still exists; matching names cannot preserve A's request");
    }
}

#[test]
fn unpresented_app_requests_have_provenance_before_a_direct_session_replacement() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let document = (*h.app.session.doc).clone();
    h.app = crate::SolveApp::new(solidcraft_engine::Session::new(document.clone()), crate::Services::default());
    // Existing callers can set the public request fields before the first UI frame.
    h.app.tree.reveal = Some("Three".into());
    h.app.tree.timeline_reveal = Some(document.features[2].id);
    h.app.tree.view_pick = Some(document.features[2].id);
    h.app.session.replace_design(solidcraft_engine::Session::new(document));
    semantic_frame(&mut h, &[ContentOwner::HISTORY], Vec::new(), egui::Modifiers::NONE);
    assert_no_pending_reveals(&h);
}

#[test]
fn parked_reveals_are_pruned_against_their_own_document_when_the_target_is_removed() {
    let mut h = Harness::new();
    three_bodies(&mut h);
    let original = h.app.docs.active;
    let owner = h.app.session.uid;
    let (tab, _, _) = matching_semantic_tab(&mut h);
    let feature = queue_all_document_reveals(&mut h);
    crate::documents::switch(&mut h.app, tab);
    let parked = h.app.docs.background_mut().find(|slot| slot.session.uid == owner).unwrap();
    parked.session.execute("selection.delete", &json!({"items":[Sel::Feature { id: feature }]})).unwrap();
    assert_no_pending_reveals(&h);
    assert!(h.app.session.doc.feature(feature).is_some(), "B retains the matching feature");
    crate::documents::switch(&mut h.app, original);
    assert_eq!(h.app.tree.timeline_reveal, Some(feature), "the request returns to A before ordinary target pruning");
    semantic_frame(&mut h, &[], Vec::new(), egui::Modifiers::NONE);
    assert_no_pending_reveals(&h);
}

/// Stage 2: every sketch row folds its contents open in the browser. Geometry groups start open,
/// dimensions start closed, and collapsing the sketch hides the whole subtree again.
#[test]
fn expanding_a_sketch_shows_its_contents_groups_and_geometry_rows() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Contents"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"ids":["l1"]})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[30,0],"radius":5})).unwrap();
    h.app.run("sketch.dimension", json!({"entities":["l1"],"value":20,"text_at":[10,8]})).unwrap();
    let dimension = h.app.session.doc.sketch(sketch).unwrap().constraints.iter().find(|c| c.kind.is_dimension()).map(|c| c.id.clone()).unwrap();
    h.app.finish_sketch();
    h.settle();

    let scope = format!("s:{sketch}");
    let group = |slug: &str| format!("browser:sketch-contents-group:{scope}:{slug}");
    let fold = |slug: &str| format!("browser:sketch-contents-fold:{scope}:{slug}");
    assert!(handle_at(&group("lines")).is_none(), "a sketch starts folded");

    h.step(&json!({"click":{"handle":"fold:Contents"}})).unwrap();
    h.settle();
    for slug in ["points", "lines", "circles", "dimensions"] {
        assert!(handle_at(&group(slug)).is_some(), "missing {slug}: {:?}", crate::scenario::handles("browser:sketch-contents-group"));
    }
    assert!(handle_at(&format!("browser:sketch-contents-row:{scope}:lines:l1")).is_some(), "the line row shows with its lines group");
    assert!(!crate::scenario::handles(&format!("browser:sketch-contents-row:{scope}:circles:")).is_empty(), "the circle row shows");
    assert!(crate::scenario::handles(&format!("browser:sketch-contents-row:{scope}:dimensions:")).is_empty(), "dimensions start closed");
    // What the rows say, not just what they are called internally.
    h.frame();
    let mut texts = Vec::new();
    for clipped in &h.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |shape| {
            if let egui::Shape::Text(text) = shape {
                texts.push(text.galley.text().to_string());
            }
        });
    }
    for want in ["Points", "Lines", "Circles", "Dimensions", "Line l1"] {
        assert!(texts.iter().any(|text| text == want), "no `{want}` in {texts:?}");
    }

    h.step(&json!({"click":{"handle":fold("dimensions")}})).unwrap();
    h.settle();
    assert!(
        handle_at(&format!("browser:sketch-contents-row:{scope}:dimensions:{dimension}")).is_some(),
        "the dimension row appears: {:?}",
        crate::scenario::handles("browser:sketch-contents-row")
    );

    // The contents are built once and kept across frames, whatever else the browser draws.
    let builds = h.app.tree.cache.as_ref().unwrap().builds;
    h.frames(6);
    assert_eq!(h.app.tree.cache.as_ref().unwrap().builds, builds, "contents are not rebuilt every frame");

    h.step(&json!({"click":{"handle":"fold:Contents"}})).unwrap();
    h.settle();
    assert!(handle_at(&group("lines")).is_none(), "folding the sketch hides its contents again");
}

/// A sketch a feature has consumed keeps its row (and its fold arrow), and its contents still
/// expand from the solved state.
#[test]
fn a_consumed_sketch_still_expands_to_its_contents() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Consumed"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
    h.app.finish_sketch();
    h.app.run("solid.extrude", json!({"sketch":"Consumed","distance":5})).unwrap();
    h.settle();
    assert!(!crate::browser::sketch_visible(&h.app, sketch), "the extrude consumes the sketch's own display");

    h.step(&json!({"click":{"handle":"fold:Consumed"}})).unwrap();
    h.settle();
    let scope = format!("s:{sketch}");
    assert!(handle_at(&format!("browser:sketch-contents-group:{scope}:lines")).is_some(), "the consumed sketch folds its contents open");
    let lines = crate::scenario::handles(&format!("browser:sketch-contents-row:{scope}:lines:"));
    assert_eq!(lines.len(), 4, "the rectangle's four lines are listed: {lines:?}");
}

/// Large groups are capped: the browser draws a group's first rows and a Show all row instead of
/// walking thousands of rows every frame.
#[test]
fn a_large_sketch_caps_its_groups_until_show_all() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Large"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    for i in 0..400 {
        let x = i as f64 * 10.0;
        h.app.run("sketch.line", json!({"points":[[x,0],[x,5]]})).unwrap();
    }
    h.app.finish_sketch();
    h.settle();
    h.step(&json!({"click":{"handle":"fold:Large"}})).unwrap();
    h.settle();

    let scope = format!("s:{sketch}");
    let shown = |slug: &str| crate::scenario::count_of(&format!("browser:sketch-contents-shown:{scope}:{slug}"));
    assert_eq!(shown("lines"), Some(200.0), "the lines group is capped");
    assert_eq!(shown("points"), Some(200.0), "the points group is capped");

    h.app.tree.expanded.insert(format!("s:{sketch}/contents/lines/all"));
    h.settle();
    assert_eq!(shown("lines"), Some(400.0), "Show all lists the whole group");
}

/// Patterns keep their row and editor when the sketch's contents are open: they move into the
/// Patterns group instead of being listed twice beside it.
#[test]
fn expanding_contents_keeps_pattern_rows_in_their_group() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.app.run("sketch.create", json!({"plane":"XY","name":"Patterned"})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.run("sketch.line", json!({"points":[[10,5],[15,5]],"ids":["source"]})).unwrap();
    let pattern =
        h.app.run("sketch.pattern.rectangular", json!({"entities":["source"],"count":4,"spacing":20,"alignment":"center","reversed":true})).unwrap()
            ["pattern"]
            .as_str()
            .unwrap()
            .to_string();
    h.app.finish_sketch();
    h.settle();
    let handle = format!("browser:sketch-pattern:{sketch}:{pattern}");
    assert!(handle_at(&handle).is_some(), "a folded sketch keeps its quick pattern list");

    h.step(&json!({"click":{"handle":"fold:Patterned"}})).unwrap();
    h.settle();
    assert!(handle_at(&handle).is_none(), "contents replace the quick list without repeating its rows");
    h.step(&json!({"click":{"handle":format!("browser:sketch-contents-fold:s:{sketch}:patterns")}})).unwrap();
    h.settle();
    assert!(handle_at(&handle).is_some(), "the Patterns group lists the pattern row");
}
