//! Issue #1462 (P5): caller attributes on B-rep objects survive the split family as
//! SMLib's default handlers carry them: a split face's (edge's) pieces each keep the
//! attribute (`SM_NO_SPLIT_IN_BREP`), a merge gives the survivor the ids it lacks, and
//! a copy passes copy attributes on.
//!
//! One case per `SM_API` / `SmBrep` call SolidCraft's split family uses: `SM_BO_SLICE`
//! and `SM_BO_IMPRINT` through `SmApiBooleanWithOptions` (the latter after
//! `SmApiBrepCopy`), `SmBrep::LocalOperation`, `SmBrep::StitchAndOrient` and
//! `SmBrep::RemoveTopologicalEdgesAndVertices`, plus the design's imprint of a box by a
//! plate.

#![allow(clippy::expect_used, clippy::indexing_slicing)] // test helpers

use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::p1::MergeOptions;
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol;
use solidcore::math::{Point2, Point3, Vec3};
use solidcore::offset::offset_executive::p2::OffsetNotifications;
use solidcore::primitive::api_primitives::{create_box, create_plane};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::brep::{p10, p2, p3, p5, p7, p8, p9};
use solidcore::topology::topology::{NotifyRecords, Topology};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{edge, vertex, Brep, BrepId, EdgeId, FaceId, RegionId};
use solidcore::{EntityRef, Kernel};

const BASE_ID: u64 = 20011;

/// `(operand, kind, index)`: kind 0 face, 1 edge.
type Origin = (u64, u64, u64);

fn origin_id(operand: u64, kind: u64, index: u64) -> u64 {
    BASE_ID + operand * 1000 + kind * 500 + index
}

fn decode(id: u64) -> Origin {
    let raw = id - BASE_ID;
    (raw / 1000, raw % 1000 / 500, raw % 500)
}

fn tag(b: &mut Brep, operand: u64) {
    let faces = b.face_list().to_vec();
    for (i, f) in faces.into_iter().enumerate() {
        let id = origin_id(operand, 0, i as u64);
        b.add_attribute(
            EntityRef::Face(f),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag face");
    }
    let edges = b.edge_list().to_vec();
    for (i, e) in edges.into_iter().enumerate() {
        let id = origin_id(operand, 1, i as u64);
        b.add_attribute(
            EntityRef::Edge(e),
            Attribute::long(id, id as i64, Behavior::Copy),
        )
        .expect("tag edge");
    }
}

fn origins(b: &Brep, e: EntityRef) -> Vec<Origin> {
    let mut out: Vec<_> = b
        .entity_attributes(e)
        .into_iter()
        .filter(|(_, a)| a.attribute_id() >= BASE_ID)
        .map(|(_, a)| decode(a.attribute_id()))
        .collect();
    out.sort_unstable();
    out
}

fn tagged_box(k: &mut Kernel, size: f64) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), size, size, size).expect("box");
    tag(&mut b, 0);
    b
}

/// How many result faces carry each of the box's six face origins; every face carries
/// at most one face origin, of operand 0.
fn pieces_per_box_face(r: &Brep) -> [usize; 6] {
    let mut count = [0; 6];
    for &f in r.face_list() {
        let o = origins(r, EntityRef::Face(f));
        assert!(o.len() <= 1, "face {f:?} has origins {o:?}");
        if let Some(&(operand, kind, index)) = o.first() {
            assert_eq!((operand, kind), (0, 0), "face {f:?}: {o:?}");
            count[index as usize] += 1;
        }
    }
    count
}

/// The plate `[-5, 15]^2 x [4, 6]` (a solid) or the sheet `[-5, 15]^2` at `z = 5`.
fn cutter(k: &mut Kernel, solid: bool) -> Brep {
    if solid {
        create_box(k, Point3::new(-5.0, -5.0, 4.0), 20.0, 20.0, 2.0).expect("plate")
    } else {
        create_plane(k, Point3::new(-5.0, -5.0, 5.0), 20.0, 20.0).expect("sheet")
    }
}

/// The four side faces of the box (`GetFaces` order of `create_box`) are split, the
/// bottom and top are not.
fn assert_sides_split(count: [usize; 6]) {
    let mut sorted = count;
    sorted.sort_unstable();
    assert_eq!(sorted[..2], [1, 1], "{count:?}");
    assert!(sorted[2..].iter().all(|&n| n >= 2), "{count:?}");
}

