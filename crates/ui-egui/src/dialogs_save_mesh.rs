//! The Save as mesh dialog (`file.save_mesh`): which bodies and the mesh format, then a file dialog.
//! Send to Slicer (`file.send_to_slicer`) shares the dialog without the format and with a slicer to
//! send the bodies to: it always asks, and pre-selects the slicer used last time. The list also has
//! a plain text row for EasyPrint (`file.open_in_easyprint`), which works in a browser too.
//!
//! Send to Slicer also has a Print face input (#8429): it shows the print face of each body being
//! sent, takes a flat face picked in the viewport as its body's print face (in place of the one it
//! had), and clears a body's when its face is taken out. Each change runs `body.print_face` at
//! once, so it undoes like any other.
//!
//! Every place that saves a mesh opens this dialog: File › Export STL / Export 3MF, the Mesh tab, and the
//! Save As Mesh item of the body menus. The slicer list (Send to Slicer only) holds the network
//! folders the user added (#2304), the programs the user added (kept between runs, newest first),
//! then the slicers found on this machine, then "Custom…", which asks for a program and adds it at
//! the top, and "Network folder…", which asks for a folder and a name; neither is ever the
//! selection. Pressing Delete over a network folder or a program the user added removes it.
//!
//! Send to Slicer has Format and Quality rows of its own (3MF, binary or ASCII STL; standard, fine
//! or finest), remembered apart from Save as Mesh's. Their first choices are what it always sent.

use crate::selection::SelectionExt as _;
use crate::ui_kit::choice_row;
use std::collections::HashMap;

use egui::{Color32, Sense, vec2};
use serde_json::{Value, json};
use solidcraft_engine::slicers::{self, Slicer};

use crate::SolveApp;
use crate::dialogs::{Kind, form_row};
use crate::selection::{BODIES, PLANAR_FACES, SelInput};
use crate::theme::Tokens;
use solidcraft_engine::Sel;

/// The formats, as shown and as `file.save_mesh` names them.
/// (label, `format` parameter, file extension).
const FORMATS: [(&str, &str, &str); 3] = [("STL", "stl", "stl"), ("3MF", "3mf", "3mf"), ("3MF with face tags", "3mf-tagged", "3mf")];
/// The list key of the EasyPrint row (no slicer has it: ids are words, programs are paths).
const EASYPRINT: &str = "easyprint";
/// Send to Slicer's formats: the label, `file.send_to_slicer`'s `format` and `ascii`, and the name
/// the preference keeps. 3MF first: what Send to Slicer always sent.
const SEND_FORMATS: [(&str, &str, bool, &str); 3] =
    [("3MF", "3mf", false, "3mf"), ("ui.mesh_format_stl_binary", "stl", false, "stl"), ("ui.mesh_format_stl_ascii", "stl", true, "stl-ascii")];
/// How closely the mesh follows curved faces (#4747): the label and `file.save_mesh`'s `quality`.
const QUALITIES: [(&str, &str); 3] = [("Standard", "standard"), ("Fine", "fine"), ("Finest", "finest")];
/// Edge of a slicer's icon in the list.
const ICON: f32 = 16.0;
/// Most programs the user can keep in the list.
const MAX_CUSTOM: usize = 24;

/// The dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveMesh {
    format: usize,
    /// Send to Slicer's format: an index into [`SEND_FORMATS`].
    send_format: usize,
    /// Send to Slicer: only the bodies and the slicer are asked.
    open: bool,
    /// Index into [`QUALITIES`]; each of the two dialogs remembers its own.
    quality: usize,
    /// The chosen slicer: an installed one's id, or the path of one the user added.
    slicer: String,
    /// The slicer list has not been read for this opening yet.
    stale: bool,
    /// Send to Slicer: the bodies and the print faces the Print face input last showed, to tell
    /// a pick or a clear in it from a change of the bodies.
    shown: Option<(Vec<String>, Vec<Sel>)>,
}

impl SaveMesh {
    /// The chosen format, as `file.save_mesh` names it (`stl`, `3mf`, `3mf-tagged`).
    #[cfg(test)]
    pub(crate) fn format_name(&self) -> &'static str {
        FORMATS.get(self.format).map_or("stl", |f| f.1)
    }
}

/// One line of the slicer list.
struct Entry {
    key: String,
    slicer: Slicer,
    /// Added by the user (can be deleted).
    user: bool,
}

/// The slicers known to the dialog, and their icons.
#[derive(Default)]
pub struct SlicerMenu {
    detected: Vec<Slicer>,
    custom: Vec<(String, Slicer)>,
    icons: HashMap<String, Option<egui::TextureHandle>>,
    /// A program chosen in the Custom… file dialog, waiting for the open dialog to take it.
    picked: Option<String>,
}

impl SlicerMenu {
    /// Look at the machine and at the saved programs again.
    pub fn refresh(&mut self, custom_paths: &[String]) {
        self.detected = slicers::detect();
        self.set_custom(custom_paths);
    }

    fn set_custom(&mut self, paths: &[String]) {
        // A program that has been uninstalled drops out of the list (it stays in the settings).
        self.custom = paths.iter().filter_map(|p| slicers::custom(p).map(|s| (p.clone(), s))).collect();
    }

    /// The list key for a remembered one: itself; or, for one remembered under another install
    /// of the same slicer (`prusa` before it had several, `prusa#x` after an uninstall), the one
    /// install of that slicer when there is exactly one; else none.
    fn key_for(&self, key: &str) -> Option<String> {
        let entries = self.entries();
        if let Some(e) = entries.iter().find(|e| e.key == key) {
            return Some(e.key.clone());
        }
        // `prusa#x` remembered while there were two installs, and one left: the plain `prusa`. Only
        // when it is exactly one: with several, nothing says which was meant, and another is not
        // picked in its place (the choice then starts from the first, as when nothing was remembered).
        let base = key.split('#').next().unwrap_or(key);
        let mut same = entries.iter().filter(|e| !e.user && e.slicer.base_id() == base);
        match (same.next(), same.next()) {
            (Some(only), None) => Some(only.key.clone()),
            _ => None,
        }
    }

    fn entries(&self) -> Vec<Entry> {
        let user = self.custom.iter().map(|(path, s)| Entry { key: path.clone(), slicer: s.clone(), user: true });
        let found = self.detected.iter().map(|s| Entry { key: s.id.clone(), slicer: s.clone(), user: false });
        user.chain(found).collect()
    }
}

/// The dialog for `file.save_mesh` or `file.send_to_slicer`, with the choices of last time.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let open = match id {
        "file.save_mesh" => false,
        "file.send_to_slicer" | "file.open_in_easyprint" => true,
        _ => return None,
    };
    let p = &app.preferences;
    let format = FORMATS.iter().position(|f| f.1 == p.save_mesh_format).unwrap_or(0);
    let slicer = match id {
        "file.open_in_easyprint" => EASYPRINT.into(),
        _ if open => p.send_slicer.clone(),
        _ => String::new(),
    };
    let remembered = if open { &p.send_quality } else { &p.save_mesh_quality };
    let quality = QUALITIES.iter().position(|q| q.1 == *remembered).unwrap_or(0);
    let send_format = SEND_FORMATS.iter().position(|f| f.3 == p.send_format).unwrap_or(0);
    let kind = Kind::SaveMesh(SaveMesh { format, send_format, quality, open, slicer, stale: true, shown: None });
    let mut inputs = vec![SelInput::new("Bodies", BODIES, true)];
    if open {
        inputs.push(SelInput::new("Print face", PLANAR_FACES, true));
    }
    Some((kind, inputs))
}

/// Send to Slicer needs a way to start a program (a browser has none) or a way to reach EasyPrint.
pub fn available(app: &SolveApp) -> bool {
    app.services.pick_program.is_some() || crate::easyprint_ui::available(app)
}

/// A slicer that was started alone and is being waited for, off the UI thread, until it can take
/// the file (`Target::ready` in `slicers`). One at a time: a second send while it lasts does nothing.
pub struct SlicerWait {
    done: std::sync::mpsc::Receiver<bool>,
    /// The slicer that was started, found once.
    slicer: slicers::Slicer,
    /// What `file.send_to_slicer` answered when it wrote the file: the file that is sent, not
    /// exported again, however the design has changed since.
    result: Value,
}

