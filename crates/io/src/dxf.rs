//! ASCII DXF (the public AutoCAD DXF reference): the ENTITIES section's LINE, CIRCLE, ARC,
//! LWPOLYLINE, POLYLINE/VERTEX, POINT, ELLIPSE, SPLINE, TEXT and MTEXT, scaled to millimetres by
//! `$INSUNITS`. Block references (INSERT, with scale, rotation and arrays) are expanded,
//! nested blocks too; paper-space entities are skipped. The writer emits an R12-style file of lines, circles,
//! arcs and points (free-form curves as polylines); the flat pattern's drawing, with ELLIPSE and SPLINE, is
//! [`crate::write_dxf_drawing`].

use std::collections::HashMap;

use solidcraft_geom::{MAX_BEZIER_DEGREE, Vec2};

use crate::sketch2d::{Geom2, MAX_DRAWING_BYTES, MAX_ENTITIES, bulge_arc};
use crate::{IoError, Result};

/// Group code / value pairs.
fn pairs(text: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        let Ok(code) = c.trim().parse::<i32>() else { continue };
        out.push((code, v.trim().to_string()));
    }
    out
}

fn units_scale(p: &[(i32, String)]) -> f64 {
    // $INSUNITS: 1 in, 2 ft, 4 mm, 5 cm, 6 m, 0 unitless (taken as mm).
    let i = p.iter().position(|(c, v)| *c == 9 && v == "$INSUNITS");
    match i.and_then(|i| p.get(i + 1)).and_then(|(_, v)| v.parse::<i32>().ok()) {
        Some(1) => 25.4,
        Some(2) => 304.8,
        Some(5) => 10.0,
        Some(6) => 1000.0,
        _ => 1.0,
    }
}

#[derive(Default)]
struct Ent {
    kind: String,
    f: Vec<(i32, f64)>,
    s: Vec<(i32, String)>,
}

