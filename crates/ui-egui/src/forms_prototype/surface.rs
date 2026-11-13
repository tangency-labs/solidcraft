//! Picking and feedback use evaluated logical elements, never tessellation triangle IDs.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Element {
    Vertex(usize),
    Edge([usize; 2]),
    Face(usize),
    Surface,
}
impl Element {
    pub fn vertices(self, cage: &solidcraft_engine::geom::subdivision::Cage) -> Vec<usize> {
        match self {
            Self::Vertex(v) => vec![v],
            Self::Edge(e) => e.to_vec(),
            Self::Face(f) => cage.faces.get(f).map(|v| v.to_vec()).unwrap_or_default(),
            Self::Surface => (0..cage.points.len()).collect(),
        }
    }
}
pub(super) fn visible(app: &SolveApp, proj: &Proj, point: Vec3) -> bool {
    let Some(form) = &app.session.forms_prototype else { return false };
    let Some(p) = proj.to_screen(point) else { return false };
    let (o, d) = proj.ray(p);
    // The CAD tessellation and exact limit samples differ by their display/fit tolerances.
    // A subpixel depth bias must not hide every surface point or curved boundary.
    let fit = controls::tolerance(app).unwrap_or(0.01);
    let (near, _) = proj.ray(p + egui::vec2(2.0, 0.0));
    let Some(visibility) = solidcore_ops::forms_view_queries::visibility(point, o, d, near, fit, proj.cam.fov > 0.0) else { return false };
    let hit = if let Some((mesh, index)) = live_cad::occlusion(app) {
        index.raycast(mesh, o, d)
    } else {
        form.surface.index.raycast(&form.surface.mesh, o, d)
    };
    // Test the exact world point rather than a ray quantized through f32 screen
    // coordinates: a touching support rim must not become a false nearer wall.
    let (context_origin, context_direction, context_depth) = visibility.context_ray();
    visibility.own_surface_visible(hit.map(|(depth, _)| depth)) && !context::occludes(app, context_origin, context_direction, context_depth)
}
pub(super) fn nearest(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<Element> {
    nearest_with(app, proj, pos, app.forms_prototype.pick)
}
pub(super) fn nearest_with(app: &SolveApp, proj: &Proj, pos: Pos2, mode: Pick) -> Option<Element> {
    let form = app.session.forms_prototype.as_ref()?;
    let retained_collapse = form.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse());
    if matches!(mode, Pick::Vertex | Pick::All) {
        let mut candidates: Vec<_> = form
            .surface
            .vertices
            .iter()
            .enumerate()
            .filter_map(|(i, &p)| {
                if !form.selectable_control(i) || !live_cad::handle_available(app, i) {
                    return None;
                }
                let p = control_position(app, i, p)?;
                let q = proj.to_screen(p)?;
                let distance = q.distance(pos);
                (distance < 10.0 && visible(app, proj, p)).then(|| (distance, solidcore_ops::forms_view_queries::distance2(p, proj.cam.eye()), i))
            })
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if let Some(&(_, _, v)) = candidates.first() {
            return Some(Element::Vertex(v));
        }
    }
    if matches!(mode, Pick::Edge | Pick::All) {
        if curves::exact_picking(app) {
            if let Some((edge, _, _)) = curves::nearest(app, proj, pos) {
                return Some(Element::Edge(edge));
            }
        } else if !retained_collapse {
            let projection = curves::projection(proj);
            let mut best: Option<(f32, [usize; 2])> = None;
            for (i, (edge, initial)) in form.surface.boundaries.iter().enumerate() {
                let curve = boundary_curve(app, i, initial);
                for segment in curve.windows(2) {
                    let Some((distance2, point)) =
                        solidcore_ops::forms_view_queries::segment_hit(projection, [pos.x, pos.y], [segment[0], segment[1]])
                    else {
                        continue;
                    };
                    if distance2 < curves::HIT_PIXELS * curves::HIT_PIXELS
                        && best.as_ref().is_none_or(|(d, _)| distance2 < *d)
                        && visible(app, proj, point)
                    {
                        best = Some((distance2, *edge));
                    }
                }
            }
            if let Some((_, e)) = best {
                return Some(Element::Edge(e));
            }
        }
    }
    if matches!(mode, Pick::Face | Pick::All) {
        // The evaluator omits derived material. Picking from it could
        // select a face behind an exact native cap or Collapse trim.
        if retained_collapse || form.hierarchy.as_ref().is_some_and(|h| h.has_quad_cap()) {
            return None;
        }
        let (o, d) = proj.ray(pos);
        let (mesh, index) = if form.profile.is_some() {
            live_cad::occlusion(app).unwrap_or((&form.surface.mesh, &form.surface.index))
        } else {
            (&form.surface.mesh, &form.surface.index)
        };
        let (depth, triangle) = index.raycast(mesh, o, d)?;
        if context::occludes(app, o, d, depth) {
            return None;
        }
        return mesh
            .tri_face
            .get(triangle)
            .map(|&face| if form.profile.is_some() && face == u32::MAX { Element::Surface } else { Element::Face(face as usize) });
    }
    None
}
pub(super) fn selected_cap(app: &SolveApp) -> Option<usize> {
    let hierarchy = app.session.forms_prototype.as_ref()?.hierarchy.as_ref()?;
    app.forms_prototype.elements.iter().find_map(|element| match *element {
        Element::Face(face) if hierarchy.is_quad_cap_face(face) => Some(face),
        _ => None,
    })
}
pub(super) fn cap_selected(app: &SolveApp) -> bool {
    selected_cap(app).is_some()
}
pub(super) fn selected_faces(app: &SolveApp) -> BTreeSet<usize> {
    let mut faces: BTreeSet<_> = app
        .forms_prototype
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Face(f) => Some(*f),
            Element::Surface => Some(u32::MAX as usize),
            _ => None,
        })
        .collect();
    if app.forms_prototype.elements.contains(&Element::Surface)
        && let Some(form) = &app.session.forms_prototype
    {
        faces.extend(0..form.logical.faces.len());
    }
    faces
}
pub(super) fn boundary_curve<'a>(app: &'a SolveApp, index: usize, initial: &'a Vec<Vec3>) -> &'a Vec<Vec3> {
    if app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some()) {
        live_cad::mesh(app).and_then(|m| m.edges.get(index)).unwrap_or(initial)
    } else {
        initial
    }
}
pub(super) fn highlight_curve(app: &SolveApp, edge: [usize; 2]) -> Option<Vec<Vec3>> {
    if !curves::exact_picking(app) {
        return None;
    }
    let curve = curves::get(app, edge)?;
    (0..=64).map(|i| curve.at(i as f64 / 64.0)).collect::<Result<_, _>>().ok()
}
fn control_position(app: &SolveApp, id: usize, evaluated: Vec3) -> Option<Vec3> {
    let form = app.session.forms_prototype.as_ref()?;
    if form.hierarchy.as_ref().is_some_and(|h| h.has_boundary_collapse()) {
        return live_cad::selection_geometry(app)?.points.as_ref().ok()?.get(&id).copied();
    }
    Some(evaluated)
}

