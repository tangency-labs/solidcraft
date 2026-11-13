//! The B-rep object (`SmBrep`): construction from serialized data, editing and
//! queries, split into the units `topology.brep_p1` .. `topology.brep_p10`.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust      | Upstream                                                        |
//! |-----------|-----------------------------------------------------------------|
//! | [`cutting`] | `SmBrepCutting`, `SmPlaneCutter`, `SmApiCut` (unit `topology.brep_cutting`) |
//! | [`local_operation`] | `LocalOperation`                                     |
//! | [`mirror`]| `Mirror` (unit `topology.brep_mirror`)                          |
//! | [`p1`]    | see the module                                                  |
//! | [`p2`]    | see the module                                                  |
//! | [`p4`]    | `~SmBrep`, `DeleteEdge`, `DeleteVertex`, `StitchFaces`, `ShrinkGeometry` |
//! | [`p5`]    | B-rep editing and transformation operations, part 5             |
//! | [`p6`]    | see the module                                                  |
//! | [`p7`]    | see the module                                                  |
//! | [`planar_section`] | `CreatePlanarSectionCurves` (unit `topology.brep_planar_section`) |
//! | [`p8`]    | making faces from surfaces and from loops of curves              |
//! | [`vertex_fans`] | `SplitVertexByFans`, `JoinVertexFans` (the fork's, `4ecfbab`) |
//! | [`p10`]   | `MakeTopologyFromData`, `ProjectAndTrim`, `RemoveAllTopology`, `TestClosureSplitRegion` (edge version) |
//! | [`is_outward_solid`] | (not upstream) a valid closed solid facing out             |
//!
//! Deviations: the B-rep records are the arenas of [`crate::topology::Brep`].

pub mod cutting;
pub mod enclosing;
pub mod local_operation;
pub mod mirror;
pub mod outer_shells;
pub mod p1;
pub mod p10;
pub mod p2;
pub mod p3;
pub mod p4;
pub mod p5;
pub mod p6;
pub mod p7;
pub mod p8;
pub mod p9;
pub mod planar_section;
pub mod sheet;
pub mod trim_extend;
pub mod validation;
pub mod vertex_fans;

// Supported public paths; numbered modules are implementation partitions.
pub use p1::{create_parallel_projection_curves, edges, faces, regions, ParallelProjectionOptions};
pub use p10::{
    make_topology_from_data, merge_curves_on_surface_through_surface, MakeTopologyOptions,
    TopologyEnv, HEALER_VERSION,
};
pub use p2::{is_manifold_solid, is_sheet, shell_vertices, ArenaNotifyState};
pub use p3::{
    api_brep_copy, delete_face, new_brep, orient_trimmed_surfaces, topology_ref,
    with_kernel_callbacks, DeleteFaceOptions,
};
pub use p4::stitch_faces;
pub use p5::{
    make_edge_in_face, remove_topological_edges_and_vertices, stitch_and_orient, EdgeInFace,
    StitchAndOrientOptions,
};
pub use p7::copy_faces;
pub use p8::{
    create_face_from_surface, create_faces_with_curves, create_planar_face_with_3d_curves,
    create_planar_faces_with_3d_curves, FaceBuildEnv,
};
pub use p9::{make_vertex_loop, orient_sheet_surfaces, OrientSheetNote};

