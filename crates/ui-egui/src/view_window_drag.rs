//! Native routing for view-tab drags. The strips report client coordinates and the
//! source translates its captured pointer to desktop pixels before hit testing.

use egui::{Align2, Context, Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_views::WindowId;

use crate::{SolveApp, app_bar::ViewDropZone};

#[derive(Default)]
pub(crate) struct State {
    strips: Vec<Strip>,
    hover: Option<Hover>,
}

impl State {
    fn set_hover(&mut self, ctx: &Context, hover: Option<Hover>) {
        if self.hover != hover {
            for target in [self.hover, hover].into_iter().flatten() {
                ctx.request_repaint_of(target.target);
            }
            self.hover = hover;
        }
    }
}

struct Strip {
    viewport: egui::ViewportId,
    tab: u64,
    window: WindowId,
    screen: Screen,
    zone: ViewDropZone,
    count: usize,
}

#[derive(Clone, Copy)]
struct Screen {
    origin: Pos2,
    pixels_per_point: f32,
}

impl Screen {
    fn current(ctx: &Context) -> Option<Self> {
        let pixels_per_point = ctx.pixels_per_point();
        let inner = ctx.input(|i| i.viewport().inner_rect)?;
        (inner.is_finite() && inner.is_positive() && pixels_per_point.is_finite() && pixels_per_point > 0.0)
            .then_some(Self { origin: (inner.min.to_vec2() * pixels_per_point).to_pos2(), pixels_per_point })
    }

    fn desktop(self, local: Pos2) -> Pos2 {
        self.origin + local.to_vec2() * self.pixels_per_point
    }

    fn local(self, desktop: Pos2) -> Pos2 {
        ((desktop - self.origin) / self.pixels_per_point).to_pos2()
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Hover {
    source: egui::ViewportId,
    target: egui::ViewportId,
    tab: u64,
    pointer: Pos2,
}

impl Strip {
    fn drop_at(&self, desktop: Pos2) -> Option<(usize, f32)> {
        let pointer = self.screen.local(desktop);
        if !self.zone.rect.contains(pointer) {
            return None;
        }
        let centres = &self.zone.view_centres;
        let index = centres.iter().find(|(_, x)| pointer.x < *x).map_or(self.count, |(index, _)| *index);
        let left = centres.iter().rev().find(|(i, _)| *i < index).map_or(self.zone.rect.left(), |(_, x)| *x);
        let right = centres.iter().find(|(i, _)| *i >= index).map_or(self.zone.rect.right(), |(_, x)| *x);
        Some((index, (left + right) / 2.0))
    }
}

/// Paint the destination's insertion marker and cache this frame's visible strips.
pub(crate) fn record(app: &mut SolveApp, ctx: &Context, zones: Vec<(u64, usize, ViewDropZone)>, drag_continues: bool) {
    let viewport = ctx.viewport_id();
    let state = &mut app.view_window_drag;
    if !drag_continues && state.hover.is_some_and(|hover| hover.source == viewport) {
        state.set_hover(ctx, None);
    }
    ctx.input(|input| {
        state
            .strips
            .retain(|strip| strip.viewport != viewport && input.raw.viewports.get(&strip.viewport).is_some_and(|info| info.minimized != Some(true)));
    });
    let Some(screen) = Screen::current(ctx) else { return };
    let window = app.view_windows.current_window();
    for (tab, count, zone) in zones {
        let strip = Strip { viewport, tab, window, screen, zone, count };
        if let Some(hover) = state.hover.filter(|hover| hover.target == viewport && hover.tab == tab)
            && let Some((_, mark)) = strip.drop_at(hover.pointer)
        {
            let rect = strip.zone.rect;
            let t = Tokens::get();
            painter(ctx).line_segment([pos2(mark, rect.top() + 3.0), pos2(mark, rect.bottom() - 3.0)], Stroke::new(2.0, t.accent));
        }
        state.strips.push(strip);
    }
}

fn painter(ctx: &Context) -> egui::Painter {
    ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("view_window_drag")))
}

pub(crate) fn route(app: &mut SolveApp, ctx: &Context, view: u64, pointer: Pos2, released: bool, below: f32) -> bool {
    if !pointer.is_finite() || !crate::view_windows::supports_pop_out(app) {
        return false;
    }
    let viewport = ctx.viewport_id();
    let client = ctx.viewport_rect();
    if !client.is_finite() || !client.is_positive() {
        return false;
    }
    let outside = !client.contains(pointer);
    let tab = app.docs.current_tab();
    let desktop = Screen::current(ctx).map(|screen| screen.desktop(pointer));
    // A pop-out can cover part of the main canvas when the main window is maximized.
    // A drag in a child keeps that child as the destination until the pointer leaves it.
    let target = desktop.filter(|_| outside || viewport == egui::ViewportId::ROOT).and_then(|point| {
        app.view_window_drag.strips.iter().rev().find_map(|strip| {
            if strip.tab == tab && strip.viewport != viewport {
                strip.drop_at(point).map(|(index, _)| (strip.viewport, strip.window, index))
            } else {
                None
            }
        })
    });
    let hover = if released { None } else { target.zip(desktop).map(|((target, _, _), pointer)| Hover { source: viewport, target, tab, pointer }) };
    app.view_window_drag.set_hover(ctx, hover);
    if let Some((_, window, index)) = target {
        if released {
            move_view(app, view, Some((window, index)), desktop);
        }
        return true;
    }
    if outside && released {
        move_view(app, view, None, desktop);
    } else if !released
        && (outside || pointer.y > below)
        && let Some(view) = app.docs.views.get(view)
    {
        paint_ghost(ctx, &view.name, view.kind.key(), client, pointer, outside, app.tr("multi_window.drop_new"));
    }
    outside
}

fn move_view(app: &mut SolveApp, view: u64, target: Option<(WindowId, usize)>, screen: Option<Pos2>) {
    if let Err(reason) = crate::view_windows::drag_drop(app, view, target, screen) {
        crate::design_view_ui::show_move_error(app, reason);
    }
}

fn paint_ghost(ctx: &Context, name: &str, kind: &str, client: Rect, pointer: Pos2, outside: bool, release: &str) {
    let t = Tokens::get();
    let painter = painter(ctx);
    let size = vec2(230.0, 150.0);
    let position = pointer - vec2(24.0, 14.0);
    let position = if outside {
        let max = client.min.max(client.max - size);
        pos2(position.x.clamp(client.left(), max.x), position.y.clamp(client.top(), max.y))
    } else {
        position
    };
    let card = Rect::from_min_size(position, size);
    let angle = -2.0_f32.to_radians();
    let rotation = egui::emath::Rot2::from_angle(angle);
    let rotate = |p: Pos2| card.center() + rotation * (p - card.center());
    let shadow = egui::Shadow { offset: [0, 14], blur: 32, ..t.shell_shadow() };
    painter.add(shadow.as_shape(card, 8).with_angle_and_pivot(angle, card.center()));
    let body = egui::epaint::RectShape::new(card, 8, t.dialog_body, Stroke::new(1.0, t.border), StrokeKind::Inside);
    painter.add(body.with_angle_and_pivot(angle, card.center()));
    let header_bottom = card.top() + 28.0;
    painter
        .line_segment([rotate(pos2(card.left() + 1.0, header_bottom)), rotate(pos2(card.right() - 1.0, header_bottom))], Stroke::new(1.0, t.border));
    let galley = painter.layout_no_wrap(name.to_owned(), crate::ui_kit::type_face::semibold(t.font_button), t.text);
    let name_at = rotate(pos2(card.left() + 12.0, card.top() + (28.0 - galley.size().y) * 0.5));
    painter.with_clip_rect(card.shrink(5.0)).add(egui::epaint::TextShape::new(name_at, galley, t.text).with_angle(angle));
    if outside {
        let y = header_bottom + 42.0;
        crate::ui_kit::icons::paint_ink(&painter, Rect::from_center_size(pos2(card.center().x, y), vec2(22.0, 22.0)), "panel_popout", t.accent);
        let inset = card.shrink(10.0);
        let rect = Rect::from_min_max(pos2(inset.left(), header_bottom + 8.0), inset.right_bottom());
        painter.extend(egui::Shape::dashed_line(
            &[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()],
            Stroke::new(2.0, t.accent),
            6.0,
            4.0,
        ));
        painter.text(pos2(rect.center().x, y + 25.0), Align2::CENTER_CENTER, release, crate::ui_kit::type_face::semibold(t.font_button), t.accent);
    } else {
        let icon = Rect::from_center_size(pos2(card.center().x, header_bottom + 58.0), vec2(22.0, 22.0));
        crate::app_bar::paint_view_glyph(&painter, icon, kind, t.text_dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_resolves_to_the_target_strips_local_slot_at_its_own_scale() {
        let screen = Screen { origin: pos2(-1000.0, 200.0), pixels_per_point: 1.5 };
        let strip = Strip {
            viewport: egui::ViewportId::ROOT,
            tab: 1,
            window: 0,
            screen,
            zone: ViewDropZone {
                document: 0,
                rect: Rect::from_min_max(pos2(100.0, 0.0), pos2(300.0, 30.0)),
                view_centres: vec![(0, 140.0), (1, 180.0), (2, 250.0)],
            },
            count: 3,
        };
        assert_eq!(strip.drop_at(screen.desktop(pos2(120.0, 20.0))), Some((0, 120.0)));
        assert_eq!(strip.drop_at(screen.desktop(pos2(210.0, 20.0))), Some((2, 215.0)));
        assert_eq!(strip.drop_at(screen.desktop(pos2(270.0, 20.0))), Some((3, 275.0)));
        assert_eq!(strip.drop_at(screen.desktop(pos2(220.0, 35.0))), None);
    }
}
