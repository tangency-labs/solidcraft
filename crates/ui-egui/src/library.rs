//! Folder-backed document navigation. Browsing never rewrites design contents;
//! selecting a file uses the existing document loader and its unsaved-document handling.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit::{Container, Edge, Surface};

#[path = "library_actions.rs"]
mod actions;

#[cfg(not(target_arch = "wasm32"))]
const MAX_ENTRIES: usize = 10_000;

const MAX_RECENT_FOLDERS: usize = 10;

const MAX_HIGHLIGHTS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Highlight {
    Red,
    Orange,
    Green,
    Blue,
    Purple,
}

impl Highlight {
    const ALL: [Self; 5] = [Self::Red, Self::Orange, Self::Green, Self::Blue, Self::Purple];

    fn label(self) -> &'static str {
        match self {
            Self::Red => "library.highlight_red",
            Self::Orange => "library.highlight_orange",
            Self::Green => "library.highlight_green",
            Self::Blue => "library.highlight_blue",
            Self::Purple => "library.highlight_purple",
        }
    }

    fn color(self) -> egui::Color32 {
        let t = crate::theme::Tokens::get();
        match self {
            Self::Red => t.library_highlights[0],
            Self::Orange => t.library_highlights[1],
            Self::Green => t.library_highlights[2],
            Self::Blue => t.library_highlights[3],
            Self::Purple => t.library_highlights[4],
        }
    }
}

#[derive(Default)]
struct CardStatus {
    active: bool,
    highlight: Option<Highlight>,
}

#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    name: String,
    folder: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileStamp {
    pub(super) bytes: u64,
    pub(super) modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    pub(super) changed: (i64, i64),
}

#[derive(Default)]
struct Listing {
    entries: Vec<Entry>,
    warning: Option<String>,
    identities: BTreeMap<PathBuf, FileStamp>,
}

#[derive(Default)]
pub struct LibraryState {
    root: Option<PathBuf>,
    recent_roots: Vec<PathBuf>,
    pinned_roots: Vec<PathBuf>,
    highlights: BTreeMap<PathBuf, Highlight>,
    current: PathBuf,
    collapsed: bool,
    search: String,
    focused: Option<bool>,
    refresh_on_return: bool,
    listing: Listing,
    filtered: Vec<usize>,
    filter_key: Option<(String, usize)>,
    listing_changed: bool,
    error: Option<String>,
    action_dialog: Option<actions::Dialog>,
    folders_expanded: bool,
    folder_menu: Option<(PathBuf, egui::Response, egui::Pos2)>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<std::sync::mpsc::Receiver<Result<Listing, String>>>,
}

impl LibraryState {
    pub fn prefs(&self) -> Value {
        json!({"root": self.root, "collapsed": self.collapsed, "recent_roots": self.recent_roots, "pinned_roots": self.pinned_roots, "highlights": self.highlights})
    }

    pub fn load(&mut self, value: Option<&Value>) {
        let Some(value) = value else { return };
        self.highlights.clear();
        if let Some(highlights) = value.get("highlights").and_then(Value::as_object) {
            for (path, color) in highlights.iter().take(MAX_HIGHLIGHTS) {
                if !path.is_empty()
                    && path.len() <= 32_768
                    && let Ok(color) = serde_json::from_value::<Highlight>(color.clone())
                {
                    self.highlights.insert(PathBuf::from(path), color);
                }
            }
        }
        self.recent_roots.clear();
        self.pinned_roots.clear();
        if let Some(recent) = value.get("recent_roots").and_then(Value::as_array) {
            for path in recent.iter().take(MAX_RECENT_FOLDERS * 2).rev().filter_map(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 32_768) {
                let path = PathBuf::from(path);
                if !self.recent_roots.contains(&path) {
                    self.recent_roots.insert(0, path);
                }
            }
        }
        if let Some(pinned) = value.get("pinned_roots").and_then(Value::as_array) {
            for path in pinned.iter().take(MAX_RECENT_FOLDERS).filter_map(Value::as_str).map(PathBuf::from) {
                if self.recent_roots.contains(&path) && !self.pinned_roots.contains(&path) {
                    self.pinned_roots.push(path);
                }
            }
        }
        self.trim_history();
        let collapsed = value.get("collapsed").and_then(Value::as_bool).unwrap_or(false);
        if let Some(root) = value.get("root").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 32_768) {
            let root = PathBuf::from(root);
            if !value.get("recent_roots").is_some_and(Value::is_array) {
                self.remember_root(root.clone());
            }
            self.root = Some(root.clone());
            self.navigate(root);
        }
        self.collapsed = collapsed;
    }

    fn set_highlight(&mut self, path: &Path, color: Option<Highlight>) {
        if let Some(color) = color {
            if path.to_str().is_none() {
                self.error = Some(crate::i18n::tr("library.utf8").into());
                return;
            }
            if self.highlights.contains_key(path) || self.highlights.len() < MAX_HIGHLIGHTS {
                self.highlights.insert(path.to_path_buf(), color);
            } else {
                self.error = Some(crate::i18n::tr("library.highlight_limit").to_string());
            }
        } else {
            self.highlights.remove(path);
        }
    }

    fn remember_root(&mut self, root: PathBuf) {
        self.recent_roots.retain(|path| path != &root);
        self.recent_roots.insert(0, root);
        self.trim_history();
    }

    fn trim_history(&mut self) {
        let mut unpinned = 0;
        self.recent_roots.retain(|path| {
            if self.pinned_roots.contains(path) {
                true
            } else {
                unpinned += 1;
                unpinned <= MAX_RECENT_FOLDERS
            }
        });
    }

    fn recent_folders(&self) -> Vec<PathBuf> {
        self.pinned_roots.iter().chain(self.recent_roots.iter().filter(|p| !self.pinned_roots.contains(p))).cloned().collect()
    }

    fn remove_recent(&mut self, path: &Path) {
        let position = self.recent_folders().iter().position(|p| p == path).unwrap_or(0);
        self.recent_roots.retain(|p| p != path);
        self.pinned_roots.retain(|p| p != path);
        let remaining = self.recent_folders();
        if let [only] = remaining.as_slice() {
            self.set_root(only.clone());
        } else if self.root.as_deref() == Some(path) {
            let next = remaining.get(position).or_else(|| remaining.first()).cloned();
            self.root = next.clone();
            self.navigate(next.unwrap_or_default());
        }
    }

    fn set_root(&mut self, root: PathBuf) {
        self.remember_root(root.clone());
        self.root = Some(root.clone());
        self.collapsed = false;
        self.folders_expanded = false;
        self.folder_menu = None;
        self.navigate(root);
    }

    fn navigate(&mut self, path: PathBuf) {
        self.current = path;
        self.search.clear();
        self.refresh();
    }

    // Record focus transitions, not frames. Defer while hidden or already scanning.
    fn focus(&mut self, focused: bool) {
        if self.focused == Some(false) && focused && self.root.is_some() {
            self.refresh_on_return = true;
        }
        self.focused = Some(focused);
    }

    pub(super) fn refresh(&mut self) {
        self.refresh_on_return = false;
        self.listing = Listing::default();
        self.filter_key = None;
        self.error = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Dropping the old receiver prevents a slow previous folder from replacing
            // the latest selection. The worker owns only paths and a bounded listing.
            self.pending = None;
            let Some(root) = self.root.clone() else { return };
            let current = self.current.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let language = crate::i18n::language();
            match std::thread::Builder::new().name("document-library".into()).spawn(move || {
                let _language = crate::i18n::scope(language);
                let _ = tx.send(read_folder(&root, &current));
            }) {
                Ok(_) => self.pending = Some(rx),
                Err(error) => self.error = Some(crate::i18n::format("library.scan_failed", &[("error", &error.to_string())])),
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(listing) => {
                            self.listing = listing;
                            self.filter_key = None;
                            self.listing_changed = true;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(50)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = Some(crate::i18n::tr("library.scan_stopped").into());
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = ctx;
        if self.refresh_on_return && !self.collapsed && !self.loading() {
            self.refresh();
        }
    }

    fn loading(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    fn update_filter(&mut self) -> bool {
        if self.filter_key.as_ref().is_some_and(|(query, len)| query == &self.search && *len == self.listing.entries.len()) {
            return false;
        }
        let query = self.search.to_lowercase();
        self.filtered.clear();
        self.filtered.extend(
            self.listing
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| query.is_empty() || entry.name.to_lowercase().contains(&query))
                .map(|(index, _)| index),
        );
        self.filter_key = Some((self.search.clone(), self.listing.entries.len()));
        true
    }

    #[cfg(test)]
    fn visible(&self) -> Vec<&Entry> {
        let query = self.search.to_lowercase();
        self.listing.entries.iter().filter(|entry| query.is_empty() || entry.name.to_lowercase().contains(&query)).collect()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_folder(root: &Path, current: &Path) -> Result<Listing, String> {
    let canonical_root = root.canonicalize().map_err(|error| crate::i18n::format("library.root_unavailable", &[("error", &error.to_string())]))?;
    let canonical_current =
        current.canonicalize().map_err(|error| crate::i18n::format("library.folder_unavailable", &[("error", &error.to_string())]))?;
    if !canonical_current.starts_with(&canonical_root) {
        return Err(crate::i18n::tr("library.outside").into());
    }
    let contents = std::fs::read_dir(current).map_err(|error| crate::i18n::format("library.read_failed", &[("error", &error.to_string())]))?;
    let mut result = Listing::default();
    let mut entries = Vec::new();
    for (index, entry) in contents.enumerate() {
        if index >= MAX_ENTRIES {
            result.warning = Some(crate::i18n::tr("library.limit").into());
            break;
        }
        let Ok(entry) = entry else {
            result.warning = Some(crate::i18n::tr("library.partial_permissions").into());
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            result.warning = Some(crate::i18n::tr("library.partial").into());
            continue;
        };
        // Do not follow symlinks or list special files. Only enumerate this directory;
        // opening a project explicitly requests its contents.
        let path = entry.path();
        let design = path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|ext| solidcraft_engine::io::is_design_ext(ext) || ext.eq_ignore_ascii_case("json"));
        if kind.is_dir() || (kind.is_file() && design) {
            // Read timestamps once in the background scan, never while painting cards.
            // Creation time puts externally copied designs first even when a copy preserves mtime.
            let newest = if kind.is_file() {
                entry.metadata().ok().and_then(|metadata| {
                    result.identities.insert(
                        path.clone(),
                        FileStamp {
                            bytes: metadata.len(),
                            modified: metadata.modified().ok(),
                            #[cfg(unix)]
                            changed: {
                                use std::os::unix::fs::MetadataExt;
                                (metadata.ctime(), metadata.ctime_nsec())
                            },
                        },
                    );
                    let times = metadata.created().ok().into_iter().chain(metadata.modified().ok());
                    // Finder preserves birth time as well as mtime when duplicating a file.
                    // Unix change time still records when the new copy arrived.
                    #[cfg(unix)]
                    let times = times.chain({
                        use std::os::unix::fs::MetadataExt;
                        u64::try_from(metadata.ctime())
                            .ok()
                            .zip(u32::try_from(metadata.ctime_nsec()).ok())
                            .and_then(|(seconds, nanos)| std::time::SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::new(seconds, nanos)))
                    });
                    times.max()
                })
            } else {
                None
            };
            entries.push((Entry { path, name: entry.file_name().to_string_lossy().into_owned(), folder: kind.is_dir() }, newest));
        }
    }
    entries.sort_by(|(a, a_time), (b, b_time)| {
        b.folder
            .cmp(&a.folder)
            .then_with(|| b_time.cmp(a_time))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    result.entries = entries.into_iter().map(|(entry, _)| entry).collect();
    Ok(result)
}

fn available(app: &SolveApp) -> bool {
    !cfg!(target_arch = "wasm32") && app.services.pick_folder.is_some()
}

fn choose_folder(app: &mut SolveApp) {
    app.pick_folder(|app, path| {
        if let Some(path) = path {
            app.library.set_root(PathBuf::from(path));
        }
    });
}

fn go_home(app: &mut SolveApp) {
    if let Some(root) = app.library.root.clone() {
        app.library.navigate(root);
    }
}

fn open_entry(app: &mut SolveApp, entry: Entry) {
    if entry.folder {
        app.library.navigate(entry.path);
    } else if let Some(path) = entry.path.to_str() {
        crate::home::open_recent(app, path);
    } else {
        app.set_status(crate::i18n::tr("library.open_utf8"), true);
    }
}

fn messages(state: &LibraryState, ui: &mut egui::Ui) {
    if state.loading() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(crate::i18n::tr("library.reading"));
        });
    } else if let Some(error) = &state.error {
        ui.label(error);
    } else if state.filtered.is_empty() {
        ui.label(if state.search.is_empty() { crate::i18n::tr("library.empty") } else { crate::i18n::tr("library.no_matches") });
    }
    if let Some(warning) = &state.listing.warning {
        ui.label(warning);
    }
}