impl Ent {
    fn get(&self, code: i32) -> Option<f64> {
        self.f.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }
    fn all(&self, code: i32) -> Vec<f64> {
        self.f.iter().filter(|(c, _)| *c == code).map(|(_, v)| *v).collect()
    }
    fn text(&self, code: i32) -> Option<&str> {
        self.s.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
}

/// Read the entities of an ASCII DXF file.
pub fn read_dxf(text: &str) -> Result<Vec<Geom2>> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the DXF file is too large".into()));
    }
    if text.starts_with("AutoCAD Binary DXF") {
        return Err(IoError::Invalid("binary DXF is not supported; save as ASCII DXF".into()));
    }
    let p = pairs(text);
    let k = units_scale(&p);
    let start = p.iter().position(|(c, v)| *c == 2 && v == "ENTITIES").ok_or_else(|| IoError::Invalid("no ENTITIES section".into()))?;
    let ents = section(&p, start)?;
    // Blocks: name → (base point, entities).
    let mut blocks: HashMap<String, (Vec2, Vec<Ent>)> = HashMap::new();
    if let Some(bs) = p.iter().position(|(c, v)| *c == 2 && v == "BLOCKS") {
        let all = section(&p, bs)?;
        let mut cur: Option<(String, Vec2, Vec<Ent>)> = None;
        for e in all {
            match e.kind.as_str() {
                "BLOCK" => {
                    let base = Vec2::new(e.get(10).unwrap_or(0.0) * k, e.get(20).unwrap_or(0.0) * k);
                    cur = Some((e.text(2).unwrap_or("").to_string(), base, Vec::new()));
                }
                "ENDBLK" => {
                    if let Some((name, base, list)) = cur.take() {
                        blocks.insert(name, (base, list));
                    }
                }
                _ => {
                    if let Some(c) = cur.as_mut() {
                        c.2.push(e);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    convert(&ents, k, &blocks, 0, &mut out, &mut 0)?;
    Ok(out)
}

/// The entities of the section whose name is at `start`.
fn section(p: &[(i32, String)], start: usize) -> Result<Vec<Ent>> {
    let mut ents: Vec<Ent> = Vec::new();
    for (c, v) in p.iter().skip(start + 1) {
        if *c == 0 {
            if v == "ENDSEC" || v == "EOF" {
                break;
            }
            ents.push(Ent { kind: v.clone(), ..Default::default() });
            if ents.len() > MAX_ENTITIES {
                return Err(IoError::Invalid("too many entities".into()));
            }
            continue;
        }
        let Some(e) = ents.last_mut() else { continue };
        let numeric = matches!(*c, 10..=59 | 60..=99 | 140..=147 | 210..=239);
        match v.parse::<f64>() {
            Ok(x) if numeric && x.is_finite() && x.abs() < 1e12 => e.f.push((*c, x)),
            _ => e.s.push((*c, v.clone())),
        }
    }
    Ok(ents)
}

/// Deepest block nesting expanded.
const MAX_BLOCK_DEPTH: usize = 16;
/// Entities visited and array cells placed while expanding blocks. Every bound below this one
/// is on the geometry that comes out, and a block that emits none trips none of them: N blocks
/// that each insert the same empty block N times fan out to N^MAX_BLOCK_DEPTH visits for no
/// output at all. This counts the work instead.
const MAX_WORK: usize = 4 * MAX_ENTITIES;

/// Entities to geometry (millimetres), expanding block references.
fn convert(ents: &[Ent], k: f64, blocks: &HashMap<String, (Vec2, Vec<Ent>)>, depth: usize, out: &mut Vec<Geom2>, work: &mut usize) -> Result<()> {
    let pt = |x: f64, y: f64| Vec2::new(x * k, y * k);
    let mut i = 0;
    while i < ents.len() {
        let Some(e) = ents.get(i) else { break };
        i += 1;
        *work = work.saturating_add(1);
        if out.len() > MAX_ENTITIES || *work > MAX_WORK {
            return Err(IoError::Invalid("the drawing is too complex".into()));
        }
        // Paper space (layouts) is not the drawing.
        if e.get(67).is_some_and(|v| v != 0.0) {
            continue;
        }
        match e.kind.as_str() {
            "INSERT" => {
                let Some((base, list)) = e.text(2).and_then(|n| blocks.get(n)) else { continue };
                if depth >= MAX_BLOCK_DEPTH {
                    continue;
                }
                let mut inner = Vec::new();
                convert(list, k, blocks, depth + 1, &mut inner, work)?;
                if inner.is_empty() {
                    continue;
                }
                let (sx, sy) = (e.get(41).unwrap_or(1.0), e.get(42).unwrap_or(1.0));
                let rot = e.get(50).unwrap_or(0.0).to_radians();
                let ins = pt(e.get(10).unwrap_or(0.0), e.get(20).unwrap_or(0.0));
                let (cols, rows) = (e.get(70).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize, e.get(71).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize);
                let cells = cols.saturating_mul(rows);
                *work = work.saturating_add(cells);
                if cells > MAX_ENTITIES || cells.saturating_mul(inner.len()) > MAX_ENTITIES || *work > MAX_WORK {
                    return Err(IoError::Invalid("the drawing is too complex".into()));
                }
                let (dc, dr) = (e.get(44).unwrap_or(0.0) * k, e.get(45).unwrap_or(0.0) * k);
                for r in 0..rows {
                    for c in 0..cols {
                        // Array offsets run along the rotated block axes.
                        let off = Vec2::from_angle(rot) * (dc * c as f64) + Vec2::from_angle(rot).perp() * (dr * r as f64);
                        let t = Affine::insert(ins + off, *base, sx, sy, rot);
                        for g in &inner {
                            t.apply(g, out);
                        }
                    }
                }
            }
            "LINE" => {
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (e.get(10), e.get(20), e.get(11), e.get(21)) {
                    out.push(Geom2::Line(pt(x0, y0), pt(x1, y1)));
                }
            }
            "POINT" => {
                if let (Some(x), Some(y)) = (e.get(10), e.get(20)) {
                    out.push(Geom2::Point(pt(x, y)));
                }
            }
            "CIRCLE" => {
                if let (Some(x), Some(y), Some(r)) = (e.get(10), e.get(20), e.get(40)) {
                    out.push(Geom2::Circle(pt(x, y), r * k));
                }
            }
            "ARC" => {
                if let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) = (e.get(10), e.get(20), e.get(40), e.get(50), e.get(51)) {
                    let c = pt(x, y);
                    let at = |deg: f64| c + Vec2::from_angle(deg.to_radians()) * (r * k);
                    out.push(Geom2::Arc { c, a: at(a0), b: at(a1) });
                }
            }
            "LWPOLYLINE" => {
                let ys = e.all(20);
                // Bulges follow their vertex: walk the pairs in order.
                let mut verts: Vec<(Vec2, f64)> = Vec::new();
                let mut xi = 0;
                for (c, v) in &e.f {
                    match *c {
                        10 => {
                            if let Some(y) = ys.get(xi) {
                                verts.push((pt(*v, *y), 0.0));
                            }
                            xi += 1;
                        }
                        42 => {
                            if let Some(last) = verts.last_mut() {
                                last.1 = *v;
                            }
                        }
                        _ => {}
                    }
                }
                let closed = e.get(70).is_some_and(|f| (f as i64) & 1 == 1);
                polyline(&verts, closed, out);
            }
            "POLYLINE" => {
                let closed = e.get(70).is_some_and(|f| (f as i64) & 1 == 1);
                let mut verts = Vec::new();
                while let Some(v) = ents.get(i) {
                    if v.kind != "VERTEX" {
                        break;
                    }
                    if let (Some(x), Some(y)) = (v.get(10), v.get(20)) {
                        verts.push((pt(x, y), v.get(42).unwrap_or(0.0)));
                    }
                    i += 1;
                }
                if ents.get(i).is_some_and(|x| x.kind == "SEQEND") {
                    i += 1;
                }
                polyline(&verts, closed, out);
            }
            "ELLIPSE" => {
                if let (Some(x), Some(y), Some(mx), Some(my), Some(ratio)) = (e.get(10), e.get(20), e.get(11), e.get(21), e.get(40)) {
                    let c = pt(x, y);
                    let major = c + Vec2::new(mx, my) * k;
                    let a = Vec2::new(mx, my).len() * k;
                    let (t0, t1) = (e.get(41).unwrap_or(0.0), e.get(42).unwrap_or(std::f64::consts::TAU));
                    if (t1 - t0 - std::f64::consts::TAU).abs() < 1e-9 || (t1 - t0).abs() < 1e-12 {
                        out.push(Geom2::Ellipse { c, major, minor: a * ratio });
                    } else {
                        // Partial ellipse: a fit spline through samples.
                        let u = (major - c) / a.max(1e-12);
                        let v = u.perp() * (a * ratio);
                        let mut sweep = t1 - t0;
                        while sweep <= 0.0 {
                            sweep += std::f64::consts::TAU;
                        }
                        let n = 16;
                        let pts = (0..=n)
                            .map(|j| {
                                let t = t0 + sweep * j as f64 / n as f64;
                                c + u * (a * t.cos()) + v * t.sin()
                            })
                            .collect();
                        out.push(Geom2::Spline { pts, control: false, degree: 3 });
                    }
                }
            }
            "SPLINE" => {
                // The degrees a sketch spline takes (a G4 blend's Bézier is degree 9).
                let degree = e.get(71).unwrap_or(3.0).clamp(1.0, MAX_BEZIER_DEGREE as f64) as u8;
                let (fx, fy) = (e.all(11), e.all(21));
                let (cx, cy) = (e.all(10), e.all(20));
                if fx.len() >= 2 && fx.len() == fy.len() {
                    out.push(Geom2::Spline { pts: fx.iter().zip(&fy).map(|(x, y)| pt(*x, *y)).collect(), control: false, degree: 3 });
                } else if cx.len() >= 2 && cx.len() == cy.len() {
                    out.push(Geom2::Spline { pts: cx.iter().zip(&cy).map(|(x, y)| pt(*x, *y)).collect(), control: true, degree });
                }
            }
            "TEXT" | "MTEXT" => {
                if let (Some(x), Some(y), Some(h), Some(t)) = (e.get(10), e.get(20), e.get(40), e.text(1)) {
                    // MTEXT paragraph codes: keep the plain text.
                    let t = t.replace("\\P", "\n");
                    out.push(Geom2::Text { text: t, at: pt(x, y), height: h * k, angle: e.get(50).unwrap_or(0.0).to_radians() });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// p ↦ m·p + t.
#[derive(Clone, Copy)]
struct Affine {
    m: [f64; 4],
    t: Vec2,
}

impl Affine {
    /// A block placed at `ins`: its base point moves there, scaled then rotated.
    fn insert(ins: Vec2, base: Vec2, sx: f64, sy: f64, rot: f64) -> Affine {
        let (s, c) = rot.sin_cos();
        let m = [c * sx, -s * sy, s * sx, c * sy];
        let mb = Vec2::new(m[0] * base.x + m[1] * base.y, m[2] * base.x + m[3] * base.y);
        Affine { m, t: ins - mb }
    }
    fn p(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.m[0] * p.x + self.m[1] * p.y, self.m[2] * p.x + self.m[3] * p.y) + self.t
    }
    fn v(&self, v: Vec2) -> Vec2 {
        Vec2::new(self.m[0] * v.x + self.m[1] * v.y, self.m[2] * v.x + self.m[3] * v.y)
    }
    fn det(&self) -> f64 {
        self.m[0] * self.m[3] - self.m[1] * self.m[2]
    }
    /// The scale when it is the same in every direction.
    fn uniform(&self) -> Option<f64> {
        let (a, b) = (self.v(Vec2::new(1.0, 0.0)), self.v(Vec2::new(0.0, 1.0)));
        ((a.len() - b.len()).abs() <= 1e-9 * a.len().max(b.len()) && a.dot(b).abs() <= 1e-9 * a.len() * b.len()).then_some(a.len())
    }
    fn apply(&self, g: &Geom2, out: &mut Vec<Geom2>) {
        let flip = self.det() < 0.0;
        let sampled = |f: &dyn Fn(f64) -> Vec2, closed: bool| {
            let n = 32;
            let mut pts: Vec<Vec2> = (0..=n).map(|j| self.p(f(j as f64 / n as f64))).collect();
            if closed {
                pts.pop();
                if let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
            }
            Geom2::Spline { pts, control: false, degree: 3 }
        };
        out.push(match g {
            Geom2::Point(p) => Geom2::Point(self.p(*p)),
            Geom2::Line(a, b) => Geom2::Line(self.p(*a), self.p(*b)),
            Geom2::Circle(c, r) => match self.uniform() {
                Some(s) => Geom2::Circle(self.p(*c), r * s),
                None => {
                    // The scaled axes are the ellipse's axes (scale comes before rotation).
                    let (u, w) = (self.v(Vec2::new(*r, 0.0)), self.v(Vec2::new(0.0, *r)));
                    let (maj, min) = if u.len() >= w.len() { (u, w) } else { (w, u) };
                    Geom2::Ellipse { c: self.p(*c), major: self.p(*c) + maj, minor: min.len() }
                }
            },
            Geom2::Arc { c, a, b } => match self.uniform() {
                Some(_) if flip => Geom2::Arc { c: self.p(*c), a: self.p(*b), b: self.p(*a) },
                Some(_) => Geom2::Arc { c: self.p(*c), a: self.p(*a), b: self.p(*b) },
                None => {
                    let (r, a0) = ((*a - *c).len(), (*a - *c).angle());
                    let mut sweep = (*b - *c).angle() - a0;
                    while sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    sampled(&|t| *c + Vec2::from_angle(a0 + sweep * t) * r, false)
                }
            },
            Geom2::Ellipse { c, major, minor } => {
                let u = *major - *c;
                let w = u.perp() * (minor / u.len().max(1e-300));
                match self.uniform() {
                    Some(s) => Geom2::Ellipse { c: self.p(*c), major: self.p(*major), minor: minor * s },
                    None => sampled(&|t| *c + u * (t * std::f64::consts::TAU).cos() + w * (t * std::f64::consts::TAU).sin(), true),
                }
            }
            Geom2::Spline { pts, control, degree } => {
                Geom2::Spline { pts: pts.iter().map(|p| self.p(*p)).collect(), control: *control, degree: *degree }
            }
            Geom2::Conic { a, apex, b, rho } => Geom2::Conic { a: self.p(*a), apex: self.p(*apex), b: self.p(*b), rho: *rho },
            Geom2::Text { text, at, height, angle } => {
                let dir = self.v(Vec2::from_angle(*angle));
                let up = self.v(Vec2::from_angle(*angle).perp());
                Geom2::Text { text: text.clone(), at: self.p(*at), height: height * up.len(), angle: dir.angle() }
            }
        });
    }
}

fn polyline(verts: &[(Vec2, f64)], closed: bool, out: &mut Vec<Geom2>) {
    let n = verts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for j in 0..segs {
        let (Some((p0, b)), Some((p1, _))) = (verts.get(j), verts.get((j + 1) % n)) else { continue };
        if p0.dist(*p1) < 1e-12 {
            continue;
        }
        if let Some(g) = bulge_arc(*p0, *p1, *b) {
            out.push(g);
        }
    }
}

/// Write 2D geometry (millimetres) as an R12-style ASCII DXF.
pub fn write_dxf(geom: &[Geom2]) -> String {
    let mut s = String::new();
    let mut w = |c: i32, v: &str| {
        s.push_str(&format!("{c}\n{v}\n"));
    };
    w(0, "SECTION");
    w(2, "HEADER");
    w(9, "$INSUNITS");
    w(70, "4");
    w(0, "ENDSEC");
    w(0, "SECTION");
    w(2, "ENTITIES");
    let f = |x: f64| format!("{x:.9}");
    for g in geom {
        match g {
            Geom2::Point(p) => {
                w(0, "POINT");
                w(8, "0");
                w(10, &f(p.x));
                w(20, &f(p.y));
            }
            Geom2::Line(a, b) => {
                w(0, "LINE");
                w(8, "0");
                w(10, &f(a.x));
                w(20, &f(a.y));
                w(11, &f(b.x));
                w(21, &f(b.y));
            }
            Geom2::Circle(c, r) => {
                w(0, "CIRCLE");
                w(8, "0");
                w(10, &f(c.x));
                w(20, &f(c.y));
                w(40, &f(*r));
            }
            Geom2::Arc { c, a, b } => {
                w(0, "ARC");
                w(8, "0");
                w(10, &f(c.x));
                w(20, &f(c.y));
                w(40, &f(c.dist(*a)));
                w(50, &f((*a - *c).angle().to_degrees()));
                w(51, &f((*b - *c).angle().to_degrees()));
            }
            Geom2::Text { text, at, height, angle } => {
                w(0, "TEXT");
                w(8, "0");
                w(10, &f(at.x));
                w(20, &f(at.y));
                w(40, &f(*height));
                w(50, &f(angle.to_degrees()));
                w(1, &text.replace('\n', " "));
            }
            // Free-form curves are written by the caller as polylines (lines).
            Geom2::Ellipse { .. } | Geom2::Spline { .. } | Geom2::Conic { .. } => {}
        }
    }
    w(0, "ENDSEC");
    w(0, "EOF");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DXF is group codes and values the user did not write, and blocks recurse. None of these
    /// may panic (solidcraft#469).
    #[test]
    fn hostile_dxf_never_panics() {
        let good = write_dxf(&[Geom2::Line(Vec2::ZERO, Vec2::new(10.0, 0.0)), Geom2::Circle(Vec2::new(5.0, 5.0), 2.0)]);
        let mut cases: Vec<String> = vec![
            String::new(),
            "0".into(),
            "0\n".into(),
            "garbage".into(),
            "0\nSECTION\n2\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n4\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n-1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n99999999\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\nnan\n20\ninf\n40\n-1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1e400\n20\n1e400\n40\n1e400\n0\nENDSEC\n0\nEOF\n".into(),
            // SPLINE degrees past any spline's, below 1, and above its control points' count.
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n255\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n-5\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n1e400\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n".into(),
            format!("0\nSECTION\n2\nBLOCKS\n{}0\nENDSEC\n0\nEOF\n", "0\nBLOCK\n2\nb\n0\nENDBLK\n".repeat(300)),
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\na\n1\nINSERT\n2\na\n0\nENDBLK\n0\nENDSEC\n0\nEOF\n".into(),
            format!("0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n".repeat(5000)),
        ];
        for cut in (0..good.len()).step_by(11) {
            cases.push(good.get(..cut).unwrap_or("").to_string());
        }
        for (from, to) in [("0\n", "1\n"), ("SECTION", "SECTI0N"), ("EOF", "EOFF")] {
            cases.push(good.replacen(from, to, 3));
        }
        for c in &cases {
            let r = std::panic::catch_unwind(|| read_dxf(c));
            assert!(r.is_ok(), "panicked on {:?}", c.get(..c.len().min(200)));
        }
    }

    /// A SPLINE keeps its degree up to the 9 a sketch spline (a G4 blend) has (solidcraft#5269); a
    /// degree past that is held to 9, and one that isn't a finite number (which the reader drops)
    /// reads as a missing one, 3.
    #[test]
    fn a_splines_degree_is_read_up_to_nine() {
        let spline = |degree: Option<&str>| {
            let points: String = (0..10).map(|i| format!("10\n{i}\n20\n{}\n", i % 3)).collect();
            let degree = degree.map(|d| format!("71\n{d}\n")).unwrap_or_default();
            format!("0\nSECTION\n2\nENTITIES\n0\nSPLINE\n{degree}{points}0\nENDSEC\n0\nEOF\n")
        };
        for (given, read) in
            [(Some("9"), 9), (Some("8"), 8), (Some("7"), 7), (Some("10"), 9), (Some("0"), 1), (Some("nan"), 3), (Some("inf"), 3), (None, 3)]
        {
            let back = read_dxf(&spline(given)).unwrap();
            assert!(matches!(&back[..], [Geom2::Spline { pts, control: true, degree }] if *degree == read && pts.len() == 10), "{given:?}: {back:?}");
        }
    }

    #[test]
    fn round_trip_and_polyline_bulges() {
        let g = vec![
            Geom2::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
            Geom2::Circle(Vec2::new(5.0, 5.0), 2.0),
            Geom2::Arc { c: Vec2::ZERO, a: Vec2::new(3.0, 0.0), b: Vec2::new(0.0, 3.0) },
        ];
        let back = read_dxf(&write_dxf(&g)).unwrap();
        assert_eq!(back.len(), 3);
        match &back[2] {
            Geom2::Arc { c, a, b } => {
                assert!(c.dist(Vec2::ZERO) < 1e-6 && a.dist(Vec2::new(3.0, 0.0)) < 1e-6 && b.dist(Vec2::new(0.0, 3.0)) < 1e-6);
            }
            x => panic!("{x:?}"),
        }
        // A closed LWPOLYLINE slot: two lines and two half circles (bulge 1), in inches.
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n4\n70\n1\n10\n0\n20\n0\n10\n2\n20\n0\n42\n1\n10\n2\n20\n1\n10\n0\n20\n1\n42\n1\n0\nENDSEC\n0\nEOF\n";
        let g = read_dxf(dxf).unwrap();
        assert_eq!(g.len(), 4, "{g:?}");
        match &g[1] {
            Geom2::Arc { c, a, .. } => {
                assert!(c.dist(Vec2::new(50.8, 12.7)) < 1e-9, "{c:?}");
                assert!((c.dist(*a) - 12.7).abs() < 1e-9);
            }
            x => panic!("{x:?}"),
        }
        assert!(read_dxf("garbage").is_err());
        // A block of a unit square and a circle, inserted twice: rotated 90° at (10, 0), and as a
        // 2 × 1 array scaled 2 × 1 at (0, 20) (the circle becomes an ellipse). Nested once more.
        let dxf = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nSQ\n10\n0\n20\n0\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nCIRCLE\n10\n0\n20\n0\n40\n1\n0\nENDBLK\n0\nBLOCK\n2\nOUTER\n10\n0\n20\n0\n0\nINSERT\n2\nSQ\n10\n5\n20\n5\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nSQ\n10\n10\n20\n0\n50\n90\n0\nINSERT\n2\nSQ\n10\n0\n20\n20\n41\n2\n42\n1\n70\n2\n44\n10\n0\nINSERT\n2\nOUTER\n10\n100\n20\n0\n0\nINSERT\n2\nNOPE\n0\nLINE\n67\n1\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
        let g = read_dxf(dxf).unwrap();
        assert_eq!(g.len(), 8, "{g:?}");
        match &g[0] {
            Geom2::Line(a, b) => assert!(a.dist(Vec2::new(10.0, 0.0)) < 1e-9 && b.dist(Vec2::new(10.0, 1.0)) < 1e-9, "{a:?} {b:?}"),
            x => panic!("{x:?}"),
        }
        match &g[5] {
            Geom2::Ellipse { c, major, minor } => {
                assert!(c.dist(Vec2::new(10.0, 20.0)) < 1e-9 && major.dist(Vec2::new(12.0, 20.0)) < 1e-9 && (minor - 1.0).abs() < 1e-9)
            }
            x => panic!("{x:?}"),
        }
        assert!(matches!(&g[7], Geom2::Circle(c, r) if c.dist(Vec2::new(105.0, 5.0)) < 1e-9 && (*r - 1.0).abs() < 1e-12));
        // A block that inserts itself stops at the depth limit.
        let selfref = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nINSERT\n2\nA\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(read_dxf(selfref).unwrap().len(), 16);
        assert!(read_dxf("AutoCAD Binary DXF\r\n").is_err());
    }

    /// Bounds on the geometry that comes out do not see a block that emits none: six
    /// self-inserts fan out to 6^16 visits for no output. Both shapes must be refused quickly.
    #[test]
    fn empty_blocks_do_not_fan_out() {
        let fanout = format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n{}0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nENDSEC\n0\nEOF\n",
            "0\nINSERT\n2\nA\n".repeat(6)
        );
        assert!(read_dxf(&fanout).is_err(), "a block of six self-inserts must be refused");

        // An empty block placed as a 10,000 by 10,000 array, a thousand times over.
        let array = format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nE\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n",
            "0\nINSERT\n2\nE\n70\n10000\n71\n10000\n".repeat(1000)
        );
        let g = read_dxf(&array).unwrap_or_default();
        assert!(g.is_empty(), "an empty block has no geometry to place");
    }
}
