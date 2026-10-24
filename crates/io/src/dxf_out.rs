//! The flat pattern's DXF drawing, as AutoCAD 2000 (`AC1015`), the oldest release that has the
//! ELLIPSE and SPLINE entities.
//!
//! Curves stay exact: a circle is a CIRCLE, a circular arc an ARC, a whole ellipse one ELLIPSE
//! and a conic arc of weight below 1 an elliptical ELLIPSE arc; Béziers (TrueType glyph
//! outlines, spline pieces) and the other conics are SPLINEs of their own degree. The file holds
//! what the entities refer to and what release 2000 needs to read them: the version, the
//! handle seed, the units and the extents; the linetype, layer and block-record tables; the
//! model and paper space blocks; handles, owners and the subclass markers of each entity; the
//! root dictionary. Layers can be dashed and a drawing can carry TEXT (for annotations). Group
//! codes follow Autodesk's public DXF reference.

use std::f64::consts::TAU;
use std::fmt::Write as _;

use solidcraft_geom::{Loop2, MAX_BEZIER_DEGREE, Seg2, Vec2};

/// A layer's linetype, defined in the file's LTYPE table so a viewer needs nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DxfLinetype {
    #[default]
    Continuous,
    /// AutoCAD's DASHED: a dash of 12.7 mm and a gap of 6.35 mm.
    Dashed,
    /// Short 2 mm dashes and 1 mm gaps for hidden edges in millimetre drawings.
    Hidden,
}

impl DxfLinetype {
    fn name(self) -> &'static str {
        match self {
            DxfLinetype::Continuous => "Continuous",
            DxfLinetype::Dashed => "DASHED",
            DxfLinetype::Hidden => "HIDDEN",
        }
    }
}

/// A layer: its name, AutoCAD colour index (1 red, 3 green, 5 blue, 7 white or black) and linetype.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfLayer {
    pub name: String,
    pub color: i16,
    pub linetype: DxfLinetype,
}

impl DxfLayer {
    /// A continuous layer.
    pub fn new(name: impl Into<String>, color: i16) -> DxfLayer {
        DxfLayer { name: name.into(), color, linetype: DxfLinetype::Continuous }
    }

    /// The same layer with a dashed line.
    pub fn dashed(self) -> DxfLayer {
        DxfLayer { linetype: DxfLinetype::Dashed, ..self }
    }
}

/// The geometry of one entity (millimetres).
#[derive(Clone, Debug, PartialEq)]
pub enum DxfShape {
    Line(Vec2, Vec2),
    /// Connected straight segments, with continuous linetype generation through the vertices.
    Polyline {
        points: Vec<Vec2>,
    },
    Circle {
        c: Vec2,
        r: f64,
    },
    /// Counter-clockwise from `a0` to `a1` (degrees).
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        a1: f64,
    },
    /// An ellipse or an arc of one, counter-clockwise from parameter `t0` to `t1` (radians):
    /// the point at `t` is `c + major·cos t + perp(major)·ratio·sin t`, `major` being the
    /// vector from the centre to the end of the major axis and `ratio` the minor radius over
    /// the major one (0 < ratio ≤ 1). A whole ellipse runs from 0 to 2π.
    Ellipse {
        c: Vec2,
        major: Vec2,
        ratio: f64,
        t0: f64,
        t1: f64,
    },
    /// A clamped B-spline of `degree` over `ctrl`, rational when `weights` are given.
    Spline {
        degree: u8,
        knots: Vec<f64>,
        ctrl: Vec<Vec2>,
        weights: Option<Vec<f64>>,
    },
    /// One line of text, centred on `at` (its baseline) and turned `angle_deg` counter-clockwise.
    /// `%%d`, `%%p` and `%%c` are AutoCAD's degree, plus-minus and diameter signs; other letters
    /// outside ASCII are written as `\U+XXXX`.
    Text {
        at: Vec2,
        height: f64,
        angle_deg: f64,
        text: String,
    },
}

/// An entity on a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfEntity {
    pub layer: String,
    pub shape: DxfShape,
}

/// The entity of one profile segment, exactly: a line, an arc (a circle when it is a whole
/// turn), an elliptical arc for a conic of weight below 1, a spline for any other conic, and for a
/// Bézier a spline of its own degree over all its control points.
pub fn seg_shape(s: &Seg2) -> DxfShape {
    match *s {
        Seg2::Line { a, b } => DxfShape::Line(a, b),
        Seg2::Arc { center, radius, start, sweep } => {
            if (sweep.abs() - TAU).abs() < 1e-9 {
                return DxfShape::Circle { c: center, r: radius };
            }
            let (a0, a1) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
            DxfShape::Arc { c: center, r: radius, a0: a0.to_degrees().rem_euclid(360.0), a1: a1.to_degrees().rem_euclid(360.0) }
        }
        Seg2::Conic { a, apex, b, w } => match conic_ellipse(a, apex, b, w) {
            Some(e) => DxfShape::Ellipse { c: e.c, major: e.major, ratio: e.ratio, t0: wrap(e.t0), t1: wrap(e.t1) },
            None => bezier_spline(&[a, apex, b], ((w - 1.0).abs() > 1e-12).then(|| vec![1.0, w, 1.0])),
        },
        Seg2::Cubic { p0, p1, p2, p3 } => bezier_spline(&[p0, p1, p2, p3], None),
        Seg2::Bezier { n, p } => bezier_spline(p.get(..=(n as usize).clamp(1, MAX_BEZIER_DEGREE)).unwrap_or(&p), None),
    }
}

/// The entities of a closed loop: one ELLIPSE when it is a whole ellipse of conic arcs, else
/// one entity for each segment.
pub fn loop_shapes(lp: &Loop2) -> Vec<DxfShape> {
    let arcs: Option<Vec<EllipseArc>> =
        lp.segs.iter().map(|s| if let Seg2::Conic { a, apex, b, w } = *s { conic_ellipse(a, apex, b, w) } else { None }).collect();
    if let Some(arcs) = arcs
        && let Some(first) = arcs.first()
        && arcs.iter().all(|e| e.same_ellipse(first))
        && (arcs.iter().map(|e| e.t1 - e.t0).sum::<f64>() - TAU).abs() < 1e-7
    {
        return vec![DxfShape::Ellipse { c: first.c, major: first.major, ratio: first.ratio, t0: 0.0, t1: TAU }];
    }
    lp.segs.iter().map(seg_shape).collect()
}

