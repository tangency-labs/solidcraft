//! Runtime icon discovery: use the same headless UI and requests as control clients.
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use solidcraft_ui_egui::{icon_svg, scenario::Harness, workspace};

fn click(h: &mut Harness, handle: &str) {
    h.step(&json!({"click":{"handle":handle}})).unwrap_or_else(|e| panic!("{handle}: {e}"));
}

fn escape(h: &mut Harness) {
    h.step(&json!({"key":"Escape"})).unwrap();
    h.call("ui.menu", json!({"close":true}));
    h.app.params = None;
    h.app.design_properties = None;
}

fn collect(h: &mut Harness, surface: &str, requested: &mut BTreeMap<String, bool>) {
    let reply = h.call("ui.icon_audit", json!({}));
    use std::io::Write as _;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/icon-audit");
    std::fs::create_dir_all(&dir).unwrap();
    let mut evidence = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("surfaces.txt")).unwrap();
    writeln!(
        evidence,
        "{surface}\tpanel={:?}\ttool={:?}\t{}",
        h.app.dialog.as_ref().map(|d| &d.kind),
        h.app.tool.as_ref().map(|t| t.cmd),
        reply["result"]["requested"]
    )
    .unwrap();
    for item in reply["result"]["requested"].as_array().expect("audit requests") {
        let name = item["name"].as_str().unwrap();
        let placeholder = item["placeholder"].as_bool().unwrap();
        if placeholder && !requested.contains_key(name) {
            eprintln!("{surface}: {name} fell back to the placeholder");
        }
        requested.insert(name.to_owned(), placeholder);
    }
}

fn menu(h: &mut Harness, target: Value, surface: &str, requested: &mut BTreeMap<String, bool>) {
    // Open on empty canvas so the picker cannot replace the prepared face/edge/sketch
    // selection with whatever happens to lie under the menu's anchor.
    let selection = h.app.session.selection.clone();
    let viewport = target["type"] == "viewport";
    let rect = h.app.viewport.rect.expect("a viewport");
    let reply = h.call("ui.menu", json!({"x":rect.right()-8.0,"y":rect.bottom()-8.0,"target":target}));
    assert_eq!(reply["ok"], true, "{surface}: {reply}");
    h.frames(3);
    let reply = h.call("ui.menu", json!({}));
    assert_eq!(reply["result"]["open"], true, "{surface}: {reply}");
    if viewport {
        assert_eq!(h.app.session.selection, selection, "{surface}: menu anchor replaced the prepared selection");
    }
    assert!(!reply["result"]["items"].as_array().unwrap().is_empty(), "{surface}: empty menu did not exercise its target");
    for item in reply["result"]["items"].as_array().unwrap().iter().chain(reply["result"]["radial"].as_array().unwrap()) {
        let id = item["id"].as_str().unwrap();
        let blank = item["icon"].as_str().unwrap().is_empty();
        // Planned workspace rows deliberately have no artwork until their workspace exists.
        let intentional = id == "-" || id.starts_with('#') || (id == "ui.workspace" && item["shortcut"] == "coming later");
        assert!(!blank || intentional, "{surface}: icon-less menu item {item}");
        assert_eq!(item["placeholder"], false, "{surface}: {item}");
        if let Some(children) = item["children"].as_array() {
            for child in children {
                let id = child["id"].as_str().unwrap();
                assert!(!id.is_empty(), "{surface}: child has no identity");
                if id == "-" || id.starts_with('#') {
                    continue;
                }
                let icon = child["icon"].as_str().unwrap();
                assert!(!icon.is_empty(), "{surface}: submenu row has no icon: {child}");
                assert!(!solidcraft_ui_egui::icon_svg::resolve(icon).placeholder, "{surface}: submenu placeholder {icon}");
            }
        }
    }
    collect(h, surface, requested);
    escape(h);
}

fn ribbon(h: &mut Harness, requested: &mut BTreeMap<String, bool>) {
    for tab in workspace::visible_tabs(&h.app) {
        click(h, &format!("tab:{tab}"));
        let panels = solidcraft_ui_egui::scenario::handles(&format!("panel:{tab}:"));
        assert!(!panels.is_empty(), "no panels on {tab}");
        for (handle, _) in panels {
            click(h, &handle);
            collect(h, &handle, requested);
            for (family, at) in solidcraft_ui_egui::scenario::handles("create_family:") {
                h.call("ui.move", json!({"x":at.x,"y":at.y}));
                h.frames(20);
                collect(h, &family, requested);
            }
            escape(h);
        }
    }
}

