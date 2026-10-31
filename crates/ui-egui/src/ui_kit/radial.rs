//! Analytic rounded annular sectors, following the board's offset-and-round construction.
use crate::{context_menu::Item, theme::Tokens};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

pub(crate) const RADIAL_RADIUS: f32 = 150.0;
const INNER: f32 = 56.0;
const GAP: f32 = 4.0;
const STEPS: usize = 16;

fn unit(a: f32) -> Vec2 {
    vec2(a.cos(), a.sin())
}

/// Two equally sampled boundaries; their ends are joined by parallel straight sides.
/// The four joins are circular arcs of the theme radius, tangent to both adjacent edges.
fn boundaries(k: usize) -> (Vec<Pos2>, Vec<Pos2>) {
    // The outer and inner circles are functional geometry; only the corner joins vary.
    // Bound the radius so the inner arc remains ordered, even for hostile theme data.
    let corner = f32::from(Tokens::get().radius_xl.saturating_sub(1)).min(12.0);
    let am = -FRAC_PI_2 + (k % 8) as f32 * FRAC_PI_4;
    let a0 = am - FRAC_PI_4 / 2.0;
    let a1 = am + FRAC_PI_4 / 2.0;
    let d = GAP / 2.0 + corner;
    let ro = RADIAL_RADIUS - corner;
    let ri = INNER + corner;
    let outer0 = a0 + (d / ro).asin();
    let outer1 = a1 - (d / ro).asin();
    let inner0 = a0 + (d / ri).asin();
    let inner1 = a1 - (d / ri).asin();
    let po0 = Pos2::ZERO + unit(outer0) * ro;
    let po1 = Pos2::ZERO + unit(outer1) * ro;
    let pi0 = Pos2::ZERO + unit(inner0) * ri;
    let pi1 = Pos2::ZERO + unit(inner1) * ri;
    let arc = |out: &mut Vec<Pos2>, center: Pos2, radius: f32, from: f32, to: f32| {
        for j in 0..=STEPS {
            out.push(center + unit(from + (to - from) * j as f32 / STEPS as f32) * radius);
        }
    };
    let mut outer = Vec::new();
    arc(&mut outer, po0, corner, a0 - FRAC_PI_2, outer0);
    arc(&mut outer, Pos2::ZERO, RADIAL_RADIUS, outer0, outer1);
    arc(&mut outer, po1, corner, outer1, a1 + FRAC_PI_2);
    let mut inner = Vec::new();
    arc(&mut inner, pi0, corner, a0 - FRAC_PI_2, inner0 - PI);
    arc(&mut inner, Pos2::ZERO, INNER, inner0, inner1);
    arc(&mut inner, pi1, corner, inner1 - PI, a1 + FRAC_PI_2 - TAU);
    (outer, inner)
}

