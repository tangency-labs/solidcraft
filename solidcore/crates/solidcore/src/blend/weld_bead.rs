//! Weld beads as a body of their own (#15129, the welds epic #15132): the beads a
//! [`FilletSession::prepare_beads_along`] lays along a body's edges, returned apart from it.
//! The body they are laid on is left as it was.
//!
//! The beads are laid on a copy of the body, as a blend merges them, and the body is taken
//! away from that copy again (a boolean difference): what is left is the material the beads
//! added, each bead bounded by its own face, the strips of the body's faces between its edge
//! and its rails, and its ends. In an inside corner a bead only adds material, so the
//! difference is the beads exactly; the strips and ends lie on the body's own surfaces.
//!
//! A bead broken along a straight edge (an intermittent weld, chain or staggered) is the
//! bead apart cut into its spans ([`beads_apart_in_spans`]): each span is the bead's body
//! within the slab between the planes square to the edge at the span's ends, so it ends in
//! the bead's section there, a plane face.
//!
//! Not upstream (solidcore's). No recursion.

use crate::blend::fillet_executive::session::FilletSession;
use crate::boolean::api_brep::{boolean_difference, boolean_intersection};
use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::math::{Point3, Vec3};
use crate::primitive::api_curves::create_line_segment;
use crate::surface::procedural_blend::bead::BeadSpan;
use crate::sweep::api_brep::create_linear_sweep;
use crate::topology::edge;
use crate::boolean::merge::p3::copy_brep;
use crate::error::{EntityRef, Result, SolidError};
use crate::kernel::Kernel;
use crate::surface::procedural_blend::chamfer::ChamferSize;
use crate::topology::proto_topology::p1::copy_brep_map::CopyBrepMap;
use crate::topology::{Brep, EdgeId, FaceId};

const OP: &str = "blend::weld_bead::beads_apart";

/// The copy of `entity` that `map` (source to copy) holds.
fn copy_of(map: &CopyBrepMap<EntityRef>, entity: EntityRef) -> Result<EntityRef> {
    map.get_at(Some(entity))?
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge or face with no copy"))
}

/// The weld beads along `edges` of `target`, as [`FilletSession::prepare_beads_along`] lays
/// them (`sizes`, `sagitta`, `along` as there), as one body apart from `target`, which is
/// left as it was: one lump per bead where the beads do not touch.
///
/// # Errors
///
/// Those of [`FilletSession::prepare_beads_along`] and of the blend that lays the beads;
/// native invalid input for an edge or face of `along` that is not `target`'s; those of the
/// copies and of the boolean difference.
pub fn beads_apart(
    k: &mut Kernel,
    target: &Brep,
    edges: &[EdgeId],
    sizes: &[ChamferSize],
    sagitta: f64,
    along: &[FaceId],
) -> Result<Brep> {
    let mut map = CopyBrepMap::new();
    let mut laid = copy_brep(k, target, None, Some(&mut map), true, true)?;
    let edges = edges
        .iter()
        .map(|&e| match copy_of(&map, EntityRef::Edge(e))? {
            EntityRef::Edge(c) => Ok(c),
            _ => Err(SolidError::invalid_input(OP, "an edge's copy is no edge")),
        })
        .collect::<Result<Vec<_>>>()?;
    let along = along
        .iter()
        .map(|&f| match copy_of(&map, EntityRef::Face(f))? {
            EntityRef::Face(c) => Ok(c),
            _ => Err(SolidError::invalid_input(OP, "a face's copy is no face")),
        })
        .collect::<Result<Vec<_>>>()?;
    {
        let mut session = FilletSession::new(k, &mut laid)?;
        session.prepare_beads_along(k, &edges, sizes, sagitta, &along)?;
        session.create_corners(k, None)?;
        session.do_filleting(k, None)?;
    }
    let body = copy_brep(k, target, None, None, true, true)?;
    boolean_difference(k, laid, body)
}

