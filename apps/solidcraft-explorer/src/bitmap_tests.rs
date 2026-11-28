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
    // Halves round up: 7 x 5 / 10 is 3.5, and 150 x 100 / 240 is 62.5.
    assert_eq!(fit(10, 7, 5), Some((5, 4)));
    assert_eq!(fit(7, 10, 5), Some((4, 5)));
    assert_eq!(fit(240, 150, 100), Some((100, 63)));
    assert_eq!(fit(1, 100, 99), Some((1, 99)));
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
    assert_eq!(scale(&quadrants(), 4, 4, 2, 2), Some(vec![255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255]));
}

#[test]
fn scaling_never_enlarges_and_checks_its_input() {
    let quad = quadrants();
    assert_eq!(scale(&quad, 4, 4, 5, 4), None);
    assert_eq!(scale(&quad, 4, 4, 4, 5), None);
    let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
    assert_eq!(scale(&quad, 4, 4, 4, 2), Some([red, red, blue, blue, blue, blue, red, red].concat()), "one side kept");
    assert_eq!(scale(&[quad.as_slice(), &[0; 4]].concat(), 4, 4, 2, 2), None, "a pixel too many");
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

#[test]
fn a_fully_transparent_pixel_is_all_zeros() {
    // Kept at its size: a transparent pixel's colour is dropped.
    let png = encode(2, 1, ColorType::Rgba, BitDepth::Eight, &[200, 100, 50, 0, 1, 2, 3, 255], None);
    assert_eq!(thumbnail_from_png(&png, 2, 1, 2).unwrap().bgra, vec![0, 0, 0, 0, 3, 2, 1, 255]);
    let palette: (&[u8], &[u8]) = (&[10, 20, 30, 40, 50, 60], &[0, 255]);
    let indexed = encode(2, 1, ColorType::Indexed, BitDepth::Eight, &[0, 1], Some(palette));
    assert_eq!(thumbnail_from_png(&indexed, 2, 1, 2).unwrap().bgra, vec![0, 0, 0, 0, 60, 50, 40, 255]);
    // Scaled: a nearly transparent pixel among transparent ones averages to alpha 0.
    let faint = encode(2, 2, ColorType::Rgba, BitDepth::Eight, &[255, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], None);
    assert_eq!(thumbnail_from_png(&faint, 2, 2, 1).unwrap().bgra, vec![0, 0, 0, 0]);
}

/// The CRC-32 a PNG chunk carries (ISO 3309, as the PNG specification gives it).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A PNG written by hand: 8-bit RGBA, `width` x `height`, `interlace` 0 or 1 (Adam7), with
/// `scanlines` (filter bytes included) stored uncompressed in one zlib stream.
fn raw_png(width: u32, height: u32, interlace: u8, scanlines: &[u8]) -> Vec<u8> {
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, interlace]);
    let (mut a, mut b) = (1_u32, 0_u32);
    for &byte in scanlines {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend_from_slice(&(scanlines.len() as u16).to_le_bytes());
    zlib.extend_from_slice(&(!(scanlines.len() as u16)).to_le_bytes());
    zlib.extend_from_slice(scanlines);
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn an_interlaced_picture_decodes_as_a_plain_one() {
    // 3 x 3 pixels, each its own colour: pixel (x, y) is [x * 80, y * 80, 7, 255].
    let p = |x: u8, y: u8| [x * 80, y * 80, 7, 255];
    let plain: Vec<u8> = (0..3).flat_map(|y| std::iter::once(0).chain((0..3).flat_map(move |x| p(x, y)))).collect();
    let rgba: Vec<u8> = (0..3).flat_map(|y| (0..3).flat_map(move |x| p(x, y))).collect();
    assert_eq!(decode(&raw_png(3, 3, 0, &plain), 3, 3), Some(rgba.clone()));
    // Adam7 on 3 x 3: pass 1 (0,0); pass 4 (2,0); pass 5 (0,2) (2,2); pass 6 (1,0), then (1,2);
    // pass 7 the middle row. Passes 2 and 3 are empty and have no scanlines.
    let mut adam7 = Vec::new();
    for row in [vec![p(0, 0)], vec![p(2, 0)], vec![p(0, 2), p(2, 2)], vec![p(1, 0)], vec![p(1, 2)], vec![p(0, 1), p(1, 1), p(2, 1)]] {
        adam7.push(0);
        adam7.extend(row.into_iter().flatten());
    }
    assert_eq!(decode(&raw_png(3, 3, 1, &adam7), 3, 3), Some(rgba));
}

#[test]
fn picture_data_that_is_short_or_long_is_never_misread() {
    let row = [0, 10, 20, 30, 255];
    assert_eq!(decode(&raw_png(1, 1, 0, &row), 1, 1), Some(vec![10, 20, 30, 255]));
    assert_eq!(decode(&raw_png(1, 2, 0, &row), 1, 2), None, "one row of two");
    assert_eq!(decode(&raw_png(1, 1, 0, &row[..3]), 1, 1), None, "half a row");
    let long = [row, [0, 99, 99, 99, 99]].concat();
    assert_eq!(decode(&raw_png(1, 1, 0, &long), 1, 1), Some(vec![10, 20, 30, 255]), "data past the last row is ignored");
}

#[test]
fn low_and_high_bit_depths_decode_to_eight_bits() {
    assert_eq!(decode(&encode(2, 1, ColorType::Grayscale, BitDepth::One, &[0b1000_0000], None), 2, 1), Some(vec![255, 255, 255, 255, 0, 0, 0, 255]));
    assert_eq!(
        decode(&encode(2, 1, ColorType::Grayscale, BitDepth::Two, &[0b1101_0000], None), 2, 1),
        Some(vec![255, 255, 255, 255, 85, 85, 85, 255])
    );
    assert_eq!(decode(&encode(2, 1, ColorType::Grayscale, BitDepth::Four, &[0x0F], None), 2, 1), Some(vec![0, 0, 0, 255, 255, 255, 255, 255]));
    let palette: (&[u8], &[u8]) = (&[10, 20, 30, 40, 50, 60], &[128]);
    assert_eq!(decode(&encode(2, 1, ColorType::Indexed, BitDepth::Four, &[0x01], Some(palette)), 2, 1), Some(vec![10, 20, 30, 128, 40, 50, 60, 255]));
    assert_eq!(
        decode(&encode(1, 1, ColorType::GrayscaleAlpha, BitDepth::Sixteen, &[0x12, 0x34, 0x56, 0x78], None), 1, 1),
        Some(vec![0x12, 0x12, 0x12, 0x56])
    );
    assert_eq!(decode(&encode(1, 1, ColorType::Rgb, BitDepth::Sixteen, &[1, 2, 3, 4, 5, 6], None), 1, 1), Some(vec![1, 3, 5, 255]));
}

#[test]
fn an_animated_picture_gives_its_first_frame() {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[1, 2, 3, 255]).unwrap();
        writer.write_image_data(&[9, 9, 9, 255]).unwrap();
    }
    assert_eq!(thumbnail_from_png(&out, 1, 1, 256).map(|b| b.bgra), Some(vec![3, 2, 1, 255]));
}

