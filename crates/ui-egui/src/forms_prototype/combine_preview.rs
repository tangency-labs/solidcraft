//! Preview the same CAD operation that Finish will evaluate, on an isolated session.
use super::*;
use solidcraft_engine::{Session, doc};

#[cfg(any(test, not(target_arch = "wasm32")))]
const NATIVE_DISPLAY_BUDGET: (usize, usize) = (2_000_000, 8_000_000);

#[cfg(any(target_arch = "wasm32", test))]
mod browser {
    use super::*;
    use serde::{Deserialize, Serialize};
    use solidcraft_engine::{forms_prototype::validation::BrowserCheck, recovery};

    #[cfg(target_arch = "wasm32")]
    pub(super) const UNAVAILABLE: &str = "forms.prototype.browser_combination_unavailable";
    pub(super) const SOURCE_LIMIT: &str = "forms.prototype.browser_combination_source_limit";
    pub(super) const DISPLAY_LIMIT: &str = "forms.prototype.browser_combination_display_limit";

    /// The browser shell runs the CAD computation in its dedicated Form worker.
    #[cfg(target_arch = "wasm32")]
    pub trait BrowserPreviewHost {
        fn preview_busy(&self) -> bool;
        fn submit_preview(
            &self,
            request: Vec<u8>,
            cancelled: std::rc::Rc<std::cell::Cell<bool>>,
            finished: std::sync::mpsc::Sender<BrowserPreviewOutcome>,
            ctx: &egui::Context,
        ) -> Result<(), String>;
    }

    pub type BrowserPreviewOutcome = Result<BrowserPreview, String>;

    /// Display-only result. The host never adopts the worker's scratch design or CAD state.
    #[derive(Serialize, Deserialize)]
    pub struct BrowserPreview {
        replaced: BTreeSet<String>,
        entries: Vec<DisplayEntry>,
    }

    #[derive(Serialize, Deserialize)]
    struct DisplayEntry {
        name: Option<String>,
        mesh: Arc<Mesh>,
        color: Option<[f32; 3]>,
        display_only_edges: bool,
        smooth_edges: Vec<bool>,
        sharp_edges: Vec<bool>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input<D> {
        document: D,
        params: serde_json::Value,
        validation: doc::kernel::Validation,
        param_check: String,
    }

    struct BoundedBytes {
        bytes: Vec<u8>,
        limit: usize,
    }

