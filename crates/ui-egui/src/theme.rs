//! Colours and widget style: a dark theme (the default, a blue-grey in the spirit of desktop CAD
//! tools, viewport #2e3440) and a light one, each with colour vision modes, and theme files that
//! set any of their colours (`theme_files`). Every colour the UI uses comes from [`Tokens`].

use std::cell::Cell;
use std::collections::BTreeMap;

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
    static CUSTOM_GENERATION: Cell<u64> = const { Cell::new(0) };
    /// The application bar's logo in the theme's accent instead of its own logo colours
    /// (Preferences › Interface › Logo), per thread like the rest.
    static LOGO_ACCENT: Cell<bool> = const { Cell::new(false) };
    /// The 3D view's own dark or light when Preferences give it one ([`ViewportTheme`], #4146),
    /// per thread like the rest. `None`: the view is as dark as the window.
    static VIEWPORT_DARK: Cell<Option<bool>> = const { Cell::new(None) };
    /// The 3D view's background colour when Preferences give it one (#7688), per thread like the
    /// rest. `None`: the theme's gradient.
    static VIEWPORT_BACKGROUND: Cell<Option<[u8; 3]>> = const { Cell::new(None) };
    /// The last published frame, guarded by every palette input so another app/context cannot
    /// reuse it after changing the thread's theme. Resolution/fade state lives in that context.
    static PUBLISHED: Cell<Option<PublishedTab>> = const { Cell::new(None) };
}

/// Exact registry names, including contextual/specialty tabs and legacy Render sub-tabs that
/// still share the Render accent. Unknown names must never silently become Model on import.
pub(crate) const TAB_NAMES: &[&str] = &[
    "MODEL",
    "SKETCH",
    "SURFACE",
    "MESH",
    "FORM",
    "SHEET METAL",
    "WELDMENTS",
    "ASSEMBLE",
    "INSERT",
    "INSPECT",
    "MANAGE",
    "VIEW",
    "RENDER",
    "RENDER APPEARANCE",
    "RENDER SCENE",
    "RENDER CAMERA",
];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Environment {
    dark: bool,
    vision: ColorVision,
    custom_generation: u64,
    logo: bool,
    viewport: Option<bool>,
    background: Option<[u8; 3]>,
}

impl Environment {
    fn current() -> Self {
        Self {
            dark: is_dark(),
            vision: vision(),
            custom_generation: CUSTOM_GENERATION.get(),
            logo: LOGO_ACCENT.get(),
            viewport: VIEWPORT_DARK.get(),
            background: viewport_background(),
        }
    }

    fn window(self, tokens: Tokens) -> Tokens {
        let tokens = if self.logo { tokens.with_accent_logo() } else { tokens };
        tokens.with_vision(self.dark, self.vision)
    }
}

#[derive(Clone, Copy)]
struct PublishedTab {
    environment: Environment,
    tokens: Tokens,
    view: Tokens,
    tab_surfaces: bool,
}

#[derive(Clone)]
struct PreparedTheme {
    file: crate::theme_files::ThemeFile,
    raw: Option<BTreeMap<String, serde_json::Value>>,
    values: BTreeMap<String, crate::theme_files::OptionValue>,
    base: Custom,
    enabled: bool,
    fade_ms: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Surfaces {
    panel: Color32,
    top: Color32,
    bottom: Color32,
}

impl Surfaces {
    fn of(t: Tokens) -> Self {
        Self { panel: t.panel, top: t.viewport_top, bottom: t.viewport_bottom }
    }

    fn apply(self, t: &mut Tokens) {
        t.panel = self.panel;
        t.viewport_top = self.top;
        t.viewport_bottom = self.bottom;
    }

