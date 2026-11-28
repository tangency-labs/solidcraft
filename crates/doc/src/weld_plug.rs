//! Plug and slot welds: the body that fills a hole or slot modelled in one part down to the part
//! under it ([`plug_weld`]), flush, convex or concave, to the full depth or partly
//! ([`PlugStyle`]).
//!
//! SolidCraft's own, as the spot welds (`weld_spot`): solidcore and solidcore-ops hold no weld
//! (jyviko and bherbruck on #18409). Every measurement and every body here is a kernel call through
//! the kernel interface, which reads the parts and never changes them; this module adds the weld's
//! rules. The `weld.plug` command and its storage come with #15131.
//!
//! # The geometry
//!
//! The *top part* has the hole; the *part below* lies under it. The hole's *bottom face* is the top
//! part's planar face that faces the part below and holds the hole as an inner loop; its outward
//! normal is the weld's way down. The weld's size is that loop: the hole at the faying surface
//! (AWS A2.4 9.2). A face that faces down with a hole in it but lies above the faying surface (the
//! ceiling of a counterbore from below) is no bottom face: the top found from it is not above it,
//! so it is passed over and the faying face is taken.
//!
//! The *hole's faces* are found from the bottom loop across edges, up to the one face that also
//! touches the faces round the bottom face's outer loop (the part's sides): the *top face*. A hole
//! that reaches a side is open, so it is no plug or slot; so is a hole whose top is not flat and
//! square to the weld, as the walk goes on through it to the sides. The faces between may be
//! anything: a straight wall, a countersink, a counterbore's step. The face the user picks must be
//! one of the hole's faces: its top and bottom faces belong to the part as a whole.
//!
//! The fill is those faces closed by two flat caps (the bottom and top loops) and sewn into a
//! solid, so it follows the walls whatever their shape. A partial fill is that solid with
//! everything above `depth` cut off; a convex face is ops' dome (#19532) on the fill's top face; a
//! concave face takes the same dome, turned down, out of it.
//!
//! The *floor* is the face of the part below that a line down from the bottom loop meets first.
//! It must be flat and square to the way down, face up (else the line started inside the part
//! below: the parts overlap) and hold the whole bottom loop: no edge of the floor
//! within the zone tolerance of the loop (the gap added when the parts do not touch), and no loop of
//! the floor inside it. A gap between the parts is not filled: the weld stands on the top part's
//! bottom face, and [`Plug::gap`] names the gap for a notice (Alexander, 26 Nov).
//!
//! # Not covered
//!
//! A dome needs a top outline that is star-shaped about its centre ([`DomeRefusal::NotStar`]): an
//! L-shaped slot is filled flush or partly, not convex or concave.

use std::f64::consts::FRAC_PI_2;

use solidcore_record::Vec3;
use solidcore_record::brep::Loop;
use solidcraft_kernel::api::FaceHit;
use solidcraft_kernel::profiles::{self, ProfileCurve as Curve};
use solidcraft_kernel::{Body, BoolOp, DomeRefusal, DomeShape, KernelError, PointClass, affine};

