//! Thread forms from the standards: each form's basic or design profile, its standard sizes and
//! its designations. docs/threads.md gives the values and where they come from.
//!
//! A groove is the space between the teeth, in an axial half-plane: a polygon in (r, z), r from
//! the axis, z along it, the groove's walls straight flanks (crest and root radii are not
//! modelled, so every groove is a polygon the screw sweep carries exactly).

use serde::{Deserialize, Serialize};

const INCH: f64 = 25.4;

/// A thread form.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadForm {
    /// ISO metric, 60° (ISO 68-1 basic profile, ISO 261/262 sizes): `M10`, `M10x1`.
    #[default]
    Metric,
    /// ISO metric trapezoidal, 30° (ISO 2904 design profile, ISO 2902 sizes): `Tr20x4`.
    Trapezoidal,
    /// General purpose Acme, 29° (ASME B1.5): `1/2-10 ACME`.
    Acme,
    /// Metric buttress (saw-tooth), 3°/30° (DIN 513-1): `S20x4`.
    Buttress,
    /// Buttress inch, 7°/45° (ASME B1.9 basic profile): `2.5-8 BUTT`.
    ButtressAsme,
    /// The square thread: no current standard defines it, so it is refused.
    Square,
}

/// Which way a thread turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHand {
    #[default]
    Right,
    Left,
    /// A right- and a left-handed groove crossing on one face (a reversing screw).
    Both,
}

/// Which way the steep (load) flank of a buttress screw's teeth faces along the thread: towards
/// the thread's end (the way its axis points) or its start. A nut cut with the same setting mates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteepFlank {
    Start,
    #[default]
    End,
}

/// The thread's free end: as cut, or chamfered (ISO 4753).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadTip {
    #[default]
    None,
    Chamfer,
}

/// For `skip_serializing_if`: a field at its default is left out of the saved document.
pub fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    *t == T::default()
}

pub fn one_start() -> u32 {
    1
}

pub fn is_one_start(n: &u32) -> bool {
    *n == 1
}

/// A thread's size: nominal (major) diameter, pitch and lead, all in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadSize {
    pub form: ThreadForm,
    pub major: f64,
    pub pitch: f64,
    /// starts × pitch.
    pub lead: f64,
}

/// ISO 261/262 coarse pitches: (nominal diameter, pitch).
pub(crate) const ISO_COARSE: [(f64, f64); 22] = [
    (1.6, 0.35),
    (2.0, 0.4),
    (2.5, 0.45),
    (3.0, 0.5),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (14.0, 2.0),
    (16.0, 2.0),
    (18.0, 2.5),
    (20.0, 2.5),
    (22.0, 2.5),
    (24.0, 3.0),
    (27.0, 3.0),
    (30.0, 3.5),
    (36.0, 4.0),
    (42.0, 4.5),
    (48.0, 5.0),
    (56.0, 5.5),
];

/// The ISO 2902 diameter-pitch series' middle pitch per diameter, which ISO 2904 (trapezoidal)
/// and DIN 513-2 (buttress, from 10 mm) share: (nominal diameter, pitch).
const ISO_2902_MEDIUM: [(f64, f64); 41] = [
    (8.0, 1.5),
    (9.0, 2.0),
    (10.0, 2.0),
    (11.0, 2.0),
    (12.0, 3.0),
    (14.0, 3.0),
    (16.0, 4.0),
    (18.0, 4.0),
    (20.0, 4.0),
    (22.0, 5.0),
    (24.0, 5.0),
    (26.0, 5.0),
    (28.0, 5.0),
    (30.0, 6.0),
    (32.0, 6.0),
    (34.0, 6.0),
    (36.0, 6.0),
    (38.0, 7.0),
    (40.0, 7.0),
    (42.0, 7.0),
    (44.0, 7.0),
    (46.0, 8.0),
    (48.0, 8.0),
    (50.0, 8.0),
    (52.0, 8.0),
    (55.0, 9.0),
    (60.0, 9.0),
    (65.0, 10.0),
    (70.0, 10.0),
    (75.0, 10.0),
    (80.0, 10.0),
    (85.0, 12.0),
    (90.0, 12.0),
    (95.0, 12.0),
    (100.0, 12.0),
    (105.0, 12.0),
    (110.0, 12.0),
    (115.0, 14.0),
    (120.0, 14.0),
    (125.0, 14.0),
    (130.0, 14.0),
];

