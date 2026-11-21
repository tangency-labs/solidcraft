//! SVG (W3C SVG 1.1) geometry for sketches: `path` (all commands), `line`, `rect`, `circle`,
//! `ellipse`, `polyline`, `polygon` and `text`, with `transform`s on elements and groups. User
//! units are CSS pixels (1/96 in) unless the root's `width`/`height` carry units with a
//! `viewBox`; y is flipped so the drawing reads upright in the sketch. A shape the file draws
//! through a reference (a `use` element) is read where the reference stands (#15538).

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use solidcore_ops::curve::CubicStanding;
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use std::collections::HashMap;
use std::ops::Range;

use crate::sketch2d::{Geom2, MAX_DRAWING_BYTES, MAX_ENTITIES};
use crate::{IoError, Result};

const MAX_DEPTH: usize = 256;
/// The deepest chain of references (`use` inside `use`) read (#15538).
const MAX_USE_DEPTH: usize = 16;
/// Elements read while following references (#15538): a reference to a group that holds more
/// references draws the same element many times over.
const MAX_USE_WORK: usize = 4 * MAX_ENTITIES;
const PX_MM: f64 = 25.4 / 96.0;

/// Affine map [a c e; b d f].
#[derive(Clone, Copy, Debug)]
struct Tf([f64; 6]);