/// Whether `b` is a valid closed solid facing out (not upstream): one closed manifold solid
/// ([`is_manifold_solid`]), passed by the kernel's validity check (its pointer check and
/// `AssertValid`, [`validation::check_brep`] at `Level0`), and its volume proved positive by
/// its certified enclosure ([`crate::mass::enclosure::volume_sign`]; an undecided sign is
/// not).
///
/// The two guard the faceuse orientation together. A body with every face flipped
/// (`FlipFaceOrientation`, which leaves a face invalid) is still manifold and keeps a
/// positive volume (the integrator reads the loops and surfaces, not the uses' orientation):
/// the pointer check is what refuses it. A valid body turned inside out (its loops and
/// surfaces reversed with its uses) has a negative volume.
///
/// A sign the enclosure leaves undecided (its interval still holds 0 when refinement ends:
/// a gap between faces as large as the volume, or the cell budget spent) is "not a solid" on
/// purpose: a solid is what is proved one, so ops' `finish` refuses the body as `NotSolid`.
/// A body the enclosure cannot read at all is an error, not `false`: a face with no NURBS form
/// (an offset surface, as the offset of a round keeps; a curve-bounded or in-volume surface),
/// a face without trim loops, or weights not proved positive.
///
/// A region the kernel is unsure of is [`crate::SolidError::UnsureRegion`], not "not a solid":
/// the volume's sign would read it as void.
///
/// # Errors
///
/// Those of the manifold test, the validity check and the volume
/// ([`crate::mass::enclosure::volume_sign`]: [`crate::SolidError::Unsupported`] for a body
/// it cannot read); [`crate::SolidError::UnsureRegion`].
pub fn is_outward_solid(
    k: &mut crate::kernel::Kernel,
    b: &mut crate::topology::Brep,
) -> crate::Result<bool> {
    Ok(outward_solid_failure(k, b)?.is_none())
}

/// Which test a body fails to be a closed solid facing out ([`outward_solid_failure`] and its
/// siblings), in the order the tests run: the first one that fails is the one named. More tests
/// may be named, so a caller outside the kernel matches it with a wildcard or says it by its
/// `Display`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SolidTest {
    /// The body is not one closed manifold solid ([`is_manifold_solid`]).
    NotManifold,
    /// The body is neither one closed manifold solid nor closed solids touching along edges or
    /// at vertices ([`crate::query::api_queries::brep_is_closed_touching_solids`]): the test of
    /// [`is_outward_touching_solids`].
    NotClosedSolids,
    /// The validity check's pointer check failed ([`validation::check_brep`] at `Level0`: its
    /// status, or a report that leaves `AssertValid` unrun): a body with every face flipped
    /// (`FlipFaceOrientation`), or a vertex whose tolerance is 1000 times the body's.
    PointersInvalid,
    /// The pointer check passed but the validity check's `AssertValid` did not: a B-spline
    /// face whose surface has an internal pole, for one.
    Invalid,
    /// A loop's certified area leaves its orientation undecided. No test of the solid check
    /// decides a loop's orientation by its area yet, so none of the checks here returns it;
    /// it is named so a caller's match already covers it when one does.
    LoopOrientationUndecided,
    /// The volume's certified enclosure lies below 0: the body is proved inside out.
    VolumeNegative,
    /// The volume's certified enclosure still holds 0 when refinement ends: its sign is not
    /// proved ([`crate::mass::enclosure::VolumeSign::Undecided`]).
    VolumeUndecided,
}

impl core::fmt::Display for SolidTest {
    /// What the body fails, as a clause: "it is inside out".
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            SolidTest::NotManifold => "it is not one closed manifold solid",
            SolidTest::NotClosedSolids => {
                "it is not closed solids, even ones touching along edges or at vertices"
            }
            SolidTest::PointersInvalid => "its pointer check fails",
            SolidTest::Invalid => "its validity check fails",
            SolidTest::LoopOrientationUndecided => "a loop's orientation is undecided",
            SolidTest::VolumeNegative => "it is inside out",
            SolidTest::VolumeUndecided => "its volume's sign is undecided",
        })
    }
}

/// As [`is_outward_solid`], naming the test the body fails: `None` where it is a valid closed
/// solid facing out, else the first test that fails ([`SolidTest::NotManifold`],
/// [`SolidTest::PointersInvalid`], [`SolidTest::Invalid`], [`SolidTest::VolumeNegative`],
/// [`SolidTest::VolumeUndecided`]). The tests and their order are [`is_outward_solid`]'s.
///
/// # Errors
///
/// As [`is_outward_solid`].
pub fn outward_solid_failure(
    k: &mut crate::kernel::Kernel,
    b: &mut crate::topology::Brep,
) -> crate::Result<Option<SolidTest>> {
    if !is_manifold_solid(b)? {
        return Ok(Some(SolidTest::NotManifold));
    }
    valid_and_outward_failure(k, b)
}

