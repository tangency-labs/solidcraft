use serde_json::{Value, json};
use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_engine::geom::paper::{Dashes, Group, Item, ObjectKind, Scene};
use solidcraft_engine::{Sel, geom::Loop2Geom};

use super::*;
use crate::scenario::Harness;

/// One frame of `ctx` outside the harness. Its texture changes (glyphs, sheet pictures) have no
/// renderer here, so they are taken as applied: egui checks that none are dropped unhandled.
fn frame_on(ctx: &egui::Context, input: egui::RawInput, ui: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
    let mut out = ctx.run_ui(input, ui);
    out.textures_delta.clear();
    out
}

#[test]
fn cell_fit_keeps_paper_inside_tiny_and_odd_cells_and_preserves_manual_framing() {
    let paper = Vec2::new(420.0, 297.0);
    for size in [vec2(800.0, 600.0), vec2(120.0, 80.0), vec2(3.0, 2.0), vec2(1200.0, 61.0)] {
        let cell = Rect::from_min_size(pos2(17.0, 32.0), size);
        let (target, scale) = sheet_fit(cell, paper, None, 96.0 / 25.4, 24.0).unwrap();
        assert_eq!(target, paper * 0.5);
        assert!(scale.is_finite() && scale > 0.0);
        assert!(paper.x * scale <= f64::from(size.x) && paper.y * scale <= f64::from(size.y));
        let manual = (Vec2::new(32.0, 19.0), 8.0);
        assert_eq!(sheet_fit(cell, paper, Some(manual), 96.0 / 25.4, 24.0), Some(manual));
    }
    let cell = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
    for bad in [Vec2::ZERO, Vec2::new(f64::NAN, 297.0), Vec2::new(420.0, f64::INFINITY)] {
        assert!(sheet_fit(cell, bad, None, 1.0, 24.0).is_none());
    }
    assert!(sheet_fit(Rect::NOTHING, paper, None, 1.0, 24.0).is_none());
    assert!(sheet_fit(cell, paper, Some((Vec2::ZERO, f64::NAN)), 1.0, 24.0).is_none());
}

#[test]
fn sheet_page_cameras_are_independent_and_passive_paint_restores_the_command_receiver() {
    use solidcraft_views::ViewKind;
    let mut h = Harness::new();
    h.app.home.open = false;
    let model = h.app.docs.views.active().id;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let (drawing, sheet, size) = (drawing.id, drawing.sheets[0].id, drawing.sheets[0].sheet.size_mm().unwrap());
    let subject = crate::sheet_views::subject(drawing, sheet);
    let first = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject.clone()), false);
    let second = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject), false);
    let key = (sheet, [size.x.to_bits(), size.y.to_bits()]);
    for (id, scale, target) in [(first, 2.5, Vec2::new(21.0, 34.0)), (second, 6.0, Vec2::new(90.0, 120.0))] {
        activate_view(&mut h.app, id);
        select_sheet(&mut h.app, sheet);
        let page = &mut h.app.drawing_workspace.page;
        page.scale = scale;
        page.cam.target.x = target.x;
        page.cam.target.y = target.y;
        page.fitted = Some(key);
        page.fitted_on = None;
    }
    crate::documents::view_activate(&mut h.app, model);
    crate::sketch_view::sync(&mut h.app).unwrap();
    let receiver = (h.app.session.open_drawing, h.app.session.model_focus, h.app.ui.tab.clone());
    let focus = h.app.session.view_context.clone();
    let camera = h.app.cam;
    let descriptors = h.app.docs.views.views().to_vec();
    let compass = h.app.viewport.compass_view;
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let ctx = h.ctx.clone();
    let _ = frame_on(&ctx, egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))), ..Default::default() }, |ui| {
        for id in [first, second] {
            ui.push_id(id, |ui| {
                crate::documents::with_view_look(&mut h.app, id, |app| {
                    assert_eq!(app.viewport.compass_view, Some(id), "passive Sheet paint keeps its exact tile identity");
                    assert!(show_view(app, ui, id));
                });
            });
        }
    });
    assert_eq!((h.app.session.open_drawing, h.app.session.model_focus, h.app.ui.tab.clone()), receiver);
    assert_eq!(h.app.session.view_context, focus);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.docs.views.views(), descriptors.as_slice(), "Sheet paint must not initialize or alter product camera descriptors");
    assert_eq!(h.app.viewport.compass_view, compass);
    assert!(Arc::ptr_eq(&h.app.session.doc, &doc));
    assert_eq!(h.app.session.undo.len(), undo);
    activate_view(&mut h.app, first);
    assert!((h.app.drawing_workspace.page.scale - 2.5).abs() < 1e-9);
    assert_eq!(h.app.drawing_workspace.page.cam.target.x, 21.0);
    activate_view(&mut h.app, second);
    assert!((h.app.drawing_workspace.page.scale - 6.0).abs() < 1e-9);
    assert_eq!(h.app.drawing_workspace.page.cam.target.x, 90.0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn sheet_page_cache_keeps_moved_and_hidden_views_but_retires_closed_views() {
    use solidcraft_views::ViewKind;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let subject = crate::sheet_views::subject(drawing.id, drawing.sheets[0].id);
    let first = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject.clone()), false);
    let second = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject), false);
    activate_view(&mut h.app, first);
    h.app.drawing_workspace.page.scale = 2.0;
    activate_view(&mut h.app, second);
    h.app.drawing_workspace.page.scale = 7.0;
    h.app.run("tab.view.move_to_window", json!({"view":first})).unwrap();
    activate_view(&mut h.app, 0);
    assert_eq!(h.app.drawing_workspace.pages.get(&first).unwrap().scale, 2.0);
    assert_eq!(h.app.drawing_workspace.pages.get(&second).unwrap().scale, 7.0);
    h.app.run("tab.view.close", json!({"id":second})).unwrap();
    activate_view(&mut h.app, 0);
    assert!(h.app.drawing_workspace.pages.contains_key(&first));
    assert!(!h.app.drawing_workspace.pages.contains_key(&second));
}

#[test]
fn passive_sheet_wheel_keeps_the_focused_other_drawings_annotation_selected() {
    use solidcraft_views::ViewKind;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    h.app.run("drawing.create", json!({"name":"First drawing"})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let (first_drawing, first_sheet) = (drawing.id, drawing.sheets[0].id);
    let note = h.app.run("drawing.note.add", json!({"anchor":{"sheet":"Sheet 1"},"at":[50,50],"content":"Selected annotation"})).unwrap();
    let note = solidcraft_engine::doc::ids::decode(note["note"].as_str().unwrap()).unwrap();
    let first = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(crate::sheet_views::subject(first_drawing, first_sheet)), false);
    h.app.run("drawing.create", json!({"name":"Second drawing"})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let (second_drawing, second_sheet) = (drawing.id, drawing.sheets[0].id);
    assert_ne!(first_drawing, second_drawing);
    let second = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(crate::sheet_views::subject(second_drawing, second_sheet)), false);
    crate::documents::view_activate(&mut h.app, first);
    crate::sketch_view::sync(&mut h.app).unwrap();
    let selection = vec![solidcraft_engine::Sel::Drawing { drawing: first_drawing, object: note }];
    h.app.session.selection = selection.clone();
    let receiver = (h.app.session.open_drawing, h.app.session.model_focus, h.app.ui.tab.clone());
    let focus = h.app.session.view_context.clone();
    let ctx = h.ctx.clone();
    let canvas = Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0));
    let mut frame = |time, events| {
        let _ = frame_on(&ctx, egui::RawInput { screen_rect: Some(canvas), focused: true, time: Some(time), events, ..Default::default() }, |ui| {
            ui.push_id(second, |ui| assert!(show_view(&mut h.app, ui, second)));
        });
        assert_eq!(h.app.session.selection, selection, "a passive drawing cannot prune the focused drawing's pick");
    };
    frame(0.0, Vec::new());
    let before = h.app.drawing_workspace.pages.get(&second).unwrap().scale;
    let events = vec![
        egui::Event::PointerMoved(canvas.center()),
        egui::Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        },
    ];
    let _ = frame_on(&ctx, egui::RawInput { screen_rect: Some(canvas), focused: true, time: Some(1.0 / 60.0), events, ..Default::default() }, |ui| {
        ui.push_id(second, |ui| assert!(show_view(&mut h.app, ui, second)));
    });
    assert_ne!(h.app.drawing_workspace.pages.get(&second).unwrap().scale, before, "the passive wheel actually runs camera navigation");
    assert_eq!(h.app.session.selection, selection);
    assert_eq!((h.app.session.open_drawing, h.app.session.model_focus, h.app.ui.tab.clone()), receiver);
    assert_eq!(h.app.session.view_context, focus);
    assert!(h.app.session.doc.drawing(first_drawing).unwrap().has_object(note));
}

#[test]
fn two_sheet_cameras_share_drawing_view_selection() {
    use solidcraft_views::ViewKind;
    let (mut h, front) = plate_sheet();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let (drawing, sheet) = (drawing.id, drawing.sheets[0].id);
    let front = solidcraft_engine::doc::ids::decode(&front).unwrap();
    let subject = crate::sheet_views::subject(drawing, sheet);
    let first = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject.clone()), false);
    let second = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject), false);
    activate_view(&mut h.app, first);
    select_sheet(&mut h.app, sheet);
    h.app.run("select.set", json!({"items": [solidcraft_engine::Sel::Drawing { drawing, object: front }]})).unwrap();
    assert_eq!(selected_drawing_view(&h.app), Some(front));
    activate_view(&mut h.app, second);
    select_sheet(&mut h.app, sheet);
    assert_eq!(selected_drawing_view(&h.app), Some(front));
    h.app.run("select.clear", json!({})).unwrap();
    activate_view(&mut h.app, first);
    assert_eq!(selected_drawing_view(&h.app), None);
}

fn annotated_sheet_views() -> (Harness, u64, [(u64, u64, u64, u64); 3]) {
    fn note(app: &mut crate::SolveApp, sheet: u64) -> u64 {
        let made = app
            .run("drawing.note.add", json!({"anchor":{"sheet":solidcraft_engine::doc::ids::encode(sheet)},"at":[50,50],"content":"Annotation"}))
            .unwrap();
        solidcraft_engine::doc::ids::decode(made["note"].as_str().unwrap()).unwrap()
    }
    let mut h = Harness::new();
    h.app.home.open = false;
    let model = h.app.docs.views.active().id;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    h.app.run("drawing.create", json!({"name":"First drawing"})).unwrap();
    let first = h.app.session.drawing("drawing.inspect").unwrap();
    let (drawing, sheet) = (first.id, first.sheets[0].id);
    let first_note = note(&mut h.app, sheet);
    let added = h.app.run("drawing.sheet.add", json!({"name":"Other sheet"})).unwrap();
    let other = solidcraft_engine::doc::ids::decode(added["sheet"].as_str().unwrap()).unwrap();
    let other_note = note(&mut h.app, other);
    h.app.run("drawing.create", json!({"name":"Second drawing"})).unwrap();
    let last = h.app.session.drawing("drawing.inspect").unwrap();
    let (last_drawing, last_sheet) = (last.id, last.sheets[0].id);
    let last_note = note(&mut h.app, last_sheet);
    let mut records = [(drawing, sheet, first_note, 0), (drawing, other, other_note, 0), (last_drawing, last_sheet, last_note, 0)];
    for (drawing, sheet, note, view) in &mut records {
        *view =
            crate::documents::view_open(&mut h.app, solidcraft_views::ViewKind::Sheet, Some(crate::sheet_views::subject(*drawing, *sheet)), false);
        assert!(h.app.session.selection.is_empty(), "another sheet's annotation must not stay selected");
        h.app.session.selection = vec![solidcraft_engine::Sel::Drawing { drawing: *drawing, object: *note }];
    }
    (h, model, records)
}

