//! Local topology tool lifecycle: preview, click to commit, Escape to cancel.
use super::*;

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    None,
    Edge,
    Points,
}
pub(super) struct State {
    mode: Mode,
    axis: usize,
    whole_loop: bool,
    counts: [usize; 2],
    chain: Vec<([usize; 2], f64, Vec3)>,
    preview: Option<(u64, usize, usize, bool, Vec<Vec<Vec3>>)>,
}
impl Default for State {
    fn default() -> Self {
        Self { mode: Mode::None, axis: 0, whole_loop: false, counts: [1, 1], chain: Vec::new(), preview: None }
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
            let r = ui.selectable_label(app.forms_prototype.tools.mode == mode, app.tr(key));
            crate::scenario::publish_handle(handle, r.rect.center());
            if r.clicked() {
                app.forms_prototype.tools.mode = if app.forms_prototype.tools.mode == mode { Mode::None } else { mode };
                app.forms_prototype.tools.chain.clear();
                app.forms_prototype.tools.preview = None;
                clear_selection(app);
            }
        }
    });
    if app.forms_prototype.tools.mode == Mode::Edge && ui.button(app.tr("forms.prototype.swap_direction")).clicked() {
        app.forms_prototype.tools.axis = 1 - app.forms_prototype.tools.axis;
    }
    if app.forms_prototype.tools.mode == Mode::Edge {
        let base = app.session.forms_prototype.as_ref().is_some_and(|f| f.hierarchy.is_none());
        if !base {
            app.forms_prototype.tools.whole_loop = false;
        }
        let label = app.tr("forms.prototype.whole_loop").to_owned();
        let r = ui.add_enabled(base, egui::Checkbox::new(&mut app.forms_prototype.tools.whole_loop, label));
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
    let r = ui.button(app.tr("forms.prototype.refine_selected"));
    crate::scenario::publish_handle("forms_refine_selected", r.rect.center());
    if r.clicked() {
        let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
        let counts = app.forms_prototype.tools.counts.map(|n| 1_usize << n.min(4));
        if run(app, "form.prototype.refine", json!({"faces":faces,"counts":counts})) {
            clear_selection(app);
        }
    }
    if app.forms_prototype.tools.mode == Mode::Points {
        let r = ui.button(app.tr("forms.prototype.commit_chain"));
        crate::scenario::publish_handle("forms_commit_chain", r.rect.center());
        if r.clicked() {
            commit_chain(app);
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
fn commit_chain(app: &mut SolveApp) {
    let points: Vec<_> = app.forms_prototype.tools.chain.iter().map(|(e, t, _)| json!({"edge":e,"fraction":t})).collect();
    if run(app, "form.prototype.insert_points", json!({"points":points})) {
        app.forms_prototype.tools.chain.clear();
        app.forms_prototype.tools.preview = None;
        clear_selection(app);
    }
}
fn edge_point(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<([usize; 2], f64, Vec3)> {
    let Element::Edge(edge) = surface::nearest_with(app, proj, pos, Pick::Edge)? else { return None };
    let form = app.session.forms_prototype.as_ref()?;
    let (_, curve) = form.surface.boundaries.iter().find(|(e, _)| *e == edge)?;
    let mut best = None;
    for (i, pair) in curve.windows(2).enumerate() {
        let a = proj.to_screen(pair[0])?;
        let b = proj.to_screen(pair[1])?;
        let ab = b - a;
        let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
        let d = pos.distance(a + ab * t);
        if best.as_ref().is_none_or(|(old, _, _)| d < *old) {
            best = Some((d, (i as f64 + f64::from(t)) / (curve.len() - 1) as f64, pair[0].lerp(pair[1], f64::from(t))));
        }
    }
    let (_, t, p) = best?;
    let (edge, t) = if edge[0] <= edge[1] { (edge, t) } else { ([edge[1], edge[0]], 1.0 - t) };
    Some((edge, t.clamp(0.001, 0.999), p))
}
pub(super) fn pointer(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj, input: &crate::navigation::CameraInput) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.forms_prototype.tools = State::default();
        return;
    }
    if ui.input(|i| i.key_pressed(egui::Key::M)) {
        app.forms_prototype.tools.axis = 1 - app.forms_prototype.tools.axis;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && app.forms_prototype.tools.mode == Mode::Points {
        commit_chain(app);
        return;
    }
    let Some(pos) = input.hover.filter(|_| input.inside && input.mode.is_none()) else { return };
    if app.forms_prototype.tools.mode == Mode::Points {
        let point = edge_point(app, proj, pos);
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
        if input.primary_pressed
            && let Some(point) = point
        {
            app.forms_prototype.tools.chain.push(point);
        }
        if ui.input(|i| i.pointer.secondary_clicked()) {
            if app.forms_prototype.tools.chain.is_empty() {
                app.forms_prototype.tools.mode = Mode::None;
            } else {
                commit_chain(app);
            }
        }
        return;
    }
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
        let applied = if key.3 {
            let edge =
                app.session.forms_prototype.as_ref().and_then(|f| f.cage.faces.get(face)).and_then(|f| Some([*f.get(key.2)?, *f.get(key.2 + 1)?]));
            edge.is_some_and(|edge| run(app, "form.prototype.insert_loop", json!({"edge":edge})))
        } else {
            run(app, "form.prototype.insert_edge", json!({"face":face,"axis":key.2}))
        };
        if applied {
            clear_selection(app);
            app.forms_prototype.tools.preview = None;
        }
    }
    if ui.input(|i| i.pointer.secondary_clicked()) {
        app.forms_prototype.tools.mode = Mode::None;
    }
}