/// Why a plug or slot weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlugRefusal {
    /// Input the weld cannot take: a size out of range, a face the top part does not have.
    #[error("{0}")]
    Invalid(&'static str),
    /// Record face `face` of the top part is not a face of a hole closed all round that opens
    /// onto a planar face facing the part below: no hole's face, the hole's own top or bottom
    /// face, or a hole that reaches the part's sides (a top that is not flat and square to the weld
    /// included).
    #[error("face {face} is not a face of a closed hole over the part below")]
    NotAHole {
        /// The picked face.
        face: usize,
    },
    /// The hole has no single flat top face square to the way down with a hole in it (a blind hole
    /// from below).
    #[error("the hole has no single flat top face square to the weld")]
    NoTopFace,
    /// The floor, record face `face` of the part below, is not flat and square to the way down.
    #[error("the floor under the hole, face {face} of the part below, is not flat and square to the weld")]
    FloorNotFlat { face: usize },
    /// The floor, record face `face` of the part below, does not hold the whole hole: one of its
    /// edges runs under the hole, or it has a hole of its own there.
    #[error("the floor under the hole, face {face} of the part below, does not reach under the whole hole")]
    FloorShort { face: usize },
    /// The kernel found no closest points between the floor's edges, on record face `face` of the
    /// part below, and the hole's loop, so the floor could not be checked.
    #[error("the floor under the hole, face {face} of the part below, could not be measured against the hole")]
    FloorUnmeasured { face: usize },
    /// The line down from the hole starts inside the part below: the parts overlap there. Record
    /// face `face` of the part below is the first face it meets, facing down.
    #[error("the part below reaches into the top part under the hole (face {face} of the part below)")]
    PartsOverlap { face: usize },
    /// A partial depth or a concave face's depth reaches `max` or past it.
    #[error("the depth reaches {max} or past it")]
    TooDeep { max: f64 },
    /// The hole's faces and its two caps do not sew into a solid.
    #[error("the hole's faces do not close into a solid")]
    NotClosed,
    /// The dome of a convex or concave face is refused, in the kernel's words.
    #[error("the weld's face: {0}")]
    Dome(DomeRefusal),
    /// The kernel refused a profile's body, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A kernel call on a body failed.
    #[error(transparent)]
    Kernel(KernelError),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
}

impl From<KernelError> for PlugRefusal {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::Dome(refusal) => PlugRefusal::Dome(refusal),
            e => PlugRefusal::Kernel(e),
        }
    }
}

type Result<T> = std::result::Result<T, PlugRefusal>;

/// The weld's face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlugFace {
    /// Flat, at the top face or at a partial fill's depth (the default, Q32).
    Flush,
    /// Raised by `height` in the middle, with ops' dome `shape` (spherical on round holes only).
    Convex {
        /// The rise in the middle above the flush face.
        height: f64,
        /// The dome's meridian; `None`: spherical on a round hole, elliptical on any other (Q126).
        shape: Option<DomeShape>,
    },
    /// Dished by `depth` in the middle: the dome turned down.
    Concave {
        /// The dip in the middle below the flush face.
        depth: f64,
        /// The dish's meridian; `None`: spherical on a round hole, elliptical on any other (Q126).
        shape: Option<DomeShape>,
    },
}

/// The convex height or concave depth a weld tool proposes when the user has used none before
/// (Q126; the last one used is the tool's to keep). No standard gives a plug weld's crown: ISO 5817
/// only caps excess weld metal (level B: 1 mm + 0.1 b, at most 5 mm).
pub const PROPOSED_CROWN: f64 = 1.0;

/// How deep the weld fills the hole.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlugFill {
    /// Up to the top face (the default, Q32).
    Full,
    /// Up to `depth` above the bottom face.
    Partial {
        /// The fill's height above the bottom face.
        depth: f64,
    },
}

/// What a plug or slot weld is built with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlugStyle {
    /// The weld's face.
    pub face: PlugFace,
    /// How deep it fills the hole.
    pub fill: PlugFill,
}

impl Default for PlugStyle {
    fn default() -> Self {
        PlugStyle { face: PlugFace::Flush, fill: PlugFill::Full }
    }
}

/// A plug or slot weld's body.
#[derive(Debug)]
pub struct Plug {
    /// The weld, for display: it fills metal the parts do not have.
    pub body: Body,
    /// How far the part below lies from the top part under the hole, when more than the zone
    /// tolerance: the gap is not filled, and the weld tool says so.
    pub gap: Option<f64>,
}

fn v3(p: [f64; 3]) -> Vec3 {
    p.into()
}

/// The edges of each loop of record face `face`, its outer loop first.
fn face_loops(b: &Body, face: usize) -> Vec<Vec<usize>> {
    let Some(f) = b.record().faces.get(face) else { return Vec::new() };
    f.loops
        .iter()
        .filter_map(|l| match l {
            Loop::Edges { edges } => Some(edges.iter().map(|e| e.edge as usize).collect()),
            Loop::Vertex { .. } => None,
        })
        .collect()
}