#[test]
fn committed_sheet_focus_restores_exact_sheet_picks_and_keeps_model_selection_separate() {
    let (mut h, model, records) = annotated_sheet_views();
    for (drawing, sheet, note, view) in [records[0], records[1], records[2], records[0]] {
        assert!(crate::documents::view_activate(&mut h.app, view));
        assert_eq!(h.app.session.open_drawing, Some(drawing));
        assert_eq!(active_sheet(&h.app), Some(sheet));
        assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Drawing { drawing, object: note }]);
        let copy =
            crate::documents::view_open(&mut h.app, solidcraft_views::ViewKind::Sheet, Some(crate::sheet_views::subject(drawing, sheet)), false);
        assert_ne!(copy, view);
        assert_eq!(
            h.app.session.selection,
            [solidcraft_engine::Sel::Drawing { drawing, object: note }],
            "two views of the same sheet share all picks"
        );
    }
    let (drawing, sheet, _, view) = records[1];
    crate::documents::view_activate(&mut h.app, view);
    let made = h
        .app
        .run("drawing.note.add", json!({"anchor":{"sheet":solidcraft_engine::doc::ids::encode(sheet)},"at":[80,80],"content":"Receiver check"}))
        .unwrap();
    let note = solidcraft_engine::doc::ids::decode(made["note"].as_str().unwrap()).unwrap();
    assert!(h.app.session.doc.drawing(drawing).unwrap().on_sheet(sheet).notes.contains(&note));
    crate::documents::view_activate(&mut h.app, model);
    let model_picks = vec![solidcraft_engine::Sel::Plane { name: "XY".into() }];
    h.app.session.selection = model_picks.clone();
    crate::documents::view_activate(&mut h.app, records[2].3);
    assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Drawing { drawing: records[2].0, object: records[2].2 }]);
    crate::documents::view_activate(&mut h.app, model);
    assert_eq!(h.app.session.selection, model_picks);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn inactive_child_sheet_scope_restores_picks_focus_and_history_but_targeted_commands_adopt_its_sheet() {
    let (mut h, _, records) = annotated_sheet_views();
    let (drawing, _, note, view) = records[0];
    let (other, _, other_note, child_view) = records[2];
    crate::documents::view_activate(&mut h.app, view);
    h.app.run("tab.view.move_to_window", json!({"view":child_view})).unwrap();
    let window = h.app.docs.views.window_of(child_view).unwrap();
    // Moving a view commits focus to its new window. Explicitly return to the
    // root Sheet so this test really paints an inactive child receiver.
    crate::view_windows::focus_view(&mut h.app, view, Some(0)).unwrap();
    crate::sketch_view::sync(&mut h.app).unwrap();
    assert_eq!(crate::view_windows::focused_view(&h.app), view);
    assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Drawing { drawing, object: note }]);
    let receiver = (h.app.session.open_drawing, h.app.session.model_focus, h.app.session.history_part());
    let focus = h.app.session.view_context.clone();
    let ctx = h.ctx.clone();
    let _ = frame_on(
        &ctx,
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))), focused: false, ..Default::default() },
        |ui| {
            crate::view_windows::with_strip(&mut h.app, Some(window), |app| {
                assert_eq!(
                    app.session.selection,
                    [solidcraft_engine::Sel::Drawing { drawing: other, object: other_note }],
                    "a command window borrows its correct sheet picks"
                );
                assert!(show_view(app, ui, child_view));
                Ok(())
            })
            .unwrap();
        },
    );
    assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Drawing { drawing, object: note }]);
    assert_eq!((h.app.session.open_drawing, h.app.session.model_focus, h.app.session.history_part()), receiver);
    assert_eq!(h.app.session.view_context, focus);
    let other_sheet = records[2].1;
    let made = crate::view_windows::with_strip(&mut h.app, Some(window), |app| {
        app.run(
            "drawing.note.add",
            json!({"anchor":{"sheet":solidcraft_engine::doc::ids::encode(other_sheet)},"at":[90,90],"content":"Explicit window receiver"}),
        )
    })
    .unwrap();
    let made = solidcraft_engine::doc::ids::decode(made["note"].as_str().unwrap()).unwrap();
    assert!(h.app.session.doc.drawing(other).unwrap().on_sheet(other_sheet).notes.contains(&made));
    assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Drawing { drawing, object: note }]);
    crate::view_windows::focus_view(&mut h.app, child_view, Some(window)).unwrap();
    crate::view_windows::with_strip(&mut h.app, Some(window), |app| {
        assert_eq!(app.session.open_drawing, Some(other));
        assert_eq!(app.session.history_part(), Some(other));
        assert_eq!(app.session.selection, [solidcraft_engine::Sel::Drawing { drawing: other, object: other_note }]);
        Ok(())
    })
    .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn control_undo_and_sheet_tools_use_the_focused_child_after_returning_to_the_model_host() {
    let (mut h, model, records) = annotated_sheet_views();
    let (drawing, sheet, note, child) = records[2];
    h.app.run("tab.view.move_to_window", json!({"view":child})).unwrap();
    let window = h.app.docs.views.window_of(child).unwrap();
    crate::documents::view_activate(&mut h.app, model);
    crate::view_windows::focus_view(&mut h.app, child, Some(window)).unwrap();
    crate::sketch_view::sync(&mut h.app).unwrap();
    crate::view_windows::with_strip(&mut h.app, Some(window), |app| {
        assert_eq!(app.session.open_drawing, Some(drawing));
        assert_eq!(app.session.history_part(), Some(drawing));
        Ok(())
    })
    .unwrap();
    assert_eq!(h.app.ui.tab, "MODEL", "the main host restores its own ribbon after the child scope");
    assert_eq!(h.app.session.view_context.focused(), child);
    let first_notes: Vec<_> = h.app.session.doc.drawing(records[0].0).unwrap().notes.iter().map(|note| note.id).collect();
    let reply = h.call("engine.execute", json!({"command":"edit.undo","params":{}}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(!h.app.session.doc.drawing(drawing).unwrap().has_object(note), "Undo takes the focused drawing's note");
    assert_eq!(h.app.session.history_part(), Some(drawing));
    assert_eq!(h.app.session.doc.drawing(records[0].0).unwrap().notes.iter().map(|note| note.id).collect::<Vec<_>>(), first_notes);
    let reply = h.call(
        "engine.execute",
        json!({"command":"drawing.note.add","params":{
            "anchor":{"sheet":solidcraft_engine::doc::ids::encode(sheet)},"at":[100,100],"content":"Focused child receiver"
        }}),
    );
    assert_eq!(reply["ok"], true, "{reply}");
    let added = solidcraft_engine::doc::ids::decode(reply["result"]["note"].as_str().unwrap()).unwrap();
    assert!(h.app.session.doc.drawing(drawing).unwrap().on_sheet(sheet).notes.contains(&added));
    h.app.apply_off_thread = None;
    h.app.run_heavy("edit.undo", json!({})).unwrap();
    assert!(!h.app.session.doc.drawing(drawing).unwrap().has_object(added), "heavy history binds its source before taking an ahead copy");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn control_drawing_open_and_new_sheet_stay_in_the_focused_child_and_explicit_root_scope_wins() {
    let (mut h, model, records) = annotated_sheet_views();
    let (drawing, sheet, _, _) = records[0];
    let child = records[2].3;
    h.app.run("tab.view.move_to_window", json!({"view":child})).unwrap();
    let window = h.app.docs.views.window_of(child).unwrap();
    crate::documents::view_activate(&mut h.app, model);
    crate::view_windows::focus_view(&mut h.app, child, Some(window)).unwrap();
    crate::sketch_view::sync(&mut h.app).unwrap();
    crate::view_windows::with_strip(&mut h.app, Some(window), |_| Ok(())).unwrap();
    h.app.docs.views.set_spec(model, solidcraft_viewport::ViewSpec::new(h.app.cam));
    let root_views = h.app.docs.views.views().to_vec();
    let root_layout = h.app.docs.views.tiles().cloned();
    let camera = h.app.cam;
    let subject = crate::sheet_views::subject(drawing, sheet);
    for _ in 0..2 {
        let reply = h.call("engine.execute", json!({"command":"drawing.open","params":{"drawing":solidcraft_engine::doc::ids::encode(drawing)}}));
        assert_eq!(reply["ok"], true, "{reply}");
        let child_views = h.app.docs.views.views_of(window).unwrap();
        assert_eq!(
            child_views.views().iter().filter(|view| view.subject.as_deref() == Some(subject.as_str())).count(),
            1,
            "reuse happens in the focused child"
        );
        assert_eq!(child_views.active().subject.as_deref(), Some(subject.as_str()));
        assert_eq!(h.app.docs.views.active().id, model);
        assert_eq!(h.app.docs.views.views(), root_views.as_slice());
        assert_eq!(h.app.docs.views.tiles(), root_layout.as_ref());
        assert_eq!(h.app.cam, camera);
    }
    let reply = h.call("engine.execute", json!({"command":"drawing.sheet.add","params":{"name":"Child sheet"}}));
    assert_eq!(reply["ok"], true, "{reply}");
    let added = solidcraft_engine::doc::ids::decode(reply["result"]["sheet"].as_str().unwrap()).unwrap();
    assert_eq!(h.app.docs.views.views_of(window).unwrap().active().subject.as_deref(), Some(crate::sheet_views::subject(drawing, added).as_str()));
    assert_eq!(h.app.docs.views.active().id, model);
    assert_eq!(h.app.docs.views.views(), root_views.as_slice());
    assert_eq!(h.app.docs.views.tiles(), root_layout.as_ref());
    assert_eq!(h.app.cam, camera);
    let child_count = h.app.docs.views.views_of(window).unwrap().len();
    crate::view_windows::with_strip(&mut h.app, Some(0), |app| {
        app.run("drawing.open", json!({"drawing":solidcraft_engine::doc::ids::encode(drawing)}))
    })
    .unwrap();
    assert_eq!(h.app.docs.views.active().subject.as_deref(), Some(subject.as_str()), "an explicit root scope overrides native child focus");
    assert_eq!(h.app.docs.views.views_of(window).unwrap().len(), child_count);
}

#[test]
fn public_drawing_open_effect_keeps_the_legacy_sheet_view_preference() {
    let mut h = Harness::new();
    h.app.preferences.design_sub_tab_sheets = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    let before = h.app.docs.views.to_json();
    h.app.run("drawing.create", json!({})).unwrap();
    assert_eq!(h.app.docs.views.to_json(), before);
    assert!(h.app.session.open_drawing.is_some());
}

#[test]
fn narrow_sheet_detail_keeps_view_outlines_but_omits_hatch_groups() {
    let stroke = Item::Stroke { path: vec![Seg2::Line { a: Vec2::ZERO, b: Vec2::new(20.0, 10.0) }], closed: false, width_mm: 0.25, pattern: None };
    let scene = Scene {
        size_mm: Vec2::new(100.0, 100.0),
        fonts: Vec::new(),
        clips: Vec::new(),
        groups: vec![
            Group { object: 1, kind: ObjectKind::View, items: vec![stroke.clone()] },
            Group { object: 2, kind: ObjectKind::Fill, items: vec![stroke] },
        ],
    };
    assert_eq!(marks_detail(&scene, 0.01, true).unwrap().len(), 2);
    assert_eq!(marks_detail(&scene, 0.01, false).unwrap().len(), 1);
}

/// A 100 × 80 × 20 mm plate (W23's fixture without its bores) on ISO A3 at 1:2: its front view and
/// the view right of it, the front's top edge dimensioned, in the Drawing workspace, with every view
/// drawn. Returns the harness and the front view's id.
fn plate_sheet() -> (Harness, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let front = h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [140, 180]})).unwrap();
    let front = front["view"].as_str().unwrap().to_owned();
    h.app.run("drawing.view.projected", json!({"parent": front, "side": "right"})).unwrap();
    h.app.run("drawing.dimension.add", json!({"view": front, "kind": {"length": {"edge": [50, 0, 20]}}})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    (h, front)
}

#[test]
fn note_tool_places_text_on_the_active_sheet() {
    let (mut h, _) = plate_sheet();
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.note"));
    h.frames(4);
    let field = crate::scenario::rect_of("drawing:note:text").unwrap().center();
    h.step(&json!({"click": [field.x, field.y]})).unwrap();
    h.step(&json!({"text": "Deburr all edges"})).unwrap();
    let paper = crate::scenario::rect_of("sheet:paper").unwrap().center();
    h.step(&json!({"click": [paper.x, paper.y]})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let note = &drawing.notes[0];
    assert_eq!(note.content.0, vec![solidcraft_engine::geom::cell::Part::Text("Deburr all edges".to_owned())]);
}

/// A note saved before alignments keeps its layout in the Notes tool's panel: its own entry is selected
/// and OK changes nothing; choosing Centre is per-line centring (`centre`), a different layout, and only
/// then does the edit set it. A note with an alignment has no such entry.
#[test]
fn a_note_saved_before_alignments_has_its_own_choice_in_the_notes_panel() {
    let (mut h, _) = plate_sheet();
    let added = h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR"})).unwrap();
    let id = solidcraft_engine::doc::ids::decode(added["note"].as_str().unwrap()).unwrap();
    let align = |h: &Harness| h.app.session.drawing("drawing.inspect").unwrap().notes.by_id(id).unwrap().align;
    let open = |h: &mut Harness| {
        crate::drawing_workspace::edit_object(&mut h.app, "note", id);
        settle(h);
        h.frames(3);
        assert!(crate::drawing_workspace::inspector_visible(&h.app), "the note's options are open");
    };
    let click = |h: &mut Harness, at: egui::Pos2| {
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(3);
    };
    // Where to click for an entry: its segment when the choice is a row of segments, and, when the kit
    // has turned it into a dropdown for want of room (as it may with the fourth entry), its place in the
    // list, which this opens.
    let entry = |h: &mut Harness, name: &str, key: &'static str| {
        if let Some(segment) = crate::scenario::rect_of(&format!("drawing:note:align:{name}")) {
            return Some(segment.center());
        }
        let widget = crate::scenario::rect_of("dialog:widget:Alignment")?.center();
        click(h, widget);
        solidcraft_ui_registry::handle_at(&format!("dialog:Alignment:{}", solidcraft_ui_text::tr(key)))
    };
    let ok = |h: &mut Harness| {
        let at = crate::scenario::rect_of("dialog:button::OK").expect("OK is drawn").center();
        click(h, at);
    };
    // A note with an alignment: its choices are Left, Center and Right, no entry for the old layout.
    open(&mut h);
    let shown = |name: &str| crate::scenario::rect_of(&format!("drawing:note:align:{name}")).is_some();
    assert!(shown("left") && shown("centre") && shown("right"), "the three choices are drawn");
    assert!(!shown("saved"), "no entry for the old layout on a note that has an alignment");
    crate::drawing_workspace::close_panel(&mut h.app);
    // The same note as an older drawing saved it.
    h.app
        .session
        .edit_drawing("drawing.note.edit", |d| {
            if let Some(at) = d.notes.index_of(id)
                && let Some(note) = d.notes.get_mut(at)
            {
                std::sync::Arc::make_mut(note).align = None;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(align(&h), None);
    open(&mut h);
    let saved = entry(&mut h, "saved", "ui.drawing_note.align_saved").expect("the old layout has its own entry");
    // Picking it, the entry that is selected, changes nothing, and neither does OK.
    click(&mut h, saved);
    let undo = h.app.session.undo.len();
    ok(&mut h);
    assert_eq!((align(&h), h.app.session.undo.len()), (None, undo), "choosing nothing changes nothing");
    open(&mut h);
    let centre = entry(&mut h, "centre", "ui.drawing_note.align_centre").expect("Center is an entry");
    click(&mut h, centre);
    ok(&mut h);
    assert_eq!(align(&h), Some(solidcraft_engine::geom::cell::HAlign::Centre), "Centre is per-line centring");
}

/// A right-click on the sheet opens the menu a sketch's right-click opens: it selects what is under
/// the pointer first, offers Edit and Delete for it, then the tools, then Undo and Redo. On empty
/// paper the selection stays, and a right-click with a tool in hand puts the tool down.
#[test]
fn right_click_opens_the_shared_menu_on_what_is_under_the_pointer() {
    use solidcraft_engine::Sel;
    let (mut h, _) = plate_sheet();
    let dimension = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let read = h.app.run("drawing.dimension.inspect", json!({"dimension": solidcraft_engine::doc::ids::encode(dimension)})).unwrap();
    let corners: Vec<[f64; 2]> = serde_json::from_value(read["layout"]["text_boxes"][0].clone()).unwrap();
    let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
    let screen = |x: f64, y: f64| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let labels = |h: &Harness| -> Vec<String> {
        crate::context_menu::describe(&h.app)["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["label"].as_str().map(str::to_owned))
            .collect()
    };
    let picked = |h: &Harness| -> Vec<u64> {
        h.app.session.selection.iter().filter_map(|s| if let Sel::Drawing { object, .. } = s { Some(*object) } else { None }).collect()
    };

    h.step(&json!({"click": screen(x, y), "button": "right"})).unwrap();
    h.frames(3);
    assert_eq!(picked(&h), [dimension], "the right-click selects the dimension under it");
    let rows = labels(&h);
    assert_eq!(&rows[..2], ["Edit Text", "Delete"], "{rows:?}");
    for row in ["Base View", "Dimension", "Note", "Undo", "Redo"] {
        assert!(rows.iter().any(|label| label == row), "{row} is offered: {rows:?}");
    }
    let undo = h.app.session.undo.len();
    crate::context_menu::pick(&mut h.app, "Delete").unwrap();
    h.frames(4);
    assert!(h.app.session.drawing("drawing.inspect").unwrap().dimension(dimension).is_none(), "the menu's Delete is the Delete key's");
    assert_eq!(h.app.session.undo.len(), undo + 1, "in one undo step");

    // Empty paper: no Edit or Delete, the tools and Undo; Undo brings the dimension back.
    h.step(&json!({"click": screen(300.0, 60.0), "button": "right"})).unwrap();
    h.frames(3);
    let rows = labels(&h);
    assert_eq!(rows.first().map(String::as_str), Some("Base View"), "{rows:?}");
    crate::context_menu::pick(&mut h.app, "Undo").unwrap();
    h.settle();
    assert!(h.app.session.drawing("drawing.inspect").unwrap().dimension(dimension).is_some());

    // A view's menu edits it; the selection of several things keeps to Delete.
    h.step(&json!({"click": screen(140.0, 180.0), "button": "right"})).unwrap();
    h.frames(3);
    let rows = labels(&h);
    assert_eq!(&rows[..3], ["Edit View", "Projected View", "Delete"], "{rows:?}");
    crate::context_menu::pick(&mut h.app, "Edit View").unwrap();
    h.frames(3);
    assert!(crate::drawing_workspace::inspector_visible(&h.app), "Edit opens the view's options, as a double-click does");
    crate::drawing_workspace::close_panel(&mut h.app);

    // The menu's tools start as their ribbon buttons do. With a tool in hand a right-click puts
    // it down, as it finishes a sketch tool.
    h.step(&json!({"click": screen(300.0, 60.0), "button": "right"})).unwrap();
    h.frames(3);
    crate::context_menu::pick(&mut h.app, "Note").unwrap();
    h.frames(3);
    assert!(crate::drawing_workspace::placing_note(&h.app));
    h.step(&json!({"click": screen(300.0, 60.0), "button": "right"})).unwrap();
    h.frames(3);
    assert!(!crate::drawing_workspace::placing_note(&h.app));
    assert_eq!(crate::context_menu::describe(&h.app)["open"], false, "and opens no menu");

    // `ui.menu` at a point opens the same menu on what is there.
    h.call("ui.menu", json!({"x": screen(140.0, 180.0)[0], "y": screen(140.0, 180.0)[1]}));
    h.frames(4);
    assert_eq!(labels(&h).first().map(String::as_str), Some("Edit View"));
    h.call("ui.menu", json!({"close": true}));
}

/// A double-click on a dimension or a note edits its text where it is, in the box a sketch
/// dimension's value is typed in: Enter changes it in one undo step, Esc leaves it, and the
/// number typed back (or nothing) shows the measured number again.
#[test]
fn double_click_edits_a_dimensions_or_a_notes_text_in_place() {
    let (mut h, _) = plate_sheet();
    h.frames(4);
    let encode = solidcraft_engine::doc::ids::encode;
    let dimension = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let note = h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR"})).unwrap();
    let note = solidcraft_engine::doc::ids::decode(note["note"].as_str().unwrap()).unwrap();
    settle(&mut h);
    let screen = |x: f64, y: f64| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let read = |h: &mut Harness| h.app.run("drawing.dimension.inspect", json!({"dimension": encode(dimension)})).unwrap();
    let text_at = |h: &mut Harness| {
        let corners: Vec<[f64; 2]> = serde_json::from_value(read(h)["layout"]["text_boxes"][0].clone()).unwrap();
        let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
        screen(x, y)
    };
    let measured = read(&mut h)["text"].as_str().unwrap().to_owned();
    let type_over = |h: &mut Harness, at: [f32; 2], text: &str, end: &str| {
        h.step(&json!({"click": at, "double": true})).unwrap();
        h.frames(4);
        assert!(crate::dim_view::editing(), "the double-click opens the box");
        assert!(!crate::drawing_workspace::inspector_visible(&h.app), "not the side panel");
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        if text.is_empty() {
            h.step(&json!({"key": "Delete"})).unwrap();
        } else {
            h.step(&json!({"text": text})).unwrap();
        }
        h.step(&json!({"key": end})).unwrap();
        h.frames(4);
        assert!(!crate::dim_view::editing(), "{end} closes the box");
    };

    let undo = h.app.session.undo.len();
    let at = text_at(&mut h);
    type_over(&mut h, at, "100 TYP", "Enter");
    assert_eq!(read(&mut h)["text"], "100 TYP");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    let at = text_at(&mut h);
    type_over(&mut h, at, "nothing", "Escape");
    assert_eq!(read(&mut h)["text"], "100 TYP", "Esc leaves the text");
    // Another text over the first is one step back to the first; the measured number typed over
    // a text shows the number again, measured, not kept as a text of its own.
    let undo = h.app.session.undo.len();
    let at = text_at(&mut h);
    type_over(&mut h, at, "SEE NOTE", "Enter");
    assert_eq!(read(&mut h)["text"], "SEE NOTE");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.settle();
    assert_eq!(read(&mut h)["text"], "100 TYP", "undone in one step, to the text before");
    let at = text_at(&mut h);
    type_over(&mut h, at, &measured, "Enter");
    assert!(read(&mut h)["record"]["override_text"].is_null(), "the number typed over a text is measured again");
    h.step(&json!({"key": "Z", "cmd": true})).unwrap();
    h.settle();
    assert_eq!(read(&mut h)["text"], "100 TYP");
    // The box holds the text as a template: `{value}` is the measured number, live.
    let at = text_at(&mut h);
    type_over(&mut h, at, "4× {value} TYP", "Enter");
    assert_eq!(read(&mut h)["text"], format!("4× {measured} TYP").as_str());
    assert_eq!(read(&mut h)["status"], "ok", "it still measures");
    let at = text_at(&mut h);
    type_over(&mut h, at, "", "Enter");
    assert_eq!(read(&mut h)["text"], measured.as_str(), "an empty text shows the measured number again");
    assert!(read(&mut h)["record"]["override_text"].is_null());
    let undo = h.app.session.undo.len();
    let at = text_at(&mut h);
    type_over(&mut h, at, &measured, "Enter");
    assert!(read(&mut h)["record"]["override_text"].is_null(), "the number typed back stays measured");
    assert_eq!(h.app.session.undo.len(), undo, "and is no step");

    // A note: its text box is to the right of and below where it is placed.
    let bounds = {
        let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
        let view = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
        view.objects.iter().find(|object| object.object == note).unwrap().bounds_mm
    };
    let at = screen((bounds.min.x + bounds.max.x) / 2.0, (bounds.min.y + bounds.max.y) / 2.0);
    type_over(&mut h, at, "DEBURR ALL EDGES", "Enter");
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    assert_eq!(drawing.notes.by_id(note).unwrap().content.0, vec![solidcraft_engine::geom::cell::Part::Text("DEBURR ALL EDGES".to_owned())]);

    // A view has no one text: its options open, as before.
    h.step(&json!({"click": screen(140.0, 180.0), "double": true})).unwrap();
    h.frames(4);
    assert!(!crate::dim_view::editing() && crate::drawing_workspace::inspector_visible(&h.app));
}

/// The one thing selected shows its options in the right panel, where a tool shows its own, and
/// one Esc clears the selection and the panel with it, as in a sketch. A dimension's options apply:
/// its offset changes in one undo step.
#[test]
fn the_selected_object_shows_its_options_and_a_dimensions_apply() {
    let (mut h, _) = plate_sheet();
    let encode = solidcraft_engine::doc::ids::encode;
    let dimension = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let screen = |x: f64, y: f64| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let read = |h: &mut Harness| h.app.run("drawing.dimension.inspect", json!({"dimension": encode(dimension)})).unwrap();
    let corners: Vec<[f64; 2]> = serde_json::from_value(read(&mut h)["layout"]["text_boxes"][0].clone()).unwrap();
    let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
    assert!(!crate::drawing_workspace::inspector_visible(&h.app));

    h.step(&json!({"click": screen(x, y)})).unwrap();
    h.frames(4);
    assert!(crate::drawing_workspace::inspector_visible(&h.app), "the selected dimension's options show");
    assert_eq!(crate::drawing_workspace::inspector_title(&h.app), "Dimension");
    let offset = read(&mut h)["record"]["offset_mm"].as_f64().unwrap();
    let field = h.widget(&format!("{offset} mm")).expect("the Offset field shows the dimension's offset");
    let undo = h.app.session.undo.len();
    h.step(&json!({"click": field})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": format!("{}", offset * 2.0)})).unwrap();
    let apply = h.widget("Apply").unwrap();
    h.step(&json!({"click": apply})).unwrap();
    h.frames(4);
    assert_eq!(read(&mut h)["record"]["offset_mm"].as_f64(), Some(offset * 2.0), "Apply moves the dimension line");
    assert_eq!(h.app.session.undo.len(), undo + 1, "in one undo step");
    assert!(crate::drawing_workspace::inspector_visible(&h.app), "Apply keeps the panel");

    // One Esc, as in a sketch: the selection goes, and its options with it.
    h.step(&json!({"click": screen(300.0, 250.0)})).unwrap();
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(4);
    assert!(h.app.session.selection.is_empty());
    assert!(!crate::drawing_workspace::inspector_visible(&h.app));

    // A view's options likewise; two things selected have no one panel.
    h.step(&json!({"click": screen(140.0, 180.0)})).unwrap();
    h.frames(4);
    assert_eq!(crate::drawing_workspace::inspector_title(&h.app), "Edit View");
    let corners: Vec<[f64; 2]> = serde_json::from_value(read(&mut h)["layout"]["text_boxes"][0].clone()).unwrap();
    let (x, y) = corners.iter().fold((0.0, 0.0), |sum, p| (sum.0 + p[0] / 4.0, sum.1 + p[1] / 4.0));
    h.step(&json!({"click": screen(x, y), "shift": true})).unwrap();
    h.frames(4);
    assert_eq!(h.app.session.selection.len(), 2);
    assert!(!crate::drawing_workspace::inspector_visible(&h.app));

    // A tool in hand keeps its own panel whatever is selected.
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.note"));
    h.frames(2);
    assert_eq!(crate::drawing_workspace::inspector_title(&h.app), "Note");
    assert!(crate::drawing_workspace::placing_note(&h.app));
}

/// A double-click on a text cell of the title block edits what it shows, in the box a dimension's
/// text is typed in: Enter keeps the text over the cell's own value in one undo step, and nothing
/// typed shows the cell's own value again. The projection symbol's cell has no text to edit.
#[test]
fn double_click_edits_a_title_block_cells_text_in_place() {
    use solidcraft_engine::geom::title_block::CellKind;
    let (mut h, _) = plate_sheet();
    let centre = |h: &Harness, text: bool| {
        let drawing = h.app.session.drawing("drawing.inspect").unwrap();
        let record = &drawing.sheets[0];
        let (block, placed) = (record.title_block.as_ref().unwrap(), record.title_block_layout().unwrap().unwrap());
        let at =
            block.cells.iter().position(|cell| if text { cell.name == "title" } else { matches!(cell.kind, CellKind::ProjectionSymbol) }).unwrap();
        let rect = placed.cells[at].rect;
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        let (x, y) = ((rect.min.x + rect.max.x) / 2.0, (rect.min.y + rect.max.y) / 2.0);
        [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
    };
    let typed = |h: &Harness| h.app.session.drawing("drawing.inspect").unwrap().sheets[0].title_block_overrides.get("title").cloned();
    let type_over = |h: &mut Harness, text: &str| {
        h.step(&json!({"click": centre(h, true), "double": true})).unwrap();
        h.frames(4);
        assert!(crate::dim_view::editing(), "the double-click opens the box on the Title cell");
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        if text.is_empty() {
            h.step(&json!({"key": "Delete"})).unwrap();
        } else {
            h.step(&json!({"text": text})).unwrap();
        }
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(4);
        assert!(!crate::dim_view::editing());
    };
    assert_eq!(typed(&h), None);
    let undo = h.app.session.undo.len();
    type_over(&mut h, "MOUNTING PLATE");
    assert_eq!(typed(&h).as_deref(), Some("MOUNTING PLATE"));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    type_over(&mut h, "MOUNTING PLATE");
    assert_eq!(h.app.session.undo.len(), undo + 1, "the same text again is no step");
    type_over(&mut h, "");
    assert_eq!(typed(&h), None, "nothing typed shows the cell's own value again");

    h.step(&json!({"click": centre(&h, false), "double": true})).unwrap();
    h.frames(4);
    assert!(!crate::dim_view::editing(), "the projection symbol has no text");
}

/// ISO 7200's cells (#20121) are edited where the others are: a double-click on each of the six opens
/// the box, and Enter types the text over the cell, which shows it on the sheet.
#[test]
fn double_click_edits_each_iso_7200_cell_in_place() {
    let (mut h, _) = plate_sheet();
    for (name, text) in [
        ("scale", "NTS"),
        ("document_type", "Fabrication drawing"),
        ("document_status", "Released"),
        ("language", "DE"),
        ("revision", "C"),
        ("department", "Sheet metal"),
    ] {
        let at = {
            let drawing = h.app.session.drawing("drawing.inspect").unwrap();
            let record = &drawing.sheets[0];
            let (block, placed) = (record.title_block.as_ref().unwrap(), record.title_block_layout().unwrap().unwrap());
            let i = block.cells.iter().position(|cell| cell.name == name).unwrap();
            let rect = placed.cells[i].rect;
            let paper = crate::scenario::rect_of("sheet:paper").unwrap();
            let (x, y) = ((rect.min.x + rect.max.x) / 2.0, (rect.min.y + rect.max.y) / 2.0);
            [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
        };
        h.step(&json!({"click": at, "double": true})).unwrap();
        h.frames(4);
        assert!(crate::dim_view::editing(), "the double-click opens the box on the {name} cell");
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": text})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(4);
        assert!(!crate::dim_view::editing());
        let typed = h.app.session.drawing("drawing.inspect").unwrap().sheets[0].title_block_overrides.get(name).cloned();
        assert_eq!(typed.as_deref(), Some(text), "{name}");
    }
}

/// A view just placed says so before its lines arrive: it is listed as waiting and shows its box
/// with Projecting…, and that goes once it is drawn. Placing the standard views and drawing them
/// evaluates the model no more times than it was evaluated before.
#[test]
fn a_view_waiting_for_its_first_lines_says_projecting() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.frames(2);
    let evaluations = h.app.session.evaluation_count();
    h.app.run("drawing.create", json!({"standard_views": true})).unwrap();
    let placed: Vec<String> =
        h.app.session.drawing("drawing.inspect").unwrap().views.iter().map(|view| solidcraft_engine::doc::ids::encode(view.id)).collect();
    assert_eq!(placed.len(), 4);
    // The first frame: one view's job has only just been asked for, so none is drawn yet.
    h.frame();
    let first = crate::drawing_sheet_page::inspect(&h.app);
    let waiting: Vec<&str> = first["waiting_views"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(waiting.len(), 4, "every placed view is waiting: {first}");
    for view in &placed {
        assert!(crate::scenario::rect_of(&format!("sheet:view:{view}:state")).is_some(), "view {view} says Projecting…");
    }
    settle(&mut h);
    assert_eq!(page(&mut h)["waiting_views"], json!([]), "none waits once they are drawn");
    assert_eq!(h.app.session.evaluation_count(), evaluations, "placing and drawing the views evaluates the model no further");
}

#[test]
fn dimension_tool_places_a_view_edge_length() {
    let (mut h, front) = plate_sheet();
    let existing = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    let screen = |x: f32, y: f32| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        [paper.left() + paper.width() * x / 420.0, paper.bottom() - paper.height() * y / 297.0]
    };
    h.step(&json!({"click": screen(140.0, 175.0)})).unwrap();
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), 1);
    h.frames(4);
    h.step(&json!({"click": screen(140.0, 163.0)})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    assert_eq!(drawing.dimensions.len(), 2);
    let placed = drawing.dimensions.iter().find(|dimension| dimension.id != existing).unwrap();
    assert_eq!(solidcraft_engine::doc::ids::encode(placed.view), front);
    assert!((placed.offset_mm.abs() - 12.0).abs() < 0.75, "offset was {} mm", placed.offset_mm);
}

/// The plate's sheet with a hole 12 across through the plate's middle, seen face on in the front
/// view: on paper (1:2, centred at 140, 180) the plate's bottom edge is y = 175, its top y = 185, its
/// left side x = 115 and the hole a circle of radius 3 about (140, 180). The Dimension tool is on.
fn holed_plate_dimensioning() -> (Harness, String) {
    let (mut h, front) = plate_sheet();
    crate::drawing_workspace::enter(&mut h.app, false).unwrap();
    let cut = json!({"radius": 6, "height": 100, "base": [50, -10, 10], "axis": "Y", "operation": "cut", "targets": ["Plate"], "name": "Hole"});
    h.app.run("solid.cylinder", cut).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    (h, front)
}

/// The same face-on plate in a nested component. STEP imports also put their bodies in
/// components, so their picks need a nonempty occurrence path all the way to the command.
fn component_plate_dimensioning() -> (Harness, Value) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    let assembly = h.app.run("component.create", json!({"name": "Assembly"})).unwrap()["occurrence"].clone();
    let part = h.app.run("component.create", json!({"name": "Part"})).unwrap()["occurrence"].clone();
    h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
    h.app
        .run("solid.cylinder", json!({"radius": 6, "height": 100, "base": [50, -10, 10], "axis": "Y", "operation": "cut", "targets": ["Plate"]}))
        .unwrap();
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    h.app.run("drawing.create", json!({"first_sheet_paper": {"size": {"standard": "a3"}, "orientation": "landscape", "dpi": 300}})).unwrap();
    let drawing = h.app.run("drawing.inspect", json!({})).unwrap();
    assert_eq!(drawing["sheets"][0]["size_mm"], json!([420.0, 297.0]), "the paper coordinates below are A3 landscape");
    h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "view": "front", "scale": "1:2", "position_mm": [140, 180]})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    (h, json!([assembly, part]))
}

#[test]
fn dimension_tool_measures_a_nested_component_edge() {
    let (mut h, path) = component_plate_dimensioning();
    click_paper(&mut h, 125.0, 175.0);
    click_paper(&mut h, 140.0, 160.0);
    let read = added_dimension(&mut h, &[]);
    assert_eq!(read["status"], "ok", "{read}");
    assert!((number(&read["value"]) - 100.0).abs() < 1e-6, "{read}");
    assert_eq!(read["record"]["kind"]["length"]["edge"]["path"], path, "{read}");
}

#[test]
fn dimension_tool_measures_a_nested_component_hole() {
    let (mut h, path) = component_plate_dimensioning();
    click_paper(&mut h, 143.0, 180.0);
    click_paper(&mut h, 150.0, 192.0);
    let read = added_dimension(&mut h, &[]);
    assert_eq!(read["text"], "Ø12.00", "{read}");
    assert!((number(&read["value"]) - 12.0).abs() < 1e-6, "{read}");
    assert_eq!(read["record"]["kind"]["radial"]["edge"]["path"], path, "{read}");
}

#[test]
fn dimension_tool_measures_a_nested_component_centre_to_edge() {
    let (mut h, path) = component_plate_dimensioning();
    click_paper(&mut h, 143.0, 180.0);
    click_paper(&mut h, 115.0, 180.0);
    click_paper(&mut h, 125.0, 160.0);
    let read = added_dimension(&mut h, &[]);
    assert_eq!(read["status"], "ok", "{read}");
    assert!((number(&read["value"]) - 50.0).abs() < 1e-6, "{read}");
    for point in ["a", "b"] {
        assert_eq!(read["record"]["kind"]["linear"][point]["path"], path, "{point}: {read}");
    }
}

/// The silhouette's face needs the same occurrence path as a component's edges.
#[test]
fn dimension_tool_measures_a_nested_component_cylinder_outline() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    let assembly = h.app.run("component.create", json!({"name": "Assembly"})).unwrap()["occurrence"].clone();
    let part = h.app.run("component.create", json!({"name": "Part"})).unwrap()["occurrence"].clone();
    h.app.run("solid.cylinder", json!({"radius": 6, "height": 100, "base": [0, 0, 0], "axis": "X", "body_name": "Rod"})).unwrap();
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    h.app.run("drawing.create", json!({"first_sheet_paper": {"size": {"standard": "a3"}, "orientation": "landscape", "dpi": 300}})).unwrap();
    h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "view": "front", "scale": "1:2", "position_mm": [140, 180]})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    let sheet = solidcraft_engine::doc::ids::encode(h.app.session.drawing("drawing.inspect").unwrap().sheets[0].id);
    let line = h.app.run("drawing.scene.line", json!({"sheet": sheet, "point_mm": [140, 183], "band_mm": 0.5})).unwrap();
    assert_eq!(line["line"]["source"]["kind"], "silhouette", "{line}");
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    click_paper(&mut h, 140.0, 183.0);
    click_paper(&mut h, 100.0, 180.0);
    let read = added_dimension(&mut h, &[]);
    assert_eq!(read["status"], "ok", "{read}");
    assert!((number(&read["value"]) - 12.0).abs() < 1e-6, "{read}");
    assert_eq!(read["text"], "Ø12.00", "{read}");
    assert_eq!(read["record"]["kind"]["cylinder"]["face"]["path"], json!([assembly, part]), "{read}");
}