/// As [`is_outward_solid`], the body allowed to be closed solids touching along edges or at
/// vertices ([`crate::query::api_queries::brep_is_closed_touching_solids`]), as a boolean's
/// result may be.
///
/// # Errors
///
/// As [`is_outward_solid`], and those of the touching-solids test.
pub fn is_outward_touching_solids(
    k: &mut crate::kernel::Kernel,
    b: &mut crate::topology::Brep,
) -> crate::Result<bool> {
    Ok(outward_touching_solids_failure(k, b)?.is_none())
}

/// As [`is_outward_touching_solids`], naming the test the body fails:
/// [`SolidTest::NotClosedSolids`] for its topology test, else as [`outward_solid_failure`].
///
/// # Errors
///
/// As [`is_outward_touching_solids`].
pub fn outward_touching_solids_failure(
    k: &mut crate::kernel::Kernel,
    b: &mut crate::topology::Brep,
) -> crate::Result<Option<SolidTest>> {
    if !is_manifold_solid(b)? && !crate::query::api_queries::brep_is_closed_touching_solids(b)? {
        return Ok(Some(SolidTest::NotClosedSolids));
    }
    valid_and_outward_failure(k, b)
}

/// The validity check, no unsure region, and the volume's sign, for a closed body: the part
/// of [`is_outward_solid`] and [`is_outward_touching_solids`] after their topology tests,
/// which both share. `true` only where the check passes (pointers and `AssertValid`, at
/// `Level0`), no region is unsure, and the volume is proved positive. A caller that holds
/// this proof for a body it has not changed since may take it for the body again under the
/// same kernel settings (solidcore-ops' proof reuse, #14830).
///
/// # Errors
///
/// As [`is_outward_solid`]'s, after its manifold test.
pub fn is_valid_and_outward(
    k: &mut crate::kernel::Kernel,
    b: &mut crate::topology::Brep,
) -> crate::Result<bool> {
    Ok(valid_and_outward_failure(k, b)?.is_none())
}

/// As [`is_valid_and_outward`], naming the test the body fails: [`SolidTest::PointersInvalid`],
/// [`SolidTest::Invalid`], [`SolidTest::VolumeNegative`] or [`SolidTest::VolumeUndecided`], in
/// that order; `None` where every one passes.
///
/// # Errors
///
/// As [`is_valid_and_outward`].
pub fn valid_and_outward_failure(
    k: &mut crate::kernel::Kernel,
    b: &mut crate::topology::Brep,
) -> crate::Result<Option<SolidTest>> {
    let c = validation::check_brep(k, b, validation::CheckLevel::Level0)?;
    // `AssertValid` does not run (`None`) where a pointer report failed
    if c.pointers_status != crate::error::SM_SUCCESS || c.assert_valid.is_none() {
        return Ok(Some(SolidTest::PointersInvalid));
    }
    if c.assert_valid != Some(true) {
        return Ok(Some(SolidTest::Invalid));
    }
    crate::mass::integrator::properties::no_unsure_region(b)?;
    Ok(match crate::mass::enclosure::volume_sign(k, b)? {
        crate::mass::enclosure::VolumeSign::Positive(_) => None,
        crate::mass::enclosure::VolumeSign::Negative(_) => Some(SolidTest::VolumeNegative),
        crate::mass::enclosure::VolumeSign::Undecided(_) => Some(SolidTest::VolumeUndecided),
    })
}

#[cfg(test)]
mod tests {
    use super::is_outward_solid;
    use crate::kernel::{Kernel, KernelOptions};
    use crate::math::Vec3;
    use crate::primitive::api_primitives::{create_box, create_plane};
    use crate::topology::face::p1::flip_face_orientation;

    /// A box faces out; the same box with every face flipped (still manifold, its volume still
    /// positive) does not, nor does a sheet.
    #[test]
    fn a_box_faces_out_and_its_flip_does_not() {
        let mut k = Kernel::new(&KernelOptions::default());
        let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
        assert!(is_outward_solid(&mut k, &mut b).unwrap());
        for f in b.face_list().to_vec() {
            flip_face_orientation(&mut b, f).unwrap();
        }
        assert!(crate::topology::brep::p2::is_manifold_solid(&b).unwrap());
        assert!(!is_outward_solid(&mut k, &mut b).unwrap());
        let mut sheet = create_plane(&mut k, Vec3::ZERO, 1.0, 1.0).unwrap();
        assert!(!is_outward_solid(&mut k, &mut sheet).unwrap());
    }
}
