//! Shared history chips, dense name rows, drop bars and rollback markers.
use crate::{
    icons,
    theme::{TabAccent, Tokens},
};
use egui::{Painter, Rect, Stroke, pos2, vec2};

/// Browser and named history rows use the same density.
pub(crate) const TREE_ROW_HEIGHT: f32 = 26.0;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum HistoryStyle {
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

pub(crate) struct HistoryChip<'a> {
    pub icon: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    pub selected: bool,
    pub hovered: bool,
    pub dim: bool,
    pub suppressed: bool,
    pub error: bool,
    pub warning: bool,
    pub accent: TabAccent,
}

/// Share the feature classification across chips and name rows. Surface aliases also
/// cover Patch/Stitch/Thicken and other surface features without a Surface-prefixed kind.
fn feature_icon_accent(kind: &str, icon: &str) -> Option<&'static str> {
    let tab = if kind.starts_with("Sketch") {
        "SKETCH"
    } else if kind.starts_with("Surface") || icon.starts_with("surface_") || icon == "continuity" {
        "SURFACE"
    } else if kind.starts_with("Mesh") {
        "MESH"
    } else if kind.starts_with("Form") {
        "FORM"
    } else {
        "MODEL"
    };
    crate::theme::icon_accent_for_tab(tab)
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
        icons::paint_accented(&p, icon, self.icon, feature_icon_accent(self.kind, self.icon), 1.0);
        if names {
            let ink = if self.selected { self.accent.ink } else { t.text };
            let font = if self.selected { crate::fonts::semibold(t.font_body) } else { egui::FontId::proportional(t.font_body) };
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
pub(crate) fn history_drop_bar(p: &Painter, rect: Rect, vertical: bool, colour: egui::Color32) {
    let points = if vertical {
        [pos2(rect.left(), rect.center().y), pos2(rect.right(), rect.center().y)]
    } else {
        [pos2(rect.center().x, rect.top()), pos2(rect.center().x, rect.bottom())]
    };
    p.line_segment(points, Stroke::new(Tokens::get().space_2, colour));
}

/// Rollback line and hollow end circle; a vertical strip uses a horizontal line.
pub(crate) fn history_marker(p: &Painter, rect: Rect, vertical: bool, accent: TabAccent) {
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

/// The same four rollback controls are used in every History orientation.
#[derive(Clone, Copy)]
pub(crate) enum HistoryTransport {
    First,
    Back,
    Forward,
    Last,
}

impl HistoryTransport {
    pub const ALL: [Self; 4] = [Self::First, Self::Back, Self::Forward, Self::Last];

    pub fn id(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::Last => "last",
        }
    }

    fn label(self) -> &'static str {
        crate::i18n::tr(match self {
            Self::First => "ui.history_first",
            Self::Back => "ui.history_back",
            Self::Forward => "ui.history_forward",
            Self::Last => "ui.history_last",
        })
    }

    pub fn show(self, ui: &mut egui::Ui, rect: Rect) -> egui::Response {
        let t = Tokens::get();
        let response = ui.interact(rect, ui.id().with(("history_transport", self.id())), egui::Sense::click());
        crate::scenario::publish_target(&format!("history:{}", self.id()), rect);
        if response.hovered() {
            ui.painter().rect_filled(rect, t.radius_sm, t.hover);
        }
        icons::glyphs::transport_glyph(ui.painter(), rect.center(), self.id(), t.icon);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label()));
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(self.label())
    }
}

pub(crate) fn history_transport_width() -> f32 {
    let t = Tokens::get();
    t.space_24 * HistoryTransport::ALL.len() as f32 + t.space_2 * (HistoryTransport::ALL.len() - 1) as f32
}

/// Thin and minimum named widths; wider named panels retain their measured width.
pub(crate) fn history_width(names: bool) -> f32 {
    let t = Tokens::get();
    if names { history_transport_width() + t.space_10 * 2.0 + t.space_8 } else { t.control_height * 2.0 }
}

pub(crate) fn history_clamp_width(width: f32) -> f32 {
    if width.is_finite() { width.clamp(history_width(false), Tokens::get().space_24 * 26.0) } else { history_width(false) }
}

/// Only the gap between thin and medium snaps. Every larger width is retained.
pub(crate) fn history_snap_width(width: f32) -> f32 {
    let width = history_clamp_width(width);
    if width < (history_width(false) + history_width(true)) / 2.0 { history_width(false) } else { width.max(history_width(true)) }
}

