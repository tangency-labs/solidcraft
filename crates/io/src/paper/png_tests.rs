use std::f64::consts::{FRAC_PI_2, TAU};

use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use super::*;

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

/// Anticlockwise from `min`.
fn rectangle(min: Vec2, max: Vec2) -> Loop2 {
    Loop2::polygon(&[min, p(max.x, min.y), max, p(min.x, max.y)])
}

fn scene(size: Vec2, items: Vec<Item>) -> Scene {
    Scene::new(size, vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }])
}

/// A 20 × 10 mm sheet: a 1 mm line along y = 5 from x = 2 to 18, and a 6 × 3 mm fill at (2, 1)
/// with a 2 × 1 mm hole at (4, 2). At 254 dpi a millimetre is 10 pixels.
fn small() -> Scene {
    scene(
        p(20.0, 10.0),
        vec![
            Item::Stroke { path: vec![line(p(2.0, 5.0), p(18.0, 5.0))], closed: false, width_mm: 1.0 },
            Item::Fill { region: Region2 { outer: rectangle(p(2.0, 1.0), p(8.0, 4.0)), holes: vec![rectangle(p(4.0, 2.0), p(6.0, 3.0))] } },
        ],
    )
}

struct Image {
    info: png::Info<'static>,
    /// One byte a sample, rows top down: 1-bit images unpacked to 0 or 255.
    samples: Vec<u8>,
}

impl Image {
    fn channels(&self) -> usize {
        self.info.color_type.samples()
    }

    /// The samples of the pixel at column `x`, row `y` from the top.
    fn at(&self, x: usize, y: usize) -> &[u8] {
        let c = self.channels();
        let i = (y * self.info.width as usize + x) * c;
        &self.samples[i..i + c]
    }
}

fn bytes(scene: &Scene, options: PngOptions) -> (Vec<u8>, PngReport) {
    let mut out = Vec::new();
    let report = paper_png(&scene.prepare().unwrap(), options, &mut out).unwrap();
    (out, report)
}

fn read(bytes: &[u8]) -> Image {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes.to_vec()));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut samples = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut samples).unwrap();
    let info = reader.info().clone();
    if info.bit_depth == png::BitDepth::One {
        // EXPAND makes a 1-bit grey sample 0 or 255 a byte.
        assert!(samples.iter().all(|s| *s == 0 || *s == 255));
    }
    Image { info, samples }
}

fn image(scene: &Scene, options: PngOptions) -> Image {
    read(&bytes(scene, options).0)
}

const AT_254: PngOptions = PngOptions { dpi: 254.0, colour: PngColour::Grey };

/// A sheet is its size at the dpi, rounded, and says its dpi, so it prints at the sheet's size:
/// 300 dpi greyscale unless asked, sRGB.
#[test]
fn a_sheet_is_its_size_at_the_dpi_and_records_it() {
    let a4 = scene(p(210.0, 297.0), vec![]);
    assert_eq!(PngOptions::default(), PngOptions { dpi: 300.0, colour: PngColour::Grey });
    let (data, report) = bytes(&a4, PngOptions::default());
    assert_eq!(report.pixels, [2480, 3508]);
    let image = read(&data);
    assert_eq!((image.info.width, image.info.height), (2480, 3508));
    assert_eq!((image.info.color_type, image.info.bit_depth), (png::ColorType::Grayscale, png::BitDepth::Eight));
    let dims = image.info.pixel_dims.unwrap();
    assert_eq!((dims.xppu, dims.yppu, dims.unit), (11811, 11811, png::Unit::Meter));
    assert!(image.info.srgb.is_some());
    assert!(image.samples.iter().all(|s| *s == 255), "blank paper is white");
    assert_eq!(bytes(&a4, PngOptions { dpi: 25.0, ..PngOptions::default() }).1.pixels, [207, 292]);
}

/// The line and the fill land on their pixels: the line 10 rows deep round row 50 with butt ends,
/// the fill's ring black and its hole white.
#[test]
fn lines_and_fills_land_on_their_pixels() {
    let image = image(&small(), AT_254);
    assert_eq!((image.info.width, image.info.height), (200, 100));
    let grey = |x, y| image.at(x, y)[0];
    for (x, y, value, what) in [
        (100, 45, 0, "the line's top row"),
        (100, 54, 0, "its bottom row"),
        (100, 44, 255, "above it"),
        (100, 55, 255, "below it"),
        (20, 50, 0, "its first column"),
        (19, 50, 255, "before its butt end"),
        (179, 50, 0, "its last column"),
        (180, 50, 255, "after it"),
        (30, 75, 0, "the fill's ring"),
        (50, 75, 255, "the fill's hole"),
        (90, 75, 255, "beside the fill"),
    ] {
        assert_eq!(grey(x, y), value, "{what}, ({x}, {y})");
    }
}

