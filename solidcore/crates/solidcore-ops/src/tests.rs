//! The skeleton every family shares: primitives, copy and transform, counts and volume, `.smb`
//! bytes, the check, the result contract and the provenance hook.

use std::fmt::Write as _;

use solidcore::kernel::UpstreamFixes;
use solidcore::topology::face::p1::flip_face_orientation;
use solidcore::{Brep, EntityRef};

use crate::body::{finish, Expect};
use crate::provenance::{self, Provenance, FIRST_ID};
use crate::session::{self, run, with_body};
use crate::{make_box, make_cone, make_cylinder, make_sphere, make_torus, Body, CheckLevel, Error};

/// Every test here runs with every fix on, as an application with all of them named would.
fn all_fixes() {
    session::set_thread_fixes(UpstreamFixes::NAMES.iter().copied()).unwrap();
}

#[test]
fn a_box_measures_exactly() {
    all_fixes();
    let b = make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    let v = b.volume(1e-8).unwrap();
    assert!((v - 6000.0).abs() < 6000.0 * 1e-9, "{v}");
}

/// The accuracy goes to the kernel as given: one it refuses is its status.
#[test]
fn a_volume_accuracy_is_the_kernels_to_refuse() {
    all_fixes();
    let ball = make_sphere([1.0, 2.0, 3.0], 5.0).unwrap();
    let at = ball.volume(1e-8).unwrap();
    assert!(
        (at - 4.0 / 3.0 * std::f64::consts::PI * 125.0).abs() < 1e-6,
        "{at}"
    );
    let refused = ball.volume(f64::INFINITY).err();
    assert!(
        matches!(refused, Some(Error::Refused { reason, .. }) if !reason.is_empty()),
        "{refused:?}"
    );
}

#[test]
fn hostile_numbers_are_refused() {
    all_fixes();
    assert_eq!(
        make_box([f64::NAN, 0.0, 0.0], [1.0; 3]).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        make_box([0.0; 3], [1.0, -1.0, 1.0]).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        make_cylinder([0.0; 3], f64::INFINITY, 1.0).err(),
        Some(Error::Argument)
    );
    assert_eq!(
        make_cone([0.0; 3], 2.0, 2.0, 1.0).err(),
        Some(Error::Argument)
    );
    assert_eq!(make_torus([0.0; 3], 1.0, 2.0).err(), Some(Error::Argument));
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let mut m = IDENTITY;
    m[3][0] = f64::NAN;
    assert_eq!(b.transform(&m).err(), Some(Error::Argument));
    let mut m = IDENTITY;
    m[0][3] = 0.5;
    assert_eq!(
        b.transform(&m).err(),
        Some(Error::Argument),
        "a projective fourth column"
    );
    let mut m = IDENTITY;
    m[3][3] = 2.0;
    assert_eq!(
        b.transform(&m).err(),
        Some(Error::Argument),
        "a projective fourth column"
    );
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
}

/// A zero scale and a rank-2 map (z onto the xy plane) are singular: refused before the kernel,
/// the body bit for bit as it was.
#[test]
fn a_singular_transform_is_refused_and_the_body_kept() {
    all_fixes();
    let b = make_box([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]).unwrap();
    let before = b.to_smb(false).unwrap();
    let mut zero = IDENTITY;
    for (i, row) in zero.iter_mut().enumerate().take(3) {
        row[i] = 0.0;
    }
    let mut rank2 = IDENTITY;
    rank2[2] = [1.0, 1.0, 0.0, 0.0];
    // near-singular: one direction flattened against the others (the rows' own scales apart,
    // this was proved regular)
    let mut flattened = IDENTITY;
    flattened[0][0] = 1e-300;
    for m in [zero, rank2, flattened] {
        assert_eq!(b.transform(&m).err(), Some(Error::Argument), "{m:?}");
    }
    assert_eq!(b.to_smb(false).unwrap(), before);
    // a regular stretch still moves it
    let mut stretch = IDENTITY;
    stretch[0][0] = 2.0;
    b.transform(&stretch).unwrap();
    assert!((b.volume(1e-8).unwrap() - 240.0).abs() < 240.0 * 1e-9);
    // a uniform scale of any size is regular: it is not refused as singular
    let mut tiny = IDENTITY;
    for (i, row) in tiny.iter_mut().enumerate().take(3) {
        row[i] = 1e-300;
    }
    assert_ne!(b.transform(&tiny).err(), Some(Error::Argument));
}

