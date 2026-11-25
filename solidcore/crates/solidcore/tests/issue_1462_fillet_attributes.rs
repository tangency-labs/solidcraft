//! Issue #1462 (fillets): caller attributes on a body's faces and edges survive a fillet
//! as SMLib's default handlers carry them. The B-rep owns its attributes
//! (`SmAObject::m_pAttributes`), so the fillet entry points keep their signatures: tag
//! the body, fillet, read the body. The executive's own `SM_AI_*` attributes do not
//! reach the caller.
//!
//! Each face and edge of the 10 x 10 x 10 box gets its own attribute id (one id per
//! origin), so a result object's origins are its attribute ids.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)] // test helpers

use solidcore::blend::fillet_executive::api::{
    chamfer_fillet, circular_fillet, create_face_from_surface, fillet_edges, fillet_edges_per_edge,
    surface_surface_fillet, variable_radius_fillet, FilletSurfaceArg,
};
use solidcore::blend::fillet_standard_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::mass::integrator::properties::{edge_length, face_area};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::Surface;
use solidcore::topology::{brep::p3::new_brep, edge, vertex, Brep, EdgeId, FaceId};
use solidcore::{EntityRef, Kernel};

/// The first application attribute id SolidCraft uses.
const BASE_ID: u64 = 10007;

/// Origin `(kind, index)` encoded in the attribute id: kind 0 face, 1 edge.
fn origin_id(kind: u64, index: u64) -> u64 {
    BASE_ID + kind * 500 + index
}

fn decode(id: u64) -> (u64, u64) {
    let raw = id - BASE_ID;
    (raw / 500, raw % 500)
}

