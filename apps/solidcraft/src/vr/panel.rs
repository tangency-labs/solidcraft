//! The Browser's window on the belt (docs/vr.md): the window's own Browser drawn by an egui
//! context of the headset's own into a picture, which the frame shows bent onto the belt's curve.
//! The ray is its pointer, Select its button, the stick its wheel; it is drawn again when it is
//! given input, when egui asks for it, and a few times a second for what changes elsewhere.

use eframe::egui_wgpu::{self, wgpu};
use solidcraft_ui_egui::SolveApp;

/// The picture's format, as the signs': colours as the screen shows them, multiplied by alpha.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Pixels a point in the window's picture, where it fits ([`drawable`]).
pub const PIXELS_PER_POINT: f32 = 2.5;

/// The most pixels the window's picture has on a side: a wider or taller window is drawn with
/// fewer pixels a point ([`drawable`]).
pub const MAX_PIXELS: f32 = 2048.0;

/// The fewest pixels the window's picture has on a side.
const MIN_PIXELS: f32 = 16.0;

/// How the window is drawn: its size in points (what egui lays out, and what the ray's place on
/// it is measured in), its picture's size in pixels, and how many pixels a point is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawable {
    pub points: egui::Vec2,
    pub pixels: [u32; 2],
    pub pixels_per_point: f32,
}

/// How a window `size` points big is drawn: at [`PIXELS_PER_POINT`] where that fits in
/// [`MAX_PIXELS`] a side, else at fewer pixels a point, so the picture keeps the window's shape
/// and egui lays out the same rows in it. A size that is not a number, or none, is drawn as the
/// smallest picture.
pub fn drawable(size: egui::Vec2) -> Drawable {
    let side = |v: f32| if v.is_finite() { v.clamp(1.0, 1e6) } else { 1.0 };
    let size = egui::vec2(side(size.x), side(size.y));
    let ppp = PIXELS_PER_POINT.min(MAX_PIXELS / size.x).min(MAX_PIXELS / size.y);
    let px = |v: f32| (v * ppp).round().clamp(MIN_PIXELS, MAX_PIXELS);
    let pixels = [px(size.x), px(size.y)];
    Drawable { points: egui::vec2(pixels[0] / ppp, pixels[1] / ppp), pixels: pixels.map(|p| p as u32), pixels_per_point: ppp }
}

/// How often (seconds) the window is drawn again with nothing given to it: visibility and the
/// selection change from the desktop, the menu or the other hand too.
const IDLE: f64 = 0.25;

/// How far (points) the ray may wander from where Select went down before the pointer follows
/// it (a drag across the eyes, a row dragged).
const STEADY: f32 = 18.0;

/// The window between frames.
pub struct Panel {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    /// The picture, its size in pixels, and how many pictures were made before it (a new one is
    /// bound anew).
    picture: Option<(wgpu::TextureView, [u32; 2])>,
    pub generation: u32,
    /// The app's fonts last copied, by their fingerprint ([`super::sign::fonts_fingerprint`]).
    fonts_for: Option<u64>,
    /// The theme it was last drawn in ([`super::sign::ThemeWatch`]): another one draws it again.
    theme_for: Option<u64>,
    /// The input not yet given to egui.
    events: Vec<egui::Event>,
    /// Where the pointer is, and where Select went down.
    pointer: Pointer,
    /// When it was drawn last (the frame's clock), and how soon egui asked to be drawn again.
    drawn_at: Option<f64>,
    again_after: f64,
    /// What its pointer lights in the design.
    lit: Vec<String>,
}

impl Panel {
    /// A window for the window's device.
    pub fn new(device: &wgpu::Device) -> Panel {
        Panel {
            ctx: egui::Context::default(),
            renderer: egui_wgpu::Renderer::new(device, FORMAT, Default::default()),
            picture: None,
            generation: 0,
            fonts_for: None,
            theme_for: None,
            events: Vec::new(),
            pointer: Pointer::default(),
            drawn_at: None,
            again_after: 0.0,
            lit: Vec::new(),
        }
    }

    /// The ray is at `at` (across and down the window, 0 to 1) of a window `size` points big, or
    /// off it (`None`).
    pub fn point(&mut self, at: Option<[f64; 2]>, size: egui::Vec2) {
        self.events.extend(self.pointer.point(at, size));
    }

    /// Select pressed (`true`) or let go on the window.
    pub fn press(&mut self, pressed: bool) {
        self.events.extend(self.pointer.press(pressed));
    }

