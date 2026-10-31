//! The print face in the app (#8429): the Send to Slicer window picks, shows and clears it
//! (`dialogs_save_mesh`), and the view shows it only while that window is open, for the bodies
//! being sent. Print Face (Ctrl+B) opens that window with its Print face input ready for a pick.
//! With the `print_face_everywhere` preference it works as it first did: Ctrl+B and the face menu
//! mark the selected flat face as its body's print face, or clear it when it already is one, and
//! the view shows every print face. Each change is a `body.print_face` command, so it undoes.

use serde_json::json;
use solidcraft_engine::Sel;

use crate::SolveApp;

/// Whether face `index` of `body` (as the viewport names it) is that body's print face now.
pub(crate) fn is_print_face(app: &SolveApp, body: &str, index: usize) -> bool {
    let world = app.session.world_state();
    world.body(body).and_then(|b| app.session.doc.print_face(&world, b)).and_then(Result::ok).is_some_and(|f| f.index == index)
}

/// `body`'s print face as a face pick, for the Send to Slicer window; `None` when it has none,
/// or it can't be found.
pub(crate) fn current(app: &SolveApp, body: &str) -> Option<Sel> {
    let world = app.session.world_state();
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

/// Print Face (Ctrl+B, the command palette): Send to Slicer with its Print face input taking
/// picks; with `print_face_everywhere`, [`toggle`].
pub(crate) fn start(app: &mut SolveApp) {
    if app.preferences.print_face_everywhere {
        return toggle(app);
    }
    app.start("file.send_to_slicer");
    if let Some(d) = app.dialog.as_mut().filter(|d| sending(&d.kind)) {
        d.active = 1.min(d.inputs.len().saturating_sub(1));
    }
}

/// `kind` is the Send to Slicer window (or Open in EasyPrint, which shares it).
fn sending(kind: &crate::dialogs::Kind) -> bool {
    matches!(kind, crate::dialogs::Kind::SaveMesh(k) if crate::dialogs_save_mesh::command_id(k) == "file.send_to_slicer")
}

/// The bodies whose print faces the view shows: `None` for every body (`print_face_everywhere`),
/// else the Bodies of an open Send to Slicer window, else none.
pub(crate) fn drawn_bodies(app: &SolveApp) -> Option<Vec<String>> {
    if app.preferences.print_face_everywhere {
        return None;
    }
    use crate::selection::SelectionExt as _;
    let open = app.dialog.as_ref().filter(|d| sending(&d.kind));
    Some(open.map(|d| crate::selection::items(&d.inputs, 0).body_names()).unwrap_or_default())
}

/// Opening Send to Slicer moved the ribbon from tab `from` to tab `to`: put it back once the
/// window is closed, however it closes.
pub(crate) fn remember_tab(app: &mut SolveApp, from: String, to: String) {
    if from != to {
        app.slicer_return_tab = Some((from, to));
    }
}

/// Per frame: Send to Slicer has closed, so the ribbon goes back to the tab it was opened from,
/// unless the user has moved it to another one meanwhile.
pub(crate) fn tick_return_tab(app: &mut SolveApp) {
    if app.slicer_return_tab.is_none() || app.dialog.as_ref().is_some_and(|d| sending(&d.kind)) {
        return;
    }
    if let Some((from, to)) = app.slicer_return_tab.take()
        && app.ui.tab == to
        && crate::workspace::visible_tabs(app).contains(&from.as_str())
    {
        app.ui.tab = from;
    }
}

/// Ctrl+B and the face menu with `print_face_everywhere`: the one selected face becomes its body's
/// print face, or stops being it when it already is.
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
    use crate::selection::SelectionExt as _;
    use serde_json::json;
    use solidcraft_engine::Sel;

    fn app() -> crate::SolveApp {
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length": 20, "width": 10, "height": 5, "corner": [0, 0, 0], "body_name": "Block"})).unwrap();
        app
    }

    /// [`app`] as it was first: Print Face (Ctrl+B) and the face menu mark the selected face.
    fn app_everywhere() -> crate::SolveApp {
        let mut app = app();
        app.preferences.print_face_everywhere = true;
        app
    }

    /// [`app`] with a second box, Lid, beside Block, and a way to start a slicer.
    fn two_bodies() -> crate::SolveApp {
        let mut app = app();
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [40, 0, 0], "body_name": "Lid"})).unwrap();
        app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        app
    }

    /// The face of `body` at `point`, as a pick.
    fn face(app: &crate::SolveApp, body: &str, point: [f64; 3]) -> Sel {
        let world = app.session.world_state();
        let p = solidcraft_engine::geom::Vec3::new(point[0], point[1], point[2]);
        let index = solidcraft_engine::doc::appearance::face_index_at(world.body(body).unwrap(), p).unwrap();
        Sel::Face { body: body.into(), index, point: p }
    }

    /// One frame of the open Send to Slicer window, after `change` to its inputs.
    fn frame(app: &mut crate::SolveApp, change: impl FnOnce(&mut [crate::selection::SelInput])) {
        let mut d = app.dialog.take().expect("Send to Slicer is open");
        change(&mut d.inputs);
        let crate::dialogs::Kind::SaveMesh(k) = &mut d.kind else { panic!("not Send to Slicer") };
        crate::dialogs_save_mesh::sync_print_faces(app, k, &mut d.inputs);
        app.dialog = Some(d);
    }

    fn inputs(app: &crate::SolveApp) -> &[crate::selection::SelInput] {
        &app.dialog.as_ref().unwrap().inputs
    }

    fn picked(app: &crate::SolveApp) -> Vec<(String, usize)> {
        inputs(app)[1].items.iter().filter_map(|s| if let Sel::Face { body, index, .. } = s { Some((body.clone(), *index)) } else { None }).collect()
    }

    /// Send to Slicer's Print face input takes one flat face per body, for several bodies: a face
    /// of a body not being sent adds that body, a second face of one replaces its first, and a face
    /// taken out clears it. The design keeps each, and the window shows them again when reopened.
    #[test]
    fn send_to_slicer_picks_one_print_face_per_body_and_adds_its_body() {
        let mut app = two_bodies();
        app.session.selection = vec![Sel::Body { name: "Block".into() }];
        app.start("file.send_to_slicer");
        assert!(inputs(&app)[1].multi);
        frame(&mut app, |_| {});
        assert!(picked(&app).is_empty());
        let side = face(&app, "Block", [20.0, 5.0, 2.5]);
        frame(&mut app, |i| i[1].items.push(side.clone()));
        let lid = face(&app, "Lid", [45.0, 5.0, 5.0]);
        frame(&mut app, |i| i[1].items.push(lid.clone()));
        let bodies = crate::selection::items(inputs(&app), 0).body_names();
        assert_eq!(bodies, ["Block", "Lid"], "Lid joins the bodies being sent");
        assert!(super::is_print_face(&app, "Block", face_index(&side)));
        assert!(super::is_print_face(&app, "Lid", face_index(&lid)));
        assert_eq!(picked(&app).len(), 2);
        // Another face of Block replaces the first: one per body.
        let bottom = face(&app, "Block", [10.0, 5.0, 0.0]);
        frame(&mut app, |i| i[1].items.push(bottom.clone()));
        assert_eq!(picked(&app).len(), 2);
        assert!(super::is_print_face(&app, "Block", face_index(&bottom)));
        // Taking Lid's out clears it.
        frame(&mut app, |i| i[1].items.retain(|s| !matches!(s, Sel::Face { body, .. } if body == "Lid")));
        assert!(!app.session.doc.print_faces.contains_key("Lid"));
        assert!(app.session.doc.print_faces.contains_key("Block"));
        // An undo shows in the window.
        app.run("edit.undo", json!({})).unwrap();
        frame(&mut app, |_| {});
        assert_eq!(picked(&app).len(), 2, "undo brings Lid's back");
        // Kept per body: taking Lid out of Bodies leaves its print face, only unshown.
        frame(&mut app, |i| i[0].items.retain(|s| !matches!(s, Sel::Body { name } if name == "Lid")));
        assert_eq!(picked(&app).len(), 1);
        assert!(app.session.doc.print_faces.contains_key("Lid"));
        crate::dialogs::cancel(&mut app);
        app.session.selection = vec![Sel::Body { name: "Lid".into() }, Sel::Body { name: "Block".into() }];
        app.start("file.send_to_slicer");
        frame(&mut app, |_| {});
        assert_eq!(picked(&app).len(), 2, "reopened, it shows both");
    }

    fn face_index(sel: &Sel) -> usize {
        if let Sel::Face { index, .. } = sel { *index } else { usize::MAX }
    }

    /// The view shows print faces only while Send to Slicer is open, for the bodies it sends; with
    /// `print_face_everywhere`, always and for every body.
    #[test]
    fn print_faces_are_drawn_only_for_the_bodies_send_to_slicer_sends() {
        let mut app = two_bodies();
        assert_eq!(super::drawn_bodies(&app), Some(vec![]));
        app.session.selection = vec![Sel::Body { name: "Lid".into() }];
        app.start("file.send_to_slicer");
        assert_eq!(super::drawn_bodies(&app), Some(vec!["Lid".to_string()]));
        crate::dialogs::cancel(&mut app);
        assert_eq!(super::drawn_bodies(&app), Some(vec![]));
        app.preferences.print_face_everywhere = true;
        assert_eq!(super::drawn_bodies(&app), None);
    }

    /// Print Face (Ctrl+B) opens Send to Slicer with its Print face input taking picks, and the
    /// face menu has Send to Slicer and no Print Face; with the old preference, both as before.
    #[test]
    fn print_face_lives_in_send_to_slicer_unless_the_preference_keeps_it_everywhere() {
        let mut app = two_bodies();
        let side = face(&app, "Block", [20.0, 5.0, 2.5]);
        app.session.selection = vec![side.clone()];
        let menu = |app: &crate::SolveApp| -> Vec<String> {
            crate::context_menu::items(app, &crate::context_menu::Target::Viewport).into_iter().map(|i| i.id).collect()
        };
        let ids = menu(&app);
        assert!(ids.iter().any(|id| id == "ui.sendToSlicer"), "{ids:?}");
        assert!(!ids.iter().any(|id| id == "body.print_face"), "{ids:?}");
        app.start("body.print_face");
        let d = app.dialog.as_ref().expect("Send to Slicer opens");
        assert_eq!(d.inputs[d.active].label, "Print face");
        assert!(app.session.doc.print_faces.is_empty(), "nothing is marked until a face is picked");
        crate::dialogs::cancel(&mut app);
        app.preferences.print_face_everywhere = true;
        app.session.selection = vec![side];
        assert!(menu(&app).iter().any(|id| id == "body.print_face"));
        app.start("body.print_face");
        assert!(app.dialog.is_none());
        assert!(app.session.doc.print_faces.contains_key("Block"));
    }

    /// Opening Send to Slicer moves the ribbon to the tab that has it; closing it, however, goes
    /// back to the tab it was opened from.
    #[test]
    fn closing_send_to_slicer_returns_to_the_tab_it_was_opened_from() {
        let mut app = two_bodies();
        let start = app.ui.tab.clone();
        app.session.selection = vec![Sel::Body { name: "Block".into() }];
        app.start("file.send_to_slicer");
        assert_ne!(app.ui.tab, start, "the ribbon shows the tab with Send to Slicer");
        super::tick_return_tab(&mut app);
        assert_ne!(app.ui.tab, start, "not while it is open");
        crate::dialogs::cancel(&mut app);
        super::tick_return_tab(&mut app);
        assert_eq!(app.ui.tab, start);
        assert!(app.slicer_return_tab.is_none());
    }

    /// Select the face of Block at `point`; its index.
    fn select_face(app: &mut crate::SolveApp, point: [f64; 3]) -> usize {
        let world = app.session.world_state();
        let body = world.body("Block").unwrap();
        let p = solidcraft_engine::geom::Vec3::new(point[0], point[1], point[2]);
        let index = solidcraft_engine::doc::appearance::face_index_at(body, p).unwrap();
        app.session.selection = vec![Sel::Face { body: "Block".into(), index, point: p }];
        index
    }

    #[test]
    fn ctrl_b_marks_the_selected_face_and_clears_it_the_second_time() {
        let mut app = app_everywhere();
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
        let mut app = app_everywhere();
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
}
