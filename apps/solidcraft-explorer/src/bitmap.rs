//! The saved picture made into what a Windows DIB section holds: decoded with an allocation
//! limit, fitted into the size File Explorer asks for and scaled down (never up) with its
//! proportions kept, as top-down rows of B, G, R, A bytes with straight (not premultiplied) alpha.
//!
//! Scaling averages the source pixels each output pixel covers, each weighted by how much of it is
//! covered, in premultiplied alpha, so the colour of a transparent pixel never bleeds into the
//! model's edge. Nothing here is Windows-only, so the tests run on every platform.

/// A picture ready for a Windows DIB section: `height` rows from the top, `width` pixels each,
/// 4 bytes a pixel in the order B, G, R, A, alpha not premultiplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// The most the PNG decoder may allocate, and the largest decoded picture: a 512 x 384 picture
/// needs 768 KiB.
const DECODE_LIMIT: usize = 8 << 20;

/// The largest size with the picture's proportions whose longer side is at most `cx`, never
/// larger than the picture; `None` when any argument is 0.
pub fn fit(width: u32, height: u32, cx: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || cx == 0 {
        return None;
    }
    if width <= cx && height <= cx {
        return Some((width, height));
    }
    let (w, h, c) = (u64::from(width), u64::from(height), u64::from(cx));
    let (to_w, to_h) = if w >= h { (c, ((h * c + w / 2) / w).max(1)) } else { (((w * c + h / 2) / h).max(1), c) };
    Some((u32::try_from(to_w).ok()?, u32::try_from(to_h).ok()?))
}

/// The PNG `png`, which must be `width` x `height`, decoded and fitted into `cx`.
pub fn thumbnail_from_png(png: &[u8], width: u32, height: u32, cx: u32) -> Option<Bitmap> {
    let (to_width, to_height) = fit(width, height, cx)?;
    let rgba = decode(png, width, height)?;
    let mut pixels = if (to_width, to_height) == (width, height) { rgba } else { scale(&rgba, width, height, to_width, to_height)? };
    for pixel in pixels.as_chunks_mut::<4>().0 {
        // A fully transparent pixel is all zeros, whatever colour the picture gave it.
        let [r, g, b, a] = *pixel;
        *pixel = if a == 0 { [0; 4] } else { [b, g, r, a] };
    }
    Some(Bitmap { width: to_width, height: to_height, bgra: pixels })
}

/// `png`, which must be `width` x `height`, as straight RGBA rows of 8 bits a channel: every
/// colour type and bit depth is expanded to that. `None` for anything else or any error.
fn decode(png: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new_with_limits(std::io::Cursor::new(png), png::Limits { bytes: DECODE_LIMIT });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    if (reader.info().width, reader.info().height) != (width, height) {
        return None;
    }
    let size = reader.output_buffer_size().filter(|&n| n <= DECODE_LIMIT)?;
    let mut buffer = vec![0_u8; size];
    let info = reader.next_frame(&mut buffer).ok()?;
    let data = buffer.get(..info.buffer_size())?;
    // The transformations leave RGBA or grey with alpha, 8 bits a channel; anything else, or a
    // frame of another size, fails the length check below.
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|&[gray, alpha]| [gray, gray, gray, alpha]).collect(),
        _ => data.to_vec(),
    };
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    (rgba.len() == pixels.checked_mul(4)?).then_some(rgba)
}

/// Straight RGBA rows `width` x `height` averaged down to `to_width` x `to_height`, no larger.
fn scale(rgba: &[u8], width: u32, height: u32, to_width: u32, to_height: u32) -> Option<Vec<u8>> {
    let (sw, sh, dw, dh) = (width as usize, height as usize, to_width as usize, to_height as usize);
    if dw == 0 || dh == 0 || dw > sw || dh > sh || rgba.len() != sw.checked_mul(sh)?.checked_mul(4)? {
        return None;
    }
    let (fx, fy) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
    let mut out = Vec::with_capacity(dw * dh * 4);
    for dy in 0..dh {
        let (y0, y1) = (dy as f64 * fy, (dy + 1) as f64 * fy);
        for dx in 0..dw {
            let (x0, x1) = (dx as f64 * fx, (dx + 1) as f64 * fx);
            // Red, green and blue premultiplied by alpha, then alpha, each times the area covered.
            let mut sum = [0.0_f64; 4];
            let mut area = 0.0_f64;
            for sy in (y0.floor() as usize)..(y1.ceil() as usize).min(sh) {
                let wy = (y1.min((sy + 1) as f64) - y0.max(sy as f64)).max(0.0);
                for sx in (x0.floor() as usize)..(x1.ceil() as usize).min(sw) {
                    let wx = (x1.min((sx + 1) as f64) - x0.max(sx as f64)).max(0.0);
                    let at = (sy * sw + sx) * 4;
                    let &[r, g, b, a] = <&[u8; 4]>::try_from(rgba.get(at..at + 4)?).ok()?;
                    let weight = wx * wy * f64::from(a) / 255.0;
                    sum[0] += f64::from(r) * weight;
                    sum[1] += f64::from(g) * weight;
                    sum[2] += f64::from(b) * weight;
                    sum[3] += weight;
                    area += wx * wy;
                }
            }
            if sum[3] > 0.0 && area > 0.0 {
                let channel = |v: f64| (v / sum[3]).round().clamp(0.0, 255.0) as u8;
                let alpha = (sum[3] / area * 255.0).round().clamp(0.0, 255.0) as u8;
                out.extend_from_slice(&[channel(sum[0]), channel(sum[1]), channel(sum[2]), alpha]);
            } else {
                out.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "bitmap_tests.rs"]
mod tests;
