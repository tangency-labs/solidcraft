//! Explicitly choose a linked component's source; all its instances share that source. Extract
//! makes a component that is not linked the source of its own design file.

use std::sync::Arc;

use serde_json::json;

use crate::SolveApp;

pub(crate) fn start_selected(app: &mut SolveApp) {
    let mut components = app.tree.picked_components.clone();
    for selection in &app.session.selection {
        if let solidcraft_engine::Sel::Occurrence { path } = selection
            && let Ok(placement) = app.session.occurrence_placement(path)
        {
            components.push(placement.component);
        }
    }
    components.sort_unstable();
    components.dedup();
    if let [component] = components.as_slice()
        && app.session.doc.links.contains_key(component)
    {
        start(app, *component);
    } else {
        app.set_status(app.tr("ui.relink_select_one").to_string(), true);
    }
}

pub(crate) fn start(app: &mut SolveApp, component: u64) {
    if app.file_picker_busy() || app.file_load_holds() {
        return;
    }
    if !app.session.doc.links.contains_key(&component) {
        app.set_status(app.tr_label("ui.relink_not_linked").to_string(), true);
        return;
    }
    let document = app.session.doc.clone();
    app.pick_open_for(&solidcraft_engine::io::DESIGN_EXTS, move |app, path| {
        let Some(path) = path else { return };
        // A host or programmatic caller can replace/edit a design while a native picker is
        // pending. Never apply that old component id to a different design or newer revision.
        if !Arc::ptr_eq(&document, &app.session.doc) {
            app.set_status(app.tr_label("ui.relink_design_changed").to_string(), true);
            return;
        }
        // Picked in the file dialog: its server is the user's choice.
        let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(&path).into_iter().collect();
        let relinked =
            solidcraft_engine::io::vfs::allowing_hosts(&hosts, || app.run("component.relink", json!({"component": component, "path": path})));
        if let Ok(result) = relinked {
            let warnings: Vec<&str> = result["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
            if warnings.is_empty() {
                app.set_status(app.tr_label("ui.relink_done").to_string(), false);
            } else {
                let text = format!("{} {}", app.tr_label("ui.relink_done"), warnings.join("; "));
                app.session.echo(text.clone());
                app.set_status(text, true);
            }
        }
    });
}

/// Ask where the component's design goes, then move its history there and link it.
pub(crate) fn extract(app: &mut SolveApp, component: u64) {
    if app.file_picker_busy() || app.file_load_holds() {
        return;
    }
    let Some(name) = app.session.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()) else { return };
    let document = app.session.doc.clone();
    let ext = solidcraft_engine::io::DESIGN_EXT;
    app.pick_save(&format!("{name}.{ext}"), &[ext], move |app, path| {
        let Some(mut path) = path else { return };
        if !Arc::ptr_eq(&document, &app.session.doc) {
            app.set_status(app.tr_label("ui.extract_design_changed").to_string(), true);
            return;
        }
        if std::path::Path::new(&path).extension().is_none() {
            path = format!("{path}.{ext}");
        }
        if let Ok(result) = app.run("component.extract", json!({"component": component, "path": path})) {
            report(app, &result, "ui.extract_done");
            let copies: Vec<(u64, String)> = result["copies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| Some((c["component"].as_u64()?, c["name"].as_str().unwrap_or_default().to_string())))
                .collect();
            if !copies.is_empty() {
                app.menu.link_copies = Some(Copies { linked: component, document: app.session.doc.clone(), copies });
            }
        }
    });
}

/// The status line after a command: `done`, then its warnings and notices.
fn report(app: &mut SolveApp, result: &serde_json::Value, done: &str) {
    let notes: Vec<&str> =
        ["warnings", "notices"].iter().flat_map(|k| result[*k].as_array().into_iter().flatten()).filter_map(|w| w.as_str()).collect();
    if notes.is_empty() {
        app.set_status(app.tr_label(done).to_string(), false);
    } else {
        let text = format!("{} {}", app.tr_label(done), notes.join("; "));
        app.session.echo(text.clone());
        app.set_status(text, true);
    }
}

/// A linked design file as it is on disk: its modification time and size (`None`: not there,
/// not allowed, or the platform keeps no times).
pub(crate) type Stamp = Option<(std::time::SystemTime, u64)>;

fn stamp(file: &str) -> Stamp {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let meta = std::fs::metadata(file).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = file;
        None
    }
}

