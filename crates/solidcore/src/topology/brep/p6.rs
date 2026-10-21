//! Edge copying, face topology construction and region-closure testing.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`copy_edges`] | `SmBrep::CopyEdges` |
//! | [`make_face_topology`], [`FaceTopologyOptions`], [`FaceTopology`] | `SmBrep::MakeFaceTopology` |
//! | [`make_face_topology_attempt`], [`FaceTopologyAttempt`] | same, retaining native status and partial outputs |
//! | `connect_edge_loops`, `find_sector`, `assign_uv_curves` (private) | its mark-locked edge-loop block, radial-sector recovery and `cpOptUVCurves` block |
//! | [`test_closure_split_region`] | `SmBrep::TestClosureSplitRegion(SmFaceuse*)` |
//! | [`test_closure_split_region_attempt`] | same, retaining native status and partial outputs |
//!
//! Deviations: pointers become arena ids; optional outputs become return values.
//! Edits preserve the upstream mutations preceding an error. A closed-shell split stops at
//! `CalculateBoundingBox` (`query.brep`) after its two `SmBrep::Notify` calls
//! ([`crate::topology::brep::p2::notify`] with the caller's [`NotifyEnv`], statuses
//! ignored; the new region gets its default `SmTopology` record when it is made, as
//! `new SmRegion`), rather than approximating the infinite-region ray casting and the
//! classification of the remaining faces, wires and shells.
//! `MakeFaceTopology`: the caller's [`Marks`] hold the objects' mark values (the
//! `SmNewMarkAndLock` block claims and releases a new mark, also on error). New objects
//! get their `SmTopology` record in the [`NotifyEnv`] under its identity, which also
//! names the face as its surface's owner. Kernel-bound construction borrows the live
//! context only at notification sites; external null or distinct contexts stay explicit.
//! `SmSurface::SetOwner`'s `SM_NO_CHANGE_OWNER`
//! is sent to the kernel's context under [`FaceTopologyOptions::surface_key`] (a caller
//! without surface identities passes `None`: not notified, and no surface argument in
//! the final `SM_NO_ADD_TO_BREP`). `new SmFace` joins the B-rep's face list at
//! allocation (part 1's `add_face_record`); upstream's later `PostInsert` is checked to
//! leave it last. A deleted shell (`delete pShell`, after `SM_NO_DESTRUCTION`) is reset
//! to `SmShell()` in its arena, off every list. The face's loops and its shell's faces
//! skeleton lists are kept as part 10 builds them. `cpOptUVCurves` is copied: upstream
//! edits the caller's curves in place and hands accepted ones to the edge uses. A
//! pointer upstream dereferences without a check (an input edge use without edge, mate,
//! shell or vertex, loop counts past the edge uses, loop vertices merged into the NULL
//! keeper shell of a face without edge loops) is [`SolidError::InvalidInput`]. A failed
//! radial-sector recovery completes the face and returns `SM_ERR`, as upstream.
//! The shell implementation comes from PR #154. Surface owner identities are
//! supplied by the caller, independently of arena indices. A null trimmed copy
//! returns a null-pointer error instead of the upstream dereference. There is no recursion;
//! the traverser uses its bounded, iterative implementation.
//! The copy notifications (`SM_NO_COPY` on each source edge and its vertices) run on a
//! `topology.brep_p2` arena state seeded from both B-reps: the callers keep no attribute
//! state, and both B-reps' objects are named by their arena identities, which overlap
//! across the two B-reps (an overlapping pair propagates nothing, as for an object
//! notified about itself). `SmEdge::Notify` and `SmVertex::Notify` pass `SM_NO_COPY`
//! straight to `SmTopology::Notify`, which is called directly (the source B-rep is only
//! read). They notify through the destination kernel's context, the only one given.
//! An attempt exposes native status separately for callers that ignore it;
//! missing dependencies and Rust representation failures remain outer errors.

use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::context::{MarkLock, MarkType, ALL_MARKS};
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::surface::surface::Surface;
use crate::topology::brep::p2::{self as brep_p2, NotifyArg};
use crate::topology::edge::{NotifyEnv, NotifyState};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::shell::{self, ShellMembers, ShellType};
use crate::topology::topology::Topology;
use crate::topology::topology_traverser::{self as tr, Entity, Marks};
use crate::topology::{
    Brep, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    Loopuse, LoopuseId, LoopuseStart, Region, RegionId, Shell, ShellId, VertexId, Vertexuse,
    VertexuseId, VertexuseOwner,
};

const OP: &str = "topology::brep::p6";

fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| SolidError::null_pointer(OP))
}

// The split's bare pointer dereferences have no native error status to ignore.
// Refuse missing arena links as representation errors, not synthetic SmStatus.
fn split_link<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| SolidError::invalid_input(OP, "unlinked topology in region split"))
}

/// Copy edges to independent wires in `destination`, in input order, returning the new
/// edges (`pNewEdges`). An empty list succeeds without inspecting the destination. Each
/// edge's trimmed NURBS copy is made into a wire edge by part 5's
/// `CreateWireEdgeFromCurve` over the copy's natural interval; then the source edge and
/// its start vertex (and its end vertex, when both edges have two) report the copy
/// (`Notify(SM_NO_COPY, pNew, pDestinationBrep, this)`, statuses ignored; see the
/// module's Deviations).
///
/// # Errors
///
/// Those of `CreateTrimmedNURBSCurve` and `CreateWireEdgeFromCurve` (`SER`); a
/// null-pointer error for a null curve copy or a missing vertex, which upstream
/// dereferences.
pub fn copy_edges(
    destination_kernel: &mut Kernel,
    source: &Brep,
    edges: &[EdgeId],
    destination: &mut Brep,
) -> Result<Vec<EdgeId>> {
    let mut result = Vec::new();
    for &edge in edges {
        let curve = required(crate::topology::edge::create_trimmed_nurbs_curve(
            destination_kernel,
            source,
            edge,
        )?)?;
        let interval = curve.natural_interval();
        let new_edge = crate::topology::brep::p5::create_wire_edge_from_curve(
            destination_kernel,
            destination,
            curve,
            interval,
        )?;

        // Notify Copy Edge - propogates attributes and reports copy
        let mut state = brep_p2::ArenaNotifyState::new(source);
        state.seed(destination);
        let data = [brep_p2::brep_ref(destination), brep_p2::brep_ref(source)];
        let mut notify_copy = |old: EntityRef, new: EntityRef| {
            use crate::topology::loopuse::notify_ref;
            crate::error::ignore_status(crate::topology::topology::notify(
                &mut state.records,
                &mut state.attributes,
                Some(&mut destination_kernel.context),
                NotifyOperation::Copy,
                notify_ref(old),
                Some(notify_ref(new)),
                Some(data[0]),
                Some(data[1]),
            ))
        };
        notify_copy(EntityRef::Edge(edge), EntityRef::Edge(new_edge))?;

        // Notify Copy Vertices - propogates attributes and reports copy
        let new_start = required(crate::topology::edge::start_vertex(destination, new_edge)?)?;
        let new_end = required(crate::topology::edge::other_vertex(
            destination,
            new_edge,
            Some(new_start),
        )?)?;
        let old_start = required(crate::topology::edge::start_vertex(source, edge)?)?;
        let old_end = required(crate::topology::edge::other_vertex(
            source,
            edge,
            Some(old_start),
        )?)?;
        notify_copy(EntityRef::Vertex(old_start), EntityRef::Vertex(new_start))?;
        if old_start != old_end && new_start != new_end {
            notify_copy(EntityRef::Vertex(old_end), EntityRef::Vertex(new_end))?;
        }

        // set optional output
        result.push(new_edge);
    }
    Ok(result)
}