/// Look at `ready` on another thread until it says so or `limit` runs out; the answer arrives on the
/// channel.
fn spawn_wait(
    ready: impl FnMut(std::time::Duration) -> bool + Send + 'static,
    limit: std::time::Duration,
    poll: std::time::Duration,
) -> std::sync::mpsc::Receiver<bool> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(slicers::wait_until(ready, limit, poll));
    });
    rx
}

/// Run `file.send_to_slicer` and report what happened. A slicer that has to be started first is
/// started and waited for in the background.
fn send_to_slicer(app: &mut SolveApp, params: Value) {
    let _language = crate::i18n::scope(&app.preferences.language);
    if let Some(wait) = &app.slicer_wait {
        // Whatever was picked this time (another slicer, other bodies): nothing else is sent.
        let line = crate::i18n::format("ui.slicer_still_starting", &[("slicer", &wait.slicer.name)]);
        app.set_status(line, false);
        return;
    }
    if let Some(target) = crate::network_folder_ui::sending(app) {
        let line = crate::i18n::format("ui.network_folder_still_sending", &[("target", target)]);
        app.set_status(line, false);
        return;
    }
    let mut params = params;
    params["single_instance"] = json!(app.preferences.slicer_single_instance);
    params["wait"] = json!("return");
    let Ok(result) = app.run("file.send_to_slicer", params.clone()) else { return };
    // A network folder: the copy goes on in the background, and the status line follows it.
    if crate::network_folder_ui::follow(app, &result) {
        return;
    }
    if result.get("starting").is_none() {
        return report(app, &result);
    }
    begin_wait(app, params, &result);
}

/// The command started the slicer on its own (`result.starting`): wait for it in the background.
fn begin_wait(app: &mut SolveApp, params: Value, result: &Value) {
    let name = result.get("starting").and_then(Value::as_str).unwrap_or_default().to_string();
    let path = result.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    let spec = params["slicer"].as_str().unwrap_or_default().to_string();
    let Some(slicer) = slicers::resolve(&spec) else {
        // Started, but it can't be found again to be sent the file: say so rather than go quiet.
        app.set_status(crate::i18n::format("ui.slicer_lost", &[("slicer", &name), ("path", &path)]), true);
        return;
    };
    // Same reading as `slicers::wait_for_ready`: when `busctl` is missing or fails, send; when it is
    // only slow, keep waiting.
    let probe = slicer.clone();
    let done =
        spawn_wait(move |left| probe.is_ready_within(left.min(slicers::PROBE_LIMIT)) != Some(false), slicers::READY_LIMIT, slicers::READY_POLL);
    app.slicer_wait = Some(SlicerWait { done, slicer, result: result.clone() });
    app.set_status(crate::i18n::format("ui.slicer_starting", &[("slicer", &name)]), false);
}

/// Per frame: when the slicer being waited for is ready, send the file; when the wait ran out, say so.
pub fn tick_wait(app: &mut SolveApp, ctx: &egui::Context) {
    use std::sync::mpsc::TryRecvError;
    crate::network_folder_ui::tick(app, ctx);
    let Some(wait) = app.slicer_wait.as_ref() else { return };
    let ready = match wait.done.try_recv() {
        Err(TryRecvError::Empty) => return ctx.request_repaint_after(std::time::Duration::from_millis(200)),
        Ok(ready) => ready,
        Err(TryRecvError::Disconnected) => false,
    };
    let Some(wait) = app.slicer_wait.take() else { return };
    let _language = crate::i18n::scope(&app.preferences.language);
    let path = wait.result.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    if ready && path.is_empty() {
        // Nothing was written to send (the answer had no path): say so, rather than open "".
        app.set_status(crate::i18n::format("ui.slicer_no_file", &[("slicer", &wait.slicer.name)]), true);
    } else if ready {
        // The file is already written: open that one, rather than export the design again.
        let mut result = wait.result;
        if let Some(obj) = result.as_object_mut() {
            obj.remove("starting");
            // After a start the file has to reach the instance that is now running, so the flag goes
            // on whatever the preference says (the preference is for a slicer that was already up).
            match wait.slicer.open(&path, true) {
                Ok(()) => obj.insert("slicer".into(), json!(wait.slicer.name)),
                Err(e) => obj.insert("slicer_error".into(), json!(e)),
            };
        }
        report(app, &result);
    } else {
        app.set_status(crate::i18n::format("ui.slicer_no_response", &[("slicer", &wait.slicer.name), ("path", &path)]), true);
    }
}

/// Open the dialog with `format` (`stl`, `3mf` or `3mf-tagged`) chosen, whatever was used last:
/// File › Export STL… and Export 3MF…. The user can still change it in the dialog.
pub fn start_with_format(app: &mut SolveApp, format: &str) {
    app.start("file.save_mesh");
    if let Some(index) = FORMATS.iter().position(|f| f.1 == format)
        && let Some(Kind::SaveMesh(k)) = app.dialog.as_mut().map(|d| &mut d.kind)
    {
        k.format = index;
    }
}

/// A popup with the slicer list is open (Delete belongs to it, not to the design's selection).
pub fn popup_open(ctx: &egui::Context) -> bool {
    let frame = ctx.cumulative_frame_nr();
    let at = ctx.data(|d| d.get_temp::<u64>(egui::Id::new("sc_slicer_popup")));
    at.is_some_and(|n| n + 1 >= frame)
}

/// The icon of an entry as a texture (`None`: no icon could be read).
fn icon(menu: &mut SlicerMenu, ctx: &egui::Context, e: &Entry) -> Option<egui::TextureId> {
    let tex = menu.icons.entry(e.key.clone()).or_insert_with(|| {
        let bytes = e.slicer.icon.as_ref()?.load()?;
        let img = decode(&bytes)?;
        Some(ctx.load_texture(format!("slicer-icon-{}", e.key), img, egui::TextureOptions::LINEAR))
    });
    tex.as_ref().map(egui::TextureHandle::id)
}

/// An icon file (PNG or ICO) as a small square image.
fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    const SIDE: u32 = 48;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let small = image::imageops::resize(&img.to_rgba8(), SIDE, SIDE, image::imageops::FilterType::Lanczos3);
    Some(egui::ColorImage::from_rgba_unmultiplied([SIDE as usize, SIDE as usize], small.as_raw()))
}

/// `text` on one line, ending in … when it is wider than `width`.
fn fitted(ui: &egui::Ui, text: &str, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), egui::FontId::proportional(Tokens::get().font_button), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}

/// An icon, or the first letter of the name on a tile when there is none.
fn paint_icon(ui: &egui::Ui, rect: egui::Rect, tex: Option<egui::TextureId>, name: &str) {
    match tex {
        Some(id) => {
            ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            let t = Tokens::get();
            ui.painter().rect_filled(rect, t.radius_sm.saturating_sub(1), t.chip);
            let letter = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                letter,
                egui::FontId::proportional(crate::theme::Tokens::get().font_small),
                t.text,
            );
        }
    }
}

/// Every body of the design, in model order; with `visible`, only the bodies the viewport shows.
pub(crate) fn body_picks(app: &SolveApp, visible: bool) -> Vec<Sel> {
    let st = app.session.world_state();
    let shown = |name: &str| !visible || app.session.body_visible(name);
    st.bodies.iter().filter(|b| shown(&b.name)).map(|b| Sel::Body { name: b.name.clone() }).collect()
}

/// Keep the Print face input and the design in step: it holds the print face of each body being
/// sent. A face picked into it becomes the print face of its body, in place of the one it had; a
/// face taken out of it clears its body's. When the bodies change, it shows their print faces.
fn sync_print_face(app: &mut SolveApp, k: &mut SaveMesh, inputs: &mut [SelInput]) {
    let bodies = crate::selection::items(inputs, 0).body_names();
    let Some(input) = inputs.get_mut(1) else { return };
    match k.shown.take() {
        Some((shown_bodies, shown)) if shown_bodies == bodies && shown != input.items => {
            let picked = input.items.clone();
            // New faces first: a body picked anew keeps its new face, and is not then cleared
            // because its old one has left the input.
            let mut done: Vec<String> = Vec::new();
            for face in picked.iter().filter(|f| !shown.contains(f)) {
                let Sel::Face { body, .. } = face else { continue };
                if !bodies.contains(body) {
                    app.set_status(crate::i18n::tr("ui.print_face_body_not_sent"), true);
                } else if !done.contains(body) {
                    crate::print_face_ui::set(app, body, Some(face));
                    done.push(body.clone());
                }
            }
            for face in shown.iter().filter(|f| !picked.contains(f)) {
                let Sel::Face { body, .. } = face else { continue };
                if !done.contains(body) {
                    crate::print_face_ui::set(app, body, None);
                    done.push(body.clone());
                }
            }
        }
        Some(shown) if shown.0 == bodies => {
            k.shown = Some(shown);
            return;
        }
        _ => {}
    }
    let current: Vec<Sel> = bodies.iter().filter_map(|b| crate::print_face_ui::current(app, b)).collect();
    input.items = current.clone();
    k.shown = Some((bodies, current));
}

