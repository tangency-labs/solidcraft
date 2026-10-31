use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbaImage};
use solidcraft_geom::Vec2;

use super::*;

fn encoded(width: u32, height: u32, pixels: &[[u8; 4]], format: ImageFormat) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, pixels.concat()).unwrap());
    let image = if format == ImageFormat::Jpeg { DynamicImage::ImageRgb8(image.to_rgb8()) } else { image };
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, format).unwrap();
    out.into_inner()
}

fn png(width: u32, height: u32, pixels: &[[u8; 4]]) -> SheetImage {
    SheetImage::decode(&encoded(width, height, pixels, ImageFormat::Png)).unwrap()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Aabb2 {
    Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) }
}

fn all(grid: &PixelGrid) -> GridWindow {
    GridWindow { x0: 0, y0: 0, x1: grid.width, y1: grid.height }
}

const QUAD: [[u8; 4]; 4] = [[200, 30, 90, 255], [0, 255, 0, 255], [10, 20, 250, 255], [255, 255, 255, 255]];

/// One texel a pixel gives the image back exactly; one pixel over four texels is their average
/// in linear light.
#[test]
fn pixels_over_whole_texels_are_the_texels_and_a_shrink_averages_in_linear_light() {
    let image = png(2, 2, &QUAD);
    assert_eq!(image.size(), [2, 2]);
    let one_each = PixelGrid::new(0.0, 0.0, 2.0, 2.0, 25.4).unwrap();
    assert_eq!(image.rows(rect(0.0, 0.0, 2.0, 2.0), &one_each, all(&one_each)).unwrap(), QUAD.concat());
    let one = PixelGrid::new(0.0, 0.0, 2.0, 2.0, 12.7).unwrap();
    let mean = |c: usize| encode8(QUAD.iter().map(|p| decode8(p[c])).sum::<f64>() / 4.0);
    assert_eq!(image.rows(rect(0.0, 0.0, 2.0, 2.0), &one, all(&one)).unwrap(), vec![mean(0), mean(1), mean(2), 255]);
}

/// Asked a row or a pixel at a time, on a sheet's grid at an odd dpi with the rectangle off the
/// pixel edges, the pixels are those of the whole window: a writer's bands agree with the whole.
#[test]
fn bands_and_single_pixels_agree_with_the_whole_window() {
    let pixels: Vec<[u8; 4]> = (0u32..12).map(|i| [(i * 37 % 256) as u8, (i * 91 % 256) as u8, (i * 53 % 256) as u8, (60 + i * 16) as u8]).collect();
    let image = png(4, 3, &pixels);
    let grid = PixelGrid::sheet(Vec2::new(10.0, 8.0), 96.0).unwrap();
    let on = rect(1.3, 2.1, 4.0, 3.9);
    let window = grid.window(on).unwrap();
    let whole = image.rows(on, &grid, window).unwrap();
    assert_eq!(whole.len(), ((window.x1 - window.x0) * (window.y1 - window.y0) * 4) as usize);
    let by_rows: Vec<u8> = (window.y0..window.y1).flat_map(|y| image.rows(on, &grid, GridWindow { y0: y, y1: y + 1, ..window }).unwrap()).collect();
    let by_pixels: Vec<u8> = (window.y0..window.y1)
        .flat_map(|y| (window.x0..window.x1).map(move |x| GridWindow { x0: x, y0: y, x1: x + 1, y1: y + 1 }))
        .flat_map(|w| image.rows(on, &grid, w).unwrap())
        .collect();
    assert_eq!((&by_rows, &by_pixels), (&whole, &whole));
}