#[test]
fn every_primitive_is_valid_at_every_level() {
    all_fixes();
    let o = [1.0, 2.0, 3.0];
    let bodies = [
        ("box", make_box(o, [2.0, 3.0, 4.0]).unwrap()),
        ("cylinder", make_cylinder(o, 15.0, 50.0).unwrap()),
        ("sphere", make_sphere(o, 12.0).unwrap()),
        ("frustum", make_cone(o, 10.0, 4.0, 20.0).unwrap()),
        ("cone", make_cone(o, 10.0, 0.0, 20.0).unwrap()),
        ("torus", make_torus(o, 22.0, 4.0).unwrap()),
    ];
    for (name, b) in &bodies {
        for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
            let r = b.check(level).unwrap();
            assert!(r.is_valid(), "{name}, {level:?}: {r}");
            assert_eq!(r.text, "");
        }
    }
}

/// A vertex whose tolerance is below the body's fails the check with its numbered line; a
/// vertex pointing at no vertexuse fails `ValidatePointers`, and `AssertValid` is then skipped.
#[test]
fn a_failed_check_reports_its_lines() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    with_body("loosen", &b, |_, br| {
        let v = br.vertex_list()[0];
        br.vertex_mut(v)?.tolerance = 0.5;
        Ok(())
    })
    .unwrap();
    let r = b.check(CheckLevel::Geometry).unwrap();
    assert!(!r.is_valid());
    assert_eq!(
        r.failures,
        r.text.lines().filter(|l| l.contains("::Test #")).count(),
        "{r}"
    );
    assert!(!r.failed_tests().is_empty(), "{r}");
    let c = make_box([0.0; 3], [1.0; 3]).unwrap();
    with_body("unhook", &c, |_, br| {
        let v = br.vertex_list()[0];
        br.vertex_mut(v)?.vertexuses.clear();
        Ok(())
    })
    .unwrap();
    let r = c.check(CheckLevel::Basic).unwrap();
    assert!(!r.is_valid());
    assert!(
        r.text
            .contains("AssertValid skipped: ValidatePointers failed"),
        "{r}"
    );
}

#[test]
fn smb_bytes_round_trip_valid() {
    all_fixes();
    let b = make_torus([0.0; 3], 10.0, 3.0).unwrap();
    for ascii in [false, true] {
        let back = Body::from_smb(b.to_smb(ascii).unwrap(), ascii).unwrap();
        assert_eq!(back.counts().unwrap(), b.counts().unwrap());
        let r = back.check(CheckLevel::All).unwrap();
        assert!(r.is_valid(), "ascii {ascii}: {r}");
    }
}

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn first_point(b: &Body) -> [f64; 3] {
    let br = b.lock();
    let p = br.vertex(br.vertex_list()[0]).unwrap().point;
    [p.x, p.y, p.z]
}

