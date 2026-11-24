//! Not upstream: removing a ring fillet (a round all the way round a boss's foot or rim)
//! and closing the hole with the sharp edge where its two tangent faces meet.
//!
//! Port of the SolidCraft fork's `SmRingFilletRemoval` (tangency-labs/smlib 52bfef0,
//! `SmFilletRemoval.cpp` / `.h`, and `SmBrep::JoinShellsAtVertex`, `SmBrep.cpp`), as
//! amended by 661f79c. WR-605; used by [`super::delete_fillet_face`] for every ring
//! fillet (formerly only with `Kernel::fixes.fillet_removal_takes_ring_fillets`).
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
//! - **661f79c.** The glue joins the shells of every region (WR-607), and `DoRemoval`
//!   neither calls `JoinShellsAtVertex` nor clips the tangent faces' domains to their
//!   surfaces', as the fork after 661f79c (not as 52bfef0).
//! - **Ownership.** The new curves are owned values moved into the B-rep (upstream's
//!   pointers, cleared once the B-rep has them). Each B-rep edit notifies through the
//!   kernel's callbacks (`with_kernel_callbacks`). As the fork, a new curve's owner is set
//!   after `SetCurve`, which records the edit's identity of the edge: a B-rep its caller
//!   names ([`NamingScheme::Caller`](crate::topology::NamingScheme::Caller)) owns the
//!   edge's geometry by it from then on.
//! - **Null pointers.** Where the fork dereferences `NULL` the port names what is
//!   missing as the parent module does: an edge's curve, a face's surface, an edge's
//!   vertex, a link of the topology it walks (a rail without primary edge use, a shell
//!   without region), or an object the removal holds as a defect, each keeping
//!   `SM_ERR_NULL_POINTER`.
//! - No recursion.

