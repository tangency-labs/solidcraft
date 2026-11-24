//! Compositions of kit controls using the weld board's spacing, expressed in theme metrics.
use super::*;
use crate::ui_kit::{self as kit, BackingTooltipExt as _, Button, Container, Surface};
use crate::weld_figures::{SizeFigure, paint_size_figure};
use egui::{Align, Align2, FontId, Layout, RichText, Sense, UiBuilder, pos2, vec2};

#[path = "weld_panel/chrome.rs"]
mod chrome;
#[path = "weld_panel/look.rs"]
pub(super) mod look;
#[path = "weld_panel/size.rs"]
mod size;

fn card_width(t: &Tokens) -> f32 {
    t.form_card_width + 2.0 * (t.space_24 + t.space_16)
}
fn figure_width(t: &Tokens) -> f32 {
    2.0 * t.form_label_width + t.space_24
}

fn child(ui: &mut Ui, rect: Rect, key: impl egui::AsIdSalt) -> Ui {
    let mut child = ui.new_child(UiBuilder::new().id_salt(key).max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.spacing_mut().item_spacing = vec2(Tokens::get().space_6, Tokens::get().space_8);
    child.spacing_mut().interact_size.y = Tokens::get().space_14;
    child
}

fn label(ui: &mut Ui, id: &str, t: &Tokens, muted: bool) -> egui::Response {
    ui.add(
        egui::Label::new(RichText::new(tr(id)).size(if muted { t.font_caption } else { t.font_label }).color(if muted {
            t.form_dim
        } else {
            t.form_ink
        }))
        .wrap(),
    )
}

fn rule(ui: &Ui, t: &Tokens, y: f32) {
    t.secondary_hline(ui.painter(), ui.max_rect().x_range(), y, t.dialog_rule);
}

fn section(ui: &mut Ui, t: &Tokens, m: &mut Measurements, key: &'static str, add: impl FnOnce(&mut Ui, &mut Measurements)) {
    let top = ui.cursor().top();
    rule(ui, t, top);
    let left = ui.max_rect().left() + t.space_14;
    let right = ui.max_rect().right() - t.space_14;
    let separator = if key == "type" { 0.0 } else { t.space_2 / 2.0 };
    let mut body = child(ui, Rect::from_min_max(pos2(left, top + separator + t.space_10), pos2(right, ui.max_rect().bottom())), key);
    let (heading, _) = body.allocate_exact_size(vec2(body.available_width(), t.space_24 - t.space_2), Sense::hover());
    let mut title = child(&mut body, heading.translate(vec2(0.0, t.space_3)), "heading");
    kit::section_heading(&mut title, tr(&format!("ui.weld.section.{key}")));
    // Headings end 6 points before the body (the parent default gap is 8).
    body.add_space(-t.space_2);
    add(&mut body, m);
    let bottom = body.min_rect().bottom() + t.space_12;
    let rect = Rect::from_min_max(pos2(ui.max_rect().left(), top), pos2(ui.max_rect().right(), bottom));
    ui.allocate_rect(rect, Sense::hover());
    m.sections.push((key, rect));
}

fn grey(ui: &mut Ui, t: &Tokens, m: &mut Measurements, key: &'static str, text: &str) {
    let top = ui.cursor().top();
    rule(ui, t, top);
    let height = t.form_inset * 2.0 + t.space_16 + t.space_2 / 2.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let dot = pos2(rect.left() + t.space_14 + (t.space_6 + t.space_2 / 2.0) / 2.0, rect.center().y);
    ui.painter().circle_filled(dot, (t.space_6 + t.space_2 / 2.0) / 2.0, t.control_border);
    ui.painter().text(pos2(dot.x + t.space_12, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(t.font_body), t.form_dim);
    m.sections.push((key, rect));
}

fn at_button(ui: &mut Ui, m: &mut Measurements, rect: Rect, key: &str, text: &str, tone: Button) -> egui::Response {
    // Anchored controls cannot move the already allocated band's flow cursor.
    let mut control = child(ui, rect, key);
    let response = kit::button_at(&mut control, key, rect, text, tone);
    m.widgets.push(response.rect);
    response
}

fn icon_choice(ui: &mut Ui, t: &Tokens, m: &mut Measurements, rect: Rect, key: &str, icon: &str, text: &str, on: bool) -> egui::Response {
    // The kit has no vertical icon/word button: compose its button surface and icon painter.
    let response = at_button(ui, m, rect, key, "", if on { Button::SmallSelected } else { Button::Normal });
    let icon_size = if rect.height() > t.control_height * 2.0 { t.control_height + t.space_2 } else { t.space_24 + t.space_2 };
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + t.space_4 + icon_size / 2.0), vec2(icon_size, icon_size));
    kit::icons::paint_icon(ui.painter(), icon_rect, icon, 1.0);
    let font = if on { kit::type_face::semibold(t.font_body) } else { FontId::proportional(t.font_body) };
    let galley = ui.painter().layout(text.to_owned(), font, t.form_ink, rect.width() - t.space_8);
    ui.painter().galley(pos2(rect.center().x - galley.size().x / 2.0, rect.bottom() - t.space_4 - galley.size().y), galley, t.form_ink);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, text));
    response.backing_on_hover_text(text)
}

