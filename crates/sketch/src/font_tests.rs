//! Font choice for sketch text. A small TrueType font is built here, so the outlines it must give are
//! known by hand; the repository's second font (BIZ UDPGothic, `assets/fonts/biz-udp-gothic`) stands for
//! a real one.

use std::path::PathBuf;

use solidcraft_geom::Vec2;

use crate::fonts::find_family_in;
use crate::tests::v;
use crate::*;

/// One glyph of the test font: its character, advance width and contours (font units, y up; an outer
/// contour runs clockwise, a counter counter-clockwise, as in TrueType).
struct Glyph {
    ch: char,
    advance: u16,
    contours: Vec<Vec<(i16, i16)>>,
}

fn block(x0: i16, y0: i16, x1: i16, y1: i16) -> Vec<(i16, i16)> {
    vec![(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
}

fn hole(x0: i16, y0: i16, x1: i16, y1: i16) -> Vec<(i16, i16)> {
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// A TrueType font of 1000 units per em with these glyphs (and an empty `.notdef`), named `family`. It has
/// no OS/2 table, so it has no capital height of its own.
fn build_font(family: &str, glyphs: &[Glyph]) -> Vec<u8> {
    fn u16s(out: &mut Vec<u8>, values: &[u16]) {
        values.iter().for_each(|v| out.extend(v.to_be_bytes()));
    }
    fn u32s(out: &mut Vec<u8>, values: &[u32]) {
        values.iter().for_each(|v| out.extend(v.to_be_bytes()));
    }
    let count = glyphs.len() as u16 + 1;
    // glyf and loca (long offsets).
    let (mut glyf, mut loca) = (Vec::new(), vec![0u32]);
    for g in std::iter::once(None).chain(glyphs.iter().map(Some)) {
        if let Some(g) = g {
            let pts: Vec<(i16, i16)> = g.contours.iter().flatten().copied().collect();
            let (xs, ys): (Vec<i16>, Vec<i16>) = pts.iter().copied().unzip();
            let (x0, x1, y0, y1) = (xs.iter().min(), xs.iter().max(), ys.iter().min(), ys.iter().max());
            u16s(&mut glyf, &[g.contours.len() as u16]);
            glyf.extend([x0, y0, x1, y1].map(|v| v.copied().unwrap_or(0)).iter().flat_map(|v| v.to_be_bytes()));
            let mut end = 0;
            for c in &g.contours {
                end += c.len() as u16;
                u16s(&mut glyf, &[end - 1]);
            }
            u16s(&mut glyf, &[0]); // no instructions
            glyf.extend(vec![1u8; pts.len()]); // on-curve points, 16-bit deltas
            for coords in [xs, ys] {
                let mut prev = 0;
                for v in coords {
                    glyf.extend((v - prev).to_be_bytes());
                    prev = v;
                }
            }
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
        loca.push(glyf.len() as u32);
    }
    let mut head = Vec::new();
    u32s(&mut head, &[0x0001_0000, 0x0001_0000, 0, 0x5F0F_3CF5]);
    u16s(&mut head, &[0, 1000]);
    head.extend([0u8; 16]); // created, modified
    u16s(&mut head, &[0, 0, 1000, 1000, 0, 8, 2, 1, 0]); // box, style, smallest size, direction, long loca, format
    let mut hhea = Vec::new();
    u32s(&mut hhea, &[0x0001_0000]);
    u16s(&mut hhea, &[800, (-200i16) as u16, 0, 1000, 0, 0, 1000, 1, 0, 0, 0, 0, 0, 0, 0, count]);
    let mut maxp = Vec::new();
    u32s(&mut maxp, &[0x0001_0000]);
    u16s(&mut maxp, &[count]);
    let mut hmtx = Vec::new();
    u16s(&mut hmtx, &[0, 0]);
    for g in glyphs {
        u16s(&mut hmtx, &[g.advance, 0]);
    }
    // cmap: one format 12 subtable, a group per character.
    let mut chars: Vec<(u32, u32)> = glyphs.iter().enumerate().map(|(i, g)| (g.ch as u32, i as u32 + 1)).collect();
    chars.sort();
    let mut cmap = Vec::new();
    u16s(&mut cmap, &[0, 1, 3, 10]);
    u32s(&mut cmap, &[12, 0x000C_0000, 16 + 12 * chars.len() as u32, 0, chars.len() as u32]);
    for (c, g) in chars {
        u32s(&mut cmap, &[c, c, g]);
    }
    // name: family (1), style (2), full name (4), all Windows Unicode.
    let strings = [(1u16, family.to_string()), (2, "Regular".to_string()), (4, format!("{family} Regular"))];
    let (mut name, mut storage) = (Vec::new(), Vec::new());
    u16s(&mut name, &[0, strings.len() as u16, 6 + 12 * strings.len() as u16]);
    for (id, s) in &strings {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        u16s(&mut name, &[3, 1, 0x409, *id, 2 * utf16.len() as u16, storage.len() as u16]);
        utf16.iter().for_each(|u| storage.extend(u.to_be_bytes()));
    }
    name.extend(storage);
    // The table directory, tables sorted by tag.
    let tables: [(&[u8; 4], Vec<u8>); 8] = [
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca.iter().flat_map(|o| o.to_be_bytes()).collect()),
        (b"maxp", maxp),
        (b"name", name),
    ];
    let (mut dir, mut body) = (Vec::new(), Vec::new());
    u32s(&mut dir, &[0x0001_0000]);
    u16s(&mut dir, &[tables.len() as u16, 128, 3, 0]);
    for (tag, data) in &tables {
        dir.extend(*tag);
        u32s(&mut dir, &[0, 12 + 16 * tables.len() as u32 + body.len() as u32, data.len() as u32]);
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    dir.extend(body);
    dir
}

/// The test font "Tiny Block": H is a 600 x 700 block, I a 100 x 700 bar and O an 800 x 700 block with a
/// 400 x 300 counter at (200, 200), all in font units.
fn tiny_font() -> Vec<u8> {
    build_font(
        "Tiny Block",
        &[
            Glyph { ch: 'H', advance: 700, contours: vec![block(0, 0, 600, 700)] },
            Glyph { ch: 'I', advance: 200, contours: vec![block(0, 0, 100, 700)] },
            Glyph { ch: 'O', advance: 900, contours: vec![block(0, 0, 800, 700), hole(200, 200, 600, 500)] },
        ],
    )
}

/// A folder of the test font and a few things that are not fonts, removed at the end of the test.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!("solidcraft-font-tests-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("nested").join("tiny.ttf"), tiny_font()).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a font").unwrap();
        std::fs::write(dir.join("broken.ttf"), "not a font either").unwrap();
        Folder(dir)
    }
    fn font(&self) -> String {
        self.0.join("nested").join("tiny.ttf").to_string_lossy().into_owned()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn web_fonts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/biz-udp-gothic")
}

/// The corner points of the lines of `geometry`, as (min, max) over x and y.
fn extent(geometry: &[LinkGeom]) -> (Vec2, Vec2) {
    let pts: Vec<Vec2> = geometry
        .iter()
        .flat_map(|g| match g {
            LinkGeom::Line(a, b) => vec![*a, *b],
            other => panic!("only lines here: {other:?}"),
        })
        .collect();
    let lo = pts.iter().fold(Vec2::new(f64::INFINITY, f64::INFINITY), |m, p| Vec2::new(m.x.min(p.x), m.y.min(p.y)));
    let hi = pts.iter().fold(Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY), |m, p| Vec2::new(m.x.max(p.x), m.y.max(p.y)));
    (lo, hi)
}

#[test]
fn a_font_file_sets_the_outlines_and_the_scale() {
    let folder = Folder::new("outlines");
    // The capital height is that of the font's H (700 units, no OS/2 table), so 7 mm tall text is 0.01 mm per unit:
    // the O is an 8 x 7 mm block (x 10 to 18, y 20 to 27) with a counter from (12, 22) to (16, 25).
    let o = text_geometry_in(Some(&folder.font()), "O", v(10.0, 20.0), 7.0, 0.0).unwrap();
    assert_eq!(o.len(), 8);
    let (lo, hi) = extent(&o);
    assert!((lo.x - 10.0).abs() < 1e-9 && (lo.y - 20.0).abs() < 1e-9 && (hi.x - 18.0).abs() < 1e-9 && (hi.y - 27.0).abs() < 1e-9, "{lo:?} {hi:?}");
    let inner = o.iter().filter(|g| matches!(g, LinkGeom::Line(a, _) if a.x > 10.5 && a.x < 17.5 && a.y > 20.5 && a.y < 26.5)).count();
    assert_eq!(inner, 4, "{o:?}");
    // The next glyph starts one advance later: an I after the 700 units of an H is 7 mm to the right.
    let hi_text = text_geometry_in(Some(&folder.font()), "HI", Vec2::ZERO, 7.0, 0.0).unwrap();
    let (lo, hi) = extent(&hi_text[4..]);
    assert!((lo.x - 7.0).abs() < 1e-9 && (hi.x - 8.0).abs() < 1e-9 && (hi.y - 7.0).abs() < 1e-9, "{lo:?} {hi:?}");
    // Without a font name the built-in one is used: its O is not a block.
    let builtin = text_geometry_in(None, "O", v(10.0, 20.0), 7.0, 0.0).unwrap();
    assert!(builtin.iter().all(|g| !matches!(g, LinkGeom::Line(..))));
    assert_eq!(builtin, text_geometry("O", v(10.0, 20.0), 7.0, 0.0).unwrap());
    // Turned by 90 degrees about its start, the 8 x 7 mm block stands 7 mm wide and 8 mm tall, left of the start.
    let turned = text_geometry_in(Some(&folder.font()), "O", Vec2::ZERO, 7.0, std::f64::consts::FRAC_PI_2).unwrap();
    let (lo, hi) = extent(&turned);
    assert!((lo.x + 7.0).abs() < 1e-9 && hi.x.abs() < 1e-9 && lo.y.abs() < 1e-9 && (hi.y - 8.0).abs() < 1e-9, "{lo:?} {hi:?}");
}

#[test]
fn a_font_without_a_capital_height_or_an_h_uses_seven_tenths_of_the_em() {
    let font = build_font("Bar", &[Glyph { ch: 'I', advance: 200, contours: vec![block(0, 0, 100, 700)] }]);
    let folder = Folder::new("no-h");
    let path = folder.0.join("bar.ttf");
    std::fs::write(&path, font).unwrap();
    // 0.7 em = 700 units: the 100 x 700 bar at a capital height of 14 mm is 2 x 14 mm.
    let bar = text_geometry_in(Some(&path.to_string_lossy()), "I", Vec2::ZERO, 14.0, 0.0).unwrap();
    let (lo, hi) = extent(&bar);
    assert!(lo.x.abs() < 1e-9 && lo.y.abs() < 1e-9 && (hi.x - 2.0).abs() < 1e-9 && (hi.y - 14.0).abs() < 1e-9, "{lo:?} {hi:?}");
}

#[test]
fn text_in_a_font_closes_into_profiles_with_counters() {
    let folder = Folder::new("profiles");
    let (at, height) = (v(10.0, 20.0), 7.0);
    let font = folder.font();
    let source =
        LinkSource::Text { text: "O".into(), at, height, angle: 0.0, font: Some(font.clone()), style: Default::default(), exprs: Default::default() };
    let mut sk = Sketch::new();
    sk.add_link(LinkKind::Text, source, &text_geometry_in(Some(&font), "O", at, height, 0.0).unwrap()).unwrap();
    // The block with the counter in it is 8 x 7 mm minus 4 x 3 mm; the counter is a region of its own.
    let mut areas: Vec<f64> = find_profiles(&sk).iter().map(|p| p.area).collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas.len(), 2, "{areas:?}");
    assert!((areas[0] - 12.0).abs() < 1e-9 && (areas[1] - 44.0).abs() < 1e-9, "{areas:?}");
}

#[test]
fn fonts_resolve_by_file_by_installed_name_or_to_the_built_in_one() {
    let folder = Folder::new("names");
    // No name, an empty one or the built-in family in any spelling: the built-in font.
    for name in [None, Some(""), Some("  "), Some("Liberation Sans"), Some("liberation-sans")] {
        assert_eq!(font_data(name).unwrap().bytes(), font_data(None).unwrap().bytes(), "{name:?}");
    }
    // A file by its path; its bytes are those of the file.
    let by_file = font_data(Some(&folder.font())).unwrap();
    assert_eq!(by_file.bytes(), tiny_font().as_slice());
    // A family by name in a font folder (searched below it, other files skipped), in any spelling; also by its full name.
    for name in ["Tiny Block", "tiny_block", "TINYBLOCK", "tiny-block", "Tiny Block Regular"] {
        let found = find_family_in(std::slice::from_ref(&folder.0), name).unwrap().unwrap_or_else(|| panic!("{name}"));
        assert_eq!(found.bytes(), tiny_font().as_slice(), "{name}");
    }
    assert!(find_family_in(std::slice::from_ref(&folder.0), "No Such Family").unwrap().is_none());
    assert!(find_family_in(&[folder.0.join("missing")], "Tiny Block").unwrap().is_none());
    // The second font of the repository, by its family name.
    let bundled = find_family_in(&[web_fonts()], "BIZ UDPGothic").unwrap().unwrap();
    assert_eq!(bundled.bytes(), std::fs::read(web_fonts().join("BIZUDPGothic-Regular.ttf")).unwrap().as_slice());
}

#[test]
fn unknown_fonts_and_unreadable_files_are_errors() {
    let folder = Folder::new("errors");
    let message = |name: &str| font_data(Some(name)).err().unwrap().to_string();
    assert!(message("No Such Family 12345").contains("unknown font `No Such Family 12345`"));
    assert!(message("C:/no/such/dir/font.ttf").contains("font file `C:/no/such/dir/font.ttf`"));
    assert!(message(&folder.0.join("notes.txt").to_string_lossy()).contains("font file"));
    assert!(message(&folder.0.join("broken.ttf").to_string_lossy()).contains("is not a TrueType/OpenType font"));
    assert!(message(&"x".repeat(2000)).contains("too long"));
    // The error reaches text_geometry_in, which makes no geometry from it.
    assert!(text_geometry_in(Some("No Such Family 12345"), "A", Vec2::ZERO, 8.0, 0.0).is_err());
}

#[test]
fn a_font_name_is_a_path_when_it_has_a_separator_or_a_font_extension() {
    let message = |name: &str| font_data(Some(name)).err().unwrap().to_string();
    // A name with a separator or a font extension is a file, whatever else it looks like; `%VAR%` is expanded in it and a
    // variable that is not set (or no variable at all) stays as written.
    assert!(message("%NO_SUCH_VAR_SOLIDCRAFT%/a.ttf").contains("font file `%NO_SUCH_VAR_SOLIDCRAFT%/a.ttf`"));
    assert!(message("100% sure.ttf").contains("font file `100% sure.ttf`"));
    assert!(message("Arial.TTF").contains("font file `Arial.TTF`"));
    assert!(message("fonts/a.otf").contains("font file `fonts/a.otf`"));
    // Anything else is a family name.
    assert!(message("No Such Family 12345").contains("unknown font"));
}

#[test]
fn a_text_that_the_font_makes_too_large_is_refused() {
    // A glyph of 300 corner points, 20 000 lines at most in a link: 66 of them are 19 800 lines, 67 are 20 100.
    let ring: Vec<(i16, i16)> =
        (0..300).map(|i| ((500.0 * (f64::from(i) * 0.0209).cos()) as i16, (500.0 * (f64::from(i) * 0.0209).sin()) as i16)).collect();
    let font = build_font("Many", &[Glyph { ch: 'H', advance: 100, contours: vec![ring] }]);
    let folder = Folder::new("large");
    let path = folder.0.join("many.ttf").to_string_lossy().into_owned();
    std::fs::write(&path, font).unwrap();
    assert_eq!(text_geometry_in(Some(&path), &"H".repeat(66), Vec2::ZERO, 5.0, 0.0).unwrap().len(), 66 * 300);
    assert!(matches!(text_geometry_in(Some(&path), &"H".repeat(67), Vec2::ZERO, 5.0, 0.0), Err(SketchError::TooLarge)));
    // Too large to draw, not to measure: 67 times one H.
    let font = font_data(Some(&path)).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    let (one, all) = (m.line_width("H", 5.0).unwrap(), m.line_width(&"H".repeat(67), 5.0).unwrap());
    assert!(one > 0.0 && (all - 67.0 * one).abs() < 1e-12 * all, "{one} {all}");
}

#[test]
fn a_font_file_is_measured_by_its_own_tables() {
    let folder = Folder::new("metrics");
    let font = font_data(Some(&folder.font())).unwrap();
    let m = TextMetrics::new(&font, &TextStyle::default()).unwrap();
    // Its H is 700 units tall, so at 7 mm a unit is 0.01 mm; ascender 800, descender -200 and no line gap put lines
    // 10 mm apart; H, I and O advance 700, 200 and 900 units.
    assert!((m.scale(7.0).unwrap() - 0.01).abs() < 1e-15);
    assert!((m.line_pitch(7.0).unwrap() - 10.0).abs() < 1e-12);
    assert!((m.line_width("HIO", 7.0).unwrap() - 18.0).abs() < 1e-12);
    // It has neither an x nor a `?`: an x is left out, taking no room and drawing nothing, but it is named.
    assert_eq!(m.line_width("HxI", 7.0).unwrap(), m.line_width("HI", 7.0).unwrap());
    assert_eq!(m.missing("xHx"), ['x', 'x']);
    let draw = |text: &str| text_geometry_in(Some(&folder.font()), text, Vec2::ZERO, 7.0, 0.0).unwrap();
    assert_eq!(draw("HxI"), draw("HI"));
    assert!(draw("xx").is_empty() && m.line_width("xx", 7.0).unwrap() == 0.0);
    // A face the file does not have (it is no collection) is an error.
    let mut elsewhere = font.clone();
    elsewhere.index = 1;
    assert!(TextMetrics::new(&elsewhere, &TextStyle::default()).is_err());
}

#[test]
fn a_font_is_kept_in_the_design_and_old_designs_read_without_one() {
    let source = |font: Option<&str>| LinkSource::Text {
        text: "I".into(),
        at: Vec2::ZERO,
        height: 5.0,
        angle: 0.0,
        font: font.map(str::to_string),
        style: Default::default(),
        exprs: Default::default(),
    };
    // No font: nothing is written, so the text of a design reads and writes as it did before fonts, and a design saved
    // before them reads as text in the built-in font.
    let plain = serde_json::to_string(&source(None)).unwrap();
    assert!(!plain.contains("font"), "{plain}");
    assert_eq!(serde_json::from_str::<LinkSource>(&plain).unwrap(), source(None));
    // A font name or path comes back as it was written.
    let json = serde_json::to_string(&source(Some("C:/fonts/Some Font.ttf"))).unwrap();
    assert!(json.contains(r#""font":"C:/fonts/Some Font.ttf""#), "{json}");
    assert_eq!(serde_json::from_str::<LinkSource>(&json).unwrap(), source(Some("C:/fonts/Some Font.ttf")));
}

#[test]
fn installed_families_are_listed_by_name_once_without_the_built_in_one() {
    let folder = Folder::new("families");
    // A second copy of the tiny font, the built-in font and the repository's second font: each family once,
    // sorted; the built-in family is not among the installed ones, broken files and other files are skipped.
    std::fs::write(folder.0.join("copy.ttf"), tiny_font()).unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/liberation-sans/LiberationSans-Regular.ttf"),
        folder.0.join("lib.ttf"),
    )
    .unwrap();
    std::fs::copy(web_fonts().join("BIZUDPGothic-Regular.ttf"), folder.0.join("biz.ttf")).unwrap();
    let families = crate::families_in(std::slice::from_ref(&folder.0));
    assert_eq!(families, ["BIZ UDPGothic", "Tiny Block"]);
    // Each name finds its font again.
    for family in &families {
        assert!(find_family_in(std::slice::from_ref(&folder.0), family).unwrap().is_some(), "{family}");
    }
    assert!(crate::families_in(&[folder.0.join("missing")]).is_empty());
}