/// The rows after the Bodies input; true when Enter was pressed in a field.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut SaveMesh, inputs: &mut [SelInput]) -> bool {
    if k.open {
        sync_print_face(app, k, inputs);
    }
    // One row each: the two side by side do not fit the dialog's width.
    for (label, visible) in [("Add all bodies", false), ("Add visible bodies", true)] {
        form_row(ui, "", |ui| {
            if crate::ui_kit::button(ui, crate::i18n::tr_label(label), crate::ui_kit::Button::Small).clicked()
                && let Some(input) = inputs.first_mut()
            {
                input.items = body_picks(app, visible);
            }
        });
    }
    if !k.open {
        choice_row(ui, "Format", "sm_format", &mut k.format, &FORMATS.map(|f| f.0));
        choice_row(ui, "Quality", "sm_quality", &mut k.quality, &QUALITIES.map(|q| q.0));
        return false;
    }
    if k.stale {
        // Look at the machine once per opening, then settle on a slicer that exists.
        k.stale = false;
        app.slicer_menu.picked = None;
        app.slicer_menu.refresh(&app.preferences.custom_slicers);
        let entries = app.slicer_menu.entries();
        let easyprint = crate::easyprint_ui::available(app);
        let remotes = network_folders(app);
        k.slicer = if easyprint && k.slicer == EASYPRINT {
            EASYPRINT.into()
        } else if remotes.iter().any(|t| t.id() == k.slicer) {
            k.slicer.clone()
        } else {
            app.slicer_menu
                .key_for(&k.slicer)
                .or_else(|| remotes.first().map(|t| t.id()))
                .or_else(|| entries.first().map(|e| e.key.clone()))
                .or_else(|| easyprint.then(|| EASYPRINT.to_string()))
                .unwrap_or_default()
        };
    }

    form_row(ui, "Slicer", |ui| {
        if let Some(path) = app.slicer_menu.picked.take() {
            match add_custom(app, &path) {
                Ok(()) => k.slicer = path,
                Err(e) => app.set_status(e, true),
            }
        }
        let choosing = app.file_picker_busy();
        ui.add_enabled_ui(!choosing, |ui| slicer_combo(app, ui, k));
    });
    // EasyPrint takes its own file; the slicers and network folders take these.
    if k.slicer != EASYPRINT {
        choice_row(ui, "Format", "sm_send_format", &mut k.send_format, &SEND_FORMATS.map(|f| f.0));
        choice_row(ui, "Quality", "sm_send_quality", &mut k.quality, &QUALITIES.map(|q| q.0));
    }
    false
}

/// The network folders the list offers: the user's, where the host can reach folders.
fn network_folders(app: &SolveApp) -> Vec<solidcraft_engine::remote_slicers::RemoteTarget> {
    if crate::network_folder_ui::available(app) { app.preferences.network_folders.clone() } else { Vec::new() }
}

/// Make `key` the dialog's slicer; with `instead`, only when that one is chosen now.
pub(crate) fn choose(k: &mut SaveMesh, key: &str, instead: Option<&str>) {
    if instead.is_none_or(|old| k.slicer == old) {
        k.slicer = key.to_string();
    }
}

/// A network folder row, or the selected one's icon in the closed list.
fn paint_network_icon(ui: &egui::Ui, rect: egui::Rect) {
    crate::icons::paint_icon(ui.painter(), rect, "network_folder", 1.0);
}

fn slicer_combo(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut SaveMesh) {
    let entries = app.slicer_menu.entries();
    let ctx = ui.ctx().clone();
    let current = entries.iter().find(|e| e.key == k.slicer);
    let remotes = network_folders(app);
    let current_remote = remotes.iter().find(|t| t.id() == k.slicer);
    let pad = "\u{2003}\u{2003}";
    let easyprint = crate::easyprint_ui::available(app);
    let easyprint_label = crate::i18n::tr("ui.easyprint_row");
    let text = match (current_remote, current) {
        (Some(t), _) => format!("{pad}{}", t.name),
        (None, Some(e)) => format!("{pad}{}", e.slicer.name),
        (None, None) if easyprint && k.slicer == EASYPRINT => format!("{pad}{easyprint_label}"),
        (None, None) => crate::i18n::tr_label("No slicer found").to_string(),
    };
    let (programs, network) = (app.services.pick_program.is_some(), crate::network_folder_ui::available(app));
    let mut pick_custom = false;
    let mut pick_network = false;
    let mut delete: Option<String> = None;
    let mut delete_remote: Option<String> = None;
    let mut chosen: Option<String> = None;
    let name = current_remote.map(|t| t.name.clone()).or_else(|| current.map(|e| e.slicer.name.clone()));
    // A long name ends in … instead of wrapping onto a second line; the full one is in the tooltip.
    let wrap = ui.style().wrap_mode;
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let r = crate::ui_kit::dropdown_menu(ui, "sm_slicer", &text, |ui| {
        let frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_slicer_popup"), frame));
        // The network folders first, as the programs the user added come before the ones found.
        for t in &remotes {
            let key = t.id();
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            if resp.hovered() || key == k.slicer {
                let fill = if resp.hovered() { Tokens::get().hover } else { Tokens::get().selected_bg };
                ui.painter().rect_filled(rect, Tokens::get().radius_sm / 2, fill);
            }
            paint_network_icon(ui, egui::Rect::from_min_size(rect.left_center() + vec2(3.0, -ICON / 2.0), vec2(ICON, ICON)));
            let text = fitted(ui, &t.name, rect.width() - (ICON + 9.0) - 6.0, Tokens::get().text);
            ui.painter().galley(rect.left_center() + vec2(ICON + 9.0, -text.size().y / 2.0), text, Tokens::get().text);
            crate::scenario::publish_handle(&format!("dialog:Slicer:{}", t.name), rect.center());
            let hint = crate::i18n::format("ui.network_folder_delete_tip", &[("folder", &t.shown())]);
            resp.clone().on_hover_text(format!("{}\n{hint}", t.name));
            if resp.hovered() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                delete_remote = Some(t.name.clone());
            }
            if resp.clicked() {
                chosen = Some(key);
                ui.close();
            }
        }
        for e in &entries {
            let tex = icon(&mut app.slicer_menu, &ctx, e);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            let selected = e.key == k.slicer;
            if resp.hovered() || selected {
                let fill = if resp.hovered() { Tokens::get().hover } else { Tokens::get().selected_bg };
                ui.painter().rect_filled(rect, Tokens::get().radius_sm / 2, fill);
            }
            paint_icon(ui, egui::Rect::from_min_size(rect.left_center() + vec2(3.0, -ICON / 2.0), vec2(ICON, ICON)), tex, &e.slicer.name);
            let text = fitted(ui, &e.slicer.name, rect.width() - (ICON + 9.0) - 6.0, Tokens::get().text);
            let (elided, at) = (text.elided, rect.left_center() + vec2(ICON + 9.0, -text.size().y / 2.0));
            ui.painter().galley(at, text, Tokens::get().text);
            crate::scenario::publish_handle(&format!("dialog:Slicer:{}", e.slicer.name), rect.center());
            if e.user {
                let hint = crate::i18n::format("ui.slicer_delete_tip", &[("program", e.slicer.command.first().map_or("", String::as_str))]);
                resp.clone().on_hover_text(if elided { format!("{}\n{hint}", e.slicer.name) } else { hint });
                if resp.hovered() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                    delete = Some(e.key.clone());
                }
            } else if elided {
                resp.clone().on_hover_text(&e.slicer.name);
            }
            if resp.clicked() {
                chosen = Some(e.key.clone());
                ui.close();
            }
        }
        if easyprint {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            if resp.hovered() || k.slicer == EASYPRINT {
                let fill = if resp.hovered() { Tokens::get().hover } else { Tokens::get().selected_bg };
                ui.painter().rect_filled(rect, 2.0, fill);
            }
            let text = fitted(ui, easyprint_label, rect.width() - (ICON + 9.0) - 6.0, Tokens::get().text);
            ui.painter().galley(rect.left_center() + vec2(ICON + 9.0, -text.size().y / 2.0), text, Tokens::get().text);
            crate::scenario::publish_handle("dialog:Slicer:EasyPrint", rect.center());
            if resp.clicked() {
                chosen = Some(EASYPRINT.to_string());
                ui.close();
            }
        }
        if !entries.is_empty() || !remotes.is_empty() || easyprint {
            ui.separator();
        }
        // A browser has no program to add, nor a folder.
        let add_row = |ui: &mut egui::Ui, label: &str, tip: &str, handle: &str| {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, Tokens::get().radius_sm / 2, Tokens::get().hover);
            }
            ui.painter().text(
                rect.left_center() + vec2(ICON + 9.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(crate::theme::Tokens::get().font_button),
                Tokens::get().text,
            );
            crate::scenario::publish_handle(handle, rect.center());
            let clicked = resp.on_hover_text(tip).clicked();
            if clicked {
                ui.close();
            }
            clicked
        };
        if programs {
            pick_custom = add_row(ui, crate::i18n::tr_label("Custom…"), crate::i18n::tr("ui.custom_program_tip"), "dialog:Slicer:Custom");
        }
        if network {
            pick_network =
                add_row(ui, crate::i18n::tr("ui.network_folder_row"), crate::i18n::tr("ui.network_folder_tip"), "dialog:Slicer:NetworkFolder");
        }
    });
    ui.style_mut().wrap_mode = wrap;
    if let Some(name) = name {
        r.clone().on_hover_text(name);
    }
    let at = egui::Rect::from_min_size(r.rect.left_center() + vec2(ui.spacing().button_padding.x + 1.0, -ICON / 2.0), vec2(ICON, ICON));
    if current_remote.is_some() {
        paint_network_icon(ui, at);
    } else if let Some(e) = current {
        let tex = icon(&mut app.slicer_menu, &ctx, e);
        paint_icon(ui, at, tex, &e.slicer.name);
    }
    if let Some(key) = chosen {
        k.slicer = key;
    }
    if let Some(name) = delete_remote {
        let key = format!("{}{name}", solidcraft_engine::remote_slicers::ID_PREFIX);
        crate::network_folder_ui::remove(app, &name);
        if k.slicer == key {
            let first = network_folders(app).first().map(|t| t.id());
            k.slicer = first.or_else(|| app.slicer_menu.entries().first().map(|e| e.key.clone())).unwrap_or_default();
        }
    }
    if pick_network {
        crate::network_folder_ui::open_new(app, true);
    }
    if let Some(key) = delete {
        app.preferences.custom_slicers.retain(|p| *p != key);
        app.slicer_menu.set_custom(&app.preferences.custom_slicers);
        if k.slicer == key {
            k.slicer = app.slicer_menu.entries().first().map(|e| e.key.clone()).unwrap_or_default();
        }
    }
    if pick_custom {
        // Closing the file dialog without a program leaves the choice as it was.
        app.pick_program(|app, path| app.slicer_menu.picked = path);
    }
}

