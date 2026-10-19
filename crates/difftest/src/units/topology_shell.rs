//! Native shell/region operations on shared native B-rep fixtures: queries, wire edges,
//! `TransferChildrenTo` (incl. face-use and wire shells in one B-rep), shell type members,
//! `RayFire` (empty-shape branch only), degeneracy, region ordering and filtering.
//! Native fixtures and raw graph imports reuse the edge-use fixture protocol.
//! Setup failures fail the harness; topology and native integer flags compare exactly.

use super::topology_edgeuse_p1::{fixture_input, import};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::topology::shell::{shell_brep, shell_region, wire_edges};
use solidcore::topology::{FaceuseId, Region, Shell, ShellId};
use solidcore_ffi::units::{topology_edgeuse_p1 as graph, topology_shell as ffi};

/// The shell/region unit runner.
pub struct Runner;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    brep: Value,
    #[serde(default)]
    void_state: Option<i32>,
    #[serde(default)]
    wires: bool,
    #[serde(default)]
    transfer: Option<TransferInputs>,
    #[serde(default)]
    type_probe: Option<TypeProbe>,
    #[serde(default)]
    degenerate: Option<DegenerateInput>,
    #[serde(default)]
    ray: Option<RayInput>,
    #[serde(default)]
    region_order: Option<RegionOrder>,
    #[serde(default)]
    region_filter: Option<RegionFilter>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionOrder {
    prefix_empty: usize,
    #[serde(default)]
    gather_faces: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionFilter {
    attribute_id: u64,
    enabled: bool,
    pattern: u8,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RayInput {
    #[serde(with = "ray_num3")]
    start: [f64; 3],
    #[serde(with = "ray_num3")]
    direction: [f64; 3],
    #[serde(with = "ray_num")]
    tolerance: f64,
    #[serde(with = "ray_opt_num", default)]
    radius: Option<f64>,
}

/// JSON numbers, plus the strings `"nan"`, `"inf"` and `"-inf"` for the values JSON
/// cannot hold (the degeneracy tolerance uses the same spelling).
fn json_number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| match value.as_str()? {
        "nan" => Some(f64::NAN),
        "inf" => Some(f64::INFINITY),
        "-inf" => Some(f64::NEG_INFINITY),
        _ => None,
    })
}

fn number_json(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else if value.is_nan() {
        json!("nan")
    } else if value > 0.0 {
        json!("inf")
    } else {
        json!("-inf")
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde `with` signature
mod ray_num {
    use super::{json_number, number_json, Value};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        number_json(*v).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        json_number(&Value::deserialize(d)?)
            .ok_or_else(|| serde::de::Error::custom("number, \"nan\", \"inf\" or \"-inf\""))
    }
}

#[allow(clippy::ref_option)] // serde `with` signature
mod ray_opt_num {
    use super::{json_number, number_json, Value};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
        v.map(number_json).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => json_number(&v)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("number, \"nan\", \"inf\" or \"-inf\"")),
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde `with` signature
mod ray_num3 {
    use super::{json_number, number_json, Value};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[f64; 3], s: S) -> Result<S::Ok, S::Error> {
        v.map(number_json).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f64; 3], D::Error> {
        let values = <[Value; 3]>::deserialize(d)?;
        let mut out = [0.0; 3];
        for (slot, value) in out.iter_mut().zip(&values) {
            *slot = json_number(value).ok_or_else(|| serde::de::Error::custom("ray number"))?;
        }
        Ok(out)
    }
}

// This adapter supports exactly empty shape trees (harness-limited, see below). There are no geometry
// handles to invent: every object-dependent accessor has an uninhabited input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NoRayObject {}
pub(crate) struct EmptyRayWorld;
impl solidcore::geom::solution_array::SolutionObject for NoRayObject {
    fn kind(&self) -> solidcore::geom::solution_array::ObjectKind {
        match *self {}
    }
}
impl solidcore::intersect::point_class::ClassObject for NoRayObject {
    fn class_kind(&self) -> solidcore::intersect::point_class::ClassObjectKind {
        match *self {}
    }
}
impl solidcore::topology::shell::ShellRayWorld<NoRayObject> for EmptyRayWorld {
    // Harness-limited: a non-empty boundary-face shape needs the B-rep's cached
    // geometry (`topology::brep_cache::BrepObjects` over a built `BrepCache`, which
    // is `query.brep`) plus `SmBrep::LocalCurveSector` (`topology.brep_p2`). The
    // cases whose C++ run reaches that stage are deferred, never compared here;
    // swap this world for `BrepObjects` when those units are ported.
    fn shell_ray_shape(
        &self,
        faces: &[solidcore::geom::object::FaceKey],
    ) -> solidcore::Result<solidcore::geom::object::Shape> {
        if faces.is_empty() {
            return solidcore::geom::object::Shape::new(&[], &[], &[], &[]);
        }
        // UNPORTED(query.brep): SmBrep cache for SmShape/ShapePointSolve on faces
        // (harness-limited: this adapter has no B-rep geometry; see the note above)
        Err(solidcore::SolidError::unported(
            "SmShape::SmShape / SmTopologySolver::ShapePointSolve on boundary faces",
            "query.brep",
        ))
    }
}
impl solidcore::intersect::topology_solver::p1::TopologyObjects<NoRayObject> for EmptyRayWorld {
    fn curve(&self, object: NoRayObject) -> solidcore::Result<&solidcore::curve::curve::Curve> {
        match object {}
    }
    fn analytic_curve_handle(&self, object: NoRayObject) -> Option<NoRayObject> {
        match object {}
    }
    fn vertex_point(&self, object: NoRayObject) -> solidcore::Result<solidcore::math::Point3> {
        match object {}
    }
    fn vertex_tolerance(&self, object: NoRayObject) -> solidcore::Result<f64> {
        match object {}
    }
    fn curve_edge(&self, object: NoRayObject) -> solidcore::Result<Option<NoRayObject>> {
        match object {}
    }
    fn edge_curve(&self, object: NoRayObject) -> solidcore::Result<Option<NoRayObject>> {
        match object {}
    }
    fn edge_interval(
        &self,
        object: NoRayObject,
    ) -> solidcore::Result<solidcore::geom::extent1d::Extent1d> {
        match object {}
    }
    fn surface_face(&self, object: NoRayObject) -> solidcore::Result<Option<NoRayObject>> {
        match object {}
    }
    fn face_surface(&self, object: NoRayObject) -> solidcore::Result<NoRayObject> {
        match object {}
    }
    fn surface(
        &self,
        object: NoRayObject,
    ) -> solidcore::Result<&solidcore::surface::surface::Surface> {
        match object {}
    }
    fn face_uv_domain(
        &self,
        object: NoRayObject,
    ) -> solidcore::Result<solidcore::geom::extent2d::Extent2d> {
        match object {}
    }
    fn face_tolerance(&self, object: NoRayObject) -> solidcore::Result<f64> {
        match object {}
    }
    fn surface_owner(&self, object: NoRayObject) -> solidcore::Result<Option<NoRayObject>> {
        match object {}
    }
    fn shape_member(&self, object: NoRayObject) -> solidcore::geom::object::ShapeMember {
        match object {}
    }
}
impl solidcore::intersect::point_class::ClassGeometry<NoRayObject> for EmptyRayWorld {
    fn curve(&self, object: NoRayObject) -> Option<&solidcore::curve::curve::Curve> {
        match object {}
    }
    fn surface(&self, object: NoRayObject) -> Option<&solidcore::surface::surface::Surface> {
        match object {}
    }
    fn volume(&self, object: NoRayObject) -> Option<&solidcore::surface::volume::Volume> {
        match object {}
    }
    fn point(&self, object: NoRayObject) -> Option<solidcore::math::Point3> {
        match object {}
    }
}

