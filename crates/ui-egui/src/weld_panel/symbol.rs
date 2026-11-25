//! Local ISO A preview; this does not create a model weld or a drawing annotation.
use super::*;
use egui::{FontId, vec2};
use solidcraft_engine::geom::weld;

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
    let (arrow_side, other_side) = if state.both_sides {
        (Some(side.clone()), Some(side))
    } else if on_other_side(state) {
        (None, Some(side))
    } else {
        (Some(side), None)
    };
    let record = weld::Weld {
        arrow_side,
        other_side,
        all_round: state.all_around,
        field: state.field_weld,
        tail: super::sections::look::process_number(state.process).map(str::to_owned),
    };
    record.validate()?;
    Ok(record)
}

/// The weld sits on the far side of the joint: the symbol goes above the dashed line.
fn on_other_side(state: &WeldPanelState) -> bool {
    match state.weld_type {
        WeldType::Groove => state.root_side == 1,
        _ => state.flip_side,
    }
}

fn basic(state: &WeldPanelState) -> Option<weld::Weld> {
    let kind = match state.weld_type {
        WeldType::Fillet => weld::Kind::Fillet,
        WeldType::Groove => weld::Kind::SingleVButt,
        _ => return None,
    };
    let side = Some(weld::WeldSide { kind, size: None, length_mm: None, intermittent: None, contour: None });
    let other = on_other_side(state);
    Some(weld::Weld {
        arrow_side: if other { None } else { side.clone() },
        other_side: if other { side } else { None },
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
    // One continuous reference line, as on the board's strip (no dashed identification line).
    let h = f64::from(t.font_caption);
    let width = f64::from(t.space_2 / 2.0);
    let mut drawing =
        record.draw(weld::Convention::Iso2553_2019B, weld::Style { height_mm: h, width_mm: width }, format_number, |text, height| {
            f64::from(ui.painter().layout_no_wrap(text.into(), FontId::proportional(height as f32), t.form_ink).size().x)
        })?;
    add_arrow(&mut drawing, h, width);
    Ok(drawing)
}

/// The arrow leader at the left end of the reference line, pointing down-left at the joint.
fn add_arrow(drawing: &mut weld::Drawing, h: f64, width: f64) {
    use solidcore_record::{Loop2, Region2, Seg2, Vec2};
    use solidcraft_engine::geom::paper::Item;
    let tip = Vec2::new(-1.4 * h, -2.0 * h);
    let along = Vec2::new(-0.57, -0.82);
    let side = Vec2::new(0.82, -0.57);
    let base = Vec2::new(tip.x - along.x * 1.2 * h, tip.y - along.y * 1.2 * h);
    let left = Vec2::new(base.x + side.x * 0.45 * h, base.y + side.y * 0.45 * h);
    let right = Vec2::new(base.x - side.x * 0.45 * h, base.y - side.y * 0.45 * h);
    drawing.items.push(Item::Stroke { path: vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: base }], closed: false, width_mm: width, pattern: None });
    drawing.items.push(Item::Fill {
        region: Region2 {
            outer: Loop2 { segs: vec![Seg2::Line { a: tip, b: left }, Seg2::Line { a: left, b: right }, Seg2::Line { a: right, b: tip }] },
            holes: Vec::new(),
        },
    });
}

fn prepared(ui: &Ui, state: &WeldPanelState, t: &Tokens) -> (Option<weld::Drawing>, bool) {
    let complete = record(state).and_then(|record| drawing(ui, &record, t));
    let defined = complete.is_ok();
    (complete.ok().or_else(|| basic(state).and_then(|record| drawing(ui, &record, t).ok())), defined)
}

/// The symbol keeps one scale whatever the state: four text heights of paper fill `height`.
pub(super) fn scale(height: f32, t: &Tokens) -> f32 {
    height / (4.0 * t.font_caption)
}

/// Returns whether a complete symbol is defined; the caller uses that for the strip hint.
pub(super) fn paint(ui: &Ui, rect: Rect, state: &WeldPanelState, t: &Tokens) -> bool {
    let (drawing, defined) = prepared(ui, state, t);
    let Some(drawing) = drawing else {
        let icon = Rect::from_center_size(rect.center(), vec2(t.control_height + t.space_4, t.control_height + t.space_4));
        crate::ui_kit::icons::paint_icon(ui.painter(), icon, "weld_symbol", 0.4);
        return false;
    };
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let ink = if defined { t.form_ink } else { t.form_dim };
    let inner = rect.shrink2(vec2(t.space_4, t.space_2));
    if let Some(fit) = crate::weld_symbol_paint::centre_in_rect(&drawing, inner, scale(inner.height(), t), &painter) {
        crate::weld_symbol_paint::paint(&drawing, |point| fit.map(point), fit.pixels_per_mm, ink, 1.0, &painter);
    }
    defined
}
