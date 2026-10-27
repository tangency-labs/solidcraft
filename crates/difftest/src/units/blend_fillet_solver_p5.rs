//! Differential checks for the accessible rollover branches. Single-interval
//! classifications are supplied directly; these checks do not claim to exercise
//! surface repair. `TestRollOver` with two real rails runs the real `reset`; its
//! remaining dependencies are reported by the differential run. The
//! multi-interval process path evaluates binormals on a wire, then compares the
//! null-surface return. Post-call state,
//! including caller-seeded duplicate adjusted corners, is compared on every pass.
use super::topology_edgeuse_p1::import;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::blend::fillet_geom::p1::{
    FilletBrep, FilletCornerId, FilletCornerType, FilletEdge, FilletGeom, FilletGeomType,
    FilletVertex,
};
use solidcore::blend::fillet_solver::p1::FilletSolver;
use solidcore::blend::fillet_solver::p5::{RolloverObjects, RolloverOwnership};
use solidcore::curve::{curve::Curve, line::Line};
use solidcore::error::{EntityRef, Result as SolidResult, SolidError};
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{ClassGeometry, PointClassType};
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p2::CurveOnClassifyObjects;
use solidcore::surface::{surface::Surface, volume::Volume};
use solidcore::topology::{
    Brep, EdgeId, Edgeuse, EdgeuseId, FaceId, LoopId, VertexId, VertexuseId,
};
use solidcore_ffi::units::blend_fillet_solver_p5 as ffi;

/// Accessible rollover branches and their post-call classification state.
pub struct Runner;

#[derive(Clone, Serialize, Deserialize)]
struct Interval {
    min: f64,
    max: f64,
    class: i32,
}
#[derive(Clone, Serialize, Deserialize)]
struct Inputs {
    op: i32,
    kind: i32,
    corner: i32,
    adjusted: Vec<i32>,
    rails: [Option<Vec<Interval>>; 2],
}
impl Inputs {
    fn flat(&self) -> Vec<f64> {
        let mut result = vec![
            f64::from(self.op),
            f64::from(self.kind),
            f64::from(self.corner),
            self.adjusted.len() as f64,
        ];
        result.extend(self.adjusted.iter().copied().map(f64::from));
        for rail in &self.rails {
            result.push(rail.as_ref().map_or(-1., |r| r.len() as f64));
            if let Some(r) = rail {
                for ivl in r {
                    result.extend([ivl.min, ivl.max, f64::from(ivl.class)]);
                }
            }
        }
        result
    }
}
fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

// Keep status, enum values, counts and ordered corner identities in exact integer
// comparisons. Only parameter endpoints use the numerical relative-error gate.
fn output(state: &[f64]) -> Result<Output, HarnessError> {
    let mut values = state.iter().copied();
    let mut ints = Vec::new();
    let mut parameters = Vec::new();
    let next = |values: &mut std::iter::Copied<std::slice::Iter<'_, f64>>| {
        values.next().ok_or_else(|| harness("short native state"))
    };
    let integer = |x: f64| {
        if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e6 {
            Ok(x as i64)
        } else {
            Err(harness("invalid native integer"))
        }
    };
    ints.push(integer(next(&mut values)?)?);
    ints.push(integer(next(&mut values)?)?);
    let count = integer(next(&mut values)?)?;
    if !(0..=16).contains(&count) {
        return Err(harness("adjusted count"));
    }
    ints.push(count);
    for _ in 0..count {
        ints.push(integer(next(&mut values)?)?);
    }
    for _ in 0..2 {
        let count = integer(next(&mut values)?)?;
        if !(-1..=8).contains(&count) {
            return Err(harness("classification count"));
        }
        ints.push(count);
        for _ in 0..count {
            parameters.extend([next(&mut values)?, next(&mut values)?]);
            ints.push(integer(next(&mut values)?)?);
        }
    }
    if values.next().is_some() {
        return Err(harness("trailing native state"));
    }
    Ok(Output::new()
        .ints("state", ints)
        .values("parameters", parameters))
}

