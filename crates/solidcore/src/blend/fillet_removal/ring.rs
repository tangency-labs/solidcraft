//! Not upstream: removing a ring fillet (a round all the way round a boss's foot or rim)
//! and closing the hole with the sharp edge where its two tangent faces meet.
//!
//! Port of the SolidCraft fork's `SmRingFilletRemoval` (tangency-labs/smlib 52bfef0,
//! `SmFilletRemoval.cpp` / `.h`, and `SmBrep::JoinShellsAtVertex`, `SmBrep.cpp`), as
//! amended by 661f79c. WR-605; used by [`super::delete_fillet_face`] only with
//! `Kernel::fixes.fillet_removal_takes_ring_fillets`.
//!
//! | Rust                                    | Fork                                           |
//! |-----------------------------------------|------------------------------------------------|
//! | [`RingFilletRemoval`]                   | `SmRingFilletRemoval`                          |
//! | [`RingFilletRemoval::new`]              | `SmRingFilletRemoval::SmRingFilletRemoval`     |
//! | [`RingFilletRemoval::is_ring`]          | `SmRingFilletRemoval::IsRing`                  |
//! | [`RingFilletRemoval::remove_face`]      | `SmRingFilletRemoval::RemoveFace`              |
//! | `collect_ring_topology` (private)       | `SmRingFilletRemoval::CollectRingTopology`     |
//! | `intersect_tangent_surfaces` (private)  | `SmRingFilletRemoval::IntersectTangentSurfaces`|
//! | `find_side_edge_extension` (private)    | `SmRingFilletRemoval::FindSideEdgeExtension`   |
//! | `do_removal` (private)                  | `SmRingFilletRemoval::DoRemoval`               |
//! | [`join_shells_at_vertex`]               | `SmBrep::JoinShellsAtVertex`                   |
//!
//! Deviations:
//! - **661f79c.** With `Kernel::fixes.glue_vertices_joins_every_region` (WR-607) the
//!   glue joins the shells of every region, and `DoRemoval` neither calls
//!   `JoinShellsAtVertex` nor clips the tangent faces' domains to their surfaces', as
//!   the fork after 661f79c; without it, as 52bfef0.
//! - **Ownership.** The new curves are owned values moved into the B-rep (upstream's
//!   pointers, cleared once the B-rep has them). Each B-rep edit notifies through the
//!   kernel's callbacks (`with_kernel_callbacks`). The fork sets a new curve's owner
//!   after `SetCurve`; the port sets it before.
//! - **Null pointers.** Where the fork dereferences `NULL` (a rail without primary edge
//!   use), the port returns [`SolidError::InvalidInput`].
//! - No recursion.

