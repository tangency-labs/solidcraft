//! Approved figure-left, expression-column-right size layout.
use super::*;

struct SizeRow<'a> {
    name: &'static str,
    value: &'a mut String,
    hint: Option<&'static str>,
}

pub(in crate::weld_panel) fn leg_hint(state: &WeldPanelState) -> Option<String> {
    let value = if state.size_by_leg { &state.size.z } else { &state.size.a };
    let value = crate::ui_kit::parse_number(value).filter(|v| *v > 0.0)?;
    let (letter, value) = if state.size_by_leg { ("a", value / std::f64::consts::SQRT_2) } else { ("z", value * std::f64::consts::SQRT_2) };
    value.is_finite().then(|| {
        solidcraft_ui_text::format("ui.weld.size.equal_leg", &[("letter", tr(&format!("ui.weld.dim.{letter}"))), ("value", &format!("{value:.3}"))])
    })
}

fn follow_equal_leg(state: &mut WeldPanelState, by_leg: bool) {
    let source = if by_leg { &state.size.z } else { &state.size.a };
    if let Some(value) = crate::ui_kit::parse_number(source).filter(|v| *v > 0.0) {
        let target = if by_leg { &mut state.size.a } else { &mut state.size.z };
        *target = (if by_leg { value / std::f64::consts::SQRT_2 } else { value * std::f64::consts::SQRT_2 }).to_string();
    }
}

