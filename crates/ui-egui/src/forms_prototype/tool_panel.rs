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
#[derive(Clone, Copy)]
enum FillChoices {
    Base,
    Refined { new_cap: bool },
}
impl FillChoices {
    fn options(self) -> &'static [(&'static str, &'static str)] {
        const AUTO: (&str, &str) = ("automatic", "forms.prototype.fill_automatic");
        const SINGLE: (&str, &str) = ("single", "forms.prototype.fill_single");
        match self {
            Self::Base => &[AUTO, SINGLE, ("minimal", "forms.prototype.fill_minimal"), ("reduced_star", "forms.prototype.fill_star")],
            Self::Refined { new_cap: true } => &[AUTO, SINGLE, ("new_cap", "forms.fill.new_cap")],
            Self::Refined { new_cap: false } => &[AUTO, SINGLE],
        }
    }
}

pub(super) struct State {
    repair: bool,
    surface_mapping: usize,
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
    normal: Option<Vec3>,
    distance: String,
    sharp: bool,
    direction: usize,
    controls: bool,
    fill: usize,
    fill_choices: FillChoices,
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
// Entry checks explain known state restrictions; command validation still qualifies the actual geometry.
pub(super) fn topology_mode(tool: &str) -> Option<Mode> {
    match tool {
        "thicken" => Some(Mode::Thicken),
        "match" => Some(Mode::Match),
        "fill_hole" => Some(Mode::Fill),
        "bridge" => Some(Mode::Bridge),
        "weld" => Some(Mode::Weld),
        "unweld" => Some(Mode::Unweld),
        "collapse" => Some(Mode::Collapse),
        "erase_and_fill" => Some(Mode::EraseFill),
        _ => None,
    }
}
fn form_state_unavailable(form: &solidcraft_engine::forms_prototype::Form, mode: Option<Mode>) -> Option<&'static str> {
    let reattachment = (form.profile.is_some() && mode == Some(Mode::Surface)) || (form.matching.is_some() && mode == Some(Mode::Match));
    if form.repair.is_some() && !reattachment {
        return Some("forms.prototype.reattach_before_tool");
    }
    if form.profile.is_some() && !matches!(mode, Some(Mode::Surface | Mode::Flatten | Mode::Straighten)) {
        return Some("forms.prototype.surface_fixed_grid");
    }
    if form.hierarchy.as_ref().is_some_and(|h| h.has_quad_cap()) && matches!(mode, Some(Mode::Bridge | Mode::Match)) {
        return Some("forms.fill.cap_topology_unavailable");
    }
    if mode == Some(Mode::Bridge) && form.hierarchy.as_ref().is_some_and(|h| h.has_quad_cap_history()) {
        return Some("forms.fill.retired_cap_bridge");
    }
    if form.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse())
        && matches!(mode, Some(Mode::Bridge | Mode::Thicken | Mode::Unweld | Mode::Fill | Mode::EraseFill | Mode::Match))
    {
        return Some("forms.collapse.topology_unavailable");
    }
    if mode == Some(Mode::Collapse)
        && let Some(hierarchy) = &form.hierarchy
    {
        if cfg!(target_arch = "wasm32") {
            return Some("forms.collapse.desktop_required");
        }
        if !hierarchy.supports_boundary_collapse()
            || form.symmetry.is_some()
            || form.matching.is_some()
            || form.pipe.is_some()
            || !form.parts.is_empty()
            || form.consumed_features().next().is_some()
        {
            return Some("forms.collapse.source_unavailable");
        }
    }
    if form.hierarchy.is_some() && matches!(mode, Some(Mode::Thicken | Mode::Weld)) {
        return Some("forms.prototype.base_tool_required");
    }
    if mode == Some(Mode::Unweld)
        && let Some(hierarchy) = &form.hierarchy
    {
        if cfg!(target_arch = "wasm32") {
            return Some("forms.unweld.refined_desktop_required");
        }
        if !hierarchy.supports_refined_unweld()
            || form.matching.is_some()
            || form.symmetry.is_some()
            || form.pipe.is_some()
            || !form.parts.is_empty()
            || form.consumed_features().next().is_some()
        {
            return Some("forms.unweld.refined_unavailable");
        }
    }
    if mode == Some(Mode::Bridge) && form.hierarchy.as_ref().is_some_and(|h| h.has_detached_material()) {
        return Some("forms.unweld.bridge_unavailable");
    }
    if mode == Some(Mode::EraseFill)
        && let Some(hierarchy) = &form.hierarchy
    {
        if cfg!(target_arch = "wasm32") {
            return Some("forms.fill.erase_desktop_required");
        }
        if !hierarchy.supports_hole_restoration() || form.matching.is_some() || form.symmetry.is_some() {
            return Some("forms.fill.erase_refined_unavailable");
        }
    }
    if mode == Some(Mode::Fill)
        && let Some(hierarchy) = &form.hierarchy
    {
        if cfg!(target_arch = "wasm32") {
            return Some("forms.fill.desktop_required");
        }
        if !hierarchy.supports_hole_restoration() || form.matching.is_some() {
            return Some("forms.fill.refined_unavailable");
        }
    }
    None
}
pub(super) fn unavailable(app: &SolveApp, mode: Mode) -> Option<&'static str> {
    let Some(form) = &app.session.forms_prototype else { return Some("forms.prototype.active_form_required") };
    if let Some(reason) = form_state_unavailable(form, Some(mode)) {
        return Some(reason);
    }
    if matches!(mode, Mode::Flatten | Mode::Straighten) && surface::cap_selected(app) {
        return Some("forms.fill.cap_rim_editing");
    }
    if mode == Mode::Fill
        && form.hierarchy.is_some()
        && !app.forms_prototype.elements.iter().any(|element| matches!(element, Element::Edge(_) | Element::Vertex(_)))
    {
        return Some("forms.fill.select_rim");
    }
    if mode == Mode::Thicken && controls::source_closed(app) == Some(true) {
        return Some("forms.prototype.open_surface_required");
    }
    if mode == Mode::Collapse
        && (app.forms_prototype.selected.len() != 2
            || (form.hierarchy.is_some() && app.forms_prototype.elements.iter().any(|e| !matches!(e, Element::Vertex(_)))))
    {
        return Some("forms.prototype.two_points_required");
    }
    if mode == Mode::EraseFill {
        if surface::selected_faces(app).is_empty() {
            return Some("forms.prototype.select_faces_for_tool");
        }
        if form.hierarchy.is_some() && app.forms_prototype.elements.iter().any(|element| !matches!(element, Element::Face(_))) {
            return Some("forms.fill.erase_faces_only");
        }
    }
    if mode == Mode::Unweld && (controls::source_closed(app) == Some(false) || tool_selection(app, mode).0.is_empty()) {
        return Some("forms.prototype.unweld_seam_required");
    }
    None
}
pub(super) fn topology_unavailable(app: &SolveApp, tool: &str) -> Option<&'static str> {
    if let Some(mode) = topology_mode(tool) {
        return unavailable(app, mode);
    }
    let Some(form) = &app.session.forms_prototype else { return Some("forms.prototype.active_form_required") };
    if let Some(reason) = form_state_unavailable(form, None) {
        return Some(reason);
    }
    let cap_selected = surface::cap_selected(app);
    if tool == "delete_faces" && cap_selected {
        if app.forms_prototype.elements.len() != 1 {
            return Some("forms.fill.select_cap_only");
        }
        if cfg!(target_arch = "wasm32") {
            return Some("forms.fill.cap_delete_desktop");
        }
    }
    if form.hierarchy.as_ref().is_some_and(|h| h.has_quad_cap())
        && (matches!(tool, "extrude_faces" | "extrude_edges") || (matches!(tool, "subdivide" | "dissolve" | "crease" | "uncrease") && cap_selected))
    {
        return Some("forms.fill.cap_topology_unavailable");
    }
    if matches!(tool, "extrude_faces" | "delete_faces" | "subdivide") && surface::selected_faces(app).is_empty() {
        return Some("forms.prototype.select_faces_for_tool");
    }
    if tool == "extrude_edges" && !app.forms_prototype.elements.iter().any(|element| matches!(element, Element::Edge(_))) {
        return Some("forms.prototype.select_boundary_edges_for_tool");
    }
    None
}
fn tool_selection(app: &SolveApp, mode: Mode) -> (Vec<[usize; 2]>, Vec<usize>) {
    let mut edges: Vec<_> =
        app.forms_prototype.elements.iter().filter_map(|element| if let Element::Edge(edge) = element { Some(*edge) } else { None }).collect();
    let faces: Vec<_> = surface::selected_faces(app).into_iter().collect();
    if mode == Mode::Unweld
        && edges.is_empty()
        && let Some(form) = &app.session.forms_prototype
    {
        let mut counts = std::collections::BTreeMap::new();
        for id in &faces {
            if let Some(face) = form.logical.faces.get(*id) {
                for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                    *counts.entry([a.min(b), a.max(b)]).or_insert(0usize) += 1;
                }
            }
        }
        edges = counts.into_iter().filter_map(|(edge, count)| (count == 1).then_some(edge)).collect();
    }
    (edges, faces)
}
pub(super) fn start(app: &mut SolveApp, mode: Mode) {
    if active(app) {
        return;
    }
    if let Some(reason) = unavailable(app, mode) {
        app.set_status(app.tr(reason).to_owned(), true);
        return;
    }
    let Some(form) = &app.session.forms_prototype else { return };
    let bounds: solidcraft_engine::geom::Aabb3 = form.cage.points.iter().copied().collect();
    let size = bounds.size();
    let distance = (size.x.max(size.y).max(size.z) * 0.1).max(0.1).to_string();
    let local_bridge = mode == Mode::Bridge && form.hierarchy.is_some();
    let vertices = if mode == Mode::Fill && form.hierarchy.is_some() {
        app.forms_prototype.elements.iter().filter_map(|element| if let Element::Vertex(id) = element { Some(*id) } else { None }).collect()
    } else {
        app.forms_prototype.selected.iter().copied().collect()
    };
    let (direction, controls) = controls::projection_settings(app);
    let (edges, faces) = tool_selection(app, mode);
    let first_openings = if mode == Mode::Match {
        solidcraft_engine::doc::forms::match_openings(&form.cage, form.hierarchy.as_ref()).unwrap_or_default()
    } else {
        form.cage.boundary_loops().unwrap_or_default()
    };
    let mut supports = Vec::new();
    let mut support = 0;
    let mut support_face = 0;
    let mut support_world = None;
    if matches!(mode, Mode::Match | Mode::Surface | Mode::Pipe) {
        let before = form.editing_feature.map_or_else(|| app.session.model.state(), |id| app.session.model.state_before(id));
        let world = match solidcraft_engine::doc::world_state(&app.session.doc, &before) {
            Ok(world) => world,
            Err(error) => {
                app.set_status(error.to_string(), true);
                return;
            }
        };
        supports = world
            .bodies
            .iter()
            .filter(|b| !b.body.is_mesh() && b.body.face_count() > 0 && b.body.face_count() <= 4096)
            .filter(|b| mode != Mode::Surface || !b.body.is_surface())
            .take(512)
            .map(|b| (b.name.clone(), b.body.face_count()))
            .collect();
        if (mode != Mode::Pipe && supports.is_empty()) || (mode == Mode::Match && first_openings.is_empty()) {
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
    let edge_matches = |ring: &[usize], edge: [usize; 2]| {
        ring.iter().zip(ring.iter().cycle().skip(1)).any(|(&a, &b)| [a.min(b), a.max(b)] == [edge[0].min(edge[1]), edge[0].max(edge[1])])
    };
    let hierarchical_layout = if mode == Mode::Match { form.hierarchy.as_ref().and_then(|h| h.layout().ok()) } else { None };
    let first_opening = first_openings
        .iter()
        .position(|ring| {
            edges.iter().any(|&edge| {
                edge_matches(ring, edge)
                    || hierarchical_layout.as_ref().is_some_and(|layout| {
                        layout.paths.iter().any(|(logical, path)| {
                            *logical == [edge[0].min(edge[1]), edge[0].max(edge[1])] && path.windows(2).all(|e| edge_matches(ring, [e[0], e[1]]))
                        })
                    })
            })
        })
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
    let mut state = State {
        repair: form.repair.is_some(),
        surface_mapping: 0,
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
        vertices,
        edges,
        faces,
        elements: app.forms_prototype.elements.clone(),
        pivot: app.forms_prototype.pivot,
        normal: basis(app).ok().map(|dirs| dirs[2]),
        distance,
        sharp: true,
        direction,
        controls,
        fill: 0,
        // Preview installs candidates in the live editor. Keep the choices
        // qualified against the original Form for the entire Apply/Cancel tool.
        fill_choices: form
            .hierarchy
            .as_ref()
            .map_or(FillChoices::Base, |hierarchy| FillChoices::Refined { new_cap: form.symmetry.is_none() && hierarchy.can_reserve_quad_cap() }),
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
        let mut given = json!({"surface":{"partition":state.surface_partition,"body":body,"face":state.support_face,"segments":segments,"continuity":state.match_order.min(1)}});
        if state.repair {
            let bits = state.surface_mapping.checked_sub(1).filter(|n| *n < 8).ok_or_else(|| app.tr("forms.repair.choose_mapping").to_owned())?;
            given["surface"]["reattach"] = json!({"swap":bits & 4 != 0,"reverse_u":bits & 1 != 0,"reverse_v":bits & 2 != 0});
        }
        return Ok(given);
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
            4 => Some(state.normal.ok_or_else(|| app.tr("forms.selection_frame.unavailable").to_owned())?),
            _ => None,
        };
        return Ok(json!({"project":{
            "tool":if state.mode==Mode::Flatten{"flatten"}else{"straighten"},"vertices":state.vertices,
            "points":if state.controls{"control"}else{"surface"},"direction":direction.map(|d|[d.x,d.y,d.z]),
            "origin":state.pivot.map(|p|[p.x,p.y,p.z]),
        }}));
    }
    if state.mode == Mode::EraseFill && matches!(state.fill_choices, FillChoices::Refined { .. }) {
        let tolerance = controls::tolerance(app).ok_or_else(|| app.tr("forms.prototype.invalid_tolerance").to_owned())?;
        return Ok(json!({"topology":{"tool":"erase_and_fill","faces":state.faces,"tolerance":tolerance}}));
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
    let &(fill, _) = state.fill_choices.options().get(state.fill).ok_or_else(|| app.tr("forms.prototype.invalid_number").to_owned())?;
    let mut topology = json!({"tool":tool,"distance":distance,"sharp":state.sharp,"fill_mode":fill,
        "vertices":state.vertices,"faces":state.faces,"edges":state.edges});
    let refined = app.session.forms_prototype.as_ref().is_some_and(|form| form.hierarchy.is_some());
    if (state.mode == Mode::Fill && matches!(state.fill_choices, FillChoices::Refined { .. })) || (state.mode == Mode::Collapse && refined) {
        let tolerance = controls::tolerance(app).ok_or_else(|| app.tr("forms.prototype.invalid_tolerance").to_owned())?;
        topology["tolerance"] = json!(tolerance);
    }
    Ok(json!({"topology":topology}))
}
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut state) = app.forms_prototype.tool_panel.take() else {
        ui.label(app.tr("forms.prototype.external_preview"));
        if crate::ui_kit::button(ui, app.tr("forms.prototype.apply"), crate::ui_kit::Button::Enabled(live_cad::mesh(app).is_some())).clicked() {
            run(app, "form.prototype.tool.apply", json!({}));
        }
        if crate::ui_kit::button(ui, app.tr("forms.prototype.cancel"), crate::ui_kit::Button::Normal).clicked() {
            run(app, "form.prototype.tool.cancel", json!({}));
        }
        return;
    };
    ui.heading(app.tr(state.mode.title()));
    if state.mode == Mode::Collapse && app.session.forms_prototype.as_ref().is_some_and(|form| form.hierarchy.is_some()) {
        ui.add(egui::Label::new(app.tr("forms.collapse.help")).wrap());
    }
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
            (0..count).map(|i| solidcraft_ui_text::format("forms.prototype.support_face_index", &[("index", &(i + 1).to_string())])).collect();
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
        let r = crate::ui_kit::button(
            ui,
            app.tr("forms.prototype.pick_support"),
            if state.picking_support { crate::ui_kit::Button::SmallSelected } else { crate::ui_kit::Button::Small },
        );
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
            let r = crate::ui_kit::checkbox(ui, app.tr("forms.prototype.match_wire"), &mut state.match_wire);
            crate::scenario::publish_handle("forms_match_wire", r.rect.center());
            if !state.support_edges.is_empty() {
                ui.label(solidcraft_ui_text::format("forms.prototype.support_edges_selected", &[("count", &state.support_edges.len().to_string())]));
                if crate::ui_kit::button(ui, app.tr("forms.prototype.clear_support_edges"), crate::ui_kit::Button::Normal).clicked() {
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
                    egui::Label::new(solidcraft_ui_text::format(
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
            ui.add_enabled_ui(!state.repair, |ui| {
                for (i, key) in ["forms.prototype.surface_u", "forms.prototype.surface_v"].iter().enumerate() {
                    crate::ui_kit::expr_row(ui, app.tr(key), &mut state.surface_segments[i]);
                }
            });
            if state.repair {
                ui.add(egui::Label::new(app.tr("forms.repair.mapping_help")).wrap());
                let options: Vec<_> = [
                    "forms.repair.choose_mapping",
                    "forms.repair.uv",
                    "forms.repair.reverse_u",
                    "forms.repair.reverse_v",
                    "forms.repair.reverse_both",
                    "forms.repair.swap",
                    "forms.repair.swap_reverse_u",
                    "forms.repair.swap_reverse_v",
                    "forms.repair.swap_reverse_both",
                ]
                .map(|key| app.tr(key).to_owned())
                .into_iter()
                .collect();
                let response = crate::ui_kit::choice_row(
                    ui,
                    app.tr("forms.repair.mapping"),
                    "forms_repair_mapping",
                    &mut state.surface_mapping,
                    &options.iter().map(String::as_str).collect::<Vec<_>>(),
                );
                crate::scenario::publish_handle("forms_repair_mapping", response.rect.center());
            }
            let r = crate::ui_kit::choice_row(
                ui,
                app.tr("forms.prototype.continuity"),
                "forms_surface_continuity",
                &mut state.match_order,
                &[app.tr("forms.prototype.continuity_g0"), app.tr("forms.prototype.continuity_g1")],
            );
            crate::scenario::publish_handle("forms_surface_continuity", r.rect.center());
            let response = crate::ui_kit::checkbox(ui, app.tr("forms.prototype.surface_partition"), &mut state.surface_partition);
            crate::scenario::publish_handle("forms_surface_partition", response.rect.center());
            if response.changed() {
                app.preferences.forms_surface_partition = state.surface_partition;
            }
            ui.add(egui::Label::new(app.tr("forms.prototype.surface_help")).wrap());
        }
    }
    if state.mode == Mode::Unweld {
        if app.session.forms_prototype.as_ref().is_some_and(|form| form.hierarchy.is_some()) {
            ui.add(egui::Label::new(app.tr("forms.unweld.refined_help")).wrap());
        } else {
            let r = crate::ui_kit::checkbox(ui, app.tr("forms.prototype.separate_forms"), &mut state.separate);
            crate::scenario::publish_handle("forms_unweld_separate", r.rect.center());
            ui.add(egui::Label::new(app.tr("forms.prototype.unweld_help")).wrap());
        }
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
        let r = crate::ui_kit::checkbox(ui, app.tr("forms.prototype.sharp_rims"), &mut state.sharp);
        crate::scenario::publish_handle("forms_tool_sharp", r.rect.center());
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.reverse"), crate::ui_kit::Button::Normal);
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
        let r = crate::ui_kit::checkbox(ui, app.tr("forms.prototype.projection_controls"), &mut state.controls);
        crate::scenario::publish_handle("forms_tool_controls", r.rect.center());
    }
    if state.mode == Mode::Fill {
        let options = state.fill_choices.options();
        let labels: Vec<_> = options.iter().map(|(_, key)| app.tr(key).to_owned()).collect();
        let r = crate::ui_kit::choice_row(
            ui,
            app.tr("forms.prototype.fill_mode"),
            "forms_tool_fill",
            &mut state.fill,
            &labels.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        crate::scenario::publish_handle("forms_tool_fill", r.rect.center());
        if matches!(state.fill_choices, FillChoices::Refined { .. }) {
            let help = if options.get(state.fill).is_some_and(|(mode, _)| *mode == "new_cap") {
                "forms.fill.new_cap_help"
            } else {
                "forms.fill.restore_material"
            };
            ui.add(egui::Label::new(app.tr(help)).wrap());
        }
    }
    if state.mode == Mode::EraseFill && matches!(state.fill_choices, FillChoices::Refined { .. }) {
        ui.add(egui::Label::new(app.tr("forms.fill.erase_refined_help")).wrap());
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
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.apply"), crate::ui_kit::Button::Enabled(ready));
        crate::scenario::publish_handle("forms_tool_apply", r.rect.center());
        if r.clicked() && run(app, "form.prototype.tool.apply", json!({})) {
            done = true;
        }
        let r = crate::ui_kit::button(ui, app.tr("forms.prototype.cancel"), crate::ui_kit::Button::Normal);
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
        .map(|(i, l)| {
            solidcraft_ui_text::format("forms.prototype.opening_count", &[("index", &(i + 1).to_string()), ("count", &l.len().to_string())])
        })
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
        && let Some((_, source, face, edge)) = hit.filter(|(_, _, _, edge)| state.mode != Mode::Pipe || edge.is_some())
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
    let refined_erase = state.mode == Mode::EraseFill && matches!(state.fill_choices, FillChoices::Refined { .. });
    let refined_unweld = state.mode == Mode::Unweld && form.hierarchy.is_some();
    let refined_collapse = state.mode == Mode::Collapse && form.hierarchy.is_some();
    let needs_native = refined_erase || refined_unweld || refined_collapse || given["topology"]["fill_mode"] == "new_cap";
    #[cfg(not(target_arch = "wasm32"))]
    {
        let permit = if needs_native {
            let Some(permit) = finish::permit(app) else {
                state.error = Some(app.tr("forms.prototype.command_worker_busy").to_owned());
                let wait = std::time::Duration::from_millis(125);
                state.deadline = ctx.input(|input| input.time) + wait.as_secs_f64();
                ctx.request_repaint_after(wait);
                return;
            };
            Some(permit)
        } else {
            None
        };
        let revision = form.revision;
        let copy = app.session.scratch();
        let wake = ctx.clone();
        let params = given.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        match std::thread::Builder::new().name("form-tool-preview".into()).spawn(move || {
            let _permit = permit;
            let result = copy.compute_commands(&[("form.prototype.tool.preview".into(), params)], None).map_err(|e| e.to_string());
            let _ = tx.send(result);
            wake.request_repaint();
        }) {
            Ok(_) => {
                state.error = None;
                state.pending = Some(Pending { revision, params: given, done: rx });
                return;
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    if needs_native {
        #[cfg(target_arch = "wasm32")]
        {
            let message = if refined_collapse {
                "forms.collapse.desktop_required"
            } else if refined_unweld {
                "forms.unweld.refined_desktop_required"
            } else if refined_erase {
                "forms.fill.erase_desktop_required"
            } else {
                "forms.fill.desktop_required"
            };
            state.error = Some(app.tr(message).to_owned());
        }
        state.applied = Some(given);
        return;
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
    use solidcraft_engine::geom::tess::from_kernel;
    #[test]
    fn tool_entry_rechecks_refinement_and_undo_without_mutating_a_refused_edit() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[1,1,1],"levels":1})).unwrap();
        assert!(unavailable(&app, Mode::Thicken).is_none());
        app.session.execute("form.prototype.refine", &json!({"faces":[0],"counts":[2,2]})).unwrap();
        let before = app.session.forms_prototype.as_ref().unwrap().definition();
        let undo = app.session.forms_prototype.as_ref().unwrap().inspect()["undo"].clone();
        let selection = app.forms_prototype.elements.clone();
        for mode in [Mode::Thicken, Mode::Weld] {
            assert_eq!(unavailable(&app, mode), Some("forms.prototype.base_tool_required"));
            start(&mut app, mode);
            assert!(!active(&app));
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
            assert_eq!(app.forms_prototype.elements, selection);
        }
        let erase_reason = if cfg!(target_arch = "wasm32") { "forms.fill.erase_desktop_required" } else { "forms.prototype.select_faces_for_tool" };
        assert_eq!(unavailable(&app, Mode::EraseFill), Some(erase_reason));
        start(&mut app, Mode::EraseFill);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(app.forms_prototype.elements, selection);
        let fill_reason = if cfg!(target_arch = "wasm32") { "forms.fill.desktop_required" } else { "forms.fill.select_rim" };
        assert_eq!(unavailable(&app, Mode::Fill), Some(fill_reason));
        start(&mut app, Mode::Fill);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(app.forms_prototype.elements, selection);
        for mode in [Mode::Bridge, Mode::Match] {
            assert!(unavailable(&app, mode).is_none(), "supported hierarchy entry keeps its detailed geometry checks");
        }
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert!(unavailable(&app, Mode::Thicken).is_none());
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        start(&mut app, Mode::Thicken);
        assert!(active(&app));
        cancel(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn refined_fill_preview_uses_a_deleted_rim_and_cancel_restores_selection() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"face","dimensions":[9,9,1],"segments":[3,3,1],"levels":0})).unwrap();
        app.run("form.prototype.refine", json!({"faces":[4],"counts":[2,2]})).unwrap();
        let f = app.session.forms_prototype.as_ref().unwrap();
        let intact = f.definition();
        let h = f.hierarchy.as_ref().unwrap();
        let leaf = h.leaves.iter().position(|leaf| leaf.active && leaf.root == 4).unwrap();
        app.run("form.prototype.topology", json!({"tool":"delete_faces","faces":[leaf]})).unwrap();
        let f = app.session.forms_prototype.as_ref().unwrap();
        let hole = f.definition();
        let undo = f.inspect()["undo"].clone();
        let rims = solidcraft_engine::doc::forms::match_openings(&hole.cage, hole.hierarchy.as_ref()).unwrap();
        let rim = rims.iter().find(|rim| rim.len() == 4).unwrap();
        let edge = [rim[0].min(rim[1]), rim[0].max(rim[1])];
        let picked = BTreeSet::from([Element::Edge(edge)]);
        app.forms_prototype.elements = picked.clone();
        selection::sync(&mut app);
        let pivot = Vec3::new(1.0, 2.0, 0.0);
        app.forms_prototype.pivot = Some(pivot);
        assert!(unavailable(&app, Mode::Fill).is_none());
        start(&mut app, Mode::Fill);
        let state = app.forms_prototype.tool_panel.as_ref().unwrap();
        let request = params(&app, state).unwrap();
        assert_eq!(request["topology"]["edges"], json!([edge]));
        assert_eq!(request["topology"]["fill_mode"], "automatic");
        assert_eq!(request["topology"]["tolerance"], 0.05);
        app.run("form.prototype.tool.preview", request).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), intact);
        cancel(&mut app);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), hole);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(app.forms_prototype.elements, picked);
        assert_eq!(app.forms_prototype.pivot, Some(pivot));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn refined_erase_and_fill_worker_cancel_restores_selected_material_and_pivot() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":0})).unwrap();
        app.run("form.prototype.refine", json!({"faces":[10],"counts":[2,2]})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let hierarchy = before.hierarchy.as_ref().unwrap();
        let faces: Vec<_> = hierarchy.leaves.iter().enumerate().filter_map(|(id, leaf)| (leaf.active && leaf.root == 10).then_some(id)).collect();
        let cap = hierarchy.leaves.len();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let picked: BTreeSet<_> = faces.iter().copied().map(Element::Face).collect();
        app.forms_prototype.elements = picked.clone();
        let vertex = form.logical.faces[faces[0]][0];
        app.forms_prototype.elements.insert(Element::Vertex(vertex));
        assert_eq!(unavailable(&app, Mode::EraseFill), Some("forms.fill.erase_faces_only"));
        app.forms_prototype.elements.remove(&Element::Vertex(vertex));
        selection::sync(&mut app);
        let pivot = Vec3::new(1.0, 2.0, 0.0);
        app.forms_prototype.pivot = Some(pivot);
        assert_eq!(unavailable(&app, Mode::EraseFill), None);
        start(&mut app, Mode::EraseFill);
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        let request = params(&app, &state).unwrap();
        assert_eq!(request, json!({"topology":{"tool":"erase_and_fill","faces":faces,"tolerance":0.05}}));
        state.desired = Some(request.clone());
        let ctx = egui::Context::default();
        let permit = finish::permit(&app).unwrap();
        compute(&mut app, &ctx, &mut state, request.clone());
        assert!(state.pending.is_none());
        assert_eq!(state.error.as_deref(), Some(app.tr("forms.prototype.command_worker_busy")));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        drop(permit);
        compute(&mut app, &ctx, &mut state, request);
        let work = state.pending.take().unwrap();
        let result = work.done.recv().unwrap();
        let (tx, done) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        state.pending = Some(Pending { done, ..work });
        app.forms_prototype.tool_panel = Some(state);
        tick(&mut app);
        let state = app.forms_prototype.tool_panel.as_ref().unwrap();
        assert!(state.error.is_none(), "{:?}", state.error);
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.hierarchy.as_ref().unwrap().quad_cap_face(), Some(cap));
        assert_eq!(form.inspect()["undo"], undo + 1);
        assert!(faces.iter().all(|&id| form.logical.faces[id].is_empty()));
        cancel(&mut app);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(app.forms_prototype.elements, picked);
        assert_eq!(app.forms_prototype.pivot, Some(pivot));
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn new_cap_preview_uses_native_worker_and_keeps_rim_selection_on_cancel() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("form.prototype.create", json!({"primitive":"face","dimensions":[8,8,1],"segments":[4,4,1],"levels":0})).unwrap();
        let edge: [usize; 2] = app.session.forms_prototype.as_ref().unwrap().logical.faces[10][..2].try_into().unwrap();
        let edge = [edge[0].min(edge[1]), edge[0].max(edge[1])];
        app.run("form.prototype.topology", json!({"tool":"delete_faces","faces":[10]})).unwrap();
        app.run("form.prototype.refine", json!({"faces":[0],"counts":[2,2]})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let before = form.definition();
        let undo = form.inspect()["undo"].as_u64().unwrap();
        let cap_face = before.hierarchy.as_ref().unwrap().leaves.len();
        let picked = BTreeSet::from([Element::Edge(edge)]);
        app.forms_prototype.elements = picked.clone();
        selection::sync(&mut app);
        let pivot = Vec3::new(1.0, 2.0, 0.0);
        app.forms_prototype.pivot = Some(pivot);
        start(&mut app, Mode::Fill);
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        assert_eq!(params(&app, &state).unwrap()["topology"]["fill_mode"], "automatic");
        state.fill = 2;
        let request = params(&app, &state).unwrap();
        assert_eq!(request["topology"]["fill_mode"], "new_cap");
        assert_eq!(request["topology"]["edges"], json!([edge]));
        state.desired = Some(request.clone());
        let ctx = egui::Context::default();
        let permit = finish::permit(&app).unwrap();
        compute(&mut app, &ctx, &mut state, request.clone());
        assert!(state.pending.is_none() && state.applied.is_none());
        assert_eq!(state.error.as_deref(), Some(app.tr("forms.prototype.command_worker_busy")));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        drop(permit);

        compute(&mut app, &ctx, &mut state, request.clone());
        let work = state.pending.take().unwrap();
        let result = work.done.recv().unwrap();
        let (tx, done) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        state.pending = Some(Pending { done, ..work });
        app.forms_prototype.tool_panel = Some(state);
        tick(&mut app);
        let state = app.forms_prototype.tool_panel.as_ref().unwrap();
        assert!(state.error.is_none(), "{:?}", state.error);
        assert_eq!(state.applied.as_ref(), Some(&request));
        assert_eq!(params(&app, state).unwrap(), request, "a live cap candidate must not remove the selected mode");
        let form = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.hierarchy.as_ref().unwrap().quad_cap_face(), Some(cap_face));
        assert_eq!(form.inspect()["undo"], undo + 1);
        // Apply a copy to exercise current exact region ownership. The live
        // preview remains open so Cancel is tested on the original editor.
        let mut accepted = SolveApp::new(Default::default(), Default::default());
        accepted.session = app.session.scratch();
        accepted.forms_prototype.controls.tolerance.clone_from(&app.forms_prototype.controls.tolerance);
        accepted.run("form.prototype.tool.apply", json!({})).unwrap();
        fit(&mut accepted);
        accepted.cam.pitch = std::f64::consts::FRAC_PI_2;
        accepted.cam.fov = 0.0;
        accepted.forms_prototype.pick = Pick::Face;
        let proj = crate::viewport::projection(&accepted, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let form = accepted.session.forms_prototype.as_ref().unwrap();
        let [a, b, c] = form.surface.mesh.tri(&form.surface.mesh.triangles[0]).unwrap();
        let skin = proj.to_screen((a + b + c) / 3.0).unwrap();
        let (o, d) = proj.ray(skin);
        assert!(form.surface.index.raycast(&form.surface.mesh, o, d).is_some());
        assert!(
            surface::nearest_with(&accepted, &proj, skin, Pick::Face).is_none(),
            "the incomplete skin fallback cannot resolve a face on a capped Form"
        );
        live_cad::install_for_test(&mut accepted);
        let exact = live_cad::selection_geometry(&accepted).unwrap();
        let native_face = exact.faces.iter().position(|owner| *owner == live_cad::ExactFaceOwner::Face(cap_face)).unwrap();
        assert!(exact.logical.as_ref().unwrap().accepts(&BTreeSet::from([Element::Face(cap_face)]), Pick::Face));
        let geometry = solidcraft_engine::doc::kernel::face_geometry(&exact.body, native_face).unwrap();
        let [[u0, u1], [v0, v1]] = exact.body.record().faces[native_face].domain;
        let (center, _, _) = geometry.surface.derivs((u0 + u1) / 2.0, (v0 + v1) / 2.0).unwrap();
        let pixel = proj.to_screen(from_kernel(center)).unwrap();
        assert!(surface::nearest_with(&accepted, &proj, pixel, Pick::Face).is_none(), "a displayed cap cannot acquire a face owner through a click");
        let offset = egui::vec2(3.0, 3.0);
        let region = crate::selection::BoxSel::from_drag(pixel + offset, pixel - offset);
        let selected = region_geometry::Query::capture(&accepted, &proj, &region).unwrap().run().unwrap();
        assert_eq!(selected, BTreeSet::from([Element::Face(cap_face)]));
        accepted.forms_prototype.elements = selected;
        selection::sync(&mut accepted);
        assert!(surface::cap_selected(&accepted));
        assert!(triad(&accepted).is_none());
        let before_delete = accepted.session.forms_prototype.as_ref().unwrap().definition();
        accepted.forms_prototype.elements.insert(Element::Edge(edge));
        assert_eq!(topology_unavailable(&accepted, "delete_faces"), Some("forms.fill.select_cap_only"));
        assert!(shortcut(&mut accepted, &ctx, egui::Key::Delete, egui::Modifiers::NONE));
        assert!(accepted.forms_prototype.pending.is_none());
        assert_eq!(accepted.session.forms_prototype.as_ref().unwrap().definition(), before_delete);
        accepted.forms_prototype.elements.remove(&Element::Edge(edge));
        assert_eq!(topology_unavailable(&accepted, "delete_faces"), None);
        accepted.forms_prototype.pivot = Some(pivot);
        assert!(shortcut(&mut accepted, &ctx, egui::Key::Delete, egui::Modifiers::NONE));
        assert!(accepted.forms_prototype.pending.is_some());
        assert_eq!(accepted.session.forms_prototype.as_ref().unwrap().definition(), before_delete);
        assert_eq!(accepted.forms_prototype.elements, BTreeSet::from([Element::Face(cap_face)]));
        assert_eq!(accepted.forms_prototype.pivot, Some(pivot));
        finish::flush_for_test(&mut accepted).unwrap();
        let form = accepted.session.forms_prototype.as_ref().unwrap();
        assert_eq!(form.inspect()["undo"], undo + 2);
        assert!(form.logical.faces[cap_face].is_empty());
        assert!(accepted.forms_prototype.elements.is_empty());
        assert_eq!(accepted.forms_prototype.pivot, None);
        cancel(&mut app);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), before);
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().inspect()["undo"], undo);
        assert_eq!(app.forms_prototype.elements, picked);
        assert_eq!(app.forms_prototype.pivot, Some(pivot));
    }
    #[test]
    fn closed_form_and_selection_reasons_match_tool_entry() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"size":4,"segments":[1,1,1],"levels":1})).unwrap();
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        assert_eq!(unavailable(&app, Mode::Thicken), Some("forms.prototype.open_surface_required"));
        assert_eq!(unavailable(&app, Mode::Collapse), Some("forms.prototype.two_points_required"));
        assert_eq!(unavailable(&app, Mode::Unweld), Some("forms.prototype.unweld_seam_required"));
        start(&mut app, Mode::Thicken);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        app.forms_prototype.elements.insert(Element::Edge([source.cage.faces[0][0], source.cage.faces[0][1]]));
        selection::sync(&mut app);
        assert!(unavailable(&app, Mode::Collapse).is_none());
        app.forms_prototype.elements.clear();
        app.forms_prototype.elements.insert(Element::Face(0));
        selection::sync(&mut app);
        assert!(unavailable(&app, Mode::Unweld).is_none(), "a face region supplies its separating seam");
    }
    #[test]
    fn retained_match_keeps_reattachment_available_and_blocks_unrelated_tools() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session
            .execute("solid.box", &json!({"corner":[-2,-3,-1],"length":4,"width":6,"height":1,"body_name":"Support","operation":"new"}))
            .unwrap();
        let body = app.session.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|plane| plane.normal().z > 0.99)).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[4,6,1],"segments":[2,2,1],"levels":1})).unwrap();
        let center =
            app.session.forms_prototype.as_ref().unwrap().cage.points.iter().position(|point| point.x.abs() < 1e-8 && point.y.abs() < 1e-8).unwrap();
        app.session.execute("form.prototype.set_vertices", &json!({"vertices":[center],"positions":[[0,0,2]]})).unwrap();
        let ring = app.session.forms_prototype.as_ref().unwrap().cage.boundary_loops().unwrap().remove(0);
        app.session
            .execute("form.prototype.match", &json!({"body":"Support","face":top,"edge":[ring[0],ring[1]],"order":0,"tolerance":0.01}))
            .unwrap();
        let feature = app.session.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let solidcraft_engine::doc::FeatureKind::Form { definition, .. } = &mut app.session.doc_mut().feature_mut(feature).unwrap().kind else {
            panic!("Form")
        };
        let solidcraft_engine::doc::forms::SupportRef::Face { body, .. } = &mut definition.matching.as_mut().unwrap().source else { panic!("face") };
        *body = "Missing support".into();
        app.session.refresh();
        app.session.execute("form.repair", &json!({"feature":feature})).unwrap();
        opened(&mut app);
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        assert!(unavailable(&app, Mode::Match).is_none());
        assert_eq!(unavailable(&app, Mode::Surface), Some("forms.prototype.reattach_before_tool"));
        assert_eq!(topology_unavailable(&app, "extrude_faces"), Some("forms.prototype.reattach_before_tool"));
        start(&mut app, Mode::Thicken);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        start(&mut app, Mode::Match);
        assert!(active(&app));
        cancel(&mut app);
        assert!(app.session.forms_prototype.as_ref().unwrap().repair.is_some());
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
    }
    #[test]
    fn retained_surface_mapping_is_explicit_and_qualification_uses_the_existing_preview_worker() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":4,"width":6,"height":1,"body_name":"Support","operation":"new"})).unwrap();
        let body = app.session.model.state().body("Support").unwrap().body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
        assert!(unavailable(&app, Mode::Surface).is_none());
        assert!(unavailable(&app, Mode::Flatten).is_none());
        assert!(unavailable(&app, Mode::Straighten).is_none());
        assert_eq!(unavailable(&app, Mode::Match), Some("forms.prototype.surface_fixed_grid"));
        assert_eq!(topology_unavailable(&app, "extrude_faces"), Some("forms.prototype.surface_fixed_grid"));
        let feature = app.session.execute("form.prototype.commit", &json!({"tolerance":0.01})).unwrap()["feature"].as_u64().unwrap();
        let solidcraft_engine::doc::FeatureKind::Form { definition, .. } = &mut app.session.doc_mut().feature_mut(feature).unwrap().kind else {
            panic!("Form")
        };
        let solidcraft_engine::doc::forms::SupportRef::Face { body, .. } = &mut definition.profile.as_mut().unwrap().source else { panic!("face") };
        *body = "Missing support".into();
        app.session.refresh();
        app.session.execute("form.repair", &json!({"feature":feature})).unwrap();
        opened(&mut app);
        let source = app.session.forms_prototype.as_ref().unwrap().definition();
        assert!(unavailable(&app, Mode::Surface).is_none());
        assert_eq!(unavailable(&app, Mode::Flatten), Some("forms.prototype.reattach_before_tool"));
        assert_eq!(topology_unavailable(&app, "thicken"), Some("forms.prototype.reattach_before_tool"));
        start(&mut app, Mode::Thicken);
        assert!(!active(&app));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        start(&mut app, Mode::Surface);
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        assert!(params(&app, &state).is_err());
        assert_eq!(state.surface_segments, ["3", "2"]);
        state.support_face = top;
        state.surface_mapping = 1;
        let given = params(&app, &state).unwrap();
        assert_eq!(given["surface"]["reattach"], json!({"swap":false,"reverse_u":false,"reverse_v":false}));
        app.forms_prototype.tool_panel = Some(state);
        let revision = app.session.forms_prototype.as_ref().unwrap().revision;
        let prepared = app.session.scratch().compute_commands(&[("form.prototype.tool.preview".into(), given.clone())], None).unwrap();
        inject(&mut app, revision, given, prepared);
        tick(&mut app);
        assert!(app.forms_prototype.tool_panel.as_ref().unwrap().error.is_none());
        assert!(app.session.forms_prototype.as_ref().unwrap().repair.is_some());
        assert!(run(&mut app, "form.prototype.tool.apply", json!({})), "the accepted worker result carries the qualified source identity");
        assert!(app.session.forms_prototype.as_ref().unwrap().repair.is_none());
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), source);
        assert!(app.session.forms_prototype.as_ref().unwrap().repair.is_some());
    }
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
    fn pipe_panel_requires_reselection_for_missing_saved_sketches_and_edges() {
        use solidcraft_engine::doc::forms::{NamedEdge, Pipe, PipeSource};
        use solidcraft_engine::geom::subdivision::pipe_network::PipeSettings;

        let mut app = SolveApp::new(Default::default(), Default::default());
        app.run("solid.box", json!({"length":20,"width":20,"height":20,"body_name":"Support"})).unwrap();
        app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        app.run("sketch.line", json!({"points":[[0,0],[20,0]]})).unwrap();
        app.finish_sketch();
        app.run("form.prototype.create", json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();

        let missing_sketch = PipeSource::Sketch { sketch: u64::MAX.into(), curves: vec!["missing".into()] };
        app.session.forms_prototype.as_mut().unwrap().pipe =
            Some(Box::new(Pipe { source: missing_sketch, settings: PipeSettings::default(), follow_path: true }));
        start(&mut app, Mode::Pipe);
        let source_required = app.tr("forms.prototype.pipe_source_required").to_owned();
        let state = app.forms_prototype.tool_panel.take().unwrap();
        assert_eq!(params(&app, &state).unwrap_err(), source_required);
        cancel(&mut app);

        let model = app.session.model.state();
        let PipeSource::Edges { body, mut edges } = PipeSource::from_edges(model.body("Support").unwrap(), &[0]).unwrap() else {
            panic!("expected a CAD edge source");
        };
        edges.push(NamedEdge { name: "missing edge".into(), pick: Vec3::new(1000.0, 1000.0, 1000.0) });
        let missing_edge = PipeSource::Edges { body, edges };
        app.session.forms_prototype.as_mut().unwrap().pipe =
            Some(Box::new(Pipe { source: missing_edge.clone(), settings: PipeSettings::default(), follow_path: true }));
        start(&mut app, Mode::Pipe);
        let state = app.forms_prototype.tool_panel.take().unwrap();
        assert_eq!(params(&app, &state).unwrap_err(), source_required);
        cancel(&mut app);

        app.session.forms_prototype.as_mut().unwrap().pipe =
            Some(Box::new(Pipe { source: missing_edge, settings: PipeSettings::default(), follow_path: false }));
        start(&mut app, Mode::Pipe);
        let mut state = app.forms_prototype.tool_panel.take().unwrap();
        let values = params(&app, &state).unwrap();
        assert!(values["pipe"].get("body").is_none(), "a sculpted cage can keep its original association");
        pipe_panel::set_regenerate_for_test(&mut state);
        assert_eq!(params(&app, &state).unwrap_err(), source_required);
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
