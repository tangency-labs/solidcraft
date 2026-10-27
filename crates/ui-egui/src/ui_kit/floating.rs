//! Shared tear-off host for tool panels and nonmodal manager windows.
#[cfg(any(not(target_arch = "wasm32"), test))]
use egui::Rect;
use egui::{Context, Id, InnerResponse, Pos2, Ui, Vec2};

/// Native title-bar drags are handled by the OS and need not send pointer events to egui.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default)]
pub(crate) struct NativeDockDrag {
    outer: Option<egui::Rect>,
    parent: Option<egui::Rect>,
    last_motion: f64,
    pub(crate) moving: bool,
    armed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeDockDrag {
    pub(crate) fn update(&mut self, outer: egui::Rect, inner: egui::Rect, parent: egui::Rect, now: f64) -> bool {
        if ![outer, inner, parent].iter().all(|r| r.is_finite() && r.is_positive()) || !now.is_finite() {
            *self = Self::default();
            return false;
        }
        let previous = self.outer.replace(outer);
        let old_parent = self.parent.replace(parent);
        let Some(previous) = previous else {
            self.last_motion = now;
            return false;
        };
        let translated = (outer.min - previous.min).length() > 0.5;
        let resized = (outer.size() - previous.size()).length() > 1.0;
        if resized || (!translated && old_parent != Some(parent)) {
            self.moving = false;
            self.armed = false;
            self.last_motion = now;
            return false;
        }
        // Require a useful part of the title bar over the app, rather than a single overlapping pixel.
        let target = parent.shrink(crate::theme::Tokens::get().space_16);
        let title_y = outer.top() + (inner.top() - outer.top()).max(super::header_height()) * 0.5;
        let overlap = (outer.right().min(target.right()) - outer.left().max(target.left())).max(0.0);
        let over_app = target.y_range().contains(title_y) && overlap >= outer.width().min(160.0);
        if translated {
            // Ignore creation/placement jitter. A new drag after settling, or a trip outside
            // the parent, arms docking; detaching must never immediately snap back.
            self.armed |= !over_app || now - self.last_motion >= 0.4;
            self.moving = true;
            self.last_motion = now;
        }
        if self.moving && (!over_app || !self.armed) && now - self.last_motion >= 0.4 {
            self.moving = false;
        }
        self.moving && self.armed && over_app && now - self.last_motion >= 0.18
    }
}

/// Keep the original content-space grab point while handing a card to a native frame.
/// The parent retains the mouse capture; starting a second OS drag after focus changes
/// can use a different event/anchor and teleport the new window.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub(crate) struct TearOff {
    inner_origin: Pos2,
    grab: Vec2,
    tracking: bool,
    shown: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl TearOff {
    pub(crate) fn new(inner_origin: Pos2, grab: Vec2, tracking: bool) -> Self {
        Self { inner_origin, grab, tracking, shown: false }
    }

    pub(crate) fn prepare(&self, builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
        if self.shown { builder } else { builder.with_active(false).with_visible(false) }
    }

    pub(crate) fn placement(&mut self, pointer: Option<Pos2>, parent_origin: Pos2, down: bool, frame_offset: Vec2) -> (Pos2, bool, bool) {
        if self.tracking
            && let Some(pointer) = pointer.filter(|p| p.is_finite())
        {
            self.inner_origin = parent_origin + pointer.to_vec2() - self.grab;
        }
        let first = !self.shown;
        self.shown = true;
        (self.inner_origin - frame_offset, first, !self.tracking || !down)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn follow_pointer(ctx: &Context, handoff: &mut Option<TearOff>, outer: Option<Rect>, inner: Option<Rect>) -> Option<Pos2> {
    let drag = handoff.as_mut()?;
    let (origin, pointer, down) = ctx.input_for(egui::ViewportId::ROOT, |i| {
        (i.viewport().inner_rect.map_or(Pos2::ZERO, |r| r.min), i.pointer.interact_pos(), i.pointer.primary_down())
    });
    let offset = outer.zip(inner).map_or(Vec2::ZERO, |(o, n)| n.min - o.min);
    let (position, show, finished) = drag.placement(pointer, origin, down, offset);
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
    if show {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }
    if finished {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        *handoff = None;
        end_handoff(ctx);
    }
    ctx.request_repaint_of(egui::ViewportId::ROOT);
    Some(position)
}

/// Enlarge the usable part of native resize borders inside the content area. macOS
/// does not implement winit's BeginResize, so resize from screen-space pointer deltas.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy)]
struct NativeResize {
    inner: Rect,
    outer: Pos2,
    pointer: Pos2,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn native_resize(ui: &mut Ui, minimum: Vec2) {
    let ctx = ui.ctx().clone();
    let (inner, outer, fullscreen) = ctx
        .input(|i| (i.viewport().inner_rect, i.viewport().outer_rect, i.viewport().fullscreen == Some(true) || i.viewport().maximized == Some(true)));
    let Some((inner, outer)) = inner.zip(outer).filter(|(i, o)| i.is_finite() && o.is_finite() && i.is_positive() && o.is_positive()) else {
        return;
    };
    if fullscreen {
        return;
    }
    let rect = ctx.content_rect();
    let corner = crate::theme::Tokens::get().space_12;
    let side = crate::theme::Tokens::get().space_6;
    let clip = ui.clip_rect();
    ui.set_clip_rect(rect);
    for (name, x, y) in super::panel::RESIZE_DIRECTIONS {
        let hit = super::panel::resize_hit(rect, x, y, corner, side, false);
        let id = Id::new("sc_native_resize").with(ctx.viewport_id()).with(name);
        let response = ui.interact(hit, id, egui::Sense::drag());
        super::panel::resize_cursor(ui, hit, &response, super::panel::resize_icon(x, y));
        crate::scenario::publish_rect(&format!("window:native_resize:{name}"), hit);
        crate::scenario::publish_handle(&format!("window:native_resize:{name}"), hit.center());
        if response.drag_started()
            && let Some(pointer) = ctx.input(|i| i.pointer.press_origin())
        {
            ctx.data_mut(|d| d.insert_temp(id, NativeResize { inner, outer: outer.min, pointer: inner.min + pointer.to_vec2() }));
        }
        if (response.dragged() || response.drag_stopped())
            && ctx.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. })))
            && let Some(start) = ctx.data(|d| d.get_temp::<NativeResize>(id))
            && let Some(pointer) = ctx.input(|i| i.pointer.interact_pos())
        {
            let delta = inner.min + pointer.to_vec2() - start.pointer;
            let mut next = start.inner;
            if x < 0 {
                next.min.x = (start.inner.left() + delta.x).min(start.inner.right() - minimum.x);
            }
            if x > 0 {
                next.max.x = (start.inner.right() + delta.x).max(start.inner.left() + minimum.x);
            }
            if y < 0 {
                next.min.y = (start.inner.top() + delta.y).min(start.inner.bottom() - minimum.y);
            }
            if y > 0 {
                next.max.y = (start.inner.bottom() + delta.y).max(start.inner.top() + minimum.y);
            }
            if next.is_finite() && next.width() <= 10000.0 && next.height() <= 10000.0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(next.size()));
                if x < 0 || y < 0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(start.outer + (next.min - start.inner.min)));
                }
            }
        }
        if response.drag_stopped() || !ctx.input(|i| i.pointer.primary_down()) {
            ctx.data_mut(|d| d.remove::<NativeResize>(id));
        }
    }
    ui.set_clip_rect(clip);
}

