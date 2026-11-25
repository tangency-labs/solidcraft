//! The pipe route planner behind the `route.*` commands (`route.rs`): a route is a 3D centreline
//! of straight segments plus the fittings named on it, and the plan is the pieces a butt-welded
//! line is built from, the way a routing tool builds a route assembly:
//! - a 90° elbow at every square corner, a tee where three segments meet (two of them in line);
//! - a reducer, a valve, a cap or a weld where the route names one;
//! - one pipe per straight span between fitting ports, cut to the span's length.
//!
//! Sizes: Sch 10S, Sch 40 and Sch 80 pipe by NPS, ASME B16.9 butt-weld fittings (long and short
//! radius elbows, straight and reducing tees, concentric reducers, caps). Pipe outside diameters
//! and walls are ASME B36.10 and B36.19; the copper sizes are ASTM B280 ACR tube with EN 1254
//! end-feed bends, and the stainless sizes are EN 10253-4 tube.
//!
//! Fitting frames (mm), the ports a fitting is placed by:
//! - elbow: port A at the origin, the pipe coming in along +X, the bend turning to +Y; port B at
//!   (A, A, 0) going out along +Y;
//! - tee: centre at the origin, run ports at (∓C, 0, 0), branch port at (0, M, 0) along +Y;
//! - reducer: large port at the origin, small port at (H, 0, 0);
//! - cap: weld end at the origin, the closed end at (E, 0, 0).
//!
//! Everything here is plain arithmetic on the spec: no kernel, no session. `route.rs` turns the
//! pieces into components, bodies and joints.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub type V3 = [f64; 3];
/// A rigid placement, row-major 3 × 4: p' = R p + t.
pub type M34 = [f64; 12];

/// Points closer than this are one route node (mm).
const TOL: f64 = 1e-3;
/// A fitting's point names the node within this distance (mm).
const FIND: f64 = 0.05;
/// The most segments and fittings a route takes.
pub const MAX_SEGMENTS: usize = 2000;
pub const MAX_FITTINGS: usize = 2000;
pub const MAX_VALVES: usize = 200;
/// Coordinates beyond this are refused (mm).
const MAX_COORD: f64 = 1e7;

// ---- sizes ----------------------------------------------------------------------------------

/// NPS, the library OD (mm), the Sch 10S, Sch 40 and Sch 80 walls (mm).
const PIPE: &[(f64, f64, f64, f64, f64)] = &[
    (0.5, 21.34, 2.11, 2.77, 3.73),
    (0.75, 26.67, 2.11, 2.87, 3.91),
    // 7/8 in (22.225 mm) copper tube, EN 1254 22 mm, 1.0 mm wall.
    (0.875, 22.225, 1.0, 1.0, 1.0),
    (1.0, 33.401, 2.77, 3.378, 4.55),
    // ASTM B280 ACR copper tube 1-1/8 in: OD 1.125 in = 28.575 mm, 1.0 mm wall. The elbow is
    // EN 1254's 28 mm end-feed bend, R 17.0 mm.
    (1.125, 28.575, 1.0, 1.0, 1.0),
    (1.25, 42.16, 2.77, 3.56, 4.85),
    (1.5, 48.26, 2.77, 3.68, 5.08),
    (2.0, 60.325, 2.77, 3.911, 5.54),
    (2.5, 73.03, 3.05, 5.16, 7.01),
    (3.0, 88.90, 3.05, 5.49, 7.62),
    (4.0, 114.30, 3.05, 6.02, 8.56),
];
/// EN 10253-4 stainless tube ODs and walls where they differ from the NPS steel table
/// (1 in tube is 33.7 x 1.6, not 33.401 Sch 10S).
const EN_TUBE: &[(f64, f64, f64)] = &[(1.0, 33.7, 1.6), (2.5, 63.5, 1.65)];
/// B16.9 90° long radius elbow, centre to end A.
const ELBOW_LR: &[(f64, f64)] = &[
    (0.5, 38.1),
    (0.75, 28.6),
    // EN 1254 copper end-feed bends: 22 mm tube R 13.0, 28 mm tube R 17.0 mm.
    (0.875, 13.0),
    (1.0, 38.1),
    (1.125, 17.0),
    (1.25, 47.6),
    (1.5, 57.2),
    (2.0, 76.2),
    (2.5, 95.3),
    (3.0, 114.3),
    (4.0, 152.4),
];
/// B16.9 straight tee, centre to end of run and outlet C.
const TEE_C: &[(f64, f64)] = &[
    (0.5, 25.4),
    (0.75, 28.6),
    (1.0, 38.1),
    // EN 1254 copper equal tee, 28 mm: centre to end 15.0 mm.
    (1.125, 15.0),
    (1.25, 47.6),
    (1.5, 57.2),
    (2.0, 63.5),
    (2.5, 76.2),
    (3.0, 85.9),
    (4.0, 104.6),
];
/// B16.9 reducing outlet tee, centre to end of outlet M.
const RTEE_M: &[((f64, f64), f64)] = &[
    ((1.0, 0.75), 38.1),
    ((1.0, 0.5), 38.1),
    // EN 1254 reducing copper tee 28 x 28 x 22: branch centre to end 15.0 mm.
    ((1.125, 0.875), 15.0),
    ((2.0, 1.0), 60.5),
    ((2.0, 1.5), 60.5),
];
/// B16.9 concentric reducer, end to end H.
const RED_H: &[((f64, f64), f64)] = &[
    ((1.0, 0.75), 50.8),
    ((1.0, 0.5), 50.8),
    ((1.25, 1.0), 50.8),
    ((2.0, 1.0), 76.2),
    ((2.0, 1.5), 76.2),
    ((1.5, 1.0), 63.5),
    ((1.5, 1.25), 63.5),
    ((2.0, 1.25), 76.2),
    ((3.0, 2.0), 88.9),
    ((2.5, 2.0), 88.9),
];
/// B16.9 cap, length E.
const CAP_E: &[(f64, f64)] = &[(0.5, 25.4), (0.75, 25.4), (1.0, 38.1), (1.5, 38.1), (2.0, 38.1), (2.5, 38.1), (3.0, 50.8)];

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Whether `n` can name a route: its pieces are `<n>_<kind>_<i>`, so it is one component name.
pub fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn look(t: &[(f64, f64)], n: f64) -> Option<f64> {
    t.iter().find(|(k, _)| same(*k, n)).map(|(_, v)| *v)
}

