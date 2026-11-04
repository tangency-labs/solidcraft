//! Preview the same CAD operation that Finish will evaluate, on an isolated session.
use super::*;
use solidcraft_engine::{Session, doc};

#[derive(Default)]
pub(super) struct State {
    requested: Option<u64>,
    pending: Option<Pending>,
    ready: Option<(u64, Result<Preview, String>)>,
}

struct Pending {
    key: u64,
    done: std::sync::mpsc::Receiver<Result<Preview, String>>,
}

pub(super) struct Preview {
    pub replaced: BTreeSet<String>,
    pub entries: Vec<context::Entry>,
}

fn key(app: &SolveApp) -> Option<u64> {
    let form = app.session.forms_prototype.as_ref()?;
    if !app.forms_prototype.show_combination || controls::operation(app) == 0 || form.profile.is_some() || form.has_tool_preview() {
        return None;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    context::key(app).hash(&mut hash);
    form.revision.hash(&mut hash);
    controls::operation(app).hash(&mut hash);
    controls::tolerance(app)?.to_bits().hash(&mut hash);
    Some(hash.finish())
}

fn compute(mut scratch: Session, params: &serde_json::Value) -> Result<Preview, String> {
    let reply = scratch.execute("form.prototype.commit", params).map_err(|e| e.to_string())?;
    let id = reply["feature"].as_u64().ok_or("Form evaluation did not return a feature identity")?;
    let result = scratch.model.result(id).ok_or("Form evaluation did not produce a model state")?;
    if let Some(error) = result.error.as_ref().or(result.warning.as_ref()) {
        return Err(error.clone());
    }
    let before = scratch.model.state_before(id);
    let mut previous = before.as_ref().clone();
    let mut changed = result.state.as_ref().clone();
    // Use local body identity before placing occurrences. Comparing re-placed
    // copies would incorrectly mark unchanged parts as Boolean results.
    previous.bodies.retain(|b| changed.body(&b.name).is_none_or(|next| !next.body.same_shape(&b.body)));
    changed.bodies.retain(|b| before.body(&b.name).is_none_or(|old| !old.body.same_shape(&b.body)));

    let mut document = std::borrow::Cow::Borrowed(scratch.doc.as_ref());
    if !scratch.pending_moves.is_empty() {
        for occurrence in &mut document.to_mut().occurrences {
            if let Some(matrix) = scratch.pending_moves.get(&occurrence.id) {
                occurrence.transform = *matrix;
            }
        }
    }
    let replaced = doc::world_state(&document, &previous).map_err(|e| e.to_string())?.bodies.into_iter().map(|body| body.name).collect();
    let mut triangles = 0usize;
    let mut edges = 0usize;
    let mut entries = Vec::new();
    for body in doc::world_state(&document, &changed).map_err(|e| e.to_string())?.bodies {
        let mesh = body.mesh();
        triangles = triangles.saturating_add(mesh.triangles.len());
        edges = edges.saturating_add(mesh.edges.iter().map(Vec::len).sum::<usize>());
        if triangles > 2_000_000 || edges > 8_000_000 {
            return Err("combined preview exceeds its display geometry budget".into());
        }
        entries.push(context::Entry { name: Some(body.name), index: None, mesh, color: body.body.color() });
    }
    Ok(Preview { replaced, entries })
}

pub(super) fn result(app: &SolveApp) -> Option<&Result<Preview, String>> {
    let key = key(app)?;
    app.forms_prototype.combination.ready.as_ref().filter(|(given, _)| *given == key).map(|(_, result)| result)
}

pub(super) fn ready(app: &SolveApp) -> Option<&Preview> {
    if extrusion::preview(app).is_some() {
        return None;
    }
    result(app)?.as_ref().ok()
}

pub(super) fn busy(app: &SolveApp) -> bool {
    key(app).is_some() && app.forms_prototype.combination.pending.is_some()
}

pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let desired = key(app);
    let state = &mut app.forms_prototype.combination;
    if let Some(pending) = &state.pending {
        let received = match pending.done.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("combined preview computation stopped".into()),
        };
        if desired == Some(pending.key) {
            state.ready = Some((pending.key, received));
            app.forms_prototype.uploaded = 0;
        } else {
            // The same inputs can be selected again after a stale job finishes.
            state.requested = None;
        }
        state.pending = None;
    }
    let Some(key) = desired else { return };
    if state.requested == Some(key)
        || app.forms_prototype.pending.is_some()
        || app.forms_prototype.drag.is_some()
        || app.forms_prototype.gizmo_drag.is_some()
        || ctx.input(|i| i.pointer.primary_down())
    {
        return;
    }
    let Ok(params) = controls::commit_params(app) else { return };
    let scratch = app.session.scratch();
    let state = &mut app.forms_prototype.combination;
    state.requested = Some(key);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, done) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        match std::thread::Builder::new().name("form-combined-preview".into()).spawn(move || {
            let _ = tx.send(compute(scratch, &params));
            wake.request_repaint();
        }) {
            Ok(_) => state.pending = Some(Pending { key, done }),
            Err(e) => state.ready = Some((key, Err(e.to_string()))),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        state.ready = Some((key, compute(scratch, &params)));
        app.forms_prototype.uploaded = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_enclosed_cutting_forms_handles_remain_editable_behind_its_combined_preview() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("solid.box", &json!({"corner":[-10,-10,-10],"length":20,"width":20,"height":20,"body_name":"Base"})).unwrap();
        app.session.execute("form.prototype.create", &json!({"size":8,"segments":[1,1,1],"levels":2})).unwrap();
        app.session.execute("form.prototype.finish_options", &json!({"operation":"cut","tolerance":"0.05 mm"})).unwrap();
        controls::load(&mut app);
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 60.0;
        app.cam.fov = 0.0;
        let projection = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let (id, point) = app
            .session
            .forms_prototype
            .as_ref()
            .unwrap()
            .surface
            .vertices
            .iter()
            .copied()
            .enumerate()
            .find(|&(_, point)| surface::visible(&app, &projection, point))
            .unwrap();
        let definition = app.session.forms_prototype.as_ref().unwrap().definition();
        let document = app.session.doc.clone();
        let params = json!({"operation":"cut","tolerance":0.05,"targets":["Base"]});
        let preview = compute(app.session.scratch(), &params).unwrap();
        assert_eq!(preview.replaced, BTreeSet::from(["Base".into()]));
        assert_eq!(preview.entries.len(), 1);
        let screen = projection.to_screen(point).unwrap();
        let (origin, direction) = projection.ray(screen);
        let depth = (point - origin).dot(direction);
        let replacement_hit = preview.entries[0].mesh.raycast(origin, direction).unwrap().0;
        let scale = origin.x.abs().max(origin.y.abs()).max(origin.z.abs()).max(depth.abs()).max(replacement_hit.abs()).max(1.0);
        let roundoff = 64.0 * f64::EPSILON * scale;
        assert!(
            depth - replacement_hit > 2.0 && depth - replacement_hit > 1_000_000.0 * roundoff,
            "the actual opaque Boolean result is genuinely ahead of this authored cutter handle, not a coincident own-skin hit"
        );
        let base = app.session.world_state().unwrap().body("Base").unwrap().mesh();
        app.forms_prototype.context.entries =
            vec![context::Entry { name: Some("Base".into()), index: Some(solidcraft_engine::geom::MeshIndex::new(&base)), mesh: base, color: None }];
        assert!(
            context::occludes(&app, origin, direction, depth),
            "the ordinary opaque Base would block this handle without the Boolean editing exception"
        );
        app.forms_prototype.combination.ready = Some((key(&app).unwrap(), Ok(preview)));
        assert!(ready(&app).is_some());
        assert_eq!(context::render_entries(&app).count(), 1, "render only the replacement Base, not the old source body");
        assert!(!context::occludes(&app, origin, direction, depth), "the combined result includes the authored cutter and is not unrelated context");
        assert!(surface::visible(&app, &projection, point));
        assert_eq!(surface::nearest_with(&app, &projection, screen, Pick::Vertex), Some(Element::Vertex(id)));
        let ctx = egui::Context::default();
        let _ = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(projection.rect), ..Default::default() }, |ui| {
            surface::overlay(&mut app, ui, &projection)
        });
        assert!(app.forms_prototype.projected.iter().any(|&(vertex, _)| vertex == id));
        assert!(Arc::ptr_eq(&document, &app.session.doc));
        assert_eq!(app.session.forms_prototype.as_ref().unwrap().definition(), definition);
    }
    #[test]
    fn combined_preview_matches_finish_and_preserves_the_editing_session() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"corner":[-0.5,-0.5,-0.5],"length":1,"width":1,"height":1,"body_name":"Base"})).unwrap();
        session.execute("form.prototype.create", &json!({"size":8,"segments":[1,1,1],"levels":2})).unwrap();
        let snapshot = session.doc.clone();
        let cage = session.forms_prototype.as_ref().unwrap().cage.clone();
        let params = json!({"operation":"join","tolerance":0.05,"targets":["Base"]});
        let preview = compute(session.scratch(), &params).unwrap();
        assert_eq!(preview.replaced, BTreeSet::from(["Base".into()]));
        assert_eq!(preview.entries.len(), 1);
        assert!(Arc::ptr_eq(&snapshot, &session.doc));
        assert_eq!(session.forms_prototype.as_ref().unwrap().cage, cage);
        let mut app = SolveApp::new(session.scratch(), Default::default());
        session.execute("form.prototype.commit", &params).unwrap();
        assert_eq!(preview.entries[0].mesh.positions, session.world_state().unwrap().body("Base").unwrap().mesh().positions);
        app.viewport.gpu = Some(crate::gpu::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: Some(egui_wgpu::wgpu::TextureFormat::Depth32Float),
            samples: 1,
            tess: false,
            oit: false,
        });
        app.forms_prototype.elements.insert(Element::Face(0));
        app.forms_prototype.hover = Some(Element::Face(1));
        let proj = crate::viewport::projection(&app, egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0)));
        let ctx = egui::Context::default();
        let draw = |app: &mut SolveApp| {
            let _ = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(proj.rect), ..Default::default() }, |ui| {
                super::super::render(app, ui, &proj)
            });
            super::super::viewport_frame(app, &proj, 1.0, app.forms_prototype.uploaded)
        };
        let before = draw(&mut app);
        assert!(!before.hl_slot.lock().unwrap().as_ref().unwrap().tris.is_empty());
        app.session.execute("form.prototype.finish_options", &json!({"operation":"join","tolerance":"0.05 mm"})).unwrap();
        controls::load(&mut app);
        app.forms_prototype.combination.ready = Some((key(&app).unwrap(), Ok(preview)));
        assert!(ready(&app).is_some());
        let combined = draw(&mut app);
        assert_ne!(combined.hl_key, before.hl_key);
        assert!(!combined.slot.lock().unwrap().as_ref().unwrap().tris.is_empty(), "the actual combined CAD result still renders");
        assert!(
            combined.hl_slot.lock().unwrap().as_ref().unwrap().tris.is_empty(),
            "source-face tints must not survive over a replacement Boolean preview"
        );
        app.forms_prototype.show_combination = false;
        let restored = draw(&mut app);
        assert_ne!(restored.hl_key, combined.hl_key);
        assert!(!restored.hl_slot.lock().unwrap().as_ref().unwrap().tris.is_empty(), "source highlights return when the combined preview is hidden");
    }
}
