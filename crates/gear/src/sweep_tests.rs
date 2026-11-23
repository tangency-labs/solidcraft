//! Property sweeps over the whole input space: deterministic pseudo-random specs for every kind,
//! each either refused with a message or planned into outlines the CAD kernel can build.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::geom::{Outline, Region, Seg, V2};
use crate::hub::{Bore, BoreKind, Chamfer, Hardware, Shaft, ShaftEnd};
use crate::involute::BasicRack;
use crate::plan::{Kind, Op, Plan, Spec, Step, plan};

/// SplitMix64: small, deterministic, good enough for sweeps.
pub(crate) struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }
    pub fn log_range(&mut self, a: f64, b: f64) -> f64 {
        (a.ln() + (b.ln() - a.ln()) * self.f()).exp()
    }
    pub fn int(&mut self, a: u32, b: u32) -> u32 {
        a + (self.next() % u64::from(b - a + 1)) as u32
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.f() < p
    }
    /// Tooth counts: small counts (where the trouble is) as often as large ones.
    pub fn teeth(&mut self, min: u32) -> u32 {
        if self.chance(0.5) { self.int(min, 30) } else { self.log_range(f64::from(min), 400.0).round() as u32 }
    }
}

pub(crate) fn random_bore(r: &mut Rng, d: f64) -> Bore {
    let kinds = [BoreKind::None, BoreKind::Round, BoreKind::Keyway, BoreKind::DFlat, BoreKind::Hex, BoreKind::Square];
    let kind = kinds[r.int(0, 5) as usize];
    let diameter = d * r.range(0.02, 0.75);
    Bore {
        kind,
        diameter,
        key_width: if r.chance(0.7) { 0.0 } else { diameter * r.range(0.1, 0.6) },
        key_depth: if r.chance(0.7) { 0.0 } else { diameter * r.range(0.05, 0.4) },
        flat: if r.chance(0.7) { 0.0 } else { diameter * r.range(0.3, 0.99) },
        on_shaft: false,
    }
}

pub(crate) fn random_spec(r: &mut Rng, kind: Kind) -> Spec {
    let mut s = Spec::defaults_for(kind);
    let min = if kind == Kind::Cycloidal { 4 } else { 3 };
    s.module = r.log_range(0.05, 50.0);
    s.teeth = r.teeth(min);
    s.teeth2 = r.teeth(min);
    if kind == Kind::Cycloidal && r.chance(0.2) {
        s.teeth2 = 0;
    }
    if kind == Kind::Planetary {
        let ideal = s.teeth + 2 * s.teeth2;
        s.teeth3 = if r.chance(0.6) {
            ideal
        } else if r.chance(0.5) {
            0
        } else {
            (i64::from(ideal) + i64::from(r.int(0, 12)) - 6).max(3) as u32
        };
        s.planets = r.int(1, 8);
    }
    s.pressure_angle = r.range(10.0, 35.0);
    s.helix = if r.chance(0.3) { 0.0 } else { r.range(-45.0, 45.0) };
    s.shift = if r.chance(0.3) { 0.0 } else { r.range(-1.0, 1.2) };
    s.shift2 = if r.chance(0.3) { 0.0 } else { r.range(-1.0, 1.2) };
    s.backlash = if r.chance(0.5) { 0.0 } else { r.range(0.0, 0.15) * s.module };
    s.rack = if r.chance(0.5) {
        BasicRack::STANDARDS[r.int(0, BasicRack::STANDARDS.len() as u32 - 1) as usize].1
    } else {
        let addendum = r.range(0.6, 1.3);
        BasicRack { addendum, dedendum: addendum + r.range(0.1, 0.6), root_radius: r.range(0.0, 0.45) }
    };
    s.face_width = s.module * r.log_range(0.5, 20.0);
    if kind == Kind::Pair && r.chance(0.4) {
        let hc = s.helix.to_radians().cos();
        let a0 = s.module / hc * f64::from(s.teeth + s.teeth2) / 2.0;
        s.center_distance = a0 * r.range(0.97, 1.05);
    }
    s.shaft_angle = if r.chance(0.4) { 90.0 } else { r.range(10.0, 170.0) };
    s.spiral_angle = r.range(0.0, 45.0);
    s.offset = if r.chance(0.5) { 0.0 } else { s.module * r.range(0.0, 80.0) };
    s.face_start = s.module * r.range(0.0, 20.0);
    s.starts = r.int(1, 6);
    s.worm_diameter = if r.chance(0.5) { 0.0 } else { s.module * r.range(2.0, 20.0) };
    s.worm_length = if r.chance(0.7) { 0.0 } else { s.module * r.range(2.0, 40.0) };
    s.rim = if r.chance(0.6) { 0.0 } else { s.module * r.range(0.3, 6.0) };
    s.rack_teeth = r.int(1, 40);
    s.rack_height = if r.chance(0.6) { 0.0 } else { s.module * r.range(0.5, 6.0) };
    s.pins = r.int(3, 60);
    s.pin_circle = r.log_range(5.0, 300.0);
    s.pin_diameter = s.pin_circle * PI / f64::from(s.pins) * r.range(0.1, 0.8);
    s.eccentricity = s.pin_circle / 2.0 / f64::from(s.pins) * r.range(0.05, 0.95);
    let d = if kind == Kind::CycloidalDrive { s.pin_circle } else { s.module * f64::from(s.teeth) };
    let d2 = if kind == Kind::Worm { s.module * 10.0 } else { s.module * f64::from(s.teeth2.max(3)) };
    s.bore = random_bore(r, d);
    s.bore2 = random_bore(r, d2);
    if r.chance(0.3) {
        s.hub_diameter = s.bore.diameter.max(d * 0.1) * r.range(1.1, 3.0);
        s.hub_length = s.module * r.range(1.0, 10.0);
    }
    if r.chance(0.35) {
        s.holes = r.int(1, 12);
        s.hole_diameter = if r.chance(0.5) { 0.0 } else { d * r.range(0.02, 0.4) };
    }
    // The body's extras (drawn last, so the draws above stay what they were): each on for a
    // part of the specs only, so plain gears are still swept.
    if r.chance(0.6) {
        random_body(r, &mut s, d, d2);
    }
    // Sprockets, pulleys, splines, face gears and elliptical pairs draw their own fields.
    crate::tests_extras::random_extras(r, &mut s);
    s
}

