//! `SmBrep::SubdivideFaceAtDiscontinuities` cuts a face along every knot of its surface:
//! its V pass counts the V knots, where upstream counts the U knots (`SmBrep.cpp:18483`,
//! WR-589, fixed on the default path). The fixtures are unit-square sheets on degree-1
//! B-spline surfaces (`fixtures/topology.brep_p10/subdivide_*.smb`), subdivided at every
//! interior knot (`SM_CT_CINFINITY`). A degree-1 sheet's knots are at its control
//! points, so the cuts are the grid lines through them (closed form):
//! - one U knot and two V knots (x = 0.3; y = 0.2 and 0.7): 6 faces, where upstream
//!   cuts at the U knot and the first V knot only (4);
//! - no U knot and one V knot (y = 0.3): 2 faces, where upstream makes no cut (1);
//! - one U knot and no V knot (x = 0.3): 2 faces, where upstream reads past its V knots.
//!
//! Two upstream test files have the same shape (difftest `subdivide_surf_rev_cinf` and
//! `subdivide_nurb_nurb_2_cinf`): their bicubic sheets have more V knots than U knots,
//! and every V knot is a simple knot where the surface is C2 and its third V derivative
//! jumps. The port cuts the face along all of them; upstream leaves the V knots past the
//! U knots' count inside the faces. These tests read the files when the pinned upstream
//! checkout is present (as the difftest does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::object::ObjectRef;
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::b_spline_surface_n_lib::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::topology::brep::p1::faces;
use solidcore::topology::brep::p10::{subdivide_face_at_discontinuities, TopologyEnv};
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, FaceId};
use solidcore::{Brep, EntityRef, Kernel, SolidError};
use std::collections::BTreeSet;

/// A distinct identity per entity kind and index.
fn key_of(e: EntityRef) -> ObjectRef {
    let (kind, index) = match e {
        EntityRef::Brep(x) => (0, x.index()),
        EntityRef::Region(x) => (1, x.index()),
        EntityRef::Shell(x) => (2, x.index()),
        EntityRef::Faceuse(x) => (3, x.index()),
        EntityRef::Face(x) => (4, x.index()),
        EntityRef::Loopuse(x) => (5, x.index()),
        EntityRef::Loop(x) => (6, x.index()),
        EntityRef::Edgeuse(x) => (7, x.index()),
        EntityRef::Edge(x) => (8, x.index()),
        EntityRef::Vertexuse(x) => (9, x.index()),
        EntityRef::Vertex(x) => (10, x.index()),
    };
    ObjectRef {
        type_code: 0,
        key: ObjectKey((1u64 << 40) * (kind + 1) + u64::from(index)),
    }
}

/// What a subdivision leaves: the result, the number of faces, and the distinct x and y
/// of the vertices (to 1e-12, as integers of 1e-12).
struct Subdivided {
    result: Result<Vec<FaceId>, SolidError>,
    faces: usize,
    xs: BTreeSet<i64>,
    ys: BTreeSet<i64>,
}

/// `v` in units of 1e-12.
fn grid(v: f64) -> i64 {
    (v * 1e12).round() as i64
}

/// Reads fixture `name` and subdivides its face at every interior knot.
fn subdivide(name: &str) -> Subdivided {
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(
        &mut k,
        &mut b,
        fixture(name),
        FileType::Ascii,
        false,
        &mut env,
    )
    .unwrap();
    let face = faces(&b, None).unwrap()[0];
    let curve_ref = |e: EdgeId, _type_code: i64| key_of(EntityRef::Edge(e));
    let mut notify = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let r = subdivide_face_at_discontinuities(
        &mut k,
        &mut b,
        &mut notify,
        &curve_ref,
        face,
        ContinuityType::CInfinity,
    );
    let n = faces(&b, None).unwrap().len();
    let points: Vec<_> = solidcore::topology::brep::p1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|v| solidcore::topology::vertex::point(&b, v).unwrap())
        .collect();
    Subdivided {
        result: r,
        faces: n,
        xs: points.iter().map(|p| grid(p.x)).collect(),
        ys: points.iter().map(|p| grid(p.y)).collect(),
    }
}

/// `values` in units of 1e-12.
fn set(values: &[f64]) -> BTreeSet<i64> {
    values.iter().map(|&v| grid(v)).collect()
}

/// A crafted fixture of the `topology.brep_p10` difftest.
fn fixture(name: &str) -> Vec<u8> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/topology.brep_p10");
    std::fs::read(root.join(name)).unwrap()
}

#[test]
fn one_u_knot_and_two_v_knots_make_six_faces() {
    let s = subdivide("subdivide_u1v2.smb");
    s.result.unwrap();
    assert_eq!(s.faces, 6);
    assert_eq!(s.xs, set(&[0.0, 0.3, 1.0]));
    assert_eq!(s.ys, set(&[0.0, 0.2, 0.7, 1.0]));
}

#[test]
fn a_v_knot_alone_is_cut() {
    let s = subdivide("subdivide_u0v1.smb");
    s.result.unwrap();
    assert_eq!(s.faces, 2);
    assert_eq!(s.xs, set(&[0.0, 1.0]));
    assert_eq!(s.ys, set(&[0.0, 0.3, 1.0]));
}

