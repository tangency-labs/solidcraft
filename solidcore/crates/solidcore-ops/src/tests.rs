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

/// A failed face lookup in feature discovery cannot confirm a groove. Public record
/// indices are checked before this step; here the executive itself refuses the selection.
#[test]
fn a_failed_feature_lookup_leaves_ring_removal_unmatched() {
    let mut k = solidcore::Kernel::default();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let face = solidcore::FaceId::from_index(0);
    assert!(!crate::trim::remove_selected_ring(&mut k, &mut b, &[face]).unwrap());
}

#[test]
fn an_unmatched_ring_does_not_export_the_input_for_history() {
    use solidcore::curve::line::Line;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::topology::brep::p5::create_wire_edge_from_curve;
    use solidcore::{Kernel, Vec3};

    let body = make_box([0.0; 3], [1.0; 3]).unwrap();
    let (_, order) = body.to_record().unwrap();
    let mut brep = body.into_inner();
    // The box faces keep their indices. The separate wire is valid in the kernel,
    // but the record exporter cannot represent it.
    let line = Line::create_canonical(Vec3::new(4.0, 4.0, 4.0), Vec3::new(1.0, 0.0, 0.0));
    create_wire_edge_from_curve(
        &mut Kernel::default(),
        &mut brep,
        line.into(),
        Extent1d::from_min_max(0.0, 1.0),
    )
    .unwrap();
    let body = Body::new(brep);
    assert!(body.to_record().is_err());
    assert!(crate::trim::remove_ring_groove(&body, &order, &[0])
        .unwrap()
        .is_none());
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

/// The check of some faces and edges of a valid body passes at every level, named through the
/// body's record order; a face or edge past the record's is an argument error.
#[test]
fn a_local_check_passes_where_the_body_does() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
    let order = b.to_record().unwrap().1;
    let all_faces: Vec<usize> = (0..order.faces.len()).collect();
    let all_edges: Vec<usize> = (0..order.edges.len()).collect();
    for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
        for (faces, edges) in [
            (vec![0], vec![]),
            (vec![1, 2], vec![0, 3]),
            (all_faces.clone(), all_edges.clone()),
        ] {
            let r = b.check_local(&order, &faces, &edges, level).unwrap();
            assert!(r.is_valid(), "{faces:?} {edges:?}, {level:?}: {r}");
            assert_eq!(r.text, "");
        }
    }
    let past = |faces: &[usize], edges: &[usize]| {
        matches!(
            b.check_local(&order, faces, edges, CheckLevel::All),
            Err(Error::Argument)
        )
    };
    assert!(past(&[order.faces.len()], &[]));
    assert!(past(&[], &[order.edges.len()]));
}

