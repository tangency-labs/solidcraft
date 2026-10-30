//! Top-down B-rep construction: regions, shells, faces, loops and edges are started and
//! ended in order, edges shared by two faces are joined by passing the existing edge,
//! and vertices are made when a loop is ended (`SmBrepConstructor`).
//!
//! Upstream: `source/SMLib/src/SmBrepData.cpp` (its first part), `source/SMLib/inc/SmBrepData.h`.
//!
//! | Rust                                                | Upstream                                         |
//! |-----------------------------------------------------|--------------------------------------------------|
//! | [`VertexNode`]                                      | `SmVertexNode` (constructors, `Set`)             |
//! | [`BrepConstructor`] / [`BrepConstructor::new`]      | `SmBrepConstructor` / its constructor            |
//! | [`BrepConstructor::start_brep`]                     | `SmBrepConstructor::StartBrep`                   |
//! | [`BrepConstructor::end_brep`]                       | `SmBrepConstructor::EndBrep`                     |
//! | [`BrepConstructor::start_region`]                   | `SmBrepConstructor::StartRegion`                 |
//! | [`BrepConstructor::end_region`]                     | `SmBrepConstructor::EndRegion`                   |
//! | [`BrepConstructor::start_shell`]                    | `SmBrepConstructor::StartShell`                  |
//! | [`BrepConstructor::end_shell`]                      | `SmBrepConstructor::EndShell`                    |
//! | [`BrepConstructor::start_face`]                     | `SmBrepConstructor::StartFace`                   |
//! | [`BrepConstructor::end_face`]                       | `SmBrepConstructor::EndFace`                     |
//! | [`BrepConstructor::start_and_end_single_vertex_loop`] | `SmBrepConstructor::StartAndEndSingleVertexLoop` |
//! | [`BrepConstructor::start_vertex_of_loop`]           | `SmBrepConstructor::StartVertexOfLoop`           |
//! | [`BrepConstructor::start_loop`]                     | `SmBrepConstructor::StartLoop`                   |
//! | [`BrepConstructor::end_loop`]                       | `SmBrepConstructor::EndLoop`                     |
//! | [`BrepConstructor::start_edge`]                     | `SmBrepConstructor::StartEdge`                   |
//! | [`BrepConstructor::set_limits`]                     | `SmBrepConstructor::SetLimits`                   |
//! | [`BrepConstructor::end_edge`]                       | `SmBrepConstructor::EndEdge`                     |
//! | [`BrepConstructor::set_face_surface`]               | `SmBrepConstructor::SetFaceSurface`              |
//! | [`BrepConstructor::set_edge_curve`]                 | `SmBrepConstructor::SetEdgeCurve`                |
//! | [`BrepConstructor::start_vertex`]                   | `SmBrepConstructor::StartVertex`                 |
//! | [`BrepConstructor::end_vertex`]                     | `SmBrepConstructor::EndVertex`                   |
//! | [`BrepConstructor::swap_regions`]                   | `SmBrepConstructor::SwapRegions`                 |
//! | [`BrepConstructor::is_manifold`]                    | `SmBrepConstructor::IsManifold`                  |
//!
//! Deviations:
//! - **Ownership.** The constructor owns the B-rep it builds (upstream `m_pBrep`, which
//!   the caller also holds): [`BrepConstructor::start_brep`] returns it mutably,
//!   [`BrepConstructor::brep`] / [`BrepConstructor::brep_mut`] reach it while it is
//!   open, and [`BrepConstructor::end_brep`] hands it over (upstream sets `m_pBrep` to
//!   `NULL`); [`BrepConstructor::into_brep`] takes it whatever the state. The current
//!   region, shells, face, loop, edge, edge use and vertex (`m_p*`) are ids into it;
//!   `GetContext` is the caller's [`Kernel`], passed to the calls that use it.
//! - **Statuses.** A `NULL` return is an error: `SM_ERR` where upstream signals `SE(SM_ERR)`
//!   and `SM_ERR_NULL_POINTER` for `NERN(NULL)`. A state check upstream does not make
//!   before dereferencing (`SetFaceSurface` without an open face, `StartVertexOfLoop` /
//!   `StartVertex` without a B-rep, `SetEdgeCurve` on an edge use without face, a null
//!   loop / edge use / vertex use link in `EndLoop` and `StartEdge`, a missing 3D curve
//!   in `EndLoop`) is [`SolidError::InvalidInput`]. `SE` statuses (`FindRadialSector`,
//!   `AddOrientedEUPair`) are ignored; [`SolidError::Unported`] always propagates.
//! - **Notifications.** The new objects carry no attributes and no notification is
//!   delivered for them (as in the other topology ports): `SmVertex::SetPoint`'s
//!   `SM_NO_CHANGE_GEOMETRY` (a new vertex has no vertex uses yet) and the
//!   `SM_NO_CHANGE_OWNER` of `SetOwner` on a new curve. The temporary edge of a
//!   non-manifold join is given its curve and deleted through [`edge::set_curve`] and
//!   [`edge::destroy`] with a records-only environment and no context. Geometry owners
//!   are the B-rep's notification identities ([`crate::topology::NamingScheme::Notify`],
//!   as [`p3::new_brep`] sets).
//! - **`EndShell(TRUE)`** first calls `SmBrep::ValidatePointers`
//!   ([`crate::topology::brep::validation::validate_pointers`], unit
//!   `topology.brep_validation`), whose status upstream ignores but whose
//!   `sm_CheckEdgeuseGeometry` makes the missing UV trim curves of the faces that have
//!   some (through `SmEdgeuse::NormalizedEvaluate`).
//! - **`SetFaceSurface`** with a surface a face already owns makes upstream copy it as a
//!   bare `SmSurface` (`new SmSurface(*pSurface)` slices the object); the port returns
//!   [`SolidError::InvalidInput`] instead.
//! - **`SetLimits`** propagates a failed `EvaluatePoint` of the snap test (upstream
//!   ignores the status and compares an uninitialised point).
//! - **`SetEdgeCurve`** lifts a UV curve on a copy of the face surface without owner
//!   (upstream clears the surface's owner for the call "to prevent trimmed surface
//!   cache"); when `LiftCurve` fails, upstream returns before restoring the owner, so
//!   the face surface is left without owner, as here.
//! - **Skeleton fields.** [`crate::topology::Face::shell`] is the shell the face's first
//!   placed face use joins, the shell's `faces` list gains each face once,
//!   [`crate::topology::Face::loops`] lists the face's loops, `Loop::edges` stays empty
//!   (as [`super::brep::p10`]), and [`crate::topology::Edge::vertices`] follow the vertex
//!   uses of the primary edge use and its mate once known (vertex id 0 until then).
//! - **No recursion**; every loop is bounded by the B-rep's records.