/// The shown design's linked components with their files as they are on disk now: `None` for a
/// file that is missing, or on a network location not allowed, which is never looked at
/// (`component.links` reports it unfound). The web build keeps no times, so it looks at nothing.
fn disk_stamps(app: &mut SolveApp) -> Vec<(u64, Stamp)> {
    let on = app.preferences.update_changed_links;
    stamps_of(&mut app.session, on)
}

/// [`disk_stamps`] for any open design (`on`: the preference `update_changed_links`).
pub(crate) fn stamps_of(session: &mut solidcraft_engine::Session, on: bool) -> Vec<(u64, Stamp)> {
    if cfg!(target_arch = "wasm32") || !on || session.doc.links.is_empty() {
        return Vec::new();
    }
    let Ok(links) = session.execute("component.links", &json!({})) else { return Vec::new() };
    links["links"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| {
            let found = l["found"].as_bool() == Some(true);
            Some((l["component"].as_u64()?, if found { stamp(l["file"].as_str()?) } else { None }))
        })
        .collect()
}

/// Set `component`'s entry in `stamps` (each link's file as the design last read it).
pub(crate) fn record(stamps: &mut Vec<(u64, Stamp)>, component: u64, stamp: Stamp) {
    match stamps.iter_mut().find(|(c, _)| *c == component) {
        Some(entry) => entry.1 = stamp,
        None => stamps.push((component, stamp)),
    }
}

/// The links of the shown design it holds no record of yet (it was just opened, a link was just
/// made) are taken as their files are now.
pub(crate) fn note_unread(app: &mut SolveApp) {
    if app.session.doc.links.keys().all(|c| app.docs.link_stamps.iter().any(|(k, _)| k == c)) {
        return;
    }
    for (c, s) in disk_stamps(app) {
        if !app.docs.link_stamps.iter().any(|(k, _)| *k == c) {
            app.docs.link_stamps.push((c, s));
        }
    }
}

/// The shown design came back to its tab: read again, off the window thread as Update Links
/// does, the linked designs whose files changed since it last read them. Each is a run of its
/// own, so one that cannot be read holds up no other; one that fails is tried again next time.
pub(crate) fn update_changed(app: &mut SolveApp) {
    let now = disk_stamps(app);
    let changed: Vec<u64> =
        now.iter().filter(|(c, s)| s.is_some() && app.docs.link_stamps.iter().any(|(k, was)| k == c && was != s)).map(|(c, _)| *c).collect();
    for (c, s) in now {
        if !app.docs.link_stamps.iter().any(|(k, _)| *k == c) {
            app.docs.link_stamps.push((c, s));
        }
    }
    // More than can wait at once: the rest are still changed when the tab next comes back.
    let room = app.file_load.room();
    for component in changed.into_iter().take(room) {
        let mut request = crate::file_load::Request::new("", crate::file_load::Kind::UpdateLinks);
        request.options.insert("component".into(), json!(component));
        request.design.clone_from(&app.session.path);
        let _ = crate::file_load::begin_request(app, request);
    }
}

/// Update Links read the shown design's link to `component` (`None`: all of them) without an
/// error: those links are now as their files are.
pub(crate) fn read_again(app: &mut SolveApp, component: Option<u64>) {
    for (c, s) in disk_stamps(app).into_iter().filter(|(c, _)| component.is_none_or(|o| o == *c)) {
        record(&mut app.docs.link_stamps, c, s);
    }
}

/// Components Extract to File found to build the same bodies as the one it linked.
#[derive(Clone, Debug)]
pub struct Copies {
    linked: u64,
    document: Arc<solidcraft_engine::doc::Document>,
    copies: Vec<(u64, String)>,
}