#[test]
fn slice_by_a_sheet_keeps_the_origins_of_the_trimmed_faces() {
    // SmApiBooleanWithOptions(SM_BO_SLICE) of the box by the sheet at z = 5: the four
    // side faces are split there and the pieces above kept, each with its face's
    // origin; the top keeps its own, and the cap is the sheet's piece, with its origin
    let mut k = Kernel::default();
    let a = tagged_box(&mut k, 10.0);
    let mut b = cutter(&mut k, false);
    tag(&mut b, 1);
    let out = boolean_with_options(
        &mut k,
        a,
        b,
        BooleanOperation::Slice,
        MergeOptions::default(),
    )
    .expect("slice");
    let r = &out.result;
    let mut faces: Vec<Vec<Origin>> = r
        .face_list()
        .iter()
        .map(|&f| origins(r, EntityRef::Face(f)))
        .collect();
    faces.sort();
    // `create_box` lists the bottom (z = 0) first
    assert_eq!(
        faces,
        [
            vec![(0, 0, 1)],
            vec![(0, 0, 2)],
            vec![(0, 0, 3)],
            vec![(0, 0, 4)],
            vec![(0, 0, 5)],
            vec![(1, 0, 0)],
        ]
    );
}

#[test]
fn imprint_of_a_box_by_a_plate_keeps_the_origin_on_both_pieces() {
    // the design's test: SM_BO_IMPRINT splits each side face of the box at z = 4 and 6
    let mut k = Kernel::default();
    let a = tagged_box(&mut k, 10.0);
    let b = cutter(&mut k, true);
    let out = boolean_with_options(
        &mut k,
        a,
        b,
        BooleanOperation::Imprint,
        MergeOptions::default(),
    )
    .expect("imprint");
    let count = pieces_per_box_face(&out.result);
    assert_sides_split(count);
    // every result face is a piece of a box face
    assert_eq!(count.iter().sum::<usize>(), out.result.face_list().len());
}

#[test]
fn imprint_of_a_copy_keeps_the_copied_origins() {
    // SmApiBrepCopy, then SM_BO_IMPRINT of the copy: the copy carries the source's
    // copy attributes, and the imprint splits them onto both pieces
    let mut k = Kernel::default();
    let source = tagged_box(&mut k, 10.0);
    let mut state = p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    let target_key = |entity| copy_key(entity, 1 << 40);
    env.key_of = &target_key;
    let copy =
        p3::api_brep_copy(&mut k, &mut env, Some(&source), &|e| copy_key(e, 0)).expect("copy");
    let b = cutter(&mut k, false);
    let out = boolean_with_options(
        &mut k,
        copy,
        b,
        BooleanOperation::Imprint,
        MergeOptions::default(),
    )
    .expect("imprint");
    assert_sides_split(pieces_per_box_face(&out.result));
}

/// A caller identity outside the kernel's reserved key range (`domain` apart).
fn copy_key(entity: EntityRef, domain: u64) -> ObjectRef {
    let reference = p3::topology_ref(0, entity);
    ObjectRef {
        type_code: reference.type_code,
        key: ObjectKey(
            domain + reference.type_code as u64 * 1_000_000 + (reference.key.0 & 0xffff_ffff),
        ),
    }
}