/// A translation, a rotation, a mirror and a non-uniform scale each move the body they are given
/// as their closed forms say: the volume times the determinant's size, a valid solid after.
#[test]
fn a_transform_moves_the_body_it_is_given() {
    all_fixes();
    let cases: [(&str, [[f64; 4]; 4], f64); 4] = [
        (
            "translation",
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [5.0, 6.0, 7.0, 1.0],
            ],
            1.0,
        ),
        (
            "quarter turn about z",
            [
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0; 4].map(|_| 0.0),
            ],
            1.0,
        ),
        (
            "mirror in x",
            [
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            1.0,
        ),
        (
            "scale 2, 3, 0.5",
            [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 3.0, 0.0, 0.0],
                [0.0, 0.0, 0.5, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            3.0,
        ),
    ];
    for (name, mut m, det) in cases {
        m[3][3] = 1.0;
        let b = make_box([1.0, 1.0, 1.0], [1.0, 2.0, 3.0]).unwrap();
        let p = first_point(&b);
        b.transform(&m).unwrap();
        let q = first_point(&b);
        let image: Vec<f64> = (0..3)
            .map(|c| p[0] * m[0][c] + p[1] * m[1][c] + p[2] * m[2][c] + m[3][c])
            .collect();
        assert_eq!(q.to_vec(), image, "{name}");
        let v = b.volume(1e-8).unwrap();
        assert!((v - 6.0 * det).abs() < 1e-8 * 6.0 * det, "{name}: {v}");
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{name}");
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Origin(Vec<usize>);

impl Provenance for Origin {
    const ID: u64 = FIRST_ID + 41;
    fn split(&self, child: Option<&Self>) -> Option<Self> {
        Some(child.map_or_else(|| self.clone(), |c| self.joined(c)))
    }
    fn merge(&self, result: Option<&Self>) -> Option<Self> {
        Some(result.map_or_else(|| self.clone(), |r| self.joined(r)))
    }
    fn coincident(&self, onto: Option<&Self>) -> Option<Self> {
        Some(onto.map_or_else(|| self.clone(), |o| self.joined(o)))
    }
}

impl Origin {
    fn joined(&self, other: &Self) -> Self {
        let mut v = self.0.clone();
        v.extend(&other.0);
        v.sort_unstable();
        v.dedup();
        Origin(v)
    }
}

/// Another kind of data under the same ID.
#[derive(Clone, Debug, PartialEq)]
struct Impostor;

impl Provenance for Impostor {
    const ID: u64 = FIRST_ID + 41;
    fn split(&self, _: Option<&Self>) -> Option<Self> {
        None
    }
    fn merge(&self, _: Option<&Self>) -> Option<Self> {
        None
    }
    fn coincident(&self, _: Option<&Self>) -> Option<Self> {
        None
    }
}

#[test]
fn one_provenance_type_per_id() {
    provenance::take::<Origin>().unwrap();
    provenance::take::<Origin>().unwrap();
    assert_eq!(provenance::take::<Impostor>(), Err(Error::Argument));
}

/// Each face of `b` carries its index, each edge 100 + its index.
fn tag(b: &mut Brep, base: usize) {
    provenance::take::<Origin>().unwrap();
    for (i, f) in b.face_list().to_vec().into_iter().enumerate() {
        provenance::attach(b, EntityRef::Face(f), Origin(vec![base + i])).unwrap();
    }
    for (i, e) in b.edge_list().to_vec().into_iter().enumerate() {
        provenance::attach(b, EntityRef::Edge(e), Origin(vec![base + 100 + i])).unwrap();
    }
}

/// A copy keeps every attribute its source carries, in the same order (solidcore#1823's
/// review: a copy made under a fresh owner identity lost them).
#[test]
fn a_copy_keeps_the_provenance_on_its_faces_and_edges() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    {
        let mut k = b.lock();
        tag(&mut k, 0);
        // Attaching again replaces.
        let f0 = k.face_list()[0];
        provenance::attach(&mut k, EntityRef::Face(f0), Origin(vec![0])).unwrap();
    }
    let c = b.copy().unwrap();
    let (src, copy) = (b.lock(), c.lock());
    assert_eq!((copy.face_list().len(), copy.edge_list().len()), (6, 12));
    for (i, (&f, &g)) in src.face_list().iter().zip(copy.face_list()).enumerate() {
        assert_eq!(
            provenance::read::<Origin>(&copy, EntityRef::Face(g)),
            Some(Origin(vec![i])),
            "face {i}"
        );
        assert_eq!(
            provenance::read::<Origin>(&src, EntityRef::Face(f)),
            Some(Origin(vec![i]))
        );
    }
    for (i, &e) in copy.edge_list().iter().enumerate() {
        assert_eq!(
            provenance::read::<Origin>(&copy, EntityRef::Edge(e)),
            Some(Origin(vec![100 + i])),
            "edge {i}"
        );
    }
}

#[test]
fn a_copy_maps_record_order_through_renumbered_entities() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let (_, order) = b.to_record().unwrap();
    {
        let mut src = b.lock();
        for (i, &f) in order.faces.iter().enumerate() {
            provenance::attach(&mut src, EntityRef::Face(f), Origin(vec![i])).unwrap();
        }
        for (i, &e) in order.edges.iter().enumerate() {
            provenance::attach(&mut src, EntityRef::Edge(e), Origin(vec![100 + i])).unwrap();
        }
        for (i, &v) in order.vertices.iter().enumerate() {
            provenance::attach(&mut src, EntityRef::Vertex(v), Origin(vec![200 + i])).unwrap();
        }
        let faces = src.face_list().iter().rev().copied().collect();
        src.set_face_list(faces).unwrap();
        let edges = src.edge_list().iter().rev().copied().collect();
        src.set_edge_list(edges).unwrap();
        let vertices = src.vertex_list().iter().rev().copied().collect();
        src.set_vertex_list(vertices).unwrap();
    }
    let (copy, mapped) = b.copy_with_order(&order).unwrap();
    assert_eq!(
        [
            mapped.faces.len(),
            mapped.edges.len(),
            mapped.vertices.len()
        ],
        [6, 12, 8]
    );
    assert!(order.faces.iter().zip(&mapped.faces).any(|(a, b)| a != b));
    let (src, copy) = (b.lock(), copy.lock());
    for (i, (&from, &to)) in order.faces.iter().zip(&mapped.faces).enumerate() {
        assert_eq!(
            provenance::read::<Origin>(&src, EntityRef::Face(from)),
            Some(Origin(vec![i]))
        );
        assert_eq!(
            provenance::read::<Origin>(&copy, EntityRef::Face(to)),
            Some(Origin(vec![i]))
        );
    }
    for (i, (&from, &to)) in order.edges.iter().zip(&mapped.edges).enumerate() {
        assert_eq!(
            provenance::read::<Origin>(&src, EntityRef::Edge(from)),
            Some(Origin(vec![100 + i]))
        );
        assert_eq!(
            provenance::read::<Origin>(&copy, EntityRef::Edge(to)),
            Some(Origin(vec![100 + i]))
        );
    }
    for (i, (&from, &to)) in order.vertices.iter().zip(&mapped.vertices).enumerate() {
        assert_eq!(
            provenance::read::<Origin>(&src, EntityRef::Vertex(from)),
            Some(Origin(vec![200 + i]))
        );
        assert_eq!(
            provenance::read::<Origin>(&copy, EntityRef::Vertex(to)),
            Some(Origin(vec![200 + i]))
        );
    }
}