/// Each colour: its PNG type, black ink and white (or no) paper.
#[test]
fn each_colour_has_its_type_ink_and_paper() {
    for (colour, kind, depth, ink, paper) in [
        (PngColour::Grey, png::ColorType::Grayscale, png::BitDepth::Eight, &[0][..], &[255][..]),
        (PngColour::BlackWhite, png::ColorType::Grayscale, png::BitDepth::One, &[0], &[255]),
        (PngColour::Rgb, png::ColorType::Rgb, png::BitDepth::Eight, &[0, 0, 0], &[255, 255, 255]),
        (PngColour::Transparent, png::ColorType::Rgba, png::BitDepth::Eight, &[0, 0, 0, 255], &[0, 0, 0, 0]),
    ] {
        let image = image(&small(), PngOptions { dpi: 254.0, colour });
        assert_eq!((image.info.color_type, image.info.bit_depth), (kind, depth), "{colour:?}");
        assert_eq!(image.at(100, 50), ink, "{colour:?}: the line");
        // Either side of its butt end, bits 3 and 4 of a 1-bit row's third byte.
        assert_eq!((image.at(19, 50), image.at(20, 50)), (paper, ink), "{colour:?}: the line's start");
        assert_eq!(image.at(100, 20), paper, "{colour:?}: the paper");
    }
}

/// A line far thinner than a pixel is a light grey in greyscale, and a full black pixel in black
/// and white, so it doesn't drop out, even between two rows' centres; edges are anti-aliased only
/// where there is grey, and black and white inks a pixel when its centre is inside.
#[test]
fn a_hairline_stays_a_pixel_wide_in_black_and_white() {
    // 50.25 rows from the top: between the centres of rows 49 and 50.
    let hair = scene(p(20.0, 10.0), vec![Item::Stroke { path: vec![line(p(2.0, 4.975), p(18.0, 4.975))], closed: false, width_mm: 0.01 }]);
    let column = |colour| {
        let image = image(&hair, PngOptions { dpi: 254.0, colour });
        (40..60).map(|y| image.at(100, y)[0]).collect::<Vec<u8>>()
    };
    let grey = column(PngColour::Grey);
    assert!(grey.iter().all(|v| *v > 200) && grey.iter().any(|v| *v < 255), "a tenth of a pixel is light grey: {grey:?}");
    let bw = column(PngColour::BlackWhite);
    assert_eq!(bw.iter().filter(|v| **v == 0).count(), 1, "one black pixel across: {bw:?}");
    let edge = image(&small(), PngOptions { dpi: 100.0, colour: PngColour::Grey });
    assert!(edge.samples.iter().any(|v| *v != 0 && *v != 255), "greyscale edges are anti-aliased");
    // A bar a fifth of a pixel wide over column 100's centre: grey in greyscale, black in black and white.
    let bar = scene(p(20.0, 10.0), vec![Item::Fill { region: Region2 { outer: rectangle(p(10.045, 2.0), p(10.065, 8.0)), holes: vec![] } }]);
    let grey = image(&bar, AT_254).at(100, 50)[0];
    assert!(grey > 0 && grey < 255, "under half a pixel is grey: {grey}");
    assert_eq!(image(&bar, PngOptions { dpi: 254.0, colour: PngColour::BlackWhite }).at(100, 50), [0], "its centre is inside");
}

/// A closed outline is joined where it starts, like its other corners, not left with two butt ends.
#[test]
fn a_closed_outline_joins_at_its_start() {
    let square = rectangle(p(2.0, 2.0), p(8.0, 8.0)).segs;
    let outline = scene(p(10.0, 10.0), vec![Item::Stroke { path: square, closed: true, width_mm: 1.0 }]);
    let image = image(&outline, AT_254);
    // A quarter of a millimetre outside each corner, inside the 1 mm line's mitred corner.
    for (x, y, what) in [(17, 82, "the start"), (82, 82, "the second corner"), (82, 17, "the third"), (17, 17, "the fourth")] {
        assert_eq!(image.at(x, y), [0], "{what}, ({x}, {y})");
    }
}

