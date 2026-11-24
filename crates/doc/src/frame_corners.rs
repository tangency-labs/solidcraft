//! Frame corners (#4003): where exactly two members of a frame end, and the planes their ends are
//! cut on. Pure geometry: the members are built long enough ([`extension`]) and cut by the caller.
//!
//! At corner P each member's direction away from P is `w`. Mitre cuts both on the bisecting plane;
//! Butt runs one member, T, through, cut flush with the other's outer face, and cuts the other, U,
//! at T's near face; Coped Butt cuts T as Butt and leaves U to be coped to T's shape.

use std::collections::HashMap;
use std::f64::consts::PI;

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::{DocError, Expr, FeatureName, Result};

/// How two members are finished where they meet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerType {
    #[default]
    None,
    /// Each corner is finished by its angle (the rule is applied where the frame is built).
    Auto,
    Mitre,
    Butt,
    /// Not offered (#17854): an older build's Coped (`coped_butt`), read as Butt with the frame-wide Coped
    /// ([`read_coped`]); and inside the build, a Butt of a Coped frame ([`CornerType::coped`]).
    CopedButt,
}

impl CornerType {
    pub fn is_none(&self) -> bool {
        *self == CornerType::None
    }
    /// Whether the member order means anything: which member runs through.
    pub fn flips(&self) -> bool {
        matches!(self, CornerType::Auto | CornerType::Butt | CornerType::CopedButt)
    }
    /// How a corner of this type is cut on a frame that copes (`coped`, #17854): a Butt as Coped Butt, its butting
    /// member coped to the through member's shape; any other type as it is.
    pub fn coped(self, coped: bool) -> CornerType {
        if coped && self == CornerType::Butt { CornerType::CopedButt } else { self }
    }
}

/// An older build's Coped corner type (`coped_butt`, the frame's or an override's) read as Butt with the frame-wide
/// Coped, which copes every butt joint of the frame (#17854).
pub fn read_coped(corner: &mut CornerType, corners: &mut [CornerOverride], coped: &mut bool) {
    for c in std::iter::once(corner).chain(corners.iter_mut().map(|o| &mut o.kind)) {
        if *c == CornerType::CopedButt {
            *c = CornerType::Butt;
            *coped = true;
        }
    }
}

/// A member of a corner (#4003): its Frame Member (`None`, the only one taken for now: this frame's own) and its
/// line's id, or its pair's `pair<N>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
pub struct MemberRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<FeatureName>,
    pub line: String,
}

/// One corner, crossing or overlap finished its own way, named by the two or more members that meet there, in order
/// (#17854): the first runs through (a butt's through member, the line a crossing keeps whole, the tube an overlap
/// leaves whole), the second makes the corner with it; of three or more, the rest keep their laying order. Read from
/// a test build's shape too: a member as a bare id, `corner` for `kind`, and `flip` ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(from = "RawOverride")]
pub struct CornerOverride {
    pub members: Vec<MemberRef>,
    pub kind: CornerType,
    /// The weld gap at this corner; none: the frame's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[walk(length = "Gap")]
    pub gap: Option<Expr>,
}

impl CornerOverride {
    /// The members' line ids, in order.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|m| m.line.as_str())
    }
}

