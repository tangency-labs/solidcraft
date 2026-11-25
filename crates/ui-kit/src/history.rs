//! Shared history chips, dense name rows, drop bars and rollback markers.
use crate::BackingTooltipExt as _;
use crate::icons;
use egui::{Painter, Rect, Stroke, pos2, vec2};
use solidcraft_theme::{TabAccent, Tokens};

/// Browser and named history rows use the same density.
pub const TREE_ROW_HEIGHT: f32 = 26.0;

/// Shared bottom-strip title: History's typography, centred baseline and trailing gap.
pub struct StripHeading(std::sync::Arc<egui::Galley>);

impl StripHeading {
    pub fn new(painter: &Painter, title: &str) -> Self {
        let t = Tokens::get();
        Self(painter.layout_no_wrap(title.to_owned(), crate::type_face::semibold(t.font_heading), t.text))
    }

    pub fn width(&self) -> f32 {
        self.0.size().x + Tokens::get().space_12
    }

    pub fn paint(&self, painter: &Painter, rect: Rect) -> Rect {
        let text = Rect::from_min_size(pos2(rect.left(), rect.center().y - self.0.size().y / 2.0), self.0.size());
        painter.galley(text.min, self.0.clone(), Tokens::get().text);
        text
    }

    pub fn show(&self, ui: &mut egui::Ui) -> egui::Response {
        let width = self.width() - ui.spacing().item_spacing.x;
        let (rect, response) = ui.allocate_exact_size(vec2(width.max(self.0.size().x), Tokens::get().control_height), egui::Sense::hover());
        self.paint(ui.painter(), rect);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), self.0.text()));
        response
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum HistoryStyle {
    Bottom,
    Icons,
    Names,
}

impl HistoryStyle {
    pub fn height(self) -> f32 {
        let t = Tokens::get();
        match self {
            Self::Bottom => t.control_height,
            Self::Icons => t.control_height + t.space_10,
            Self::Names => TREE_ROW_HEIGHT,
        }
    }
}

pub struct HistoryChip<'a> {
    pub icon: &'a str,
    pub name: &'a str,
    /// The icon's accent (an icon palette name the host gives each step; `None`: its artwork's
    /// own colours).
    pub icon_accent: Option<&'a str>,
    pub selected: bool,
    pub hovered: bool,
    pub dim: bool,
    pub suppressed: bool,
    pub error: bool,
    pub warning: bool,
    /// A row that points at an item listed elsewhere (a sketch under its first feature, #6489):
    /// its name is in secondary text.
    pub reference: bool,
    pub accent: TabAccent,
    /// Draw the icon in this one colour instead of its artwork: a sketch's solve state (#7245).
    pub ink: Option<egui::Color32>,
}

impl HistoryChip<'_> {
    /// All strip variants use this painter; no connections are drawn between features.
    pub fn paint(&self, painter: &Painter, rect: Rect, style: HistoryStyle) {
        let t = Tokens::get();
        let mut p = painter.clone();
        if self.dim || self.suppressed {
            p.multiply_opacity(0.42);
        }
        let fill = if self.error {
            t.form_error_fill
        } else if self.selected {
            self.accent.soft
        } else if self.hovered {
            t.hover
        } else if style == HistoryStyle::Names {
            egui::Color32::TRANSPARENT
        } else {
            t.timeline_item
        };
        let edge = if self.error {
            t.form_error
        } else if self.selected || self.hovered {
            self.accent.color
        } else {
            t.border
        };
        let radius = if style == HistoryStyle::Icons { t.radius_lg } else { t.radius_sm };
        if style != HistoryStyle::Names || self.error {
            let weight = if style == HistoryStyle::Icons && (self.selected || self.error) { 1.5 } else { 1.0 };
            p.rect(
                rect,
                radius,
                fill,
                if self.error || self.selected || self.hovered { Stroke::new(weight, edge) } else { t.tertiary_border(edge) },
                egui::StrokeKind::Inside,
            );
        } else {
            p.rect_filled(rect, radius, fill);
        }
        let names = style == HistoryStyle::Names;
        let side = if names {
            t.space_14 + 1.0
        } else if style == HistoryStyle::Icons {
            t.space_16 + t.space_3
        } else {
            rect.height() - t.space_8
        };
        let icon =
            Rect::from_center_size(if names { pos2(rect.left() + t.space_8 + side / 2.0, rect.center().y) } else { rect.center() }, vec2(side, side));
        match self.ink {
            Some(ink) => icons::paint_ink(&p, icon, self.icon, ink),
            None => icons::paint_accented(&p, icon, self.icon, self.icon_accent, 1.0),
        }
        if names {
            let ink = if self.selected {
                self.accent.ink
            } else if self.reference {
                t.text_dim
            } else {
                t.text
            };
            let font = if self.selected { crate::type_face::semibold(t.font_body) } else { egui::FontId::proportional(t.font_body) };
            let text_rect = Rect::from_min_max(
                pos2(icon.right() + t.space_6 + 1.0, rect.top()),
                pos2(rect.right() - if self.error || self.warning { t.space_16 } else { t.space_8 }, rect.bottom()),
            );
            let galley = p.layout_no_wrap(self.name.to_owned(), font, ink);
            p.with_clip_rect(text_rect.intersect(p.clip_rect())).galley(pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0), galley, ink);
        }
        if self.suppressed {
            p.line_segment([icon.left_bottom(), icon.right_top()], Stroke::new(1.5, t.text_dim));
        }
        if self.error || self.warning {
            let side = if names { t.space_12 } else { t.space_10 };
            let at = if names { pos2(rect.right() - t.space_3 - side, rect.center().y - side / 2.0) } else { rect.right_top() - vec2(side, 0.0) };
            icons::paint_icon(&p, Rect::from_min_size(at, vec2(side, side)), if self.error { "error" } else { "warning" }, 1.0);
        }
    }
}

