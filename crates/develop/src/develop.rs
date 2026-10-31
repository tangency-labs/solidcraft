//! Laying a sheet's panels out flat through its bends and seams.

use std::collections::VecDeque;
use std::f64::consts::PI;

use solidcraft_geom::{Aabb2, Loop2, Region2, Seg2, Vec2, Vec3};

use crate::chart::Flatten;
use crate::fold::Wrap;
use crate::{Anchor, Bend, Corner, CornerRelief, Joint, JointKind, Lap, Panel, Relief, Rigid2, Rip, Sheet, Surface, Tail};

/// The most panels and joints one development takes: a work limit, never a truncation.
const MAX_PANELS: usize = 10_000;
const MAX_JOINTS: usize = 20_000;

/// How far from a unit vector an anchor's direction may be.
const UNIT_TOL: f64 = 1e-9;

/// Points closer than this (mm) along a polyline are one point when a panel's reach is measured.
const REACH_TOL: f64 = 1e-3;

/// A panel's edge this close (mm) to a bend zone's end ends with it.
const EDGE_TOL: f64 = 1e-6;

/// How far (mm) a rip's cut points may lie off the line through its ends and still be drawn as it.
const RIP_TOL: f64 = 1e-9;

/// How far (mm) a torn end's spline may lie from the curve the tear follows: well inside what a
/// press brake holds, so that a couple of spans do; the most spans it takes; and how many points
/// along it are looked at.
const TEAR_TOL: f64 = 1e-3;
const MAX_TEAR_SPANS: usize = 16;
const TEAR_SAMPLES: usize = 64;

/// A sheet laid out flat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Development {
    /// Where each panel's neutral chart lies in the flat frame (a planar panel's neutral chart
    /// is its chart).
    pub placements: Vec<Option<Rigid2>>,
    /// Each panel with material, in the flat frame.
    pub panels: Vec<Option<FlatPanel>>,
    /// The pieces of the flat pattern: the panels that bends and seams hold together. The root's
    /// piece is first and keeps the flat frame; the others are laid beside it, left to right.
    pub pieces: Vec<Vec<usize>>,
    /// Each bend's zone in the flat frame.
    pub zones: Vec<Zone>,
    /// What to take out of each panel before the pieces are joined: its material between its
    /// tangent line and a bend's anchor, along the zone and, where its tail is trimmed, beyond
    /// it. A rip's cut is no trim: it is in its panels' outlines (see [`FlatPanel`]).
    pub trims: Vec<Trim>,
    /// Bends and seams that close a loop, where the layout reached both panels another way.
    pub loops: Vec<Mismatch>,
    /// What the corners' reliefs cut from the flat, from every piece they reach.
    pub cuts: Vec<Region2>,
}

/// A panel laid flat, its rips' cuts taken off. For each loop of its region, `origins` gives what
/// each segment is in the sheet (see [`EdgeFrom`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlatPanel {
    pub region: Region2,
    pub origins: Vec<Vec<EdgeFrom>>,
    /// The largest strain the flat leaves on the panel's neutral surface: none on a plane, a
    /// cylinder or a cone; on a free-form panel what its fit could not avoid, which is more than
    /// a rounding only where the panel is not developable.
    pub stretch: f64,
}

/// What an edge of a flat panel is in its sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeFrom {
    /// Segment `k` of the same loop of the panel's own region, or what a rip left of it.
    Own(usize),
    /// The panel's new edge along joint `j`'s rip, with what joins it to the edges beside it.
    Rip(usize),
}

/// A bend's zone in the flat frame: its region, its edge on the `a` panel's side (`near`) and on
/// the `b` panel's side (`far`), each from the end at the anchors' origin to the other. Its
/// ends join `near[k]` to `far[k]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub joint: usize,
    pub region: Region2,
    pub near: [Vec2; 2],
    pub far: [Vec2; 2],
}

/// What a joint takes from a panel: the region to cut, and the line along which the panel's
/// material ends once it is cut (its tangent line at a bend, its cut edge at a rip).
#[derive(Clone, Debug, PartialEq)]
pub struct Trim {
    pub panel: usize,
    pub joint: usize,
    pub region: Region2,
    pub line: [Vec2; 2],
}

