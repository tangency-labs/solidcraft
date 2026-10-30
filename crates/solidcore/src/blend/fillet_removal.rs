//! Fillet removal: deleting a four-sided fillet face and closing the hole with the
//! sharp edge where its tangent faces meet.
//!
//! Upstream: `source/SMLib/src/SmFilletRemoval.cpp`, `source/SMLib/inc/SmFilletRemoval.h`;
//! co-ported `SmEdge::SetTrimCurve` (`SmEdge.cpp`; `SmEdge::Replace3DCurve` in `topology::edge`),
//! `SmBrep::GlueEdges` (`SmBrep.cpp`) and the `UpdateAnalyticalDomain` dispatch of the
//! `SmBSplineSurface` kinds (`SmSurface.h`, `SmPlane.h`).
//!
//! | Rust                                         | Upstream                                         |
//! |----------------------------------------------|--------------------------------------------------|
//! | [`delete_fillet_face`]                       | `SmBrep::DeleteFilletFace`                       |
//! | [`FilletRemoval`]                            | `SmFilletRemoval`                                |
//! | [`FilletRemoval::new`]                       | `SmFilletRemoval::SmFilletRemoval`               |
//! | [`FilletRemoval::remove_face`]               | `SmFilletRemoval::RemoveFace`                    |
//! | `collect_fillet_topology` (private)          | `SmFilletRemoval::CollectFilletTopology`         |
//! | `calc_new_corner_vertices` (private)         | `SmFilletRemoval::CalcNewCornerVertices`         |
//! | `find_side_edge_extensions` (private)        | `SmFilletRemoval::FindSideEdgeExtensions`        |
//! | `intersect_tangent_surfaces` (private)       | `SmFilletRemoval::IntersectTangentSurfaces`      |
//! | `do_removal` (private)                       | `SmFilletRemoval::DoRemoval`                     |
//! | [`FilletCornerInfo`]                         | `SmFilletCornerInfo`                             |
//! | [`move_vtx_and_edge_to_adjacent_face`]       | `SmFaceRemoval::MoveVtxAndEdgeToAdjacentFace`    |
//! | [`set_trim_curve`]                           | `SmEdge::SetTrimCurve`                           |
//! | `topology::edge::replace_3d_curve` (shared)  | `SmEdge::Replace3DCurve`                         |
//! | [`glue_edges`]                               | `SmBrep::GlueEdges`                              |
//! | [`update_analytical_domain`]                 | `SmSurface::UpdateAnalyticalDomain` (virtual)    |
//! | [`FilletRemoval::set_exact_only`], [`FilletRemoval::not_exact`], [`FilletRemoval::sharp_edge`] | the fork's `SetExactOnly`, `NotExact`, `GetSharpEdge` (WR-606, not upstream) |
//!
//! Deviations:
//! - **Object model.** `SmFaceRemoval` (an abstract base with one derived class) is not
//!   a separate type: its member `MoveVtxAndEdgeToAdjacentFace` is the free function
//!   [`move_vtx_and_edge_to_adjacent_face`]. The faces, edges and vertices are ids of
//!   the B-rep the removal edits, which every method takes; the curves the removal
//!   computes are owned values moved into the B-rep where upstream hands it pointers.
//! - **Out-of-bounds evaluation.** Upstream turns on `m_bOutOfBoundsEnabled` of the
//!   B-rep's own surfaces for its intersections and restores it; the port intersects
//!   evaluation copies of the surfaces with the flag on, leaving the B-rep's surfaces
//!   alone.
//! - **Ring fillets (not upstream).** With `Kernel::fixes.fillet_removal_takes_ring_fillets`
//!   (WR-605, the SolidCraft fork's tangency-labs/smlib 52bfef0) [`delete_fillet_face`]
//!   hands a ring fillet to [`ring::RingFilletRemoval`].
//! - **Notifications.** Each B-rep edit notifies through the kernel's callbacks
//!   (`with_kernel_callbacks`), one edit at a time, as upstream's objects notify their
//!   context.
//! - **Cached topology.** After `MoveVtxAndEdgeToAdjacentFace` relinks edge uses and
//!   vertex uses directly, and before [`delete_fillet_face`] returns, the port's cached
//!   topology fields are refreshed (`Brep::refresh_cached_topology`; upstream has no such
//!   fields).
//! - **Null pointers.** Where upstream dereferences `NULL` (an edge use missing where it
//!   sets a UV trim curve, a vertex without its vertex uses), the port returns
//!   [`SolidError::InvalidInput`].
//! - **Curve owners.** A curve the removal installs is owned under its edge's
//!   [`Brep::owner_identity`], the identity the B-rep check compares (`SetOwner(pEdge)`),
//!   and `ValidateGeometry` checks it under the same identities.
//! - **Exact only (not upstream).** WR-606, the SolidCraft fork's tangency-labs/smlib
//!   5b0534b: [`FilletRemoval::set_exact_only`] (off by default; [`delete_fillet_face`]
//!   never sets it) makes the removal refuse what SMLib would only approximate. A side
//!   edge that stops short of a new corner runs on along its own line (its end span
//!   continued) instead of a fitted intersection of its faces. A curved side edge, a
//!   corner of more than three edges, or a face other than a plane whose new corner lies
//!   outside its surface's natural domain fails the removal before the B-rep changes.
//!   The fork installs the run-on curve with `SetCurve` and then sets its owner; the
//!   port sets the owner first, as for the new edge.
//! - No recursion.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::NotifyOperation;
use crate::geom::tol;
use crate::geom::vector2d::{Point2, Vec2};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::plane::Plane;
use crate::surface::surface::p3::set_out_of_bounds_enabled;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p2::{self as brep_p2, NotifyArg};
use crate::topology::brep::p3::{self as brep_p3, with_kernel_callbacks, DeleteFaceOptions};
use crate::topology::brep::p4::{self as brep_p4, DeleteEnv};
use crate::topology::edge::{self, SetCurveOptions};
use crate::topology::edgeuse::p1::{self as eu1, UvTrimCurveArg};
use crate::topology::edgeuse::p3::get_or_create_uv_trim_curve;
use crate::topology::topology_traverser::Marks;
use crate::topology::{vertex, vertexuse, Brep, EdgeId, EdgeuseId, FaceId, LoopuseStart, VertexId};

pub mod ring;

const OP: &str = "blend::fillet_removal";

fn sm_err() -> SolidError {
    SolidError::sm_err(OP)
}

fn null(what: &'static str) -> SolidError {
    SolidError::invalid_input(OP, what)
}

/// `NER(p)`: a `NULL` pointer is `SM_ERR_NULL_POINTER`.
fn ner<T>(v: Option<T>) -> Result<T> {
    v.ok_or_else(|| SolidError::null_pointer(OP))
}

/// A status upstream turns into `SER(SM_ERR)` whatever it was: any failure but an
/// unported callee becomes `SM_ERR`.
fn or_sm_err<T>(r: Result<T>) -> Result<T> {
    r.map_err(|e| if e.is_unported() { e } else { sm_err() })
}

/// One corner of the fillet face (`SmFilletCornerInfo`): its vertex, the vertex's UV
/// positions in the fillet, end and tangent faces, the tangent face's UV position of the
/// new corner, and the side edge (the third edge at the vertex) with its extension.
///
/// The side edge's curve extension: when [`FilletCornerInfo::new_side_edge_extension_3d`]
/// is `None`, [`FilletCornerInfo::new_side_edge_param`] is the new corner's parameter on
/// the side edge's curve, which already reaches it.
#[derive(Debug, Clone, Default)]
pub struct FilletCornerInfo {
    /// `m_pVertex`.
    pub vertex: Option<VertexId>,
    /// `m_sUVFilSurf`.
    pub uv_fil_surf: Point2,
    /// `m_sUVTanSurfOld`.
    pub uv_tan_surf_old: Point2,
    /// `m_sUVEndSurfOld`.
    pub uv_end_surf_old: Point2,
    /// `m_sUVTanSurfNew`.
    pub uv_tan_surf_new: Point2,
    /// `m_pNewSideEdgeExtension3d`.
    pub new_side_edge_extension_3d: Option<Curve>,
    /// `m_pNewSideEdgeExtEndFaceUV`.
    pub new_side_edge_ext_end_face_uv: Option<Curve>,
    /// `m_pNewSideEdgeExtTanFaceUV`.
    pub new_side_edge_ext_tan_face_uv: Option<Curve>,
    /// Not upstream (WR-606, exact only; SolidCraft change, tangency-labs/smlib 5b0534b):
    /// the side edge's line run on to the new corner (`m_pNewSideCurve`), until
    /// `DoRemoval` installs it.
    pub new_side_curve: Option<Curve>,
    /// WR-606 (`m_bSideCurveReplaced`): `DoRemoval` put [`Self::new_side_curve`] on the
    /// side edge.
    pub side_curve_replaced: bool,
    /// WR-606 (`m_bRebuildSideUVs`, exact only): `DoRemoval` makes the side edge's UV
    /// trim curves anew.
    pub rebuild_side_uvs: bool,
    /// `m_pSideEdge`.
    pub side_edge: Option<EdgeId>,
    /// `m_bHighEndOfSideEdge`.
    pub high_end_of_side_edge: bool,
    /// `m_dNewSideEdgeParam`: the parameter of the srf/srf/srf intersection point on the
    /// side edge's curve.
    pub new_side_edge_param: f64,
}

impl FilletCornerInfo {
    /// `NoSideEdge`: more than three edges meet at the corner vertex.
    #[must_use]
    pub const fn no_side_edge(&self) -> bool {
        self.side_edge.is_none()
    }
}