/// The local check runs the tolerance pass on the edges it is given. A box edge whose curve is
/// moved 0.01 off its two faces (its tolerance is 5e-6) fails it there, with the pass's own
/// reports (`SmBrep` list 2 about the edge's uses): its vertex/edge gaps (tests 0 and 2) and its
/// edge/face gap (test 4). The whole check refuses the body too. The local check of an edge with
/// no vertex or face of the moved one still passes.
#[test]
fn a_local_check_tests_the_gaps_of_the_edges_it_is_given() {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::vector3d::{Point3, Vec3};
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let order = b.to_record().unwrap().1;
    let moved = crate::order::edge(&order, 0).unwrap();
    // an edge with no vertex and no face of the moved one
    let apart = with_body("move an edge", &b, |k, br| {
        // the identity frame at (0.01, 0.01, 0.01): a move, as a placement holds it
        let shift = Axis2Placement::from_stored(
            Point3::new(0.01, 0.01, 0.01),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        br.edge_mut(moved)?
            .curve
            .as_mut()
            .expect("a box edge has its curve")
            .transform(k, &shift, None)?;
        let ends = |br: &Brep, e| solidcore::topology::edge::vertices(br, e, None);
        let faces = |br: &Brep, e| solidcore::topology::edge::faces(br, e, None);
        let (mv, mf) = (ends(br, moved)?, faces(br, moved)?);
        for i in 1..order.edges.len() {
            let e = crate::order::edge(&order, i).expect("a record edge");
            if ends(br, e)?.iter().all(|v| !mv.contains(v))
                && faces(br, e)?.iter().all(|f| !mf.contains(f))
            {
                return Ok(i);
            }
        }
        panic!("a box has an edge apart from each of its edges");
    })
    .unwrap();
    assert!(!b.check(CheckLevel::Basic).unwrap().is_valid());
    let local = b.check_local(&order, &[], &[0], CheckLevel::Basic).unwrap();
    assert!(!local.is_valid(), "{local}");
    let pass = |test: u32| {
        local.text.lines().any(|l| {
            l.starts_with(&format!("SmBrep::Test #{test} ")) && l.ends_with("[owner SmEdgeuse]")
        })
    };
    assert!(pass(4), "the edge/face gap: {local}");
    assert!(pass(0) && pass(2), "the vertex/edge gaps: {local}");
    let r = b
        .check_local(&order, &[], &[apart], CheckLevel::Basic)
        .unwrap();
    assert!(r.is_valid(), "edge {apart}: {r}");
}

/// A vertex whose tolerance is below the body's fails the check with its numbered line; a
/// vertex pointing at no vertexuse fails `ValidatePointers`, and `AssertValid` is then skipped.
#[test]
fn a_failed_check_reports_its_lines() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    with_body("shrink", &b, |_, br| {
        let v = br.vertex_list()[0];
        let tolerance = br.tolerance() * 0.5;
        br.vertex_mut(v)?.tolerance = tolerance;
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

#[test]
fn source_chord_coordinates_cross_the_affine_seam() {
    use crate::affine::chord_coordinates;
    use solidcore_record::Vec2;

    assert_eq!(
        chord_coordinates(Vec2::new(0.875, 0.15625), 0.25, 1.5).unwrap(),
        Vec2::new(0.5, 0.125)
    );
    assert!(matches!(
        chord_coordinates(Vec2::ZERO, 0.0, 0.0),
        Err(Error::Refused { .. })
    ));
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

/// Attached by record index through the body, read back per face and edge; a copy carries it.
#[test]
fn attached_provenance_reads_back_by_record_index() {
    use crate::provenance::{CarriedBy, Carrier};
    all_fixes();
    let b = make_box([0.0; 3], [1.0; 3]).unwrap();
    let o = b.own_order();
    let data = vec![
        (Carrier::Face(2), Origin(vec![7])),
        (Carrier::Edge(5), Origin(vec![105])),
        // attaching again replaces
        (Carrier::Face(2), Origin(vec![2])),
    ];
    b.attach(&o, data).unwrap();
    let want = |faces: usize, edges: usize| CarriedBy {
        faces: (0..faces)
            .map(|i| (i == 2).then(|| Origin(vec![2])))
            .collect(),
        edges: (0..edges)
            .map(|i| (i == 5).then(|| Origin(vec![105])))
            .collect(),
    };
    assert_eq!(b.carried::<Origin>(&o), Ok(want(6, 12)));
    assert!(b.tolerance().unwrap() > 0.0);
    let c = b.copy().unwrap();
    assert_eq!(c.carried::<Origin>(&c.own_order()), Ok(want(6, 12)));
    // an index past the lists is refused before anything is attached
    let past = vec![
        (Carrier::Face(0), Origin(vec![0])),
        (Carrier::Edge(12), Origin(vec![1])),
    ];
    assert_eq!(b.attach(&o, past), Err(Error::Argument));
    assert_eq!(b.carried::<Origin>(&o), Ok(want(6, 12)));
    // another type under the same ID is refused, and reads as none
    assert_eq!(
        b.attach(&o, vec![(Carrier::Face(0), Impostor)]),
        Err(Error::Argument)
    );
    let none = b.carried::<Impostor>(&o).unwrap();
    assert!(none.faces.iter().chain(&none.edges).all(Option::is_none));
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

/// A solid input must give a solid: an empty result is `Empty`, a box with its faces flipped
/// is `NotSolidBecause` naming the pointer check, a box is a solid. What is expected comes
/// from the input.
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
        Some((
            Some(Error::Empty),
            Some(Error::NotSolidBecause(crate::SolidTest::PointersInvalid)),
            true,
            true
        ))
    );
}

/// The result check of a proved operation names the test it fails, as `check_solid` does
/// (#15035's rule for every result check): `finish_under` under [`Contract::Proved`] returns
/// `NotSolidBecause(test)`, and `not_solid_test` reads the test. The helper is the check every
/// proved operation's result runs; no cheap operation makes its own result fail here.
#[test]
fn a_proved_result_names_the_test_it_fails() {
    use crate::body::{finish_under, Contract};
    use crate::SolidTest;
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::topology::brep::p1::regions;
    all_fixes();
    let out = run("proved result", |k| {
        let mut flipped = create_box(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0)?;
        for f in flipped.face_list().to_vec() {
            flip_face_orientation(&mut flipped, f)?;
        }
        let mut inside_out = create_box(k, solidcore::Vec3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0)?;
        for r in regions(&inside_out, None)? {
            let g = inside_out.region_mut(r)?;
            g.is_void = 1 - g.is_void;
        }
        let proven =
            |k: &mut solidcore::Kernel, b| finish_under(k, b, Expect::SOLID, Contract::Proved);
        Ok(Some([
            proven(k, flipped)?.err(),
            proven(k, inside_out)?.err(),
        ]))
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        out,
        [
            Some(Error::NotSolidBecause(SolidTest::PointersInvalid)),
            Some(Error::NotSolidBecause(SolidTest::VolumeNegative)),
        ]
    );
    assert_eq!(
        out.each_ref()
            .map(|e| e.as_ref().and_then(Error::not_solid_test)),
        [
            Some(SolidTest::PointersInvalid),
            Some(SolidTest::VolumeNegative)
        ]
    );
    assert_eq!(Error::Argument.not_solid_test(), None);
}

/// `check_solid` names the test a body fails (#15035): a box passes; a sheet is not manifold; a
/// box with every face flipped fails the pointer check; a box with its material outside is
/// proved inside out. The body checked is unchanged, and `not_solid_test` reads each test.
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
    assert_eq!(
        tests
            .each_ref()
            .map(|e| e.as_ref().and_then(Error::not_solid_test)),
        [
            Some(SolidTest::NotManifold),
            Some(SolidTest::PointersInvalid),
            Some(SolidTest::VolumeNegative),
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
/// ([`Error::Empty`]), a solid's result that is no solid ([`Error::NotSolidBecause`]), and a kernel
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
    assert_eq!(
        opened,
        Err(Error::NotSolidBecause(crate::SolidTest::NotManifold))
    );
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
/// result that is no solid is `NotSolidBecause(NotManifold)`, the body as it came in.
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
    assert_eq!(
        r,
        Err(Error::NotSolidBecause(crate::SolidTest::NotManifold))
    );
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

/// A real boundary crossing the cylinder's base cut must be split on both rims. The
/// 90-degree, radius-5, height-3 sheet becomes two 45-degree faces on one shell, sharing
/// the new ruling; its four original boundaries become six pieces, none discarded.
#[test]
fn periodic_seam_completion_splits_real_boundaries_and_keeps_their_shell() {
    use solidcore_record::brep::{
        Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace,
        Region, Shell, Surface, Vertex, VERSION,
    };

    all_fixes();
    let (r, h, w) = (5.0, 3.0, std::f64::consts::FRAC_1_SQRT_2);
    let ring = [
        [r, 0.0, 1.0],
        [r * w, r * w, w],
        [0.0, r, 1.0],
        [-r * w, r * w, w],
        [-r, 0.0, 1.0],
        [-r * w, -r * w, w],
        [0.0, -r, 1.0],
        [r * w, -r * w, w],
        [r, 0.0, 1.0],
    ];
    // De Casteljau at half of the quarters on either side of the cut: these are
    // pieces of the surface's own circle, on its parameters 3.5 .. 4.5, not a fit.
    let average = |a: [f64; 3], b: [f64; 3]| std::array::from_fn(|d| 0.5 * (a[d] + b[d]));
    let (left, right) = (average(ring[7], ring[8]), average(ring[0], ring[1]));
    let (lo, hi) = (
        average(average(ring[6], ring[7]), left),
        average(right, average(ring[1], ring[2])),
    );
    let arc = |z: f64| Curve::RationalSpline {
        knots: vec![3.5, 3.5, 3.5, 4.0, 4.0, 4.5, 4.5, 4.5],
        homogeneous: [lo, left, ring[8], right, hi]
            .map(|p| [p[0], p[1], z * p[2], p[2]])
            .to_vec(),
    };
    let point = |p: [f64; 3], z| [p[0] / p[2], p[1] / p[2], z];
    let vertices = [point(lo, 0.0), point(hi, 0.0), point(hi, h), point(lo, h)]
        .map(|point| Vertex {
            point,
            tolerance: 1e-6,
        })
        .to_vec();
    let surface = Surface::RationalSpline {
        u_knots: vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.],
        v_knots: vec![0., 0., h, h],
        homogeneous: ring
            .iter()
            .map(|p| vec![[p[0], p[1], 0.0, p[2]], [p[0], p[1], h * p[2], p[2]]])
            .collect(),
    };
    // Degree-one B-splines keep the ruling's parameter in [0, h], just like v.
    // A record Line's vector is end - start per unit parameter: [0, h] would
    // otherwise traverse this height-h vector h times.
    let ruling = |a: usize, b: usize| Curve::Spline {
        knots: vec![0.0, 0.0, h, h],
        points: vec![vertices[a].point, vertices[b].point],
    };
    let curves = [arc(0.0), ruling(1, 2), arc(h), ruling(0, 3)];
    let intervals = [[3.5, 4.5], [0.0, h], [3.5, 4.5], [0.0, h]];
    let ends = [[0, 1], [1, 2], [3, 2], [0, 3]];
    let directions = [true, true, false, false];
    let pc = [
        [[3.5, 0.0], [4.5, 0.0]],
        [[4.5, 0.0], [4.5, h]],
        [[3.5, h], [4.5, h]],
        [[3.5, 0.0], [3.5, h]],
    ];
    let edges = curves
        .into_iter()
        .enumerate()
        .map(|(i, curve)| Edge {
            curve,
            nurbs: None,
            interval: intervals[i],
            vertices: ends[i],
            tolerance: 1e-6,
            uses: vec![EdgeUseRef {
                face: 0,
                r#loop: 0,
                position: i as u32,
                upper: !directions[i],
            }],
        })
        .collect();
    let boundary = (0..4)
        .map(|i| OrientedEdge {
            edge: i as u32,
            forward: directions[i],
            pcurve: Curve2::Spline {
                knots: vec![
                    intervals[i][0],
                    intervals[i][0],
                    intervals[i][1],
                    intervals[i][1],
                ],
                points: pc[i].to_vec(),
            },
        })
        .collect();
    let source = Record {
        version: VERSION,
        tolerance: 1e-6,
        regions: vec![Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: Vec::new(),
        }],
        faces: vec![Face {
            surface,
            nurbs: None,
            domain: [[0.0, 4.0], [0.0, h]],
            loops: vec![Loop::Edges { edges: boundary }],
        }],
        edges,
        vertices,
    };
    crate::record_check::validate(&source).unwrap();
    crate::record_check::agreement(&source).unwrap();
    let (completed, origins) = crate::pcurve::complete_periodic_seams(&source)
        .unwrap()
        .unwrap();
    assert_eq!(origins, [0, 0]);
    assert_eq!(completed.shells.len(), 1);
    assert_eq!(
        (
            completed.faces.len(),
            completed.edges.len(),
            completed.vertices.len()
        ),
        (2, 7, 6)
    );
    crate::record_check::validate(&completed).unwrap();
    crate::record_check::agreement(&completed).unwrap();
    // The two 45-degree patches occupy [3.5, 4] and [0, 0.5], respectively.
    // A seam copy left on the opposite side would inflate a face's UV domain
    // to a full period even though its 3D boundary still encloses only 45 degrees.
    for face in &completed.faces {
        assert!((face.domain[0][1] - face.domain[0][0] - 0.5).abs() < 1e-9);
        assert!(face.domain[0][0] >= -1e-9 && face.domain[0][1] <= 4.0 + 1e-9);
    }
    let seam = completed.edges.iter().find(|e| e.uses.len() == 2).unwrap();
    assert_ne!(seam.uses[0].face, seam.uses[1].face);
    // The inserted curve-on-surface must be stored as its exact iso B-spline,
    // retaining its own edge parameter rather than the surface's [0, h].
    assert!(matches!(
        seam.nurbs.as_ref().unwrap_or(&seam.curve),
        Curve::Spline { .. } | Curve::RationalSpline { .. }
    ));
    if let Curve::Spline { knots, .. } | Curve::RationalSpline { knots, .. } =
        seam.nurbs.as_ref().unwrap_or(&seam.curve)
    {
        // This clamped ruling's form must retain the seam parameter, not height.
        assert_eq!(knots.first(), Some(&seam.interval[0]));
        assert_eq!(knots.last(), Some(&seam.interval[1]));
    }
    assert_eq!(seam.interval, [0.0, 1.0]);
    for &v in &seam.vertices {
        let [x, y, z] = completed.vertices[v as usize].point;
        assert!((x - r).abs() < 1e-9 && y.abs() < 1e-9);
        assert!(z.abs() < 1e-9 || (z - h).abs() < 1e-9);
    }
    assert!(crate::pcurve::complete_periodic_seams(&completed)
        .unwrap()
        .is_none());
    let (body, _) = Body::from_record(&completed, true).unwrap();
    let check = body.check(CheckLevel::All).unwrap();
    assert!(check.is_valid(), "{check}");
}

/// #20575: a seam whose pcurve overshoots the base cut by a sliver (a surface fitted a little
/// off its edges) is marked for replacement. Here deleting it leaves a missing seam the
/// classification does find, so the natural seam is inserted in its place: the face stays one
/// face of five edges, a seam used twice, and the completed record is valid and agrees with
/// itself. (Where deleting it finds nothing missing the seam is kept; see the coil-wire tests
/// in the kernel's STEP import.)
#[test]
fn periodic_seam_completion_replaces_a_seam_that_overshoots_the_cut() {
    use solidcore_record::brep::{
        Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace,
        Region, Shell, Surface, Vertex, VERSION,
    };

    all_fixes();
    let (r, h, w, over) = (5.0, 3.0, std::f64::consts::FRAC_1_SQRT_2, 5e-5);
    let ring = [
        [r, 0.0, 1.0],
        [r * w, r * w, w],
        [0.0, r, 1.0],
        [-r * w, r * w, w],
        [-r, 0.0, 1.0],
        [-r * w, -r * w, w],
        [0.0, -r, 1.0],
        [r * w, -r * w, w],
        [r, 0.0, 1.0],
    ];
    let surface = Surface::RationalSpline {
        u_knots: vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.],
        v_knots: vec![0., 0., h, h],
        homogeneous: ring
            .iter()
            .map(|p| vec![[p[0], p[1], 0.0, p[2]], [p[0], p[1], h * p[2], p[2]]])
            .collect(),
    };
    let arc = |from: usize, z: f64| Curve::RationalSpline {
        knots: [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0]
            .map(|k| k + 2.0 * (from / 4) as f64)
            .to_vec(),
        homogeneous: ring[from..=from + 4]
            .iter()
            .map(|p| [p[0], p[1], z * p[2], p[2]])
            .collect(),
    };
    let vertices = [[r, 0.0, 0.0], [-r, 0.0, 0.0], [r, 0.0, h], [-r, 0.0, h]]
        .map(|point| Vertex {
            point,
            tolerance: 1e-3,
        })
        .to_vec();
    let line = |a: usize, b: usize| Curve::Spline {
        knots: vec![0.0, 0.0, h, h],
        points: vec![vertices[a].point, vertices[b].point],
    };
    // Edges, numbered as the loop meets them: bottom halves 0 and 1, the seam 2, top halves 3
    // and 4.
    let curves = [arc(0, 0.0), arc(4, 0.0), line(0, 2), arc(4, h), arc(0, h)];
    let intervals = [[0.0, 2.0], [2.0, 4.0], [0.0, h], [2.0, 4.0], [0.0, 2.0]];
    let ends = [[0, 1], [1, 0], [0, 2], [3, 2], [2, 3]];
    // The loop: the bottom left to right, the seam up (its use at the cut's high copy), the
    // top right to left, the seam down (its use at the low copy).
    let uses = [
        (0, true),
        (1, true),
        (2, true),
        (3, false),
        (4, false),
        (2, false),
    ];
    let line2 = |a: [f64; 2], b: [f64; 2], interval: [f64; 2]| Curve2::Spline {
        knots: vec![interval[0], interval[0], interval[1], interval[1]],
        points: vec![a, b],
    };
    let pcurves = [
        line2([0.0, 0.0], [2.0, 0.0], [0.0, 2.0]),
        line2([2.0, 0.0], [4.0, 0.0], [2.0, 4.0]),
        line2([4.0, 0.0], [4.0 + over, h], [0.0, h]),
        line2([2.0, h], [4.0, h], [2.0, 4.0]),
        line2([0.0, h], [2.0, h], [0.0, 2.0]),
        line2([0.0, 0.0], [0.0, h], [0.0, h]),
    ];
    let edges = curves
        .into_iter()
        .enumerate()
        .map(|(e, curve)| Edge {
            curve,
            nurbs: None,
            interval: intervals[e],
            vertices: ends[e],
            tolerance: 1e-3,
            uses: uses
                .iter()
                .enumerate()
                .filter(|(_, u)| u.0 == e)
                .map(|(position, u)| EdgeUseRef {
                    face: 0,
                    r#loop: 0,
                    position: position as u32,
                    upper: !u.1,
                })
                .collect(),
        })
        .collect();
    let boundary = uses
        .iter()
        .zip(pcurves)
        .map(|(u, pcurve)| OrientedEdge {
            edge: u.0 as u32,
            forward: u.1,
            pcurve,
        })
        .collect();
    let source = Record {
        version: VERSION,
        tolerance: 1e-6,
        regions: vec![Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: Vec::new(),
        }],
        faces: vec![Face {
            surface,
            nurbs: None,
            domain: [[0.0, 4.0], [0.0, h]],
            loops: vec![Loop::Edges { edges: boundary }],
        }],
        edges,
        vertices,
    };
    crate::record_check::validate(&source).unwrap();
    let mut kernel = solidcore::Kernel::default();
    let (mut body, order) = solidcore_convert::brep::to_kernel(&mut kernel, &source, true).unwrap();
    let face = order.faces[0];
    let pieces =
        solidcore::topology::face::p1::complete_imported_face_seams(&mut kernel, &mut body, face)
            .unwrap();
    assert_eq!(pieces.map(|p| p.len()), Some(1));
    let (back, _) = solidcore_convert::brep::to_record(&mut kernel, &mut body).unwrap();
    assert_eq!((back.faces.len(), back.edges.len()), (1, 5));
    assert!(
        back.edges.iter().any(|e| e.uses.len() == 2),
        "the seam is used twice"
    );
    crate::record_check::validate(&back).unwrap();
    crate::record_check::agreement(&back).unwrap();
    let domain = back.faces[0].domain;
    assert!(
        domain[0][0] >= -1e-9 && domain[0][1] <= 4.0 + 1e-9,
        "{domain:?}"
    );
}

/// A boundary crossing the cut whose pcurve is fitted to its edge rather than on the edge's own
/// parameter (as a STEP file's intersection edge is, #22462): the 45-degree sheet of
/// [`periodic_seam_completion_splits_real_boundaries_and_keeps_their_shell`], its top arc's
/// pcurve a quadratic in u through the arc's ends whose middle control point is 1e-6 past the
/// cut, so that where the arc meets the cut the pcurve is 5e-7 past it. The seam splits the arc
/// there, and the arc's trim at the same parameter, so the piece the face keeps, on u in
/// [3.5, 4], reaches that sliver across the cut, which no whole-period translation fits: at the
/// end of its parameter with the arc running round the axis, at its start with the arc written
/// the other way. That piece is cut where its pcurve meets the cut: the sheet completes as the
/// exact one does, two faces of seven edges, each within the base period, the piece ending on
/// the cut.
#[test]
fn periodic_seam_completion_ends_a_fitted_pcurve_on_the_cut_it_crosses() {
    for reversed in [false, true] {
        fitted_pcurve_sheet_completes(reversed);
    }
}

/// The sheet of [`periodic_seam_completion_ends_a_fitted_pcurve_on_the_cut_it_crosses`], its top
/// arc written against the axis when `reversed`, completed and checked.
fn fitted_pcurve_sheet_completes(reversed: bool) {
    use solidcore_record::brep::{
        Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace,
        Region, Shell, Surface, Vertex, VERSION,
    };

    all_fixes();
    let (r, h, w) = (5.0, 3.0, std::f64::consts::FRAC_1_SQRT_2);
    // The top arc's pcurve is off the arc's parameter by up to `lead / 2` in u, which is at most
    // 7.1e-6 mm on the radius-5 circle (whose speed is at most 2 r / w = 14.2 per unit of u); the
    // arc's tolerance, 1e-5, holds it.
    let (lead, top_tolerance) = (1e-6, 1e-5);
    let ring = [
        [r, 0.0, 1.0],
        [r * w, r * w, w],
        [0.0, r, 1.0],
        [-r * w, r * w, w],
        [-r, 0.0, 1.0],
        [-r * w, -r * w, w],
        [0.0, -r, 1.0],
        [r * w, -r * w, w],
        [r, 0.0, 1.0],
    ];
    let average = |a: [f64; 3], b: [f64; 3]| std::array::from_fn(|d| 0.5 * (a[d] + b[d]));
    let (left, right) = (average(ring[7], ring[8]), average(ring[0], ring[1]));
    let (lo, hi) = (
        average(average(ring[6], ring[7]), left),
        average(right, average(ring[1], ring[2])),
    );
    // Its knots are symmetric about 4, so the arc written the other way is its points reversed.
    let arc = |z: f64, against: bool| {
        let mut points = [lo, left, ring[8], right, hi];
        if against {
            points.reverse();
        }
        Curve::RationalSpline {
            knots: vec![3.5, 3.5, 3.5, 4.0, 4.0, 4.5, 4.5, 4.5],
            homogeneous: points.map(|p| [p[0], p[1], z * p[2], p[2]]).to_vec(),
        }
    };
    let point = |p: [f64; 3], z| [p[0] / p[2], p[1] / p[2], z];
    let vertices = [point(lo, 0.0), point(hi, 0.0), point(hi, h), point(lo, h)]
        .map(|point| Vertex {
            point,
            tolerance: 1e-6,
        })
        .to_vec();
    let surface = Surface::RationalSpline {
        u_knots: vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.],
        v_knots: vec![0., 0., h, h],
        homogeneous: ring
            .iter()
            .map(|p| vec![[p[0], p[1], 0.0, p[2]], [p[0], p[1], h * p[2], p[2]]])
            .collect(),
    };
    let ruling = |a: usize, b: usize| Curve::Spline {
        knots: vec![0.0, 0.0, h, h],
        points: vec![vertices[a].point, vertices[b].point],
    };
    let curves = [
        arc(0.0, false),
        ruling(1, 2),
        arc(h, reversed),
        ruling(0, 3),
    ];
    let intervals = [[3.5, 4.5], [0.0, h], [3.5, 4.5], [0.0, h]];
    let top_ends = if reversed { [2, 3] } else { [3, 2] };
    let ends = [[0, 1], [1, 2], top_ends, [0, 3]];
    let directions = [true, true, reversed, false];
    let top_points = if reversed {
        vec![[4.5, h], [4.0 + lead, h], [3.5, h]]
    } else {
        vec![[3.5, h], [4.0 + lead, h], [4.5, h]]
    };
    let tolerances = [1e-6, 1e-6, top_tolerance, 1e-6];
    let line = |a: [f64; 2], b: [f64; 2], [t0, t1]: [f64; 2]| Curve2::Spline {
        knots: vec![t0, t0, t1, t1],
        points: vec![a, b],
    };
    let pcurves = [
        line([3.5, 0.0], [4.5, 0.0], intervals[0]),
        line([4.5, 0.0], [4.5, h], intervals[1]),
        Curve2::Spline {
            knots: vec![3.5, 3.5, 3.5, 4.5, 4.5, 4.5],
            points: top_points,
        },
        line([3.5, 0.0], [3.5, h], intervals[3]),
    ];
    let edges = curves
        .into_iter()
        .enumerate()
        .map(|(i, curve)| Edge {
            curve,
            nurbs: None,
            interval: intervals[i],
            vertices: ends[i],
            tolerance: tolerances[i],
            uses: vec![EdgeUseRef {
                face: 0,
                r#loop: 0,
                position: i as u32,
                upper: !directions[i],
            }],
        })
        .collect();
    let boundary = pcurves
        .into_iter()
        .enumerate()
        .map(|(i, pcurve)| OrientedEdge {
            edge: i as u32,
            forward: directions[i],
            pcurve,
        })
        .collect();
    let source = Record {
        version: VERSION,
        tolerance: 1e-6,
        regions: vec![Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: Vec::new(),
        }],
        faces: vec![Face {
            surface,
            nurbs: None,
            domain: [[0.0, 4.0], [0.0, h]],
            loops: vec![Loop::Edges { edges: boundary }],
        }],
        edges,
        vertices,
    };
    crate::record_check::validate(&source).unwrap();
    crate::record_check::agreement(&source).unwrap();
    let (completed, origins) = crate::pcurve::complete_periodic_seams(&source)
        .unwrap()
        .unwrap();
    assert_eq!(origins, [0, 0], "reversed {reversed}");
    assert_eq!(
        (
            completed.faces.len(),
            completed.edges.len(),
            completed.vertices.len()
        ),
        (2, 7, 6),
        "reversed {reversed}"
    );
    crate::record_check::validate(&completed).unwrap();
    crate::record_check::agreement(&completed).unwrap();
    // Each face is its 45 degrees, [3.5, 4] or [0, 0.5], within the base period: the top arc's
    // pieces end at most `lead / 2` inside a face's side, and no further.
    let round = 1e-9 * 4.0;
    for face in &completed.faces {
        assert!(
            face.domain[0][0] >= -round && face.domain[0][1] <= 4.0 + round,
            "reversed {reversed}: {:?}",
            face.domain
        );
        assert!(
            (face.domain[0][1] - face.domain[0][0] - 0.5).abs() <= round,
            "reversed {reversed}: {:?}",
            face.domain
        );
        for l in &face.loops {
            let Loop::Edges { edges } = l else { continue };
            for u in edges {
                let points: Vec<[f64; 2]> = match &u.pcurve {
                    Curve2::Spline { points, .. } => points.clone(),
                    Curve2::RationalSpline { homogeneous, .. } => homogeneous
                        .iter()
                        .map(|p| [p[0] / p[2], p[1] / p[2]])
                        .collect(),
                };
                for p in &points {
                    assert!(
                        p[0] >= face.domain[0][0] - round && p[0] <= face.domain[0][1] + round,
                        "reversed {reversed}, edge {}: {points:?} outside {:?}",
                        u.edge,
                        face.domain
                    );
                }
            }
        }
    }
    let (body, _) = Body::from_record(&completed, true).unwrap();
    let check = body.check(CheckLevel::All).unwrap();
    assert!(check.is_valid(), "reversed {reversed}: {check}");
}

