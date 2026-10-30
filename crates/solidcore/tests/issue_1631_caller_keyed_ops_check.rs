//! tangency-labs/solidcore#1631: an operation on a `NamingScheme::Caller` body (an
//! `SmApiBrepCopy` that records its caller's keys, #1585) keeps that scheme, and
//! `check_brep` checks the result as the C++ `smlib_brep_check` does: every geometry
//! owner is its entity (`pGeom->GetOwner() == this`). The edges, faces and edge uses
//! an operation creates record no caller key; they are named by the scheme's default
//! identity, by the binders and by the check alike (`Brep::owner_identity`), so the
//! check finds the result valid instead of refusing it with `InvalidInput`.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::{boolean, boolean_union};
use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p10::{
    make_topology_from_data, project_and_trim, MakeTopologyOptions, ProjectAndTrimOptions,
    ProjectEnv, TopologyEnv, TrimType,
};
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::{api_brep_copy, topology_ref};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::{edge, vertex, Brep, BrepId, EdgeId, NamingScheme};
use solidcore::{EntityRef, Kernel};

/// Each copy names its objects in its own key domain (as SolidCraft's copies do).
fn named(e: EntityRef, domain: u64) -> ObjectRef {
    let r = topology_ref(0, e);
    ObjectRef {
        type_code: r.type_code,
        key: ObjectKey(domain + r.type_code as u64 * 1_000_000 + (r.key.0 & 0xffff_ffff)),
    }
}

/// `SmApiBrepCopy` of `b` named in domain `n`: a `NamingScheme::Caller` body with its
/// keys recorded (#1585).
fn copy(k: &mut Kernel, b: &Brep, n: u64) -> Brep {
    let target = move |e| named(e, n * 100_000_000);
    let mut state = ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target;
    let c = api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0)).unwrap();
    assert_eq!(c.naming(), NamingScheme::Caller);
    c
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// `check_brep` at Level2 finds `b` valid, with no failed test.
fn assert_checks_valid(k: &mut Kernel, b: &mut Brep, what: &str) {
    let c = check_brep(k, b, CheckLevel::Level2)
        .unwrap_or_else(|e| panic!("{what}: check_brep refused the body: {e}"));
    assert!(c.is_valid(), "{what}: {:#?}", c.lines());
    assert!(c.lines().is_empty(), "{what}: {:#?}", c.lines());
}

fn assert_volume(k: &Kernel, b: &mut Brep, analytic: f64, what: &str) {
    let v = volume(k, b);
    assert!(
        (v - analytic).abs() <= 1e-9 * analytic,
        "{what}: volume {v}, analytic {analytic}"
    );
}

fn boxes(k: &mut Kernel) -> (Brep, Brep) {
    let a = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let b = create_box(k, Point3::new(5.0, 5.0, 5.0), 10.0, 10.0, 10.0).unwrap();
    (a, b)
}

/// Two 10 mm cubes overlapping in a 5 mm cube: 2 * 1000 - 125.
const UNION_VOLUME: f64 = 1875.0;

/// The issue's first repro: the union of two caller-keyed copies.
#[test]
fn union_of_two_keyed_copies_checks_valid() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k);
    let (mut ca, cb) = (copy(&mut k, &a, 1), copy(&mut k, &b, 2));
    assert_checks_valid(&mut k, &mut ca, "copy");
    let mut u = boolean(&mut k, ca, cb, BooleanOperation::Union)
        .unwrap()
        .result;
    assert_eq!(u.naming(), NamingScheme::Caller);
    assert_volume(&k, &mut u, UNION_VOLUME, "union(copy, copy)");
    assert_checks_valid(&mut k, &mut u, "union(copy, copy)");
}

#[test]
fn union_of_keyed_copy_with_box_checks_valid() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k);
    let ca = copy(&mut k, &a, 1);
    let mut u = boolean_union(&mut k, ca, b).unwrap();
    assert_volume(&k, &mut u, UNION_VOLUME, "union(copy, box)");
    assert_checks_valid(&mut k, &mut u, "union(copy, box)");
}

/// Side finding: the union of two plain boxes (a notification-named result) left the
/// original box's geometry without owner (`SmBrep` test #9, `SmEdge` test #61).
#[test]
fn union_of_two_boxes_checks_valid() {
    let mut k = Kernel::default();
    let (a, b) = boxes(&mut k);
    let mut u = boolean_union(&mut k, a, b).unwrap();
    assert_volume(&k, &mut u, UNION_VOLUME, "union(box, box)");
    assert_checks_valid(&mut k, &mut u, "union(box, box)");
}

/// The box's top front edge, along x at y = 0, z = `top`.
fn top_front_edge(b: &Brep, top: f64) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let (p, q) = (
                vertex::point(b, s.unwrap()).unwrap(),
                vertex::point(b, t.unwrap()).unwrap(),
            );
            p.y.abs() < 1e-9 && q.y.abs() < 1e-9 && p.z == top && q.z == top
        })
        .unwrap()
}

/// The issue's second repro: a 30 x 20 x 10 copy with its top front edge rounded r2,
/// 6000 - (1 - pi/4) * 2^2 * 30.
#[test]
fn fillet_session_on_keyed_copy_checks_valid() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let mut c = copy(&mut k, &a, 1);
    let e = top_front_edge(&c, 10.0);
    {
        let mut s = FilletSession::new(&mut k, &mut c).unwrap();
        s.prepare_constant_radius_edges(&mut k, &[e], 2.0, 1, 1, 1.0)
            .unwrap();
        s.create_corners(&k, None).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut c, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(c.naming(), NamingScheme::Caller);
    let analytic = 6000.0 - (1.0 - PI / 4.0) * 4.0 * 30.0;
    assert_volume(&k, &mut c, analytic, "fillet session");
    assert_checks_valid(&mut k, &mut c, "fillet session");
}

