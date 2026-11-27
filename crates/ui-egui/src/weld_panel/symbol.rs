//! Live symbol shared by the panel and saved graphic, using its chosen convention (default ISO A).
use super::*;
use egui::{FontId, vec2};
use solidcraft_engine::geom::weld;

pub(crate) fn record(state: &WeldPanelState) -> weld::Result<weld::Weld> {
    use solidcraft_engine::doc::weld::{GROOVES, WeldSettings, WeldSpans};
    let typ = state.weld_type.info().label.strip_prefix("ui.weld.type.").ok_or(weld::Invalid("unknown panel type"))?;
    let mut size = std::collections::BTreeMap::new();
    if state.weld_type == WeldType::Fillet {
        let (key, value) = if state.size_by_leg { ("z", &state.size.z) } else { ("a", &state.size.a) };
        size.insert(key.into(), crate::ui_kit::parse_number(value).ok_or(weld::Invalid("size is not a number"))?);
        for (key, value) in [("z1", &state.size.z1), ("z2", &state.size.z2)] {
            if !value.trim().is_empty() {
                size.insert(key.into(), crate::ui_kit::parse_number(value).ok_or(weld::Invalid("leg is not a number"))?);
            }
        }
    }
    if (state.weld_type == WeldType::Fillet && !state.size.s.trim().is_empty()) || (state.weld_type == WeldType::Butt && state.partial_penetration) {
        size.insert("s".into(), crate::ui_kit::parse_number(&state.size.s).ok_or(weld::Invalid("penetration is not a number"))?);
    }
    let spans = if state.spans {
        state
            .span_count
            .trim()
            .parse::<u32>()
            .ok()
            .zip(crate::ui_kit::parse_number(&state.span_length))
            .zip(crate::ui_kit::parse_number(&state.span_pitch))
            .map(|((count, length_mm), pitch_mm)| WeldSpans { count, length_mm, pitch_mm })
    } else {
        None
    };
    WeldSettings {
        weld_type: typ.into(),
        size,
        size_by_leg: state.size_by_leg,
        groove: GROOVES.get(state.groove).copied().unwrap_or("v").into(),
        penetration: if state.partial_penetration { "partial" } else { "full" }.into(),
        face: match state.face {
            Face::Flat => "flat",
            Face::Convex => "convex",
            Face::Concave => "concave",
        }
        .into(),
        side: if on_other_side(state) { "other" } else { "arrow" }.into(),
        spans,
        staggered: state.staggered,
        both_sides: state.both_sides,
        all_around: state.all_around,
        field_weld: state.field_weld,
        process: (!state.process.is_empty()).then(|| state.process.clone()),
        ..Default::default()
    }
    .symbol()
}
/// Manufacturing side; `convention` chooses ISO A's identification-line position for it.
fn on_other_side(state: &WeldPanelState) -> bool {
    state.side == 1
}

fn basic(state: &WeldPanelState) -> Option<weld::Weld> {
    let kind = match state.weld_type {
        WeldType::Fillet => weld::Kind::Fillet,
        WeldType::Butt if state.groove == 0 => weld::Kind::SquareButt,
        WeldType::Butt if state.groove == 1 => weld::Kind::SingleVButt,
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

pub(crate) fn convention(state: &WeldPanelState) -> weld::Convention {
    match state.symbol_standard.as_str() {
        "iso2553_2019_b" => weld::Convention::Iso2553_2019B,
        "aws_a2_4_2020" => weld::Convention::AwsA2_4_2020,
        _ => weld::Convention::Iso2553_2019A {
            identification: if on_other_side(state) { weld::IdentificationLine::Below } else { weld::IdentificationLine::Above },
        },
    }
}

pub(super) fn drawing(ui: &Ui, record: &weld::Weld, state: &WeldPanelState, t: &Tokens) -> weld::Result<weld::Drawing> {
    let h = f64::from(t.font_body);
    let width = f64::from(t.space_2 / 2.0);
    let mut drawing = record.draw(convention(state), weld::Style { height_mm: h, width_mm: width }, format_number, |text, height| {
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
    let complete = record(state).and_then(|record| drawing(ui, &record, state, t));
    let defined = complete.is_ok();
    (complete.ok().or_else(|| basic(state).and_then(|record| drawing(ui, &record, state, t).ok())), defined)
}

/// The symbol keeps one scale whatever the state: four text heights of paper fill `height`.
pub(super) fn scale(height: f32, t: &Tokens) -> f32 {
    height / (6.0 * t.font_body)
}

/// Returns whether a complete symbol is defined; the caller uses that for the strip hint.
pub(super) fn paint(ui: &Ui, rect: Rect, state: &WeldPanelState, t: &Tokens) -> bool {
    let (drawing, defined) = prepared(ui, state, t);
    let Some(drawing) = drawing else {
        let icon = Rect::from_center_size(
            egui::pos2(rect.left() + t.space_24, rect.center().y),
            vec2(t.control_height + t.space_4, t.control_height + t.space_4),
        );
        crate::ui_kit::icons::paint_icon(ui.painter(), icon, state.weld_type.info().icon, 0.4);
        let hint = if state.weld_type == WeldType::Butt { "ui.weld.symbol.shape_unavailable" } else { "ui.weld.symbol.type_unavailable" };
        let text = ui.painter().layout(tr(hint).into(), FontId::proportional(t.font_body), t.form_dim, (rect.width() - t.space_24 * 3.).max(0.));
        ui.painter().galley(egui::pos2(icon.right() + t.space_8, rect.center().y - text.size().y / 2.), text, t.form_dim);
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