/// The faces that use record edge `edge`.
fn edge_faces(b: &Body, edge: usize) -> Vec<usize> {
    b.record().edges.get(edge).map_or(Vec::new(), |e| e.uses.iter().map(|u| u.face as usize).collect())
}

/// Where a line down from `at` (against `up`) meets the faces of `b`, nearest first. The line
/// starts `tol` above `at`, so a point the kernel puts a hair inside a part it lies on still meets
/// that part's face first; the distances are from `at`.
fn hits_below(b: &Body, at: [f64; 3], up: [f64; 3], tol: f64) -> Result<Vec<FaceHit>> {
    let from = std::array::from_fn(|i| at[i] + up[i] * tol);
    let mut hits = b.face_hits_by_ray(from, up.map(|v| -v))?;
    for h in &mut hits {
        h.distance = (h.distance - tol).max(0.0);
    }
    Ok(hits)
}

/// The point at the middle of record edge `edge`.
fn edge_middle(b: &Body, edge: usize) -> Result<[f64; 3]> {
    match solidcraft_kernel::edge_middle(b, edge)? {
        Some(p) => Ok(p.into()),
        None => Err(PlugRefusal::Invalid("an edge whose middle the kernel cannot find")),
    }
}

/// The profile curves of record edges `edges` of `b`: each edge's NURBS form when the record has
/// one.
fn curves(b: &Body, edges: &[usize]) -> Result<Vec<Curve>> {
    let mut out = Vec::new();
    for &e in edges {
        let Some(edge) = b.record().edges.get(e) else {
            return Err(PlugRefusal::Invalid("an edge the record does not have"));
        };
        out.extend(profiles::curve_pieces(edge.nurbs.as_ref().unwrap_or(&edge.curve), edge.interval)?);
    }
    Ok(out)
}

/// How far the box round `b` reaches: its edges added up, at least its diagonal.
fn reach_of(b: &Body) -> Result<f64> {
    let bounds = b.exact_bounds()?;
    let (lo, hi): ([f64; 3], [f64; 3]) = (bounds.min.into(), bounds.max.into());
    Ok(lo.iter().zip(hi).map(|(a, b)| b - a).sum::<f64>())
}

