//! Control-flow tests for executive p3. Dependency responses are deliberately
//! scripted here; these tests do not establish C++ geometric parity.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::boolean::topology_intersector::p1::{IntersectionResults, IntersectionTable};
use crate::geom::solution_array::ObjectKind;
use crate::intersect::point_class::ClassObjectKind;
use std::cell::Cell;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Handle(u8);
impl ClassObject for Handle {
    fn class_kind(&self) -> ClassObjectKind {
        if (10..=13).contains(&self.0) {
            ClassObjectKind::Edge
        } else {
            ClassObjectKind::Other
        }
    }
}
impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Other
    }
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // independent scripted callback scenarios
struct Access {
    process_error: Option<SolidError>,
    mapped_edges: bool,
    corner_enables_global: bool,
    degenerate_edge: Option<Handle>,
    insert_live_state: bool,
    geoms: Vec<Handle>,
    append_geom: bool,
    geom_snapshots: Cell<usize>,
    geom_reads: Cell<usize>,
    created_attributes: usize,
    attributes: BTreeMap<Handle, Vec<Handle>>,
    removed_edges: Vec<Option<Handle>>,
    marked_pairs: Vec<(Handle, Handle)>,
    orient_error: Option<SolidError>,
    merge_error: Option<SolidError>,

