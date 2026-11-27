//! The one way a file is previewed. Wherever the app pictures a file or a part (the start page, a
//! parts library, a dialog), it draws it with [`file_preview`]: an isometric grid in the theme's
//! 3D view colours, the model's cut-out picture on it, its footprint shadow, and the lift when
//! the pointer is on its card. The host gives the pixels ([`PreviewImage`]) and keeps the
//! textures ([`PreviewTextures`]); what a preview looks like and how it moves is decided here.
//!
//! A picture is isometric, seen from the one home view, so the grid is the same under all of them
//! and needs nothing of the model.

use std::f32::consts::FRAC_PI_6;
use std::sync::Arc;

use egui::epaint::{ColorMode, PathShape, PathStroke};
use egui::{Color32, ColorImage, Context, Id, Mesh, Pos2, Rect, Shape, TextureHandle, TextureId, TextureOptions, Ui, Vec2, pos2, vec2};

use solidcraft_theme::Tokens;

/// A picture to preview: the model cut out over transparent pixels, premultiplied, 4:3.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewImage {
    pub picture: ColorImage,
    pub shadow: Option<PreviewShadow>,
}

/// The model's footprint on the ground: how much of each cell it covers.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewShadow {
    pub mask: Vec<u8>,
    pub size: [usize; 2],
    /// The area the mask covers, in parts of the picture: (0, 0) to (1, 1) is the picture.
    pub area: Rect,
}

/// The widest a picture is kept at, and the copy drawn in small slots: textures have no smaller
/// levels of their own, and a picture drawn far below its size shimmers.
const LARGE_WIDTH: usize = 384;
const SMALL_WIDTH: usize = 128;

/// A preview's textures. Dropping them frees them.
pub struct PreviewTextures {
    large: TextureHandle,
    small: TextureHandle,
    shadow: Option<ShadowTextures>,
}

/// The footprint blurred twice: tight where the model touches the ground, wide as it rises.
struct ShadowTextures {
    tight: TextureHandle,
    wide: TextureHandle,
    /// The blurred textures' area, in parts of the picture.
    area: Rect,
    /// The middle of the footprint, in parts of the picture.
    ground: Pos2,
    /// The footprint's width, as a part of the picture's.
    footprint: f32,
}

/// What [`file_preview`] draws: handles into [`PreviewTextures`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewArt {
    large: TextureId,
    small: TextureId,
    shadow: Option<ShadowArt>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ShadowArt {
    tight: TextureId,
    wide: TextureId,
    area: Rect,
    ground: Pos2,
    footprint: f32,
}

impl PreviewTextures {
    /// Upload `image` as textures named after `name`.
    pub fn load(ctx: &Context, name: &str, image: PreviewImage) -> Self {
        let upload = |suffix: &str, image: ColorImage| ctx.load_texture(format!("{name}:{suffix}"), image, TextureOptions::LINEAR);
        let large = shrink(&image.picture, LARGE_WIDTH);
        let small = shrink(&large, SMALL_WIDTH);
        let shadow = image.shadow.and_then(|shadow| {
            let [tight_blur, wide_blur] = [1, 6].map(|radius| blurred(&shadow.mask, shadow.size, radius));
            let (tight, size) = tight_blur?;
            let (wide, _) = wide_blur?;
            let cell = vec2(shadow.area.width() / shadow.size[0] as f32, shadow.area.height() / shadow.size[1] as f32);
            let grown = shadow.area.expand2(cell * BLUR_ROOM as f32);
            let covered = covered(&shadow.mask, shadow.size)?;
            let ground = covered.center();
            Some(ShadowTextures {
                tight: upload("shadow", alpha_image(tight, size)),
                wide: upload("soft-shadow", alpha_image(wide, size)),
                area: grown,
                ground: shadow.area.min + vec2(ground.x * shadow.area.width(), ground.y * shadow.area.height()),
                footprint: covered.width() * shadow.area.width(),
            })
        });
        PreviewTextures { large: upload("picture", large), small: upload("small", small), shadow }
    }

    pub fn art(&self) -> PreviewArt {
        PreviewArt {
            large: self.large.id(),
            small: self.small.id(),
            shadow: self.shadow.as_ref().map(|s| ShadowArt {
                tight: s.tight.id(),
                wide: s.wide.id(),
                area: s.area,
                ground: s.ground,
                footprint: s.footprint,
            }),
        }
    }
}