/// Topology created by `MakeFaceTopology` when construction succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceTopology {
    /// Newly created face (`rpNewFace`).
    pub face: FaceId,
    /// `rpNewShell`: the shell created for an edgeless face, or the shell split off
    /// by `TestClosureSplitRegion`.
    pub shell: Option<ShellId>,
    /// `rpNewRegion`: new region when the face splits an existing region.
    pub region: Option<RegionId>,
}

/// Native status and outputs written by `MakeFaceTopology` before it returns.
#[derive(Debug)]
pub struct FaceTopologyAttempt {
    /// The native `SmStatus`, represented as success or [`SolidError::Upstream`]
    /// (`SM_ERR` after a radial-sector recovery failure, which still completes the face).
    pub status: Result<()>,
    /// `rpNewFace` once the face is allocated, even on error.
    pub face: Option<FaceId>,
    /// `rpNewShell` as last written, even on error.
    pub shell: Option<ShellId>,
    /// `rpNewRegion` as last written, even on error.
    pub region: Option<RegionId>,
}

/// Input loops, isolated vertices, and the upstream face-construction flags.
#[derive(Debug, Clone, Default)]
pub struct FaceTopologyOptions<'a> {
    /// Explicit target region; `None` selects the B-rep's infinite region.
    pub region: Option<RegionId>,
    /// Number of input edge uses in each loop, outer loop first.
    pub edge_loops: &'a [usize],
    /// Concatenated, ordered edge uses, one closed sequence per loop.
    pub edgeuses: &'a [EdgeuseId],
    /// Isolated loop vertices.
    pub loop_vertices: &'a [VertexId],
    /// Verify radial sectors; upstream default is false.
    pub verify_radial_sectors: bool,
    /// Calculate UV trims; upstream default is false.
    pub calculate_trims: bool,
    /// Optional UV curves (including null entries) for the trim stage. They are
    /// copied: upstream edits the caller's curves in place and hands accepted ones
    /// to the edge uses.
    pub uv_curves: Option<&'a [Option<crate::nurbs::b_spline_curve::BSplineCurve>]>,
    /// The identity the surface is notified under (`SM_NO_CHANGE_OWNER` when it gets
    /// or loses the new face as owner, and the `SM_NO_GET_SURFACE` argument of the
    /// final `SM_NO_ADD_TO_BREP`). `None` for a caller whose surfaces have no
    /// identity: the owner changes are then not notified and the final notification
    /// has no surface argument.
    pub surface_key: Option<ObjectKey>,
}

// A pointer upstream dereferences without a check: a missing link is a Rust
// representation error, never a native status.
fn deref<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| SolidError::invalid_input(OP, "unlinked topology in face construction"))
}

// `new (this) Sm*()`: every new topology object has its `SmTopology` part.
fn register(env: &mut NotifyState<'_>, entity: EntityRef) {
    env.records.entry((env.key_of)(entity).key).or_default();
}

// `pSurface->SetOwner(pNewOwner)` on the new face's surface, with the
// `SM_NO_CHANGE_OWNER` notification `SmSurface::SetOwner` sends when the new owner
// is NULL or a face and differs from the current one. `face_listed` is whether the
// face is on the B-rep's face list yet (`SM_NO_GET_BREP(pNewF)`).
fn set_surface_owner(
    k: &mut Kernel,
    brep: &mut Brep,
    env: &mut NotifyState<'_>,
    face: FaceId,
    owner: Option<ObjectRef>,
    surface_key: Option<ObjectKey>,
    face_listed: bool,
) -> Result<()> {
    let surface = brep
        .face(face)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "face without surface"))?;
    let old = surface.owner();
    if let Some(key) = surface_key.filter(|_| owner != old) {
        let this = ObjectRef {
            type_code: surface.type_code(),
            key,
        };
        let data2 = owner.and(face_listed.then(|| brep_p2::brep_ref(brep)));
        crate::geom::a_object::notify(
            env.attributes,
            Some(&mut k.context),
            NotifyOperation::ChangeOwner,
            this,
            owner,
            data2,
            old,
        )?;
    }
    if let Some(surface) = brep.face_mut(face)?.surface.as_mut() {
        surface.set_owner(owner);
    }
    // the surface reads the face's tolerance through GetFace() once it is owned again
    brep.sync_face_surface_tolerance(face)
}

// `delete pShell` of an emptied shell: `~SmShell` notifies `SM_NO_DESTRUCTION`
// and resets the type. An arena record cannot be freed, so it is reset to
// `SmShell()` (already off its region's list, so unreachable).
fn delete_shell(
    k: &mut Kernel,
    brep: &mut Brep,
    env: &mut NotifyState<'_>,
    shell: ShellId,
) -> Result<()> {
    let this = (env.key_of)(EntityRef::Shell(shell));
    env.with_env(k, |env| {
        ignore_status(crate::topology::topology::notify(
            env.records,
            env.attributes,
            env.context.as_deref_mut(),
            NotifyOperation::Destruction,
            this,
            Some(this),
            None,
            None,
        ))
    })?;
    *brep.shell_mut(shell)? = Shell::default();
    Ok(())
}

// `SER(pRegion->Remove(pShell))`: `SmOwningTopology::Remove`'s SM_ERR (the shell is
// not on the region's list, `InvalidInput` in the shell port) is the native status.
fn region_remove(brep: &mut Brep, region: RegionId, shell: ShellId) -> Result<()> {
    shell::region_remove_shell(brep, region, shell).map_err(|error| match error {
        SolidError::InvalidInput { .. } => SolidError::sm_err(OP),
        other => other,
    })
}

// `pV->PostInsert(pVU)`.
fn post_insert_vertexuse(brep: &mut Brep, vertex: VertexId, vu: VertexuseId) -> Result<()> {
    brep.vertex_mut(vertex)?.vertexuses.push(vu);
    brep.vertexuse_mut(vu)?.vertex = Some(vertex);
    Ok(())
}

// `pShell->PostInsert(pFU)`: the face use joins the tail of the shell's face-use list.
fn post_insert_faceuse(brep: &mut Brep, shell: ShellId, fu: FaceuseId) -> Result<()> {
    shell::post_insert_faceuse(brep, shell, fu)?;
    if let Some(face) = brep.faceuse(fu)?.face {
        let record = brep.shell_mut(shell)?;
        if !record.faces.contains(&face) {
            record.faces.push(face);
        }
    }
    Ok(())
}