/// A [`CornerOverride`] as written, in either shape.
#[derive(Deserialize)]
struct RawOverride {
    members: Vec<RawMember>,
    #[serde(alias = "corner")]
    kind: CornerType,
    #[serde(default)]
    gap: Option<Expr>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawMember {
    Line(String),
    Ref(MemberRef),
}

impl From<RawOverride> for CornerOverride {
    fn from(r: RawOverride) -> Self {
        let members = r
            .members
            .into_iter()
            .map(|m| match m {
                RawMember::Line(line) => MemberRef { feature: None, line },
                RawMember::Ref(m) => m,
            })
            .collect();
        CornerOverride { members, kind: r.kind, gap: r.gap }
    }
}

/// An override as the build reads it: its record's lines in order, its type, Flip, whether its first line is not
/// the one today's rules put first of its first two, and its weld gap (mm), the record's or else the frame's
/// (`eval_frame`'s `overrides`).
#[derive(Clone, Debug, PartialEq)]
pub struct Override {
    pub members: Vec<String>,
    pub corner: CornerType,
    pub flip: bool,
    pub gap: f64,
}

/// The largest weld gap (mm).
pub const MAX_GAP: f64 = 1000.0;

/// Weld gap `g` (mm) when it is 0…[`MAX_GAP`]; else why not, `what` naming it.
pub fn checked_gap(g: f64, what: &str) -> Result<f64> {
    if (0.0..=MAX_GAP).contains(&g) { Ok(g) } else { Err(DocError::Invalid(format!("{what} must be 0…{MAX_GAP} mm, not {g} mm"))) }
}

/// `cut` moved `by` (mm) into the side it keeps: a welded end set back for its weld gap. At 0 the cut as it is.
fn set_back(cut: Cut, by: f64) -> Cut {
    if by > 0.0 { Cut { origin: cut.origin + cut.normal * by, normal: cut.normal } } else { cut }
}

/// How close (mm) two line ends must be to meet.
pub const CORNER_TOL: f64 = 1e-3;
/// Members within this length (mm) of each other are equally long.
pub(crate) const EQUAL_LENGTH: f64 = 1.0;
/// Members within this angle (degrees) of a straight run on are in line: nothing to cut.
pub(crate) const IN_LINE_DEG: f64 = 0.01;
/// The sharpest interior angle (degrees) a corner can be finished at.
pub(crate) const MIN_INTERIOR_DEG: f64 = 5.0;

/// Where exactly two members end. `ends[k] = (member index, at_start)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    pub point: Vec3,
    pub ends: [(usize, bool); 2],
}

/// A cut keeps the side where dot(X - origin, normal) >= 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub origin: Vec3,
    pub normal: Vec3,
}

/// The points where exactly two members end and no third member ends or passes, ordered by the
/// lower member index, then the other.
pub fn find_corners(segments: &[(String, Vec3, Vec3)]) -> Vec<Corner> {
    let mut corners: Vec<Corner> = meetings(segments)
        .into_iter()
        .filter_map(|(point, ends)| match ends.as_slice() {
            &[a, b] => Some(Corner { point, ends: [a, b] }),
            _ => None,
        })
        .collect();
    corners.sort_by_key(|c| {
        let [(a, sa), (b, sb)] = c.ends;
        (a, b, sa, sb)
    });
    corners
}

/// The points where three or more members end and no other member ends or passes, each with its ends
/// `(member, at_start)` by member index.
pub fn find_crowded(segments: &[(String, Vec3, Vec3)]) -> Vec<(Vec3, Vec<(usize, bool)>)> {
    meetings(segments).into_iter().filter(|(_, ends)| ends.len() >= 3).collect()
}