/// The removal of one fillet face (`SmFilletRemoval`): the fillet face must have four
/// edges, two opposite ones tangent (the sides) and the other two not (the ends); the
/// surfaces of its four neighbours must be `SmBSplineSurface`s. All intersections are
/// made before the B-rep changes, except that a side edge's UV trim curve that does not
/// reach its new corner is extended (or cleared) on the way, as upstream does.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // upstream's independent SmBoolean members
pub struct FilletRemoval {
    /// `m_pFaceToDelete`.
    face_to_delete: FaceId,
    /// `m_d3dTol`.
    tol_3d: f64,
    /// `m_aTanEdges`.
    tan_edges: [Option<EdgeId>; 2],
    /// `m_aTanFaces`.
    tan_faces: [Option<FaceId>; 2],
    /// `m_aEndEdges`.
    end_edges: [Option<EdgeId>; 2],
    /// `m_aEndFaces`.
    end_faces: [Option<FaceId>; 2],
    /// `m_eRailParam`: u or v runs along the fillet.
    rail_param: SurfParamType,
    /// `m_bEndsSwapped`.
    ends_swapped: bool,
    /// `m_bSidesSwapped`.
    sides_swapped: bool,
    /// `m_aNewVtxPts`.
    new_vtx_pts: [Point3; 2],
    /// `m_aNewVtxTols`.
    new_vtx_tols: [f64; 2],
    /// `m_aUVEndSurfNew`.
    uv_end_surf_new: [Point2; 2],
    /// `m_dNewEdgeDeviation`.
    new_edge_deviation: f64,
    /// `m_pNewEdgeCurve3d`.
    new_edge_curve_3d: Option<Curve>,
    /// `m_aNewEdgeCurveUVs`.
    new_edge_curve_uvs: [Option<Curve>; 2],
    /// `m_aCornerInfo[which end][which side]`.
    corners: [[FilletCornerInfo; 2]; 2],
    /// Not upstream (WR-606, `m_bExactOnly`): see [`FilletRemoval::set_exact_only`].
    exact_only: bool,
    /// Not upstream (WR-606, `m_bNotExact`): see [`FilletRemoval::not_exact`].
    not_exact: bool,
}

impl FilletRemoval {
    /// `SmFilletRemoval(pFaceToDelete, d3dTolerance)`.
    #[must_use]
    pub fn new(face_to_delete: FaceId, tol_3d: f64) -> Self {
        Self {
            face_to_delete,
            tol_3d,
            tan_edges: [None; 2],
            tan_faces: [None; 2],
            end_edges: [None; 2],
            end_faces: [None; 2],
            rail_param: SurfParamType::U,
            ends_swapped: false,
            sides_swapped: false,
            new_vtx_pts: [Point3::UNDEF; 2],
            new_vtx_tols: [0.0; 2],
            uv_end_surf_new: [Point2::UNDEF; 2],
            new_edge_deviation: -1.0,
            new_edge_curve_3d: None,
            new_edge_curve_uvs: [None, None],
            corners: Default::default(),
            exact_only: false,
            not_exact: false,
        }
    }

    /// Not upstream (WR-606; SolidCraft change, tangency-labs/smlib 5b0534b,
    /// `SetExactOnly`): remove only what SMLib makes exactly. A side edge runs on to
    /// the new corner only along its line (not on a fitted intersection), a face grows
    /// only within its surface's domain or on a plane, and every corner has a side
    /// edge. Otherwise [`FilletRemoval::remove_face`] fails before changing the B-rep,
    /// [`FilletRemoval::not_exact`] true where a continuation was the reason. Off by
    /// default (upstream's removal); [`delete_fillet_face`] never sets it.
    pub fn set_exact_only(&mut self, exact_only: bool) {
        self.exact_only = exact_only;
    }

    /// Not upstream (WR-606, `NotExact`): an exact-only removal failed because a
    /// continuation would not be exact.
    #[must_use]
    pub const fn not_exact(&self) -> bool {
        self.not_exact
    }

    /// Not upstream (WR-606, `GetSharpEdge`): the edge the fillet became (after a
    /// successful [`FilletRemoval::remove_face`]).
    #[must_use]
    pub const fn sharp_edge(&self) -> Option<EdgeId> {
        self.tan_edges[0]
    }

    /// `RemoveFace`: collects the fillet's topology, intersects the new corners, the side
    /// edges' extensions and the tangent surfaces, then edits the B-rep.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a face that is not a four-sided fillet between B-spline surfaces or
    /// when an intersection fails (the B-rep is unchanged then, but for side-edge UV trim
    /// curves extended or cleared on the way); those of the edits.
    pub fn remove_face(&mut self, k: &mut Kernel, b: &mut Brep) -> Result<()> {
        // Collect the two tangent edges, two end edges,
        // and the Faces across those edges.
        self.collect_fillet_topology(k, b)?;

        // For both ends:
        for which_end in 0..2 {
            self.calc_new_corner_vertices(k, b, which_end)?;
            // For both sides at this end:
            for which_side in 0..2 {
                self.find_side_edge_extensions(k, b, which_end, which_side)?;
            }
        }

        // Intersect the surfaces of the tangent Faces (which have been extended).
        self.intersect_tangent_surfaces(k, b)?;

        // Not upstream (WR-606, exact only): a face grows over the fillet's place within
        // its surface's domain, or on a plane (a plane continued is the same plane); any
        // other surface would be continued past its domain, which SMLib does not do
        // exactly.
        if self.exact_only {
            for ii in 0..2 {
                let uv_end_new = *self
                    .uv_end_surf_new
                    .get(ii)
                    .ok_or_else(|| null("end index"))?;
                let faces = [self.end_face(ii)?, self.tan_face(ii)?];
                let uvs = [
                    [uv_end_new, uv_end_new],
                    [
                        self.corner(0, ii)?.uv_tan_surf_new,
                        self.corner(1, ii)?.uv_tan_surf_new,
                    ],
                ];
                for (face, face_uvs) in faces.into_iter().zip(uvs) {
                    let srf = ner(crate::topology::face::p2::surface(b, face)?)?;
                    let domain = srf.natural_uv_domain_with_nurb(k)?;
                    let slack = EFF_ZERO_SQRT * (1.0 + domain.max_dimension());
                    let mut inside = true;
                    for uv in face_uvs {
                        if uv.x < domain.min().x - slack
                            || uv.x > domain.max().x + slack
                            || uv.y < domain.min().y - slack
                            || uv.y > domain.max().y + slack
                        {
                            inside = false;
                        }
                    }
                    if !inside && !srf.is_kind_of(Plane::TYPE)? {
                        self.not_exact = true;
                        return Err(sm_err());
                    }
                }
            }
        }

        // Ok, we've collected all of the info without incident.
        // Start digging into the Brep.
        self.do_removal(k, b)
    }

    fn corner(&self, which_end: usize, which_side: usize) -> Result<&FilletCornerInfo> {
        self.corners
            .get(which_end)
            .and_then(|c| c.get(which_side))
            .ok_or_else(|| null("corner index"))
    }

    fn corner_mut(&mut self, which_end: usize, which_side: usize) -> Result<&mut FilletCornerInfo> {
        self.corners
            .get_mut(which_end)
            .and_then(|c| c.get_mut(which_side))
            .ok_or_else(|| null("corner index"))
    }

    fn tan_edge(&self, i: usize) -> Result<EdgeId> {
        ner(self.tan_edges.get(i).copied().flatten())
    }

    fn end_edge(&self, i: usize) -> Result<EdgeId> {
        ner(self.end_edges.get(i).copied().flatten())
    }

    fn tan_face(&self, i: usize) -> Result<FaceId> {
        ner(self.tan_faces.get(i).copied().flatten())
    }

    fn end_face(&self, i: usize) -> Result<FaceId> {
        ner(self.end_faces.get(i).copied().flatten())
    }

