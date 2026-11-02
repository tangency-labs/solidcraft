//! Parts library cards: a part's picture on the tile ground with its name under it, a folder with
//! how many parts it holds, a names-only row, a placeholder while a folder is read, and the folder
//! button that starts the library's breadcrumb. Tokens only; the Model accent marks a selection.
use egui::{Response, Sense, TextureId, Ui, vec2};

use crate::theme::{TabAccent, Tokens};

/// The library's card sizes: three Small, two Medium or one Large across the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartCardSize {
    Small,
    Medium,
    Large,
}

impl PartCardSize {
    /// The card's width in a list `room` wide: its columns and the gaps between them.
    pub(crate) fn width(self, room: f32) -> f32 {
        let gaps = Tokens::get().space_8 * (self.columns() - 1) as f32;
        ((room - gaps) / self.columns() as f32).max(Tokens::get().space_24)
    }

    /// The picture's height.
    pub(crate) fn picture(self) -> f32 {
        match self {
            Self::Small => 56.0,
            Self::Medium => 88.0,
            Self::Large => 150.0,
        }
    }

    pub(crate) fn columns(self) -> usize {
        match self {
            Self::Small => 3,
            Self::Medium => 2,
            Self::Large => 1,
        }
    }

    fn font(self) -> f32 {
        match self {
            Self::Small => Tokens::get().font_panel,
            Self::Medium | Self::Large => Tokens::get().font_caption,
        }
    }

    /// The card's whole height: the picture and one line of name.
    pub(crate) fn height(self) -> f32 {
        self.picture() + Tokens::get().space_4 + self.font() + Tokens::get().space_4
    }
}

/// What a card shows.
#[derive(Clone, Copy)]
pub(crate) enum PartFace<'a> {
    /// A part: its picture once drawn (`None` while it is drawn), and its kind where it is not a
    /// SolidCraft design (STEP, MESH).
    Part { picture: Option<TextureId>, badge: Option<&'a str> },
    /// A folder, with "N parts".
    Folder { count: &'a str },
    /// A card at its final size while the folder is read, so nothing jumps.
    Placeholder,
}

pub(crate) struct PartCard<'a> {
    pub name: &'a str,
    pub face: PartFace<'a>,
    pub size: PartCardSize,
    /// The list's width, which the size's columns share.
    pub room: f32,
    pub selected: bool,
    /// The card being dragged into the view: its picture fades.
    pub dragging: bool,
}

/// One card. The response clicks and drags; the full name is its tooltip.
pub(crate) fn part_card(ui: &mut Ui, card: PartCard<'_>) -> Response {
    let t = Tokens::get();
    let size = card.size;
    let sense = if matches!(card.face, PartFace::Placeholder) { Sense::hover() } else { Sense::click_and_drag() };
    let width = size.width(card.room);
    let (rect, response) = ui.allocate_exact_size(vec2(width, size.height()), sense);
    let picture = egui::Rect::from_min_size(rect.min, vec2(width, size.picture()));
    let accent = TabAccent::for_tab(&t, "MODEL");
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect_filled(picture, t.radius_md, t.dialog_footer);
    if card.selected {
        painter.rect_filled(picture, t.radius_md, accent.soft);
    }
    let edge = if card.selected {
        t.primary_border(accent.color)
    } else if response.hovered() && !matches!(card.face, PartFace::Placeholder) {
        t.control_border_stroke(t.form_dim)
    } else {
        t.control_border_stroke(t.control_border)
    };
    painter.rect_stroke(picture, t.radius_md, edge, egui::StrokeKind::Inside);
    let inner = picture.shrink(t.space_4);
    match card.face {
        PartFace::Part { picture: Some(texture), .. } => {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            if card.dragging {
                child.set_opacity(0.45);
            }
            egui::Image::new((texture, inner.size())).maintain_aspect_ratio(true).fit_to_exact_size(inner.size()).paint_at(&child, inner);
        }
        PartFace::Part { picture: None, .. } | PartFace::Placeholder => {
            painter.hline(inner.shrink(t.space_8).x_range(), inner.center().y, t.control_border_stroke(t.control_border));
        }
        PartFace::Folder { count } => {
            let icon = (size.picture() * 0.4).clamp(t.space_16, t.space_24 * 2.0);
            let at = egui::Rect::from_center_size(inner.center() - vec2(0.0, t.space_6), vec2(icon, icon));
            crate::icons::paint_ink(&painter, at, "folder", accent.color);
            let line = egui::Rect::from_min_max(egui::pos2(inner.left(), at.bottom() + t.space_2), egui::pos2(inner.right(), inner.bottom()));
            painter.text(line.center_top(), egui::Align2::CENTER_TOP, count, egui::FontId::proportional(t.font_caption), t.text_dim);
        }
    }
    if let PartFace::Part { badge: Some(badge), .. } = card.face {
        pill(&painter, picture.left_top() + vec2(t.space_4, t.space_4), badge, accent.color);
    }
    let name = egui::Rect::from_min_max(egui::pos2(rect.left(), picture.bottom() + t.space_4), rect.max);
    if matches!(card.face, PartFace::Placeholder) {
        let bar = egui::Rect::from_min_size(name.min + vec2(0.0, size.font() * 0.25), vec2(name.width() * 0.6, size.font() * 0.6));
        painter.rect_filled(bar, t.radius_sm, t.dialog_footer);
        return response;
    }
    super::text::paint_text(ui, name, card.name, egui::FontId::proportional(size.font()), t.form_ink, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, card.name));
    response.on_hover_text(card.name)
}