fn look2(t: &[((f64, f64), f64)], n: f64, m: f64) -> Option<f64> {
    t.iter().find(|((a, b), _)| same(*a, n) && same(*b, m)).map(|(_, v)| *v)
}

fn look3(t: &[(f64, f64, f64)], n: f64) -> Option<(f64, f64, f64)> {
    t.iter().find(|r| same(r.0, n)).copied()
}

/// The pipe schedule a route is built in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Schedule {
    #[default]
    #[serde(rename = "40")]
    S40,
    #[serde(rename = "80")]
    S80,
    #[serde(rename = "10S")]
    S10S,
    /// B36.19 Schedule 40S stainless pipe.
    #[serde(rename = "40S")]
    S40S,
    /// ASTM B280 ACR / EN 1254 copper tube (7/8 in and 1-1/8 in, 1.0 mm wall).
    #[serde(rename = "ACR")]
    Acr,
    /// EN 10253-4 stainless tube (the sizes in `EN_TUBE`).
    #[serde(rename = "EN")]
    En,
}

/// The elbow a square corner takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Elbow {
    #[default]
    LongRadius,
    ShortRadius,
}

/// The pipe OD of NPS `n` in schedule `s`: the library size, except the EN tube sizes whose
/// OD differs from the NPS table.
pub fn od(n: f64, s: Schedule) -> Option<f64> {
    match s {
        Schedule::Acr if same(n, 0.875) => Some(22.225),
        Schedule::Acr if same(n, 1.125) => Some(28.575),
        Schedule::Acr => None,
        Schedule::En => look3(EN_TUBE, n).map(|r| r.1),
        _ if copper_only(n) => None,
        _ => PIPE.iter().find(|r| same(r.0, n)).map(|r| r.1),
    }
}

/// The copper tube sizes in `PIPE` that no steel schedule has: only `Schedule::Acr` takes them.
fn copper_only(n: f64) -> bool {
    same(n, 0.875) || same(n, 1.125)
}

/// The ASME B36.10/B36.19 OD of NPS `n` (reducers and caps).
pub fn od_b3610(n: f64) -> Option<f64> {
    od(n, Schedule::S10S)
}

pub fn wall(n: f64, s: Schedule) -> Option<f64> {
    match s {
        Schedule::Acr if same(n, 0.875) || same(n, 1.125) => Some(1.0),
        Schedule::Acr => None,
        Schedule::En => look3(EN_TUBE, n).map(|r| r.2),
        Schedule::S10S | Schedule::S40 | Schedule::S80 | Schedule::S40S => {
            if copper_only(n) {
                return None;
            }
            let r = PIPE.iter().find(|r| same(r.0, n))?;
            Some(match s {
                Schedule::S10S => r.2,
                Schedule::S40 | Schedule::S40S => r.3,
                Schedule::S80 => r.4,
                Schedule::Acr | Schedule::En => return None,
            })
        }
    }
}

/// Bore of the library OD, and of the B36.10 OD.
fn bore(n: f64, s: Schedule) -> Option<f64> {
    Some(od(n, s)? - 2.0 * wall(n, s)?)
}

fn bore_b3610(n: f64, s: Schedule) -> Option<f64> {
    Some(od_b3610(n)? - 2.0 * wall(n, s)?)
}

/// An elbow's centre to end: B16.9 long radius (1.5 D), short radius (1.0 D, from 1 in up).
pub fn elbow_a(n: f64, e: Elbow) -> Option<f64> {
    match e {
        Elbow::LongRadius => look(ELBOW_LR, n),
        Elbow::ShortRadius if n >= 1.0 && od(n, Schedule::S10S).is_some() => Some(n * 25.4),
        Elbow::ShortRadius => None,
    }
}

pub fn tee_c(n: f64) -> Option<f64> {
    look(TEE_C, n)
}

/// A tee's outlet length: C for a straight tee, M for a reducing outlet.
pub fn tee_m(n: f64, nb: f64) -> Option<f64> {
    if same(n, nb) { tee_c(n) } else { look2(RTEE_M, n, nb) }
}

pub fn reducer_h(n1: f64, n2: f64) -> Option<f64> {
    look2(RED_H, n1, n2)
}

pub fn cap_e(n: f64) -> Option<f64> {
    look(CAP_E, n)
}

/// `1`, `0_75`, `2_5`: an NPS in a name.
pub fn nps_s(n: f64) -> String {
    let t = format!("{n}");
    t.replace('.', "_")
}

/// The size tables, as `route.fittings` lists them.
pub fn library() -> Value {
    let pipes: Vec<Value> = PIPE
        .iter()
        .map(|&(n, o, w10s, w40, w80)| {
            json!({"nps": n, "od": o, "od_b3610": od_b3610(n), "od_en": od(n, Schedule::En), "wall": {"10S": w10s, "40": w40, "80": w80,
                   "ACR": wall(n, Schedule::Acr), "EN": wall(n, Schedule::En)}, "elbow_long_radius": look(ELBOW_LR, n),
                   "elbow_short_radius": elbow_a(n, Elbow::ShortRadius), "tee": tee_c(n), "cap": cap_e(n)})
        })
        .collect();
    let rt: Vec<Value> = RTEE_M.iter().map(|((a, b), m)| json!({"nps": a, "nps_branch": b, "m": m})).collect();
    let red: Vec<Value> = RED_H.iter().map(|((a, b), h)| json!({"nps": a, "nps_small": b, "h": h})).collect();
    json!({"pipes": pipes, "reducing_tees": rt, "reducers": red, "schedules": ["10S", "40", "40S", "80", "ACR", "EN"], "elbows": ["long_radius", "short_radius"]})
}

// ---- vectors --------------------------------------------------------------------------------

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n > 0.0 { mul(a, 1.0 / n) } else { a }
}
fn near(a: V3, b: V3, t: f64) -> bool {
    norm(sub(a, b)) < t
}

