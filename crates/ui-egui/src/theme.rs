//! Colours and widget style: a dark theme (the default, a blue-grey in the spirit of desktop CAD
//! tools, viewport #2e3440) and a light one, each with colour vision modes, and theme files that
//! set any of their colours (`theme_files`). Every colour the UI uses comes from [`Tokens`].

use std::cell::Cell;

#[path = "../../../assets/styles/ui/palettes.rs"]
mod palettes;

use egui::{Color32, Stroke};
use serde::{Deserialize, Serialize};

thread_local! {
    /// Theme state belongs to the UI thread drawing the frame. Concurrent app/test renderers
    /// must not change one another's palette while a frame or icon texture is being built.
    static DARK: Cell<bool> = const { Cell::new(true) };
    /// The colour vision mode, per thread. Each app
    /// sets it from its own preferences at the start of every frame (`SolveApp::logic`), on the
    /// thread that draws it. Tests run many apps side by side on their own threads in one process:
    /// with one global, a test in a mode changed the colours under another one
    /// (`hover_stays_off_during_spin_and_animated_views` failed that way). No worker thread reads
    /// it: the preview thread is handed its colours (`preview::Colors`) and thumbnails are drawn by
    /// the engine in the model's colours, so a new thread starts Off and stays Off.
    static VISION: Cell<ColorVision> = const { Cell::new(ColorVision::Off) };
    /// The palette of a theme file (`theme_files`) in place of the built-in one of its base
    /// (dark or light), per thread like the rest. `None`: the built-in palettes.
    static CUSTOM: Cell<Option<Custom>> = const { Cell::new(None) };
    /// The application bar's logo in the theme's accent instead of its own logo colours
    /// (Preferences › Interface › Logo), per thread like the rest.
    static LOGO_ACCENT: Cell<bool> = const { Cell::new(false) };
    /// The 3D view's own dark or light when Preferences give it one ([`ViewportTheme`], #4146),
    /// per thread like the rest. `None`: the view is as dark as the window.
    static VIEWPORT_DARK: Cell<Option<bool>> = const { Cell::new(None) };
    /// The 3D view's background colour when Preferences give it one (#7688), per thread like the
    /// rest. `None`: the theme's gradient.
    static VIEWPORT_BACKGROUND: Cell<Option<[u8; 3]>> = const { Cell::new(None) };
}

/// Three structural border levels plus independent content and control outlines.
/// Focus, selection, status, artwork and CAD geometry do not use these roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderRole {
    Primary,
    Secondary,
    Tertiary,
    Container,
    Control,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Borders {
    pub primary: Option<Stroke>,
    pub secondary: Option<Stroke>,
    /// Pattern for secondary section rules; solid preserves existing themes.
    pub secondary_dotted: bool,
    pub tertiary: Option<Stroke>,
    /// Solid outlines for nested previews, information wells and shortcut keycaps.
    pub container: Option<Stroke>,
    pub control: Option<Stroke>,
    pub primary_inset: Option<InsetBorder>,
    /// Optional directional chrome. Absent themes keep their flat strokes.
    pub bevel: Option<Bevel>,
}

/// Two directional edge bands, shared by raised chrome and recessed editors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bevel {
    pub width: f32,
    pub highlight: Color32,
    pub light: Color32,
    pub shadow: Color32,
    pub dark_shadow: Color32,
}

impl Bevel {
    /// Draw inside existing geometry; no extra input targets or layout allocation.
    pub fn paint(self, painter: &egui::Painter, rect: egui::Rect, radius: impl Into<egui::CornerRadius>, recessed: bool, enabled: bool) {
        if !rect.is_finite() || self.width <= 0.0 || !self.width.is_finite() || rect.width().min(rect.height()) <= self.width * 4.0 {
            return;
        }
        let radius = radius.into();
        let bands = if recessed {
            [(self.shadow, self.highlight), (self.dark_shadow, self.light)]
        } else {
            [(self.highlight, self.dark_shadow), (self.light, self.shadow)]
        };
        for (index, (upper, lower)) in bands.into_iter().enumerate() {
            let inset = (index as f32 + 0.5) * self.width;
            let edge = rect.shrink(inset);
            let mut rounding: egui::epaint::CornerRadiusF32 = radius.into();
            rounding -= inset;
            let mut path = Vec::new();
            egui::epaint::tessellator::path::rounded_rectangle(&mut path, edge, rounding);
            if let Some(first) = path.first().copied() {
                path.push(first);
            }
            for pair in path.windows(2) {
                if let Some((a, b)) = pair.first().zip(pair.get(1)) {
                    let middle = *a + (*b - *a) * 0.5;
                    let relative = (middle - edge.center()) / edge.size();
                    let color = if relative.x + relative.y < 0.0 { upper } else { lower };
                    let color = if enabled { color } else { color.gamma_multiply(0.45) };
                    painter.line_segment([*a, *b], Stroke::new(self.width, color));
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InsetBorder {
    pub stroke: Stroke,
    /// Clear distance from the inner edge of the primary stroke, in logical points.
    pub gap: f32,
}

impl Borders {
    pub const LEGACY: Self = Self {
        primary: None,
        secondary: None,
        secondary_dotted: false,
        tertiary: None,
        container: None,
        control: None,
        primary_inset: None,
        bevel: None,
    };

    pub fn stroke(self, role: BorderRole, fallback: Stroke) -> Stroke {
        match role {
            BorderRole::Primary => self.primary,
            BorderRole::Secondary => self.secondary,
            BorderRole::Tertiary => self.tertiary,
            BorderRole::Container => self.container,
            BorderRole::Control => self.control,
        }
        .unwrap_or(fallback)
    }
}

impl Tokens {
    /// Bevel themes use the editable surface colour independently of button faces.
    pub fn input_fill(&self) -> Color32 {
        if self.borders.bevel.is_some() { self.field } else { self.control_fill }
    }

    pub fn input_bevel(&self, ui: &egui::Ui, response: &egui::Response) {
        if let Some(bevel) = self.borders.bevel {
            bevel.paint(ui.painter(), response.rect.shrink(1.0), self.control_radius, true, response.enabled());
        }
    }

    pub fn button_bevel(&self, ui: &egui::Ui, response: &egui::Response, selected: bool) {
        self.button_bevel_with_radius(ui, response, selected, self.control_radius);
    }

    /// Compound controls supply the same corner geometry as their selected fill.
    pub fn button_bevel_with_radius(&self, ui: &egui::Ui, response: &egui::Response, selected: bool, radius: impl Into<egui::CornerRadius>) {
        if let Some(bevel) = self.borders.bevel {
            let pressed = response.enabled()
                && (response.is_pointer_button_down_on()
                    || (response.has_focus() && ui.input(|i| i.key_down(egui::Key::Space) || i.key_down(egui::Key::Enter))));
            bevel.paint(ui.painter(), response.rect, radius, selected || pressed, response.enabled());
        }
    }

    pub fn primary_border(&self, color: Color32) -> Stroke {
        self.borders.stroke(BorderRole::Primary, Stroke::new(1.0, color))
    }
    pub fn secondary_border(&self, color: Color32) -> Stroke {
        self.borders.stroke(BorderRole::Secondary, Stroke::new(1.0, color))
    }
    /// Secondary rules share a pattern as well as a stroke. Dot count is bounded for tiny widths.
    fn secondary_dots(&self, painter: &egui::Painter, path: &[egui::Pos2], stroke: Stroke) {
        if stroke.width <= 0.0 || path.iter().any(|p| !p.is_finite()) {
            return;
        }
        let length: f32 = path.windows(2).map(|pair| pair.first().zip(pair.get(1)).map_or(0.0, |(a, b)| a.distance(*b))).sum();
        if !length.is_finite() {
            return;
        }
        let spacing = (stroke.width * 3.0).max(3.0).max(length / 4096.0);
        painter.extend(egui::Shape::dotted_line(path, stroke.color, spacing, stroke.width * 0.5));
    }

    pub fn secondary_line(&self, painter: &egui::Painter, points: [egui::Pos2; 2], color: Color32) {
        let stroke = self.secondary_border(color);
        if self.borders.secondary_dotted {
            self.secondary_dots(painter, &points, stroke);
        } else {
            painter.line_segment(points, stroke);
        }
    }

    pub fn secondary_hline(&self, painter: &egui::Painter, range: impl Into<egui::emath::Rangef>, y: f32, color: Color32) {
        let range = range.into();
        self.secondary_line(painter, [egui::pos2(range.min, y), egui::pos2(range.max, y)], color);
    }

    pub fn secondary_vline(&self, painter: &egui::Painter, x: f32, range: impl Into<egui::emath::Rangef>, color: Color32) {
        let range = range.into();
        self.secondary_line(painter, [egui::pos2(x, range.min), egui::pos2(x, range.max)], color);
    }

    /// Enclosures stay solid independently of the section-divider pattern.
    pub fn container_rect(&self, painter: &egui::Painter, rect: egui::Rect, radius: impl Into<egui::CornerRadius>, fill: Color32, color: Color32) {
        let radius = radius.into();
        painter.rect(rect, radius, fill, self.container_border(color), egui::StrokeKind::Inside);
        if let Some(bevel) = self.borders.bevel {
            bevel.paint(painter, rect, radius, false, true);
        }
    }

    pub fn container_border(&self, color: Color32) -> Stroke {
        self.borders.stroke(BorderRole::Container, Stroke::new(1.0, color))
    }

    pub fn tertiary_border(&self, color: Color32) -> Stroke {
        self.borders.stroke(BorderRole::Tertiary, Stroke::new(1.0, color))
    }
    pub fn control_border_stroke(&self, color: Color32) -> Stroke {
        self.borders.stroke(BorderRole::Control, Stroke::new(1.0, color))
    }
}

/// A theme file's palette and whether it is dark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Custom {
    pub dark: bool,
    pub tokens: Tokens,
}

/// Use a theme file's palette for its base, or the built-in palettes (`None`). Returns whether
/// that changed anything (the visuals follow on the next [`apply`]).
pub fn set_custom(custom: Option<Custom>) -> bool {
    CUSTOM.with(|c| c.replace(custom) != custom)
}

/// Draw the logo in the theme's accent (`true`) or its own logo colours. Returns whether that
/// changed anything (the visuals follow on the next [`apply`]).
pub fn set_logo_accent(on: bool) -> bool {
    LOGO_ACCENT.with(|c| c.replace(on) != on)
}

/// Isolated widget tests restore the thread's theme even when an assertion fails.
#[cfg(test)]
pub(crate) fn scoped_tokens(tokens: Tokens) -> impl Drop {
    struct Restore(Option<Custom>);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_custom(self.0);
        }
    }
    let restore = Restore(CUSTOM.get());
    set_custom(Some(Custom { dark: is_dark(), tokens }));
    restore
}

/// A tab's selection colours. This is deliberately separate from the global widget style:
/// application chrome, checkboxes, status colours and dialog actions retain their own tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabAccent {
    pub color: Color32,
    pub ink: Color32,
    pub soft: Color32,
}

/// Palette ramp for icons on a surface explicitly accented by its active tab.
pub(crate) fn icon_accent_for_tab(tab: &str) -> Option<&'static str> {
    match tab {
        "SKETCH" => Some("sketch"),
        "SURFACE" => Some("surface"),
        "MESH" => Some("mesh"),
        "FORM" => Some("form"),
        _ => None,
    }
}