/// A click at paper millimetres (`x`, `y`) of the A3 sheet, a frame drawn after it.
fn click_paper(h: &mut Harness, x: f32, y: f32) {
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let at = [paper.left() + paper.width() * x / 420.0, paper.bottom() - paper.height() * y / 297.0];
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
}

/// The dimension the tool added last, as inspect reads it.
fn added_dimension(h: &mut Harness, before: &[u64]) -> Value {
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    assert_eq!(drawing.dimensions.len(), before.len() + 1, "one dimension was added");
    let id = drawing.dimensions.iter().map(|d| d.id).find(|id| !before.contains(id)).unwrap();
    h.app.run("drawing.dimension.inspect", json!({"dimension": solidcraft_engine::doc::ids::encode(id)})).unwrap()
}

/// With the Dimension tool in hand, the edge under the pointer is found before any click, to be
/// shaded as a sketch shades the curve under the pointer: a straight edge, a circle, nothing over
/// empty paper. It is the edge a click then picks, and it goes when the tool is put down.
#[test]
fn dimension_tool_shades_the_edge_under_the_pointer_before_the_click() {
    use crate::drawing_workspace::dimension::{Shape, hovered_shape, picked_shapes};
    let (mut h, _) = holed_plate_dimensioning();
    let hover = |h: &mut Harness, x: f32, y: f32| {
        let paper = crate::scenario::rect_of("sheet:paper").unwrap();
        h.step(&json!({"move": [paper.left() + paper.width() * x / 420.0, paper.bottom() - paper.height() * y / 297.0]})).unwrap();
        h.frames(2);
        hovered_shape(&h.app)
    };
    assert!(matches!(hover(&mut h, 125.0, 175.0), Some(Shape::Straight(_))), "the plate's bottom edge");
    assert!(picked_shapes(&h.app).is_empty(), "shaded, not picked");
    assert!(matches!(hover(&mut h, 143.0, 180.0), Some(Shape::Round(_))), "the hole's circle");
    assert_eq!(hover(&mut h, 128.0, 180.0), None, "empty paper inside the view");
    assert_eq!(hover(&mut h, 60.0, 60.0), None, "empty paper off the view");

    let shaded = hover(&mut h, 125.0, 175.0);
    click_paper(&mut h, 125.0, 175.0);
    assert_eq!(picked_shapes(&h.app).first().copied(), shaded, "a click picks the edge that was shaded");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert_eq!(hovered_shape(&h.app), None, "nothing stays shaded once the tool is down");
}

/// Two edges picked one after the other dimension the distance between them, as in a sketch: the
/// plate's bottom and top edges, parallel, read its 20 mm height, measured square to them
/// (vertical), with the line through the third click.
#[test]
fn dimension_tool_dimensions_between_two_parallel_edges() {
    let (mut h, _) = holed_plate_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 125.0, 175.0);
    click_paper(&mut h, 125.0, 185.0);
    use crate::drawing_workspace::dimension::{Shape, picked_shapes};
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Straight(_), Shape::Straight(_)]), "{:?}", picked_shapes(&h.app));
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), before.len(), "two picks place nothing yet");
    click_paper(&mut h, 100.0, 180.0);
    let read = added_dimension(&mut h, &before);
    assert!((number(&read["value"]) - 20.0).abs() < 1e-6, "{read}");
    assert_eq!(read["record"]["kind"]["linear"]["direction"], "vertical", "{read}");
    assert!(picked_shapes(&h.app).is_empty(), "the tool starts again, as the sketch's does");
}