/// Horizontal/bottom pulls need a useful part of the panel outside; the top handoff
/// must still occur while its title pointer is inside the native window.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn crosses_border(parent: Rect, desired: Rect, pointer: Pos2, grab: Vec2) -> bool {
    if !parent.is_finite() || !desired.is_finite() || !pointer.is_finite() {
        return false;
    }
    let horizontal = (desired.width() * 0.18).clamp(48.0, 96.0);
    let bottom = (desired.height() * 0.18).clamp(48.0, 96.0);
    (desired.left() < parent.left() - horizontal && pointer.x < parent.left() + 24.0)
        || (desired.right() > parent.right() + horizontal && pointer.x > parent.right() - 24.0)
        || (desired.bottom() > parent.bottom() + bottom && pointer.y > parent.bottom() - 24.0)
        || (desired.top() < parent.top() && pointer.y < parent.top() + 8.0 && grab.y > 0.0)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn mark_handoff(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new("sc_native_handoff"), true));
}
pub(crate) fn end_handoff(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new("sc_native_handoff"), false));
}
pub(crate) fn handoff_dragging(ctx: &Context) -> bool {
    if !ctx.input(|i| i.pointer.primary_down()) {
        end_handoff(ctx);
        return false;
    }
    ctx.data(|d| d.get_temp::<bool>(Id::new("sc_native_handoff"))).unwrap_or(false)
}

