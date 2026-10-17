//! Edge copying, face topology construction and region-closure testing.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`copy_edges`] | `SmBrep::CopyEdges` |
//! | [`make_face_topology`] | `SmBrep::MakeFaceTopology` |
//! | [`test_closure_split_region`] | `SmBrep::TestClosureSplitRegion(SmFaceuse*)` |
//! | [`test_closure_split_region_attempt`] | same, retaining native status and partial outputs |
//!
//! Deviations: pointers become arena ids; optional outputs become return values.
//! Edits preserve the upstream mutations preceding an error. Missing callees stop
//! at their first call: copying stops at `CreateWireEdgeFromCurve` (`brep_p5`), face
//! construction at `SetTolerance` (`face_p3`), and a closed-shell split at `Notify`
//! (`brep_p2`). The dependent remainder of each algorithm is deferred, rather than
//! approximating construction, attributes, radial sectors, trims or ray casting.
//! The shell implementation comes from PR #154. Surface owner identities are
//! supplied by the caller, independently of arena indices. A null trimmed copy
//! returns a null-pointer error instead of the upstream dereference. There is no recursion;
//! the traverser uses its bounded, iterative implementation.
//! An attempt exposes native status separately for callers that ignore it;
//! missing dependencies and Rust representation failures remain outer errors.

use crate::error::{Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::context::{MarkLock, ALL_MARKS};
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::shell::{self, ShellType};
use crate::topology::topology_traverser::{self as tr, Marks};
use crate::topology::{
    Brep, EdgeId, EdgeuseId, Face, FaceId, Faceuse, FaceuseId, Region, RegionId, Shell, ShellId,
    VertexId,
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

/// Copy edges to independent wires in `destination`, in input order.
/// An empty list succeeds without inspecting the destination. Nonempty input
/// trims a NURBS copy in the destination's context before reaching the unported
/// wire constructor; copy notifications are not emitted before construction.
pub fn copy_edges(
    destination_kernel: &Kernel,
    source: &Brep,
    edges: &[EdgeId],
    destination: &mut Brep,
) -> Result<Vec<EdgeId>> {
    let result = Vec::new();
    if let Some(&edge) = edges.first() {
        let _curve = required(crate::topology::edge::create_trimmed_nurbs_curve(
            destination_kernel,
            source,
            edge,
        )?)?;
        let _ = destination;
        // UNPORTED(topology.brep_p5): SmBrep::CreateWireEdgeFromCurve
        return Err(SolidError::unported(
            "SmBrep::CreateWireEdgeFromCurve",
            "topology.brep_p5",
        ));
    }
    Ok(result)
}

/// Topology created by `MakeFaceTopology` when construction succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceTopology {
    /// Newly created face.
    pub face: FaceId,
    /// New shell, including the shell created for an edgeless face.
    pub shell: Option<ShellId>,
    /// New region when the face splits an existing region.
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
    /// Optional UV curves (including null entries) for the trim stage.
    pub uv_curves: Option<&'a [Option<crate::nurbs::b_spline_curve::BSplineCurve>]>,
}

/// Begin constructing a face from ordered edge-use loops and isolated vertices.
/// `face_key` is the caller's identity for the new face. A missing region or
/// surface fails before allocation. Malformed loop counts return `InvalidInput`
/// instead of reading past the input. The active `SM_USE_OLDTOL` call to
/// `SmFace::SetTolerance` currently defers every otherwise valid construction.
/// On error, allocated records remain in `brep`, as upstream retains its partial
/// edits; callers must not treat an error as an atomic rollback.
pub fn make_face_topology(
    brep: &mut Brep,
    surface: Option<Surface>,
    _orientation: OrientType,
    face_key: ObjectKey,
    options: &FaceTopologyOptions<'_>,
) -> Result<FaceTopology> {
    let region = required(options.region.or(brep.infinite_region()))?;
    brep.region(region)?;
    let mut surface = required(surface)?;
    let shell = if let Some(&first) = options.edgeuses.first() {
        required(eu::shell(brep, first)?)?
    } else {
        let shell = brep.add_shell_record(Shell {
            shell_type: ShellType::Faceuse,
            region: Some(region),
            ..Shell::default()
        })?;
        brep.region_mut(region)?.shells.push(shell);
        shell
    };

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
        let mut last = *required(loop_edges.last())?;
        for &current in loop_edges {
            let mate = required(eu::mate(brep, last)?)?;
            let current_vertex = required(brep.edgeuse(current)?.vertexuse)?;
            let last_vertex = required(brep.edgeuse(mate)?.vertexuse)?;
            // SE(INVALID_INPUT), not SER: inconsistent ordering only reports a
            // diagnostic upstream. Do not turn it into an early return here.
            let _connected =
                brep.vertexuse(current_vertex)?.vertex == brep.vertexuse(last_vertex)?.vertex;
            last = current;
        }
        used = end;
    }

    let face = brep.add_face_record(Face::new(shell))?;
    let first = brep.add_faceuse_record(Faceuse {
        face: Some(face),
        ..Faceuse::default()
    })?;
    let second = brep.add_faceuse_record(Faceuse {
        face: Some(face),
        mate: Some(first),
        ..Faceuse::default()
    })?;
    brep.faceuse_mut(first)?.mate = Some(second);
    surface.set_owner(Some(ObjectRef {
        type_code: crate::topology::face::p2::TYPE,
        key: face_key,
    }));
    let domain = surface.natural_uv_domain();
    let record = brep.face_mut(face)?;
    record.surface = Some(surface);
    record.uv_domain = domain;
    // This call precedes primary-faceuse assignment, list insertion, loop
    // allocation, radial sectors, trims and TestClosureSplitRegion upstream.
    // UNPORTED(topology.face_p3): SmFace::SetTolerance
    Err(SolidError::unported(
        "SmFace::SetTolerance",
        "topology.face_p3",
    ))
}