/// The bead along the straight edge `edge` of `target` ([`beads_apart`], with `size`,
/// `sagitta` and `along`), as one body per span of `spans`, in their order: each the bead
/// between the planes square to the edge at its span's ends (fractions of the edge's length
/// from the start of its curve's interval). `target` is left as it was.
///
/// # Errors
///
/// Those of [`beads_apart`]; native invalid input for no span, spans out of order or
/// overlapping ([`BeadSpan`]), or an edge that is not a line (a curved edge's spans end on
/// its normal planes, which a slab does not bound: not built yet); those of the slabs'
/// sweeps and of the boolean intersections.
pub fn beads_apart_in_spans(
    k: &mut Kernel,
    target: &Brep,
    edge_id: EdgeId,
    size: ChamferSize,
    sagitta: f64,
    along: &[FaceId],
    spans: &[BeadSpan],
) -> Result<Vec<Brep>> {
    let spans = BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?;
    let curve = target
        .edge(edge_id)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
    if !matches!(curve.kind(), CurveKind::Line(_)) {
        return Err(SolidError::invalid_input(
            OP,
            "a bead's spans are cut along a straight edge only so far",
        ));
    }
    let interval = edge::interval(target, edge_id)?;
    let point = |t: f64| -> Result<Point3> {
        let pv = curve.evaluate(k, interval.evaluate(t), 0, false, false)?;
        pv.first()
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "the edge's curve gave no point"))
    };
    let start = point(0.0)?;
    let run = point(1.0)? - start;
    let length = run.length();
    let dir = run.unitize()?;
    let along_faces = along.to_vec();
    let whole = beads_apart(k, target, &[edge_id], &[size], sagitta, &along_faces)?;
    // The slab's half width: past the bead's section, which lies within its setbacks and
    // sagitta's reach of the edge (the arc's radius bounds an overhang's).
    let setbacks: f64 = match size {
        ChamferSize::Distances([a, b]) => a + b,
        ChamferSize::DistanceAngle { distance, .. } => 4.0 * distance,
        ChamferSize::Chord(c) => 4.0 * c,
    };
    let half = 4.0 * (setbacks + sagitta.abs()) + 1.0;
    let [u, v] = square_to(dir)?;
    let mut out = Vec::with_capacity(spans.len());
    for span in spans {
        let o = start + run * span.from;
        let corners = [
            o + u * half + v * half,
            o - u * half + v * half,
            o - u * half - v * half,
            o + u * half - v * half,
        ];
        let sides = corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .map(|(&a, &b)| create_line_segment(a, b).map(Curve::from))
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        let slab = create_linear_sweep(k, &refs, dir, length * span.share(), true)?;
        let piece = copy_brep(k, &whole, None, None, true, true)?;
        out.push(boolean_intersection(k, piece, slab)?);
    }
    Ok(out)
}

/// Two unit vectors square to the unit `dir` and to each other, `u x v = dir`.
fn square_to(dir: Vec3) -> Result<[Vec3; 2]> {
    let pick = if dir.x.abs() <= dir.y.abs() && dir.x.abs() <= dir.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if dir.y.abs() <= dir.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let u = pick.cross(dir).unitize()?;
    let v = dir.cross(u);
    Ok([u, v])
}

/// The length of the bead along `edge_id` of `target` in each of `spans`, in their order:
/// the edge's arc length times the span's share (spans are fractions of that length). For a
/// weld table; a bead's volume is its body's ([`crate::mass::integrator::properties::brep_volume`] of a body from
/// [`beads_apart`] or [`beads_apart_in_spans`]).
///
/// # Errors
///
/// Native invalid input for spans out of order or overlapping ([`BeadSpan`]) or an edge with
/// no curve; those of the edge's arc length.
pub fn span_lengths(
    k: &Kernel,
    target: &Brep,
    edge_id: EdgeId,
    spans: &[BeadSpan],
) -> Result<Vec<f64>> {
    let spans = BeadSpan::along(&spans.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>())?;
    let curve = target
        .edge(edge_id)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
    let length = curve.length(k, &edge::interval(target, edge_id)?, LENGTH_ACCURACY)?;
    Ok(spans.iter().map(|s| length * s.share()).collect())
}

/// The relative accuracy [`span_lengths`] asks of an edge's arc length (a line's and a
/// circle's are exact whatever is asked).
const LENGTH_ACCURACY: f64 = 1e-12;