    /// `CollectFilletTopology`: the two tangent and two end edges, the faces across
    /// them, the four corner vertices with their UV positions in their three faces and
    /// their side edges, and the fillet surface's orientation to the ends and sides.
    fn collect_fillet_topology(&mut self, k: &Kernel, b: &mut Brep) -> Result<()> {
        // Identify two tangent Edges and two end Edges.

        // Assumption: GetEdges() returns Edges ordered around the Loop.
        let edges = crate::topology::face::p4::edges(b, self.face_to_delete, None, false, None)?;
        let [e0, e1, e2, e3] = edges.as_slice() else {
            return Err(sm_err());
        };
        let (e0, e1, e2, e3) = (*e0, *e1, *e2, *e3);

        let tangency_tol_deg = 2.0; // [for fillet1018.smb, Face[5]; was 0.5]
        let tangent =
            |b: &mut Brep, e: EdgeId| edge::is_tangent_edge(k, b, e, tangency_tol_deg, false);

        if tangent(b, e0)? {
            if tangent(b, e1)? {
                return Err(sm_err());
            }
            if !tangent(b, e2)? {
                return Err(sm_err());
            }
            if tangent(b, e3)? {
                return Err(sm_err());
            }

            self.tan_edges = [Some(e0), Some(e2)];
            self.end_edges = [Some(e1), Some(e3)];
        } else if tangent(b, e1)? {
            if tangent(b, e0)? {
                return Err(sm_err());
            }
            if tangent(b, e2)? {
                return Err(sm_err());
            }
            if !tangent(b, e3)? {
                return Err(sm_err());
            }

            self.tan_edges = [Some(e1), Some(e3)];
            self.end_edges = [Some(e0), Some(e2)];
        } else {
            return Err(sm_err());
        }

        // Get the four corner vertices.

        // First collect both verts from all four Edges: eight total,
        // of which there will be four matching pairs.
        let mut end_verts = [[None; 2]; 2];
        let mut tan_verts = [[None; 2]; 2];
        for (i, (tan, end)) in tan_verts.iter_mut().zip(end_verts.iter_mut()).enumerate() {
            let verts = edge::vertices(b, self.tan_edge(i)?, None)?;
            let [v0, v1] = verts.as_slice() else {
                return Err(sm_err());
            };
            *tan = [*v0, *v1];

            let verts = edge::vertices(b, self.end_edge(i)?, None)?;
            let [v0, v1] = verts.as_slice() else {
                return Err(sm_err());
            };
            *end = [*v0, *v1];
        }

        // Now sort these out.
        for (which_end, &[ev0, ev1]) in end_verts.iter().enumerate() {
            for (which_side, &[tv0, tv1]) in tan_verts.iter().enumerate() {
                let found = if ev0 == tv0 || ev0 == tv1 {
                    ev0
                } else if ev1 == tv0 || ev1 == tv1 {
                    ev1
                } else {
                    return Err(sm_err());
                };
                self.corner_mut(which_end, which_side)?.vertex = found;
            }
        } // end double loop on two ends / two sides.

        // Find the adjacent Faces.
        // Also, all Surfaces have to be SmBSplineSurfaces.
        // because of CreateExtendedSurface().
        let fil_face = self.face_to_delete;
        let is_b_spline = |b: &Brep, f: FaceId| -> Result<bool> {
            match crate::topology::face::p2::surface(b, f)? {
                Some(s) => s.is_kind_of(BSplineSurface::TYPE),
                None => Ok(false),
            }
        };
        let other_face = |faces: &[FaceId]| -> Result<FaceId> {
            let [f0, f1] = faces else {
                return Err(sm_err());
            };
            if *f0 == fil_face {
                Ok(*f1)
            } else if *f1 == fil_face {
                Ok(*f0)
            } else {
                Err(sm_err())
            }
        };
        for which_end in 0..2 {
            let faces = edge::faces(b, self.tan_edge(which_end)?, None)?;
            let tan_face = other_face(&faces)?;
            if let Some(slot) = self.tan_faces.get_mut(which_end) {
                *slot = Some(tan_face);
            }
            if !is_b_spline(b, tan_face)? {
                return Err(sm_err());
            }

            let faces = edge::faces(b, self.end_edge(which_end)?, None)?;
            let end_face = other_face(&faces)?;
            if let Some(slot) = self.end_faces.get_mut(which_end) {
                *slot = Some(end_face);
            }
            if !is_b_spline(b, end_face)? {
                return Err(sm_err());
            }
        } // end for loop on two ends of fillet

        // Fill in surface uv info for the vertices.
        // There are four Vertices, each is in three Faces.
        // Also find the side edges: the third edge coming into each corner vertex.
        for which_end in 0..2 {
            for which_side in 0..2 {
                let vtx = ner(self.corner(which_end, which_side)?.vertex)?;
                let end_face = self.end_face(which_end)?;
                let tan_face = self.tan_face(which_side)?;

                // Fillet Face:
                let vu = ner(vertex::vertexuse_of_face(b, vtx, Some(fil_face))?)?;
                let uv_fil = vertexuse::compute_uv_point(k, b, vu, true)?;

                // End Face:
                let vu = ner(vertex::vertexuse_of_face(b, vtx, Some(end_face))?)?;
                let uv_end = vertexuse::compute_uv_point(k, b, vu, true)?;

                // Tangent Face:
                let vu = ner(vertex::vertexuse_of_face(b, vtx, Some(tan_face))?)?;
                let uv_tan = vertexuse::compute_uv_point(k, b, vu, true)?;

                // Also find the side edges.
                let vertex_edges = vertex::edges(b, vtx, None)?;
                let mut side_edge = None;
                let mut high_end = false;
                if vertex_edges.len() == 3 {
                    let end_edge = self.end_edge(which_end)?;
                    let tan_edge = self.tan_edge(which_side)?;
                    for &this_edge in &vertex_edges {
                        if this_edge == end_edge || this_edge == tan_edge {
                            continue;
                        }
                        side_edge = Some(this_edge);
                        break;
                    }
                    let side = ner(side_edge)?;

                    // We will also want to know which end of the side edge we're at.
                    let side_vtx = edge::start_vertex(b, side)?;
                    if side_vtx == Some(vtx) {
                        high_end = false;
                    } else {
                        let side_vtx = edge::other_vertex(b, side, side_vtx)?;
                        if side_vtx == Some(vtx) {
                            high_end = true;
                        } else {
                            return Err(sm_err()); // If this happens it's our own fault.
                        }
                    }
                }
                // (more than three edges: we do things differently; see FilletCornerInfo)

                let corner = self.corner_mut(which_end, which_side)?;
                corner.uv_fil_surf = uv_fil;
                corner.uv_end_surf_old = uv_end;
                corner.uv_tan_surf_old = uv_tan;
                corner.side_edge = side_edge;
                corner.high_end_of_side_edge = high_end;
            } // end inner loop
        } // end double loop on four corners, filling in surface uv and side-edge info

        // Now set the orientation of the fillet surface w.r.t. our Ends and Sides:
        // Look at the directions of the vectors between corner uv values.
        let c00 = self.corner(0, 0)?.uv_fil_surf;
        let vec_end_to_end: Vec2 = self.corner(1, 0)?.uv_fil_surf - c00;
        let vec_side_to_side: Vec2 = self.corner(0, 1)?.uv_fil_surf - c00;
        // (Unitize's status is ignored: a zero vector stays as it is)
        let vec_end_to_end = vec_end_to_end.unitize().unwrap_or(vec_end_to_end);
        let vec_side_to_side = vec_side_to_side.unitize().unwrap_or(vec_side_to_side);

        if vec_end_to_end.x.abs() > vec_end_to_end.y.abs() {
            self.rail_param = SurfParamType::U;
            self.ends_swapped = vec_end_to_end.x < 0.0;
            self.sides_swapped = vec_side_to_side.y < 0.0;
        } else {
            self.rail_param = SurfParamType::V;
            self.ends_swapped = vec_end_to_end.y < 0.0;
            self.sides_swapped = vec_side_to_side.x < 0.0;
        }

        Ok(())
    }

    /// `CalcNewCornerVertices(lWhichEnd)`: the srf/srf/srf intersection of the end face
    /// with the two tangent faces at `which_end` (the surfaces evaluated out of bounds,
    /// their domains expanded): the new corner, its tolerance and its UV positions.
    fn calc_new_corner_vertices(&mut self, k: &Kernel, b: &Brep, which_end: usize) -> Result<()> {
        // Find what will be the new ends of the new Edge:
        // on each end, srf/srf/srf intersect the two tangent Faces and the end Face.
        let tan0 = out_of_bounds_copy(b, self.tan_face(0)?)?;
        let tan1 = out_of_bounds_copy(b, self.tan_face(1)?)?;
        let mut tan_dom0 = tan0.natural_uv_domain_with_nurb(k)?;
        let mut tan_dom1 = tan1.natural_uv_domain_with_nurb(k)?;

        let end_surf = out_of_bounds_copy(b, self.end_face(which_end)?)?;
        let mut end_dom = end_surf.natural_uv_domain_with_nurb(k)?;

        // Also expand the domains, for srf/srf/srf intersect.
        tan_dom0.expand_relative(2.0);
        tan_dom1.expand_relative(2.0);
        end_dom.expand_relative(2.0);

        let c0 = self.corner(which_end, 0)?;
        let c1 = self.corner(which_end, 1)?;
        let uv_end: Point2 = (c0.uv_end_surf_old + c1.uv_end_surf_old) / 2.0;
        let uv_tan0 = c0.uv_tan_surf_old;
        let uv_tan1 = c1.uv_tan_surf_old;

        let sol = match end_surf.local_surface_surface_intersect(
            k, &end_dom, &tan0, &tan_dom0, &tan1, &tan_dom1, uv_end, uv_tan0, uv_tan1,
        ) {
            Ok(Some(sol)) => sol,
            Err(e) if e.is_unported() => return Err(e),
            Ok(None) | Err(_) => return Err(sm_err()),
        };

        // Store the new vtx position and the three surface uv's.
        // Surface uv's first.
        let (uv_end, uv_tan0, uv_tan1) = (sol.uv1, sol.uv2, sol.uv3);
        if let Some(slot) = self.uv_end_surf_new.get_mut(which_end) {
            *slot = uv_end;
        }
        self.corner_mut(which_end, 0)?.uv_tan_surf_new = uv_tan0;
        self.corner_mut(which_end, 1)?.uv_tan_surf_new = uv_tan1;

        // Evaluate all three surfaces, check 3d distances,
        // and use the average position.
        // (statuses ignored upstream: a failure leaves the default point)
        let point = |s: &Surface, uv: Point2| -> Result<Point3> {
            Ok(ignored(s.evaluate_point(k, uv))?.unwrap_or(Point3::UNDEF))
        };
        let vtx_pt_end = point(&end_surf, uv_end)?;
        let vtx_pt_tan0 = point(&tan0, uv_tan0)?;
        let vtx_pt_tan1 = point(&tan1, uv_tan1)?;

        let dist00 = vtx_pt_end.distance(vtx_pt_tan0);
        let dist01 = vtx_pt_end.distance(vtx_pt_tan1);
        let dist11 = vtx_pt_tan0.distance(vtx_pt_tan1);
        let tol = self.tol_3d;
        if dist00 > tol || dist01 > tol || dist11 > tol {
            return Err(sm_err());
        }

        let new_vtx_pt = (vtx_pt_end + vtx_pt_tan0 + vtx_pt_tan1) / 3.0;
        if let Some(slot) = self.new_vtx_pts.get_mut(which_end) {
            *slot = new_vtx_pt;
        }
        if let Some(slot) = self.new_vtx_tols.get_mut(which_end) {
            *slot = max3(dist00, dist01, dist11);
        }

        Ok(())
    }