/// Put a program the user picked at the top of the list (and keep it).
fn add_custom(app: &mut SolveApp, path: &str) -> Result<(), String> {
    if slicers::custom(path).is_none() {
        return Err(crate::i18n::format("ui.not_a_program", &[("path", path)]));
    }
    let list = &mut app.preferences.custom_slicers;
    list.retain(|p| p != path);
    list.insert(0, path.to_string());
    list.truncate(MAX_CUSTOM);
    app.slicer_menu.set_custom(&app.preferences.custom_slicers);
    Ok(())
}

/// What the Save (or Send) button asks `file.save_mesh` or `file.send_to_slicer` to do; Save is
/// without the file name.
pub fn commands(k: &SaveMesh, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let format = FORMATS.get(k.format).map_or("stl", |f| f.1);
    let mut p = json!({ "format": format });
    // Standard is the command's own default, so it is left out. Sending sets its own below.
    if let Some((_, quality)) = QUALITIES.get(k.quality).filter(|_| k.quality > 0 && !k.open) {
        p["quality"] = json!(quality);
    }
    let names = crate::selection::items(inputs, 0).body_names();
    if names.is_empty() {
        return Err(crate::i18n::tr("ui.save_mesh_pick_bodies").into());
    }
    p["bodies"] = json!(names);
    if k.open {
        if k.slicer.is_empty() {
            return Err(crate::i18n::tr("ui.save_mesh_no_slicer_open").into());
        }
        p["format"] = json!("3mf");
        if k.slicer == EASYPRINT {
            return Ok(vec![("file.open_in_easyprint".into(), p)]);
        }
        // Standard and binary are the command's defaults, so they are left out.
        let (_, format, ascii, _) = SEND_FORMATS.get(k.send_format).copied().unwrap_or(SEND_FORMATS[0]);
        p["format"] = json!(format);
        if ascii {
            p["ascii"] = json!(true);
        }
        if let Some((_, quality)) = QUALITIES.get(k.quality).filter(|_| k.quality > 0) {
            p["quality"] = json!(quality);
        }
        p["slicer"] = json!(k.slicer);
        return Ok(vec![("file.send_to_slicer".into(), p)]);
    }
    Ok(vec![("file.save_mesh".into(), p)])
}

/// Save and Send need at least one body.
pub fn ready(inputs: &[SelInput]) -> bool {
    !crate::selection::items(inputs, 0).body_names().is_empty()
}

/// The command the dialog is for: its name is the dialog's title.
pub fn command_id(k: &SaveMesh) -> &'static str {
    if k.open { "file.send_to_slicer" } else { "file.save_mesh" }
}

/// The button's label: Save, or Send when the file goes to a slicer.
pub fn button(k: &SaveMesh) -> &'static str {
    if k.open { "Send" } else { "Save" }
}

/// Save: ask for a file name and write the file. Send: write it to the slicer folder and start the
/// slicer with it, with no file dialog.
pub fn run(app: &mut SolveApp, k: &SaveMesh, inputs: &[SelInput]) -> Result<(), String> {
    let (_, params) = commands(k, inputs)?.into_iter().next().ok_or("nothing to save")?;
    if k.open {
        app.preferences.send_slicer = k.slicer.clone();
        if k.slicer == EASYPRINT {
            crate::easyprint_ui::request(app, params);
        } else {
            app.preferences.send_format = SEND_FORMATS.get(k.send_format).map_or("3mf", |f| f.3).into();
            app.preferences.send_quality = QUALITIES.get(k.quality).map_or("standard", |q| q.1).into();
            send_to_slicer(app, params);
        }
        return Ok(());
    }
    let (format, ext) = FORMATS.get(k.format).map_or(("stl", "stl"), |f| (f.1, f.2));
    let name = crate::selection::items(inputs, 0).body_names().first().cloned().unwrap_or_else(|| app.session.doc.name.clone());
    app.preferences.save_mesh_format = format.into();
    app.preferences.save_mesh_quality = QUALITIES.get(k.quality).map_or("standard", |q| q.1).into();
    // The file dialog answers later; the dialog closes meanwhile, as for every export.
    app.pick_save(&format!("{name}.{ext}"), &[ext], move |app, path| {
        if let Some(path) = path {
            save(app, params, &path);
        }
    });
    Ok(())
}

/// Run `file.save_mesh` into `path` and report what happened.
fn save(app: &mut SolveApp, mut params: Value, path: &str) {
    // Runs from the file picker's answer, outside the frame's language scope.
    let _language = crate::i18n::scope(&app.preferences.language);
    params["path"] = json!(path);
    let Ok(result) = app.run("file.save_mesh", params) else { return };
    report(app, &result);
}