fn toggle_chip(ui: &mut Ui, id: &str, icon: &str, value: &mut bool) {
    let t = Tokens::get();
    let text_width = ui.painter().layout_no_wrap(tr(id).into(), kit::type_face::semibold(t.font_label), t.form_ink).size().x.ceil();
    let width = (text_width + t.space_16 + t.space_14 + t.space_6 + t.space_4).min(ui.available_width());
    // Keep the board's 30-point row while centring the kit's 28-point chip in it.
    let (rect, _) = ui.allocate_exact_size(vec2(width, t.control_height + t.space_2), Sense::hover());
    let mut control = child(ui, rect.shrink2(vec2(0.0, t.space_2 / 2.0)), id);
    let activated = kit::segmented(
        &mut control,
        &mut 0,
        &[tr(id)],
        kit::Segments { states: &[*value], icons: &[icon], intrinsic: true, compact: true, ..Default::default() },
    );
    if activated {
        *value = !*value;
    }
}

fn types(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements, event: &mut WeldPanelEvent) {
    section(ui, t, m, "type", |ui, m| {
        let options: Vec<_> = TYPES.iter().filter(|t| t.visible).collect();
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height * 2.0 + t.space_10), Sense::hover());
        let width = (row.width() - 3.0 * t.space_6) / 4.0;
        for (index, info) in options.iter().enumerate() {
            let rect = Rect::from_min_size(row.min + vec2(index as f32 * (width + t.space_6), 0.0), vec2(width, row.height()));
            if chrome::type_choice(ui, t, m, rect, info, state.weld_type == info.weld_type).clicked() {
                state.weld_type = info.weld_type;
                *event = WeldPanelEvent::TypePicked(info.weld_type);
            }
        }
        let info = state.weld_type.info();
        let height = if info.visible { t.control_height + t.space_2 } else { t.control_height + t.space_8 };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let response = if info.visible { chrome::more_button(ui, m, rect, t) } else { at_button(ui, m, rect, "more", "", Button::SmallSelected) };
        m.more_button = Some(rect);
        let more = ui.painter().layout_no_wrap(tr("ui.weld.more_types").into(), FontId::proportional(t.font_label), t.form_ink);
        let arrow_side = t.space_12 + t.space_2;
        let more_left = if info.visible {
            rect.center().x - (more.size().x + t.space_6 + arrow_side) / 2.0
        } else {
            rect.right() - t.space_10 - arrow_side - t.space_6 - more.size().x
        };
        ui.painter().galley(pos2(more_left, rect.center().y - more.size().y / 2.0), more.clone(), t.form_ink);
        kit::dropdown_indicator(
            ui.painter(),
            Rect::from_center_size(pos2(more_left + more.size().x + t.space_6 + arrow_side / 2.0, rect.center().y), vec2(arrow_side, arrow_side)),
            t.form_dim,
        );
        if !info.visible {
            let icon_rect = Rect::from_center_size(
                pos2(rect.left() + t.space_10 + (t.space_24 - t.space_2) / 2.0, rect.center().y),
                vec2(t.space_24 - t.space_2, t.space_24 - t.space_2),
            );
            kit::icons::paint_icon(ui.painter(), icon_rect, info.icon, 1.0);
            let text = ui.painter().layout_no_wrap(tr(info.label).into(), kit::type_face::semibold(t.font_label), t.form_ink);
            ui.painter()
                .with_clip_rect(
                    Rect::from_min_max(pos2(icon_rect.right() + t.space_8, rect.top()), pos2(more_left - t.space_8, rect.bottom()))
                        .intersect(ui.clip_rect()),
                )
                .galley(pos2(icon_rect.right() + t.space_8, rect.center().y - text.size().y / 2.0), text, t.form_ink);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("ui.weld.more_types")));
        if response.clicked() {
            state.type_picker_open = !state.type_picker_open;
        }
        let mut open = state.type_picker_open;
        Container::new(ui.ctx(), Surface::Popup).menu(&response).open_bool(&mut open).width(card_width(t) + t.space_24 * 2.0 + t.space_16).show(
            |ui| {
                egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 2.0 * t.space_24).show(ui, |ui| {
                    let families =
                        ["ui.weld.family.arc", "ui.weld.family.point", "ui.weld.family.seam", "ui.weld.family.surface", "ui.weld.family.solid"];
                    for (family, title) in families.iter().enumerate() {
                        kit::section_heading(ui, tr(title));
                        let items: Vec<_> = TYPES.iter().filter(|info| info.family == family).collect();
                        for row in items.chunks(4) {
                            let width = (ui.available_width() - t.space_4 * 3.0) / 4.0;
                            let (band, _) =
                                ui.allocate_exact_size(vec2(ui.available_width(), t.control_height * 2.0 + t.space_16 + t.space_4), Sense::hover());
                            for (column, info) in row.iter().enumerate() {
                                let rect = Rect::from_min_size(band.min + vec2(column as f32 * (width + t.space_4), 0.0), vec2(width, band.height()));
                                let mut cell = child(ui, rect, info.icon);
                                let choice = Rect::from_min_size(rect.min, vec2(width, t.control_height * 2.0 + t.space_2));
                                if icon_choice(
                                    &mut cell,
                                    t,
                                    &mut Measurements::default(),
                                    choice,
                                    info.icon,
                                    info.icon,
                                    tr(info.label),
                                    state.weld_type == info.weld_type,
                                )
                                .clicked()
                                {
                                    state.weld_type = info.weld_type;
                                    *event = WeldPanelEvent::TypePicked(info.weld_type);
                                    ui.close();
                                }
                                let tag = if info.look_tbd {
                                    Some("ui.weld.look_tbd")
                                } else if info.soon {
                                    Some("ui.weld.soon")
                                } else {
                                    None
                                };
                                if let Some(tag) = tag {
                                    ui.painter().text(
                                        pos2(rect.center().x, rect.bottom() - t.space_2),
                                        Align2::CENTER_BOTTOM,
                                        tr(tag),
                                        FontId::proportional(t.font_micro),
                                        t.form_dim,
                                    );
                                }
                            }
                        }
                    }
                    label(ui, "ui.weld.keep_type", t, true);
                });
            },
        );
        state.type_picker_open = open;
    });
}