/// Throwaway offset notification stores keyed by entity id (the
/// `topology.brep_local_operation` runner's).
fn with_notifications<R>(
    b: &mut Brep,
    f: impl FnOnce(&mut Brep, &mut OffsetNotifications<'_>) -> R,
) -> R {
    fn key(entity: EntityRef, namespace: u64) -> ObjectRef {
        let (index, type_code) = match entity {
            EntityRef::Brep(id) => (id.index(), solidcore::geom::a_object::BREP_TYPE),
            EntityRef::Region(id) => (id.index(), solidcore::topology::Region::type_code()),
            EntityRef::Shell(id) => (id.index(), solidcore::topology::Shell::type_code()),
            EntityRef::Face(id) => (id.index(), solidcore::topology::face::p2::TYPE),
            EntityRef::Faceuse(id) => (id.index(), 16022),
            EntityRef::Loopuse(id) => (id.index(), solidcore::topology::loopuse::TYPE),
            EntityRef::Edge(id) => (id.index(), edge::TYPE),
            EntityRef::Edgeuse(id) => (id.index(), solidcore::topology::edgeuse::p1::TYPE),
            EntityRef::Vertex(id) => (id.index(), vertex::TYPE),
            EntityRef::Vertexuse(id) => (id.index(), solidcore::topology::vertexuse::TYPE),
            EntityRef::Loop(id) => (id.index(), solidcore::topology::topology::TYPE),
        };
        ObjectRef {
            key: ObjectKey((namespace << 60) | ((type_code as u64) << 32) | u64::from(index)),
            type_code,
        }
    }
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let root = EntityRef::Brep(BrepId::from_index(0));
    let mut entities = vec![root];
    entities.extend(b.regions().iter().map(|(id, _)| EntityRef::Region(id)));
    entities.extend(b.shells().iter().map(|(id, _)| EntityRef::Shell(id)));
    entities.extend(b.faces().iter().map(|(id, _)| EntityRef::Face(id)));
    entities.extend(b.faceuses().iter().map(|(id, _)| EntityRef::Faceuse(id)));
    entities.extend(b.loopuses().iter().map(|(id, _)| EntityRef::Loopuse(id)));
    entities.extend(b.edges().iter().map(|(id, _)| EntityRef::Edge(id)));
    entities.extend(b.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)));
    entities.extend(b.vertices().iter().map(|(id, _)| EntityRef::Vertex(id)));
    entities.extend(
        b.vertexuses()
            .iter()
            .map(|(id, _)| EntityRef::Vertexuse(id)),
    );
    for entity in entities {
        records.insert(key(entity, 1).key, Topology::new());
    }
    let result_brep = key(root, 3);
    records.insert(result_brep.key, Topology::new());
    let original = |e| key(e, 1);
    let result = |e| key(e, 3);
    let original_curve = |e: EdgeId, t: i64| ObjectRef {
        key: ObjectKey((5_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
        type_code: t,
    };
    let result_curve = |e: EdgeId, t: i64| ObjectRef {
        key: ObjectKey((4_u64 << 60) | ((t as u64) << 32) | u64::from(e.index())),
        type_code: t,
    };
    let lists = |_: bool, region: RegionId| {
        [0_u64, 1].map(|list| ObjectRef {
            key: ObjectKey((6_u64 << 60) | (list << 32) | u64::from(region.index())),
            type_code: 0,
        })
    };
    let mut notifications = OffsetNotifications {
        records: &mut records,
        attributes: &mut attributes,
        original_key_of: &original,
        result_key_of: &result,
        original_curve_ref: &original_curve,
        result_curve_ref: &result_curve,
        boolean_source_lists: &lists,
        result_brep,
        merge_identities: None,
    };
    f(b, &mut notifications)
}

#[test]
fn local_operation_keeps_every_face_origin() {
    // SmBrep::LocalOperation: the top face of a 2-box gets a raised plane; every face is
    // rebuilt by the extended offset (COPY of its source) and copied back into the
    // B-rep (CopyFaces), so each keeps its own origin
    let mut k = Kernel::default();
    let mut b = tagged_box(&mut k, 2.0);
    let top = b
        .face_list()
        .iter()
        .copied()
        .find(|&f| {
            let s = solidcore::topology::face::p2::surface(&b, f)
                .expect("surface")
                .expect("some");
            let p = s
                .evaluate_point(&k, b.face(f).expect("face").uv_domain.evaluate(0.5, 0.5))
                .expect("point");
            (p.z - 2.0).abs() < 1e-9
        })
        .expect("top");
    let raised = Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, 3.0), Vec3::Z));
    with_notifications(&mut b, |b, notes| {
        local_operation(&mut k, b, &[top], vec![raised], notes)
    })
    .expect("local operation");
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(pieces_per_box_face(&b), [1; 6]);
}

/// Planar sheet faces on one plane over the UV boxes `domains`, in one B-rep, each
/// made by `SmBrep::CreateFaceFromSurface` (unstitched: every face has its own edges).
fn sheet_faces(k: &mut Kernel, domains: &[[f64; 4]]) -> (Brep, Vec<FaceId>) {
    let mut b = p3::new_brep(k, tol::USE_DEFAULT, tol::USE_DEFAULT).expect("brep");
    b.set_editing_enabled(true);
    let mut faces: Vec<FaceId> = Vec::new();
    for &[u0, v0, u1, v1] in domains {
        let face = p3::with_kernel_callbacks(k, &mut b, |k, b, env| {
            let key_of = env.key_of;
            let this_face = |f: FaceId| key_of(EntityRef::Face(f));
            let mut marks = Marks::default();
            let mut fenv = p8::FaceBuildEnv {
                env,
                marks: &mut marks,
                this_face: &this_face,
            };
            let plane = Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, 0.0), Vec3::Z));
            let domain = Extent2d::from_points(Point2::new(u0, v0), Point2::new(u1, v1));
            p8::create_face_from_surface(k, b, Some(plane), &domain, &mut fenv)
        })
        .expect("face");
        faces.push(face);
    }
    b.refresh_cached_topology();
    assert_eq!(b.edge_list().len(), 4 * domains.len());
    (b, faces)
}

