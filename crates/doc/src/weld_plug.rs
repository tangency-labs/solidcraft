//! Plug and slot welds: the body that fills a hole or slot modelled in one part down to the part
//! under it ([`plug_weld`]), flush, convex or concave, to the full depth or partly
//! ([`PlugStyle`]).
//!
//! SolidCraft's own, as the spot welds (`weld_spot`): solidcore and solidcore-ops hold no weld
//! (jyviko and bherbruck on #18409). Every measurement and every body here is an ops call, always on
//! a copy, so the parts are never changed; this module adds the weld's rules. The `weld.plug`
//! command and its storage come with #15131.
//!
//! # The geometry
//!
//! The *top part* has the hole; the *part below* lies under it. The hole's *bottom face* is the top
//! part's planar face that faces the part below and holds the hole as an inner loop; its outward
//! normal is the weld's way down. The weld's size is that loop: the hole at the faying surface
//! (AWS A2.4 9.2).
//!
//! The *hole's faces* are found from the bottom loop across edges, up to the one face that also
//! touches the faces round the bottom face's outer loop (the part's sides): the *top face*. A hole
//! that reaches a side is open, so it is no plug or slot. The faces between may be anything: a
//! straight wall, a countersink, a counterbore's step.
//!
//! The fill is those faces closed by two flat caps (the bottom and top loops) and sewn into a
//! solid, so it follows the walls whatever their shape. A partial fill is that solid with
//! everything above `depth` cut off; a convex face is ops' dome (#19532) on the fill's top face; a
//! concave face takes the same dome, turned down, out of it.
//!
//! The *floor* is the face of the part below that a line down from the bottom loop meets first.
//! It must be flat and square to the way down and hold the whole bottom loop: no edge of the floor
//! within the zone tolerance of the loop (the gap added when the parts do not touch), and no loop of
//! the floor inside it. A gap between the parts is not filled: the weld stands on the top part's
//! bottom face, and [`Plug::gap`] names the gap for a notice (Alexander, 26 Nov).
//!
//! # Not covered
//!
//! A dome needs a top outline that is star-shaped about its centre ([`DomeRefusal::NotStar`]): an
//! L-shaped slot is filled flush or partly, not convex or concave.

use solidcore_record::brep::Brep;
use solidcraft_kernel::services::boolean::{BooleanOp, boolean};
use solidcraft_kernel::services::dome::{DomeRefusal, DomeShape, dome};
use solidcraft_kernel::services::measure::PointClass;
use solidcraft_kernel::services::stitch::sew;
use solidcraft_kernel::services::sweep::{self, Curve, DraftCorner};
use solidcraft_kernel::services::trim::remove_faces;
use solidcraft_kernel::services::{ExactBody as Body, Order};

/// Why a plug or slot weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlugRefusal {
    /// Input the weld cannot take: a size out of range, a face the top part does not have.
    #[error("{0}")]
    Invalid(&'static str),
    /// Record face `face` of the top part is not a face of a hole closed all round that opens
    /// onto a planar face facing the part below.
    #[error("face {face} is not a face of a closed hole over the part below")]
    NotAHole {
        /// The picked face.
        face: usize,
    },
    /// The hole has no single flat top face square to the way down.
    #[error("the hole has no single flat top face square to the weld")]
    NoTopFace,
    /// The floor, record face `face` of the part below, is not flat and square to the way down.
    #[error("the floor under the hole, face {face} of the part below, is not flat and square to the weld")]
    FloorNotFlat { face: usize },
    /// The floor, record face `face` of the part below, does not hold the whole hole: one of its
    /// edges runs under the hole, or it has a hole of its own there.
    #[error("the floor under the hole, face {face} of the part below, does not reach under the whole hole")]
    FloorShort { face: usize },
    /// A partial depth or a concave face's depth reaches `max` or past it.
    #[error("the depth reaches {max} or past it")]
    TooDeep { max: f64 },
    /// The hole's faces and its two caps do not sew into a solid.
    #[error("the hole's faces do not close into a solid")]
    NotClosed,
    /// The dome of a convex or concave face is refused, in the kernel's words.
    #[error("the weld's face: {0}")]
    Dome(DomeRefusal),
    /// An ops call failed.
    #[error(transparent)]
    Ops(solidcraft_kernel::services::Error),
}

