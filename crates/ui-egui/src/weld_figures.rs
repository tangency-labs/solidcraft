//! Technical weld sections, in the specification's 176 × 120 drawing coordinates.
//! These are illustrations only; dimension hit targets focus the corresponding form field.

use egui::{Color32, FontId, Painter, Pos2, Sense, Shape, Stroke, TextFormat, Vec2, pos2, vec2};
use solidcraft_theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeFigure {
    Fillet,
    Groove,
    Spot,
    Plug,
    Stud,
    Overlay,
    Edge,
    Seam,
    Tack,
}

#[derive(Clone, Debug)]
pub(crate) struct FigureState {
    pub face: crate::weld_panel::Face,
    pub blend_toes: bool,
    pub side: usize,
    pub both_sides: bool,
    pub partial: bool,
    pub root: usize,
    pub groove: usize,
    pub advanced: bool,
    pub tack_mode: usize,
    pub add_fillet: bool,
    pub by_leg: bool,
    pub spans: bool,
    pub span_values: [f64; 3],
    pub staggered: bool,
    pub values: std::collections::BTreeMap<&'static str, f64>,
    pub letters: std::collections::BTreeSet<&'static str>,
}

impl FigureState {
    pub fn from_panel(p: &crate::weld_panel::WeldPanelState) -> Self {
        let s = &p.size;
        let fields = [
            ("a", &s.a),
            ("z", &s.z),
            ("z1", &s.z1),
            ("z2", &s.z2),
            ("h", &s.h),
            ("s", &s.s),
            ("d", &s.d),
            ("w", &s.w),
            ("t", &s.t),
            ("l", &s.l),
            ("e", &s.e),
            ("n", &s.n),
            ("o", &p.overlap),
            ("b", &p.root_gap),
        ];
        let letters = fields.iter().filter_map(|(k, v)| (!v.trim().is_empty()).then_some(*k)).collect();
        let values: std::collections::BTreeMap<_, _> =
            fields.into_iter().filter_map(|(k, v)| crate::ui_kit::parse_number(v).filter(|v| v.is_finite() && *v >= 0.).map(|v| (k, v))).collect();
        let span_values = [(&p.span_count, 3.), (&p.span_length, 10.), (&p.span_pitch, 40.)]
            .map(|(text, fallback)| crate::ui_kit::parse_number(text).filter(|v| *v > 0.).unwrap_or(fallback));
        Self {
            face: p.face,
            blend_toes: p.blend_toes,
            side: p.side,
            both_sides: p.both_sides,
            spans: p.spans,
            span_values,
            staggered: p.staggered,
            partial: p.partial_penetration,
            root: p.root,
            groove: p.groove,
            advanced: p.advanced_open,
            tack_mode: p.tack_mode,
            add_fillet: p.add_fillet,
            by_leg: p.size_by_leg,
            values,
            letters,
        }
    }
}

pub(crate) struct FigureResponse {
    pub response: egui::Response,
    pub focus: Option<&'static str>,
}

pub(crate) fn paint_size_figure(ui: &mut egui::Ui, tokens: &Tokens, figure: SizeFigure, state: &FigureState, width: f32) -> FigureResponse {
    // The caller supplies layout points; reject invalid sizes before allocating or painting.
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, width * 120.0 / 176.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, ui.is_enabled(), solidcraft_ui_text::tr("ui.weld.size.sketch")));
    if width == 0.0 || !ui.is_rect_visible(rect) {
        return FigureResponse { response, focus: None };
    }
    let drawing = Drawing { painter: ui.painter(), origin: rect.min, scale: width / 176.0, tokens, state, figure, letters: Default::default() };
    match figure {
        SizeFigure::Fillet => drawing.fillet(state.advanced),
        SizeFigure::Groove => drawing.groove(),
        SizeFigure::Spot => drawing.spot(),
        SizeFigure::Plug => drawing.plug(),
        SizeFigure::Stud => drawing.stud(),
        SizeFigure::Overlay => drawing.overlay(),
        SizeFigure::Edge => drawing.edge(),
        SizeFigure::Seam => drawing.seam(),
        SizeFigure::Tack => drawing.tack(),
    }
    if state.advanced && state.spans && matches!(figure, SizeFigure::Fillet | SizeFigure::Groove | SizeFigure::Edge | SizeFigure::Seam) {
        drawing.span_pattern();
    }
    let mut focus = None;
    for (letter, bounds) in drawing.letters.into_inner() {
        let hit = bounds.expand(tokens.space_2).intersect(rect);
        let button = ui.interact(hit, ui.make_persistent_id(("dimension", letter)), Sense::click());
        solidcraft_ui_registry::publish_target(&format!("weld:dimension:{letter}"), hit);
        if button.clicked() {
            focus = Some(letter);
        }
    }
    FigureResponse { response, focus }
}

