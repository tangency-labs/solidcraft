//! Two concave blends separated at a vertex by an opening in their floor.
//!
//! The vertex link has six sectors. Around it are the first blend, the upright
//! corner of the post, the second blend, one rim of the opening, its downward
//! corner, and the other rim. The two pairs of walls across the floor continue
//! in the same oriented planes. Each blend can end against its own trim
//! support, using the ordinary single-edge End construction.

use crate::blend::blend_corner::mixed::leaving;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::topology::TopologyFailure;
use crate::error::{Result, SolidError};
use crate::geom::certified::{distance, next_down, next_up};
use crate::geom::expansion;
use crate::geom::extent3d::Extent3d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::gamma;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, vertex, vertexuse, Brep, EdgeId, FaceId, VertexId};

mod corridor;
use corridor::{certified_limit_span, resolved_vertex, Corridor};

const OP: &str = "blend::blend_corner::separate_ends::classify_separate_ends";

struct Fan {
    edges: [EdgeId; 6],
    sectors: [FaceId; 6],
}

/// Two selections that meet at a candidate cutout corner.
pub struct SeparateEndPair {
    /// Where the selected edges share an endpoint.
    pub vertex: VertexId,
    /// The two edges to blend.
    pub edges: [EdgeId; 2],
    /// Offsets or radii in the same order as the edges.
    pub sizes: [f64; 2],
}

/// Whether separate Ends fit the selected geometry.
pub enum SeparateEndFit {
    /// The selection does not qualify for separate Ends.
    NotApplicable,
    /// Every pair fits and the blend envelopes are disjoint.
    Fits,
    /// A common size exceeds the certified room for the whole selection.
    Limited {
        /// The largest representable common size allowed by the supports.
        size: f64,
        /// The selected edge whose support limits the size.
        edge: EdgeId,
    },
}

struct Supports {
    corridor: Corridor,
    rails: [EdgeId; 3],
    span: Extent3d,
}

/// Qualifies separate Ends and finds a support limit for a common size.
///
/// # Errors
/// Propagates allocation failures and failures reading or evaluating the candidate geometry.
pub fn independent_separate_ends(
    k: &Kernel,
    b: &mut Brep,
    pairs: &[SeparateEndPair],
) -> Result<SeparateEndFit> {
    let common = pairs
        .first()
        .is_some_and(|first| pairs.iter().all(|pair| pair.sizes == [first.sizes[0]; 2]));
    let mut supports = Vec::new();
    supports
        .try_reserve_exact(pairs.len())
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    let mut limit: Option<(f64, EdgeId)> = None;
    for pair in pairs {
        k.check_cancelled(OP)?;
        let [a, z] = pair.sizes;
        if !pair
            .sizes
            .into_iter()
            .all(|size| size.is_finite() && size > 0.0)
        {
            return Ok(SeparateEndFit::NotApplicable);
        }
        let Some(geometry) = classify_separate_ends(k, b, pair.vertex, pair.edges)? else {
            return Ok(SeparateEndFit::NotApplicable);
        };
        let size = a.max(z);
        let [first, second] = pair.edges;
        let (_, at) = geometry.corridor.at;
        for (((_, end), rail), (width, edge)) in geometry
            .corridor
            .ends
            .into_iter()
            .zip(geometry.rails)
            .zip([(a, first), (z, second), (size, first)])
        {
            match has_room(at, end, width) {
                Some(true) => {}
                Some(false) if common => {
                    let Some(fit) = support_limit(at, end) else {
                        return Ok(SeparateEndFit::NotApplicable);
                    };
                    if limit.is_none_or(|(old, _)| fit < old) {
                        if !support_stops(b, rail, pair.vertex, at, end)? {
                            return Ok(SeparateEndFit::NotApplicable);
                        }
                        limit = Some((fit, edge));
                    }
                }
                _ => return Ok(SeparateEndFit::NotApplicable),
            }
        }
        supports.push((geometry, size));
    }

    let mut envelopes = Vec::new();
    envelopes
        .try_reserve_exact(supports.len())
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))?;
    for (geometry, requested) in supports {
        let size = limit.map_or(requested, |(fit, _)| fit);
        let span = if limit.is_some() {
            let Some(span) = certified_limit_span(k, b, &geometry.corridor, size)? else {
                return Ok(SeparateEndFit::NotApplicable);
            };
            span
        } else {
            geometry.span
        };
        let Some(envelope) = swept_envelope(span, size) else {
            return Ok(SeparateEndFit::NotApplicable);
        };
        if envelopes
            .iter()
            .any(|other| !envelope.are_disjoint(other, 0.0))
        {
            return Ok(SeparateEndFit::NotApplicable);
        }
        envelopes.push(envelope);
    }
    Ok(match limit {
        Some((size, edge)) => SeparateEndFit::Limited { size, edge },
        None => SeparateEndFit::Fits,
    })
}

