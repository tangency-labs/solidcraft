//! Not upstream (WR-614, tangency-labs/solidcore#1794; opt-in
//! `Kernel::fixes.boolean_coincident_strip_drops_crossing`): the crossing line of two
//! planes that stay within tolerance of each other across a strip bounded by their edges
//! is not an edge.
//!
//! Two planar faces `fa` (B-rep 1) and `fb` (B-rep 2) crossing at a small angle meet in a
//! line fixed only to `tol / sin(angle)`. When one face has an edge lying on the other
//! (an edge-on-face edge definition, within tolerance by construction), parallel to the
//! crossing, covering it, farther from it than the tolerance and with the face's interior
//! toward the crossing, and on the opposite side of the crossing the other face ends at an
//! edge likewise parallel to, covering and facing the crossing within the edge/face
//! intersection tolerance (either as a second edge-on-face definition or as a boundary
//! edge within `GetXSectTol3d(edge, plane_face)`), the two planes stay within tolerance of
//! each other across the whole strip where both faces exist (the gap between two planes is
//! linear across it). Upstream still makes the crossing an edge definition: its end
//! vertices are picked per B-rep (`SmProtoVertex::GetFinalVertex`) and never related by
//! `SetRelationships`, so `SmMerge::MakeFaceuse` fails on the edge's inconsistent vertex
//! mates (`SM_ERR`). Removing the crossing's edge definition before `Resolve` (and its
//! proto edge when that is left without one) leaves the strip bounded by the existing
//! edges, while keeping crossings where either face continues out of tolerance past the
//! crossing.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `TopologyIntersector::drop_crossings_in_coincident_strips` (crate) | none (own design) |
//!
//! Deviations: not upstream at all; off by default.

use crate::error::Result;
use crate::geom::solution_array::SolutionObject;
use crate::geom::tol::{xsect_tol3d, zone_tol3d};
use crate::intersect::point_class::{ClassObject, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::proto_topology::{EdgeDefId, ProtoEdgeId};

use super::p1::{ignore_soft, TopologyIntersector};
use super::p2::IntersectorBrepP2;

const OP_DROP_STRIP: &str =
    "boolean::topology_intersector::coincident_strip::drop_crossings_in_coincident_strips";

/// An edge definition as the strip test reads it: its proto edge, its source objects and
/// their classification types in B-reps 1 and 2, the start, midpoint and end of its curve
/// over its interval, and its tolerance.
struct Def<O> {
    pe: ProtoEdgeId,
    ed: EdgeDefId,
    src: [(PointClassType, Option<O>); 2],
    pts: [Point3; 3],
    tol: f64,
}

/// Origin and unit normal of a planar face's surface.
#[derive(Clone, Copy)]
struct PlanePos {
    origin: Point3,
    unit_normal: Vec3,
}

impl PlanePos {
    fn distance(self, pt: Point3) -> f64 {
        (pt - self.origin).dot(self.unit_normal).abs()
    }
}

impl<O: ClassObject + SolutionObject + Ord> TopologyIntersector<O> {
    /// Removes every face/face edge definition of two planes whose crossing lies in a
    /// strip bounded on both sides by edges of the two faces within tolerance of the
    /// opposite plane (module docs), and every proto edge that is left without an edge
    /// definition, and returns the curves deleted with them.
    ///
    /// # Errors
    ///
    /// Those of the curve and binormal evaluations and the face/edge connection tests.
    pub(crate) fn drop_crossings_in_coincident_strips(
        &mut self,
        k: &Kernel,
        brep: &mut impl IntersectorBrepP2<O>,
    ) -> Result<Vec<O>> {
        k.check_cancelled(OP_DROP_STRIP)?;
        let Some(mgr) = self.proto_topo_mgr.as_mut() else {
            return Ok(Vec::new());
        };
        let mut defs = Vec::new();
        for &pe in mgr.proto_edges() {
            let Some(edge) = mgr.proto_edge(pe) else {
                continue;
            };
            for &ed in edge.edge_definitions() {
                let Some(def) = mgr.edge_def(ed) else {
                    continue;
                };
                let Some(curve) = def.xsect_curve_3d().and_then(|c| brep.curve(c)) else {
                    continue;
                };
                let ivl = def.xsect_interval();
                defs.push(Def {
                    pe,
                    ed,
                    src: [
                        (def.src_topo_type(1), def.src_topo(1)),
                        (def.src_topo_type(2), def.src_topo(2)),
                    ],
                    pts: [
                        curve.evaluate_point(k, ivl.min())?,
                        curve.evaluate_point(k, ivl.mid())?,
                        curve.evaluate_point(k, ivl.max())?,
                    ],
                    tol: def.tolerance(k),
                });
            }
        }
        let mut doomed: Vec<(ProtoEdgeId, EdgeDefId)> = Vec::new();
        for crossing in &defs {
            if in_coincident_strip(k, brep, crossing, &defs)? {
                doomed.push((crossing.pe, crossing.ed));
            }
        }
        let mut deleted = Vec::new();
        for (pe, ed) in doomed {
            if let Some(edge) = mgr.proto_edge_mut(pe) {
                ignore_soft(edge.remove_edge_definition(ed))?;
            }
            deleted.extend(mgr.delete_edge_definition(ed));
            if mgr
                .proto_edge(pe)
                .is_some_and(|e| e.num_edge_definitions() == 0)
            {
                ignore_soft(mgr.remove_proto_edge(k, pe))?;
                mgr.forget_proto_edge(pe);
                deleted.extend(mgr.delete_proto_edge_object(pe));
            }
        }
        Ok(deleted)
    }
}

/// The plane of `face` when its surface is planar with a valid normal.
fn plane_pos<O: ClassObject>(brep: &impl IntersectorBrepP2<O>, face: O) -> Option<PlanePos> {
    let srf = brep
        .face_surface(face)
        .flatten()
        .and_then(|s| brep.surface(s))?;
    let SurfaceKind::Plane(plane) = srf.kind() else {
        return None;
    };
    let pos = plane.position();
    let n = pos.z_axis();
    if !n.is_initialized() {
        return None;
    }
    let len = n.length();
    (len > EFF_ZERO).then(|| PlanePos {
        origin: pos.origin(),
        unit_normal: n / len,
    })
}

/// Whether `face`'s interior along `edge` lies toward the crossing line (`off` points from
/// the crossing line to `edge`, so `face`'s inward binormal at `edge`'s midpoint must dot
/// negatively with `off`).
fn faces_inward<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    edge: O,
    face: O,
    off: Vec3,
) -> Result<bool> {
    let Some(eu) = brep.edge_edgeuse_of_face(edge, face)? else {
        return Ok(false);
    };
    let Some(ivl) = brep.edge_interval(edge) else {
        return Ok(false);
    };
    let Some(binorm) = ignore_soft(brep.edgeuse_evaluate_binormal(k, eu, ivl.mid()))? else {
        return Ok(false);
    };
    Ok(binorm.is_initialized() && binorm.dot(off) < 0.0)
}

