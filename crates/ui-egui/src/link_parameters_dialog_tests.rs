use std::path::{Path, PathBuf};

use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::context_menu::{self, Target as MenuTarget};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-link-parameters-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 10 × `w` × `h` block (user parameters `h` 5 mm and `w` 20 mm), saved at `path`.
fn save_block(path: &Path) {
    let mut part = Session::default();
    part.execute("parameters.add", &json!({"name": "h", "expression": "5 mm"})).unwrap();
    part.execute("parameters.add", &json!({"name": "w", "expression": "20 mm"})).unwrap();
    part.execute("solid.box", &json!({"length": 10, "width": "w", "height": "h", "body_name": "Block"})).unwrap();
    part.execute("file.save", &json!({"path": path})).unwrap();
}

fn settle(app: &mut SolveApp) {
    let ctx = egui::Context::default();
    let began = std::time::Instant::now();
    while !app.file_load.idle() {
        assert!(began.elapsed() < std::time::Duration::from_secs(120), "the insert went on");
        crate::file_load::tick(app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn volume(app: &SolveApp) -> f64 {
    crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| solidcraft_engine::kernel::measure(&b.body).unwrap().volume).sum()
}

/// Wait for the linked design's parameters, read beside the window, and take them in as the
/// window would.
fn read(app: &mut SolveApp) {
    let reading = app.link_parameters_reading.take().expect("a read was started");
    let outcome = reading.done.recv_timeout(std::time::Duration::from_secs(120)).expect("the read finishes");
    assert_eq!(reading.tab, app.docs.active_tab());
    if reading.refresh {
        refreshed(app, &reading.target, reading.tab, reading.source, outcome);
    } else {
        finish(app, reading.target, reading.tab, reading.source, outcome);
    }
}

fn row<'a>(form: &'a mut Form, name: &str) -> &'a mut Row {
    form.rows.iter_mut().find(|r| r.name == name).unwrap()
}

