//! Cams (`joint.cam`): a revolute joint (the cam) drives a follower joint, a slider (a translating
//! follower) or a revolute (an oscillating follower), so that the follower rides the cam's
//! profile. The follower's value over one turn of the cam is worked out from the profile (a closed
//! sketch profile or a closed loop of the cam body's edges) and the follower (a knife edge, a
//! roller or a flat face) and kept on the motion link as a table, which `MotionLink::follow`
//! interpolates; the table is worked out again whenever the design changes.
//!
//! Everything is worked out in the cam's fixed frame C: the frame of the cam joint's other side,
//! in which the cam's profile turns by the joint's value about z. The follower's line (or pivot) is
//! its joint's A-side frame, which stays put whichever side of that joint moves. The follower comes
//! toward the cam from where it starts (a translating follower from far along its line, an
//! oscillating one from its arm pointing away from the cam's axis) and stops where it first
//! touches the profile: the follower stays in contact, as a spring would hold it.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb2, Aabb3, Ring, Vec2, Vec3};

use super::{JointKind, JointOrigin, MotionLink, Size, component_bodies, component_of, flip_of, occurrence_world, size_of};
use crate::{DocError, Document, IDENTITY, Mat, ModelState, Result, apply_point, apply_vector, mat_inverse, mat_mul};

/// Samples of a cam's table over one turn of the cam (one per degree).
pub const CAM_SAMPLES: usize = 360;
/// Points a cam's profile is sampled into at most.
pub const MAX_PROFILE_POINTS: usize = 20_000;
/// Edges a cam's profile is picked from at most.
pub const MAX_PROFILE_EDGES: usize = 64;

/// Where a cam's profile comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CamProfile {
    /// A closed profile of a sketch (feature id) in the cam's component, by the ids of its outer
    /// loop's curves (the profile sharing most of them; with none given, the profile with the
    /// largest outer loop): that outer loop.
    Sketch {
        sketch: u64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        curves: Vec<String>,
    },
    /// Edges of the cam's bodies forming one closed loop: points on them in the component's
    /// coordinates and the edges' persistent names (found first).
    Edges {
        occurrence: u64,
        picks: Vec<Vec3>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        names: Vec<String>,
    },
}

/// What touches the cam.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Follower {
    /// A point (a knife edge seen along the cam's axis).
    #[default]
    Knife,
    /// A roller of radius `radius` about the follower's point.
    Roller { radius: Size },
    /// A flat face square to a translating follower's line.
    Flat,
}

/// A cam coupling's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cam {
    pub profile: CamProfile,
    #[serde(default)]
    pub follower: Follower,
    /// An oscillating follower's knife tip or roller centre, on the follower.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<JointOrigin>,
    /// A translating follower rides the cam from the −z side of its line; an oscillating one
    /// comes to the cam turning the other way.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reverse: bool,
}

/// A value of a periodic table over one turn of `x` (radians): a monotone cubic through the
/// samples (Hermite, with central slopes limited as Fritsch and Carlson do), so it has no kinks
/// and never goes past its two samples (a follower on a dwell stays on it up to a rise). NaN for
/// an empty table or a non-finite `x`.
pub fn lift_at(table: &[f64], x: f64) -> f64 {
    let n = table.len();
    let u = (x.rem_euclid(TAU) / TAU * n as f64).rem_euclid(n as f64);
    if n == 0 || !u.is_finite() {
        return f64::NAN;
    }
    let i = u.floor();
    let f = u - i;
    let i = i as usize;
    let at = |k: usize| table.get(k % n).copied().unwrap_or(f64::NAN);
    let (p0, p1, p2, p3) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
    // The slope at a sample from the steps either side: none where they disagree (an extremum or
    // a flat), else their mean, at most three times the smaller.
    let slope = |a: f64, b: f64| if a * b <= 0.0 { 0.0 } else { 0.5 * (a + b).signum() * (a + b).abs().min(6.0 * a.abs().min(b.abs())) };
    let (m1, m2) = (slope(p1 - p0, p2 - p1), slope(p2 - p1, p3 - p2));
    let (f2, f3) = (f * f, f * f * f);
    (2.0 * f3 - 3.0 * f2 + 1.0) * p1 + (f3 - 2.0 * f2 + f) * m1 + (3.0 * f2 - 2.0 * f3) * p2 + (f3 - f2) * m2
}

fn rot(v: Vec2, a: f64) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Where a translating follower coming along `-d` from far along its line through `g` first
/// touches the closed polygon `poly`: the largest `t` of its point `g + t·d` (its roller's centre
/// for `r > 0`; for `flat`, its face square to `d`). `None` when it misses the profile.
pub fn first_contact_along(poly: &Ring<Vec2>, g: Vec2, d: Vec2, r: f64, flat: bool) -> Option<f64> {
    let n = d.perp();
    let mut best: Option<f64> = None;
    let mut take = |t: f64| {
        if t.is_finite() && best.is_none_or(|b| t > b) {
            best = Some(t);
        }
    };
    if flat {
        for p in poly {
            take((*p - g).dot(d));
        }
        return best;
    }
    for (&p, &q) in poly.edges() {
        let (a, b) = ((p - g).dot(n), (q - g).dot(n));
        // Edges wholly outside the band the follower sweeps.
        if a.min(b) > r || a.max(b) < -r {
            continue;
        }
        if r <= 0.0 {
            if a == b {
                // Along the line: both ends.
                take((p - g).dot(d));
                take((q - g).dot(d));
            } else {
                let k = a / (a - b);
                take((p - g).dot(d) + (q - p).dot(d) * k);
            }
            continue;
        }
        // The roller's centre a radius from the edge: on its two parallels (the foot within the
        // edge) or about its start (each vertex starts one edge).
        let len = p.dist(q);
        if let Some(e) = (q - p).normalized() {
            let m = e.perp();
            let dm = d.dot(m);
            if dm.abs() > 1e-15 {
                for side in [r, -r] {
                    let t = (side - (g - p).dot(m)) / dm;
                    let foot = (g + d * t - p).dot(e);
                    if (0.0..=len).contains(&foot) {
                        take(t);
                    }
                }
            }
        }
        if a.abs() <= r {
            take((p - g).dot(d) + (r * r - a * a).max(0.0).sqrt());
        }
    }
    best
}

