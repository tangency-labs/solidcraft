//! Document context and staged companion Forms are meshed away from the window.
use super::*;
use solidcraft_engine::{Session, doc};

pub(super) struct Entry {
    pub name: Option<String>,
    pub index: Option<solidcraft_engine::geom::MeshIndex>,
    pub mesh: Arc<Mesh>,
    pub color: Option<[f32; 3]>,
}
struct Pending {
    key: u64,
    done: std::sync::mpsc::Receiver<Result<Vec<Entry>, String>>,
}
#[derive(Default)]
pub(super) struct State {
    _document: Option<Arc<doc::Document>>,
    _parts: Vec<Arc<doc::forms::Definition>>,
    desired: Option<u64>,
    requested: Option<u64>,
    ready: Option<u64>,
    pending: Option<Pending>,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
    pub version: u64,
}
pub(super) fn key(app: &SolveApp) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    tool_panel::matching(app).hash(&mut hash);
    (Arc::as_ptr(&app.session.doc) as usize).hash(&mut hash);
    app.forms_prototype.other_preview.hash(&mut hash);
    controls::tolerance(app).map(f64::to_bits).hash(&mut hash);
    if let Some(f) = &app.session.forms_prototype {
        serde_json::to_string(&f.profile.as_ref().map(|p| &p.source)).unwrap_or_default().hash(&mut hash);
        f.editing_feature.hash(&mut hash);
        for (id, p) in &f.parts {
            id.hash(&mut hash);
            (Arc::as_ptr(&p.definition) as usize).hash(&mut hash);
        }
        for id in f.consumed_features() {
            id.hash(&mut hash);
        }
    }
    for (id, m) in &app.session.pending_moves {
        id.hash(&mut hash);
        for v in m.iter().flatten() {
            v.to_bits().hash(&mut hash);
        }
    }
    hash.finish()
}
pub(super) fn complete(app: &SolveApp) -> bool {
    app.forms_prototype.context.ready == Some(key(app)) && app.forms_prototype.context.error.is_none()
}
pub(super) fn busy(app: &SolveApp) -> bool {
    app.forms_prototype.context.pending.is_some()
}
fn source(session: &Session) -> Arc<doc::ModelState> {
    session.forms_prototype.as_ref().and_then(|f| f.editing_feature).map_or_else(|| session.model.state(), |id| session.model.state_before(id))
}
fn build(session: Session, other: Option<u64>, tolerance: f64, matching: bool) -> Result<Vec<Entry>, String> {
    let Some(form) = &session.forms_prototype else { return Ok(Vec::new()) };
    let mut document = session.doc.as_ref().clone();
    for occurrence in &mut document.occurrences {
        if let Some(transform) = session.pending_moves.get(&occurrence.id) {
            occurrence.transform = *transform;
        }
    }
    let state = doc::world_state(&document, &source(&session)).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut triangles = 0usize;
    let mut edge_points = 0usize;
    let mut push = |mesh: Arc<Mesh>, color: Option<[f32; 3]>, name: Option<String>| -> Result<(), String> {
        triangles = triangles.saturating_add(mesh.triangles.len());
        edge_points = edge_points.saturating_add(mesh.edges.iter().map(Vec::len).sum::<usize>());
        if triangles > 2_000_000 || edge_points > 8_000_000 {
            return Err("context preview geometry budget exceeded".into());
        }
        // Shared display/picking occlusion uses this worker-built index as well as source settings.
        let index = Some(solidcraft_engine::geom::MeshIndex::new(&mesh));
        entries.push(Entry { mesh, color, name, index });
        Ok(())
    };
    let mut visible = std::collections::BTreeSet::new();
    let mut visible_names = std::collections::BTreeSet::new();
    let mut hidden_names = std::collections::BTreeSet::new();
    for id in form.editing_feature.into_iter().chain(form.consumed_features()) {
        if let Some(result) = session.model.result(id) {
            let placed = doc::world_state(&document, &result.state).map_err(|e| e.to_string())?;
            hidden_names.extend(placed.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.clone()));
        }
    }
    for body in &state.bodies {
        if Some(body.feature) == form.editing_feature || form.consumes(body.feature) || !session.body_visible(&body.name) {
            continue;
        }
        visible.insert(body.feature);
        visible_names.insert(body.name.clone());
        let mesh = if !matching
            && let Some(profile) = &form.profile
            && profile.source.body().as_ref() == body.name
        {
            let (_, face) = profile.resolve(&state).map_err(|e| e.to_string())?;
            let open = doc::kernel::remove_faces(&body.body, &[face]).map_err(|e| e.to_string())?;
            Arc::new(open.mesh(&doc::kernel::MeshTolerance::chord(tolerance.min(0.05))).map_err(|e| e.to_string())?)
        } else {
            body.mesh()
        };
        push(mesh, body.body.color(), Some(body.name.clone()))?;
    }
    let final_state = doc::world_state(&document, &session.model.state()).map_err(|e| e.to_string())?;
    for body in &final_state.bodies {
        if visible_names.contains(&body.name) || hidden_names.contains(&body.name) || form.consumes(body.feature) || !session.body_visible(&body.name)
        {
            continue;
        }
        visible.insert(body.feature);
        visible_names.insert(body.name.clone());
        push(body.mesh(), body.body.color(), Some(body.name.clone()))?;
    }
    for part in form.parts.values() {
        let (mesh, _) = live_cad::compute(part.definition.as_ref().clone(), tolerance).map_err(|e| format!("{}: {e}", part.name))?;
        push(Arc::new(mesh), None, None)?;
    }
    if let Some(id) = other.filter(|id| !visible.contains(id) && !form.consumes(*id))
        && let Some(feature) = document.feature(id)
        && let doc::FeatureKind::Form { definition, .. } = &feature.kind
    {
        let mut definition = definition.as_ref().clone();
        definition.placement =
            doc::forms::Placement::from_matrix(&doc::mat_mul(&document.component_transform(feature.component), &definition.placement.matrix()))
                .map_err(|e| e.to_string())?;
        let (mesh, _) = live_cad::compute(definition, tolerance)?;
        push(Arc::new(mesh), None, None)?;
    }
    Ok(entries)
}

pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let matching = tool_panel::matching(app);
    let requested = key(app);
    let tolerance = controls::tolerance(app).unwrap_or(0.01);
    let other = app.forms_prototype.other_preview;
    let state = &mut app.forms_prototype.context;
    if state.desired != Some(requested) {
        state._document = Some(app.session.doc.clone());
        state._parts = app.session.forms_prototype.iter().flat_map(|f| f.parts.values().map(|p| p.definition.clone())).collect();
        state.desired = Some(requested);
        state.entries.clear();
        state.error = None;
        state.ready = None;
        state.version = state.version.wrapping_add(1);
    }
    if let Some(pending) = &state.pending {
        let result = match pending.done.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("context preview stopped".into())),
        };
        if let Some(result) = result {
            if pending.key == requested {
                match result {
                    Ok(entries) => {
                        state.entries = entries;
                        state.error = None;
                        state.ready = Some(requested);
                    }
                    Err(error) => {
                        state.error = Some(error);
                    }
                }
                state.version = state.version.wrapping_add(1);
            }
            state.pending = None;
        }
    }
    if state.pending.is_some() || state.requested == Some(requested) {
        return;
    }
    state.requested = Some(requested);
    let form = app.session.forms_prototype.as_ref();
    if source(&app.session).bodies.is_empty()
        && app.session.model.state().bodies.is_empty()
        && other.is_none()
        && form.is_none_or(|f| f.parts.is_empty())
    {
        state.ready = Some(requested);
        return;
    }
    let session = app.session.scratch();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        match std::thread::Builder::new().name("form-context-preview".into()).spawn(move || {
            let _ = tx.send(build(session, other, tolerance, matching));
            wake.request_repaint();
        }) {
            Ok(_) => state.pending = Some(Pending { key: requested, done: rx }),
            Err(e) => state.error = Some(e.to_string()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        match build(session, other, tolerance, matching) {
            Ok(entries) => {
                state.entries = entries;
                state.ready = Some(requested);
            }
            Err(e) => state.error = Some(e),
        }
        state.version = state.version.wrapping_add(1);
    }
}
fn retained_entries(app: &SolveApp) -> impl Iterator<Item = &Entry> {
    let combined = combine_preview::ready(app);
    app.forms_prototype
        .context
        .entries
        .iter()
        .filter(move |entry| !entry.name.as_ref().is_some_and(|name| combined.is_some_and(|preview| preview.replaced.contains(name))))
}
pub(super) fn render_entries(app: &SolveApp) -> impl Iterator<Item = &Entry> {
    retained_entries(app).chain(combine_preview::ready(app).into_iter().flat_map(|preview| preview.entries.iter()))
}
/// Unchanged opaque context shares exactly the renderer's visibility/replacement filter.
/// Boolean replacement meshes include the authored Form/cutter itself: its editing
/// handles retain their existing overlay semantics, rather than guessing triangle ownership.
pub(super) fn occludes(app: &SolveApp, origin: Vec3, direction: Vec3, depth: f64) -> bool {
    retained_entries(app).any(|entry| {
        let Some((hit, _)) = entry.index.as_ref().and_then(|index| index.raycast(&entry.mesh, origin, direction)) else { return false };
        // Only arithmetic roundoff at a coincident hit is allowed, independent of fit
        // tolerance, camera pixel size or a world-space geometry/display lift.
        let scale = origin.x.abs().max(origin.y.abs()).max(origin.z.abs()).max(depth.abs()).max(hit.abs()).max(1.0);
        hit < depth - 64.0 * f64::EPSILON * scale
    })
}

