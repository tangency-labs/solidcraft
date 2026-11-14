//! Writing a sketch as an SVG drawing: millimetres (or centimetres or inches) at 1:1.
//!
//! The file's `width` and `height` carry the unit and its `viewBox` is the same size without one,
//! so one user unit is one unit of length and the drawing opens at its real size in a program that
//! reads the physical size (a CNC router's software, a laser cutter's, a drawing program). The
//! drawing's lower left corner is the origin, the Y axis points down as SVG wants (so the picture
//! is the sketch as it looks on screen), and a closed profile is one `<path>` ending in `Z` with
//! its holes as further sub-paths, so the receiving program sees a closed contour.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::Write as _;

use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_geom::prelude::*;

use crate::{IoError, Result};

/// The distance within which a curve drawn as a polyline follows the curve, in the sketch's
/// millimetres. The same sampling the DXF writer uses for free-form curves.
const FLATTEN_TOL: f64 = 0.01;

/// A line this thin (as a share of a millimetre) is drawn so a viewer shows it.
const STROKE_MM: f64 = 0.1;

/// One thing in the drawing, in the sketch's own millimetres with Y up.
#[derive(Clone, Debug)]
pub enum SvgShape<'a> {
    /// A straight line.
    Line(Vec2, Vec2),
    /// A full circle: centre, radius.
    Circle(Vec2, f64),
    /// A counter-clockwise arc about `c` from `a` to `b`; the same point twice is a full circle.
    Arc {
        /// The centre.
        c: Vec2,
        /// Where the arc starts.
        a: Vec2,
        /// Where the arc ends.
        b: Vec2,
    },
    /// Connected straight pieces, open (a free-form curve sampled).
    Polyline(Vec<Vec2>),
    /// A closed profile: the outline and the holes cut out of it.
    Region {
        /// The outline.
        outer: &'a Loop2,
        /// The holes.
        holes: Vec<&'a Loop2>,
    },
}

/// The units a drawing is written in: the suffix of `width` and `height` and how many of them are
/// in a millimetre. `None` for a unit SVG has no name for.
pub fn svg_unit(name: &str) -> Option<(&'static str, f64)> {
    match name.to_ascii_lowercase().as_str() {
        "mm" => Some(("mm", 1.0)),
        "cm" => Some(("cm", 0.1)),
        "in" | "inch" => Some(("in", 1.0 / 25.4)),
        _ => None,
    }
}

/// The corners of the box round what is drawn.
struct Bounds {
    min: Vec2,
    max: Vec2,
    any: bool,
}

impl Bounds {
    fn new() -> Self {
        Bounds { min: Vec2::new(f64::MAX, f64::MAX), max: Vec2::new(f64::MIN, f64::MIN), any: false }
    }

    fn add(&mut self, p: Vec2) {
        self.min = Vec2::new(self.min.x.min(p.x), self.min.y.min(p.y));
        self.max = Vec2::new(self.max.x.max(p.x), self.max.y.max(p.y));
        self.any = true;
    }

    /// An arc's ends and each of the compass points it passes through.
    fn add_arc(&mut self, c: Vec2, r: f64, start: f64, sweep: f64) {
        self.add(c + Vec2::from_angle(start) * r);
        self.add(c + Vec2::from_angle(start + sweep) * r);
        for k in 0..4 {
            let at = f64::from(k) * FRAC_PI_2;
            if (at - start).rem_euclid(TAU) <= sweep {
                self.add(c + Vec2::from_angle(at) * r);
            }
        }
    }

    fn add_loop(&mut self, lp: &Loop2) {
        for s in &lp.segs {
            match *s {
                Seg2::Line { a, b } => {
                    self.add(a);
                    self.add(b);
                }
                Seg2::Arc { center, radius, start, sweep } => {
                    if sweep >= 0.0 {
                        self.add_arc(center, radius, start, sweep);
                    } else {
                        self.add_arc(center, radius, start + sweep, -sweep);
                    }
                }
                _ => s.polyline(FLATTEN_TOL).into_iter().for_each(|p| self.add(p)),
            }
        }
    }
}

