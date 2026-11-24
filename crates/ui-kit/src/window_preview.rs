//! Draws the small window shown at the top of the pop-out options menu.

use crate::window_options_menu::{CardHeaders, TopBar, WindowMenu};
use egui::{Color32, Mesh, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

/// The miniature window, in its own units.
const WINDOW: egui::Vec2 = egui::Vec2::new(900.0, 560.0);
/// The card's miniature cube face, in the same units.
const CARD_FACE: egui::Vec2 = egui::Vec2::splat(78.0);

pub(crate) fn show(ui: &mut Ui, menu: &WindowMenu<'_>, max_height: f32) {
    let width = (ui.available_width() - 16.0).max(0.0);
    let height = (width * 0.65).min(max_height);
    if height <= 0.0 || width <= 0.0 {
        return;
    }
    ui.horizontal(|ui| {
        ui.add_space(Tokens::get().space_8);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &menu.text.preview));
        let t = Tokens::get();
        let painter = ui.painter_at(rect);
        desktop(&painter, rect, t.accent_soft.lerp_to_gamma(t.figure_ground, 0.32), t.surface_accent.lerp_to_gamma(t.figure_ground, 0.45));
        let inset = (rect.height() * 0.055).min(12.0);
        let scale = ((rect.width() - 2.0 * inset) / WINDOW.x).min((rect.height() - 2.0 * inset) / WINDOW.y);
        let window = Rect::from_center_size(rect.center(), WINDOW * scale);
        let radius = (16.0 * scale).round() as u8;
        for (spread, opacity) in [(18.0, 0.12), (10.0, 0.20), (3.0, 0.30)] {
            painter.rect_filled(window.translate(vec2(0.0, 12.0 * scale)).expand(spread * scale), radius, t.figure_shadow.gamma_multiply(opacity));
        }
        painter.rect(window, radius, t.figure_ground, Stroke::new(3.0 * scale, t.border), StrokeKind::Inside);
        let preview = Painter::with_clip_rect(&painter, window);
        miniature(&preview, window, scale, menu, t);
    });
}

fn desktop(painter: &Painter, rect: Rect, start: Color32, end: Color32) {
    let radius = 10.0_f32.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), start.lerp_to_gamma(end, 0.5));
    for (corner, (x, y)) in [
        (rect.right() - radius, rect.bottom() - radius),
        (rect.left() + radius, rect.bottom() - radius),
        (rect.left() + radius, rect.top() + radius),
        (rect.right() - radius, rect.top() + radius),
    ]
    .into_iter()
    .enumerate()
    {
        for sample in 0..=4 {
            let angle = std::f32::consts::FRAC_PI_2 * (corner as f32 + sample as f32 / 4.0);
            let at = pos2(x + radius * angle.cos(), y + radius * angle.sin());
            let blend = ((at.x - rect.left()) / rect.width() + (at.y - rect.top()) / rect.height()) * 0.5;
            mesh.colored_vertex(at, start.lerp_to_gamma(end, blend));
        }
    }
    for edge in 1..=20 {
        mesh.add_triangle(0, edge, edge % 20 + 1);
    }
    painter.add(Shape::mesh(mesh));
}

