//! Every colour that carries a meaning comes from `theme::Tokens` (#778): the engine's sketch and
//! axis tags, the surface analysis colours and every colour an icon draws. These tests hold that
//! the tokens equal what was drawn before, and that a new colour cannot skip them.

use egui::Color32;
use solidcraft_engine::render::Rgb;
use solidcraft_engine::view::colors;

use crate::theme::Tokens;

fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// The new tokens hold exactly the values that were drawn in place before they were tokens, so
/// with no colour vision mode on, nothing on screen changes.
#[test]
fn moved_colours_keep_the_values_they_were_drawn_with() {
    for t in [Tokens::light(), Tokens::dark()] {
        assert_eq!(t.sketch_projected, rgb(colors::SKETCH_PROJECTED));
        assert_eq!(t.sketch_lost, rgb(colors::SKETCH_LOST));
        assert_eq!(t.sketch_construction, rgb(colors::SKETCH_CONSTRUCTION));
        assert_eq!(t.sketch_centerline, rgb(colors::SKETCH_CENTERLINE));
        assert_eq!(t.axes(), [rgb(colors::AXIS_X), rgb(colors::AXIS_Y), rgb(colors::AXIS_Z)]);
        assert_eq!(t.interference, Color32::from_rgb(235, 45, 40));
        // The shader's former literals, bit for bit.
        assert_eq!(t.analysis.draft, [[0.20, 0.75, 0.30], [0.95, 0.80, 0.15], [0.85, 0.22, 0.20]]);
        assert_eq!(t.analysis.access, [[0.20, 0.75, 0.30], [0.85, 0.22, 0.20]]);
        assert_eq!(t.analysis.curvature, [[0.08, 0.12, 0.45], [0.53, 0.51, 0.425], [0.98, 0.90, 0.40]]);
        assert!(!t.analysis.hatch);
        assert_eq!(t.logo, [Color32::from_rgb(96, 158, 236), Color32::from_rgb(58, 136, 238), Color32::from_rgb(55, 108, 197)]);
        assert_eq!(t.icons.marker_halo, Color32::from_black_alpha(150));
        assert_eq!(t.icons.agent, Color32::from_rgb(168, 85, 247));
    }
    let (l, d) = (Tokens::light(), Tokens::dark());
    assert_eq!((l.bend_up, l.bend_down), (Color32::from_rgb(30, 140, 50), Color32::from_rgb(200, 90, 20)));
    assert_eq!((d.bend_up, d.bend_down), (Color32::from_rgb(110, 200, 120), Color32::from_rgb(240, 150, 80)));
}