/// The mark types whose counters the shim reports, in its order.
const MARK_TYPES: [solidcore::geom::context::MarkType; 5] = {
    use solidcore::geom::context::MarkType::{Mark, Mark2, Mark3, MarkAssert, MarkIo};
    [Mark, Mark2, Mark3, MarkIo, MarkAssert]
};

/// Current mark counters of the kernel context, taken before a compared call.
fn mark_counters(k: &solidcore::kernel::Kernel) -> [u64; 5] {
    MARK_TYPES.map(|mark| k.context.current_mark(mark))
}

/// How far a call advanced each mark counter, then whether each is still locked
/// (the shim's `MarkSnap::put`): upstream's `SmNewMarkAndLock` side effects.
fn mark_delta(k: &solidcore::kernel::Kernel, before: [u64; 5]) -> [i64; 10] {
    let mut out = [0; 10];
    for ((mark, was), slot) in MARK_TYPES.into_iter().zip(before).zip(out.iter_mut()) {
        *slot = k.context.current_mark(mark).wrapping_sub(was) as i64;
    }
    for (mark, slot) in MARK_TYPES.into_iter().zip(out.iter_mut().skip(5)) {
        *slot = i64::from(k.context.is_mark_locked(mark));
    }
    out
}

const RAY_RECORD: usize = 17;

fn ray_output(values: &[f64]) -> Result<Output, HarnessError> {
    if !values.len().is_multiple_of(RAY_RECORD) {
        return Err(failure("invalid ray output length"));
    }
    let mut ints = Vec::new();
    let mut floats = Vec::new();
    for record in values.as_chunks::<RAY_RECORD>().0 {
        for value in record.iter().take(4).chain(record.iter().skip(7)) {
            if !value.is_finite() || value.fract() != 0.0 || value.abs() > (1_u64 << 53) as f64 {
                return Err(failure("invalid ray integer output"));
            }
            ints.push(*value as i64);
        }
        floats.extend(record.iter().skip(4).take(3).copied());
    }
    Ok(Output::new()
        .ints("records", ints)
        .values("classification", floats))
}

