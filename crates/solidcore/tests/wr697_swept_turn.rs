//! WR-697 (smlib#79 step 4b, the fork's tangency-labs/smlib@e647dac, 1fcbdb7, af7a4d9):
//! with the session's swept turns (`FilletSession::set_swept_turns`, the fork's
//! `SetSweptTurns`) the blend corner's Turn is Swept: each round ends on its own section
//! and the corner's face is the end section revolved about the side edge, an exact torus
//! (`Kernel::fixes.fillet_blend_corner`).
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
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// Every fix but `geometric_g2_continuity`, with or without `fillet_blend_corner`.
fn all_fixes(blend: bool) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
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
    let mut k = all_fixes(true);
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
fn without_the_flag_the_swept_turns_are_mitres() {
    let mut k = all_fixes(false);
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    assert_eq!(get_faces(&b).unwrap().len(), 15);
    assert!(tori(&b).is_empty());
}

#[test]
fn a_pocket_rim_is_refused_naming_the_radius() {
    let mut k = all_fixes(true);
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
    let mut k = all_fixes(true);
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
    let mut k = all_fixes(true);
    let mut b = boss_base(&mut k);
    let (code, why) = round(&mut k, &mut b, &BASE, &[1.0, 1.5, 1.0, 1.5]).unwrap_err();
    // the fork's SM_FILERR_CORNER_UNSUPPORTED (10 there: this port has no 9 before it)
    assert_eq!(code, FilletErrorType::CornerUnsupported as i32);
    assert!(why.contains("constant rounds of one radius"), "{why}");
}

/// The swept boss base's torus faces turned round (`SmFace::SwapUV`, rebuilding each torus
/// from its NURB) `times` times: the volume, check lines and tori after.
fn turned_round(fixed: bool, times: usize) -> (f64, Vec<String>, Vec<(f64, f64)>) {
    let mut k = all_fixes(true);
    k.fixes.fillet_blend_corner = true;
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    k.fixes.fillet_blend_corner = fixed;
    // the rebuild from the NURB is what `SwapUV` runs without WR-701's
    // `swap_uv_keeps_analytic_definition` (which keeps the analytic torus instead)
    k.fixes.swap_uv_keeps_analytic_definition = false;
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
    let v = volume(&k, &mut b);
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    (v, lines, tori(&b))
}

/// The fork's af7a4d9 (`turn swept_tori_turned_round`): a horn torus rebuilt from its
/// NURB after `SwapUV` stays the horn torus it was, its faces' edges on it.
#[test]
fn horn_tori_turned_round_stay_horn_tori() {
    for times in [1, 2] {
        let (v, lines, tori) = turned_round(true, times);
        eprintln!("turned {times}: volume {v:.12}, tori {tori:?}, {lines:?}");
        assert!(lines.is_empty(), "{lines:?}");
        for (major, minor) in tori {
            assert!((major - minor).abs() <= 0.0, "{major} {minor}");
        }
        assert!(
            (v - FORK_SWEPT_VOLUME).abs() <= 1e-9 * FORK_SWEPT_VOLUME,
            "{v}"
        );
    }
}

#[test]
fn without_the_flag_turned_horn_tori_are_rebuilt_as_upstream() {
    // upstream's rebuild: the start directions from the NURB's corner points on the axis
    // (zero), a spindle torus whose edges are up to 1.41 off it
    let (v, lines, tori) = turned_round(false, 1);
    eprintln!("unfixed: volume {v:.12}, tori {tori:?}, {lines:?}");
    assert!(lines.iter().any(|l| l.contains("Bad Gap")), "{lines:?}");
    assert!(
        tori.iter().any(|&(major, minor)| major != minor),
        "{tori:?}"
    );
}