    piecewise_error: Option<SolidError>,
    stitch_error: bool,
    editing: bool,
    fillet_editing: bool,
    events: Vec<&'static str>,
    stitch_tolerances: Vec<f64>,
    solver_tolerance: Option<f64>,
    brep_tolerances: Vec<f64>,
    stitch_lamina: u64,
    /// What `keeps_open_result` answers.
    keeps_open: bool,
    /// What the open body's own orientation (`just_unify`) returns.
    unify_error: Option<SolidError>,
    corner_metadata: Option<CornerView<Handle>>,
    corner_predicates: [bool; 2],
    corner_point: Point3,
    constant_results: Vec<bool>,
    corner_fail: Option<&'static str>,
    /// The error each scripted stage fails with, by the stage's name ([`Access::stage`]).
    stage_errors: BTreeMap<&'static str, SolidError>,
    /// The corner the scripted rollover test adjusts (else it fails, as `absent`).
    rollover_adjusts: Option<Handle>,
}
impl Access {
    /// The result of the stage `name`: its scripted error, if any. A failure at `topology`
    /// notes the corner's refusal (`SM_FILERR_NO_ROOM`) when `topology_noted` is scripted,
    /// or that the corner is not built (`CornerUnsupported`) when `topology_unsupported` is;
    /// `vertices_again` fails only the second vertex geometry of a run, `stitch_faces` the
    /// fillet body's stitch and `do_stitching` every stitch into the target.
    fn stage(&self, name: &'static str) -> Result<()> {
        self.stage_errors.get(name).cloned().map_or(Ok(()), Err)
    }
}
fn absent<T>() -> Result<T> {
    Err(invalid("test handle has no geometry"))
}
impl ExecutiveAccess<Handle> for Access {
    fn solver_base(&self, _: Handle) -> Result<&crate::blend::fillet_solver::p1::FilletSolver> {
        Err(SolidError::invalid_input(
            "test::executive_access",
            "no retained solver",
        ))
    }
    fn solver_base_mut(
        &mut self,
        _: Handle,
    ) -> Result<&mut crate::blend::fillet_solver::p1::FilletSolver> {
        Err(SolidError::invalid_input(
            "test::executive_access",
            "no retained solver",
        ))
    }
    fn solver_has_surface(&self, _: Handle, _: usize) -> Result<bool> {
        Err(SolidError::invalid_input(
            "test::executive_access",
            "no retained solver",
        ))
    }
    fn edge_g1_knots(&mut self, _: &Kernel, _: Handle) -> Result<Vec<f64>> {
        Err(SolidError::invalid_input(
            "test::executive_access",
            "no retained solver",
        ))
    }
    fn edgeuse_vertex(&self, _: Handle) -> Result<Option<Handle>> {
        Err(SolidError::invalid_input(
            "test::executive_access",
            "no retained solver",
        ))
    }
    fn reinitialize_corner_edges(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        absent()
    }
    fn corner_tangent_solver(&self, _: Handle, _: Handle) -> Result<Option<Handle>> {
        absent()
    }
    fn make_corner_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("corner_topology");
        if self.corner_enables_global {
            executive.options.global_merge = true;
        }
        if let Some(e) = self.stage_errors.get("topology_noted") {
            executive.error.code = FilletErrorType::NoRoom as i32;
            return Err(e.clone());
        }
        if let Some(e) = self.stage_errors.get("topology_unsupported") {
            executive.error.code = FilletErrorType::CornerUnsupported as i32;
            return Err(e.clone());
        }
        self.stage("topology")
    }
    fn corner_error_info(
        &self,
        _: Handle,
        _: &FilletExecutive<Handle>,
    ) -> Result<CornerView<Handle>> {
        self.corner_metadata
            .clone()
            .ok_or_else(|| invalid("no corner metadata"))
    }
    fn is_blending_corner(&self, _: Handle) -> Result<bool> {
        Ok(self.corner_predicates[0])
    }
    fn is_tangent_corner(&self, _: Handle) -> Result<bool> {
        assert!(self.corner_predicates[0]);
        Ok(self.corner_predicates[1])
    }
    fn corner_solvers(&self, _: Handle, _: &FilletExecutive<Handle>) -> Result<Vec<Handle>> {
        assert!(self.corner_predicates[0] && self.corner_predicates[1]);
        // Script the upstream repeated-first-edgeuse lookup, not distinct solvers.
        Ok(vec![Handle(3), Handle(3)])
    }
    fn corner_vertex(&self, _: Handle) -> Result<Option<Handle>> {
        Ok(Some(Handle(6)))
    }
    fn corner_approx_tolerance(&self, _: Handle) -> Result<f64> {
        Ok(0.125)
    }
    fn solver_geoms(&self, _: Handle) -> Result<Vec<Handle>> {
        self.geom_snapshots.set(self.geom_snapshots.get() + 1);
        Ok(self.geoms.clone())
    }
    fn solver_geom(&self, _: Handle, index: usize) -> Result<Option<Handle>> {
        self.geom_reads.set(self.geom_reads.get() + 1);
        Ok(self.geoms.get(index).copied())
    }
    fn solver_edge(&self, _: Handle) -> Result<Handle> {
        Ok(Handle(5))
    }
    // scripted: every rail and end section on its face (no cliff overflow to choose)
    fn solver_rails_fit_judged(&mut self, _: &mut Kernel, _: Handle) -> Result<RailsFit> {
        Ok(RailsFit {
            fit: true,
            touches_rim: false,
            rolls_over: false,
            into_material: false,
            boundaries_into_material: false,
            left_across: Vec::new(),
            return_across: Vec::new(),
            continued_cliffs: Vec::new(),
            wraps_past_both_ends: false,
        })
    }
    fn solver_end_sections_fit(&mut self, _: &mut Kernel, _: Handle) -> Result<bool> {
        Ok(true)
    }
    fn solver_approx_tolerance(&self, _: Handle) -> Result<f64> {
        Ok(self.solver_tolerance.unwrap_or(0.125))
    }
    fn geom_face(&self, _: Handle) -> Result<Option<Handle>> {
        Ok(Some(Handle(2)))
    }
    fn topo_edge(&self, edge: Handle) -> Result<TopoEdgeView<Handle>> {
        assert_eq!(edge, Handle(20));
        Ok(TopoEdgeView {
            on_edge: true,
            has_curve: true,
            brep_edge: Some(Handle(11)),
        })
    }
    fn vertex_point(&self, _: Handle) -> Result<Point3> {
        Ok(self.corner_point)
    }
    fn edge_curve_ends(&self, _: &Kernel, _: Handle) -> Result<(Point3, Point3)> {
        Ok((Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)))
    }
    fn set_calc_as_const_rad(&mut self, _: Handle, value: bool) -> Result<()> {
        self.constant_results.push(value);
        Ok(())
    }
    fn new_brep(&mut self, _: &Kernel) -> Result<Handle> {
        self.events.push("new");
        Ok(Handle(1))
    }
    fn delete_brep(&mut self, _: Handle) -> Result<()> {
        self.events.push("delete");
        Ok(())
    }
    fn tolerance(&self, _: &Kernel, _: Handle, _: bool) -> Result<f64> {
        Ok(0.125)
    }
    fn editing_enabled(&self, _: Handle) -> Result<bool> {
        Ok(self.editing)
    }
    fn set_editing_enabled(&mut self, brep: Handle, value: bool) -> Result<()> {
        if brep == Handle(0) {
            self.editing = value;
        } else {
            self.fillet_editing = value;
        }
        Ok(())
    }
    fn edges(&self, brep: Handle) -> Result<Vec<Handle>> {
        Ok(if !self.mapped_edges {
            vec![]
        } else if brep == Handle(0) {
            vec![Handle(10), Handle(12)]
        } else {
            vec![Handle(11), Handle(13)]
        })
    }
    fn create_corners(
        &mut self,
        _: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
        _: Option<&mut ErrorInfo<Handle>>,
    ) -> Result<()> {
        self.events.push("corners");
        Ok(())
    }
    fn vertices(&self, _: Handle) -> Result<Vec<Handle>> {
        Ok(vec![])
    }
    fn faces(&self, _: Handle) -> Result<Vec<Handle>> {
        Ok(vec![Handle(2)])
    }
    fn add_attribute(&mut self, edge: Handle, attribute: Option<Handle>) -> Result<()> {
        if let Some(attribute) = attribute {
            self.attributes.get_mut(&attribute).unwrap().push(edge);
        }
        Ok(())
    }
    fn new_keep_attribute(&mut self, _: &Kernel) -> Result<Handle> {
        let attribute = Handle(u8::try_from(30 + self.created_attributes).unwrap());
        self.created_attributes += 1;
        self.attributes.insert(attribute, vec![]);
        self.events.push("new_attr");
        Ok(attribute)
    }
    fn attribute_users(&self, attribute: Handle) -> Result<Vec<Handle>> {
        Ok(self.attributes.get(&attribute).unwrap().clone())
    }
    fn delete_attribute(&mut self, attribute: Handle) -> Result<()> {
        self.attributes.remove(&attribute).unwrap();
        self.events.push("delete_attr");
        Ok(())
    }
    fn with_marks<T>(
        &mut self,
        k: &mut Kernel,
        _: Handle,
        _: Handle,
        mates: &[(Handle, Handle)],
        body: impl FnOnce(&mut Self, &mut Kernel, &[ContextMark]) -> Result<T>,
    ) -> Result<T> {
        self.events.push("marks");
        self.marked_pairs = mates.to_vec();
        body(self, k, &[])
    }
    fn shrink_face(&mut self, _: &mut Kernel, _: Handle) -> Result<()> {
        absent()
    }
    fn notify_face(
        &mut self,
        _: &mut Kernel,
        face: Handle,
        operation: NotifyOperation,
        data1: Option<Handle>,
        data2: Option<Handle>,
        data3: Option<Handle>,
    ) -> Result<()> {
        assert_eq!(operation, NotifyOperation::PreEdit);
        assert_eq!(data1, Some(face));
        assert!(data2.is_none());
        assert!(data3.is_none());
        self.events.push("notify");
        Ok(())
    }
    fn boolean_post_process(
        &mut self,
        _: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
        all: bool,
    ) -> Result<()> {
        assert!(all);
        self.events.push("post");
        Ok(())
    }
    fn process_smooth_edge_sequences(
        &mut self,
        k: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("smooth");
        // DoFilleting sets this flag before invoking CreateFilletBrep.
        let _ = k.context.doing_boolean();
        self.process_error.clone().map_or(Ok(()), Err)
    }
    fn set_brep_tolerance(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        tol: f64,
        larger: bool,
        cascade: bool,
    ) -> Result<()> {
        self.brep_tolerances.push(tol);
        assert!(larger && cascade);
        self.events.push("tolerance");
        Ok(())
    }
    fn stitch_faces(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: f64,
        faces: Option<&[Handle]>,
        spine: bool,
    ) -> Result<StitchAttempt> {
        assert!(faces.is_none());
        assert!(!spine);
        self.events.push("stitch_faces");
        Ok(StitchAttempt {
            status: self.stage("stitch_faces").err(),
            stitched: 0,
            lamina: 0,
            max_vertex_gap: 0.0,
            max_edge_gap: 0.0,
        })
    }
    // the fake has no blend geometry: no face is taken whole (WR-320)
    fn solvers_faces_outlast(&mut self, _: &mut Kernel, _: &[Handle]) -> Result<bool> {
        Ok(true)
    }
    // nor do any two blends' bands overlap (WR-542)
    fn solvers_bands_clear_other_rails(&mut self, _: &mut Kernel, _: &[Handle]) -> Result<bool> {
        Ok(true)
    }
    fn piecewise_merge(&mut self, k: &mut Kernel, _: &mut FilletExecutive<Handle>) -> Result<()> {
        assert!(k.context.doing_boolean());
        assert!(self.editing);
        self.events.push("piecewise");
        self.piecewise_error.clone().map_or(Ok(()), Err)
    }
    fn set_fillet_brep_edge_map(
        &mut self,
        _: &mut Kernel,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("edge_map");
        Ok(())
    }
    fn delete_faces(
        &mut self,
        _: &mut Kernel,
        _: &mut TopologyIntersector<Handle>,
        _: &[Handle],
        keep: Option<&[Handle]>,
    ) -> Result<()> {
        assert!(keep.is_none());
        self.events.push("delete_faces");
        Ok(())
    }
    fn merge_brep(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: Handle,
        copy: Option<&mut CopyBrepMap<Handle>>,
        reverse: Option<&mut CopyBrepMap<Handle>>,
        analytics: bool,
    ) -> Result<()> {
        assert!(copy.is_none() && reverse.is_none() && analytics);
        self.events.push("merge");
        self.merge_error.clone().map_or(Ok(()), Err)
    }
    fn do_stitching(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        tolerance: f64,
        vertices: Option<&[Handle]>,
        edges: Option<&[Handle]>,
        settings: StitchSettings,
        min_vertex: Option<&mut f64>,
        min_edge: Option<&mut f64>,
    ) -> Result<StitchAttempt> {
        assert!(
            vertices.is_none() && edges.is_none() && min_vertex.is_none() && min_edge.is_none()
        );
        assert!(settings.do_region_nesting && settings.ignore_problems && settings.validate_result);
        assert!(settings.split_edges_with_vertices && settings.making_manifold_solid);
        assert!(
            !settings.fast_edge_compare
                && !settings.squeeze_small_edges
                && !settings.remove_laminar_slivers
        );
        self.stitch_tolerances.push(tolerance);
        Ok(StitchAttempt {
            status: self
                .stage("do_stitching")
                .err()
                .or_else(|| self.stitch_error.then(failure)),
            stitched: 0,
            lamina: self.stitch_lamina,
            max_vertex_gap: 0.0,
            max_edge_gap: 0.0,
        })
    }
    fn remove_topo_edges(
        &mut self,
        _: &mut Kernel,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.removed_edges = executive.topo_edges.clone();
        self.events.push("remove_topo");
        Ok(())
    }
    fn orient_trimmed_surfaces(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        outward: bool,
        solid: &mut bool,
        unify: bool,
        faces: Option<&mut Vec<Handle>>,
    ) -> Result<()> {
        assert!(outward && !*solid && faces.is_none());
        if unify {
            self.events.push("unify");
            return self.unify_error.clone().map_or(Ok(()), Err);
        }
        self.events.push("orient");
        if self.created_attributes > 0 {
            assert!(
                !self.attributes.is_empty(),
                "orientation must see temporary attributes"
            );
        }
        self.orient_error.clone().map_or(Ok(()), Err)
    }
    fn keeps_open_result(&mut self, _: &mut Kernel, _: Handle, _: &SolidError) -> Result<bool> {
        self.events.push("keeps_open");
        Ok(self.keeps_open)
    }
    fn calc_corner_vertices(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("corner_vertices");
        if self.corner_fail == Some("vertices") {
            Err(failure())
        } else if self
            .events
            .iter()
            .filter(|e| **e == "corner_vertices")
            .count()
            > 1
        {
            self.stage("vertices_again")
        } else {
            self.stage("vertices")
        }
    }
    fn check_degeneracy(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<bool> {
        self.events.push("degeneracy");
        if let Some(edge) = self.degenerate_edge {
            executive
                .topo_edges
                .retain(|candidate| *candidate != Some(edge));
            return Ok(true);
        }
        Ok(false)
    }
    fn adjust_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("adjust");
        self.stage("adjust")
    }
    fn adjust_tangent_corner(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("adjust_tangent");
        self.stage("adjust_tangent")
    }
    fn calc_corner_edges(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("corner_edges");
        self.stage("edges")
    }
    fn calc_fillet_geom(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.stage("solver")?;
        executive.error.code = 6;
        Ok(())
    }
    fn test_rollover(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        adjusted: &mut Vec<Handle>,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        let corner = self
            .rollover_adjusts
            .ok_or_else(|| invalid("test handle has no geometry"))?;
        adjusted.push(corner);
        Ok(())
    }
    fn trim_rails(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.stage("trim")
    }
    fn make_face_brep(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: Handle,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        assert!(executive.fillet.is_some());
        self.events.push("make_face");
        self.stage("make_face")?;
        if self.append_geom {
            self.append_geom = false;
            self.geoms.push(Handle(9));
        }
        Ok(())
    }
    fn intersect_insert_relate(
        &mut self,
        _: &mut Kernel,
        _: &mut TopologyIntersector<Handle>,
    ) -> Result<()> {
        self.events.push("global");
        Ok(())
    }
    fn insert_geom_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        if self.insert_live_state {
            executive
                .intersector
                .seed_mates_for_test(&[(Handle(10), Handle(11))]);
            let mut table = IntersectionTable::default();
            table.brep_surfaces.push(Some(Handle(8)));
            table.other_surfaces.push(Some(Handle(9)));
            table.results.push(IntersectionResults::default());
            executive.intersector.set_intersection_table(Some(table));
        }
        Ok(())
    }
    fn insert_corner_topology(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        if self.insert_live_state {
            assert_eq!(
                executive
                    .intersector
                    .intersection_table()
                    .unwrap()
                    .brep_surfaces,
                [Some(Handle(8))]
            );
            executive
                .intersector
                .seed_mates_for_test(&[(Handle(12), Handle(13))]);
        }
        Ok(())
    }
    fn trim_original_surfaces(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        kept: &mut Vec<Handle>,
        _: &mut Vec<Handle>,
        _: &[ContextMark],
        executive: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        if self.insert_live_state {
            assert_eq!(
                executive.intersector.other_mate(Some(Handle(12))),
                Some(Handle(13))
            );
        }
        kept.push(Handle(2));
        Ok(())
    }
    fn calc_corner_geom(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        self.events.push("corner_geom");
        self.stage("corner_geom")
    }
    fn geom_has_surface(&self, _: Handle) -> Result<bool> {
        absent()
    }
    fn geom_rails(&self, _: Handle) -> Result<Vec<Option<Handle>>> {
        absent()
    }
    fn geom_side_edgeuses(&self, _: Handle) -> Result<Vec<Handle>> {
        absent()
    }
    fn edge_edgeuses(&self, _: Handle) -> Result<Vec<Handle>> {
        absent()
    }
    fn fillet_edge_note(
        &self,
        _: Handle,
    ) -> Result<(Option<&'static str>, FilletEdgeType, FilStatus)> {
        absent()
    }
    fn new_executive_objects(&mut self, _: &mut Kernel) -> Result<ExecutiveObjects<Handle>> {
        absent()
    }
    fn edgeuse_edge(&self, _: Handle) -> Result<Option<Handle>> {
        absent()
    }
    fn fillet_edge_original_face(&self, _: Handle) -> Result<Option<Handle>> {
        absent()
    }
    fn fillet_edge_original_edge(&self, _: Handle) -> Result<Option<Handle>> {
        absent()
    }
    fn find_fillet_brep_edge(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &[Handle],
        _: bool,
    ) -> Result<()> {
        absent()
    }
    fn corner_edges(&self, _: Handle) -> Result<Vec<Option<Handle>>> {
        absent()
    }
    fn blend_edgeuse(&mut self, _: &mut Kernel, _: Handle) -> Result<Option<Handle>> {
        absent()
    }
    fn edge_tolerance(&self, _: Handle) -> Result<f64> {
        absent()
    }
    fn edge_vertices(&self, _: Handle) -> Result<(Option<Handle>, Option<Handle>)> {
        absent()
    }
    fn variable_radius_solver(
        &self,
        _: Handle,
    ) -> Result<Option<&crate::blend::fillet_standard_solver::p1::VariableRadiusFs>> {
        absent()
    }
    fn variable_radius_solver_mut(
        &mut self,
        _: Handle,
    ) -> Result<Option<&mut crate::blend::fillet_standard_solver::p1::VariableRadiusFs>> {
        absent()
    }
    fn solver_edgeuse(&self, _: Handle, _: usize) -> Result<Option<Handle>> {
        absent()
    }
    fn solver_corner(&self, _: Handle, _: usize) -> Result<Option<Handle>> {
        absent()
    }
    fn solver_other_corner(&self, _: Handle, _: Handle) -> Result<Option<Handle>> {
        absent()
    }
    fn corner_setback_distance(&self, _: Handle) -> Result<f64> {
        absent()
    }
    fn new_surface_generator(
        &mut self,
        _: &mut Kernel,
        _: FilletSurfaceGenerator,
    ) -> Result<Handle> {
        absent()
    }
    fn new_constant_radius_solver(
        &mut self,
        _: &mut Kernel,
        _: f64,
        _: f64,
        _: f64,
        _: f64,
        _: Handle,
    ) -> Result<Handle> {
        absent()
    }
    fn set_surface_generator(&mut self, _: Handle, _: Handle) -> Result<()> {
        absent()
    }
    fn new_fillet_solver(
        &mut self,
        _: &mut Kernel,
        _: &crate::blend::fillet_executive::p2::SolverSpec<'_, Handle>,
    ) -> Result<Handle> {
        absent()
    }
}