fn assistant(h: &mut Harness, requested: &mut BTreeMap<String, bool>) {
    use solidcraft_ui_egui::agent_chat::{Item, backend::ToolStatus};
    h.call("ui.agent_chat", json!({"features":true,"open":true,"picker":false,"width":440}));
    h.app.agent_chat.items = vec![
        Item::User { text: "Round the plate edges".into(), image: false, pictures: vec![], context: Default::default(), files: vec![] },
        Item::Tool {
            id: "audit".into(),
            title: "solid.fillet".into(),
            kind: "other".into(),
            status: Some(ToolStatus::Failed),
            output: Some("Use a smaller radius".into()),
        },
        Item::Approval { id: 1, title: "Export plate.step".into(), tool: "export".into(), answer: None },
    ];
    h.frames(3);
    collect(h, "Assistant composer, tool and approval", requested);
    h.app.agent_chat.items.clear();
    h.call("ui.agent_chat", json!({"open":false}));
    h.frames(2);
}

fn panels(h: &mut Harness, requested: &mut BTreeMap<String, bool>) {
    for command in solidcraft_engine::command_specs() {
        let panel = solidcraft_ui_egui::dialogs::Dialog::for_command(&h.app, command.id).is_some()
            || solidcraft_ui_egui::tools::Tool::for_command(command.id).is_some()
            || matches!(command.id, "parameters.change" | "document.metadata" | "material.properties.validate");
        if !panel {
            continue;
        }
        if matches!(command.id, "canvas.insert" | "canvas.decal") {
            h.step(&json!({"make_image":"audit-canvas.png"})).unwrap();
            h.step(&json!({"choose_file":"audit-canvas.png"})).unwrap();
        }
        let info = command.info(&h.app.session);
        let surface = format!("tool panel:{} enabled={} reason={:?}", command.id, info.enabled, info.disabled_reason);
        assert_eq!(h.call("ui.start", json!({"command":command.id}))["ok"], true, "{}", command.id);
        h.frames(2);
        collect(h, &surface, requested);
        escape(h);
    }
}

fn context_rows(h: &mut Harness, prefix: &str, requested: &mut BTreeMap<String, bool>) {
    for (handle, _) in solidcraft_ui_egui::scenario::handles(prefix) {
        if handle == "timeline:rollback" {
            continue;
        }
        h.step(&json!({"click":{"handle":handle},"button":"right"})).unwrap();
        collect(h, &handle, requested);
        escape(h);
    }
}