/// The placement taking a fitting frame whose x and y point along `x` and `y` (world) to `origin`.
fn frame(x: V3, y: V3, origin: V3) -> M34 {
    let x = unit(x);
    let y = unit(sub(y, mul(x, dot(x, y))));
    let z = cross(x, y);
    [x[0], y[0], z[0], origin[0], x[1], y[1], z[1], origin[1], x[2], y[2], z[2], origin[2]]
}

/// `a` after `b`.
pub fn compose(a: &M34, b: &M34) -> M34 {
    let mut m = [0.0; 12];
    for r in 0..3 {
        for c in 0..3 {
            m[r * 4 + c] = (0..3).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum();
        }
        m[r * 4 + 3] = (0..3).map(|k| a[r * 4 + k] * b[k * 4 + 3]).sum::<f64>() + a[r * 4 + 3];
    }
    m
}

pub fn apply(m: &M34, p: V3) -> V3 {
    [m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3], m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7], m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11]]
}

fn turn(m: &M34, v: V3) -> V3 {
    [m[0] * v[0] + m[1] * v[1] + m[2] * v[2], m[4] * v[0] + m[5] * v[1] + m[6] * v[2], m[8] * v[0] + m[9] * v[1] + m[10] * v[2]]
}

/// Whether `m` is a rotation and translation (no scale, shear or mirror), within 1e-6.
pub fn rigid(m: &M34) -> bool {
    let c = |i: usize| [m[i], m[4 + i], m[8 + i]];
    let (x, y, z) = (c(0), c(1), c(2));
    m.iter().all(|v| v.is_finite())
        && (norm(x) - 1.0).abs() < 1e-6
        && (norm(y) - 1.0).abs() < 1e-6
        && dot(x, y).abs() < 1e-6
        && near(cross(x, y), z, 1e-6)
}

/// The rotation taking unit `a` to unit `b` and `up_a` (square to a) to `up_b` (square to b).
fn rot_to(a: V3, b: V3, up_a: V3, up_b: V3) -> [[f64; 3]; 3] {
    let za = cross(a, up_a);
    let zb = cross(b, up_b);
    let (aa, bb) = ([a, up_a, za], [b, up_b, zb]);
    let mut r = [[0.0; 3]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| bb[k][i] * aa[k][j]).sum();
        }
    }
    r
}

// ---- the spec -------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub a: V3,
    pub b: V3,
    /// The segment's NPS (default: the route's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nps: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fitting {
    /// Spans the segment between its two points.
    Reducer {
        large: V3,
        small: V3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nps: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nps_small: Option<f64>,
        /// The upstream fitting outlet the reducer is fitted onto when its large end runs past
        /// that outlet (a route built with an overlap). The reducer is held there and the
        /// overlap is recorded, not refused.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        joint: Option<V3>,
    },
    /// Its ports on a and b, its spindle along up; `overlap` lets its body run past the next pipe
    /// end (a route built that way), recorded instead of refused.
    Valve {
        lib: String,
        a: V3,
        b: V3,
        up: V3,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        overlap: bool,
    },
    /// Closes an end node.
    Cap { at: V3 },
    /// A butt weld between two straight pipes: the pipe is cut there.
    Weld { at: V3 },
    /// An end left open (a tee outlet, a weld end).
    Open { at: V3 },
}

/// A valve in the route's valve library: its STEP file, its ports and spindle in its part frame;
/// `pre` takes the STEP file's own frame to that part frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Valve {
    pub step: String,
    pub port_a: V3,
    pub port_b: V3,
    pub up: V3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre: Option<M34>,
    /// Kept as given (a catalogue match, a size): not read.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub name: String,
    /// The route's NPS: segments without their own take it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nps: Option<f64>,
    #[serde(default)]
    pub schedule: Schedule,
    #[serde(default)]
    pub elbow: Elbow,
    /// A polyline: each point to the next is a segment at the route's NPS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<V3>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fittings: Vec<Fitting>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub valves: BTreeMap<String, Valve>,
    /// The end a point move holds still (default: the first segment's start).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<V3>,
    /// The display colours of pipe, fittings and valves (`#rrggbb`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colours: Option<BTreeMap<String, String>>,
}

impl Spec {
    /// The segments with their NPS filled in: the polyline's, then the listed ones.
    pub fn all_segments(&self) -> Result<Vec<(V3, V3, f64)>, String> {
        let mut out = Vec::new();
        for w in self.points.windows(2) {
            if let [a, b] = w {
                let n = self.nps.ok_or("`points` need the route's `nps`")?;
                out.push((*a, *b, n));
            }
        }
        for s in &self.segments {
            let n = s.nps.or(self.nps).ok_or("a segment without `nps` needs the route's `nps`")?;
            out.push((s.a, s.b, n));
        }
        Ok(out)
    }

