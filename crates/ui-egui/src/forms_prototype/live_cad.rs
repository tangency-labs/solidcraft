//! A release preview of the actual fitted CAD body, separate from saved design state.
use super::*;
type Outcome = Result<(Mesh, solidcraft_engine::doc::forms::ConversionReport), String>;
struct Source {
    document: Arc<solidcraft_engine::doc::Document>,
    before: Arc<solidcraft_engine::doc::ModelState>,
    moves: std::collections::BTreeMap<u64, solidcraft_engine::doc::Mat>,
}
fn compute_context(definition: solidcraft_engine::doc::forms::Definition, tolerance: f64, source: Option<Source>) -> Outcome {
    if let Some(profile) = &definition.profile {
        let source = source.ok_or("Form Surface preview requires its source document")?;
        let mut document = source.document.as_ref().clone();
        for occurrence in &mut document.occurrences {
            if let Some(matrix) = source.moves.get(&occurrence.id) {
                occurrence.transform = *matrix;
            }
        }
        let state = solidcraft_engine::doc::world_state(&document, &source.before);
        return profile.display(&state, &definition.cage, &definition.placement, tolerance).map_err(|e| e.to_string());
    }
    compute(definition, tolerance)
}
fn key(app: &SolveApp, revision: u64, tolerance: f64) -> (u64, u64) {
    let revision = if app.session.forms_prototype.as_ref().is_some_and(|f| f.profile.is_some()) {
        revision ^ context::key(app).rotate_left(17)
    } else {
        revision
    };
    (revision, tolerance.to_bits())
}
struct Pending {
    key: (u64, u64),
    source: solidcraft_engine::doc::forms::Definition,
    done: std::sync::mpsc::Receiver<Outcome>,
}
#[derive(Default)]
pub(super) struct State {
    source: Option<solidcraft_engine::doc::forms::Definition>,
    placement_reuses: u64,
    pending: Option<Pending>,
    requested: Option<(u64, u64)>,
    ready: Option<((u64, u64), Mesh, solidcraft_engine::doc::forms::ConversionReport)>,
    index: Option<solidcraft_engine::geom::MeshIndex>,
    pub(super) error: Option<String>,
}
pub(super) fn compute(definition: solidcraft_engine::doc::forms::Definition, tolerance: f64) -> Outcome {
    let (body, report) = solidcraft_engine::doc::forms::convert(&definition, tolerance).map_err(|e| e.to_string())?;
    let mut mesh = body.display_mesh(tolerance.min(0.05)).map_err(|e| e.to_string())?;
    if mesh.triangles.is_empty() || mesh.tri_face.len() != mesh.triangles.len() {
        return Err("the CAD preview has no usable face tessellation".into());
    }
    for face in &mut mesh.tri_face {
        *face = *report.patch_roots.get(*face as usize).ok_or("the CAD preview lost a logical Form face")? as u32;
    }
    mesh.seams.resize(mesh.edges.len(), false);
    for (i, faces) in mesh.edge_faces.iter_mut().enumerate() {
        let interior = faces.len() > 1;
        for face in faces.iter_mut() {
            *face = *report.patch_roots.get(*face as usize).ok_or("the CAD preview lost a logical edge neighbour")? as u32;
        }
        faces.sort_unstable();
        faces.dedup();
        if let Some(seam) = mesh.seams.get_mut(i) {
            *seam = interior && faces.len() == 1;
        }
    }
    Ok((mesh, report))
}
pub(super) fn mesh(app: &SolveApp) -> Option<&Mesh> {
    let form = app.session.forms_prototype.as_ref()?;
    let tolerance = controls::tolerance(app)?;
    app.forms_prototype.live.ready.as_ref().filter(|(given, _, _)| *given == key(app, form.revision, tolerance)).map(|(_, m, _)| m)
}
pub(super) fn report(app: &SolveApp) -> Option<&solidcraft_engine::doc::forms::ConversionReport> {
    mesh(app)?;
    app.forms_prototype.live.ready.as_ref().map(|(_, _, report)| report)
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    let live = &app.forms_prototype.live;
    let ready = mesh(app).is_some();
    json!({"kind":if ready{"cad"}else{"subdivision"},"busy":live.pending.is_some(),"error":live.error,"placement_reuses":live.placement_reuses,"fit":if ready{live.ready.as_ref().map(|(_,_,r)|r)}else{None}})
}
fn same_shape(form: &solidcraft_engine::forms_prototype::Form, source: &solidcraft_engine::doc::forms::Definition) -> bool {
    form.profile.is_none() && form.cage == source.cage && form.hierarchy == source.hierarchy && form.matching == source.matching
}
fn place_mesh(mesh: &mut Mesh, old: &solidcraft_engine::doc::forms::Placement, new: &solidcraft_engine::doc::forms::Placement) {
    for p in &mut mesh.positions {
        *p = new.point(old.local_point(*p));
    }
    for n in &mut mesh.normals {
        *n = new.vector(old.local_vector(*n));
    }
    for edge in &mut mesh.edges {
        for p in edge {
            *p = new.point(old.local_point(*p));
        }
    }
}
pub(super) fn busy(app: &SolveApp) -> bool {
    app.forms_prototype.live.pending.is_some()
}
pub(super) fn occlusion(app: &SolveApp) -> Option<(&Mesh, &solidcraft_engine::geom::MeshIndex)> {
    Some((mesh(app)?, app.forms_prototype.live.index.as_ref()?))
}
pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(form) = &app.session.forms_prototype else {
        return;
    };
    let Some(tolerance) = controls::tolerance(app) else {
        return;
    };
    if !tolerance.is_finite() || !(1e-6..=1.0).contains(&tolerance) {
        return;
    }
    let key = key(app, form.revision, tolerance);
    let live = &mut app.forms_prototype.live;
    if let Some(source) = &mut live.source
        && let Some((previous, mesh, _)) = &mut live.ready
        && *previous != key
        && previous.1 == key.1
        && same_shape(form, source)
    {
        place_mesh(mesh, &source.placement, &form.placement);
        source.placement = form.placement.clone();
        *previous = key;
        live.index = Some(solidcraft_engine::geom::MeshIndex::new(mesh));
        live.requested = Some(key);
        live.placement_reuses = live.placement_reuses.saturating_add(1);
        app.forms_prototype.uploaded = 0;
    }
    if live.requested != Some(key) {
        live.error = None;
    }
    if let Some(pending) = &live.pending {
        let result = match pending.done.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("CAD preview computation stopped".into())),
        };
        if let Some(result) = result {
            if pending.key == key || (pending.key.1 == key.1 && same_shape(form, &pending.source)) {
                match result {
                    Ok((mut mesh, report)) => {
                        if pending.source.placement != form.placement {
                            place_mesh(&mut mesh, &pending.source.placement, &form.placement);
                            live.placement_reuses = live.placement_reuses.saturating_add(1);
                        }
                        live.index = Some(solidcraft_engine::geom::MeshIndex::new(&mesh));
                        live.ready = Some((key, mesh, report));
                        let mut source = pending.source.clone();
                        source.placement = form.placement.clone();
                        live.source = Some(source);
                        live.requested = Some(key);
                        live.error = None;
                    }
                    Err(e) => {
                        live.error = Some(e);
                        live.ready = None;
                    }
                }
                app.forms_prototype.uploaded = 0;
            }
            live.pending = None;
        }
    }
    let interacting = app.forms_prototype.drag.is_some() || app.forms_prototype.gizmo_drag.is_some() || ctx.input(|i| i.pointer.primary_down());
    // Native fits run on a worker. Browser fits still run synchronously, so never start
    // one from a pointer gesture there until a browser worker is available.
    let wait_for_release = interacting && (app.preferences.forms_mesh_while_dragging || cfg!(target_arch = "wasm32"));
    if live.pending.is_some() || live.requested == Some(key) || wait_for_release || app.forms_prototype.pending.is_some() {
        return;
    }
    let definition = form.definition();
    let source = definition.clone();
    let context = definition.profile.as_ref().map(|_| Source {
        document: app.session.doc.clone(),
        before: form.editing_feature.map_or_else(|| app.session.model.state(), |id| app.session.model.state_before(id)),
        moves: app.session.pending_moves.clone(),
    });
    live.requested = Some(key);
    live.error = None;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        match std::thread::Builder::new().name("form-cad-preview".into()).spawn(move || {
            let result = compute_context(definition, tolerance, context);
            let _ = tx.send(result);
            wake.request_repaint();
        }) {
            Ok(_) => live.pending = Some(Pending { key, source, done: rx }),
            Err(e) => live.error = Some(e.to_string()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    match compute_context(definition, tolerance, context) {
        Ok((mesh, report)) => {
            live.index = Some(solidcraft_engine::geom::MeshIndex::new(&mesh));
            live.ready = Some((key, mesh, report));
            live.source = Some(source);
            app.forms_prototype.uploaded = 0;
        }
        Err(e) => live.error = Some(e),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_preview_uses_filled_cad_and_projects_guide_edges_on_that_surface() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top})).unwrap();
        let p = app.session.forms_prototype.as_ref().unwrap().surface.vertices[4] + Vec3::Z;
        app.session.execute("form.prototype.set_surface_vertices", &json!({"vertices":[4],"positions":[[p.x,p.y,p.z]]})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let definition = form.definition();
        let source = Source { document: app.session.doc.clone(), before: app.session.model.state(), moves: Default::default() };
        let (mesh, report) = compute_context(definition, 0.01, Some(source)).unwrap();
        assert_eq!(mesh.normals.len(), mesh.positions.len());
        assert_eq!(mesh.edges.len(), form.surface.boundaries.len());
        assert!(mesh.edges.iter().all(|e| e.len() == 25));
        assert!(report.sampled_deviation_mm <= 0.01);
        assert_eq!(report.patches, 4);
        assert!(mesh.edge_faces.iter().all(|faces| !faces.is_empty() && faces.iter().all(|&id| id < 4)));
        assert!(mesh.edge_faces.iter().any(|faces| faces.len() == 2));
        assert!(mesh.edges.iter().any(|e| e[e.len() / 2].dist_to_segment(e[0], *e.last().unwrap()) > 1e-3));
        let tolerance = controls::tolerance(&app).unwrap();
        let current = key(&app, form.revision, tolerance);
        app.forms_prototype.live.index = Some(solidcraft_engine::geom::MeshIndex::new(&mesh));
        app.forms_prototype.live.ready = Some((current, mesh, report));
        fit(&mut app);
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let point = app.session.forms_prototype.as_ref().unwrap().surface.vertices[4];
        let screen = proj.to_screen(point).unwrap();
        assert!(matches!(surface::nearest_with(&app, &proj, screen, Pick::Face), Some(Element::Face(0..=3))));
        assert_eq!(surface::nearest_with(&app, &proj, screen, Pick::Vertex), Some(Element::Vertex(4)));
        app.forms_prototype.elements = [Element::Surface].into_iter().collect();
        selection::sync(&mut app);
        assert_eq!(app.forms_prototype.selected.len(), 9);
    }
    #[test]
    fn live_cad_drag_preference_keeps_the_fast_path_and_can_request_a_fit_while_held() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        assert!(app.preferences.forms_mesh_while_dragging);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: Pos2::new(30.0, 30.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |_| {
            assert!(ctx.input(|i| i.pointer.primary_down()));
            tick(&mut app, &ctx);
        });
        assert!(app.forms_prototype.live.pending.is_none());
        app.preferences.forms_mesh_while_dragging = false;
        let _ = ctx.run_ui(Default::default(), |_| tick(&mut app, &ctx));
        assert!(app.forms_prototype.live.pending.is_some());
        let f = app.session.forms_prototype.as_ref().unwrap();
        assert_eq!(app.forms_prototype.live.requested, Some((f.revision, 0.01_f64.to_bits())));
        assert_eq!(f.inspect()["undo"], 0);
    }
    #[test]
    fn a_rigid_move_reuses_fitted_cad_without_starting_a_worker() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({"segments":[1,1,1],"levels":1})).unwrap();
        let form = app.session.forms_prototype.as_ref().unwrap();
        let source = form.definition();
        let (cad_mesh, report) = compute(source.clone(), 0.01).unwrap();
        let original = cad_mesh.positions.clone();
        let key = (form.revision, 0.01_f64.to_bits());
        app.forms_prototype.live.source = Some(source);
        app.forms_prototype.live.index = Some(solidcraft_engine::geom::MeshIndex::new(&cad_mesh));
        app.forms_prototype.live.ready = Some((key, cad_mesh, report));
        app.forms_prototype.live.requested = Some(key);
        let matrix = solidcraft_engine::doc::rigid(Vec3::new(30.0, 20.0, 10.0), Vec3::ZERO, Vec3::X, 0.4);
        app.session.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
        let ctx = egui::Context::default();
        tick(&mut app, &ctx);
        assert!(app.forms_prototype.live.pending.is_none());
        assert_eq!(app.forms_prototype.live.placement_reuses, 1);
        let displayed = mesh(&app).unwrap();
        for (p, q) in original.iter().zip(&displayed.positions) {
            assert!(solidcraft_engine::doc::apply_point(&matrix, *p).dist(*q) < 1e-9);
        }
        app.session.execute("form.prototype.undo", &json!({})).unwrap();
        tick(&mut app, &ctx);
        assert!(app.forms_prototype.live.pending.is_none());
        let displayed = mesh(&app).unwrap();
        for (p, q) in original.iter().zip(&displayed.positions) {
            assert!(p.dist(*q) < 1e-9);
        }
    }
    #[test]
    fn cad_display_keeps_surface_vertices_and_edges_pickable() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &json!({})).unwrap();
        fit(&mut app);
        let f = app.session.forms_prototype.as_ref().unwrap();
        let (mesh, report) = compute(f.definition(), 0.01).unwrap();
        let key = (f.revision, 0.01_f64.to_bits());
        app.forms_prototype.live.index = Some(solidcraft_engine::geom::MeshIndex::new(&mesh));
        app.forms_prototype.live.ready = Some((key, mesh, report));
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let f = app.session.forms_prototype.as_ref().unwrap();
        let mut vertices = 0;
        let mut edges = 0;
        for (id, &p) in f.surface.vertices.iter().enumerate() {
            if surface::visible(&app, &proj, p) {
                let pos = proj.to_screen(p).unwrap();
                assert_eq!(surface::nearest_with(&app, &proj, pos, Pick::Vertex), Some(Element::Vertex(id)));
                vertices += 1;
            }
        }
        for (edge, curve) in &f.surface.boundaries {
            let p = curve[curve.len() / 2];
            if surface::visible(&app, &proj, p) {
                let pos = proj.to_screen(p).unwrap();
                assert_eq!(surface::nearest_with(&app, &proj, pos, Pick::Edge), Some(Element::Edge(*edge)));
                edges += 1;
            }
        }
        assert!(vertices >= 4 && edges >= 4);
    }
    #[test]
    fn release_preview_has_a_cad_face_id_for_every_triangle() {
        let parameters =
            solidcraft_engine::geom::subdivision::primitives::Parameters { segments: [1; 3], dimensions: [10.; 3], ..Default::default() };
        let definition = solidcraft_engine::doc::forms::Definition {
            imported: None,
            pipe: None,
            profile: None,
            matching: None,
            placement: Default::default(),
            cage: parameters.build().unwrap(),
            parameters,
            symmetry: None,
            hierarchy: None,
        };
        let (mesh, report) = compute(definition, 0.05).unwrap();
        assert_eq!(report.patches, 6);
        assert!(report.solid);
        assert_eq!(mesh.tri_face.iter().copied().collect::<BTreeSet<_>>(), (0..6).collect());
        assert_eq!(mesh.edges.iter().enumerate().filter(|(i, _)| !mesh.seams.get(*i).copied().unwrap_or(false)).count(), 12);
    }
}