/// How far a joint that closes a loop is from fitting: laid out through the rest of the loop,
/// its second panel lands `distance` (mm) and `angle` (radians) from where the joint itself
/// would put it. A ring of bends and seams never fits; it needs a rip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mismatch {
    pub joint: usize,
    pub distance: f64,
    pub angle: f64,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DevelopError {
    #[error("the sheet has no panel {0}")]
    NoPanel(usize),
    #[error("panel {0}: {1}")]
    Panel(usize, &'static str),
    #[error("joint {0}: {1}")]
    Joint(usize, &'static str),
    #[error("the sheet exceeds the development's work limit")]
    TooLarge,
    #[error("the fold takes a place in space for each of the flat's {0} pieces")]
    Roots(usize),
}

/// A joint's frame in the flat: where its `a` anchor lies, the joint's direction, and the
/// direction from the `a` panel toward the `b` panel.
struct JointFrame {
    origin: Vec2,
    along: Vec2,
    toward_b: Vec2,
}

impl JointFrame {
    fn from_a(joint: &Joint, a: &Rigid2) -> Option<JointFrame> {
        let along = a.vector(joint.a.dir).normalized()?;
        Some(JointFrame { origin: a.point(joint.a.origin), along, toward_b: right_of(along) })
    }

    /// The frame seen from the `b` panel, whose anchor lies `offset` from the `a` one.
    fn from_b(joint: &Joint, b: &Rigid2, offset: f64) -> Option<JointFrame> {
        let along = b.vector(joint.b.dir).normalized()?;
        let toward_b = right_of(along);
        Some(JointFrame { origin: b.point(joint.b.origin) - toward_b * offset, along, toward_b })
    }

    fn place_b(&self, joint: &Joint, offset: f64) -> Option<Rigid2> {
        Rigid2::onto(joint.b.origin, joint.b.dir, self.origin + self.toward_b * offset, self.along)
    }

    fn place_a(&self, joint: &Joint) -> Option<Rigid2> {
        Rigid2::onto(joint.a.origin, joint.a.dir, self.origin, self.along)
    }

    fn at(&self, along: f64, across: f64) -> Vec2 {
        self.origin + self.along * along + self.toward_b * across
    }
}

/// Rotated -90 degrees: with a panel's material on the left of an anchor, the other panel's
/// side.
fn right_of(v: Vec2) -> Vec2 {
    Vec2::new(v.y, -v.x)
}

/// How far the `b` anchor lies from the `a` one in the flat, toward the `b` panel, for a joint
/// the layout follows: for a bend, the zone's width less each panel's setback from its tangent
/// line to its anchor; for a seam, nothing. A rip is not followed.
fn anchor_offset(kind: &JointKind, sheet: &Sheet) -> Option<f64> {
    match kind {
        JointKind::Bend(bend) => Some(bend.allowance(sheet.thickness, sheet.k_factor) - 2.0 * bend.setback(sheet.thickness)),
        JointKind::Seam => Some(0.0),
        JointKind::Rip(_) => None,
    }
}

/// Lays the sheet out flat. Each panel is first put on its neutral surface (a roll's is longer
/// or shorter round it than its sheet), then the `root` panel's neutral chart is the flat frame
/// and every panel that a chain of bends and seams reaches from it is turned and shifted so that
/// each bend's zone fits between its two panels and each seam's panels meet edge to edge. Panels
/// that rips keep apart are laid out the same way as pieces of their own, beside the root's. A
/// joint that closes a loop is laid out once, through the rest of the loop, and reported in
/// [`Development::loops`].
pub fn develop(sheet: &Sheet, root: usize) -> Result<Development, DevelopError> {
    if sheet.panels.len() > MAX_PANELS || sheet.joints.len() > MAX_JOINTS {
        return Err(DevelopError::TooLarge);
    }
    if root >= sheet.panels.len() {
        return Err(DevelopError::NoPanel(root));
    }
    for (j, joint) in sheet.joints.iter().enumerate() {
        check_joint(sheet, j, joint)?;
    }
    // Every panel on its neutral surface, and the joints' anchors with them: from here on the
    // layout works on planar charts.
    let maps: Vec<Flatten> = sheet
        .panels
        .iter()
        .enumerate()
        .map(|(i, p)| Flatten::of(&p.surface, sheet).map_err(|e| DevelopError::Panel(i, e)))
        .collect::<Result<_, _>>()?;
    let mut neutral = Sheet {
        thickness: sheet.thickness,
        k_factor: sheet.k_factor,
        panels: Vec::with_capacity(sheet.panels.len()),
        joints: Vec::with_capacity(sheet.joints.len()),
        corners: sheet.corners.clone(),
    };
    let mut origins = Vec::with_capacity(sheet.panels.len());
    for (i, (panel, map)) in sheet.panels.iter().zip(&maps).enumerate() {
        let (region, from) = map.region(&panel.region).map_err(|e| DevelopError::Panel(i, e))?;
        neutral.panels.push(Panel { name: panel.name.clone(), surface: Surface::Plane, region });
        origins.push(from.into_iter().map(|l| l.into_iter().map(EdgeFrom::Own).collect::<Vec<_>>()).collect::<Vec<_>>());
    }
    // What each rip takes off its panels, worked out on their own surfaces and carried into their
    // neutral charts.
    let mut bands: Vec<[Option<RipBand>; 2]> = Vec::with_capacity(sheet.joints.len());
    for (j, joint) in sheet.joints.iter().enumerate() {
        bands.push(match &joint.kind {
            JointKind::Rip(rip) => rip_bands(sheet, &maps, j, joint, rip)?,
            _ => [None, None],
        });
    }
    for (j, joint) in sheet.joints.iter().enumerate() {
        let moved = |a: &Anchor| -> Result<Anchor, DevelopError> {
            let map = maps.get(a.panel).ok_or(DevelopError::NoPanel(a.panel))?;
            let dir = map.dir(a.origin, a.dir).ok_or(DevelopError::Joint(j, "an anchor has no direction"))?;
            Ok(Anchor { panel: a.panel, origin: map.point(a.origin), dir })
        };
        neutral.joints.push(Joint { a: moved(&joint.a)?, b: moved(&joint.b)?, kind: joint.kind.clone() });
    }
    let stretch: Vec<f64> = maps.iter().map(Flatten::stretch).collect();
    let sheet = &neutral;

    let mut touching: Vec<Vec<usize>> = vec![Vec::new(); sheet.panels.len()];
    for (j, joint) in sheet.joints.iter().enumerate() {
        if anchor_offset(&joint.kind, sheet).is_some() {
            for p in [joint.a.panel, joint.b.panel] {
                touching.get_mut(p).ok_or(DevelopError::NoPanel(p))?.push(j);
            }
        }
    }
    let mut placements: Vec<Option<Rigid2>> = vec![None; sheet.panels.len()];
    let mut followed = vec![false; sheet.joints.len()];
    let mut pieces = Vec::new();
    let mut next = Some(root);
    while let Some(start) = next {
        if let Some(p) = placements.get_mut(start) {
            *p = Some(Rigid2::IDENTITY);
        }
        pieces.push(lay_out(sheet, start, &touching, &mut placements, &mut followed)?);
        next = placements.iter().position(Option::is_none);
    }
    // The pieces beside the root's, left to right, with their lowest edges level.
    let gap = (10.0 * sheet.thickness).max(5.0);
    let bounds: Vec<Option<Aabb2>> = pieces.iter().map(|piece| piece_bounds(sheet, piece, &placements)).collect();
    if let Some(Some(first)) = bounds.first() {
        let mut x = first.max.x + gap;
        for (piece, b) in pieces.iter().zip(&bounds).skip(1) {
            let Some(b) = b else { continue };
            let shift = Vec2::new(x - b.min.x, first.min.y - b.min.y);
            for &p in piece {
                if let Some(Some(place)) = placements.get_mut(p) {
                    place.shift += shift;
                }
            }
            x += b.max.x - b.min.x + gap;
        }
    }

    let mut out = Development { pieces, ..Development::default() };
    out.panels = sheet
        .panels
        .iter()
        .zip(&placements)
        .zip(origins)
        .zip(stretch)
        .map(|(((panel, place), origins), stretch)| match place {
            Some(place) if !panel.region.outer.segs.is_empty() => Some(FlatPanel { region: place.region(&panel.region), origins, stretch }),
            _ => None,
        })
        .collect();
    out.placements = placements;
    let mut sides: Vec<Vec<RipSide>> = (0..sheet.panels.len()).map(|_| Vec::new()).collect();
    for (j, joint) in sheet.joints.iter().enumerate() {
        let (Some(a), Some(b)) = (out.placements.get(joint.a.panel).copied().flatten(), out.placements.get(joint.b.panel).copied().flatten()) else {
            continue;
        };
        let frame = JointFrame::from_a(joint, &a).ok_or(DevelopError::Joint(j, "an anchor has no direction"))?;
        if let Some(offset) = anchor_offset(&joint.kind, sheet)
            && !followed.get(j).copied().unwrap_or(false)
            && let Some(fits) = frame.place_b(joint, offset)
        {
            let distance = b.point(joint.b.origin).dist(fits.point(joint.b.origin));
            out.loops.push(Mismatch { joint: j, distance, angle: fits.inverse().then(&b).angle().abs() });
        }
        match &joint.kind {
            JointKind::Bend(bend) => add_zone(&mut out, sheet, j, joint, bend, &frame, (&a, &b))?,
            JointKind::Seam => {}
            JointKind::Rip(_) => {
                let Some(bands) = bands.get(j) else { continue };
                // Each panel's loops run with its material on the left: the `a` panel's along
                // the rip, the `b` panel's against it.
                for ((band, place), (panel, along)) in bands.iter().zip([&a, &b]).zip([(joint.a.panel, true), (joint.b.panel, false)]) {
                    let Some(band) = band else { continue };
                    let cut: Vec<Seg2> = band.inner.iter().map(|s| place.seg(s)).collect();
                    let ends = band.ends.map(|p| place.point(p));
                    let side = if along {
                        RipSide { joint: j, ends, middle: place.point(band.middle), cut }
                    } else {
                        RipSide {
                            joint: j,
                            ends: [ends[1], ends[0]],
                            middle: place.point(band.middle),
                            cut: cut.iter().rev().map(Seg2::reversed).collect(),
                        }
                    };
                    sides.get_mut(panel).ok_or(DevelopError::NoPanel(panel))?.push(side);
                }
            }
        }
    }
    for (flat, sides) in out.panels.iter_mut().zip(&sides) {
        if let Some(flat) = flat
            && !sides.is_empty()
        {
            *flat = cut_by_rips(flat, sides)?;
        }
    }
    out.cuts = sheet.corners.iter().filter_map(|c| corner_cut(&out, c)).collect();
    Ok(out)
}

/// Lays out the piece that bends and seams hold together with `start`, already placed: a
/// breadth-first walk that places each panel from the first joint that reaches it. Returns the
/// piece's panels.
fn lay_out(
    sheet: &Sheet,
    start: usize,
    touching: &[Vec<usize>],
    placements: &mut [Option<Rigid2>],
    followed: &mut [bool],
) -> Result<Vec<usize>, DevelopError> {
    let mut piece = vec![start];
    let mut queue = VecDeque::from([start]);
    while let Some(panel) = queue.pop_front() {
        let Some(at) = placements.get(panel).copied().flatten() else { continue };
        for &j in touching.get(panel).map(Vec::as_slice).unwrap_or_default() {
            let (Some(joint), Some(false)) = (sheet.joints.get(j), followed.get(j).copied()) else { continue };
            let Some(offset) = anchor_offset(&joint.kind, sheet) else { continue };
            let (other, placed) = if joint.a.panel == panel {
                (joint.b.panel, JointFrame::from_a(joint, &at).and_then(|f| f.place_b(joint, offset)))
            } else {
                (joint.a.panel, JointFrame::from_b(joint, &at, offset).and_then(|f| f.place_a(joint)))
            };
            if placements.get(other).copied().flatten().is_some() {
                continue;
            }
            let placed = placed.ok_or(DevelopError::Joint(j, "an anchor has no direction"))?;
            if let Some(slot) = placements.get_mut(other) {
                *slot = Some(placed);
            }
            if let Some(f) = followed.get_mut(j) {
                *f = true;
            }
            piece.push(other);
            queue.push_back(other);
        }
    }
    Ok(piece)
}

/// The flat bounding box of a piece's material.
fn piece_bounds(sheet: &Sheet, piece: &[usize], placements: &[Option<Rigid2>]) -> Option<Aabb2> {
    let mut b: Option<Aabb2> = None;
    for &p in piece {
        let (Some(panel), Some(Some(place))) = (sheet.panels.get(p), placements.get(p)) else { continue };
        if panel.region.outer.segs.is_empty() {
            continue;
        }
        let pb = place.loop2(&panel.region.outer).polyline(REACH_TOL).bounds();
        b = Some(b.map_or(pb, |b| b.union(&pb)));
    }
    b
}

fn check_joint(sheet: &Sheet, j: usize, joint: &Joint) -> Result<(), DevelopError> {
    for anchor in [&joint.a, &joint.b] {
        if anchor.panel >= sheet.panels.len() {
            return Err(DevelopError::NoPanel(anchor.panel));
        }
        if !anchor.origin.is_finite() || !((anchor.dir.len() - 1.0).abs() <= UNIT_TOL) {
            return Err(DevelopError::Joint(j, "an anchor's direction is not a unit vector"));
        }
    }
    // A rip may part a panel from itself: a tube cut open along its own seam.
    if joint.a.panel == joint.b.panel && !matches!(joint.kind, JointKind::Rip(_)) {
        return Err(DevelopError::Joint(j, "both anchors are on one panel"));
    }
    match &joint.kind {
        JointKind::Bend(bend) => {
            if !(bend.angle > 0.0 && bend.angle <= PI) || !(bend.radius >= 0.0) || !bend.radius.is_finite() {
                return Err(DevelopError::Joint(j, "a bend turns more than 0 and at most a half turn, round a radius of 0 or more"));
            }
            if bend.datum == crate::Datum::Sharp && !(bend.setback(sheet.thickness).is_finite() && bend.angle < PI) {
                return Err(DevelopError::Joint(j, "a half turn has no virtual sharp: anchor it at its tangent lines"));
            }
            let (near, far) = (bend.ends.near, bend.ends.far);
            if ![near.0, near.1, far.0, far.1].iter().all(|v| v.is_finite()) {
                return Err(DevelopError::Joint(j, "the zone's ends are not numbers"));
            }
            let flat = |a: &Anchor| sheet.panels.get(a.panel).is_some_and(|p| p.surface == Surface::Plane);
            if !(flat(&joint.a) && flat(&joint.b)) {
                return Err(DevelopError::Joint(j, "not supported yet: a bend on the edge of a roll"));
            }
        }
        JointKind::Seam => {}
        JointKind::Rip(rip) => {
            let n = rip.turns.len();
            if n < 2 || rip.edges.0.len() != n || rip.edges.1.len() != n {
                return Err(DevelopError::Joint(j, "a rip's edge has two or more points on each panel, and a turn at each"));
            }
            let finite = |s: &Seg2| s.start().is_finite() && s.end().is_finite();
            if !rip.edges.0.iter().chain(&rip.edges.1).all(|p| p.is_finite())
                || !rip.turns.iter().all(|t| t.is_finite())
                || !rip.curves.0.iter().chain(&rip.curves.1).all(finite)
            {
                return Err(DevelopError::Joint(j, "a rip's edge is not numbers"));
            }
            let fine = match rip.lap {
                Lap::Miter { gap } => gap >= 0.0 && gap.is_finite(),
                Lap::Cut(a, b) => a >= 0.0 && b >= 0.0 && a.is_finite() && b.is_finite(),
            };
            if !fine {
                return Err(DevelopError::Joint(j, "a rip cuts 0 or more from each side"));
            }
        }
    }
    Ok(())
}

/// What a rip leaves of one of its panels, in the panel's neutral chart: the panel's new edge,
/// its edge moved into the panel by the cut at each point, square to the edge on the panel's own
/// surface; and where its edge along the rip starts, ends and has its middle, for finding that
/// stretch of the panel's outline. All run the edge's way.
struct RipBand {
    inner: Vec<Seg2>,
    ends: [Vec2; 2],
    middle: Vec2,
}

/// The bands a rip takes off its panels `[a, b]` (see [`RipBand`]), `None` where it takes
/// nothing. The cut at each point of the edge is the lap's for the sheet's turn there; the way
/// square to the edge is found with the panel's surface metric, so every kind of surface, and
/// every course the edge takes on it, is one computation. Where the cut is the same all along
/// and the edge is known exactly as lines and arcs in a plane, it is moved exactly.
fn rip_bands(sheet: &Sheet, maps: &[Flatten], j: usize, joint: &Joint, rip: &Rip) -> Result<[Option<RipBand>; 2], DevelopError> {
    let cuts: Vec<(f64, f64)> = rip.turns.iter().map(|turn| rip.lap.cuts(*turn, sheet.thickness)).collect();
    let off = || DevelopError::Joint(j, "a rip's edge leaves its panel's surface");
    let mut out = [None, None];
    // The `a` panel's material lies on the left of the edge's way, the `b` panel's on the right.
    for (slot, (panel, edge, side)) in out.iter_mut().zip([(joint.a.panel, &rip.edges.0, 1.0), (joint.b.panel, &rip.edges.1, -1.0)]) {
        let depth: Vec<f64> = cuts.iter().map(|c| if side > 0.0 { c.0 } else { c.1 }).collect();
        if !depth.iter().any(|d| *d > 0.0) {
            continue;
        }
        let (Some(surface), Some(map)) = (sheet.panels.get(panel).map(|p| &p.surface), maps.get(panel)) else {
            return Err(DevelopError::NoPanel(panel));
        };
        let (Some(first), Some(last)) = (edge.first(), edge.last()) else { return Err(off()) };
        let middle = if edge.len() == 2 { first.lerp(*last, 0.5) } else { edge.get(edge.len() / 2).copied().unwrap_or(*first) };
        let (ends, middle) = ([map.point(*first), map.point(*last)], map.point(middle));
        // A plane's chart is its neutral chart.
        let curve = if side > 0.0 { &rip.curves.0 } else { &rip.curves.1 };
        let even = depth.first().filter(|d0| depth.iter().all(|d| (*d - **d0).abs() <= RIP_TOL));
        if let (Surface::Plane, Some(d)) = (surface, even)
            && let Some(inner) = moved(curve, side * d)
        {
            *slot = Some(RipBand { inner, ends, middle });
            continue;
        }
        let mut inner = Vec::with_capacity(edge.len());
        for (k, (q, d)) in edge.iter().zip(&depth).enumerate() {
            let way = edge_way(edge, k).ok_or_else(off)?;
            let [e, f, g] = crate::fold::metric(surface, *q).ok_or_else(off)?;
            let dot = |x: Vec2, y: Vec2| x.x * y.x * e + (x.x * y.y + x.y * y.x) * f + x.y * y.y * g;
            // Square to the edge on the surface, toward the panel's material, a unit long there.
            let across = Vec2::new(-way.y, way.x) * side;
            let square = across - way * (dot(across, way) / dot(way, way));
            let length = dot(square, square).sqrt();
            if !(length > 0.0) {
                return Err(off());
            }
            inner.push(map.point(*q + square * (*d / length)));
        }
        *slot = Some(RipBand { inner: curve_through(&inner), ends, middle });
    }
    Ok(out)
}

/// `curve` moved `d` to the left of its way (to its right for `d` below 0), exactly: in a plane
/// a line moves to a line and a circular arc to an arc about the same center. None for a
/// free-form span, for an arc that would turn inside out, and for pieces that would come apart.
fn moved(curve: &[Seg2], d: f64) -> Option<Vec<Seg2>> {
    let out: Vec<Seg2> = curve
        .iter()
        .map(|s| match *s {
            Seg2::Line { a, b } => {
                let u = (b - a).normalized()?;
                let left = Vec2::new(-u.y, u.x) * d;
                Some(Seg2::Line { a: a + left, b: b + left })
            }
            // Turning counter-clockwise, the left is toward the center.
            Seg2::Arc { center, radius, start, sweep } => {
                let radius = radius - d * sweep.signum();
                (radius > 0.0).then_some(Seg2::Arc { center, radius, start, sweep })
            }
            _ => None,
        })
        .collect::<Option<_>>()?;
    let whole = out.windows(2).all(|w| matches!(w, [p, q] if p.end().dist(q.start()) <= RIP_TOL));
    (whole && !out.is_empty()).then_some(out)
}

/// The way an edge given as points runs at its `k`th point: across its neighbors, and at its ends
/// to second order, (-3 p0 + 4 p1 - p2) / 2, so that a curve's ends lean no more than its middle.
fn edge_way(edge: &[Vec2], k: usize) -> Option<Vec2> {
    let at = |i: usize| edge.get(i).copied();
    let n = edge.len();
    match (k, n) {
        (_, 0 | 1) => None,
        (_, 2) => Some(at(1)? - at(0)?),
        (0, _) => Some((at(1)? * 4.0 - at(0)? * 3.0 - at(2)?) * 0.5),
        (k, n) if k + 1 == n => Some((at(k)? * 3.0 - at(k - 1)? * 4.0 + at(k - 2)?) * 0.5),
        (k, _) => Some((at(k + 1)? - at(k - 1)?) * 0.5),
    }
}

/// A rip's cut on one of its panels, in the flat frame, run the way the panel's loops run (its
/// material on the left): the panel's edge along the rip from `ends[0]` through `middle` to
/// `ends[1]`, and the panel's new edge in its place.
struct RipSide {
    joint: usize,
    ends: [Vec2; 2],
    middle: Vec2,
    cut: Vec<Seg2>,
}

/// A stretch of a loop's new outline, each edge with what it is.
type Stretch = Vec<(Seg2, EdgeFrom)>;

/// How near (mm) a rip's ends lie to its panel's outline, and two edges' ends that meet.
const ON_OUTLINE: f64 = 1e-6;
/// How near (mm) the middle of a rip's edge, drawn by its points, lies to the outline's curve.
const MIDDLE_TOL: f64 = 1e-3;
/// Chords each curved edge is looked at as, to find where two edges cross.
const CROSSING_CHORDS: usize = 16;
/// How far, in gaps it closes, a join carries two edges on to where they meet. Past that they
/// run on nearly straight, and are stepped across instead of carried on in a long sliver.
const MITER_LIMIT: f64 = 4.0;
/// How far, in gaps it closes, a join finds two edges crossing: a crossing farther from the
/// corner is another part of the outline, not this corner's.
const CROSSING_LIMIT: f64 = 50.0;

/// The panel with each rip's stretch of its outline replaced by the rip's new edge. Each new edge
/// is joined to the edge before and after it: both trimmed back to where they cross; else carried
/// on, the way they leave, to where they meet; else, where they run side by side (a cut ending on
/// a tangent edge cut less), stepped square across. So the outline is made whole by what each
/// edge is, never by cutting pieces away, and every edge of it says what it is.
fn cut_by_rips(flat: &FlatPanel, sides: &[RipSide]) -> Result<FlatPanel, DevelopError> {
    let loops: Vec<&Loop2> = std::iter::once(&flat.region.outer).chain(&flat.region.holes).collect();
    let far = flat.region.outer.segs.iter().map(Seg2::length).sum::<f64>() + 1.0;
    let mut on: Vec<Vec<(f64, f64, &RipSide)>> = vec![Vec::new(); loops.len()];
    for side in sides {
        let lost = || DevelopError::Joint(side.joint, "a rip's edge is not on its panel's outline");
        let found = loops.iter().enumerate().find_map(|(li, l)| {
            let (s, e) = (place_on(&l.segs, side.ends[0], ON_OUTLINE)?, place_on(&l.segs, side.ends[1], ON_OUTLINE)?);
            Some((li, s, e, place_on(&l.segs, side.middle, MIDDLE_TOL)?))
        });
        let Some((li, s, e, m)) = found else { return Err(lost()) };
        // The stretch from its start to its end the way the loop runs holds its middle.
        let n = loops.get(li).map_or(1.0, |l| l.segs.len() as f64);
        if (m - s).rem_euclid(n) > (e - s).rem_euclid(n) && (e - s).rem_euclid(n) > 0.0 {
            return Err(DevelopError::Joint(side.joint, "a rip's edge runs against its panel's outline"));
        }
        on.get_mut(li).ok_or_else(lost)?.push((s, e, side));
    }
    let mut out = flat.clone();
    for (li, rips) in on.iter_mut().enumerate() {
        let (Some(lp), Some(from)) = (loops.get(li), flat.origins.get(li)) else { continue };
        if rips.is_empty() {
            continue;
        }
        rips.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut stretches: Vec<Stretch> = Vec::with_capacity(2 * rips.len());
        for (i, (_, e, side)) in rips.iter().enumerate() {
            stretches.push(side.cut.iter().map(|c| (*c, EdgeFrom::Rip(side.joint))).collect());
            let next = rips.get((i + 1) % rips.len()).map_or(*e, |r| r.0);
            let rest = portion(&lp.segs, *e, next);
            stretches.push(rest.into_iter().map(|(seg, k)| (seg, from.get(k).copied().unwrap_or(EdgeFrom::Own(k)))).collect());
        }
        stretches.retain(|s| !s.is_empty());
        let count = stretches.len();
        for i in 0..count {
            let next = (i + 1) % count;
            if next == i {
                continue;
            }
            let mut b = stretches.get_mut(next).map(std::mem::take).unwrap_or_default();
            if let Some(a) = stretches.get_mut(i) {
                join(a, &mut b, far);
            }
            if let Some(slot) = stretches.get_mut(next) {
                *slot = b;
            }
        }
        let edges: Stretch = stretches.into_iter().flatten().filter(|(s, _)| s.length() > 0.0).collect();
        let new = Loop2 { segs: edges.iter().map(|e| e.0).collect() };
        if li == 0 {
            out.region.outer = new;
        } else if let Some(h) = out.region.holes.get_mut(li - 1) {
            *h = new;
        }
        if let Some(o) = out.origins.get_mut(li) {
            *o = edges.iter().map(|e| e.1).collect();
        }
    }
    Ok(out)
}

/// Where `p` lies on a loop, within `tol`: its segment's place in the loop plus the parameter
/// along it, so that places compare along the loop.
fn place_on(segs: &[Seg2], p: Vec2, tol: f64) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for (k, s) in segs.iter().enumerate() {
        let t = s.closest_param(p);
        let d = s.point_at(t).dist(p);
        if d <= tol && best.is_none_or(|b| d < b.1) {
            best = Some((k as f64 + t, d));
        }
    }
    best.map(|b| b.0)
}

/// The stretch of a loop from place `from` to place `to` the way it runs (see [`place_on`]), each
/// piece with the index of the segment it is of; nothing where they are one place.
fn portion(segs: &[Seg2], from: f64, to: f64) -> Vec<(Seg2, usize)> {
    let n = segs.len();
    let total = n as f64;
    let snap = |x: f64| if (x - x.round()).abs() < 1e-9 { x.round() } else { x };
    let (from, mut to) = (snap(from).rem_euclid(total), snap(to).rem_euclid(total));
    if (to - from).abs() < 1e-9 {
        return Vec::new();
    }
    if to < from {
        to += total;
    }
    let mut out = Vec::new();
    let mut x = from;
    while to - x > 1e-9 {
        let k = x.floor();
        let end = (k + 1.0).min(to);
        let i = (k as usize) % n.max(1);
        if let Some(s) = segs.get(i) {
            let piece = part(s, x - k, end - k);
            if piece.length() > 0.0 {
                out.push((piece, i));
            }
        }
        x = end;
    }
    out
}

/// The part of `s` from parameter `t0` to `t1`.
fn part(s: &Seg2, t0: f64, t1: f64) -> Seg2 {
    let right = if t0 > 0.0 { s.split_at(t0).1 } else { *s };
    let t = if t0 < 1.0 { (t1 - t0) / (1.0 - t0) } else { 1.0 };
    if t >= 1.0 { right } else { right.split_at(t.max(0.0)).0 }
}

/// Joins the stretch `a` to the stretch `b` after it (see [`cut_by_rips`]). An edge the join adds
/// is the rip's that made the corner: `a`'s where both are rips' and it carries `a` on.
fn join(a: &mut Stretch, b: &mut Stretch, far: f64) {
    let (Some(&(last, from_a)), Some(&(first, from_b))) = (a.last(), b.first()) else { return };
    let (pa, pb) = (last.end(), first.start());
    if pa.dist(pb) <= ON_OUTLINE {
        return;
    }
    let made = |x: EdgeFrom, y: EdgeFrom| if matches!(x, EdgeFrom::Rip(_)) { x } else { y };
    let gap = pa.dist(pb);
    // The crossing nearest the corner within `limit` gaps of it, and not where either stretch
    // meets the stretch on its far side.
    let (a_far, b_far) = (a.first().map_or(pa, |e| e.0.start()), b.last().map_or(pb, |e| e.0.end()));
    let nearest = |found: Vec<(usize, f64, usize, f64, Vec2)>, limit: f64| {
        found
            .into_iter()
            .filter(|c| c.4.dist(pa) <= limit * gap && c.4.dist(pb) <= limit * gap)
            .filter(|c| c.4.dist(a_far) > ON_OUTLINE && c.4.dist(b_far) > ON_OUTLINE)
            .min_by(|p, q| (p.4.dist(pa) + p.4.dist(pb)).total_cmp(&(q.4.dist(pa) + q.4.dist(pb))))
    };
    let segs = |s: &Stretch| s.iter().map(|e| e.0).collect::<Vec<Seg2>>();
    // Where they cross.
    if let Some((ia, u, ib, v, _)) = nearest(crossings(&segs(a), &segs(b)), CROSSING_LIMIT) {
        trim_end(a, ia, u);
        trim_start(b, ib, v);
        return;
    }
    let (Some(ta), Some(tb)) = (last.end_tangent().normalized(), first.start_tangent().normalized()) else {
        a.push((Seg2::Line { a: pa, b: pb }, made(from_a, from_b)));
        return;
    };
    // `a` carried on to `b`'s first edge.
    if let Some((_, _, _, v, x)) = nearest(crossings(&[Seg2::Line { a: pa, b: pa + ta * far }], &[first]), MITER_LIMIT) {
        a.push((Seg2::Line { a: pa, b: x }, made(from_a, from_b)));
        trim_start(b, 0, v);
        return;
    }
    // `b` carried back to `a`'s last edge.
    if let Some((_, u, _, _, x)) = nearest(crossings(&[last], &[Seg2::Line { a: pb - tb * far, b: pb }]), MITER_LIMIT) {
        trim_end(a, a.len() - 1, u);
        b.insert(0, (Seg2::Line { a: x, b: pb }, made(from_b, from_a)));
        return;
    }
    // Both carried on, to where they meet; else a step across.
    let across = ta.cross(tb);
    if across.abs() > 1e-12 {
        let (u, v) = ((pb - pa).cross(tb) / across, (pb - pa).cross(ta) / across);
        if u > 0.0 && v > 0.0 && u <= MITER_LIMIT * gap && v <= MITER_LIMIT * gap {
            let x = pa + ta * u;
            a.push((Seg2::Line { a: pa, b: x }, made(from_a, from_b)));
            b.insert(0, (Seg2::Line { a: x, b: pb }, made(from_b, from_a)));
            return;
        }
    }
    a.push((Seg2::Line { a: pa, b: pb }, made(from_a, from_b)));
}

/// `a` ending at parameter `u` of its edge `ia`.
fn trim_end(a: &mut Stretch, ia: usize, u: f64) {
    a.truncate(ia + 1);
    if let Some(last) = a.last_mut() {
        last.0 = part(&last.0, 0.0, u);
        if last.0.length() <= ON_OUTLINE * 1e-3 {
            a.pop();
        }
    }
}

/// `b` starting at parameter `v` of its edge `ib`.
fn trim_start(b: &mut Stretch, ib: usize, v: f64) {
    b.drain(..ib.min(b.len()));
    if let Some(first) = b.first_mut() {
        first.0 = part(&first.0, v, 1.0);
        if first.0.length() <= ON_OUTLINE * 1e-3 {
            b.remove(0);
        }
    }
}

/// Where the edges of `a` cross those of `b`: each crossing's edge and parameter on either, and
/// its point. Each edge is looked at as chords ([`CROSSING_CHORDS`] of a curve, one of a line),
/// and a crossing of chords followed onto the edges themselves.
pub fn crossings(a: &[Seg2], b: &[Seg2]) -> Vec<(usize, f64, usize, f64, Vec2)> {
    let chords = |s: &Seg2| if matches!(s, Seg2::Line { .. }) { 1 } else { CROSSING_CHORDS };
    let mut out: Vec<(usize, f64, usize, f64, Vec2)> = Vec::new();
    for (ia, sa) in a.iter().enumerate() {
        let na = chords(sa);
        for (ib, sb) in b.iter().enumerate() {
            let nb = chords(sb);
            for i in 0..na {
                let (p0, p1) = (sa.point_at(i as f64 / na as f64), sa.point_at((i + 1) as f64 / na as f64));
                for k in 0..nb {
                    let (q0, q1) = (sb.point_at(k as f64 / nb as f64), sb.point_at((k + 1) as f64 / nb as f64));
                    let (d1, d2) = (p1 - p0, q1 - q0);
                    let den = d1.cross(d2);
                    if den.abs() <= 1e-15 * d1.len() * d2.len() {
                        continue;
                    }
                    let (s, r) = ((q0 - p0).cross(d2) / den, (q0 - p0).cross(d1) / den);
                    let slack = 1e-9;
                    if !((-slack..=1.0 + slack).contains(&s) && (-slack..=1.0 + slack).contains(&r)) {
                        continue;
                    }
                    let (mut u, mut v) = ((i as f64 + s.clamp(0.0, 1.0)) / na as f64, (k as f64 + r.clamp(0.0, 1.0)) / nb as f64);
                    // Onto the edges themselves: Newton on a(u) = b(v).
                    for _ in 0..20 {
                        let f = sa.point_at(u) - sb.point_at(v);
                        let (du, dv) = (sa.derivative(u), sb.derivative(v));
                        let det = du.cross(-dv);
                        if f.len() <= 1e-13 || det.abs() <= 1e-300 {
                            break;
                        }
                        let (step_u, step_v) = ((-f).cross(-dv) / det, du.cross(-f) / det);
                        (u, v) = ((u + step_u).clamp(0.0, 1.0), (v + step_v).clamp(0.0, 1.0));
                    }
                    let x = sa.point_at(u);
                    let fresh = out.iter().all(|c| c.4.dist(x) > ON_OUTLINE);
                    if x.dist(sb.point_at(v)) <= ON_OUTLINE && fresh {
                        out.push((ia, u, ib, v, x));
                    }
                }
            }
        }
    }
    out
}

/// A curve through `points` in order: a line where they lie on one, else a clamped cubic spline
/// through every one of them, joined with no change of curvature.
pub fn curve_through(points: &[Vec2]) -> Vec<Seg2> {
    let (Some(&a), Some(&b)) = (points.first(), points.last()) else { return Vec::new() };
    let Some(d) = (b - a).normalized() else { return Vec::new() };
    if points.iter().all(|p| (*p - a).cross(d).abs() <= RIP_TOL) {
        return vec![Seg2::Line { a, b }];
    }
    let n = points.len();
    // Its way at its ends to second order: (-3 p0 + 4 p1 - p2) / 2, and the same from the end.
    let (start, end) = match (points.get(1), points.get(2), points.get(n - 2), points.get(n - 3)) {
        (Some(p1), Some(p2), Some(q1), Some(q2)) if n > 2 => ((*p1 * 4.0 - a * 3.0 - *p2) * 0.5, (b * 3.0 - *q1 * 4.0 + *q2) * 0.5),
        _ => (b - a, b - a),
    };
    let rates = clamped_rates(points, 1.0, start, end);
    points
        .windows(2)
        .zip(rates.windows(2))
        .filter_map(|(p, m)| match (p, m) {
            ([p0, p1], [m0, m1]) => Some(Seg2::Cubic { p0: *p0, p1: *p0 + *m0 * (1.0 / 3.0), p2: *p1 - *m1 * (1.0 / 3.0), p3: *p1 }),
            _ => None,
        })
        .collect()
}

/// The bend's zone, and what each panel loses to it.
fn add_zone(
    out: &mut Development,
    sheet: &Sheet,
    j: usize,
    joint: &Joint,
    bend: &Bend,
    frame: &JointFrame,
    placed: (&Rigid2, &Rigid2),
) -> Result<(), DevelopError> {
    let width = bend.allowance(sheet.thickness, sheet.k_factor);
    let setback = bend.setback(sheet.thickness);
    // Across the joint, from the `a` anchor: the `a` panel's tangent line, then the zone, then
    // the `b` panel's tangent line.
    let (near_line, far_line) = (-setback, width - setback);
    let (near, far) = (bend.ends.near, bend.ends.far);
    if width > 0.0 && (near.1 > near.0 || far.1 > far.0) {
        let (n0, n1, f0, f1) = (frame.at(near.0, near_line), frame.at(near.1, near_line), frame.at(far.0, far_line), frame.at(far.1, far_line));
        // Each end straight across, or torn along the curve it follows; the loop runs along the
        // `a` side, up the end, back along the `b` side and down the start.
        let end = |relief: &Relief, (from, to): (f64, f64)| match *relief {
            Relief::Tear { normal, offset } => torn(sheet, bend, frame, (normal, offset), (from, to)),
            _ => vec![Seg2::Line { a: frame.at(from, near_line), b: frame.at(to, far_line) }],
        };
        let mut segs = vec![Seg2::Line { a: n0, b: n1 }];
        segs.extend(end(&bend.relief.1, (near.1, far.1)));
        segs.push(Seg2::Line { a: f1, b: f0 });
        segs.extend(end(&bend.relief.0, (near.0, far.0)).iter().rev().map(Seg2::reversed));
        out.zones.push(Zone { joint: j, region: Region2 { outer: Loop2 { segs }.ccw(), holes: Vec::new() }, near: [n0, n1], far: [f0, f1] });
    }
    // A panel's band runs from its tangent line across its anchor, and a thickness past it so
    // that an edge lying on the anchor leaves no sliver.
    let depth = setback + sheet.thickness.max(REACH_TOL);
    let bands = [(joint.a.panel, placed.0, near, near_line, depth, bend.tails.0), (joint.b.panel, placed.1, far, far_line, -depth, bend.tails.1)];
    let relieved = |r: &Relief| matches!(r, Relief::Notch | Relief::Tear { .. });
    for (panel, place, (u0, u1), line, depth, tail) in bands {
        // The band's ends run on past the panel wherever nothing of the panel lies beyond them
        // in the band: always for a trimmed tail, which goes the same way, and for a kept one
        // where the panel ends with the zone. Either way the cut is the same, and an end left
        // on the panel's own edge would be a rounding away from it, for the planar booleans to
        // trip on.
        let reach = panel_reach(sheet, panel)?;
        let extent = if tail == Tail::Keep { band_extent(sheet, panel, place, frame, line, depth)? } else { None };
        let (s0, s1) = match (tail, extent) {
            (Tail::Trim, _) => (u0.min(u1) - reach, u0.max(u1) + reach),
            (Tail::Keep, None) => continue,
            (Tail::Keep, Some((lo, hi))) => (
                if lo >= u0 - EDGE_TOL || relieved(&bend.relief.0) { u0 - reach } else { u0 },
                if hi <= u1 + EDGE_TOL || relieved(&bend.relief.1) { u1 + reach } else { u1 },
            ),
        };
        if s1 > s0 {
            let corners = [frame.at(s0, line), frame.at(s1, line), frame.at(s1, line + depth), frame.at(s0, line + depth)];
            let region = Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() };
            out.trims.push(Trim { panel, joint: j, region, line: [corners[0], corners[1]] });
        }
        // A bend relief where the panel runs on past an end: a slot past the end, from inside
        // the panel's tangent line out through the band, and on beside the zone and the other
        // panel as far as that panel reaches across the joint, and the slot's width further,
        // so that whatever of this panel lies beside them in the flat lets them go. `out_dir`
        // runs past the end.
        let Some((lo, hi)) = extent else { continue };
        let (other, beyond) = if depth > 0.0 { (joint.b.panel, far_line) } else { (joint.a.panel, near_line) };
        let across = panel_across(sheet, other, if depth > 0.0 { &joint.b } else { &joint.a })?;
        for (relief, u, out_dir) in [(bend.relief.0, u0, -1.0), (bend.relief.1, u1, 1.0)] {
            let Relief::Slot { width: w, depth: d } = relief else { continue };
            let runs_on = if out_dir < 0.0 { lo < u - w - EDGE_TOL } else { hi > u + w + EDGE_TOL };
            if !(runs_on && w > 0.0) {
                continue;
            }
            let (s0, s1) = if out_dir < 0.0 { (u - w, u) } else { (u, u + w) };
            // The other panel's anchor lies its setback back across the zone from its tangent line.
            let past = beyond + depth.signum() * (across - setback + w);
            let l1 = if depth > 0.0 { (line + depth).max(past) } else { (line + depth).min(past) };
            let (l0, l1) = (line - depth.signum() * d.max(0.0), l1);
            let corners = [frame.at(s0, l0), frame.at(s1, l0), frame.at(s1, l1), frame.at(s0, l1)];
            let region = Region2 { outer: Loop2::polygon(&corners).ccw(), holes: Vec::new() };
            out.trims.push(Trim { panel, joint: j, region, line: [corners[0], corners[1]] });
        }
    }
    Ok(())
}

