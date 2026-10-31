//! Status-audit regressions on the real primitive model, with failures injected at
//! existing topology service boundaries. No synthetic success replaces the sweep.

use super::*;
use crate::curve::composite_curve::OffsetMapAttribute;
use crate::error::{Result, SolidError};
use crate::geom::attribute::TagData;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::MarkType;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::p1::NurbCircleParam;
use crate::primitive::primitive_creation::p3::PrimitiveAccess;
use crate::sweep::topology_sweep::access::{EdgeRef, FaceRef, SweepAccess, SweepMergeId};
use crate::sweep::topology_sweep::SweepEntity;
use crate::topology::brep::p10::TrimType;
use crate::topology::edge::NotifyEnv;
use crate::topology::{Brep, BrepId, FaceId};

use crate::primitive::primitive_creation::model::PrimitiveModel;

struct FaultAccess<'k> {
    inner: PrimitiveModel<'k>,
    allocated: Vec<BrepId>,
    deleted: Vec<BrepId>,
    calls: Vec<&'static str>,
    fail: (&'static str, usize),
}
fn injected() -> SolidError {
    SolidError::sm_err("audit_483_injected_planar_service")
}
impl<'k> FaultAccess<'k> {
    fn new(k: &'k mut Kernel, fail: (&'static str, usize)) -> Self {
        Self {
            inner: PrimitiveModel::new(k, Brep::new_with_infinite_region().unwrap()),
            allocated: vec![],
            deleted: vec![],
            calls: vec![],
            fail,
        }
    }
    fn check(&mut self, name: &'static str) -> Result<()> {
        self.calls.push(name);
        if (self.fail.0 == name && self.calls.iter().filter(|&&s| s == name).count() == self.fail.1)
            || (self.fail.0 == "recover_search"
                && matches!(name, "with_breps_mut" | "project_and_trim"))
        {
            Err(injected())
        } else {
            Ok(())
        }
    }
}
impl SweepAccess for FaultAccess<'_> {
    fn brep(&self, id: BrepId) -> Result<&Brep> {
        self.inner.brep(id)
    }
    fn brep_mut(&mut self, id: BrepId) -> Result<&mut Brep> {
        self.inner.brep_mut(id)
    }
    fn kernel(&self, arg1: BrepId) -> Result<&Kernel> {
        self.inner.kernel(arg1)
    }
    fn kernel_mut(&mut self, arg1: BrepId) -> Result<&mut Kernel> {
        self.inner.kernel_mut(arg1)
    }
    fn with_brep_mut<T>(
        &mut self,
        arg1: BrepId,
        arg2: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &dyn Fn(FaceId) -> ObjectRef,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        self.check("with_brep_mut")?;
        self.inner.with_brep_mut(arg1, arg2)
    }
    fn with_breps_mut<T>(
        &mut self,
        arg1: BrepId,
        arg2: BrepId,
        arg3: impl FnOnce(
            &mut Kernel,
            &mut Brep,
            &mut Brep,
            &dyn Fn(crate::EntityRef) -> ObjectRef,
            &dyn Fn(FaceId) -> ObjectRef,
            Option<&mut crate::geom::context::Context>,
            &mut NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        self.check("with_breps_mut")?;
        self.inner.with_breps_mut(arg1, arg2, arg3)
    }
    fn with_brep_notifications<T>(
        &mut self,
        arg1: BrepId,
        arg2: impl FnOnce(&mut Brep, &mut NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        self.inner.with_brep_notifications(arg1, arg2)
    }
    fn retain_hcr(&mut self, arg1: BrepId, arg2: crate::blend::hcr::Hcr) -> Result<BrepId> {
        self.inner.retain_hcr(arg1, arg2)
    }
    fn hcr_session(&self, arg1: BrepId) -> Result<&crate::blend::hcr::Hcr> {
        self.inner.hcr_session(arg1)
    }
    fn new_brep(&mut self, arg1: BrepId) -> Result<BrepId> {
        let id = self.inner.new_brep(arg1)?;
        self.allocated.push(id);
        Ok(id)
    }
    fn delete_brep(&mut self, id: BrepId) {
        self.deleted.push(id);
        self.inner.delete_brep(id);
    }
    fn new_merge(
        &mut self,
        arg1: BrepId,
        arg2: BrepId,
        arg3: BrepId,
        arg4: f64,
        arg5: f64,
    ) -> Result<SweepMergeId> {
        self.inner.new_merge(arg1, arg2, arg3, arg4, arg5)
    }
    fn merge_operands(&self, arg1: SweepMergeId) -> Result<(BrepId, BrepId)> {
        self.inner.merge_operands(arg1)
    }
    fn merge_relations(
        &mut self,
        arg1: SweepMergeId,
    ) -> Result<&mut crate::boolean::topology_intersector::p1::Relations> {
        self.inner.merge_relations(arg1)
    }
    fn copy_faces(
        &mut self,
        arg1: BrepId,
        arg2: &[FaceId],
        arg3: BrepId,
        arg4: SweepMergeId,
        arg5: &mut crate::sweep::topology_sweep::access::BTreeEdgeMap,
    ) -> Result<()> {
        self.inner.copy_faces(arg1, arg2, arg3, arg4, arg5)
    }
    fn piecewise_merge(&mut self, arg1: SweepMergeId, arg2: bool, arg3: bool) -> Result<BrepId> {
        self.inner.piecewise_merge(arg1, arg2, arg3)
    }
    fn union(&mut self, arg1: SweepMergeId) -> Result<BrepId> {
        self.inner.union(arg1)
    }
    fn delete_merge(&mut self, arg1: SweepMergeId) {
        self.inner.delete_merge(arg1);
    }
    fn tag(&self, arg1: SweepEntity) -> Result<Option<TagData>> {
        self.inner.tag(arg1)
    }
    fn add_tag(&mut self, arg1: SweepEntity, arg2: TagData) -> Result<()> {
        self.inner.add_tag(arg1, arg2)
    }
    fn curve_tag(&self, arg1: EdgeRef) -> Result<Option<TagData>> {
        self.inner.curve_tag(arg1)
    }
    fn set_editing(&mut self, arg1: BrepId, editing: bool) -> Result<bool> {
        self.inner.set_editing(arg1, editing)
    }
    fn lock_mark(&mut self, arg1: BrepId, arg2: i32) -> Result<MarkType> {
        self.inner.lock_mark(arg1, arg2)
    }
    fn unlock_mark(&mut self, arg1: BrepId, arg2: MarkType) -> Result<()> {
        self.inner.unlock_mark(arg1, arg2)
    }
    fn is_marked(&self, arg1: SweepEntity, arg2: MarkType) -> Result<bool> {
        self.inner.is_marked(arg1, arg2)
    }
    fn mark(&mut self, arg1: SweepEntity, arg2: MarkType) -> Result<()> {
        self.inner.mark(arg1, arg2)
    }
    fn collect_faces(
        &mut self,
        arg1: FaceRef,
        arg2: MarkType,
        arg3: &mut Vec<FaceId>,
    ) -> Result<()> {
        self.inner.collect_faces(arg1, arg2, arg3)
    }
    fn infinite_region(&mut self, b: BrepId) -> Result<crate::topology::RegionId> {
        self.check("sweep_start")?;
        self.inner.infinite_region(b)
    }
    fn remove_faces(&mut self, b: BrepId, f: &[FaceId]) -> Result<()> {
        self.check("remove_faces")?;
        self.inner.remove_faces(b, f)
    }
}
impl PrimitiveAccess for FaultAccess<'_> {
    fn delete_primitive_faces(&mut self, arg1: BrepId, arg2: &[FaceId]) -> Result<()> {
        self.inner.delete_primitive_faces(arg1, arg2)
    }
    fn with_primitive_brep<T>(
        &mut self,
        arg1: BrepId,
        body: impl FnOnce(&mut Kernel, &mut Brep) -> Result<T>,
    ) -> Result<T> {
        self.inner.with_primitive_brep(arg1, body)
    }
    fn axis_inside_face(&mut self, arg1: FaceRef, arg2: &Curve) -> Result<bool> {
        self.inner.axis_inside_face(arg1, arg2)
    }
    fn stitch_and_orient(&mut self, b: BrepId) -> Result<()> {
        self.check("stitch_and_orient")?;
        self.inner.stitch_and_orient(b)
    }
}
impl PrimitiveCreationAccess for FaultAccess<'_> {
    fn copy_faces_into(&mut self, arg1: BrepId, arg2: &[FaceId], arg3: BrepId) -> Result<()> {
        self.inner.copy_faces_into(arg1, arg2, arg3)
    }
    fn attach_offset_map(&mut self, arg1: FaceRef, arg2: OffsetMapAttribute) -> Result<()> {
        self.inner.attach_offset_map(arg1, arg2)
    }
    fn offset_map(&self, arg1: FaceRef) -> Result<Option<OffsetMapAttribute>> {
        self.inner.offset_map(arg1)
    }
    fn project_and_trim(
        &mut self,
        arg1: BrepId,
        arg2: &BSplineCurve,
        arg3: Vec3,
        arg4: Point3,
        arg5: TrimType,
        arg6: f64,
    ) -> Result<()> {
        self.check("project_and_trim")?;
        self.inner
            .project_and_trim(arg1, arg2, arg3, arg4, arg5, arg6)
    }

    fn stitch_faces(&mut self, b: BrepId, tol: f64) -> Result<()> {
        self.check("stitch_faces")?;
        self.inner.stitch_faces(b, tol)
    }
    fn remove_topological_edges_and_vertices(&mut self, b: BrepId) -> Result<()> {
        self.check("remove_topology")?;
        self.inner.remove_topological_edges_and_vertices(b)
    }
    fn is_manifold_solid(&mut self, b: BrepId) -> Result<bool> {
        self.check("manifold")?;
        self.inner.is_manifold_solid(b)
    }
    fn boolean_trees(&mut self, b: &mut Vec<BrepId>, tree: &[i64]) -> Result<()> {
        self.check("boolean_trees")?;
        self.inner.boolean_trees(b, tree)
    }
}