/// One red texel stretched over 5 × 3 mm at 254 dpi paints every pixel of its window, those it
/// only partly covers included; pixels of the grid wholly outside its rectangle stay transparent.
#[test]
fn an_upscale_fills_its_window_and_nothing_outside_it() {
    let red = png(1, 1, &[[255, 0, 0, 255]]);
    let grid = PixelGrid::new(0.0, 0.0, 5.05, 3.0, 254.0).unwrap();
    let rows = red.rows(rect(0.0, 0.0, 5.05, 3.0), &grid, all(&grid)).unwrap();
    assert!(rows.chunks(4).all(|p| p == [255, 0, 0, 255]), "{} px", rows.len() / 4);
    let sheet = PixelGrid::new(0.0, 0.0, 4.0, 4.0, 25.4).unwrap();
    let rows = red.rows(rect(2.0, 1.0, 3.0, 2.0), &sheet, all(&sheet)).unwrap();
    let painted: Vec<usize> = rows.chunks(4).enumerate().filter(|(_, p)| *p != [0; 4]).map(|(i, _)| i).collect();
    // Row 2 from the top (y 1 to 2 mm), column 2.
    assert_eq!(painted, vec![2 * 4 + 2]);
    // A rectangle past a pixel's edge only by rounding doesn't paint that pixel.
    let rows = red.rows(rect(0.0, 0.0, 2.0 + 2e-12, 4.0), &sheet, all(&sheet)).unwrap();
    assert_eq!(rows.chunks(4).take(4).map(|p| p[3]).collect::<Vec<_>>(), [255, 255, 0, 0]);
}

/// A pixel across a texel edge weighs each texel by how much of it the pixel covers, wherever it
/// starts and ends.
#[test]
fn a_pixel_across_texels_weighs_them_by_what_it_covers() {
    let texels = [[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]];
    let image = png(3, 1, &texels);
    let mean =
        |a: [u8; 4], b: [u8; 4]| [0, 1, 2].map(|c| encode8((decode8(a[c]) + decode8(b[c])) / 2.0)).into_iter().chain([255]).collect::<Vec<u8>>();
    // Pixels from 0.5 to 1.5 mm and 1.5 to 2.5 mm, over texels a millimetre wide.
    let grid = PixelGrid::new(0.5, 0.0, 2.0, 1.0, 25.4).unwrap();
    let rows = image.rows(rect(0.0, 0.0, 3.0, 1.0), &grid, all(&grid)).unwrap();
    assert_eq!(rows, [mean(texels[0], texels[1]), mean(texels[1], texels[2])].concat());
}

/// Half-transparent white beside transparent black, shrunk to one pixel: white at a quarter
/// alpha. Averaged straight it would be a dark grey fringe.
#[test]
fn a_shrink_averages_with_alpha_premultiplied() {
    let image = png(2, 1, &[[255, 255, 255, 128], [0, 0, 0, 0]]);
    let grid = PixelGrid::new(0.0, 0.0, 2.0, 1.0, 12.7).unwrap();
    assert_eq!(image.rows(rect(0.0, 0.0, 2.0, 1.0), &grid, all(&grid)).unwrap(), vec![255, 255, 255, 64]);
    // Half-transparent red beside opaque blue: the red counts for its alpha, so blue leads.
    let (image, a) = (png(2, 1, &[[255, 0, 0, 128], [0, 0, 255, 255]]), 128.0 / 255.0);
    let expected = vec![encode8(a / (a + 1.0)), 0, encode8(1.0 / (a + 1.0)), ((a + 1.0) / 2.0 * 255.0_f64).round() as u8];
    assert_eq!(image.rows(rect(0.0, 0.0, 2.0, 1.0), &grid, all(&grid)).unwrap(), expected);
    let clear = png(1, 1, &[[9, 9, 9, 0]]);
    assert_eq!(clear.rows(rect(0.0, 0.0, 2.0, 1.0), &grid, all(&grid)).unwrap(), vec![0; 4]);
}

/// A window off its grid, an empty or endless rectangle, are refused.
#[test]
fn a_window_off_its_grid_or_a_rectangle_of_no_size_is_refused() {
    let image = png(1, 1, &[[1, 2, 3, 255]]);
    let grid = PixelGrid::new(0.0, 0.0, 2.0, 2.0, 25.4).unwrap();
    let on = rect(0.0, 0.0, 2.0, 2.0);
    for window in [GridWindow { x0: 0, y0: 0, x1: 3, y1: 1 }, GridWindow { x0: 1, y0: 0, x1: 1, y1: 1 }] {
        assert_eq!(image.rows(on, &grid, window).unwrap_err(), "a window outside its grid");
    }
    for bad in [rect(0.0, 0.0, 0.0, 2.0), rect(0.0, f64::NAN, 2.0, 2.0), rect(0.0, 0.0, f64::INFINITY, 2.0)] {
        assert!(image.rows(bad, &grid, all(&grid)).unwrap_err().contains("finite rectangle"));
    }
}