use crate::curve::curve::p2::BSPLINE_CURVE_TYPE;
use crate::curve::curve::Curve;
use crate::error::{ignored, EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::USE_DEFAULT;
use crate::geom::vector2d::Point2;
use crate::heal::stitch::p1::{DefaultStitchCallback, Stitch};
use crate::heal::stitch::p2::MinGapRequest;
use crate::kernel::Kernel;
use crate::math::{Point3, UNDEF_DOUBLE};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::Surface;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3;
use crate::topology::edge::{self, SetCurveOptions};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p4 as face4;
use crate::topology::loopuse::{self, notify_ref};
use crate::topology::shell::{self, ShellType};
use crate::topology::{
    face, Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId,
    Loop, LoopId, Loopuse, LoopuseId, LoopuseStart, Region, RegionId, Shell, ShellId, Vertex,
    VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};

const OP: &str = "topology::brep_constructor";

/// `SM_UNDEF_ULONG`: the node type of an empty [`VertexNode`].
pub const UNDEF_NODE_TYPE: u64 = 123_456_789;

/// A bound of an edge's curve (`SmVertexNode`): a curve parameter, or a 3D or UV point
/// that is dropped on the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VertexNode {
    /// `m_lType`: `0` 3D point, `1` curve parameter, `2` UV point.
    pub node_type: u64,
    /// `m_dParameter`.
    pub parameter: f64,
    /// `m_vPoint` (a UV point has `z = 0`).
    pub point: Point3,
}

impl Default for VertexNode {
    /// `SmVertexNode()`: type `SM_UNDEF_ULONG`, parameter `SM_UNDEF_DOUBLE`, the origin.
    fn default() -> Self {
        Self {
            node_type: UNDEF_NODE_TYPE,
            parameter: UNDEF_DOUBLE,
            point: Point3::new(0.0, 0.0, 0.0),
        }
    }
}

impl VertexNode {
    /// `SmVertexNode(double)`: a curve parameter.
    #[must_use]
    pub const fn from_parameter(parameter: f64) -> Self {
        Self {
            node_type: 1,
            parameter,
            point: Point3::new(0.0, 0.0, 0.0),
        }
    }

    /// `SmVertexNode(const SmPoint3d&)`: a 3D point.
    #[must_use]
    pub const fn from_point(point: Point3) -> Self {
        Self {
            node_type: 0,
            parameter: 0.0,
            point,
        }
    }

    /// `SmVertexNode(const SmPoint2d&)`: a UV point (`z = 0`).
    #[must_use]
    pub const fn from_uv_point(uv: Point2) -> Self {
        Self {
            node_type: 2,
            parameter: 0.0,
            point: Point3::new(uv.x, uv.y, 0.0),
        }
    }
}

/// The construction state (`SmBrepConstructor`): the B-rep being built and the open
/// region, shells, face, loop, edge and vertex.
#[derive(Debug, Clone)]
pub struct BrepConstructor {
    /// `m_pBrep`.
    brep: Option<Brep>,
    /// `m_pRegion`.
    region: Option<RegionId>,
    /// `m_pOuterRegion`.
    outer_region: Option<RegionId>,
    /// `m_pOuterShell`.
    outer_shell: Option<ShellId>,
    /// `m_pInnerShell`.
    inner_shell: Option<ShellId>,
    /// `m_pFace`.
    face: Option<FaceId>,
    /// `m_pLoop`.
    loop_: Option<LoopId>,
    /// `m_pEdge`.
    edge: Option<EdgeId>,
    /// `m_pEdgeuse`.
    edgeuse: Option<EdgeuseId>,
    /// `m_pVertex`.
    vertex: Option<VertexId>,
    /// `m_bIsManifold`.
    is_manifold: bool,
    /// `m_eLoopSurfaceOrientation`: the orientation of the open loop to the surface.
    loop_surface_orientation: OrientType,
}

impl Default for BrepConstructor {
    fn default() -> Self {
        Self::new()
    }
}

/// The B-rep, or the error upstream's dereference of a `NULL` `m_pBrep` becomes.
fn open<'a>(brep: &'a mut Option<Brep>, what: &'static str) -> Result<&'a mut Brep> {
    brep.as_mut().ok_or(SolidError::invalid_input(OP, what))
}

fn null(what: &'static str) -> SolidError {
    SolidError::invalid_input(OP, what)
}

/// `pRegion->PostInsert(pShell)`.
fn post_insert_shell(b: &mut Brep, region: RegionId, s: ShellId) -> Result<()> {
    b.region_mut(region)?.shells.push(s);
    b.shell_mut(s)?.region = Some(region);
    Ok(())
}

/// `pShell->PostInsert(pFU)`, keeping the skeleton's face lists.
fn post_insert_faceuse(b: &mut Brep, s: ShellId, fu: FaceuseId) -> Result<()> {
    let face = b.faceuse(fu)?.face;
    let mate_on_shell = match b.faceuse(fu)?.mate {
        Some(m) => b.faceuse(m)?.shell.is_some(),
        None => false,
    };
    shell::post_insert_faceuse(b, s, fu)?;
    if let Some(f) = face {
        let rec = b.shell_mut(s)?;
        if !rec.faces.contains(&f) {
            rec.faces.push(f);
        }
        if !mate_on_shell {
            b.face_mut(f)?.shell = s;
        }
    }
    Ok(())
}

/// `pFU->PostInsert(pLU)`.
fn post_insert_loopuse(b: &mut Brep, fu: FaceuseId, lu: LoopuseId) -> Result<()> {
    b.faceuse_mut(fu)?.loopuses.push(lu);
    b.loopuse_mut(lu)?.faceuse = Some(fu);
    Ok(())
}

/// `pE->PostInsert(pEU)`.
fn post_insert_edgeuse(b: &mut Brep, e: EdgeId, eu: EdgeuseId) -> Result<()> {
    b.edge_mut(e)?.edgeuses.push(eu);
    b.edgeuse_mut(eu)?.edge = Some(e);
    Ok(())
}

/// `pV->PostInsert(pVU)`.
fn post_insert_vertexuse(b: &mut Brep, v: VertexId, vu: VertexuseId) -> Result<()> {
    b.vertex_mut(v)?.vertexuses.push(vu);
    b.vertexuse_mut(vu)?.vertex = Some(v);
    Ok(())
}

/// `pE->Remove(pEU)` (status not checked upstream): takes the edge use off the list.
fn remove_edgeuse(b: &mut Brep, e: EdgeId, eu: EdgeuseId) -> Result<()> {
    if b.edgeuse(eu)?.edge == Some(e) {
        b.edge_mut(e)?.edgeuses.retain(|&x| x != eu);
        b.edgeuse_mut(eu)?.edge = None;
    }
    Ok(())
}

/// The skeleton's vertex pair of `e` from the vertex uses of its primary edge use and
/// that use's mate (start first), where both are known.
fn sync_edge_vertices(b: &mut Brep, e: EdgeId) -> Result<()> {
    let Some(&prim) = b.edge(e)?.edgeuses.first() else {
        return Ok(());
    };
    let Some(mate) = ignored(eu1::mate(b, prim))?.flatten() else {
        return Ok(());
    };
    let vertex_of = |eu: EdgeuseId| -> Result<Option<VertexId>> {
        Ok(match b.edgeuse(eu)?.vertexuse {
            Some(vu) => b.vertexuse(vu)?.vertex,
            None => None,
        })
    };
    let (p, m) = (vertex_of(prim)?, vertex_of(mate)?);
    let (start, end) = if b.edgeuse(prim)?.orientation == OrientType::Same {
        (p, m)
    } else {
        (m, p)
    };
    let rec = b.edge_mut(e)?;
    if let Some(v) = start {
        rec.vertices[0] = v;
    }
    if let Some(v) = end {
        rec.vertices[1] = v;
    }
    Ok(())
}

