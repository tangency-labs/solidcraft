//! `RemoveTopoEdges` cases: a fixture B-rep (an upstream test file or a generated face,
//! wire or network, `topology_vertexuse::BrepIn`) and the edges to remove, as indices
//! into the walk of the built B-rep. Both sides' B-reps after the call go through
//! `topology_brep_p10::snapshot` (topology in walk order, sampled geometry, domains,
//! tolerances); the C++ one is re-imported from the shim's export first.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_executive::p1::remove_topo_edges;
use solidcore::kernel::Kernel;
use solidcore::topology::EdgeId;
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_executive_p1 as ffi;

use super::super::topology_vertexuse::{self as shared, BrepIn};
use super::super::{topology_brep_p10, topology_brep_p4 as p4};
use crate::{HarnessError, Output, Rng};

/// The shim's guard: two listed edges share a vertex.
const SHARED_VERTEX: f64 = -3.0;
/// The shim's guard: a face of a listed edge does not own its surface.
const UNOWNED_SURFACE: f64 = -4.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TopoIn {
    brep: BrepIn,
    edges: Vec<i64>,
    #[serde(default = "yes")]
    editing: bool,
}

const fn yes() -> bool {
    true
}

fn err(m: &str) -> HarnessError {
    HarnessError::new(format!("blend.fillet_executive_p1 topo: {m}"))
}

fn with_status(status: f64, editing: bool, snapshot: Output) -> Output {
    let mut o = snapshot;
    o = o.values("status", vec![status]);
    o.ints("editing", vec![i64::from(editing)])
}

pub(super) fn cpp(i: &TopoIn) -> Result<Output, HarnessError> {
    let (path, blk) = p4::block(&i.brep)?;
    let out = ffi::remove_topo_edges(&path, &blk, &i.edges, i.editing)
        .map_err(|e| err(&e.to_string()))?;
    let Some((&status, rest)) = out.split_first() else {
        return Err(err("empty shim output"));
    };
    if status == -1.0 {
        return Ok(Output::rejected("fixture B-rep did not build"));
    }
    if status == SHARED_VERTEX {
        return Ok(Output::rejected(
            "two topo edges share a vertex: merging at it can delete a later listed edge, \
             which upstream then reads freed (FFI guard)",
        ));
    }
    if status == UNOWNED_SURFACE {
        return Ok(Output::rejected(
            "a face of a topo edge does not own its surface: upstream compares surface \
             pointers and asks the deleted surface for its face (FFI guard)",
        ));
    }
    let (&editing, body) = rest.split_first().ok_or_else(|| err("no editing flag"))?;
    let mut exported = vec![0.0];
    exported.extend_from_slice(body);
    let (mut b, exact) = shared::import(&exported)?;
    if exact.iter().any(|v| !v) {
        return Ok(Output::rejected("the C++ result's surfaces do not import exactly"));
    }
    b.set_editing_enabled(editing != 0.0);
    let snapshot = topology_brep_p10::snapshot(&Kernel::default(), &b)?;
    Ok(with_status(status, editing != 0.0, snapshot))
}

pub(super) fn rust(i: &TopoIn) -> Result<Output, HarnessError> {
    use crate::own_builder::{build_as, Built, Conventions};
    let (path, _) = p4::block(&i.brep)?;
    // The fixture B-rep made by the Rust kernel in the shim's traversal order (an `.smb`
    // file by the Rust reader), geometry owned under the arena identities
    // `p4::own_surfaces` gives; no C++ state reaches the Rust side.
    let mut k = Kernel::default();
    let conv = &Conventions::EDGEUSE_GRAPH;
    let built = match &i.brep {
        BrepIn::File { rebuild, .. } => {
            crate::own_reader::file_in_order(&mut k, &path, *rebuild, false, false, conv)?
        }
        src => build_as(&mut k, src, false, false, conv)?
            .ok_or_else(|| err("source not built by the Rust kernel"))?,
    };
    let mut b = match built {
        Built::Brep(b) => *b,
        // the C++ side reports a fixture that does not build as rejected
        Built::Failed(e) => return Ok(Output::unported(&e).unwrap_or_default()),
    };
    p4::own_surfaces(&mut b)?;
    b.set_editing_enabled(i.editing);
    let edges: Vec<Option<EdgeId>> = i
        .edges
        .iter()
        .map(|&e| u32::try_from(e).ok().map(EdgeId::from_index))
        .collect();
    let status = match remove_topo_edges(&mut k, &mut b, &edges) {
        Ok(()) => 0.0,
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Ok(o);
            }
            match e {
                SolidError::Upstream { code, .. } => f64::from(code),
                other => return Err(err(&format!("rust: {other}"))),
            }
        }
    };
    let editing = b.editing_enabled();
    let snapshot = topology_brep_p10::snapshot(&Kernel::default(), &b)?;
    Ok(with_status(status, editing, snapshot))
}

/// One generated case, or `None` when the draw is unusable (a fixture that does not
/// build or has no edge).
fn try_generate(rng: &mut Rng) -> Option<TopoIn> {
    let brep = shared::gen_brep(rng);
    let (path, blk) = p4::block(&brep).ok()?;
    let f = solidcore_ffi::units::topology_topology_traverser::export(&path, &blk).ok()?;
    let n_edges = solidcore_ffi::units::topology_vertexuse::counts(&f.graph)?[7];
    if n_edges == 0 {
        return None;
    }
    // one edge per generated case; multi-edge sequences (whose earlier merge can
    // delete a later listed edge unless disjoint) are covered by fixtures
    let edges = vec![rng.below(n_edges as u64) as i64];
    Some(TopoIn {
        brep,
        edges,
        editing: !rng.chance(0.1),
    })
}

/// Generated `RemoveTopoEdges` cases per run.
pub(super) const MIN_GENERATED: usize = 64;

pub(super) fn generate(rng: &mut Rng) -> Option<Value> {
    let i = (0..64).find_map(|_| try_generate(rng))?;
    Some(serde_json::json!({ "remove_topo_edges": serde_json::to_value(i).ok()? }))
}