/// A wall of a closed periodic surface, bounded by two rims each made of four quarter arcs. A
/// rim's trims are lifted continuously from u = 0.5 to u = 4.5, so its last arc crosses the cut
/// (u = 4) at the middle of its parameter, and the seam has to split it there. Completing the
/// seams rebuilds the trims of the uses at the cut; every completed trim must still run over its
/// own edge's interval, which the record's validity check requires (an arc is not shortened, and
/// no trim covers only part of it).
#[test]
fn periodic_seam_completion_keeps_every_trim_over_its_edge_interval() {
    use solidcore_record::brep::{
        Brep as Record, Curve, Curve2, Edge, EdgeUseRef, Face, Loop, OrientedEdge, OrientedFace,
        Region, Shell, Surface, Vertex, VERSION,
    };

    all_fixes();
    let (r, h, w) = (5.0, 3.0, std::f64::consts::FRAC_1_SQRT_2);
    // The surface's circle: homogeneous (x, y, weight) at each 45 degrees from the cut.
    let ring = [
        [r, 0.0, 1.0],
        [r * w, r * w, w],
        [0.0, r, 1.0],
        [-r * w, r * w, w],
        [-r, 0.0, 1.0],
        [-r * w, -r * w, w],
        [0.0, -r, 1.0],
        [r * w, -r * w, w],
        [r, 0.0, 1.0],
    ];
    let surface = Surface::RationalSpline {
        u_knots: vec![0., 0., 0., 1., 1., 2., 2., 3., 3., 4., 4., 4.],
        v_knots: vec![0., 0., h, h],
        homogeneous: ring
            .iter()
            .map(|p| vec![[p[0], p[1], 0.0, p[2]], [p[0], p[1], h * p[2], p[2]]])
            .collect(),
    };
    // Arc `q` is the surface's own circle over u in [q + 0.5, q + 1.5], found by halving its
    // spans, so each arc of the last crosses nothing but the last, which crosses the cut at its
    // middle. The edge's parameter is an unrelated affine image of u, as a file's would be.
    let average =
        |a: [f64; 3], b: [f64; 3]| -> [f64; 3] { std::array::from_fn(|d| 0.5 * (a[d] + b[d])) };
    let at = |i: usize| ring[i % 8];
    let net = |q: usize| {
        let m = q + 1;
        let (p0, p1, p2, p3, p4) = (
            at(2 * m - 2),
            at(2 * m - 1),
            at(2 * m),
            at(2 * m + 1),
            at(2 * m + 2),
        );
        let (left, right) = (average(p1, p2), average(p2, p3));
        [
            average(average(p0, p1), left),
            left,
            p2,
            right,
            average(right, average(p3, p4)),
        ]
    };
    let interval = |_: usize| [0.5, 23.0 / 54.0 + 0.5];
    let arc = |q: usize, z: f64| {
        let [t0, t1] = interval(q);
        let tm = 0.5 * (t0 + t1);
        Curve::RationalSpline {
            knots: vec![t0, t0, t0, tm, tm, t1, t1, t1],
            homogeneous: net(q).map(|p| [p[0], p[1], z * p[2], p[2]]).to_vec(),
        }
    };
    // Records number edges and vertices in first-appearance order: the bottom rim's arcs
    // 0..4 in loop order, then the top rim's, which runs against u (arc 3 first).
    let edge_of = |q: usize, top: bool| if top { 7 - q } else { q };
    let vertex_of = |q: usize, top: bool| if top { [5, 7, 6, 4][q % 4] } else { q % 4 };
    let vertices = (0..8)
        .map(|i| {
            let (q, top) = if i < 4 {
                (i, false)
            } else {
                ([3, 0, 2, 1][i - 4], true)
            };
            let p = net(q)[0];
            Vertex {
                point: [p[0] / p[2], p[1] / p[2], if top { h } else { 0.0 }],
                tolerance: 1e-6,
            }
        })
        .collect();
    let mut edges: Vec<Edge> = Vec::new();
    for i in 0..8 {
        let (top, q) = if i < 4 { (false, i) } else { (true, 7 - i) };
        edges.push(Edge {
            curve: arc(q, if top { h } else { 0.0 }),
            nurbs: None,
            interval: interval(q),
            vertices: [vertex_of(q, top) as u32, vertex_of(q + 1, top) as u32],
            tolerance: 1e-6,
            uses: vec![EdgeUseRef {
                face: 0,
                r#loop: u32::from(top),
                position: if top { i - 4 } else { i } as u32,
                upper: top,
            }],
        });
    }
    let used = |q: usize, top: bool| {
        let [t0, t1] = interval(q);
        let v = if top { h } else { 0.0 };
        OrientedEdge {
            edge: edge_of(q, top) as u32,
            forward: !top,
            pcurve: Curve2::Spline {
                knots: vec![t0, t0, t1, t1],
                points: vec![[q as f64 + 0.5, v], [q as f64 + 1.5, v]],
            },
        }
    };
    // The bottom rim runs along u, the top rim against it, the wall on the left of both.
    let bottom = (0..4).map(|q| used(q, false)).collect();
    let top = (0..4).rev().map(|q| used(q, true)).collect();
    let source = Record {
        version: VERSION,
        tolerance: 1e-6,
        regions: vec![Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: Vec::new(),
        }],
        faces: vec![Face {
            surface,
            nurbs: None,
            domain: [[0.0, 4.0], [0.0, h]],
            loops: vec![Loop::Edges { edges: bottom }, Loop::Edges { edges: top }],
        }],
        edges,
        vertices,
    };
    // (Not `agreement`: a rim lifted round the whole period jumps by it where its loop closes,
    // which is what the import's completion removes.)
    crate::record_check::validate(&source).unwrap();
    let (completed, origins) = crate::pcurve::complete_periodic_seams(&source)
        .unwrap()
        .unwrap();
    assert!(origins.iter().all(|&face| face == 0));
    for face in &completed.faces {
        for l in &face.loops {
            let Loop::Edges { edges } = l else { continue };
            for u in edges {
                let edge = &completed.edges[u.edge as usize];
                let span = u.pcurve.interval().unwrap();
                assert!(
                    (span[0] - edge.interval[0]).abs() < 1e-9
                        && (span[1] - edge.interval[1]).abs() < 1e-9,
                    "edge {}: its trim runs over {span:?}, the edge over {:?}",
                    u.edge,
                    edge.interval
                );
            }
        }
    }
    crate::record_check::validate(&completed).unwrap();
    crate::record_check::agreement(&completed).unwrap();
    assert!(crate::pcurve::import_check_failure(&completed).is_none());
    // The completed record is the kernel's own, so it is not held to the item cap for input:
    // the budget is the file's, set where the file is read. A record past that cap may still
    // disagree with its geometry, but its size is not the reason it is refused.
    let mut large = completed.clone();
    large.edges[0].curve = Curve::Polyline {
        points: (0..solidcore_record::brep::MAX_ITEMS)
            .map(|i| [i as f64 * 1e-6, 0., 0.])
            .collect(),
    };
    assert_eq!(large.validate().map_err(|e| e.0), Err("item budget"));
    let why = crate::pcurve::import_check_failure(&large).unwrap_or_default();
    assert!(!why.contains("the completed record is invalid"), "{why}");
    // A completion that leaves a trim over only the first half of its edge's interval, as
    // rebuilding a split edge's trim by sampling did for a real model, is refused by the
    // record's validity check, so the import keeps the record it had.
    let mut short = completed.clone();
    let Loop::Edges { edges } = &mut short.faces[0].loops[0] else {
        panic!("the wall's first loop is an edge loop");
    };
    let (Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. }) =
        &mut edges[0].pcurve;
    let (first, last) = (knots[0], knots[knots.len() - 1]);
    for t in knots.iter_mut().filter(|t| **t == last) {
        *t = 0.5 * (first + last);
    }
    assert!(crate::record_check::validate(&short).is_err());
    assert!(crate::pcurve::import_check_failure(&short).is_some());
    // A completion that is used says nothing. One the checks refuse is named, so the import
    // can tell a reader why the body kept its imported trims.
    let (used, skipped) = crate::pcurve::complete_periodic_seams_noted(&source).unwrap();
    assert!(used.is_some() && skipped.is_none());
    let why = crate::pcurve::import_check_failure(&short).unwrap();
    assert!(why.contains("the completed record is invalid"), "{why}");
    assert!(crate::pcurve::import_check_failure(&completed).is_none());
    // A trim that starts a rounding error off its edge's interval is moved onto it.
    let mut rounded = completed.clone();
    let Loop::Edges { edges } = &mut rounded.faces[0].loops[0] else {
        panic!("the wall's first loop is an edge loop");
    };
    let edge_interval = completed.edges[edges[0].edge as usize].interval;
    let (Curve2::Spline { knots, .. } | Curve2::RationalSpline { knots, .. }) =
        &mut edges[0].pcurve;
    for t in knots.iter_mut().filter(|t| **t == edge_interval[0]) {
        *t += 5.5e-17;
    }
    assert_ne!(knots[0], edge_interval[0]);
    crate::pcurve::snap_trim_ends(&mut rounded);
    assert_eq!(rounded, completed);
    let (body, _) = Body::from_record(&completed, true).unwrap();
    let check = body.check(CheckLevel::All).unwrap();
    assert!(check.is_valid(), "{check}");
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

