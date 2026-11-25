//! Colours a plugin gives the ribbon's icons, its button blocks and the panels each frame
//! (a `tint` shader, `shaders@0.2`). The painters ask here for a region's colour; the host evaluates the plugin's
//! `tint` for every region asked about in a frame and hands the answers back with [`answer`],
//! which later frames paint. With no tint on, asking costs one lookup and paints as before.

use std::collections::{HashMap, HashSet};

use egui::{Color32, Context, Id, Pos2, Rect, Rgba};

/// The most regions asked about in one frame; more are painted untinted.
pub const MAX_ASKED: usize = 8192;
/// How far a region may have moved and still take its old answer, in points.
const NEAR: f32 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Icon = 0,
    Button = 1,
    Panel = 2,
}

/// One region the host asks the plugin about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ask {
    pub region: Region,
    pub rect: Rect,
    pub at: Pos2,
    /// The region's own colour.
    pub base: Color32,
    /// 1 for an enabled control, less for a disabled one.
    pub enabled: f32,
}

type Key = (Region, [i32; 6]);

impl Ask {
    /// `ScRegion` as the tint shader reads it: rect, base (linear), position, kind, enabled.
    pub fn bytes(&self) -> [u8; 48] {
        let base = Rgba::from(self.base);
        let f =
            [self.rect.left(), self.rect.top(), self.rect.right(), self.rect.bottom(), base.r(), base.g(), base.b(), base.a(), self.at.x, self.at.y];
        let mut out = [0u8; 48];
        for (i, v) in f.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        out[40..44].copy_from_slice(&(self.region as u32).to_le_bytes());
        out[44..48].copy_from_slice(&self.enabled.to_le_bytes());
        out
    }

    fn key(&self) -> Key {
        let q = |v: f32| (v * 4.0).round() as i32;
        (self.region, [q(self.rect.left()), q(self.rect.top()), q(self.rect.right()), q(self.rect.bottom()), q(self.at.x), q(self.at.y)])
    }
}

#[derive(Clone, Default)]
struct Frame {
    on: bool,
    asked: Vec<Ask>,
    seen: HashSet<Key>,
    answers: HashMap<Key, [f32; 4]>,
    /// The answers by region kind and point, for a region that moved since it was asked about.
    near: Vec<(Region, Pos2, [f32; 4])>,
}

fn id() -> Id {
    Id::unique("sc_plugin_tint")
}

/// Set by the host at the start of each frame: whether a plugin's tint is on. Off forgets every answer.
pub fn set_on(ctx: &Context, on: bool) {
    if !on && !is_on(ctx) {
        return;
    }
    ctx.data_mut(|d| {
        let frame = d.get_temp_mut_or_default::<Frame>(id());
        if !on {
            *frame = Frame::default();
        }
        frame.on = on;
        frame.asked.clear();
        frame.seen.clear();
    });
}

pub fn is_on(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<Frame>(id()).is_some_and(|f| f.on))
}

/// The plugin's last answer for `ask` (linear RGBA, straight alpha), and `ask` is asked again this
/// frame. `None` with no tint on or no answer yet.
pub fn ask(ctx: &Context, ask: Ask) -> Option<[f32; 4]> {
    if !is_on(ctx) {
        return None;
    }
    ctx.data_mut(|d| {
        let frame = d.get_temp_mut_or_default::<Frame>(id());
        let key = ask.key();
        if frame.asked.len() < MAX_ASKED && frame.seen.insert(key) {
            frame.asked.push(ask);
        }
        frame.answers.get(&key).copied().or_else(|| {
            // Scrolled or laid out anew: the nearest answer of its kind, until its own comes.
            frame
                .near
                .iter()
                .filter(|(region, at, _)| *region == ask.region && at.distance(ask.at) <= NEAR)
                .min_by(|a, b| a.1.distance(ask.at).total_cmp(&b.1.distance(ask.at)))
                .map(|(_, _, tint)| *tint)
        })
    })
}

/// The regions asked about this frame, for the host to evaluate.
pub fn take_asked(ctx: &Context) -> Vec<Ask> {
    ctx.data_mut(|d| std::mem::take(&mut d.get_temp_mut_or_default::<Frame>(id()).asked))
}

/// The plugin's answers to `asked`, one each, which replace the earlier ones.
pub fn answer(ctx: &Context, asked: &[Ask], tints: &[[f32; 4]]) {
    ctx.data_mut(|d| {
        let frame = d.get_temp_mut_or_default::<Frame>(id());
        if !frame.on {
            return;
        }
        let clean = |t: &[f32; 4]| t.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 });
        frame.answers = asked.iter().zip(tints).map(|(a, t)| (a.key(), clean(t))).collect();
        frame.near = asked.iter().zip(tints).map(|(a, t)| (a.region, a.at, clean(t))).collect();
    });
}

/// An answer as the painters draw it: sRGB, premultiplied by its alpha.
pub fn colour(tint: [f32; 4]) -> Color32 {
    Color32::from(Rgba::from_rgba_unmultiplied(tint[0], tint[1], tint[2], tint[3]))
}

/// A ribbon icon in the plugin's tint, or as it always is.
pub fn icon(p: &egui::Painter, r: Rect, name: &str, opacity: f32) {
    let base = solidcraft_theme::Tokens::get().accent;
    let asked = Ask { region: Region::Icon, rect: r, at: r.center(), base, enabled: opacity.clamp(0.0, 1.0) };
    match ask(p.ctx(), asked) {
        Some(t) if t[3] > 0.0 => {
            let opaque = Color32::from(Rgba::from_rgb(t[0], t[1], t[2]));
            crate::icons::paint_tinted(p, r, name, opacity, opaque, t[3]);
        }
        _ => crate::icons::paint_icon(p, r, name, opacity),
    }
}

/// A ribbon button's block in the plugin's tint, over `base`, its own colour; nothing without one.
pub fn button(p: &egui::Painter, r: Rect, radius: u8, base: Color32, enabled: bool) {
    let asked = Ask { region: Region::Button, rect: r, at: r.center(), base, enabled: if enabled { 1.0 } else { 0.35 } };
    if let Some(t) = ask(p.ctx(), asked)
        && t[3] > 0.0
    {
        p.rect_filled(r, radius, colour(t));
    }
}

#[cfg(test)]
#[path = "tint_tests.rs"]
mod tests;
