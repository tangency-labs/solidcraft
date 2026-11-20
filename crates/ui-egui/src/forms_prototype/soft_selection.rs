//! Proportional transforms use authored logical adjacency and evaluated world points.
use super::*;
use solidcore_ops::form_transform::MAX_WEIGHT_POINTS;
use solidcraft_ui_kit::BackingTooltipExt as _;
use std::collections::BTreeMap;

const MAX_TARGETS: usize = 256;

pub(super) struct State {
    pub enabled: bool,
    pub radius: String,
    pub(super) falloff: usize,
    cached: Option<(u64, Result<BTreeMap<usize, f64>, String>)>,
}
impl Default for State {
    fn default() -> Self {
        Self { enabled: false, radius: "20 mm".into(), falloff: 0, cached: None }
    }
}
pub(super) struct Targets {
    pub indices: Vec<usize>,
    pub points: Vec<Vec3>,
    pub weights: Vec<f64>,
    pub control: bool,
    pub influence: BTreeMap<usize, f64>,
}
fn message(id: &str) -> String {
    solidcraft_ui_text::tr(id).to_owned()
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let active = app.forms_prototype.drag.is_some() || app.forms_prototype.gizmo_drag.is_some();
    ui.add_enabled_ui(!active, |ui| {
        let label = app.tr("forms.soft.enabled").to_owned();
        let help = app.tr("forms.soft.help").to_owned();
        let r = crate::ui_kit::checkbox(ui, &label, &mut app.forms_prototype.soft.enabled).backing_on_hover_text(help);
        crate::scenario::publish_handle("forms_soft_selection", r.rect.center());
        if !app.forms_prototype.soft.enabled {
            return;
        }
        let label = app.tr("forms.soft.radius").to_owned();
        crate::ui_kit::expr_row(ui, &label, &mut app.forms_prototype.soft.radius);
        let label = app.tr("forms.soft.falloff").to_owned();
        let options = ["forms.soft.smooth", "forms.soft.linear", "forms.soft.sharp"].map(|key| app.tr(key).to_owned());
        crate::ui_kit::choice_row(
            ui,
            &label,
            "forms_soft_falloff",
            &mut app.forms_prototype.soft.falloff,
            &options.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        if !active {
            refresh(app)
        }
        if let Some((_, Err(error))) = &app.forms_prototype.soft.cached {
            ui.colored_label(Tokens::get().warning, error);
        }
    });
}

pub(super) fn weights(
    points: &[Vec3],
    faces: &[Vec<usize>],
    seeds: &BTreeSet<usize>,
    editable: &BTreeSet<usize>,
    locked: &BTreeSet<usize>,
    radius: f64,
    kind: usize,
) -> Result<BTreeMap<usize, f64>, String> {
    use solidcore_ops::form_transform::WeightError;
    solidcore_ops::form_transform::weights(points, faces, seeds, editable, locked, radius, kind).map_err(|error| {
        message(match error {
            WeightError::InvalidRadius => "forms.soft.invalid_radius",
            WeightError::InvalidSource => "forms.soft.invalid_source",
            WeightError::Limit => "forms.soft.limit",
        })
    })
}
fn capture(app: &SolveApp) -> Result<Targets, String> {
    let form = app.session.forms_prototype.as_ref().ok_or_else(|| message("forms.soft.invalid_source"))?;
    if form.hierarchy.as_ref().is_some_and(|h| h.has_trim_wall()) {
        return Err(message("forms.soft.trim_wall"));
    }
    let radius = controls::number(app, &app.forms_prototype.soft.radius, solidcraft_engine::doc::expr::Kind::Length)
        .ok_or_else(|| message("forms.soft.invalid_radius"))?;
    let editable =
        form.logical.faces.iter().flatten().copied().filter(|&id| form.selectable_control(id) && live_cad::handle_available(app, id)).collect();
    let locked = form.profile.as_ref().map(|p| p.boundary()).unwrap_or_default();
    let mut influence = weights(
        &form.surface.vertices,
        &form.logical.faces,
        &app.forms_prototype.selected,
        &editable,
        &locked,
        radius,
        app.forms_prototype.soft.falloff,
    )?;
    let mut drivers = influence.clone();
    if let Some(symmetry) = &form.symmetry {
        // Author one target per pair: sending conflicting targets for both partners would
        // average away the drag normal to the mirror plane in the engine's constraint step.
        for (&id, &w) in &drivers.clone() {
            let partner = *symmetry.partners.get(id).ok_or_else(|| message("forms.soft.invalid_source"))?;
            let peer = drivers.get(&partner).copied().unwrap_or(0.0);
            let weight = w.max(peer);
            influence.insert(id, weight);
            influence.insert(partner, weight);
            if partner != id {
                let keep = if w > peer
                    || (w == peer && app.forms_prototype.selected.contains(&id) && !app.forms_prototype.selected.contains(&partner))
                {
                    id
                } else if peer > w || (w == peer && app.forms_prototype.selected.contains(&partner) && !app.forms_prototype.selected.contains(&id)) {
                    partner
                } else {
                    id.min(partner)
                };
                drivers.remove(&if keep == id { partner } else { id });
                drivers.insert(keep, weight);
            }
        }
    }
    if influence.len() > MAX_TARGETS {
        return Err(message("forms.soft.limit"));
    }
    let indices: Vec<_> = drivers.keys().copied().collect();
    if indices.is_empty() {
        return Err(message("forms.soft.empty"));
    }
    let points = indices
        .iter()
        .map(|&id| form.surface.vertices.get(id).copied().ok_or_else(|| message("forms.soft.invalid_source")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Targets { indices, points, weights: drivers.into_values().collect(), control: false, influence })
}
/// Numeric controls can use this same immutable gesture packet without changing selection.
pub(super) fn targets(app: &SolveApp, extrusion: bool) -> Result<Targets, String> {
    if app.forms_prototype.soft.enabled && !extrusion {
        return capture(app);
    }
    let form = app.session.forms_prototype.as_ref().ok_or_else(|| message("forms.soft.invalid_source"))?;
    let indices: Vec<_> = app.forms_prototype.selected.iter().copied().collect();
    let control = form.hierarchy.is_none() && indices.len() == form.cage.points.len();
    let controls;
    let source = if control {
        controls = form.control_positions();
        &controls
    } else {
        &form.surface.vertices
    };
    let points =
        indices.iter().map(|&i| source.get(i).copied().ok_or_else(|| message("forms.soft.invalid_source"))).collect::<Result<Vec<_>, _>>()?;
    let weights = vec![1.0; indices.len()];
    Ok(Targets { indices, points, weights, control, influence: BTreeMap::new() })
}
pub(super) fn preview_key(app: &SolveApp) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    app.session.document_id.hash(&mut hash);
    app.session.authoring_revision().hash(&mut hash);
    app.session.forms_prototype.as_ref().map(|f| f.revision).hash(&mut hash);
    context::key(app).hash(&mut hash);
    live_cad::mesh(app).is_some().hash(&mut hash);
    live_cad::busy(app).hash(&mut hash);
    if let Some(form) = &app.session.forms_prototype {
        // Accepted fixed CAD guide handles can change without an authoring revision.
        // Hash the actual logical inputs and availability, never tessellation topology.
        form.surface.vertices.len().hash(&mut hash);
        for point in form.surface.vertices.iter().take(MAX_WEIGHT_POINTS + 1) {
            point.x.to_bits().hash(&mut hash);
            point.y.to_bits().hash(&mut hash);
            point.z.to_bits().hash(&mut hash);
        }
        let boundary = form.profile.as_ref().map(|p| p.boundary()).unwrap_or_default();
        boundary.hash(&mut hash);
        boundary.first().map(|&id| live_cad::handle_available(app, id)).hash(&mut hash);
    }
    app.forms_prototype.selected.hash(&mut hash);
    app.forms_prototype.soft.radius.hash(&mut hash);
    app.forms_prototype.soft.falloff.hash(&mut hash);
    hash.finish()
}
fn refresh(app: &mut SolveApp) {
    let key = preview_key(app);
    if app.forms_prototype.soft.cached.as_ref().is_none_or(|(old, _)| *old != key) {
        let found = capture(app).map(|targets| targets.influence);
        app.forms_prototype.soft.cached = Some((key, found));
    }
}
pub(super) fn paint(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    if !app.forms_prototype.soft.enabled || extrusion::active(app) {
        return;
    }
    let drag = app.forms_prototype.drag.as_ref().or(app.forms_prototype.gizmo_drag.as_ref());
    let weights = if let Some(drag) = drag {
        if drag.extrude {
            return;
        }
        drag.influence.clone()
    } else {
        refresh(app);
        app.forms_prototype.soft.cached.as_ref().and_then(|(_, weights)| weights.as_ref().ok()).cloned().unwrap_or_default()
    };
    let Some(form) = &app.session.forms_prototype else { return };
    let tokens = Tokens::get();
    for (id, weight) in weights {
        let Some(&point) = form.surface.vertices.get(id) else { continue };
        if !surface::visible(app, proj, point) {
            continue;
        }
        let Some(pos) = proj.to_screen(point) else { continue };
        let color =
            egui::Color32::from(egui::Rgba::from(tokens.body_edge) * (1.0 - weight as f32) + egui::Rgba::from(tokens.sel_vertex) * weight as f32);
        ui.painter().circle_filled(pos, 3.5 + 1.5 * weight as f32, color);
        ui.painter().circle_stroke(pos, 3.5 + 1.5 * weight as f32, Stroke::new(1.0, tokens.viewport_top));
    }
}