fn picks(state: &mut WeldPanelState) -> &mut Vec<String> {
    if state.weld_type == WeldType::Groove {
        return &mut state.picks.groove;
    }
    match state.weld_type.info().kind {
        WhereKind::Line => &mut state.picks.line,
        WhereKind::Point => &mut state.picks.point,
        WhereKind::Hole => &mut state.picks.hole,
        WhereKind::Ring => &mut state.picks.ring,
        WhereKind::Area => &mut state.picks.area,
    }
}

fn where_section(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements, event: &mut WeldPanelEvent) {
    section(ui, t, m, "where", |ui, _| {
        let kind = state.weld_type.info().kind;
        let groove = state.weld_type == WeldType::Groove;
        let stud = state.weld_type == WeldType::Stud;
        let frame = Container::floating_card(Surface::Navigation)
            .frame(ui.ctx(), t.control_fill, kit::frame::Side::Left)
            .inner_margin(t.space_6)
            .stroke(t.control_border_stroke(t.form_primary));
        Container::show_backing(ui, frame, |ui| {
            ui.set_min_width((ui.available_width()).max(0.0));
            ui.set_min_height(t.space_24 + t.space_2);
            ui.spacing_mut().item_spacing.x = t.space_6;
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (index, pick) in picks(state).iter().enumerate() {
                    let remove_label = solidcraft_ui_text::format("ui.weld.remove_pick", &[("name", pick)]);
                    if kit::settings_chip(
                        ui,
                        pick,
                        kit::SettingsChip {
                            key: Some(&index.to_string()),
                            icon: (kind != WhereKind::Point).then_some("select_faces_edges"),
                            remove: Some(&remove_label),
                            ..Default::default()
                        },
                    )
                    .clicked()
                    {
                        remove = Some(index);
                    }
                }
                if let Some(index) = remove {
                    picks(state).remove(index);
                    *event = WeldPanelEvent::PicksChanged;
                }
                if picks(state).is_empty() && kit::button(ui, tr("ui.weld.pick_geometry"), Button::Normal).clicked() {
                    *event = WeldPanelEvent::Pick(kind);
                }
            });
        });
        let hint = if groove {
            "ui.weld.where.groove"
        } else {
            match kind {
                WhereKind::Line => "ui.weld.where.line",
                WhereKind::Point => "ui.weld.where.point",
                WhereKind::Hole => "ui.weld.where.hole",
                WhereKind::Ring if stud => "ui.weld.where.stud",
                WhereKind::Ring => "ui.weld.where.ring",
                WhereKind::Area => "ui.weld.where.area",
            }
        };
        ui.add(egui::Label::new(RichText::new(tr(hint)).size(t.font_caption).line_height(Some(t.font_caption * 1.45)).color(t.form_dim)).wrap());
        if groove {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(t.space_24 + t.space_2, t.space_24 + t.space_2), Sense::hover());
                kit::icons::paint_icon(ui.painter(), rect, "weld_groove_v", 1.0);
                label(ui, "ui.weld.detected", t, false);
                label(ui, "ui.weld.from_parts", t, true);
            });
        } else if kind == WhereKind::Line {
            ui.horizontal(|ui| {
                toggle_chip(ui, "ui.weld.tangent_chain", "weld_tangent_chain", &mut state.tangent_chain);
                if state.sheet_picked {
                    toggle_chip(ui, "ui.weld.flip_side", "weld_flip_side", &mut state.flip_side);
                }
            });
            if state.sheet_picked {
                label(ui, "ui.weld.flip_hint", t, true);
            }
        } else if kind == WhereKind::Point {
            let y = ui.cursor().top();
            ui.painter().extend(kit::dashed_line(
                &[pos2(ui.max_rect().left(), y), pos2(ui.max_rect().right(), y)],
                ui.clip_rect(),
                t.control_border_stroke(t.dialog_rule),
                t.space_4,
                t.space_3,
            ));
            ui.add_space(t.space_8);
            // The kit's fixed-label gutter is 14: 88 + 14 retains the board's 92 + 10 control edge.
            kit::settings_form(ui, t.form_label_width + t.space_24 - t.space_6, |ui| {
                ui.spacing_mut().item_spacing.y = t.space_6;
                ui.label(RichText::new(tr("ui.weld.pattern")).font(kit::type_face::semibold(t.font_body)).color(t.form_ink));
                kit::sized_row(ui, tr("ui.weld.count"), t.control_height + t.space_2, |ui| {
                    ui.set_width(t.form_label_width + t.space_16 + t.space_4);
                    kit::field(ui, &mut state.pattern_count);
                });
                kit::sized_row(ui, tr("ui.weld.pitch"), t.control_height + t.space_2, |ui| {
                    ui.set_width(t.form_label_width + 2.0 * t.space_24 + t.space_2);
                    expression_unit(ui, &mut state.pattern_pitch, "ui.weld.mm", t);
                });
                kit::sized_row(ui, tr("ui.weld.edge_distance"), t.control_height + t.space_2, |ui| {
                    ui.set_width(t.form_label_width + 2.0 * t.space_24 + t.space_2);
                    expression_unit(ui, &mut state.edge_distance, "ui.weld.mm", t);
                });
            });
        }
    });
}