/// Two edges that aren't parallel dimension the angle between them: the plate's bottom edge and its
/// left side, 90°.
#[test]
fn dimension_tool_dimensions_the_angle_between_two_edges() {
    let (mut h, _) = holed_plate_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 125.0, 175.0);
    click_paper(&mut h, 115.0, 179.0);
    click_paper(&mut h, 122.0, 181.0);
    let read = added_dimension(&mut h, &before);
    assert!(read["record"]["kind"]["angle"].is_object(), "{read}");
    assert!((number(&read["value"]) - 90.0).abs() < 1e-6, "{read}");
}

/// One round edge dimensions its size: the hole's rim, a whole circle, reads Ø12, its text where
/// the click placed it.
#[test]
fn dimension_tool_dimensions_a_holes_diameter() {
    let (mut h, _) = holed_plate_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 143.0, 180.0);
    use crate::drawing_workspace::dimension::{Shape, picked_shapes};
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Round(_)]), "{:?}", picked_shapes(&h.app));
    click_paper(&mut h, 150.0, 192.0);
    let read = added_dimension(&mut h, &before);
    assert!(read["record"]["kind"]["radial"].is_object(), "{read}");
    assert_eq!(read["text"], "Ø12.00", "{read}");
}

/// #21034: the curve where two corner fillets meet is neither a line nor a face-on circle, and the
/// engine measures neither a length nor a size for it. The tool says so and picks nothing, rather
/// than sending a length the engine refuses. A 30×20×10 box, 1:1 front view centred at (140, 180):
/// the curve's point (0.586, 0.586, 9.414) is on paper at (125.586, 184.414).
#[test]
fn dimension_tool_refuses_a_curve_it_cannot_dimension_with_a_reason() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.app.run("solid.box", json!({"length": 30, "width": 20, "height": 10, "body_name": "Block"})).unwrap();
    h.app.run("solid.fillet", json!({"edges": [[15, 0, 10], [0, 10, 10]], "radius": 2, "tangent_chain": false})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "view": "front", "scale": "1:1", "position_mm": [140, 180]})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    let sheet = solidcraft_engine::doc::ids::encode(h.app.session.drawing("drawing.inspect").unwrap().sheets[0].id);
    let line = h.app.run("drawing.scene.line", json!({"sheet": sheet, "point_mm": [125.586, 184.414], "band_mm": 0.3})).unwrap();
    assert_eq!(line["line"]["source"]["kind"], "edge", "{line}");
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    let before = h.app.session.drawing("drawing.inspect").unwrap().dimensions.len();
    click_paper(&mut h, 125.586, 184.414);
    use crate::drawing_workspace::dimension::picked_shapes;
    assert!(picked_shapes(&h.app).is_empty(), "{:?}", picked_shapes(&h.app));
    let status = h.app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    assert!(status.contains("can't be dimensioned"), "{status}");
    click_paper(&mut h, 100.0, 180.0);
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), before, "nothing is placed");
}

/// A Ø12 rod 100 long along X, seen side on in a 1:2 front view centred at (140, 180) on A3: on paper
/// its two outline lines (silhouettes, not edges) run from x = 115 to 165 at y = 177 and 183, and its
/// ends, circles seen edge on, are the edges x = 115 and x = 165. The Dimension tool is on.
fn rod_dimensioning() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.app.run("solid.cylinder", json!({"radius": 6, "height": 100, "base": [0, 0, 0], "axis": "X", "body_name": "Rod"})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    h.app.run("drawing.view.base", json!({"sheet": "Sheet 1", "scale": "1:2", "position_mm": [140, 180]})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    settle(&mut h);
    let sheet = solidcraft_engine::doc::ids::encode(h.app.session.drawing("drawing.inspect").unwrap().sheets[0].id);
    for y in [177.0, 183.0] {
        let line = h.app.run("drawing.scene.line", json!({"sheet": sheet, "point_mm": [140.0, y], "band_mm": 0.5})).unwrap();
        assert_eq!(line["line"]["source"]["kind"], "silhouette", "an outline line at y = {y}: {line}");
    }
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    h
}

/// A cylinder seen side on is dimensioned by its outline, as on paper (ISO 129-1): the outline line
/// under the pointer is shaded, a click picks it, and the next click places the rod's Ø12 there.
#[test]
fn dimension_tool_dimensions_a_cylinders_diameter_from_one_outline() {
    use crate::drawing_workspace::dimension::{Shape, hovered_shape, picked_shapes};
    let mut h = rod_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    h.step(&json!({"move": [paper.left() + paper.width() * 140.0 / 420.0, paper.bottom() - paper.height() * 183.0 / 297.0]})).unwrap();
    h.frames(2);
    assert!(matches!(hovered_shape(&h.app), Some(Shape::Straight(_))), "the outline line is shaded: {:?}", hovered_shape(&h.app));
    click_paper(&mut h, 140.0, 183.0);
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Straight(_)]), "{:?}", picked_shapes(&h.app));
    // Past the rod's left end: the engine draws a Ø between outlines beyond their ends.
    click_paper(&mut h, 100.0, 180.0);
    let read = added_dimension(&mut h, &before);
    assert!(read["record"]["kind"]["cylinder"].is_object(), "{read}");
    assert_eq!(read["text"], "Ø12.00", "{read}");
}

/// Both outline lines of one cylinder, picked one after the other, are its diameter: one Ø12, not a
/// distance between two lines.
#[test]
fn dimension_tool_dimensions_a_cylinders_diameter_from_its_two_outlines() {
    use crate::drawing_workspace::dimension::{Shape, picked_shapes};
    let mut h = rod_dimensioning();
    let before: Vec<u64> = h.app.session.drawing("drawing.inspect").unwrap().dimensions.iter().map(|d| d.id).collect();
    click_paper(&mut h, 140.0, 183.0);
    click_paper(&mut h, 140.0, 177.0);
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Straight(_), Shape::Straight(_)]), "{:?}", picked_shapes(&h.app));
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), before.len(), "two picks place nothing yet");
    click_paper(&mut h, 100.0, 180.0);
    let read = added_dimension(&mut h, &before);
    assert!(read["record"]["kind"]["cylinder"].is_object(), "{read}");
    assert_eq!(read["text"], "Ø12.00", "{read}");
}

/// An outline line and an edge measure nothing the drawing has a dimension for: the second pick is
/// refused with a reason, the first kept, and no place on the sheet is asked for (#20181). The rod's
/// outline, then its left end.
#[test]
fn dimension_tool_refuses_an_outline_and_an_edge_saying_why() {
    use crate::drawing_workspace::dimension::{Shape, picked_shapes};
    let mut h = rod_dimensioning();
    let before = h.app.session.drawing("drawing.inspect").unwrap().dimensions.len();
    click_paper(&mut h, 140.0, 183.0);
    click_paper(&mut h, 115.0, 180.0);
    assert!(matches!(picked_shapes(&h.app)[..], [Shape::Straight(_)]), "only the outline stays picked: {:?}", picked_shapes(&h.app));
    let status = h.app.status.as_ref().map(|(text, _, _)| text.clone()).unwrap_or_default();
    assert_eq!(status, solidcraft_ui_text::tr("ui.drawing_dimension_tool.measures_nothing"));
    click_paper(&mut h, 100.0, 180.0);
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), before + 1, "the outline alone still places its Ø");
}

/// A Ø between a cylinder's outline lines goes past one of its ends: with the pointer inside the
/// rod's length, the tool says so before the click, in the engine's words without the command's
/// name, rather than showing nothing (#20181).
#[test]
fn dimension_tool_says_why_it_cannot_place_before_the_click() {
    use crate::drawing_workspace::dimension::refusal;
    let mut h = rod_dimensioning();
    let before = h.app.session.drawing("drawing.inspect").unwrap().dimensions.len();
    click_paper(&mut h, 140.0, 183.0);
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    h.step(&json!({"move": [paper.left() + paper.width() * 140.0 / 420.0, paper.bottom() - paper.height() * 190.0 / 297.0]})).unwrap();
    h.frames(2);
    let why = refusal(&h.app).map(str::to_owned).unwrap_or_default();
    assert!(why.contains("past one of them") && !why.contains("drawing.dimension.add"), "{why:?}");
    // Past the left end it places, and the reason is gone.
    h.step(&json!({"move": [paper.left() + paper.width() * 100.0 / 420.0, paper.bottom() - paper.height() * 180.0 / 297.0]})).unwrap();
    h.frames(2);
    assert_eq!(refusal(&h.app), None);
    click_paper(&mut h, 100.0, 180.0);
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), before + 1);
}

/// Esc drops the tool's picks first and keeps the tool, as in a sketch; the next Esc leaves it.
#[test]
fn dimension_tool_escape_drops_picks_then_the_tool() {
    let (mut h, _) = holed_plate_dimensioning();
    click_paper(&mut h, 125.0, 175.0);
    assert_eq!(crate::drawing_workspace::dimension::picked_shapes(&h.app).len(), 1);
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(crate::drawing_workspace::dimension::picked_shapes(&h.app).is_empty());
    assert!(crate::drawing_workspace::placing_dimension(&h.app), "the tool stays on");
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(!crate::drawing_workspace::placing_dimension(&h.app));
}

/// Paper millimetres (`x`, `y`) of the A3 sheet on screen.
fn on_screen(x: f64, y: f64) -> [f32; 2] {
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    [paper.left() + paper.width() * x as f32 / 420.0, paper.bottom() - paper.height() * y as f32 / 297.0]
}

/// The text centre of drawing dimension `id`, as inspect lays it out, and the whole answer.
fn text_of(h: &mut Harness, id: u64) -> (Vec2, Value) {
    let read = h.app.run("drawing.dimension.inspect", json!({"dimension": solidcraft_engine::doc::ids::encode(id)})).unwrap();
    let corners: Vec<[f64; 2]> = serde_json::from_value(read["layout"]["text_boxes"][0].clone()).unwrap();
    let centre = corners.iter().fold(Vec2::ZERO, |sum, p| sum + Vec2::new(p[0], p[1])) * 0.25;
    (centre, read)
}

/// With the Dimension tool in hand a placed dimension still drags, as in a sketch: its text
/// follows the pointer in one undo step, the tool stays in hand, and nothing is picked or added.
/// Over the dimension's text it is the dimension that lights up, not the edge under it.
#[test]
fn a_placed_dimension_drags_while_the_dimension_tool_is_in_hand() {
    let (mut h, _) = plate_sheet();
    let id = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    let (from, before) = text_of(&mut h, id);
    h.step(&json!({"move": on_screen(from.x, from.y)})).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h)["hover"]["object"], solidcraft_engine::doc::ids::encode(id), "the dimension is what lights up");
    assert_eq!(crate::drawing_workspace::dimension::hovered_shape(&h.app), None, "no edge is shaded under its text");
    let undo = h.app.session.undo.len();
    h.step(&json!({"drag": [on_screen(from.x, from.y), on_screen(from.x + 15.0, from.y + 12.0)], "steps": 6})).unwrap();
    h.frames(2);
    assert!(crate::dim_view::dragged().is_none(), "the drag ended");
    let (to, after) = text_of(&mut h, id);
    assert!((to - (from + Vec2::new(15.0, 12.0))).len() < 1.0, "the text followed the pointer: {from:?} -> {to:?}");
    assert!(number(&after["record"]["offset_mm"]) > number(&before["record"]["offset_mm"]) + 10.0, "{before}\n{after}");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the whole drag");
    assert!(crate::drawing_workspace::placing_dimension(&h.app), "the tool is still in hand");
    assert!(crate::drawing_workspace::dimension::picked_shapes(&h.app).is_empty(), "nothing was picked");
    assert_eq!(h.app.session.drawing("drawing.inspect").unwrap().dimensions.len(), 1, "and no dimension was added");

    // A click on an edge still picks for a new dimension: the edge the placed dimension measures
    // too, which lies inside its box but is not its text.
    click_paper(&mut h, 150.0, 185.0);
    assert_eq!(crate::drawing_workspace::dimension::picked_shapes(&h.app).len(), 1, "the top edge, under the dimension's lines");
}

/// With the Dimension tool in hand a double-click changes no mode. On a placed dimension's text it
/// opens the box its text is typed in, and the tool is still in hand after Enter and after Esc. On
/// an edge it is one pick, not a pick and a dimension dropped on the edge. On a view's inside and
/// on empty paper it does nothing.
#[test]
fn a_double_click_with_the_dimension_tool_in_hand_changes_no_mode() {
    let (mut h, _) = plate_sheet();
    let id = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    assert!(crate::drawing_workspace::start(&mut h.app, "drawing.dimension"));
    h.frames(4);
    let tool = |h: &Harness| crate::drawing_workspace::placing_dimension(&h.app) && crate::drawing_workspace::inspector_title(&h.app) == "Dimension";
    let count = |h: &Harness| h.app.session.drawing("drawing.inspect").unwrap().dimensions.len();
    let double = |h: &mut Harness, x: f64, y: f64| {
        h.step(&json!({"move": on_screen(x, y)})).unwrap();
        h.frames(2);
        h.step(&json!({"click": on_screen(x, y), "double": true})).unwrap();
        h.frames(4);
    };

    // Its text: the box, with the tool kept through Enter and through Esc.
    let (text, _) = text_of(&mut h, id);
    double(&mut h, text.x, text.y);
    assert!(crate::dim_view::editing() && tool(&h), "the box opens and the tool stays");
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "100 REF"})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    h.frames(4);
    assert_eq!(text_of(&mut h, id).1["text"], "100 REF");
    assert!(!crate::dim_view::editing() && tool(&h), "Enter keeps the tool");
    let (text, _) = text_of(&mut h, id);
    double(&mut h, text.x, text.y);
    assert!(crate::dim_view::editing());
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(4);
    assert!(!crate::dim_view::editing() && tool(&h), "Esc closes the box and keeps the tool");
    assert_eq!(count(&h), 1);

    // An edge: picked once, and nothing placed on it.
    double(&mut h, 140.0, 175.0);
    assert_eq!(crate::drawing_workspace::dimension::picked_shapes(&h.app).len(), 1, "one pick");
    assert_eq!(count(&h), 1, "no dimension dropped on the edge");
    assert!(tool(&h));
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(tool(&h) && crate::drawing_workspace::dimension::picked_shapes(&h.app).is_empty(), "Esc dropped the pick only");

    // A view's inside and empty paper: nothing at all.
    for (x, y) in [(140.0, 180.0), (300.0, 60.0)] {
        double(&mut h, x, y);
        assert!(tool(&h) && !crate::dim_view::editing(), "({x}, {y}): still the tool, no editor");
        assert!(crate::drawing_workspace::dimension::picked_shapes(&h.app).is_empty() && count(&h) == 1, "({x}, {y}): nothing picked or added");
        assert!(h.app.session.selection.is_empty(), "({x}, {y}): nothing selected");
    }
}

/// A placed dimension's text drags, as a sketch dimension's does: it follows the pointer by the
/// same drag (`dim_view`), and the whole drag is one undo step.
#[test]
fn a_drawing_dimensions_text_drags_like_a_sketch_dimensions() {
    let (mut h, _) = plate_sheet();
    let id = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let (from, before) = text_of(&mut h, id);
    let undo = h.app.session.undo.len();
    h.step(&json!({"drag": [on_screen(from.x, from.y), on_screen(from.x + 15.0, from.y + 12.0)], "steps": 6})).unwrap();
    h.frames(2);
    assert!(crate::dim_view::dragged().is_none(), "the drag ended");
    let (to, after) = text_of(&mut h, id);
    assert!((to - (from + Vec2::new(15.0, 12.0))).len() < 1.0, "the text followed the pointer: {from:?} -> {to:?}\n{after}");
    assert!(number(&after["record"]["offset_mm"]) > number(&before["record"]["offset_mm"]) + 10.0, "{before}\n{after}");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the whole drag");
    h.app.run("edit.undo", json!({})).unwrap();
    let (back, _) = text_of(&mut h, id);
    assert!((back - from).len() < 1e-6, "undo puts it back: {from:?} {back:?}");
}

/// A note drags by the same drag: it follows the pointer, a sheet's note and a view's alike, and
/// the whole drag is one undo step.
#[test]
fn a_note_drags_like_a_sketch_dimensions_text() {
    let (mut h, front) = plate_sheet();
    let encode = solidcraft_engine::doc::ids::encode;
    let on_sheet = h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR"})).unwrap();
    let on_view = h.app.run("drawing.note.add", json!({"anchor": {"view": front}, "at": [10, 60], "content": "SEE DETAIL"})).unwrap();
    for note in [on_sheet, on_view] {
        let id = solidcraft_engine::doc::ids::decode(note["note"].as_str().unwrap()).unwrap();
        settle(&mut h);
        let at = |h: &Harness| Vec2::from(h.app.session.drawing("drawing.inspect").unwrap().notes.by_id(id).unwrap().at);
        let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
        let bounds = drawing_scene::extract(&mut h.app.session, sheet).unwrap().objects.iter().find(|object| object.object == id).unwrap().bounds_mm;
        let grab = (bounds.min + bounds.max) * 0.5;
        let (from, undo) = (at(&h), h.app.session.undo.len());
        h.step(&json!({"drag": [on_screen(grab.x, grab.y), on_screen(grab.x + 20.0, grab.y - 15.0)], "steps": 6})).unwrap();
        h.frames(2);
        assert!(crate::dim_view::dragged().is_none(), "the drag ended");
        let to = at(&h);
        assert!((to - (from + Vec2::new(20.0, -15.0))).len() < 1.0, "note {} followed the pointer: {from:?} -> {to:?}", encode(id));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the whole drag");
        h.app.run("edit.undo", json!({})).unwrap();
        assert!((at(&h) - from).len() < 1e-9, "undo puts it back");
        h.app.run("edit.redo", json!({})).unwrap();
    }
}