#[test]
fn a_copy_refuses_an_order_entry_without_a_copy() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let (_, mut order) = b.to_record().unwrap();
    order
        .faces
        .push(solidcore::topology::FaceId::from_index(1_000_000));
    assert!(matches!(b.copy_with_order(&order), Err(Error::Record(_))));
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    assert!((b.volume(1e-9).unwrap() - 1.0).abs() <= 1e-9);
}

/// A slab cut out of a box's middle splits each side face in two: both pieces carry the side's
/// data. A box joined to its neighbour along a face merges their coplanar sides: the merged face
/// carries both. Faces the boolean finds lying on one another join theirs onto the survivor.
#[test]
fn provenance_goes_through_splits_and_merges() {
    use solidcore::boolean::api_brep::boolean_difference;
    use solidcore::primitive::api_primitives::create_box;
    all_fixes();
    let split = run("split", |k| {
        let mut a = create_box(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 3.0, 3.0, 3.0)?;
        tag(&mut a, 0);
        let slab = create_box(k, solidcore::Vec3::new(-1.0, -1.0, 1.0), 5.0, 5.0, 1.0)?;
        boolean_difference(k, a, slab)
    })
    .unwrap();
    let seen: Vec<Option<Origin>> = split
        .face_list()
        .iter()
        .map(|&f| provenance::read::<Origin>(&split, EntityRef::Face(f)))
        .collect();
    // the box's faces: its top and bottom whole, each side in two pieces; the slab's two faces
    // (the cut's walls) carry nothing, the slab carrying nothing
    assert_eq!(seen.len(), 2 + 2 * 4 + 2, "{seen:?}");
    assert_eq!(seen.iter().filter(|o| o.is_none()).count(), 2, "{seen:?}");
    let mut counts = [0usize; 6];
    for o in seen.iter().flatten() {
        assert_eq!(o.0.len(), 1, "{o:?}");
        counts[o.0[0]] += 1;
    }
    let mut sorted = counts;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        [1, 1, 2, 2, 2, 2],
        "pieces per face of the box: {counts:?}"
    );
}

/// The merge and coincidence rules, as the kernel's attribute store runs them: a merge runs
/// once per attribute id with the first source's value (`MergeInBrep`'s `Merge(a, b, a)`),
/// and the survivor still carries both sources' data; an entity found lying on another hands
/// its data on to that one.
#[test]
fn provenance_goes_through_merges_and_coincidences() {
    use crate::provenance::Carried;
    use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
    let mut store = AttributeStore::default();
    let (a, b, c, d) = (ObjectKey(1), ObjectKey(2), ObjectKey(3), ObjectKey(4));
    let on = |store: &mut AttributeStore, x: ObjectKey, o: Origin| {
        let h = store
            .insert(Attribute::application(
                Origin::ID,
                Behavior::Copy,
                Carried(o),
            ))
            .unwrap();
        store.add_attribute(x, h).unwrap();
        h
    };
    let read = |store: &AttributeStore, x: ObjectKey| {
        let h = store.find_attribute(x, Origin::ID)?;
        store
            .get(h)?
            .application_value::<Carried<Origin>>()
            .map(|c| c.0.clone())
    };
    let ha = on(&mut store, a, Origin(vec![1]));
    on(&mut store, b, Origin(vec![2]));
    store.merge(ha, a, b, a).unwrap();
    assert_eq!(read(&store, a), Some(Origin(vec![1, 2])));
    let hc = on(&mut store, c, Origin(vec![3]));
    on(&mut store, d, Origin(vec![4]));
    store.coincident(hc, c, d).unwrap();
    assert_eq!(read(&store, d), Some(Origin(vec![3, 4])));
}

