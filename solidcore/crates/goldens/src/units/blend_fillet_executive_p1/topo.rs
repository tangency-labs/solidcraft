//! `RemoveTopoEdges` cases: a fixture B-rep (an upstream test file or a generated face,
//! wire or network, `topology_vertexuse::BrepIn`) and the edges to remove, as indices
//! into the walk of the B-rep the Rust kernel builds (the generator draws an edge of it).
//! The B-rep after the call goes through `topology_brep_p10::snapshot` (topology in walk
//! order, sampled geometry, domains, tolerances).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_executive::p1::remove_topo_edges;
use solidcore::kernel::Kernel;
use solidcore::topology::EdgeId;

use super::super::topology_vertexuse::{self as shared, BrepIn};
use super::super::{topology_brep_p10, topology_brep_p4 as p4};
use crate::{HarnessError, Output, Rng};

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

/// The fixture B-rep made by the Rust kernel in the reference shim's traversal order (an
/// `.smb` file by the Rust reader), its geometry owned as the build left it (no owner is
/// added).
fn fixture(k: &mut Kernel, brep: &BrepIn) -> Result<crate::own_builder::Built, HarnessError> {
    use crate::own_builder::{build_as, Conventions};
    let (path, _) = p4::block(brep)?;
    let conv = &Conventions::EDGEUSE_GRAPH;
    Ok(match brep {
        BrepIn::File { rebuild, .. } => {
            crate::own_reader::file_in_order(k, &path, *rebuild, false, false, conv)?
        }
        src => build_as(k, src, false, false, conv)?
            .ok_or_else(|| err("source not built by the Rust kernel"))?,
    })
}

pub(super) fn rust(i: &TopoIn) -> Result<Output, HarnessError> {
    use crate::own_builder::Built;
    let mut k = Kernel::default();
    let mut b = match fixture(&mut k, &i.brep)? {
        Built::Brep(b) => *b,
        // the C++ side reports a fixture that does not build as rejected
        Built::Failed(e) => return Ok(Output::unported(&e).unwrap_or_default()),
    };
    p4::sync_face_tolerances(&mut b)?;
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
                e if solidcore::error::upstream_status(&e).is_some() => {
                    f64::from(solidcore::error::upstream_status(&e).unwrap_or(-1))
                }
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
    // a fixture that does not build is redrawn
    let Ok(crate::own_builder::Built::Brep(b)) = fixture(&mut Kernel::default(), &brep) else {
        return None;
    };
    let n_edges = b.edges().len();
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
