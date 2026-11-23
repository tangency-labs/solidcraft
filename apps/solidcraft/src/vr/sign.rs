//! The signs in the room (what a ray points at, what a hand did, the stick's labels, the menu's
//! items): the words painted by egui, in the app's fonts and the colours given, into a picture of
//! their own, which the frame then shows as a small board in the room.

use eframe::egui_wgpu::{self, wgpu};

/// A sign's picture in pixels: wide enough for a long part name at [`FONT_SIZE`], tall enough
/// for [`SIGN_LINES`] lines (a short sign uses the middle).
pub const SIGN_PX: [u32; 2] = [1024, 256];

/// How many lines a sign's words may take before they are cut short.
const SIGN_LINES: usize = 3;

/// How many times one line's height a sign's picture is: a board said to be `h` tall is drawn
/// this many times `h`, so its one line keeps the size asked for.
pub const SIGN_LINE_SHARE: f64 = 2.0;

/// A menu slot's picture in pixels: the desktop radial menu's slot, scaled up.
pub const BUBBLE_PX: [u32; 2] = [320, 224];

/// What a picture shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// A board with words on one line.
    Sign,
    /// A menu item, drawn as a slot of SolidCraft's radial menu: the dialog's body and border, its
    /// icon in colour, its words small beneath (the icon alone where it has none, the words alone
    /// where there is no icon); `lit` (marked, open
    /// or current) in the tab's accent; one that cannot be taken (or a ring passed) at `fade`
    /// percent opacity (100: not faded).
    Bubble { icon: Option<&'static str>, lit: bool, fade: u8 },
    /// The box around a group of slots (the belt): the dialog's body and border, `ratio` times as
    /// wide as tall (in tenths, so it can be compared and kept).
    Panel { ratio_tenths: u32 },
    /// The bar under the belt that says where to take hold of it; `lit` while a hand is on it.
    Handle { lit: bool },
    /// A picture drawn elsewhere (the Browser's window), `size` pixels.
    Picture { size: [u32; 2] },
    /// A joint's glyph, as the window draws it.
    Joint { badge: solidcraft_ui_egui::vr_pick::JointBadge },
}

impl Look {
    /// The picture's size in pixels.
    pub fn size(self) -> [u32; 2] {
        match self {
            Look::Sign => SIGN_PX,
            Look::Bubble { .. } => BUBBLE_PX,
            Look::Panel { ratio_tenths } => [(ratio_tenths.clamp(10, 200) * 128 / 10).min(2048), 128],
            Look::Handle { .. } => [256, 48],
            Look::Picture { size } => size,
            Look::Joint { .. } => [128, 128],
        }
    }
}

/// The picture's format: colours as the screen shows them, multiplied by alpha (what egui draws
/// into a format that is not sRGB). The frame's shader turns them into linear light.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The name's size in the picture (pixels).
const FONT_SIZE: f32 = 64.0;

/// A sign's words longer than this many characters are cut short with an ellipsis (they also
/// wrap, and are cut at the end of the last line that fits).
const MAX_CHARS: usize = 160;

/// Paints signs on the window's device, with an egui context of its own (the window's is in the
/// middle of its frame when the headset's runs).
pub struct Painter {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    /// The app's fonts last copied, by their fingerprint ([`fonts_fingerprint`]): another
    /// language or interface font brings other faces.
    fonts_for: Option<u64>,
}

/// A fingerprint of the window's fonts (`definitions`): which faces each family takes, in order,
/// and which font data each name holds (by identity and length, not by reading every byte). The
/// same definitions give the same number; another face, family order or font file another one.
/// Cheap enough for every frame: the fonts are compared, never copied, until they change.
pub fn fonts_fingerprint(definitions: &egui::FontDefinitions) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (name, data) in &definitions.font_data {
        name.hash(&mut h);
        // A new font (or its tweak) is a new entry. The copy a painter keeps holds the old one,
        // so no new font can take its place in memory while it is compared with.
        (std::sync::Arc::as_ptr(data) as usize).hash(&mut h);
        AsRef::<[u8]>::as_ref(&*data.font).len().hash(&mut h);
        data.index.hash(&mut h);
    }
    for (family, faces) in &definitions.families {
        family.hash(&mut h);
        faces.hash(&mut h);
    }
    h.finish()
}

