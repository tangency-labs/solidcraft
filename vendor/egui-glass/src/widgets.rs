use egui::{Color32, InnerResponse, Margin, Rect, Response, Sense, Shape, Ui, Vec2, WidgetText};

use crate::backdrop::backdrop_state;
use crate::renderer::GlassCallback;
use crate::GlassStyle;

/// Paints a glass surface filling `rect`. Building block for custom widgets.
pub fn paint_glass(ui: &Ui, rect: Rect, style: &GlassStyle) {
    ui.painter().add(glass_shape(ui, rect, style));
}

fn glass_shape(ui: &Ui, rect: Rect, style: &GlassStyle) -> Shape {
    glass_shape_in(ui.ctx(), rect, style)
}

/// SolidCraft: the glass surface for `rect` as a shape, for an app that places it in a paint
/// list itself (under a panel's own fill) instead of through [`paint_glass`].
pub fn glass_shape_in(ctx: &egui::Context, rect: Rect, style: &GlassStyle) -> Shape {
    let (backdrop_rect, fill) = match crate::live::live_state(ctx) {
        Some(live) => (live.screen_rect, Color32::TRANSPARENT),
        None => backdrop_state(ctx).map(|s| (s.rect, s.fill)).unwrap_or((rect, Color32::TRANSPARENT)),
    };
    let margin = style.shadow_radius * 1.5;
    let callback = GlassCallback::new(rect, backdrop_rect, fill, *style, ctx.cumulative_pass_nr());
    Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect.expand(margin), callback))
}

fn expand_margin(rect: Rect, m: Margin, sign: f32) -> Rect {
    Rect::from_min_max(rect.min - sign * egui::vec2(m.left as f32, m.top as f32), rect.max + sign * egui::vec2(m.right as f32, m.bottom as f32))
}

/// Flat widget visuals for controls sitting on glass: transparent idle
/// background, soft translucent highlight, capsule corners, no strokes.
/// Apple never stacks glass on glass.
fn flat_visuals(ui: &mut Ui, style: &GlassStyle) {
    let highlight = if style.is_dark() { Color32::from_white_alpha(36) } else { Color32::from_black_alpha(14) };
    let fg = text_color(ui, style);
    let widgets = &mut ui.style_mut().visuals.widgets;
    for w in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
        w.bg_stroke = egui::Stroke::NONE;
        w.corner_radius = egui::CornerRadius::same(u8::MAX);
        w.expansion = 0.0;
        w.fg_stroke.color = fg;
    }
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_fill = Color32::TRANSPARENT;
    widgets.hovered.weak_bg_fill = highlight;
    widgets.hovered.bg_fill = highlight;
    widgets.active.weak_bg_fill = highlight.gamma_multiply(1.6);
    widgets.active.bg_fill = highlight.gamma_multiply(1.6);
    ui.style_mut().visuals.selection.bg_fill = if style.is_dark() { Color32::from_white_alpha(56) } else { Color32::from_black_alpha(26) };
    ui.style_mut().visuals.selection.stroke.color = fg;
}

/// Text colour that reads on this glass: light on dark tints, else the theme's strong text.
fn text_color(ui: &Ui, style: &GlassStyle) -> Color32 {
    if style.is_dark() {
        Color32::WHITE
    } else {
        ui.visuals().strong_text_color()
    }
}

/// A glass container: card, section, sidebar, sheet.
pub struct Glass {
    style: GlassStyle,
    inner_margin: Margin,
    preserve_theme: bool,
}

impl Default for Glass {
    /// [`GlassStyle::regular`]: the tuned default for cards and small surfaces.
    fn default() -> Self {
        Self::new(GlassStyle::regular())
    }
}

impl Glass {
    /// Creates a container with `style` and a 16-point inner margin.
    pub fn new(style: GlassStyle) -> Self {
        Self { style, inner_margin: Margin::same(16), preserve_theme: false }
    }

    /// A large frosted sheet (sidebar, sheet): [`GlassStyle::panel`].
    pub fn panel() -> Self {
        Self::new(GlassStyle::panel())
    }

    /// [`Glass::panel`] for dark themes.
    pub fn panel_dark() -> Self {
        Self::new(GlassStyle::panel_dark())
    }

    /// Sets the space between the glass edge and its contents.
    pub fn inner_margin(mut self, margin: impl Into<Margin>) -> Self {
        self.inner_margin = margin.into();
        self
    }

    /// Keeps the parent UI's control visuals instead of applying flat glass visuals.
    /// Defaults to `false`. Glass material and padding are unaffected; choose a tint
    /// that provides enough contrast with the preserved text colours.
    pub fn preserve_theme(mut self, preserve: bool) -> Self {
        self.preserve_theme = preserve;
        self
    }