/// WCAG contrast of two opaque sRGB colours.
pub(crate) fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let luminance = |c: Color32| {
        let linear = solidcraft_engine::geom::srgb::decode8;
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

impl TabAccent {
    pub fn for_tab(t: &Tokens, tab: &str) -> Self {
        let (color, ink, soft) = match tab {
            "SKETCH" => (t.sketch_accent, t.sketch_ink, t.sketch_soft),
            "SURFACE" => (t.surface_fill, t.surface_ink, t.surface_soft),
            "MESH" => (t.mesh_fill, t.mesh_ink, t.mesh_soft),
            "FORM" => (t.form_fill, t.form_tab_ink, t.form_soft),
            "SHEET METAL" => (t.sheet_fill, t.sheet_ink, t.sheet_soft),
            "RENDER" | "RENDER APPEARANCE" | "RENDER SCENE" | "RENDER CAMERA" => (t.render_fill, t.render_ink, t.render_soft),
            // Model and shared utility tabs retain their existing selection exactly.
            _ => (t.accent, t.selected_ink, t.accent_soft),
        };
        let ink = if contrast_ratio(ink, soft) >= 4.5 {
            ink
        } else if contrast_ratio(color, soft) >= 4.5 {
            color
        } else if contrast_ratio(t.text, soft) >= 4.5 {
            t.text
        } else if contrast_ratio(Color32::BLACK, soft) >= contrast_ratio(Color32::WHITE, soft) {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        Self { color, ink, soft }
    }

    /// Small text on a tab's solid pill. Brighter fills need dark ink in either theme.
    pub fn on_fill(self, t: &Tokens) -> Color32 {
        if contrast_ratio(t.on_accent, self.color) >= 4.5 {
            t.on_accent
        } else if contrast_ratio(t.tab_on_fill_dark, self.color) >= 4.5 {
            t.tab_on_fill_dark
        } else if contrast_ratio(Color32::BLACK, self.color) >= contrast_ratio(Color32::WHITE, self.color) {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    }

    pub fn current(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Self>(egui::Id::new("sc_tab_accent"))).unwrap_or_else(|| Self::for_tab(&Tokens::get(), "MODEL"))
    }

    pub fn current_icon_accent(ctx: &egui::Context) -> Option<&'static str> {
        ctx.data(|d| d.get_temp::<Option<&'static str>>(egui::Id::new("sc_tab_icon_accent"))).flatten()
    }

    pub fn publish(app: &crate::SolveApp, ctx: &egui::Context) {
        let accent = Self::for_tab(&Tokens::get(), &app.ui.tab);
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("sc_tab_accent"), accent);
            d.insert_temp(egui::Id::new("sc_tab_icon_accent"), icon_accent_for_tab(&app.ui.tab));
            d.insert_temp(egui::Id::new("sc_sketch_open"), app.session.active_sketch.is_some());
            d.insert_temp(egui::Id::new("sc_tool_accent"), app.tool.is_some() || app.dialog.is_some());
        });
    }

    /// Paint only the focused numeric editor, without changing sibling buttons or checkboxes.
    pub fn number_focus(ui: &egui::Ui, response: &egui::Response) {
        let active = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("sc_tool_accent"))).unwrap_or(false);
        let t = Tokens::get();
        let sketch = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("sc_sketch_open"))).unwrap_or(false);
        let color = if sketch { t.sketch_accent } else { Self::current(ui.ctx()).color };
        if active && response.has_focus() && color != t.accent {
            ui.painter().rect_stroke(response.rect, t.radius_sm, Stroke::new(1.0, color), egui::StrokeKind::Inside);
        }
    }
}