/// Tells when the theme's colours change: a generation that goes up each time the theme's
/// tokens are not the ones seen last (light or dark, a custom theme, the colour vision mode, a
/// tab's palette). Signs and the Browser's window painted in one generation are painted again in
/// the next.
#[derive(Default)]
pub struct ThemeWatch {
    last: Option<solidcraft_theme::Tokens>,
    generation: u64,
}

impl ThemeWatch {
    /// The generation of the theme whose tokens are `now`.
    pub fn generation(&mut self, now: solidcraft_theme::Tokens) -> u64 {
        if self.last.as_ref() != Some(&now) {
            if self.last.is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            self.last = Some(now);
        }
        self.generation
    }
}

impl Painter {
    /// A painter for the window's device.
    pub fn new(device: &wgpu::Device) -> Painter {
        Painter { ctx: egui::Context::default(), renderer: egui_wgpu::Renderer::new(device, FORMAT, Default::default()), fonts_for: None }
    }

    /// `text` painted as `look` says in `ink` on `fill`, in the fonts `app` (the window's context)
    /// has now, `fonts` their fingerprint ([`fonts_fingerprint`]). The picture is [`Look::size`],
    /// transparent around the board.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        rs: &egui_wgpu::RenderState,
        app: &egui::Context,
        fonts: u64,
        text: &str,
        fill: egui::Color32,
        ink: egui::Color32,
        look: Look,
    ) -> wgpu::TextureView {
        let [w, h] = look.size();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32));
        let input = || egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        if self.fonts_for != Some(fonts) {
            // New fonts take effect from the next pass: one empty pass first, its font picture
            // uploaded like any other.
            self.ctx.set_fonts(app.fonts(|f| f.definitions().clone()));
            let mut out = self.ctx.run_ui(input(), |_| {});
            self.upload(rs, &out.textures_delta);
            self.free(&out.textures_delta);
            out.textures_delta.clear();
            self.fonts_for = Some(fonts);
        }
        let mut out = self.ctx.run_ui(input(), |ui| {
            let painter = ui.painter();
            match look {
                Look::Sign => {
                    // Wrapped to the picture's width, at most SIGN_LINES lines, the board around
                    // the words in the picture's middle.
                    let pad = egui::vec2(FONT_SIZE * 0.45, FONT_SIZE * 0.2);
                    let galley = painter.layout_job(wrapped(&short(text, MAX_CHARS), FONT_SIZE, ink, w as f32 - pad.x * 2.0, SIGN_LINES));
                    let board = egui::Rect::from_center_size(screen.center(), (galley.size() + pad * 2.0).min(screen.size()));
                    painter.rect_filled(board, (FONT_SIZE + pad.y * 2.0) * 0.3, fill);
                    painter.galley(board.min + pad, galley, ink);
                }
                Look::Panel { .. } => {
                    let t = solidcraft_theme::Tokens::get();
                    let scale = h as f32 / (t.space_24 * 2.0);
                    let stroke = egui::Stroke::new(scale, t.dialog_border);
                    painter.rect(screen.shrink(stroke.width), f32::from(t.radius_xl) * scale, t.dialog_body, stroke, egui::StrokeKind::Inside);
                }
                Look::Picture { .. } => {}
                Look::Joint { badge } => solidcraft_ui_egui::vr_pick::paint_joint(painter, screen, badge),
                Look::Handle { lit } => {
                    let t = solidcraft_theme::Tokens::get();
                    let accent = solidcraft_theme::TabAccent::current(app);
                    let bar = screen.shrink2(egui::vec2(h as f32 * 0.3, h as f32 * 0.25));
                    painter.rect_filled(bar, bar.height() * 0.5, if lit { accent.color } else { t.form_dim });
                }
                Look::Bubble { icon, lit, fade } => {
                    // The desktop radial menu's slot (`ui_kit::radial`): its colours and strokes,
                    // the pictogram and its words laid out to fit inside it.
                    let t = solidcraft_theme::Tokens::get();
                    let accent = solidcraft_theme::TabAccent::current(app);
                    let slot = egui::vec2(t.space_24 * 2.0 + t.space_12, t.space_24 + t.space_16 + t.space_2);
                    let scale = (w as f32 / slot.x).min(h as f32 / slot.y);
                    let mut painter = painter.clone();
                    if fade < 100 {
                        painter.multiply_opacity(f32::from(fade) / 100.0);
                    }
                    let stroke = egui::Stroke::new(if lit { 1.6 } else { 1.0 } * scale, if lit { accent.color } else { t.dialog_border });
                    let board = screen.shrink(stroke.width);
                    let fill = if lit { accent.soft } else { t.dialog_body };
                    painter.rect(board, f32::from(t.radius_xl) * scale, fill, stroke, egui::StrokeKind::Inside);
                    let ink = if lit { accent.ink } else { t.form_ink };
                    let inside = board.shrink(f32::from(t.radius_xl) * scale * 0.6);
                    match icon {
                        // Without words the icon alone fills the middle.
                        Some(name) if text.trim().is_empty() => {
                            let side = inside.height().min(inside.width() * 0.6).max(0.0);
                            let at = egui::Rect::from_center_size(inside.center(), egui::vec2(side, side));
                            solidcraft_ui_egui::ui_kit::icons::paint_accented(
                                &painter,
                                at,
                                name,
                                solidcraft_theme::TabAccent::current_icon_accent(app),
                                1.0,
                            );
                        }
                        Some(name) => {
                            // One line under the pictogram: a word too long for it ("Show/Hide",
                            // "Zusammensetzen") is set smaller before it is cut.
                            let words = short(text, BUBBLE_CHARS);
                            let natural = painter.layout_no_wrap(words.clone(), egui::FontId::proportional(t.font_small * scale), ink).size().x;
                            let size = fit_size(natural, inside.width(), t.font_small * scale);
                            let label = painter.layout_job(wrapped(&words, size, ink, inside.width(), 1));
                            let gap = t.space_2 * scale;
                            let side = (inside.height() - label.size().y - gap).min(inside.width() * 0.6).max(0.0);
                            let top = inside.center().y - (side + gap + label.size().y) * 0.5;
                            let at = egui::Rect::from_min_size(egui::pos2(inside.center().x - side * 0.5, top), egui::vec2(side, side));
                            solidcraft_ui_egui::ui_kit::icons::paint_accented(
                                &painter,
                                at,
                                name,
                                solidcraft_theme::TabAccent::current_icon_accent(app),
                                1.0,
                            );
                            let label_at = egui::pos2(inside.center().x - label.size().x * 0.5, top + side + gap);
                            painter.galley(label_at, label, ink);
                        }
                        None => {
                            let label = painter.layout_job(wrapped(&short(text, BUBBLE_CHARS), t.font_label * scale, ink, inside.width(), 2));
                            painter.galley(inside.center() - label.size() * 0.5, label, ink);
                        }
                    }
                }
            }
        });
        let primitives = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        self.upload(rs, &out.textures_delta);
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sc_vr_sign"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: out.pixels_per_point };
        let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_vr_sign") });
        let uploads = self.renderer.update_buffers(&rs.device, &rs.queue, &mut encoder, &primitives, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_vr_sign"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
            self.renderer.render(&mut pass, &primitives, &screen);
        }
        rs.queue.submit(uploads.into_iter().chain([encoder.finish()]));
        self.free(&out.textures_delta);
        // Handled: egui checks nothing is left unapplied.
        out.textures_delta.clear();
        view
    }

    /// Give the renderer the pictures (the font atlas) egui made or changed in a pass.
    fn upload(&mut self, rs: &egui_wgpu::RenderState, delta: &egui::TexturesDelta) {
        for (id, images) in &delta.set {
            for image in images {
                self.renderer.update_texture(&rs.device, &rs.queue, *id, image);
            }
        }
    }

    /// Let go of the pictures egui no longer uses after a pass.
    fn free(&mut self, delta: &egui::TexturesDelta) {
        for id in &delta.free {
            self.renderer.free_texture(id);
        }
    }
}

