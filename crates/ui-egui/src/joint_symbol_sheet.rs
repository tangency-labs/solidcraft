//! An overview sheet of the joint symbols: every joint type in every `joint_symbols` style, and
//! how a revolute joint draws when its parts can't move, when suppressed, with a limit, picked
//! and piled on another, in either theme. Drawn by the same code as the viewport and written as
//! SVG when `SOLIDCRAFT_JOINT_SHEET` names a folder:
//!
//! ```text
//! SOLIDCRAFT_JOINT_SHEET=target/joint-sheet cargo test -p solidcraft-ui-egui --lib joint_symbol_sheet
//! ```

use egui::{Color32, Pos2, Shape, pos2, vec2};
use solidcraft_engine::doc::joints::JointKind;

use crate::dialogs_assembly::{JointGlyph, TYPES, kind_label, lit_strokes, ring_shapes};
use crate::joint_symbols::{Family, Mood, RING_AND_AXIS, STYLES, badge, badge_shapes, family};
use solidcraft_theme::Tokens;

const LABEL: f32 = 190.0;
const COLUMN: f32 = 92.0;
const ROW: f32 = 58.0;
const HEAD: f32 = 74.0;
/// Half the ring and axis's motion axis, as a joint 5 % of the view height tall would draw in a
/// view about 500 points high.
const LEN: f64 = 24.0;

/// A row of the sheet: a joint type, or a revolute joint in one of the states.
#[derive(Clone, Copy)]
enum Row {
    Type(JointKind),
    Quiet,
    Suppressed,
    Limit,
    Picked,
    Faint,
}

impl Row {
    /// The row's name and the dim note under it.
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Row::Type(kind) => (
                kind_label(kind),
                match family(kind) {
                    Family::Fixed => "fixed",
                    Family::Motion => "motion",
                    Family::Relation => "orientation",
                    Family::Contact => "contact",
                },
            ),
            Row::Quiet => ("Can't move", "parts fully constrained"),
            Row::Suppressed => ("Suppressed", ""),
            Row::Limit => ("With a limit", ""),
            Row::Picked => ("Picked", "in the browser"),
            Row::Faint => ("Piled up", "lies on another joint"),
        }
    }

    /// The joint type the row draws; the state rows draw a revolute joint.
    fn kind(self) -> JointKind {
        match self {
            Row::Type(kind) => kind,
            _ => JointKind::Revolute,
        }
    }
}

/// Every joint type in the Type box's order, then the states.
fn rows() -> Vec<Row> {
    let types = TYPES.iter().filter_map(|t| JointKind::parse(t)).map(Row::Type);
    types.chain([Row::Quiet, Row::Suppressed, Row::Limit, Row::Picked, Row::Faint]).collect()
}

/// The shapes one cell draws, centred on `c`, as `joint_glyphs` would draw them.
fn cell(style: u8, row: Row, c: Pos2, t: &Tokens) -> Vec<Shape> {
    let kind = row.kind();
    let picked = matches!(row, Row::Picked);
    let faded = matches!(row, Row::Faint);
    let col = match row {
        Row::Picked => t.sel_edge,
        Row::Suppressed => t.text_dim,
        _ => t.accent,
    };
    // A view from above and to the side: the joint's z up the screen.
    let screen = |x: f64, y: f64, z: f64| Some(c + vec2(((x - y) * 0.866) as f32, ((x + y) * 0.42 - z) as f32));
    let limits = if matches!(row, Row::Limit) { vec![Some((-0.6, 1.9))] } else { vec![None; kind.dofs().len()] };
    if style == RING_AND_AXIS {
        let g = JointGlyph { id: 0, picked, rigid: kind == JointKind::Rigid, len: LEN, faded, centre: c };
        // Drawn as the viewport draws a picked or a suppressed joint.
        let (stroke, rim) = lit_strokes(t, picked, false, matches!(row, Row::Suppressed));
        return ring_shapes(&screen, kind, &limits, &g, stroke, rim, t);
    }
    let mood = match row {
        Row::Quiet => Mood::Quiet,
        Row::Suppressed => Mood::Suppressed,
        Row::Picked => Mood::Picked,
        _ => Mood::Bright,
    };
    let look = badge(style, kind, mood, col, t);
    let mut shapes = badge_shapes(c, vec2(1.0, 0.0), kind, &look, matches!(row, Row::Limit), faded, t);
    if faded {
        // The joint it lies on, drawn over it, as the plan draws them.
        let over = badge(style, JointKind::Slider, Mood::Bright, col, t);
        shapes.extend(badge_shapes(c + vec2(-9.0, -6.0), vec2(1.0, 0.0), JointKind::Slider, &over, false, false, t));
    }
    shapes
}

/// A colour as SVG writes it, with its alpha.
fn colour(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0)
}