fn state() -> FilletExecutive<Handle> {
    FilletExecutive {
        target: Some(Handle(0)),
        fillet: None,
        corners: vec![],
        solvers: vec![],
        topo_edges: vec![],
        edge_attribute: None,
        initialized: true,
        options: ExecutionOptions {
            piecewise_merge: false,
            topology_insertion: false,
            global_merge: false,
            trimming: false,
            classification: false,
        },
        error: ErrorInfo::default(),
        intersector: TopologyIntersector::new(None, None, 1e-5, 0.5),
        intersection_edges: vec![],
        intersection_vertices: vec![],
        law_sequence_edges: vec![],
        pseudo_brep: None,
        surface_generators: vec![],
        filleted_vertices: Vec::new(),
        vertex_corner_map: BTreeMap::new(),
        edgeuse_solver_map: BTreeMap::new(),
        self_intersection_handler: None,
        mark_corner_patches: false,
        cliff_overflow: false,
        continued_cliffs: BTreeMap::new(),
        open_ends: Vec::new(),
        overflow_first: false,
        classify_only: false,
        roll_over_pass: false,
        overflow_edges: Vec::new(),
        mitred_edges: Vec::new(),
        fan_split_points: Vec::new(),
        earlier_pass_faces: Vec::new(),
        fit_radius: -1.0,
        fit_edge: None,
        fit_kind: FitKind::Radius,
        fit_setback: -1.0,
        rollover_adjusting: false,
    }
}

