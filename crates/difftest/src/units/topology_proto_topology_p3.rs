//! Runner for unit `topology.proto_topology_p3`: `FindOrCreateProtoVertex` /
//! `FindOrCreateProtoEdge`, the splits of proto edges and edge definitions,
//! `SplitIntersectingProtoEdges`, `GlueCoincidentBrepVertices` and
//! `ResolveOverlappingProtoEdges` of `SmProtoTopology.cpp`.
//!
//! It shares the proto-script engine of `topology.proto_topology_p1` (the shim, the FFI
//! validation and the Rust interpreter): the ops 120 to 126 of
//! `solidcore_ffi::units::topology_proto_topology_p1::proto_op`. Generated cases are
//! proto scripts rich in those ops.

use serde_json::Value;

use super::topology_proto_topology_p1 as engine;
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases at least.
pub const MIN_GENERATED: usize = 3000;

/// `topology.proto_topology_p3` runner.
pub struct Runner;

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.proto_topology_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        engine::generate_p3(rng)
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in engine::edge_cases_p3() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        engine::run_cpp_case(case)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        engine::run_rust_case(case)
    }
}