/// `pCurve->DropPoint(GetNaturalInterval(), point, NULL, tol, NULL, ...)` (default
/// `SM_SO_MINIMIZE`), retried with `100 * tol` when no point is found; then the
/// dropped parameter snaps to the natural interval end `at_max` selects when its
/// curve point lies within the edge's zone tolerance of that end's.
fn drop_limit(
    k: &Kernel,
    curve: &Curve,
    point: Point3,
    tol: f64,
    zone_tol: f64,
    at_max: bool,
) -> Result<f64> {
    let natural = curve.natural_interval();
    let mut dropped = curve.drop_point(
        k,
        &natural,
        point,
        None,
        tol,
        None,
        SolverOperation::Minimize,
    )?;
    if !dropped.success {
        dropped = curve.drop_point(
            k,
            &natural,
            point,
            None,
            tol * 100.0,
            None,
            SolverOperation::Minimize,
        )?;
        if !dropped.success {
            return Err(SolidError::sm_err(OP));
        }
    }
    let mut t = dropped.parameter;

    // snap to the curve interval end when the 3d point is within tol
    // Accounts for dropped point falling on wrong side of seam
    let end = if at_max { natural.max() } else { natural.min() };
    let edge_end = curve.evaluate_point(k, t)?;
    let curve_end = curve.evaluate_point(k, end)?;
    if edge_end.distance_squared(curve_end) < zone_tol * zone_tol {
        t = end;
    }
    Ok(t)
}