/// A single drop indicator in either strip orientation.
pub fn history_drop_bar(p: &Painter, rect: Rect, vertical: bool, colour: egui::Color32) {
    let points = if vertical {
        [pos2(rect.left(), rect.center().y), pos2(rect.right(), rect.center().y)]
    } else {
        [pos2(rect.center().x, rect.top()), pos2(rect.center().x, rect.bottom())]
    };
    p.line_segment(points, Stroke::new(Tokens::get().space_2, colour));
}

/// Rollback line and hollow end circle; a vertical strip uses a horizontal line.
pub fn history_marker(p: &Painter, rect: Rect, vertical: bool, accent: TabAccent) {
    let t = Tokens::get();
    let outer_radius = t.space_10 / 2.0;
    let centre = if vertical { pos2(rect.right() - outer_radius, rect.center().y) } else { pos2(rect.center().x, rect.bottom() - outer_radius) };
    let direction = if vertical { vec2(1.0, 0.0) } else { vec2(0.0, 1.0) };
    let start = if vertical { rect.left_center() } else { rect.center_top() };
    // The line reaches one point inside the circle's outer edge, as on the reference board.
    let end = centre - direction * (outer_radius - 1.0);
    p.line_segment([start, end], Stroke::new(t.space_2, accent.color));
    // Circle strokes are centred on their radius: inset by half the stroke for a 10 pt outside diameter.
    p.circle(centre, outer_radius - t.space_2 / 2.0, if vertical { t.panel } else { t.timeline }, Stroke::new(t.space_2, accent.color));
}

/// The host's names for the Play control: playing History, and stopping it while it plays.
#[derive(Clone, Copy, Debug)]
pub struct PlayLabels<'a> {
    pub play: &'a str,
    pub stop: &'a str,
}

/// The same controls are used in every History orientation: first, previous, Play History, next
/// and last, or the four without Play when `history_play_button` is off.
#[derive(Clone, Copy, PartialEq)]
pub enum HistoryTransport {
    First,
    Back,
    Play,
    Forward,
    Last,
}

impl HistoryTransport {
    pub const ALL: [Self; 4] = [Self::First, Self::Back, Self::Forward, Self::Last];
    pub const WITH_PLAY: [Self; 5] = [Self::First, Self::Back, Self::Play, Self::Forward, Self::Last];

