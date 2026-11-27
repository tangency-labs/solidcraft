//! View backgrounds from plugins (capability `shaders@0.1`, `docs/plugin-shaders.md`).
//!
//! The WGSL is checked off the window thread and built under an error scope on it; whatever fails
//! leaves the theme's background, with one line in the status bar.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use egui_wgpu::wgpu;

use solidcraft_engine::plugin::shader::STATE_BINDING;

use crate::SolveApp;

/// The status line's message when a background can't be drawn.
const UNREADABLE: &str = "prefs.background_shader_failed";

pub(crate) struct Loaded {
    pub(crate) wgsl: String,
    pub(crate) animated: bool,
    pub(crate) kind: solidcraft_engine::plugin::shader::ShaderKind,
    /// Its manifest entry, for its settings.
    pub(crate) shader: solidcraft_engine::plugin::shader::Shader,
    /// Each texture's mip levels, largest first: size and straight RGBA.
    pub(crate) textures: Vec<Vec<([u32; 2], Vec<u8>)>>,
    /// One line for each chosen picture that was refused and replaced by the shipped one.
    pub(crate) notes: Vec<String>,
    pub(crate) state: Option<[u32; 2]>,
}

pub(crate) type Pending = Arc<Mutex<Option<Result<Loaded, String>>>>;

/// A background ready to draw, and its target the size of the view.
struct Active {
    key: String,
    animated: bool,
    shader: solidcraft_engine::plugin::shader::Shader,
    pipeline: wgpu::RenderPipeline,
    /// One, or two for a state shader: each binds one grid to both state bindings, read as prev or current.
    bind_groups: Vec<wgpu::BindGroup>,
    uniforms: wgpu::Buffer,
    /// A picture per window and view in it: each shows its own, so a second window (or tile)
    /// drawing its view does not draw over the picture another one shows.
    targets: Slots<TargetKey, Target>,
    state: Option<Sim>,
}

/// The window (egui viewport) and the view in it (`Viewport::scene_slot`) a picture is drawn for.
type TargetKey = (egui::ViewportId, Option<u64>);

/// How long a view's picture is kept after it was last drawn: a closed window or a view gone.
const TARGET_KEEP_SECS: f64 = 5.0;

/// The least time between two state steps: views and windows drawing in the same frame step
/// the state once, not once each.
const STEP_GAP_SECS: f64 = 0.012;

/// Values by key with the time each was last used, so the ones no longer used can be dropped.
struct Slots<K, T> {
    map: HashMap<K, (T, f64)>,
}

impl<K, T> Default for Slots<K, T> {
    fn default() -> Self {
        Self { map: HashMap::new() }
    }
}

impl<K: Eq + std::hash::Hash, T> Slots<K, T> {
    /// `key`'s value, marked used at `now`.
    fn use_at(&mut self, key: &K, now: f64) -> Option<&mut T> {
        self.map.get_mut(key).map(|(value, used)| {
            *used = now;
            value
        })
    }

    fn insert(&mut self, key: K, value: T, now: f64) {
        self.map.insert(key, (value, now));
    }

    fn remove(&mut self, key: &K) -> Option<T> {
        self.map.remove(key).map(|(value, _)| value)
    }

    /// Takes out the values not used for `keep` seconds before `now` (or used "later", from
    /// another clock), so their GPU pictures can be freed.
    fn take_stale(&mut self, now: f64, keep: f64) -> Vec<T> {
        let mut stale = Vec::new();
        for (key, (value, used)) in std::mem::take(&mut self.map) {
            if now - used > keep || used > now + keep {
                stale.push(value);
            } else {
                self.map.insert(key, (value, used));
            }
        }
        stale
    }

    fn take_all(&mut self) -> Vec<T> {
        self.map.drain().map(|(_, (value, _))| value).collect()
    }
}

/// Whether the state steps for a draw at `now`, after its last step at `last`: at most once per
/// `STEP_GAP_SECS`, however many views and windows draw it (or when the clock runs back).
fn steps_now(last: Option<f64>, now: f64) -> bool {
    last.is_none_or(|last| now - last >= STEP_GAP_SECS || now < last)
}

/// A state shader's two grids: each frame the step writes the other one, then the background reads it.
struct Sim {
    pipeline: wgpu::RenderPipeline,
    views: [wgpu::TextureView; 2],
    size: [u32; 2],
    /// The grid the last step wrote.
    current: usize,
    /// The step this frame: 0 on the first, then counting up (back to 1 after `FRAME_WRAP`).
    frame: u32,
    /// The last step's number and when it ran (the input clock): views drawn between two steps
    /// read the grid it wrote.
    stepped: Option<(u32, f64)>,
}

/// The step counter's wrap: well past the grid's hash and warm-up needs, and far from f32's exact range.
const FRAME_WRAP: u32 = 1_000_000;

/// The next step's number: counting up, and back to 1 (never 0, which is the reset) after `FRAME_WRAP`.
fn next_step(frame: u32) -> u32 {
    if frame >= FRAME_WRAP { 1 } else { frame + 1 }
}

struct Target {
    size: [u32; 2],
    /// The uniforms of the last frame drawn into it: a still background draws again only when
    /// they change (the view turned, resized or the theme changed).
    drawn: Option<Vec<u8>>,
    view: wgpu::TextureView,
    id: egui::TextureId,
}

#[derive(Default)]
pub(crate) struct State {
    render: Option<egui_wgpu::RenderState>,
    loading: Option<(String, Pending)>,
    active: Option<Active>,
    /// The key that failed, so it is not tried again every frame; a new choice clears it.
    failed: Option<String>,
    /// `failed` is only because its plugin is turned off: turning it on again clears it.
    off: bool,
}

impl State {
    /// The window's GPU. A new device (after a lost one) starts over.
    pub(crate) fn install(&mut self, rs: &egui_wgpu::RenderState) {
        *self = Self { render: Some(rs.clone()), ..Self::default() };
    }
}