#[test]
fn uninitialized_changes_only_generic_error_members() {
    let mut executive = state();
    executive.initialized = false;
    executive.error.status = 9;
    let mut access = Access::default();
    let mut k = Kernel::default();
    assert!(matches!(
        executive.do_fillet(&mut k, &mut access),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. })
    ));
    assert_eq!(executive.error.code, 1);
    assert_eq!(executive.error.status, 9);
    assert_eq!(
        executive.error.comment,
        "ERROR: fillet parameters not set up."
    );
    assert!(access.events.is_empty());
    assert!(!executive.intersector.doing_fillets);
}

#[test]
fn null_create_status_is_remapped_by_do_filleting_and_boolean_restores() {
    let mut executive = state();
    executive.target = None;
    let mut access = Access::default();
    let mut k = Kernel::default();
    assert!(matches!(
        executive.create_fillet_brep(&mut k, &mut access, None),
        Err(ref e) if e.failed_routine_status() == Some(1006)
    ));
    for prior in [false, true] {
        k.context.set_doing_boolean(prior);
        assert!(matches!(
            executive.do_filleting(&mut k, &mut access, None),
            Err(ref e) if e.failed_routine_status() == Some(1001)
        ));
        assert_eq!(k.context.doing_boolean(), prior);
    }
    assert!(executive.intersector.doing_fillets);
}

#[test]
fn ordinary_smooth_error_is_ignored_and_tail_and_selective_assignment_run() {
    let mut executive = state();
    executive.error.code = 6;
    let mut output = ErrorInfo {
        code: 5,
        comment: "keep comment".into(),
        side_face: Some(Handle(7)),
        ..ErrorInfo::default()
    };
    let mut access = Access {
        process_error: Some(failure()),
        ..Access::default()
    };
    executive
        .create_fillet_brep(&mut Kernel::default(), &mut access, Some(&mut output))
        .unwrap();
    assert_eq!(
        access.events,
        ["smooth", "new", "tolerance", "stitch_faces"]
    );
    assert!(access.fillet_editing);
    assert_eq!(executive.fillet, Some(Handle(1)));
    assert_eq!(output.code, 6);
    assert_eq!(output.comment, "keep comment");
    assert_eq!(output.side_face, Some(Handle(7)));
}

#[test]
fn deferral_is_never_swallowed_or_remapped_and_restores_boolean() {
    let expected = SolidError::unported(
        "SmFilletExecutive::ProcessSmoothEdgeSequences",
        "blend.fillet_executive_p2",
    );
    let mut access = Access {
        process_error: Some(expected.clone()),
        ..Access::default()
    };
    let mut executive = state();
    let mut k = Kernel::default();
    assert_eq!(
        executive.do_filleting(&mut k, &mut access, None),
        Err(expected)
    );
    assert_eq!(access.events, ["smooth"]);
    assert!(!k.context.doing_boolean());
    assert!(executive.intersector.doing_fillets);
    assert!(executive.fillet.is_none());
}

#[test]
fn piecewise_return_restores_both_temporary_flags_on_success_and_error() {
    for error in [
        None,
        Some(failure()),
        Some(SolidError::unported(
            "SmFilletExecutive::DoPiecewiseMerge",
            "blend.fillet_executive_p1",
        )),
    ] {
        let mut executive = state();
        executive.options.piecewise_merge = true;
        let mut access = Access {
            piecewise_error: error.clone(),
            ..Access::default()
        };
        let mut k = Kernel::default();
        assert_eq!(
            executive.do_filleting(&mut k, &mut access, None),
            error.map_or(Ok(()), Err)
        );
        assert!(!access.editing);
        assert!(!k.context.doing_boolean());
        assert_eq!(access.events.last(), Some(&"piecewise"));
        assert!(!access.events.contains(&"edge_map"));
    }
}

/// The stitch is retried five times with doubled tolerances; then the orientation's
/// failure is the fillet's (fork 873c7c9, `SER( m_pTargetBrep->OrientTrimmedSurfaces(..) )`;
/// upstream ignores it, WR-661). The one failure passed over is the closed-solid check's
/// "face uses gathered from both sides" on a result left open (lamina edges), and only
/// when `keeps_open_result` keeps that body (an open input whose boundary the result
/// keeps, level-2 clean) and the open body's own orientation (outward, just unifying the
/// normals, as `StitchAndOrient` runs it for lamina) then succeeds; otherwise the
/// closed-solid check's failure is returned. No other failure asks.
#[test]
fn stitching_retries_exactly_five_times_doubles_tolerance_then_passes_on_orientation_error() {
    let gathered = SolidError::topology(
        "topology::brep::p3",
        crate::error::topology::TopologyFailure::GatheredBothSides,
    );
    // (lamina, orientation error, keeps_open answer, unify error, asked, kept)
    for (lamina, error, keeps_open, unify_error, asked, kept) in [
        (1, failure(), true, None, false, false),
        (1, gathered.clone(), true, None, true, true),
        // the open body's own orientation fails: the closed-solid failure is returned
        (1, gathered.clone(), true, Some(failure()), true, false),
        // the open result is not kept (closed input left open, a gap, level-2 failure)
        (1, gathered.clone(), false, None, true, false),
        (0, gathered.clone(), true, None, false, false),
    ] {
        let unifies = asked && keeps_open;
        let mut executive = state();
        executive.options.trimming = true;
        let mut access = Access {
            stitch_lamina: lamina,
            orient_error: Some(error.clone()),
            keeps_open,
            unify_error,
            ..Access::default()
        };
        let mut k = Kernel::default();
        let result = executive.do_filleting(&mut k, &mut access, None);
        if kept {
            assert_eq!(result, Ok(()), "lamina {lamina}");
        } else {
            assert_eq!(result, Err(error.clone()), "lamina {lamina}");
        }
        assert_eq!(
            access.events.contains(&"keeps_open"),
            asked,
            "lamina {lamina} keeps_open {keeps_open}"
        );
        access.events.retain(|event| *event != "keeps_open");
        assert_eq!(
            access.events.last() == Some(&"unify"),
            unifies,
            "lamina {lamina} keeps_open {keeps_open}"
        );
        access.events.retain(|event| *event != "unify");
        let tolerances: &[f64] = if lamina == 0 {
            &[0.125]
        } else {
            &[0.125, 0.25, 0.5, 1.0, 2.0]
        };
        assert_eq!(access.stitch_tolerances, tolerances, "lamina {lamina}");
        assert_eq!(access.events.last(), Some(&"orient"), "lamina {lamina}");
        assert!(access.events.contains(&"post"));
        assert!(executive.fillet.is_none());
        assert!(executive.intersector.other_brep().is_none());
        assert!(!access.editing);
    }
}

