//! How a tapered extrusion closes its profile's sharp corners ([`DraftCorner`]), and what each
//! rule gives at each corner of a profile ([`draft_corners`]), so a dialog can offer the others
//! where the chosen one is a poor fit.
//!
//! At the far end the profile is offset by the run of its sides (`distance · tan taper`). Where
//! a corner turns away from that offset the offset curves part, and the rule closes the gap:
//! [`DraftCorner::PreserveCurvature`] continues each curve as itself (a line along its line, an
//! arc along its circle) to where the two cross, [`DraftCorner::Tangent`] continues each straight
//! along its end tangent, [`DraftCorner::Round`] closes it with an arc about the corner. Where the
//! corner turns into the offset the offset curves overlap and every rule trims them where they
//! cross.
use serde::{Deserialize, Serialize};
use solidcraft_geom::{Region2, Seg2, Vec2};

use crate::Result;

/// How a tapered extrusion closes the gap its offset opens at a sharp corner of the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftCorner {
    /// Each wall continues as itself (a line's plane, an arc's cone) until it meets its
    /// neighbour: every section is the profile's curves offset and continued as themselves.
    #[default]
    PreserveCurvature,
    /// Each offset curve continues straight along its end tangent (a flat face each side).
    Tangent,
    /// An arc about the corner (a cone about it).
    Round,
}

impl DraftCorner {
    /// The three rules, in the order a dialog offers them.
    pub const ALL: [DraftCorner; 3] = [DraftCorner::PreserveCurvature, DraftCorner::Tangent, DraftCorner::Round];
}

/// Why a rule cannot close a corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerRefusal {
    /// The continued curves do not meet (two parallel lines, two circles that part).
    Apart,
    /// A curve of the profile is neither a line nor an arc: its wall has no exact continuation.
    NotExact,
    /// A wall at the corner closes before the far end (its arc shrinks to its centre).
    WallCloses,
}

/// What one rule gives at a corner, at the far end of the extrusion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerFill {
    /// How far the offset curve before the corner and the one after it run on past their ends
    /// to close it (negative: trimmed back where they overlap). For [`DraftCorner::Round`] the
    /// arc's length is split between the two.
    pub extension: [f64; 2],
    /// Whether the corner cuts inside the exact offset of the corner itself (its far point is
    /// nearer the profile's corner than the offset distance): the notch preserving curvature
    /// makes where a curve bends back toward the solid.
    pub inward: bool,
}

/// What one rule gives at a corner, or why it cannot close it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerOption {
    pub rule: DraftCorner,
    pub fill: std::result::Result<CornerFill, CornerRefusal>,
}

/// One sharp corner of a profile and what each rule gives there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerReport {
    /// The corner on the profile, in the sketch's coordinates.
    pub at: Vec2,
    /// Whether the offset opens a gap there (the rules differ) or the offset curves overlap
    /// (every rule trims them where they cross).
    pub gap: bool,
    /// [`DraftCorner::ALL`] in order.
    pub options: [CornerOption; 3],
}

/// An offset curve at the far end as its rule continues it: a line or a circle, or neither.
#[derive(Clone, Copy, Debug)]
enum Carrier {
    Line { p: Vec2, d: Vec2 },
    Circle { c: Vec2, r: f64, ccw: bool },
    Free,
}

/// The sharp corners of `region`'s outer loop (the loop a tapered extrusion takes) and what each
/// rule gives at each, for an extrusion `distance` long whose sides run out by `taper` radians
/// (positive grows the profile, as [`crate::extrude_tapered`] takes it).
pub fn draft_corners(region: &Region2, distance: f64, taper: f64) -> Result<Vec<CornerReport>> {
    crate::build::region_ok(region)?;
    if !(distance.is_finite() && taper.is_finite()) {
        return Err(crate::KernelError::Invalid("taper or distance is not finite".into()));
    }
    let outer = crate::build::oriented_outer(region)?;
    let segs = &outer.segs;
    let d = distance * taper.tan();
    let n = segs.len();
    // The walls are built as one exact set: a profile with a sharp corner keeps its curvature only
    // when every curve is a line or an arc.
    let free = segs.iter().any(|s| matches!(carrier(s, d), Carrier::Free));
    let mut out = Vec::new();
    for k in 0..n {
        let (Some(a), Some(b)) = (segs.get((k + n - 1) % n), segs.get(k)) else { continue };
        let (ta, tb) = (a.end_tangent(), b.start_tangent());
        let turn = ta.cross(tb).atan2(ta.dot(tb));
        if turn.abs() <= 1e-9 {
            continue;
        }
        let p = b.start();
        // The outer loop runs counterclockwise: growing moves each curve to its right.
        let (ca, cb) = (carrier(a, d), carrier(b, d));
        let (ea, eb) = (a.end() + right(ta) * d, b.start() + right(tb) * d);
        // A left turn opens a gap when the profile grows, a right turn when it shrinks.
        let gap = (turn > 0.0) == (d > 0.0);
        let curvature = if free { Err(CornerRefusal::NotExact) } else { continued(ca, cb, ea, eb, p, d) };
        let options = if gap {
            [
                CornerOption { rule: DraftCorner::PreserveCurvature, fill: curvature },
                CornerOption { rule: DraftCorner::Tangent, fill: tangent(ea, ta, eb, tb, p, d) },
                CornerOption {
                    rule: DraftCorner::Round,
                    fill: Ok(CornerFill { extension: [0.5 * d.abs() * turn.abs(), 0.5 * d.abs() * turn.abs()], inward: false }),
                },
            ]
        } else {
            // Overlapping offsets: every rule trims them where they cross.
            let trim = curvature.map(|f| CornerFill { inward: false, ..f });
            DraftCorner::ALL.map(|rule| CornerOption { rule, fill: if rule == DraftCorner::PreserveCurvature { curvature } else { trim } })
        };
        out.push(CornerReport { at: p, gap, options });
    }
    Ok(out)
}