    /// Hostile input first: names, counts, sizes, finite coordinates.
    pub fn check(&self) -> Result<(), String> {
        if !name_ok(&self.name) {
            return Err("`name` must be 1…64 letters, digits, `-` or `_`".into());
        }
        if self.points.len() == 1 {
            return Err("`points` needs at least two points".into());
        }
        let segs = self.points.len().saturating_sub(1) + self.segments.len();
        if segs == 0 {
            return Err("give `points` (a polyline) or `segments`".into());
        }
        if segs > MAX_SEGMENTS || self.fittings.len() > MAX_FITTINGS || self.valves.len() > MAX_VALVES {
            return Err(format!("a route takes at most {MAX_SEGMENTS} segments, {MAX_FITTINGS} fittings and {MAX_VALVES} valves"));
        }
        let fin = |p: &V3| p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD);
        let mut pts: Vec<&V3> = self.points.iter().collect();
        pts.extend(self.segments.iter().flat_map(|s| [&s.a, &s.b]));
        pts.extend(self.start.iter());
        for f in &self.fittings {
            match f {
                Fitting::Reducer { large, small, joint, .. } => {
                    pts.extend([large, small]);
                    pts.extend(joint.iter());
                }
                Fitting::Valve { a, b, up, .. } => pts.extend([a, b, up]),
                Fitting::Cap { at } | Fitting::Weld { at } | Fitting::Open { at } => pts.push(at),
            }
        }
        for v in self.valves.values() {
            pts.extend([&v.port_a, &v.port_b, &v.up]);
            if v.step.is_empty() || v.step.len() > 4096 {
                return Err("a valve's `step` must be the path of its STEP file".into());
            }
            if let Some(m) = &v.pre
                && !rigid(m)
            {
                return Err("a valve's `pre` must be a rigid 3 × 4 matrix".into());
            }
        }
        if let Some(p) = pts.iter().find(|p| !fin(p)) {
            return Err(format!("{p:?}: coordinates must be finite and within {MAX_COORD} mm"));
        }
        for (_, _, n) in self.all_segments()? {
            if od(n, self.schedule).is_none() {
                return Err(format!("NPS {n} is not in the pipe table (route.fittings lists the sizes)"));
            }
        }
        if let Some(c) = &self.colours
            && (c.len() > 8 || c.values().any(|v| v.len() != 7 || !v.starts_with('#') || !v.bytes().skip(1).all(|b| b.is_ascii_hexdigit())))
        {
            return Err("`colours` maps pipe, fitting and valve to #rrggbb".into());
        }
        Ok(())
    }

    /// The spec with every point and direction taken through `m`.
    pub fn transformed(&self, m: &M34) -> Spec {
        let mut s = self.clone();
        let p = |v: &mut V3| *v = apply(m, *v);
        s.points.iter_mut().for_each(p);
        for g in &mut s.segments {
            g.a = apply(m, g.a);
            g.b = apply(m, g.b);
        }
        if let Some(st) = &mut s.start {
            *st = apply(m, *st);
        }
        for f in &mut s.fittings {
            match f {
                Fitting::Reducer { large, small, joint, .. } => {
                    *large = apply(m, *large);
                    *small = apply(m, *small);
                    if let Some(j) = joint {
                        *j = apply(m, *j);
                    }
                }
                Fitting::Valve { a, b, up, .. } => {
                    *a = apply(m, *a);
                    *b = apply(m, *b);
                    *up = turn(m, *up);
                }
                Fitting::Cap { at } | Fitting::Weld { at } | Fitting::Open { at } => *at = apply(m, *at),
            }
        }
        s
    }

    /// Every point of the spec that a move of `old` (route nodes) takes along `delta`.
    fn moved(&self, old: &[V3], delta: V3) -> Spec {
        let mv = |p: &mut V3| {
            if old.iter().any(|q| near(*p, *q, TOL)) {
                *p = add(*p, delta);
            }
        };
        let mut s = self.clone();
        s.points.iter_mut().for_each(mv);
        for g in &mut s.segments {
            mv(&mut g.a);
            mv(&mut g.b);
        }
        for f in &mut s.fittings {
            match f {
                Fitting::Reducer { large, small, joint, .. } => {
                    mv(large);
                    mv(small);
                    if let Some(j) = joint {
                        mv(j);
                    }
                }
                Fitting::Valve { a, b, .. } => {
                    mv(a);
                    mv(b);
                }
                Fitting::Cap { at } | Fitting::Weld { at } | Fitting::Open { at } => mv(at),
            }
        }
        // The anchor is a route node too: when the move takes it, it goes with the points so the
        // next carried move still finds it.
        if let Some(start) = &mut s.start {
            mv(start);
        }
        s
    }
}

// ---- the route graph ------------------------------------------------------------------------

struct Graph {
    pts: Vec<V3>,
    /// [i, j, nps]
    edges: Vec<(usize, usize, f64)>,
}

impl Graph {
    fn new(segs: &[(V3, V3, f64)]) -> Graph {
        let mut g = Graph { pts: Vec::new(), edges: Vec::new() };
        for &(a, b, n) in segs {
            let (i, j) = (g.node(a), g.node(b));
            if i != j {
                g.edges.push((i, j, n));
            }
        }
        g
    }
    fn node(&mut self, p: V3) -> usize {
        if let Some(k) = self.pts.iter().position(|q| near(p, *q, TOL)) {
            return k;
        }
        self.pts.push(p);
        self.pts.len() - 1
    }
    fn find(&self, p: V3) -> Result<usize, String> {
        self.pts.iter().position(|q| near(p, *q, FIND)).ok_or_else(|| format!("no route node at {p:?}"))
    }
    fn pt(&self, k: usize) -> V3 {
        self.pts.get(k).copied().unwrap_or_default()
    }
    fn nps(&self, e: usize) -> f64 {
        self.edges.get(e).map_or(0.0, |x| x.2)
    }
    /// The edges at node k: (edge, unit direction away from k, the other node).
    fn inc(&self, k: usize) -> Vec<(usize, V3, usize)> {
        let mut out = Vec::new();
        for (e, &(i, j, _)) in self.edges.iter().enumerate() {
            if i == k {
                out.push((e, unit(sub(self.pt(j), self.pt(i))), j));
            } else if j == k {
                out.push((e, unit(sub(self.pt(i), self.pt(j))), i));
            }
        }
        out
    }
}

// ---- the plan -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Pipe,
    Elbow,
    Tee,
    Reducer,
    Cap,
    Valve,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Pipe => "pipe",
            Kind::Elbow => "elbow",
            Kind::Tee => "tee",
            Kind::Reducer => "reducer",
            Kind::Cap => "cap",
            Kind::Valve => "valve",
        }
    }
}

/// One piece of the route: a fitting placed by `matrix` (from its frame), or a pipe from `a` to
/// `b`; `ports` are where it is welded to its neighbours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    pub name: String,
    pub kind: Kind,
    /// The fitting's size key (`elbow90_2`, `tee_1x0_75`) or the valve's library key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<M34>,
    pub ports: Vec<V3>,
    pub nps: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nps_small: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<V3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<V3>,
    /// A pipe's cut length (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// How far a valve's body runs past its span (mm), when the route allows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap: Option<f64>,
    /// An elbow's corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<V3>,
    /// An elbow's bend angle in degrees (90 for a square corner).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
}

impl Piece {
    fn new(kind: Kind, nps: f64, ports: Vec<V3>) -> Piece {
        Piece {
            name: String::new(),
            kind,
            lib: None,
            matrix: None,
            ports,
            nps,
            nps_small: None,
            a: None,
            b: None,
            length: None,
            step: None,
            overlap: None,
            corner: None,
            angle: None,
        }
    }
}

fn round4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

/// A fitting's "up" when the route gives none: world Z, or world X for a vertical run.
fn default_up(d: V3) -> V3 {
    if d[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] }
}

