//! A section view's hatching (#13046): each body the section cuts has its cut faces hatched on
//! paper with thin parallel lines, or painted solid when they are too thin to hatch.
//!
//! - **The cut faces are read exactly** from the cut body: its planar faces that lie in the cut's
//!   plane and face out of the kept side, their loops' edges read on the view's plane
//!   ([`Body::planar_edges`]). So a cut hole's edge bounds its face and nothing is hatched inside it.
//! - **Which hatch a body takes is the screen's rule** ([`solidcraft_viewport::hatch::hatches`]):
//!   45° or 135°, and bodies whose cut faces touch differ. It is seeded by the body's name, so a body
//!   keeps its hatch across edits. On paper the screen's colour becomes a spacing, the other way
//!   drafting tells touching parts apart.
//! - The base spacing is [`SPACING_LINES`] narrow line widths of the view's line group, so a
//!   heavier line group hatches more openly.
//! - **The lines come from the drawing's hatcher** ([`solidcore_ops::hatch::generate`], #7064), one
//!   plain family per body. It takes straight-sided outlines, so each cut face's outline is followed
//!   to within [`FLATTEN_MM`] on paper as a closed polygon, under the line that draws it. A body the hatcher
//!   refuses (a budget, an uncertain contact) is drawn unhatched, and the view says which and why.
//! - A cut face narrower than its spacing everywhere would get one line at most, so it is painted
//!   solid instead, as drafting does for thin parts.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_4;

use solidcore_ops::hatch::{Mark, generate};
use solidcore_record::brep::Loop;
use solidcore_record::hatch::{Family, Pattern};
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_doc::drawing::view::Cut;
use solidcraft_geom::Similarity2;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, PlanarPiece};
use solidcraft_viewport::hatch::{CutPart, HATCH_COLOURS, hatches as pick, seed};

use super::ShownSet;

/// The base hatch spacing, in narrow line widths: 2.5 mm in the default 0.5 line group.
const SPACING_LINES: f64 = 10.0;
/// Each screen colour's spacing on paper, as a share of the base spacing.
const SPACINGS: [f64; HATCH_COLOURS] = [1.0, 0.6, 1.6];
/// How near antiparallel a face's normal and the cut plane's must be, by their dot product, for the
/// face to be a cut face.
const FACING: f64 = 1.0 - 1e-9;
/// How closely a cut face's curved edges are followed for the hatcher, in paper millimetres.
/// The hatcher's work grows with the square of an outline's points, and its budget refused a
/// 560 mm flange at 1:1 at 0.01 mm. At 0.05 mm an 800 mm flange at 1:1 takes about 60% of it.
/// Where 0.05 mm and 0.01 mm hatch differently, the difference lies within 0.05 mm of the true
/// outline, under the visible line that draws it (0.5 mm wide in the default line group).
pub const FLATTEN_MM: f64 = 0.05;
/// How closely a cut face's outline is followed for the decisions made on it (which parts touch,
/// which are too thin to hatch), in paper millimetres. They have no budget, so they stay fine.
const OUTLINE_MM: f64 = 0.01;
/// Points a free-form edge is sampled at, per control point.
const SPLINE_SAMPLES: usize = 8;
/// The most points one free-form edge is sampled at.
const MAX_SPLINE_SAMPLES: usize = 1024;

/// One body's hatching on paper.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyHatch {
    /// The body's index among the shown set's.
    pub shown: usize,
    /// Its lines' angle on paper, in radians: π/4 or 3π/4.
    pub angle: f64,
    /// Its lines' spacing, in paper millimetres.
    pub spacing_mm: f64,
    /// Painted solid: its cut faces are too thin to hatch.
    pub filled: bool,
    /// Its cut faces, on paper.
    pub regions: Vec<Region2>,
    /// Its hatch lines, on paper: none when it is filled.
    pub strokes: Vec<Seg2>,
}

/// A section's hatching: each body hatched, and each body the hatcher refused, with why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hatching {
    pub bodies: Vec<BodyHatch>,
    /// Bodies with cut faces drawn unhatched: their index among the shown set's, and why.
    pub refused: Vec<(usize, String)>,
}