    /// The stick turned the wheel by `delta` points (more than nothing: the rows move down,
    /// showing those above).
    pub fn scroll(&mut self, delta: f32) {
        if delta != 0.0 && self.pointer.at.is_some() {
            self.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    /// Draw the window's picture ([`Panel::picture`]) as `size` says ([`drawable`]), again where
    /// it needs to be at `now` (the frame's clock), in the fonts and style the window (`app_ctx`)
    /// has: `fonts` is its fonts' fingerprint and `theme` the theme's generation
    /// ([`super::sign::ThemeWatch`]); either changed draws it again at once.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        rs: &egui_wgpu::RenderState,
        app_ctx: &egui::Context,
        fonts: u64,
        theme: u64,
        app: &mut SolveApp,
        size: Drawable,
        now: f64,
    ) {
        let px = size.pixels;
        let new_picture = self.picture.as_ref().is_none_or(|(_, s)| *s != px);
        let due = self.drawn_at.is_none_or(|at| now - at >= IDLE.min(self.again_after) || now < at);
        let restyled = self.fonts_for != Some(fonts) || self.theme_for != Some(theme);
        if !new_picture && !due && !restyled && self.events.is_empty() {
            return;
        }
        if new_picture {
            let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_vr_panel"),
                size: wgpu::Extent3d { width: px[0], height: px[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.picture = Some((texture.create_view(&wgpu::TextureViewDescriptor::default()), px));
            self.generation = self.generation.wrapping_add(1);
        }
        if self.fonts_for != Some(fonts) {
            self.ctx.set_fonts(app_ctx.fonts(|f| f.definitions().clone()));
            self.fonts_for = Some(fonts);
        }
        self.theme_for = Some(theme);
        self.ctx.set_global_style(app_ctx.global_style());
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.points);
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(now),
            predicted_dt: self.drawn_at.map_or(1.0 / 30.0, |at| (now - at).clamp(0.0, 0.25) as f32),
            events: std::mem::take(&mut self.events),
            focused: true,
            ..Default::default()
        };
        // The picture's pixels a point, from this pass on (a zoom factor would wait for the next).
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(size.pixels_per_point);
        let lit = &mut self.lit;
        let mut out = self.ctx.run_ui(input, |ui| {
            let t = solidcraft_theme::Tokens::get();
            let rect = ui.max_rect();
            // The window's frame, as the Browser's panel on the desktop.
            let stroke = egui::Stroke::new(1.0, t.dialog_border);
            ui.painter().rect(rect.shrink(0.5), f32::from(t.radius_xl), t.panel, stroke, egui::StrokeKind::Inside);
            let title = rect.with_max_y(rect.top() + t.control_height + t.space_8);
            ui.painter().text(
                egui::pos2(title.left() + t.space_12, title.center().y),
                egui::Align2::LEFT_CENTER,
                solidcraft_ui_text::tr("vr.belt_browser"),
                egui::FontId::proportional(t.font_label),
                t.form_ink,
            );
            ui.painter().hline(rect.x_range().shrink(t.space_8), title.bottom(), egui::Stroke::new(1.0, t.dialog_rule));
            let body = rect.with_min_y(title.bottom() + t.space_4).shrink2(egui::vec2(t.space_4, t.space_4));
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
            inner.set_clip_rect(body);
            solidcraft_ui_egui::vr_browser::show(app, &mut inner, lit);
        });
        self.again_after = out.viewport_output.get(&egui::ViewportId::ROOT).map_or(IDLE, |v| v.repaint_delay.as_secs_f64());
        self.drawn_at = Some(now);
        let primitives = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        for (id, images) in &out.textures_delta.set {
            for image in images {
                self.renderer.update_texture(&rs.device, &rs.queue, *id, image);
            }
        }
        let Some((view, _)) = self.picture.as_ref() else { return };
        let screen_desc = egui_wgpu::ScreenDescriptor { size_in_pixels: px, pixels_per_point: out.pixels_per_point };
        let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_vr_panel") });
        let uploads = self.renderer.update_buffers(&rs.device, &rs.queue, &mut encoder, &primitives, &screen_desc);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_vr_panel"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &primitives, &screen_desc);
        }
        rs.queue.submit(uploads.into_iter().chain([encoder.finish()]));
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        // Handled: egui checks nothing is left unapplied.
        out.textures_delta.clear();
    }

    /// The window's picture as last drawn ([`Panel::paint`]); none before it first is.
    pub fn picture(&self) -> Option<&wgpu::TextureView> {
        self.picture.as_ref().map(|(view, _)| view)
    }