    /// The controls History shows: the four, or five with the Play button preference on.
    pub fn shown(play: bool) -> &'static [Self] {
        if play { &Self::WITH_PLAY } else { &Self::ALL }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Back => "back",
            Self::Play => "play",
            Self::Forward => "forward",
            Self::Last => "last",
        }
    }

    pub fn label(self, playing: bool, play: PlayLabels<'_>) -> &str {
        match self {
            Self::First => solidcraft_ui_text::tr("ui.history_first"),
            Self::Back => solidcraft_ui_text::tr("ui.history_back"),
            Self::Play if playing => play.stop,
            Self::Play => play.play,
            Self::Forward => solidcraft_ui_text::tr("ui.history_forward"),
            Self::Last => solidcraft_ui_text::tr("ui.history_last"),
        }
    }

    /// `playing` turns the Play button into Stop, named by `play`; the other controls ignore
    /// both. Published as `history:` and its [`HistoryTransport::id`].
    pub fn show(self, ui: &mut egui::Ui, rect: Rect, playing: bool, play: PlayLabels<'_>) -> egui::Response {
        self.show_as(ui, rect, playing, "history:", play)
    }

    /// [`HistoryTransport::show`], published in the widget registry as `prefix` followed by its
    /// [`HistoryTransport::id`].
    pub fn show_as(self, ui: &mut egui::Ui, rect: Rect, playing: bool, prefix: &str, play: PlayLabels<'_>) -> egui::Response {
        let t = Tokens::get();
        let response = ui.interact(rect, ui.scope_id().with(("history_transport", self.id())), egui::Sense::click());
        solidcraft_ui_registry::publish_target(&format!("{prefix}{}", self.id()), rect);
        if response.hovered() {
            ui.painter().rect_filled(rect, t.radius_sm, t.hover);
        }
        let glyph = if self == Self::Play && playing { "pause" } else { self.id() };
        crate::icons::glyphs::transport_glyph(ui.painter(), rect.center(), glyph, t.icon);
        let label = self.label(playing, play);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
        response.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(label)
    }
}

pub fn history_transport_width() -> f32 {
    let t = Tokens::get();
    t.space_24 * HistoryTransport::ALL.len() as f32 + t.space_2 * (HistoryTransport::ALL.len() - 1) as f32
}

/// The width of `count` transport buttons side by side (five with Play History shown).
pub fn history_controls_width(count: usize) -> f32 {
    let t = Tokens::get();
    t.space_24 * count as f32 + t.space_2 * count.saturating_sub(1) as f32
}

/// Thin and minimum named widths; wider named panels retain their measured width.
pub fn history_width(names: bool) -> f32 {
    let t = Tokens::get();
    if names { history_transport_width() + t.space_10 * 2.0 + t.space_8 } else { t.control_height * 2.0 }
}

