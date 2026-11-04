//! Local topology tool lifecycle: preview, click to commit, Escape to cancel.
use super::*;

mod curve;

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    None,
    Edge,
    Points,
}
pub(super) struct State {
    curve: curve::State,
    mode: Mode,
    axis: usize,
    whole_loop: bool,
    counts: [usize; 2],
    chain: Vec<([usize; 2], f64, Vec3)>,
    preview: Option<(u64, usize, usize, bool, Vec<Vec<Vec3>>)>,
}
impl Default for State {
    fn default() -> Self {
        Self { curve: Default::default(), mode: Mode::None, axis: 0, whole_loop: false, counts: [1, 1], chain: Vec::new(), preview: None }
    }
}
pub(super) fn active(app: &SolveApp) -> bool {
    app.forms_prototype.tools.mode != Mode::None
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
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
                app.forms_prototype.tools.preview = None;
                clear_selection(app);
            }
        }
    });
    if app.forms_prototype.tools.mode == Mode::Edge
        && crate::ui_kit::button(ui, app.tr("forms.prototype.swap_direction"), crate::ui_kit::Button::Normal).clicked()
    {
        app.forms_prototype.tools.axis = 1 - app.forms_prototype.tools.axis;
    }
    if app.forms_prototype.tools.mode == Mode::Edge {
        let base = app.session.forms_prototype.as_ref().is_some_and(|f| f.hierarchy.is_none());
        if !base {
            app.forms_prototype.tools.whole_loop = false;
        }
        let label = app.tr("forms.prototype.whole_loop").to_owned();
        let r = ui.add_enabled_ui(base, |ui| crate::ui_kit::checkbox(ui, &label, &mut app.forms_prototype.tools.whole_loop)).inner;
        crate::scenario::publish_handle("forms_insert_whole_loop", r.rect.center());
    }
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
    if app.forms_prototype.tools.mode == Mode::Points {
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.commit_chain"), crate::ui_kit::Button::Normal);
        crate::scenario::publish_handle("forms_commit_chain", r.rect.center());
        if r.clicked() {
            commit_chain(app, ui.ctx());
        }
    }
    if active(app) {
        ui.add(egui::Label::new(app.tr("forms.prototype.insert_help")).wrap());
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
    if id == "form.prototype.insert_points" {
        app.forms_prototype.tools.chain.clear();
        app.forms_prototype.tools.curve = Default::default();
    }
    app.forms_prototype.tools.preview = None;
}
pub(super) fn pointer(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj, input: &crate::navigation::CameraInput) {
    curve::tick(app, ui.ctx());
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.forms_prototype.tools = State::default();
        return;
    }
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
    let Some(Element::Face(face)) = surface::nearest_with(app, proj, pos, Pick::Face) else { return };
    let Some(form) = &app.session.forms_prototype else { return };
    let key = (form.revision, face, app.forms_prototype.tools.axis, app.forms_prototype.tools.whole_loop);
    if app.forms_prototype.tools.preview.as_ref().is_none_or(|p| (p.0, p.1, p.2, p.3) != key) {
        let preview = (|| {
            if key.3 {
                let f = form.cage.faces.get(face).filter(|f| f.len() == 4)?;
                let seed = [*f.get(key.2)?, *f.get(key.2 + 1)?];
                let (cage, edges) = form.cage.insert_edge_ring(seed, 0.5).ok()?;
                let surface = cage.surface(cage.preview_level(form.levels).ok()?).ok()?;
                let keys: BTreeSet<_> = edges.into_iter().map(|e| [e[0].min(e[1]), e[0].max(e[1])]).collect();
                return Some(
                    surface
                        .boundaries
                        .into_iter()
                        .filter_map(|(e, p)| {
                            keys.contains(&[e[0].min(e[1]), e[0].max(e[1])]).then(|| p.into_iter().map(|p| form.world_point(p)).collect())
                        })
                        .collect(),
                );
            }
            let mut candidate = form.clone();
            candidate.refine(&[face], if key.2 == 0 { [2, 1] } else { [1, 2] }).ok()?;
            let h = candidate.hierarchy.as_ref()?;
            let ends = h.seam_endpoints(face, key.2).ok()?;
            let curve = candidate.surface.boundaries.into_iter().find(|(edge, _)| edge.contains(&ends[0]) && edge.contains(&ends[1]))?.1;
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
    if input.primary_pressed {
        if key.3 {
            let edge =
                app.session.forms_prototype.as_ref().and_then(|f| f.cage.faces.get(face)).and_then(|f| Some([*f.get(key.2)?, *f.get(key.2 + 1)?]));
            if let Some(edge) = edge {
                apply_edit(app, ui.ctx(), "form.prototype.insert_loop", json!({"edge":edge}));
            }
        } else {
            apply_edit(app, ui.ctx(), "form.prototype.insert_edge", json!({"face":face,"axis":key.2}));
        }
    }
    if ui.input(|i| i.pointer.secondary_clicked()) {
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
    fn insertion_click_limit_covers_ready_and_pending_points() {
        let (mut app, _, projection, a, _) = fixture();
        let point = curves::nearest(&app, &projection, a).unwrap();
        app.forms_prototype.tools.chain = vec![point; 32];
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        assert_eq!(app.forms_prototype.tools.chain.len(), 32);
        app.forms_prototype.tools.chain.truncate(31);
        let _pending = curves::pending_for_test(&mut app);
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        curve::point(&mut app, &egui::Context::default(), &projection, a, true);
        assert_eq!(app.forms_prototype.tools.chain.len(), 31);
        assert!(app.forms_prototype.pending.is_none());
    }
}
