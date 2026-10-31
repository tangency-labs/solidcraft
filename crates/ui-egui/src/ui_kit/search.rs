//! Command-search composition: one expanding ribbon editor and a clipped results sheet.
//! Domain ranking and command dispatch stay with the caller.
use crate::{icons, theme::Tokens};
use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

#[derive(Clone, Copy)]
pub(crate) enum SearchBadge {
    Kind,
    Valid,
    Invalid,
}

/// Geometry uses the same already-eased progress as the field bounds.
pub(crate) struct SearchFieldGeometry {
    pub height: f32,
    pub radius: f32,
    pub progress: f32,
}

struct FieldSpacing {
    left: f32,
    right: f32,
    icon_gap: f32,
    key_gap: f32,
    key_padding: f32,
    editor_margin: f32,
}

impl FieldSpacing {
    fn at(progress: f32) -> Self {
        let t = Tokens::get();
        Self {
            left: egui::lerp(t.space_4..=t.space_12, progress),
            right: egui::lerp(t.space_4..=t.space_10, progress),
            icon_gap: egui::lerp(t.space_4..=t.space_12, progress),
            key_gap: egui::lerp(t.space_4..=t.space_10, progress),
            key_padding: egui::lerp(t.space_4..=t.space_10, progress),
            editor_margin: egui::lerp(0.0..=t.form_inset, progress),
        }
    }
}

pub(crate) struct CommandSearch;

impl CommandSearch {
    pub(crate) fn width() -> f32 {
        let t = Tokens::get();
        t.form_card_width * 2.0 - t.space_8
    }

    /// Reserve the translated hint at the exact editor font, plus compact controls.
    /// The small text allowance covers pixel rounding and the editor's caret.
    pub(crate) fn closed_width(ctx: &egui::Context) -> f32 {
        let t = Tokens::get();
        let spacing = FieldSpacing::at(0.0);
        let (hint, key) = ctx.fonts_mut(|fonts| {
            (
                fonts.layout_no_wrap(crate::i18n::tr_label("Search commands").to_owned(), FontId::proportional(t.font_button), t.form_dim).size().x,
                fonts.layout_no_wrap("S".to_owned(), FontId::monospace(t.font_small), t.form_dim).size().x,
            )
        });
        (spacing.left + t.space_16 + spacing.icon_gap + hint + spacing.key_gap + key + spacing.key_padding + spacing.right + t.space_2).ceil()
    }

    pub(crate) fn field_radius(progress: f32, floating: bool) -> f32 {
        let t = Tokens::get();
        // Preserve the original ribbon inset and rounding in both layouts.
        let closed =
            if floating { t.radius_lg.saturating_sub(t.space_4 as u8).max(t.radius_sm / 2) } else { t.radius_md.saturating_sub(t.space_2 as u8 / 2) };
        egui::lerp(f32::from(closed)..=f32::from(t.radius_xl), progress)
    }

    fn inner_radius() -> u8 {
        let t = Tokens::get();
        t.radius_xl.saturating_sub(t.space_6 as u8)
    }