use super::{
    bspline_part, bspline_part_with_nurb, curve_without_net, defect, edge_curve, edge_vertex,
    editing_disabled, face_surface, glue_edges, have, null, own, own_edge_curve, refused,
    set_vertex_point, stopped, OP,
};
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::{BlendFailure, RemovalInput, RemovalRefusal as R, RemovalStop};
use crate::error::{ignore_status, EntityRef, Failure, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::SolverOperation;
use crate::geom::object::NotifyOperation;
use crate::geom::solution_array::SolutionType;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::{max2, Point3, BIG_DOUBLE, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::extend::Continuity;
use crate::surface::side::BoxSide;
use crate::surface::surface::exact_pieces::certified_lift;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
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
/// A seam that ends before the sharp edge is continued along its line or, for a circular
/// isoparametric seam, along the supporting surface within its natural domain. Everything
/// is computed before the B-rep changes.
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
    /// `m_aSideCurves`: a side edge's curve continued to the new vertex (`None`: its
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

fn is_circle(curve: &Curve) -> bool {
    matches!(curve.kind(), CurveKind::Ellipse(ellipse) if ellipse.is_circle())
}

/// An owned affine seam UV supplies the correspondence between a circular isocurve
/// and the original edge. The two sides of a periodic seam can have different fixed UVs.
struct SeamIso {
    curve: Curve,
    fixed_axis: SurfParamType,
    v0: f64,
    v1: f64,
    uv_interval: Extent1d,
}

impl SeamIso {
    fn from_trim(k: &Kernel, surface: &Surface, trim: &Curve, tol: f64) -> Result<Option<Self>> {
        let Some(part) = bspline_part_with_nurb(trim)? else {
            return Ok(None);
        };
        // Equal homogeneous weights cancel exactly, including on STEP/record splines.
        if part.degree()? != 1
            || part.number_control_points()? != 2
            || (part.is_rational()?
                && !matches!(part.control_points()?, [a, b] if a.w.is_finite() && a.w != 0.0 && a.w == b.w))
        {
            return Ok(None);
        }
        let ends = part.ends()?;
        let (start, end) = (ends.start, ends.end);
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let (fixed_axis, fixed, fixed_span, v0, v1, varying) = if start.x == end.x {
            (
                SurfParamType::U,
                start.x,
                domain.u_interval(),
                start.y,
                end.y,
                domain.v_interval(),
            )
        } else if start.y == end.y {
            (
                SurfParamType::V,
                start.y,
                domain.v_interval(),
                start.x,
                end.x,
                domain.u_interval(),
            )
        } else {
            return Ok(None);
        };
        if v0 == v1
            || !(v1 - v0).is_finite()
            || !fixed.is_finite()
            || !fixed_span.contains_value(fixed, 0.0)
        {
            return Ok(None);
        }
        let curve = surface
            .create_iso_parametric_curve(k, fixed_axis, fixed, tol, None, false)?
            .curve;
        if !is_circle(&curve) || curve.natural_interval() != varying {
            return Ok(None);
        }
        Ok(Some(Self {
            curve,
            fixed_axis,
            v0,
            v1,
            uv_interval: part.natural_interval(),
        }))
    }

    fn edge_parameter(&self, v: f64) -> f64 {
        self.uv_interval
            .evaluate((v - self.v0) / (self.v1 - self.v0))
    }

    /// The arc missing from this end of a meridian or tube circle, measured on the
    /// circle from its natural boundary in the direction of the selected edge end.
    fn missing_arc(
        &self,
        k: &Kernel,
        point: Point3,
        at_high: bool,
        tol: f64,
    ) -> Result<Option<(BoxSide, f64)>> {
        let varying = self.curve.natural_interval();
        let drop = self.curve.drop_point(
            k,
            &varying,
            point,
            None,
            tol,
            None,
            SolverOperation::Minimize,
        )?;
        if !drop.success {
            return Err(stopped(RemovalStop::SeamDropStopped));
        }
        if drop.distance <= tol {
            return Ok(None);
        }
        let high = at_high == (self.v1 > self.v0);
        let t = if high { varying.max() } else { varying.min() };
        let evaluated = self.curve.evaluate(k, t, 1, high, true)?;
        let [start, derivative, ..] = evaluated.as_slice() else {
            return Err(defect(
                "a curve evaluated with one derivative gives two vectors",
            ));
        };
        let CurveKind::Ellipse(circle) = self.curve.kind() else {
            return Err(defect("a seam's iso curve is made only from a circle"));
        };
        let center = circle.position().origin();
        let radius = circle.radius();
        let radial = (*start - center).unitize()?;
        let forward = if high { *derivative } else { -*derivative }.unitize()?;
        let target = point - center;
        if (target.length() - radius).abs() > tol || target.dot(radial.cross(forward)).abs() > tol {
            return Err(refused(R::SeamTrimMissesNewVertex));
        }
        let angle = target.dot(forward).atan2(target.dot(radial));
        if angle <= 0.0 || !angle.is_finite() {
            return Err(refused(R::SeamTrimMissesNewVertex));
        }
        let side = match (self.fixed_axis == SurfParamType::U, high) {
            (true, false) => BoxSide::VMin,
            (true, true) => BoxSide::VMax,
            (false, false) => BoxSide::UMin,
            (false, true) => BoxSide::UMax,
        };
        // Include the requested vertex in the search interval. Its eventual distance
        // and the entire new pair of pcurves are checked at the original tolerance.
        Ok(Some((side, radius * angle + tol)))
    }
}

/// Replacing geometry invalidates the face's edge uses. The extension kept the old
/// parameters, so each existing pcurve, including either side of a seam, still applies.
fn install_extended_surface(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    surface: Surface,
) -> Result<()> {
    let domain = crate::topology::face::p2::uv_domain(b, f)?;
    let mut trims = Vec::new();
    for eu in crate::topology::face::p4::upward_edgeuses(b, f, false, None)? {
        if let (Some(trim), Some(owner)) = eu1::uv_trim_curve(b, eu)? {
            trims.push((owner, trim.clone_for_evaluation()?));
        }
    }
    let owner = b.owner_identity(EntityRef::Face(f));
    with_kernel_callbacks(k, b, |k, b, env| {
        brep_p2::replace_surface(
            k,
            b,
            env,
            f,
            surface,
            owner,
            brep_p2::OldSurface::Delete,
            false,
        )
    })?;
    crate::topology::face::p2::set_uv_domain(b, f, &domain)?;
    for (eu, trim) in trims {
        eu1::set_uv_trim_curve(b, eu, UvTrimCurveArg::New(Box::new(trim)), 0.0, true, None)?;
    }
    Ok(())
}

/// Continue a straight seam along its end span, using the rail gap to bound the search.
pub(super) fn extend_line_seam(
    k: &Kernel,
    curve: &Curve,
    at_high: bool,
    reach: f64,
    tol: f64,
) -> Result<Curve> {
    let ivl = curve.natural_interval();
    let end = if at_high { ivl.max() } else { ivl.min() };
    let pv = curve.evaluate(k, end, 1, true, false)?;
    let [_, d1, ..] = pv.as_slice() else {
        return Err(null("short evaluation"));
    };
    let speed = d1.length();
    if speed.is_nan() || speed <= EFF_ZERO {
        return Err(stopped(RemovalStop::RunOnEndHasNoSpeed));
    }
    let reach_param = (reach + 10.0 * tol) / speed;
    let far = if at_high {
        end + reach_param
    } else {
        end - reach_param
    };
    let part = own(
        bspline_part_with_nurb(curve)?,
        "the seam's B-spline part was checked before it is run on",
    )?;
    match part.create_extended_curve(far, ContinuityType::CInfinity, true)? {
        Some(longer) => Ok(Curve::from(longer)),
        None => Err(stopped(RemovalStop::CurveNotExtended)),
    }
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
        own(self.rails.get(i).copied().flatten(), "the ring's rails, tangent faces and vertices are found for both indices before any is read")
    }

    fn tan_face(&self, i: usize) -> Result<FaceId> {
        own(self.tan_faces.get(i).copied().flatten(), "the ring's rails, tangent faces and vertices are found for both indices before any is read")
    }

    fn vertex(&self, i: usize) -> Result<VertexId> {
        own(self.vertices.get(i).copied().flatten(), "the ring's rails, tangent faces and vertices are found for both indices before any is read")
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
        let fil_face = have(self.face_to_delete, RemovalInput::FaceToDelete)?;
        if !Self::is_ring(k, b, Some(fil_face), tangency_tol_deg)? {
            return Err(refused(R::NotARing));
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
                return Err(refused(R::EdgeNotBetweenTwoFaces));
            };
            if rail > 1 {
                return Err(refused(R::MoreThanTwoRails));
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
            return Err(refused(R::RailsNotBetweenTwoFaces));
        }
        for ii in 0..2 {
            let surface = crate::topology::face::p2::surface(b, self.tan_face(ii)?)?;
            match surface {
                Some(s) if s.is_kind_of(BSplineSurface::TYPE)? => {}
                _ => return Err(refused(R::TangentFaceNotBSpline)),
            }
            let rail = self.rail(ii)?;
            let v = edge_vertex(edge::start_vertex(b, rail)?, rail)?;
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
                return Err(refused(R::SeamDoesNotJoinRails));
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
                    return Err(refused(R::RailVertexHasOtherEdges));
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
            // (not upstream, #18575) copies without their faces (the copy constructor resets
            // the owner): the sharp edge lies where the fillet was, off both faces, so the
            // solve is of the untrimmed surfaces, and a face-owned surface's cache made
            // without its face is not ported
            let surf0 = face_surface(b, self.tan_face(0)?)?.clone();
            let surf1 = face_surface(b, self.tan_face(1)?)?.clone();
            surf0.global_surface_intersect(
                k,
                &surf0.natural_uv_domain_with_nurb(k)?,
                &surf1,
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
            let prim = have(
                eu1::primary_edgeuse(b, self.rail(ii)?)?,
                RemovalInput::PrimaryEdgeUse,
            )?;
            for kk in 0..NUM_SAMPLES {
                let t = (f64::from(kk) + 0.5) / f64::from(NUM_SAMPLES);
                pts.push(normalized_evaluate(k, b, prim, t, false, false, true)?.point);
            }
        }
        let mut chord = 0.0;
        {
            let rail1 = self.rail(1)?;
            let rail1_ivl = edge::interval(b, rail1)?;
            let rail1_curve = edge_curve(b, rail1)?;
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
                    return Err(stopped(RemovalStop::RailDropStopped));
                }
                chord = max2(chord, d.distance);
            }
        }
        let reach = 6.0 * chord;

        // The closed curve every rail sample lies within reach of, the nearest.
        let mut found = curves_3d.len();
        let mut best = BIG_DOUBLE;
        // a drop that stopped proves nothing about the distance (#15034 rule 5)
        let mut undecided = false;
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
                        undecided = true;
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
            // (an undecided drop proves no inexactness)
            self.not_exact = !undecided;
            return Err(no_sharp_curve(undecided));
        };
        if bspline_part(&sharp).is_none() {
            return Err(curve_without_net(None, &sharp));
        }

        // Run as rail 0.
        {
            let rail0 = self.rail(0)?;
            let rail0_curve = edge_curve(b, rail0)?;
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
                return Err(stopped(RemovalStop::SharpDropStopped));
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
            let side_curve = edge_curve(b, side)?.clone();
            if bspline_part(&side_curve).is_none() {
                return Err(curve_without_net(Some(side), &side_curve));
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
                let linear = side_curve.is_linear(k, tol, false)?.linear;
                let (longer, search) = if linear {
                    let longer =
                        extend_line_seam(k, &side_curve, self.side_at_high(ii)?, reach, tol)?;
                    let search = longer.natural_interval();
                    (longer, search)
                } else if is_circle(&side_curve) {
                    Self::extend_circular_seam(
                        k,
                        b,
                        self.tan_face(ii)?,
                        side,
                        self.side_at_high(ii)?,
                        tol,
                    )?
                } else {
                    return Err(curved_seam_without_crossing());
                };
                solutions = sharp.global_curve_intersect(k, &ivl, &longer, &search, tol, &none)?;
                if solutions.is_empty() {
                    // the global intersector finding no crossing is no proof that the curve
                    // misses the sharp edge (#15034 rule 5)
                    return Err(stopped(RemovalStop::SeamCrossingNotFound));
                }
                if !linear
                    && solutions
                        .as_slice()
                        .iter()
                        .any(|s| s.solution_type == SolutionType::RangeOfValues)
                {
                    return Err(refused(R::SeamCrossesSharpEdgeTwice));
                }
                *self
                    .side_curves
                    .get_mut(ii)
                    .ok_or_else(|| null("side index"))? = Some(longer);
            }
            // One crossing (a closed curve starting there meets it at both its ends).
            let Some(first) = solutions.get(0) else {
                return Err(defect("the crossings were checked to be there above"));
            };
            let (first_t, first_side_t) = (first.start.parameters[0], first.start.parameters[1]);
            let at = sharp.evaluate_point(k, first_t)?;
            for kk in 1..solutions.len() {
                let Some(s) = solutions.get(kk) else {
                    return Err(defect("a crossing's index is below their count"));
                };
                let other = sharp.evaluate_point(k, s.start.parameters[0])?;
                if other.distance(at) > tol {
                    return Err(refused(R::SeamCrossesSharpEdgeTwice));
                }
            }
            if have_vertex {
                // Both tangent faces have a seam there: they must cross the sharp edge
                // together.
                if at.distance(self.new_vertex) > tol {
                    return Err(refused(R::SeamsCrossApart));
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
            own(
                bspline_part(&before).map(|_| ()),
                "the sharp curve's B-spline form was checked when it was chosen",
            )?;
            let mut after_ivl = Extent1d::from_min_max(start_at, ivl.max());
            let mut before_ivl = Extent1d::from_min_max(ivl.min(), start_at);
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

    /// A STEP sphere may end exactly at the fillet rail. Continue its meridian (or
    /// a torus's tube) without moving any of the face's existing UV parameters.
    pub(super) fn extend_circular_support(
        k: &mut Kernel,
        b: &mut Brep,
        side: EdgeId,
        ends: [Option<Point3>; 2],
        tol: f64,
    ) -> Result<()> {
        if !edge::curve(b, side)?.is_some_and(is_circle) {
            return Ok(());
        }
        let edge_uses = b.edge(side)?.edgeuses.clone();
        for f in edge_faces(b, side)? {
            let tol = tol
                .min(crate::topology::face::p2::tolerance(b, f)?)
                .min(edge::tolerance(b, side)?);
            for (at_high, point) in [false, true].into_iter().zip(ends) {
                let Some(point) = point else { continue };
                let surface = face_surface(b, f)?;
                let swap = match surface.kind() {
                    SurfaceKind::Sphere(s) => s.base().swap_uv(),
                    SurfaceKind::Torus(t) => t.base().swap_uv(),
                    _ => break,
                };
                let mut extension = None;
                for &eu in &edge_uses {
                    if eu1::face(b, eu)? != Some(f) {
                        continue;
                    }
                    if let Some(trim) = eu1::uv_trim_curve_pointer(b, eu)? {
                        if let Some(iso) = SeamIso::from_trim(k, surface, trim, tol)? {
                            extension = iso.missing_arc(k, point, at_high, tol)?;
                            break;
                        }
                    }
                }
                let Some((side, distance)) = extension else {
                    continue;
                };
                // Every meridian or tube has the same radius as the one measured at
                // the middle of the domain by the parameter-preserving extender.
                if side.is_u() != swap {
                    return Err(curved_seam_without_crossing());
                }
                let grown = surface.extended_sides_keeping_parameters(
                    k,
                    &[side],
                    Continuity::Natural,
                    distance,
                )?;
                let deviation = grown.source + grown.extension;
                if !deviation.is_finite() || deviation > tol {
                    return Err(stopped(RemovalStop::ExtensionDeviates));
                }
                let SurfaceKind::BSpline(net) = grown.surface.kind() else {
                    return Err(stopped(RemovalStop::GrownSurfaceNotBSpline));
                };
                // Analytic recognition keeps the certified extended net, its knots
                // and its UVs; rebuilding the canonical sphere would change the old UVs.
                let continued = match surface.kind() {
                    SurfaceKind::Sphere(_) => {
                        crate::surface::sphere::Sphere::is_nurb_surface_sphere(k, net, 1.0)?
                            .map(Surface::from)
                    }
                    SurfaceKind::Torus(_) => {
                        crate::surface::torus::Torus::is_nurb_surface_torus(k, net, 1.0)?
                            .map(Surface::from)
                    }
                    _ => None,
                }
                .ok_or_else(|| stopped(RemovalStop::ExtendedSupportNotRecognised))?;
                install_extended_surface(k, b, f, continued)?;
            }
        }
        Ok(())
    }

    /// Lift the seam's affine UV line on its support, keeping the edge's parameter.
    /// Search only beyond the end at the fillet and within the support's natural domain.
    pub(super) fn extend_circular_seam(
        k: &Kernel,
        b: &Brep,
        face: FaceId,
        side: EdgeId,
        at_high: bool,
        tol: f64,
    ) -> Result<(Curve, Extent1d)> {
        let surface = face_surface(b, face)?;
        if !matches!(
            surface.kind(),
            SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)
        ) {
            return Err(curved_seam_without_crossing());
        }
        let edge_ivl = edge::interval(b, side)?;
        for &eu in &b.edge(side)?.edgeuses {
            if eu1::face(b, eu)? != Some(face) {
                continue;
            }
            let (uv, owner) = eu1::uv_trim_curve(b, eu)?;
            let Some(uv) = uv.filter(|_| owner == Some(eu)) else {
                continue;
            };
            let Some(iso) = SeamIso::from_trim(k, surface, uv, tol)? else {
                continue;
            };
            let varying = iso.curve.natural_interval();
            let ta = iso.edge_parameter(varying.min());
            let tb = iso.edge_parameter(varying.max());
            if !ta.is_finite() || !tb.is_finite() {
                return Err(stopped(RemovalStop::SeamParameterNotFinite));
            }
            let extended = Extent1d::from_min_max(ta, tb);
            let search = if at_high {
                Extent1d::from_raw(edge_ivl.max(), extended.max())
            } else {
                Extent1d::from_raw(extended.min(), edge_ivl.min())
            };
            if search.length() <= 0.0 || !edge_ivl.is_contained_by(&extended, 0.0) {
                continue;
            }
            let mut arc = iso.curve;
            if iso.v1 < iso.v0 {
                arc.reverse_parameterization(k, &varying)?;
            }
            arc.edit_parameterization_exactly(k, &extended)?;
            return Ok((arc, search));
        }
        Err(curved_seam_without_crossing())
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
        let surf = face_surface(b, self.tan_face(which_side)?)?;
        let new_t = *self
            .side_params
            .get(which_side)
            .ok_or_else(|| null("side index"))?;

        let at_high = self.side_at_high(which_side)?;
        let circular = self
            .side_curves
            .get_mut(which_side)
            .and_then(Option::as_mut)
            .filter(|arc| is_circle(arc));
        let seam_ivl = if let Some(arc) = circular {
            let old = edge::interval(b, side)?;
            let mut span = if at_high {
                Extent1d::from_min_max(old.min(), new_t)
            } else {
                Extent1d::from_min_max(new_t, old.max())
            };
            let intended = span;
            arc.trim(k, &mut span, TrimMode::Notify)?;
            if span != intended {
                arc.edit_parameterization_exactly(k, &intended)?;
            }
            Some(intended)
        } else {
            None
        };

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
                return Err(refused(R::SeamHasMoreThanTwoTrims));
            }
            let extended = if uv.natural_interval().contains_value(new_t, 0.0) {
                if bspline_part(uv).is_none() {
                    return Err(curve_without_net(Some(side), uv));
                }
                uv.clone()
            } else {
                let Some(part) = bspline_part_with_nurb(uv)? else {
                    return Err(curve_without_net(Some(side), uv));
                };
                match part.create_extended_curve(new_t, ContinuityType::CInfinity, true)? {
                    Some(longer) => Curve::from(longer),
                    None => return Err(stopped(RemovalStop::CurveNotExtended)),
                }
            };

            // It must reach the face's surface at the new vertex.
            let uv_at = extended.evaluate_point(k, new_t)?;
            let at = surf.evaluate_point(k, Point2::new(uv_at.x, uv_at.y))?;
            if at.distance(self.new_vertex) > tol {
                return Err(refused(R::SeamTrimMissesNewVertex));
            }
            if let Some(span) = seam_ivl {
                // Both sides of the seam must lift to the same 3D edge over its entire trim.
                let arc = own(
                    self.side_curves.get(which_side).and_then(Option::as_ref),
                    "a seam's span is kept only with the arc it was found on",
                )?;
                let seam_tol = tol.min(edge::tolerance(b, side)?);
                certified_lift(k, surf, &extended, arc, span, seam_tol)?;
            }
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
        }
        if pair == 0 {
            return Err(refused(R::SeamHasNoOwnTrim));
        }
        Ok(())
    }

    /// `DoRemoval`: the fillet goes, its rails become the sharp edge.
    #[allow(clippy::too_many_lines)]
    fn do_removal(&mut self, k: &mut Kernel, b: &mut Brep) -> Result<()> {
        let new_edge_curve = own(
            self.new_edge_curve.take(),
            "the sharp curve is made before the ring is removed",
        )?;
        with_kernel_callbacks(k, b, |_k, b, env| {
            let this = Some(NotifyArg::Object(brep_p2::brep_ref(b)));
            brep_p2::notify(b, env, None, NotifyOperation::PreEdit, [this, None, None])
        })?;

        // The fillet goes; its seam (a wire edge then) with it.
        let face_to_delete = own(
            self.face_to_delete.take(),
            "the face to delete is set when the removal is made",
        )?;
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
                // Its curve continued (the old part keeps its parameters); its UV trims
                // go with the old curve and are replaced below.
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
            let c = edge_curve(b, e)?;
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

        // UV trims of the sharp edge on both tangent faces, and their domains to take
        // them in.
        let uses = b.edge(survive_edge)?.edgeuses.clone();
        for &eu in &uses {
            get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status?;
            if eu1::uv_trim_curve(b, eu)?.0.is_none() {
                return Err(uv_trim_not_made(eu));
            }
        }
        for ii in 0..2 {
            let face = self.tan_face(ii)?;
            let eu = have(
                edge::edgeuse_of_face(b, survive_edge, Some(face))?,
                RemovalInput::EdgeUseOnFace,
            )?;
            let Some(uv) = eu1::uv_trim_curve(b, eu)?.0 else {
                return Err(uv_trim_not_made(eu));
            };
            let mut face_domain: Extent2d = crate::topology::face::p2::uv_domain(b, face)?;
            let uv_ivl = uv.natural_interval();
            for kk in 0..=16u32 {
                let p = uv.evaluate_point(k, uv_ivl.evaluate(f64::from(kk) / 16.0))?;
                face_domain.add_point(Point2::new(p.x, p.y));
            }
            crate::topology::face::p2::set_uv_domain(b, face, &face_domain)?;
        }
        Ok(())
    }
}

