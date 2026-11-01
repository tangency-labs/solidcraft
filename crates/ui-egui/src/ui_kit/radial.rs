//! Analytic rounded annular sectors, following the board's offset-and-round construction.
use crate::{context_menu::Item, theme::Tokens};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

pub(crate) const RADIAL_RADIUS: f32 = 150.0;
/// Outer radius of the second layer of sectors, around the first.
pub(crate) const RADIAL_OUTER: f32 = 250.0;
const INNER: f32 = 56.0;
/// The compact menu (icons only): radius of its hub, of its first layer and of its second.
const COMPACT_INNER: f32 = 30.0;
const COMPACT_RADIUS: f32 = 90.0;
const COMPACT_OUTER: f32 = 150.0;
const GAP: f32 = 4.0;
const STEPS: usize = 16;

fn unit(a: f32) -> Vec2 {
    vec2(a.cos(), a.sin())
}

/// Inner and outer radius of a layer's sectors: 0 is the first layer, 1 the one around it; the
/// compact menu's are smaller.
const fn radii(layer: usize, compact: bool) -> (f32, f32) {
    match (layer, compact) {
        (0, false) => (INNER, RADIAL_RADIUS),
        (_, false) => (RADIAL_RADIUS + GAP, RADIAL_OUTER),
        (0, true) => (COMPACT_INNER, COMPACT_RADIUS),
        (_, true) => (COMPACT_RADIUS + GAP, COMPACT_OUTER),
    }
}

/// The menu's measures, full or compact, for placing it and telling its layers apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RadialLayout {
    pub compact: bool,
}

impl RadialLayout {
    /// Outer radius of the first layer: where the second one begins.
    pub(crate) const fn edge(self) -> f32 {
        radii(0, self.compact).1
    }

    /// How far the menu reaches from its centre: past the second layer when it has commands.
    pub(crate) fn reach(self, two_layers: bool) -> f32 {
        radii(usize::from(two_layers), self.compact).1
    }

    /// The (layer, sector) drawn at offset `d` from the centre of a picture `scale` times the
    /// menu's real size: none in the hub, the gaps or outside.
    pub(crate) fn slot_at(self, d: Vec2, scale: f32) -> Option<(usize, usize)> {
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        let d = d / scale;
        (0..2).flat_map(|layer| (0..8).map(move |k| (layer, k))).find(|(layer, k)| contains(*k, *layer, self.compact, d))
    }
}

/// Two equally sampled boundaries; their ends are joined by parallel straight sides.
/// The four joins are circular arcs of the theme radius, tangent to both adjacent edges.
fn boundaries(k: usize, layer: usize, compact: bool) -> (Vec<Pos2>, Vec<Pos2>) {
    let (inner_r, outer_r) = radii(layer, compact);
    // The outer and inner circles are functional geometry; only the corner joins vary.
    // Bound the radius so the inner arc remains ordered, even for hostile theme data.
    let corner = f32::from(Tokens::get().radius_xl.saturating_sub(1)).min(12.0);
    let am = -FRAC_PI_2 + (k % 8) as f32 * FRAC_PI_4;
    let a0 = am - FRAC_PI_4 / 2.0;
    let a1 = am + FRAC_PI_4 / 2.0;
    let d = GAP / 2.0 + corner;
    let ro = outer_r - corner;
    let ri = inner_r + corner;
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
    arc(&mut outer, Pos2::ZERO, outer_r, outer0, outer1);
    arc(&mut outer, po1, corner, outer1, a1 + FRAC_PI_2);
    let mut inner = Vec::new();
    arc(&mut inner, pi0, corner, a0 - FRAC_PI_2, inner0 - PI);
    arc(&mut inner, Pos2::ZERO, inner_r, inner0, inner1);
    arc(&mut inner, pi1, corner, inner1 - PI, a1 + FRAC_PI_2 - TAU);
    (outer, inner)
}