/// A drag from one of several selected things moves them all together by the pointer's way, as
/// a sketch drags its selected geometry, in one undo step: here a view, a note on the sheet and a
/// dimension on another view. The view's own dimension goes with it once, not twice.
#[test]
fn a_selection_drags_together_in_one_undo_step() {
    use solidcraft_engine::Sel;
    let (mut h, front) = plate_sheet();
    let decode = |v: &Value, key: &str| solidcraft_engine::doc::ids::decode(v[key].as_str().unwrap()).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap().clone();
    let front_id = solidcraft_engine::doc::ids::decode(&front).unwrap();
    let right = drawing.views.iter().find(|view| view.id != front_id).unwrap().id;
    let on_front = drawing.dimensions[0].id;
    let note = h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR"})).unwrap();
    let note = decode(&note, "note");
    let side =
        h.app.run("drawing.dimension.add", json!({"view": solidcraft_engine::doc::ids::encode(right), "kind": {"length": {"edge": [100, 40, 20]}}}));
    let side = decode(&side.unwrap(), "dimension");
    settle(&mut h);
    h.app.session.selection = [front_id, note, side].map(|object| Sel::Drawing { drawing: drawing.id, object }).to_vec();
    let centre = |h: &Harness| h.app.session.drawing("drawing.inspect").unwrap().view_centre(front_id).unwrap();
    let note_at = |h: &Harness| Vec2::from(h.app.session.drawing("drawing.inspect").unwrap().notes.by_id(note).unwrap().at);
    let (front_from, note_from, side_from, own_from) = (centre(&h), note_at(&h), text_of(&mut h, side).0, text_of(&mut h, on_front).0);
    let undo = h.app.session.undo.len();
    let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
    let bounds = drawing_scene::extract(&mut h.app.session, sheet).unwrap().objects.iter().find(|object| object.object == note).unwrap().bounds_mm;
    let grab = (bounds.min + bounds.max) * 0.5;
    let by = Vec2::new(20.0, -15.0);
    h.step(&json!({"drag": [on_screen(grab.x, grab.y), on_screen(grab.x + by.x, grab.y + by.y)], "steps": 6})).unwrap();
    h.frames(2);
    assert!(crate::dim_view::dragged().is_none(), "the drag ended");
    let moved = |what: &str, from: Vec2, to: Vec2| assert!((to - (from + by)).len() < 1.0, "{what} moved by {:?}, not {by:?}", to - from);
    moved("the view", front_from, centre(&h));
    moved("the note", note_from, note_at(&h));
    moved("the other view's dimension", side_from, text_of(&mut h, side).0);
    moved("the view's own dimension", own_from, text_of(&mut h, on_front).0);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the whole drag");
    h.app.run("edit.undo", json!({})).unwrap();
    assert!((centre(&h) - front_from).len() < 1e-9 && (note_at(&h) - note_from).len() < 1e-9, "one undo puts them all back");
    assert!((text_of(&mut h, side).0 - side_from).len() < 1e-6, "and the dimension");
}

/// What the pointer is over, and what is selected, is lit in the accent colour as in a sketch,
/// with no box round it.
#[test]
fn hover_lights_a_drawing_dimension_as_a_sketch_does() {
    let (mut h, _) = plate_sheet();
    let id = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
    let (text, _) = text_of(&mut h, id);
    let at = on_screen(text.x, text.y);
    h.step(&json!({"move": at})).unwrap();
    h.frames(2);
    let hover = &page(&mut h)["hover"];
    assert_eq!(hover["kind"], "dimension", "{hover}");
    assert_eq!(hover["lit"], true, "{hover}");
}

/// The group the sheet draws for object `object` now.
fn drawn_group(h: &mut Harness, object: u64) -> Group {
    settle(h);
    let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
    let view = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    view.scene.groups.iter().find(|group| group.object == object).unwrap().clone()
}

/// What a group draws, whatever its object's id: the preview's object is a draft's.
fn drawn_items(group: &Group) -> String {
    serde_json::to_string(&group.items).unwrap()
}

/// Placing a dimension previews it as it will be: the preview is the placed dimension's own group,
/// item for item (its lines, arrowheads and text, their sizes, weights and places), not a smaller
/// stand-in.
#[test]
fn a_dimension_previews_exactly_as_it_is_placed() {
    let (mut h, front) = plate_sheet();
    for (kind, drag) in [
        (json!({"length": {"edge": [50, 0, 0], "direction": "aligned"}}), json!({"line_through_mm": [140, 160]})),
        (json!({"length": {"edge": [0, 0, 10], "direction": "aligned"}}), json!({"line_through_mm": [100, 180]})),
    ] {
        let mut params = json!({"view": front, "kind": kind});
        for (key, value) in drag.as_object().unwrap() {
            params[key] = value.clone();
        }
        let preview = solidcraft_engine::cmd::drawing_dimension_preview(&h.app.session, &params).unwrap();
        let [shown] = &preview.groups[..] else { panic!("one group: {:?}", preview.groups.len()) };
        let added = h.app.run("drawing.dimension.add", params).unwrap();
        let id = solidcraft_engine::doc::ids::decode(added["dimension"].as_str().unwrap()).unwrap();
        assert_eq!(drawn_items(shown), drawn_items(&drawn_group(&mut h, id)), "the preview is what was placed");
    }
}

/// A note previews as it will be placed: in the drawing's lettering at its paper size, item for item.
#[test]
fn a_note_previews_exactly_as_it_is_placed() {
    let (mut h, _) = plate_sheet();
    let params = json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR ALL EDGES"});
    let preview = solidcraft_engine::drawing_scene::note_preview_scene(&h.app.session, &params).unwrap();
    let [shown] = &preview.groups[..] else { panic!("one group: {:?}", preview.groups.len()) };
    let added = h.app.run("drawing.note.add", params).unwrap();
    let id = solidcraft_engine::doc::ids::decode(added["note"].as_str().unwrap()).unwrap();
    assert_eq!(drawn_items(shown), drawn_items(&drawn_group(&mut h, id)), "the preview is what was placed");
}

/// Frames until no view or dimension is waiting to be drawn.
fn settle(h: &mut Harness) {
    for _ in 0..5_000 {
        h.frame();
        if page(h)["working"] == false {
            h.frames(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the sheet never finished drawing: {}", page(h));
}

fn page(h: &mut Harness) -> Value {
    h.call("ui.inspect", json!({}))["result"]["sheet_page"].clone()
}

/// What the last frame showed, without running another.
fn inspect_now(h: &Harness) -> Value {
    super::inspect(&h.app)
}

fn number(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// 100 % is (96 / 25.4) × calibration points per paper millimetre over the UI zoom, so a
/// millimetre is the same number of physical pixels at every UI scale; a calibration out of range
/// is not used.
#[test]
fn true_size_is_one_millimetre_at_any_ui_scale() {
    for native in [1.0f32, 1.25, 2.0] {
        for calibration in [0.5f32, 1.0, 1.25, 2.0] {
            let pixels: Vec<f64> = [0.75f32, 1.0, 1.5, 2.5].iter().map(|zoom| true_size(calibration, *zoom) * f64::from(native * zoom)).collect();
            let want = 96.0 / 25.4 * f64::from(native * calibration);
            assert!(pixels.iter().all(|p| (p - want).abs() < 1e-9), "{native} {calibration}: {pixels:?}");
        }
    }
    for hostile in [0.0f32, -1.0, 2.5, f32::NAN, f32::INFINITY] {
        assert_eq!(true_size(hostile, 1.0), true_size(1.0, 1.0), "calibration {hostile}");
    }
    for hostile in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(true_size(1.0, hostile), true_size(1.0, 1.0), "UI zoom {hostile}");
    }
    // On the page: at 100 % and UI scale 100 %, a 50 mm edge is 188.98 points (W23-4).
    let (mut h, _) = plate_sheet();
    assert_eq!(h.call("ui.view", json!({"view": "actual_size"}))["ok"], true);
    h.frames(2);
    let p = page(&mut h);
    assert!((number(&p["zoom"]) - 1.0).abs() < 1e-9, "{p}");
    assert!((50.0 * number(&p["points_per_mm"]) - 188.976).abs() < 1e-3, "{p}");
    let visible = p["visible_mm"].as_array().unwrap().iter().map(number).collect::<Vec<_>>();
    let canvas = h.app.viewport.rect.unwrap();
    assert!(((visible[2] - visible[0]) * number(&p["points_per_mm"]) - f64::from(canvas.width())).abs() < 1e-3, "{p}");
    // A calibration of 1.25 makes 100 % a quarter larger.
    h.app.preferences.drawing_calibration = 1.25;
    h.step(&json!({"click": {"handle": "nav:slot:sheet_zoom"}})).unwrap();
    h.step(&json!({"click": {"handle": "sheet:actual_size"}})).unwrap();
    h.frames(2);
    let p = page(&mut h);
    assert!((number(&p["points_per_mm"]) - 1.25 * 96.0 / 25.4).abs() < 1e-9 && (number(&p["zoom"]) - 1.0).abs() < 1e-9, "{p}");
}

/// #19070: Fit Sheet frames the sheet in the part of the window the ribbon, the Browser and the
/// History leave free, so no edge of the paper is under them, in a small window too.
#[test]
fn fit_sheet_keeps_the_paper_clear_of_the_ribbon_browser_and_history() {
    let (mut h, _) = plate_sheet();
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(6);
    h.app.run("view.home", json!({})).unwrap();
    h.frames(4);
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let canvas = h.app.viewport.rect.unwrap();
    let mut covers: Vec<(&str, Rect)> = h.app.shell.cards.iter().map(|r| ("a ribbon card", *r)).collect();
    covers.extend(h.app.shell.browser.map(|r| ("the Browser", r)));
    covers.extend(h.app.shell.history.map(|r| ("the History bar", r)));
    assert!(!covers.is_empty(), "the shell reports what covers the canvas");
    for (what, rect) in covers.into_iter().filter(|(_, r)| r.intersects(canvas)) {
        assert!(!paper.intersects(rect), "the paper {paper:?} is under {what} {rect:?}");
    }
}

/// A short Browser (folded, or on a small window) is wider than tall, yet lies against the left
/// edge: the paper is framed to its right, not below it.
#[test]
fn fit_sheet_frames_beside_a_short_browser_not_under_it() {
    let (mut h, _) = plate_sheet();
    h.frames(2);
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(6);
    let canvas = h.app.viewport.rect.unwrap();
    let short = Rect::from_min_size(canvas.min + vec2(8.0, 180.0), vec2(260.0, 60.0));
    h.app.shell.browser = Some(short);
    h.app.ui.show_browser = true;
    h.app.run("view.home", json!({})).unwrap();
    h.frames(4);
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    assert!(!paper.intersects(short), "the paper {paper:?} is under the short Browser {short:?}");
    assert!(paper.min.x >= short.right() - 1.0, "the paper {paper:?} is framed beside the Browser {short:?}");
}

/// Fit Sheet shows the whole sheet, and comes back after any zoom; typed and wheeled zooms stay
/// within 5 % to 3200 %, and a zoom that isn't a number changes nothing.
#[test]
fn fit_sheet_shows_the_whole_sheet_and_zoom_stays_in_range() {
    let (mut h, _) = plate_sheet();
    let whole = |p: &Value| {
        let v = p["visible_mm"].as_array().unwrap().iter().map(number).collect::<Vec<_>>();
        v[0] <= 0.0 && v[1] <= 0.0 && v[2] >= 420.0 && v[3] >= 297.0
    };
    let p = page(&mut h);
    assert_eq!(p["size_mm"], json!([420.0, 297.0]));
    assert!(whole(&p), "the first frame fits the sheet: {p}");
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let canvas = h.app.viewport.rect.unwrap();
    assert!(canvas.contains_rect(paper), "{paper:?} in {canvas:?}");
    // Fitted above the navigation bar, as the design's layout board has it.
    let bar = h.app.viewport.layout.as_ref().unwrap().navigation;
    assert!(bar.is_positive() && !paper.intersects(bar), "{paper:?} clear of the navigation bar {bar:?}");
    // A fitted sheet is fitted again when the window changes size, until something moves it.
    h.call("ui.resize", json!({"width": 900, "height": 600}));
    h.frames(4);
    assert!(whole(&page(&mut h)), "refitted in a smaller window: {}", page(&mut h));
    h.call("ui.resize", json!({"width": 1400, "height": 900}));
    h.frames(4);
    let ctx = h.ctx.clone();
    set_zoom(&mut h.app, &ctx, 1.5);
    h.frames(2);
    h.call("ui.resize", json!({"width": 1200, "height": 800}));
    h.frames(4);
    assert!((number(&page(&mut h)["zoom"]) - 1.5).abs() < 1e-9, "a chosen zoom stays through a resize");
    for (zoom, want) in [(1000.0, 32.0), (0.001, 0.05), (2.0, 2.0)] {
        let ctx = h.ctx.clone();
        set_zoom(&mut h.app, &ctx, zoom);
        h.frames(2);
        assert!((number(&page(&mut h)["zoom"]) - want).abs() < 1e-9, "{zoom}");
    }
    let ctx = h.ctx.clone();
    set_zoom(&mut h.app, &ctx, f64::NAN);
    set_zoom(&mut h.app, &ctx, f64::INFINITY);
    h.frames(2);
    assert!((number(&page(&mut h)["zoom"]) - 2.0).abs() < 1e-9);
    assert!(!whole(&page(&mut h)), "at 200 % the sheet is larger than the canvas");
    // The wheel zooms in no further than 3200 %.
    let centre = h.app.viewport.rect.unwrap().center();
    for _ in 0..80 {
        h.app.synthetic.push(egui::Event::PointerMoved(centre));
        h.app.synthetic.push(egui::Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
    }
    // egui's smoothed scrolling runs on for some frames after the last wheel event.
    h.app.synthetic.push(egui::Event::PointerGone);
    h.frames(120);
    let zoom = number(&page(&mut h)["zoom"]);
    assert!(zoom > 2.0 && zoom <= 32.0 + 1e-9, "{zoom}");
    // Fit (F6 runs view.fit), Home and the zoom menu's Fit sheet all frame it again.
    for fit in [json!({"command": "view.fit"}), json!({"command": "view.home"})] {
        set_zoom(&mut h.app, &ctx, 4.0);
        h.frames(2);
        h.app.run(fit["command"].as_str().unwrap(), json!({})).unwrap();
        h.frames(2);
        assert!(whole(&page(&mut h)), "{fit}");
    }
    set_zoom(&mut h.app, &ctx, 4.0);
    h.frames(2);
    h.step(&json!({"click": {"handle": "nav:slot:sheet_zoom"}})).unwrap();
    h.step(&json!({"click": {"handle": "sheet:fit"}})).unwrap();
    h.frames(2);
    assert!(whole(&page(&mut h)));
    assert!(h.call("ui.view", json!({"view": "front"}))["ok"] == false, "a sheet has no 3D views");
}

/// The navigation bar shows whole percent, and tenths below 10 %; typing a shown value back keeps it.
#[test]
fn the_zoom_readout_is_rounded() {
    for (zoom, shown) in [(0.72996, 73.0), (1.0, 100.0), (0.0549, 5.5), (0.05, 5.0), (0.0996, 10.0), (31.99, 3199.0)] {
        assert_eq!(shown_percent(zoom), shown, "{zoom}");
    }
}

/// The pan tool drags the sheet with the pointer on the page's own camera: the Design camera, the
/// design and its undo steps stay as they were.
#[test]
fn pan_moves_the_sheet_and_leaves_the_design_camera() {
    let (mut h, _) = plate_sheet();
    let (cam, doc, undo) = (h.app.cam, h.app.session.doc.to_json(), h.app.session.undo.len());
    h.app.run("view.tool", json!({"tool": "pan"})).unwrap();
    let before = crate::scenario::rect_of("sheet:paper").unwrap();
    // From an empty corner of the paper, not the canvas's middle: where the fitted sheet puts its
    // views depends on the free area around it.
    let from = before.min + vec2(15.0, 15.0);
    h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 120.0, "y1": from.y + 40.0, "steps": 8}));
    h.frames(2);
    let after = crate::scenario::rect_of("sheet:paper").unwrap();
    let moved = after.min - before.min;
    assert!((moved.x - 120.0).abs() < 2.0 && (moved.y - 40.0).abs() < 2.0, "{moved:?}");
    assert!((after.width() - before.width()).abs() < 1e-3, "panning doesn't zoom");
    assert_eq!(h.app.cam, cam);
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
}

#[test]
fn navigation_tools_do_not_drag_drawing_views() {
    let (mut h, front) = plate_sheet();
    let (cam, doc, undo) = (h.app.cam, h.app.session.doc.to_json(), h.app.session.undo.len());
    for tool in ["pan", "zoom"] {
        for edge in [true, false] {
            h.call("ui.view", json!({"view": "fit"}));
            h.frames(4);
            let view = h.app.run("drawing.view.inspect", json!({"view": front})).unwrap();
            let bounds = &view["boundary_mm"];
            let point = Vec2::new(
                if edge { number(&bounds["min"][0]) } else { (number(&bounds["min"][0]) + number(&bounds["max"][0])) / 2.0 },
                (number(&bounds["min"][1]) + number(&bounds["max"][1])) / 2.0,
            );
            let at = OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap());
            let from = at.screen(point);
            let before = crate::scenario::rect_of("sheet:paper").unwrap();
            h.app.run("view.tool", json!({"tool": tool})).unwrap();
            h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 120.0, "y1": from.y + 40.0, "steps": 8}));
            h.frames(2);
            let after = crate::scenario::rect_of("sheet:paper").unwrap();
            if tool == "pan" {
                let moved = after.min - before.min;
                assert!((moved.x - 120.0).abs() < 2.0 && (moved.y - 40.0).abs() < 2.0, "{tool}, edge={edge}: {moved:?}");
                assert!((after.width() - before.width()).abs() < 1e-3);
            } else {
                assert!((after.width() - before.width()).abs() > 1.0, "{tool}, edge={edge}: the sheet did not zoom");
            }
            assert_eq!(h.app.run("drawing.view.inspect", json!({"view": front})).unwrap(), view);
            assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc.clone(), undo));
            assert_eq!(h.app.cam, cam);
        }
    }
}

