//! Issue #1462 (P4): caller attributes on a B-rep's faces and edges reach the result of
//! an offset or a shell as SMLib's notifications carry them. Each offset face is a COPY
//! of its source face's attributes (`SmOffsetExecutive.cpp:1237`); a shell's outer faces
//! are the source's own faces (the difference edits them, and the Edit handler is a
//! no-op), its inner faces offset copies of them.
//!
//! Each source face and edge gets its own attribute id, so a result object's origins are
//! its attribute ids.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

use solidcore::geom::attribute::{Attribute, Behavior};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{
    offset_brep_simple, shell_brep_simple, OffsetOptions, ShellOptions,
};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::face::p2::face_vertices;
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel};

const FACE_BASE: u64 = 10007;
const EDGE_BASE: u64 = 10507;

fn tag(b: &mut Brep, behavior: Behavior) {
    let faces = b.face_list().to_vec();
    for (i, f) in faces.into_iter().enumerate() {
        let id = FACE_BASE + i as u64;
        b.add_attribute(EntityRef::Face(f), Attribute::long(id, id as i64, behavior))
            .expect("tag face");
    }
    let edges = b.edge_list().to_vec();
    for (i, e) in edges.into_iter().enumerate() {
        let id = EDGE_BASE + i as u64;
        b.add_attribute(EntityRef::Edge(e), Attribute::long(id, id as i64, behavior))
            .expect("tag edge");
    }
}

fn ids(b: &Brep, e: EntityRef) -> Vec<u64> {
    let mut out: Vec<u64> = b
        .entity_attributes(e)
        .into_iter()
        .map(|(_, a)| a.attribute_id())
        .collect();
    out.sort_unstable();
    out
}

/// Per face: (the face's centre, rounded to 1e-6, and its attribute ids), sorted.
fn face_origins(b: &Brep) -> Vec<([i64; 3], Vec<u64>)> {
    let mut out: Vec<_> = b
        .face_list()
        .iter()
        .map(|&f| {
            let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
            for v in face_vertices(b, f, false, None).expect("face vertices") {
                let p = b.vertex(v).expect("vertex").point;
                for (k, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                    lo[k] = lo[k].min(c);
                    hi[k] = hi[k].max(c);
                }
            }
            let centre = [0, 1, 2].map(|k| ((lo[k] + hi[k]) / 2.0 * 1e6).round() as i64);
            (centre, ids(b, EntityRef::Face(f)))
        })
        .collect();
    out.sort();
    out
}

fn tagged_box(k: &mut Kernel, behavior: Behavior) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).expect("box");
    tag(&mut b, behavior);
    b
}

/// Only the caller's ids (the merge path adds the `SM_AI_SURFACEMAP` system attribute).
fn caller_ids(mut v: Vec<u64>) -> Vec<u64> {
    v.retain(|&id| id >= FACE_BASE);
    v
}

/// The box's faces: (centre, the caller's ids), as [`face_origins`].
fn box_faces(b: &Brep) -> Vec<([i64; 3], Vec<u64>)> {
    face_origins(b)
        .into_iter()
        .map(|(c, ids)| (c, caller_ids(ids)))
        .collect()
}

/// The centre of the face of a 10-box at `centre` moved `d` along its outward normal.
fn moved(centre: [i64; 3], d: f64) -> [i64; 3] {
    let step = (d * 1e6).round() as i64;
    centre.map(|c| match c {
        0 => -step,
        10_000_000 => 10_000_000 + step,
        _ => c,
    })
}

fn options(distance: f64, merge: bool) -> OffsetOptions {
    OffsetOptions {
        distance,
        extend_convex_edges: true,
        merge_results: merge,
    }
}

#[test]
fn offset_box_faces_copy_their_source_faces() {
    for merge in [false, true] {
        let mut k = Kernel::default();
        let mut b = tagged_box(&mut k, Behavior::Copy);
        let source = box_faces(&b);
        let key = b.key();
        let r = offset_brep_simple(&mut k, &mut b, options(1.0, merge)).expect("offset");
        // SmOffsetExecutive.cpp:1237: each offset face is a COPY of its source face's
        // attributes
        let mut expected: Vec<_> = source
            .iter()
            .map(|(c, ids)| (moved(*c, 1.0), ids.clone()))
            .collect();
        expected.sort();
        assert_eq!(box_faces(&r), expected, "merge_results {merge}");
        for (_, ids) in &expected {
            assert_eq!(ids.len(), 1);
        }
        // the source keeps its own attributes and identity
        assert_eq!(box_faces(&b), source);
        assert_eq!(b.key(), key);
    }
}

#[test]
fn shell_box_inner_faces_carry_their_sources() {
    for merge in [false, true] {
        let mut k = Kernel::default();
        let b = tagged_box(&mut k, Behavior::Copy);
        let source = box_faces(&b);
        // face 1 is the top (z = 10)
        let top = b.face_list()[1];
        assert_eq!(
            caller_ids(ids(&b, EntityRef::Face(top))),
            vec![FACE_BASE + 1]
        );
        let r = shell_brep_simple(
            &mut k,
            &b,
            ShellOptions {
                offset: options(-1.0, merge),
                create_offset_solid: false,
                merge_sheet_side_walls: false,
            },
            &[top],
        )
        .expect("shell");
        let faces = box_faces(&r);
        assert_eq!(faces.len(), 11, "merge_results {merge}: {faces:?}");
        for (centre, ids) in &source {
            // the outer face is the source face itself: the difference edits it (the
            // Edit handler is a no-op), so it keeps exactly its own origin; the open
            // top's rim is what remains of the top face
            assert!(
                faces.contains(&(*centre, ids.clone())),
                "merge_results {merge}: outer {centre:?} {ids:?} in {faces:?}"
            );
            if *centre == [5_000_000, 5_000_000, 10_000_000] {
                continue;
            }
            // the inner wall is the offset of the source face (a COPY); the side walls
            // reach up to the open top, so only the normal coordinate is known
            let inner = moved(*centre, -1.0);
            let found = faces.iter().any(|(c, got)| {
                got == ids
                    && (0..3).all(|axis| inner[axis] == 5_000_000 || c[axis] == inner[axis])
                    && c != centre
            });
            assert!(
                found,
                "merge_results {merge}: inner of {centre:?} {ids:?} in {faces:?}"
            );
        }
        // every face has exactly one origin
        for (centre, ids) in &faces {
            assert_eq!(ids.len(), 1, "merge_results {merge}: {centre:?}");
        }
        // the source is not changed
        assert_eq!(box_faces(&b), source);
    }
}

#[test]
fn rounded_offset_edges_keep_their_sources_through_the_stitch() {
    let mut k = Kernel::default();
    let mut b = tagged_box(&mut k, Behavior::Copy);
    let r = offset_brep_simple(
        &mut k,
        &mut b,
        OffsetOptions {
            distance: 1.0,
            extend_convex_edges: false,
            merge_results: false,
        },
    )
    .expect("offset");
    // each source edge's offset edges on both of its faces' offsets (sm_NotifyEdgeAndVertices
    // at SmOffsetExecutive.cpp:3066) keep its id through the stitch that glues them to the
    // rounding faces: every one of the 12 ids on exactly 2 edges, as the C++ reference
    let mut count = std::collections::BTreeMap::new();
    for &e in r.edge_list() {
        for id in caller_ids(ids(&r, EntityRef::Edge(e))) {
            *count.entry(id).or_insert(0) += 1;
        }
    }
    let expected: std::collections::BTreeMap<u64, i32> =
        (0..12).map(|i| (EDGE_BASE + i, 2)).collect();
    assert_eq!(count, expected);
}
