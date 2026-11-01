//! Colour vision modes (#778), measured. Colours that appear together must stay apart (CIEDE2000
//! 10 or more) for normal vision and for the loss each mode is for, with two simulations of that
//! loss: Machado, Oliveira and Fernandes 2009 (severity 1) and Brettel, Viénot and Mollon 1997.
//! The instruments are checked first: CIEDE2000 on the 34 pairs Sharma, Wu and Dalal published
//! with their paper, Brettel on white and its own anchors.
//!
//! When a token edit breaks a mode here, adjust its measured variant. Shared icon accents live
//! in `assets/icons/ui/design/palette.toml`; `assets/styles/ui/palettes.rs` links those generated
//! values to UI tokens and owns the remaining UI-only colour-vision adjustments.

use std::collections::BTreeSet;

use egui::Color32;
use serde_json::json;

use crate::theme::{ColorVision, Tokens};

type Rgb8 = [u8; 3];
type V3 = [f64; 3];
type M3 = [[f64; 3]; 3];

/// The smallest difference a pair may have (CIEDE2000).
const FLOOR: f64 = 10.0;
/// The shading the analysis colours get in the shader (`lit`: 0.55 turned away, 1 facing the eye).
const LIT: [f64; 4] = [0.55, 0.7, 0.85, 1.0];
const MODES: [ColorVision; 3] = [ColorVision::Protan, ColorVision::Deutan, ColorVision::Tritan];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Eye {
    Normal,
    Protan,
    Deutan,
    Tritan,
}

#[derive(Clone, Copy, Debug)]
enum Instrument {
    Machado,
    Brettel,
}

/// The loss a mode is for.
fn eye(mode: ColorVision) -> Eye {
    match mode {
        ColorVision::Off => Eye::Normal,
        ColorVision::Protan => Eye::Protan,
        ColorVision::Deutan => Eye::Deutan,
        ColorVision::Tritan => Eye::Tritan,
    }
}

// ---- the instruments ----

