//! Change a placed standard part through the existing, undoable `parts.resize` command.

use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::{Sel, doc::Document};

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::dialogs_parts::Family;
use crate::selection::SelInput;
use crate::ui_kit::{choice_row, row};

/// The active catalogue configuration belongs to the shared part, not its placements.
pub(crate) fn configuration(app: &SolveApp, component: u64) -> Option<String> {
    let part = app.session.doc.parts.get(&component)?;
    let size = part.size.trim();
    if size.is_empty() {
        return None;
    }
    Some(configuration_name(size, part.length))
}

fn configuration_name(size: &str, length: Option<f64>) -> String {
    match length.filter(|length| length.is_finite() && *length > 0.0) {
        Some(length) => format!("{size}×{length}"),
        None => size.to_owned(),
    }
}

pub(crate) fn browser_label(app: &SolveApp, component: u64, name: &str) -> String {
    match configuration(app, component) {
        Some(configuration) => format!("{name} [{configuration}]"),
        None => name.to_owned(),
    }
}

#[derive(Clone)]
pub struct Resize {
    pub component: u64,
    pub size: usize,
    pub length: Option<f64>,
    original_size: usize,
    family: Family,
    source: Arc<Document>,
}

impl std::fmt::Debug for Resize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resize")
            .field("component", &self.component)
            .field("family", &self.family.id)
            .field("size", &self.size)
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

/// Resolve a whole definition, including bodies displayed through an occurrence path.
pub fn component_for_bodies(app: &SolveApp, bodies: &[String]) -> Option<u64> {
    let state = app.session.world_state();
    let mut component = None;
    for name in bodies {
        let body = state.bodies.iter().find(|b| b.name == *name)?;
        let id = app.session.doc.model_body_component(body);
        if !app.session.doc.parts.contains_key(&id) || component.is_some_and(|c| c != id) {
            return None;
        }
        component = Some(id);
    }
    component
}

pub fn selected_component(app: &SolveApp) -> Option<u64> {
    if app.session.selection.is_empty() {
        let [id] = app.tree.picked_components.as_slice() else { return None };
        return app.session.doc.parts.contains_key(id).then_some(*id);
    }
    let mut bodies = Vec::new();
    let mut component = None;
    for sel in &app.session.selection {
        match sel {
            Sel::Body { name } => bodies.push(name.clone()),
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => bodies.push(body.clone()),
            Sel::Occurrence { path } => {
                let id = app.session.occurrence_placement(path).ok()?.component;
                if !app.session.doc.parts.contains_key(&id) || component.is_some_and(|c| c != id) {
                    return None;
                }
                component = Some(id);
            }
            _ => return None,
        }
    }
    if !bodies.is_empty() {
        let id = component_for_bodies(app, &bodies)?;
        if component.is_some_and(|c| c != id) {
            return None;
        }
        component = Some(id);
    }
    component
}

impl Resize {
    fn new(app: &SolveApp, component: u64) -> Option<Self> {
        let part = app.session.doc.parts.get(&component)?;
        let response = app.session.scratch().execute("parts.library", &json!({})).ok()?;
        let mut family = crate::dialogs_parts::families(&response).into_iter().find(|f| f.id == part.family)?;
        for (label, row) in &mut family.sizes {
            if let Some(diameter) = row.get("size").and_then(Value::as_f64) {
                *label = diameter.to_string();
            }
        }
        let size = family.sizes.iter().position(|(s, row)| match family.id.as_str() {
            "dowel_pin" => part
                .size
                .trim_start_matches(['d', 'D', 'Ø'])
                .trim()
                .parse::<f64>()
                .ok()
                .is_some_and(|d| row.get("d").and_then(Value::as_f64) == Some(d)),
            "bearing" => s == part.size.trim(),
            _ => s.eq_ignore_ascii_case(&part.size),
        })?;
        Some(Self { component, size, original_size: size, length: part.length, family, source: app.session.doc.clone() })
    }

    fn current(&self, app: &SolveApp) -> Result<(), String> {
        if !Arc::ptr_eq(&self.source, &app.session.doc) || !app.session.doc.parts.contains_key(&self.component) {
            return Err("The design changed. Reopen Change Part Size for the component.".into());
        }
        Ok(())
    }