/// `pEdge->SetCurve(pCurve, TRUE)` (deleting the old curve and the edge uses' UV trim
/// curves; status ignored), then `pCurve->SetOwner(pEdge)`: owned under the edge's
/// [`Brep::owner_identity`] as `SetCurve` left it, what the check compares.
fn install_curve(k: &mut Kernel, b: &mut Brep, e: EdgeId, curve: Option<Curve>) -> Result<()> {
    with_kernel_callbacks(k, b, |_k, b, env| {
        let options = SetCurveOptions {
            delete_old_curve: true,
            ..SetCurveOptions::default()
        };
        ignore_status(edge::set_curve(b, e, curve, options, env, [None, None]))?;
        own_edge_curve(b, e)
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
    let Some(v) = v else {
        return Err(stopped(RemovalStop::VertexNotGiven));
    };
    if !b.editing_enabled() {
        return Err(editing_disabled());
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
            let region = have(b.shell(join)?.region, RemovalInput::ShellRegion)?;
            shell::region_remove_shell(b, region, join)?;
            with_kernel_callbacks(k, b, |k, b, env| shell::destroy(k, b, join, env))?;
            if let Some(slot) = shells.get_mut(jj) {
                *slot = None;
            }
        }
    }
    Ok(())
}

/// A curved seam with no crossing and no proved continuation: a named stop. The global
/// intersector finding no crossing proves nothing about the seam stopping short, so nothing
/// is proved inexact either (#15034 rule 5).
#[track_caller]
fn curved_seam_without_crossing() -> SolidError {
    stopped(RemovalStop::SeamContinuationNotFound)
}

/// The edge use `eu`'s UV trim curve is not made ([`RemovalStop::UvTrimNotMade`] at the
/// edge use, `SM_ERR_NULL_POINTER`).
#[track_caller]
fn uv_trim_not_made(eu: EdgeuseId) -> SolidError {
    Failure::new(OP, BlendFailure::RemovalStopped(RemovalStop::UvTrimNotMade))
        .at(EntityRef::Edgeuse(eu))
        .into_error()
}

/// Why no candidate curve is the sharp edge: a named stop when a drop onto a candidate
/// stopped (`undecided`), which proves nothing about it (#15034 rule 5); otherwise
/// [`R::TangentSurfacesDoNotMeet`]: the surfaces don't meet near the fillet within their
/// domains, so the sharp edge would lie on a continuation of one of them.
#[track_caller]
fn no_sharp_curve(undecided: bool) -> SolidError {
    if undecided {
        stopped(RemovalStop::SharpCurveNotDecided)
    } else {
        refused(R::TangentSurfacesDoNotMeet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drop onto a candidate curve that stopped proves nothing: no sharp curve is then a
    /// named stop, never the refusal (#15034 rule 5).
    #[test]
    fn no_sharp_curve_is_refused_only_when_every_drop_converged() {
        let undecided = no_sharp_curve(true);
        assert_eq!(
            undecided.blend_failure(),
            Some(&BlendFailure::RemovalStopped(
                RemovalStop::SharpCurveNotDecided
            ))
        );
        assert!(!undecided.is_defect() && !undecided.is_refusal());
        let refused = no_sharp_curve(false);
        assert_eq!(
            refused.blend_failure(),
            Some(&BlendFailure::RemovalRefused(R::TangentSurfacesDoNotMeet))
        );
        assert!(refused.is_refusal() && !refused.is_defect());
        assert_eq!(
            undecided.failed_routine_status(),
            refused.failed_routine_status()
        );
    }

    /// A curved seam with no crossing found is not proved to stop short of the sharp edge:
    /// a named stop, never a refusal.
    #[test]
    fn a_curved_seam_without_a_crossing_is_a_named_stop() {
        let e = curved_seam_without_crossing();
        assert_eq!(
            e.blend_failure(),
            Some(&BlendFailure::RemovalStopped(
                RemovalStop::SeamContinuationNotFound
            ))
        );
        assert!(!e.is_defect() && !e.is_refusal());
        assert_eq!(e.failed_routine_status(), Some(crate::error::SM_ERR));
    }

    /// A removal whose face is already gone (an earlier call of the same removal took it)
    /// names the missing face to delete, as upstream's `NER` (`SM_ERR_NULL_POINTER`): the
    /// public `remove_face` reaches it, so it is not a defect.
    #[test]
    fn a_removal_without_its_face_names_it() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let mut removal = RingFilletRemoval::new(FaceId::from_index(0), 1e-6);
        removal.face_to_delete = None;
        let e = removal.collect_ring_topology(&k, &mut b).unwrap_err();
        assert_eq!(
            e.blend_failure(),
            Some(&BlendFailure::RemovalInputMissing(
                RemovalInput::FaceToDelete
            ))
        );
        assert_eq!(
            e.failed_routine_status(),
            Some(crate::error::SM_ERR_NULL_POINTER)
        );
        assert!(!e.is_defect() && !e.is_refusal());
    }
}
