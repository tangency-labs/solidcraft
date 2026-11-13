//! The UI kit's component gallery: a sidebar of every public component of `solidcraft-ui-kit`,
//! grouped by kind, and a page for each with a live example in its real states, what it is for,
//! how to call it and its do's and don'ts; a light/dark switch and a Theme panel that edits the
//! tokens every example draws with. Desktop (`cargo run -p ui-kit-gallery`) and browser
//! (docs.solidcraft.io/kit/).
//!
//! The gallery is SolidCraft's own reference for contributors, under the SolidCraft Brand and
//! Interface Licence like the kit it shows; it is not a library for other programs.

mod pages;
mod theme_panel;

use egui::{Rect, Ui, UiBuilder, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_ui_kit as kit;

pub use pages::{Group, PAGES, Page};
pub use theme_panel::ThemeEditor;

/// The kit's English messages, for the components that read their text through
/// `solidcraft_ui_text` (button names, empty states, tooltips).
static ENGLISH: solidcraft_ui_text::Catalog =
    solidcraft_ui_text::Catalog::new("en", "English", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/locales/en.json")));

/// Install what the kit draws with, once per process before the first frame: SolidCraft's look
/// (icon set, glyphs, built-in themes) and the English messages.
pub fn install_tables() {
    static LOCALES: [&solidcraft_ui_text::Catalog; 1] = [&ENGLISH];
    solidcraft_ui_text::install(&LOCALES);
    solidcraft_look::install_tables();
}

/// The editable values the examples draw, kept between frames so every example is live.
pub struct Demo {
    pub text: String,
    pub empty: String,
    pub long: String,
    pub password: String,
    pub number: f64,
    pub length: f64,
    pub angle: f64,
    pub whole: i32,
    pub vector: [f64; 3],
    pub count: String,
    pub fraction: f32,
    pub checked: bool,
    pub second: bool,
    pub third: bool,
    pub selected: usize,
    pub choice: usize,
    pub segment: usize,
    pub rgb: [u8; 3],
    pub swatch: usize,
    pub search: String,
    pub filter: String,
    pub rows: Vec<(String, String)>,
    pub sort: kit::Sort,
    pub lens: usize,
    pub open_menu: bool,
    pub rename: String,
    pub status: kit::StatusPresentation,
    pub last: String,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            text: "Bracket".into(),
            empty: String::new(),
            long: "A note that runs over more than one line,\nas a description does.".into(),
            password: "hunter2".into(),
            number: 0.5,
            length: 12.0,
            angle: 45.0,
            whole: 6,
            vector: [10.0, 0.0, 25.0],
            count: "6".into(),
            fraction: 0.4,
            checked: true,
            second: false,
            third: true,
            selected: 0,
            choice: 1,
            segment: 0,
            rgb: [0x4c, 0x8b, 0xf5],
            swatch: 1,
            search: String::new(),
            filter: String::new(),
            rows: vec![("Width".into(), "40 mm".into()), ("Height".into(), "Width / 2".into()), ("Thickness".into(), "3 mm".into())],
            sort: kit::Sort::Ascending,
            lens: 0,
            open_menu: false,
            rename: "Sketch1".into(),
            status: kit::StatusPresentation::default(),
            last: String::new(),
        }
    }
}

impl Demo {
    /// Remember what an example returned, shown under it so the reader sees the component's output.
    pub fn said(&mut self, what: impl Into<String>) {
        self.last = what.into();
    }
}

/// The gallery's state: the page shown, the sidebar's filter, the examples' values, the theme.
pub struct Gallery {
    pub page: usize,
    pub filter: String,
    pub demo: Demo,
    pub theme: ThemeEditor,
    pub theme_open: bool,
}

const TOP: f32 = 44.0;
const SIDEBAR: f32 = 240.0;
const THEME_WIDTH: f32 = 320.0;

impl Gallery {
    /// The gallery on `ctx`, with SolidCraft's interface faces installed into it.
    pub fn new(ctx: &egui::Context) -> Self {
        solidcraft_look::fonts::install(ctx);
        let page = page_from_location().unwrap_or(0);
        Self { page, filter: String::new(), demo: Demo::default(), theme: ThemeEditor::new(), theme_open: false }
    }