/// The angles in [0, 2π) where an oscillating follower's tip touches the closed polygon `poly`:
/// its tip (a roller's centre for `r > 0`) at `arm` from the pivot `pivot`, turned from the angle
/// `start` the way `turn` says (+1 counter-clockwise), sorted.
fn touches_turning(poly: &Ring<Vec2>, pivot: Vec2, arm: f64, r: f64, start: f64, turn: f64) -> Vec<f64> {
    let tip = |a: f64| pivot + Vec2::from_angle(a) * arm;
    let mut out = Vec::new();
    let mut take = |a: f64, keep: &dyn Fn(Vec2) -> bool| {
        let turned = (turn * (a - start)).rem_euclid(TAU);
        if turned.is_finite() && turned < TAU && keep(tip(a)) {
            out.push(turned);
        }
    };
    // Angles of the tip's direction u where u·w = h: both solutions.
    let meet = |w: Vec2, h: f64| -> Vec<f64> {
        let len = w.len();
        if len <= 0.0 || !(h / len).is_finite() || (h / len).abs() > 1.0 {
            return Vec::new();
        }
        let (base, spread) = (w.angle(), (h / len).clamp(-1.0, 1.0).acos());
        vec![base + spread, base - spread]
    };
    for (&p, &q) in poly.edges() {
        let near = dist_to_segment(pivot, p, q);
        let far = (p - pivot).len().max((q - pivot).len());
        if near > arm + r || far < arm - r {
            continue;
        }
        let len = p.dist(q);
        let Some(e) = (q - p).normalized() else { continue };
        let m = e.perp();
        // On a parallel of the edge a radius away (the edge itself for a knife), the foot within it.
        for side in if r > 0.0 { vec![r, -r] } else { vec![0.0] } {
            for a in meet(m * arm, side - (pivot - p).dot(m)) {
                take(a, &|t: Vec2| (0.0..=len).contains(&(t - p).dot(e)));
            }
        }
        if r > 0.0 {
            // A radius from the edge's start: |pivot + arm·u − p| = r.
            let w = p - pivot;
            for a in meet(w, (arm * arm + w.len2() - r * r) / (2.0 * arm)) {
                take(a, &|_| true);
            }
        }
    }
    out.sort_by(f64::total_cmp);
    out
}

/// Whether an oscillating follower's tip, turned `turned` from `start` the way `turn` says,
/// overlaps the profile by more than `slack`.
fn overlaps_turning(poly: &Ring<Vec2>, pivot: Vec2, arm: f64, r: f64, (start, turn): (f64, f64), turned: f64, slack: f64) -> bool {
    let at = pivot + Vec2::from_angle(start + turn * turned) * arm;
    let gap = poly.edges().map(|(p, q)| dist_to_segment(at, *p, *q)).fold(f64::INFINITY, f64::min);
    // Inside counts only clear of the boundary: a knife on an edge touches it.
    gap < r - slack || (gap > slack && poly.contains(at))
}

/// Where an oscillating follower first touches the closed polygon `poly` turning from `start`
/// (the angle it turns through, in (0, 2π)): see `touches_turning`. `None` when it misses the
/// profile or starts on it.
pub fn first_contact_turning(poly: &Ring<Vec2>, pivot: Vec2, arm: f64, r: f64, start: f64, turn: f64) -> Option<f64> {
    if overlaps_turning(poly, pivot, arm, r, (start, turn), 0.0, -f64::EPSILON) {
        return None;
    }
    touches_turning(poly, pivot, arm, r, start, turn).into_iter().find(|t| *t > 0.0)
}

/// Where an oscillating follower that touched the profile at `from` (turned from `start`) is
/// after the cam turned on a little: pushed back to where the profile now lets it be when its tip
/// overlaps the profile (by more than `slack`), else on to where it next touches. It stays on the
/// part of the profile it rides, however the profile crosses its tip's circle elsewhere.
pub fn next_contact_turning(poly: &Ring<Vec2>, pivot: Vec2, arm: f64, r: f64, (start, turn): (f64, f64), from: f64, slack: f64) -> Option<f64> {
    let touches = touches_turning(poly, pivot, arm, r, start, turn);
    // A touch where the tip is clear of the rest of the profile (a roller inside the profile is a
    // radius from the parallels of the edges near the one it overlaps too).
    let clear = |t: &f64| !overlaps_turning(poly, pivot, arm, r, (start, turn), *t, slack);
    if overlaps_turning(poly, pivot, arm, r, (start, turn), from, slack) {
        touches.into_iter().rev().filter(|t| *t < from).find(clear)
    } else {
        touches.into_iter().filter(|t| *t >= from - 1e-12).find(clear)
    }
}