/// Is the dark theme on?
pub fn is_dark() -> bool {
    DARK.get()
}

/// Is the 3D view dark? As the window, unless Preferences give the view its own (#4146).
pub fn viewport_dark() -> bool {
    VIEWPORT_DARK.get().unwrap_or_else(is_dark)
}

/// The 3D view's own background colour, or `None` for the theme's gradient (#7688).
pub fn viewport_background() -> Option<[u8; 3]> {
    VIEWPORT_BACKGROUND.get()
}

/// The common conversion boundary for compiled icon palette data.
pub(crate) const fn icon_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Colour vision modes (#778): a second set of values for the colours that carry a meaning, for
/// a weak or missing red (protan), green (deutan) or blue (tritan) sense. Not a filter: Off gives
/// back the theme's colours unchanged. Protan and Deutan share one red-green table, measured
/// clear for both (`colour_vision_tests`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorVision {
    #[default]
    Off,
    Protan,
    Deutan,
    Tritan,
}

impl ColorVision {
    pub const ALL: [ColorVision; 4] = [ColorVision::Off, ColorVision::Protan, ColorVision::Deutan, ColorVision::Tritan];

    /// The saved value (`ui.prefs` `color_vision`).
    pub fn id(self) -> &'static str {
        match self {
            ColorVision::Off => "off",
            ColorVision::Protan => "protan",
            ColorVision::Deutan => "deutan",
            ColorVision::Tritan => "tritan",
        }
    }

    pub fn from_id(id: &str) -> Option<ColorVision> {
        ColorVision::ALL.into_iter().find(|m| m.id() == id)
    }

    /// Its name in the Preferences (a message id).
    pub fn label_id(self) -> &'static str {
        match self {
            ColorVision::Off => "ui.colour_vision_off",
            ColorVision::Protan => "ui.colour_vision_protan",
            ColorVision::Deutan => "ui.colour_vision_deutan",
            ColorVision::Tritan => "ui.colour_vision_tritan",
        }
    }

    /// Who it is for, in one line (its tooltip; a message id).
    pub fn help_id(self) -> &'static str {
        match self {
            ColorVision::Off => "ui.colour_vision_off_tip",
            ColorVision::Protan => "ui.colour_vision_protan_tip",
            ColorVision::Deutan => "ui.colour_vision_deutan_tip",
            ColorVision::Tritan => "ui.colour_vision_tritan_tip",
        }
    }
}

/// From saved preferences: a value this version doesn't know (a later version's mode, a hand
/// edit) is Off rather than an error, which would lose every other saved preference with it.
/// `ui.prefs` refuses such a value before it gets here (`prefs::set`).
impl<'de> Deserialize<'de> for ColorVision {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str().and_then(ColorVision::from_id).unwrap_or_default())
    }
}

/// The 3D view's colours (#4146): the theme's own, or the built-in Light or Dark theme's whatever
/// the window uses, so a dark window can show the model on a light view and the other way round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportTheme {
    #[default]
    Theme,
    Light,
    Dark,
}

impl ViewportTheme {
    pub const ALL: [ViewportTheme; 3] = [ViewportTheme::Theme, ViewportTheme::Light, ViewportTheme::Dark];

    /// The saved value (`ui.prefs` `viewport_theme`).
    pub fn id(self) -> &'static str {
        match self {
            ViewportTheme::Theme => "theme",
            ViewportTheme::Light => "light",
            ViewportTheme::Dark => "dark",
        }
    }

    pub fn from_id(id: &str) -> Option<ViewportTheme> {
        ViewportTheme::ALL.into_iter().find(|m| m.id() == id)
    }

    /// Its name in the Preferences (a message id).
    pub fn label_id(self) -> &'static str {
        match self {
            ViewportTheme::Theme => "ui.viewport_theme_follow",
            ViewportTheme::Light => "ui.light",
            ViewportTheme::Dark => "ui.dark",
        }
    }

    /// The view's own dark or light, or `None` to draw it as the window ([`set_viewport_dark`]).
    pub fn dark(self) -> Option<bool> {
        match self {
            ViewportTheme::Theme => None,
            ViewportTheme::Light => Some(false),
            ViewportTheme::Dark => Some(true),
        }
    }
}

/// From saved preferences: an unknown value is Theme, as [`ColorVision`]'s is Off.
impl<'de> Deserialize<'de> for ViewportTheme {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str().and_then(ViewportTheme::from_id).unwrap_or_default())
    }
}

/// The colour vision mode on now (on this thread).
pub fn vision() -> ColorVision {
    VISION.with(Cell::get)
}

/// Switch the colour vision mode (the visuals follow on the next [`apply`]).
pub fn set_vision(mode: ColorVision) {
    VISION.with(|v| v.set(mode));
}

/// An sRGB colour carried by model data, including file colours and point-cloud shading.
pub(crate) const fn model_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// An sRGB colour given as 0..1 (the analysis colours), for drawing in the UI.
pub fn srgb(c: [f32; 3]) -> Color32 {
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(byte(c[0]), byte(c[1]), byte(c[2]))
}

/// Surface analysis colours, straight sRGB 0..1 as the shader takes them (exact, so they read
/// the same as when they were literals in the WGSL). The light theme's are the headless default.
pub use solidcraft_engine::render::AnalysisColors;

/// Colours for native viewport markers and the agent pointer. Command artwork uses the SVG palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconColors {
    pub highlight: Color32,
    pub agent: Color32,
    pub marker_halo: Color32,
}