// Only used by the empty-list call or before the current ReSet deferral. Real
// target topology is imported from the exact same native rectangular fixture;
// rail curves live in this separate pseudo-B-rep. No default Kernel is hidden.
struct Objects<'a> {
    k: &'a Kernel,
    fillets: &'a FilletBrep,
    target: &'a Brep,
}
impl ClassGeometry<EntityRef> for Objects<'_> {
    /// `pVertex->GetVertexuseOfFace(pFace)->GetVertexFaceGap()`: its `GetLength()`,
    /// or `0.0` without a gap (as the `topology.face_p4` runner).
    fn vertex_face_gap(
        &self,
        vertex: EntityRef,
        surface: &Surface,
        face: Option<EntityRef>,
    ) -> Option<f64> {
        let EntityRef::Vertex(v) = vertex else {
            return None;
        };
        let f = match face {
            Some(EntityRef::Face(f)) => f,
            Some(_) => return None,
            None => self
                .target
                .faces()
                .iter()
                .find(|(_, r)| r.surface.as_ref().is_some_and(|s| std::ptr::eq(s, surface)))
                .map(|(f, _)| f)?,
        };
        let vu = solidcore::topology::vertex::vertexuse_of_face(self.target, v, Some(f)).ok()??;
        let mut b = self.target.clone();
        let gap =
            solidcore::topology::vertexuse::vertex_face_gap(self.k, &mut b, vu, false).ok()?;
        Some(gap.map_or(0.0, |g| g.gap_3d))
    }
    fn curve(&self, o: EntityRef) -> Option<&Curve> {
        match o {
            EntityRef::Edge(e) => {
                let offset = self.target.edges().len() as u32;
                if let Some(index) = e.index().checked_sub(offset) {
                    self.fillets
                        .brep()
                        .edge(EdgeId::from_index(index))
                        .ok()?
                        .curve
                        .as_ref()
                } else {
                    self.target.edge(e).ok()?.curve.as_ref()
                }
            }
            EntityRef::Edgeuse(e) => self.target.edgeuse(e).ok()?.uv_trim_curve.as_ref(),
            _ => None,
        }
    }
    fn surface(&self, o: EntityRef) -> Option<&Surface> {
        match o {
            EntityRef::Face(f) => self.target.face(f).ok()?.surface.as_ref(),
            _ => None,
        }
    }
    fn volume(&self, _: EntityRef) -> Option<&Volume> {
        None
    }
    fn point(&self, o: EntityRef) -> Option<Point3> {
        if let EntityRef::Vertex(v) = o {
            Some(self.target.vertex(v).ok()?.point)
        } else {
            None
        }
    }
    fn classification_vertex_point(&self, o: EntityRef) -> Option<Point3> {
        self.point(o)
    }
    fn topology_zone_tol_3d(&self, o: EntityRef) -> Option<f64> {
        match o {
            EntityRef::Vertex(v) => Some(self.target.vertex(v).ok()?.tolerance),
            EntityRef::Edge(e) => Some(self.target.edge(e).ok()?.tolerance),
            EntityRef::Face(f) => Some(self.target.face(f).ok()?.tolerance),
            _ => None,
        }
    }
    fn topology_entity(&self, o: EntityRef) -> Option<(&Brep, EntityRef)> {
        match o {
            EntityRef::Face(_)
            | EntityRef::Vertex(_)
            | EntityRef::Edge(_)
            | EntityRef::Vertexuse(_)
            | EntityRef::Edgeuse(_)
            | EntityRef::Loop(_)
            | EntityRef::Faceuse(_)
            | EntityRef::Loopuse(_)
            | EntityRef::Shell(_)
            | EntityRef::Region(_)
            | EntityRef::Brep(_) => Some((self.target, o)),
        }
    }
    fn topology_handle(&self, entity: EntityRef) -> Option<EntityRef> {
        Some(entity)
    }
}
impl CurveOnClassifyObjects<EntityRef> for Objects<'_> {
    fn face(&self, f: FaceId) -> EntityRef {
        EntityRef::Face(f)
    }
    fn vertex(&self, v: VertexId) -> EntityRef {
        EntityRef::Vertex(v)
    }
    fn edge(&self, e: EdgeId) -> EntityRef {
        EntityRef::Edge(e)
    }
    fn vertexuse(&self, v: VertexuseId) -> EntityRef {
        EntityRef::Vertexuse(v)
    }
    fn edgeuse(&self, e: EdgeuseId) -> EntityRef {
        EntityRef::Edgeuse(e)
    }
    fn edge_curve(&self, e: EdgeId) -> EntityRef {
        EntityRef::Edge(e)
    }
    fn trim_curve(&self, e: EdgeuseId) -> EntityRef {
        EntityRef::Edgeuse(e)
    }
    fn face_surface(&self, f: FaceId) -> Option<EntityRef> {
        Some(EntityRef::Face(f))
    }
    fn loop_id(&self, o: EntityRef) -> Option<LoopId> {
        if let EntityRef::Loop(l) = o {
            Some(l)
        } else {
            None
        }
    }
}
impl RolloverObjects for Objects<'_> {
    fn rail_curve(&self, e: EdgeId) -> EntityRef {
        EntityRef::Edge(EdgeId::from_index(
            self.target.edges().len() as u32 + e.index(),
        ))
    }
    fn rail_mate_uv_curve(&self, _: EdgeuseId) -> Option<EntityRef> {
        None
    }
    fn corner_type(&self, _: FilletCornerId) -> SolidResult<FilletCornerType> {
        Err(SolidError::invalid_input(
            "fixture",
            "unexpected corner lookup",
        ))
    }
}
fn corner_type(code: i32) -> FilletCornerType {
    match code {
        1 => FilletCornerType::Open,
        2 => FilletCornerType::Degenerate,
        3 => FilletCornerType::OneByOne,
        4 => FilletCornerType::TwoByTwo,
        5 => FilletCornerType::ThreeByTwoMixed,
        6 => FilletCornerType::FourByThree,
        7 => FilletCornerType::NByOneClosed,
        8 => FilletCornerType::NByOne,
        9 => FilletCornerType::NByTwo,
        10 => FilletCornerType::NByN,
        11 => FilletCornerType::NByNConvex,
        12 => FilletCornerType::NByNConcave,
        _ => FilletCornerType::Unknown,
    }
}
struct Ownership;
impl RolloverOwnership for Ownership {
    fn edge_owner(
        &self,
        _: EdgeId,
    ) -> SolidResult<(
        solidcore::geom::object::ObjectRef,
        Option<solidcore::geom::object::ObjectRef>,
    )> {
        Err(SolidError::invalid_input(
            "fixture",
            "unexpected new curve owner",
        ))
    }
    fn owner_changed(
        &mut self,
        _: &mut Curve,
        _: solidcore::curve::curve::p2::OwnerChange,
    ) -> SolidResult<()> {
        Err(SolidError::invalid_input(
            "fixture",
            "unexpected owner notification",
        ))
    }
}