/// The weld that fills the hole of the top part whose record face `wall` the user picked (any face
/// of the hole), down to the part below, in `style`. Both parts are read, never changed.
///
/// # Errors
/// [`PlugRefusal::Invalid`] for a face the top part does not have or a size that is not finite and
/// positive. The other [`PlugRefusal`]s where the parts do not take the weld; the errors of the
/// kernel calls otherwise.
pub fn plug_weld(top: &Body, below: &Body, wall: usize, style: &PlugStyle) -> Result<Plug> {
    let positive = |v: f64| v.is_finite() && v > 0.0;
    let sizes_ok = match style.face {
        PlugFace::Flush => true,
        PlugFace::Convex { height, .. } => positive(height),
        PlugFace::Concave { depth, .. } => positive(depth),
    } && match style.fill {
        PlugFill::Full => true,
        PlugFill::Partial { depth } => positive(depth),
    };
    if !sizes_ok {
        return Err(PlugRefusal::Invalid("a size that is not finite and positive"));
    }
    if wall >= top.face_count() {
        return Err(PlugRefusal::Invalid("a face the top part does not have"));
    }
    let tol = profiles::zone_tolerance()?;

    let hole = find_hole(top, below, wall, tol)?;
    let gap = floor_under(top, below, &hole, tol)?;

    // the fill by the walls: the hole's faces and two caps, sewn
    let others: Vec<usize> = (0..top.face_count()).filter(|f| !hole.faces.contains(f)).collect();
    let walls = solidcraft_kernel::remove_faces(top, &others)?;
    let bottom_cap = profiles::planar_faces(&curves(top, &hole.bottom_loop)?)?;
    let top_cap = profiles::planar_faces(&curves(top, &hole.top_loop)?)?;
    let sewn = solidcraft_kernel::stitch(&[&walls, &bottom_cap, &top_cap], tol)?;
    if sewn.open_edge_count()? > 0 {
        return Err(PlugRefusal::NotClosed);
    }
    let mut fill = sewn;

    let thickness = hole.thickness;
    let level = match style.fill {
        PlugFill::Full => thickness,
        PlugFill::Partial { depth } if depth >= thickness => return Err(PlugRefusal::TooDeep { max: thickness }),
        PlugFill::Partial { depth } => {
            let cutter = above(&fill, hole.up, hole.at, depth)?;
            fill = solidcraft_kernel::boolean(&fill, &cutter, BoolOp::Cut)?.ok_or(PlugRefusal::NotClosed)?;
            depth
        }
    };

    let body = match style.face {
        PlugFace::Flush => fill,
        PlugFace::Convex { height, shape } => {
            let face = square_face_at(&fill, hole.up, hole.at, level)?.ok_or(PlugRefusal::NoTopFace)?;
            domed(&fill, face, height, shape)?
        }
        PlugFace::Concave { depth, .. } if depth >= level => return Err(PlugRefusal::TooDeep { max: level }),
        PlugFace::Concave { depth, shape } => {
            // a prism of the weld's top outline, its lower face domed down, taken out of the fill
            let face = square_face_at(&fill, hole.up, hole.at, level)?.ok_or(PlugRefusal::NoTopFace)?;
            let outline = face_loops(&fill, face).into_iter().next().ok_or(PlugRefusal::NoTopFace)?;
            let tool = profiles::extrude_curves(&curves(&fill, &outline)?, hole.up, thickness, true)?;
            let low = square_face_at(&tool, hole.up, hole.at, level)?.ok_or(PlugRefusal::NoTopFace)?;
            let dished = domed(&tool, low, depth, shape)?;
            solidcraft_kernel::boolean(&fill, &dished, BoolOp::Cut)?.ok_or(PlugRefusal::NotClosed)?
        }
    };
    Ok(Plug { body, gap: (gap > tol).then_some(gap) })
}

/// A hole of the top part: its record faces and edges.
struct Hole {
    /// The bottom face and the hole's loop on it.
    bottom: usize,
    bottom_loop: Vec<usize>,
    /// The hole's faces and the loop on its top face.
    faces: Vec<usize>,
    top_loop: Vec<usize>,
    /// The top face's level above the bottom face.
    thickness: f64,
    /// The way up, out of the part below, and a point of the bottom loop.
    up: [f64; 3],
    at: [f64; 3],
}