/// The hatching of every body `set` shows that the section `cut` cut, through `frame`, in line
/// `group`: each body whose cut faces read exactly, in the shown set's order. A body is hatched
/// whole or not at all.
pub fn hatches(set: &ShownSet, cut: &Cut, frame: &ViewFrame, group: LineGroup) -> Hatching {
    let Some(plane) = cut.line.planes(&cut.parent).ok().and_then(|planes| planes.into_iter().next()) else {
        return Hatching::default();
    };
    let (basis, map) = (frame.basis(), frame.paper_map());
    let mut cut_faces: Vec<(usize, &str, Vec<Region2>)> = Vec::new();
    for (at, shown) in set.bodies.iter().enumerate() {
        let Some(body) = set.body(shown).filter(|_| !shown.reference && set.cut.contains(&shown.index)) else { continue };
        let regions: Vec<Region2> = faces_on(&body.body, &plane, &basis).iter().map(|r| similar(r, &map)).collect();
        if !regions.is_empty() {
            cut_faces.push((at, body.name.as_str(), regions));
        }
    }
    // The screen's rule picks each hatch from the cut faces' outlines on paper, flat on z = 0.
    let outlines: Vec<Vec<(Vec3, Vec3)>> = cut_faces.iter().map(|(_, _, regions)| outline(regions)).collect();
    let parts: Vec<CutPart> = cut_faces.iter().zip(&outlines).map(|((_, name, _), segments)| CutPart { seed: seed(name), segments }).collect();
    let picked = pick(&parts, Vec3::Z);
    let base = SPACING_LINES * LineKind::Thin.width_mm(group);
    let mut out = Hatching::default();
    for ((shown, _, regions), hatch) in cut_faces.into_iter().zip(picked) {
        let angle = if hatch.turned() { 3.0 * FRAC_PI_4 } else { FRAC_PI_4 };
        let spacing_mm = base * SPACINGS.get(hatch.colour()).copied().unwrap_or(1.0);
        let filled = regions.iter().all(|r| thin(r, spacing_mm));
        let strokes = if filled { Ok(Vec::new()) } else { lines_across(&regions, angle, spacing_mm) };
        match strokes {
            Ok(strokes) => out.bodies.push(BodyHatch { shown, angle, spacing_mm, filled, regions, strokes }),
            Err(why) => out.refused.push((shown, why)),
        }
    }
    out
}

/// Plain lines at `angle`, `spacing_mm` apart through the paper's origin, across `regions`, each
/// followed as a closed polygon; or why the hatcher refused one.
fn lines_across(regions: &[Region2], angle: f64, spacing_mm: f64) -> std::result::Result<Vec<Seg2>, String> {
    let family = Family { angle, origin: Vec2::ZERO, delta_x: 0.0, delta_y: spacing_mm, dashes: Vec::new() };
    let pattern = Pattern { name: "SECTION".into(), description: "A section's plain hatch lines".into(), families: vec![family] };
    let mut out = Vec::new();
    for region in regions {
        let marks = generate(&pattern, &polygonal(region)).map_err(|e| e.to_string())?;
        out.extend(marks.into_iter().filter_map(|mark| match mark {
            Mark::Line { a, b } => Some(Seg2::Line { a, b }),
            Mark::Dot { .. } => None,
        }));
    }
    Ok(out)
}

/// `region` with each loop followed to within [`FLATTEN_MM`] by straight sides, each starting exactly
/// where the last one ends; a point repeated in a row once.
fn polygonal(region: &Region2) -> Region2 {
    let ring = |l: &Loop2| {
        let mut points: Vec<Vec2> = l.polyline(FLATTEN_MM).into_vec();
        points.dedup();
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        Loop2::polygon(&points)
    };
    Region2 { outer: ring(&region.outer), holes: region.holes.iter().map(ring).collect() }
}