fn run(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut solver = FilletSolver::new(1e-5, 1e-3, 1e-3);
    let mut fb = FilletBrep::new();
    let mut geom = FilletGeom::new(
        &k,
        None,
        FilletGeomType::from_code(i64::from(i.kind)).ok_or_else(|| harness("kind"))?,
    )
    .map_err(harness)?;
    let mut target = import(&ffi::target().map_err(harness)?)?;
    let face = target.faces().ids().next();
    let region = fb
        .brep_mut()
        .add_region_record(solidcore::topology::Region {
            in_brep: true,
            ..solidcore::topology::Region::default()
        })
        .map_err(harness)?;
    let mut rails = Vec::new();
    for (index, cls) in i.rails.iter().enumerate() {
        let a = fb
            .brep_mut()
            .add_vertex(Vec3::new(0.25, 0.25 + 0.25 * index as f64, 0.))
            .map_err(harness)?;
        let b = fb
            .brep_mut()
            .add_vertex(Vec3::new(0.75, 0.25 + 0.25 * index as f64, 0.))
            .map_err(harness)?;
        let edge = fb.brep_mut().add_edge(a, b).map_err(harness)?;
        fb.insert_fillet_vertex(
            a,
            FilletVertex::new(
                &k,
                if i.corner < 0 {
                    None
                } else {
                    Some(FilletCornerId(0))
                },
            ),
        );
        fb.insert_fillet_vertex(b, FilletVertex::new(&k, None));
        let eu0 = fb
            .brep_mut()
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                orientation: OrientType::Same,
                ..Edgeuse::default()
            })
            .map_err(harness)?;
        let eu1 = fb
            .brep_mut()
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                orientation: OrientType::Opposite,
                ..Edgeuse::default()
            })
            .map_err(harness)?;
        fb.brep_mut().edge_mut(edge).map_err(harness)?.edgeuses = vec![eu0, eu1];
        for (vertex, edgeuse) in [(a, eu0), (b, eu1)] {
            let vu = fb
                .brep_mut()
                .add_vertexuse_record(solidcore::topology::Vertexuse {
                    vertex: Some(vertex),
                    owner: solidcore::topology::VertexuseOwner::Edgeuse(Some(edgeuse)),
                    ..solidcore::topology::Vertexuse::default()
                })
                .map_err(harness)?;
            fb.brep_mut()
                .edgeuse_mut(edgeuse)
                .map_err(harness)?
                .vertexuse = Some(vu);
            fb.brep_mut()
                .vertex_mut(vertex)
                .map_err(harness)?
                .vertexuses
                .push(vu);
        }

        if index == 0 {
            solver.edgeuses[0] = Some(eu0);
        }
        if i.op == 2 {
            fb.brep_mut().edge_mut(edge).map_err(harness)?.curve =
                Some(Curve::from(Line::from_points(
                    Vec3::new(0.25, 0.25 + 0.25 * index as f64, 0.),
                    Vec3::new(0.75, 0.25 + 0.25 * index as f64, 0.),
                    3,
                )));
        }
        let mut record = FilletEdge::with_corner(None);
        record.set_original_face(face);
        if let Some(ivls) = cls {
            let mut cc = CurveClassification::new(&k);
            cc.set_size(&k, ivls.len()).map_err(harness)?;
            for (j, ivl) in ivls.iter().enumerate() {
                let r = cc.at_mut(j).map_err(harness)?;
                // The native fixture ignores SetMinMax status; a negative interval
                // leaves the default inside-out extent unchanged.
                let _ = r.interval.set_min_max(ivl.min, ivl.max);
                let object = match ivl.class {
                    101 => Some(EntityRef::Region(region)),
                    102 => face.map(EntityRef::Face),
                    103 => Some(EntityRef::Edge(edge)),
                    104 => Some(EntityRef::Vertex(a)),
                    109 => Some(EntityRef::Edgeuse(eu0)),
                    _ => None,
                };
                r.mid.set_class_object(
                    PointClassType::from_code(ivl.class).ok_or_else(|| harness("class"))?,
                    object,
                );
            }
            record.set_indexed_curve_class(Some(&cc)).map_err(harness)?;
        }
        fb.insert_fillet_edge(edge, record);
        geom.set_rail(index, Some(edge));
        rails.push(edge);
    }
    let id = fb.add_geom(geom);
    solver.fillet_geoms = vec![id];
    let mut adjusted: Vec<_> = i
        .adjusted
        .iter()
        .map(|&c| FilletCornerId(c as u32))
        .collect();
    let outcome = if i.op == 0 {
        let mut snapshot = fb.brep().clone();
        solver.process_roll_over(
            &k,
            &mut snapshot,
            &mut fb,
            id,
            // the fixture names a corner type, not corner objects: an empty table
            &mut FixedCornerType(corner_type(i.corner)),
            &mut adjusted,
            &mut Ownership,
        )
    } else {
        if i.op == 1 {
            solver.fillet_geoms.clear();
        }
        let snapshot = fb.clone();
        let target_objects = target.clone();
        solver.test_roll_over(
            &k,
            &mut target,
            &mut fb,
            &Objects {
                k: &k,
                fillets: &snapshot,
                target: &target_objects,
            },
            &mut adjusted,
            &mut Ownership,
        )
    };
    if let Err(e) = &outcome {
        if let Some(output) = Output::unported(e) {
            return Ok(output);
        }
    }
    let status = match outcome {
        Ok(()) => 0,
        Err(SolidError::Upstream { code, .. }) => code,
        Err(e) => return Err(harness(e)),
    };
    let mut state = vec![
        f64::from(status),
        f64::from(fb.geom(id).map_err(harness)?.fillet_geom_type() as i32),
        adjusted.len() as f64,
    ];
    state.extend(adjusted.iter().map(|c| f64::from(c.0)));
    for rail in rails {
        let cc = fb.fillet_edge(rail).map_err(harness)?.curve_class();
        state.push(cc.map_or(-1., |cc| cc.size() as f64));
        if let Some(cc) = cc {
            for ivl in cc.intervals() {
                state.extend([
                    ivl.min(),
                    ivl.max(),
                    f64::from(ivl.mid.point_class().code()),
                ]);
            }
        }
    }
    output(&state)
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_solver_p5"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match ffi::eval(&i.flat()) {
            Ok(state) => output(&state),
            Err(solidcore_ffi::FfiError::InvalidArgument(_)) => {
                Ok(Output::rejected("incomplete recovery fixture"))
            }
            Err(e) => Err(harness(e)),
        }
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run(&case.parse()?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let kind = rng.below(7) as i32;
        let corner = rng.below(14) as i32 - 1;
        let op = i32::from(rng.below(8) == 0);
        let mut rails: [Option<Vec<Interval>>; 2] = [None, None];
        for rail in &mut rails {
            if rng.below(4) != 0 {
                let n = if rng.below(6) == 0 { 2 } else { 1 };
                *rail = Some(
                    (0..n)
                        .map(|_| {
                            let a = rng.uniform(-100., 100.);
                            Interval {
                                min: a,
                                max: a + rng.uniform(0.001, 20.),
                                class: rng
                                    .choose(&[100, 101, 102, 103, 104, 109])
                                    .copied()
                                    .unwrap_or(100),
                            }
                        })
                        .collect(),
                );
            }
        }
        serde_json::to_value(Inputs {
            op,
            kind,
            corner,
            adjusted: vec![1, 0, 1],
            rails,
        })
        .ok()
    }
}

/// The fixture's corners: every corner has the case's type, no vertex has a corner
/// and the corner table is empty.
struct FixedCornerType(FilletCornerType);

impl solidcore::blend::fillet_geom::p3::SplitCorners for FixedCornerType {
    fn corner_of_vertex(
        &self,
        _: VertexId,
    ) -> Option<FilletCornerId> {
        None
    }
    fn lookup(&self) -> solidcore::blend::fillet_geom::p1::CornerLookup<'_> {
        solidcore::blend::fillet_geom::p1::CornerLookup::default()
    }
    fn corner_mut(
        &mut self,
        _: FilletCornerId,
    ) -> solidcore::Result<&mut solidcore::blend::fillet_corner::p3::FilletCorner> {
        Err(SolidError::null_pointer("difftest::blend_fillet_solver_p5"))
    }
    fn corner_type(&self, _: FilletCornerId) -> solidcore::Result<FilletCornerType> {
        Ok(self.0)
    }
}