    /// The window closed: Select held on it lets go, the pointer leaves it and what it lit goes
    /// out.
    pub fn close(&mut self, app: &mut SolveApp) {
        self.events.extend(self.pointer.gone());
        solidcraft_ui_egui::vr_pick::light(app, &mut self.lit, Vec::new());
    }
}

/// The window's pointer: where it is, and where Select went down. While Select is held the
/// pointer stays where it went down until the ray goes further than [`STEADY`], so the shake of
/// pulling the trigger is a click, not a drag; past that it follows the ray (a drag).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pointer {
    /// Where the pointer is (points), while a ray is on the window.
    at: Option<egui::Pos2>,
    /// Where Select went down, while it is held (endlessly far once the ray has gone past
    /// [`STEADY`] from it).
    pressed_at: Option<egui::Pos2>,
}

impl Pointer {
    /// The ray is at `at` (across and down the window, 0 to 1) of a window `size` points big, or
    /// off it (`None`; a place that is not a number is off it too). What to tell egui, `None`
    /// when the pointer stays where it was.
    fn point(&mut self, at: Option<[f64; 2]>, size: egui::Vec2) -> Option<egui::Event> {
        let mut pos = at.map(|[u, v]| egui::pos2(u as f32 * size.x, v as f32 * size.y)).filter(|p| p.x.is_finite() && p.y.is_finite());
        if let (Some(p), Some(down)) = (pos, self.pressed_at) {
            if p.distance(down) < STEADY {
                pos = Some(down);
            } else {
                // Gone far enough: a drag, which follows the ray from now on.
                self.pressed_at = Some(egui::pos2(f32::INFINITY, f32::INFINITY));
            }
        }
        if pos == self.at {
            return None;
        }
        self.at = pos;
        Some(match pos {
            Some(pos) => egui::Event::PointerMoved(pos),
            None => egui::Event::PointerGone,
        })
    }

    /// Select pressed (`true`) or let go: what to tell egui, `None` while the ray is off the
    /// window.
    fn press(&mut self, pressed: bool) -> Option<egui::Event> {
        self.pressed_at = if pressed { self.at } else { None };
        self.at.map(|pos| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE })
    }

    /// The window closed: what to tell egui, in order. Select held on it lets go first (where
    /// the pointer is, else where it went down), so nothing in the window stays pressed; then the
    /// pointer is gone. Nothing when the pointer was off it and nothing was held.
    fn gone(&mut self) -> Vec<egui::Event> {
        let mut out = Vec::new();
        if let Some(down) = self.pressed_at.take()
            && let Some(pos) = self.at.or(Some(down).filter(|p| p.x.is_finite() && p.y.is_finite()))
        {
            out.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
        }
        if self.at.take().is_some() {
            out.push(egui::Event::PointerGone);
        }
        out
    }
}