fn view_edge(h: &mut Harness, view: &str) -> egui::Pos2 {
    let view = h.app.run("drawing.view.inspect", json!({"view": view})).unwrap();
    let bounds = &view["boundary_mm"];
    let point = Vec2::new(number(&bounds["min"][0]), (number(&bounds["min"][1]) + number(&bounds["max"][1])) / 2.0);
    OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap()).screen(point)
}

#[test]
fn navigation_tools_keep_view_click_selection() {
    let (mut h, front) = plate_sheet();
    let right =
        h.app.session.drawing("drawing.inspect").unwrap().views.iter().find(|view| solidcraft_engine::doc::ids::encode(view.id) != front).unwrap().id;
    let right = solidcraft_engine::doc::ids::encode(right);
    let (doc, undo, cam) = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam);
    let drawing = h.app.session.drawing("drawing.inspect").unwrap().id;
    let pick = |view: &str| Sel::Drawing { drawing, object: solidcraft_engine::doc::ids::decode(view).unwrap() };
    assert_ne!(pick(&front), pick(&right));
    let empty = OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap()).screen(Vec2::new(350.0, 80.0));
    for tool in ["pan", "zoom"] {
        h.app.run("view.tool", json!({"tool": tool})).unwrap();
        for view in [&front, &right] {
            let point = view_edge(&mut h, view);
            h.step(&json!({"click": [point.x, point.y]})).unwrap();
            h.frames(4);
            assert_eq!(h.app.session.selection, [pick(view)], "{tool}: select {view}");
        }
        h.step(&json!({"click": [empty.x, empty.y]})).unwrap();
        h.frames(4);
        assert!(h.app.session.selection.is_empty(), "{tool}: clear selection on empty paper");
        assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam), (doc.clone(), undo, cam));
    }
}

#[test]
fn navigation_cancels_a_view_move_before_release() {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    for tool in ["pan", "zoom"] {
        for clear_before_release in [false, true] {
            let (mut h, front) = plate_sheet();
            let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
            let from = view_edge(&mut h, &front);
            h.step(&json!({"click": [from.x, from.y]})).unwrap();
            h.frames(4);
            let drawing = h.app.session.drawing("drawing.inspect").unwrap().id;
            assert_eq!(h.app.session.selection, [Sel::Drawing { drawing, object: solidcraft_engine::doc::ids::decode(&front).unwrap() }]);
            let at = OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap());
            h.frame_with(vec![egui::Event::PointerMoved(from), button(from, true)]);
            let preview = from + vec2(32.0, 24.0);
            h.frame_with(vec![egui::Event::PointerMoved(preview)]);
            h.frames(2);
            assert!(h.ctx.input(|input| input.pointer.primary_down()));
            let offset = at.paper(preview) - at.paper(from);
            let label = solidcraft_ui_text::format(
                "ui.drawing_view_selection.offset",
                &[("x", &format!("{:+.1}", offset.x)), ("y", &format!("{:+.1}", offset.y))],
            );
            assert!(texts(&h.shapes).contains(&label), "a view-move preview is painted");
            assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc.clone(), undo));
            h.app.run("view.tool", json!({"tool": tool})).unwrap();
            let release = preview + vec2(12.0, 6.0);
            h.frame_with(vec![egui::Event::PointerMoved(release)]);
            h.frames(2);
            if clear_before_release {
                h.app.run("view.tool", json!({"tool": "none"})).unwrap();
                h.frames(2);
            }
            h.frame_with(vec![button(release, false)]);
            h.app.run("view.tool", json!({"tool": "none"})).unwrap();
            h.frames(4);
            assert_eq!(
                (h.app.session.doc.to_json(), h.app.session.undo.len()),
                (doc.clone(), undo),
                "{tool}, clear_before_release={clear_before_release}"
            );
            let from = view_edge(&mut h, &front);
            h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 32.0, "y1": from.y + 24.0, "steps": 8}));
            h.frames(2);
            assert_ne!(h.app.session.doc.to_json(), doc, "a fresh view drag still works");
            h.app.run("edit.undo", json!({})).unwrap();
            h.frames(2);
            assert_eq!(h.app.session.doc.to_json(), doc, "one Undo restores the fresh view move");
        }
    }
}

fn navigation_drag_fixture(kind: &str) -> (Harness, egui::Pos2) {
    let (mut h, front) = plate_sheet();
    let point = if kind == "dimension" {
        let id = h.app.session.drawing("drawing.inspect").unwrap().dimensions[0].id;
        text_of(&mut h, id).0
    } else {
        let note = h.app.run("drawing.note.add", json!({"anchor": {"sheet": "Sheet 1"}, "at": [300, 230], "content": "DEBURR"})).unwrap();
        let note = solidcraft_engine::doc::ids::decode(note["note"].as_str().unwrap()).unwrap();
        settle(&mut h);
        if kind == "group" {
            let drawing = h.app.session.drawing("drawing.inspect").unwrap().id;
            h.app.session.selection =
                [solidcraft_engine::doc::ids::decode(&front).unwrap(), note].map(|object| Sel::Drawing { drawing, object }).to_vec();
            h.frames(2);
            let edge = view_edge(&mut h, &front);
            return (h, edge);
        }
        let sheet = active_sheet(&h.app).unwrap();
        let bounds =
            drawing_scene::extract(&mut h.app.session, sheet).unwrap().objects.iter().find(|object| object.object == note).unwrap().bounds_mm;
        (bounds.min + bounds.max) * 0.5
    };
    let screen = OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap()).screen(point);
    (h, screen)
}

#[test]
fn navigation_tools_do_not_drag_drawing_annotations_or_groups() {
    for tool in ["pan", "zoom"] {
        for kind in ["dimension", "note", "group"] {
            let (mut h, from) = navigation_drag_fixture(kind);
            let (doc, undo, cam) = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam);
            let before = crate::scenario::rect_of("sheet:paper").unwrap();
            h.app.run("view.tool", json!({"tool": tool})).unwrap();
            h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 80.0, "y1": from.y + 24.0, "steps": 8}));
            h.frames(2);
            let after = crate::scenario::rect_of("sheet:paper").unwrap();
            if tool == "pan" {
                let moved = after.min - before.min;
                assert!((moved.x - 80.0).abs() < 2.0 && (moved.y - 24.0).abs() < 2.0, "{tool}, {kind}: {moved:?}");
            } else {
                assert!((after.width() - before.width()).abs() > 1.0, "{tool}, {kind}: the sheet did not zoom");
            }
            assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam), (doc, undo, cam), "{tool}, {kind}");
            assert!(crate::dim_view::dragged().is_none());
        }
    }
}

#[test]
fn navigation_stops_a_live_drawing_drag() {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    for tool in ["pan", "zoom"] {
        for kind in ["dimension", "note", "group"] {
            let (mut h, from) = navigation_drag_fixture(kind);
            let original = h.app.session.doc.to_json();
            h.frame_with(vec![egui::Event::PointerMoved(from), button(from, true)]);
            let moved = from + vec2(32.0, 24.0);
            h.frame_with(vec![egui::Event::PointerMoved(moved)]);
            h.frames(2);
            assert!(h.ctx.input(|input| input.pointer.primary_down()) && crate::dim_view::dragged().is_some());
            assert_ne!(h.app.session.doc.to_json(), original, "{kind}: a live drag has applied a move");
            let (doc, undo, revision) = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision);
            h.app.run("view.tool", json!({"tool": tool})).unwrap();
            let release = moved + vec2(16.0, 8.0);
            h.frame_with(vec![egui::Event::PointerMoved(release)]);
            h.frames(2);
            assert!(crate::dim_view::dragged().is_none(), "{tool}, {kind}: the drawing drag stopped");
            h.frame_with(vec![button(release, false)]);
            h.app.run("view.tool", json!({"tool": "none"})).unwrap();
            h.frames(2);
            assert_eq!(
                (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.session.revision),
                (doc, undo, revision),
                "{tool}, {kind}: navigation adds no move or undo step"
            );
            h.app.run("edit.undo", json!({})).unwrap();
            h.frames(2);
            assert_eq!(h.app.session.doc.to_json(), original, "{tool}, {kind}: one Undo restores the earlier live move");
        }
    }
}

/// Orbit can stay latched when Design is left for Drawing. A sheet cannot orbit, so its objects
/// still take ordinary drags, in one undo step, without moving either camera.
#[test]
fn navigation_allows_drawing_drags_with_orbit_latched_in_design() {
    for kind in ["view", "dimension", "note", "group"] {
        let (mut h, from) = if kind == "view" {
            let (mut h, front) = plate_sheet();
            let from = view_edge(&mut h, &front);
            (h, from)
        } else {
            navigation_drag_fixture(kind)
        };
        // The classic switcher, which this test covers: since #19655 the default lets the focused
        // view choose the workspace (`workspace_follows_view`).
        h.app.preferences.workspace_follows_view = false;
        let point = OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap()).paper(from);
        let selected = h.app.session.selection.clone();
        assert_eq!(h.call("ui.workspace", json!({"workspace": "design"}))["ok"], true);
        assert!(!crate::drawing_workspace::active(&h.app));
        h.app.run("view.tool", json!({"tool": "orbit"})).unwrap();
        assert_eq!(h.app.viewport.nav, Some(crate::viewport::NavMode::Orbit));
        assert_eq!(h.call("ui.workspace", json!({"workspace": "drawing"}))["ok"], true);
        h.frames(4);
        assert!(crate::drawing_workspace::active(&h.app));
        assert_eq!(h.app.viewport.nav, Some(crate::viewport::NavMode::Orbit), "Orbit stayed latched after entering Drawing");
        assert_eq!(h.app.session.selection, selected, "the drawing's selection, including its group, was restored");
        let from = OnScreen::of(&h.app.drawing_workspace.page.cam, h.app.viewport.rect.unwrap()).screen(point);
        let (doc, undo, cam, sheet_cam) = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam, h.app.drawing_workspace.page.cam);
        assert_eq!(h.call("ui.drag", json!({"x0": from.x, "y0": from.y, "x1": from.x + 32.0, "y1": from.y + 24.0, "steps": 8}))["ok"], true);
        h.frames(2);
        assert_ne!(h.app.session.doc.to_json(), doc, "{kind}: the ordinary drag moved the drawing object");
        assert_eq!(h.app.session.undo.len(), undo + 1, "{kind}: one undo step for the drag");
        assert_eq!((h.app.cam, h.app.drawing_workspace.page.cam), (cam, sheet_cam), "{kind}: unsupported Orbit moved neither camera");
        assert_eq!(h.app.viewport.nav, Some(crate::viewport::NavMode::Orbit));
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.doc.to_json(), doc, "{kind}: one Undo restores the ordinary drag");
    }
}

/// A side button the user mapped to Pan drags the sheet through the shared navigation, and a plain
/// click on it moves nothing. Unmapped, the same drag does nothing.
#[test]
fn a_mapped_side_button_pans_the_drawing_sheet() {
    let (mut h, _) = plate_sheet();
    let drag = |h: &mut Harness| {
        let at = h.app.viewport.rect.unwrap().center();
        let ev = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Extra2, pressed, modifiers: egui::Modifiers::NONE };
        h.frame_with(vec![egui::Event::PointerMoved(at)]);
        h.frame_with(vec![ev(true)]);
        for i in 1..=8 {
            h.frame_with(vec![egui::Event::PointerMoved(at + vec2(10.0, 3.0) * i as f32)]);
        }
        h.frame_with(vec![ev(false)]);
        h.frames(2);
    };
    let paper = || crate::scenario::rect_of("sheet:paper").unwrap();
    let (doc, undo, cam, before) = (h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam, paper());
    drag(&mut h);
    assert_eq!(paper(), before, "unmapped, the side button leaves the sheet alone");
    assert_eq!(h.call("ui.prefs", json!({"nav_button_map": {"extra2": "pan"}}))["ok"], true);
    drag(&mut h);
    let moved = paper().min - before.min;
    assert!((moved.x - 80.0).abs() < 4.0 && (moved.y - 24.0).abs() < 4.0, "the sheet panned with the drag: {moved:?}");
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len(), h.app.cam), (doc, undo, cam), "the design and its camera are untouched");
}