impl Tf {
    const ID: Tf = Tf([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn translate(x: f64, y: f64) -> Tf {
        Tf([1.0, 0.0, 0.0, 1.0, x, y])
    }
    fn scale(sx: f64, sy: f64) -> Tf {
        Tf([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }
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
        Geom2::Nurbs { pts, knots, weights, .. } => {
            pts.iter().all(|p| p.is_finite()) && knots.iter().all(|k| k.is_finite()) && weights.iter().all(|w| w.is_finite())
        }
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
    warnings: Vec<CurveWarning>,
}

/// A cubic of an imported drawing that is kept as drawn although SolidCraft's curve check
/// refuses it (or could not check it), so a body made from it would fail: the reading warns
/// about it by name rather than refusing the file (#16190).
#[derive(Clone, Debug, PartialEq)]
pub struct CurveWarning {
    /// Its number among the drawing's imported curves and lines, from 1.
    pub number: usize,
    /// Its start, in the drawing's millimetres.
    pub from: Vec2,
    /// Its end, in the drawing's millimetres.
    pub to: Vec2,
    /// `None`: the check refuses it. `Some(why)`: the kernel could not check it.
    pub unchecked: Option<String>,
}

impl std::fmt::Display for CurveWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (n, a, b) = (self.number, self.from, self.to);
        write!(f, "curve {n} of the drawing, from ({:.4}, {:.4}) to ({:.4}, {:.4}) mm, ", a.x, a.y, b.x, b.y)?;
        match &self.unchecked {
            None => f.write_str(
                "bends too sharply or slows almost to a stop near an end: SolidCraft's curve check refuses it, so a body made from it will fail. Move its handles away from its ends, or draw it as a line",
            ),
            Some(why) => write!(f, "could not be checked: {why}"),
        }
    }
}

/// An SVG drawing's geometry and the warnings about curves kept as drawn ([`CurveWarning`]).
#[derive(Clone, Debug, Default)]
pub struct SvgDrawing {
    /// The geometry, in millimetres, y up.
    pub geometry: Vec<Geom2>,
    /// One warning for each cubic the curve check refuses or could not check, in order.
    pub warnings: Vec<CurveWarning>,
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
        let p = p.map(|q| t.apply(q));
        if !p.iter().all(|q| q.is_finite()) {
            return Err(IoError::Invalid(OUT_OF_RANGE.into()));
        }
        // Measured after the SVG transform: a cubic within the model's zone tolerance of its chord
        // is that line; one the kernel's curve check refuses is split exactly into pieces it keeps
        // (lines where a piece lies within the zone tolerance of its chord). Only a cubic that no
        // such split serves stays as drawn, with a warning naming it (#16190).
        let pieces = match solidcore_ops::curve::cubic_pieces(p.map(|q| [q.x, q.y, 0.0])) {
            Ok(pieces) => pieces,
            Err(e) => {
                let number = self.g.len() + 1;
                self.warnings.push(CurveWarning { number, from: p[0], to: p[3], unchecked: Some(e.to_string()) });
                return self.push(Geom2::Spline { pts: p.to_vec(), control: true, degree: 3 });
            }
        };
        for piece in pieces {
            let q = piece.points.map(|[x, y, _]| Vec2::new(x, y));
            match piece.standing {
                CubicStanding::Chord => self.line(Tf::ID, q[0], q[3])?,
                CubicStanding::Curve => self.push(Geom2::Spline { pts: q.to_vec(), control: true, degree: 3 })?,
                CubicStanding::Refused => {
                    let number = self.g.len() + 1;
                    self.warnings.push(CurveWarning { number, from: q[0], to: q[3], unchecked: None });
                    self.push(Geom2::Spline { pts: q.to_vec(), control: true, degree: 3 })?;
                }
            }
        }
        Ok(())
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

fn path(out: &mut Out, t: Tf, d: &str) -> Result<()> {
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
            let o = base(p);
            let v = |i: usize| Vec2::new(a[i], a[i + 1]) + o;
            match c.to_ascii_uppercase() {
                'M' => {
                    let q = v(0);
                    if k == 0 {
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
    Ok(())
}

fn points_list(s: &str) -> Vec<Vec2> {
    numbers(s).chunks(2).filter(|c| c.len() == 2).map(|c| Vec2::new(c[0], c[1])).collect()
}

/// Whether an element is hidden with `display: none` (as an attribute or in `style`).
fn hidden(a: &[(String, String)]) -> bool {
    let none = |v: &str| v.trim().trim_end_matches("!important").trim().eq_ignore_ascii_case("none");
    attr(a, "display").is_some_and(none)
        || attr(a, "style")
            .is_some_and(|s| s.split(';').filter_map(|d| d.split_once(':')).any(|(k, v)| k.trim().eq_ignore_ascii_case("display") && none(v)))
}

/// Whether an element's contents are only drawn through a reference (#11039): the contents of
/// `defs`, `clipPath`, `mask`, `symbol`, `marker` and `pattern` are not drawn where they stand.
fn defined_only(name: &str) -> bool {
    matches!(name, "defs" | "clipPath" | "mask" | "symbol" | "marker" | "pattern")
}

/// Whether an element and everything in it is left out of the drawing (#11039): the contents of
/// `defs`, `clipPath`, `mask`, `symbol`, `marker` and `pattern` are only drawn through a
/// reference, and `display: none` (as an attribute or in `style`) hides an element with its children.
/// A `symbol` or `svg` that a reference points at is drawn where the reference stands, not where
/// it is defined (#15538): it is the reference that draws it, so its own `display: none` does not
/// hide it. The reference that points at it, and the shapes it holds, keep their own.
fn not_drawn(name: &str, a: &[(String, String)], referenced: bool) -> bool {
    (defined_only(name) || hidden(a)) && !referenced
}

/// The root element's transformation (user units to millimetres, y flipped) and its sheet in
/// user units (`viewBox`, else `width` and `height` as a box at the origin), when it has one.
fn root_transform(a: &[(String, String)]) -> (Tf, Option<[f64; 4]>) {
    let vb = attr(a, "viewBox").map(numbers).filter(|v| v.len() == 4);
    let (w, h) = (attr(a, "width").and_then(length_mm), attr(a, "height").and_then(length_mm));
    let (sx, sy, ox, oy, sheet) = match (vb, w, h) {
        (Some(v), Some(w), Some(h)) if v[2] > 0.0 && v[3] > 0.0 => (w / v[2], h / v[3], v[0], v[1], Some([v[0], v[1], v[2], v[3]])),
        (Some(v), _, _) => (PX_MM, PX_MM, v[0], v[1], (v[2] > 0.0 && v[3] > 0.0).then_some([v[0], v[1], v[2], v[3]])),
        (None, Some(w), Some(h)) if w > 0.0 && h > 0.0 => (PX_MM, PX_MM, 0.0, 0.0, Some([0.0, 0.0, w / PX_MM, h / PX_MM])),
        _ => (PX_MM, PX_MM, 0.0, 0.0, None),
    };
    (Tf([sx, 0.0, 0.0, -sy, -ox * sx, oy * sy]), sheet)
}

/// The length an element gives, in user units: an absolute length as it is, a percentage as that
/// share of the viewport it stands in, and `None` where the element gives none.
fn length_in(a: &[(String, String)], k: &str, viewport: Option<f64>) -> Option<f64> {
    let v = attr(a, k)?.trim();
    if let Some(percent) = v.strip_suffix('%') {
        let percent = numbers(percent).first().copied()?;
        return Some(percent / 100.0 * viewport?);
    }
    numbers(v).first().copied()
}

/// The viewport the root element establishes, in user units: its `viewBox` when it has one, else
/// its `width` and `height` as a box of pixels.
fn root_viewport(a: &[(String, String)]) -> Viewport {
    if let Some(vb) = attr(a, "viewBox").map(numbers).filter(|v| v.len() == 4)
        && vb[2] > 0.0
        && vb[3] > 0.0
    {
        return Viewport { width: Some(vb[2]), height: Some(vb[3]) };
    }
    let px = |k: &str| attr(a, k).and_then(length_mm).map(|mm| mm / PX_MM);
    Viewport { width: px("width"), height: px("height") }
}

/// The box a referenced element is drawn in, in user units: the size the reference gives, else the
/// size the element declares, else the whole viewport the reference stands in (SVG 1.1 gives an
/// unsized instance 100% dimensions).
fn instance_box(a: &[(String, String)], r: Reference) -> Viewport {
    Viewport {
        width: r.size.width.or_else(|| length_in(a, "width", r.containing.width)).or(r.containing.width),
        height: r.size.height.or_else(|| length_in(a, "height", r.containing.height)).or(r.containing.height),
    }
}

/// What the reading does with the element a reference points at: draw it in this transformation
/// and in a box of this size, or draw nothing at all.
enum Ref {
    /// Draw it with this transformation, in a box of this size.
    Draw(Tf, Viewport),
    /// Draw nothing: its box holds nothing.
    Nothing,
}

/// The transformation that draws the element a reference points at: a `symbol` or `svg` is drawn
/// in the box the reference gives it, its `viewBox` scaled to fit the box and centred in it
/// (SVG 1.1 `preserveAspectRatio`, which we do not read). A box of no size, or a `viewBox` of no
/// size, draws nothing.
fn reference_tf(a: &[(String, String)], r: Reference) -> Ref {
    let box_ = instance_box(a, r);
    // A box of no size holds nothing: the shapes it would hold are left out.
    if box_.width.is_some_and(|w| w <= 0.0) || box_.height.is_some_and(|h| h <= 0.0) {
        return Ref::Nothing;
    }
    let Some(vb) = attr(a, "viewBox").map(numbers).filter(|v| v.len() == 4) else { return Ref::Draw(Tf::ID, box_) };
    // A `viewBox` of no size holds nothing either.
    if vb[2] <= 0.0 || vb[3] <= 0.0 {
        return Ref::Nothing;
    }
    let w = box_.width.unwrap_or(vb[2]);
    let h = box_.height.unwrap_or(vb[3]);
    // The box keeps its shape: the viewBox is scaled to fit it and centred in it.
    let k = (w / vb[2]).min(h / vb[3]);
    let tx = (w - vb[2] * k) / 2.0 - vb[0] * k;
    let ty = (h - vb[3] * k) / 2.0 - vb[1] * k;
    Ref::Draw(Tf::translate(tx, ty).mul(Tf::scale(k, k)), box_)
}

/// The sheet of an SVG drawing in millimetres, in the coordinates its geometry comes out in: the
/// root's `viewBox` (else its `width` and `height`) through the same transformation. None when
/// the file gives no sheet.
pub fn svg_sheet(text: &str) -> Option<(Vec2, Vec2)> {
    if text.len() > MAX_DRAWING_BYTES {
        return None;
    }
    let mut rd = Reader::from_reader(text.as_bytes());
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut elements = 0;
    while elements < MAX_ENTITIES {
        match rd.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                elements += 1;
                if e.local_name().as_ref() == b"svg" {
                    let (tf, sheet) = root_transform(&attrs(&e));
                    let [x, y, w, h] = sheet?;
                    let (a, b) = (tf.apply(Vec2::new(x, y)), tf.apply(Vec2::new(x + w, y + h)));
                    let (lo, hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
                    return (lo.is_finite() && hi.is_finite() && hi.x > lo.x && hi.y > lo.y).then_some((lo, hi));
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
    None
}

/// The size of a viewport, in user units: `None` where it is not known.
#[derive(Clone, Copy, Default)]
struct Viewport {
    width: Option<f64>,
    height: Option<f64>,
}

/// What a reference (`use`) gives the element it points at (#15538): the size it states, and the
/// viewport it stands in, which a size in percent is read against.
#[derive(Clone, Copy, Default)]
struct Reference {
    /// The size the reference states, in user units.
    size: Viewport,
    /// The viewport the reference stands in, in user units.
    containing: Viewport,
}

/// The state of reading an SVG document: the file, the elements its references point at, and the
/// geometry read so far.
struct SvgReader<'a> {
    /// The file.
    text: &'a str,
    /// The elements the file names with `id`, and where each one is in it, so that a reference
    /// (`use`) to one of them can be read where the reference stands (#15538).
    ids: HashMap<String, Range<usize>>,
    /// Whether the elements named with `id` have been indexed yet: a file without a reference
    /// never pays for the index.
    indexed: bool,
    /// The geometry read so far.
    out: Out,
    /// The references being read, so that a reference that leads back to itself is not read twice.
    chain: Vec<String>,
    /// Elements read while following references.
    work: usize,
}

impl SvgReader<'_> {
    /// Read the elements of the file in the byte range `span`, drawn with `base` in place.
    /// `target` says the range holds the element a reference points at, with what the reference
    /// gives it.
    fn read_span(&mut self, span: Range<usize>, base: Tf, depth: usize, target: Option<Reference>) -> Result<()> {
        let Some(part) = self.text.get(span.clone()) else { return Ok(()) };
        let mut rd = Reader::from_reader(part.as_bytes());
        rd.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut stack: Vec<Tf> = Vec::new();
        let mut root: Option<Tf> = None;
        // The viewport the elements of this range stand in, in user units: the one the reference
        // that points at them stands in, else the one the root element establishes.
        let mut containing = target.map(|r| r.containing).unwrap_or_default();
        let mut pending_text: Option<(Tf, Vec2, f64)> = None;
        // The depth of the element whose contents are not drawn (#11039), while inside one.
        let mut skip_from: Option<usize> = None;
        // Whether the element being read is the one a reference points at.
        let mut first = true;
        loop {
            let ev = rd.read_event_into(&mut buf).map_err(|e| IoError::Invalid(format!("SVG: {e}")))?;
            let (e, empty) = match &ev {
                Event::Start(e) => (Some(e.clone()), false),
                Event::Empty(e) => (Some(e.clone()), true),
                Event::End(_) => {
                    stack.pop();
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
                            self.out.push(Geom2::Text { text: s, at: tf.apply(at), height: h * 0.7 * k, angle: dir.angle() })?;
                        }
                    }
                    buf.clear();
                    continue;
                }
                Event::Eof => break,
                _ => (None, false),
            };
            let Some(e) = e else {
                buf.clear();
                continue;
            };
            let a = attrs(&e);
            let parent = stack.last().copied().or(root).unwrap_or(base);
            let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
            // A `symbol` or `svg` a reference points at is drawn in the box the reference gives it,
            // not where it is defined (#15538).
            let referenced = first && target.is_some() && matches!(name.as_str(), "symbol" | "svg");
            first = false;
            if depth > 0 {
                // A reference multiplies the work: the same element can be drawn many times over.
                self.work = self.work.saturating_add(1);
                if self.work > MAX_USE_WORK {
                    return Err(IoError::Invalid("the drawing is too complex".into()));
                }
            }
            // What is done with the element a reference points at: drawn in the box the reference
            // gives it, or not drawn at all when that box holds nothing.
            let mut pointed: Option<(Tf, Viewport)> = None;
            let mut nothing = false;
            if referenced {
                match reference_tf(&a, target.unwrap_or_default()) {
                    Ref::Draw(t, box_) => pointed = Some((t, box_)),
                    Ref::Nothing => nothing = true,
                }
            }
            if skip_from.is_some() || nothing || not_drawn(&name, &a, referenced) {
                // Keep the stack in step with the end tags; draw nothing until this element closes.
                if !empty {
                    skip_from = skip_from.or(Some(stack.len()));
                    stack.push(parent);
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
            if name == "svg" && depth == 0 && root.is_none() {
                let (r, _) = root_transform(&a);
                root = Some(r);
                tf = r;
                // The root establishes the viewport its contents stand in.
                containing = root_viewport(&a);
            }
            if let Some((t, box_)) = pointed {
                // The element a reference points at is drawn in the box the reference gives it,
                // and what it holds stands in that box.
                tf = tf.mul(t);
                containing = box_;
            }
            if !tf.is_finite() {
                return Err(IoError::Invalid(OUT_OF_RANGE.into()));
            }
            match name.as_str() {
                "path" => path(&mut self.out, tf, attr(&a, "d").unwrap_or(""))?,
                "line" => self.out.line(tf, Vec2::new(num_attr(&a, "x1"), num_attr(&a, "y1")), Vec2::new(num_attr(&a, "x2"), num_attr(&a, "y2")))?,
                "rect" => {
                    let (x, y, w, h) = (num_attr(&a, "x"), num_attr(&a, "y"), num_attr(&a, "width"), num_attr(&a, "height"));
                    if w > 0.0 && h > 0.0 {
                        let c = [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)];
                        for i in 0..4 {
                            self.out.line(tf, c[i], c[(i + 1) % 4])?;
                        }
                    }
                }
                "circle" => {
                    let r = num_attr(&a, "r");
                    if r > 0.0 {
                        self.out.elliptic(tf, Vec2::new(num_attr(&a, "cx"), num_attr(&a, "cy")), r, r, 0.0, 0.0, std::f64::consts::TAU)?;
                    }
                }
                "ellipse" => {
                    let (rx, ry) = (num_attr(&a, "rx"), num_attr(&a, "ry"));
                    if rx > 0.0 && ry > 0.0 {
                        self.out.elliptic(tf, Vec2::new(num_attr(&a, "cx"), num_attr(&a, "cy")), rx, ry, 0.0, 0.0, std::f64::consts::TAU)?;
                    }
                }
                "polyline" | "polygon" => {
                    let pts = points_list(attr(&a, "points").unwrap_or(""));
                    for w in pts.windows(2) {
                        self.out.line(tf, w[0], w[1])?;
                    }
                    if name == "polygon"
                        && let (Some(f), Some(l)) = (pts.first(), pts.last())
                    {
                        self.out.line(tf, *l, *f)?;
                    }
                }
                "text" => {
                    let size = attr(&a, "font-size").map(numbers).and_then(|v| v.first().copied()).unwrap_or(16.0);
                    pending_text = Some((tf, Vec2::new(num_attr(&a, "x"), num_attr(&a, "y")), size));
                }
                "use" => {
                    // A reference draws the element it points at, where the reference stands
                    // (#15538): that element and its contents are drawn as if they were here.
                    let href = attr(&a, "href").unwrap_or("").trim().to_string();
                    if let Some(id) = href.strip_prefix('#') {
                        // The elements the reference can point at are indexed once, when the
                        // first reference asks for them.
                        if !self.indexed {
                            self.ids = index_ids(self.text)?;
                            self.indexed = true;
                        }
                        // The size the reference gives the element it points at: an absolute size
                        // as it is, a percentage as that share of the viewport it stands in.
                        let size = Viewport { width: length_in(&a, "width", containing.width), height: length_in(&a, "height", containing.height) };
                        // The reference's own placement, appended to its transformation.
                        let at = tf.mul(Tf::translate(num_attr(&a, "x"), num_attr(&a, "y")));
                        self.read_reference(id, at, depth, Reference { size, containing })?;
                    }
                }
                _ => {}
            }
            if !empty {
                stack.push(tf);
                if stack.len() > MAX_DEPTH {
                    return Err(IoError::Invalid("SVG nested too deeply".into()));
                }
            }
            buf.clear();
        }
        Ok(())
    }

    /// Read the element a reference points at, drawn where the reference stands (#15538).
    fn read_reference(&mut self, id: &str, at: Tf, depth: usize, reference: Reference) -> Result<()> {
        // A reference that leads back to itself is not read again.
        if self.chain.iter().any(|r| r == id) {
            return Ok(());
        }
        // A chain of references deeper than the reader follows is refused: the drawing would be
        // left incomplete, and a drawing that says so is better than one that does not.
        if depth >= MAX_USE_DEPTH {
            return Err(IoError::Invalid("the drawing is too complex".into()));
        }
        let Some(span) = self.ids.get(id).cloned() else { return Ok(()) };
        self.chain.push(id.to_string());
        let read = self.read_span(span, at, depth + 1, Some(reference));
        self.chain.pop();
        read
    }
}

/// Read the geometry of an SVG document (millimetres, y up), without its warnings
/// ([`read_svg_drawing`]). A reference (a `use` element) is followed: the element it points at is
/// read where the reference stands (#15538).
pub fn read_svg(text: &str) -> Result<Vec<Geom2>> {
    read_svg_drawing(text).map(|d| d.geometry)
}

/// Read the geometry of an SVG document (millimetres, y up) and the warnings about curves kept as
/// drawn that the curve check refuses. A reference (a `use` element) is followed: the element it
/// points at is read where the reference stands (#15538).
pub fn read_svg_drawing(text: &str) -> Result<SvgDrawing> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the SVG file is too large".into()));
    }
    let mut reader =
        SvgReader { text, ids: HashMap::new(), indexed: false, out: Out { g: Vec::new(), warnings: Vec::new() }, chain: Vec::new(), work: 0 };
    reader.read_span(0..text.len(), Tf::ID, 0, None)?;
    Ok(SvgDrawing { geometry: reader.out.g, warnings: reader.out.warnings })
}

/// The elements a file names with `id`, and where each one is in the file, so that a reference
/// (`use`) to one of them can be read where the reference stands (#15538). The scan is bounded: a
/// file that nests deeper than the reader reads, or names more elements than a drawing can hold,
/// is refused rather than indexed without end.
fn index_ids(text: &str) -> Result<HashMap<String, Range<usize>>> {
    let mut ids: HashMap<String, Range<usize>> = HashMap::new();
    let mut rd = Reader::from_reader(text.as_bytes());
    rd.config_mut().trim_text(true);
    let mut buf = Vec::new();
    // The elements that are open, and the byte each one starts at.
    let mut open: Vec<(Option<String>, usize)> = Vec::new();
    loop {
        let start = rd.buffer_position() as usize;
        let ev = rd.read_event_into(&mut buf);
        let end = rd.buffer_position() as usize;
        match &ev {
            Ok(Event::Start(e)) => {
                // The open elements are a stack: a file that nests deeper than the reader reads
                // is refused, so that the stack cannot grow without end.
                if open.len() >= MAX_DEPTH {
                    return Err(IoError::Invalid("the SVG file nests too deeply to index its references".into()));
                }
                open.push((id_of(e), start));
            }
            Ok(Event::Empty(e)) => {
                if let Some(id) = id_of(e) {
                    ids.entry(id).or_insert(start..end);
                }
            }
            Ok(Event::End(_)) => {
                if let Some((id, from)) = open.pop()
                    && let Some(id) = id
                {
                    ids.entry(id).or_insert(from..end);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
        if ids.len() >= MAX_ENTITIES {
            return Err(IoError::Invalid("the SVG file names too many elements to index its references".into()));
        }
    }
    Ok(ids)
}

/// The `id` an element is named by, when it has one.
fn id_of(e: &BytesStart) -> Option<String> {
    let a = attrs(e);
    attr(&a, "id").map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
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

    /// The sheet: the viewBox in the millimetres `width` and `height` give, else in pixels; only
    /// `width` and `height` make a sheet at the origin; without either there is none.
    #[test]
    fn the_sheet_follows_the_root_like_the_geometry() {
        let close = |a: Vec2, b: Vec2| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
        let both = svg_sheet(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 200 100"/>"#).unwrap();
        assert!(close(both.0, Vec2::new(0.0, -50.0)) && close(both.1, Vec2::new(100.0, 0.0)), "{both:?}");
        let vb = svg_sheet(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="10 20 200 100"/>"#).unwrap();
        assert!(close(vb.0, Vec2::new(0.0, -100.0 * PX_MM)) && close(vb.1, Vec2::new(200.0 * PX_MM, 0.0)), "{vb:?}");
        let wh = svg_sheet(r#"<svg xmlns="http://www.w3.org/2000/svg" width="30mm" height="20mm"/>"#).unwrap();
        assert!(close(wh.0, Vec2::new(0.0, -20.0)) && close(wh.1, Vec2::new(30.0, 0.0)), "{wh:?}");
        assert!(svg_sheet(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="3" height="4"/></svg>"#).is_none());
        assert!(svg_sheet(r#"<svg viewBox="0 0 0 10"/>"#).is_none(), "a sheet without area is none");
    }

    /// The parser recurses over groups and reads numbers out of a document the user did not write,
    /// so none of these may panic: `catch_unwind` turns one into a failed test, the way
    /// `hostile_files_never_panic` does for STEP (solidcraft#469).
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

    #[test]
    fn a_cubic_within_the_zone_tolerance_of_its_chord_is_a_line() {
        let read = |path, transform| {
            read_svg(&format!(r#"<svg width="10mm" height="10mm" viewBox="0 0 10 10"><path transform="{transform}" d="{path}"/></svg>"#))
        };
        let spline = |g: &[Geom2]| matches!(g, [Geom2::Spline { pts, control: true, degree: 3 }] if pts.len() == 4);
        assert!(read("M1 1 C1 1 1 1 1 1", "").unwrap().is_empty());
        // 0.000005 mm off its chord, within the zone tolerance (0.00001 mm): a line. Measured
        // after the transform, ten times that is a curve.
        let path = "M0 0 C1 0.000005 2 0.000005 3 0";
        assert!(matches!(read(path, "").unwrap().as_slice(), [Geom2::Line(_, _)]));
        assert!(spline(&read(path, "scale(10)").unwrap()));
        assert!(spline(&read("M0 0 C1 0.0005 2 0.0005 3 0", "").unwrap()));
        // Equal endpoints do not make a curve degenerate.
        assert!(spline(&read("M0 0 C10 10 -10 10 0 0", "").unwrap()));
        // Collinear and overshooting its ends, each of these cubics stops and turns back, which
        // the curve check refuses: both are imported as drawn, each with a warning naming it.
        let d = read_svg_drawing(
            r#"<svg width="10mm" height="10mm" viewBox="0 0 10 10"><path d="M0 0 L0 5 M0 0 C-10 0 20 0 1 0 M0 3 C-10 3 20 3 1 3"/></svg>"#,
        )
        .unwrap();
        assert!(matches!(d.geometry.as_slice(), [Geom2::Line(..), Geom2::Spline { .. }, Geom2::Spline { .. }]), "{:?}", d.geometry);
        assert_eq!(d.warnings.iter().map(|w| (w.number, w.unchecked.is_none())).collect::<Vec<_>>(), [(2, true), (3, true)]);
        assert!(
            d.warnings[1].to_string().starts_with("curve 3 of the drawing, from (0.0000, -3.0000) to (1.0000, -3.0000) mm, bends"),
            "{}",
            d.warnings[1]
        );
        // Collinear, stopping at its middle and turning back (x reaches 2, ends at 1): split
        // there, each half would be within the zone tolerance of its chord, and the lines 0 to 2
        // and 2 to 1 would fold over each other. It is kept as drawn, with a warning.
        let d = read_svg_drawing(r#"<svg width="10mm" height="10mm" viewBox="0 0 10 10"><path d="M0 0 C3 0 2 0 1 0"/></svg>"#).unwrap();
        assert!(matches!(d.geometry.as_slice(), [Geom2::Spline { .. }]), "{:?}", d.geometry);
        assert_eq!(d.warnings.iter().map(|w| (w.number, w.unchecked.is_none())).collect::<Vec<_>>(), [(1, true)]);
    }

    /// Synthetic outlines (#16190). e and f each have a cubic whose inner control points almost
    /// meet its start: about 0.00053 mm off its chord, above the zone tolerance, so it is no line,
    /// and its bend there is too sharp for the curve check at the curve's size. It is imported
    /// split exactly into pieces the check passes, with no warning, and the outline extrudes to
    /// a valid solid of the drawn volume. g, a hook about 0.00007 mm across, and h's clearly
    /// curved cubics pass the check and stay one curve each.
    #[test]
    fn imported_cubic_outlines_extrude_to_valid_solids() {
        use solidcore_record::{Loop2, Plane, Region2, Seg2};
        let svg = |path: &str| format!(r#"<svg width="2480px" height="3508px" viewBox="0 0 2480 3508"><path d="{path}"/></svg>"#);
        let splines = |g: &[Geom2]| g.iter().filter(|g| matches!(g, Geom2::Spline { .. })).count();

        let g = "M200 200 L2000 200 L2000 1500 L1000.0001 1500 C 1000.0002 1500.0001, 1000.0001 1500.0002, 1000 1500.0001 L200 1500 Z";
        let d = read_svg_drawing(&svg(g)).unwrap();
        assert_eq!(splines(&d.geometry), 1);
        assert!(d.warnings.is_empty());

        // Expected volumes from integrating (x dy - y dx)/2 exactly over the line and cubic
        // polynomials, times (25.4/96)^2 and the 2 mm depth.
        let e =
            "M200 200 L2000 200 L2000 1500 C 1990 1500.01, 1980 1500.02, 1900 1500.03 C 1899.99 1500.031, 1899.98 1500.032, 1800 1500 L200 1500 Z";
        let f = "M200 200 L2000 200 C 2100 200, 2200 300, 2200 400 C 2200.001 400.002, 2200.002 400.001, 2200 500 L2000 1500 L200 1500 Z";
        let h = "M200 200 L2000 200 L2000 1500 C 1500 1500, 1200 1600, 1000 1500 C 900 1450, 1100 1600, 1000 1499.99 L200 1500 Z";
        for (name, path, volume) in [("e", e, 327620.81656275195), ("f", f, 348691.6294241992), ("h", h, 330664.92026605905)] {
            let d = read_svg_drawing(&svg(path)).unwrap();
            assert!(d.warnings.is_empty(), "{name}: {:?}", d.warnings);
            if name == "h" {
                assert_eq!(splines(&d.geometry), 2);
            } else {
                // The refused cubic is now more than one piece.
                assert!(d.geometry.len() > 6, "{name}: {:?}", d.geometry);
            }
            let segs = d
                .geometry
                .iter()
                .map(|g| match g {
                    Geom2::Line(a, b) => Seg2::Line { a: *a, b: *b },
                    Geom2::Spline { pts, control: true, degree: 3 } => {
                        let [p0, p1, p2, p3] = pts.as_slice() else { panic!("four cubic controls") };
                        Seg2::Cubic { p0: *p0, p1: *p1, p2: *p2, p3: *p3 }
                    }
                    other => panic!("{name}: unexpected imported geometry: {other:?}"),
                })
                .collect();
            let region = Region2 { outer: Loop2 { segs }, holes: Vec::new() };
            let bodies = solidcraft_kernel::extrude(&Plane::XY, &[region], 0.0, 2.0).unwrap();
            assert_eq!(bodies.len(), 1, "{name}");
            let measured = solidcraft_kernel::measure_mass(&bodies[0]).unwrap();
            assert!((measured.volume - volume).abs() < 0.005, "{name}: {measured:?}");
            let check = solidcraft_kernel::validity(&bodies[0]);
            assert!(check.ok() && matches!(check.smlib, Some(solidcraft_kernel::SmlibCheck::Valid)), "{name}: {check:?}");
        }
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

    /// #15538: a file that draws its shapes through a reference (a `use` element pointing at a
    /// shape in `defs`, as Silhouette Studio exports it) draws them, so they are imported, where
    /// the reference stands; a shape the file only defines is still not imported (#11039).
    #[test]
    fn a_shape_the_file_draws_through_a_reference_is_imported() {
        let doc = |body: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"##
            )
        };
        let defs = r##"<defs><path id="drawn" d="M0 0 L10 10"/><circle id="only_defined" cx="50" cy="50" r="5"/></defs>"##;
        // The line the file draws through the reference, at the place the reference stands.
        let g = read_svg(&doc(&format!(r##"{defs}<g><use xlink:href="#drawn" x="20" y="30"/></g>"##))).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        assert!(matches!(g[0], Geom2::Line(a, b) if a.dist(Vec2::new(20.0, -30.0)) < 1e-9 && b.dist(Vec2::new(30.0, -40.0)) < 1e-9), "{g:?}");
        // The same reference with the `href` of SVG 2, and one inside a transformed group.
        let g = read_svg(&doc(&format!(r##"{defs}<use href="#drawn"/>"##))).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        let g = read_svg(&doc(&format!(r##"{defs}<g transform="translate(10 20)"><use xlink:href="#drawn"/></g>"##))).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        assert!(matches!(g[0], Geom2::Line(a, b) if a.dist(Vec2::new(10.0, -20.0)) < 1e-9 && b.dist(Vec2::new(20.0, -30.0)) < 1e-9), "{g:?}");
        // A reference to a group draws the shapes of the group.
        let g = read_svg(&doc(r##"<defs><g id="group"><rect x="1" y="2" width="3" height="4"/></g></defs><use xlink:href="#group"/>"##)).unwrap();
        assert_eq!(g.len(), 4, "{g:?}");
        // A shape the file only defines and never draws is still not imported (#11039).
        let g = read_svg(&doc(defs)).unwrap();
        assert!(g.is_empty(), "{g:?}");
        // A reference that points at nothing, or at a shape the file hides, draws nothing.
        let g = read_svg(&doc(r##"<use xlink:href="#nowhere"/>"##)).unwrap();
        assert!(g.is_empty(), "{g:?}");
        let g = read_svg(&doc(r##"<defs><path id="hidden" display="none" d="M0 0 L10 10"/></defs><use xlink:href="#hidden"/>"##)).unwrap();
        assert!(g.is_empty(), "{g:?}");
        // A reference that leads back to itself is not read twice.
        let g = read_svg(&doc(r##"<defs><g id="loop"><use xlink:href="#loop"/><path d="M0 0 L1 1"/></g></defs><use xlink:href="#loop"/>"##)).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        // A shape a reference draws past the range of `f64` is refused, as it is without one.
        match read_svg(&doc(r##"<defs><path id="big" transform="scale(1e308)" d="M0 0 L10 10"/></defs><use xlink:href="#big"/>"##)) {
            Err(IoError::Invalid(m)) => assert!(m.contains("out of range"), "{m}"),
            other => panic!("expected an out-of-range error, got {other:?}"),
        }
    }

    /// #15538: a reference that nests deeper than the reader follows is refused with a clear
    /// error, not read half way and reported as a drawing that is complete.
    #[test]
    fn a_reference_that_nests_too_deeply_is_refused() {
        let chain = |n: usize| {
            let mut defs = String::new();
            for i in 1..n {
                defs.push_str(&format!(r##"<g id="d{i}"><use href="#d{}"/></g>"##, i + 1));
            }
            defs.push_str(&format!(r#"<g id="d{n}"><path d="M0 0 L1 1"/></g>"#));
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><defs>{defs}</defs><use href="#d1"/></svg>"##
            )
        };
        // A chain the reader follows is read to its end.
        let g = read_svg(&chain(MAX_USE_DEPTH)).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        // One deeper is refused: the drawing would be incomplete, and it says so.
        match read_svg(&chain(MAX_USE_DEPTH + 1)) {
            Err(IoError::Invalid(m)) => assert!(m.contains("too complex"), "{m}"),
            other => panic!("expected a complexity error, got {other:?}"),
        }
    }

    /// #15538: a file that nests deeper than the reader reads is refused when the elements its
    /// references point at are indexed, so that the stack of open elements cannot grow without
    /// end.
    #[test]
    fn a_file_nested_too_deeply_is_refused() {
        let deep = format!(r##"<svg xmlns="http://www.w3.org/2000/svg"><use href="#nowhere"/>{}</svg>"##, "<g>".repeat(MAX_DEPTH + 10));
        match read_svg(&deep) {
            Err(IoError::Invalid(m)) => assert!(m.contains("to index its references"), "{m}"),
            other => panic!("expected a nesting error, got {other:?}"),
        }
    }

    /// #15538: a symbol a reference points at is drawn where the reference stands, even when the
    /// symbol hides itself: it is the reference that draws it. The reference, and the shapes the
    /// symbol draws, still honour their own `display: none`.
    #[test]
    fn a_referenced_symbol_is_drawn_even_when_it_hides_itself() {
        let doc =
            |body: &str| format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"##);
        let symbol = r##"<defs><symbol id="s" display="none"><path d="M0 0 L10 10"/></symbol></defs>"##;
        // The symbol hides itself, the reference draws it: it is drawn.
        let g = read_svg(&doc(&format!(r##"{symbol}<use href="#s"/>"##))).unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
        assert!(matches!(g[0], Geom2::Line(a, b) if a.dist(Vec2::new(0.0, 0.0)) < 1e-9 && b.dist(Vec2::new(10.0, -10.0)) < 1e-9), "{g:?}");
        // The reference hides itself: nothing is drawn.
        let g = read_svg(&doc(&format!(r##"{symbol}<use href="#s" display="none"/>"##))).unwrap();
        assert!(g.is_empty(), "{g:?}");
        // A shape the symbol draws that hides itself is left out.
        let g =
            read_svg(&doc(r##"<defs><symbol id="s"><path d="M0 0 L10 10" display="none"/><path d="M0 0 L5 5"/></symbol></defs><use href="#s"/>"##))
                .unwrap();
        assert_eq!(g.len(), 1, "{g:?}");
    }

    /// #15538: a reference that gives no size gives the element it points at the viewport it
    /// stands in (SVG 1.1 gives an unsized instance 100% dimensions); a size in percent is that
    /// share of the viewport, and a size in user units is what it says.
    #[test]
    fn a_reference_without_a_size_is_drawn_in_the_containing_viewport() {
        let doc =
            |body: &str| format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"##);
        let symbol = r##"<defs><symbol id="s" viewBox="0 0 10 10"><path d="M0 0 L10 0"/></symbol></defs>"##;
        let line = |body: &str| {
            let g = read_svg(&doc(body)).unwrap();
            match g.as_slice() {
                [Geom2::Line(a, b)] => b.dist(*a),
                other => panic!("expected one line, got {other:?}"),
            }
        };
        // No size: the whole viewport, so the line is ten times the size the symbol declares.
        assert!((line(&format!(r##"{symbol}<use href="#s"/>"##)) - 100.0).abs() < 1e-9);
        // Half the viewport, in percent.
        assert!((line(&format!(r##"{symbol}<use href="#s" width="50%"/>"##)) - 50.0).abs() < 1e-9);
        // A size in user units is what it says.
        assert!((line(&format!(r##"{symbol}<use href="#s" width="20" height="20"/>"##)) - 20.0).abs() < 1e-9);
    }

    /// #15538: a reference that gives the element it points at a box of no size, or an element
    /// whose `viewBox` has no size, draws nothing: the shapes it holds are left out, not drawn at
    /// their own size.
    #[test]
    fn a_reference_in_a_box_of_no_size_draws_nothing() {
        let doc =
            |body: &str| format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"##);
        let symbol = r##"<defs><symbol id="s" viewBox="0 0 10 10"><path d="M0 0 L10 10"/></symbol></defs>"##;
        // A box of no width: nothing is drawn.
        let g = read_svg(&doc(&format!(r##"{symbol}<use href="#s" width="0" height="10"/>"##))).unwrap();
        assert!(g.is_empty(), "{g:?}");
        // A `viewBox` of no size: nothing is drawn.
        let empty = r##"<defs><symbol id="t" viewBox="0 0 0 10"><path d="M0 0 L10 10"/></symbol></defs>"##;
        let g = read_svg(&doc(&format!(r##"{empty}<use href="#t"/>"##))).unwrap();
        assert!(g.is_empty(), "{g:?}");
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