impl From<solidcraft_kernel::services::Error> for PlugRefusal {
    fn from(e: solidcraft_kernel::services::Error) -> Self {
        match e {
            solidcraft_kernel::services::Error::Dome(refusal) => PlugRefusal::Dome(refusal),
            e => PlugRefusal::Ops(e),
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

/// One part, copied, with its record and the record's order.
struct Copied {
    body: Body,
    record: Brep,
    order: Order,
}

fn copied(body: &Body, order: &Order) -> Result<(Copied, Order)> {
    let (copy, mapped) = body.copy_with_order(order)?;
    let (record, own) = copy.to_record()?;
    Ok((Copied { body: copy, record, order: own }, mapped))
}

/// The sweep curves of record edges `edges`: each edge's NURBS form when the record has one.
fn curves(record: &Brep, edges: &[usize]) -> Result<Vec<Curve>> {
    let mut out = Vec::new();
    for &e in edges {
        let Some(edge) = record.edges.get(e) else {
            return Err(PlugRefusal::Invalid("an edge the record does not have"));
        };
        out.extend(Curve::from_record(edge.nurbs.as_ref().unwrap_or(&edge.curve), edge.interval)?);
    }
    Ok(out)
}

/// The weld that fills the hole of the top part whose record face `wall` the user picked (any face
/// of the hole), down to the part below, in `style`. Both parts are read, never changed: every solve
/// runs on a copy. `top` and `below` are each a part and its record order.
///
/// # Errors
/// [`PlugRefusal::Invalid`] for a face the top part does not have or a size that is not finite and
/// positive. The other [`PlugRefusal`]s where the parts do not take the weld; the errors of the ops
/// calls otherwise.
pub fn plug_weld(top: (&Body, &Order), below: (&Body, &Order), wall: usize, style: &PlugStyle) -> Result<Plug> {
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
    let (t, mapped) = copied(top.0, top.1)?;
    let Some(picked) = mapped.faces.get(wall).and_then(|id| t.order.faces.iter().position(|f| f == id)) else {
        return Err(PlugRefusal::Invalid("a face the top part does not have"));
    };
    let (b, _) = copied(below.0, below.1)?;
    let tol = sweep::zone_tolerance()?;

    let hole = find_hole(&t, &b, picked)?;
    let gap = floor_under(&t, &b, &hole, tol)?;

    // the fill by the walls: the hole's faces and two caps, sewn
    let others: Vec<usize> = (0..t.order.faces.len()).filter(|f| !hole.faces.contains(f)).collect();
    let walls = remove_faces(&t.body, &t.order, &others)?;
    let bottom_cap = sweep::planar_faces(&curves(&t.record, &hole.bottom_loop)?)?;
    let top_cap = sweep::planar_faces(&curves(&t.record, &hole.top_loop)?)?;
    let sewn = sew(&[&walls, &bottom_cap, &top_cap], tol)?;
    if !sewn.closed {
        return Err(PlugRefusal::NotClosed);
    }
    let mut fill = sewn.body;

    let Some(thickness) = t.body.face_square_level(&t.order, hole.top, hole.up, hole.at)?.map(|l| l.level) else {
        return Err(PlugRefusal::NoTopFace);
    };
    let level = match style.fill {
        PlugFill::Full => thickness,
        PlugFill::Partial { depth } if depth >= thickness => return Err(PlugRefusal::TooDeep { max: thickness }),
        PlugFill::Partial { depth } => {
            // everything above `depth`: the bottom face's outline raised by it
            let cutter = sweep::extrude(&curves(&t.record, &hole.outer_loop)?, hole.up, thickness, 0.0, DraftCorner::Curvature, true)?;
            let [x, y, z] = hole.up.map(|v| v * depth);
            cutter.transform(&[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]])?;
            fill = boolean(fill, cutter, BooleanOp::Subtract)?.ok_or(PlugRefusal::NotClosed)?;
            depth
        }
    };

    let body = match style.face {
        PlugFace::Flush => fill,
        PlugFace::Convex { height, shape } => {
            let (_, fo) = fill.to_record()?;
            let face = square_face_at(&fill, &fo, hole.up, hole.at, level)?.ok_or(PlugRefusal::NoTopFace)?;
            domed(&fill, &fo, face, height, shape)?
        }
        PlugFace::Concave { depth, .. } if depth >= level => return Err(PlugRefusal::TooDeep { max: level }),
        PlugFace::Concave { depth, shape } => {
            // a prism of the weld's top outline, its lower face domed down, taken out of the fill
            let (record, fo) = fill.to_record()?;
            let face = square_face_at(&fill, &fo, hole.up, hole.at, level)?.ok_or(PlugRefusal::NoTopFace)?;
            let outline = fill.face_loops(&fo, face)?.into_iter().next().ok_or(PlugRefusal::NoTopFace)?;
            let tool = sweep::extrude(&curves(&record, &outline)?, hole.up, thickness, 0.0, DraftCorner::Curvature, true)?;
            let (_, too) = tool.to_record()?;
            let low = square_face_at(&tool, &too, hole.up, hole.at, level)?.ok_or(PlugRefusal::NoTopFace)?;
            let dished = domed(&tool, &too, low, depth, shape)?;
            boolean(fill, dished, BooleanOp::Subtract)?.ok_or(PlugRefusal::NotClosed)?
        }
    };
    Ok(Plug { body, gap: (gap > tol).then_some(gap) })
}

/// A hole of the top part, read from its copy: record faces and edges of [`Copied::order`].
struct Hole {
    /// The bottom face, its outer loop and the hole's loop on it.
    bottom: usize,
    outer_loop: Vec<usize>,
    bottom_loop: Vec<usize>,
    /// The hole's faces, its top face and the loop on it.
    faces: Vec<usize>,
    top: usize,
    top_loop: Vec<usize>,
    /// The way up, out of the part below, and a point of the bottom loop.
    up: [f64; 3],
    at: [f64; 3],
}

/// The hole of the top part `t` that face `picked` belongs to, opening onto a face that faces the
/// part below `b`.
fn find_hole(t: &Copied, b: &Copied, picked: usize) -> Result<Hole> {
    let adj = t.body.adjacency(&t.order)?;
    let across = |edges: &[usize], not: usize| -> Vec<usize> {
        let mut v: Vec<usize> = edges.iter().filter_map(|&e| adj.edge_faces.get(e)).flatten().copied().filter(|&f| f != not).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    for bottom in 0..t.order.faces.len() {
        let Some(normal) = t.body.face_plane_normal(&t.order, bottom)? else { continue };
        let down = normal.value;
        let loops = t.body.face_loops(&t.order, bottom)?;
        let Some((outer_loop, inner)) = loops.split_first() else { continue };
        for bottom_loop in inner {
            let Some(&first) = bottom_loop.first() else { continue };
            let at = t.body.edge_middle(&t.order, first)?;
            // the loop must face the part below
            if b.body.face_hits_by_ray(&b.order, at, down)?.is_empty() {
                continue;
            }
            let sides = across(outer_loop, bottom);
            let touches_sides = |f: usize| {
                adj.face_edges
                    .get(f)
                    .is_some_and(|es| es.iter().any(|&e| adj.edge_faces.get(e).is_some_and(|fs| fs.iter().any(|g| sides.contains(g)))))
            };
            let mut faces = across(bottom_loop, bottom);
            let (mut tops, mut opens) = (Vec::new(), false);
            let mut stack = faces.clone();
            while let Some(f) = stack.pop() {
                for g in across(adj.face_edges.get(f).map_or(&[][..], Vec::as_slice), f) {
                    if g == bottom || faces.contains(&g) || tops.contains(&g) {
                        continue;
                    }
                    if sides.contains(&g) {
                        opens = true;
                    } else if touches_sides(g) {
                        tops.push(g);
                    } else {
                        faces.push(g);
                        stack.push(g);
                    }
                }
            }
            if !faces.contains(&picked) {
                continue;
            }
            if opens {
                return Err(PlugRefusal::NotAHole { face: picked });
            }
            let [top] = tops[..] else {
                return Err(PlugRefusal::NoTopFace);
            };
            let top_loop = adj.face_edges.get(top).map_or(Vec::new(), |es| {
                es.iter().copied().filter(|&e| adj.edge_faces.get(e).is_some_and(|fs| fs.iter().any(|g| faces.contains(g)))).collect()
            });
            return Ok(Hole {
                bottom,
                outer_loop: outer_loop.clone(),
                bottom_loop: bottom_loop.clone(),
                faces,
                top,
                top_loop,
                up: down.map(|v| -v),
                at,
            });
        }
    }
    Err(PlugRefusal::NotAHole { face: picked })
}

/// The gap under the hole, once the floor is found flat, square and holding the whole bottom loop.
fn floor_under(t: &Copied, b: &Copied, hole: &Hole, tol: f64) -> Result<f64> {
    let down = hole.up.map(|v| -v);
    let first = b.body.face_hits_by_ray(&b.order, hole.at, down)?;
    let Some(hit) = first.first() else {
        return Err(PlugRefusal::NotAHole { face: hole.bottom });
    };
    let (floor, gap) = (hit.face, hit.distance);
    if b.body.face_square_level(&b.order, floor, hole.up, hole.at)?.is_none() {
        return Err(PlugRefusal::FloorNotFlat { face: floor });
    }
    // no edge of the floor under the loop: none closer to it than the gap
    let rims = b.body.rim_edges(&b.order, &[floor])?;
    for &e in &hole.bottom_loop {
        let curve = t.body.edge_curve(&t.order, e)?;
        if b.body.closest_to_curve(&b.order, &[], &rims, &curve)?.is_none_or(|c| c.distance <= gap + tol) {
            return Err(PlugRefusal::FloorShort { face: floor });
        }
    }
    // no loop of the floor inside the loop: one point of each, against the loop's prism down to it
    let prism = sweep::extrude(&curves(&t.record, &hole.bottom_loop)?, down, gap + tol.max(1.0), 0.0, DraftCorner::Curvature, true)?;
    for l in b.body.face_loops(&b.order, floor)? {
        let Some(&e) = l.first() else { continue };
        let q = b.body.edge_middle(&b.order, e)?;
        if prism.classify_point(q)? != PointClass::Outside {
            return Err(PlugRefusal::FloorShort { face: floor });
        }
    }
    Ok(gap)
}

/// The dome of `height` on record face `face` of `body` in `shape`; with none, spherical where the
/// face is a circle (the dome itself says when it is not) and elliptical otherwise.
fn domed(body: &Body, order: &Order, face: usize, height: f64, shape: Option<DomeShape>) -> Result<Body> {
    let Some(shape) = shape else {
        return match dome(body, order, face, height, DomeShape::Spherical, false) {
            Err(solidcraft_kernel::services::Error::Dome(DomeRefusal::SphericalNeedsCircle)) => {
                Ok(dome(body, order, face, height, DomeShape::Elliptical, false)?)
            }
            r => Ok(r?),
        };
    };
    Ok(dome(body, order, face, height, shape, false)?)
}

/// The face of `body` square to `up` at `level` above `at`.
fn square_face_at(body: &Body, order: &Order, up: [f64; 3], at: [f64; 3], level: f64) -> Result<Option<usize>> {
    for f in 0..order.faces.len() {
        if let Some(l) = body.face_square_level(order, f, up, at)?
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
