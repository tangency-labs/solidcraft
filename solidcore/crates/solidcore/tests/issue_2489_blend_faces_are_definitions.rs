//! tangency-labs/solidcore#2489: every face a blend makes is a definition, never a bare
//! NURBS (#2388's direction), checked by `blend::definition_check` over a set of blend
//! cases with every fix on (`UpstreamFixes::all()`).
//!
//! The check starts as a ledger of the kinds that are not definitions yet,
//! `data/blend_definition_ledger/`, one file per kind naming its issue. The ledger only
//! shrinks: the test fails on a bare face of a kind the ledger does not list (a regression,
//! or a new path that builds bare faces), and on a listed kind no case makes bare any more
//! (fixed: delete its file). It ends empty.
//!
//! The cases: a box edge rounded r 5 (an exact cylinder, ending in two Ends), its Gⁿ round
//! (continuity 2), its chamfer; three equal rounds at a box corner (the Ball) and three
//! unequal ones (the Setback), and beside a cylinder (the curved Setback, #2343); a boss's base rounded r 1 with its Turns Mitred and Swept; a
//! cut ball's rim (two Joins); #2256's tangent peg (the Mixed corner); and the offset of
//! #2388's chamfered box (the offsets of a chamfer and two rounds); and the two
//! captured mirrored shell rims, using the geometry matrix's shared builder and rig fix set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::collections::{BTreeMap, BTreeSet};

use solidcore::blend::definition_check::{
    bare_blend_faces, is_definition, mark_blend_faces, offsets_of_blends, BareBlendFace,
    BlendFaceKind, BlendFaceLog, MadeBlendFace,
};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{offset_brep_simple, OffsetOptions};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeId;
use solidcore::{Brep, EntityRef, Kernel};

#[path = "support/mirrored_shell_rim.rs"]
mod mirrored_shell_rim;

/// Every fix on, the blend face log on.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k.blend_face_log = Some(BlendFaceLog::default());
    k
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1_f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

fn picks(k: &Kernel, b: &Brep, at: &[[f64; 3]]) -> Vec<EdgeId> {
    at.iter().map(|&p| pick(k, b, p)).collect()
}

/// Rounds the edges at `at` together with circular sections of `radii` (one each), Turns
/// Swept when `swept`.
fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], radii: &[f64], swept: bool) {
    let edges = picks(k, b, at);
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, radii, &xs, 1, 1.0)
            .unwrap();
        s.set_swept_turns(swept);
        s.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
}

/// The faces the last blend made.
fn made(k: &Kernel) -> Vec<MadeBlendFace> {
    k.blend_face_log.as_ref().unwrap().last().to_vec()
}

/// One case's result: the bare faces among those its blend made.
struct Case {
    name: &'static str,
    bare: Vec<BareBlendFace>,
}

fn checked(name: &'static str, b: &Brep, made: &[MadeBlendFace]) -> Case {
    assert!(!made.is_empty(), "{name}: the blend made no face");
    let bare = bare_blend_faces(b, made).unwrap();
    let kinds: BTreeSet<String> = made.iter().map(|m| m.kind.name()).collect();
    eprintln!(
        "{name}: {} faces made ({kinds:?}), {} bare",
        made.len(),
        bare.len()
    );
    for f in &bare {
        eprintln!(
            "  bare face {} ({}, {})",
            f.face.index(),
            f.kind.name(),
            f.surface
        );
    }
    Case { name, bare }
}

fn edge_round(xsect: u32, continuity: u32) -> (Kernel, Brep) {
    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    let thumbweight = if xsect == 2 {
        solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT
    } else {
        1.0
    };
    fillet_edges(&mut k, &mut b, &[e], 5.0, xsect, continuity, thumbweight).unwrap();
    (k, b)
}

const CORNER: [[f64; 3]; 3] = [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]];

const BOSS_BASE: [[f64; 3]; 4] = [
    [20.0, 10.0, 10.0],
    [25.0, 15.0, 10.0],
    [20.0, 20.0, 10.0],
    [15.0, 15.0, 10.0],
];

fn boss_base(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let boss = create_box(k, Point3::new(15.0, 10.0, 10.0), 10.0, 10.0, 5.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

fn peg_key(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
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
        key: ObjectKey(((kind + 1) << 32) | u64::from(i)),
    }
}

/// #2256's tangent peg (`data/issue_2256_peg_step_reimported.smb`).
fn peg(k: &mut Kernel) -> Brep {
    use solidcore::topology::api_files::read_brep_from_file;
    use solidcore::topology::brep::p10::TopologyEnv;
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/issue_2256_peg_step_reimported.smb"
    ))
    .unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &peg_key,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(data), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// #2388's chamfered box: a 40 x 30 x 20 box, its top back edge rounded r 5, then the
