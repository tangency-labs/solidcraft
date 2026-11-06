//! Navigation chrome and measurements shared by the viewport and Preferences editor.
use crate::{
    icons,
    navigation_bar::{Idle, Kind, Size},
    theme::Tokens,
};
use egui::{Rect, Response, Sense, Ui, pos2, vec2};

pub(crate) const NAV_BORDER: f32 = 1.0;
pub(crate) const NAV_PADDING: f32 = 3.0;
pub(crate) const NAV_GAP: f32 = 2.0;
pub(crate) const NAV_BOTTOM: f32 = 7.0;
pub(crate) const NAV_EDGE: f32 = 12.0;
pub(crate) const NAV_SEPARATOR: f32 = 9.0;
pub(crate) const NAV_SEPARATOR_HEIGHT: f32 = 18.0;
pub(crate) const NAV_MENU_LEFT: f32 = 7.0;
pub(crate) const NAV_MENU_RIGHT: f32 = 6.0;
pub(crate) const NAV_MENU_GAP: f32 = 5.0;
pub(crate) const NAV_CHEVRON: f32 = 12.0;
pub(crate) const NAV_TEXT: f32 = 12.5;
pub(crate) const NAV_LABEL_GAP: f32 = 6.0;
pub(crate) const NAV_LABEL_PADDING: f32 = 7.0;
pub(crate) const NAV_POPUP_WIDTH: f32 = 244.0;
pub(crate) const NAV_CONTEXT_WIDTH: f32 = 232.0;
pub(crate) const NAV_MORE_WIDTH: f32 = 200.0;
pub(crate) const NAV_POPUP_GAP: f32 = 6.0;
pub(crate) const NAV_POPUP_PADDING: i8 = 5;
pub(crate) const NAV_MENU_ROW: f32 = 30.0;
pub(crate) const NAV_MENU_RADIUS: u8 = 6;
pub(crate) const NAV_MENU_HEADING: f32 = 22.0;
pub(crate) const NAV_HEADING_TEXT: f32 = 11.0;
pub(crate) const NAV_HEADING_TRACKING: f32 = 0.07;
pub(crate) const NAV_MENU_ICON: f32 = 17.0;
pub(crate) const NAV_MENU_INSET: f32 = 8.0;
pub(crate) const NAV_ROW_GAP: f32 = 9.0;
pub(crate) const NAV_FOCUS_WIDTH: f32 = 2.0;
pub(crate) const NAV_FOCUS_OFFSET: f32 = 1.0;
pub(crate) const NAV_IDLE_OPACITY: f32 = 0.45;
pub(crate) const NAV_HIDDEN_OPACITY: f32 = 0.4;
pub(crate) const NAV_FADE_SECONDS: f32 = 0.12;
pub(crate) const NAV_TOOLTIP_DELAY: f64 = 0.8;
pub(crate) const NAV_TOOLTIP_KEY_LINE_HEIGHT: f32 = 15.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct NavigationMetrics {
    pub button: f32,
    pub icon: f32,
    pub radius: u8,
    pub button_radius: u8,
}
impl NavigationMetrics {
    pub(crate) fn for_size(size: Size) -> Self {
        match size {
            Size::Regular => Self { button: 32.0, icon: 18.0, radius: 10, button_radius: 7 },
            Size::Compact => Self { button: 26.0, icon: 16.0, radius: 9, button_radius: 6 },
        }
    }
    pub(crate) fn height(self) -> f32 {
        self.button + 2.0 * (NAV_BORDER + NAV_PADDING)
    }
}

pub(crate) struct NavigationButton<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub icon: &'a str,
    pub selected: bool,
    pub kind: Kind,
    pub labels: bool,
    pub menu: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavigationFill {
    None,
    Hover,
    Press,
    Active,
}

/// Actions never latch, and keyboard focus has its own outline rather than a fill.
pub(crate) fn navigation_visual(kind: Kind, active: bool, hovered: bool, pressed: bool, keyboard_focus: bool) -> (NavigationFill, bool) {
    let fill = if pressed || (active && kind == Kind::Menu) {
        NavigationFill::Press
    } else if active && matches!(kind, Kind::Tool | Kind::Toggle) {
        NavigationFill::Active
    } else if hovered {
        NavigationFill::Hover
    } else {
        NavigationFill::None
    };
    (fill, keyboard_focus)
}