impl BrepConstructor {
    /// `SmBrepConstructor()`: nothing open, manifold, loop orientation unknown.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            brep: None,
            region: None,
            outer_region: None,
            outer_shell: None,
            inner_shell: None,
            face: None,
            loop_: None,
            edge: None,
            edgeuse: None,
            vertex: None,
            is_manifold: true,
            loop_surface_orientation: OrientType::Unknown,
        }
    }

    /// The B-rep being built (`m_pBrep`), `None` before [`BrepConstructor::start_brep`]
    /// and after [`BrepConstructor::end_brep`].
    #[must_use]
    pub const fn brep(&self) -> Option<&Brep> {
        self.brep.as_ref()
    }

    /// The B-rep being built, mutably.
    pub fn brep_mut(&mut self) -> Option<&mut Brep> {
        self.brep.as_mut()
    }

    /// Takes the B-rep being built, whatever the construction state.
    #[must_use]
    pub fn into_brep(self) -> Option<Brep> {
        self.brep
    }

    /// `IsManifold`: false once an edge was joined by a third face.
    #[must_use]
    pub const fn is_manifold(&self) -> bool {
        self.is_manifold
    }

    /// `SwapRegions`: exchanges the open region and the outer region.
    pub fn swap_regions(&mut self) {
        core::mem::swap(&mut self.outer_region, &mut self.region);
    }

    /// `StartBrep`: starts a new B-rep (`SmBrep()` with default tolerance settings);
    /// `model_size_estimate` is not used.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` while a B-rep is open; those of [`p3::new_brep`].
    pub fn start_brep(&mut self, k: &mut Kernel, model_size_estimate: f64) -> Result<&mut Brep> {
        let _ = model_size_estimate; // SM_REF1: not used
        if self.brep.is_some() {
            return Err(SolidError::null_pointer(OP));
        }
        let b = p3::new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        Ok(self.brep.insert(b))
    }

    /// `EndBrep`: ends the B-rep and hands it over.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without B-rep or while a region, shell, face or loop is open.
    pub fn end_brep(&mut self) -> Result<Brep> {
        // Make sure the Brep is active and everything else has been shut down within
        // the brep using the 'End**' command.
        if self.brep.is_none()
            || self.region.is_some()
            || self.outer_shell.is_some()
            || self.inner_shell.is_some()
            || self.face.is_some()
            || self.loop_.is_some()
        {
            return Err(SolidError::sm_err(OP));
        }
        self.brep.take().ok_or(SolidError::sm_err(OP))
    }

    /// `StartRegion`: a new region (a solid) whose shells will bound it from
    /// `outer_region` (`None`: the infinite region). `is_solid_region` is not used.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without B-rep or while a region is open.
    pub fn start_region(
        &mut self,
        is_solid_region: bool,
        outer_region: Option<RegionId>,
    ) -> Result<RegionId> {
        let _ = is_solid_region; // not used
                                 // Make sure the brep is open and there are no open regions.
        if self.region.is_some() {
            return Err(SolidError::null_pointer(OP));
        }
        let Some(b) = self.brep.as_mut() else {
            return Err(SolidError::null_pointer(OP));
        };
        let r = b.add_region_record(Region::default())?;
        b.post_insert_region(r)?;
        self.region = Some(r);
        self.outer_region = outer_region;
        Ok(r)
    }

    /// `EndRegion`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open region or while a shell is open.
    pub fn end_region(&mut self) -> Result<()> {
        // Make sure the region is open and all contained shells are closed
        if self.region.is_none() || self.outer_shell.is_some() || self.inner_shell.is_some() {
            return Err(SolidError::sm_err(OP));
        }
        self.region = None;
        self.outer_region = None;
        Ok(())
    }

    /// `StartShell`: a new face use shell in the outer region (the infinite region when
    /// none was given), and, when a region is open, its inner shell in that region.
    /// Returns `(rpOuterShell, rpInnerShell)`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without B-rep or while a shell is open; [`SolidError::InvalidInput`] for
    /// a B-rep without infinite region (upstream dereferences it).
    pub fn start_shell(&mut self) -> Result<(ShellId, Option<ShellId>)> {
        if self.outer_shell.is_some() || self.inner_shell.is_some() {
            return Err(SolidError::sm_err(OP));
        }
        let Some(b) = self.brep.as_mut() else {
            return Err(SolidError::sm_err(OP));
        };

        let outer = b.add_shell_record(Shell::default())?;
        b.shell_mut(outer)?
            .set_shell_type(ShellType::Faceuse.code());
        self.outer_shell = Some(outer);

        // If m_pOuterRegion is NULL then everything goes into the infinite region.
        if self.outer_region.is_none() {
            self.outer_region = b.infinite_region();
        }
        let outer_region = self
            .outer_region
            .ok_or(null("B-rep without infinite region"))?;

        // Test for open shell case
        post_insert_shell(b, outer_region, outer)?;

        self.inner_shell = None;
        let Some(region) = self.region else {
            return Ok((outer, None));
        };
        let inner = b.add_shell_record(Shell::default())?;
        b.shell_mut(inner)?
            .set_shell_type(ShellType::Faceuse.code());
        post_insert_shell(b, region, inner)?;
        self.inner_shell = Some(inner);
        Ok((outer, Some(inner)))
    }

    /// `EndShell`: ends the shell; with `stitch_shell` first stitches the vertices and
    /// edges of its faces (`SmStitch` at the B-rep tolerance, no region nesting,
    /// validating).
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open shell or while a face is open; those of the stitching.
    pub fn end_shell(&mut self, k: &mut Kernel, stitch_shell: bool) -> Result<()> {
        let Some(outer) = self.outer_shell else {
            return Err(SolidError::sm_err(OP));
        };
        if self.face.is_some() {
            return Err(SolidError::sm_err(OP));
        }

        if stitch_shell {
            let b = open(&mut self.brep, "EndShell: no B-rep")?;
            validate_pointers(k, b)?;
            let mut shell_edges: Vec<EdgeId> = Vec::new();
            let mut shell_vertices: Vec<VertexId> = Vec::new();
            for fu in b.shell(outer)?.get_faceuses()? {
                let f = b.faceuse(fu)?.face.ok_or(null("face use without face"))?;
                shell_edges.extend(face4::edges(b, f, None, false, None)?);
                shell_vertices.extend(face4::vertices(b, f, None, false, None)?);
            }
            let mut callback = DefaultStitchCallback;
            let tol = b.tolerance();
            let mut stitch = Stitch::new(&mut callback, tol, false, true);
            stitch.do_stitching(
                k,
                b,
                Some(&shell_vertices),
                Some(&shell_edges),
                MinGapRequest::default(),
            )?;
        }

        self.outer_shell = None;
        self.inner_shell = None;
        Ok(())
    }

    /// `StartFace`: a new face and its two face uses; the upward one (same orientation
    /// as the surface) goes on the outer shell when `outside_faceuse` is
    /// [`OrientType::Same`] (the surface normal points out of the solid), the other on
    /// the inner shell (on the outer shell too for an open shell).
    ///
    /// # Errors
    ///
    /// `SM_ERR` without B-rep or open shell.
    pub fn start_face(&mut self, outside_faceuse: OrientType) -> Result<FaceId> {
        let (Some(b), Some(outer)) = (self.brep.as_mut(), self.outer_shell) else {
            return Err(SolidError::sm_err(OP));
        };

        // (the skeleton shell is set when a face use joins one)
        let f = b.add_face_record(Face::new(outer))?;
        let tol = b.tolerance();
        b.face_mut(f)?.tolerance = tol;
        self.face = Some(f);

        let fu1 = b.add_faceuse_record(Faceuse::default())?;
        let fu2 = b.add_faceuse_record(Faceuse::default())?;

        // Connect faceuses with face, faceuses to each other, set orientations
        *b.faceuse_mut(fu1)? = Faceuse {
            face: Some(f),
            mate: Some(fu2),
            orientation: OrientType::Same,
            ..Faceuse::default()
        };
        *b.faceuse_mut(fu2)? = Faceuse {
            face: Some(f),
            mate: Some(fu1),
            orientation: OrientType::Opposite,
            ..Faceuse::default()
        };
        b.face_mut(f)?.faceuse = Some(fu1); // set pFU1 as the upward faceuse

        // Connect up to shells
        if outside_faceuse == OrientType::Same {
            post_insert_faceuse(b, outer, fu1)?;
            post_insert_faceuse(b, self.inner_shell.unwrap_or(outer), fu2)?;
        } else {
            post_insert_faceuse(b, self.inner_shell.unwrap_or(outer), fu1)?;
            post_insert_faceuse(b, outer, fu2)?;
        }
        Ok(f)
    }

    /// `EndFace`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open face or while a loop is open.
    pub fn end_face(&mut self) -> Result<()> {
        if self.face.is_none() || self.loop_.is_some() {
            return Err(SolidError::sm_err(OP));
        }
        self.face = None;
        Ok(())
    }

    /// `StartAndEndSingleVertexLoop`: a vertex loop at a new vertex at `point` in the
    /// open face.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without B-rep or open face.
    pub fn start_and_end_single_vertex_loop(&mut self, point: Point3) -> Result<LoopId> {
        if self.brep.is_none() || self.face.is_none() {
            return Err(SolidError::sm_err(OP));
        }
        let l = self.start_loop(OrientType::Same)?; // default behavior
        let b = open(&mut self.brep, "no B-rep")?;
        let lu1 = b.loop_(l)?.loopuse.ok_or(null("loop without loop use"))?;
        let lu2 = b.loopuse(lu1)?.mate.ok_or(null("loop use without mate"))?;

        let vu1 = b.add_vertexuse_record(Vertexuse::default())?;
        let vu2 = b.add_vertexuse_record(Vertexuse::default())?;
        let tol = b.tolerance();
        // Set Vertex data; add vertex to brep
        let v = b.add_vertex_record(Vertex {
            point,
            tolerance: tol,
            ..Vertex::default()
        })?;

        // Connect vertex and vertexuses
        post_insert_vertexuse(b, v, vu1)?;
        post_insert_vertexuse(b, v, vu2)?;

        // Connect vertexuses to loop
        b.loopuse_mut(lu1)?.start = LoopuseStart::Vertexuse(Some(vu1));
        b.loopuse_mut(lu2)?.start = LoopuseStart::Vertexuse(Some(vu2));
        b.vertexuse_mut(vu1)?.owner = VertexuseOwner::Loopuse(Some(lu1));
        b.vertexuse_mut(vu2)?.owner = VertexuseOwner::Loopuse(Some(lu2));

        self.loop_ = None;
        Ok(l)
    }

    /// `StartVertexOfLoop`: a new vertex at `point` on the B-rep's vertex list.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without B-rep (upstream dereferences it).
    pub fn start_vertex_of_loop(&mut self, point: Point3) -> Result<VertexId> {
        let b = open(&mut self.brep, "StartVertexOfLoop: no B-rep")?;
        let tolerance = b.tolerance();
        b.add_vertex_record(Vertex {
            point,
            tolerance,
            ..Vertex::default()
        })
    }

    /// `StartLoop`: a new loop in the open face. The first loop is the outer loop;
    /// `orientation_to_surface` tells whether the edges that follow run
    /// counter-clockwise ([`OrientType::Same`]) or clockwise about the surface normal.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without B-rep or open face; [`SolidError::InvalidInput`] for a face
    /// without its face uses.
    pub fn start_loop(&mut self, orientation_to_surface: OrientType) -> Result<LoopId> {
        let (Some(b), Some(f)) = (self.brep.as_mut(), self.face) else {
            return Err(SolidError::sm_err(OP));
        };

        self.edge = None;
        let l = b.add_loop_record(Loop {
            face: f,
            edges: Vec::new(),
            loopuse: None,
        })?;
        self.loop_ = Some(l);
        let lu1 = b.add_loopuse_record(Loopuse::default())?;
        let lu2 = b.add_loopuse_record(Loopuse::default())?;

        // Connect up Loop and loopuses
        b.loopuse_mut(lu1)?.loop_ = Some(l);
        b.loopuse_mut(lu2)?.loop_ = Some(l);

        let fu1 = b.face(f)?.faceuse.ok_or(null("face without face use"))?;
        let fu2 = b.faceuse(fu1)?.mate.ok_or(null("face use without mate"))?;
        post_insert_loopuse(b, fu1, lu1)?; // connect pLU1 to upward faceuse
        post_insert_loopuse(b, fu2, lu2)?;
        b.face_mut(f)?.loops.push(l);
        // Primary loopuse is the one connected to upward faceuse
        b.loop_mut(l)?.loopuse = Some(lu1);

        // cache the loop orientation relative to the surface
        self.loop_surface_orientation = orientation_to_surface;
        // Set loop orientation: the first loopuse must be the outer one
        let orientation = if b.faceuse(fu1)?.loopuses.len() == 1 {
            OrientType::Same
        } else {
            OrientType::Opposite
        };
        b.loopuse_mut(lu1)?.orientation = orientation;
        b.loopuse_mut(lu2)?.orientation = orientation;

        // Connect loopuses to each other
        b.loopuse_mut(lu1)?.mate = Some(lu2);
        b.loopuse_mut(lu2)?.mate = Some(lu1);
        Ok(l)
    }

    /// `EndLoop`: ends the loop and gives each vertex use of its upward loop use (and
    /// the matching one of the downward loop use) a vertex: the one already on a
    /// neighbouring edge use, else a new vertex at the edge's end.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open loop or when an edge use's clockwise neighbour's mate is
    /// not its mate's counter-clockwise neighbour; [`SolidError::InvalidInput`] for a
    /// null link upstream dereferences; those of `IsClosed` and `EvaluatePoint`.
    pub fn end_loop(&mut self, k: &Kernel) -> Result<()> {
        let Some(l) = self.loop_ else {
            return Err(SolidError::sm_err(OP));
        };
        let b = open(&mut self.brep, "EndLoop: no B-rep")?;

        // Here we need to loop through and create vertices for each vertexuse.
        let lu = b.loop_(l)?.loopuse.ok_or(null("loop without loop use"))?;
        let vertexuses = loopuse::vertexuses(b, lu, None)?;

        for vu in vertexuses {
            let vu = vu.ok_or(null("EndLoop: null vertex use"))?;
            let VertexuseOwner::Edgeuse(Some(eu)) = b.vertexuse(vu)?.owner else {
                return Err(null("EndLoop: vertex use of no edge use"));
            };
            let e = eu1::edge(b, eu)?;
            let mate = eu1::mate(b, eu)?.ok_or(null("EndLoop: edge use without mate"))?;
            let mate_ccw = b.edgeuse(mate)?.ccw.ok_or(null("EndLoop: null CCW link"))?;
            let vu_mate_ccw = b
                .edgeuse(mate_ccw)?
                .vertexuse
                .ok_or(null("EndLoop: edge use without vertex use"))?;
            let radial = eu1::radial(b, eu)?;

            let mut vu_radial_ccw = None;
            if radial != Some(mate) {
                let r = radial.ok_or(null("EndLoop: edge use without radial"))?;
                let r_ccw = b.edgeuse(r)?.ccw.ok_or(null("EndLoop: null CCW link"))?;
                vu_radial_ccw = b.edgeuse(r_ccw)?.vertexuse;
            }

            let v = if let Some(v) = b.vertexuse(vu)?.vertex {
                v
            } else {
                let mut found = None;
                if let Some(v) = b.vertexuse(vu_mate_ccw)?.vertex {
                    found = Some(v);
                } else if let Some(v) = match vu_radial_ccw {
                    Some(x) => b.vertexuse(x)?.vertex,
                    None => None,
                } {
                    found = Some(v);
                } else if let Some(r) = radial {
                    let r_vu = b
                        .edgeuse(r)?
                        .vertexuse
                        .ok_or(null("EndLoop: edge use without vertex use"))?;
                    if let Some(rv) = b.vertexuse(r_vu)?.vertex {
                        let e = e.ok_or(null("EndLoop: edge use without edge"))?;
                        let rec = b.edge(e)?;
                        let curve = rec
                            .curve
                            .as_ref()
                            .ok_or(null("EndLoop: edge without curve"))?;
                        if curve.is_closed(k, &rec.interval, rec.tolerance)? {
                            found = Some(rv);
                        }
                    }
                }

                let v = if let Some(v) = found {
                    v
                } else {
                    let tolerance = b.tolerance();
                    let v = b.add_vertex_record(Vertex {
                        tolerance,
                        ..Vertex::default()
                    })?;
                    let e = e.ok_or(null("EndLoop: edge use without edge"))?;
                    let rec = b.edge(e)?;
                    let curve = rec
                        .curve
                        .as_ref()
                        .ok_or(null("EndLoop: edge without curve"))?;
                    let t = if b.edgeuse(eu)?.orientation == OrientType::Opposite {
                        rec.interval.max()
                    } else {
                        rec.interval.min()
                    };
                    let point = curve.evaluate_point(k, t)?;
                    b.vertex_mut(v)?.point = point;
                    v
                };
                post_insert_vertexuse(b, v, vu)?;
                v
            };

            // Now hook up vertexuse on the bottom loopuse
            if b.vertexuse(vu_mate_ccw)?.vertex.is_none() {
                post_insert_vertexuse(b, v, vu_mate_ccw)?;
            }
            for x in [eu, mate_ccw] {
                if let Some(e) = b.edgeuse(x)?.edge {
                    sync_edge_vertices(b, e)?;
                }
            }
            let cw = eu1::cw_edgeuse(b, eu)?.ok_or(null("EndLoop: null CW link"))?;
            let cw_mate = eu1::mate(b, cw)?;
            if cw_mate != Some(mate_ccw) {
                return Err(SolidError::sm_err(OP));
            }
        }

        self.loop_ = None;
        self.edge = None;
        self.loop_surface_orientation = OrientType::Unknown;
        Ok(())
    }

    /// `StartEdge`: a new edge use pair in the open loop, after the previous one in the
    /// loop's direction. `orientation_in_loop` is [`OrientType::Same`] when the loop
    /// runs along the edge's curve. With `existing_edge` the pair joins that edge (a
    /// third face makes the B-rep non-manifold: the pair goes into the radial sector of
    /// its face); else a new edge is made, and the optional start and end vertices get
    /// the pair's vertex uses: the start vertex goes to the use that runs with the curve,
    /// so they are the curve's start and end (upstream's comment says "relative to the
    /// loop orientation", which only holds for a loop and edge both [`OrientType::Same`]).
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open loop; [`SolidError::InvalidInput`] for a null link upstream
    /// dereferences; [`SolidError::NotFound`] for an id of no record; those of the
    /// non-manifold join.
    pub fn start_edge(
        &mut self,
        k: &mut Kernel,
        orientation_in_loop: OrientType,
        existing_edge: Option<EdgeId>,
        start_vertex: Option<VertexId>,
        end_vertex: Option<VertexId>,
    ) -> Result<EdgeId> {
        let Some(l) = self.loop_ else {
            return Err(SolidError::sm_err(OP));
        };
        let b = open(&mut self.brep, "StartEdge: no B-rep")?;
        let (mut start_vertex, mut end_vertex) = (start_vertex, end_vertex);

        let e = match existing_edge {
            None => {
                let tolerance = b.tolerance();
                b.add_edge_record(Edge {
                    tolerance,
                    ..Edge::new([VertexId::from_index(0); 2])
                })?
            }
            Some(e) => {
                b.edge(e)?;
                e
            }
        };
        let eu1_ = b.add_edgeuse_record(Edgeuse::default())?;
        let eu2_ = b.add_edgeuse_record(Edgeuse::default())?;
        let vu1 = b.add_vertexuse_record(Vertexuse::default())?;
        let vu2 = b.add_vertexuse_record(Vertexuse::default())?;

        // Hook up vertexuses and edgeuses
        b.edgeuse_mut(eu1_)?.vertexuse = Some(vu1);
        b.edgeuse_mut(eu2_)?.vertexuse = Some(vu2);
        b.vertexuse_mut(vu1)?.owner = VertexuseOwner::Edgeuse(Some(eu1_));
        b.vertexuse_mut(vu2)?.owner = VertexuseOwner::Edgeuse(Some(eu2_));

        // Set orientation
        let (o1, o2) = if orientation_in_loop == self.loop_surface_orientation {
            (OrientType::Same, OrientType::Opposite)
        } else {
            core::mem::swap(&mut start_vertex, &mut end_vertex);
            (OrientType::Opposite, OrientType::Same)
        };
        b.edgeuse_mut(eu1_)?.orientation = o1;
        b.edgeuse_mut(eu2_)?.orientation = o2;

        // Set Loopuse pointers (and the edgeuse and loopuse types)
        let lu = b.loop_(l)?.loopuse.ok_or(null("loop without loop use"))?;
        let lu_mate = b.loopuse(lu)?.mate.ok_or(null("loop use without mate"))?;
        b.edgeuse_mut(eu1_)?.owner = EdgeuseOwner::Loopuse(Some(lu));
        b.loopuse_mut(lu)?.start = LoopuseStart::Edgeuse(Some(eu1_));
        b.edgeuse_mut(eu2_)?.owner = EdgeuseOwner::Loopuse(Some(lu_mate));
        b.loopuse_mut(lu_mate)?.start = LoopuseStart::Edgeuse(Some(eu2_));

        // Set CCW and CW pointers
        let set_ccw = |b: &mut Brep, eu: EdgeuseId, x: Option<EdgeuseId>| -> Result<()> {
            b.edgeuse_mut(eu)?.ccw = x;
            Ok(())
        };
        let set_cw = |b: &mut Brep, eu: EdgeuseId, x: Option<EdgeuseId>| -> Result<()> {
            b.edgeuse_mut(eu)?.cw = x;
            Ok(())
        };
        if self.edge.is_none() {
            // first edge make loop closed and add other edgeuses later
            set_ccw(b, eu1_, Some(eu1_))?;
            set_cw(b, eu1_, Some(eu1_))?;
            set_ccw(b, eu2_, Some(eu2_))?;
            set_cw(b, eu2_, Some(eu2_))?;
        } else {
            let prev1 = self
                .edgeuse
                .ok_or(null("StartEdge: no previous edge use"))?;
            let prev2 =
                eu1::mate(b, prev1)?.ok_or(null("StartEdge: previous edge use without mate"))?;
            if self.loop_surface_orientation == OrientType::Same {
                // If we're walking the loop with the left hand rule for the upward side
                // of the surface
                let ccw = b
                    .edgeuse(prev1)?
                    .ccw
                    .ok_or(null("StartEdge: null CCW link"))?;
                set_ccw(b, eu1_, Some(ccw))?;
                set_cw(b, ccw, Some(eu1_))?;
                set_ccw(b, prev1, Some(eu1_))?;
                set_cw(b, eu1_, Some(prev1))?;

                let cw = b
                    .edgeuse(prev2)?
                    .cw
                    .ok_or(null("StartEdge: null CW link"))?;
                set_cw(b, eu2_, Some(cw))?;
                set_ccw(b, cw, Some(eu2_))?;
                set_cw(b, prev2, Some(eu2_))?;
                set_ccw(b, eu2_, Some(prev2))?;
            } else {
                // If we're walking the loop with the left hand rule for the downward side
                // of the surface
                let cw = b
                    .edgeuse(prev1)?
                    .cw
                    .ok_or(null("StartEdge: null CW link"))?;
                set_cw(b, eu1_, Some(cw))?;
                set_ccw(b, cw, Some(eu1_))?;
                set_cw(b, prev1, Some(eu1_))?;
                set_ccw(b, eu1_, Some(prev1))?;

                let ccw = b
                    .edgeuse(prev2)?
                    .ccw
                    .ok_or(null("StartEdge: null CCW link"))?;
                set_ccw(b, eu2_, Some(ccw))?;
                set_cw(b, ccw, Some(eu2_))?;
                set_ccw(b, prev2, Some(eu2_))?;
                set_cw(b, eu2_, Some(prev2))?;
            }
        }

        // Connect up edge and edgeuses.
        // Note that the first edgeuse has to be same orientation as the edge.
        if existing_edge.is_none() {
            if let Some(v) = start_vertex {
                post_insert_vertexuse(b, v, vu1)?;
            }
            if let Some(v) = end_vertex {
                post_insert_vertexuse(b, v, vu2)?;
            }
            if o1 == OrientType::Same {
                post_insert_edgeuse(b, e, eu1_)?;
                post_insert_edgeuse(b, e, eu2_)?;
            } else {
                // pEU2 is same orient
                post_insert_edgeuse(b, e, eu2_)?;
                post_insert_edgeuse(b, e, eu1_)?;
            }
        } else {
            let prim =
                eu1::primary_edgeuse(b, e)?.ok_or(null("StartEdge: edge without edge use"))?;
            let prim_mate = eu1::mate(b, prim)?.ok_or(null("StartEdge: edge use without mate"))?;
            let vertex_of = |b: &Brep, eu: EdgeuseId| -> Result<Option<VertexId>> {
                let vu = b
                    .edgeuse(eu)?
                    .vertexuse
                    .ok_or(null("StartEdge: edge use without vertex use"))?;
                Ok(b.vertexuse(vu)?.vertex)
            };

            // Attach new EUs to existing VUs
            let (to_prim, to_mate) = if b.edgeuse(prim)?.orientation == o1 {
                (vu1, vu2)
            } else {
                (vu2, vu1)
            };
            if let Some(v) = vertex_of(b, prim)? {
                post_insert_vertexuse(b, v, to_prim)?;
            }
            if let Some(v) = vertex_of(b, prim_mate)? {
                post_insert_vertexuse(b, v, to_mate)?;
            }

            // Find the Edgeuse sector for the new EU's
            let sector = if edge::is_lamina(b, e)? {
                prim_mate // Simple if manifold
            } else {
                // If not manifold, find the radial sector this face lives in.
                self.is_manifold = false;
                non_manifold_sector(k, b, e, eu1_, eu2_, o1)?.unwrap_or(prim_mate)
            };
            ignored(edge::add_oriented_eu_pair(b, e, eu1_, eu2_, Some(sector)))?;
        }
        sync_edge_vertices(b, e)?;

        self.edge = Some(e);
        self.edgeuse = Some(eu1_);
        Ok(e)
    }

    /// `SetLimits`: the open edge's interval from `start` and `end` (the curve's minimum
    /// and maximum ends): a parameter as given, a point dropped on the curve's natural
    /// interval (at the edge tolerance, else 100 times it) and snapped to the natural
    /// end when its curve point is within the edge tolerance of it.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open edge or edge curve, or when a point does not drop;
    /// `SM_ERR_INVALID_INPUT` (as [`SolidError::Upstream`] of `SetMinMax`) for a
    /// decreasing interval; those of `DropPoint` and `EvaluatePoint`.
    pub fn set_limits(&mut self, k: &Kernel, start: &VertexNode, end: &VertexNode) -> Result<()> {
        let (Some(b), Some(e)) = (self.brep.as_mut(), self.edge) else {
            return Err(SolidError::sm_err(OP));
        };
        let rec = b.edge(e)?;
        let Some(curve) = rec.curve.as_ref() else {
            return Err(SolidError::sm_err(OP));
        };
        let tol = rec.tolerance;
        // SmTol::GetZoneTol3d(m_pEdge): the edge's tolerance
        let zone_tol = rec.tolerance;

        // Set the limits on the current edge.
        let min = if start.node_type == 1 {
            start.parameter
        } else {
            drop_limit(k, curve, start.point, tol, zone_tol, false)?
        };
        let max = if end.node_type == 1 {
            end.parameter
        } else {
            drop_limit(k, curve, end.point, tol, zone_tol, true)?
        };
        b.edge_mut(e)?.interval.set_min_max(min, max)
    }

    /// `EndEdge`: nothing to do; the edge stays the loop's previous edge.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open edge.
    pub fn end_edge(&mut self) -> Result<()> {
        if self.edge.is_none() {
            return Err(SolidError::sm_err(OP));
        }
        Ok(())
    }

    /// `SetFaceSurface`: binds `surface` to the open face with UV domain `uv_domain`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without open face (upstream dereferences it) or
    /// for a surface a face already owns (see the module notes).
    pub fn set_face_surface(&mut self, mut surface: Surface, uv_domain: &Extent2d) -> Result<()> {
        let f = self.face.ok_or(null("SetFaceSurface: no open face"))?;
        let b = open(&mut self.brep, "SetFaceSurface: no B-rep")?;
        // copy surface when pSurface already has a Face owner
        if surface
            .owner()
            .is_some_and(|o| o.type_code == face::p2::TYPE)
        {
            return Err(null(
                "SetFaceSurface: surface owned by a face (upstream slices its copy)",
            ));
        }

        // set Face<->surface pointers
        let face_ref = notify_ref(EntityRef::Face(f));
        surface.set_owner(Some(face_ref));
        let rec = b.face_mut(f)?;
        rec.surface = Some(surface);
        rec.uv_domain = *uv_domain;
        b.set_geometry_owner_key(EntityRef::Face(f), Some(face_ref));
        b.sync_face_surface_tolerance(f)
    }

    /// `SetEdgeCurve`: binds a curve to the open edge. A 3D curve (`uv_curve` false)
    /// becomes the edge's curve; a UV curve becomes the UV trim curve of the open edge
    /// use and, when the edge has no curve yet, is lifted on the face surface (at the
    /// edge tolerance, continuity checks skipped) to make it.
    ///
    /// # Errors
    ///
    /// `SM_ERR` without open edge or edge use, or for a 3D curve on an edge that has
    /// one; `SM_ERR_NULL_POINTER` for a UV curve that is not a B-spline curve kind;
    /// [`SolidError::InvalidInput`] for a missing curve or face surface (upstream
    /// dereferences them); those of `LiftCurve`.
    pub fn set_edge_curve(
        &mut self,
        k: &Kernel,
        curve: Option<Curve>,
        uv_curve: bool,
    ) -> Result<()> {
        let (Some(b), Some(e), Some(eu)) = (self.brep.as_mut(), self.edge, self.edgeuse) else {
            return Err(SolidError::sm_err(OP));
        };
        let f = eu1::face(b, eu)?.ok_or(null("SetEdgeCurve: edge use without face"))?;

        if !uv_curve {
            if b.edge(e)?.curve.is_some() {
                // Don't need to set curve of this edge it has already been done.
                return Err(SolidError::sm_err(OP));
            }
            let mut curve = curve.ok_or(null("SetEdgeCurve: no curve"))?;
            let edge_ref = notify_ref(EntityRef::Edge(e));
            let _ = curve.set_owner(Some(edge_ref), None);
            b.edge_mut(e)?.curve = Some(curve);
            b.set_geometry_owner_key(EntityRef::Edge(e), Some(edge_ref));
            return Ok(());
        }

        // m_pUVTrimCurve = SM_CAST_PTR(SmBSplineCurve, pCurve), owned by the edge use
        let natural = curve.as_ref().map(Curve::natural_interval);
        let uv = curve
            .map(|mut c| {
                let _ = c.set_owner(Some(notify_ref(EntityRef::Edgeuse(eu))), None);
                c
            })
            .filter(|c| c.is_kind_of(BSPLINE_CURVE_TYPE));
        b.edgeuse_mut(eu)?.uv_trim_curve = uv;
        let (Some(uv), Some(natural)) = (b.edgeuse(eu)?.uv_trim_curve.as_ref(), natural) else {
            return Err(SolidError::null_pointer(OP));
        };
        if b.edge(e)?.curve.is_some() {
            return Ok(());
        }

        let rec = b.face(f)?;
        let surface = rec
            .surface
            .as_ref()
            .ok_or(null("SetEdgeCurve: face without surface"))?;
        // pSurf->SetOwner(NULL): To prevent trimmed surface cache
        let mut ownerless = surface.clone();
        let _ = ownerless.set_owner(None);
        let tol = b.edge(e)?.tolerance;
        let lifted = ownerless.lift_curve(k, &rec.uv_domain, uv, &natural, tol, true);
        let lifted = match lifted {
            Ok(l) => l,
            Err(err) => {
                // (SER returns before pSurf->SetOwner(pFace))
                if let Some(s) = b.face_mut(f)?.surface.as_mut() {
                    let _ = s.set_owner(None);
                }
                return Err(err);
            }
        };
        let mut curve_3d = lifted.curve;
        let edge_ref = notify_ref(EntityRef::Edge(e));
        let _ = curve_3d.set_owner(Some(edge_ref), None);
        b.edge_mut(e)?.curve = Some(curve_3d);
        b.set_geometry_owner_key(EntityRef::Edge(e), Some(edge_ref));
        Ok(())
    }

    /// `StartVertex`: a new vertex at `point` on the B-rep's vertex list, the open
    /// vertex.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] without B-rep (upstream dereferences it).
    pub fn start_vertex(&mut self, point: Point3) -> Result<VertexId> {
        let v = self.start_vertex_of_loop(point)?;
        self.vertex = Some(v);
        Ok(v)
    }

    /// `EndVertex`.
    pub fn end_vertex(&mut self) {
        self.vertex = None;
    }
}