    /// Lays out contents and paints glass behind them, applying flat control visuals
    /// unless [`Self::preserve_theme`] is enabled.
    /// Returns the closure's result and the container's hover response.
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        let background = ui.painter().add(Shape::Noop);
        let max_rect = expand_margin(ui.available_rect_before_wrap(), self.inner_margin, -1.0);
        let mut content = ui.new_child(egui::UiBuilder::new().max_rect(max_rect));
        if !self.preserve_theme {
            flat_visuals(&mut content, &self.style);
        }
        let inner = add_contents(&mut content);
        let rect = expand_margin(content.min_rect(), self.inner_margin, 1.0);
        ui.painter().set(background, glass_shape(ui, rect, &self.style));
        let response = ui.allocate_rect(rect, Sense::hover());
        InnerResponse::new(inner, response)
    }
}

/// A capsule glass button with a text (or icon glyph) label.
pub struct GlassButton {
    text: WidgetText,
    style: GlassStyle,
    padding: Vec2,
    min_size: Vec2,
    text_color: Option<Color32>,
    accessible_name: Option<String>,
    animate: bool,
}

impl GlassButton {
    /// Creates a capsule button with a minimum size of 44 by 44 points.
    pub fn new(text: impl Into<WidgetText>) -> Self {
        Self {
            text: text.into(),
            style: GlassStyle::default().capsule(),
            padding: Vec2::new(18.0, 10.0),
            min_size: Vec2::new(44.0, 44.0),
            text_color: None,
            accessible_name: None,
            animate: true,
        }
    }

    /// Replaces the material and corner shape, including the default capsule radius.
    pub fn style(mut self, style: GlassStyle) -> Self {
        self.style = style;
        self
    }

    /// Square/circular icon button.
    pub fn icon(mut self) -> Self {
        self.padding = Vec2::splat(10.0);
        self
    }

    /// Sets the minimum size in logical points; larger content can expand it.
    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = size;
        self
    }

    /// Overrides the automatic light/dark label colour.
    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    /// Sets the name announced by assistive technology, without changing the label.
    /// Use a descriptive action such as "Back" for an icon-only button.
    pub fn accessible_name(mut self, name: impl Into<String>) -> Self {
        self.accessible_name = Some(name.into());
        self
    }

    /// Enables hover/press material transitions (default `true`).
    /// Uses the UI's animation time, capped at 120 ms; `false` switches instantly.
    /// Input and keyboard focus remain immediate.
    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    /// Draws the button and returns its interaction response, including clicks.
    pub fn show(self, ui: &mut Ui) -> Response {
        let galley = self.text.into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button);
        let size = (galley.size() + 2.0 * self.padding).max(self.min_size);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let accessible_name = self.accessible_name.as_deref().unwrap_or_else(|| galley.text());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), accessible_name));
        if ui.is_rect_visible(rect) {
            let style = crate::animation::interaction_style(ui, &response, self.style, self.animate);
            paint_glass(ui, rect, &style);
            let color = self.text_color.unwrap_or_else(|| text_color(ui, &style));
            let pos = rect.center() - galley.size() / 2.0;
            ui.painter().galley(pos, galley, color);
            if response.has_focus() {
                let radius = style.corner_radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
                let stroke = egui::Stroke::new(ui.visuals().selection.stroke.width.max(2.0), ui.visuals().selection.stroke.color);
                ui.painter().rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
            }
        }
        response
    }
}

/// A capsule glass bar holding flat buttons (`ui.button`, `ui.selectable_label`, …).
/// By default, children get a transparent idle background and a soft highlight on hover,
/// following Apple's rule of never stacking glass on glass.
pub struct GlassToolbar {
    style: GlassStyle,
    spacing: f32,
    preserve_theme: bool,
}

impl Default for GlassToolbar {
    fn default() -> Self {
        Self::new(GlassStyle::regular())
    }
}

impl GlassToolbar {
    /// Creates a toolbar with a capsule-shaped material and 6-point item spacing.
    pub fn new(style: GlassStyle) -> Self {
        Self { style: style.capsule(), spacing: 6.0, preserve_theme: false }
    }

    /// Sets horizontal spacing between items in logical points.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Keeps the parent UI's control visuals. Defaults to `false`.
    /// Toolbar spacing and padding are unaffected. See [`Glass::preserve_theme`].
    pub fn preserve_theme(mut self, preserve: bool) -> Self {
        self.preserve_theme = preserve;
        self
    }

    /// Draws a horizontal row of controls on a glass capsule.
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
        Glass::new(self.style).preserve_theme(self.preserve_theme).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = self.spacing;
            ui.spacing_mut().button_padding = Vec2::new(12.0, 10.0);
            ui.horizontal(add_contents).inner
        })
    }
}
