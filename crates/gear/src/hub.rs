//! Bores (round, keyed to DIN 6885, D flat, hex, splined later), hubs and lightening holes.

use std::f64::consts::{PI, TAU};

use crate::geom::{Outline, Seg, V2};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BoreKind {
    #[default]
    None,
    Round,
    /// A parallel key per DIN 6885 (width and depth from the bore unless given).
    Keyway,
    /// A round bore with a flat (motor shafts): `flat` is the distance from the flat to the far
    /// side of the bore.
    DFlat,
    /// Hexagonal: `diameter` across flats.
    Hex,
    /// Square: `diameter` across flats.
    Square,
}

impl BoreKind {
    pub const ALL: [(BoreKind, &'static str); 6] = [
        (BoreKind::None, "none"),
        (BoreKind::Round, "round"),
        (BoreKind::Keyway, "keyway"),
        (BoreKind::DFlat, "d_flat"),
        (BoreKind::Hex, "hex"),
        (BoreKind::Square, "square"),
    ];
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(k, _)| *k == self).map(|(_, n)| *n).unwrap_or("none")
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, n)| *n == s).map(|(k, _)| *k)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Bore {
    pub kind: BoreKind,
    pub diameter: f64,
    /// Keyway width and depth into the hub (0: from DIN 6885).
    pub key_width: f64,
    pub key_depth: f64,
    /// D flat: distance across from the flat to the bore's far side (0: 0.9 × diameter).
    pub flat: f64,
    /// The hole a shaft the gear is one body with fills (a train's built shaft): too big, it is
    /// the shaft that is too wide for the gear, not a bore to make smaller ([`root_words`]).
    pub on_shaft: bool,
}

/// DIN 6885 A: (bore over, bore up to, key width b, hub groove depth t2).
const DIN6885: &[(f64, f64, f64, f64)] = &[
    (6.0, 8.0, 2.0, 1.0),
    (8.0, 10.0, 3.0, 1.4),
    (10.0, 12.0, 4.0, 1.8),
    (12.0, 17.0, 5.0, 2.3),
    (17.0, 22.0, 6.0, 2.8),
    (22.0, 30.0, 8.0, 3.3),
    (30.0, 38.0, 10.0, 3.3),
    (38.0, 44.0, 12.0, 3.3),
    (44.0, 50.0, 14.0, 3.8),
    (50.0, 58.0, 16.0, 4.3),
    (58.0, 65.0, 18.0, 4.4),
    (65.0, 75.0, 20.0, 4.9),
    (75.0, 85.0, 22.0, 5.4),
    (85.0, 95.0, 25.0, 5.4),
    (95.0, 110.0, 28.0, 6.4),
    (110.0, 130.0, 32.0, 7.4),
    (130.0, 150.0, 36.0, 8.4),
    (150.0, 170.0, 40.0, 9.4),
    (170.0, 200.0, 45.0, 10.4),
];

/// The DIN 6885 key width and hub groove depth for a bore, if the table covers it.
pub fn din6885(d: f64) -> Option<(f64, f64)> {
    DIN6885.iter().find(|(lo, hi, _, _)| d > *lo && d <= *hi).map(|(_, _, b, t2)| (*b, *t2)).or_else(|| {
        // Below 6 mm: scale the smallest key.
        (d > 2.0 && d <= 6.0).then_some((d / 3.0, d / 6.0))
    })
}