fn right(t: Vec2) -> Vec2 {
    let t = t.normalized().unwrap_or(t);
    Vec2::new(t.y, -t.x)
}

/// `s` offset by `d` to its right, as the curve its far-end section continues along.
fn carrier(s: &Seg2, d: f64) -> Carrier {
    match *s {
        Seg2::Line { a, b } => match (b - a).normalized() {
            Some(t) => Carrier::Line { p: a + right(t) * d, d: t },
            None => Carrier::Free,
        },
        Seg2::Arc { center, radius, sweep, .. } => {
            let ccw = sweep > 0.0;
            // Right of a counterclockwise arc is away from its centre.
            Carrier::Circle { c: center, r: if ccw { radius + d } else { radius - d }, ccw }
        }
        _ => Carrier::Free,
    }
}

/// The tangent rule: the end tangents' crossing.
fn tangent(ea: Vec2, ta: Vec2, eb: Vec2, tb: Vec2, p: Vec2, d: f64) -> std::result::Result<CornerFill, CornerRefusal> {
    let (ta, tb) = (ta.normalized().unwrap_or(ta), tb.normalized().unwrap_or(tb));
    let den = ta.cross(tb);
    if den.abs() < 1e-12 {
        return Err(CornerRefusal::Apart);
    }
    let s = (eb - ea).cross(tb) / den;
    let x = ea + ta * s;
    let t = (x - eb).dot(tb);
    Ok(CornerFill { extension: [s, -t], inward: x.dist(p) < d.abs() - 1e-9 })
}

/// The curvature rule: the continued curves' crossing nearest the corner.
fn continued(ca: Carrier, cb: Carrier, ea: Vec2, eb: Vec2, p: Vec2, d: f64) -> std::result::Result<CornerFill, CornerRefusal> {
    for c in [ca, cb] {
        match c {
            Carrier::Free => return Err(CornerRefusal::NotExact),
            Carrier::Circle { r, .. } if r <= 0.0 => return Err(CornerRefusal::WallCloses),
            _ => {}
        }
    }
    let mut cands: Vec<Vec2> = Vec::with_capacity(2);
    match (ca, cb) {
        (Carrier::Line { p: pa, d: da }, Carrier::Line { p: pb, d: db }) => {
            let den = da.cross(db);
            if den.abs() >= 1e-12 {
                cands.push(pa + da * ((pb - pa).cross(db) / den));
            }
        }
        (Carrier::Line { p: l, d: dl }, Carrier::Circle { c, r, .. }) | (Carrier::Circle { c, r, .. }, Carrier::Line { p: l, d: dl }) => {
            let w = l - c;
            let bq = dl.dot(w);
            let disc = bq * bq - (w.len2() - r * r);
            if disc >= 0.0 {
                let root = disc.sqrt();
                cands.extend([l + dl * (-bq - root), l + dl * (-bq + root)]);
            }
        }
        (Carrier::Circle { c: c1, r: r1, .. }, Carrier::Circle { c: c2, r: r2, .. }) => {
            let e = c2 - c1;
            let dd = e.len();
            if dd > 1e-12 {
                let e = e / dd;
                let along = (r1 * r1 - r2 * r2 + dd * dd) / (2.0 * dd);
                let h2 = r1 * r1 - along * along;
                if h2 >= 0.0 {
                    let h = h2.sqrt();
                    cands.extend([c1 + e * along + e.perp() * h, c1 + e * along - e.perp() * h]);
                }
            }
        }
        _ => {}
    }
    let Some(x) = cands.into_iter().min_by(|u, v| u.dist(p).total_cmp(&v.dist(p))) else { return Err(CornerRefusal::Apart) };
    let extension = [run(ca, ea, x), -run(cb, eb, x)];
    Ok(CornerFill { extension, inward: x.dist(p) < d.abs() - 1e-9 })
}

/// How far along a carrier, the way the profile runs, from `from` to `x`.
fn run(c: Carrier, from: Vec2, x: Vec2) -> f64 {
    match c {
        Carrier::Line { d, .. } => (x - from).dot(d),
        Carrier::Circle { c, r, ccw } => {
            let turn = (from - c).cross(x - c).atan2((from - c).dot(x - c));
            r * if ccw { turn } else { -turn }
        }
        Carrier::Free => 0.0,
    }
}
