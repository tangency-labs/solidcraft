//! WR-697 (smlib#79 step 4b, the fork's tangency-labs/smlib@e647dac, 1fcbdb7, af7a4d9):
//! with the session's swept turns (`FilletSession::set_swept_turns`, the fork's
//! `SetSweptTurns`) the blend corner's Turn is Swept: each round ends on its own section
//! and the corner's face is the end section revolved about the side edge, an exact torus.
//!
//! The fork's capi case `turn swept_boss_base`: a 40 x 30 x 10 plate with a 10 x 10 x 5
//! boss on it (x 15..25, y 10..20), the boss's four base edges rounded r 1 together. At
//! each vertical boss edge the two rounds' end sections at (cx, cy, 11) turn about the
//! edge: a horn torus of radii 1, 1 centred on the edge at (cx, cy, 11). The fork's notes
//! give the closed form `12500 + 40 A + 2 pi M = 12508.885259020` and its own volume
//! 12508.885258803. The fork 5e8c9c1, built locally and run as capi's `round_edges` calls
//! the executive (`SetSweptTurns(TRUE)` before `CreateFilletCorners`), prints volume
//! 12508.885258803208, 19 faces and four tori of radii 1, 1. Refused there: the same
//! rounds along a pocket's rim (a sharp concave side edge, `SM_FILERR_NO_ROOM`, 8) and
//! unequal radii 1 and 1.5 (`SM_FILERR_CORNER_UNSUPPORTED`, 10 there, 9 here), both at
//! `CreateFilletCorners` with `SM_ERR`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`.
fn all_fixes() -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> EdgeId {
    let p = Point3::new(at[0], at[1], at[2]);
    let mut best: Option<(EdgeId, f64)> = None;
    for e in get_edges(b).unwrap() {
        let Some(c) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let ivl = edge::interval(b, e).unwrap();
        let d = c
            .drop_point(
                k,
                &ivl,
                p,
                None,
                BIG_DOUBLE,
                None,
                SolverOperation::Minimize,
            )
            .unwrap();
        if d.success && d.distance < 0.1 && best.is_none_or(|(_, bd)| d.distance < bd) {
            best = Some((e, d.distance));
        }
    }
    best.unwrap().0
}

fn boss_base(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let boss = create_box(k, Point3::new(15.0, 10.0, 10.0), 10.0, 10.0, 5.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

fn pocket(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let cut = create_box(k, Point3::new(10.0, 10.0, 5.0), 20.0, 10.0, 6.0).unwrap();
    boolean_difference(k, plate, cut).unwrap()
}

const BASE: [[f64; 3]; 4] = [
    [20.0, 10.0, 10.0],
    [25.0, 15.0, 10.0],
    [20.0, 20.0, 10.0],
    [15.0, 15.0, 10.0],
];

const RIM: [[f64; 3]; 4] = [
    [20.0, 10.0, 5.0],
    [30.0, 15.0, 5.0],
    [20.0, 20.0, 5.0],
    [10.0, 15.0, 5.0],
];

/// Rounds the edges at `at`, swept turns on: the error code and reason on failure.
fn round(
    k: &mut Kernel,
    b: &mut Brep,
    at: &[[f64; 3]],
    radii: &[f64],
) -> Result<(), (i32, String)> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, radii, &xs, 1, 1.0)
            .unwrap();
        s.set_swept_turns(true);
        if s.do_fillet(k).is_err() {
            let info = s.error_info();
            return Err((info.code, info.comment.clone()));
        }
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Ok(())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The tori among `b`'s faces: (major, minor) radii.
fn tori(b: &Brep) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        if let Some(s) = face2::surface(b, f).unwrap() {
            if let SurfaceKind::Torus(t) = s.kind() {
                out.push((t.major_radius(), t.minor_radius()));
            }
        }
    }
    out
}

/// The fork 5e8c9c1's volume of the swept boss base (run locally).
const FORK_SWEPT_VOLUME: f64 = 12_508.885_258_803_208;