#[test]
fn final_stitch_failure_sets_bad_merge_and_stops_before_cleanup_postprocess() {
    let mut executive = state();
    executive.options.trimming = true;
    let mut access = Access {
        stitch_error: true,
        ..Access::default()
    };
    let mut k = Kernel::default();
    assert!(matches!(
        executive.do_filleting(&mut k, &mut access, None),
        Err(ref e) if e.failed_routine_status() == Some(1001)
    ));
    assert_eq!(access.stitch_tolerances.len(), 5);
    assert_eq!(executive.error.code, 7);
    assert!(!access.events.contains(&"post"));
    assert!(!access.editing);
    assert!(!k.context.doing_boolean());
}

fn corner() -> CornerView<Handle> {
    CornerView {
        vertex: Some(Handle(6)),
        corner_type: FilletCornerType::NByTwo,
        name: "corner name".into(),
    }
}

#[test]
fn tangent_corner_constant_radius_uses_both_distances_and_twice_approx_tolerance() {
    for (point, expected) in [
        (0.0, true),
        (0.25, true),
        (0.2500000001, false),
        (5.0, false),
        (9.75, true),
        (10.0, true),
    ] {
        let mut executive = state();
        executive.corners.push(Handle(7));
        let mut access = Access {
            corner_metadata: Some(corner()),
            corner_predicates: [true, true],
            corner_point: Point3::new(point, 0.0, 0.0),
            ..Access::default()
        };
        executive
            .create_fillet_brep(&mut Kernel::default(), &mut access, None)
            .unwrap();
        assert_eq!(access.constant_results, [expected]);
        assert_eq!(
            access.events,
            [
                "smooth",
                "corner_topology",
                "corner_vertices",
                "degeneracy",
                "adjust",
                "adjust_tangent",
                "corner_vertices",
                "corner_edges",
                "new",
                "tolerance",
                "corner_geom",
                "stitch_faces"
            ]
        );
    }
}

#[test]
fn corner_failure_records_context_and_stops_without_allocating_shell() {
    let mut executive = state();
    executive.corners.push(Handle(7));
    let mut access = Access {
        corner_metadata: Some(corner()),
        corner_fail: Some("vertices"),
        ..Access::default()
    };
    assert!(matches!(
        executive.create_fillet_brep(&mut Kernel::default(), &mut access, None),
        Err(ref e) if e.failed_routine_status() == Some(1001)
    ));
    assert_eq!(executive.error.code, 3);
    assert_eq!(executive.error.status, 2);
    assert_eq!(executive.error.corner_type, FilletCornerType::NByTwo);
    assert_eq!(executive.error.vertex, Some(Handle(6)));
    assert_eq!(executive.error.surfaces, [None]);
    assert_eq!(executive.error.entity_name, "corner name");
    assert_eq!(
        executive.error.comment,
        "Fillet Corner Error: Unable to Compute Geometry"
    );
    assert!(!access.events.contains(&"new"));
}

#[test]
fn nonblending_or_nontangent_corners_do_not_read_solver_metadata() {
    for predicates in [[false, true], [true, false]] {
        let mut executive = state();
        executive.corners.push(Handle(7));
        let mut access = Access {
            corner_predicates: predicates,
            ..Access::default()
        };
        executive
            .create_fillet_brep(&mut Kernel::default(), &mut access, None)
            .unwrap();
        assert_eq!(access.constant_results, [predicates[0]]);
    }
}

#[test]
fn corner_error_reporting_preserves_a_null_filleted_vertex() {
    let mut executive = state();
    executive.corners.push(Handle(7));
    let mut metadata = corner();
    metadata.vertex = None;
    let mut access = Access {
        corner_metadata: Some(metadata),
        corner_fail: Some("vertices"),
        ..Access::default()
    };
    assert!(matches!(
        executive.create_fillet_brep(&mut Kernel::default(), &mut access, None),
        Err(ref e) if e.failed_routine_status() == Some(1001)
    ));
    assert_eq!(executive.error.code, 3);
    assert!(executive.error.vertex.is_none());
}

#[test]
fn disabled_mapping_experiments_do_not_add_candidates_or_intersections() {
    for trimming in [false, true] {
        let mut executive = state();
        executive.options.trimming = trimming;
        executive.intersection_edges = vec![Some(Handle(12))];
        executive.intersection_vertices = vec![Some(Handle(6))];
        executive
            .intersector
            .seed_mates_for_test(&[(Handle(10), Handle(11))]);
        let mut access = Access {
            mapped_edges: true,
            ..Access::default()
        };
        executive
            .do_filleting(&mut Kernel::default(), &mut access, None)
            .unwrap();
        assert_eq!(access.created_attributes, 0);
        assert!(access.removed_edges.is_empty());
        assert!(executive.topo_edges.is_empty());
        assert_eq!(executive.intersection_edges, [Some(Handle(12))]);
        assert_eq!(executive.intersection_vertices, [Some(Handle(6))]);
    }
}

#[test]
fn corner_topology_updates_the_global_merge_branch_on_the_live_executive() {
    let mut executive = state();
    executive.corners.push(Handle(7));
    let mut access = Access {
        corner_enables_global: true,
        ..Access::default()
    };
    executive
        .do_filleting(&mut Kernel::default(), &mut access, None)
        .unwrap();
    assert!(executive.options.global_merge);
    assert!(access.events.contains(&"global"));
}

#[test]
fn degeneracy_removes_deleted_candidates_before_the_attribute_scan() {
    let mut executive = state();
    executive.corners.push(Handle(7));
    executive.topo_edges.push(Some(Handle(20)));
    executive.options.trimming = true;
    let mut access = Access {
        mapped_edges: true,
        degenerate_edge: Some(Handle(20)),
        ..Access::default()
    };
    executive
        .do_filleting(&mut Kernel::default(), &mut access, None)
        .unwrap();
    assert_eq!(access.created_attributes, 0);
    assert!(access.removed_edges.is_empty());
}

#[test]
fn insertion_hooks_share_mates_and_existing_ssi_with_subsequent_phases() {
    let mut executive = state();
    executive.solvers.push(Handle(3));
    executive.corners.push(Handle(7));
    executive.options.topology_insertion = true;
    executive.options.trimming = true;
    let mut access = Access {
        mapped_edges: true,
        insert_live_state: true,
        geoms: vec![Handle(8)],
        ..Access::default()
    };
    executive
        .do_filleting(&mut Kernel::default(), &mut access, None)
        .unwrap();
    assert_eq!(
        access.marked_pairs,
        [(Handle(10), Handle(11)), (Handle(12), Handle(13))]
    );
    assert!(executive.intersector.intersection_table().is_some());
    assert_eq!(executive.error.code, 6);
}