/// Maximum zone tolerance of `edge`'s start and end vertices.
fn edge_end_tol<O: ClassObject>(
    k: &Kernel,
    brep: &impl IntersectorBrepP2<O>,
    edge: O,
) -> Result<f64> {
    let v0 = brep.edge_start_vertex(edge)?;
    let v1 = brep.edge_other_vertex(edge, v0)?;
    let t0 = v0
        .and_then(|v| brep.topology_tolerance(v))
        .map_or(0.0, |t| zone_tol3d(k, t));
    let t1 = v1
        .and_then(|v| brep.topology_tolerance(v))
        .map_or(0.0, |t| zone_tol3d(k, t));
    Ok(t0.max(t1))
}

/// Whether `face` has a boundary edge within intersection tolerance of `other_plane` that
/// runs parallel to and covers the crossing line `(p0, dir, len)` on the side opposite to
/// at least one offset in `opposite_defs`, with `face`'s interior toward the crossing.
#[allow(clippy::too_many_arguments)]
fn face_edge_bounds_strip<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    face: O,
    other_plane: PlanePos,
    other_face_tol: f64,
    p0: Point3,
    dir: Vec3,
    len: f64,
    crossing_tol: f64,
    opposite_defs: &[Vec3],
) -> Result<bool> {
    for edge in brep.face_edges(face)? {
        let Some(ivl) = brep.edge_interval(edge) else {
            continue;
        };
        let pts = {
            let Some(curve) = brep.edge_curve(edge).flatten().and_then(|c| brep.curve(c)) else {
                continue;
            };
            [
                curve.evaluate_point(k, ivl.min())?,
                curve.evaluate_point(k, ivl.mid())?,
                curve.evaluate_point(k, ivl.max())?,
            ]
        };
        let edge_tol = zone_tol3d(k, brep.topology_tolerance(edge).unwrap_or(0.0));
        let plane_tol = xsect_tol3d(k, edge_tol + other_face_tol).max(crossing_tol);
        if pts.iter().any(|&q| other_plane.distance(q) > plane_tol) {
            continue;
        }
        let end_tol = plane_tol + edge_end_tol(k, brep, edge)?;
        let Some(off) = band_offset_pts(p0, dir, len, pts, crossing_tol, end_tol) else {
            continue;
        };
        if !opposite_defs.iter().any(|other| off.dot(*other) < 0.0) {
            continue;
        }
        if faces_inward(k, brep, edge, face, off)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `crossing`, a face/face edge definition of two planes, lies in a strip bounded
/// on both sides by edges of the two faces within tolerance of the opposite plane, with at
/// least one side represented by an edge-on-face definition in `defs` (module docs).
fn in_coincident_strip<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl IntersectorBrepP2<O>,
    crossing: &Def<O>,
    defs: &[Def<O>],
) -> Result<bool> {
    let [(PointClassType::Face, Some(fa)), (PointClassType::Face, Some(fb))] = crossing.src else {
        return Ok(false);
    };
    let (Some(pa), Some(pb)) = (plane_pos(brep, fa), plane_pos(brep, fb)) else {
        return Ok(false);
    };
    let [p0, _, p1] = crossing.pts;
    let axis = p1 - p0;
    let len = axis.length();
    if len <= crossing.tol {
        return Ok(false);
    }
    let dir = axis / len;
    let mut def_on_fb = Vec::new();
    let mut def_on_fa = Vec::new();
    for def in defs.iter().filter(|d| d.ed != crossing.ed) {
        let edge_side = match def.src {
            [(PointClassType::Edge, Some(e)), (PointClassType::Face, Some(f))]
                if f == fb && brep.edge_is_connected_to_face(e, fa)? =>
            {
                Some((true, e, fa))
            }
            [(PointClassType::Face, Some(f)), (PointClassType::Edge, Some(e))]
                if f == fa && brep.edge_is_connected_to_face(e, fb)? =>
            {
                Some((false, e, fb))
            }
            _ => None,
        };
        let Some((is_a_on_fb, edge, owner_face)) = edge_side else {
            continue;
        };
        let end_tol = crossing.tol.max(def.tol) + edge_end_tol(k, brep, edge)?;
        let Some(off) = band_offset_pts(p0, dir, len, def.pts, crossing.tol, end_tol) else {
            continue;
        };
        if !faces_inward(k, brep, edge, owner_face, off)? {
            continue;
        }
        if is_a_on_fb {
            def_on_fb.push(off);
        } else {
            def_on_fa.push(off);
        }
    }
    if def_on_fb.is_empty() && def_on_fa.is_empty() {
        return Ok(false);
    }
    if def_on_fb
        .iter()
        .any(|a| def_on_fa.iter().any(|b| a.dot(*b) < 0.0))
    {
        return Ok(true);
    }
    let fa_tol = zone_tol3d(k, brep.topology_tolerance(fa).unwrap_or(0.0));
    let fb_tol = zone_tol3d(k, brep.topology_tolerance(fb).unwrap_or(0.0));
    if !def_on_fa.is_empty()
        && face_edge_bounds_strip(
            k,
            brep,
            fa,
            pb,
            fb_tol,
            p0,
            dir,
            len,
            crossing.tol,
            &def_on_fa,
        )?
    {
        return Ok(true);
    }
    if !def_on_fb.is_empty()
        && face_edge_bounds_strip(
            k,
            brep,
            fb,
            pa,
            fa_tol,
            p0,
            dir,
            len,
            crossing.tol,
            &def_on_fb,
        )?
    {
        return Ok(true);
    }
    Ok(false)
}

/// The offset from the crossing line (through `p0` along unit `dir`, `len` long) of the
/// segment `[q0, qm, q1]`, when it runs straight and parallel to the line within `tol`,
/// covers `[0, len]` within `end_tol` and lies farther from the line than `tol`; else
/// `None`.
fn band_offset_pts(
    p0: Point3,
    dir: Vec3,
    len: f64,
    [q0, qm, q1]: [Point3; 3],
    tol: f64,
    end_tol: f64,
) -> Option<Vec3> {
    let (s0, sm, s1) = ((q0 - p0).dot(dir), (qm - p0).dot(dir), (q1 - p0).dot(dir));
    let r0 = (q0 - p0) - dir * s0;
    let rm = (qm - p0) - dir * sm;
    let r1 = (q1 - p0) - dir * s1;
    let off = (r0 + r1) / 2.0;
    let parallel = (r0 - r1).length() <= tol && (rm - off).length() <= tol;
    let covers = s0.min(s1) <= end_tol && s0.max(s1) >= len - end_tol;
    (parallel && covers && off.length() > tol).then_some(off)
}
