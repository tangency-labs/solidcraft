//! The drawing engine behind the Preferences figures: a figure is a short list of operations in its
//! own coordinates (`preferences_figure_data.rs`, compiled from the SVGs in
//! `docs/design/preferences/figures/`), painted with egui shapes in the colours of the current
//! `Tokens`. Nothing is rasterised and no image is loaded; every figure costs a few dozen shapes.
//! Any rectangle, including an empty, infinite or enormous one, is safe to paint into.
use crate::theme::Tokens;
use egui::{Align2, Color32, CornerRadius, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

/// Colour roles of the figures. Each reads one `Tokens` field (`docs/design/preferences/tokens.md`).
#[derive(Clone, Copy, Debug)]
pub(super) enum Ink {
    Ground,
    Line,
    Top,
    Left,
    Right,
    Card,
    Accent,
    Reference,
    AxisX,
    AxisY,
    AxisZ,
    Ok,
    Warn,
    Dim,
    Text,
    Border,
    ControlBorder,
    Control,
    /// Legacy SVG role for conflicting sketch geometry and the conflict-state glyph.
    Note,
    NoteFill,
    /// Legacy SVG ink: lettering uses accent foreground; strokes use model highlights.
    White,
    /// Legacy SVG ink: hatch strokes use figure lines; filled silhouettes use model shadows.
    Black,
    Highlight,
    Shadow,
    /// Colour vision sample colours: Z axis blue, projected, X axis, bend up and bend down.
    Cv0,
    Cv1,
    Cv2,
    Cv3,
    Cv4,
    /// A colour of a theme thumbnail, from its `ThemeSample`.
    Slot(u8),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct P {
    pub ink: Ink,
    pub a: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct L {
    pub ink: Ink,
    pub a: f32,
    pub w: f32,
    /// Dash and gap, `[0.0, 0.0]` for a solid line.
    pub dash: [f32; 2],
    pub round: bool,
}

/// A vertical gradient over the figure's `y` range.
#[derive(Clone, Copy, Debug)]
pub(super) struct G {
    pub top: Ink,
    pub bottom: Ink,
    pub y: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Op {
    Rect {
        r: [f32; 4],
        radius: f32,
        fill: Option<P>,
        line: Option<L>,
    },
    Circle {
        c: [f32; 3],
        fill: Option<P>,
        line: Option<L>,
    },
    Poly {
        pts: &'static [[f32; 2]],
        closed: bool,
        fill: Option<P>,
        grad: Option<G>,
        line: Option<L>,
    },
    /// A soft black ellipse: opaque `a` at the centre, clear at the edge.
    Glow {
        c: [f32; 4],
        a: f32,
    },
    Text {
        at: [f32; 2],
        size: f32,
        anchor: u8,
        ink: Ink,
        a: f32,
        s: &'static str,
        bold: bool,
    },
    Clip(&'static [[f32; 2]]),
    Unclip,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Figure {
    pub size: [f32; 2],
    pub ops: &'static [Op],
}

/// The twelve colours a theme thumbnail is drawn with: ground, bar and side panel, muted marks,
/// accent, the window face's top and bottom, grid, cube top, cube outline, cube left, cube right
/// and the thumbnail's border. A colour may carry alpha (a glass theme's panels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeSample {
    pub slots: [Color32; 12],
}

impl ThemeSample {
    /// A thumbnail of the theme whose tokens these are.
    pub fn from_tokens(t: &Tokens) -> ThemeSample {
        let mix = |a: Color32, b: Color32, f: f32| a.lerp_to_gamma(b, f);
        ThemeSample {
            slots: [
                t.dialog_footer,
                t.control_fill,
                t.form_dim.gamma_multiply(0.7),
                t.form_primary,
                t.viewport_top,
                t.viewport_bottom,
                t.form_dim.gamma_multiply(0.1),
                t.body,
                t.form_dim,
                mix(t.body, t.body_edge, 0.25),
                mix(t.body, t.body_edge, 0.5),
                t.dialog_border,
            ],
        }
    }

    /// The same colours with every alpha scaled (a glass theme's fills).
    pub fn glass(mut self, alpha: f32) -> ThemeSample {
        for slot in &mut self.slots[1..2] {
            *slot = slot.gamma_multiply(alpha);
        }
        self
    }
}

/// Largest side, in points, a figure is painted at; anything larger is skipped, not drawn wrong.
const MAX_SIDE: f32 = 16_384.0;
const MAX_POINTS: usize = 512;

struct Ctx<'a> {
    t: &'a Tokens,
    sample: Option<&'a ThemeSample>,
    /// Only the Appearance samples interpret the legacy SVG colour roles semantically.
    colour_vision: Option<usize>,
    origin: Pos2,
    scale: f32,
}

impl Ctx<'_> {
    fn ink(&self, ink: Ink, a: f32) -> Color32 {
        let t = self.t;
        let semantic = match (self.colour_vision, ink) {
            (Some(0), Ink::Cv0) => Some(t.sketch_locked),
            (Some(1), Ink::Cv1) => Some(t.sketch_projected),
            (Some(2), Ink::Cv2 | Ink::AxisX) => Some(t.axis_x),
            (Some(2), Ink::Cv0 | Ink::AxisY) => Some(t.axis_y),
            (Some(2), Ink::AxisZ) => Some(t.axis_z),
            (Some(3), Ink::Cv1) => Some(t.bend_up),
            (Some(3), Ink::Cv3) => Some(t.bend_down),
            (Some(4), Ink::Cv3) => Some(crate::theme::srgb(t.analysis.draft[0])),
            (Some(4), Ink::Cv1) => Some(crate::theme::srgb(t.analysis.draft[1])),
            (Some(4), Ink::Cv4) => Some(crate::theme::srgb(t.analysis.draft[2])),
            (Some(4), Ink::Black) => Some(crate::theme::srgb(t.analysis.draft[2].map(|c| c * 0.6))),
            (Some(5), Ink::Cv2) => Some(t.error),
            (Some(5), Ink::NoteFill) => Some(t.timeline_item_error),
            _ => None,
        };
        let c = semantic.unwrap_or_else(|| match ink {
            Ink::Ground => t.figure_ground,
            Ink::Line => t.figure_line,
            Ink::Top => t.figure_face_top,
            Ink::Left => t.figure_face_left,
            Ink::Right => t.figure_face_right,
            Ink::Card => t.figure_card,
            Ink::Accent => t.figure_accent,
            Ink::Reference => t.figure_reference,
            Ink::AxisX => t.figure_axis_x,
            Ink::AxisY => t.figure_axis_y,
            Ink::AxisZ => t.figure_axis_z,
            Ink::Ok => t.figure_ok,
            Ink::Warn => t.figure_warn,
            Ink::Dim => t.form_dim,
            Ink::Text => t.form_ink,
            Ink::Border => t.dialog_border,
            Ink::ControlBorder => t.control_border,
            Ink::Control => t.control_fill,
            Ink::Note => t.figure_conflict,
            Ink::NoteFill => t.pref_note_fill,
            Ink::White => t.pref_on_accent,
            Ink::Black => t.figure_line,
            Ink::Highlight => t.figure_highlight,
            Ink::Shadow => t.figure_shadow,
            Ink::Cv0 => t.axis_z,
            Ink::Cv1 => t.sketch_projected,
            Ink::Cv2 => t.axis_x,
            Ink::Cv3 => t.bend_up,
            Ink::Cv4 => t.bend_down,
            Ink::Slot(n) => self.sample.and_then(|s| s.slots.get(n as usize).copied()).unwrap_or(Color32::TRANSPARENT),
        });
        if a >= 1.0 { c } else { c.gamma_multiply(a.clamp(0.0, 1.0)) }
    }

    fn at(&self, p: [f32; 2]) -> Pos2 {
        self.origin + vec2(p[0] * self.scale, p[1] * self.scale)
    }

    fn stroke(&self, line: &L) -> Stroke {
        let ink = match line.ink {
            Ink::White => Ink::Highlight,
            ink => ink,
        };
        Stroke::new((line.w * self.scale).clamp(0.0, 64.0), self.ink(ink, line.a))
    }
}

/// Paints `figure` into `rect`, scaled to fit and centred, with `Tokens::get()` colours.
pub(super) fn paint(p: &Painter, rect: Rect, figure: &Figure, sample: Option<&ThemeSample>, clip: Option<&[[f32; 2]]>) {
    paint_with_tokens(p, rect, figure, sample, clip, &Tokens::get(), None);
}

/// Reuse the six canonical figures with the semantic palette being previewed, without global state.
pub(super) fn paint_colour_vision(p: &Painter, rect: Rect, index: usize, t: &Tokens) {
    use super::preferences_figure_data as figures;
    let figure = match index {
        0 => &figures::COLOUR_VISION_FIXED,
        1 => &figures::COLOUR_VISION_PROJECTED,
        2 => &figures::COLOUR_VISION_AXES,
        3 => &figures::COLOUR_VISION_BENDS,
        4 => &figures::COLOUR_VISION_DRAFT,
        5 => &figures::COLOUR_VISION_FAILED,
        _ => return,
    };
    paint_with_tokens(p, rect, figure, None, None, t, Some(index));
}

fn paint_with_tokens(
    p: &Painter,
    rect: Rect,
    figure: &Figure,
    sample: Option<&ThemeSample>,
    clip: Option<&[[f32; 2]]>,
    t: &Tokens,
    colour_vision: Option<usize>,
) {
    let size = rect.size();
    if !rect.min.is_finite() || !size.is_finite() || size.x < 1.0 || size.y < 1.0 || size.x > MAX_SIDE || size.y > MAX_SIDE {
        return;
    }
    let [fw, fh] = figure.size;
    if !(fw > 0.0 && fh > 0.0) {
        return;
    }
    let scale = (size.x / fw).min(size.y / fh);
    let fitted = Rect::from_center_size(rect.center(), vec2(fw * scale, fh * scale));
    let ctx = Ctx { t, sample, colour_vision, origin: fitted.min, scale };
    let p = p.with_clip_rect(fitted.intersect(p.clip_rect()));
    let outer: Option<Vec<Pos2>> = clip.map(|c| c.iter().map(|&q| ctx.at(q)).collect());
    // Nested clips are inside one another in every figure, so the innermost one is the clip.
    let mut stack: Vec<Vec<Pos2>> = Vec::new();
    for op in figure.ops {
        match op {
            Op::Clip(points) if outer.is_none() => stack.push(points.iter().map(|&q| ctx.at(q)).collect()),
            Op::Unclip if outer.is_none() => {
                stack.pop();
            }
            Op::Clip(_) | Op::Unclip => {}
            _ => draw(&p, &ctx, op, outer.as_deref().or(stack.last().map(Vec::as_slice))),
        }
    }
}

fn draw(p: &Painter, ctx: &Ctx, op: &Op, clip: Option<&[Pos2]>) {
    match op {
        Op::Rect { r, radius, fill, line } => {
            let rect = Rect::from_min_size(ctx.at([r[0], r[1]]), vec2(r[2] * ctx.scale, r[3] * ctx.scale));
            if clip.is_some() {
                let pts = rounded(rect, radius * ctx.scale);
                poly(p, ctx, &pts, true, fill, None, line, clip);
                return;
            }
            let corner = CornerRadius::same((radius * ctx.scale).clamp(0.0, 255.0).round() as u8);
            if let Some(f) = fill {
                p.rect_filled(rect, corner, ctx.ink(f.ink, f.a));
            }
            if let Some(l) = line {
                p.rect_stroke(rect, corner, ctx.stroke(l), StrokeKind::Middle);
            }
        }
        Op::Circle { c, fill, line } => {
            let centre = ctx.at([c[0], c[1]]);
            let radius = c[2] * ctx.scale;
            if clip.is_some() {
                let pts: Vec<Pos2> = (0..24)
                    .map(|i| {
                        let a = i as f32 * std::f32::consts::TAU / 24.0;
                        centre + vec2(a.cos(), a.sin()) * radius
                    })
                    .collect();
                poly(p, ctx, &pts, true, fill, None, line, clip);
                return;
            }
            if let Some(f) = fill {
                p.circle_filled(centre, radius, ctx.ink(f.ink, f.a));
            }
            if let Some(l) = line {
                p.circle_stroke(centre, radius, ctx.stroke(l));
            }
        }
        Op::Poly { pts, closed, fill, grad, line } => {
            let points: Vec<Pos2> = pts.iter().map(|&q| ctx.at(q)).collect();
            poly(p, ctx, &points, *closed, fill, grad.as_ref(), line, clip);
        }
        Op::Glow { c, a } => {
            let centre = ctx.at([c[0], c[1]]);
            let radius = vec2(c[2] * ctx.scale, c[3] * ctx.scale);
            glow(p, centre, radius, ctx.ink(Ink::Shadow, *a));
        }
        Op::Text { at, size, anchor, ink, a, s, .. } => {
            if clip.is_some() {
                return;
            }
            let align = match anchor {
                1 => Align2::CENTER_CENTER,
                2 => Align2::RIGHT_CENTER,
                _ => Align2::LEFT_CENTER,
            };
            // The board's captions are 6.5 to 7.5 units: too small to read at card size, so they
            // are drawn a third larger (never under 8 points). The SVG's `y` is the baseline, which
            // sits about 0.36 em below the middle of a line of digits and capitals.
            // Longer captions sit in tight pills and on cube faces, so they are drawn slightly smaller than the board (egui's monospace face is wider than the board's).
            let grow = match s.chars().count() {
                0..=4 => 1.2,
                5..=6 => 0.9,
                _ => 0.76,
            };
            let size = (size * ctx.scale * grow).clamp(7.0, 256.0);
            let at = ctx.at(*at) - vec2(0.0, size * 0.36);
            p.text(at, align, s, FontId::monospace(size), ctx.ink(*ink, *a));
        }
        Op::Clip(_) | Op::Unclip => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn poly(p: &Painter, ctx: &Ctx, pts: &[Pos2], closed: bool, fill: &Option<P>, grad: Option<&G>, line: &Option<L>, clip: Option<&[Pos2]>) {
    if pts.len() < 2 || pts.len() > MAX_POINTS || pts.iter().any(|q| !q.is_finite()) {
        return;
    }
    let clipped;
    let (shape, fillable) = match clip {
        Some(c) if pts.len() >= 3 && closed => {
            clipped = clip_polygon(pts, c);
            (&clipped[..], clipped.len() >= 3)
        }
        _ => (pts, pts.len() >= 3 && closed),
    };
    if fillable {
        if let Some(g) = grad {
            let (top, bottom) = (ctx.ink(g.top, 1.0), ctx.ink(g.bottom, 1.0));
            let (y0, y1) = (ctx.at([0.0, g.y[0]]).y, ctx.at([0.0, g.y[1]]).y);
            let span = (y1 - y0).max(1e-3);
            fill_polygon(p, shape, |q| top.lerp_to_gamma(bottom, ((q.y - y0) / span).clamp(0.0, 1.0)));
        } else if let Some(f) = fill {
            let ink = match f.ink {
                Ink::Black => Ink::Shadow,
                ink => ink,
            };
            let c = ctx.ink(ink, f.a);
            fill_polygon(p, shape, |_| c);
        }
    }
    if let Some(l) = line {
        let stroke = ctx.stroke(l);
        if stroke.width <= 0.0 || stroke.color == Color32::TRANSPARENT {
            return;
        }
        let mut path = pts.to_vec();
        if closed {
            path.push(pts[0]);
        }
        let runs = match clip {
            Some(c) => clip_path(&path, c),
            None => vec![path],
        };
        for run in runs {
            if l.dash[0] > 0.0 && l.dash[1] > 0.0 {
                let dash = (l.dash[0] * ctx.scale).max(0.5);
                let gap = (l.dash[1] * ctx.scale).max(0.5);
                p.extend(crate::viewport::dashed_line(&run, p.clip_rect(), stroke, dash, gap));
            } else {
                p.add(Shape::line(run, stroke));
            }
        }
    }
}

fn rounded(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5).max(0.0);
    if r < 0.01 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    let mut out = Vec::with_capacity(28);
    for (centre, start) in [
        (rect.right_top() + vec2(-r, r), -90.0_f32),
        (rect.right_bottom() + vec2(-r, -r), 0.0),
        (rect.left_bottom() + vec2(r, -r), 90.0),
        (rect.left_top() + vec2(r, r), 180.0),
    ] {
        for k in 0..=6 {
            let a = (start + 90.0 * k as f32 / 6.0).to_radians();
            out.push(centre + vec2(a.cos(), a.sin()) * r);
        }
    }
    out
}

fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn is_convex(pts: &[Pos2]) -> bool {
    let n = pts.len();
    let mut sign = 0.0_f32;
    for i in 0..n {
        let c = cross(pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        if c.abs() < 1e-4 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// Fills a simple polygon with `colour_at` per vertex: as a convex polygon when it is one, else by
/// ear clipping (at most `MAX_POINTS` vertices).
fn fill_polygon(p: &Painter, pts: &[Pos2], colour_at: impl Fn(Pos2) -> Color32) {
    let n = pts.len();
    if !(3..=MAX_POINTS).contains(&n) {
        return;
    }
    let mut mesh = Mesh::default();
    for &q in pts {
        mesh.colored_vertex(q, colour_at(q));
    }
    if is_convex(pts) {
        for i in 1..n as u32 - 1 {
            mesh.add_triangle(0, i, i + 1);
        }
    } else {
        let area: f32 = (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum();
        let mut idx: Vec<usize> = if area >= 0.0 { (0..n).collect() } else { (0..n).rev().collect() };
        let mut guard = 0;
        while idx.len() > 3 && guard < n * n {
            guard += 1;
            let m = idx.len();
            let mut cut = None;
            for k in 0..m {
                let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
                if cross(pts[a], pts[b], pts[c]) <= 0.0 {
                    continue;
                }
                let inside = idx.iter().any(|&o| {
                    o != a
                        && o != b
                        && o != c
                        && cross(pts[a], pts[b], pts[o]) >= 0.0
                        && cross(pts[b], pts[c], pts[o]) >= 0.0
                        && cross(pts[c], pts[a], pts[o]) >= 0.0
                });
                if !inside {
                    cut = Some(k);
                    break;
                }
            }
            let Some(k) = cut else { break };
            let (a, b, c) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            mesh.add_triangle(a as u32, b as u32, c as u32);
            idx.remove(k);
        }
        if idx.len() == 3 {
            mesh.add_triangle(idx[0] as u32, idx[1] as u32, idx[2] as u32);
        }
    }
    p.add(Shape::mesh(mesh));
}

fn glow(p: &Painter, centre: Pos2, radius: egui::Vec2, ink: Color32) {
    const SEGMENTS: u32 = 32;
    if !(radius.x > 0.0 && radius.y > 0.0) {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, ink);
    for i in 0..=SEGMENTS {
        let a = std::f32::consts::TAU * i as f32 / SEGMENTS as f32;
        mesh.colored_vertex(centre + vec2(a.cos() * radius.x, a.sin() * radius.y), Color32::TRANSPARENT);
    }
    for i in 1..=SEGMENTS {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(Shape::mesh(mesh));
}

/// Sutherland-Hodgman clip of a polygon against a convex clip polygon of either winding.
fn clip_polygon(subject: &[Pos2], clip: &[Pos2]) -> Vec<Pos2> {
    let n = clip.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f32 = (0..n).map(|i| clip[i].x * clip[(i + 1) % n].y - clip[(i + 1) % n].x * clip[i].y).sum();
    let side = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut out = subject.to_vec();
    for i in 0..n {
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let inside = |q: Pos2| side * cross(a, b, q) >= 0.0;
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        for j in 0..input.len() {
            let (cur, prev) = (input[j], input[(j + input.len() - 1) % input.len()]);
            if inside(cur) {
                if !inside(prev) {
                    out.push(intersect(prev, cur, a, b));
                }
                out.push(cur);
            } else if inside(prev) {
                out.push(intersect(prev, cur, a, b));
            }
        }
    }
    out
}

fn intersect(p1: Pos2, p2: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let (d1, d2) = (p2 - p1, b - a);
    let den = d1.x * d2.y - d1.y * d2.x;
    if den.abs() < 1e-9 {
        return p1;
    }
    let t = ((a.x - p1.x) * d2.y - (a.y - p1.y) * d2.x) / den;
    pos2(p1.x + d1.x * t, p1.y + d1.y * t)
}

/// The parts of a polyline inside a convex clip polygon (Cyrus-Beck per segment).
fn clip_path(path: &[Pos2], clip: &[Pos2]) -> Vec<Vec<Pos2>> {
    let n = clip.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f32 = (0..n).map(|i| clip[i].x * clip[(i + 1) % n].y - clip[(i + 1) % n].x * clip[i].y).sum();
    let side = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    for w in path.windows(2) {
        let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
        let d = w[1] - w[0];
        let mut visible = true;
        for i in 0..n {
            let (a, b) = (clip[i], clip[(i + 1) % n]);
            let num = side * cross(a, b, w[0]);
            // The signed distance of the point at t from this edge is num + t * den.
            let den = side * cross(a, b, w[1]) - num;
            if den.abs() < 1e-9 {
                if num < 0.0 {
                    visible = false;
                    break;
                }
            } else {
                let t = -num / den;
                if den > 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if visible && t0 < t1 {
            let (a, b) = (w[0] + d * t0, w[0] + d * t1);
            match runs.last_mut() {
                Some(run) if run.last().is_some_and(|l| l.distance(a) < 1e-3) => run.push(b),
                _ => runs.push(vec![a, b]),
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Pos2> {
        vec![pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)]
    }

    #[test]
    fn clipping_a_square_to_a_triangle_keeps_the_overlap() {
        let tri = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(0.0, 10.0)];
        let out = clip_polygon(&square(), &tri);
        let n = out.len();
        let area: f32 = (0..n).map(|i| out[i].x * out[(i + 1) % n].y - out[(i + 1) % n].x * out[i].y).sum::<f32>().abs() / 2.0;
        assert!((area - 50.0).abs() < 1e-3, "{area}");
    }

    #[test]
    fn a_path_is_cut_at_the_clip_edge() {
        let tri = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(0.0, 10.0)];
        let runs = clip_path(&[pos2(-5.0, 2.0), pos2(15.0, 2.0)], &tri);
        assert_eq!(runs.len(), 1);
        assert!((runs[0][0].x - 0.0).abs() < 1e-3 && (runs[0][1].x - 8.0).abs() < 1e-3, "{:?}", runs);
    }

    #[test]
    fn concave_polygons_are_detected() {
        assert!(is_convex(&square()));
        assert!(!is_convex(&[pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(5.0, 2.0), pos2(10.0, 10.0), pos2(0.0, 10.0)]));
    }
}