/// The start angle and counter-clockwise sweep of the arc about `c` from `a` to `b`; the same
/// point twice is a full turn.
fn arc_angles(c: Vec2, a: Vec2, b: Vec2) -> (f64, f64) {
    let start = (a - c).angle();
    let sweep = ((b - c).angle() - start).rem_euclid(TAU);
    (start, if sweep < 1e-12 { TAU } else { sweep })
}

/// A number with at most four decimals and no trailing zeros; `-0` is `0`.
fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_string() } else { s.to_string() }
}

/// Where a sketch point lands on the page: from the drawing's lower left, Y flipped, in the unit.
struct Page {
    left: f64,
    top: f64,
    k: f64,
}

impl Page {
    fn xy(&self, p: Vec2) -> String {
        format!("{} {}", num((p.x - self.left) * self.k), num((self.top - p.y) * self.k))
    }

    fn len(&self, v: f64) -> String {
        num(v * self.k)
    }

    /// An arc's `A` command to `to`; SVG's sweep flag is 1 for clockwise on the page, which is a
    /// clockwise turn here too once Y is flipped, so a counter-clockwise sketch arc is 0.
    fn arc_to(&self, r: f64, sweep: f64, to: Vec2) -> String {
        let large = u8::from(sweep.abs() > PI);
        let flag = u8::from(sweep < 0.0);
        format!("A{r} {r} 0 {large} {flag} {to}", r = self.len(r), to = self.xy(to))
    }

    /// One closed loop as sub-path data.
    fn loop_d(&self, lp: &Loop2) -> String {
        let mut d = String::new();
        let mut first = true;
        for s in &lp.segs {
            let start = match *s {
                Seg2::Line { a, .. } => a,
                Seg2::Arc { center, radius, start, .. } => center + Vec2::from_angle(start) * radius,
                _ => s.polyline(FLATTEN_TOL).first().copied().unwrap_or_default(),
            };
            if first {
                let _ = write!(d, "M{}", self.xy(start));
                first = false;
            }
            match *s {
                Seg2::Line { b, .. } => {
                    let _ = write!(d, "L{}", self.xy(b));
                }
                Seg2::Arc { center, radius, start, sweep } => {
                    if (sweep.abs() - TAU).abs() < 1e-9 {
                        // A full circle is two half turns: SVG cannot draw one from a point to itself.
                        let way = if sweep >= 0.0 { PI } else { -PI };
                        let mid = center + Vec2::from_angle(start + way) * radius;
                        let _ = write!(d, "{}{}", self.arc_to(radius, way, mid), self.arc_to(radius, way, start_point(center, radius, start)));
                    } else {
                        let end = center + Vec2::from_angle(start + sweep) * radius;
                        let _ = write!(d, "{}", self.arc_to(radius, sweep, end));
                    }
                }
                _ => {
                    for p in s.polyline(FLATTEN_TOL).into_iter().skip(1) {
                        let _ = write!(d, "L{}", self.xy(p));
                    }
                }
            }
        }
        if !d.is_empty() {
            d.push('Z');
        }
        d
    }
}

fn start_point(center: Vec2, radius: f64, start: f64) -> Vec2 {
    center + Vec2::from_angle(start) * radius
}

