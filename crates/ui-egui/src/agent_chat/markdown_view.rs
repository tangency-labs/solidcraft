//! An assistant's text as the panel shows it: Markdown (paragraphs, headings, lists, quotes,
//! fenced code, bold, italic, code, links), parsed by [`super::markdown`].
//!
//! Parsing and laying out a long reply every frame would show in the frame time while it streams,
//! so the parsed blocks are kept per conversation item and parsed again only when its text
//! changes.

use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::type_face::RichTextExt as _;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::markdown::{self, Block, Span};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Margin, RichText, Stroke, vec2};

use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr;

/// The parsed blocks of `src`, kept in the context's temporary memory under `slot` (the item's
/// index) and replaced when the text changes.
fn blocks(ctx: &egui::Context, slot: usize, src: &str) -> Arc<Vec<Block>> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let key = h.finish();
    let id = egui::Id::unique(("agent_chat_markdown", slot));
    if let Some((k, b)) = ctx.data(|d| d.get_temp::<(u64, Arc<Vec<Block>>)>(id))
        && k == key
    {
        return b;
    }
    let parsed = Arc::new(markdown::parse(src));
    ctx.data_mut(|d| d.insert_temp(id, (key, parsed.clone())));
    parsed
}

fn job(ui: &egui::Ui, spans: &[Span], size: f32, color: Color32) -> LayoutJob {
    let t = Tokens::get();
    let mut j = LayoutJob::default();
    j.wrap.max_width = ui.available_width();
    let base = FontId::proportional(size);
    for s in spans {
        let plain = TextFormat { font_id: base.clone(), color, ..Default::default() };
        match s {
            Span::Text(x) => j.append(x, 0.0, plain),
            Span::Bold(x) => j.append(x, 0.0, TextFormat { font_id: crate::ui_kit::type_face::semibold(size), color: t.text, ..plain }),
            Span::Italic(x) => j.append(x, 0.0, TextFormat { italics: true, ..plain }),
            Span::Code(x) => {
                j.append(x, 0.0, TextFormat { font_id: FontId::monospace(size - (t.font_button - t.font_body)), background: t.field, ..plain })
            }
            Span::Link { label, .. } => j.append(label, 0.0, TextFormat { color: t.accent, underline: Stroke::new(1.0, t.accent), ..plain }),
        }
    }
    j
}

/// The host of a web address: `example.com` for `https://example.com/a?b`.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// A selectable label of the conversation. egui leaves a label scrolled out of view out of the
/// text selection, so a selection whose start had scrolled away was dropped, and Copy missed the
/// hidden text (#11527). Out of view this one still takes part: it keeps its end of the selection
/// and gives its text to Copy, clipped to nothing so it is not drawn and never moves the selection.
/// `selected`: the whole transcript is selected (#10676), so its text is drawn over the selection
/// colour, as egui draws a label's own selection, and outlined with the selection stroke: a
/// prompt's bubble is filled with that same colour, so the fill alone would not show there.
pub(super) fn selectable(ui: &mut egui::Ui, label: egui::Label, selected: bool) -> egui::Response {
    use egui::text_selection::LabelSelectionState;
    let behind = ui.painter().add(egui::Shape::Noop);
    let (pos, galley, response) = label.selectable(true).layout_in_ui(ui);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), galley.text()));
    // As `egui::Label` draws a label that is not a button.
    let colour = ui.visuals().text_color();
    if ui.is_rect_visible(response.rect) {
        let underline = if response.has_focus() || response.highlighted() { Stroke::new(1.0, colour) } else { Stroke::NONE };
        LabelSelectionState::label_text_selection(ui, &response, pos, galley, colour, underline);
    } else if ui.ctx().plugin::<LabelSelectionState>().lock().has_selection() {
        let clip = ui.clip_rect();
        ui.set_clip_rect(egui::Rect::NOTHING);
        LabelSelectionState::label_text_selection(ui, &response, pos, galley, colour, Stroke::NONE);
        ui.set_clip_rect(clip);
    }
    if selected {
        let selection = ui.visuals().selection;
        let corner = egui::CornerRadius::same(Tokens::get().radius_sm / 2);
        let mark = egui::epaint::RectShape::new(response.rect, corner, selection.bg_fill, selection.stroke, egui::StrokeKind::Outside);
        ui.painter().set(behind, mark);
    }
    response
}

/// A paragraph's text, then its links as buttons (a laid-out text cannot be clicked in parts).
fn spans(ui: &mut egui::Ui, spans: &[Span], size: f32, color: Color32, selected: bool) {
    spans_in(ui, spans, size, color, selected, true);
}

