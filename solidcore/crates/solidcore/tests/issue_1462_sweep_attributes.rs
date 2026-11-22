//! Issue #1462 (P6): caller attributes and sweep tags through the sweeps. A B-rep owns
//! its attributes (`SmAObject::m_pAttributes`), so a sweep of a B-rep's own faces
//! (`SmApiNonManifoldSweep` / `SmApiNonManifoldRotationalSweep`, in place) carries them
//! as SMLib's default handlers do, and a tagging `SmPrimitiveCreation` sweep leaves its
//! `SM_AI_TAG` attributes on the result, as upstream's do.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::geom::attribute::{ids, tag_ids, Attribute, AttributeValue, Behavior, TagData};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::primitive::primitive_creation::model::PrimitiveModel;
use solidcore::primitive::primitive_creation::p1::PrimitiveCreation;
use solidcore::primitive::primitive_creation::p3::RepeatedSweepOptions;
use solidcore::sweep::api_brep::{
    non_manifold_rotational_sweep, non_manifold_sweep, SweepSelection,
};
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

/// The first application attribute id SolidCraft uses.
const BASE_ID: u64 = 10007;

/// Tags every face and edge (list order) of `b` with its own copy-behaviour long
/// attribute: face `i` id `BASE_ID + i`, edge `i` id `BASE_ID + 500 + i`.
fn tag_faces(b: &mut Brep) {
    let faces = b.face_list().to_vec();
    for (i, f) in faces.into_iter().enumerate() {
        let id = BASE_ID + i as u64;
        b.add_attribute(
            EntityRef::Face(f),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag face");
    }
    let edges = b.edge_list().to_vec();
    for (i, e) in edges.into_iter().enumerate() {
        let id = BASE_ID + 500 + i as u64;
        b.add_attribute(
            EntityRef::Edge(e),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag edge");
    }
}

/// The application ids (less `BASE_ID`) on each live object of `entities`, sorted
/// (one list per object, the lists sorted).
fn origins(b: &Brep, entities: impl Iterator<Item = EntityRef>) -> Vec<Vec<u64>> {
    let mut out: Vec<Vec<u64>> = entities
        .map(|e| {
            let mut ids: Vec<u64> = b
                .entity_attributes(e)
                .into_iter()
                .map(|(_, a)| a.attribute_id())
                .filter(|&id| id >= BASE_ID)
                .map(|id| id - BASE_ID)
                .collect();
            ids.sort_unstable();
            ids
        })
        .collect();
    out.sort();
    out
}

/// The face and edge origins of `b` (`GetFaces` / `GetEdges` objects).
fn face_origins(b: &Brep) -> (Vec<Vec<u64>>, Vec<Vec<u64>>) {
    let faces = solidcore::topology::brep::p1::faces(b, None).expect("faces");
    let edges = solidcore::topology::brep::p1::edges(b, None).expect("edges");
    (
        origins(b, faces.into_iter().map(EntityRef::Face)),
        origins(b, edges.into_iter().map(EntityRef::Edge)),
    )
}

fn lists(l: &[&[u64]]) -> Vec<Vec<u64>> {
    l.iter().map(|x| x.to_vec()).collect()
}

// The expected origins (ids less `BASE_ID`; edges from 500) are those the C++
// reference gives (`fixtures/boolean.api_brep/with_attributes_nm_sweeps.json`,
// `attributed_sweep_face0` and `attributed_rotational_sweep_face0`).

/// Box faces keep their origins; the swept block's cap and four sides have none.
const LINEAR_FACES: &[&[u64]] = &[&[], &[], &[], &[], &[], &[0], &[1], &[2], &[3], &[4], &[5]];
/// The four vertical box edges the stitch splits where the block meets them keep their
/// origin on both pieces (505, 506, 508, 510); the block's own edges have none.
const LINEAR_EDGES: &[&[u64]] = &[
    &[],
    &[],
    &[],
    &[],
    &[500],
    &[501],
    &[502],
    &[503],
    &[504],
    &[505],
    &[505],
    &[506],
    &[506],
    &[507],
    &[508],
    &[508],
    &[509],
    &[510],
    &[510],
    &[511],
];
const REVOLVE_FACES: &[&[u64]] = &[&[], &[], &[], &[], &[], &[0], &[1], &[2], &[3], &[4], &[5]];
const REVOLVE_EDGES: &[&[u64]] = &[
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[500],
    &[501],
    &[502],
    &[503],
    &[504],
    &[505],
    &[506],
    &[507],
    &[508],
    &[509],
    &[510],
    &[511],
];

fn tagged_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("box");
    tag_faces(&mut b);
    b
}

/// A linear sweep of one tagged face of a box (in place): the box's faces keep their
/// origins, and the faces the sweep builds from them carry those origins too.
#[test]
fn linear_sweep_of_a_tagged_face_carries_its_origins() {
    let mut k = Kernel::default();
    let mut b = tagged_box(&mut k);
    let face = b.face_list()[0];
    let selected = [Some(face)];
    let selection = SweepSelection {
        faces: Some(&selected),
        ..SweepSelection::default()
    };
    non_manifold_sweep(
        &mut k,
        &mut b,
        None,
        Vec3::new(0.0, 0.0, 1.0),
        5.0,
        true,
        false,
        selection,
    )
    .expect("sweep");
    let (faces, edges) = face_origins(&b);
    assert_eq!(faces, lists(LINEAR_FACES));
    assert_eq!(edges, lists(LINEAR_EDGES));
}

/// A revolve of one tagged face of a box (in place).
#[test]
fn revolve_of_a_tagged_face_carries_its_origins() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 5.0, 5.0).expect("box");
    tag_faces(&mut b);
    let face = b.face_list()[0];
    let selected = [Some(face)];
    let selection = SweepSelection {
        faces: Some(&selected),
        ..SweepSelection::default()
    };
    non_manifold_rotational_sweep(
        &mut k,
        &mut b,
        None,
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        90.0,
        true,
        false,
        selection,
    )
    .expect("revolve");
    let (faces, edges) = face_origins(&b);
    assert_eq!(faces, lists(REVOLVE_FACES));
    assert_eq!(edges, lists(REVOLVE_EDGES));
}