#[test]
fn requested_menu_icons_have_artwork_and_match_manifest() {
    let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/icon-audit/surfaces.txt");
    if evidence.exists() {
        std::fs::remove_file(&evidence).unwrap();
    }
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.services.install_app = Some(solidcraft_ui_egui::install::InstallApp {
        offer: Box::new(|| Some(solidcraft_ui_egui::install::Offer::Prompt)),
        prompt: Box::new(|| {}),
    });
    h.app.preferences.radial_context_menu = true;
    assert_eq!(h.call("ui.icon_audit", json!({"reset":true}))["ok"], true);
    h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
    h.frames(3);
    let mut requested = BTreeMap::new();
    collect(&mut h, "initial browser and timeline", &mut requested);
    assistant(&mut h, &mut requested);

    ribbon(&mut h, &mut requested);
    h.app.start("inspect.colored_components");
    h.frames(2);
    click(&mut h, "panel:INSPECT:CHECK");
    collect(&mut h, "component colors enabled", &mut requested);
    escape(&mut h);
    h.app.start("inspect.colored_components");
    click(&mut h, "appbar:file");
    assert!(h.widget("Export Current View as DXF…").is_some(), "painted File rows retain accessible action labels");
    collect(&mut h, "File", &mut requested);
    escape(&mut h);
    click(&mut h, "appbar:help");
    collect(&mut h, "Help", &mut requested);
    escape(&mut h);
    menu(&mut h, json!({"type":"workspace"}), "workspace", &mut requested);
    solidcraft_ui_egui::render_workspace::enter(&mut h.app, true).unwrap();
    h.frames(3);
    ribbon(&mut h, &mut requested);
    solidcraft_ui_egui::render_workspace::enter(&mut h.app, false).unwrap();
    h.frames(3);

    let body = h.app.session.model.state().bodies.first().unwrap().name.clone();
    let sketch = h.app.session.world_state().sketches.first().unwrap().feature;
    for target in [
        json!({"type":"viewport"}),
        json!({"type":"body","name":body}),
        json!({"type":"sketch","id":sketch}),
        json!({"type":"feature","id":sketch}),
        json!({"type":"component","id":0}),
        json!({"type":"origin"}),
        json!({"type":"origin_plane","name":"XY"}),
        json!({"type":"units"}),
        json!({"type":"named_views"}),
        json!({"type":"named_view","name":"Top"}),
        json!({"type":"toolbar"}),
        json!({"type":"toolbar_command","id":"solid.extrude"}),
        json!({"type":"folder","component":0,"scope":"c0","folder":"bodies"}),
        json!({"type":"folder","component":0,"scope":"c0","folder":"sketches"}),
        json!({"type":"folder","component":0,"scope":"c0","folder":"construction"}),
        json!({"type":"section"}),
        json!({"type":"canvas","id":1}),
    ] {
        menu(&mut h, target.clone(), &target.to_string(), &mut requested);
    }
    for selection in [
        json!({"type":"face","body":body,"index":0,"point":[0,0,0]}),
        json!({"type":"edge","body":body,"index":0,"point":[0,0,0]}),
        json!({"type":"vertex","body":body,"point":[0,0,0]}),
        json!({"type":"external_sketch_curve","sketch":sketch,"id":"l1"}),
    ] {
        h.app.run("select.set", json!({"items":[selection]})).unwrap();
        menu(&mut h, json!({"type":"viewport"}), &selection.to_string(), &mut requested);
    }
    h.app.run("select.clear", json!({})).unwrap();

    // Keep history traversal in a small fixture, independent of viewport/browser selection.
    let mut history = Harness::new();
    history.app.home.open = false;
    for name in ["History A", "History B"] {
        history.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":name})).unwrap();
    }
    history.frames(3);
    // Named feature history has independent menus in both layout preferences.
    for left in [false, true] {
        history.app.preferences.timeline_left = left;
        history.frames(3);
        context_rows(&mut history, "timeline:", &mut requested);
    }
    // The seam has its own checked display menu, independent of feature and folder rows.
    history.step(&json!({"click":{"handle":"history:names"},"button":"right"})).unwrap();
    collect(&mut history, "history seam", &mut requested);
    escape(&mut history);
    let features = history.app.session.doc.features.iter().take(2).map(|f| f.id).collect::<Vec<_>>();
    let group = history.app.run("timeline.group", json!({"features":features,"name":"Audit history folder"})).unwrap()["group"].as_u64().unwrap();
    history.frames(3);
    context_rows(&mut history, "timeline-folder:", &mut requested);
    history.app.tree.picked_timeline_groups.clear();
    history.app.run("select.set", json!({"items":features.iter().map(|id| json!({"type":"feature","id":id})).collect::<Vec<_>>()})).unwrap();
    history.frames(3);
    history.step(&json!({"click":{"handle":"timeline:History A"},"button":"right"})).unwrap();
    collect(&mut history, "history feature selection", &mut requested);
    escape(&mut history);
    history.app.run("select.clear", json!({})).unwrap();
    history.app.tree.picked_timeline_groups = vec![group];
    history.frames(3);
    history.step(&json!({"click":{"handle":"timeline-folder:Audit history folder"},"button":"right"})).unwrap();
    collect(&mut history, "history folder selection", &mut requested);
    escape(&mut history);
    history.app.tree.picked_timeline_groups.clear();
    // Document tabs currently expose close buttons and an overflow list, but no
    // right-click menu (in this branch or origin/main).
    h.step(&json!({"click":{"handle":"tab:design:0"},"button":"right"})).unwrap();
    escape(&mut h);

    // Overflow uses egui's nested panel menus. Open each visible panel submenu.
    h.call("ui.resize", json!({"width":360,"height":1000}));
    let mut overflow_count = 0;
    let mut overflow_rows = 0;
    for tab in workspace::visible_tabs(&h.app) {
        // The tabs were clicked above at full width; narrow layouts can clip their hit regions.
        h.app.ui.tab = tab.into();
        h.frames(3);
        if solidcraft_ui_egui::scenario::handle_at("toolbar:overflow").is_some() {
            overflow_count += 1;
            click(&mut h, "toolbar:overflow");
            collect(&mut h, "overflow", &mut requested);
            for panel in workspace::panels(tab) {
                if let Some([x, y]) = h.widget(panel) {
                    h.call("ui.move", json!({"x":x,"y":y}));
                    h.frames(20);
                    overflow_rows += solidcraft_ui_egui::scenario::handles("toolbar:").iter().filter(|(name, _)| name != "toolbar:overflow").count();
                    collect(&mut h, &format!("overflow:{tab}:{panel}"), &mut requested);
                }
            }
            escape(&mut h);
        }
    }
    assert!(overflow_count > 0, "the narrow traversal must actually open overflow menus");
    assert!(overflow_rows > 0, "overflow submenus must actually draw command rows");
    h.call("ui.resize", json!({"width":1600,"height":1000}));
    // Browser grouping and construction planes add row-specific menus.
    let group = h.app.run("browser.group", json!({"component":0,"folder":"bodies","items":[body]})).unwrap()["group"].as_u64().unwrap();
    menu(&mut h, json!({"type":"group","scope":"c0","id":group}), "body group", &mut requested);
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":25,"name":"Audit plane"})).unwrap();
    let plane = h.app.session.doc.features.last().unwrap().id;
    menu(&mut h, json!({"type":"plane","id":plane}), "construction plane", &mut requested);
    // Actual hit lists at body corners and face centres (including overlapping origin planes).
    h.app.fit_view();
    h.frames(3);
    for world in [[0.0, 0.0, 0.0], [40.0, 25.0, 8.0], [40.0, 25.0, 22.0]] {
        let [x, y] = h.at(&json!({"world":world})).unwrap();
        assert_eq!(h.call("ui.selectOther", json!({"x":x,"y":y}))["ok"], true);
        collect(&mut h, "Select Other", &mut requested);
        h.call("ui.selectOther", json!({"close":true}));
    }

    // Search every command by its full label so results beyond the first page are painted.
    for command in solidcraft_engine::command_specs() {
        solidcraft_ui_egui::shortcut_box::open(&mut h.app, None);
        h.app.sbox.text = command.label.to_owned();
        h.frames(2);
        collect(&mut h, command.id, &mut requested);
        escape(&mut h);
    }
    // Every registered command is also started through its interactive entry point. Some
    // require geometry or an active sketch; the second pass supplies sketch context.
    for sketching in [false, true] {
        if sketching {
            h.app.run("sketch.edit", json!({"sketch":sketch})).unwrap();
            h.frames(3);
            ribbon(&mut h, &mut requested);
            for selection in [json!({"type":"sketch_curve","id":"l1"}), json!({"type":"sketch_point","id":"p1"})] {
                h.app.run("select.set", json!({"items":[selection]})).unwrap();
                menu(&mut h, json!({"type":"viewport"}), "sketch entity", &mut requested);
            }
            h.app.run("select.clear", json!({})).unwrap();
        }
        panels(&mut h, &mut requested);
    }
    // Sheet and assembly contexts expose contextual tabs and browser row kinds that
    // a single solid part cannot offer. Use repository-owned public sample recipes.
    for sample in solidcraft_engine::sample::samples().into_iter().filter(|s| matches!(s.name, "Sheet Metal Bracket" | "Hinge Assembly")) {
        let mut sample_h = Harness::new();
        sample_h.app.home.open = false;
        sample_h.app.session.run_script(&(sample.script)()).unwrap();
        sample_h.frames(3);
        ribbon(&mut sample_h, &mut requested);
        let joints = sample_h.app.session.doc.assembly.joints.iter().map(|j| j.id).collect::<Vec<_>>();
        for id in joints {
            menu(&mut sample_h, json!({"type":"joint","id":id}), "assembly joint", &mut requested);
        }
        let paths = sample_h.app.session.doc.occurrences.iter().map(|o| (o.id, o.component)).collect::<Vec<_>>();
        for (id, component) in paths {
            menu(&mut sample_h, json!({"type":"component","id":component}), "component", &mut requested);
            menu(&mut sample_h, json!({"type":"occurrence","path":[id]}), "occurrence", &mut requested);
        }
        context_rows(&mut sample_h, "row:", &mut requested);
        panels(&mut sample_h, &mut requested);
        collect(&mut sample_h, sample.name, &mut requested);
    }
    let mut geometry = Harness::new();
    geometry.app.home.open = false;
    geometry.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Audit block"})).unwrap();
    geometry.app.run("solid.split_body", json!({"body":"Audit block","plane":{"origin":[5,0,0],"normal":[1,0,0]}})).unwrap();
    geometry.frames(3);
    collect(&mut geometry, "Split feature history", &mut requested);
    geometry.app.run("sketch.create", json!({"plane":"XY","name":"Wall profile"})).unwrap();
    geometry.app.run("sketch.line", json!({"points":[[20,0],[30,0]]})).unwrap();
    geometry.app.run("sketch.finish", json!({})).unwrap();
    geometry.app.run("surface.extrude", json!({"sketch":"Wall profile","curves":["l1"],"distance":10,"body_name":"Audit surface"})).unwrap();
    let mesh = geometry.dir.join("audit.obj");
    std::fs::write(&mesh, "o Audit mesh\nv 40 0 0\nv 50 0 0\nv 40 10 0\nv 40 0 10\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n").unwrap();
    geometry.app.run("file.insert_mesh", json!({"path":mesh})).unwrap();
    geometry.frames(3);
    let bodies = geometry.app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    assert!(bodies.iter().any(|b| b == "Audit surface"));
    for name in bodies {
        menu(&mut geometry, json!({"type":"body","name":name}), &format!("solid/surface/mesh body {name}"), &mut requested);
        geometry.app.run("select.set", json!({"items":[{"type":"body","name":name}]})).unwrap();
        menu(&mut geometry, json!({"type":"viewport"}), &format!("solid/surface/mesh viewport {name}"), &mut requested);
    }
    panels(&mut geometry, &mut requested);
    // Home recent-file context menus have their own native popup dispatch.
    let path = geometry.dir.join("audit.solidcraft");
    geometry.app.run("file.save", json!({"path":path})).unwrap();
    geometry.app.home.recent = vec![path.to_string_lossy().into_owned()];
    geometry.app.recoverable.push(solidcraft_engine::recovery::Entry {
        id: "icon-audit".into(),
        path: Some("audit.solidcraft".into()),
        name: "Audit design".into(),
        saved_at: 0,
        features: 1,
    });
    geometry.app.home.open = true;
    geometry.frames(3);
    context_rows(&mut geometry, "home:recent:", &mut requested);
    click(&mut geometry, "appbar:file");
    geometry.step(&json!({"widget":geometry.app.tr("ui.open_recent")})).expect("click Open Recent submenu");
    geometry.frames(20);
    assert!(geometry.widget(geometry.app.tr("ui.clear_recent")).is_some(), "the recent-design submenu must be open");
    collect(&mut geometry, "File recent designs", &mut requested);
    escape(&mut geometry);
    // Populate actual document slots, then click the same overflow launcher as a person.
    let mut documents = Harness::new();
    for _ in 0..12 {
        solidcraft_ui_egui::documents::open_tab(&mut documents.app, solidcraft_engine::Session::default()).unwrap();
    }
    documents.app.home.open = false;
    documents.call("ui.resize", json!({"width":640,"height":1000}));
    click(&mut documents, "tab:overflow");
    documents.frames(3);
    assert!(documents.ctx.data(|d| d.get_temp::<bool>(egui::Id::new("sc_doc_more"))).unwrap_or(false), "the document list must actually be open");
    collect(&mut documents, "document overflow list (active icon is a selection marker)", &mut requested);
    // Diagnostic badges and Copy Error/Warning Message only appear on troubled features.
    for failing in [false, true] {
        let mut diagnostics = Harness::new();
        diagnostics.app.home.open = false;
        if failing {
            diagnostics.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
            diagnostics.app.run("parameters.change", json!({"name":"width","expression":"5 mm"})).unwrap();
        } else {
            diagnostics.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"One"})).unwrap();
            diagnostics.app.run("solid.fillet", json!({"edges":[[5,0,10]],"radius":20,"fit":"reduce","body":"One"})).unwrap();
        }
        for left in [false, true] {
            diagnostics.app.preferences.timeline_left = left;
            diagnostics.frames(3);
            context_rows(&mut diagnostics, "timeline:", &mut requested);
            collect(&mut diagnostics, if failing { "failed history" } else { "warning history" }, &mut requested);
        }
    }
    // Existing scenarios actually interact with dimension editors, corner popups, sketch
    // palette controls and overlapping sketch hit lists, in addition to opening their tools.
    for (name, source) in [
        ("dimensions and constraints", include_str!("scenarios/s11_sketch_constraints.json")),
        ("corner popup", include_str!("scenarios/sketch_corner_window.json")),
        ("sketch palette", include_str!("scenarios/sketch_palette_linetype_icons.json")),
        ("sketch Select Other", include_str!("scenarios/select_other_coincident_lines.json")),
        ("canvas calibration", include_str!("scenarios/s19_canvas_calibrate.json")),
    ] {
        let mut scenario = Harness::new();
        scenario.run(&serde_json::from_str(source).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        collect(&mut scenario, name, &mut requested);
    }
    icon_svg::audit::stop();
    let manifest = requested.iter().map(|(name, placeholder)| format!("{name}\t{placeholder}\n")).collect::<String>();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/ui/design/requested_icons.txt");
    if std::env::var("UPDATE_ICON_MANIFEST").as_deref() == Ok("1") {
        std::fs::write(&path, &manifest).unwrap();
    } else {
        // A Windows checkout (core.autocrlf) gives the file CRLF line endings (#4932).
        let expected = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert_eq!(
            expected, manifest,
            "Runtime icon discovery changed. Regenerate with UPDATE_ICON_MANIFEST=1 cargo test -p solidcraft-ui-egui --test icon_audit"
        );
    }
    let missing = requested.iter().filter(|(_, placeholder)| **placeholder).map(|(name, _)| name).collect::<Vec<_>>();
    assert!(missing.is_empty(), "Runtime placeholder requests (see surface diagnostics above): {missing:?}");
}