#[test]
fn temporary_attributes_survive_orientation_and_are_cleaned_on_errors_or_deferrals() {
    for error in [
        None,
        Some(failure()),
        Some(SolidError::unported(
            "SmBrep::OrientTrimmedSurfaces",
            "topology.brep_p3",
        )),
    ] {
        let mut executive = state();
        executive.options.trimming = true;
        executive.topo_edges.push(Some(Handle(20)));
        let mut access = Access {
            mapped_edges: true,
            orient_error: error.clone(),
            ..Access::default()
        };
        let result = executive.do_filleting(&mut Kernel::default(), &mut access, None);
        // every orientation failure is passed on (fork 873c7c9 `SER`, WR-661)
        assert_eq!(result.is_err(), error.is_some());
        assert_eq!(access.created_attributes, 1);
        assert!(access.attributes.is_empty());
        assert_eq!(access.events.last(), Some(&"delete_attr"));
        assert!(
            access.events.iter().position(|s| *s == "orient").unwrap()
                < access
                    .events
                    .iter()
                    .position(|s| *s == "delete_attr")
                    .unwrap()
        );
    }
}

#[test]
fn face_creation_reads_the_live_geometry_array_without_repeated_snapshots() {
    let mut executive = state();
    executive.solvers.push(Handle(3));
    let mut access = Access {
        geoms: vec![Handle(8)],
        append_geom: true,
        ..Access::default()
    };
    executive
        .create_fillet_brep(&mut Kernel::default(), &mut access, None)
        .unwrap();
    assert_eq!(
        access.events.iter().filter(|s| **s == "make_face").count(),
        2
    );
    assert_eq!(access.geom_reads.get(), 3);
    assert_eq!(access.geom_snapshots.get(), 0);
}

#[test]
fn temporary_attributes_are_also_cleaned_when_merge_fails_or_defers_before_orientation() {
    for error in [
        failure(),
        SolidError::unported("SmBrep::MergeBrep", "topology.brep_p3"),
    ] {
        let mut executive = state();
        executive.options.trimming = true;
        executive.topo_edges.push(Some(Handle(20)));
        let mut access = Access {
            mapped_edges: true,
            merge_error: Some(error.clone()),
            ..Access::default()
        };
        assert_eq!(
            executive.do_filleting(&mut Kernel::default(), &mut access, None),
            Err(error)
        );
        assert_eq!(access.created_attributes, 1);
        assert!(access.attributes.is_empty());
        assert!(!access.events.contains(&"orient"));
        assert_eq!(access.events.last(), Some(&"delete_attr"));
        assert!(!access.editing);
    }
}

#[test]
fn solver_tolerance_sentinel_is_converted_at_set_tolerance_call() {
    let mut executive = state();
    executive.solvers.push(Handle(3));
    let mut access = Access {
        solver_tolerance: Some(crate::geom::tol::USE_DEFAULT),
        ..Access::default()
    };
    let mut k = Kernel::default();
    executive
        .create_fillet_brep(&mut k, &mut access, None)
        .unwrap();
    assert_eq!(
        access.brep_tolerances,
        [0.125, zone_tol3d(&k, crate::geom::tol::USE_DEFAULT)]
    );
}

#[test]
fn tag_merge_component_retains_allocated_markers_when_merge_fails() {
    let mut executive = state();
    executive.topo_edges.push(Some(Handle(20)));
    let mut access = Access {
        mapped_edges: true,
        merge_error: Some(failure()),
        ..Access::default()
    };
    let mut attributes = Vec::new();
    let result = executive.tag_and_merge_fillet_faces(
        &mut Kernel::default(),
        &mut access,
        Handle(0),
        Handle(1),
        &mut attributes,
    );
    assert!(result.is_err());
    assert_eq!(attributes, [Handle(30)]);
    assert_eq!(access.attributes.get(&Handle(30)), Some(&vec![Handle(11)]));
    assert_eq!(access.events, ["new_attr", "merge"]);
    assert_eq!(executive.topo_edges, [Some(Handle(20))]);
}

// ---- a failed stage keeps its error as the cause of the executive's failure (#13790) ----

/// The executive's run of `create_fillet_brep` (one corner, one solver with one geometry)
/// whose stage `stage` fails with `error`.
fn failing_at(stage: &'static str, error: SolidError) -> (FilletExecutive<Handle>, Result<()>) {
    let mut executive = state();
    executive.corners.push(Handle(7));
    executive.solvers.push(Handle(3));
    let mut access = Access {
        corner_metadata: Some(corner()),
        geoms: vec![Handle(8)],
        stage_errors: BTreeMap::from([(stage, error)]),
        ..Access::default()
    };
    let result = executive.create_fillet_brep(&mut Kernel::default(), &mut access, None);
    (executive, result)
}

/// A stage error with no class (a bare status), a stage failure (a defect), and failures
/// whose own causes are a refusal, a cancellation and an unported call: none of them is
/// passed on by the executive, whatever their causes hold (decisions 2 and 3 of #19801).
fn stage_errors() -> [SolidError; 5] {
    use crate::error::blend::BlendFailure;
    let over = |cause: SolidError| {
        Failure::new("test::stage", BlendFailure::ExecutiveStepFailed)
            .caused_by(cause)
            .into_error()
    };
    [
        SolidError::sm_err("test::stage"),
        SolidError::blend("test::stage", BlendFailure::Defect { why: "x" }),
        over(SolidError::blend(
            "test::corner",
            BlendFailure::NoRoom(NoRoomReason::BallDoesNotFit),
        )),
        over(SolidError::Cancelled {
            operation: "test::corner",
        }),
        over(SolidError::unported("SmTest::Call", "blend.test")),
    ]
}

/// `e` is the executive's failure as it was before its cause was kept (the same operation,
/// reason, status and kind), now keeping `cause`, whose class it takes.
fn assert_failure_keeps(e: &SolidError, cause: &SolidError) {
    let bare = failure();
    assert_eq!(e.operation(), OP, "{e:?}");
    assert_eq!(e.blend_failure(), bare.blend_failure(), "{e:?}");
    assert_eq!(e.sm_status(), Some(SM_ERR), "{e:?}");
    assert_eq!(
        e.failed_routine_status(),
        bare.failed_routine_status(),
        "{e:?}"
    );
    assert_eq!(e.kind(), bare.kind(), "{e:?}");
    assert_eq!(e.entity(), None, "{e:?}");
    assert_eq!(e.cause(), Some(cause), "{e:?}");
    let source = std::error::Error::source(e.failure().unwrap()).map(ToString::to_string);
    assert_eq!(source, Some(cause.to_string()));
    assert_eq!(e.to_string(), format!("{bare}; caused by {cause}"));
    // the kernel's chain-following properties now follow the kept cause
    assert_eq!(e.is_defect(), cause.is_defect(), "{e:?}");
    assert_eq!(e.is_refusal(), cause.is_refusal(), "{e:?}");
    assert_eq!(e.sized(), cause.sized(), "{e:?}");
    assert_eq!(e.is_port_failure(), cause.is_port_failure(), "{e:?}");
    // a status answer (the failure has a status) unless its kept chain holds a port failure
    // or a cancellation
    let stopped = std::iter::successors(Some(cause), |c| c.cause()).any(SolidError::is_cancelled);
    assert_eq!(
        e.is_status_answer(),
        !cause.is_port_failure() && !stopped,
        "{e:?}"
    );
}