/// A parameter reduced to [0, 2π); one within 1e-12 of a full turn is 0.
fn wrap(t: f64) -> f64 {
    let r = t.rem_euclid(TAU);
    if r > TAU - 1e-12 { 0.0 } else { r }
}

/// An elliptical arc as DXF describes it (the parameters are not yet reduced to 0…2π).
#[derive(Clone, Copy, Debug)]
struct EllipseArc {
    c: Vec2,
    major: Vec2,
    ratio: f64,
    t0: f64,
    t1: f64,
}

impl EllipseArc {
    fn same_ellipse(&self, o: &EllipseArc) -> bool {
        let tol = 1e-7 * self.major.len().max(1.0);
        let parallel = self.major.cross(o.major).abs() <= tol * self.major.len();
        self.c.dist(o.c) <= tol && parallel && (self.major.len() - o.major.len()).abs() <= tol && (self.ratio - o.ratio).abs() <= 1e-9
    }
}

/// The ellipse a rational quadratic Bézier of weight `w` below 1 (end weights 1) lies on. The
/// arc is `c + u·cos θ + v·sin θ` for θ from −α to α, `cos α = w`, with the conjugate
/// semi-diameters `u` (from the centre towards the arc's middle) and `v` (towards its end).
/// With `m` the middle of the chord, `apex − m = u·(1 − w²)/w` and `m − c = w·u`, so
/// `c = m − (apex − m)·w²/(1 − w²)`, and `v = (b − m)/sin α`. The principal axes are where
/// |u cos t + v sin t| is largest and smallest, tan 2t = 2 u·v / (u² − v²).
fn conic_ellipse(a: Vec2, apex: Vec2, b: Vec2, w: f64) -> Option<EllipseArc> {
    // Within 1e-6 of 1 the centre is hundreds of metres away: a rational spline is exact too.
    if !(w > 1e-9 && w < 1.0 - 1e-6) {
        return None;
    }
    let m = (a + b) * 0.5;
    let c = m - (apex - m) * (w * w / (1.0 - w * w));
    let alpha = w.acos();
    let (u, v) = ((m - c) / w, (b - m) / alpha.sin());
    if !(c.is_finite() && u.is_finite() && v.is_finite()) || u.cross(v).abs() < 1e-18 {
        return None;
    }
    let t = 0.5 * (2.0 * u.dot(v)).atan2(u.dot(u) - v.dot(v));
    let major = u * t.cos() + v * t.sin();
    let minor = v * t.cos() - u * t.sin();
    if !(major.len() > 1e-12) {
        return None;
    }
    let ratio = (minor.len() / major.len()).clamp(1e-12, 1.0);
    // DXF runs counter-clockwise with the minor axis a quarter turn ahead of the major one. If
    // `minor` lies on the other side the arc, which runs a → b with increasing θ, is clockwise.
    let (t0, t1) = if major.cross(minor) > 0.0 { (-alpha - t, alpha - t) } else { (t - alpha, t + alpha) };
    Some(EllipseArc { c, major, ratio, t0, t1 })
}

/// A Bézier over `pts` (weights `weights`) as a clamped spline of degree `pts.len() − 1`.
fn bezier_spline(pts: &[Vec2], weights: Option<Vec<f64>>) -> DxfShape {
    let degree = pts.len().saturating_sub(1).max(1);
    let knots = std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect();
    DxfShape::Spline { degree: degree as u8, knots, ctrl: pts.to_vec(), weights }
}

impl DxfShape {
    /// Points on the shape (or its control polygon) for the drawing's extents.
    fn sample(&self, out: &mut Vec<Vec2>) {
        match self {
            DxfShape::Line(a, b) => out.extend([*a, *b]),
            DxfShape::Polyline { points } => out.extend(points.iter().copied()),
            DxfShape::Circle { c, r } => out.extend([*c - Vec2::new(*r, *r), *c + Vec2::new(*r, *r)]),
            DxfShape::Arc { c, r, a0, a1 } => {
                let sweep = (a1 - a0).rem_euclid(360.0);
                out.extend((0..=64).map(|i| *c + Vec2::from_angle((a0 + sweep * i as f64 / 64.0).to_radians()) * *r));
            }
            DxfShape::Ellipse { c, major, ratio, t0, t1 } => {
                let (minor, sweep) = (major.perp() * *ratio, if (t1 - t0 - TAU).abs() < 1e-12 { TAU } else { (t1 - t0).rem_euclid(TAU) });
                out.extend((0..=128).map(|i| {
                    let t = t0 + sweep * i as f64 / 128.0;
                    *c + *major * t.cos() + minor * t.sin()
                }));
            }
            // A spline lies within the hull of its control points.
            DxfShape::Spline { ctrl, .. } => out.extend(ctrl.iter().copied()),
            // Where a viewer puts the letters depends on its font: 0.8 heights for each.
            DxfShape::Text { at, height, angle_deg, text } => {
                let (along, up) = (Vec2::from_angle(angle_deg.to_radians()), Vec2::from_angle(angle_deg.to_radians()).perp());
                let half = 0.4 * height * text.chars().count() as f64;
                out.extend([-half, half].map(|s| *at + along * s));
                out.extend([-half, half].map(|s| *at + along * s + up * *height));
            }
        }
    }
}

/// Group codes and handles, in file order.
struct Out {
    s: String,
    next: u64,
}

