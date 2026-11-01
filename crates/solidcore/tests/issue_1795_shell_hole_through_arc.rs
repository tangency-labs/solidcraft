//! tangency-labs/solidcore#1795: an inward shell of 2 of the #1528 drilled plate, open at
//! its top (z = 27.5) or at its face y = 20. The plate's arc side is a cylinder r 10 about z;
//! the hole, r 5 along x at y = z = 12.5, opens through it. Inset by 2 the arc becomes r 8
//! and the hole r 7, axes 2.5 apart, so the hole's tube passes out through the arc's offset.
//! SolidCraft's `smlib_shell` (the fork's C++) reports 1001 on its plate. Pinned SMLib
//! 1.0.1 shells this reconstruction open at the top and fails it open at y = 20 with
//! `SM_ERR` (1001), through `SmApiShellBrepFull` and through a bare `ShellBrep`; the port
//! does the same, with the same values (the `offset.offset_executive_p2` cases
//! `issue1795_plate_{api,static}_shell_2_*`; WR-617).
//!
//! `shell_open_at_the_top_shells_as_upstream` and `shell_open_at_y20_fails_as_upstream`
//! pin those default results (the API path, as `smlib_shell` calls it).
//! `issue_1795_extended_edges_mated` (ignored) shells the plate under WR-344's opt-in
//! `offset_extended_edges_mated`, the setting that made the top-open shell fail before its
//! refinement; `issue_1795_y20_read_back` (ignored) shells the y = 20 case on every
//! operand (in memory, written and read back, the committed file, a copy), all of which
//! fail alike.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{api_shell, planar_faces_at, report, volume, Notifications};
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::UpstreamFixes;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::brep::{p2 as brep_p2, p3 as brep_p3};
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, EntityRef, Kernel, SolidError};