/// Whether `region` is narrower than `spacing` everywhere: it would get one hatch line at most.
fn thin(region: &Region2, spacing: f64) -> bool {
    2.0 * region.inradius(OUTLINE_MM) < spacing
}

/// `body`'s faces that lie in `plane` and face against its normal (out of the side the section
/// keeps), each as a region in `basis`'s coordinates: its first loop the outline, the others holes.
/// A face with an edge that doesn't read exactly is left out.
fn faces_on(body: &Body, plane: &Plane, basis: &Plane) -> Vec<Region2> {
    let tol = body.planar_edges_tolerance();
    let record = body.record();
    let mut out = Vec::new();
    for (index, face) in record.faces.iter().enumerate() {
        let Some(on) = body.planar_face(index) else { continue };
        if on.normal().dot(plane.normal()) > -FACING || plane.height(on.origin).abs() > tol {
            continue;
        }
        let loops: Vec<&[solidcore_record::brep::OrientedEdge]> = face
            .loops
            .iter()
            .filter_map(|l| match l {
                Loop::Edges { edges } => Some(edges.as_slice()),
                Loop::Vertex { .. } => None,
            })
            .collect();
        let edges: Vec<usize> = loops.iter().flat_map(|l| l.iter().map(|e| e.edge as usize)).collect();
        let read: BTreeMap<usize, Option<Vec<PlanarPiece>>> = edges.iter().copied().zip(body.planar_edges(basis, &edges)).collect();
        let rings: Option<Vec<Loop2>> = loops
            .iter()
            .map(|l| {
                let mut segs = Vec::new();
                for used in l.iter() {
                    let pieces = read.get(&(used.edge as usize))?.as_ref()?;
                    let mut along: Vec<Seg2> = pieces.iter().map(piece_segs).collect::<Option<Vec<_>>>()?.concat();
                    if !used.forward {
                        along = along.iter().rev().map(|s| s.reversed()).collect();
                    }
                    segs.extend(along);
                }
                Some(Loop2 { segs })
            })
            .collect();
        let Some(mut rings) = rings.filter(|r| !r.is_empty()) else { continue };
        let outer = rings.remove(0).ccw();
        out.push(Region2 { outer, holes: rings.into_iter().map(|h| h.ccw().reversed()).collect() });
    }
    out
}

/// A piece of an edge as segments: itself, or a free-form curve of several spans as a polyline
/// through points along it (hatching only needs the outline drawn to within a line's width).
fn piece_segs(piece: &PlanarPiece) -> Option<Vec<Seg2>> {
    match piece {
        PlanarPiece::Seg(s) => Some(vec![*s]),
        PlanarPiece::Spline(s) => {
            let degree = u8::try_from(s.degree).ok()?;
            let nurbs = solidcraft_sketch::Nurbs { knots: s.knots.clone(), weights: s.weights.clone() };
            let n = (s.points.len() * SPLINE_SAMPLES).clamp(2, MAX_SPLINE_SAMPLES);
            let points: Vec<Vec2> = (0..=n).map(|i| nurbs.point(&s.points, degree, i as f64 / n as f64)).collect::<Option<_>>()?;
            Some(points.windows(2).filter_map(|w| Some(Seg2::Line { a: *w.first()?, b: *w.get(1)? })).collect())
        }
    }
}

/// `region` through the paper map `map`.
fn similar(region: &Region2, map: &Similarity2) -> Region2 {
    let ring = |l: &Loop2| Loop2 { segs: l.segs.iter().map(|s| s.similar(map)).collect() };
    Region2 { outer: ring(&region.outer), holes: region.holes.iter().map(ring).collect() }
}

/// `regions`' outlines as segments on z = 0, for the screen's rule.
fn outline(regions: &[Region2]) -> Vec<(Vec3, Vec3)> {
    let flat = |p: &Vec2| Vec3::new(p.x, p.y, 0.0);
    let rings = regions.iter().flat_map(|r| r.loops()).map(|l| l.polyline(OUTLINE_MM));
    rings.flat_map(|ring| ring.edges().map(|(a, b)| (flat(a), flat(b))).collect::<Vec<_>>()).collect()
}