fn rust_ray(brep: &solidcore::topology::Brep, input: &RayInput) -> Result<Output, HarnessError> {
    use solidcore::intersect::point_class::{PointClassType, PointClassification};
    use solidcore::topology::shell::{ray_fire, ShellRayInput, ShellRayState};
    let mut kernel = solidcore::kernel::Kernel::default();
    let mut out = Vec::new();
    // The shim's guard: non-finite ray values would reach the intersection solver
    // through a shell's boundary faces (a face use whose mate lies in another shell).
    let finite = input
        .start
        .iter()
        .chain(&input.direction)
        .all(|v| v.is_finite())
        && input.tolerance.is_finite()
        && input.radius.is_none_or(f64::is_finite);
    if !finite {
        for id in brep.shells().ids() {
            for faceuse in brep
                .shell(id)
                .map_err(failure)?
                .get_faceuses()
                .map_err(failure)?
            {
                let mate = brep.faceuse(faceuse).map_err(failure)?.mate;
                if let Some(mate) = mate {
                    if brep.faceuse(mate).map_err(failure)?.shell != Some(id) {
                        return Ok(Output::err("rejected_input"));
                    }
                }
            }
        }
    }
    for id in brep.shells().ids() {
        let counters = mark_counters(&kernel);
        let mut classification = PointClassification::<NoRayObject>::new(0.125);
        classification.set_class_object(PointClassType::Point, None);
        classification.set_gap_3d(123.0);
        classification.set_pre_snap_param(456.0);
        let attempt = ray_fire(
            &mut kernel,
            brep,
            solidcore::topology::BrepId::from_index(0),
            id,
            &EmptyRayWorld,
            ShellRayInput {
                start: solidcore::math::Point3::new(input.start[0], input.start[1], input.start[2]),
                direction: solidcore::math::Vec3::new(
                    input.direction[0],
                    input.direction[1],
                    input.direction[2],
                ),
                zone_tolerance: input.tolerance,
                sphere_bound: input.radius,
                state: ShellRayState {
                    region: brep.regions().ids().next(),
                    classification,
                },
            },
        );
        let status = match attempt.status {
            Ok(()) => 0,
            Err(error) => {
                if let Some(deferred) = Output::unported(&error) {
                    return Ok(deferred);
                }
                match error {
                    solidcore::error::SolidError::Upstream { code, .. } => code,
                    error => return Err(failure(error)),
                }
            }
        };
        let pc = attempt.state.classification;
        out.extend([
            f64::from(status),
            attempt.state.region.map_or(-1.0, |r| f64::from(r.index())),
            f64::from(pc.point_class().code()),
            f64::from(pc.object().is_none()),
            pc.src_zone_tol_3d(),
            pc.gap_3d(&kernel, &EmptyRayWorld, None).map_err(failure)?,
            pc.pre_snap_param(),
        ]);
        out.extend(mark_delta(&kernel, counters).map(|v| v as f64));
    }
    ray_output(&out)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DegenerateInput {
    tolerance: Option<Value>,
}

fn degeneracy_tolerance(input: &DegenerateInput) -> Result<Option<f64>, HarnessError> {
    let Some(value) = &input.tolerance else {
        return Ok(None);
    };
    let value = if let Some(number) = value.as_f64() {
        number
    } else {
        match value.as_str() {
            Some("nan") => f64::NAN,
            Some("inf") => f64::INFINITY,
            Some("-inf") => f64::NEG_INFINITY,
            _ => return Err(failure("invalid degeneracy tolerance")),
        }
    };
    Ok(Some(value))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeProbe {
    code: i64,
    shell: Option<usize>,
}

fn type_probe_index(probe: &TypeProbe, counts: [usize; 10]) -> Result<Option<usize>, HarnessError> {
    match probe.shell {
        None => Ok(None),
        Some(index) => {
            let count = counts.get(1).copied().unwrap_or(0);
            if count == 0 {
                return Err(failure("type fixture has no existing shell"));
            }
            Ok(Some(index % count))
        }
    }
}

fn rust_type_members(
    brep: &mut solidcore::topology::Brep,
    probe: &TypeProbe,
    selected: Option<usize>,
) -> Result<Output, HarnessError> {
    use solidcore::topology::shell::ShellMembers;
    let id = if let Some(index) = selected {
        ShellId::from_index(u32::try_from(index).map_err(failure)?)
    } else {
        let region = brep
            .infinite_region()
            .ok_or_else(|| failure("fixture has no infinite region"))?;
        let id = brep
            .add_shell_record(Shell {
                region: Some(region),
                ..Shell::new()
            })
            .map_err(failure)?;
        brep.region_mut(region).map_err(failure)?.shells.push(id);
        id
    };
    brep.shell_mut(id)
        .map_err(failure)?
        .set_shell_type(probe.code);
    let shell = brep.shell(id).map_err(failure)?;
    let (kind, head, count) = match &shell.members {
        ShellMembers::Empty => (0, -1, 0),
        ShellMembers::Faceuses(ids) => (
            if ids.is_empty() { 0 } else { 16022 },
            ids.first().map_or(-1, |id| i64::from(id.index())),
            ids.len(),
        ),
        ShellMembers::Vertexuse(id) => (16006, i64::from(id.index()), 1),
        ShellMembers::Edgeuse(id) => {
            let edge = brep
                .edgeuse(*id)
                .map_err(failure)?
                .edge
                .ok_or_else(|| failure("head has no edge"))?;
            (
                16005,
                i64::from(id.index()),
                brep.edge(edge).map_err(failure)?.edgeuses.len(),
            )
        }
    };
    Ok(Output::new().ints(
        "records",
        vec![
            shell.shell_type.code(),
            flag(shell.is_vertex_shell()),
            flag(shell.is_wire_shell()),
            flag(shell.is_faceuse_shell()),
            flag(shell.is_unknown_shell()),
            shell
                .first_faceuse()
                .map_err(failure)?
                .map_or(-1, |id| i64::from(id.index())),
            shell
                .wire_edge(brep)
                .map_err(failure)?
                .map_or(-1, |id| i64::from(id.index())),
            shell
                .vertex(brep)
                .map_err(failure)?
                .map_or(-1, |id| i64::from(id.index())),
            kind,
            head,
            count as i64,
        ],
    ))
}

const TYPES: [i64; 9] = [16013, 16012, 16000, 16999, -1, 16010, 16001, 12060, 11001];

fn failure(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}
fn flag(value: bool) -> i64 {
    i64::from(value)
}
fn list(out: &mut Vec<i64>, values: impl IntoIterator<Item = u32>) {
    let values: Vec<_> = values.into_iter().collect();
    out.push(values.len() as i64);
    out.extend(values.into_iter().map(i64::from));
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferInputs {
    #[serde(default)]
    split_faceuse: bool,
    #[serde(default)]
    require_faceuse_pair: bool,
    #[serde(default)]
    extra_empty: usize,
    source: usize,
    recipient: usize,
    candidates: Option<Vec<i64>>,
    /// Transfers without a candidate list that run before the compared one
    /// (`[source, recipient]`, each modulo the shell count), e.g. to give a face-use
    /// shell wire edges. Their statuses end the output.
    #[serde(default)]
    pre: Vec<[usize; 2]>,
}

struct ResolvedTransfer {
    pre: Vec<(usize, usize)>,
    split_faceuse: bool,
    require_faceuse_pair: bool,
    extra_empty: usize,
    source: usize,
    recipient: usize,
    candidates: Option<Vec<Option<usize>>>,
}

fn resolve_transfer(
    input: &TransferInputs,
    counts: [usize; 10],
) -> Result<ResolvedTransfer, HarnessError> {
    if input.extra_empty > 64 {
        return Err(failure("too many empty shells"));
    }
    let shells = counts
        .get(1)
        .copied()
        .and_then(|n| n.checked_add(input.extra_empty))
        .ok_or_else(|| failure("shell count overflow"))?;
    if shells == 0 {
        return Err(failure("transfer fixture has no shells"));
    }
    let edges = counts
        .get(7)
        .copied()
        .ok_or_else(|| failure("missing edge count"))?;
    let candidates = if let Some(list) = &input.candidates {
        let mut resolved = Vec::new();
        for &entry in list {
            // `-1`, and any selector into a B-rep without edges, is a NULL entry.
            resolved.push(if entry == -1 || (entry >= 0 && edges == 0) {
                None
            } else if entry < 0 {
                return Err(failure("invalid wire candidate selector"));
            } else {
                Some(usize::try_from(entry).map_err(failure)? % edges)
            });
        }
        Some(resolved)
    } else {
        None
    };
    if input.pre.len() > 8 {
        return Err(failure("too many preliminary transfers"));
    }
    Ok(ResolvedTransfer {
        pre: input
            .pre
            .iter()
            .map(|&[from, to]| (from % shells, to % shells))
            .collect(),
        split_faceuse: input.split_faceuse,
        require_faceuse_pair: input.require_faceuse_pair,
        extra_empty: input.extra_empty,
        source: input.source % shells,
        recipient: input.recipient % shells,
        candidates,
    })
}

fn rust_transfer_records(
    brep: &mut solidcore::topology::Brep,
    options: &ResolvedTransfer,
) -> Result<Output, HarnessError> {
    use solidcore::topology::shell::{faceuse_sibling_links, transfer_children, ShellMembers};
    use solidcore::topology::{EdgeId, EdgeuseOwner, VertexuseOwner};
    use solidcore::SolidError;
    let region = brep
        .infinite_region()
        .ok_or_else(|| failure("fixture has no infinite region"))?;
    for _ in 0..options.extra_empty {
        let shell = brep
            .add_shell_record(Shell {
                region: Some(region),
                ..Shell::new()
            })
            .map_err(failure)?;
        brep.region_mut(region).map_err(failure)?.shells.push(shell);
    }
    if options.split_faceuse {
        if options.extra_empty == 0 {
            return Err(failure("split fixture needs an extra shell"));
        }
        let original = ShellId::from_index(0);
        let extra_index = brep
            .shells()
            .len()
            .checked_sub(options.extra_empty)
            .ok_or_else(|| failure("extra shell index underflow"))?;
        let extra = ShellId::from_index(u32::try_from(extra_index).map_err(failure)?);
        let members = match &mut brep.shell_mut(original).map_err(failure)?.members {
            ShellMembers::Faceuses(members) if members.len() >= 2 => members,
            _ => return Err(failure("split fixture needs a face-use cycle")),
        };
        let moved = members.remove(0);
        let target = brep.shell_mut(extra).map_err(failure)?;
        target.shell_type = solidcore::topology::shell::ShellType::Faceuse;
        target.members = ShellMembers::Faceuses(vec![moved]);
        brep.faceuse_mut(moved).map_err(failure)?.shell = Some(extra);
        // Construct the two baseline cycles independently of the transfer port.
        for id in [original, extra] {
            let members = match &brep.shell(id).map_err(failure)?.members {
                ShellMembers::Faceuses(members) => members.clone(),
                _ => return Err(failure("split fixture lost its cycle")),
            };
            let mut previous = members.last().copied();
            for (&member, &next) in members.iter().zip(members.iter().cycle().skip(1)) {
                brep.faceuse_mut(member).map_err(failure)?.sibling_links =
                    Some((Some(next), previous));
                previous = Some(member);
            }
        }
    }
    let candidates = if let Some(list) = &options.candidates {
        let mut ids = Vec::new();
        for &entry in list {
            ids.push(match entry {
                None => None,
                Some(index) => Some(EdgeId::from_index(u32::try_from(index).map_err(failure)?)),
            });
        }
        Some(ids)
    } else {
        None
    };
    let source = ShellId::from_index(u32::try_from(options.source).map_err(failure)?);
    let recipient = ShellId::from_index(u32::try_from(options.recipient).map_err(failure)?);
    if options.require_faceuse_pair {
        let source_record = brep.shell(source).map_err(failure)?;
        let target_record = brep.shell(recipient).map_err(failure)?;
        if source == recipient
            || source_record.shell_type.code() != 16022
            || target_record.shell_type.code() != 16022
            || !matches!(&source_record.members,ShellMembers::Faceuses(ids) if !ids.is_empty())
            || !matches!(&target_record.members,ShellMembers::Faceuses(ids) if !ids.is_empty())
        {
            return Err(failure(
                "fixture must contain two distinct nonempty face-use shells",
            ));
        }
    }
    // The native shim rejects exactly the inputs that crash C++ (null dereferences in
    // `GetWireEdges` and in the candidate filter); mirror its predicates so a rejected
    // input compares as `rejected_input` on both sides.
    let mut kernel = solidcore::kernel::Kernel::default();
    let rejected = || Output::err("rejected_input");
    let crashes_wire_edges =
        |brep: &solidcore::topology::Brep, id: ShellId| -> Result<bool, HarnessError> {
            let shell = brep.shell(id).map_err(failure)?;
            Ok(shell.is_wire_shell() && shell.wire_edge(brep).map_err(failure)?.is_none())
        };
    let mut pre_status = Vec::new();
    for &(from, to) in &options.pre {
        for id in brep.shells().ids().collect::<Vec<_>>() {
            if crashes_wire_edges(brep, id)? {
                return Ok(rejected());
            }
        }
        let from = ShellId::from_index(u32::try_from(from).map_err(failure)?);
        let to = ShellId::from_index(u32::try_from(to).map_err(failure)?);
        pre_status.push(match transfer_children(&mut kernel, brep, from, to, None) {
            Ok(()) => 0,
            Err(SolidError::Upstream { code, .. }) => i64::from(code),
            Err(error) => return Output::unported(&error).ok_or_else(|| failure(error)),
        });
    }
    {
        let size = |shell: &Shell| match &shell.members {
            ShellMembers::Empty => 0,
            ShellMembers::Faceuses(ids) => ids.len(),
            ShellMembers::Vertexuse(_) | ShellMembers::Edgeuse(_) => 1,
        };
        let (from, to) = (
            brep.shell(source).map_err(failure)?,
            brep.shell(recipient).map_err(failure)?,
        );
        let reads_candidates = !(size(to) == 0 && from.is_vertex_shell())
            && !(size(to) != 0 && (to.is_vertex_shell() || from.is_vertex_shell()));
        if let Some(list) = &candidates {
            for entry in list {
                let unreadable = match entry {
                    None => true,
                    Some(edge) => solidcore::topology::edgeuse::p1::primary_edgeuse(brep, *edge)
                        .map_err(failure)?
                        .is_none(),
                };
                if reads_candidates && unreadable {
                    return Ok(rejected());
                }
            }
        }
        for id in brep.shells().ids().collect::<Vec<_>>() {
            if id != source && id != recipient && crashes_wire_edges(brep, id)? {
                return Ok(rejected());
            }
        }
        if candidates.is_none() && reads_candidates && crashes_wire_edges(brep, source)? {
            return Ok(rejected());
        }
    }
    let counters = mark_counters(&kernel);
    let status =
        match transfer_children(&mut kernel, brep, source, recipient, candidates.as_deref()) {
            Ok(()) => 0,
            Err(SolidError::Upstream { code, .. }) => i64::from(code),
            Err(error) => return Output::unported(&error).ok_or_else(|| failure(error)),
        };
    let transfer_marks = mark_delta(&kernel, counters);
    let mut out = vec![status];
    out.extend(transfer_marks);
    out.extend([
        brep.shells().len() as i64,
        brep.faceuses().len() as i64,
        brep.edgeuses().len() as i64,
        brep.vertexuses().len() as i64,
        brep.regions().len() as i64,
    ]);
    for (_, shell) in brep.shells().iter() {
        let (size, kind, head) = match &shell.members {
            ShellMembers::Empty => (0, 0, -1),
            ShellMembers::Faceuses(ids) => (
                ids.len(),
                if ids.is_empty() { 0 } else { 16022 },
                ids.first().map_or(-1, |id| i64::from(id.index())),
            ),
            ShellMembers::Edgeuse(id) => {
                let edge = brep
                    .edgeuse(*id)
                    .map_err(failure)?
                    .edge
                    .ok_or_else(|| failure("wire head has no edge"))?;
                (
                    brep.edge(edge).map_err(failure)?.edgeuses.len(),
                    16005,
                    i64::from(id.index()),
                )
            }
            ShellMembers::Vertexuse(id) => (1, 16006, i64::from(id.index())),
        };
        out.extend([
            shell.shell_type.code(),
            size as i64,
            shell.region.map_or(-1, |id| i64::from(id.index())),
            kind,
            head,
        ]);
    }
    for (id, faceuse) in brep.faceuses().iter() {
        let (next, last) = faceuse_sibling_links(brep, id).map_err(failure)?;
        out.extend([
            faceuse.shell.map_or(-1, |id| i64::from(id.index())),
            next.map_or(-1, |id| i64::from(id.index())),
            last.map_or(-1, |id| i64::from(id.index())),
        ]);
    }
    for (_, edgeuse) in brep.edgeuses().iter() {
        let (kind, id) = match edgeuse.owner {
            EdgeuseOwner::Shell(id) => (16013, id.map_or(-1, |id| i64::from(id.index()))),
            EdgeuseOwner::Loopuse(id) => (16004, id.map_or(-1, |id| i64::from(id.index()))),
            EdgeuseOwner::Unknown => (16999, -1),
        };
        out.extend([kind, id]);
    }
    for (_, vertexuse) in brep.vertexuses().iter() {
        let (kind, id) = match vertexuse.owner {
            VertexuseOwner::Shell(id) => (16013, id.map_or(-1, |id| i64::from(id.index()))),
            VertexuseOwner::Loopuse(id) => (16004, id.map_or(-1, |id| i64::from(id.index()))),
            VertexuseOwner::Edgeuse(id) => (16005, id.map_or(-1, |id| i64::from(id.index()))),
            VertexuseOwner::Unknown => (16999, -1),
        };
        out.extend([kind, id]);
    }
    for (_, region) in brep.regions().iter() {
        out.extend([
            region.shells.len() as i64,
            region.shells.first().map_or(-1, |id| i64::from(id.index())),
        ]);
        list(&mut out, region.shells.iter().copied().map(ShellId::index));
    }
    for shell in brep.shells().ids() {
        let counters = mark_counters(&kernel);
        let wires = wire_edges(&mut kernel, brep, shell).map_err(failure)?;
        out.push(0);
        list(&mut out, wires.into_iter().map(EdgeId::index));
        out.extend(mark_delta(&kernel, counters));
    }
    out.push(pre_status.len() as i64);
    out.extend(pre_status);
    Ok(Output::new().ints("records", out))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.shell"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let inputs: Inputs = case.parse()?;
        let (path, block) = fixture_input(inputs.brep)?;
        if let Some(filter) = inputs.region_filter {
            if filter.pattern > 2
                || inputs.region_order.is_some()
                || inputs.transfer.is_some()
                || inputs.type_probe.is_some()
                || inputs.degenerate.is_some()
                || inputs.ray.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure("invalid region filtering operation"));
            }
            return integer_records(
                ffi::region_filter(
                    &path,
                    &block,
                    filter.attribute_id,
                    filter.enabled,
                    filter.pattern,
                )
                .map_err(failure)?,
            );
        }
        if let Some(order) = inputs.region_order {
            let prefix = order.prefix_empty;
            if prefix > 8
                || inputs.transfer.is_some()
                || inputs.type_probe.is_some()
                || inputs.degenerate.is_some()
                || inputs.ray.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure("invalid region ordering operation"));
            }
            return integer_records(
                ffi::region_order(&path, &block, prefix, order.gather_faces).map_err(failure)?,
            );
        }
        if inputs.transfer.is_some() && (inputs.wires || inputs.void_state.is_some()) {
            return Err(failure("transfer cannot be combined with query mutations"));
        }
        if let Some(ray) = inputs.ray {
            if inputs.degenerate.is_some()
                || inputs.type_probe.is_some()
                || inputs.transfer.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure("ray cannot be combined with another operation"));
            }
            let values = match ffi::ray_fire(
                &path,
                &block,
                ray.start,
                ray.direction,
                ray.tolerance,
                ray.radius,
            ) {
                // The shim's guard: non-finite ray values with boundary faces.
                Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                    return Ok(Output::err("rejected_input"));
                }
                other => other.map_err(failure)?,
            };
            return ray_output(&values);
        }
        if let Some(degenerate) = inputs.degenerate {
            if inputs.type_probe.is_some()
                || inputs.transfer.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure(
                    "degeneracy cannot be combined with another operation",
                ));
            }
            let values = ffi::degeneracy(&path, &block, degeneracy_tolerance(&degenerate)?)
                .map_err(failure)?;
            return Ok(Output::new().ints(
                "records",
                values
                    .into_iter()
                    .map(|value| value as i64)
                    .collect::<Vec<_>>(),
            ));
        }
        if let Some(probe) = inputs.type_probe {
            if inputs.transfer.is_some() || inputs.wires || inputs.void_state.is_some() {
                return Err(failure(
                    "type probe cannot be combined with another operation",
                ));
            }
            let export = graph::eval(graph::Op::Export, &path, &block).map_err(failure)?;
            let counts = graph::counts(&export)
                .ok_or_else(|| failure("native type fixture build failed"))?;
            let values =
                ffi::type_members(&path, &block, type_probe_index(&probe, counts)?, probe.code)
                    .map_err(failure)?;
            return Ok(Output::new().ints("records", values.to_vec()));
        }
        let native = if let Some(transfer) = inputs.transfer {
            let export = graph::eval(graph::Op::Export, &path, &block).map_err(failure)?;
            let counts = graph::counts(&export)
                .ok_or_else(|| failure("native transfer fixture build failed"))?;
            let options = resolve_transfer(&transfer, counts)?;
            match ffi::transfer(
                &path,
                &block,
                options.extra_empty,
                options.source,
                options.recipient,
                options.candidates.as_deref(),
                options.split_faceuse,
                &options.pre,
            ) {
                // The shim's guard: the input would crash native code.
                Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                    return Ok(Output::err("rejected_input"));
                }
                other => other,
            }
        } else if inputs.wires {
            ffi::wire_edges(&path, &block)
        } else {
            ffi::query(&path, &block, inputs.void_state)
        }
        .map_err(failure)?;
        if native
            .iter()
            .any(|v| !v.is_finite() || v.fract() != 0.0 || v.abs() > (1_u64 << 53) as f64)
        {
            return Err(failure("native topology query returned an invalid integer"));
        }
        Ok(Output::new().ints(
            "records",
            native.into_iter().map(|v| v as i64).collect::<Vec<_>>(),
        ))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let inputs: Inputs = case.parse()?;
        let (path, block) = fixture_input(inputs.brep)?;
        let export = graph::eval(graph::Op::Export, &path, &block).map_err(failure)?;
        let mut brep = import(&export)?;
        if let Some(filter) = inputs.region_filter {
            if filter.pattern > 2
                || inputs.region_order.is_some()
                || inputs.transfer.is_some()
                || inputs.type_probe.is_some()
                || inputs.degenerate.is_some()
                || inputs.ray.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure("invalid region filtering operation"));
            }
            return rust_region_filter(&brep, &filter);
        }
        if let Some(order) = inputs.region_order {
            let prefix = order.prefix_empty;
            if prefix > 8
                || inputs.transfer.is_some()
                || inputs.type_probe.is_some()
                || inputs.degenerate.is_some()
                || inputs.ray.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure("invalid region ordering operation"));
            }
            return rust_region_order(&mut brep, prefix, order.gather_faces);
        }
        if inputs.transfer.is_some() && (inputs.wires || inputs.void_state.is_some()) {
            return Err(failure("transfer cannot be combined with query mutations"));
        }
        if let Some(ray) = inputs.ray {
            if inputs.degenerate.is_some()
                || inputs.type_probe.is_some()
                || inputs.transfer.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure("ray cannot be combined with another operation"));
            }
            return rust_ray(&brep, &ray);
        }
        if let Some(degenerate) = inputs.degenerate {
            if inputs.type_probe.is_some()
                || inputs.transfer.is_some()
                || inputs.wires
                || inputs.void_state.is_some()
            {
                return Err(failure(
                    "degeneracy cannot be combined with another operation",
                ));
            }
            let tolerance = degeneracy_tolerance(&degenerate)?;
            let mut kernel = solidcore::kernel::Kernel::default();
            let mut out = vec![brep.shells().len() as i64, brep.regions().len() as i64];
            for id in brep.shells().ids() {
                match solidcore::topology::shell::shell_is_degenerate(
                    &mut kernel,
                    &brep,
                    id,
                    tolerance,
                ) {
                    Ok(value) => out.push(flag(value)),
                    Err(error) => return Output::unported(&error).ok_or_else(|| failure(error)),
                }
            }
            for id in brep.regions().ids() {
                match solidcore::topology::shell::region_is_degenerate(
                    &mut kernel,
                    &brep,
                    id,
                    tolerance,
                ) {
                    Ok(value) => out.push(flag(value)),
                    Err(error) => return Output::unported(&error).ok_or_else(|| failure(error)),
                }
            }
            return Ok(Output::new().ints("records", out));
        }
        if let Some(probe) = inputs.type_probe {
            if inputs.transfer.is_some() || inputs.wires || inputs.void_state.is_some() {
                return Err(failure(
                    "type probe cannot be combined with another operation",
                ));
            }
            let counts = graph::counts(&export)
                .ok_or_else(|| failure("native type fixture build failed"))?;
            return rust_type_members(&mut brep, &probe, type_probe_index(&probe, counts)?);
        }
        if let Some(transfer) = inputs.transfer {
            let counts = graph::counts(&export)
                .ok_or_else(|| failure("native transfer fixture build failed"))?;
            return rust_transfer_records(&mut brep, &resolve_transfer(&transfer, counts)?);
        }
        if inputs.wires {
            let mut out = vec![brep.shells().len() as i64];
            let mut kernel = solidcore::kernel::Kernel::default();
            for shell in brep.shells().ids() {
                let counters = mark_counters(&kernel);
                match wire_edges(&mut kernel, &brep, shell) {
                    Ok(wires) => {
                        out.push(0);
                        list(
                            &mut out,
                            wires.into_iter().map(solidcore::topology::EdgeId::index),
                        );
                        out.extend(mark_delta(&kernel, counters));
                    }
                    Err(error) => return Output::unported(&error).ok_or_else(|| failure(error)),
                }
            }
            return Ok(Output::new().ints("records", out));
        }
        let mut out = vec![brep.shells().len() as i64, brep.regions().len() as i64];
        let mut kernel = solidcore::kernel::Kernel::default();
        for (id, shell) in brep.shells().iter() {
            out.extend([
                Shell::type_code(),
                flag(shell.is_vertex_shell()),
                flag(shell.is_wire_shell()),
                flag(shell.is_faceuse_shell()),
                flag(shell.is_unknown_shell()),
                flag(shell.is_closed(&brep, id).map_err(failure)?),
                shell_region(&brep, id)
                    .map_err(failure)?
                    .map_or(-1, |v| i64::from(v.index())),
                flag(shell_brep(&brep, id).map_err(failure)?.is_some()),
                shell
                    .first_faceuse()
                    .map_err(failure)?
                    .map_or(-1, |v| i64::from(v.index())),
                shell
                    .wire_edge(&brep)
                    .map_err(failure)?
                    .map_or(-1, |v| i64::from(v.index())),
                shell
                    .vertex(&brep)
                    .map_err(failure)?
                    .map_or(-1, |v| i64::from(v.index())),
            ]);
            out.extend(TYPES.into_iter().map(|ty| flag(Shell::is_kind_of(ty))));
            text(&mut out, Shell::type_string());
            text(&mut out, Shell::class_string());
            list(
                &mut out,
                shell
                    .get_faceuses()
                    .map_err(failure)?
                    .into_iter()
                    .map(FaceuseId::index),
            );
            let counters = mark_counters(&kernel);
            list(
                &mut out,
                shell
                    .mated_shells(&mut kernel, &brep)
                    .map_err(failure)?
                    .into_iter()
                    .map(ShellId::index),
            );
            out.extend(mark_delta(&kernel, counters));
        }
        let regions: Vec<_> = brep.regions().ids().collect();
        for id in regions {
            if let Some(value) = inputs.void_state {
                brep.region_mut(id).map_err(failure)?.set_is_void(value);
            }
            let region = brep.region(id).map_err(failure)?;
            out.extend([
                Region::type_code(),
                i64::from(region.is_void()),
                flag(region.is_infinite_region(&brep, id)),
                flag(region.get_brep(&brep).is_some()),
            ]);
            out.extend(TYPES.into_iter().map(|ty| flag(Region::is_kind_of(ty))));
            text(&mut out, Region::type_string());
            text(&mut out, Region::class_string());
            list(
                &mut out,
                region
                    .get_shells(None, |_, _| Ok(false))
                    .map_err(failure)?
                    .into_iter()
                    .map(ShellId::index),
            );
        }
        Ok(Output::new().ints("records", out))
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let (brep, kind) = generate_brep(rng)?;
        let scale = [1e-3, 1.0, 1e3].get(rng.below(3) as usize).copied()?;
        let mut case = serde_json::Map::new();
        match rng.below(12) {
            0 | 1 => {
                // Query: native void states, or the wire-edge listing.
                if rng.chance(0.5) {
                    case.insert("wires".into(), json!(true));
                } else {
                    case.insert("void_state".into(), json!(rng.below(3)));
                }
            }
            2..=6 => {
                // TransferChildrenTo: any shell pair (self included), any candidate
                // list (null entries, duplicates, empty), optional preceding transfers.
                let extra_empty = rng.below(4) as usize;
                let split = kind == BrepKind::File && extra_empty >= 1 && rng.chance(0.2);
                let candidates = match rng.below(5) {
                    0 | 1 => Value::Null,
                    2 => json!([]),
                    _ => Value::Array(
                        (0..=rng.below(5))
                            .map(|_| {
                                if rng.chance(0.25) {
                                    json!(-1)
                                } else {
                                    json!(rng.below(12))
                                }
                            })
                            .collect(),
                    ),
                };
                let pre: Vec<Value> = if rng.chance(0.35) {
                    (0..=rng.below(2))
                        .map(|_| json!([rng.below(8), rng.below(8)]))
                        .collect()
                } else {
                    Vec::new()
                };
                case.insert(
                    "transfer".into(),
                    json!({"extra_empty": extra_empty, "source": rng.below(8),
                        "recipient": rng.below(8), "candidates": candidates,
                        "split_faceuse": split, "pre": pre}),
                );
            }
            7 | 8 => {
                // Arbitrary SM_TYPE codes (including values beyond 2^53).
                let code = match rng.below(4) {
                    0 => rng.next_u64() as i64,
                    1 => 16000 + rng.below(1100) as i64,
                    2 => [0_i64, -1, 1, i64::MAX, i64::MIN]
                        .get(rng.below(5) as usize)
                        .copied()?,
                    _ => [16004_i64, 16005, 16006, 16013, 16022, 16999]
                        .get(rng.below(6) as usize)
                        .copied()?,
                };
                let shell = if rng.chance(0.4) {
                    Value::Null
                } else {
                    json!(rng.below(6))
                };
                case.insert("type_probe".into(), json!({"code": code, "shell": shell}));
            }
            9 => {
                let direction = if rng.chance(0.1) {
                    [0.0; 3]
                } else {
                    [
                        rng.uniform(-1.0, 1.0),
                        rng.uniform(-1.0, 1.0),
                        rng.uniform(-1.0, 1.0),
                    ]
                };
                let tolerance = [1e-9, 1e-7, 1e-3, 1.0]
                    .get(rng.below(4) as usize)
                    .copied()?;
                let radius = if rng.chance(0.4) {
                    Value::Null
                } else {
                    json!([1e-3, 1.0, 1e3, 1e6].get(rng.below(4) as usize).copied()?)
                };
                let mut ray = json!({"start": rng.point3(-2.0, 2.0).map(|v| v * scale),
                    "direction": direction, "tolerance": tolerance, "radius": radius});
                if rng.chance(0.2) {
                    // Non-finite ray inputs: C++ only crashes on them once boundary
                    // faces reach the solver, so empty-shape runs compare them.
                    let special = ["nan", "inf", "-inf"].get(rng.below(3) as usize).copied()?;
                    let slot = match rng.below(4) {
                        0 => ray
                            .get_mut("start")
                            .and_then(|v| v.get_mut(rng.below(3) as usize)),
                        1 => ray
                            .get_mut("direction")
                            .and_then(|v| v.get_mut(rng.below(3) as usize)),
                        2 => ray.get_mut("tolerance"),
                        _ => ray.get_mut("radius"),
                    };
                    if let Some(slot) = slot {
                        *slot = json!(special);
                    }
                }
                case.insert("ray".into(), ray);
            }
            10 => {
                let tolerance = match rng.below(4) {
                    0 => Value::Null,
                    1 => json!(["nan", "inf", "-inf"].get(rng.below(3) as usize).copied()?),
                    _ => json!([0.0, 1e-12, 1e-7, 1e-3, 1.0, 1e3, -1.0]
                        .get(rng.below(7) as usize)
                        .copied()?),
                };
                case.insert("degenerate".into(), json!({"tolerance": tolerance}));
            }
            _ => {
                if rng.chance(0.5) {
                    case.insert(
                        "region_order".into(),
                        json!({"prefix_empty": rng.below(9), "gather_faces": rng.chance(0.5)}),
                    );
                } else {
                    case.insert(
                        "region_filter".into(),
                        json!({"attribute_id": rng.below(1 << 20), "enabled": rng.chance(0.5),
                            "pattern": rng.below(3)}),
                    );
                }
            }
        }
        case.insert("brep".into(), brep);
        Some(Value::Object(case))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BrepKind {
    File,
    Other,
}