/// The hole of the top part `t` that face `picked` belongs to, opening onto a face that faces the
/// part below `b` at the faying surface: a candidate whose top is not more than `tol` above it is
/// passed over (brett19 on #22646).
fn find_hole(t: &Body, b: &Body, picked: usize, tol: f64) -> Result<Hole> {
    let n = t.face_count();
    let face_edges: Vec<Vec<usize>> = (0..n).map(|f| face_loops(t, f).concat()).collect();
    let across = |edges: &[usize], not: usize| -> Vec<usize> {
        let mut v: Vec<usize> = edges.iter().flat_map(|&e| edge_faces(t, e)).filter(|&f| f != not).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    for bottom in 0..n {
        let Some(normal) = t.face_plane_normal(bottom)? else { continue };
        let down = normal.value;
        let loops = face_loops(t, bottom);
        let Some((outer_loop, inner)) = loops.split_first() else { continue };
        // the stops of this bottom face's way up, found once for all its loops
        let mut stop: Option<Vec<bool>> = None;
        for bottom_loop in inner {
            let up = down.map(|v| -v);
            // the loop must face the part below: the first point of it with the part below under it
            let mut under = None;
            for &e in bottom_loop {
                let p = edge_middle(t, e)?;
                if !hits_below(b, p, up, tol)?.is_empty() {
                    under = Some(p);
                    break;
                }
            }
            let Some(at) = under else { continue };
            // A flat face square to the weld with a hole in it is a stop: the top, or a step inside
            // the hole (a counterbore's). The part's outside is everything reached from the faces
            // round the bottom face without crossing a stop; the hole is everything reached from
            // the bottom loop the same way. A stop next to both is the top; a stop next to the hole
            // only is a step, and the hole goes on through it. Chamfered or split outer edges are
            // just more outside (CodeRabbit on #22646).
            if stop.is_none() {
                let mut flags = vec![false; n];
                for (g, s) in flags.iter_mut().enumerate() {
                    *s = g != bottom && t.face_square_level(g, v3(up), v3(at))?.is_some() && face_loops(t, g).len() >= 2;
                }
                stop = Some(flags);
            }
            let stop = stop.as_deref().unwrap_or(&[]);
            let walk = |from: Vec<usize>, region: &mut Vec<usize>, met: &mut Vec<usize>| {
                let mut stack = from;
                while let Some(f) = stack.pop() {
                    if f == bottom || region.contains(&f) || met.contains(&f) {
                        continue;
                    }
                    if stop.get(f).copied().unwrap_or(false) {
                        met.push(f);
                        continue;
                    }
                    region.push(f);
                    stack.extend(across(face_edges.get(f).map_or(&[][..], Vec::as_slice), f));
                }
            };
            let (mut outside, mut outer_stops) = (Vec::new(), Vec::new());
            walk(across(outer_loop, bottom), &mut outside, &mut outer_stops);
            let (mut faces, mut stops) = (Vec::new(), Vec::new());
            walk(across(bottom_loop, bottom), &mut faces, &mut stops);
            loop {
                let steps: Vec<usize> = stops.iter().copied().filter(|g| !outer_stops.contains(g) && !faces.contains(g)).collect();
                if steps.is_empty() {
                    break;
                }
                for g in steps {
                    stops.retain(|&s| s != g);
                    faces.push(g);
                    let next = across(face_edges.get(g).map_or(&[][..], Vec::as_slice), g);
                    walk(next, &mut faces, &mut stops);
                }
            }
            // the picked face must be the hole's own: the top face is the part's, and a pick of it
            // would name whichever hole comes first
            if !faces.contains(&picked) {
                continue;
            }
            if faces.iter().any(|f| outside.contains(f)) {
                return Err(PlugRefusal::NotAHole { face: picked });
            }
            let [top] = stops[..] else {
                return Err(PlugRefusal::NoTopFace);
            };
            let Some(thickness) = t.face_square_level(top, v3(up), v3(at))?.map(|l| l.level) else {
                return Err(PlugRefusal::NoTopFace);
            };
            // not the faying surface: a face above it, as a counterbore's ceiling from below
            if thickness <= tol {
                continue;
            }
            let top_loop = face_edges
                .get(top)
                .map_or(Vec::new(), |es| es.iter().copied().filter(|&e| edge_faces(t, e).iter().any(|g| faces.contains(g))).collect());
            return Ok(Hole { bottom, bottom_loop: bottom_loop.clone(), faces, top_loop, thickness, up, at });
        }
    }
    Err(PlugRefusal::NotAHole { face: picked })
}

/// The gap under the hole, once the floor is found flat, square and holding the whole bottom loop.
fn floor_under(t: &Body, b: &Body, hole: &Hole, tol: f64) -> Result<f64> {
    let down = hole.up.map(|v| -v);
    let first = hits_below(b, hole.at, hole.up, tol)?;
    let Some(hit) = first.first() else {
        return Err(PlugRefusal::NotAHole { face: hole.bottom });
    };
    let (floor, gap) = (hit.face, hit.distance);
    if b.face_square_level(floor, v3(hole.up), v3(hole.at))?.is_none() {
        return Err(PlugRefusal::FloorNotFlat { face: floor });
    }
    // a floor facing down: the line started inside the part below (brett19 on #22646)
    let faces_up = b.face_plane_normal(floor)?.is_some_and(|n| n.value.iter().zip(hole.up).map(|(a, u)| a * u).sum::<f64>() > 0.0);
    if !faces_up {
        return Err(PlugRefusal::PartsOverlap { face: floor });
    }
    // no edge of the floor under the loop: none closer to it than the gap
    let rims = b.rim_edges(&[floor])?;
    for &e in &hole.bottom_loop {
        let Some(closest) = b.closest_to_edge_of(&[], &rims, t, e)? else {
            return Err(PlugRefusal::FloorUnmeasured { face: floor });
        };
        if closest.distance <= gap + tol {
            return Err(PlugRefusal::FloorShort { face: floor });
        }
    }
    // no loop of the floor inside the loop: one point of each, against the loop's prism down to it.
    // The prism runs on past the floor by the top part's own size, so a floor loop at the gap lies
    // inside it, not on its end face, at any scale.
    let prism = profiles::extrude_curves(&curves(t, &hole.bottom_loop)?, down, gap + reach_of(t)?, true)?;
    for l in face_loops(b, floor) {
        let Some(&e) = l.first() else { continue };
        let q = edge_middle(b, e)?;
        if prism.classify_point(v3(q))? != PointClass::Outside {
            return Err(PlugRefusal::FloorShort { face: floor });
        }
    }
    Ok(gap)
}

/// A block holding everything of `fill` above `depth` along `up` from `at`: a square in the plane at
/// that depth, wider than the fill's box reaches from it, extruded up as far. It does not lean on
/// the plate's outline, so a hole that widens past it is still cut (CodeRabbit on #22646).
fn above(fill: &Body, up: [f64; 3], at: [f64; 3], depth: f64) -> Result<Body> {
    // the box's edges added up are at least its diagonal: no point of the fill is farther from
    // the square's middle than that plus the depth
    let reach = reach_of(fill)? + depth;
    // the kernel chooses a direction itself when x runs along up
    let [_, u] = affine::orthonormal_axes(up, [1.0, 0.0, 0.0])?;
    let w = affine::apply_point(&affine::rotation([0.0; 3], up, FRAC_PI_2)?, u);
    let corner = |a: f64, b: f64| std::array::from_fn(|i| at[i] + up[i] * depth + (u[i] * a + w[i] * b) * reach);
    let square = [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)];
    let sides: Vec<Curve> = (0..4).map(|i| Curve::Line { start: square[i], end: square[(i + 1) % 4] }).collect();
    Ok(profiles::extrude_curves(&sides, up, reach, true)?)
}

/// The dome of `height` on record face `face` of `body` in `shape`; with none, spherical where the
/// face is a circle (the dome itself says when it is not) and elliptical otherwise.
fn domed(body: &Body, face: usize, height: f64, shape: Option<DomeShape>) -> Result<Body> {
    let Some(shape) = shape else {
        return match solidcraft_kernel::dome(body, face, height, DomeShape::Spherical, false) {
            Err(KernelError::Dome(DomeRefusal::SphericalNeedsCircle)) => {
                Ok(solidcraft_kernel::dome(body, face, height, DomeShape::Elliptical, false)?)
            }
            r => Ok(r?),
        };
    };
    Ok(solidcraft_kernel::dome(body, face, height, shape, false)?)
}

/// The face of `body` square to `up` at `level` above `at`.
fn square_face_at(body: &Body, up: [f64; 3], at: [f64; 3], level: f64) -> Result<Option<usize>> {
    for f in 0..body.face_count() {
        if let Some(l) = body.face_square_level(f, v3(up), v3(at))?
            && (l.level - level).abs() <= l.tolerance
        {
            return Ok(Some(f));
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "weld_plug_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
