//! tangency-labs/solidcore#2369 (smlib#104, WR-616): three curvature-continuous (G2) rounds
//! of radius 2 meeting at a corner where every edge is rounded, all convex or all concave,
//! with every face there a plane, are the Setback's to fill with `fillet_blend_corner`.
//! This matches the fork's `SmBlendCorner::ClassifySetBack` (tangency-labs/smlib `b5bb747`:
//! rounds of one kind, all `SM_FSG_BLEND_CURVE`, are G2, so `SM_BCE_SETBACK` with fill order
//! 2). Each corner builds a valid body. Without the flag the pinned `SmFilletConvexNxNCorner`
//! builds it, as before.
//!
//! The bodies are SolidCraft's kernel-rig inputs (brett19/solidcraft `b47da26e8`,
//! `perf-inputs/2369-g2-corners/*/before-fillet.smb`), each taken just before its fillet.
//! They are test data only:
//! - `data/issue_2369_g2_bar_corner.smb` (sha256 `f3bff138...cc6d`): a bar 40 x 16 x 10 whose
//!   far end is a half disc R8 about (40, 8), with a disc R8 on its top extruded 10 up and
//!   joined. The three edges at its top corner (0, 0, 10) are rounded
//!   (SolidCraft case report-fillet-curvature-three-edges-at-bar-corner).
//! - `data/issue_2369_g2_bar_corner_residues.smb` (sha256 `e4546903...0e3e`): the same bar
//!   with the sketch solver's residues of about 1e-10, its corner at (0.489294, 0.057925, 10)
//!   (report-fillet-curvature-three-edges-at-bar-corner-solver-residues, and with the setback
//!   cube cut away afterwards, ...-outside-setback).
//! - `data/issue_2369_g2_pocket_corner.smb` (sha256 `1f512da3...2ab5`): a 60 x 40 x 10 plate
//!   less a 30 x 20 x 5 pocket from (15, 10, 5). Its two floor edges and the upright edge at
//!   (15, 10, 5) are concave (cov-fillet-g2-pocket-corner).
//!
//! The outside-setback case's value is closed-form, quoted from SolidCraft's
//! `expected.json`: V = V0 - 27 - (the three sections outside the cube) = 9526.705048390. It
//! is checked to 1e-9 relative. The others pin what the port builds (measured; flag on: the
//! Setback's G2 fill, 16, 16 and 20 faces). SolidCraft's expected values for them (9361.3087
//! within 0.29%, 9545.9794 within 0.28%, the pocket by its invariants only) are graded by
//! SolidCraft, not here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helpers

use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletCornerType;
use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId, VertexId};
use solidcore::EntityRef;