fn expression_unit(ui: &mut Ui, value: &mut String, unit: &str, t: &Tokens) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), Sense::hover());
    let width = ui.painter().layout_no_wrap(tr(unit).into(), FontId::proportional(t.font_body), t.form_dim).size().x;
    let field = Rect::from_min_max(rect.min, pos2((rect.right() - width - t.space_6).max(rect.left()), rect.bottom()));
    let mut field_ui = child(ui, field, "expression");
    field_ui.set_clip_rect(ui.clip_rect().intersect(field));
    kit::field(&mut field_ui, value);
    ui.painter().text(rect.right_center(), Align2::RIGHT_CENTER, tr(unit), FontId::proportional(t.font_body), t.form_dim);
    rect
}

fn face(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements) {
    if state.weld_type == WeldType::Spot {
        grey(ui, t, m, "face", tr("ui.weld.face.spot"));
        return;
    }
    if matches!(state.weld_type, WeldType::Overlay | WeldType::Stud | WeldType::FrictionStir | WeldType::Friction) {
        grey(ui, t, m, "face", tr("ui.weld.face.unused"));
        return;
    }
    section(ui, t, m, "face", |ui, m| {
        let choices = [
            (Face::Flat, "weld_face_flat", if state.weld_type == WeldType::Plug { "ui.weld.flush" } else { "ui.weld.flat" }),
            (Face::Convex, "weld_face_convex", "ui.weld.convex"),
            (Face::Concave, "weld_face_concave", "ui.weld.concave"),
        ];
        let count = if state.weld_type == WeldType::Groove { 2 } else { 3 };
        let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height * 2.0), Sense::hover());
        let width = (band.width() - (count - 1) as f32 * t.space_6) / count as f32;
        for (i, (face, icon, text)) in choices.iter().take(count).enumerate() {
            let rect = Rect::from_min_size(band.min + vec2(i as f32 * (width + t.space_6), 0.0), vec2(width, band.height()));
            if icon_choice(ui, t, m, rect, icon, icon, tr(text), state.face == *face).clicked() {
                state.face = *face;
            }
        }
        if matches!(state.weld_type, WeldType::Fillet | WeldType::Groove) {
            ui.add_space(t.space_2);
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.y = t.space_16;
                ui.add_enabled_ui(state.weld_type == WeldType::Groove || state.face == Face::Convex, |ui| {
                    kit::checkbox(ui, tr("ui.weld.blend"), &mut state.blend_toes)
                });
                label(ui, if state.weld_type == WeldType::Groove { "ui.weld.overlap_h" } else { "ui.weld.needs_convex" }, t, true);
            });
        }
    });
}