struct Drawing<'a> {
    painter: &'a Painter,
    origin: Pos2,
    scale: f32,
    tokens: &'a Tokens,
    state: &'a FigureState,
    figure: SizeFigure,
    letters: std::cell::RefCell<Vec<(&'static str, egui::Rect)>>,
}

impl Drawing<'_> {
    fn point(&self, p: Pos2) -> Pos2 {
        let mut p = p;
        if self.state.side == 1 {
            match self.figure {
                SizeFigure::Fillet | SizeFigure::Tack => p.x = 176. - p.x,
                SizeFigure::Groove | SizeFigure::Edge | SizeFigure::Plug | SizeFigure::Seam | SizeFigure::Overlay => p.y = 120. - p.y,
                SizeFigure::Spot | SizeFigure::Stud => {}
            }
        }
        self.origin + p.to_vec2() * self.scale
    }

    fn stroke(&self, width: f32, color: Color32) -> Stroke {
        let pixels_per_point = self.painter.ctx().pixels_per_point();
        Stroke::new((width * self.scale * pixels_per_point).round().max(1.0) / pixels_per_point, color)
    }

    fn line(&self, a: Pos2, b: Pos2, width: f32, color: Color32) {
        self.painter.line_segment([self.point(a), self.point(b)], self.stroke(width, color));
    }

    fn extension(&self, a: Pos2, b: Pos2) {
        self.line(a, b, 0.7, self.tokens.text_dim);
    }

    fn outline(&self, polygon: &[Pos2]) {
        let stroke = self.stroke(1.3, self.tokens.icon);
        // Separate edges and disks produce round joins, including the T-joint's reentrant corners.
        for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
            self.painter.line_segment([self.point(*a), self.point(*b)], stroke);
            self.painter.circle_filled(self.point(*a), stroke.width * 0.5, stroke.color);
        }
    }

    fn plate(&self, polygon: &[Pos2]) {
        for [a, b] in hatch_segments(polygon) {
            self.line(a, b, 0.7, self.tokens.text_dim.gamma_multiply(0.75));
        }
        self.outline(polygon);
    }

    /// All callers here are convex; the wavy overlay uses a strip mesh instead.
    fn weld(&self, polygon: &[Pos2]) {
        self.painter.add(Shape::convex_polygon(polygon.iter().map(|p| self.point(*p)).collect(), self.tokens.accent, Stroke::NONE));
        self.outline(polygon);
    }

    fn arrow(&self, tip: Pos2, direction: Vec2) {
        let base = tip - direction * 6.5;
        let side = vec2(-direction.y, direction.x) * 2.2;
        self.painter.add(Shape::convex_polygon([tip, base + side, base - side].map(|p| self.point(p)).to_vec(), self.tokens.text, Stroke::NONE));
    }

    fn dimension(&self, a: Pos2, b: Pos2, outside: bool) {
        let direction = unit(b - a);
        if outside {
            self.arrow(a, direction);
            self.arrow(b, -direction);
            self.line(a - direction * 12.0, a, 1.1, self.tokens.text_dim);
            self.line(b, b + direction * 12.0, 1.1, self.tokens.text_dim);
        } else {
            self.arrow(a, -direction);
            self.arrow(b, direction);
            self.line(a, b, 1.1, self.tokens.text_dim);
        }
    }

    /// `anchor` is SVG text-anchor (0 = start, 0.5 = middle, 1 = end); y is the baseline.
    fn text(&self, at: Pos2, text: &'static str, anchor: f32) {
        let font_size = self.tokens.font_body.max(self.tokens.font_body * self.scale);
        let mut job = egui::text::LayoutJob::default();
        job.append(
            solidcraft_ui_text::tr(&format!("ui.weld.dim.{text}")),
            0.0,
            TextFormat { font_id: FontId::monospace(font_size), color: self.tokens.text, italics: true, ..Default::default() },
        );
        let galley = self.painter.layout_job(job);
        let baseline = galley.rows.first().and_then(|row| row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y)).unwrap_or(galley.size().y);
        let origin = self.point(at) - vec2(galley.size().x * anchor, baseline);
        self.letters.borrow_mut().push((text, galley.rect.translate(origin.to_vec2())));
        self.painter.galley(origin, galley, self.tokens.text);
    }

    fn amount(&self, key: &str, nominal: f32, reference: f64) -> f32 {
        self.state.values.get(key).map_or(nominal, |v| nominal * (0.6 + 0.4 * (v / (v + reference)) as f32))
    }

    fn span_pattern(&self) {
        let [count, length, pitch] = self.state.span_values;
        // This is a bounded illustration, rather than a model-space placement decision.
        let count = count.clamp(1., 6.) as usize;
        let step = 116. / count.saturating_sub(1).max(1) as f32 * (0.6 + 0.4 * (pitch / (pitch + 40.)) as f32);
        let bead = step * (length / pitch).clamp(0.12, 0.8) as f32;
        for i in 0..count {
            let x = 24. + i as f32 * step;
            let y = if self.state.staggered && i % 2 == 1 { 22. } else { 20. };
            self.weld(&[pos2(x, y), pos2(x + bead, y), pos2(x + bead, y + 3.), pos2(x, y + 3.)]);
        }
        let end = 24. + count.saturating_sub(1) as f32 * step + bead;
        self.dimension(pos2(24., 17.), pos2(end, 17.), end - 24. < 13.);
        self.text(pos2(end + 8., 12.), "n", 0.5);
        self.text(pos2(24. + bead / 2., 12.), "l", 0.5);
        self.text(pos2(24. + bead / 2. + step / 2., 12.), "e", 0.5);
    }

    fn face_points(&self, a: Pos2, b: Pos2) -> Vec<Pos2> {
        use crate::weld_panel::Face;
        let normal = unit(vec2(b.y - a.y, a.x - b.x));
        let bulge = match self.state.face {
            Face::Flat => 0.,
            Face::Convex => 8.,
            Face::Concave => -8.,
        };
        let mut curve = vec![a];
        quadratic(&mut curve, a, a.lerp(b, 0.5) + normal * bulge, b);
        curve
    }

    fn contour(&self, a: Pos2, b: Pos2) {
        let curve = self.face_points(a, b);
        for pair in curve.windows(2) {
            self.line(pair[0], pair[1], 1.3, self.tokens.icon);
        }
        if self.state.blend_toes && matches!(self.figure, SizeFigure::Fillet | SizeFigure::Groove) {
            self.painter.circle_stroke(self.point(a), self.tokens.space_3 * self.scale, self.stroke(1.3, self.tokens.accent));
            self.painter.circle_stroke(self.point(b), self.tokens.space_3 * self.scale, self.stroke(1.3, self.tokens.accent));
        }
    }

    /// A fan from the joint root fills both convex and concave faces without treating a
    /// concave outline as a convex polygon or leaving the old straight face underneath it.
    fn face_weld(&self, root: Pos2, a: Pos2, b: Pos2) {
        let face = self.face_points(a, b);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(self.point(root), self.tokens.accent);
        for point in &face {
            mesh.colored_vertex(self.point(*point), self.tokens.accent);
        }
        for i in 1..face.len() {
            mesh.add_triangle(0, i as u32, i as u32 + 1);
        }
        self.painter.add(mesh);
        self.line(root, a, 1.3, self.tokens.icon);
        self.line(b, root, 1.3, self.tokens.icon);
        self.contour(a, b);
    }

    fn face_band(&self, a: Pos2, b: Pos2, base_y: f32) {
        let face = self.face_points(a, b);
        let mut mesh = egui::Mesh::default();
        for point in &face {
            mesh.colored_vertex(self.point(*point), self.tokens.accent);
            mesh.colored_vertex(self.point(pos2(point.x, base_y)), self.tokens.accent);
        }
        for i in 0..face.len().saturating_sub(1) {
            let index = (2 * i) as u32;
            mesh.add_triangle(index, index + 2, index + 1);
            mesh.add_triangle(index + 1, index + 2, index + 3);
        }
        self.painter.add(mesh);
        self.line(pos2(a.x, base_y), a, 1.3, self.tokens.icon);
        self.line(b, pos2(b.x, base_y), 1.3, self.tokens.icon);
        self.line(pos2(a.x, base_y), pos2(b.x, base_y), 1.3, self.tokens.icon);
        self.contour(a, b);
    }

    fn fillet(&self, advanced: bool) {
        let plate_top = if advanced && self.state.spans { 35. } else { 22. };
        let gap = if self.state.letters.contains("b") && self.state.values.get("b").is_none_or(|v| *v > 0.) { self.amount("b", 10., 3.) } else { 0. };
        if gap > 0. {
            self.plate(&[pos2(62., plate_top), pos2(78., plate_top), pos2(78., 86. - gap), pos2(62., 86. - gap)]);
            self.plate(&[pos2(6., 86.), pos2(170., 86.), pos2(170., 104.), pos2(6., 104.)]);
        } else {
            self.plate(&[
                pos2(6., 86.),
                pos2(62., 86.),
                pos2(62., plate_top),
                pos2(78., plate_top),
                pos2(78., 86.),
                pos2(170., 86.),
                pos2(170., 104.),
                pos2(6., 104.),
            ]);
        }
        let leg = self.amount(if self.state.by_leg { "z" } else { "a" }, 30., if self.state.by_leg { 4.2 } else { 3. });
        let right = pos2(78. + self.amount("z2", leg, 5.), 86.);
        let top = pos2(78., 86. - self.amount("z1", leg, 5.));
        let root = pos2(78., 86.);
        let horizontal = right.x - root.x;
        let vertical = root.y - top.y;
        let projection = vertical * vertical / (horizontal * horizontal + vertical * vertical);
        let foot = top + vec2(horizontal, vertical) * projection;
        let normal = unit(foot - root);
        let tangent = unit(right - top);
        let deep = self.state.letters.contains("s");
        let deep_root = if deep { root - normal * (self.amount("s", 18., 8.) / 2.) } else { root };
        self.face_weld(deep_root, top, right);
        self.extension(right, pos2(right.x, 40.));
        self.dimension(pos2(78., 48.), pos2(right.x, 48.), right.x - 78. < 13.);
        self.text(pos2((78. + right.x) / 2., 40.), if advanced && self.state.letters.contains("z2") { "z2" } else { "z" }, 0.5);
        let offset = if deep { tangent * 5. } else { Vec2::ZERO };
        self.dimension(root + offset, foot + offset, (foot - root).length() < 13.);
        self.text(foot + offset + vec2(8., 4.), "a", 0.5);
        if self.state.both_sides {
            let opposite = |p: Pos2| pos2(140. - p.x, p.y);
            self.face_weld(opposite(deep_root), opposite(right), opposite(top));
        }
        if advanced && self.state.letters.contains("z1") {
            self.extension(top, pos2(130., top.y));
            self.extension(root, pos2(130., root.y));
            self.dimension(pos2(122., top.y), pos2(122., 86.), 86. - top.y < 13.);
            self.text(pos2(140., 72.), "z1", 0.5);
        }
        if deep {
            let shift = tangent * 18.;
            self.extension(deep_root, deep_root - shift);
            self.extension(foot, foot - shift);
            self.dimension(deep_root - shift, foot - shift, (foot - deep_root).length() < 13.);
            self.text((deep_root - shift).lerp(foot - shift, 0.5) - vec2(11., 0.), "s", 0.5);
        }
        if advanced && gap > 0. {
            self.extension(pos2(62., 86. - gap), pos2(16., 86. - gap));
            self.extension(pos2(62., 86.), pos2(16., 86.));
            self.dimension(pos2(22., 86. - gap), pos2(22., 86.), true);
            self.text(pos2(12., 86. - gap / 2.), "b", 0.5);
        }
    }

    fn groove(&self) {
        // Each groove has its own preparation profile; X/K have their root at mid-depth.
        let (mut left, mut right): (Vec<Pos2>, Vec<Pos2>) = match self.state.groove {
            0 => (vec![pos2(72., 52.), pos2(72., 92.)], vec![pos2(96., 52.), pos2(96., 92.)]),
            2 => (vec![pos2(72., 52.), pos2(72., 92.)], vec![pos2(112., 52.), pos2(96., 92.)]),
            3 => (
                vec![pos2(60., 52.), pos2(62., 74.), pos2(70., 88.), pos2(80., 92.)],
                vec![pos2(108., 52.), pos2(106., 74.), pos2(98., 88.), pos2(88., 92.)],
            ),
            4 => (vec![pos2(76., 52.), pos2(76., 92.)], vec![pos2(112., 52.), pos2(110., 74.), pos2(100., 88.), pos2(88., 92.)]),
            5 => (vec![pos2(60., 52.), pos2(80., 78.), pos2(80., 92.)], vec![pos2(108., 52.), pos2(88., 78.), pos2(88., 92.)]),
            6 => (vec![pos2(76., 52.), pos2(76., 92.)], vec![pos2(112., 52.), pos2(88., 78.), pos2(88., 92.)]),
            7 => (vec![pos2(60., 52.), pos2(80., 72.), pos2(60., 92.)], vec![pos2(108., 52.), pos2(88., 72.), pos2(108., 92.)]),
            8 => (vec![pos2(76., 52.), pos2(76., 92.)], vec![pos2(112., 52.), pos2(88., 72.), pos2(112., 92.)]),
            _ => (vec![pos2(60., 52.), pos2(80., 92.)], vec![pos2(108., 52.), pos2(88., 92.)]),
        };
        let left_root_x = left.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        let right_root_x = right.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let root_center = (left_root_x + right_root_x) / 2.;
        if self.state.letters.contains("b") {
            // Keep a visible fusion zone for a closed square joint; do not dimension it as b.
            let gap = if self.state.values.get("b") == Some(&0.) { 2. } else { self.amount("b", 16., 3.) };
            for p in &mut left {
                p.x += root_center - gap / 2. - left_root_x;
            }
            for p in &mut right {
                p.x += root_center + gap / 2. - right_root_x;
            }
        }
        let mut plate = vec![pos2(24., 52.)];
        plate.extend(&left);
        plate.push(pos2(24., 92.));
        self.plate(&plate);
        let mut plate = vec![pos2(170., 52.)];
        plate.extend(&right);
        plate.push(pos2(170., 92.));
        self.plate(&plate);
        // Horizontal strips keep concave groove fills valid for egui's convex tessellator.
        let depth = if self.state.partial { self.amount("s", 30., 8.) } else { 40. };
        let h = self.amount("h", 10., 3.);
        let concave = self.state.face == crate::weld_panel::Face::Concave;
        let top = if concave { 52. + h } else { 52. - h };
        // The quadratic's eight-unit control offset gives a four-unit dip at its middle.
        let band_base = if concave { top + 4. } else { 52. };
        let x_at = |profile: &[Pos2], y: f32| {
            profile
                .windows(2)
                .find_map(|pair| {
                    (pair[0].y <= y && y <= pair[1].y && pair[1].y > pair[0].y)
                        .then(|| pair[0].x + (pair[1].x - pair[0].x) * (y - pair[0].y) / (pair[1].y - pair[0].y))
                })
                .unwrap_or(84.)
        };
        for y in 52..(52 + depth as i32) {
            if (y as f32) < band_base {
                continue;
            }
            self.line(pos2(x_at(&left, y as f32), y as f32), pos2(x_at(&right, y as f32), y as f32), 1.3, self.tokens.accent);
        }
        for side in [&left, &right] {
            for pair in side.windows(2) {
                self.line(pair[0], pair[1], 1.3, self.tokens.icon);
            }
        }
        let overlap = if self.state.letters.contains("o") { self.amount("o", 10., 3.) } else { h };
        let cap_overlap = if self.state.blend_toes { overlap } else { 0. };
        let cap_left = left[0].x - cap_overlap;
        let cap_right = right[0].x + cap_overlap;
        self.face_band(pos2(cap_left, top), pos2(cap_right, top), band_base);
        let cap = self.face_points(pos2(cap_left, top), pos2(cap_right, top));
        let height_point =
            (if concave { cap.iter().max_by(|a, b| a.y.total_cmp(&b.y)).copied() } else { cap.iter().min_by(|a, b| a.y.total_cmp(&b.y)).copied() })
                .unwrap_or(pos2(root_center, top));
        let height_x = cap_right + 16.;
        self.extension(height_point, pos2(height_x + 6., height_point.y));
        self.extension(pos2(cap_right, 52.), pos2(height_x + 6., 52.));
        self.dimension(pos2(height_x, height_point.y), pos2(height_x, 52.), true);
        self.text(pos2(height_x + 8., 35.), "h", 0.5);
        if self.state.partial {
            self.dimension(pos2(11., 52.), pos2(11., 52. + depth), false);
            self.text(pos2(11., 44.), "s", 0.5);
        }
        match self.state.root {
            1 => {
                self.contour(pos2(76., 98.), pos2(92., 98.));
                self.weld(&[pos2(76., 92.), pos2(84., 100.), pos2(92., 92.)]);
            }
            2 => self.weld(&[pos2(68., 93.), pos2(100., 93.), pos2(100., 100.), pos2(68., 100.)]),
            _ => {}
        }
        if self.state.both_sides {
            self.contour(pos2(76., 96.), pos2(92., 96.));
        }
        if self.state.add_fillet {
            self.weld(&[pos2(114., 52.), pos2(114., 40.), pos2(128., 52.)]);
        }
        if self.state.advanced && (self.state.letters.contains("o") || self.state.blend_toes) {
            self.dimension(pos2(left[0].x - overlap, 44.), pos2(left[0].x, 44.), true);
            self.text(pos2(left[0].x - overlap / 2., 40.), "o", 0.5);
        }
        if self.state.advanced && self.state.letters.contains("b") && self.state.values.get("b") != Some(&0.) {
            let a = left.iter().max_by(|a, b| a.x.total_cmp(&b.x)).copied().unwrap_or(pos2(80., 92.));
            let b = right.iter().min_by(|a, b| a.x.total_cmp(&b.x)).copied().unwrap_or(pos2(88., 92.));
            self.extension(a, pos2(a.x, 114.));
            self.extension(b, pos2(b.x, 114.));
            self.dimension(pos2(a.x, 111.), pos2(b.x, 111.), true);
            self.text(pos2(root_center, 108.), "b", 0.5);
        }
    }
    fn spot(&self) {
        self.plate(&[pos2(6., 46.), pos2(112., 46.), pos2(112., 60.), pos2(6., 60.)]);
        self.plate(&[pos2(58., 60.), pos2(170., 60.), pos2(170., 74.), pos2(58., 74.)]);
        let radius = self.amount("d", 17., 8.);
        let ellipse: Vec<_> = (0..96)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / 96.0;
                pos2(85. + radius * angle.cos(), 60. + 11. * angle.sin())
            })
            .collect();
        self.weld(&ellipse);
        self.extension(pos2(85. - radius, 62.), pos2(85. - radius, 98.));
        self.extension(pos2(85. + radius, 62.), pos2(85. + radius, 98.));
        self.dimension(pos2(85. - radius, 92.), pos2(85. + radius, 92.), false);
        self.text(pos2(85., 108.), "d", 0.5);
    }

    fn plug(&self) {
        self.plate(&[pos2(6., 44.), pos2(40., 44.), pos2(40., 64.), pos2(6., 64.)]);
        self.plate(&[pos2(86., 44.), pos2(120., 44.), pos2(120., 64.), pos2(86., 64.)]);
        self.plate(&[pos2(6., 64.), pos2(120., 64.), pos2(120., 92.), pos2(6., 92.)]);
        let top = 64. - self.amount("h", 20., 20.);
        self.face_band(pos2(40., top), pos2(86., top), 64.);
        self.extension(pos2(86., top), pos2(146., top));
        self.extension(pos2(120., 64.), pos2(146., 64.));
        self.dimension(pos2(141., top), pos2(141., 64.), true);
        self.text(pos2(153., 57.), "h", 0.5);
        if self.state.advanced && self.state.letters.contains("e") {
            let pitch = self.amount("e", 80., 40.);
            for x in [24., 24. + pitch] {
                self.painter.circle(
                    self.point(pos2(x, 16.)),
                    self.tokens.space_6 * self.scale,
                    self.tokens.accent,
                    self.stroke(1.3, self.tokens.icon),
                );
                self.extension(pos2(x, 22.), pos2(x, 34.));
            }
            self.dimension(pos2(24., 32.), pos2(24. + pitch, 32.), false);
            self.text(pos2(24. + pitch / 2., 28.), "e", 0.5);
        }
    }

    fn stud(&self) {
        let half = self.amount("d", 10., 8.);
        self.plate(&[
            pos2(6., 88.),
            pos2(80. - half, 88.),
            pos2(80. - half, 28.),
            pos2(80. + half, 28.),
            pos2(80. + half, 88.),
            pos2(170., 88.),
            pos2(170., 106.),
            pos2(6., 106.),
        ]);
        self.weld(&[pos2(80. + half, 88.), pos2(80. + half, 82.), pos2(88. + half, 88.)]);
        self.weld(&[pos2(80. - half, 88.), pos2(80. - half, 82.), pos2(72. - half, 88.)]);
        self.extension(pos2(80. - half, 28.), pos2(80. - half, 10.));
        self.extension(pos2(80. + half, 28.), pos2(80. + half, 10.));
        self.dimension(pos2(80. - half, 20.), pos2(80. + half, 20.), true);
        self.text(pos2(80., 14.), "d", 0.5);
        if self.state.advanced && self.state.letters.contains("e") {
            let pitch = self.amount("e", 112., 40.);
            for x in [26., 26. + pitch] {
                self.extension(pos2(x, 100.), pos2(x, 114.));
            }
            self.dimension(pos2(26., 110.), pos2(26. + pitch, 110.), false);
            self.text(pos2(26. + pitch / 2., 108.), "e", 0.5);
        }
    }

    fn overlay(&self) {
        let top = 78. - self.amount("t", 16., 4.);
        self.plate(&[pos2(6., 78.), pos2(150., 78.), pos2(150., 102.), pos2(6., 102.)]);
        let bead_width = self.amount("w", 44., 6.);
        let overlap = if self.state.letters.contains("o") { self.amount("o", bead_width * 0.45, 3.) } else { 0. };
        let pitch = bead_width - overlap;
        let mut wave = vec![pos2(6., 62.)];
        let mut x: f32 = 6.;
        for i in 0..16 {
            if x >= 150. {
                break;
            }
            let end = (x + pitch).min(150.);
            quadratic(&mut wave, pos2(x, 62.), pos2((x + end) / 2., if i % 2 == 0 { 54. } else { 70. }), pos2(end, 62.));
            x = end;
        }
        // The wave is concave: triangulate strips, rather than asking egui for a convex fill.
        let mut mesh = egui::Mesh::default();
        for p in &wave {
            mesh.colored_vertex(self.point(pos2(p.x, p.y + top - 62.)), self.tokens.accent);
            mesh.colored_vertex(self.point(pos2(p.x, 78.)), self.tokens.accent);
        }
        for i in 0..wave.len().saturating_sub(1) {
            let a = (i * 2) as u32;
            mesh.add_triangle(a, a + 2, a + 1);
            mesh.add_triangle(a + 1, a + 2, a + 3);
        }
        self.painter.add(mesh);
        for p in &mut wave {
            p.y += top - 62.;
        }
        wave.extend([pos2(150., 78.), pos2(6., 78.)]);
        self.outline(&wave);
        self.extension(pos2(150., top), pos2(166., top));
        self.extension(pos2(150., 78.), pos2(166., 78.));
        self.dimension(pos2(162., top), pos2(162., 78.), true);
        self.text(pos2(166., 56.), "t", 0.);
        if self.state.advanced && self.state.letters.contains("w") {
            self.dimension(pos2(18., 32.), pos2(18. + bead_width, 32.), false);
            self.text(pos2(18. + bead_width / 2., 26.), "w", 0.5);
        }
        if self.state.advanced && self.state.letters.contains("o") {
            self.dimension(pos2(18. + pitch, 44.), pos2(18. + bead_width, 44.), true);
            self.text(pos2(18. + (pitch + bead_width) / 2., 40.), "o", 0.5);
        }
    }

    fn edge(&self) {
        self.plate(&[pos2(40., 50.), pos2(76., 50.), pos2(76., 106.), pos2(40., 106.)]);
        self.plate(&[pos2(80., 50.), pos2(116., 50.), pos2(116., 106.), pos2(80., 106.)]);
        let w = self.amount("w", 38., 8.);
        self.face_weld(pos2(78., 50.), pos2(78. - w / 2., 48.), pos2(78. + w / 2., 48.));
        self.dimension(pos2(78. - w / 2., 26.), pos2(78. + w / 2., 26.), false);
        self.text(pos2(78., 20.), "w", 0.5);
    }

    fn seam(&self) {
        self.plate(&[pos2(10., 46.), pos2(130., 46.), pos2(130., 60.), pos2(10., 60.)]);
        self.plate(&[pos2(42., 60.), pos2(166., 60.), pos2(166., 76.), pos2(42., 76.)]);
        let w = self.amount("w", 24., 8.);
        let end = 52. + self.amount("l", 68., 10.);
        self.weld(&[pos2(52., 60. - w / 3.), pos2(end, 60. - w / 3.), pos2(end, 60. + w / 3.), pos2(52., 60. + w / 3.)]);
        self.dimension(pos2(140., 60. - w / 3.), pos2(140., 60. + w / 3.), true);
        self.text(pos2(156., 63.), "w", 0.5);
        if self.state.advanced {
            self.extension(pos2(52., 76.), pos2(52., 102.));
            self.extension(pos2(end, 76.), pos2(end, 102.));
            self.dimension(pos2(52., 98.), pos2(end, 98.), false);
            self.text(pos2((52. + end) / 2., 114.), "l", 0.5);
        }
    }

    fn tack(&self) {
        self.plate(&[pos2(10., 36.), pos2(164., 36.), pos2(164., 62.), pos2(10., 62.)]);
        self.plate(&[pos2(10., 64.), pos2(164., 64.), pos2(164., 90.), pos2(10., 90.)]);
        let length = self.amount("l", 18., 10.);
        let count = if self.state.tack_mode == 1 { self.state.values.get("n").copied().unwrap_or(3.).clamp(1., 6.) as usize } else { 3 };
        let step = match self.state.tack_mode {
            0 => self.amount("e", 56., 40.),
            1 if count > 1 => (140. - length) / count.saturating_sub(1) as f32,
            1 => 0.,
            _ => 56.,
        };
        for i in 0..count {
            let x = 16. + i as f32 * step;
            if self.state.both_sides {
                self.weld(&[pos2(x, 52.), pos2(x + length, 52.), pos2(x + length, 60.), pos2(x, 60.)]);
                self.weld(&[pos2(x, 66.), pos2(x + length, 66.), pos2(x + length, 74.), pos2(x, 74.)]);
            } else {
                self.weld(&[pos2(x, 57.), pos2(x + length, 57.), pos2(x + length, 69.), pos2(x, 69.)]);
            }
            if self.state.tack_mode == 2 {
                self.painter.circle_filled(self.point(pos2(x + length / 2., 63.)), self.tokens.space_2, self.tokens.text);
            }
        }
        let start_y = if self.state.both_sides { 52. } else { 57. };
        self.extension(pos2(16., start_y), pos2(16., 20.));
        self.extension(pos2(16. + length, start_y), pos2(16. + length, 20.));
        self.dimension(pos2(16., 24.), pos2(16. + length, 24.), true);
        self.text(pos2(16. + length / 2., 18.), "l", 0.5);
        match self.state.tack_mode {
            0 => {
                let first = 16. + length / 2.;
                self.extension(pos2(first, 69.), pos2(first, 108.));
                self.extension(pos2(first + step, 69.), pos2(first + step, 108.));
                self.dimension(pos2(first, 104.), pos2(first + step, 104.), false);
                self.text(pos2(first + step / 2., 119.), "e", 0.5);
            }
            1 => {
                let end = 16. + count.saturating_sub(1) as f32 * step + length;
                self.dimension(pos2(16., 104.), pos2(end, 104.), end - 16. < 13.);
                self.text(pos2((16. + end) / 2., 119.), "n", 0.5);
            }
            _ => {}
        }
    }
}