#[test]
fn a_u_knot_alone_is_cut() {
    let s = subdivide("subdivide_u1v0.smb");
    s.result.unwrap();
    assert_eq!(s.faces, 2);
    assert_eq!(s.xs, set(&[0.0, 0.3, 1.0]));
    assert_eq!(s.ys, set(&[0.0, 1.0]));
}

/// What subdividing the face of an upstream test file at every interior knot leaves,
/// with its surface's knots and the surface across each V knot.
struct UpstreamSheet {
    u_knots: usize,
    v_knots: usize,
    /// Per V knot: the V derivatives of orders 0 to 3 at it, from below and from above
    /// (`Side::Left` / `Side::Right` span), at an interior u.
    sides: Vec<[[Point3; 4]; 2]>,
    /// The surface at both ends of each V knot's iso line (u at the domain's ends).
    knot_line_ends: Vec<[Point3; 2]>,
    faces: usize,
    vertices: Vec<Point3>,
}

/// Reads `pt_TestFiles/<rel>` (the face is its only one) and subdivides it at every
/// interior knot; `None` without the upstream checkout.
fn subdivide_upstream(rel: &str) -> Option<UpstreamSheet> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut k = Kernel::default();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(&mut k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    let all = faces(&b, None).unwrap();
    assert_eq!(all.len(), 1);
    let face = all[0];
    let surface = b.face(face).unwrap().surface.clone().unwrap();
    let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let (u_knots, v_knots) = surface
        .find_knots_by_max_continuity(&k, ContinuityType::CInfinity, false)
        .unwrap();
    let u = domain.min().x + 0.37 * (domain.max().x - domain.min().x);
    let sides = v_knots
        .iter()
        .map(|&v| {
            [Side::Left, Side::Right].map(|v_side| {
                let flags = EvaluateFlags {
                    v_side,
                    ..EvaluateFlags::default()
                };
                let d = surface
                    .evaluate(&k, Point2::new(u, v), 0, 3, flags)
                    .unwrap();
                [d[0][0], d[0][1], d[0][2], d[0][3]]
            })
        })
        .collect();
    let knot_line_ends = v_knots
        .iter()
        .map(|&v| {
            [domain.min().x, domain.max().x]
                .map(|u| surface.evaluate_point(&k, Point2::new(u, v)).unwrap())
        })
        .collect();
    let curve_ref = |e: EdgeId, _type_code: i64| key_of(EntityRef::Edge(e));
    let mut notify = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    subdivide_face_at_discontinuities(
        &mut k,
        &mut b,
        &mut notify,
        &curve_ref,
        face,
        ContinuityType::CInfinity,
    )
    .unwrap();
    let vertices = solidcore::topology::brep::p1::vertices(&b, None)
        .unwrap()
        .into_iter()
        .map(|v| solidcore::topology::vertex::point(&b, v).unwrap())
        .collect();
    Some(UpstreamSheet {
        u_knots: u_knots.len(),
        v_knots: v_knots.len(),
        sides,
        knot_line_ends,
        faces: faces(&b, None).unwrap().len(),
        vertices,
    })
}

/// Every V knot is a C2 discontinuity of the surface itself: the V derivatives of
/// orders 0 to 2 agree from both sides (to rounding), the third jumps by more than a
/// tenth of its size (a point inside a span gives both sides the same polynomial).
/// The face is cut along each of them: both ends of every knot's iso line are vertices.
fn assert_cut_at_every_v_knot(s: &UpstreamSheet) {
    for (i, [below, above]) in s.sides.iter().enumerate() {
        for order in 0..3 {
            let (a, b) = (below[order], above[order]);
            assert!(
                (a - b).length() <= 1e-12 * a.length().max(b.length()).max(1.0),
                "V knot {i}: derivative {order} differs: {a:?} {b:?}"
            );
        }
        let (a, b) = (below[3], above[3]);
        assert!(
            (a - b).length() > 0.1 * a.length().max(b.length()),
            "V knot {i}: third derivative does not jump: {a:?} {b:?}"
        );
    }
    for (i, ends) in s.knot_line_ends.iter().enumerate() {
        for p in ends {
            assert!(
                s.vertices.iter().any(|v| (*v - *p).length() <= 1e-9),
                "V knot {i}: no vertex at {p:?}"
            );
        }
    }
}

/// `surf_rev.smb`: one U knot and three V knots. Cut along all four, 2 x 4 faces;
/// upstream cuts at the U knot and the first V knot only (2 x 2).
#[test]
fn surf_rev_is_cut_at_every_v_knot() {
    let Some(s) = subdivide_upstream("surf_rev.smb") else {
        return;
    };
    assert_eq!((s.u_knots, s.v_knots), (1, 3));
    assert_cut_at_every_v_knot(&s);
    assert_eq!(s.faces, 8);
}

/// `Fillet/nurb_nurb_2.smb`: no U knot and three V knots. Cut along the three, 4 faces;
/// upstream makes no cut (1 face).
#[test]
fn nurb_nurb_2_is_cut_at_every_v_knot() {
    let Some(s) = subdivide_upstream("Fillet/nurb_nurb_2.smb") else {
        return;
    };
    assert_eq!((s.u_knots, s.v_knots), (0, 3));
    assert_cut_at_every_v_knot(&s);
    assert_eq!(s.faces, 4);
}