fn inputs() -> (Vec<Curve>, Vec<Curve>) {
    let profile = BSplineCurve::create_circle_segment(
        3,
        &Axis2Placement::IDENTITY,
        1.0,
        0.0,
        360.0,
        NurbCircleParam::Quadratic,
    )
    .unwrap();
    let path = BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::Z * 5.0).unwrap();
    (vec![profile.into()], vec![path.into()])
}
fn enable_audited_fixes(k: &mut Kernel) {
    k.fixes.boolean_temporary_transform_statuses = true;
    k.fixes.primitive_path_curve_statuses = true;
    k.fixes.planar_path_profile_placement_statuses = true;
    k.fixes.planar_path_sweep_statuses = true;
    k.fixes.fillet_corner_statuses = true;
    k.fixes.fillet_edge_curve_domain_statuses = true;
    k.fixes.fillet_trim_status = true;
    k.fixes.offset_tangent_gap_statuses = true;
    k.fixes.ssi_alignment_statuses = true;
}
#[test]
fn audit_483_planar_straight_sweep_keeps_valid_result_with_all_flags() {
    let (profiles, paths) = inputs();
    for combined in [false, true] {
        let mut k = Kernel::default();
        k.fixes.planar_path_sweep_statuses = true;
        if combined {
            enable_audited_fixes(&mut k);
        }
        let mut a = FaultAccess::new(&mut k, ("", 0));
        let result = PrimitiveCreation::create_sweep_along_planar_path(
            &mut a,
            PrimitiveModel::MAIN,
            &profiles,
            &paths,
            1e-6,
            true,
            true,
            true,
        )
        .unwrap();
        let body = a.brep(result).unwrap();
        assert!(crate::topology::brep::p2::is_manifold_solid(body).unwrap());
        assert!(!body.face_list().is_empty());
        assert!(!a.deleted.contains(&result));
        assert!(a.brep(PrimitiveModel::MAIN).is_ok());
    }
}
#[test]
fn audit_483_planar_failures_release_owned_bodies_and_stop() {
    let (profiles, paths) = inputs();
    for stage in [
        ("sweep_start", 1),
        ("remove_faces", 1),
        ("remove_faces", 2),
        ("boolean_trees", 1),
        ("stitch_faces", 1),
        ("remove_topology", 1),
        ("remove_topology", 2),
        ("manifold", 1),
    ] {
        let mut k = Kernel::default();
        enable_audited_fixes(&mut k);
        let mut a = FaultAccess::new(&mut k, stage);
        let result = PrimitiveCreation::create_sweep_along_planar_path(
            &mut a,
            PrimitiveModel::MAIN,
            &profiles,
            &paths,
            1e-6,
            false,
            false,
            true,
        );
        assert_eq!(
            result.unwrap_err(),
            injected(),
            "stage {stage:?}, calls {:?}",
            a.calls
        );
        assert_eq!(a.calls.last(), Some(&stage.0));
        assert!(!a.allocated.is_empty());
        for &id in &a.allocated {
            assert!(a.brep(id).is_err(), "leaked {id:?} at {stage:?}");
        }
        assert!(a.brep(PrimitiveModel::MAIN).is_ok());
        assert!(!a.deleted.contains(&PrimitiveModel::MAIN));
    }
}
#[test]
fn audit_483_planar_trim_stops_before_second_operand_after_failure() {
    let (profiles, _) = inputs();
    let cutting = as_bspline(&profiles[0]).unwrap();
    let mut k = Kernel::default();
    k.fixes.planar_path_sweep_statuses = true;
    let mut a = FaultAccess::new(&mut k, ("project_and_trim", 1));
    let id = a.new_brep(PrimitiveModel::MAIN).unwrap();
    let other = a.new_brep(PrimitiveModel::MAIN).unwrap();
    assert_eq!(
        trim_both(
            &mut a,
            [id, other],
            &cutting,
            Vec3::Z,
            [Vec3::X, Vec3::Y],
            1e-6
        )
        .unwrap_err(),
        injected()
    );
    assert_eq!(a.calls, vec!["project_and_trim"]);
}