/// The pieces of the route, in build order: the spec's reducers and valves, then the node
/// fittings (caps, elbows, tees), then the pipes; each named `<route>_<kind>_<n>`.
pub fn plan(spec: &Spec) -> Result<Vec<Piece>, String> {
    spec.check()?;
    let segs = spec.all_segments()?;
    let g = Graph::new(&segs);
    // (node, edge) → the length of the edge the fitting at that node takes; None: the whole edge.
    let mut occ: BTreeMap<(usize, usize), Option<f64>> = BTreeMap::new();
    let mut pieces = Vec::new();
    let mut used: BTreeSet<usize> = BTreeSet::new();
    let mut named: BTreeMap<usize, &Fitting> = BTreeMap::new();
    for f in &spec.fittings {
        let (a, b) = match f {
            Fitting::Cap { at } | Fitting::Weld { at } | Fitting::Open { at } => {
                named.insert(g.find(*at)?, f);
                continue;
            }
            Fitting::Reducer { large, small, .. } => (*large, *small),
            Fitting::Valve { a, b, .. } => (*a, *b),
        };
        let (ia, ib) = (g.find(a)?, g.find(b)?);
        let e = g
            .inc(ia)
            .into_iter()
            .find(|x| x.2 == ib)
            .map(|x| x.0)
            .ok_or_else(|| format!("fitting ports {a:?} {b:?} are not the ends of one segment"))?;
        occ.insert((ia, e), None);
        occ.insert((ib, e), None);
        used.insert(e);
        let (pa, pb) = (g.pt(ia), g.pt(ib));
        let d = unit(sub(pb, pa));
        let span = norm(sub(pb, pa));
        match f {
            Fitting::Reducer { nps, nps_small, joint, .. } => {
                let other = |k: usize| g.inc(k).into_iter().find(|x| x.0 != e).map(|x| g.nps(x.0));
                let n1 = nps.or_else(|| other(ia)).ok_or("a reducer at an open end needs its `nps`")?;
                let n2 = nps_small.or_else(|| other(ib)).ok_or("a reducer at an open end needs its `nps_small`")?;
                if let (Some(given), Some(pipe)) = (*nps, other(ia))
                    && !same(given, pipe)
                {
                    return Err(format!("the reducer at {pa:?}: its large end is NPS {given} but the pipe there is NPS {pipe}"));
                }
                if let (Some(given), Some(pipe)) = (*nps_small, other(ib))
                    && !same(given, pipe)
                {
                    return Err(format!("the reducer at {pb:?}: its small end is NPS {given} but the pipe there is NPS {pipe}"));
                }
                let h = reducer_h(n1, n2).ok_or_else(|| format!("no {n1} × {n2} reducer in the library"))?;
                if (span - h).abs() > FIND {
                    return Err(format!("reducer {n1} × {n2}: its span {span:.2} mm is not its length H {h} mm"));
                }
                let up = default_up(d);
                let mut pc = Piece::new(Kind::Reducer, n1, vec![pa, pb]);
                pc.matrix = Some(frame(d, sub(up, mul(d, dot(up, d))), pa));
                pc.lib = Some(format!("reducer_{}x{}", nps_s(n1), nps_s(n2)));
                pc.nps_small = Some(n2);
                // A reducer fitted straight onto a fitting's outlet: its large end runs past
                // that outlet, so the outlet is inside the reducer's body. Add the outlet as a
                // contact port (the weld the route really makes) and record the overlap.
                if let Some(j) = joint {
                    let along = dot(sub(*j, pa), d);
                    let off = norm(sub(sub(*j, pa), mul(d, along)));
                    if off <= FIND && along > FIND && along < span - FIND {
                        pc.overlap = Some(round4(along));
                        pc.ports.push(*j);
                    }
                }
                pieces.push(pc);
            }
            Fitting::Valve { lib, up, overlap, .. } => {
                let v = spec.valves.get(lib).ok_or_else(|| format!("valve `{lib}` is not in the route's `valves`"))?;
                let ax = sub(v.port_b, v.port_a);
                let ln = norm(ax);
                if ln < TOL {
                    return Err(format!("valve `{lib}`: its ports are one point"));
                }
                let ax = unit(ax);
                let up_l = unit(sub(v.up, mul(ax, dot(v.up, ax))));
                let up_w = unit(sub(*up, mul(d, dot(*up, d))));
                if norm(up_l) < 0.5 || norm(up_w) < 0.5 {
                    return Err(format!("valve `{lib}`: its spindle (`up`) runs along its ports"));
                }
                let r = rot_to(ax, d, up_l, up_w);
                let rp = [0, 1, 2].map(|i| (0..3).map(|j| r[i][j] * v.port_a[j]).sum::<f64>());
                let t = sub(pa, rp);
                let mut m = [r[0][0], r[0][1], r[0][2], t[0], r[1][0], r[1][1], r[1][2], t[1], r[2][0], r[2][1], r[2][2], t[2]];
                if let Some(pre) = &v.pre {
                    m = compose(&m, pre);
                }
                let mut pc = Piece::new(Kind::Valve, g.nps(e), vec![pa, pb]);
                pc.matrix = Some(m);
                pc.lib = Some(lib.clone());
                pc.step = Some(v.step.clone());
                if (span - ln).abs() > FIND {
                    if !(*overlap && ln > span) {
                        return Err(format!("valve `{lib}` is {ln:.2} mm port to port on a {span:.2} mm span"));
                    }
                    pc.overlap = Some(round4(ln - span));
                }
                pieces.push(pc);
            }
            _ => {}
        }
    }
    for k in 0..g.pts.len() {
        let p = g.pt(k);
        let all = g.inc(k);
        if let Some(f) = named.get(&k) {
            if let Fitting::Weld { .. } = f {
                let straight = matches!(all.as_slice(), [x, y] if (dot(x.1, y.1) + 1.0).abs() <= 1e-9);
                if !straight {
                    return Err(format!("the weld at {p:?} is not between two straight segments"));
                }
                if let [x, y] = all.as_slice()
                    && !same(g.nps(x.0), g.nps(y.0))
                {
                    return Err(format!("the weld at {p:?} joins NPS {} and {}: a weld joins pipes of one size", g.nps(x.0), g.nps(y.0)));
                }
                continue;
            }
            let [(e, d, _)] = all.as_slice() else {
                return Err(format!("the cap or open end at {p:?} is not on an end node"));
            };
            if let Fitting::Cap { .. } = f {
                let n = g.nps(*e);
                let mut pc = Piece::new(Kind::Cap, n, vec![p]);
                pc.matrix = Some(frame(mul(*d, -1.0), default_up(*d), p));
                pc.lib = Some(format!("cap_{}", nps_s(n)));
                cap_e(n).ok_or_else(|| format!("no NPS {n} cap in the library"))?;
                pieces.push(pc);
            }
            occ.entry((k, *e)).or_insert(Some(0.0));
            continue;
        }
        let free: Vec<_> = all.iter().filter(|x| !used.contains(&x.0)).cloned().collect();
        if all.len() == 2 && free.len() == 2 {
            let ((e1, d1, _), (e2, d2, _)) = match free.as_slice() {
                [x, y] => (*x, *y),
                _ => continue,
            };
            if (dot(d1, d2) + 1.0).abs() < 1e-9 {
                continue;
            }
            let turn = (-dot(d1, d2)).clamp(-1.0, 1.0).acos();
            if !turn.is_finite() || turn <= 1e-6 || turn >= std::f64::consts::PI - 1e-6 {
                return Err(format!("the corner at {p:?} has no bend angle (cos {:.4})", dot(d1, d2)));
            }
            let n = g.nps(e1);
            if !same(g.nps(e2), n) {
                return Err(format!("the elbow at {p:?} joins NPS {n} and {}", g.nps(e2)));
            }
            let r = elbow_a(n, spec.elbow).ok_or_else(|| format!("no NPS {n} {:?} elbow in the library", spec.elbow))?;
            // The bend's ends sit at its tangent points: t = R tan(A/2) from the corner. For a
            // 90 deg elbow t = R, the B16.9 centre-to-end; a sloped leg takes the same bend cut
            // to its own angle.
            let t = r * (turn / 2.0).tan();
            if !t.is_finite() || t < 0.0 {
                return Err(format!("the bend at {p:?} needs a tangent length beyond the library's {r} mm radius"));
            }
            let mut pc = Piece::new(Kind::Elbow, n, vec![add(p, mul(d1, t)), add(p, mul(d2, t))]);
            pc.matrix = Some(frame(mul(d1, -1.0), d2, add(p, mul(d1, t))));
            pc.lib = Some(format!("elbow90_{}{}", nps_s(n), if spec.elbow == Elbow::ShortRadius { "_sr" } else { "" }));
            pc.corner = Some(p);
            pc.angle = Some(round4(turn.to_degrees()));
            occ.insert((k, e1), Some(t));
            occ.insert((k, e2), Some(t));
            pieces.push(pc);
        } else if all.len() == 3 && free.len() == 3 {
            let pair = free.iter().flat_map(|x| free.iter().map(move |y| (x, y))).find(|(x, y)| x.0 < y.0 && (dot(x.1, y.1) + 1.0).abs() < 1e-9);
            let Some((r1, r2)) = pair else {
                return Err(format!("the tee at {p:?} has no straight run"));
            };
            let Some(br) = free.iter().find(|x| x.0 != r1.0 && x.0 != r2.0) else { continue };
            if dot(br.1, r1.1).abs() > 1e-6 {
                return Err(format!("the tee at {p:?}: its branch is not square to its run"));
            }
            let (n, nb) = (g.nps(r1.0), g.nps(br.0));
            if !same(g.nps(r2.0), n) {
                return Err(format!("the tee at {p:?} joins NPS {n} and {} on its run", g.nps(r2.0)));
            }
            let c = tee_c(n).ok_or_else(|| format!("no NPS {n} tee in the library"))?;
            let m = tee_m(n, nb).ok_or_else(|| format!("no {n} × {nb} tee in the library"))?;
            let mut pc = Piece::new(Kind::Tee, n, vec![add(p, mul(r1.1, c)), add(p, mul(r2.1, c)), add(p, mul(br.1, m))]);
            pc.matrix = Some(frame(r2.1, br.1, p));
            pc.lib = Some(format!("tee_{}{}", nps_s(n), if same(nb, n) { String::new() } else { format!("x{}", nps_s(nb)) }));
            if !same(nb, n) {
                pc.nps_small = Some(nb);
            }
            occ.insert((k, r1.0), Some(c));
            occ.insert((k, r2.0), Some(c));
            occ.insert((k, br.0), Some(m));
            pieces.push(pc);
        } else if all.len() == 3 {
            // A fitting (a valve or a reducer) owns one of the three arms: no free span is left
            // for a tee, so the junction cannot be connected.
            return Err(format!("a fitting occupies an arm of the junction at {p:?}: no tee fits there"));
        } else if all.len() == 2 && free.len() == 1 {
            // One arm is a fitting's port: a pipe can only run straight into it.
            let Some(f) = free.first() else { continue };
            let Some(o) = all.iter().find(|x| x.0 != f.0) else { continue };
            if (dot(f.1, o.1) + 1.0).abs() > 1e-9 {
                return Err(format!("the fitting at {p:?} meets a pipe at an angle: no elbow fits between them"));
            }
        } else if all.len() == 2 && free.is_empty() {
            // Two fittings meet at the node: their ports must be in line.
            let (Some(a), Some(b)) = (all.first(), all.get(1)) else { continue };
            if (dot(a.1, b.1) + 1.0).abs() > 1e-9 {
                return Err(format!("the fittings at {p:?} meet at an angle: no elbow fits between them"));
            }
        } else if all.len() > 3 {
            return Err(format!("{} segments meet at {p:?}", all.len()));
        }
    }
    // Pipes: each chain of free edges through straight-through nodes.
    let through = |k: usize| {
        let inc = g.inc(k);
        let free: Vec<_> = inc.iter().filter(|x| !used.contains(&x.0)).collect();
        !named.contains_key(&k) && inc.len() == 2 && matches!(free.as_slice(), [x, y] if (dot(x.1, y.1) + 1.0).abs() < 1e-9)
    };
    let mut seen = BTreeSet::new();
    for (e0, &(i0, j0, n)) in g.edges.iter().enumerate() {
        if used.contains(&e0) || seen.contains(&e0) {
            continue;
        }
        seen.insert(e0);
        let mut ends = Vec::new();
        for (start, _) in [(i0, j0), (j0, i0)] {
            let (mut k, mut prev) = (start, e0);
            let mut steps = 0;
            while through(k) && steps <= g.edges.len() {
                steps += 1;
                let Some(e2) = g.inc(k).into_iter().find(|x| x.0 != prev && !used.contains(&x.0)).map(|x| x.0) else { break };
                if !same(g.nps(e2), n) {
                    return Err(format!("the NPS changes at {:?} without a reducer", g.pt(k)));
                }
                seen.insert(e2);
                prev = e2;
                let Some(&(ii, jj, _)) = g.edges.get(e2) else { break };
                k = if ii == k { jj } else { ii };
            }
            ends.push((k, prev));
        }
        let [(ka, ea), (kb, eb)] = ends.as_slice() else { continue };
        let d = unit(sub(g.pt(*kb), g.pt(*ka)));
        let take = |k: usize, e: usize| occ.get(&(k, e)).copied().flatten().unwrap_or(0.0);
        let pa = add(g.pt(*ka), mul(d, take(*ka, *ea)));
        let pb = sub(g.pt(*kb), mul(d, take(*kb, *eb)));
        let ln = dot(sub(pb, pa), d);
        if ln < -0.01 {
            // A reducer fitted onto a fitting's outlet overlaps that outlet; the short edge
            // between the fitting's centre and the reducer's large end is the overlap, not a pipe.
            let overlap = spec.fittings.iter().any(|f| {
                let Fitting::Reducer { large, small, joint: Some(j), .. } = f else { return false };
                if !near(*large, g.pt(*ka), FIND) && !near(*large, g.pt(*kb), FIND) {
                    return false;
                }
                let s = sub(*small, *large);
                let l = norm(s);
                if l <= TOL {
                    return false;
                }
                let along = dot(sub(*j, *large), s) / l;
                let off = norm(sub(sub(*j, *large), mul(unit(s), along)));
                off <= FIND && along > FIND && along < l - FIND
            });
            if !overlap {
                return Err(format!("fittings overlap by {:.2} mm between {:?} and {:?}", -ln, g.pt(*ka), g.pt(*kb)));
            }
        }
        if ln > 0.01 {
            let mut pc = Piece::new(Kind::Pipe, n, vec![pa, pb]);
            pc.a = Some(pa);
            pc.b = Some(pb);
            pc.length = Some(round4(ln));
            pieces.push(pc);
        }
    }
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for p in &mut pieces {
        let base = if p.kind == Kind::Pipe { format!("pipe_{}", nps_s(p.nps)) } else { p.kind.word().to_string() };
        let c = count.entry(base.clone()).or_insert(0);
        *c += 1;
        p.name = format!("{}_{base}_{c}", spec.name);
    }
    Ok(pieces)
}