impl Out {
    fn code(&mut self, c: i32, v: impl std::fmt::Display) {
        let _ = write!(self.s, "{c:>3}\n{v}\n");
    }
    /// A real number: up to twelve decimals, at least one.
    fn num(&mut self, c: i32, v: f64) {
        let text = format!("{:.12}", if v.is_finite() { v } else { 0.0 });
        let text = text.trim_end_matches('0');
        let text = if text.ends_with('.') { format!("{text}0") } else { text.to_string() };
        self.code(c, if text == "-0.0" { "0.0" } else { text.as_str() });
    }
    /// A point in the plane at height 0: codes `c`, `c + 10`, `c + 20`.
    fn point(&mut self, c: i32, p: Vec2) {
        self.num(c, p.x);
        self.num(c + 10, p.y);
        self.num(c + 20, 0.0);
    }
    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next);
        self.next += 1;
        h
    }
    /// The start of a symbol table; returns its handle.
    fn table(&mut self, name: &str, entries: usize) -> String {
        let h = self.handle();
        self.code(0, "TABLE");
        self.code(2, name);
        self.code(5, &h);
        self.code(330, 0);
        self.code(100, "AcDbSymbolTable");
        self.code(70, entries);
        h
    }
    /// The start of a symbol table record.
    fn record(&mut self, kind: &str, subclass: &str, table: &str) {
        let h = self.handle();
        self.code(0, kind);
        self.code(5, h);
        self.code(330, table);
        self.code(100, "AcDbSymbolTableRecord");
        self.code(100, subclass);
    }
    /// A BLOCK and its ENDBLK, empty, owned by the block record `owner`.
    fn block(&mut self, owner: &str, name: &str, paper: bool) {
        for (kind, sub) in [("BLOCK", "AcDbBlockBegin"), ("ENDBLK", "AcDbBlockEnd")] {
            let h = self.handle();
            self.code(0, kind);
            self.code(5, h);
            self.code(330, owner);
            self.code(100, "AcDbEntity");
            if paper {
                self.code(67, 1);
            }
            self.code(8, "0");
            self.code(100, sub);
            if kind == "BLOCK" {
                self.code(2, name);
                self.code(70, 0);
                self.point(10, Vec2::ZERO);
                self.code(3, name);
                self.code(1, "");
            }
        }
    }
}

/// A text on one line, as a TEXT entity holds it: at most 250 characters, control characters as
/// spaces, letters outside ASCII as `\U+XXXX` (`?` beyond the first 65536).
fn text_value(s: &str) -> String {
    s.chars()
        .take(250)
        .map(|c| match c {
            c if c.is_ascii_control() => " ".to_string(),
            c if c.is_ascii() => c.to_string(),
            c => u16::try_from(u32::from(c)).map_or_else(|_| "?".to_string(), |u| format!("\\U+{u:04X}")),
        })
        .collect()
}

