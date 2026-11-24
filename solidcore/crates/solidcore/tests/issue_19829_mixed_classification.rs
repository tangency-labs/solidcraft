//! tangency-labs/solidcraft#19829 (TE-B-F3): the Mixed corner's classification
//! (`classify_mixed`) says "not mixed" as a value, `Ok(false)`, and a failure as an error
//! with a named cause; its caller, `FilletCorner::create`, passes the error on instead of
//! taking it for "no corner", and keeps every corner it chose before.
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
use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_corner::p3::{FilletCorner, MIXED_CORNER_OF_FOUR_EDGES};
use solidcore::blend::fillet_solver::p1::FilletSolver;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGenerator;
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
    /// The edge uses around the vertex that are not blended.
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
    let vertex = ends(ef).into_iter().find(|v| ends(ea).contains(v)).unwrap();
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

/// A blend's solver: the tolerances the classification reads, and a surface generator
/// that is neither a chamfer's nor a constant round's.
fn solver() -> FilletSolver {
    let mut s = FilletSolver::new(TOL, 1e-3, TAN_TOL_DEG.to_radians());
    s.fsg = Some(FilletSurfaceGenerator::Base);
    s
}

/// Every region of `c` made a void: the body is then its faces only, as a sheet is, and
/// no edge of it bounds material (`edge_bounds_material`).
fn as_sheet(c: &mut Corner) {
    for r in solidcore::topology::brep::p1::regions(&c.b, None).unwrap() {
        c.b.region_mut(r).unwrap().is_void = 1;
    }
}

/// `FilletCorner::create` at `c`'s vertex with a solver for each blend.
fn create(k: &Kernel, c: &mut Corner) -> solidcore::Result<Option<FilletCorner>> {
    let s = solver();
    let blends = c.solver_eus;
    FilletCorner::create_with_solver_lookup(
        k,
        &mut c.b,
        c.vertex,
        &c.solver_eus,
        &[],
        |_| false,
        |eu| {
            if blends.contains(&eu) {
                Ok(&s)
            } else {
                Err(SolidError::Unsupported {
                    operation: "test::solver_of",
                    reason: "not a blend",
                })
            }
        },
    )
}

/// The classification of `c`, each blend with a solver when `solved`.
fn classify(k: &Kernel, c: &mut Corner, solved: bool) -> solidcore::Result<bool> {
    let solver = solver();
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
    assert_eq!(
        solidcore::error::upstream_status(err),
        Some(SM_ERR_NULL_POINTER),
        "{err}"
    );
    assert!(
        format!("{err:?}").contains(&format!("{:?}", EntityRef::Edgeuse(eu))),
        "{err:?}"
    );
}

/// Both pegs are Mixed corners, of three and of four edges, and `FilletCorner::create`
/// makes each the blend corner's Mixed.
#[test]
fn both_pegs_are_mixed() {
    let mut k = kernel();
    for (mut c, edges) in [(tangent_peg(&mut k), 3), (drafted_peg(&mut k), 4)] {
        assert_eq!(c.all_eus.len(), edges);
        assert!(classify(&k, &mut c, true).unwrap());
        let corner = create(&k, &mut c).unwrap().unwrap();
        assert_eq!(corner.blend.unwrap().event, BlendCornerEvent::Mixed);
    }
}

/// "Not mixed" is `Ok(false)`, not an error: blends without solvers, and one blend only.
#[test]
fn not_mixed_is_a_value() {
    let mut k = kernel();
    for mut c in [tangent_peg(&mut k), drafted_peg(&mut k)] {
        assert!(!classify(&k, &mut c, false).unwrap());
        let solver = solver();
        let one = [c.solver_eus[0]];
        let lookup = |_| Some(&solver);
        let found = classify_mixed(
            &k,
            &mut c.b,
            c.vertex,
            &one,
            &c.all_eus,
            &lookup,
            TAN_TOL_DEG,
        );
        assert!(!found.unwrap());
    }
}

/// A sheet's edges bound no material, so a convex and a concave blend there are no Mixed
/// ring: `Ok(false)`. Before #19829 the ring's convexity test failed on them
/// (`EdgeBoundsNoMaterial`), and `FilletCorner::create` took the failure for "no corner";
/// it keeps that corner: none at three edges (Corner Case Not Implemented), the refusal of
/// a convex and a concave round at four.
#[test]
fn a_sheet_is_not_mixed_and_keeps_its_corner() {
    let mut k = kernel();
    let mut c = tangent_peg(&mut k);
    as_sheet(&mut c);
    assert!(!classify(&k, &mut c, true).unwrap());
    assert!(create(&k, &mut c).unwrap().is_none());

    let mut c = drafted_peg(&mut k);
    as_sheet(&mut c);
    assert!(!classify(&k, &mut c, true).unwrap());
    match create(&k, &mut c) {
        Err(SolidError::Unsupported { reason, .. }) => {
            assert_eq!(reason, MIXED_CORNER_OF_FOUR_EDGES);
        }
        other => panic!("{other:?}"),
    }
}

/// Four edges where an edge not blended has no curve are not mixed (that edge is no
/// tangent line), and `FilletCorner::create` refuses them as a convex and a concave round
/// that are not the Mixed corner, as before #19829.
#[test]
fn an_edge_without_a_curve_beside_the_seam_is_not_mixed() {
    let mut k = kernel();
    let mut c = drafted_peg(&mut k);
    for &other in &c.others.clone() {
        let e = eu1::edge(&c.b, other).unwrap().unwrap();
        let curve = c.b.edge_mut(e).unwrap().curve.take();
        let found = classify(&k, &mut c, true);
        let corner = create(&k, &mut c);
        c.b.edge_mut(e).unwrap().curve = curve;
        assert!(!found.unwrap());
        match corner {
            Err(SolidError::Unsupported { reason, .. }) => {
                assert_eq!(reason, MIXED_CORNER_OF_FOUR_EDGES);
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(classify(&k, &mut c, true).unwrap());
}

/// A blended edge use without an edge (a malformed body handed to the public
/// classification) is a failure, never "not mixed": `EdgeUseWithoutEdge` at that edge use.
/// An edge use not blended without an edge is never Mixed: not mixed, or the ring's
/// topology lookups refuse it as invalid input.
#[test]
fn a_blended_edge_use_without_an_edge_is_a_failure() {
    let mut k = kernel();
    for mut c in [tangent_peg(&mut k), drafted_peg(&mut k)] {
        for eu in c.all_eus.clone() {
            let e = c.b.edgeuse_mut(eu).unwrap().edge.take();
            let found = classify(&k, &mut c, true);
            c.b.edgeuse_mut(eu).unwrap().edge = e;
            if c.solver_eus.contains(&eu) {
                assert_edge_use_without_edge(&found.unwrap_err(), eu);
            } else {
                // never Mixed: not mixed, or the ring's topology lookups refuse it
                match found {
                    Ok(mixed) => assert!(!mixed, "{eu:?}"),
                    Err(err) => assert_eq!(err.kind(), ErrorKind::InvalidInput, "{err}"),
                }
            }
        }
        assert!(classify(&k, &mut c, true).unwrap());
    }
}
