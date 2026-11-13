//! Local topology tool lifecycle: preview, click to commit, Escape to cancel.
use super::*;

mod curve;
mod loop_preview;

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    None,
    Edge,
    Points,
}
pub(super) struct State {
    curve: curve::State,
    loop_preview: loop_preview::State,
    mode: Mode,
    axis: usize,
    whole_loop: bool,
    counts: [usize; 2],
    chain: Vec<([usize; 2], f64, Vec3)>,
    preview: Option<(u64, usize, usize, bool, Vec<Vec<Vec3>>)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            curve: Default::default(),
            loop_preview: Default::default(),
            mode: Mode::None,
            axis: 0,
            whole_loop: false,
            counts: [1, 1],
            chain: Vec::new(),
            preview: None,
        }
    }
}
pub(super) fn active(app: &SolveApp) -> bool {
    app.forms_prototype.tools.mode != Mode::None
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct EdgePick {
    axis: usize,
    whole_loop: bool,
}

pub(super) fn edge_pick(app: &SolveApp) -> Option<EdgePick> {
    let state = &app.forms_prototype.tools;
    (state.mode == Mode::Edge).then_some(EdgePick { axis: state.axis, whole_loop: state.whole_loop })
}

pub(super) fn commit_face(app: &mut SolveApp, ctx: &egui::Context, face: usize, key: EdgePick) {
    if edge_pick(app) != Some(key) {
        return;
    }
    if key.whole_loop && loop_preview::commit(app, ctx, face, key.axis) {
        return;
    }
    if key.whole_loop && app.session.forms_prototype.as_ref().is_some_and(|form| form.hierarchy.is_none()) {
        let edge = app
            .session
            .forms_prototype
            .as_ref()
            .and_then(|form| form.logical.faces.get(face))
            .and_then(|f| Some([*f.get(key.axis)?, *f.get(key.axis + 1)?]));
        if let Some(edge) = edge {
            apply_edit(app, ctx, "form.prototype.insert_loop", json!({"edge":edge}));
        }
    } else if !key.whole_loop {
        apply_edit(app, ctx, "form.prototype.insert_edge", json!({"face":face,"axis":key.axis}));
    }
}
pub(super) fn show_switches(app: &mut SolveApp, ui: &mut egui::Ui, compact: bool) {
    ui.horizontal_wrapped(|ui| {
        for (mode, key, handle) in
            [(Mode::Edge, "forms.prototype.insert_edge", "forms_insert_edge"), (Mode::Points, "forms.prototype.insert_points", "forms_insert_points")]
        {
            let r = crate::ui_kit::button(
                ui,
                app.tr(key),
                if app.forms_prototype.tools.mode == mode { crate::ui_kit::Button::SmallSelected } else { crate::ui_kit::Button::Small },
            );
            crate::scenario::publish_handle(handle, r.rect.center());
            if r.clicked() {
                app.forms_prototype.tools.mode = if app.forms_prototype.tools.mode == mode { Mode::None } else { mode };
                app.forms_prototype.tools.chain.clear();
                app.forms_prototype.tools.curve = Default::default();
                loop_preview::cancel(app);
                clear_selection(app);
            }
        }
        if compact && active(app) {
            let response = crate::ui_kit::button(ui, app.tr("forms.prototype.cancel"), crate::ui_kit::Button::Small);
            crate::scenario::publish_handle("forms_cancel_insertion", response.rect.center());
            if response.clicked() {
                cancel(app);
            }
        }
    });
}

pub(super) fn show_options(app: &mut SolveApp, ui: &mut egui::Ui, compact: bool) {
    loop_preview::poll(app);
    if app.forms_prototype.tools.mode == Mode::Edge
        && crate::ui_kit::button(ui, app.tr("forms.prototype.swap_direction"), crate::ui_kit::Button::Normal).clicked()
    {
        app.forms_prototype.tools.axis = 1 - app.forms_prototype.tools.axis;
    }
    if app.forms_prototype.tools.mode == Mode::Edge {
        let unavailable = loop_preview::unavailable(app);
        let label = app.tr("forms.prototype.whole_loop").to_owned();
        let can_change = unavailable.is_none() || app.forms_prototype.tools.whole_loop;
        let r = ui.add_enabled_ui(can_change, |ui| crate::ui_kit::checkbox(ui, &label, &mut app.forms_prototype.tools.whole_loop)).inner;
        crate::scenario::publish_handle("forms_insert_whole_loop", r.rect.center());
        if r.changed() {
            loop_preview::cancel(app);
        }
        if let Some(reason) = unavailable {
            ui.add(egui::Label::new(app.tr(reason)).wrap());
        }
        loop_preview::show_status(app, ui);
    }
    if !compact || !active(app) {
        show_refinement(app, ui);
    }
    if app.forms_prototype.tools.mode == Mode::Points {
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.commit_chain"), crate::ui_kit::Button::Normal);
        crate::scenario::publish_handle("forms_commit_chain", r.rect.center());
        if r.clicked() {
            commit_chain(app, ui.ctx());
        }
    }
    if active(app) {
        let help = match (compact, app.forms_prototype.tools.mode) {
            (true, Mode::Edge) => "forms.prototype.insert_edge_help",
            (true, Mode::Points) => "forms.prototype.insert_points_help",
            _ => "forms.prototype.insert_help",
        };
        ui.add(egui::Label::new(app.tr(help)).wrap());
    }
}
fn show_refinement(app: &mut SolveApp, ui: &mut egui::Ui) {
    let labels: [String; 2] = ["forms.prototype.u", "forms.prototype.v"].map(|s| app.tr(s).to_owned());
    let options = ["1", "2", "4", "8", "16"];
    ui.columns(2, |columns| {
        for (i, ui) in columns.iter_mut().enumerate() {
            crate::ui_kit::choice_row(
                ui,
                &labels[i],
                if i == 0 { "form_refine_u" } else { "form_refine_v" },
                &mut app.forms_prototype.tools.counts[i],
                &options,
            );
        }
    });
    let r = crate::ui_kit::button(ui, app.tr("forms.prototype.refine_selected"), crate::ui_kit::Button::Normal);
    crate::scenario::publish_handle("forms_refine_selected", r.rect.center());
    if r.clicked() {
        let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
        let counts = app.forms_prototype.tools.counts.map(|n| 1_usize << n.min(4));
        apply_edit(app, ui.ctx(), "form.prototype.refine", json!({"faces":faces,"counts":counts}));
    }
}
fn clear_selection(app: &mut SolveApp) {
    app.forms_prototype.selected.clear();
    app.forms_prototype.elements.clear();
    app.forms_prototype.pivot = None;
}
fn commit_chain(app: &mut SolveApp, ctx: &egui::Context) {
    if curve::defer_commit(app) {
        return;
    }
    let points: Vec<_> = app.forms_prototype.tools.chain.iter().map(|(e, t, _)| json!({"edge":e,"fraction":t})).collect();
    if let Some(tolerance) = controls::tolerance(app) {
        apply_edit(app, ctx, "form.prototype.insert_points", json!({"points":points,"tolerance":tolerance}));
    }
}
fn apply_edit(app: &mut SolveApp, ctx: &egui::Context, id: &'static str, params: serde_json::Value) {
    if id == "form.prototype.insert_points" || app.session.forms_prototype.as_ref().is_some_and(|f| f.matching.is_some()) {
        finish::command(app, ctx, id, params);
    } else if run(app, id, params) {
        completed(app, id);
    }
}
pub(super) fn completed(app: &mut SolveApp, id: &str) {
    loop_preview::cancel(app);
    if id == "form.prototype.insert_points" {
        app.forms_prototype.tools.chain.clear();
        app.forms_prototype.tools.curve = Default::default();
    }
    app.forms_prototype.tools.preview = None;
}
fn cancel(app: &mut SolveApp) {
    loop_preview::cancel(app);
    face_press::cancel(app);
    let loop_preview = std::mem::take(&mut app.forms_prototype.tools.loop_preview);
    app.forms_prototype.tools = State { loop_preview, ..Default::default() };
}
pub(super) fn pointer(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj, input: &crate::navigation::CameraInput) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel(app);
        return;
    }
    curve::tick(app, ui.ctx());
    if ui.input(|i| i.key_pressed(egui::Key::M)) {
        app.forms_prototype.tools.axis = 1 - app.forms_prototype.tools.axis;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && app.forms_prototype.tools.mode == Mode::Points {
        commit_chain(app, ui.ctx());
        return;
    }
    let pos = input.hover.filter(|_| input.inside && input.mode.is_none());
    if app.forms_prototype.tools.mode == Mode::Points {
        let point = pos.and_then(|pos| curve::point(app, ui.ctx(), proj, pos, input.primary_pressed));
        let tokens = Tokens::get();
        let mut points: Vec<_> = app.forms_prototype.tools.chain.iter().map(|p| p.2).collect();
        if let Some((_, _, p)) = point {
            points.push(p);
        }
        for &p in &points {
            if let Some(p) = proj.to_screen(p) {
                ui.painter().circle_filled(p, 5.0, tokens.accent);
            }
        }
        for pair in points.windows(2) {
            if let (Some(a), Some(b)) = (proj.to_screen(pair[0]), proj.to_screen(pair[1])) {
                ui.painter().line_segment([a, b], Stroke::new(2.0, tokens.accent));
            }
        }
        // Every click, including an already cached one, is consumed by the FIFO.
        if pos.is_some() && ui.input(|i| i.pointer.secondary_clicked()) {
            if curve::empty(app) {
                app.forms_prototype.tools.mode = Mode::None;
            } else {
                commit_chain(app, ui.ctx());
            }
        }
        return;
    }
    let Some(pos) = pos else { return };
    let secondary = ui.input(|i| i.pointer.secondary_clicked());
    if secondary && face_press::enabled(app) {
        loop_preview::cancel(app);
        face_press::cancel(app);
        app.forms_prototype.tools.mode = Mode::None;
        return;
    }
    let exact_press = input.primary_pressed && face_press::insert(app, ui, proj, input);
    let Some(Element::Face(face)) = surface::nearest_with(app, proj, pos, Pick::Face) else { return };
    let Some(form) = &app.session.forms_prototype else { return };
    let key = (form.revision, face, app.forms_prototype.tools.axis, app.forms_prototype.tools.whole_loop);
    if key.3 {
        loop_preview::hover(app, ui.ctx(), face, key.2);
    } else if app.forms_prototype.tools.preview.as_ref().is_none_or(|p| (p.0, p.1, p.2, p.3) != key) {
        let preview = (|| {
            let mut candidate = form.clone();
            candidate.refine(&[face], if key.2 == 0 { [2, 1] } else { [1, 2] }).ok()?;
            let h = candidate.hierarchy.as_ref()?;
            let ends = h.seam_endpoints(face, key.2).ok()?;
            let (_, curve) = candidate.surface.boundaries.iter().find(|(edge, _)| edge.contains(&ends[0]) && edge.contains(&ends[1]))?;
            let mut curves = vec![curve.clone()];
            if let Some(symmetry) = &form.symmetry {
                let reflected: Vec<_> = curve.iter().map(|p| form.world_point(symmetry.reflect(form.placement.local_point(*p)))).collect();
                if curve.iter().zip(&reflected).any(|(a, b)| a.dist(*b) > 1e-9) {
                    curves.push(reflected);
                }
            }
            Some(curves)
        })()
        .unwrap_or_default();
        app.forms_prototype.tools.preview = Some((key.0, key.1, key.2, key.3, preview));
    }
    if let Some((_, _, _, _, curves)) = &app.forms_prototype.tools.preview {
        for pair in curves.iter().flat_map(|c| c.windows(2)) {
            if surface::visible(app, proj, (pair[0] + pair[1]) * 0.5)
                && let (Some(a), Some(b)) = (proj.to_screen(pair[0]), proj.to_screen(pair[1]))
            {
                ui.painter().line_segment([a, b], Stroke::new(2.5, Tokens::get().accent));
            }
        }
    }
    if input.primary_pressed
        && !exact_press
        && let Some(key) = edge_pick(app)
    {
        commit_face(app, ui.ctx(), face, key);
    }
    if secondary {
        loop_preview::cancel(app);
        app.forms_prototype.tools.mode = Mode::None;
    }
}

