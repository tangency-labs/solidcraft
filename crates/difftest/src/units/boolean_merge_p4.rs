//! Real native `ManifoldBoolean` versus the Rust orchestration entry point.
//! The real kernel adapter delegates the ported later stages;
//! genuine missing dependencies still defer at their exact call sites.
//!
//! Both sides' B-reps are compared with each loop use's start edge use chosen
//! canonically ([`canonical_loop_starts`]): upstream's `BooleanPostProcess` deletes
//! the intersection edges in `m_sIntersectionEdges` order, which `GetMappedBrepObjects`
//! takes from a pointer-keyed map (`plan/upstream-issues.md` ND-258), and each
//! `DeleteEdge` -> `SmLoopuse::CollectEdgeuses` resets the loop's start, so the native
//! start (and the walk numbering it seeds) depends on heap addresses.
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solidcore::boolean::merge::p1::MergeOptions;
use solidcore::boolean::merge::p4::{BooleanKernels, KernelManifoldIntersector, ManifoldState};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::context::MarkLock;
use solidcore::kernel::Kernel;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::boolean_merge_p4::{self as ffi, Operand};

/// Differential runner for the manifold Boolean unit.
pub struct Runner;

#[derive(Debug, Serialize, Deserialize)]
struct Inputs {
    primary: Operand,
    other: Operand,
    operation: i32,
    #[serde(default = "defaults")]
    flags: [bool; 5],
    #[serde(default = "yes")]
    manifold: bool,
    #[serde(default)]
    distinct_contexts: bool,
    #[serde(default)]
    caller_lock: bool,
}
fn defaults() -> [bool; 5] {
    [false, false, true, false, false]
}
fn yes() -> bool {
    true
}
fn error(e: &(impl ToString + ?Sized)) -> HarnessError {
    HarnessError::new(e.to_string())
}
fn request(i: &Inputs) -> ffi::Request {
    ffi::Request {
        operation: i.operation,
        flags: i.flags,
        manifold: i.manifold,
        distinct_contexts: i.distinct_contexts,
        caller_lock: i.caller_lock,
    }
}
fn import(input: &Operand) -> Result<Option<Brep>, HarnessError> {
    if input[0] == -1.0 {
        return Ok(None);
    }
    let raw = ffi::export(input).map_err(|e| error(&e))?;
    if raw == [-1.0] {
        return Err(error("native input constructor rejected"));
    }
    let (mut b, _) = super::topology_vertexuse::import(&raw)?;
    own_geometry(&mut b)?;
    // record the arena naming `own_geometry` gives the owners (and each owner)
    b.use_arena_naming().map_err(|x| error(&x.to_string()))?;
    Ok(Some(b))
}

/// Give the imported geometry the owners the library's arena adapter keys its B-rep
/// cache by (`arena_identity`): each edge's curve is owned by its edge, as natively,
/// and each surface the importer marked as owned by its face keeps that face. Without
/// this the cache would leave every curve and owned surface out of its trees.
fn own_geometry(b: &mut Brep) -> Result<(), HarnessError> {
    use solidcore::topology::brep_cache::{arena_identity, CacheObject};
    let edges: Vec<_> = b.edges().ids().collect();
    for e in edges {
        let edge = b.edge_mut(e).map_err(|x| error(&x.to_string()))?;
        if let Some(curve) = edge.curve.as_mut() {
            curve.set_owner(Some(arena_identity(CacheObject::Edge(e))), None);
        }
    }
    let faces: Vec<_> = b.faces().ids().collect();
    for f in faces {
        let face = b.face_mut(f).map_err(|x| error(&x.to_string()))?;
        if let Some(surface) = face.surface.as_mut() {
            if surface.owner().is_some() {
                surface.set_owner(Some(arena_identity(CacheObject::Face(f))));
            }
        }
    }
    Ok(())
}