fn extent(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements) {
    if state.weld_type.info().kind != WhereKind::Line {
        let id = if state.weld_type == WeldType::Spot {
            "ui.weld.extent.spot".into()
        } else {
            format!("ui.weld.extent.{}", state.weld_type.info().label.trim_start_matches("ui.weld.type."))
        };
        grey(ui, t, m, "extent", tr(&id));
        return;
    }
    section(ui, t, m, "extent", |ui, _| {
        kit::settings_form(ui, t.form_label_width + t.space_6, |ui| {
            kit::sized_row(ui, tr("ui.weld.extent"), t.space_16, |ui| label(ui, "ui.weld.whole_edge", t, false));
            kit::checkbox(ui, tr("ui.weld.spans"), &mut state.spans);
            if state.spans {
                kit::expr_row(ui, tr("ui.weld.pitch"), &mut state.pattern_pitch);
                kit::expr_row(ui, tr("ui.weld.count"), &mut state.pattern_count);
            }
            toggle_chip(ui, "ui.weld.both_sides", "weld_both_sides", &mut state.both_sides);
            kit::sized_row(ui, tr("ui.weld.corners"), t.space_16 + t.space_16 * 2.0, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = t.space_2;
                    label(ui, "ui.weld.automatic", t, false);
                    label(ui, "ui.weld.corner_hint", t, true);
                })
            });
        });
    });
}

