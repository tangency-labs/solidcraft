//! Differential tests for merge orchestration. Leaf-only trees move stable
//! handles into an input pool imported before running the port. Results compare
//! identity/order/null slots, face/edge/vertex counts and ordered vertex positions
//! read from the actual Rust B-rep. Native result graphs are freshly imported and
//! projected to the same digest. Curve/surface values and adjacency are not compared.
//! Geometric merges remain deferred at the exact missing dependency call.
//! Family 6 runs `NonManifoldBoolean` through `DependencyMerge::with_engine` against a
//! context-only native `SmMerge` with the same explicit members and at most one
//! operand: it compares the NER status, the restored manifold flag, the lamina mode
//! the engine's intersector received and the null result (error-path coverage only).
use std::collections::BTreeSet;

use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use port::{BooleanDriver, PiecewiseAccess};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::boolean::merge::p1::{MergeState, VertexCopy};
use solidcore::boolean::merge::p2::{MergeOperand, TopologyCopy};
use solidcore::boolean::merge::p4::{KernelManifoldIntersector, ManifoldState};
use solidcore::boolean::merge::p5::process_wires_and_shell_vertices;
use solidcore::boolean::merge::{p1::MergeOptions, p3 as port};
use solidcore::boolean::topology_intersector::p1::{Relations, TopologyIntersector};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{ids, Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::context::MarkLock;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::topology::topology::Topology;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{brep::p1 as bp, Brep};
use solidcore::SolidError;
use solidcore_ffi::units::boolean_merge_p3 as ffi;

/// The five merge functions of `boolean.merge_p3`.
pub struct Runner;
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Operand {
    point: [f64; 3],
    #[serde(default)]
    size: Option<[f64; 3]>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    family: u8,
    operation: i32,
    #[serde(default)]
    options: Option<u8>,
    #[serde(default)]
    a: Vec<Operand>,
    #[serde(default)]
    b: Vec<Operand>,
    #[serde(default)]
    tree: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_refusal: Option<String>,
}
fn harness(error: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(error.to_string())
}
fn refusal(i: &Inputs, expected: &str) -> Result<Output, HarnessError> {
    match ffi::validate(&encode(i)) {
        Err(solidcore_ffi::FfiError::InvalidArgument(reason))
            if expected == "UB-190" && reason.starts_with("UB-190:") =>
        {
            // Guard-only check: no native operation or Rust kernel result is
            // compared. Counted as both-errored/no_values, never as a deferral.
            Ok(Output::err("ffi refusal: UB-190"))
        }
        _ => Err(harness("expected the specific UB-190 FFI refusal")),
    }
}
fn encode(i: &Inputs) -> Vec<f64> {
    let mut data = vec![
        f64::from(i.family),
        f64::from(i.operation),
        f64::from(i.options.unwrap_or(0)),
        f64::from(u8::from(i.options.is_some())),
        i.a.len() as f64,
        i.b.len() as f64,
    ];
    for operand in i.a.iter().chain(&i.b) {
        data.push(f64::from(u8::from(operand.size.is_some())));
        data.extend(operand.point);
        if let Some(size) = operand.size {
            data.extend(size);
        }
    }
    data.push(i.tree.len() as f64);
    data.extend(i.tree.iter().map(|&v| v as f64));
    data
}
/// The deferral tag of `ManifoldBoolean` calls with no Rust-side engine (family 4).
const HARNESS_ENGINE_STATE: &str = "harness:merge-engine-state";
/// Family 6: `NonManifoldBoolean` on an adopted engine (see the module docs).
const ENGINE_PROBE: u8 = 6;

fn boolean_operation(code: i32) -> Result<BooleanOperation, HarnessError> {
    Ok(match code {
        0 => BooleanOperation::Unknown,
        1 => BooleanOperation::Union,
        2 => BooleanOperation::Intersection,
        3 => BooleanOperation::Difference,
        4 => BooleanOperation::ExclusiveOr,
        5 => BooleanOperation::Merge,
        6 => BooleanOperation::PartialMerge,
        7 => BooleanOperation::Imprint,
        8 => BooleanOperation::ImprintClassify,
        9 => BooleanOperation::ExtractSeparate,
        10 => BooleanOperation::Slice,
        _ => return Err(harness("invalid operation")),
    })
}

fn engine_output(status: i64, manifold: bool, lamina: u64, result: bool) -> Output {
    Output::new()
        .int("status", status)
        .int("manifold", i64::from(manifold))
        .int("lamina", lamina as i64)
        .int("result", i64::from(result))
}

/// The engine mirrors the native `EngineAccess`: the context-only constructor's
/// members (`KernelManifoldIntersector::new`), the same explicit tolerances and
/// `SetMergeOptions`, manifold flag true and lamina mode 0 before the call.
fn engine_probe(i: &Inputs, pool: &mut Pool) -> Result<Output, HarnessError> {
    let mut breps = std::mem::take(&mut pool.breps).into_iter();
    let mut primary = if i.a.is_empty() { None } else { breps.next() };
    let mut other = if i.b.is_empty() { None } else { breps.next() };
    let mut intersector = KernelManifoldIntersector::new(0).map_err(|error| harness(&error))?;
    intersector.set_intersection_tolerances(1e-9, 0.3);
    intersector
        .merge
        .set_merge_options(i.options.map(options).as_ref());
    let mut driver = port::DependencyMerge::with_engine(port::MergeEngine {
        intersector,
        state: ManifoldState::default(),
        attributes: AttributeStore::default(),
    });
    let mut k = Kernel::default();
    let mut manifold = true;
    let mut lamina = 0;
    let result = port::non_manifold_boolean(
        &mut k,
        &mut driver,
        &mut primary,
        &mut other,
        &mut manifold,
        &mut lamina,
        boolean_operation(i.operation)?,
    );
    let engine = driver
        .into_engine()
        .ok_or_else(|| harness("adopted engine was not returned"))?;
    let code = match &result {
        Ok(_) => 0,
        Err(error) => match Output::unported(error) {
            Some(deferred) => return Ok(deferred),
            None => status(error),
        },
    };
    Ok(engine_output(
        code,
        manifold,
        engine.intersector.intersect_lamina_edges(),
        matches!(result, Ok(Some(_))),
    ))
}

fn native_engine_output(raw: &[f64]) -> Result<Output, HarnessError> {
    let [status, manifold, lamina, result] = raw else {
        return Err(harness("invalid engine probe output"));
    };
    let flag = |value: f64| {
        if value == 0.0 || value == 1.0 {
            Ok(value == 1.0)
        } else {
            Err(harness("invalid engine probe flag"))
        }
    };
    let mut input: &[f64] = std::slice::from_ref(lamina);
    let lamina = take_count(&mut input)? as u64;
    Ok(engine_output(
        *status as i64,
        flag(*manifold)?,
        lamina,
        flag(*result)?,
    ))
}

fn options(mask: u8) -> MergeOptions {
    MergeOptions::new(
        mask & 1 != 0,
        mask & 2 != 0,
        mask & 4 != 0,
        mask & 8 != 0,
        mask & 16 != 0,
    )
}
fn status(error: &SolidError) -> i64 {
    match error {
        SolidError::InvalidInput { .. } => 1007,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}
fn output(raw: &[f64], family: u8) -> Result<Output, HarnessError> {
    let (&status, rest) = raw.split_first().ok_or_else(|| harness("empty result"))?;
    if status != 0.0 {
        return Ok(Output::err(format!("status={status}")));
    }
    let mut input = rest;
    let mut controls = Vec::new();
    if family >= 3 {
        controls.push(take_count(&mut input)? as i64);
        controls.push(take_count(&mut input)? as i64);
    }
    let count = take_count(&mut input)?;
    let mut identities = Vec::new();
    let mut topology_counts = Vec::new();
    for _ in 0..count {
        identities.push(take_number(&mut input)? as i64);
        let length = take_count(&mut input)?;
        let record = input
            .get(..length)
            .ok_or_else(|| harness("truncated result digest"))?;
        input = input
            .get(length..)
            .ok_or_else(|| harness("truncated result digest"))?;
        if length == 0 {
            topology_counts.extend([-1, -1, -1]);
        } else {
            let mut counts = record;
            for _ in 0..3 {
                topology_counts.push(take_count(&mut counts)? as i64);
            }
            if record.len() != 3 + 3 * topology_counts.last().copied().unwrap_or(0) as usize {
                return Err(harness("invalid result digest shape"));
            }
        }
    }
    let surface_count = if family == 2 {
        take_count(&mut input)?
    } else {
        0
    };
    if !input.is_empty() {
        return Err(harness("trailing result output"));
    }
    Ok(Output::new()
        .int("status", status as i64)
        .ints("operand_identity", identities)
        .ints("topology_counts", topology_counts)
        .ints("controls", controls)
        .int("surface_count", surface_count as i64)
        .values("record", rest.to_vec()))
}

fn record_brep(brep: &Brep, identity: i64, values: &mut Vec<f64>) -> solidcore::Result<()> {
    let mut digest = Vec::new();
    super::heal_stitch_p1::digest(brep, &mut digest)?;
    values.extend([identity as f64, digest.len() as f64]);
    values.extend(digest);
    Ok(())
}

fn take_number(input: &mut &[f64]) -> Result<f64, HarnessError> {
    let (&number, rest) = input
        .split_first()
        .ok_or_else(|| harness("truncated record"))?;
    *input = rest;
    Ok(number)
}

fn take_count(input: &mut &[f64]) -> Result<usize, HarnessError> {
    let value = take_number(input)?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > 16384.0 {
        return Err(harness("invalid native record count"));
    }
    Ok(value as usize)
}

fn native_brep(input: &mut &[f64], values: &mut Vec<f64>) -> Result<(), HarnessError> {
    let identity = take_number(input)?;
    let count = take_count(input)?;
    let graph = input
        .get(..count)
        .ok_or_else(|| harness("truncated native graph"))?;
    *input = input
        .get(count..)
        .ok_or_else(|| harness("truncated native graph"))?;
    if count == 0 {
        if identity != -1.0 {
            return Err(harness("null native result has an identity"));
        }
        values.extend([-1.0, 0.0]);
        return Ok(());
    }
    let mut tagged = vec![0.0];
    tagged.extend(graph);
    let (brep, _) = super::topology_vertexuse::import(&tagged)?;
    record_brep(&brep, identity as i64, values).map_err(|error| harness(&error))
}

fn native_output(raw: &[f64], family: u8) -> Result<Output, HarnessError> {
    let mut input = raw;
    let status = take_number(&mut input)?;
    if status != 0.0 {
        return Ok(Output::err(format!("status={status}")));
    }
    let mut values = vec![status];
    if family == 3 || family == 4 {
        values.push(take_number(&mut input)?); // native manifold flag
        values.push(take_number(&mut input)?); // native lamina mode
    }
    let count = take_count(&mut input)?;
    if (family == 1 || family >= 3) && count > 1 {
        return Err(harness("invalid single-result flag"));
    }
    values.push(count as f64);
    for _ in 0..count {
        native_brep(&mut input, &mut values)?;
    }
    if family == 2 {
        values.push(take_count(&mut input)? as f64);
    }
    if !input.is_empty() {
        return Err(harness("trailing native output"));
    }
    output(&values, family)
}

fn single_result(
    pool: &Pool,
    result: Option<usize>,
    manifold: bool,
    lamina: u64,
    values: &mut Vec<f64>,
) -> solidcore::Result<()> {
    values.extend([f64::from(u8::from(manifold)), lamina as f64]);
    values.push(f64::from(u8::from(result.is_some())));
    if let Some(id) = result {
        pool.record(id, values)?;
    }
    Ok(())
}

fn list_result(result: &port::BooleanListsResult, values: &mut Vec<f64>) -> solidcore::Result<()> {
    values.push(result.breps.len() as f64);
    for brep in &result.breps {
        // BooleanLists returns independent copies, so no input pointer identity.
        record_brep(brep, -1, values)?;
    }
    values.push(result.surfaces.len() as f64);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PoolRef {
    Brep(usize),
    Surface(usize, usize),
}
impl ClassObject for PoolRef {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Surface(..) => ClassObjectKind::Surface,
            Self::Brep(..) => ClassObjectKind::Other,
        }
    }
}
impl SolutionObject for PoolRef {
    fn kind(&self) -> ObjectKind {
        match self {
            Self::Surface(..) => ObjectKind::Surface,
            Self::Brep(..) => ObjectKind::Other,
        }
    }
}

struct Pool {
    breps: Vec<Brep>,
    original_count: usize,
    retired: BTreeSet<usize>,
    intersector: TopologyIntersector<PoolRef>,
    attributes: AttributeStore,
    primary: Option<usize>,
    other: Option<usize>,
    manifold: bool,
    /// The engine the last `construct` built (`boolean.merge_p2`).
    engine: Option<port::MergeEngine>,
}
impl Pool {
    fn load(i: &Inputs) -> Result<Self, HarnessError> {
        let mut breps = Vec::new();

        for operand in i.a.iter().chain(&i.b) {
            let exported = ffi::eval(&encode(&Inputs {
                family: 5,
                operation: 0,
                options: None,
                a: vec![operand.clone()],
                b: Vec::new(),
                tree: Vec::new(),
                expected_refusal: None,
            }))
            .map_err(|error| harness(&error))?;
            // export_graph starts at validate_only; vertexuse import expects a
            // leading success tag. Geometry comes only from fixture construction.
            let mut tagged = vec![0.0];
            tagged.extend(&exported);
            let (mut brep, _) = super::topology_vertexuse::import(&tagged)?;
            // The vertexuse importer predates canonical arena owner keys. Preserve
            // its native owned/unowned flag, but translate existing owners so
            // canonical GetSurfaces sees the same standalone surfaces as C++.
            let faces: Vec<_> = brep.faces().ids().collect();
            for face in faces {
                if let Some(surface) = brep
                    .face_mut(face)
                    .map_err(|error| harness(&error))?
                    .surface
                    .as_mut()
                {
                    if surface.owner().is_some() {
                        surface.set_owner(Some(solidcore::topology::brep_cache::arena_identity(
                            solidcore::topology::brep_cache::CacheObject::Face(face),
                        )));
                    }
                }
                // a new owner key drops the cached face tolerance: record it again
                brep.sync_face_surface_tolerance(face)
                    .map_err(|error| harness(&error))?;
            }
            breps.push(brep);
        }
        Ok(Self {
            breps,
            original_count: i.a.len() + i.b.len(),
            retired: BTreeSet::new(),
            intersector: TopologyIntersector::new(
                Some(PoolRef::Brep(0)),
                Some(PoolRef::Brep(i.a.len())),
                0.0,
                0.0,
            ),
            attributes: AttributeStore::new(),
            primary: Some(0),
            other: Some(i.a.len()),
            manifold: true,
            engine: None,
        })
    }
    /// Move a live B-rep out of its slot (an empty one stays behind).
    fn take_brep(&mut self, id: usize) -> solidcore::Result<Brep> {
        self.brep(id)?;
        let slot = self
            .breps
            .get_mut(id)
            .ok_or_else(|| SolidError::null_pointer("harness::merge_p3"))?;
        Ok(std::mem::replace(slot, Brep::new()))
    }
    /// Put a B-rep back in its slot, or retire the slot when it was consumed.
    fn restore_brep(&mut self, id: Option<usize>, brep: Option<Brep>) {
        let Some(id) = id else { return };
        match (brep, self.breps.get_mut(id)) {
            (Some(brep), Some(slot)) => *slot = brep,
            _ => {
                self.retired.insert(id);
            }
        }
    }
    fn brep(&self, id: usize) -> solidcore::Result<&Brep> {
        if self.retired.contains(&id) {
            return Err(SolidError::invalid_input(
                "harness::merge_p3",
                "deleted B-rep handle",
            ));
        }
        self.breps
            .get(id)
            .ok_or_else(|| SolidError::null_pointer("harness::merge_p3"))
    }
    fn surface_key(&self, surface: (usize, usize)) -> solidcore::Result<ObjectKey> {
        let brep = self.brep(surface.0)?;
        if surface.1 >= bp::surfaces(brep, None)?.len() {
            return Err(SolidError::invalid_input(
                "harness::merge_p3",
                "bad surface handle",
            ));
        }
        let b = u32::try_from(surface.0).map_err(|_| {
            SolidError::invalid_input("harness::merge_p3", "surface owner overflow")
        })?;
        let s = u32::try_from(surface.1).map_err(|_| {
            SolidError::invalid_input("harness::merge_p3", "surface index overflow")
        })?;
        Ok(ObjectKey(
            ((u64::from(b) << 32) | u64::from(s))
                .checked_add(1)
                .ok_or_else(|| {
                    SolidError::invalid_input("harness::merge_p3", "surface key overflow")
                })?,
        ))
    }
    fn record(&self, id: usize, values: &mut Vec<f64>) -> solidcore::Result<()> {
        let brep = self.brep(id)?;
        let identity = if id < self.original_count {
            id as i64
        } else {
            -1
        };
        record_brep(brep, identity, values)
    }
}
impl BooleanDriver for Pool {
    type Operand = usize;
    fn same_operand(&self, left: &usize, right: &usize) -> bool {
        left == right
    }
    fn construct(
        &mut self,
        k: &mut Kernel,
        primary: &mut Option<usize>,
        other: &mut Option<usize>,
        approx_tol: f64,
        angle_tol: f64,
        options: Option<&MergeOptions>,
    ) -> solidcore::Result<()> {
        // SmMerge(context, primary, other, tol, angle, options) (boolean.merge_p2): the
        // pool's intersector takes the constructed tolerances, the manifold flag its
        // m_bManifoldBoolean.
        let primary = primary.ok_or(SolidError::invalid_input(
            "harness::merge_p3",
            "SmMerge without primary B-rep",
        ))?;
        let other = other.map(|o| self.brep(o)).transpose()?;
        let merge = MergeState::<TopologyIntersector<MergeOperand>>::with_breps(
            k,
            (self.brep(primary)?, MergeOperand::Primary),
            other.map(|o| (o, MergeOperand::Other)),
            approx_tol,
            angle_tol,
            options,
        )?;
        self.intersector
            .set_this_approx_tol_3d(merge.intersector.this_approx_tol_3d());
        self.intersector
            .set_this_ang_tol_rad(merge.intersector.this_ang_tol_rad());
        self.manifold = merge.manifold_boolean.unwrap_or(self.manifold);
        self.engine = Some(port::MergeEngine::from_merge(&merge)?);
        Ok(())
    }
    fn manifold_boolean(
        &mut self,
        k: &mut Kernel,
        primary: &mut Option<usize>,
        other: &mut Option<usize>,
        manifold: &mut bool,
        lamina: &mut u64,
        operation: BooleanOperation,
        lock: Option<&mut MarkLock>,
    ) -> solidcore::Result<Option<usize>> {
        // ManifoldBoolean (boolean.merge_p4) on the engine `construct` built, run by
        // port::DependencyMerge on the pool's B-reps, moved in and back.
        let Some(engine) = self.engine.take() else {
            // Family 4 runs PiecewiseMerge on the native SmMerge(c, a, b, ...) the shim
            // built, but no Rust engine holds the pool's piecewise intersector state.
            return Err(SolidError::unported(
                "SmMerge::ManifoldBoolean (pool engine state)",
                HARNESS_ENGINE_STATE,
            ));
        };
        if primary.is_some() && primary == other {
            self.engine = Some(engine);
            return Err(SolidError::invalid_input(
                "harness::merge_p3",
                "ManifoldBoolean of one B-rep with itself",
            ));
        }
        let ids = (*primary, *other);
        let mut a = ids.0.map(|id| self.take_brep(id)).transpose()?;
        let mut b = match ids.1.map(|id| self.take_brep(id)).transpose() {
            Ok(b) => b,
            Err(error) => {
                self.restore_brep(ids.0, a);
                return Err(error);
            }
        };
        let mut driver = port::DependencyMerge::with_engine(engine);
        let result = driver.manifold_boolean(k, &mut a, &mut b, manifold, lamina, operation, lock);
        self.engine = driver.into_engine();
        // An assigned rpResult is the primary B-rep, back in its own slot.
        let result = result.map(|assigned| match assigned {
            Some(brep) => {
                a = Some(brep);
                true
            }
            None => false,
        });
        self.restore_brep(ids.0, a);
        self.restore_brep(ids.1, b);
        *primary = ids.0.filter(|id| !self.retired.contains(id));
        *other = ids.1.filter(|id| !self.retired.contains(id));
        result.map(|assigned| if assigned { ids.0 } else { None })
    }
}
impl PiecewiseAccess for Pool {
    type Surface = (usize, usize);
    fn operands(&self) -> (Option<usize>, Option<usize>) {
        (self.primary, self.other)
    }
    fn set_operands(&mut self, a: Option<usize>, b: Option<usize>) {
        self.primary = a;
        self.other = b;
        self.intersector.set_primary_brep(a.map(PoolRef::Brep));
        self.intersector.set_other_brep(b.map(PoolRef::Brep));
    }
    fn surfaces(&self, id: usize) -> solidcore::Result<Vec<Self::Surface>> {
        let brep = self.brep(id)?;
        Ok(bp::surfaces(brep, None)?
            .iter()
            .enumerate()
            .map(|(s, _)| (id, s))
            .collect())
    }
    fn intersect_insert_relate(&mut self, _k: &mut Kernel) -> solidcore::Result<()> {
        // UNPORTED(boolean.topology_intersector_p2): SmTopologyIntersector::IntersectInsertRelate
        Err(SolidError::unported(
            "SmTopologyIntersector::IntersectInsertRelate",
            "boolean.topology_intersector_p2",
        ))
    }
    fn clear_relationships(&mut self) -> solidcore::Result<()> {
        self.intersector.clear_relationships()
    }
    fn set_subset(&mut self, surfaces: &[Self::Surface]) {
        let handles: Vec<_> = surfaces
            .iter()
            .map(|&(b, s)| PoolRef::Surface(b, s))
            .collect();
        self.intersector.set_subset(&handles);
    }
    fn surface_map(
        &mut self,
        surface: Self::Surface,
        only_if_missing: bool,
    ) -> solidcore::Result<()> {
        let key = self.surface_key(surface)?;
        if only_if_missing
            && self
                .attributes
                .find_attribute(key, ids::SURFACEMAP)
                .is_some()
        {
            return Ok(());
        }
        let attribute = self.attributes.insert(Attribute::pointer(
            ids::SURFACEMAP,
            key.0,
            Behavior::Reference,
        ))?;
        self.attributes.add_attribute(key, attribute)
    }
    fn surface_map_users(
        &self,
        surface: Self::Surface,
    ) -> solidcore::Result<Option<Vec<Self::Surface>>> {
        let key = self.surface_key(surface)?;
        let Some(attribute) = self.attributes.find_attribute(key, ids::SURFACEMAP) else {
            return Ok(None);
        };
        let users = self.attributes.users(attribute)?;
        users
            .into_iter()
            .map(|key| {
                let key = key.0.checked_sub(1).ok_or_else(|| {
                    SolidError::invalid_input("harness::merge_p3", "bad attribute user")
                })?;
                let surface = ((key >> 32) as usize, (key & u64::from(u32::MAX)) as usize);
                self.surface_key(surface)?;
                Ok(surface)
            })
            .collect::<solidcore::Result<Vec<_>>>()
            .map(Some)
    }
    fn new_face_brep(&mut self, k: &mut Kernel, source: usize) -> solidcore::Result<usize> {
        let mut result = empty_brep(
            k,
            solidcore::geom::tol::USE_DEFAULT,
            solidcore::geom::tol::USE_DEFAULT,
            None,
        )?;
        let tolerance = self.brep(source)?.tolerance();
        set_tolerance(k, &mut result, tolerance, true, true)?;
        result.set_editing_enabled(true);
        let id = self.breps.len();
        self.breps.push(result); // append only: a handle is never reused
        Ok(id)
    }
    fn manifold_flag(&self) -> bool {
        self.manifold
    }
    fn set_controls(&mut self, manifold: bool, wires: bool, lamina: u64) {
        self.manifold = manifold;
        self.intersector.intersect_wire_edges = wires;
        self.intersector.intersect_lamina_edges = lamina;
    }
    fn process_wires(&mut self, k: &mut Kernel, lock: &MarkLock) -> solidcore::Result<()> {
        // ProcessWiresAndShellVertices(SM_BO_MERGE, keep, mark1, mark2) (merge_p5) with
        // merge_p2's copy environment. The pool's intersector names only B-reps and
        // surfaces, so its topology mates are empty, and the pool marks no topology:
        // empty Relations and Marks are its actual state, not a second mate map.
        let (Some(p), Some(o)) = (self.primary, self.other) else {
            return Err(SolidError::invalid_input(
                "harness::merge_p3",
                "wire processing without both operands",
            ));
        };
        self.brep(p)?;
        self.brep(o)?;
        if p == o {
            return Err(SolidError::invalid_input(
                "harness::merge_p3",
                "wire processing of one B-rep with itself",
            ));
        }
        let (low, high) = (p.min(o), p.max(o));
        let (head, tail) = self.breps.split_at_mut(high);
        let (lower, upper) = (
            head.get_mut(low)
                .ok_or(SolidError::null_pointer("harness::merge_p3"))?,
            tail.first_mut()
                .ok_or(SolidError::null_pointer("harness::merge_p3"))?,
        );
        let (primary, other) = if p < o {
            (lower, upper)
        } else {
            (upper, lower)
        };
        let mut records = std::collections::BTreeMap::new();
        for (base, brep) in [(0u64, &*primary), (1_000_000, &*other)] {
            for v in brep.vertices().ids() {
                records.insert(ObjectKey(base + u64::from(v.index())), Topology::new());
            }
            for e in brep.edges().ids() {
                records.insert(
                    ObjectKey(2_000_000 + base + u64::from(e.index())),
                    Topology::new(),
                );
            }
        }
        let object = |key: u64, type_code: i64| solidcore::geom::object::ObjectRef {
            key: ObjectKey(key),
            type_code,
        };
        let mut copy = TopologyCopy {
            vertices: VertexCopy {
                records: &mut records,
                attributes: &mut self.attributes,
                source_key: &|v| object(1_000_000 + u64::from(v.index()), 16021),
                destination_key: &|e| match e {
                    solidcore::EntityRef::Vertex(v) => object(u64::from(v.index()), 16021),
                    solidcore::EntityRef::Vertexuse(v) => {
                        object(6_000_000 + u64::from(v.index()), 16006)
                    }
                    _ => object(9_000_000, 16001),
                },
            },
            edge_source_key: &|e| object(3_000_000 + u64::from(e.index()), 16016),
            edge_destination_key: &|e| object(2_000_000 + u64::from(e.index()), 16016),
            face_source_key: &|f| object(5_000_000 + u64::from(f.index()), 16002),
            face_destination_key: &|f| object(4_000_000 + u64::from(f.index()), 16002),
        };
        let mut keep = Vec::new(); // local and discarded, as upstream
        process_wires_and_shell_vertices(
            k,
            primary,
            other,
            &mut Marks::default(),
            &mut Marks::default(),
            &mut Relations::default(),
            &mut copy,
            None,
            BooleanOperation::Merge,
            lock.mark_type1(),
            lock.mark_type2(),
            &mut keep,
        )
    }
    fn release_marks(&mut self, k: &mut Kernel, lock: &mut MarkLock) -> solidcore::Result<()> {
        lock.release(std::slice::from_mut(&mut k.context))
    }
    fn register_empty_ssi(&mut self, a: Self::Surface, b: Self::Surface) -> solidcore::Result<()> {
        register_ssi(&mut self.intersector, a, b, &[], &[], &[], &[], &[])
    }
    fn clear_existing_ssi(&mut self) -> solidcore::Result<()> {
        // The fixture adapter only registers empty curve arrays. No curve handles
        // can be created here until RegisterExistingSSI is wired above.
        let deleted = self.intersector.clear_existing_ssi();
        if !deleted.is_empty() {
            // UNPORTED(boolean.topology_intersector_p2): SmTopologyIntersector::RegisterExistingSSI (curve storage)
            return Err(SolidError::unported(
                "SmTopologyIntersector::RegisterExistingSSI (curve storage)",
                "boolean.topology_intersector_p2",
            ));
        }
        Ok(())
    }
    fn notify_pre_edit(&mut self, k: &mut Kernel, a: usize) -> solidcore::Result<()> {
        self.brep(a)?;
        let brep = self
            .breps
            .get_mut(a)
            .ok_or_else(|| SolidError::null_pointer("harness::merge_p3"))?;
        let caller = solidcore::geom::object::ObjectRef {
            key: brep.key(),
            type_code: solidcore::geom::a_object::BREP_TYPE,
        };
        notify(
            k,
            brep,
            solidcore::geom::object::NotifyOperation::PreEdit,
            caller,
            None,
            None,
        )
    }
    fn delete_other(&mut self, k: &mut Kernel) -> solidcore::Result<()> {
        if let Some(id) = self.other {
            self.brep(id)?;
            let brep = self
                .breps
                .get_mut(id)
                .ok_or_else(|| SolidError::null_pointer("harness::merge_p3"))?;
            destroy_brep(k, brep, &mut self.attributes)?;
            self.retired.insert(id);
        }
        self.set_operands(self.primary, None);
        Ok(())
    }
}

fn destroy_brep(
    k: &mut Kernel,
    brep: &mut Brep,
    _attributes: &mut AttributeStore,
) -> solidcore::Result<()> {
    solidcore::topology::brep::p4::destroy(k, brep)
}

/// `new SmBrep(dSize, dRatio, pOptContextForStack)`
/// (`topology::brep::p3::new_brep`; `pOptContextForStack` is the allocating kernel).
fn empty_brep(
    k: &mut Kernel,
    model_size_estimate: f64,
    large_small_size_ratio: f64,
    stack_context: Option<&Kernel>,
) -> solidcore::Result<Brep> {
    let _ = stack_context;
    solidcore::topology::brep::p3::new_brep(k, model_size_estimate, large_small_size_ratio)
}

/// `pNewBrep->SetTolerance(tol, bUpdateOnlyIfLarger, bCascadeToBndries)`
/// (`topology::brep::p2::set_tolerance`); the status is ignored (`SmMerge.cpp:466`), so
/// only [`SolidError::Unported`] propagates.
fn set_tolerance(
    k: &mut Kernel,
    brep: &mut Brep,
    tolerance: f64,
    update_only_if_larger: bool,
    cascade_to_boundaries: bool,
) -> solidcore::Result<()> {
    match solidcore::topology::brep::p2::set_tolerance(
        k,
        brep,
        tolerance,
        update_only_if_larger,
        cascade_to_boundaries,
    ) {
        Err(e) if e.is_unported() => Err(e),
        _ => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)] // all five native SSI arrays remain explicit
fn register_ssi(
    _intersector: &mut TopologyIntersector<PoolRef>,
    _a: (usize, usize),
    _b: (usize, usize),
    _curves_3d: &[PoolRef],
    _uv1: &[PoolRef],
    _uv2: &[PoolRef],
    _orientations: &[solidcore::surface::surface::p2::TsectCurveType],
    _deviations: &[f64],
) -> solidcore::Result<()> {
    // UNPORTED(boolean.topology_intersector_p2): SmTopologyIntersector::RegisterExistingSSI
    Err(SolidError::unported(
        "SmTopologyIntersector::RegisterExistingSSI",
        "boolean.topology_intersector_p2",
    ))
}

/// `SmBrep::Notify(op, caller, data1, data2)` (`topology::brep::p2::notify`). The fixture
/// pool carries no attribute records, so the notification runs on an empty store over
/// the B-rep's regions, as `heal.stitch_p1`'s `PRE_EDIT`; no Rust caller holds the
/// B-rep's `SM_OC_BREP` cache.
fn notify(
    _k: &mut Kernel,
    brep: &mut Brep,
    operation: solidcore::geom::object::NotifyOperation,
    caller: solidcore::geom::object::ObjectRef,
    data1: Option<solidcore::geom::object::ObjectRef>,
    data2: Option<solidcore::geom::object::ObjectRef>,
) -> solidcore::Result<()> {
    use solidcore::topology::brep::p2;
    let mut state = p2::RegionNotifyState::new(brep);
    p2::notify(
        brep,
        &mut state.env(),
        None,
        operation,
        [
            Some(p2::NotifyArg::Object(caller)),
            data1.map(p2::NotifyArg::Object),
            data2.map(p2::NotifyArg::Object),
        ],
    )
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    if i.family > 4 && i.family != ENGINE_PROBE {
        return Err(harness("fixture-only family is not a unit operation"));
    }
    ffi::validate(&encode(i)).map_err(|error| harness(&error))?;
    let mut pool = Pool::load(i)?;
    if i.family == ENGINE_PROBE {
        return engine_probe(i, &mut pool);
    }
    let mut k = Kernel::default();
    let opts = i.options.map(options);
    let mut values = vec![0.0];
    let result: solidcore::Result<()> = match i.family {
        0 => {
            let mut ids: Vec<_> = (0..i.a.len()).map(Some).collect();
            port::boolean_trees(&mut k, &mut ids, &i.tree, opts.as_ref(), &mut pool).and_then(
                |()| {
                    values.push(ids.len() as f64);
                    for id in ids {
                        if let Some(id) = id {
                            pool.record(id, &mut values)?;
                        } else {
                            values.extend([-1.0, 0.0]);
                        }
                    }
                    Ok(())
                },
            )
        }
        1 => {
            let mut ids: Vec<_> = (0..i.a.len()).collect();
            let op = u32::try_from(i.operation).unwrap_or(u32::MAX);
            port::merge_breps(&mut k, &mut ids, op, opts.as_ref(), &mut pool).and_then(|id| {
                values.push(1.0);
                pool.record(id, &mut values)
            })
        }
        2 => port::boolean_lists(
            &mut k,
            pool.breps
                .get(..i.a.len())
                .ok_or_else(|| harness("bad first list"))?,
            &[],
            pool.breps
                .get(i.a.len()..)
                .ok_or_else(|| harness("bad second list"))?,
            &[],
            i.operation,
            opts.as_ref(),
        )
        .and_then(|result| list_result(&result, &mut values)),
        3 => {
            let mut primary = Some(0);
            let mut other = Some(1);
            let mut manifold = true;
            let mut lamina = 0;
            let operation = boolean_operation(i.operation)?;
            // MergeAccess merge(context, a, b, 0.0, 0.0, options), as the shim builds it.
            pool.construct(&mut k, &mut primary, &mut other, 0.0, 0.0, opts.as_ref())
                .and_then(|()| {
                    port::non_manifold_boolean(
                        &mut k,
                        &mut pool,
                        &mut primary,
                        &mut other,
                        &mut manifold,
                        &mut lamina,
                        operation,
                    )
                })
                .and_then(|result| single_result(&pool, result, manifold, lamina, &mut values))
        }
        4 => port::piecewise_merge(
            &mut k,
            &mut pool,
            None,
            i.options.unwrap_or(0) & 1 != 0,
            i.options.unwrap_or(0) & 2 != 0,
        )
        .and_then(|result| {
            single_result(
                &pool,
                result,
                pool.manifold,
                pool.intersector.intersect_lamina_edges,
                &mut values,
            )
        }),
        _ => return Err(harness("fixture-only family")),
    };
    match result {
        Ok(()) => output(&values, i.family),
        Err(error) => Ok(Output::unported(&error)
            .unwrap_or_else(|| Output::err(format!("status={}", status(&error))))),
    }
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.merge_p3"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(256),
                ..ctx.clone()
            },
        )
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs =
            serde_json::from_value(case.inputs.clone()).map_err(|error| harness(&error))?;
        if let Some(expected) = &i.expected_refusal {
            return refusal(&i, expected);
        }
        rust(&i)
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs =
            serde_json::from_value(case.inputs.clone()).map_err(|error| harness(&error))?;
        if i.family > 4 && i.family != ENGINE_PROBE {
            return Err(harness("fixture-only family is not a unit operation"));
        }
        if let Some(expected) = &i.expected_refusal {
            return refusal(&i, expected);
        }
        if i.family == ENGINE_PROBE {
            return native_engine_output(&ffi::eval(&encode(&i)).map_err(|error| harness(&error))?);
        }
        native_output(
            &ffi::eval(&encode(&i)).map_err(|error| harness(&error))?,
            i.family,
        )
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let family = match rng.below(10) {
            0..=5 => 0,
            6 => 1,
            7 => 2,
            8 => 3,
            _ => 4,
        };
        let n = if family >= 3 {
            1
        } else {
            rng.below(6) as usize
        };
        let m = if family == 0 || family == 1 {
            0
        } else if family >= 3 {
            1
        } else {
            rng.below(3) as usize
        };
        let scale = *rng.choose(&[1.0e-9, 1.0e-3, 1.0, 1.0e6, 1.0e12])?;
        let mut operand = || Operand {
            point: rng.point3(-scale, scale),
            size: None,
        };
        let a: Vec<_> = (0..n).map(|_| operand()).collect();
        let b: Vec<_> = (0..m).map(|_| operand()).collect();
        let mut tree: Vec<i64> = Vec::new();
        if family == 0 {
            let mut ids: Vec<_> = (0..n as i64).collect();
            for j in (1..ids.len()).rev() {
                ids.swap(j, rng.below((j + 1) as u64) as usize);
            }
            tree.extend(ids.into_iter().take(rng.below((n + 1) as u64) as usize));
            if rng.below(3) == 0 {
                tree.push(-rng.range_i64(1, 7));
            }
        }
        let operation = if family >= 3 {
            rng.below(11) as i32
        } else {
            rng.range_i64(-1, 5) as i32
        };
        let options = if rng.below(2) == 0 {
            None
        } else {
            Some(rng.below(32) as u8)
        };
        Some(json!(Inputs {
            family,
            operation,
            options,
            a,
            b,
            tree,
            expected_refusal: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            family: 3,
            operation: 1,
            options: None,
            a: vec![Operand {
                point: [0.0, 0.0, 0.0],
                size: Some([2.0, 3.0, 4.0]),
            }],
            b: vec![Operand {
                point: [5.0, 0.0, 0.0],
                size: None,
            }],
            tree: Vec::new(),
            expected_refusal: None,
        }
    }

    fn fixture_graph() -> Vec<f64> {
        let mut input = inputs();
        input.family = 5;
        input.b.clear();
        ffi::eval(&encode(&input)).unwrap()
    }

    /// Both engines must complete each nested fixture with its expected vertex count.
    #[cfg(feature = "cpp")]
    #[test]
    fn nested_postfix_fixtures_compare_successful_geometry() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/boolean.merge_p3/nested_postfix.json"
        );
        let text = std::fs::read_to_string(path).unwrap();
        let fixtures: Value = serde_json::from_str(&text).unwrap();
        for fixture in fixtures["cases"].as_array().unwrap() {
            let input: Inputs = serde_json::from_value(fixture["inputs"].clone()).unwrap();
            let expected_vertices = fixture["expected_vertices"].as_i64().unwrap();
            let native = native_output(&ffi::eval(&encode(&input)).unwrap(), input.family).unwrap();
            for result in [rust(&input).unwrap(), native] {
                assert!(result.error.is_none(), "{}: {:?}", fixture["name"], result.error);
                assert!(result.deferred.is_none(), "{}: {:?}", fixture["name"], result.deferred);
                assert!(result.rejected.is_none(), "{}: {:?}", fixture["name"], result.rejected);
                assert_eq!(result.ints["status"], [0], "{}", fixture["name"]);
                let vertices: i64 = result.ints["topology_counts"]
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|counts| counts[2].max(0))
                    .sum();
                assert_eq!(vertices, expected_vertices, "{}", fixture["name"]);
                assert!(!result.values["record"].is_empty(), "{}", fixture["name"]);
            }
        }
    }

    #[test]
    fn single_success_records_compare_rust_geometry_controls_and_nulls() {
        let mut pool = Pool::load(&inputs()).unwrap();
        let graph = fixture_graph();
        let mut native = vec![0.0, 1.0, 1.0, 1.0, 0.0, graph.len() as f64];
        native.extend(&graph);
        let mut rust = vec![0.0];
        single_result(&pool, Some(0), true, 1, &mut rust).unwrap();
        for family in [3, 4] {
            assert_eq!(
                output(&rust, family).unwrap(),
                native_output(&native, family).unwrap()
            );
            let mut changed = vec![0.0];
            single_result(&pool, Some(0), false, 3, &mut changed).unwrap();
            assert_ne!(
                output(&changed, family).unwrap(),
                native_output(&native, family).unwrap()
            );
            let mut null = vec![0.0];
            single_result(&pool, None, true, 1, &mut null).unwrap();
            assert_eq!(
                output(&null, family).unwrap(),
                native_output(&[0.0, 1.0, 1.0, 0.0], family).unwrap()
            );
        }
        let vertex = pool.breps[0].vertices().ids().next().unwrap();
        pool.breps[0].vertex_mut(vertex).unwrap().point.x += 0.125;
        let mut changed = vec![0.0];
        single_result(&pool, Some(0), true, 1, &mut changed).unwrap();
        assert_ne!(
            output(&changed, 3).unwrap(),
            native_output(&native, 3).unwrap()
        );
    }

    #[test]
    fn list_success_records_compare_actual_outputs_and_surface_count() {
        let pool = Pool::load(&inputs()).unwrap();
        let graph = fixture_graph();
        let mut native = vec![0.0, 1.0, -1.0, graph.len() as f64];
        native.extend(&graph);
        native.push(0.0);
        let mut result = port::BooleanListsResult {
            breps: vec![pool.breps[0].clone()],
            surfaces: Vec::new(),
        };
        let mut rust = vec![0.0];
        list_result(&result, &mut rust).unwrap();
        assert_eq!(
            output(&rust, 2).unwrap(),
            native_output(&native, 2).unwrap()
        );
        result.surfaces.push(
            solidcore::surface::plane::Plane::from_normal(
                solidcore::math::Vec3::ZERO,
                solidcore::math::Vec3::new(0.0, 0.0, 1.0),
            )
            .into(),
        );
        let mut changed = vec![0.0];
        list_result(&result, &mut changed).unwrap();
        assert_ne!(
            output(&changed, 2).unwrap(),
            native_output(&native, 2).unwrap()
        );
        assert_eq!(
            native_output(&[0.0, 0.0, 0.0], 2).unwrap(),
            output(&[0.0, 0.0, 0.0], 2).unwrap()
        );
    }

    #[test]
    fn piecewise_adapter_uses_canonical_subset_attributes_controls_and_marks() {
        let mut pool = Pool::load(&inputs()).unwrap();
        assert_eq!(pool.surfaces(0).unwrap().len(), 6);
        assert!(pool.surfaces(1).unwrap().is_empty());
        pool.set_subset(&[(0, 0), (0, 0)]);
        assert_eq!(pool.intersector.subset().unwrap().len(), 1);
        pool.set_subset(&[]);
        assert!(pool.intersector.subset().is_none());
        pool.surface_map((0, 0), false).unwrap();
        assert_eq!(pool.surface_map_users((0, 0)).unwrap(), Some(vec![(0, 0)]));
        let key = pool.surface_key((0, 0)).unwrap();
        pool.surface_map((0, 0), true).unwrap();
        assert_eq!(pool.attributes.attributes(key).len(), 1);
        pool.surface_map((0, 0), false).unwrap();
        assert_eq!(pool.attributes.attributes(key).len(), 2);
        pool.set_controls(false, false, 3);
        assert!(!pool.manifold_flag() && !pool.intersector.intersect_wire_edges);
        assert_eq!(pool.intersector.intersect_lamina_edges, 3);
        pool.clear_relationships().unwrap();
        pool.clear_existing_ssi().unwrap();
        let mut k = Kernel::default();
        let mut lock = MarkLock::new(
            std::slice::from_mut(&mut k.context),
            0,
            solidcore::geom::context::ALL_MARKS,
        )
        .unwrap();
        pool.release_marks(&mut k, &mut lock).unwrap();
        assert_eq!(lock, MarkLock::unbound());
        pool.retired.insert(0); // the successful destructor's logical retirement
        pool.set_operands(Some(0), None);
        assert!(pool.surfaces(0).is_err());
        assert!(pool.record(0, &mut Vec::new()).is_err());
        assert!(pool.delete_other(&mut k).is_ok()); // delete NULL performs no call
    }

    #[test]
    fn piecewise_dependency_errors_name_the_next_missing_owner() {
        let mut pool = Pool::load(&inputs()).unwrap();
        let mut k = Kernel::default();
        // the face B-rep is built (SmBrep constructor, topology.brep_p3;
        // CreateFaceFromSurface, topology.brep_p8)
        let r = pool.new_face_brep(&mut k, 0);
        assert!(r.is_ok(), "new_face_brep: {r:?}");
        // SmBrep::Notify is topology.brep_p2's (#149): PRE_EDIT of the fixture B-rep runs
        let r = pool.notify_pre_edit(&mut k, 0);
        assert!(r.is_ok(), "notify_pre_edit: {r:?}");
        assert!(matches!(
            pool.register_empty_ssi((0, 0), (0, 1)),
            Err(SolidError::Unported {
                unit: "boolean.topology_intersector_p2",
                ..
            })
        ));
        // SmMerge's constructor and copy environment are boolean.merge_p2's (#229) and
        // SmBrep::Point3DClassify is query.brep's: ProcessWiresAndShellVertices runs to
        // completion on the fixture
        let r = pool.process_wires(&mut k, &MarkLock::unbound());
        assert!(r.is_ok(), "process_wires: {r:?}");
        // ~SmBrep is topology.brep_p4's: the other operand is destroyed and released
        let r = pool.delete_other(&mut k);
        assert!(r.is_ok(), "delete_other: {r:?}");
        assert_eq!(pool.other, None);
    }
}