/// The solid colours of the lines and outlines in a frame (not fills, not missing strokes).
fn stroke_colours(shapes: &[egui::epaint::ClippedShape]) -> std::collections::BTreeSet<[u8; 4]> {
    fn walk(s: &egui::Shape, out: &mut std::collections::BTreeSet<[u8; 4]>) {
        let stroke = match s {
            egui::Shape::LineSegment { stroke, .. } => Some(stroke.color),
            egui::Shape::Path(p) => match p.stroke.color {
                egui::epaint::ColorMode::Solid(c) if p.stroke.width > 0.0 => Some(c),
                _ => None,
            },
            egui::Shape::Vec(v) => {
                v.iter().for_each(|s| walk(s, out));
                None
            }
            _ => None,
        };
        if let Some(c) = stroke.filter(|c| *c != Color32::TRANSPARENT) {
            out.insert(c.to_array());
        }
    }
    let mut out = std::collections::BTreeSet::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

/// Every axis is drawn in one set of X, Y and Z colours, the `axis_*` tokens (#976), in both
/// themes and every colour vision mode: the Move gizmo's arrows, squares and rings, the view
/// triad's ends, and the headless picture's origin axes, as the origin axes, the navigation
/// cube's triad, 3D sketch triads and joint origins draw them.
#[test]
fn every_axis_is_drawn_in_one_set_of_colours() {
    use crate::theme::{ColorVision, set_dark, set_vision};
    use egui::{Pos2, Rect, vec2};
    use solidcraft_engine::geom::Vec3;
    let ctx = crate::fonts::test_context();
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
    let cam = solidcraft_engine::render::Camera::default();
    let proj = crate::viewport::Proj { cam, vp: cam.view_proj(4.0 / 3.0, 100.0), rect };
    let triad = crate::gizmo::Triad { center: Vec3::ZERO, translate: true, rotate: true };
    for (dark, mode) in [false, true].into_iter().flat_map(|d| ColorVision::ALL.map(|m| (d, m))) {
        set_dark(dark);
        set_vision(mode);
        let t = Tokens::themed(dark, mode);
        let axes: std::collections::BTreeSet<[u8; 4]> = t.axes().iter().map(|c| c.to_array()).collect();
        assert_eq!(axes.len(), 3, "dark {dark}, {mode:?}: three different axis colours");

        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let painter = ui.painter().clone();
            crate::gizmo::show(ui, &painter, &proj, egui::Id::new("one_set_gizmo"), &triad, 1.0);
        });
        out.textures_delta.clear();
        let mut gizmo = stroke_colours(&out.shapes);
        gizmo.remove(&Color32::from_gray(60).to_array()); // the centre ball's neutral rim
        assert_eq!(gizmo, axes, "dark {dark}, {mode:?}: the Move gizmo");

        let c = Pos2::new(100.0, 100.0);
        for (name, d, want) in [("x", Vec3::X, t.axis_x), ("y", Vec3::Y, t.axis_y), ("z", Vec3::Z, t.axis_z)] {
            let at = c + vec2(32.0, 0.0);
            let mut out =
                ctx.run_ui(egui::RawInput::default(), |ui| crate::icons::navigation::axis_tip(ui.painter(), c, name, &d, at, 0.5, false, t));
            out.textures_delta.clear();
            let end = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Circle(e) if e.center == at => Some(e.fill),
                _ => None,
            });
            assert_eq!(end, Some(want), "dark {dark}, {mode:?}: the view triad's {name} end");
        }

        assert_eq!(
            crate::viewport::scene_palette(dark, mode).axes.map(|c| [c.0, c.1, c.2, 255]),
            t.axes().map(|c| c.to_array()),
            "dark {dark}, {mode:?}: the picture"
        );
    }
    set_dark(true);
    set_vision(ColorVision::Off);
}

/// The analysis colours reach the shader in the order its `ana_c` array reads them.
#[test]
fn analysis_colours_reach_the_shader_in_its_order() {
    let a = Tokens::light().analysis;
    let v = crate::gpu::analysis_colors(&a);
    let at = |i: usize| [v[4 * i], v[4 * i + 1], v[4 * i + 2]];
    assert_eq!([at(0), at(1), at(2)], a.draft);
    assert_eq!([at(3), at(4)], a.access);
    assert_eq!([at(5), at(6), at(7)], a.curvature);
    assert!((0..8).all(|i| v[4 * i + 3] == 1.0));
    assert_eq!(v[32], 0.0, "no stripes without a colour vision mode");
    let mut striped = a;
    striped.hatch = true;
    assert_eq!(crate::gpu::analysis_colors(&striped)[32], 1.0);
}

// Icons no longer take their colours from the caller or from `IconColors`: the SVG masters name
// palette roles (`assets/icons/ui/design/palette.toml`, light and dark, plus accents) that
// `icon_svg` resolves. `icon_svg`'s own tests hold that every master parses and that only the
// palette's colours exist, including its colour-vision variants.