#[test]
fn boss_base_swept_turns_are_horn_tori() {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    let v = volume(&k, &mut b);
    let faces = get_faces(&b).unwrap().len();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let tori = tori(&b);
    eprintln!("volume {v:.12}, {faces} faces, tori {tori:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(faces, 19);
    assert_eq!(tori.len(), 4);
    for (major, minor) in tori {
        assert!((major - 1.0).abs() <= 1e-12 && (minor - 1.0).abs() <= 1e-12);
    }
    assert!(
        (v - FORK_SWEPT_VOLUME).abs() <= 1e-9 * FORK_SWEPT_VOLUME,
        "{v}"
    );
}

#[test]
fn a_pocket_rim_is_refused_naming_the_radius() {
    let mut k = all_fixes();
    let mut b = pocket(&mut k);
    let (code, why) = round(&mut k, &mut b, &RIM, &[1.0; 4]).unwrap_err();
    assert_eq!(code, FilletErrorType::NoRoom as i32);
    assert!(
        why.contains("round that edge at a radius of at least 1 first"),
        "{why}"
    );
}

/// The fork's `NoteFitRadius(0.0, edge)` (5e8c9c1) at the pocket rim's sharp concave side
/// edge: no radius builds the swept turn, so the refusal's fit size is 0, for the first
/// round's edge.
#[test]
fn a_pocket_rim_notes_that_no_radius_fits() {
    let mut k = all_fixes();
    let mut b = pocket(&mut k);
    let edges: Vec<EdgeId> = RIM.iter().map(|&p| pick(&k, &b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_per_edge_constant_radius(&mut k, &edges, &[1.0; 4], &xs, 1, 1.0)
        .unwrap();
    s.set_swept_turns(true);
    assert!(s.do_fillet(&mut k).is_err());
    let fit = s.fit_size().expect("a fit size is noted");
    eprintln!("{fit:?}");
    assert_eq!(fit.size, 0.0);
    assert!(fit.edge.is_some_and(|e| edges.contains(&e)), "{fit:?}");
}

#[test]
fn unequal_rounds_are_not_swept_yet() {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    let (code, why) = round(&mut k, &mut b, &BASE, &[1.0, 1.5, 1.0, 1.5]).unwrap_err();
    // the fork's SM_FILERR_CORNER_UNSUPPORTED (10 there: this port has no 9 before it)
    assert_eq!(code, FilletErrorType::CornerUnsupported as i32);
    assert!(why.contains("constant rounds of one radius"), "{why}");
}

/// The swept boss base's torus faces turned round (`SmFace::SwapUV`, which keeps each
/// torus's definition, WR-701) `times` times: the volume, check lines and tori after, and
/// each torus face's surface then rebuilt from its NURB (`Surface::reverse` along u runs
/// the torus's `RebuildSTEPFromNURBParameters` on the swapped net): its radii.
type Turned = (f64, Vec<String>, Vec<(f64, f64)>, Vec<(f64, f64)>);

fn turned_round(times: usize) -> Turned {
    let mut k = all_fixes();
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    let torus_faces: Vec<_> = get_faces(&b)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            face2::surface(&b, f)
                .unwrap()
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Torus(_)))
        })
        .collect();
    assert_eq!(torus_faces.len(), 4);
    for _ in 0..times {
        for &f in &torus_faces {
            solidcore::topology::face::p1::swap_uv(&k, &mut b, f).unwrap();
        }
    }
    let rebuilt = torus_faces
        .iter()
        .map(|&f| {
            let mut s = face2::surface(&b, f).unwrap().unwrap().clone();
            s.reverse(&k, SurfParamType::U).unwrap();
            let SurfaceKind::Torus(t) = s.kind() else {
                panic!("rebuilt as {:?}", s.kind());
            };
            (t.major_radius(), t.minor_radius())
        })
        .collect();
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    (v, lines, tori(&b), rebuilt)
}

/// The fork's af7a4d9 (`turn swept_tori_turned_round`): a horn torus turned round stays
/// the horn torus it was, its faces' edges on it, and rebuilt from its swapped NURB it is
/// the horn torus again.
#[test]
fn horn_tori_turned_round_stay_horn_tori() {
    for times in [1, 2] {
        let (v, lines, tori, rebuilt) = turned_round(times);
        eprintln!("turned {times}: volume {v:.12}, tori {tori:?}, rebuilt {rebuilt:?}, {lines:?}");
        assert!(lines.is_empty(), "{lines:?}");
        for (major, minor) in tori.into_iter().chain(rebuilt) {
            assert!((major - minor).abs() <= 0.0, "{major} {minor}");
        }
        assert!(
            (v - FORK_SWEPT_VOLUME).abs() <= 1e-9 * FORK_SWEPT_VOLUME,
            "{v}"
        );
    }
}

// ---- #2488: a Swept turn's torus keeps its definition ----

use solidcore::surface::procedural_blend::convert::{corner_rebuild, corner_to_record};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::procedural_blend::record::{CornerRecord, SupportRecord};
use solidcore::surface::procedural_blend::BlendNet;
use solidcore::surface::surface::Surface;

/// The swept boss base, with or without `fillet_round_as_definition`.
fn swept_boss_base(definition: bool) -> (Kernel, Brep) {
    let mut k = all_fixes();
    k.fixes.fillet_round_as_definition = definition;
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    (k, b)
}