/// Insert Design picks the file, the dialog asks which values the link passes, and OK inserts
/// with them, as a person does it.
#[test]
fn insert_design_asks_which_values_the_link_passes_and_inserts_with_them() {
    let dir = folder("insert");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut h = crate::scenario::Harness::new();
    h.app.run("parameters.add", json!({"name": "h", "expression": "8 mm"})).unwrap();
    h.app.run("parameters.add", json!({"name": "lift", "expression": "30 mm"})).unwrap();
    h.step(&json!({"choose_file": part.to_string_lossy()})).unwrap();
    h.app.pick_design_to_insert();
    // The design is read beside the window; the window goes on drawing meanwhile.
    for _ in 0..2000 {
        if h.app.link_parameters.is_some() {
            break;
        }
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let form = h.app.link_parameters.clone().expect("the dialog asks before inserting");
    assert!(h.app.session.doc.links.is_empty(), "nothing is inserted before OK");
    assert!(!form.by_name, "unticked, although this design has an `h` too: the default is the design's saved sizes");
    let names: Vec<&str> = form.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["h", "w"]);
    assert_eq!((form.rows[0].design.as_str(), form.rows[0].here.as_deref()), ("5 mm", Some("8 mm")));
    assert_eq!(form.rows[1].here, None, "this design has no `w`");
    for handle in ["link_parameters:by_name", "link_parameters:ok", "link_parameters:cancel", "link_parameter:keep:w"] {
        assert!(crate::scenario::handle_at(handle).is_some(), "missing {handle}");
    }

    // w follows `lift`; h comes by name once the box is ticked.
    row(h.app.link_parameters.as_mut().unwrap(), "w").expression = "lift".into();
    h.frames(2);
    h.step(&json!({"click": {"handle": "link_parameters:by_name"}})).unwrap();
    h.frames(2);
    assert!(h.app.link_parameters.as_ref().unwrap().by_name);
    h.step(&json!({"click": {"handle": "link_parameters:ok"}})).unwrap();
    h.frames(2);
    settle(&mut h.app);
    assert!(h.app.link_parameters.is_none());
    let link = h.app.session.doc.links.values().next().expect("inserted").clone();
    assert!(link.parameters_by_name);
    assert_eq!(link.parameters.get("w").map(String::as_str), Some("lift"));
    assert!((volume(&h.app) - 10.0 * 30.0 * 8.0).abs() < 1e-6, "{}", volume(&h.app));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancel_inserts_nothing_and_a_design_without_user_parameters_is_not_asked_about() {
    let dir = folder("cancel");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    ask_insert(&mut app, part.to_str().unwrap(), None);
    read(&mut app);
    assert!(!app.link_parameters.as_ref().unwrap().by_name, "no name matches: unticked");
    app.link_parameters = None;
    assert!(app.session.doc.links.is_empty());

    let plain = dir.join("plain.sc");
    let mut part = Session::default();
    part.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
    part.execute("file.save", &json!({"path": plain})).unwrap();
    ask_insert(&mut app, plain.to_str().unwrap(), None);
    read(&mut app);
    assert!(app.link_parameters.is_none(), "nothing to ask");
    settle(&mut app);
    assert_eq!(app.session.doc.links.len(), 1, "inserted as before");
    let _ = std::fs::remove_dir_all(dir);
}

/// Link Parameters… on a linked component shows what its link passes now, and OK sets the new
/// values; a refused setting keeps the dialog open with the reason.
#[test]
fn link_parameters_edits_an_existing_link_and_a_refusal_keeps_the_dialog() {
    let dir = folder("link");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "h", "expression": "8 mm"})).unwrap();
    app.run("parameters.add", json!({"name": "w", "expression": "30 mm"})).unwrap();
    let inserted = app.run("component.insert_design", json!({"path": part, "name": "Block", "by_name": true})).unwrap();
    let component = inserted["component"].as_u64().unwrap();

    let item = context_menu::entries(&app, &MenuTarget::Component { id: component }).into_iter().find(|i| i.id == "ui.linkParameters");
    assert!(item.is_some(), "the linked component's menu has Link Parameters…");
    open_link(&mut app, component);
    read(&mut app);
    // OK closes the dialog before it applies, as the window does.
    let mut form = app.link_parameters.take().expect("open");
    assert!(form.by_name);
    assert_eq!(form.rows.iter().map(|r| (r.name.as_str(), r.keep)).collect::<Vec<_>>(), [("h", false), ("w", false)]);

    // Keep `w` at the block's own 20 mm.
    row(&mut form, "w").keep = true;
    apply(&mut app, &form).unwrap();
    let link = app.session.doc.links.get(&component).unwrap();
    assert_eq!(link.parameters_kept.iter().collect::<Vec<_>>(), ["w"]);
    assert!((volume(&app) - 10.0 * 20.0 * 8.0).abs() < 1e-6);

    // An expression that doesn't work out here is refused and changes nothing.
    open_link(&mut app, component);
    read(&mut app);
    let mut form = app.link_parameters.take().unwrap();
    assert!(row(&mut form, "w").keep, "the dialog shows what the link keeps");
    row(&mut form, "h").expression = "nothing_here".into();
    let refused = apply(&mut app, &form).unwrap_err();
    assert!(refused.contains("nothing_here"), "{refused}");
    assert!(app.session.doc.links.get(&component).unwrap().parameters.is_empty());

    // The Parameters window and the Browser say what the link passes.
    let passed = passed(&app);
    assert!(passed.iter().any(|l| l == "h → Block: h = 8 mm"), "{passed:?}");
    assert!(passed.iter().any(|l| l == "Block: w keeps its own value"), "{passed:?}");
    let tip = tooltip(&app.session.doc, component).unwrap();
    assert!(tip.contains("h = 8 mm") && tip.contains("w: its own value"), "{tip}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_value_edit_that_a_link_cannot_pass_is_said_in_the_parameters_window() {
    let dir = folder("warnings");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "12 mm"})).unwrap();
    app.run("component.insert_design", json!({"path": part, "name": "Block", "parameters": {"h": "lift"}})).unwrap();
    // The file goes: the next value edit keeps the link's bodies and says why.
    std::fs::remove_file(&part).unwrap();
    app.run("parameters.change", json!({"name": "lift", "expression": "7 mm"})).unwrap();
    assert!(app.link_warnings.iter().any(|w| w.contains("Block keeps its bodies")), "{:?}", app.link_warnings);
    // Another edit still can't pass `lift` (7 mm, the bodies have 12 mm): it says so again.
    app.run("parameters.add", json!({"name": "other", "expression": "1 mm"})).unwrap();
    app.run("parameters.change", json!({"name": "other", "expression": "2 mm"})).unwrap();
    assert!(app.link_warnings.iter().any(|w| w.contains("Block keeps its bodies")), "{:?}", app.link_warnings);
    // With `lift` back at the value the bodies have, there is nothing to pass and nothing to say.
    app.run("parameters.change", json!({"name": "lift", "expression": "12 mm"})).unwrap();
    assert!(app.link_warnings.is_empty(), "each edit's own report replaces the last: {:?}", app.link_warnings);
    let _ = std::fs::remove_dir_all(dir);
}

/// A link that takes dozens of values by name (a whole box's sizes) lists the link's own
/// expressions first and only a few values in the tooltip, rounded; the Parameters window has
/// them all.
#[test]
fn a_long_list_of_passed_values_is_cut_short_in_the_tooltip_and_rounded() {
    // The app installs the message catalogs.
    let _app = SolveApp::new(Session::default(), Default::default());
    let mut doc = solidcraft_engine::doc::Document::default();
    let mut link = solidcraft_engine::doc::DesignLink { parameters_by_name: true, ..Default::default() };
    for i in 0..12 {
        link.parameter_values.insert(format!("a{i:02}"), "43.857142857142854 mm".into());
    }
    link.parameters.insert("z_own".into(), "lift".into());
    link.parameter_values.insert("z_own".into(), "7 mm".into());
    doc.links.insert(5, link);
    let tip = tooltip(&doc, 5).unwrap();
    let lines: Vec<&str> = tip.lines().collect();
    assert_eq!(lines.len(), 1 + TOOLTIP_VALUES + 1, "{tip}");
    assert_eq!(lines[1], "z_own = 7 mm", "the link's own expression comes first");
    assert_eq!(lines[2], "a00 = 43.8571 mm");
    assert!(lines.last().unwrap().contains("5 more"), "{tip}");
    assert_eq!(shown("30"), "30");
}

