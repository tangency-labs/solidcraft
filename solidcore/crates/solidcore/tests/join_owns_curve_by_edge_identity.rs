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

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ObjectRef;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p2, p5, p9};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::Brep;
use solidcore::topology::{edge, face, vertexuse, EdgeuseId, VertexuseId};
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
    // (two faces, each with its two sides)
    assert_eq!(uses.len(), 4);
    // per edge use: its edge/face gap, its edge/edge gap, its clockwise neighbour's edge/edge
    // gap and its vertex use's vertex/edge gap
    let gaps = |k: &Kernel, b: &mut Brep, force: bool| -> Vec<[Option<f64>; 4]> {
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
            out.push([
                face.map(|g| g.gap_3d),
                edge.map(|g| g.gap_3d),
                neighbour.map(|g| g.gap_3d),
                vertex.map(|g| g.gap_3d),
            ]);
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
    // the closed form: the shortened edge still lies on both faces, and its ends are 1 from
    // the box corners, where the neighbours and the vertices are
    let expected = [0.0, 1.0, 1.0, 1.0];
    for (i, row) in recalculated.iter().enumerate() {
        for (got, want) in row.iter().zip(expected) {
            let got = got.unwrap();
            assert!(
                (got - want).abs() <= 1e-9,
                "edge use {i}: gaps {row:?}, want {expected:?}"
            );
        }
    }
}

/// Fills every gap cache of `b`: each edge use's edge/face and edge/edge gap and each vertex
/// use's vertex/edge and vertex/face gap.
fn fill_gap_caches(k: &Kernel, b: &mut Brep) {
    let edgeuses: Vec<EdgeuseId> = b.edgeuses().ids().collect();
    for eu in edgeuses {
        eu1::max_edge_face_gap(k, b, eu, false).unwrap();
        eu1::edge_ccw_edge_gap(k, b, eu, false).unwrap();
    }
    let vertexuses: Vec<VertexuseId> = b.vertexuses().ids().collect();
    for vu in vertexuses {
        vertexuse::vertex_edge_gap(k, b, vu, false).unwrap();
        vertexuse::vertex_face_gap(k, b, vu, false).unwrap();
    }
}

/// Each gap still cached on `edgeuses` and `vertexuses` (vertex/face gaps only with
/// `vertex_face`) against its recalculation, parameters and edge uses included: the
/// disagreements and the number of gaps compared. An edge/face gap's stored parameters are
/// where the largest gap was found, which is not unique on an edge that lies on its face, so
/// they must name two points that are the stored gap apart.
fn stale_gaps(
    k: &Kernel,
    b: &mut Brep,
    edgeuses: &[EdgeuseId],
    vertexuses: &[VertexuseId],
    vertex_face: bool,
) -> (Vec<String>, usize) {
    // what is still cached, read before any recalculation writes the caches again
    let held_edges: Vec<_> = edgeuses
        .iter()
        .map(|&eu| {
            let r = b.edgeuse(eu).unwrap();
            (eu, r.max_edge_face_gap, r.edge_ccw_edge_gap)
        })
        .collect();
    let held_vertices: Vec<_> = vertexuses
        .iter()
        .map(|&vu| {
            let r = b.vertexuse(vu).unwrap();
            (vu, r.vertex_edge_gap, r.vertex_face_gap)
        })
        .collect();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9;
    let (mut stale, mut compared) = (Vec::new(), 0);
    for (vu, edge_gap, face_gap) in held_vertices {
        if edge_gap.is_init() {
            let forced = vertexuse::vertex_edge_gap(k, b, vu, true).unwrap().unwrap();
            if !(close(edge_gap.gap_3d, forced.gap_3d) && close(edge_gap.edge_t, forced.edge_t)) {
                stale.push(format!(
                    "{vu:?} caches vertex/edge gap {} at edge_t {}, recalculated {} at {}",
                    edge_gap.gap_3d, edge_gap.edge_t, forced.gap_3d, forced.edge_t
                ));
            }
            compared += 1;
        }
        if vertex_face && face_gap.is_init() {
            let forced = vertexuse::vertex_face_gap(k, b, vu, true).unwrap().unwrap();
            if !(close(face_gap.gap_3d, forced.gap_3d)
                && close(face_gap.face_uv.x, forced.face_uv.x)
                && close(face_gap.face_uv.y, forced.face_uv.y))
            {
                stale.push(format!(
                    "{vu:?} caches vertex/face gap {} at {:?}, recalculated {} at {:?}",
                    face_gap.gap_3d, face_gap.face_uv, forced.gap_3d, forced.face_uv
                ));
            }
            compared += 1;
        }
    }
    for (eu, face_gap, edge_gap) in held_edges {
        if edge_gap.is_init() {
            let forced = eu1::edge_ccw_edge_gap(k, b, eu, true).unwrap().unwrap();
            if !(close(edge_gap.gap_3d, forced.gap_3d)
                && edge_gap.edgeuse1 == forced.edgeuse1
                && close(edge_gap.edge_t0, forced.edge_t0)
                && close(edge_gap.edge_t1, forced.edge_t1))
            {
                stale.push(format!(
                    "{eu:?} caches edge/edge gap {} to {:?} at {} and {}, recalculated {} to {:?} at {} and {}",
                    edge_gap.gap_3d,
                    edge_gap.edgeuse1,
                    edge_gap.edge_t0,
                    edge_gap.edge_t1,
                    forced.gap_3d,
                    forced.edgeuse1,
                    forced.edge_t0,
                    forced.edge_t1
                ));
            }
            compared += 1;
        }
        if face_gap.is_init() {
            let forced = eu1::max_edge_face_gap(k, b, eu, true).unwrap().unwrap();
            let e = eu1::edge(b, eu).unwrap().unwrap();
            let f = eu1::face(b, eu).unwrap().unwrap();
            let on_edge = edge::curve(b, e)
                .unwrap()
                .unwrap()
                .evaluate_point(k, face_gap.edge_t)
                .unwrap();
            let on_face = face::p2::surface(b, f)
                .unwrap()
                .unwrap()
                .evaluate_point(k, face_gap.face_uv)
                .unwrap();
            let apart = (on_edge - on_face).length();
            if !(close(face_gap.gap_3d, forced.gap_3d) && close(apart, face_gap.gap_3d)) {
                stale.push(format!(
                    "{eu:?} caches edge/face gap {} at edge_t {} and {:?}, which are {apart} apart; recalculated {}",
                    face_gap.gap_3d, face_gap.edge_t, face_gap.face_uv, forced.gap_3d
                ));
            }
            compared += 1;
        }
    }
    (stale, compared)
}