fn dist_to_segment(x: Vec2, p: Vec2, q: Vec2) -> f64 {
    let d = q - p;
    let t = if d.len2() > 0.0 { ((x - p).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    x.dist(p + d * t)
}

/// Everything a cam's table is worked out from, in the cam's fixed frame C.
struct Setup {
    /// The profile at cam value 0.
    poly: Ring<Vec2>,
    /// +1 when the cam turns by +value about C's z (it is the joint's b side), −1 otherwise.
    turn: f64,
    /// A translating follower's line (a point and its unit direction, the slider's +z), or an
    /// oscillating follower's pivot.
    at: Vec2,
    dir: Vec2,
    /// An oscillating follower's arm (pivot to tip) where it is now.
    arm: Option<Vec2>,
    /// The side a translating follower rides from (+1: from +dir), or the way an oscillating one
    /// turns toward the cam (+1: counter-clockwise).
    side: f64,
    /// b's value per mm of the follower's travel along `dir`, or per radian of its turn about z.
    gain: f64,
    radius: f64,
    flat: bool,
    size: f64,
    /// C in the world, and the occurrence the follower's joint moves.
    frame: Mat,
    moving: u64,
}

fn joint(doc: &Document, id: u64) -> Result<&super::Joint> {
    doc.assembly.joints.iter().find(|j| j.id == id).ok_or_else(|| DocError::Unknown(format!("joint {id}")))
}

fn world(doc: &Document, occ: u64) -> Mat {
    if occ == 0 { IDENTITY } else { occurrence_world(doc, occ) }
}

/// The profile's points in its component's coordinates, the occurrence placing them, and the
/// profile's size. `fa`, `fb`: the cam joint's frames (picked edges continue along the edges in
/// the plane square to the cam's axis).
fn profile_points(doc: &Document, st: &ModelState, p: &CamProfile, cam: &super::Joint, (fa, fb): (&Mat, &Mat)) -> Result<(Vec<Vec3>, u64, f64)> {
    let bad = |m: &str| DocError::Invalid(format!("cam: {m}"));
    match p {
        CamProfile::Sketch { sketch, curves } => {
            let sk = st.sketches.iter().find(|s| s.feature == *sketch).ok_or_else(|| bad(&format!("no sketch {sketch}")))?;
            let comp = doc.feature(*sketch).map(|f| f.component).unwrap_or(0);
            let sides: Vec<u64> =
                [cam.b.occurrence, cam.a.occurrence].into_iter().filter(|o| component_of(doc, *o).is_ok_and(|c| c == comp)).collect();
            let occ = match sides.as_slice() {
                [o] => *o,
                [] => return Err(bad("the profile's sketch must be in the component the cam joint turns")),
                _ => return Err(bad("both sides of the cam joint are that sketch's component: pick the cam's edges instead")),
            };
            let lp = &sketch_profile(sk, curves).ok_or_else(|| bad("the sketch has no closed profile (or the cam's profile is gone)"))?.region.outer;
            let too_many = || bad(&format!("the profile is too detailed (more than {MAX_PROFILE_POINTS} points)"));
            let size = bounds_2d(&sampled(lp, 0.1).ok_or_else(too_many)?);
            let pts = sampled(lp, tolerance(size)).ok_or_else(too_many)?;
            let pl = &sk.plane;
            Ok((pts.iter().map(|q| pl.origin + pl.x * q.x + pl.y * q.y).collect(), occ, size))
        }
        CamProfile::Edges { occurrence, picks, names } => {
            if *occurrence != cam.a.occurrence && *occurrence != cam.b.occurrence {
                return Err(bad("the profile's edges must be on the part the cam joint turns"));
            }
            if picks.is_empty() || picks.len() > MAX_PROFILE_EDGES || !picks.iter().all(|p| p.is_finite()) {
                return Err(bad(&format!("pick 1 to {MAX_PROFILE_EDGES} edges")));
            }
            let comp = component_of(doc, *occurrence)?;
            let bodies = component_bodies(doc, st, comp);
            let size = bodies.iter().map(|b| b.body.size()).fold(0.0, f64::max);
            if bodies.is_empty() || !size.is_finite() || size <= 0.0 {
                return Err(bad("the cam has no body"));
            }
            // Each body sampled to its own size (a large part beside the cam leaves the cam fine).
            // Each body's edges with the faces beside them.
            let meshes: Vec<solidcraft_geom::Mesh> = bodies
                .iter()
                .map(|b| b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(tolerance(b.body.size()))))
                .collect::<std::result::Result<_, _>>()?;
            let edges: Vec<(usize, &Vec<Vec3>, &[u32])> = meshes
                .iter()
                .enumerate()
                .flat_map(|(k, m)| m.edges.iter().enumerate().map(move |(i, e)| (k, e, m.edge_faces.get(i).map(Vec::as_slice).unwrap_or(&[]))))
                .collect();
            let mut lines: Vec<&Vec<Vec3>> = Vec::new();
            let mut sides: Vec<(usize, &[u32])> = Vec::new();
            for (k, pick) in picks.iter().enumerate() {
                let mut at = *pick;
                // A named edge is looked for on the body that has it, and only there.
                let mut on: Option<usize> = None;
                if let Some(name) = names.get(k).filter(|n| !n.is_empty()) {
                    let (body, p) = bodies
                        .iter()
                        .enumerate()
                        .find_map(|(i, b)| crate::naming::find_edge(b, name, *pick).ok().map(|p| (i, p)))
                        .ok_or_else(|| bad(&format!("the edge `{name}` no longer exists (edit the cam to pick again)")))?;
                    (on, at) = (Some(body), p);
                }
                let nearest = edges
                    .iter()
                    .filter(|e| on.is_none_or(|b| e.0 == b))
                    .map(|e| {
                        let d = e.1.windows(2).filter_map(|w| match w {
                            [p, q] => Some(at.dist_to_segment(*p, *q)),
                            _ => None,
                        });
                        (d.fold(f64::MAX, f64::min), e)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .filter(|(d, _)| *d <= 1e-3 * size + 1e-3)
                    .ok_or_else(|| bad("no edge there"))?;
                if !lines.iter().any(|l| std::ptr::eq(*l, nearest.1.1)) {
                    lines.push(nearest.1.1);
                    sides.push((nearest.1.0, nearest.1.2));
                }
            }
            // The other edges in the picked edges' plane square to the cam's axis, beside a face of
            // theirs: a loop continues along them (a circle the kernel keeps as two arcs is picked
            // with one click), and not onto another part touching it.
            let f = if *occurrence == cam.b.occurrence { fb } else { fa };
            let axis = Vec3::new(f[2][0], f[2][1], f[2][2]);
            let level = lines.first().and_then(|l| l.first()).map(|p| p.dot(axis)).unwrap_or(0.0);
            // Joins and the plane to the picked edges' own size, not their bodies' (a small cam in a
            // component with a large part must not close a gap of its own size).
            let span = extent(lines.iter().flat_map(|l| l.iter()));
            let (flat, join) = (1e-6 * span + 1e-9, 1e-6 * span + 1e-9);
            let beside = |(k, faces): (usize, &[u32])| sides.iter().any(|(sk, sf)| *sk == k && sf.iter().any(|f| faces.contains(f)));
            let pool: Vec<&Vec<Vec3>> = edges
                .iter()
                .filter(|(k, e, faces)| {
                    !lines.iter().any(|l| std::ptr::eq(*l, *e)) && beside((*k, faces)) && e.iter().all(|p| (p.dot(axis) - level).abs() <= flat)
                })
                .map(|(_, e, _)| *e)
                .collect();
            let loop_ = chain(&lines, &pool, join)
                .ok_or_else(|| bad(&format!("the picked edges must form one closed loop (of at most {MAX_PROFILE_POINTS} points)")))?;
            // The profile's own size (not its bodies'), for the checks on its plane and area.
            let size = extent(loop_.iter());
            Ok((loop_, *occurrence, size))
        }
    }
}

/// Polylines joined end to end into one closed loop, without the repeated joints and the closing
/// point: every one of `lines` once, either way round, continued along `pool` where no line goes
/// on. `None` when they don't close, or close leaving a line out.
fn chain(lines: &[&Vec<Vec3>], pool: &[&Vec<Vec3>], tol: f64) -> Option<Vec<Vec3>> {
    let (first, rest) = lines.split_first()?;
    let mut out: Vec<Vec3> = first.to_vec();
    let (mut used, mut taken) = (vec![false; rest.len()], vec![false; pool.len()]);
    // The pool's ends by their cell of a `tol` grid, so the next one is found among a few.
    let cell = |p: Vec3| -> Option<[i64; 3]> {
        let c = [p.x / tol, p.y / tol, p.z / tol];
        c.iter().all(|v| v.is_finite() && v.abs() < 1e15).then(|| c.map(|v| v.round() as i64))
    };
    let mut ends: std::collections::BTreeMap<[i64; 3], Vec<(usize, bool)>> = std::collections::BTreeMap::new();
    for (k, l) in pool.iter().enumerate() {
        for (p, flip) in [(l.first(), false), (l.last(), true)] {
            if let Some(c) = p.and_then(|p| cell(*p)) {
                ends.entry(c).or_default().push((k, flip));
            }
        }
    }
    let near_end = |end: Vec3, taken: &[bool]| -> Option<(usize, bool)> {
        let c = cell(end)?;
        for d in 0..27i64 {
            let key = [c[0] + d % 3 - 1, c[1] + d / 3 % 3 - 1, c[2] + d / 9 - 1];
            for &(k, flip) in ends.get(&key).into_iter().flatten() {
                let l = pool.get(k)?;
                let p = if flip { l.last()? } else { l.first()? };
                if taken.get(k) == Some(&false) && p.dist(end) <= tol {
                    return Some((k, flip));
                }
            }
        }
        None
    };
    // The next picked polyline on from `end` (a few at most), and whether it runs backwards.
    let next = |end: Vec3, used: &[bool]| {
        rest.iter().enumerate().filter(|(k, _)| used.get(*k) == Some(&false)).find_map(|(k, l)| {
            if l.first()?.dist(end) <= tol {
                Some((k, false))
            } else if l.last()?.dist(end) <= tol {
                Some((k, true))
            } else {
                None
            }
        })
    };
    for _ in 0..rest.len() + pool.len() {
        let end = *out.last()?;
        if out.len() > 3 && out.first()?.dist(end) <= tol {
            break;
        }
        if out.len() > MAX_PROFILE_POINTS {
            return None;
        }
        let (l, flip) = match next(end, &used) {
            Some((k, flip)) => {
                *used.get_mut(k)? = true;
                (*rest.get(k)?, flip)
            }
            None => {
                let (k, flip) = near_end(end, &taken)?;
                *taken.get_mut(k)? = true;
                (*pool.get(k)?, flip)
            }
        };
        if flip {
            out.extend(l.iter().rev().skip(1));
        } else {
            out.extend(l.iter().skip(1));
        }
    }
    if out.len() < 4 || out.len() > MAX_PROFILE_POINTS + 1 || out.first()?.dist(*out.last()?) > tol || !used.iter().all(|u| *u) {
        return None;
    }
    out.pop();
    Some(out)
}

/// The profile of a solved sketch a cam's `curves` name: the one sharing most of them (the larger
/// on a tie), or with none named the one with the largest outer loop. `None` when no profile
/// shares at least half of them (most of the cam's outline gone: a neighbouring region sharing a
/// curve with it is not the cam).
pub fn sketch_profile<'a>(sk: &'a crate::SolvedSketch, curves: &[String]) -> Option<&'a solidcraft_sketch::Profile> {
    if curves.len() > MAX_PROFILE_POINTS {
        return None;
    }
    let named: std::collections::BTreeSet<&String> = curves.iter().collect();
    let shared = |p: &solidcraft_sketch::Profile| p.outer_curves.iter().filter(|c| named.contains(c)).count();
    let area = |p: &solidcraft_sketch::Profile| p.region.outer.signed_area().abs();
    sk.profiles
        .iter()
        .filter(|p| curves.is_empty() || 2 * shared(p) >= curves.len())
        .max_by(|a, b| shared(a).cmp(&shared(b)).then(area(a).total_cmp(&area(b))))
}

/// A loop's polyline within `tol`, segment by segment: `None` past `MAX_PROFILE_POINTS` points.
fn sampled(lp: &solidcraft_geom::Loop2, tol: f64) -> Option<Vec<Vec2>> {
    let mut out = Vec::new();
    for seg in &lp.segs {
        let p = seg.polyline(tol);
        out.extend(p.iter().take(p.len().saturating_sub(1)));
        if out.len() > MAX_PROFILE_POINTS {
            return None;
        }
    }
    Some(out)
}

/// The diagonal of the box around some points (0 for none).
fn extent<'a>(pts: impl Iterator<Item = &'a Vec3>) -> f64 {
    pts.copied().collect::<Aabb3>().diagonal()
}