/// The #1528 operand: a plate 27.5 thick on the profile (0, 0)-(42.5, 0)-(42.5, 20)-(0, 20)
/// plus the half circle r 10 centred (0, 10) on the x < 0 side, less a blind hole r 5 along
/// x from x = -10 to x = 7.5 at (y, z) = (12.5, 12.5).
fn drilled(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let cs: Vec<Curve> = vec![
        Curve::from(curves::create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(curves::create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let body = sweeps::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap();
    // a cylinder r 5 along z from (-10, 12.5, 12.5), turned onto +x about its base
    let mut drill =
        primitives::create_cylinder(k, Point3::new(-10.0, 12.5, 12.5), 5.0, 17.5).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        90.0,
    )
    .unwrap();
    let mut b = boolean::boolean_difference(k, body, drill).unwrap();
    b.refresh_cached_topology();
    b
}

/// The shell options of the issue: 2 inward, extended convex edges, merged results.
fn options(create_offset_solid: bool) -> ShellOptions {
    ShellOptions {
        offset: OffsetOptions {
            distance: -2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid,
        merge_sheet_side_walls: false,
    }
}

/// The default fixes shell the plate through the API as pinned SMLib 1.0.1 does (the
/// difftest case agrees to 0): a closed manifold solid of 16 faces, 37 edges and 24
/// vertices, volume 9655.150582612916.
#[test]
fn shell_open_at_the_top_shells_as_upstream() {
    let mut k = Kernel::default();
    let mut b = drilled(&mut k);
    let open = planar_faces_at(&k, &b, 2, 27.5);
    assert_eq!(open.len(), 1, "the top face");
    let r = api_shell(&mut k, &mut b, options(false), &open).unwrap();
    assert_eq!(
        (
            r.face_list().len(),
            r.edge_list().len(),
            r.vertex_list().len()
        ),
        (16, 37, 24)
    );
    let label = "1795 shell 2 open at z 27.5";
    assert!(report(&k, label, Ok(r), Some(9655.150582612916), 1e-9));
}

/// Open at y = 20 the default fixes fail the shell with `SM_ERR` (1001, in
/// `make_faceuse`), as pinned SMLib 1.0.1 does (WR-617; difftest
/// `issue1795_plate_{api,static}_shell_2_y20*` compare the status), with and without the
/// offset solid.
#[test]
fn shell_open_at_y20_fails_as_upstream() {
    for cos in [false, true] {
        let mut k = Kernel::default();
        let mut b = drilled(&mut k);
        let open = planar_faces_at(&k, &b, 1, 20.0);
        assert_eq!(open.len(), 1, "the y = 20 face");
        let r = api_shell(&mut k, &mut b, options(cos), &open);
        assert!(
            matches!(r, Err(SolidError::Upstream { code: 1001, .. })),
            "create_offset_solid={cos}: {:?}",
            r.map(|b| b.face_list().len())
        );
    }
}

/// The top (`z = 27.5`) and `y = 20` faces' indices in the `GetFaces` order of the committed
/// #1528 operand file, as the difftest runner reads it (the indices the `issue1795` cases
/// select).
/// Run with `--ignored --nocapture` to print it.
#[test]
#[ignore = "prints the fixture's face index"]
fn issue1528_plate_file_top_face_index() {
    let mut k = Kernel::default();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(root.join("fixtures/offset.offset_executive_p2/data/issue1528_plate.smb"))
            .unwrap();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let fresh = Brep::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut read = read_brep_from_file(&mut k, Some(bytes), true, false, &mut env).unwrap();
    read.refresh_cached_topology();
    let n = read.face_list().len();
    for (label, axis, value) in [("top", 2, 27.5), ("y20", 1, 20.0)] {
        let faces = planar_faces_at(&k, &read, axis, value);
        assert_eq!(faces.len(), 1, "the {label} face");
        let index = read
            .face_list()
            .iter()
            .position(|&f| f == faces[0])
            .unwrap();
        eprintln!("ISSUE 1795 {label} face index {index} of {n}");
    }
}

/// `SmApiShellBrepFull(brep, -2, TRUE, TRUE, create_offset_solid, top)` as SolidCraft's
/// `smlib_shell` calls it (its third flag `TRUE`), under `fixes`.
/// The face left open: the top `z = 27.5` (the difftest case's) or the side `y = 20`, the
/// plane the arc meets tangentially at `(0, 20)`, which the issue's tangent-gap bridge
/// ("the wall joining the arc's offset to the kept top at y 18..20") points to.
#[derive(Debug, Clone, Copy)]
enum Open {
    Z27,
    Y20,
}

impl Open {
    fn faces(self, k: &Kernel, b: &Brep) -> Vec<FaceId> {
        match self {
            Open::Z27 => planar_faces_at(k, b, 2, 27.5),
            Open::Y20 => planar_faces_at(k, b, 1, 20.0),
        }
    }
}

/// The plate shelled through the API (`SmApiShellBrepFull`, as `smlib_shell` and the
/// difftest runner call it) under `fixes`.
fn probe(fixes: &UpstreamFixes, create_offset_solid: bool, open: Open) -> solidcore::Result<Brep> {
    let mut k = Kernel {
        fixes: *fixes,
        ..Default::default()
    };
    let mut b = drilled(&mut k);
    let open = open.faces(&k, &b);
    assert_eq!(open.len(), 1, "one open face");
    api_shell(&mut k, &mut b, options(create_offset_solid), &open)
}

/// The plate under `offset_extended_edges_mated` (WR-344) alone, with `create_offset_solid`
/// as `smlib_shell` passes it: the setting under which the top-open shell failed before
/// WR-344's refinement left an unmated edge to the merge. Run with `--ignored --nocapture`.
#[test]
#[ignore = "probe: the #1795 failure under offset_extended_edges_mated"]
fn issue_1795_extended_edges_mated() {
    // (`UpstreamFixes` is `#[non_exhaustive]`: set the field on a default)
    let mut fixes = UpstreamFixes::default();
    fixes.offset_extended_edges_mated = true;
    for open in [Open::Z27, Open::Y20] {
        match probe(&fixes, true, open) {
            Err(e) => eprintln!(
                "PROBE 1795 open={open:?} offset_extended_edges_mated, cos=true: Err({e:?})"
            ),
            Ok(_) => {
                eprintln!("PROBE 1795 open={open:?} offset_extended_edges_mated, cos=true: Ok");
            }
        }
    }
}

/// The input as the issue states it: "passes `smlib_brep_check` level 2". Checked on a body
/// of its own (a query can change the body a shell then sees). Run with `--ignored
/// --nocapture`.
#[test]
#[ignore = "probe: the #1795 input's check_brep level 2"]
fn issue_1795_input_checks_clean() {
    let mut k = Kernel::default();
    let mut b = drilled(&mut k);
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    eprintln!(
        "PROBE 1795 input check_brep level 2: failures {} valid {} {:?}",
        c.failure_count(),
        c.is_valid(),
        c.lines()
    );
}

/// The drilled plate's edges as the shell sees them (its `Original` arena): per edge use,
/// its edge; per edge, its end vertices' points, its curve kind and its faces' surface kinds.
/// Maps the trace's edge uses to edge ids. Run with `--ignored --nocapture`.
#[test]
#[ignore = "probe: the #1795 operand's edge uses and edges"]
fn issue_1795_edge_map() {
    let mut k = Kernel::default();
    k.fixes.offset_extended_edges_mated = true;
    let b = drilled(&mut k);
    let kind = |d: String| d.split(['(', ' ', '{']).next().unwrap_or("").to_string();
    for (eu, rec) in b.edgeuses().iter() {
        eprintln!(
            "PROBE 1795 edgeuse {} -> edge {:?}",
            eu.index(),
            rec.edge.map(EdgeId::index)
        );
    }
    for e in b.edge_list().iter().copied() {
        let rec = b.edge(e).unwrap();
        let points: Vec<_> = solidcore::topology::edge::vertices(&b, e, None)
            .unwrap()
            .into_iter()
            .flatten()
            .map(|v| b.vertex(v).unwrap().point)
            .collect();
        let faces: Vec<String> = solidcore::topology::edge::faces(&b, e, None)
            .unwrap()
            .into_iter()
            .map(|f| {
                let s = b
                    .face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .map(|s| kind(format!("{:?}", s.kind())));
                format!("face {} {:?}", f.index(), s)
            })
            .collect();
        eprintln!(
            "PROBE 1795 edge {}: curve {:?} vertices {points:?} faces {faces:?}",
            e.index(),
            rec.curve.as_ref().map(|c| kind(format!("{:?}", c.kind())))
        );
    }
}

/// The notification layout `issue_1528_write_operand.rs` writes the operand with: a caller
/// key per entity, in key domain `base`.
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

/// `b` written as `issue_1528_write_operand.rs` writes the fixture file.
fn written(k: &Kernel, b: &Brep) -> Vec<u8> {
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
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// `bytes` read into a new B-rep, as the difftest runner reads a file source.
fn read_back(k: &mut Kernel, bytes: Vec<u8>) -> Brep {
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let read_key = |e| named(e, 1 << 40);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), true, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// `b` through the API (default fixes) in `k`, `open` removed.
fn api_body(k: &mut Kernel, mut b: Brep, create_offset_solid: bool, open: Open) -> String {
    let faces = open.faces(k, &b);
    assert_eq!(faces.len(), 1, "one open face");
    let index = b.face_list().iter().position(|&f| f == faces[0]);
    let r = api_shell(k, &mut b, options(create_offset_solid), &faces);
    describe(k, index, r)
}

/// `b` copied as `SmApiBrepCopy` copies it (the `SmBrep` copy constructor), keyed in its
/// own domain.
fn copied(k: &mut Kernel, b: &Brep) -> Brep {
    let target = |e| named(e, 1 << 44);
    let mut state = brep_p2::ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    env.key_of = &target;
    let mut c = brep_p3::api_brep_copy(k, &mut env, Some(b), &|e| named(e, 0)).unwrap();
    c.refresh_cached_topology();
    c
}

/// `OffsetExecutive::shell_brep` (default fixes) of `b` itself in `k`, `open` removed.
fn shell_body(k: &mut Kernel, mut b: Brep, create_offset_solid: bool, open: Open) -> String {
    let faces = open.faces(k, &b);
    assert_eq!(faces.len(), 1, "one open face");
    let index = b.face_list().iter().position(|&f| f == faces[0]);
    let mut notifications = Notifications::new(&mut b);
    let r = OffsetExecutive::shell_brep(
        k,
        b,
        options(create_offset_solid),
        &faces,
        &mut notifications.context(),
    )
    .result;
    describe(k, index, r)
}

/// The plate as the probe reports it: the open face's index, then the status or the
/// result's counts and volume.
fn describe(k: &Kernel, index: Option<usize>, r: solidcore::Result<Brep>) -> String {
    match r {
        Err(e) => format!("open face index {index:?}: Err({e:?})"),
        Ok(mut b) => {
            b.refresh_cached_topology();
            format!(
                "open face index {index:?}: Ok faces={} edges={} vertices={} volume={:?}",
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len(),
                volume(k, &mut b)
            )
        }
    }
}

/// #1795 Y20 (WR-617): the pinned C++ and the port fail the committed `.smb` open at
/// y = 20 (`issue1795_plate_{api,static}_shell_2_y20*`), and so does every other operand
/// of the plate. Shells, with the default fixes, the body as built (in its building
/// kernel and in a fresh one), the body written and read back (fresh kernel) and the
/// committed file, then the committed file and the in-memory body through the API, and the
/// in-memory body copied with `api_brep_copy` then shelled. Run with `--ignored --nocapture`.
#[test]
#[ignore = "probe: #1795 Y20 in-memory body against its read-back"]
fn issue_1795_y20_read_back() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let committed =
        std::fs::read(root.join("fixtures/offset.offset_executive_p2/data/issue1528_plate.smb"))
            .unwrap();
    for open in [Open::Y20, Open::Z27] {
        for cos in [false, true] {
            let tag = format!("open={open:?} cos={cos}");
            let mut k = Kernel::default();
            let b = drilled(&mut k);
            eprintln!(
                "PROBE 1795 {tag} in-memory, building kernel: {}",
                shell_body(&mut k, b, cos, open)
            );

            let mut k1 = Kernel::default();
            let b = drilled(&mut k1);
            let mut k = Kernel::default();
            eprintln!(
                "PROBE 1795 {tag} in-memory, fresh kernel: {}",
                shell_body(&mut k, b, cos, open)
            );

            let mut k1 = Kernel::default();
            let b = drilled(&mut k1);
            let bytes = written(&k1, &b);
            eprintln!(
                "PROBE 1795 {tag} written bytes {} (committed {}, same {})",
                bytes.len(),
                committed.len(),
                bytes == committed
            );
            let mut k = Kernel::default();
            let b = read_back(&mut k, bytes);
            eprintln!(
                "PROBE 1795 {tag} read back, fresh kernel: {}",
                shell_body(&mut k, b, cos, open)
            );

            let mut k = Kernel::default();
            let b = read_back(&mut k, committed.clone());
            eprintln!(
                "PROBE 1795 {tag} committed file: {}",
                shell_body(&mut k, b, cos, open)
            );

            let mut k = Kernel::default();
            let b = read_back(&mut k, committed.clone());
            eprintln!(
                "PROBE 1795 {tag} committed file, API: {}",
                api_body(&mut k, b, cos, open)
            );

            let mut k = Kernel::default();
            let b = drilled(&mut k);
            eprintln!(
                "PROBE 1795 {tag} in-memory, API: {}",
                api_body(&mut k, b, cos, open)
            );

            let mut k = Kernel::default();
            let b = drilled(&mut k);
            let c = copied(&mut k, &b);
            eprintln!(
                "PROBE 1795 {tag} in-memory, api_brep_copy then shell_brep: {}",
                shell_body(&mut k, c, cos, open)
            );
        }
    }
}

/// WR-617: the shell's offset faces before the merge (`merge_results` off, the executive's
/// offset B-rep as `ShellBrep` returns it unmerged), open at y = 20 and, for comparison, at
/// the top. Prints each face's surface kind and the point at the middle of its domain, then
/// every edge with an end near the plane x = 0 (the arc's tangent junction with y = 20,
/// where the merge's vertex relation fails: vertices at (0, 18, 2), (0, 18.5, 2) and
/// (0, 19.5, 2)), with its end points and faces. Run with `--ignored --nocapture`.
#[test]
#[ignore = "probe: WR-617, the unmerged offset faces at the tangent junction"]
fn issue_1795_unmerged_near_x0() {
    let kind = |d: String| d.split(['(', ' ', '{']).next().unwrap_or("").to_string();
    for (open, cos) in [(Open::Y20, false), (Open::Y20, true), (Open::Z27, true)] {
        let mut k = Kernel::default();
        let mut b = drilled(&mut k);
        let faces = open.faces(&k, &b);
        let mut unmerged = options(cos);
        unmerged.offset.merge_results = false;
        let shelled = api_shell(&mut k, &mut b, unmerged, &faces);
        let Ok(r) = shelled
            .inspect_err(|e| eprintln!("PROBE 1795 unmerged open={open:?} cos={cos}: Err({e:?})"))
        else {
            continue;
        };
        eprintln!(
            "PROBE 1795 unmerged open={open:?} cos={cos}: {} faces, {} edges",
            r.face_list().len(),
            r.edge_list().len()
        );
        for &f in r.face_list() {
            let face = r.face(f).unwrap();
            let s = face.surface.as_ref();
            let mid = s.map(|s| s.evaluate_point(&k, face.uv_domain.mid()).unwrap());
            eprintln!(
                "PROBE 1795 unmerged open={open:?} cos={cos} face {}: {:?} mid {mid:?}",
                f.index(),
                s.map(|s| kind(format!("{:?}", s.kind())))
            );
        }
        for &e in r.edge_list() {
            let points: Vec<Point3> = solidcore::topology::edge::vertices(&r, e, None)
                .unwrap()
                .into_iter()
                .flatten()
                .map(|v| r.vertex(v).unwrap().point)
                .collect();
            if !points.iter().any(|p| p.x.abs() < 0.05) {
                continue;
            }
            let on: Vec<u32> = solidcore::topology::edge::faces(&r, e, None)
                .unwrap()
                .into_iter()
                .map(FaceId::index)
                .collect();
            eprintln!(
                "PROBE 1795 unmerged open={open:?} cos={cos} edge {}: curve {:?} vertices {points:?} faces {on:?}",
                e.index(),
                r.edge(e)
                    .unwrap()
                    .curve
                    .as_ref()
                    .map(|c| kind(format!("{:?}", c.kind())))
            );
        }
    }
}