fn flags(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements) {
    section(ui, t, m, "flags", |ui, _| {
        ui.horizontal(|ui| {
            toggle_chip(ui, "ui.weld.tack", "weld_flag_tack", &mut state.tack);
            toggle_chip(ui, "ui.weld.field", "weld_flag_field", &mut state.field_weld);
            toggle_chip(ui, "ui.weld.all_around", "weld_flag_all_around", &mut state.all_around);
        });
    });
}

fn notices(ui: &mut Ui, state: &WeldPanelState, t: &Tokens, m: &mut Measurements, event: &mut WeldPanelEvent) {
    section(ui, t, m, "notices", |ui, m| {
        if state.notices.is_empty() {
            label(ui, "ui.weld.no_notices", t, true);
        }
        // Stable sort retains source order among notices of equal severity.
        let mut order: Vec<_> = state.notices.iter().enumerate().collect();
        order.sort_by_key(|(_, n)| match n.kind {
            NoticeKind::Refusal | NoticeKind::Lost => 0,
            NoticeKind::Clash => 1,
            NoticeKind::Warn => 2,
            NoticeKind::Info => 3,
        });
        for (index, notice) in order {
            let (ink, fill) = match notice.kind {
                NoticeKind::Warn => (t.pref_note, t.pref_note_fill),
                NoticeKind::Clash | NoticeKind::Refusal | NoticeKind::Lost => (t.form_error, t.form_error_fill),
                NoticeKind::Info => (t.form_ink, t.selected_bg),
            };
            let frame = Container::floating_card(Surface::Navigation)
                .frame(ui.ctx(), fill, kit::frame::Side::Left)
                .stroke(egui::Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(t.space_8 as i8, t.space_6 as i8));
            Container::show_backing(ui, frame, |ui| {
                let width = ui.available_width();
                let action_label = match &notice.action {
                    Some(NoticeAction::UseFit(value)) => Some(solidcraft_ui_text::format("ui.weld.use_fit", &[("value", value)])),
                    Some(NoticeAction::PickAgain) => Some(tr("ui.weld.pick_again").into()),
                    _ => None,
                };
                let action_width = action_label.as_ref().map_or(0.0, |s| kit::ContentWidth::button(ui, s));
                let text_width = (width - t.space_24 - t.space_8 - if action_label.is_some() { action_width + t.space_8 } else { 0.0 }).max(0.0);
                let galley = ui.painter().layout(tr(notice.message).into(), FontId::proportional(t.font_body), ink, text_width);
                let (row, _) = ui.allocate_exact_size(vec2(width, galley.size().y.max(t.space_24)), Sense::hover());
                // The notice's reveal target ends before its action; it must never steal that click.
                let reveal = Rect::from_min_max(row.min, pos2(row.left() + t.space_24 + t.space_8 + text_width, row.bottom()));
                if ui.interact(reveal, ui.make_persistent_id(("notice", index)), Sense::click()).clicked() {
                    *event = WeldPanelEvent::InspectNotice(index);
                }
                let icon =
                    Rect::from_center_size(pos2(row.left() + t.space_10, row.center().y), vec2(t.space_16 + t.space_4, t.space_16 + t.space_4));
                kit::icons::paint_icon(ui.painter(), icon, icon_name(notice.kind), 1.0);
                ui.painter().galley(pos2(row.left() + t.space_24 + t.space_8, row.center().y - galley.size().y / 2.0), galley, ink);
                if let Some(caption) = action_label {
                    let rect = Rect::from_center_size(pos2(row.right() - action_width / 2.0, row.center().y), vec2(action_width, t.space_24));
                    m.notice_actions.push((index, rect));
                    let mut action_ui = child(ui, rect, ("notice_action", index));
                    if kit::button_at(&mut action_ui, "action", rect, RichText::new(caption).color(ink), Button::DangerOutlined).clicked() {
                        *event = if matches!(notice.action, Some(NoticeAction::UseFit(_))) {
                            WeldPanelEvent::UseFit(index)
                        } else {
                            WeldPanelEvent::PickAgain(index)
                        };
                    }
                }
                if let Some(NoticeAction::Stretches(chips)) = &notice.action {
                    ui.horizontal_wrapped(|ui| {
                        for (stretch, id) in chips.iter().enumerate() {
                            if kit::button(ui, tr(id), Button::DangerOutlined).clicked() {
                                *event = WeldPanelEvent::WeldStretch(index, stretch);
                            }
                        }
                    });
                }
            });
        }
    });
}