/// What the window's button does when the hands pressing Select on it go from `before` to `now`
/// (left first): down when the first hand presses, up when the last one lets go, nothing while
/// another hand still holds it (two hands are one pointer's one button).
pub fn button_change(before: [bool; 2], now: [bool; 2]) -> Option<bool> {
    let (was, is) = (before.iter().any(|p| *p), now.iter().any(|p| *p));
    (was != is).then_some(is)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's size in points in these tests (powers of two: places on it are exact).
    const SIZE: egui::Vec2 = egui::vec2(256.0, 512.0);

    /// The pointer moved to (`x`, `y`), as egui is told.
    fn moved(x: f32, y: f32) -> Option<egui::Event> {
        Some(egui::Event::PointerMoved(egui::pos2(x, y)))
    }

    /// Select pressed or let go at (`x`, `y`), as egui is told.
    fn button(x: f32, y: f32, pressed: bool) -> Option<egui::Event> {
        Some(egui::Event::PointerButton { pos: egui::pos2(x, y), button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE })
    }

    /// Where the ray is on the window, `x` and `y` points from its top left corner.
    fn on(x: f32, y: f32) -> Option<[f64; 2]> {
        Some([f64::from(x / SIZE.x), f64::from(y / SIZE.y)])
    }

    /// The pointer follows the ray across the window, saying nothing while the ray stays put.
    #[test]
    fn the_pointer_follows_the_ray_across_the_window() {
        let mut p = Pointer::default();
        assert_eq!(p.point(Some([0.5, 0.5]), SIZE), moved(128.0, 256.0));
        assert_eq!(p.point(Some([0.5, 0.5]), SIZE), None, "nothing new");
        assert_eq!(p.point(on(10.0, 20.0), SIZE), moved(10.0, 20.0));
    }

    /// Pressed, the pointer stays where Select went down while the ray shakes within
    /// [`STEADY`], so a click lands where it was aimed, and it lets go there too.
    #[test]
    fn a_shaky_click_lands_where_select_went_down() {
        let mut p = Pointer::default();
        p.point(on(100.0, 100.0), SIZE);
        assert_eq!(p.press(true), button(100.0, 100.0, true));
        for (x, y) in [(110.0, 100.0), (100.0, 117.0), (88.0, 90.0), (112.0, 112.0)] {
            assert_eq!(p.point(on(x, y), SIZE), None, "{x} {y}: held at the press");
        }
        assert_eq!(p.press(false), button(100.0, 100.0, false));
        assert_eq!(p.point(on(105.0, 100.0), SIZE), moved(105.0, 100.0), "let go: it follows again at once");
    }

    /// Past [`STEADY`] from where Select went down, the press is a drag: the pointer follows the
    /// ray from then on, even back near where it went down, and lets go where the ray is.
    #[test]
    fn a_press_moved_past_steady_becomes_a_drag_that_follows_the_ray() {
        let mut p = Pointer::default();
        p.point(on(100.0, 100.0), SIZE);
        p.press(true);
        assert_eq!(p.point(on(119.0, 100.0), SIZE), moved(119.0, 100.0), "past the steady distance");
        assert_eq!(p.point(on(101.0, 100.0), SIZE), moved(101.0, 100.0), "and back: still following");
        assert_eq!(p.point(on(140.0, 160.0), SIZE), moved(140.0, 160.0));
        assert_eq!(p.press(false), button(140.0, 160.0, false));
    }

    /// Off the window the pointer is gone (said once); a press there tells egui nothing and holds
    /// nothing, so the ray coming back is followed at once.
    #[test]
    fn a_press_off_the_window_holds_nothing() {
        let mut p = Pointer::default();
        assert_eq!(p.point(None, SIZE), None, "never on it: nothing to say");
        p.point(on(50.0, 50.0), SIZE);
        assert_eq!(p.point(None, SIZE), Some(egui::Event::PointerGone));
        assert_eq!(p.point(None, SIZE), None, "said once");
        assert_eq!(p.press(true), None);
        assert_eq!(p.point(on(52.0, 50.0), SIZE), moved(52.0, 50.0), "not held at a press that was not on it");
        // Pressed on it, off and back near the press: held there again.
        p.press(true);
        p.point(None, SIZE);
        assert_eq!(p.point(on(60.0, 50.0), SIZE), moved(52.0, 50.0), "back within the steady distance");
    }

    /// A place on the window or a window size that is not a number is off the window: egui is
    /// never told of a pointer at no place.
    #[test]
    fn a_ray_or_size_that_is_not_a_number_is_off_the_window() {
        let mut p = Pointer::default();
        p.point(on(50.0, 50.0), SIZE);
        assert_eq!(p.point(Some([f64::NAN, 0.5]), SIZE), Some(egui::Event::PointerGone));
        for (at, size) in [
            (Some([0.5, f64::INFINITY]), SIZE),
            (Some([f64::NAN; 2]), SIZE),
            (Some([0.5, 0.5]), egui::vec2(f32::NAN, 10.0)),
            (Some([0.5, 0.5]), egui::vec2(f32::INFINITY, 10.0)),
            (Some([1e300, 0.5]), SIZE),
        ] {
            assert_eq!(p.point(at, size), None, "{at:?} {size:?}: still off");
            assert_eq!(p.at, None);
        }
        assert_eq!(p.press(true), None);
    }

    /// The window as wide as the setting allows (600 points, 968 tall: 2420 pixels at
    /// [`PIXELS_PER_POINT`]) is drawn at fewer pixels a point, its picture within
    /// [`MAX_PIXELS`] and of the window's shape, and egui lays out as many points as the window
    /// has: a ray at the middle of the board is the pointer at the middle of what egui drew. A
    /// window that fits keeps [`PIXELS_PER_POINT`].
    #[test]
    fn the_widest_browser_window_keeps_its_shape_and_its_pointer() {
        let ratio = crate::vr::belt::WINDOW_RATIO as f32;
        for width in (200..=600).step_by(10).map(|w| w as f32) {
            let size = egui::vec2(width, width / ratio);
            let d = drawable(size);
            assert!(d.pixels.iter().all(|p| (16..=2048).contains(p)), "{width}: {:?}", d.pixels);
            let ppp = d.pixels_per_point;
            assert!(ppp > 0.0 && ppp <= PIXELS_PER_POINT, "{width}: {ppp}");
            // What egui lays out is the picture at its pixels a point, within a pixel of the
            // window asked for.
            assert!((d.points.x * ppp - d.pixels[0] as f32).abs() < 1e-3 && (d.points.y * ppp - d.pixels[1] as f32).abs() < 1e-3, "{width}");
            assert!((d.points - size).abs().max_elem() <= 1.0 / ppp, "{width}: {:?} for {size:?}", d.points);
            // The picture has the window's shape (the board is drawn from it), within a pixel.
            let picture = d.pixels[0] as f32 / d.pixels[1] as f32;
            assert!((picture - ratio).abs() < 2.0 / d.pixels[1] as f32, "{width}: {picture}");
            // The middle of the board is the middle of the laid-out window.
            let mut p = Pointer::default();
            assert_eq!(p.point(Some([0.5, 0.5]), d.points), moved(d.points.x * 0.5, d.points.y * 0.5), "{width}");
        }
        let widest = drawable(egui::vec2(600.0, 600.0 / ratio));
        assert_eq!(widest.pixels[1], 2048, "as tall as a picture may be");
        assert!(widest.pixels_per_point < PIXELS_PER_POINT);
        let usual = drawable(egui::vec2(300.0, 300.0 / ratio));
        assert_eq!(usual.pixels_per_point, PIXELS_PER_POINT, "a window that fits is drawn as ever");
        assert_eq!(usual.pixels, [750, 1210]);
    }

    /// A window size that is not a number, none, or a huge one still makes a picture the GPU
    /// can make, at a pixels-a-point that is a number.
    #[test]
    fn a_window_size_that_is_not_one_makes_a_picture_still() {
        for size in
            [egui::vec2(f32::NAN, 10.0), egui::vec2(0.0, 0.0), egui::vec2(-5.0, 300.0), egui::vec2(f32::INFINITY, 1.0), egui::vec2(1e30, 1e30)]
        {
            let d = drawable(size);
            assert!(d.pixels.iter().all(|p| (16..=2048).contains(p)), "{size:?}: {:?}", d.pixels);
            assert!(d.pixels_per_point.is_finite() && d.pixels_per_point > 0.0, "{size:?}");
            assert!(d.points.x.is_finite() && d.points.y.is_finite(), "{size:?}");
        }
    }

    /// The window closing while Select is held on it lets go first, then the pointer goes, so
    /// nothing in it stays pressed; Select let go later says nothing more. Closing with nothing
    /// held only takes the pointer away, and closing again says nothing.
    #[test]
    fn closing_the_window_mid_press_lets_go_before_the_pointer_goes() {
        let mut p = Pointer::default();
        p.point(on(100.0, 100.0), SIZE);
        p.press(true);
        assert_eq!(p.gone(), [button(100.0, 100.0, false).unwrap(), egui::Event::PointerGone]);
        assert_eq!(p.press(false), None, "let go once only");
        assert!(p.gone().is_empty(), "said once");
        // Dragged past the steady distance: it lets go where the pointer is.
        p.point(on(50.0, 50.0), SIZE);
        p.press(true);
        p.point(on(150.0, 50.0), SIZE);
        assert_eq!(p.gone(), [button(150.0, 50.0, false).unwrap(), egui::Event::PointerGone]);
        // The ray went off it while held: it lets go where Select went down; the pointer was
        // already said to be gone.
        p.point(on(60.0, 60.0), SIZE);
        p.press(true);
        p.point(None, SIZE);
        assert_eq!(p.gone(), [button(60.0, 60.0, false).unwrap()]);
        // Nothing held.
        p.point(on(10.0, 10.0), SIZE);
        assert_eq!(p.gone(), [egui::Event::PointerGone]);
    }

    /// Two hands pressing Select on the window are one button: down when the first presses, up
    /// only when the last lets go, nothing when one hand hands over to the other.
    #[test]
    fn two_hands_on_the_window_press_one_button() {
        let (f, t) = (false, true);
        assert_eq!(button_change([f, f], [t, f]), Some(true));
        assert_eq!(button_change([t, f], [t, t]), None, "the second hand adds no press");
        assert_eq!(button_change([t, t], [f, t]), None, "one still holds it");
        assert_eq!(button_change([f, t], [f, f]), Some(false), "the last lets go");
        assert_eq!(button_change([t, f], [f, t]), None, "handed over in one frame");
        assert_eq!(button_change([f, f], [t, t]), Some(true), "both at once: one press");
        assert_eq!(button_change([t, t], [f, f]), Some(false));
        assert_eq!(button_change([f, f], [f, f]), None);
    }
}
