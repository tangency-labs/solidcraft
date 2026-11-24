//! A section view's hatching (#13046): each body the section cuts has its cut faces hatched on
//! paper with thin parallel lines, or painted solid when they are too thin to hatch.
//!
//! - **The cut faces are read exactly** from the cut body: its planar faces that lie in the cut's
//!   plane and face out of the kept side. A part that only touches the plane, where two parts meet,
//!   was not cut and isn't hatched ([`cut_faces`]). A part cut through, with a face of its own on the
//!   plane too, keeps only the faces the cut made, by the cut's history. Their loops' edges are read
//!   on the view's plane ([`Body::planar_edges`]), so a cut hole's edge bounds its face and nothing is
//!   hatched inside it.
//! - **Which hatch a body takes is the screen's rule** ([`solidcraft_viewport::hatch::hatches`]):
//!   45° or 135°, and bodies whose cut faces touch differ. It is seeded by the body's name, so a body
//!   keeps its hatch across edits. On paper the screen's colour becomes a spacing, the other way
//!   drafting tells touching parts apart.
//! - The base spacing is [`SPACING_LINES`] narrow line widths of the view's line group, so a
//!   heavier line group hatches more openly.
//! - **The lines come from the drawing's hatcher** ([`Pattern::marks`], #7064), one
//!   plain family per body. It takes straight-sided outlines, so each cut face's outline is followed
//!   to within [`FLATTEN_MM`] on paper as a closed polygon, under the line that draws it. A body the hatcher
//!   refuses (a budget, an uncertain contact) is drawn unhatched, and the view says which and why.
//! - A cut face narrower than its spacing everywhere would get one line at most, so it is painted
//!   solid instead, as drafting does for thin parts.

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_4;

use solidcore_ops::profile::Mark;
use solidcore_record::brep::Loop;
use solidcore_record::profile::line_family::LineFamily;
use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_doc::drawing::hatch::Pattern;
use solidcraft_doc::drawing::view::Cut;
use solidcraft_geom::Similarity2;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::history::{FaceOrigin, History};
use solidcraft_kernel::{Body, PlanarPiece};
use solidcraft_viewport::hatch::{CutPart, HATCH_COLOURS, hatches as pick, seed};

use super::ShownSet;

/// Why a body is hatched in doubt ([`CutFaces::InDoubt`]).
const DOUBT: &str = "its own face on the cutting plane may be hatched too: without the cut's history it can't be told from a cut face";
/// The base hatch spacing, in narrow line widths: 2.5 mm in the default 0.5 line group.
const SPACING_LINES: f64 = 10.0;
/// Each screen colour's spacing on paper, as a share of the base spacing.
const SPACINGS: [f64; HATCH_COLOURS] = [1.0, 0.6, 1.6];
/// How near antiparallel a face's normal and the cut plane's must be, by their dot product, for the
/// face to be a cut face.
const FACING: f64 = 1.0 - 1e-9;
/// The half-space box's place among a section's operands: the section is the body's intersection
/// with it, and keeps that boolean's history over the body, then the box ([`solidcraft_kernel::half_space`]).
const CUTTER: usize = 1;
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

/// A section's hatching: each body hatched, each body the hatcher refused, and each body hatched in
/// doubt, with why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hatching {
    pub bodies: Vec<BodyHatch>,
    /// Bodies with cut faces drawn unhatched: their index among the shown set's, and why.
    pub refused: Vec<(usize, String)>,
    /// Bodies hatched where a face of their own may be hatched too ([`CutFaces::InDoubt`]): their
    /// index among the shown set's, and why.
    pub doubted: Vec<(usize, String)>,
}

/// How a section tells its cut faces among its faces that lie in the plane and face out of the kept
/// side, as `cut_faces` finds by exact geometry and the cut's history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutFaces {
    /// By the cut's history: the faces the cut made alone.
    Traced,
    /// All of them: the body, uncut, had no face of its own there.
    All,
    /// All of them, in doubt: the body had a face of its own there, and the cut kept no history to
    /// tell it from a cut face.
    InDoubt,
}