fn icon_name(kind: NoticeKind) -> &'static str {
    match kind {
        NoticeKind::Warn => "weld_state_tight",
        NoticeKind::Clash => "weld_state_clash",
        NoticeKind::Refusal | NoticeKind::Lost => "weld_state_lost_reference",
        NoticeKind::Info => "weld_state_no_mass",
    }
}

fn advanced(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements) {
    if state.advanced_open && state.weld_type == WeldType::Fillet {
        section(ui, t, m, "pro", |ui, _| {
            let heading_y = ui.cursor().top() - t.space_6 - (t.space_24 - t.space_2) / 2.0;
            ui.painter().text(
                pos2(ui.max_rect().right(), heading_y),
                Align2::RIGHT_CENTER,
                tr("ui.weld.pro_fields"),
                FontId::proportional(t.font_caption),
                t.form_dim,
            );
            kit::row(ui, tr("ui.weld.legs"), |ui| {
                ui.columns(2, |columns| {
                    if let Some(ui) = columns.get_mut(0) {
                        kit::field(ui, &mut state.size.z1);
                    }
                    if let Some(ui) = columns.get_mut(1) {
                        kit::field(ui, &mut state.size.z);
                    }
                })
            });
            kit::row(ui, tr("ui.weld.density"), |ui| expression_unit(ui, &mut state.density, "ui.weld.kg_m3", t));
            label(ui, "ui.weld.no_mass_hint", t, true);
            kit::row(ui, tr("ui.weld.overlap"), |ui| expression_unit(ui, &mut state.overlap, "ui.weld.mm", t));
            kit::checkbox(ui, tr("ui.weld.different_halves"), &mut state.different_halves);
            kit::checkbox(ui, tr("ui.weld.sharp_mitre"), &mut state.sharp_mitre);
            kit::checkbox(ui, tr("ui.weld.merge_crossing"), &mut state.merge_crossing);
            kit::row(ui, tr("ui.weld.spans_short"), |ui| {
                let mut selected = usize::from(state.staggered);
                kit::segmented(
                    ui,
                    &mut selected,
                    &[tr("ui.weld.chain"), tr("ui.weld.staggered")],
                    kit::Segments { compact: true, icons: &["weld_extent_chain", "weld_extent_staggered"], ..Default::default() },
                );
                state.staggered = selected == 1;
            });
            kit::row(ui, tr("ui.weld.ends"), |ui| {
                kit::segmented(
                    ui,
                    &mut state.ends,
                    &[tr("ui.weld.square"), tr("ui.weld.tapered"), tr("ui.weld.rounded")],
                    kit::Segments {
                        compact: true,
                        glyphs: Some(&|painter, rect, index, _| {
                            kit::icons::paint_icon(
                                painter,
                                Rect::from_center_size(rect.center(), vec2(t.space_24, t.space_24)),
                                ["weld_end_square", "weld_end_tapered", "weld_end_rounded"].get(index).copied().unwrap_or("weld_end_square"),
                                1.0,
                            )
                        }),
                        ..Default::default()
                    },
                );
            });
            kit::choice_row(ui, tr("ui.weld.reference_plane"), "reference_plane", &mut state.reference_plane, &[tr("ui.weld.sketch_plane_off")]);
            label(ui, "ui.weld.floating_hint", t, true);
        });
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height + t.space_8), Sense::hover());
    rule(ui, t, rect.top());
    let disclosure = Rect::from_center_size(pos2(rect.left() + t.space_14 + t.space_6, rect.center().y), vec2(t.space_14, t.space_14));
    // The kit exposes the disclosure glyph and its small button, but no full text row.
    // One invisible hit area covers the whole row without painting over the rounded card.
    let response = ui.interact(rect, ui.make_persistent_id("advanced"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    kit::disclosure(ui.painter(), disclosure.center(), state.advanced_open, t.form_dim);
    let text_rect = Rect::from_min_max(pos2(disclosure.right() + t.space_6, rect.top()), rect.max);
    let title = ui.painter().layout_no_wrap(tr("ui.weld.section.advanced").into(), kit::type_face::semibold(t.font_label), t.form_ink);
    ui.painter().galley(pos2(text_rect.left(), text_rect.center().y - title.size().y / 2.0), title.clone(), t.form_ink);
    ui.painter().text(
        pos2(text_rect.left() + title.size().x + t.space_6, text_rect.center().y),
        Align2::LEFT_CENTER,
        tr("ui.weld.pro_fields"),
        FontId::proportional(t.font_label),
        t.form_dim,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::DisclosureTriangle, ui.is_enabled(), state.advanced_open, tr("ui.weld.section.advanced"))
    });
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_expanded(state.advanced_open));
    if response.clicked() {
        response.request_focus();
        state.advanced_open = !state.advanced_open;
    }
    m.sections.push(("advanced", rect));
}