use super::{bspline_part, glue_edges, ner, null, set_vertex_point, sm_err, OP};
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::Curve;
use crate::error::{ignore_status, EntityRef, Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::NotifyOperation;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{max2, Point3, BIG_DOUBLE, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::p4::NoObject;
use crate::topology::brep::p2::{self as brep_p2, NotifyArg};
use crate::topology::brep::p3::{self as brep_p3, with_kernel_callbacks, DeleteFaceOptions};
use crate::topology::edge::{self, SetCurveOptions};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p1::UvTrimCurveArg;
use crate::topology::edgeuse::p3::{get_or_create_uv_trim_curve, normalized_evaluate};
use crate::topology::topology_traverser::Marks;
use crate::topology::{shell, vertex, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

/// The removal of one ring fillet face (`SmRingFilletRemoval`, not upstream).
///
/// A ring fillet has no ends: its two rails are closed edges, each with one vertex, and
/// its surface's seam (if it has one) is an edge from one rail's vertex to the other's.
/// The face's edges are two closed tangent manifold edges and at most one seam edge
/// joining the rails' vertices; each rail's other face (a tangent face) has a surface
/// derived from `SmBSplineSurface`, and the two tangent faces differ; at each rail's
/// vertex the only other edges are the fillet's seam and at most one seam of the tangent
/// face (a cylinder's seam line).
///
/// The sharp edge is the surface/surface intersection of the two tangent faces'
/// surfaces within their natural domains, the closed curve nearest to the rails.
/// Nothing is continued past a surface's domain, and an edge past its curve only along
/// a line: otherwise [`RingFilletRemoval::remove_face`] fails with
/// [`RingFilletRemoval::not_exact`]. Everything is computed before the B-rep changes.
#[derive(Debug, Clone)]
pub struct RingFilletRemoval {
    /// `m_pFaceToDelete`.
    face_to_delete: Option<FaceId>,
    /// `m_d3dTol`.
    tol_3d: f64,
    /// `m_aRails`: the closed tangent edges.
    rails: [Option<EdgeId>; 2],
    /// `m_aTanFaces`: the face across each rail.
    tan_faces: [Option<FaceId>; 2],
    /// `m_aVertices`: each rail's vertex.
    vertices: [Option<VertexId>; 2],
    /// `m_pSeam`: the fillet's seam.
    seam: Option<EdgeId>,
    /// `m_aSideEdges`: the tangent face's seam at each rail's vertex.
    side_edges: [Option<EdgeId>; 2],
    /// `m_aSideAtHigh`: the side edge's high end is at the rail's vertex.
    side_at_high: [bool; 2],
    /// `m_aSideParams`: where each side edge reaches the new vertex, on its curve.
    side_params: [f64; 2],
    /// `m_aSideCurves`: a side edge's line continued to the new vertex (`None`: its
    /// curve reaches it).
    side_curves: [Option<Curve>; 2],
    /// `m_aSideUVs[side][k]`: the side edge's UV trims run on to the new vertex.
    side_uvs: [[Option<Curve>; 2]; 2],
    /// `m_aSideUVOwners[side][k]`: the edge uses that own them.
    side_uv_owners: [[Option<EdgeuseId>; 2]; 2],
    /// `m_sNewVertex`: the sharp edge's vertex.
    new_vertex: Point3,
    /// `m_pNewEdgeCurve`: the sharp edge, starting and ending at the new vertex, running
    /// as rail 0.
    new_edge_curve: Option<Curve>,
    /// `m_bNotExact`.
    not_exact: bool,
}

/// `lNumSamples`: the samples taken along each rail.
const NUM_SAMPLES: u32 = 8;

/// `pE->GetFaces(sFaces)`.
fn edge_faces(b: &Brep, e: EdgeId) -> Result<Vec<FaceId>> {
    edge::faces(b, e, None)
}

/// `SM_DEG2RAD`.
fn deg_to_rad(deg: f64) -> f64 {
    deg * (PI / 180.0)
}

impl RingFilletRemoval {
    /// `SmRingFilletRemoval(pFaceToDelete, d3dTolerance)`.
    #[must_use]
    pub fn new(face_to_delete: FaceId, tol_3d: f64) -> Self {
        Self {
            face_to_delete: Some(face_to_delete),
            tol_3d,
            rails: [None; 2],
            tan_faces: [None; 2],
            vertices: [None; 2],
            seam: None,
            side_edges: [None; 2],
            side_at_high: [false; 2],
            side_params: [0.0; 2],
            side_curves: [None, None],
            side_uvs: Default::default(),
            side_uv_owners: [[None; 2]; 2],
            new_vertex: Point3::UNDEF,
            new_edge_curve: None,
            not_exact: false,
        }
    }

    /// `NotExact`: [`RingFilletRemoval::remove_face`] failed because the sharp edge would
    /// need a surface or an edge continued past its domain.
    #[must_use]
    pub const fn not_exact(&self) -> bool {
        self.not_exact
    }

    /// `GetSharpEdge`: the edge the fillet became (after a successful
    /// [`RingFilletRemoval::remove_face`]).
    #[must_use]
    pub const fn sharp_edge(&self) -> Option<EdgeId> {
        self.rails[0]
    }

    /// `IsRing(pFace, dTangencyTolDeg)`: `face` is bounded as a ring fillet: two closed
    /// tangent manifold edges, and at most one edge whose only face is `face` (its
    /// seam). Nothing about the surface is checked.
    ///
    /// # Errors
    ///
    /// Those of the edge queries and of `IsTangentEdge` (which may make UV trim curves).
    pub fn is_ring(
        k: &Kernel,
        b: &mut Brep,
        face: Option<FaceId>,
        tangency_tol_deg: f64,
    ) -> Result<bool> {
        let Some(face) = face else {
            return Ok(false);
        };
        let edges = crate::topology::face::p4::edges(b, face, None, false, None)?;
        let (mut rails, mut seams) = (0u32, 0u32);
        for e in edges {
            let faces = edge_faces(b, e)?;
            if faces.len() == 1 && faces.first() == Some(&face) {
                seams += 1;
                continue;
            }
            if edge::is_closed(b, e)?
                && edge::is_manifold(b, e)?
                && edge::is_tangent_edge(k, b, e, tangency_tol_deg, false)?
            {
                rails += 1;
                continue;
            }
            return Ok(false);
        }
        Ok(rails == 2 && seams <= 1)
    }

    /// `RemoveFace`: collects the ring's topology, intersects the tangent surfaces,
    /// finds the side edges' extensions, then edits the B-rep.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a face that is not a removable ring fillet or when the sharp edge
    /// cannot be found exactly ([`RingFilletRemoval::not_exact`] then tells); those of
    /// the intersections and the edits.
    pub fn remove_face(&mut self, k: &mut Kernel, b: &mut Brep) -> Result<()> {
        self.collect_ring_topology(k, b)?;
        self.intersect_tangent_surfaces(k, b)?;
        self.find_side_edge_extension(k, b, 0)?;
        self.find_side_edge_extension(k, b, 1)?;

        // Everything is known: change the Brep.
        self.do_removal(k, b)
    }

    fn rail(&self, i: usize) -> Result<EdgeId> {
        ner(self.rails.get(i).copied().flatten())
    }

    fn tan_face(&self, i: usize) -> Result<FaceId> {
        ner(self.tan_faces.get(i).copied().flatten())
    }

    fn vertex(&self, i: usize) -> Result<VertexId> {
        ner(self.vertices.get(i).copied().flatten())
    }

    fn side_edge(&self, i: usize) -> Option<EdgeId> {
        self.side_edges.get(i).copied().flatten()
    }

    fn side_at_high(&self, i: usize) -> Result<bool> {
        self.side_at_high
            .get(i)
            .copied()
            .ok_or_else(|| null("side index"))
    }

    /// `CollectRingTopology`: the rails, the tangent faces, the rails' vertices, the
    /// fillet's seam and the tangent faces' seams at the vertices.
    fn collect_ring_topology(&mut self, k: &Kernel, b: &mut Brep) -> Result<()> {
        let tangency_tol_deg = 2.0; // as SmFilletRemoval::CollectFilletTopology
        let fil_face = ner(self.face_to_delete)?;
        if !Self::is_ring(k, b, Some(fil_face), tangency_tol_deg)? {
            return Err(sm_err());
        }

        let edges = crate::topology::face::p4::edges(b, fil_face, None, false, None)?;
        let mut rail = 0usize;
        for e in edges {
            let faces = edge_faces(b, e)?;
            if faces.len() == 1 && faces.first() == Some(&fil_face) {
                self.seam = Some(e);
                continue;
            }
            let [f0, f1] = faces.as_slice() else {
                return Err(sm_err());
            };
            if rail > 1 {
                return Err(sm_err());
            }
            let other = if *f0 == fil_face { *f1 } else { *f0 };
            *self.rails.get_mut(rail).ok_or_else(|| null("rail index"))? = Some(e);
            *self
                .tan_faces
                .get_mut(rail)
                .ok_or_else(|| null("rail index"))? = Some(other);
            rail += 1;
        }
        if rail != 2
            || self.tan_faces[0] == self.tan_faces[1]
            || self.tan_faces[0] == Some(fil_face)
            || self.tan_faces[1] == Some(fil_face)
        {
            return Err(sm_err());
        }
        for ii in 0..2 {
            let surface = crate::topology::face::p2::surface(b, self.tan_face(ii)?)?;
            match surface {
                Some(s) if s.is_kind_of(BSplineSurface::TYPE)? => {}
                _ => return Err(sm_err()),
            }
            let v = ner(edge::start_vertex(b, self.rail(ii)?)?)?;
            *self
                .vertices
                .get_mut(ii)
                .ok_or_else(|| null("rail index"))? = Some(v);
        }

        // The fillet's seam joins the two rails' vertices.
        if let Some(seam) = self.seam {
            let v0 = edge::start_vertex(b, seam)?;
            let v1 = edge::other_vertex(b, seam, v0)?;
            if !((v0 == self.vertices[0] && v1 == self.vertices[1])
                || (v0 == self.vertices[1] && v1 == self.vertices[0]))
            {
                return Err(sm_err());
            }
        }

        // At each rail's vertex, besides the rail and the fillet's seam: at most one seam
        // of the tangent face (an edge whose only face it is).
        for ii in 0..2 {
            let v = self.vertex(ii)?;
            let (rail, tan_face) = (self.rail(ii)?, self.tan_face(ii)?);
            for e in vertex::edges(b, v, None)? {
                if e == rail || Some(e) == self.seam {
                    continue;
                }
                let faces = edge_faces(b, e)?;
                if self.side_edge(ii).is_some()
                    || faces.len() != 1
                    || faces.first() != Some(&tan_face)
                    || edge::is_closed(b, e)?
                {
                    return Err(sm_err());
                }
                let at_high = edge::start_vertex(b, e)? != Some(v);
                *self
                    .side_edges
                    .get_mut(ii)
                    .ok_or_else(|| null("side index"))? = Some(e);
                *self
                    .side_at_high
                    .get_mut(ii)
                    .ok_or_else(|| null("side index"))? = at_high;
            }
        }
        Ok(())
    }

    /// `IntersectTangentSurfaces`: the sharp edge, where the two tangent faces' surfaces
    /// meet, the closed curve nearest to the rails, running as rail 0 and starting at the
    /// new vertex: where the sharp edge crosses a tangent face's seam (its seam edge is to
    /// run on to it), or where the curve starts when neither tangent face has a seam at
    /// the rail.
    #[allow(clippy::too_many_lines)]
    fn intersect_tangent_surfaces(&mut self, k: &Kernel, b: &mut Brep) -> Result<()> {
        let tol = self.tol_3d;
        let ssi = {
            let surf0 = ner(crate::topology::face::p2::surface(b, self.tan_face(0)?)?)?;
            let surf1 = ner(crate::topology::face::p2::surface(b, self.tan_face(1)?)?)?;
            surf0.global_surface_intersect(
                k,
                &surf0.natural_uv_domain_with_nurb(k)?,
                surf1,
                &surf1.natural_uv_domain_with_nurb(k)?,
                [true, true],
                Some(tol / 4.0),
                Some(deg_to_rad(20.0)),
            )?
        };
        let mut curves_3d: Vec<Option<Curve>> = ssi.curves_3d.into_iter().map(Some).collect();

        // The rails' samples, and how far apart the rails are: a sharp edge lies within a
        // few chords of its fillet's rails (r tan(phi/2) from each, the chord being
        // 2 r sin(phi/2), for faces turning by phi < 170 degrees: within 6 chords).
        let mut rail_pts: [Vec<Point3>; 2] = [Vec::new(), Vec::new()];
        for (ii, pts) in rail_pts.iter_mut().enumerate() {
            let prim = ner(eu1::primary_edgeuse(b, self.rail(ii)?)?)?;
            for kk in 0..NUM_SAMPLES {
                let t = (f64::from(kk) + 0.5) / f64::from(NUM_SAMPLES);
                pts.push(normalized_evaluate(k, b, prim, t, false, false, true)?.point);
            }
        }
        let mut chord = 0.0;
        {
            let rail1 = self.rail(1)?;
            let rail1_ivl = edge::interval(b, rail1)?;
            let rail1_curve = ner(edge::curve(b, rail1)?)?;
            for &p in &rail_pts[0] {
                let d = rail1_curve.drop_point(
                    k,
                    &rail1_ivl,
                    p,
                    None,
                    tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                if !d.success {
                    return Err(sm_err());
                }
                chord = max2(chord, d.distance);
            }
        }
        let reach = 6.0 * chord;

        // The closed curve every rail sample lies within reach of, the nearest.
        let mut found = curves_3d.len();
        let mut best = BIG_DOUBLE;
        for (cc, c) in curves_3d.iter().enumerate() {
            let Some(c) = c else {
                continue;
            };
            if c.is_degenerate(k, tol, None)? || !c.is_closed(k, &c.natural_interval(), tol)? {
                continue;
            }
            let mut worst = 0.0;
            'rails: for pts in &rail_pts {
                for &p in pts {
                    if worst > reach {
                        break 'rails;
                    }
                    let d = c.drop_point(
                        k,
                        &c.natural_interval(),
                        p,
                        None,
                        tol,
                        None,
                        SolverOperation::Minimize,
                    )?;
                    worst = if d.success {
                        max2(worst, d.distance)
                    } else {
                        BIG_DOUBLE
                    };
                }
            }
            if worst <= reach && worst < best {
                best = worst;
                found = cc;
            }
        }
        let Some(mut sharp) = curves_3d.get_mut(found).and_then(Option::take) else {
            // The surfaces don't meet near the fillet within their domains: the sharp
            // edge would lie on a continuation of one of them.
            self.not_exact = true;
            return Err(sm_err());
        };
        if bspline_part(&sharp).is_none() {
            return Err(SolidError::null_pointer(OP));
        }

        // Run as rail 0.
        {
            let rail0 = self.rail(0)?;
            let rail0_curve = ner(edge::curve(b, rail0)?)?;
            let mid = edge::interval(b, rail0)?.evaluate(0.5);
            // (SmCurve's default: non-zero tangents)
            let pv = rail0_curve.evaluate(k, mid, 1, true, true)?;
            let [p0, p1, ..] = pv.as_slice() else {
                return Err(null("short evaluation"));
            };
            let d = sharp.drop_point(
                k,
                &sharp.natural_interval(),
                *p0,
                None,
                tol,
                None,
                SolverOperation::Minimize,
            )?;
            if !d.success {
                return Err(sm_err());
            }
            // (SmBSplineCurve's default: exact tangents)
            let qv = sharp.evaluate(k, d.parameter, 1, true, false)?;
            let [_, q1, ..] = qv.as_slice() else {
                return Err(null("short evaluation"));
            };
            // the rail's own direction: its edge's orientation on the curve
            if p1.dot(*q1) < 0.0 {
                let old = sharp.natural_interval();
                sharp.reverse_parameterization(k, &old)?;
            }
        }

        // The new vertex: where the sharp edge crosses a tangent face's seam at its rail's
        // vertex.
        let ivl = sharp.natural_interval();
        let mut start_at = ivl.min();
        let mut have_vertex = false;
        for ii in 0..2 {
            let Some(side) = self.side_edge(ii) else {
                continue;
            };
            let mut side_curve = ner(edge::curve(b, side)?)?.clone();
            if bspline_part(&side_curve).is_none() {
                return Err(SolidError::null_pointer(OP));
            }
            let none = CurveIntersectOptions::<NoObject>::default();
            let mut solutions = sharp.global_curve_intersect(
                k,
                &ivl,
                &side_curve,
                &side_curve.natural_interval(),
                tol,
                &none,
            )?;
            if solutions.is_empty() {
                // The seam's curve stops short of the sharp edge (the fillet trimmed it):
                // a line runs on along itself (its end span continued, the same line),
                // anything else would have to be continued past its curve.
                if !side_curve.is_linear(k, tol, false)?.linear {
                    self.not_exact = true;
                    return Err(sm_err());
                }
                let side_ivl = side_curve.natural_interval();
                let at_high = self.side_at_high(ii)?;
                let end = if at_high {
                    side_ivl.max()
                } else {
                    side_ivl.min()
                };
                // (SmBSplineCurve's default: exact tangents)
                let pv = side_curve.evaluate(k, end, 1, true, false)?;
                let [_, d1, ..] = pv.as_slice() else {
                    return Err(null("short evaluation"));
                };
                let speed = d1.length();
                // (`!(dSpeed > SM_EFF_ZERO)`: a NaN speed fails too)
                if speed.is_nan() || speed <= EFF_ZERO {
                    return Err(sm_err());
                }
                let reach_param = (reach + 10.0 * tol) / speed;
                let far = if at_high {
                    end + reach_param
                } else {
                    end - reach_param
                };
                let part = ner(bspline_part(&side_curve))?;
                let longer = Curve::from(ner(part.create_extended_curve(
                    far,
                    ContinuityType::CInfinity,
                    true,
                )?)?);
                *self
                    .side_curves
                    .get_mut(ii)
                    .ok_or_else(|| null("side index"))? = Some(longer.clone());
                side_curve = longer;
                solutions = sharp.global_curve_intersect(
                    k,
                    &ivl,
                    &side_curve,
                    &side_curve.natural_interval(),
                    tol,
                    &none,
                )?;
                if solutions.is_empty() {
                    return Err(sm_err());
                }
            }
            // One crossing (a closed curve starting there meets it at both its ends).
            let first = solutions.get(0).ok_or_else(sm_err)?;
            let (first_t, first_side_t) = (first.start.parameters[0], first.start.parameters[1]);
            let at = sharp.evaluate_point(k, first_t)?;
            for kk in 1..solutions.len() {
                let s = solutions.get(kk).ok_or_else(sm_err)?;
                let other = sharp.evaluate_point(k, s.start.parameters[0])?;
                if other.distance(at) > tol {
                    return Err(sm_err());
                }
            }
            if have_vertex {
                // Both tangent faces have a seam there: they must cross the sharp edge
                // together.
                if at.distance(self.new_vertex) > tol {
                    return Err(sm_err());
                }
            } else {
                self.new_vertex = at;
                start_at = first_t;
                have_vertex = true;
            }
            *self
                .side_params
                .get_mut(ii)
                .ok_or_else(|| null("side index"))? = first_side_t;
        }
        if !have_vertex {
            self.new_vertex = sharp.evaluate_point(k, ivl.min())?;
        }

        // Start the closed curve at the new vertex: the piece after it, then the piece
        // before it.
        let snap = EFF_ZERO * (1.0 + ivl.max_dimension());
        if start_at > ivl.min() + snap && start_at < ivl.max() - snap {
            let mut before = sharp.clone();
            if bspline_part(&before).is_none() {
                return Err(SolidError::null_pointer(OP));
            }
            let mut after_ivl = crate::geom::extent1d::Extent1d::from_min_max(start_at, ivl.max());
            let mut before_ivl = crate::geom::extent1d::Extent1d::from_min_max(ivl.min(), start_at);
            sharp.trim(k, &mut after_ivl, TrimMode::Notify)?;
            before.trim(k, &mut before_ivl, TrimMode::Notify)?;
            crate::blend::fillet_corner::p3::join_with_gap(
                k,
                &mut sharp,
                true,
                &mut before,
                false,
                Some(tol),
            )?;
        }
        self.new_edge_curve = Some(sharp);
        Ok(())
    }

    /// `FindSideEdgeExtension(lWhichSide)`: a tangent face's seam at a rail's vertex runs
    /// on to the new vertex, on its own curve (found by `IntersectTangentSurfaces`) and
    /// on its UV trims, each continued along its own line. A UV trim that does not reach
    /// a point of the face's surface at the new vertex fails the removal.
    fn find_side_edge_extension(&mut self, k: &Kernel, b: &Brep, which_side: usize) -> Result<()> {
        let Some(side) = self.side_edge(which_side) else {
            return Ok(());
        };
        let tol = self.tol_3d;
        let surf = ner(crate::topology::face::p2::surface(
            b,
            self.tan_face(which_side)?,
        )?)?;
        let new_t = *self
            .side_params
            .get(which_side)
            .ok_or_else(|| null("side index"))?;

        let uses = b.edge(side)?.edgeuses.clone();
        let mut pair = 0usize;
        for eu in uses {
            let (uv, owner) = eu1::uv_trim_curve(b, eu)?;
            let Some(uv) = uv else {
                continue;
            };
            if owner != Some(eu) {
                continue;
            }
            if pair > 1 {
                return Err(sm_err());
            }
            let extended = if uv.natural_interval().contains_value(new_t, 0.0) {
                if bspline_part(uv).is_none() {
                    return Err(SolidError::null_pointer(OP));
                }
                uv.clone()
            } else {
                let part = ner(bspline_part(uv))?;
                Curve::from(ner(part.create_extended_curve(
                    new_t,
                    ContinuityType::CInfinity,
                    true,
                )?)?)
            };

            // It must reach the face's surface at the new vertex.
            let uv_at = extended.evaluate_point(k, new_t)?;
            let slot = self
                .side_uvs
                .get_mut(which_side)
                .and_then(|s| s.get_mut(pair))
                .ok_or_else(|| null("side index"))?;
            *slot = Some(extended);
            *self
                .side_uv_owners
                .get_mut(which_side)
                .and_then(|s| s.get_mut(pair))
                .ok_or_else(|| null("side index"))? = Some(eu);
            pair += 1;

            let at = surf.evaluate_point(k, Point2::new(uv_at.x, uv_at.y))?;
            if at.distance(self.new_vertex) > tol {
                return Err(sm_err());
            }
        }
        if pair == 0 {
            return Err(sm_err());
        }
        Ok(())
    }

    /// `DoRemoval`: the fillet goes, its rails become the sharp edge.
    #[allow(clippy::too_many_lines)]
    fn do_removal(&mut self, k: &mut Kernel, b: &mut Brep) -> Result<()> {
        let every_region = k.fixes.glue_vertices_joins_every_region;
        let new_edge_curve = ner(self.new_edge_curve.take())?;
        with_kernel_callbacks(k, b, |_k, b, env| {
            let this = Some(NotifyArg::Object(brep_p2::brep_ref(b)));
            brep_p2::notify(b, env, None, NotifyOperation::PreEdit, [this, None, None])
        })?;

        // The fillet goes; its seam (a wire edge then) with it.
        let face_to_delete = ner(self.face_to_delete.take())?;
        with_kernel_callbacks(k, b, |k, b, env| {
            brep_p3::delete_face(
                k,
                b,
                env,
                &mut Marks::default(),
                face_to_delete,
                DeleteFaceOptions {
                    delete_edges_vertices: true,
                    do_region_nesting: false,
                    region_to_keep: None,
                },
            )
        })?;
        self.seam = None;

        // The tangent faces' seams run on to the new vertex.
        for ii in 0..2 {
            let Some(side) = self.side_edge(ii) else {
                continue;
            };
            let side_param = *self.side_params.get(ii).ok_or_else(|| null("side index"))?;
            let mut side_ivl = edge::interval(b, side)?;
            // (statuses ignored)
            if self.side_at_high(ii)? {
                ignore_status(side_ivl.set_min_max(side_ivl.min(), side_param))?;
            } else {
                ignore_status(side_ivl.set_min_max(side_param, side_ivl.max()))?;
            }
            if let Some(curve) = self.side_curves.get_mut(ii).and_then(Option::take) {
                // Its line continued (the old part keeps its parameters); its UV trims go
                // with the old curve and are replaced below.
                install_curve(k, b, side, Some(curve))?;
            }
            edge::set_interval(b, side, &side_ivl)?;
            for jj in 0..2 {
                let owner = self
                    .side_uv_owners
                    .get(ii)
                    .and_then(|s| s.get(jj))
                    .copied()
                    .flatten();
                let Some(owner) = owner else {
                    continue;
                };
                let uv = self
                    .side_uvs
                    .get_mut(ii)
                    .and_then(|s| s.get_mut(jj))
                    .and_then(Option::take);
                let arg = match uv {
                    Some(c) => UvTrimCurveArg::New(Box::new(c)),
                    None => UvTrimCurveArg::Null,
                };
                // (status ignored)
                ignore_status(eu1::set_uv_trim_curve(b, owner, arg, 0.0, true, None))?;
            }
        }

        // Both vertices to the new one, both rails onto the sharp edge's curve.
        let (v0, v1) = (self.vertex(0)?, self.vertex(1)?);
        // (statuses ignored)
        ignore_status(set_vertex_point(k, b, v0, self.new_vertex))?;
        ignore_status(set_vertex_point(k, b, v1, self.new_vertex))?;

        let survive_edge = self.rail(0)?;
        let delete_edge = self.rail(1)?;
        let tangent = |b: &Brep, e: EdgeId| -> Result<crate::math::Vec3> {
            let c = ner(edge::curve(b, e)?)?;
            // (SmCurve's default: non-zero tangents)
            let pv = c.evaluate(k, edge::interval(b, e)?.evaluate(0.5), 1, true, true)?;
            pv.get(1).copied().ok_or_else(|| null("short evaluation"))
        };
        let pv0 = tangent(b, survive_edge)?;
        let pv1 = tangent(b, delete_edge)?;
        let orient = if pv0.dot(pv1) > 0.0 {
            OrientType::Same
        } else {
            OrientType::Opposite
        };

        let new_ivl = new_edge_curve.natural_interval();
        // deletes the rail's curve and UV trims
        install_curve(k, b, survive_edge, Some(new_edge_curve))?;
        edge::set_interval(b, survive_edge, &new_ivl)?;
        install_curve(k, b, delete_edge, None)?;
        edge::set_interval(b, delete_edge, &new_ivl)?;

        // The vertex with a seam on it survives (a seam's end is that vertex).
        let (survive_vtx, delete_vtx) =
            if self.side_edges[0].is_some() || self.side_edges[1].is_none() {
                (v0, v1)
            } else {
                (v1, v0)
            };
        crate::topology::brep::p5::glue_vertices(k, b, Some(survive_vtx), Some(delete_vtx), None)?;
        self.vertices = [Some(survive_vtx); 2];

        with_kernel_callbacks(k, b, |k, b, env| {
            glue_edges(k, b, env, survive_edge, orient, 0.0, delete_edge)
        })?;
        self.rails[1] = Some(survive_edge);

        if !every_region {
            // Deleting the fillet split each shell it alone held together (the boss's top
            // from the rest) into one for each part; joined again along the sharp edge
            // they are one shell.
            join_shells_at_vertex(k, b, Some(survive_vtx))?;
        }

        // UV trims of the sharp edge on both tangent faces, and their domains to take
        // them in.
        let uses = b.edge(survive_edge)?.edgeuses.clone();
        for &eu in &uses {
            get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
            if eu1::uv_trim_curve(b, eu)?.0.is_none() {
                return Err(SolidError::null_pointer(OP));
            }
        }
        for ii in 0..2 {
            let face = self.tan_face(ii)?;
            let eu = ner(edge::edgeuse_of_face(b, survive_edge, Some(face))?)?;
            let uv = ner(eu1::uv_trim_curve(b, eu)?.0)?;
            let mut face_domain: Extent2d = crate::topology::face::p2::uv_domain(b, face)?;
            let uv_ivl = uv.natural_interval();
            for kk in 0..=16u32 {
                let p = uv.evaluate_point(k, uv_ivl.evaluate(f64::from(kk) / 16.0))?;
                face_domain.add_point(Point2::new(p.x, p.y));
            }
            if every_region {
                crate::topology::face::p2::set_uv_domain(b, face, &face_domain)?;
            } else {
                // The face lies within its surface's domain (nothing was continued).
                let surf_domain = ner(crate::topology::face::p2::surface(b, face)?)?
                    .natural_uv_domain_with_nurb(k)?;
                let in_surface = face_domain.intersect(&surf_domain)?;
                crate::topology::face::p2::set_uv_domain(b, face, &in_surface)?;
            }
        }
        Ok(())
    }
}

/// `pEdge->SetCurve(pCurve, TRUE)` (deleting the old curve and the edge uses' UV trim
/// curves; status ignored), then `pCurve->SetOwner(pEdge)` (the owner set first here,
/// to the edge's [`Brep::owner_identity`]).
fn install_curve(k: &mut Kernel, b: &mut Brep, e: EdgeId, curve: Option<Curve>) -> Result<()> {
    with_kernel_callbacks(k, b, |_k, b, env| {
        let mut curve = curve;
        if let Some(c) = curve.as_mut() {
            // the identity the B-rep names the edge's geometry owner by (what the check
            // compares), not the edit's notification identity
            let owner = b.owner_identity(EntityRef::Edge(e));
            let _ = c.set_owner(Some(owner), Some(brep_p2::brep_ref(b)));
        }
        let options = SetCurveOptions {
            delete_old_curve: true,
            ..SetCurveOptions::default()
        };
        ignore_status(edge::set_curve(b, e, curve, options, env, [None, None]))
    })
}

/// Not upstream (`SmBrep::JoinShellsAtVertex`, the fork's 52bfef0, removed again by
/// 661f79c): the shells of each region that `v` is in become one (their faces were
/// joined at it: `GlueVertices` joins one region's pair only).
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without a vertex or for a shell without region; `SM_ERR` when
/// editing is disabled; those of the shell edits.
pub fn join_shells_at_vertex(k: &mut Kernel, b: &mut Brep, v: Option<VertexId>) -> Result<()> {
    let v = ner(v)?;
    if !b.editing_enabled() {
        return Err(sm_err());
    }

    let mut shells: Vec<Option<crate::topology::ShellId>> =
        vertex::shells(b, v)?.into_iter().map(Some).collect();
    for ii in 0..shells.len() {
        for jj in ii + 1..shells.len() {
            let (Some(keep), Some(join)) = (
                shells.get(ii).copied().flatten(),
                shells.get(jj).copied().flatten(),
            ) else {
                continue;
            };
            if keep == join || b.shell(keep)?.region != b.shell(join)?.region {
                continue;
            }
            let wires: Vec<Option<EdgeId>> = shell::wire_edges(k, b, join)?
                .into_iter()
                .map(Some)
                .collect();
            shell::transfer_children(k, b, join, keep, Some(&wires))?;
            let region = ner(b.shell(join)?.region)?;
            shell::region_remove_shell(b, region, join)?;
            with_kernel_callbacks(k, b, |k, b, env| shell::destroy(k, b, join, env))?;
            if let Some(slot) = shells.get_mut(jj) {
                *slot = None;
            }
        }
    }
    Ok(())
}