/// Each stage whose failure the executive reports with its own failure (a corner's topology,
/// vertex geometry, topology adjustment, tangent adjustment, edges and face geometry; a
/// solver's geometry, rail trim and face) notes the same error info as before and returns
/// the same failure, now keeping the stage's error as its cause.
#[test]
fn a_failed_stage_keeps_its_error_as_the_executives_cause() {
    // (stage, (error code, error status, comment))
    let corner = |comment| (3, 2, comment);
    let solver = |status, comment| (4, status, comment);
    let stages = [
        (
            "topology",
            corner("Fillet Corner Error: Unable to Make Topology"),
        ),
        (
            "vertices",
            corner("Fillet Corner Error: Unable to Compute Geometry"),
        ),
        (
            "adjust",
            corner("Fillet Corner Error: Topology Adjustment Failure"),
        ),
        ("solver", solver(10, "Fillet Surface Computation Failure")),
        (
            "adjust_tangent",
            corner("Fillet Corner Error: Topology Adjustment Failure"),
        ),
        (
            "edges",
            corner("Fillet Corner Error: Corner Edge Computation Failure"),
        ),
        (
            "trim",
            solver(
                7,
                "Fillet Surface Computation Failure: Unable to Trim Rail Curve",
            ),
        ),
        (
            "corner_geom",
            corner("Fillet Corner Error: Corner Geometry Computation Failure"),
        ),
        (
            "make_face",
            solver(
                2,
                "Fillet Surface Computation Failure: Fillet Face Creation Error",
            ),
        ),
    ];
    for (stage, (code, status, comment)) in stages {
        for cause in stage_errors() {
            let (executive, result) = failing_at(stage, cause.clone());
            let e = result.unwrap_err();
            assert_failure_keeps(&e, &cause);
            assert_eq!(executive.error.code, code, "{stage}");
            assert_eq!(executive.error.status, status, "{stage}");
            assert_eq!(executive.error.comment, comment, "{stage}");
        }
    }
}

/// A corner that noted its refusal at its topology (`SM_FILERR_NO_ROOM`) keeps that note,
/// and the failure keeps the topology's error as its cause.
#[test]
fn a_corner_refused_at_its_topology_keeps_the_topologys_error() {
    for cause in stage_errors() {
        let (executive, result) = failing_at("topology_noted", cause.clone());
        assert_failure_keeps(&result.unwrap_err(), &cause);
        assert_eq!(executive.error.code, FilletErrorType::NoRoom as i32);
        assert_eq!(
            executive.error.comment,
            ErrorInfo::<Handle>::default().comment
        );
    }
}

/// An unported call, a cancellation or unavailable face trims at a stage is passed on as it
/// is, noting nothing (the stage's error itself decides, as before); a refusal of a corner's
/// vertex geometry is passed on as it is, while a refusal at a stage that does not pass
/// refusals on (the corner's edges) is the executive's failure, keeping it.
#[test]
fn a_stage_passes_on_only_what_it_passed_on_before() {
    use crate::error::blend::BlendFailure;
    let passed = [
        SolidError::unported("SmTest::Call", "blend.test"),
        SolidError::Cancelled {
            operation: "test::stage",
        },
        SolidError::FaceTrimsUnavailable {
            operation: "test::stage",
            entity: crate::error::EntityRef::Face(crate::topology::FaceId::from_index(2)),
        },
    ];
    for stage in ["topology", "vertices", "solver", "edges", "make_face"] {
        for error in passed.clone() {
            let (executive, result) = failing_at(stage, error.clone());
            assert_eq!(result, Err(error), "{stage}");
            // no corner or solver failure noted (the scripted solver geometry notes 6)
            let noted_by_stages = if ["edges", "make_face"].contains(&stage) {
                6
            } else {
                0
            };
            assert_eq!(executive.error.code, noted_by_stages, "{stage}");
        }
    }
    let refusal = SolidError::blend(
        "test::corner",
        BlendFailure::NoRoom(NoRoomReason::BallDoesNotFit),
    );
    let (executive, result) = failing_at("vertices", refusal.clone());
    assert_eq!(result, Err(refusal.clone()));
    assert_eq!(executive.error.code, 0);
    let (executive, result) = failing_at("edges", refusal.clone());
    assert_failure_keeps(&result.unwrap_err(), &refusal);
    assert_eq!(executive.error.code, 3);
}

/// A refusal or a cancellation among a stage error's causes changes nothing about what the
/// executive passes on (decisions 2 and 3 of #19801): the corner's vertex geometry, which
/// passes on a refusal it is given, reports the executive's failure for a failure over a
/// refusal, and for a failure over a cancellation. Only the chain-following properties of the
/// returned failure see them.
#[test]
fn a_refusal_or_a_cancellation_under_a_stage_error_is_not_passed_on() {
    let [_, _, over_refusal, over_cancellation, _] = stage_errors();
    for cause in [&over_refusal, &over_cancellation] {
        assert!(!cause.is_unported_or_cancelled());
        let (executive, result) = failing_at("vertices", cause.clone());
        assert_failure_keeps(&result.unwrap_err(), cause);
        assert_eq!(executive.error.code, 3);
        assert_eq!(
            executive.error.comment,
            "Fillet Corner Error: Unable to Compute Geometry"
        );
    }
    let refused = failing_at("vertices", over_refusal).1.unwrap_err();
    assert_eq!(refused.blend_failure(), failure().blend_failure());
    assert!(refused.is_refusal() && refused.sized() && refused.is_status_answer());
    let stopped = failing_at("vertices", over_cancellation).1.unwrap_err();
    assert!(!stopped.is_cancelled() && !stopped.is_status_answer());
}

/// `DoFilleting`'s failure for a `CreateFilletBrep` that failed keeps that failure, which
/// keeps its stage's error; a cancellation at a stage is passed on through both.
#[test]
fn do_filleting_keeps_the_failure_of_the_fillet_body() {
    let run = |error: SolidError| {
        let mut executive = state();
        executive.corners.push(Handle(7));
        let mut access = Access {
            corner_metadata: Some(corner()),
            stage_errors: BTreeMap::from([("vertices", error)]),
            ..Access::default()
        };
        executive.do_filleting(&mut Kernel::default(), &mut access, None)
    };
    let stage = SolidError::sm_err("test::stage");
    let e = run(stage.clone()).unwrap_err();
    let created = e.cause().unwrap().clone();
    assert_failure_keeps(&created, &stage);
    assert_failure_keeps(&e, &created);
    assert_eq!(e.root_cause(), &stage);
    let cancelled = SolidError::Cancelled {
        operation: "test::stage",
    };
    assert_eq!(run(cancelled.clone()), Err(cancelled));
}

/// The preview's corner stages and the rail classification's rollover keep their errors as
/// the failure's cause too, noting the same error info.
#[test]
fn the_preview_and_the_rollover_keep_the_stages_error() {
    for (stage, comment) in [
        ("topology", "Fillet Corner Error: Unable to Make Topology"),
        (
            "vertices",
            "Fillet Corner Error: Unable to Compute Geometry",
        ),
    ] {
        for cause in stage_errors() {
            let mut executive = state();
            executive.corners.push(Handle(7));
            let mut access = Access {
                corner_metadata: Some(corner()),
                stage_errors: BTreeMap::from([(stage, cause.clone())]),
                ..Access::default()
            };
            let e = executive
                .preview_fillet_geometry(&mut Kernel::default(), &mut access)
                .unwrap_err();
            assert_failure_keeps(&e, &cause);
            assert_eq!(executive.error.code, 3, "{stage}");
            assert_eq!(executive.error.comment, comment, "{stage}");
        }
    }
    // the scripted rollover test fails with the access's own error
    let mut executive = state();
    executive.solvers.push(Handle(3));
    executive.options.classification = true;
    let mut access = Access::default();
    let e = executive
        .classify_solver_rails(&mut Kernel::default(), &mut access)
        .unwrap_err();
    assert_failure_keeps(&e, &invalid("test handle has no geometry"));
    assert_eq!(executive.error.code, 4);
    assert_eq!(executive.error.status, 7);
    assert_eq!(executive.error.edge, Some(Handle(5)));
    assert_eq!(
        executive.error.comment,
        "Fillet Solver Error: Unable to Process Rollover"
    );
}