/// Every colour and size the UI draws with. The colours drawn in the 3D view are listed again in
/// [`Tokens::with_viewport`], which takes them from the other side when the 3D view is set to the
/// other one's colours (#4146): a new colour drawn in the view goes in both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// Navigation chrome from the approved Navbar boards.
    pub nav_hover: Color32,
    pub nav_press: Color32,
    pub nav_dim: Color32,
    pub nav_viewport: Color32,
    pub nav_tip: Color32,
    pub nav_tip_ink: Color32,
    pub nav_tip_key_border: Color32,
    pub nav_field: Color32,

    /// Optional structural/control strokes. None retains each surface’s legacy styling.
    pub borders: Borders,
    /// File/start presentation, with explicit foreground/background pairs.
    pub file_dim: Color32,
    pub file_recovery_well: Color32,
    pub file_action_height: f32,
    pub file_back_height: f32,
    pub file_recovery_icon: f32,
    pub file_grid_gap: f32,
    pub file_background: Color32,
    pub file_surface: Color32,
    pub file_border: Color32,
    /// Neutral multiplication tint for File-view model thumbnails.
    pub file_image_tint: Color32,
    pub file_rail: Color32,
    pub file_rail_ink: Color32,
    pub file_rail_hover: Color32,
    pub file_primary: Color32,
    pub file_primary_ink: Color32,
    pub file_primary_border: Color32,
    pub file_recovery: Color32,
    pub file_recovery_ink: Color32,
    pub file_recovery_border: Color32,
    pub file_nav_width: f32,
    pub file_nav_height: f32,
    pub file_tile_height: f32,
    pub file_tile_icon: f32,
    pub file_recent_height: f32,
    pub file_sample_image_height: f32,
    pub file_content_inset_x: f32,
    pub file_content_inset_y: f32,
    pub file_section_gap: f32,
    pub font_start_title: f32,
    // Shared dimensions in logical pixels; identical in both themes.
    pub space_2: f32,
    pub space_3: f32,
    pub space_4: f32,
    pub space_6: f32,
    pub space_8: f32,
    pub space_10: f32,
    pub space_12: f32,
    pub space_14: f32,
    pub space_16: f32,
    pub space_24: f32,
    pub radius_sm: u8,
    pub radius_md: u8,
    pub radius_lg: u8,
    pub radius_xl: u8,
    pub font_badge: f32,
    pub font_micro: f32,
    pub font_panel: f32,
    pub font_small: f32,
    pub font_caption: f32,
    pub font_body: f32,
    pub font_label: f32,
    pub font_button: f32,
    pub font_heading: f32,
    /// Tiny world-axis triad letters.
    pub font_triad: f32,
    /// Bold letters on the navigation compass discs.
    pub font_compass: f32,
    /// Home section titles, home actions and update titles.
    pub font_subtitle: f32,
    /// Introductory text on the home page.
    pub font_intro: f32,
    /// Top-level headings in assistant Markdown.
    pub font_article_heading: f32,
    /// Application title in the About dialog.
    pub font_title: f32,
    /// Application name on the home page.
    pub font_display: f32,
    /// Height of dense form controls and rows.
    pub control_height: f32,
    /// Card text, hint, primary and validation colours.
    pub form_ink: Color32,
    pub form_dim: Color32,
    pub form_primary: Color32,
    /// Primary action fill; theme files without this token inherit `form_primary`.
    pub form_primary_fill: Color32,
    pub form_error: Color32,
    pub form_error_fill: Color32,
    /// Successful validation text and its badge surface.
    pub form_success: Color32,
    pub form_success_fill: Color32,
    /// Corner radius of dense form controls and actions.
    pub control_radius: u8,
    /// Width of the dense form label column.
    pub form_label_width: f32,
    /// Vertical gap between dense form rows.
    pub form_gap_y: f32,
    /// Height of the full-width dense form footer.
    pub form_footer_height: f32,
    /// Height of the dense form header.
    pub form_header_height: f32,
    /// Outer width of a dense feature card.
    pub form_card_width: f32,
    /// Size of the icon in a dense form header.
    pub form_icon_size: f32,
    /// Size of the dense form close button.
    pub form_close_size: f32,
    /// Text inset in controls and gap after the header icon.
    pub form_inset: f32,
    /// Inset of artwork inside a dense form icon button.
    pub form_icon_inset: f32,
    /// Size of the section disclosure chevron.
    pub form_section_icon_size: f32,
    /// Height of a dense form checkbox row.
    pub form_checkbox_height: f32,
    /// Minimum height of a dense form footer action.
    pub form_button_height: f32,
    /// Outer width of the sketch options card.
    pub sketch_palette_width: f32,
    /// Square creation-mode button size and radius.
    pub sketch_mode_size: f32,
    pub sketch_mode_radius: u8,
    pub sketch_mode_border: f32,
    /// Opacity of the sketch card and its selected accent tint.
    pub sketch_palette_alpha: f32,
    pub sketch_mode_tint: f32,
    /// Footer navigation icon size.
    pub sketch_footer_icon_size: f32,
    /// Extra section heading letter spacing as a fraction of font size.
    pub font_section_tracking: f32,
    pub app_bar: Color32,
    /// Dividers between the application bar groups.
    pub app_bar_separator: Color32,
    pub toolbar: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub sketch_accent: Color32,
    pub surface_accent: Color32,
    pub mesh_accent: Color32,
    pub form_accent: Color32,
    pub render_accent: Color32,
    /// Per-tab fills, small-text inks and selected-row tints. Sketch reuses sketch_accent.
    /// The other fill keys preserve the legacy *_accent values used by other surfaces.
    pub sketch_ink: Color32,
    pub sketch_soft: Color32,
    pub surface_fill: Color32,
    pub surface_ink: Color32,
    pub surface_soft: Color32,
    pub mesh_fill: Color32,
    pub mesh_ink: Color32,
    pub mesh_soft: Color32,
    pub form_fill: Color32,
    pub form_tab_ink: Color32,
    pub form_soft: Color32,
    pub sheet_fill: Color32,
    pub sheet_ink: Color32,
    pub sheet_soft: Color32,
    pub render_fill: Color32,
    pub render_ink: Color32,
    pub render_soft: Color32,
    /// Dark text for tab pills whose fill has insufficient contrast with on_accent.
    pub tab_on_fill_dark: Color32,
    /// Ink on accent-filled buttons and selection chips.
    pub on_accent: Color32,
    /// Agent status on the dark application bar, in either theme.
    pub agent_ok: Color32,
    pub agent_busy: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub icon: Color32,
    pub timeline: Color32,
    pub hover: Color32,
    // ---- viewport selection feedback ----
    /// Selected face (flat fill).
    pub sel_face: Color32,
    /// Selected edge core and its lighter rim.
    pub sel_edge: Color32,
    pub sel_edge_rim: Color32,
    /// Hovered edge: dark core with a light halo.
    pub hover_edge: Color32,
    pub hover_edge_halo: Color32,
    /// How much lighter a hovered face is (grey levels).
    pub hover_face_lift: u8,
    /// Selected profile fill and outline.
    pub sel_profile: Color32,
    pub sel_profile_edge: Color32,
    /// Hovered profile: faint tint and a bright outline.
    pub hover_profile: Color32,
    pub hover_profile_edge: Color32,
    /// Selected vertex dot.
    pub sel_vertex: Color32,
    // ---- origin ----
    pub origin_plane: Color32,
    pub origin_plane_edge: Color32,
    pub origin_plane_hover: Color32,
    pub origin_point: Color32,
    /// The X, Y and Z colours, one set for every axis drawn (#976): the origin axes, the view
    /// triad, the navigation cube's triad, the Move gizmo, a 3D sketch point's triad, joint
    /// origins, measured distances along the axes and the headless picture's origin axes.
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub axis_z: Color32,
    /// Construction planes.
    pub construction_plane: Color32,
    /// Construction axes and points.
    pub construction_geom: Color32,
    // ---- bodies and live previews ----
    pub body: Color32,
    pub body_edge: Color32,
    /// Command manipulators (direction arrows, handles).
    pub manipulator: Color32,
    /// Allowed assembly motion, separate from the selected face's blue.
    pub joint_manipulator: Color32,
    /// Assembly Move axis and centre, distinct from its draggable amber arrows.
    pub joint_axis: Color32,
    /// New surface in a feature preview.
    pub preview_add: Color32,
    /// The surface of a body a feature preview creates (New Body), apart from a join (#1805).
    pub preview_new: Color32,
    /// Proposed sketch geometry, distinct from blue selections.
    pub preview_sketch: Color32,
    /// Removed material in a feature preview (translucent).
    pub preview_cut: Color32,
    /// Navigation cube faces, outline and labels.
    pub cube_face: Color32,
    pub cube_border: Color32,
    pub cube_text: Color32,
    /// Light blue hover fill, readable on the pale cube in either theme.
    pub cube_hover: Color32,
    // ---- viewport ----
    /// Viewport background (top and bottom of the gradient; equal for a flat fill).
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
    pub grid: Color32,
    pub grid_major: Color32,
    /// A whole selected body.
    pub body_selected: Color32,
    /// Sketch curves: under-constrained, fully constrained (and finished sketches).
    pub sketch: Color32,
    pub sketch_fixed: Color32,
    /// Wider rim beneath sketch curves to separate them from shaded faces.
    pub sketch_halo: Color32,
    /// Fixed (locked) sketch geometry.
    pub sketch_locked: Color32,
    /// The open sketch's curves and points that a failing constraint holds, and that
    /// constraint's glyph or dimension, while the constraints conflict (#7244). The error red by
    /// default; a theme file that sets `error` but not this key takes its `error`.
    pub sketch_conflict: Color32,
    /// Sketch dimensions: driving, driven (reference). Each theme gives them a colour of their
    /// own, apart from the sketch curves they measure (#7861).
    pub dimension: Color32,
    pub dimension_driven: Color32,
    /// Sketch points: fully constrained, free.
    pub sketch_point: Color32,
    pub sketch_point_free: Color32,
    /// Command dialogs: panel, border, and the filled selection chip.
    pub dialog_bg: Color32,
    pub dialog_border: Color32,
    pub chip: Color32,
    /// Inside of bodies cut by a section.
    pub section_cap: Color32,
    /// Closed sketch profiles (translucent fill).
    pub profile_fill: Color32,
    /// Constraint glyph boxes.
    pub glyph_bg: Color32,
    pub glyph_edge: Color32,
    pub glyph_text: Color32,
    /// Chips drawn over the viewport (status, navigation bar, dimension labels).
    pub overlay: Color32,
    /// Rubber band of the shape being drawn.
    pub rubber_band: Color32,
    /// Light text on the application bar.
    pub app_bar_text: Color32,
    /// A hovered tab on the application bar (keeps `app_bar_text` readable).
    pub app_bar_hover: Color32,
    pub app_bar_hover_subtle: Color32,
    pub app_bar_hover_button: Color32,
    pub app_bar_hover_strong: Color32,
    /// The active document tab: a solid surface lifted from the bar.
    pub app_bar_tab_active: Color32,
    /// Dense feature dialogs: the body surface, the footer band, the dividers, input controls
    /// (fill and border), field labels, and the selected-element foreground and background.
    pub dialog_body: Color32,
    pub dialog_footer: Color32,
    pub dialog_rule: Color32,
    pub control_fill: Color32,
    pub control_border: Color32,
    pub dialog_label: Color32,
    pub selected_ink: Color32,
    pub selected_bg: Color32,
    /// Timeline items: normal and failed.
    pub timeline_item: Color32,
    pub timeline_item_error: Color32,
    pub timeline_marker: Color32,
    /// Button and field backgrounds on panels.
    pub field: Color32,
    // ---- box selection ----
    pub box_window: Color32,
    pub box_crossing: Color32,
    // ---- colours that came from the engine or were drawn in place ----
    /// Sketch curves the engine tags (`view::colors`): projected, lost link, construction,
    /// centerline.
    pub sketch_projected: Color32,
    pub sketch_lost: Color32,
    pub sketch_construction: Color32,
    pub sketch_centerline: Color32,
    /// Sheet metal flat pattern: bend up, bend down.
    pub bend_up: Color32,
    pub bend_down: Color32,
    /// Interference: the clash fill, before its shading.
    pub interference: Color32,
    /// Continuity Analysis seams, best to worst as `inspect.continuity` judges them: smooth (G2),
    /// tangent with a curvature jump (G1), a crease (G0) and a gap. Bluish green, blue, yellow
    /// and reddish purple stay apart in every colour vision mode, so no mode changes them.
    pub continuity: [Color32; 4],
    pub analysis: AnalysisColors,
    pub icons: IconColors,
    /// Optional SVG display roles and semantic ramps from the active theme file.
    pub icon_palette: Option<crate::icon_svg::Palette>,
    /// The SolidCraft mark on the application bar: light, mid and dark faces (the gaps between
    /// them show the bar).
    pub logo: [Color32; 3],
}