#[test]
fn the_decoder_stops_at_its_allocation_limit() {
    // 2048 x 1024 RGBA is exactly 8 MiB decoded; one more row is past the limit.
    let at_limit = encode(2048, 1024, ColorType::Rgba, BitDepth::Eight, &vec![0; 2048 * 1024 * 4], None);
    assert_eq!(decode(&at_limit, 2048, 1024).map(|rgba| rgba.len()), Some(DECODE_LIMIT));
    let over = encode(2048, 1025, ColorType::Rgba, BitDepth::Eight, &vec![0; 2048 * 1025 * 4], None);
    assert_eq!(decode(&over, 2048, 1025), None);
}

#[test]
fn scaling_weights_each_pixel_by_the_area_it_covers() {
    // 3 x 3 to 2 x 2: each output pixel covers 1.5 x 1.5 source pixels, so the corner pixel counts
    // 1, the edge pixels 0.5 and the centre 0.25. The expected values come from the formula of
    // SPEC section 4.3 worked in exact fractions.
    let source = [
        [0, 0, 0, 255],
        [0, 0, 0, 255],
        [200, 0, 0, 255],
        [255, 0, 0, 255],
        [0, 0, 0, 255],
        [0, 90, 0, 51],
        [0, 0, 0, 255],
        [30, 60, 90, 255],
        [0, 0, 0, 0],
    ]
    .concat();
    assert_eq!(scale(&source, 3, 3, 2, 2), Some(vec![57, 0, 0, 255, 108, 5, 0, 210, 63, 13, 20, 255, 18, 46, 53, 96]));
}
