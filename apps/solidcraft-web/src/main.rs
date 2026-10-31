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
mod server;
#[cfg(any(target_arch = "wasm32", test))]
mod space_mouse;
#[cfg(target_arch = "wasm32")]
mod webdav;

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

/// Validate design links with the shared URL rules before mapping a same-origin server file.
#[cfg(any(target_arch = "wasm32", test))]
fn server_link_path(value: &str, page_origin: &str) -> Option<String> {
    let url = design_url(value, page_origin).ok()?;
    let page = url::Url::parse(page_origin).ok()?;
    if url.origin() != page.origin() {
        return None;
    }
    // Validate the original path so URL normalization cannot hide forbidden components.
    let path = if value.starts_with('/') { value.to_owned() } else { format!("/{}", value.split_once("://")?.1.split_once('/')?.1) };
    server::from_endpoint(&path)
}

#[cfg(any(target_arch = "wasm32", test))]
fn check_download_size(current: usize, incoming: u64) -> Result<(), String> {
    let cap = solidcraft_engine::io::MAX_DESIGN_BYTES as u64;
    if current as u64 > cap || incoming > cap.saturating_sub(current as u64) {
        return Err(format!("Design exceeds the {} MB size limit", cap / (1024 * 1024)));
    }
    Ok(())
}

/// Ctrl+W (⌘W, without Shift or Alt) while a design is shown closes it in the app, so the
/// browser must not close the tab (#5366). On the start page the browser closes the tab.
#[cfg(any(target_arch = "wasm32", test))]
fn closes_design(key: &str, command: bool, shift: bool, alt: bool, design_shown: bool) -> bool {
    design_shown && command && !shift && !alt && key.eq_ignore_ascii_case("w")
}

/// The settings a tab last wrote to the browser's storage, or loaded from another tab (#8644).
/// Every open tab of the app shares that storage, and eframe saves each tab's whole copy every
/// 30 s and when the page loses focus, so a tab still holding old settings put them back over a
/// change made in another. A change is now written in the frame it is made, and the other tabs
/// load it ([`settings_written`]).
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default, Debug)]
struct PrefsWriter {
    written: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl PrefsWriter {
    /// `prefs` if they are not the settings written or loaded last (they count as written from
    /// now on).
    fn take(&mut self, prefs: String) -> Option<String> {
        if self.written.as_deref() == Some(prefs.as_str()) {
            return None;
        }
        self.written = Some(prefs.clone());
        Some(prefs)
    }

    /// The settings the tab holds after loading another tab's: the tab doesn't write them back.
    fn loaded(&mut self, prefs: String) {
        self.written = Some(prefs);
    }
}

/// The settings another tab wrote, from a `storage` event (the browser sends one to every other
/// tab of the page when one tab writes). Other keys, and settings taken away (clearing the site's
/// data), leave this tab's settings as they are.
#[cfg(any(target_arch = "wasm32", test))]
fn settings_written(key: Option<&str>, value: Option<String>) -> Option<String> {
    value.filter(|_| key == Some(solidcraft_ui_egui::prefs::STORAGE_KEY))
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
        /// A design is shown (not the start page): Ctrl+W closes it, not the browser tab.
        static DESIGN_SHOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Ctrl+W (⌘W) closes the design shown, as on the desktop; on the start page it is left to
    /// the browser, which closes the tab (#5366). Browsers that keep Ctrl+W to themselves still
    /// close the tab, after asking when a design is unsaved ([`ask_before_unload`]).
    fn keep_ctrl_w_for_the_design(window: &web_sys::Window) {
        let on_key = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
            let command = e.ctrl_key() || e.meta_key();
            if crate::closes_design(&e.key(), command, e.shift_key(), e.alt_key(), DESIGN_SHOWN.with(std::cell::Cell::get)) {
                e.prevent_default();
            }
        });
        let _ = window.add_event_listener_with_callback_and_bool("keydown", on_key.as_ref().unchecked_ref(), true);
        // The listener lives as long as the page.
        on_key.forget();
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

    /// Settings another tab wrote, waiting for this tab's next frame (#8644).
    type PrefsInbox = Rc<std::cell::Cell<Option<String>>>;