    /// Draw one frame of the gallery filling `ui`.
    pub fn show(&mut self, ui: &mut Ui) {
        // Every example draws with the edited tokens: the theme's base, then the edited palette
        // for that base (restored when the frame ends).
        solidcraft_theme::set_dark(self.theme.dark);
        let _tokens = solidcraft_theme::scoped_tokens(self.theme.tokens);
        self.theme.apply(ui.ctx());
        let t = Tokens::get();
        let whole = ui.max_rect();
        ui.painter().rect_filled(whole, 0.0, t.panel);
        let top = Rect::from_min_max(whole.min, pos2(whole.right(), whole.top() + TOP));
        let theme_w = if self.theme_open { THEME_WIDTH.min(whole.width() * 0.4) } else { 0.0 };
        let side = Rect::from_min_max(pos2(whole.left(), top.bottom()), pos2(whole.left() + SIDEBAR.min(whole.width() * 0.3), whole.bottom()));
        let theme = Rect::from_min_max(pos2(whole.right() - theme_w, top.bottom()), whole.max);
        let main = Rect::from_min_max(pos2(side.right(), top.bottom()), pos2(theme.left(), whole.bottom()));

        ui.painter().rect_filled(top, 0.0, t.app_bar);
        ui.scope_builder(UiBuilder::new().max_rect(top.shrink2(vec2(t.space_12, t.space_6))), |ui| self.top_bar(ui));
        ui.painter().rect_filled(side, 0.0, t.panel_header);
        ui.painter().vline(side.right(), side.y_range(), egui::Stroke::new(1.0, t.border));
        ui.scope_builder(UiBuilder::new().max_rect(side.shrink(t.space_8)), |ui| self.sidebar(ui));
        ui.scope_builder(UiBuilder::new().max_rect(main.shrink2(vec2(t.space_24, t.space_12))), |ui| self.page_view(ui));
        if self.theme_open {
            ui.painter().rect_filled(theme, 0.0, t.panel_header);
            ui.painter().vline(theme.left(), theme.y_range(), egui::Stroke::new(1.0, t.border));
            ui.scope_builder(UiBuilder::new().max_rect(theme.shrink(t.space_12)), |ui| {
                egui::ScrollArea::vertical().id_salt("theme_panel").auto_shrink([false, false]).show(ui, |ui| self.theme.panel(ui));
            });
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let t = Tokens::get();
        ui.horizontal_centered(|ui| {
            let mark = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover()).0;
            solidcraft_look::glyphs::brand_mark(ui.painter(), mark.center());
            ui.label(egui::RichText::new("SolidCraft UI kit").size(t.font_heading).strong().color(t.app_bar_text));
            ui.label(egui::RichText::new(format!("{} components", PAGES.len())).size(t.font_small).color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.theme_open { "Close theme" } else { "Theme…" };
                if kit::button(ui, label, if self.theme_open { kit::Button::SmallSelected } else { kit::Button::Small }).clicked() {
                    self.theme_open = !self.theme_open;
                }
                ui.add_space(t.space_8);
                let mut tone = usize::from(self.theme.dark);
                ui.scope(|ui| {
                    ui.set_width(150.0);
                    if kit::segmented(ui, &mut tone, &["Light", "Dark"], kit::Segments { dense: true, ..Default::default() }) {
                        self.theme.choose_tone(tone == 1);
                    }
                });
            });
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let t = Tokens::get();
        kit::search_field(ui, egui::Id::unique("gallery_filter"), &mut self.filter, "Find a component");
        ui.add_space(t.space_6);
        let filter = self.filter.to_lowercase();
        egui::ScrollArea::vertical().id_salt("sidebar").auto_shrink([false, false]).show(ui, |ui| {
            for group in Group::ALL {
                let shown: Vec<usize> =
                    PAGES.iter().enumerate().filter(|(_, p)| p.group == group && (filter.is_empty() || p.matches(&filter))).map(|(i, _)| i).collect();
                if shown.is_empty() {
                    continue;
                }
                ui.add_space(t.space_6);
                kit::section_heading(ui, group.title());
                for i in shown {
                    let row = kit::Row { compact: true, label: PAGES[i].title, selected: self.page == i, ..Default::default() };
                    if kit::tree_row(ui, egui::Id::unique(("gallery_page", PAGES[i].id)), &row).clicked {
                        self.page = i;
                        self.demo.last.clear();
                        set_location(PAGES[i].id);
                    }
                }
            }
        });
    }

    fn page_view(&mut self, ui: &mut Ui) {
        let Some(page) = PAGES.get(self.page) else { return };
        egui::ScrollArea::vertical().id_salt(("page", page.id)).auto_shrink([false, false]).show(ui, |ui| show_page(ui, page, &mut self.demo));
    }
}

/// One component's page: its title, what it is for, the live example, how to call it, its rules.
pub fn show_page(ui: &mut Ui, page: &Page, demo: &mut Demo) {
    let t = Tokens::get();
    ui.set_max_width(ui.available_width().min(860.0));
    ui.label(egui::RichText::new(page.group.title()).size(t.font_small).color(t.text_dim));
    ui.label(egui::RichText::new(page.title).size(t.font_page_heading).strong().color(t.text));
    ui.add_space(t.space_4);
    ui.label(egui::RichText::new(prose(page.what)).size(t.font_body).color(t.text));
    ui.add_space(t.space_12);
    heading(ui, "Live example");
    // The example's card: painted behind it once its size is known.
    let backing = ui.painter().add(egui::Shape::Noop);
    let inset = t.space_12;
    let outer = ui.available_rect_before_wrap();
    let inner = Rect::from_min_max(outer.min + vec2(inset, inset), pos2(outer.right() - inset, outer.bottom()));
    let drawn = ui
        .scope_builder(UiBuilder::new().max_rect(inner), |ui| {
            ui.set_width(inner.width());
            ui.push_id(page.id, |ui| (page.demo)(ui, demo));
            if !demo.last.is_empty() {
                ui.add_space(t.space_8);
                kit::text(ui, &format!("Returned: {}", demo.last), kit::Text::MonoNote("returned"));
            }
        })
        .response
        .rect;
    let card = Rect::from_min_max(outer.min, pos2(outer.right(), drawn.bottom() + inset));
    ui.painter()
        .set(backing, egui::epaint::RectShape::new(card, t.radius_md, t.dialog_bg, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside));
    ui.advance_cursor_after_rect(card);
    ui.add_space(t.space_12);
    heading(ui, "How to use it");
    kit::read_only_text(ui, page.usage.trim());
    ui.add_space(t.space_12);
    ui.columns(2, |columns| {
        rules(&mut columns[0], "Do", page.dos, t.agent_ok);
        rules(&mut columns[1], "Don't", page.donts, t.error);
    });
    ui.add_space(t.space_24);
}

fn heading(ui: &mut Ui, title: &str) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(title).size(t.font_subtitle).strong().color(t.text));
    ui.add_space(t.space_4);
}