/// [`spans`]; `wrap` false keeps the text on one line however narrow the panel (a table cell: the
/// table scrolls sideways instead of breaking words).
fn spans_in(ui: &mut egui::Ui, spans: &[Span], size: f32, color: Color32, selected: bool, wrap: bool) {
    let t = Tokens::get();
    let mut text = job(ui, spans, size, color);
    if !wrap {
        text.wrap.max_width = f32::INFINITY;
    }
    selectable(ui, egui::Label::new(text).wrap_mode(if wrap { egui::TextWrapMode::Wrap } else { egui::TextWrapMode::Extend }), selected);
    let links: Vec<(&str, &str)> =
        spans.iter().filter_map(|s| if let Span::Link { label, url } = s { Some((label.as_str(), url.as_str())) } else { None }).collect();
    if !links.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (label, url) in links {
                // The text above already shows the label; the button says where it goes.
                let to = format!("↗ {}", host(url));
                ui.hyperlink_to(RichText::new(to).size(size - (t.font_button - t.font_body)), url).backing_on_hover_text(format!("{label}: {url}"));
            }
        });
    }
}

/// The blocks as plain text, as they read on screen: what Copy takes from a reply when the whole
/// transcript is selected (#10676).
pub(super) fn plain_text(blocks: &[Block]) -> String {
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        out.push(match block {
            Block::Paragraph(s) | Block::Heading { spans: s, .. } => markdown::plain(s),
            Block::Code { text, .. } | Block::Pre(text) | Block::Table { source: text, .. } => text.trim_end_matches('\n').to_owned(),
            Block::List { ordered, start, items } => items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let mark = if *ordered { format!("{}.", start.saturating_add(i as u64)) } else { "•".to_string() };
                    format!("{mark} {}", markdown::plain(item))
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Block::Quote(s) => format!("> {}", markdown::plain(s)),
            Block::Rule => "---".to_string(),
        });
    }
    out.join("\n\n")
}

/// Draw `src` as Markdown. `slot`: the conversation item it belongs to; `selected`: the whole
/// transcript is selected.
pub fn show(ui: &mut egui::Ui, slot: usize, src: &str, selected: bool) {
    let t = Tokens::get();
    let size = t.font_button;
    let parsed = blocks(ui.ctx(), slot, src);
    for (n, block) in parsed.iter().enumerate() {
        match block {
            Block::Paragraph(s) => spans(ui, s, size, t.text, selected),
            Block::Heading { level, spans: s } => {
                let size = match level {
                    1 => t.font_article_heading,
                    2 => t.font_subtitle,
                    _ => t.font_heading,
                };
                ui.add_space(4.0);
                selectable(ui, egui::Label::new(RichText::new(markdown::plain(s)).size(size).semibold().color(t.text)).wrap(), selected);
            }
            Block::Code { text, .. } => {
                crate::ui_kit::Container::show_backing(
                    ui,
                    egui::Frame::NONE.fill(t.field).corner_radius(t.radius_sm).inner_margin(Margin::same(6)),
                    |ui| {
                        ui.set_width(ui.available_width());
                        selectable(
                            ui,
                            egui::Label::new(RichText::new(text).monospace().size(size - (t.font_button - t.font_body)).color(t.text)).wrap(),
                            selected,
                        );
                        if crate::ui_kit::button(ui, tr("ui.agent_chat_copy"), crate::ui_kit::Button::Small).clicked() {
                            ui.ctx().copy_text(text.clone());
                        }
                    },
                );
            }
            Block::List { ordered, start, items } => {
                for (i, item) in items.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        let mark = if *ordered { format!("{}.", start.saturating_add(i as u64)) } else { "•".to_string() };
                        ui.label(RichText::new(mark).size(size).color(t.text_dim));
                        ui.vertical(|ui| spans(ui, item, size, t.text, selected));
                    });
                }
            }
            Block::Quote(s) => {
                ui.horizontal_top(|ui| {
                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, t.radius_sm / 4, t.border);
                    ui.vertical(|ui| spans(ui, s, size, t.text_dim, selected));
                });
            }
            Block::Table { header, rows, .. } => {
                egui::ScrollArea::horizontal().id_salt(("agent_chat_table", slot, n)).show(ui, |ui| {
                    egui::Grid::new(("agent_chat_table_grid", slot, n)).striped(true).spacing(vec2(14.0, 4.0)).show(ui, |ui| {
                        for cell in header {
                            let head = RichText::new(markdown::plain(cell)).size(size).semibold().color(t.text);
                            selectable(ui, egui::Label::new(head).extend(), selected);
                        }
                        ui.end_row();
                        for row in rows {
                            for cell in row {
                                // One grid item per cell: the text and its link buttons share it.
                                ui.vertical(|ui| spans_in(ui, cell, size, t.text, selected, false));
                            }
                            ui.end_row();
                        }
                    });
                });
            }
            Block::Pre(text) => {
                egui::ScrollArea::horizontal().id_salt(("agent_chat_pre", slot, text.len())).show(ui, |ui| {
                    selectable(ui, egui::Label::new(RichText::new(text).monospace().size(t.font_small).color(t.text)), selected);
                });
            }
            Block::Rule => {
                ui.separator();
            }
        }
        ui.add_space(3.0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_link_button_names_the_host() {
        assert_eq!(super::host("https://example.com/a?b#c"), "example.com");
        assert_eq!(super::host("http://localhost:8080/x"), "localhost:8080");
        assert_eq!(super::host("https://a.b"), "a.b");
    }
}