/// The faces' surfaces whose definition is a corner's of `kind`.
fn corner_faces(b: &Brep, kind: CornerKind) -> Vec<Surface> {
    get_faces(b)
        .unwrap()
        .into_iter()
        .filter_map(|f| face2::surface(b, f).unwrap().cloned())
        .filter(|s| {
            s.procedural_blend()
                .and_then(|d| d.corner())
                .is_some_and(|c| c.kind() == kind)
        })
        .collect()
}

/// Every face's surface over a 9 x 9 grid of its natural domain, as bits.
fn face_bits(k: &Kernel, b: &Brep) -> Vec<Vec<[u64; 3]>> {
    get_faces(b)
        .unwrap()
        .into_iter()
        .filter_map(|f| face2::surface(b, f).unwrap())
        .map(|s| {
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

/// `rec` with its first NURBS support's form moved by `by` along x, y and z.
fn moved(rec: &SupportRecord, by: f64) -> SupportRecord {
    let mut rec = rec.clone();
    if let SupportRecord::Nurbs { form, .. } = &mut rec {
        let w = form.weights.clone();
        for (i, p) in form.points.iter_mut().enumerate() {
            let wi = w.as_ref().map_or(1.0, |w| w[i]);
            for x in p.iter_mut() {
                *x += by * wi;
            }
        }
    }
    rec
}

#[test]
fn the_swept_tori_are_their_definitions() {
    let (k, mut b) = swept_boss_base(true);
    let tori = corner_faces(&b, CornerKind::Swept);
    assert_eq!(tori.len(), 4);
    for s in &tori {
        assert!(matches!(s.kind(), SurfaceKind::Torus(_)));
        let d = s.procedural_blend().unwrap();
        assert!(matches!(d.net(), BlendNet::Exact(_)));
        assert_eq!(d.bound(), 0.0);
        assert_eq!(d.order(), 1);
        assert!(d.support(0).is_some());
        assert_eq!(d.recertify(&k).unwrap(), 0.0);
    }
    // the volume and every surface bit for bit as without the definition
    let (k0, mut b0) = swept_boss_base(false);
    assert!(corner_faces(&b0, CornerKind::Swept).is_empty());
    assert_eq!(volume(&k, &mut b).to_bits(), volume(&k0, &mut b0).to_bits());
    assert_eq!(face_bits(&k, &b), face_bits(&k0, &b0));
}

#[test]
fn a_swept_torus_round_trips_through_its_record() {
    let (k, b) = swept_boss_base(true);
    for s in corner_faces(&b, CornerKind::Swept) {
        let rec = corner_to_record(&k, &s).unwrap();
        let json = serde_json::to_string(&rec).unwrap();
        let back = corner_rebuild(&k, &serde_json::from_str(&json).unwrap()).unwrap();
        assert!(matches!(back.kind(), SurfaceKind::Torus(_)));
        assert_eq!(corner_to_record(&k, &back).unwrap(), rec);
        assert_eq!(back.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }
}

#[test]
fn a_swept_torus_off_its_common_face_or_its_axis_is_refused() {
    let (k, b) = swept_boss_base(true);
    let s = &corner_faces(&b, CornerKind::Swept)[0];
    let CornerRecord::Swept(rec) = corner_to_record(&k, s).unwrap() else {
        panic!("not a swept record");
    };
    // the common face moved apart from the torus
    let mut off = rec.clone();
    off.common = moved(&rec.common, 1e-3);
    let why = corner_rebuild(&k, &CornerRecord::Swept(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("do not touch its common face"), "{why}");
    // the side edge moved: the torus is not its end section's about it
    let mut off = rec.clone();
    off.axis_point[0] += 1e-3;
    let why = corner_rebuild(&k, &CornerRecord::Swept(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("side edge"), "{why}");
    // the net moved off its definition
    let mut off = rec;
    off.net = moved(&off.net, 1e-3);
    let why = corner_rebuild(&k, &CornerRecord::Swept(off))
        .unwrap_err()
        .to_string();
    assert!(why.contains("torus"), "{why}");
}

#[test]
fn a_moved_swept_torus_is_certified_again() {
    let (k, b) = swept_boss_base(true);
    for s in corner_faces(&b, CornerKind::Swept) {
        let mut s = s.copy(&k).unwrap();
        let mut place = solidcore::geom::axis2_placement::Axis2Placement::default();
        place.rotate_about_axis(
            0.3,
            solidcore::math::Vec3::new(1.0, 2.0, 3.0).unitize().unwrap(),
        );
        place.translate(solidcore::math::Vec3::new(1e2, -2e2, 5e1));
        s.transform(&k, &place, None).unwrap();
        assert_eq!(s.procedural_blend().unwrap().recertify(&k).unwrap(), 0.0);
    }
}