impl Tokens {
    /// An expanded outline follows its fill, including a theme's deliberately square corners.
    pub(crate) fn outset_radius(radius: f32, outset: f32) -> f32 {
        if radius > 0.0 { radius + outset } else { 0.0 }
    }

    /// Subtle shell elevation, derived from the semantic application-bar colour.
    pub(crate) fn shell_shadow(self) -> egui::Shadow {
        egui::Shadow { offset: [0, self.space_2 as i8], blur: self.space_8 as u8, spread: 0, color: self.app_bar.gamma_multiply(0.18) }
    }

    /// The X, Y and Z axis colours, in that order.
    pub const fn axes(&self) -> [Color32; 3] {
        [self.axis_x, self.axis_y, self.axis_z]
    }

    /// The tokens of the current theme and colour vision mode, the colours drawn in the 3D view
    /// from the view's own dark or light ([`viewport_dark`]).
    pub fn get() -> Tokens {
        Tokens::get_in(vision())
    }

    /// [`Tokens::get`] in another colour vision mode: the Colour vision preference previews the
    /// mode being chosen before it is applied (#7152).
    pub fn get_in(mode: ColorVision) -> Tokens {
        let window = Tokens::themed(is_dark(), mode);
        let tokens = if viewport_dark() == is_dark() { window } else { window.with_viewport(Tokens::themed(viewport_dark(), mode)) };
        tokens.with_background(viewport_background())
    }

    /// The tokens of the 3D view's side (#4146), for a window colour drawn straight onto the view
    /// with nothing behind it (a status line, a label or a warning beside the geometry), so it
    /// reads on the view's background. As [`Tokens::get`] unless the view has its own side.
    pub fn view() -> Tokens {
        Tokens::themed(viewport_dark(), vision()).with_background(viewport_background())
    }

    /// These tokens with the 3D view's background one flat colour (#7688), or as they are for `None`.
    pub fn with_background(mut self, colour: Option<[u8; 3]>) -> Tokens {
        if let Some([r, g, b]) = colour {
            self.viewport_top = Color32::from_rgb(r, g, b);
            self.viewport_bottom = self.viewport_top;
        }
        self
    }

    /// These tokens with the colours drawn in the 3D view taken from `view`: the background and
    /// grid, bodies, selection and hover, the origin and construction geometry, previews, sketches
    /// and their dimensions and glyphs, the navigation cube and triads, box selection and the
    /// analysis colours. The window keeps its own: panels, dialogs, menus, and the chips and cards
    /// over the view (`overlay` with its text, icons and borders).
    pub fn with_viewport(self, view: Tokens) -> Tokens {
        Tokens {
            sel_face: view.sel_face,
            sel_edge: view.sel_edge,
            sel_edge_rim: view.sel_edge_rim,
            hover_edge: view.hover_edge,
            hover_edge_halo: view.hover_edge_halo,
            hover_face_lift: view.hover_face_lift,
            sel_profile: view.sel_profile,
            sel_profile_edge: view.sel_profile_edge,
            hover_profile: view.hover_profile,
            hover_profile_edge: view.hover_profile_edge,
            sel_vertex: view.sel_vertex,
            origin_plane: view.origin_plane,
            origin_plane_edge: view.origin_plane_edge,
            origin_plane_hover: view.origin_plane_hover,
            origin_point: view.origin_point,
            axis_x: view.axis_x,
            axis_y: view.axis_y,
            axis_z: view.axis_z,
            construction_plane: view.construction_plane,
            construction_geom: view.construction_geom,
            body: view.body,
            body_edge: view.body_edge,
            manipulator: view.manipulator,
            joint_manipulator: view.joint_manipulator,
            joint_axis: view.joint_axis,
            preview_add: view.preview_add,
            preview_new: view.preview_new,
            preview_sketch: view.preview_sketch,
            preview_cut: view.preview_cut,
            cube_face: view.cube_face,
            cube_border: view.cube_border,
            cube_text: view.cube_text,
            cube_hover: view.cube_hover,
            viewport_top: view.viewport_top,
            viewport_bottom: view.viewport_bottom,
            grid: view.grid,
            grid_major: view.grid_major,
            body_selected: view.body_selected,
            sketch: view.sketch,
            sketch_fixed: view.sketch_fixed,
            sketch_locked: view.sketch_locked,
            sketch_conflict: view.sketch_conflict,
            dimension: view.dimension,
            dimension_driven: view.dimension_driven,
            sketch_point: view.sketch_point,
            sketch_point_free: view.sketch_point_free,
            section_cap: view.section_cap,
            profile_fill: view.profile_fill,
            glyph_bg: view.glyph_bg,
            glyph_edge: view.glyph_edge,
            glyph_text: view.glyph_text,
            rubber_band: view.rubber_band,
            box_window: view.box_window,
            box_crossing: view.box_crossing,
            sketch_projected: view.sketch_projected,
            sketch_lost: view.sketch_lost,
            sketch_construction: view.sketch_construction,
            sketch_centerline: view.sketch_centerline,
            bend_up: view.bend_up,
            bend_down: view.bend_down,
            interference: view.interference,
            continuity: view.continuity,
            analysis: view.analysis,
            ..self
        }
    }