#[derive(Clone)]
pub(super) struct Title {
    pub key: String,
    pub docked: Pos2,
}

pub(super) fn register(ctx: &Context, id: Id, key: &str, docked: Pos2) {
    ctx.data_mut(|d| d.insert_temp(id.with("floating_title"), Title { key: key.into(), docked }));
}

pub(super) fn close_handled(ctx: &Context) {
    if let Some(id) = ctx.data(|d| d.get_temp::<Id>(Id::new("sc_native_content_host"))) {
        ctx.data_mut(|d| d.insert_temp(id.with("floating_close_handled"), true));
    }
}

pub(super) fn native_content(ctx: &Context, id: Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(id.with("floating_native_content"))).unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct Placement {
    detached: bool,
    place: Option<[f32; 4]>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default)]
struct Runtime {
    native: bool,
    last: Option<Rect>,
    grab: Option<Vec2>,
    pressed_size: Option<Vec2>,
    handoff: Option<TearOff>,
    docking: NativeDockDrag,
    frame: u64,
}

/// The callback is shared by the embedded and native host, so drafts keep their owner.
pub(super) fn show<R>(
    ctx: &Context,
    id: Id,
    title: &str,
    window: egui::Window<'_>,
    mut add: impl FnMut(&mut Ui) -> R,
) -> Option<InnerResponse<Option<R>>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (id, title);
        window.show(ctx, |ui| add(ui))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut placement = ctx.data_mut(|d| d.get_persisted::<Placement>(id.with("floating_placement"))).unwrap_or_default();
        let mut runtime = ctx.data(|d| d.get_temp::<Runtime>(id.with("floating_runtime"))).unwrap_or_default();
        let frame = ctx.cumulative_frame_nr_for(egui::ViewportId::ROOT);
        if runtime.frame > 0 && frame > runtime.frame.saturating_add(1) {
            runtime.native = false;
        }
        runtime.frame = frame;
        let parent = ctx.input_for(egui::ViewportId::ROOT, |i| i.viewport().inner_rect);
        let shown = if placement.detached {
            let opening = !runtime.native;
            runtime.native = true;
            let minimum = [260.0, 160.0];
            let mut builder = egui::ViewportBuilder::default().with_title(title).with_resizable(true).with_min_inner_size(minimum);
            if opening {
                runtime.docking = NativeDockDrag::default();
                if let Some([x, y, w, h]) = placement.place.filter(|p| {
                    p.iter().all(|v| v.is_finite())
                        && p[0].abs() < 100000.0
                        && p[1].abs() < 100000.0
                        && (260.0..=10000.0).contains(&p[2])
                        && (160.0..=10000.0).contains(&p[3])
                }) {
                    builder = builder.with_position([x, y]).with_inner_size([w, h]);
                } else {
                    builder = builder.with_inner_size(runtime.last.map_or(Vec2::new(600.0, 500.0), |r| r.size()));
                }
            }
            if let Some(drag) = &runtime.handoff {
                builder = drag.prepare(builder);
            }
            let response = ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id.with("floating_native")), builder, |ui, class| {
                let child = ui.ctx().clone();
                child.data_mut(|d| {
                    d.insert_temp(id.with("floating_native_content"), true);
                    d.insert_temp(Id::new("sc_native_content_host"), id);
                    d.insert_temp(id.with("floating_close_handled"), false);
                });
                let response = super::Container::new(&child, super::Surface::Viewport)
                    .central()
                    .frame(super::Container::new(&child, super::Surface::Panel).edge_frame(crate::theme::Tokens::get().dialog_body))
                    .show(ui, |ui| add(ui));
                child.data_mut(|d| {
                    d.insert_temp(id.with("floating_native_content"), false);
                    d.remove::<Id>(Id::new("sc_native_content_host"));
                });
                if class != egui::ViewportClass::EmbeddedWindow {
                    let (outer, inner, closing) = child.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested()));
                    if runtime.handoff.is_none() {
                        native_resize(ui, minimum.into());
                    }
                    let following = follow_pointer(&child, &mut runtime.handoff, outer, inner);
                    if opening && following.is_none() {
                        child.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    if let (Some(outer), Some(inner)) = (outer, inner) {
                        let position = following.unwrap_or(outer.min);
                        placement.place = Some([position.x, position.y, inner.width(), inner.height()]);
                        if !closing
                            && following.is_none()
                            && let Some(parent) = parent
                        {
                            if runtime.docking.update(outer, inner, parent, child.input(|i| i.time)) {
                                placement.detached = false;
                                runtime.native = false;
                                runtime.handoff = None;
                                end_handoff(&child);
                                child.data_mut(|d| {
                                    d.insert_persisted(id.with("size"), inner.size());
                                    d.insert_temp(id.with("floating_dock_position"), outer.min - parent.min.to_vec2());
                                });
                                child.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                                child.request_repaint_of(egui::ViewportId::ROOT);
                            } else if runtime.docking.moving {
                                child.request_repaint_after(std::time::Duration::from_millis(50));
                            }
                        }
                    }
                    if closing {
                        if !child.data(|d| d.get_temp::<bool>(id.with("floating_close_handled"))).unwrap_or(false) {
                            placement.detached = false;
                            runtime.native = false;
                        }
                        child.request_repaint_of(egui::ViewportId::ROOT);
                    }
                }
                let mut response = response;
                if class != egui::ViewportClass::EmbeddedWindow {
                    response.response.rect = Rect::NOTHING;
                }
                response
            });
            Some(InnerResponse { inner: Some(response.inner), response: response.response })
        } else {
            runtime.native = false;
            if ctx.input(|i| i.pointer.primary_pressed()) {
                runtime.pressed_size = runtime.last.map(|r| r.size());
                runtime.grab = ctx.input(|i| i.pointer.press_origin()).zip(runtime.last).filter(|(p, r)| r.contains(*p)).map(|(p, r)| p - r.min);
            }
            let shown = window.show(ctx, |ui| add(ui));
            if let Some(shown) = &shown {
                let rect = shown.response.rect;
                runtime.last = Some(rect);
                let info = ctx.data(|d| d.get_temp::<Title>(id.with("floating_title")));
                if let Some(info) = info
                    && crate::frame::title_dragging(ctx, &info.key)
                    && let Some(pointer) = ctx.input(|i| i.pointer.latest_pos())
                {
                    let grab = *runtime.grab.get_or_insert_with(|| ctx.input(|i| i.pointer.press_origin()).unwrap_or(pointer) - rect.min);
                    let desired = Rect::from_min_size(pointer - grab, rect.size());
                    if crosses_border(ctx.content_rect(), desired, pointer, grab) {
                        let origin = parent.map_or(Pos2::ZERO, |r| r.min);
                        let pos = origin + desired.min.to_vec2();
                        placement.detached = true;
                        placement.place = Some([pos.x, pos.y, rect.width(), rect.height()]);
                        runtime.handoff = Some(TearOff::new(pos, grab, ctx.input(|i| i.pointer.primary_down())));
                        mark_handoff(ctx);
                        ctx.request_repaint();
                    }
                }
            }
            if !ctx.input(|i| i.pointer.primary_down()) {
                runtime.grab = None;
                runtime.pressed_size = None;
            }
            shown
        };
        ctx.data_mut(|d| {
            d.insert_persisted(id.with("floating_placement"), placement);
            d.insert_temp(id.with("floating_runtime"), runtime);
        });
        shown
    }
}

pub(super) fn drag_size(ctx: &Context, id: Id) -> Option<Vec2> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let title = ctx.data(|d| d.get_temp::<Title>(id.with("floating_title")))?;
        if !crate::frame::title_dragging(ctx, &title.key) {
            return None;
        }
        ctx.data(|d| d.get_temp::<Runtime>(id.with("floating_runtime"))).and_then(|s| s.pressed_size)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, id);
        None
    }
}

#[cfg(test)]
pub(crate) fn detached(ctx: &Context, id: Id) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ctx.data_mut(|d| d.get_persisted::<Placement>(id.with("floating_placement"))).is_some_and(|p| p.detached)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, id);
        false
    }
}
