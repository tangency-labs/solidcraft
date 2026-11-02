//! WR-697 (smlib#79 step 4b, the fork's tangency-labs/smlib@32fd80a): the blend
//! corner's Mitre turn of two equal cylinders is their exact plane section, and the
//! fillet faces keep their own trim curves (`Kernel::fixes.fillet_blend_corner`).
//!
//! The fork's capi case `turn mitre_boss_base`: a 40 x 30 x 10 plate with a 10 x 10 x 5
//! boss on it (x 15..25, y 10..20), the boss's four base edges rounded r 1 together. At
//! each of the boss's vertical edges the two rounds turn (a Mitre turn of three edges):
//! each r 1 cylinder runs on to the walls' bisector plane, and the mitre is that plane's
//! section of either, an ellipse arc from the rails' crossing on the plate X (cx +- 1,
//! cy +- 1, 10) to the side vertex S (cx, cy, 11). The body's volume is
//! `12500 + 40 (1 - pi/4) + 8 (5/6 - pi/4)` (closed form; the fork's test). The fork
//! 5e8c9c1 (built locally, `SmFilletExecutive` as capi's `round_edges` calls it) prints
//! volume 12508.967554846866 and the mitres 1.26e-15 to 3.77e-15 off the plane and 2.44e-15
//! to 3.55e-15 off the cylinders; without the flag the mitre is the marched intersection,
//! 1.386e-5 off the cylinders, and the volume 12508.967542131706 (the fork's "as before"
//! value, 12508.967542132, in its 74e9beb notes).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point2, Point3, BIG_DOUBLE};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

const PI: f64 = core::f64::consts::PI;

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

fn pick(k: &Kernel, b: &Brep, at: [f64; 3]) -> Option<EdgeId> {
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
    best.map(|(e, _)| e)
}

fn boss_base(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let boss = create_box(k, Point3::new(15.0, 10.0, 10.0), 10.0, 10.0, 5.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], radii: &[f64]) -> Result<(), String> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p).unwrap()).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, b).map_err(|e| e.to_string())?;
        s.prepare_per_edge_constant_radius(k, &edges, radii, &xs, 1, 1.0)
            .and_then(|()| s.do_fillet(k))
            .map_err(|e| format!("status: {e}"))?;
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).map_err(|e| e.to_string())
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

const BASE: [[f64; 3]; 4] = [
    [20.0, 10.0, 10.0],
    [25.0, 15.0, 10.0],
    [20.0, 20.0, 10.0],
    [15.0, 15.0, 10.0],
];

/// What the fork's `turn_mitre_corners` measures at each mitre S (cx, cy, 11) - X (cx +-
/// 1, cy +- 1, 10): the mitres found per corner, and the largest distance of 65 points of
/// each off the bisector plane, off the two cylinders, and of its trim curves on its two
/// faces (lifted onto them) off it at its own parameters.
#[derive(Debug, Default)]
struct Mitres {
    count: [usize; 4],
    plane: f64,
    cyl: f64,
    trims: f64,
    faces: usize,
}