/// ASME B1.5 general purpose Acme: (nominal diameter, threads per inch), in inches.
const ACME_SIZES: [(&str, f64, f64); 23] = [
    ("1/4", 0.25, 16.0),
    ("5/16", 0.3125, 14.0),
    ("3/8", 0.375, 12.0),
    ("7/16", 0.4375, 12.0),
    ("1/2", 0.5, 10.0),
    ("5/8", 0.625, 8.0),
    ("3/4", 0.75, 6.0),
    ("7/8", 0.875, 6.0),
    ("1", 1.0, 5.0),
    ("1 1/8", 1.125, 5.0),
    ("1 1/4", 1.25, 5.0),
    ("1 3/8", 1.375, 4.0),
    ("1 1/2", 1.5, 4.0),
    ("1 3/4", 1.75, 4.0),
    ("2", 2.0, 4.0),
    ("2 1/4", 2.25, 3.0),
    ("2 1/2", 2.5, 3.0),
    ("2 3/4", 2.75, 3.0),
    ("3", 3.0, 2.0),
    ("3 1/2", 3.5, 2.0),
    ("4", 4.0, 2.0),
    ("4 1/2", 4.5, 2.0),
    ("5", 5.0, 2.0),
];

/// The radii and widths of a form's groove, external or internal, for one size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrooveProfile {
    /// The flank angles from the radial direction of the groove's wall towards −z and towards +z.
    pub flank_lo: f64,
    pub flank_hi: f64,
    /// Where the groove's teeth have their crest: the major radius (external) or the minor one
    /// (internal), and the tooth's width there.
    pub crest_r: f64,
    pub crest_flat: f64,
    /// The groove's root: the minor radius (external) or the major one (internal).
    pub root_r: f64,
    /// The basic depth of engagement, for the "cuts next to nothing" check.
    pub basic_depth: f64,
    /// A nut's groove: its teeth's crests towards the axis.
    pub internal: bool,
}

impl GrooveProfile {
    /// The groove's axial width at radius `r`: the pitch less the tooth there.
    pub fn width(&self, pitch: f64, r: f64) -> f64 {
        // How far `r` is from the crest towards the root (negative past the crest, in the air
        // the groove reaches into).
        let into = if self.internal { r - self.crest_r } else { self.crest_r - r };
        pitch - self.crest_flat - into * (self.flank_lo.tan() + self.flank_hi.tan())
    }
    /// The groove's (−z, +z) extents at radius `r`, its root flat centred on z = 0.
    pub fn extents(&self, pitch: f64, r: f64) -> (f64, f64) {
        let root = self.width(pitch, self.root_r) / 2.0;
        let d = (r - self.root_r).abs();
        (root + d * self.flank_lo.tan(), root + d * self.flank_hi.tan())
    }
    /// One turn of the groove over radii `a..b` (between root and crest): 2π∫ r w(r) dr.
    pub fn turn_volume(&self, pitch: f64, a: f64, b: f64) -> f64 {
        // w(r) = c0 + c1 r, linear in r.
        let (w0, w1) = (self.width(pitch, a), self.width(pitch, b));
        if (b - a).abs() < 1e-15 {
            return 0.0;
        }
        let c1 = (w1 - w0) / (b - a);
        let c0 = w0 - c1 * a;
        let f = |r: f64| c0 * r * r / 2.0 + c1 * r * r * r / 3.0;
        2.0 * std::f64::consts::PI * (f(b) - f(a))
    }
}

impl ThreadForm {
    pub const ALL: [ThreadForm; 6] =
        [ThreadForm::Metric, ThreadForm::Trapezoidal, ThreadForm::Acme, ThreadForm::Buttress, ThreadForm::ButtressAsme, ThreadForm::Square];