/// Arcs are drawn from lines within 1/8 px: on a 40 mm circle at 600 dpi, every pixel the stroke's
/// middle crosses is black and pixels a pixel outside its 0.5 mm width are white, at 64 angles.
#[test]
fn a_large_circle_is_round_to_the_pixel() {
    let c = p(50.0, 50.0);
    let circle = scene(
        p(100.0, 100.0),
        vec![Item::Stroke { path: vec![Seg2::Arc { center: c, radius: 40.0, start: 0.0, sweep: TAU }], closed: true, width_mm: 0.5 }],
    );
    let image = image(&circle, PngOptions { dpi: 600.0, colour: PngColour::Grey });
    let per_mm = 600.0 / 25.4;
    let pixel = |q: Vec2| image.at((q.x * per_mm) as usize, ((100.0 - q.y) * per_mm) as usize)[0];
    for k in 0..64 {
        let towards = Vec2::from_angle(TAU * f64::from(k) / 64.0);
        assert_eq!(pixel(c + towards * 40.0), 0, "on the circle at {k}/64");
        for r in [40.0 - 0.25 - 1.5 / per_mm, 40.0 + 0.25 + 1.5 / per_mm] {
            assert_eq!(pixel(c + towards * r), 255, "off the circle at {k}/64, r {r}");
        }
    }
}

/// Drawn a few rows at a time or all at once, the file is the same bytes; and again the same bytes.
#[test]
fn bands_change_nothing_and_the_bytes_repeat() {
    let mut items = small().groups.remove(0).items;
    items.push(Item::Stroke {
        path: vec![Seg2::Arc { center: p(14.0, 3.0), radius: 2.5, start: 0.3, sweep: 1.5 * FRAC_PI_2 }],
        closed: false,
        width_mm: 0.35,
    });
    let sheet = scene(p(20.0, 10.0), items);
    let prepared = sheet.prepare().unwrap();
    for colour in [PngColour::Grey, PngColour::BlackWhite, PngColour::Rgb, PngColour::Transparent] {
        let options = PngOptions { dpi: 300.0, colour };
        let whole = &mut Vec::new();
        write(&prepared, options, &mut *whole, usize::MAX).unwrap();
        let width = read(whole).info.width as usize;
        for band_rows in [1, 3, 7] {
            let rows = &mut Vec::new();
            write(&prepared, options, &mut *rows, band_rows * width * 4).unwrap();
            assert_eq!(whole, rows, "{colour:?}, {band_rows} rows a band");
        }
        assert_eq!(bytes(&sheet, options).0, *whole, "{colour:?}: the same bytes again");
    }
}

/// Out of range resolutions, a curve the walk can't take yet, and more points than the cap are
/// refused, never drawn some other way.
#[test]
fn what_cannot_be_drawn_is_refused() {
    let prepared_small = small();
    let prepared = prepared_small.prepare().unwrap();
    for dpi in [24.9, 2400.1, 0.0, -300.0, f64::NAN, f64::INFINITY] {
        let e = paper_png(&prepared, PngOptions { dpi, colour: PngColour::Grey }, Vec::new()).unwrap_err();
        assert!(e.to_string().contains("25 to 2400 dpi"), "{dpi}: {e}");
    }
    for dpi in [25.0, 2400.0] {
        assert!(paper_png(&prepared, PngOptions { dpi, colour: PngColour::Grey }, Vec::new()).is_ok(), "{dpi}");
    }
    let ellipse = scene(
        p(20.0, 10.0),
        vec![Item::Stroke { path: vec![Seg2::Conic { a: p(0.0, 0.0), apex: p(5.0, 5.0), b: p(10.0, 0.0), w: 0.5 }], closed: false, width_mm: 0.35 }],
    );
    let e = paper_png(&ellipse.prepare().unwrap(), PngOptions::default(), Vec::new()).unwrap_err();
    assert!(e.to_string().contains("not supported yet"), "{e}");
    // 1,000 circles of 100 m at 2400 dpi, each about 19,000 points: over 2^24 in all. A small sheet,
    // so a cap that failed would cost seconds, not hours.
    let big = Loop2::circle(p(0.0, 0.0), 1e5);
    let circles = (0..1000).map(|_| Item::Fill { region: Region2 { outer: big.clone(), holes: vec![] } }).collect();
    let e =
        paper_png(&scene(p(20.0, 10.0), circles).prepare().unwrap(), PngOptions { dpi: 2400.0, colour: PngColour::Grey }, Vec::new()).unwrap_err();
    assert!(e.to_string().contains("16777216 points"), "{e}");
}
