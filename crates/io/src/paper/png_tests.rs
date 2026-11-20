use std::cell::RefCell;
use std::f64::consts::{FRAC_PI_2, TAU};

use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_geom::Aabb2;
use solidcraft_geom::paper::text::{Cluster, Embed, Font, FontFlags, FontGlyph, FontMetrics, PlacedGlyph, TextRun};
use solidcraft_geom::paper::{Dashes, Group, Item, NoPictures, ObjectKind, Scene};
use solidcraft_geom::prelude::*;

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
            Item::Stroke { path: vec![line(p(2.0, 5.0), p(18.0, 5.0))], closed: false, width_mm: 1.0, pattern: None },
            Item::Fill { region: Region2 { outer: rectangle(p(2.0, 1.0), p(8.0, 4.0)), holes: vec![rectangle(p(4.0, 2.0), p(6.0, 3.0))] } },
        ],
    )
}

pub(in crate::paper) struct Image {
    pub(in crate::paper) info: png::Info<'static>,
    /// One byte a sample, rows top down: 1-bit images unpacked to 0 or 255.
    pub(in crate::paper) samples: Vec<u8>,
}

impl Image {
    fn channels(&self) -> usize {
        self.info.color_type.samples()
    }

    /// The samples of the pixel at column `x`, row `y` from the top.
    pub(in crate::paper) fn at(&self, x: usize, y: usize) -> &[u8] {
        let c = self.channels();
        let i = (y * self.info.width as usize + x) * c;
        &self.samples[i..i + c]
    }
}

fn bytes(scene: &Scene, options: PngOptions) -> (Vec<u8>, PngReport) {
    let mut out = Vec::new();
    let report = paper_png(&scene.prepare().unwrap(), options, &NoPictures, &mut out).unwrap();
    (out, report)
}