    fn lengths(&self) -> Option<Vec<f64>> {
        // A future library family may give each size its own lengths.
        let row = &self.family.sizes.get(self.size)?.1;
        let mut lengths: Vec<f64> = match row.get("lengths").and_then(Value::as_array) {
            Some(values) => values.iter().filter_map(|v| v.as_f64().or_else(|| v.get("length").and_then(Value::as_f64))).collect(),
            None => self.family.lengths.clone()?,
        };
        if let Some(current) = self.length {
            lengths.push(current);
        }
        lengths.retain(|l| l.is_finite() && *l > 0.0 && *l <= 2000.0);
        lengths.sort_by(f64::total_cmp);
        lengths.dedup();
        Some(lengths)
    }
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    if id != "parts.resize" {
        return None;
    }
    Some((Kind::PartSize(Resize::new(app, selected_component(app)?)?), vec![]))
}

/// Select the context menu's exact target before using the normal command-start lifecycle.
pub fn open(app: &mut SolveApp, component: u64) {
    if Resize::new(app, component).is_none() {
        app.set_status("Select an inserted standard part to change its size", true);
        return;
    }
    app.session.selection.clear();
    app.tree.picked_components = vec![component];
    app.start("parts.resize");
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, k: &mut Resize) -> bool {
    row(ui, "Type", |ui| {
        ui.add(egui::Label::new(format!("{} ({})", crate::dialogs_parts::family_name(&k.family), k.family.standard)).wrap());
    });
    let sizes: Vec<&str> = k.family.sizes.iter().map(|s| s.0.as_str()).collect();
    choice_row(ui, "Size", "part_resize_size", &mut k.size, &sizes);
    if let Some(lengths) = k.lengths() {
        let mut index = k.length.and_then(|l| lengths.iter().position(|x| *x == l)).unwrap_or(0);
        let labels: Vec<String> = lengths.iter().map(|l| crate::prefs::show_mm(app, *l, 1)).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        choice_row(ui, "Length", "part_resize_length", &mut index, &labels);
        k.length = lengths.get(index).copied();
    }
    if let Some((size, _)) = k.family.sizes.get(k.size) {
        row(ui, "Configuration", |ui| {
            ui.label(configuration_name(size, k.length));
        });
    }
    crate::ui_kit::text(ui, crate::i18n::tr("ui.part_size_shared"), crate::ui_kit::Text::Hint("part_size_shared"));
    false
}