/// `m_pBrep->ValidatePointers()` with its defaults (no report list, `SM_LEVEL_0`, no
/// test requests, `SM_NO_WALK`): its status is ignored, but its
/// `sm_CheckEdgeuseGeometry` makes the missing UV trim curves of the faces that have
/// some, so it is not skipped. The constructor's B-rep names geometry owners by its
/// notification identities ([`p3::new_brep`]), the keys of the owner checks.
fn validate_pointers(k: &Kernel, b: &mut Brep) -> Result<()> {
    ignored(crate::topology::brep::validation::validate_pointers(
        k,
        b,
        None,
        0,
        false,
        &notify_ref,
    ))?;
    Ok(())
}

/// The radial sector of a non-manifold join: a temporary edge with a copy of `e`'s
/// curve and interval takes the new pair (same-oriented use first) while
/// `FindRadialSector` classifies its face; the pair is then taken off it and it is
/// deleted. `None` when no sector is found (status ignored upstream).
fn non_manifold_sector(
    k: &mut Kernel,
    b: &mut Brep,
    e: EdgeId,
    eu1_: EdgeuseId,
    eu2_: EdgeuseId,
    o1: OrientType,
) -> Result<Option<EdgeuseId>> {
    // Create a temporary edge
    let tolerance = b.tolerance();
    let temp = b.add_edge_record(Edge {
        tolerance,
        ..Edge::new([VertexId::from_index(0); 2])
    })?;
    let interval: Extent1d = b.edge(e)?.interval;
    edge::set_interval(b, temp, &interval)?;
    if o1 == OrientType::Same {
        post_insert_edgeuse(b, temp, eu1_)?;
        post_insert_edgeuse(b, temp, eu2_)?;
    } else {
        // pEU2 is same orient
        post_insert_edgeuse(b, temp, eu2_)?;
        post_insert_edgeuse(b, temp, eu1_)?;
    }
    let copy = b
        .edge(e)?
        .curve
        .as_ref()
        .ok_or(null("StartEdge: joined edge without curve"))?
        .copy(k)?;
    let mut state = ArenaNotifyState::new(b);
    {
        let mut env = state.env(None);
        edge::set_curve(
            b,
            temp,
            Some(copy),
            SetCurveOptions::default(),
            &mut env,
            [None, None],
        )?;
    }

    // This doesn't matter, as AddOrientedEUPair adjusts for either case
    let found = ignored(edge::find_radial_sector(
        k,
        b,
        e,
        eu1_,
        None,
        OrientType::Same,
        None,
    ))?;
    // (a coincident face use is only asserted against upstream)
    let sector = found.and_then(|s| s.radial_edgeuse);

    remove_edgeuse(b, temp, eu1_)?;
    remove_edgeuse(b, temp, eu2_)?;
    // SmObjDelete sCleanEdge(pTempEdge)
    state.seed(b);
    let mut env = state.env(None);
    edge::destroy(k, b, temp, &mut env)?;
    Ok(sector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::math::Vec3;
    use crate::surface::plane::Plane;
    use crate::topology::brep::p1 as bp1;

    fn line(a: Point3, b: Point3) -> Curve {
        Curve::from(Line::create_canonical(a, b - a))
    }

    /// A unit cube from six planar faces with shared edges (outward normals).
    fn cube(k: &mut Kernel) -> Brep {
        let p = |i: usize| {
            let x = (i & 1) as f64;
            let y = ((i >> 1) & 1) as f64;
            let z = ((i >> 2) & 1) as f64;
            Point3::new(x, y, z)
        };
        // faces as CCW vertex cycles seen from outside
        let faces: [[usize; 4]; 6] = [
            [0, 2, 3, 1], // z = 0
            [4, 5, 7, 6], // z = 1
            [0, 1, 5, 4], // y = 0
            [2, 6, 7, 3], // y = 1
            [0, 4, 6, 2], // x = 0
            [1, 3, 7, 5], // x = 1
        ];
        let mut c = BrepConstructor::new();
        c.start_brep(k, USE_DEFAULT).unwrap();
        c.start_region(true, None).unwrap();
        c.start_shell().unwrap();
        let vs: Vec<VertexId> = (0..8)
            .map(|i| c.start_vertex_of_loop(p(i)).unwrap())
            .collect();
        let mut edges: Vec<((usize, usize), EdgeId)> = Vec::new();
        for f in faces {
            let (a, b0, c0) = (p(f[0]), p(f[1]), p(f[3]));
            let frame = Axis2Placement::new(a, b0 - a, c0 - a);
            let plane = Surface::from(Plane::create_canonical(&frame).unwrap());
            c.start_face(OrientType::Same).unwrap();
            c.set_face_surface(plane, &Extent2d::from_bounds(-2.0, -2.0, 2.0, 2.0))
                .unwrap();
            c.start_loop(OrientType::Same).unwrap();
            for j in 0..4 {
                let (s, t) = (f[j], f[(j + 1) % 4]);
                let key = (s.min(t), s.max(t));
                let existing = edges.iter().find(|(x, _)| *x == key).map(|(_, e)| *e);
                let orient = if s < t {
                    OrientType::Same
                } else {
                    OrientType::Opposite
                };
                let e = c
                    .start_edge(k, orient, existing, Some(vs[key.0]), Some(vs[key.1]))
                    .unwrap();
                if existing.is_none() {
                    c.set_edge_curve(k, Some(line(p(key.0), p(key.1))), false)
                        .unwrap();
                    c.set_limits(
                        k,
                        &VertexNode::from_point(p(key.0)),
                        &VertexNode::from_point(p(key.1)),
                    )
                    .unwrap();
                    edges.push((key, e));
                }
                c.end_edge().unwrap();
            }
            c.end_loop(k).unwrap();
            c.end_face().unwrap();
        }
        c.end_shell(k, false).unwrap();
        c.end_region().unwrap();
        c.end_brep().unwrap()
    }

    #[test]
    fn cube_counts_and_closure() {
        let mut k = Kernel::default();
        let b = cube(&mut k);
        assert_eq!(bp1::faces(&b, None).unwrap().len(), 6);
        assert_eq!(bp1::edges(&b, None).unwrap().len(), 12);
        assert_eq!(bp1::vertices(&b, None).unwrap().len(), 8);
        for e in bp1::edges(&b, None).unwrap() {
            assert_eq!(b.edge(e).unwrap().edgeuses.len(), 4);
            let ivl = b.edge(e).unwrap().interval;
            assert!((ivl.max() - ivl.min() - 1.0).abs() < 1e-12);
        }
        assert!(crate::topology::brep::p2::is_manifold_solid(&b).unwrap());
    }

    #[test]
    fn state_errors() {
        let mut k = Kernel::default();
        let mut c = BrepConstructor::new();
        assert!(c.end_brep().is_err());
        assert!(c.start_shell().is_err());
        assert!(c.start_face(OrientType::Same).is_err());
        c.start_brep(&mut k, USE_DEFAULT).unwrap();
        assert!(c.start_brep(&mut k, USE_DEFAULT).is_err());
        assert!(c.start_loop(OrientType::Same).is_err());
        assert!(c.end_loop(&k).is_err());
        assert!(c.end_edge().is_err());
        assert!(c
            .set_face_surface(
                Surface::from(Plane::create_canonical(&Axis2Placement::default()).unwrap()),
                &Extent2d::new()
            )
            .is_err());
        c.start_shell().unwrap();
        assert!(c.start_shell().is_err());
        assert!(c.end_brep().is_err());
        c.end_shell(&mut k, false).unwrap();
        assert!(c.end_brep().is_ok());
        let _ = Vec3::new(0.0, 0.0, 0.0);
    }
}