fn random_shaft(r: &mut Rng, d: f64) -> Shaft {
    let ends = [ShaftEnd::Plain, ShaftEnd::DFlat, ShaftEnd::Keyway, ShaftEnd::Hex, ShaftEnd::Cross, ShaftEnd::CentreHole];
    let diameter = d * r.range(0.03, 1.2);
    let stepped = r.chance(0.4);
    Shaft {
        diameter,
        length: d * r.range(0.02, 2.0),
        step_diameter: if stepped { diameter * r.range(0.3, 1.5) } else { 0.0 },
        step_length: if stepped { d * r.range(0.02, 1.0) } else { 0.0 },
        end: ends[r.int(0, 5) as usize],
        end_length: if r.chance(0.5) { 0.0 } else { d * r.range(0.01, 1.5) },
        end_size: if r.chance(0.5) { 0.0 } else { diameter * r.range(0.05, 1.3) },
        chamfer: if r.chance(0.5) { 0.0 } else { diameter * r.range(0.01, 0.8) },
    }
}

fn random_hardware(r: &mut Rng, d: f64) -> Hardware {
    let mut h = Hardware::default();
    if r.chance(0.5) {
        h.set_screw = d * r.range(0.01, 0.3);
        h.set_screws = r.int(0, 4);
    }
    if r.chance(0.4) {
        h.nut_width = d * r.range(0.02, 0.4);
        h.nut_thickness = if r.chance(0.5) { 0.0 } else { d * r.range(0.01, 0.2) };
    }
    if r.chance(0.4) {
        h.insert_diameter = d * r.range(0.02, 0.4);
        h.insert_depth = if r.chance(0.5) { 0.0 } else { d * r.range(0.01, 0.4) };
    }
    if r.chance(0.4) {
        h.clamp_slit = d * r.range(0.005, 0.3);
        h.clamp_screw = if r.chance(0.3) { 0.0 } else { d * r.range(0.01, 0.2) };
    }
    if r.chance(0.5) {
        h.bearing_od = d * r.range(0.1, 1.1);
        h.bearing_width = d * r.range(0.02, 0.6);
        h.bearing_fit = if r.chance(0.5) { 0.0 } else { r.range(0.0, 0.3) };
    }
    h
}

