//! Roller chain sprockets to ISO 606 (whose A series is ANSI B29.1): the chain sizes, the tooth
//! form (roller seating arc, flank arcs and tip, each at the middle of the standard's range), the
//! axial tooth width and side chamfer, and the measurements a drawing gives.

use std::f64::consts::{PI, TAU};

use crate::geom::{Outline, Seg, V2};

/// A roller chain size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    /// The id used in parameters ("08b", "ansi40").
    pub id: &'static str,
    /// What people call it ("08B", "ANSI 40").
    pub label: &'static str,
    /// Pitch p (mm).
    pub pitch: f64,
    /// Roller diameter d1 (mm; the bush for rollerless ANSI 25 and 35).
    pub roller: f64,
    /// Width between the inner plates b1 (mm).
    pub inner_width: f64,
    /// Transverse pitch pt between strands of a duplex or triplex chain (mm).
    pub strand_pitch: f64,
}

const fn chain(id: &'static str, label: &'static str, pitch: f64, roller: f64, inner_width: f64, strand_pitch: f64) -> Chain {
    Chain { id, label, pitch, roller, inner_width, strand_pitch }
}

/// ISO 606 B series (European) and A series (ANSI B29.1) chains: pitch, roller Ø, inner width,
/// transverse pitch.
pub const CHAINS: &[Chain] = &[
    chain("04b", "04B", 6.0, 4.0, 2.8, 5.5),
    chain("05b", "05B", 8.0, 5.0, 3.0, 5.64),
    chain("06b", "06B", 9.525, 6.35, 5.72, 10.24),
    chain("08b", "08B", 12.7, 8.51, 7.75, 13.92),
    chain("10b", "10B", 15.875, 10.16, 9.65, 16.59),
    chain("12b", "12B", 19.05, 12.07, 11.68, 19.46),
    chain("16b", "16B", 25.4, 15.88, 17.02, 31.88),
    chain("20b", "20B", 31.75, 19.05, 19.56, 36.45),
    chain("24b", "24B", 38.1, 25.4, 25.4, 48.36),
    chain("ansi25", "ANSI 25 (04C)", 6.35, 3.3, 3.18, 6.4),
    chain("ansi35", "ANSI 35 (06C)", 9.525, 5.08, 4.77, 10.13),
    chain("ansi40", "ANSI 40 (08A)", 12.7, 7.92, 7.85, 14.38),
    chain("ansi50", "ANSI 50 (10A)", 15.875, 10.16, 9.4, 18.11),
    chain("ansi60", "ANSI 60 (12A)", 19.05, 11.91, 12.57, 22.78),
    chain("ansi80", "ANSI 80 (16A)", 25.4, 15.88, 15.75, 29.29),
    chain("ansi100", "ANSI 100 (20A)", 31.75, 19.05, 18.9, 35.76),
    chain("ansi120", "ANSI 120 (24A)", 38.1, 22.23, 25.22, 45.44),
];

/// The id meaning "the chain's sizes are given by hand".
pub const CUSTOM: &str = "custom";

/// The chain size with this id.
pub fn chain_by_id(id: &str) -> Option<&'static Chain> {
    CHAINS.iter().find(|c| c.id == id)
}

/// A sprocket's tooth form in its transverse plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprocket {
    pub teeth: u32,
    pub pitch: f64,
    pub roller: f64,
}

/// The transverse outline and what became of the tip.
#[derive(Clone, Debug)]
pub struct SprocketTeeth {
    pub outline: Outline,
    /// The tip radius used (lowered when the teeth would come to a point).
    pub ra: f64,
    pub pointed: bool,
    /// The smallest radius of the outline (the bottom of the roller seats).
    pub r_min: f64,
}