fn contains(k: usize, layer: usize, compact: bool, at: Vec2) -> bool {
    let (inner_r, outer_r) = radii(layer, compact);
    if !at.is_finite() || !(inner_r..=outer_r).contains(&at.length()) {
        return false;
    }
    let (outer, inner) = boundaries(k, layer, compact);
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

/// Paints sector `k` of `layer` centred at `center`, `scale` times its real size.
#[allow(clippy::too_many_arguments)]
fn paint_sector(p: &egui::Painter, center: Pos2, k: usize, layer: usize, compact: bool, scale: f32, fill: Color32, stroke: Stroke) {
    let (outer, inner) = boundaries(k, layer, compact);
    let at = |q: &Pos2| center + q.to_vec2() * scale;
    let mut mesh = egui::Mesh::default();
    for (o, i) in outer.iter().zip(&inner) {
        mesh.colored_vertex(at(o), fill);
        mesh.colored_vertex(at(i), fill);
    }
    for j in 0..outer.len().saturating_sub(1) {
        let a = (j * 2) as u32;
        mesh.add_triangle(a, a + 2, a + 1);
        mesh.add_triangle(a + 1, a + 2, a + 3);
    }
    p.add(egui::Shape::mesh(mesh));
    let outline = outer.iter().chain(inner.iter().rev()).map(at).collect();
    p.add(egui::Shape::closed_line(outline, stroke));
}

/// The radial menu with one layer of sectors.
#[cfg(test)]
pub(crate) fn radial_menu(ui: &mut Ui, center: Pos2, items: &[Item], hot: Option<usize>) -> Option<Item> {
    radial_menu_layers(ui, center, items, &[], hot.map(|k| (0, k)), false)
}

/// The radial menu: `inner` in the first layer of sectors, clockwise from the top, and `outer`
/// in a second layer around it; `hot` is the (layer, sector) under the pointer. The full menu
/// writes each command's name under its icon and the lit one's in the hub; the `compact` one
/// shows icons only, and the lit command's name above the menu.
pub(crate) fn radial_menu_layers(
    ui: &mut Ui,
    center: Pos2,
    inner: &[Item],
    outer: &[Item],
    hot: Option<(usize, usize)>,
    compact: bool,
) -> Option<Item> {
    let t = Tokens::get();
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let mut picked = None;
    let layers = [inner, outer];
    let all = layers.iter().enumerate().flat_map(|(layer, items)| items.iter().take(8).enumerate().map(move |(k, item)| (layer, k, item)));
    for (layer, k, item) in all {
        let enabled = item.enabled && ui.is_enabled();
        let lit = enabled && hot == Some((layer, k));
        let mut painter = ui.painter().clone();
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        paint_sector(
            &painter,
            center,
            k,
            layer,
            compact,
            1.0,
            if lit { accent.soft } else { t.dialog_body },
            Stroke::new(if lit { 1.6 } else { 1.0 }, if lit { accent.color } else { t.dialog_border }),
        );
        let a = -FRAC_PI_2 + k as f32 * FRAC_PI_4;
        let (inner_r, outer_r) = radii(layer, compact);
        let at = center + unit(a) * (inner_r + outer_r) / 2.0;
        let target = if compact {
            Rect::from_center_size(at, vec2(t.space_24 + t.space_12, t.space_24 + t.space_12))
        } else {
            Rect::from_center_size(at, vec2(t.space_24 * 2.0 + t.space_12, t.space_24 + t.space_16 + t.space_2))
        };
        let response = ui.interact(target, ui.id().with(("radial", layer, k)), Sense::hover());
        crate::scenario::publish_target(&format!("radial:{}", item.id), target);
        let ink = if lit { accent.ink } else { t.form_ink };
        let icon_at = if compact { at } else { at - vec2(0.0, t.space_6) };
        crate::icons::paint_accented(
            &painter,
            Rect::from_center_size(icon_at, vec2(t.space_24, t.space_24)),
            &item.icon,
            crate::theme::TabAccent::current_icon_accent(ui.ctx()),
            1.0,
        );
        let label = crate::i18n::tr_label(&item.label);
        if !compact {
            let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), FontId::proportional(t.font_small), ink);
            job.wrap.max_width = 74.0;
            job.wrap.max_rows = 1;
            let galley = painter.layout_job(job);
            painter.galley(at + vec2(-galley.size().x / 2.0, t.space_10), galley, ink);
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        if lit && ui.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| contains(k, layer, compact, p - center))) {
            picked = Some(item.clone());
        }
    }
    let p = ui.painter();
    let hub = radii(0, compact).0;
    p.circle(center, hub - t.space_6, t.dialog_body, Stroke::new(1.0, t.dialog_border));
    let lit = hot.and_then(|(layer, k)| layers.get(layer).and_then(|items| items.get(k)));
    if compact {
        // No room in the hub: the name goes on a label above the menu.
        if let Some(item) = lit {
            let text = match crate::i18n::tr_label(&item.shortcut) {
                "" => crate::i18n::tr_label(&item.label).to_string(),
                key => format!("{}  {key}", crate::i18n::tr_label(&item.label)),
            };
            let galley = p.layout_no_wrap(text, FontId::proportional(t.font_label), t.form_ink);
            let reach = RadialLayout { compact: true }.reach(!outer.is_empty()) + t.space_8;
            let size = galley.size() + vec2(t.space_16, t.space_8);
            let above = Rect::from_center_size(egui::pos2(center.x, center.y - reach - size.y / 2.0), size);
            // Below the menu when there is no room above it.
            let rect = if above.top() < ui.ctx().content_rect().top() { above.translate(vec2(0.0, 2.0 * reach + size.y)) } else { above };
            p.rect(rect, t.radius_lg, t.dialog_body, Stroke::new(1.0, t.dialog_border), egui::StrokeKind::Inside);
            p.galley(rect.center() - galley.size() / 2.0, galley, t.form_ink);
        }
        return picked;
    }
    if let Some(item) = lit {
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

/// A picture of the radial menu, `scale` times its real size, for the editor of its commands:
/// the sectors of both layers with their icons, the `marked` one lit. With `slots` the free
/// places of both layers are drawn too, each with a plus, so they can be picked.
#[allow(clippy::too_many_arguments)]
pub(crate) fn radial_preview(
    p: &egui::Painter,
    center: Pos2,
    scale: f32,
    inner: &[Item],
    outer: &[Item],
    compact: bool,
    marked: Option<(usize, usize)>,
    slots: bool,
) {
    let t = Tokens::get();
    for (layer, items) in [inner, outer].into_iter().enumerate() {
        let shown = if slots { 8 } else { items.len().min(8) };
        for k in 0..shown {
            let item = items.get(k);
            let lit = marked == Some((layer, k));
            let (fill, stroke) = match (lit, item.is_some()) {
                (true, _) => (t.accent_soft, Stroke::new(1.6, t.accent)),
                (false, true) => (t.dialog_body, Stroke::new(1.0, t.dialog_border)),
                (false, false) => (t.dialog_body.gamma_multiply(0.35), Stroke::new(1.0, t.dialog_border.gamma_multiply(0.6))),
            };
            paint_sector(p, center, k, layer, compact, scale, fill, stroke);
            let (inner_r, outer_r) = radii(layer, compact);
            let at = center + unit(-FRAC_PI_2 + k as f32 * FRAC_PI_4) * (inner_r + outer_r) / 2.0 * scale;
            match item {
                Some(item) => {
                    let side = (t.space_24 * scale).max(14.0);
                    crate::icons::paint_icon(p, Rect::from_center_size(at, vec2(side, side)), &item.icon, 1.0);
                }
                None => {
                    p.text(at, Align2::CENTER_CENTER, "+", FontId::proportional(t.font_label), t.form_dim);
                }
            }
        }
    }
    p.circle(center, (radii(0, compact).0 - t.space_6) * scale, t.dialog_body, Stroke::new(1.0, t.dialog_border));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_clicks_ignore_the_disc_gaps_outside_and_nonfinite_positions() {
        for k in 0..8 {
            let a = -FRAC_PI_2 + k as f32 * FRAC_PI_4;
            assert!(contains(k, 0, false, unit(a) * 104.0));
            for at in [Vec2::ZERO, unit(a) * 200.0, vec2(f32::NAN, 0.0), vec2(f32::INFINITY, 0.0), unit(a + FRAC_PI_4 / 2.0) * 104.0] {
                assert!(!contains(k, 0, false, at), "sector {k}: {at:?}");
            }
            // The second layer lies around the first, with the same gap between them.
            assert!(contains(k, 1, false, unit(a) * 200.0));
            for at in [unit(a) * 104.0, unit(a) * (RADIAL_RADIUS + GAP / 2.0), unit(a) * 300.0, vec2(f32::NAN, 0.0)] {
                assert!(!contains(k, 1, false, at), "outer sector {k}: {at:?}");
            }
            // The compact menu: the same, smaller.
            assert!(contains(k, 0, true, unit(a) * 60.0) && contains(k, 1, true, unit(a) * 120.0));
            assert!(!contains(k, 0, true, unit(a) * 104.0) && !contains(k, 1, true, unit(a) * 200.0));
        }
    }

    #[test]
    fn neighbouring_sectors_have_parallel_four_point_gaps_and_four_round_corners() {
        for (layer, k, compact) in (0..2).flat_map(|layer| (0..8).flat_map(move |k| [(layer, k, false), (layer, k, true)])) {
            let (outer, inner) = boundaries(k, layer, compact);
            let (next_outer, next_inner) = boundaries(k + 1, layer, compact);
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
            for (layer, compact) in [(0, false), (1, false), (0, true), (1, true)] {
                for k in 0..8 {
                    let (outer, inner) = boundaries(k, layer, compact);
                    assert_eq!(outer.len(), inner.len());
                    assert!(outer.iter().chain(&inner).all(|p| p.is_finite()));
                    let a = -FRAC_PI_2 + k as f32 * FRAC_PI_4;
                    let (inner_r, outer_r) = radii(layer, compact);
                    let mid = (inner_r + outer_r) / 2.0;
                    assert!(contains(k, layer, compact, unit(a) * mid), "radius {radius}, layer {layer}, compact {compact}");
                    assert!(!contains(k, layer, compact, unit(a + FRAC_PI_4 / 2.0) * mid));
                }
            }
        }
    }

    #[test]
    fn a_picture_of_the_menu_finds_the_sector_under_a_point() {
        for compact in [false, true] {
            let layout = RadialLayout { compact };
            for (layer, k) in [(0, 0), (0, 3), (1, 2), (1, 7)] {
                let (inner_r, outer_r) = radii(layer, compact);
                let d = unit(-FRAC_PI_2 + k as f32 * FRAC_PI_4) * (inner_r + outer_r) / 2.0;
                assert_eq!(layout.slot_at(d * 0.5, 0.5), Some((layer, k)), "compact {compact}");
            }
            assert_eq!(layout.slot_at(Vec2::ZERO, 0.5), None, "the hub");
            assert_eq!(layout.slot_at(vec2(0.0, -1000.0), 0.5), None, "outside");
            assert_eq!(layout.slot_at(vec2(0.0, -50.0), 0.0), None, "no picture");
            assert_eq!(layout.slot_at(vec2(f32::NAN, 0.0), 1.0), None);
        }
    }

    #[test]
    fn the_compact_menu_is_smaller_and_its_layers_meet_at_its_edge() {
        assert!(
            RadialLayout { compact: true }.reach(true) < RadialLayout { compact: false }.reach(true) * 0.65,
            "two compact layers: well under two thirds the size"
        );
        assert_eq!(RadialLayout { compact: true }.reach(false), RadialLayout { compact: true }.edge());
        assert_eq!(RadialLayout { compact: false }.reach(false), RADIAL_RADIUS);
        assert_eq!(RadialLayout { compact: false }.reach(true), RADIAL_OUTER);
        for compact in [false, true] {
            assert_eq!(radii(1, compact).0, RadialLayout { compact }.edge() + GAP);
        }
    }
}
