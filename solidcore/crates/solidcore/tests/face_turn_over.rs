//! A face of a sheet turned over (`topology::face::p1::turn_over`, `SmFace::SwapUV`) and a sheet
//! turned over whole (`topology::brep::sheet::turn_over_sheet`): the surface's u and v swapped,
//! so the normal in the upward use's sense (which `query::continuity` reads) is negated at every
//! point, the upward use still the Same one and the body valid before and after; turning over
//! twice gives the uses, loops and normals back; a closed body, and a face between two shells,
//! are refused before anything changes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)] // test helpers

use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives::{create_box, create_plane};
use solidcore::query::api_queries::face_normal_at_edge;
use solidcore::topology::brep::sheet::{turn_over_sheet, turn_over_shell};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p1::turn_over;
use solidcore::topology::face::p2::{loops, upward_faceuse};
use solidcore::topology::{Brep, FaceId, Faceuse, Loop};
use solidcore::{Kernel, SolidError, Vec3};

fn valid(k: &mut Kernel, b: &mut Brep) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    assert!(c.is_valid(), "{:?}", c.failures().collect::<Vec<_>>());
}

/// The 3 by 2 rectangle sheet at z = 0.
fn sheet(k: &mut Kernel) -> Brep {
    create_plane(k, Vec3::ZERO, 3.0, 2.0).unwrap()
}

/// The upward use's orientation of face `f`: its side against its surface.
fn side(b: &Brep, f: FaceId) -> OrientType {
    let fu = upward_faceuse(b, f).unwrap().unwrap();
    b.faceuse(fu).unwrap().orientation
}

/// The unit normal of face `f` at the middle of each of its edges (in the edges' order), in the
/// sense of its upward use.
fn upward_normals(k: &Kernel, b: &mut Brep, f: FaceId) -> Vec<Vec3> {
    let fu = upward_faceuse(b, f).unwrap().unwrap();
    let mut edges = Vec::new();
    for l in loops(b, f, false, None).unwrap() {
        edges.extend(b.loop_(l).unwrap().edges.iter().map(|e| e.edge));
    }
    assert!(!edges.is_empty());
    // in edge order: turned over, the loops run the other way
    edges.sort_unstable();
    edges.dedup();
    edges
        .into_iter()
        .map(|e| face_normal_at_edge(k, b, e, f, fu).unwrap().normal.value)
        .collect()
}

/// What turning over changes and must give back: each face's upward use, each loop, each use.
fn topology(
    b: &Brep,
) -> (
    Vec<Option<solidcore::topology::FaceuseId>>,
    Vec<Loop>,
    Vec<Faceuse>,
) {
    (
        b.face_list()
            .iter()
            .map(|&f| b.face(f).unwrap().faceuse)
            .collect(),
        b.loops().iter().map(|(_, l)| l.clone()).collect(),
        b.faceuses().iter().map(|(_, u)| u.clone()).collect(),
    )
}

#[test]
fn a_sheet_turned_over_faces_the_other_way_and_stays_valid() {
    let mut k = Kernel::default();
    let mut b = sheet(&mut k);
    valid(&mut k, &mut b);
    let f = b.face_list()[0];
    assert_eq!(side(&b, f), OrientType::Same);
    let before = upward_normals(&k, &mut b, f);
    for n in &before {
        assert!((n.z.abs() - 1.0).abs() < 1e-12, "{n:?}");
    }

    turn_over_sheet(&k, &mut b).unwrap();
    valid(&mut k, &mut b);
    assert_eq!(
        side(&b, f),
        OrientType::Same,
        "the upward use is still the Same one"
    );
    let after = upward_normals(&k, &mut b, f);
    assert_eq!(after.len(), before.len());
    for (a, n) in after.iter().zip(&before) {
        let sum = *a + *n;
        assert!(
            sum.x.abs() + sum.y.abs() + sum.z.abs() < 1e-12,
            "the normal at the same point is negated: {a:?} against {n:?}"
        );
    }
}

#[test]
fn turning_a_sheet_over_twice_gives_it_back() {
    let mut k = Kernel::default();
    let mut b = sheet(&mut k);
    let before = topology(&b);
    let f = b.face_list()[0];
    let normals = upward_normals(&k, &mut b, f);

    turn_over(&k, &mut b, f).unwrap();
    b.refresh_cached_topology();
    assert_ne!(
        topology(&b),
        before,
        "turned over once, the face is not as it was"
    );
    turn_over(&k, &mut b, f).unwrap();
    b.refresh_cached_topology();
    assert_eq!(topology(&b), before);
    for (a, n) in upward_normals(&k, &mut b, f).iter().zip(&normals) {
        let d = *a - *n;
        assert!(
            d.x.abs() + d.y.abs() + d.z.abs() < 1e-12,
            "{a:?} against {n:?}"
        );
    }

    let shell = b
        .faceuse(upward_faceuse(&b, f).unwrap().unwrap())
        .unwrap()
        .shell
        .unwrap();
    turn_over_shell(&k, &mut b, shell).unwrap();
    turn_over_shell(&k, &mut b, shell).unwrap();
    assert_eq!(topology(&b), before);
    valid(&mut k, &mut b);
}

#[test]
fn a_closed_body_is_refused_before_anything_changes() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::ZERO, 1.0, 2.0, 3.0).unwrap();
    let before = topology(&b);

    assert!(matches!(
        turn_over_sheet(&k, &mut b),
        Err(SolidError::Unsupported { .. })
    ));
    let f = b.face_list()[0];
    let shell = b
        .faceuse(upward_faceuse(&b, f).unwrap().unwrap())
        .unwrap()
        .shell
        .unwrap();
    assert!(matches!(
        turn_over_shell(&k, &mut b, shell),
        Err(SolidError::Unsupported { .. })
    ));
    assert!(matches!(
        turn_over(&k, &mut b, f),
        Err(SolidError::InvalidInput { .. })
    ));
    assert_eq!(topology(&b), before, "nothing changed");
    valid(&mut k, &mut b);
}