/// The corner-edge loop's repeated vertex geometry, a corner its topology noted as not built
/// (`CornerUnsupported`) and the fillet body's own stitch keep the stage's error, noting the
/// same error info as before.
#[test]
fn the_repeated_vertex_geometry_the_unsupported_corner_and_the_stitch_keep_the_error() {
    for cause in stage_errors() {
        let (executive, result) = failing_at("vertices_again", cause.clone());
        assert_failure_keeps(&result.unwrap_err(), &cause);
        assert_eq!(executive.error.code, 3);
        assert_eq!(
            executive.error.comment,
            "Fillet Corner Error: Vertex Computation Failure"
        );

        let (executive, result) = failing_at("topology_unsupported", cause.clone());
        assert_failure_keeps(&result.unwrap_err(), &cause);
        assert_eq!(
            executive.error.code,
            FilletErrorType::CornerUnsupported as i32
        );

        let (executive, result) = failing_at("stitch_faces", cause.clone());
        assert_failure_keeps(&result.unwrap_err(), &cause);
        assert_eq!(executive.error.code, 7);
        assert_eq!(
            executive.error.comment,
            "Fillet Stitching Failure: Error Stitching Fillets Together"
        );
    }
}

/// A refusal of the vertex geometry is passed on as it is at the corner-edge loop's
/// repeated call and at the preview, as at the first call.
#[test]
fn a_refused_vertex_geometry_is_passed_on_at_every_call() {
    use crate::error::blend::BlendFailure;
    let refusal = SolidError::blend(
        "test::corner",
        BlendFailure::NoRoom(NoRoomReason::BallDoesNotFit),
    );
    let (executive, result) = failing_at("vertices_again", refusal.clone());
    assert_eq!(result, Err(refusal.clone()));
    assert_ne!(executive.error.code, 3);
    let mut executive = state();
    executive.corners.push(Handle(7));
    let mut access = Access {
        corner_metadata: Some(corner()),
        stage_errors: BTreeMap::from([("vertices", refusal.clone())]),
        ..Access::default()
    };
    assert_eq!(
        executive.preview_fillet_geometry(&mut Kernel::default(), &mut access),
        Err(refusal)
    );
    assert_eq!(executive.error.code, 0);
}

/// The merge's stitch into the target, failing on its fifth try, keeps the stitch's error.
#[test]
fn the_stitch_into_the_target_keeps_its_error() {
    for cause in stage_errors() {
        let mut executive = state();
        executive.options.trimming = true;
        let mut access = Access {
            stage_errors: BTreeMap::from([("do_stitching", cause.clone())]),
            ..Access::default()
        };
        let e = executive
            .do_filleting(&mut Kernel::default(), &mut access, None)
            .unwrap_err();
        assert_failure_keeps(&e, &cause);
        assert_eq!(access.stitch_tolerances.len(), 5);
        assert_eq!(executive.error.code, 7);
        assert_eq!(
            executive.error.comment,
            "Fillet Stitching Failure: Error Stitching Fillets To Original Brep"
        );
    }
}

/// The edges of a corner the rollover adjusted keep their error: as the corner's failure,
/// or, for an End refused as passing over its end face, as the failure that keeps that
/// refusal's note.
#[test]
fn the_edges_after_a_rollover_keep_their_error() {
    for passes_over_end in [false, true] {
        for cause in stage_errors() {
            let mut executive = state();
            executive.solvers.push(Handle(3));
            executive.options.classification = true;
            if passes_over_end {
                executive.error.code = FilletErrorType::NoRoom as i32;
                executive.error.no_room = Some(NoRoomReason::EndPassesOverEndFace);
            }
            let mut access = Access {
                corner_metadata: Some(corner()),
                rollover_adjusts: Some(Handle(7)),
                stage_errors: BTreeMap::from([("edges", cause.clone())]),
                ..Access::default()
            };
            let e = executive
                .classify_solver_rails(&mut Kernel::default(), &mut access)
                .unwrap_err();
            assert_failure_keeps(&e, &cause);
            if passes_over_end {
                assert_eq!(executive.error.code, FilletErrorType::NoRoom as i32);
            } else {
                assert_eq!(executive.error.code, 3);
                assert_eq!(
                    executive.error.comment,
                    "Fillet Corner Error: Edge Computation Failure After Rollover Adjustment"
                );
            }
        }
    }
}

/// A face of an End refused as passing over its end face keeps the face's error, keeping
/// that refusal's note.
#[test]
fn a_face_past_its_end_face_keeps_the_faces_error() {
    for cause in stage_errors() {
        let mut executive = state();
        executive.solvers.push(Handle(3));
        executive.error.code = FilletErrorType::NoRoom as i32;
        executive.error.no_room = Some(NoRoomReason::EndPassesOverEndFace);
        let mut access = Access {
            geoms: vec![Handle(8)],
            stage_errors: BTreeMap::from([("make_face", cause.clone())]),
            ..Access::default()
        };
        let e = executive
            .build_fillet_faces(&mut Kernel::default(), &mut access)
            .unwrap_err();
        assert_failure_keeps(&e, &cause);
        assert_eq!(
            executive.error.no_room_reason(),
            Some(NoRoomReason::EndPassesOverEndFace)
        );
    }
}

/// A solver-geometry access whose ease before the setbacks fails with its error.
struct Easing(SolidError);
impl SolverGeometryAccess<Handle> for Easing {
    fn geometry_calculate(
        &mut self,
        _: &mut Kernel,
        _: Handle,
        _: &mut FilletExecutive<Handle>,
    ) -> Result<()> {
        Ok(())
    }
    fn geometry_edge(&self, solver: Handle) -> Result<Handle> {
        Ok(solver)
    }
    fn geometry_ease_set_back_sections(&mut self, _: &mut Kernel, _: &[Handle]) -> Result<()> {
        Err(self.0.clone())
    }
}

/// The ease before the setbacks keeps its error, noting the same error info as before; an
/// unported easing or a cancellation is passed on as it is.
#[test]
fn the_ease_before_the_setbacks_keeps_its_error() {
    for cause in stage_errors() {
        let mut executive = state();
        let e = executive
            .ease_set_back_sections(&mut Kernel::default(), &mut Easing(cause.clone()))
            .unwrap_err();
        assert_failure_keeps(&e, &cause);
        assert_eq!(executive.error.code, FilletErrorType::EdgeProblem as i32);
    }
    let cancelled = SolidError::Cancelled {
        operation: "test::stage",
    };
    let mut executive = state();
    assert_eq!(
        executive.ease_set_back_sections(&mut Kernel::default(), &mut Easing(cancelled.clone())),
        Err(cancelled)
    );
    assert_eq!(executive.error.code, 0);
}