#[test]
fn audit_is_opt_in_drains_and_records_resolved_aliases() {
    icon_svg::audit::stop();
    icon_svg::resolve("extrude");
    assert!(icon_svg::audit::take().is_empty());
    icon_svg::audit::start();
    icon_svg::resolve("surface_extrude");
    icon_svg::resolve("surface_extrude");
    icon_svg::resolve("missing_audit_test_icon");
    let requests = icon_svg::audit::take();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().any(|r| r.name == "surface_extrude" && r.master == "extrude" && !r.placeholder));
    assert!(requests.iter().any(|r| r.name == "missing_audit_test_icon" && r.master == "placeholder" && r.placeholder));
    assert!(icon_svg::audit::take().is_empty());
    icon_svg::audit::stop();
}

/// Review evidence uses the application's actual SVG renderer, without a window or screenshot.
#[test]
fn offset_mode_artwork_review() {
    use solidcraft_ui_egui::theme::{ColorVision, Tokens};
    let names = ["offset", "offset_two_sides", "offset_symmetric", "sketch_chamfer", "sketch_chamfer_two_distances", "split_body"];
    let mut sheet = tiny_skia::Pixmap::new(400, names.len() as u32 * 88).unwrap();
    for (row, name) in names.iter().enumerate() {
        let resolved = icon_svg::resolve(name);
        assert!(!resolved.placeholder, "{name}");
        for (column, dark) in [false, true].into_iter().enumerate() {
            let [r, g, b, _] = Tokens::themed(dark, ColorVision::Off).panel.to_array();
            let mut background = tiny_skia::Paint::default();
            background.set_color_rgba8(r, g, b, 255);
            sheet.fill_rect(
                tiny_skia::Rect::from_xywh(column as f32 * 200.0, row as f32 * 88.0, 200.0, 88.0).unwrap(),
                &background,
                tiny_skia::Transform::identity(),
                None,
            );
            let mut x = column as i32 * 200 + 8;
            for px in [64, 24, 16] {
                let look = icon_svg::Look::themed(dark, resolved.accent, ColorVision::Off);
                let pm = icon_svg::render(icon_svg::doc_for(resolved.master, px).unwrap(), look, px).unwrap();
                sheet.draw_pixmap(x, row as i32 * 88 + 8, pm.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
                x += px as i32 + 16;
            }
        }
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/icon-audit");
    std::fs::create_dir_all(&dir).unwrap();
    image::save_buffer(dir.join("offset-review.png"), sheet.data(), sheet.width(), sheet.height(), image::ColorType::Rgba8).unwrap();
}

#[test]
fn overflow_icons_with_many_pins_at_200_points() {
    let mut h = Harness::new();
    icon_svg::audit::start();
    let steps = serde_json::from_str(include_str!("scenarios/s48_toolbar_many_pins.json")).unwrap();
    if let Err(error) = h.run(&steps) {
        h.step(&json!({"debug":"widgets"})).unwrap();
        h.step(&json!({"debug":"handles"})).unwrap();
        panic!("{error}");
    }
    let requested = icon_svg::audit::take();
    icon_svg::audit::stop();
    let manifest =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/ui/design/requested_icons.txt")).unwrap();
    for icon in requested {
        assert!(!icon.placeholder, "{}", icon.name);
        assert!(manifest.lines().any(|line| line == format!("{}\tfalse", icon.name)), "{} is absent from runtime discovery", icon.name);
    }
}