/// The dialog, and the read that opens it, belong to the design they were opened in: another
/// tab with a linked component of the same id is never changed by them, and the design's own
/// tab gets them back when it keeps its work (review of PR #1).
#[test]
fn the_dialog_stays_with_its_design_when_the_tab_changes() {
    let dir = folder("tabs");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    let first = app.run("component.insert_design", json!({"path": part, "name": "First"})).unwrap()["component"].as_u64().unwrap();
    let mut other = Session::default();
    let second = other.execute("component.insert_design", &json!({"path": part, "name": "Second"})).unwrap()["component"].as_u64().unwrap();
    assert_eq!(first, second, "component ids are each design's own");

    // Without "keep dialogs open when switching designs", leaving the tab cancels the dialog.
    open_link(&mut app, first);
    read(&mut app);
    row(app.link_parameters.as_mut().unwrap(), "h").expression = "42 mm".into();
    crate::documents::open_tab(&mut app, other).unwrap();
    assert!(app.link_parameters.is_none() && app.link_parameters_reading.is_none(), "the other design has no dialog");
    assert!(app.session.doc.links.get(&second).unwrap().parameters.is_empty());

    // With it, the dialog and a read still going wait with their design.
    app.preferences.keep_work_on_tab_switch = true;
    open_link(&mut app, second);
    let back = app.docs.active_tab();
    crate::documents::open_tab(&mut app, Session::default()).unwrap();
    assert!(app.link_parameters_reading.is_none(), "the read went with its tab");
    let mut form = Form::from_reply(Target::Link { component: second }, back, &json!({"parameters": [{"name": "h", "unit": "mm"}]}));
    row(&mut form, "h").expression = "42 mm".into();
    assert_eq!(apply(&mut app, &form), Ok(()), "a form from another tab does nothing");
    assert!(app.session.doc.links.is_empty());
    // Back in its tab, the read is there to finish.
    crate::documents::switch(&mut app, 1);
    assert_eq!(app.docs.active_tab(), back);
    read(&mut app);
    assert!(app.link_parameters.as_ref().is_some_and(|f| f.tab == back && f.target == Target::Link { component: second }));
    let _ = std::fs::remove_dir_all(dir);
}

/// A Link Parameters dialog, and a read on its way to one, belong to the design the link read
/// when they started. After Relink Source the old design's rows and what was typed in them are
/// never set on the new design: the dialog reads the link again (review of #20542).
#[test]
fn a_relink_reads_the_dialog_again_and_drops_the_old_designs_draft() {
    let dir = folder("relinked");
    let (part, other) = (dir.join("block.sc"), dir.join("other.sc"));
    save_block(&part);
    let mut wide = Session::default();
    wide.execute("parameters.add", &json!({"name": "h", "expression": "7 mm"})).unwrap();
    wide.execute("solid.box", &json!({"length": 10, "width": 30, "height": "h", "body_name": "Block"})).unwrap();
    wide.execute("file.save", &json!({"path": other})).unwrap();
    let mut app = SolveApp::new(Session::default(), Default::default());
    let component = app.run("component.insert_design", json!({"path": part, "name": "Block"})).unwrap()["component"].as_u64().unwrap();

    // The dialog is open with a draft when the link is given another design.
    open_link(&mut app, component);
    read(&mut app);
    row(app.link_parameters.as_mut().unwrap(), "h").expression = "42 mm".into();
    let draft = app.link_parameters.clone().unwrap();
    app.run("component.relink", json!({"component": component, "path": other})).unwrap();
    assert_eq!(apply(&mut app, &draft), Ok(()), "OK on the old rows sets nothing");
    assert!(app.session.doc.links.get(&component).unwrap().parameters.is_empty());
    follow_source(&mut app);
    assert!(app.link_parameters.is_none(), "the old design's rows are gone");
    read(&mut app);
    let form = app.link_parameters.clone().unwrap();
    assert!(form.file.ends_with("other.sc"), "{}", form.file);
    assert_eq!(form.rows.iter().map(|r| (r.name.as_str(), r.expression.as_str())).collect::<Vec<_>>(), [("h", "")]);
    assert_eq!(apply(&mut app, &form), Ok(()));

    // A read that was still going when the link changed opens nothing of the old design.
    open_link(&mut app, component);
    app.run("component.relink", json!({"component": component, "path": part})).unwrap();
    follow_source(&mut app);
    read(&mut app);
    let form = app.link_parameters.clone().unwrap();
    assert!(form.file.ends_with("block.sc"), "{}", form.file);
    assert_eq!(form.rows.len(), 2, "the block's `h` and `w`");
    let _ = std::fs::remove_dir_all(dir);
}

