//! SVG (W3C SVG 1.1) geometry for sketches: `path` (all commands), `line`, `rect`, `circle`,
//! `ellipse`, `polyline`, `polygon` and `text`, with `transform`s on elements and groups. User
//! units are CSS pixels (1/96 in) unless the root's `width`/`height` carry units with a
//! `viewBox`; y is flipped so the drawing reads upright in the sketch.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::sketch2d::{Geom2, MAX_DRAWING_BYTES, MAX_ENTITIES};
use crate::{IoError, Result};

mod diagnostics;

pub use diagnostics::{SvgDiagnostic, SvgDrawing};

const MAX_DEPTH: usize = 256;
const PX_MM: f64 = 25.4 / 96.0;

/// Affine map [a c e; b d f].
#[derive(Clone, Copy, Debug)]
struct Tf([f64; 6]);

impl Tf {
    const ID: Tf = Tf([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn mul(self, o: Tf) -> Tf {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        Tf([a * a2 + c * b2, b * a2 + d * b2, a * c2 + c * d2, b * c2 + d * d2, a * e2 + c * f2 + e, b * e2 + d * f2 + f])
    }
    fn apply(self, p: Vec2) -> Vec2 {
        let [a, b, c, d, e, f] = self.0;
        Vec2::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }
    fn vec(self, v: Vec2) -> Vec2 {
        let [a, b, c, d, ..] = self.0;
        Vec2::new(a * v.x + c * v.y, b * v.x + d * v.y)
    }
    /// Uniform scale (with any rotation or mirror) and its factor.
    fn uniform(self) -> Option<f64> {
        let (u, v) = (self.vec(Vec2::X), self.vec(Vec2::Y));
        ((u.len() - v.len()).abs() < 1e-9 * u.len().max(1e-12) && u.dot(v).abs() < 1e-9 * u.len2().max(1e-24)).then_some(u.len())
    }
    fn flips(self) -> bool {
        let [a, b, c, d, ..] = self.0;
        a * d - b * c < 0.0
    }
    fn is_finite(self) -> bool {
        self.0.iter().all(|x| x.is_finite())
    }
}

/// Only the paint information needed to identify unpainted source outlines. Unknown paint
/// values count as painted, so unsupported CSS never makes a visible path eligible by mistake.
#[derive(Clone, Copy)]
struct Paint {
    fill: bool,
    stroke: bool,
}

impl Paint {
    const DEFAULT: Self = Self { fill: true, stroke: false };

    fn inherited(self, attrs: &[(String, String)]) -> Self {
        let value = |v: &str, inherited: bool, initial: bool| match v.trim().to_ascii_lowercase().as_str() {
            "none" => false,
            "inherit" | "unset" => inherited,
            "initial" => initial,
            _ => true,
        };
        let mut paint = Self {
            fill: attr(attrs, "fill").map(|v| value(v, self.fill, true)).unwrap_or(self.fill),
            stroke: attr(attrs, "stroke").map(|v| value(v, self.stroke, false)).unwrap_or(self.stroke),
        };
        let (mut fill_important, mut stroke_important) = (false, false);
        if let Some(style) = attr(attrs, "style") {
            for (key, text) in style.split(';').filter_map(|d| d.split_once(':')) {
                let text = text.trim();
                let important = text.to_ascii_lowercase().ends_with("!important");
                let text = if important { text.get(..text.len().saturating_sub(10)).unwrap_or("").trim() } else { text };
                match key.trim().to_ascii_lowercase().as_str() {
                    "fill" if important || !fill_important => {
                        paint.fill = value(text, self.fill, true);
                        fill_important = important;
                    }
                    "stroke" if important || !stroke_important => {
                        paint.stroke = value(text, self.stroke, false);
                        stroke_important = important;
                    }
                    _ => {}
                }
            }
        }
        paint
    }
}

/// Every number of `g` is finite: a transform such as `scale(1e308)` can push a finite size in
/// the file past the range of `f64`, and an infinite curve must never reach a sketch.
fn geom_is_finite(g: &Geom2) -> bool {
    match g {
        Geom2::Point(p) => p.is_finite(),
        Geom2::Line(a, b) => a.is_finite() && b.is_finite(),
        Geom2::Circle(c, r) => c.is_finite() && r.is_finite(),
        Geom2::Arc { c, a, b } => c.is_finite() && a.is_finite() && b.is_finite(),
        Geom2::Ellipse { c, major, minor } => c.is_finite() && major.is_finite() && minor.is_finite(),
        Geom2::Spline { pts, .. } => pts.iter().all(|p| p.is_finite()),
        Geom2::Conic { a, apex, b, rho } => a.is_finite() && apex.is_finite() && b.is_finite() && rho.is_finite(),
        Geom2::Text { at, height, angle, .. } => at.is_finite() && height.is_finite() && angle.is_finite(),
    }
}

const OUT_OF_RANGE: &str = "SVG geometry is out of range: a transform or size scales a shape past the largest number SolidCraft can store";

fn numbers(s: &str) -> Vec<f64> {
    // Numbers separated by commas/space, also "1-2" and "1.5.5" forms.
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && out.len() < 1_000_000 {
        while i < b.len() && !(b[i] == b'-' || b[i] == b'+' || b[i] == b'.' || b[i].is_ascii_digit()) {
            i += 1;
        }
        let st = i;
        let mut seen_dot = false;
        let mut seen_e = false;
        if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
            i += 1;
        }
        while i < b.len() {
            let ch = b[i];
            if ch.is_ascii_digit() {
                i += 1;
            } else if ch == b'.' && !seen_dot && !seen_e {
                seen_dot = true;
                i += 1;
            } else if (ch == b'e' || ch == b'E') && !seen_e {
                seen_e = true;
                i += 1;
                if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if let Some(x) = s.get(st..i).and_then(|t| t.parse::<f64>().ok()).filter(|x| x.is_finite()) {
            out.push(x);
        } else if i == st {
            i += 1;
        }
    }
    out
}

fn parse_transform(s: &str) -> Tf {
    let mut t = Tf::ID;
    for part in s.split(')') {
        let Some((name, args)) = part.split_once('(') else { continue };
        let n = numbers(args);
        let g = |i: usize, d: f64| n.get(i).copied().unwrap_or(d);
        let m = match name.trim().trim_start_matches(',').trim() {
            "matrix" if n.len() >= 6 => Tf([n[0], n[1], n[2], n[3], n[4], n[5]]),
            "translate" => Tf([1.0, 0.0, 0.0, 1.0, g(0, 0.0), g(1, 0.0)]),
            "scale" => Tf([g(0, 1.0), 0.0, 0.0, g(1, g(0, 1.0)), 0.0, 0.0]),
            "rotate" => {
                let (s, c) = g(0, 0.0).to_radians().sin_cos();
                let r = Tf([c, s, -s, c, 0.0, 0.0]);
                let (cx, cy) = (g(1, 0.0), g(2, 0.0));
                Tf([1.0, 0.0, 0.0, 1.0, cx, cy]).mul(r).mul(Tf([1.0, 0.0, 0.0, 1.0, -cx, -cy]))
            }
            "skewX" => Tf([1.0, 0.0, g(0, 0.0).to_radians().tan(), 1.0, 0.0, 0.0]),
            "skewY" => Tf([1.0, g(0, 0.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
            _ => Tf::ID,
        };
        t = t.mul(m);
    }
    t
}

fn attrs(e: &BytesStart) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .map(|a| {
            let k = String::from_utf8_lossy(a.key.local_name().as_ref()).to_string();
            let v = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.to_string())
                .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
            (k, v)
        })
        .collect()
}

fn attr<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn num_attr(a: &[(String, String)], k: &str) -> f64 {
    attr(a, k).map(numbers).and_then(|v| v.first().copied()).unwrap_or(0.0)
}

/// Length with a unit, in millimetres (None without a unit or for %).
fn length_mm(s: &str) -> Option<f64> {
    let v = numbers(s).first().copied()?;
    let u = s.trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E');
    Some(match u.trim() {
        "mm" => v,
        "cm" => v * 10.0,
        "in" => v * 25.4,
        "pt" => v * 25.4 / 72.0,
        "pc" => v * 25.4 / 6.0,
        "px" | "" => v * PX_MM,
        _ => return None,
    })
}

struct Out {
    g: Vec<Geom2>,
    source_segments: Vec<usize>,
    source_segment: usize,
    unpainted: Vec<std::ops::Range<usize>>,
    closed: Vec<std::ops::Range<usize>>,
    spans: Vec<(usize, usize, std::ops::Range<usize>)>,
}

impl Out {
    fn push(&mut self, g: Geom2) -> Result<()> {
        if self.g.len() >= MAX_ENTITIES {
            return Err(IoError::Invalid("too many SVG elements".into()));
        }
        if !geom_is_finite(&g) {
            return Err(IoError::Invalid(OUT_OF_RANGE.into()));
        }
        self.g.push(g);
        self.source_segments.push(self.source_segment);
        Ok(())
    }
    fn line(&mut self, t: Tf, a: Vec2, b: Vec2) -> Result<()> {
        let (a, b) = (t.apply(a), t.apply(b));
        if !(a.is_finite() && b.is_finite()) {
            // Checked before the length: the distance between infinite ends is NaN, which would
            // drop the line silently.
            return Err(IoError::Invalid(OUT_OF_RANGE.into()));
        }
        if a.dist(b) > 1e-12 { self.push(Geom2::Line(a, b)) } else { Ok(()) }
    }
    fn cubic(&mut self, t: Tf, p: [Vec2; 4]) -> Result<()> {
        self.push(Geom2::Spline { pts: p.iter().map(|q| t.apply(*q)).collect(), control: true, degree: 3 })
    }
    fn quad(&mut self, t: Tf, a: Vec2, c: Vec2, b: Vec2) -> Result<()> {
        let (a, c, b) = (t.apply(a), t.apply(c), t.apply(b));
        if (b - a).cross(c - a).abs() < 1e-12 { self.push(Geom2::Line(a, b)) } else { self.push(Geom2::Conic { a, apex: c, b, rho: 0.5 }) }
    }
    /// Ellipse `c + rx·cos t·u + ry·sin t·v` (u at angle `phi`) from `t0` sweeping `dt`.
    fn elliptic(&mut self, t: Tf, c: Vec2, rx: f64, ry: f64, phi: f64, t0: f64, dt: f64) -> Result<()> {
        let u = Vec2::from_angle(phi);
        let v = u.perp();
        let at = |a: f64| c + u * (rx * a.cos()) + v * (ry * a.sin());
        let circle = t.uniform().filter(|_| (rx - ry).abs() < 1e-9 * rx.max(ry));
        if let Some(k) = circle {
            let (p0, p1) = (t.apply(at(t0)), t.apply(at(t0 + dt)));
            let cc = t.apply(c);
            let ccw = (dt > 0.0) != t.flips();
            if dt.abs() >= std::f64::consts::TAU - 1e-9 {
                return self.push(Geom2::Circle(cc, rx * k));
            }
            return self.push(if ccw { Geom2::Arc { c: cc, a: p0, b: p1 } } else { Geom2::Arc { c: cc, a: p1, b: p0 } });
        }
        if dt.abs() >= std::f64::consts::TAU - 1e-9 {
            // Full ellipse: centre, major end, minor radius after the transform (affine images
            // of ellipses are ellipses; sample to find the axes).
            let pts: Vec<Vec2> = (0..720).map(|i| t.apply(at(i as f64 / 720.0 * std::f64::consts::TAU))).collect();
            let cc = t.apply(c);
            let (mut far, mut near) = (cc, f64::INFINITY);
            for q in &pts {
                if q.dist(cc) > far.dist(cc) {
                    far = *q;
                }
                near = near.min(q.dist(cc));
            }
            return self.push(Geom2::Ellipse { c: cc, major: far, minor: near });
        }
        // Elliptic arc: cubic Béziers of at most 90 degrees each.
        let n = ((dt.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
        let h = dt / n as f64;
        let k = 4.0 / 3.0 * (h / 4.0).tan();
        let d = |a: f64| u * (-rx * a.sin()) + v * (ry * a.cos());
        for i in 0..n {
            let (a0, a1) = (t0 + h * i as f64, t0 + h * (i + 1) as f64);
            self.cubic(t, [at(a0), at(a0) + d(a0) * k, at(a1) - d(a1) * k, at(a1)])?;
        }
        Ok(())
    }
}

/// SVG endpoint arc → centre form (SVG 1.1 implementation notes F.6.5).
#[allow(clippy::too_many_arguments)]
fn arc_to(out: &mut Out, t: Tf, p0: Vec2, mut rx: f64, mut ry: f64, phi_deg: f64, large: bool, sweep: bool, p1: Vec2) -> Result<()> {
    if p0.dist(p1) < 1e-12 {
        return Ok(());
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx < 1e-12 || ry < 1e-12 {
        return out.line(t, p0, p1);
    }
    let phi = phi_deg.to_radians();
    let (s, c) = phi.sin_cos();
    let d = (p0 - p1) * 0.5;
    let x1 = Vec2::new(c * d.x + s * d.y, -s * d.x + c * d.y);
    let lam = (x1.x / rx).powi(2) + (x1.y / ry).powi(2);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * x1.y * x1.y - ry * ry * x1.x * x1.x).max(0.0);
    let den = rx * rx * x1.y * x1.y + ry * ry * x1.x * x1.x;
    let mut co = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    if large == sweep {
        co = -co;
    }
    let cx1 = Vec2::new(co * rx * x1.y / ry, -co * ry * x1.x / rx);
    let mid = (p0 + p1) * 0.5;
    let cc = Vec2::new(c * cx1.x - s * cx1.y, s * cx1.x + c * cx1.y) + mid;
    let ang = |u: Vec2, v: Vec2| u.cross(v).atan2(u.dot(v));
    let u = Vec2::new((x1.x - cx1.x) / rx, (x1.y - cx1.y) / ry);
    let v = Vec2::new((-x1.x - cx1.x) / rx, (-x1.y - cx1.y) / ry);
    let t0 = ang(Vec2::X, u);
    let mut dt = ang(u, v);
    if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }
    out.elliptic(t, cc, rx, ry, phi, t0, dt)
}

fn path(out: &mut Out, t: Tf, d: &str, path_index: usize, unpainted: bool) -> Result<()> {
    let mut span_start = out.g.len();
    let mut subpath = 0usize;
    out.source_segment = 0;
    // Tokenise into commands and their numbers.
    let mut cmds: Vec<(char, Vec<f64>)> = Vec::new();
    let mut cur: Option<(char, String)> = None;
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() && ch != 'e' && ch != 'E' {
            if let Some((c, s)) = cur.take() {
                cmds.push((c, numbers(&s)));
            }
            cur = Some((ch, String::new()));
        } else if let Some((_, s)) = cur.as_mut() {
            s.push(ch);
        }
    }
    if let Some((c, s)) = cur.take() {
        cmds.push((c, numbers(&s)));
    }
    let (mut p, mut start) = (Vec2::ZERO, Vec2::ZERO);
    let mut last_c: Option<Vec2> = None; // last cubic control (for S)
    let mut last_q: Option<Vec2> = None; // last quadratic control (for T)
    for (c, n) in cmds {
        let rel = c.is_ascii_lowercase();
        let base = |p: Vec2| if rel { p } else { Vec2::ZERO };
        let arity = match c.to_ascii_uppercase() {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            _ => 0,
        };
        if c.eq_ignore_ascii_case(&'Z') {
            out.source_segment += 1;
            out.line(t, p, start)?;
            p = start;
            last_c = None;
            last_q = None;
            continue;
        }
        if arity == 0 {
            continue;
        }
        for (k, a) in n.chunks(arity).enumerate() {
            if a.len() < arity {
                break;
            }
            if c.eq_ignore_ascii_case(&'M') && k == 0 {
                out.source_segment = 0;
            } else {
                out.source_segment += 1;
            }
            let o = base(p);
            let v = |i: usize| Vec2::new(a[i], a[i + 1]) + o;
            match c.to_ascii_uppercase() {
                'M' => {
                    let q = v(0);
                    if k == 0 {
                        if subpath > 0 {
                            out.spans.push((path_index, subpath, span_start..out.g.len()));
                            if unpainted {
                                out.unpainted.push(span_start..out.g.len());
                            }
                            if p == start {
                                out.closed.push(span_start..out.g.len());
                            }
                        }
                        subpath += 1;
                        span_start = out.g.len();
                        p = q;
                        start = q;
                    } else {
                        out.line(t, p, q)?;
                        p = q;
                    }
                    last_c = None;
                    last_q = None;
                }
                'L' => {
                    let q = v(0);
                    out.line(t, p, q)?;
                    p = q;
                    last_c = None;
                    last_q = None;
                }
                'H' => {
                    let q = Vec2::new(a[0] + if rel { p.x } else { 0.0 }, p.y);
                    out.line(t, p, q)?;
                    p = q;
                    last_c = None;
                    last_q = None;
                }
                'V' => {
                    let q = Vec2::new(p.x, a[0] + if rel { p.y } else { 0.0 });
                    out.line(t, p, q)?;
                    p = q;
                    last_c = None;
                    last_q = None;
                }
                'C' => {
                    let (c1, c2, q) = (v(0), v(2), v(4));
                    out.cubic(t, [p, c1, c2, q])?;
                    last_c = Some(c2);
                    last_q = None;
                    p = q;
                }
                'S' => {
                    let c1 = last_c.map(|lc| p * 2.0 - lc).unwrap_or(p);
                    let (c2, q) = (v(0), v(2));
                    out.cubic(t, [p, c1, c2, q])?;
                    last_c = Some(c2);
                    last_q = None;
                    p = q;
                }
                'Q' => {
                    let (c1, q) = (v(0), v(2));
                    out.quad(t, p, c1, q)?;
                    last_q = Some(c1);
                    last_c = None;
                    p = q;
                }
                'T' => {
                    let c1 = last_q.map(|lq| p * 2.0 - lq).unwrap_or(p);
                    let q = v(0);
                    out.quad(t, p, c1, q)?;
                    last_q = Some(c1);
                    last_c = None;
                    p = q;
                }
                'A' => {
                    let q = Vec2::new(a[5], a[6]) + o;
                    arc_to(out, t, p, a[0], a[1], a[2], a[3] != 0.0, a[4] != 0.0, q)?;
                    last_c = None;
                    last_q = None;
                    p = q;
                }
                _ => {}
            }
        }
    }
    if subpath > 0 {
        out.spans.push((path_index, subpath, span_start..out.g.len()));
        if unpainted {
            out.unpainted.push(span_start..out.g.len());
        }
        if p == start {
            out.closed.push(span_start..out.g.len());
        }
    }
    Ok(())
}

fn points_list(s: &str) -> Vec<Vec2> {
    numbers(s).chunks(2).filter(|c| c.len() == 2).map(|c| Vec2::new(c[0], c[1])).collect()
}

/// Whether an element and everything in it is left out of the drawing (#11039): the contents of
/// `defs`, `clipPath`, `mask`, `symbol`, `marker` and `pattern` are only drawn through a
/// reference, and `display: none` (as an attribute or in `style`) hides an element with its children.
fn not_drawn(name: &str, a: &[(String, String)]) -> bool {
    if matches!(name, "defs" | "clipPath" | "mask" | "symbol" | "marker" | "pattern") {
        return true;
    }
    let none = |v: &str| v.trim().trim_end_matches("!important").trim().eq_ignore_ascii_case("none");
    attr(a, "display").is_some_and(none)
        || attr(a, "style")
            .is_some_and(|s| s.split(';').filter_map(|d| d.split_once(':')).any(|(k, v)| k.trim().eq_ignore_ascii_case("display") && none(v)))
}

/// Read the geometry of an SVG document (millimetres, y up). `scale` multiplies the result.
pub fn read_svg(text: &str) -> Result<Vec<Geom2>> {
    Ok(read_svg_internal(text, None, false)?.geometry)
}

/// Read source-addressed diagnostics, optionally approximating closed path cubics and repairing
/// suspect open curves within a millimetre bound.
/// A missing tolerance preserves all source geometry. Repairs never exceed the requested bound.
pub fn read_svg_diagnosed(text: &str, repair_tolerance: Option<f64>) -> Result<SvgDrawing> {
    if repair_tolerance.is_some_and(|t| !t.is_finite() || t <= 0.0) {
        return Err(IoError::Invalid("SVG repair tolerance must be finite and positive".into()));
    }
    read_svg_internal(text, repair_tolerance, true)
}

fn read_svg_internal(text: &str, repair_tolerance: Option<f64>, diagnose: bool) -> Result<SvgDrawing> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the SVG file is too large".into()));
    }
    let mut rd = Reader::from_reader(text.as_bytes());
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<Tf> = Vec::new();
    let mut paints: Vec<Paint> = Vec::new();
    let mut paint_unknown = false;
    let mut out = Out { g: Vec::new(), source_segments: Vec::new(), source_segment: 0, unpainted: Vec::new(), closed: Vec::new(), spans: Vec::new() };
    let mut path_index = 0usize;
    let mut root: Option<Tf> = None;
    let mut pending_text: Option<(Tf, Vec2, f64)> = None;
    // The depth of the element whose contents are not drawn (#11039), while inside one.
    let mut skip_from: Option<usize> = None;
    loop {
        let ev = rd.read_event_into(&mut buf).map_err(|e| IoError::Invalid(format!("SVG: {e}")))?;
        let (e, empty) = match &ev {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            Event::End(_) => {
                stack.pop();
                paints.pop();
                if skip_from.is_some_and(|d| stack.len() <= d) {
                    skip_from = None;
                }
                buf.clear();
                continue;
            }
            Event::Text(tx) => {
                if let Some((tf, at, h)) = pending_text.take() {
                    let s = String::from_utf8_lossy(tx.as_ref()).trim().to_string();
                    if !s.is_empty() {
                        let k = tf.uniform().unwrap_or(1.0);
                        let dir = tf.vec(Vec2::X);
                        out.push(Geom2::Text { text: s, at: tf.apply(at), height: h * 0.7 * k, angle: dir.angle() })?;
                    }
                }
                buf.clear();
                continue;
            }
            Event::PI(pi) => {
                if String::from_utf8_lossy(pi.as_ref()).trim_start().to_ascii_lowercase().starts_with("xml-stylesheet") {
                    paint_unknown = true;
                }
                (None, false)
            }
            Event::Eof => break,
            _ => (None, false),
        };
        let Some(e) = e else {
            buf.clear();
            continue;
        };
        let a = attrs(&e);
        let parent = stack.last().copied().or(root).unwrap_or(Tf::ID);
        let paint = paints.last().copied().unwrap_or(Paint::DEFAULT).inherited(&a);
        let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
        if name == "style"
            || attr(&a, "class").is_some()
            || attr(&a, "style").is_some_and(|s| {
                s.contains("/*")
                    || s.contains('\\')
                    || s.split(';').filter_map(|d| d.split_once(':')).any(|(key, value)| {
                        key.trim().eq_ignore_ascii_case("all")
                            || (value.contains('!') && !value.trim_end().to_ascii_lowercase().ends_with("!important"))
                    })
            })
        {
            paint_unknown = true;
        }
        if name == "path" {
            path_index += 1;
        }
        if skip_from.is_some() || not_drawn(&name, &a) {
            // Keep the stack in step with the end tags; draw nothing until this element closes.
            if !empty {
                skip_from = skip_from.or(Some(stack.len()));
                stack.push(parent);
                paints.push(paint);
                if stack.len() > MAX_DEPTH {
                    return Err(IoError::Invalid("SVG nested too deeply".into()));
                }
            }
            buf.clear();
            continue;
        }
        let mut tf = match attr(&a, "transform") {
            Some(s) => parent.mul(parse_transform(s)),
            None => parent,
        };
        if name == "svg" && root.is_none() {
            // Root: user units to millimetres, y flipped.
            let vb = attr(&a, "viewBox").map(numbers).filter(|v| v.len() == 4);
            let (sx, sy, ox, oy) = match (vb, attr(&a, "width").and_then(length_mm), attr(&a, "height").and_then(length_mm)) {
                (Some(v), Some(w), Some(h)) if v[2] > 0.0 && v[3] > 0.0 => (w / v[2], h / v[3], v[0], v[1]),
                (Some(v), _, _) => (PX_MM, PX_MM, v[0], v[1]),
                _ => (PX_MM, PX_MM, 0.0, 0.0),
            };
            let r = Tf([sx, 0.0, 0.0, -sy, -ox * sx, oy * sy]);
            root = Some(r);
            tf = r;
        }
        if !tf.is_finite() {
            return Err(IoError::Invalid(OUT_OF_RANGE.into()));
        }
        match name.as_str() {
            "path" => {
                path(&mut out, tf, attr(&a, "d").unwrap_or(""), path_index, !paint.fill && !paint.stroke)?;
            }
            "line" => out.line(tf, Vec2::new(num_attr(&a, "x1"), num_attr(&a, "y1")), Vec2::new(num_attr(&a, "x2"), num_attr(&a, "y2")))?,
            "rect" => {
                let (x, y, w, h) = (num_attr(&a, "x"), num_attr(&a, "y"), num_attr(&a, "width"), num_attr(&a, "height"));
                if w > 0.0 && h > 0.0 {
                    let c = [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)];
                    for i in 0..4 {
                        out.line(tf, c[i], c[(i + 1) % 4])?;
                    }
                }
            }
            "circle" => {
                let r = num_attr(&a, "r");
                if r > 0.0 {
                    out.elliptic(tf, Vec2::new(num_attr(&a, "cx"), num_attr(&a, "cy")), r, r, 0.0, 0.0, std::f64::consts::TAU)?;
                }
            }
            "ellipse" => {
                let (rx, ry) = (num_attr(&a, "rx"), num_attr(&a, "ry"));
                if rx > 0.0 && ry > 0.0 {
                    out.elliptic(tf, Vec2::new(num_attr(&a, "cx"), num_attr(&a, "cy")), rx, ry, 0.0, 0.0, std::f64::consts::TAU)?;
                }
            }
            "polyline" | "polygon" => {
                let pts = points_list(attr(&a, "points").unwrap_or(""));
                for w in pts.windows(2) {
                    out.line(tf, w[0], w[1])?;
                }
                if name == "polygon"
                    && let (Some(f), Some(l)) = (pts.first(), pts.last())
                {
                    out.line(tf, *l, *f)?;
                }
            }
            "text" => {
                let size = attr(&a, "font-size").map(numbers).and_then(|v| v.first().copied()).unwrap_or(16.0);
                pending_text = Some((tf, Vec2::new(num_attr(&a, "x"), num_attr(&a, "y")), size));
            }
            _ => {}
        }
        if !empty {
            stack.push(tf);
            paints.push(paint);
            if stack.len() > MAX_DEPTH {
                return Err(IoError::Invalid("SVG nested too deeply".into()));
            }
        }
        buf.clear();
    }
    if diagnose {
        diagnostics::process(
            out.g,
            out.spans,
            out.source_segments,
            if paint_unknown { Vec::new() } else { out.unpainted },
            out.closed,
            repair_tolerance,
        )
    } else {
        Ok(SvgDrawing { geometry: out.g, diagnostics: Vec::new() })
    }
}

/// The text of an SVG file's bytes: a UTF-8 byte-order mark is dropped and UTF-16 files (with
/// their mark, as some Windows tools write them) are decoded. A gzip-compressed file (`.svgz`
/// under any name) is refused with a clear error instead of being read as text (#9641).
pub fn decode_svg(bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the SVG file is too large".into()));
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(IoError::Invalid(
            "this SVG is gzip-compressed (an .svgz file); SolidCraft reads plain SVG only: save it as plain (uncompressed) SVG in a vector editor first"
                .into(),
        ));
    }
    let utf16 = |rest: &[u8], le: bool| -> String {
        let units = rest.as_chunks::<2>().0.iter().map(|&pair| if le { u16::from_le_bytes(pair) } else { u16::from_be_bytes(pair) });
        char::decode_utf16(units).map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
    };
    Ok(match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    })
}