    pub(crate) fn field(
        ui: &mut Ui,
        value: &mut String,
        invalid: bool,
        focus: bool,
        open: bool,
        geometry: SearchFieldGeometry,
        limit: usize,
    ) -> Response {
        let t = Tokens::get();
        let SearchFieldGeometry { height, radius, progress } = geometry;
        let spacing = FieldSpacing::at(progress);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let focused = open && (focus || ui.memory(|m| m.has_focus(egui::Id::unique("sc_sbox_text"))));
        let ink = if invalid {
            t.form_error
        } else if focused {
            t.form_primary
        } else {
            t.control_border
        };
        if focused {
            ui.painter().rect_filled(
                r.expand(t.space_3),
                Tokens::outset_radius(radius, t.space_3),
                if invalid { t.form_error_fill } else { t.accent_soft },
            );
        }
        ui.painter().rect(
            r,
            radius,
            t.input_fill(),
            if focused || invalid { Stroke::new(1.0, ink) } else { t.control_border_stroke(ink) },
            egui::StrokeKind::Inside,
        );
        if let Some(bevel) = t.borders.bevel {
            bevel.paint(ui.painter(), r.shrink(if focused || invalid { 1.0 } else { 0.0 }), radius, true, ui.is_enabled());
        }
        icons::paint_icon(
            ui.painter(),
            Rect::from_center_size(r.left_center() + vec2((spacing.left + t.space_16 / 2.0).min(r.width() / 2.0), 0.0), vec2(t.space_16, t.space_16)),
            "zoom",
            1.0,
        );
        let cap = if open { "Esc" } else { "S" };
        let key_width = Self::key_with_padding(ui, r.right_center() - vec2(spacing.right, 0.0), cap, spacing.key_padding);
        let editor_left = (r.left() + spacing.left + t.space_16 + spacing.icon_gap).min(r.right());
        let editor = Rect::from_min_max(
            pos2(editor_left, r.top()),
            pos2((r.right() - spacing.right - key_width - spacing.key_gap).max(editor_left), r.bottom()),
        );
        let mono = value.contains('{');
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(editor), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(editor));
                // egui 0.37 floors a text edit's height at the theme's interact height, which centred
                // the text below the field's middle.
                ui.spacing_mut().interact_size.y = height;
                ui.add_sized(
                    editor.size(),
                    super::fields::text_input(egui::TextEdit::singleline(value))
                        .min_size(vec2(0.0, height))
                        .margin(vec2(spacing.editor_margin, 0.0))
                        .id(egui::Id::unique("sc_sbox_text"))
                        .char_limit(limit)
                        .interactive(open)
                        .frame(egui::Frame::NONE)
                        .hint_text(crate::i18n::tr_label("Search commands"))
                        .font(if mono { FontId::monospace(t.font_label) } else { FontId::proportional(t.font_button) }),
                )
            })
            .inner;
        if focus {
            response.request_focus();
        }
        if ui.rect_contains_pointer(r) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        crate::scenario::publish_target("ribbon:search", r);
        crate::scenario::publish_target("shortcut:search", r);
        crate::scenario::publish_target("palette:search", r);
        response
    }

    fn key(ui: &Ui, right: egui::Pos2, label: &str) -> f32 {
        Self::key_with_padding(ui, right, label, Tokens::get().space_10)
    }

    fn key_with_padding(ui: &Ui, right: egui::Pos2, label: &str, padding: f32) -> f32 {
        let t = Tokens::get();
        let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::monospace(t.font_small), t.form_dim);
        let size = galley.size() + vec2(padding, t.space_4);
        let r = Rect::from_min_size(pos2(right.x - size.x, right.y - size.y / 2.0), size);
        t.container_rect(ui.painter(), r, Self::inner_radius(), t.dialog_footer, t.dialog_rule);
        ui.painter().galley(r.min + vec2(padding / 2.0, t.space_2), galley, t.form_dim);
        size.x
    }

    pub(crate) fn heading(ui: &mut Ui, label: &str, count: usize) {
        super::Menu::heading(ui, &format!("{}  {count}", crate::i18n::tr(label)));
    }

    pub(crate) fn detail(ui: &mut Ui, text: &str) {
        let t = Tokens::get();
        egui::Frame::NONE
            .fill(t.control_fill)
            .stroke(t.container_border(t.dialog_rule))
            .corner_radius(t.control_radius)
            .inner_margin(t.space_8)
            .show(ui, |ui| {
                super::text(ui, text, super::Text::MonoNote("command_parameters"));
            });
    }

    /// Lay out the full sheet, but allocate and paint only its unfolding height. The
    /// remembered natural height is refreshed with a sizing pass when results change.
    pub(crate) fn sheet(ui: &mut Ui, width: f32, progress: f32, command: bool, add: impl FnOnce(&mut Ui)) -> Rect {
        let t = Tokens::get();
        let id = ui.scope_id().with("search_sheet_height");
        let known = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
        let origin = ui.cursor().min;
        let clip = Rect::from_min_size(origin, vec2(width, known * progress));
        let background = ui.painter().add(egui::Shape::Noop);
        let mut child =
            ui.new_child(egui::UiBuilder::new().id_salt("search_sheet").max_rect(Rect::from_min_size(origin, vec2(width, ui.available_height()))));
        child.set_clip_rect(ui.clip_rect().intersect(clip));
        child.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::NONE.inner_margin(egui::Margin { left: t.space_6 as i8, right: t.space_6 as i8, top: t.space_4 as i8, bottom: 0 }).show(
            &mut child,
            |ui| {
                ui.set_width((width - t.space_12).max(0.0));
                add(ui);
            },
        );
        child.add_space(t.space_6);
        Self::footer(&mut child, command);
        let natural = child.min_rect().height();
        ui.ctx().data_mut(|d| d.insert_temp(id, natural));
        if (known - natural).abs() > 0.5 {
            ui.ctx().request_discard("command search sheet resized");
        }
        let sheet = Rect::from_min_size(origin, vec2(width, natural * progress));
        let frame = super::Menu::frame().inner_margin(0);
        ui.painter().set(background, frame.paint(sheet.shrink(frame.stroke.width)));
        // The edge-to-edge footer shares the card silhouette; keep the outer rule
        // above its fill, as an overflow-clipped footer would in the board.
        ui.painter().rect_stroke(sheet, t.radius_xl, frame.stroke, egui::StrokeKind::Inside);
        super::Container::paint_inset(ui.painter(), sheet, frame);
        ui.allocate_rect(sheet, Sense::hover());
        crate::scenario::publish_target("ribbon:search_popup", sheet);
        crate::scenario::publish_target("palette:outer", sheet);
        sheet
    }

    fn footer(ui: &mut Ui, command: bool) {
        let t = Tokens::get();
        let start = ui.cursor().min;
        let width = ui.available_width();
        let background = ui.painter().add(egui::Shape::Noop);
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(t.space_14 as i8, t.space_3 as i8)).show(ui, |ui| {
            ui.set_width((width - t.space_14 * 2.0).max(0.0));
            super::text(ui, crate::i18n::tr(if command { "search.command_keys" } else { "search.keys" }), super::Text::Note("search_keys"));
        });
        let r = Rect::from_min_max(start, pos2(start.x + width, ui.min_rect().bottom()));
        ui.painter()
            .set(background, egui::Shape::rect_filled(r, egui::CornerRadius { nw: 0, ne: 0, sw: t.radius_xl, se: t.radius_xl }, t.dialog_footer));
        t.secondary_hline(ui.painter(), r.x_range(), r.top(), t.dialog_rule);
        crate::scenario::publish_rect("ribbon:search_footer", r);
    }
}