impl Bore {
    /// The bore's outline, centred at the origin, the key or flat towards +y.
    pub fn outline(&self) -> Option<Outline> {
        let r = self.diameter / 2.0;
        if r <= 0.0 {
            return None;
        }
        Some(match self.kind {
            BoreKind::None => return None,
            BoreKind::Round => Outline::circle(V2::ZERO, r),
            BoreKind::Keyway => {
                let (b0, t0) = din6885(self.diameter).unwrap_or((self.diameter / 4.0, self.diameter / 8.0));
                let w = if self.key_width > 0.0 { self.key_width } else { b0 }.min(self.diameter * 0.9);
                let t = if self.key_depth > 0.0 { self.key_depth } else { t0 };
                let yb = (r * r - w * w / 4.0).max(0.0).sqrt();
                let top = r + t;
                let (p1, p2, p3, p4) = (V2::new(w / 2.0, yb), V2::new(w / 2.0, top), V2::new(-w / 2.0, top), V2::new(-w / 2.0, yb));
                Outline { segs: vec![Seg::Line(p1, p2), Seg::Line(p2, p3), Seg::Line(p3, p4), Seg::Arc { c: V2::ZERO, a: p4, b: p1, ccw: true }] }
            }
            BoreKind::DFlat => {
                let across = if self.flat > 0.0 { self.flat } else { 0.9 * self.diameter }.clamp(0.05 * self.diameter, self.diameter * 0.999);
                let yf = across - r;
                let x = (r * r - yf * yf).max(0.0).sqrt();
                let (a, b) = (V2::new(x, yf), V2::new(-x, yf));
                Outline { segs: vec![Seg::Line(a, b), Seg::Arc { c: V2::ZERO, a: b, b: a, ccw: true }] }
            }
            BoreKind::Hex | BoreKind::Square => {
                let n = if self.kind == BoreKind::Hex { 6 } else { 4 };
                let rc = r / (PI / f64::from(n)).cos();
                let pts: Vec<V2> = (0..n).map(|k| V2::polar(rc, PI / 2.0 - PI / f64::from(n) + TAU * f64::from(k) / f64::from(n))).collect();
                Outline { segs: (0..pts.len()).map(|i| Seg::Line(pts[i], pts[(i + 1) % pts.len()])).collect() }
            }
        })
    }
    /// The radius the bore reaches out to (keyway or corners included).
    pub fn reach(&self) -> f64 {
        match self.kind {
            BoreKind::None => 0.0,
            _ => self.outline().map(|o| o.max_radius()).unwrap_or(0.0),
        }
    }
}

/// What to say of `bore` reaching a gear's tooth roots (`root`: their Ø; `module`): a real bore
/// "the bore (reaching Ø15) cuts into the tooth roots (Ø12.1); use a smaller bore", a shaft the
/// gear sits on "sits on a shaft (Ø15), which is wider than its tooth roots (Ø12.1): use a shaft
/// of at most Ø9 or a bigger gear". Starts after the gear's name: "{who}{words}".
pub fn root_words(bore: &Bore, root: f64, module: f64) -> String {
    let one = |v: f64| crate::axes::num((v * 10.0).round() / 10.0);
    if bore.on_shaft {
        format!(
            " sits on a shaft (Ø{}), which is wider than its tooth roots (Ø{}): use a shaft of at most Ø{} or a bigger gear",
            one(bore.diameter),
            one(root),
            crate::axes::num(largest_shaft(root, module))
        )
    } else {
        format!(": the bore (reaching Ø{}) cuts into the tooth roots (Ø{}); use a smaller bore", one(2.0 * bore.reach()), one(root))
    }
}

/// The widest shaft (Ø, mm) a gear with tooth roots of Ø`root` takes: a wall of a module (at
/// least 1 mm) each side under the roots, rounded down to a whole mm (half a mm under Ø3).
pub fn largest_shaft(root: f64, module: f64) -> f64 {
    let d = root - 2.0 * module.max(1.0);
    let step = if d < 3.0 { 0.5 } else { 1.0 };
    ((d / step).floor() * step).max(0.5)
}

/// Round lightening holes on a circle between the hub and the rim.
pub fn lightening_holes(count: u32, diameter: f64, at_radius: f64, phase: f64) -> Vec<Outline> {
    (0..count).map(|k| Outline::circle(V2::polar(at_radius, phase + TAU * f64::from(k) / f64::from(count.max(1))), diameter / 2.0)).collect()
}