/// Construct a face from ordered edge-use loops and isolated vertices
/// (`SmBrep::MakeFaceTopology`), returning the new face and the shell/region outputs.
/// `marks` holds the objects' mark values (the lock claims a new mark, as does the
/// region split); `env` names and notifies the new objects. A missing region or
/// surface fails before allocation (`SM_ERR_NULL_POINTER`).
///
/// # Errors
///
/// The native statuses of the construction (see [`make_face_topology_attempt`]);
/// [`SolidError::InvalidInput`] for loop counts past the input and links upstream
/// dereferences without a check; [`SolidError::Unported`] from the callees.
pub fn make_face_topology(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyEnv<'_>,
    surface: Option<Surface>,
    orientation: OrientType,
    options: &FaceTopologyOptions<'_>,
) -> Result<FaceTopology> {
    let attempt = make_face_topology_attempt(k, brep, marks, env, surface, orientation, options)?;
    attempt.status?;
    Ok(FaceTopology {
        face: deref(attempt.face)?,
        shell: attempt.shell,
        region: attempt.region,
    })
}

/// [`make_face_topology`], retaining the native status and the outputs written before
/// it returns: upstream sets `rpNewFace` as soon as the face is allocated, and goes on
/// past a failed radial-sector recovery (returning `SM_ERR` with a complete face).
/// On error, allocated records remain in `brep`, as upstream retains its partial
/// edits; callers must not treat an error as an atomic rollback.
///
/// # Errors
///
/// Unported dependencies and Rust representation failures (as
/// [`make_face_topology`]); native statuses are in [`FaceTopologyAttempt::status`].
pub fn make_face_topology_attempt(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyEnv<'_>,
    surface: Option<Surface>,
    orientation: OrientType,
    options: &FaceTopologyOptions<'_>,
) -> Result<FaceTopologyAttempt> {
    make_face_topology_attempt_with_state(
        k,
        brep,
        marks,
        &mut NotifyState::from_env(env),
        surface,
        orientation,
        options,
    )
}

pub(crate) fn make_face_topology_with_state(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    state: &mut NotifyState<'_>,
    surface: Option<Surface>,
    orientation: OrientType,
    options: &FaceTopologyOptions<'_>,
) -> Result<FaceTopology> {
    let attempt = make_face_topology_attempt_with_state(
        k,
        brep,
        marks,
        state,
        surface,
        orientation,
        options,
    )?;
    attempt.status?;
    Ok(FaceTopology {
        face: deref(attempt.face)?,
        shell: attempt.shell,
        region: attempt.region,
    })
}

fn make_face_topology_attempt_with_state(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyState<'_>,
    surface: Option<Surface>,
    orientation: OrientType,
    options: &FaceTopologyOptions<'_>,
) -> Result<FaceTopologyAttempt> {
    let mut attempt = FaceTopologyAttempt {
        status: Ok(()),
        face: None,
        shell: None,
        region: None,
    };
    let status = build_face(
        k,
        brep,
        marks,
        env,
        surface,
        orientation,
        options,
        &mut attempt,
    );
    match status {
        Ok(()) => Ok(attempt),
        Err(error @ SolidError::Upstream { .. }) => {
            attempt.status = Err(error);
            Ok(attempt)
        }
        Err(error) => Err(error),
    }
}

/// The new loop uses, one per input loop then one per loop vertex.
struct NewLoopuses {
    upper: Vec<LoopuseId>,
    lower: Vec<LoopuseId>,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
fn build_face(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyState<'_>,
    surface: Option<Surface>,
    orientation: OrientType,
    options: &FaceTopologyOptions<'_>,
    out: &mut FaceTopologyAttempt,
) -> Result<()> {
    // Check for region default.
    let region = options.region.or(brep.infinite_region());

    // Check input
    let region = required(region)?;
    brep.region(region)?;
    let surface = required(surface)?;

    // Get shell to contain new faceuses
    // If no Edgeuses given, create a new one.
    let shell = if let Some(&first) = options.edgeuses.first() {
        eu::shell(brep, first)?
    } else {
        let shell = brep.add_shell_record(Shell {
            shell_type: ShellType::Faceuse,
            ..Shell::default()
        })?;
        register(env, EntityRef::Shell(shell));
        // pRegion->PostInsert(pShell)
        brep.region_mut(region)?.shells.push(shell);
        brep.shell_mut(shell)?.region = Some(region);

        // This will also be the output shell.
        out.shell = Some(shell);
        Some(shell)
    };

    // Test orientation of the Edgeuses to see if everything is consistent
    // for every input edgeLoop
    let mut used = 0usize;
    for &count in options.edge_loops {
        // Upstream skips zero-sized loops after its release assertion.
        if count == 0 {
            continue;
        }
        let end = used
            .checked_add(count)
            .ok_or_else(|| SolidError::invalid_input(OP, "edge loop count overflow"))?;
        let loop_edges = options
            .edgeuses
            .get(used..end)
            .ok_or_else(|| SolidError::invalid_input(OP, "edge loop exceeds input edge uses"))?;
        let mut last = *deref(loop_edges.last())?;
        for &current in loop_edges {
            // Make sure that every edgeuse connects to the previous loop edgeuse
            // through a common vertex
            let mate = deref(eu::mate(brep, last)?)?;
            let current_vertex = deref(brep.edgeuse(current)?.vertexuse)?;
            let last_vertex = deref(brep.edgeuse(mate)?.vertexuse)?;
            // SE(INVALID_INPUT), not SER: inconsistent ordering only reports a
            // diagnostic upstream. Do not turn it into an early return here.
            let _connected =
                brep.vertexuse(current_vertex)?.vertex == brep.vertexuse(last_vertex)?.vertex;
            last = current;
        }
        used = end;
    }

    // Allocate new Face and Faceuses
    let shell = deref(shell)?;
    let face = brep.add_face_record(Face::new(shell))?;
    let fu1 = brep.add_faceuse_record(Faceuse::default())?;
    let fu2 = brep.add_faceuse_record(Faceuse::default())?;
    for entity in [
        EntityRef::Face(face),
        EntityRef::Faceuse(fu1),
        EntityRef::Faceuse(fu2),
    ] {
        register(env, entity);
    }

    // set outputs: the caller has access to the face in case of an error
    out.face = Some(face);

    // Connect face and faceuses
    brep.faceuse_mut(fu1)?.face = Some(face);
    brep.faceuse_mut(fu2)?.face = Some(face);
    brep.faceuse_mut(fu1)?.mate = Some(fu2);
    brep.faceuse_mut(fu2)?.mate = Some(fu1);

    // connect face to surface just temporarily to help with some radial sectoring
    // that will follow (the face is not on the face list yet).
    let face_ref = (env.key_of)(EntityRef::Face(face));
    let domain = surface.natural_uv_domain();
    brep.face_mut(face)?.surface = Some(surface);
    set_surface_owner(
        k,
        brep,
        env,
        face,
        Some(face_ref),
        options.surface_key,
        false,
    )?;
    brep.face_mut(face)?.uv_domain = domain;

    // Have to set the tolerance here, it might be used before we return. Default to
    // Brep tolerance (SM_OLDTOL_LINE: SetTolerance(GetTolerance()) = TRUE, TRUE).
    let tolerance = brep.tolerance();
    crate::topology::face::p3::set_tolerance(brep, face, tolerance, true, true)?;

    // pNewFU1 gets same orientation as the surface, pNewFU2 is opp to that.
    // The primary FU is the one with the same orientation as the surface.
    brep.faceuse_mut(fu1)?.orientation = orientation;
    if orientation == OrientType::Same {
        brep.face_mut(face)?.faceuse = Some(fu1); // FU1 is primary faceuse
        brep.faceuse_mut(fu2)?.orientation = OrientType::Opposite;
    } else {
        brep.face_mut(face)?.faceuse = Some(fu2); // FU2 is primary faceuse
        brep.faceuse_mut(fu2)?.orientation = OrientType::Same;
    }

    // Put face into brep's face list: `add_face_record` appended it, and no face has
    // been allocated since, so the list order is native.
    if brep.face_list().last() != Some(&face) {
        return Err(SolidError::invalid_input(
            OP,
            "new face is not last on the face list",
        ));
    }

    // Put faceuses into shell - modify existing Brep
    // Handle special case where shell goes from being wireEdge shell to face shell.
    {
        let record = brep.shell_mut(shell)?;
        if record.members != ShellMembers::Empty && record.shell_type != ShellType::Faceuse {
            record.members = ShellMembers::Empty;
            record.shell_type = ShellType::Faceuse;
        }
    }
    post_insert_faceuse(brep, shell, fu1)?;
    post_insert_faceuse(brep, shell, fu2)?;

    // construct and initialize a loop with loopuses for every input loop
    let total = options
        .edge_loops
        .len()
        .checked_add(options.loop_vertices.len())
        .ok_or_else(|| SolidError::invalid_input(OP, "loop count overflow"))?;
    let mut loopuses = NewLoopuses {
        upper: Vec::new(),
        lower: Vec::new(),
    };
    for ii in 0..total {
        // make new Loop and Loopuses
        let l = brep.add_loop_record(Loop {
            face,
            edges: Vec::new(),
            loopuse: None,
        })?;
        let lu1 = brep.add_loopuse_record(Loopuse::default())?;
        let lu2 = brep.add_loopuse_record(Loopuse::default())?;
        for entity in [
            EntityRef::Loop(l),
            EntityRef::Loopuse(lu1),
            EntityRef::Loopuse(lu2),
        ] {
            register(env, entity);
        }

        // add them to lists
        loopuses.upper.push(lu1);
        loopuses.lower.push(lu2);

        // Set faceuse, loopuse, and loops member
        let lu_orientation = if ii == 0 {
            OrientType::Same
        } else {
            OrientType::Opposite
        };
        brep.loop_mut(l)?.loopuse = Some(if orientation == OrientType::Same {
            lu1
        } else {
            lu2
        });
        // pNewFU1->PostInsert(pNewLU1); pNewFU2->PostInsert(pNewLU2)
        for (lu, mate, fu) in [(lu1, lu2, fu1), (lu2, lu1, fu2)] {
            *brep.loopuse_mut(lu)? = Loopuse {
                faceuse: Some(fu),
                loop_: Some(l),
                mate: Some(mate),
                start: LoopuseStart::Unknown,
                orientation: lu_orientation,
            };
            brep.faceuse_mut(fu)?.loopuses.push(lu);
        }
        brep.face_mut(face)?.loops.push(l);
    }

    // This is the point of no return. Any error past this point leaves the
    // partially connected topology; a radial-sector failure is noted and the
    // construction carries on.
    let mut return_stat: Result<()> = Ok(());

    // increment and lock an unlocked mark
    let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS)?;
    let connected = connect_edge_loops(
        k,
        brep,
        marks,
        env,
        lock.mark_type(),
        ConnectInput {
            region,
            face,
            options,
            loopuses: &loopuses,
            surface_key: options.surface_key,
        },
        &mut return_stat,
    );
    lock.release(std::slice::from_mut(&mut k.context))?;
    let keep_shell = connected?;