fn bounds_2d(pts: &[Vec2]) -> f64 {
    pts.iter().copied().collect::<Aabb2>().diagonal()
}

/// The chord error a profile is sampled to: 0.1 µm per 100 mm of its size, at least 0.1 µm.
fn tolerance(size: f64) -> f64 {
    1e-4 * (size / 100.0).max(1.0)
}

/// The joints, frames and profile a cam link's table is worked out from.
fn setup(doc: &Document, st: &ModelState, l: &MotionLink, c: &Cam) -> Result<Setup> {
    let bad = |m: &str| DocError::Invalid(format!("cam: {m}"));
    let (cam, fol) = (joint(doc, l.a)?, joint(doc, l.b)?);
    if cam.kind != JointKind::Revolute {
        return Err(bad("the cam turns on a revolute joint"));
    }
    if !matches!(fol.kind, JointKind::Slider | JointKind::Revolute) {
        return Err(bad("the follower slides (slider) or swings (revolute)"));
    }
    if cam.a.occurrence == cam.b.occurrence {
        return Err(bad("the cam joint joins two parts"));
    }
    let (fa, fb) = (super::resolve_origin(doc, st, &cam.a)?, super::resolve_origin(doc, st, &cam.b)?);
    let (pts, occ, size) = profile_points(doc, st, &c.profile, cam, (&fa, &fb))?;
    if pts.len() < 3 || pts.len() > MAX_PROFILE_POINTS || !size.is_finite() || size <= 0.0 {
        return Err(bad(&format!("the profile must have 3 to {MAX_PROFILE_POINTS} points")));
    }
    let inv = |m: &Mat| mat_inverse(m).ok_or_else(|| bad("degenerate frame"));
    // The cam's fixed frame C (world) and the profile at value 0 in it, as `through` places the
    // joint's sides.
    let on_b = occ == cam.b.occurrence;
    let (frame, local, fixed) = if on_b {
        (mat_mul(&world(doc, cam.a.occurrence), &fa), mat_mul(&flip_of(cam), &inv(&fb)?), cam.a.occurrence)
    } else {
        (mat_mul(&mat_mul(&world(doc, cam.b.occurrence), &fb), &inv(&flip_of(cam))?), inv(&fa)?, cam.b.occurrence)
    };
    let v: Vec<Vec3> = pts.iter().map(|p| apply_point(&local, *p)).collect();
    let mean = v.iter().map(|p| p.z).sum::<f64>() / v.len() as f64;
    if v.iter().any(|p| (p.z - mean).abs() > 1e-6 * size + 1e-9) {
        return Err(bad("the profile must lie in a plane square to the cam's axis"));
    }
    let poly: Ring<Vec2> = v.iter().map(|p| Vec2::new(p.x, p.y)).collect();
    if !poly.iter().all(|p| p.is_finite()) || area(&poly).abs() <= 1e-12 * size * size {
        return Err(bad("the profile encloses no area"));
    }
    // The follower's line or pivot in C: its joint's frame on the side that stays (the side away
    // from the cam's fixed part moves; b when neither side is that part), as `through` places it.
    // The moving side travels (or turns) by +value along its z when it is b, −value when it is a.
    let c_inv = inv(&frame)?;
    let moves_b = follower_is_b(doc, cam, fol, (fixed, occ))?;
    let (ga, gb) = (super::resolve_origin(doc, st, &fol.a)?, super::resolve_origin(doc, st, &fol.b)?);
    let guide = if moves_b {
        mat_mul(&world(doc, fol.a.occurrence), &ga)
    } else {
        mat_mul(&mat_mul(&world(doc, fol.b.occurrence), &gb), &inv(&flip_of(fol))?)
    };
    let to_c = mat_mul(&c_inv, &guide);
    let (g, d) = (apply_point(&to_c, Vec3::ZERO), apply_vector(&to_c, Vec3::Z));
    let (moving, sign_f) = if moves_b { (fol.b.occurrence, 1.0) } else { (fol.a.occurrence, -1.0) };
    let (radius, flat) = match &c.follower {
        Follower::Knife => (0.0, false),
        Follower::Roller { radius } => (size_of(doc, st, radius)?, false),
        Follower::Flat => (0.0, true),
    };
    let side = if c.reverse { -1.0 } else { 1.0 };
    let turn = if on_b { 1.0 } else { -1.0 };
    let at = Vec2::new(g.x, g.y);
    // Where the follower touches now (in C): its tip, or the roller's picked round face.
    let face = match &c.follower {
        Follower::Roller { radius: Size::Circle(o) } => Some(o),
        _ => None,
    };
    // A roller picked by its round face (as its radius or its tip) turns about the face's axis:
    // square to the profile's plane.
    let roller = matches!(c.follower, Follower::Roller { .. });
    for o in [c.tip.as_ref(), face].into_iter().flatten().filter(|o| roller && matches!(o.snap, super::Snap::CircleCenter { .. })) {
        let at = mat_mul(&c_inv, &doc.origin_world(st, o)?);
        if Vec3::new(at[2][0], at[2][1], 0.0).len() > 1e-6 {
            return Err(bad("the roller's axis must be parallel to the cam's"));
        }
    }
    let tip = match c.tip.as_ref().or(face) {
        Some(o) => {
            let t = apply_point(&mat_mul(&c_inv, &doc.origin_world(st, o)?), Vec3::ZERO);
            Some(Vec2::new(t.x, t.y))
        }
        None => None,
    };
    if fol.kind == JointKind::Slider {
        if d.z.abs() > 1e-6 {
            return Err(bad("the follower must slide square to the cam's axis"));
        }
        let dir = Vec2::new(d.x, d.y).normalized().ok_or_else(|| bad("degenerate frame"))?;
        // Its line: along the slider's axis, through its tip (where it is across the line, which
        // doesn't change as it slides), else through the axis.
        let at = tip.map(|t| at + dir.perp() * (t - at).dot(dir.perp())).unwrap_or(at);
        return Ok(Setup { poly, turn, at, dir, arm: None, side, gain: sign_f, radius, flat, size, frame, moving });
    }
    if flat {
        return Err(bad("a flat-faced follower slides (use a slider)"));
    }
    if Vec2::new(d.x, d.y).len() > 1e-6 {
        return Err(bad("the follower must swing about an axis parallel to the cam's"));
    }
    let t = tip.ok_or_else(|| bad("an oscillating follower needs its tip (the knife's point or the roller's centre)"))?;
    if at.len() <= 1e-9 * size {
        return Err(bad("the follower's pivot is on the cam's axis"));
    }
    let arm = t - at;
    if arm.len() <= 1e-9 * size {
        return Err(bad("the follower's tip is on its pivot"));
    }
    // b turns the follower by its value about the joint's z, which is C's z or against it.
    Ok(Setup { poly, turn, at, dir: Vec2::X, arm: Some(arm), side, gain: sign_f * d.z.signum(), radius, flat, size, frame, moving })
}

