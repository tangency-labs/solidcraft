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
    let _ = (width, height, cx);
    None
}

/// The PNG `png`, which must be `width` x `height`, decoded and fitted into `cx`.
pub fn thumbnail_from_png(png: &[u8], width: u32, height: u32, cx: u32) -> Option<Bitmap> {
    let _ = (png, width, height, cx);
    None
}

/// `png`, which must be `width` x `height`, as straight RGBA rows of 8 bits a channel: every
/// colour type and bit depth is expanded to that. `None` for anything else or any error.
fn decode(png: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let _ = (png, width, height, DECODE_LIMIT);
    None
}

/// Straight RGBA rows `width` x `height` averaged down to `to_width` x `to_height`, no larger.
fn scale(rgba: &[u8], width: u32, height: u32, to_width: u32, to_height: u32) -> Option<Vec<u8>> {
    let _ = (rgba, width, height, to_width, to_height);
    None
}

#[cfg(test)]
#[path = "bitmap_tests.rs"]
mod tests;
