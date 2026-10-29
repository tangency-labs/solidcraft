//! Pipe source selection and bounded expression fields for the shared modal preview.
use super::*;
use solidcraft_engine::doc::forms::PipeSource;
use solidcraft_engine::geom::subdivision::pipe_network::PipeSettings;

pub(super) struct Inputs {
    source_kind: usize,
    sketches: Vec<(u64, String)>,
    sketch: usize,
    curves: Vec<String>,
    pub(super) diameter: String,
    density: String,
    samples: String,
    branch: usize,
    pub(super) overrides: std::collections::BTreeMap<String, [String; 2]>,
    pub(super) all_edges: bool,
    was_sculpted: bool,
    regenerate: bool,
    source_changed: bool,
}
impl Inputs {
    pub(super) fn new(app: &SolveApp) -> Self {
        let form = app.session.forms_prototype.as_ref();
        let before = form.and_then(|f| f.editing_feature).map_or_else(|| app.session.model.state(), |id| app.session.model.state_before(id));
        let sketches: Vec<_> = before
            .sketches
            .iter()
            .filter(|s| !s.sketch.curves.is_empty() || s.sketch.wires.iter().any(|w| w.pts.len() > 1))
            .take(512)
            .map(|s| (s.feature, s.name.clone()))
            .collect();
        let pipe = form.and_then(|f| f.pipe.as_deref());
        let mut settings = pipe.map_or_else(PipeSettings::default, |p| p.settings.clone());
        let mut source_kind = usize::from(sketches.is_empty());
        if pipe.is_none()
            && source_kind == 1
            && let Some(body) = before.bodies.iter().find(|b| !b.body.is_mesh())
        {
            settings.diameter = (body.body.size() * 0.05).clamp(0.1, 10.0);
        }
        let mut sketch = 0;
        let mut curves = Vec::new();
        if let Some(pipe) = pipe {
            match &pipe.source {
                PipeSource::Sketch { sketch: id, curves: ids } => {
                    source_kind = 0;
                    sketch = sketches.iter().position(|(n, _)| *n == id.0).unwrap_or(0);
                    curves = ids.clone();
                }
                PipeSource::Edges { .. } => source_kind = 1,
            }
        }
        Self {
            source_kind,
            sketches,
            sketch,
            curves,
            diameter: settings.diameter.to_string(),
            density: settings.section_density.to_string(),
            samples: settings.path_samples.to_string(),
            branch: 0,
            overrides: settings
                .overrides
                .iter()
                .map(|(key, v)| (key.clone(), [v.diameter.unwrap_or(0.0).to_string(), v.samples.unwrap_or(0).to_string()]))
                .collect(),
            all_edges: true,
            was_sculpted: pipe.is_some_and(|p| !p.follow_path),
            regenerate: false,
            source_changed: false,
        }
    }
}
pub(super) fn select_source(app: &SolveApp, state: &mut State, world: Option<&solidcraft_engine::doc::ModelState>) {
    if let Some(PipeSource::Edges { body, edges }) = app.session.forms_prototype.as_ref().and_then(|f| f.pipe.as_ref()).map(|p| &p.source) {
        state.support = state.supports.iter().position(|(name, _)| name == body.as_ref()).unwrap_or(0);
        if let Some(model) = world.and_then(|w| w.body(body)) {
            state.support_edges =
                edges.iter().filter_map(|e| solidcraft_engine::doc::naming::locate_edge(model, &e.name, e.pick).ok().map(|hit| hit.edge)).collect();
        }
        if let Some(pipe) = &mut state.pipe {
            pipe.all_edges = false;
        }
    }
}
pub(super) fn has_sources(state: &State) -> bool {
    state.pipe.as_ref().is_some_and(|p| !p.sketches.is_empty() || !state.supports.is_empty())
}
pub(super) fn cad(state: &State) -> bool {
    state.pipe.as_ref().is_some_and(|p| p.source_kind == 1)
}
pub(super) fn picked(state: &mut State) {
    if let Some(p) = &mut state.pipe {
        p.source_changed = true;
        p.all_edges = false;
        p.overrides.clear();
    }
}
pub(super) fn highlight(state: &State, edge: usize) -> bool {
    state.pipe.as_ref().is_some_and(|p| p.all_edges || state.support_edges.contains(&edge))
}
fn number(app: &SolveApp, text: &str, kind: solidcraft_engine::doc::expr::Kind) -> Result<f64, String> {
    controls::number(app, text, kind).ok_or_else(|| app.tr("forms.prototype.invalid_number").to_owned())
}
fn integer(app: &SolveApp, text: &str, min: usize, max: usize) -> Result<usize, String> {
    let n = number(app, text, solidcraft_engine::doc::expr::Kind::Unitless)?;
    if n.fract() != 0.0 || n < min as f64 || n > max as f64 {
        return Err(app.tr("forms.prototype.invalid_number").to_owned());
    }
    Ok(n as usize)
}
pub(super) fn params(app: &SolveApp, state: &State) -> Result<serde_json::Value, String> {
    use solidcraft_engine::doc::expr::Kind;
    let p = state.pipe.as_ref().ok_or_else(|| app.tr("forms.prototype.pipe_source_required").to_owned())?;
    let diameter = number(app, &p.diameter, Kind::Length)?;
    if !(0.1..=10_000.0).contains(&diameter) {
        return Err(app.tr("forms.prototype.invalid_number").to_owned());
    }
    let density = integer(app, &p.density, 1, 12)?;
    let samples = integer(app, &p.samples, 1, 20)?;
    let mut overrides = Vec::new();
    for (key, fields) in &p.overrides {
        let d = number(app, &fields[0], Kind::Length)?;
        let n = integer(app, &fields[1], 0, 20)?;
        if d != 0.0 && !(0.1..=10_000.0).contains(&d) {
            return Err(app.tr("forms.prototype.invalid_number").to_owned());
        }
        if d != 0.0 || n != 0 {
            overrides.push(json!({"segment":key,"diameter":d,"samples":n}));
        }
    }
    let mut given = json!({"diameter":diameter,"section_density":density,"path_samples":samples,"overrides":overrides,"regenerate":p.regenerate});
    if !(p.was_sculpted && !p.regenerate && !p.source_changed) {
        if p.source_kind == 0 {
            let (id, _) = p.sketches.get(p.sketch).ok_or_else(|| app.tr("forms.prototype.pipe_source_required").to_owned())?;
            given["sketch"] = json!(id);
            if !p.curves.is_empty() {
                given["curves"] = json!(p.curves);
            }
        } else {
            let (body, _) = state.supports.get(state.support).ok_or_else(|| app.tr("forms.prototype.pipe_source_required").to_owned())?;
            given["body"] = json!(body);
            if !p.all_edges {
                if state.support_edges.is_empty() {
                    return Err(app.tr("forms.prototype.pipe_source_required").to_owned());
                }
                given["edges"] = json!(state.support_edges);
            }
        }
    }
    Ok(json!({"pipe":given}))
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State) {
    let Some(mut p) = state.pipe.take() else {
        return;
    };
    if p.was_sculpted {
        let r = ui.checkbox(&mut p.regenerate, app.tr("forms.prototype.pipe_regenerate"));
        crate::scenario::publish_handle("forms_pipe_regenerate", r.rect.center());
        ui.add(egui::Label::new(app.tr("forms.prototype.pipe_sculpted_help")).wrap());
    }
    ui.add_enabled_ui(!p.was_sculpted || p.regenerate, |ui| {
        let old = p.source_kind;
        let options = [app.tr("forms.prototype.pipe_source_sketch"), app.tr("forms.prototype.pipe_source_edges")];
        let r = crate::ui_kit::choice_row(ui, app.tr("forms.prototype.pipe_source"), "forms_pipe_source_kind", &mut p.source_kind, &options);
        crate::scenario::publish_handle("forms_pipe_source_kind", r.rect.center());
        if old != p.source_kind {
            p.source_changed = true;
            p.overrides.clear();
            p.branch = 0;
            p.all_edges = true;
            state.picking_support = false;
        }
        if p.source_kind == 0 {
            let labels: Vec<_> = p.sketches.iter().map(|(_, name)| name.as_str()).collect();
            if labels.is_empty() {
                ui.label(app.tr("forms.prototype.pipe_source_required"));
            } else {
                let old = p.sketch;
                let r = crate::ui_kit::choice_row(ui, app.tr("forms.prototype.pipe_sketch"), "forms_pipe_sketch", &mut p.sketch, &labels);
                crate::scenario::publish_handle("forms_pipe_sketch", r.rect.center());
                if old != p.sketch {
                    p.curves.clear();
                    p.overrides.clear();
                    p.source_changed = true;
                    p.branch = 0;
                }
            }
        } else {
            let labels: Vec<_> = state.supports.iter().map(|(name, _)| name.as_str()).collect();
            if labels.is_empty() {
                ui.label(app.tr("forms.prototype.pipe_source_required"));
            } else {
                let old = state.support;
                let r = crate::ui_kit::choice_row(ui, app.tr("forms.prototype.pipe_body"), "forms_pipe_body", &mut state.support, &labels);
                crate::scenario::publish_handle("forms_pipe_body", r.rect.center());
                if old != state.support {
                    state.support_edges.clear();
                    p.all_edges = true;
                    p.source_changed = true;
                    p.overrides.clear();
                    p.branch = 0;
                }
                let r = ui.checkbox(&mut p.all_edges, app.tr("forms.prototype.pipe_all_edges"));
                crate::scenario::publish_handle("forms_pipe_all_edges", r.rect.center());
                if r.changed() {
                    p.source_changed = true;
                }
                let r = ui.selectable_label(state.picking_support, app.tr("forms.prototype.pipe_pick_edges"));
                crate::scenario::publish_handle("forms_pipe_pick_edges", r.rect.center());
                if r.clicked() {
                    state.picking_support = !state.picking_support;
                    if state.picking_support {
                        p.all_edges = false;
                        p.source_changed = true;
                    }
                }
                if !p.all_edges {
                    ui.label(crate::i18n::format("forms.prototype.pipe_edges_count", &[("count", &state.support_edges.len().to_string())]));
                }
            }
        }
        for (key, value) in [
            ("forms.prototype.pipe_diameter", &mut p.diameter),
            ("forms.prototype.pipe_section_density", &mut p.density),
            ("forms.prototype.pipe_path_samples", &mut p.samples),
        ] {
            crate::ui_kit::expr_row(ui, app.tr(key), value);
        }
        if let Some(form) = &app.session.forms_prototype
            && !form.pipe_segments.is_empty()
        {
            ui.separator();
            let options: Vec<_> = form
                .pipe_segments
                .iter()
                .enumerate()
                .map(|(i, _)| crate::i18n::format("forms.prototype.pipe_segment", &[("index", &(i + 1).to_string())]))
                .collect();
            p.branch = p.branch.min(options.len() - 1);
            let r = crate::ui_kit::choice_row(
                ui,
                app.tr("forms.prototype.pipe_branch"),
                "forms_pipe_segment",
                &mut p.branch,
                &options.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            crate::scenario::publish_handle("forms_pipe_segment", r.rect.center());
            if let Some(segment) = form.pipe_segments.get(p.branch) {
                let fields = p.overrides.entry(segment.key.clone()).or_insert_with(|| ["0".into(), "0".into()]);
                crate::ui_kit::expr_row(ui, app.tr("forms.prototype.pipe_segment_diameter"), &mut fields[0]);
                crate::ui_kit::expr_row(ui, app.tr("forms.prototype.pipe_segment_samples"), &mut fields[1]);
            }
            ui.add(egui::Label::new(app.tr("forms.prototype.pipe_inherit")).wrap());
        }
    });
    ui.add(egui::Label::new(app.tr("forms.prototype.pipe_help")).wrap());
    state.pipe = Some(p);
}
pub(super) fn overlay(app: &SolveApp, ui: &egui::Ui, proj: &Proj, state: &State) {
    let Some(p) = &state.pipe else {
        return;
    };
    let Some(form) = &app.session.forms_prototype else {
        return;
    };
    let Some(segment) = form.pipe_segments.get(p.branch) else {
        return;
    };
    for pair in segment.path.windows(2) {
        if let (Some(a), Some(b)) = (proj.to_screen(form.world_point(pair[0])), proj.to_screen(form.world_point(pair[1]))) {
            ui.painter().line_segment([a, b], Stroke::new(2.5, Tokens::get().accent));
        }
    }
}