fn random_body(r: &mut Rng, s: &mut Spec, d: f64, d2: f64) {
    let m = s.module;
    if r.chance(0.3) {
        s.shaft_front = random_shaft(r, d);
    }
    if r.chance(0.3) {
        s.shaft_back = random_shaft(r, d);
    }
    if r.chance(0.3) {
        s.shaft2_front = random_shaft(r, d2);
    }
    if r.chance(0.3) {
        s.shaft2_back = random_shaft(r, d2);
    }
    if r.chance(0.3) {
        s.tooth_chamfer_front = Chamfer { size: m * r.range(0.05, 4.0), angle: r.range(8.0, 82.0) };
    }
    if r.chance(0.3) {
        s.tooth_chamfer_back = Chamfer { size: m * r.range(0.05, 4.0), angle: r.range(8.0, 82.0) };
    }
    if r.chance(0.3) {
        s.bore_chamfer = m * r.range(0.02, 2.0);
    }
    if r.chance(0.3) {
        s.hub_chamfer = m * r.range(0.02, 2.0);
    }
    if r.chance(0.3) {
        s.tip_round = r.range(0.0, 0.5);
    }
    if r.chance(0.25) {
        s.xy_compensation = m * r.range(0.0, 0.12);
    }
    if r.chance(0.25) {
        s.elephant_foot = m * r.range(0.0, 0.12);
        s.elephant_foot_height = if r.chance(0.5) { 0.0 } else { m * r.range(0.05, 3.0) };
    }
    if r.chance(0.2) {
        s.clearance = r.range(0.05, 0.6);
    }
    if r.chance(0.2) {
        s.tip_shortening = m * r.range(0.0, 0.4);
    }
    if r.chance(0.15) {
        s.tip_diameter = d * r.range(0.85, 1.2);
    }
    if r.chance(0.15) {
        s.tip_diameter2 = d2 * r.range(0.85, 1.2);
    }
    if r.chance(0.25) {
        s.tip_relief = m * r.range(0.0, 0.08);
        s.tip_relief_height = if r.chance(0.5) { 0.0 } else { m * r.range(0.05, 1.5) };
    }
    if r.chance(0.2) {
        s.crowning = m * r.range(0.0, 0.08);
    }
    if r.chance(0.2) {
        s.backlash2 = Some(m * r.range(0.0, 0.15));
    }
    if r.chance(0.2) {
        s.face_width2 = s.face_width * r.range(0.4, 2.5);
        s.face_offset2 = s.face_width * r.range(-0.8, 0.8);
    }
    if r.chance(0.2) {
        s.nozzle = r.range(0.2, 0.8);
    }
    if r.chance(0.35) {
        s.hardware = random_hardware(r, d);
    }
    if r.chance(0.35) {
        s.hardware2 = random_hardware(r, d2);
    }
    if (s.hardware != Hardware::default() || s.hardware2 != Hardware::default()) && s.hub_diameter == 0.0 && r.chance(0.7) {
        s.hub_diameter = s.bore.diameter.max(d * 0.1) * r.range(1.1, 3.0);
        s.hub_length = s.module * r.range(1.0, 10.0);
    }
}

// ---------------------------------------------------------------------------------------------
// Geometry checks.

/// The outline as a closed polyline (last point = first), splines as the kernel fits them.
fn polyline(o: &Outline, tol: f64) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    for s in &o.segs {
        let mut pts = Vec::new();
        match s {
            Seg::Spline(p) => crate::geom::natural_spline(p, 4, &mut pts),
            other => other.sample(tol, &mut pts),
        }
        if !out.is_empty() && !pts.is_empty() {
            pts.remove(0);
        }
        out.extend(pts);
    }
    out
}

fn bbox(pts: &[V2]) -> (V2, V2) {
    let mut lo = V2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = V2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        lo = V2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = V2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

fn orient(a: V2, b: V2, c: V2) -> f64 {
    (b - a).cross(c - a)
}

fn point_seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// Whether two segments cross or come within `eps` of each other.
fn segs_touch(a: V2, b: V2, c: V2, d: V2, eps: f64) -> bool {
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    point_seg_dist(c, a, b) < eps || point_seg_dist(d, a, b) < eps || point_seg_dist(a, c, d) < eps || point_seg_dist(b, c, d) < eps
}

/// The first pair of crossing edges among the loops (each closed: last point = first), edges
/// next to each other in a loop excepted. Returns the crossing point's neighbourhood.
fn first_crossing(loops: &[&[V2]], eps: f64) -> Option<(usize, V2)> {
    // Edges: (loop, index in loop, loop edge count, a, b).
    let mut edges: Vec<(usize, usize, usize, V2, V2)> = Vec::new();
    for (l, pts) in loops.iter().enumerate() {
        let mut clean: Vec<V2> = Vec::new();
        for p in pts.iter() {
            if clean.last().is_none_or(|q: &V2| q.dist(*p) > eps * 1e-3) {
                clean.push(*p);
            }
        }
        let n = clean.len().saturating_sub(1);
        for i in 0..n {
            edges.push((l, i, n, clean[i], clean[i + 1]));
        }
    }
    if edges.len() < 3 {
        return None;
    }
    let all: Vec<V2> = edges.iter().flat_map(|e| [e.3, e.4]).collect();
    let (lo, hi) = bbox(&all);
    let size = (hi.x - lo.x).max(hi.y - lo.y).max(1e-12);
    let cells = ((edges.len() as f64).sqrt() * 1.5).clamp(1.0, 1500.0) as usize;
    let cell = size / cells as f64;
    let key = |v: f64, o: f64| (((v - o) / cell).floor().max(0.0) as usize).min(cells - 1);
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cells * cells];
    for (k, e) in edges.iter().enumerate() {
        let (x0, x1) = (key(e.3.x.min(e.4.x) - eps, lo.x), key(e.3.x.max(e.4.x) + eps, lo.x));
        let (y0, y1) = (key(e.3.y.min(e.4.y) - eps, lo.y), key(e.3.y.max(e.4.y) + eps, lo.y));
        for gx in x0..=x1 {
            for gy in y0..=y1 {
                grid[gy * cells + gx].push(k as u32);
            }
        }
    }
    for c in &grid {
        for (ii, &i) in c.iter().enumerate() {
            for &j in &c[ii + 1..] {
                let (e, f) = (&edges[i as usize], &edges[j as usize]);
                if e.0 == f.0 {
                    let n = e.2;
                    let d = e.1.abs_diff(f.1);
                    if d <= 1 || d == n - 1 {
                        continue;
                    }
                }
                if segs_touch(e.3, e.4, f.3, f.4, eps) {
                    return Some((e.0.max(f.0), e.3));
                }
            }
        }
    }
    None
}