impl Sprocket {
    fn z(&self) -> f64 {
        f64::from(self.teeth.max(3))
    }
    /// Pitch diameter d = p / sin(180°/z).
    pub fn pitch_diameter(&self) -> f64 {
        self.pitch / (PI / self.z()).sin()
    }
    /// Roller seating radius ri: the middle of 0.505·d1 … 0.505·d1 + 0.069·∛d1.
    pub fn seating_radius(&self) -> f64 {
        0.505 * self.roller + 0.0345 * self.roller.cbrt()
    }
    /// Roller seating angle α: the middle of 120° − 90°/z … 140° − 90°/z (radians).
    pub fn seating_angle(&self) -> f64 {
        (130.0 - 90.0 / self.z()).to_radians()
    }
    /// Tooth flank radius re: the middle of 0.12·d1·(z + 2) … 0.008·d1·(z² + 180).
    pub fn flank_radius(&self) -> f64 {
        let z = self.z();
        0.5 * (0.12 * self.roller * (z + 2.0) + 0.008 * self.roller * (z * z + 180.0))
    }
    /// The tip diameter's range: d + p·(1 − 1.6/z) − d1 … d + 1.25·p − d1.
    pub fn tip_diameter_range(&self) -> (f64, f64) {
        let d = self.pitch_diameter();
        (d + self.pitch * (1.0 - 1.6 / self.z()) - self.roller, d + 1.25 * self.pitch - self.roller)
    }
    /// The tip diameter used: the middle of the range.
    pub fn tip_diameter(&self) -> f64 {
        let (lo, hi) = self.tip_diameter_range();
        0.5 * (lo + hi)
    }
    /// Root diameter df = d − d1.
    pub fn root_diameter(&self) -> f64 {
        self.pitch_diameter() - self.roller
    }
    /// The caliper (measuring) diameter: the root diameter for even teeth, d·cos(90°/z) − d1 for
    /// odd (measured from one root to the root nearest opposite).
    pub fn caliper_diameter(&self) -> f64 {
        if self.teeth.is_multiple_of(2) { self.root_diameter() } else { self.pitch_diameter() * (PI / (2.0 * self.z())).cos() - self.roller }
    }
    /// The largest hub (shroud) diameter that clears the inner plates: p·cot(180°/z) − 1.04·h2 −
    /// 0.76, with the plate depth h2 taken as 0.9·p (a typical value; the standard gives it per
    /// chain).
    pub fn shroud_diameter(&self) -> f64 {
        self.pitch / (PI / self.z()).tan() - 1.04 * 0.9 * self.pitch - 0.76
    }
    /// The space's half-shape, centred on +x: the roller centre, the end of the seat (upper side),
    /// the flank arc's centre.
    fn seat(&self) -> (V2, V2, V2) {
        let o = V2::new(self.pitch_diameter() / 2.0, 0.0);
        let half = self.seating_angle() / 2.0;
        let u = V2::new(-half.cos(), half.sin());
        let ri = self.seating_radius();
        (o, o + u * ri, o + u * (ri + self.flank_radius()))
    }
    /// Where the upper flank meets the circle of radius `ra` (the crossing nearest the seat).
    fn flank_meets(&self, ra: f64) -> Option<V2> {
        let (_, e, c) = self.seat();
        let re = self.flank_radius();
        let dc = c.len();
        if dc <= 0.0 {
            return None;
        }
        let cos = (dc * dc + ra * ra - re * re) / (2.0 * dc * ra);
        if !(-1.0..=1.0).contains(&cos) {
            return None;
        }
        let phi = cos.acos();
        let (a, b) = (V2::polar(ra, c.angle() + phi), V2::polar(ra, c.angle() - phi));
        Some(if a.dist(e) < b.dist(e) { a } else { b })
    }
    /// Half the tooth's angle at radius `ra` (what is left of the half pitch past the flank).
    fn tooth_half_at(&self, ra: f64) -> Option<f64> {
        self.flank_meets(ra).map(|t| PI / self.z() - t.angle())
    }
    /// The teeth, counter-clockwise, a roller seat (tooth space) centred at `phase`, tip radius
    /// `tip` (lowered if the teeth would come to a point).
    pub fn outline(&self, phase: f64, tip: f64) -> SprocketTeeth {
        let z = self.z();
        let pitch_angle = TAU / z;
        let land = 0.04 * self.pitch;
        let mut ra = tip;
        let ok = |r: f64| self.tooth_half_at(r).is_some_and(|h| h * r >= land / 2.0);
        let mut pointed = false;
        if !ok(ra) {
            pointed = true;
            let (_, e, _) = self.seat();
            let (mut lo, mut hi) = (e.len(), ra);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if ok(mid) { lo = mid } else { hi = mid }
            }
            ra = lo;
        }
        let (o, e_up, c_up) = self.seat();
        let t_up = self.flank_meets(ra).unwrap_or(e_up);
        let (e_lo, c_lo, t_lo) = (e_up.flip_y(), c_up.flip_y(), t_up.flip_y());
        let mut segs = Vec::new();
        for k in 0..self.teeth.max(3) {
            let a = phase + pitch_angle * f64::from(k);
            let r = |p: V2| p.rot(a);
            segs.push(minor_arc(r(c_lo), r(t_lo), r(e_lo)));
            segs.push(minor_arc(r(o), r(e_lo), r(e_up)));
            segs.push(minor_arc(r(c_up), r(e_up), r(t_up)));
            let next = t_lo.rot(a + pitch_angle);
            if r(t_up).dist(next) > 1e-6 {
                segs.push(Seg::Arc { c: V2::ZERO, a: r(t_up), b: next, ccw: true });
            }
        }
        crate::involute::join_ends(&mut segs);
        let r_min = o.x - self.seating_radius();
        SprocketTeeth { outline: Outline { segs }, ra, pointed, r_min }
    }
}