    /// The tokens of a theme and colour vision mode: a theme file's palette when one is on for
    /// that base ([`set_custom`]), else the built-in one; the colour vision mode on top.
    pub fn themed(dark: bool, mode: ColorVision) -> Tokens {
        let custom = CUSTOM.get().filter(|c| c.dark == dark).map(|c| c.tokens);
        let t = custom.unwrap_or(if dark { Tokens::dark() } else { Tokens::light() });
        let t = if LOGO_ACCENT.get() { t.with_accent_logo() } else { t };
        t.with_vision(dark, mode)
    }

    /// These tokens with the logo's three faces in shades of the accent: lighter, the accent
    /// itself, darker (as the built-in logo's blues are).
    pub fn with_accent_logo(mut self) -> Tokens {
        let accent = self.accent.to_opaque();
        self.logo = [accent.lerp_to_gamma(Color32::WHITE, 0.22), accent, accent.lerp_to_gamma(Color32::BLACK, 0.2)];
        self
    }

    /// These tokens with a colour vision mode's values on top (the tables in `palettes.rs`); Off
    /// gives them back unchanged.
    pub fn with_vision(self, dark: bool, mode: ColorVision) -> Tokens {
        match mode {
            ColorVision::Off => self,
            ColorVision::Protan | ColorVision::Deutan => self.red_green(dark),
            ColorVision::Tritan => self.blue_yellow(dark),
        }
    }

    /// Fade idle cube colours into the viewport while retaining opaque facets.
    pub fn navigation_cube(self, active: bool) -> Tokens {
        if active {
            return self;
        }
        let mut t = self;
        t.cube_face = t.cube_face.lerp_to_gamma(t.viewport_top, 0.55);
        t.cube_border = t.cube_border.lerp_to_gamma(t.viewport_top, 0.55);
        t.cube_text = t.cube_text.lerp_to_gamma(t.viewport_top, 0.55);
        t.axis_x = t.axis_x.lerp_to_gamma(t.viewport_top, 0.55);
        t.axis_y = t.axis_y.lerp_to_gamma(t.viewport_top, 0.55);
        t.axis_z = t.axis_z.lerp_to_gamma(t.viewport_top, 0.55);
        t
    }
}

/// How a physical material looks (the default appearance follows the material).
pub fn material_color(name: &str) -> Option<Color32> {
    solidcraft_engine::doc::material_color(name).map(|[r, g, b]| Color32::from_rgb(r, g, b))
}

/// Switch themes (the visuals follow on the next [`apply`]).
pub fn set_dark(dark: bool) {
    DARK.set(dark);
}

/// Give the 3D view its own dark or light (#4146), or `None` to draw it as dark as the window.
pub fn set_viewport_dark(dark: Option<bool>) {
    VIEWPORT_DARK.set(dark);
}

/// Give the 3D view a background colour of its own (#7688), or `None` for the theme's gradient.
pub fn set_viewport_background(colour: Option<[u8; 3]>) {
    VIEWPORT_BACKGROUND.set(colour);
}

fn visuals(dark: bool) -> egui::Visuals {
    let t = Tokens::themed(dark, vision());
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.window_stroke = t.primary_border(t.border);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.panel_header;
    v.override_text_color = Some(t.text);
    v.widgets.inactive.weak_bg_fill = t.panel_header;
    v.widgets.inactive.bg_fill = t.panel_header;
    v.widgets.active.weak_bg_fill = t.accent_soft;
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = t.tertiary_border(t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    // egui uses the open-state fill for a movable window's active title bar.
    v.widgets.open.weak_bg_fill = t.panel_header;
    for state in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        state.bg_stroke = t.borders.stroke(BorderRole::Control, state.bg_stroke);
    }
    v.window_corner_radius = egui::CornerRadius::same(t.radius_md);
    v.menu_corner_radius = egui::CornerRadius::same(t.radius_md);
    // Keep the existing 2/3 point egui shapes under the default 4 point small token.
    // Set every interaction state so hovering or focusing cannot restore rounded corners.
    for widget in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.active, &mut v.widgets.open] {
        widget.corner_radius = egui::CornerRadius::same(t.radius_sm / 2);
    }
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(t.radius_sm.saturating_sub(1));
    v
}

/// Seconds the pointer rests on a widget before its tooltip opens.
const TOOLTIP_DELAY: f32 = 0.25;