/// The points where two or more ends of different members meet, each end there within CORNER_TOL of
/// every other and of no end elsewhere, and no other member passes: each point (the mean of its ends)
/// with its ends `(member, at_start)` by member index.
fn meetings(segments: &[(String, Vec3, Vec3)]) -> Vec<(Vec3, Vec<(usize, bool)>)> {
    let ends: Vec<(usize, bool, Vec3)> =
        segments.iter().enumerate().flat_map(|(m, (_, a, b))| [(m, true, *a), (m, false, *b)]).filter(|e| e.2.is_finite()).collect();
    // Ends within CORNER_TOL of each other fall in the same or neighbouring cells.
    let cell = |p: Vec3| ((p.x / CORNER_TOL).floor() as i64, (p.y / CORNER_TOL).floor() as i64, (p.z / CORNER_TOL).floor() as i64);
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (k, e) in ends.iter().enumerate() {
        grid.entry(cell(e.2)).or_default().push(k);
    }
    // End k and the ends near it, sorted.
    let with_near = |k: usize, p: Vec3| {
        let (x, y, z) = cell(p);
        let mut out = vec![k];
        for dx in -1..=1i64 {
            for dy in -1..=1i64 {
                for dz in -1..=1i64 {
                    let key = (x.wrapping_add(dx), y.wrapping_add(dy), z.wrapping_add(dz));
                    let hits = grid.get(&key).into_iter().flatten();
                    out.extend(hits.filter(|&&j| j != k && ends.get(j).is_some_and(|e| e.2.dist(p) <= CORNER_TOL)));
                }
            }
        }
        out.sort_unstable();
        out
    };
    let mut out = Vec::new();
    for (i, &(_, _, pi)) in ends.iter().enumerate() {
        let here = with_near(i, pi);
        // Each point once, from its first end; every end there near exactly the others.
        if here.len() < 2 || here.first() != Some(&i) || here.iter().skip(1).any(|&j| ends.get(j).is_none_or(|e| with_near(j, e.2) != here)) {
            continue;
        }
        let at: Vec<(usize, bool, Vec3)> = here.iter().filter_map(|&j| ends.get(j).copied()).collect();
        // A member with both ends here has no length: no corner.
        if at.windows(2).any(|w| matches!(w, [a, b] if a.0 == b.0)) {
            continue;
        }
        let (Some(&(_, _, first)), n) = (at.first(), at.len()) else { continue };
        let point = at.iter().skip(1).fold(first, |s, e| s + e.2) * (1.0 / n as f64);
        // ponytail: every point checks every member, O(members × points), fine under MAX_MEMBERS; grid the segments if it shows.
        let crossed = segments.iter().enumerate().any(|(m, (_, a, b))| !at.iter().any(|e| e.0 == m) && point.dist_to_segment(*a, *b) <= CORNER_TOL);
        if !crossed {
            out.push((point, at.into_iter().map(|(m, s, _)| (m, s)).collect()));
        }
    }
    out
}

/// "Corner at (x, y, z) is {what}", each coordinate to 2 decimals without trailing zeros.
pub fn corner_error(p: Vec3, what: &str) -> DocError {
    let r = |c: f64| (c * 100.0).round() / 100.0 + 0.0;
    DocError::Invalid(format!("Corner at ({}, {}, {}) is {what}", r(p.x), r(p.y), r(p.z)))
}

/// Member `m`'s end at `corner`: (m, at_start).
pub fn end_at(corner: &Corner, m: usize) -> Result<(usize, bool)> {
    corner.ends.iter().copied().find(|e| e.0 == m).ok_or_else(|| DocError::Invalid(format!("member {m} is not at the corner it is coped at")))
}

/// A member's unit direction away from the corner at its end `(member, at_start)`.
pub fn away(segments: &[(String, Vec3, Vec3)], (m, at_start): (usize, bool)) -> Option<Vec3> {
    let (_, a, b) = segments.get(m)?;
    (if at_start { *b - *a } else { *a - *b }).normalized()
}

/// Member m's length as drawn ([`through`]'s `length` for members that are whole lines).
pub fn segment_length(segments: &[(String, Vec3, Vec3)]) -> impl Fn(usize) -> f64 + '_ {
    |m| segments.get(m).map_or(0.0, |(_, a, b)| a.dist(*b))
}