/// The data of `b`'s entity `e`.
fn origins(b: &Brep, e: EntityRef) -> Vec<usize> {
    provenance::read::<Origin>(b, e).map_or_else(Vec::new, |o| o.0)
}

/// Two unit boxes side by side, united: the kernel merges their coplanar sides and their
/// collinear edges (`MergeInBrep`), and the merged face and edge carry both boxes' data.
#[test]
fn a_union_keeps_both_boxes_provenance_on_what_it_merges() {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::query::api_queries::{edge_bounding_box, face_bounding_box};
    all_fixes();
    let u = run("union", |k| {
        let mut a = create_box(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0)?;
        tag(&mut a, 0);
        let mut b = create_box(k, solidcore::Vec3::new(1.0, 0.0, 0.0), 1.0, 1.0, 1.0)?;
        tag(&mut b, 1000);
        boolean_union(k, a, b)
    })
    .unwrap();
    assert_eq!((u.face_list().len(), u.edge_list().len()), (6, 12));
    let k = solidcore::kernel::Kernel::default();
    let from_both = |v: &[usize], lo: usize| {
        v.len() == 2
            && v.iter().any(|&x| (lo..lo + 100).contains(&x))
            && v.iter().any(|&x| (1000 + lo..1100 + lo).contains(&x))
    };
    // the top, z = 1 over x in [0, 2]
    let top = u
        .face_list()
        .iter()
        .copied()
        .find(|&f| {
            let bx = face_bounding_box(&k, &u, f, true).unwrap();
            (bx.min().z - 1.0).abs() < 1e-9 && (bx.max().z - 1.0).abs() < 1e-9
        })
        .unwrap();
    let on_top = origins(&u, EntityRef::Face(top));
    assert!(from_both(&on_top, 0), "{on_top:?}");
    // the edge along x at y = 0, z = 0
    let along = u
        .edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let bx = edge_bounding_box(&k, &u, e, true).unwrap();
            bx.max().x - bx.min().x > 1.5 && bx.max().y.abs() < 1e-9 && bx.max().z.abs() < 1e-9
        })
        .unwrap();
    let on_edge = origins(&u, EntityRef::Edge(along));
    assert!(from_both(&on_edge, 100), "{on_edge:?}");
}

/// A box's edge deleted: the two faces it parted merge into one, which carries both's data.
#[test]
fn a_face_merge_by_delete_edge_keeps_both_faces_provenance() {
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::topology::brep::p3::edit_brep;
    use solidcore::topology::brep::p4::{delete_edge, DeleteEnv};
    all_fixes();
    let b = run("merged", |k| {
        let mut b = create_box(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0)?;
        tag(&mut b, 0);
        let e = b.edge_list()[0];
        edit_brep(k, &mut b, |k, b, scope| {
            delete_edge(k, b, &mut DeleteEnv::from_notify(scope.env), e, None)
        })?;
        Ok(b)
    })
    .unwrap();
    assert_eq!(b.face_list().len(), 5);
    let merged: Vec<Vec<usize>> = b
        .face_list()
        .iter()
        .map(|&f| origins(&b, EntityRef::Face(f)))
        .filter(|o| o.len() == 2)
        .collect();
    assert_eq!(
        merged.len(),
        1,
        "one face carries two faces' data: {merged:?}"
    );
}

/// A solid input must give a solid: an empty result is `Empty`, a box turned inside out is
/// `NotSolid`, a box is a solid. What is expected comes from the input.
#[test]
fn the_result_contract() {
    all_fixes();
    let out = run("contract", |k| {
        let mut input = solidcore::primitive::api_primitives::create_box(
            k,
            solidcore::Vec3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )?;
        let expect = Expect::of(k, &mut input)?;
        let empty = finish(k, Brep::new_with_infinite_region()?, expect)?.err();
        let mut inside_out = solidcore::primitive::api_primitives::create_box(
            k,
            solidcore::Vec3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )?;
        for f in inside_out.face_list().to_vec() {
            flip_face_orientation(&mut inside_out, f)?;
        }
        let flipped = finish(k, inside_out, expect)?.err();
        let same = finish(k, input, expect)?.is_ok();
        let any = solidcore::primitive::api_primitives::create_box(
            k,
            solidcore::Vec3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )?;
        let sheet_ok = finish(k, any, Expect::FACES)?.is_ok();
        Ok(Some((empty, flipped, same, sheet_ok)))
    })
    .unwrap();
    assert_eq!(
        out,
        Some((Some(Error::Empty), Some(Error::NotSolid), true, true))
    );
}

