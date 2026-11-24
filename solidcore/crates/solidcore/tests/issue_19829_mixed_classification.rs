//! tangency-labs/solidcraft#19829 (TE-B-F3): the Mixed corner's classification
//! (`classify_mixed`) says "not mixed" as a value, `Ok(false)`, and a failure as an error
//! with a named cause; its caller, `FilletCorner::create`, passes the error on instead of
//! taking it for "no corner".
//!
//! The bodies are the two Mixed corners the kernel builds: #2256's tangent peg
//! (`data/issue_2256_peg_step_reimported.smb`, three edges at V = (40, 0, 10)) and #2368's
//! drafted peg (`data/issue_2368_solidcraft_together_r2.smb`, four edges at V: the seam's
//! line runs on past V as an edge of the cone, #2953). At each, the classification is
//! called as `FilletCorner::create` calls it: the outward edge uses of the front top edge
//! (convex) and the concave arc, every outward edge use around V, a solver for each blend.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::blend_corner::mixed::classify_mixed;
use solidcore::blend::fillet_solver::p1::FilletSolver;
use solidcore::error::blend::BlendFailure;
use solidcore::error::{ErrorKind, SM_ERR_NULL_POINTER};
use solidcore::geom::attribute::AttributeStore;
use solidcore::kernel::{Kernel, UpstreamFixes};
use solidcore::math::Point3;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, loopuse, vertexuse, EdgeId, EdgeuseId, VertexId};
use solidcore::{Brep, EntityRef, SolidError};

const TANGENT_PEG: &[u8] = include_bytes!("data/issue_2256_peg_step_reimported.smb");
const DRAFTED_PEG: &[u8] = include_bytes!("data/issue_2368_solidcraft_together_r2.smb");

/// The blends' tolerance and the tangency tolerance (degrees) the classification is given.
const TOL: f64 = 1e-5;
const TAN_TOL_DEG: f64 = 0.5;