/// Move the route node at `at` by `delta`. With `carry`, every node beyond it (away from the
/// route's start) moves with it, as a dragged leg does; the fittings named by those points
/// follow. Returns the new spec and how many nodes moved.
pub fn move_point(spec: &Spec, at: V3, delta: V3, carry: bool) -> Result<(Spec, usize), String> {
    let segs = spec.all_segments()?;
    let g = Graph::new(&segs);
    let k0 = g.find(at)?;
    let mut moved: BTreeSet<usize> = BTreeSet::from([k0]);
    if carry {
        let first = segs.first().map(|s| s.0).ok_or("the route has no segments")?;
        let start = g.find(spec.start.unwrap_or(first))?;
        // The path from k0 back to the start: those nodes stay.
        let mut prev: BTreeMap<usize, Option<usize>> = BTreeMap::from([(k0, None)]);
        let mut stack = vec![k0];
        while let Some(x) = stack.pop() {
            for (_, _, o) in g.inc(x) {
                if let std::collections::btree_map::Entry::Vacant(v) = prev.entry(o) {
                    v.insert(Some(x));
                    stack.push(o);
                }
            }
        }
        let mut back = BTreeSet::new();
        let mut p = Some(start);
        while let Some(x) = p {
            if x == k0 {
                break;
            }
            back.insert(x);
            p = prev.get(&x).copied().flatten();
        }
        let mut stack = vec![k0];
        while let Some(x) = stack.pop() {
            for (_, _, o) in g.inc(x) {
                if moved.contains(&o) || back.contains(&o) {
                    continue;
                }
                moved.insert(o);
                stack.push(o);
            }
        }
    }
    let old: Vec<V3> = moved.iter().map(|&k| g.pt(k)).collect();
    Ok((spec.moved(&old, delta), moved.len()))
}

