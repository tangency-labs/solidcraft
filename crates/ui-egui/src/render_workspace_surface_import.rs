//! Thin file/provenance controls. Native codecs run once on a scratch-session worker;
//! only a complete, still-current command transaction reaches the displayed design.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct View {
    pub(super) path: String,
    pub(super) author: String,
    pub(super) source: String,
    pub(super) license: String,
    pub(super) repeat_mm: [f64; 2],
    pub(super) reveal: bool,
    pub(super) pending: Option<Pending>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            path: String::new(),
            author: String::new(),
            source: String::new(),
            license: String::new(),
            repeat_mm: [100.0; 2],
            reveal: false,
            pending: None,
        }
    }
}

type Imported = Result<solidcraft_engine::ComputedCommands, String>;
#[derive(Clone)]
pub(super) struct Pending {
    pub(super) result: Arc<std::sync::Mutex<std::sync::mpsc::Receiver<Imported>>>,
    pub(super) document: Weak<Document>,
    pub(super) revision: u64,
    pub(super) shot: String,
    pub(super) method: ImageMethod,
    pub(super) snapshot: Value,
}
impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingSurfaceImport").field("revision", &self.revision).field("shot", &self.shot).finish_non_exhaustive()
    }
}

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

pub(super) fn pick(app: &mut SolveApp, previous: String) {
    let document = Arc::downgrade(&app.session.doc);
    let revision = app.session.revision;
    let shot = app.render_workspace.view.name.clone();
    let method = app.render_workspace.view.method;
    let extensions: &[&str] = if method == ImageMethod::CpuRaytrace { &["png"] } else { &["png", "jpg", "jpeg"] };
    app.pick_open_for(extensions, move |app, path| {
        if app.session.revision != revision
            || !Weak::ptr_eq(&document, &Arc::downgrade(&app.session.doc))
            || app.render_workspace.view.name != shot
            || app.render_workspace.view.method != method
            || app.render_workspace.view.library.imported.path != previous
        {
            return;
        }
        if let Some(path) = path {
            app.render_workspace.view.library.imported.path = path;
        }
    });
}

pub(super) fn request(app: &mut SolveApp, view: &super::View) -> Result<Value, String> {
    if app.render_workspace.view.method == ImageMethod::Raster {
        return Err(app.tr("render.surface_import.raytrace_required").into());
    }
    if !current_target(app, view) {
        return Err(app.tr("render.material_revalidate_needed").into());
    }
    let input = &view.imported;
    if input.path.trim().is_empty() || [&input.author, &input.source, &input.license].into_iter().any(|text| text.trim().is_empty()) {
        return Err(app.tr("render.surface_import.complete").into());
    }
    let target = view.target.as_ref().ok_or_else(|| app.tr("render.material_choose_body").to_string())?;
    let scene = app.run("render.scene", json!({}))?;
    let body = scene["bodies"]
        .get(target.ordinal)
        .filter(|body| body["name"].as_str() == Some(target.name.as_str()))
        .ok_or_else(|| app.tr("render.material_revalidate_needed").to_string())?;
    let min: Vec3 = serde_json::from_value(body["bounds"]["min"].clone()).map_err(|e| e.to_string())?;
    let max: Vec3 = serde_json::from_value(body["bounds"]["max"].clone()).map_err(|e| e.to_string())?;
    let center = (min + max) * 0.5;
    let [width, height] = input.repeat_mm;
    let (sin, cos) = view.mapping_angle_deg.to_radians().sin_cos();
    let (u, v) = match view.mapping_plane {
        1 => ([0.0, cos * width, sin * width], [0.0, -sin * height, cos * height]),
        2 => ([sin * width, 0.0, cos * width], [cos * height, 0.0, -sin * height]),
        _ => ([cos * width, sin * width, 0.0], [-sin * height, cos * height, 0.0]),
    };
    let frame = solidcraft_engine::render::job::textures::PlanarFrame { origin: [center.x, center.y, center.z], u, v };
    frame.validate().map_err(|e| e.to_string())?;
    Ok(json!({"name":app.render_workspace.view.name,"set":patch(app)?,"body_index":target.ordinal,"body_name":target.name,
        "frame":frame,"maps":{"base_color_srgb":{"path":input.path}},
        "provenance":{"author":input.author,"source":input.source,"license":input.license},
        "normal_strength":1.0,"flip_v":false,"replace":true}))
}

