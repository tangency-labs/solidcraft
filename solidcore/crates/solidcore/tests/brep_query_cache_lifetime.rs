//! WR-732 (#2503): with `UpstreamFixes::brep_ray_classifies_current_trims`, a B-rep
//! point classification classifies against each face's current trims, also while the
//! B-rep is being edited. `SmTrimSrfCache::BuildTree` implants no trims while editing
//! is on, so upstream counts a face it builds a cache for then as its untrimmed
//! surface, and answers right only when it reuses a cache it built before the edit.
//! The rays build their caches afresh, so once editing was on (the fillet session's
//! trim step, #2365) points inside the bar classified as outside. With the flag the
//! ray's tree of a face whose loops close is built against the face as it is,
//! whatever the editing flag; a face whose loops are open mid-edit is refused (#2624).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::ObjectRef;
use solidcore::intersect::point_class::PointClassType;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::query::brep::BrepQuery;
use solidcore::query::face::p2::note_face_surface_cache;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::make_vertex_loop;
use solidcore::topology::brep::p5::{make_edge_in_face, EdgeInFace};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::FaceId;
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// Notification keys for the edits.
fn key_of(entity: EntityRef) -> ObjectRef {
    let (type_code, index) = match entity {
        EntityRef::Face(f) => (1, f.index()),
        EntityRef::Edge(e) => (2, e.index()),
        EntityRef::Edgeuse(e) => (3, e.index()),
        EntityRef::Vertex(v) => (4, v.index()),
        EntityRef::Vertexuse(v) => (5, v.index()),
        EntityRef::Loop(l) => (6, l.index()),
        EntityRef::Loopuse(l) => (7, l.index()),
        EntityRef::Faceuse(f) => (8, f.index()),
        EntityRef::Shell(s) => (9, s.index()),
        EntityRef::Region(r) => (10, r.index()),
        EntityRef::Brep(_) => (11, 0),
    };
    ObjectRef {
        type_code: 0,
        key: ObjectKey(type_code * 1_000_000 + u64::from(index)),
    }
}

/// A kernel with the WR-732 fix on.
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.brep_ray_classifies_current_trims = true;
    k
}

/// Points in the bar, in the post, and outside both.
const POINTS: [[f64; 3]; 6] = [
    [5.0, 4.0, 3.15],
    [20.0, 8.0, 5.0],
    [40.0, 8.0, 15.0],
    [44.0, 8.0, 5.0],
    [-1.0, 8.0, 5.0],
    [20.0, 8.0, 11.0],
];

/// The bar running into the post (#2365): a 40 x 16 x 10 bar joined to an R8 x 20
/// cylinder about (40, 8).
fn bar_into_post(k: &mut Kernel) -> Brep {
    let bar = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 16.0, 10.0).unwrap();
    let post = create_cylinder(k, Point3::new(40.0, 8.0, 0.0), 8.0, 20.0).unwrap();
    boolean_union(k, bar, post).unwrap()
}

/// Whether each of `points` classifies to a material region of `b`.
fn in_material_at(k: &Kernel, b: &mut Brep, points: &[[f64; 3]]) -> Vec<bool> {
    points
        .iter()
        .map(|&[x, y, z]| {
            let pc = BrepQuery::of_brep(b)
                .point_3d_classify(k, b, Point3::new(x, y, z), 1e-6, false)
                .unwrap();
            pc.point_class() == PointClassType::Region
                && match pc.object() {
                    Some(EntityRef::Region(r)) => b.region(r).unwrap().is_void() == 0,
                    _ => false,
                }
        })
        .collect()
}

#[test]
fn classification_is_unchanged_once_editing_is_on() {
    let mut k = kernel();
    let mut b = bar_into_post(&mut k);
    let expected = [true, true, true, true, false, false];
    assert_eq!(in_material_at(&k, &mut b, &POINTS), expected, "editing off");
    b.set_editing_enabled(true);
    assert_eq!(in_material_at(&k, &mut b, &POINTS), expected, "editing on");
    b.set_editing_enabled(false);
    assert_eq!(
        in_material_at(&k, &mut b, &POINTS),
        expected,
        "editing off again"
    );
}

/// The face a point on the boundary of `b` classifies to.
fn face_at(k: &Kernel, b: &mut Brep, p: Point3) -> Result<Option<FaceId>, SolidError> {
    let pc = BrepQuery::of_brep(b).point_3d_classify(k, b, p, 1e-6, true)?;
    Ok(match (pc.point_class(), pc.object()) {
        (PointClassType::Face, Some(EntityRef::Face(f))) => Some(f),
        _ => None,
    })
}