/// A names-only row (the List size): the kind's icon, the name and the STEP / MESH pill.
pub(crate) fn part_row(ui: &mut Ui, name: &str, icon: &str, badge: Option<&str>, selected: bool) -> Response {
    let t = Tokens::get();
    let width = super::metrics::dimension(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.control_height), Sense::click_and_drag());
    let accent = TabAccent::for_tab(&t, "MODEL");
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect(rect, t.radius_sm, accent.soft, t.primary_border(accent.color), egui::StrokeKind::Inside);
    } else if response.hovered() {
        painter.rect_filled(rect, t.radius_sm, t.hover);
    }
    let glyph = egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_12, rect.center().y), vec2(t.space_14, t.space_14));
    crate::icons::paint_ink(&painter, glyph, icon, if icon == "folder" { accent.color } else { t.form_dim });
    let mut right = rect.right() - t.space_4;
    if let Some(badge) = badge {
        right = pill_left_of(&painter, egui::pos2(right, rect.center().y), badge, accent.color) - t.space_4;
    }
    let text =
        egui::Rect::from_min_max(egui::pos2(glyph.right() + t.space_8, rect.top()), egui::pos2(right.max(glyph.right() + t.space_8), rect.bottom()));
    super::text::paint_text(ui, text, name, egui::FontId::proportional(t.font_small), t.form_ink, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response.on_hover_text(name)
}

/// The breadcrumb's first segment: the folder's icon, its name (cut short) and a chevron; it
/// opens the choose-folder action. Tokens: control fill, border and radius.
pub(crate) fn folder_button(ui: &mut Ui, name: &str, max_width: f32) -> Response {
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_small);
    let text = ui.painter().layout_no_wrap(name.to_owned(), font.clone(), t.form_ink).size().x;
    let chrome = t.space_8 + t.space_14 + t.space_6 + t.space_6 + t.space_10 + t.space_8;
    let width = (text + chrome).min(max_width.max(chrome + t.space_24));
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.control_height), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        t.control_radius,
        if response.hovered() { t.hover } else { t.control_fill },
        t.control_border_stroke(t.control_border),
        egui::StrokeKind::Inside,
    );
    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_8 + t.space_14 / 2.0, rect.center().y), vec2(t.space_14, t.space_14));
    crate::icons::paint_ink(&painter, icon, "folder", TabAccent::for_tab(&t, "MODEL").color);
    let chevron =
        egui::Rect::from_center_size(egui::pos2(rect.right() - t.space_8 - t.space_10 / 2.0, rect.center().y), vec2(t.space_10, t.space_10));
    crate::icons::paint_ink(&painter, chevron, "chevron", t.form_dim);
    let label = egui::Rect::from_min_max(egui::pos2(icon.right() + t.space_6, rect.top()), egui::pos2(chevron.left() - t.space_6, rect.bottom()));
    super::text::paint_text(ui, label, name, font, t.form_ink, false);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response
}

/// A small outlined pill whose top left is at `at`.
fn pill(painter: &egui::Painter, at: egui::Pos2, text: &str, ink: egui::Color32) {
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(t.font_badge), ink);
    let r = egui::Rect::from_min_size(at, galley.size() + vec2(t.space_8, t.space_2 * 2.0));
    painter.rect(r, r.height() / 2.0, t.panel, t.control_border_stroke(ink), egui::StrokeKind::Inside);
    painter.galley(r.min + vec2(t.space_4, t.space_2), galley, ink);
}

/// A pill ending at `right_center`; returns its left edge.
fn pill_left_of(painter: &egui::Painter, right_center: egui::Pos2, text: &str, ink: egui::Color32) -> f32 {
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(t.font_badge), ink);
    let size = galley.size() + vec2(t.space_8, t.space_2 * 2.0);
    let left = right_center.x - size.x;
    pill(painter, egui::pos2(left, right_center.y - size.y / 2.0), text, ink);
    left
}