/// Shared card layout for real documents and the built-in examples.
fn thumbnail_card(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    name: &str,
    texture: Option<egui::TextureHandle>,
    icon: &str,
    status: CardStatus,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    response.widget_info(|| {
        if status.active {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, true, name)
        } else {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name)
        }
    });
    let t = crate::theme::Tokens::get();
    ui.painter().rect(
        rect,
        4.0,
        if response.hovered() || response.has_focus() { t.hover } else { t.field },
        egui::Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    let image = egui::Rect::from_min_max(rect.min + egui::vec2(4.0, 4.0), egui::pos2(rect.right() - 4.0, rect.bottom() - 30.0));
    if let Some(texture) = texture {
        crate::home::paint_document_thumbnail(ui.painter(), &texture, image, false);
    } else {
        crate::icons::paint_on(ui.painter(), egui::Rect::from_center_size(image.center(), egui::vec2(36.0, 36.0)), icon, None, 1.0);
    }
    if status.active {
        let center = egui::pos2(image.left() + 10.0, image.top() + 10.0);
        ui.painter().circle_filled(center, 5.0, t.field);
        ui.painter().circle_filled(center, 4.0, t.accent);
        crate::scenario::publish_rect("library:active_indicator", egui::Rect::from_center_size(center, egui::vec2(10.0, 10.0)));
    }
    let mut label = egui::Rect::from_min_max(egui::pos2(rect.left() + 6.0, rect.bottom() - 25.0), rect.max - egui::vec2(6.0, 3.0));
    if let Some(highlight) = status.highlight {
        let marker = egui::Rect::from_min_size(label.min + egui::vec2(0.0, 1.0), egui::vec2(4.0, 16.0));
        ui.painter().rect_filled(marker, 1.0, highlight.color());
        crate::scenario::publish_rect("library:highlight_indicator", marker);
        label.min.x += 10.0;
    }
    ui.painter().with_clip_rect(label).text(label.left_top(), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(t.font_body), t.text);
    response
}

/// Reuse Home's background jobs and cached sample sessions; never create files for examples.
fn sample_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    let samples = solidcraft_engine::sample::samples();
    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
    let height = width * 0.75 + 32.0;
    let mut chosen = None;
    egui::ScrollArea::vertical().id_salt("library_samples").show_rows(ui, height, samples.len().div_ceil(2), |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                for column in 0..2 {
                    let index = row * 2 + column;
                    let Some(sample) = samples.get(index) else { continue };
                    let texture = crate::home::document_thumbnail(app, ui.ctx(), &format!("sample:{index}"));
                    let response = thumbnail_card(ui, width, height, app.tr_label(sample.name), texture, "box", CardStatus::default());
                    crate::scenario::publish_handle(&format!("library:sample:{index}"), response.rect.center());
                    if response.on_hover_text(app.tr_label(sample.about)).clicked() {
                        chosen = Some(index);
                    }
                }
            });
        }
    });
    if let Some(index) = chosen {
        crate::home::open_sample(app, index);
    }
}

fn rows(app: &mut SolveApp, ui: &mut egui::Ui) -> Option<Entry> {
    let mut chosen = None;
    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - gap) / 2.0;
    let height = width * 0.75 + 32.0;
    egui::ScrollArea::vertical().id_salt("library_list").show_rows(ui, height, app.library.filtered.len().div_ceil(2), |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                for column in 0..2 {
                    let index = row * 2 + column;
                    let Some(entry) = app.library.filtered.get(index).and_then(|i| app.library.listing.entries.get(*i)).cloned() else { continue };
                    let texture =
                        if entry.folder { None } else { entry.path.to_str().and_then(|path| crate::home::document_thumbnail(app, ui.ctx(), path)) };
                    let active = !entry.folder && app.session.path.as_deref().map(Path::new) == Some(entry.path.as_path());
                    let highlight = if entry.folder { None } else { app.library.highlights.get(&entry.path).copied() };
                    let response = thumbnail_card(
                        ui,
                        width,
                        height,
                        &entry.name,
                        texture,
                        if entry.folder { "folder" } else { "new" },
                        CardStatus { active, highlight },
                    );
                    crate::scenario::publish_handle(&format!("library:entry:{index}"), response.rect.center());
                    actions::context_menu(app, ui, &response, &entry);
                    let tooltip =
                        if active { format!("{}\n{}", entry.path.display(), app.tr("home.open_now")) } else { entry.path.display().to_string() };
                    let tooltip = if let Some(highlight) = highlight {
                        format!("{tooltip}\n{}: {}", app.tr("library.highlight"), app.tr(highlight.label()))
                    } else {
                        tooltip
                    };
                    if response.on_hover_text(tooltip).clicked() {
                        chosen = Some(entry);
                    }
                }
            });
        }
    });
    chosen
}