/// The shorter arc about `c` from `a` to `b`.
pub(crate) fn minor_arc(c: V2, a: V2, b: V2) -> Seg {
    Seg::Arc { c, a, b, ccw: (a - c).cross(b - c) > 0.0 }
}

/// Tooth width bf1 for one strand: 0.93·b1 (simplex) or 0.91·b1 (duplex and up) for pitches up
/// to 12.7 mm, 0.95·b1 or 0.93·b1 above.
pub fn tooth_width(pitch: f64, inner_width: f64, strands: u32) -> f64 {
    let k = match (pitch <= 12.7 + 1e-9, strands <= 1) {
        (true, true) => 0.93,
        (true, false) => 0.91,
        (false, true) => 0.95,
        (false, false) => 0.93,
    };
    k * inner_width
}

/// The axial section of the toothed rim, as a closed polygon in the (radius, z) half plane: the
/// teeth of each strand from `z = strand × pt` for `width`, with the tooth side chamfer (side
/// radius rx = p, chamfer width ba = 0.13·p, both nominal; the arc is drawn as its chord) cutting the tips' corners, the core
/// kept below radius `r_core` (the gaps between strands are cut separately). For a revolve that
/// intersects the teeth.
pub fn chamfer_profile(pitch: f64, ra: f64, width: f64, strands: u32, strand_pitch: f64, r_core: f64) -> Vec<V2> {
    let rx = pitch;
    let ba = (0.13 * pitch).min(width * 0.4);
    let h = (rx * rx - (rx - ba) * (rx - ba)).max(0.0).sqrt();
    let rc = (ra - h).max(r_core);
    let past = 0.02 * pitch;
    // The arc from the side face (at rc) out past the tip, bottom side of a band at z = 0.
    let centre = V2::new(rc, rx);
    let a0 = -PI / 2.0;
    let reach = ((ra + past - rc) / rx).clamp(-1.0, 1.0).asin();
    let a1 = -PI / 2.0 + reach;
    // One chord of the side radius: a single cone per side keeps the boolean quick.
    let n = 1;
    let arc: Vec<V2> = (0..=n).map(|i| centre + V2::polar(rx, a0 + (a1 - a0) * f64::from(i) / f64::from(n))).collect();
    let mut pts = vec![V2::new(0.0, -1.0), V2::new(rc, -1.0)];
    for s in 0..strands.max(1) {
        let z0 = f64::from(s) * strand_pitch;
        pts.extend(arc.iter().map(|q| V2::new(q.x, z0 + q.y)));
        pts.extend(arc.iter().rev().map(|q| V2::new(q.x, z0 + width - q.y)));
    }
    let top = f64::from(strands.max(1) - 1) * strand_pitch + width;
    pts.push(V2::new(rc, top + 1.0));
    pts.push(V2::new(0.0, top + 1.0));
    pts
}