/// A bubble's words longer than this many characters are cut short with an ellipsis.
const BUBBLE_CHARS: usize = 40;

/// `text` in `ink` at `size`, wrapped to `width`, at most `rows` lines (the last ending in an
/// ellipsis where the words go on).
fn wrapped(text: &str, size: f32, ink: egui::Color32, width: f32, rows: usize) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), egui::FontId::proportional(size), ink, width.max(1.0));
    job.wrap.max_rows = rows.max(1);
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job
}

/// The smallest share of its size a bubble's line of words is set at to fit; below it, the
/// words are cut with an ellipsis instead.
const BUBBLE_MIN_SHRINK: f32 = 0.7;

/// The size to set a line `natural` wide at `size` in, so it fits in `width`: `size` where it
/// fits, smaller where not, never below [`BUBBLE_MIN_SHRINK`] of it.
fn fit_size(natural: f32, width: f32, size: f32) -> f32 {
    if !(natural.is_finite() && width.is_finite() && natural > width && width > 0.0) {
        return size;
    }
    size * (width / natural).max(BUBBLE_MIN_SHRINK)
}

/// `text` on one line, at most `max` characters (the last an ellipsis when cut).
fn short(text: &str, max: usize) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bubble's line that is too wide is set smaller to fit, but never below the floor; one
    /// that fits, or a width that is no width, keeps its size.
    #[test]
    fn a_bubbles_long_word_is_set_smaller_before_it_is_cut() {
        assert_eq!(fit_size(80.0, 100.0, 20.0), 20.0);
        assert!((fit_size(125.0, 100.0, 20.0) - 16.0).abs() < 1e-4);
        assert!((fit_size(1000.0, 100.0, 20.0) - 20.0 * BUBBLE_MIN_SHRINK).abs() < 1e-4);
        for (natural, width) in [(f32::NAN, 100.0), (125.0, 0.0), (125.0, f32::INFINITY), (f32::INFINITY, 100.0)] {
            assert_eq!(fit_size(natural, width, 20.0), 20.0, "{natural} in {width}");
        }
    }

    /// A sign's words are put on one line (runs of spaces and line breaks made one space) and
    /// cut at the most characters with an ellipsis, counting characters, not bytes.
    #[test]
    fn a_signs_words_are_put_on_one_line_and_cut_with_an_ellipsis() {
        assert_eq!(short("Bracket  left\n\tside ", 40), "Bracket left side");
        assert_eq!(short("", 40), "");
        assert_eq!(short("exactly", 7), "exactly", "as long as allowed: not cut");
        let cut = short(&"x".repeat(500), MAX_CHARS);
        assert_eq!(cut.chars().count(), MAX_CHARS);
        assert!(cut.ends_with('…') && cut.starts_with("xxx"));
        let umlauts = short(&"äöü ".repeat(100), BUBBLE_CHARS);
        assert_eq!(umlauts.chars().count(), BUBBLE_CHARS, "characters, not bytes");
        for max in 1..5 {
            assert!(short("a long name", max).chars().count() <= max, "{max}");
        }
    }

    /// Wrapped words never ask for no lines or a width of nothing (or one that is not a number):
    /// at least one line, at least a point wide.
    #[test]
    fn wrapped_words_take_at_least_one_line_a_point_wide() {
        for (width, rows) in [(f32::NAN, 0), (-5.0, 0), (0.0, 3), (300.0, 2)] {
            let job = wrapped("words", 20.0, egui::Color32::WHITE, width, rows);
            assert!(job.wrap.max_width >= 1.0, "{width}");
            assert_eq!(job.wrap.max_rows, rows.max(1));
            assert_eq!(job.wrap.overflow_character, Some('…'));
        }
    }

    /// The fonts' fingerprint is the same for the same definitions (a copy of them too: the
    /// font files are shared, not read), and another for another face first in a family, a face
    /// added, or another font file under the same name.
    #[test]
    fn the_fonts_fingerprint_changes_with_the_faces_not_with_a_copy() {
        let fonts = egui::FontDefinitions::default();
        let same = fonts_fingerprint(&fonts);
        assert_eq!(fonts_fingerprint(&fonts.clone()), same, "a copy shares the files");
        let mut reordered = fonts.clone();
        if let Some(faces) = reordered.families.get_mut(&egui::FontFamily::Proportional) {
            faces.reverse();
        }
        assert_ne!(fonts_fingerprint(&reordered), same, "another face first");
        let mut added = fonts.clone();
        added.font_data.insert("extra".into(), std::sync::Arc::new(egui::FontData::from_static(b"not a font")));
        assert_ne!(fonts_fingerprint(&added), same, "a face added");
        let mut replaced = fonts.clone();
        let name = replaced.font_data.keys().next().cloned().expect("egui has default fonts");
        replaced.font_data.insert(name, std::sync::Arc::new(egui::FontData::from_static(b"another file")));
        assert_ne!(fonts_fingerprint(&replaced), same, "another file under the same name");
    }

    /// The theme's generation stays while its tokens do and goes up once for each change, also
    /// back to a theme seen before (its signs were painted over since).
    #[test]
    fn the_theme_generation_goes_up_when_the_tokens_change() {
        let mut watch = ThemeWatch::default();
        let dark = solidcraft_theme::Tokens::get();
        let first = watch.generation(dark);
        assert_eq!(watch.generation(dark), first, "the same theme");
        let mut light = dark;
        light.dialog_body = egui::Color32::from_rgb(1, 2, 3);
        let second = watch.generation(light);
        assert_ne!(second, first, "a colour the signs paint with changed");
        assert_eq!(watch.generation(light), second);
        let third = watch.generation(dark);
        assert!(third != second && third != first, "back again is another painting");
    }

    /// Each picture is a size the GPU can make: a box's width follows its ratio but stays within
    /// the most a picture may be, for a ratio of nothing or a huge one too.
    #[test]
    fn every_look_has_a_picture_size_the_gpu_can_make() {
        assert_eq!(Look::Sign.size(), SIGN_PX);
        assert_eq!(Look::Bubble { icon: None, lit: false, fade: 100 }.size(), BUBBLE_PX);
        assert_eq!(Look::Panel { ratio_tenths: 25 }.size(), [320, 128]);
        assert_eq!(Look::Panel { ratio_tenths: 0 }.size(), [128, 128], "never narrower than tall");
        assert_eq!(Look::Panel { ratio_tenths: u32::MAX }.size(), [2048, 128], "never wider than a picture may be");
        assert_eq!(Look::Picture { size: [600, 900] }.size(), [600, 900]);
        for look in [Look::Sign, Look::Handle { lit: true }, Look::Panel { ratio_tenths: 99 }, Look::Bubble { icon: Some("eye"), lit: true, fade: 0 }]
        {
            let [w, h] = look.size();
            assert!((1..=2048).contains(&w) && (1..=2048).contains(&h), "{look:?}");
        }
    }
}