/// Recognizes separate Ends with square trim supports at `v` and records their lengths.
///
/// A right-angle concave blend fits its trim support only up to the neighboring
/// rim length and the post height. The far ends must also have square trim
/// planes so the ordinary end construction stays inside the swept bounds.
/// The span encloses the two selected edges for a subsequent interference check.
///
/// This checks a single closed manifold fan of straight edges and oriented exterior
/// planes. The post and opening occupy opposite quadrants of the floor. At the
/// corner their two wall planes continue past the floor with the same outward
/// senses. The shape and dimensions of the opening away from `v` are immaterial.
///
/// # Errors
/// Propagates failures reading the B-rep and evaluating the incident edges.
fn classify_separate_ends(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
    picked: [EdgeId; 2],
) -> Result<Option<Supports>> {
    let [first, second] = picked;
    let Some(fan) = vertex_fan(b, v, first)? else {
        return Ok(None);
    };
    let Fan {
        edges: [a, b1, c, d, e, f],
        sectors: [ab, bc, cd, de, ef, fa],
    } = fan;
    // The starting edge use can be on either side of the shell.
    let (up, rim_b, down, rim_a, faces) = if c == second {
        (b1, d, e, f, [ab, bc, cd, de, ef, fa])
    } else if e == second {
        (f, d, c, b1, [fa, ef, de, cd, bc, ab])
    } else {
        return Ok(None);
    };

    let u = leaving(k, b, a, v, OP)?;
    let w = leaving(k, b, second, v, OP)?;
    let n = leaving(k, b, up, v, OP)?;
    let back_u = leaving(k, b, rim_b, v, OP)?;
    let back_n = leaving(k, b, down, v, OP)?;
    let back_w = leaving(k, b, rim_a, v, OP)?;
    // Roundoff for the unit tangents and their dot products and differences.
    // The model's angular and intersection tolerances do not enter this test.
    let roundoff = gamma(64);
    let square = [u.dot(w), u.dot(n), w.dot(n)]
        .into_iter()
        .all(|dot| dot.abs() <= roundoff);
    let opposite = [u + back_u, n + back_n, w + back_w]
        .into_iter()
        .all(|sum| sum.length() <= roundoff);
    if !(square && opposite) {
        return Ok(None);
    }

    let at = vertex::point(b, v)?;
    for (face, normal) in faces.into_iter().zip([-w, -u, n, -w, -u, n]) {
        if !oriented_plane_at(b, face, at, normal, roundoff)? {
            return Ok(None);
        }
    }

    // With line edges and plane supports the oriented dihedral is constant.
    if !(is_concave_edge(k, b, first)? && is_concave_edge(k, b, second)?) {
        return Ok(None);
    }
    let (Some((vertex_a, end_a, far_a)), Some((vertex_b, end_b, far_b))) = (
        square_cap(b, first, v, u, roundoff)?,
        square_cap(b, second, v, w, roundoff)?,
    ) else {
        return Ok(None);
    };
    let rails = [rim_a, rim_b, up];
    let (Some(rim_a), Some(rim_b), Some(up)) = (
        other_end(b, rim_a, v)?,
        other_end(b, rim_b, v)?,
        other_end(b, up, v)?,
    ) else {
        return Ok(None);
    };
    let span = Extent3d::from_points(at, end_a).union(&Extent3d::from_point(end_b));
    Ok(Some(Supports {
        corridor: Corridor {
            at: (v, at),
            ends: [rim_a, rim_b, up],
            far: [vertex_a, vertex_b],
            faces,
            far_caps: [far_a, far_b],
        },
        rails,
        span,
    }))
}