    // Next
    //  - iterate over every input vertex
    //    - Build new vertexuses to connect newFace to the input vertices
    //    - remove every shell owned by a vertexuse moving all
    //      shell children to the 'keeper' shell.
    for (m, &v) in options.loop_vertices.iter().enumerate() {
        let vu1 = if crate::topology::vertex::is_shell_vertex(brep, v)? {
            // reuse the shell/vertex Vertexuse as the 1st new loopuse/vertex vertexuse
            let vu1 = deref(crate::topology::vertex::primary_vertexuse(brep, v)?)?;

            // remove the vertexuse from the shell and the shell from the vertexuse
            let VertexuseOwner::Shell(shell_vu) = brep.vertexuse(vu1)?.owner else {
                return Err(SolidError::invalid_input(
                    OP,
                    "shell vertex use without shell",
                ));
            };
            let shell_vu = deref(shell_vu)?;
            brep.shell_mut(shell_vu)?.members = ShellMembers::Empty;
            brep.vertexuse_mut(vu1)?.owner = VertexuseOwner::Shell(None);

            // remove the shell from the region (status ignored) and delete
            let shell_region = deref(eu::shell_region(brep, shell_vu)?)?;
            match shell::region_remove_shell(brep, shell_region, shell_vu) {
                // `SmOwningTopology::Remove`'s SM_ERR (not on the list) is ignored
                Ok(()) | Err(SolidError::InvalidInput { .. }) => {}
                Err(error) => return Err(error),
            }
            delete_shell(k, brep, env, shell_vu)?;
            vu1
        } else {
            // vertex is connected to some Edges (any combination of wires and edges):
            // make a new loopuse/vertex vertexuse
            let vu1 = brep.add_vertexuse_record(Vertexuse::default())?;
            register(env, EntityRef::Vertexuse(vu1));
            post_insert_vertexuse(brep, v, vu1)?;

            let vertex_shell = crate::topology::vertex::shell_in_region(brep, v, Some(region))?;

            // when Vertex shell does not match the keep shell - merge shells
            if let Some(vertex_shell) = vertex_shell.filter(|s| Some(*s) != keep_shell) {
                // upstream transfers to a NULL keeper (no edge loops) unchecked
                let keep = deref(keep_shell)?;
                shell::transfer_children(k, brep, vertex_shell, keep, None)?;

                // remove VertexShell from its Region->Shell list
                let reg = deref(eu::shell_region(brep, vertex_shell)?)?;
                region_remove(brep, reg, vertex_shell)?;

                // delete the shell
                delete_shell(k, brep, env, vertex_shell)?;
            }
            vu1
        };

        let vu2 = brep.add_vertexuse_record(Vertexuse::default())?;
        register(env, EntityRef::Vertexuse(vu2));
        post_insert_vertexuse(brep, v, vu2)?;
        let index = m
            .checked_add(options.edge_loops.len())
            .ok_or_else(|| SolidError::invalid_input(OP, "loop count overflow"))?;
        let lu1 = *deref(loopuses.upper.get(index))?;
        let lu2 = *deref(loopuses.lower.get(index))?;
        for (vu, lu) in [(vu1, lu1), (vu2, lu2)] {
            brep.vertexuse_mut(vu)?.owner = VertexuseOwner::Loopuse(Some(lu));
            brep.loopuse_mut(lu)?.start = LoopuseStart::Vertexuse(Some(vu));
        }
    }

    // when asked - Assign UVTrimCurves to NewFace->Edgeuses
    if options.calculate_trims {
        // create trim curves before TestClosureSplitRegion, because it might create
        // the trim curves anyway

        // clean up any given curves to use as NewFace->Edgeuse->UVTrimCurves
        if let Some(curves) = options.uv_curves {
            assign_uv_curves(k, brep, &loopuses.upper, curves)?;
        }

        // build and fix NewFace->Edgeuse->UVTrimCurves
        crate::topology::face::p3::create_uv_trim_curves(k, brep, face, true, None)?;
    }