/// A header declaring 9000 × 9000 is refused by its size, before the pixels are decoded.
#[test]
fn an_image_over_64_megapixels_is_refused_by_its_header() {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 9000, 9000);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::One);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&vec![0; 9000 * 1125]).unwrap();
    writer.finish().unwrap();
    let e = SheetImage::decode(&out).unwrap_err();
    assert_eq!(e, "an image of 9000 × 9000 pixels: at most 64 megapixels");
}

/// 36 megapixels of 16-bit RGBA are under the pixel cap, but decoding them would take 288 MiB: the
/// working memory is capped too, before anything is decoded.
#[test]
fn an_image_that_decodes_past_the_memory_cap_is_refused() {
    use std::io::Write;
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 6000, 6000);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut header = encoder.write_header().unwrap();
    let mut writer = header.stream_writer().unwrap();
    (0..6000).for_each(|_| writer.write_all(&[0; 6000 * 8]).unwrap());
    writer.finish().unwrap();
    drop(header);
    let e = SheetImage::decode(&out).unwrap_err();
    assert_eq!(e, "an image of 6000 × 6000 pixels takes over 256 MB to decode");
}

/// Empty bytes, another format, a cut-off file and damaged pixel data are errors, never panics,
/// and so is every one of a few thousand mutations of a PNG and a JPEG; what decodes resamples.
#[test]
fn damaged_files_are_errors_never_panics() {
    let pixels: Vec<[u8; 4]> = (0u8..20).map(|i| [i * 12, 255 - i * 9, i * 5, 255]).collect();
    let png_bytes = encoded(5, 4, &pixels, ImageFormat::Png);
    let jpeg_bytes = encoded(5, 4, &pixels, ImageFormat::Jpeg);
    SheetImage::decode(&jpeg_bytes).unwrap();
    let mut flipped = png_bytes.clone();
    let idat = flipped.windows(4).position(|w| w == b"IDAT").unwrap();
    flipped[idat + 6] ^= 0xff;
    for bad in [
        &b""[..],
        b"GIF89a\x01\x00\x01\x00",
        &png_bytes[..png_bytes.len() - 1],
        &jpeg_bytes[..jpeg_bytes.len() / 2],
        &jpeg_bytes[..jpeg_bytes.len() - 2],
        &flipped,
    ] {
        assert!(SheetImage::decode(bad).is_err());
    }
    let grid = PixelGrid::new(0.0, 0.0, 3.0, 2.0, 50.0).unwrap();
    let mut seed = 0x5eed_u64;
    for case in 0..4000 {
        let mut bytes = if case % 2 == 0 { png_bytes.clone() } else { jpeg_bytes.clone() };
        for _ in 0..1 + case % 4 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let at = (seed >> 33) as usize % bytes.len();
            bytes[at] = (seed >> 13) as u8;
        }
        if let Ok(image) = SheetImage::decode(&bytes) {
            assert_eq!(image.rows(rect(0.0, 0.0, 3.0, 2.0), &grid, all(&grid)).unwrap().len(), (grid.pixels() * 4) as usize);
        }
    }
}

/// An EXIF orientation of 6 (turn a quarter clockwise) stands the image up: red-then-blue across
/// becomes red above blue.
#[test]
fn an_exif_orientation_is_applied() {
    // Big-endian TIFF header, one IFD entry: Orientation (0x0112), SHORT, 1 value, 6.
    const EXIF: [u8; 26] = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0];
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_chunk(png::chunk::ChunkType(*b"eXIf"), &EXIF).unwrap();
    writer.write_image_data(&[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    writer.finish().unwrap();
    let image = SheetImage::decode(&out).unwrap();
    assert_eq!(image.size(), [1, 2]);
    let grid = PixelGrid::new(0.0, 0.0, 1.0, 2.0, 25.4).unwrap();
    assert_eq!(image.rows(rect(0.0, 0.0, 1.0, 2.0), &grid, all(&grid)).unwrap(), [[255, 0, 0, 255], [0, 0, 255, 255]].concat());
}