/// The hatching of every body `set` shows that the section `cut` cut, through `frame`, in line
/// `group`: each body whose cut faces read exactly, in the shown set's order. A body is hatched
/// whole or not at all.
pub fn hatches(set: &ShownSet, cut: &Cut, frame: &ViewFrame, group: LineGroup) -> Hatching {
    let Some(plane) = cut.line.planes(&cut.parent).ok().and_then(|planes| planes.into_iter().next()) else {
        return Hatching::default();
    };
    let (basis, map) = (frame.basis(), frame.paper_map());
    let mut out = Hatching::default();
    let mut cut_faces: Vec<(usize, &str, Vec<Region2>)> = Vec::new();
    for (at, shown) in set.bodies.iter().enumerate() {
        let Some((body, &telling)) = set.body(shown).filter(|_| !shown.reference).zip(set.cut.get(&shown.index)) else { continue };
        let faces = faces_on(&body.body, telling, &plane, &basis);
        if faces.is_empty() {
            continue;
        }
        if telling == CutFaces::InDoubt {
            out.doubted.push((at, DOUBT.into()));
        }
        cut_faces.push((at, body.name.as_str(), faces.iter().map(|r| similar(r, &map)).collect()));
    }
    // The screen's rule picks each hatch from the cut faces' outlines on paper, flat on z = 0.
    let outlines: Vec<Vec<(Vec3, Vec3)>> = cut_faces.iter().map(|(_, _, regions)| outline(regions)).collect();
    let parts: Vec<CutPart> = cut_faces.iter().zip(&outlines).map(|((_, name, _), segments)| CutPart { seed: seed(name), segments }).collect();
    let picked = pick(&parts, Vec3::Z);
    let base = SPACING_LINES * LineKind::Thin.width_mm(group);
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
    let family = LineFamily { angle, origin: Vec2::ZERO, delta_x: 0.0, delta_y: spacing_mm, dashes: Vec::new() };
    let pattern = Pattern { name: "SECTION".into(), description: "A section's plain hatch lines".into(), families: vec![family] };
    let mut out = Vec::new();
    for region in regions {
        let marks = pattern.marks(&polygonal(region)).map_err(|e| e.to_string())?;
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

/// How the section `section` of `uncut` along `plane` tells its cut faces, by exact geometry and the
/// cut's history; `None` when nothing of `uncut` was cut. The section keeps the side `plane`'s normal
/// points to ([`solidcraft_doc::ModelBody::section`]).
/// - `uncut` doesn't reach past the plane, by its exact box: it only touches the plane, and a face of
///   it there (where two parts meet) is its own. Nothing is cut.
/// - The section has the cut's history: [`CutFaces::Traced`].
/// - Else, by `uncut`'s own faces: none on the plane facing out of the kept side, [`CutFaces::All`];
///   one, [`CutFaces::InDoubt`].
pub(super) fn cut_faces(uncut: &Body, section: &Body, plane: &Plane) -> Option<CutFaces> {
    let tol = uncut.planar_edges_tolerance();
    // The lowest point of the box round `uncut` along the normal: the box holds the body.
    let lowest = uncut.exact_bounds().map(|b| {
        let (n, half) = (plane.normal(), b.size() * 0.5);
        plane.height(b.center()) - (n.x.abs() * half.x + n.y.abs() * half.y + n.z.abs() * half.z)
    });
    if lowest.is_ok_and(|h| h >= -tol) {
        return None;
    }
    if traced(section).is_some() {
        return Some(CutFaces::Traced);
    }
    let own = (0..uncut.face_count()).any(|index| facing_out_on(uncut, index, plane, tol));
    Some(if own { CutFaces::InDoubt } else { CutFaces::All })
}

/// The history of `section` over the body and the half-space box, when it kept one.
fn traced(section: &Body) -> Option<&History> {
    section.history().filter(|h| h.operands == CUTTER + 1)
}

/// Whether face `index` of `body` is planar, lies in `plane` (within `tol`) and faces against its
/// normal, out of the side a section keeps.
fn facing_out_on(body: &Body, index: usize, plane: &Plane, tol: f64) -> bool {
    body.planar_face(index).is_some_and(|on| on.normal().dot(plane.normal()) <= -FACING && plane.height(on.origin).abs() <= tol)
}

/// The cut faces of `body`, a section, told as `telling` says ([`cut_faces`]): its faces that lie in
/// `plane` and face against its normal (out of the side the section keeps), each as a region in
/// `basis`'s coordinates ([`region`]). A face with an edge that doesn't read exactly is left out.
fn faces_on(body: &Body, telling: CutFaces, plane: &Plane, basis: &Plane) -> Vec<Region2> {
    let tol = body.planar_edges_tolerance();
    let record = body.record();
    let history = traced(body).filter(|_| telling == CutFaces::Traced);
    let mut out = Vec::new();
    for (index, face) in record.faces.iter().enumerate() {
        if !facing_out_on(body, index, plane, tol) || history.is_some_and(|h| !made_by_cut(h, index)) {
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
        if let Some(region) = rings.and_then(region) {
            out.push(region);
        }
    }
    out
}

/// Whether face `index` of a section came from the cut alone: from the half-space box and from no
/// face of the body. A face of the body's own that lies on the plane (where two parts meet) came
/// from the body too: the boolean joins a face found lying on another into that face's origins.
fn made_by_cut(history: &History, index: usize) -> bool {
    let from_cutter = |origin: &FaceOrigin| matches!(origin, FaceOrigin::Face { operand: CUTTER, .. });
    history.faces.get(index).is_some_and(|face| !face.origins.is_empty() && face.origins.iter().all(from_cutter))
}

/// A face's rings on the view's plane as a region, each turned the region's way: the outline, the
/// ring round the others, is the one with the largest area, and the others are holes. The record
/// tells them apart by orientation, not order (truck lists some holes first), and the view's plane
/// may see the face from behind. `None` for no rings.
fn region(mut rings: Vec<Loop2>) -> Option<Region2> {
    let area = |ring: &Loop2| ring.signed_area().abs();
    let outline = rings.iter().enumerate().max_by(|(_, a), (_, b)| area(a).total_cmp(&area(b))).map(|(at, _)| at)?;
    let outer = rings.remove(outline).ccw();
    Some(Region2 { outer, holes: rings.into_iter().map(|h| h.ccw().reversed()).collect() })
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

#[cfg(test)]
mod tests {
    use solidcraft_kernel::history::{Change, FaceProv};
    use solidcraft_kernel::{BoolOp, boolean, box_solid, half_space};

    use super::*;

    /// A face whose record lists its hole first (truck does; the record tells the loops apart by
    /// orientation, not order), seen from either side: the region is the 10 mm square less the
    /// Ø4 hole, its outline the square's ring.
    #[test]
    fn a_face_whose_hole_comes_first_keeps_its_outline() {
        let square = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]);
        let hole = Loop2::circle(Vec2::new(5.0, 5.0), 2.0).reversed();
        for (outline, hole) in [(square.clone(), hole.clone()), (square.reversed(), hole.reversed())] {
            let region = region(vec![hole, outline]).unwrap();
            assert!((region.outer.signed_area() - 100.0).abs() < 1e-9, "{region:?}");
            assert_eq!(region.holes.len(), 1, "{region:?}");
            assert!((region.holes[0].signed_area() + 4.0 * std::f64::consts::PI).abs() < 1e-9, "{region:?}");
        }
    }

    /// A face on the plane is the cut's when every face it came from is the half-space box's: the cap
    /// alone, not a part's own face found lying on the cap (whose origins name both).
    #[test]
    fn a_face_is_the_cuts_when_all_its_origins_are_the_boxs() {
        let face = |origins: Vec<FaceOrigin>, change| FaceProv { origins, change };
        let (own, cap) = (FaceOrigin::Face { operand: 0, face: 3 }, FaceOrigin::Face { operand: CUTTER, face: 0 });
        let history = History {
            operands: 2,
            faces: vec![
                face(vec![cap], Change::Modified),
                face(vec![own, cap], Change::Merged),
                face(vec![own], Change::Kept),
                face(vec![], Change::Generated),
            ],
            edges: vec![],
        };
        let made: Vec<bool> = (0..5).map(|index| made_by_cut(&history, index)).collect();
        assert_eq!(made, [true, false, false, false, false]);
    }

    /// Which faces a section takes as cut, by the uncut body: none of a block only touching the plane
    /// from the kept side; by the cut's history for a section that kept one; without history, every
    /// face on the plane for a block cut through, and the same in doubt for a stepped block whose step
    /// lies on the plane. The plane is x = 0, keeping x ≥ 0.
    #[test]
    fn a_section_tells_its_cut_faces_by_the_uncut_body() {
        let plane = Plane::from_normal(Vec3::ZERO, Vec3::X).unwrap();
        let block = |x0: f64, x1: f64, z1: f64| box_solid(Vec3::new(x0, 0.0, 0.0), Vec3::new(x1, 10.0, z1)).unwrap();
        // A body with no history over a body and a box stands for a section that kept none.
        let untraced = block(0.0, 10.0, 10.0);
        assert!(traced(&untraced).is_none());
        assert_eq!(cut_faces(&block(0.0, 10.0, 10.0), &untraced, &plane), None, "touching from the kept side");
        let through = block(-10.0, 10.0, 10.0);
        assert_eq!(cut_faces(&through, &untraced, &plane), Some(CutFaces::All));
        let (kept, _) = half_space(&through, &plane, true).unwrap();
        assert_eq!(cut_faces(&through, &kept.unwrap(), &plane), Some(CutFaces::Traced));
        // A step at x = 0: the part below z = 5 crosses the plane, the face above it lies on it.
        let stepped = boolean(&block(-10.0, 10.0, 5.0), &block(0.0, 10.0, 10.0), BoolOp::Union).unwrap().unwrap();
        assert_eq!(cut_faces(&stepped, &untraced, &plane), Some(CutFaces::InDoubt));
        // In doubt, the faces on the plane are hatched as before: the section's face there.
        let faces = faces_on(&untraced, CutFaces::InDoubt, &plane, &plane);
        assert!(faces.len() == 1 && (faces[0].area() - 100.0).abs() < 1e-9, "{faces:?}");
    }
}
