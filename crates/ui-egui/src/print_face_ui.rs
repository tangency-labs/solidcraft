//! The print face in the app (#8429): Ctrl+B and the face menu mark the selected flat face as its
//! body's print face, or clear it when it already is one; the Send to Slicer window shows and
//! changes it (`dialogs_save_mesh`). Each change is a `body.print_face` command, so it undoes.

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
        let world = app.session.world_state();
        let body = world.body("Block").unwrap();
        let p = solidcraft_engine::geom::Vec3::new(point[0], point[1], point[2]);
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
}