    /// `FindSideEdgeExtensions(lWhichEnd, lWhichSide)`: whether the side edge's curve
    /// reaches the new corner (then its parameter there, the edge's UV trim curves
    /// extended to it, or cleared where the extension misses it), else the intersection
    /// of the end and tangent faces from the old corner to the new one. A corner with
    /// more than three edges takes the intersection.
    fn find_side_edge_extensions(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        which_end: usize,
        which_side: usize,
    ) -> Result<()> {
        let tol = self.tol_3d;
        let mut crv_dist = 2.0 * tol;
        let mut crv_param = 0.0;
        let mut success = false;
        let mut stat_ok = false;

        let new_vtx_pt = *self
            .new_vtx_pts
            .get(which_end)
            .ok_or_else(|| null("end index"))?;
        let corner = self.corner(which_end, which_side)?;
        let side_edge = corner.side_edge;
        let high_end = corner.high_end_of_side_edge;

        if let Some(side) = side_edge {
            let ivl = edge::interval(b, side)?;
            let crv_guess_param = if high_end { ivl.max() } else { ivl.min() };
            let curve = ner(edge::curve(b, side)?)?;
            match curve.drop_point(
                k,
                &curve.natural_interval(),
                new_vtx_pt,
                None,
                tol * 10.0,
                Some(crv_guess_param),
                SolverOperation::Minimize,
            ) {
                Ok(d) => {
                    stat_ok = true;
                    success = d.success;
                    crv_param = d.parameter;
                    crv_dist = d.distance;
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => {}
            }
        }

        // Not upstream (WR-606, exact only): a side edge too short to reach the new
        // corner runs on along its own line (its end span continued: the same line); a
        // curved one would need a fitted intersection.
        if self.exact_only && !(stat_ok && success && crv_dist <= tol) {
            // a corner of more than three edges keeps its end edge, on a fitted intersection
            let Some(side) = side_edge else {
                return Err(sm_err());
            };
            let side_curve = ner(edge::curve(b, side)?)?.clone();
            if bspline_part(&side_curve).is_none() || !side_curve.is_linear(k, tol, false)?.linear {
                self.not_exact = true;
                return Err(sm_err());
            }
            let curve_ivl = side_curve.natural_interval();
            let end = if high_end {
                curve_ivl.max()
            } else {
                curve_ivl.min()
            };
            let pv = side_curve.evaluate(k, end, 1, true, false)?;
            let [p0, d1, ..] = pv.as_slice() else {
                return Err(null("short evaluation"));
            };
            let (p0, d1) = (*p0, *d1);
            let speed = d1.length();
            // (`!(dSpeed > SM_EFF_ZERO)`: a NaN speed fails too)
            if speed.is_nan() || speed <= EFF_ZERO {
                return Err(sm_err());
            }
            let reach = (p0.distance(new_vtx_pt) + 10.0 * tol) / speed;
            let far = if high_end { end + reach } else { end - reach };
            let side_part = ner(bspline_part(&side_curve))?;
            let longer = Curve::from(ner(side_part.create_extended_curve(
                far,
                ContinuityType::CInfinity,
                true,
            )?)?);
            let guess = far;
            let drop = longer.drop_point(
                k,
                &longer.natural_interval(),
                new_vtx_pt,
                None,
                tol * 10.0,
                Some(guess),
                SolverOperation::Minimize,
            );
            self.corner_mut(which_end, which_side)?.new_side_curve = Some(longer);
            match drop {
                Ok(d) if d.success && d.distance <= tol => {
                    stat_ok = true;
                    success = d.success;
                    crv_param = d.parameter;
                    crv_dist = d.distance;
                }
                Err(e) if e.is_unported() => return Err(e),
                // the corner is off the side edge's line
                _ => return Err(sm_err()),
            }
        }

        if stat_ok && success && crv_dist <= tol {
            // Good, the curve is long enough, just update the Edge domain.
            let side = ner(side_edge)?;
            {
                let corner = self.corner_mut(which_end, which_side)?;
                corner.new_side_edge_param = crv_param;
                // Just to make sure:
                corner.new_side_edge_extension_3d = None;
            }

            // Not upstream (WR-606, exact only): nothing changes before DoRemoval(), which
            // makes this side edge's UV trims anew from its curve once its interval
            // reaches the corner.
            if self.exact_only {
                self.corner_mut(which_end, which_side)?.rebuild_side_uvs = true;
                return Ok(());
            }
            let mut side_edge_extent = edge::interval(b, side)?;
            side_edge_extent.add_value(crv_param);
            let side_tol = edge::tolerance(b, side)?;
            let uv_end_new = *self
                .uv_end_surf_new
                .get(which_end)
                .ok_or_else(|| null("end index"))?;
            let uv_tan_new = self.corner(which_end, which_side)?.uv_tan_surf_new;

            // Do End-face uv curve first, then Tan-face uv curve. (Upstream keeps the
            // last UV trim curve read across the two faces when the second has no edge
            // use on its face.)
            let mut uv_curve: Option<Curve> = None;
            for (face, target) in [
                (self.end_face(which_end)?, uv_end_new),
                (self.tan_face(which_side)?, uv_tan_new),
            ] {
                let edgeuse = edge::edgeuse_of_face(b, side, Some(face))?;
                if let Some(eu) = edgeuse {
                    uv_curve = eu1::uv_trim_curve(b, eu)?.0.cloned();
                }
                let Some(curve) = uv_curve.as_ref() else {
                    continue;
                };
                let uv_extent = curve.natural_interval();
                if side_edge_extent.is_contained_by(&uv_extent, EFF_ZERO) {
                    continue;
                }
                let extended = match bspline_part(curve) {
                    Some(bs) => ignored(bs.create_extended_curve(
                        crv_param,
                        ContinuityType::CInfinity,
                        true,
                    ))?
                    .flatten(),
                    None => None,
                };
                // Check whether it's good enough.
                let Some(extended) = extended else {
                    continue;
                };
                let eu = edgeuse.ok_or_else(|| null("side edge use"))?;
                let uv_eval = ignored(extended.evaluate_point(crv_param))?.unwrap_or(Point3::UNDEF);
                if uv_eval.closer_than(side_tol, Point3::new(target.x, target.y, 0.0)) {
                    // Good enough.  Swap out the uv curve for the extended one.
                    eu1::set_uv_trim_curve(
                        b,
                        eu,
                        UvTrimCurveArg::New(Box::new(Curve::from(extended))),
                        0.0,
                        true,
                        None,
                    )?;
                } else {
                    // At this point, just clear out the uv curve,
                    // it will be re-created sooner or later.
                    eu1::set_uv_trim_curve(b, eu, UvTrimCurveArg::Null, 0.0, true, None)?;
                }
            } // end both faces
        } else {
            // We have to intersect the end part and append to the edge curve.
            let corner = self.corner(which_end, which_side)?;
            let uv_end_new = *self
                .uv_end_surf_new
                .get(which_end)
                .ok_or_else(|| null("end index"))?;
            let surface0_points = [corner.uv_end_surf_old, uv_end_new];
            let surface1_points = [corner.uv_tan_surf_old, corner.uv_tan_surf_new];

            // Definitely want to give it a start direction.
            // Just start-to-end should be fine, it won't curve much
            // over the radius of a fillet.
            let vtx = ner(corner.vertex)?;
            let start_dir: Vec3 = new_vtx_pt - b.vertex(vtx)?.point;

            let end_surf = out_of_bounds_copy(b, self.end_face(which_end)?)?;
            let tan_surf = out_of_bounds_copy(b, self.tan_face(which_side)?)?;
            let mut uv_domain_end = end_surf.natural_uv_domain_with_nurb(k)?;
            let mut uv_domain_tan = tan_surf.natural_uv_domain_with_nurb(k)?;

            // Also expand the domains, for srf/srf/srf intersect.
            uv_domain_end.expand_relative(2.0);
            uv_domain_tan.expand_relative(2.0);

            let int = or_sm_err(end_surf.point_based_surface_intersect(
                k,
                &uv_domain_end,
                &tan_surf,
                &uv_domain_tan,
                &surface0_points,
                &surface1_points,
                [false, false],
                [Some(start_dir), None],
                Some(tol),
                None,
            ))?;
            let Some(int_crv_3d) = int.curve_3d else {
                return Err(sm_err());
            };

            // Save the curves in the kit.
            let corner = self.corner_mut(which_end, which_side)?;
            corner.new_side_edge_extension_3d = Some(int_crv_3d);
            corner.new_side_edge_ext_end_face_uv = int.surface1_uv_curve;
            corner.new_side_edge_ext_tan_face_uv = int.surface2_uv_curve;
        } // end else: had to intersect end portion of side edge curve.

        Ok(())
    }

    /// `IntersectTangentSurfaces`: the intersection of the two tangent faces' surfaces
    /// (evaluated out of bounds, their domains expanded) through the new corners, starting
    /// along the fillet surface's rail direction: the new edge's 3D and UV curves.
    fn intersect_tangent_surfaces(&mut self, k: &Kernel, b: &Brep) -> Result<()> {
        // Collect the uv points for point-based surface intersect.
        let surface0_points = [
            self.corner(0, 0)?.uv_tan_surf_new,
            self.corner(1, 0)?.uv_tan_surf_new,
        ];
        let surface1_points = [
            self.corner(0, 1)?.uv_tan_surf_new,
            self.corner(1, 1)?.uv_tan_surf_new,
        ];

        // Get a start direction.
        let uv = self.corner(0, 0)?.uv_fil_surf;
        let fil_surface = ner(crate::topology::face::p2::surface(b, self.face_to_delete)?)?;
        let (_pt, su, sv) = fil_surface.evaluate_1st_derivatives(k, uv, true, true)?;
        let start_dir = if self.rail_param == SurfParamType::U {
            if self.ends_swapped {
                -su
            } else {
                su
            }
        } else if self.ends_swapped {
            -sv
        } else {
            sv
        };

        let tan0 = out_of_bounds_copy(b, self.tan_face(0)?)?;
        let tan1 = out_of_bounds_copy(b, self.tan_face(1)?)?;
        let mut uv_domain1 = tan0.natural_uv_domain_with_nurb(k)?;
        let mut uv_domain2 = tan1.natural_uv_domain_with_nurb(k)?;

        // Also expand the domains, for srf/srf/srf intersect.
        uv_domain1.expand_relative(2.0);
        uv_domain2.expand_relative(2.0);

        let int = or_sm_err(tan0.point_based_surface_intersect(
            k,
            &uv_domain1,
            &tan1,
            &uv_domain2,
            &surface0_points,
            &surface1_points,
            [false, false],
            [Some(start_dir), None],
            Some(self.tol_3d),
            None,
        ))?;
        let Some(int_crv_3d) = int.curve_3d else {
            return Err(sm_err());
        };

        self.new_edge_curve_3d = Some(int_crv_3d);
        self.new_edge_curve_uvs = [int.surface1_uv_curve, int.surface2_uv_curve];
        self.new_edge_deviation = int.deviation.unwrap_or(0.0);

        Ok(())
    }

    /// `DoRemoval`: the topology edit. An end edge at a corner of more than three edges
    /// moves to the tangent face and takes the end/tangent intersection as its geometry;
    /// the fillet face is deleted; the end and tangent faces' domains and surfaces grow
    /// to the new corners; the corner vertices move there and the side edges are
    /// extended to them; the tangent edges take the tangent faces' intersection and are
    /// glued into the new edge, the end edges deleted and the corner vertices glued.
    fn do_removal(&mut self, k: &mut Kernel, b: &mut Brep) -> Result<()> {
        with_kernel_callbacks(k, b, |_k, b, env| {
            let this = Some(NotifyArg::Object(brep_p2::brep_ref(b)));
            brep_p2::notify(b, env, None, NotifyOperation::PreEdit, [this, None, None])
        })?;

        // Before we can delete the Fillet Face, we have to check a complication.
        // If any corner had more than three Edges incident on the vertex,
        // then that End Edge will not be deleted,
        // it must be moved from the Fillet Face to the Tangent Face on the
        // side of the corner in question.
        for which_end in 0..2 {
            let no_side = [
                self.corner(which_end, 0)?.no_side_edge(),
                self.corner(which_end, 1)?.no_side_edge(),
            ];
            if !(no_side[0] || no_side[1]) {
                continue;
            }
            // Here we keep the edge, but it needs to be changed.
            // It was the edge between the Fillet Face and the End Face,
            // and it will now be between the Tangent Face and the End Face.
            let which_side = usize::from(!no_side[0]);
            let end_edge = self.end_edge(which_end)?;
            let corner_vertex = ner(self.corner(which_end, which_side)?.vertex)?;
            // (status ignored upstream)
            ignore_status(move_vtx_and_edge_to_adjacent_face(
                b,
                corner_vertex,
                end_edge,
                self.face_to_delete,
            ))?;
            // (not upstream: the port's cached topology fields follow the relinked uses)
            b.refresh_cached_topology();

            // Now change its geometry.
            let old_ivl = edge::interval(b, end_edge)?;
            let old_tan = curve_tangent(
                k,
                ner(edge::curve(b, end_edge)?)?,
                old_ivl.evaluate(0.5),
                false,
            )?;
            let corner = self.corner_mut(which_end, which_side)?;
            let new_crv = ner(corner.new_side_edge_extension_3d.as_mut())?;
            let new_tan =
                curve_tangent(k, new_crv, new_crv.natural_interval().evaluate(0.5), false)?;
            // Check whether the new geometry goes in the right direction.
            if old_tan.dot(new_tan) < 0.0 {
                let dummy = Extent1d::from_min_max(0.0, 1.0);
                // (statuses ignored upstream)
                ignore_status(new_crv.reverse_parameterization(k, &dummy))?;
                if let Some(c) = corner.new_side_edge_ext_end_face_uv.as_mut() {
                    ignore_status(c.reverse_parameterization(k, &dummy))?;
                }
                if let Some(c) = corner.new_side_edge_ext_tan_face_uv.as_mut() {
                    ignore_status(c.reverse_parameterization(k, &dummy))?;
                }
            }
            let new_crv = ner(corner.new_side_edge_extension_3d.take())?;
            let end_uv = corner.new_side_edge_ext_end_face_uv.take();
            let tan_uv = corner.new_side_edge_ext_tan_face_uv.take();

            // Set m_aEndEdges->Curve (deletes the edge's curve and UV trim curves)
            with_kernel_callbacks(k, b, |_k, b, env| {
                edge::replace_3d_curve(b, end_edge, Some(new_crv), env)
            })?;
            own_edge_curve(b, end_edge)?;
            // Set Edgeuse->UVTrimCurve for the edge uses into the end and tangent faces
            // (no validation: they are in an inconsistent state)
            let deviation = self.new_edge_deviation;
            set_trim_curve(
                k,
                b,
                end_edge,
                self.end_face(which_end)?,
                end_uv,
                deviation,
                false,
            )?;
            set_trim_curve(
                k,
                b,
                end_edge,
                self.tan_face(which_side)?,
                tan_uv,
                deviation,
                false,
            )?;
        } // end no-side-edge checks, both ends

        // Now we can delete the fillet Face.
        // Two FALSE flags:
        //  don't delete Edges, etc.;
        //  and don't combine shells/regions.
        let face_to_delete = self.face_to_delete;
        // (status ignored upstream)
        ignore_status(with_kernel_callbacks(k, b, |k, b, env| {
            brep_p3::delete_face(
                k,
                b,
                env,
                &mut Marks::default(),
                face_to_delete,
                DeleteFaceOptions {
                    delete_edges_vertices: false,
                    do_region_nesting: false,
                    region_to_keep: None,
                },
            )
        }))?;

        // Loop over both End faces and Side (tangent) faces,
        // extending them and their surfaces to include any new uv points.
        for i in 0..2 {
            // Update the end faces.
            let face = self.end_face(i)?;
            let mut face_domain = crate::topology::face::p2::uv_domain(b, face)?;
            face_domain.add_point(
                *self
                    .uv_end_surf_new
                    .get(i)
                    .ok_or_else(|| null("end index"))?,
            );
            grow_face(k, b, face, &face_domain)?;

            // Now do the side (tangent) faces.  These have two new uv points.
            let face = self.tan_face(i)?;
            let mut face_domain = crate::topology::face::p2::uv_domain(b, face)?;
            face_domain.add_point(self.corner(0, i)?.uv_tan_surf_new);
            face_domain.add_point(self.corner(1, i)?.uv_tan_surf_new);
            grow_face(k, b, face, &face_domain)?;
        } // end loop on two End faces and two Tangent faces.

        // Now, for both ends:
        //   Set the new vertex positions,
        //   and extend the two side edges.
        for which_end in 0..2 {
            // Set the vertex positions,
            // and adjust their tolerances (only if bigger: TRUE arg).
            let new_vtx_pt = *self
                .new_vtx_pts
                .get(which_end)
                .ok_or_else(|| null("end index"))?;
            let new_tol = *self
                .new_vtx_tols
                .get(which_end)
                .ok_or_else(|| null("end index"))?;

            // But leave the corner alone if we didn't record a side edge
            // (more than three vertices at that vertex).
            for which_side in 0..2 {
                let corner = self.corner(which_end, which_side)?;
                if corner.no_side_edge() {
                    continue;
                }
                let v = ner(corner.vertex)?;
                set_vertex_point(k, b, v, new_vtx_pt)?;
                let zone = tol::zone_tol3d(k, new_tol);
                vertex::set_tolerance(b, v, zone, true, true)?;
            }

            // For the side edges: append the last bit of intersection if necessary.
            for which_side in 0..2 {
                if self.corner(which_end, which_side)?.no_side_edge() {
                    continue; // more than three edges, don't extend anything.
                }
                self.extend_side_edge(k, b, which_end, which_side)?;
            } // end loop on two Sides at this end, updating side edges.
        } // end loop on both Ends.

        self.install_new_edge(k, b)
    }

    /// The side edge part of `DoRemoval` at one corner: the edge's domain grows to the
    /// new corner's parameter, or the intersection piece is joined onto its curve and UV
    /// trim curves and its domain reset to the far vertex's drop and the joined end.
    fn extend_side_edge(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        which_end: usize,
        which_side: usize,
    ) -> Result<()> {
        let tol = self.tol_3d;
        let end_face = self.end_face(which_end)?;
        let tan_face = self.tan_face(which_side)?;
        let corner = self.corner(which_end, which_side)?;
        let side_edge = ner(corner.side_edge)?;
        let high_end = corner.high_end_of_side_edge;
        let mut new_side_edge_domain = edge::interval(b, side_edge)?;

        if corner.new_side_edge_extension_3d.is_none() {
            let new_side_edge_param = corner.new_side_edge_param;
            // Not upstream (WR-606, exact only): its line run on to the corner (the old
            // part keeps its parameters); the UV trims go with the old curve and are made
            // anew at the end of DoRemoval.
            if let Some(mut new_curve) = self
                .corner_mut(which_end, which_side)?
                .new_side_curve
                .take()
            {
                with_kernel_callbacks(k, b, |_k, b, env| {
                    let owner = (env.key_of)(EntityRef::Edge(side_edge));
                    let _ = new_curve.set_owner(Some(owner), Some(brep_p2::brep_ref(b)));
                    let options = SetCurveOptions {
                        delete_old_curve: true,
                        ..SetCurveOptions::default()
                    };
                    edge::set_curve(b, side_edge, Some(new_curve), options, env, [None, None])
                        .map(drop)
                })?;
                self.corner_mut(which_end, which_side)?.side_curve_replaced = true;
            }

            // Curve was long enough: just update the Edge's domain.
            new_side_edge_domain.add_value(new_side_edge_param);
            edge::set_interval(b, side_edge, &new_side_edge_domain)?;

            // And the two uv curves: in End face, and Tangent face. (Upstream reads the
            // end face's, making it when missing, and does nothing with it.)
            ignore_status(edge::uv_trim_curve_of_surface(
                k,
                b,
                side_edge,
                Some(end_face),
            ))?;
            return Ok(());
        }

        // Append this curve piece to the side edge's curve.
        // First, we had better make sure that the side edge curve
        // doesn't extend past the original vertex position.
        let mut curve = ner(b.edge_mut(side_edge)?.curve.take())?;
        let r = (|| -> Result<Extent1d> {
            // pCurve will have to be a SmBSplineCurve, for JoinWith().
            if bspline_part(&curve).is_none() {
                return Err(SolidError::null_pointer(OP));
            }
            let mut curve_domain = curve.natural_interval();
            if high_end {
                curve_domain.set_min_max(curve_domain.min(), new_side_edge_domain.max())?;
            } else {
                curve_domain.set_min_max(new_side_edge_domain.min(), curve_domain.max())?;
            }
            // may snap the interval by tol to existing knots (status ignored)
            ignore_status(curve.trim(k, &mut curve_domain, TrimMode::Notify))?;

            // Now append the new intersection curve.
            let corner = self.corner_mut(which_end, which_side)?;
            let mut ext = ner(corner.new_side_edge_extension_3d.take())?;
            crate::blend::fillet_corner::p3::join_with(k, &mut curve, high_end, &mut ext, false)?;
            Ok(curve.natural_interval())
        })();
        b.edge_mut(side_edge)?.curve = Some(curve);
        let curve_domain = r?;

        // Find the SideEdge's new domain, and update the Edge.
        // Notes: JoinWith() can completely change the parameterization,
        // and, the far end of the Edge (the vertex) is not necessarily
        // at the end of the curve.
        // So: Update the Edge's domain:
        //   This end: same as this end of curve.
        //   Far end: have to drop the far vertex to the curve.
        // (this end's parameter, the far vertex's point and the guess for its drop)
        let (fixed, far_point, guess) = if high_end {
            let t1 = curve_domain.max();
            // Set t0 at the far end.
            let start = ner(edge::start_vertex(b, side_edge)?)?;
            let far_point = b.vertex(start)?.point;
            (
                t1,
                far_point,
                curve_domain.clamp_value(new_side_edge_domain.min()),
            )
        } else {
            let t0 = curve_domain.min();
            // Set t1 at the far end.
            let near = edge::start_vertex(b, side_edge)?;
            let far = ner(edge::other_vertex(b, side_edge, near)?)?;
            let far_point = b.vertex(far)?.point;
            (
                t0,
                far_point,
                curve_domain.clamp_value(new_side_edge_domain.max()),
            )
        };
        let curve = ner(edge::curve(b, side_edge)?)?;
        let drop = or_sm_err(curve.drop_point(
            k,
            &curve_domain,
            far_point,
            None,
            tol,
            Some(guess),
            SolverOperation::Minimize,
        ))?;
        if !drop.success || drop.distance > tol {
            return Err(sm_err());
        }
        let (t0, t1) = if high_end {
            (drop.parameter, fixed)
        } else {
            (fixed, drop.parameter)
        };

        new_side_edge_domain.set_min_max(t0, t1)?;
        edge::set_interval(b, side_edge, &new_side_edge_domain)?;

        // ... and then we have to do the uv curves also.
        // These domains should be the same as the curve.
        for (face, at_end_face) in [(end_face, true), (tan_face, false)] {
            let corner = self.corner_mut(which_end, which_side)?;
            let slot = if at_end_face {
                &mut corner.new_side_edge_ext_end_face_uv
            } else {
                &mut corner.new_side_edge_ext_tan_face_uv
            };
            let Some(ext_uv) = slot.as_mut() else {
                continue;
            };
            // This gets the same-oriented EU in the Face:
            let Some(side_eu) = edge::edgeuse_of_face(b, side_edge, Some(face))? else {
                continue;
            };
            let Some(holder) = uv_trim_holder(b, side_eu)? else {
                continue;
            };
            let mut side_uv = ner(b.edgeuse_mut(holder)?.uv_trim_curve.take())?;
            let joined = crate::blend::fillet_corner::p3::join_with(
                k,
                &mut side_uv,
                high_end,
                ext_uv,
                false,
            );
            if joined.is_ok() {
                // (status ignored upstream)
                ignore_status(side_uv.edit_parameterization(k, &curve_domain))?;
            }
            b.edgeuse_mut(holder)?.uv_trim_curve = Some(side_uv);
            joined?;
            // Done with this:
            *slot = None;
        }

        Ok(())
    }

    /// The tail of `DoRemoval`: the tangent faces' intersection becomes the surviving
    /// tangent edge's curve (turned to run with it), the end edges are deleted and the
    /// corner vertices glued (at an end without a corner of more than three edges), the
    /// two tangent edges glued, and the new UV trim curves set (the last one validating
    /// the edge's geometry).
    fn install_new_edge(&mut self, k: &mut Kernel, b: &mut Brep) -> Result<()> {
        // Install the intersection curve of the two tangent faces
        // into the two tangent edges -- they will be glued.
        // First check the relative orientation of the two tangent edges: needed for Glue.
        let tan_edge0 = self.tan_edge(0)?;
        let tan_edge1 = self.tan_edge(1)?;
        let domain0 = edge::interval(b, tan_edge0)?;
        let domain1 = edge::interval(b, tan_edge1)?;
        let pv0 = curve_tangent(
            k,
            ner(edge::curve(b, tan_edge0)?)?,
            domain0.evaluate(0.5),
            true,
        )?;
        let pv1 = curve_tangent(
            k,
            ner(edge::curve(b, tan_edge1)?)?,
            domain1.evaluate(0.5),
            true,
        )?;
        let orient = if pv0.dot(pv1) > 0.0 {
            OrientType::Same
        } else {
            OrientType::Opposite
        };

        // Note, don't install the new curve in both edges.
        // One edge will be deleted, which deletes its curve.
        let survive_edge = tan_edge0;
        let delete_edge = tan_edge1;

        // We have to get the direction of the new Edge curve right:
        // it has to run in the same direction as the Edge.
        let mut new_curve = ner(self.new_edge_curve_3d.take())?;
        let new_domain = new_curve.natural_interval();
        let new_tan = curve_tangent(k, &new_curve, new_domain.evaluate(0.5), true)?;
        if pv0.dot(new_tan) < 0.0 {
            // (statuses ignored upstream)
            ignore_status(new_curve.reverse_parameterization(k, &new_domain))?;
            for uv in self.new_edge_curve_uvs.iter_mut().flatten() {
                let domain = uv.natural_interval();
                ignore_status(uv.reverse_parameterization(k, &domain))?;
            }
        }
        let domain0 = new_curve.natural_interval();

        // TRUE = delete the edge's curve (and its UV trim curves)
        with_kernel_callbacks(k, b, |_k, b, env| {
            let owner = b.owner_identity(EntityRef::Edge(survive_edge));
            let _ = new_curve.set_owner(Some(owner), Some(brep_p2::brep_ref(b)));
            let options = SetCurveOptions {
                delete_old_curve: true,
                ..SetCurveOptions::default()
            };
            edge::set_curve(b, survive_edge, Some(new_curve), options, env, [None, None])?;
            edge::set_curve(b, delete_edge, None, options, env, [None, None])?;
            Ok(())
        })?;

        // and update the Edges' intervals.
        // Would be just the domain of the new int curve: it went vertex-to-vertex.
        edge::set_interval(b, survive_edge, &domain0)?;
        edge::set_interval(b, delete_edge, &domain0)?;

        // Now we can delete the two end Edges, and glue the vertices,
        // unless either vertex at either end has more than three vertices
        // (in which case we didn't record a side edge).
        for which_end in 0..2 {
            let c0 = self.corner(which_end, 0)?;
            let c1 = self.corner(which_end, 1)?;
            if c0.no_side_edge() || c1.no_side_edge() {
                continue;
            }
            let (v0, v1) = (c0.vertex, c1.vertex);
            let end_edge = self.end_edge(which_end)?;
            with_kernel_callbacks(k, b, |k, b, env| {
                brep_p4::delete_edge(k, b, &mut DeleteEnv::from_notify(env), end_edge, None)
            })?;
            crate::topology::brep::p5::glue_vertices(k, b, v0, v1, None)?;
        }

        // Now glue the two tangent edges, which will become the new (non-tangent) Edge.
        with_kernel_callbacks(k, b, |k, b, env| {
            glue_edges(k, b, env, survive_edge, orient, 0.0, delete_edge)
        })?;

        // Set Edgeuse->UVTrimCurve for the edge uses into the tangent faces
        // (the last one validating the geometry)
        let [uv0, uv1] = std::mem::take(&mut self.new_edge_curve_uvs);
        let deviation = self.new_edge_deviation;
        set_trim_curve(k, b, survive_edge, self.tan_face(0)?, uv0, deviation, false)?;
        set_trim_curve(k, b, survive_edge, self.tan_face(1)?, uv1, deviation, true)?;

        // Not upstream (WR-606, exact only): UV trims of the side edges that run on to the
        // corners, made anew.
        for which_end in 0..2 {
            for which_side in 0..2 {
                let corner = self.corner(which_end, which_side)?;
                let Some(side) = corner.side_edge else {
                    continue;
                };
                if !(corner.side_curve_replaced || corner.rebuild_side_uvs) {
                    continue;
                }
                let uses = b.edge(side)?.edgeuses.clone();
                for &eu in &uses {
                    // (status ignored)
                    ignore_status(eu1::delete_uv_trim_curve(b, eu))?;
                }
                for &eu in &uses {
                    get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
                    if eu1::uv_trim_curve(b, eu)?.0.is_none() {
                        return Err(SolidError::null_pointer(OP));
                    }
                }
            }
        }

        // And that should do it.
        Ok(())
    }
}

/// `smos_3Max`.
fn max3(a: f64, b: f64, c: f64) -> f64 {
    let ab = if a > b { a } else { b };
    if ab > c {
        ab
    } else {
        c
    }
}

/// The `SmBSplineCurve` part of a curve (`SM_CAST_PTR(SmBSplineCurve, ..)`).
fn bspline_part(c: &Curve) -> Option<&crate::nurbs::b_spline_curve::BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(c) => Some(c),
        CurveKind::Line(c) => Some(&c.base),
        CurveKind::Ellipse(c) => Some(c.base()),
        CurveKind::Hyperbola(c) => Some(c.base()),
        CurveKind::Parabola(c) => Some(c.base()),
        _ => None,
    }
}

