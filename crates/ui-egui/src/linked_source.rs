//! Explicitly choose a linked component's source, for all its placements or for one of them.
//! Extract makes a component that is not linked the source of its own design file.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::SolveApp;

/// Relink Source for the selection, which must name exactly one linked component, through
/// its browser rows or placements.
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
        start(app, *component, None);
    } else {
        app.set_status(app.tr("ui.relink_select_one").to_string(), true);
    }
}

/// Ask for the design a linked component shows instead: in all its placements, or with
/// `occurrence` in that one only.
pub(crate) fn start(app: &mut SolveApp, component: u64, occurrence: Option<u64>) {
    start_with(app, component, occurrence, false);
}

/// `start`; with `sync` the bodies take the new source's names (#16711).
pub(crate) fn start_with(app: &mut SolveApp, component: u64, occurrence: Option<u64>, sync: bool) {
    if app.file_picker_busy() || app.file_load_holds() {
        return;
    }
    if !app.session.doc.links.contains_key(&component) {
        app.set_status(app.tr_label("ui.relink_not_linked").to_string(), true);
        return;
    }
    let document = app.session.doc.clone();
    // A design of any release, or a plain JSON design: what Insert Design takes.
    let mut extensions = solidcraft_engine::io::DESIGN_EXTS.to_vec();
    extensions.push("json");
    app.pick_open_for(&extensions, move |app, path| {
        let Some(path) = path else { return };
        // A host or programmatic caller can replace/edit a design while a native picker is
        // pending. Never apply that old component id to a different design or newer revision.
        if !Arc::ptr_eq(&document, &app.session.doc) {
            app.set_status(app.tr_label("ui.relink_design_changed").to_string(), true);
            return;
        }
        // Picked in the file dialog: its server is the user's choice.
        let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(&path).into_iter().collect();
        let mut params = json!({"component": component, "path": path});
        if let Some(occurrence) = occurrence {
            params["occurrence"] = json!(occurrence);
        }
        if sync {
            params["names"] = json!("sync");
        }
        let relinked = solidcraft_engine::io::vfs::allowing_hosts(&hosts, || app.run("component.relink", params));
        if let Ok(result) = relinked {
            let done = if occurrence.is_some() { app.tr_label("ui.relink_one_done") } else { app.tr_label("ui.relink_done") }.to_string();
            let warnings: Vec<&str> = result["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
            if warnings.is_empty() {
                app.set_status(done, false);
            } else {
                let text = format!("{done} {}", warnings.join("; "));
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
fn report(app: &mut SolveApp, result: &Value, done: &str) {
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
    if !on {
        say_changed(app);
    }
    stamps_of(&mut app.session, on)
}

/// Update active (#6892, under Parts and assemblies): nothing is read by itself, so the status line
/// names the linked parts whose files changed since the design read them; Update Links brings
/// them in. Live reads them instead ([`update_changed`]).
fn say_changed(app: &mut SolveApp) {
    if !crate::prefs::parts_and_assemblies(&app.preferences) || app.session.doc.links.is_empty() {
        return;
    }
    let changed = changed_since_read(&mut app.session);
    if changed.is_empty() {
        return;
    }
    let names: Vec<&str> = app.session.doc.components.iter().filter(|c| changed.contains(&c.id)).map(|c| c.name.as_str()).collect();
    let text = solidcraft_ui_text::format("linked.changed_since_read", &[("names", &names.join(", "))]);
    app.set_status(text, false);
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
            // A part inside another design changes when that design's file does.
            Some((l["component"].as_u64()?, if found { stamp(l["inside"].as_str().or(l["file"].as_str())?) } else { None }))
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

/// The shown design's linked components whose files are no longer what the design last read
/// (`component.links` `changed`), as when one changed while the design was closed (#9739).
fn changed_since_read(session: &mut solidcraft_engine::Session) -> Vec<u64> {
    let Ok(links) = session.execute("component.links", &json!({})) else { return Vec::new() };
    links["links"].as_array().into_iter().flatten().filter(|l| l["changed"].as_bool() == Some(true)).filter_map(|l| l["component"].as_u64()).collect()
}

/// The shown design's linked components with the files they read, as the engine resolves them
/// (`component.links` `file`).
fn read_files(session: &mut solidcraft_engine::Session) -> Vec<(u64, PathBuf)> {
    let Ok(links) = session.execute("component.links", &json!({})) else { return Vec::new() };
    // A part inside another design is read from that design's file.
    let file = |l: &Value| l["inside"].as_str().or(l["file"].as_str()).map(PathBuf::from);
    links["links"].as_array().into_iter().flatten().filter_map(|l| Some((l["component"].as_u64()?, file(l)?))).collect()
}

/// The links of the shown design it holds no record of yet (it was just opened, a link was just
/// made) are taken as their files are now, except a file that changed since the design last read
/// it (#9739): that one counts as not read yet, so [`update_changed`] reads it.
pub(crate) fn note_unread(app: &mut SolveApp) {
    if app.session.doc.links.keys().all(|c| app.docs.link_stamps.iter().any(|(k, _)| k == c)) {
        return;
    }
    let now = disk_stamps(app);
    let stale = if now.is_empty() { Vec::new() } else { changed_since_read(&mut app.session) };
    for (c, s) in now {
        if !app.docs.link_stamps.iter().any(|(k, _)| *k == c) {
            app.docs.link_stamps.push((c, if stale.contains(&c) { None } else { s }));
        }
    }
}

/// The shown design came back to its tab, or was just opened: read again, off the window thread
/// as Update Links does, the linked designs whose files changed since it last read them. All in
/// one run, so they come in as one undo step (#9743); one that cannot be read holds up no other,
/// keeps its bodies and is tried again next time.
pub(crate) fn update_changed(app: &mut SolveApp) {
    let now = disk_stamps(app);
    // Links met here for the first time: those whose files changed since the design last read
    // them (#9739) are read again too.
    let first: Vec<u64> = now.iter().map(|(c, _)| *c).filter(|c| !app.docs.link_stamps.iter().any(|(k, _)| k == c)).collect();
    let stale = if first.is_empty() { Vec::new() } else { changed_since_read(&mut app.session) };
    // Only links inside the design's own folder, or in a folder the user allowed linked files from
    // (`linked_file_folders`), are read unasked, when the design is met or when their files change
    // later: a design received from someone else names what it likes as "changed since last read",
    // and one reaching out of its folder would copy another design of the user's into it. For the
    // others the user is asked whether to load them (`link_folders_prompt`); after Not now they stay
    // out of date until their folder is allowed, and Update Links reads them anyway.
    let design = app.session.path.clone();
    let outside: Vec<u64> = now
        .iter()
        .map(|(c, _)| *c)
        .filter(|c| !app.session.doc.links.get(c).is_some_and(|l| inside_design_folder(design.as_deref(), &l.path)))
        .collect();
    let files = if outside.is_empty() { Vec::new() } else { read_files(&mut app.session) };
    let mut allowed: Vec<u64> = Vec::new();
    let mut ask: Vec<(u64, String)> = Vec::new();
    for &c in &outside {
        // The folder of the file the engine reads for the link, as it resolves it.
        let Some(folder) = files.iter().find(|(k, _)| *k == c).and_then(|(_, f)| lexical(f).parent().map(Path::to_path_buf)) else { continue };
        // A design opened by a program reads none of them unasked, allowed folders included: it
        // may work within narrower folders than the user's, and it asks nothing.
        if app.docs.quiet_open {
            continue;
        }
        if app.preferences.linked_file_folders.iter().any(|f| folder_within(&folder, Path::new(f))) {
            allowed.push(c);
            continue;
        }
        // Not now (or a question still open) holds the link back until its folder is allowed.
        if crate::link_folders_prompt::held(app, c) {
            continue;
        }
        let changed_later = now.iter().any(|(k, s)| *k == c && s.is_some() && app.docs.link_stamps.iter().any(|(x, was)| *x == c && was != s));
        if (first.contains(&c) && stale.contains(&c)) || (changed_later && design.is_some()) {
            ask.push((c, folder.to_string_lossy().into_owned()));
        } else if changed_later {
            // An unsaved design has no folder: its links follow their files as before.
            allowed.push(c);
        }
    }
    let may_read = |c: &u64| !outside.contains(c) || allowed.contains(c);
    let changed: Vec<u64> = now
        .iter()
        .filter(|(c, s)| {
            s.is_some() && may_read(c) && (app.docs.link_stamps.iter().any(|(k, was)| k == c && was != s) || (first.contains(c) && stale.contains(c)))
        })
        .map(|(c, _)| *c)
        .collect();
    // Only a file that is there to read is asked about (a missing one, or one on a server not
    // allowed, has no stamp).
    ask.retain(|(c, _)| now.iter().any(|(k, s)| k == c && s.is_some()));
    for (c, s) in now {
        if !app.docs.link_stamps.iter().any(|(k, _)| *k == c) {
            app.docs.link_stamps.push((c, if changed.contains(&c) { None } else { s }));
        }
    }
    crate::link_folders_prompt::ask(app, ask);
    // No room to wait: they are still changed when the tab next comes back.
    if changed.is_empty() || app.file_load.room() == 0 {
        return;
    }
    read(app, changed);
}

/// Read `components`, links of the shown design, again off the window thread (Update Links for
/// those alone).
pub(crate) fn read(app: &mut SolveApp, components: Vec<u64>) {
    let mut request = crate::file_load::Request::new("", crate::file_load::Kind::UpdateLinks);
    request.options.insert("components".into(), json!(components));
    // A linked design saved without the bodies of its own links reads them no further than this
    // design's folder and its own: the user allowed those two, not what the design names.
    request.options.insert("linked_bodies".into(), json!("folder"));
    request.design.clone_from(&app.session.path);
    let _ = crate::file_load::begin_request(app, request);
}

/// `p` with `.` and `..` taken out, judged from the text alone.
fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// Whether `link`, as the design saved at `design` keeps it, names a file in the design's own
/// folder or below it, judged from the link's text (`..` is followed, the link itself is not
/// asked of the disk). The folder counts both as the design's path names it and as the disk
/// resolves it: a link is kept with its symlinks resolved, so a design opened through a symlink
/// (`/var` → `/private/var` on macOS, a linked home folder) still finds its own links inside.
/// An unsaved design has no folder.
fn inside_design_folder(design: Option<&str>, link: &str) -> bool {
    use std::path::{Component, Path, PathBuf};
    let Some(dir) = design.and_then(|d| Path::new(d).parent()).filter(|d| !d.as_os_str().is_empty()) else { return false };
    let lexical = |p: &Path| {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                c => out.push(c),
            }
        }
        out
    };
    let Ok(dir) = std::path::absolute(dir) else { return false };
    let dir = lexical(&dir);
    let target = lexical(&dir.join(link));
    target.starts_with(&dir) || std::fs::canonicalize(&dir).is_ok_and(|real| target.starts_with(real))
}

/// Whether `folder` is `allowed` or below it. Windows paths compare without case, as Windows does,
/// and a drive path matches its `\\?\` spelling (`\\?\C:\parts` is `C:\parts`), the form
/// `canonicalize` and some folder pickers return.
fn folder_within(folder: &Path, allowed: &Path) -> bool {
    // Both sides are normalised after the `\\?\` comes off: inside a verbatim path only `\`
    // separates names, outside it `/` does too, so a `..` written with `/` resolves here.
    let (folder, allowed) = (lexical(&drive_path(folder)), lexical(&drive_path(allowed)));
    if allowed.as_os_str().is_empty() || !allowed.is_absolute() {
        return false;
    }
    if cfg!(windows) {
        let (mut f, mut a) = (folder.components(), allowed.components());
        loop {
            match (f.next(), a.next()) {
                (_, None) => return true,
                (Some(x), Some(y)) if x.as_os_str().eq_ignore_ascii_case(y.as_os_str()) => {}
                _ => return false,
            }
        }
    }
    folder.starts_with(&allowed)
}

/// `p` without the `\\?\` of a verbatim drive path (`\\?\C:\parts` is `C:\parts`). Other verbatim
/// paths (`\\?\UNC\…`, `\\?\Volume{…}\…`) have no drive spelling and stay as they are.
fn drive_path(p: &Path) -> PathBuf {
    let text = p.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if matches!(rest.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic()) => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// Update Links (`options` as it was asked) read the shown design's links again: those it read
/// without an error (`result`'s `failed` names the others) are now as their files are.
pub(crate) fn read_again(app: &mut SolveApp, options: &serde_json::Map<String, Value>, result: &Value) {
    let asked: Option<Vec<u64>> = match (options.get("component"), options.get("components")) {
        (Some(c), _) => Some(c.as_u64().into_iter().collect()),
        (None, Some(list)) => Some(list.as_array().into_iter().flatten().filter_map(Value::as_u64).collect()),
        (None, None) => None,
    };
    let failed: Vec<u64> = result["failed"].as_array().into_iter().flatten().filter_map(|f| f["component"].as_u64()).collect();
    for (c, s) in disk_stamps(app).into_iter().filter(|(c, _)| asked.as_ref().is_none_or(|a| a.contains(c)) && !failed.contains(c)) {
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
    let (closed, action) =
        crate::ui_kit::Confirm::new(egui::Id::unique("sc_link_copies"), "help", solidcraft_ui_text::tr_label("Link Identical Copies"))
            .actions("Link", "Keep", None)
            .show(ctx, true, |ui| {
                let text = solidcraft_ui_text::format("ui.link_copies_text", &[("name", &name)]);
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

    fn menu(app: &SolveApp, target: Target) -> Option<crate::ui_kit::Item> {
        context_menu::entries(app, &target).into_iter().find(|i| i.id == "ui.relinkSource")
    }

    fn answer(app: &mut SolveApp, path: Option<String>) {
        app.services.pick_open = Some(Box::new(move |extensions| {
            assert_eq!(extensions, &["sc", "solidcraft", "json"], "a design of any release, or a plain JSON design");
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

    /// #12227: a placement's menu relinks that placement alone when the component has others.
    #[test]
    fn relink_menu_relinks_one_placement_of_several() {
        let (mut app, component, occurrence, folder, moved) = fixture("one");
        let rows = |app: &SolveApp, target: &Target| -> Vec<String> { context_menu::entries(app, target).into_iter().map(|i| i.id).collect() };
        assert!(
            !rows(&app, &Target::Occurrence { path: vec![occurrence] }).contains(&"ui.relinkPlacement".to_string()),
            "one placement: only the row for all"
        );
        let second = app.run("occurrence.copy", json!({"occurrence": occurrence, "translate": [90,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
        assert!(!rows(&app, &Target::Component { id: component }).contains(&"ui.relinkPlacement".to_string()), "no placement to relink");
        let target = Target::Occurrence { path: vec![second] };
        let ids = rows(&app, &target);
        let at = ids.iter().position(|id| id == "ui.relinkPlacement").expect("a row for this placement");
        assert_eq!(ids[at + 1], "ui.relinkSource", "right above the row for all instances");
        let item = context_menu::entries(&app, &target).into_iter().find(|i| i.id == "ui.relinkPlacement").unwrap();
        assert_eq!(item.params, Some(json!({"component": component, "occurrence": second})));
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        context_menu::run_item(&mut app, &item, egui::Pos2::ZERO);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        let shows = |app: &SolveApp, o: u64| app.session.doc.occurrences.iter().find(|x| x.id == o).unwrap().component;
        assert_eq!(shows(&app, occurrence), component);
        assert_ne!(shows(&app, second), component);
        assert!(app.session.doc.links[&shows(&app, second)].path.ends_with("moved.solidcraft"));
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("this instance") && !error), "{:?}", app.status);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn relink_picker_cancel_bad_file_and_changed_document_keep_the_link() {
        let (mut app, component, _, folder, moved) = fixture("cancel");
        let before = serde_json::to_value(&*app.session.doc).unwrap();
        let undo = app.session.undo.len();
        answer(&mut app, None);
        start(&mut app, component, None);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before);
        assert_eq!(app.session.undo.len(), undo);
        answer(&mut app, Some(folder.join("missing.solidcraft").to_string_lossy().into_owned()));
        start(&mut app, component, None);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(serde_json::to_value(&*app.session.doc).unwrap(), before);
        assert_eq!(app.session.undo.len(), undo);
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| *error));
        answer(&mut app, Some(moved.to_string_lossy().into_owned()));
        start(&mut app, component, None);
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
    fn relink_picks_a_plain_json_design_as_insert_design_does() {
        let (mut app, component, _, folder, _) = fixture("json");
        let plain = folder.join("plain.json");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length": 10, "width": 20, "height": 45})).unwrap();
        part.execute("file.save", &json!({"path": plain})).unwrap();
        answer(&mut app, Some(plain.to_string_lossy().into_owned()));
        start(&mut app, component, None);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.session.doc.links[&component].path.ends_with("plain.json"));
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
        let find =
            |app: &SolveApp, id: u64| context_menu::entries(app, &Target::Component { id }).into_iter().find(|i| i.id == "ui.extractComponent");
        assert!(find(&app, 0).is_none());
        // Not offered while the component can't go: here, moved without Capture Position
        app.run("occurrence.move", json!({"occurrence": occurrence, "translate": [5, 0, 0], "capture": false})).unwrap();
        assert!(find(&app, component).is_none());
        app.run("component.revert_position", json!({})).unwrap();
        // In Relink Source's place in the Component flyout, right below New Component
        let rows = context_menu::items(&app, &Target::Component { id: component });
        let ids: Vec<&str> = rows.iter().find(|i| i.label == "Component").unwrap().children.iter().map(|i| i.id.as_str()).collect();
        let create = ids.iter().position(|i| *i == "component.create").unwrap();
        assert_eq!(ids.get(create + 1), Some(&"ui.extractComponent"));
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
            crate::open_meshing::session_world(&app.session)
                .bodies
                .iter()
                .map(|b| solidcraft_engine::kernel::measure(&b.body).unwrap().volume)
                .sum::<f64>()
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
        // Update active under Parts and assemblies (#6892): the status line names what changed,
        // and still nothing is read until Update Links.
        app.preferences.environment = crate::prefs::ENVIRONMENT_PARTS_AND_ASSEMBLIES.into();
        saved_elsewhere(&mut app);
        assert!(app.file_load.idle());
        let said = app.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(said.starts_with("Changed since this assembly read them:") && said.ends_with("Update Links brings them in."), "{said}");
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
        crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| solidcraft_engine::kernel::measure(&b.body).unwrap().volume).sum()
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

    /// #9743: the links that changed while the tab was away come back as one undo step.
    #[test]
    fn links_to_a_changed_design_come_back_together_and_one_undo_takes_them_back() {
        let (mut app, folder, plate, cube) = two_links("one-step");
        app.run("component.insert_design", json!({"path": plate, "translate": [0, 100, 0]})).unwrap();
        app.run("file.save", json!({})).unwrap();
        assert!((volume(&app) - 13000.0).abs() < 1e-6, "{}", volume(&app));
        crate::documents::switch(&mut app, 1);
        save_part(&plate, [10.0, 20.0, 30.0], true);
        save_part(&cube, [10.0, 10.0, 10.0], true);
        crate::documents::switch(&mut app, 0);
        settle(&mut app);
        assert!((volume(&app) - 13750.0).abs() < 1e-6, "{}", volume(&app));
        assert_eq!(app.run("edit.undo", json!({})).unwrap()["undone"], "Update Links");
        assert!((volume(&app) - 13000.0).abs() < 1e-6, "every copy of the part goes back together: {}", volume(&app));
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

    /// #9739: a linked design changed while the assembly was closed is read when the assembly
    /// opens, in place of the blank design or in a tab of its own. Nothing changed, nothing is.
    #[test]
    fn a_design_changed_while_the_assembly_was_closed_is_read_when_it_opens() {
        let (app, folder, plate, _) = two_links("closed");
        drop(app);
        let case = folder.join("case.solidcraft");
        let open = |in_tab: bool| {
            let mut app = SolveApp::new(Session::default(), Default::default());
            if in_tab {
                app.run("solid.box", json!({"length": 1, "width": 1, "height": 1})).unwrap();
            }
            let mut opened = Session::default();
            opened.execute("doc.open", &json!({"path": case})).unwrap();
            crate::documents::adopt(&mut app, opened);
            assert_eq!(app.docs.count(), if in_tab { 2 } else { 1 });
            app
        };
        let app = open(false);
        assert!(app.file_load.idle(), "nothing changed: nothing to read");
        assert!((volume(&app) - 7000.0).abs() < 1e-6, "{}", volume(&app));
        save_part(&plate, [10.0, 20.0, 30.0], true);
        for in_tab in [false, true] {
            let mut app = open(in_tab);
            settle(&mut app);
            assert!((volume(&app) - 7250.0).abs() < 1e-6, "in a tab of its own: {in_tab}: {}", volume(&app));
        }
        let _ = std::fs::remove_dir_all(folder);
    }

    /// A design met for the first time reads unasked only the changed links inside its own folder:
    /// one that reaches out of it (`..`, an absolute path elsewhere) stays out of date for Update
    /// Links, so a received design can't copy another of the user's designs into itself.
    #[test]
    fn opening_a_design_reads_unasked_only_links_inside_its_folder() {
        let folder = std::env::temp_dir().join(format!("solidcraft-links-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("received")).unwrap();
        let outside = folder.join("housing.solidcraft");
        save_part(&outside, [10.0, 20.0, 30.0], false);
        let case = folder.join("received").join("case.solidcraft");
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("component.insert_design", json!({"path": outside})).unwrap();
        app.run("file.save", json!({"path": case})).unwrap();
        drop(app);
        save_part(&outside, [10.0, 20.0, 30.0], true);
        let mut app = SolveApp::new(Session::default(), Default::default());
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": case})).unwrap();
        crate::documents::adopt(&mut app, opened);
        assert!(app.file_load.idle(), "a link out of the design's folder is not read unasked");
        assert!((volume(&app) - 6000.0).abs() < 1e-6, "{}", volume(&app));
        let links = app.session.execute("component.links", &json!({})).unwrap();
        assert_eq!(links["links"][0]["changed"], true, "it stays marked out of date");
        let base = folder.join("received").join("case.solidcraft").to_string_lossy().into_owned();
        assert!(inside_design_folder(Some(&base), "part.sc") && inside_design_folder(Some(&base), "sub/../parts/a.sc"));
        assert!(!inside_design_folder(Some(&base), "../housing.solidcraft") && !inside_design_folder(Some(&base), "parts/../../x.sc"));
        assert!(!inside_design_folder(Some(&base), &outside.to_string_lossy()) && !inside_design_folder(None, "part.sc"));
        // A design opened through a symlinked folder: its links, kept with symlinks resolved, are
        // still inside; one out of the real folder is still out.
        #[cfg(unix)]
        {
            let alias = folder.join("alias");
            std::os::unix::fs::symlink(folder.join("received"), &alias).unwrap();
            let through = alias.join("case.solidcraft").to_string_lossy().into_owned();
            let real = folder.join("received").canonicalize().unwrap();
            assert!(inside_design_folder(Some(&through), &real.join("part.sc").to_string_lossy()));
            assert!(!inside_design_folder(Some(&through), &real.join("../housing.solidcraft").to_string_lossy()));
        }
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Relative links are judged by the file the reader opens for them, as `component.links`
    /// reports it: these two are outside the design's folder, so neither is read as it opens.
    #[cfg(unix)]
    #[test]
    fn resolves_like_the_reader_for_relative_links() {
        let top = std::env::temp_dir().join(format!("solidcraft-links-reader-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let real = top.join("a").join("r");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        std::fs::create_dir_all(top.join("a").join("view")).unwrap();
        let up = top.join("x.solidcraft");
        let beside = top.join("a").join("view").join("x.solidcraft");
        save_part(&up, [10.0, 20.0, 30.0], false);
        save_part(&beside, [10.0, 20.0, 30.0], false);
        let case = real.join("case.solidcraft");
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("component.insert_design", json!({"path": up})).unwrap();
        app.run("component.insert_design", json!({"path": beside})).unwrap();
        app.run("file.save", json!({"path": case})).unwrap();
        drop(app);
        // How the design keeps the links.
        let (saved, _) = solidcraft_engine::io::read_design_file(&case.to_string_lossy(), None::<fn(solidcraft_engine::io::DesignPreview)>).unwrap();
        let mut kept: Vec<&str> = saved.links.values().map(|l| l.path.as_str()).collect();
        kept.sort_unstable();
        assert_eq!(kept, ["../../x.solidcraft", "../view/x.solidcraft"], "how the design keeps the links");
        save_part(&up, [10.0, 20.0, 30.0], true);
        save_part(&beside, [10.0, 20.0, 30.0], true);
        let view = real.join("sub").join("view");
        std::os::unix::fs::symlink(&real, &view).unwrap();
        let through = view.join("case.solidcraft");
        let mut opened = Session::default();
        opened.execute("doc.open", &json!({"path": through})).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        crate::documents::adopt(&mut app, opened);
        assert!(app.file_load.idle(), "neither link is read as the design opens");
        assert!((volume(&app) - 12000.0).abs() < 1e-6, "{}", volume(&app));
        let links = app.session.execute("component.links", &json!({})).unwrap();
        let files: Vec<std::path::PathBuf> =
            links["links"].as_array().unwrap().iter().map(|l| std::path::PathBuf::from(l["file"].as_str().unwrap())).collect();
        let (up, beside) = (up.canonicalize().unwrap(), beside.canonicalize().unwrap());
        assert!(files.contains(&up) && files.contains(&beside), "the reader opens the files outside the folder: {files:?}");
        assert!(links["links"].as_array().unwrap().iter().all(|l| l["changed"] == true), "both stay out of date: {links}");
        let design = through.to_string_lossy().into_owned();
        assert!(app.session.doc.links.values().all(|l| !inside_design_folder(Some(&design), &l.path)), "{:?}", app.session.doc.links);
        let _ = std::fs::remove_dir_all(top);
    }

    /// A received design in `<root>/received` linking `<root>/parts/housing.solidcraft`, whose file
    /// changed after the design last read it (6000 mm³ when read, 6250 now).
    fn outside_link(name: &str) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let root = std::env::temp_dir().join(format!("solidcraft-link-folders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("received")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        let housing = root.join("parts").join("housing.solidcraft");
        save_part(&housing, [10.0, 20.0, 30.0], false);
        let case = root.join("received").join("case.solidcraft");
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("component.insert_design", json!({"path": housing})).unwrap();
        app.run("file.save", json!({"path": case})).unwrap();
        drop(app);
        save_part(&housing, [10.0, 20.0, 30.0], true);
        let folder = engine_folder(&root.join("parts"));
        (root, case, folder)
    }

    /// `dir` as the engine names it in a question: symlinks in it resolved, without the `\\?\`
    /// that `canonicalize` adds on Windows.
    fn engine_folder(dir: &std::path::Path) -> String {
        solidcraft_engine::agent_setup::plain_path(&dir.canonicalize().unwrap())
    }

    /// Open `case` as File › Open does, or (`program`) as a control-channel caller does.
    fn open(app: &mut SolveApp, case: &std::path::Path, program: bool) {
        let mut request = crate::file_load::Request::new(&case.to_string_lossy(), crate::file_load::Kind::Open);
        if program {
            request.reply = Some(std::sync::mpsc::channel().0);
        }
        crate::file_load::begin_request(app, request).unwrap();
        settle(app);
    }

    fn out_of_date(app: &mut SolveApp) -> bool {
        app.session.execute("component.links", &json!({})).unwrap()["links"][0]["changed"] == true
    }

    /// Opening a design whose changed link is in another folder asks once, naming the folder, and
    /// reads nothing meanwhile; Load reads it and remembers the folder in the user's preferences.
    #[test]
    fn a_link_in_another_folder_asks_once_and_load_reads_it_and_remembers_the_folder() {
        let (root, case, folder) = outside_link("load");
        let mut app = SolveApp::new(Session::default(), Default::default());
        open(&mut app, &case, false);
        let prompt = app.link_folders_prompt.first().cloned().expect("asked");
        assert_eq!(prompt.folders, std::slice::from_ref(&folder));
        assert_eq!(prompt.session, app.session.uid);
        assert!((volume(&app) - 6000.0).abs() < 1e-6, "nothing is read before the answer: {}", volume(&app));
        assert!(out_of_date(&mut app));
        // Coming back to the tab doesn't ask again.
        app.link_folders_prompt.clear();
        update_changed(&mut app);
        assert!(app.link_folders_prompt.is_empty(), "asked once");
        app.link_folders_prompt = vec![prompt];
        crate::link_folders_prompt::answer(&mut app, crate::link_folders_prompt::Answer::Load);
        settle(&mut app);
        assert!(app.link_folders_prompt.is_empty());
        assert!((volume(&app) - 6250.0).abs() < 1e-6, "Load reads the link: {}", volume(&app));
        assert!(!out_of_date(&mut app));
        assert_eq!(app.preferences.linked_file_folders, [folder]);
        let _ = std::fs::remove_dir_all(root);
    }

    /// A design saved without the bodies of its linked designs (`file.save`'s
    /// `linked_bodies: false`) reads none from another folder unasked as it opens: the link has
    /// no bodies until Load reads its file, which is the same question a changed file asks.
    #[test]
    fn a_design_saved_without_linked_bodies_asks_before_it_reads_a_link_in_another_folder() {
        let root = std::env::temp_dir().join(format!("solidcraft-link-folders-left-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("received")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        let housing = root.join("parts").join("housing.solidcraft");
        save_part(&housing, [10.0, 20.0, 30.0], false);
        let case = root.join("received").join("case.solidcraft");
        let mut saved = SolveApp::new(Session::default(), Default::default());
        saved.run("component.insert_design", json!({"path": housing})).unwrap();
        saved.run("file.save", json!({"path": case, "linked_bodies": false})).unwrap();
        drop(saved);
        let folder = engine_folder(&root.join("parts"));

        let mut app = SolveApp::new(Session::default(), Default::default());
        open(&mut app, &case, false);
        let prompt = app.link_folders_prompt.first().cloned().expect("asked");
        assert_eq!(prompt.folders, std::slice::from_ref(&folder));
        assert!(volume(&app).abs() < 1e-6, "nothing is read before the answer: {}", volume(&app));
        assert!(out_of_date(&mut app));
        crate::link_folders_prompt::answer(&mut app, crate::link_folders_prompt::Answer::Load);
        settle(&mut app);
        assert!((volume(&app) - 6000.0).abs() < 1e-6, "Load reads the link: {}", volume(&app));
        assert!(!out_of_date(&mut app));
        assert_eq!(app.preferences.linked_file_folders, [folder]);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Recovering a design saved without linked bodies asks as opening it does: a link in
    /// another folder that the autosaved copy has no bodies of is not read until Load.
    #[test]
    fn recovering_a_design_saved_without_linked_bodies_asks_before_it_reads_a_link_in_another_folder() {
        let root = std::env::temp_dir().join(format!("solidcraft-link-folders-left-out-recover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("received")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        let housing = root.join("parts").join("housing.solidcraft");
        save_part(&housing, [10.0, 20.0, 30.0], false);
        let case = root.join("received").join("case.solidcraft");
        let mut saved = Session::default();
        saved.execute("component.insert_design", &json!({"path": housing})).unwrap();
        saved.execute("file.save", &json!({"path": case, "linked_bodies": false})).unwrap();
        // The design as the app had it when it stopped: the link not read yet, and a change.
        let mut stopped = Session::default();
        stopped.execute("doc.open", &json!({"path": case, "linked_bodies": "folder"})).unwrap();
        stopped.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0]})).unwrap();
        let recovery = root.join("recovery");
        let mut autosaver = solidcraft_engine::recovery::Autosaver::new(&recovery, std::time::Duration::ZERO).unwrap();
        assert!(autosaver.save(&stopped).unwrap());
        drop(autosaver);
        let entry = solidcraft_engine::recovery::orphans(&recovery).pop().unwrap();
        let folder = engine_folder(&root.join("parts"));

        let mut app = SolveApp::new(Session::default(), Default::default());
        let kind = crate::file_load::Kind::Recover { id: entry.id.clone(), dir: Some(recovery.to_string_lossy().into_owned()) };
        crate::file_load::begin_request(&mut app, crate::file_load::Request::new(&entry.name, kind)).unwrap();
        settle(&mut app);
        // Only its own 5 x 5 x 5 box.
        assert!((volume(&app) - 125.0).abs() < 1e-6, "nothing is read before the answer: {}", volume(&app));
        let prompt = app.link_folders_prompt.first().cloned().expect("asked");
        assert_eq!(prompt.folders, std::slice::from_ref(&folder));
        crate::link_folders_prompt::answer(&mut app, crate::link_folders_prompt::Answer::Load);
        settle(&mut app);
        assert!((volume(&app) - 6125.0).abs() < 1e-6, "Load reads the link: {}", volume(&app));
        let _ = std::fs::remove_dir_all(root);
    }

    /// Not now leaves the link out of date and remembers nothing; Update Links still reads it.
    #[test]
    fn not_now_keeps_a_link_in_another_folder_out_of_date_until_update_links() {
        let (root, case, _) = outside_link("not-now");
        let mut app = SolveApp::new(Session::default(), Default::default());
        open(&mut app, &case, false);
        assert!(!app.link_folders_prompt.is_empty());
        crate::link_folders_prompt::answer(&mut app, crate::link_folders_prompt::Answer::NotNow);
        settle(&mut app);
        assert!(app.link_folders_prompt.is_empty() && app.file_load.idle());
        assert!((volume(&app) - 6000.0).abs() < 1e-6, "{}", volume(&app));
        assert!(out_of_date(&mut app));
        assert!(app.preferences.linked_file_folders.is_empty());
        // The file changes again while the design is open: coming back to its tab still reads
        // nothing, as the user hasn't allowed the folder.
        let housing = root.join("parts").join("housing.solidcraft");
        save_part(&housing, [10.0, 20.0, 40.0], false);
        update_changed(&mut app);
        assert!(app.file_load.idle() && app.link_folders_prompt.is_empty(), "not read, not asked again");
        assert!((volume(&app) - 6000.0).abs() < 1e-6, "{}", volume(&app));
        let mut request = crate::file_load::Request::new("", crate::file_load::Kind::UpdateLinks);
        request.design.clone_from(&app.session.path);
        crate::file_load::begin_request(&mut app, request).unwrap();
        settle(&mut app);
        assert!((volume(&app) - 8000.0).abs() < 1e-6, "Update Links reads it: {}", volume(&app));
        let _ = std::fs::remove_dir_all(root);
    }

    /// On Windows a drive folder matches its `\\?\` spelling either way round, and a verbatim
    /// folder with no drive (a volume or a server share) still matches itself and what is below it.
    #[test]
    fn verbatim_windows_folders_match_their_own_spelling() {
        if !cfg!(windows) {
            return;
        }
        let within = |folder: &str, allowed: &str| folder_within(Path::new(folder), Path::new(allowed));
        assert!(within(r"C:\work\parts\gears", r"\\?\C:\work\parts"));
        assert!(within(r"\\?\C:\work\parts", r"c:\work"));
        assert!(within(r"\\?\Volume{0123}\parts\gears", r"\\?\Volume{0123}\parts"));
        assert!(within(r"\\?\UNC\server\share\parts", r"\\?\UNC\server\share"));
        assert!(!within(r"\\?\Volume{0123}\other", r"\\?\Volume{0123}\parts"));
        assert!(!within(r"C:\work\parts-old", r"\\?\C:\work\parts"));
        // Once the prefix is off, `/` separates names too, and a `..` written with it resolves.
        assert!(!within(r"\\?\C:\work\parts/../other", r"C:\work\parts"));
        assert!(!within(r"\\?\C:\work\parts/..", r"C:\work\parts"));
        assert!(within(r"\\?\C:\work\parts/gears", r"C:\work\parts"));
    }

    /// A folder the user allowed (or one below it) loads as it opens, with no question.
    #[test]
    fn a_link_in_an_allowed_folder_loads_without_asking() {
        let (root, case, folder) = outside_link("allowed");
        let mut allowed_folders = vec![folder.clone(), engine_folder(&root)];
        // The same folder as `canonicalize` spells it on Windows, `\\?\C:\…`, is the same folder.
        if cfg!(windows) {
            allowed_folders.push(format!(r"\\?\{folder}"));
        }
        for allowed in allowed_folders {
            let mut app = SolveApp::new(Session::default(), Default::default());
            app.preferences.linked_file_folders = vec![allowed];
            open(&mut app, &case, false);
            assert!(app.link_folders_prompt.is_empty(), "no question for an allowed folder");
            assert!((volume(&app) - 6250.0).abs() < 1e-6, "{}", volume(&app));
        }
        // A sibling folder that only starts with the same letters is not inside it.
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.linked_file_folders = vec![format!("{folder}-old"), "parts".into()];
        open(&mut app, &case, false);
        assert!(!app.link_folders_prompt.is_empty(), "a relative or neighbouring folder allows nothing");
        let _ = std::fs::remove_dir_all(root);
    }

    /// What a design holds never allows a folder: a design naming the folder list, or saved with
    /// keys of the preferences, still asks, and opening it changes no preference.
    #[test]
    fn a_design_cannot_allow_a_folder_itself() {
        let (root, case, folder) = outside_link("design");
        // The same design as plain JSON beside it, with the folder named in it.
        let mut read = Session::default();
        read.execute("doc.open", &json!({"path": case})).unwrap();
        let mut text = serde_json::to_value(&*read.doc).unwrap();
        text["linked_file_folders"] = json!([folder]);
        text["preferences"] = json!({"linked_file_folders": [folder]});
        let case = case.with_extension("json");
        std::fs::write(&case, serde_json::to_string(&text).unwrap()).unwrap();
        let mut app = SolveApp::new(Session::default(), Default::default());
        let before = serde_json::to_value(&app.preferences).unwrap();
        open(&mut app, &case, false);
        assert!(!app.link_folders_prompt.is_empty(), "still asks");
        assert_eq!(serde_json::to_value(&app.preferences).unwrap(), before, "the design changed no preference");
        assert!(out_of_date(&mut app));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A link in an allowed folder follows its file while the design is open; once the folder is
    /// removed from the list, a later change is no longer read by itself: it asks again, and after
    /// Not now it neither reads nor asks.
    #[test]
    fn removing_an_allowed_folder_stops_reading_its_links_by_themselves() {
        let (root, case, folder) = outside_link("removed");
        let housing = root.join("parts").join("housing.solidcraft");
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.linked_file_folders = vec![folder];
        open(&mut app, &case, false);
        assert!((volume(&app) - 6250.0).abs() < 1e-6, "{}", volume(&app));
        save_part(&housing, [10.0, 20.0, 40.0], false);
        update_changed(&mut app);
        settle(&mut app);
        assert!((volume(&app) - 8000.0).abs() < 1e-6, "an allowed folder's change is read: {}", volume(&app));
        app.preferences.linked_file_folders.clear();
        save_part(&housing, [10.0, 20.0, 30.0], true);
        update_changed(&mut app);
        assert!(app.file_load.idle(), "no longer read by itself");
        assert!((volume(&app) - 8000.0).abs() < 1e-6, "{}", volume(&app));
        assert_eq!(app.link_folders_prompt.len(), 1, "it asks again instead");
        crate::link_folders_prompt::answer(&mut app, crate::link_folders_prompt::Answer::NotNow);
        save_part(&housing, [10.0, 20.0, 40.0], false);
        update_changed(&mut app);
        assert!(app.file_load.idle() && app.link_folders_prompt.is_empty(), "after Not now: not read, not asked");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Two designs opened one after the other each keep their question: the first is still asked
    /// when its tab comes back, and answering it reads that design's link only.
    #[test]
    fn each_design_keeps_its_own_question() {
        let (first_root, first, first_folder) = outside_link("first");
        let (second_root, second, _) = outside_link("second");
        let mut app = SolveApp::new(Session::default(), Default::default());
        open(&mut app, &first, false);
        let first_uid = app.session.uid;
        open(&mut app, &second, false);
        assert_ne!(app.session.uid, first_uid);
        let sessions: Vec<u64> = app.link_folders_prompt.iter().map(|p| p.session).collect();
        assert_eq!(sessions, [first_uid, app.session.uid], "both questions wait");
        crate::documents::switch(&mut app, 0);
        assert_eq!(app.session.uid, first_uid);
        crate::link_folders_prompt::answer(&mut app, crate::link_folders_prompt::Answer::Load);
        settle(&mut app);
        assert!((volume(&app) - 6250.0).abs() < 1e-6, "{}", volume(&app));
        assert_eq!(app.preferences.linked_file_folders, [first_folder]);
        assert_eq!(app.link_folders_prompt.len(), 1, "the second design's question still waits");
        let _ = std::fs::remove_dir_all(first_root);
        let _ = std::fs::remove_dir_all(second_root);
    }

    /// A design opened through the control channel never asks, and reads no link in another
    /// folder, even one the user allowed: it stays out of date.
    #[test]
    fn a_design_opened_by_a_program_never_asks() {
        let (root, case, folder) = outside_link("program");
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.preferences.linked_file_folders = vec![folder];
        open(&mut app, &case, true);
        assert!(app.link_folders_prompt.is_empty() && app.file_load.idle());
        assert!(!app.docs.quiet_open, "only that open is quiet");
        assert!((volume(&app) - 6000.0).abs() < 1e-6, "{}", volume(&app));
        assert!(out_of_date(&mut app));
        let _ = std::fs::remove_dir_all(root);
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
        start(&mut app, component, None);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert!(app.status.as_ref().is_some_and(|(s, _, error)| s.contains("check this component's joints") && *error));
        assert!(app.session.log.iter().any(|s| s.contains("check this component's joints")));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// #16711: Relink Source and Use Its Names takes the new source's body names; Relink Source keeps them.
    #[test]
    fn relink_source_and_use_its_names_takes_the_new_sources_body_names() {
        let (mut app, component, _, folder, _) = fixture("names");
        let named = folder.join("named.solidcraft");
        let mut part = Session::default();
        part.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30, "body_name": "Plate"})).unwrap();
        part.execute("file.save", &json!({"path": named})).unwrap();
        let names = |app: &SolveApp| -> Vec<String> {
            app.session.model.state().bodies.iter().filter(|b| b.component == component).map(|b| b.name.clone()).collect()
        };
        let before = names(&app);
        assert!(!before.contains(&"Plate".to_string()));
        let rows = context_menu::entries(&app, &Target::Component { id: component });
        fn find<'a>(rows: &'a [crate::ui_kit::Item], id: &str) -> Option<&'a crate::ui_kit::Item> {
            rows.iter().find_map(|i| if i.id == id { Some(i) } else { find(&i.children, id) })
        }
        let item = find(&rows, "ui.relinkSourceNames").expect("the row");
        assert_eq!(item.params, Some(json!({"component": component})));
        answer(&mut app, Some(named.to_string_lossy().into_owned()));
        start(&mut app, component, None);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(names(&app), before, "Relink Source keeps the names");
        app.run("edit.undo", json!({})).unwrap();
        answer(&mut app, Some(named.to_string_lossy().into_owned()));
        context_menu::run_item(&mut app, item, egui::Pos2::ZERO);
        crate::file_picker::tick(&mut app, &egui::Context::default());
        assert_eq!(names(&app), ["Plate"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The component menu is a fixed 250 pt column that cuts a long label with an ellipsis, so in
    /// German "Relink Source (All Instances)…" and "Relink Source and Use Its Names…" must tell
    /// themselves apart at the start of the label and the new row must fit whole.
    #[test]
    fn the_two_relink_rows_differ_at_the_start_in_german() {
        crate::i18n::install();
        let _de = solidcraft_ui_text::scope("de");
        let all = solidcraft_ui_text::tr_command("ui.relinkSource", "Relink Source (All Instances)…");
        let names = solidcraft_ui_text::tr_command("ui.relinkSourceNames", "Relink Source and Use Its Names…");
        assert_eq!(names, "Mit Quellnamen neu verknüpfen…");
        let start = |s: &str| s.chars().take(8).collect::<String>();
        assert_ne!(start(all), start(names), "{all:?} and {names:?} read alike when cut");
        assert!(names.chars().count() <= 30, "{names:?} fits the menu column without an ellipsis");
    }

    /// A saved design's STEP link to a file outside the design's own folder is asked about when
    /// the tab comes back, and the file is not read meanwhile (the other test uses an unsaved
    /// design, which has no folder).
    #[test]
    fn a_saved_designs_step_link_outside_its_folder_asks_on_tab_return_and_reads_nothing() {
        let root = std::env::temp_dir().join(format!("solidcraft-step-link-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("received")).unwrap();
        std::fs::create_dir_all(root.join("parts")).unwrap();
        let source = root.join("parts").join("bracket.step");
        let export = |length: u32| {
            let mut part = Session::default();
            part.execute("solid.box", &json!({"length": length, "width": 10, "height": 10})).unwrap();
            part.execute("file.export", &json!({"path": source})).unwrap();
        };
        export(10);
        let case = root.join("received").join("case.solidcraft");
        let mut app = SolveApp::new(Session::default(), Default::default());
        app.run("file.insert_step", json!({"path": source})).unwrap();
        app.run("file.save", json!({"path": case})).unwrap();
        assert!(app.session.path.is_some(), "the design is saved");
        assert!((volume(&app) - 1000.0).abs() < 1e-6, "{}", volume(&app));
        let folder = engine_folder(&root.join("parts"));
        // Away to another tab; the source is replaced there by a longer part.
        crate::documents::open_tab(&mut app, Session::default()).unwrap();
        export(30);
        crate::documents::switch(&mut app, 0);
        let prompt = app.link_folders_prompt.first().cloned().expect("asked about the folder on tab return");
        assert_eq!(prompt.folders, std::slice::from_ref(&folder));
        assert!(app.file_load.idle(), "the file is not read while the question is open");
        assert!((volume(&app) - 1000.0).abs() < 1e-6, "nothing is read before the answer: {}", volume(&app));
        assert_eq!(app.session.execute("component.links", &json!({})).unwrap()["links"][0]["changed"], true);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn step_links_refresh_on_tab_return_obey_preferences_and_offer_link_actions() {
        let shot = |app: &mut SolveApp, name: &str| {
            if let Some(folder) = solidcraft_engine::geom::env::var("SCENARIO_SHOTS") {
                #[cfg(not(target_arch = "wasm32"))]
                if solidcraft_engine::capture::installed().is_none() {
                    solidcraft_engine::capture::install(Arc::new(|scene, cam, width, height| {
                        let (rgba, renderer) = solidcraft_gpu::env::headless_rgba(scene, cam, width, height)?;
                        Ok(solidcraft_engine::capture::Picture { rgba, renderer })
                    }));
                }
                std::fs::create_dir_all(&folder).unwrap();
                app.run("view.fit", json!({})).unwrap();
                if let Some(animation) = app.cam_anim.take() {
                    app.cam = animation.to;
                }
                let path = PathBuf::from(folder).join(format!("{name}.png"));
                let (request, _reply) = crate::control::ControlRequest::new("ui.render", json!({"path": path, "width": 1000, "height": 700}));
                match crate::control::handle(app, &crate::fonts::test_context(), &request) {
                    crate::control::Outcome::Done(reply) => assert_eq!(reply["ok"], true, "{reply}"),
                    _ => panic!("ui.render must finish without a window"),
                }
            }
        };
        let folder = std::env::temp_dir().join(format!("solidcraft-step-links-ui-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("bracket.step");
        let export = |length| {
            let mut part = Session::default();
            part.execute("solid.box", &json!({"length": length, "width": 10, "height": 10})).unwrap();
            part.execute("file.export", &json!({"path": source})).unwrap();
        };
        export(10);
        let mut app = SolveApp::new(Session::default(), Default::default());
        let linked = app.run("file.insert_step", json!({"path": source})).unwrap()["component"].as_u64().unwrap();
        for target in [Target::Component { id: linked }, Target::Occurrence { path: app.session.doc.first_path(linked) }] {
            let items = context_menu::items(&app, &target);
            // Main's component menu no longer lists Update Links or Break Link; a STEP link still
            // offers none of the linked-design actions.
            assert!(!items.iter().any(|item| matches!(item.id.as_str(), "ui.editInPlace" | "ui.openLinkedDesign" | "ui.relinkSource")));
        }
        app.start("component.update_links");
        assert!(app.file_load.busy(), "Update Links runs as a background update");
        settle(&mut app);
        shot(&mut app, "step-link-before");
        app.run("file.insert_step", json!({"path": source, "link": false})).unwrap();
        assert_eq!(app.session.doc.links.len(), 1, "link: false inserts an independent copy");
        // A request from the file picker or control channel passes the same option on.
        let mut request = crate::file_load::Request::new(source.to_str().unwrap(), crate::file_load::Kind::Insert);
        request.options.insert("link".into(), json!(false));
        crate::file_load::begin_request(&mut app, request).unwrap();
        settle(&mut app);
        assert_eq!(app.session.doc.links.len(), 1);
        crate::documents::open_tab(&mut app, Session::default()).unwrap();
        export(20);
        app.preferences.update_changed_links = false;
        crate::documents::switch(&mut app, 0);
        assert!(app.file_load.idle());
        let feature = app.session.doc.links[&linked].feature;
        let volume = |app: &SolveApp| {
            solidcraft_engine::kernel::measure(&app.session.model.state().bodies.iter().find(|b| b.feature == feature).unwrap().body).unwrap().volume
        };
        assert!((volume(&app) - 1000.0).abs() < 1e-6);
        app.preferences.update_changed_links = true;
        crate::documents::switch(&mut app, 1);
        crate::documents::switch(&mut app, 0);
        settle(&mut app);
        assert!((volume(&app) - 2000.0).abs() < 1e-6);
        shot(&mut app, "step-link-after");
        // Corrupt source on tab return keeps the import and remains retryable.
        crate::documents::switch(&mut app, 1);
        std::fs::write(&source, b"corrupt STEP").unwrap();
        crate::documents::switch(&mut app, 0);
        settle(&mut app);
        assert!((volume(&app) - 2000.0).abs() < 1e-6);
        assert_eq!(app.session.execute("component.links", &json!({})).unwrap()["links"][0]["changed"], true);
        crate::documents::switch(&mut app, 1);
        crate::documents::switch(&mut app, 0);
        assert!(app.file_load.busy(), "the same corrupt source is retried next time");
        settle(&mut app);
        assert!((volume(&app) - 2000.0).abs() < 1e-6);
        crate::documents::switch(&mut app, 1);
        export(30);
        crate::documents::switch(&mut app, 0);
        settle(&mut app);
        assert!((volume(&app) - 3000.0).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(folder);
    }
}