pub(crate) fn navigation_fill(t: &Tokens, fill: NavigationFill) -> Option<egui::Color32> {
    match fill {
        NavigationFill::None => None,
        NavigationFill::Hover => Some(t.nav_hover),
        NavigationFill::Press => Some(t.nav_press),
        NavigationFill::Active => Some(t.selected_bg),
    }
}

/// The label and icon colour of a slot. An active slot keeps the accent ink on its selected fill
/// where that reads; a theme whose selected fill is as dark as its accent (SolidCraft 95's navy)
/// gets the theme's selected-text ink instead, so an active toggle never turns into a blank block.
pub(crate) fn navigation_ink(t: &Tokens, fill: NavigationFill) -> egui::Color32 {
    if fill != NavigationFill::Active {
        return t.form_ink;
    }
    if crate::theme::contrast_ratio(t.form_primary, t.selected_bg) >= NAV_ACTIVE_MIN_CONTRAST { t.form_primary } else { t.selected_ink }
}

/// WCAG small-text contrast for an active slot's label on its selected fill.
pub(crate) const NAV_ACTIVE_MIN_CONTRAST: f64 = 4.5;

pub(crate) fn navigation_opacity(idle: Idle, active: bool) -> f32 {
    match idle {
        _ if active => 1.0,
        Idle::Always => 1.0,
        Idle::Fade => NAV_IDLE_OPACITY,
        Idle::Hide => 0.0,
    }
}

pub(crate) fn navigation_button(ui: &mut Ui, rect: Rect, button: NavigationButton<'_>) -> Response {
    navigation_button_editable(ui, rect, button, false)
}

pub(crate) fn navigation_button_editable(ui: &mut Ui, rect: Rect, button: NavigationButton<'_>, editing: bool) -> Response {
    let t = Tokens::get();
    let metrics =
        NavigationMetrics::for_size(if rect.height() < NavigationMetrics::for_size(Size::Regular).button { Size::Compact } else { Size::Regular });
    let response = ui.interact(rect, ui.make_persistent_id(("nav_slot", button.id)), if editing { Sense::click_and_drag() } else { Sense::click() });
    let pressed = response.is_pointer_button_down_on();
    if pressed || response.clicked_by(egui::PointerButton::Primary) {
        response.surrender_focus();
    }
    let (fill, focus) = navigation_visual(button.kind, button.selected, response.hovered(), pressed, response.has_focus());
    if let Some(fill) = navigation_fill(&t, fill) {
        ui.painter().rect_filled(rect, metrics.button_radius, fill);
    }
    if focus {
        ui.painter().rect_stroke(
            rect.expand(NAV_FOCUS_OFFSET + NAV_FOCUS_WIDTH),
            metrics.button_radius.saturating_add((NAV_FOCUS_OFFSET + NAV_FOCUS_WIDTH) as u8),
            egui::Stroke::new(NAV_FOCUS_WIDTH, t.form_primary),
            egui::StrokeKind::Inside,
        );
    }
    let ink = navigation_ink(&t, fill);
    let text = button.labels.then(|| ui.painter().layout_no_wrap(button.label.to_owned(), crate::fonts::medium(NAV_TEXT), ink));
    let mut x = if button.menu {
        rect.left() + NAV_MENU_LEFT + if editing { NAV_BORDER } else { 0.0 }
    } else {
        rect.center().x - (metrics.icon + text.as_ref().map_or(0.0, |g| NAV_LABEL_GAP + g.size().x)) / 2.0
    };
    icons::paint_ink(
        ui.painter(),
        Rect::from_center_size(pos2(x + metrics.icon / 2.0, rect.center().y), vec2(metrics.icon, metrics.icon)),
        button.icon,
        ink,
    );
    x += metrics.icon;
    if let Some(text) = text {
        x += if button.menu { NAV_MENU_GAP } else { NAV_LABEL_GAP };
        ui.painter().galley(pos2(x, rect.center().y - text.size().y / 2.0), text, ink);
    }
    if button.menu {
        let chevron = Rect::from_center_size(
            pos2(rect.right() - NAV_MENU_RIGHT - if editing { NAV_BORDER } else { 0.0 } - NAV_CHEVRON / 2.0, rect.center().y),
            vec2(NAV_CHEVRON, NAV_CHEVRON),
        );
        icons::paint_ink_flipped(ui.painter(), chevron, "chevron", t.form_dim);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), button.selected, button.label));
    response
}