/// `image` no wider than `width`, each pixel the average of those it covers.
fn shrink(image: &ColorImage, width: usize) -> ColorImage {
    let [from_w, from_h] = image.size;
    if from_w <= width || from_w == 0 || from_h == 0 {
        return image.clone();
    }
    let height = (from_h * width / from_w).max(1);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let rows = y * from_h / height..((y + 1) * from_h / height).max(y * from_h / height + 1);
        for x in 0..width {
            let columns = x * from_w / width..((x + 1) * from_w / width).max(x * from_w / width + 1);
            let (mut sum, mut count) = ([0u32; 4], 0u32);
            for sy in rows.clone() {
                for sx in columns.clone() {
                    if let Some(p) = image.pixels.get(sy * from_w + sx) {
                        for (total, value) in sum.iter_mut().zip(p.to_array()) {
                            *total += u32::from(value);
                        }
                        count += 1;
                    }
                }
            }
            let [r, g, b, a] = sum.map(|total| ((total + count / 2) / count.max(1)) as u8);
            pixels.push(Color32::from_rgba_premultiplied(r, g, b, a));
        }
    }
    ColorImage::new([width, height], pixels)
}

/// Clear cells added around the mask so a blur has somewhere to spread.
const BLUR_ROOM: usize = 20;

/// The mask, with [`BLUR_ROOM`] around it, box blurred three times (close to a Gaussian).
fn blurred(mask: &[u8], [width, height]: [usize; 2], radius: usize) -> Option<(Vec<u8>, [usize; 2])> {
    if width == 0 || height == 0 || mask.len() != width.checked_mul(height)? {
        return None;
    }
    let size = [width + 2 * BLUR_ROOM, height + 2 * BLUR_ROOM];
    let mut cells = vec![0u8; size[0] * size[1]];
    for (y, row) in mask.chunks(width).enumerate() {
        let start = (y + BLUR_ROOM) * size[0] + BLUR_ROOM;
        cells.get_mut(start..start + width)?.copy_from_slice(row);
    }
    let mut line = Vec::new();
    for _ in 0..3 {
        // Along the rows, then along the columns.
        for (length, step, lines, next) in [(size[0], 1, size[1], size[0]), (size[1], size[0], size[0], 1)] {
            for l in 0..lines {
                line.clear();
                line.extend((0..length).filter_map(|i| cells.get(l * next + i * step).copied()));
                let at = |i: usize| line.get(i).map_or(0, |v| u32::from(*v));
                let window = (2 * radius + 1) as u32;
                let mut sum: u32 = (0..=radius).map(at).sum();
                for i in 0..length {
                    if let Some(cell) = cells.get_mut(l * next + i * step) {
                        *cell = ((sum + window / 2) / window).min(255) as u8;
                    }
                    sum += at(i + radius + 1);
                    sum = sum.saturating_sub(i.checked_sub(radius).map_or(0, at));
                }
            }
        }
    }
    Some((cells, size))
}

/// The box around the covered cells, in parts of the mask.
fn covered(mask: &[u8], [width, height]: [usize; 2]) -> Option<Rect> {
    let covered = mask.iter().enumerate().filter(|(_, v)| **v > 0).map(|(i, _)| (i % width.max(1), i / width.max(1)));
    let (mut lo, mut hi) = ((usize::MAX, usize::MAX), (0, 0));
    for (x, y) in covered {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x + 1), hi.1.max(y + 1));
    }
    let part = |(x, y): (usize, usize)| pos2(x as f32 / width as f32, y as f32 / height as f32);
    (lo.0 < hi.0).then(|| Rect::from_min_max(part(lo), part(hi)))
}

/// White at each cell's coverage, so that a tint gives the shadow its colour and strength.
fn alpha_image(cells: Vec<u8>, size: [usize; 2]) -> ColorImage {
    ColorImage::new(size, cells.into_iter().map(Color32::from_white_alpha).collect())
}

/// The lift's spring: how far the model is off the ground (0 rests, 1 is lifted) and how fast.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Lift {
    height: f32,
    speed: f32,
}