pub(super) fn append(app: &SolveApp, scene: &mut GpuScene) {
    let tokens = Tokens::get();
    let edge = scene.color(tokens.body_edge.to_array());
    for entry in render_entries(app) {
        let color = entry.color.map_or(tokens.body, crate::theme::srgb);
        let color = scene.color(color.to_array());
        for triangle in &entry.mesh.triangles {
            for &id in triangle {
                if let (Some(p), Some(n)) = (entry.mesh.positions.get(id as usize), entry.mesh.normals.get(id as usize)) {
                    scene.tri(p.to_f32(), n.to_f32(), color);
                }
            }
        }
        for (i, curve) in entry.mesh.edges.iter().enumerate() {
            if entry.mesh.seams.get(i).copied().unwrap_or(false) {
                continue;
            }
            for pair in curve.windows(2) {
                scene.line(pair[0].to_f32(), pair[1].to_f32(), edge, 1.0, false);
            }
        }
    }
}
pub(super) fn inspect(app: &SolveApp) -> serde_json::Value {
    json!({"bodies":app.forms_prototype.context.entries.len(),"busy":busy(app),"ready":complete(app),"error":app.forms_prototype.context.error})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surface_context_omits_its_source_face_but_keeps_it_pickable_in_source_settings() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Support","operation":"new"})).unwrap();
        let body = app.session.model.state().bodies[0].body.clone();
        let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        app.session.execute("form.surface", &json!({"body":"Support","face":top})).unwrap();
        let editing = build(app.session.scratch(), None, 0.05, false).unwrap();
        let picking = build(app.session.scratch(), None, 0.05, true).unwrap();
        assert_eq!(editing.len(), 1);
        assert_eq!(picking.len(), 1);
        assert_eq!(editing[0].mesh.tri_face.iter().copied().collect::<BTreeSet<_>>().len(), 5);
        assert_eq!(picking[0].mesh.tri_face.iter().copied().collect::<BTreeSet<_>>().len(), 6);
        assert!(editing[0].index.is_some());
        assert!(picking[0].index.is_some());
    }
    #[test]
    fn opaque_support_hides_surface_guides_from_drawing_and_picking_and_context_off_restores_them() {
        for offset in [Vec3::ZERO, Vec3::new(100.0, -50.0, 20.0)] {
            let mut app = SolveApp::new(Default::default(), Default::default());
            app.session
                .execute(
                    "solid.box",
                    &json!({"corner":[offset.x,offset.y,offset.z],"length":12,"width":8,"height":2,"body_name":"Support","operation":"new"}),
                )
                .unwrap();
            let body = app.session.model.state().bodies[0].body.clone();
            let top = (0..body.face_count()).find(|&i| body.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
            app.session.execute("form.surface", &json!({"body":"Support","face":top,"segments":[3,2],"continuity":0,"partition":false})).unwrap();
            let points = app.session.forms_prototype.as_ref().unwrap().surface.vertices.clone();
            let moved = [points[5] + Vec3::new(0.2, -0.3, 1.25), points[6] + Vec3::new(-0.15, 0.1, 0.4)];
            app.session
                .execute(
                    "form.prototype.set_surface_vertices",
                    &json!({"vertices":[5,6],"positions":moved.iter().map(|p|[p.x,p.y,p.z]).collect::<Vec<_>>()}),
                )
                .unwrap();
            let definition = app.session.forms_prototype.as_ref().unwrap().definition();
            let document = app.session.doc.clone();
            // The active mesh is the actual fitted Surface, not a box or fabricated blocker.
            let (mesh, _) =
                definition.profile.as_ref().unwrap().display(&app.session.model.state(), &definition.cage, &definition.placement, 0.01).unwrap();
            let form = app.session.forms_prototype.as_mut().unwrap();
            form.surface.index = solidcraft_engine::geom::MeshIndex::new(&mesh);
            form.surface.mesh = mesh;
            let points = [form.surface.vertices[5], form.surface.vertices[6]];
            app.forms_prototype.context.entries = build(app.session.scratch(), None, 0.01, false).unwrap();
            assert_eq!(app.forms_prototype.context.entries.len(), 1);
            app.cam.target = Vec3::new(6.0, 4.0, 2.2) + offset;
            app.cam.distance = 28.9;
            app.cam.yaw = -std::f64::consts::FRAC_PI_4;
            app.cam.pitch = -1.5;
            app.cam.fov = 0.0;
            let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0));
            let bottom = crate::viewport::projection(&app, rect);
            let ctx = egui::Context::default();
            let draw = |app: &mut SolveApp, projection: &Proj| {
                let _ = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
                    surface::overlay(app, ui, projection)
                });
            };
            draw(&mut app, &bottom);
            for (id, point) in [5, 6].into_iter().zip(points) {
                let screen = bottom.to_screen(point).unwrap();
                let (origin, direction) = bottom.ray(screen);
                let depth = (point - origin).dot(direction);
                let hit = app.forms_prototype.context.entries[0].mesh.raycast(origin, direction).unwrap().0;
                assert!(hit < depth - 2.0, "the actual opaque support bottom, not own-skin roundoff, blocks the guide");
                assert!(!surface::visible(&app, &bottom, point));
                assert!(!app.forms_prototype.projected.iter().any(|&(i, _)| i == id));
                assert_eq!(surface::nearest_with(&app, &bottom, screen, Pick::Vertex), None);
                assert_eq!(surface::nearest_with(&app, &bottom, screen, Pick::Face), None);
            }
            // Above the removed source face, actual own-skin guides remain available.
            app.cam.pitch = 0.6154797086703874;
            let above = crate::viewport::projection(&app, rect);
            let touching = Vec3::new(12.0, 0.0, 2.0) + offset;
            assert!(surface::visible(&app, &above, touching), "a guide touching the actual support rim is not hidden by its side wall");
            for (id, point) in [5, 6].into_iter().zip(points) {
                assert!(surface::visible(&app, &above, point));
                assert_eq!(surface::nearest_with(&app, &above, above.to_screen(point).unwrap(), Pick::Vertex), Some(Element::Vertex(id)));
            }
            // Toggle the actual source body rather than bypassing the visibility predicate.
            app.session.execute("browser.visibility", &json!({"items":["Support"],"visible":false})).unwrap();
            app.forms_prototype.context.entries = build(app.session.scratch(), None, 0.01, false).unwrap();
            assert!(app.forms_prototype.context.entries.is_empty());
            draw(&mut app, &bottom);
            for (id, point) in [5, 6].into_iter().zip(points) {
                assert!(surface::visible(&app, &bottom, point));
                assert!(app.forms_prototype.projected.iter().any(|&(i, _)| i == id));
                assert_eq!(surface::nearest_with(&app, &bottom, bottom.to_screen(point).unwrap(), Pick::Vertex), Some(Element::Vertex(id)));
            }
            app.session.execute("browser.visibility", &json!({"items":["Support"],"visible":true})).unwrap();
            app.forms_prototype.context.entries = build(app.session.scratch(), None, 0.01, false).unwrap();
            assert!(points.into_iter().all(|p| !surface::visible(&app, &bottom, p)));
            assert!(Arc::ptr_eq(&document, &app.session.doc));
            assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), definition);
        }
    }
    fn fixture() -> SolveApp {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"length":10,"width":8,"height":6,"name":"Base"})).unwrap();
        app.session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
        let feature = app.session.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap();
        app.session.execute("form.edit", &json!({"feature":feature})).unwrap();
        app
    }
    fn preview_split(app: &mut SolveApp) {
        let face = app.session.forms_prototype.as_ref().unwrap().cage.faces[0].clone();
        let edges: Vec<_> = face.iter().copied().zip(face.iter().copied().cycle().skip(1)).take(4).map(|(a, b)| [a, b]).collect();
        app.session.execute("form.prototype.tool.begin", &json!({})).unwrap();
        app.session.execute("form.prototype.tool.preview", &json!({"unweld":{"edges":edges}})).unwrap();
    }
    #[test]
    fn context_shows_the_preceding_body_and_staged_part_without_the_old_active_form() {
        let mut app = fixture();
        let edited = app.session.forms_prototype.as_ref().unwrap().editing_feature.unwrap();
        app.session.execute("form.prototype.close", &json!({})).unwrap();
        app.session.execute("form.prototype.create", &json!({"primitive":"face","size":5,"segments":[1,1,1],"levels":1})).unwrap();
        let matrix = doc::rigid(Vec3::X * 30.0, Vec3::ZERO, Vec3::Z, 0.0);
        app.session.execute("form.prototype.transform", &json!({"matrix":matrix})).unwrap();
        app.session.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
        app.session.execute("form.edit", &json!({"feature":edited})).unwrap();
        let document = serde_json::to_value(app.session.doc.as_ref()).unwrap();
        let entries = build(app.session.scratch(), None, 0.05, false).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.index.is_some()), "opaque display context is indexed once in its worker");
        let pickable = build(app.session.scratch(), None, 0.05, true).unwrap();
        assert!(pickable.iter().all(|entry| entry.name.is_some() && entry.index.is_some()));
        let bounds: solidcraft_engine::geom::Aabb3 = entries[0].mesh.positions.iter().copied().collect();
        assert!(bounds.size().dist(Vec3::new(10.0, 8.0, 6.0)) < 1e-9);
        preview_split(&mut app);
        assert_eq!(build(app.session.scratch(), None, 0.05, false).unwrap().len(), 3);
        app.session.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        assert_eq!(build(app.session.scratch(), None, 0.05, false).unwrap().len(), 2);
        assert_eq!(serde_json::to_value(app.session.doc.as_ref()).unwrap(), document);
    }
    #[test]
    fn later_saved_forms_start_a_context_worker_without_any_preceding_body() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        let mut features = Vec::new();
        for _ in 0..2 {
            app.session.execute("form.prototype.create", &json!({"primitive":"face","segments":[1,1,1],"levels":1})).unwrap();
            features.push(app.session.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap()["feature"].as_u64().unwrap());
        }
        app.session.execute("form.edit", &json!({"feature":features[0]})).unwrap();
        assert!(source(&app.session).bodies.is_empty());
        tick(&mut app, &egui::Context::default());
        assert!(app.forms_prototype.context.pending.is_some());
        assert!(!complete(&app));
        assert_eq!(build(app.session.scratch(), None, 0.05, false).unwrap().len(), 1);
    }
    #[test]
    fn a_late_companion_preview_cannot_return_after_cancel() {
        let mut app = fixture();
        preview_split(&mut app);
        let old = key(&app);
        let entries = build(app.session.scratch(), None, 0.05, false).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Ok(entries)).unwrap();
        app.forms_prototype.context.pending = Some(Pending { key: old, done: rx });
        app.forms_prototype.context.requested = Some(old);
        app.forms_prototype.context.desired = Some(old);
        app.session.execute("form.prototype.tool.cancel", &json!({})).unwrap();
        let current = key(&app);
        assert_ne!(current, old);
        tick(&mut app, &egui::Context::default());
        assert!(app.forms_prototype.context.entries.is_empty());
        assert_ne!(app.forms_prototype.context.ready, Some(old));
        assert_eq!(app.forms_prototype.context.pending.as_ref().unwrap().key, current);
    }
}
