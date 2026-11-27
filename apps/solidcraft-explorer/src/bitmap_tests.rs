use png::{BitDepth, ColorType};

use super::*;

/// A PNG of `width` x `height` with `data` as its pixels, and a private `scSh` chunk as SolidCraft
/// writes one. `palette` is the palette and its transparency, for an indexed PNG.
fn encode(width: u32, height: u32, color: ColorType, depth: BitDepth, data: &[u8], palette: Option<(&[u8], &[u8])>) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some((colors, alphas)) = palette {
            encoder.set_palette(colors.to_vec());
            encoder.set_trns(alphas.to_vec());
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_chunk(png::chunk::ChunkType(*b"scSh"), &[0; 8]).unwrap();
        writer.write_image_data(data).unwrap();
    }
    out
}

/// `width` x `height` RGBA pixels: the top row green, the rest red, all opaque.
fn green_top(width: u32, height: u32) -> Vec<u8> {
    (0..height).flat_map(|y| (0..width).flat_map(move |_| if y == 0 { [0, 255, 0, 255] } else { [255, 0, 0, 255] })).collect()
}

/// The 4 x 4 picture of the scaling tests: red where `(x < 2) == (y < 2)`, blue elsewhere, all opaque.
fn quadrants() -> Vec<u8> {
    (0..4).flat_map(|y| (0..4).flat_map(move |x| if (x < 2) == (y < 2) { [255, 0, 0, 255] } else { [0, 0, 255, 255] })).collect()
}

#[test]
fn a_picture_fits_its_box_with_its_proportions() {
    assert_eq!(fit(512, 384, 256), Some((256, 192)));
    assert_eq!(fit(512, 384, 1024), Some((512, 384)));
    assert_eq!(fit(512, 384, 512), Some((512, 384)));
    assert_eq!(fit(240, 150, 96), Some((96, 60)));
    assert_eq!(fit(240, 150, 32), Some((32, 20)));
    assert_eq!(fit(150, 240, 32), Some((20, 32)));
    assert_eq!(fit(1, 4096, 16), Some((1, 16)));
    assert_eq!(fit(0, 150, 32), None);
    assert_eq!(fit(240, 0, 32), None);
    assert_eq!(fit(240, 150, 0), None);
}

#[test]
fn every_colour_type_decodes_to_straight_rgba() {
    assert_eq!(decode(&encode(2, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4, 5, 6, 7, 8], None), 2, 1), Some(vec![1, 2, 3, 4, 5, 6, 7, 8]));
    assert_eq!(decode(&encode(1, 1, ColorType::Rgb, BitDepth::Eight, &[9, 8, 7], None), 1, 1), Some(vec![9, 8, 7, 255]));
    assert_eq!(decode(&encode(1, 1, ColorType::Grayscale, BitDepth::Eight, &[77], None), 1, 1), Some(vec![77, 77, 77, 255]));
    assert_eq!(decode(&encode(1, 1, ColorType::GrayscaleAlpha, BitDepth::Eight, &[77, 10], None), 1, 1), Some(vec![77, 77, 77, 10]));
    let palette: (&[u8], &[u8]) = (&[10, 20, 30, 40, 50, 60], &[0, 255]);
    assert_eq!(decode(&encode(2, 1, ColorType::Indexed, BitDepth::Eight, &[0, 1], Some(palette)), 2, 1), Some(vec![10, 20, 30, 0, 40, 50, 60, 255]));
    assert_eq!(
        decode(&encode(1, 1, ColorType::Rgba, BitDepth::Sixteen, &[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0], None), 1, 1),
        Some(vec![0x12, 0x56, 0x9A, 0xDE])
    );
}

#[test]
fn a_picture_of_another_size_or_a_broken_one_is_a_miss() {
    let rgba = encode(2, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4, 5, 6, 7, 8], None);
    assert_eq!(decode(&rgba, 1, 2), None);
    let mut broken = rgba.clone();
    let idat = broken.windows(4).position(|w| w == b"IDAT").unwrap();
    let len = u32::from_be_bytes(broken[idat - 4..idat].try_into().unwrap()) as usize;
    broken[idat + 4 + len - 1] ^= 0xFF;
    assert_eq!(decode(&broken, 2, 1), None);
    assert_eq!(decode(&rgba[..rgba.len() - 20], 2, 1), None);
    assert_eq!(decode(b"", 2, 1), None);
    assert_eq!(decode(b"GIF89a", 2, 1), None);
}

#[test]
fn scaling_averages_in_premultiplied_alpha() {
    assert_eq!(scale(&[0, 0, 0, 255, 90, 90, 90, 255, 180, 180, 180, 255], 3, 1, 2, 1), Some(vec![30, 30, 30, 255, 150, 150, 150, 255]));
    // Straight averaging would give a dark [128, 0, 0, 128].
    assert_eq!(scale(&[255, 0, 0, 255, 0, 0, 0, 0], 2, 1, 1, 1), Some(vec![255, 0, 0, 128]));
    assert_eq!(scale(&[0; 8], 2, 1, 1, 1), Some(vec![0, 0, 0, 0]));
    assert_eq!(
        scale(&quadrants(), 4, 4, 2, 2),
        Some(vec![255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255])
    );
}

#[test]
fn scaling_never_enlarges_and_checks_its_input() {
    let quad = quadrants();
    assert_eq!(scale(&quad, 4, 4, 5, 4), None);
    assert_eq!(scale(&quad, 4, 4, 0, 1), None);
    assert_eq!(scale(&quad[..60], 4, 4, 2, 2), None);
}

#[test]
fn a_thumbnail_is_bgra_rows_from_the_top() {
    let png = encode(4, 4, ColorType::Rgba, BitDepth::Eight, &green_top(4, 4), None);
    let b = thumbnail_from_png(&png, 4, 4, 4).unwrap();
    assert_eq!((b.width, b.height), (4, 4));
    assert_eq!(b.bgra[0..4], [0, 255, 0, 255]);
    assert_eq!(b.bgra[48..52], [0, 0, 255, 255]);
    let b = thumbnail_from_png(&png, 4, 4, 2).unwrap();
    assert_eq!((b.width, b.height), (2, 2));
    assert_eq!(b.bgra[0..4], [0, 128, 128, 255]);
    assert_eq!(b.bgra[8..12], [0, 0, 255, 255]);
}

#[test]
fn a_thumbnail_needs_a_size_and_a_matching_picture() {
    let png = encode(4, 4, ColorType::Rgba, BitDepth::Eight, &green_top(4, 4), None);
    assert_eq!(thumbnail_from_png(&png, 4, 4, 0), None);
    assert_eq!(thumbnail_from_png(&png, 4, 3, 4), None);
}

#[test]
fn the_largest_saved_picture_scales_to_every_cache_size() {
    let png = encode(512, 384, ColorType::Rgba, BitDepth::Eight, &green_top(512, 384), None);
    for (cx, expected) in [(1024, (512, 384)), (256, (256, 192)), (96, (96, 72)), (32, (32, 24))] {
        let b = thumbnail_from_png(&png, 512, 384, cx).unwrap();
        assert_eq!((b.width, b.height), expected);
        assert_eq!(b.bgra.len(), (b.width * b.height * 4) as usize);
    }
}

#[test]
fn a_private_chunk_does_not_stop_decoding() {
    assert_eq!(decode(&encode(1, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4], None), 1, 1), Some(vec![1, 2, 3, 4]));
}
