//! `SmBrep::RemoveTopologicalEdgesAndVertices` joining two edges at a topological vertex
//! sets the surviving curve's owner to the surviving edge (`SetOwner(pSurvivingEdge)`).
//! Run through the plain entry point (the kernel's identities), the joined curve must
//! still be owned under the edge identity the B-rep check compares
//! (`Brep::owner_identity`); before, it kept the kernel key and the check reported
//! `Test #61` (`pE->ValidateGeometry(FALSE)`) on it (found on solidcraft#4492's mitred
//! rounds, `offset::offset_faces`).
//!
//! The join and the split set their edges' intervals through `edge::set_interval`, which
//! clears the gaps cached over the old interval (#20939).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ObjectRef;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p2, p5, p9};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::Brep;
use solidcore::topology::{edge, vertexuse};
use solidcore::{EntityRef, Kernel};

/// The level-2 check's lines and the edges whose curve owner is not their identity.
fn faults(k: &mut Kernel, b: &mut Brep) -> (Vec<String>, Vec<String>) {
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    let owners = b
        .edge_list()
        .iter()
        .filter_map(|&e| {
            let owner = b.edge(e).unwrap().curve.as_ref().and_then(Curve::owner);
            let identity = b.owner_identity(EntityRef::Edge(e));
            (owner != Some(identity)).then(|| format!("{e:?}: {owner:?} vs {identity:?}"))
        })
        .collect();
    (lines, owners)
}

#[test]
fn a_joined_edge_curve_is_owned_by_its_edge() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let naming = b.naming();
    let e = b.edge_list()[0];
    b.set_editing_enabled(true);
    // split the edge at its middle under the B-rep's own identities
    let pieces = {
        let mut state = p2::ArenaNotifyState::new(&b);
        p9::make_vertex_split_edge(
            &mut k,
            &mut b,
            e,
            0.5,
            &mut state.env(None),
            &|edge, kind| ObjectRef {
                key: notify_ref(EntityRef::Edge(edge)).key,
                type_code: kind,
            },
        )
        .unwrap()
        .edges
        .unwrap()
    };
    assert_eq!(pieces.len(), 2);
    b.refresh_cached_topology();
    let edges = b.edge_list().len();
    assert_eq!(
        faults(&mut k, &mut b),
        (vec![], vec![]),
        "the split B-rep ({naming:?}) is valid"
    );
    // the split vertex is topological: the join removes it and merges the two pieces
    p5::remove_topological_edges_and_vertices(&mut k, &mut b, None, None).unwrap();
    b.refresh_cached_topology();
    assert_eq!(b.edge_list().len(), edges - 1);
    assert_eq!(
        faults(&mut k, &mut b),
        (vec![], vec![]),
        "the joined B-rep ({naming:?}) is valid"
    );
}

/// #20939: `edge::set_interval` clears every gap cached over the old interval. A box edge is
/// shortened by a tenth at each end; its edge uses' edge/face and edge/edge gaps, its
/// clockwise neighbours' edge/edge gaps (to its start) and its vertex uses' vertex/edge gaps
/// then read as their recalculation. Before, each kept the gap of the whole edge: 0 where the
/// shortened edge is a tenth of its length (1) from its neighbours and vertices.
#[test]
fn set_interval_clears_the_gaps_of_the_old_interval() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let e = b.edge_list()[0];
    let uses = b.edge(e).unwrap().edgeuses.clone();
    assert_eq!(uses.len(), 2);
    let gaps = |k: &Kernel, b: &mut Brep, force: bool| -> Vec<String> {
        let mut out = Vec::new();
        for &eu in &uses {
            let (cw, vu) = {
                let r = b.edgeuse(eu).unwrap();
                (r.cw.unwrap(), r.vertexuse.unwrap())
            };
            let face = eu1::max_edge_face_gap(k, b, eu, force).unwrap();
            let edge = eu1::edge_ccw_edge_gap(k, b, eu, force).unwrap();
            let neighbour = eu1::edge_ccw_edge_gap(k, b, cw, force).unwrap();
            let vertex = vertexuse::vertex_edge_gap(k, b, vu, force).unwrap();
            out.push(format!(
                "{eu:?}: edge/face {:?}, edge/edge {:?}, clockwise {cw:?} edge/edge {:?}, vertex/edge {:?}",
                face.map(|g| g.gap_3d),
                edge.map(|g| g.gap_3d),
                neighbour.map(|g| g.gap_3d),
                vertex.map(|g| g.gap_3d),
            ));
        }
        out
    };
    // every cache filled over the whole edge
    let whole = gaps(&k, &mut b, false);
    let ivl = edge::interval(&b, e).unwrap();
    let tenth = 0.1 * ivl.length();
    let shortened = Extent1d::from_min_max(ivl.min() + tenth, ivl.max() - tenth);
    edge::set_interval(&mut b, e, &shortened).unwrap();
    let cached = gaps(&k, &mut b, false);
    let recalculated = gaps(&k, &mut b, true);
    assert_eq!(cached, recalculated);
    // the edit moved the gaps (else the comparison shows nothing)
    assert_ne!(whole, recalculated);
}