/// The folder of the turned-on plugin that offers background `key`, if one is installed.
pub(crate) fn folder_of(app: &SolveApp, key: &str) -> Option<String> {
    let (plugin, shader) = key.split_once('/')?;
    if !crate::plugins::enabled(app, plugin) {
        return None;
    }
    let mut paths: Vec<String> = app.session.plugin_state.packages.get(plugin).cloned().into_iter().collect();
    if let Some(dir) = app.session.plugin_state.plugins_dir() {
        paths.push(dir.join(plugin).to_string_lossy().into_owned());
    }
    paths.into_iter().find(|path| manifest(path).is_ok_and(|m| m.id == plugin && m.shaders.iter().any(|s| s.id == shader)))
}

/// A plugin folder's manifest alone: a background needs no component.
pub(crate) fn manifest(folder: &str) -> Result<solidcraft_engine::plugin::Manifest, String> {
    let dir = std::path::Path::new(folder);
    let name = dir.file_name().and_then(|n| n.to_str()).ok_or("a plugin folder needs a name")?;
    let path = dir.join("plugin.json");
    let size = std::fs::metadata(&path).map_err(|e| format!("plugin.json: {e}"))?.len();
    if size > solidcraft_engine::plugin::MAX_MANIFEST_BYTES as u64 {
        return Err("plugin.json: too large".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("plugin.json: {e}"))?;
    solidcraft_engine::plugin::parse_manifest(&bytes, name).map_err(|e| e.to_string())
}

/// The preference's keys for `shader`'s chosen pictures (`plugin/shader/<texture name>`).
fn pictures_of(prefs: &BTreeMap<String, String>, shader: &str) -> BTreeMap<String, String> {
    let prefix = format!("{shader}/");
    prefs.iter().filter(|(k, _)| k.starts_with(&prefix)).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The key a background is loaded, drawn and failed under: the shader's key, and with chosen
/// pictures also those, so a new picture loads the background again. Without pictures it is the
/// shader's key alone.
fn load_key(shader: &str, pictures: &BTreeMap<String, String>) -> String {
    if pictures.is_empty() { shader.to_owned() } else { format!("{shader}\n{pictures:?}") }
}

/// The background's key as it stands in the preferences.
fn current_key(app: &SolveApp) -> String {
    let shader = app.preferences.background_shader.as_str();
    load_key(shader, &pictures_of(&app.preferences.background_shader_pictures, shader))
}

/// The key as the status line names it: its shader, without the pictures.
fn shown(key: &str) -> &str {
    key.split('\n').next().unwrap_or(key)
}

/// Reads one file of at most `max` bytes.
fn read_file(path: &std::path::Path, max: usize) -> Result<Vec<u8>, String> {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let size = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
    if size > max as u64 {
        return Err(format!("{name}: larger than {} KiB", max / 1024));
    }
    std::fs::read(path).map_err(|e| format!("{name}: {e}"))
}

/// Reads and checks the chosen background off the window thread.
pub(crate) fn load(folder: String, key: String, pictures: BTreeMap<String, String>, limit: u32) -> Result<Loaded, String> {
    let manifest = manifest(&folder)?;
    let shader_id = key.split_once('/').map(|(_, s)| s).unwrap_or_default();
    let shader = manifest.shaders.iter().find(|s| s.id == shader_id).ok_or_else(|| format!("{key} is not in the plugin any more"))?;
    let dir = std::path::Path::new(&folder);
    let read = |file: &str, max: usize| read_file(&dir.join(file), max);
    let source = read(&shader.source, solidcraft_engine::plugin::shader::MAX_SOURCE_BYTES)?;
    let source = String::from_utf8(source).map_err(|_| format!("{} is not UTF-8 text", shader.source))?;
    let compiled = solidcraft_engine::plugin::shader::compile(shader, &source).map_err(|e| e.to_string())?;
    let side = limit.min(solidcraft_engine::plugin::shader::MAX_TEXTURE_SIDE);
    let max = solidcraft_engine::plugin::shader::MAX_TEXTURE_BYTES;
    let mut notes = Vec::new();
    let textures = shader
        .textures
        .iter()
        .map(|t| {
            // The chosen picture, else the shipped one; a chosen picture that won't read or decode
            // falls back to the shipped one, and a texture with neither is an error.
            let shipped = || match &t.file {
                Some(file) => read(file, max).and_then(|bytes| mips(&bytes, side).map_err(|e| format!("{file}: {e}"))),
                None => Err(format!("{}: no picture chosen", t.name)),
            };
            match pictures.get(&format!("{key}/{}", t.name)) {
                Some(path) => read_file(std::path::Path::new(path), max)
                    .and_then(|bytes| {
                        let name = std::path::Path::new(path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                        mips(&bytes, side).map_err(|e| format!("{name}: {e}"))
                    })
                    .or_else(|error| {
                        if t.file.is_some() {
                            notes.push(error);
                            shipped()
                        } else {
                            Err(error)
                        }
                    }),
                None => shipped(),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Loaded {
        wgsl: compiled.wgsl,
        animated: shader.animated,
        kind: shader.kind,
        shader: shader.clone(),
        textures,
        notes,
        state: shader.state.map(|s| [s.width, s.height]),
    })
}

/// A picture's mip levels, at most `limit` on a side, down to 1×1.
fn mips(bytes: &[u8], limit: u32) -> Result<Vec<([u32; 2], Vec<u8>)>, String> {
    let mut image = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
    if image.width().max(image.height()) > limit {
        let (w, h) = (image.width(), image.height());
        let scale = f64::from(limit) / f64::from(w.max(h));
        let fit = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
        image = image::imageops::resize(&image, fit(w).min(w), fit(h).min(h), image::imageops::FilterType::Triangle);
    }
    let mut levels = Vec::new();
    loop {
        let (w, h) = (image.width(), image.height());
        let next = (w > 1 || h > 1).then(|| image::imageops::resize(&image, (w / 2).max(1), (h / 2).max(1), image::imageops::FilterType::Triangle));
        levels.push(([w, h], image.into_raw()));
        match next {
            Some(smaller) => image = smaller,
            None => return Ok(levels),
        }
    }
}

pub(crate) fn paint(app: &mut SolveApp, painter: &egui::Painter, rect: egui::Rect, tokens: &solidcraft_theme::Tokens) {
    if !allowed(app) {
        return;
    }
    let shader = app.preferences.background_shader.clone();
    let pictures = pictures_of(&app.preferences.background_shader_pictures, &shader);
    let key = load_key(&shader, &pictures);
    let state = &mut app.plugin_shaders;
    if state.failed.as_ref().is_some_and(|failed| *failed != key) {
        // Another choice: the one that failed may be tried again when it is chosen again.
        state.failed = None;
    }
    if shader.is_empty() || state.render.is_none() || state.failed.as_deref() == Some(key.as_str()) {
        return;
    }
    let ctx = painter.ctx().clone();
    if state.active.as_ref().is_some_and(|a| a.key != key) {
        retire(state);
    }
    if state.active.is_none() {
        let Some(rs) = state.render.clone() else { return };
        if state.loading.as_ref().is_none_or(|(k, _)| *k != key) {
            let Some(folder) = folder_of(app, &shader) else {
                missing(app, &key);
                return;
            };
            let pending: Pending = Arc::default();
            let (slot, repaint, k, limit) = (pending.clone(), ctx.clone(), shader.clone(), rs.device.limits().max_texture_dimension_2d);
            std::thread::spawn(move || {
                let result = load(folder, k, pictures, limit);
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
                repaint.request_repaint();
            });
            app.plugin_shaders.loading = Some((key.clone(), pending));
            return;
        }
        let state = &mut app.plugin_shaders;
        let done = state.loading.as_ref().and_then(|(_, p)| p.lock().unwrap_or_else(PoisonError::into_inner).take());
        let Some(done) = done else { return };
        state.loading = None;
        let notes = done.as_ref().map(|loaded| loaded.notes.clone()).unwrap_or_default();
        match done.and_then(|loaded| build(&rs, &key, loaded)) {
            Ok(active) => {
                state.active = Some(active);
                announce(app, &key, &notes);
            }
            Err(error) => {
                state.failed = Some(key.clone());
                let message = format!("{} {}: {error}", solidcraft_ui_text::tr(UNREADABLE), shown(&key));
                log::warn!("{message}");
                app.set_status(message, true);
                return;
            }
        }
    }
    let state = &mut app.plugin_shaders;
    let (Some(rs), Some(active)) = (state.render.clone(), state.active.as_mut()) else { return };
    if solidcraft_viewport_egui::window_device(&rs).lost.is_lost() {
        // The shader may be why the driver gave up: turn it off before the app restarts, so the
        // next start does not run it again (the restart also turns it off, `after_loss`).
        let key = active.key.clone();
        app.plugin_shaders.failed = Some(key.clone());
        app.preferences.background_shader.clear();
        log::warn!("graphics device lost while the background effect {} was drawn; it is turned off", shown(&key));
        return;
    }
    let ppp = ctx.pixels_per_point();
    let max = rs.device.limits().max_texture_dimension_2d;
    let size = [((rect.width() * ppp).round() as u32).clamp(1, max), ((rect.height() * ppp).round() as u32).clamp(1, max)];
    let time = if active.animated {
        let focused = ctx.input(|i| i.focused);
        // Slower behind another window, where nobody is working.
        ctx.request_repaint_after(std::time::Duration::from_millis(if focused { 33 } else { 100 }));
        (ctx.input(|i| i.time) % 3600.0) as f32
    } else {
        0.0
    };
    let settings = settings(&app.preferences.shader_settings, &active.key, &active.shader);
    // The input clock, not the frame: windows count their frames apart, and several views draw
    // in one frame.
    let now = ctx.input(|i| i.time);
    let (step, grid, stepping) = match active.state.as_mut() {
        Some(sim) => {
            let stepping = steps_now(sim.stepped.map(|(_, at)| at), now);
            let step = match (stepping, sim.stepped) {
                (false, Some((step, _))) => step,
                _ => {
                    let step = sim.frame;
                    sim.frame = next_step(step);
                    sim.stepped = Some((step, now));
                    step
                }
            };
            (step as f32, sim.size.map(|v| v as f32), stepping)
        }
        None => (0.0, [0.0, 0.0], false),
    };
    let uniforms = host_uniforms(size, time, ppp, &app.cam, tokens, (step, grid), settings);
    let key = (ctx.viewport_id(), app.viewport.scene_slot());
    let state = &mut app.plugin_shaders;
    let Some(active) = state.active.as_mut() else { return };
    free(&rs, active.targets.take_stale(now, TARGET_KEEP_SECS));
    let Some(id) = draw(&rs, active, key, now, size, &uniforms, stepping) else { return };
    painter.image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
}

/// A plugin turned off after its background was drawn, or while it loads, drops it, saying so once.
fn allowed(app: &mut SolveApp) -> bool {
    if app.preferences.background_shader.is_empty() {
        return true;
    }
    let key = current_key(app);
    let shader = app.preferences.background_shader.clone();
    let plugin = shader.split_once('/').map_or(shader.as_str(), |(id, _)| id).to_owned();
    let enabled = crate::plugins::enabled(app, &plugin);
    let state = &mut app.plugin_shaders;
    if state.failed.as_deref() == Some(key.as_str()) {
        if !(state.off && enabled) {
            return true;
        }
        // Its plugin is on again: try the background again.
        state.failed = None;
    }
    state.off = false;
    if enabled {
        return true;
    }
    forget(app, &plugin);
    missing(app, &key);
    app.plugin_shaders.off = true;
    false
}

/// Says once, when a background loads, which chosen pictures it could not use and that the
/// shipped ones are shown instead.
fn announce(app: &mut SolveApp, key: &str, notes: &[String]) {
    if notes.is_empty() {
        return;
    }
    let message = format!("{}: {}: {}", solidcraft_ui_text::tr("prefs.background_picture_fallback"), shown(key), notes.join("; "));
    log::warn!("{message}");
    app.set_status(message, true);
}

/// Drops the pictures chosen for `plugin`'s backgrounds, for a plugin that is removed.
pub(crate) fn drop_pictures(app: &mut SolveApp, plugin: &str) {
    let prefix = format!("{plugin}/");
    app.preferences.background_shader_pictures.retain(|key, _| !key.starts_with(&prefix));
}

/// Drops the pictures of plugins that are no longer in the plugins folder: `present` are the ids
/// found there, including the ones whose manifest is refused.
pub(crate) fn drop_pictures_not_in(app: &mut SolveApp, present: &dyn Fn(&str) -> bool) {
    app.preferences.background_shader_pictures.retain(|key, _| key.split_once('/').is_some_and(|(plugin, _)| present(plugin)));
}

/// Sets the picture a texture is chosen as (`plugin/shader/<texture name>`), checked the way the
/// preference checks are. The background loads again on its own, because its key changes.
pub(crate) fn choose_picture(app: &mut SolveApp, picture: &str, path: String) -> Result<(), String> {
    let mut pictures = app.preferences.background_shader_pictures.clone();
    pictures.insert(picture.to_owned(), path.clone());
    if !valid_pictures(&pictures) {
        return Err(solidcraft_ui_text::format("prefs.background_picture_refused", &[("picture", &path)]));
    }
    app.preferences.background_shader_pictures = pictures;
    Ok(())
}

/// Whether `pictures` may be stored: at most `MAX_PICTURES` entries, each keyed
/// `plugin/shader/<texture name>` with an absolute path of at most `MAX_PICTURE_PATH` characters.
pub(crate) fn valid_pictures(pictures: &BTreeMap<String, String>) -> bool {
    pictures.len() <= crate::plugins::MAX_PICTURES && pictures.iter().all(|(key, path)| valid_picture(key, path))
}

/// One entry of `valid_pictures`.
pub(crate) fn valid_picture(key: &str, path: &str) -> bool {
    let parts: Vec<&str> = key.split('/').collect();
    let [plugin, shader, texture] = parts[..] else { return false };
    crate::plugins::valid_id(plugin)
        && crate::plugins::valid_id(shader)
        && solidcraft_engine::plugin::shader::texture_name(texture)
        && (1..=crate::plugins::MAX_PICTURE_PATH).contains(&path.chars().count())
        && std::path::Path::new(path).is_absolute()
        && !path.chars().any(char::is_control)
}

/// Drops `plugin`'s background, drawn or loading, so it is read again from its folder when next chosen.
pub(crate) fn forget(app: &mut SolveApp, plugin: &str) {
    let state = &mut app.plugin_shaders;
    let owns = |key: &str| key.split_once('/').is_some_and(|(id, _)| id == plugin);
    if state.active.as_ref().is_some_and(|a| owns(&a.key)) {
        retire(state);
    }
    if state.loading.as_ref().is_some_and(|(key, _)| owns(key)) {
        state.loading = None;
    }
}

/// The chosen background's plugin is missing or off: say so once, not every frame.
fn missing(app: &mut SolveApp, key: &str) {
    app.plugin_shaders.failed = Some(key.to_owned());
    let message = format!("{} {}: {}", solidcraft_ui_text::tr(UNREADABLE), shown(key), solidcraft_ui_text::tr("prefs.background_shader_missing"));
    log::warn!("{message}");
    app.set_status(message, true);
}

fn retire(state: &mut State) {
    if let (Some(rs), Some(mut active)) = (&state.render, state.active.take()) {
        free(rs, active.targets.take_all());
    }
}

/// Lets egui forget pictures no longer drawn.
fn free(rs: &egui_wgpu::RenderState, targets: Vec<Target>) {
    if targets.is_empty() {
        return;
    }
    let mut renderer = rs.renderer.write();
    for target in targets {
        renderer.free_texture(&target.id);
    }
}

/// Linear-light RGBA of a theme colour, alpha 1.
pub(crate) fn linear(c: egui::Color32) -> [f32; 4] {
    let l = egui::Rgba::from(c);
    [l.r(), l.g(), l.b(), 1.0]
}

/// The values of `key`'s settings (`sc.settings`) from the preference, keyed `plugin/shader/setting`.
pub(crate) fn settings(
    stored: &std::collections::BTreeMap<String, f32>,
    key: &str,
    shader: &solidcraft_engine::plugin::shader::Shader,
) -> [f32; solidcraft_engine::plugin::shader::MAX_SETTINGS] {
    shader.setting_values(|id| stored.get(&format!("{key}/{id}")).copied())
}

/// `ScHost` as the shader reads it (`solidcraft_engine::plugin::shader::PRELUDE`).
pub(crate) fn host_uniforms(
    size: [u32; 2],
    time: f32,
    scale: f32,
    cam: &solidcraft_viewport::Camera,
    t: &solidcraft_theme::Tokens,
    (step, grid): (f32, [f32; 2]),
    settings: [f32; solidcraft_engine::plugin::shader::MAX_SETTINGS],
) -> Vec<u8> {
    const HALF_HEIGHT: f64 = 0.5;
    let (right, up, back) = cam.basis();
    let aspect = f64::from(size[0]) / f64::from(size[1].max(1));
    let axis = |v: solidcore_record::Vec3, by: f64| [(v.x * by) as f32, (v.y * by) as f32, (v.z * by) as f32, 0.0];
    let dark = egui::Rgba::from(t.viewport_bottom).intensity() < 0.25;
    let mut f: Vec<f32> = vec![size[0] as f32, size[1] as f32, time, scale];
    f.extend(axis(right, HALF_HEIGHT * aspect));
    f.extend(axis(up, HALF_HEIGHT));
    f.extend(axis(back, -1.0));
    for c in [t.viewport_top, t.viewport_bottom, t.accent, t.text] {
        f.extend(linear(c));
    }
    f.extend([if dark { 1.0 } else { 0.0 }, step, grid[0], grid[1]]);
    f.extend(settings);
    debug_assert_eq!(f.len() * 4, solidcraft_engine::plugin::shader::HOST_UNIFORM_BYTES);
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Plain, not sRGB: the view shows a native texel as a screen value and blends there, so the
/// fragment stage stores the colour sRGB-encoded and premultiplied.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Half floats, so a state keeps its precision between frames.
const FORMAT_STATE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The pipeline and bindings for `loaded`, made under error scopes: a module this graphics card
/// refuses is an error here, never a crash.
fn build(rs: &egui_wgpu::RenderState, key: &str, loaded: Loaded) -> Result<Active, String> {
    if loaded.kind != solidcraft_engine::plugin::shader::ShaderKind::Background {
        return Err("not a background".into());
    }
    let device = &rs.device;
    let scopes = [wgpu::ErrorFilter::Validation, wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal].map(|f| device.push_error_scope(f));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sc_plugin_background"),
        source: wgpu::ShaderSource::Wgsl(loaded.wgsl.as_str().into()),
    });
    let mut entries = vec![
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ];
    for i in 0..loaded.textures.len() {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: solidcraft_engine::plugin::shader::FIRST_TEXTURE_BINDING + i as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    // The state grids are read with textureLoad only, so they need not be filterable.
    if loaded.state.is_some() {
        for binding in [STATE_BINDING, STATE_BINDING + 1] {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
    }
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sc_plugin_background"), entries: &entries });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sc_plugin_background"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = make_pipeline(device, &module, &pipeline_layout, "sc_fragment", FORMAT);
    let step = loaded.state.map(|_| make_pipeline(device, &module, &pipeline_layout, "sc_state_fragment", FORMAT_STATE));
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sc_plugin_background"),
        size: solidcraft_engine::plugin::shader::HOST_UNIFORM_BYTES as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sampler = |mode: wgpu::AddressMode| {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_plugin_background"),
            address_mode_u: mode,
            address_mode_v: mode,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        })
    };
    let (clamp, repeat) = (sampler(wgpu::AddressMode::ClampToEdge), sampler(wgpu::AddressMode::Repeat));
    let views: Vec<wgpu::TextureView> = loaded.textures.iter().map(|levels| upload(&rs.device, &rs.queue, levels)).collect();
    let samplers = (&clamp, &repeat);
    let grids: Vec<wgpu::TextureView> = loaded.state.map_or_else(Vec::new, |size| (0..2).map(|_| grid(device, size)).collect());
    let bind_groups: Vec<wgpu::BindGroup> = if grids.is_empty() {
        vec![group(device, &layout, &uniforms, samplers, &views, None)]
    } else {
        grids.iter().map(|grid| group(device, &layout, &uniforms, samplers, &views, Some(grid))).collect()
    };
    let mut error = None;
    for scope in scopes.into_iter().rev() {
        if let Some(e) = pollster::block_on(scope.pop()) {
            error = Some(e.to_string());
        }
    }
    if let Some(error) = error {
        return Err(error);
    }
    let state = match (loaded.state, step) {
        (Some(size), Some(pipeline)) => {
            <[wgpu::TextureView; 2]>::try_from(grids).ok().map(|views| Sim { pipeline, views, size, current: 0, frame: 0, stepped: None })
        }
        _ => None,
    };
    Ok(Active {
        key: key.to_string(),
        animated: loaded.animated,
        shader: loaded.shader,
        pipeline,
        bind_groups,
        uniforms,
        targets: Slots::default(),
        state,
    })
}

fn make_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sc_plugin_background"),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("sc_vertex"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// One of a state shader's grids, zeroed by the GPU.
fn grid(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_plugin_background_state"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT_STATE,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// With a `grid`, both state bindings name it: the step reads it as `sc_state_prev`, the background as `sc_state`.
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    (clamp, repeat): (&wgpu::Sampler, &wgpu::Sampler),
    views: &[wgpu::TextureView],
    grid: Option<&wgpu::TextureView>,
) -> wgpu::BindGroup {
    let mut bindings = vec![
        wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(clamp) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(repeat) },
    ];
    for (i, view) in views.iter().enumerate() {
        bindings.push(wgpu::BindGroupEntry {
            binding: solidcraft_engine::plugin::shader::FIRST_TEXTURE_BINDING + i as u32,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    if let Some(grid) = grid {
        for binding in [STATE_BINDING, STATE_BINDING + 1] {
            bindings.push(wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(grid) });
        }
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sc_plugin_background"), layout, entries: &bindings })
}

pub(crate) fn upload(device: &wgpu::Device, queue: &wgpu::Queue, levels: &[([u32; 2], Vec<u8>)]) -> wgpu::TextureView {
    let [w, h] = levels.first().map_or([1, 1], |(size, _)| *size);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_plugin_background_picture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len().max(1) as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, ([lw, lh], rgba)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * lw), rows_per_image: Some(*lh) },
            wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn draw(
    rs: &egui_wgpu::RenderState,
    active: &mut Active,
    key: TargetKey,
    now: f64,
    size: [u32; 2],
    uniforms: &[u8],
    stepping: bool,
) -> Option<egui::TextureId> {
    if active.targets.use_at(&key, now).is_none_or(|t| t.size != size) {
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sc_plugin_background_target"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = rs.renderer.write();
        let id = match active.targets.remove(&key) {
            Some(old) => {
                renderer.update_egui_texture_from_wgpu_texture(&rs.device, &view, wgpu::FilterMode::Linear, old.id);
                old.id
            }
            None => renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear),
        };
        active.targets.insert(key, Target { size, view, id, drawn: None }, now);
    }
    let target = active.targets.use_at(&key, now)?;
    if target.drawn.as_deref() == Some(uniforms) {
        return Some(target.id);
    }
    target.drawn = Some(uniforms.to_vec());
    rs.queue.write_buffer(&active.uniforms, 0, uniforms);
    let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_plugin_background") });
    // The state step first, writing the grid that is not current; the background then reads it.
    // A draw between two steps reads the grid the last step wrote.
    let mut group = active.state.as_ref().map_or(0, |sim| sim.current);
    if let Some(sim) = active.state.as_mut().filter(|_| stepping) {
        let prev = sim.current;
        let next = 1 - prev;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sc_plugin_background_step"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sim.views[next],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&sim.pipeline);
            pass.set_bind_group(0, &active.bind_groups[prev], &[]);
            pass.draw(0..3, 0..1);
        }
        sim.current = next;
        group = next;
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_plugin_background"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&active.pipeline);
        pass.set_bind_group(0, &active.bind_groups[group], &[]);
        pass.draw(0..3, 0..1);
    }
    rs.queue.submit([encoder.finish()]);
    Some(target.id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_picture_gets_every_mip_level_down_to_one_pixel_and_fits_the_limit() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(40, 20).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let levels = super::mips(png.get_ref(), 16).unwrap();
        let sizes: Vec<[u32; 2]> = levels.iter().map(|(s, _)| *s).collect();
        assert_eq!(sizes, vec![[16, 8], [8, 4], [4, 2], [2, 1], [1, 1]]);
        assert!(levels.iter().all(|([w, h], rgba)| rgba.len() == (w * h * 4) as usize));
        assert!(super::mips(b"not a picture", 16).is_err());
    }

    #[test]
    fn the_host_uniforms_are_the_size_the_prelude_declares() {
        let tokens = solidcraft_theme::Tokens::get();
        let bytes =
            super::host_uniforms([800, 400], 1.5, 2.0, &solidcraft_viewport::Camera::default(), &tokens, (7.0, [96.0, 48.0]), [5.0, 0.0, 0.0, 0.0]);
        assert_eq!(bytes.len(), solidcraft_engine::plugin::shader::HOST_UNIFORM_BYTES);
        let first: Vec<f32> = bytes.chunks(4).take(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(first, vec![800.0, 400.0, 1.5, 2.0]);
        // The step and the grid's size follow the theme's dark flag, at the prelude's offsets.
        let floats: Vec<f32> = bytes.chunks(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(&floats[33..37], &[7.0, 96.0, 48.0, 5.0], "then the settings");
    }

    #[test]
    fn the_step_counts_up_and_never_wraps_to_the_reset_value() {
        assert_eq!(super::next_step(0), 1);
        assert_eq!(super::next_step(super::FRAME_WRAP - 1), super::FRAME_WRAP);
        assert_eq!(super::next_step(super::FRAME_WRAP), 1);
    }

    #[test]
    fn each_window_and_view_keeps_its_own_background_picture() {
        // The main window's view and a popped-out window's view: drawing one leaves the other's
        // picture as it was, so the main window's panels never show the other window's view.
        let main: super::TargetKey = (egui::ViewportId::ROOT, None);
        let popped: super::TargetKey = (egui::ViewportId::from_hash_of("model 2"), Some(1 << 63));
        let tile: super::TargetKey = (egui::ViewportId::ROOT, Some(2));
        let mut pictures = super::Slots::<super::TargetKey, &str>::default();
        pictures.insert(main, "main view", 1.0);
        pictures.insert(popped, "model 2", 1.0);
        pictures.insert(tile, "tile", 1.0);
        *pictures.use_at(&popped, 1.1).unwrap() = "model 2, next frame";
        assert_eq!(pictures.use_at(&main, 1.1).copied(), Some("main view"));
        assert_eq!(pictures.use_at(&tile, 1.1).copied(), Some("tile"));
        assert_eq!(pictures.use_at(&popped, 1.1).copied(), Some("model 2, next frame"));
    }

    #[test]
    fn a_closed_windows_background_picture_is_dropped_after_a_while() {
        let main: super::TargetKey = (egui::ViewportId::ROOT, None);
        let popped: super::TargetKey = (egui::ViewportId::from_hash_of("model 2"), Some(1 << 63));
        let mut pictures = super::Slots::<super::TargetKey, &str>::default();
        pictures.insert(main, "main view", 0.0);
        pictures.insert(popped, "model 2", 0.0);
        // Only the main window keeps drawing.
        for second in 1..=3 {
            pictures.use_at(&main, f64::from(second));
        }
        assert!(pictures.take_stale(3.0, super::TARGET_KEEP_SECS).is_empty(), "kept for a while");
        for second in 4..=10 {
            pictures.use_at(&main, f64::from(second));
        }
        assert_eq!(pictures.take_stale(10.0, super::TARGET_KEEP_SECS), vec!["model 2"]);
        assert_eq!(pictures.use_at(&main, 10.0).copied(), Some("main view"), "the drawn one stays");
        assert_eq!(pictures.take_all(), vec!["main view"]);
    }

    #[test]
    fn the_state_steps_once_however_many_windows_draw_it() {
        // The first draw steps; the main window and a popped-out window drawing in the same frame
        // step it once; the next frame steps again.
        assert!(super::steps_now(None, 5.0));
        assert!(!super::steps_now(Some(5.0), 5.0), "a second window in the same frame");
        assert!(!super::steps_now(Some(5.0), 5.0 + super::STEP_GAP_SECS / 2.0));
        assert!(super::steps_now(Some(5.0), 5.0 + 0.033), "the next frame at 30 per second");
        assert!(super::steps_now(Some(5.0), 1.0), "a clock that went back does not stop it");
    }

    #[test]
    fn the_background_turns_with_the_camera_but_not_with_zoom_or_pan() {
        let tokens = solidcraft_theme::Tokens::get();
        let at = |cam: &solidcraft_viewport::Camera| super::host_uniforms([800, 400], 0.0, 1.0, cam, &tokens, (0.0, [0.0, 0.0]), [0.0; 4]);
        let cam = solidcraft_viewport::Camera::default();
        let mut turned = cam;
        turned.orbit(200.0, 0.0);
        assert_ne!(at(&cam), at(&turned));
        let mut moved = cam;
        moved.distance *= 3.0;
        moved.target.x += 50.0;
        assert_eq!(at(&cam), at(&moved));
    }

    #[test]
    fn a_background_stops_when_its_plugin_is_turned_off() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.background_shader = "dusk/dusk".into();
        h.app.plugin_shaders.loading = Some(("dusk/dusk".into(), super::Pending::default()));
        assert!(super::allowed(&mut h.app), "an enabled plugin's background may draw");
        assert!(h.app.plugin_shaders.loading.is_some());
        h.app.preferences.disabled_plugins.insert("dusk".into());
        assert!(!super::allowed(&mut h.app), "a turned-off plugin's background must not draw");
        assert!(h.app.plugin_shaders.loading.is_none(), "its pending load is dropped");
        assert_eq!(h.app.plugin_shaders.failed.as_deref(), Some("dusk/dusk"));
    }

    #[test]
    fn a_background_comes_back_when_its_plugin_is_turned_on_again() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.background_shader = "dusk/dusk".into();
        h.app.preferences.disabled_plugins.insert("dusk".into());
        assert!(!super::allowed(&mut h.app));
        assert_eq!(h.app.plugin_shaders.failed.as_deref(), Some("dusk/dusk"));
        h.app.preferences.disabled_plugins.remove("dusk");
        assert!(super::allowed(&mut h.app));
        assert_eq!(h.app.plugin_shaders.failed, None, "the background is tried again");
    }

    #[test]
    fn removing_a_plugin_forgets_its_background() {
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_shaders.loading = Some(("dusk/dusk".into(), super::Pending::default()));
        super::forget(&mut h.app, "other");
        assert!(h.app.plugin_shaders.loading.is_some(), "another plugin's removal leaves it");
        super::forget(&mut h.app, "dusk");
        assert!(h.app.plugin_shaders.loading.is_none());
    }

    const PICTURE_SHADER: &str = "fn background(px: ScPixel) -> vec4<f32> {\n    return textureSampleLevel(picture, sc_clamp, px.uv, 0.0);\n}\n";

    /// A plugin `own` with one background whose texture `picture` is chosen, shipped as
    /// `default.png` (solid blue) when `shipped` is set; returns its folder.
    fn own_plugin(h: &crate::scenario::Harness, shipped: bool) -> std::path::PathBuf {
        let dir = h.dir.join("own");
        std::fs::create_dir_all(&dir).unwrap();
        let file = if shipped { r#","file":"default.png""# } else { "" };
        let manifest = format!(
            r#"{{"id":"own","name":"Own","version":"1","capabilities":["shaders@0.2"],"shaders":[{{"id":"own","name":"Own","kind":"background","source":"own.wgsl","textures":[{{"name":"picture","choose":"Picture"{file}}}]}}]}}"#
        );
        std::fs::write(dir.join("plugin.json"), manifest).unwrap();
        std::fs::write(dir.join("own.wgsl"), PICTURE_SHADER).unwrap();
        if shipped {
            write_png(&dir.join("default.png"), [0, 0, 255]);
        }
        dir
    }

    fn write_png(path: &std::path::Path, rgb: [u8; 3]) {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4, 2, image::Rgba([rgb[0], rgb[1], rgb[2], 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        std::fs::write(path, png.get_ref()).unwrap();
    }

    /// The first mip level of the texture's picture, the bytes that are drawn.
    fn first_texel(loaded: &super::Loaded) -> Vec<u8> {
        loaded.textures[0][0].1[..4].to_vec()
    }

    #[test]
    fn a_chosen_picture_replaces_the_shipped_one_and_a_broken_one_falls_back() {
        let h = crate::scenario::Harness::new();
        let dir = own_plugin(&h, true);
        let folder = dir.to_string_lossy().into_owned();
        let shipped = super::load(folder.clone(), "own/own".into(), Default::default(), 512).unwrap();
        assert_eq!(first_texel(&shipped), vec![0, 0, 255, 255]);
        let red = dir.join("red.png");
        write_png(&red, [255, 0, 0]);
        let chosen: std::collections::BTreeMap<String, String> = [("own/own/picture".to_owned(), red.to_string_lossy().into_owned())].into();
        let loaded = super::load(folder.clone(), "own/own".into(), chosen.clone(), 512).unwrap();
        assert_eq!(first_texel(&loaded), vec![255, 0, 0, 255], "the chosen picture is drawn");
        assert_ne!(super::load_key("own/own", &chosen), super::load_key("own/own", &Default::default()), "a new picture is a new key");
        let missing: std::collections::BTreeMap<String, String> =
            [("own/own/picture".to_owned(), h.dir.join("gone.png").to_string_lossy().into_owned())].into();
        let fallback = super::load(folder.clone(), "own/own".into(), missing, 512).unwrap();
        assert_eq!(first_texel(&fallback), vec![0, 0, 255, 255], "a missing chosen picture falls back to the shipped file");
        let garbage = dir.join("garbage.png");
        std::fs::write(&garbage, b"not a picture").unwrap();
        let broken: std::collections::BTreeMap<String, String> = [("own/own/picture".to_owned(), garbage.to_string_lossy().into_owned())].into();
        assert_eq!(first_texel(&super::load(folder, "own/own".into(), broken, 512).unwrap()), vec![0, 0, 255, 255]);
    }

    /// A pick that won't read or decode shows the shipped picture and is named in one status line.
    #[test]
    fn a_bad_pick_falls_back_and_is_named_in_one_status_line() {
        let mut h = crate::scenario::Harness::new();
        let dir = own_plugin(&h, true);
        let folder = dir.to_string_lossy().into_owned();
        let garbage = dir.join("garbage.png");
        std::fs::write(&garbage, b"not a picture").unwrap();
        let broken: std::collections::BTreeMap<String, String> = [("own/own/picture".to_owned(), garbage.to_string_lossy().into_owned())].into();
        let loaded = super::load(folder.clone(), "own/own".into(), broken, 512).unwrap();
        assert_eq!(first_texel(&loaded), vec![0, 0, 255, 255]);
        assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
        assert!(loaded.notes[0].contains("garbage.png"), "{:?}", loaded.notes);
        let none = super::load(folder, "own/own".into(), Default::default(), 512).unwrap();
        assert!(none.notes.is_empty(), "no pick, nothing to say");
        super::announce(&mut h.app, "own/own", &none.notes);
        assert!(h.app.status.is_none(), "nothing to say sets no status");
        super::announce(&mut h.app, "own/own", &loaded.notes);
        let (text, _, _) = h.app.status.clone().unwrap();
        assert!(text.contains("garbage.png") && text.contains("own/own"), "{text}");
    }

    #[test]
    fn a_chosen_texture_with_no_shipped_file_and_no_picture_fails_with_one_line() {
        let h = crate::scenario::Harness::new();
        let dir = own_plugin(&h, false);
        let folder = dir.to_string_lossy().into_owned();
        let error = super::load(folder.clone(), "own/own".into(), Default::default(), 512).err().unwrap();
        assert!(error.contains("no picture chosen"), "{error}");
        let gone: std::collections::BTreeMap<String, String> =
            [("own/own/picture".to_owned(), h.dir.join("gone.png").to_string_lossy().into_owned())].into();
        assert!(super::load(folder, "own/own".into(), gone, 512).is_err());
    }

    #[test]
    fn chosen_pictures_are_keyed_by_texture_name_and_checked_as_preferences() {
        let h = crate::scenario::Harness::new();
        let path = h.dir.join("pick.png").to_string_lossy().into_owned();
        assert!(super::valid_picture("own/own/picture", &path));
        assert!(super::valid_picture("own/own/city_lights2", &path));
        for (key, path) in [
            ("own/own/0", path.as_str()),
            ("own/own/3", path.as_str()),
            ("own/own", path.as_str()),
            ("own/own/", path.as_str()),
            ("Own/own/picture", path.as_str()),
            ("own/own/Picture", path.as_str()),
            ("own/own/sc_picture", path.as_str()),
            ("own/own/picture", "pick.png"),
            ("own/own/picture", ""),
        ] {
            assert!(!super::valid_picture(key, path), "{key} {path}");
        }
        assert!(!super::valid_picture("own/own/picture", &format!("{}{}", path, "x".repeat(1024))));
    }

    /// The cap is `MAX_PICTURES` entries: that many valid keys are kept and one more is refused.
    #[test]
    fn at_most_the_cap_of_valid_pictures_may_be_stored() {
        let h = crate::scenario::Harness::new();
        let path = h.dir.join("pick.png").to_string_lossy().into_owned();
        let keys = |n: usize| -> std::collections::BTreeMap<String, String> { (0..n).map(|i| (format!("p{i}/own/picture"), path.clone())).collect() };
        assert!(keys(1).keys().all(|key| super::valid_picture(key, &path)), "the keys are valid");
        assert!(super::valid_pictures(&keys(crate::plugins::MAX_PICTURES)));
        assert!(!super::valid_pictures(&keys(crate::plugins::MAX_PICTURES + 1)));
    }

    /// A plugin update that reorders its textures keeps each picture with its texture.
    #[test]
    fn a_picture_stays_with_its_texture_when_the_textures_are_reordered() {
        let h = crate::scenario::Harness::new();
        let dir = h.dir.join("reorder").join("own");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("own.wgsl"),
            "fn background(px: ScPixel) -> vec4<f32> {
    return textureSampleLevel(sky, sc_clamp, px.uv, 0.0) + textureSampleLevel(ground, sc_clamp, px.uv, 0.0);
}
",
        )
        .unwrap();
        let red = dir.join("red.png");
        write_png(&red, [255, 0, 0]);
        let write_manifest = |order: [&str; 2]| {
            let textures = order.map(|n| format!(r#"{{"name":"{n}","choose":"{n}"}}"#)).join(",");
            std::fs::write(
                dir.join("plugin.json"),
                format!(
                    r#"{{"id":"own","name":"Own","version":"1","capabilities":["shaders@0.2"],"shaders":[{{"id":"own","name":"Own","kind":"background","source":"own.wgsl","textures":[{textures}]}}]}}"#
                ),
            )
            .unwrap();
        };
        let green = dir.join("green.png");
        write_png(&green, [0, 255, 0]);
        let chosen: std::collections::BTreeMap<String, String> =
            [("own/own/sky".to_owned(), red.to_string_lossy().into_owned()), ("own/own/ground".to_owned(), green.to_string_lossy().into_owned())]
                .into();
        let folder = dir.to_string_lossy().into_owned();
        write_manifest(["sky", "ground"]);
        let before = super::load(folder.clone(), "own/own".into(), chosen.clone(), 512).unwrap();
        assert_eq!((before.textures[0][0].1[..4].to_vec(), before.textures[1][0].1[..4].to_vec()), (vec![255, 0, 0, 255], vec![0, 255, 0, 255]));
        write_manifest(["ground", "sky"]);
        let after = super::load(folder, "own/own".into(), chosen, 512).unwrap();
        assert_eq!((after.textures[0][0].1[..4].to_vec(), after.textures[1][0].1[..4].to_vec()), (vec![0, 255, 0, 255], vec![255, 0, 0, 255]));
    }

    /// Turning a plugin off keeps the pictures its person chose.
    #[test]
    fn turning_a_plugin_off_keeps_its_chosen_pictures() {
        let mut h = crate::scenario::Harness::new();
        let path = h.dir.join("pick.png").to_string_lossy().into_owned();
        h.app.preferences.background_shader = "own/own".into();
        h.app.preferences.background_shader_pictures.insert("own/own/picture".into(), path.clone());
        h.app.preferences.disabled_plugins.insert("own".into());
        assert!(!super::allowed(&mut h.app));
        assert_eq!(h.app.preferences.background_shader_pictures.get("own/own/picture"), Some(&path));
    }

    /// Choose… stores the picture under the texture's key; the preference round trip is `ui.prefs`.
    #[test]
    fn the_preference_takes_pictures_the_card_stores_and_refuses_bad_ones() {
        let mut h = crate::scenario::Harness::new();
        let path = h.dir.join("pick.png").to_string_lossy().into_owned();
        let stored = crate::prefs::control(&mut h.app, &serde_json::json!({"background_shader_pictures": {"own/own/picture": path}})).unwrap();
        assert_eq!(stored["background_shader_pictures"]["own/own/picture"], serde_json::json!(path));
        assert_eq!(h.app.preferences.background_shader_pictures.get("own/own/picture"), Some(&path));
        assert!(crate::prefs::control(&mut h.app, &serde_json::json!({"background_shader_pictures": {"own/own/BAD": path}})).is_err());
        assert!(crate::prefs::control(&mut h.app, &serde_json::json!({"background_shader_pictures": {"own/own/picture": "pick.png"}})).is_err());
        assert_eq!(h.app.preferences.background_shader_pictures.len(), 1, "a refused change stores nothing");
        assert!(super::choose_picture(&mut h.app, "own/own/picture", "x".repeat(2000)).is_err());
        assert!(super::choose_picture(&mut h.app, "own/own/picture", path.clone()).is_ok());
    }
}