/// A link by name that matches no name passes nothing, so the Browser shows no badge for it; a
/// design that replaces the shown one takes none of the old one's dialogs (review of #20542).
#[test]
fn nothing_passed_shows_no_badge_and_a_replaced_design_keeps_no_dialog() {
    let dir = folder("nothing-passed");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    let component =
        app.run("component.insert_design", json!({"path": part, "name": "Block", "by_name": true})).unwrap()["component"].as_u64().unwrap();
    assert_eq!(tooltip(&app.session.doc, component), None, "no name matches");
    app.run("parameters.add", json!({"name": "h", "expression": "9 mm"})).unwrap();
    assert!(tooltip(&app.session.doc, component).is_some_and(|t| t.contains("h = 9 mm")));

    open_link(&mut app, component);
    read(&mut app);
    open_link(&mut app, component);
    design_replaced(&mut app);
    assert!(app.link_parameters.is_none() && app.link_parameters_reading.is_none() && app.link_parameters_waiting.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// What the last value edit could not pass belongs to that design and is old news after any
/// later change (review of PR #1).
#[test]
fn link_warnings_stay_with_their_design_and_go_with_the_next_change() {
    let dir = folder("warnings-tabs");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "12 mm"})).unwrap();
    app.run("component.insert_design", json!({"path": part, "name": "Block", "parameters": {"h": "lift"}})).unwrap();
    std::fs::remove_file(&part).unwrap();
    app.run("parameters.change", json!({"name": "lift", "expression": "7 mm"})).unwrap();
    assert!(!app.link_warnings.is_empty());
    crate::documents::open_tab(&mut app, Session::default()).unwrap();
    assert!(app.link_warnings.is_empty(), "the new design has none");
    crate::documents::switch(&mut app, 0);
    assert!(app.link_warnings.iter().any(|w| w.contains("Block keeps its bodies")), "back with its design: {:?}", app.link_warnings);
    // The same through the commands the keys run: going to a tab is no change to its design.
    app.run("view.next_tab", json!({})).unwrap();
    assert!(app.link_warnings.is_empty(), "the other design has none: {:?}", app.link_warnings);
    app.run("view.previous_tab", json!({})).unwrap();
    assert!(app.link_warnings.iter().any(|w| w.contains("Block keeps its bodies")), "back by the keys: {:?}", app.link_warnings);
    app.run("view.next_tab", json!({})).unwrap();
    app.run("view.next_tab", json!({})).unwrap();
    assert!(app.link_warnings.iter().any(|w| w.contains("Block keeps its bodies")), "round again: {:?}", app.link_warnings);
    app.run("edit.undo", json!({})).unwrap();
    assert!(app.link_warnings.is_empty(), "Undo took the edit back: {:?}", app.link_warnings);
    let _ = std::fs::remove_dir_all(dir);
}

/// OK computing beside the window: the draft waits with the command. Refused once the rebuild is
/// in (the linked file changed meanwhile), the dialog opens again with its expressions and the
/// reason; stopped, it opens as it was; done, it stays closed (review of PR #1).
#[test]
fn a_refusal_after_the_rebuild_brings_the_dialog_back_with_its_draft() {
    let dir = folder("deferred");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "12 mm"})).unwrap();
    let component = app.run("component.insert_design", json!({"path": part, "name": "Block"})).unwrap()["component"].as_u64().unwrap();
    // Every history command computes beside the window, however quick.
    app.apply_off_thread = Some(std::time::Duration::ZERO);
    let ctx = egui::Context::default();
    let wait = |app: &mut SolveApp, serial: u64| {
        let began = std::time::Instant::now();
        while app.heavy_pending(serial) {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "the command ends");
            app.poll_deferred(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    };
    let draft = |app: &mut SolveApp| {
        open_link(app, component);
        read(app);
        let mut form = app.link_parameters.take().expect("open");
        row(&mut form, "h").expression = "lift".into();
        form
    };

    // Refused: the block's file changed after the dialog read it.
    let form = draft(&mut app);
    let mut changed = Session::default();
    changed.execute("parameters.add", &json!({"name": "h", "expression": "6 mm"})).unwrap();
    changed.execute("solid.box", &json!({"length": 10, "width": 20, "height": "h", "body_name": "Block"})).unwrap();
    changed.execute("file.save", &json!({"path": part})).unwrap();
    assert_eq!(apply(&mut app, &form), Ok(()), "taken on");
    let serial = app.link_parameters_waiting.as_ref().map(|(s, _)| *s).expect("the draft waits with its command");
    assert!(app.link_parameters.is_none(), "closed while it computes");
    wait(&mut app, serial);
    let back = app.link_parameters.clone().expect("the dialog is back");
    assert_eq!(back.rows.iter().find(|r| r.name == "h").map(|r| r.expression.as_str()), Some("lift"), "with its expressions");
    assert!(back.error.as_deref().is_some_and(|e| e.contains("changed since")), "and the reason: {:?}", back.error);
    assert!(app.link_parameters_waiting.is_none());
    assert!(app.session.doc.links.get(&component).unwrap().parameters.is_empty(), "nothing changed");

    // Stopped: it comes back as it was, with no reason to give.
    app.link_parameters = None;
    app.run("component.update_links", json!({})).unwrap();
    let form = draft(&mut app);
    assert_eq!(apply(&mut app, &form), Ok(()));
    assert!(app.link_parameters_waiting.is_some());
    app.deferred = None;
    app.deferred_queue.clear();
    stopped(&mut app);
    let back = app.link_parameters.take().expect("back after Stop");
    assert_eq!((back.rows.iter().find(|r| r.name == "h").map(|r| r.expression.as_str()), back.error.as_deref()), (Some("lift"), None));

    // Done: closed for good, and the link passes the value.
    assert_eq!(apply(&mut app, &back), Ok(()));
    let serial = app.link_parameters_waiting.as_ref().map(|(s, _)| *s).expect("waits");
    wait(&mut app, serial);
    stopped(&mut app);
    assert!(app.link_parameters.is_none() && app.link_parameters_waiting.is_none());
    assert_eq!(app.session.doc.links.get(&component).unwrap().parameters.get("h").map(String::as_str), Some("lift"));
    let _ = std::fs::remove_dir_all(dir);
}