fn key_of(e: EntityRef) -> ObjectRef {
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

/// Every opt-in fix on, with `fillet_blend_corner` (WR-616) on or off.
fn kernel(blend: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = UpstreamFixes::all();
    k.fixes.fillet_blend_corner = blend;
    k
}

fn read(k: &mut Kernel, file: &str) -> Brep {
    let data = std::fs::read(format!("{}/tests/data/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(data), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The vertex the three picked edges share.
fn corner_vertex(b: &Brep, es: &[EdgeId]) -> VertexId {
    let ends = |e: EdgeId| edge::vertices(b, e, None).unwrap();
    let (first, rest) = es.split_first().unwrap();
    ends(*first)
        .into_iter()
        .find(|v| rest.iter().all(|&e| ends(e).contains(v)))
        .unwrap()
        .unwrap()
}

/// What the corner was made as.
type Made = (FilletCornerType, Option<BlendCornerEvent>);

/// SolidCraft's `curvature` fillet (its `blend_edges`): the picked edges rounded at radius 2
/// with the G2 section at the equal-area thumbweight, the corners created, then filleted
/// and stitched.
fn round(blend: bool, file: &str, picks: &[[f64; 3]]) -> (Kernel, Brep, Made) {
    round_kept(blend, true, file, picks)
}

/// [`round`], with `definition` (`fillet_round_as_definition`, #2488) set.
fn round_kept(
    blend: bool,
    definition: bool,
    file: &str,
    picks: &[[f64; 3]],
) -> (Kernel, Brep, Made) {
    let mut k = kernel(blend);
    k.fixes.fillet_round_as_definition = definition;
    let mut b = read(&mut k, file);
    let es: Vec<EdgeId> = picks.iter().map(|p| pick(&k, &b, *p)).collect();
    let vtx = corner_vertex(&b, &es);
    let made = {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &es,
            &[2.0],
            &[FilletSurfaceGeneratorType::BlendCurve],
            2,
            GN_DEFAULT_THUMBWEIGHT,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        let mut made = None;
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            if fc.vertex == Some(vtx) {
                made = Some((fc.corner_type(), fc.blend.as_ref().map(|b| b.event)));
            }
        }
        s.do_filleting(&mut k, None).unwrap();
        made.unwrap()
    };
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    (k, b, made)
}

const BAR: [[f64; 3]; 3] = [[0.0, 0.0, 4.0], [20.0, 0.0, 10.0], [0.0, 8.0, 10.0]];
const RESIDUES: [[f64; 3]; 3] = [
    [0.4892940775280783, 0.0579, 4.0],
    [21.53, 0.0579, 10.0],
    [0.4892940775280783, 8.058, 10.0],
];
const POCKET: [[f64; 3]; 3] = [[30.0, 10.0, 5.0], [15.0, 20.0, 5.0], [15.0, 10.0, 7.5]];

/// The outside-setback case's closed form (SolidCraft's expected.json).
const OUTSIDE_SETBACK: f64 = 9526.705048390453;

/// The body has volume `want` to 1e-9 relative, is valid and has `faces` faces.
fn assert_built(k: &mut Kernel, b: &mut Brep, faces: usize, want: f64) {
    // the volume before the check, which may refine the body's tolerances
    let v = volume(k, b);
    assert!(check_brep(k, b, CheckLevel::Level2).unwrap().is_valid());
    assert_eq!(b.faces().ids().count(), faces);
    assert!(
        (v - want).abs() <= 1e-9 * v.abs().max(want.abs()),
        "volume {v:.12}, want {want:.12}"
    );
}

fn assert_setback(made: Made) {
    assert_eq!(made.1, Some(BlendCornerEvent::Setback));
}

fn assert_pinned(made: Made) {
    assert_eq!(made, (FilletCornerType::NByNConvex, None));
}

/// The residues bar rounded, less the cube [-1, 3]^2 x [7, 11] about its corner: the three
/// G2 rounds outside the setback, exactly.
fn outside_setback(blend: bool) -> (Kernel, Brep, Made) {
    let (mut k, b, made) = round(blend, "issue_2369_g2_bar_corner_residues.smb", &RESIDUES);
    let cube = create_box(
        &mut k,
        Point3::new(-0.5107059224719217, -0.9420745494701371, 7.0),
        4.0,
        4.0,
        4.0,
    )
    .unwrap();
    let mut cut = boolean_difference(&mut k, b, cube).unwrap();
    cut.refresh_cached_topology();
    (k, cut, made)
}

#[test]
fn outside_setback_is_the_closed_form_with_the_setback() {
    let (mut k, mut b, made) = outside_setback(true);
    assert_setback(made);
    assert_built(&mut k, &mut b, 13, OUTSIDE_SETBACK);
}

#[test]
fn outside_setback_is_the_closed_form_without_the_flag() {
    let (mut k, mut b, made) = outside_setback(false);
    assert_pinned(made);
    assert_built(&mut k, &mut b, 13, OUTSIDE_SETBACK);
}

#[test]
fn bar_corner_is_the_g2_setback_fill() {
    let (mut k, mut b, made) = round(true, "issue_2369_g2_bar_corner.smb", &BAR);
    assert_setback(made);
    assert_built(&mut k, &mut b, 16, 9364.160085727);
}

#[test]
fn bar_corner_without_the_flag_is_the_pinned_corner() {
    let (mut k, mut b, made) = round(false, "issue_2369_g2_bar_corner.smb", &BAR);
    assert_pinned(made);
    assert_built(&mut k, &mut b, 11, 9362.339740228);
}

#[test]
fn residues_bar_corner_is_the_g2_setback_fill() {
    let (mut k, mut b, made) = round(true, "issue_2369_g2_bar_corner_residues.smb", &RESIDUES);
    assert_setback(made);
    assert_built(&mut k, &mut b, 16, 9548.830815317);
}

#[test]
fn residues_bar_corner_without_the_flag_is_the_pinned_corner() {
    let (mut k, mut b, made) = round(false, "issue_2369_g2_bar_corner_residues.smb", &RESIDUES);
    assert_pinned(made);
    assert_built(&mut k, &mut b, 11, 9547.010488401);
}

#[test]
fn pocket_corner_is_the_g2_setback_fill() {
    let (mut k, mut b, made) = round(true, "issue_2369_g2_pocket_corner.smb", &POCKET);
    assert_setback(made);
    assert_built(&mut k, &mut b, 20, 21044.360971410);
}

#[test]
fn pocket_corner_without_the_flag_is_the_pinned_corner() {
    let (mut k, mut b, made) = round(false, "issue_2369_g2_pocket_corner.smb", &POCKET);
    assert_pinned(made);
    assert_built(&mut k, &mut b, 15, 21046.181316782);
}

// ---- #2488: a Setback's fill patches keep the fill's definition ----

use solidcore::surface::procedural_blend::convert::{corner_rebuild, corner_to_record};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::procedural_blend::record::{CornerRecord, SupportRecord};
use solidcore::surface::procedural_blend::BlendNet;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::face::p2 as face2;

/// The faces' surfaces whose definition is a corner's of `kind`.
fn corner_faces(b: &Brep, kind: CornerKind) -> Vec<Surface> {
    b.faces()
        .ids()
        .filter_map(|f| face2::surface(b, f).unwrap().cloned())
        .filter(|s| {
            s.procedural_blend()
                .and_then(|d| d.corner())
                .is_some_and(|c| c.kind() == kind)
        })
        .collect()
}

/// Every face's surface over a 9 x 9 grid of its natural domain, as bits; a round that is
/// a procedural blend by its net (its own definition is #2480's, and evaluates by solving
/// it).
fn face_bits(k: &Kernel, b: &Brep) -> Vec<Vec<[u64; 3]>> {
    b.faces()
        .ids()
        .filter_map(|f| face2::surface(b, f).unwrap())
        .map(|s| {
            let s = match s.kind() {
                SurfaceKind::ProceduralBlend(pb) => Surface::from(pb.net().copy()),
                _ => s.clone(),
            };
            let d = s.natural_uv_domain_with_nurb(k).unwrap();
            let mut pts = Vec::new();
            for i in 0..=8 {
                for j in 0..=8 {
                    let u = d.min().x + (d.max().x - d.min().x) * f64::from(i) / 8.0;
                    let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0;
                    let p = s
                        .evaluate_point(k, solidcore::geom::vector2d::Point2::new(u, v))
                        .unwrap();
                    pts.push([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
                }
            }
            pts
        })
        .collect()
}

const CASES: [(&str, &[[f64; 3]]); 2] = [
    ("issue_2369_g2_bar_corner.smb", &BAR),
    ("issue_2369_g2_pocket_corner.smb", &POCKET),
];

#[test]
fn the_setback_fills_are_their_definitions() {
    for (file, picks) in CASES {
        let (k, mut b, made) = round_kept(true, true, file, picks);
        assert_setback(made);
        let patches = corner_faces(&b, CornerKind::Setback);
        assert_eq!(patches.len(), 6, "{file}");
        for s in &patches {
            let d = s.procedural_blend().unwrap();
            assert!(matches!(d.net(), BlendNet::Approx(_)));
            // the fill's tolerance bounds it; where built, the net is the rule's patch
            assert!(d.bound() > 0.0 && d.bound() < 1e-3, "{}", d.bound());
            assert_eq!(d.order(), 2);
            assert!((0..3).all(|i| d.support(i).is_some()));
            assert_eq!(d.recertify(&k).unwrap(), 0.0);
        }
        // the volume and every surface as without the definition, to the rounding of the part
        // (the flag also makes the G2 rounds procedural blends, #2480, evaluated by their
        // definitions, and the fill reads the rounds' own evaluations; the corner's definition
        // itself changes no number: its build makes the fill again and requires it bit for bit)
        let (k0, mut b0, _) = round_kept(true, false, file, picks);
        assert!(corner_faces(&b0, CornerKind::Setback).is_empty());
        let (v, v0) = (volume(&k, &mut b), volume(&k0, &mut b0));
        eprintln!("{file}: volume {v:.17e}, without the definition {v0:.17e}");
        assert!((v - v0).abs() <= 1e-12 * v0, "{v} against {v0}");
        let (on, off) = (face_bits(&k, &b), face_bits(&k0, &b0));
        assert_eq!(on.len(), off.len());
        let kinds: Vec<String> = b
            .faces()
            .ids()
            .filter_map(|f| face2::surface(&b, f).unwrap())
            .map(|s| format!("{:?}", std::mem::discriminant(s.kind())))
            .collect();
        for (i, (x, y)) in on.iter().zip(&off).enumerate() {
            let worst = x
                .iter()
                .zip(y)
                .map(|(p, q)| {
                    (0..3)
                        .map(|c| (f64::from_bits(p[c]) - f64::from_bits(q[c])).abs())
                        .fold(0.0, f64::max)
                })
                .fold(0.0, f64::max);
            // (64 ulps of the part's size, 40)
            let tol = 64.0 * f64::EPSILON * 40.0;
            assert!(
                worst <= tol,
                "{file}: face {i} ({}) differs by {worst:e}",
                kinds[i]
            );
        }
    }
}

#[test]
fn a_setback_fill_round_trips_through_its_record() {
    let (k, b, _) = round_kept(true, true, "issue_2369_g2_bar_corner.smb", &BAR);
    for s in corner_faces(&b, CornerKind::Setback) {
        let rec = corner_to_record(&k, &s).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        let back = corner_rebuild(&k, &serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(corner_to_record(&k, &back).unwrap(), rec);
        let d = back.procedural_blend().unwrap().recertify(&k).unwrap();
        eprintln!("rebuilt: {d:e} from the fill made again");
    }
}

#[test]
fn a_moved_setback_fill_is_certified_again() {
    let (k, b, _) = round_kept(true, true, "issue_2369_g2_bar_corner.smb", &BAR);
    for s in corner_faces(&b, CornerKind::Setback) {
        let mut s = s.copy(&k).unwrap();
        let mut place = solidcore::geom::axis2_placement::Axis2Placement::default();
        place.rotate_about_axis(
            0.3,
            solidcore::math::Vec3::new(1.0, 2.0, 3.0).unitize().unwrap(),
        );
        place.translate(solidcore::math::Vec3::new(1e2, -2e2, 5e1));
        s.transform(&k, &place, None).unwrap();
        let d = s.procedural_blend().unwrap();
        let proved = d.recertify(&k).unwrap();
        eprintln!("moved: {proved:e}");
        assert!(proved <= d.bound());
    }
}

#[test]
fn a_setback_fill_off_its_rounds_is_refused() {
    let (k, b, _) = round_kept(true, true, "issue_2369_g2_bar_corner.smb", &BAR);
    let s = &corner_faces(&b, CornerKind::Setback)[0];
    let CornerRecord::Setback(rec) = corner_to_record(&k, s).unwrap() else {
        panic!("not a setback's record");
    };
    // a round's surface moved: the rule makes another fill
    let mut off = rec.clone();
    let moved = match &mut off.rounds[0].surface {
        SupportRecord::Nurbs { form, .. } => &mut form.points,
        SupportRecord::Blend(b) => &mut b.net.points,
    };
    for p in moved.iter_mut() {
        p[0] += 1e-3;
    }
    assert!(corner_rebuild(&k, &CornerRecord::Setback(off)).is_err());
    // the net moved off its fill
    let mut off = rec;
    for p in &mut off.net.points {
        p[2] += 1e-3;
    }
    let why = corner_rebuild(&k, &CornerRecord::Setback(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("not its fill's"), "{why}");
}