fn finish(app: &mut SolveApp, imported: Imported) -> Result<(), String> {
    if app.file_load.busy() || app.file_picker_busy() {
        return Err("a file operation is in progress".into());
    }
    let replies = app.session.accept_commands(imported?).map_err(|e| e.to_string())?;
    for (id, reply) in replies {
        super::super::apply_command(app, &id, &reply)?;
        app.command_completed(&id, Ok(reply))?;
    }
    setups::load(app);
    Ok(())
}

fn begin(app: &mut SolveApp, view: &mut super::View, ctx: &egui::Context) -> Result<(), String> {
    let params = request(app, view)?;
    let session = app.session.scratch();
    let commands = vec![("render.surface.import".into(), params)];
    #[cfg(not(target_arch = "wasm32"))]
    {
        if WORKER_ACTIVE.compare_exchange(false, true, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire).is_err() {
            return Err(app.tr("render.surface_import.busy").into());
        }
        let permit = WorkerPermit;
        let (sender, receiver) = std::sync::mpsc::channel();
        let pending = Pending {
            result: Arc::new(std::sync::Mutex::new(receiver)),
            document: Arc::downgrade(&app.session.doc),
            revision: app.session.revision,
            shot: app.render_workspace.view.name.clone(),
            method: app.render_workspace.view.method,
            snapshot: patch(app)?,
        };
        let repaint = ctx.clone();
        std::thread::Builder::new()
            .name("surface import".into())
            .spawn(move || {
                let _permit = permit;
                let result = session.compute_commands(&commands, None).map_err(|e| e.to_string());
                let _ = sender.send(result);
                repaint.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        view.imported.pending = Some(pending);
        ctx.request_repaint();
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        // The browser host has no Rust worker threads. Codec validation is bounded
        // by the same 1024-square resource admission; the async picker uses its VFS.
        let _ = ctx;
        finish(app, session.compute_commands(&commands, None).map_err(|e| e.to_string()))
    }
}

pub(super) fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(pending) = app.render_workspace.view.library.imported.pending.as_ref() else { return };
    let current = pending.revision == app.session.revision
        && Weak::ptr_eq(&pending.document, &Arc::downgrade(&app.session.doc))
        && pending.shot == app.render_workspace.view.name
        && pending.method == app.render_workspace.view.method
        && patch(app).ok().as_ref() == Some(&pending.snapshot);
    if !current {
        app.render_workspace.view.library.imported.pending = None;
        return;
    }
    let result = match pending.result.lock() {
        Ok(receiver) => match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(app.tr("render.surface_import.failed").into())),
        },
        Err(_) => Some(Err(app.tr("render.surface_import.failed").into())),
    };
    let Some(result) = result else {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        return;
    };
    app.render_workspace.view.library.imported.pending = None;
    if let Err(error) = finish(app, result) {
        app.set_status(error, true);
    }
}

