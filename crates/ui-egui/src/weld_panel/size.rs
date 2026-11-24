//! Approved figure-left, expression-column-right size layout.
use super::*;

struct SizeRow<'a> {
    name: &'static str,
    value: &'a mut String,
    hint: Option<&'static str>,
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
        let mut rows = Vec::new();
        let values = &mut state.size;
        let figure = match state.weld_type {
            WeldType::Fillet => {
                if state.size_by_leg {
                    rows.push(SizeRow { name: "z", value: &mut values.z, hint: Some("ui.weld.size.leg_hint") });
                    if state.advanced_open {
                        rows.push(SizeRow { name: "a", value: &mut values.a, hint: None });
                    }
                } else {
                    rows.push(SizeRow { name: "a", value: &mut values.a, hint: Some("ui.weld.size.leg_hint") });
                    if state.advanced_open {
                        rows.push(SizeRow { name: "z", value: &mut values.z, hint: None });
                    }
                }
                if state.advanced_open {
                    rows.push(SizeRow { name: "z1", value: &mut values.z1, hint: None });
                }
                SizeFigure::Fillet
            }
            WeldType::Groove => {
                rows.push(SizeRow { name: "h", value: &mut values.h, hint: None });
                rows.push(SizeRow { name: "s", value: &mut values.s, hint: Some("ui.weld.size.full_depth") });
                SizeFigure::Groove
            }
            WeldType::Spot => {
                rows.push(SizeRow { name: "d", value: &mut values.d, hint: Some("ui.weld.size.proposed") });
                SizeFigure::Spot
            }
            WeldType::Plug => {
                rows.push(SizeRow { name: "fill", value: &mut values.fill, hint: Some("ui.weld.size.hole") });
                SizeFigure::Plug
            }
            WeldType::Stud => {
                rows.push(SizeRow { name: "collar", value: &mut values.collar, hint: None });
                rows.push(SizeRow { name: "fill", value: &mut values.fill, hint: None });
                SizeFigure::Stud
            }
            WeldType::Overlay => {
                rows.push(SizeRow { name: "t", value: &mut values.t, hint: None });
                SizeFigure::Overlay
            }
            _ => {
                rows.push(SizeRow { name: "w", value: &mut values.w, hint: None });
                match state.weld_type.info().kind {
                    WhereKind::Line => SizeFigure::Fillet,
                    WhereKind::Point => SizeFigure::Spot,
                    WhereKind::Hole => SizeFigure::Plug,
                    WhereKind::Ring => SizeFigure::Stud,
                    WhereKind::Area => SizeFigure::Overlay,
                }
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
        let root_height = if state.weld_type == WeldType::Groove { t.space_16 + t.space_2 + t.control_height + row_gap } else { 0.0 };
        let fields_height = heights.iter().sum::<f32>() + row_gap * rows.len().saturating_sub(1) as f32 + root_height;
        let figure_height = fw * 120.0 / 176.0;
        let (body, _) = ui.allocate_exact_size(vec2(ui.available_width(), figure_height.max(fields_height)), Sense::hover());
        m.content = Some(body);
        let fr = Rect::from_min_size(pos2(body.left(), body.center().y - figure_height / 2.0), vec2(fw, figure_height));
        let mut figure_ui = child(ui, fr, "figure");
        m.figure = Some(paint_size_figure(&mut figure_ui, t, figure, state.advanced_open, fw).rect);
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
            m.widgets.push(expression_unit(&mut field_ui, row.value, "ui.weld.mm", t));
            if let Some(galley) = hints.get(index).and_then(Option::as_ref) {
                ui.painter().galley(pos2(rect.left(), y + t.control_height + t.space_2), galley.clone(), t.form_dim);
            }
            m.fields.push(rect);
            y += height + row_gap;
        }
        if state.weld_type == WeldType::Groove {
            let rect = Rect::from_min_size(pos2(column.left(), y), vec2(column_width, root_height));
            let mut root = child(ui, rect, "root");
            root.spacing_mut().item_spacing.y = t.space_2;
            label(&mut root, "ui.weld.root_side", t, true);
            kit::segmented(
                &mut root,
                &mut state.root_side,
                &[tr("ui.weld.arrow"), tr("ui.weld.other")],
                kit::Segments { compact: true, ..Default::default() },
            );
        }
    });
}