/// A random fixture: vertex shells, a plane face, the wire network, an upstream
/// `.smb`, or a face and wire networks in one B-rep (face-use and wire shells together).
fn generate_brep(rng: &mut Rng) -> Option<(Value, BrepKind)> {
    let scale = [1e-3, 1.0, 1e3].get(rng.below(3) as usize).copied()?;
    let plane = |rng: &mut Rng| {
        let normal = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), 1.0];
        json!({"surface": {"plane": {"o": rng.point3(-2.0, 2.0).map(|v| v * scale), "n": normal}},
            "dom": [-scale, -scale, scale, scale]})
    };
    Some(match rng.below(10) {
        0 | 1 => {
            let mut points: Vec<[f64; 3]> = (0..=rng.below(4))
                .map(|_| rng.point3(-2.0, 2.0).map(|v| v * scale))
                .collect();
            if let (true, Some(&first)) = (rng.chance(0.3), points.first()) {
                // A near-coincident pair at the tolerance limit.
                points.push(first.map(|v| v + 1e-7 * scale));
            }
            (
                json!({"kind": "vertices", "points": points}),
                BrepKind::Other,
            )
        }
        2 | 3 => {
            let mut face = plane(rng);
            if let Some(map) = face.as_object_mut() {
                map.insert("kind".into(), json!("face"));
            }
            (face, BrepKind::Other)
        }
        4 => (json!({"kind": "network"}), BrepKind::Other),
        5 => {
            let file = ["Mouse.smb", "Endcap.smb", "box_with_large_fillet.smb"]
                .get(rng.below(3) as usize)
                .copied()?;
            (json!({"kind": "file", "file": file}), BrepKind::File)
        }
        _ => {
            let face = rng.chance(0.7);
            let networks = if face { rng.below(3) } else { 1 + rng.below(3) };
            let mut brep = json!({"kind": "mixed", "networks": networks});
            if let (true, Some(map)) = (face, brep.as_object_mut()) {
                map.insert("face".into(), plane(rng));
            }
            (brep, BrepKind::Other)
        }
    })
}

