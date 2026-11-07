//! The print face in the app (#8429): Ctrl+B and the face menu mark the selected flat face as its
//! body's print face, or clear it when it already is one; the Send to Slicer window shows and
//! changes it (`dialogs_save_mesh`). Each change is a `body.print_face` command, so it undoes.

use serde_json::json;
use solidcraft_engine::Sel;

use crate::SolveApp;

/// Whether the view tints print faces now (#11512): while the Send to Slicer window is open, or
/// at all times with the "Keep print faces" preference on.
pub(crate) fn shown(app: &SolveApp) -> bool {
    app.preferences.keep_print_faces || send_to_slicer_open(app)
}

/// Whether the Send to Slicer window (or Open with EasyPrint, which shares it) is open.
fn send_to_slicer_open(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(&d.kind, crate::dialogs::Kind::SaveMesh(k) if k.sends()))
}

/// Hand the "Keep print faces" preference to `session`: whether a save keeps print faces and
/// an opened design keeps the ones it has.
pub(crate) fn sync(session: &mut solidcraft_engine::Session, prefs: &crate::prefs::Prefs) {
    session.keep_print_faces = prefs.keep_print_faces;
}

/// Whether face `index` of `body` (as the viewport names it) is that body's print face now.
pub(crate) fn is_print_face(app: &SolveApp, body: &str, index: usize) -> bool {
    let world = crate::open_meshing::session_world(&app.session);
    world.body(body).and_then(|b| app.session.doc.print_face(&world, b)).and_then(Result::ok).is_some_and(|f| f.index == index)
}

/// `body`'s print face as a face pick, for the Send to Slicer window; `None` when it has none,
/// or it can't be found.
pub(crate) fn current(app: &SolveApp, body: &str) -> Option<Sel> {
    let world = crate::open_meshing::session_world(&app.session);
    let b = world.body(body)?;
    let face = app.session.doc.print_face(&world, b)?.ok()?;
    let point = solidcraft_engine::doc::naming::face_spot(b, face.index).map_or(face.point, |spot| spot.point);
    Some(Sel::Face { body: body.to_string(), index: face.index, point })
}

/// Mark face `face` as its body's print face (a face pick), or clear `body`'s with none.
pub(crate) fn set(app: &mut SolveApp, body: &str, face: Option<&Sel>) -> bool {
    let params = match face {
        Some(Sel::Face { body, point, .. }) => json!({"body": body, "point": [point.x, point.y, point.z]}),
        _ => json!({"body": body}),
    };
    app.run("body.print_face", params).is_ok()
}