#[test]
fn panning_a_tiny_fitted_sheet_keeps_its_zoom_on_the_next_frame() {
    use egui::{Event, Modifiers, PointerButton};
    use solidcraft_views::ViewKind;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":40,"width":30,"height":5})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let drawing = h.app.session.drawing("drawing.inspect").unwrap();
    let subject = crate::sheet_views::subject(drawing.id, drawing.sheets[0].id);
    let id = crate::documents::view_open(&mut h.app, ViewKind::Sheet, Some(subject), false);
    crate::documents::view_activate(&mut h.app, id);
    crate::sketch_view::sync(&mut h.app).unwrap();
    activate_view(&mut h.app, id);
    h.app.run("view.tool", json!({"tool":"pan"})).unwrap();
    let ctx = h.ctx.clone();
    let canvas = Rect::from_min_size(Pos2::ZERO, vec2(120.0, 80.0));
    let mut time = 0.0;
    let mut frame = |app: &mut crate::SolveApp, events| {
        time += 1.0 / 60.0;
        let _ = frame_on(&ctx, egui::RawInput { screen_rect: Some(canvas), focused: true, time: Some(time), events, ..Default::default() }, |ui| {
            assert!(show_view(app, ui, id));
        });
    };
    frame(&mut h.app, Vec::new());
    let before = h.app.drawing_workspace.page.scale;
    let target = h.app.drawing_workspace.page.cam.target;
    assert!(before < true_size_now(&h.app, &ctx) * ZOOM.start());
    assert!(h.app.drawing_workspace.page.fitted_on.is_some());
    let from = canvas.center();
    let to = from + vec2(16.0, 10.0);
    frame(
        &mut h.app,
        vec![
            Event::PointerMoved(from),
            Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        ],
    );
    frame(&mut h.app, vec![Event::PointerMoved(to)]);
    frame(&mut h.app, vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    assert_ne!(h.app.drawing_workspace.page.cam.target, target, "the pointer drag actually pans");
    assert!(h.app.drawing_workspace.page.fitted_on.is_none());
    frame(&mut h.app, Vec::new());
    frame(&mut h.app, Vec::new());
    assert!((h.app.drawing_workspace.page.scale / before - 1.0).abs() < 1e-8, "panning never raises a tiny fitted sheet to the manual zoom floor");
}

/// Each sheet colour is drawn in its tokens, standing off the backdrop, and is screen only: the
/// design, its undo steps and the exported SVG are the same bytes under every one.
#[test]
fn sheet_colour_is_screen_only() {
    let (mut h, _) = plate_sheet();
    let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
    let svg = |h: &mut Harness| h.app.run("drawing.export", json!({"sheet": "Sheet 1"})).unwrap()["svg"].as_str().unwrap().to_owned();
    let first = svg(&mut h);
    let t = Tokens::get();
    let mut seen = Vec::new();
    for colour in SheetColour::ALL {
        h.app.preferences.drawing_sheet_colour = colour.id().into();
        h.frames(2);
        let p = page(&mut h);
        let (paper, ink) = colour.paper(&t, solidcraft_theme::is_dark());
        let (stage, paper) = stage_and_paper(paper, t.viewport_top);
        assert_eq!((&p["stage"], &p["paper"]), (&json!(hex(stage)), &json!(hex(paper))), "{colour:?}");
        assert_eq!(p["ink"], json!(hex(ink)), "{colour:?}");
        seen.push(p["paper"].clone());
        assert_eq!(svg(&mut h), first, "{colour:?}");
    }
    assert_ne!(seen[1], seen[2], "White and Blueprint differ");
    assert_ne!(seen[2], seen[3], "Blueprint and Prussian Blue differ");
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
    let bad = h.call("ui.prefs", json!({"drawing_sheet_colour": "blue"}));
    assert_eq!(bad["ok"], false, "{bad}");
    let bad = h.call("ui.prefs", json!({"drawing_calibration": 3.0}));
    assert_eq!(bad["ok"], false, "{bad}");
}

/// The backdrop has no effect on the design, undo steps or exported SVG.
#[test]
fn sheet_backdrop_is_a_screen_only_preference() {
    let (mut h, _) = plate_sheet();
    assert_eq!(h.app.preferences.drawing_sheet_backdrop, "none");
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let svg = |h: &mut Harness| h.app.run("drawing.export", json!({"sheet": "Sheet 1"})).unwrap()["svg"].as_str().unwrap().to_owned();
    let first = svg(&mut h);
    for (_, id, _) in SheetBackdrop::ALL {
        assert_eq!(h.call("ui.prefs", json!({"drawing_sheet_backdrop": id}))["ok"], true);
        h.frames(2);
        assert_eq!(h.app.preferences.drawing_sheet_backdrop, id);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(svg(&mut h), first);
    }
    let bad = h.call("ui.prefs", json!({"drawing_sheet_backdrop": "invalid"}));
    assert_eq!(bad["ok"], false);
    assert_eq!(h.app.preferences.drawing_sheet_backdrop, "none");
}

/// The sheet as SVG and as a 150 dpi PNG, each written to a file by `drawing.export` sent over the
/// control channel (which carries the window's context, as an agent's export does), read back:
/// `tag` names the files.
fn exported(h: &mut Harness, tag: &str) -> [Vec<u8>; 2] {
    ["svg", "png"].map(|format| {
        let path = h.dir.join(format!("{tag}.{format}"));
        let dpi = (format == "png").then_some(150);
        let mut p = json!({"sheet": "Sheet 1", "format": format, "path": path.to_string_lossy()});
        if let Some(dpi) = dpi {
            p["dpi"] = json!(dpi);
        }
        let reply = h.call("engine.execute", json!({"command": "drawing.export", "params": p}));
        assert_eq!(reply["ok"], true, "{reply}");
        std::fs::read(&path).unwrap()
    })
}

/// A drawing's exports don't depend on the window (#4894 W23-3; Cam, 2 Nov): the sheet is built in
/// millimetres at a fixed dpi, so the window's scale and size never reach the file. Cam's two
/// screens: the same session exports on a 4K screen (2 pixels a point, the window filling it), is
/// dragged to a 1080p screen (1 pixel a point, a smaller window) and draws a few frames, then
/// exports again; and at the harness's own scale in the plate sheet's window. The window's context
/// then has 2 and 1 pixels a point. The SVG and the PNG are the same bytes every time.
#[test]
fn a_drawing_exports_the_same_on_a_4k_and_a_1080p_screen() {
    let (mut h, _) = plate_sheet();
    let here = exported(&mut h, "here");
    // The 4K screen at 200 %: 3840 x 2160 pixels are 1920 x 1080 points.
    h.screen_scale = Some(2.0);
    h.call("ui.resize", json!({"width": 1920, "height": 1080}));
    h.frames(3);
    assert_eq!(h.ctx.pixels_per_point(), 2.0);
    let on_4k = exported(&mut h, "4k");
    // Dragged to the 1080p screen at 100 %, in a window smaller than the screen.
    h.screen_scale = Some(1.0);
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(3);
    assert_eq!(h.ctx.pixels_per_point(), 1.0);
    let on_1080p = exported(&mut h, "1080p");
    for (screen, files) in [("4K", &on_4k), ("1080p", &on_1080p)] {
        assert!(files[0] == here[0], "the SVG on the {screen} screen differs");
        assert!(files[1] == here[1], "the PNG on the {screen} screen differs");
    }
    assert!(here[0].starts_with(b"<?xml") && here[1].starts_with(b"\x89PNG"));
}

/// Every choice, in either theme, stands off the stage behind it, and White is never pure white.
/// The sheet is white paper unless another colour is chosen, in a dark theme too: Follows the
/// theme is a choice, not the default. A setting saved while the default followed the theme
/// (`auto`) was never chosen, and reads as White.
#[test]
fn the_sheet_is_white_unless_follows_the_theme_is_chosen() {
    let t = Tokens::dark();
    assert_eq!(crate::prefs::Prefs::default().drawing_sheet_colour, "white");
    let default = SheetColour::parse(&crate::prefs::Prefs::default().drawing_sheet_colour).unwrap();
    assert_eq!(default.paper(&t, true), (t.paper_white, t.paper_ink), "white paper in a dark theme");
    assert_eq!(SheetColour::parse("auto"), Some(SheetColour::White), "the old default's name");
    let follows = SheetColour::parse("theme").unwrap();
    assert_eq!(follows.paper(&t, true), (t.paper_dark, t.paper_dark_ink), "chosen, it follows a dark theme");
    assert_eq!(follows.paper(&t, false), (t.paper_white, t.paper_ink));
    for colour in SheetColour::ALL {
        assert_eq!(SheetColour::parse(colour.id()), Some(colour));
    }

    let (mut h, _) = plate_sheet();
    assert_eq!(h.call("ui.prefs", json!({"drawing_sheet_colour": "theme"}))["ok"], true);
    assert_eq!(h.call("ui.prefs", json!({"drawing_sheet_colour": "auto"}))["ok"], true, "a saved file's old name still loads");
    h.frames(2);
    assert_eq!(page(&mut h)["paper"], json!(hex(stage_and_paper(Tokens::get().paper_white, Tokens::get().viewport_top).1)));
}

#[test]
fn every_sheet_colour_stands_off_the_backdrop() {
    crate::test_support::host_data();
    for t in [Tokens::light(), Tokens::dark()] {
        for dark in [false, true] {
            for colour in SheetColour::ALL {
                let (paper, ink) = colour.paper(&t, dark);
                let (stage, shown) = stage_and_paper(paper, t.viewport_top);
                assert!((lightness(shown) - lightness(stage)).abs() >= STAND_OFF, "{colour:?} {dark}");
                assert!((lightness(shown) - lightness(ink)).abs() > 0.5, "{colour:?} {dark}: ink reads on its paper");
            }
        }
        assert_ne!(t.paper_white, Color32::WHITE);
        // White keeps its colour in the light theme: the stage behind it goes darker instead.
        assert_eq!(stage_and_paper(t.paper_white, Tokens::light().viewport_top).1, t.paper_white);
        assert_eq!(SheetColour::Auto.paper(&t, false), SheetColour::White.paper(&t, false));
        assert_eq!(SheetColour::Auto.paper(&t, true), (t.paper_dark, t.paper_dark_ink));
    }
    // "Follows the theme" in the Blueprint theme is its blueprint blue (D24, D25).
    let blueprint = crate::theme_files::resolve(crate::theme_files::find("blueprint", &[]).unwrap());
    assert_eq!(SheetColour::Auto.paper(&blueprint, true).0, Tokens::dark().paper_blueprint);
    for colour in SheetColour::ALL {
        assert_eq!(SheetColour::parse(colour.id()), Some(colour));
    }
    assert_eq!(SheetColour::parse("Blueprint"), None);
    assert_eq!(SheetColour::parse("warm"), None);
}

fn square(at: Vec2, side: f64) -> Loop2 {
    let p = [at, at + Vec2::new(side, 0.0), at + Vec2::new(side, side), at + Vec2::new(0.0, side)];
    Loop2 { segs: (0..4).map(|i| Seg2::Line { a: p[i], b: p[(i + 1) % 4] }).collect() }
}

/// A stroke, a dashed stroke, an arc and a fill with a hole each become what they are: the dashes
/// at their lengths, the arc within the tolerance, the fill's triangles its area.
#[test]
fn marks_draw_each_item_of_the_scene() {
    let line =
        Item::Stroke { path: vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }], closed: false, width_mm: 0.5, pattern: None };
    let dashed = Item::Stroke {
        path: vec![Seg2::Line { a: Vec2::new(0.0, 10.0), b: Vec2::new(30.0, 10.0) }],
        closed: false,
        width_mm: 0.35,
        pattern: Some(Dashes { lengths_mm: vec![5.0, 5.0], phase_mm: 0.0 }),
    };
    let arc = Item::Stroke {
        path: vec![Seg2::Arc { center: Vec2::new(50.0, 50.0), radius: 20.0, start: 0.0, sweep: std::f64::consts::PI }],
        closed: false,
        width_mm: 0.5,
        pattern: None,
    };
    let fill =
        Item::Fill { region: Region2 { outer: square(Vec2::new(60.0, 0.0), 10.0), holes: vec![square(Vec2::new(62.0, 2.0), 2.0).reversed()] } };
    let scene = Scene {
        size_mm: Vec2::new(100.0, 100.0),
        groups: vec![Group { object: 1, kind: ObjectKind::View, items: vec![line, dashed, arc, fill] }],
        fonts: Vec::new(),
        clips: Vec::new(),
    };
    let marks = Marks::of(&scene, 7, 0.01);
    assert!(marks.error.is_none(), "{:?}", marks.error);
    let lines: Vec<&Vec<Vec2>> = marks.marks.iter().filter_map(|m| if let Mark::Line { points, .. } = m { Some(points) } else { None }).collect();
    assert_eq!(lines.len(), 1 + 3 + 1, "the line, three dashes, the arc");
    for dash in &lines[1..4] {
        let length: f64 = dash.windows(2).map(|w| w[0].dist(w[1])).sum();
        assert!((length - 5.0).abs() < 1e-9, "{dash:?}");
    }
    assert!(lines[4].iter().all(|p| (p.dist(Vec2::new(50.0, 50.0)) - 20.0).abs() <= 0.01 + 1e-9) && lines[4].len() > 20);
    let Some(Mark::Fill { triangles, .. }) = marks.marks.last() else { panic!("{:?}", marks.marks) };
    let area: f64 = triangles.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() / 2.0).sum();
    assert!((area - 96.0).abs() < 1e-6, "{area}");
    // A scene that isn't valid draws nothing and says why, never panics.
    let mut bad = scene.clone();
    bad.size_mm = Vec2::new(f64::NAN, 1.0);
    let marks = Marks::of(&bad, 8, 0.01);
    assert!(marks.marks.is_empty() && marks.error.is_some());
}

/// The plate's sheet draws its border, views, dimension and title block text: every text run's
/// glyphs are filled, and all of it lies on the paper.
#[test]
fn the_plate_sheet_draws_its_lines_and_text() {
    let (mut h, _) = plate_sheet();
    let sheet = active_sheet(&h.app).unwrap();
    let view = solidcraft_engine::drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    let glyphs: usize = view.scene.groups.iter().flat_map(|g| &g.items).map(|i| if let Item::Text(run) = i { run.glyphs.len() } else { 0 }).sum();
    assert!(glyphs > 20, "the title block has text: {glyphs}");
    assert!(view.scene.groups.iter().any(|g| g.kind == ObjectKind::Dimension), "the dimension is drawn");
    let marks = Marks::of(&view.scene, view.generation, 0.01);
    assert!(marks.error.is_none());
    let fills = marks.marks.iter().filter(|m| matches!(m, Mark::Fill { .. })).count();
    assert!(fills >= glyphs / 2, "{fills} fills for {glyphs} glyphs (a space has none)");
    let paper = Aabb2 { min: Vec2::ZERO, max: Vec2::new(420.0, 297.0) }.inflated(1.0);
    for m in &marks.marks {
        let b = match m {
            Mark::Line { bounds, .. } | Mark::Fill { bounds, .. } => bounds,
            Mark::Picture { rect, .. } => rect,
        };
        assert!(paper.contains(b.min) && paper.contains(b.max), "{b:?}");
    }
    assert_eq!(page(&mut h)["views"].as_array().unwrap().len(), 2);
    assert!(page(&mut h)["views"].as_array().unwrap().iter().all(|v| v["status"] == "drawn" && v["words"] == json!([])));
}

/// Every textured mesh in `shapes` (a picture's pixels; text and fills use the font texture), with the
/// screen box of its vertices.
fn textured(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Mesh, Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Mesh, Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() && !mesh.vertices.is_empty() => {
                let bounds = Rect::from_points(&mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
                out.push(((**mesh).clone(), bounds));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

/// A shaded view's picture is painted on the sheet page, over the picture's place on the sheet, once
/// it is drawn: not only its dashed frame (#18413).
#[test]
fn a_shaded_views_picture_is_painted_on_the_page() {
    let (mut h, front) = plate_sheet();
    h.app.run("drawing.view.edit", json!({"view": front, "set": {"visual_style": "shaded"}})).unwrap();
    settle(&mut h);
    let sheet = active_sheet(&h.app).unwrap();
    let view = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    let picture = view
        .scene
        .groups
        .iter()
        .filter(|g| solidcraft_engine::doc::ids::encode(g.object) == front)
        .flat_map(|g| &g.items)
        .find_map(|i| if let Item::Picture(p) = i { Some(*p) } else { None })
        .expect("a shaded view draws a picture");
    let pixels = view.pictures.iter().find(|p| p.source == picture.source).expect("the engine hands the page its pixels");
    assert!(pixels.rows.is_some() && !pixels.stale, "the picture is drawn: {:?}", pixels.why);
    // The paper's screen rectangle maps millimetres to points: the picture's rectangle on screen.
    let paper = crate::scenario::rect_of("sheet:paper").unwrap();
    let size = view.scene.size_mm;
    let screen = |p: Vec2| pos2(paper.left() + (p.x / size.x) as f32 * paper.width(), paper.bottom() - (p.y / size.y) as f32 * paper.height());
    let expected = Rect::from_two_pos(screen(picture.rect.min), screen(picture.rect.max));
    let meshes = textured(&h.shapes);
    assert!(
        meshes.iter().any(|(_, b)| (b.min - expected.min).length() < 1.5 && (b.max - expected.max).length() < 1.5),
        "a textured mesh covers the picture's place {expected:?}: {:?}",
        meshes.iter().map(|(_, b)| *b).collect::<Vec<_>>()
    );
}

/// A picture of a `width` × `height` grid, one millimetre a pixel, with its lower-left corner at the
/// sheet's origin: red pixels, or none (`rows: None`) and `why`.
fn hand_picture(source: u64, width: u32, height: u32, rows: bool, why: Option<&str>) -> SheetPicture {
    let grid = PixelGrid { x0: 0.0, y0: 0.0, pitch: 1.0, width, height };
    let rows = rows.then(|| Arc::from([255u8, 0, 0, 255].repeat((width * height) as usize)));
    SheetPicture { source, view: 1, grid, rows, stale: false, why: why.map(str::to_owned) }
}

/// One view's group holding a picture over `rect`, on a 100 mm square sheet, cut to `clip`.
fn picture_scene(source: u64, rect: Aabb2, opacity: f64, clip: Option<Clip>) -> Scene {
    let item = Item::Picture(solidcraft_engine::geom::paper::Picture { source, rect, opacity });
    let groups = vec![Group { object: 1, kind: ObjectKind::View, items: vec![item] }];
    Scene { size_mm: Vec2::new(100.0, 100.0), groups, fonts: Vec::new(), clips: clip.map(|c| vec![(1, c)]).unwrap_or_default() }
}

/// What `marks` paints with `pictures`, at 10 points a millimetre with the paper's origin at (0, 0).
fn painted(ctx: &egui::Context, marks: &Marks, pictures: &PictureTextures) -> Vec<egui::epaint::ClippedShape> {
    let at = OnScreen { centre: Pos2::ZERO, target: Vec2::ZERO, scale: 10.0 };
    let visible = Aabb2 { min: Vec2::new(-1e3, -1e3), max: Vec2::new(1e3, 1e3) };
    frame_on(ctx, egui::RawInput::default(), |ui| marks.paint(ui.painter(), &at, visible, Color32::BLACK, 1.0, pictures)).shapes
}

/// Each text shape's words in `shapes`.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

/// A picture is uploaded once for its rows: the same rows again upload nothing, new rows (another
/// allocation, even of the same bytes) upload once more, and a picture that leaves the sheet is let go.
#[test]
fn a_picture_uploads_once_and_again_only_for_new_rows() {
    let ctx = egui::Context::default();
    let mut pictures = PictureTextures::default();
    let first = hand_picture(7, 4, 2, true, None);
    for _ in 0..3 {
        pictures.sync(&ctx, std::slice::from_ref(&first), 2048);
    }
    assert_eq!(pictures.uploads, 1, "the same rows upload once");
    let again = hand_picture(7, 4, 2, true, None);
    pictures.sync(&ctx, &[again], 2048);
    assert_eq!(pictures.uploads, 2, "new rows upload again");
    pictures.sync(&ctx, &[], 2048);
    assert!(pictures.get(7).is_none(), "a picture that left the sheet is let go");
    // Rows that aren't the grid's size upload nothing, and the picture keeps its frame.
    let mut short = hand_picture(8, 4, 2, true, None);
    short.rows = Some(Arc::from(vec![0u8; 5]));
    pictures.sync(&ctx, &[short], 2048);
    assert!(pictures.get(8).is_some_and(|u| u.tiles.is_empty()));
}

/// A picture not drawn yet shows its dashed frame and no pixels; one that can't be drawn says why,
/// in words beside its frame (D26).
#[test]
fn a_picture_not_drawn_shows_its_frame_and_one_that_failed_says_why() {
    // The catalogs, as the app installs them at start: the words are a message.
    crate::i18n::install();
    let ctx = egui::Context::default();
    let rect = Aabb2 { min: Vec2::new(10.0, 10.0), max: Vec2::new(40.0, 30.0) };
    let marks = Marks::of(&picture_scene(7, rect, 1.0, None), 1, 0.01);
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 30, 20, false, None)], 2048);
    let shapes = painted(&ctx, &marks, &pictures);
    assert!(textured(&shapes).is_empty() && texts(&shapes).is_empty() && !shapes.is_empty(), "only the frame");
    pictures.sync(&ctx, &[hand_picture(7, 30, 20, false, Some("too large to draw"))], 2048);
    let shapes = painted(&ctx, &marks, &pictures);
    assert!(textured(&shapes).is_empty());
    assert!(texts(&shapes).iter().any(|t| t.contains("too large to draw")), "{:?}", texts(&shapes));
    assert_eq!(pictures.inspect()[0]["shown"], "failed");
}

/// A picture wider than the GPU's largest texture is split into tiles that meet edge to edge: each
/// tile's texture runs over its own columns, row 0 at the top, at the item's opacity.
#[test]
fn a_picture_past_the_gpus_side_is_split_into_tiles() {
    let ctx = egui::Context::default();
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(3.0, 1.0) };
    let marks = Marks::of(&picture_scene(7, rect, 0.5, None), 1, 0.01);
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 3, 1, true, None)], 2);
    assert_eq!(pictures.get(7).unwrap().tiles.len(), 2);
    let meshes = textured(&painted(&ctx, &marks, &pictures));
    assert_eq!(meshes.len(), 2, "one mesh a tile");
    let (a, b) = (&meshes[0], &meshes[1]);
    assert_ne!(a.0.texture_id, b.0.texture_id);
    // 10 points a millimetre: the first tile is columns 0-1 (0 to 20 points), the second column 2.
    assert!((a.1.min.x - 0.0).abs() < 1e-3 && (a.1.max.x - 20.0).abs() < 1e-3 && (b.1.min.x - 20.0).abs() < 1e-3 && (b.1.max.x - 30.0).abs() < 1e-3);
    for (mesh, _) in &meshes {
        for v in &mesh.vertices {
            // Paper y up is screen y down: the picture's top (y = 1 mm, -10 points) is the texture's row 0.
            let v_expected = if (v.pos.y + 10.0).abs() < 1e-3 { 0.0 } else { 1.0 };
            assert!((v.uv.y - v_expected).abs() < 1e-6 && (0.0..=1.0).contains(&v.uv.x), "{v:?}");
            assert_eq!(v.color, Color32::WHITE.gamma_multiply(0.5), "the item's opacity");
        }
    }
}

