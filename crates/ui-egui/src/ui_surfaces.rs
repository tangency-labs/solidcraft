//! The window's panels as drawn this frame, found in the paint lists after layout: rects filled
//! with one of the theme's surface colours. Plugin hooks that colour or replace panels use them.

use egui::epaint::{RectShape, Shape};

/// One panel's fill in the paint list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Surface {
    pub layer: egui::LayerId,
    /// The shape that holds the fill: a rect, or a list with the fill among its parts.
    pub index: egui::layers::ShapeIdx,
    pub shape: RectShape,
    /// On a dialog, popup or other floating layer rather than the window's background.
    pub floating: bool,
}

/// The theme's surface colours.
pub(crate) fn fills(t: &solidcraft_theme::Tokens) -> [egui::Color32; 8] {
    [t.panel, t.panel_header, t.toolbar, t.timeline, t.dialog_bg, t.dialog_body, t.app_bar, t.overlay]
}

/// Smaller than 24×20 or larger than 60 % of the window is skipped, as is a rect inside a surface
/// already found on its layer (a header inside its panel).
pub(crate) fn scan(ctx: &egui::Context, fills: &[egui::Color32]) -> Vec<Surface> {
    let screen = ctx.content_rect();
    let mut layers = vec![egui::LayerId::background()];
    layers.extend(ctx.memory(|m| m.layer_ids().filter(|l| *l != egui::LayerId::background()).collect::<Vec<_>>()));
    let mut found = Vec::new();
    ctx.graphics_mut(|g| {
        for layer in &layers {
            let Some(list) = g.get_mut(*layer) else { continue };
            let mut done: Vec<egui::Rect> = Vec::new();
            for i in 0..list.next_idx().0 {
                let index = egui::layers::ShapeIdx(i);
                list.mutate_shape(index, |clipped| {
                    let rect = match &clipped.shape {
                        Shape::Rect(r) => Some(r),
                        Shape::Vec(parts) => parts.iter().find_map(|p| match p {
                            Shape::Rect(r) if fills.contains(&r.fill) => Some(r),
                            _ => None,
                        }),
                        _ => None,
                    };
                    let Some(r) = rect else { return };
                    let take = fills.contains(&r.fill)
                        && r.rect.width() >= 24.0
                        && r.rect.height() >= 20.0
                        && r.rect.area() <= screen.area() * 0.6
                        && !done.iter().any(|d| d.contains_rect(r.rect));
                    if take {
                        done.push(r.rect);
                        found.push(Surface { layer: *layer, index, shape: r.clone(), floating: layer.order != egui::Order::Background });
                    }
                });
            }
        }
    });
    found
}

/// Puts `over` right above the surface's fill.
pub(crate) fn add_over(ctx: &egui::Context, surface: &Surface, fills: &[egui::Color32], over: Shape) {
    ctx.graphics_mut(|g| {
        let Some(list) = g.get_mut(surface.layer) else { return };
        list.mutate_shape(surface.index, |clipped| match &mut clipped.shape {
            Shape::Vec(parts) => {
                let at = parts.iter().position(|p| matches!(p, Shape::Rect(r) if fills.contains(&r.fill))).map_or(parts.len(), |j| j + 1);
                parts.insert(at, over);
            }
            shape => {
                let fill = std::mem::replace(shape, Shape::Noop);
                *shape = Shape::Vec(vec![fill, over]);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfaces_are_theme_fills_neither_tiny_nor_whole_window_nor_nested() {
        let ctx = egui::Context::default();
        let fill = egui::Color32::from_rgb(30, 31, 32);
        let mut found = Vec::new();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let p = ui.painter();
            let at = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
            p.rect_filled(at(10.0, 10.0, 300.0, 200.0), 6, fill);
            p.rect_filled(at(20.0, 20.0, 100.0, 30.0), 0, fill);
            p.rect_filled(at(400.0, 10.0, 10.0, 10.0), 0, fill);
            p.rect_filled(at(400.0, 100.0, 200.0, 100.0), 0, egui::Color32::RED);
            p.rect_filled(ui.ctx().content_rect(), 0, fill);
            found = scan(ui.ctx(), &[fill]);
            if let Some(first) = found.first() {
                add_over(ui.ctx(), first, &[fill], Shape::rect_filled(first.shape.rect, 0, egui::Color32::GREEN));
            }
        });
        out.textures_delta.clear();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].shape.rect, egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(300.0, 200.0)));
        assert!(!found[0].floating);
    }
}
