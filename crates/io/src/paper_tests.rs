use solidcraft_geom::paper::{Group, Item, NoPictures, ObjectKind, Picture, Scene};
use solidcraft_geom::{Aabb2, Loop2, Region2, Seg2, Vec2};

use super::png::tests::{Image, Provider, read};
use super::*;

const BLUES: [PaperColour; 2] = [PaperColour::Blueprint, PaperColour::PrussianBlue];

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// A 20 x 10 mm sheet: a line whose edges fall inside pixels, and a fill. At 254 dpi a millimetre
/// is 10 pixels.
fn sheet(mut items: Vec<Item>) -> Scene {
    items.push(Item::Stroke { path: vec![Seg2::Line { a: p(2.0, 5.03), b: p(18.0, 5.03) }], closed: false, width_mm: 1.0 });
    let square = Loop2::polygon(&[p(2.0, 1.0), p(8.0, 1.0), p(8.0, 4.0), p(2.0, 4.0)]);
    items.push(Item::Fill { region: Region2 { outer: square, holes: vec![] } });
    Scene::new(p(20.0, 10.0), vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }])
}

fn picture(source: u64, opacity: f64) -> Item {
    Item::Picture(Picture { source, rect: Aabb2 { min: p(14.0, 7.0), max: p(16.0, 8.0) }, opacity })
}

fn svg(scene: &Scene, paper: Option<PaperColour>) -> String {
    let (bytes, _) = paper_svg_on(&scene.prepare().unwrap(), paper, &Provider::default(), 254.0).unwrap();
    String::from_utf8(bytes).unwrap()
}

fn png(scene: &Scene, colour: PngColour) -> Image {
    let mut out = Vec::new();
    paper_png(&scene.prepare().unwrap(), PngOptions { dpi: 254.0, colour }, &Provider::default(), &mut out).unwrap();
    read(&out)
}

/// Blue paper changes only the SVG's colours and adds its rectangle, under everything; a picture's
/// `<image>` is the same bytes. Without a paper, `paper_svg` is the same document.
#[test]
fn a_blue_svg_is_the_white_one_in_its_colours() {
    let scene = sheet(vec![picture(1, 1.0)]);
    let white = svg(&scene, None);
    assert_eq!(white.into_bytes(), paper_svg(&scene.prepare().unwrap(), &Provider::default(), 254.0).unwrap().0);
    let white = svg(&scene, None);
    assert!(white.contains("<image"));
    for (paper, hex) in BLUES.into_iter().zip(["#123A76", "#003153"]) {
        let (root_end, _) = white.match_indices(">\n").nth(1).unwrap();
        let rect = format!("<rect width=\"20\" height=\"10\" fill=\"{hex}\" stroke=\"none\"/>\n");
        let expected = format!("{}{rect}{}", &white[..root_end + 2], &white[root_end + 2..]).replace("#000", "#E2EEFC");
        assert_eq!(svg(&scene, Some(paper)), expected);
    }
}

/// Blue paper changes only the PNG's colours: its size and type are the RGB one's, and every pixel
/// lies between paper and ink as far as the RGB one lies between white and black, so edges blend
/// from one to the other. The line's middle is exactly the ink, the sheet's corner exactly the paper.
#[test]
fn a_blue_png_is_the_white_one_in_its_colours() {
    let scene = sheet(vec![]);
    let white = png(&scene, PngColour::Rgb);
    let edges = white.samples.iter().filter(|s| **s != 0 && **s != 255).count();
    assert!(edges > 0, "the sheet has edges inside pixels");
    for paper in BLUES {
        let blue = png(&scene, PngColour::On(paper));
        assert_eq!((blue.info.color_type, blue.info.width, blue.info.height), (white.info.color_type, white.info.width, white.info.height));
        assert_eq!(format!("{:?}", blue.info.pixel_dims), format!("{:?}", white.info.pixel_dims));
        assert_eq!(blue.at(0, 0), paper.rgb());
        assert_eq!(blue.at(100, 50), PaperColour::INK);
        for (i, (b, w)) in blue.samples.iter().zip(&white.samples).enumerate() {
            let (paper, ink) = (f64::from(paper.rgb()[i % 3]), f64::from(PaperColour::INK[i % 3]));
            let expected = paper + (ink - paper) * (1.0 - f64::from(*w) / 255.0);
            assert!((f64::from(*b) - expected).abs() <= 2.0, "sample {i}: {b}, expected {expected}");
        }
    }
}

/// A picture keeps its own colours on blue paper; what it lets through shows the paper. Lines are
/// the ink wherever a picture isn't.
#[test]
fn a_picture_keeps_its_colours_on_blue_paper() {
    let paper = PaperColour::Blueprint;
    let [r, g, b] = paper.rgb().map(f64::from);
    let solid = png(&sheet(vec![picture(1, 1.0)]), PngColour::On(paper));
    assert_eq!(solid.at(150, 25), [255, 0, 0], "red stays red");
    let half = png(&sheet(vec![picture(1, 0.5)]), PngColour::On(paper));
    let blended = [(255.0 + r) / 2.0, g / 2.0, b / 2.0];
    assert!(half.at(150, 25).iter().zip(blended).all(|(got, want)| (f64::from(*got) - want).abs() <= 1.0), "{:?}", half.at(150, 25));
    assert_eq!(half.at(0, 0), paper.rgb());
    assert_eq!(half.at(100, 50), PaperColour::INK);
}

/// A sheet with nothing on it is all paper, and white's `Rgb` is untouched by the blue rule.
#[test]
fn an_empty_sheet_is_all_paper() {
    let empty = Scene::new(p(20.0, 10.0), vec![]);
    let write = |colour| {
        let mut out = Vec::new();
        paper_png(&empty.prepare().unwrap(), PngOptions { dpi: 25.4, colour }, &NoPictures, &mut out).unwrap();
        read(&out)
    };
    assert!(write(PngColour::Rgb).samples.iter().all(|s| *s == 255));
    for paper in BLUES {
        assert!(write(PngColour::On(paper)).samples.chunks(3).all(|c| c == paper.rgb()));
    }
}
