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

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::Brep;

    fn mouse(k: &mut Kernel) -> Brep {
        let brep: BrepIn = serde_json::from_value(serde_json::json!(
            {"kind": "file", "file": "Mouse.smb", "rebuild": false}
        ))
        .unwrap();
        let crate::own_builder::Built::Brep(b) = fixture(k, &brep).unwrap() else {
            panic!("fixture")
        };
        let mut b = *b;
        p4::sync_face_tolerances(&mut b).unwrap();
        b.set_editing_enabled(true);
        b
    }

    /// The `remove_topo_edges_mouse_two_struts` case: Mouse.smb (4 faces, 6 edges, a
    /// manifold solid that passes Level 2) with its edges 1 and 2 removed. The drop of the
    /// joined edge onto its face (zone tolerance 3.63, UV tolerances about 1.8) came back in
    /// pieces whose ends are 0.54 apart in UV, which `longest_drop_run` took as one run and
    /// `create_by_joining` refused (`segments not end to end`), so the removal failed. It
    /// now keeps the longer piece and the removal succeeds with the topology main gives (2
    /// faces, 1 edge, a manifold solid).
    ///
    /// The result is not a valid body on main either: the edges are not struts (their faces
    /// meet at an angle), so the removal changes the shape (volume 141.58 to 0.70) and the
    /// check reports the same three failures on main (a UV gap in the loop, and edge and
    /// vertex tolerances of 3.63). The test holds what this change is about: the removal
    /// builds, with main's topology, and the check finds no failure of another kind.
    #[test]
    fn mouse_two_struts_removal_builds_with_mains_topology() {
        let mut k = Kernel::default();
        let mut input = mouse(&mut k);
        assert!(check_brep(&mut k, &mut input, CheckLevel::Level2)
            .unwrap()
            .lines()
            .is_empty());
        let mut k = Kernel::default();
        let mut b = mouse(&mut k);
        assert_eq!((b.faces().len(), b.edges().len()), (4, 6));
        remove_topo_edges(
            &mut k,
            &mut b,
            &[Some(EdgeId::from_index(1)), Some(EdgeId::from_index(2))],
        )
        .unwrap();
        assert_eq!((b.faces().len(), b.edges().len()), (2, 1));
        assert!(solidcore::topology::brep::p2::is_manifold_solid(&b).unwrap());
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        // the failure kinds main reports for this input, each once (one loop, edge and
        // vertex), and no other
        let known = [
            ("SmTrimmingTools::Test #1 ", 1),
            ("SmBrep::Test #59 ", 1),
            ("SmBrep::Test #82 ", 1),
            ("AssertValid skipped: ValidatePointers failed", 1),
        ];
        for line in &lines {
            assert!(
                known.iter().any(|(kind, _)| line.starts_with(kind)),
                "a failure main does not have: {line}"
            );
        }
        let counts: Vec<usize> = known
            .iter()
            .map(|(kind, _)| lines.iter().filter(|l| l.starts_with(kind)).count())
            .collect();
        let expected: Vec<usize> = known.iter().map(|&(_, n)| n).collect();
        assert_eq!(counts, expected, "{lines:#?}");
    }

    /// WR-441 (`crate::bstuv_trim_rows`): `remove_topo_edges_mouse_two_struts`, whose joined
    /// edge's trim keeps its short end spans. Every UV trim of the result is certified on
    /// its edge within the edge's tolerance (`trims_on_edges::certify_edgeuse`, a
    /// rounded-interval bound both ways, refined to an eighth of the tolerance). Upstream's
    /// cleanup cut the joined edge's trim down to 7 spans: `N_CrvRemoveDegenSegs` measured
    /// the UV span lengths against the 3D zone tolerance. That trim had points 7.8 off its
    /// edge, past the 3.67 tolerance (measured when the PR retiring the flag was written).
    #[test]
    fn bstuv_mouse_two_struts_trims_lie_on_their_edges() {
        let mut k = Kernel::default();
        let mut b = mouse(&mut k);
        remove_topo_edges(
            &mut k,
            &mut b,
            &[Some(EdgeId::from_index(1)), Some(EdgeId::from_index(2))],
        )
        .unwrap();
        let trims: Vec<_> = b
            .edgeuses()
            .iter()
            .filter(|(_, r)| r.uv_trim_curve.is_some())
            .map(|(x, _)| x)
            .collect();
        let joined = solidcore::topology::EdgeuseId::from_index(23);
        assert!(trims.contains(&joined), "the joined edge's edge use has a trim");
        for x in trims {
            let e = b.edgeuse(x).unwrap().edge.unwrap();
            let tol = b.edge(e).unwrap().tolerance;
            let cert =
                crate::trims_on_edges::certify_edgeuse(&Kernel::default(), &b, x, 0.125 * tol)
                    .unwrap();
            let trim = b.edgeuse(x).unwrap().uv_trim_curve.as_ref().unwrap();
            let spans = match trim.kind() {
                solidcore::curve::curve::CurveKind::BSpline(bs) => {
                    let mut d = bs.gw_nurb().unwrap().knots().to_vec();
                    d.dedup();
                    d.len() - 1
                }
                _ => 0,
            };
            eprintln!("BSTUV mouse two struts: {spans} spans, {cert:?}");
            if x == joined {
                assert_eq!(spans, 77, "the joined edge's trim keeps its spans");
            }
            assert!(
                cert.holds(),
                "edge use {x:?}'s trim is {:e} off edge {e:?} (tolerance {tol:e})",
                cert.bound
            );
        }
    }
}