/// Split a region when the seed face use bounds a closed set of neighbors.
/// Returns `(new_shell, new_region)`. Already separated mates, lamina faces and
/// non-closed traversals succeed without creating either object. Closed sets move
/// to a new solid region/shell before the first notification defers on `brep_p2`.
/// The caller's marks and kernel context persist across calls. Every acquired
/// mark lock is released on success and on error.
pub fn test_closure_split_region(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
    do_infinite_region_testing: bool,
    start: FaceuseId,
) -> Result<(Option<ShellId>, Option<RegionId>)> {
    let attempt =
        test_closure_split_region_attempt(k, brep, marks, do_infinite_region_testing, start)?;
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
    _do_infinite_region_testing: bool,
    start: FaceuseId,
) -> Result<RegionSplitAttempt> {
    let mut attempt = RegionSplitAttempt {
        status: Ok(()),
        shell: None,
        region: None,
    };
    let status = split_region_into(k, brep, marks, start, &mut attempt);
    finish_split_attempt(attempt, status)
}

fn split_region_into(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
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
    let result = split_closed(k, brep, marks, start, &lock, attempt);
    lock.release(std::slice::from_mut(&mut k.context))?;
    result
}

fn split_closed(
    k: &mut Kernel,
    brep: &mut Brep,
    marks: &mut Marks,
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
    // Notify(PRE_EDIT) and Notify(SPLIT_IN_BREP) precede CalculateBoundingBox,
    // infinite-region testing, classification of residual faces/wires/shells,
    // and empty-shell/region cleanup. None of those may run before this call.
    // UNPORTED(topology.brep_p2): SmBrep::Notify
    Err(SolidError::unported("SmBrep::Notify", "topology.brep_p2"))
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
        let result =
            test_closure_split_region_attempt(&mut k, &mut b, &mut Marks::default(), true, first);
        assert!(matches!(
            result,
            Err(SolidError::Unported {
                unit: "topology.brep_p2",
                ..
            })
        ));
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
            SolidError::unported("SmBrep::Notify", "topology.brep_p2"),
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
        let result = test_closure_split_region_attempt(
            &mut Kernel::default(),
            &mut b,
            &mut Marks::default(),
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
        assert!(matches!(
            make_face_topology(
                &mut b,
                Some(surface),
                OrientType::Same,
                ObjectKey(999),
                &options
            ),
            Err(SolidError::InvalidInput { .. })
        ));
        assert_eq!(b.faces().len(), 0);
        assert_eq!(b.loopuses().len(), 0);
    }
}