/// The most colour literals each UI file may hold, and why. None of them carries a meaning that a
/// colour vision mode must change. A file with more fails here: make the new colour a token in
/// `theme::Tokens` (and `assets/styles/ui/palettes.rs`) instead, or, if it carries no meaning,
/// raise the file's count here and give the reason. Fewer is always fine. The icon sources may
/// hold none at all.
const ALLOWED_LITERALS: &[(&str, usize, &str)] = &[
    ("navigation.rs", 1, "the navigation cube's face shading, made from a token"),
    ("canvas.rs", 3, "white rings around manipulator handles"),
    ("context_menu.rs", 1, "the radial menu's shadow"),
    ("dialogs_appearance.rs", 1, "a design appearance: the colour is the fact"),
    ("dialogs_save_mesh.rs", 1, "an installed slicer's own icon, drawn untinted"),
    ("dialogs_library.rs", 1, "a library appearance's swatch: the colour is the fact"),
    ("render_ui/scene.rs", 2, "Browser appearance swatches: the colours are document data"),
    ("render_workspace_library.rs", 1, "a catalog appearance preview: the colour is authored data"),
    ("render_workspace_materials.rs", 1, "an authored material swatch: the displayed colour is the material data"),
    ("dialogs_assembly.rs", 1, "the interference fill, shaded from its token"),
    ("dialogs_motion.rs", 1, "a white outline"),
    ("gizmo.rs", 2, "the gizmo's neutral grey ball"),
    ("icons.rs", 1, "the white tint that carries an icon image's opacity"),
    ("ref_images.rs", 1, "a reference image's opacity tint"),
    ("theme_files.rs", 1, "a colour read from a theme file, which sets tokens"),
    ("titlebar.rs", 2, "the window close button's red hover (the system's convention) and white ink"),
    ("toolbar.rs", 1, "the version text"),
    ("locale_labels.rs", 3, "on-demand vector labels for CJK locales on web, rendered untinted with neutral text colours"),
];

const COLOUR_LITERALS: &[&str] = &[
    "Color32::from_rgb(",
    "Color32::from_rgba_unmultiplied(",
    "Color32::from_rgba_unmultiplied_const(",
    "Color32::from_rgba_premultiplied(",
    "Color32::from_gray(",
    "Color32::from_black_alpha(",
    "Color32::from_white_alpha(",
    "Color32::from_hex(",
    "Color32::WHITE",
    "Color32::BLACK",
    "Color32::RED",
    "Color32::GREEN",
    "Color32::BLUE",
    "Color32::YELLOW",
    "Color32::GOLD",
    "Color32::ORANGE",
    "Color32::BROWN",
    "Color32::KHAKI",
    "Color32::GRAY",
    "Color32::LIGHT_GRAY",
    "Color32::DARK_GRAY",
    "Color32::DARK_RED",
    "Color32::LIGHT_RED",
    "Color32::DARK_BLUE",
    "Color32::LIGHT_BLUE",
    "Color32::DARK_GREEN",
    "Color32::LIGHT_GREEN",
    "Color32::LIGHT_YELLOW",
];

#[test]
fn colours_outside_the_theme_are_only_the_listed_neutral_ones() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dirs = [root.join("src"), root.join("../../assets/icons/ui/source")];
    let mut found = Vec::new();
    fn scan(dir: &std::path::Path, prefix: &str, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            let relative = format!("{prefix}{name}");
            if path.is_dir() {
                scan(&path, &format!("{relative}/"), found);
                continue;
            }
            if !name.ends_with(".rs") || name.ends_with("_tests.rs") || matches!(name, "theme.rs" | "gallery.rs" | "test_support.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            let code = src.split("#[cfg(test)]").next().unwrap_or(&src);
            let n: usize = COLOUR_LITERALS.iter().map(|p| code.matches(p).count()).sum();
            let allowed = ALLOWED_LITERALS.iter().find(|(f, _, _)| *f == relative).map_or(0, |(_, n, _)| *n);
            if n > allowed {
                found.push(format!("{relative}: {n} colour literal(s), at most {allowed} allowed"));
            }
        }
    }
    for dir in &dirs {
        scan(dir, "", &mut found);
    }
    assert!(found.is_empty(), "colour literals outside theme::Tokens: {found:?}");
}