/// While one Link Parameters OK computes, the design opens no other of these dialogs, so the
/// draft that comes back when it is refused is never lost to another one (review of PR #1).
#[test]
fn a_second_dialog_waits_until_the_first_ok_is_done() {
    let dir = folder("second");
    let part = dir.join("block.sc");
    let other = dir.join("other.sc");
    save_block(&part);
    save_block(&other);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "12 mm"})).unwrap();
    let a = app.run("component.insert_design", json!({"path": part, "name": "A"})).unwrap()["component"].as_u64().unwrap();
    let b = app.run("component.insert_design", json!({"path": other, "name": "B"})).unwrap()["component"].as_u64().unwrap();
    app.apply_off_thread = Some(std::time::Duration::ZERO);

    // A's OK is held computing; its file changes meanwhile, so it will be refused.
    open_link(&mut app, a);
    read(&mut app);
    let mut form = app.link_parameters.take().unwrap();
    row(&mut form, "h").expression = "lift".into();
    // A read for B already going when A's OK is given.
    open_link(&mut app, b);
    let early = app.link_parameters_reading.take().expect("B's read started before A's OK");
    let mut changed = Session::default();
    changed.execute("parameters.add", &json!({"name": "h", "expression": "6 mm"})).unwrap();
    changed.execute("solid.box", &json!({"length": 10, "width": 20, "height": "h", "body_name": "Block"})).unwrap();
    changed.execute("file.save", &json!({"path": part})).unwrap();
    assert_eq!(apply(&mut app, &form), Ok(()));
    let serial = app.link_parameters_waiting.as_ref().map(|(s, _)| *s).expect("A waits");

    // B's dialog, Link Parameters or Insert Design, does not open while A computes.
    open_link(&mut app, b);
    assert!(app.link_parameters_reading.is_none(), "no read starts");
    ask_insert(&mut app, &other.to_string_lossy(), None);
    assert!(app.link_parameters_reading.is_none(), "Insert Design waits too");
    let outcome = early.done.recv_timeout(std::time::Duration::from_secs(120)).unwrap();
    finish(&mut app, early.target, early.tab, early.source, outcome);
    assert!(app.link_parameters.is_none(), "a read already going opens nothing");
    assert!(
        app.status.as_ref().is_some_and(|s| format!("{s:?}").contains(&solidcraft_ui_text::tr("ui.link_parameters.busy").to_string())),
        "the status line says why"
    );

    // A is refused: its draft comes back with the reason.
    let ctx = egui::Context::default();
    let began = std::time::Instant::now();
    while app.heavy_pending(serial) {
        assert!(began.elapsed() < std::time::Duration::from_secs(120));
        app.poll_deferred(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let back = app.link_parameters.clone().expect("A's draft is back");
    assert_eq!(back.target, Target::Link { component: a });
    assert_eq!(back.rows.iter().find(|r| r.name == "h").map(|r| r.expression.as_str()), Some("lift"));
    assert!(back.error.is_some());
    // Then B's opens as before.
    app.link_parameters = None;
    open_link(&mut app, b);
    read(&mut app);
    assert_eq!(app.link_parameters.as_ref().map(|f| f.target.clone()), Some(Target::Link { component: b }));
    let _ = std::fs::remove_dir_all(dir);
}

/// The other order: A's OK is refused, its draft comes back, and only then does a read for B
/// that started earlier finish. B's dialog does not take A's place, so A's target, its typed
/// expression and the reason stay on screen; nor does a new read start while A is open
/// (review of PR #1).
#[test]
fn a_late_read_never_replaces_a_refused_draft() {
    let dir = folder("late-read");
    let part = dir.join("block.sc");
    let other = dir.join("other.sc");
    save_block(&part);
    save_block(&other);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "12 mm"})).unwrap();
    let a = app.run("component.insert_design", json!({"path": part, "name": "A"})).unwrap()["component"].as_u64().unwrap();
    let b = app.run("component.insert_design", json!({"path": other, "name": "B"})).unwrap()["component"].as_u64().unwrap();
    app.apply_off_thread = Some(std::time::Duration::ZERO);

    // B's read starts first and is held back.
    open_link(&mut app, b);
    let late = app.link_parameters_reading.take().expect("B's read started");
    // A's dialog: its OK computes, and its file changes meanwhile, so it is refused.
    open_link(&mut app, a);
    read(&mut app);
    let mut form = app.link_parameters.take().unwrap();
    row(&mut form, "h").expression = "lift".into();
    let mut changed = Session::default();
    changed.execute("parameters.add", &json!({"name": "h", "expression": "6 mm"})).unwrap();
    changed.execute("solid.box", &json!({"length": 10, "width": 20, "height": "h", "body_name": "Block"})).unwrap();
    changed.execute("file.save", &json!({"path": part})).unwrap();
    assert_eq!(apply(&mut app, &form), Ok(()));
    let serial = app.link_parameters_waiting.as_ref().map(|(s, _)| *s).expect("A waits");
    let ctx = egui::Context::default();
    let began = std::time::Instant::now();
    while app.heavy_pending(serial) {
        assert!(began.elapsed() < std::time::Duration::from_secs(120));
        app.poll_deferred(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let shown = |app: &SolveApp| {
        let f = app.link_parameters.as_ref().expect("A's draft is on screen");
        (f.target.clone(), f.rows.iter().find(|r| r.name == "h").map(|r| r.expression.clone()), f.error.is_some())
    };
    let refused = (Target::Link { component: a }, Some("lift".to_string()), true);
    assert_eq!(shown(&app), refused, "A is back with its draft and the reason");

    // B's read finishes now: A stays.
    let outcome = late.done.recv_timeout(std::time::Duration::from_secs(120)).unwrap();
    finish(&mut app, late.target, late.tab, late.source, outcome);
    assert_eq!(shown(&app), refused, "a late read does not take A's place");
    assert!(app.status.as_ref().is_some_and(|s| s.0 == solidcraft_ui_text::tr("ui.link_parameters.open_already")), "{:?}", app.status);
    // Nor does a new one start while A is open: the only read is A's own, reading its changed
    // file again for its draft ([`refused`]).
    open_link(&mut app, b);
    assert!(app.link_parameters_reading.as_ref().is_none_or(|r| r.refresh && r.target == Target::Link { component: a }));
    assert_eq!(shown(&app), refused);
    read(&mut app);
    assert_eq!(shown(&app), refused, "A's draft over its file as it is now");
    assert!(app.link_parameters.as_ref().is_some_and(|f| f.changed), "and it offers to take the file in");
    let _ = std::fs::remove_dir_all(dir);
}

/// A design open in another tab, picked from Insert Design's list, asks which values its link
/// passes as a picked file does, and Cancel inserts nothing; OK inserts it with them and keeps
/// the note about its unsaved changes (review of PR #1: the list inserted at the saved sizes,
/// without asking).
#[test]
fn a_design_open_in_another_tab_asks_too() {
    let dir = folder("open-tab");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let mut open = Session::default();
    open.execute("doc.open", &json!({"path": part})).unwrap();
    crate::documents::open_tab(&mut h.app, open).unwrap();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0]})).unwrap();
    crate::documents::switch(&mut h.app, 0);
    h.app.run("parameters.add", json!({"name": "h", "expression": "8 mm"})).unwrap();
    let name = h.app.docs.background().next().map(|s| s.doc.name.clone()).unwrap();
    let pick = |h: &mut crate::scenario::Harness| {
        let item = context_menu::entries(&h.app, &MenuTarget::InsertDesign).into_iter().find(|i| i.label == name).expect("listed");
        context_menu::run_item(&mut h.app, &item, egui::pos2(0.0, 0.0));
        h.settle();
    };

    pick(&mut h);
    let form = h.app.link_parameters.clone().expect("the dialog asks before inserting");
    assert!(!form.by_name && h.app.session.doc.links.is_empty(), "{form:?}");
    h.step(&json!({"click": {"handle": "link_parameters:cancel"}})).unwrap();
    h.settle();
    assert!(h.app.link_parameters.is_none() && h.app.session.doc.links.is_empty() && h.app.session.doc.occurrences.is_empty());

    pick(&mut h);
    h.step(&json!({"click": {"handle": "link_parameters:ok"}})).unwrap();
    h.settle();
    // OK with the box left unticked inserts the design at its saved sizes, as before this dialog.
    let link = h.app.session.doc.links.values().next().expect("inserted").clone();
    assert!(!link.parameters_by_name && link.parameters.is_empty(), "{link:?}");
    assert!((volume(&h.app) - 10.0 * 20.0 * 5.0).abs() < 1e-6, "{}", volume(&h.app));
    assert!(h.app.status.as_ref().is_some_and(|(s, _, _)| s.contains("unsaved changes")), "{:?}", h.app.status);
    let _ = std::fs::remove_dir_all(dir);
}