/// The public reducer must carry the new Core orientation refusal through Ops,
/// rather than flattening its typed cause into an unexplained `SM_ERR` status.
#[test]
fn mesh_reduction_preserves_the_uncertified_orientation_reason() {
    use solidcore::error::mass::MassFailure;
    let positions = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.], [0.5, 0.5, 0.]];
    // Four nondegenerate coplanar facets form a zero-volume tetrahedral incidence;
    // the fifth record repeats the first, so this is only the new duplicate path.
    let indices = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3, 0, 2, 1];
    let error = crate::reduce_mesh(&positions, &indices, None, 4).unwrap_err();
    assert_eq!(
        error,
        Error::Refused {
            operation: "mesh.reduce",
            reason: MassFailure::MESH_VOLUME_SIGN_UNCERTAIN_REASON,
        }
    );
    assert_eq!(
        error.to_string(),
        format!(
            "mesh.reduce: {}",
            MassFailure::MESH_VOLUME_SIGN_UNCERTAIN_REASON
        )
    );
    // Existing mass failures still retain their old status-only policy.
    let other = solidcore::error::Failure::new("mass::op", MassFailure::Integrand).into_error();
    assert_eq!(
        crate::error::status(other),
        Error::Status(solidcore::error::SM_ERR)
    );
}