/// How many moving joints part occurrence `from` from each occurrence it is joined to, over the
/// unsuppressed joints but `skip`: a rigid joint moves nothing, and the root and the grounded
/// occurrences are all fixed to the world (`u64::MAX` here), so they cost nothing either.
fn hops(doc: &Document, from: u64, skip: [u64; 2]) -> std::collections::BTreeMap<u64, usize> {
    const WORLD: u64 = u64::MAX;
    let mut next: std::collections::BTreeMap<u64, Vec<(u64, usize)>> = std::collections::BTreeMap::new();
    let mut join = |a: u64, b: u64, cost: usize| {
        next.entry(a).or_default().push((b, cost));
        next.entry(b).or_default().push((a, cost));
    };
    for j in doc.assembly.joints.iter().filter(|j| !j.suppressed && !skip.contains(&j.id)).take(super::MAX_JOINTS) {
        join(j.a.occurrence, j.b.occurrence, usize::from(j.kind != JointKind::Rigid));
    }
    join(0, WORLD, 0);
    for o in doc.occurrences.iter().filter(|o| o.grounded) {
        join(o.id, WORLD, 0);
    }
    // Breadth first with free steps taken first (each occurrence settled once, at its least cost).
    let mut cost = std::collections::BTreeMap::from([(from, 0usize)]);
    let mut done = std::collections::BTreeSet::new();
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(o) = queue.pop_front() {
        if !done.insert(o) {
            continue;
        }
        let here = cost.get(&o).copied().unwrap_or(0);
        for (other, step) in next.get(&o).into_iter().flatten() {
            let d = here + step;
            if cost.get(other).is_none_or(|c| d < *c) {
                cost.insert(*other, d);
                if *step == 0 {
                    queue.push_front(*other);
                } else {
                    queue.push_back(*other);
                }
            }
        }
    }
    cost
}

/// Whether a follower joint's b side is the follower (its a side the guide): the guide is the side
/// fewer moving joints from the part the cam turns against (`frame`), through the other joints. A guide joined only to
/// the profile's part (`cam`) would turn with the profile: refused.
fn follower_is_b(doc: &Document, cam_joint: &super::Joint, fol: &super::Joint, (frame, cam): (u64, u64)) -> Result<bool> {
    let skip = [cam_joint.id, fol.id];
    let from = hops(doc, frame, skip);
    Ok(match (from.get(&fol.a.occurrence), from.get(&fol.b.occurrence)) {
        (Some(a), Some(b)) => a <= b,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => {
            let from = hops(doc, cam, skip);
            if from.contains_key(&fol.a.occurrence) || from.contains_key(&fol.b.occurrence) {
                return Err(DocError::Invalid(
                    "cam: the follower's guide turns with the cam's profile (put it on the part the cam turns against)".into(),
                ));
            }
            true
        }
    })
}