fn text(output: &mut Vec<i64>, value: &str) {
    output.push(value.len() as i64);
    output.extend(value.bytes().map(i64::from));
}

fn integer_records(values: Vec<f64>) -> Result<Output, HarnessError> {
    if values
        .iter()
        .any(|v| !v.is_finite() || v.fract() != 0.0 || v.abs() > (1_u64 << 53) as f64)
    {
        return Err(failure("invalid native ordering integer"));
    }
    Ok(Output::new().ints(
        "records",
        values.into_iter().map(|v| v as i64).collect::<Vec<_>>(),
    ))
}

fn rust_region_order(
    brep: &mut solidcore::topology::Brep,
    prefix: usize,
    gather_faces: bool,
) -> Result<Output, HarnessError> {
    let regions: Vec<_> = brep.regions().ids().collect();
    if gather_faces {
        if let Some(&target) = regions.first() {
            for &source in regions.iter().skip(1) {
                let original = brep.region(source).map_err(failure)?.shells.clone();
                for shell in original {
                    if brep.shell(shell).map_err(failure)?.is_faceuse_shell() {
                        brep.region_mut(source)
                            .map_err(failure)?
                            .shells
                            .retain(|id| *id != shell);
                        brep.region_mut(target).map_err(failure)?.shells.push(shell);
                        brep.shell_mut(shell).map_err(failure)?.region = Some(target);
                    }
                }
            }
        }
    }
    let mut out = vec![regions.len() as i64];
    for region in regions {
        let original = brep.region(region).map_err(failure)?.shells.clone();
        let mut head = Vec::new();
        for _ in 0..prefix {
            let shell = brep
                .add_shell_record(Shell {
                    region: Some(region),
                    ..Shell::new()
                })
                .map_err(failure)?;
            head.push(shell);
        }
        head.extend(original);
        brep.region_mut(region).map_err(failure)?.shells = head;
        match solidcore::topology::shell::order_region_shells(
            &mut solidcore::kernel::Kernel::default(),
            brep,
            region,
        ) {
            Ok(()) => out.push(0),
            Err(error) => return Output::unported(&error).ok_or_else(|| failure(error)),
        }
        list(
            &mut out,
            brep.region(region)
                .map_err(failure)?
                .shells
                .iter()
                .map(|s| s.index()),
        );
    }
    Ok(Output::new().ints("records", out))
}

fn rust_region_filter(
    brep: &solidcore::topology::Brep,
    filter: &RegionFilter,
) -> Result<Output, HarnessError> {
    use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
    let mut attributes = AttributeStore::new();
    for (index, (id, _)) in brep.shells().iter().enumerate() {
        if filter.pattern == 2 || (filter.pattern == 1 && index.is_multiple_of(2)) {
            let tag = attributes
                .insert(Attribute::long(filter.attribute_id, 123, Behavior::Copy))
                .map_err(failure)?;
            attributes
                .add_attribute(ObjectKey(u64::from(id.index())), tag)
                .map_err(failure)?;
        }
    }
    let mut out = vec![brep.regions().len() as i64];
    for (_, region) in brep.regions().iter() {
        let shells = region
            .get_shells(
                filter.enabled.then_some(filter.attribute_id),
                |shell, id| {
                    Ok(attributes
                        .find_attribute(ObjectKey(u64::from(shell.index())), id)
                        .is_some())
                },
            )
            .map_err(failure)?;
        list(&mut out, shells.into_iter().map(ShellId::index));
    }
    Ok(Output::new().ints("records", out))
}
