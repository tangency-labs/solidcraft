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
    let depth = (point - o).dot(d);
    // The CAD tessellation and exact limit samples differ by their display/fit tolerances.
    // A subpixel depth bias must not hide every surface point or curved boundary.
    let fit = controls::tolerance(app).unwrap_or(0.01);
    let (near, _) = proj.ray(p + egui::vec2(2.0, 0.0));
    let tolerance = (near - o).len().max(3.0 * fit.max(0.05));
    if let Some((mesh, index)) = live_cad::occlusion(app) {
        index.raycast(mesh, o, d).is_none_or(|(t, _)| t >= depth - tolerance)
    } else {
        form.surface.index.raycast(&form.surface.mesh, o, d).is_none_or(|(t, _)| t >= depth - tolerance)
    }
}
pub(super) fn nearest(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<Element> {
    nearest_with(app, proj, pos, app.forms_prototype.pick)
}
pub(super) fn nearest_with(app: &SolveApp, proj: &Proj, pos: Pos2, mode: Pick) -> Option<Element> {
    let form = app.session.forms_prototype.as_ref()?;
    if matches!(mode, Pick::Vertex | Pick::All) {
        let mut candidates: Vec<_> = form
            .surface
            .vertices
            .iter()
            .enumerate()
            .filter_map(|(i, &p)| {
                if form.hierarchy.as_ref().is_some_and(|h| !h.controls.contains_key(&i)) {
                    return None;
                }
                let q = proj.to_screen(p)?;
                let distance = q.distance(pos);
                (distance < 10.0 && visible(app, proj, p)).then_some((distance, (p - proj.cam.eye()).len2(), i))
            })
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        if let Some(&(_, _, v)) = candidates.first() {
            return Some(Element::Vertex(v));
        }
    }
    if matches!(mode, Pick::Edge | Pick::All) {
        let mut best: Option<(f32, [usize; 2])> = None;
        for (i, (edge, initial)) in form.surface.boundaries.iter().enumerate() {
            let curve = boundary_curve(app, i, initial);
            for segment in curve.windows(2) {
                let (Some(a), Some(b)) = (proj.to_screen(segment[0]), proj.to_screen(segment[1])) else { continue };
                let ab = b - a;
                let t = if ab.length_sq() > 1e-6 { ((pos - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                let distance = pos.distance(a + ab * t);
                let point = segment[0] + (segment[1] - segment[0]) * f64::from(t);
                if distance < 9.0 && best.as_ref().is_none_or(|(d, _)| distance < *d) && visible(app, proj, point) {
                    best = Some((distance, *edge));
                }
            }
        }
        if let Some((_, e)) = best {
            return Some(Element::Edge(e));
        }
    }
    if matches!(mode, Pick::Face | Pick::All) {
        let (o, d) = proj.ray(pos);
        if form.profile.is_some() {
            if let Some((mesh, index)) = live_cad::occlusion(app) {
                let (_, triangle) = index.raycast(mesh, o, d)?;
                return mesh.tri_face.get(triangle).map(|&face| if face == u32::MAX { Element::Surface } else { Element::Face(face as usize) });
            }
            let (_, triangle) = form.surface.index.raycast(&form.surface.mesh, o, d)?;
            return form
                .surface
                .mesh
                .tri_face
                .get(triangle)
                .map(|&face| if face == u32::MAX { Element::Surface } else { Element::Face(face as usize) });
        }
        let (_, triangle) = form.surface.index.raycast(&form.surface.mesh, o, d)?;
        return form.surface.mesh.tri_face.get(triangle).map(|&f| Element::Face(f as usize));
    }
    None
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
pub(super) fn overlay(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj) {
    let Some(form) = &app.session.forms_prototype else { return };
    let tokens = Tokens::get();
    app.forms_prototype.projected = form
        .surface
        .vertices
        .iter()
        .enumerate()
        .filter(|(i, _)| form.hierarchy.as_ref().is_none_or(|h| h.controls.contains_key(i)))
        .filter_map(|(i, &p)| visible(app, proj, p).then(|| proj.to_screen(p).map(|p| (i, p))).flatten())
        .collect();
    if app.forms_prototype.cage {
        for ([a, b], _) in &form.surface.boundaries {
            if let (Some(a), Some(b)) = (
                form.cage.points.get(*a).and_then(|p| proj.to_screen(form.world_point(*p))),
                form.cage.points.get(*b).and_then(|p| proj.to_screen(form.world_point(*p))),
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
    for &(i, p) in &app.forms_prototype.projected {
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