/// A DXF drawing (AutoCAD 2000, millimetres) of `entities`. `layers` are the layers they are on
/// (layer `0` is always there).
pub fn write_dxf_drawing(layers: &[DxfLayer], entities: &[DxfEntity]) -> String {
    let mut pts = Vec::new();
    for e in entities {
        e.shape.sample(&mut pts);
    }
    pts.retain(|p| p.is_finite());
    let (lo, hi) = pts
        .iter()
        .fold(None, |acc: Option<(Vec2, Vec2)>, p| {
            Some(acc.map_or((*p, *p), |(lo, hi)| (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))))
        })
        .unwrap_or((Vec2::ZERO, Vec2::ZERO));

    let mut layer_list = vec![DxfLayer::new("0", 7)];
    for l in layers {
        if !layer_list.iter().any(|x| x.name == l.name) {
            layer_list.push(l.clone());
        }
    }

    // Everything but the header, which needs the handle seed.
    let mut o = Out { s: String::new(), next: 0x20 };
    let (model, paper) = (o.handle(), o.handle());
    let root = o.handle();

    o.code(0, "SECTION");
    o.code(2, "TABLES");
    // The three every drawing has, and DASHED, which the dashed layers name: dash and gap lengths
    // (a gap is negative) after the pattern's total.
    let mut linetypes: Vec<(&str, &str, &[f64])> = vec![
        ("ByBlock", "", &[]),
        ("ByLayer", "", &[]),
        ("Continuous", "Solid line", &[]),
        ("DASHED", "Dashed __ __ __ __ __ __ __ __ __ __", &[12.7, -6.35]),
    ];
    if layer_list.iter().any(|l| l.linetype == DxfLinetype::Hidden) {
        linetypes.push(("HIDDEN", "Hidden edges", &[2.0, -1.0]));
    }
    let t = o.table("LTYPE", linetypes.len());
    for (name, description, pattern) in linetypes {
        o.record("LTYPE", "AcDbLinetypeTableRecord", &t);
        o.code(2, name);
        o.code(70, 0);
        o.code(3, description);
        o.code(72, 65);
        o.code(73, pattern.len());
        o.num(40, pattern.iter().map(|d| d.abs()).sum());
        for d in pattern {
            o.num(49, *d);
            o.code(74, 0);
        }
    }
    o.code(0, "ENDTAB");
    let t = o.table("LAYER", layer_list.len());
    for l in &layer_list {
        o.record("LAYER", "AcDbLayerTableRecord", &t);
        o.code(2, &l.name);
        o.code(70, 0);
        o.code(62, l.color);
        o.code(6, l.linetype.name());
    }
    o.code(0, "ENDTAB");
    // The one text style, which a TEXT entity uses when it names none.
    let t = o.table("STYLE", 1);
    o.record("STYLE", "AcDbTextStyleTableRecord", &t);
    o.code(2, "Standard");
    o.code(70, 0);
    o.num(40, 0.0);
    o.num(41, 1.0);
    o.num(50, 0.0);
    o.code(71, 0);
    o.num(42, 2.5);
    o.code(3, "txt");
    o.code(4, "");
    o.code(0, "ENDTAB");
    let t = o.table("BLOCK_RECORD", 2);
    for (h, name) in [(&model, "*Model_Space"), (&paper, "*Paper_Space")] {
        o.code(0, "BLOCK_RECORD");
        o.code(5, h);
        o.code(330, &t);
        o.code(100, "AcDbSymbolTableRecord");
        o.code(100, "AcDbBlockTableRecord");
        o.code(2, name);
    }
    o.code(0, "ENDTAB");
    o.code(0, "ENDSEC");

    o.code(0, "SECTION");
    o.code(2, "BLOCKS");
    o.block(&model, "*Model_Space", false);
    o.block(&paper, "*Paper_Space", true);
    o.code(0, "ENDSEC");

    o.code(0, "SECTION");
    o.code(2, "ENTITIES");
    for e in entities {
        let kind = match &e.shape {
            DxfShape::Line(..) => "LINE",
            DxfShape::Polyline { .. } => "LWPOLYLINE",
            DxfShape::Circle { .. } => "CIRCLE",
            DxfShape::Arc { .. } => "ARC",
            DxfShape::Ellipse { .. } => "ELLIPSE",
            DxfShape::Spline { .. } => "SPLINE",
            DxfShape::Text { .. } => "TEXT",
        };
        let h = o.handle();
        o.code(0, kind);
        o.code(5, h);
        o.code(330, &model);
        o.code(100, "AcDbEntity");
        o.code(8, if e.layer.trim().is_empty() { "0" } else { e.layer.as_str() });
        match &e.shape {
            DxfShape::Polyline { points } => {
                o.code(100, "AcDbPolyline");
                o.code(90, points.len());
                // Autodesk DXF reference: bit 128 (PLINEGEN) continues the dash pattern.
                o.code(70, 128);
                for p in points {
                    o.num(10, p.x);
                    o.num(20, p.y);
                }
            }
            DxfShape::Line(a, b) => {
                o.code(100, "AcDbLine");
                o.point(10, *a);
                o.point(11, *b);
            }
            DxfShape::Circle { c, r } => {
                o.code(100, "AcDbCircle");
                o.point(10, *c);
                o.num(40, *r);
            }
            DxfShape::Arc { c, r, a0, a1 } => {
                o.code(100, "AcDbCircle");
                o.point(10, *c);
                o.num(40, *r);
                o.code(100, "AcDbArc");
                o.num(50, *a0);
                o.num(51, *a1);
            }
            DxfShape::Ellipse { c, major, ratio, t0, t1 } => {
                o.code(100, "AcDbEllipse");
                o.point(10, *c);
                o.point(11, *major);
                o.num(210, 0.0);
                o.num(220, 0.0);
                o.num(230, 1.0);
                o.num(40, *ratio);
                o.num(41, *t0);
                o.num(42, *t1);
            }
            DxfShape::Spline { degree, knots, ctrl, weights } => {
                o.code(100, "AcDbSpline");
                o.num(210, 0.0);
                o.num(220, 0.0);
                o.num(230, 1.0);
                // Flags: 4 rational, 8 planar.
                o.code(70, 8 + if weights.is_some() { 4 } else { 0 });
                o.code(71, degree);
                o.code(72, knots.len());
                o.code(73, ctrl.len());
                o.code(74, 0);
                // Knots, then weights, then control points: the order of the reference.
                for k in knots {
                    o.num(40, *k);
                }
                for w in weights.iter().flatten() {
                    o.num(41, *w);
                }
                for p in ctrl {
                    o.point(10, *p);
                }
            }
            DxfShape::Text { at, height, angle_deg, text } => {
                o.code(100, "AcDbText");
                o.point(10, *at);
                o.num(40, *height);
                o.code(1, text_value(text));
                o.num(50, *angle_deg);
                // Centred (72 = 1): a reader that places the text by the second alignment point
                // and one that takes the first both put its middle on `at`.
                o.code(72, 1);
                o.point(11, *at);
                o.code(100, "AcDbText");
                o.code(73, 0);
            }
        }
    }
    o.code(0, "ENDSEC");

    o.code(0, "SECTION");
    o.code(2, "OBJECTS");
    o.code(0, "DICTIONARY");
    o.code(5, root.as_str());
    o.code(330, 0);
    o.code(100, "AcDbDictionary");
    o.code(281, 1);
    o.code(0, "ENDSEC");
    o.code(0, "EOF");

    let mut head = Out { s: String::new(), next: 0 };
    head.code(0, "SECTION");
    head.code(2, "HEADER");
    head.code(9, "$ACADVER");
    head.code(1, "AC1015");
    head.code(9, "$HANDSEED");
    head.code(5, format!("{:X}", o.next));
    head.code(9, "$INSUNITS");
    head.code(70, 4);
    head.code(9, "$MEASUREMENT");
    head.code(70, 1);
    head.code(9, "$EXTMIN");
    head.point(10, lo);
    head.code(9, "$EXTMAX");
    head.point(10, hi);
    head.code(0, "ENDSEC");
    head.s + &o.s
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

    use super::*;
    use crate::{Geom2, read_dxf};

    /// The conic quarter of the ellipse `c ± u`, `c ± v` from `c + u` to `c + v`.
    fn quarter(c: Vec2, u: Vec2, v: Vec2) -> Seg2 {
        Seg2::Conic { a: c + u, apex: c + u + v, b: c + v, w: FRAC_1_SQRT_2 }
    }

    /// The ellipse of 50 × 24 round (50, 40): its four quarters, counter-clockwise from (75, 40).
    fn oval() -> [Seg2; 4] {
        let (c, u, v) = (Vec2::new(50.0, 40.0), Vec2::new(25.0, 0.0), Vec2::new(0.0, 12.0));
        [quarter(c, u, v), quarter(c, v, -u), quarter(c, -u, -v), quarter(c, -v, u)]
    }

    fn ellipse_point(e: &DxfShape, t: f64) -> Vec2 {
        let DxfShape::Ellipse { c, major, ratio, .. } = e else { panic!("{e:?}") };
        *c + *major * t.cos() + major.perp() * (*ratio * t.sin())
    }

    /// A DXF text as (group code, value) pairs.
    fn pairs(dxf: &str) -> Vec<(i32, String)> {
        let lines: Vec<&str> = dxf.lines().collect();
        lines.chunks(2).map(|c| (c[0].trim().parse().unwrap(), c.get(1).unwrap_or(&"").trim().to_string())).collect()
    }

    /// The entities of the ENTITIES section: their type and group codes.
    fn entities(dxf: &str) -> Vec<(String, Vec<(i32, String)>)> {
        let p = pairs(dxf);
        let start = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "ENTITIES")).unwrap();
        let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
        for (c, v) in &p[start + 1..] {
            match (*c, v.as_str()) {
                (0, "ENDSEC") => break,
                (0, kind) => out.push((kind.to_string(), Vec::new())),
                _ => out.last_mut().unwrap().1.push((*c, v.clone())),
            }
        }
        out
    }

    fn values(e: &(String, Vec<(i32, String)>), code: i32) -> Vec<f64> {
        e.1.iter().filter(|(c, _)| *c == code).map(|(_, v)| v.parse().unwrap()).collect()
    }

    fn near(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_conic_arc_below_weight_one_is_an_elliptical_arc() {
        // By hand: the first quarter of the 25 × 12 ellipse round (50, 40) runs from (75, 40)
        // over the corner (75, 52) to (50, 52) with weight cos 45°. Its centre is
        // m − (apex − m)·w²/(1 − w²) = (62.5, 46) − (12.5, 6)·1 = (50, 40); the semi-diameters
        // u = (m − c)/w and v = (b − m)/sin 45° are (25, 12) and (−25, 12) over √2, which are
        // equally long, so the principal axes are 45° from them: the major one is
        // (u − v)/√2 = (25, 0), the minor one (u + v)/√2 = (0, 12), the ratio 12/25 and, from
        // (75, 40) to (50, 52), the parameters 0 to π/2.
        let [first, second, ..] = oval();
        let DxfShape::Ellipse { c, major, ratio, t0, t1 } = seg_shape(&first) else { panic!("{:?}", seg_shape(&first)) };
        assert!(c.dist(Vec2::new(50.0, 40.0)) < 1e-9 && major.dist(Vec2::new(25.0, 0.0)) < 1e-9, "{c:?} {major:?}");
        assert!((ratio - 0.48).abs() < 1e-12 && t0.abs() < 1e-9 && (t1 - FRAC_PI_2).abs() < 1e-9, "{ratio} {t0} {t1}");
        // Whatever axis the second quarter is described by, its arc starts and ends at the
        // segment's ends and every point of it lies on the segment.
        let tilt = |v: Vec2| Vec2::from_angle(0.3) * v.x + Vec2::from_angle(0.3).perp() * v.y;
        let (centre, u, v) = (Vec2::new(5.0, -2.0), tilt(Vec2::new(15.0, 0.0)), tilt(Vec2::new(0.0, 8.0)));
        let general = quarter(centre, u, v).split_at(0.3).1;
        for seg in [first, second, quarter(centre, u, v), quarter(centre, v, -u).reversed(), general, general.reversed()] {
            let e = seg_shape(&seg);
            let DxfShape::Ellipse { t0, t1, .. } = e else { panic!("{e:?}") };
            let sweep = (t1 - t0).rem_euclid(TAU);
            for i in 0..=20 {
                let p = ellipse_point(&e, t0 + sweep * i as f64 / 20.0);
                assert!(seg.point_at(seg.closest_param(p)).dist(p) < 1e-9, "{seg:?} {p:?}");
            }
            let ends = [ellipse_point(&e, t0), ellipse_point(&e, t0 + sweep)];
            assert!(ends.iter().any(|p| p.dist(seg.start()) < 1e-9) && ends.iter().any(|p| p.dist(seg.end()) < 1e-9), "{ends:?} {seg:?}");
        }
    }

    #[test]
    fn four_quarters_are_one_ellipse() {
        let quarters = oval();
        let reversed = Loop2 { segs: quarters.iter().rev().map(Seg2::reversed).collect() };
        let shifted = Loop2 { segs: quarters.iter().cycle().skip(2).take(4).copied().collect() };
        for lp in [Loop2 { segs: quarters.to_vec() }, reversed, shifted] {
            let shapes = loop_shapes(&lp);
            let [DxfShape::Ellipse { c, major, ratio, t0, t1 }] = shapes.as_slice() else { panic!("{shapes:?}") };
            assert!(
                c.dist(Vec2::new(50.0, 40.0)) < 1e-9 && (major.len() - 25.0).abs() < 1e-9 && major.cross(Vec2::X).abs() < 1e-9,
                "{c:?} {major:?}"
            );
            assert!((ratio - 0.48).abs() < 1e-12 && *t0 == 0.0 && *t1 == TAU, "{ratio} {t0} {t1}");
        }
        // Three quarters and a line are pieces: three ELLIPSE arcs and a LINE.
        let (q, line) = (oval(), Seg2::Line { a: Vec2::new(50.0, 28.0), b: Vec2::new(50.0, 28.0) + Vec2::X });
        let open = loop_shapes(&Loop2 { segs: vec![q[0], q[1], q[2], line] });
        assert!(matches!(open.as_slice(), [DxfShape::Ellipse { t1, .. }, _, _, DxfShape::Line(..)] if (*t1 - FRAC_PI_2).abs() < 1e-9), "{open:?}");
        // A circle of arcs is the caller's business, but a whole turn is one CIRCLE.
        let turn = Seg2::Arc { center: Vec2::new(3.0, 4.0), radius: 2.0, start: -PI, sweep: -TAU };
        assert_eq!(seg_shape(&turn), DxfShape::Circle { c: Vec2::new(3.0, 4.0), r: 2.0 });
    }

    #[test]
    fn beziers_and_the_other_conics_are_splines() {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 3.0), Vec2::new(4.0, 0.0)];
        let cubic = seg_shape(&Seg2::Cubic { p0: p[0], p1: p[1], p2: p[2], p3: p[3] });
        assert_eq!(cubic, DxfShape::Spline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], ctrl: p.to_vec(), weights: None });
        // A quadratic (a glyph curve) is a parabola: degree 2, not rational. Weight 2 is a
        // hyperbola: rational. Weight 1 − 10⁻⁹ is no ellipse a reader could place: rational too.
        let conic = |w: f64| seg_shape(&Seg2::Conic { a: p[0], apex: p[1], b: p[3], w });
        let degree_2 = |s: &DxfShape, weights: Option<Vec<f64>>| {
            *s == DxfShape::Spline { degree: 2, knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], ctrl: vec![p[0], p[1], p[3]], weights }
        };
        assert!(degree_2(&conic(1.0), None));
        assert!(degree_2(&conic(2.0), Some(vec![1.0, 2.0, 1.0])));
        assert!(degree_2(&conic(1.0 - 1e-9), Some(vec![1.0, 1.0 - 1e-9, 1.0])));
        // A quintic piece: six control points and twelve knots.
        let mut q = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
        for (i, s) in q.iter_mut().enumerate() {
            *s = Vec2::new(i as f64, (i * i) as f64);
        }
        let DxfShape::Spline { degree, knots, ctrl, weights } = seg_shape(&Seg2::Bezier { n: 5, p: q }) else { panic!() };
        assert_eq!((degree, knots.len(), ctrl.len(), weights), (5, 12, 6, None));
        assert!(knots[..6].iter().all(|k| *k == 0.0) && knots[6..].iter().all(|k| *k == 1.0) && ctrl[5] == q[5]);
    }

    #[test]
    fn a_bezier_is_a_spline_of_its_own_degree_over_all_its_control_points() {
        let mut q = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
        for (i, s) in q.iter_mut().enumerate() {
            *s = Vec2::new(i as f64, (i * i) as f64);
        }
        // Every degree a Bézier piece can have, up to a G4 blend's 9: n + 1 control points, all of
        // them, and 2(n + 1) clamped knots.
        for n in 4..=solidcraft_geom::MAX_BEZIER_DEGREE as u8 {
            let k = usize::from(n) + 1;
            let DxfShape::Spline { degree, knots, ctrl, weights } = seg_shape(&Seg2::Bezier { n, p: q }) else { panic!("degree {n}") };
            assert_eq!((degree, knots.len(), weights), (n, 2 * k, None), "degree {n}");
            assert_eq!(ctrl, q[..k], "degree {n}");
        }
        // A degree no piece can have (hostile data) is held to what the piece holds, never past it.
        let shape = |n: u8| seg_shape(&Seg2::Bezier { n, p: q });
        assert!(matches!(shape(0), DxfShape::Spline { degree: 1, ref ctrl, .. } if ctrl.len() == 2));
        assert!(matches!(shape(u8::MAX), DxfShape::Spline { degree: 9, ref ctrl, .. } if ctrl[..] == q[..]));
    }

    #[test]
    fn drawings_carry_version_units_extents_and_handles() {
        let ents = vec![
            DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Line(Vec2::new(-1.0, 2.0), Vec2::new(10.0, 2.0)) },
            DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 4.0 } },
            DxfEntity { layer: "CUT".into(), shape: DxfShape::Line(Vec2::ZERO, Vec2::X) },
        ];
        let dxf = write_dxf_drawing(&[DxfLayer::new("OUTLINE", 7), DxfLayer::new("CUT", 1)], &ents);
        let p = pairs(&dxf);
        let after = |var: &str| &p[p.iter().position(|(c, v)| (*c, v.as_str()) == (9, var)).unwrap() + 1..];
        assert_eq!(after("$ACADVER")[0], (1, "AC1015".to_string()));
        assert_eq!(after("$INSUNITS")[0], (70, "4".to_string()));
        // The circle reaches (1, 1) to (9, 9), the first line starts at x = −1 and the last at y = 0.
        assert_eq!(&after("$EXTMIN")[..3], &[(10, "-1.0".to_string()), (20, "0.0".to_string()), (30, "0.0".to_string())]);
        assert_eq!(&after("$EXTMAX")[..2], &[(10, "10.0".to_string()), (20, "9.0".to_string())]);
        // Layer 0 and the two layers are in the LAYER table, each entity is on one of them,
        // and the entities belong to the model space block record.
        let layer_table = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "LAYER")).unwrap();
        let names: Vec<&str> = p[layer_table..]
            .iter()
            .take_while(|(c, v)| (*c, v.as_str()) != (0, "ENDTAB"))
            .filter(|(c, _)| *c == 2)
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(names, ["LAYER", "0", "OUTLINE", "CUT"]);
        // (The first "*Model_Space" is the BLOCK_RECORD, whose handle is four pairs before its name.)
        let model_at = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "*Model_Space")).unwrap();
        let model = p[model_at - 4].1.clone();
        assert_eq!(p[model_at - 4].0, 5);
        for e in entities(&dxf) {
            assert!(e.1.contains(&(330, model.clone())) && e.1.contains(&(100, "AcDbEntity".to_string())), "{e:?}");
            assert!(names.contains(&e.1.iter().find(|(c, _)| *c == 8).unwrap().1.as_str()));
        }
        // Handles are unique and the seed is above all of them.
        let seed_at = p.iter().position(|(c, v)| (*c, v.as_str()) == (9, "$HANDSEED")).unwrap() + 1;
        let handles: Vec<u64> =
            p.iter().enumerate().filter(|(i, (c, _))| *i != seed_at && *c == 5).map(|(_, (_, v))| u64::from_str_radix(v, 16).unwrap()).collect();
        let mut sorted = handles.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), handles.len());
        assert!(handles.len() > ents.len() && handles.iter().all(|h| *h < u64::from_str_radix(&p[seed_at].1, 16).unwrap()));
        assert!(dxf.ends_with("  0\nEOF\n"));
        // No entities: a valid, empty drawing.
        assert_eq!(entities(&write_dxf_drawing(&[], &[])).len(), 0);
    }

    #[test]
    fn what_is_written_is_what_the_reader_gets_back() {
        let quarter_one = seg_shape(&oval()[0]);
        let cubic = [Vec2::new(0.0, 0.0), Vec2::new(5.0, 10.0), Vec2::new(15.0, 10.0), Vec2::new(20.0, 0.0)];
        let shapes = vec![
            DxfShape::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
            DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 2.0 },
            DxfShape::Arc { c: Vec2::ZERO, r: 3.0, a0: 0.0, a1: 90.0 },
            loop_shapes(&Loop2 { segs: oval().to_vec() }).remove(0),
            quarter_one,
            seg_shape(&Seg2::Cubic { p0: cubic[0], p1: cubic[1], p2: cubic[2], p3: cubic[3] }),
        ];
        let ents: Vec<DxfEntity> = shapes.into_iter().map(|shape| DxfEntity { layer: "0".into(), shape }).collect();
        let dxf = write_dxf_drawing(&[], &ents);
        let back = read_dxf(&dxf).unwrap();
        assert_eq!(back.len(), 6, "{back:?}");
        assert!(matches!(&back[0], Geom2::Line(a, b) if a.dist(Vec2::ZERO) < 1e-9 && b.dist(Vec2::new(10.0, 0.0)) < 1e-9));
        assert!(matches!(&back[1], Geom2::Circle(c, r) if c.dist(Vec2::new(5.0, 5.0)) < 1e-9 && (r - 2.0).abs() < 1e-9));
        assert!(
            matches!(&back[2], Geom2::Arc { c, a, b } if c.dist(Vec2::ZERO) < 1e-9 && a.dist(Vec2::new(3.0, 0.0)) < 1e-9 && b.dist(Vec2::new(0.0, 3.0)) < 1e-9)
        );
        // The whole ellipse: centre, end of the major axis, minor radius.
        assert!(
            matches!(&back[3], Geom2::Ellipse { c, major, minor } if c.dist(Vec2::new(50.0, 40.0)) < 1e-9 && major.dist(Vec2::new(75.0, 40.0)) < 1e-9 && (minor - 12.0).abs() < 1e-9),
            "{:?}",
            back[3]
        );
        // The reader makes an elliptical arc a spline through points of it: from (75, 40) to (50, 52).
        let Geom2::Spline { pts, control: false, .. } = &back[4] else { panic!("{:?}", back[4]) };
        let on_ellipse = |p: &Vec2| (((p.x - 50.0) / 25.0).powi(2) + ((p.y - 40.0) / 12.0).powi(2) - 1.0).abs() < 1e-9;
        assert!(
            pts.first().is_some_and(|p| p.dist(Vec2::new(75.0, 40.0)) < 1e-9) && pts.last().is_some_and(|p| p.dist(Vec2::new(50.0, 52.0)) < 1e-9)
        );
        assert!(pts.iter().all(on_ellipse));
        // The cubic: degree and control points.
        let Geom2::Spline { pts, control: true, degree: 3 } = &back[5] else { panic!("{:?}", back[5]) };
        assert!(pts.iter().zip(&cubic).all(|(a, b)| a.dist(*b) < 1e-9) && pts.len() == 4);
        // A degree-9 Bézier (a G4 blend) comes back at its own degree over all ten points.
        let mut q = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
        for (i, s) in q.iter_mut().enumerate() {
            *s = Vec2::new(i as f64, (i % 3) as f64);
        }
        let nine = write_dxf_drawing(&[], &[DxfEntity { layer: "0".into(), shape: seg_shape(&Seg2::Bezier { n: 9, p: q }) }]);
        let back_nine = read_dxf(&nine).unwrap();
        assert!(
            matches!(&back_nine[..], [Geom2::Spline { pts, control: true, degree: 9 }] if pts.len() == 10 && pts.iter().zip(&q).all(|(a, b)| a.dist(*b) < 1e-9)),
            "{back_nine:?}"
        );

        // The numbers in the file itself, which the reader does not keep: the ellipse...
        let file = entities(&dxf);
        assert_eq!(file.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["LINE", "CIRCLE", "ARC", "ELLIPSE", "ELLIPSE", "SPLINE"]);
        assert!(near(&values(&file[3], 10), &[50.0]) && near(&values(&file[3], 20), &[40.0]) && near(&values(&file[3], 11), &[25.0]));
        assert!(near(&values(&file[3], 21), &[0.0]) && near(&values(&file[3], 40), &[0.48]) && near(&values(&file[3], 41), &[0.0]));
        assert!(near(&values(&file[3], 42), &[TAU]) && near(&values(&file[3], 230), &[1.0]));
        // ...the elliptical arc from parameter 0 to π/2...
        assert!(near(&values(&file[4], 41), &[0.0]) && near(&values(&file[4], 42), &[FRAC_PI_2]) && near(&values(&file[4], 40), &[0.48]));
        // ...and the spline: planar, degree 3, eight knots, four control points.
        let spline = &file[5];
        assert_eq!(values(spline, 70), [8.0]);
        assert_eq!((values(spline, 71), values(spline, 72), values(spline, 73), values(spline, 74)), (vec![3.0], vec![8.0], vec![4.0], vec![0.0]));
        assert!(near(&values(spline, 40), &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]));
        assert!(near(&values(spline, 10), &[0.0, 5.0, 15.0, 20.0]) && near(&values(spline, 20), &[0.0, 10.0, 10.0, 0.0]));
        assert!(values(spline, 41).is_empty());
    }

    #[test]
    fn a_rational_spline_lists_knots_then_weights_then_control_points() {
        let hyperbola = seg_shape(&Seg2::Conic { a: Vec2::ZERO, apex: Vec2::new(1.0, 2.0), b: Vec2::new(3.0, 0.0), w: 2.0 });
        let dxf = write_dxf_drawing(&[], &[DxfEntity { layer: "0".into(), shape: hyperbola }]);
        let e = &entities(&dxf)[0];
        assert_eq!(values(e, 70), [12.0]);
        assert!(near(&values(e, 41), &[1.0, 2.0, 1.0]) && near(&values(e, 40), &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]));
        let at = |code: i32| e.1.iter().position(|(c, _)| *c == code).unwrap();
        let last = |code: i32| e.1.iter().rposition(|(c, _)| *c == code).unwrap();
        assert!(last(40) < at(41) && last(41) < at(10), "{:?}", e.1);
    }

    /// The records of a symbol table, each as its group codes after the type.
    fn table_records(p: &[(i32, String)], table: &str) -> Vec<Vec<(i32, String)>> {
        let start = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, table)).unwrap();
        let mut out: Vec<Vec<(i32, String)>> = Vec::new();
        for (c, v) in &p[start + 1..] {
            match (*c, v.as_str()) {
                (0, "ENDTAB") => break,
                (0, _) => out.push(Vec::new()),
                _ => {
                    if let Some(record) = out.last_mut() {
                        record.push((*c, v.clone()));
                    }
                }
            }
        }
        out
    }

    fn field(record: &[(i32, String)], code: i32) -> &str {
        record.iter().find(|(c, _)| *c == code).map_or("", |(_, v)| v.as_str())
    }

    #[test]
    fn a_dashed_layer_names_its_linetype_and_the_table_defines_it() {
        let dxf = write_dxf_drawing(&[DxfLayer::new("BEND", 1).dashed(), DxfLayer::new("CUT", 4)], &[]);
        let p = pairs(&dxf);
        // The linetypes of every drawing, then DASHED: a dash of 12.7 mm and a gap of 6.35 mm, 19.05 mm in all.
        let types = table_records(&p, "LTYPE");
        assert_eq!(types.iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["ByBlock", "ByLayer", "Continuous", "DASHED"]);
        let dashed = &types[3];
        let numbers = |code: i32| dashed.iter().filter(|(c, _)| *c == code).map(|(_, v)| v.parse::<f64>().unwrap()).collect::<Vec<_>>();
        assert!(near(&numbers(49), &[12.7, -6.35]) && near(&numbers(40), &[19.05]) && near(&numbers(73), &[2.0]) && near(&numbers(72), &[65.0]));
        // The table says how many records it holds.
        let at = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "LTYPE")).unwrap();
        assert_eq!(p[at..].iter().find(|(c, _)| *c == 70).map(|(_, v)| v.as_str()), Some("4"));
        // Each layer names its linetype.
        let layers = table_records(&p, "LAYER");
        assert_eq!(
            layers.iter().map(|r| (field(r, 2), field(r, 62), field(r, 6))).collect::<Vec<_>>(),
            [("0", "7", "Continuous"), ("BEND", "1", "DASHED"), ("CUT", "4", "Continuous")]
        );
    }

    #[test]
    fn hidden_polylines_continue_short_dashes_across_chords_and_round_trip() {
        let points = vec![Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(2., 1.), Vec2::new(3., 1.)];
        let layer = DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", 8) };
        let text = write_dxf_drawing(&[layer], &[DxfEntity { layer: "HIDDEN".into(), shape: DxfShape::Polyline { points } }]);
        let records = entities(&text);
        assert_eq!(records[0].0, "LWPOLYLINE");
        assert_eq!(field(&records[0].1, 70), "128");
        assert_eq!(field(&records[0].1, 90), "4");
        let types = table_records(&pairs(&text), "LTYPE");
        let hidden = types.iter().find(|r| field(r, 2) == "HIDDEN").unwrap();
        assert_eq!(hidden.iter().filter(|(c, _)| *c == 49).map(|(_, v)| v.parse::<f64>().unwrap()).collect::<Vec<_>>(), [2., -1.]);
        assert_eq!(read_dxf(&text).unwrap().len(), 3);
    }

    #[test]
    fn text_is_centred_on_its_point_and_the_reader_gets_it_back() {
        let at = Vec2::new(30.0, 12.0);
        let note = DxfShape::Text { at, height: 3.0, angle_deg: 90.0, text: "AB%%d\nC\u{b0}".into() };
        let dxf = write_dxf_drawing(&[DxfLayer::new("NOTES", 8)], &[DxfEntity { layer: "NOTES".into(), shape: note }]);
        let file = entities(&dxf);
        let [text] = file.as_slice() else { panic!("{file:?}") };
        assert_eq!(text.0, "TEXT");
        // One line (the newline is a space), the sign outside ASCII a code of its own, `%%d` AutoCAD's degree sign.
        assert_eq!(text.1.iter().find(|(c, _)| *c == 1).map(|(_, v)| v.as_str()), Some("AB%%d C\\U+00B0"));
        // Centred (72 = 1) on the second alignment point, which is the first as well; 3 mm high, turned 90°.
        assert!(
            near(&values(text, 11), &[30.0])
                && near(&values(text, 21), &[12.0])
                && near(&values(text, 10), &[30.0])
                && near(&values(text, 20), &[12.0])
        );
        assert!(
            near(&values(text, 72), &[1.0]) && near(&values(text, 73), &[0.0]) && near(&values(text, 40), &[3.0]) && near(&values(text, 50), &[90.0])
        );
        // Its style is in the STYLE table, and the layer it is on in the LAYER table.
        let p = pairs(&dxf);
        assert_eq!(table_records(&p, "STYLE").iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["Standard"]);
        assert!(text.1.contains(&(8, "NOTES".to_string())));
        // The reader finds the text where it was put, as high and as turned.
        let back = read_dxf(&dxf).unwrap();
        assert!(
            matches!(&back[..], [Geom2::Text { at: found, height, angle, .. }] if found.dist(at) < 1e-9 && (height - 3.0).abs() < 1e-9 && (angle - FRAC_PI_2).abs() < 1e-9),
            "{back:?}"
        );
    }

    #[test]
    fn the_extents_hold_the_text() {
        // Five letters of 3 mm, turned to read upwards: 0.8 × 3 × 5 = 12 mm long around y = 12, and
        // 3 mm to the left of the baseline at x = 30.
        let shape = DxfShape::Text { at: Vec2::new(30.0, 12.0), height: 3.0, angle_deg: 90.0, text: "ABCDE".into() };
        let dxf = write_dxf_drawing(&[], &[DxfEntity { layer: "0".into(), shape }]);
        let p = pairs(&dxf);
        let after = |var: &str| &p[p.iter().position(|(c, v)| (*c, v.as_str()) == (9, var)).unwrap() + 1..];
        assert_eq!(&after("$EXTMIN")[..2], &[(10, "27.0".to_string()), (20, "6.0".to_string())]);
        assert_eq!(&after("$EXTMAX")[..2], &[(10, "30.0".to_string()), (20, "18.0".to_string())]);
    }
}