pub(super) fn show(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements) {
    section(ui, t, m, "size", |ui, m| {
        if state.weld_type == WeldType::Fillet {
            let heading_y = ui.cursor().top() - t.space_6 - (t.space_24 - t.space_2);
            let rect = Rect::from_min_size(
                pos2(ui.max_rect().right() - t.control_height * 2.0, heading_y),
                vec2(t.control_height * 2.0, t.space_24 - t.space_2),
            );
            let mut control = child(ui, rect, "size_by");
            let mut selected = usize::from(state.size_by_leg);
            kit::segmented(
                &mut control,
                &mut selected,
                &[tr("ui.weld.dim.a"), tr("ui.weld.dim.z")],
                kit::Segments { small: true, compact: true, ..Default::default() },
            );
            state.size_by_leg = selected == 1;
        }
        if state.weld_type == WeldType::Butt {
            kit::row(ui, tr("ui.weld.groove"), |ui| {
                let ids = solidcraft_engine::doc::weld::GROOVES;
                let shape = ids.get(state.groove).copied().unwrap_or("v");
                kit::dropdown_menu(ui, "groove", tr(&format!("ui.weld.groove.{shape}")), |ui| {
                    for (index, shape) in ids.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(t.space_24, t.space_24), Sense::hover());
                            kit::icons::paint_icon(ui.painter(), rect, &format!("weld_groove_{shape}"), 1.0);
                            if kit::button(ui, tr(&format!("ui.weld.groove.{shape}")), Button::Normal).clicked() {
                                state.groove = index;
                                ui.close();
                            }
                        });
                    }
                });
            });
            let mut selected = usize::from(state.partial_penetration);
            kit::segmented(
                ui,
                &mut selected,
                &[tr("ui.weld.penetration.full"), tr("ui.weld.penetration.partial")],
                kit::Segments { compact: true, ..Default::default() },
            );
            state.partial_penetration = selected == 1;
            if state.groove > 1 {
                label(ui, "ui.weld.symbol.shape_unavailable", t, true);
            }
        }
        let figure_state = FigureState::from_panel(state);
        let before_a = state.size.a.clone();
        let before_z = state.size.z.clone();
        let mut rows = Vec::new();
        let values = &mut state.size;
        let figure = match state.weld_type {
            WeldType::Fillet => {
                if state.size_by_leg {
                    rows.push(SizeRow { name: "z", value: &mut values.z, hint: None });
                } else {
                    rows.push(SizeRow { name: "a", value: &mut values.a, hint: None });
                }
                SizeFigure::Fillet
            }
            WeldType::Butt => {
                rows.push(SizeRow { name: "h", value: &mut values.h, hint: None });
                if state.partial_penetration {
                    rows.push(SizeRow { name: "s", value: &mut values.s, hint: None });
                }
                SizeFigure::Groove
            }
            WeldType::Spot => {
                rows.push(SizeRow { name: "d", value: &mut values.d, hint: Some("ui.weld.size.proposed") });
                SizeFigure::Spot
            }
            WeldType::Plug => {
                rows.push(SizeRow { name: "h", value: &mut values.h, hint: Some("ui.weld.size.hole") });
                SizeFigure::Plug
            }
            WeldType::Stud => {
                rows.push(SizeRow { name: "d", value: &mut values.d, hint: None });
                SizeFigure::Stud
            }
            WeldType::Surfacing => {
                rows.push(SizeRow { name: "t", value: &mut values.t, hint: None });
                SizeFigure::Overlay
            }
            WeldType::Tack => {
                rows.push(SizeRow { name: "l", value: &mut values.l, hint: None });
                if state.tack_mode == 0 {
                    rows.push(SizeRow { name: "e", value: &mut values.e, hint: None });
                }
                if state.tack_mode == 1 {
                    rows.push(SizeRow { name: "n", value: &mut values.n, hint: None });
                }
                SizeFigure::Tack
            }
            WeldType::Edge => {
                rows.push(SizeRow { name: "w", value: &mut values.w, hint: None });
                SizeFigure::Edge
            }
            WeldType::Seam => {
                rows.push(SizeRow { name: "w", value: &mut values.w, hint: None });
                SizeFigure::Seam
            }
        };
        let fw = figure_width(t);
        let column_width = (ui.available_width() - fw - t.space_12).max(0.0);
        let row_gap = t.space_6 + t.space_2 / 2.0;
        let hints: Vec<_> = rows
            .iter()
            .map(|row| row.hint.map(|id| ui.painter().layout(tr(id).into(), FontId::proportional(t.font_small), t.form_dim, column_width)))
            .collect();
        let heights: Vec<_> = hints.iter().map(|hint| t.control_height + hint.as_ref().map_or(0.0, |g| t.space_2 + g.size().y)).collect();
        let fields_height = heights.iter().sum::<f32>() + row_gap * rows.len().saturating_sub(1) as f32;
        let figure_height = fw * 120.0 / 176.0;
        let (body, _) = ui.allocate_exact_size(vec2(ui.available_width(), figure_height.max(fields_height)), Sense::hover());
        m.content = Some(body);
        let fr = Rect::from_min_size(pos2(body.left(), body.center().y - figure_height / 2.0), vec2(fw, figure_height));
        let mut figure_ui = child(ui, fr, "figure");
        let painted = paint_size_figure(&mut figure_ui, t, figure, &figure_state, fw);
        m.figure = Some(painted.response.rect);
        if let Some(letter) = painted.focus {
            state.focus_dimension = Some(letter);
            if state.weld_type == WeldType::Fillet && matches!(letter, "a" | "z") {
                state.size_by_leg = letter == "z";
            }
        }
        let column = Rect::from_min_size(pos2(fr.right() + t.space_12, body.center().y - fields_height / 2.0), vec2(column_width, fields_height));
        m.field_column = Some(column);
        let mut y = column.top();
        for (index, row) in rows.iter_mut().enumerate() {
            let height = heights.get(index).copied().unwrap_or(t.control_height);
            let rect = Rect::from_min_size(pos2(column.left(), y), vec2(column_width, height));
            let label_width = t.space_24 + t.space_16 + t.space_6;
            let mut letter = child(ui, Rect::from_min_size(rect.min, vec2(label_width, t.control_height)), ("letter", row.name));
            letter.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new(tr(&format!("ui.weld.dim.{}", row.name))).font(FontId::monospace(t.font_body)).italics().color(t.form_dim));
            });
            let field = Rect::from_min_max(pos2(rect.left() + label_width + t.space_6, y), pos2(rect.right(), y + t.control_height));
            let mut field_ui = child(ui, field, row.name);
            let focus = state.focus_dimension == Some(row.name);
            m.widgets.push(expression_unit_focus(
                &mut field_ui,
                row.value,
                if row.name == "n" { "ui.weld.unit.count" } else { "ui.weld.mm" },
                t,
                focus,
            ));
            if focus {
                state.focus_dimension = None;
            }
            if let Some(galley) = hints.get(index).and_then(Option::as_ref) {
                ui.painter().galley(pos2(rect.left(), y + t.control_height + t.space_2), galley.clone(), t.form_dim);
            }
            m.fields.push(rect);
            y += height + row_gap;
        }
        if state.weld_type == WeldType::Fillet {
            if before_a != state.size.a {
                follow_equal_leg(state, false);
            } else if before_z != state.size.z {
                follow_equal_leg(state, true);
            }
            if let Some(hint) = leg_hint(state) {
                ui.label(RichText::new(hint).size(t.font_body).color(t.form_dim));
            }
        }
        advanced(ui, state, t, m);
        if !matches!(state.weld_type, WeldType::Fillet | WeldType::Butt) {
            label(ui, "ui.weld.symbol.type_unavailable", t, true);
        }
        if state.weld_type == WeldType::Fillet && (!state.size.z1.trim().is_empty() || !state.size.z2.trim().is_empty()) {
            label(ui, "ui.weld.symbol.legs_unavailable", t, true);
        }
        if state.symbol_standard == "aws_a2_4_2020"
            && match state.weld_type {
                WeldType::Fillet => !state.size_by_leg || !state.size.s.trim().is_empty() || state.spans,
                WeldType::Butt => state.partial_penetration,
                WeldType::Edge | WeldType::Plug | WeldType::Spot | WeldType::Seam | WeldType::Stud | WeldType::Surfacing | WeldType::Tack => false,
            }
        {
            label(ui, "ui.weld.symbol.size_unavailable", t, true);
        }
    });
}