/// The analysis shader takes its colours from the uniform (and so from the tokens). Only the
/// zebra and environment map neutrals and direction vectors stay as literals.
#[test]
fn analysis_shader_colours_come_from_the_uniform() {
    let src = include_str!("../../../assets/shaders/viewport.wgsl");
    let start = src.find("fn analysis(").unwrap_or(0);
    let end = src.find("fn stripe(").unwrap_or(src.len());
    let body = &src[start..end];
    let mut literals = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("vec3<f32>(") {
        let tail = &rest[i..];
        let close = tail.find(')').unwrap_or(tail.len() - 1);
        literals.push(&tail[..=close]);
        rest = &tail[close..];
    }
    assert_eq!(
        literals,
        [
            "vec3<f32>(0.0, 0.0, 1.0)",
            "vec3<f32>(1.0, 0.0, 0.0)",
            "vec3<f32>(c)",
            "vec3<f32>(0.32, 0.34, 0.38)",
            "vec3<f32>(0.80, 0.86, 0.95)",
            "vec3<f32>(0.6)",
            "vec3<f32>(0.9)",
            "vec3<f32>(0.55, 0.56, 0.60)",
            "vec3<f32>(0.20, 0.20, 0.22)",
            "vec3<f32>(0.35)",
            "vec3<f32>(1.0)",
        ],
        "a new colour in the analysis shader must come from `ana_c` (theme::AnalysisColors)"
    );
    assert_eq!(body.matches("u.ana_c[").count(), 9, "every analysis colour reads `ana_c`");
}

/// Every colour the engine tags a scene line with is mapped to a token by the viewport, or carries
/// no meaning a colour vision mode must change. A new engine tag fails here until it is placed.
#[test]
fn every_engine_colour_tag_is_placed() {
    const MAPPED: &[&str] = &[
        "SKETCH",
        "SKETCH_DONE",
        "SKETCH_FIXED",
        "SKETCH_LOCKED",
        "SKETCH_PROJECTED",
        "SKETCH_LOST",
        "SKETCH_CONSTRUCTION",
        "SKETCH_CENTERLINE",
        "AXIS_X",
        "AXIS_Y",
        "AXIS_Z",
    ];
    // Bodies, edges, profiles, the grid and the background: the GPU viewport draws these from
    // their own tokens; the engine's values are only the headless snapshot's.
    const NEUTRAL: &[&str] = &["BODY", "BODY_SELECTED", "EDGE", "PROFILE", "GRID", "GRID_MAJOR", "BG_TOP", "BG_BOTTOM", "SECTION_CAP"];
    let src = include_str!("../../engine/src/view.rs");
    let start = src.find("pub mod colors {").unwrap_or(0);
    let block = &src[start..];
    let block = &block[..block.find("\n}").unwrap_or(block.len())];
    let mut names: Vec<&str> = block.lines().filter_map(|l| l.trim().strip_prefix("pub const ")).filter_map(|l| l.split(':').next()).collect();
    names.sort_unstable();
    let mut placed: Vec<&str> = MAPPED.iter().chain(NEUTRAL).copied().collect();
    placed.sort_unstable();
    assert_eq!(names, placed, "an engine colour tag must be mapped to a token in `viewport::sketch_color` or listed as neutral");
}

/// Preferences › Interface › Logo: the logo in shades of the theme's accent (the accent itself in
/// the middle, a lighter and a darker face), in a built-in theme and a theme file alike, and back
/// to the theme's own logo colours.
#[test]
fn the_logo_can_take_the_themes_accent() {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::theme::set_logo_accent(false);
        }
    }
    let _reset = Reset;
    let mut h = crate::scenario::Harness::new();
    for theme in ["dark", "gruvbox-dark"] {
        h.call("ui.prefs", serde_json::json!({"theme": theme, "logo_accent": false}));
        h.frames(1);
        let own = Tokens::get();
        h.call("ui.prefs", serde_json::json!({"logo_accent": true}));
        h.frames(1);
        let t = Tokens::get();
        let accent = t.accent.to_opaque();
        assert_eq!(t.logo[1], accent, "{theme}");
        let lum = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(lum(t.logo[0]) > lum(accent) && lum(t.logo[2]) < lum(accent), "{theme}: {:?}", t.logo);
        assert_eq!(Tokens { logo: own.logo, ..t }, own, "{theme}: only the logo changes");
        h.call("ui.prefs", serde_json::json!({"logo_accent": false}));
        h.frames(1);
        assert_eq!(Tokens::get().logo, own.logo, "{theme}: the theme's own again");
    }
}
