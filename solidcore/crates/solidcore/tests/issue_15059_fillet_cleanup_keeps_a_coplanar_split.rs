//! tangency-labs/solidcraft#15059 (WR-459): the fillet clean-up's keep
//! (`Kernel::fixes.fillet_cleanup_keeps_target_faces`) still keeps something since WR-602
//! made the clean-up check a kept slit: an edge of the target between two different faces
//! on coincident surfaces.
//!
//! The body: a prism of side 10 whose front side is two coplanar faces (its profile's front
//! edge drawn as two collinear lines), so the edge between them has a face on each side, both
//! in the plane y = 0. `DoFilleting`'s `BooleanPostProcess(TRUE)` checks every edge of the
//! target; without the keep `IsTopological` takes that edge as removable and merges the two
//! faces. A round on the back top edge, far from it:
//! - with the keep, the edge stays: 8 faces (the prism's 7 and the round), 18 edges;
//! - without it, the edge goes and the front is one face: 7 faces, 15 edges.
//!
//! The volume is the closed form's either way, `S^3 - (1 - pi / 4) r^2 S`, within the
//! relative accuracy the volume is asked to (`brep_volume`'s 1e-6). The clean-up alone
//! (`boolean_post_process` over every edge, as `fork_cac11ea` runs it) does the same: with
//! every edge and face kept the edge stays, with nothing kept it goes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::merge::p1::{boolean_post_process, CleanupRelations};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

/// The prism's side.
const S: f64 = 10.0;
/// The round's radius.
const R: f64 = 1.0;
/// The relative accuracy the volume is asked to (`brep_volume`'s tolerance).
const VOLUME_TOL: f64 = 1e-6;

fn line(a: [f64; 2], b: [f64; 2]) -> Curve {
    Curve::from(
        create_line_segment(Point3::new(a[0], a[1], 0.0), Point3::new(b[0], b[1], 0.0)).unwrap(),
    )
}

/// The square of side S in z = 0, its front edge (y = 0) as two lines meeting at x = S / 2,
/// swept up S.
fn prism(k: &mut Kernel) -> Brep {
    let cs = [
        line([0.0, 0.0], [0.5 * S, 0.0]),
        line([0.5 * S, 0.0], [S, 0.0]),
        line([S, 0.0], [S, S]),
        line([S, S], [0.0, S]),
        line([0.0, S], [0.0, 0.0]),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), S, true).unwrap()
}

/// The points of edge `e`'s ends.
fn ends(b: &Brep, e: EdgeId) -> Vec<Point3> {
    edge::vertices(b, e, None)
        .unwrap()
        .into_iter()
        .flatten()
        .map(|v| vertex::point(b, v).unwrap())
        .collect()
}

/// The edge between the two front faces: its ends (S/2, 0, 0) and (S/2, 0, S).
fn split_edge(b: &Brep) -> Option<EdgeId> {
    p1::edges(b, None).unwrap().into_iter().find(|&e| {
        let p = ends(b, e);
        p.len() == 2
            && p.iter()
                .all(|p| (p.x - 0.5 * S).abs() < 1e-9 && p.y.abs() < 1e-9)
    })
}

/// The back top edge: its ends on y = S, z = S.
fn back_top(b: &Brep) -> EdgeId {
    p1::edges(b, None)
        .unwrap()
        .into_iter()
        .find(|&e| {
            let p = ends(b, e);
            p.len() == 2
                && p.iter()
                    .all(|p| (p.y - S).abs() < 1e-9 && (p.z - S).abs() < 1e-9)
        })
        .expect("the back top edge")
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, VOLUME_TOL, &attrs).unwrap()
}

/// `(faces, edges)`.
fn counts(b: &Brep) -> (usize, usize) {
    (
        p1::faces(b, None).unwrap().len(),
        p1::edges(b, None).unwrap().len(),
    )
}

/// The prism rounded r 1 on its back top edge with the keep as `keep` says.
fn rounded(keep: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_cleanup_keeps_target_faces = keep;
    let mut b = prism(&mut k);
    let e = back_top(&b);
    fillet_edges(&mut k, &mut b, &[e], R, 1, 1, 1.0).unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    (k, b)
}

#[test]
fn the_prism_has_a_two_face_edge_between_coplanar_faces() {
    let mut k = Kernel::default();
    let b = prism(&mut k);
    assert_eq!(counts(&b), (7, 15));
    let split = split_edge(&b).expect("the split edge");
    assert_eq!(edge::faces(&b, split, None).unwrap().len(), 2);
}

#[test]
fn the_keep_keeps_the_target_s_coplanar_split_after_a_round() {
    let want = S * S * S - (1.0 - core::f64::consts::FRAC_PI_4) * R * R * S;
    let (k, mut kept) = rounded(true);
    let split = split_edge(&kept).expect("the keep keeps the split edge");
    assert_eq!(edge::faces(&kept, split, None).unwrap().len(), 2);
    assert_eq!(counts(&kept), (8, 18));
    let v = volume(&k, &mut kept);
    assert!(
        (v - want).abs() <= VOLUME_TOL * want,
        "volume {v}, want {want}"
    );

    let (k, mut merged) = rounded(false);
    assert!(
        split_edge(&merged).is_none(),
        "without the keep the split goes"
    );
    assert_eq!(counts(&merged), (7, 15));
    let v = volume(&k, &mut merged);
    assert!(
        (v - want).abs() <= VOLUME_TOL * want,
        "volume {v}, want {want}"
    );
}

#[test]
fn the_clean_up_alone_keeps_the_split_only_when_told() {
    for keep in [false, true] {
        let mut k = Kernel::default();
        let mut b = prism(&mut k);
        let relations = if keep {
            CleanupRelations {
                kept_edges: p1::edges(&b, None).unwrap().into_iter().collect(),
                kept_faces: p1::faces(&b, None).unwrap().into_iter().collect(),
                ..CleanupRelations::default()
            }
        } else {
            CleanupRelations::default()
        };
        // `DoFilleting` edits the body it cleans up.
        b.set_editing_enabled(true);
        boolean_post_process(&mut k, &mut b, &relations, true).unwrap();
        assert!(is_manifold_solid(&b).unwrap());
        if keep {
            assert!(split_edge(&b).is_some());
            assert_eq!(counts(&b), (7, 15));
        } else {
            assert!(split_edge(&b).is_none());
            assert_eq!(counts(&b), (6, 12));
        }
    }
}
