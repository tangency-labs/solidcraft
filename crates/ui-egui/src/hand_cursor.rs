//! Original runtime cursor artwork. Native upload goes through the existing safe egui
//! output hook; no window handles, assets or platform FFI enter this module.
use std::sync::{Arc, OnceLock};

use egui::{CursorIcon, CustomCursorImage};
use solidcraft_theme::Tokens;

const SIDE: u16 = 32;
const SAMPLES: u32 = 4;
static HANDS: OnceLock<[CustomCursorImage; 2]> = OnceLock::new();

fn capsule(p: (f32, f32), a: (f32, f32), b: (f32, f32), radius: f32) -> bool {
    let v = (b.0 - a.0, b.1 - a.1);
    let length = v.0 * v.0 + v.1 * v.1;
    let t = if length > 0.0 { ((p.0 - a.0) * v.0 + (p.1 - a.1) * v.1) / length } else { 0.0 }.clamp(0.0, 1.0);
    (p.0 - a.0 - t * v.0).powi(2) + (p.1 - a.1 - t * v.1).powi(2) <= radius * radius
}

fn polygon(p: (f32, f32), vertices: &[(f32, f32)]) -> bool {
    let mut inside = false;
    for (a, b) in vertices.iter().zip(vertices.iter().cycle().skip(1)).take(vertices.len()) {
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

fn silhouette(p: (f32, f32), closed: bool) -> bool {
    let palm = ((p.0 - 17.0) / 8.0).powi(2) + ((p.1 - 21.0) / 7.0).powi(2) <= 1.0;
    let wrist = polygon(p, &[(11.0, 23.0), (23.0, 23.0), (23.0, 30.0), (12.0, 30.0)]);
    let fingers = if closed {
        [(10.5, 14.0), (15.5, 12.0), (20.5, 12.5), (25.0, 14.5)].into_iter().any(|a| capsule(p, a, (a.0, 20.0), 2.6))
            || capsule(p, (8.0, 20.0), (17.0, 22.0), 3.0)
    } else {
        [(10.5, 7.0), (15.5, 3.5), (20.5, 5.0), (25.0, 9.0)].into_iter().any(|a| capsule(p, a, (a.0, 21.0), 2.1))
            || capsule(p, (10.0, 23.0), (5.0, 16.0), 2.6)
    };
    palm || wrist || fingers
}

/// Straight RGBA, antialiased from an original union of capsules and a wrist polygon.
pub(crate) fn generate(closed: bool) -> CustomCursorImage {
    let t = Tokens::dark();
    let fill = t.on_accent.to_opaque().to_array();
    let outline = t.app_bar.to_opaque().to_array();
    let mut rgba = vec![0_u8; usize::from(SIDE) * usize::from(SIDE) * 4];
    for (pixel, bytes) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let mut channels = [0_u32; 4];
        let mut covered = 0_u32;
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let p = (
                    (pixel % usize::from(SIDE)) as f32 + (sx as f32 + 0.5) / SAMPLES as f32,
                    (pixel / usize::from(SIDE)) as f32 + (sy as f32 + 0.5) / SAMPLES as f32,
                );
                if silhouette(p, closed) {
                    let interior = [(-1.1, 0.0), (1.1, 0.0), (0.0, -1.1), (0.0, 1.1), (-0.8, -0.8), (0.8, -0.8), (-0.8, 0.8), (0.8, 0.8)]
                        .into_iter()
                        .all(|(x, y)| silhouette((p.0 + x, p.1 + y), closed));
                    for (sum, value) in channels.iter_mut().zip(if interior { fill } else { outline }) {
                        *sum += u32::from(value);
                    }
                    covered += 1;
                }
            }
        }
        if covered > 0 {
            for (channel, (byte, sum)) in bytes.iter_mut().zip(channels).enumerate() {
                *byte = (sum / if channel == 3 { SAMPLES * SAMPLES } else { covered }) as u8;
            }
        }
    }
    CustomCursorImage { rgba: Arc::from(rgba), size: [SIDE, SIDE], hotspot: [17, 21] }
}

/// Stable Arc identities let egui-winit reuse its cached native cursor each frame.
pub(crate) fn request(icon: CursorIcon) -> Option<(CustomCursorImage, CursorIcon)> {
    let (held, fallback) = match icon {
        CursorIcon::Grab => (false, CursorIcon::PointingHand),
        CursorIcon::Grabbing => (true, CursorIcon::Grabbing),
        _ => return None,
    };
    HANDS.get_or_init(|| [generate(false), generate(true)]).get(usize::from(held)).cloned().map(|image| (image, fallback))
}

#[cfg(windows)]
pub(crate) fn apply(ctx: &egui::Context) {
    if let Some((image, fallback)) = request(ctx.output(|o| o.cursor_icon)) {
        ctx.output_mut(|o| {
            o.cursor_icon = fallback;
            o.cursor_image = Some(image);
        });
    } else if HANDS.get().is_some_and(|hands| {
        ctx.output(|o| o.cursor_image.as_ref().is_some_and(|image| hands.iter().any(|hand| Arc::ptr_eq(&hand.rgba, &image.rgba))))
    }) {
        // PlatformOutput keeps cursor images between frames. Release only our artwork,
        // leaving unrelated custom cursors alone.
        ctx.set_cursor_image(None);
    }
}