pub(super) fn controls(app: &mut SolveApp, ui: &mut egui::Ui, view: &mut super::View) {
    let title = app.tr("render.surface_import.title").to_string();
    if std::mem::take(&mut view.imported.reveal) {
        let section = ui.make_persistent_id("surface_import");
        ui.ctx().data_mut(|data| data.insert_persisted(section, true));
        ui.scroll_to_cursor(Some(egui::Align::Min));
    }
    ui_kit::section(ui, "surface_import", &title, false, |ui| {
        if view.imported.pending.is_some() {
            ui_kit::text(ui, app.tr("render.surface_import.busy"), ui_kit::Text::Note("surface_import:busy"));
            if ui_kit::button(ui, app.tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                view.imported.pending = None;
            }
            return;
        }
        if !current_target(app, view) {
            let previous = view.target.clone();
            if let Err(error) = refresh_targets(app, view) {
                app.set_status(error, true);
            } else {
                view.target = previous.filter(|target| view.targets.contains(target));
            }
        }
        let picked = app.session.selection.iter().find_map(|sel| match sel {
            solidcraft_engine::Sel::Body { name } | solidcraft_engine::Sel::Face { body: name, .. } => Some(name),
            _ => None,
        });
        if view.target.is_none() {
            view.target = view.targets.iter().find(|target| picked.is_some_and(|name| target.name == *name)).cloned();
        }
        ui_kit::row(ui, app.tr("render.stage_panels.target"), |ui| {
            let name = view.target.as_ref().map_or(app.tr("render.material_choose_body"), |target| target.name.as_str()).to_string();
            ui_kit::dropdown_menu(ui, "surface_import_target", &name, |ui| {
                for target in &view.targets {
                    if ui_kit::list_picker(ui, &target.name, &target.name, view.target.as_ref() == Some(target)).clicked() {
                        view.target = Some(target.clone());
                        ui.close();
                    }
                }
            });
        });
        let label = if view.imported.path.is_empty() {
            app.tr("render.appearance_editor.choose_image").to_string()
        } else {
            std::path::Path::new(&view.imported.path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
        };
        let choose = ui_kit::button(ui, &label, ui_kit::Button::Normal);
        crate::scenario::publish_target("render:surface-import:choose", choose.rect);
        if choose.clicked() {
            pick(app, view.imported.path.clone());
        }
        for (key, value, maximum) in [
            ("render.surface_import.author", &mut view.imported.author, 512),
            ("render.surface_import.source", &mut view.imported.source, 2048),
            ("render.surface_import.license", &mut view.imported.license, 4096),
        ] {
            ui_kit::row(ui, app.tr(key), |ui| {
                ui_kit::bounded_cell_editor(ui, ui.id().with(key), value, "", false, maximum);
            });
        }
        ui_kit::row(ui, app.tr("render.stage_panels.plane"), |ui| {
            ui_kit::segmented(ui, &mut view.mapping_plane, &["xy", "yz", "zx"], ui_kit::Segments { compact: true, ..Default::default() });
        });
        for (key, value) in ["render.surface_import.repeat_u", "render.surface_import.repeat_v"].into_iter().zip(&mut view.imported.repeat_mm) {
            ui_kit::row(ui, app.tr(key), |ui| {
                ui_kit::number_field(ui, value, 0.001..=1e6, 1.0, " mm");
            });
        }
        ui_kit::row(ui, app.tr("render.stage_panels.grain_angle"), |ui| {
            ui_kit::number_field(ui, &mut view.mapping_angle_deg, -180.0..=180.0, 1.0, "°");
        });
        let raytrace = app.render_workspace.view.method != ImageMethod::Raster;
        let ready = raytrace
            && current_target(app, view)
            && !view.imported.path.is_empty()
            && [&view.imported.author, &view.imported.source, &view.imported.license].into_iter().all(|text| !text.trim().is_empty());
        let save = ui_kit::button(ui, app.tr("render.surface_import.save"), ui_kit::Button::Enabled(ready));
        crate::scenario::publish_target("render:surface-import:save", save.rect);
        if save.clicked()
            && let Err(error) = begin(app, view, ui.ctx())
        {
            app.set_status(error, true);
        }
        if !raytrace {
            ui_kit::text(ui, app.tr("render.surface_import.raytrace_required"), ui_kit::Text::Note("surface_import:raytrace"));
        }
    });
}