/// A face edited mid-session classifies against its new trims: a circle made in a
/// sphere's face while the sphere is edited cuts a cap off it. Upstream builds both
/// faces' trees untrimmed then (a planar face is classified by its loops, so the test
/// needs a curved one), and gives a point of the cap to whichever face the solve meets
/// first.
#[test]
fn a_face_edited_mid_session_classifies_against_its_new_trims() {
    let mut k = kernel();
    let mut b = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let in_cap = Point3::new(-5.0, 0.0, 0.0);
    let off_cap = Point3::new(0.0, -5.0, 0.0);
    let sphere = face_at(&k, &mut b, off_cap)
        .unwrap()
        .expect("on the sphere");
    assert_eq!(face_at(&k, &mut b, in_cap).unwrap(), Some(sphere));

    b.set_editing_enabled(true);
    // the circle x = -4, radius 3, on the sphere and away from its seam and poles
    let circle = Ellipse::new_circle(
        &k,
        Point3::new(-4.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Extent1d::from_min_max(0.0, 360.0),
        3.0,
        &EllipseOptions::default(),
    )
    .unwrap();
    let interval = Curve::from(circle.clone()).natural_interval();
    // the circle's vertex, where it starts and ends
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let (_, vertex) = make_vertex_loop(
        &mut b,
        sphere,
        Point3::new(-4.0, 3.0, 0.0),
        OrientType::Opposite,
        &mut env,
    )
    .unwrap();
    let mut made = EdgeInFace::default();
    make_edge_in_face(
        &mut k,
        &mut b,
        true,
        Some(sphere),
        Some(vertex),
        Some(vertex),
        Curve::from(circle),
        None,
        interval,
        OrientType::Same,
        1e-6,
        &mut made,
        None,
    )
    .unwrap();
    let cap = made.face.expect("the circle cuts a cap off the sphere");
    assert_ne!(cap, sphere);

    // still editing: each point is on the face its trims now hold it in
    assert_eq!(
        face_at(&k, &mut b, in_cap).unwrap(),
        Some(cap),
        "in the cap"
    );
    assert_eq!(
        face_at(&k, &mut b, off_cap).unwrap(),
        Some(sphere),
        "off it"
    );
    b.set_editing_enabled(false);
    assert_eq!(
        face_at(&k, &mut b, in_cap).unwrap(),
        Some(cap),
        "editing off"
    );
    assert_eq!(
        face_at(&k, &mut b, off_cap).unwrap(),
        Some(sphere),
        "editing off"
    );
}

/// While editing, a face whose loop does not close has no boundary to implant: with the
/// fix, a classification whose ray meets it is refused naming the face, not answered
/// against its untrimmed surface.
#[test]
fn an_open_loop_mid_edit_is_refused_naming_its_face() {
    let broken = |k: &mut Kernel| {
        // (the stitch that builds it classifies mid-glue, where an edge use can start
        // at a vertex still being merged into its edge's: those loops close)
        let mut b = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 1.5, 10.0).unwrap();
        // in the side face, the first edge use starts where the second does: its edge
        // then ends away from the second's start, and the loop no longer closes
        let mut opened = Vec::new();
        let faces: Vec<FaceId> = b.faces().ids().collect();
        for f in faces {
            let lu = solidcore::topology::face::p4::upward_loopuses(&b, f, false, None).unwrap()[0];
            let eus = solidcore::topology::loopuse::edgeuses(&b, lu, None, None).unwrap();
            if let [first, second, ..] = eus[..] {
                let start = b.edgeuse(second).unwrap().vertexuse;
                b.edgeuse_mut(first).unwrap().vertexuse = start;
                opened.push(f);
            }
        }
        let [side] = opened[..] else {
            panic!("the side face alone, not {opened:?}");
        };
        b.set_editing_enabled(true);
        (b, side)
    };
    let mut k = kernel();
    let (mut b, side) = broken(&mut k);
    // the region's ray fires and the boundary solve
    for boundaries in [false, true] {
        let p = Point3::new(0.0, 0.0, 5.0);
        let refused = BrepQuery::of_brep(&b).point_3d_classify(&k, &mut b, p, 1e-6, boundaries);
        assert!(
            matches!(
                refused,
                Err(SolidError::FaceTrimsUnavailable { entity: EntityRef::Face(f), .. }) if f == side
            ),
            "{refused:?}"
        );
    }
}

/// Mid-edit, an edge's cached end pair (`Edge::vertices`, refreshed only when a public
/// operation returns) can name a vertex the edit has merged away, while its edge uses
/// already name the survivor. Loop closure reads the ends through the uses, as
/// upstream's `SmEdge::GetVertices` does, so such a face still closes and classifies
/// against its current trims (#2624: the stitch inside a fillet's finish fires its
/// region rays at such faces).
#[test]
fn a_stale_edge_end_pair_mid_edit_still_closes_its_faces() {
    let mut k = kernel();
    let mut b = bar_into_post(&mut k);
    let expected = [true, true, true, true, false, false];
    assert_eq!(in_material_at(&k, &mut b, &POINTS), expected, "editing off");
    b.set_editing_enabled(true);
    // a vertex merged away mid-edit: every edge's cached pair still names it
    let merged = b
        .add_vertex_record(solidcore::topology::Vertex::default())
        .unwrap();
    b.remove_vertex_record(merged).unwrap();
    let edges: Vec<_> = b.edges().ids().collect();
    for e in edges {
        b.edge_mut(e).unwrap().vertices = [merged, merged];
    }
    assert_eq!(
        in_material_at(&k, &mut b, &POINTS),
        expected,
        "editing on, stale end pairs"
    );
}

/// `SmBSplineSurface::Transform` notifies `SM_NO_PRE_EDIT`, and `SmSurface::Notify`
/// deletes the surface's trimmed cache (`SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF)`),
/// so a face classification after the move does not reuse a cache of where the face
/// was.
#[test]
fn a_moved_body_drops_its_trimmed_caches() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 1.5, 10.0).unwrap();
    let faces: Vec<FaceId> = b.faces().ids().collect();
    for &f in &faces {
        note_face_surface_cache(&k, &mut b, f).unwrap();
    }
    let with_cache = |b: &Brep| {
        faces
            .iter()
            .filter(|&&f| {
                b.face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .is_some_and(Surface::has_trim_cache)
            })
            .count()
    };
    assert_eq!(with_cache(&b), faces.len(), "a face lookup keeps the cache");
    api_rotate(
        &mut k,
        Some(TransformObject::Brep(&mut b)),
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        90.0,
    )
    .unwrap();
    assert_eq!(
        with_cache(&b),
        0,
        "a transformed surface keeps a trimmed cache"
    );
}