fn miniature(painter: &Painter, window: Rect, scale: f32, menu: &WindowMenu<'_>, t: Tokens) {
    let point = |x, y| window.min + vec2(x, y) * scale;
    let region = |x, y, w, h| Rect::from_min_size(point(x, y), vec2(w, h) * scale);
    if menu.top_bar == TopBar::Always {
        let bar = region(3.0, 3.0, 894.0, 44.0);
        painter.rect_filled(bar, (12.0 * scale).round() as u8, t.app_bar);
        crate::icons::paint_ink(painter, region(18.0, 13.0, 20.0, 20.0), "ws_design", t.app_bar_text);
        painter.rect_filled(region(52.0, 10.0, 401.0, 29.0), 2, t.app_bar_hover_button);
        for (x, label, selected) in [(62.0, "ui.design", false), (162.0, "Model", true), (259.0, "Model", false)] {
            let tab = region(x, 14.0, 88.0, 21.0);
            if selected {
                painter.rect_filled(tab, 2, t.app_bar_text.gamma_multiply(0.16));
            }
            painter.text(
                tab.center(),
                egui::Align2::CENTER_CENTER,
                solidcraft_ui_text::tr(label),
                crate::type_face::semibold(t.font_body * scale),
                t.app_bar_text,
            );
        }
        crate::app_bar::paint_layout_glyph(painter, region(411.0, 15.0, 18.0, 18.0), "layout.grid", t.app_bar_text);
        for x in [830.0, 838.0, 846.0] {
            painter.circle_filled(point(x, 24.0), 2.8 * scale, t.app_bar_text);
        }
    }
    cube(painter, point(446.0, 300.0), 290.0 * scale, t);
    let card = region(633.0, 368.0, 253.0, 176.0);
    painter.rect_filled(card.translate(vec2(0.0, 8.0 * scale)), 3, t.figure_shadow);
    painter.rect(card, 3, t.figure_card, Stroke::new(2.5 * scale, t.accent.gamma_multiply(0.65)), StrokeKind::Inside);
    let header = region(636.0, 371.0, 247.0, 25.0);
    match menu.card_headers {
        CardHeaders::Always => {
            painter.rect_filled(header, 2, t.panel_header);
            painter.line_segment([header.left_bottom(), header.right_bottom()], Stroke::new(3.0 * scale, t.border));
        }
        CardHeaders::Hover => {
            painter.rect_filled(header, 2, t.panel_header.gamma_multiply(0.55));
            painter.extend(crate::dashed_line(
                &[header.left_bottom(), header.right_bottom()],
                card,
                Stroke::new(3.0 * scale, t.accent),
                14.0 * scale,
                8.0 * scale,
            ));
        }
        CardHeaders::Hidden => {}
    }
    let top = Rect::from_center_size(card.center() + vec2(0.0, 6.0 * scale), CARD_FACE * scale);
    painter.rect(top, 0, t.figure_face_top, Stroke::new(3.0 * scale, t.figure_line), StrokeKind::Inside);
    if menu.panels {
        let top = if menu.ribbon { 146.0 } else { 62.0 };
        for (x, width, height) in [(17.0, 164.0, 238.0), (732.0, 150.0, 186.0)] {
            let panel = region(x, top, width, height);
            painter.rect(panel, 3, t.figure_card, Stroke::new(3.0 * scale, t.border), StrokeKind::Inside);
            painter.rect_filled(region(x + 3.0, top + 3.0, width - 6.0, 22.0), 2, t.panel_header);
            for y in [42.0, 61.0, 80.0] {
                painter.rect_filled(region(x + 14.0, top + y, width * 0.55, 4.0), 1, t.text_dim.gamma_multiply(0.36));
            }
        }
    }
    if menu.ribbon {
        let ribbon_top = if menu.top_bar == TopBar::Always { 51.0 } else { 31.0 };
        let ribbon = region(14.0, ribbon_top, 872.0, 84.0);
        if menu.ribbon_auto_hide {
            painter.rect_filled(ribbon, 3, t.toolbar.gamma_multiply(0.86));
            painter.extend(crate::dashed_line(
                &[ribbon.left_top(), ribbon.right_top(), ribbon.right_bottom(), ribbon.left_bottom(), ribbon.left_top()],
                window,
                Stroke::new(3.0 * scale, t.accent),
                14.0 * scale,
                8.0 * scale,
            ));
        } else {
            painter.rect(ribbon, 3, t.toolbar, Stroke::new(2.0 * scale, t.border), StrokeKind::Inside);
        }
        for (index, icon) in ["sketch", "extrude", "revolve", "fillet", "chamfer", "shell", "pattern", "measure"].into_iter().enumerate() {
            let x = 38.0 + index as f32 * 105.0;
            crate::icons::paint_ink(painter, region(x, ribbon_top + 15.0, 28.0, 28.0), icon, t.text_dim);
            painter.rect_filled(region(x - 7.0, ribbon_top + 55.0, 42.0, 3.0), 1, t.text_dim.gamma_multiply(0.40));
            if index < 7 {
                painter.line_segment([point(x + 66.0, ribbon_top + 14.0), point(x + 66.0, ribbon_top + 68.0)], Stroke::new(scale, t.border));
            }
        }
    }
    if menu.top_bar == TopBar::Edge {
        painter.rect_filled(region(416.0, 7.0, 68.0, 6.0), (5.0 * scale).round() as u8, t.text_dim.gamma_multiply(0.5));
    }
}

/// The three faces of the cube in the Layout preferences schematic share the projected
/// 30-degree edges and its theme face and outline roles.
fn cube(painter: &Painter, center: Pos2, width: f32, t: Tokens) {
    let p = |x, y| center + vec2(x, y) * width;
    let [a, b, c, d, e, f, g] = [p(0.0, -0.58), p(0.5, -0.29), p(0.0, 0.0), p(-0.5, -0.29), p(-0.5, 0.29), p(0.0, 0.58), p(0.5, 0.29)];
    let line = Stroke::new(width * 0.026, t.figure_line);
    for (points, fill) in [([d, c, f, e], t.figure_face_left), ([b, g, f, c], t.figure_face_right), ([a, b, c, d], t.figure_face_top)] {
        painter.add(Shape::convex_polygon(points.to_vec(), fill, line));
    }
}