#[test]
fn audit_483_checked_sweep_supports_wr400_open_profiles() {
    let (_, paths) = inputs();
    let profiles = [
        BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::X)
            .unwrap()
            .into(),
        BSplineCurve::create_line_segment(3, Vec3::X, Vec3::X + Vec3::Y)
            .unwrap()
            .into(),
    ];
    let mut k = Kernel::default();
    enable_audited_fixes(&mut k);
    k.fixes.planar_path_uncapped_open_profiles = true;
    let mut a = FaultAccess::new(&mut k, ("", 0));
    let id = PrimitiveCreation::create_sweep_along_planar_path(
        &mut a,
        PrimitiveModel::MAIN,
        &profiles,
        &paths,
        1e-6,
        false,
        false,
        true,
    )
    .unwrap();
    assert_eq!(a.brep(id).unwrap().face_list().len(), 2);
    assert!(!a.is_manifold_solid(id).unwrap());
    assert!(!a.deleted.contains(&id));
}

#[test]
fn audit_483_union_failure_releases_every_owned_piece() {
    let (profiles, mut paths) = inputs();
    paths.push(
        BSplineCurve::create_line_segment(3, Vec3::Z * 5.0, Vec3::Z * 10.0)
            .unwrap()
            .into(),
    );
    let mut k = Kernel::default();
    enable_audited_fixes(&mut k);
    let mut a = FaultAccess::new(&mut k, ("boolean_trees", 1));
    assert_eq!(
        PrimitiveCreation::create_sweep_along_planar_path(
            &mut a,
            PrimitiveModel::MAIN,
            &profiles,
            &paths,
            1e-6,
            false,
            false,
            true
        )
        .unwrap_err(),
        injected()
    );
    assert_eq!(a.allocated.len(), 2);
    for &id in &a.allocated {
        assert!(a.brep(id).is_err());
    }
    assert!(a.brep(PrimitiveModel::MAIN).is_ok());
}