// ---------------------------------------------------------------------------------------------
// Integral shafts, chamfers and hub hardware (built by `crate::body`).

/// What the free end of an integral shaft carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShaftEnd {
    #[default]
    Plain,
    /// A flat (towards +y): `end_size` is the distance from the flat to the far side.
    DFlat,
    /// A key slot (towards +y): `end_size` is its width (0: DIN 6885 for the diameter).
    Keyway,
    /// Hexagonal: `end_size` across flats.
    Hex,
    /// A "+" for hobby servo horns and toy axles: `end_size` is the width of its arms.
    Cross,
    /// A centre hole down the axis: `end_size` is its diameter, `end_length` its depth.
    CentreHole,
}

impl ShaftEnd {
    pub const ALL: [(ShaftEnd, &'static str); 6] = [
        (ShaftEnd::Plain, "plain"),
        (ShaftEnd::DFlat, "d_flat"),
        (ShaftEnd::Keyway, "keyway"),
        (ShaftEnd::Hex, "hex"),
        (ShaftEnd::Cross, "cross"),
        (ShaftEnd::CentreHole, "centre_hole"),
    ];
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(k, _)| *k == self).map(|(_, n)| *n).unwrap_or("plain")
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, n)| *n == s).map(|(k, _)| *k)
    }
}

/// A shaft made in one piece with a gear, on one side of it: a first length next to the gear,
/// an optional second length (a step or shoulder) beyond it, and what its free end carries.
/// Lengths are measured from the gear's face. No diameter or no length: no shaft.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Shaft {
    pub diameter: f64,
    pub length: f64,
    /// The second length's diameter and length (0: none). Smaller than `diameter` for a
    /// shoulder, larger for a collar.
    pub step_diameter: f64,
    pub step_length: f64,
    pub end: ShaftEnd,
    /// How far the end feature runs back from the free end (0: 1.5 × the end's diameter; a
    /// centre hole: 1.5 × its own diameter).
    pub end_length: f64,
    /// The end feature's size, see [`ShaftEnd`] (0: from the diameter).
    pub end_size: f64,
    /// A 45° chamfer on the free end (mm).
    pub chamfer: f64,
}

impl Shaft {
    pub fn exists(&self) -> bool {
        self.diameter > 0.0 && self.length > 0.0
    }
    pub fn stepped(&self) -> bool {
        self.exists() && self.step_diameter > 0.0 && self.step_length > 0.0
    }
    /// From the gear's face to the free end.
    pub fn total(&self) -> f64 {
        if !self.exists() {
            0.0
        } else if self.stepped() {
            self.length + self.step_length
        } else {
            self.length
        }
    }
    /// The diameter at the free end.
    pub fn end_diameter(&self) -> f64 {
        if self.stepped() { self.step_diameter } else { self.diameter }
    }
    pub fn min_diameter(&self) -> f64 {
        if self.stepped() { self.diameter.min(self.step_diameter) } else { self.diameter }
    }
    pub fn max_diameter(&self) -> f64 {
        if self.stepped() { self.diameter.max(self.step_diameter) } else { self.diameter }
    }
}

/// An edge chamfer: `size` across the face (radially, mm; 0: none) at `angle` degrees to the
/// face, so it runs `size`·tan(`angle`) along the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chamfer {
    pub size: f64,
    pub angle: f64,
}

impl Default for Chamfer {
    fn default() -> Self {
        Self { size: 0.0, angle: 45.0 }
    }
}

impl Chamfer {
    /// How far the chamfer runs along the axis.
    pub fn depth(&self) -> f64 {
        self.size * self.angle.to_radians().tan()
    }
}