/// Link Parameters… on a link whose file changed since this design read it (a parameter renamed
/// there): the dialog lists the file's parameters as they are now and offers to take the file in;
/// OK without it is refused, with it the link takes the file and the new name in one step
/// (review of PR #1: neither Update Links nor Link Parameters could repair such a link).
#[test]
fn a_changed_file_is_taken_in_only_when_the_dialog_is_asked_to() {
    let dir = folder("changed");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "8 mm"})).unwrap();
    app.run("component.insert_design", json!({"path": part, "name": "Block", "parameters": {"h": "lift"}})).unwrap();
    let component = *app.session.doc.links.keys().next().unwrap();
    // The block's `h` becomes `height` on disk.
    let mut edited = Session::default();
    edited.execute("doc.open", &json!({"path": part})).unwrap();
    edited.execute("parameters.rename", &json!({"name": "h", "new_name": "height"})).unwrap();
    edited.execute("file.save", &json!({"path": part})).unwrap();

    open_link(&mut app, component);
    read(&mut app);
    let mut form = app.link_parameters.take().expect("the dialog opens");
    assert!(form.changed && !form.update, "{form:?}");
    let names: Vec<&str> = form.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["height", "w"], "the file's parameters as they are now");
    assert_eq!(form.gone, ["h"], "the old name's expression goes");
    form.rows[0].expression = "lift".into();
    let refused = apply(&mut app, &form).expect_err("a changed file is refused unasked");
    assert!(refused.contains("update"), "{refused}");
    form.update = true;
    apply(&mut app, &form).unwrap();
    let link = app.session.doc.links.get(&component).unwrap().clone();
    assert_eq!(link.parameters.get("height").map(String::as_str), Some("lift"));
    assert!(!link.parameters.contains_key("h"));
    assert_eq!(link.file_sha256.as_deref(), Some(solidcraft_engine::doc::DesignLink::file_digest(&std::fs::read(&part).unwrap()).as_str()));
    assert!((volume(&app) - 10.0 * 20.0 * 8.0).abs() < 1e-6, "{}", volume(&app));
    let _ = std::fs::remove_dir_all(dir);
}

