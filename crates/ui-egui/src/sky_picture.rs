//! The pictures behind the model that a sky effect shows: a plugin's own, decoded here, or the one
//! the host draws itself when the plugin gives none.

/// A picture's pixels, RGBA.
pub(crate) struct Picture {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

/// A picture about twice as wide as tall is a 360 panorama: it wraps round the view.
pub(crate) fn is_panorama(size: [u32; 2]) -> bool {
    let ratio = size[0] as f32 / size[1].max(1) as f32;
    (1.9..=2.1).contains(&ratio)
}

/// The part of a flat picture (as texture coordinates) that shows in a window: it covers the window
/// from its centre, so a narrower window keeps the middle of it.
pub(crate) fn cover_uv(image: egui::Vec2, window: egui::Vec2) -> egui::Rect {
    let scale = (window.x / image.x).max(window.y / image.y).max(f32::EPSILON);
    let half = window / scale / image / 2.0;
    egui::Rect::from_min_max(egui::pos2(0.5 - half.x, 0.5 - half.y), egui::pos2(0.5 + half.x, 0.5 + half.y))
}

/// A picture's pixels, at most `limit` on a side (what the GPU takes).
pub(crate) fn decode(bytes: &[u8], limit: u32) -> Result<Picture, String> {
    let mut image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    if image.width().max(image.height()) > limit {
        image = image.resize(limit, limit, image::imageops::FilterType::Triangle);
    }
    let rgba = image.into_rgba8();
    Ok(Picture { size: [rgba.width(), rgba.height()], rgba: rgba.into_raw() })
}

fn hash(a: i32, b: i32, c: i32) -> f32 {
    let mut h = (a as u32).wrapping_mul(0x9e37_79b1) ^ (b as u32).wrapping_mul(0x85eb_ca6b) ^ (c as u32).wrapping_mul(0xc2b2_ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth noise over a direction, so a wrap-around picture has no seam.
fn noise(p: [f32; 3]) -> f32 {
    let (i, f) = (p.map(f32::floor), [0, 1, 2].map(|k| p[k] - p[k].floor()));
    let s = f.map(|t| t * t * (3.0 - 2.0 * t));
    let at = |dx: i32, dy: i32, dz: i32| hash(i[0] as i32 + dx, i[1] as i32 + dy, i[2] as i32 + dz);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x = |dy, dz| lerp(at(0, dy, dz), at(1, dy, dz), s[0]);
    let y = |dz| lerp(x(0, dz), x(1, dz), s[1]);
    lerp(y(0), y(1), s[2])
}

/// The galaxy when the plugin gives no picture: a night sky as a 2:1 panorama `width` pixels wide,
/// a faint band of dust across it and stars. The same every time.
pub(crate) fn galaxy(width: u32) -> Picture {
    let (w, h) = (width.max(8), (width.max(8) / 2).max(4));
    let tilt = {
        let n = [0.35_f32, -0.25, 1.0];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        n.map(|v| v / len)
    };
    // Stars are one per cell of this many pixels, so none is cut by the wrap.
    const CELL: u32 = 4;
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        let lat = (0.5 - (y as f32 + 0.5) / h as f32) * std::f32::consts::PI;
        for x in 0..w {
            let lon = ((x as f32 + 0.5) / w as f32 - 0.5) * std::f32::consts::TAU;
            let dir = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];
            let off = dir[0] * tilt[0] + dir[1] * tilt[1] + dir[2] * tilt[2];
            let band = (-(off / 0.2).powi(2)).exp();
            let dust = noise(dir.map(|v| v * 4.0)) * 0.6 + noise(dir.map(|v| v * 9.0)) * 0.3 + noise(dir.map(|v| v * 21.0)) * 0.1;
            let glow = band * (0.25 + dust * 0.75);
            let mut c = [5.0 + glow * 60.0, 8.0 + glow * 52.0, 22.0 + glow * 70.0];
            let (cx, cy) = ((x / CELL) as i32, (y / CELL) as i32);
            // Denser along the band; the pole rows crowd, so thin them out there.
            let chance = (0.05 + band * 0.22) * lat.cos().max(0.2);
            if hash(cx, cy, 1) < chance {
                let at = [(cx as u32 * CELL) as f32 + 1.5 + hash(cx, cy, 2), (cy as u32 * CELL) as f32 + 1.5 + hash(cx, cy, 3)];
                let d2 = (x as f32 + 0.5 - at[0]).powi(2) + (y as f32 + 0.5 - at[1]).powi(2);
                let bright = hash(cx, cy, 4).powi(3) * 0.9 + 0.1;
                let warm = hash(cx, cy, 5);
                let tint = [0.75 + 0.25 * warm, 0.85, 1.0 - 0.3 * warm];
                let star = (-d2 / 0.7).exp() * bright * 340.0;
                for (channel, tint) in c.iter_mut().zip(tint) {
                    *channel += star * tint;
                }
            }
            let i = ((y * w + x) * 4) as usize;
            for (byte, channel) in rgba[i..i + 3].iter_mut().zip(c) {
                *byte = channel.clamp(0.0, 255.0) as u8;
            }
        }
    }
    Picture { size: [w, h], rgba }
}

/// The scene behind the rain when the plugin gives no picture: a dark night, 16:9 and `width`
/// pixels wide, with soft out-of-focus lights low in it. The same every time.
pub(crate) fn rain_night(width: u32) -> Picture {
    let (w, h) = (width.max(16), (width.max(16) * 9 / 16).max(9));
    let mut light = vec![[0.0_f32; 3]; (w * h) as usize];
    for k in 0..90 {
        let at = [hash(k, 1, 7) * w as f32, (0.35 + 0.65 * hash(k, 2, 7).sqrt()) * h as f32];
        let radius = (0.004 + 0.02 * hash(k, 3, 7).powi(2)) * w as f32;
        let warm = hash(k, 4, 7);
        let tint = if warm < 0.6 { [1.0, 0.72 + 0.2 * warm, 0.4] } else { [0.45, 0.7, 1.0] };
        let power = 0.4 + 0.6 * hash(k, 5, 7);
        let (x0, x1) = ((at[0] - radius).floor().max(0.0) as u32, ((at[0] + radius).ceil() as u32).min(w - 1));
        let (y0, y1) = ((at[1] - radius).floor().max(0.0) as u32, ((at[1] + radius).ceil() as u32).min(h - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - at[0]).powi(2) + (y as f32 + 0.5 - at[1]).powi(2)).sqrt() / radius;
                if d < 1.0 {
                    let v = power * (1.0 - smooth(0.0, 1.0, d));
                    for (channel, tint) in light[(y * w + x) as usize].iter_mut().zip(tint) {
                        *channel += v * tint;
                    }
                }
            }
        }
    }
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        let t = y as f32 / h as f32;
        for x in 0..w {
            let l = light[(y * w + x) as usize];
            let base = [7.0 + 8.0 * t, 9.0 + 6.0 * t, 18.0 + 8.0 * t];
            let i = ((y * w + x) * 4) as usize;
            for ((byte, base), light) in rgba[i..i + 3].iter_mut().zip(base).zip(l) {
                *byte = (base + light * 300.0).clamp(0.0, 255.0) as u8;
            }
        }
    }
    Picture { size: [w, h], rgba }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luma(p: &Picture, x: u32, y: u32) -> f32 {
        let i = ((y * p.size[0] + x) * 4) as usize;
        (f32::from(p.rgba[i]) + f32::from(p.rgba[i + 1]) + f32::from(p.rgba[i + 2])) / 765.0
    }

    #[test]
    fn the_galaxy_without_a_picture_is_a_dark_panorama_with_stars() {
        let sky = galaxy(512);
        assert_eq!(sky.size, [512, 256]);
        assert_eq!(sky.rgba.len(), 512 * 256 * 4);
        assert!(is_panorama(sky.size));
        let all: Vec<f32> = (0..256).flat_map(|y| (0..512).map(move |x| (x, y))).map(|(x, y)| luma(&sky, x, y)).collect();
        let mean = all.iter().sum::<f32>() / all.len() as f32;
        assert!((0.01..0.3).contains(&mean), "a night sky: {mean}");
        assert!(all.iter().any(|&l| l > 0.6), "stars are bright");
        assert!(sky.rgba.chunks(4).all(|px| px[3] == 255), "opaque");
    }

    #[test]
    fn the_galaxy_is_the_same_every_time_and_wraps_without_a_seam() {
        let (a, b) = (galaxy(512), galaxy(512));
        assert!(a.rgba == b.rgba);
        let seam: f32 = (0..256).map(|y| (luma(&a, 0, y) - luma(&a, 511, y)).abs()).sum::<f32>() / 256.0;
        let inside: f32 = (0..256).map(|y| (luma(&a, 255, y) - luma(&a, 256, y)).abs()).sum::<f32>() / 256.0;
        assert!(seam < inside * 2.0 + 0.02, "seam {seam}, inside {inside}");
    }

    #[test]
    fn a_picture_about_twice_as_wide_as_tall_is_a_panorama() {
        assert!(is_panorama([4096, 2048]) && is_panorama([2000, 1000]));
        assert!(!is_panorama([1920, 1080]) && !is_panorama([1000, 1000]));
    }

    #[test]
    fn a_flat_picture_covers_the_window_from_its_centre() {
        let wide = cover_uv(egui::vec2(1000.0, 500.0), egui::vec2(500.0, 500.0));
        assert_eq!((wide.min.y, wide.max.y), (0.0, 1.0));
        assert!((wide.min.x - 0.25).abs() < 1e-6 && (wide.max.x - 0.75).abs() < 1e-6);
        let tall = cover_uv(egui::vec2(500.0, 500.0), egui::vec2(1000.0, 500.0));
        assert_eq!((tall.min.x, tall.max.x), (0.0, 1.0));
        assert!((tall.min.y - 0.25).abs() < 1e-6 && (tall.max.y - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_picture_is_decoded_and_shrunk_to_what_the_gpu_takes() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let whole = decode(png.get_ref(), 1024).unwrap();
        assert_eq!((whole.size, &whole.rgba[..4]), ([64, 32], &[10, 20, 30, 255][..]));
        assert_eq!(decode(png.get_ref(), 16).unwrap().size, [16, 8]);
        assert!(decode(b"not a picture", 1024).is_err());
    }

    #[test]
    fn the_rain_without_a_picture_is_a_dark_flat_night_with_lights() {
        let night = rain_night(640);
        assert_eq!(night.size, [640, 360]);
        assert!(!is_panorama(night.size), "rain shows a flat picture");
        assert!(night.rgba == rain_night(640).rgba, "the same every time");
        let all: Vec<f32> = (0..360).flat_map(|y| (0..640).map(move |x| (x, y))).map(|(x, y)| luma(&night, x, y)).collect();
        let mean = all.iter().sum::<f32>() / all.len() as f32;
        assert!((0.03..0.4).contains(&mean), "a night: {mean}");
        assert!(all.iter().any(|&l| l > 0.5), "with lights in it");
        assert!(night.rgba.chunks(4).all(|px| px[3] == 255), "opaque");
    }
}