fn kernel() -> Kernel {
    Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn read(k: &mut Kernel, bytes: &[u8]) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &loopuse::notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The edge nearest `p`.
fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The edge use of `e` that starts at `v`.
fn outward(b: &Brep, e: EdgeId, v: VertexId) -> EdgeuseId {
    edge::edgeuses(b, e, None, None)
        .unwrap()
        .into_iter()
        .find(|&eu| {
            let vu = eu1::vertexuse(b, eu).unwrap().unwrap();
            vertexuse::vertex(b, vu).unwrap() == Some(v)
        })
        .unwrap()
}

/// A Mixed corner's vertex as `FilletCorner::create` hands it to the classification.
struct Corner {
    b: Brep,
    vertex: VertexId,
    solver_eus: [EdgeuseId; 2],
    all_eus: Vec<EdgeuseId>,
    /// The edge uses around the vertex that are not blended: the seam first.
    others: Vec<EdgeuseId>,
}

/// The corner where the front top edge (near `front`) meets the concave arc (near `arc`).
fn corner(k: &mut Kernel, bytes: &[u8], front: [f64; 3], arc: [f64; 3]) -> Corner {
    let b = read(k, bytes);
    let (ef, ea) = (pick(k, &b, front), pick(k, &b, arc));
    let ends = |e| {
        [
            edge::start_vertex(&b, e).unwrap().unwrap(),
            edge::end_vertex(&b, e).unwrap().unwrap(),
        ]
    };
    let vertex = ends(ef)
        .into_iter()
        .find(|v| ends(ea).contains(v))
        .unwrap();
    let solver_eus = [outward(&b, ef, vertex), outward(&b, ea, vertex)];
    // FilletCorner::create's walk: clockwise, then radial, one edge use per edge
    let mut all_eus = vec![solver_eus[0]];
    let mut eu = solver_eus[0];
    loop {
        let cw = eu1::cw_edgeuse(&b, eu).unwrap().unwrap();
        eu = eu1::radial(&b, cw).unwrap().unwrap();
        if eu == solver_eus[0] {
            break;
        }
        let e = eu1::edge(&b, eu).unwrap();
        if all_eus.iter().any(|&o| eu1::edge(&b, o).unwrap() == e) {
            continue;
        }
        let blended = solver_eus
            .into_iter()
            .find(|&s| eu1::edge(&b, s).unwrap() == e);
        all_eus.push(blended.unwrap_or(eu));
    }
    let others = all_eus
        .iter()
        .copied()
        .filter(|eu| !solver_eus.contains(eu))
        .collect();
    Corner {
        b,
        vertex,
        solver_eus,
        all_eus,
        others,
    }
}

fn tangent_peg(k: &mut Kernel) -> Corner {
    corner(k, TANGENT_PEG, [20.0, 0.0, 10.0], [32.0, 8.0, 10.0])
}

fn drafted_peg(k: &mut Kernel) -> Corner {
    corner(
        k,
        DRAFTED_PEG,
        [20.0, 0.874_89, 10.0],
        [32.874_89, 8.0, 10.0],
    )
}

/// The classification of `c`, each blend with a solver when `solved`.
fn classify(k: &Kernel, c: &mut Corner, solved: bool) -> solidcore::Result<bool> {
    let solver = FilletSolver::new(TOL, 1e-3, TAN_TOL_DEG.to_radians());
    let blends = c.solver_eus;
    let lookup = |eu: EdgeuseId| (solved && blends.contains(&eu)).then_some(&solver);
    classify_mixed(
        k,
        &mut c.b,
        c.vertex,
        &c.solver_eus,
        &c.all_eus,
        &lookup,
        TAN_TOL_DEG,
    )
}

/// `err` is the caller's edge use `eu` without an edge: invalid input, not a defect, with
/// the `SM_ERR_NULL_POINTER` status the site returned before.
fn assert_edge_use_without_edge(err: &SolidError, eu: EdgeuseId) {
    assert_eq!(
        err.blend_failure(),
        Some(&BlendFailure::EdgeUseWithoutEdge),
        "{err}"
    );
    assert_eq!(err.kind(), ErrorKind::InvalidInput, "{err}");
    assert!(!err.is_defect(), "{err}");
    assert_eq!(solidcore::error::upstream_status(err), Some(SM_ERR_NULL_POINTER), "{err}");
    assert!(format!("{err:?}").contains(&format!("{:?}", EntityRef::Edgeuse(eu))), "{err:?}");
}

/// Both pegs are Mixed corners, of three and of four edges.
#[test]
fn both_pegs_classify_as_mixed() {
    let mut k = kernel();
    let mut c = tangent_peg(&mut k);
    assert_eq!(c.all_eus.len(), 3);
    assert!(classify(&k, &mut c, true).unwrap());
    let mut c = drafted_peg(&mut k);
    assert_eq!(c.all_eus.len(), 4);
    assert!(classify(&k, &mut c, true).unwrap());
}

/// "Not mixed" is `Ok(false)`, not an error: blends without solvers, one blend only, and
/// tangency held to nothing.
#[test]
fn not_mixed_is_a_value() {
    let mut k = kernel();
    for mut c in [tangent_peg(&mut k), drafted_peg(&mut k)] {
        assert!(!classify(&k, &mut c, false).unwrap());
        let solver = FilletSolver::new(TOL, 1e-3, TAN_TOL_DEG.to_radians());
        let one = [c.solver_eus[0]];
        let lookup = |_| Some(&solver);
        let found =
            classify_mixed(&k, &mut c.b, c.vertex, &one, &c.all_eus, &lookup, TAN_TOL_DEG);
        assert!(!found.unwrap());
    }
}

/// Four edges whose edge beyond the seam is not a line (here it has no curve) are not
/// mixed: `Ok(false)`. Before #19829 its leaving tangent was read anyway, and the missing
/// curve was an error the caller took for "no corner", which at four edges is the same
/// refusal as "not mixed".
#[test]
fn an_edge_beyond_the_seam_that_is_not_a_line_is_not_mixed() {
    let mut k = kernel();
    let mut c = drafted_peg(&mut k);
    assert!(classify(&k, &mut c, true).unwrap());
    // the edge beyond: of the two not blended, the one whose removal keeps a seam
    for &beyond in &c.others.clone() {
        let e = eu1::edge(&c.b, beyond).unwrap().unwrap();
        let curve = c.b.edge_mut(e).unwrap().curve.take();
        let found = classify(&k, &mut c, true);
        c.b.edge_mut(e).unwrap().curve = curve;
        assert!(!found.unwrap());
    }
}

/// A ring's edge use without an edge (a malformed body) is a failure, never "not mixed":
/// `EdgeUseWithoutEdge` at that edge use, for the blends' edge uses (the ring's lookup)
/// and for the seam's and the edge beyond's (the classification's own lookups, the site
/// #19829 converts).
#[test]
fn a_ring_edge_use_without_an_edge_is_a_failure() {
    let mut k = kernel();
    for mut c in [tangent_peg(&mut k), drafted_peg(&mut k)] {
        let mut detached = 0;
        for eu in c.all_eus.clone() {
            let e = c.b.edgeuse_mut(eu).unwrap().edge.take();
            let found = classify(&k, &mut c, true);
            c.b.edgeuse_mut(eu).unwrap().edge = e;
            match found {
                Err(err) => {
                    assert_edge_use_without_edge(&err, eu);
                    detached += 1;
                }
                // a seam without an edge can leave the ring unresolved: not mixed
                Ok(mixed) => assert!(!mixed, "{eu:?}"),
            }
        }
        // both blends' edge uses at least
        assert!(detached >= 2, "{detached}");
        assert!(classify(&k, &mut c, true).unwrap());
    }
}