/// A Link Parameters dialog whose OK is refused because the linked file changed after the dialog
/// read it (a parameter renamed there): the dialog reads the file again and lists it as it is
/// now, keeping what was typed, offers to take it in, and OK with that works, with no cancel
/// and no draft lost (review of PR #1, round 2).
#[test]
fn a_file_changed_while_the_dialog_is_open_is_offered_after_the_refusal() {
    let dir = folder("changed-open");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("parameters.add", json!({"name": "lift", "expression": "8 mm"})).unwrap();
    app.run("parameters.add", json!({"name": "wide", "expression": "30 mm"})).unwrap();
    let inserted = app.run("component.insert_design", json!({"path": part, "name": "Block"})).unwrap();
    let component = inserted["component"].as_u64().unwrap();
    open_link(&mut app, component);
    read(&mut app);
    let mut form = app.link_parameters.take().expect("open");
    assert!(!form.changed);
    row(&mut form, "h").expression = "lift".into();
    row(&mut form, "w").expression = "wide".into();
    // The block's `h` becomes `height` on disk while the dialog is open.
    let mut edited = Session::default();
    edited.execute("doc.open", &json!({"path": part})).unwrap();
    edited.execute("parameters.rename", &json!({"name": "h", "new_name": "height"})).unwrap();
    edited.execute("file.save", &json!({"path": part})).unwrap();

    let error = apply(&mut app, &form).expect_err("refused: the file changed");
    refused(&mut app, form, error);
    assert!(app.link_parameters.as_ref().is_some_and(|f| f.error.is_some()), "the draft is back with the reason at once");
    read(&mut app);
    let mut form = app.link_parameters.take().expect("still open");
    assert!(form.changed && !form.update && form.error.is_some(), "{form:?}");
    let rows: Vec<(&str, &str)> = form.rows.iter().map(|r| (r.name.as_str(), r.expression.as_str())).collect();
    assert_eq!(rows, [("height", ""), ("w", "wide")], "the file as it is now, with what was typed kept");
    assert_eq!(form.gone, ["h"], "the expression typed for the old name is said to go");
    row(&mut form, "height").expression = "lift".into();
    form.update = true;
    apply(&mut app, &form).unwrap();
    let link = app.session.doc.links.get(&component).unwrap().clone();
    assert_eq!(link.parameters.get("height").map(String::as_str), Some("lift"));
    assert!((volume(&app) - 10.0 * 30.0 * 8.0).abs() < 1e-6, "{}", volume(&app));
    let _ = std::fs::remove_dir_all(dir);
}

/// A feature edit that changes what a link is passed when the linked file is gone: the
/// Parameters window says the link kept its bodies, as after a value edit (review of PR #1,
/// round 2).
#[test]
fn a_feature_edit_a_link_cannot_follow_is_said_in_the_parameters_window() {
    let dir = folder("feature-warnings");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut app = SolveApp::new(Session::default(), Default::default());
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 12, "corner": [50, 0, 0], "body_name": "Post"})).unwrap();
    let post = app.session.doc.features[0].id;
    let height = app.session.doc.features[0].param_names[2].clone();
    app.run("component.insert_design", json!({"path": part, "name": "Block", "parameters": {"h": height}})).unwrap();
    std::fs::remove_file(&part).unwrap();
    app.run("timeline.edit", json!({"feature": post, "set": {"height": 7}})).unwrap();
    assert!(app.link_warnings.iter().any(|w| w.contains("Block keeps its bodies")), "{:?}", app.link_warnings);
    let _ = std::fs::remove_dir_all(dir);
}