/// `check_solid` names the test a body fails (#15035): a box passes; a sheet is not manifold; a
/// box with every face flipped fails the pointer check; a box with its material outside is
/// proved inside out. The body checked is unchanged.
#[test]
fn check_solid_names_the_test_a_body_fails() {
    use crate::SolidTest;
    use solidcore::primitive::api_primitives::{create_box, create_plane};
    use solidcore::topology::brep::p1::regions;
    all_fixes();
    let boxed = |k: &mut solidcore::Kernel| {
        create_box(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 1.0, 2.0, 3.0)
    };
    let solid = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    assert_eq!(solid.check_solid(), Ok(()));
    let bodies = run("check_solid bodies", |k| {
        let sheet = create_plane(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0)?;
        let mut flipped = boxed(k)?;
        for f in flipped.face_list().to_vec() {
            flip_face_orientation(&mut flipped, f)?;
        }
        let mut inside_out = boxed(k)?;
        for r in regions(&inside_out, None)? {
            let g = inside_out.region_mut(r)?;
            g.is_void = 1 - g.is_void;
        }
        Ok(Some([sheet, flipped, inside_out].map(Body::new)))
    })
    .unwrap()
    .unwrap();
    let before = bodies.each_ref().map(|b| b.to_smb(true).unwrap());
    let tests = bodies.each_ref().map(|b| b.check_solid().err());
    assert_eq!(
        tests,
        [
            Some(Error::NotSolidBecause(SolidTest::NotManifold)),
            Some(Error::NotSolidBecause(SolidTest::PointersInvalid)),
            Some(Error::NotSolidBecause(SolidTest::VolumeNegative)),
        ]
    );
    assert_eq!(bodies.each_ref().map(|b| b.to_smb(true).unwrap()), before);
}

/// A kernel invariant found broken is `Invariant`: the kernel's class of it, marked a defect,
/// the operation and what did not hold; a call made before its preparation stays `Exception`
/// (#15035). No input reaches a broken invariant (each one found is a defect, fixed), so the
/// kernel's error is made here.
#[test]
fn a_broken_kernel_invariant_is_named_not_an_exception() {
    use solidcore::error::ErrorKind;
    use solidcore::SolidError;
    let invariant = crate::error::status(SolidError::PortInvariant {
        operation: "offset::close_loop_gaps",
        reason: "a kept edge without a 3D curve",
    });
    assert_eq!(
        invariant,
        Error::Invariant {
            class: ErrorKind::PortInvariant,
            defect: true,
            operation: "offset::close_loop_gaps",
            reason: "a kept edge without a 3D curve",
        }
    );
    assert_eq!(
        invariant.to_string(),
        "solidcore broke its own invariant in offset::close_loop_gaps \
         (port_invariant, a defect to report): a kept edge without a 3D curve"
    );
    assert_eq!(
        crate::error::status(SolidError::PreparationRequired {
            operation: "query::distance",
        }),
        Error::Exception
    );
}

/// What a refused edit must give back as it came in, beyond the `.smb` bytes (which carry no
/// attributes): the naming, the key, the locked box, and every face's and edge's attributes in
/// order (a temporary one and the provenance among them), with the geometry and topology.
fn as_it_is(b: &Body) -> String {
    let b = b.lock();
    let mut out = format!(
        "{:?} {:?} {:?}",
        b.naming(),
        b.key(),
        b.locked_bounding_box()
    );
    let entities = b
        .face_list()
        .iter()
        .map(|&f| EntityRef::Face(f))
        .chain(b.edge_list().iter().map(|&e| EntityRef::Edge(e)));
    for e in entities {
        let _ = write!(out, " {e:?}: {:?}", b.entity_attributes(e));
    }
    let _ = write!(out, " {:?} {:?}", b.faces(), b.vertices());
    out
}