/// Index into `corner.ends` (0 or 1) of the member that runs through, unflipped: the longer by
/// `length` (a piece of a line cut where tubes cross ranks by its whole line, as Auto ranks it), then
/// the one nearer the sketch's X axis, then the one drawn first: `drawn[m]` is member m's place in
/// the sketch's drawing order (a member missing from it ranks by its index), never the pick order.
pub fn through(segments: &[(String, Vec3, Vec3)], corner: &Corner, length: &dyn Fn(usize) -> f64, sketch_x: Option<Vec3>, drawn: &[usize]) -> usize {
    let [e0, e1] = corner.ends;
    let pick = |first: bool| if first { 0 } else { 1 };
    let (l0, l1) = (length(e0.0), length(e1.0));
    if (l0 - l1).abs() > EQUAL_LENGTH {
        return pick(l0 > l1);
    }
    if let Some(x) = sketch_x {
        let along = |e| away(segments, e).map_or(0.0, |w| w.dot(x).abs());
        let (c0, c1) = (along(e0), along(e1));
        if (c0 - c1).abs() > 1e-9 {
            return pick(c0 > c1);
        }
    }
    let rank = |m: usize| drawn.get(m).copied().unwrap_or(m);
    pick((rank(e0.0), e0.0) <= (rank(e1.0), e1.0))
}

/// The interior angle (radians) at a corner: π for in line.
pub fn interior(segments: &[(String, Vec3, Vec3)], corner: &Corner) -> f64 {
    let [e0, e1] = corner.ends;
    match (away(segments, e0), away(segments, e1)) {
        (Some(a), Some(b)) => a.dot(b).clamp(-1.0, 1.0).acos(),
        _ => PI,
    }
}

/// How far a placed section reaches from `point` along unit `dir`: the max over the regions'
/// outer loops (lines' vertices and arcs' extremes), each 2D point p mapped as
/// plane.origin + plane.x*p.x + plane.y*p.y.
pub fn reach(plane: &Plane, regions: &[Region2], point: Vec3, dir: Vec3) -> f64 {
    let d = Vec2::new(plane.x.dot(dir), plane.y.dot(dir));
    let base = (plane.origin - point).dot(dir);
    let mut best = f64::NEG_INFINITY;
    for seg in regions.iter().flat_map(|r| &r.outer.segs) {
        let mut take = |p: Vec2| best = best.max(base + p.dot(d));
        match *seg {
            Seg2::Line { a, b } => {
                take(a);
                take(b);
            }
            Seg2::Arc { center, radius, start, sweep } => {
                take(seg.start());
                take(seg.end());
                // The arc's furthest point along d, if the arc passes it.
                let (lo, hi) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                let t = d.y.atan2(d.x);
                let t = t + ((lo - t) / std::f64::consts::TAU).ceil() * std::f64::consts::TAU;
                if t <= hi {
                    take(center + Vec2::from_angle(t) * radius);
                }
            }
            _ => seg.polyline(1e-3).into_iter().for_each(take),
        }
    }
    best
}

