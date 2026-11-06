//! tangency-labs/solidcore#2715: a fillet the session refuses puts the target back as it
//! was. Three steps of the session took that copy with `Brep::clone`, whose copy
//! constructor drops every surface's owner: the caller's body came back from the refusal
//! with no face owning its surface, so `GetSurfaces` listed none and the `.smb` writer left
//! every surface out (a file of faces without geometry). The steps now take a
//! `Brep::snapshot`, which keeps the geometry exactly.
//!
//! Each case is a refusal the step's own tests pin (the cliff overflow's subtraction,
//! WR-631; the trim at creases, WR-695; the kept features, #2273). After it the body must
//! still own every surface, and written and read back have the same faces and volume.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cone};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p1::surfaces;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, EntityRef, Kernel};

/// A caller key per entity in key domain `base` (as `issue_1795_shell_hole_through_arc`).
fn named(e: EntityRef, base: u64) -> ObjectRef {
    let (type_code, kind, index) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
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
        type_code,
        key: ObjectKey(base + 1 + (kind << 32) + u64::from(index)),
    }
}

/// `b` written as a binary `.smb` file and read back into a new B-rep.
fn written_and_read_back(k: &mut Kernel, b: &Brep) -> Brep {
    let store = AttributeStore::new();
    let recs = NotifyRecords::new();
    let key_of = |e| named(e, 0);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &recs,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bytes = write_brep_to_file(k, Some(b), &env, false).unwrap();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let read_key = |e| named(e, 1 << 40);
    let mut tenv = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut out = read_brep_from_file(k, Some(bytes), false, false, &mut tenv).unwrap();
    out.refresh_cached_topology();
    out
}

/// The body a refused fillet left: still the whole solid of `volume`, owning every
/// face's surface, and the same faces and volume once saved and read back.
fn assert_left_whole(k: &mut Kernel, body: &mut Brep, volume: f64, case: &str) {
    assert!(is_manifold_solid(body).unwrap(), "{case}: not left whole");
    let faces = body.faces().ids().count();
    assert_eq!(
        surfaces(body, None).unwrap().len(),
        faces,
        "{case}: a face of the refused body does not own its surface"
    );
    let v = brep_volume_default(k, body, 1e-7).unwrap();
    assert!((v - volume).abs() <= 1e-9 * volume, "{case}: volume {v}");
    let mut back = written_and_read_back(k, body);
    assert_eq!(back.faces().ids().count(), faces, "{case}: faces read back");
    assert_eq!(
        surfaces(&back, None).unwrap().len(),
        faces,
        "{case}: surfaces read back"
    );
    let w = brep_volume_default(k, &mut back, 1e-7).unwrap();
    assert!(
        (w - volume).abs() <= 1e-9 * volume,
        "{case}: volume read back {w}"
    );
}

/// The edge from `p` to `q` (either way).
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
        })
        .expect("edge")
}

/// The cliff overflow's flags (as `wr631_cliff_overflow_construction`), with or without
/// the trim at the creases.
fn cliff_kernel(trim: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_overflow_trim_at_creases = trim;
    k
}

/// The session's refusal of `e` of `body` blended `r` (`chamfer`: a linear section), with
/// its error code and comment.
fn refused(k: &mut Kernel, body: &mut Brep, e: EdgeId, r: f64, chamfer: bool) -> (i32, String) {
    let mut session = FilletSession::new(k, body).unwrap();
    session
        .prepare_constant_radius_edges(k, &[e], r, u32::from(!chamfer), 1, 1.0)
        .unwrap();
    let result = session.do_fillet(k);
    let info = session.error_info().clone();
    drop(session);
    assert!(result.is_err(), "the blend r {r} is built");
    (info.code, info.comment)
}