/// A body read from `.smb` (its naming the caller's), its faces and edges tagged with
/// provenance, face 0 carrying a temporary and a copied attribute, its bounding box locked.
fn kept_body() -> Body {
    use solidcore::geom::attribute::{Attribute, Behavior};
    let bytes = make_box([1.0, 2.0, 3.0], [4.0, 5.0, 6.0])
        .unwrap()
        .to_smb(false)
        .unwrap();
    let b = Body::from_smb(bytes, false).unwrap();
    run("lock the box", |k| {
        let mut br = b.lock();
        tag(&mut br, 0);
        let f0 = EntityRef::Face(br.face_list()[0]);
        br.add_attribute(f0, Attribute::long(10_077, 7, Behavior::Temp))?;
        br.add_attribute(f0, Attribute::long(10_078, 8, Behavior::Copy))?;
        solidcore::query::brep::lock_bounding_box(k, &mut br, false)
    })
    .unwrap();
    {
        let br = b.lock();
        assert_eq!(br.naming(), solidcore::topology::NamingScheme::Caller);
        assert!(br.locked_bounding_box().is_some());
    }
    b
}

/// Changes everything [`as_it_is`] reads.
fn scramble(br: &mut Brep) {
    br.set_naming(solidcore::topology::NamingScheme::Notify);
    br.set_key(solidcore::geom::attribute::ObjectKey(99));
    br.set_locked_bounding_box(None);
    let f0 = EntityRef::Face(br.face_list()[0]);
    let first = br.entity_attributes(f0)[0].0;
    br.remove_attribute(f0, first).unwrap();
}

/// A refused edit leaves the body exactly as it came in: a result with no faces
/// ([`Error::Empty`]), a solid's result that is no solid ([`Error::NotSolid`]), and a kernel
/// error after the body was edited. Each edit first changes what the body carries.
#[test]
fn a_refused_edit_leaves_the_body_as_it_was() {
    all_fixes();
    let b = kept_body();
    let before = as_it_is(&b);
    let smb = b.to_smb(false).unwrap();
    let emptied: crate::Result<()> = with_body("empties", &b, |k, br| {
        scramble(br);
        // an edit that leaves no face
        *br = Brep::default();
        Ok(finish(k, std::mem::take(br), Expect::FACES)?.map(|r| *br = r))
    })
    .unwrap();
    assert_eq!(emptied, Err(Error::Empty));
    assert_eq!(as_it_is(&b), before);
    let opened: crate::Result<()> = with_body("opens", &b, |k, br| {
        let expect = Expect::of(k, br)?;
        scramble(br);
        let sheet =
            solidcore::primitive::api_primitives::create_plane(k, solidcore::Vec3::ZERO, 1.0, 1.0)?;
        Ok(finish(k, sheet, expect)?.map(|r| *br = r))
    })
    .unwrap();
    assert_eq!(opened, Err(Error::NotSolid));
    assert_eq!(as_it_is(&b), before);
    let failed: crate::Result<()> = with_body("fails after an edit", &b, |_, br| {
        scramble(br);
        let v = br.vertex_list()[0];
        br.vertex_mut(v)?.tolerance = 0.5;
        Err(solidcore::SolidError::sm_err("test"))
    });
    assert_eq!(failed, Err(Error::Status(1001)));
    assert_eq!(as_it_is(&b), before);
    assert_eq!(b.to_smb(false).unwrap(), smb);
    // a kept edit stays
    with_body("keeps", &b, |_, br| {
        let v = br.vertex_list()[0];
        br.vertex_mut(v)?.tolerance = 0.5;
        Ok(())
    })
    .unwrap();
    assert_ne!(as_it_is(&b), before);
}

/// An edit that changes the body and then panics gives it back as it came in, usable: the
/// panic is the session's ([`Error::Internal`]), and the body still counts and carries what it
/// carried.
#[test]
fn a_panicking_edit_leaves_the_body_as_it_was_and_usable() {
    all_fixes();
    let b = kept_body();
    let before = as_it_is(&b);
    let r: crate::Result<()> = with_body("panics part way", &b, |_, br| {
        let f0 = EntityRef::Face(br.face_list()[0]);
        let first = br.entity_attributes(f0)[0].0;
        br.remove_attribute(f0, first)?;
        br.set_locked_bounding_box(None);
        panic!("part way through an edit")
    });
    assert!(matches!(r, Err(Error::Internal(ref m)) if m.contains("part way")));
    assert_eq!(b.counts().unwrap(), [6, 12, 8]);
    assert_eq!(as_it_is(&b), before);
    assert!(b.copy().is_ok());
}