/// Hardware for fixing a (printed) gear: everything is off at 0. Set screws, nut traps, insert
/// pockets and the clamp slit go in the hub (which the gear must have); the bearing pocket goes
/// in the gear's back face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hardware {
    /// Set-screw hole diameter: radial, through the hub wall at mid length, over the key or flat
    /// (+y).
    pub set_screw: f64,
    /// How many set screws (1 to 4), 90° apart.
    pub set_screws: u32,
    /// Captive nut trap on each set screw: the nut's width across flats and its thickness
    /// (0: 0.45 × the width). A slot open to the hub's end.
    pub nut_width: f64,
    pub nut_thickness: f64,
    /// Heat-set insert pocket at the mouth of each set-screw hole: diameter and depth (0: most
    /// of the hub wall).
    pub insert_diameter: f64,
    pub insert_depth: f64,
    /// Clamping hub: the width of a slit through the hub wall (opposite the set screw), and the
    /// diameter of the clamp screw's hole across it (0: none).
    pub clamp_slit: f64,
    pub clamp_screw: f64,
    /// Bearing pocket in the back face: the bearing's outside diameter and width, and how much
    /// larger than the bearing the pocket is made (on the diameter, for a printed fit).
    pub bearing_od: f64,
    pub bearing_width: f64,
    pub bearing_fit: f64,
}

impl Default for Hardware {
    fn default() -> Self {
        Self {
            set_screw: 0.0,
            set_screws: 1,
            nut_width: 0.0,
            nut_thickness: 0.0,
            insert_diameter: 0.0,
            insert_depth: 0.0,
            clamp_slit: 0.0,
            clamp_screw: 0.0,
            bearing_od: 0.0,
            bearing_width: 0.0,
            bearing_fit: 0.0,
        }
    }
}

impl Hardware {
    /// Sets the bearing pocket from a standard bearing's name; false when the name is unknown.
    pub fn set_bearing(&mut self, name: &str) -> bool {
        match bearing(name) {
            Some((_, od, w)) => {
                self.bearing_od = od;
                self.bearing_width = w;
                true
            }
            None => false,
        }
    }
}