    pub fn name(self) -> &'static str {
        match self {
            ThreadForm::Metric => "metric",
            ThreadForm::Trapezoidal => "trapezoidal",
            ThreadForm::Acme => "acme",
            ThreadForm::Buttress => "buttress",
            ThreadForm::ButtressAsme => "buttress_asme",
            ThreadForm::Square => "square",
        }
    }

    pub fn from_name(s: &str) -> Option<ThreadForm> {
        ThreadForm::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(s.trim()))
    }

    /// The standard a form's profile and sizes come from.
    pub fn standard(self) -> &'static str {
        match self {
            ThreadForm::Metric => "ISO 68-1, ISO 261",
            ThreadForm::Trapezoidal => "ISO 2904, ISO 2902",
            ThreadForm::Acme => "ASME B1.5",
            ThreadForm::Buttress => "DIN 513-1, DIN 513-2",
            ThreadForm::ButtressAsme => "ASME B1.9",
            ThreadForm::Square => "none",
        }
    }

    /// Why a form cannot be cut, if it cannot.
    pub fn refused(self) -> Option<&'static str> {
        (self == ThreadForm::Square).then_some(
            "no current standard defines a square thread (ISO, ASME and DIN have none; Machinery's Handbook gives only historical proportions): use trapezoidal (ISO 2904) or acme (ASME B1.5)",
        )
    }

    /// The form a designation's prefix names: `M` metric, `Tr` trapezoidal, `S` buttress, an inch
    /// size with `ACME` or `BUTT`.
    pub fn of_designation(s: &str) -> Option<ThreadForm> {
        let t = s.trim().to_ascii_uppercase();
        if t.contains("ACME") {
            Some(ThreadForm::Acme)
        } else if t.contains("BUTT") {
            Some(ThreadForm::ButtressAsme)
        } else if t.starts_with("TR") {
            Some(ThreadForm::Trapezoidal)
        } else if t.starts_with('S') {
            Some(ThreadForm::Buttress)
        } else if t.starts_with('M') {
            Some(ThreadForm::Metric)
        } else {
            None
        }
    }

    /// The standard sizes, (nominal diameter, pitch) in mm; none for ASME buttress (its pitch is
    /// always given) and the square thread.
    pub fn sizes(self) -> Vec<(f64, f64)> {
        match self {
            ThreadForm::Metric => ISO_COARSE.to_vec(),
            ThreadForm::Trapezoidal => ISO_2902_MEDIUM.to_vec(),
            ThreadForm::Buttress => ISO_2902_MEDIUM.iter().copied().filter(|(d, _)| *d >= 10.0).collect(),
            ThreadForm::Acme => ACME_SIZES.iter().map(|(_, d, n)| (d * INCH, INCH / n)).collect(),
            ThreadForm::ButtressAsme | ThreadForm::Square => Vec::new(),
        }
    }

    /// The standard designations of [`sizes`](Self::sizes), in order.
    pub fn designations(self) -> Vec<String> {
        self.sizes().into_iter().map(|(d, p)| self.format(d, p, p)).collect()
    }

    /// The size a designation names (`starts` starts unless the designation gives its lead).
    pub fn parse(self, s: &str, starts: u32) -> Result<ThreadSize, String> {
        if let Some(why) = self.refused() {
            return Err(why.into());
        }
        let bad = || format!("unknown {} thread `{s}` ({})", self.name(), self.example());
        if let Some(f) = ThreadForm::of_designation(s)
            && f != self
        {
            return Err(format!("`{s}` is a {} designation, not {}", f.name(), self.name()));
        }
        let t = s.trim().to_ascii_uppercase().replace('×', "X").replace(' ', "");
        let n = f64::from(starts.max(1));
        let (major, pitch, lead) = match self {
            ThreadForm::Metric => {
                let body = t.strip_prefix('M').ok_or_else(bad)?;
                // M16xPh3P1.5 (ISO 965-1: lead 3, pitch 1.5), M10x1.5, M10.
                match body.split_once('X') {
                    Some((d, rest)) => {
                        let d = num(d).ok_or_else(bad)?;
                        match rest.strip_prefix("PH").and_then(|r| r.split_once('P')) {
                            Some((l, p)) => (d, num(p).ok_or_else(bad)?, num(l).ok_or_else(bad)?),
                            None => {
                                let p = num(rest).ok_or_else(bad)?;
                                (d, p, p * n)
                            }
                        }
                    }
                    None => {
                        let d = num(body).ok_or_else(bad)?;
                        let p = self.coarse(d).ok_or_else(|| format!("no coarse pitch for M{} (give it: M{}x1)", show(d), show(d)))?;
                        (d, p, p * n)
                    }
                }
            }
            ThreadForm::Trapezoidal | ThreadForm::Buttress => {
                let body = t.strip_prefix(if self == ThreadForm::Trapezoidal { "TR" } else { "S" }).ok_or_else(bad)?;
                // Tr40x7, Tr40x14(P7) (ISO 2903: lead 14, pitch 7), Tr40 (the standard pitch).
                match body.split_once('X') {
                    Some((d, rest)) => {
                        let d = num(d).ok_or_else(bad)?;
                        match rest.split_once("(P") {
                            Some((l, p)) => (d, num(p.strip_suffix(')').ok_or_else(bad)?).ok_or_else(bad)?, num(l).ok_or_else(bad)?),
                            None => {
                                let p = num(rest).ok_or_else(bad)?;
                                (d, p, p * n)
                            }
                        }
                    }
                    None => {
                        let d = num(body).ok_or_else(bad)?;
                        let p = self.coarse(d).ok_or_else(|| format!("no standard pitch for a {} mm {} thread: give it", show(d), self.name()))?;
                        (d, p, p * n)
                    }
                }
            }
            ThreadForm::Acme | ThreadForm::ButtressAsme => {
                // 1/2-10 ACME, 1 1/4-5, 2.5-8 BUTT (a class such as -2G or -2A is a tolerance, ignored).
                let body = t.trim_end_matches("ACME").trim_end_matches("PUSH-BUTT").trim_end_matches("PULL-BUTT").trim_end_matches("BUTT");
                let body = body.trim_end_matches('-');
                let mut parts = body.split('-');
                let d = parts.next().and_then(inches).ok_or_else(bad)?;
                let tpi = parts.next().and_then(num).ok_or_else(bad)?;
                if let Some(rest) = parts.next()
                    && !(rest.len() == 2 && rest.ends_with(['G', 'C', 'A', 'B']))
                {
                    return Err(bad());
                }
                let p = INCH / tpi;
                (d * INCH, p, p * n)
            }
            ThreadForm::Square => return Err(bad()),
        };
        if !(major.is_finite() && pitch.is_finite() && lead.is_finite() && major > 0.0 && pitch > 0.0 && pitch < major) {
            return Err(bad());
        }
        let k = lead / pitch;
        if !(k >= 1.0 - 1e-9 && (k - k.round()).abs() < 1e-6 && k.round() <= 8.0) {
            return Err(format!("the lead of `{s}` is not 1 to 8 times its pitch"));
        }
        Ok(ThreadSize { form: self, major, pitch, lead })
    }

    fn example(self) -> &'static str {
        match self {
            ThreadForm::Metric => "like M8 or M8x1",
            ThreadForm::Trapezoidal => "like Tr20x4 or Tr40x14(P7)",
            ThreadForm::Acme => "like 1/2-10 ACME",
            ThreadForm::Buttress => "like S20x4 or S40x14(P7)",
            ThreadForm::ButtressAsme => "like 2.5-8 BUTT",
            ThreadForm::Square => "no standard",
        }
    }

    /// The standard pitch of a diameter in the form's table.
    fn coarse(self, d: f64) -> Option<f64> {
        self.sizes().into_iter().find(|(n, _)| (n - d).abs() < 1e-9).map(|(_, p)| p)
    }

    /// A size's designation, in the form its standard writes it.
    pub fn format(self, d: f64, p: f64, lead: f64) -> String {
        let multi = (lead - p).abs() > 1e-9;
        match self {
            ThreadForm::Metric => {
                if multi {
                    format!("M{}xPh{}P{}", show(d), show(lead), show(p))
                } else if self.coarse(d).is_some_and(|q| (q - p).abs() < 1e-9) {
                    format!("M{}", show(d))
                } else {
                    format!("M{}x{}", show(d), show(p))
                }
            }
            ThreadForm::Trapezoidal | ThreadForm::Buttress => {
                let pre = if self == ThreadForm::Trapezoidal { "Tr" } else { "S" };
                if multi { format!("{pre}{}x{}(P{})", show(d), show(lead), show(p)) } else { format!("{pre}{}x{}", show(d), show(p)) }
            }
            ThreadForm::Acme | ThreadForm::ButtressAsme => {
                // Acme sizes by their fractions (ASME B1.5's table), buttress in decimal inches.
                let di = d / INCH;
                let named = ACME_SIZES.iter().find(|(_, x, _)| self == ThreadForm::Acme && (x - di).abs() < 1e-9);
                let name = named.map(|(n, _, _)| n.to_string()).unwrap_or_else(|| show(di));
                let tpi = show(INCH / p);
                format!("{name}-{tpi} {}", if self == ThreadForm::Acme { "ACME" } else { "BUTT" })
            }
            ThreadForm::Square => format!("{}x{}", show(d), show(p)),
        }
    }

    /// The minor diameter a hole for this size is drilled to: the tap drill (major − pitch) for
    /// metric, the internal thread's minor diameter for the others.
    pub fn drill(self, d: f64, p: f64) -> f64 {
        match self {
            ThreadForm::Metric => d - p,
            _ => 2.0 * self.groove(d, p, true).crest_r,
        }
    }

    /// The standard size a cylinder of this diameter takes: a shaft is the major diameter, a hole
    /// the drill. None when no size is within 15% (or the form has no size table).
    pub fn fit(self, diameter: f64, internal: bool) -> Option<(f64, f64)> {
        self.sizes()
            .into_iter()
            .map(|(n, p)| (n, p, if internal { self.drill(n, p) } else { n }))
            .min_by(|a, b| (a.2 - diameter).abs().total_cmp(&(b.2 - diameter).abs()))
            .filter(|(_, _, want)| (want - diameter).abs() <= 0.15 * diameter)
            .map(|(n, p, _)| (n, p))
    }

    /// The groove of size (d, P): external (between a screw's teeth) or internal (a nut's).
    /// `flank_lo`/`flank_hi` are for a buttress with its steep flank towards the end (+z); the
    /// caller swaps them for [`SteepFlank::Start`].
    pub fn groove(self, d: f64, p: f64, internal: bool) -> GrooveProfile {
        let deg = |a: f64| a.to_radians();
        let half = |a: f64| (deg(a), deg(a));
        // (flanks, tooth crest flat, basic depth, external minor radius, internal minor radius,
        // internal major radius)
        let (fl, flat, depth, r3, r1, r4) = match self {
            ThreadForm::Metric | ThreadForm::Square => {
                // ISO 68-1: H = P sqrt3/2, basic depth 5H/8, flats P/8 at the major and P/4 at
                // the minor diameter.
                let h = p * 3f64.sqrt() / 2.0;
                let r_minor = d / 2.0 - 5.0 * h / 8.0;
                let flat = if internal { p / 4.0 } else { p / 8.0 };
                (half(30.0), flat, 5.0 * h / 8.0, r_minor, r_minor, d / 2.0)
            }
            ThreadForm::Trapezoidal => {
                // ISO 2904: 30°, H1 = P/2, flats P/2 - P/2 tan 15° (0.366 P) at both crests,
                // clearance ac on the external minor and internal major diameters.
                let ac = if p <= 1.5 {
                    0.15
                } else if p <= 5.0 {
                    0.25
                } else if p <= 12.0 {
                    0.5
                } else {
                    1.0
                };
                let flat = p / 2.0 - p / 2.0 * deg(15.0).tan();
                (half(15.0), flat, p / 2.0, d / 2.0 - p / 2.0 - ac, d / 2.0 - p / 2.0, d / 2.0 + ac)
            }
            ThreadForm::Acme => {
                // ASME B1.5 general purpose: 29°, h = P/2, flat 0.3707 P, the external minor
                // 0.020" under the basic minor (0.010" finer than 10 per inch), the internal
                // major as far over the basic major.
                let a = if p >= INCH / 10.0 - 1e-9 { 0.020 * INCH } else { 0.010 * INCH };
                let flat = p / 2.0 - p / 2.0 * deg(14.5).tan();
                (half(14.5), flat, p / 2.0, d / 2.0 - p / 2.0 - a / 2.0, d / 2.0 - p / 2.0, d / 2.0 + a / 2.0)
            }
            ThreadForm::Buttress => {
                // DIN 513-1: 3° load flank, 30° clearance flank, H1 = 0.75 P, flats w = 0.26384 P,
                // the external minor h3 = 0.86777 P deep (clearance ac = 0.11777 P), the internal
                // thread from D1 = d - 1.5 P to D = d.
                let (t3, t30) = (deg(3.0).tan(), deg(30.0).tan());
                let flat = (p - 0.75 * p * (t3 + t30)) / 2.0;
                ((deg(3.0), deg(30.0)), flat, 0.75 * p, d / 2.0 - 0.86777 * p, d / 2.0 - 0.75 * p, d / 2.0)
            }
            ThreadForm::ButtressAsme => {
                // ASME B1.9 basic profile: 7° load flank, 45° clearance flank, h = 0.6 P, flats
                // (P - 0.6 P (tan 7° + tan 45°))/2 = 0.16316 P, no allowances.
                let flat = (p - 0.6 * p * (deg(7.0).tan() + deg(45.0).tan())) / 2.0;
                ((deg(7.0), deg(45.0)), flat, 0.6 * p, d / 2.0 - 0.6 * p, d / 2.0 - 0.6 * p, d / 2.0)
            }
        };
        // The steep flank faces +z on the screw's teeth: the external groove's −z wall, the
        // internal groove's +z wall (the nut's teeth face the other way).
        let (lo, hi) = if internal { (fl.1, fl.0) } else { fl };
        if internal {
            GrooveProfile { flank_lo: lo, flank_hi: hi, crest_r: r1, crest_flat: flat, root_r: r4, basic_depth: depth, internal }
        } else {
            GrooveProfile { flank_lo: lo, flank_hi: hi, crest_r: d / 2.0, crest_flat: flat, root_r: r3, basic_depth: depth, internal }
        }
    }
}

