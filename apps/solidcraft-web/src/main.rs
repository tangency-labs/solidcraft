//! SolidCraft in the browser.
//!
//! Runs the same [`solidcraft_ui_egui::SolveApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2 (remembered until
//! `?webgpu`), `?sample` opens the sample part.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod api;
#[cfg(any(target_arch = "wasm32", test))]
mod disk;
#[cfg(target_arch = "wasm32")]
mod easyprint;
#[cfg(target_arch = "wasm32")]
mod files;
#[cfg(target_arch = "wasm32")]
mod install;
#[cfg(target_arch = "wasm32")]
mod linked;
#[cfg(any(target_arch = "wasm32", test))]
mod space_mouse;

/// The browser switches tabs with Ctrl+Tab and Ctrl+Shift+Tab, so the page leaves those keys to
/// it (#5227); plain Tab and Shift+Tab still move between the app's fields. (Ctrl+PageUp and
/// Ctrl+PageDown already reach the browser.)
#[cfg(any(target_arch = "wasm32", test))]
fn browser_switches_tabs(key: &str, ctrl: bool) -> bool {
    ctrl && key == "Tab"
}

#[cfg(any(target_arch = "wasm32", test))]
fn query_flag(query: &str, name: &str) -> bool {
    url::form_urlencoded::parse(query.trim_start_matches('?').as_bytes()).any(|(key, _)| key == name)
}

/// Launch options use exact query keys; URL values are decoded only once.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default, Debug)]
struct Launch {
    open: Option<String>,
    embed: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
impl Launch {
    fn parse(query: &str) -> Self {
        let mut launch = Self::default();
        let mut embed = false;
        for (key, value) in url::form_urlencoded::parse(query.trim_start_matches('?').as_bytes()) {
            match key.as_ref() {
                "open" if launch.open.is_none() => launch.open = Some(value.into_owned()),
                "embed" => embed = true,
                _ => {}
            }
        }
        launch.embed = embed && launch.open.is_some();
        launch
    }

    fn persists(&self) -> bool {
        !self.embed
    }

    fn full_app_url(&self, mut url: url::Url) -> String {
        url.set_query(None);
        url.set_fragment(None);
        if let Some(open) = &self.open {
            url.query_pairs_mut().append_pair("open", open);
        }
        url.to_string()
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn design_url(value: &str, page_origin: &str) -> Result<url::Url, String> {
    if value.len() > 16_384 {
        return Err("Design URL is too long".into());
    }
    let decoded = percent_encoding::percent_decode_str(value).decode_utf8().map_err(|_| "Invalid design URL encoding")?;
    if decoded.chars().any(|ch| ch.is_control() || ch == '\\') {
        return Err("Design links must not contain backslashes or control characters".into());
    }
    // Check before URL parsing normalizes away dot segments. Decode escaped slashes too.
    let path = value.split(['?', '#']).next().unwrap_or_default();
    let path = percent_encoding::percent_decode_str(path).collect::<Vec<_>>();
    if path.split(|byte| *byte == b'/').any(|segment| segment == b"..") {
        return Err("Design links must not contain parent path segments".into());
    }
    let page = url::Url::parse(page_origin).map_err(|_| "Invalid page origin")?;
    let url = if value.starts_with('/') && !value.starts_with("//") {
        page.join(value).map_err(|_| "Invalid design path")?
    } else {
        url::Url::parse(value).map_err(|_| "Invalid design URL")?
    };
    if (url.scheme() != "https" && url.origin() != page.origin())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Design links must use HTTPS or the page's own origin without credentials".into());
    }
    Ok(url)
}

#[cfg(any(target_arch = "wasm32", test))]
fn check_download_size(current: usize, incoming: u64) -> Result<(), String> {
    let cap = solidcraft_engine::io::MAX_DESIGN_BYTES as u64;
    if current as u64 > cap || incoming > cap.saturating_sub(current as u64) {
        return Err(format!("Design exceeds the {} MB size limit", cap / (1024 * 1024)));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::rc::Rc;

    use solidcraft_engine::Session;
    use solidcraft_engine::io::vfs;
    use solidcraft_ui_egui::{Services, SolveApp, ui_kit};
    use wasm_bindgen::JsCast as _;

    use crate::api::Api;
    use crate::disk::{self, SaveTarget};
    use crate::files::{self, Files, Intent};
    use crate::space_mouse::SpaceMouse;

    const CANVAS_ID: &str = "solidcraft_canvas";
    const LOADING_ID: &str = "solidcraft_loading";
    /// A 32-bit float depth buffer (no stencil): WebGPU and WebGL2 both have it, and the viewport
    /// draws it with reversed depth, whose precision follows the distance from the eye (#1434).
    const DEPTH_BITS: u8 = 32;

    /// The loading animation (assets/styles/web/loading-logo.svg) settles into the finished mark
    /// about 3.2 s after the page starts; this adds a short hold. The app doesn't land before it.
    const LOADING_MIN_MS: f64 = 3600.0;
    /// The fade from the loading screen into the app.
    const LOADING_FADE_MS: i32 = 300;

    /// Take the loading screen away once the animation has played to the finished mark: wait out
    /// what is left of `LOADING_MIN_MS`, fade it, then remove it.
    fn dismiss_loading(el: web_sys::Element) {
        let Some(window) = web_sys::window() else {
            el.remove();
            return;
        };
        let now = window.performance().map_or(LOADING_MIN_MS, |p| p.now());
        let wait = (LOADING_MIN_MS - now).max(0.0) as i32;
        let fade = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = el.set_attribute("class", "done");
            let gone = wasm_bindgen::closure::Closure::once_into_js(move || el.remove());
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(gone.unchecked_ref(), LOADING_FADE_MS);
            }
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(fade.unchecked_ref(), wait);
    }

    /// Seconds between autosaves of unsaved work.
    const AUTOSAVE_S: f64 = 30.0;

    thread_local! {
        /// An open design has unsaved changes: leaving the page asks first.
        static UNSAVED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// The browser's "leave site? changes you made may not be saved" prompt while a design has
    /// unsaved changes (the page can't show its own dialog when it is being closed).
    fn ask_before_unload(window: &web_sys::Window) {
        let on_unload = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(|e: web_sys::BeforeUnloadEvent| {
            if UNSAVED.with(std::cell::Cell::get) {
                e.prevent_default();
                e.set_return_value("unsaved");
            }
        });
        let _ = window.add_event_listener_with_callback("beforeunload", on_unload.as_ref().unchecked_ref());
        // The listener lives as long as the page.
        on_unload.forget();
    }

    /// eframe cancels the browser's action for every Tab it gets, so the browser couldn't switch
    /// tabs while the app has the keyboard. Its tab-switching keys stop here, on the way down to
    /// the canvas, before eframe sees them; the browser then acts on them as usual.
    fn leave_tab_switching_to_browser(window: &web_sys::Window) {
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
            if crate::browser_switches_tabs(&e.key(), e.ctrl_key()) {
                e.stop_propagation();
            }
        });
        let _ = window.add_event_listener_with_callback_and_bool("keydown", on_key.as_ref().unchecked_ref(), true);
        // The listener lives as long as the page.
        on_key.forget();
    }