/// Common deep-groove ball bearings (ISO 15 boundary dimensions): (name, bore, outside diameter,
/// width) in mm. Series: MR (miniature, metric), 6xx (60 / 62 miniature), 68xx and 69xx (thin
/// section), 60xx (extra light), 62xx (light), 63xx (medium). The first ones in each series are
/// the ones the Gear Generator's lists showed before the table grew, in their old order.
pub const BEARINGS: &[(&str, f64, f64, f64)] = &[
    ("MR63", 3.0, 6.0, 2.5),
    ("MR84", 4.0, 8.0, 3.0),
    ("MR105", 5.0, 10.0, 4.0),
    ("MR115", 5.0, 11.0, 4.0),
    ("MR126", 6.0, 12.0, 4.0),
    ("MR128", 8.0, 12.0, 3.5),
    ("623", 3.0, 10.0, 4.0),
    ("624", 4.0, 13.0, 5.0),
    ("625", 5.0, 16.0, 5.0),
    ("626", 6.0, 19.0, 6.0),
    ("608", 8.0, 22.0, 7.0),
    ("688", 8.0, 16.0, 5.0),
    ("6800", 10.0, 19.0, 5.0),
    ("6801", 12.0, 21.0, 5.0),
    ("6802", 15.0, 24.0, 5.0),
    ("6803", 17.0, 26.0, 5.0),
    ("6804", 20.0, 32.0, 7.0),
    ("6000", 10.0, 26.0, 8.0),
    ("6001", 12.0, 28.0, 8.0),
    ("6002", 15.0, 32.0, 9.0),
    ("6003", 17.0, 35.0, 10.0),
    ("6004", 20.0, 42.0, 12.0),
    ("6200", 10.0, 30.0, 9.0),
    ("6201", 12.0, 32.0, 10.0),
    ("6202", 15.0, 35.0, 11.0),
    // Miniature (MR).
    ("MR52", 2.0, 5.0, 2.0),
    ("MR62", 2.0, 6.0, 2.5),
    ("MR74", 4.0, 7.0, 2.5),
    ("MR83", 3.0, 8.0, 3.0),
    ("MR85", 5.0, 8.0, 2.5),
    ("MR95", 5.0, 9.0, 3.0),
    ("MR104", 4.0, 10.0, 4.0),
    ("MR106", 6.0, 10.0, 3.0),
    ("MR117", 7.0, 11.0, 3.0),
    ("MR137", 7.0, 13.0, 4.0),
    ("MR148", 8.0, 14.0, 4.0),
    // 60 and 62 miniature.
    ("603", 3.0, 9.0, 5.0),
    ("604", 4.0, 12.0, 4.0),
    ("605", 5.0, 14.0, 5.0),
    ("606", 6.0, 17.0, 6.0),
    ("607", 7.0, 19.0, 6.0),
    ("609", 9.0, 24.0, 7.0),
    ("627", 7.0, 22.0, 7.0),
    ("628", 8.0, 24.0, 8.0),
    ("629", 9.0, 26.0, 8.0),
    // Thin section, 68 series.
    ("683", 3.0, 7.0, 2.0),
    ("684", 4.0, 9.0, 2.5),
    ("685", 5.0, 11.0, 3.0),
    ("686", 6.0, 13.0, 3.5),
    ("687", 7.0, 14.0, 3.5),
    ("689", 9.0, 17.0, 4.0),
    ("6805", 25.0, 37.0, 7.0),
    ("6806", 30.0, 42.0, 7.0),
    // Thin section, 69 series.
    ("693", 3.0, 8.0, 3.0),
    ("694", 4.0, 11.0, 4.0),
    ("695", 5.0, 13.0, 4.0),
    ("696", 6.0, 15.0, 5.0),
    ("697", 7.0, 17.0, 5.0),
    ("698", 8.0, 19.0, 6.0),
    ("699", 9.0, 20.0, 6.0),
    ("6900", 10.0, 22.0, 6.0),
    ("6901", 12.0, 24.0, 6.0),
    ("6902", 15.0, 28.0, 7.0),
    ("6903", 17.0, 30.0, 7.0),
    ("6904", 20.0, 37.0, 9.0),
    ("6905", 25.0, 42.0, 9.0),
    // 60, 62 and 63 series.
    ("6005", 25.0, 47.0, 12.0),
    ("6006", 30.0, 55.0, 13.0),
    ("6203", 17.0, 40.0, 12.0),
    ("6204", 20.0, 47.0, 14.0),
    ("6205", 25.0, 52.0, 15.0),
    ("6206", 30.0, 62.0, 16.0),
    ("6300", 10.0, 35.0, 11.0),
    ("6301", 12.0, 37.0, 12.0),
    ("6302", 15.0, 42.0, 13.0),
];

/// The bearing series a name from [`BEARINGS`] belongs to, for grouping a picker: "MR",
/// "6xx", "68xx", "69xx", "60xx", "62xx" or "63xx".
pub fn bearing_series(name: &str) -> &'static str {
    let n = name.trim();
    if n.len() >= 2 && n[..2].eq_ignore_ascii_case("MR") {
        "MR"
    } else if n.len() == 3 {
        match &n[..2] {
            "68" => "68xx",
            "69" => "69xx",
            _ => "6xx",
        }
    } else {
        match n.get(..2) {
            Some("68") => "68xx",
            Some("69") => "69xx",
            Some("60") => "60xx",
            Some("62") => "62xx",
            Some("63") => "63xx",
            _ => "6xx",
        }
    }
}

/// A standard bearing's (bore, outside diameter, width) by name ("608", "6001", "MR105", ...).
pub fn bearing(name: &str) -> Option<(f64, f64, f64)> {
    let name = name.trim();
    BEARINGS.iter().find(|(n, ..)| n.eq_ignore_ascii_case(name)).map(|(_, d, od, w)| (*d, *od, *w))
}
