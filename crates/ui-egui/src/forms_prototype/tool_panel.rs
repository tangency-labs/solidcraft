//! Modal tool previews are computed on copies and accepted only for current inputs.
use super::*;

mod pipe_panel;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Mode {
    Bridge,
    Collapse,
    EraseFill,
    Fill,
    Flatten,
    Match,
    Pipe,
    Straighten,
    Surface,
    Thicken,
    Unweld,
    Weld,
}
impl Mode {
    fn title(self) -> &'static str {
        match self {
            Self::Bridge => "forms.prototype.bridge",
            Self::Collapse => "forms.prototype.collapse",
            Self::EraseFill => "forms.prototype.erase_and_fill",
            Self::Fill => "forms.prototype.fill_hole",
            Self::Flatten => "forms.prototype.flatten",
            Self::Match => "forms.prototype.match",
            Self::Pipe => "cmd.form.pipe",
            Self::Straighten => "forms.prototype.straighten",
            Self::Surface => "forms.prototype.surface",
            Self::Thicken => "forms.prototype.thicken",
            Self::Unweld => "forms.prototype.unweld",
            Self::Weld => "forms.prototype.weld",
        }
    }
}
struct Pending {
    revision: u64,
    params: serde_json::Value,
    done: std::sync::mpsc::Receiver<Result<solidcraft_engine::ComputedCommands, String>>,
}
struct OtherForm {
    label: String,
    target: solidcraft_engine::schema::forms_prototype::FormObject,
    openings: Vec<Vec<usize>>,
}
pub(super) struct State {
    pipe: Option<pipe_panel::Inputs>,
    surface_partition: bool,
    surface_segments: [String; 2],
    match_adjacent: usize,
    picking_support: bool,
    support_edges: BTreeSet<usize>,
    supports: Vec<(String, usize)>,
    support: usize,
    support_face: usize,
    match_order: usize,
    match_wire: bool,
    match_tolerances: [String; 3],
    other_forms: Vec<OtherForm>,
    other: usize,
    first_openings: Vec<Vec<usize>>,
    first_opening: usize,
    second_opening: usize,
    separate: bool,
    mode: Mode,
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
    faces: Vec<usize>,
    elements: BTreeSet<Element>,
    pivot: Option<Vec3>,
    normal: Vec3,
    distance: String,
    sharp: bool,
    direction: usize,
    controls: bool,
    fill: usize,
    local_bridge: bool,
    desired: Option<serde_json::Value>,
    applied: Option<serde_json::Value>,
    deadline: f64,
    pending: Option<Pending>,
    error: Option<String>,
}
pub(super) fn active(app: &SolveApp) -> bool {
    app.forms_prototype.tool_panel.is_some() || app.session.forms_prototype.as_ref().is_some_and(|f| f.has_tool_preview())
}
pub(super) fn matching(app: &SolveApp) -> bool {
    app.forms_prototype.matching_preview
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let Some(state) = &app.forms_prototype.tool_panel else {
        return serde_json::Value::Null;
    };
    json!({"mode":state.mode.title(),"pending":state.pending.is_some(),"error":state.error,"desired":state.desired,"applied":state.applied,
        "picking_support":state.picking_support,"support_edges":state.support_edges})
}
pub(super) fn start(app: &mut SolveApp, mode: Mode) {
    if active(app) {
        return;
    }
    let Some(form) = &app.session.forms_prototype else { return };
    if form.profile.is_some() && !matches!(mode, Mode::Surface | Mode::Flatten | Mode::Straighten) {
        app.set_status(app.tr("forms.prototype.surface_fixed_grid").to_owned(), true);
        return;
    }
    if matches!(mode, Mode::Thicken | Mode::Fill | Mode::EraseFill | Mode::Weld | Mode::Unweld | Mode::Collapse | Mode::Match)
        && form.hierarchy.is_some()
    {
        app.set_status(app.tr("forms.prototype.base_tool_required").to_owned(), true);
        return;
    }
    if mode == Mode::Thicken && form.cage.boundary_loops().is_ok_and(|l| l.is_empty()) {
        app.set_status(app.tr("forms.prototype.open_surface_required").to_owned(), true);
        return;
    }
    let bounds: solidcraft_engine::geom::Aabb3 = form.cage.points.iter().copied().collect();
    let size = bounds.size();
    let distance = (size.x.max(size.y).max(size.z) * 0.1).max(0.1).to_string();
    let local_bridge = mode == Mode::Bridge && form.hierarchy.is_some();
    let (direction, controls) = controls::projection_settings(app);
    let mut edges: Vec<_> = app.forms_prototype.elements.iter().filter_map(|e| if let Element::Edge(e) = e { Some(*e) } else { None }).collect();
    let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
    if mode == Mode::Unweld && edges.is_empty() {
        let mut counts = std::collections::BTreeMap::new();
        for id in &faces {
            if let Some(face) = form.cage.faces.get(*id) {
                for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                    *counts.entry([a.min(b), a.max(b)]).or_insert(0usize) += 1;
                }
            }
        }
        edges = counts.into_iter().filter_map(|(edge, n)| (n == 1).then_some(edge)).collect();
    }
    let first_openings = form.cage.boundary_loops().unwrap_or_default();
    let mut supports = Vec::new();
    let mut support = 0;
    let mut support_face = 0;
    let mut support_world = None;
    if matches!(mode, Mode::Match | Mode::Surface | Mode::Pipe) {
        let before = form.editing_feature.map_or_else(|| app.session.model.state(), |id| app.session.model.state_before(id));
        let world = solidcraft_engine::doc::world_state(&app.session.doc, &before);
        supports = world
            .bodies
            .iter()
            .filter(|b| !b.body.is_mesh() && b.body.face_count() > 0 && b.body.face_count() <= 4096)
            .filter(|b| mode != Mode::Surface || !b.body.is_surface())
            .take(512)
            .map(|b| (b.name.clone(), b.body.face_count()))
            .collect();
        if (mode != Mode::Pipe && supports.is_empty()) || (mode == Mode::Match && (first_openings.is_empty() || form.symmetry.is_some())) {
            app.set_status(
                app.tr(if mode == Mode::Surface { "forms.prototype.surface_required" } else { "forms.prototype.match_required" }).to_owned(),
                true,
            );
            return;
        }
        if mode == Mode::Surface
            && let Some(profile) = &form.profile
            && let Ok((body, face)) = profile.resolve(&world)
        {
            support = supports.iter().position(|(name, _)| name == &body.name).unwrap_or(0);
            support_face = face;
        }
        support_world = Some(world);
    }
    let first_opening = first_openings
        .iter()
        .position(|ring| edges.iter().any(|e| ring.windows(2).any(|pair| pair.contains(&e[0]) && pair.contains(&e[1]))))
        .unwrap_or(0);
    let mut other_forms = Vec::new();
    if mode == Mode::Weld {
        for (&id, part) in &form.parts {
            if let Ok(openings) = part.definition.cage.boundary_loops()
                && !openings.is_empty()
                && part.definition.hierarchy.is_none()
            {
                other_forms.push(OtherForm {
                    label: part.name.clone(),
                    target: solidcraft_engine::schema::forms_prototype::FormObject::Draft { part: id },
                    openings,
                });
            }
        }
        for feature in &app.session.doc.features {
            if Some(feature.id) == form.editing_feature || form.consumes(feature.id) {
                continue;
            }
            if let solidcraft_engine::doc::FeatureKind::Form { definition, operation: solidcraft_engine::doc::Operation::NewBody, .. } = &feature.kind
                && definition.hierarchy.is_none()
                && let Ok(openings) = definition.cage.boundary_loops()
                && !openings.is_empty()
            {
                other_forms.push(OtherForm {
                    label: feature.name.clone(),
                    target: solidcraft_engine::schema::forms_prototype::FormObject::Feature { feature: feature.id },
                    openings,
                });
            }
        }
        if first_openings.is_empty() || other_forms.is_empty() {
            app.set_status(app.tr("forms.prototype.weld_openings_required").to_owned(), true);
            return;
        }
    }
    if mode == Mode::Unweld && (!first_openings.is_empty() || edges.is_empty()) {
        app.set_status(app.tr("forms.prototype.unweld_seam_required").to_owned(), true);
        return;
    }
    if mode == Mode::Collapse && app.forms_prototype.selected.len() != 2 {
        app.set_status(app.tr("forms.prototype.two_points_required").to_owned(), true);
        return;
    }
    let mut state = State {
        pipe: (mode == Mode::Pipe).then(|| pipe_panel::Inputs::new(app)),
        surface_partition: form.profile.as_ref().map_or(app.preferences.forms_surface_partition, |p| p.partition),
        surface_segments: form.profile.as_ref().map_or(["2".into(), "2".into()], |p| p.segments.map(|v| v.to_string())),
        match_adjacent: usize::from(form.matching.as_ref().map_or(app.preferences.forms_match_adjacent, |m| m.adjacent_faces)),
        picking_support: false,
        support_edges: BTreeSet::new(),
        supports,
        support,
        support_face,
        match_order: if mode == Mode::Surface {
            form.profile.as_ref().map_or(0, |p| p.continuity)
        } else {
            form.matching.as_ref().map_or(0, |m| m.config.order)
        },
        match_wire: mode == Mode::Pipe,
        match_tolerances: ["0.001 mm".into(), "0.1".into(), "0.01".into()],
        other_forms,
        other: 0,
        first_openings,
        first_opening,
        second_opening: 0,
        separate: true,
        mode,
        vertices: app.forms_prototype.selected.iter().copied().collect(),
        edges,
        faces,
        elements: app.forms_prototype.elements.clone(),
        pivot: app.forms_prototype.pivot,
        normal: basis(app)[2],
        distance,
        sharp: true,
        direction,
        controls,
        fill: 0,
        local_bridge,
        desired: None,
        applied: None,
        deadline: 0.0,
        pending: None,
        error: None,
    };
    if mode == Mode::Pipe {
        pipe_panel::select_source(app, &mut state, support_world.as_ref());
        if !pipe_panel::has_sources(&state) {
            app.set_status(app.tr("forms.prototype.pipe_source_required").to_owned(), true);
            return;
        }
    }
    if run(app, "form.prototype.tool.begin", json!({})) {
        app.forms_prototype.matching_preview = matches!(mode, Mode::Match | Mode::Surface | Mode::Pipe);
        app.forms_prototype.tools = Default::default();
        app.forms_prototype.elements.clear();
        app.forms_prototype.selected.clear();
        app.forms_prototype.pivot = None;
        app.forms_prototype.tool_panel = Some(state);
    }
}
fn params(app: &SolveApp, state: &State) -> Result<serde_json::Value, String> {
    if state.mode == Mode::Pipe {
        return pipe_panel::params(app, state);
    }
    if state.mode == Mode::Surface {
        let (body, _) = state.supports.get(state.support).ok_or_else(|| app.tr("forms.prototype.surface_required").to_owned())?;
        let mut segments = [2usize; 2];
        for (i, text) in state.surface_segments.iter().enumerate() {
            let n = controls::number(app, text, solidcraft_engine::doc::expr::Kind::Unitless)
                .filter(|n| (1.0..=16.0).contains(n) && n.fract() == 0.0)
                .ok_or_else(|| app.tr("forms.prototype.invalid_number").to_owned())?;
            segments[i] = n as usize;
        }
        return Ok(
            json!({"surface":{"partition":state.surface_partition,"body":body,"face":state.support_face,"segments":segments,"continuity":state.match_order.min(1)}}),
        );
    }
    if state.mode == Mode::Match {
        let (body, _) = state.supports.get(state.support).ok_or_else(|| app.tr("forms.prototype.match_required").to_owned())?;
        let ring = state
            .first_openings
            .get(state.first_opening)
            .filter(|r| r.len() > 1)
            .ok_or_else(|| app.tr("forms.prototype.match_required").to_owned())?;
        let mut tolerances = [0.001, 0.1, 0.01];
        for (i, text) in state.match_tolerances.iter().enumerate().take(state.match_order + 1) {
            tolerances[i] = controls::number(
                app,
                text,
                match i {
                    0 => solidcraft_engine::doc::expr::Kind::Length,
                    1 => solidcraft_engine::doc::expr::Kind::Angle,
                    _ => solidcraft_engine::doc::expr::Kind::Unitless,
                },
            )
            .ok_or_else(|| app.tr("forms.prototype.invalid_number").to_owned())?;
            if i == 1 {
                tolerances[i] = tolerances[i].to_degrees();
            }
        }
        let mut given = json!({"matching":{"adjacent_faces":state.match_adjacent==1 && !state.match_wire,"body":body,"face":state.support_face,"wire_from_face":state.match_wire,"edge":[ring[0],ring[1]],"order":state.match_order,"position_mm":tolerances[0],"tangent_deg":tolerances[1],"curvature_relative":tolerances[2],"tolerance":controls::tolerance(app).unwrap_or(0.01)}});
        if state.match_wire && !state.support_edges.is_empty() {
            given["matching"].as_object_mut().ok_or_else(|| app.tr("forms.prototype.invalid_number").to_owned())?.remove("face");
            given["matching"]["wire_from_face"] = json!(false);
            given["matching"]["edges"] = json!(state.support_edges);
        }
        return Ok(given);
    }
    if state.mode == Mode::Unweld {
        return Ok(json!({"unweld":{"edges":state.edges,"separate":state.separate,"name":app.tr("forms.prototype.unwelded_name")}}));
    }
    if state.mode == Mode::Weld {
        let other = state.other_forms.get(state.other).ok_or_else(|| app.tr("forms.prototype.weld_openings_required").to_owned())?;
        let a = state
            .first_openings
            .get(state.first_opening)
            .filter(|l| l.len() > 1)
            .ok_or_else(|| app.tr("forms.prototype.weld_openings_required").to_owned())?;
        let b = other
            .openings
            .get(state.second_opening)
            .filter(|l| l.len() > 1)
            .ok_or_else(|| app.tr("forms.prototype.weld_openings_required").to_owned())?;
        let target = match other.target {
            solidcraft_engine::schema::forms_prototype::FormObject::Draft { part } => json!({"kind":"draft","part":part}),
            solidcraft_engine::schema::forms_prototype::FormObject::Feature { feature } => json!({"kind":"feature","feature":feature}),
        };
        return Ok(json!({"weld":{"other":target,"first_edge":[a[0],a[1]],"second_edge":[b[0],b[1]]}}));
    }
    if matches!(state.mode, Mode::Flatten | Mode::Straighten) {
        let direction = match state.direction {
            1 => Some(Vec3::X),
            2 => Some(Vec3::Y),
            3 => Some(Vec3::Z),
            4 => Some(state.normal),
            _ => None,
        };
        return Ok(json!({"project":{
            "tool":if state.mode==Mode::Flatten{"flatten"}else{"straighten"},"vertices":state.vertices,
            "points":if state.controls{"control"}else{"surface"},"direction":direction.map(|d|[d.x,d.y,d.z]),
            "origin":state.pivot.map(|p|[p.x,p.y,p.z]),
        }}));
    }
    let tool = match state.mode {
        Mode::Bridge => "bridge",
        Mode::EraseFill => "erase_and_fill",
        Mode::Fill => "fill_hole",
        Mode::Thicken => "thicken",
        Mode::Collapse => "weld",
        _ => return Err(app.tr("forms.prototype.invalid_number").to_owned()),
    };
    let distance = if state.mode == Mode::Thicken {
        let v = controls::number(app, &state.distance, solidcraft_engine::doc::expr::Kind::Length)
            .ok_or_else(|| app.tr("forms.prototype.invalid_number").to_owned())?;
        if !v.is_finite() || v.abs() < 1e-6 || v.abs() > 10_000.0 {
            return Err(app.tr("forms.prototype.invalid_number").to_owned());
        }
        v
    } else {
        0.0
    };
    let fill = ["automatic", "single", "minimal", "reduced_star"].get(state.fill).copied().unwrap_or("automatic");
    Ok(json!({"topology":{"tool":tool,"distance":distance,"sharp":state.sharp,"fill_mode":fill,
        "vertices":state.vertices,"faces":state.faces,"edges":state.edges}}))
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut state) = app.forms_prototype.tool_panel.take() else {
        ui.label(app.tr("forms.prototype.external_preview"));
        if ui.add_enabled(live_cad::mesh(app).is_some(), egui::Button::new(app.tr("forms.prototype.apply"))).clicked() {
            run(app, "form.prototype.tool.apply", json!({}));
        }
        if ui.button(app.tr("forms.prototype.cancel")).clicked() {
            run(app, "form.prototype.tool.cancel", json!({}));
        }
        return;
    };
    ui.heading(app.tr(state.mode.title()));
    if state.mode == Mode::Pipe {
        pipe_panel::show(app, ui, &mut state);
    }
    if matches!(state.mode, Mode::Match | Mode::Surface) {
        let bodies: Vec<_> = state.supports.iter().map(|(name, _)| name.as_str()).collect();
        let old = state.support;
        let r = crate::ui_kit::choice_row(ui, app.tr("forms.prototype.support_body"), "forms_match_body", &mut state.support, &bodies);
        crate::scenario::publish_handle("forms_match_body", r.rect.center());
        if old != state.support {
            state.support_face = 0;
            state.support_edges.clear();
        }
        let count = state.supports.get(state.support).map_or(0, |(_, count)| *count);
        let faces: Vec<_> =
            (0..count).map(|i| crate::i18n::format("forms.prototype.support_face_index", &[("index", &(i + 1).to_string())])).collect();
        let previous_face = state.support_face;
        let r = crate::ui_kit::choice_row(
            ui,
            app.tr("forms.prototype.support_face"),
            "forms_match_face",
            &mut state.support_face,
            &faces.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        crate::scenario::publish_handle("forms_match_face", r.rect.center());
        if previous_face != state.support_face {
            state.support_edges.clear();
        }
        let r = ui.selectable_label(state.picking_support, app.tr("forms.prototype.pick_support"));
        crate::scenario::publish_handle("forms_match_pick", r.rect.center());
        if r.clicked() {
            state.picking_support = !state.picking_support;
        }
        if state.mode == Mode::Match {
            let openings = opening_choices(&state.first_openings);
            let r = crate::ui_kit::choice_row(
                ui,
                app.tr("forms.prototype.first_opening"),
                "forms_match_opening",
                &mut state.first_opening,
                &openings.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            crate::scenario::publish_handle("forms_match_opening", r.rect.center());
            let orders = ["G0", "G1", "G2"];
            let r = crate::ui_kit::choice_row(ui, app.tr("forms.prototype.continuity"), "forms_match_order", &mut state.match_order, &orders);
            crate::scenario::publish_handle("forms_match_order", r.rect.center());
            if state.match_order > 0 && !state.match_wire {
                let modes: [String; 2] = ["forms.prototype.selected_support", "forms.prototype.adjacent_support"].map(|key| app.tr(key).to_owned());
                let previous = state.match_adjacent;
                let r = crate::ui_kit::choice_row(
                    ui,
                    app.tr("forms.prototype.continuity_support"),
                    "forms_match_continuity_support",
                    &mut state.match_adjacent,
                    &modes.iter().map(String::as_str).collect::<Vec<_>>(),
                );
                crate::scenario::publish_handle("forms_match_continuity_support", r.rect.center());
                if previous != state.match_adjacent {
                    app.preferences.forms_match_adjacent = state.match_adjacent == 1;
                }
            }
            let r = ui.checkbox(&mut state.match_wire, app.tr("forms.prototype.match_wire"));
            crate::scenario::publish_handle("forms_match_wire", r.rect.center());
            if !state.support_edges.is_empty() {
                ui.label(crate::i18n::format("forms.prototype.support_edges_selected", &[("count", &state.support_edges.len().to_string())]));
                if ui.button(app.tr("forms.prototype.clear_support_edges")).clicked() {
                    state.support_edges.clear();
                }
            }
            for (i, key) in ["forms.prototype.match_position", "forms.prototype.match_tangent", "forms.prototype.match_curvature"]
                .iter()
                .enumerate()
                .take(state.match_order + 1)
            {
                crate::ui_kit::expr_row(ui, app.tr(key), &mut state.match_tolerances[i]);
            }
            ui.add(egui::Label::new(app.tr("forms.prototype.match_help")).wrap());
            if state.desired == state.applied
                && state.error.is_none()
                && let Some(report) = app.session.forms_prototype.as_ref().and_then(|f| f.match_report.as_ref())
            {
                ui.add(
                    egui::Label::new(crate::i18n::format(
                        match state.match_order {
                            0 => "forms.prototype.match_report_g0",
                            1 => "forms.prototype.match_report_g1",
                            _ => "forms.prototype.match_report",
                        },
                        &[
                            ("position", &format!("{:.6}", report.position_mm)),
                            ("tangent", &format!("{:.4}", report.tangent_deg)),
                            ("curvature", &format!("{:.6}", report.curvature_relative)),
                        ],
                    ))
                    .wrap(),
                );
            }
        } else {
            for (i, key) in ["forms.prototype.surface_u", "forms.prototype.surface_v"].iter().enumerate() {
                crate::ui_kit::expr_row(ui, app.tr(key), &mut state.surface_segments[i]);
            }
            let r = crate::ui_kit::choice_row(
                ui,
                app.tr("forms.prototype.continuity"),
                "forms_surface_continuity",
                &mut state.match_order,
                &["G0", "G1"],
            );
            crate::scenario::publish_handle("forms_surface_continuity", r.rect.center());
            let response = ui.checkbox(&mut state.surface_partition, app.tr("forms.prototype.surface_partition"));
            crate::scenario::publish_handle("forms_surface_partition", response.rect.center());
            if response.changed() {
                app.preferences.forms_surface_partition = state.surface_partition;
            }
            ui.add(egui::Label::new(app.tr("forms.prototype.surface_help")).wrap());
        }
    }
    if state.mode == Mode::Unweld {
        let r = ui.checkbox(&mut state.separate, app.tr("forms.prototype.separate_forms"));
        crate::scenario::publish_handle("forms_unweld_separate", r.rect.center());
        ui.add(egui::Label::new(app.tr("forms.prototype.unweld_help")).wrap());
    }
    if state.mode == Mode::Weld {
        let labels: Vec<_> = state.other_forms.iter().map(|f| f.label.clone()).collect();
        let previous = state.other;
        let r = crate::ui_kit::choice_row(
            ui,
            app.tr("forms.prototype.other_form"),
            "forms_weld_other",
            &mut state.other,
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        crate::scenario::publish_handle("forms_weld_other", r.rect.center());
        if previous != state.other {
            state.second_opening = 0;
        }
        let first = opening_choices(&state.first_openings);
        let second = state.other_forms.get(state.other).map(|f| opening_choices(&f.openings)).unwrap_or_default();
        for (label, id, value, options) in [
            ("forms.prototype.first_opening", "forms_weld_first_opening", &mut state.first_opening, first),
            ("forms.prototype.second_opening", "forms_weld_second_opening", &mut state.second_opening, second),
        ] {
            let r = crate::ui_kit::choice_row(ui, app.tr(label), id, value, &options.iter().map(String::as_str).collect::<Vec<_>>());
            crate::scenario::publish_handle(id, r.rect.center());
        }
        app.forms_prototype.other_preview = state.other_forms.get(state.other).and_then(|f| match f.target {
            solidcraft_engine::schema::forms_prototype::FormObject::Feature { feature } => Some(feature),
            _ => None,
        });
        ui.add(egui::Label::new(app.tr("forms.prototype.weld_help")).wrap());
    }
    if state.mode == Mode::Thicken {
        let label = app.tr("forms.prototype.thickness").to_owned();
        crate::ui_kit::row(ui, &label, |ui| {
            let response = ui.scope(|ui| crate::ui_kit::field(ui, &mut state.distance));
            crate::scenario::publish_handle("forms_tool_distance", response.response.rect.center());
        });
        let r = ui.checkbox(&mut state.sharp, app.tr("forms.prototype.sharp_rims"));
        crate::scenario::publish_handle("forms_tool_sharp", r.rect.center());
        let r = ui.button(app.tr("forms.prototype.reverse"));
        crate::scenario::publish_handle("forms_tool_reverse", r.rect.center());
        if r.clicked()
            && let Some(value) = controls::number(app, &state.distance, solidcraft_engine::doc::expr::Kind::Length)
        {
            state.distance = (-value).to_string();
        }
    }
    if matches!(state.mode, Mode::Flatten | Mode::Straighten) {
        let options: [String; 5] =
            ["forms.prototype.best_fit", "forms.prototype.x", "forms.prototype.y", "forms.prototype.z", "forms.prototype.oriented_normal"]
                .map(|key| app.tr(key).to_owned());
        let r = crate::ui_kit::choice_row(
            ui,
            app.tr(if state.mode == Mode::Flatten { "forms.prototype.plane_normal" } else { "forms.prototype.line_direction" }),
            "forms_tool_direction",
            &mut state.direction,
            &options.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        crate::scenario::publish_handle("forms_tool_direction", r.rect.center());
        let r = ui.checkbox(&mut state.controls, app.tr("forms.prototype.projection_controls"));
        crate::scenario::publish_handle("forms_tool_controls", r.rect.center());
    }
    if state.mode == Mode::Fill {
        let options: [String; 4] =
            ["forms.prototype.fill_automatic", "forms.prototype.fill_single", "forms.prototype.fill_minimal", "forms.prototype.fill_star"]
                .map(|key| app.tr(key).to_owned());
        let r = crate::ui_kit::choice_row(
            ui,
            app.tr("forms.prototype.fill_mode"),
            "forms_tool_fill",
            &mut state.fill,
            &options.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        crate::scenario::publish_handle("forms_tool_fill", r.rect.center());
    }
    if state.local_bridge {
        ui.add(egui::Label::new(app.tr("forms.prototype.bridge_bakes_local")).wrap());
    }
    let now = ui.input(|i| i.time);
    match params(app, &state) {
        Ok(given) => {
            if state.desired.as_ref() != Some(&given) {
                state.desired = Some(given);
                state.deadline = now + 0.12;
                state.error = None;
            }
        }
        Err(error) => {
            state.desired = None;
            state.error = Some(error);
        }
    }
    if state.pending.is_none()
        && state.desired != state.applied
        && let Some(given) = state.desired.clone()
    {
        if now >= state.deadline {
            compute(app, ui.ctx(), &mut state, given);
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(125));
        }
    }
    if state.pending.is_some() || live_cad::busy(app) || context::busy(app) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(app.tr("forms.prototype.previewing"));
        });
    }
    if let Some(error) = state.error.as_ref().or(app.forms_prototype.live.error.as_ref()).or(app.forms_prototype.context.error.as_ref()) {
        ui.add(egui::Label::new(egui::RichText::new(error).color(Tokens::get().error)).wrap());
    }
    let ready = state.pending.is_none()
        && state.desired.is_some()
        && state.desired == state.applied
        && state.error.is_none()
        && context::complete(app)
        && live_cad::mesh(app).is_some();
    let mut done = false;
    ui.horizontal(|ui| {
        let r = ui.add_enabled(ready, egui::Button::new(app.tr("forms.prototype.apply")));
        crate::scenario::publish_handle("forms_tool_apply", r.rect.center());
        if r.clicked() && run(app, "form.prototype.tool.apply", json!({})) {
            done = true;
        }
        let r = ui.button(app.tr("forms.prototype.cancel"));
        crate::scenario::publish_handle("forms_tool_cancel", r.rect.center());
        if r.clicked() && run(app, "form.prototype.tool.cancel", json!({})) {
            app.forms_prototype.elements = state.elements.clone();
            app.forms_prototype.pivot = state.pivot;
            selection::sync(app);
            done = true;
        }
    });
    if !done {
        app.forms_prototype.tool_panel = Some(state);
    } else {
        app.forms_prototype.other_preview = None;
        app.forms_prototype.matching_preview = false;
    }
}
fn opening_choices(openings: &[Vec<usize>]) -> Vec<String> {
    openings
        .iter()
        .enumerate()
        .map(|(i, l)| crate::i18n::format("forms.prototype.opening_count", &[("index", &(i + 1).to_string()), ("count", &l.len().to_string())]))
        .collect()
}
pub(super) fn pointer(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj, input: &crate::navigation::CameraInput) {
    let Some(mut state) = app.forms_prototype.tool_panel.take() else {
        return;
    };
    if state.mode == Mode::Pipe {
        pipe_panel::overlay(app, ui, proj, &state);
        if !pipe_panel::cad(&state) {
            app.forms_prototype.tool_panel = Some(state);
            return;
        }
    }
    if !matches!(state.mode, Mode::Match | Mode::Surface | Mode::Pipe) {
        app.forms_prototype.tool_panel = Some(state);
        return;
    }
    let entries = &app.forms_prototype.context.entries;
    let hit = if state.picking_support && input.inside && input.mode.is_none() {
        input.hover.and_then(|pos| {
            let (origin, direction) = proj.ray(pos);
            entries
                .iter()
                .enumerate()
                .filter_map(|(i, entry)| {
                    let body = entry.name.as_ref()?;
                    let source = state.supports.iter().position(|(name, _)| name == body)?;
                    let (distance, triangle) = entry.index.as_ref()?.raycast(&entry.mesh, origin, direction)?;
                    let face = *entry.mesh.tri_face.get(triangle)? as usize;
                    Some((distance, i, source, face))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, entry, source, face)| {
                    let edge = if state.match_wire {
                        entries[entry]
                            .mesh
                            .edges
                            .iter()
                            .enumerate()
                            .filter_map(|(index, curve)| {
                                let distance = curve
                                    .windows(2)
                                    .filter_map(|p| {
                                        let a = proj.to_screen(p[0])?;
                                        let b = proj.to_screen(p[1])?;
                                        let ab = b - a;
                                        let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                                        Some(pos.distance(a + ab * t))
                                    })
                                    .min_by(f32::total_cmp)?;
                                (distance < 10.0).then_some((distance, index))
                            })
                            .min_by(|a, b| a.0.total_cmp(&b.0))
                            .map(|p| p.1)
                    } else {
                        None
                    };
                    (entry, source, face, edge)
                })
        })
    } else {
        None
    };
    if input.primary_pressed
        && let Some((_, source, face, edge)) = hit
    {
        if state.mode == Mode::Pipe {
            pipe_panel::picked(&mut state);
        }
        if state.support != source {
            state.support_edges.clear();
        }
        state.support = source;
        if state.match_wire {
            if let Some(edge) = edge
                && !state.support_edges.insert(edge)
            {
                state.support_edges.remove(&edge);
            }
        } else {
            state.support_face = face;
            state.support_edges.clear();
            state.picking_support = false;
        }
    }
    let chosen = state.supports.get(state.support).map(|(name, _)| name);
    for (i, entry) in entries.iter().enumerate() {
        let selected = entry.name.as_ref() == chosen && chosen.is_some();
        for (edge, curve) in entry.mesh.edges.iter().enumerate() {
            let hover = hit.is_some_and(|(entry, _, face, e)| {
                entry == i && if state.match_wire { e == Some(edge) } else { entry_faces(&app.forms_prototype.context.entries[i].mesh, edge, face) }
            });
            let highlight = selected
                && if state.mode == Mode::Pipe {
                    pipe_panel::highlight(&state, edge)
                } else if state.match_wire && !state.support_edges.is_empty() {
                    state.support_edges.contains(&edge)
                } else {
                    entry_faces(&entry.mesh, edge, state.support_face)
                };
            if hover || highlight {
                for pair in curve.windows(2) {
                    if let (Some(a), Some(b)) = (proj.to_screen(pair[0]), proj.to_screen(pair[1])) {
                        ui.painter().line_segment([a, b], Stroke::new(if hover { 3.0 } else { 2.0 }, Tokens::get().accent));
                    }
                }
            }
        }
    }
    app.forms_prototype.tool_panel = Some(state);
}
fn entry_faces(mesh: &Mesh, edge: usize, face: usize) -> bool {
    mesh.edge_faces.get(edge).is_some_and(|faces| faces.contains(&(face as u32)))
}
fn compute(app: &mut SolveApp, ctx: &egui::Context, state: &mut State, given: serde_json::Value) {
    let Some(form) = &app.session.forms_prototype else { return };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let revision = form.revision;
        let copy = app.session.scratch();
        let wake = ctx.clone();
        let params = given.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        match std::thread::Builder::new().name("form-tool-preview".into()).spawn(move || {
            let result = copy.compute_commands(&[("form.prototype.tool.preview".into(), params)], None).map_err(|e| e.to_string());
            let _ = tx.send(result);
            wake.request_repaint();
        }) {
            Ok(_) => {
                state.pending = Some(Pending { revision, params: given, done: rx });
                return;
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    let _ = (ctx, form);
    match app.session.execute("form.prototype.tool.preview", &given) {
        Ok(_) => {
            state.applied = Some(given);
            state.error = None;
        }
        Err(error) => {
            state.applied = Some(given);
            state.error = Some(error.to_string());
        }
    }
}
pub(super) fn tick(app: &mut SolveApp) {
    let Some(mut state) = app.forms_prototype.tool_panel.take() else { return };
    if !app.session.forms_prototype.as_ref().is_some_and(|f| f.has_tool_preview()) {
        return;
    }
    let Some(pending) = &state.pending else {
        app.forms_prototype.tool_panel = Some(state);
        return;
    };
    let result = match pending.done.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            app.forms_prototype.tool_panel = Some(state);
            return;
        }
        Err(_) => Err(app.tr("forms.prototype.preview_stopped").to_owned()),
    };
    let current =
        state.desired.as_ref() == Some(&pending.params) && app.session.forms_prototype.as_ref().is_some_and(|f| f.revision == pending.revision);
    let given = pending.params.clone();
    state.pending = None;
    if current {
        state.applied = Some(given);
        match result.and_then(|prepared| app.session.accept_commands(prepared).map_err(|e| e.to_string())) {
            Ok(replies) => {
                state.error = None;
                for (id, reply) in replies {
                    let _ = app.command_completed(&id, Ok(reply));
                }
            }
            Err(error) => state.error = Some(error),
        }
    }
    app.forms_prototype.tool_panel = Some(state);
}
pub(super) fn cancel(app: &mut SolveApp) {
    app.forms_prototype.matching_preview = false;
    app.forms_prototype.other_preview = None;
    if let Some(state) = app.forms_prototype.tool_panel.take()
        && run(app, "form.prototype.tool.cancel", json!({}))
    {
        app.forms_prototype.elements = state.elements;
        app.forms_prototype.pivot = state.pivot;
        selection::sync(app);
    } else if app.session.forms_prototype.as_ref().is_some_and(|f| f.has_tool_preview()) {
        run(app, "form.prototype.tool.cancel", json!({}));
        selection::sync(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipe_ribbon_uses_expression_fields_segment_overrides_and_source_based_cancel() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":20,"width":20,"height":20,"body_name":"Support","operation":"new"})).unwrap();
        let mesh = app.session.model.state().body("Support").unwrap().mesh();
        let edges: BTreeSet<_> = mesh
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.first().is_some_and(|p| p.len() < 1e-8) || e.last().is_some_and(|p| p.len() < 1e-8))
            .map(|(i, _)| i)
            .collect();
        app.start("form.pipe");
        assert!(app.dialog.is_none());
        assert!(matching(&app));
        let original = app.session.forms_prototype.as_ref().unwrap().definition();
        let key = context::key(&app);
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        assert_eq!(context::key(&app), key);
        state.pipe.as_mut().unwrap().diameter = "1 mm + 1 mm".into();
        state.pipe.as_mut().unwrap().all_edges = false;
        state.support_edges = edges;
        let given = params(&app, &state).unwrap();
        assert_eq!(given["pipe"]["diameter"], 2.0);
        app.forms_prototype.tool_panel = Some(state);
        let revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.tool.preview".into(), given.clone())], None).unwrap();
        inject(&mut app, revision, given, prepared);
        tick(&mut app);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.pipe.is_some());
        assert_eq!(form.pipe_segments.len(), 3);
        let segment = form.pipe_segments[0].key.clone();
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        state.pipe.as_mut().unwrap().overrides.insert(segment.clone(), ["3 mm".into(), "4".into()]);
        let given = params(&app, &state).unwrap();
        app.forms_prototype.tool_panel = Some(state);
        let revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.tool.preview".into(), given.clone())], None).unwrap();
        inject(&mut app, revision, given, prepared);
        tick(&mut app);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.pipe.as_ref().unwrap().settings.overrides[&segment].diameter, Some(3.0));
        assert_eq!(form.pipe_segments[0].samples, 4);
        cancel(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
        assert!(app.session.forms_prototype.as_ref().unwrap().pipe_segments.is_empty());
        assert!(!matching(&app));
    }
    #[test]
    fn matching_preview_keeps_context_identity_while_drawing_and_cancels_the_association() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session
            .execute("solid.box", &json!({"length":4,"width":6,"height":8,"corner":[-2,-3,0],"body_name":"Support","operation":"new"}))
            .unwrap();
        let body = app.session.model.state().bodies[0].clone();
        let top = (0..body.body.face_count()).find(|&i| body.body.planar_face(i).is_some_and(|p| p.normal().dot(Vec3::Z) > 0.99)).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,2],"levels":2})).unwrap();
        let original = app.session.forms_prototype.as_ref().unwrap().definition();
        start(&mut app, Mode::Match);
        assert!(matching(&app));
        let key = context::key(&app);
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        assert_eq!(context::key(&app), key, "drawing the tool must not invalidate its context preview");
        state.support_face = top;
        state.match_order = 1;
        state.match_tolerances[1] = "0.1 deg".into();
        let given = params(&app, &state).unwrap();
        assert!((given["matching"]["tangent_deg"].as_f64().unwrap() - 0.1).abs() < 1e-12);
        app.forms_prototype.tool_panel = Some(state);
        let revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.tool.preview".into(), given.clone())], None).unwrap();
        inject(&mut app, revision, given, prepared);
        tick(&mut app);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert!(form.matching.is_some());
        assert!(form.match_report.as_ref().unwrap().position_mm <= 0.001);
        assert_eq!(app.session.doc.features.len(), 1);
        cancel(&mut app);
        assert!(!matching(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
    }
    fn inject(app: &mut SolveApp, revision: u64, given: serde_json::Value, result: solidcraft_engine::ComputedCommands) {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Ok(result)).unwrap();
        let state = app.forms_prototype.tool_panel.as_mut().unwrap();
        state.desired = Some(given.clone());
        state.pending = Some(Pending { revision, params: given, done: rx });
    }
    #[test]
    fn prepared_preview_changes_only_the_form_and_cancel_restores_it() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","levels":1})).unwrap();
        let original = app.session.forms_prototype.as_ref().unwrap().definition();
        start(&mut app, Mode::Thicken);
        let given = json!({"topology":{"tool":"thicken","distance":2.0}});
        let revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.tool.preview".into(), given.clone())], None).unwrap();
        inject(&mut app, revision, given, prepared);
        tick(&mut app);
        let f = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(f.cage.points.len(), original.cage.points.len() * 2);
        assert_eq!(f.inspect()["undo"], 1);
        assert!(app.session.doc.features.is_empty());
        assert!(app.session.undo.is_empty());
        cancel(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
        assert!(!active(&app));
    }
    #[test]
    fn late_results_cannot_replace_a_cancelled_and_reopened_tool() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","levels":1})).unwrap();
        let original = app.session.forms_prototype.as_ref().unwrap().definition();
        start(&mut app, Mode::Thicken);
        let given = json!({"topology":{"tool":"thicken","distance":3.0}});
        let revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.tool.preview".into(), given.clone())], None).unwrap();
        cancel(&mut app);
        start(&mut app, Mode::Thicken);
        inject(&mut app, revision, given, prepared);
        tick(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), original);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], 0);
        assert!(active(&app));
        cancel(&mut app);
    }
}