#[test]
fn audit_483_failed_mitre_search_is_returned() {
    let profile = BSplineCurve::create_circle_segment(
        3,
        &Axis2Placement::IDENTITY,
        0.05,
        0.0,
        360.0,
        NurbCircleParam::Quadratic,
    )
    .unwrap();
    let paths = [
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::IDENTITY,
            5.0,
            0.0,
            90.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap()
        .into(),
        BSplineCurve::create_line_segment(3, Vec3::Y * 5.0, Vec3::Y * 10.0)
            .unwrap()
            .into(),
    ];
    let mut k = Kernel::default();
    enable_audited_fixes(&mut k);
    let mut a = FaultAccess::new(&mut k, ("recover_search", 0));
    let result = PrimitiveCreation::create_sweep_along_planar_path(
        &mut a,
        PrimitiveModel::MAIN,
        &[profile.into()],
        &paths,
        1e-6,
        false,
        false,
        true,
    );
    assert_eq!(result.unwrap_err(), injected(), "{:?}", a.calls);
    // fork c8f6a31 (`CreateSweepAlongPlanarPath`, `SER(pBrepBefore->BrepIntersect(..))`,
    // #2009): the failed mitre intersection search is returned, before any trim.
    assert_eq!(a.calls.last(), Some(&"with_breps_mut"));
    assert!(!a.calls.contains(&"project_and_trim"));
    for &id in &a.allocated {
        assert!(a.brep(id).is_err());
    }
}