/// The first derivative of `pCurve->Evaluate(t, 1, bFromLeft, aPV)` (non-zero tangents,
/// the default), whose status upstream ignores: a failure leaves the default-constructed
/// vector ([`Vec3::UNDEF`]).
fn curve_tangent(k: &Kernel, c: &Curve, t: f64, from_left: bool) -> Result<Vec3> {
    Ok(ignored(c.evaluate(k, t, 1, from_left, true))?
        .and_then(|pv| pv.get(1).copied())
        .unwrap_or(Vec3::UNDEF))
}

/// The edge use holding `eu`'s UV trim curve (`GetUVTrimCurve`: its own, else its
/// mate's); `None` when neither holds one.
fn uv_trim_holder(b: &Brep, eu: EdgeuseId) -> Result<Option<EdgeuseId>> {
    let (curve, holder) = eu1::uv_trim_curve(b, eu)?;
    Ok(match curve {
        None => None,
        Some(_) => holder.or(Some(eu)),
    })
}

/// An evaluation copy of `f`'s surface with out-of-bounds evaluation on (upstream turns
/// it on temporarily on the face's own `SmBSplineSurface`).
fn out_of_bounds_copy(b: &Brep, f: FaceId) -> Result<Surface> {
    let mut s = ner(crate::topology::face::p2::surface(b, f)?)?.clone_for_evaluation()?;
    set_out_of_bounds_enabled(&mut s, true);
    Ok(s)
}