/// round's edge on the top and the bottom front edge rounded r 2 by G2 sections.
fn chamfered_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let c = pick(k, &b, [20.0, 30.0, 20.0]);
    fillet_edges(k, &mut b, &[c], 5.0, 0, 2, 0.0).unwrap();
    let both = [
        pick(k, &b, [20.0, 25.0, 20.0]),
        pick(k, &b, [20.0, 0.0, 0.0]),
    ];
    fillet_edges(
        k,
        &mut b,
        &both,
        2.0,
        2,
        2,
        solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT,
    )
    .unwrap();
    b
}

/// Every case, each with the faces its last blend made and the bare ones.
fn cases() -> Vec<Case> {
    let mut out = Vec::new();

    let (k, b) = edge_round(1, 1);
    out.push(checked("box edge round r 5", &b, &made(&k)));
    let (k, b) = edge_round(2, 2);
    out.push(checked("box edge G2 round r 5", &b, &made(&k)));
    let (k, b) = edge_round(0, 1);
    out.push(checked("box edge chamfer 5", &b, &made(&k)));

    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    round(&mut k, &mut b, &CORNER, &[2.0; 3], false);
    out.push(checked("box corner, three rounds r 2", &b, &made(&k)));

    let mut k = kernel();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    round(&mut k, &mut b, &CORNER, &[1.0, 1.5, 2.0], false);
    out.push(checked("box corner, rounds r 1, 1.5, 2", &b, &made(&k)));

    // #2343: unequal rounds at a vertex beside a cylinder, the curved Setback (the D prism:
    // a cylinder R 10, 10 high, cut at x = 4; its top corner at (4, sqrt 84, 10))
    let mut k = kernel();
    let cylinder = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0).unwrap();
    let cut = create_box(&mut k, Point3::new(4.0, -20.0, -1.0), 20.0, 40.0, 12.0).unwrap();
    let mut b = boolean_difference(&mut k, cylinder, cut).unwrap();
    let h = 84.0_f64.sqrt();
    round(
        &mut k,
        &mut b,
        &[[4.0, 0.0, 10.0], [4.0, h, 5.0], [-10.0, 0.0, 10.0]],
        &[2.0, 2.5, 3.0],
        false,
    );
    out.push(checked(
        "D prism corner beside its cylinder, rounds r 2, 2.5, 3",
        &b,
        &made(&k),
    ));

    for swept in [false, true] {
        let mut k = kernel();
        let mut b = boss_base(&mut k);
        round(&mut k, &mut b, &BOSS_BASE, &[1.0; 4], swept);
        let name = if swept {
            "boss base r 1, swept turns"
        } else {
            "boss base r 1, mitred turns"
        };
        out.push(checked(name, &b, &made(&k)));
    }

    let mut k = kernel();
    let s = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let cut = create_box(&mut k, Point3::new(-20.0, -20.0, 4.0), 40.0, 40.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, s, cut).unwrap();
    let q = 42.0_f64.sqrt();
    round(
        &mut k,
        &mut b,
        &[[q, q, 4.0], [-q, -q, 4.0]],
        &[2.0; 2],
        false,
    );
    out.push(checked("cut ball's rim r 2", &b, &made(&k)));

    let mut k = kernel();
    let mut b = peg(&mut k);
    let es = picks(&k, &b, &[[20.0, 0.0, 10.0], [32.0, 8.0, 10.0]]);
    fillet_edges(&mut k, &mut b, &es, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    out.push(checked("tangent peg's mixed corner r 2", &b, &made(&k)));

    // #2388's own fix set: the offset of this body with every fix on is refused
    // (`offset_brep`, status 1001)
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes.fillet_round_as_definition = true;
    k.blend_face_log = Some(BlendFaceLog::default());
    let mut b = chamfered_box(&mut k);
    let log = k.blend_face_log.take().unwrap();
    // the faces of the last blend, and the earlier blends' faces it left (each a face
    // made by a blend, its kind the first maker's)
    let mut blends: Vec<MadeBlendFace> = Vec::new();
    for m in log.sessions.iter().rev().flatten() {
        let live = b.face(m.face).is_ok_and(|f| f.surface.is_some());
        if live && !blends.iter().any(|x| x.face == m.face) {
            blends.push(*m);
        }
    }
    mark_blend_faces(&mut b, &blends).unwrap();
    let offset = offset_brep_simple(
        &mut k,
        &mut b,
        OffsetOptions {
            distance: 0.5,
            extend_convex_edges: true,
            merge_results: true,
        },
    )
    .unwrap();
    let made = offsets_of_blends(&offset).unwrap();
    out.push(checked(
        "offset 0.5 of #2388's chamfered box",
        &offset,
        &made,
    ));

    // The mirrored-rim matrix grades these same captures and builder at every seed.
    // Keep the ledger's own evidence non-vacuous for its chamfer entry too.
    for edited in [false, true] {
        let (mut k, mut b) = if edited {
            mirrored_shell_rim::edited_input()
        } else {
            mirrored_shell_rim::input()
        };
        let (name, point, size) = if edited {
            let diagonal = 4.0 / 2.0_f64.sqrt();
            (
                "edited mirrored-rim chamfer",
                Point3::new(29.0 + diagonal, 19.0 + diagonal, 33.0),
                0.627,
            )
        } else {
            (
                "original mirrored-rim chamfer",
                Point3::new(3.8786796564403563, 36.121320343559645, 30.0),
                0.47,
            )
        };
        mirrored_shell_rim::build_rim(
            &mut k,
            &mut b,
            point,
            mirrored_shell_rim::Kind::Chamfer,
            size,
        )
        .unwrap();
        let case = checked(name, &b, &self::made(&k));
        assert_eq!(case.bare.len(), 4, "{name}: four cone strips remain bare");
        for bare in &case.bare {
            assert_eq!(bare.kind, BlendFaceKind::Chamfer);
            assert_eq!(bare.surface, "cone");
        }
        out.push(case);
    }

    out
}