/// The same six-dot grip is used by Assistant and History headers.
pub(crate) fn panel_grip(ui: &mut egui::Ui) -> egui::Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(t.space_10, t.space_16 + t.space_2), egui::Sense::hover());
    for row in 0..3 {
        for col in 0..2 {
            ui.painter().circle_filled(rect.left_top() + vec2(2.5 + col as f32 * 5.0, 4.0 + row as f32 * 5.0), 1.1, t.text_dim);
        }
    }
    response
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HistoryHandleState {
    Rest,
    Hover,
    Drag,
}

/// Seam geometry is independent of the strip's contents and consumes no layout space.
pub(crate) struct HistorySeam {
    pub grip: Rect,
    pub pill: Rect,
    pub edge: Rect,
}

impl HistorySeam {
    pub fn new(card: Rect) -> Self {
        let t = Tokens::get();
        Self {
            grip: Rect::from_center_size(pos2(card.right() - t.space_2 - t.space_3 / 2.0, card.center().y), vec2(t.space_3, TREE_ROW_HEIGHT)),
            pill: Rect::from_center_size(card.right_center(), vec2(t.space_16 + t.space_2, t.control_height + t.space_16)),
            edge: Rect::from_min_max(pos2(card.right() - t.space_10, card.top()), card.right_bottom()),
        }
    }

    pub fn paint(&self, p: &Painter, card: Rect, names: bool, state: HistoryHandleState) {
        let t = Tokens::get();
        let accent = TabAccent::current(p.ctx());
        match state {
            HistoryHandleState::Rest => {
                p.rect_filled(self.grip, t.space_2, t.control_border);
            }
            HistoryHandleState::Hover => {
                p.rect(self.pill, self.pill.width() / 2.0, accent.soft, Stroke::new(1.0, accent.color), egui::StrokeKind::Inside);
                let c = self.pill.center();
                let dir = if names { -1.0 } else { 1.0 };
                p.add(egui::Shape::line(
                    vec![c + vec2(-dir * t.space_2, -t.space_4), c + vec2(dir * t.space_2, 0.0), c + vec2(-dir * t.space_2, t.space_4)],
                    Stroke::new(t.space_2, accent.color),
                ));
            }
            HistoryHandleState::Drag => {
                p.rect_stroke(card, t.radius_lg, Stroke::new(1.0, accent.color), egui::StrokeKind::Inside);
            }
        }
    }
}

pub(crate) struct HistorySeamResponse {
    pub toggle: bool,
    pub width: Option<f32>,
    pub released: bool,
    #[cfg(test)]
    pub response: egui::Response,
}