/// `pVertex->SetPoint(p)` through the kernel's callbacks.
fn set_vertex_point(k: &mut Kernel, b: &mut Brep, v: VertexId, p: Point3) -> Result<()> {
    with_kernel_callbacks(k, b, |_k, b, env| {
        let this = (env.key_of)(EntityRef::Vertex(v));
        let key_of = env.key_of;
        let vertexuse_ref = |vu| key_of(EntityRef::Vertexuse(vu));
        vertex::set_point(
            b,
            v,
            p,
            &mut *env.records,
            &mut *env.attributes,
            env.context.as_deref_mut(),
            this,
            &vertexuse_ref,
        )
    })
}

/// The face-growing part of `DoRemoval`: `SetUVDomain(domain)`, then for an
/// `SmBSplineSurface` the four boundary spans expanded to it and `UpdateAnalyticalDomain`
/// (statuses ignored upstream).
fn grow_face(k: &Kernel, b: &mut Brep, f: FaceId, domain: &Extent2d) -> Result<()> {
    crate::topology::face::p2::set_uv_domain(b, f, domain)?;
    let Some(surface) = b.face_mut(f)?.surface.as_mut() else {
        return Ok(());
    };
    if !surface.is_kind_of(BSplineSurface::TYPE)? {
        return Ok(());
    }
    for (param, dir) in [
        (domain.min().x, SurfParamType::U),
        (domain.max().x, SurfParamType::U),
        (domain.min().y, SurfParamType::V),
        (domain.max().y, SurfParamType::V),
    ] {
        ignore_status(expand_boundary_span(k, surface, param, dir))?;
    }
    ignore_status(update_analytical_domain(k, surface, domain))
}