    impl std::io::Write for BoundedBytes {
        fn write(&mut self, source: &[u8]) -> std::io::Result<usize> {
            if source.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(SOURCE_LIMIT));
            }
            self.bytes.extend_from_slice(source);
            Ok(source.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn independent_design(document: &doc::Document) -> bool {
        document.features.len() <= 64
            && document.links.is_empty()
            && document.parts.is_empty()
            && document.point_clouds.is_empty()
            && document.canvases.is_empty()
            && document.render.is_empty()
            && document.newer_file.is_none()
            && document.features.iter().all(|feature| !matches!(&feature.kind, doc::FeatureKind::Import { .. } | doc::FeatureKind::MeshImport { .. }))
    }

    fn independent_form(session: &Session) -> bool {
        BrowserCheck::available(session)
            && session.forms_prototype.as_ref().is_some_and(|form| form.parts.is_empty() && form.consumed_features().next().is_none())
    }

    impl BrowserPreview {
        pub const MAX_REQUEST: usize = 512 * 1024;
        pub const MAX_REPLY: usize = 8 * 1024 * 1024;

        pub(super) fn capture(session: &Session, params: serde_json::Value) -> Result<Vec<u8>, String> {
            let spec = solidcraft_engine::find_command("form.prototype.commit").ok_or("Form commit is not registered")?;
            session.check_command(spec, &params).map_err(|error| error.to_string())?;
            if !independent_form(session) || !independent_design(&session.doc) || !session.pending_moves.is_empty() || !session.exploded.is_empty() {
                return Err(SOURCE_LIMIT.into());
            }
            // Bounded streaming on the page. All feature evaluation and meshing happen in the worker.
            let mut wire = BoundedBytes { bytes: vec![0; 4], limit: Self::MAX_REQUEST };
            let input = Input {
                document: session.doc.as_ref(),
                params,
                validation: session.validation(),
                param_check: session.param_check.name().to_owned(),
            };
            serde_json::to_writer(&mut wire, &input).map_err(|_| SOURCE_LIMIT.to_owned())?;
            let size = u32::try_from(wire.bytes.len() - 4).map_err(|error| error.to_string())?;
            wire.bytes.get_mut(..4).ok_or("missing Form preview header")?.copy_from_slice(&size.to_le_bytes());
            recovery::write_form_preview_source(session, &mut wire).map_err(|_| SOURCE_LIMIT.to_owned())?;
            Ok(wire.bytes)
        }

        pub fn run(bytes: &[u8]) -> Result<Vec<u8>, String> {
            let outcome = Self::compute(bytes);
            let mut wire = BoundedBytes { bytes: Vec::new(), limit: Self::MAX_REPLY };
            if serde_json::to_writer(&mut wire, &outcome).is_err() {
                return serde_json::to_vec(&Err::<Self, _>(DISPLAY_LIMIT)).map_err(|error| error.to_string());
            }
            Ok(wire.bytes)
        }

        fn compute(bytes: &[u8]) -> BrowserPreviewOutcome {
            if bytes.len() > Self::MAX_REQUEST {
                return Err(SOURCE_LIMIT.into());
            }
            let (header, bytes) = bytes.split_at_checked(4).ok_or("incomplete Form preview header")?;
            let size = u32::from_le_bytes(header.try_into().map_err(|_| "invalid Form preview header")?) as usize;
            let (input, draft) = bytes.split_at_checked(size).ok_or("incomplete Form preview document")?;
            let Input { document, params, validation, param_check } =
                serde_json::from_slice(input).map_err(|error| format!("invalid Form preview document: {error}"))?;
            if !independent_design(&document) {
                return Err(SOURCE_LIMIT.into());
            }
            let mut session = recovery::restore_form_preview(document, draft, validation).map_err(|error| error.to_string())?;
            if !independent_form(&session) {
                return Err(SOURCE_LIMIT.into());
            }
            session.param_check = solidcraft_engine::schema::ParamCheck::parse(&param_check).ok_or("invalid Form preview parameter mode")?;
            let Preview { replaced, entries } = super::compute(session, &params, (50_000, 150_000))
                .map_err(|error| if error == super::DISPLAY_BUDGET { DISPLAY_LIMIT.to_owned() } else { error })?;
            let entries = entries
                .into_iter()
                .map(|entry| DisplayEntry {
                    name: entry.name,
                    color: entry.color,
                    display_only_edges: entry.mesh.display_only_edges,
                    smooth_edges: entry.mesh.smooth_edges.clone(),
                    sharp_edges: entry.mesh.sharp_edges.clone(),
                    mesh: entry.mesh,
                })
                .collect();
            Ok(Self { replaced, entries })
        }

        pub fn decode_reply(bytes: &[u8]) -> BrowserPreviewOutcome {
            if bytes.len() > Self::MAX_REPLY {
                return Err(DISPLAY_LIMIT.into());
            }
            serde_json::from_slice(bytes).map_err(|error| format!("invalid Form preview result: {error}"))?
        }

        pub(super) fn into_preview(self) -> Preview {
            let entries = self
                .entries
                .into_iter()
                .map(|DisplayEntry { name, mut mesh, color, display_only_edges, smooth_edges, sharp_edges }| {
                    let m = Arc::make_mut(&mut mesh);
                    m.display_only_edges = display_only_edges;
                    m.smooth_edges = smooth_edges;
                    m.sharp_edges = sharp_edges;
                    context::Entry { name, index: None, mesh, color, exact: None, detached_companion: false }
                })
                .collect();
            Preview { replaced: self.replaced, entries }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::{BrowserPreview, BrowserPreviewHost, BrowserPreviewOutcome};

#[derive(Default)]
pub(super) struct State {
    requested: Option<u64>,
    pending: Option<Pending>,
    ready: Option<(u64, Result<Preview, String>)>,
}

struct Pending {
    key: u64,
    done: std::sync::mpsc::Receiver<PreviewOutcome>,
    #[cfg(target_arch = "wasm32")]
    cancelled: std::rc::Rc<std::cell::Cell<bool>>,
}

#[cfg(not(target_arch = "wasm32"))]
type PreviewOutcome = Result<Preview, String>;
#[cfg(target_arch = "wasm32")]
type PreviewOutcome = BrowserPreviewOutcome;

#[cfg(target_arch = "wasm32")]
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.set(true);
    }
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
    form.source_revision().hash(&mut hash);
    app.session.validation().hash(&mut hash);
    app.session.param_check.name().hash(&mut hash);
    controls::operation(app).hash(&mut hash);
    controls::tolerance(app)?.to_bits().hash(&mut hash);
    Some(hash.finish())
}

const DISPLAY_BUDGET: &str = "combined preview exceeds its display geometry budget";

fn compute(mut scratch: Session, params: &serde_json::Value, (max_triangles, max_edges): (usize, usize)) -> Result<Preview, String> {
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
        if triangles > max_triangles || edges > max_edges {
            return Err(DISPLAY_BUDGET.into());
        }
        entries.push(context::Entry { name: Some(body.name), index: None, mesh, color: body.body.color(), exact: None, detached_companion: false });
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

#[cfg(target_arch = "wasm32")]
fn localized_error(error: String) -> String {
    match error.as_str() {
        browser::UNAVAILABLE | browser::SOURCE_LIMIT | browser::DISPLAY_LIMIT => solidcraft_ui_text::tr(&error).to_owned(),
        _ => error,
    }
}

pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let desired = key(app);
    let state = &mut app.forms_prototype.combination;
    #[cfg(target_arch = "wasm32")]
    if state.pending.as_ref().is_some_and(|pending| desired != Some(pending.key)) {
        state.pending = None;
        state.requested = None;
    }
    if let Some(pending) = &state.pending {
        let received = match pending.done.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("combined preview computation stopped".into()),
        };
        if desired == Some(pending.key) {
            #[cfg(target_arch = "wasm32")]
            let received = received.map(BrowserPreview::into_preview).map_err(localized_error);
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
    #[cfg(target_arch = "wasm32")]
    if app.browser_form_check.as_ref().is_some_and(|host| host.preview_busy()) {
        // The existing job will wake this page when it replies.
        return;
    }
    let state = &mut app.forms_prototype.combination;
    state.requested = Some(key);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let scratch = app.session.scratch();
        let (tx, done) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        match std::thread::Builder::new().name("form-combined-preview".into()).spawn(move || {
            let _ = tx.send(compute(scratch, &params, NATIVE_DISPLAY_BUDGET));
            wake.request_repaint();
        }) {
            Ok(_) => state.pending = Some(Pending { key, done }),
            Err(e) => state.ready = Some((key, Err(e.to_string()))),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let (finished, done) = std::sync::mpsc::channel();
        let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
        let queued = app
            .browser_form_check
            .as_ref()
            .ok_or_else(|| browser::UNAVAILABLE.to_owned())
            .and_then(|host| host.submit_preview(BrowserPreview::capture(&app.session, params)?, cancelled.clone(), finished, ctx));
        match queued {
            Ok(()) => state.pending = Some(Pending { key, done, cancelled }),
            Err(error) => state.ready = Some((key, Err(localized_error(error)))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_worker_combined_preview_matches_exact_native_result() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"corner":[-0.5,-0.5,-0.5],"length":1,"width":1,"height":1,"body_name":"Base"})).unwrap();
        session.execute("form.prototype.create", &json!({"size":8,"segments":[1,1,1],"levels":2})).unwrap();
        let params = json!({"operation":"join","tolerance":0.05,"targets":["Base"]});
        let expected = compute(session.scratch(), &params, NATIVE_DISPLAY_BUDGET).unwrap();
        let request = browser::BrowserPreview::capture(&session, params).unwrap();
        let reply = browser::BrowserPreview::run(&request).unwrap();
        let preview = browser::BrowserPreview::decode_reply(&reply).unwrap().into_preview();
        assert_eq!(preview.replaced, expected.replaced);
        assert_eq!(preview.entries.len(), expected.entries.len());
        for (actual, expected) in preview.entries.iter().zip(expected.entries) {
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.mesh.positions.len(), expected.mesh.positions.len());
            assert_eq!(actual.mesh.triangles.len(), expected.mesh.triangles.len());
            let actual_bounds = actual.mesh.positions.iter().copied().collect::<solidcraft_engine::geom::Aabb3>();
            let expected_bounds = expected.mesh.positions.iter().copied().collect::<solidcraft_engine::geom::Aabb3>();
            assert!((actual_bounds.min - expected_bounds.min).len() < 1e-10);
            assert!((actual_bounds.max - expected_bounds.max).len() < 1e-10);
            assert_eq!(
                actual.mesh.smooth_edges.iter().filter(|&&smooth| smooth).count(),
                expected.mesh.smooth_edges.iter().filter(|&&smooth| smooth).count()
            );
            assert_eq!(
                actual.mesh.sharp_edges.iter().filter(|&&sharp| sharp).count(),
                expected.mesh.sharp_edges.iter().filter(|&&sharp| sharp).count()
            );
        }
        assert!(session.forms_prototype.is_some());
        let reply = browser::BrowserPreview::run(&[0, 0]).unwrap();
        assert!(browser::BrowserPreview::decode_reply(&reply).is_err());
    }

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
        let preview = compute(app.session.scratch(), &params, NATIVE_DISPLAY_BUDGET).unwrap();
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
        app.forms_prototype.context.entries = vec![context::Entry {
            name: Some("Base".into()),
            index: Some(Arc::new(solidcraft_engine::geom::MeshIndex::new(&base))),
            mesh: base,
            color: None,
            exact: None,
            detached_companion: false,
        }];
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
        assert!(app.forms_prototype.projected.iter().any(|&(vertex, _, _)| vertex == id));
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
        let preview = compute(session.scratch(), &params, NATIVE_DISPLAY_BUDGET).unwrap();
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