/// Keep import shortcuts fixed below the scrolling cards, level with the timeline.
fn insert_footer(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let container = Container::new(ui.ctx(), Surface::Panel);
    let gap = container.gap();
    container.edge("sc_library_insert", Edge::Bottom).exact_size(40.0 + gap * 2.0).show_separator_line(false).frame(egui::Frame::NONE).show(
        ui,
        |ui| {
            let card =
                if container.floating() { crate::timeline::floating_rect(ui.max_rect()).shrink2(egui::vec2(t.space_4, 0.0)) } else { ui.max_rect() };
            ui.painter().add(container.frame(ui.ctx(), t.timeline, crate::frame::Side::Left).paint(card));
            crate::scenario::publish_rect("library:import_card", card);
            let mut controls = ui.new_child(egui::UiBuilder::new().id_salt("library_import_card").max_rect(card));
            controls.set_clip_rect(card);
            let ui = &mut controls;
            let enabled = app.services.pick_open.is_some() && !app.file_picker_busy() && !app.file_load.busy();
            let [step, iges, mesh] = ui
                .add_enabled_ui(enabled, |ui| {
                    crate::ui_kit::button_row_in_card(
                        ui,
                        "library",
                        [
                            crate::ui_kit::PairButton { key: "insert_step", label: "STEP", icon: Some("insert_step") },
                            crate::ui_kit::PairButton { key: "insert_iges", label: "IGES", icon: Some("insert_step") },
                            crate::ui_kit::PairButton { key: "insert_mesh", label: "Mesh", icon: Some("insert_mesh") },
                        ],
                    )
                })
                .inner;
            for (clicked, command, extensions) in [
                (step, "file.insert_step", &["step", "stp"][..]),
                (iges, "file.insert_step", &["igs", "iges"][..]),
                (mesh, "file.insert_mesh", &["3mf", "stl", "obj", "glb", "gltf"][..]),
            ] {
                if clicked && solidcraft_engine::find_command(command).is_some_and(|spec| spec.info(&app.session).enabled) {
                    app.home.open = false;
                    app.pick_open_for(extensions, |app, path| {
                        if let Some(path) = path {
                            app.insert_path_async(&path);
                        }
                    });
                }
            }
        },
    );
}

fn contents(app: &mut SolveApp, ui: &mut egui::Ui) -> Option<Entry> {
    ui.horizontal(|ui| {
        let (rect, home) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
        if home.hovered() || home.has_focus() {
            ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        crate::icons::paint_ink(ui.painter(), rect.shrink(2.0), "home", crate::theme::Tokens::get().text);
        ui.painter().rect_stroke(
            rect,
            crate::theme::Tokens::get().control_radius,
            egui::Stroke::new(1.0, crate::theme::Tokens::get().border),
            egui::StrokeKind::Inside,
        );
        home.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::i18n::tr("library.home")));
        crate::scenario::publish_handle("library:home", home.rect.center());
        if home.on_hover_text(crate::i18n::tr("library.home_hint")).clicked() {
            go_home(app);
        }
        let (chevron, _) = ui.allocate_exact_size(egui::vec2(12.0, 24.0), egui::Sense::hover());
        crate::ui_kit::disclosure(ui.painter(), chevron.center(), app.library.folders_expanded, crate::theme::Tokens::get().text_dim);
        let name = app
            .library
            .root
            .as_deref()
            .map(|root| root.file_name().unwrap_or(root.as_os_str()).to_string_lossy().into_owned())
            .unwrap_or_else(|| app.tr("home.samples").to_string());
        let size = egui::vec2((ui.available_width() - 32.0).max(1.0), 24.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let mut label_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        label_ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let picker = crate::ui_kit::list_picker(&mut label_ui, "library_folder_section", &name, app.library.folders_expanded);
        picker.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::CollapsingHeader,
                true,
                app.library.folders_expanded,
                crate::i18n::tr("library.recent_folders"),
            )
        });
        crate::scenario::publish_target("library:folders", picker.rect);
        if picker.on_hover_text(crate::i18n::tr("library.recent_folders")).clicked() {
            app.library.folders_expanded = !app.library.folders_expanded;
            app.library.folder_menu = None;
        }
        let (rect, settings) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
        if settings.hovered() || settings.has_focus() {
            ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
        }
        crate::icons::paint_ink(ui.painter(), rect.shrink(4.0), "settings", crate::theme::Tokens::get().text);
        ui.painter().rect_stroke(
            rect,
            crate::theme::Tokens::get().control_radius,
            egui::Stroke::new(1.0, crate::theme::Tokens::get().border),
            egui::StrokeKind::Inside,
        );
        let settings = settings.on_hover_text(crate::i18n::tr("library.choose_folder"));
        settings.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::i18n::tr("library.choose_folder")));
        crate::scenario::publish_handle("library:settings", settings.rect.center());
        if settings.clicked() {
            choose_folder(app);
        }
    });
    if app.library.folders_expanded {
        ui.separator();
        let recent = app.library.recent_folders();
        let mut selected = None;
        egui::ScrollArea::vertical().id_salt("library_recent_folders").max_height(180.0).show(ui, |ui| {
            for (index, path) in recent.iter().enumerate() {
                if index > 0 && index == app.library.pinned_roots.len() {
                    ui.separator();
                }
                let label = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                let duplicate_name = recent.iter().filter(|other| other.file_name() == path.file_name()).count() > 1;
                let label = if duplicate_name { path.display().to_string() } else { label.into_owned() };
                let response = crate::ui_kit::list_picker(ui, &format!("library_folder_{index}"), &label, app.library.root.as_ref() == Some(path));
                crate::scenario::publish_handle(&format!("library:recent:{index}"), response.rect.center());
                let response = response.on_hover_text(path.display().to_string());
                let keyboard = response.has_focus() && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F10));
                if response.secondary_clicked() || keyboard {
                    let at = ui.ctx().pointer_interact_pos().unwrap_or(response.rect.center());
                    app.library.folder_menu = Some((path.clone(), response.clone(), at));
                }
                if response.clicked() {
                    selected = Some(path.clone());
                }
            }
        });
        if let Some(path) = selected {
            app.library.set_root(path);
        }
        ui.separator();
    }
    if let Some((path, response, at)) = app.library.folder_menu.take() {
        let mut open = true;
        Container::new(ui.ctx(), Surface::Popup).context_menu(&response).open_bool(&mut open).at_position(at).show(|ui| {
            let pinned = app.library.pinned_roots.contains(&path);
            let pin_key = if pinned { "library.unpin_folder" } else { "library.pin_folder" };
            if crate::ui_kit::button(
                ui,
                crate::i18n::tr(pin_key),
                crate::ui_kit::Button::Enabled(pinned || app.library.pinned_roots.len() < MAX_RECENT_FOLDERS),
            )
            .clicked()
            {
                if pinned {
                    app.library.pinned_roots.retain(|p| p != &path);
                    app.library.trim_history();
                } else {
                    app.library.pinned_roots.push(path.clone());
                }
                ui.close();
            }
            if crate::ui_kit::button(ui, crate::i18n::tr("library.remove_folder"), crate::ui_kit::Button::Normal).clicked() {
                app.library.remove_recent(&path);
                ui.close();
            }
        });
        if open {
            app.library.folder_menu = Some((path, response, at));
        }
    }
    if app.library.root.is_none() {
        egui::Frame::NONE.fill(crate::theme::Tokens::get().field).inner_margin(8.0).corner_radius(4.0).show(ui, |ui| {
            ui.label(crate::i18n::tr("library.samples_note"));
        });
        ui.add_space(6.0);
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| sample_rows(app, ui));
        return None;
    }
    if app.library.root.as_ref().is_some_and(|root| &app.library.current != root) {
        ui.horizontal_wrapped(|ui| {
            if let Some(root) = app.library.root.clone() {
                let relative = app.library.current.strip_prefix(&root).unwrap_or(Path::new(""));
                let parts: Vec<_> = relative.components().map(|c| c.as_os_str().to_owned()).collect();
                let mut path = root;
                for part in parts {
                    path.push(&part);
                    ui.label("/");
                    if crate::ui_kit::button(ui, part.to_string_lossy(), crate::ui_kit::Button::Small).clicked() {
                        app.library.navigate(path.clone());
                    }
                }
            }
        });
    }
    let t = crate::theme::Tokens::get();
    ui.horizontal(|ui| {
        let button_size = 32.0;
        let width = (ui.available_width() - button_size - ui.spacing().item_spacing.x).max(1.0);
        let search = ui
            .allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::Frame::NONE
                    .fill(t.field)
                    .stroke(egui::Stroke::new(1.0, t.border))
                    .corner_radius(t.control_radius)
                    .inner_margin(egui::Margin::symmetric(t.space_6 as i8, t.space_4 as i8))
                    .show(ui, |ui| {
                        let response = crate::ui_kit::shell_search_field(
                            ui,
                            ui.id().with("library_search"),
                            &mut app.library.search,
                            crate::i18n::tr("library.search_hint"),
                        );
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, crate::i18n::tr("library.search_label")));
                    })
                    .response
                    .rect
            })
            .inner;
        crate::scenario::publish_rect("library:search_card", search);
        let rect =
            egui::Rect::from_center_size(egui::pos2(ui.cursor().left() + button_size / 2.0, search.center().y), egui::vec2(button_size, button_size));
        ui.allocate_rect(rect, egui::Sense::hover());
        let refresh = crate::ui_kit::icon_button_colored(ui, rect, "refresh", crate::i18n::tr("library.refresh"), Some(t.text));
        crate::scenario::publish_rect("library:refresh", rect);
        ui.painter().rect_stroke(rect, t.control_radius, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        if refresh.clicked() {
            app.library.refresh();
        }
    });
    ui.add_space(t.space_6);
    app.library.update_filter();
    messages(&app.library, ui);
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 0)).show(ui, |ui| rows(app, ui)).inner
}