/// `SmBSplineSurface::ExpandBoundarySpan` on a surface's `SmBSplineSurface` part, its
/// NURB made first as upstream's `GetOrCreateGwNurbPointer` makes it.
fn expand_boundary_span(
    k: &Kernel,
    surface: &mut Surface,
    param: f64,
    dir: SurfParamType,
) -> Result<()> {
    surface.natural_uv_domain_with_nurb(k)?;
    let base = match surface.kind_mut() {
        SurfaceKind::BSpline(s) => s,
        SurfaceKind::Plane(p) => p.base_mut(),
        SurfaceKind::Cone(c) => c.base_mut(),
        SurfaceKind::SurfOfRevolution(s) => s.base_mut(),
        SurfaceKind::Sphere(s) => s.base_mut().base_mut(),
        SurfaceKind::Torus(s) => s.base_mut().base_mut(),
        SurfaceKind::SurfOfExtrusion(s) => s.base_mut(),
        _ => return Err(SolidError::null_pointer(OP)),
    };
    base.expand_boundary_span(param, dir)
}

/// `SmSurface::UpdateAnalyticalDomain(crNewNurbsDomain)` (virtual; not otherwise
/// ported): the analytic (STEP) domain set from the NURB domain without rebuilding the
/// NURB. `SmPlane`'s override takes the domain and drops its NURB; the surfaces of
/// revolution (cones, spheres and tori too) and of extrusion run theirs; any other
/// surface has the `SmSurface` default, which does nothing.
///
/// # Errors
///
/// Those of the overrides.
pub fn update_analytical_domain(
    k: &Kernel,
    surface: &mut Surface,
    domain: &Extent2d,
) -> Result<()> {
    match surface.kind_mut() {
        SurfaceKind::Plane(p) => {
            p.update_analytical_domain(domain);
            Ok(())
        }
        SurfaceKind::Cone(c) => c.rev.update_analytical_domain(k, domain),
        SurfaceKind::SurfOfRevolution(s) => s.update_analytical_domain(k, domain),
        SurfaceKind::Sphere(s) => s.base_mut().update_analytical_domain(k, domain),
        SurfaceKind::Torus(s) => s.base_mut().update_analytical_domain(k, domain),
        SurfaceKind::SurfOfExtrusion(s) => s.update_analytical_domain(k, domain),
        _ => Ok(()),
    }
}

/// `SmFaceRemoval::MoveVtxAndEdgeToAdjacentFace(pVtxToMove, pEdgeToMove, pFromFace)`: the
/// other edge into `vtx_to_move` that is also in `from_face` moves off it, to the other
/// end of `edge_to_move`: the two edge uses of `edge_to_move` in `from_face` (and their
/// mates) are unlinked from their loops and linked into the adjacent faces' loops,
/// taking those loops' loop uses, and the vertex uses they pass are moved from
/// `vtx_to_move` to the edge's other vertex. Only the topology changes.
///
/// # Errors
///
/// `SM_ERR` when the edge has no edge use on `from_face`; [`SolidError::InvalidInput`]
/// where upstream dereferences null; [`SolidError::NotFound`] for a dangling id.
pub fn move_vtx_and_edge_to_adjacent_face(
    b: &mut Brep,
    vtx_to_move: VertexId,
    edge_to_move: EdgeId,
    from_face: FaceId,
) -> Result<()> {
    // Grab this before we change anything:
    let opp_vtx = ner(edge::other_vertex(b, edge_to_move, Some(vtx_to_move))?)?;

    // Start with the edge use oriented in the direction (vtx_to_move -> edge_to_move),
    // i.e., whose vertex use corresponds to vtx_to_move.
    let Some(mut eu_to_move) = edge::edgeuse_of_face(b, edge_to_move, Some(from_face))? else {
        return Err(sm_err());
    };
    let eu_vertex = |b: &Brep, eu: EdgeuseId| -> Result<Option<VertexId>> {
        let vu = ner(eu1::vertexuse(b, eu)?)?;
        Ok(b.vertexuse(vu)?.vertex)
    };
    if eu_vertex(b, eu_to_move)? != Some(vtx_to_move) {
        eu_to_move = ner(eu1::mate(b, eu_to_move)?)?;
    }

    // first pass: 'Before' means CW
    relink(b, eu_to_move, vtx_to_move, opp_vtx, true)?;

    // Now repeat for the Mates.  CW and CCW get switched.
    let mate = ner(eu1::mate(b, eu_to_move)?)?;
    relink(b, mate, vtx_to_move, opp_vtx, false)
}

/// One pass of [`move_vtx_and_edge_to_adjacent_face`]: `eu` leaves its loop for the
/// adjacent one (beside the radial of its clockwise neighbour in the first pass, of its
/// counterclockwise neighbour in the mates' pass) and a vertex use moves to `opp_vtx`.
fn relink(
    b: &mut Brep,
    eu: EdgeuseId,
    vtx_to_move: VertexId,
    opp_vtx: VertexId,
    first_pass: bool,
) -> Result<()> {
    let rec = b.edgeuse(eu)?;
    let (cw, ccw) = (ner(rec.cw)?, ner(rec.ccw)?);

    // Make sure that this EU is not the one in the list that the EU's Loopuse points to.
    let lu = ner(eu1::loopuse(b, eu)?)?;
    if b.loopuse(lu)?.start == LoopuseStart::Edgeuse(Some(eu)) {
        // Just slide it around the loop by one.
        b.loopuse_mut(lu)?.start = LoopuseStart::Edgeuse(Some(ccw));
    }

    // Get the EUs in the adjacent face where eu should go,
    // and grab the vertexuse to move, before we change anything.
    let (insert_before, insert_after, vu_to_move) = if first_pass {
        let before = ner(eu1::radial(b, cw)?)?;
        let after = ner(b.edgeuse(before)?.cw)?;
        (before, after, ner(eu1::vertexuse(b, before)?)?)
    } else {
        let after = ner(eu1::radial(b, ccw)?)?;
        let before = ner(b.edgeuse(after)?.ccw)?;
        (before, after, ner(eu1::vertexuse(b, ccw)?)?)
    };

    // Unhitch eu from its doubly-linked EU list.
    b.edgeuse_mut(ccw)?.cw = Some(cw);
    b.edgeuse_mut(cw)?.ccw = Some(ccw);

    // Insert between Before and After.
    b.edgeuse_mut(insert_after)?.ccw = Some(eu);
    b.edgeuse_mut(insert_before)?.cw = Some(eu);
    let after_owner = b.edgeuse(insert_after)?.owner;
    {
        let rec = b.edgeuse_mut(eu)?;
        rec.cw = Some(insert_after);
        rec.ccw = Some(insert_before);
        // Change eu's Loopuse.
        rec.owner = after_owner;
    }

    // And move the Vertexuse.
    let list = &mut b.vertex_mut(vtx_to_move)?.vertexuses;
    if let Some(at) = list.iter().position(|&x| x == vu_to_move) {
        list.remove(at);
    }
    b.vertex_mut(opp_vtx)?.vertexuses.push(vu_to_move);
    b.vertexuse_mut(vu_to_move)?.vertex = Some(opp_vtx);
    Ok(())
}