/// Writes `shape` as SVG: the circles, lines, paths and rectangles the joint glyphs are made of.
fn svg_shape(shape: &Shape, out: &mut String) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|s| svg_shape(s, out)),
        Shape::Circle(c) => out.push_str(&format!(
            "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>\n",
            c.center.x,
            c.center.y,
            c.radius,
            colour(c.fill),
            colour(c.stroke.color),
            c.stroke.width
        )),
        Shape::LineSegment { points, stroke } => out.push_str(&format!(
            "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"{}\"/>\n",
            points[0].x,
            points[0].y,
            points[1].x,
            points[1].y,
            colour(stroke.color),
            stroke.width
        )),
        Shape::Path(p) => {
            let egui::epaint::ColorMode::Solid(stroke) = p.stroke.color else { return };
            let points: Vec<String> = p.points.iter().map(|q| format!("{:.2},{:.2}", q.x, q.y)).collect();
            let tag = if p.closed { "polygon" } else { "polyline" };
            out.push_str(&format!(
                "<{tag} points=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"/>\n",
                points.join(" "),
                if p.closed { colour(p.fill) } else { "none".into() },
                colour(stroke),
                p.stroke.width
            ));
        }
        Shape::Rect(r) => out.push_str(&format!(
            "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>\n",
            r.rect.min.x,
            r.rect.min.y,
            r.rect.width(),
            r.rect.height(),
            colour(r.fill),
            colour(r.stroke.color),
            r.stroke.width
        )),
        _ => {}
    }
}

/// Writes a label at `at`, anchored at its start, middle or end.
fn text(out: &mut String, at: Pos2, size: f32, fill: Color32, anchor: &str, weight: u16, s: &str) {
    out.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{size}\" fill=\"{}\" text-anchor=\"{anchor}\" font-weight=\"{weight}\">{s}</text>\n",
        at.x,
        at.y,
        colour(fill)
    ));
}

/// The sheet in `t`'s theme, and every cell's shapes.
fn sheet(t: &Tokens, theme: &str) -> (String, Vec<Vec<Vec<Shape>>>) {
    let rows = rows();
    let width = LABEL + COLUMN * STYLES.len() as f32 + 16.0;
    let types = rows.iter().filter(|r| matches!(r, Row::Type(_))).count();
    let height = HEAD + ROW * rows.len() as f32 + 34.0 + 16.0;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" font-family=\"Segoe UI, Helvetica, Arial, sans-serif\">\n<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>\n",
        colour(t.panel)
    );
    text(&mut out, pos2(16.0, 28.0), 17.0, t.text, "start", 600, &format!("Joint symbols, {theme} theme"));
    text(&mut out, pos2(16.0, 48.0), 12.0, t.text_dim, "start", 400, "Preferences ▸ Graphics ▸ Joint symbols. Ring and axis is the default.");
    for (i, name) in STYLES.iter().enumerate() {
        let x = LABEL + COLUMN * (i as f32 + 0.5);
        text(&mut out, pos2(x, HEAD - 8.0), 12.0, t.text, "middle", 600, name);
    }
    let mut cells = Vec::new();
    let mut y = HEAD;
    for (r, row) in rows.iter().enumerate() {
        if r == types {
            y += 34.0;
            text(&mut out, pos2(16.0, y - 10.0), 13.0, t.text, "start", 600, "States, shown on a revolute joint");
        }
        let (name, note) = row.label();
        text(&mut out, pos2(16.0, y + ROW / 2.0 + if note.is_empty() { 4.0 } else { -2.0 }), 13.0, t.text, "start", 400, name);
        if !note.is_empty() {
            text(&mut out, pos2(16.0, y + ROW / 2.0 + 13.0), 11.0, t.text_dim, "start", 400, note);
        }
        let mut line = Vec::new();
        for style in 0..STYLES.len() {
            let x = LABEL + COLUMN * style as f32;
            out.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"4\" fill=\"{}\"/>\n",
                x + 2.0,
                y + 2.0,
                COLUMN - 4.0,
                ROW - 4.0,
                colour(t.viewport_bottom)
            ));
            let shapes = cell(style as u8, *row, pos2(x + COLUMN / 2.0, y + ROW / 2.0), t);
            shapes.iter().for_each(|s| svg_shape(s, &mut out));
            line.push(shapes);
        }
        cells.push(line);
        y += ROW;
    }
    out.push_str("</svg>\n");
    (out, cells)
}

/// Every style draws every type and state in both themes, each badge's symbol apart from its fill;
/// with `SOLIDCRAFT_JOINT_SHEET` set, the sheet is written there.
#[test]
fn joint_symbol_sheet_draws_every_type_in_every_style() {
    for (t, theme) in [(Tokens::dark(), "dark"), (Tokens::light(), "light")] {
        let (svg, cells) = sheet(&t, theme);
        for (row, line) in rows().iter().zip(&cells) {
            for (style, shapes) in STYLES.iter().zip(line) {
                assert!(!shapes.is_empty(), "{theme}: {style} draws nothing for {}", row.label().0);
            }
        }
        // In every badge style each type's symbol stands apart from its badge; a dot has none.
        for style in 1..STYLES.len() as u8 {
            for kind in TYPES.iter().filter_map(|t| JointKind::parse(t)) {
                let look = badge(style, kind, Mood::Bright, t.sel_edge, &t);
                assert_ne!(look.ink.color, look.fill, "{theme}: {} {kind:?} draws its symbol in its fill", STYLES[style as usize]);
                assert!(look.ink.width > 0.0 || look.half == 0.0, "{theme}: {} {kind:?} draws no symbol", STYLES[style as usize]);
            }
        }
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_JOINT_SHEET") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("joint-symbols-{theme}.svg")), svg).unwrap();
        }
    }
}