/// The cuts at one corner: (member index, at_start, Cut) for each member it cuts. Coped's U gets
/// no plane cut here; it returns T's cut only, plus the pair (t, u) for coping. `placed(member,
/// at)` is the member's section plane placed square to its line at `at`; `length`, `sketch_x` and
/// `drawn` pick the member that runs through ([`through`]). The weld gap `gap` (mm) sets each
/// mitre cut back half of it, square to the mitre plane, and a butt's U cut all of it; T keeps its
/// cut.
pub fn corner_cuts(
    kind: CornerType,
    flip: bool,
    gap: f64,
    corner: &Corner,
    segments: &[(String, Vec3, Vec3)],
    length: &dyn Fn(usize) -> f64,
    placed: &dyn Fn(usize, Vec3) -> Plane,
    regions: &[Region2],
    sketch_x: Option<Vec3>,
    drawn: &[usize],
) -> Result<(Vec<(usize, bool, Cut)>, Option<(usize, usize)>)> {
    let angle = interior(segments, corner);
    // Auto is resolved into a joint by its caller (frame_layout's table); as a kind here it cuts nothing.
    if matches!(kind, CornerType::None | CornerType::Auto) || angle > PI - IN_LINE_DEG.to_radians() {
        return Ok((Vec::new(), None));
    }
    let p = corner.point;
    if angle < MIN_INTERIOR_DEG.to_radians() {
        return Err(corner_error(p, "too sharp to finish"));
    }
    let [e0, e1] = corner.ends;
    let dir = |e: (usize, bool)| away(segments, e).ok_or_else(|| DocError::Invalid(format!("member {} of a corner has no length", e.0)));
    if kind == CornerType::Mitre {
        let n = (dir(e0)? - dir(e1)?).normalized().ok_or_else(|| DocError::Invalid("a mitre corner has no bisector".into()))?;
        let (c0, c1) = (Cut { origin: p, normal: n }, Cut { origin: p, normal: -n });
        return Ok((vec![(e0.0, e0.1, set_back(c0, gap / 2.0)), (e1.0, e1.1, set_back(c1, gap / 2.0))], None));
    }
    let (te, ue) = if (through(segments, corner, length, sketch_x, drawn) == 0) != flip { (e0, e1) } else { (e1, e0) };
    let (t, u) = (-dir(te)?, dir(ue)?);
    // d made square to `axis`: the normal of a side face of the member along axis.
    let across =
        |d: Vec3, axis: Vec3| (d - axis * d.dot(axis)).normalized().ok_or_else(|| DocError::Invalid("a butt corner has no side to meet".into()));
    let finite = |r: f64| if r.is_finite() { Ok(r) } else { Err(DocError::Invalid("the member profile has no area".into())) };
    // T stops at U's outer face, U at T's near face.
    let m = across(t, u)?;
    let t_cut = Cut { origin: p + m * finite(reach(&placed(ue.0, p), regions, p, m))?, normal: -m };
    if kind == CornerType::CopedButt {
        return Ok((vec![(te.0, te.1, t_cut)], Some((te.0, ue.0))));
    }
    let n = across(u, t)?;
    let u_cut = set_back(Cut { origin: p + n * finite(reach(&placed(te.0, p), regions, p, n))?, normal: n }, gap);
    Ok((vec![(te.0, te.1, t_cut), (ue.0, ue.1, u_cut)], None))
}

/// Auto's angled butt (#4003 spec A5) at `corner`: the `long` member cut square to its own line at the corner, and
/// the `short` one on that same plane, keeping its own side, so it sits on the long member's square end. Nothing when
/// either is not at the corner or the long one has no length. The weld gap `gap` (mm) sets the short one's cut back.
pub fn angled_butt_cuts(corner: &Corner, long: usize, short: usize, segments: &[(String, Vec3, Vec3)], gap: f64) -> Vec<(usize, bool, Cut)> {
    let end = |m: usize| corner.ends.iter().copied().find(|e| e.0 == m);
    let (Some(l), Some(s)) = (end(long), end(short)) else { return Vec::new() };
    let Some(w) = away(segments, l) else { return Vec::new() };
    let p = corner.point;
    vec![(l.0, l.1, Cut { origin: p, normal: w }), (s.0, s.1, set_back(Cut { origin: p, normal: -w }, gap))]
}

/// How far past its corner a member must be built for `cut` to cross its whole section, plus
/// 1 mm (never less than 1 mm): `plane` is the member's section placed at the corner `point`, and
/// `away` its unit direction away from the corner. A section point q meets the cut plane
/// ((q - origin)·n) / (away·n) back from the corner, furthest for the q that reaches furthest
/// along n. `None` when the section has no area or the cut runs along the member.
pub fn extension(plane: &Plane, regions: &[Region2], point: Vec3, away: Vec3, cut: &Cut) -> Option<f64> {
    let along = away.dot(cut.normal);
    let back = (reach(plane, regions, point, cut.normal) - (cut.origin - point).dot(cut.normal)) / along;
    (along > 1e-9 && back.is_finite()).then(|| back.max(0.0) + 1.0)
}

#[cfg(test)]
#[path = "frame_corners_tests.rs"]
mod tests;