    const JAPANESE_FONT_PATH: &str = "./fonts/BIZUDPGothic-Regular.ttf";

    type FontLoadResult = Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>>;

    struct Shell {
        app: SolveApp,
        launch: crate::Launch,
        linked: crate::linked::Download,
        full_url: Option<String>,
        api: Api,
        files: Rc<Files>,
        /// When the last autosave was (seconds), and the revision it holds (none stored: `None`).
        autosaved_at: f64,
        autosaved: Option<u64>,
        /// A stored design being renamed: (its name, the new name).
        renaming: Option<(String, String)>,
        /// The rename field takes the keyboard when it appears.
        rename_focus: bool,
        /// The start page's recent designs were filled from the stored ones.
        recents_synced: bool,
        /// On-demand CJK font: fetched only when a CJK language is selected.
        font: solidcraft_ui_egui::fonts::CjkFontLoader,
        font_result: FontLoadResult,
        /// The fetched CJK font's bytes: kept so a language change can re-order the fallbacks.
        cjk_bytes: Option<Vec<u8>>,
        /// The language the fonts were last set up for (a CJK UI leads with the CJK face).
        font_language: String,
        space_mouse: SpaceMouse,
        wheel_gestures: solidcraft_ui_egui::WebWheelGestures,
    }

    impl Shell {
        /// Files from the computer, writes to store, messages and the autosave.
        fn files_tick(&mut self, ctx: &egui::Context) {
            if !self.launch.persists() {
                return;
            }
            if self.files.ctx.borrow().is_none() {
                *self.files.ctx.borrow_mut() = Some(ctx.clone());
            }
            // A ready picker answer is consumed in app.logic later in this frame. Keep
            // incoming files queued until then so they cannot replace its target design.
            if self.app.file_picker_busy() {
                ctx.request_repaint();
                return;
            }
            files::flush(&self.files);
            // A write to the computer's disk that failed: the design is not saved after all.
            for path in self.files.unsaved.borrow_mut().drain(..) {
                if self.app.session.path.as_deref() == Some(path.as_str()) {
                    self.app.session.mark_unsaved();
                }
            }
            // Designs opened from the computer's disk (the files window): open them.
            let opened: Vec<_> = self.files.opened.borrow_mut().drain(..).collect();
            for path in opened {
                self.app.open_path(&path);
                self.files.panel.set(false);
            }
            // Recent designs whose file is gone are dropped from the list.
            for path in self.files.gone.borrow_mut().drain(..) {
                self.app.home.recent.retain(|r| *r != path);
                self.app.home.opened_at.remove(&path);
            }
            for (text, error) in self.files.messages.borrow_mut().drain(..) {
                self.app.set_status(text, error);
            }
            let mut incoming: Vec<_> = self.files.incoming.borrow_mut().drain(..).collect();
            // A glTF file's buffers picked with it are kept next to it, not opened.
            incoming.retain_mut(|(name, bytes, _)| {
                let companion =
                    std::path::Path::new(name.as_str()).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("bin"));
                if companion {
                    vfs::insert(&format!("{}{name}", files::UPLOAD), std::mem::take(bytes));
                }
                !companion
            });
            for (name, bytes, intent) in incoming {
                let design = solidcraft_engine::io::is_design_path(&name);
                let path = if design && intent == Intent::Open {
                    // A design from the computer is kept in the browser (a copy), so Save works.
                    let path = files::free_name(&name);
                    if let Err(e) = vfs::write(&path, &bytes) {
                        self.app.set_status(e.to_string(), true);
                    }
                    files::flush(&self.files);
                    path
                } else {
                    let path = format!("{}{name}", files::UPLOAD);
                    vfs::insert(&path, bytes);
                    path
                };
                match intent {
                    Intent::Open => self.app.open_path(&path),
                    Intent::Insert => self.app.drop_path(&path),
                }
                self.files.panel.set(false);
            }
            if self.files.ready.get() && !self.recents_synced {
                self.recents_synced = true;
                self.sync_recents();
            }
            let now = ctx.input(|i| i.time);
            // Unsaved work is kept (at most every AUTOSAVE_S); saved work needs no autosave.
            if self.files.ready.get() && self.files.recovery.borrow().is_none() {
                let s = &self.app.session;
                let changed = s.is_dirty() && self.autosaved != Some(s.revision);
                let wait = AUTOSAVE_S - (now - self.autosaved_at);
                if changed && wait <= 0.0 {
                    files::autosave(&self.files, Some((&s.design(), s.path.as_deref())));
                    self.autosaved = Some(s.revision);
                    self.autosaved_at = now;
                } else if changed {
                    // Come back for it even when nothing else happens.
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
                } else if !s.is_dirty() && self.autosaved.is_some() {
                    files::autosave(&self.files, None);
                    self.autosaved = None;
                }
            }
            if !self.files.ready.get() || !self.files.incoming.borrow().is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        /// The start page's recent designs are the stored ones: the remembered order first (those
        /// still stored), then the others, newest first.
        fn sync_recents(&mut self) {
            let mut entries = self.files.entries.borrow().clone();
            entries.sort_by(|a, b| b.modified.total_cmp(&a.modified));
            let forgotten = self.app.home.forgotten.clone();
            let recent = &mut self.app.home.recent;
            let known = self.files.disk_known.borrow().clone();
            // A design on the computer's disk stays listed while its file is remembered.
            recent.retain(|p| vfs::exists(p) || disk::parse(p).is_some_and(|(id, _)| known.contains(id)));
            for e in entries.iter().filter(|e| solidcraft_engine::io::is_design_path(&e.name)) {
                let p = format!("{}{}", files::OPFS, e.name);
                // A design the user removed from the list is not added back while it is stored.
                if !recent.contains(&p) && !forgotten.contains(&p) {
                    recent.push(p);
                }
            }
            recent.truncate(solidcraft_ui_egui::home::MAX_RECENT);
            // Remembered files that no recent design or autosave names are forgotten in time.
            let recovered = self.files.recovery.borrow().as_ref().and_then(|(_, p)| p.clone());
            let keep = recent.iter().chain(recovered.as_ref()).filter_map(|p| disk::parse(p)).map(|(id, _)| id.to_string()).collect();
            disk::prune(keep);
            // Drop forgotten entries whose stored design is gone, so the set does not grow without bound.
            let stored: std::collections::BTreeSet<String> =
                entries.iter().filter(|e| solidcraft_engine::io::is_design_path(&e.name)).map(|e| format!("{}{}", files::OPFS, e.name)).collect();
            self.app.home.forgotten.retain(|p| stored.contains(p));
        }