pub(in crate::paper) fn read(bytes: &[u8]) -> Image {
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

/// A sheet is its size at the dpi, rounded outward as the sheet's pixel grid is (nothing is
/// clipped), and says its dpi, so it prints at the sheet's size: 300 dpi greyscale unless asked,
/// sRGB. A side over the grid's limit is refused with the limit named.
#[test]
fn a_sheet_is_its_size_at_the_dpi_and_records_it() {
    let a4 = scene(p(210.0, 297.0), vec![]);
    assert_eq!(PngOptions::default(), PngOptions { dpi: 300.0, colour: PngColour::Grey });
    let (data, report) = bytes(&a4, PngOptions::default());
    assert_eq!(report.pixels, [2481, 3508]);
    let image = read(&data);
    assert_eq!((image.info.width, image.info.height), (2481, 3508));
    assert_eq!((image.info.color_type, image.info.bit_depth), (png::ColorType::Grayscale, png::BitDepth::Eight));
    let dims = image.info.pixel_dims.unwrap();
    assert_eq!((dims.xppu, dims.yppu, dims.unit), (11811, 11811, png::Unit::Meter));
    assert!(image.info.srgb.is_some());
    assert!(image.samples.iter().all(|s| *s == 255), "blank paper is white");
    assert_eq!(bytes(&a4, PngOptions { dpi: 25.0, ..PngOptions::default() }).1.pixels, [207, 293]);
    assert_eq!(bytes(&scene(p(420.0, 297.0), vec![]), PngOptions::default()).1.pixels, [4961, 3508]);
    let a0 = scene(p(1189.0, 841.0), vec![]);
    let e = paper_png(&a0.prepare().unwrap(), PngOptions { dpi: 1500.0, colour: PngColour::Grey }, &NoPictures, Vec::new()).unwrap_err();
    assert!(e.to_string().contains("over the limit of 65536 px"), "{e}");
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
    let hair =
        scene(p(20.0, 10.0), vec![Item::Stroke { path: vec![line(p(2.0, 4.975), p(18.0, 4.975))], closed: false, width_mm: 0.01, pattern: None }]);
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
    let outline = scene(p(10.0, 10.0), vec![Item::Stroke { path: square, closed: true, width_mm: 1.0, pattern: None }]);
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
        vec![Item::Stroke { path: vec![Seg2::Arc { center: c, radius: 40.0, start: 0.0, sweep: TAU }], closed: true, width_mm: 0.5, pattern: None }],
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

/// Drawn a few rows at a time or all at once, the file is the same bytes, pictures and all, and
/// each picture row is asked for once; and again the same bytes.
#[test]
fn bands_change_nothing_and_the_bytes_repeat() {
    let mut items = small().groups.remove(0).items;
    items.push(Item::Stroke {
        path: vec![Seg2::Arc { center: p(14.0, 3.0), radius: 2.5, start: 0.3, sweep: 1.5 * FRAC_PI_2 }],
        closed: false,
        width_mm: 0.35,
        pattern: None,
    });
    items.insert(0, picture(1, p(1.0, 1.0), p(3.0, 2.0), 1.0));
    items.push(picture(2, p(12.0, 5.0), p(16.0, 9.0), 0.7));
    items.push(picture(3, p(8.0, 0.5), p(10.03, 4.5), 1.0));
    let sheet = scene(p(20.0, 10.0), items);
    let prepared = sheet.prepare().unwrap();
    // The rows asked for, by source: a band asks only for its own rows of a picture.
    let rows_asked = |provider: &Provider| {
        let mut rows = [0; 4];
        for (source, _, window) in provider.asked.borrow().iter() {
            rows[*source as usize] += window.y1 - window.y0;
        }
        rows
    };
    for colour in [PngColour::Grey, PngColour::BlackWhite, PngColour::Rgb, PngColour::Transparent] {
        let options = PngOptions { dpi: 300.0, colour };
        let (whole, once) = (&mut Vec::new(), Provider::default());
        write(&prepared, options, &once, &mut *whole, usize::MAX).unwrap();
        let width = read(whole).info.width as usize;
        for band_rows in [1, 3, 7] {
            let (rows, banded) = (&mut Vec::new(), Provider::default());
            write(&prepared, options, &banded, &mut *rows, band_rows * width * 4).unwrap();
            assert_eq!(whole, rows, "{colour:?}, {band_rows} rows a band");
            assert_eq!(rows_asked(&banded), rows_asked(&once), "{colour:?}, {band_rows} rows a band: each picture row once");
        }
        let again = &mut Vec::new();
        paper_png(&prepared, options, &Provider::default(), &mut *again).unwrap();
        assert_eq!(again, whole, "{colour:?}: the same bytes again");
    }
}

/// The window's background export watches the sheet PNG: before each band it hears the rows drawn so
/// far, and once cancelled the PNG stops at the next band with an error. A watch changes no byte.
#[test]
fn a_watched_sheet_png_reports_its_rows_and_stops_when_cancelled() {
    struct Stop {
        reports: std::sync::Mutex<Vec<(&'static str, usize, usize)>>,
        after: usize,
    }
    impl solidcraft_kernel::watch::Watch for Stop {
        fn cancelled(&self) -> bool {
            self.reports.lock().unwrap().len() >= self.after
        }
        fn progress(&self, stage: &'static str, done: usize, total: usize) {
            self.reports.lock().unwrap().push((stage, done, total));
        }
    }
    let sheet = small();
    let prepared = sheet.prepare().unwrap();
    let options = PngOptions { dpi: 254.0, colour: PngColour::Grey };
    let (plain, _) = bytes(&sheet, options);
    // 20 x 10 mm at 254 dpi is 200 x 100 pixels; three rows a band make 34 bands.
    let run = |after| {
        let watch = std::sync::Arc::new(Stop { reports: Default::default(), after });
        let mut out = Vec::new();
        let written = solidcraft_kernel::watch::watching(watch.clone(), || write(&prepared, options, &NoPictures, &mut out, 3 * 200 * 4));
        let reports = watch.reports.lock().unwrap().clone();
        (written.map(|_| out), reports)
    };
    let (whole, reports) = run(usize::MAX);
    assert_eq!(whole.unwrap(), plain, "a watch changes no byte");
    assert_eq!(reports.len(), 34);
    assert!(reports.iter().enumerate().all(|(i, r)| *r == ("sheet_png", 3 * i, 100)), "{reports:?}");
    let (stopped, reports) = run(2);
    assert!(stopped.unwrap_err().to_string().contains("cancelled"));
    assert_eq!(reports.len(), 2, "it stops before the third band");
}

/// Out of range resolutions, text (B2b) and more points than the cap are refused, never drawn some
/// other way. An ellipse, which the walk once refused, draws.
#[test]
fn what_cannot_be_drawn_is_refused() {
    let prepared_small = small();
    let prepared = prepared_small.prepare().unwrap();
    for dpi in [24.9, 2400.1, 0.0, -300.0, f64::NAN, f64::INFINITY] {
        let e = paper_png(&prepared, PngOptions { dpi, colour: PngColour::Grey }, &NoPictures, Vec::new()).unwrap_err();
        assert!(e.to_string().contains("25 to 2400 dpi"), "{dpi}: {e}");
    }
    for dpi in [25.0, 2400.0] {
        assert!(paper_png(&prepared, PngOptions { dpi, colour: PngColour::Grey }, &NoPictures, Vec::new()).is_ok(), "{dpi}");
    }
    let ellipse = scene(
        p(20.0, 10.0),
        vec![Item::Stroke {
            path: vec![Seg2::Conic { a: p(0.0, 0.0), apex: p(5.0, 5.0), b: p(10.0, 0.0), w: 0.5 }],
            closed: false,
            width_mm: 0.35,
            pattern: None,
        }],
    );
    // An ellipse draws, as cubics (A3b).
    assert!(paper_png(&ellipse.prepare().unwrap(), PngOptions::default(), &NoPictures, Vec::new()).is_ok());
    // 1,000 circles of 100 m at 2400 dpi, each about 19,000 points: over 2^24 in all. A small sheet,
    // so a cap that failed would cost seconds, not hours.
    let big = Loop2::circle(p(0.0, 0.0), 1e5);
    let circles = (0..1000).map(|_| Item::Fill { region: Region2 { outer: big.clone(), holes: vec![] } }).collect();
    let e =
        paper_png(&scene(p(20.0, 10.0), circles).prepare().unwrap(), PngOptions { dpi: 2400.0, colour: PngColour::Grey }, &NoPictures, Vec::new())
            .unwrap_err();
    assert!(e.to_string().contains("16777216 points"), "{e}");
}

/// Pictures by source: 0 a checkerboard of the asked grid's own pixels, 1 red, 2 blue at half
/// alpha, 3 mid grey, 4 noise that doesn't compress, 666 an error; every request is kept, and
/// `short` drops a byte. The SVG tests use it too.
#[derive(Default)]
pub(in crate::paper) struct Provider {
    pub(in crate::paper) asked: RefCell<Vec<(u64, PixelGrid, GridWindow)>>,
    pub(in crate::paper) short: bool,
}

impl Pictures for Provider {
    fn rows(&self, source: u64, grid: &PixelGrid, window: GridWindow) -> std::result::Result<Vec<u8>, String> {
        self.asked.borrow_mut().push((source, *grid, window));
        if source == 666 {
            return Err("boom".into());
        }
        let mut rows = Vec::new();
        for y in window.y0..window.y1 {
            for x in window.x0..window.x1 {
                rows.extend(match source {
                    0 if (x + y) % 2 == 0 => [0, 0, 0, 255],
                    0 => [255, 255, 255, 255],
                    1 => [255, 0, 0, 255],
                    2 => [0, 0, 255, 128],
                    4 => (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).wrapping_mul(2_246_822_519).to_le_bytes(),
                    _ => [128, 128, 128, 255],
                });
            }
        }
        if self.short {
            rows.pop();
        }
        Ok(rows)
    }
}

fn picture(source: u64, min: Vec2, max: Vec2, opacity: f64) -> Item {
    Item::Picture(Picture { source, rect: Aabb2 { min, max }, opacity })
}

fn pictured(scene: &Scene, options: PngOptions, pictures: &Provider) -> Image {
    let mut out = Vec::new();
    paper_png(&scene.prepare().unwrap(), options, pictures, &mut out).unwrap();
    read(&out)
}

const RGB_254: PngOptions = PngOptions { dpi: 254.0, colour: PngColour::Rgb };

/// A picture fills exactly its window of the sheet's grid: a 2 × 1 mm picture 3 mm below the
/// top, from x = 4, is columns 40 to 59 and rows 30 to 39 at 254 dpi, and a fill over the same
/// rectangle covers every one of its pixels, so the two register. Also on a sheet 100.8 pixels
/// high, whose grid is anchored at its top edge as the lines are, not at its bottom.
#[test]
fn a_picture_fills_exactly_its_window_and_registers_with_the_lines() {
    for height in [10.0, 10.08] {
        let rect = rectangle(p(4.0, height - 4.0), p(6.0, height - 3.0));
        let red = picture(1, p(4.0, height - 4.0), p(6.0, height - 3.0), 1.0);
        let image = pictured(&scene(p(20.0, height), vec![red.clone()]), RGB_254, &Provider::default());
        for (x, y, colour, what) in [
            (40, 30, [255, 0, 0], "its top-left pixel"),
            (59, 39, [255, 0, 0], "its bottom-right pixel"),
            (39, 35, [255, 255, 255], "left of it"),
            (60, 35, [255, 255, 255], "right of it"),
            (50, 29, [255, 255, 255], "above it"),
            (50, 40, [255, 255, 255], "below it"),
        ] {
            assert_eq!(image.at(x, y), colour, "{height} mm high: {what}, ({x}, {y})");
        }
        let covered = Item::Fill { region: Region2 { outer: rect, holes: vec![] } };
        let image = pictured(&scene(p(20.0, height), vec![red, covered]), RGB_254, &Provider::default());
        assert!(image.samples.chunks(3).all(|c| c[1] == c[0]), "{height} mm high: no red left beside the fill's edges");
    }
}

/// Paint goes on in the scene's order: a fill before a picture is under it, one after it is over it.
#[test]
fn pictures_keep_their_place_in_the_paint_order() {
    let red = || picture(1, p(4.0, 6.0), p(6.0, 7.0), 1.0);
    let fill = || Item::Fill { region: Region2 { outer: rectangle(p(3.0, 5.0), p(7.0, 8.0)), holes: vec![] } };
    let under = pictured(&scene(p(20.0, 10.0), vec![fill(), red()]), RGB_254, &Provider::default());
    assert_eq!(under.at(50, 35), [255, 0, 0], "the picture over the fill");
    assert_eq!(under.at(35, 35), [0, 0, 0], "the fill around it");
    let over = pictured(&scene(p(20.0, 10.0), vec![red(), fill()]), RGB_254, &Provider::default());
    assert_eq!(over.at(50, 35), [0, 0, 0], "the fill over the picture");
}

/// Opacity scales a picture's alpha: red at half over white paper is pink; on transparent paper
/// the pixel keeps the half alpha.
#[test]
fn opacity_scales_the_alpha() {
    let half = scene(p(20.0, 10.0), vec![picture(1, p(4.0, 6.0), p(6.0, 7.0), 0.5)]);
    assert_eq!(pictured(&half, RGB_254, &Provider::default()).at(50, 35), [255, 128, 128]);
    let clear = pictured(&half, PngOptions { dpi: 254.0, colour: PngColour::Transparent }, &Provider::default());
    assert_eq!(clear.at(50, 35), [255, 0, 0, 128]);
    let blue = scene(p(20.0, 10.0), vec![picture(2, p(4.0, 6.0), p(6.0, 7.0), 1.0)]);
    assert_eq!(pictured(&blue, RGB_254, &Provider::default()).at(50, 35), [127, 127, 255], "its own half alpha");
}

/// Up to 1200 dpi a picture is asked for on the sheet's own grid. Above it, on the grid of half
/// the dpi at 2400, each of whose pixels fills 2 × 2 of the sheet's.
#[test]
fn above_1200_dpi_a_coarser_pictures_pixels_are_copied() {
    let checks = scene(p(20.0, 10.0), vec![picture(0, p(4.0, 4.0), p(5.0, 5.0), 1.0)]);
    for (dpi, k) in [(1200.0, 1), (2400.0, 2)] {
        let provider = Provider::default();
        let image = pictured(&checks, PngOptions { dpi, colour: PngColour::Rgb }, &provider);
        let sheet = PixelGrid::sheet(p(20.0, 10.0), dpi).unwrap();
        let window = sheet.window(Aabb2 { min: p(4.0, 4.0), max: p(5.0, 5.0) }).unwrap();
        let asked = provider.asked.borrow();
        assert_eq!(asked.len(), 1, "one band, one request");
        assert!((asked[0].1.pitch - sheet.pitch * f64::from(k)).abs() < 1e-12, "{dpi}: asked at {} mm", asked[0].1.pitch);
        for y in window.y0..window.y1 {
            for x in window.x0..window.x1 {
                let black = (x / k + y / k) % 2 == 0;
                assert_eq!(image.at(x as usize, y as usize), if black { [0, 0, 0] } else { [255, 255, 255] }, "{dpi}: ({x}, {y})");
            }
        }
    }
}

/// In black and white a picture is dithered at its place on the sheet: mid grey is half black,
/// whatever the band; a line over it stays solid; the report says so.
#[test]
fn black_and_white_dithers_pictures_and_keeps_lines_solid() {
    // 16 × 16 pixels at 254 dpi, from column 8 and row 8.
    let grey = picture(3, p(0.8, 7.6), p(2.4, 9.2), 1.0);
    let line = Item::Stroke { path: vec![line(p(0.0, 8.0), p(20.0, 8.0))], closed: false, width_mm: 0.2, pattern: None };
    let sheet = scene(p(20.0, 10.0), vec![grey, line]);
    let options = PngOptions { dpi: 254.0, colour: PngColour::BlackWhite };
    let prepared = sheet.prepare().unwrap();
    let (whole, rows) = (&mut Vec::new(), &mut Vec::new());
    let report = write(&prepared, options, &Provider::default(), &mut *whole, usize::MAX).unwrap();
    write(&prepared, options, &Provider::default(), &mut *rows, 1).unwrap();
    assert_eq!(whole, rows, "bands change nothing");
    assert!(report.dithered);
    let image = read(whole);
    let black = (8..24).flat_map(|y| (8..24).map(move |x| (x, y))).filter(|&(x, y)| y != 19 && y != 20 && image.at(x, y)[0] == 0).count();
    assert_eq!(black, 14 * 16 / 2, "half of mid grey is black, away from the line");
    assert!((0..200).all(|x| image.at(x, 19)[0] == 0 && image.at(x, 20)[0] == 0), "the line is solid across the picture");
    assert!(!bytes(&small(), options).1.dithered, "nothing to dither");
}

/// A provider's pixels are checked: too few bytes, or an error, refuse the PNG with the source
/// named; no provider refuses any picture; a picture off the sheet asks for nothing.
#[test]
fn a_providers_pixels_are_checked() {
    let sheet = |source| scene(p(20.0, 10.0), vec![picture(source, p(4.0, 6.0), p(6.0, 7.0), 1.0)]);
    let refused = |s: &Scene, provider: &dyn Pictures| paper_png(&s.prepare().unwrap(), RGB_254, provider, Vec::new()).unwrap_err().to_string();
    let short = refused(&sheet(1), &Provider { short: true, ..Provider::default() });
    assert!(short.contains("picture 1: 799 bytes where 800 were asked for"), "{short}");
    assert!(refused(&sheet(666), &Provider::default()).contains("picture 666: boom"));
    assert!(refused(&sheet(1), &NoPictures).contains("no pixels for picture 1"));
    let provider = Provider::default();
    pictured(&scene(p(20.0, 10.0), vec![picture(1, p(30.0, 30.0), p(40.0, 40.0), 1.0)]), RGB_254, &provider);
    assert!(provider.asked.borrow().is_empty());
}

/// The dark runs along row `y`, as pixel ranges.
fn runs(image: &Image, y: usize) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for x in 0..=image.info.width as usize {
        let dark = x < image.info.width as usize && image.at(x, y)[0] < 128;
        match (dark, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    runs
}

/// The dashes, as millimetre ranges along a line of `length`, that SVG draws from a dasharray of
/// one dash and gap and an offset: what a reader of the SVG's numbers expects.
fn svg_dashes(dash: f64, gap: f64, offset: f64, length: f64) -> Vec<(f64, f64)> {
    let mut at = -offset;
    let mut dashes = Vec::new();
    while at < length {
        if at + dash > 0.0 {
            dashes.push((at.max(0.0), (at + dash).min(length)));
        }
        at += dash + gap;
    }
    dashes
}

/// A 100 mm stroke dashed 4.2 and 1.05 mm (ISO 128-2's dashed line at d = 0.35 mm) has 20
/// dashes, 19 whole and the first 0.25 mm of the 20th, at the SVG's positions to the pixel; the
/// phase moves them, and one inside a gap starts the line in it. At 508 dpi a millimetre is 20
/// pixels, and these ends fall between pixels.
#[test]
fn a_dashed_line_has_the_svgs_dashes_to_the_pixel() {
    for (phase_mm, count) in [(0.0, 20), (0.5, 20), (4.5, 19)] {
        let pattern = Some(Dashes { lengths_mm: vec![4.2, 1.05], phase_mm });
        let sheet =
            scene(p(110.0, 20.0), vec![Item::Stroke { path: vec![line(p(5.0, 10.0), p(105.0, 10.0))], closed: false, width_mm: 1.0, pattern }]);
        let (text, _) = crate::paper::paper_svg(&sheet.prepare().unwrap(), &NoPictures, 300.0).unwrap();
        let text = String::from_utf8(text).unwrap();
        let number = |name: &str| text.split(&format!("{name}=\"")).nth(1).map(|t| t.split('"').next().unwrap().to_string());
        let array: Vec<f64> = number("stroke-dasharray").unwrap().split(' ').map(|n| n.parse().unwrap()).collect();
        let offset: f64 = number("stroke-dashoffset").map_or(0.0, |n| n.parse().unwrap());
        let expected: Vec<(usize, usize)> = svg_dashes(array[0], array[1], offset, 100.0)
            .iter()
            .map(|(a, b)| (((5.0 + a) * 20.0).round() as usize, ((5.0 + b) * 20.0).round() as usize))
            .collect();
        assert_eq!(expected.len(), count, "{phase_mm}");
        let image = image(&sheet, PngOptions { dpi: 508.0, colour: PngColour::Grey });
        assert_eq!(runs(&image, 200), expected, "phase {phase_mm}");
    }
    let last = svg_dashes(4.2, 1.05, 0.0, 100.0).last().copied().unwrap();
    assert!((last.1 - last.0 - 0.25).abs() < 1e-9, "{last:?}");
}

/// A closed square's pattern runs round its corners and joins at its start: 6 mm dashes and 2 mm
/// gaps round a 10 mm square from its lower-left corner, anticlockwise.
#[test]
fn a_closed_strokes_dashes_run_round_its_corners() {
    let pattern = Some(Dashes { lengths_mm: vec![6.0, 2.0], phase_mm: 0.0 });
    let square = rectangle(p(5.0, 5.0), p(15.0, 15.0)).segs;
    let sheet = scene(p(20.0, 20.0), vec![Item::Stroke { path: square, closed: true, width_mm: 0.5, pattern }]);
    let image = image(&sheet, AT_254);
    // Pixel centres at (x, y) mm from the lower-left corner.
    let dark = |x: f64, y: f64| image.at((x * 10.0) as usize, ((20.0 - y) * 10.0) as usize)[0] < 128;
    // Along the bottom: dash 0-6, gap 6-8, dash 8-14 round the corner to 4 mm up the right side.
    assert!(dark(8.0, 5.0) && !dark(12.0, 5.0) && dark(14.0, 5.0) && dark(15.0, 7.0));
    // Gap 14-16 (4-6 mm up), dash 16-22 (6 mm up, round the top corner, 2 mm left).
    assert!(!dark(15.0, 10.0) && dark(15.0, 13.0) && dark(13.5, 15.0) && !dark(12.0, 15.0));
    // The last dash, 32-38, ends 2 mm above the start; the gap 38-40 reaches it.
    assert!(dark(5.0, 12.0) && !dark(5.0, 6.0));
}

/// A line that falls in a gap draws nothing; one past tiny-skia's million dashes is refused, not
/// drawn solid or left out; the dashes' points count against the sheet's.
#[test]
fn a_line_in_a_gap_draws_nothing_and_too_many_dashes_are_refused() {
    let mut sheet = Sheet { px: 1.0, height: 0.0, tolerance: 0.125, points: 0 };
    let line = |length: f32| {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        builder.line_to(length, 0.0);
        builder.finish().unwrap()
    };
    let pattern = |lengths: &[i64], phase| GridDashes { lengths: lengths.to_vec(), phase };
    assert!(sheet.dash(&line(1.0), &pattern(&[1, 10], 2)).unwrap().is_none());
    assert_eq!(sheet.points, 0);
    assert!(sheet.dash(&line(1.0), &pattern(&[1, 10], 0)).unwrap().is_some());
    assert!(sheet.points > 0);
    let refused = sheet.dash(&line(3e6), &pattern(&[1, 1], 0)).unwrap_err();
    assert!(refused.to_string().contains("too many dashes"), "{refused}");
    sheet.points = MAX_POINTS - 3;
    assert!(sheet.dash(&line(100.0), &pattern(&[1, 1], 0)).unwrap_err().to_string().contains("points"));
}

/// A 20 x 10 mm sheet of `runs` set in `fonts`; at 254 dpi a millimetre is 10 pixels.
fn text_sheet(runs: Vec<TextRun>, fonts: Vec<Font>) -> Scene {
    Scene { fonts, ..scene(p(20.0, 10.0), runs.into_iter().map(Item::Text).collect()) }
}

/// A font of one glyph, id 1, with `outline` in units of a thousandth of an em; its bytes are
/// never read (the writers take the scene's outlines).
fn made_up_font(outline: Vec<Loop2>) -> Font {
    let metrics = FontMetrics {
        ascent: 900,
        descent: -200,
        cap_height: 700,
        bbox: [-500, -500, 1000, 900],
        italic_angle: 0.0,
        flags: FontFlags::default(),
        stem_v: 80,
    };
    Font {
        id: [1; 32],
        bytes: std::sync::Arc::from(vec![1u8]),
        face: 0,
        postscript_name: "MadeUp".into(),
        family: "Made Up".into(),
        embed: Embed::Outlines,
        num_glyphs: 2,
        units_per_em: 1000,
        metrics,
        glyphs: [(1, FontGlyph { advance: 1000, outline })].into(),
    }
}

/// A run of `count` of [`made_up_font`]'s glyph, all at `origin`, `scale` millimetres a unit.
fn made_up_run(origin: Vec2, scale: f64, count: usize) -> TextRun {
    TextRun {
        font: [1; 32],
        scale,
        origin,
        angle: 0.0,
        slant: 0.0,
        glyphs: vec![PlacedGlyph { id: 1, x: 0.0, rise: 0.0, advance: 0.0 }; count],
        text: "A".into(),
        clusters: vec![Cluster { glyphs: 0..count as u32, text: 0..1 }],
        rtl: false,
    }
}

/// A sheet's PNG file, drawn `band_bytes` at a time.
fn drawn(scene: &Scene, options: PngOptions, band_bytes: usize) -> Vec<u8> {
    let mut out = Vec::new();
    write(&scene.prepare().unwrap(), options, &NoPictures, &mut out, band_bytes).unwrap();
    out
}

/// A sheet's grey samples at 254 dpi, with the file's bytes.
fn greys(scene: &Scene, band_bytes: usize) -> (Vec<u8>, Vec<u8>) {
    let out = drawn(scene, AT_254, band_bytes);
    let samples = read(&out).samples;
    (out, samples)
}

/// One glyph made of two overlapping boxes turning the same way, 9 x 7 mm, set at (2, 1): where
/// they overlap is inside both, so nonzero fills it, where even-odd would leave it white (#4890 B2b).
#[test]
fn a_glyph_is_filled_nonzero_where_the_scene_puts_it() {
    let font = made_up_font(vec![rectangle(p(0.0, 0.0), p(600.0, 700.0)), rectangle(p(300.0, 0.0), p(900.0, 700.0))]);
    let run = made_up_run(p(2.0, 1.0), 0.01, 1);
    let (_, grey) = greys(&text_sheet(vec![run], vec![font]), BAND_BYTES);
    // The glyph covers x 2 to 11 mm and y 1 to 8 mm: columns 20 to 110, rows 20 to 90 from the top.
    let at = |x: usize, y: usize| grey[y * 200 + x];
    for (x, y, value, what) in [
        (35, 55, 0, "the first box alone"),
        (65, 55, 0, "where the boxes overlap"),
        (95, 55, 0, "the second box alone"),
        (115, 55, 255, "right of the glyph"),
        (65, 95, 255, "below the baseline"),
        (65, 15, 255, "above the cap height"),
    ] {
        assert_eq!(at(x, y), value, "{what} at ({x}, {y})");
    }
}

/// Real text in the drawing font, the marks a dimension uses and a line read right to left,
/// darkens the sheet only within its glyphs' outlines and gives the same bytes every time (#4890
/// B2b). Moved by a millimetre, its pixels leave those outlines. Drawn in bands, in every colour, it
/// leaves no seam: where a band cuts a glyph, tiny-skia may cover an edge pixel one sixteenth
/// differently (16 grey levels), and never more, nor more than a few pixels a sheet.
#[test]
fn real_text_lands_inside_its_outlines_whatever_the_band() {
    let style = solidcraft_sketch::TextStyle::default();
    for line in ["\u{d8}12 \u{b1}0.1 30\u{b0}", "\u{5e7}\u{5d5} 12"] {
        // 2 mm high, so the line fits the 20 mm sheet and a run moved along it shows.
        let (runs, fonts) = solidcraft_sketch::paper_text(line, None, "", p(2.0, 3.0), 2.0, 0.0, &style).unwrap().unwrap();
        let font = |run: &TextRun| fonts.iter().find(|f| f.id == run.font).unwrap();
        // The outlines' extent, each piece sampled 16 times along its length.
        let points: Vec<Vec2> = runs
            .iter()
            .flat_map(|run| run.outlines(font(run)).collect::<Vec<_>>())
            .flat_map(|l| l.segs)
            .flat_map(|s| (0..=16).map(move |i| s.point_at(f64::from(i) / 16.0)))
            .collect();
        let bounds = points.iter().fold(Aabb2::EMPTY, |b, q| b.union(&Aabb2 { min: *q, max: *q }));
        let sheet = text_sheet(runs.clone(), fonts.clone());
        let (bytes, grey) = greys(&sheet, BAND_BYTES);
        let mut exported = Vec::new();
        paper_png(&sheet.prepare().unwrap(), AT_254, &NoPictures, &mut exported).unwrap();
        assert_eq!(exported, bytes, "{line}: an export is drawn in the default band height");
        for colour in [PngColour::Grey, PngColour::BlackWhite, PngColour::Rgb, PngColour::Transparent] {
            let options = PngOptions { colour, ..AT_254 };
            let whole = drawn(&sheet, options, BAND_BYTES);
            assert_eq!(drawn(&sheet, options, BAND_BYTES), whole, "{line}: {colour:?} the same bytes again");
            for rows in [1, 3, 7] {
                let banded = drawn(&sheet, options, 200 * 4 * rows);
                assert_eq!(drawn(&sheet, options, 200 * 4 * rows), banded, "{line}: {colour:?} the same bytes again in bands of {rows} rows");
                // tiny-skia's edge clipping where a band cuts a path can move an edge pixel one AA step.
                let off: Vec<u8> = read(&banded).samples.iter().zip(&read(&whole).samples).map(|(a, b)| a.abs_diff(*b)).filter(|d| *d > 0).collect();
                // One step is 16 grey levels; in black and white such a pixel may cross the threshold.
                let step = if colour == PngColour::BlackWhite { u8::MAX } else { 16 };
                assert!(
                    off.len() <= 32 && off.iter().all(|d| *d <= step),
                    "{line}: {colour:?} in bands of {rows} rows: at most 32 samples, each one AA step, off the whole sheet: {off:?}"
                );
            }
        }
        let pixels = |grey: &[u8]| -> Vec<(usize, usize)> {
            (0..100).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|&(x, y)| grey[y * 200 + x] < 128).collect()
        };
        // A pixel's centre in sheet millimetres, inside the outlines' extent with a pixel's slack.
        let inside = |&(x, y): &(usize, usize)| {
            let (mx, my) = ((x as f64 + 0.5) / 10.0, 10.0 - (y as f64 + 0.5) / 10.0);
            mx >= bounds.min.x - 0.1 && mx <= bounds.max.x + 0.1 && my >= bounds.min.y - 0.1 && my <= bounds.max.y + 0.1
        };
        let dark = pixels(&grey);
        assert!(dark.len() > 50, "{line} draws {} dark pixels", dark.len());
        assert!(dark.iter().all(inside), "{line}: a dark pixel off its outlines");
        // The same text a millimetre to the right is no longer all where its outlines are.
        let moved: Vec<TextRun> = runs.iter().map(|r| TextRun { origin: r.origin + p(1.0, 0.0), ..r.clone() }).collect();
        let (_, shifted) = greys(&text_sheet(moved, fonts), BAND_BYTES);
        assert!(!pixels(&shifted).iter().all(inside), "{line}: a moved run must leave its outlines");
    }
}

/// Text's points count toward the cap like any line's, added up over the sheet: a run of 600
/// glyphs, each a 100 m circle of four Béziers, is drawn at 2400 dpi, and two of them are refused,
/// not drawn (#4890 B2b).
#[test]
fn text_counts_toward_the_point_cap() {
    let k = 0.552_284_75 * 500.0;
    let q = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| Seg2::Cubic { p0: a, p1: b, p2: c, p3: d };
    let (e, n, w, s) = (p(500.0, 0.0), p(0.0, 500.0), p(-500.0, 0.0), p(0.0, -500.0));
    let circle = Loop2 {
        segs: vec![
            q(e, p(500.0, k), p(k, 500.0), n),
            q(n, p(-k, 500.0), p(-500.0, k), w),
            q(w, p(-500.0, -k), p(-k, -500.0), s),
            q(s, p(k, -500.0), p(500.0, -k), e),
        ],
    };
    let options = PngOptions { dpi: 2400.0, colour: PngColour::Grey };
    let drawn = |runs: usize| {
        let runs = (0..runs).map(|_| made_up_run(p(0.0, 0.0), 200.0, 600)).collect();
        paper_png(&text_sheet(runs, vec![made_up_font(vec![circle.clone()])]).prepare().unwrap(), options, &NoPictures, Vec::new())
    };
    assert!(drawn(1).is_ok());
    let e = drawn(2).unwrap_err();
    assert!(e.to_string().contains("16777216 points"), "{e}");
}

fn physical_ink_runs(image: &Image, y: usize) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for x in 0..=image.info.width as usize {
        let dark = x < image.info.width as usize && image.at(x, y)[0] < 128;
        match (dark, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                runs.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    runs
}

/// One prepared paper scene, not three model exports: a 40 mm model line already
/// placed at 1:2, 1:1 and 2:1 is 20, 40 and 80 mm on paper, while its authored
/// 0.4 mm pen stays 0.4 mm. Both real writers read that same prepared value.
#[test]
fn one_prepared_scene_keeps_paper_lengths_and_fixed_pens_in_svg_and_png() {
    let sheet = scene(
        p(120.0, 80.0),
        [0.5, 1.0, 2.0]
            .into_iter()
            .zip([15.0, 40.0, 65.0])
            .map(|(scale, y)| Item::Stroke { path: vec![line(p(10.0, y), p(10.0 + 40.0 * scale, y))], closed: false, width_mm: 0.4, pattern: None })
            .collect(),
    );
    let prepared = sheet.prepare().unwrap();
    let (svg, svg_report) = crate::paper::paper_svg(&prepared, &NoPictures, 300.0).unwrap();
    let xml = std::str::from_utf8(&svg).unwrap();
    assert!(xml.contains("width=\"120mm\" height=\"80mm\" viewBox=\"0 0 120 80\""));
    assert_eq!(xml.matches("stroke-width=\"0.4\"").count(), 3);
    for path in ["M10 65L30 65", "M10 40L50 40", "M10 15L90 15"] {
        assert!(xml.contains(path), "paper coordinates in SVG: {path}: {xml}");
    }
    assert_eq!(crate::paper::paper_svg(&prepared, &NoPictures, 300.0).unwrap(), (svg, svg_report), "the same prepared SVG repeats byte for byte");
    for (dpi, per_mm) in [(254.0, 10usize), (508.0, 20)] {
        let options = PngOptions { dpi, colour: PngColour::Grey };
        let mut first = Vec::new();
        let mut again = Vec::new();
        let report = paper_png(&prepared, options, &NoPictures, &mut first).unwrap();
        let repeated = paper_png(&prepared, options, &NoPictures, &mut again).unwrap();
        assert_eq!(first, again, "{dpi}: repeat bytes");
        assert_eq!(report, repeated, "{dpi}: repeat report");
        let image = read(&first);
        assert_eq!((image.info.width, image.info.height), (120 * per_mm as u32, 80 * per_mm as u32));
        let physical = image.info.pixel_dims.unwrap();
        assert_eq!((physical.xppu, physical.yppu, physical.unit), (per_mm as u32 * 1000, per_mm as u32 * 1000, png::Unit::Meter));
        for (y, length) in [(15usize, 20usize), (40, 40), (65, 80)] {
            let row = (80 - y) * per_mm;
            let start = 10 * per_mm;
            let end = (10 + length) * per_mm;
            assert_eq!(physical_ink_runs(&image, row), [(start, end)], "{dpi}: {length} mm paper length, independent of the fixed pen");
            let pen: Vec<_> = (row - per_mm..row + per_mm).filter(|r| image.at((start + end) / 2, *r)[0] < 128).collect();
            assert_eq!(pen, (row - per_mm / 5..row + per_mm / 5).collect::<Vec<_>>(), "{dpi}: every scale uses the same 0.4 mm pen");
            assert_eq!(image.at(start - 1, row), [255]);
            assert_eq!(image.at(end, row), [255], "butt ends do not extend paper length");
        }
    }
}

/// The outward pixel grid's partial last row must not change the paper origin:
/// SVG's y flip and PNG's top-origin rows agree on a sheet 100.8 pixels high at 254 dpi.
#[test]
fn svg_and_png_share_the_paper_origin_on_a_fractional_pixel_sheet() {
    let sheet =
        scene(p(40.0, 10.08), vec![Item::Stroke { path: vec![line(p(15.0, 2.08), p(15.0, 8.08))], closed: false, width_mm: 0.4, pattern: None }]);
    let prepared = sheet.prepare().unwrap();
    let (svg, _) = crate::paper::paper_svg(&prepared, &NoPictures, 300.0).unwrap();
    let xml = std::str::from_utf8(&svg).unwrap();
    assert!(xml.contains("height=\"10.08mm\"") && xml.contains("M15 8L15 2"), "{xml}");
    let mut png = Vec::new();
    let report = paper_png(&prepared, AT_254, &NoPictures, &mut png).unwrap();
    let image = read(&png);
    assert_eq!(report.pixels, [400, 101]);
    let ink_rows: Vec<_> = (0..101).filter(|row| image.at(150, *row)[0] < 128).collect();
    assert_eq!(ink_rows, (20..80).collect::<Vec<_>>(), "6 mm at 10 px/mm, anchored at the physical top edge rather than the rounded image bottom");
    assert_eq!(physical_ink_runs(&image, 50), [(148, 152)], "the 0.4 mm pen is four pixels wide");
    assert_eq!(image.at(150, 19), [255]);
    assert_eq!(image.at(150, 80), [255]);
}
