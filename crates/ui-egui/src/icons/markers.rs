//! Constraint, assembly snap, hole placement and dimension marker artwork.
use crate::dialogs_assembly::SnapKind;
use egui::{Color32, Pos2, Stroke, vec2};
use solidcraft_theme::Tokens;

pub(crate) fn snap(painter: &egui::Painter, c: Pos2, tip: Option<Pos2>, kind: SnapKind, col: Color32, tag: Option<&str>) {
    // A dark halo under every stroke keeps the glyph readable on light and dark faces alike.
    let ic = Tokens::get().icons;
    let halo = Stroke::new(4.5, ic.marker_halo);
    let white = Stroke::new(1.5, ic.highlight);
    if let Some(tip) = tip {
        let d = (tip - c).normalized();
        let n = vec2(-d.y, d.x);
        let head = vec![tip + d * 2.0, tip - d * 9.0 + n * 5.0, tip - d * 9.0 - n * 5.0];
        painter.line_segment([c, tip], halo);
        painter.add(egui::Shape::convex_polygon(head.clone(), Color32::TRANSPARENT, halo));
        painter.line_segment([c, tip], Stroke::new(2.0, col));
        painter.add(egui::Shape::convex_polygon(head, col, Stroke::NONE));
    }
    let shape = |fill: Color32, stroke: Stroke, grow: f32| -> egui::Shape {
        match kind {
            SnapKind::FaceCenter => egui::Shape::rect_filled(egui::Rect::from_center_size(c, vec2(12.0 + grow, 12.0 + grow)), 1.5, fill),
            SnapKind::CircleCenter => egui::Shape::circle_filled(c, 7.0 + grow / 2.0, fill),
            SnapKind::EdgeMid => {
                let r = 8.0 + grow / 2.0;
                egui::Shape::convex_polygon(vec![c + vec2(0.0, -r), c + vec2(r * 0.93, r * 0.7), c + vec2(-r * 0.93, r * 0.7)], fill, stroke)
            }
            SnapKind::Vertex => {
                let r = 7.5 + grow / 2.0;
                egui::Shape::convex_polygon(vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)], fill, stroke)
            }
        }
    };
    painter.add(shape(ic.marker_halo, Stroke::NONE, 4.0));
    painter.add(shape(col, Stroke::NONE, 0.0));
    match kind {
        SnapKind::FaceCenter => {
            painter.rect_stroke(egui::Rect::from_center_size(c, vec2(12.0, 12.0)), 1.5, white, egui::StrokeKind::Middle);
        }
        SnapKind::CircleCenter => {
            painter.circle_stroke(c, 7.0, white);
            painter.line_segment([c - vec2(4.5, 0.0), c + vec2(4.5, 0.0)], Stroke::new(1.2, ic.highlight));
            painter.line_segment([c - vec2(0.0, 4.5), c + vec2(0.0, 4.5)], Stroke::new(1.2, ic.highlight));
        }
        SnapKind::EdgeMid | SnapKind::Vertex => {
            painter.add(shape(Color32::TRANSPARENT, white, 0.0));
        }
    }
    // What it snaps to (hovered), or which component it is (picked): a small label.
    if let Some(tag) = tag {
        let t = Tokens::get();
        let font = egui::FontId::proportional(solidcraft_theme::Tokens::get().font_caption);
        let g = painter.layout_no_wrap(tag.to_string(), font, t.text);
        let at = c + vec2(12.0, 10.0);
        let r = egui::Rect::from_min_size(at, g.size()).expand2(vec2(5.0, 2.0));
        painter.rect(r, 3.0, t.panel.gamma_multiply(0.92), Stroke::new(1.0, col), egui::StrokeKind::Inside);
        painter.galley(at, g, t.text);
    }
}
/// A place a hole can snap to, before the cursor reaches it: a small hollow mark of its kind
/// (a square for a face's centre, a ring for a round edge's centre).
pub(crate) fn snap_hint(painter: &egui::Painter, c: Pos2, kind: SnapKind, col: Color32) {
    let ic = Tokens::get().icons;
    for stroke in [Stroke::new(3.5, ic.marker_halo), Stroke::new(1.4, col)] {
        match kind {
            SnapKind::FaceCenter => {
                painter.rect_stroke(egui::Rect::from_center_size(c, vec2(8.0, 8.0)), 1.0, stroke, egui::StrokeKind::Middle);
            }
            SnapKind::CircleCenter => {
                painter.circle_stroke(c, 4.5, stroke);
            }
            SnapKind::EdgeMid | SnapKind::Vertex => {
                let r = 5.0;
                let diamond = vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)];
                painter.add(egui::Shape::convex_polygon(diamond, Color32::TRANSPARENT, stroke));
            }
        }
    }
    painter.circle_filled(c, 1.3, col);
}

pub(crate) fn glyph_icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Coincident" => "c_coincident",
        "Horizontal" => "c_horizontal",
        "Vertical" => "c_vertical",
        "Parallel" => "c_parallel",
        "Perpendicular" => "c_perpendicular",
        "Collinear" => "c_collinear",
        "Tangent" => "c_tangent",
        "Smooth" => "c_smooth",
        "Equal" => "c_equal",
        "Concentric" => "c_concentric",
        "MidPoint" | "PointAlong" => "c_midpoint",
        "Symmetry" => "c_symmetry",
        "Fix" => "c_fix",
        "Pierce" => "c_pierce",
        _ => return None,
    })
}

/// Constraint icon id for an automatic snap / inference label when one exists.
pub(crate) fn snap_icon(label: &str) -> Option<&'static str> {
    Some(match label {
        "//" => "c_parallel",
        "\u{22a5}" => "c_perpendicular",
        "T" => "c_tangent",
        "Mid" => "c_midpoint",
        "H" => "c_horizontal",
        "V" => "c_vertical",
        "H+V" => "c_hv",
        "On" | "End" | "Pt" => "c_coincident",
        _ => return None,
    })
}
/// A dimension arrow with a rim of `rim` around it, so it reads on a bright face (#17575).
pub(crate) fn dimension_arrow_rimmed(painter: &egui::Painter, t: Pos2, back: Pos2, col: Color32, rim: Color32) {
    let d = (t - back).normalized();
    let n = vec2(-d.y, d.x);
    let tri = [t, t - d * 9.0 + n * 3.0, t - d * 9.0 - n * 3.0];
    painter.add(egui::Shape::convex_polygon(tri.to_vec(), rim, Stroke::NONE));
    // The rim is the triangle grown by 1.25 px with ROUND corners (#17575): a stroked polygon would
    // join the three corners sharply.
    for i in 0..3 {
        painter.add(egui::Shape::line_segment([tri[i], tri[(i + 1) % 3]], Stroke::new(2.5, rim)));
        painter.circle_filled(tri[i], 1.25, rim);
    }
    dimension_arrow(painter, t, back, col);
}
/// A rim of width `width` under the polyline `pts`, with round ends and joins (#17575).
pub(crate) fn rounded_rim_line(painter: &egui::Painter, pts: &[Pos2], width: f32, rim: Color32) {
    painter.add(egui::Shape::line(pts.to_vec(), Stroke::new(width, rim)));
    for p in pts {
        painter.circle_filled(*p, width / 2.0, rim);
    }
}
pub(crate) fn dimension_arrow(painter: &egui::Painter, t: Pos2, back: Pos2, col: Color32) {
    let d = (t - back).normalized();
    let n = vec2(-d.y, d.x);
    painter.add(egui::Shape::convex_polygon(vec![t, t - d * 9.0 + n * 3.0, t - d * 9.0 - n * 3.0], col, Stroke::NONE));
}