/// A body the kernel cannot snapshot (here: inside a scope that clears a surface's owner) is
/// refused before the edit runs, typed, and untouched.
#[test]
fn an_edit_without_a_snapshot_is_refused_up_front() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let f0 = b.lock().face_list()[0];
    b.lock().begin_surface_owner_cleared(f0);
    let before = as_it_is(&b);
    let ran = std::cell::Cell::new(false);
    let r: crate::Result<()> = with_body("edits", &b, |_, br| {
        ran.set(true);
        br.set_locked_bounding_box(None);
        Ok(())
    });
    assert!(matches!(r, Err(Error::Unsupported(_))), "{r:?}");
    assert!(!ran.get());
    assert_eq!(as_it_is(&b), before);
    b.lock().end_surface_owner_cleared(f0);
}

/// An operation that makes a solid from a sheet states it ([`Expect::SOLID`]): a sheet's
/// result that is no solid is `NotSolid`, the body as it came in.
#[test]
fn a_result_stated_solid_must_be_one() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let before = b.to_smb(false).unwrap();
    let r: crate::Result<()> = with_body("a sheet stated solid", &b, |k, br| {
        let sheet =
            solidcore::primitive::api_primitives::create_plane(k, solidcore::Vec3::ZERO, 1.0, 1.0)?;
        Ok(finish(k, sheet, Expect::SOLID)?.map(|r| *br = r))
    })
    .unwrap();
    assert_eq!(r, Err(Error::NotSolid));
    assert_eq!(b.to_smb(false).unwrap(), before);
}

/// A body goes to the record and back through ops with its order: record face, edge and vertex
/// `i` is the kernel entity `Order` names, in both directions; a record the conversion refuses
/// is the operation's refusal, as data.
#[test]
fn a_body_goes_to_the_record_and_back_in_record_order() {
    all_fixes();
    let b = make_cylinder([1.0, 2.0, 3.0], 4.0, 5.0).unwrap();
    let (r, order) = b.to_record().unwrap();
    assert_eq!(order.faces.len(), r.faces.len());
    assert_eq!(order.edges.len(), r.edges.len());
    assert_eq!(order.vertices.len(), r.vertices.len());
    let (back, back_order) = Body::from_record(&r, true).unwrap();
    assert_eq!(back.counts().unwrap(), b.counts().unwrap());
    // record vertex i is the kernel vertex the order names, at the record's point
    {
        let kernel = back.lock();
        for (i, v) in back_order.vertices.iter().enumerate() {
            let p = kernel.vertex(*v).unwrap().point;
            assert_eq!([p.x, p.y, p.z], r.vertices[i].point, "vertex {i}");
        }
    }
    let (again, _) = back.to_record().unwrap();
    assert_eq!(again, r);
    let mut bad = r.clone();
    bad.version = 3;
    assert!(matches!(
        Body::from_record(&bad, true),
        Err(Error::Record(_))
    ));
}

/// A record's structure is checked before the kernel sees it: a use's interval outside its
/// curve's range, a coordinate that is not a number, a circle of no radius, are refused as
/// invalid records (the kernel's pcurve takes converted, checked values).
#[test]
fn pcurves_of_malformed_records_are_refused_as_invalid() {
    use crate::pcurve::{pcurves, EdgeUse, Error};
    use solidcore_record::brep::{Curve, Frame, Surface};
    let plane = Surface::Plane {
        origin: [0.; 3],
        u_axis: [1., 0., 0.],
        v_axis: [0., 1., 0.],
    };
    let line = Curve::Line {
        start: [0.; 3],
        end: [1., 0., 0.],
    };
    let one = |curve: &Curve, interval: [f64; 2]| {
        let c = curve.clone();
        pcurves(
            &plane,
            &[vec![EdgeUse {
                edge: 0,
                curve: &c,
                interval,
                forward: true,
                tolerance: 0.0,
                given: None,
            }]],
            1e-6,
        )
    };
    for interval in [
        [1., 1.],
        [2., 1.],
        [f64::NAN, 1.],
        [0., f64::INFINITY],
        [0., 1e13],
    ] {
        assert!(
            matches!(one(&line, interval), Err(Error::Invalid(_))),
            "{interval:?}"
        );
    }
    let nan = Curve::Line {
        start: [f64::NAN, 0., 0.],
        end: [1., 0., 0.],
    };
    assert!(matches!(one(&nan, [0., 1.]), Err(Error::Invalid(_))));
    let frame = Frame {
        origin: [0.; 3],
        x_axis: [1., 0., 0.],
        y_axis: [0., 1., 0.],
    };
    let collapsed = Curve::Circle { frame, radius: 0. };
    assert!(matches!(one(&collapsed, [0., 1.]), Err(Error::Invalid(_))));
    // And a well-formed one still gets its pcurve.
    assert!(one(&line, [0., 1.]).is_ok());
}