/// Focusable seam button and edge gesture, shared by the panel and CPU interaction tests.
pub(crate) fn history_seam(ui: &mut egui::Ui, card: Rect, names: bool) -> HistorySeamResponse {
    let seam = HistorySeam::new(card);
    let id = ui.id().with("history_seam");
    let edge = ui.interact(seam.edge, id.with("edge"), egui::Sense::CLICK | egui::Sense::DRAG);
    let button = ui.interact(seam.pill, id, egui::Sense::click_and_drag());
    let label = crate::i18n::tr(if names { "ui.collapse" } else { "ui.expand" });
    button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if button.clicked() {
        button.request_focus();
    }
    let drag = if button.dragged() || button.drag_started() || button.drag_stopped() { &button } else { &edge };
    let start_id = id.with("start_width");
    if drag.drag_started() {
        let press_x = ui.input(|i| i.pointer.press_origin()).map_or(card.right(), |p| p.x);
        ui.data_mut(|d| d.insert_temp(start_id, (card.width(), press_x)));
    }
    let width = if drag.dragged() || drag.drag_stopped() {
        let (start_width, press_x) = ui.data(|d| d.get_temp::<(f32, f32)>(start_id)).unwrap_or((card.width(), card.right()));
        let pointer_x = ui.input(|i| i.pointer.latest_pos()).map_or(press_x, |p| p.x);
        Some(history_clamp_width(start_width + pointer_x - press_x))
    } else {
        None
    };
    if drag.drag_stopped() {
        ui.data_mut(|d| d.remove::<(f32, f32)>(start_id));
    }
    let dragging = width.is_some();
    if dragging || edge.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if !dragging && button.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let hovered = edge.hovered() || button.hovered();
    let state = if dragging {
        HistoryHandleState::Drag
    } else if hovered {
        HistoryHandleState::Hover
    } else {
        HistoryHandleState::Rest
    };
    seam.paint(ui.painter(), card, names, state);
    let response = button.clone().union(edge.clone());
    if hovered && !dragging {
        let mut tip = egui::Tooltip::for_widget(&response);
        let frame = super::Container::new(ui.ctx(), super::Surface::Toast).tooltip_frame(ui.ctx());
        tip.popup = tip.popup.anchor(seam.pill).align(egui::RectAlign::RIGHT_START).frame(frame);
        tip.show(|ui| {
            let t = Tokens::get();
            ui.label(egui::RichText::new(label).font(crate::fonts::semibold(t.font_body)).color(t.text));
            ui.label(egui::RichText::new(crate::i18n::tr("ui.history_edge_hint")).size(t.font_small).color(t.text_dim));
        });
    }
    crate::scenario::publish_handle("history:names", seam.pill.center());
    crate::scenario::publish_rect("history:seam", seam.edge);
    crate::scenario::publish_rect("history:handle", seam.pill);
    // egui's click response includes Enter and Space for a focused button.
    HistorySeamResponse {
        toggle: button.clicked() || edge.double_clicked(),
        width,
        released: drag.drag_stopped(),
        #[cfg(test)]
        response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seam_geometry_and_paint_states_follow_tokens_in_both_themes() {
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            for names in [false, true] {
                let card = Rect::from_min_size(pos2(20.0, 30.0), vec2(history_width(names), 280.0));
                let seam = HistorySeam::new(card);
                assert_eq!(seam.grip.size(), vec2(3.0, 26.0));
                assert_eq!(seam.grip.center().y, card.center().y);
                assert_eq!(seam.grip.right(), card.right() - 2.0);
                assert_eq!(seam.pill.size(), vec2(18.0, 44.0));
                assert_eq!(seam.pill.center(), card.right_center());
                assert_eq!(seam.edge.width(), 10.0);
                for state in [HistoryHandleState::Rest, HistoryHandleState::Hover, HistoryHandleState::Drag] {
                    let ctx = crate::fonts::test_context();
                    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| seam.paint(ui.painter(), card, names, state));
                    out.textures_delta.clear();
                    let t = Tokens::get();
                    let accent = TabAccent::current(&ctx);
                    let rect = out
                        .shapes
                        .iter()
                        .find_map(|s| match &s.shape {
                            egui::Shape::Rect(r) => Some(r),
                            _ => None,
                        })
                        .unwrap();
                    match state {
                        HistoryHandleState::Rest => {
                            assert_eq!(rect.rect, seam.grip);
                            assert_eq!(rect.fill, t.control_border);
                        }
                        HistoryHandleState::Hover => {
                            assert_eq!(rect.rect, seam.pill);
                            assert_eq!(rect.fill, accent.soft);
                            assert_eq!(rect.stroke.color, accent.color);
                        }
                        HistoryHandleState::Drag => {
                            assert_eq!(rect.rect, card);
                            assert_eq!(rect.stroke.color, accent.color);
                            assert!(!out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(_))));
                            assert!(!out.shapes.iter().any(|s| matches!(s.shape, egui::Shape::LineSegment { .. })));
                        }
                    }
                }
            }
        }
        crate::theme::set_dark(previous);
    }

    struct SeamHarness {
        ctx: egui::Context,
        names: bool,
        width: f32,
        time: f64,
        focused: bool,
    }

    impl SeamHarness {
        fn new(names: bool) -> Self {
            let mut h = Self { ctx: crate::fonts::test_context(), names, width: history_width(names), time: 0.0, focused: false };
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }

        fn card(&self) -> Rect {
            Rect::from_min_size(pos2(20.0, 20.0), vec2(self.width, 280.0))
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            self.time += 0.05;
            let card = self.card();
            let mut out = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
                    time: Some(self.time),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| {
                    let result = history_seam(ui, card, self.names);
                    self.focused = result.response.has_focus();
                    if let Some(width) = result.width {
                        self.width = width;
                        if result.released {
                            self.names = history_snap_width(width) >= history_width(true);
                            self.width = history_snap_width(width);
                        }
                    } else if result.toggle {
                        self.names = !self.names;
                        self.width = history_width(self.names);
                    }
                },
            );
            out.textures_delta.clear();
            out
        }
    }

    #[test]
    fn seam_edge_cursor_pill_click_double_click_and_keyboard_toggle() {
        use crate::test_support::{key, pointer};
        let mut h = SeamHarness::new(false);
        let edge = pos2(h.card().right() - 4.0, h.card().top() + 30.0);
        assert_eq!(h.frame(vec![egui::Event::PointerMoved(edge)]).platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
        let pill = h.card().right_center();
        assert_eq!(h.frame(vec![egui::Event::PointerMoved(pill)]).platform_output.cursor_icon, egui::CursorIcon::PointingHand);
        h.frame(pointer(pill, true));
        h.frame(pointer(pill, false));
        assert!(h.names);
        let mut h = SeamHarness::new(false);
        for _ in 0..2 {
            h.frame(pointer(edge, true));
            h.frame(pointer(edge, false));
        }
        assert!(h.names, "double-click toggles once");
        let mut h = SeamHarness::new(false);
        h.ctx.enable_accesskit();
        let focus = h.frame(key(egui::Key::Tab));
        assert!(
            focus
                .platform_output
                .events
                .iter()
                .any(|e| matches!(e, egui::output::OutputEvent::FocusGained(info) if info.label.as_deref() == Some("Expand")))
        );
        h.frame(vec![]);
        assert!(h.focused, "Tab reaches the seam button");
        h.frame(key(egui::Key::Enter));
        assert!(h.names);
        h.frame(key(egui::Key::Space));
        assert!(!h.names);
    }

    #[test]
    fn seam_drag_resizes_continuously_and_snaps_both_directions() {
        use crate::test_support::pointer;
        for (names, delta, expected) in [(false, 40.0, true), (false, 20.0, false), (true, -40.0, false), (true, -20.0, true)] {
            for pill in [false, true] {
                let mut h = SeamHarness::new(names);
                let start = if pill { h.card().right_center() } else { pos2(h.card().right() - 4.0, h.card().top() + 30.0) };
                let end = start + vec2(delta, 0.0);
                h.frame(pointer(start, true));
                h.frame(vec![egui::Event::PointerMoved(start + vec2(delta / 2.0, 0.0))]);
                h.frame(vec![egui::Event::PointerMoved(end)]);
                assert_eq!(h.width, history_width(names) + delta, "continuous width before release");
                h.frame(pointer(end, false));
                assert_eq!(h.names, expected);
                assert_eq!(h.width, history_snap_width(history_width(names) + delta));
            }
        }
        for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1000.0, 10000.0] {
            assert!((history_width(false)..=Tokens::get().space_24 * 26.0).contains(&history_clamp_width(hostile)));
        }
    }

    #[test]
    fn chip_selection_error_and_rollback_use_tokens_in_both_themes() {
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            let t = Tokens::get();
            let accent = TabAccent::for_tab(&t, "SURFACE");
            for style in [HistoryStyle::Icons, HistoryStyle::Names] {
                for (selected, error, dim) in [(true, false, false), (false, true, false), (true, false, true)] {
                    let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(158.0, style.height()));
                    let ctx = crate::fonts::test_context();
                    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                        HistoryChip {
                            icon: "surface_extrude",
                            name: "Offset1",
                            kind: "SurfaceOffsetFeature",
                            selected,
                            hovered: false,
                            dim,
                            suppressed: false,
                            error,
                            warning: false,
                            accent,
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
        crate::theme::set_dark(previous);
    }

    #[test]
    fn chips_have_no_connector_shapes_in_either_theme_or_width() {
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            let t = Tokens::get();
            for style in [HistoryStyle::Bottom, HistoryStyle::Icons, HistoryStyle::Names] {
                let ctx = crate::fonts::test_context();
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    for index in 0..3 {
                        let rect = Rect::from_min_size(
                            pos2(20.0, 20.0 + index as f32 * (style.height() + t.space_4)),
                            vec2(if style == HistoryStyle::Names { 158.0 } else { style.height() }, style.height()),
                        );
                        HistoryChip {
                            icon: "box",
                            name: "Box",
                            kind: "BoxFeature",
                            selected: index == 0,
                            hovered: false,
                            dim: index == 2,
                            suppressed: false,
                            error: false,
                            warning: false,
                            accent: TabAccent::for_tab(&t, "MODEL"),
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
        crate::theme::set_dark(previous);
    }

    #[test]
    fn both_strip_orientations_share_the_drop_bar_and_marker_geometry() {
        let t = Tokens::get();
        let accent = TabAccent::for_tab(&t, "SURFACE");
        for vertical in [false, true] {
            let rect = Rect::from_min_size(pos2(20.0, 20.0), if vertical { vec2(160.0, 12.0) } else { vec2(12.0, 28.0) });
            let ctx = crate::fonts::test_context();
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
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            let t = Tokens::get();
            let accent = TabAccent::for_tab(&t, "SURFACE");
            // Both vertical widths retain the 12 pt marker row; the bottom strip rotates it.
            for (vertical, size) in [(true, vec2(46.0, 12.0)), (true, vec2(158.0, 12.0)), (false, vec2(12.0, 28.0))] {
                let rect = Rect::from_min_size(pos2(20.0, 20.0), size);
                let ctx = crate::fonts::test_context();
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
        crate::theme::set_dark(previous);
    }
}