pub fn apply(ctx: &egui::Context) {
    let dark = is_dark();
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(visuals(dark));
    let t = Tokens::get();
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.space_6, t.space_4);
        s.spacing.button_padding = egui::vec2(t.space_6, t.space_3);
        // egui waits for the pointer to come to a full stop (its velocity settles over 0.1 s)
        // and then 0.5 s more, so a tooltip felt slow and never showed while the hand still
        // drifted over a button (#7679). Count only the short delay since the last movement.
        s.interaction.show_tooltips_only_when_still = false;
        s.interaction.tooltip_delay = TOOLTIP_DELAY;
    });
    // The new visuals are painted on the next pass: without a repaint request the widgets
    // keep the previous theme's colours until some other event schedules a pass (#5802).
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    #[test]
    fn widget_states_and_popup_frames_follow_theme_radii_without_changing_defaults() {
        use super::*;
        let old_custom = CUSTOM.get();
        for dark in [false, true] {
            set_custom(None);
            let actual = visuals(dark);
            let original = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
            assert_eq!(actual.menu_corner_radius, original.menu_corner_radius);
            for (a, b) in [
                (actual.widgets.noninteractive, original.widgets.noninteractive),
                (actual.widgets.inactive, original.widgets.inactive),
                (actual.widgets.hovered, original.widgets.hovered),
                (actual.widgets.active, original.widgets.active),
                (actual.widgets.open, original.widgets.open),
            ] {
                assert_eq!(a.corner_radius, b.corner_radius);
            }
            let tokens = Tokens { radius_sm: 0, radius_md: 0, ..Tokens::dark() };
            set_custom(Some(Custom { dark, tokens }));
            let square = visuals(dark);
            set_custom(old_custom);
            assert_eq!(square.window_corner_radius, egui::CornerRadius::ZERO);
            assert_eq!(square.menu_corner_radius, egui::CornerRadius::ZERO);
            for widget in [square.widgets.noninteractive, square.widgets.inactive, square.widgets.hovered, square.widgets.active, square.widgets.open]
            {
                assert_eq!(widget.corner_radius, egui::CornerRadius::ZERO);
            }
        }
        assert_eq!(Tokens::outset_radius(0.0, 3.0), 0.0);
        assert_eq!(Tokens::outset_radius(6.0, 3.0), 9.0);
    }

    /// A theme change repaints: egui paints the new visuals on the next pass, so without a
    /// repaint request the widgets keep the previous theme's colours until another event
    /// schedules a pass (#5802).
    #[test]
    fn applying_a_theme_asks_for_a_repaint_with_the_new_visuals() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let ctx = egui::Context::default();
        // The integration hook fires on every repaint request, whoever asked for it.
        let requests = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&requests);
        ctx.set_request_repaint_callback(move |_| {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        // Let egui's own startup repaints settle: the context must be idle before the
        // theme changes, so the only repaint left is the one the theme asks for.
        let mut settled = false;
        for _ in 0..8 {
            ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
            if !ctx.has_requested_repaint() {
                settled = true;
                break;
            }
        }
        assert!(settled, "the context must be idle before the theme changes");
        let before = requests.load(Ordering::Relaxed);
        let previous = super::is_dark();
        super::set_dark(!previous);
        let dark = super::is_dark();
        let expected = super::visuals(dark);
        // The theme is applied outside a pass: inside one egui schedules repaints of its
        // own, and the request of the theme would hide among them.
        super::apply(&ctx);
        super::set_dark(previous);
        assert!(requests.load(Ordering::Relaxed) > before, "a theme change must ask for a repaint");
        assert!(ctx.has_requested_repaint(), "a theme change must leave a repaint pending");
        let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
        assert_eq!(ctx.style_of(theme).visuals, expected, "the new theme's visuals must be in place");
    }

    /// Seconds from the pointer's last step onto a button until its tooltip opens, at 60 fps.
    fn seconds_until_tooltip(ctx: &egui::Context) -> Option<f64> {
        let frame = 1.0 / 60.0;
        let mut shown_at = None;
        for step in 0..180 {
            let time = f64::from(step) * frame;
            // The pointer drifts onto the button for a third of a second, then rests.
            let events = match u8::try_from(step) {
                Ok(step) if step <= 20 => vec![egui::Event::PointerMoved(egui::pos2(20.0 + f32::from(step), 20.0))],
                _ => Vec::new(),
            };
            let input = egui::RawInput { time: Some(time), events, ..Default::default() };
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add_sized([120.0, 40.0], egui::Button::new("Extrude")).on_hover_ui(|ui| {
                        ui.label("tip");
                        shown_at.get_or_insert(time);
                    });
                });
            });
            if let Some(at) = shown_at {
                return Some(at - 20.0 * frame);
            }
        }
        None
    }

    /// A tooltip opens a quarter second after the pointer settles on a button, not after
    /// egui's full stop plus half a second (#7679).
    #[test]
    fn tooltips_open_soon_after_the_pointer_rests() {
        let ctx = egui::Context::default();
        super::apply(&ctx);
        let wait = seconds_until_tooltip(&ctx);
        assert!(wait.is_some_and(|s| s < 0.45), "the tooltip must open promptly, waited {wait:?} s");
        // egui's own timing, for comparison: well over half a second.
        let stock = egui::Context::default();
        let stock_wait = seconds_until_tooltip(&stock);
        assert!(stock_wait.is_none_or(|s| s > 0.55), "egui's default waits longer, waited {stock_wait:?} s");
    }

    #[test]
    fn file_view_foreground_pairs_hold_small_text_contrast_without_changing_accent() {
        for dark in [false, true] {
            for vision in super::ColorVision::ALL {
                let t = super::Tokens::themed(dark, vision);
                for (ink, surface) in [
                    (t.file_rail_ink, t.file_rail),
                    (t.file_rail_ink, t.file_rail_hover),
                    (t.file_primary_ink, t.file_primary),
                    (t.file_primary_ink, t.file_background),
                    (t.file_recovery_ink, t.file_recovery),
                    (t.form_ink, t.file_surface),
                    (t.file_dim, t.file_surface),
                    (t.file_dim, t.file_background),
                    (t.file_dim, t.file_primary),
                ] {
                    assert!(super::contrast_ratio(ink, surface) >= 4.5, "dark={dark}: {ink:?} on {surface:?}");
                }
            }
        }
    }

    #[test]
    fn tab_accents_map_explicit_tokens_without_changing_model() {
        for dark in [false, true] {
            for vision in super::ColorVision::ALL {
                let t = super::Tokens::themed(dark, vision);
                // Explicit palette tints and text replace the old gamma lerps. They can be
                // overridden independently by theme files and hold small-text contrast.
                for (tab, color, ink, soft) in [
                    ("SKETCH", t.sketch_accent, t.sketch_ink, t.sketch_soft),
                    ("SURFACE", t.surface_fill, t.surface_ink, t.surface_soft),
                    ("MESH", t.mesh_fill, t.mesh_ink, t.mesh_soft),
                    ("FORM", t.form_fill, t.form_tab_ink, t.form_soft),
                    ("SHEET METAL", t.sheet_fill, t.sheet_ink, t.sheet_soft),
                    ("RENDER", t.render_fill, t.render_ink, t.render_soft),
                ] {
                    let accent = super::TabAccent::for_tab(&t, tab);
                    assert_eq!(accent, super::TabAccent { color, ink, soft });
                    assert_ne!(accent.soft, color);
                    assert_ne!(accent.soft, t.panel);
                }
                for tab in ["MODEL", "SOLID", "ASSEMBLE", "INSERT", "INSPECT", "MANAGE", "VIEW", "unknown"] {
                    assert_eq!(super::TabAccent::for_tab(&t, tab), super::TabAccent { color: t.accent, ink: t.selected_ink, soft: t.accent_soft });
                }
            }
        }
    }

    #[test]
    fn tab_accents_match_approved_palette() {
        let rgb = |hex: u32| egui::Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        for (tab, light, dark) in [
            ("SKETCH", [0x057f45, 0x057f45, 0xedf7f1], [0x128d45, 0x46be78, 0x1f3b2b]),
            ("SURFACE", [0xa45000, 0xa45000, 0xfbebd8], [0xb26a14, 0xe0914a, 0x45331e]),
            ("MESH", [0xe0b200, 0x8a6d00, 0xfff8dc], [0xc6a800, 0xd4b726, 0x41391a]),
            ("FORM", [0x7349cd, 0x7349cd, 0xeee8fa], [0x8a6fcc, 0xb39bee, 0x372f4f]),
            ("SHEET METAL", [0x1e6f96, 0x1e6f96, 0xe1f0f7], [0x2a7fa8, 0x4fb3d9, 0x1f3a47]),
            ("RENDER", [0xa8326e, 0xa8326e, 0xf8e6ef], [0xc8588f, 0xe074ae, 0x402336]),
        ] {
            for (t, values) in [(super::Tokens::light(), light), (super::Tokens::dark(), dark)] {
                let [color, ink, soft] = values.map(rgb);
                assert_eq!(super::TabAccent::for_tab(&t, tab), super::TabAccent { color, ink, soft }, "{tab}");
            }
            let accent = super::TabAccent::for_tab(&super::Tokens::light(), tab);
            assert_eq!(accent.ink != accent.color, tab == "MESH", "only light Mesh separates ink from fill");
        }
        // These existing Model values intentionally take precedence over the board's proposal.
        for (t, values) in [(super::Tokens::light(), [0x2678da, 0x1a507f, 0xd6e6fa]), (super::Tokens::dark(), [0x3a88ee, 0x9cc7f5, 0x2a466e])] {
            let [color, ink, soft] = values.map(rgb);
            assert_eq!(super::TabAccent::for_tab(&t, "MODEL"), super::TabAccent { color, ink, soft });
        }
    }

    #[test]
    fn built_in_selected_tab_labels_hold_wcag_small_text_contrast() {
        let mut themes = vec![("Dark", super::Tokens::dark()), ("Light", super::Tokens::light())];
        themes.extend(crate::theme_files::builtin().iter().map(|theme| (theme.name.as_str(), crate::theme_files::resolve(theme))));
        let mut failures = Vec::new();
        for (name, tokens) in themes {
            for tab in crate::workspace::TABS.iter().copied().chain(["SKETCH", "MESH", "FORM", "SHEET METAL", "RENDER"]) {
                let accent = super::TabAccent::for_tab(&tokens, tab);
                let ratio = super::contrast_ratio(accent.ink, accent.soft);
                if ratio < 4.5 {
                    failures.push(format!("{name} {tab}: {ratio:.4}:1 ({:?} on {:?})", accent.ink, accent.soft));
                }
            }
        }
        assert!(failures.is_empty(), "selected tab contrast below 4.5:1:\n{}", failures.join("\n"));
    }

    #[test]
    fn tab_accents_hold_wcag_small_text_contrast_in_both_themes() {
        for (dark, t) in [(false, super::Tokens::light()), (true, super::Tokens::dark())] {
            for tab in ["MODEL", "SKETCH", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER"] {
                let accent = super::TabAccent::for_tab(&t, tab);
                let card = if dark { t.panel } else { egui::Color32::WHITE };
                for (name, ink, ground) in [
                    ("ink on card", accent.ink, card),
                    ("ink on panel", accent.ink, t.panel),
                    ("ink on soft", accent.ink, accent.soft),
                    ("text on fill", accent.on_fill(&t), accent.color),
                ] {
                    let ratio = super::contrast_ratio(ink, ground);
                    assert!(ratio >= 4.5, "{tab} dark={dark}: {name} is {ratio:.4}:1 ({ink:?} on {ground:?})");
                }
                let white_ok = super::contrast_ratio(t.on_accent, accent.color) >= 4.5;
                assert_eq!(accent.on_fill(&t), if white_ok { t.on_accent } else { t.tab_on_fill_dark });
            }
        }
    }

    #[test]
    fn tab_accent_context_is_per_app_and_does_not_change_widget_visuals() {
        let mut harness = crate::scenario::Harness::new();
        let app = &mut harness.app;
        let first = crate::fonts::test_context();
        let second = crate::fonts::test_context();
        let before = first.style_of(egui::Theme::Dark).visuals.clone();
        app.ui.tab = "SURFACE".into();
        super::TabAccent::publish(app, &first);
        assert_eq!(super::TabAccent::current(&first).color, super::Tokens::get().surface_fill);
        assert_eq!(super::TabAccent::current(&second).color, super::Tokens::get().accent);
        assert_eq!(super::TabAccent::current_icon_accent(&first), Some("surface"));
        assert_eq!(super::TabAccent::current_icon_accent(&second), None);
        assert_eq!(first.style_of(egui::Theme::Dark).visuals, before, "dialog OK, checkboxes and status styles must not inherit the tab");
        app.ui.tab = "MODEL".into();
        super::TabAccent::publish(app, &first);
        assert_eq!(super::TabAccent::current(&first).soft, super::Tokens::get().accent_soft);
        assert_eq!(super::TabAccent::current_icon_accent(&first), None);
    }

    #[test]
    fn concurrent_ui_renderers_keep_their_own_theme_and_vision() {
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for (dark, vision) in [(false, super::ColorVision::Off), (true, super::ColorVision::Protan)] {
                let barrier = &barrier;
                scope.spawn(move || {
                    super::set_dark(dark);
                    super::set_vision(vision);
                    barrier.wait();
                    assert_eq!(super::is_dark(), dark);
                    assert_eq!(super::vision(), vision);
                    assert_eq!(super::Tokens::get().sketch_accent, super::Tokens::themed(dark, vision).sketch_accent);
                });
            }
        });
    }

    /// A 3D view set to the other side's colours (#4146): the view's colours come from the other
    /// side and everything else stays the window's, in every colour vision mode. A view as dark as
    /// the window gives the window's tokens unchanged.
    #[test]
    fn a_view_on_the_other_side_takes_its_colours() {
        use super::{ColorVision, Tokens};
        for mode in ColorVision::ALL {
            super::set_vision(mode);
            for window in [false, true] {
                super::set_dark(window);
                for same in [None, Some(window)] {
                    super::set_viewport_dark(same);
                    assert_eq!(super::viewport_dark(), window);
                    assert_eq!(Tokens::get(), Tokens::themed(window, mode), "{mode:?} {window} {same:?}");
                    assert_eq!(Tokens::view(), Tokens::get(), "{mode:?} {window} {same:?}");
                }
                super::set_viewport_dark(Some(!window));
                assert_eq!(super::viewport_dark(), !window);
                let (t, w, v) = (Tokens::get(), Tokens::themed(window, mode), Tokens::themed(!window, mode));
                // What is drawn straight onto the view takes all of its side's colours.
                assert_eq!(Tokens::view(), v);
                // The two sides differ, so the checks below tell them apart.
                assert_ne!((w.viewport_top, w.panel), (v.viewport_top, v.panel));
                // The view's: background and grid, bodies, selection and hover, sketches with their
                // dimensions and glyphs, the origin, the cube and triads, box selection, analysis.
                assert_eq!((t.viewport_top, t.viewport_bottom, t.grid, t.grid_major), (v.viewport_top, v.viewport_bottom, v.grid, v.grid_major));
                assert_eq!(
                    (t.body, t.body_edge, t.body_selected, t.sel_face, t.sel_edge, t.hover_edge),
                    (v.body, v.body_edge, v.body_selected, v.sel_face, v.sel_edge, v.hover_edge)
                );
                assert_eq!(
                    (t.sketch, t.sketch_fixed, t.dimension, t.glyph_bg, t.glyph_text, t.profile_fill),
                    (v.sketch, v.sketch_fixed, v.dimension, v.glyph_bg, v.glyph_text, v.profile_fill)
                );
                assert_eq!(
                    (t.origin_plane, t.axis_x, t.cube_face, t.cube_text, t.axis_y, t.axis_z),
                    (v.origin_plane, v.axis_x, v.cube_face, v.cube_text, v.axis_y, v.axis_z)
                );
                assert_eq!(
                    (t.box_window, t.section_cap, t.preview_add, t.sketch_construction, t.continuity),
                    (v.box_window, v.section_cap, v.preview_add, v.sketch_construction, v.continuity)
                );
                assert_eq!((t.joint_manipulator, t.joint_axis), (v.joint_manipulator, v.joint_axis));
                assert_eq!(t.analysis, v.analysis);
                // The window's: panels, text, accents, dialogs, and the chips over the view with
                // their borders and icons.
                assert_eq!(
                    (t.panel, t.text, t.border, t.accent, t.accent_soft, t.icon),
                    (w.panel, w.text, w.border, w.accent, w.accent_soft, w.icon)
                );
                assert_eq!(
                    (t.overlay, t.hover, t.dialog_bg, t.field, t.timeline, t.app_bar),
                    (w.overlay, w.hover, w.dialog_bg, w.field, w.timeline, w.app_bar)
                );
            }
        }
        super::set_viewport_dark(None);
    }
}