/// `cliff_overflow`'s subtraction refused (#1467's box at r10.01 and r10.02, as
/// `wr631_cliff_overflow_construction`): the target is put back from the copy taken
/// before the Boolean.
#[test]
fn a_refused_cliff_subtraction_leaves_a_body_that_saves_its_surfaces() {
    let (a, b) = (Point3::new(0.0, 20.0, 10.0), Point3::new(40.0, 20.0, 10.0));
    let mut seen = 0;
    for r in [10.01, 10.02] {
        let mut k = cliff_kernel(false);
        let mut body = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
        let e = edge_between(&k, &body, a, b);
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
            .unwrap();
        let result = session.do_fillet(&mut k);
        let info = session.error_info().clone();
        drop(session);
        if result.is_ok() {
            continue;
        }
        assert_eq!(info.code, FilletErrorType::NoRoom as i32, "r {r}: {info:?}");
        assert_eq!(
            info.comment,
            "Fillet Error: subtracting a cliff overflow's removal region leaves no solid"
        );
        assert_left_whole(&mut k, &mut body, 8000.0, &format!("cliff r {r}"));
        seen += 1;
    }
    assert!(seen > 0, "neither radius is refused by the subtraction");
}

/// The stepped bar of `wr695_overflow_trim_at_creases`: its profile
/// `(0,0) (10,0) (10,5) (7,5) (7,10) (0,10)` swept 20 along z.
fn stepped_bar(k: &mut Kernel) -> Brep {
    const PROFILE: [[f64; 2]; 6] = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 5.0],
        [7.0, 5.0],
        [7.0, 10.0],
        [0.0, 10.0],
    ];
    let cs: Vec<Curve> = PROFILE
        .iter()
        .zip(PROFILE.iter().cycle().skip(1))
        .map(|(a, b)| {
            Curve::from(
                create_line_segment(Vec3::new(a[0], a[1], 0.0), Vec3::new(b[0], b[1], 0.0))
                    .unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 20.0, true).unwrap()
}

/// `trim_at_creases` refused (the stepped bar's inner step edge chamfered 4, which reaches
/// every face, as `wr695_overflow_trim_at_creases`): the target is put back from the copy
/// taken before the trim.
#[test]
fn a_refused_trim_at_creases_leaves_a_body_that_saves_its_surfaces() {
    let mut k = cliff_kernel(true);
    let mut body = stepped_bar(&mut k);
    let e = edge_between(
        &k,
        &body,
        Point3::new(10.0, 5.0, 0.0),
        Point3::new(10.0, 5.0, 20.0),
    );
    let (code, comment) = refused(&mut k, &mut body, e, 4.0, true);
    assert_eq!(code, FilletErrorType::BadMerge as i32);
    assert!(
        comment.contains("reaches every face of the body"),
        "{comment}"
    );
    assert_left_whole(&mut k, &mut body, 1700.0, "trim at creases");
}

/// The fork's opt-ins `fork_eb883ad_keep_features` runs the kept features under.
fn keep_kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut fixes = UpstreamFixes::default();
    fixes.fillet_refuse_rails_leaving_faces = true;
    fixes.fillet_cliff_overflow = true;
    fixes.fillet_overflow_trim_at_creases = true;
    fixes.linear_sweep_arc_is_cylinder = true;
    fixes.precise_properties_body_length_scale = true;
    fixes.fillet_keep_features = true;
    k.fixes = fixes;
    k
}

/// `keep_features_in_bands` refused (a boss leaning 30 degrees inside the round's band, as
/// `fork_eb883ad_keep_features`): the target is put back from the copy taken before the
/// steps. Through the public `fillet_edges`.
#[test]
fn a_refused_kept_feature_leaves_a_body_that_saves_its_surfaces() {
    let mut k = keep_kernel();
    let block = create_box(&mut k, Point3::ZERO, 40.0, 30.0, 20.0).unwrap();
    let cone = create_cone(&mut k, Point3::new(20.0, 4.0, 20.0), 2.0, 1.0, 1.732).unwrap();
    let mut body = boolean_union(&mut k, block, cone).unwrap();
    let mut before = body.clone_standing_for_self().unwrap();
    let volume = brep_volume_default(&k, &mut before, 1e-7).unwrap();
    let q = Point3::new(5.0, 0.0, 20.0);
    let e = body
        .edge_list()
        .iter()
        .copied()
        .find(|&e| edge_closest_point(&k, &body, e, q).is_ok_and(|c| c.closest.distance < 0.1))
        .expect("the top front edge");
    assert!(fillet_edges(&mut k, &mut body, &[e], 8.0, 1, 1, 1.0).is_err());
    assert_left_whole(&mut k, &mut body, volume, "kept feature");
}