    // Next - see if new Face splits the existing region - move shell children
    // appropriately
    if !options.edge_loops.is_empty() {
        let split = test_closure_split_region_attempt_with_state(k, brep, marks, env, true, fu2)?;
        out.shell = split.shell;
        out.region = split.region;
        split.status?;
    }

    // Notify of face construction operation (status ignored)
    let surface_type = deref(brep.face(face)?.surface.as_ref())?.type_code();
    let surface_ref = options.surface_key.map(|key| ObjectRef {
        type_code: surface_type,
        key,
    });
    env.with_env(k, |env| {
        ignore_status(brep_p2::notify(
            brep,
            env,
            None,
            NotifyOperation::AddToBrep,
            [
                Some(NotifyArg::Entity(EntityRef::Face(face))),
                Some(NotifyArg::Object(brep_p2::brep_ref(brep))),
                surface_ref.map(NotifyArg::Object),
            ],
        ))
    })?;

    return_stat
}

/// The arguments of the edge-loop connection step.
#[derive(Clone, Copy)]
struct ConnectInput<'a, 'b> {
    region: RegionId,
    face: FaceId,
    options: &'a FaceTopologyOptions<'b>,
    loopuses: &'a NewLoopuses,
    surface_key: Option<ObjectKey>,
}

/// Inside the mark lock: select one keeper shell (removing every other shell owned
/// by a loop's first edge use, its children moving to the keeper), build or reuse
/// the two edge uses connecting the new face to each input edge use's edge, and
/// link them into the new loop uses. Returns the keeper shell.
#[allow(clippy::too_many_lines)] // one upstream block
fn connect_edge_loops(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyState<'_>,
    mark: MarkType,
    input: ConnectInput<'_, '_>,
    return_stat: &mut Result<()>,
) -> Result<Option<ShellId>> {
    let ConnectInput {
        region,
        face,
        options,
        loopuses,
        surface_key,
    } = input;
    let mut used_edges: Vec<EdgeId> = Vec::new();
    let mut used = 0usize;
    let mut keep_shell: Option<ShellId> = None;

    // for every input EdgeLoop
    for (kk, &count) in options.edge_loops.iter().enumerate() {
        let mut edgeuses1: Vec<EdgeuseId> = Vec::new();
        let mut edgeuses2: Vec<EdgeuseId> = Vec::new();
        let lu1 = *deref(loopuses.upper.get(kk))?;
        let lu2 = *deref(loopuses.lower.get(kk))?;

        // for every input edgeuse in this input loop
        for j in 0..count {
            let mut input_eu = *options.edgeuses.get(used).ok_or_else(|| {
                SolidError::invalid_input(OP, "edge loop exceeds input edge uses")
            })?;
            used += 1;

            // Use the first edge on the first loop to determine the shell to keep.
            if j == 0 {
                if kk == 0 {
                    // keep this edgeuses shell
                    keep_shell = eu::shell(brep, input_eu)?;
                } else {
                    // for subsequent loops - remove unique shells
                    let shell_of_eu = eu::shell(brep, input_eu)?;
                    if shell_of_eu != keep_shell {
                        let del_shell = deref(shell_of_eu)?;
                        let keep = deref(keep_shell)?;

                        // move all shell faceuses and edgesuses to pKeepShell
                        crate::topology::topology_traverser::remove_shell(
                            k, brep, del_shell, keep,
                        )?;

                        // remove shell from its Region->Shell list
                        let reg = deref(eu::shell_region(brep, del_shell)?)?;
                        region_remove(brep, reg, del_shell)?;

                        // delete the shell
                        delete_shell(k, brep, env, del_shell)?;
                    }
                }
            }

            // if the edge is an unmarked wire
            let edge = deref(brep.edgeuse(input_eu)?.edge)?;
            if crate::topology::edge::is_wire(brep, edge)?
                && !marks.is_marked(k, Entity::Edge(edge), mark)
            {
                // Place existing edgeuses into the edgeuse array
                edgeuses1.push(input_eu);
                edgeuses2.push(deref(eu::mate(brep, input_eu)?)?);
                // so that the second time we create new edgeuses for it
                marks.mark(k, Entity::Edge(edge), mark);
            } else {
                // Create and set new edgeuse/vertexuse pair
                let new_eu1 = brep.add_edgeuse_record(Edgeuse::default())?;
                let new_eu2 = brep.add_edgeuse_record(Edgeuse::default())?;
                let new_vu1 = brep.add_vertexuse_record(Vertexuse::default())?;
                let new_vu2 = brep.add_vertexuse_record(Vertexuse::default())?;
                for entity in [
                    EntityRef::Edgeuse(new_eu1),
                    EntityRef::Edgeuse(new_eu2),
                    EntityRef::Vertexuse(new_vu1),
                    EntityRef::Vertexuse(new_vu2),
                ] {
                    register(env, entity);
                }
                brep.edgeuse_mut(new_eu1)?.vertexuse = Some(new_vu1);
                brep.edgeuse_mut(new_eu2)?.vertexuse = Some(new_vu2);
                brep.vertexuse_mut(new_vu1)?.owner = VertexuseOwner::Edgeuse(Some(new_eu1));
                brep.vertexuse_mut(new_vu2)?.owner = VertexuseOwner::Edgeuse(Some(new_eu2));
                let input_mate = deref(eu::mate(brep, input_eu)?)?;
                let v1 = deref(
                    brep.vertexuse(deref(brep.edgeuse(input_eu)?.vertexuse)?)?
                        .vertex,
                )?;
                let v2 = deref(
                    brep.vertexuse(deref(brep.edgeuse(input_mate)?.vertexuse)?)?
                        .vertex,
                )?;
                let o1 = eu::orientation(brep, input_eu)?;
                let o2 = eu::orientation(brep, input_mate)?;
                eu::set_orientation(brep, new_eu1, o1)?;
                eu::set_orientation(brep, new_eu2, o2)?;
                post_insert_vertexuse(brep, v1, new_vu1)?;
                post_insert_vertexuse(brep, v2, new_vu2)?;

                // If we have a duplicate edge then we may have to insert it into a
                // different radial sector than that specified by the original. Also
                // its easy to pick the wrong radial sector in cases where a face is
                // being connected to an edge already connected to 2 or more faces.
                // So when bVerifyRadialSectors == TRUE, check those edges.
                if used_edges.contains(&edge)
                    || (options.verify_radial_sectors
                        && crate::topology::edge::edgeuse_count(brep, edge)? >= 4)
                {
                    let sector = SectorInput {
                        edge,
                        tmp: new_eu1,
                        loopuse: lu1,
                        region,
                        face,
                        surface_key,
                    };
                    if let Some(found) = find_sector(k, brep, env, sector)? {
                        input_eu = found;
                    } else {
                        // Note the problem and carry on.
                        *return_stat = Err(SolidError::sm_err(OP));
                        used_edges.push(edge);
                    }
                } else {
                    used_edges.push(edge);
                }

                // insert new mated edgeuse pair after pEU on the pE radial edgeuse list
                crate::topology::edge::add_oriented_eu_pair(
                    brep,
                    edge,
                    new_eu1,
                    new_eu2,
                    Some(input_eu),
                )?;
                edgeuses1.push(new_eu1);
                edgeuses2.push(new_eu2);
            }

            // Now connect up edgeuses with loopuses
            let eu1 = *deref(edgeuses1.last())?;
            let eu2 = *deref(edgeuses2.last())?;

            // Connect with Loopuse1, then with Loopuse2; the first edge use starts the
            // loop use (keeps order same for queries as input curves)
            for (e, lu) in [(eu1, lu1), (eu2, lu2)] {
                brep.edgeuse_mut(e)?.owner = EdgeuseOwner::Loopuse(Some(lu));
                if j == 0 {
                    brep.loopuse_mut(lu)?.start = LoopuseStart::Edgeuse(Some(e));
                }
            }
        }

        // Connect up clockwise/counter clockwise and loopuse pointers for each edgeuse.
        let (Some(&first_last1), Some(&first_last2)) = (edgeuses1.last(), edgeuses2.last()) else {
            break;
        };
        let mut last1 = first_last1;
        let mut last2 = first_last2;
        for (&eu1, &eu2) in edgeuses1.iter().zip(&edgeuses2) {
            // Connect CW and CCW pointers around loop
            brep.edgeuse_mut(eu1)?.cw = Some(last1);
            brep.edgeuse_mut(last1)?.ccw = Some(eu1);
            brep.edgeuse_mut(eu2)?.ccw = Some(last2);
            brep.edgeuse_mut(last2)?.cw = Some(eu2);

            // Set Last ones
            last1 = eu1;
            last2 = eu2;
        }
    }
    Ok(keep_shell)
}