/// A closed unit square of lines in the XY plane (the taper profile).
fn square() -> Vec<Curve> {
    let c = [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::Y, Vec3::Y, Vec3::ZERO];
    c.windows(2)
        .map(|w| {
            BSplineCurve::create_line_segment(3, w[0], w[1])
                .unwrap()
                .into()
        })
        .collect()
}

fn taper_with(pass_on: bool, fail: (&'static str, usize)) -> (Result<()>, Vec<&'static str>) {
    let mut k = Kernel::default();
    k.fixes.sweep_statuses_pass_on = pass_on;
    let mut a = FaultAccess::new(&mut k, fail);
    let region = a.infinite_region(PrimitiveModel::MAIN).unwrap();
    a.calls.clear();
    let mut primitive = PrimitiveCreation::new(Some(region), 0, false);
    let mut normal = Vec3::Z;
    let r = primitive.create_taper_extrude(
        &mut a,
        PrimitiveModel::MAIN,
        square(),
        1.0,
        5.0,
        3,
        &mut normal,
        crate::curve::composite_curve::OffsetCornerType::LinearExtension,
    );
    (r, a.calls)
}

/// fork c8f6a31 (`CreateTaperExtrude`, `SER(pBrep->StitchFaces(dBrepTol, ..))`, #2009):
/// the scratch B-rep's failed stitch is returned with `sweep_statuses_pass_on`;
/// upstream goes on with the unstitched faces.
#[test]
fn issue_2009_taper_passes_on_a_failed_scratch_stitch() {
    let (off, calls) = taper_with(false, ("stitch_faces", 1));
    assert!(calls.contains(&"stitch_faces"), "{calls:?}");
    assert_ne!(off.err(), Some(injected()));
    let (on, _) = taper_with(true, ("stitch_faces", 1));
    assert_eq!(on.unwrap_err(), injected());
}

/// fork c8f6a31 (`CreateTaperExtrude`, `SER(pBrep->StitchAndOrient())`, #2009): the
/// final stitch-and-orient's failure is returned with `sweep_statuses_pass_on`;
/// upstream reports success.
#[test]
fn issue_2009_taper_passes_on_a_failed_stitch_and_orient() {
    let (off, calls) = taper_with(false, ("stitch_and_orient", 1));
    assert!(calls.contains(&"stitch_and_orient"), "{calls:?}");
    assert!(off.is_ok(), "{off:?}");
    let (on, _) = taper_with(true, ("stitch_and_orient", 1));
    assert_eq!(on.unwrap_err(), injected());
}

/// fork c8f6a31 (`CreatePipeSweep`, `SER(SmCircle::CreateCanonical(..))`, #2009): a pipe
/// radius the circle refuses (zero) is that circle's error with
/// `sweep_statuses_pass_on`; upstream sweeps the null profile it leaves.
#[test]
fn issue_2009_pipe_sweep_passes_on_a_failed_circle() {
    let path = BSplineCurve::create_line_segment(3, Vec3::ZERO, Vec3::Z * 5.0).unwrap();
    let circle_error = |pass_on: bool| {
        let mut k = Kernel::default();
        k.fixes.sweep_statuses_pass_on = pass_on;
        let mut a = FaultAccess::new(&mut k, ("", 0));
        let region = a.infinite_region(PrimitiveModel::MAIN).unwrap();
        let mut primitive = PrimitiveCreation::new(Some(region), 0, false);
        let (mut s, mut d, mut e) = (Vec::new(), Vec::new(), Vec::new());
        let r = primitive.create_pipe_sweep(
            &mut a,
            PrimitiveModel::MAIN,
            0.0,
            &path,
            0.0,
            true,
            false,
            &mut s,
            &mut d,
            &mut e,
        );
        matches!(
            r,
            Err(SolidError::Upstream { operation, .. }) if operation.contains("ellipse")
        )
    };
    assert!(!circle_error(false));
    assert!(circle_error(true));
}