/// A polygon's signed area, from its first point (so a small profile far from the axis keeps its
/// digits).
fn area(poly: &Ring<Vec2>) -> f64 {
    let o = poly.first().copied().unwrap_or(Vec2::ZERO);
    poly.edges().map(|(p, q)| (*p - o).cross(*q - o)).sum::<f64>() / 2.0
}

/// The follower's places at the samples 0 ..= `CAM_SAMPLES` of a turn, `place(u, prev)` giving
/// it with the cam `u` samples on (from `prev`, where it was, when it is `follows`ed from sample
/// to sample). A followed follower must come back where it started after the turn. A step much
/// larger than the steps either side of it is followed again in finer steps: a smooth rise
/// shrinks with them, a jump (the follower leaving the profile and landing on another part of it)
/// stays one step, and is refused. `floor`: steps no larger than it are not looked at.
fn round(place: &dyn Fn(f64, Option<f64>) -> Result<f64>, follows: bool, floor: f64) -> Result<Vec<f64>> {
    let mut raw: Vec<f64> = Vec::with_capacity(CAM_SAMPLES + 1);
    for k in 0..=CAM_SAMPLES {
        let prev = if follows { raw.last().copied() } else { None };
        raw.push(place(k as f64, prev)?);
    }
    let at = |k: usize| raw.get(k).copied().unwrap_or(f64::NAN);
    if follows && (at(0) - at(CAM_SAMPLES)).abs() > 1e-6 {
        return Err(DocError::Invalid("cam: after a turn of the cam the follower does not come back where it started (it would jump)".into()));
    }
    let step = |k: usize| (at(k + 1) - at(k)).abs();
    for k in 0..CAM_SAMPLES {
        let (d, around) = (step(k), step((k + CAM_SAMPLES - 1) % CAM_SAMPLES).max(step((k + 1) % CAM_SAMPLES)));
        if d <= floor || d <= 3.0 * around {
            continue;
        }
        let (mut here, mut most) = (at(k), 0.0f64);
        for j in 1..=32 {
            let next = place(k as f64 + f64::from(j) / 32.0, follows.then_some(here))?;
            most = most.max((next - here).abs());
            here = next;
        }
        if most > 0.5 * d {
            return Err(DocError::Invalid(format!("cam: the follower would jump near {k} degrees of the cam's turn (it leaves the profile there)")));
        }
    }
    Ok(raw)
}

/// A cam link's table: b's values at a's values 2π·k / `CAM_SAMPLES`, offset left out. A
/// translating follower stands at its first contact coming along its line; an oscillating one
/// starts at its first contact turning from pointing away from the cam's axis and is followed from
/// sample to sample (`round`).
pub fn table(doc: &Document, st: &ModelState, l: &MotionLink, c: &Cam) -> Result<Vec<f64>> {
    let s = setup(doc, st, l, c)?;
    let start = s.at.angle();
    let follows = s.arm.is_some();
    // Where the follower is with the cam `u` samples on: mm along its line, or the angle its arm
    // turned through from pointing away from the cam's axis (the same direction in C at every
    // sample), on from `prev` for an arm.
    let place = |u: f64, prev: Option<f64>| -> Result<f64> {
        // The follower turned back instead of the profile turned on.
        let q = -s.turn * TAU * u / CAM_SAMPLES as f64;
        match s.arm {
            None => first_contact_along(&s.poly, rot(s.at, q), rot(s.dir * s.side, q), s.radius, s.flat)
                .map(|t| t * s.side)
                .ok_or_else(|| DocError::Invalid("cam: the follower's line misses the cam at some angles".into())),
            Some(arm) => {
                let (pivot, from) = (rot(s.at, q), (start + q, s.side));
                match prev {
                    None => first_contact_turning(&s.poly, pivot, arm.len(), s.radius, from.0, from.1),
                    Some(p) => next_contact_turning(&s.poly, pivot, arm.len(), s.radius, from, p, 1e-9 * s.size),
                }
                .ok_or_else(|| DocError::Invalid("cam: the follower's arm does not reach the cam at every angle (or starts on it)".into()))
            }
        }
    };
    let raw = round(&place, follows, if follows { 1e-6 } else { 1e-6 * s.size })?;
    let out: Vec<f64> = raw.iter().take(CAM_SAMPLES).map(|v| s.gain * if follows { start + s.side * v } else { *v }).collect();
    if !out.iter().all(|v| v.is_finite() && v.abs() <= 1e9 * s.size.max(1.0)) {
        return Err(DocError::Invalid("cam: the follower's travel is out of range".into()));
    }
    Ok(out)
}