/// The route's nodes: every point segments meet at or end on, in order.
pub fn nodes(spec: &Spec) -> Result<Vec<V3>, String> {
    Ok(Graph::new(&spec.all_segments()?).pts)
}

// ---- fitting geometry -----------------------------------------------------------------------

/// The commands that model a fitting in its own frame, its bodies named `body`.
pub fn fitting_commands(p: &Piece, sched: Schedule, elbow: Elbow, body: &str) -> Result<Vec<(&'static str, Value)>, String> {
    let n = p.nps;
    let miss = || format!("{}: no NPS {n} in the library", p.name);
    let circle = |name: &str, dia: f64| {
        vec![
            ("sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": name})),
            ("sketch.circle.center", json!({"center": [0, 0], "diameter": dia})),
            ("sketch.finish", json!({})),
        ]
    };
    let mut c = Vec::new();
    match p.kind {
        Kind::Elbow => {
            // A bend of the route's own angle on the fitting's centreline radius R: the torus
            // segment starts at the tangent point, its axis R from that point, and turns through
            // the angle the two legs make. A 90 deg elbow is the same solid at 90 deg.
            let r = elbow_a(n, elbow).ok_or_else(miss)?;
            let a = p.angle.unwrap_or(90.0);
            if !a.is_finite() || !(0.0 < a && a < 180.0) {
                return Err(format!("{}: a bend angle must be between 0 and 180 deg", p.name));
            }
            let (o, i) = (od(n, sched).ok_or_else(miss)?, bore(n, sched).ok_or_else(miss)?);
            for (k, dia, cut) in [("o", o, false), ("i", i, true)] {
                let sk = format!("{body}_{k}");
                c.extend(circle(&sk, dia));
                let mut q = json!({"sketch": sk, "axis": {"origin": [0.0, r, 0.0], "dir": [0, 0, 1]}, "angle": format!("{a} deg")});
                if let Some(m) = q.as_object_mut() {
                    if cut {
                        m.insert("operation".into(), json!("cut"));
                        m.insert("targets".into(), json!([body]));
                    } else {
                        m.insert("body_name".into(), json!(body));
                    }
                }
                c.push(("solid.revolve", q));
            }
        }
        Kind::Tee => {
            let nb = p.nps_small.unwrap_or(n);
            let cr = tee_c(n).ok_or_else(miss)?;
            let m = tee_m(n, nb).ok_or_else(miss)?;
            let (o, ob, i, ib) = (
                od(n, sched).ok_or_else(miss)?,
                od(nb, sched).ok_or_else(miss)?,
                bore(n, sched).ok_or_else(miss)?,
                bore(nb, sched).ok_or_else(miss)?,
            );
            c.push(("solid.cylinder", json!({"diameter": o, "height": 2.0 * cr, "base": [-cr, 0, 0], "axis": [1, 0, 0], "body_name": body})));
            c.push((
                "solid.cylinder",
                json!({"diameter": ob, "height": m, "base": [0, 0, 0], "axis": [0, 1, 0], "operation": "join", "targets": [body]}),
            ));
            c.push((
                "solid.cylinder",
                json!({"diameter": i, "height": 2.0 * cr, "base": [-cr, 0, 0], "axis": [1, 0, 0], "operation": "cut", "targets": [body]}),
            ));
            c.push((
                "solid.cylinder",
                json!({"diameter": ib, "height": m, "base": [0, 0, 0], "axis": [0, 1, 0], "operation": "cut", "targets": [body]}),
            ));
        }
        Kind::Reducer => {
            let n2 = p.nps_small.ok_or_else(miss)?;
            let h = reducer_h(n, n2).ok_or_else(miss)?;
            let (o1, o2) = (od_b3610(n).ok_or_else(miss)?, od_b3610(n2).ok_or_else(miss)?);
            let (i1, i2) = (bore_b3610(n, sched).ok_or_else(miss)?, bore_b3610(n2, sched).ok_or_else(miss)?);
            c.push(("solid.cone", json!({"diameter": o1, "top_diameter": o2, "height": h, "base": [0, 0, 0], "axis": [1, 0, 0], "body_name": body})));
            c.push((
                "solid.cone",
                json!({"diameter": i1, "top_diameter": i2, "height": h, "base": [0, 0, 0], "axis": [1, 0, 0], "operation": "cut", "targets": [body]}),
            ));
        }
        Kind::Cap => {
            // A 2:1 elliptical head on a straight skirt: a sphere scaled to half its depth.
            let e = cap_e(n).ok_or_else(miss)?;
            // The cap takes the pipe's own outside diameter and bore: a Schedule EN or ACR
            // route's tube size is not the B36.10 size.
            let (o, i) = (od(n, sched).ok_or_else(miss)?, bore(n, sched).ok_or_else(miss)?);
            let x0 = e - o / 4.0;
            for (dia, nm) in [(o, body.to_string()), (i, format!("{body}_bore"))] {
                let h = format!("{nm}_h");
                c.push(("solid.cylinder", json!({"diameter": dia, "height": x0, "base": [0, 0, 0], "axis": [1, 0, 0], "body_name": nm})));
                c.push(("solid.sphere", json!({"diameter": dia, "center": [x0, 0, 0], "body_name": h})));
                c.push(("solid.scale", json!({"bodies": [h], "factors": [0.5, 1, 1], "origin": [x0, 0, 0]})));
                c.push(("solid.combine", json!({"target": nm, "tools": [h], "operation": "join"})));
            }
            c.push(("solid.combine", json!({"target": body, "tools": [format!("{body}_bore")], "operation": "cut"})));
        }
        Kind::Pipe | Kind::Valve => return Err(format!("{}: not a fitting modelled from the library", p.name)),
    }
    Ok(c)
}