/// The arguments of one radial-sector recovery.
#[derive(Clone, Copy)]
struct SectorInput {
    edge: EdgeId,
    tmp: EdgeuseId,
    loopuse: LoopuseId,
    region: RegionId,
    face: FaceId,
    surface_key: Option<ObjectKey>,
}

/// Temporarily connect the new edge use with its loop and edge, give it a temporary
/// UV trim curve, and find the radial sector of the edge containing the new face.
/// Returns the sector's edge use, `None` when sectoring fails (a native status).
fn find_sector(
    k: &mut Kernel,
    brep: &mut Brep,
    env: &mut NotifyState<'_>,
    input: SectorInput,
) -> Result<Option<EdgeuseId>> {
    let SectorInput {
        edge,
        tmp,
        loopuse,
        region,
        face,
        surface_key,
    } = input;
    // Just temporarily connect this with loop and edge so sectoring will work
    {
        let record = brep.edgeuse_mut(tmp)?;
        record.owner = EdgeuseOwner::Loopuse(Some(loopuse));
        record.edge = Some(edge);
    }

    // Create a temporary UV curve to help in the sectoring; the surface loses its
    // owner to prevent trimmed surface cache creation before the face is fully created.
    set_surface_owner(k, brep, env, face, None, surface_key, true)?;

    // Note: don't exit here (status ignored).
    let created = crate::topology::edgeuse::p3::create_uv_trim_curve(k, brep, tmp, false, None);
    match created {
        Ok(created) => {
            // TRUE = delete existing UVTrimCurve; `sCleanUV` deletes the curve at the
            // end of the scope, after the field reset below
            eu::set_uv_trim_curve(
                brep,
                tmp,
                eu::UvTrimCurveArg::New(Box::new(created.curve)),
                0.0,
                true,
                None,
            )?;
        }
        // the status is ignored
        Err(SolidError::Upstream { .. }) => {}
        Err(error) => return Err(error),
    }

    // Now do sectoring
    let found = crate::topology::edge::find_radial_sector(
        k,
        brep,
        edge,
        tmp,
        Some(region),
        OrientType::Same,
        None,
    );

    // Reset the temporary field changes
    let face_ref = (env.key_of)(EntityRef::Face(face));
    set_surface_owner(k, brep, env, face, Some(face_ref), surface_key, true)?;
    {
        let record = brep.edgeuse_mut(tmp)?;
        record.uv_trim_curve = None;
        record.owner = EdgeuseOwner::Loopuse(None);
        record.edge = None;
    }

    match found {
        Ok(sector) => Ok(sector.radial_edgeuse),
        Err(SolidError::Upstream { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Assign the caller's UV curves to the new face's edge uses in loop-use order:
/// each non-null curve is reversed for an opposite edge use, reparameterized to the
/// edge interval and set when close enough (`CheckAndSetUVTrimCurve`, statuses
/// ignored). Running out of curves stops each loop use.
fn assign_uv_curves(
    k: &Kernel,
    brep: &mut Brep,
    upper: &[LoopuseId],
    curves: &[Option<crate::nurbs::b_spline_curve::BSplineCurve>],
) -> Result<()> {
    let mut index = 0usize;
    for &lu in upper {
        let edgeuses = crate::topology::loopuse::edgeuses(brep, lu, None, None)?;
        for e in edgeuses {
            // check state - iUVCurve indx within cpOptUVCurves bound
            let Some(entry) = curves.get(index) else {
                break;
            };
            index += 1;

            // skip NULL entries
            let Some(curve) = entry else {
                continue;
            };
            let mut curve = curve.clone();

            // when needed - reverse pUVCurve parameterization
            if eu::orientation(brep, e)? == OrientType::Opposite {
                let interval = curve.natural_interval();
                ignore_status(curve.reverse_parameterization(&interval).map(|_| ()))?;
            }

            // set pUVCurve->ParameterRange = assoc Edgeuse->Edge->ParamRange
            let edge = deref(brep.edgeuse(e)?.edge)?;
            let interval = crate::topology::edge::interval(brep, edge)?;
            ignore_status(curve.edit_parameterization(&interval))?;

            ignore_status(
                crate::topology::edgeuse::p2::check_and_set_uv_trim_curve(
                    k,
                    brep,
                    e,
                    crate::curve::curve::Curve::from(curve),
                    true,
                    None,
                )
                .map(|_| ()),
            )?;
        }
    }
    Ok(())
}

/// Split a region when the seed face use bounds a closed set of neighbors.
/// Returns `(new_shell, new_region)`. Already separated mates, lamina faces and
/// non-closed traversals succeed without creating either object. Closed sets move
/// to a new solid region/shell, both notified through `env`, before the B-rep box
/// defers on `query.brep`.
/// The caller's marks and kernel context persist across calls. Every acquired
/// mark lock is released on success and on error.
pub fn test_closure_split_region(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyEnv<'_>,
    do_infinite_region_testing: bool,
    start: FaceuseId,
) -> Result<(Option<ShellId>, Option<RegionId>)> {
    let attempt =
        test_closure_split_region_attempt(k, brep, marks, env, do_infinite_region_testing, start)?;
    attempt.status?;
    Ok((attempt.shell, attempt.region))
}

/// Native status and outputs written by `TestClosureSplitRegion` before it exits.
#[derive(Debug)]
pub struct RegionSplitAttempt {
    /// The native `SmStatus`, represented as success or [`SolidError::Upstream`].
    /// A caller whose C++ counterpart ignores the status may ignore this field.
    pub status: Result<()>,
    /// New shell once the upstream output assignment is reached, even on error.
    pub shell: Option<ShellId>,
    /// New region once the upstream output assignment is reached, even on error.
    pub region: Option<RegionId>,
}

fn finish_split_attempt(
    mut attempt: RegionSplitAttempt,
    status: Result<()>,
) -> Result<RegionSplitAttempt> {
    match status {
        Ok(()) => Ok(attempt),
        Err(error @ SolidError::Upstream { .. }) => {
            attempt.status = Err(error);
            Ok(attempt)
        }
        // These failures do not correspond to a native status. In particular,
        // an unavailable callee or missing typed resolver must never become a
        // successful empty split just because native callers ignore SmStatus.
        Err(error) => Err(error),
    }
}

/// Test region closure, retaining native error status and partial output writes.
/// The outer result propagates unported dependencies and Rust representation
/// failures. Native `SmStatus` errors are in [`RegionSplitAttempt::status`], so
/// unchecked C++ callers can continue with the same partially written outputs.
pub fn test_closure_split_region_attempt(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyEnv<'_>,
    do_infinite_region_testing: bool,
    start: FaceuseId,
) -> Result<RegionSplitAttempt> {
    test_closure_split_region_attempt_with_state(
        k,
        brep,
        marks,
        &mut NotifyState::from_env(env),
        do_infinite_region_testing,
        start,
    )
}

fn test_closure_split_region_attempt_with_state(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyState<'_>,
    _do_infinite_region_testing: bool,
    start: FaceuseId,
) -> Result<RegionSplitAttempt> {
    let mut attempt = RegionSplitAttempt {
        status: Ok(()),
        shell: None,
        region: None,
    };
    let status = split_region_into(k, brep, marks, env, start, &mut attempt);
    finish_split_attempt(attempt, status)
}

fn split_region_into(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyState<'_>,
    start: FaceuseId,
    attempt: &mut RegionSplitAttempt,
) -> Result<()> {
    let seed = brep.faceuse(start)?;
    if let Some(mate) = seed.mate {
        if brep.faceuse(mate)?.shell != seed.shell {
            return Ok(());
        }
    }
    if tr::has_lamina_edge(brep, split_link(seed.face)?)? {
        return Ok(());
    }
    let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS)?;
    let result = split_closed(k, brep, marks, env, start, &lock, attempt);
    lock.release(std::slice::from_mut(&mut k.context))?;
    result
}

fn split_closed(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    env: &mut NotifyState<'_>,
    start: FaceuseId,
    lock: &MarkLock,
    attempt: &mut RegionSplitAttempt,
) -> Result<()> {
    if !tr::closure_traversal(k, brep, marks, start, lock, None)? {
        return Ok(());
    }
    let new_shell = brep.add_shell_record(Shell {
        shell_type: ShellType::Faceuse,
        ..Shell::default()
    })?;
    let new_region = brep.add_region_record(Region {
        in_brep: true,
        shells: vec![new_shell],
        is_void: 0,
    })?;
    brep.shell_mut(new_shell)?.region = Some(new_region);
    // the new region's SmTopology members (default-constructed)
    let region_key = (env.key_of)(EntityRef::Region(new_region)).key;
    env.records.insert(region_key, Topology::default());

    let old_shell = split_link(brep.faceuse(start)?.shell)?;
    let old_region = split_link(brep.shell(old_shell)?.region)?;
    // Keep these native list queries in their upstream order; arena iteration
    // would include detached objects and change the traversal.
    let _wires = shell::wire_edges(k, brep, old_shell)?;
    let _faceuses = brep.shell(old_shell)?.get_faceuses()?;
    let _shells = brep.region(old_region)?.shells.clone();
    let mut replaced = Vec::new();
    tr::faceuse_shell_replacement(k, brep, marks, start, new_shell, &mut replaced, lock)?;
    let mut other = Vec::new();
    tr::faceuse_traversal(
        k,
        brep,
        marks,
        split_link(brep.faceuse(start)?.mate)?,
        lock.mark_type(),
        Some(&mut other),
    )?;
    // Upstream publishes these pointers only after both traversals. They stay
    // available to unchecked callers when a later native operation fails.
    attempt.shell = Some(new_shell);
    attempt.region = Some(new_region);
    // notify this Brep and the OldRegion for attribute propagation (SmBrep.cpp:27954-5;
    // both statuses ignored)
    let this = Some(NotifyArg::Object(brep_p2::brep_ref(brep)));
    env.with_env(k, |env| {
        ignore_status(brep_p2::notify(
            brep,
            env,
            None,
            NotifyOperation::PreEdit,
            [this, None, None],
        ))
    })?;
    let region = |r| Some(NotifyArg::Entity(EntityRef::Region(r)));
    env.with_env(k, |env| {
        ignore_status(brep_p2::notify(
            brep,
            env,
            None,
            NotifyOperation::SplitInBrep,
            [region(old_region), region(old_region), region(new_region)],
        ))
    })?;

    // We will use this bbox a few times times.
    // just need a rough bound for the radius of the brep.
    let brep_bbox = qbrep::calculate_bounding_box(
        k,
        brep,
        BoundingBoxOptions {
            approx: true,
            tight: false,
            expand_by_zone_tol_3d: true,
        },
    )?;
    let _brep_sphere = brep_bbox.sphere_bound();

    // OMITTED SmBrep.cpp:27966-28583 (this unit's own remainder, not a callee):
    // infinite-region testing (FindAndSetInfiniteRegion), the inner-shell choice
    // (IsInnerShell), redistribution of faces, wires and shells joined only by
    // vertices (SmShell::RayFire) and empty-shell/region cleanup. See
    // plan/findings.md (topology.brep_p6).
    Err(SolidError::unported(
        "SmBrep::TestClosureSplitRegion (after CalculateBoundingBox, SmBrep.cpp:27966-28583)",
        "topology.brep_p6",
    ))
}

/// A status upstream ignores: only [`SolidError::Unported`] and the port's
/// representation failures ([`SolidError::InvalidInput`], [`SolidError::NotFound`])
/// propagate.
fn ignore_status(r: Result<()>) -> Result<()> {
    match r {
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::InvalidInput { .. }
            | SolidError::NotFound { .. }),
        ) => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::context::MarkType;
    use crate::math::Vec3;
    use crate::surface::plane::Plane;
    use crate::topology::shell::ShellMembers;

    #[test]
    fn split_releases_its_lock_and_preserves_the_callers_lock_on_dependency_error() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        let region = b
            .add_region_record(Region {
                in_brep: true,
                is_void: 1,
                ..Region::default()
            })
            .unwrap();
        let shell = b
            .add_shell_record(Shell {
                region: Some(region),
                shell_type: ShellType::Faceuse,
                ..Shell::default()
            })
            .unwrap();
        b.region_mut(region).unwrap().shells.push(shell);
        let face = b.add_face_record(Face::new(shell)).unwrap();
        let first = b
            .add_faceuse_record(Faceuse {
                shell: Some(shell),
                face: Some(face),
                ..Faceuse::default()
            })
            .unwrap();
        let second = b
            .add_faceuse_record(Faceuse {
                shell: Some(shell),
                face: Some(face),
                mate: Some(first),
                ..Faceuse::default()
            })
            .unwrap();
        b.faceuse_mut(first).unwrap().mate = Some(second);
        b.face_mut(face).unwrap().faceuse = Some(first);
        b.shell_mut(shell).unwrap().members = ShellMembers::Faceuses(vec![first, second]);
        let mut outer = MarkLock::new(
            std::slice::from_mut(&mut k.context),
            0,
            MarkType::Mark.code(),
        )
        .unwrap();
        let mut state = crate::topology::brep::p2::RegionNotifyState::new(&b);
        let result = test_closure_split_region_attempt(
            &mut k,
            &mut b,
            &mut Marks::default(),
            &mut state.env(),
            true,
            first,
        );
        // the B-rep box is ported (query.brep); the split stops where this unit's port
        // ends (the OMITTED remainder after CalculateBoundingBox)
        assert!(matches!(
            result,
            Err(SolidError::Unported {
                unit: "topology.brep_p6",
                ..
            })
        ));
        // the new region got its (default) SmTopology record
        assert_eq!(state.records.len(), 2);
        assert!(k.context.is_mark_locked(MarkType::Mark));
        assert!(!k.context.is_mark_locked(MarkType::Mark2));
        assert_eq!(b.regions().len(), 2);
        assert_eq!(b.region(RegionId::from_index(1)).unwrap().is_void(), 0);
        assert_eq!(b.region(region).unwrap().is_void(), 1);
        assert_eq!(
            b.faceuse(first).unwrap().shell,
            Some(ShellId::from_index(1))
        );
        assert_eq!(b.faceuse(second).unwrap().shell, Some(shell));
        outer.release(std::slice::from_mut(&mut k.context)).unwrap();
    }

    #[test]
    fn ignored_native_status_preserves_outputs_but_never_hides_rust_refusals() {
        let partial = || RegionSplitAttempt {
            status: Ok(()),
            shell: Some(ShellId::from_index(7)),
            region: Some(RegionId::from_index(11)),
        };
        let error = SolidError::sm_err("native::split");
        let attempt = finish_split_attempt(partial(), Err(error.clone())).unwrap();
        assert_eq!(attempt.status, Err(error));
        assert_eq!(attempt.shell, Some(ShellId::from_index(7)));
        assert_eq!(attempt.region, Some(RegionId::from_index(11)));
        for error in [
            SolidError::invalid_input(OP, "typed resolver required"),
            SolidError::unported("SmBrep::TestClosureSplitRegion", "topology.brep_p6"),
        ] {
            assert_eq!(
                finish_split_attempt(partial(), Err(error.clone())).unwrap_err(),
                error
            );
        }
    }

    #[test]
    fn missing_split_links_are_outer_errors_even_for_unchecked_callers() {
        let mut b = Brep::new();
        let seed = b.add_faceuse_record(Faceuse::default()).unwrap();
        let mut state = crate::topology::brep::p2::RegionNotifyState::default();
        let result = test_closure_split_region_attempt(
            &mut Kernel::default(),
            &mut b,
            &mut Marks::default(),
            &mut state.env(),
            true,
            seed,
        );
        assert!(matches!(result, Err(SolidError::InvalidInput { .. })));
        assert_eq!(b.shells().len(), 0);
        assert_eq!(b.regions().len(), 0);
    }

    #[test]
    fn extreme_loop_counts_fail_before_allocation_from_the_count() {
        let mut b = Brep::new();
        let region = b.add_region_record(Region::default()).unwrap();
        let surface = Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ));
        let options = FaceTopologyOptions {
            region: Some(region),
            edge_loops: &[usize::MAX],
            ..FaceTopologyOptions::default()
        };
        let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&b);
        assert!(matches!(
            make_face_topology(
                &mut Kernel::default(),
                &mut b,
                &mut Marks::default(),
                &mut state.env(None),
                Some(surface),
                OrientType::Same,
                &options
            ),
            Err(SolidError::InvalidInput { .. })
        ));
        assert_eq!(b.faces().len(), 0);
        assert_eq!(b.loopuses().len(), 0);
    }

    fn z_plane() -> Surface {
        Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ))
    }

    #[test]
    fn edgeless_face_gets_a_new_shell_and_its_primary_faceuse() {
        for orientation in [OrientType::Same, OrientType::Opposite] {
            let mut k = Kernel::default();
            let mut b = Brep::new_with_infinite_region().unwrap();
            let region = b.infinite_region().unwrap();
            let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&b);
            let before = k.context.current_mark(MarkType::Mark);
            let made = make_face_topology(
                &mut k,
                &mut b,
                &mut Marks::default(),
                &mut state.env(None),
                Some(z_plane()),
                orientation,
                &FaceTopologyOptions::default(),
            )
            .unwrap();
            let shell = made.shell.unwrap();
            assert_eq!(made.region, None);
            assert_eq!(b.region(region).unwrap().shells, vec![shell]);
            let face = b.face(made.face).unwrap();
            let primary = face.faceuse.unwrap();
            assert_eq!(b.faceuse(primary).unwrap().orientation, OrientType::Same);
            let ShellMembers::Faceuses(members) = &b.shell(shell).unwrap().members else {
                panic!("face-use shell expected");
            };
            assert_eq!(members.len(), 2);
            assert_eq!(b.faceuse(members[0]).unwrap().orientation, orientation);
            assert!(face.loops.is_empty());
            assert_eq!(face.tolerance, b.tolerance());
            // the mark lock claimed (and released) a new mark
            assert!(k.context.current_mark(MarkType::Mark) > before);
            assert!(!k.context.is_mark_locked(MarkType::Mark));
        }
    }

    #[test]
    fn a_shell_vertex_becomes_a_vertex_loop_and_its_shell_is_deleted() {
        let mut k = Kernel::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        let region = b.infinite_region().unwrap();
        let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&b);
        let (old_shell, v) = crate::topology::brep::p9::make_shell_vertex(
            &mut b,
            None,
            Vec3::new(0.5, 0.5, 0.0),
            &mut state.env(None),
        )
        .unwrap();
        state.seed(&b);
        let made = make_face_topology(
            &mut k,
            &mut b,
            &mut Marks::default(),
            &mut state.env(None),
            Some(z_plane()),
            OrientType::Same,
            &FaceTopologyOptions {
                loop_vertices: &[v],
                ..FaceTopologyOptions::default()
            },
        )
        .unwrap();
        let shell = made.shell.unwrap();
        assert_eq!(b.region(region).unwrap().shells, vec![shell]);
        assert_eq!(b.shell(old_shell).unwrap(), &Shell::default());
        let uses = &b.vertex(v).unwrap().vertexuses;
        assert_eq!(uses.len(), 2);
        let face = b.face(made.face).unwrap();
        assert_eq!(face.loops.len(), 1);
        for &vu in uses {
            let VertexuseOwner::Loopuse(Some(lu)) = b.vertexuse(vu).unwrap().owner else {
                panic!("loop vertex use expected");
            };
            assert_eq!(
                b.loopuse(lu).unwrap().start,
                LoopuseStart::Vertexuse(Some(vu))
            );
            assert_eq!(b.loopuse(lu).unwrap().orientation, OrientType::Same);
        }
    }

    #[test]
    fn a_null_surface_or_region_fails_before_allocation() {
        let mut b = Brep::new();
        let mut state = crate::topology::brep::p2::ArenaNotifyState::new(&b);
        let r = make_face_topology_attempt(
            &mut Kernel::default(),
            &mut b,
            &mut Marks::default(),
            &mut state.env(None),
            Some(z_plane()),
            OrientType::Same,
            &FaceTopologyOptions::default(),
        )
        .unwrap();
        assert!(matches!(
            r.status,
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
        assert_eq!(r.face, None);
        assert_eq!(b.faces().len(), 0);
        assert_eq!(b.shells().len(), 0);
    }
}