/// Why [`read_svg`] found nothing to import in `text`, in words for the person inserting it
/// (#9641): a picture embedded in an SVG, shapes drawn only from another file, or a file that is
/// not an SVG drawing at all, rather than a bare "no geometry".
pub fn svg_nothing_to_import(text: &str) -> String {
    let mut rd = Reader::from_reader(text.as_bytes());
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let (mut svg, mut images, mut external_uses, mut elements) = (false, 0usize, 0usize, 0usize);
    let mut broken = None;
    while elements < MAX_ENTITIES {
        match rd.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                elements += 1;
                match e.local_name().as_ref() {
                    b"svg" => svg = true,
                    b"image" => images += 1,
                    b"use" => {
                        let a = attrs(&e);
                        if attr(&a, "href").is_some_and(|h| !h.trim_start().starts_with('#')) {
                            external_uses += 1;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                broken = Some(e.to_string());
                break;
            }
        }
        buf.clear();
    }
    if !svg {
        return match broken {
            Some(e) => format!("the file is not a readable SVG drawing ({e})"),
            None => "the file is not an SVG drawing: it has no <svg> element (it may be a picture or a web page saved with an .svg name)".into(),
        };
    }
    if images > 0 {
        return format!(
            "the SVG holds only {} embedded picture{} (<image>), not outlines, so there is nothing to draw in the sketch; trace the picture into paths in a vector editor, or save it as PNG/JPEG and use Insert > Canvas, then Canvas to Sketch",
            if images == 1 { "an" } else { "several" },
            if images == 1 { "" } else { "s" }
        );
    }
    if external_uses > 0 {
        return "the SVG's shapes are drawn with <use> from another file, which SolidCraft does not follow; save the drawing as a single self-contained SVG first".into();
    }
    "the SVG has no shapes SolidCraft reads (path, line, rect, circle, ellipse, polyline, polygon or text)".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_diagnostics_preserve_geometry_and_address_compound_paths() {
        let text = r#"<svg width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0L10 0L10 10L0 0C-.01 -.01 -.02 -.01 -.02 0Z M12 0C11.9 0 14 2 15 3L12 0Z"/></svg>"#;
        let original = read_svg(text).unwrap();
        let drawing = read_svg_diagnosed(text, None).unwrap();
        assert_eq!(original, drawing.geometry);
        assert!(drawing.diagnostics.iter().any(|d| d.path == 1 && d.subpath == 1 && d.segment == 4 && d.kind == "attached_loop" && !d.repaired));
        assert!(drawing.diagnostics.iter().any(|d| d.subpath == 2 && d.segment == 1 && d.kind == "backtracking_curve"));
        let repaired = read_svg_diagnosed(text, Some(0.05)).unwrap();
        assert!(repaired.diagnostics.iter().any(|d| d.kind == "attached_loop" && d.repaired));
        assert!(repaired.diagnostics.iter().all(|d| d.max_deviation_mm.is_none_or(|e| e <= 0.05)));
        assert!(repaired.geometry.iter().all(|g| !matches!(g, Geom2::Spline { .. })));
        assert!(matches!(repaired.geometry.first(), Some(Geom2::Line(a,b)) if *a == Vec2::ZERO && *b == Vec2::new(10.0,0.0)));
    }

    #[test]
    fn svg_repair_respects_transform_units_and_refuses_large_attached_loop() {
        let text =
            r#"<svg width="20mm" height="20mm" viewBox="0 0 20 20"><g transform="scale(2)"><path d="M0 0L5 0L5 5L0 0C1 0 1 1 0 1Z"/></g></svg>"#;
        let drawing = read_svg_diagnosed(text, Some(0.01)).unwrap();
        assert!(drawing.diagnostics.iter().any(|d| d.kind == "attached_loop" && !d.repaired));
        assert!(drawing.geometry.iter().any(|g| matches!(g, Geom2::Line(a,b) if *a == Vec2::ZERO && *b == Vec2::new(10.0,0.0))));
        for t in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            assert!(read_svg_diagnosed(text, Some(t)).is_err());
        }
        assert!(read_svg_diagnosed(text, Some(1e-300)).is_ok());
    }

    #[test]
    fn svg_repair_keeps_unrelated_open_cubics_exact() {
        let text = r#"<svg width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 0C1 0 2 1 3 1 M5 0C4.9 -.1 6 1 7 2L5 0Z"/></svg>"#;
        let original = read_svg(text).unwrap();
        let repaired = read_svg_diagnosed(text, Some(0.01)).unwrap();
        assert_eq!(original.first(), repaired.geometry.first());
        assert_eq!(repaired.diagnostics.len(), 1);
        assert!(repaired.diagnostics[0].repaired);
        assert_eq!(repaired.diagnostics[0].subpath, 2);
        let monotone = r#"<svg><path d="M0 0C10 0 9 0 19 0"/></svg>"#;
        assert!(read_svg_diagnosed(monotone, None).unwrap().diagnostics.is_empty());
    }

    #[test]
    fn svg_repair_bounds_actual_curve_distance_without_changing_endpoints() {
        let text = r#"<svg width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M5 5C-10 -5 30 15 15 15"/></svg>"#;
        let drawing = read_svg_diagnosed(text, Some(0.01)).unwrap();
        let diagnostic = drawing.diagnostics.first().unwrap();
        assert!(diagnostic.repaired);
        assert_eq!(diagnostic.highlight.len(), 17);
        assert_eq!(diagnostic.highlight.first(), Some(&Vec2::new(5.0, -5.0)));
        assert_eq!(diagnostic.highlight.last(), Some(&Vec2::new(15.0, -15.0)));
        let points = [Vec2::new(5.0, -5.0), Vec2::new(-10.0, 5.0), Vec2::new(30.0, -15.0), Vec2::new(15.0, -15.0)];
        for i in 0..=10_000 {
            let t = f64::from(i) / 10_000.0;
            let u = 1.0 - t;
            let p = points[0] * (u * u * u) + points[1] * (3.0 * u * u * t) + points[2] * (3.0 * u * t * t) + points[3] * (t * t * t);
            let distance = drawing
                .geometry
                .iter()
                .filter_map(|g| {
                    if let Geom2::Line(a, b) = g {
                        let d = *b - *a;
                        let t = ((p - *a).dot(d) / d.len2()).clamp(0.0, 1.0);
                        Some(p.dist(*a + d * t))
                    } else {
                        None
                    }
                })
                .fold(f64::INFINITY, f64::min);
            assert!(distance <= 0.01 + 1e-12, "distance {distance}");
        }
    }

    #[test]
    fn svg_diagnostics_count_source_segments_despite_omitted_lines_and_split_arcs() {
        let text = r#"<svg width="30mm" height="30mm" viewBox="0 0 30 30"><path d="M0 0L0 0L0 0A10 10 0 0 1 20 0C19 -.1 22 .1 23 0"/></svg>"#;
        let drawing = read_svg_diagnosed(text, None).unwrap();
        let issue = drawing.diagnostics.iter().find(|d| d.kind == "backtracking_curve").unwrap();
        assert_eq!(issue.path, 1);
        assert_eq!(issue.subpath, 1);
        assert_eq!(issue.segment, 4);
        let text = r#"<svg><path d="M0 0L0 0Z C-1 -1 1 1 2 2"/></svg>"#;
        let drawing = read_svg_diagnosed(text, None).unwrap();
        assert_eq!(drawing.diagnostics[0].segment, 3);
    }

    #[test]
    fn svg_repair_never_claims_an_overflowed_or_underflowed_distance_bound() {
        let huge = r#"<svg><path d="M1e308 1e308C1e308 1e308 -1e308 -1e308 -1e308 -1e308"/></svg>"#;
        let drawing = read_svg_diagnosed(huge, Some(0.01)).unwrap();
        assert!(!drawing.diagnostics.is_empty());
        assert!(drawing.diagnostics.iter().all(|d| !d.repaired && d.max_deviation_mm.is_none()));
        let tiny = r#"<svg><path d="M0 0C0 0 1e-200 1e-200 0 1e-200"/></svg>"#;
        let drawing = read_svg_diagnosed(tiny, Some(1e-300)).unwrap();
        assert!(!drawing.diagnostics.is_empty());
        assert!(drawing.diagnostics.iter().all(|d| !d.repaired));
    }

    #[test]
    fn svg_repair_reserves_rounding_error_and_preserves_isolated_closed_cubics() {
        // The tiny bow is larger than the requested tolerance, although subdivision at this
        // large absolute coordinate can round its controls onto a straight line.
        let text = r#"<svg width="1mm" height="1mm" viewBox="0 0 1 1"><path d="M100000000 100000000C100000000 100000000 100000000.00000001 100000000.00000001 100000000.00000001 100000000"/></svg>"#;
        let original = read_svg(text).unwrap();
        let drawing = read_svg_diagnosed(text, Some(1e-9)).unwrap();
        assert_eq!(drawing.geometry, original);
        assert!(!drawing.diagnostics.is_empty());
        assert!(drawing.diagnostics.iter().all(|d| !d.repaired && d.max_deviation_mm.is_none()));

        // Removing this entire loop would leave an empty drawing, which has no finite
        // Hausdorff distance to the original curve. Keep it unresolved instead.
        let text = r#"<svg width="1mm" height="1mm" viewBox="0 0 1 1"><path d="M0 0C.01 0 .01 .01 0 0Z"/></svg>"#;
        let original = read_svg(text).unwrap();
        let drawing = read_svg_diagnosed(text, Some(0.1)).unwrap();
        assert_eq!(drawing.geometry, original);
        assert!(!drawing.geometry.is_empty());
        assert!(drawing.diagnostics.iter().any(|d| d.kind == "closed_cubic" && !d.repaired && d.max_deviation_mm.is_none()));
    }

    #[test]
    fn svg_repair_normalizes_only_closed_boundaries_sharing_exact_cubics() {
        // A source boundary is shared exactly at the first cubic, then follows the same
        // carrier with differently split and rounded cubic controls. The arrangement must
        // see consistent analytic pieces, while unrelated artwork stays exact.
        let text = r#"<svg width="30mm" height="20mm" viewBox="-5 -5 30 20">
            <path d="M0 0L20 0L20 10L0 10C-.5 9.25 -.875 8.4375 -1.125 7.59375C-1.875 5.0625 -1.5 2.25 0 0Z"/>
            <path d="M0 0L10 0L10 10L0 10C-.5 9.25 -.875 8.4375 -1.125 7.59375C-1.2450 7.1888 -1.3362 6.7766 -1.3986 6.3606C-1.7262 4.1769 -1.2600 1.8900 0 0Z"/>
            <path d="M25 0C26 0 27 1 28 1"/>
        </svg>"#;
        let original = read_svg(text).unwrap();
        let diagnosed = read_svg_diagnosed(text, None).unwrap();
        assert_eq!(diagnosed.geometry, original);
        assert!(diagnosed.diagnostics.iter().any(|d| d.kind == "shared_boundary" && !d.repaired));
        let repaired = read_svg_diagnosed(text, Some(0.05)).unwrap();
        assert_eq!(repaired.geometry.last(), original.last());
        assert_eq!(repaired.geometry.iter().filter(|g| matches!(g, Geom2::Spline { .. })).count(), 1);
        assert_eq!(repaired.diagnostics.len(), 5);
        assert!(repaired.diagnostics.iter().all(|d| d.kind == "shared_boundary"
            && d.repaired
            && d.max_deviation_mm.is_some_and(|b| b <= 0.05)
            && !d.replacement_highlight.is_empty()));
    }

    #[test]
    fn svg_shared_boundary_normalization_also_recognizes_exact_lines() {
        let text = r#"<svg width="30mm" height="20mm" viewBox="-5 -5 30 20"><path d="M0 0L10 0L20 0L20 10L10 10L0 10C-2 7 -2 3 0 0Z"/><path d="M0 0L10 0L10 10L0 10C-.7400 8.8900 -1.2062 7.6431 -1.3986 6.3606C-1.7262 4.1769 -1.2600 1.8900 0 0Z"/></svg>"#;
        let drawing = read_svg_diagnosed(text, Some(0.05)).unwrap();
        assert_eq!(drawing.diagnostics.len(), 3);
        assert!(drawing.diagnostics.iter().all(|d| d.kind == "shared_boundary" && d.repaired));
        assert!(drawing.geometry.iter().all(|g| matches!(g, Geom2::Line(..))));
    }

    #[test]
    fn svg_repair_normalizes_known_unpainted_closed_outlines_within_the_bound() {
        let text = r#"<svg width="10mm" height="10mm" viewBox="0 0 10 10"><g fill="none"><g stroke="none"><path d="M0 0C2 0 4 2 6 2L6 6L0 6Z"/></g></g></svg>"#;
        let original = read_svg(text).unwrap();
        let diagnosed = read_svg_diagnosed(text, None).unwrap();
        assert_eq!(diagnosed.geometry, original);
        assert!(diagnosed.diagnostics.iter().any(|d| d.kind == "unpainted_boundary" && !d.repaired));
        let repaired = read_svg_diagnosed(text, Some(0.01)).unwrap();
        assert!(repaired.geometry.iter().all(|g| matches!(g, Geom2::Line(..))));
        assert!(repaired.diagnostics.iter().all(|d| d.kind == "unpainted_boundary" && d.repaired && d.max_deviation_mm.is_some_and(|b| b <= 0.01)));
        // Inline style wins over the presentation attribute.
        let text = text.replace("stroke=\"none\"", "stroke=\"red\" style=\"stroke:none\"");
        assert!(read_svg_diagnosed(&text, Some(0.01)).unwrap().diagnostics.iter().any(|d| d.kind == "unpainted_boundary" && d.repaired));
    }

    #[test]
    fn svg_unpainted_outline_normalization_is_conservative_about_paint_and_css() {
        let path = r#"<path fill="none" d="M0 0C2 0 4 2 6 2L6 6L0 6Z"/>"#;
        let cases = [
            format!("<svg stroke=\"red\">{path}</svg>"),
            format!("<svg><g stroke=\"red\">{path}</g></svg>"),
            format!("<svg class=\"art\">{path}</svg>"),
            format!("<svg>{path}<style>path{{stroke:red}}</style></svg>"),
            format!("<?xml-stylesheet href=\"paint.css\" type=\"text/css\"?><svg>{path}</svg>"),
            format!("<svg>{}</svg>", path.replace("fill=\"none\"", "fill=\"none\" class=\"art\"")),
            format!("<svg>{}</svg>", path.replace("fill=\"none\"", "fill=\"none\" style=\"stroke:red!important;stroke:none\"")),
            format!("<svg>{}</svg>", path.replace("fill=\"none\"", "fill=\"none\" style=\"all:initial\"")),
            format!("<svg>{}</svg>", path.replace("fill=\"none\"", "fill=\"none\" style=\"stroke/**/:red\"")),
            format!("<svg>{}</svg>", path.replace("fill=\"none\"", "fill=\"none\" style=\"str\\6fke:red\"")),
            format!("<svg>{}</svg>", path.replace("fill=\"none\"", "fill=\"none\" style=\"stroke:red ! important;stroke:none\"")),
            format!("<svg>{}</svg>", path.replace("L6 6L0 6Z", "")),
        ];
        for text in cases {
            let original = read_svg(&text).unwrap();
            let diagnosed = read_svg_diagnosed(&text, None).unwrap();
            assert_eq!(diagnosed.geometry, original, "{text}");
            let drawing = read_svg_diagnosed(&text, Some(0.01)).unwrap();
            // Generic closed-outline approximation is paint-agnostic; unsupported CSS must
            // still never be classified as a known unpainted outline.
            assert!(drawing.diagnostics.iter().all(|d| d.kind != "unpainted_boundary"), "{text}");
            assert!(drawing.diagnostics.iter().all(|d| !d.repaired || d.max_deviation_mm.is_some_and(|b| b <= 0.01)), "{text}");
        }
    }

    #[test]
    fn svg_repair_approximates_closed_cubics_and_preserves_open_and_analytic_curves() {
        let text = r#"<svg width="40mm" height="20mm" viewBox="0 0 40 20">
            <path d="M0 0C2 0 4 2 6 2L6 6L0 6Z"/>
            <path d="M20 0C21 0 22 1 23 1"/>
            <circle cx="30" cy="5" r="1"/>
            <path d="M2 0A2 2 0 0 0 0 2C-1 2 -2 1 -2 0L2 0Z"/>
        </svg>"#;
        let original = read_svg(text).unwrap();
        let diagnosed = read_svg_diagnosed(text, None).unwrap();
        assert_eq!(diagnosed.geometry, original);
        assert!(diagnosed.diagnostics.is_empty(), "ordinary curves are not source defects");
        let repaired = read_svg_diagnosed(text, Some(0.01)).unwrap();
        assert_eq!(repaired.diagnostics.len(), 2);
        assert!(
            repaired.diagnostics.iter().all(|d| d.kind == "outline_approximation" && d.repaired && d.max_deviation_mm.is_some_and(|b| b <= 0.01))
        );
        assert_eq!(repaired.geometry.iter().filter(|g| matches!(g, Geom2::Spline { .. })).count(), 1, "the unrelated open cubic stays exact");
        for g in original.iter().filter(|g| matches!(g, Geom2::Circle(..) | Geom2::Arc { .. })) {
            assert!(repaired.geometry.contains(g), "analytic curves stay exact");
        }
        let open = original.iter().find(|g| matches!(g, Geom2::Spline { pts, .. } if pts.first() == Some(&Vec2::new(20.0, 0.0)))).unwrap();
        assert!(repaired.geometry.contains(open));
    }

    /// The parser recurses over groups and reads numbers out of a document the user did not write,
    /// so none of these may panic: `catch_unwind` turns one into a failed test, the way
    /// `hostile_files_never_panic` does for STEP (solidcraft#469).
    #[test]
    fn svg_repair_uses_the_fewest_equal_chords_within_the_bound() {
        // Midpoint bisection would need 64 chords for each of the two halves at 0.01.
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path d="M0 10 C0 -3.333 20 -3.333 20 10 C20 23.333 0 23.333 0 10 Z"/></svg>"#;
        let repaired = read_svg_diagnosed(text, Some(0.01)).unwrap();
        assert!(repaired.diagnostics.iter().all(|d| d.repaired && d.max_deviation_mm.is_some_and(|b| b <= 0.01)));
        assert!(repaired.geometry.iter().all(|g| matches!(g, Geom2::Line(..))));
        assert!(repaired.geometry.len() <= 80, "{} chords", repaired.geometry.len());
        let ends = |g: &Geom2| if let Geom2::Line(a, b) = g { (*a, *b) } else { (Vec2::ZERO, Vec2::ZERO) };
        for pair in repaired.geometry.windows(2) {
            assert_eq!(ends(&pair[0]).1, ends(&pair[1]).0, "chords stay connected end to end");
        }
    }

    #[test]
    fn hostile_svg_never_panics() {
        let good = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
            <g transform="translate(10 5)"><rect x="1" y="2" width="3" height="4"/><path d="M0 0 L5 5 Z"/></g>
        </svg>"#;
        let mut cases: Vec<String> = vec![
            String::new(),
            "<svg".into(),
            "<svg><".into(),
            "<svg><path d=".into(),
            "<svg><path d='M 1 1 L'/></svg>".into(),
            "<svg><path d='M nan nan L inf inf'/></svg>".into(),
            "<svg><path d='M 1e400 1e400 L 1 1'/></svg>".into(),
            "<svg><path d='M1e308 1e308 C-1e308 1e308 1e308 -1e308 -1e308 -1e308 Z'/></svg>".into(),
            "<svg><rect width='-1' height='-1'/></svg>".into(),
            "<svg><use href='#a'/><g id='a'><use href='#a'/></g></svg>".into(),
            format!("<svg>{}{}</svg>", "<g>".repeat(300), "</g>".repeat(300)),
            format!("<svg>{}</svg>", "<g>".repeat(300)),
            "<svg><![CDATA[<g>]]></svg>".into(),
            "<?xml version='1.0'?><!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><svg>&x;</svg>".into(),
            format!("<svg><path d='{}'/></svg>", "L 1 1 ".repeat(20_000)),
        ];
        for cut in (0..good.len()).step_by(13) {
            cases.push(good.get(..cut).unwrap_or("").to_string());
        }
        for (from, to) in [("10mm", "-10mm"), ("0 0 100 50", "0 0 0 0"), ("<g ", "<x "), ("rect", "rectx")] {
            cases.push(good.replacen(from, to, 3));
        }
        for c in &cases {
            let r = std::panic::catch_unwind(|| read_svg(c));
            assert!(r.is_ok(), "panicked on {:?}", c.get(..c.len().min(200)));
            let r = std::panic::catch_unwind(|| read_svg_diagnosed(c, Some(0.01)));
            assert!(r.is_ok(), "diagnosis or repair panicked on {:?}", c.get(..c.len().min(200)));
        }
    }

    #[test]
    fn shapes_paths_and_units() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
            <rect x="10" y="10" width="20" height="10"/>
            <circle cx="50" cy="25" r="5"/>
            <g transform="translate(70 0)"><path d="M0 0 C 10 0 10 10 20 10 Q 25 20 30 10 A 5 5 0 0 1 40 10 Z"/></g>
            <ellipse cx="80" cy="40" rx="6" ry="3"/>
        </svg>"#;
        let g = read_svg(svg).unwrap();
        // rect 4 lines, circle, cubic, quad, arc, closing line, ellipse
        assert_eq!(g.len(), 4 + 1 + 4 + 1, "{g:?}");
        // y flipped: the circle centre is at (50, -25) in mm.
        assert!(g.iter().any(|x| matches!(x, Geom2::Circle(c, r) if c.dist(Vec2::new(50.0, -25.0)) < 1e-9 && (r - 5.0).abs() < 1e-9)));
        assert!(g.iter().any(|x| matches!(x, Geom2::Spline { control: true, .. })));
        assert!(g.iter().any(|x| matches!(x, Geom2::Conic { .. })));
        assert!(g.iter().any(|x| matches!(x, Geom2::Arc { .. })));
        assert!(g.iter().any(|x| matches!(x, Geom2::Ellipse { minor, .. } if (minor - 3.0).abs() < 1e-3)));
        assert!(read_svg("<svg><path d='M 1e400 0 L nan 3'/></svg>").is_ok());
        assert!(read_svg("<svg").is_err() || read_svg("<svg").is_ok());
        // Plain pixels.
        let g = read_svg(r#"<svg><line x1="0" y1="0" x2="96" y2="0"/></svg>"#).unwrap();
        assert!(matches!(g[0], Geom2::Line(a, b) if (a.dist(b) - 25.4).abs() < 1e-9));
    }

    /// #9641: a file with nothing to import says why (an embedded picture, shapes from another
    /// file, not an SVG at all), and SVG files saved as UTF-16 or with a byte-order mark read.
    #[test]
    fn a_drawing_with_nothing_to_import_says_why() {
        let picture = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="64" height="64">
            <defs><pattern id="p"><use xlink:href="#i"/></pattern><image id="i" width="64" height="64" xlink:href="data:image/png;base64,iVBORw0KGgo="/></defs>
            <rect width="0" height="0" fill="url(#p)"/></svg>"##;
        assert!(read_svg(picture).unwrap().is_empty());
        let why = svg_nothing_to_import(picture);
        assert!(why.contains("embedded picture") && why.contains("Canvas"), "{why}");
        let external = r#"<svg xmlns="http://www.w3.org/2000/svg"><use href="shapes.svg#logo"/></svg>"#;
        assert!(svg_nothing_to_import(external).contains("another file"), "{}", svg_nothing_to_import(external));
        assert!(svg_nothing_to_import("<html><body>Not found</body></html>").contains("not an SVG drawing"));
        assert!(svg_nothing_to_import("<svg><g/></svg>").contains("no shapes"));

        let square = r#"<?xml version="1.0" encoding="UTF-16"?><svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#;
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(square.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(square.encode_utf16().flat_map(u16::to_be_bytes)).collect();
        let bom: Vec<u8> = [0xEF, 0xBB, 0xBF].into_iter().chain(square.bytes()).collect();
        for bytes in [le, be, bom, square.as_bytes().to_vec()] {
            let text = decode_svg(&bytes).unwrap();
            assert_eq!(read_svg(&text).unwrap().len(), 4, "{bytes:?}");
        }
        let gz = decode_svg(&[0x1f, 0x8b, 8, 0, 0, 0]).unwrap_err().to_string();
        assert!(gz.contains("gzip"), "{gz}");
        for bytes in [&[0xFF, 0xFE, 0x3C][..], &[0xFE, 0xFF, 0xD8, 0x00][..], &[][..]] {
            assert!(decode_svg(bytes).is_ok());
        }
    }

    /// #11039: shapes a file only defines (`defs`, `clipPath`, `mask`, `symbol`, `marker`,
    /// `pattern`) or hides (`display: none`) are not drawn by a browser, so they are not imported.
    #[test]
    fn shapes_a_file_only_defines_or_hides_are_not_imported() {
        let doc = |body: &str| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><rect x="5" y="5" width="30" height="30"/>{body}</svg>"#
            )
        };
        for body in [
            "",
            r#"<defs><clipPath id="c"><circle cx="20" cy="20" r="10"/></clipPath></defs>"#,
            r#"<defs><mask id="m"><circle cx="20" cy="20" r="10"/></mask></defs>"#,
            r#"<defs><symbol id="s"><circle cx="20" cy="20" r="10"/></symbol></defs>"#,
            r#"<defs><circle id="d" cx="20" cy="20" r="10"/><text x="1" y="1">A</text></defs>"#,
            r#"<clipPath id="c"><g><circle cx="20" cy="20" r="10"/></g></clipPath>"#,
            r#"<marker id="k"><path d="M0 0 L5 5"/></marker><pattern id="p"><line x1="0" y1="0" x2="5" y2="5"/></pattern>"#,
            r#"<circle cx="20" cy="20" r="10" style="display:none"/>"#,
            r#"<circle cx="20" cy="20" r="10" style="fill: red; display : none !important"/>"#,
            r#"<g display="none"><circle cx="20" cy="20" r="10"/><g><line x1="0" y1="0" x2="5" y2="5"/></g></g>"#,
            r#"<g transform="scale(1e308)" display="none"><circle r="5"/></g>"#,
        ] {
            let g = read_svg(&doc(body)).unwrap();
            assert_eq!(g.len(), 4, "{body}: {g:?}");
        }
        // What follows a skipped part still reads, and a shape that is merely styled still draws.
        let g = read_svg(&doc(r#"<defs><circle r="1"/></defs><g style="display:inline"><circle cx="20" cy="20" r="10"/></g>"#)).unwrap();
        assert_eq!(g.len(), 5, "{g:?}");
    }

    /// A transform that scales a shape past the range of `f64` is refused with a clear error
    /// instead of handing the sketch an infinite curve (solidcraft#5020).
    #[test]
    fn geometry_scaled_to_infinity_is_refused() {
        let doc = |body: &str| format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100mm" height="100mm">{body}</svg>"#);
        for body in [
            r#"<g transform="scale(1e308)"><circle r="5"/></g>"#,
            r#"<g transform="scale(1e308)"><g transform="scale(1e308)"></g></g>"#,
            r#"<g transform="scale(1e308)"><rect width="5" height="5"/></g>"#,
            r#"<g transform="scale(1e308)"><path d="M0 0 C 1 1 2 2 5 5"/></g>"#,
            r#"<g transform="matrix(1e308 0 0 1e308 1e308 1e308)"><line x1="1" y1="1" x2="5" y2="5"/></g>"#,
            r#"<g transform="scale(1e308)"><text x="0" y="0" font-size="16">A</text></g>"#,
        ] {
            match read_svg(&doc(body)) {
                Err(IoError::Invalid(m)) => assert!(m.contains("out of range"), "{body}: {m}"),
                other => panic!("{body}: expected an out-of-range error, got {other:?}"),
            }
        }
        // Large but finite drawings still read.
        let g = read_svg(&doc(r#"<g transform="scale(1e6)"><circle r="5"/></g>"#)).unwrap();
        assert!(matches!(g[0], Geom2::Circle(_, r) if (r - 5e6).abs() < 1e-3));
    }
}