pub(crate) fn navigation_separator(ui: &Ui, rect: Rect) {
    let t = Tokens::get();
    ui.painter().vline(
        rect.center().x,
        rect.center().y - NAV_SEPARATOR_HEIGHT / 2.0..=rect.center().y + NAV_SEPARATOR_HEIGHT / 2.0,
        egui::Stroke::new(NAV_BORDER, t.control_border),
    );
}

pub(crate) fn navigation_edit_outline(ui: &Ui, rect: Rect, hidden: bool, selected: bool) {
    let t = Tokens::get();
    let metrics =
        NavigationMetrics::for_size(if rect.height() < NavigationMetrics::for_size(Size::Regular).button { Size::Compact } else { Size::Regular });
    let mut painter = ui.painter().clone();
    if hidden {
        painter.multiply_opacity(NAV_HIDDEN_OPACITY);
        let mut points = Vec::new();
        egui::epaint::tessellator::path::rounded_rectangle(
            &mut points,
            egui::epaint::RoundedRect::new(
                rect.shrink(NAV_BORDER / 2.0),
                egui::epaint::CornerRadiusF32::same(f32::from(metrics.button_radius) - NAV_BORDER / 2.0),
            ),
        );
        if let Some(first) = points.first().copied() {
            points.push(first);
        }
        painter.extend(egui::Shape::dashed_line(&points, egui::Stroke::new(NAV_BORDER, t.control_border), t.space_4, t.space_3));
    }
    if selected {
        painter.rect_stroke(
            rect.expand(NAV_FOCUS_WIDTH),
            metrics.button_radius.saturating_add(NAV_FOCUS_WIDTH as u8),
            egui::Stroke::new(NAV_FOCUS_WIDTH, t.form_primary),
            egui::StrokeKind::Inside,
        );
    }
}

pub(crate) fn navigation_surface(ui: &Ui, rect: Rect, size: Size, editing: bool) {
    let t = Tokens::get();
    let frame = super::Container::new(ui.ctx(), super::Surface::Navigation)
        .frame(ui.ctx(), t.dialog_body, crate::frame::Side::Left)
        .fill(if editing { t.dialog_body } else { t.dialog_body.gamma_multiply(0.97) })
        .stroke(egui::Stroke::new(NAV_BORDER, t.dialog_border))
        .corner_radius(NavigationMetrics::for_size(size).radius);
    super::Container::paint_frame(ui.painter(), frame, rect.shrink(NAV_BORDER));
}

#[cfg(test)]
mod active_ink_tests {
    use super::{NavigationFill, navigation_fill, navigation_ink};

    #[test]
    fn active_navigation_slots_stay_readable_in_every_built_in_theme() {
        let mut themes = vec![("Dark", crate::theme::Tokens::dark()), ("Light", crate::theme::Tokens::light())];
        themes.extend(crate::theme_files::builtin().iter().map(|theme| (theme.name.as_str(), crate::theme_files::resolve(theme))));
        let mut failures = Vec::new();
        for (name, tokens) in themes {
            let Some(fill) = navigation_fill(&tokens, NavigationFill::Active) else { continue };
            let ink = navigation_ink(&tokens, NavigationFill::Active);
            let ratio = crate::theme::contrast_ratio(ink, fill);
            if ratio < 3.0 {
                failures.push(format!("{name}: {ratio:.2}:1 ({ink:?} on {fill:?})"));
            }
        }
        assert!(failures.is_empty(), "active navigation slot unreadable:\n{}", failures.join("\n"));
    }
}