fn swept_envelope(span: Extent3d, size: f64) -> Option<Extent3d> {
    let lo = span.min();
    let hi = span.max();
    let outward_low = Point3::new(
        next_down(lo.x - size),
        next_down(lo.y - size),
        next_down(lo.z - size),
    );
    let outward_high = Point3::new(
        next_up(hi.x + size),
        next_up(hi.y + size),
        next_up(hi.z + size),
    );
    let envelope = Extent3d::from_points(outward_low, outward_high);
    envelope.is_bounded().then_some(envelope)
}

fn square_cap(
    b: &Brep,
    e: EdgeId,
    v: VertexId,
    outward: Vec3,
    roundoff: f64,
) -> Result<Option<(VertexId, Point3, FaceId)>> {
    let Some((end, at)) = other_end(b, e, v)? else {
        return Ok(None);
    };
    if vertex::edges(b, end, None)?.len() != 3 {
        return Ok(None);
    }
    for face in vertex::faces(b, end)? {
        if oriented_plane_at(b, face, at, outward, roundoff)? {
            return Ok(Some((end, at, face)));
        }
    }
    Ok(None)
}

/// A vertex that just splits a straight support is not a geometric bound on its reach.
fn support_stops(b: &Brep, rail: EdgeId, start: VertexId, at: Point3, end: Point3) -> Result<bool> {
    let Some(v) = edge::other_vertex(b, rail, Some(start))? else {
        return Ok(false);
    };
    let from = [at.x, at.y, at.z];
    let to = [end.x, end.y, end.z];
    let mut varying = from
        .into_iter()
        .zip(to)
        .enumerate()
        .filter(|(_, (a, z))| a != z);
    let Some((axis, (a, z))) = varying.next() else {
        return Ok(false);
    };
    if varying.next().is_some() {
        return Ok(false);
    }
    for continuation in vertex::edges(b, v, None)?
        .into_iter()
        .filter(|&id| id != rail)
    {
        if !matches!(
            edge::curve(b, continuation)?.map(Curve::kind),
            Some(CurveKind::Line(_))
        ) {
            return Ok(false);
        }
        let Some((end_vertex, _)) = other_end(b, continuation, v)? else {
            return Ok(false);
        };
        let Some(p) = resolved_vertex(b, end_vertex)? else {
            return Ok(false);
        };
        let q = [p.x, p.y, p.z];
        if q.into_iter().zip(to).enumerate().all(|(i, (p, e))| {
            if i == axis {
                (a < z && p > e) || (a > z && p < e)
            } else {
                p == e
            }
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn other_end(b: &Brep, e: EdgeId, v: VertexId) -> Result<Option<(VertexId, Point3)>> {
    let Some(end) = edge::other_vertex(b, e, Some(v))?.filter(|&end| end != v) else {
        return Ok(None);
    };
    let p = vertex::point(b, end)?;
    Ok([p.x, p.y, p.z]
        .into_iter()
        .all(f64::is_finite)
        .then_some((end, p)))
}

/// Compare squared lengths exactly when the distance bound straddles the requested
/// size, including exact equality. Refuse an arithmetic range the expansion cannot
/// represent.
fn has_room(start: Point3, end: Point3, size: f64) -> Option<bool> {
    let measured = distance(start, end);
    if measured.value.is_finite() {
        if next_down(measured.value - measured.bound) >= size {
            return Some(true);
        }
        if next_up(measured.value + measured.bound) < size {
            return Some(false);
        }
    }
    let size_squared = expansion::product(size, size)?;
    let slack = [(start.x, end.x), (start.y, end.y), (start.z, end.z)]
        .into_iter()
        .try_fold(expansion::sub(&[], &size_squared), |sum, (a, b)| {
            let delta = expansion::sub(&[a], &[b]);
            delta.iter().try_fold(sum, |sum, &term| {
                Some(expansion::add(&sum, &expansion::scale(&delta, term)?))
            })
        });
    slack
        .filter(|slack| slack.iter().all(|part| part.is_finite()))
        .map(|slack| expansion::sign(&slack) >= 0)
}

/// A distance's floating-point enclosure spans only a few values at ordinary scales.
/// Find the last one that fits, keeping the upper endpoint strictly outside by rounding
/// the enclosure outward.
fn support_limit(start: Point3, end: Point3) -> Option<f64> {
    let measured = distance(start, end);
    let lower = next_down(measured.value - measured.bound);
    let upper = next_up(measured.value + measured.bound);
    if !(lower > 0.0 && upper.is_finite() && upper > lower) {
        return None;
    }
    let (mut lo, mut hi) = (lower.to_bits(), upper.to_bits());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if has_room(start, end, f64::from_bits(mid))? {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(f64::from_bits(lo))
}

/// Each sector lies between its outward edge and the next edge of the fan.
fn vertex_fan(b: &Brep, v: VertexId, start: EdgeId) -> Result<Option<Fan>> {
    let incident = vertex::edges(b, v, None)?;
    if incident.len() != 6 || !incident.contains(&start) {
        return Ok(None);
    }

    let uses = vertex::vertexuses_of_edge(b, v, Some(start), None)?;
    let Some(&start_use) = uses.first() else {
        return Ok(None);
    };
    let Some(start_use) = vertexuse::edgeuse(b, start_use)? else {
        return Ok(None);
    };
    let mut current = start_use;
    let mut edges = Vec::with_capacity(6);
    let mut sectors = Vec::with_capacity(6);
    for _ in 0..6 {
        let (Some(edge), Some(face), Some(clockwise)) = (
            eu1::edge(b, current)?,
            eu1::face(b, current)?,
            eu1::cw_edgeuse(b, current)?,
        ) else {
            return Ok(None);
        };
        if !incident.contains(&edge)
            || edges.contains(&edge)
            || !edge::is_manifold(b, edge)?
            || !matches!(
                edge::curve(b, edge)?.map(Curve::kind),
                Some(CurveKind::Line(_))
            )
        {
            return Ok(None);
        }
        let Some(next) = eu1::radial(b, clockwise)? else {
            return Ok(None);
        };
        edges.push(edge);
        sectors.push(face);
        current = next;
    }
    if current != start_use {
        return Ok(None);
    }
    let (Ok(edges), Ok(sectors)) = (edges.try_into(), sectors.try_into()) else {
        return Ok(None);
    };
    Ok(Some(Fan { edges, sectors }))
}

fn oriented_plane_at(
    b: &Brep,
    face: FaceId,
    at: Point3,
    outward: Vec3,
    roundoff: f64,
) -> Result<bool> {
    let Some(SurfaceKind::Plane(plane)) = face2::surface(b, face)?.map(Surface::kind) else {
        return Ok(false);
    };
    let Some(outside) = b.infinite_region() else {
        return Ok(false);
    };
    let [up, down] = face2::face_side_regions(b, face)?;
    let (origin, normal, _) = plane.is_planar();
    let (normal, inside) = match (up, down) {
        (Some(up), Some(down)) if up == outside && down != outside => (normal, down),
        (Some(up), Some(down)) if down == outside && up != outside => (-normal, up),
        _ => return Ok(false),
    };
    if b.region(inside)?.is_void() != 0 {
        return Ok(false);
    }
    let delta = at - origin;
    let scale = delta.length();
    Ok(scale.is_finite()
        && normal.dot(delta).abs() <= roundoff * scale
        && (normal - outward).length() <= roundoff)
}