/// The direction of `v` on the illustration's own screen-space canvas; a zero or non-finite
/// vector points along x. Nothing here is model geometry: it only places arrowheads and bulges.
fn unit(v: Vec2) -> Vec2 {
    let length = v.length();
    if length.is_finite() && length > 1e-9 { v / length } else { vec2(1.0, 0.0) }
}

/// Flatten a quadratic at 32 steps: subpixel chord error even at twice the design size.
fn quadratic(points: &mut Vec<Pos2>, start: Pos2, control: Pos2, end: Pos2) {
    for i in 1..=32 {
        let t = i as f32 / 32.0;
        let u = 1.0 - t;
        points.push((start.to_vec2() * (u * u) + control.to_vec2() * (2.0 * u * t) + end.to_vec2() * (t * t)).to_pos2());
    }
}

/// Even/odd scan conversion of x+y=c lines. Half-open edge crossings handle vertices once;
/// pairing sorted intersections also clips concave plates, without bridging their notches.
fn hatch_segments(polygon: &[Pos2]) -> Vec<[Pos2; 2]> {
    if polygon.len() < 3 || polygon.iter().any(|p| !p.is_finite()) {
        return Vec::new();
    }
    let spacing = 4.5 * std::f32::consts::SQRT_2;
    let min = polygon.iter().map(|p| p.x + p.y).fold(f32::INFINITY, f32::min);
    let max = polygon.iter().map(|p| p.x + p.y).fold(f32::NEG_INFINITY, f32::max);
    let first = (min / spacing).ceil() as i32;
    let last = (max / spacing).floor() as i32;
    let mut result = Vec::new();
    for index in first..=last {
        let c = index as f32 * spacing;
        let mut crossings = Vec::new();
        for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
            let av = a.x + a.y;
            let bv = b.x + b.y;
            if (av <= c && c < bv) || (bv <= c && c < av) {
                let t = (c - av) / (bv - av);
                crossings.push(a.x + t * (b.x - a.x));
            }
        }
        crossings.sort_by(f32::total_cmp);
        for [a, b] in crossings.as_chunks::<2>().0 {
            result.push([pos2(*a, c - a), pos2(*b, c - b)]);
        }
    }
    result
}

#[cfg(test)]
#[path = "weld_figures_tests.rs"]
mod tests;