/// Ctrl+B and the face menu: the one selected face becomes its body's print face, or stops being
/// it when it already is.
pub(crate) fn toggle(app: &mut SolveApp) {
    let picked = match app.session.selection.as_slice() {
        [face @ Sel::Face { body, index, .. }] => Some((face.clone(), body.clone(), *index)),
        _ => None,
    };
    let Some((face, body, index)) = picked else {
        app.set_status(crate::i18n::tr("ui.print_face_pick"), true);
        return;
    };
    let clear = is_print_face(app, &body, index);
    if set(app, &body, if clear { None } else { Some(&face) }) {
        let id = if clear { "ui.print_face_cleared" } else { "ui.print_face_set" };
        app.set_status(crate::i18n::format(id, &[("body", &body)]), false);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::Sel;

    fn app() -> crate::SolveApp {
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length": 20, "width": 10, "height": 5, "corner": [0, 0, 0], "body_name": "Block"})).unwrap();
        app
    }

    /// Select the face of Block at `point`; its index.
    fn select_face(app: &mut crate::SolveApp, point: [f64; 3]) -> usize {
        let world = crate::open_meshing::session_world(&app.session);
        let body = world.body("Block").unwrap();
        let p = solidcore_record::Vec3::new(point[0], point[1], point[2]);
        let index = solidcraft_engine::doc::appearance::face_index_at(body, p).unwrap();
        app.session.selection = vec![Sel::Face { body: "Block".into(), index, point: p }];
        index
    }

    #[test]
    fn ctrl_b_marks_the_selected_face_and_clears_it_the_second_time() {
        let mut app = app();
        let index = select_face(&mut app, [20.0, 5.0, 2.5]);
        app.start("body.print_face");
        assert!(app.session.doc.print_faces.contains_key("Block"));
        assert!(super::is_print_face(&app, "Block", index));
        assert!(super::current(&app, "Block").is_some());
        app.start("body.print_face");
        assert!(app.session.doc.print_faces.is_empty());
        assert!(super::current(&app, "Block").is_none());
    }

    #[test]
    fn ctrl_b_without_one_face_selected_marks_nothing() {
        let mut app = app();
        app.session.selection = vec![Sel::Body { name: "Block".into() }];
        app.start("body.print_face");
        assert!(app.session.doc.print_faces.is_empty());
    }

    #[test]
    fn the_highlight_strength_defaults_to_a_fifth_and_ui_prefs_keeps_it_in_range() {
        let mut app = app();
        assert!((app.preferences.print_face_highlight - 0.2).abs() < 1e-6);
        crate::prefs::set(&mut app, &json!({"print_face_highlight": 0.5})).unwrap();
        assert!((app.preferences.print_face_highlight - 0.5).abs() < 1e-6);
        assert!(crate::prefs::set(&mut app, &json!({"print_face_highlight": 1.5})).is_err());
        assert!((app.preferences.print_face_highlight - 0.5).abs() < 1e-6);
    }

    /// What the view's highlight layer draws, in bytes: more when a print face is tinted.
    fn drawn(h: &crate::scenario::Harness) -> usize {
        let sc = crate::viewport::build_highlight(&h.app);
        let blocks: usize = sc.blocks.iter().map(|b| b.bytes.vertices.len() + b.bytes.lines.len() + b.bytes.hidden.len()).sum();
        sc.tris.len() + sc.lines.len() + sc.hidden.len() + sc.overlay.len() + blocks
    }

    /// A harness with Block, nothing selected, and Block's top face marked as its print face; and
    /// what the highlight layer drew before the face was marked.
    fn marked() -> (crate::scenario::Harness, usize) {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 20, "width": 10, "height": 5, "corner": [0, 0, 0], "body_name": "Block"})).unwrap();
        h.app.session.selection.clear();
        h.settle();
        let before = drawn(&h);
        let top = Sel::Face { body: "Block".into(), index: 0, point: solidcore_record::Vec3::new(10.0, 5.0, 5.0) };
        assert!(super::set(&mut h.app, "Block", Some(&top)));
        h.app.session.selection.clear();
        h.settle();
        assert!(h.app.session.doc.print_faces.contains_key("Block"));
        (h, before)
    }

    #[test]
    fn the_print_face_is_tinted_only_while_send_to_slicer_is_open() {
        let (mut h, before) = marked();
        assert!(!h.app.preferences.keep_print_faces, "off by default");
        assert!(!super::shown(&h.app), "no tint with the window closed");
        assert_eq!(drawn(&h), before, "the closed window leaves the view as it was before the face was marked");
        h.app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        h.app.start("file.send_to_slicer");
        h.settle();
        assert!(h.app.dialog.is_some(), "Send to Slicer is open");
        assert!(super::shown(&h.app), "the window shows the print face");
        assert!(drawn(&h) > before, "and the view tints it");
        crate::dialogs::cancel(&mut h.app);
        h.settle();
        assert!(h.app.dialog.is_none());
        assert!(!super::shown(&h.app), "closing the window hides the tint");
        assert_eq!(drawn(&h), before);
        assert!(h.app.session.doc.print_faces.contains_key("Block"), "the print face itself stays for this session");
    }

    #[test]
    fn keep_print_faces_tints_the_print_face_at_all_times_as_before() {
        let (mut h, before) = marked();
        crate::prefs::set(&mut h.app, &json!({"keep_print_faces": true})).unwrap();
        h.settle();
        assert!(super::shown(&h.app));
        assert!(drawn(&h) > before, "the window is closed, and the view tints the print face");
    }

    /// Save the design to a fresh file for `case` and open it again in the app; the file.
    fn save_and_reopen(app: &mut crate::SolveApp, case: &str) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-print-face-ui-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("block.solidcraft").to_string_lossy().into_owned();
        app.run("file.save", json!({"path": path})).unwrap();
        app.run("file.new", json!({})).unwrap();
        app.run("doc.open", json!({"path": path})).unwrap();
        path
    }

    #[test]
    fn a_print_face_is_not_saved_unless_kept() {
        let mut app = app();
        select_face(&mut app, [20.0, 5.0, 2.5]);
        app.start("body.print_face");
        assert!(app.session.doc.print_faces.contains_key("Block"));
        let path = save_and_reopen(&mut app, "default");
        assert!(app.session.doc.print_faces.is_empty(), "the design opens without its print face");
        assert_eq!(app.session.doc.features.len(), 1, "and with everything else");
        // A design saved with a print face (by an older version, or with the preference on)
        // opens without it, and the rest of it intact.
        let mut older = solidcraft_engine::Session::default();
        older.execute("doc.open", &json!({"path": path})).unwrap();
        older.execute("body.print_face", &json!({"body": "Block", "point": [20, 5, 2.5]})).unwrap();
        older.execute("file.save", &json!({"path": path})).unwrap();
        app.run("doc.open", json!({"path": path})).unwrap();
        assert!(app.session.doc.print_faces.is_empty());
        assert!(!app.session.is_dirty());
        assert_eq!(app.session.doc.features.len(), 1);
        // With the preference on, the print face is saved and opened as before.
        crate::prefs::set(&mut app, &json!({"keep_print_faces": true})).unwrap();
        app.run("doc.open", json!({"path": path})).unwrap();
        assert!(app.session.doc.print_faces.contains_key("Block"), "kept, an older design opens with its print face");
        save_and_reopen(&mut app, "kept");
        assert!(app.session.doc.print_faces.contains_key("Block"), "and a save keeps it");
    }
}