fn contains(k: usize, at: Vec2) -> bool {
    if !at.is_finite() || !(INNER..=RADIAL_RADIUS).contains(&at.length()) {
        return false;
    }
    let (outer, inner) = boundaries(k);
    let outline: Vec<_> = outer.into_iter().chain(inner.into_iter().rev()).collect();
    let Some(last) = outline.last() else {
        return false;
    };
    let mut previous = *last;
    let mut inside = false;
    for current in outline {
        if (previous.y > at.y) != (current.y > at.y) {
            let x = (current.x - previous.x) * (at.y - previous.y) / (current.y - previous.y) + previous.x;
            if at.x < x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn paint_sector(p: &egui::Painter, center: Pos2, k: usize, fill: Color32, stroke: Stroke) {
    let (outer, inner) = boundaries(k);
    let mut mesh = egui::Mesh::default();
    for (o, i) in outer.iter().zip(&inner) {
        mesh.colored_vertex(*o + center.to_vec2(), fill);
        mesh.colored_vertex(*i + center.to_vec2(), fill);
    }
    for j in 0..outer.len().saturating_sub(1) {
        let a = (j * 2) as u32;
        mesh.add_triangle(a, a + 2, a + 1);
        mesh.add_triangle(a + 1, a + 2, a + 3);
    }
    p.add(egui::Shape::mesh(mesh));
    let outline = outer.iter().chain(inner.iter().rev()).map(|p| *p + center.to_vec2()).collect();
    p.add(egui::Shape::closed_line(outline, stroke));
}

pub(crate) fn radial_menu(ui: &mut Ui, center: Pos2, items: &[Item], hot: Option<usize>) -> Option<Item> {
    let t = Tokens::get();
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let mut picked = None;
    for (k, item) in items.iter().take(8).enumerate() {
        let enabled = item.enabled && ui.is_enabled();
        let lit = enabled && hot == Some(k);
        let mut painter = ui.painter().clone();
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        paint_sector(
            &painter,
            center,
            k,
            if lit { accent.soft } else { t.dialog_body },
            Stroke::new(if lit { 1.6 } else { 1.0 }, if lit { accent.color } else { t.dialog_border }),
        );
        let a = -FRAC_PI_2 + k as f32 * FRAC_PI_4;
        let at = center + unit(a) * 104.0;
        let target = Rect::from_center_size(at, vec2(t.space_24 * 2.0 + t.space_12, t.space_24 + t.space_16 + t.space_2));
        let response = ui.interact(target, ui.scope_id().with(("radial", k)), Sense::hover());
        crate::scenario::publish_target(&format!("radial:{}", item.id), target);
        let ink = if lit { accent.ink } else { t.form_ink };
        crate::icons::paint_accented(
            &painter,
            Rect::from_center_size(at - vec2(0.0, t.space_6), vec2(t.space_24, t.space_24)),
            &item.icon,
            crate::theme::TabAccent::current_icon_accent(ui.ctx()),
            1.0,
        );
        let label = crate::i18n::tr_label(&item.label);
        let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), FontId::proportional(t.font_small), ink);
        job.wrap.max_width = 74.0;
        job.wrap.max_rows = 1;
        let galley = painter.layout_job(job);
        painter.galley(at + vec2(-galley.size().x / 2.0, t.space_10), galley, ink);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
        if lit && ui.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| contains(k, p - center))) {
            picked = Some(item.clone());
        }
    }
    let p = ui.painter();
    p.circle(center, INNER - t.space_6, t.dialog_body, Stroke::new(1.0, t.dialog_border));
    if let Some(item) = hot.and_then(|k| items.get(k)) {
        p.text(
            center - vec2(0.0, t.space_6),
            Align2::CENTER_CENTER,
            crate::i18n::tr_label(&item.label),
            FontId::proportional(t.font_label),
            t.form_ink,
        );
        p.text(
            center + vec2(0.0, t.space_14),
            Align2::CENTER_CENTER,
            crate::i18n::tr_label(&item.shortcut),
            FontId::monospace(t.font_small),
            t.form_dim,
        );
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_clicks_ignore_the_disc_gaps_outside_and_nonfinite_positions() {
        for k in 0..8 {
            let a = -FRAC_PI_2 + k as f32 * FRAC_PI_4;
            assert!(contains(k, unit(a) * 104.0));
            for at in [Vec2::ZERO, unit(a) * 200.0, vec2(f32::NAN, 0.0), vec2(f32::INFINITY, 0.0), unit(a + FRAC_PI_4 / 2.0) * 104.0] {
                assert!(!contains(k, at), "sector {k}: {at:?}");
            }
        }
    }

    #[test]
    fn neighbouring_sectors_have_parallel_four_point_gaps_and_four_round_corners() {
        for k in 0..8 {
            let (outer, inner) = boundaries(k);
            let (next_outer, next_inner) = boundaries(k + 1);
            // These are the actual ends of the straight sides, at both ends of the ring.
            let outer_gap = outer.last().unwrap().distance(next_outer.first().copied().unwrap());
            let inner_gap = inner.last().unwrap().distance(next_inner.first().copied().unwrap());
            assert!((outer_gap - GAP).abs() < 0.01, "{k}: {outer_gap}");
            assert!((inner_gap - GAP).abs() < 0.01, "{k}: {inner_gap}");
            assert!((outer_gap - inner_gap).abs() < 0.5);
            for boundary in [&outer, &inner] {
                for corner in [0, 2 * (STEPS + 1)] {
                    let points = &boundary[corner..corner + STEPS + 1];
                    // Rounded joins contain an arc, with no concentrated sharp turn.
                    let mut turning = 0.0;
                    for triple in points.windows(3) {
                        let a = (triple[1] - triple[0]).normalized();
                        let b = (triple[2] - triple[1]).normalized();
                        let angle = a.dot(b).clamp(-1.0, 1.0).acos();
                        assert!(angle < 0.12, "sharp corner: {angle}");
                        turning += angle;
                    }
                    assert!(turning > 1.0, "corner must be a curved arc");
                }
            }
        }
    }

    #[test]
    fn theme_corner_extremes_keep_radial_geometry_finite_and_clicks_in_their_sectors() {
        for radius in [0, 1, 2, 16, 32] {
            let _theme = crate::theme::scoped_tokens(Tokens { radius_xl: radius, ..Tokens::dark() });
            for k in 0..8 {
                let (outer, inner) = boundaries(k);
                assert_eq!(outer.len(), inner.len());
                assert!(outer.iter().chain(&inner).all(|p| p.is_finite()));
                let a = -FRAC_PI_2 + k as f32 * FRAC_PI_4;
                assert!(contains(k, unit(a) * 104.0));
                assert!(!contains(k, unit(a + FRAC_PI_4 / 2.0) * 104.0));
            }
        }
    }
}