/// Drag handle for shared panel headers.
pub fn panel_grip(ui: &mut egui::Ui) -> egui::Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(t.space_10, t.space_16 + t.space_2), egui::Sense::hover());
    for row in 0..3 {
        for col in 0..2 {
            ui.painter().circle_filled(rect.left_top() + vec2(2.5 + col as f32 * 5.0, 4.0 + row as f32 * 5.0), 1.1, t.text_dim);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_selection_error_and_rollback_use_tokens_in_both_themes() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let t = Tokens::get();
            let accent = TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Surface);
            for style in [HistoryStyle::Icons, HistoryStyle::Names] {
                for (selected, error, dim) in [(true, false, false), (false, true, false), (true, false, true)] {
                    let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(158.0, style.height()));
                    let ctx = crate::test_support::context();
                    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                        HistoryChip {
                            icon: "surface_extrude",
                            name: "Offset1",
                            icon_accent: None,
                            selected,
                            hovered: false,
                            dim,
                            suppressed: false,
                            error,
                            warning: false,
                            reference: false,
                            accent,
                            ink: None,
                        }
                        .paint(ui.painter(), rect, style)
                    });
                    out.textures_delta.clear();
                    let background = out
                        .shapes
                        .iter()
                        .find_map(|s| match &s.shape {
                            egui::Shape::Rect(r) if r.rect == rect => Some(r),
                            _ => None,
                        })
                        .unwrap();
                    let opacity = if dim { 0.42 } else { 1.0 };
                    assert_eq!(background.fill, if error { t.form_error_fill } else { accent.soft }.gamma_multiply(opacity));
                    if style == HistoryStyle::Icons || error {
                        assert_eq!(background.stroke.color, if error { t.form_error } else { accent.color }.gamma_multiply(opacity));
                    }
                    if style == HistoryStyle::Names {
                        let text = out
                            .shapes
                            .iter()
                            .find_map(|s| match &s.shape {
                                egui::Shape::Text(t) => Some(t),
                                _ => None,
                            })
                            .unwrap();
                        assert_eq!(text.galley.text(), "Offset1");
                        assert_eq!(text.fallback_color, if selected { accent.ink } else { t.text }.gamma_multiply(opacity));
                    }
                }
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn chips_have_no_connector_shapes_in_either_theme_or_width() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let t = Tokens::get();
            for style in [HistoryStyle::Bottom, HistoryStyle::Icons, HistoryStyle::Names] {
                let ctx = crate::test_support::context();
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    for index in 0..3 {
                        let rect = Rect::from_min_size(
                            pos2(20.0, 20.0 + index as f32 * (style.height() + t.space_4)),
                            vec2(if style == HistoryStyle::Names { 158.0 } else { style.height() }, style.height()),
                        );
                        HistoryChip {
                            icon: "box",
                            name: "Box",
                            icon_accent: None,
                            selected: index == 0,
                            hovered: false,
                            dim: index == 2,
                            suppressed: false,
                            error: false,
                            warning: false,
                            reference: false,
                            accent: TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Main),
                            ink: None,
                        }
                        .paint(ui.painter(), rect, style);
                    }
                });
                out.textures_delta.clear();
                let mut connections = 0;
                for clipped in out.shapes {
                    crate::test_support::each_shape(&clipped.shape, &mut |shape| {
                        if matches!(shape, egui::Shape::LineSegment { .. } | egui::Shape::Path(_)) {
                            connections += 1;
                        }
                    });
                }
                assert_eq!(connections, 0, "dark={dark}");
            }
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn both_strip_orientations_share_the_drop_bar_and_marker_geometry() {
        let t = Tokens::get();
        let accent = TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Surface);
        for vertical in [false, true] {
            let rect = Rect::from_min_size(pos2(20.0, 20.0), if vertical { vec2(160.0, 12.0) } else { vec2(12.0, 28.0) });
            let ctx = crate::test_support::context();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| history_drop_bar(ui.painter(), rect, vertical, accent.color));
            out.textures_delta.clear();
            let lines: Vec<_> = out
                .shapes
                .iter()
                .filter_map(|s| if let egui::Shape::LineSegment { points, stroke } = s.shape { Some((points, stroke)) } else { None })
                .collect();
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].1, Stroke::new(t.space_2, accent.color));
            assert_eq!(lines[0].0, if vertical { [rect.left_center(), rect.right_center()] } else { [rect.center_top(), rect.center_bottom()] });
        }
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            let t = Tokens::get();
            let accent = TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Surface);
            // Both vertical widths retain the 12 pt marker row; the bottom strip rotates it.
            for (vertical, size) in [(true, vec2(46.0, 12.0)), (true, vec2(158.0, 12.0)), (false, vec2(12.0, 28.0))] {
                let rect = Rect::from_min_size(pos2(20.0, 20.0), size);
                let ctx = crate::test_support::context();
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| history_marker(ui.painter(), rect, vertical, accent));
                out.textures_delta.clear();
                let lines: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match s.shape {
                        egui::Shape::LineSegment { points, stroke } => Some((points, stroke)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].1, Stroke::new(2.0, accent.color));
                let circles: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Circle(c) => Some(c),
                        _ => None,
                    })
                    .collect();
                assert_eq!(circles.len(), 1);
                let circle = circles[0];
                assert_eq!(circle.stroke, Stroke::new(2.0, accent.color));
                assert_eq!(circle.fill, if vertical { t.panel } else { t.timeline });
                assert_ne!(circle.fill, accent.color);
                let outer = circle.radius + circle.stroke.width / 2.0;
                assert_eq!(outer * 2.0, 10.0);
                let bounds = Rect::from_center_size(circle.center, vec2(outer * 2.0, outer * 2.0));
                assert!(rect.contains_rect(bounds));
                assert_eq!(lines[0].0[0], if vertical { rect.left_center() } else { rect.center_top() });
                if vertical {
                    assert_eq!(rect.height(), 12.0);
                    assert_eq!(bounds.right(), rect.right());
                    assert_eq!(lines[0].0[1], pos2(bounds.left() + 1.0, circle.center.y));
                } else {
                    assert_eq!(bounds.bottom(), rect.bottom());
                    assert_eq!(lines[0].0[1], pos2(circle.center.x, bounds.top() + 1.0));
                }
                assert!(!out.shapes.iter().any(|s| matches!(s.shape, egui::Shape::Path(_))), "no arrowheads");
            }
        }
        solidcraft_theme::set_dark(previous);
    }
}