#[cfg(test)]
pub(super) fn queue_point_for_test(app: &mut SolveApp, projection: Proj, position: Pos2) {
    curve::queue_for_test(app, projection, position);
}
#[cfg(test)]
pub(super) fn commit_points_for_test(app: &mut SolveApp, ctx: &egui::Context) {
    commit_chain(app, ctx);
}
#[cfg(test)]
pub(super) fn tick_points_for_test(app: &mut SolveApp, ctx: &egui::Context) {
    curve::tick(app, ctx);
}

#[cfg(test)]
pub(super) fn point_count_for_test(app: &SolveApp) -> usize {
    app.forms_prototype.tools.chain.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_loop_reports_its_trim_wall_restriction_without_editing_the_form() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"face","dimensions":[20,20,1],"segments":[4,4,1],"levels":1})).unwrap();
        app.run("form.prototype.refine", json!({"faces":[5],"counts":[2,2]})).unwrap();
        let old = app.session.forms_prototype.as_ref().unwrap().logical.faces[16].clone();
        app.run("form.prototype.topology", json!({"tool":"delete_faces","faces":[16]})).unwrap();
        let hierarchy = app.session.forms_prototype.as_ref().unwrap().hierarchy.as_ref().unwrap();
        let edge = old
            .iter()
            .copied()
            .zip(old.iter().copied().cycle().skip(1))
            .take(old.len())
            .find_map(|(a, b)| hierarchy.trim_attachments(&[[a, b]]).ok()?.into_iter().find(|t| t.root == 5).map(|t| t.edge))
            .unwrap();
        app.run("form.prototype.extrude_drag", json!({"edges":[edge],"displacement":[0,0,2]})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.hierarchy.as_ref().unwrap().has_trim_wall());
        let before = form.definition();
        app.forms_prototype.tools.mode = Mode::Edge;
        app.forms_prototype.tools.whole_loop = true;
        let key = edge_pick(&app).unwrap();
        commit_face(&mut app, &egui::Context::default(), 0, key);
        let (message, _, error) = app.status.as_ref().unwrap();
        assert!(*error);
        assert_eq!(message, app.tr("forms.insert.whole_loop_trim_wall"));
        assert!(app.forms_prototype.pending.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn base_loop_hover_reuses_the_prepared_insert_transaction() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"face","segments":[1,1,1],"size":10,"levels":0})).unwrap();
        app.forms_prototype.tools.mode = Mode::Edge;
        app.forms_prototype.tools.whole_loop = true;
        let original = app.session.forms_prototype.as_ref().unwrap().definition();
        let (geometry, expected) = loop_preview::install_for_test(&mut app, 0, 0);
        assert_eq!(geometry.unwrap().len(), 1);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);

        let ctx = egui::Context::default();
        assert!(loop_preview::commit(&mut app, &ctx, 0, 0));
        finish::flush_for_test(&mut app).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), expected);
    }

    fn hierarchical_hover_fixture() -> (SolveApp, usize) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"size":10,"levels":0})).unwrap();
        app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let f = app.session.forms_prototype.as_ref().unwrap();
        let vertex = *f.hierarchy.as_ref().unwrap().controls.iter().find(|(_, level)| **level == 1).unwrap().0;
        let p = f.cage.points[vertex] + Vec3::new(0.02, -0.01, 0.03);
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[vertex],"positions":[[p.x,p.y,p.z]]})).unwrap();
        let face = app.session.forms_prototype.as_ref().unwrap().logical.faces.iter().position(|face| !face.is_empty()).unwrap();
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        app.forms_prototype.tools.mode = Mode::Edge;
        app.forms_prototype.tools.whole_loop = true;
        (app, face)
    }

    #[test]
    fn hierarchical_loop_hover_draws_the_qualified_candidate_and_click_reuses_its_one_checkpoint() {
        let (mut app, face) = hierarchical_hover_fixture();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let old_undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap();
        let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let (geometry, expected) = loop_preview::install_for_test(&mut app, face, 0);
        assert!(!geometry.as_ref().unwrap().is_empty());
        loop_preview::hover(&mut app, &egui::Context::default(), face, 0);
        assert_eq!(&app.forms_prototype.tools.preview.as_ref().unwrap().4, geometry.as_ref().unwrap());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
        loop_preview::commit(&mut app, &egui::Context::default(), face, 0);
        assert!(app.forms_prototype.pending.is_some(), "the existing command worker handoff owns acceptance");
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        finish::tick(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), expected);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].as_u64().unwrap(), old_undo + 1);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn mixed_depth_hover_previews_a_seam_between_two_existing_t_controls() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[12,4,1],"segments":[3,1,1],"levels":0})).unwrap();
        app.session.execute("form.prototype.refine", &json!({"faces":[0,2],"counts":[2,2]})).unwrap();
        app.session.param_check = solidcraft_engine::schema::ParamCheck::Strict;
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        app.forms_prototype.tools.mode = Mode::Edge;
        app.forms_prototype.tools.axis = 1;
        app.forms_prototype.tools.whole_loop = true;
        let form = app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let old_undo = form.inspect()["undo"].as_u64().unwrap();
        // In this 12 x 4 strip the coarse middle root has both transverse
        // sides split at Y=0 by the two refined outer roots. The entire loop
        // has existing stations, and only the middle seam is newly inserted.
        let ends = [-2.0, 2.0].map(|x| *form.logical.faces[1].iter().find(|&&id| form.cage.points[id].dist(Vec3::new(x, 0.0, 0.0)) < 1e-12).unwrap());
        let endpoints = ends.map(|id| form.surface.vertices[id]);
        assert_eq!(form.logical.faces[1].len(), 6);
        let controls = &before.hierarchy.as_ref().unwrap().controls;
        assert!(ends.iter().all(|id| controls.contains_key(id)));

        let (geometry, expected) = loop_preview::install_for_test(&mut app, 1, 1);
        let curves = geometry.unwrap();
        let [curve] = curves.as_slice() else { panic!("only the new middle seam should be previewed") };
        let first = *curve.first().unwrap();
        let last = *curve.last().unwrap();
        assert!(
            first.dist(endpoints[0]) < 1e-10 && last.dist(endpoints[1]) < 1e-10
                || first.dist(endpoints[1]) < 1e-10 && last.dist(endpoints[0]) < 1e-10
        );
        assert_eq!(&expected.hierarchy.as_ref().unwrap().controls, controls, "no new station IDs are needed for the middle seam");
        let ctx = egui::Context::default();
        loop_preview::hover(&mut app, &ctx, 1, 1);
        assert_eq!(app.forms_prototype.tools.preview.as_ref().unwrap().4, curves);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        loop_preview::commit(&mut app, &ctx, 1, 1);
        finish::tick(&mut app);
        let after = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(after.definition(), expected);
        assert_eq!(after.inspect()["undo"].as_u64().unwrap(), old_undo + 1);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        app.session.execute("form.prototype.redo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), expected);
    }

    #[test]
    fn hierarchical_loop_click_can_handoff_an_unpolled_worker_without_applying_preview_state() {
        let (mut app, face) = hierarchical_hover_fixture();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let (_, expected) = loop_preview::install_for_test(&mut app, face, 0);
        assert!(app.forms_prototype.tools.preview.is_none());
        loop_preview::commit(&mut app, &egui::Context::default(), face, 0);
        assert!(app.forms_prototype.pending.is_some());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        finish::tick(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), expected);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn hierarchical_loop_hover_waits_for_preview_capacity_and_preserves_the_click() {
        let (mut app, face) = hierarchical_hover_fixture();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let hold = loop_preview::occupy_worker_for_test(&app).unwrap();
        let ctx = egui::Context::default();
        loop_preview::hover(&mut app, &ctx, face, 0);
        assert!(!loop_preview::has_pending_for_test(&app));
        assert!(app.forms_prototype.tools.preview.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        drop(hold);
        loop_preview::hover(&mut app, &ctx, face, 0);
        assert!(loop_preview::has_pending_for_test(&app));
        loop_preview::commit(&mut app, &ctx, face, 0);
        finish::flush_for_test(&mut app).unwrap();
        assert_ne!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
    }

    #[test]
    fn hierarchical_loop_cancel_drains_one_worker_and_never_authors_or_accepts_its_result() {
        let (mut app, face) = hierarchical_hover_fixture();
        let before = app.session.forms_prototype.as_ref().unwrap().inspect();
        let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let _ = loop_preview::install_for_test(&mut app, face, 0);
        loop_preview::cancel(&mut app);
        assert!(loop_preview::has_pending_for_test(&app), "cancellation retains a running worker until its result drains");
        loop_preview::poll(&mut app);
        assert!(!loop_preview::has_pending_for_test(&app));
        assert!(!loop_preview::has_ready_for_test(&app));
        assert!(app.forms_prototype.tools.preview.is_none() && app.forms_prototype.pending.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
    }

    #[test]
    fn hierarchical_loop_worker_feedback_is_discarded_after_source_or_document_changes() {
        for new_document in [false, true] {
            let (mut app, face) = hierarchical_hover_fixture();
            let _ = loop_preview::install_for_test(&mut app, face, 0);
            if new_document {
                app.session = solidcraft_engine::Session::default();
            } else {
                let p = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::new(0.1, 0.0, 0.0);
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z]]})).unwrap();
            }
            let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
            loop_preview::poll(&mut app);
            assert!(!loop_preview::has_ready_for_test(&app));
            assert!(app.forms_prototype.tools.preview.is_none() && app.forms_prototype.pending.is_none());
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
        }
    }

    #[test]
    fn hierarchical_loop_prepared_click_keeps_full_source_qualification_after_handoff() {
        for geometry in [false, true] {
            let (mut app, face) = hierarchical_hover_fixture();
            let _ = loop_preview::install_for_test(&mut app, face, 0);
            loop_preview::commit(&mut app, &egui::Context::default(), face, 0);
            if geometry {
                let p = app.session.forms_prototype.as_ref().unwrap().cage.points[0] + Vec3::new(0.1, 0.0, 0.0);
                app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z]]})).unwrap();
            } else {
                // Finish options leave cage/revision/feature unchanged. Only
                // the full transaction source check can reject this result.
                app.session.execute("form.prototype.finish_options", &json!({"tolerance":"0.02 mm"})).unwrap();
            }
            let before = app.session.forms_prototype.as_ref().unwrap().inspect();
            let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
            finish::tick(&mut app);
            assert!(app.forms_prototype.pending.is_none());
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), before);
            assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
        }
    }

    fn fixture() -> (SolveApp, Vec<solidcraft_engine::doc::forms::EdgeCurve>, Proj, Pos2, Pos2) {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[10,10,1],"segments":[1,1,1],"levels":0})).unwrap();
        app.forms_prototype.controls.tolerance = "0.01 mm".into();
        let face = app.session.forms_prototype.as_ref().unwrap().cage.faces[0].clone();
        let a = [face[0].min(face[1]), face[0].max(face[1])];
        let b = [face[2].min(face[3]), face[2].max(face[3])];
        let curves = solidcraft_engine::forms_prototype::insertion_curves(&app.session, 0.01, &[]).unwrap();
        fit(&mut app);
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let first = projection.to_screen(curves.iter().find(|curve| curve.edge == a).unwrap().at(0.3).unwrap()).unwrap();
        let second = projection.to_screen(curves.iter().find(|curve| curve.edge == b).unwrap().at(0.65).unwrap()).unwrap();
        curves::install_for_test(&mut app, Ok(curves.clone()));
        (app, curves, projection, first, second)
    }
    #[test]
    fn insertion_click_fifo_keeps_cached_a_cold_b_then_a_in_actual_click_order() {
        let (mut app, descriptors, projection, a, b) = fixture();
        let original = app.session.forms_prototype.as_ref().unwrap().inspect();
        let recovery = solidcraft_engine::recovery::capture_bytes(&app.session).unwrap();
        let expected_a = curves::nearest(&app, &projection, a).unwrap();
        let expected_b = curves::nearest(&app, &projection, b).unwrap();
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        assert_eq!(app.forms_prototype.tools.chain.len(), 1);
        let pending = curves::pending_for_test(&mut app);
        curve::point(&mut app, &egui::Context::default(), &projection, b, true);
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        assert_eq!(app.forms_prototype.tools.chain.len(), 1, "a later cached/ready click cannot overtake an earlier pending click");
        pending.send(Ok(descriptors)).unwrap();
        curve::tick(&mut app, &egui::Context::default());
        let chain = &app.forms_prototype.tools.chain;
        assert_eq!(chain.iter().map(|point| point.0).collect::<Vec<_>>(), vec![expected_a.0, expected_b.0, expected_a.0]);
        for (actual, expected) in chain.iter().zip([expected_a, expected_b, expected_a]) {
            assert!(actual.2.dist(expected.2) < 1e-8);
        }
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), original);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&app.session).unwrap(), recovery);
    }
    #[test]
    fn deferred_insertion_commit_is_cancelled_by_failed_stale_or_cancelled_prefix() {
        for mode in [0, 1, 2, 3] {
            let (mut app, descriptors, projection, a, b) = fixture();
            let original = app.session.forms_prototype.as_ref().unwrap().inspect();
            let pending = curves::pending_for_test(&mut app);
            curve::queue_for_test(&mut app, projection, a);
            curve::queue_for_test(&mut app, projection, b);
            assert!(curve::defer_commit(&mut app));
            if mode == 0 {
                pending.send(Err("exact curve preparation failed".into())).unwrap();
            } else if mode == 1 {
                app.session.execute("form.prototype.quality", &json!({"levels":3})).unwrap();
                pending.send(Ok(descriptors)).unwrap();
            } else if mode == 2 {
                app.forms_prototype.tools = State::default();
                pending.send(Ok(descriptors)).unwrap();
            } else {
                app.forms_prototype.tools = State::default();
                curve::queue_for_test(&mut app, projection, a + egui::vec2(4000.0, 4000.0));
                curve::queue_for_test(&mut app, projection, b);
                assert!(curve::defer_commit(&mut app));
                pending.send(Ok(descriptors)).unwrap();
            }
            curve::tick(&mut app, &egui::Context::default());
            assert!(app.forms_prototype.tools.chain.is_empty());
            assert!(app.forms_prototype.pending.is_none(), "failed prefix must cancel deferred Enter instead of committing a surviving suffix");
            if mode != 1 {
                assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), original);
            }
        }
    }
    #[test]
    fn escape_discards_a_deferred_insert_when_curves_become_ready() {
        let (mut app, descriptors, proj, a, b) = fixture();
        let original = app.session.forms_prototype.as_ref().unwrap().inspect();
        let pending = curves::pending_for_test(&mut app);
        app.forms_prototype.tools.mode = Mode::Points;
        curve::queue_for_test(&mut app, proj, a);
        curve::queue_for_test(&mut app, proj, b);
        assert!(curve::defer_commit(&mut app));
        pending.send(Ok(descriptors)).unwrap();
        let ctx = egui::Context::default();
        let raw =
            egui::RawInput { screen_rect: Some(proj.rect), events: crate::test_support::key(egui::Key::Escape), focused: true, ..Default::default() };
        let _ = crate::test_support::frame(&ctx, raw, |ui| {
            let response = ui.interact(proj.rect, ui.scope_id().with("form_insert_escape"), egui::Sense::click());
            let input = crate::navigation::handle(&mut app, ui, proj.rect, &response, crate::navigation::NavCaps::MODEL);
            pointer(&mut app, ui, &proj, &input);
        });
        assert!(curve::empty(&app));
        assert!(!active(&app));
        assert!(app.forms_prototype.pending.is_none());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect(), original);
    }
    #[test]
    fn insertion_click_limit_covers_ready_and_pending_points() {
        let (mut app, descriptors, projection, a, _) = fixture();
        let point = curves::nearest(&app, &projection, a).unwrap();
        app.forms_prototype.tools.chain = vec![point; 32];
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        assert_eq!(app.forms_prototype.tools.chain.len(), 32);
        app.forms_prototype.tools.chain.truncate(31);
        let pending = curves::pending_for_test(&mut app);
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        assert_eq!(app.forms_prototype.tools.chain.len(), 31);
        assert!(app.forms_prototype.pending.is_none());
        pending.send(Ok(descriptors)).unwrap();
        curve::tick(&mut app, &egui::Context::default());
        assert_eq!(app.forms_prototype.tools.chain.len(), 32, "the rejected click leaves the accepted pending click in the queue");
    }
}