pub fn commands(app: &SolveApp, k: &Resize) -> Result<Vec<(String, Value)>, String> {
    k.current(app)?;
    let (size, _) = k.family.sizes.get(k.size).ok_or("Pick a part size")?;
    let mut params = json!({"component":k.component,"size":size});
    if let Some(length) = k.length {
        if !length.is_finite() || length <= 0.0 || length > 2000.0 {
            return Err("Part length must be greater than zero and at most 2000 mm".into());
        }
        params["length"] = json!(length);
    }
    if k.size == k.original_size && k.source.parts.get(&k.component).is_some_and(|part| part.length == k.length) {
        return Ok(Vec::new());
    }
    Ok(vec![("parts.resize".into(), params)])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::context_menu::{self, Target};
    use solidcraft_engine::{Session, geom::Vec3};

    fn part(family: &str, size: &str, length: Option<f64>) -> (SolveApp, u64) {
        let mut app = SolveApp::new(Session::default(), crate::Services::default());
        let mut p = json!({"family":family,"size":size,"point":[10,20,30]});
        if let Some(length) = length {
            p["length"] = json!(length);
        }
        let id = app.run("parts.insert", p).unwrap()["component"].as_u64().unwrap();
        (app, id)
    }

    fn menu(app: &mut SolveApp, target: Target) {
        let items = context_menu::items(app, &target);
        let item = items.iter().find(|i| i.id == "parts.resize").expect("Change Part Size in context menu");
        context_menu::run_item(app, item, egui::Pos2::ZERO);
    }

    fn editor(app: &SolveApp) -> &Resize {
        let Kind::PartSize(k) = &app.dialog.as_ref().unwrap().kind else { panic!("not a part-size dialog") };
        k
    }

    fn change(app: &mut SolveApp, size: &str, length: Option<f64>) {
        let Kind::PartSize(k) = &mut app.dialog.as_mut().unwrap().kind else { panic!("not a part-size dialog") };
        k.size = k.family.sizes.iter().position(|(s, _)| s == size).unwrap();
        k.length = length;
    }

    fn apply(app: &mut SolveApp) {
        let commands = crate::dialogs::apply_commands(app, app.dialog.as_ref().unwrap()).unwrap();
        for (id, params) in commands {
            app.run(&id, params).unwrap();
        }
        crate::dialogs::cancel(app);
    }

    #[test]
    fn part_size_browser_configuration_edits_the_same_part_and_follows_history_and_reopen() {
        let (mut app, component) = part("socket_head_cap_screw", "M6", Some(20.0));
        let first = app.session.doc.occurrence_of(component).unwrap().id;
        let second = app.run("occurrence.copy", json!({"occurrence":first,"translate":[40,0,0]})).unwrap()["occurrence"].as_u64().unwrap();
        app.run("joint.create", json!({"name":"Seat","type":"rigid","a":{"occurrence":0,"point":[0,0,0]},"b":{"occurrence":first,"point":[0,0,0]}}))
            .unwrap();
        let occurrences = app.session.doc.occurrences.clone();
        let joints = app.session.doc.assembly.joints.clone();
        let before = app.session.doc.clone();
        let undo = app.session.undo.len();
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.home.open = false;
        for id in [first, second] {
            h.app.tree.collapsed.insert(format!("o:{id}"));
        }
        h.frames(3);
        assert_eq!(crate::browser::drawn_rows().iter().filter(|(label, _, _, _)| label.ends_with("[M6×20]")).count(), 2);
        let at = h.call("ui.at", json!({"handle":format!("row:occurrence:{second}")}));
        let at = at["result"].as_array().unwrap();
        h.call("ui.click", json!({"x":at[0],"y":at[1],"double":true}));
        assert_eq!(editor(&h.app).component, component);
        assert_eq!(h.app.session.doc, before, "opening configuration is not a model edit");
        assert_eq!(h.app.session.undo.len(), undo);
        crate::dialogs::cancel(&mut h.app);
        h.frames(2);
        let at = h.call("ui.at", json!({"handle":format!("activate:occurrence:{second}")}));
        let at = at["result"].as_array().unwrap();
        h.call("ui.click", json!({"x":at[0],"y":at[1]}));
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.session.active_occurrence_path, Some(vec![second]));
        let items = context_menu::items(&h.app, &Target::Occurrence { path: vec![second] });
        let edit = items.iter().find(|item| item.id == "parts.resize").unwrap();
        assert_eq!(edit.label, "Edit");
        context_menu::run_item(&mut h.app, edit, egui::Pos2::ZERO);
        change(&mut h.app, "M8", Some(25.0));
        apply(&mut h.app);
        h.frames(3);
        assert_eq!(crate::browser::drawn_rows().iter().filter(|(label, _, _, _)| label.ends_with("[M8×25]")).count(), 2);
        assert_eq!(h.app.session.doc.occurrences, occurrences);
        assert_eq!(h.app.session.doc.assembly.joints, joints);
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert_eq!(h.app.session.active_occurrence_path, Some(vec![second]));
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(3);
        assert_eq!(crate::browser::drawn_rows().iter().filter(|(label, _, _, _)| label.ends_with("[M6×20]")).count(), 2);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(configuration(&h.app, component).as_deref(), Some("M8×25"));
        let file = std::env::temp_dir().join(format!("solidcraft-part-configuration-{}.solidcraft", std::process::id()));
        h.app.run("file.save", json!({"path":file})).unwrap();
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path":file})).unwrap();
        let reopened = SolveApp::new(reopened, Default::default());
        assert_eq!(configuration(&reopened, component).as_deref(), Some("M8×25"));
        assert_eq!(reopened.session.doc.occurrences, occurrences);
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(file.with_extension("solidcraft.bak"));
    }

    #[test]
    fn part_size_configuration_names_use_catalogue_dimensions_and_keep_ordinary_labels() {
        for (family, size, length, expected) in
            [("socket_head_cap_screw", "M6", Some(20.5), "M6×20.5"), ("hex_nut", "M8", None, "M8"), ("bearing", "608", None, "608")]
        {
            let (mut app, component) = part(family, size, length);
            app.run("document.units", json!({"units":"in"})).unwrap();
            assert_eq!(configuration(&app, component).as_deref(), Some(expected));
            assert_eq!(browser_label(&app, component, "Named placement"), format!("Named placement [{expected}]"));
            assert_eq!(browser_label(&app, 0, "Assembly"), "Assembly");
        }
    }

    #[test]
    fn part_size_prefills_actual_values_and_resizes_shared_occurrences_in_one_undo() {
        let (mut app, id) = part("socket_head_cap_screw", "M5", Some(20.5));
        app.run("occurrence.copy", json!({"component":id,"translate":[40,0,0]})).unwrap();
        let before = app.session.doc.clone();
        let undo = app.session.undo.len();
        let occurrences = before.occurrences.clone();
        menu(&mut app, Target::Component { id });
        let k = editor(&app);
        assert_eq!(k.family.sizes[k.size].0, "M5");
        assert_eq!(k.length, Some(20.5));
        assert!(k.lengths().unwrap().contains(&20.5), "keep non-catalogue lengths on opening");
        assert_eq!(app.session.undo.len(), undo);
        change(&mut app, "M6", Some(30.0));
        apply(&mut app);
        assert_eq!(app.session.doc.occurrences, occurrences);
        assert_eq!(app.session.doc.parts[&id].size, "M6");
        assert_eq!(app.session.doc.parts[&id].length, Some(30.0));
        assert_eq!(app.session.undo.len(), undo + 1);
        let after = app.session.doc.clone();
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc, before);
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.session.doc, after);
    }

    #[test]
    fn part_size_keeps_the_joint_and_placement_of_a_seated_screw() {
        let mut app = SolveApp::new(Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length":40,"width":30,"height":10,"body_name":"Plate"})).unwrap();
        app.run("solid.hole", json!({"position":[20,15,10],"diameter":6.4})).unwrap();
        let out = app.run("parts.insert", json!({"family":"socket_head_cap_screw","size":"M6","length":20,"at":[23.2,15,10]})).unwrap();
        assert!(out["joint"].is_number());
        let id = out["component"].as_u64().unwrap();
        let joints = app.session.doc.assembly.joints.clone();
        let occurrences = app.session.doc.occurrences.clone();
        menu(&mut app, Target::Component { id });
        change(&mut app, "M8", Some(25.0));
        apply(&mut app);
        assert_eq!(app.session.doc.assembly.joints, joints);
        assert_eq!(app.session.doc.occurrences, occurrences);
        let state = app.session.world_state();
        let body = state.bodies.iter().find(|b| app.session.doc.model_body_component(b) == id).unwrap();
        let bounds = body.mesh().bounds();
        assert!((bounds.max.z - 18.0).abs() < 1e-6 && (bounds.min.z + 15.0).abs() < 1e-6);
    }

    #[test]
    fn part_size_resolves_body_face_and_occurrence_contexts_and_rejects_mixed_targets() {
        let (mut app, id) = part("hex_nut", "M5", None);
        let body = app.session.world_state().bodies.iter().find(|b| app.session.doc.model_body_component(b) == id).unwrap().name.clone();
        menu(&mut app, Target::Body { name: body.clone() });
        assert_eq!(editor(&app).component, id);
        crate::dialogs::cancel(&mut app);
        app.session.selection = vec![Sel::Face { body: body.clone(), index: 0, point: Vec3::ZERO }];
        menu(&mut app, Target::Viewport);
        assert_eq!(editor(&app).component, id);
        crate::dialogs::cancel(&mut app);
        let occurrence = app.session.doc.occurrence_of(id).unwrap().id;
        menu(&mut app, Target::Occurrence { path: vec![occurrence] });
        assert_eq!(editor(&app).component, id);
        assert!(component_for_bodies(&app, &["missing".into()]).is_none());
        app.session.selection = vec![Sel::Body { name: body }, Sel::Plane { name: "XY".into() }];
        assert!(selected_component(&app).is_none());
        assert!(!context_menu::items(&app, &Target::Component { id: 0 }).iter().any(|i| i.id == "parts.resize"));
    }

    #[test]
    fn part_size_cancel_and_preview_do_not_change_the_design_or_history() {
        let (mut app, id) = part("hex_bolt", "M6", Some(20.0));
        let before = app.session.doc.clone();
        let undo = app.session.undo.len();
        menu(&mut app, Target::Component { id });
        change(&mut app, "M8", Some(30.0));
        let commands = crate::dialogs::apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
        let mut scratch = app.session.scratch();
        scratch.execute(&commands[0].0, &commands[0].1).unwrap();
        assert_eq!(scratch.doc.parts[&id].size, "M8");
        crate::dialogs::cancel(&mut app);
        assert_eq!(app.session.doc, before);
        assert_eq!(app.session.undo.len(), undo);
    }

    #[test]
    fn part_size_refuses_stale_documents_and_invalid_values() {
        let (mut app, id) = part("socket_head_cap_screw", "M5", Some(20.0));
        let mut k = Resize::new(&app, id).unwrap();
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 2001.0] {
            k.length = Some(bad);
            assert!(commands(&app, &k).is_err());
        }
        k.length = Some(20.0);
        k.size = usize::MAX;
        assert!(commands(&app, &k).is_err());
        let k = Resize::new(&app, id).unwrap();
        let (other, other_id) = part("hex_bolt", "M6", Some(20.0));
        assert_eq!(id, other_id, "same numeric id in another document");
        assert!(commands(&other, &k).is_err());
        app.run("document.units", json!({"units":"in"})).unwrap();
        assert!(commands(&app, &k).is_err());
    }

    #[test]
    fn part_size_round_trips_all_builtin_families_without_adding_a_length_to_rings() {
        for (family, size, length) in [
            ("socket_head_cap_screw", "M5", Some(20.0)),
            ("hex_bolt", "M5", Some(20.0)),
            ("hex_nut", "M5", None),
            ("washer", "M5", None),
            ("set_screw", "M5", Some(10.0)),
            ("dowel_pin", "6", Some(20.0)),
            ("bearing", "608", None),
        ] {
            let (app, id) = part(family, size, length);
            let k = Resize::new(&app, id).unwrap();
            assert_eq!(k.component, id);
            assert_eq!(k.family.sizes[k.size].0, size);
            assert_eq!(k.length, length);
            assert!(commands(&app, &k).unwrap().is_empty(), "confirming unchanged values does not rebuild");
        }
    }

    #[test]
    fn part_size_unchanged_ok_keeps_added_features_and_has_no_undo_step() {
        let (mut app, id) = part("hex_bolt", "M6", Some(20.0));
        app.run("component.activate", json!({"component":id})).unwrap();
        app.run("solid.box", json!({"length":2,"width":3,"height":4,"body_name":"Added lug"})).unwrap();
        let before = app.session.doc.clone();
        let undo = app.session.undo.len();
        menu(&mut app, Target::Component { id });
        apply(&mut app);
        assert_eq!(app.session.doc, before);
        assert_eq!(app.session.undo.len(), undo);
    }

    #[test]
    fn part_size_opens_accepted_size_aliases_without_rebuilding_unchanged_parts() {
        for (family, size, canonical, length) in [
            ("dowel_pin", "D6", "6", Some(20.0)),
            ("dowel_pin", "Ø6", "6", Some(20.0)),
            ("dowel_pin", "6.0", "6", Some(20.0)),
            ("bearing", " 608 ", "608", None),
        ] {
            let (mut app, id) = part(family, size, length);
            let before = app.session.doc.clone();
            let undo = app.session.undo.len();
            menu(&mut app, Target::Component { id });
            let k = editor(&app);
            assert_eq!(k.family.sizes[k.size].0, canonical);
            assert!(commands(&app, k).unwrap().is_empty());
            apply(&mut app);
            assert_eq!(app.session.doc, before);
            assert_eq!(app.session.undo.len(), undo);
            assert_eq!(app.session.doc.parts[&id].size, size);
        }
    }

    #[test]
    fn part_size_length_labels_use_design_units_without_rescaling_the_command() {
        let (mut app, id) = part("socket_head_cap_screw", "M5", Some(20.5));
        app.run("document.units", json!({"units":"in"})).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.home.open = false;
        menu(&mut h.app, Target::Component { id });
        h.frames(3);
        let label = crate::prefs::show_mm(&h.app, 20.5, 1);
        assert!(label.contains("in"), "a length in inches: {label}");
        assert!(h.widget_near(&label, "Length").is_some());
        change(&mut h.app, "M6", Some(20.5));
        let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        assert_eq!(commands[0].1["length"], 20.5);
    }

    #[test]
    fn part_size_real_widgets_change_size_length_and_confirm_once() {
        let (app, id) = part("socket_head_cap_screw", "M5", Some(20.5));
        let mut h = crate::scenario::Harness::new();
        h.app = app;
        h.app.home.open = false;
        h.frames(3);
        let undo = h.app.session.undo.len();
        menu(&mut h.app, Target::Component { id });
        h.frames(3);
        h.step(&json!({"widget":"M5","near":"Size"})).unwrap();
        h.step(&json!({"widget":"M6"})).unwrap();
        let current = crate::prefs::show_mm(&h.app, 20.5, 1);
        let shorter = crate::prefs::show_mm(&h.app, 10.0, 1);
        h.step(&json!({"widget":current,"near":"Length"})).unwrap();
        h.step(&json!({"widget":shorter})).unwrap();
        assert_eq!(editor(&h.app).length, Some(10.0));
        h.step(&json!({"widget":"OK"})).unwrap();
        for _ in 0..200 {
            h.frames(1);
            if h.app.dialog.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.app.dialog.is_none(), "the operation completed: {:?}", h.app.status);
        assert_eq!(h.app.session.doc.parts[&id].size, "M6");
        assert_eq!(h.app.session.doc.parts[&id].length, Some(10.0));
        assert_eq!(h.app.session.undo.len(), undo + 1);
    }
}