/// The side a new cam's follower rides it from, as `Cam::reverse`: a translating follower from the
/// side of the cam's axis its bodies are on now, an oscillating one turning toward the cam the way
/// its arm is turned now from pointing away from the cam's axis.
pub fn default_reverse(doc: &Document, st: &ModelState, l: &MotionLink, c: &Cam) -> Result<bool> {
    let s = setup(doc, st, l, &Cam { reverse: false, ..c.clone() })?;
    if let Some(arm) = s.arm {
        let off = (arm.angle() - s.at.angle() + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI;
        return Ok(off < 0.0);
    }
    let comp = component_of(doc, s.moving)?;
    let bb = component_bodies(doc, st, comp).iter().fold(solidcraft_geom::Aabb3::EMPTY, |bb, b| bb.union(&b.mesh().bounds()));
    if bb.min.x > bb.max.x {
        return Ok(false);
    }
    let inv = mat_inverse(&s.frame).ok_or_else(|| DocError::Invalid("cam: degenerate frame".into()))?;
    let centre = apply_point(&mat_mul(&inv, &world(doc, s.moving)), bb.center());
    // The cam's axis is C's z: the follower is on the −dir side of it.
    Ok(Vec2::new(centre.x, centre.y).dot(s.dir) < 0.0)
}

/// How far a translating follower's line passes from the cam's axis (mm; 0 for an oscillating one).
pub fn follower_offset(doc: &Document, st: &ModelState, l: &MotionLink, c: &Cam) -> Result<f64> {
    let s = setup(doc, st, l, c)?;
    Ok(if s.arm.is_some() { 0.0 } else { s.at.cross(s.dir).abs() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A circle of radius `r` about `c` as a polygon of `n` points (its sag is r·(1 − cos π/n)).
    fn circle(c: Vec2, r: f64, n: usize) -> Ring<Vec2> {
        (0..n).map(|k| c + Vec2::from_angle(TAU * k as f64 / n as f64) * r).collect()
    }

    /// An eccentric circle (radius 30, its centre 10 from the axis) turned by θ against a
    /// follower sliding along +x: a knife edge on a line `h` from the axis stands at
    /// 10·cos θ + √(30² − (h − 10·sin θ)²), a roller of radius 5 as the knife on the circle of
    /// radius 35, and a flat face at 10·cos θ + 30.
    #[test]
    fn a_translating_follower_rides_an_eccentric_circle() {
        let (e, r) = (10.0, 30.0);
        let poly = circle(Vec2::new(e, 0.0), r, 4096);
        for k in 0..72 {
            let th = TAU * k as f64 / 72.0 - 0.3;
            // The follower turned back by θ, as `table` does.
            let (d, at) = (rot(Vec2::X, -th), |h: f64| rot(Vec2::new(0.0, h), -th));
            for h in [0.0, 6.0, -12.0] {
                let knife = first_contact_along(&poly, at(h), d, 0.0, false).unwrap_or(f64::NAN);
                let want = e * th.cos() + (r * r - (h - e * th.sin()).powi(2)).sqrt();
                assert!((knife - want).abs() < 1e-4, "knife θ {th} h {h}: {knife} vs {want}");
                let roller = first_contact_along(&poly, at(h), d, 5.0, false).unwrap_or(f64::NAN);
                let want = e * th.cos() + ((r + 5.0).powi(2) - (h - e * th.sin()).powi(2)).sqrt();
                assert!((roller - want).abs() < 1e-4, "roller θ {th} h {h}: {roller} vs {want}");
            }
            let flat = first_contact_along(&poly, Vec2::ZERO, d, 0.0, true).unwrap_or(f64::NAN);
            assert!((flat - (e * th.cos() + r)).abs() < 1e-4, "flat θ {th}: {flat}");
        }
        // A line past the cam misses it.
        assert_eq!(first_contact_along(&poly, Vec2::new(0.0, 50.0), Vec2::X, 0.0, false), None);
        assert_eq!(first_contact_along(&poly, Vec2::new(0.0, 50.0), Vec2::X, 5.0, false), None);
    }

    /// An arm of 50 about (−50, 40) with a roller of 5 on the same cam: the roller's centre is where
    /// the arm's circle meets the circle of radius 35 about the cam's centre, the first of the two
    /// met turning from the arm pointing away from the axis.
    #[test]
    fn an_oscillating_follower_meets_the_offset_circle() {
        let (e, r, roller) = (10.0, 30.0, 5.0);
        let (pivot, arm) = (Vec2::new(-50.0, 40.0), 50.0);
        let start = pivot.angle();
        for k in 0..36 {
            let th = TAU * k as f64 / 36.0;
            let c = Vec2::from_angle(th) * e;
            let poly = circle(c, r, 4096);
            // The two circles meet at m ± n.
            let (dv, l) = (c - pivot, (c - pivot).len());
            let a = (arm * arm - (r + roller).powi(2) + l * l) / (2.0 * l);
            let m = pivot + dv * (a / l);
            let n = dv.perp() * ((arm * arm - a * a).sqrt() / l);
            for turn in [1.0, -1.0] {
                let swept = |p: Vec2| (turn * ((p - pivot).angle() - start)).rem_euclid(TAU);
                let want = swept(m + n).min(swept(m - n));
                let got = first_contact_turning(&poly, pivot, arm, roller, start, turn).unwrap_or(f64::NAN);
                assert!((got - want).abs() < 1e-5, "θ {th} turn {turn}: {got} vs {want}");
            }
        }
        // Followed on from where it touches, it stays on the part of the profile it rides: a C
        // whose two arms both cross the tip's circle, the tip resting on the second arm.
        let c = Ring::from(
            [[-10.0, 20.0], [30.0, 20.0], [30.0, 14.0], [-4.0, 14.0], [-4.0, -14.0], [30.0, -14.0], [30.0, -20.0], [-10.0, -20.0]].map(Vec2::from),
        );
        let (pv, l) = (Vec2::new(40.0, 0.0), 22.0);
        let touches = touches_turning(&c, pv, l, 0.0, 0.0, 1.0);
        let on = |t: f64| (pv + Vec2::from_angle(t) * l).y;
        let first = first_contact_turning(&c, pv, l, 0.0, 0.0, 1.0).unwrap_or(f64::NAN);
        let second = touches.iter().copied().find(|t| on(*t) < 0.0 && *t > first).unwrap_or(f64::NAN);
        assert!(on(first) > 0.0 && second > first, "{touches:?}");
        let stay = next_contact_turning(&c, pv, l, 0.0, (0.0, 1.0), second, 1e-9).unwrap_or(f64::NAN);
        assert!((stay - second).abs() < 1e-12, "{stay} vs {second}");
        // A knife on an edge touches it, it doesn't overlap it; just inside, it does.
        let square = Ring::from([[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(Vec2::from));
        let (pv, l) = (Vec2::new(0.0, -4.0), 3.0);
        let up = std::f64::consts::FRAC_PI_2;
        assert!(!overlaps_turning(&square, pv, l, 0.0, (up, 1.0), 0.0, 1e-9));
        assert!(overlaps_turning(&square, pv, l + 0.01, 0.0, (up, 1.0), 0.0, 1e-9));
        assert!((area(&square.iter().map(|p| *p * 0.004 + Vec2::new(524_288.0, 524_288.0)).collect::<Ring<_>>()) - 6.4e-5).abs() < 1e-12);
        // An arm that can't reach, and one that starts on the cam.
        let poly = circle(Vec2::new(e, 0.0), r, 512);
        assert_eq!(first_contact_turning(&poly, pivot, 5.0, 0.0, start, 1.0), None);
        assert_eq!(first_contact_turning(&poly, Vec2::new(10.0, 0.0), 10.0, 0.0, 0.0, 1.0), None);
    }

    /// An arm whose first contact vanishes mid-turn and lands on another lobe jumps, though it
    /// comes back after the turn: refused. On the eccentric circle it follows smoothly.
    #[test]
    fn a_follower_that_jumps_is_refused() {
        let arm_round = |poly: &Ring<Vec2>, pivot: Vec2, arm: f64| {
            let start = pivot.angle();
            let place = |u: f64, prev: Option<f64>| -> Result<f64> {
                let q = -TAU * u / CAM_SAMPLES as f64;
                let p = rot(pivot, q);
                match prev {
                    None => first_contact_turning(poly, p, arm, 0.0, start + q, 1.0),
                    Some(v) => next_contact_turning(poly, p, arm, 0.0, (start + q, 1.0), v, 1e-9),
                }
                .ok_or_else(|| DocError::Invalid("misses".into()))
            };
            round(&place, true, 1e-6)
        };
        let lobes: Ring<Vec2> = (0..240)
            .map(|k| {
                let t = TAU * f64::from(k) / 240.0;
                Vec2::from_angle(t) * (28.0 + 2.0 / 3.0 + 13.0 * (3.0 * t).cos())
            })
            .collect();
        let e = arm_round(&lobes, Vec2::new(35.0, 0.0), 20.0).map_err(|e| e.to_string()).err().unwrap_or_default();
        assert!(e.contains("jump"), "{e}");
        assert!(arm_round(&circle(Vec2::new(10.0, 0.0), 30.0, 2048), Vec2::new(-50.0, 40.0), 50.0).is_ok());
    }

    /// The table is read between its samples by a periodic cubic: a cosine sampled once a degree
    /// reads back within a millionth anywhere, a turn either way round included; nothing reads
    /// from an empty table or at a non-finite angle.
    #[test]
    fn a_table_reads_smoothly_between_its_samples() {
        let t: Vec<f64> = (0..CAM_SAMPLES).map(|k| (TAU * k as f64 / CAM_SAMPLES as f64).cos()).collect();
        for x in [0.0, 0.0123, 1.0, 3.1, -2.5, 6.2, 40.0, -1e5] {
            assert!((lift_at(&t, x) - x.cos()).abs() < 1e-6, "{x}: {}", lift_at(&t, x));
        }
        assert!(lift_at(&[], 1.0).is_nan() && lift_at(&t, f64::NAN).is_nan() && lift_at(&t, f64::INFINITY).is_nan());
        // A dwell up to a rise reads flat up to the rise: never below the dwell, nor past the rise.
        let mut step = vec![30.0; CAM_SAMPLES];
        for (k, v) in step.iter_mut().enumerate().skip(100).take(60) {
            *v = 30.0 + 0.9 * (k - 99) as f64;
        }
        for k in 0..36_000 {
            let v = lift_at(&step, TAU * k as f64 / 36_000.0);
            assert!((30.0 - 1e-12..=30.0 + 0.9 * 60.0 + 1e-12).contains(&v), "{k}: {v}");
        }
        assert!(lift_at(&[2.0], 1e300).is_finite());
    }

    /// The guide is the side of the follower's joint fewer moving joints from the cam's frame: a
    /// guide on four rigid joints from it beats a piston three moving joints round a slider-crank;
    /// a guide only on the cam's own side is refused.
    #[test]
    fn the_guide_is_the_side_fewer_moving_joints_from_the_frame() {
        let origin = JointOrigin { occurrence: 0, snap: super::super::Snap::Point { point: Vec3::ZERO, z: None, x: None } };
        let joint = |id: u64, kind: JointKind, a: u64, b: u64| super::super::Joint {
            id,
            name: format!("J{id}"),
            kind,
            a: JointOrigin { occurrence: a, ..origin.clone() },
            b: JointOrigin { occurrence: b, ..origin.clone() },
            values: Vec::new(),
            limits: Vec::new(),
            flip: false,
            offset: 0.0,
            angle: 0.0,
            suppressed: false,
            exprs: Default::default(),
        };
        let occ = |id: u64, grounded: bool| crate::Occurrence { id, component: id, parent: 0, name: format!("O{id}"), transform: IDENTITY, grounded };
        // 1 frame (grounded), 2 cam, 3-6 a rigid chain to the guide 7, 8 the piston, 9 crank, 10 rod.
        let mut doc = Document { occurrences: (1..=10).map(|id| occ(id, id == 1)).collect(), ..Document::default() };
        let (r, v, s) = (JointKind::Rigid, JointKind::Revolute, JointKind::Slider);
        doc.assembly.joints = vec![
            joint(1, v, 1, 2),
            joint(2, r, 1, 3),
            joint(3, r, 3, 4),
            joint(4, r, 4, 5),
            joint(5, r, 5, 7),
            joint(6, s, 8, 7),
            joint(7, v, 1, 9),
            joint(8, v, 9, 10),
            joint(9, v, 10, 8),
        ];
        let (cam, fol) = (doc.assembly.joints[0].clone(), doc.assembly.joints[5].clone());
        // The piston (a) follows, the guide (b) stays.
        assert!(matches!(follower_is_b(&doc, &cam, &fol, (1, 2)), Ok(false)));
        // A guide joined only to the cam turns with it.
        doc.assembly.joints[1] = joint(2, r, 2, 3);
        doc.occurrences.iter_mut().for_each(|o| o.grounded = o.id == 2);
        doc.assembly.joints.truncate(6);
        assert!(follower_is_b(&doc, &cam, &fol, (1, 2)).is_err());
    }

    /// Edges join end to end either way round into one closed loop; a gap or a spare edge is no loop.
    #[test]
    fn edges_chain_into_one_closed_loop() {
        let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
        let (a, b, c) = (vec![p(0.0, 0.0), p(1.0, 0.0)], vec![p(0.0, 1.0), p(1.0, 0.0)], vec![p(0.0, 1.0), p(0.0, 0.0)]);
        let loop3 = chain(&[&a, &b, &c], &[], 1e-9).unwrap_or_default();
        assert_eq!(loop3, vec![p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0)]);
        assert_eq!(chain(&[&a, &b], &[], 1e-9), None);
        let d = vec![p(5.0, 5.0), p(6.0, 6.0)];
        assert_eq!(chain(&[&a, &b, &c, &d], &[], 1e-9), None);
        let ring = vec![p(1.0, 0.0), p(0.0, 1.0), p(-1.0, 0.0), p(0.0, -1.0), p(1.0, 0.0)];
        assert_eq!(chain(&[&ring], &[], 1e-9).map(|l| l.len()), Some(4));
        // One pick goes on along the other edges of its plane; a spare pick still fails.
        assert_eq!(chain(&[&a], &[&d, &c, &b], 1e-9).map(|l| l.len()), Some(3));
        assert_eq!(chain(&[&a, &d], &[&c, &b], 1e-9), None);
        // A loop of many short edges goes round from one of them, found by their ends; past the
        // point cap it is refused, without going on round it.
        let ring = |n: usize| -> Vec<Vec<Vec3>> {
            let at = |k: usize| Vec3::new((TAU * k as f64 / n as f64).cos(), (TAU * k as f64 / n as f64).sin(), 0.0) * 100.0;
            (0..n).map(|k| vec![at(k), at(k + 1)]).collect()
        };
        let small = ring(10_000);
        let rest: Vec<&Vec<Vec3>> = small.iter().skip(1).rev().collect();
        assert_eq!(chain(&[&small[0]], &rest, 1e-6).map(|l| l.len()), Some(10_000));
        let big = ring(100_000);
        let rest: Vec<&Vec<Vec3>> = big.iter().skip(1).collect();
        assert_eq!(chain(&[&big[0]], &rest, 1e-7), None);
    }
}
