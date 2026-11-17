//! Native routing for view-tab drags. The strips report client coordinates and the
//! source translates its captured pointer to desktop pixels before hit testing.

use egui::{Align2, Context, Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_views::{DesignView, ViewKind, WindowId};

use crate::{SolveApp, app_bar::ViewDropZone};

#[derive(Default)]
pub(crate) struct State {
    strips: Vec<Strip>,
    hover: Option<Hover>,
}

impl State {
    fn set_hover(&mut self, ctx: &Context, hover: Option<Hover>) {
        if self.hover != hover {
            for target in [self.hover.as_ref(), hover.as_ref()].into_iter().flatten() {
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

#[derive(PartialEq)]
struct Hover {
    source: egui::ViewportId,
    target: egui::ViewportId,
    tab: u64,
    pointer: Pos2,
    name: String,
    kind: ViewKind,
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
    if !drag_continues && state.hover.as_ref().is_some_and(|hover| hover.source == viewport) {
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
        if let Some(hover) = state.hover.as_ref().filter(|hover| hover.target == viewport && hover.tab == tab)
            && let Some((_, mark)) = strip.drop_at(hover.pointer)
        {
            let rect = strip.zone.rect;
            let t = Tokens::get();
            let painter = painter(ctx);
            painter.line_segment([pos2(mark, rect.top() + 3.0), pos2(mark, rect.bottom() - 3.0)], Stroke::new(2.0, t.accent));
            let tab = held_rect(ctx.viewport_rect(), pos2(mark, rect.bottom()), vec2(144.0, 28.0), vec2(72.0, -3.0));
            painter.rect_filled(tab, 6, t.app_bar_tab_active);
            painter.rect_stroke(tab, 6, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            paint_header(&painter, tab, &hover.name, hover.kind.key(), t.app_bar_text);
        }
        state.strips.push(strip);
    }
}

fn painter(ctx: &Context) -> egui::Painter {
    ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("view_window_drag")))
}

pub(crate) fn route(app: &mut SolveApp, ctx: &Context, view: u64, pointer: Pos2, released: bool) -> bool {
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
    let view_info = app.docs.views.get(view);
    let last_main_view = app.view_windows.current_window() == 0 && app.docs.views.len() == 1;
    let hover = if released || last_main_view {
        None
    } else {
        target.zip(desktop).zip(view_info).map(|(((target, _, _), pointer), view)| Hover {
            source: viewport,
            target,
            tab,
            pointer,
            name: view.name.clone(),
            kind: view.kind,
        })
    };
    app.view_window_drag.set_hover(ctx, hover);
    if let Some((_, window, index)) = target {
        if released {
            move_view(app, view, Some((window, index)), desktop);
        } else if last_main_view && let Some(view) = view_info {
            ctx.set_cursor_icon(egui::CursorIcon::NotAllowed);
            paint_preview(ctx, view, pointer, Preview::Held(Some(app.tr("multi_window.last_view"))));
        }
        return true;
    }
    if outside {
        if released {
            move_view(app, view, None, desktop);
        } else if let Some(view) = view_info {
            let refusal = if last_main_view {
                Some(app.tr("multi_window.last_view"))
            } else if !crate::view_windows::can_create(app) {
                Some(app.tr("multi_window.too_many"))
            } else {
                None
            };
            let target = if let Some(reason) = refusal {
                ctx.set_cursor_icon(egui::CursorIcon::NotAllowed);
                Preview::Held(Some(reason))
            } else {
                let rect = crate::view_windows::drag_window_outline(app, view.id, desktop)
                    .zip(Screen::current(ctx))
                    .map(|(rect, screen)| Rect::from_min_max(screen.local(rect.min), screen.local(rect.max)));
                Preview::Window { rect, label: app.tr("multi_window.drop_new") }
            };
            paint_preview(ctx, view, pointer, target);
        }
    }
    outside
}

fn move_view(app: &mut SolveApp, view: u64, target: Option<(WindowId, usize)>, screen: Option<Pos2>) {
    if let Err(reason) = crate::view_windows::drag_drop(app, view, target, screen) {
        crate::design_view_ui::show_move_error(app, reason);
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Preview<'a> {
    Held(Option<&'a str>),
    Tile { rect: Rect, neighbors: &'a [Rect], label: &'a str, focused: bool },
    Window { rect: Option<Rect>, label: &'a str },
}

/// The drag stays represented by the same view header. Its body expands into the prospective
/// cell or turns into a miniature of the independently framed window.
pub(crate) fn paint_preview(ctx: &Context, view: &DesignView, pointer: Pos2, preview: Preview<'_>) {
    let client = ctx.viewport_rect();
    if !client.is_positive() || !client.is_finite() {
        return;
    }
    let t = Tokens::get();
    let painter = painter(ctx).with_clip_rect(client);
    let held = held_rect(client, pointer, vec2(166.0, 30.0), vec2(24.0, 14.0));
    let (goal, label, window) = match preview {
        Preview::Held(label) => (if label.is_some() { held_rect(client, pointer, vec2(230.0, 70.0), vec2(24.0, 14.0)) } else { held }, label, false),
        Preview::Tile { rect, neighbors, label, .. } => {
            for neighbor in neighbors {
                painter.rect_stroke(*neighbor, 8, Stroke::new(1.0, t.accent.gamma_multiply(0.45)), StrokeKind::Inside);
            }
            painter.rect_stroke(rect, 8, Stroke::new(2.0, t.accent), StrokeKind::Inside);
            (rect, Some(label), false)
        }
        Preview::Window { rect, label } => {
            if let Some(rect) = rect.filter(|rect| rect.intersect(client).is_positive()) {
                painter.extend(egui::Shape::dashed_line(
                    &[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()],
                    Stroke::new(2.0, t.accent),
                    6.0,
                    4.0,
                ));
            }
            let ratio = rect.map_or(1.55, |rect| (rect.width() / rect.height()).clamp(1.15, 2.5));
            let size = vec2(230.0, (230.0 / ratio).clamp(100.0, 184.0));
            (held_rect(client, pointer, size, vec2(24.0, 14.0)), Some(label), true)
        }
    };
    let rect = morph(ctx, view.id, held, goal);
    let header = Rect::from_min_max(rect.min, pos2(rect.right(), (rect.top() + crate::view_tiles::HEADER).min(rect.bottom())));
    let tiled = matches!(preview, Preview::Tile { .. });
    let fill = if tiled { t.accent_soft.gamma_multiply(0.58) } else { t.dialog_body };
    painter.rect_filled(rect, 8, fill);
    painter.rect_stroke(rect, 8, Stroke::new(if window { 2.0 } else { 1.0 }, t.accent), StrokeKind::Inside);
    let header_bg = if window { t.app_bar } else { t.dialog_body };
    painter.rect_filled(header.shrink(1.0), 7, header_bg);
    let focused = matches!(preview, Preview::Tile { focused: true, .. });
    let ink = if window { t.app_bar_text } else { t.text };
    if focused {
        let dot = header.right_center() - vec2(13.0, 0.0);
        painter.circle_filled(dot, 3.5, t.accent);
        paint_header(&painter, Rect::from_min_max(header.min, pos2(dot.x - 6.0, header.bottom())), &view.name, view.kind.key(), ink);
    } else {
        paint_header(&painter, header, &view.name, view.kind.key(), ink);
    }

    if let Some(label) = label.filter(|_| rect.height() >= header.height() + 24.0) {
        let body = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max).shrink(7.0);
        if body.is_positive() {
            let content = painter.with_clip_rect(body);
            let ink = if tiled || window { t.accent } else { t.text_dim };
            if window && body.height() > 44.0 {
                let icon = Rect::from_center_size(body.center() - vec2(0.0, 12.0), vec2(20.0, 20.0));
                crate::ui_kit::icons::paint_ink(&content, icon, "panel_popout", ink);
            }
            let offset = if window && body.height() > 44.0 { 13.0 } else { 0.0 };
            let center = body.center() + vec2(0.0, offset);
            let font = crate::ui_kit::type_face::semibold(t.font_button);
            if tiled {
                let galley = content.layout_no_wrap(label.to_owned(), font, t.text);
                let pill = Rect::from_center_size(center, (galley.size() + vec2(20.0, 10.0)).min(body.size()));
                content.rect_filled(pill, 6, t.dialog_body);
                content.rect_stroke(pill, 6, Stroke::new(1.0, t.border), StrokeKind::Inside);
                content.galley(center - galley.size() * 0.5, galley, t.text);
            } else {
                content.text(center, Align2::CENTER_CENTER, label, font, ink);
            }
        }
    }
}

fn paint_header(painter: &egui::Painter, rect: Rect, name: &str, icon: &str, ink: egui::Color32) {
    let t = Tokens::get();
    let painter = painter.with_clip_rect(rect.shrink(4.0));
    let start = rect.left_center() + vec2(13.0, 0.0);
    crate::app_bar::paint_view_glyph(&painter, Rect::from_center_size(start, vec2(15.0, 15.0)), icon, ink);
    painter.text(start + vec2(13.0, 0.0), Align2::LEFT_CENTER, name, crate::ui_kit::type_face::semibold(t.font_button), ink);
}

fn held_rect(client: Rect, pointer: Pos2, size: egui::Vec2, grab: egui::Vec2) -> Rect {
    let size = size.min(client.size());
    let max = client.max - size;
    let position = pointer - grab;
    Rect::from_min_size(pos2(position.x.clamp(client.left(), max.x), position.y.clamp(client.top(), max.y)), size)
}

#[derive(Clone, Copy)]
struct Morph {
    frame: u64,
    rect: Rect,
}

fn morph(ctx: &Context, view: u64, held: Rect, goal: Rect) -> Rect {
    let frame = ctx.cumulative_frame_nr();
    let id = Id::unique(("view_drag_morph", ctx.viewport_id(), view));
    let old = ctx.data(|data| data.get_temp::<Morph>(id)).filter(|old| frame.saturating_sub(old.frame) <= 1);
    let duration = ctx.global_style().animation_time.clamp(0.0, 0.11);
    let dt = ctx.input(|input| input.stable_dt);
    let fraction = if duration > 0.0 { (dt.max(0.0) * 3.0 / duration).min(1.0) } else { 1.0 };
    let previous = old.map_or(held, |old| old.rect);
    let rect = Rect::from_min_max(previous.min.lerp(goal.min, fraction), previous.max.lerp(goal.max, fraction));
    let settled = rect.min.distance(goal.min) <= 0.5 && rect.max.distance(goal.max) <= 0.5;
    let rect = if settled { goal } else { rect };
    if !settled {
        ctx.request_repaint();
    }
    ctx.data_mut(|data| data.insert_temp(id, Morph { frame, rect }));
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_stays_in_the_client_and_snaps_to_the_target_without_animation() {
        let client = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 550.0));
        let held = held_rect(client, pos2(875.0, -40.0), vec2(230.0, 140.0), vec2(24.0, 14.0));
        assert_eq!(held, Rect::from_min_max(pos2(570.0, 0.0), pos2(800.0, 140.0)));
        let tile = Rect::from_min_max(pos2(400.0, 44.0), pos2(795.0, 540.0));
        let ctx = Context::default();
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert_eq!(morph(ui.ctx(), 2, held, tile), tile);
            assert_eq!(morph(ui.ctx(), 2, held, held), held);
        });
    }

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