impl Lift {
    /// The spring after `dt` seconds towards lifted or at rest. It overshoots a little on the way
    /// up, and bounces once on the ground without ever going through it.
    fn step(mut self, up: bool, dt: f32) -> Self {
        let (target, stiffness, damping) = if up { (1.0, 240.0, 17.0) } else { (0.0, 210.0, 13.0) };
        let h = dt.clamp(0.0, 1.0 / 30.0) / 4.0;
        for _ in 0..4 {
            self.speed += (stiffness * (target - self.height) - damping * self.speed) * h;
            self.height += self.speed * h;
            if !up && self.height < 0.0 {
                self.height = 0.0;
                self.speed = if self.speed.abs() > 1.2 { -self.speed * 0.22 } else { 0.0 };
            }
        }
        if (self.height - target).abs() < 0.002 && self.speed.abs() < 0.02 { Lift { height: target, speed: 0.0 } } else { self }
    }
}

/// The preview's lift this frame, moved on from the last one's. It asks for frames only while it
/// moves. With animations off nothing moves.
fn lift(ui: &Ui, id: Id, hovered: bool) -> f32 {
    if ui.ctx().global_style().animation_time <= 0.0 {
        return 0.0;
    }
    let before: Lift = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if !hovered && before == Lift::default() {
        return 0.0;
    }
    let after = before.step(hovered, ui.input(|i| i.stable_dt));
    if after != before {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| {
        if after == Lift::default() {
            d.remove::<Lift>(id);
        } else {
            d.insert_temp(id, after);
        }
    });
    after.height
}

/// The picture's units: its layout is given for a frame this wide, whatever size it is drawn at.
const FRAME: Vec2 = vec2(512.0, 384.0);
/// The picture is drawn a little smaller than the frame, which leaves room to lift it.
const PART: f32 = 0.9;
/// The grid's cell in frame units, and every how many lines one is stronger.
const CELL: f32 = 32.0;
const MAJOR: i32 = 4;
/// A frame narrower than this (points) is a small slot: wider cells, and a longer lift to show.
const SMALL_FRAME: f32 = 96.0;
/// How far the grid's glow reaches, in frame units, and past a wider footprint: this many times
/// its width, so the light always shows around the model that hides its middle.
const GLOW_REACH: f32 = 240.0;
const GLOW_PAST_FOOTPRINT: f32 = 0.9;
const GLOW_STRENGTH: f32 = 0.7;

/// No slot has more grid lines each way than this, however it is shaped.
const MOST_LINES: i32 = 40;

/// Draw a file's preview filling `rect`: the grid, and `art` on it, if there is a picture yet.
/// `hovered` lifts the model (the pointer, or the keyboard, is on the card that holds it); `id`
/// keeps the lift between frames.
pub fn file_preview(ui: &Ui, id: Id, rect: Rect, art: Option<PreviewArt>, hovered: bool, opacity: f32) {
    let height = lift(ui, id, hovered && art.is_some() && ui.is_enabled());
    if !ui.is_rect_visible(rect) || !rect.is_finite() || rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let t = Tokens::view();
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
    painter.add(backdrop(rect, f32::from(t.radius_md), t.viewport_top, t.viewport_bottom));
    // The largest 4:3 frame inside the slot, centred.
    let scale = (rect.width() / FRAME.x).min(rect.height() / FRAME.y);
    let frame = Rect::from_center_size(rect.center(), FRAME * scale);
    let small = frame.width() < SMALL_FRAME;
    let drawn = Rect::from_center_size(frame.center(), frame.size() * PART);
    let in_picture = |p: Pos2| drawn.min + vec2(p.x * drawn.width(), p.y * drawn.height());
    let shadow = art.and_then(|a| a.shadow);
    // The point of the ground under the model, which the glow and the shadow gather round.
    let ground = shadow.map_or(frame.center(), |s| in_picture(s.ground));
    let footprint = shadow.map_or(0.0, |s| s.footprint * drawn.width());
    let glow = Glow { at: ground, reach: (GLOW_REACH * scale).max(footprint * GLOW_PAST_FOOTPRINT), strength: height.clamp(0.0, 1.0) };
    grid(&painter, rect, glow, scale, small, &t);

    let Some(art) = art else { return };
    let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    if let Some(s) = art.shadow {
        let risen = height.clamp(0.0, 1.0);
        let spread = 1.0 + risen * 0.05;
        let around = |p: Pos2| ground + (in_picture(p) - ground) * spread;
        let area = Rect::from_min_max(around(s.area.min), around(s.area.max));
        // A darker ground needs a stronger shadow to show.
        let strength = if t.viewport_bottom.intensity() < 0.5 { 0.55 } else { 0.34 };
        for (texture, share) in [(s.tight, 0.55 - 0.4 * risen), (s.wide, 0.25 + 0.5 * risen)] {
            painter.image(texture, area, whole, Color32::BLACK.gamma_multiply(strength * share * opacity));
        }
    }
    let rise = height * if small { 30.0 } else { 22.0 } * scale;
    let grown = 1.0 + height.max(0.0) * 0.018;
    let target = Rect::from_center_size(drawn.center() - vec2(0.0, rise), drawn.size() * grown);
    let texture = if target.width() * ui.ctx().pixels_per_point() <= SMALL_WIDTH as f32 * 1.5 { art.small } else { art.large };
    painter.image(texture, target, whole, Tokens::get().file_image_tint.gamma_multiply(opacity));
}