/// The commands that model a pipe where it is (its own frame is the route's).
pub fn pipe_commands(p: &Piece, sched: Schedule, body: &str) -> Result<Vec<(&'static str, Value)>, String> {
    let n = p.nps;
    let miss = || format!("{}: no NPS {n} in the library", p.name);
    let (a, b, ln) = (p.a.ok_or_else(miss)?, p.b.ok_or_else(miss)?, p.length.ok_or_else(miss)?);
    let d = unit(sub(b, a));
    Ok(vec![
        ("solid.cylinder", json!({"diameter": od(n, sched).ok_or_else(miss)?, "height": ln, "base": a, "axis": d, "body_name": body})),
        (
            "solid.cylinder",
            json!({"diameter": bore(n, sched).ok_or_else(miss)?, "height": ln, "base": a, "axis": d, "operation": "cut", "targets": [body]}),
        ),
    ])
}

/// Which piece each piece is welded to, so that every piece is held once by one already held:
/// (holder, piece) index pairs, the first pipe the root. Pieces touching nothing held come back
/// in the second list.
pub fn welds(pieces: &[Piece]) -> (Vec<(usize, usize)>, Vec<usize>) {
    let Some(root) = pieces.iter().position(|p| p.kind == Kind::Pipe).or(if pieces.is_empty() { None } else { Some(0) }) else {
        return (Vec::new(), Vec::new());
    };
    let mut held = vec![root];
    let mut rest: Vec<usize> = (0..pieces.len()).filter(|&i| i != root).collect();
    let mut out = Vec::new();
    'outer: while !rest.is_empty() {
        for (ri, &i) in rest.iter().enumerate() {
            let Some(p) = pieces.get(i) else { continue };
            let q =
                held.iter().copied().find(|&h| pieces.get(h).is_some_and(|hp| p.ports.iter().any(|a| hp.ports.iter().any(|b| near(*a, *b, FIND)))));
            if let Some(h) = q {
                out.push((h, i));
                held.push(i);
                rest.remove(ri);
                continue 'outer;
            }
        }
        break;
    }
    (out, rest)
}