/// The shapes as an SVG document in `unit` (`per_mm` of them in a millimetre, from [`svg_unit`]).
/// Refused when there is nothing to draw, when a number is not finite, or when the drawing has no
/// width or no height (an SVG of zero size draws nothing).
pub fn write_svg(shapes: &[SvgShape<'_>], unit: &str, per_mm: f64) -> Result<String> {
    let mut b = Bounds::new();
    for shape in shapes {
        match shape {
            SvgShape::Line(p, q) => {
                b.add(*p);
                b.add(*q);
            }
            SvgShape::Circle(c, r) => {
                b.add(*c + Vec2::new(*r, *r));
                b.add(*c - Vec2::new(*r, *r));
            }
            SvgShape::Arc { c, a, b: end } => {
                let (start, sweep) = arc_angles(*c, *a, *end);
                b.add_arc(*c, c.dist(*a), start, sweep);
            }
            SvgShape::Polyline(points) => points.iter().for_each(|p| b.add(*p)),
            SvgShape::Region { outer, holes } => {
                b.add_loop(outer);
                holes.iter().for_each(|h| b.add_loop(h));
            }
        }
    }
    if !b.any {
        return Err(IoError::Invalid("nothing to export: the sketch has no visible curves".into()));
    }
    let (w, h) = ((b.max.x - b.min.x) * per_mm, (b.max.y - b.min.y) * per_mm);
    if !(w.is_finite() && h.is_finite() && per_mm.is_finite() && per_mm > 0.0) {
        return Err(IoError::Invalid("the sketch has a coordinate that is not a finite number".into()));
    }
    if w <= 0.0 || h <= 0.0 {
        return Err(IoError::Invalid("the visible curves have no width or no height, so the SVG would have no size".into()));
    }
    let page = Page { left: b.min.x, top: b.max.y, k: per_mm };
    let (ws, hs) = (num(w), num(h));
    let mut out = String::new();
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(out, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{ws}{unit}\" height=\"{hs}{unit}\" viewBox=\"0 0 {ws} {hs}\">");
    let _ = writeln!(out, "<g fill=\"none\" stroke=\"#000000\" stroke-width=\"{}\">", page.len(STROKE_MM));
    for shape in shapes {
        match shape {
            SvgShape::Line(p, q) => {
                let _ = writeln!(out, "<path d=\"M{}L{}\"/>", page.xy(*p), page.xy(*q));
            }
            SvgShape::Circle(c, r) => {
                let _ = writeln!(
                    out,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                    num((c.x - page.left) * page.k),
                    num((page.top - c.y) * page.k),
                    page.len(*r)
                );
            }
            SvgShape::Arc { c, a, b: end } => {
                let (start, sweep) = arc_angles(*c, *a, *end);
                let r = c.dist(*a);
                if (sweep - TAU).abs() < 1e-9 {
                    let _ = writeln!(
                        out,
                        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                        num((c.x - page.left) * page.k),
                        num((page.top - c.y) * page.k),
                        page.len(r)
                    );
                } else {
                    let _ = writeln!(out, "<path d=\"M{}{}\"/>", page.xy(*a), page.arc_to(r, sweep, *c + Vec2::from_angle(start + sweep) * r));
                }
            }
            SvgShape::Polyline(points) => {
                let mut it = points.iter();
                if let Some(first) = it.next() {
                    let mut d = format!("M{}", page.xy(*first));
                    for p in it {
                        let _ = write!(d, "L{}", page.xy(*p));
                    }
                    let _ = writeln!(out, "<path d=\"{d}\"/>");
                }
            }
            SvgShape::Region { outer, holes } => {
                let mut d = page.loop_d(outer);
                for hole in holes {
                    d.push_str(&page.loop_d(hole));
                }
                let _ = writeln!(out, "<path d=\"{d}\"/>");
            }
        }
    }
    let _ = writeln!(out, "</g>");
    let _ = writeln!(out, "</svg>");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f64, h: f64) -> Loop2 {
        let c = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)];
        Loop2 { segs: (0..4).map(|i| Seg2::Line { a: c[i], b: c[(i + 1) % 4] }).collect() }
    }

    #[test]
    fn a_rectangle_is_one_closed_path_at_its_size_in_millimetres() {
        let outer = rect(30.0, 20.0);
        let svg = write_svg(&[SvgShape::Region { outer: &outer, holes: vec![] }], "mm", 1.0).unwrap();
        assert!(svg.contains("width=\"30mm\" height=\"20mm\" viewBox=\"0 0 30 20\""), "{svg}");
        // Y is flipped: the sketch's lower left (0, 0) is the page's (0, 20).
        assert!(svg.contains("<path d=\"M0 20L30 20L30 0L0 0L0 20Z\"/>"), "{svg}");
    }

    #[test]
    fn a_hole_is_a_second_sub_path_of_the_same_closed_contour() {
        let outer = rect(30.0, 20.0);
        let hole = Loop2::circle(Vec2::new(10.0, 10.0), 4.0);
        let svg = write_svg(&[SvgShape::Region { outer: &outer, holes: vec![&hole] }], "mm", 1.0).unwrap();
        let path = svg.lines().find(|l| l.starts_with("<path")).unwrap();
        assert_eq!(path.matches('Z').count(), 2, "{path}");
        assert_eq!(path.matches('M').count(), 2, "{path}");
        // The full circle is two half turns: 4 mm radius at the page's (10, 10).
        assert_eq!(path.matches("A4 4 0 0 0").count(), 2, "{path}");
    }

    #[test]
    fn units_scale_the_size_and_the_view_box_together() {
        let outer = rect(25.4, 50.8);
        let (name, k) = svg_unit("IN").unwrap();
        let svg = write_svg(&[SvgShape::Region { outer: &outer, holes: vec![] }], name, k).unwrap();
        assert!(svg.contains("width=\"1in\" height=\"2in\" viewBox=\"0 0 1 2\""), "{svg}");
        assert_eq!(svg_unit("cm"), Some(("cm", 0.1)));
        assert_eq!(svg_unit("furlong"), None);
    }

    #[test]
    fn a_counter_clockwise_arc_keeps_its_bulge_and_the_bounds_include_it() {
        // A half circle about the origin from (10, 0) over the top to (-10, 0).
        let arc = SvgShape::Arc { c: Vec2::new(0.0, 0.0), a: Vec2::new(10.0, 0.0), b: Vec2::new(-10.0, 0.0) };
        let svg = write_svg(&[arc], "mm", 1.0).unwrap();
        // Its top (0, 10) is in the bounds: 20 wide and 10 high; it starts at the page's (20, 10).
        assert!(svg.contains("width=\"20mm\" height=\"10mm\""), "{svg}");
        assert!(svg.contains("<path d=\"M20 10A10 10 0 0 0 0 10\"/>"), "{svg}");
    }

    #[test]
    fn a_circle_is_a_circle_and_the_same_point_twice_is_a_full_circle() {
        let svg = write_svg(&[SvgShape::Circle(Vec2::new(5.0, 5.0), 2.0)], "mm", 1.0).unwrap();
        assert!(svg.contains("width=\"4mm\" height=\"4mm\""), "{svg}");
        assert!(svg.contains("<circle cx=\"2\" cy=\"2\" r=\"2\"/>"), "{svg}");
        let full = SvgShape::Arc { c: Vec2::new(0.0, 0.0), a: Vec2::new(3.0, 0.0), b: Vec2::new(3.0, 0.0) };
        assert!(write_svg(&[full], "mm", 1.0).unwrap().contains("<circle cx=\"3\" cy=\"3\" r=\"3\"/>"));
    }

    #[test]
    fn nothing_to_draw_and_a_drawing_with_no_size_are_refused() {
        assert!(write_svg(&[], "mm", 1.0).unwrap_err().to_string().contains("no visible curves"));
        let flat = SvgShape::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0));
        assert!(write_svg(&[flat], "mm", 1.0).unwrap_err().to_string().contains("no width or no height"));
        let bad = SvgShape::Line(Vec2::new(0.0, 0.0), Vec2::new(f64::NAN, 5.0));
        assert!(write_svg(&[bad], "mm", 1.0).is_err());
    }

    #[test]
    fn what_is_written_reads_back_at_the_same_size() {
        let outer = rect(30.0, 20.0);
        let svg = write_svg(&[SvgShape::Region { outer: &outer, holes: vec![] }, SvgShape::Circle(Vec2::new(10.0, 10.0), 4.0)], "mm", 1.0).unwrap();
        let geom = crate::read_svg(&svg).unwrap();
        assert!(!geom.is_empty());
        let (min, max) = crate::svg_sheet(&svg).unwrap();
        assert!((max.x - min.x - 30.0).abs() < 1e-6 && (max.y - min.y - 20.0).abs() < 1e-6, "{min:?} {max:?}");
    }
}