pub(super) fn show_measured(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens) -> (WeldPanelEvent, Measurements) {
    let mut event = WeldPanelEvent::None;
    let mut measured = Measurements::default();
    let frame = kit::Panel::feature_frame(ui.ctx(), kit::LayoutMode::Floating, kit::frame::Side::Left);
    let response = Container::show_backing(ui, frame, |ui| {
        ui.set_width(card_width(t) - frame.total_margin().sum().x);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            kit::scope(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(t.space_6, 0.0);
                ui.spacing_mut().interact_size.y = t.space_14;
                chrome::header(ui, state, t, &mut measured, &mut event);
                types(ui, state, t, &mut measured, &mut event);
                where_section(ui, state, t, &mut measured, &mut event);
                size::show(ui, state, t, &mut measured);
                face(ui, state, t, &mut measured);
                extent(ui, state, t, &mut measured);
                flags(ui, state, t, &mut measured);
                look::show(ui, state, t, &mut measured);
                notices(ui, state, t, &mut measured, &mut event);
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_footer_height), Sense::hover());
                rule(ui, t, rect.top());
                let button = rect.shrink2(vec2(t.space_14, t.space_10));
                if at_button(ui, &mut measured, button, "new_bead", tr("ui.weld.new_bead"), Button::Normal)
                    .backing_on_hover_text(tr("ui.weld.new_bead_tip"))
                    .clicked()
                {
                    event = WeldPanelEvent::NewBead;
                }
                kit::icons::paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        pos2(button.center().x - t.form_label_width, button.center().y),
                        vec2(t.space_16 + t.space_2, t.space_16 + t.space_2),
                    ),
                    "weld_new_bead",
                    1.0,
                );
                measured.sections.push(("buttons", rect));
                advanced(ui, state, t, &mut measured);
            })
        });
    });
    measured.card = Some(response.response.rect);
    (event, measured)
}
