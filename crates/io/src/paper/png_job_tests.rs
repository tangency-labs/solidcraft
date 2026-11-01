use solidcraft_geom::paper::{Group, Item, ObjectKind, Scene};
use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use super::*;

/// A 20 × 10 mm sheet: a 1 mm line, a thin arc and a fill with a hole.
fn sheet() -> Scene {
    let p = Vec2::new;
    let square = |min: Vec2, max: Vec2| Loop2::polygon(&[min, p(max.x, min.y), max, p(min.x, max.y)]);
    let items = vec![
        Item::Stroke { path: vec![Seg2::Line { a: p(2.0, 5.0), b: p(18.0, 5.0) }], closed: false, width_mm: 1.0 },
        Item::Stroke { path: vec![Seg2::Arc { center: p(14.0, 3.0), radius: 2.5, start: 0.3, sweep: 2.4 }], closed: false, width_mm: 0.35 },
        Item::Fill { region: Region2 { outer: square(p(2.0, 1.0), p(8.0, 4.0)), holes: vec![square(p(4.0, 2.0), p(6.0, 3.0))] } },
    ];
    Scene { size_mm: p(20.0, 10.0), groups: vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }] }
}

/// Written a band a step, the file is the one written at once with the same bands, byte for byte
/// (with `paper_png`'s, the bands are its own); the rows written only go up, to the image's rows,
/// and the report is known before the first band.
#[test]
fn a_png_written_a_band_a_step_is_the_png_written_at_once() {
    let sheet = sheet();
    let scene = sheet.prepare().unwrap();
    for colour in [PngColour::Grey, PngColour::BlackWhite, PngColour::Rgb, PngColour::Transparent] {
        let options = PngOptions { dpi: 300.0, colour };
        let mut whole = Vec::new();
        let report = paper_png(&scene, options, &mut whole).unwrap();
        for band_rows in [1, 7, 1000] {
            let band_bytes = band_rows * report.pixels[0] as usize * 4;
            let mut at_once = Vec::new();
            assert_eq!(write(&scene, options, &mut at_once, band_bytes).unwrap(), report);
            let mut png = PngWriter::with_bands(&scene, options, band_bytes).unwrap();
            assert_eq!((png.report(), png.rows()), (report, [0, report.pixels[1]]));
            let (mut out, mut rows) = (Vec::new(), Vec::new());
            while png.step(&mut out).unwrap() {
                rows.push(png.rows()[0]);
            }
            assert!(rows.windows(2).all(|w| w[0] < w[1]) && png.rows() == [report.pixels[1]; 2], "{colour:?}, {band_rows}: {rows:?}");
            assert_eq!(png.finish(&mut out).unwrap(), report);
            assert_eq!(out, at_once, "{colour:?}, {band_rows} rows a band");
        }
        let mut png = PngWriter::new(&scene, options).unwrap();
        let mut out = Vec::new();
        while png.step(&mut out).unwrap() {}
        png.finish(&mut out).unwrap();
        assert_eq!(out, whole, "{colour:?}: the job's own bands are paper_png's");
    }
}

/// A writer that fails is reported where it fails, the file's last bytes included: png drops an
/// error writing those as its stream ends, so they go through the writer's own buffer.
#[test]
fn a_write_that_fails_is_reported_even_on_the_last_byte() {
    let sheet = sheet();
    let scene = sheet.prepare().unwrap();
    let options = PngOptions::default();
    let mut whole = Vec::new();
    paper_png(&scene, options, &mut whole).unwrap();
    let mut short = Limited { bytes: Vec::new(), limit: whole.len() - 1 };
    let mut png = PngWriter::new(&scene, options).unwrap();
    while png.step(&mut short).unwrap() {}
    assert!(png.finish(&mut short).unwrap_err().to_string().contains("no room"));
    let mut short = Limited { bytes: Vec::new(), limit: 10 };
    assert!(PngWriter::new(&scene, options).unwrap().step(&mut short).is_err(), "the header doesn't fit");
}

/// Takes `limit` bytes, then refuses.
struct Limited {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for Limited {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + buf.len() > self.limit {
            return Err(std::io::Error::other("no room"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