/// The 3D view's gradient, with rounded corners.
fn backdrop(rect: Rect, radius: f32, top: Color32, bottom: Color32) -> Shape {
    let radius = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    let color = |y: f32| top.lerp_to_gamma(bottom, ((y - rect.top()) / rect.height()).clamp(0.0, 1.0));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), color(rect.center().y));
    let inner = rect.shrink(radius);
    // Each corner's arc, clockwise from the top left.
    for (corner, from) in [(inner.left_top(), 2.0), (inner.right_top(), 3.0), (inner.right_bottom(), 0.0), (inner.left_bottom(), 1.0)] {
        for step in 0..=6 {
            let angle = (from + step as f32 / 6.0) * std::f32::consts::FRAC_PI_2;
            let at = corner + vec2(angle.cos(), angle.sin()) * radius;
            mesh.colored_vertex(at, color(at.y));
        }
    }
    let ring = mesh.vertices.len() as u32 - 1;
    for i in 0..ring {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % ring);
    }
    Shape::mesh(mesh)
}

/// The light on the grid around a rising model.
#[derive(Clone, Copy)]
struct Glow {
    at: Pos2,
    reach: f32,
    strength: f32,
}

/// The ground grid of a true isometric view, its two directions 30 degrees off level, fading out
/// towards the slot's corners, with `glow` tinting the lines around the model as it rises.
fn grid(painter: &egui::Painter, rect: Rect, glow: Glow, scale: f32, small: bool, t: &Tokens) {
    let centre = rect.center();
    let cell = CELL * scale * if small { 2.0 } else { 1.0 };
    let reach = rect.size().length() * 0.55;
    if cell <= 0.5 || reach <= 0.0 {
        return;
    }
    let lines = ((reach * 1.2 / cell).ceil() as i32).clamp(1, MOST_LINES);
    let axes = [vec2(FRAC_PI_6.cos(), FRAC_PI_6.sin()), vec2(-FRAC_PI_6.cos(), FRAC_PI_6.sin())];
    let (minor, major, accent) = (t.grid, t.grid_major, t.accent);
    let width = (scale / 0.4).clamp(0.6, 1.5);
    for (along, across) in [(axes[1], axes[0]), (axes[0], axes[1])] {
        // The lines are spaced along the other direction.
        for i in -lines..=lines {
            let strong = i.rem_euclid(MAJOR) == 0;
            let base = if strong { major } else { minor };
            let through = centre + across * (i as f32 * cell);
            const PIECES: i32 = 12;
            let points: Vec<Pos2> = (-PIECES..=PIECES).map(|k| through + along * (k as f32 / PIECES as f32 * reach * 1.4)).collect();
            let color = move |_: Rect, p: Pos2| {
                let far = (p - centre).length() / reach;
                let fade = (1.0 - far * far).clamp(0.0, 1.0);
                let from_ground = p - glow.at;
                // How far over the ground, not over the screen: the ground is seen at a slant.
                let d = vec2(from_ground.x, from_ground.y * 1.732).length() / glow.reach.max(1.0);
                let lit = glow.strength * (-d * d).exp() * GLOW_STRENGTH;
                // A lit line fades less, so the glow still shows towards the slot's edges.
                base.lerp_to_gamma(accent, lit).gamma_multiply(fade.max(lit))
            };
            let stroke =
                PathStroke { width: width * if strong { 1.0 } else { 0.7 }, color: ColorMode::UV(Arc::new(color)), kind: egui::StrokeKind::Middle };
            painter.add(Shape::Path(PathShape { points, closed: false, fill: Color32::TRANSPARENT, stroke }));
        }
    }
}

#[cfg(test)]
#[path = "file_preview_tests.rs"]
mod tests;
