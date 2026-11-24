//! Local ISO A preview; this does not create a model weld or a drawing annotation.
use super::*;
use egui::{Color32, FontId, Shape, Stroke, pos2, vec2};
use solidcore_record::Seg2;
use solidcraft_engine::geom::{
    Seg2Geom,
    paper::{Dashes, Item},
    weld,
};

pub(super) fn record(state: &WeldPanelState) -> weld::Result<weld::Weld> {
    let (kind, value) = match state.weld_type {
        WeldType::Fillet => (weld::Kind::Fillet, if state.size_by_leg { &state.size.z } else { &state.size.a }),
        WeldType::Groove => (weld::Kind::SingleVButt, &state.size.s),
        _ => return Err(weld::Invalid("unsupported panel type")),
    };
    let value = crate::ui_kit::parse_number(value).ok_or(weld::Invalid("size is not a number"))?;
    let size = match kind {
        weld::Kind::Fillet if state.size_by_leg => weld::Size::Leg(value),
        weld::Kind::Fillet => weld::Size::Throat(value),
        _ => weld::Size::ButtPenetration(value),
    };
    let side = weld::WeldSide {
        kind,
        size: Some(size),
        length_mm: None,
        intermittent: None,
        contour: Some(match state.face {
            Face::Flat => weld::Contour::Flush,
            Face::Convex => weld::Contour::Convex,
            Face::Concave => weld::Contour::Concave,
        }),
    };
    let record = weld::Weld {
        arrow_side: Some(side.clone()),
        other_side: state.both_sides.then_some(side),
        all_round: state.all_around,
        field: state.field_weld,
        tail: super::sections::look::process_number(state.process).map(str::to_owned),
    };
    record.validate()?;
    Ok(record)
}

fn basic(state: &WeldPanelState) -> Option<weld::Weld> {
    let kind = match state.weld_type {
        WeldType::Fillet => weld::Kind::Fillet,
        WeldType::Groove => weld::Kind::SingleVButt,
        _ => return None,
    };
    Some(weld::Weld {
        arrow_side: Some(weld::WeldSide { kind, size: None, length_mm: None, intermittent: None, contour: None }),
        other_side: None,
        all_round: false,
        field: false,
        tail: None,
    })
}

fn format_number(value: f64) -> weld::Result<(String, f64)> {
    use solidcraft_engine::doc::display_units::{NumberFormat, format_number};
    let shown = format_number(value, 6, &NumberFormat { trailing_zeros: false, ..Default::default() })
        .map_err(|_| weld::Invalid("invalid preview dimension"))?;
    Ok((shown.text, shown.value))
}

pub(super) fn drawing(ui: &Ui, record: &weld::Weld, t: &Tokens) -> weld::Result<weld::Drawing> {
    record.draw(
        weld::Convention::Iso2553_2019A { identification: weld::IdentificationLine::Above },
        weld::Style { height_mm: f64::from(t.font_caption), width_mm: f64::from(t.space_2 / 2.0) },
        format_number,
        |text, height| f64::from(ui.painter().layout_no_wrap(text.into(), FontId::proportional(height as f32), t.form_ink).size().x),
    )
}

fn points(path: &[Seg2]) -> Vec<egui::Pos2> {
    // These are bounded, locally generated symbol curves, flattened for display only.
    let mut points: Vec<_> = path.iter().flat_map(|seg| seg.polyline(0.05)).map(|p| pos2(p.x as f32, -p.y as f32)).collect();
    points.dedup();
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    points
}

fn dashed(points: &[egui::Pos2], stroke: Stroke, pattern: &Dashes) -> Vec<Shape> {
    // Paper phase consumes the pattern at the start; egui's dash offset instead adds a gap.
    let mut phase = pattern.phase_mm as f32;
    let mut index = 0;
    for (i, length) in pattern.lengths_mm.iter().enumerate() {
        index = i;
        if phase < *length as f32 {
            break;
        }
        phase -= *length as f32;
    }
    let Some(length) = pattern.lengths_mm.get(index) else { return Vec::new() };
    let mut remaining = *length as f32 - phase;
    let mut shapes = Vec::new();
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        let mut along = 0.0;
        // Draw's validated local pattern has positive lengths and bounded coordinates.
        for _ in 0..10_000 {
            if along >= length || remaining <= 0.0 {
                break;
            }
            let available = length - along;
            let finishes_dash = remaining <= available;
            let end = if finishes_dash { along + remaining } else { length };
            if index.is_multiple_of(2) {
                shapes.push(Shape::line_segment([pair[0].lerp(pair[1], along / length), pair[0].lerp(pair[1], end / length)], stroke));
            }
            if finishes_dash {
                index = (index + 1) % pattern.lengths_mm.len();
                let Some(next) = pattern.lengths_mm.get(index) else { break };
                remaining = *next as f32;
            } else {
                remaining -= available;
            }
            along = end;
        }
    }
    shapes
}

fn shapes(ui: &Ui, drawing: &weld::Drawing, color: Color32) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for item in &drawing.items {
        match item {
            Item::Stroke { path, closed, width_mm, pattern } => {
                let mut points = points(path);
                let stroke = Stroke::new(*width_mm as f32, color);
                if let Some(pattern) = pattern {
                    if *closed && let Some(first) = points.first().copied() {
                        points.push(first);
                    }
                    shapes.extend(dashed(&points, stroke, pattern));
                } else if *closed {
                    shapes.push(Shape::closed_line(points, stroke));
                } else {
                    shapes.push(Shape::line(points, stroke));
                }
            }
            // Weld::draw currently emits only a convex triangular field flag, without holes.
            Item::Fill { region } => shapes.push(Shape::convex_polygon(points(&region.outer.segs), color, Stroke::NONE)),
            Item::Text(_) | Item::Picture(_) => {}
        }
    }
    for label in &drawing.text {
        let galley = ui.painter().layout_no_wrap(label.text.clone(), FontId::proportional(label.height_mm as f32), color);
        let baseline = galley.rows.first().and_then(|r| r.glyphs.first().map(|g| r.pos.y + g.pos.y)).unwrap_or(0.0);
        let at = pos2(label.baseline.x as f32, -label.baseline.y as f32 - baseline);
        shapes.push(Shape::galley(at, galley, color));
    }
    shapes
}

/// Returns whether a complete symbol is defined; the caller uses that for the strip hint.
pub(super) fn paint(ui: &Ui, rect: Rect, state: &WeldPanelState, t: &Tokens) -> bool {
    let complete = record(state).and_then(|record| drawing(ui, &record, t));
    let defined = complete.is_ok();
    let drawing = complete.ok().or_else(|| basic(state).and_then(|record| drawing(ui, &record, t).ok()));
    let Some(drawing) = drawing else {
        let icon = Rect::from_center_size(rect.center(), vec2(t.control_height + t.space_4, t.control_height + t.space_4));
        crate::ui_kit::icons::paint_icon(ui.painter(), icon, "weld_symbol", 0.4);
        return false;
    };
    let mut shapes = shapes(ui, &drawing, if defined { t.form_ink } else { t.form_dim });
    let bounds = shapes.iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()));
    if !bounds.is_finite() || !bounds.is_positive() {
        return false;
    }
    let inner = rect.shrink(t.space_4);
    let scale = (inner.width() / bounds.width()).min(inner.height() / bounds.height()).min(1.0);
    let transform = egui::emath::TSTransform::new(inner.center().to_vec2() - bounds.center().to_vec2() * scale, scale);
    for shape in &mut shapes {
        shape.transform(transform);
    }
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).extend(shapes);
    defined
}