    /// Settings another tab of the app writes go to `inbox`, and this tab draws a frame, which
    /// loads them.
    fn hear_settings_from_other_tabs(window: &web_sys::Window, ctx: &egui::Context, inbox: PrefsInbox) {
        let ctx = ctx.clone();
        let on_storage = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::StorageEvent)>::new(move |e: web_sys::StorageEvent| {
            if let Some(prefs) = crate::settings_written(e.key().as_deref(), e.new_value()) {
                inbox.set(Some(prefs));
                ctx.request_repaint();
            }
        });
        let _ = window.add_event_listener_with_callback("storage", on_storage.as_ref().unchecked_ref());
        // The listener lives as long as the page.
        on_storage.forget();
    }

    const JAPANESE_FONT_PATH: &str = "./fonts/BIZUDPGothic-Regular.ttf";
    const CHINESE_SC_FONT_PATH: &str = "./fonts/CraftUI-SC.otf";
    const CHINESE_TC_FONT_PATH: &str = "./fonts/CraftUI-TC.otf";

    type FontLoadResult = Rc<std::cell::RefCell<Option<Result<(String, Vec<u8>), String>>>>;

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
        /// On-demand script fonts: fetched only when a CJK language is selected.
        font: solidcraft_ui_egui::fonts::CjkFontLoader,
        font_result: FontLoadResult,
        /// The fetched Japanese font's bytes: kept so a language change can re-order the fallbacks.
        cjk_bytes: Option<Vec<u8>>,
        /// The fetched Chinese font's bytes (Simplified or Traditional for this session).
        chinese_bytes: Option<Vec<u8>>,
        /// Which Chinese face `chinese_bytes` is: `"zh"` or `"zh-TW"`.
        chinese_kind: Option<&'static str>,
        /// Kind requested by the in-flight fetch (`"ja"`, `"zh"`, or `"zh-TW"`).
        font_fetch_kind: Option<&'static str>,
        /// The language the fonts were last set up for (a CJK UI leads with the CJK face).
        font_language: String,
        space_mouse: SpaceMouse,
        wheel_gestures: solidcraft_ui_egui::WebWheelGestures,
        /// The settings this tab wrote or loaded last, and those another tab wrote since (#8644).
        prefs_writer: crate::PrefsWriter,
        other_tab_prefs: PrefsInbox,
    }

    impl Shell {
        /// The settings as they are kept between visits: with the user's own language, not a
        /// session-only English fallback.
        fn persisted_prefs(&mut self) -> String {
            let saved = self.font.persisted_language(&self.app.preferences.language).to_owned();
            let live = std::mem::replace(&mut self.app.preferences.language, saved);
            let prefs = self.app.prefs();
            self.app.preferences.language = live;
            prefs
        }

        /// A settings change goes to the browser's storage in the frame it is made, and from
        /// there to the other open tabs (#8644). Input is what changes settings, so a frame
        /// without any is skipped; eframe's own save still catches anything else.
        fn store_prefs(&mut self, ctx: &egui::Context) {
            if !self.launch.persists() || ctx.input(|i| i.events.is_empty()) {
                return;
            }
            let prefs = self.persisted_prefs();
            let Some(prefs) = self.prefs_writer.take(prefs) else { return };
            let stored =
                web_sys::window().and_then(|w| w.local_storage().ok().flatten()).map(|s| s.set_item(solidcraft_ui_egui::prefs::STORAGE_KEY, &prefs));
            if !matches!(stored, Some(Ok(()))) {
                log::warn!("settings: the browser's storage did not take them; they are saved again later");
                self.prefs_writer = crate::PrefsWriter::default();
            }
        }

        /// Settings another tab wrote replace this tab's, so this tab's own saves keep them
        /// instead of putting its older copy back (#8644).
        fn load_other_tabs_prefs(&mut self) {
            let Some(prefs) = self.other_tab_prefs.take() else { return };
            self.app.load_prefs(&prefs);
            let held = self.persisted_prefs();
            self.prefs_writer.loaded(held);
        }

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
            // A write to the computer's disk that failed: the design is not saved after all.
            for path in self.files.unsaved.borrow_mut().drain(..) {
                if self.app.session.path.as_deref() == Some(path.as_str()) {
                    self.app.session.mark_unsaved();
                }
            }
            crate::server::conflict_prompt(&self.files, &mut self.app);
            if self.app.file_picker_busy() {
                ctx.request_repaint();
                return;
            }
            files::flush(&self.files);
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
                if intent == Intent::Theme {
                    solidcraft_ui_egui::prefs::import_theme_bytes(&mut self.app, bytes);
                    continue;
                }
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
                    Intent::Insert | Intent::Theme => self.app.drop_path(&path),
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
            recent.retain(|p| crate::server::parse(p).is_some() || vfs::exists(p) || disk::parse(p).is_some_and(|(id, _)| known.contains(id)));
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
            let _language = solidcraft_ui_egui::i18n::scope(&self.app.preferences.language);
            crate::server::show(&self.files, ctx);
            if !self.files.panel.get() {
                return;
            }
            let mut act: Option<(&'static str, String)> = None;
            let closed = solidcraft_ui_egui::ui_kit::Window::new(egui::Id::new("sc_browser_files"), "folder", "Files in this browser")
                // Room for long names; the kit still caps the window at the browser's width.
                .cards(4.0)
                .show_body(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if self.files.server.enabled.get()
                            && ui_kit::button(ui, solidcraft_ui_egui::i18n::tr("web.server.open_folder"), ui_kit::Button::Normal).clicked()
                        {
                            let future = crate::server::pick(&self.files, None);
                            let files = self.files.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Some(path) = future.await {
                                    files.opened.borrow_mut().push(path);
                                    files.wake();
                                }
                            });
                            self.files.panel.set(false);
                        }
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
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        // The name takes the width the size and the actions leave (#8097).
                        let empty = "Designs you save appear here. They stay in this browser.";
                        ui_kit::name_list(ui, "opfs_files", &["Name", "Size", "Actions"], empty, |list| {
                            for e in &entries {
                                list.row(&e.name, |row| {
                                    match &mut self.renaming {
                                        Some((from, to)) if *from == e.name => row.cell(|ui| {
                                            let r =
                                                ui_kit::cell_editor(ui, ui.make_persistent_id(("stored_file_name", from.as_str())), to, "", false);
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
                                        }),
                                        _ => {
                                            row.name(&e.name);
                                        }
                                    }
                                    row.cell(|ui| ui.label(format!("{:.1} KB", e.size as f64 / 1024.0)));
                                    row.cell(|ui| {
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
                                });
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

        /// Whether the shell already has the on-demand face the live language needs.
        fn script_font_ready(&self) -> bool {
            match solidcraft_ui_egui::fonts::web_script_font_kind(&self.app.preferences.language) {
                Some("zh") => self.chinese_bytes.is_some() && self.chinese_kind == Some("zh"),
                Some("zh-TW") => self.chinese_bytes.is_some() && self.chinese_kind == Some("zh-TW"),
                Some("ja") => self.cjk_bytes.is_some(),
                _ => true,
            }
        }

        /// Fetch and install a script font when a CJK language is selected. If that fails, this
        /// session falls back to English (no tofu); the saved language preference is kept.
        fn tick_font(&mut self, ctx: &egui::Context) {
            let done = self.font_result.borrow_mut().take();
            if let Some(res) = done {
                let kind = self.font_fetch_kind.take();
                let installed = match res {
                    Ok((path, bytes)) => {
                        match kind {
                            Some("zh") => {
                                self.chinese_bytes = Some(bytes);
                                self.chinese_kind = Some("zh");
                            }
                            Some("zh-TW") => {
                                self.chinese_bytes = Some(bytes);
                                self.chinese_kind = Some("zh-TW");
                            }
                            _ => {
                                self.cjk_bytes = Some(bytes);
                            }
                        }
                        log::info!("loaded web script font from {path}");
                        true
                    }
                    Err(err) => {
                        let path = match kind {
                            Some("zh") => CHINESE_SC_FONT_PATH,
                            Some("zh-TW") => CHINESE_TC_FONT_PATH,
                            _ => JAPANESE_FONT_PATH,
                        };
                        log::warn!("failed to fetch script font from {path}: {err}");
                        false
                    }
                };
                if !installed && solidcraft_ui_egui::fonts::needs_cjk_font(&self.app.preferences.language) {
                    log::warn!("script font unavailable; showing English for this session");
                }
                self.font.finished(installed, &mut self.app.preferences.language);
                self.apply_fonts(ctx);
                ctx.request_repaint();
            }
            let ready = self.script_font_ready();
            if self.font.tick(&mut self.app.preferences.language, ready) {
                self.fetch_font(ctx, solidcraft_ui_egui::fonts::web_script_font_kind(&self.app.preferences.language).unwrap_or("ja"));
            }
            // The CJK face leads only a CJK UI: its Cyrillic glyphs are full-width, so switching
            // to Ukrainian (or English) must put the Latin face back in front.
            if self.app.preferences.language != self.font_language {
                self.apply_fonts(ctx);
            }
        }

        /// Set up the UI fonts for the live language, keeping fetched script fonts as fallbacks
        /// (Chinese leads a Chinese UI; see [`solidcraft_ui_egui::fonts::install_web_script_fonts`]).
        fn apply_fonts(&mut self, ctx: &egui::Context) {
            if self.cjk_bytes.is_none() && self.chinese_bytes.is_none() {
                solidcraft_ui_egui::fonts::setup_fonts(ctx);
            } else {
                solidcraft_ui_egui::fonts::install_web_script_fonts(
                    ctx,
                    self.cjk_bytes.clone(),
                    self.chinese_bytes.clone(),
                    &self.app.preferences.language,
                );
            }
            self.font_language.clone_from(&self.app.preferences.language);
        }

        /// CJK text on screen with no Japanese font yet (an English UI showing a Japanese name):
        /// fetch the Japanese font, so the text does not stay tofu (#720).
        fn watch_text_for_font(&mut self, ctx: &egui::Context) {
            if self.font.watching_text() && solidcraft_ui_egui::fonts::painted_cjk(ctx) && self.font.text_needs_font() {
                self.fetch_font(ctx, "ja");
            }
        }

        fn fetch_font(&mut self, ctx: &egui::Context, kind: &'static str) {
            let path = match kind {
                "zh" => CHINESE_SC_FONT_PATH,
                "zh-TW" => CHINESE_TC_FONT_PATH,
                _ => JAPANESE_FONT_PATH,
            };
            self.font_fetch_kind = Some(kind);
            let slot = self.font_result.clone();
            let ctx = ctx.clone();
            let path = path.to_owned();
            wasm_bindgen_futures::spawn_local(async move {
                let res = fetch_font_bytes(&path).await.map(|bytes| (path, bytes));
                *slot.borrow_mut() = Some(res);
                ctx.request_repaint();
            });
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // First: a language another tab chose fetches its font this frame.
            self.load_other_tabs_prefs();
            self.tick_font(ctx);
            self.app.logic(ctx);
            self.api.poll();
            self.space_mouse.logic(&mut self.app, ctx);
            self.files_tick(ctx);
            UNSAVED.with(|u| u.set(solidcraft_ui_egui::documents::any_dirty(&self.app)));
            DESIGN_SHOWN.with(|d| d.set(solidcraft_ui_egui::toolbar::design_shown(&self.app)));
        }
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
            let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
            if let Some(delay) = self.wheel_gestures.update(raw, now) {
                ctx.request_repaint_after(delay);
            }
            if self.launch.persists() {
                let theme = solidcraft_ui_egui::prefs::theme_drop_target(&self.app, ctx);
                files::take_drops(&self.files, raw, theme);
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
            self.store_prefs(&ctx);
        }
        fn persist_egui_memory(&self) -> bool {
            self.launch.persists()
        }
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            if !self.launch.persists() {
                return;
            }
            let prefs = self.persisted_prefs();
            storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, prefs);
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
            keep_ctrl_w_for_the_design(&window);
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
                            crate::server::probe(&files);
                        }
                        let (panel, opener, saver, shown) = (files.clone(), files.clone(), files.clone(), files.clone());
                        let (restorable, restorer) = (files.clone(), files.clone());
                        let (persisted, conflict) = (files.clone(), files.clone());
                        let services = if launch.embed { Services::default() } else { Services {
                            // Open shows the browser's open dialog for a design where it has one
                            // (the file keeps its handle, so Save writes back to it), else the
                            // browser's files window. With the server folder enabled, designs
                            // use its picker. Other file types use a matching upload dialog.
                            pick_open: Some(Box::new(move |exts: &[&str]| match disk::open_route(disk::supported(), opener.server.enabled.get(), exts) {
                                disk::OpenRoute::Server => crate::server::pick(&opener, None),
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
                                if design && saver.server.enabled.get() { return crate::server::pick(&saver, Some(name)); }
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
                                (restorable.server.enabled.get() && crate::server::parse(path).is_some()) || disk::parse(path).is_some_and(|(id, _)| restorable.disk_known.borrow().contains(id))
                            })),
                            restore_file: Some(Box::new(move |path: &str| {
                                if crate::server::parse(path).is_some() { crate::server::restore(&restorer, path); } else { disk::restore(&restorer, path); }
                            })),
                            webdav_http: Some(crate::webdav::service()),
                            persist_save: Some(Box::new(move |path| crate::server::persist(&persisted, path))),
                            save_conflict_answer: Some(Box::new(move |answer| crate::server::answer_conflict(&conflict, answer))),
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
                        // Listening before reading: settings another tab writes from now on are
                        // newer than those read here.
                        let other_tab_prefs = PrefsInbox::default();
                        if launch.persists() {
                            hear_settings_from_other_tabs(&window, &cc.egui_ctx, other_tab_prefs.clone());
                        }
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
                        let linked = crate::linked::Download::start(launch.open.as_deref(), &cc.egui_ctx, &files);
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
                            chinese_bytes: None,
                            chinese_kind: None,
                            font_fetch_kind: None,
                            font_language,
                            space_mouse: SpaceMouse::default(),
                            wheel_gestures: solidcraft_ui_egui::WebWheelGestures::default(),
                            prefs_writer: crate::PrefsWriter::default(),
                            other_tab_prefs,
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
    fn server_paths_are_bounded_and_keep_their_storage_identity() {
        for api in [false, true] {
            assert_eq!(disk::open_route(api, false, &["solidcraft"]), if api { disk::OpenRoute::Disk } else { disk::OpenRoute::Panel });
            assert_eq!(disk::open_route(api, true, &["solidcraft"]), disk::OpenRoute::Server);
            assert_eq!(disk::open_route(api, true, &[]), disk::OpenRoute::Server);
            for server in [false, true] {
                assert_eq!(disk::open_route(api, server, &["dxf"]), disk::OpenRoute::Upload);
                assert_eq!(disk::open_route(api, server, &["png", "jpg", "jpeg"]), disk::OpenRoute::Upload);
            }
        }
        let path = "Converted/part 1.solidcraft";
        let engine = server::engine_path(path).unwrap();
        assert_eq!(disk::route(&engine), disk::WriteTo::Server);
        assert_eq!(server::parse(&engine), Some(path));
        let endpoint = server::endpoint(path).unwrap();
        assert_eq!(endpoint, "/designs/Converted/part%201.solidcraft");
        assert_eq!(server::from_endpoint(&endpoint).as_deref(), Some(path));
        assert_eq!(server_link_path("https://solidcraft.io/designs/a.solidcraft", "https://solidcraft.io").as_deref(), Some("a.solidcraft"));
        assert!(server_link_path("https://other.solidcraft.io/designs/a.solidcraft", "https://solidcraft.io").is_none());
        assert!(server_link_path("https://solidcraft.io@other.solidcraft.io/designs/a.solidcraft", "https://solidcraft.io").is_none());
        assert_eq!(server_link_path("http://SOLIDCRAFT.io:80/designs/a.solidcraft", "http://solidcraft.io").as_deref(), Some("a.solidcraft"));
        assert!(server_link_path("/designs/%2e%2e/a.solidcraft", "http://solidcraft.io").is_none());
        assert!(server_link_path("/designs/./a.solidcraft", "http://solidcraft.io").is_none());
        let launch = Launch::parse("?open=%2Fdesigns%2FConverted%2Fpart%25201.solidcraft");
        assert_eq!(server::from_endpoint(launch.open.as_deref().unwrap()).as_deref(), Some(path));
        for bad in ["../x", "/x", "a/../x", "a\\x", "a//x", "a/%2e%2e/x", "a?x", "a#x", "a:x", ".hidden", "a/.", "a/", "a/\0x"] {
            assert!(!server::valid(bad), "{bad:?}");
            assert!(server::engine_path(bad).is_none());
        }
        for bad in ["/designs/%2e%2e/x", "/designs/a%2Fx", "/designs/%252e%252e/x", "/designs/a%5Cx", "//designs/x", "/designs/a?x"] {
            assert!(server::from_endpoint(bad).is_none(), "{bad}");
        }
        assert!(!server::valid(&"x".repeat(256)));
        assert!(!server::valid(&["a"; 33].join("/")));
        assert_eq!(disk::route("/opfs/part.solidcraft"), disk::WriteTo::Opfs);
        assert_eq!(disk::route("/disk/id/part.solidcraft"), disk::WriteTo::Disk);
        assert_eq!(disk::route("/downloads/part.step"), disk::WriteTo::Download);
        assert!(design_url("/elsewhere/part.solidcraft", "http://solidcraft.io").is_ok());
    }

    #[test]
    fn server_saves_require_the_opened_version_or_an_explicit_overwrite() {
        assert_eq!(server::save_condition(None, false), Some(("If-None-Match", "*")));
        assert_eq!(server::save_condition(Some("\"first\""), false), Some(("If-Match", "\"first\"")));
        assert_eq!(server::save_condition(Some("\"first\""), true), None);
        assert_eq!(server::save_condition(None, true), None);
        assert!(server::save_conflict(412, true));
        assert!(!server::save_conflict(412, false), "programmatic saves never open a prompt");
        for status in [200, 201, 204, 401, 403, 404, 409, 413, 500] {
            assert!(!server::save_conflict(status, true));
        }
    }

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

    #[test]
    fn ctrl_w_closes_the_design_not_the_tab() {
        // Ctrl+W and ⌘W (either case of the key) close the design shown.
        assert!(closes_design("w", true, false, false, true));
        assert!(closes_design("W", true, false, false, true));
        // On the start page the browser closes the tab.
        assert!(!closes_design("w", true, false, false, false));
        // Ctrl+Shift+W (close the window), plain W and other keys stay the browser's or the app's.
        assert!(!closes_design("W", true, true, false, true));
        assert!(!closes_design("w", true, false, true, true));
        assert!(!closes_design("w", false, false, false, true));
        assert!(!closes_design("s", true, false, false, true));
    }

    /// A tab writes its settings when they change, and not again while they stay the same; the
    /// settings it loaded from another tab aren't written back (#8644).
    #[test]
    fn changed_settings_are_written_once_and_loaded_ones_not_at_all() {
        let mut writer = PrefsWriter::default();
        assert_eq!(writer.take(r#"{"a":1}"#.into()).as_deref(), Some(r#"{"a":1}"#), "the first settings are written");
        assert_eq!(writer.take(r#"{"a":1}"#.into()), None, "the same settings are not written again");
        assert_eq!(writer.take(r#"{"a":2}"#.into()).as_deref(), Some(r#"{"a":2}"#), "a change is written at once");
        writer.loaded(r#"{"a":3}"#.into());
        assert_eq!(writer.take(r#"{"a":3}"#.into()), None, "settings from another tab are not written back");
        assert_eq!(writer.take(r#"{"a":4}"#.into()).as_deref(), Some(r#"{"a":4}"#), "this tab's next change is written");
    }

    /// Only the settings another tab wrote are loaded: not other keys, and not settings taken
    /// away by clearing the site's data (#8644).
    #[test]
    fn only_settings_another_tab_wrote_are_loaded() {
        let key = solidcraft_ui_egui::prefs::STORAGE_KEY;
        assert_eq!(settings_written(Some(key), Some("{}".into())).as_deref(), Some("{}"));
        assert_eq!(settings_written(Some("solidcraft.webgl"), Some("1".into())), None);
        assert_eq!(settings_written(Some(key), None), None, "removed settings leave this tab's");
        assert_eq!(settings_written(None, None), None, "clearing all storage leaves this tab's settings");
    }
}