/// The sorted origin counts of `b`'s edges, after checking that face `i` of `faces`
/// still carries exactly its own origin.
fn stitched_edge_origins(b: &Brep, faces: &[FaceId]) -> Vec<Vec<Origin>> {
    for (i, &f) in faces.iter().enumerate() {
        assert_eq!(origins(b, EntityRef::Face(f)), [(0, 0, i as u64)]);
    }
    let mut out: Vec<_> = b
        .edge_list()
        .iter()
        .map(|&e| origins(b, EntityRef::Edge(e)))
        .collect();
    out.sort();
    out
}

#[test]
fn stitch_and_orient_unions_the_glued_edges() {
    // SmBrep::StitchAndOrient of two adjacent sheet faces of one B-rep: the stitch glues
    // their coincident edges, and the edge that survives gains the other's origin
    let mut k = Kernel::default();
    let (mut b, faces) = sheet_faces(&mut k, &[[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]]);
    tag(&mut b, 0);
    p5::stitch_and_orient(&mut k, &mut b, &p5::StitchAndOrientOptions::default()).expect("stitch");
    let edges = stitched_edge_origins(&b, &faces);
    assert_eq!(edges.len(), 7);
    let mut counts: Vec<usize> = edges.iter().map(Vec::len).collect();
    counts.sort_unstable();
    assert_eq!(counts, [1, 1, 1, 1, 1, 1, 2]);
}

#[test]
fn stitch_and_orient_splits_a_long_edge_onto_both_pieces() {
    // a T-junction: face 0's right edge spans the left edges of faces 1 and 2, so the
    // stitch splits it at their common vertex (MakeVertexSplitEdge) before gluing; both
    // pieces keep its origin, each glued to one of the short edges
    let mut k = Kernel::default();
    let (mut b, faces) = sheet_faces(
        &mut k,
        &[
            [0.0, 0.0, 0.5, 1.0],
            [0.5, 0.0, 1.0, 0.5],
            [0.5, 0.5, 1.0, 1.0],
        ],
    );
    tag(&mut b, 0);
    // the long edge through the middle of the sheet (its midpoint is the centre)
    let ends = |e| {
        let (s, t) = edge::start_end_vertices(&b, e).expect("ends");
        let p = vertex::point(&b, s.expect("start")).expect("p");
        let q = vertex::point(&b, t.expect("end")).expect("q");
        (p, q)
    };
    let centre = Point3::new(0.0, 0.0, 0.0)
        + b.edge_list()
            .iter()
            .map(|&e| ends(e).0 - Point3::new(0.0, 0.0, 0.0))
            .fold(Vec3::new(0.0, 0.0, 0.0), |a, v| a + v)
            * (1.0 / b.edge_list().len() as f64);
    let long = b.edge_list().iter().copied().find(|&e| {
        let (p, q) = ends(e);
        (p - q).length() > 0.9 && ((p + (q - p) * 0.5) - centre).length() < 0.2
    });
    let long_origin = origins(&b, EntityRef::Edge(long.expect("long edge")));
    assert_eq!(long_origin.len(), 1);
    p5::stitch_and_orient(&mut k, &mut b, &p5::StitchAndOrientOptions::default()).expect("stitch");
    let edges = stitched_edge_origins(&b, &faces);
    let carriers = edges.iter().filter(|o| o.contains(&long_origin[0])).count();
    assert_eq!(carriers, 2, "{edges:?}");
    for o in edges.iter().filter(|o| o.contains(&long_origin[0])) {
        assert_eq!(o.len(), 2, "{o:?}");
    }
}

