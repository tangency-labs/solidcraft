//! Ephemeral shot previews. One cancellable CPU task; no host, job identity or document edits.
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::SolveApp;
use solidcraft_engine::render::{Camera, job::CancelToken};

pub(super) const LIMIT: usize = 16;
#[cfg(not(target_arch = "wasm32"))]
const SIZE: [u32; 2] = [104, 60];

// Cancellation is cooperative. Keep a single process-wide worker permit so rapid tab/document
// changes cannot accumulate detached workers while the previous task finishes its bounded step.
#[cfg(not(target_arch = "wasm32"))]
static WORKER_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(not(target_arch = "wasm32"))]
struct WorkerPermit;
#[cfg(not(target_arch = "wasm32"))]
impl Drop for WorkerPermit {
    fn drop(&mut self) {
        WORKER_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Key {
    pub name: String,
    revision: u64,
    scene: u64,
    camera: u64,
}

impl Key {
    pub(super) fn new(name: String, revision: u64, scene: u64, camera: Camera) -> Self {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        // Camera's complete serialized numerical intent, including roll, projection and up axis.
        if let Ok(bytes) = serde_json::to_vec(&camera) {
            bytes.hash(&mut hash);
        }
        Self { name, revision, scene, camera: hash.finish() }
    }
}

#[derive(Default)]
pub(super) struct Cache {
    pub entries: BTreeMap<Key, Option<egui::TextureHandle>>,
    visible: Vec<String>,
    pending: Option<Pending>,
}
struct Pending {
    key: Key,
    cancel: CancelToken,
    result: std::sync::mpsc::Receiver<Option<egui::ColorImage>>,
}
impl Drop for Cache {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.cancel();
        }
    }
}
impl Cache {
    pub fn show(&mut self, names: Vec<String>, ctx: &egui::Context) {
        let names = names.into_iter().take(LIMIT).collect::<Vec<_>>();
        if self.visible != names {
            self.visible = names;
            ctx.request_repaint();
        }
    }

    pub fn texture(&self, name: &str) -> Option<egui::TextureId> {
        self.entries.iter().find(|(key, _)| key.name == name).and_then(|(_, value)| value.as_ref().map(egui::TextureHandle::id))
    }

    pub fn retain(&mut self, keys: &[Key]) {
        self.entries.retain(|key, _| keys.iter().take(LIMIT).any(|wanted| wanted == key));
        if let Some(pending) = &self.pending
            && !keys.iter().take(LIMIT).any(|key| key == &pending.key)
        {
            // Keep the receiver until the worker exits: invalidation cannot spawn extra workers.
            pending.cancel.cancel();
        }
    }

    fn poll(&mut self, ctx: &egui::Context, keys: &[Key]) {
        let Some(pending) = &self.pending else { return };
        let image = match pending.result.try_recv() {
            Ok(image) => image,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        if let Some(pending) = self.pending.take()
            && keys.iter().take(LIMIT).any(|key| key == &pending.key)
            && !pending.cancel.is_cancelled()
        {
            self.entries.insert(pending.key, image.map(|image| ctx.load_texture("shot_thumbnail", image, egui::TextureOptions::LINEAR)));
        }
    }
}

pub(super) fn refresh(app: &mut SolveApp, ctx: &egui::Context) {
    let mut scene = std::collections::hash_map::DefaultHasher::new();
    app.session.visibility.hidden_bodies.hash(&mut scene);
    app.session.visibility.hidden_occurrences.hash(&mut scene);
    (std::sync::Arc::as_ptr(&app.session.world_state()) as usize).hash(&mut scene);
    let scene = scene.finish();
    let mut keys = Vec::new();
    for name in std::iter::once(&app.render_workspace.view.name)
        .chain(app.render_workspace.thumbnails.visible.iter())
        .chain(app.session.doc.render.setups.keys())
    {
        if keys.len() == LIMIT {
            break;
        }
        if keys.iter().any(|key: &Key| &key.name == name) {
            continue;
        }
        if let Some(setup) = app.session.doc.render.setups.get(name) {
            keys.push(Key::new(name.clone(), app.session.revision, scene, setup.camera));
        }
    }
    let cache = &mut app.render_workspace.thumbnails;
    cache.retain(&keys);
    cache.poll(ctx, &keys);
    if cache.pending.is_some() {
        return;
    }
    let Some(key) = keys.into_iter().find(|key| !cache.entries.contains_key(key)) else { return };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if WORKER_ACTIVE.compare_exchange(false, true, std::sync::atomic::Ordering::Acquire, std::sync::atomic::Ordering::Relaxed).is_err() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        let permit = WorkerPermit;
        let Some(setup) = app.session.doc.render.setups.get(&key.name) else { return };
        let request = solidcraft_engine::render::job::ImageRequest {
            studio_weights: None,
            camera: setup.camera,
            width: SIZE[0],
            height: SIZE[1],
            background: setup.background,
            transparent_background: false,
            integrator: Default::default(),
            raytrace: None,
            environment: None,
            textures: None,
        };
        let scene = solidcraft_engine::product_scene::extract(&app.session);
        let cancel = CancelToken::default();
        let worker_cancel = cancel.clone();
        let (send, result) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new().name("shot-thumbnail".into()).spawn(move || {
            let _permit = permit;
            let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let scene = scene.ok()?;
                let mut task = solidcraft_engine::render::job::ImageTask::new(scene, request, worker_cancel).ok()?;
                loop {
                    match task.step(1024).ok()? {
                        solidcraft_engine::render::job::TaskPoll::Pending(_) => {}
                        solidcraft_engine::render::job::TaskPoll::Complete(png) => {
                            let image = image::load_from_memory(&png).ok()?.to_rgba8();
                            return Some(egui::ColorImage::from_rgba_unmultiplied([SIZE[0] as usize, SIZE[1] as usize], image.as_raw()));
                        }
                    }
                }
            }))
            .ok()
            .flatten();
            let _ = send.send(image);
        });
        if worker.is_ok() {
            app.render_workspace.thumbnails.pending = Some(Pending { key, cancel, result });
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            app.render_workspace.thumbnails.entries.insert(key, None);
        }
    }
    #[cfg(target_arch = "wasm32")]
    cache.entries.insert(key, None);
}