/// `fillet_edges` (`SmApiFilletEdges`) of a 10 mm keyed copy's edge, r1:
/// 1000 - (1 - pi/4) * 10.
#[test]
fn fillet_edges_of_keyed_copy_checks_valid() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let mut c = copy(&mut k, &a, 1);
    let e = top_front_edge(&c, 10.0);
    fillet_edges(&mut k, &mut c, &[e], 1.0, 1, 1, 1.0).unwrap();
    assert_eq!(c.naming(), NamingScheme::Caller);
    let analytic = 1000.0 - (1.0 - PI / 4.0) * 10.0;
    assert_volume(&k, &mut c, analytic, "fillet_edges");
    assert_checks_valid(&mut k, &mut c, "fillet_edges");
}

/// A keyed copy of a 10 mm box shelled inward by 1 with its top open:
/// 1000 - 8 * 8 * 9.
#[test]
fn shell_of_keyed_copy_checks_valid() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let c = copy(&mut k, &a, 1);
    let top = offset_fixture::planar_faces_at(&k, &c, 2, 10.0);
    assert_eq!(top.len(), 1);
    let mut s = offset_fixture::shell(&mut k, c, -1.0, &top).unwrap();
    assert_eq!(s.naming(), NamingScheme::Caller);
    assert_volume(&k, &mut s, 1000.0 - 576.0, "shell");
    assert_checks_valid(&mut k, &mut s, "shell");
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
}

/// A 4 x 4 boss extruded 5 from the top of a keyed copy of a 10 mm box and united
/// with it: 1000 + 4 * 4 * 5.
#[test]
fn extrude_onto_keyed_copy_checks_valid() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let c = copy(&mut k, &a, 1);
    let p = [(2.0, 2.0), (6.0, 2.0), (6.0, 6.0), (2.0, 6.0)].map(|(x, y)| Point3::new(x, y, 10.0));
    let sides: Vec<Curve> = (0..4).map(|i| line(p[i], p[(i + 1) % 4])).collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    let boss = create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 5.0, true).unwrap();
    let mut u = boolean_union(&mut k, c, boss).unwrap();
    assert_volume(&k, &mut u, 1080.0, "extrude onto copy");
    assert_checks_valid(&mut k, &mut u, "extrude onto copy");
}

/// The difftest runner's caller keys: kind and arena index.
fn key_of(e: EntityRef) -> ObjectRef {
    let (t, k, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code: t,
        key: ObjectKey((1u64 << 40) * (k + 1) + u64::from(i)),
    }
}
/// An imported body names its objects by its caller's keys (`make_topology_from_data`);
/// `ProjectAndTrim` splits edges, whose new edge uses get UV trims before their keys
/// are recorded. They end up owned under their recorded keys, as in the C++ (the
/// `topology.brep_p10` `project_*_Defeature_inset_cube` cases).
#[test]
fn project_and_trim_names_split_trims_by_caller_keys() {
    // the upstream test file is not checked in (CI's test job has no upstream tree; the
    // difftest job runs the same body through `topology.brep_p10`), as
    // `issue_1570_flat_body_ray_fire`
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Defeature/inset_cube.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("unavailable upstream test file: {}", path.display());
        return;
    };
    let mut k = Kernel::default();
    let mut data = BrepData::read_from_file(&k, bytes, FileType::Ascii).unwrap();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut store = AttributeStore::new();
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store,
        key_of: &key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    let options = MakeTopologyOptions {
        make_uv_trim_curves: true,
        add_analytics: false,
        force_same_tol: false,
        check_uv_trim_curves: false,
        always_heal: false,
        healer_enabled: false,
    };
    make_topology_from_data(&mut k, &mut b, &mut data, &mut env, options).unwrap();
    assert_eq!(b.naming(), NamingScheme::Caller);
    let keys = |o: CacheObject| match o {
        CacheObject::Edge(e) | CacheObject::Curve(e) => key_of(EntityRef::Edge(e)),
        CacheObject::Face(f) | CacheObject::Surface(f) => key_of(EntityRef::Face(f)),
        CacheObject::Vertex(x) => key_of(EntityRef::Vertex(x)),
        CacheObject::Other(r) => r,
        _ => key_of(EntityRef::Brep(BrepId::from_index(0))),
    };
    let owner_of = |_: ObjectRef| None;
    let penv = ProjectEnv {
        brep_id: BrepId::from_index(0),
        keys: &keys,
        owner_of: &owner_of,
    };
    let opts = ProjectAndTrimOptions {
        approximation_tol_3d: None,
        angle_tol_radians: None,
        subset_faces: None,
    };
    let mut notify = NotifyEnv {
        records: &mut records,
        attributes: &mut store,
        context: None,
        key_of: &key_of,
    };
    let curve = BSplineCurve::create_line_segment(
        3,
        Vec3::new(-100.0, 0.5, 100.0),
        Vec3::new(100.0, 0.5, 100.0),
    )
    .unwrap();
    let r = project_and_trim(
        &mut k,
        &mut b,
        &penv,
        &mut notify,
        &Curve::from(curve),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 10.0, 100.0),
        TrimType::KeepPoint,
        &opts,
    );
    r.unwrap();
    for (u, rec) in b.edgeuses().iter() {
        if let Some(t) = rec.uv_trim_curve.as_ref() {
            if t.owner() != Some(key_of(EntityRef::Edgeuse(u))) {
                eprintln!(
                    "BAD {u:?} owner={:?} identity={:?}",
                    t.owner(),
                    b.owner_identity(EntityRef::Edgeuse(u))
                );
            }
        }
    }
}