/// Rotates the start of every edge loop use the fixture walk reaches to its canonical
/// edge use: the one whose own vertex point, then its CCW neighbour's vertex point, is
/// least (exact, `f64::total_cmp`, x before y before z). The loop and its cycle are
/// unchanged; only where the walk enters it. An exact tie between two edge uses of one loop has no
/// canonical answer and is a harness error rather than a guess (ND-258).
pub(super) fn canonical_loop_starts(b: &mut Brep) -> Result<(), HarnessError> {
    use solidcore::topology::{EdgeuseId, LoopuseId, LoopuseStart};
    let point = |b: &Brep, eu: EdgeuseId| -> Result<[f64; 3], HarnessError> {
        let vu = b
            .edgeuse(eu)
            .map_err(|x| error(&x.to_string()))?
            .vertexuse
            .ok_or_else(|| error("loop edge use without vertex use"))?;
        let v = b
            .vertexuse(vu)
            .map_err(|x| error(&x.to_string()))?
            .vertex
            .ok_or_else(|| error("vertex use without vertex"))?;
        let p = b.vertex(v).map_err(|x| error(&x.to_string()))?.point;
        Ok([p.x, p.y, p.z])
    };
    let cmp = |a: &[f64; 6], c: &[f64; 6]| {
        a.iter()
            .zip(c)
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| o.is_ne())
            .unwrap_or(core::cmp::Ordering::Equal)
    };
    // only the loop uses the fixture walk reaches: after a Boolean's deletes the arena
    // keeps dead loop use records whose edge uses are gone
    let walk_error = |_| error("fixture walk failed");
    let g = super::topology_brep_p3::walk(b).map_err(walk_error)?;
    let mut loopuses = Vec::with_capacity(g.len(4));
    for i in 0..g.len(4) {
        loopuses.push(LoopuseId::from_index(g.at(4, i).map_err(walk_error)?));
    }
    for lu in loopuses {
        let start = b.loopuse(lu).map_err(|x| error(&x.to_string()))?.start;
        let LoopuseStart::Edgeuse(Some(start)) = start else {
            continue;
        };
        // the CCW cycle from the start
        let mut cycle = vec![start];
        loop {
            let last = *cycle.last().ok_or_else(|| error("empty loop"))?;
            let next = b
                .edgeuse(last)
                .map_err(|x| error(&x.to_string()))?
                .ccw
                .ok_or_else(|| error("open loop"))?;
            if next == start {
                break;
            }
            if cycle.len() > b.edgeuses().len() {
                return Err(error("loop cycle does not close"));
            }
            cycle.push(next);
        }
        let mut keyed = Vec::with_capacity(cycle.len());
        for &eu in &cycle {
            let ccw = b
                .edgeuse(eu)
                .map_err(|x| error(&x.to_string()))?
                .ccw
                .ok_or_else(|| error("open loop"))?;
            let [a, c, d] = point(b, eu)?;
            let [e, f, g] = point(b, ccw)?;
            keyed.push(([a, c, d, e, f, g], eu));
        }
        keyed.sort_by(|x, y| cmp(&x.0, &y.0));
        if let [first, second, ..] = keyed.as_slice() {
            if cmp(&first.0, &second.0).is_eq() {
                return Err(error("unbreakable loop-start tie (ND-258)"));
            }
        }
        let best = keyed.first().map(|x| x.1).ok_or_else(|| error("empty loop"))?;
        b.loopuse_mut(lu).map_err(|x| error(&x.to_string()))?.start =
            LoopuseStart::Edgeuse(Some(best));
    }
    Ok(())
}

fn append_graph(k: &Kernel, output: &mut Output, prefix: &str, brep: &Brep) -> Result<(), HarnessError> {
    let graph = super::topology_brep_p10::snapshot(k, brep)?;
    for (key, values) in graph.ints {
        output.ints.insert(format!("{prefix}.{key}"), values);
    }
    for (key, values) in graph.values {
        output.values.insert(format!("{prefix}.{key}"), values);
    }
    Ok(())
}

struct Record<'a> {
    remaining: &'a [f64],
}
impl<'a> Record<'a> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let (v, rest) = self
            .remaining
            .split_first()
            .ok_or_else(|| error("short native record"))?;
        self.remaining = rest;
        Ok(*v)
    }
    fn take(&mut self, n: usize) -> Result<&'a [f64], HarnessError> {
        if n > self.remaining.len() {
            return Err(error("short native graph"));
        }
        let (v, rest) = self.remaining.split_at(n);
        self.remaining = rest;
        Ok(v)
    }
    fn done(&self) -> Result<(), HarnessError> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(error("trailing native state"))
        }
    }
}