/// A 10 x 10 x 10 box with one copy attribute per face and edge, and the edge ids in
/// tagging order.
fn tagged_box(k: &mut Kernel) -> (Brep, Vec<EdgeId>) {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("box");
    for (i, f) in b.face_list().to_vec().into_iter().enumerate() {
        let id = origin_id(0, i as u64);
        b.add_attribute(
            EntityRef::Face(f),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag face");
    }
    let edges = b.edge_list().to_vec();
    for (i, &e) in edges.iter().enumerate() {
        let id = origin_id(1, i as u64);
        b.add_attribute(
            EntityRef::Edge(e),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag edge");
    }
    (b, edges)
}

/// Every attribute id of `e` (system and caller).
fn ids(b: &Brep, e: EntityRef) -> Vec<u64> {
    let mut out: Vec<_> = b
        .entity_attributes(e)
        .into_iter()
        .map(|(_, a)| a.attribute_id())
        .collect();
    out.sort_unstable();
    out
}

/// The caller origins of `e`; panics on a system attribute (id below 10000).
fn origins(b: &Brep, e: EntityRef) -> Vec<(u64, u64)> {
    let ids = ids(b, e);
    assert!(
        ids.iter().all(|&id| id >= BASE_ID),
        "{e:?} carries a system attribute: {ids:?}"
    );
    ids.into_iter().map(decode).collect()
}

fn endpoints(b: &Brep, e: EdgeId) -> (Point3, Point3) {
    let (s, t) = edge::start_end_vertices(b, e).unwrap();
    (
        vertex::point(b, s.unwrap()).unwrap(),
        vertex::point(b, t.unwrap()).unwrap(),
    )
}

/// The box's top edge along X at y = 0.
fn top_front_edge(b: &Brep) -> EdgeId {
    let found: Vec<_> = b
        .edge_list()
        .iter()
        .copied()
        .filter(|&e| {
            let (p, q) = endpoints(b, e);
            p.y.abs() < 1e-9
                && q.y.abs() < 1e-9
                && (p.z - 10.0).abs() < 1e-9
                && (q.z - 10.0).abs() < 1e-9
        })
        .collect();
    assert_eq!(found.len(), 1);
    found[0]
}

/// Per face: (area rounded to 1e-6, origins), sorted.
fn faces(k: &Kernel, b: &mut Brep) -> Vec<(i64, Vec<(u64, u64)>)> {
    let mut out: Vec<_> = b
        .face_list()
        .to_vec()
        .into_iter()
        .map(|f| {
            let area = face_area(k, b, f, 1e-9).expect("area");
            ((area * 1e6).round() as i64, origins(b, EntityRef::Face(f)))
        })
        .collect();
    out.sort();
    out
}

/// Per edge: (length rounded to 1e-6, origins), sorted.
fn edges(k: &Kernel, b: &Brep) -> Vec<(i64, Vec<(u64, u64)>)> {
    let mut out: Vec<_> = b
        .edge_list()
        .iter()
        .map(|&e| {
            let length = edge_length(k, b, e, 1e-9).expect("length");
            (
                (length * 1e6).round() as i64,
                origins(b, EntityRef::Edge(e)),
            )
        })
        .collect();
    out.sort();
    out
}

fn um(x: f64) -> i64 {
    (x * 1e6).round() as i64
}

/// The six box faces and twelve box edges keep their origins through a round of the
/// top front edge, as SMLib's split/copy/merge handlers carry them; the executive's
/// `SM_AI_*` attributes reach no object.
fn check_one_edge_round(
    k: &Kernel,
    b: &mut Brep,
    before: &[(EdgeId, (Point3, Point3), u64)],
    filleted: u64,
    radius: Option<f64>,
) {
    let faces = faces(k, b);
    assert_eq!(faces.len(), 7, "{faces:?}");
    // the fillet face has no origin; every box face keeps its own, once
    let bare: Vec<_> = faces.iter().filter(|(_, o)| o.is_empty()).collect();
    assert_eq!(bare.len(), 1, "{faces:?}");
    let mut kept: Vec<_> = faces.iter().flat_map(|(_, o)| o.clone()).collect();
    kept.sort_unstable();
    assert_eq!(
        kept,
        (0..6).map(|i| (0, i)).collect::<Vec<_>>(),
        "{faces:?}"
    );
    assert!(faces.iter().all(|(_, o)| o.len() <= 1), "{faces:?}");
    if let Some(r) = radius {
        // the fillet face, the two trimmed adjacent faces, the two side faces
        assert_eq!(bare[0].0, um(PI * r / 2.0 * 10.0), "{faces:?}");
        let areas: Vec<_> = faces
            .iter()
            .filter(|(_, o)| !o.is_empty())
            .map(|f| f.0)
            .collect();
        let side = um(100.0 - (r * r - PI * r * r / 4.0));
        let trimmed = um(100.0 - 10.0 * r);
        assert_eq!(
            areas.iter().filter(|&&a| a == trimmed).count(),
            2,
            "{faces:?}"
        );
        assert_eq!(areas.iter().filter(|&&a| a == side).count(), 2, "{faces:?}");
        assert_eq!(
            areas.iter().filter(|&&a| a == um(100.0)).count(),
            2,
            "{faces:?}"
        );
    }
    // the filleted edge's origin is gone from every edge
    let edge_ids: Vec<_> = b.edge_list().to_vec();
    for &e in &edge_ids {
        assert!(
            !ids(b, EntityRef::Edge(e)).contains(&filleted),
            "{e:?} keeps the filleted edge's origin"
        );
    }
    // each other box edge keeps its origin: untouched (length 10) or the remaining
    // piece of an edge split at a fillet end (length 10 - r); new edges have none
    let summary = edges(k, b);
    assert_eq!(summary.len(), 15, "{summary:?}");
    let mut carried: Vec<_> = summary.iter().flat_map(|(_, o)| o.clone()).collect();
    carried.sort_unstable();
    let mut expected: Vec<_> = before
        .iter()
        .map(|(_, _, id)| decode(*id))
        .filter(|&o| o != decode(filleted))
        .collect();
    expected.sort_unstable();
    assert_eq!(carried, expected, "{summary:?}");
    assert!(summary.iter().all(|(_, o)| o.len() <= 1), "{summary:?}");
    assert_eq!(
        summary.iter().filter(|(_, o)| o.is_empty()).count(),
        4,
        "{summary:?}"
    );
    // the split edges: the piece left of each box edge at a fillet end, along its line
    let mut split = 0;
    for &e in &edge_ids {
        let Some(&id) = ids(b, EntityRef::Edge(e)).first() else {
            continue;
        };
        let (p, q) = endpoints(b, e);
        let (_, (a, c), _) = before.iter().find(|(_, _, o)| *o == id).unwrap();
        let on_line = |x: Point3| (x - *a).cross(*c - *a).length() < 1e-6;
        assert!(on_line(p) && on_line(q), "{e:?} left its box edge's line");
        if ((p - q).length() - 10.0).abs() > 1e-6 {
            split += 1;
        }
    }
    assert_eq!(split, 4);
}

fn edge_table(b: &Brep) -> Vec<(EdgeId, (Point3, Point3), u64)> {
    b.edge_list()
        .iter()
        .map(|&e| (e, endpoints(b, e), ids(b, EntityRef::Edge(e))[0]))
        .collect()
}

#[test]
fn issue_1462_a_rounded_box_edge_keeps_the_box_origins() {
    let mut k = Kernel::default();
    let (mut b, _) = tagged_box(&mut k);
    let before = edge_table(&b);
    let e = top_front_edge(&b);
    let filleted = ids(&b, EntityRef::Edge(e))[0];
    fillet_edges(&mut k, &mut b, &[e], 2.0, 1, 1, 1.0).expect("fillet");
    check_one_edge_round(&k, &mut b, &before, filleted, Some(2.0));
}

#[test]
fn issue_1462_per_edge_and_variable_radius_rounds_keep_the_box_origins() {
    let mut k = Kernel::default();
    let (mut b, _) = tagged_box(&mut k);
    let before = edge_table(&b);
    let e = top_front_edge(&b);
    let filleted = ids(&b, EntityRef::Edge(e))[0];
    fillet_edges_per_edge(
        &mut k,
        &mut b,
        &[e],
        &[2.0],
        &[FilletSurfaceGeneratorType::Circular],
        1,
        1.0,
    )
    .expect("per-edge fillet");
    check_one_edge_round(&k, &mut b, &before, filleted, Some(2.0));

    let mut k = Kernel::default();
    let (mut b, _) = tagged_box(&mut k);
    let before = edge_table(&b);
    let e = top_front_edge(&b);
    let filleted = ids(&b, EntityRef::Edge(e))[0];
    variable_radius_fillet(&mut k, &mut b, &[e], 1.0, 2.0, 1, 1).expect("variable fillet");
    check_one_edge_round(&k, &mut b, &before, filleted, None);
}

/// Every box edge is blended: the six box faces keep their origins, the blend and
/// corner faces have none, and no box edge survives (nor its origin).
fn check_all_edges_blended(k: &Kernel, b: &mut Brep) {
    let faces = faces(k, b);
    let mut kept: Vec<_> = faces.iter().flat_map(|(_, o)| o.clone()).collect();
    kept.sort_unstable();
    assert_eq!(
        kept,
        (0..6).map(|i| (0, i)).collect::<Vec<_>>(),
        "{faces:?}"
    );
    // at least one blend face per box edge
    assert!(faces.len() >= 6 + 12, "{faces:?}");
    let summary = edges(k, b);
    assert!(summary.iter().all(|(_, o)| o.is_empty()), "{summary:?}");
}

#[test]
fn issue_1462_chamfered_and_circular_rounded_boxes_keep_the_face_origins() {
    let mut k = Kernel::default();
    let (mut b, _) = tagged_box(&mut k);
    chamfer_fillet(&mut k, &mut b, 1.0).expect("chamfer");
    check_all_edges_blended(&k, &mut b);

    let mut k = Kernel::default();
    let (mut b, _) = tagged_box(&mut k);
    circular_fillet(&mut k, &mut b, 1.0).expect("circular fillet");
    check_all_edges_blended(&k, &mut b);
}

fn bilinear(corners: [[f64; 3]; 4]) -> Surface {
    Surface::from(
        BSplineSurface::create_bilinear_surface(
            Vec3::from(corners[0]),
            Vec3::from(corners[1]),
            Vec3::from(corners[2]),
            Vec3::from(corners[3]),
        )
        .unwrap(),
    )
}

const FIRST: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [10.0, 0.0, 0.0],
    [0.0, 10.0, 0.0],
    [10.0, 10.0, 0.0],
];
const SECOND: [[f64; 3]; 4] = [
    [5.0, 0.0, -5.0],
    [5.0, 0.0, 5.0],
    [5.0, 10.0, -5.0],
    [5.0, 10.0, 5.0],
];

fn tagged_sheet(k: &mut Kernel, surfaces: &[&Surface], first_id: u64) -> (Brep, Vec<FaceId>) {
    let mut body = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let mut faces = Vec::new();
    for (i, surface) in surfaces.iter().enumerate() {
        let f = create_face_from_surface(k, &mut body, surface).unwrap();
        let id = origin_id(0, first_id + i as u64);
        body.add_attribute(
            EntityRef::Face(f),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .unwrap();
        faces.push(f);
    }
    (body, faces)
}

/// Per face of `b`: its origins, sorted.
fn sheet_origins(b: &Brep) -> Vec<Vec<(u64, u64)>> {
    let mut out: Vec<_> = b
        .face_list()
        .iter()
        .map(|&f| origins(b, EntityRef::Face(f)))
        .collect();
    out.sort();
    out
}

#[test]
fn issue_1462_a_surface_fillet_splits_the_base_faces_with_their_origins() {
    // both base faces in one body: each splits at its rail, both pieces keep its origin
    let mut k = Kernel::default();
    let (first, second) = (bilinear(FIRST), bilinear(SECOND));
    let (mut body, faces) = tagged_sheet(&mut k, &[&first, &second], 0);
    let fillet = surface_surface_fillet(
        &mut k,
        FilletSurfaceArg::Face {
            body: &mut body,
            face: faces[0],
        },
        FilletSurfaceArg::FirstBodyFace(faces[1]),
        2.0,
        2.0,
        0.01,
        0,
        false,
        false,
    )
    .expect("fillet")
    .expect("a fillet B-rep");
    assert_eq!(
        sheet_origins(&body),
        vec![vec![(0, 0)], vec![(0, 0)], vec![(0, 1)], vec![(0, 1)]]
    );
    // the returned fillet B-rep carries no executive attribute
    assert!(fillet.attributed_entities().is_empty());

    // each base face in a body of its own (the second base B-rep)
    let mut k = Kernel::default();
    let (mut a, fa) = tagged_sheet(&mut k, &[&first], 0);
    let (mut c, fc) = tagged_sheet(&mut k, &[&second], 1);
    surface_surface_fillet(
        &mut k,
        FilletSurfaceArg::Face {
            body: &mut a,
            face: fa[0],
        },
        FilletSurfaceArg::Face {
            body: &mut c,
            face: fc[0],
        },
        2.0,
        2.0,
        0.01,
        0,
        false,
        false,
    )
    .expect("fillet")
    .expect("a fillet B-rep");
    assert_eq!(sheet_origins(&a), vec![vec![(0, 0)], vec![(0, 0)]]);
    assert_eq!(sheet_origins(&c), vec![vec![(0, 1)], vec![(0, 1)]]);
}