fn inside(p: V2, poly: &[V2]) -> bool {
    let mut c = false;
    // Closed whatever the last point is (a sampled circle's last point misses its first by a hair).
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

fn signed_area(p: &[V2]) -> f64 {
    p.windows(2).map(|w| w[0].cross(w[1])).sum::<f64>() * 0.5
}

/// An outline the kernel can sketch: finite, closed, no degenerate pieces, smooth splines,
/// counter-clockwise and not crossing itself. Returns its polyline.
pub(crate) fn check_outline(o: &Outline) -> Result<Vec<V2>, String> {
    if o.segs.is_empty() {
        return Err("empty outline".into());
    }
    let mut pts: Vec<V2> = Vec::new();
    for s in &o.segs {
        match s {
            Seg::Line(a, b) => pts.extend([*a, *b]),
            Seg::Arc { c, a, b, .. } => pts.extend([*c, *a, *b]),
            Seg::Spline(p) => pts.extend(p.iter().copied()),
            Seg::Circle(c, r) => {
                if !r.is_finite() || *r <= 0.0 {
                    return Err(format!("circle of radius {r}"));
                }
                pts.push(*c)
            }
        }
    }
    if pts.iter().any(|p| !p.is_finite()) {
        return Err("non-finite point".into());
    }
    if o.segs.len() == 1
        && let Seg::Circle(..) = o.segs[0]
    {
        return Ok(polyline(o, 1e-3));
    }
    let (lo, hi) = bbox(&pts);
    let scale = (hi - lo).len().max(1e-12);
    let tiny = 1e-9 * scale;
    for (i, s) in o.segs.iter().enumerate() {
        let next = &o.segs[(i + 1) % o.segs.len()];
        if s.end().dist(next.start()) > tiny {
            return Err(format!("gap of {:e} after piece {i}", s.end().dist(next.start())));
        }
        match s {
            Seg::Circle(..) => return Err("a circle inside a multi-piece outline".into()),
            Seg::Line(a, b) => {
                if a.dist(*b) <= 1e-7 * scale {
                    return Err(format!("zero-length line {i}"));
                }
            }
            Seg::Arc { c, a, b, .. } => {
                let (ra, rb) = (c.dist(*a), c.dist(*b));
                if (ra - rb).abs() > 1e-6 * ra.max(1e-12) || a.dist(*b) <= 1e-7 * scale || ra <= 0.0 {
                    return Err(format!("bad arc {i}: radii {ra} {rb}, chord {}", a.dist(*b)));
                }
            }
            Seg::Spline(p) => {
                if p.len() < 2 {
                    return Err(format!("spline {i} with {} points", p.len()));
                }
                for (j, w) in p.windows(2).enumerate() {
                    if w[0].dist(w[1]) < 1e-6 * scale {
                        return Err(format!("near-duplicate spline points in spline {i} at {j} ({:e} apart, outline {scale:.4})", w[0].dist(w[1])));
                    }
                }
                for (j, w) in p.windows(3).enumerate() {
                    let (u, v) = (w[1] - w[0], w[2] - w[1]);
                    let turn = u.cross(v).atan2(u.dot(v)).abs().to_degrees();
                    if turn > 60.0 {
                        return Err(format!("kink of {turn:.1}° in spline {i} at point {} of {}", j + 1, p.len()));
                    }
                }
            }
        }
    }
    let poly = polyline(o, scale * 1e-4);
    let area = signed_area(&poly);
    if !(area > 0.0) {
        return Err(format!("not counter-clockwise (area {area})"));
    }
    if let Some((_, at)) = first_crossing(&[&poly], 1e-9 * scale) {
        return Err(format!("crosses itself near ({:.5}, {:.5})", at.x, at.y));
    }
    Ok(poly)
}

/// A region: every loop valid, no loops crossing, outers apart, each hole inside an outer.
pub(crate) fn check_region(r: &Region) -> Result<Vec<Vec<V2>>, String> {
    if r.outer.is_empty() {
        return Err("region without an outline".into());
    }
    let mut outer = Vec::new();
    for (i, o) in r.outer.iter().enumerate() {
        outer.push(check_outline(o).map_err(|e| format!("outline {i}: {e}"))?);
    }
    let mut holes = Vec::new();
    for (i, o) in r.holes.iter().enumerate() {
        holes.push(check_outline(o).map_err(|e| format!("hole {i}: {e}"))?);
    }
    let loops: Vec<&[V2]> = outer.iter().chain(holes.iter()).map(|v| v.as_slice()).collect();
    if loops.len() > 1 {
        let all: Vec<V2> = loops.iter().flat_map(|l| l.iter().copied()).collect();
        let (lo, hi) = bbox(&all);
        if let Some((l, at)) = first_crossing(&loops, 1e-9 * (hi - lo).len()) {
            return Err(format!("loop {l} touches another near ({:.4}, {:.4})", at.x, at.y));
        }
    }
    for (i, a) in outer.iter().enumerate() {
        for (j, b) in outer.iter().enumerate() {
            if i != j && inside(a[0], b) {
                return Err(format!("outline {i} inside outline {j}"));
            }
        }
    }
    for (i, h) in holes.iter().enumerate() {
        if !outer.iter().any(|o| inside(h[0], o)) {
            return Err(format!("hole {i} is outside the outline"));
        }
        for (j, g) in holes.iter().enumerate() {
            if i != j && inside(h[0], g) {
                return Err(format!("hole {i} inside hole {j}"));
            }
        }
    }
    let _ = outer.len();
    Ok(outer.into_iter().chain(holes).collect())
}

fn check_polygon(p: &[V2], what: &str, radial: bool) -> Result<(), String> {
    if p.len() < 3 || p.iter().any(|q| !q.is_finite()) {
        return Err(format!("{what}: bad polygon"));
    }
    if radial && p.iter().any(|q| q.x <= 0.0) {
        return Err(format!("{what}: reaches the axis ({:?})", p.iter().map(|q| q.x).fold(f64::INFINITY, f64::min)));
    }
    let mut closed = p.to_vec();
    closed.push(p[0]);
    let (lo, hi) = bbox(p);
    if signed_area(&closed).abs() < 1e-12 * (hi - lo).len().powi(2) {
        return Err(format!("{what}: no area"));
    }
    if first_crossing(&[&closed], 1e-9 * (hi - lo).len()).is_some() {
        return Err(format!("{what}: crosses itself"));
    }
    Ok(())
}

fn same_shape(a: &Outline, b: &Outline) -> bool {
    a.segs.len() == b.segs.len() && a.segs.iter().zip(&b.segs).all(|(x, y)| std::mem::discriminant(x) == std::mem::discriminant(y))
}

/// Every problem with a plan, or none.
pub(crate) fn check_plan(p: &Plan) -> Result<(), String> {
    for f in &p.facts {
        if !f.value.is_finite() {
            return Err(format!("fact '{}' is {}", f.label, f.value));
        }
    }
    for f in &p.facts {
        if let Some(pre) = f.label.strip_suffix("tip diameter")
            && let Some(root) = p.facts.iter().find(|g| g.label == format!("{pre}root diameter"))
            && f.value <= root.value
        {
            return Err(format!("{}: tip Ø{} not above root Ø{}", f.label, f.value, root.value));
        }
    }
    for b in &p.bodies {
        let fr = &b.frame;
        if fr.origin.iter().chain(&fr.x).chain(&fr.z).any(|v| !v.is_finite()) {
            return Err(format!("{}: frame not finite", b.name));
        }
        let mut base: Option<Vec<V2>> = None;
        // The heights the toothed stack (the base and what is joined to it tooth for tooth) runs
        // between: a cut is held to the base's outline only where it is in the teeth.
        let mut base_z = (f64::INFINITY, f64::NEG_INFINITY);
        for (k, st) in b.steps.iter().enumerate() {
            let at = format!("{} step {k}", b.name);
            match st {
                Step::Prism { region, z0, height, twist, op } => {
                    if !(z0.is_finite() && height.is_finite() && *height > 0.0 && twist.is_finite()) {
                        return Err(format!("{at}: prism z0 {z0} height {height} twist {twist}"));
                    }
                    let loops = check_region(region).map_err(|e| format!("{at}: {e}"))?;
                    if k == 0 && region.outer.len() == 1 {
                        base = Some(loops[0].clone());
                    }
                    if *op != Op::Cut
                        && *op != Op::Intersect
                        && region.outer.len() == 1
                        && region.outer[0].segs.len() > 1
                        && (k == 0 || base.is_some())
                    {
                        base_z = (base_z.0.min(*z0), base_z.1.max(z0 + height));
                    }
                    let in_teeth = z0 + height > base_z.0 + 1e-9 && *z0 < base_z.1 - 1e-9;
                    if *op == Op::Cut
                        && in_teeth
                        && let Some(base) = &base
                    {
                        let (lo, hi) = bbox(base);
                        let eps = 1e-9 * (hi - lo).len();
                        for (i, l) in loops.iter().enumerate() {
                            if !l.iter().step_by((l.len() / 16).max(1)).all(|q| inside(*q, base)) || first_crossing(&[base, l], eps).is_some() {
                                return Err(format!("{at}: cut outline {i} is not inside the body's outline"));
                            }
                        }
                    }
                }
                Step::Loft { sections, .. } => {
                    if sections.len() < 2 {
                        return Err(format!("{at}: loft with {} sections", sections.len()));
                    }
                    for (i, (o, h)) in sections.iter().enumerate() {
                        if !h.is_finite() {
                            return Err(format!("{at}: section height {h}"));
                        }
                        let l = check_outline(o).map_err(|e| format!("{at}: section {i}: {e}"))?;
                        if i == 0 {
                            base = Some(l);
                        }
                        base_z = (base_z.0.min(*h), base_z.1.max(*h));
                        if !same_shape(o, &sections[0].0) {
                            return Err(format!("{at}: loft sections don't match piece for piece"));
                        }
                    }
                }
                Step::Taper { outline, z0, height, twist, scale, .. } => {
                    if !(z0.is_finite() && height.is_finite() && *height > 0.0 && twist.is_finite() && scale.is_finite() && *scale > 0.0) {
                        return Err(format!("{at}: taper z0 {z0} height {height} twist {twist} scale {scale}"));
                    }
                    base = Some(check_outline(outline).map_err(|e| format!("{at}: {e}"))?);
                }
                Step::RingTaper { outline, rim, z0, height, twist, scale, .. } => {
                    if !(z0.is_finite()
                        && height.is_finite()
                        && *height > 0.0
                        && twist.is_finite()
                        && scale.is_finite()
                        && *scale > 0.0
                        && rim.is_finite())
                    {
                        return Err(format!("{at}: ring taper z0 {z0} height {height} twist {twist} scale {scale} rim {rim}"));
                    }
                    check_outline(outline).map_err(|e| format!("{at}: {e}"))?;
                    // The rim clears the teeth all the way up.
                    if *rim <= outline.max_radius() * scale.max(1.0) {
                        return Err(format!("{at}: the rim (r {rim}) doesn't clear the teeth (r {})", outline.max_radius()));
                    }
                }
                Step::Revolve { profile, op } => {
                    check_polygon(profile, &at, false)?;
                    // A chamfer's triangle stays on its side of the axis.
                    // (A pocket turned about its axis, a differential's, closes on it.)
                    if *op == Op::Cut && profile.iter().any(|q| q.x < 0.0) {
                        return Err(format!("{at}: a revolved cut reaches the axis"));
                    }
                }
                Step::Placed { frame, step } => {
                    let dot = |a: &[f64; 3], b: &[f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                    let square = (dot(&frame.x, &frame.x) - 1.0).abs() < 1e-9
                        && (dot(&frame.z, &frame.z) - 1.0).abs() < 1e-9
                        && dot(&frame.x, &frame.z).abs() < 1e-9;
                    if !square || frame.origin.iter().any(|v| !v.is_finite()) {
                        return Err(format!("{at}: placed in a frame that is not one"));
                    }
                    match step.as_ref() {
                        Step::Prism { region, z0, height, twist, op: Op::Cut } => {
                            if !(z0.is_finite() && height.is_finite() && *height > 0.0 && *twist == 0.0) {
                                return Err(format!("{at}: placed prism z0 {z0} height {height}"));
                            }
                            check_region(region).map_err(|e| format!("{at}: {e}"))?;
                        }
                        Step::Placed { .. } => return Err(format!("{at}: a placed step in a placed step")),
                        // (A differential's pinion pins and an integral ring's teeth: any step, in its frame.)
                        inner => {
                            let mut one = Plan::default();
                            one.bodies =
                                vec![crate::plan::Body { name: at.clone(), frame: crate::plan::Frame::at([0.0; 3]), steps: vec![inner.clone()] }];
                            check_plan(&one)?;
                        }
                    }
                }
                Step::Coil { section, pitch, z0, height, .. } => {
                    if !(pitch.is_finite() && *pitch > 0.0 && z0.is_finite() && height.is_finite() && *height > 0.0) {
                        return Err(format!("{at}: coil pitch {pitch} height {height}"));
                    }
                    check_polygon(section, &at, true)?;
                }
            }
        }
    }
    for g in &p.preview {
        let ok = g.region.outer.iter().chain(&g.region.holes).all(|o| o.polyline(1.0).iter().all(|q| q.is_finite()));
        if !ok || !g.center.is_finite() || !g.speed.is_finite() || !g.pitch_radius.is_finite() {
            return Err(format!("preview '{}' not finite", g.name));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The sweeps.

pub(crate) fn sweep(kinds: &[Kind], per_kind: usize, seed: u64) {
    let mut failures: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut refusals: BTreeMap<String, usize> = BTreeMap::new();
    let mut built = 0usize;
    let mut slowest = (0.0, String::new());
    for &kind in kinds {
        let mut r = Rng(seed ^ (kind as u64).wrapping_mul(0x1234_5678_9ABC));
        for i in 0..per_kind {
            let s = random_spec(&mut r, kind);
            let t0 = std::time::Instant::now();
            let res = catch_unwind(AssertUnwindSafe(|| plan(&s)));
            let dt = t0.elapsed().as_secs_f64();
            if dt > slowest.0 {
                slowest = (dt, format!("{kind:?} #{i} z{} z{} z{}", s.teeth, s.teeth2, s.teeth3));
            }
            let problem = match res {
                Err(e) => {
                    let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                    Some(format!("panic: {msg}"))
                }
                Ok(Err(e)) => {
                    assert!(!e.trim().is_empty(), "empty refusal for {s:?}");
                    let key: String = format!("{kind:?}: {}", e.split(|c: char| c.is_ascii_digit()).next().unwrap_or(&e));
                    *refusals.entry(key).or_default() += 1;
                    None
                }
                Ok(Ok(p)) => {
                    built += 1;
                    check_plan(&p).err()
                }
            };
            if let Some(m) = problem {
                // Group by the message with names and numbers taken out.
                let tail = m.split_once(" step ").map(|(_, t)| t).unwrap_or(&m);
                let mut generic = String::new();
                for c in tail.split('(').next().unwrap_or(tail).chars() {
                    if c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' && generic.ends_with('#') {
                        if !generic.ends_with('#') {
                            generic.push('#');
                        }
                    } else {
                        generic.push(c);
                    }
                }
                let key = format!("{kind:?}: {}", generic.trim());
                let e = failures.entry(key).or_insert((0, format!("#{i} {m}\n    {s:?}")));
                e.0 += 1;
            }
        }
    }
    eprintln!("built {built} plans; slowest plan {:.1} ms ({})", slowest.0 * 1e3, slowest.1);
    for (k, n) in &refusals {
        eprintln!("refused {n:5}× {k}");
    }
    if !failures.is_empty() {
        let mut msg = String::new();
        for (k, (n, ex)) in &failures {
            msg += &format!("\n{n:5}× {k}\n    e.g. {ex}\n");
        }
        if let Ok(path) = std::env::var("GEAR_SWEEP_LOG") {
            let _ = std::fs::write(format!("{path}-{seed}.txt"), &msg);
        }
        panic!("{} kinds of failure:{msg}", failures.len());
    }
}

#[test]
fn sweep_parallel_axis_kinds() {
    sweep(&[Kind::Spur, Kind::Helical, Kind::Herringbone, Kind::Internal, Kind::Rack, Kind::Pair, Kind::Planetary], 400, 1);
}

#[test]
fn sweep_bevel_and_crossed_kinds() {
    sweep(&[Kind::Bevel, Kind::SpiralBevel, Kind::Hypoid, Kind::CrossedHelical, Kind::Worm], 400, 2);
}

#[test]
fn sweep_cycloidal_kinds() {
    sweep(&[Kind::Cycloidal, Kind::CycloidalDrive], 600, 3);
}

/// `plan` stays interactive for big gears: under 50 ms for 400 teeth of any kind.
#[test]
fn big_gears_plan_quickly() {
    for kind in Kind::ALL.iter().copied() {
        let mut s = Spec::defaults_for(kind);
        s.teeth = 400;
        s.teeth2 = 400;
        s.teeth3 = 0;
        s.pins = 200;
        s.pin_circle = 600.0;
        s.pin_diameter = 4.0;
        s.eccentricity = 0.8;
        s.face_width = 20.0;
        s.rack_teeth = 400;
        if kind == Kind::Planetary {
            s.teeth2 = 200;
            s.planets = 6;
        }
        // The largest these types take.
        match kind {
            Kind::Sprocket => (s.teeth, s.teeth2) = (250, 250),
            Kind::FaceGear => (s.teeth, s.teeth2) = (100, 400),
            Kind::Elliptical => s.teeth = 200,
            // The kernel's loft takes so many curves: a ring gear of about 100 teeth lofts.
            Kind::Hypoid => (s.teeth, s.teeth2) = (25, 100),
            _ => {}
        }
        // A differential plans several sets (gears, case, drive): seconds, not milliseconds.
        if kind == Kind::Differential {
            continue;
        }
        let mut best = f64::INFINITY;
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let p = plan(&s).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
            assert!(!p.bodies.is_empty());
        }
        let points: usize = match plan(&s).unwrap().bodies[0].steps.first() {
            Some(Step::Prism { region, .. }) => {
                region.outer.iter().chain(&region.holes).flat_map(|o| &o.segs).map(|g| if let Seg::Spline(p) = g { p.len() } else { 2 }).sum()
            }
            _ => 0,
        };
        eprintln!("timing {kind:?}: {best:.2} ms, first sketch {points} points");
        crate::test_util::assert_quick(&format!("{kind:?}"), best, 50.0);
    }
}

/// The slowest shapes the sweeps found (a bevel wheel near a crown gear, a fine herringbone).
#[test]
fn awkward_gears_plan_quickly() {
    let mut bevel = Spec::defaults_for(Kind::Bevel);
    (bevel.teeth, bevel.teeth2) = (15, 100);
    let mut herringbone = Spec::defaults_for(Kind::Herringbone);
    (herringbone.teeth, herringbone.shift, herringbone.pressure_angle) = (26, -0.3, 14.5);
    let mut planetary = Spec::defaults_for(Kind::Planetary);
    (planetary.teeth, planetary.teeth2, planetary.teeth3, planetary.planets) = (337, 373, 1083, 8);
    for s in [bevel, herringbone, planetary] {
        let mut best = f64::INFINITY;
        for _ in 0..5 {
            let t = std::time::Instant::now();
            plan(&s).unwrap_or_else(|e| panic!("{:?}: {e}", s.kind));
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
        }
        eprintln!("timing awkward {:?}: {best:.2} ms", s.kind);
        assert!(best < 50.0, "{:?} took {best:.1} ms", s.kind);
    }
}

/// Impossible inputs are refused with a message that names the value and the way out.
#[test]
fn impossible_inputs_are_refused_clearly() {
    let refusal = |edit: &dyn Fn(&mut Spec), kind: Kind| -> String {
        let mut s = Spec::defaults_for(kind);
        edit(&mut s);
        plan(&s).err().unwrap_or_else(|| panic!("{kind:?} should be refused"))
    };
    let has = |e: String, words: &[&str]| assert!(words.iter().all(|w| e.contains(w)), "{e}");
    has(refusal(&|s| (s.pressure_angle, s.rack.dedendum) = (35.0, 1.25), Kind::Spur), &["pointed", "dedendum"]);
    has(refusal(&|s| (s.teeth, s.shift) = (5, -1.0), Kind::Spur), &["5 teeth", "profile shift"]);
    has(refusal(&|s| (s.teeth, s.shift) = (3, -0.9), Kind::Spur), &["root circle", "more teeth"]);
    has(refusal(&|s| s.bore = Bore { kind: BoreKind::Round, diameter: 36.0, ..Bore::default() }, Kind::Spur), &["bore", "tooth roots"]);
    has(refusal(&|s| s.backlash = 2.0, Kind::Spur), &["backlash"]);
    has(refusal(&|s| s.shift = f64::NAN, Kind::Spur), &["profile shift"]);
    has(refusal(&|s| s.rack_height = 1.0, Kind::Rack), &["rack height"]);
    has(refusal(&|s| (s.shaft_angle, s.teeth, s.teeth2) = (150.0, 40, 12), Kind::Bevel), &["cone angle"]);
    has(refusal(&|s| (s.shaft_angle, s.helix) = (90.0, 80.0), Kind::CrossedHelical), &["helix angle"]);
    has(refusal(&|s| s.worm_diameter = 3.0, Kind::Worm), &["worm", "diameter"]);
    has(refusal(&|s| (s.starts, s.worm_diameter) = (6, 9.0), Kind::Worm), &["lead angle"]);
    has(refusal(&|s| s.teeth3 = 19, Kind::Planetary), &["ring teeth"]);
    has(refusal(&|s| s.eccentricity = 2.6, Kind::CycloidalDrive), &["eccentricity"]);
    has(refusal(&|s| s.pin_diameter = 16.0, Kind::CycloidalDrive), &["pins overlap"]);
    has(refusal(&|s| (s.pin_diameter, s.eccentricity) = (14.0, 2.3), Kind::CycloidalDrive), &["too big"]);
    has(refusal(&|s| (s.teeth, s.shift) = (3, -1.0), Kind::Internal), &["internal teeth"]);
    // Lightening holes that don't fit are left out with a note rather than overlapping.
    let mut s = Spec::defaults_for(Kind::Spur);
    (s.teeth, s.holes, s.hole_diameter) = (60, 12, 40.0);
    let p = plan(&s).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("left out")), "{:?}", p.warnings);
    check_plan(&p).unwrap();
    s.hole_diameter = 0.0;
    let p = plan(&s).unwrap();
    assert!(p.warnings.is_empty() && p.bodies[0].steps.len() == 2, "{:?}", p.warnings);
    check_plan(&p).unwrap();
}