/// A picture in a view cut to a circle (a detail view) shows only inside the circle, as the writers
/// cut it: every corner of its mesh lies on or inside the circle and inside the picture.
#[test]
fn a_circle_clip_cuts_the_picture() {
    let ctx = egui::Context::default();
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(10.0, 10.0) };
    let clip = Clip::Circle { center: Vec2::new(5.0, 5.0), radius: 3.0 };
    let marks = Marks::of(&picture_scene(7, rect, 1.0, Some(clip)), 1, 0.01);
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 10, 10, true, None)], 2048);
    let meshes = textured(&painted(&ctx, &marks, &pictures));
    assert_eq!(meshes.len(), 1);
    let vertices = &meshes[0].0.vertices;
    assert!(vertices.len() > 16, "a circle, not the rectangle: {}", vertices.len());
    for v in vertices {
        let paper = Vec2::new(f64::from(v.pos.x) / 10.0, -f64::from(v.pos.y) / 10.0);
        assert!(paper.dist(Vec2::new(5.0, 5.0)) <= 3.0 + 1e-6, "{paper:?}");
        // The texture coordinates follow the paper: u from the left edge, v down from the top.
        assert!((f64::from(v.uv.x) - paper.x / 10.0).abs() < 1e-5 && (f64::from(v.uv.y) - (10.0 - paper.y) / 10.0).abs() < 1e-5, "{v:?}");
    }
}

/// A stale picture (its view changed, the new picture not drawn yet) keeps its last pixels where they
/// were drawn, over its own grid; a fresh one is cut to its item's place, as the export cuts it.
#[test]
fn a_stale_picture_shows_over_its_own_grid_until_the_new_one_comes() {
    let ctx = egui::Context::default();
    // The view is now 10 mm square; its last picture was drawn 20 mm square.
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(10.0, 10.0) };
    let marks = Marks::of(&picture_scene(7, rect, 1.0, None), 1, 0.01);
    let mut pictures = PictureTextures::default();
    let mut last = hand_picture(7, 20, 20, true, None);
    let width = |pictures: &PictureTextures| {
        let meshes = textured(&painted(&ctx, &marks, pictures));
        assert_eq!(meshes.len(), 1);
        meshes[0].1.width()
    };
    pictures.sync(&ctx, std::slice::from_ref(&last), 2048);
    assert!((width(&pictures) - 100.0).abs() < 1e-3, "a fresh picture is cut to its place");
    last.stale = true;
    pictures.sync(&ctx, std::slice::from_ref(&last), 2048);
    assert!((width(&pictures) - 200.0).abs() < 1e-3, "a stale one shows whole, over its own grid");
}

/// A view moved on its sheet keeps its picture's pixels (the same rows) and the engine moves their
/// grid: the page paints them at the new place, past the old one, with no new upload.
#[test]
fn a_moved_views_picture_follows_it_without_a_new_upload() {
    let ctx = egui::Context::default();
    let mut pictures = PictureTextures::default();
    let first = hand_picture(7, 10, 10, true, None);
    pictures.sync(&ctx, std::slice::from_ref(&first), 2048);
    let moved = SheetPicture { grid: PixelGrid { x0: 50.0, ..first.grid }, ..first.clone() };
    pictures.sync(&ctx, std::slice::from_ref(&moved), 2048);
    assert_eq!(pictures.uploads, 1, "the same pixels upload once");
    let rect = Aabb2 { min: Vec2::new(50.0, 0.0), max: Vec2::new(60.0, 10.0) };
    let meshes = textured(&painted(&ctx, &Marks::of(&picture_scene(7, rect, 1.0, None), 1, 0.01), &pictures));
    assert_eq!(meshes.len(), 1, "drawn at its new place");
    // 10 points a millimetre: 500 to 600 points.
    assert!((meshes[0].1.min.x - 500.0).abs() < 1e-3 && (meshes[0].1.max.x - 600.0).abs() < 1e-3, "{:?}", meshes[0].1);
}

/// The page paints in the scene's order: a fill before a picture lies under it, one after it on it.
#[test]
fn fills_and_pictures_paint_in_the_scenes_order() {
    // Whether each mesh in `shapes` is textured (a picture's) or not (a fill's), in paint order.
    fn meshes(shapes: &[egui::epaint::ClippedShape]) -> Vec<bool> {
        fn walk(shape: &egui::Shape, out: &mut Vec<bool>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                egui::Shape::Mesh(mesh) if !mesh.vertices.is_empty() => out.push(mesh.texture_id != egui::TextureId::default()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|s| walk(&s.shape, &mut out));
        out
    }
    let ctx = egui::Context::default();
    let mut pictures = PictureTextures::default();
    pictures.sync(&ctx, &[hand_picture(7, 10, 10, true, None)], 2048);
    let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(10.0, 10.0) };
    let fill = Item::Fill { region: Region2 { outer: square(Vec2::new(2.0, 2.0), 5.0), holes: Vec::new() } };
    let picture = Item::Picture(solidcraft_engine::geom::paper::Picture { source: 7, rect, opacity: 1.0 });
    for (items, expected) in [(vec![fill.clone(), picture.clone()], [false, true]), (vec![picture, fill], [true, false])] {
        let groups = vec![Group { object: 1, kind: ObjectKind::View, items }];
        let scene = Scene { size_mm: Vec2::new(100.0, 100.0), groups, fonts: Vec::new(), clips: Vec::new() };
        assert_eq!(meshes(&painted(&ctx, &Marks::of(&scene, 1, 0.01), &pictures)), expected);
    }
}

/// How long making a sheet picture's textures takes on the window thread: 16 Mpx (the provider's
/// largest) and a view at 300 dpi on A3. Ignored: a measure, not a check (`cargo test -- --ignored`).
#[test]
#[ignore]
fn picture_upload_timing() {
    let ctx = egui::Context::default();
    // A desktop GPU's largest side, so the 16 Mpx picture is one texture, as `sync` is told.
    let _ = frame_on(&ctx, egui::RawInput { max_texture_side: Some(16_384), ..Default::default() }, |_| {});
    for (w, h) in [(4096u32, 4096u32), (1181, 945)] {
        let p = hand_picture(1, w, h, true, None);
        let start = std::time::Instant::now();
        let mut pictures = PictureTextures::default();
        pictures.sync(&ctx, std::slice::from_ref(&p), 16_384);
        println!("picture {w} x {h}: upload {:.1} ms", start.elapsed().as_secs_f64() * 1e3);
        // Each frame after: the same answer again, and its meshes (one tile, then a circle clip).
        let start = std::time::Instant::now();
        for _ in 0..100 {
            pictures.sync(&ctx, std::slice::from_ref(&p), 16_384);
        }
        println!("  sync of the same answer: {:.2} us a frame", start.elapsed().as_secs_f64() * 1e4);
        let rect = Aabb2 { min: Vec2::ZERO, max: Vec2::new(f64::from(w), f64::from(h)) };
        let at = OnScreen { centre: Pos2::ZERO, target: Vec2::ZERO, scale: 0.1 };
        let u = pictures.get(1).unwrap();
        for clip in [None, Some(Clip::Circle { center: rect.center(), radius: f64::from(h) / 3.0 })] {
            let start = std::time::Instant::now();
            for _ in 0..100 {
                for tile in &u.tiles {
                    std::hint::black_box(tile_mesh(&at, &u.grid, tile, rect, clip.as_ref(), 1.0, 0.02));
                }
            }
            println!("  meshes, clip {clip:?}: {:.2} us a frame", start.elapsed().as_secs_f64() * 1e4);
        }
        let _ = frame_on(&ctx, egui::RawInput::default(), |_| {});
    }
}

/// A final synchronous sketch solve is painted in the frame that completes it, without another
/// pointer event or an extra layout pass to make the finished marks visible.
#[test]
fn a_single_sheet_sketch_is_painted_when_its_last_step_finishes() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.ctx.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let drawing = h.app.run("drawing.create", json!({})).unwrap();
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.frames(2);
    let made = h.app.run("drawing.sketch.create", json!({"owner": drawing["sheets"][0]["id"]})).unwrap();
    h.app.run("sketch.line", json!({"points": [[15, 25], [65, 25]], "infer": false})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let (doc, undo) = (h.app.session.doc.to_json(), h.app.session.undo.len());
    h.frame();
    let sheet = active_sheet(&h.app).unwrap();
    let current = drawing_scene::extract(&mut h.app.session, sheet).unwrap();
    assert!(current.stale_sketches.is_empty(), "the single sketch has finished");
    assert!(
        current
            .scene
            .groups
            .iter()
            .any(|group| { group.kind == ObjectKind::Sketch && json!(solidcraft_engine::doc::ids::encode(group.object)) == made["sketch"] })
    );
    let marks = h.app.drawing_workspace.page.marks.as_ref().unwrap();
    assert_eq!(marks.generation, current.generation, "the page painted the completed scene in this frame");
    assert!(marks.marks.iter().any(|mark| matches!(mark, Mark::Line { points, .. }
        if points.as_slice() == [Vec2::new(15.0, 25.0), Vec2::new(65.0, 25.0)])));
    assert_eq!((h.app.session.doc.to_json(), h.app.session.undo.len()), (doc, undo));
}

/// A view's state is in words beside it: one whose settings changed says it needs an update until it
/// is drawn again, and one with nothing to show says why.
#[test]
fn a_views_state_is_in_words() {
    let (mut h, front) = plate_sheet();
    h.app.run("drawing.view.edit", json!({"view": front, "set": {"scale": "1:5"}})).unwrap();
    // The first frame after the edit: the view is drawn beside the window from now on.
    h.frame();
    assert!(crate::scenario::rect_of(&format!("sheet:view:{front}:state")).is_some(), "its words are beside it");
    let p = inspect_now(&h);
    let state = p["views"].as_array().unwrap().iter().find(|v| v["view"] == json!(front)).unwrap().clone();
    assert_eq!(state["out_of_date"], true, "{p}");
    assert_eq!(state["words"], json!(["Needs update"]), "{p}");
    assert_eq!(p["working"], true);
    settle(&mut h);
    let p = page(&mut h);
    assert!(p["views"].as_array().unwrap().iter().all(|v| v["out_of_date"] == false), "{p}");

    // A view with nothing to draw, or one that can't be drawn, says why, out of date or not.
    use solidcraft_engine::drawing_view::Unmeasured;
    let nothing = words(&Status::Placeholder(Unmeasured::NothingChosen), false);
    assert_eq!(nothing, [format!("Nothing to draw: {}", Unmeasured::NothingChosen)]);
    let failed = words(&Status::Failed("too many lines".into()), true);
    assert_eq!(failed, ["Can't draw this view: too many lines", "Needs update"]);
    assert!(words(&Status::Drawn, false).is_empty());
}

/// Without a drawing open the page shows the first-sheet start (#18159), and the control channel's
/// sheet page is empty; outside Drawing there is none either.
#[test]
fn no_drawing_open_shows_the_note() {
    let mut h = Harness::new();
    h.app.home.open = false;
    crate::drawing_workspace::enter(&mut h.app, true).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h), Value::Null);
    assert!(h.widget("Set up your drawing").is_some());
    let (mut h, _) = plate_sheet();
    crate::drawing_workspace::enter(&mut h.app, false).unwrap();
    h.frames(2);
    assert_eq!(page(&mut h), Value::Null);
}

#[test]
fn offscreen_arrow_tracks_the_edge_and_hides_when_paper_is_visible() {
    let area = Rect::from_min_max(pos2(50.0, 20.0), pos2(250.0, 220.0));
    let sheet_at = |x, y| Rect::from_center_size(pos2(x, y), vec2(30.0, 40.0));
    let (centre, direction) = offscreen_arrow(area, sheet_at(350.0, 120.0), 20.0).unwrap();
    assert_eq!(centre, pos2(230.0, 120.0));
    assert_eq!(direction, vec2(1.0, 0.0));

    let (lower, lower_direction) = offscreen_arrow(area, sheet_at(350.0, 200.0), 20.0).unwrap();
    assert_eq!(lower.x, centre.x);
    assert!(lower.y > centre.y);
    assert!(lower_direction.x > 0.0 && lower_direction.y > 0.0);

    let (above, up) = offscreen_arrow(area, sheet_at(150.0, -60.0), 20.0).unwrap();
    assert_eq!(above, pos2(150.0, 40.0));
    assert_eq!(up, vec2(0.0, -1.0));

    assert!(offscreen_arrow(area, sheet_at(260.0, 120.0), 20.0).is_none());
    assert!(offscreen_arrow(area, sheet_at(350.0, 120.0), 110.0).is_none());
}

/// Every path's points in `shapes`, in paint order: a stroked line or outline.
fn paths(shapes: &[egui::epaint::ClippedShape]) -> Vec<Vec<Pos2>> {
    fn walk(shape: &egui::Shape, out: &mut Vec<Vec<Pos2>>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            egui::Shape::Path(path) => out.push(path.points.clone()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

/// A drag shows a view where it goes with its own lines, but a detail view's callout on the parent (its
/// outline and letter) stays: the preview of the moving view leaves it out, while lighting the same
/// view for a hover still draws it (#20981).
#[test]
fn a_drags_preview_leaves_a_details_callout_where_it_is() {
    let line =
        Item::Stroke { path: vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }], closed: false, width_mm: 0.35, pattern: None };
    let ring = Item::Stroke {
        path: vec![Seg2::Arc { center: Vec2::new(50.0, 50.0), radius: 5.0, start: 0.0, sweep: std::f64::consts::TAU }],
        closed: true,
        width_mm: 0.18,
        pattern: None,
    };
    let groups =
        vec![Group { object: 2, kind: ObjectKind::View, items: vec![line] }, Group { object: 2, kind: ObjectKind::ViewLabel, items: vec![ring] }];
    let scene = Scene { size_mm: Vec2::new(100.0, 100.0), groups, fonts: Vec::new(), clips: Vec::new() };
    let marks = Marks::of(&scene, 1, 0.01);
    assert!(marks.error.is_none(), "{:?}", marks.error);
    let ctx = egui::Context::default();
    let pictures = PictureTextures::default();
    let visible = Aabb2 { min: Vec2::new(-1e3, -1e3), max: Vec2::new(1e3, 1e3) };
    // The preview's camera: the view 30 mm to the right, as the drag's offset shifts the paper.
    let shifted = OnScreen { centre: Pos2::ZERO, target: Vec2::new(-30.0, 0.0), scale: 10.0 };
    let run = |paint: &dyn Fn(&egui::Painter)| frame_on(&ctx, egui::RawInput::default(), |ui| paint(ui.painter())).shapes;
    let moving = run(&|painter| marks.paint_moving(painter, &shifted, visible, 1.0, &pictures, &[2], Color32::RED));
    let drawn = paths(&moving);
    assert_eq!(drawn.len(), 1, "only the view's own line moves, not the callout: {drawn:?}");
    assert!(drawn[0].iter().all(|p| p.y.abs() < 1e-3), "it is the view's line: {drawn:?}");
    let lit = run(&|painter| marks.paint_lit(painter, &shifted, visible, 1.0, &pictures, &[2], Color32::RED));
    assert_eq!(paths(&lit).len(), 2, "a lit detail still shows its callout");
}

/// Dragging a detail view moves the detail and leaves its callout on the parent where it was, in the
/// scene the sheet draws after the release (#20981).
#[test]
fn dragging_a_detail_view_leaves_its_callout_in_place() {
    use solidcraft_engine::Sel;
    let (mut h, front) = plate_sheet();
    let made = h
        .app
        .run(
            "drawing.view.detail",
            json!({"parent": front, "boundary_mm": {"circle": {"centre": [140, 180], "radius": 10}}, "place": {"at": [300, 100]}}),
        )
        .unwrap();
    let detail = solidcraft_engine::doc::ids::decode(made["view"].as_str().unwrap()).unwrap();
    settle(&mut h);
    let sheet = crate::drawing_sheet_page::active_sheet(&h.app).unwrap();
    let callout = |h: &mut Harness| {
        let scene = drawing_scene::extract(&mut h.app.session, sheet).unwrap().scene;
        let found = scene
            .groups
            .iter()
            .find(|g| g.object == detail && g.kind == ObjectKind::ViewLabel && g.items.iter().any(|i| matches!(i, Item::Stroke { .. })));
        format!("{:?}", found.map(|g| &g.items))
    };
    let before = callout(&mut h);
    assert!(before.contains("Stroke"), "the callout is drawn: {before}");
    let drawing = h.app.session.drawing("drawing.inspect").unwrap().clone();
    h.app.session.selection = vec![Sel::Drawing { drawing: drawing.id, object: detail }];
    let from = drawing.view_centre(detail).unwrap();
    let by = Vec2::new(-40.0, 30.0);
    h.step(&json!({"drag": [on_screen(from.x, from.y), on_screen(from.x + by.x, from.y + by.y)], "steps": 6})).unwrap();
    h.frames(2);
    let after = h.app.session.drawing("drawing.inspect").unwrap().view_centre(detail).unwrap();
    let moved = after - from;
    assert!((moved.x - by.x).abs() < 1.0 && (moved.y - by.y).abs() < 1.0, "the detail moved by {moved:?}, not {by:?}");
    assert_eq!(callout(&mut h), before, "the callout stays where it was");
}
