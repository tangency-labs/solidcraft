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
    let state = doc::world_state(&document, &source(&session));
    let mut entries = Vec::new();
    let mut triangles = 0usize;
    let mut edge_points = 0usize;
    let mut push = |mesh: Arc<Mesh>, color: Option<[f32; 3]>, name: Option<String>| -> Result<(), String> {
        triangles = triangles.saturating_add(mesh.triangles.len());
        edge_points = edge_points.saturating_add(mesh.edges.iter().map(Vec::len).sum::<usize>());
        if triangles > 2_000_000 || edge_points > 8_000_000 {
            return Err("context preview geometry budget exceeded".into());
        }
        let index = matching.then(|| solidcraft_engine::geom::MeshIndex::new(&mesh));
        entries.push(Entry { mesh, color, name, index });
        Ok(())
    };
    let mut visible = std::collections::BTreeSet::new();
    let mut visible_names = std::collections::BTreeSet::new();
    let mut hidden_names = std::collections::BTreeSet::new();
    for id in form.editing_feature.into_iter().chain(form.consumed_features()) {
        if let Some(result) = session.model.result(id) {
            let placed = doc::world_state(&document, &result.state);
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
            Arc::new(open.display_mesh(tolerance.min(0.05)).map_err(|e| e.to_string())?)
        } else {
            body.mesh()
        };
        push(mesh, body.body.color(), Some(body.name.clone()))?;
    }
    let final_state = doc::world_state(&document, &session.model.state());
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
pub(super) fn append(app: &SolveApp, scene: &mut GpuScene) {
    let tokens = Tokens::get();
    let edge = scene.color(tokens.body_edge.to_array());
    for entry in &app.forms_prototype.context.entries {
        let color = entry.color.map_or(tokens.body, |c| egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
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
        assert!(picking[0].index.is_some());
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
        assert!(entries.iter().all(|entry| entry.index.is_none()));
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