pub(super) fn overlay(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    let Some(form) = &app.session.forms_prototype else { return };
    let tokens = Tokens::get();
    if extrusion::preview(app).is_some() {
        // Pending topology is display-only and cannot provide new pick handles.
        app.forms_prototype.projected.clear();
        return;
    }
    app.forms_prototype.projected = form
        .surface
        .vertices
        .iter()
        .enumerate()
        .filter(|(i, _)| form.selectable_control(*i) && live_cad::handle_available(app, *i))
        .filter_map(|(i, &p)| {
            let p = control_position(app, i, p)?;
            visible(app, proj, p).then(|| proj.to_screen(p).map(|screen| (i, p, screen))).flatten()
        })
        .collect();
    if app.forms_prototype.cage {
        for ([a, b], _) in &form.surface.boundaries {
            if let (Some(a), Some(b)) = (
                form.logical.points.get(*a).and_then(|p| proj.to_screen(form.world_point(*p))),
                form.logical.points.get(*b).and_then(|p| proj.to_screen(form.world_point(*p))),
            ) {
                ui.painter().line_segment([a, b], Stroke::new(0.8, tokens.grid_major));
            }
        }
    }
    for (i, (edge, initial)) in form.surface.boundaries.iter().enumerate() {
        let curve = boundary_curve(app, i, initial);
        let selected = app.forms_prototype.elements.contains(&Element::Edge(*edge));
        let hovered = app.forms_prototype.hover == Some(Element::Edge(*edge));
        if selected || hovered {
            let exact = highlight_curve(app, *edge);
            let curve = exact.as_ref().unwrap_or(curve);
            for pair in curve.windows(2) {
                if !visible(app, proj, (pair[0] + pair[1]) * 0.5) {
                    continue;
                }
                if let (Some(a), Some(b)) = (proj.to_screen(pair[0]), proj.to_screen(pair[1])) {
                    ui.painter().line_segment([a, b], Stroke::new(2.5, if selected { tokens.sel_edge } else { tokens.hover_edge }));
                }
            }
        }
    }
    for &(i, _, p) in &app.forms_prototype.projected {
        let selected = app.forms_prototype.selected.contains(&i);
        let hover = app.forms_prototype.hover == Some(Element::Vertex(i));
        if !(form.pipe.is_some() && tool_panel::active(app))
            && (selected || hover || app.forms_prototype.points || app.forms_prototype.pick == Pick::Vertex)
        {
            let color = if selected { tokens.sel_vertex } else { tokens.body_edge };
            ui.painter().circle_filled(p, if selected || hover { 5.0 } else { 3.5 }, color);
            ui.painter().circle_stroke(p, if selected || hover { 5.0 } else { 3.5 }, Stroke::new(1.0, tokens.viewport_top));
        }
    }
}