/// Design-view title-bar control, available even when the document panel is collapsed.
pub(super) fn app_bar_toggle(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    if !available(app) || app.home.open {
        return false;
    }
    let t = crate::theme::Tokens::get();
    let response = ui.interact(rect, ui.id().with("document_browser_toggle"), egui::Sense::click());
    let label = if app.library.collapsed { crate::i18n::tr("library.show") } else { crate::i18n::tr("library.hide") };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    crate::scenario::publish_handle("library:toggle", rect.center());
    crate::scenario::publish_rect("library:toggle", rect);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 4.0, t.app_bar_hover);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect.shrink(1.0), 4.0, egui::Stroke::new(1.0, t.app_bar_text), egui::StrokeKind::Inside);
    }
    crate::icons::paint_app_bar(ui.painter(), rect.shrink(4.0), "document_browser", 1.0);
    if response.on_hover_text(label).clicked() {
        ui.memory_mut(|memory| memory.request_focus(ui.id().with("document_browser_toggle")));
        app.library.collapsed = !app.library.collapsed;
    }
    true
}

pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    if !available(app) {
        return;
    }
    ui.input(|input| {
        // A quick app switch can deliver both events in the same frame.
        for event in &input.events {
            if let egui::Event::WindowFocused(focused) = event {
                app.library.focus(*focused);
            }
        }
        app.library.focus(input.focused);
    });
    // Home owns the full File view. Keep focus tracking, but defer folder work until Design returns.
    if app.home.open {
        actions::dialog(app, ui.ctx());
        return;
    }
    app.library.poll(ui.ctx());
    if std::mem::take(&mut app.library.listing_changed) {
        crate::home::refresh_document_thumbnails(&mut app.home, &app.library.listing.identities);
    }
    if app.library.collapsed {
        actions::dialog(app, ui.ctx());
        return;
    }
    let width = 300.0;
    let mut chosen = None;
    Container::new(ui.ctx(), Surface::Panel)
        .edge("sc_document_library", Edge::Left)
        .exact_size(width)
        .resizable(false)
        .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(crate::theme::Tokens::get().panel))
        .show(ui, |ui| {
            crate::scenario::publish_rect("library:panel", ui.max_rect());
            insert_footer(app, ui);
            // Keep content widget identities independent of the footer.
            ui.scope_builder(egui::UiBuilder::new().id(ui.id().with("library_contents")), |ui| {
                egui::Frame::NONE.inner_margin(6).show(ui, |ui| {
                    chosen = contents(app, ui);
                });
            });
        });
    if let Some(entry) = chosen {
        open_entry(app, entry);
    }
    actions::dialog(app, ui.ctx());
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn wait(app: &mut SolveApp) {
        for _ in 0..200 {
            app.library.poll(&egui::Context::default());
            if !app.library.loading() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.library.loading(), "folder scan did not finish");
    }

    fn folder_picker(h: &mut Harness, path: Option<String>) {
        h.app.services.pick_folder = Some(Box::new(move || Box::pin(std::future::ready(path.clone()))));
    }

    #[test]
    fn search_cache_reuses_results_until_query_or_listing_changes() {
        let mut state = LibraryState::default();
        state.listing.entries =
            (0..10_000).map(|i| Entry { path: PathBuf::from(format!("{i}.sc")), name: format!("Design {i}"), folder: false }).collect();
        state.search = "DESIGN 123".into();
        assert!(state.update_filter());
        assert_eq!(state.filtered.len(), 11);
        for _ in 0..20 {
            assert!(!state.update_filter());
        }
        state.search = "absent".into();
        assert!(state.update_filter());
        assert!(state.filtered.is_empty());
        state.refresh();
        assert!(state.update_filter());
        assert!(state.filtered.is_empty());
    }

    #[test]
    fn folder_scan_tracks_external_replacements_without_reading_designs() {
        let h = Harness::new();
        let path = h.dir.join("Changed.sc");
        std::fs::write(&path, b"old").unwrap();
        let before = read_folder(&h.dir, &h.dir).unwrap();
        std::fs::write(&path, b"new bytes").unwrap();
        let after = read_folder(&h.dir, &h.dir).unwrap();
        assert_ne!(before.identities.get(&path), after.identities.get(&path));
    }

    #[test]
    fn recent_folders_are_bounded_deduplicated_and_persist_pins_and_removals() {
        let mut state = LibraryState::default();
        state.load(Some(&json!({"root":"/legacy/CAD", "collapsed":true})));
        assert_eq!(state.recent_folders(), [PathBuf::from("/legacy/CAD")]);
        state.pinned_roots.push(PathBuf::from("/legacy/CAD"));
        for index in 0..30 {
            state.remember_root(PathBuf::from(format!("/project/{index}")));
        }
        assert_eq!(state.recent_roots.len(), MAX_RECENT_FOLDERS + 1);
        assert_eq!(state.recent_folders()[0], PathBuf::from("/legacy/CAD"));
        state.remember_root(PathBuf::from("/project/29"));
        assert_eq!(state.recent_roots.len(), MAX_RECENT_FOLDERS + 1);
        let mut pinned_restore = LibraryState::default();
        pinned_restore.load(Some(&state.prefs()));
        assert_eq!(pinned_restore.pinned_roots, state.pinned_roots);
        assert_eq!(pinned_restore.recent_folders(), state.recent_folders());
        state.remove_recent(Path::new("/legacy/CAD"));
        let mut restored = LibraryState::default();
        restored.load(Some(&state.prefs()));
        assert_eq!(restored.root, state.root, "replacement folder persists after removal");
        assert_eq!(restored.recent_folders(), state.recent_folders(), "removed active folder stays removed on restart");
        assert!(restored.pinned_roots.is_empty());
        restored.load(Some(&json!({"recent_roots":[null,"","/valid","/valid",42],"pinned_roots":["/missing","/valid","/valid"]})));
        assert_eq!(restored.recent_folders(), [PathBuf::from("/valid")]);
        assert_eq!(restored.pinned_roots.len(), 1);
    }

    #[test]
    fn recent_folder_dropdown_switches_pins_and_removes_without_touching_files() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.home.open = false;
        let first = h.dir.join("First");
        let second = h.dir.join("Second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::fs::write(first.join("keep.txt"), b"untouched").unwrap();
        h.app.library.set_root(first.clone());
        h.app.library.set_root(second.clone());
        h.frames(3);
        let search_before = crate::scenario::rect_of("library:search_card").unwrap();
        h.step(&json!({"click":{"handle":"library:folders"}})).unwrap();
        assert!(crate::scenario::rect_of("library:search_card").unwrap().top() > search_before.top());
        h.step(&json!({"click":{"handle":"library:recent:1"},"button":"right"})).unwrap();
        assert!(h.app.library.folders_expanded);
        assert!(h.widget("Second").is_some());
        h.step(&json!({"widget":"Pin to top"})).unwrap();
        assert_eq!(h.app.library.recent_folders()[0], first);
        h.frames(2);
        assert!(h.app.library.folders_expanded);
        h.step(&json!({"click":{"handle":"library:recent:0"}})).unwrap();
        wait(&mut h.app);
        assert_eq!(h.app.library.root.as_ref(), Some(&first));
        assert!(h.app.library.search.is_empty());
        h.frames(2);
        h.step(&json!({"click":{"handle":"library:folders"}})).unwrap();
        h.step(&json!({"click":{"handle":"library:recent:0"},"button":"right"})).unwrap();
        h.step(&json!({"widget":"Remove from list"})).unwrap();
        assert!(!h.app.library.folders_expanded);
        assert_eq!(h.app.library.root.as_ref(), Some(&second));
        assert!(!h.app.library.recent_folders().contains(&first));
        assert_eq!(std::fs::read(first.join("keep.txt")).unwrap(), b"untouched");
    }

    #[test]
    fn removing_three_folders_down_to_one_selects_the_survivor_and_collapses() {
        for remove_active in [true, false] {
            for pinned in [true, false] {
                let mut h = Harness::new();
                folder_picker(&mut h, None);
                h.app.home.open = false;
                let paths = [h.dir.join("First"), h.dir.join("Second"), h.dir.join("Third")];
                for path in &paths {
                    std::fs::create_dir(path).unwrap();
                    std::fs::write(path.join("keep.txt"), b"untouched").unwrap();
                    h.app.library.set_root(path.clone());
                }
                let survivor = &paths[if remove_active { 0 } else { 2 }];
                if pinned {
                    h.app.library.pinned_roots.push(survivor.clone());
                }
                h.frames(3);
                h.step(&json!({"click":{"handle":"library:folders"}})).unwrap();
                for (step, remove) in [if remove_active { &paths[2] } else { &paths[0] }, &paths[1]].iter().enumerate() {
                    let index = h.app.library.recent_folders().iter().position(|path| path == *remove).unwrap();
                    h.step(&json!({"click":{"handle":format!("library:recent:{index}")},"button":"right"})).unwrap();
                    h.step(&json!({"widget":"Remove from list"})).unwrap();
                    wait(&mut h.app);
                    h.frames(2);
                    assert_eq!(h.app.library.recent_folders().len(), 2 - step);
                    assert_eq!(h.app.library.folders_expanded, step == 0);
                }
                assert_eq!(h.app.library.root.as_ref(), Some(survivor));
                assert_eq!(&h.app.library.current, survivor);
                assert!(h.app.library.folder_menu.is_none());
                for path in &paths {
                    assert_eq!(std::fs::read(path.join("keep.txt")).unwrap(), b"untouched");
                }
            }
        }
    }

    #[test]
    fn removing_folders_switches_only_the_active_view_and_clears_the_last_listing() {
        let mut h = Harness::new();
        let first = h.dir.join("First");
        let second = h.dir.join("Second");
        let third = h.dir.join("Third");
        for path in [&first, &second, &third] {
            std::fs::create_dir(path).unwrap();
            std::fs::create_dir(path.join("Contents")).unwrap();
            h.app.library.set_root(path.clone());
        }
        h.app.library.pinned_roots.push(first.clone());
        h.app.library.folders_expanded = true;
        h.app.library.remove_recent(&third);
        wait(&mut h.app);
        assert_eq!(h.app.library.root.as_ref(), Some(&second), "select the row after the removed active row");
        assert!(h.app.library.folders_expanded);
        assert_eq!(h.app.library.current, second);
        assert_eq!(h.app.library.listing.entries.len(), 1);
        h.app.library.remove_recent(&first);
        assert_eq!(h.app.library.root.as_ref(), Some(&second), "removing an inactive row preserves the view");
        h.app.library.search = "stale search".into();
        h.app.library.remove_recent(&second);
        wait(&mut h.app);
        assert!(h.app.library.root.is_none());
        assert!(h.app.library.current.as_os_str().is_empty());
        assert!(h.app.library.listing.entries.is_empty());
        assert!(h.app.library.search.is_empty());
        assert!(!h.app.library.loading());
        assert!(second.join("Contents").is_dir());
        let mut restored = LibraryState::default();
        restored.load(Some(&h.app.library.prefs()));
        assert!(restored.root.is_none());
        assert!(restored.recent_folders().is_empty());
    }

    #[test]
    fn highlights_survive_folder_switches_refresh_and_restart() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.home.open = false;
        let first = h.dir.join("First");
        let second = h.dir.join("Second");
        for folder in [&first, &second] {
            std::fs::create_dir(folder).unwrap();
            std::fs::write(folder.join("Model.solidcraft"), b"untouched design").unwrap();
        }
        h.app.library.set_root(first.clone());
        h.app.library.set_highlight(&first.join("Model.solidcraft"), Some(Highlight::Orange));
        wait(&mut h.app);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:highlight_indicator").is_some());
        h.app.library.set_root(second.clone());
        wait(&mut h.app);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:highlight_indicator").is_none(), "same filename in another folder is independent");
        h.app.library.set_root(first.clone());
        wait(&mut h.app);
        h.app.library.refresh();
        wait(&mut h.app);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:highlight_indicator").is_some());
        let prefs = h.app.library.prefs();
        h.app.library = LibraryState::default();
        h.app.library.load(Some(&prefs));
        wait(&mut h.app);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:highlight_indicator").is_some());
        assert_eq!(h.app.library.highlights.get(&first.join("Model.solidcraft")), Some(&Highlight::Orange));
        for folder in [&first, &second] {
            assert_eq!(std::fs::read(folder.join("Model.solidcraft")).unwrap(), b"untouched design");
        }
        for theme in [crate::theme::Tokens::light(), crate::theme::Tokens::dark()] {
            for color in theme.library_highlights {
                for ground in [theme.field, theme.hover] {
                    assert!(crate::theme::contrast_ratio(color, ground) >= 3.0);
                }
            }
        }
    }

    #[test]
    fn file_highlights_persist_by_full_path_and_tolerate_invalid_preferences() {
        let mut state = LibraryState::default();
        let first = Path::new("/CAD/Model.solidcraft");
        let other = Path::new("/Other/Model.solidcraft");
        state.set_highlight(first, Some(Highlight::Red));
        state.set_highlight(other, Some(Highlight::Blue));
        let mut restored = LibraryState::default();
        restored.load(Some(&state.prefs()));
        assert_eq!(restored.highlights, state.highlights);
        restored.set_highlight(first, None);
        assert_eq!(restored.highlights.get(other), Some(&Highlight::Blue));
        assert!(!restored.highlights.contains_key(first));
        restored.load(Some(&json!({"highlights":{"/valid":"purple","/bad":"chartreuse","":"red","/invalid":42}})));
        assert_eq!(restored.highlights.len(), 1);
        assert_eq!(restored.highlights.get(Path::new("/valid")), Some(&Highlight::Purple));
        restored.load(Some(&json!({"root":null})));
        assert!(restored.highlights.is_empty(), "old settings remain compatible");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let invalid = Path::new(std::ffi::OsStr::from_bytes(b"/CAD/invalid\xff.solidcraft"));
            restored.set_highlight(invalid, Some(Highlight::Red));
            assert!(restored.error.is_some());
            assert!(restored.highlights.is_empty());
            assert_eq!(restored.prefs()["highlights"], json!({}), "invalid filename must not break preference serialization");
        }
    }

    #[test]
    fn active_thumbnail_dot_follows_exact_file_and_active_tab() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.home.open = false;
        let first = h.dir.join("Model.solidcraft");
        let second = h.dir.join("Model copy.solidcraft");
        for path in [&first, &second] {
            std::fs::write(path, b"same preview").unwrap();
        }
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:active_indicator").is_none());
        h.app.session.path = Some(first.to_string_lossy().into_owned());
        h.frames(3);
        let first_dot = crate::scenario::rect_of("library:active_indicator").unwrap();
        let entry_index = h.app.library.visible().iter().position(|entry| entry.path == first).unwrap();
        assert!(crate::scenario::handle_at(&format!("library:entry:{entry_index}")).unwrap().distance(first_dot.center()) < 200.0);
        let mut session = solidcraft_engine::Session::default();
        session.path = Some(second.to_string_lossy().into_owned());
        crate::documents::open_tab(&mut h.app, session).unwrap();
        h.frames(3);
        let second_dot = crate::scenario::rect_of("library:active_indicator").unwrap();
        assert_ne!(first_dot, second_dot);
        crate::documents::switch(&mut h.app, 0);
        h.frames(3);
        assert_eq!(crate::scenario::rect_of("library:active_indicator"), Some(first_dot));
        h.app.session.path = Some(h.dir.join("Elsewhere/Model.solidcraft").to_string_lossy().into_owned());
        h.frames(3);
        assert!(crate::scenario::rect_of("library:active_indicator").is_none(), "same basename in another folder must not match");
        h.app.session.path = None;
        h.frames(3);
        assert!(crate::scenario::rect_of("library:active_indicator").is_none());
    }

    #[test]
    fn folder_listing_places_newest_designs_first_and_folders_above_them() {
        let h = Harness::new();
        std::fs::create_dir(h.dir.join("Project")).unwrap();
        let older = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        let newer = older + std::time::Duration::from_secs(60);
        for (name, time) in [("A old.solidcraft", older), ("Z newest.solidcraft", newer), ("B newest.solidcraft", newer)] {
            let file = std::fs::File::create(h.dir.join(name)).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(time)).unwrap();
        }
        let listing = read_folder(&h.dir, &h.dir).unwrap();
        let names: Vec<_> = listing.entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Project", "B newest.solidcraft", "Z newest.solidcraft", "A old.solidcraft"]);
        // Track the shared current/legacy design extensions, without listing import formats.
        for name in ["Current.SC", "Legacy.solvecraft", "Plain.json", "Import.step", "Import.iges", "Import.stl"] {
            std::fs::write(h.dir.join(name), b"listing fixture").unwrap();
        }
        let listing = read_folder(&h.dir, &h.dir).unwrap();
        for name in ["Current.SC", "Legacy.solvecraft", "Plain.json"] {
            assert!(listing.entries.iter().any(|entry| entry.name == name), "missing {name}");
        }
        for name in ["Import.step", "Import.iges", "Import.stl"] {
            assert!(!listing.entries.iter().any(|entry| entry.name == name), "import file listed: {name}");
        }
    }

    #[test]
    fn returning_to_app_refreshes_external_changes_once_and_preserves_search() {
        let mut h = Harness::new();
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        h.app.library.search = "external".into();
        h.app.library.focus(true);
        assert!(!h.app.library.refresh_on_return);
        h.app.library.focus(false);
        std::fs::write(h.dir.join("External.solidcraft"), b"test listing").unwrap();
        h.app.library.poll(&h.ctx);
        assert!(h.app.library.visible().is_empty(), "no scans while away");
        h.app.library.focus(true);
        wait(&mut h.app);
        assert_eq!(h.app.library.visible().len(), 1);
        assert_eq!(h.app.library.search, "external");
        std::fs::write(h.dir.join("External second.solidcraft"), b"test listing").unwrap();
        for _ in 0..5 {
            h.app.library.focus(true);
            h.app.library.poll(&h.ctx);
            assert!(!h.app.library.loading());
        }
        assert_eq!(h.app.library.visible().len(), 1, "focused frames do not rescan");
        h.app.library.focus(false);
        h.app.library.focus(true);
        wait(&mut h.app);
        assert_eq!(h.app.library.visible().len(), 2);
    }

    #[test]
    fn quick_focus_loss_and_return_in_one_frame_still_refreshes() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        h.frames(2);
        std::fs::write(h.dir.join("Quick return.solidcraft"), b"listing").unwrap();
        let raw =
            egui::RawInput { focused: true, events: vec![egui::Event::WindowFocused(false), egui::Event::WindowFocused(true)], ..Default::default() };
        let ctx = h.ctx.clone();
        let _ = ctx.run_ui(raw, |ui| panel(&mut h.app, ui));
        wait(&mut h.app);
        assert_eq!(h.app.library.visible().len(), 1);
    }

    #[test]
    fn focus_refresh_waits_for_active_scan_and_hidden_browser_and_skips_examples() {
        let mut state = LibraryState::default();
        state.focus(false);
        state.focus(true);
        assert!(!state.refresh_on_return);
        let mut h = Harness::new();
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.library.pending = Some(rx);
        h.app.library.focus(false);
        h.app.library.focus(true);
        h.app.library.poll(&h.ctx);
        assert!(h.app.library.refresh_on_return);
        h.app.library.collapsed = true;
        tx.send(Ok(Listing::default())).unwrap();
        h.app.library.poll(&h.ctx);
        assert!(!h.app.library.loading());
        assert!(h.app.library.refresh_on_return);
        std::fs::write(h.dir.join("Added while hidden.solidcraft"), b"listing").unwrap();
        h.app.library.collapsed = false;
        wait(&mut h.app);
        assert_eq!(h.app.library.visible().len(), 1);
        assert!(!h.app.library.refresh_on_return);
    }

    #[test]
    fn unconfigured_browser_shows_examples_without_file_actions_or_writes() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.home.open = false;
        let before = std::fs::read_dir(&h.dir).unwrap().count();
        h.frames(3);
        assert!(h.widget("Sample designs only. Use the gear above to choose your CAD folder.").is_some());
        for sample in solidcraft_engine::sample::samples() {
            assert!(h.widget(sample.name).is_some(), "{}", sample.name);
        }
        h.step(&json!({"click":{"handle":"library:sample:0"},"button":"right"})).unwrap();
        h.frames(2);
        for label in ["Rename", "Move to Trash", "Insert into Current Design", "Search documents", "Refresh"] {
            assert!(h.widget(label).is_none(), "{label}");
        }
        assert!(h.app.library.root.is_none());
        assert_eq!(std::fs::read_dir(&h.dir).unwrap().count(), before);
    }

    #[test]
    fn choosing_folder_from_gear_collapses_history_but_cancel_preserves_it() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.library.set_root(h.dir.clone());
        folder_picker(&mut h, None);
        h.frames(3);
        h.step(&json!({"click":{"handle":"library:folders"}})).unwrap();
        assert!(h.app.library.folders_expanded);
        h.step(&json!({"click":{"handle":"library:settings"}})).unwrap();
        crate::file_picker::tick(&mut h.app, &h.ctx);
        h.frames(3);
        assert!(h.app.library.folders_expanded);
        assert_eq!(h.app.library.root.as_ref(), Some(&h.dir));
        let selected = h.dir.join("Chosen folder");
        std::fs::create_dir(&selected).unwrap();
        std::fs::create_dir(selected.join("Visible contents")).unwrap();
        folder_picker(&mut h, Some(selected.to_string_lossy().into_owned()));
        h.step(&json!({"click":{"handle":"library:settings"}})).unwrap();
        crate::file_picker::tick(&mut h.app, &h.ctx);
        wait(&mut h.app);
        h.frames(3);
        assert_eq!(h.app.library.root.as_ref(), Some(&selected));
        assert!(h.widget("Visible contents").is_some());
        assert!(!h.app.library.folders_expanded);
        assert!(h.app.library.folder_menu.is_none());
    }

    #[test]
    fn choosing_folder_replaces_examples_and_cancel_keeps_them() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.home.open = false;
        h.frames(3);
        h.step(&json!({"click":{"handle":"library:settings"}})).unwrap();
        crate::file_picker::tick(&mut h.app, &h.ctx);
        h.frames(3);
        assert!(h.app.library.root.is_none());
        assert!(h.widget("Sample Plate").is_some());
        std::fs::create_dir(h.dir.join("My project")).unwrap();
        let directory = h.dir.to_string_lossy().into_owned();
        folder_picker(&mut h, Some(directory.clone()));
        h.step(&json!({"click":{"handle":"library:settings"}})).unwrap();
        crate::file_picker::tick(&mut h.app, &h.ctx);
        wait(&mut h.app);
        h.frames(3);
        assert!(h.widget("Sample Plate").is_none());
        assert!(h.widget("My project").is_some());
        assert!(h.widget("Search documents").is_some());
        assert_eq!(h.app.library.prefs()["root"], directory);
        let mut restored = LibraryState::default();
        restored.load(Some(&h.app.library.prefs()));
        assert_eq!(restored.root, h.app.library.root);
    }

    #[test]
    fn insert_footer_uses_existing_picker_and_cancel_preserves_design() {
        use std::{cell::Cell, rc::Rc};
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.run("solid.box", json!({"length":4,"width":5,"height":6})).unwrap();
        let original = h.app.session.doc.to_json();
        let asked = Rc::new(Cell::new(0));
        let count = asked.clone();
        let cancel = Rc::new(Cell::new(false));
        let answer = cancel.clone();
        let filters = Rc::new(std::cell::RefCell::new(Vec::new()));
        let captured = filters.clone();
        h.app.services.pick_open = Some(Box::new(move |extensions| {
            captured.borrow_mut().push(extensions.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
            count.set(count.get() + 1);
            let answer = answer.clone();
            Box::pin(std::future::poll_fn(move |_| if answer.get() { std::task::Poll::Ready(None) } else { std::task::Poll::Pending }))
        }));
        // No library root is required for inserting into the active design.
        for handle in ["library:insert_step", "library:insert_iges", "library:insert_mesh"] {
            cancel.set(false);
            h.app.home.open = false;
            h.frames(3);
            h.step(&json!({"click":{"handle":handle}})).unwrap();
            assert!(h.app.file_picker_busy());
            assert!(!h.app.home.open);
            assert_eq!(h.app.session.doc.to_json(), original);
            let calls = asked.get();
            let other = if handle.ends_with("mesh") { "library:insert_step" } else { "library:insert_mesh" };
            h.step(&json!({"click":{"handle":other}})).unwrap();
            assert_eq!(asked.get(), calls, "a busy picker must not open another dialog");
            cancel.set(true);
            crate::file_picker::tick(&mut h.app, &h.ctx);
            assert!(!h.app.file_picker_busy());
            assert_eq!(h.app.session.doc.to_json(), original);
            assert!(h.app.library.root.is_none());
        }
        assert_eq!(asked.get(), 3);
        assert_eq!(*filters.borrow(), vec![vec!["step", "stp"], vec!["igs", "iges"], vec!["3mf", "stl", "obj", "glb", "gltf"]]);
    }

    #[test]
    fn insert_footer_stays_at_bottom_with_many_cards_and_hides_when_collapsed() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            folder_picker(&mut h, None);
            h.frames(3);
            let before = crate::scenario::handle_at("library:insert_mesh").unwrap();
            let step = crate::scenario::handle_at("library:insert_step").unwrap();
            let iges = crate::scenario::handle_at("library:insert_iges").unwrap();
            assert!(step.x < iges.x && iges.x < before.x);
            assert!(before.x - step.x > 100.0, "import controls overlap: {before:?} {step:?}");
            // Follow the shared History layout: floating inset or flush docked edge.
            let card = crate::scenario::rect_of("library:import_card").unwrap();
            assert!((card.height() - 40.0).abs() < 0.5, "card={card:?}, controls={before:?}/{step:?}");
            let gap = if mode == crate::shell::LayoutMode::Floating { crate::shell::SHELL_GAP } else { 0.0 };
            let panel = crate::scenario::rect_of("library:panel").unwrap();
            assert!((card.bottom() - (panel.bottom() - gap)).abs() < 0.5, "{mode:?}: {card:?} {panel:?}");
            assert!(card.contains(before) && card.contains(step) && card.contains(iges));
            h.app.library.root = Some(h.dir.clone());
            h.app.library.current = h.dir.clone();
            h.app.library.listing.entries =
                (0..1000).map(|i| Entry { path: h.dir.join(format!("Project {i}")), name: format!("Project {i}"), folder: true }).collect();
            h.frames(3);
            let search = crate::scenario::rect_of("library:search_card").unwrap();
            let refresh = crate::scenario::rect_of("library:refresh").unwrap();
            assert!(search.right() < refresh.left(), "search and refresh overlap");
            assert!((search.center().y - refresh.center().y).abs() < 0.5, "search and refresh must share a row");
            assert_eq!(crate::scenario::handle_at("library:insert_mesh"), Some(before));
            assert!(h.widget("Mesh").is_some());
            assert!(h.widget("STEP").is_some());
            h.app.library.collapsed = true;
            h.frames(3);
            assert!(crate::scenario::handle_at("library:insert_mesh").is_none());
            assert!(crate::scenario::handle_at("library:insert_step").is_none());
        }
    }

    #[test]
    fn home_owns_the_full_width_and_returning_preserves_browser_visibility() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for collapsed in [false, true] {
                let mut h = Harness::new();
                folder_picker(&mut h, None);
                h.app.library.set_root(h.dir.clone());
                wait(&mut h.app);
                h.app.library.collapsed = collapsed;
                let saved = h.app.library.prefs();
                let design = h.app.session.doc.to_json();
                h.call("ui.set", json!({"layout":mode}));
                h.app.home.open = true;
                h.frames(3);
                assert!(crate::scenario::rect_of("home:page").unwrap().left() < 1.0);
                for handle in
                    ["library:panel", "library:toggle", "library:insert_step", "library:insert_iges", "library:insert_mesh", "library:settings"]
                {
                    assert!(crate::scenario::rect_of(handle).is_none(), "{handle} on Home");
                }
                assert_eq!(h.app.library.prefs(), saved);
                h.step(&json!({"click":{"handle":"home:back"}})).unwrap();
                h.frames(3);
                assert!(!h.app.home.open);
                assert_eq!(crate::scenario::rect_of("library:panel").is_none(), collapsed);
                assert!(crate::scenario::rect_of("library:toggle").is_some());
                assert_eq!(h.app.library.prefs(), saved);
                assert_eq!(h.app.session.doc.to_json(), design);
            }
        }
    }

    #[test]
    fn browser_chrome_uses_shell_colors_in_builtin_and_custom_themes() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.library.root = Some(h.dir.join("CAD Files"));
        h.app.library.current = h.app.library.root.clone().unwrap();
        h.app.home.open = false;
        for theme in ["light", "dark", "gruvbox-dark", "gruvbox-light", "cupertino", "high-contrast"] {
            h.app.preferences.theme = theme.into();
            h.frames(3);
            let ctx = h.ctx.clone();
            let output = crate::test_support::frame(
                &ctx,
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))), ..Default::default() },
                |ui| {
                    h.app.logic(ui.ctx());
                    h.app.ui(ui);
                },
            );
            let t = crate::theme::Tokens::get();
            for label in ["CAD Files", "STEP", "IGES", "Mesh"] {
                let text = output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(text) if text.galley.text() == label => Some(text),
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing {label} in {theme}"));
                assert!(text.galley.job.sections.iter().all(|s| s.format.color == t.text), "{label} in {theme}");
            }
            for key in ["library:insert_mesh", "library:insert_step", "library:insert_iges", "library:search_card", "library:refresh"] {
                let rect = crate::scenario::rect_of(key).unwrap();
                assert!(
                    output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect == rect && r.stroke.color == t.border)),
                    "{key} in {theme}"
                );
            }
        }
    }

    #[test]
    fn header_gear_stays_at_the_right_and_titlebar_toggle_precedes_file() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        for theme in ["light", "dark"] {
            h.app.preferences.theme = theme.into();
            for width in [520, 800, 1280] {
                h.call("ui.resize", json!({"width":width,"height":720}));
                for name in ["CAD Files".to_owned(), "Long CAD directory ".repeat(30)] {
                    h.app.library.root = Some(h.dir.join(&name));
                    h.app.library.current = h.app.library.root.clone().unwrap();
                    h.app.home.open = false;
                    h.frames(3);
                    let panel = crate::scenario::rect_of("library:panel").unwrap();
                    let gear = crate::scenario::handle_at("library:settings").unwrap();
                    assert!((gear.x - (panel.right() - 20.0)).abs() < 1.0, "{panel:?} {gear:?}");
                    let toggle = crate::scenario::rect_of("library:toggle").unwrap();
                    let file = crate::scenario::rect_of("appbar:file").unwrap();
                    let home = crate::scenario::rect_of("appbar:home").unwrap();
                    assert!(toggle.right() < home.left());
                    assert!(home.right() < file.left());
                    assert!(toggle.bottom() < panel.top());
                    let viewport = h.app.viewport.rect.unwrap();
                    assert!(viewport.left() >= panel.right(), "{panel:?} {viewport:?}");
                }
            }
        }
    }

    #[test]
    fn lists_projects_and_designs_without_reading_or_modifying_them() {
        let h = Harness::new();
        std::fs::create_dir(h.dir.join("Project A")).unwrap();
        let design = h.dir.join("Plate.SOLIDCRAFT");
        std::fs::write(&design, b"deliberately not a design").unwrap();
        std::fs::write(h.dir.join("notes.txt"), b"keep").unwrap();
        let listing = read_folder(&h.dir, &h.dir).unwrap();
        assert_eq!(listing.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Project A", "Plate.SOLIDCRAFT"]);
        assert!(listing.entries[0].folder);
        assert_eq!(std::fs::read(design).unwrap(), b"deliberately not a design");
        assert!(read_folder(&h.dir, h.dir.parent().unwrap()).is_err());
        assert!(read_folder(&h.dir.join("gone"), &h.dir).is_err());
    }

    #[test]
    fn successful_save_and_save_as_refresh_the_selected_folder() {
        let mut h = Harness::new();
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        assert!(h.app.library.visible().is_empty());
        h.app.library.search = "saved".into();
        let root = h.app.library.root.clone();
        let current = h.app.library.current.clone();

        h.app.run("solid.box", json!({"length":4,"width":5,"height":6})).unwrap();
        // Unrelated edits must not trigger directory scans.
        assert!(h.app.library.pending.is_none());
        let saved = h.dir.join("Saved.solidcraft");
        let target = saved.to_string_lossy().into_owned();
        h.app.services.pick_save = Some(Box::new(move |_, _| {
            let target = target.clone();
            Box::pin(std::future::ready(Some(target)))
        }));
        crate::toolbar::save(&mut h.app);
        assert!(h.app.file_picker_busy());
        crate::file_picker::tick(&mut h.app, &h.ctx);
        crate::save_job::tick(&mut h.app, true);
        wait(&mut h.app);
        assert_eq!(h.app.session.path.as_deref(), saved.to_str());
        assert_eq!(h.app.library.visible().len(), 1);
        assert_eq!(h.app.library.visible()[0].path, saved);

        // Normal Save also updates the list; no folder picker or Refresh is invoked.
        std::fs::copy(&saved, h.dir.join("Saved copy.solidcraft")).unwrap();
        crate::toolbar::save(&mut h.app);
        crate::save_job::tick(&mut h.app, true);
        wait(&mut h.app);
        assert_eq!(h.app.library.visible().len(), 2);
        assert_eq!(h.app.library.search, "saved");
        assert_eq!(h.app.library.root, root);
        assert_eq!(h.app.library.current, current);
        assert!(h.app.run("file.save_as", json!({"path":h.dir.to_string_lossy()})).is_err());
        assert!(h.app.library.pending.is_none(), "failed saves must not trigger a folder scan");
    }

    #[test]
    fn copy_and_move_commands_refresh_the_listing_without_touching_the_open_design() {
        let mut h = Harness::new();
        let source = h.dir.join("Original.solidcraft");
        std::fs::write(&source, solidcraft_engine::io::write_design(&solidcraft_engine::doc::Document::new("Original"))).unwrap();
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        let doc = h.app.session.doc.to_json();
        let copy = h.dir.join("Copy.solidcraft");
        h.app.run("file.copy", json!({"path":source,"destination":copy})).unwrap();
        wait(&mut h.app);
        assert_eq!(h.app.library.visible().len(), 2);
        let renamed = h.dir.join("Renamed.solidcraft");
        h.app.run("file.move", json!({"path":copy,"destination":renamed})).unwrap();
        wait(&mut h.app);
        assert!(h.app.library.visible().iter().any(|e| e.path == renamed));
        assert!(!h.app.library.visible().iter().any(|e| e.path == copy));
        assert_eq!(h.app.session.doc.to_json(), doc);
        assert!(h.app.library.pending.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn direct_partial_copy_error_refreshes_after_destination_was_created() {
        use std::os::unix::fs::PermissionsExt;
        let mut h = Harness::new();
        let source = h.dir.join("Original.solidcraft");
        let destination = h.dir.join("Partial.solidcraft");
        std::fs::write(&source, solidcraft_engine::io::write_design(&solidcraft_engine::doc::Document::new("Original"))).unwrap();
        h.app.library.set_root(h.dir.clone());
        wait(&mut h.app);
        let original_permissions = std::fs::metadata(&h.dir).unwrap().permissions();
        // Write+execute permits creating the destination, while opening the parent
        // for its post-persist directory sync fails without read permission.
        std::fs::set_permissions(&h.dir, std::fs::Permissions::from_mode(0o300)).unwrap();
        let result = h.app.run("file.copy", json!({"path":source,"destination":destination}));
        std::fs::set_permissions(&h.dir, original_permissions).unwrap();
        assert!(result.is_err());
        assert!(destination.exists(), "fixture must exercise failure after destination creation");
        assert!(source.exists());
        assert!(h.app.library.pending.is_some(), "partial command errors must refresh too");
        wait(&mut h.app);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_the_library() {
        let h = Harness::new();
        std::os::unix::fs::symlink(h.dir.parent().unwrap(), h.dir.join("outside")).unwrap();
        std::os::unix::fs::symlink("missing.solidcraft", h.dir.join("linked.solidcraft")).unwrap();
        assert!(read_folder(&h.dir, &h.dir).unwrap().entries.is_empty());
        assert!(read_folder(&h.dir, &h.dir.join("outside")).is_err());
    }

    #[test]
    fn picker_cancel_restart_and_navigation_preserve_the_current_design() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        let before = h.app.session.doc.to_json();
        let project = h.dir.join("Project A");
        std::fs::create_dir(&project).unwrap();
        let selected = h.dir.to_string_lossy().into_owned();
        folder_picker(&mut h, Some(selected));
        h.app.home.open = false;
        choose_folder(&mut h.app);
        assert!(h.app.file_picker_busy());
        crate::file_picker::tick(&mut h.app, &h.ctx);
        wait(&mut h.app);
        assert_eq!(h.app.library.root.as_ref(), Some(&h.dir));
        assert_eq!(h.app.session.doc.to_json(), before);
        h.frames(3);
        h.step(&json!({"click":{"handle":"library:entry:0"}})).unwrap();
        wait(&mut h.app);
        assert_eq!(h.app.library.current, project);
        assert!(!h.app.home.open);
        assert_eq!(h.app.session.doc.to_json(), before);
        go_home(&mut h.app);
        wait(&mut h.app);
        assert_eq!(h.app.library.current, h.dir);
        h.app.library.collapsed = true;
        h.app.library.set_highlight(&h.dir.join("labelled.solidcraft"), Some(Highlight::Green));
        h.app.home.forget("removed-from-home.solidcraft");
        let prefs = h.app.prefs();
        let mut restored = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        restored.load_prefs(&prefs);
        wait(&mut restored);
        assert_eq!(restored.library.root.as_ref(), Some(&h.dir));
        // Root restoration must preserve the saved collapsed setting.
        assert!(restored.library.collapsed);
        assert_eq!(restored.library.highlights, h.app.library.highlights);
        assert_eq!(restored.home.forgotten, h.app.home.forgotten);
        folder_picker(&mut h, None);
        choose_folder(&mut h.app);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert_eq!(h.app.library.root.as_ref(), Some(&h.dir));
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn stale_scan_search_refresh_and_removed_folder() {
        let mut h = Harness::new();
        let project = h.dir.join("Project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(h.dir.join("Old.solidcraft"), b"old").unwrap();
        std::fs::write(project.join("New.solidcraft"), b"new").unwrap();
        h.app.library.set_root(h.dir.clone());
        h.app.library.navigate(project.clone());
        wait(&mut h.app);
        assert_eq!(h.app.library.visible()[0].name, "New.solidcraft");
        h.app.library.search = "NEW".into();
        assert_eq!(h.app.library.visible().len(), 1);
        h.app.library.search = "absent".into();
        assert!(h.app.library.visible().is_empty());
        go_home(&mut h.app);
        wait(&mut h.app);
        assert!(h.app.library.search.is_empty());
        h.app.library.navigate(project.clone());
        wait(&mut h.app);
        std::fs::remove_dir_all(project).unwrap();
        h.app.library.refresh();
        wait(&mut h.app);
        assert!(h.app.library.error.is_some());
        assert!(h.app.library.listing.entries.is_empty());
        go_home(&mut h.app);
        wait(&mut h.app);
        assert!(h.app.library.error.is_none());
    }
    #[test]
    fn opening_a_library_design_retains_the_unsaved_tab_and_invalid_files_do_not_replace_it() {
        let mut h = Harness::new();
        let file = h.dir.join("Saved.solidcraft");
        let mut writer = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        writer.run("solid.box", json!({"length":4,"width":5,"height":6})).unwrap();
        writer.run("file.save_as", json!({"path":file.to_string_lossy()})).unwrap();
        h.app.run("solid.box", json!({"length":12,"width":13,"height":14})).unwrap();
        let unsaved = h.app.session.doc.to_json();
        let before = crate::documents::tabs_info(&h.app).len();
        open_entry(&mut h.app, Entry { path: file, name: "Saved.solidcraft".into(), folder: false });
        for _ in 0..1000 {
            crate::file_load::tick(&mut h.app, &h.ctx);
            if h.app.file_load.idle() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.app.file_load.idle());
        assert!(crate::documents::tabs_info(&h.app).len() > before);
        // The loaded tab is the saved design, and the dirty design remains switchable.
        let tabs = crate::documents::tabs_info(&h.app);
        let dirty = tabs.iter().position(|(_, dirty)| *dirty).unwrap();
        crate::documents::switch(&mut h.app, dirty);
        assert_eq!(h.app.session.doc.to_json(), unsaved);
        let bad = h.dir.join("Broken.solidcraft");
        std::fs::write(&bad, b"not a design").unwrap();
        open_entry(&mut h.app, Entry { path: bad, name: "Broken.solidcraft".into(), folder: false });
        for _ in 0..1000 {
            crate::file_load::tick(&mut h.app, &h.ctx);
            if h.app.file_load.idle() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.app.file_load.idle());
        assert_eq!(h.app.session.doc.to_json(), unsaved);
    }
    #[test]
    fn large_libraries_draw_only_visible_rows_and_the_panel_toggle_supports_keyboard() {
        let mut h = Harness::new();
        folder_picker(&mut h, None);
        h.app.library.root = Some(h.dir.clone());
        h.app.library.current = h.dir.clone();
        h.app.home.open = false;
        h.app.library.listing.entries = (0..10_000)
            .map(|i| {
                let name = format!("Design {i:05}.solidcraft");
                Entry { path: h.dir.join(&name), name, folder: false }
            })
            .collect();
        h.frames(3);
        assert!(h.widget("Design 00000.solidcraft").is_some());
        assert!(h.widget("Design 09999.solidcraft").is_none());
        h.step(&json!({"click":{"handle":"library:toggle"}})).unwrap();
        assert!(h.app.library.collapsed);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:panel").is_none());
        assert!(h.widget("Search documents").is_none());
        assert!(h.widget("Show documents").is_some());
        let collapsed_toggle = crate::scenario::rect_of("library:toggle").unwrap();
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(!h.app.library.collapsed);
        h.frames(3);
        assert!(crate::scenario::rect_of("library:panel").is_some());
        assert_eq!(crate::scenario::rect_of("library:toggle"), Some(collapsed_toggle));
        assert_eq!(h.app.library.root.as_ref(), Some(&h.dir));
        h.call("ui.resize", json!({"width":800,"height":600}));
        h.frames(3);
        assert!(h.app.viewport.rect.is_some());
    }
    #[cfg(unix)]
    #[test]
    fn an_explicitly_selected_symlink_root_keeps_breadcrumb_paths_consistent() {
        let mut h = Harness::new();
        let storage = h.dir.join("Storage");
        std::fs::create_dir_all(storage.join("Project")).unwrap();
        let alias = h.dir.join("Documents");
        std::os::unix::fs::symlink(&storage, &alias).unwrap();
        let listing = read_folder(&alias, &alias).unwrap();
        let project = listing.entries.first().unwrap().clone();
        assert_eq!(project.path, alias.join("Project"));
        h.app.library.set_root(alias.clone());
        open_entry(&mut h.app, project);
        wait(&mut h.app);
        assert_eq!(h.app.library.current.strip_prefix(&alias).unwrap(), Path::new("Project"));
        assert!(h.app.library.error.is_none());
    }
}