/// #20939: `edge::reverse_orientation` maps the edge's parameters `t` to `a + b - t` and
/// moves each vertex use to the edge's other end, so a gap cached before it keeps parameters
/// that name the wrong point. On a ball drilled off its centre, every gap cache is filled and
/// one edge is reversed; every gap still cached then agrees with its recalculation,
/// parameters included. Before, a vertex use of the reversed edge kept `edge_t` 0.7635 where
/// its recalculation gives 0.2365.
#[test]
fn reverse_orientation_leaves_no_gap_with_a_stale_parameter() {
    let mut k = Kernel::default();
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let drill = create_cylinder(&mut k, Point3::new(1.5, 0.7, -10.0), 2.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, ball, drill).unwrap();
    fill_gap_caches(&k, &mut b);
    let e = b.edge_list()[0];
    edge::reverse_orientation(&k, &mut b, e).unwrap();
    let edgeuses: Vec<EdgeuseId> = b.edgeuses().ids().collect();
    let vertexuses: Vec<VertexuseId> = b.vertexuses().ids().collect();
    let (stale, compared) = stale_gaps(&k, &mut b, &edgeuses, &vertexuses, true);
    assert_eq!(stale, Vec::<String>::new());
    // the caches the reversal did not touch are still there to compare
    assert!(compared > 0, "no gap left cached");
}

/// #20939: the join of two edges at a topological vertex grows the surviving edge over the
/// removed one, and the survivor's vertex uses at the removed vertex move to the far end. On
/// a box edge split at its middle, every gap cache is filled before the join; after it the
/// survivor's edge uses' gaps, its vertex uses' vertex/edge gaps and the edge/edge gaps of the
/// neighbours measured to it agree with their recalculation. Before, the survivor kept the
/// vertex/edge gap's parameter at the removed vertex and its edge/edge gap to the removed
/// edge. (Not compared, and the same before: a moved vertex use's vertex/face gap, and the
/// edge/edge gap of the removed edge's other neighbour, which still names the removed edge.)
#[test]
fn the_join_leaves_its_survivor_no_gap_with_a_stale_parameter() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let e = b.edge_list()[0];
    b.set_editing_enabled(true);
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
    b.refresh_cached_topology();
    fill_gap_caches(&k, &mut b);
    // the caches the join must not leave stale are there before it (else the test shows
    // nothing)
    for piece in pieces {
        for eu in b.edge(piece).unwrap().edgeuses.clone() {
            let r = b.edgeuse(eu).unwrap();
            assert!(r.max_edge_face_gap.is_init() && r.edge_ccw_edge_gap.is_init());
            let vu = r.vertexuse.unwrap();
            assert!(b.vertexuse(vu).unwrap().vertex_edge_gap.is_init());
        }
    }
    p5::remove_topological_edges_and_vertices(&mut k, &mut b, None, None).unwrap();
    b.refresh_cached_topology();
    let edges = b.edge_list().to_vec();
    let survivor = pieces.into_iter().find(|e| edges.contains(e)).unwrap();
    let own = b.edge(survivor).unwrap().edgeuses.clone();
    let mut uses = own.clone();
    let mut vertexuses = Vec::new();
    for &eu in &own {
        let r = b.edgeuse(eu).unwrap();
        vertexuses.extend(r.vertexuse);
        let cw = r.cw.unwrap();
        let measured_to_survivor = b.edgeuse(cw).unwrap().edge_ccw_edge_gap.edgeuse1 == Some(eu);
        if measured_to_survivor {
            uses.push(cw);
        }
    }
    let (stale, _) = stale_gaps(&k, &mut b, &uses, &vertexuses, false);
    assert_eq!(stale, Vec::<String>::new());
}