#[test]
fn remove_topological_vertices_unions_the_merged_edges() {
    // split an edge (both pieces keep its origin), tag the pieces apart, then
    // SmBrep::RemoveTopologicalEdgesAndVertices merges them back: the survivor carries
    // the origin and both piece tags, and every face keeps its own
    let mut k = Kernel::default();
    let mut b = tagged_box(&mut k, 1.0);
    let e = b.edge_list()[0];
    b.set_editing_enabled(true);
    let out = p3::with_kernel_callbacks(&mut k, &mut b, |k, b, env| {
        let serial = b.notify_serial().expect("serial");
        p9::make_vertex_split_edge(k, b, e, 0.5, env, &|edge, kind| ObjectRef {
            key: p3::topology_ref(serial, EntityRef::Edge(edge)).key,
            type_code: kind,
        })
    })
    .expect("split");
    let pieces = out.edges.expect("split edges");
    assert_eq!(pieces.len(), 2);
    for (i, &piece) in pieces.iter().enumerate() {
        let id = origin_id(1, 1, i as u64);
        b.add_attribute(
            EntityRef::Edge(piece),
            Attribute::long(id, 0, Behavior::Copy),
        )
        .expect("piece tag");
    }
    let edges = b.edge_list().len();
    p5::remove_topological_edges_and_vertices(&mut k, &mut b, None, None).expect("remove");
    assert_eq!(b.edge_list().len(), edges - 1);
    assert_eq!(pieces_per_box_face(&b), [1; 6]);
    let merged: Vec<_> = b
        .edge_list()
        .iter()
        .map(|&x| origins(&b, EntityRef::Edge(x)))
        .filter(|o| o.len() > 1)
        .collect();
    assert_eq!(merged, [vec![(0, 1, 0), (1, 1, 0), (1, 1, 1)]]);
}

#[test]
fn merge_curves_on_surface_splits_a_face_onto_both_pieces() {
    // SmBrep::MergeCurvesOnSurface of a line across the box's top face, an edit under
    // the kernel's identities (`with_kernel_callbacks`, which lends the B-rep's
    // attributes): the face splits (SM_NO_SPLIT_IN_BREP) and both pieces keep its origin
    let mut k = Kernel::default();
    let mut b = tagged_box(&mut k, 10.0);
    let top = b.face_list()[1];
    let line = Curve::from(
        Line::create_line_segment(
            3,
            Point3::new(-1.0, 5.0, 10.0),
            Point3::new(11.0, 5.0, 10.0),
            None,
        )
        .expect("line"),
    );
    b.set_editing_enabled(true);
    let (new_faces, _) = p3::with_kernel_callbacks(&mut k, &mut b, |k, b, env| {
        let serial = b.notify_serial().expect("serial");
        p10::merge_curves_on_surface(
            k,
            b,
            env,
            &|edge, kind| ObjectRef {
                key: p3::topology_ref(serial, EntityRef::Edge(edge)).key,
                type_code: kind,
            },
            top,
            1e-6,
            &[line],
            None,
            false,
        )
    })
    .expect("merge curves");
    assert!(!new_faces.is_empty());
    let count = pieces_per_box_face(&b);
    assert_eq!(count[1], 2, "{count:?}");
    assert_eq!(count.iter().sum::<usize>(), b.face_list().len());
}

#[test]
fn copy_faces_with_a_merge_copies_the_face_attributes() {
    // SmBrep::CopyFaces(faces, dest, bUseMerge = TRUE) into a new B-rep: each copied
    // face gets its source's copy attribute (SM_NO_COPY), in the caller's store the copy
    // notifies through, under the destination identity of the new face
    let mut k = Kernel::default();
    let mut src = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).expect("box");
    let mut dest = p3::new_brep(&mut k, tol::USE_DEFAULT, tol::USE_DEFAULT).expect("dest");
    let faces = src.face_list().to_vec();
    let mut state = p2::ArenaNotifyState::new(&src);
    // the source faces under the arena state's own identities
    let keys: Vec<ObjectKey> = {
        let env = state.env(None);
        faces
            .iter()
            .map(|&f| (env.key_of)(EntityRef::Face(f)).key)
            .collect()
    };
    for (i, &key) in keys.iter().enumerate() {
        let id = origin_id(0, 0, i as u64);
        let h = state
            .attributes
            .insert(Attribute::long(id, 0, Behavior::Copy))
            .expect("insert");
        state.attributes.add_attribute(key, h).expect("tag");
    }
    let dest_face = |f: FaceId| ObjectRef {
        type_code: solidcore::topology::face::p2::TYPE,
        key: ObjectKey((1 << 41) | u64::from(f.index())),
    };
    let (copied, all) = p7::copy_faces(
        &mut k,
        &mut src,
        &faces,
        &mut dest,
        true,
        &dest_face,
        &mut state.env(None),
    )
    .expect("copy faces");
    assert!(all);
    assert_eq!(copied.len(), 6);
    let mut ids: Vec<u64> = copied
        .iter()
        .flat_map(|&f| state.attributes.attributes(dest_face(f).key))
        .map(|h| state.attributes.get(h).expect("live").attribute_id())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (0..6).map(|i| origin_id(0, 0, i)).collect::<Vec<_>>());
}