/// The status line after a save or a send.
fn report(app: &mut SolveApp, result: &Value) {
    let path = result.get("path").and_then(Value::as_str).unwrap_or_default();
    // A network folder: where the copy went (no program was started, nothing is replaced).
    if result.get("remote").and_then(Value::as_bool) == Some(true) {
        let target = result.get("target").and_then(Value::as_str).unwrap_or_default();
        match (result.get("slicer_error").and_then(Value::as_str), result.get("sent_to").and_then(Value::as_str)) {
            (Some(e), _) => app.set_status(e.to_string(), true),
            (None, Some(to)) => app.set_status(crate::i18n::format("ui.sent_to_network_folder", &[("target", target), ("path", to)]), false),
            (None, None) => app.set_status(crate::i18n::format("ui.saved_to", &[("path", path)]), false),
        }
        return;
    }
    match result.get("slicer_error").and_then(Value::as_str) {
        Some(e) => app.set_status(e.to_string(), true),
        None => match result.get("slicer").and_then(Value::as_str) {
            Some(s) => {
                let mut line = match (result.get("numbered").and_then(Value::as_bool), std::path::Path::new(path).file_name()) {
                    (Some(true), Some(file)) => crate::i18n::format("ui.sent_to_slicer_copy", &[("slicer", s), ("file", &file.to_string_lossy())]),
                    _ => crate::i18n::format("ui.sent_to_slicer", &[("slicer", s)]),
                };
                // Said once: sending the same bodies again overwrites the file the slicer has open.
                if result.get("numbered").is_some() && !std::mem::replace(&mut app.preferences.slicer_resend_hint_seen, true) {
                    line = format!("{line}. {}", crate::i18n::tr("ui.slicer_resend_hint"));
                }
                if result.get("stl_fallback").and_then(Value::as_bool) == Some(true) {
                    line = format!("{line}. {}", crate::i18n::tr("ui.slicer_stl_fallback"));
                }
                app.set_status(line, false);
            }
            None => app.set_status(crate::i18n::format("ui.saved_to", &[("path", path)]), false),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::Sel;

    #[test]
    fn without_a_program_dialog_send_to_slicer_is_unavailable_and_starts_nothing() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        assert!(app.services.pick_program.is_none());
        assert!(!available(&app));
        app.preferences.send_slicer = "prusa".into();
        app.start("file.send_to_slicer");
        assert!(app.dialog.is_none(), "no dialog to choose a slicer in");
        assert!(matches!(&app.status, Some((_, _, true))));
        app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        assert!(available(&app));
    }

    /// An app with a box in it, `fake-slicer` (a script that writes its arguments to `seen`) among the
    /// user's programs, and the parameters Send to Slicer would send.
    #[cfg(unix)]
    fn app_with_fake_slicer(tag: &str) -> (SolveApp, std::path::PathBuf, Value) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("solidcraft-slicer-wait-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (script, seen) = (dir.join("fake-slicer"), dir.join("seen"));
        std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {}\n", seen.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"})).unwrap();
        app.preferences.custom_slicers = vec![script.to_string_lossy().into_owned()];
        let params = json!({"slicer": script, "bodies": ["Part"], "wait": "no"});
        (app, seen, params)
    }

    #[cfg(unix)]
    fn sent(seen: &std::path::Path) -> usize {
        for _ in 0..100 {
            if seen.exists() && std::fs::metadata(seen).map(|m| m.len() > 0).unwrap_or(false) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        std::fs::read_to_string(seen).map(|t| t.lines().count()).unwrap_or(0)
    }

    #[cfg(unix)]
    #[test]
    fn the_menu_entries_always_ask_and_a_script_with_a_slicer_does_not() {
        let (mut app, seen, mut params) = app_with_fake_slicer("ask");
        app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        app.run("select.set", json!({"items": [{"type": "body", "name": "Part"}]})).unwrap();
        // Last time's slicer is remembered and bodies are selected: still the picker, with it pre-selected.
        let key = params["slicer"].as_str().unwrap().to_string();
        app.preferences.send_slicer = key.clone();
        app.start("file.send_to_slicer");
        let Some(Kind::SaveMesh(k)) = app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("no picker opened") };
        assert!(k.open);
        assert_eq!(k.slicer, key, "pre-selected");
        assert!(!seen.exists(), "nothing sent until the user confirms");
        // A script names the slicer: no dialog, the file goes.
        app.dialog = None;
        params["wait"] = json!("no");
        app.run("file.send_to_slicer", params).unwrap();
        assert!(app.dialog.is_none());
        assert_eq!(sent(&seen), 1);
        let _ = std::fs::remove_dir_all(seen.parent().unwrap());
        solidcraft_engine::slicers::set_chosen_programs(&[]);
    }

    #[cfg(unix)]
    #[test]
    fn a_started_slicer_is_waited_for_and_then_sent_the_file_once() {
        let ctx = egui::Context::default();
        let (mut app, seen, params) = app_with_fake_slicer("ready");
        // Ready on the third look; a second click while it waits starts and sends nothing.
        let mut looks = 0;
        let done = spawn_wait(
            move |_| {
                looks += 1;
                looks == 3
            },
            std::time::Duration::from_secs(5),
            std::time::Duration::from_millis(5),
        );
        let slicer = slicers::custom(params["slicer"].as_str().unwrap()).unwrap();
        app.slicer_wait = Some(SlicerWait { done, slicer, result: json!({"path": "/p"}) });
        send_to_slicer(&mut app, params);
        assert!(app.slicer_wait.is_some(), "still the one wait");
        let status = app.status.clone().unwrap();
        assert!(!status.2 && status.0.contains("Still waiting") && status.0.contains("Nothing else was sent"), "{status:?}");
        assert!(status.0.contains(&app.slicer_wait.as_ref().unwrap().slicer.name), "names the slicer being waited for: {status:?}");
        assert!(!seen.exists(), "the second click sent nothing");
        for _ in 0..200 {
            tick_wait(&mut app, &ctx);
            if app.slicer_wait.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(app.slicer_wait.is_none());
        assert_eq!(sent(&seen), 1, "one send");
        assert!(app.status.as_ref().is_some_and(|(_, _, error)| !*error), "{:?}", app.status);
        let _ = std::fs::remove_dir_all(seen.parent().unwrap());
        solidcraft_engine::slicers::set_chosen_programs(&[]);
    }

    #[test]
    fn a_started_slicer_that_can_not_be_found_again_is_reported() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let result = json!({"starting": "Ghost Slicer", "path": "/tmp/part.3mf"});
        begin_wait(&mut app, json!({"slicer": "no such slicer anywhere", "bodies": ["Part"]}), &result);
        assert!(app.slicer_wait.is_none());
        let status = app.status.clone().unwrap();
        assert!(status.2 && status.0.contains("Ghost Slicer") && status.0.contains("/tmp/part.3mf"), "{status:?}");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_that_was_written_is_sent_whatever_happens_to_the_design_meanwhile() {
        let ctx = egui::Context::default();
        let (mut app, seen, params) = app_with_fake_slicer("same-file");
        let dir = seen.parent().unwrap().to_path_buf();
        let file = dir.join("part.3mf");
        std::fs::write(&file, b"as exported").unwrap();
        let slicer = slicers::custom(params["slicer"].as_str().unwrap()).unwrap();
        let done = spawn_wait(|_| true, std::time::Duration::from_secs(5), std::time::Duration::from_millis(5));
        let result = json!({"path": file, "numbered": false, "starting": slicer.name});
        app.slicer_wait = Some(SlicerWait { done, slicer, result });
        // The design changes while the slicer starts: a body is added and the first one removed.
        app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "Late", "operation": "new"})).unwrap();
        app.run("solid.remove", json!({"bodies": ["Part"]})).unwrap();
        let revision = app.session.revision;
        for _ in 0..200 {
            tick_wait(&mut app, &ctx);
            if app.slicer_wait.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(sent(&seen), 1);
        assert_eq!(std::fs::read_to_string(&seen).unwrap().trim(), file.to_string_lossy(), "the file already written");
        assert_eq!(std::fs::read(&file).unwrap(), b"as exported", "not written again");
        assert_eq!(app.session.revision, revision, "no command ran for the send");
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["fake-slicer", "part.3mf", "seen"], "no second file");
        assert!(app.status.as_ref().is_some_and(|(m, _, error)| !*error && m.contains("Sent")), "{:?}", app.status);
        let _ = std::fs::remove_dir_all(dir);
        solidcraft_engine::slicers::set_chosen_programs(&[]);
    }

    #[cfg(unix)]
    #[test]
    fn the_launcher_gets_the_flag_on_a_send_and_after_a_start_even_when_the_preference_is_off() {
        use std::os::unix::fs::PermissionsExt;
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("solidcraft-slicer-flag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // The launcher is recognised by its command, and PrusaSlicer by its name.
        let folder = dir.join("slic3r-app-launcher");
        std::fs::create_dir_all(&folder).unwrap();
        let (script, seen) = (folder.join("PrusaSlicer"), dir.join("seen"));
        std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" >> {}\n", seen.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"})).unwrap();
        let spec = script.to_string_lossy().into_owned();
        app.preferences.custom_slicers = vec![spec.clone()];
        app.preferences.slicer_single_instance = false;
        // A normal send with the preference off: this launcher still gets the flag, it can't hand a file over without.
        app.run("file.send_to_slicer", json!({"slicer": spec, "bodies": ["Part"], "wait": "no", "single_instance": false})).unwrap();
        assert_eq!(sent(&seen), 1);
        assert!(std::fs::read_to_string(&seen).unwrap().starts_with("--single-instance "), "{:?}", std::fs::read_to_string(&seen));
        // The send that follows a start: the flag, preference or not.
        let slicer = slicers::custom(&spec).unwrap();
        assert!(slicer.waits_for_ready().is_some());
        let done = spawn_wait(|_| true, std::time::Duration::from_secs(5), std::time::Duration::from_millis(5));
        app.slicer_wait = Some(SlicerWait { done, slicer, result: json!({"path": "/tmp/part.3mf"}) });
        for _ in 0..200 {
            tick_wait(&mut app, &ctx);
            if app.slicer_wait.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for _ in 0..100 {
            if std::fs::read_to_string(&seen).map(|t| t.lines().count()).unwrap_or(0) >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        assert_eq!(sent(&seen), 2);
        let second = std::fs::read_to_string(&seen).unwrap().lines().nth(1).unwrap().to_string();
        assert_eq!(second, "--single-instance /tmp/part.3mf");
        let _ = std::fs::remove_dir_all(dir);
        solidcraft_engine::slicers::set_chosen_programs(&[]);
    }

    #[cfg(unix)]
    #[test]
    fn a_ready_slicer_with_no_file_to_send_is_reported_and_nothing_is_started() {
        let ctx = egui::Context::default();
        let (mut app, seen, params) = app_with_fake_slicer("nofile");
        let slicer = slicers::custom(params["slicer"].as_str().unwrap()).unwrap();
        let done = spawn_wait(|_| true, std::time::Duration::from_secs(5), std::time::Duration::from_millis(5));
        app.slicer_wait = Some(SlicerWait { done, slicer, result: json!({"starting": "Fake"}) });
        for _ in 0..200 {
            tick_wait(&mut app, &ctx);
            if app.slicer_wait.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(app.slicer_wait.is_none());
        let status = app.status.clone().unwrap();
        assert!(status.2 && status.0.contains("no file"), "{status:?}");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!seen.exists(), "the slicer was not started with an empty path");
        let _ = std::fs::remove_dir_all(seen.parent().unwrap());
        solidcraft_engine::slicers::set_chosen_programs(&[]);
    }

    #[cfg(unix)]
    #[test]
    fn a_slicer_that_never_comes_up_is_reported_and_nothing_is_sent() {
        let ctx = egui::Context::default();
        let (mut app, seen, params) = app_with_fake_slicer("late");
        let done = spawn_wait(|_| false, std::time::Duration::from_millis(30), std::time::Duration::from_millis(5));
        let slicer = slicers::custom(params["slicer"].as_str().unwrap()).unwrap();
        app.slicer_wait = Some(SlicerWait { done, slicer, result: json!({"path": "/tmp/part.3mf"}) });
        for _ in 0..400 {
            tick_wait(&mut app, &ctx);
            if app.slicer_wait.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(app.slicer_wait.is_none());
        let status = app.status.clone().unwrap();
        assert!(status.2 && status.0.contains("/tmp/part.3mf"), "{status:?}");
        assert!(!seen.exists());
        let _ = std::fs::remove_dir_all(seen.parent().unwrap());
    }

    #[test]
    fn a_long_slicer_name_is_cut_with_an_ellipsis_to_fit_its_row() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let long = "PrusaSlicer (Flatpak, master, a rather long install label)";
            let galley = fitted(ui, long, 120.0, Color32::WHITE);
            assert!(galley.elided, "does not fit, so it is cut");
            assert!(galley.size().x <= 120.0 + 0.5, "{}", galley.size().x);
            assert_eq!(galley.rows.len(), 1, "one line");
            let short = fitted(ui, "OrcaSlicer", 120.0, Color32::WHITE);
            assert!(!short.elided && short.size().x <= 120.0);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn a_remembered_install_that_is_gone_falls_back_to_the_same_slicer() {
        let prusa = |id: &str| slicers::Slicer {
            id: id.into(),
            name: "PrusaSlicer".into(),
            command: vec!["prusa-slicer".into()],
            icon: None,
            prusa_major: None,
        };
        let orca =
            slicers::Slicer { id: "orca".into(), name: "OrcaSlicer".into(), command: vec!["orca-slicer".into()], icon: None, prusa_major: None };
        let menu = |detected: Vec<slicers::Slicer>| SlicerMenu { detected, ..Default::default() };
        // Two installs when it was remembered, one (now plain) left.
        let lone = menu(vec![orca.clone(), prusa("prusa")]);
        assert_eq!(lone.key_for("prusa#prusaslicer").as_deref(), Some("prusa"));
        assert_eq!(lone.key_for("prusa").as_deref(), Some("prusa"));
        // Remembered plain, now two: which of them was meant is not known, so none is taken; an exact key is itself.
        let two = menu(vec![prusa("prusa#a"), prusa("prusa#b")]);
        assert_eq!(two.key_for("prusa"), None);
        assert_eq!(two.key_for("prusa#b").as_deref(), Some("prusa#b"));
        // An AppImage remembered when it was alone, now two versions side by side: not remembered.
        let side_by_side = menu(vec![prusa("prusa#appimage-1f-prusaslicer29"), prusa("prusa#appimage-1f-prusaslicer210")]);
        assert_eq!(side_by_side.key_for("prusa#appimage-1f"), None);
        // And after an upgrade, the id is the same and is found.
        assert_eq!(menu(vec![prusa("prusa#appimage-1f")]).key_for("prusa#appimage-1f").as_deref(), Some("prusa#appimage-1f"));
        // A different slicer is never picked for it.
        assert_eq!(menu(vec![orca]).key_for("prusa#x"), None);
    }

    #[test]
    fn send_to_slicer_asks_for_bodies_a_slicer_a_format_and_a_quality() {
        let k = SaveMesh { format: 0, send_format: 0, quality: 0, open: true, slicer: "prusa".into(), stale: false, shown: None };
        let mut input = SelInput::new("Bodies", BODIES, true);
        assert!(commands(&k, std::slice::from_ref(&input)).is_err());
        input.items = vec![Sel::Body { name: "Part".into() }];
        let cmds = commands(&k, std::slice::from_ref(&input)).unwrap();
        assert_eq!(cmds, [("file.send_to_slicer".to_string(), json!({"slicer": "prusa", "bodies": ["Part"], "format": "3mf"}))]);
        // Save as mesh is a plain export in the chosen format: no slicer in it, whatever was set.
        let save = SaveMesh { format: 2, open: false, slicer: "prusa".into(), ..k.clone() };
        let (id, p) = commands(&save, std::slice::from_ref(&input)).unwrap().remove(0);
        assert_eq!((id.as_str(), p["format"].as_str(), p.get("slicer")), ("file.save_mesh", Some("3mf-tagged"), None));
        assert_eq!(button(&save), "Save");
        assert_eq!(button(&k), "Send");
        // The Format and Quality rows (#2304): ASCII STL at Fine is said; 3MF and Standard are the defaults.
        let ascii_fine = SaveMesh { send_format: 2, quality: 1, ..k.clone() };
        let (_, p) = commands(&ascii_fine, std::slice::from_ref(&input)).unwrap().remove(0);
        assert_eq!(p, json!({"slicer": "prusa", "bodies": ["Part"], "format": "stl", "ascii": true, "quality": "fine"}));
        let (_, p) = commands(&SaveMesh { send_format: 1, ..k.clone() }, std::slice::from_ref(&input)).unwrap().remove(0);
        assert_eq!((p["format"].as_str(), p.get("ascii")), (Some("stl"), None), "binary STL");
        // EasyPrint takes its own 3MF whatever the rows say.
        let (id, p) = commands(&SaveMesh { slicer: EASYPRINT.into(), ..ascii_fine }, std::slice::from_ref(&input)).unwrap().remove(0);
        assert_eq!((id.as_str(), p["format"].as_str(), p.get("quality"), p.get("ascii")), ("file.open_in_easyprint", Some("3mf"), None, None));
        assert!(commands(&SaveMesh { slicer: String::new(), ..k }, &[input]).is_err());
    }

    #[test]
    fn send_to_slicer_starts_as_it_always_sent_and_each_dialog_remembers_its_own_choices() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let opened = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Kind::SaveMesh(k)) => k.clone(),
            _ => panic!("no dialog"),
        };
        app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        app.start("file.send_to_slicer");
        let k = opened(&app);
        // Changes a default: no. A new user sends a Standard 3MF, as before the rows existed.
        assert_eq!((SEND_FORMATS[k.send_format].3, QUALITIES[k.quality].1), ("3mf", "standard"));
        app.dialog = None;
        app.preferences.send_format = "stl-ascii".into();
        app.preferences.send_quality = "fine".into();
        app.preferences.save_mesh_quality = "finest".into();
        app.start("file.send_to_slicer");
        let k = opened(&app);
        assert_eq!((SEND_FORMATS[k.send_format].3, QUALITIES[k.quality].1), ("stl-ascii", "fine"));
        app.dialog = None;
        app.start("file.save_mesh");
        assert_eq!(QUALITIES[opened(&app).quality].1, "finest", "Save as Mesh keeps its own quality");
        // Something unreadable in the settings file starts from the defaults.
        app.dialog = None;
        app.preferences.send_format = "obj".into();
        app.preferences.send_quality = "ultra".into();
        app.start("file.send_to_slicer");
        let k = opened(&app);
        assert_eq!((k.send_format, k.quality), (0, 0));
    }

    /// A share folder for a test, removed when it ends.
    struct Share(std::path::PathBuf);

    impl Share {
        fn new(tag: &str) -> Share {
            let dir = std::env::temp_dir().join(format!("solidcraft-ui-network-folder-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Share(dir)
        }
        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Share {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Frames until the status line holds `text`.
    fn until_status(h: &mut crate::scenario::Harness, text: &str) -> String {
        for _ in 0..300 {
            if let Some((line, _, _)) = &h.app.status
                && line.contains(text)
            {
                return line.clone();
            }
            h.frame();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the status line never said {text:?}: {:?}", h.app.status);
    }

    #[test]
    fn a_network_folder_is_added_in_its_window_listed_first_and_sent_to_without_a_program() {
        let share = Share::new("window");
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"})).unwrap();
        h.app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        let folder = share.path();
        h.app.services.pick_folder = Some(Box::new(move || {
            let folder = folder.clone();
            Box::pin(async move { Some(folder) })
        }));
        h.app.preferences.send_format = "stl".into();
        h.app.start("file.send_to_slicer");
        h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Part".into() }];
        h.settle();
        // "Network folder…" in the list opens the window; Browse fills the folder, Test writes there.
        h.step(&json!({"widget": "Slicer", "near": "Slicer"})).unwrap();
        h.settle();
        h.step(&json!({"click": {"handle": "dialog:Slicer:NetworkFolder"}})).unwrap();
        h.settle();
        assert!(h.widget("Add Network Folder").is_some(), "the window opened from the list");
        h.step(&json!({"click": {"handle": "dialog:NetworkFolder:Browse"}})).unwrap();
        h.settle();
        h.step(&json!({"widget": "Name", "near": "Name"})).unwrap();
        h.step(&json!({"text": "Orca on homelab"})).unwrap();
        h.settle();
        h.step(&json!({"widget": "Test"})).unwrap();
        for _ in 0..100 {
            h.frame();
            if h.widget("A file can be written there.").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.widget("A file can be written there.").is_some(), "the test result is shown");
        assert_eq!(std::fs::read_dir(&share.0).unwrap().count(), 0, "the test file is deleted");
        h.step(&json!({"widget": "Add"})).unwrap();
        h.settle();
        assert_eq!(h.app.preferences.network_folders.len(), 1, "kept in the preferences");
        assert_eq!(
            (h.app.preferences.network_folders[0].name.as_str(), h.app.preferences.network_folders[0].folder()),
            ("Orca on homelab", share.path().as_str())
        );
        let Some(Kind::SaveMesh(k)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("Send to Slicer closed") };
        assert_eq!(k.slicer, "remote:Orca on homelab", "the folder just added is the choice");
        assert_eq!(network_folders(&h.app).first().map(|t| t.id()), Some(k.slicer.clone()), "listed first");
        // Send: a binary STL under the design and body's name; no program, the path in the status line.
        let inputs = h.app.dialog.as_ref().unwrap().inputs.clone();
        run(&mut h.app, &k, &inputs).unwrap();
        let line = until_status(&mut h, "Sent to Orca on homelab");
        let sent = share.0.join("Untitled - Part.stl");
        assert!(line.contains(&sent.to_string_lossy().into_owned()), "{line}");
        assert!(!std::fs::read(&sent).unwrap().starts_with(b"PK"));
        assert_eq!(h.app.preferences.send_slicer, "remote:Orca on homelab", "remembered");
        // Again: a number is added rather than replacing the file.
        run(&mut h.app, &k, &inputs).unwrap();
        until_status(&mut h, "Untitled - Part (2).stl");
        // Delete over its row takes it out (the files sent there stay).
        crate::network_folder_ui::remove(&mut h.app, "Orca on homelab");
        assert!(h.app.preferences.network_folders.is_empty() && sent.exists());
    }

    /// Frames, up to about 20 s, until `done` holds.
    fn until(h: &mut crate::scenario::Harness, what: &str, done: impl Fn(&crate::scenario::Harness) -> bool) {
        for _ in 0..2000 {
            h.frame();
            if done(h) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("never: {what}; status {:?}", h.app.status);
    }

    #[test]
    fn an_sftp_server_is_added_in_its_window_once_its_key_is_shown_and_trusted() {
        use solidcraft_engine::remote_sftp::{self, test_server};
        let share = Share::new("sftp");
        let prints = share.0.join("prints");
        std::fs::create_dir_all(&prints).unwrap();
        let key = share.0.join("id_ed25519");
        std::fs::write(&key, test_server::key(7).to_openssh(Default::default()).unwrap().as_bytes()).unwrap();
        let authorized = vec![test_server::key(7).public_key().clone()];
        let running = test_server::start(1, test_server::Options { authorized, ..Default::default() }, &share.0).unwrap();
        let fingerprint = running.host_key.public_key().fingerprint(Default::default()).to_string();
        // Not the user's own ~/.ssh: a server only this test knows.
        *remote_sftp::TEST_SETUP.lock().unwrap() =
            Some(remote_sftp::Setup { known_hosts: None, agent: None, confirmed: None, limit: std::time::Duration::from_secs(10) });

        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"})).unwrap();
        h.app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        h.app.services.pick_folder = Some(Box::new(|| Box::pin(async { None })));
        h.app.start("file.send_to_slicer");
        h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Part".into() }];
        h.settle();
        h.step(&json!({"widget": "Slicer", "near": "Slicer"})).unwrap();
        h.settle();
        h.step(&json!({"click": {"handle": "dialog:Slicer:NetworkFolder"}})).unwrap();
        h.settle();
        h.step(&json!({"click": {"handle": "dialog:NetworkFolder:SFTP"}})).unwrap();
        h.settle();
        assert!(h.widget("Key file").is_some() && h.widget("Browse…").is_none(), "the SFTP rows replace the folder's");
        let server = format!("maker@127.0.0.1:{}", running.port);
        // Name, then Tab from field to field, as a person fills it in.
        h.step(&json!({"widget": "Name", "near": "Name"})).unwrap();
        for (i, text) in ["Orca box".to_string(), server.clone(), prints.to_string_lossy().into_owned(), key.to_string_lossy().into_owned()]
            .into_iter()
            .enumerate()
        {
            if i > 0 {
                h.step(&json!({"key": "Tab"})).unwrap();
            }
            h.step(&json!({"text": text})).unwrap();
            h.settle();
        }
        // Test: the server is new, so nothing is written; its key is shown to check, with Trust.
        h.step(&json!({"widget": "Test"})).unwrap();
        until(&mut h, "the key is shown", |h| h.widget("Trust this key").is_some());
        let words = [("fingerprint", fingerprint.as_str()), ("algorithm", "ssh-ed25519")];
        let shown = crate::i18n::format("ui.network_folder_host_unknown", &words);
        assert!(shown.contains(&fingerprint) && h.widget(&shown).is_some(), "the fingerprint is shown: {shown}");
        assert_eq!(std::fs::read_dir(&prints).unwrap().count(), 0);
        // The window grows to the answer first; the button is clicked where it then is.
        h.settle();
        h.step(&json!({"click": {"handle": "dialog:NetworkFolder:Trust"}})).unwrap();
        until(&mut h, "the test passes", |h| h.widget("A file can be written there.").is_some());
        assert!(h.widget(&crate::i18n::format("ui.network_folder_host_trusted", &words)).is_some(), "the key trusted is shown");
        assert_eq!(std::fs::read_dir(&prints).unwrap().count(), 0, "the test file is deleted");
        h.step(&json!({"widget": "Add"})).unwrap();
        h.settle();

        let saved = h.app.preferences.network_folders.first().cloned().unwrap();
        let address = saved.address().unwrap();
        assert_eq!((saved.name.as_str(), address.to_string(), saved.folder()), ("Orca box", server.clone(), prints.to_string_lossy().as_ref()));
        let stored = serde_json::to_string(&h.app.preferences.network_folders).unwrap();
        assert!(stored.contains(&fingerprint) && !stored.to_lowercase().contains("password"), "{stored}");
        let Some(Kind::SaveMesh(k)) = h.app.dialog.as_ref().map(|d| d.kind.clone()) else { panic!("Send to Slicer closed") };
        assert_eq!(k.slicer, "remote:Orca box");
        let inputs = h.app.dialog.as_ref().unwrap().inputs.clone();
        run(&mut h.app, &k, &inputs).unwrap();
        until(&mut h, "sent", |h| h.app.status.as_ref().is_some_and(|(line, _, _)| line.starts_with("Sent to Orca box")));
        let line = h.app.status.clone().unwrap().0;
        assert!(line.contains(&format!("sftp://{server}")), "{line}");
        assert_eq!(std::fs::read_dir(&prints).unwrap().count(), 1);
        // Its row in the preferences shows where the files go.
        assert!(saved.shown().starts_with(&format!("sftp://{server}/")));
        *remote_sftp::TEST_SETUP.lock().unwrap() = None;
    }

    #[test]
    fn a_network_folder_that_is_not_mounted_is_reported_and_nothing_is_made() {
        let share = Share::new("gone");
        let gone = share.0.join("not-mounted");
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"})).unwrap();
        h.app.services.pick_folder = Some(Box::new(|| Box::pin(async { None })));
        h.app.preferences.network_folders = vec![solidcraft_engine::remote_slicers::RemoteTarget::folder_target("NAS", &gone.to_string_lossy())];
        let k = SaveMesh { format: 0, send_format: 0, quality: 0, open: true, slicer: "remote:NAS".into(), stale: false, shown: None };
        let mut input = SelInput::new("Bodies", BODIES, true);
        input.items = vec![Sel::Body { name: "Part".into() }];
        run(&mut h.app, &k, &[input]).unwrap();
        let line = until_status(&mut h, "mounted");
        assert!(matches!(&h.app.status, Some((_, _, true))), "an error: {line}");
        assert!(line.contains("NAS"), "{line}");
        assert!(!gone.exists(), "a missing folder is never made");
    }

    #[test]
    fn network_folders_added_by_a_script_are_kept_in_the_preferences_and_across_restarts() {
        let share = Share::new("script");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.services.pick_folder = Some(Box::new(|| Box::pin(async { None })));
        app.run("file.slicers.add_remote", json!({"name": "Printer share", "folder": share.path()})).unwrap();
        assert!(app.dialog.is_none(), "no window for a script");
        assert_eq!(app.preferences.network_folders.len(), 1);
        let listed = app.run("file.slicers", json!({})).unwrap();
        assert_eq!((listed["slicers"][0]["id"].as_str(), listed["slicers"][0]["remote"].as_bool()), (Some("remote:Printer share"), Some(true)));
        let tested = app.run("file.slicers.test", json!({"name": "Printer share"})).unwrap();
        assert_eq!(tested["ok"], true);
        // The next start reads them back from the settings file; the design never holds them.
        let saved = app.prefs();
        assert!(!app.session.doc.to_json().contains(&share.path()));
        let mut next = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        next.load_prefs(&saved);
        assert_eq!(next.preferences.network_folders, app.preferences.network_folders);
        assert_eq!(next.session.network_folders, app.preferences.network_folders);
        next.run("file.slicers.remove_remote", json!({"name": "remote:printer share"})).unwrap();
        assert!(next.preferences.network_folders.is_empty());
        // A hand-edited settings file with a bad entry: it is left out, not a crash.
        next.preferences.network_folders = vec![solidcraft_engine::remote_slicers::RemoteTarget::folder_target("", "relative")];
        let listed = next.run("file.slicers", json!({})).unwrap();
        assert!(listed["slicers"].as_array().unwrap().iter().all(|t| t["remote"].is_null()));
    }

    #[test]
    fn the_browser_offers_no_network_folder() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.network_folders = vec![solidcraft_engine::remote_slicers::RemoteTarget::folder_target("NAS", "/mnt/nas")];
        assert!(app.services.pick_folder.is_none());
        assert!(!crate::network_folder_ui::available(&app));
        assert!(network_folders(&app).is_empty(), "no folders where none can be written");
    }

    #[test]
    fn the_print_face_input_takes_one_face_for_each_body_sent() {
        let mut h = crate::scenario::Harness::new();
        for (name, x) in [("One", 0), ("Two", 20)] {
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
        }
        h.app.services.pick_program = Some(Box::new(|| Box::pin(async { None })));
        h.app.start("file.send_to_slicer");
        let all = body_picks(&h.app, false);
        h.app.dialog.as_mut().unwrap().inputs[0].items = all;
        h.settle();
        // A click on the face of `body` at `point` in the Print face input; that face's index.
        let pick = |h: &mut crate::scenario::Harness, body: &str, point: [f64; 3]| {
            let p = solidcraft_engine::geom::Vec3::new(point[0], point[1], point[2]);
            let index = solidcraft_engine::doc::appearance::face_index_at(h.app.session.world_state().body(body).unwrap(), p).unwrap();
            let d = h.app.dialog.as_mut().unwrap();
            d.active = 1;
            d.pick(&h.app.session, Sel::Face { body: body.into(), index, point: p });
            h.settle();
            index
        };
        let shown = |h: &crate::scenario::Harness| h.app.dialog.as_ref().unwrap().inputs[1].items.len();
        let one_bottom = pick(&mut h, "One", [5.0, 5.0, 0.0]);
        let two_bottom = pick(&mut h, "Two", [25.0, 5.0, 0.0]);
        assert!(crate::print_face_ui::is_print_face(&h.app, "One", one_bottom));
        assert!(crate::print_face_ui::is_print_face(&h.app, "Two", two_bottom), "a second body takes its own face");
        assert_eq!(shown(&h), 2);
        // Another face of One takes the place of its first; Two keeps its own.
        let one_top = pick(&mut h, "One", [5.0, 5.0, 10.0]);
        assert!(crate::print_face_ui::is_print_face(&h.app, "One", one_top));
        assert!(crate::print_face_ui::is_print_face(&h.app, "Two", two_bottom));
        assert_eq!(shown(&h), 2);
        // A click on a body's print face takes it out, and clears only that body's.
        pick(&mut h, "One", [5.0, 5.0, 10.0]);
        assert!(crate::print_face_ui::current(&h.app, "One").is_none());
        assert!(crate::print_face_ui::is_print_face(&h.app, "Two", two_bottom));
        assert_eq!(shown(&h), 1);
        // Each change is a command of its own.
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(crate::print_face_ui::is_print_face(&h.app, "One", one_top));
        // With Two alone sent, the input shows its face; a face of One, not sent, is refused.
        h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::Body { name: "Two".into() }];
        h.settle();
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[1].items.len(), 1);
        pick(&mut h, "One", [5.0, 5.0, 0.0]);
        assert!(crate::print_face_ui::is_print_face(&h.app, "One", one_top), "One is not being sent: its print face stays");
        assert!(matches!(&h.app.status, Some((_, _, true))));
        assert_eq!(shown(&h), 1);
    }
}