/// The ledger: each kind not a definition yet, with its issue (the first `#n` of its
/// file), by the kind's name (the file's stem, `/` written `-`).
fn ledger() -> BTreeMap<String, u32> {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/blend_definition_ledger"
    );
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        let issue = text
            .split_whitespace()
            .find_map(|w| w.strip_prefix('#'))
            .and_then(|n| {
                n.trim_end_matches(|c: char| !c.is_ascii_digit())
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| panic!("{stem}: the ledger entry names no issue"));
        out.insert(stem, issue);
    }
    out
}

fn ledger_name(kind: BlendFaceKind) -> String {
    kind.name().replace('/', "-")
}

/// The check over every case against the ledger: no bare face of an unlisted kind, and no
/// listed kind that no case makes bare.
#[test]
fn every_bare_blend_face_is_of_a_kind_the_ledger_lists() {
    let ledger = ledger();
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unlisted = Vec::new();
    for case in cases() {
        for bare in &case.bare {
            let name = ledger_name(bare.kind);
            let what = format!(
                "{}: face {} ({}, {})",
                case.name,
                bare.face.index(),
                bare.kind.name(),
                bare.surface
            );
            if !ledger.contains_key(&name) {
                unlisted.push(what.clone());
            }
            seen.entry(name).or_default().push(what);
        }
    }
    assert!(
        unlisted.is_empty(),
        "bare blend faces of kinds the ledger does not list: {unlisted:#?}"
    );
    let fixed: Vec<String> = ledger
        .iter()
        .filter(|(name, _)| !seen.contains_key(*name))
        .map(|(name, issue)| format!("{name} (#{issue})"))
        .collect();
    assert!(
        fixed.is_empty(),
        "listed kinds no case makes bare any more (fixed? delete their ledger files): {fixed:?}"
    );
}

/// The log changes nothing: the same blend with the log off gives the same faces, each
/// with the same surface kind, and leaves no mark on the body.
#[test]
fn the_log_changes_no_result() {
    let (k, logged) = edge_round(1, 1);
    assert!(!made(&k).is_empty());
    let mut k = kernel();
    k.blend_face_log = None;
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    fillet_edges(&mut k, &mut b, &[e], 5.0, 1, 1, 1.0).unwrap();
    let kinds = |b: &Brep| -> Vec<(u32, bool)> {
        b.face_list()
            .iter()
            .map(|&f| {
                let s = b.face(f).unwrap().surface.as_ref().unwrap();
                (f.index(), is_definition(s))
            })
            .collect()
    };
    assert_eq!(kinds(&logged), kinds(&b));
    let marks = |b: &Brep| {
        b.face_list()
            .iter()
            .filter(|&&f| {
                b.find_attribute(
                    EntityRef::Face(f),
                    solidcore::geom::attribute::ids::FILLET_BLEND_FACE,
                )
                .is_some()
            })
            .count()
    };
    assert_eq!(marks(&logged), 0);
}