impl super::MenuItem<'_> {
    /// 38 pt search row: shared menu states, 26 pt tile, highlighted match, path and keycap.
    pub(crate) fn search_row(&self, ui: &mut Ui, query: &str, path: &str, badge: Option<SearchBadge>) -> Response {
        let t = Tokens::get();
        let (r, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), t.control_height + t.space_10),
            if self.enabled { Sense::click() } else { Sense::hover() },
        );
        let hot = self.enabled && (self.highlighted || response.hovered());
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(r));
        if hot {
            painter.rect_filled(r, CommandSearch::inner_radius(), t.accent_soft);
        }
        let tile = Rect::from_center_size(
            pos2(r.left() + t.space_8 + (t.space_24 + t.space_2) / 2.0, r.center().y),
            vec2(t.space_24 + t.space_2, t.space_24 + t.space_2),
        );
        painter.rect_filled(tile, CommandSearch::inner_radius(), if hot { t.accent_soft } else { t.dialog_footer });
        icons::paint_icon(&painter, tile.shrink(t.space_6), self.icon, if self.enabled { 1.0 } else { 0.4 });
        let right = r.right() - t.space_8;
        let key_width = if self.shortcut.is_empty() {
            0.0
        } else if let Some(badge) = badge {
            let ink = match badge {
                SearchBadge::Kind => t.form_dim,
                SearchBadge::Valid => t.form_success,
                SearchBadge::Invalid => t.form_error,
            };
            let galley = painter.layout_no_wrap(self.shortcut.to_owned(), crate::fonts::semibold(t.font_small), ink);
            let size = galley.size() + vec2(t.space_16, t.space_4);
            let rect = Rect::from_min_size(pos2(right - size.x, r.center().y - size.y / 2.0), size);
            painter.rect_filled(
                rect,
                CommandSearch::inner_radius(),
                match badge {
                    SearchBadge::Invalid => t.form_error_fill,
                    SearchBadge::Valid => t.form_success_fill,
                    SearchBadge::Kind => t.dialog_footer,
                },
            );
            painter.galley(rect.min + vec2(t.space_8, t.space_2), galley, ink);
            size.x + t.space_10
        } else {
            CommandSearch::key(ui, pos2(right, r.center().y), self.shortcut) + t.space_10
        };
        let left = tile.right() + t.space_10;
        let label = self.label;
        let normal = egui::TextFormat::simple(FontId::proportional(t.font_label), if self.enabled { t.form_ink } else { t.form_dim });
        let mut job = egui::text::LayoutJob::default();
        // Match by character boundaries, including case mappings that change UTF-8 byte lengths.
        let q = query.trim().to_lowercase();
        let hit = (!q.is_empty())
            .then(|| {
                label.char_indices().find_map(|(start, _)| {
                    let mut lowered = String::new();
                    for (offset, c) in label.get(start..)?.char_indices() {
                        lowered.extend(c.to_lowercase());
                        if lowered == q {
                            return Some((start, start + offset + c.len_utf8()));
                        }
                        if !q.starts_with(&lowered) {
                            break;
                        }
                    }
                    None
                })
            })
            .flatten();
        if let Some((start, end)) = hit {
            job.append(label.get(..start).unwrap_or_default(), 0.0, normal.clone());
            job.append(label.get(start..end).unwrap_or_default(), 0.0, egui::TextFormat::simple(crate::fonts::bold(t.font_label), t.selected_ink));
            job.append(label.get(end..).unwrap_or_default(), 0.0, normal);
        } else {
            job.append(label, 0.0, normal);
        }
        job.wrap.max_width = (right - key_width - left).max(0.0);
        job.wrap.max_rows = 1;
        let galley = painter.layout_job(job);
        painter.galley(pos2(left, r.top() + t.space_3), galley, t.form_ink);
        let path_rect = Rect::from_min_max(pos2(left, r.center().y), pos2((right - key_width).max(left), r.bottom() - t.space_2));
        if let Some((tab, group)) = path.split_once('›') {
            let tab = painter.layout_no_wrap(tab.trim().to_owned(), FontId::proportional(t.font_small), t.form_dim);
            let x = (left + tab.size().x + t.space_6).min(path_rect.right());
            painter.with_clip_rect(path_rect).galley(pos2(left, path_rect.center().y - tab.size().y / 2.0), tab, t.form_dim);
            icons::glyphs::breadcrumb(&painter.with_clip_rect(path_rect), pos2(x, path_rect.center().y), t.form_dim);
            super::text::paint_text(
                ui,
                Rect::from_min_max(pos2((x + t.space_6).min(path_rect.right()), path_rect.top()), path_rect.max),
                group.trim(),
                FontId::proportional(t.font_small),
                t.form_dim,
                false,
            );
        } else {
            super::text::paint_text(ui, path_rect, path, FontId::proportional(t.font_small), t.form_dim, false);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, self.enabled, label));
        crate::scenario::publish_target(&format!("menu:{}", self.id), r);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_corners_interpolate_and_nested_arcs_are_concentric() {
        let t = Tokens::get();
        for floating in [true, false] {
            let closed = if floating { 4.0 } else { 5.0 };
            for progress in [0.0, 0.5, 1.0] {
                let radius = CommandSearch::field_radius(progress, floating);
                assert_eq!(radius, egui::lerp(closed..=10.0, progress));
                let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(188.0, 40.0));
                let ring = rect.expand(t.space_3);
                let ring_radius = radius + t.space_3;
                assert_eq!(ring_radius, radius + 3.0);
                assert_eq!(rect.min + vec2(radius, radius), ring.min + vec2(ring_radius, ring_radius));
                // Inspect the real paint shapes so a stale fill/ring radius cannot
                // pass by merely keeping the geometry calculation correct.
                let ctx = crate::fonts::test_context();
                let mut query = String::new();
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    CommandSearch::field(ui, &mut query, false, true, true, SearchFieldGeometry { height: 40.0, radius, progress }, 4096);
                });
                output.textures_delta.clear();
                let shapes: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(r) => Some(r),
                        _ => None,
                    })
                    .collect();
                let fill = shapes.iter().find(|r| r.fill == t.control_fill).expect("field fill");
                let ring = shapes.iter().find(|r| r.fill == t.accent_soft).expect("focus ring");
                assert_eq!(fill.corner_radius, egui::CornerRadius::same(radius.round() as u8));
                assert_eq!(ring.corner_radius, egui::CornerRadius::same((radius + t.space_3).round() as u8));
                assert_eq!(ring.rect, fill.rect.expand(t.space_3));
            }
        }
        assert_eq!(t.radius_xl, 10);
        assert_eq!(CommandSearch::inner_radius(), 4);
        assert_eq!(f32::from(CommandSearch::inner_radius()), f32::from(t.radius_xl) - t.space_6);
    }

    #[test]
    fn square_theme_search_and_focus_ring_stay_square_through_expansion() {
        let tokens = Tokens { radius_sm: 0, radius_md: 0, radius_lg: 0, radius_xl: 0, control_radius: 0, sketch_mode_radius: 0, ..Tokens::dark() };
        let _theme = crate::theme::scoped_tokens(tokens);
        let mut radii = Vec::new();
        for floating in [false, true] {
            for progress in [0.0, 0.5, 1.0] {
                let radius = CommandSearch::field_radius(progress, floating);
                let ctx = crate::fonts::test_context();
                let mut query = String::new();
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    CommandSearch::field(ui, &mut query, false, true, true, SearchFieldGeometry { height: 40.0, radius, progress }, 4096);
                });
                output.textures_delta.clear();
                for shape in output.shapes {
                    if let egui::Shape::Rect(rect) = shape.shape
                        && (rect.fill == tokens.control_fill || rect.fill == tokens.accent_soft)
                    {
                        radii.push(rect.corner_radius);
                    }
                }
            }
        }
        assert_eq!(radii.len(), 12, "field and focus ring in both layouts at three expansion stages");
        assert!(radii.iter().all(|radius| *radius == egui::CornerRadius::ZERO));
    }
}