/// Insert Design's OK whose insert fails (the file went after the dialog read it): the dialog
/// comes back with what was typed and why, and when the design's tab is left while the insert
/// runs, it comes back only once that tab is shown again (review of PR #1, round 4: the draft
/// was lost).
#[test]
fn a_failed_insert_brings_its_draft_back_in_its_own_tab() {
    let dir = folder("insert-fails");
    let part = dir.join("block.sc");
    let ask = |h: &mut crate::scenario::Harness, part: &Path| {
        save_block(part);
        h.step(&json!({"choose_file": part.to_string_lossy()})).unwrap();
        h.app.pick_design_to_insert();
        for _ in 0..2000 {
            if h.app.link_parameters.is_some() {
                break;
            }
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let form = h.app.link_parameters.as_mut().expect("the dialog asks");
        row(form, "w").expression = "lift".into();
        row(form, "h").keep = true;
        // Drawn, so its OK and Cancel are where a click finds them.
        h.frames(2);
    };
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.run("parameters.add", json!({"name": "h", "expression": "8 mm"})).unwrap();
    h.app.run("parameters.add", json!({"name": "lift", "expression": "30 mm"})).unwrap();
    let own_tab = h.app.docs.active_tab();
    let draft = |h: &crate::scenario::Harness| {
        h.app.link_parameters.as_ref().map(|f| (row_of(f, "w").expression.clone(), row_of(f, "h").keep, f.error.is_some(), f.tab))
    };

    ask(&mut h, &part);
    std::fs::remove_file(&part).unwrap();
    h.step(&json!({"click": {"handle": "link_parameters:ok"}})).unwrap();
    h.settle();
    assert!(h.app.session.doc.links.is_empty(), "nothing was inserted");
    assert_eq!(draft(&h), Some(("lift".to_string(), true, true, own_tab)), "the draft is back with the reason");

    // Again, with another design shown while the insert runs. These dialogs stay with their
    // design across a tab switch, with the "Keep dialogs and tools open when switching designs"
    // preference off too (review of PR #1, loop 2): the draft comes back in its own tab whether
    // the insert fails before the switch (it goes with the tab's work) or after it (it waits for
    // the tab).
    assert!(!h.app.preferences.keep_work_on_tab_switch);
    h.step(&json!({"click": {"handle": "link_parameters:cancel"}})).unwrap();
    h.settle();
    ask(&mut h, &part);
    std::fs::remove_file(&part).unwrap();
    h.step(&json!({"click": {"handle": "link_parameters:ok"}})).unwrap();
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    // Whether its insert still runs, has failed and waits for its tab, or came back and went
    // with its tab's work, nothing comes back over this design.
    h.frames(10);
    assert!(h.app.link_parameters.is_none(), "not over another design");
    crate::documents::switch(&mut h.app, 0);
    h.settle();
    assert_eq!(h.app.docs.active_tab(), own_tab);
    assert_eq!(draft(&h), Some(("lift".to_string(), true, true, own_tab)), "back in its own tab");
    let _ = std::fs::remove_dir_all(dir);
}

/// Insert Design open in two designs, the first's insert still under way when the second's OK
/// is pressed: each draft waits for its own insert, so stopping the first brings the first's
/// draft back in its own tab while the second design takes its insert (review of PR #1, round
/// 5: the second OK took the first's place, and the first's draft was lost). Both dialogs stay
/// open across the switches without the keep-dialogs preference.
#[test]
fn inserts_from_two_designs_keep_each_draft() {
    let dir = folder("two-inserts");
    let (one, two) = (dir.join("one.sc"), dir.join("two.sc"));
    save_block(&one);
    save_block(&two);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    assert!(!h.app.preferences.keep_work_on_tab_switch);
    let ask = |h: &mut crate::scenario::Harness, part: &Path, w: &str| {
        h.step(&json!({"choose_file": part.to_string_lossy()})).unwrap();
        h.app.pick_design_to_insert();
        for _ in 0..2000 {
            if h.app.link_parameters.is_some() {
                break;
            }
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let form = h.app.link_parameters.as_mut().expect("the dialog asks");
        row(form, "w").expression = w.into();
        row(form, "h").keep = true;
        h.frames(2);
    };
    let frames_until = |h: &mut crate::scenario::Harness, what: &str, done: &dyn Fn(&SolveApp) -> bool| {
        for _ in 0..5000 {
            if done(&h.app) {
                return;
            }
            h.frames(1);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("{what}");
    };
    h.app.run("parameters.add", json!({"name": "first", "expression": "30 mm"})).unwrap();
    let first = h.app.docs.active_tab();
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    h.app.run("parameters.add", json!({"name": "second", "expression": "40 mm"})).unwrap();
    let second = h.app.docs.active_tab();
    ask(&mut h, &two, "second");
    // To the first design: the second's dialog goes with its tab.
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    assert_eq!(h.app.docs.active_tab(), first);
    assert!(h.app.link_parameters.is_none(), "the second design's dialog is not shown over the first");
    ask(&mut h, &one, "first");
    // Its OK (as the dialog's OK runs it), and the tab is left before a frame can land the
    // insert: done, it waits for its tab.
    let form = h.app.link_parameters.take().unwrap();
    apply(&mut h.app, &form).unwrap();
    crate::documents::switch(&mut h.app, 1);
    let serial = h.app.link_parameters_inserting.iter().find(|(_, f)| f.tab == first).map(|(s, _)| *s).expect("the first draft waits for its insert");
    frames_until(&mut h, "the first insert is in, waiting for its tab", &|app| {
        app.file_load.operations().iter().any(|o| o.serial == serial && o.waiting)
    });
    assert_eq!(h.app.docs.active_tab(), second);
    let form = h.app.link_parameters.as_ref().expect("the second design's dialog is back with its tab");
    assert_eq!((row_of(form, "w").expression.as_str(), form.tab), ("second", second));
    // Its OK the same way: a scenario click would wait for every file operation to end.
    let form = h.app.link_parameters.take().unwrap();
    apply(&mut h.app, &form).unwrap();
    frames_until(&mut h, "the second design takes its insert", &|app| !app.session.doc.links.is_empty());
    let link = h.app.session.doc.links.values().next().unwrap();
    assert_eq!(link.parameters.get("w").map(String::as_str), Some("second"));
    assert_eq!(h.app.link_parameters_inserting.iter().map(|(s, _)| *s).collect::<Vec<_>>(), vec![serial], "only the first insert is left");
    // Stopped (its row's X in the operations panel): its draft comes back in its own design.
    crate::file_load::cancel_waiting(&mut h.app, serial);
    h.frames(2);
    assert!(h.app.link_parameters.is_none(), "not over the second design");
    crate::documents::switch(&mut h.app, 0);
    h.frames(2);
    assert_eq!(h.app.docs.active_tab(), first);
    assert!(h.app.session.doc.links.is_empty(), "nothing was inserted in the first design");
    let form = h.app.link_parameters.as_ref().expect("the first draft is back");
    assert_eq!((row_of(form, "w").expression.as_str(), row_of(form, "h").keep, form.error.is_some(), form.tab), ("first", true, false, first));
    let _ = std::fs::remove_dir_all(dir);
}

fn row_of<'a>(form: &'a Form, name: &str) -> &'a Row {
    form.rows.iter().find(|r| r.name == name).unwrap()
}