/// A number; fractions allowed (`1/2`).
fn num(s: &str) -> Option<f64> {
    let s = s.trim();
    let v = match s.split_once('/') {
        Some((a, b)) => a.parse::<f64>().ok()? / b.parse::<f64>().ok()?,
        None => s.parse::<f64>().ok()?,
    };
    v.is_finite().then_some(v)
}

/// An inch size: `1`, `0.75`, `3/4`, `1 1/4` (spaces already gone: `11/4` is read as 1 1/4 only
/// from the table's names).
fn inches(s: &str) -> Option<f64> {
    if let Some((_, d, _)) = ACME_SIZES.iter().find(|(n, _, _)| n.replace(' ', "") == s) {
        return Some(*d);
    }
    num(s)
}

fn show(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn designations_read_and_write_back() {
        for (form, s, d, p, l) in [
            (ThreadForm::Metric, "M10", 10.0, 1.5, 1.5),
            (ThreadForm::Metric, "M10x1", 10.0, 1.0, 1.0),
            (ThreadForm::Metric, "M16xPh3P1.5", 16.0, 1.5, 3.0),
            (ThreadForm::Trapezoidal, "Tr20x4", 20.0, 4.0, 4.0),
            (ThreadForm::Trapezoidal, "Tr40x14(P7)", 40.0, 7.0, 14.0),
            (ThreadForm::Buttress, "S40x7", 40.0, 7.0, 7.0),
            (ThreadForm::Acme, "1/2-10 ACME", 12.7, 2.54, 2.54),
            (ThreadForm::Acme, "1 1/4-5 ACME", 31.75, 5.08, 5.08),
            (ThreadForm::ButtressAsme, "2.5-8 BUTT", 63.5, 3.175, 3.175),
        ] {
            let z = form.parse(s, 1).unwrap();
            assert!((z.major - d).abs() < 1e-9 && (z.pitch - p).abs() < 1e-9 && (z.lead - l).abs() < 1e-9, "{s}: {z:?}");
            assert_eq!(form.format(z.major, z.pitch, z.lead), s);
            assert_eq!(ThreadForm::of_designation(s), Some(form), "{s}");
        }
        assert!(ThreadForm::Trapezoidal.parse("M10", 1).is_err());
        assert!(ThreadForm::Square.parse("20x4", 1).is_err());
        assert_eq!(ThreadForm::Trapezoidal.parse("Tr20x4", 2).unwrap().lead, 8.0);
    }

    /// The design diameters against the standards' tables: ISO 2904 (Tr10x2: d3 7.5, D4 10.5, D1 8;
    /// Tr40x7: 32, 41, 33; Tr100x20: 78, 102, 80), DIN 513 (S10x2: d3 6.529, D1 7; S40x7: 27.851,
    /// 29.5), ISO 68-1 (M10: d1 = 8.376).
    #[test]
    fn design_diameters_match_the_tables() {
        let ext = |f: ThreadForm, d: f64, p: f64| 2.0 * f.groove(d, p, false).root_r;
        let int = |f: ThreadForm, d: f64, p: f64| (2.0 * f.groove(d, p, true).crest_r, 2.0 * f.groove(d, p, true).root_r);
        let tr = ThreadForm::Trapezoidal;
        for (d, p, d3, d4, d1) in [(10.0, 2.0, 7.5, 10.5, 8.0), (40.0, 7.0, 32.0, 41.0, 33.0), (100.0, 20.0, 78.0, 102.0, 80.0)] {
            assert!((ext(tr, d, p) - d3).abs() < 1e-9 && (int(tr, d, p).0 - d1).abs() < 1e-9 && (int(tr, d, p).1 - d4).abs() < 1e-9);
        }
        let s = ThreadForm::Buttress;
        for (d, p, d3, d1) in [(10.0, 2.0, 6.529, 7.0), (40.0, 7.0, 27.851, 29.5)] {
            assert!((ext(s, d, p) - d3).abs() < 5e-4 && (int(s, d, p).0 - d1).abs() < 1e-9, "S{d}x{p}");
        }
        assert!((ext(ThreadForm::Metric, 10.0, 1.5) - 8.376).abs() < 5e-4);
        // DIN 513 flat w = 0.26384 P.
        assert!((s.groove(10.0, 2.0, false).crest_flat - 0.26384 * 2.0).abs() < 1e-5);
        // ASME B1.9 flat 0.16316 P, B1.5 flat 0.3707 P.
        assert!((ThreadForm::ButtressAsme.groove(50.0, 4.0, false).crest_flat - 0.16316 * 4.0).abs() < 1e-4);
        assert!((ThreadForm::Acme.groove(25.4, 5.08, false).crest_flat - 0.3707 * 5.08).abs() < 1e-3);
    }

    /// The basic profiles mesh: at the pitch line (half the basic depth in from the crest) groove
    /// and tooth are each half a pitch (symmetric forms), and an external tooth fits the internal
    /// groove of the same size.
    #[test]
    fn grooves_are_half_a_pitch_at_the_pitch_line() {
        for f in [ThreadForm::Metric, ThreadForm::Trapezoidal, ThreadForm::Acme] {
            let (d, p) = (20.0, 2.5);
            let g = f.groove(d, p, false);
            let r = d / 2.0
                - match f {
                    ThreadForm::Metric => 3.0 * p * 3f64.sqrt() / 16.0,
                    _ => p / 4.0,
                };
            assert!((g.width(p, r) - p / 2.0).abs() < 1e-9, "{f:?}");
        }
    }
}