fn native_snapshot(k: &Kernel, raw: &[f64]) -> Result<Output, HarnessError> {
    let mut rd = Record { remaining: raw };
    let mut header = Vec::new();
    for _ in 0..9 {
        header.push(rd.num().map_err(|e| error(&e))? as i64);
    }
    let component_count = header
        .get(7)
        .zip(header.get(8))
        .and_then(|(&a, &b)| a.checked_add(b))
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| error("invalid component count"))?;
    let mut output = Output::new().ints("status_and_state", header);
    let mut graph = |rd: &mut Record<'_>, prefix: &str| -> Result<(), HarnessError> {
        let n = rd.num().map_err(|e| error(&e))? as usize;
        let record = rd.take(n).map_err(|e| error(&e))?;
        let (editing, body) = record
            .split_first()
            .ok_or_else(|| error("empty native graph"))?;
        let mut exported = vec![0.0];
        exported.extend_from_slice(body);
        let (mut b, exact) = super::topology_vertexuse::import(&exported)?;
        if exact.iter().any(|v| !v) {
            return Err(error("inexact post-state surface import"));
        }
        b.set_editing_enabled(*editing != 0.0);
        // (the loop starts in canonical order, as the Rust side's: ND-258)
        canonical_loop_starts(&mut b)?;
        append_graph(k, &mut output, prefix, &b)
    };
    let mut presence = Vec::new();
    for prefix in ["primary", "other"] {
        let present = rd.num().map_err(|e| error(&e))? != 0.0;
        presence.push(i64::from(present));
        if present {
            graph(&mut rd, prefix)?;
        }
    }
    for i in 0..component_count {
        graph(&mut rd, &format!("component.{i}"))?;
    }
    rd.done().map_err(|e| error(&e))?;
    Ok(output.ints("survivors", presence))
}