fn to_lin(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn mul(m: &M3, v: V3) -> V3 {
    [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

fn mat(a: &M3, b: &M3) -> M3 {
    [0, 1, 2].map(|r| [0, 1, 2].map(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

fn inv(m: &M3) -> M3 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
        [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
        [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det],
    ]
}

fn cross(u: V3, v: V3) -> V3 {
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

fn dot(u: V3, v: V3) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

/// Machado, Oliveira, Fernandes 2009, severity 1.0, on linear sRGB (the authors' Table 1).
fn machado(eye: Eye) -> M3 {
    match eye {
        Eye::Protan => [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
        Eye::Deutan => [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]],
        Eye::Tritan => [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.303900]],
        Eye::Normal => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    }
}

/// Linear sRGB to CIE XYZ (D65), IEC 61966-2-1.
const XYZ_FROM_LIN: M3 = [[0.4124564, 0.3575761, 0.1804375], [0.2126729, 0.7151522, 0.0721750], [0.0193339, 0.1191920, 0.9503041]];
/// XYZ to cone responses (LMS), Smith and Pokorny 1975.
const LMS_FROM_XYZ: M3 = [[0.15514, 0.54312, -0.03286], [-0.15514, 0.45684, 0.03286], [0.0, 0.0, 0.00801]];
/// CIE 1931 2-degree colour matching functions (CIE 018:2019) at Brettel's anchors.
const CIE_475: V3 = [0.1421, 0.1126, 1.0419];
const CIE_485: V3 = [0.05795001, 0.1693, 0.6162];
const CIE_575: V3 = [0.8425, 0.9154, 0.0018];
const CIE_660: V3 = [0.1649, 0.061, 0.0];

/// The missing cone (0 L, 1 M, 2 S) and the two anchor wavelengths of its half-planes.
fn anchors(eye: Eye) -> (usize, V3, V3) {
    match eye {
        Eye::Protan => (0, CIE_475, CIE_575),
        Eye::Deutan => (1, CIE_475, CIE_575),
        Eye::Normal | Eye::Tritan => (2, CIE_485, CIE_660),
    }
}

/// Brettel, Viénot, Mollon 1997: the missing cone's response is put back on one of two half-planes
/// through white and an anchor colour, chosen by the side of the white axis the colour is on.
fn brettel(eye: Eye, lin: V3) -> V3 {
    let lms = mat(&LMS_FROM_XYZ, &XYZ_FROM_LIN);
    let (c, w1, w2) = anchors(eye);
    let white = mul(&lms, [1.0; 3]);
    let (a1, a2) = (mul(&LMS_FROM_XYZ, w1), mul(&LMS_FROM_XYZ, w2));
    let mut axis = [0.0; 3];
    axis[c] = 1.0;
    let sep = cross(white, axis);
    let mut q = mul(&lms, lin);
    let n = if dot(q, sep) * dot(a1, sep) >= 0.0 { cross(white, a1) } else { cross(white, a2) };
    q[c] = -(0..3).filter(|k| *k != c).map(|k| n[k] * q[k]).sum::<f64>() / n[c];
    mul(&inv(&lms), q)
}

/// What an eye sees of an sRGB colour, as sRGB 0..1.
fn simulate(c: Rgb8, eye: Eye, how: Instrument) -> V3 {
    let srgb = c.map(|v| f64::from(v) / 255.0);
    if eye == Eye::Normal {
        return srgb;
    }
    let lin = srgb.map(to_lin);
    let out = match how {
        Instrument::Machado => mul(&machado(eye), lin),
        Instrument::Brettel => brettel(eye, lin),
    };
    out.map(|v| to_srgb(v.clamp(0.0, 1.0)))
}

/// CIELAB (D65) of an sRGB colour.
fn lab(srgb: V3) -> V3 {
    let [x, y, z] = mul(&XYZ_FROM_LIN, srgb.map(to_lin));
    let f = |t: f64| if t > (6.0f64 / 29.0).powi(3) { t.cbrt() } else { t / (3.0 * (6.0f64 / 29.0).powi(2)) + 4.0 / 29.0 };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 (Sharma, Wu, Dalal 2005).
fn de2000(l1: V3, l2: V3) -> f64 {
    let ([lightness1, a1, b1], [lightness2, a2, b2]) = (l1, l2);
    let (c1, c2) = (a1.hypot(b1), a2.hypot(b2));
    let cb = (c1 + c2) / 2.0;
    let g = 0.5 * (1.0 - (cb.powi(7) / (cb.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64, c: f64| if c == 0.0 { 0.0 } else { b.atan2(a).to_degrees().rem_euclid(360.0) };
    let (h1p, h2p) = (hue(b1, a1p, c1p), hue(b2, a2p, c2p));
    let dlp = lightness2 - lightness1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhp_big = 2.0 * (c1p * c2p).sqrt() * (dhp / 2.0).to_radians().sin();
    let lbp = (lightness1 + lightness2) / 2.0;
    let cbp = (c1p + c2p) / 2.0;
    let hbp = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let cos = |deg: f64| deg.to_radians().cos();
    let t = 1.0 - 0.17 * cos(hbp - 30.0) + 0.24 * cos(2.0 * hbp) + 0.32 * cos(3.0 * hbp + 6.0) - 0.20 * cos(4.0 * hbp - 63.0);
    let dtheta = 30.0 * (-((hbp - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cbp.powi(7) / (cbp.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lbp - 50.0).powi(2) / (20.0 + (lbp - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cbp;
    let sh = 1.0 + 0.015 * cbp * t;
    let rt = -(2.0 * dtheta).to_radians().sin() * rc;
    ((dlp / sl).powi(2) + (dcp / sc).powi(2) + (dhp_big / sh).powi(2) + rt * (dcp / sc) * (dhp_big / sh)).sqrt()
}

fn diff(a: Rgb8, b: Rgb8, eye: Eye, how: Instrument) -> f64 {
    de2000(lab(simulate(a, eye, how)), lab(simulate(b, eye, how)))
}

fn normal(a: Rgb8, b: Rgb8) -> f64 {
    diff(a, b, Eye::Normal, Instrument::Machado)
}

/// The smallest difference for normal vision and for the mode's loss (both instruments).
fn worst(mode: ColorVision, a: Rgb8, b: Rgb8) -> f64 {
    let e = eye(mode);
    normal(a, b).min(diff(a, b, e, Instrument::Machado)).min(diff(a, b, e, Instrument::Brettel))
}

// ---- the colours that appear together ----

fn rgb(c: Color32) -> Rgb8 {
    [c.r(), c.g(), c.b()]
}

/// A translucent colour over an opaque ground, in sRGB (close to egui's blend).
fn over(c: Color32, ground: Color32) -> Rgb8 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let k = f64::from(a) / 255.0;
    let mix = |f: u8, g: u8| (f64::from(f) * k + f64::from(g) * (1.0 - k)).round() as u8;
    [mix(r, ground.r()), mix(g, ground.g()), mix(b, ground.b())]
}

/// An analysis colour as the shader shades it.
fn lit(c: [f32; 3], k: f64) -> Rgb8 {
    c.map(|x| ((f64::from(x) * 255.0).round() * k).min(255.0).round() as u8)
}

struct Group {
    name: &'static str,
    /// Colours that must differ from each other.
    members: Vec<(&'static str, Rgb8)>,
    /// What they are drawn on.
    grounds: Vec<(&'static str, Rgb8)>,
}

fn group(name: &'static str, members: &[(&'static str, Color32)], grounds: &[(&'static str, Color32)]) -> Group {
    for (n, c) in members.iter().chain(grounds) {
        assert_eq!(c.a(), 255, "{name}: {n} is translucent; put it over its ground first");
    }
    Group { name, members: members.iter().map(|(n, c)| (*n, rgb(*c))).collect(), grounds: grounds.iter().map(|(n, c)| (*n, rgb(*c))).collect() }
}

/// Every group of meaning-carrying colours that appear together (and the icons' colours, drawn on
/// the toolbar with their caller's ink, fill and accent). Each colour is named by its `Tokens`
/// field, so `every_token_is_measured_or_carries_no_meaning` knows which tokens are measured.
fn groups(t: &Tokens) -> Vec<Group> {
    let lift = |c: Color32| {
        Color32::from_rgb(c.r().saturating_add(t.hover_face_lift), c.g().saturating_add(t.hover_face_lift), c.b().saturating_add(t.hover_face_lift))
    };
    let a = &t.analysis;
    let mut out = vec![
        group(
            "sketch lines",
            &[
                ("sketch", t.sketch),
                ("sketch_fixed", t.sketch_fixed),
                ("sketch_locked", t.sketch_locked),
                ("sketch_conflict", t.sketch_conflict),
                ("sketch_projected", t.sketch_projected),
                ("sketch_lost", t.sketch_lost),
                ("sketch_construction", t.sketch_construction),
                ("sketch_centerline", t.sketch_centerline),
                ("preview_sketch", t.preview_sketch),
            ],
            &[("viewport_top", t.viewport_top), ("viewport_bottom", t.viewport_bottom)],
        ),
        group(
            "sketch points",
            &[
                ("sketch_point", t.sketch_point),
                ("sketch_point_free", t.sketch_point_free),
                ("sketch_locked", t.sketch_locked),
                ("sketch_conflict", t.sketch_conflict),
            ],
            &[("viewport_top", t.viewport_top)],
        ),
        group(
            "timeline",
            &[("timeline_item", t.timeline_item), ("timeline_item_error", t.timeline_item_error), ("hover", t.hover), ("accent_soft", t.accent_soft)],
            &[],
        ),
        group("card text", &[("form_ink", t.form_ink), ("form_error", t.form_error)], &[("dialog_body", t.dialog_body)]),
        group("card hints", &[("form_dim", t.form_dim)], &[("dialog_body", t.dialog_body)]),
        group("card primary", &[("form_primary", t.form_primary)], &[("dialog_body", t.dialog_body)]),
        group("navigation status", &[("form_primary", t.form_primary), ("warning", t.warning)], &[("dialog_body", t.dialog_body)]),
        // Validation is identified by its words as well as colour. Measure both the
        // ink and badge surfaces against the corresponding failure states.
        group(
            "card validation",
            &[("form_success", t.form_success), ("form_error", t.form_error)],
            &[("form_success_fill", t.form_success_fill), ("form_error_fill", t.form_error_fill)],
        ),
        group("validation status", &[("form_success", t.form_success), ("error", t.error)], &[("panel", t.panel)]),
        group("validation surfaces", &[("form_success_fill", t.form_success_fill), ("form_error_fill", t.form_error_fill)], &[]),
        group("UI text", &[("error", t.error), ("warning", t.warning), ("text", t.text)], &[("panel", t.panel)]),
        // Every axis drawn: origin, view triad, navigation cube, Move gizmo… (one set, #976).
        group("axes", &[("axis_x", t.axis_x), ("axis_y", t.axis_y), ("axis_z", t.axis_z)], &[("viewport_top", t.viewport_top)]),
        group(
            "assembly Move",
            &[("joint_manipulator", t.joint_manipulator), ("joint_axis", t.joint_axis)],
            &[
                ("viewport_top", t.viewport_top),
                ("viewport_bottom", t.viewport_bottom),
                ("body", t.body),
                ("sel_face", t.sel_face),
                ("body_selected", t.body_selected),
            ],
        ),
        group("bend lines", &[("bend_up", t.bend_up), ("bend_down", t.bend_down)], &[("viewport_top", t.viewport_top)]),
        group("box select", &[("box_window", t.box_window), ("box_crossing", t.box_crossing)], &[("viewport_top", t.viewport_top)]),
        // Sketch dimensions (#7861): their lines and values, driving and driven, apart from the
        // curves they measure, the construction lines and the conflict red (each its own group:
        // those three are measured against each other elsewhere).
        group(
            "sketch dimensions",
            &[("dimension", t.dimension), ("dimension_driven", t.dimension_driven), ("sketch", t.sketch), ("sketch_fixed", t.sketch_fixed)],
            &[("viewport_top", t.viewport_top), ("viewport_bottom", t.viewport_bottom)],
        ),
        group(
            "dimensions and construction",
            &[("dimension", t.dimension), ("dimension_driven", t.dimension_driven), ("sketch_construction", t.sketch_construction)],
            &[],
        ),
        group("dimensions and conflicts", &[("dimension", t.dimension), ("dimension_driven", t.dimension_driven), ("error", t.error)], &[]),
        group(
            "selection",
            &[
                ("body", t.body),
                ("sel_face", t.sel_face),
                ("body_selected", t.body_selected),
                ("hover_face_lift", lift(t.body)),
                ("preview_add", t.preview_add),
                ("preview_new", t.preview_new),
            ],
            &[],
        ),
        group("section", &[("section_cap", t.section_cap)], &[("body", t.body)]),
        group("interference", &[("interference", t.interference)], &[("body", t.body)]),
        group(
            "continuity seams",
            &[
                ("continuity[0]", t.continuity[0]),
                ("continuity[1]", t.continuity[1]),
                ("continuity[2]", t.continuity[2]),
                ("continuity[3]", t.continuity[3]),
            ],
            &[("body", t.body), ("viewport_top", t.viewport_top)],
        ),
        // The scale's three stops, unlit. Along the scale, see
        // `the_curvature_scale_lightens_steadily_for_every_eye` (#975).
        group(
            "curvature map",
            &[
                ("analysis.curvature[0]", crate::theme::srgb(a.curvature[0])),
                ("analysis.curvature[1]", crate::theme::srgb(a.curvature[1])),
                ("analysis.curvature[2]", crate::theme::srgb(a.curvature[2])),
            ],
            &[],
        ),
        group(
            "ribbon accents",
            &[("glyph_text", t.glyph_text), ("sketch_accent", t.sketch_accent), ("construction_geom", t.construction_geom)],
            &[("toolbar", t.toolbar)],
        ),
        // Tab names and icons identify the tabs; each accent must read against the toolbar.
        group("surface tab", &[("surface_accent", t.surface_accent)], &[("toolbar", t.toolbar)]),
        group("mesh tab", &[("mesh_accent", t.mesh_accent)], &[("toolbar", t.toolbar)]),
        group("form tab", &[("form_accent", t.form_accent)], &[("toolbar", t.toolbar)]),
        group("agent status", &[("agent_ok", t.agent_ok), ("agent_busy", t.agent_busy)], &[("app_bar", t.app_bar)]),
        group("logo", &[("logo[0]", t.logo[0]), ("logo[1]", t.logo[1]), ("logo[2]", t.logo[2])], &[("app_bar", t.app_bar)]),
    ];
    // Tab fills and small-text inks have distinct jobs: compare each with its actual
    // grounds, rather than with the same-hue fill/ink. WCAG text ratios are also guarded
    // in theme::tests::tab_accents_hold_wcag_small_text_contrast_in_both_themes.
    for (name, ink_name, ink, soft_name, soft) in [
        ("sketch tab text", "sketch_ink", t.sketch_ink, "sketch_soft", t.sketch_soft),
        ("surface tab text", "surface_ink", t.surface_ink, "surface_soft", t.surface_soft),
        ("mesh tab text", "mesh_ink", t.mesh_ink, "mesh_soft", t.mesh_soft),
        ("form tab text", "form_tab_ink", t.form_tab_ink, "form_soft", t.form_soft),
        ("sheet tab text", "sheet_ink", t.sheet_ink, "sheet_soft", t.sheet_soft),
        ("render tab text", "render_ink", t.render_ink, "render_soft", t.render_soft),
    ] {
        out.push(group(name, &[(ink_name, ink)], &[(soft_name, soft), ("panel", t.panel)]));
    }
    for (name, key, fill) in [
        ("surface tab fill", "surface_fill", t.surface_fill),
        ("mesh tab fill", "mesh_fill", t.mesh_fill),
        ("form tab fill", "form_fill", t.form_fill),
        ("sheet tab fill", "sheet_fill", t.sheet_fill),
        ("render tab fill", "render_fill", t.render_fill),
    ] {
        out.push(group(name, &[(key, fill)], &[("panel", t.panel)]));
    }
    out.push(group("dark pill text", &[("tab_on_fill_dark", t.tab_on_fill_dark)], &[("mesh_fill", t.mesh_fill)]));
    // The removed part of a cut preview is translucent over the body.
    if let Some(sel) = out.iter_mut().find(|g| g.name == "selection") {
        sel.members.push(("preview_cut", over(t.preview_cut, t.body)));
    }
    out
}

/// Measured by the shaded check in `every_mode_keeps_colours_apart_for_its_eye`.
const SHADED: [&str; 2] = ["analysis.draft", "analysis.access"];

/// Tokens no colour vision mode needs to keep apart, and why. A new token fails
/// `every_token_is_measured_or_carries_no_meaning` until it is in a group above or here.
const NO_MEANING: &[(&str, &str)] = &[
    ("library_highlights", "user-assigned named labels, not application status; menu/tooltip name each colour and library tests guard card contrast"),
    ("nav_hover", "neutral hover surface; placement and pointer feedback identify the control"),
    ("nav_press", "neutral pressed/open surface; interaction and menu presence identify the state"),
    ("nav_dim", "neutral secondary ink; the Later label identifies the unavailable option"),
    ("nav_field", "neutral control surface, not a semantic category"),
    ("nav_tip", "neutral tooltip surface, not a semantic category"),
    ("nav_tip_ink", "neutral tooltip text; words identify the control and value"),
    ("nav_tip_key_border", "neutral keycap outline; its key label carries the meaning"),
    ("nav_viewport", "neutral preview-stage background, not model or status colour"),
    ("borders.primary", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.secondary", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.secondary_dotted", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.tertiary", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.container", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.control", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.primary_inset", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("borders.bevel", "structural chrome: hierarchy uses width, pattern and placement, not hue"),
    ("icon_palette", "optional SVG palette; actual roles and theme contrast are guarded in icon and theme tests"),
    ("file_dim", "File view secondary text, WCAG guarded in theme tests"),
    ("form_primary_fill", "ground under a filled primary field; form_primary carries the meaning, contrast guarded in theme tests"),
    ("file_recovery_well", "decorative recovery icon ground"),
    ("file_action_height", "layout dimension"),
    ("file_back_height", "layout dimension"),
    ("file_recovery_icon", "layout dimension"),
    ("file_grid_gap", "layout dimension"),
    ("file_nav_width", "layout dimension"),
    ("file_nav_height", "layout dimension"),
    ("file_tile_height", "layout dimension"),
    ("file_tile_icon", "layout dimension"),
    ("file_recent_height", "layout dimension"),
    ("file_sample_image_height", "layout dimension"),
    ("file_content_inset_x", "layout dimension"),
    ("file_content_inset_y", "layout dimension"),
    ("file_section_gap", "layout dimension"),
    ("font_start_title", "layout dimension"),
    ("file_background", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_surface", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_image_tint", "neutral image multiplication; model pixels retain their original colours"),
    ("file_border", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_rail", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_rail_ink", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_rail_hover", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_primary", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_primary_ink", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_primary_border", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_recovery", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_recovery_ink", "File view chrome; paired text contrast is guarded in theme tests"),
    ("file_recovery_border", "File view chrome; paired text contrast is guarded in theme tests"),
    ("space_2", "layout dimension"),
    ("space_3", "layout dimension"),
    ("space_4", "layout dimension"),
    ("space_6", "layout dimension"),
    ("space_8", "layout dimension"),
    ("space_10", "layout dimension"),
    ("space_12", "layout dimension"),
    ("space_14", "layout dimension"),
    ("space_16", "layout dimension"),
    ("space_24", "layout dimension"),
    ("radius_sm", "layout dimension"),
    ("radius_md", "layout dimension"),
    ("radius_lg", "layout dimension"),
    ("radius_xl", "layout dimension"),
    ("font_badge", "layout dimension"),
    ("font_micro", "layout dimension"),
    ("font_panel", "layout dimension"),
    ("font_small", "layout dimension"),
    ("font_caption", "layout dimension"),
    ("font_body", "layout dimension"),
    ("font_label", "layout dimension"),
    ("font_button", "layout dimension"),
    ("font_heading", "layout dimension"),
    ("font_triad", "layout dimension"),
    ("font_compass", "layout dimension"),
    ("font_subtitle", "layout dimension"),
    ("font_intro", "layout dimension"),
    ("font_article_heading", "layout dimension"),
    ("font_title", "layout dimension"),
    ("font_display", "layout dimension"),
    ("control_height", "layout dimension"),
    ("control_radius", "layout dimension"),
    ("form_label_width", "layout dimension"),
    ("form_gap_y", "layout dimension"),
    ("form_footer_height", "layout dimension"),
    ("form_header_height", "layout dimension"),
    ("form_card_width", "layout dimension"),
    ("form_icon_size", "layout dimension"),
    ("form_close_size", "layout dimension"),
    ("form_inset", "layout dimension"),
    ("form_icon_inset", "layout dimension"),
    ("form_section_icon_size", "layout dimension"),
    ("form_checkbox_height", "layout dimension"),
    ("form_button_height", "layout dimension"),
    ("sketch_palette_width", "layout dimension"),
    ("sketch_mode_size", "layout dimension"),
    ("sketch_mode_radius", "layout dimension"),
    ("sketch_mode_border", "layout dimension"),
    ("sketch_palette_alpha", "neutral panel opacity"),
    ("sketch_mode_tint", "selected background tint of the measured sketch accent"),
    ("sketch_footer_icon_size", "layout dimension"),
    ("font_section_tracking", "layout dimension"),
    ("on_accent", "ink on an accent fill"),
    ("app_bar_hover_subtle", "translucent hover chrome"),
    ("app_bar_hover_button", "translucent hover chrome"),
    ("app_bar_hover_strong", "translucent hover chrome"),
    ("app_bar_tab_active", "the active document tab's surface"),
    ("dialog_footer", "dense dialog footer band"),
    ("dialog_rule", "dense dialog divider"),
    ("control_fill", "input control fill"),
    ("control_border", "input control border"),
    ("dialog_label", "dense dialog field label"),
    ("selected_ink", "foreground of selected elements"),
    ("selected_bg", "background of selected elements"),
    ("render_accent", "the Render workspace's identity colour"),
    // The window's chrome: grounds, text and borders (measured as grounds where meaning sits on them).
    ("panel_header", "chrome"),
    ("icon", "neutral control ink; command artwork uses the SVG palette"),
    ("border", "chrome"),
    ("text_dim", "secondary text"),
    ("accent", "the one interface accent"),
    ("timeline", "the timeline's ground"),
    ("dialog_bg", "chrome"),
    ("dialog_border", "chrome"),
    ("chip", "a label's ground"),
    ("overlay", "a chip's ground over the view"),
    ("app_bar_text", "text"),
    ("app_bar_separator", "neutral chrome divider"),
    ("app_bar_hover", "a hovered tab, translucent white"),
    ("timeline_marker", "the rollback marker, alone"),
    ("field", "a field's ground"),
    // Highlights: shades of the accent blue over what they mark, by design close to each other.
    ("sel_edge", "selection highlight"),
    ("sel_edge_rim", "selection highlight"),
    ("hover_edge", "hover highlight"),
    ("hover_edge_halo", "hover highlight"),
    ("sel_profile", "selection highlight"),
    ("sel_profile_edge", "selection highlight"),
    ("hover_profile", "hover highlight"),
    ("hover_profile_edge", "hover highlight"),
    ("sel_vertex", "selection highlight"),
    ("rubber_band", "the shape being drawn"),
    ("manipulator", "a drag handle, alone"),
    // Planes, points, the grid and the model's edges: one neutral each, told apart by shape.
    ("origin_plane", "a plane, told apart by its place"),
    ("origin_plane_edge", "a plane's edge"),
    ("origin_plane_hover", "hover highlight"),
    ("origin_point", "the origin, alone"),
    ("construction_plane", "a plane"),
    ("body_edge", "the model's edges"),
    ("sketch_halo", "neutral rim beneath sketch curves; the curves carry the state colours"),
    ("grid", "the grid"),
    ("grid_major", "the grid"),
    ("profile_fill", "a closed profile's tint"),
    ("glyph_bg", "a constraint glyph's ground"),
    ("glyph_edge", "a constraint glyph's edge"),
    // The navigation cube: labelled faces.
    ("cube_face", "labelled"),
    ("cube_border", "labelled"),
    ("cube_text", "labelled"),
    ("cube_hover", "hover highlight"),
    // Not a colour.
    ("analysis.hatch", "a flag: stripes on the failing faces"),
    // Icon colours that are a shade of a measured one, or one mark alone.
    ("icons.highlight", "white ink on viewport markers and the agent pointer"),
    ("icons.agent", "the agent's pointer, alone"),
    ("icons.marker_halo", "a dark halo under a marker"),
];

/// Every token field, by path (`sketch`, `analysis.draft`, `triad`…): the arrays of distinct
/// meanings by element where only some elements are measured.
fn token_fields(t: &Tokens) -> BTreeSet<String> {
    let text = format!("{t:#?}");
    let mut out = BTreeSet::new();
    let mut parent: Option<String> = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let Some((name, rest)) = line.trim_start().split_once(": ") else { continue };
        match indent {
            4 if rest.ends_with('{') => parent = Some(name.to_string()),
            4 => {
                parent = None;
                out.insert(name.to_string());
            }
            8 => {
                if let Some(p) = &parent {
                    out.insert(format!("{p}.{name}"));
                }
            }
            _ => {}
        }
    }
    out
}

/// A group member's token field (`continuity[0]` is part of `continuity`).
fn field_of(name: &str) -> &str {
    name.split('[').next().unwrap_or(name)
}

#[test]
fn every_token_is_measured_or_carries_no_meaning() {
    for t in [Tokens::light(), Tokens::dark()] {
        let measured: BTreeSet<&str> =
            groups(&t).iter().flat_map(|g| g.members.iter().chain(&g.grounds).map(|(n, _)| field_of(n))).chain(SHADED).collect();
        // An element of an array (`triad[0]`) is listed for the record; its array is measured.
        let listed: BTreeSet<&str> = NO_MEANING.iter().map(|(n, _)| field_of(n)).collect();
        let fields = token_fields(&t);
        let unplaced: Vec<&String> = fields.iter().filter(|f| !measured.contains(f.as_str()) && !listed.contains(f.as_str())).collect();
        assert!(unplaced.is_empty(), "tokens neither measured (`groups`) nor listed in `NO_MEANING`: {unplaced:?}");
        let stale: Vec<&&str> = measured.iter().chain(&listed).filter(|f| !fields.contains(**f)).collect();
        assert!(stale.is_empty(), "names that are not token fields: {stale:?}");
        let both: Vec<&str> = NO_MEANING.iter().map(|(n, _)| *n).filter(|n| !n.contains('[') && measured.contains(n)).collect();
        assert!(both.is_empty(), "measured and also listed as carrying no meaning: {both:?}");
    }
}

/// Every pair in the groups: (group, name a, a, name b, b).
fn pairs(t: &Tokens) -> Vec<(String, Rgb8, Rgb8)> {
    let mut out = Vec::new();
    for g in groups(t) {
        for (i, (na, a)) in g.members.iter().enumerate() {
            for (nb, b) in g.members.iter().skip(i + 1).chain(&g.grounds) {
                out.push((format!("{}: {na} vs {nb}", g.name), *a, *b));
            }
        }
    }
    out
}

/// Pairs under the floor that the tables leave as they are (decided for #778). Each is close to
/// the floor for normal vision too, and construction and centerline lines also differ by their
/// dash patterns.
fn left_as_they_are(mode: ColorVision, dark: bool) -> BTreeSet<String> {
    let mut v = vec!["sketch lines: sketch_construction vs sketch_centerline"];
    if mode == ColorVision::Tritan {
        v.push("sketch lines: sketch_lost vs sketch_centerline");
        v.push(if dark { "timeline: hover vs accent_soft" } else { "timeline: timeline_item vs accent_soft" });
        v.push("selection: sel_face vs preview_add");
    }
    v.into_iter().map(String::from).collect()
}

// ---- the tests ----

/// Sharma, Wu, Dalal, "The CIEDE2000 Color-Difference Formula: Implementation Notes,
/// Supplementary Test Data, and Mathematical Observations", Color Research and Application 30
/// (2005), Table 1: L*, a*, b* of each pair, then the difference.
const SHARMA: [[f64; 7]; 34] = [
    [50.0000, 2.6772, -79.7751, 50.0000, 0.0000, -82.7485, 2.0425],
    [50.0000, 3.1571, -77.2803, 50.0000, 0.0000, -82.7485, 2.8615],
    [50.0000, 2.8361, -74.0200, 50.0000, 0.0000, -82.7485, 3.4412],
    [50.0000, -1.3802, -84.2814, 50.0000, 0.0000, -82.7485, 1.0000],
    [50.0000, -1.1848, -84.8006, 50.0000, 0.0000, -82.7485, 1.0000],
    [50.0000, -0.9009, -85.5211, 50.0000, 0.0000, -82.7485, 1.0000],
    [50.0000, 0.0000, 0.0000, 50.0000, -1.0000, 2.0000, 2.3669],
    [50.0000, -1.0000, 2.0000, 50.0000, 0.0000, 0.0000, 2.3669],
    [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0009, 7.1792],
    [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0010, 7.1792],
    [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0011, 7.2195],
    [50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0012, 7.2195],
    [50.0000, -0.0010, 2.4900, 50.0000, 0.0009, -2.4900, 4.8045],
    [50.0000, -0.0010, 2.4900, 50.0000, 0.0010, -2.4900, 4.8045],
    [50.0000, -0.0010, 2.4900, 50.0000, 0.0011, -2.4900, 4.7461],
    [50.0000, 2.5000, 0.0000, 50.0000, 0.0000, -2.5000, 4.3065],
    [50.0000, 2.5000, 0.0000, 73.0000, 25.0000, -18.0000, 27.1492],
    [50.0000, 2.5000, 0.0000, 61.0000, -5.0000, 29.0000, 22.8977],
    [50.0000, 2.5000, 0.0000, 56.0000, -27.0000, -3.0000, 31.9030],
    [50.0000, 2.5000, 0.0000, 58.0000, 24.0000, 15.0000, 19.4535],
    [50.0000, 2.5000, 0.0000, 50.0000, 3.1736, 0.5854, 1.0000],
    [50.0000, 2.5000, 0.0000, 50.0000, 3.2972, 0.0000, 1.0000],
    [50.0000, 2.5000, 0.0000, 50.0000, 1.8634, 0.5757, 1.0000],
    [50.0000, 2.5000, 0.0000, 50.0000, 3.2592, 0.3350, 1.0000],
    [60.2574, -34.0099, 36.2677, 60.4626, -34.1751, 39.4387, 1.2644],
    [63.0109, -31.0961, -5.8663, 62.8187, -29.7946, -4.0864, 1.2630],
    [61.2901, 3.7196, -5.3901, 61.4292, 2.2480, -4.9620, 1.8731],
    [35.0831, -44.1164, 3.7933, 35.0232, -40.0716, 1.5901, 1.8645],
    [22.7233, 20.0904, -46.6940, 23.0331, 14.9730, -42.5619, 2.0373],
    [36.4612, 47.8580, 18.3852, 36.2715, 50.5065, 21.2231, 1.4146],
    [90.8027, -2.0831, 1.4410, 91.1528, -1.6435, 0.0447, 1.4441],
    [90.9257, -0.5406, -0.9208, 88.6381, -0.8985, -0.7239, 1.5381],
    [6.7747, -0.2908, -2.4247, 5.8714, -0.0985, -2.2286, 0.6377],
    [2.0776, 0.0795, -1.1350, 0.9033, -0.0636, -0.5514, 0.9082],
];

#[test]
fn ciede2000_reproduces_sharmas_test_pairs() {
    for (i, p) in SHARMA.iter().enumerate() {
        let got = de2000([p[0], p[1], p[2]], [p[3], p[4], p[5]]);
        assert!((got - p[6]).abs() < 1e-4, "pair {}: {got}, published {}", i + 1, p[6]);
        // The formula is symmetric.
        assert!((de2000([p[3], p[4], p[5]], [p[0], p[1], p[2]]) - got).abs() < 1e-9, "pair {}", i + 1);
    }
}

#[test]
fn the_simulations_keep_white_and_brettels_anchors() {
    let lms = mat(&LMS_FROM_XYZ, &XYZ_FROM_LIN);
    for e in [Eye::Protan, Eye::Deutan, Eye::Tritan] {
        let w = brettel(e, [1.0; 3]);
        assert!(w.iter().all(|v| (v - 1.0).abs() < 1e-9), "{e:?}: white -> {w:?}");
        let (_, a1, a2) = anchors(e);
        for a in [a1, a2] {
            let lin = mul(&inv(&lms), mul(&LMS_FROM_XYZ, a));
            let back = brettel(e, lin);
            assert!(lin.iter().zip(back).all(|(x, y)| (x - y).abs() < 1e-9), "{e:?}: anchor {lin:?} -> {back:?}");
        }
        let m = machado(e);
        assert!(m.iter().all(|row| (row.iter().sum::<f64>() - 1.0).abs() < 1e-5), "{e:?}: Machado keeps white");
    }
    // Known losses: pure red and green merge for a red-green loss, blue and green for a blue one.
    let (red, green, blue) = ([220, 60, 60], [60, 170, 60], [60, 100, 230]);
    assert!(normal(red, green) > 60.0);
    for how in [Instrument::Machado, Instrument::Brettel] {
        assert!(diff(red, green, Eye::Deutan, how) < 10.0, "{how:?}");
        assert!(diff(blue, green, Eye::Tritan, how) < diff(blue, green, Eye::Normal, how), "{how:?}");
    }
}

#[test]
fn off_gives_the_theme_unchanged() {
    assert_eq!(Tokens::themed(false, ColorVision::Off), Tokens::light());
    assert_eq!(Tokens::themed(true, ColorVision::Off), Tokens::dark());
    for dark in [false, true] {
        assert!(!Tokens::themed(dark, ColorVision::Off).analysis.hatch);
        // Protan and Deutan share the red-green table.
        assert_eq!(Tokens::themed(dark, ColorVision::Protan), Tokens::themed(dark, ColorVision::Deutan));
    }
}

/// The token fields whose `Debug` text differs (`continuity`, `analysis.draft`…), so a field cannot
/// change in a mode by accident.
fn changed_fields(a: &Tokens, b: &Tokens) -> BTreeSet<String> {
    let (la, lb) = (format!("{a:#?}"), format!("{b:#?}"));
    let (la, lb): (Vec<&str>, Vec<&str>) = (la.lines().collect(), lb.lines().collect());
    assert_eq!(la.len(), lb.len());
    let mut out = BTreeSet::new();
    for i in (0..la.len()).filter(|i| la[*i] != lb[*i]) {
        // The named fields above this line, innermost first, up to a field of `Tokens`.
        let mut path = Vec::new();
        let mut indent = usize::MAX;
        for line in la[..=i].iter().rev() {
            let here = line.len() - line.trim_start().len();
            if here < indent
                && let Some((name, _)) = line.trim_start().split_once(':')
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                path.push(name.to_string());
                indent = here;
            }
            if indent <= 4 {
                break;
            }
        }
        path.reverse();
        out.insert(path.join("."));
    }
    out
}

#[test]
fn each_mode_changes_only_its_table() {
    let set = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>();
    let red_green_both = [
        "axis_x",
        "axis_y",
        "analysis.draft",
        "analysis.access",
        "analysis.hatch",
        "bend_up",
        "bend_down",
        "surface_accent",
        "mesh_accent",
        "form_accent",
        "sketch_accent",
        "preview_sketch",
        "form_success_fill",
        "sketch_conflict",
    ];
    let mut red_green_light =
        red_green_both.into_iter().filter(|name| !["surface_accent", "mesh_accent", "form_accent"].contains(name)).collect::<Vec<_>>();
    red_green_light.extend(["sketch_projected", "sketch_locked", "timeline_item_error", "construction_geom", "glyph_text", "form_success"]);
    for (dark, mode, want) in [
        (false, ColorVision::Protan, set(&red_green_light)),
        (true, ColorVision::Protan, set(&red_green_both)),
        (false, ColorVision::Deutan, set(&red_green_light)),
        (true, ColorVision::Deutan, set(&red_green_both)),
        (
            false,
            ColorVision::Tritan,
            set(&[
                "surface_accent",
                "mesh_accent",
                "form_accent",
                "agent_busy",
                "sketch_locked",
                "box_crossing",
                "construction_geom",
                "glyph_text",
                "preview_sketch",
                "sketch_conflict",
                "analysis.hatch",
            ]),
        ),
        (
            true,
            ColorVision::Tritan,
            set(&[
                "surface_accent",
                "mesh_accent",
                "form_accent",
                "agent_busy",
                "sketch_locked",
                "box_crossing",
                "construction_geom",
                "glyph_text",
                "preview_sketch",
                "sketch_conflict",
                "analysis.hatch",
            ]),
        ),
    ] {
        let base = Tokens::themed(dark, ColorVision::Off);
        assert_eq!(changed_fields(&base, &Tokens::themed(dark, mode)), want, "{mode:?}, dark {dark}");
    }
}

#[test]
fn every_mode_keeps_colours_apart_for_its_eye() {
    let mut report = Vec::new();
    for dark in [false, true] {
        let off = Tokens::themed(dark, ColorVision::Off);
        for mode in MODES {
            let on = Tokens::themed(dark, mode);
            let mut left = BTreeSet::new();
            for ((name, a0, b0), (_, a1, b1)) in pairs(&off).into_iter().zip(pairs(&on)) {
                // Close for everybody by design (hover tints, a shade of one hue), and not changed
                // by the mode: not a colour vision fault.
                if normal(a0, b0) < FLOOR && (a0, b0) == (a1, b1) {
                    continue;
                }
                let w = worst(mode, a1, b1);
                if w < FLOOR {
                    report.push(format!("{mode:?} dark={dark} {name}: {w:.1} (before {:.1})", worst(mode, a0, b0)));
                    left.insert(name);
                }
            }
            assert_eq!(left, left_as_they_are(mode, dark), "{mode:?}, dark {dark}: {report:#?}");
            // Draft and Accessibility, for faces at any angle to the light against each other.
            let a = &on.analysis;
            let shaded = [(a.draft[0], a.draft[1]), (a.draft[0], a.draft[2]), (a.draft[1], a.draft[2]), (a.access[0], a.access[1])];
            for (p, q) in shaded {
                for x in LIT {
                    for y in LIT {
                        let w = worst(mode, lit(p, x), lit(q, y));
                        assert!(w >= FLOOR, "{mode:?}, dark {dark}: {p:?} lit {x} vs {q:?} lit {y}: {w:.1}");
                    }
                }
            }
        }
    }
}

/// Badge meaning must survive each simulation even for subtle default tints (which
/// the general groups check may otherwise skip when they are close for normal vision).
#[test]
fn validation_badges_are_distinct_and_readable_in_every_mode() {
    let mut failures = Vec::new();
    for dark in [false, true] {
        for mode in std::iter::once(ColorVision::Off).chain(MODES) {
            let t = Tokens::themed(dark, mode);
            for (name, other) in [("form_error", t.form_error), ("error", t.error)] {
                let distance = worst(mode, rgb(t.form_success), rgb(other));
                if distance < FLOOR {
                    failures.push(format!("{mode:?} dark={dark} form_success vs {name}: {distance:.2}"));
                }
            }
            let distance = worst(mode, rgb(t.form_success_fill), rgb(t.form_error_fill));
            if distance < FLOOR {
                failures.push(format!("{mode:?} dark={dark} validation fills: {distance:.2}"));
            }
            let contrast = crate::theme::contrast_ratio(t.form_success, t.form_success_fill);
            if contrast < 4.5 {
                failures.push(format!("{mode:?} dark={dark} success contrast: {contrast:.2}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every shipped theme draws sketch dimensions in a colour of their own (#7861): apart from the
/// sketch curves, construction lines, the conflict red and the hover and selection accent, and
/// readable on the viewport — a driving value as text, a driven one as a dimmed line. The
/// two-colour themes (1-bit) tell them apart by shape alone.
#[test]
fn shipped_themes_draw_dimensions_in_their_own_readable_colour() {
    use crate::theme::contrast_ratio;
    use crate::theme_files::{builtin, is_two_colour, resolve};
    let themes = [("light", Tokens::light()), ("dark", Tokens::dark())]
        .into_iter()
        .map(|(id, t)| (id.to_string(), t))
        .chain(builtin().iter().filter(|f| !is_two_colour(&f.id)).map(|f| (f.id.clone(), resolve(f))));
    let mut failures = Vec::new();
    for (id, t) in themes {
        for (name, dim, min_contrast) in [("dimension", t.dimension, 4.5), ("dimension_driven", t.dimension_driven, 3.0)] {
            for (ground, g) in [("viewport_top", t.viewport_top), ("viewport_bottom", t.viewport_bottom)] {
                let c = contrast_ratio(dim, g);
                if c < min_contrast {
                    failures.push(format!("{id}: {name} on {ground}: contrast {c:.2}"));
                }
            }
            let others = [
                ("sketch", t.sketch),
                ("sketch_fixed", t.sketch_fixed),
                ("sketch_locked", t.sketch_locked),
                ("sketch_projected", t.sketch_projected),
                ("sketch_construction", t.sketch_construction),
                ("error", t.error),
                ("accent", t.accent),
            ];
            for (other, o) in others {
                let d = normal(rgb(dim), rgb(o));
                if d < FLOOR {
                    failures.push(format!("{id}: {name} vs {other}: {d:.1}"));
                }
            }
        }
        let d = normal(rgb(t.dimension), rgb(t.dimension_driven));
        if d < FLOOR {
            failures.push(format!("{id}: dimension vs dimension_driven: {d:.1}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_shader_gets_the_modes_colours_and_its_stripes() {
    for dark in [false, true] {
        let off = crate::gpu::analysis_colors(&Tokens::themed(dark, ColorVision::Off).analysis);
        assert_eq!(off[32], 0.0, "no stripes in Off");
        for mode in MODES {
            let t = Tokens::themed(dark, mode);
            let v = crate::gpu::analysis_colors(&t.analysis);
            assert_eq!(v[32], 1.0, "{mode:?}: stripes on the bad faces");
            assert_eq!(v[..3], t.analysis.draft[0], "{mode:?}");
            assert_eq!(v[16..19], t.analysis.access[1], "{mode:?}");
        }
    }
}

// ---- the preference ----

/// Every message the Colour vision section shows (the heading, the mode names and their
/// tooltips, the sample's names and the note) is in every catalogue, translated: in Japanese,
/// Korean, Russian and Chinese none is the English text. (German keeps "Protan", "Deutan" and
/// "Tritan".)
#[test]
fn the_sections_messages_are_in_every_catalog() {
    let load = |text: &str| -> serde_json::Map<String, serde_json::Value> { serde_json::from_str(text).unwrap() };
    let english = load(include_str!(concat!(env!("OUT_DIR"), "/locales/en.json")));
    let ids: Vec<&str> = ColorVision::ALL
        .into_iter()
        .flat_map(|m| [m.label_id(), m.help_id()])
        .chain(crate::ui_kit::COLOUR_VISION_SAMPLE)
        .chain(["ui.colour_vision", "ui.colour_vision_mode", "ui.colour_vision_sample", "ui.colour_vision_note"])
        .collect();
    for (lang, text) in [
        ("cs", include_str!(concat!(env!("OUT_DIR"), "/locales/cs.json"))),
        ("da", include_str!(concat!(env!("OUT_DIR"), "/locales/da.json"))),
        ("de", include_str!(concat!(env!("OUT_DIR"), "/locales/de.json"))),
        ("ja", include_str!(concat!(env!("OUT_DIR"), "/locales/ja.json"))),
        ("ko", include_str!(concat!(env!("OUT_DIR"), "/locales/ko.json"))),
        ("nb", include_str!(concat!(env!("OUT_DIR"), "/locales/nb.json"))),
        ("pl", include_str!(concat!(env!("OUT_DIR"), "/locales/pl.json"))),
        ("ru", include_str!(concat!(env!("OUT_DIR"), "/locales/ru.json"))),
        ("zh", include_str!(concat!(env!("OUT_DIR"), "/locales/zh.json"))),
    ] {
        let catalog = load(text);
        for id in &ids {
            let (Some(en), Some(here)) = (english.get(*id), catalog.get(*id)) else {
                panic!("{lang}: missing message {id} (English: {})", english.contains_key(*id));
            };
            // Danish and Norwegian use the same established names for these colour-vision modes.
            let shared_name =
                matches!(lang, "da" | "nb") && ["ui.colour_vision_deutan", "ui.colour_vision_protan", "ui.colour_vision_tritan"].contains(id);
            assert!(lang == "de" || shared_name || here != en, "{lang}: {id} is the English text {en}");
        }
    }
}

fn app() -> crate::SolveApp {
    crate::SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default())
}

#[test]
fn saved_preferences_load_the_mode_and_a_bad_one_loads_as_off() {
    use crate::prefs::Prefs;
    // Saved before the mode existed.
    let old: Prefs = serde_json::from_value(json!({"theme": "light"})).unwrap();
    assert_eq!((old.color_vision, old.theme.as_str()), (ColorVision::Off, "light"));
    for mode in ColorVision::ALL {
        let p = Prefs { color_vision: mode, ..Prefs::default() };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["color_vision"], mode.id());
        assert_eq!(serde_json::from_value::<Prefs>(v).unwrap(), p);
    }
    // A value this version doesn't know (a later SolidCraft's mode, a hand edit) is Off, and the
    // rest of the preferences still load.
    for bad in [json!("achromat"), json!("PROTAN"), json!(""), json!(3), json!(null), json!(true), json!({"protan": 1}), json!(["tritan"])] {
        let p: Prefs = serde_json::from_value(json!({"theme": "light", "color_vision": bad})).unwrap();
        assert_eq!((p.color_vision, p.theme.as_str()), (ColorVision::Off, "light"), "{bad}");
    }
}

#[test]
fn ui_prefs_sets_each_mode_and_refuses_anything_else() {
    let mut app = app();
    for mode in ColorVision::ALL {
        let out = crate::prefs::set(&mut app, &json!({"color_vision": mode.id()})).unwrap();
        assert_eq!(out["color_vision"], mode.id());
        assert_eq!(app.preferences.color_vision, mode);
    }
    let long = "p".repeat(1 << 20);
    for bad in [
        json!("purple"),
        json!("Protan"),
        json!(" protan"),
        json!(""),
        json!(long),
        json!(2),
        json!(-1.5),
        json!(null),
        json!(false),
        json!([]),
        json!({}),
    ] {
        let before = app.preferences.clone();
        let err = crate::prefs::set(&mut app, &json!({"color_vision": bad, "theme": "light"})).unwrap_err();
        assert!(err.contains("off, protan, deutan or tritan"), "{err}");
        assert_eq!(app.preferences, before, "a refused value changes nothing");
    }
}

/// Random values (fixed seed): strings near the four ids and far from them, numbers, nesting.
/// `ui.prefs` takes exactly the four ids and refuses the rest without a change; saved
/// preferences load every value, unknown ones as Off. Nothing panics.
/// The section previews the mode being chosen (#7152): the sample's lines take that mode's
/// colours. The status chips beside it keep theirs in every mode.
#[test]
fn the_sample_previews_the_mode_being_chosen() {
    let mut h = crate::scenario::Harness::new();
    crate::prefs::control(&mut h.app, &json!({"open": "Interface"})).unwrap();
    let lines = |h: &crate::scenario::Harness| {
        let mut out = BTreeSet::new();
        for s in &h.shapes {
            crate::test_support::each_shape(&s.shape, &mut |shape| {
                if let egui::Shape::LineSegment { stroke, .. } = shape {
                    out.insert(stroke.color.to_array());
                }
            });
        }
        out
    };
    for mode in [ColorVision::Protan, ColorVision::Tritan, ColorVision::Off] {
        crate::prefs::control(&mut h.app, &json!({"color_vision": mode.id()})).unwrap();
        h.frames(2);
        let (want, off) = (Tokens::get_in(mode), Tokens::get_in(ColorVision::Off));
        let painted = lines(&h);
        for (name, c) in [("axis_x", want.axis_x), ("axis_y", want.axis_y), ("sketch_locked", want.sketch_locked)] {
            assert!(painted.contains(&c.to_array()), "{mode:?}: the sample draws {name} {c:?}");
        }
        if mode != ColorVision::Off {
            let changed = [(want.axis_x, off.axis_x), (want.sketch_locked, off.sketch_locked)];
            assert!(changed.iter().any(|(a, b)| a != b), "{mode:?} recolours what the sample shows");
        }
    }
    crate::prefs::control(&mut h.app, &json!({"button": "cancel"})).unwrap();
}

#[test]
fn random_values_never_panic_and_only_the_four_ids_are_taken() {
    fn value(next: &mut impl FnMut() -> u64, depth: u32) -> serde_json::Value {
        let ids = ["off", "protan", "deutan", "tritan"];
        match next() % if depth > 3 { 6 } else { 8 } {
            0 => json!(ids[(next() % 4) as usize]),
            1 => {
                // A near miss: one id with a character changed, added or taken away.
                let mut s: Vec<char> = ids[(next() % 4) as usize].chars().collect();
                let i = (next() % s.len() as u64) as usize;
                match next() % 3 {
                    0 => s[i] = char::from_u32((next() % 0x2FFF) as u32).unwrap_or('?'),
                    1 => s.insert(i, ['\0', ' ', 'É', '\u{301}', 'ـ'][(next() % 5) as usize]),
                    _ => {
                        s.remove(i);
                    }
                }
                json!(s.into_iter().collect::<String>())
            }
            2 => json!((0..next() % 40).map(|_| char::from_u32((next() % 0x10FFFF) as u32).unwrap_or('x')).collect::<String>()),
            3 => json!(next() as i64),
            4 => json!(Some(f64::from_bits(next())).filter(|x| x.is_finite()).unwrap_or(0.5)),
            5 => [json!(null), json!(true), json!(false)][(next() % 3) as usize].clone(),
            6 => json!((0..next() % 4).map(|_| value(next, depth + 1)).collect::<Vec<_>>()),
            _ => json!({"color_vision": value(next, depth + 1)}),
        }
    }
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut app = app();
    let mut taken = 0;
    for _ in 0..4000 {
        let v = value(&mut next, 0);
        let id = v.as_str().and_then(ColorVision::from_id);
        let before = app.preferences.clone();
        match crate::prefs::set(&mut app, &json!({"color_vision": v})) {
            Ok(_) => {
                taken += 1;
                assert_eq!(Some(app.preferences.color_vision), id, "{v}");
            }
            Err(e) => {
                assert!(id.is_none() && e.contains("off, protan, deutan or tritan"), "{v}: {e}");
                assert_eq!(app.preferences, before, "{v}");
            }
        }
        let saved: crate::prefs::Prefs = serde_json::from_value(json!({"theme": "light", "color_vision": v})).unwrap();
        assert_eq!((saved.color_vision, saved.theme.as_str()), (id.unwrap_or_default(), "light"), "{v}");
    }
    assert!(taken > 200, "the four ids came up {taken} times");
}

#[test]
fn cancel_puts_the_mode_back_ok_keeps_it_and_restore_defaults_resets_interface_only() {
    let mut app = app();
    let control = |app: &mut crate::SolveApp, p: serde_json::Value| crate::prefs::control(app, &p).unwrap();
    control(&mut app, json!({"open": "Interface"}));
    control(&mut app, json!({"color_vision": "protan"}));
    assert_eq!(app.preferences.color_vision, ColorVision::Protan);
    control(&mut app, json!({"button": "cancel"}));
    assert_eq!(app.preferences.color_vision, ColorVision::Off);
    control(&mut app, json!({"open": "Interface"}));
    control(&mut app, json!({"color_vision": "tritan", "button": "ok"}));
    assert_eq!(app.preferences.color_vision, ColorVision::Tritan);
    // Restore Defaults on another page leaves it.
    control(&mut app, json!({"open": "General"}));
    control(&mut app, json!({"button": "defaults"}));
    assert_eq!(app.preferences.color_vision, ColorVision::Tritan);
    control(&mut app, json!({"open": "Interface"}));
    control(&mut app, json!({"button": "defaults"}));
    assert_eq!(app.preferences.color_vision, ColorVision::Off);
    control(&mut app, json!({"button": "ok"}));
}

/// #975: the Curvature Map reads from flat to tight with hue lost too. Along the shader's two
/// blends its lightness rises the whole way, and each quarter of the scale is a clear step, for
/// normal vision and each simulated loss. The former blue, green, red scale rose and fell.
#[test]
fn the_curvature_scale_lightens_steadily_for_every_eye() {
    let c = Tokens::light().analysis.curvature;
    // Flat to middle over the first half, middle to tight over the second, as the shader mixes.
    let at = |t: f64| -> Rgb8 {
        let (a, b, u) = if t < 0.5 { (c[0], c[1], t * 2.0) } else { (c[1], c[2], t * 2.0 - 1.0) };
        std::array::from_fn(|k| ((f64::from(a[k]) + (f64::from(b[k]) - f64::from(a[k])) * u) * 255.0).round().clamp(0.0, 255.0) as u8)
    };
    for eye in [Eye::Normal, Eye::Protan, Eye::Deutan, Eye::Tritan] {
        let labs: Vec<V3> = (0..=4).map(|i| lab(simulate(at(f64::from(i) / 4.0), eye, Instrument::Machado))).collect();
        for w in labs.windows(2) {
            assert!(w[1][0] > w[0][0], "{eye:?}: the lightness falls along the scale: {labs:?}");
            let step = de2000(w[0], w[1]);
            assert!(step >= 10.0, "{eye:?}: a quarter of the scale is only {step:.1} apart: {labs:?}");
        }
    }
}