    fn blend(self, to: Self, progress: f32) -> Self {
        // Return endpoints verbatim, including premultiplied alpha and byte rounding.
        if progress <= 0.0 {
            return self;
        }
        if progress >= 1.0 {
            return to;
        }
        let eased = 1.0 - (1.0 - progress).powi(3);
        Self {
            panel: oklab_blend(self.panel, to.panel, eased),
            top: oklab_blend(self.top, to.top, eased),
            bottom: oklab_blend(self.bottom, to.bottom, eased),
        }
    }
}

/// Interpolate unassociated sRGB in Oklab; alpha stays independent and is premultiplied once
/// by Color32 on output. This avoids dark fringes when an opacity provider is also enabled.
fn oklab_blend(from: Color32, to: Color32, fraction: f32) -> Color32 {
    let lab = |color: Color32| {
        let [r, g, b, a] = color.to_srgba_unmultiplied();
        let decode = solidcraft_engine::geom::srgb::decode8;
        let (r, g, b) = (decode(r), decode(g), decode(b));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        [
            0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
            f64::from(a),
        ]
    };
    let (a, b) = (lab(from), lab(to));
    let mix = |i: usize| a[i] + (b[i] - a[i]) * f64::from(fraction);
    let (light, a, b) = (mix(0), mix(1), mix(2));
    let l = (light + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (light - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (light - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let encode = |linear: f64| {
        let linear = linear.clamp(0.0, 1.0);
        let srgb = if linear <= 0.0031308 { 12.92 * linear } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
        (srgb * 255.0).round() as u8
    };
    Color32::from_rgba_unmultiplied(
        encode(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        encode(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        encode(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
        mix(3).round() as u8,
    )
}

#[derive(Clone, Copy)]
struct Fade {
    from: Surfaces,
    to: Surfaces,
    start: f64,
    seconds: f64,
}

impl Fade {
    fn sample(self, now: f64) -> (Surfaces, bool) {
        if !now.is_finite() || self.seconds <= 0.0 {
            return (self.to, false);
        }
        let progress = ((now - self.start) / self.seconds).clamp(0.0, 1.0) as f32;
        (self.from.blend(self.to, progress), progress < 1.0)
    }
}

#[derive(Clone, Default)]
struct TabThemeState {
    prepared: Option<PreparedTheme>,
    environment: Option<Environment>,
    targets: BTreeMap<String, TabTarget>,
    tab: String,
    shown: Option<Surfaces>,
    fade: Option<Fade>,
    /// Ownership of the two egui fills survives target/theme invalidation until restored.
    styled_surfaces: bool,
    #[cfg(test)]
    resolutions: usize,
}

#[derive(Clone, Copy)]
struct TabTarget {
    tokens: Tokens,
    view: Tokens,
    accent: TabAccent,
}

const TAB_STATE: &str = "sc_tab_theme_state";

/// Cache the authored theme and raw saved values before any effective-value normalization.
/// Followed-file reloads use full content equality, including edits that keep the same id/base.
/// The caller supplies its existing normalization contract only on a cache miss.
pub(crate) fn prepare_tabs(
    ctx: &egui::Context,
    file: Option<&crate::theme_files::ThemeFile>,
    raw: Option<&BTreeMap<String, serde_json::Value>>,
    values: impl FnOnce() -> BTreeMap<String, crate::theme_files::OptionValue>,
) -> Option<Custom> {
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<TabThemeState>(egui::Id::new(TAB_STATE));
        let same = match (&state.prepared, file) {
            (Some(old), Some(file)) => old.file == *file && old.raw.as_ref() == raw,
            (None, None) => true,
            _ => false,
        };
        if !same {
            state.prepared = file.map(|file| {
                let values = values();
                let (enabled, fade_ms) = crate::theme_options::tab_settings(file, &values);
                let base = Custom { dark: file.base.is_dark(), tokens: crate::theme_files::resolve_with(file, &values) };
                PreparedTheme { file: file.clone(), raw: raw.cloned(), values, base, enabled, fade_ms }
            });
            state.environment = None;
            state.targets.clear();
            // A theme/option change applies immediately; only tab changes animate.
            state.shown = None;
            state.fade = None;
            PUBLISHED.set(None);
        }
        state.prepared.as_ref().map(|p| p.base)
    })
}

impl TabThemeState {
    fn publish(&mut self, tab: &str, now: f64) -> (PublishedTab, TabAccent, bool) {
        let environment = Environment::current();
        if self.environment != Some(environment) {
            self.environment = Some(environment);
            self.targets.clear();
            self.shown = None;
            self.fade = None;
        }
        // Bound the cache even if an internal caller supplies an unknown tab name.
        let tab = if TAB_NAMES.contains(&tab) { tab } else { "MODEL" };
        // Borrow the key on resting frames; own a name only when caching a new target.
        let target = if let Some(target) = self.targets.get(tab) {
            *target
        } else {
            #[cfg(test)]
            {
                self.resolutions += 1;
            }
            let window = if let Some(prepared) = &self.prepared {
                environment.window(crate::theme_files::resolve_for_tab(&prepared.file, &prepared.values, Some(tab)))
            } else {
                Tokens::themed(environment.dark, environment.vision)
            };
            let accent = TabAccent::for_tab(&window, tab);
            let window = if viewport_dark() == is_dark() { window } else { window.with_viewport(Tokens::themed(viewport_dark(), vision())) };
            let target = TabTarget {
                tokens: window.with_background(environment.background),
                view: Tokens::themed(viewport_dark(), vision()).with_background(environment.background),
                accent,
            };
            self.targets.insert(tab.to_owned(), target);
            target
        };
        let destination = Surfaces::of(target.tokens);
        // Sample the old fade before retargeting, even when two tab changes happen in one frame.
        if let Some(fade) = self.fade {
            let (shown, running) = fade.sample(now);
            let mut current = target.tokens;
            shown.apply(&mut current);
            if running {
                current.protect_tab_panel();
            }
            self.shown = Some(Surfaces::of(current));
            if !running {
                self.fade = None;
            }
        }
        if self.tab != tab || self.shown.is_none() {
            let duration = self.prepared.as_ref().filter(|p| p.enabled).map_or(0.0, |p| p.fade_ms);
            self.fade = self.shown.filter(|from| *from != destination && duration > 0.0 && now.is_finite()).map(|from| Fade {
                from,
                to: destination,
                start: now,
                seconds: f64::from(duration) / 1000.0,
            });
            tab.clone_into(&mut self.tab);
            if self.fade.is_none() {
                self.shown = Some(destination);
            }
        }
        let mut tokens = target.tokens;
        self.shown.unwrap_or(destination).apply(&mut tokens);
        // Protect interpolated alpha too; opacity's readability floor applies throughout a fade.
        if self.fade.is_some() {
            tokens.protect_tab_panel();
        }
        self.shown = Some(Surfaces::of(tokens));
        let mut view = if viewport_dark() == is_dark() { tokens } else { target.view };
        view.viewport_top = tokens.viewport_top;
        view.viewport_bottom = tokens.viewport_bottom;
        let tab_surfaces = self.prepared.as_ref().is_some_and(|p| p.enabled && !p.file.tabs.is_empty());
        (PublishedTab { environment, tokens, view, tab_surfaces }, target.accent, self.fade.is_some())
    }
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
    let changed = CUSTOM.with(|c| c.replace(custom) != custom);
    if changed {
        CUSTOM_GENERATION.with(|generation| generation.set(generation.get().wrapping_add(1)));
    }
    changed
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

/// Composite a translucent foreground over an opaque background in sRGB for contrast checks.
pub(crate) fn composite(fill: Color32, background: Color32) -> Color32 {
    let rgb = fill.to_srgba_unmultiplied();
    let a = f64::from(rgb[3]) / 255.0;
    let channel = |index: usize| (f64::from(rgb[index]) * a + f64::from(background[index]) * (1.0 - a)).round() as u8;
    Color32::from_rgb(channel(0), channel(1), channel(2))
}

fn readable_fill(fill: Color32, inks: &[Color32], scale: f32) -> Color32 {
    let readable = |candidate| {
        [Color32::BLACK, Color32::WHITE].iter().all(|background| {
            let ground = composite(candidate, *background);
            inks.iter().all(|ink| contrast_ratio(composite(*ink, ground), ground) >= 4.5)
        })
    };
    // A sampled/retargeted colour that already reads must stay bit-exact, including alpha.
    if scale >= 1.0 && readable(fill) {
        return fill;
    }
    let [r, g, b, alpha] = fill.to_srgba_unmultiplied();
    let requested = (f32::from(alpha) * scale).round() as u8;
    // At most 256 cheap colour comparisons per changed role; no allocations, GPU or I/O.
    for a in requested..=255 {
        let candidate = Color32::from_rgba_unmultiplied(r, g, b, a);
        if readable(candidate) {
            return candidate;
        }
    }
    // An authored foreground may already fail at full opacity. Never alter that foreground;
    // full alpha gives it the strongest protection this option can supply.
    Color32::from_rgb(r, g, b)
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
        let now = ctx.input(|i| i.time);
        let (published, accent, fading, style_surfaces) = ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<TabThemeState>(egui::Id::new(TAB_STATE));
            let (published, accent, fading) = state.publish(&app.ui.tab, now);
            let style_surfaces = published.tab_surfaces || state.styled_surfaces;
            state.styled_surfaces = published.tab_surfaces;
            (published, accent, fading, style_surfaces)
        });
        PUBLISHED.set(Some(published));
        if style_surfaces {
            let fills = {
                let style = ctx.global_style();
                (style.visuals.panel_fill, style.visuals.window_fill)
            };
            if fills != (published.tokens.panel, published.tokens.panel) {
                ctx.global_style_mut(|style| {
                    style.visuals.panel_fill = published.tokens.panel;
                    style.visuals.window_fill = published.tokens.panel;
                });
            }
        }
        if fading {
            ctx.request_repaint();
        }
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

/// Whether the published theme opts into per-tab surfaces. Render's unauthored preview uses
/// this cached policy; it must retain its existing scene backplate when the option is absent/off.
pub(crate) fn tab_surfaces_enabled() -> bool {
    PUBLISHED.get().is_some_and(|p| p.environment == Environment::current() && p.tab_surfaces)
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
    /// Preferences page title, independent of dialog and article headings.
    pub font_page_heading: f32,
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
    /// Ground under every Preferences figure.
    pub figure_ground: Color32,
    /// Outlines and reference lines of Preferences figures.
    pub figure_line: Color32,
    /// Neutral light reflected by a model in a Preferences figure, independent of theme polarity.
    pub figure_highlight: Color32,
    /// Neutral shadow cast by a model in a Preferences figure, independent of theme polarity.
    pub figure_shadow: Color32,
    /// Top face of the Preferences figure cube.
    pub figure_face_top: Color32,
    /// Left face of the Preferences figure cube.
    pub figure_face_left: Color32,
    /// Right face of the Preferences figure cube.
    pub figure_face_right: Color32,
    /// Paper, panels and pads drawn inside Preferences figures.
    pub figure_card: Color32,
    /// What a Preferences figure or selected card acts on.
    pub figure_accent: Color32,
    /// Reference point in a Preferences figure: pivot, origin, pointer.
    pub figure_reference: Color32,
    /// X axis in a Preferences figure.
    pub figure_axis_x: Color32,
    /// Y axis in a Preferences figure.
    pub figure_axis_y: Color32,
    /// Z axis in a Preferences figure.
    pub figure_axis_z: Color32,
    /// Done or fully constrained mark in a Preferences figure.
    pub figure_ok: Color32,
    /// Under-constrained sketch mark in a Preferences figure.
    pub figure_warn: Color32,
    /// Conflicting sketch geometry or state glyph in a Preferences figure.
    pub figure_conflict: Color32,
    /// Segmented control well and tint behind figures in Preferences.
    pub pref_well: Color32,
    /// Warning badge text and stroke in Preferences.
    pub pref_note: Color32,
    /// Warning badge fill in Preferences.
    pub pref_note_fill: Color32,
    /// Ink on a Preferences accent fill.
    pub pref_on_accent: Color32,
    /// Success badge text in Preferences.
    pub pref_ok: Color32,
    /// Success badge fill in Preferences.
    pub pref_ok_fill: Color32,
    /// Ink of a dimmed Preferences control.
    pub pref_disabled: Color32,
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
        if let Some(published) = PUBLISHED.get().filter(|p| p.environment == Environment::current()) {
            if mode == published.environment.vision {
                return published.tokens;
            }
            let mut tokens = Self::base_in(mode);
            Surfaces::of(published.tokens).apply(&mut tokens);
            return tokens;
        }
        Self::base_in(mode)
    }

    /// Unanimated colours for geometry cache keys: fading backgrounds must not rebuild bodies.
    pub(crate) fn base() -> Tokens {
        Self::base_in(vision())
    }

    fn base_in(mode: ColorVision) -> Tokens {
        let window = Tokens::themed(is_dark(), mode);
        let tokens = if viewport_dark() == is_dark() { window } else { window.with_viewport(Tokens::themed(viewport_dark(), mode)) };
        tokens.with_background(viewport_background())
    }

    /// The tokens of the 3D view's side (#4146), for a window colour drawn straight onto the view
    /// with nothing behind it (a status line, a label or a warning beside the geometry), so it
    /// reads on the view's background. As [`Tokens::get`] unless the view has its own side.
    pub fn view() -> Tokens {
        if let Some(published) = PUBLISHED.get().filter(|p| p.environment == Environment::current()) {
            return published.view;
        }
        Tokens::themed(viewport_dark(), vision()).with_background(viewport_background())
    }

    pub(crate) fn protect_tab_panel(&mut self) {
        self.panel = readable_fill(self.panel, &[self.text, self.text_dim, self.form_ink, self.dialog_label], 1.0);
    }

    /// Scale only floating surface fill alpha, preserving full-opacity tokens exactly.
    /// Raise each fill's floor to protect text over both black and white viewport extremes.
    pub(crate) fn apply_panel_opacity(&mut self, percent: f32) {
        if !percent.is_finite() || percent >= 100.0 {
            return;
        }
        let scale = percent.clamp(40.0, 100.0) / 100.0;
        let body_inks = [self.text, self.form_ink, self.dialog_label];
        for fill in [&mut self.panel, &mut self.panel_header, &mut self.dialog_bg, &mut self.dialog_body, &mut self.dialog_footer, &mut self.overlay]
        {
            *fill = readable_fill(*fill, &body_inks, scale);
        }
        self.nav_tip = readable_fill(self.nav_tip, &[self.nav_tip_ink], scale);
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
        let mut t = match mode {
            ColorVision::Off => return self,
            ColorVision::Protan | ColorVision::Deutan => self.red_green(dark),
            ColorVision::Tritan => self.blue_yellow(dark),
        };
        // Figure axes carry the same X/Y/Z meanings as the viewport and its navigation triad.
        t.figure_axis_x = t.axis_x;
        t.figure_axis_y = t.axis_y;
        t.figure_axis_z = t.axis_z;
        // Match SketchState::ink: fully constrained, under constrained, and conflicting.
        t.figure_ok = t.sketch_fixed;
        t.figure_warn = t.sketch;
        t.figure_conflict = t.error;
        t
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

pub fn apply(ctx: &egui::Context) {
    let dark = is_dark();
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(visuals(dark));
    let t = Tokens::get();
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(t.space_6, t.space_4);
        s.spacing.button_padding = egui::vec2(t.space_6, t.space_3);
    });
    // The new visuals are painted on the next pass: without a repaint request the widgets
    // keep the previous theme's colours until some other event schedules a pass (#5802).
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs_state(base: &str, overrides: BTreeMap<String, crate::theme_files::OptionValue>) -> TabThemeState {
        let file = crate::theme_files::tab_fixture(base);
        let values = crate::theme_files::effective_values(&file, &overrides).0;
        let (enabled, fade_ms) = crate::theme_options::tab_settings(&file, &values);
        let base = Custom { dark: file.base.is_dark(), tokens: crate::theme_files::resolve_with(&file, &values) };
        set_dark(base.dark);
        set_custom(Some(base));
        set_vision(ColorVision::Off);
        set_viewport_dark(None);
        set_viewport_background(None);
        TabThemeState { prepared: Some(PreparedTheme { file, values, raw: None, base, enabled, fade_ms }), ..Default::default() }
    }

    #[test]
    fn tabs_fade_has_exact_endpoints_oklab_cubic_midpoint_and_independent_alpha() {
        let from = Surfaces { panel: Color32::BLACK, top: Color32::BLACK, bottom: Color32::BLACK };
        let to = Surfaces { panel: Color32::WHITE, top: Color32::WHITE, bottom: Color32::WHITE };
        assert_eq!(from.blend(to, 0.0), from);
        assert_eq!(from.blend(to, 1.0), to);
        // Neutral Oklab L at half-time is 1 - (1 - .5)^3 = .875. The independent
        // sRGB encoding of .875^3 is .8375, or byte 214 (gamma blending would give 223).
        assert_eq!(from.blend(to, 0.5), Surfaces { panel: Color32::from_gray(214), top: Color32::from_gray(214), bottom: Color32::from_gray(214) });
        let a = Color32::from_rgba_unmultiplied(40, 90, 130, 128);
        let b = Color32::from_rgba_unmultiplied(90, 150, 190, 64);
        assert_eq!(oklab_blend(a, b, 0.875).a(), 72);
        let fade = Fade { from, to, start: 1.0, seconds: 0.24 };
        assert_eq!(fade.sample(1.0), (from, true));
        assert_eq!(fade.sample(1.24), (to, false));
        assert_eq!(Fade { seconds: 0.0, ..fade }.sample(1.0), (to, false));
        assert_eq!(fade.sample(f64::NAN), (to, false));
    }

    #[test]
    fn tabs_fade_retargets_from_displayed_surfaces_and_stops_resolving_at_rest() {
        for opacity in [40.0, 100.0] {
            let mut state = tabs_state("dark", BTreeMap::from([("opacity".into(), crate::theme_files::OptionValue::Num(opacity))]));
            let (model, _, repaint) = state.publish("MODEL", 0.0);
            assert!(!repaint);
            let (start, _, repaint) = state.publish("SKETCH", 1.0);
            assert!(repaint);
            assert_eq!(Surfaces::of(start.tokens), Surfaces::of(model.tokens));
            let (middle, _, _) = state.publish("SKETCH", 1.12);
            let (retarget, _, repaint) = state.publish("RENDER", 1.12);
            assert!(repaint);
            assert_eq!(Surfaces::of(retarget.tokens), Surfaces::of(middle.tokens));
            assert_eq!(retarget.view.viewport_top, retarget.tokens.viewport_top);
            assert_eq!(retarget.view, retarget.tokens);
            let (end, _, repaint) = state.publish("RENDER", 1.36);
            assert!(!repaint);
            assert_eq!(state.resolutions, 3);
            for frame in 0..20 {
                let (resting, _, repaint) = state.publish("RENDER", 2.0 + f64::from(frame));
                assert!(!repaint);
                assert_eq!(resting.tokens, end.tokens);
            }
            assert_eq!(state.resolutions, 3, "resting frames must hit the resolved-token cache");
            state.publish("MODEL", 30.0);
            assert_eq!(state.resolutions, 3, "returning to a tab reuses its target");
        }
    }

    #[test]
    fn tabs_panel_text_contrast_holds_at_five_fade_steps_with_opacity() {
        use crate::theme_files::OptionValue;
        for base in ["dark", "light"] {
            for percent in [40.0, 75.0, 100.0] {
                for mode in ColorVision::ALL {
                    for destination in ["SKETCH", "RENDER", "MODEL"] {
                        let mut state = tabs_state(base, BTreeMap::from([("opacity".into(), OptionValue::Num(percent))]));
                        set_vision(mode);
                        state.publish(if destination == "MODEL" { "RENDER" } else { "MODEL" }, 0.0);
                        state.publish(destination, 1.0);
                        for step in [0.0, 0.25, 0.5, 0.75, 1.0] {
                            let (frame, _, _) = state.publish(destination, 1.0 + step * 0.24);
                            let t = frame.tokens;
                            for behind in [Color32::BLACK, Color32::WHITE] {
                                let ground = composite(t.panel, behind);
                                for ink in [t.text, t.text_dim, t.form_ink, t.dialog_label] {
                                    assert!(
                                        contrast_ratio(composite(ink, ground), ground) >= 4.5,
                                        "{base} {mode:?} opacity {percent} tab {destination} step {step}"
                                    );
                                }
                            }
                            assert_eq!((frame.view.viewport_top, frame.view.viewport_bottom), (t.viewport_top, t.viewport_bottom));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tabs_cache_invalidates_for_theme_values_vision_and_viewport_overrides() {
        let ctx = egui::Context::default();
        let mut file = crate::theme_files::tab_fixture("dark");
        let normalizations = Cell::new(0);
        let prepare = |file: &crate::theme_files::ThemeFile, raw: Option<&BTreeMap<String, serde_json::Value>>| {
            let custom = prepare_tabs(&ctx, Some(file), raw, || {
                normalizations.set(normalizations.get() + 1);
                let saved = raw.into_iter().flatten().map(|(k, v)| (k.clone(), serde_json::from_value(v.clone()).unwrap())).collect();
                crate::theme_files::effective_values(file, &saved).0
            });
            set_dark(file.base.is_dark());
            set_custom(custom);
        };
        let publish = || ctx.data_mut(|d| d.get_temp_mut_or_default::<TabThemeState>(egui::Id::new(TAB_STATE)).publish("SKETCH", 1.0));
        set_vision(ColorVision::Off);
        set_viewport_dark(None);
        set_viewport_background(None);
        prepare(&file, None);
        let before = publish().0.tokens;
        prepare(&file, None);
        assert_eq!(publish().0.tokens, before);
        assert_eq!(normalizations.get(), 1);
        // The content changes while identity/base remain the same, as with a followed reload.
        file.tabs.get_mut("SKETCH").unwrap().insert("viewport_top".into(), serde_json::json!("#1a4232"));
        prepare(&file, None);
        assert_ne!(publish().0.tokens.viewport_top, before.viewport_top);
        assert_eq!(normalizations.get(), 2);
        set_vision(ColorVision::Tritan);
        assert_eq!(publish().0.tokens.axis_x, before.with_vision(true, ColorVision::Tritan).axis_x);
        set_viewport_dark(Some(false));
        assert_eq!(publish().0.tokens.viewport_top, Tokens::light().viewport_top);
        set_viewport_background(Some([3, 7, 11]));
        assert_eq!(publish().0.tokens.viewport_top, Color32::from_rgb(3, 7, 11));
        set_viewport_background(None);
        set_viewport_dark(None);
        let raw = BTreeMap::from([("tab_colours".into(), serde_json::json!(false))]);
        prepare(&file, Some(&raw));
        assert_eq!(publish().0.tokens, Tokens::base());
        assert_eq!(normalizations.get(), 3);
    }

    #[test]
    fn tabs_publish_requests_repaints_only_during_fade_and_zero_is_instant() {
        use crate::theme_files::OptionValue;
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        let ctx = crate::fonts::test_context();
        let mut state = tabs_state("dark", BTreeMap::new());
        state.publish("MODEL", 0.0);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(TAB_STATE), state));
        app.ui.tab = "SKETCH".into();
        let frame = |time| {
            let mut out = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                TabAccent::publish(&app, ui.ctx());
                crate::gpu::paint_background(ui.painter(), ui.max_rect(), &Tokens::get());
            });
            let colors: Vec<_> = out
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => Some(mesh.vertices.iter().map(|vertex| vertex.color).collect()),
                    _ => None,
                })
                .unwrap();
            let tokens = Tokens::get();
            assert_eq!(colors, [tokens.viewport_top, tokens.viewport_top, tokens.viewport_bottom, tokens.viewport_bottom]);
            out.textures_delta.clear();
            out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        assert!(frame(1.0).is_zero());
        assert!(frame(1.12).is_zero());
        // Drain the previously requested pass, then verify the endpoint leaves the loop asleep.
        for time in [1.24, 1.3, 1.4] {
            frame(time);
        }
        assert_eq!(frame(2.0), std::time::Duration::MAX);
        let (end, _, _) = ctx.data_mut(|d| d.get_temp_mut_or_default::<TabThemeState>(egui::Id::new(TAB_STATE)).publish("SKETCH", 2.0));
        assert_eq!(Tokens::get(), end.tokens);
        let mut instant = tabs_state("dark", BTreeMap::from([("tab_fade_ms".into(), OptionValue::Num(0.0))]));
        instant.publish("MODEL", 0.0);
        let (frame, _, repaint) = instant.publish("SKETCH", 1.0);
        assert!(!repaint);
        assert_eq!(
            frame.tokens,
            crate::theme_files::resolve_for_tab(&instant.prepared.as_ref().unwrap().file, &instant.prepared.as_ref().unwrap().values, Some("SKETCH"))
        );
    }

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

    #[test]
    fn preferences_foreground_pairs_hold_small_text_contrast() {
        for dark in [false, true] {
            for vision in ColorVision::ALL {
                let t = Tokens::themed(dark, vision);
                for (ink_name, ink, surface_name, surface) in [
                    ("pref_note", t.pref_note, "pref_note_fill", t.pref_note_fill),
                    ("pref_ok", t.pref_ok, "pref_ok_fill", t.pref_ok_fill),
                    ("pref_on_accent", t.pref_on_accent, "figure_accent", t.figure_accent),
                    ("figure_accent", t.figure_accent, "figure_card", t.figure_card),
                    ("figure_accent", t.figure_accent, "selected_bg", t.selected_bg),
                ] {
                    let contrast = contrast_ratio(ink, surface);
                    assert!(contrast >= 4.5, "dark={dark} {vision:?}: {ink_name} on {surface_name}: {contrast:.2}");
                }
                assert_eq!(t.figure_highlight, Color32::WHITE, "a model highlight stays light in either theme");
                assert_eq!(t.figure_shadow, Color32::BLACK, "a model shadow stays dark in either theme");
            }
        }
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
    fn tabs_surface_publication_restores_base_fills_without_restyling_widgets() {
        use crate::theme_files::{OptionValue, ThemeFile};
        for base in ["dark", "light"] {
            let ctx = crate::fonts::test_context();
            let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            let file = crate::theme_files::tab_fixture(base);
            set_dark(file.base.is_dark());
            set_vision(ColorVision::Off);
            set_viewport_dark(None);
            set_viewport_background(None);
            set_custom(None);
            let prepare = |file: &ThemeFile, enabled: Option<bool>| {
                let raw = enabled.map(|enabled| BTreeMap::from([("tab_colours".into(), serde_json::json!(enabled))]));
                let custom = prepare_tabs(&ctx, Some(file), raw.as_ref(), || {
                    let saved = enabled.map(|enabled| BTreeMap::from([("tab_colours".into(), OptionValue::Bool(enabled))])).unwrap_or_default();
                    crate::theme_files::effective_values(file, &saved).0
                });
                set_custom(custom)
            };
            let publish = |app: &crate::SolveApp, time| {
                ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| TabAccent::publish(app, ui.ctx())).textures_delta.clear();
            };
            prepare(&file, Some(true));
            apply(&ctx);
            let before = ctx.global_style().visuals.clone();
            let base_custom = CUSTOM.get();
            app.ui.tab = "MODEL".into();
            publish(&app, 0.0);
            assert_eq!(ctx.global_style().visuals, before);
            app.ui.tab = "SKETCH".into();
            publish(&app, 1.0);
            publish(&app, 1.12);
            let panel = Tokens::get().panel;
            assert_ne!(panel, before.panel_fill, "exercise an actual animated panel fill");
            let mut animated = before.clone();
            animated.panel_fill = panel;
            animated.window_fill = panel;
            assert_eq!(ctx.global_style().visuals, animated, "all non-surface widget/selection/status visuals stay unchanged");
            // Repair either owned fill independently, even when the panel already matches.
            ctx.global_style_mut(|style| style.visuals.window_fill = before.window_fill);
            publish(&app, 1.12);
            assert_eq!(ctx.global_style().visuals, animated);

            assert!(!prepare(&file, Some(false)), "toggling surfaces does not change the base Custom tokens");
            assert_eq!(CUSTOM.get(), base_custom);
            // Deliberately omit apply(): a full restyle would conceal stale animated fills.
            publish(&app, 1.13);
            assert_eq!(ctx.global_style().visuals, before, "turning surfaces off must restore both base fills");

            assert!(!prepare(&file, Some(true)));
            publish(&app, 2.0);
            assert_ne!(ctx.global_style().visuals.panel_fill, before.panel_fill);
            let mut plain = file.clone();
            plain.tabs.clear();
            plain.options.clear();
            assert!(!prepare(&plain, None), "a no-option theme can retain the exact same base tokens");
            assert_eq!(CUSTOM.get(), base_custom);
            publish(&app, 2.1);
            assert_eq!(ctx.global_style().visuals, before, "restoring a no-option theme must release the surface fills");
        }
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