/// A torn end of a bend's zone in the flat, from `from` along the joint on the `a` panel's tangent
/// line to `to` on the `b` panel's, as a cubic spline within [`TEAR_TOL`] of the curve it follows:
/// at each turn round the bend, the length along the joint where the bend's metal, through its
/// whole thickness, keeps off the tear's plane by as much as the ends give in proportion.
fn torn(sheet: &Sheet, bend: &Bend, frame: &JointFrame, (normal, offset): (Vec3, f64), (from, to): (f64, f64)) -> Vec<Seg2> {
    let (t, theta) = (sheet.thickness, bend.angle);
    let wrap = Wrap::of(bend, t, sheet.k_factor);
    let neutral = bend.radius + sheet.k_factor * t;
    let straight = vec![Seg2::Line { a: frame.at(from, -bend.setback(t)), b: frame.at(to, theta * neutral - bend.setback(t)) }];
    // Along the joint, the distance off the plane changes alike at every depth.
    if !(normal.x.abs() > 1e-9 && neutral > 0.0 && theta > 0.0) {
        return straight;
    }
    let off = |along: f64, phi: f64, depth: f64| wrap.at(along, phi, depth).dot(normal) - offset;
    let least = |along: f64, phi: f64| off(along, phi, 0.0).min(off(along, phi, t));
    let (c0, c1) = (least(from, 0.0), least(to, theta));
    // Where along the joint each face of the sheet would keep its share off the plane: the
    // nearer of the two to the plane keeps the metal off it.
    let need = |phi: f64| {
        let clear = c0 + (c1 - c0) * phi / theta;
        [0.0, t].map(|depth| from + (clear - off(from, phi, depth)) / normal.x)
    };
    let deep = |phi: f64| {
        let [sheet, deep] = need(phi);
        if normal.x > 0.0 { deep > sheet } else { deep < sheet }
    };
    let at = |phi: f64| -> Vec2 {
        let [sheet, deep] = need(phi);
        let along = if normal.x > 0.0 { sheet.max(deep) } else { sheet.min(deep) };
        frame.at(along, phi * neutral - bend.setback(t))
    };
    // The curve is smooth but where the face that keeps the metal off changes: there it turns a
    // corner. Each smooth run is a cubic spline through it.
    let mut breaks = vec![0.0];
    let mid = |k: usize| theta * (k as f64 + 0.5) / TEAR_SAMPLES as f64;
    for k in 1..TEAR_SAMPLES {
        let (mut lo, mut hi) = (mid(k - 1), mid(k));
        if deep(lo) != deep(hi) {
            let side = deep(lo);
            for _ in 0..60 {
                let m = (lo + hi) / 2.0;
                if deep(m) == side {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            breaks.push((lo + hi) / 2.0);
        }
    }
    breaks.push(theta);
    let mut out = Vec::new();
    for w in breaks.windows(2) {
        if let [p0, p1] = w {
            out.extend(spline(&at, *p0, *p1));
        }
    }
    out
}

/// The curve `at` from `a` to `b` as a cubic spline in the fewest even spans that keep within
/// [`TEAR_TOL`] of it: through the curve at the spans' ends, along it at its two ends (its
/// direction there taken from inside the run), and joined with no change of curvature.
fn spline(at: &dyn Fn(f64) -> Vec2, a: f64, b: f64) -> Vec<Seg2> {
    let rate = |x: f64, h: f64| (at(x) * -3.0 + at(x + h) * 4.0 - at(x + 2.0 * h)) * (1.0 / (2.0 * h));
    let step = (b - a) * 1e-5;
    let (start, end) = (rate(a, step), rate(b, -step));
    let mut segs = Vec::new();
    for spans in 1..=MAX_TEAR_SPANS {
        let h = (b - a) / spans as f64;
        let points: Vec<Vec2> = (0..=spans).map(|i| at(a + h * i as f64)).collect();
        let rates = clamped_rates(&points, h, start, end);
        segs = points
            .windows(2)
            .zip(rates.windows(2))
            .filter_map(|(p, m)| match (p, m) {
                ([p0, p1], [m0, m1]) => Some(Seg2::Cubic { p0: *p0, p1: *p0 + *m0 * (h / 3.0), p2: *p1 - *m1 * (h / 3.0), p3: *p1 }),
                _ => None,
            })
            .collect();
        let strays = (0..=TEAR_SAMPLES).any(|k| {
            let x = k as f64 / TEAR_SAMPLES as f64 * spans as f64;
            let i = (x.floor() as usize).min(spans - 1);
            segs.get(i).is_some_and(|seg| seg.point_at(x - i as f64).dist(at(a + h * x)) > TEAR_TOL)
        });
        if !strays {
            break;
        }
    }
    segs
}

/// A clamped cubic spline's rates (per unit of its parameter) at `points`, `h` apart: `start` and
/// `end` at its ends, and between them those that keep its curvature unbroken,
/// `m[i - 1] + 4 m[i] + m[i + 1] = 3 (p[i + 1] - p[i - 1]) / h`, solved down the tridiagonal.
fn clamped_rates(points: &[Vec2], h: f64, start: Vec2, end: Vec2) -> Vec<Vec2> {
    let n = points.len().saturating_sub(1);
    let mut m = vec![start; n + 1];
    if let Some(last) = m.last_mut() {
        *last = end;
    }
    if n < 2 {
        return m;
    }
    // Forward sweep over the inner rates 1 ..= n - 1, then back.
    let (mut diag, mut rhs) = (vec![4.0; n + 1], vec![Vec2::ZERO; n + 1]);
    for i in 1..n {
        rhs[i] = (points[i + 1] - points[i - 1]) * (3.0 / h);
    }
    rhs[1] -= start;
    rhs[n - 1] -= end;
    for i in 2..n {
        let f = 1.0 / diag[i - 1];
        diag[i] -= f;
        let carried = rhs[i - 1] * f;
        rhs[i] -= carried;
    }
    for i in (1..n).rev() {
        let next = if i + 1 < n { m[i + 1] } else { Vec2::ZERO };
        m[i] = (rhs[i] - next) * (1.0 / diag[i]);
    }
    m
}

/// The relief a corner cuts from the flat: centered where its two bends' zones' center lines
/// cross. `None` where either bend has no zone, or the two run alike.
fn corner_cut(dev: &Development, corner: &Corner) -> Option<Region2> {
    let center_line = |j: usize| {
        let z = dev.zones.iter().find(|z| z.joint == j)?;
        let [n0, n1] = z.near;
        let [f0, f1] = z.far;
        let (a, b) = ((n0 + f0) * 0.5, (n1 + f1) * 0.5);
        Some((a, (b - a).normalized()?))
    };
    let [ja, jb] = corner.joints;
    let ((p, u), (q, w)) = (center_line(ja)?, center_line(jb)?);
    let cross = u.cross(w);
    let r = corner.size / 2.0;
    if !(cross.abs() > 1e-9 && r > 0.0) {
        return None;
    }
    let c = p + u * ((q - p).cross(w) / cross);
    let round = match corner.relief {
        CornerRelief::Round => true,
        CornerRelief::Square => false,
        CornerRelief::Notch | CornerRelief::Tear => return None,
    };
    let outer = if round {
        Loop2 { segs: vec![Seg2::Arc { center: c, radius: r, start: 0.0, sweep: PI }, Seg2::Arc { center: c, radius: r, start: PI, sweep: PI }] }
    } else {
        // Its sides run along the two bends, each `r` from the center.
        let k = r / cross.abs();
        Loop2::polygon(&[c + (u + w) * k, c + (w - u) * k, c - (u + w) * k, c + (u - w) * k])
    };
    Some(Region2 { outer: outer.ccw(), holes: Vec::new() })
}

/// Where along a bend a panel's outline lies inside the band from `line` to `line + depth`
/// across it, in the flat: the least and greatest lengths along the bend, or `None` when none
/// of it does.
fn band_extent(sheet: &Sheet, panel: usize, place: &Rigid2, frame: &JointFrame, line: f64, depth: f64) -> Result<Option<(f64, f64)>, DevelopError> {
    let region = &sheet.panels.get(panel).ok_or(DevelopError::NoPanel(panel))?.region;
    let (lo, hi) = (line.min(line + depth) - EDGE_TOL, line.max(line + depth) + EDGE_TOL);
    let pts: Vec<(f64, f64)> = (&region.outer.polyline(REACH_TOL))
        .into_iter()
        .map(|q| {
            let p = place.point(*q) - frame.origin;
            (p.dot(frame.along), p.dot(frame.toward_b))
        })
        .collect();
    let mut extent: Option<(f64, f64)> = None;
    let mut take = |u: f64| extent = Some(extent.map_or((u, u), |(a, b)| (a.min(u), b.max(u))));
    // Each edge of the outline, clipped to the band.
    for (k, &(u_a, v_a)) in pts.iter().enumerate() {
        let Some(&(u_b, v_b)) = pts.get((k + 1) % pts.len()) else { continue };
        let at = |v: f64| if (v_b - v_a).abs() > 1e-15 { u_a + (u_b - u_a) * (v - v_a) / (v_b - v_a) } else { u_a };
        let (t0, t1) = (v_a.min(v_b).max(lo), v_a.max(v_b).min(hi));
        if t0 <= t1 {
            take(at(t0));
            take(at(t1));
            if (lo..=hi).contains(&v_a) {
                take(u_a);
            }
        }
    }
    Ok(extent)
}

/// How far panel `panel`'s region reaches across a joint from its anchor `at` there: what a bend
/// relief beside it runs on past. A curved panel's is not measured yet (none).
fn panel_across(sheet: &Sheet, panel: usize, at: &Anchor) -> Result<f64, DevelopError> {
    let p = sheet.panels.get(panel).ok_or(DevelopError::NoPanel(panel))?;
    if p.surface != Surface::Plane || p.region.outer.segs.is_empty() {
        return Ok(0.0);
    }
    Ok((&p.region.outer.polyline(REACH_TOL)).into_iter().map(|q| (*q - at.origin).cross(at.dir).abs()).fold(0.0, f64::max))
}

/// How far a panel's material reaches: the diagonal of its region's bounding box, which bounds
/// how far along a joint a trimmed tail must go.
fn panel_reach(sheet: &Sheet, panel: usize) -> Result<f64, DevelopError> {
    let region = &sheet.panels.get(panel).ok_or(DevelopError::NoPanel(panel))?.region;
    if region.outer.segs.is_empty() {
        return Ok(0.0);
    }
    Ok(region.outer.polyline(REACH_TOL).bounds().size().len())
}