        /// The stored designs: open, insert, rename, delete, download; open from the computer.
        fn file_panel(&mut self, ctx: &egui::Context) {
            if !self.files.panel.get() {
                return;
            }
            let mut act: Option<(&'static str, String)> = None;
            let closed = solidcraft_ui_egui::ui_kit::Window::new(egui::Id::new("sc_browser_files"), "folder", "Files in this browser")
                .cards(3.0)
                .show_body(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, "Open from computer…", ui_kit::Button::Normal).clicked() {
                            files::pick_from_disk(&self.files, Intent::Open);
                        }
                        if ui_kit::button(ui, "Insert from computer…", ui_kit::Button::Normal)
                            .on_hover_text(solidcraft_ui_egui::i18n::tr("web.files.insert_drawings_hint"))
                            .clicked()
                        {
                            files::pick_from_disk(&self.files, Intent::Insert);
                        }
                        if ui_kit::button(ui, "Download open design", ui_kit::Button::Normal).clicked() {
                            act = Some(("download_open", String::new()));
                        }
                    });
                    if !disk::supported() {
                        ui.weak("This browser can't save to a folder: designs stay in this browser. Download design saves a file.");
                    }
                    ui.separator();
                    if !self.files.ready.get() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Reading the browser's storage…");
                        });
                        return;
                    }
                    let entries = self.files.entries.borrow().clone();
                    if entries.is_empty() {
                        ui.label("Designs you save appear here. They stay in this browser.");
                    }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        egui::Grid::new("opfs_files").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                            for e in &entries {
                                match &mut self.renaming {
                                    Some((from, to)) if *from == e.name => {
                                        let r = ui_kit::cell_editor(ui, ui.make_persistent_id(("stored_file_name", from.as_str())), to, "", false);
                                        if std::mem::take(&mut self.rename_focus) {
                                            r.request_focus();
                                        }
                                        if r.lost_focus() {
                                            act = Some(if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                                ("rename", to.clone())
                                            } else {
                                                ("rename_cancel", String::new())
                                            });
                                        }
                                    }
                                    _ => {
                                        ui.label(&e.name);
                                    }
                                }
                                ui.label(format!("{:.1} KB", e.size as f64 / 1024.0));
                                ui.horizontal(|ui| {
                                    if ui_kit::button(ui, "Open", ui_kit::Button::Small).clicked() {
                                        act = Some(("open", e.name.clone()));
                                    }
                                    let design = solidcraft_engine::io::is_design_path(&e.name);
                                    if !design
                                        && ui_kit::button(ui, "Insert", ui_kit::Button::Small)
                                            .on_hover_text(solidcraft_ui_egui::i18n::tr("web.files.insert_stored_drawings_hint"))
                                            .clicked()
                                    {
                                        act = Some(("insert", e.name.clone()));
                                    }
                                    if ui_kit::button(ui, "Rename", ui_kit::Button::Small).clicked() {
                                        self.renaming = Some((e.name.clone(), e.name.clone()));
                                        self.rename_focus = true;
                                    }
                                    if ui_kit::button(ui, "Save to computer…", ui_kit::Button::Small)
                                        .on_hover_text("A copy in a file you choose; the stored design stays here")
                                        .clicked()
                                    {
                                        act = Some(("save_copy", e.name.clone()));
                                    }
                                    if ui_kit::button(ui, "Download", ui_kit::Button::Small).clicked() {
                                        act = Some(("download", e.name.clone()));
                                    }
                                    if ui_kit::button(ui, "Delete", ui_kit::Button::Small).clicked() {
                                        act = Some(("delete", e.name.clone()));
                                    }
                                });
                                ui.end_row();
                            }
                        });
                    });
                });
            if closed {
                self.files.panel.set(false);
                self.renaming = None;
            }
            let Some((what, name)) = act else { return };
            let path = format!("{}{name}", files::OPFS);
            match what {
                "open" => {
                    self.app.open_path(&path);
                    self.files.panel.set(false);
                }
                "insert" => {
                    self.app.insert_path(&path);
                    self.files.panel.set(false);
                }
                "download" => match vfs::read(&path, solidcraft_engine::io::MAX_DESIGN_BYTES) {
                    Ok(b) => {
                        if let Err(e) = files::download(&name, &b) {
                            self.app.set_status(e, true);
                        }
                    }
                    Err(e) => self.app.set_status(e.to_string(), true),
                },
                "save_copy" => match vfs::read(&path, solidcraft_engine::io::MAX_DESIGN_BYTES) {
                    Ok(b) => disk::save_copy(&self.files, &name, b),
                    Err(e) => self.app.set_status(e.to_string(), true),
                },
                "download_open" => {
                    let d = &self.app.session.doc;
                    let result = self
                        .app
                        .session
                        .design_container(solidcraft_engine::io::DesignCompression::Fast, false, false)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| files::download(&format!("{}.{}", d.name, solidcraft_engine::io::DESIGN_EXT), &bytes));
                    if let Err(e) = result {
                        self.app.set_status(e, true);
                    }
                }
                "delete" => {
                    files::delete(&self.files, &name);
                    self.app.home.recent.retain(|r| *r != path);
                    // The file is gone, so there is nothing left to keep forgotten.
                    self.app.home.forgotten.remove(&path);
                }
                "rename_cancel" => self.renaming = None,
                "rename" => {
                    if let Some((from, _)) = self.renaming.take() {
                        match files::rename(&self.files, &from, &name) {
                            Ok(()) => {
                                // Recent designs follow the new name.
                                let (old, new) = (format!("{}{from}", files::OPFS), format!("{}{name}", files::OPFS));
                                for r in &mut self.app.home.recent {
                                    if *r == old {
                                        *r = new.clone();
                                    }
                                }
                                // A forgotten design follows its new name.
                                if self.app.home.forgotten.remove(&old) {
                                    self.app.home.forgotten.insert(new);
                                }
                            }
                            Err(e) => self.app.set_status(e, true),
                        }
                    }
                }
                _ => {}
            }
        }

        /// Offer the autosave an earlier session left.
        fn recovery_prompt(&mut self, ctx: &egui::Context) {
            let Some((name, path)) = self.files.recovery.borrow().clone() else { return };
            let mut answer = None;
            let _ = solidcraft_ui_egui::ui_kit::Window::new(egui::Id::new("sc_browser_recovery"), "restore", "Recover unsaved work?")
                .required()
                .show_body(ctx, |ui| {
                    ui.label(format!("“{name}” had unsaved changes when this page last closed."));
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, "Recover", ui_kit::Button::Normal).clicked() {
                            answer = Some(true);
                        }
                        if ui_kit::button(ui, "Discard", ui_kit::Button::Normal).clicked() {
                            answer = Some(false);
                        }
                    });
                });
            let Some(recover) = answer else { return };
            if recover {
                let recovery = match files::prepare_recovery(path.as_deref()) {
                    Ok(recovery) => recovery,
                    Err(error) => {
                        self.app.set_status(error, true);
                        return;
                    }
                };
                self.app.open_path(&recovery);
                if self.app.session.path.as_deref() != Some(recovery.as_str()) {
                    return;
                }
                // It belongs to its own file again (saving goes there); it is unsaved.
                self.app.session.path = path;
                self.app.session.doc_mut().name = name;
                self.app.session.mark_unsaved();
                self.app.home.open = false;
            }
            *self.files.recovery.borrow_mut() = None;
            files::autosave(&self.files, None);
        }

        /// Fetch and install the CJK font when a CJK language is selected. If that fails, this
        /// session falls back to English (no tofu); the saved language preference is kept.
        fn tick_font(&mut self, ctx: &egui::Context) {
            let done = self.font_result.borrow_mut().take();
            if let Some(res) = done {
                let installed = match res {
                    Ok(bytes) => {
                        self.cjk_bytes = Some(bytes);
                        true
                    }
                    Err(err) => {
                        log::warn!("failed to fetch CJK font from {JAPANESE_FONT_PATH}: {err}");
                        false
                    }
                };
                if installed {
                    log::info!("loaded web CJK font from {JAPANESE_FONT_PATH}");
                } else if solidcraft_ui_egui::fonts::needs_cjk_font(&self.app.preferences.language) {
                    log::warn!("CJK font unavailable; showing English for this session");
                }
                self.font.finished(installed, &mut self.app.preferences.language);
                self.apply_fonts(ctx);
                ctx.request_repaint();
            }
            if self.font.tick(&mut self.app.preferences.language) {
                self.fetch_font(ctx);
            }
            // The CJK face leads only a CJK UI: its Cyrillic glyphs are full-width, so switching
            // to Ukrainian (or English) must put the Latin face back in front.
            if self.app.preferences.language != self.font_language {
                self.apply_fonts(ctx);
            }
        }

        /// Set up the UI fonts for the live language, keeping the fetched CJK font as a fallback
        /// (it leads only for a CJK UI, see [`solidcraft_ui_egui::fonts::install_cjk_font_for`]).
        fn apply_fonts(&mut self, ctx: &egui::Context) {
            match self.cjk_bytes.clone() {
                Some(bytes) => {
                    solidcraft_ui_egui::fonts::install_cjk_font_for(ctx, bytes, &self.app.preferences.language);
                }
                None => solidcraft_ui_egui::fonts::setup_fonts(ctx),
            }
            self.font_language.clone_from(&self.app.preferences.language);
        }

        /// CJK text on screen with no CJK font yet (an English UI showing a Japanese name):
        /// fetch the font, so the text does not stay tofu (#720).
        fn watch_text_for_font(&mut self, ctx: &egui::Context) {
            if self.font.watching_text() && solidcraft_ui_egui::fonts::painted_cjk(ctx) && self.font.text_needs_font() {
                self.fetch_font(ctx);
            }
        }

        fn fetch_font(&self, ctx: &egui::Context) {
            let slot = self.font_result.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let res = fetch_font_bytes(JAPANESE_FONT_PATH).await;
                *slot.borrow_mut() = Some(res);
                ctx.request_repaint();
            });
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.tick_font(ctx);
            self.app.logic(ctx);
            self.api.poll();
            self.space_mouse.logic(&mut self.app, ctx);
            self.files_tick(ctx);
            UNSAVED.with(|u| u.set(solidcraft_ui_egui::documents::any_dirty(&self.app)));
        }
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
            if let Some(delay) = self.wheel_gestures.update(raw, now) {
                ctx.request_repaint_after(delay);
            }
            if self.launch.persists() {
                files::take_drops(&self.files, raw);
            } else {
                raw.dropped_files.clear();
            }
            // Before the app's own hook, which adds the control channel's events.
            self.space_mouse.raw_input_hook(ctx, self.app.preferences.space_mouse_enabled, raw);
            self.app.raw_input_hook(raw);
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.linked.tick(&mut self.app, ui.ctx());
            UNSAVED.with(|u| u.set(self.launch.persists() && solidcraft_ui_egui::documents::any_dirty(&self.app)));
            if self.launch.embed {
                ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Chrome).edge("embed_header", ui_kit::Edge::Top).show(ui, |ui| {
                    if ui_kit::button(ui, solidcraft_ui_egui::i18n::tr("web.open_full"), ui_kit::Button::Normal).clicked()
                        && let (Some(window), Some(url)) = (web_sys::window(), &self.full_url)
                    {
                        let _ = window.open_with_url_and_target_and_features(url, "_blank", "noopener");
                    }
                });
            }
            self.app.ui(ui);
            let ctx = ui.ctx().clone();
            if self.launch.persists() {
                self.file_panel(&ctx);
                self.recovery_prompt(&ctx);
            }
            self.watch_text_for_font(&ctx);
        }
        fn persist_egui_memory(&self) -> bool {
            self.launch.persists()
        }
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            if !self.launch.persists() {
                return;
            }
            // Persist the user's own language, not a session-only English fallback.
            let saved = self.font.persisted_language(&self.app.preferences.language).to_owned();
            let live = std::mem::replace(&mut self.app.preferences.language, saved);
            storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, self.app.prefs());
            self.app.preferences.language = live;
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    /// Where `?webgl` is remembered. The installed app starts from its manifest's start URL, which
    /// has no query, so the choice has to outlive the page it was made on.
    const WEBGL_KEY: &str = "solidcraft.webgl";

    /// Render with WebGL2: asked for now (`?webgl`, remembered) or before (until `?webgpu`).
    fn webgl() -> bool {
        let storage = crate::Launch::parse(&query()).persists().then(|| web_sys::window().and_then(|w| w.local_storage().ok().flatten())).flatten();
        let q = query();
        if crate::query_flag(&q, "webgl") {
            if let Some(s) = &storage {
                let _ = s.set_item(WEBGL_KEY, "1");
            }
            return true;
        }
        if crate::query_flag(&q, "webgpu") {
            if let Some(s) = &storage {
                let _ = s.remove_item(WEBGL_KEY);
            }
            return false;
        }
        storage.and_then(|s| s.get_item(WEBGL_KEY).ok().flatten()).is_some()
    }

    /// Fetch font bytes asynchronously through the browser.
    async fn fetch_font_bytes(url: &str) -> Result<Vec<u8>, String> {
        let window = web_sys::window().ok_or("no window")?;
        let resp_val = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url)).await.map_err(|e| format!("{e:?}"))?;
        let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "response cast failed")?;
        if !resp.ok() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let ab_val = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
        let uint8_arr = js_sys::Uint8Array::new(&ab_val);
        Ok(uint8_arr.to_vec())
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        wasm_bindgen_futures::spawn_local(async {
            let Some(window) = web_sys::window() else { return };
            let launch = crate::Launch::parse(&query());
            if launch.persists() {
                ask_before_unload(&window);
            }
            leave_tab_switching_to_browser(&window);
            let Some(document) = window.document() else { return };
            let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
                log::error!("missing <canvas id=\"{CANVAS_ID}\">");
                return;
            };
            let mut options = eframe::WebOptions { depth_buffer: DEPTH_BITS, ..Default::default() };
            if webgl()
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }
            let result = eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
                        let files = Rc::new(Files::default());
                        if launch.persists() {
                            files::start(&files);
                        }
                        let (panel, opener, saver, shown) = (files.clone(), files.clone(), files.clone(), files.clone());
                        let (restorable, restorer) = (files.clone(), files.clone());
                        let services = if launch.embed { Services::default() } else { Services {
                            // Open shows the browser's open dialog for a design where it has one
                            // (the file keeps its handle, so Save writes back to it), else the
                            // browser's files window. A command that reads another kind of file
                            // (Insert DXF or SVG, an image, a mesh, settings) gets a file dialog
                            // offering that kind, and runs on the picked file.
                            pick_open: Some(Box::new(move |exts: &[&str]| match disk::open_route(disk::supported(), exts) {
                                disk::OpenRoute::Disk => Box::pin(disk::pick_open(&opener)),
                                disk::OpenRoute::Upload => Box::pin(files::pick_upload(&opener, exts)),
                                disk::OpenRoute::Panel => {
                                    panel.panel.set(true);
                                    Box::pin(std::future::ready(None))
                                }
                            })),
                            // A design is saved to a file the user picks, or kept in the browser
                            // where the browser can't pick one; everything else is downloaded.
                            pick_save: Some(Box::new(move |name: &str, exts: &[&str]| {
                                let design = exts.contains(&solidcraft_engine::io::DESIGN_EXT);
                                match disk::save_target(disk::supported(), design) {
                                    SaveTarget::Disk => Box::pin(disk::pick_save(&saver, name)),
                                    SaveTarget::Browser => {
                                        saver.say("This browser can't save to a folder: the design is kept in this browser. File > Download design saves a file.", false);
                                        Box::pin(std::future::ready(Some(format!("{}{name}", files::OPFS))))
                                    }
                                    SaveTarget::Download => Box::pin(std::future::ready(Some(format!("{}{name}", files::DOWNLOADS)))),
                                }
                            })),
                            // A recent design on the disk, from an earlier session, opens by asking
                            // the browser for its remembered file.
                            can_restore: Some(Box::new(move |path: &str| {
                                disk::parse(path).is_some_and(|(id, _)| restorable.disk_known.borrow().contains(id))
                            })),
                            restore_file: Some(Box::new(move |path: &str| disk::restore(&restorer, path))),
                            download_design: Some(Box::new(files::download)),
                            easyprint_upload: Some(std::sync::Arc::new(crate::easyprint::upload)),
                            browser_files: Some(Box::new(move || shown.panel.set(true))),
                            install_app: Some(crate::install::service(&cc.egui_ctx)),
                            ..Default::default()
                        }};
                        // Thumbnails of saves and start-page cards.
                        solidcraft_ui_egui::gpu::install_cpu_picture_renderer();
                        let (sender, receiver) = std::sync::mpsc::channel();
                        let mut app = SolveApp::new(Session::default(), services).with_control(receiver);
                        app.embedded = launch.embed;
                        if launch.persists() && let Some(p) = cc.storage.and_then(|s| solidcraft_ui_egui::prefs::stored(|k| s.get_string(k))) {
                            app.load_prefs(&p);
                        }
                        if let Some(rs) = &cc.wgpu_render_state {
                            // eframe's web painter has no stencil buffer.
                            app.set_wgpu(rs, DEPTH_BITS, 0, 1);
                        }
                        if launch.open.is_none() && crate::query_flag(&query(), "sample") {
                            if let Err(e) = app.session.run_script(&solidcraft_engine::sample::script()) {
                                app.set_status(format!("sample: {e}"), true);
                            }
                            app.session.doc_mut().name = "Sample Plate".into();
                            app.session.mark_saved();
                        }
                        if launch.open.is_none() && !crate::query_flag(&query(), "sample") {
                            // The empty design takes Units of new designs (#1205). The sample
                            // part is built in millimetres and is left that way.
                            solidcraft_ui_egui::prefs::apply_new_design(&mut app);
                            let rev = app.session.revision;
                            app.home.show(rev);
                        }
                        let api = Api::install(sender, cc.egui_ctx.clone()).map_err(|e| format!("SolidCraft browser API: {e:?}"))?;
                        let font_language = app.preferences.language.clone();
                        let linked = crate::linked::Download::start(launch.open.as_deref(), &cc.egui_ctx);
                        let full_url = window.location().href().ok().and_then(|value| url::Url::parse(&value).ok()).map(|url| launch.full_app_url(url));
                        Ok(Box::new(Shell {
                            app,
                            launch,
                            linked,
                            full_url,
                            api,
                            files,
                            autosaved_at: f64::NEG_INFINITY,
                            autosaved: None,
                            renaming: None,
                            rename_focus: false,
                            recents_synced: false,
                            font: Default::default(),
                            font_result: Rc::new(std::cell::RefCell::new(None)),
                            cjk_bytes: None,
                            font_language,
                            space_mouse: SpaceMouse::default(),
                            wheel_gestures: solidcraft_ui_egui::WebWheelGestures::default(),
                        }))
                    }),
                )
                .await;
            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => dismiss_loading(el),
                    Err(e) => {
                        el.set_inner_html(&format!("<p>SolidCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>"))
                    }
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("solidcraft-web only runs in the browser: build it with `trunk build --release` in apps/solidcraft-web");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_query_decodes_once_and_embed_disables_persistence() {
        assert!(!query_flag("?open=https://example.org/webgl?sample", "webgl"));
        assert!(!query_flag("?open=https://example.org/webgl?sample", "sample"));
        assert!(query_flag("?embed&webgl", "webgl"));
        let launch = Launch::parse("?embed&open=https%3A%2F%2Fexample.org%2Farm.solidcraft%3Fx%3Da%2520b%26y%3D1");
        assert_eq!(launch.open.as_deref(), Some("https://example.org/arm.solidcraft?x=a%20b&y=1"));
        assert!(launch.embed);
        assert!(!launch.persists());
        assert!(!Launch::parse("?embed").embed);
        assert!(!Launch::parse("?other=embed&reopen=https://example.org").embed);
        assert!(Launch::parse("?open=https://example.org/a").persists());
        assert_eq!(Launch::parse("?open=first&open=second").open.as_deref(), Some("first"));
        let launch = Launch::parse("?embed&open=/dutchman/jobs/abc/part.solidcraft%3Fx%3Da%2520b");
        assert!(launch.embed);
        assert!(!launch.persists());
        let page = url::Url::parse("http://solidcraft.io/?embed#view").expect("synthetic page");
        let full = url::Url::parse(&launch.full_app_url(page)).expect("full app URL");
        let reopened = Launch::parse(full.query().unwrap_or_default());
        assert_eq!(reopened.open, launch.open);
        assert_eq!(reopened.open.as_deref(), Some("/dutchman/jobs/abc/part.solidcraft?x=a%20b"));
        assert!(!reopened.embed);
        assert!(reopened.persists());
        assert!(full.fragment().is_none());
    }

    #[test]
    fn design_links_refuse_hostile_schemes_credentials_and_sizes() {
        assert!(design_url("https://example.org/arm.solidcraft?download=1", "http://solidcraft.io").is_ok());
        for value in [
            "http://example.org/a",
            "data:application/json,{}",
            "blob:https://example.org/a",
            "javascript:alert(1)",
            "https://user:secret@example.org/a",
            "//evil.example/x",
            "/../x",
            "/%2e%2e/x",
            "/a/%2E./x",
            "/a%2f..%2fx",
            "/a\\x",
            "/a%5cx",
            "/a\nx",
            "/a%00x",
            "/a%7fx",
            "/a%C2%85x",
            "/a%3f/../x",
            "/a/..#view",
            "/a/..?download=1",
            "https://solidcraft.io/a/../x",
            "http://user:secret@solidcraft.io/x",
            "http://solidcraft.io.evil.example/x",
            "http://solidcraft.io:8080/x",
            "relative.solidcraft",
            "",
        ] {
            assert!(design_url(value, "http://solidcraft.io").is_err(), "{value}");
        }
        assert!(design_url(&format!("https://example.org/{}", "x".repeat(16_384)), "http://solidcraft.io").is_err());
        assert!(design_url("http://redirect.example.org/a", "http://solidcraft.io").is_err());

        let cap = solidcraft_engine::io::MAX_DESIGN_BYTES;
        assert!(check_download_size(0, cap as u64).is_ok());
        assert!(check_download_size(0, cap as u64 + 1).is_err());
        assert!(check_download_size(cap - 2, 2).is_ok());
        assert!(check_download_size(cap - 2, 3).is_err());
        assert!(check_download_size(0, u64::MAX).is_err());
        assert!(check_download_size(cap, 1).is_err());
        assert!(check_download_size(usize::MAX, 0).is_err());
    }

    #[test]
    fn design_links_allow_same_origin_paths_and_redirects() {
        for origin in ["http://solidcraft.io", "https://solidcraft.io", "http://solidcraft.io:8080"] {
            assert_eq!(design_url("/a/b.solidcraft", origin).map(|url| url.to_string()), Ok(format!("{origin}/a/b.solidcraft")));
            assert!(design_url(&format!("{origin}/x"), origin).is_ok());
            // Final response URLs follow the same origin and scheme rules as the request.
            assert!(design_url("https://docs.solidcraft.io/x", origin).is_ok());
            assert!(design_url("http://other/x", origin).is_err());
        }
        assert!(design_url("http://solidcraft.io/x", "https://solidcraft.io").is_err());
        assert!(design_url("http://SOLIDCRAFT.io:80/x", "http://solidcraft.io").is_ok());
    }

    #[test]
    fn only_ctrl_tab_is_left_to_the_browser() {
        // Ctrl+Tab and Ctrl+Shift+Tab (the event's key is "Tab" either way) switch browser tabs.
        assert!(browser_switches_tabs("Tab", true));
        // Tab and Shift+Tab move between the app's fields; Ctrl with other keys stays the app's.
        assert!(!browser_switches_tabs("Tab", false));
        assert!(!browser_switches_tabs("s", true));
    }
}