fn rust_snapshot(
    status: i32,
    returned: bool,
    kernels: &BooleanKernels<'_>,
    primary: Option<&Brep>,
    other: Option<&Brep>,
    state: &ManifoldState,
) -> Result<Output, HarnessError> {
    let other_kernel = kernels.other.as_deref().unwrap_or(kernels.primary);
    let k: &Kernel = kernels.primary;
    let mut output = Output::new()
        .ints(
            "status_and_state",
            vec![
                i64::from(status),
                i64::from(returned),
                i64::from(kernels.primary.context.doing_boolean()),
                i64::from(other_kernel.context.doing_boolean()),
                i64::from(state.intersection_loop_closed),
                state.intersection_edges.len() as i64,
                state.intersection_vertices.len() as i64,
                state.primary_components.len() as i64,
                state.other_components.len() as i64,
            ],
        )
        .ints(
            "survivors",
            vec![i64::from(primary.is_some()), i64::from(other.is_some())],
        );
    for (prefix, b) in [("primary", primary), ("other", other)] {
        if let Some(b) = b {
            append_graph(k, &mut output, prefix, b)?;
        }
    }
    for (i, b) in state
        .primary_components
        .iter()
        .chain(&state.other_components)
        .enumerate()
    {
        append_graph(k, &mut output, &format!("component.{i}"), b)?;
    }
    Ok(output)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.merge_p4"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        let i: Inputs = case.parse()?;
        let raw = ffi::eval(&i.primary, &i.other, request(&i)).map_err(|e| error(&e))?;
        if raw == [-1.0] {
            return Ok(Output::rejected("native primitive constructor"));
        }
        // Status is data: Output::err would suppress all state comparison.
        native_snapshot(k, &raw)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let operation = BooleanOperation::from_code(i.operation)
            .ok_or_else(|| error("unknown Boolean operation"))?;
        let mut primary = import(&i.primary)?;
        let mut other = import(&i.other)?;
        let mut k = Kernel::default();
        let mut other_k = Kernel::default();
        let mut kernels = BooleanKernels {
            primary: &mut k,
            other: if i.distinct_contexts {
                Some(&mut other_k)
            } else {
                None
            },
        };
        let mut state = ManifoldState::default();
        let mut attributes = AttributeStore::new();
        let mut adapter = KernelManifoldIntersector::new(0).map_err(|e| error(&e.to_string()))?;
        adapter.set_intersection_tolerances(1e-9, 0.3);
        let mut lock = MarkLock::unbound();
        let [cookie_cutter, imprinting, boolean_post_process, imprint_and_classify_faces, keep_other_brep] =
            i.flags;
        let options = MergeOptions {
            cookie_cutter,
            imprinting,
            boolean_post_process,
            imprint_and_classify_faces,
            keep_other_brep,
        };
        adapter.merge.set_merge_options(Some(&options));
        let result = adapter.manifold_boolean(
            &mut kernels,
            &mut primary,
            &mut other,
            &mut state,
            &mut attributes,
            i.manifold,
            operation,
            if i.caller_lock { Some(&mut lock) } else { None },
        );
        // (the loop starts in canonical order, as the native side's: ND-258)
        if !matches!(&result, Err(e) if e.is_unported()) {
            for b in primary
                .iter_mut()
                .chain(other.iter_mut())
                .chain(state.primary_components.iter_mut())
                .chain(state.other_components.iter_mut())
            {
                canonical_loop_starts(b)?;
            }
        }
        match result {
            Err(e) if e.is_unported() => {
                Ok(Output::unported(&e).ok_or_else(|| error("missing deferral"))?)
            }
            Err(SolidError::Upstream { code, .. }) => rust_snapshot(
                code,
                false,
                &kernels,
                primary.as_ref(),
                other.as_ref(),
                &state,
            ),
            Err(e) => Err(error(&e)),
            Ok(returned) => rust_snapshot(
                0,
                returned,
                &kernels,
                primary.as_ref(),
                other.as_ref(),
                &state,
            ),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = match rng.below(5) {
            0 => 1e-9,
            1 => 1e-3,
            2 => 1.0,
            3 => 1e3,
            _ => 1e6,
        };
        let mut operand = || {
            let kind = rng.below(4) as f64;
            let [x, y, z] = rng.point3(-2.0 * scale, 2.0 * scale);
            [
                kind,
                x,
                y,
                z,
                rng.uniform(0.5, 2.0) * scale,
                rng.uniform(0.5, 2.0) * scale,
                rng.uniform(0.5, 2.0) * scale,
            ]
        };
        let a = operand();
        let b = operand();
        Some(json!({"primary":a,"other":b,"operation":rng.below(11),
            "flags":[rng.chance(0.2),rng.chance(0.2),rng.chance(0.5),rng.chance(0.2),rng.chance(0.5)],
            "manifold":rng.chance(0.8),"distinct_contexts":rng.chance(0.5),"caller_lock":rng.chance(0.5)}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::topology::VertexId;

    #[test]
    fn error_post_state_detects_mutations() {
        let mut k = Kernel::default();
        let mut other_k = Kernel::default();
        let mut primary = import(&[1.0, 2.0, 3.0, 4.0, 1.0, 1.0, 1.0]).unwrap();
        let other: Option<Brep> = None;
        let mut state = ManifoldState::default();
        let mut snapshot = |k: &mut Kernel, primary: Option<&Brep>, state: &ManifoldState| {
            rust_snapshot(
                1008,
                false,
                &BooleanKernels {
                    primary: k,
                    other: Some(&mut other_k),
                },
                primary,
                other.as_ref(),
                state,
            )
            .unwrap()
        };
        let baseline = snapshot(&mut k, primary.as_ref(), &state);
        k.context.set_doing_boolean(true);
        assert_ne!(baseline, snapshot(&mut k, primary.as_ref(), &state));
        k.context.set_doing_boolean(false);
        primary.as_mut().unwrap().set_editing_enabled(true);
        assert_ne!(baseline, snapshot(&mut k, primary.as_ref(), &state));
        primary.as_mut().unwrap().set_editing_enabled(false);
        state.intersection_vertices.push(VertexId::from_index(0));
        assert_ne!(baseline, snapshot(&mut k, primary.as_ref(), &state));
        state.intersection_vertices.clear();
        let b = primary.as_mut().unwrap();
        let id = b.vertices().ids().next().unwrap();
        b.vertex_mut(id).unwrap().point.x += 1.0;
        assert_ne!(baseline, snapshot(&mut k, primary.as_ref(), &state));
        primary = None;
        assert_ne!(baseline, snapshot(&mut k, primary.as_ref(), &state));
    }
}