/// Link the copies too, or leave them as they are (also the card's buttons).
pub fn link_copies(app: &mut SolveApp, accept: bool) -> bool {
    let Some(c) = app.menu.link_copies.take() else { return false };
    // The design changed since they were found: the ids may name something else now
    if accept && Arc::ptr_eq(&c.document, &app.session.doc) {
        let ids: Vec<u64> = c.copies.iter().map(|(id, _)| *id).collect();
        if let Ok(result) = app.run("component.replace_with_link", json!({"components": ids, "with": c.linked})) {
            report(app, &result, "ui.replace_with_link_done");
        }
    }
    true
}

/// The card offering to link the copies Extract to File found.
pub fn show_copies(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(c) = app.menu.link_copies.clone() else { return };
    let name = app.session.doc.components.iter().find(|x| x.id == c.linked).map(|x| x.name.clone()).unwrap_or_default();
    let (closed, action) = crate::ui_kit::Confirm::new(egui::Id::new("sc_link_copies"), "help", crate::i18n::tr_label("Link Identical Copies"))
        .actions("Link", "Keep", None)
        .show(ctx, true, |ui| {
            let text = crate::i18n::format("ui.link_copies_text", &[("name", &name)]);
            crate::ui_kit::text(ui, &text, crate::ui_kit::Text::Report("copies", false));
            for (id, copy) in &c.copies {
                crate::ui_kit::text(ui, &format!("  • {copy}"), crate::ui_kit::Text::Report(&format!("copy:{id}"), false));
            }
        });
    match action {
        crate::ui_kit::Action::Ok => drop(link_copies(app, true)),
        crate::ui_kit::Action::Cancel => drop(link_copies(app, false)),
        _ if closed => drop(link_copies(app, false)),
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::context_menu::{self, Target};
    use solidcraft_engine::Session;
    use std::path::PathBuf;

    fn fixture(name: &str) -> (SolveApp, u64, u64, PathBuf, PathBuf) {
        let folder = std::env::temp_dir().join(format!("solidcraft-relink-ui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("source.solidcraft");
        let moved = folder.join("moved.solidcraft");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        part.execute("file.save", &json!({"path": source})).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let inserted = app.run("component.insert_design", json!({"path": source})).unwrap();
        let component = inserted["component"].as_u64().unwrap();
        let occurrence = inserted["occurrence"].as_u64().unwrap();
        std::fs::rename(&source, &moved).unwrap();
        (app, component, occurrence, folder, moved)
    }

    fn menu(app: &SolveApp, target: Target) -> Option<context_menu::Item> {
        context_menu::items(app, &target).into_iter().find(|i| i.id == "ui.relinkSource")
    }

    fn answer(app: &mut SolveApp, path: Option<String>) {
        app.services.pick_open = Some(Box::new(move |extensions| {
            assert_eq!(extensions, &["sc", "solidcraft", "solvecraft"], "a design of any release");
            let result = path.clone();
            Box::pin(async move { result })
        }));
    }

    #[test]
    fn relink_menu_uses_the_selected_link_from_components_and_occurrences() {
        let (mut app, component, occurrence, folder, moved) = fixture("menu");
        let second = app.run("occurrence.copy", json!({"occurrence": occurrence, "translate": [90,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
        let occurrences = app.session.doc.occurrences.clone();
        let item = menu(&app, Target::Component { id: component }).unwrap();
        assert!(item.label.contains("All Instances"));
        assert_eq!(menu(&app, Target::Occurrence { path: vec![second] }).unwrap().params, Some(json!({"component": component})));
        assert!(menu(&app, Target::Component { id: 0 }).is_none());
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        let before = app.session.doc.clone();
        context_menu::run_item(&mut app, &item, egui::Pos2::ZERO);
        assert!(app.file_picker_busy());
        assert!(Arc::ptr_eq(&before, &app.session.doc), "choosing is not an edit");
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(!app.file_picker_busy());
        assert!(app.session.doc.links[&component].path.ends_with("moved.solidcraft"));
        assert_eq!(app.session.doc.occurrences, occurrences);
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("all instances") && !error));
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), serde_json::to_value(&*before).unwrap());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_picker_cancel_bad_file_and_changed_document_keep_the_link() {
        let (mut app, component, _, folder, moved) = fixture("cancel");
        let before = serde_json::to_value(&*app.session.doc).unwrap();
        let undo = app.session.undo.len();
        answer(&mut app, None);
        start(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before);
        assert_eq!(app.session.undo.len(), undo);
        answer(&mut app, Some(folder.join("missing.solidcraft").to_string_lossy().into_owned()));
        start(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before);
        assert_eq!(app.session.undo.len(), undo);
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        start(&mut app, component);
        // The ordinary UI blocks edits while picking. Simulate a host replacing/editing the
        // session directly to exercise the continuation's independent stale-target guard.
        app.session.execute("component.rename", &json!({"component": component, "name": "Changed"})).unwrap();
        let changed = serde_json::to_value(&*app.session.doc).unwrap();
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), changed);
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("assembly changed") && *error));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_start_requires_one_linked_selection_and_opens_its_picker() {
        let (mut app, component, occurrence, folder, moved) = fixture("start");
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        app.start("component.relink");
        assert!(!app.file_picker_busy());
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("Select one linked") && *error));
        app.session.selection = vec![solidcraft_engine::Sel::Occurrence { path: vec![occurrence] }];
        app.start("component.relink");
        assert!(app.file_picker_busy());
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.session.doc.links[&component].path.ends_with("moved.solidcraft"));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn extract_menu_asks_where_to_save_and_links_the_component_there() {
        let folder = std::env::temp_dir().join(format!("solidcraft-extract-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let made = app.run("component.create", json!({"name": "Bracket", "grounded": false})).unwrap();
        let (component, occurrence) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
        app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
        let find = |app: &SolveApp, id: u64| context_menu::items(app, &Target::Component { id }).into_iter().find(|i| i.id == "ui.extractComponent");
        assert!(find(&app, 0).is_none());
        // Not offered while the component can't go: here, moved without Capture Position
        app.run("occurrence.move", json!({"occurrence": occurrence, "translate": [5, 0, 0], "capture": false})).unwrap();
        assert!(find(&app, component).is_none());
        app.run("component.revert_position", json!({})).unwrap();
        // In Relink Source's place, right after Properties
        let ids: Vec<String> = context_menu::items(&app, &Target::Component { id: component }).into_iter().map(|i| i.id).collect();
        let properties = ids.iter().position(|i| i == "ui.properties").unwrap();
        assert_eq!(ids.get(properties + 1).map(String::as_str), Some("ui.extractComponent"));
        let item = find(&app, component).unwrap();
        let chosen = folder.join("bracket").to_string_lossy().into_owned();
        app.services.pick_save = Some(Box::new(move |name, extensions| {
            assert_eq!((name, extensions), ("Bracket.sc", &["sc"][..]));
            let result = Some(chosen.clone());
            Box::pin(async move { result })
        }));
        context_menu::run_item(&mut app, &item, egui::Pos2::ZERO);
        assert!(app.file_picker_busy());
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(folder.join("bracket.sc").is_file());
        assert!(app.session.doc.links[&component].path.ends_with("bracket.sc"));
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("its own design") && !error));
        assert!(find(&app, component).is_none(), "a linked component is not extracted again");
        assert!(menu(&app, Target::Component { id: component }).is_some());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn extract_offers_to_link_identical_copies() {
        let folder = std::env::temp_dir().join(format!("solidcraft-extract-copies-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let made = app.run("component.create", json!({"name": "Bracket"})).unwrap();
        let (component, occurrence) = (made["component"].as_u64().unwrap(), made["occurrence"].as_u64().unwrap());
        app.run("solid.box", json!({"length": 10, "width": 20, "height": 30})).unwrap();
        app.run("component.activate", json!({"component": 0})).unwrap();
        let copy = app.run("component.paste_new", json!({"component": component, "occurrence": occurrence, "translate": [50, 0, 0]})).unwrap();
        let (copy, copy_occurrence) = (copy["component"].as_u64().unwrap(), copy["occurrence"].as_u64().unwrap());
        let chosen = folder.join("bracket.solidcraft").to_string_lossy().into_owned();
        app.services.pick_save = Some(Box::new(move |_, _| {
            let result = Some(chosen.clone());
            Box::pin(async move { result })
        }));
        extract(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.session.doc.links.contains_key(&component));
        assert_eq!(app.menu.link_copies.as_ref().map(|c| c.copies.iter().map(|(id, _)| *id).collect::<Vec<_>>()), Some(vec![copy]));
        // Keep leaves the copy alone; Link makes its placement one of the linked component's
        let mut kept = app.menu.link_copies.clone();
        assert!(link_copies(&mut app, false));
        assert!(app.session.doc.components.iter().any(|c| c.id == copy));
        app.menu.link_copies = kept.take();
        assert!(link_copies(&mut app, true));
        assert!(app.session.doc.components.iter().all(|c| c.id != copy));
        assert_eq!(app.session.doc.occurrences.iter().find(|o| o.id == copy_occurrence).map(|o| o.component), Some(component));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn changed_linked_designs_update_the_assembly_when_its_tab_returns() {
        let folder = std::env::temp_dir().join(format!("solidcraft-changed-link-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("plate.solidcraft");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        part.execute("file.save", &json!({"path": file})).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("component.insert_design", json!({"path": file})).unwrap();
        let volume = |app: &SolveApp| {
            app.session.world_state().bodies.iter().map(|b| solidcraft_engine::kernel::measure(&b.body).unwrap().volume).sum::<f64>()
        };
        let ctx = egui::Context::default();
        let settle = |app: &mut SolveApp| {
            let began = std::time::Instant::now();
            while !app.file_load.idle() {
                assert!(began.elapsed() < std::time::Duration::from_secs(120), "Update Links went on");
                crate::file_load::tick(app, &ctx);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        let saved_box = |app: &mut SolveApp| {
            crate::documents::switch(app, 1);
            app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new"})).unwrap();
            app.run("file.save", json!({})).unwrap();
            crate::documents::switch(app, 0);
        };
        assert!((volume(&app) - 6000.0).abs() < 1e-6);
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": file})).unwrap();
        crate::documents::open_tab(&mut app, opened).unwrap();
        assert_eq!(app.docs.active, 1);
        // Back without saving: nothing to take up
        crate::documents::switch(&mut app, 0);
        assert!(app.file_load.idle());
        assert!((volume(&app) - 6000.0).abs() < 1e-6);
        // Changed and saved there: coming back updates the link, off the window thread
        saved_box(&mut app);
        assert!(app.file_load.busy(), "the update runs as Update Links does");
        settle(&mut app);
        assert!((volume(&app) - 7000.0).abs() < 1e-6, "{}", volume(&app));
        assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("Update Links done"));
        // Changed outside the app (another window, a checkout) while the tab was away: the same.
        // The part's own tab now refuses to save over that change, so it stays out of the rest
        let saved_elsewhere = |app: &mut SolveApp| {
            crate::documents::switch(app, 1);
            let mut elsewhere = Session::default();
            elsewhere.execute("doc.open", &json!({"path": file})).unwrap();
            elsewhere.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "operation": "new"})).unwrap();
            elsewhere.execute("file.save", &json!({})).unwrap();
            crate::documents::switch(app, 0);
        };
        saved_elsewhere(&mut app);
        settle(&mut app);
        assert!((volume(&app) - 8000.0).abs() < 1e-6, "{}", volume(&app));
        // Turned off, the link waits for Update Links as before
        app.preferences.update_changed_links = false;
        saved_elsewhere(&mut app);
        assert!(app.file_load.idle());
        assert!((volume(&app) - 8000.0).abs() < 1e-6, "{}", volume(&app));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// An assembly saved in `folder` linking a 10x20x30 plate and a 10 mm cube, with a blank
    /// second tab to step away to; the helpers save a part, settle the background work and
    /// measure the assembly.
    fn two_links(name: &str) -> (SolveApp, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let folder = std::env::temp_dir().join(format!("solidcraft-two-links-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let (plate, cube) = (folder.join("plate.solidcraft"), folder.join("cube.solidcraft"));
        save_part(&plate, [10.0, 20.0, 30.0], false);
        save_part(&cube, [10.0, 10.0, 10.0], false);
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("component.insert_design", json!({"path": plate})).unwrap();
        app.run("component.insert_design", json!({"path": cube, "translate": [100, 0, 0]})).unwrap();
        app.run("file.save", json!({"path": folder.join("case.solidcraft")})).unwrap();
        crate::documents::open_tab(&mut app, Session::default()).unwrap();
        crate::documents::switch(&mut app, 0);
        (app, folder, plate, cube)
    }

    /// A box design at `path`; `extra` adds a 5x5x10 box on top (250 mm³).
    fn save_part(path: &std::path::Path, size: [f64; 3], extra: bool) {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": size[0], "width": size[1], "height": size[2]})).unwrap();
        if extra {
            s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 10, "corner": [0, 0, size[2]], "operation": "join"})).unwrap();
        }
        s.execute("file.save", &json!({"path": path, "overwrite": true})).unwrap();
    }

    fn settle(app: &mut SolveApp) {
        let ctx = egui::Context::default();
        let began = std::time::Instant::now();
        while !app.file_load.idle() {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "Update Links went on");
            crate::file_load::tick(app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn volume(app: &SolveApp) -> f64 {
        app.session.world_state().bodies.iter().map(|b| solidcraft_engine::kernel::measure(&b.body).unwrap().volume).sum()
    }

    /// Away to the blank tab and back to the assembly.
    fn come_back(app: &mut SolveApp) {
        crate::documents::switch(app, 1);
        crate::documents::switch(app, 0);
        settle(app);
    }

    #[test]
    fn a_link_that_cannot_be_read_holds_up_no_other_and_is_read_again_next_time() {
        let (mut app, folder, plate, cube) = two_links("unreadable");
        assert!((volume(&app) - 7000.0).abs() < 1e-6);
        std::fs::write(&plate, b"not a design").unwrap();
        save_part(&cube, [10.0, 10.0, 10.0], true);
        come_back(&mut app);
        assert!((volume(&app) - 7250.0).abs() < 1e-6, "the cube is read, the plate keeps its bodies: {}", volume(&app));
        // Readable again (and changed): the next return takes it up
        save_part(&plate, [10.0, 20.0, 30.0], true);
        come_back(&mut app);
        assert!((volume(&app) - 7500.0).abs() < 1e-6, "{}", volume(&app));
        // Nothing changed since: nothing is read
        crate::documents::switch(&mut app, 1);
        crate::documents::switch(&mut app, 0);
        assert!(app.file_load.idle());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_change_made_while_the_assembly_is_shown_is_read_when_it_next_comes_back() {
        let (mut app, folder, plate, _) = two_links("shown");
        // Changed elsewhere while the assembly is the tab in front
        save_part(&plate, [10.0, 20.0, 30.0], true);
        come_back(&mut app);
        assert!((volume(&app) - 7250.0).abs() < 1e-6, "{}", volume(&app));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn closing_tabs_together_reads_nothing_on_the_way() {
        let (mut app, folder, plate, _) = two_links("batch");
        crate::documents::switch(&mut app, 1);
        save_part(&plate, [10.0, 20.0, 30.0], true);
        // Close All from the blank tab: the assembly is shown on the way, and closed
        crate::documents::close_many(&mut app, vec![0, 1]);
        assert!(app.file_load.idle(), "no Update Links for a tab that is closing");
        assert_eq!(app.docs.count(), 1);
        assert!(app.status.as_ref().is_none_or(|(_, _, error)| !error), "{:?}", app.status);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn saving_a_tab_from_its_save_changes_prompt_is_not_held_up_by_its_links() {
        let (mut app, folder, plate, _) = two_links("prompt");
        app.run("solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [0, 300, 0], "operation": "new"})).unwrap();
        crate::documents::switch(&mut app, 1);
        save_part(&plate, [10.0, 20.0, 30.0], true);
        // Closing the unsaved assembly from the other tab asks; Save shows it, saves and closes it
        crate::documents::close(&mut app, 0, false);
        assert_eq!(app.docs.closing, Some(0));
        crate::documents::answer_prompt(&mut app, crate::documents::Answer::Save);
        assert!(app.file_load.idle(), "no link read before the save");
        crate::save_job::tick(&mut app, true);
        crate::documents::saved_for_close(&mut app);
        assert_eq!(app.docs.count(), 1, "saved and closed: {:?}", app.status);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_design_opened_in_place_of_the_blank_one_reads_what_changes_while_it_is_shown() {
        let (app, folder, plate, _) = two_links("adopted");
        drop(app);
        let mut app = SolveApp::new(Session::default(), Default::default());
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": folder.join("case.solidcraft")})).unwrap();
        crate::documents::adopt(&mut app, opened);
        assert_eq!(app.docs.count(), 1);
        save_part(&plate, [10.0, 20.0, 30.0], true);
        crate::documents::open_tab(&mut app, Session::default()).unwrap();
        crate::documents::switch(&mut app, 0);
        settle(&mut app);
        assert!((volume(&app) - 7250.0).abs() < 1e-6, "{}", volume(&app));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn cancelling_a_close_all_reads_what_the_tab_left_in_front_missed() {
        let (assembly, folder, plate, _) = two_links("cancelled");
        drop(assembly);
        // An unsaved design, a blank tab (shown) and the assembly after it
        let mut unsaved = Session::default();
        unsaved.execute("solid.box", &json!({"length": 1, "width": 1, "height": 1})).unwrap();
        let mut app = SolveApp::new(unsaved, Default::default());
        crate::documents::open_tab(&mut app, Session::default()).unwrap();
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": folder.join("case.solidcraft")})).unwrap();
        crate::documents::open_tab(&mut app, opened).unwrap();
        settle(&mut app);
        crate::documents::switch(&mut app, 1);
        save_part(&plate, [10.0, 20.0, 30.0], true);
        // Closing the first two shows the assembly on the way, then asks about the unsaved one
        crate::documents::close_many(&mut app, vec![0, 1]);
        assert_eq!(app.docs.closing, Some(0));
        assert!(app.file_load.idle());
        crate::documents::answer_prompt(&mut app, crate::documents::Answer::Cancel);
        settle(&mut app);
        assert!(app.session.path.as_deref().is_some_and(|p| p.ends_with("case.solidcraft")));
        assert!((volume(&app) - 7250.0).abs() < 1e-6, "{}", volume(&app));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_link_whose_file_comes_back_is_read() {
        let (app, folder, plate, _) = two_links("returns");
        drop(app);
        std::fs::remove_file(&plate).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": folder.join("case.solidcraft")})).unwrap();
        crate::documents::adopt(&mut app, opened);
        crate::documents::open_tab(&mut app, Session::default()).unwrap();
        crate::documents::switch(&mut app, 0);
        assert!(app.file_load.idle(), "a missing file is not read");
        save_part(&plate, [10.0, 20.0, 30.0], true);
        come_back(&mut app);
        assert!((volume(&app) - 7250.0).abs() < 1e-6, "{}", volume(&app));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn reads_waiting_for_a_design_no_longer_shown_are_dropped_and_done_on_its_return() {
        let (mut app, folder, plate, cube) = two_links("dropped");
        save_part(&plate, [10.0, 20.0, 30.0], true);
        save_part(&cube, [10.0, 10.0, 10.0], true);
        crate::documents::switch(&mut app, 1);
        crate::documents::switch(&mut app, 0);
        assert!(app.file_load.busy());
        // Another design is shown before the two reads are done (the control channel can)
        crate::documents::switch(&mut app, 1);
        settle(&mut app);
        assert!(app.session.doc.links.is_empty() && app.session.path.is_none(), "the blank design is left alone");
        assert!(!app.status.as_ref().is_some_and(|(s, _, _)| s.contains("not linked")), "{:?}", app.status);
        crate::documents::switch(&mut app, 0);
        settle(&mut app);
        assert!((volume(&app) - 7500.0).abs() < 1e-6, "both are read on the return: {}", volume(&app));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_displays_the_warning_when_changed_geometry_has_joints() {
        let (mut app, component, occurrence, folder, moved) = fixture("warning");
        app.run(
            "joint.create",
            json!({"name": "Seat", "type": "rigid", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": occurrence, "point": [0,0,0]}}),
        )
        .unwrap();
        let mut changed = Session::default();
        changed.execute("solid.box", &json!({"length": 10, "width": 20, "height": 45})).unwrap();
        changed.execute("file.save", &json!({"path": moved})).unwrap();
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        start(&mut app, component);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("check this component's joints") && *error));
        assert!(app.session.log.iter().any(|s| s.contains("check this component's joints")));
        let _ = std::fs::remove_dir_all(folder);
    }
}