/// The identities the B-rep names its geometry owners by ([`Brep::owner_identity`]),
/// without borrowing it: what [`crate::topology::brep::validation::check_brep`] checks
/// owners against.
fn owner_keys(b: &Brep) -> impl Fn(EntityRef) -> crate::geom::object::ObjectRef {
    let naming = b.naming();
    let recorded = b.geometry_owner_keys().clone();
    move |e| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e)
}

/// `pCurve->SetOwner(pEdge)` for `e`'s curve: owned under the edge's
/// [`Brep::owner_identity`] (`replace_3d_curve` names it by the edit's notification
/// identity).
fn own_edge_curve(b: &mut Brep, e: EdgeId) -> Result<()> {
    let owner = b.owner_identity(EntityRef::Edge(e));
    let brep_ref = brep_p2::brep_ref(b);
    if let Some(c) = b.edge_mut(e)?.curve.as_mut() {
        let _ = c.set_owner(Some(owner), Some(brep_ref));
    }
    Ok(())
}

/// `SmEdge::SetTrimCurve(pFace, pUVTrimCurve, dMaxDistanceToSurface, bDoValidation)`
/// (CO-PORTED, `SmEdge.cpp`): the edge's first edge use on `face` oriented
/// [`OrientType::Same`] takes `uv_curve` (`SetUVTrimCurve`, the old curve deleted); with
/// `validate` the edge's geometry is then checked, fixing tolerances
/// (`ValidateGeometry(TRUE)`).
///
/// # Errors
///
/// `SM_ERR` when no such edge use exists (and from the validation); those of
/// [`eu1::set_uv_trim_curve`].
pub fn set_trim_curve(
    k: &mut Kernel,
    b: &mut Brep,
    e: EdgeId,
    face: FaceId,
    uv_curve: Option<Curve>,
    max_distance_to_surface: f64,
    validate: bool,
) -> Result<()> {
    for eu in edge::edgeuses(b, e, None, None)? {
        // Find the Edgeuse connecting this Edge to the target Face->UpwardFaceuse
        if eu1::face(b, eu)? == Some(face) && eu1::orientation(b, eu)? == OrientType::Same {
            let arg = match uv_curve {
                Some(c) => UvTrimCurveArg::New(Box::new(c)),
                None => UvTrimCurveArg::Null,
            };
            eu1::set_uv_trim_curve(b, eu, arg, max_distance_to_surface, true, None)?;
            if validate {
                // Check the Geometry: side effects change tolerance and interval values
                with_kernel_callbacks(k, b, |k, b, _env| {
                    let keys = owner_keys(b);
                    crate::topology::brep::validation::validate_geometry(k, b, e, true, &keys)
                })?;
            }
            return Ok(());
        }
    }
    // should never arrive there
    Err(sm_err())
}

/// `SmBrep::GlueEdges(pSurvivingEdge, eRelativeOrientation, dEdgeGap, pEdgeToDelete)`
/// (CO-PORTED, `SmBrep.cpp`): glues the lamina edge `to_delete` onto `survivor` (their
/// vertices already glued): the B-rep and the edges are notified, `to_delete`'s edge use
/// pair loses its UV trim curves and moves (orientations swapped for
/// [`OrientType::Opposite`]) into `survivor`'s list, whose tolerance grows to enclose
/// `edge_gap`, and `to_delete` is destroyed.
///
/// # Errors
///
/// `SM_ERR` when `to_delete` is not a lamina edge; [`SolidError::InvalidInput`] where
/// upstream dereferences null; those of the notifications and list edits.
pub fn glue_edges(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut edge::NotifyEnv<'_>,
    survivor: EdgeId,
    relative_orientation: OrientType,
    edge_gap: f64,
    to_delete: EdgeId,
) -> Result<()> {
    // Make sure we have both lamina edges.
    if !eu1::edge_is_lamina(b, to_delete)? {
        return Err(sm_err());
    }

    let this = Some(NotifyArg::Object(brep_p2::brep_ref(b)));
    brep_p2::notify(b, env, None, NotifyOperation::PreEdit, [this, None, None])?;
    let surv_ref = (env.key_of)(EntityRef::Edge(survivor));
    let brep_ref = brep_p2::brep_ref(b);
    edge::notify(
        b,
        survivor,
        env,
        NotifyOperation::PreEdit,
        [Some(surv_ref), Some(brep_ref), None],
    )?;
    brep_p2::notify(
        b,
        env,
        None,
        NotifyOperation::MergeInBrep,
        [
            Some(NotifyArg::Entity(EntityRef::Edge(survivor))),
            Some(NotifyArg::Entity(EntityRef::Edge(to_delete))),
            Some(NotifyArg::Object(brep_ref)),
        ],
    )?;
    // Notify faces of edge being deleted of editing operation
    for f in edge::faces(b, to_delete, None)? {
        let face_ref = (env.key_of)(EntityRef::Face(f));
        crate::topology::face::p3::notify(
            b,
            f,
            env,
            None,
            NotifyOperation::PreEdit,
            [Some(face_ref), Some(brep_ref), None],
        )?;
    }

    // Remove edgeuses from pEdgeToDelete and orient them again
    let deu1 = ner(b.edge(to_delete)?.edgeuses.first().copied())?;
    let deu2 = ner(eu1::mate(b, deu1)?)?;
    // Remove Geometry from Edgeuses
    for eu in [deu1, deu2] {
        let rec = b.edgeuse_mut(eu)?;
        rec.uv_trim_curve = None;
        rec.drop_curve_fail = None;
    }
    crate::topology::brep::p7::edge_remove(b, to_delete, deu1)?;
    crate::topology::brep::p7::edge_remove(b, to_delete, deu2)?;
    if relative_orientation == OrientType::Opposite {
        let o1 = eu1::orientation(b, deu1)?;
        let o2 = eu1::orientation(b, deu2)?;
        eu1::set_orientation(b, deu1, o2)?;
        eu1::set_orientation(b, deu2, o1)?;
    }

    // Add those edgeuses to the surviving edge
    let primary = b.edge(survivor)?.edgeuses.first().copied();
    edge::add_oriented_eu_pair(b, survivor, deu1, deu2, primary)?;

    // Set tolerance based on edge gap
    let del_tol = b.edge(to_delete)?.tolerance;
    let surv_tol = b.edge(survivor)?.tolerance;
    // (smos_Max: a > b ? a : b)
    let max_tol = if del_tol > surv_tol {
        del_tol
    } else {
        surv_tol
    };
    let mut enclosing_radius = edge_gap + max_tol;
    if enclosing_radius < edge_gap * 2.0 {
        enclosing_radius = edge_gap * 2.0;
    }
    if enclosing_radius > surv_tol * 1.1 {
        edge::set_tolerance(b, survivor, enclosing_radius, true, true)?;
    }

    // Delete the edge to delete and we are done
    edge::destroy(k, b, to_delete, env)
}

/// `SmBrep::DeleteFilletFace(pFilletFace, pd3dTol)`: removes the four-sided fillet face
/// `fillet_face` and closes the hole with the sharp edge where its tangent faces meet
/// ([`FilletRemoval::remove_face`]); `tol_3d` defaults to the face's tolerance. With
/// `Kernel::fixes.fillet_removal_takes_ring_fillets` (WR-605, not upstream) a ring
/// fillet goes to [`ring::RingFilletRemoval`] instead.
///
/// # Errors
///
/// `SM_ERR` when editing is not enabled, the face is not a removable fillet or an
/// intersection fails; those of [`FilletRemoval::remove_face`].
pub fn delete_fillet_face(
    k: &mut Kernel,
    b: &mut Brep,
    fillet_face: FaceId,
    tol_3d: Option<f64>,
) -> Result<()> {
    // check state - m_bEditingEnabled must be on
    if !b.editing_enabled() {
        return Err(sm_err());
    }
    let tol_3d = match tol_3d {
        Some(t) => t,
        None => crate::topology::face::p2::tolerance(b, fillet_face)?,
    };
    // Not upstream (WR-605, `fixes.fillet_removal_takes_ring_fillets`; SolidCraft change,
    // tangency-labs/smlib 52bfef0): a ring fillet (closed rails, no ends) has its own
    // removal.
    if k.fixes.fillet_removal_takes_ring_fillets
        && ring::RingFilletRemoval::is_ring(k, b, Some(fillet_face), 2.0)?
    {
        let mut removal = ring::RingFilletRemoval::new(fillet_face, tol_3d);
        let r = removal.remove_face(k, b);
        // (not upstream: the port's cached topology fields, true again for the caller)
        b.refresh_cached_topology();
        return r;
    }
    let mut removal = FilletRemoval::new(fillet_face, tol_3d);
    let r = removal.remove_face(k, b);
    // (not upstream: the port's cached topology fields, true again for the caller)
    b.refresh_cached_topology();
    r
}