fn mitres(k: &Kernel, b: &Brep) -> Mitres {
    let mut out = Mitres::default();
    for (i, (cx, cy)) in [(15.0, 10.0), (25.0, 10.0), (25.0, 20.0), (15.0, 20.0)]
        .into_iter()
        .enumerate()
    {
        let ox: f64 = if cx > 20.0 { 1.0 } else { -1.0 };
        let oy: f64 = if cy > 15.0 { 1.0 } else { -1.0 };
        let s = Point3::new(cx, cy, 11.0);
        let x = Point3::new(cx + ox, cy + oy, 10.0);
        for e in get_edges(b).unwrap() {
            let a = vertex::point(b, edge::start_vertex(b, e).unwrap().unwrap()).unwrap();
            let z = vertex::point(b, edge::end_vertex(b, e).unwrap().unwrap()).unwrap();
            if !((a.distance(s) < 1e-6 && z.distance(x) < 1e-6)
                || (a.distance(x) < 1e-6 && z.distance(s) < 1e-6))
            {
                continue;
            }
            let c = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            out.count[i] += 1;
            for t in 0..=64 {
                let p = c
                    .evaluate_point(k, ivl.evaluate(f64::from(t) / 64.0))
                    .unwrap();
                out.plane = out
                    .plane
                    .max(((p.x - cx) * ox - (p.y - cy) * oy).abs() / 2.0_f64.sqrt());
                out.cyl = out
                    .cyl
                    .max(((p.x - (cx + ox)).hypot(p.z - 11.0) - 1.0).abs());
                out.cyl = out
                    .cyl
                    .max(((p.y - (cy + oy)).hypot(p.z - 11.0) - 1.0).abs());
            }
            let mut faces = Vec::new();
            for eu in edge::edgeuses(b, e, None, None).unwrap() {
                let f = eu1::face(b, eu).unwrap().unwrap();
                if faces.contains(&f) {
                    continue;
                }
                faces.push(f);
                let surface = face2::surface(b, f).unwrap().unwrap();
                let Some(uv) = eu1::uv_trim_curve(b, eu).unwrap().0.cloned() else {
                    out.trims = f64::INFINITY;
                    continue;
                };
                for t in 0..=64 {
                    let at = ivl.evaluate(f64::from(t) / 64.0);
                    let p = c.evaluate_point(k, at).unwrap();
                    let w = uv.evaluate_point(k, at).unwrap();
                    let q = surface.evaluate_point(k, Point2::new(w.x, w.y)).unwrap();
                    out.trims = out.trims.max(p.distance(q));
                }
            }
            out.faces = out.faces.max(faces.len());
        }
    }
    out
}

/// The boss base rounded, stitched: its volume, face count, check lines and mitres.
fn boss_base_rounded(blend: bool) -> (f64, usize, Vec<String>, Mitres) {
    let mut k = all_fixes(blend);
    let mut b = boss_base(&mut k);
    round(&mut k, &mut b, &BASE, &[1.0; 4]).unwrap();
    let v = volume(&k, &mut b);
    let faces = get_faces(&b).unwrap().len();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let m = mitres(&k, &b);
    (v, faces, lines, m)
}

const BOSS_BASE_MITRE: f64 = 12500.0 + 40.0 * (1.0 - PI / 4.0) + 8.0 * (5.0 / 6.0 - PI / 4.0);

#[test]
fn boss_base_mitres_are_the_rounds_exact_plane_sections() {
    let (v, faces, lines, m) = boss_base_rounded(true);
    eprintln!("volume {v:.12} want {BOSS_BASE_MITRE:.12}, {faces} faces, {m:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(faces, 15);
    assert_eq!(m.count, [1; 4]);
    assert_eq!(m.faces, 2);
    // the fork's turn_mitre_corners: exact to 1e-12
    assert!(
        m.plane <= 1e-12 && m.cyl <= 1e-12 && m.trims <= 1e-12,
        "{m:?}"
    );
    // the fork's 1e-11 (its volume quadrature is to 1e-12)
    assert!(
        (v - BOSS_BASE_MITRE).abs() <= 1e-11 * BOSS_BASE_MITRE,
        "{v}"
    );
}

#[test]
fn without_the_flag_the_mitres_are_marched() {
    let (v, faces, lines, m) = boss_base_rounded(false);
    eprintln!("volume {v:.12}, {faces} faces, {m:?}");
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(faces, 15);
    assert_eq!(m.count, [1; 4]);
    // the marched intersection, 1.386e-5 off the cylinders
    assert!(m.cyl > 1e-6, "{m:?}");
    assert!((v - BOSS_BASE_MITRE).abs() > 1e-9 * BOSS_BASE_MITRE, "{v}");
}