/// A tagging repeated linear sweep (`SmPrimitiveCreation(region, lTagID > 0)`) of a
/// square: each profile edge's tag is `(tag id, ordinal i + 1)`
/// (`SmTopologySweep::TagProfile`), and the side face and far edge swept from it carry
/// a copy whose ordinal is one `SM_TI_SWEEP_STEP_DELTA` higher, per step
/// (`SmTopologySweep.cpp:2384-2396`). The tags stay on the result.
#[test]
fn tag_ordinals_step_by_sweep_step_delta() {
    use solidcore::curve::curve::Curve;
    use solidcore::primitive::api_curves::create_line_segment;
    const TAG_ID: i64 = 17;
    let mut k = Kernel::default();
    let brep = solidcore::topology::brep::p3::new_brep(
        &mut k,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    )
    .expect("brep");
    let region = brep.infinite_region();
    // a closed 2 x 2 square, so the profile is a face and the sweep builds caps
    let corners = [
        Point3::new(-1.0, -1.0, 0.0),
        Point3::new(1.0, -1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(-1.0, 1.0, 0.0),
        Point3::new(-1.0, -1.0, 0.0),
    ];
    let mut curves: Vec<Option<Curve>> = corners
        .windows(2)
        .map(|w| Some(Curve::from(create_line_segment(w[0], w[1]).expect("line"))))
        .collect();
    let mut model = PrimitiveModel::new(&mut k, brep);
    let mut pc = PrimitiveCreation::new(region, TAG_ID, true);
    pc.create_repeated_linear_sweep(
        &mut model,
        PrimitiveModel::MAIN,
        &mut curves,
        Vec3::new(0.0, 0.0, 1.0),
        2.0,
        RepeatedSweepOptions {
            repetitions: 2,
            cap_ends: true,
            ..RepeatedSweepOptions::default()
        },
    )
    .expect("sweep");
    let b = model.into_brep().expect("result");
    let tag_of = |e: EntityRef| -> Option<TagData> {
        b.find_attribute(e, ids::TAG).and_then(|a| match a.value() {
            AttributeValue::Tag(t) => Some(*t),
            _ => None,
        })
    };
    let d = tag_ids::SWEEP_STEP_DELTA;
    let ordinals = |tags: Vec<Option<TagData>>| -> Vec<i64> {
        let mut out: Vec<i64> = tags
            .into_iter()
            .flatten()
            .map(|t| {
                assert_eq!(t.primary_id, TAG_ID);
                t.primary_ordinal
            })
            .collect();
        out.sort_unstable();
        out
    };
    // the four side faces of each step: profile edge i's ordinal (i + 1) plus one
    // SM_TI_SWEEP_STEP_DELTA per step
    let faces = ordinals(
        b.face_list()
            .iter()
            .map(|&f| tag_of(EntityRef::Face(f)))
            .collect(),
    );
    let steps: Vec<i64> = [d, 2 * d]
        .into_iter()
        .flat_map(|step| (1..=4).map(move |i| i + step))
        .collect();
    assert_eq!(faces, steps);
    // the profile edges keep (i + 1) and each step's far edges carry the step's copy
    let edges = ordinals(
        b.edge_list()
            .iter()
            .map(|&e| tag_of(EntityRef::Edge(e)))
            .collect(),
    );
    let mut expected: Vec<i64> = (1..=4).chain(steps).collect();
    expected.sort_unstable();
    assert_eq!(edges, expected);
}