fn rules(ui: &mut Ui, title: &str, items: &[&str], mark: egui::Color32) {
    let t = Tokens::get();
    heading(ui, title);
    for item in items {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(if title == "Do" { "✓" } else { "✗" }).color(mark).strong());
            ui.label(egui::RichText::new(prose(item)).size(t.font_body).color(t.text));
        });
    }
}

/// Page prose as shown: code names lose their Markdown backticks.
fn prose(text: &str) -> String {
    text.replace('`', "")
}

/// A labelled state of an example: "Default", "Disabled", "Error", "Empty".
pub fn state(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get();
    ui.label(egui::RichText::new(name).size(t.font_small).color(t.text_dim));
    ui.scope(add);
    ui.add_space(t.space_8);
}

/// [`state`] drawn disabled, as every kit control draws inside a disabled `Ui`.
pub fn disabled(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    state(ui, "Disabled", |ui| {
        ui.add_enabled_ui(false, add);
    });
}

/// The page named in the address (`#checkbox`) in the browser; none on the desktop.
fn page_from_location() -> Option<usize> {
    #[cfg(target_arch = "wasm32")]
    {
        let hash = web_sys::window().and_then(|w| w.location().hash().ok())?;
        let id = hash.trim_start_matches('#');
        PAGES.iter().position(|p| p.id == id)
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// Name the page in the address, so a link opens it.
fn set_location(_id: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(location) = web_sys::window().map(|w| w.location()) {
        let _ = location.set_hash(_id);
    }
}

#[cfg(test)]
#[path = "gallery_tests.rs"]
mod tests;
