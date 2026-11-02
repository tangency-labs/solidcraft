//! WR-697 (smlib#79 step 4d, the fork's tangency-labs/smlib@794946e, 3e26f76, 68e039c,
//! 543d25a, 53c47b8): with `Kernel::fixes.fillet_blend_corner` a blend ending at a vertex
//! is the blend corner's End, every G1 continuation of blends is its Join on one shared
//! section, a mixed-convexity End's rail ends at its crossing with the end face's surface,
//! and a round whose rails both lie on its neighbours' rails keeps its face.
//!
//! The volumes are closed forms; the fork 5e8c9c1 values were printed by the fork built
//! locally and run on the same bodies through `SmFilletExecutive` (noted per case).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::blend_corner::{BlendCornerEvent, BlendEnd};
use solidcore::blend::fillet_corner::p3::FilletCornerKind;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3, BIG_DOUBLE};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::topology::brep::p5::{stitch_and_orient, transform, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

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

fn session<'a>(k: &mut Kernel, b: &'a mut Brep, at: &[[f64; 3]], r: f64) -> FilletSession<'a> {
    let edges: Vec<EdgeId> = at.iter().map(|&p| pick(k, b, p)).collect();
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    let radii = vec![r; edges.len()];
    let mut s = FilletSession::new(k, b).unwrap();
    s.prepare_per_edge_constant_radius(k, &edges, &radii, &xs, 1, 1.0)
        .unwrap();
    s
}

/// Rounds the edges at `at` at radius `r` together.
fn round(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], r: f64) {
    {
        let mut s = session(k, b, at, r);
        s.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
}

/// A corner's blend event and End kind.
type Event = (BlendCornerEvent, BlendEnd);
/// A corner's vertex point, kind, blend and edge use counts.
type Corner = (Point3, FilletCornerKind, Option<Event>, usize);

/// The corners' (vertex point, kind, blend event and End kind, 10 x blended edge uses +
/// edge uses) as `CreateFilletCorners` makes them.
fn corners(k: &mut Kernel, b: &mut Brep, at: &[[f64; 3]], r: f64) -> Vec<Corner> {
    let mut s = session(k, b, at, r);
    s.create_corners(k, None).unwrap();
    let mut out = Vec::new();
    for id in s.fillet_corners().unwrap() {
        let c = s.corner(id).unwrap();
        let v = c.filleted_vertex().unwrap();
        out.push((
            vertex::point(s.target(), v).unwrap(),
            c.kind,
            c.blend.map(|bc| (bc.event, bc.end)),
            c.solver_eus.as_ref().map_or(0, Vec::len) * 10 + c.all_eus.as_ref().map_or(0, Vec::len),
        ));
    }
    out
}

fn at(cs: &[Corner], p: [f64; 3]) -> (FilletCornerKind, Option<Event>) {
    let q = Point3::new(p[0], p[1], p[2]);
    let c = cs.iter().find(|c| c.0.distance(q) < 1e-6).unwrap();
    (c.1, c.2)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs)
        .or_else(|_| brep_volume(k, b, 1e-9, &attrs))
        .unwrap()
}

fn faces(b: &Brep) -> usize {
    get_faces(b).unwrap().len()
}

fn nearest_vertex(b: &Brep, q: Point3) -> f64 {
    let mut best = f64::MAX;
    for e in get_edges(b).unwrap() {
        let v0 = edge::vertex(b, e).unwrap();
        let v1 = edge::other_vertex(b, e, v0).unwrap();
        for v in [v0, v1].into_iter().flatten() {
            best = best.min(vertex::point(b, v).unwrap().distance(q));
        }
    }
    best
}

fn clean(k: &mut Kernel, b: &mut Brep) {
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// 794946e: a 20 x 4 x 4 bar, its four long edges rounded r 2 in one call, is the rod of
/// radius 2 (80 pi, 6 faces); without the fix one round is dropped (7 faces). The fork
/// 5e8c9c1 prints 251.327412287185155 and 6 faces.
#[test]
fn a_square_bar_rounded_on_its_four_long_edges_is_a_rod() {
    let edges = [
        [10.0, 0.0, 0.0],
        [10.0, 4.0, 0.0],
        [10.0, 0.0, 4.0],
        [10.0, 4.0, 4.0],
    ];
    for blend in [true, false] {
        let mut k = all_fixes(blend);
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 4.0, 4.0).unwrap();
        round(&mut k, &mut b, &edges, 2.0);
        let v = volume(&k, &mut b);
        eprintln!("blend {blend}: volume {v:.15}, {} faces", faces(&b));
        clean(&mut k, &mut b);
        if blend {
            assert!((v - 80.0 * PI).abs() <= 1e-9 * v, "{v}");
            assert_eq!(faces(&b), 6);
        } else {
            // pinned SMLib drops a round
            assert!((v - 268.495559215389).abs() <= 1e-9 * v, "{v}");
            assert_eq!(faces(&b), 7);
        }
    }
}

/// The step block: a 40 x 30 x 10 block under a 20 x 30 x 10 one whose wall is the cylinder
/// R 20 about the y axis through (0, 0, 10).
fn cylinder_wall(k: &mut Kernel) -> Brep {
    let a = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let mut c = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 20.0, 30.0).unwrap();
    let pl = Axis2Placement::new(Point3::new(0.0, 0.0, 10.0), Vec3::Z, Vec3::X);
    transform(k, &mut c, &pl, None, false).unwrap();
    let below = create_box(k, Point3::new(-30.0, -1.0, -20.0), 60.0, 32.0, 30.0).unwrap();
    let c = boolean_difference(k, c, below).unwrap();
    let left = create_box(k, Point3::new(-30.0, -1.0, 5.0), 30.0, 32.0, 30.0).unwrap();
    let c = boolean_difference(k, c, left).unwrap();
    boolean_union(k, a, c).unwrap()
}

/// 3e26f76, 53c47b8: the step's front edge (y = 0, z = 10) rounded r 2 ends at (20, 0, 10)
/// on the cylinder wall, a mixed-convexity End (its front side edge convex, the step's
/// edge concave). Its rail on the front face ends where it crosses the cylinder,
/// (sqrt 396, 0, 8). The round takes the spandrel swept from the cylinder to x = 40:
/// with z = 8 + 2 sin t, the integral of 4 (1 - cos t) cos t (40 - sqrt(400 - (2 sin t - 2)^2))
/// over [0, pi/2]. The fork 5e8c9c1 (and 68e039c) prints 21407.602511807886913 with the
/// fix, and 0bd3f4d (before 3e26f76) 21407.602511915742070, the pinned rail end's.
#[test]
fn a_mixed_convexity_end_s_rail_ends_on_the_end_face_s_surface() {
    let n = 20000;
    let h = PI / 2.0 / f64::from(n);
    let mut removed = 0.0;
    for i in 0..=n {
        let t = f64::from(i) * h;
        let f = (2.0 - 2.0 * t.cos())
            * (40.0 - (400.0 - (2.0 * t.sin() - 2.0).powi(2)).sqrt())
            * 2.0
            * t.cos();
        let w = if i == 0 || i == n {
            1.0
        } else if i % 2 == 1 {
            4.0
        } else {
            2.0
        };
        removed += w * f;
    }
    removed *= h / 3.0;
    let want = 12000.0 + 3000.0 * PI - removed;
    for blend in [true, false] {
        let mut k = all_fixes(blend);
        let mut b = cylinder_wall(&mut k);
        let cs = corners(&mut k, &mut b.clone(), &[[30.0, 0.0, 10.0]], 2.0);
        let (kind, event) = at(&cs, [20.0, 0.0, 10.0]);
        assert_eq!(kind, FilletCornerKind::NByOne);
        assert_eq!(
            event,
            blend.then_some((BlendCornerEvent::End, BlendEnd::OnEndFace))
        );
        round(&mut k, &mut b, &[[30.0, 0.0, 10.0]], 2.0);
        let v = volume(&k, &mut b);
        let off = nearest_vertex(&b, Point3::new(396.0_f64.sqrt(), 0.0, 8.0));
        eprintln!("blend {blend}: volume {v:.15} want {want:.15}; rail end off {off:.3e}");
        clean(&mut k, &mut b);
        assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
        assert!(off <= 1e-13, "{off}");
        let fork = if blend {
            21407.602511807886913
        } else {
            21407.602511915742070
        };
        assert!((v - fork).abs() <= 1e-10, "{v} vs the fork's {fork}");
    }
}

/// The same End on a plane wall sloped back (the upper block's profile (0, 10), (20, 10),
/// (17, 20), (0, 20) swept along y): the rail ends at (20.6, 0, 8), and the round takes
/// 20 (4 - pi) - 0.3 (20/3 - 2 pi) = 78 - 19.4 pi. The fork 5e8c9c1 prints
/// 17532.946897573881870 (9.4e-8 off the closed form) with the same rail end and counts.
#[test]
fn a_mixed_convexity_end_on_a_sloped_wall_has_its_closed_form() {
    use solidcore::curve::curve::Curve;
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::sweep::api_brep::create_linear_sweep;
    let mut k = all_fixes(true);
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
    let pts = [
        Point3::new(0.0, 0.0, 10.0),
        Point3::new(20.0, 0.0, 10.0),
        Point3::new(17.0, 0.0, 20.0),
        Point3::new(0.0, 0.0, 20.0),
    ];
    let sides: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % 4]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    let c = create_linear_sweep(&mut k, &refs, Vec3::Y, 30.0, true).unwrap();
    let mut b = boolean_union(&mut k, a, c).unwrap();
    round(&mut k, &mut b, &[[30.0, 0.0, 10.0]], 2.0);
    let v = volume(&k, &mut b);
    let want = 17550.0 - 78.0 + 19.4 * PI;
    clean(&mut k, &mut b);
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert!(nearest_vertex(&b, Point3::new(20.6, 0.0, 8.0)) <= 1e-13);
    assert_eq!(faces(&b), 9);
}

/// 68e039c, 543d25a: a ball R 10 cut at z = 4, its rim's two edges rounded r 2. At one
/// vertex the two edges meet alone (pinned: the 2x2 corner), at the other across the
/// ball's seam (pinned: the Nx2 corner): both are the blend corner's Join. The round is a torus piece
/// about z; V = 4000 pi / 3 - 288 pi less what it removes, 3261.518762521006 (the fork's
/// capi `exact_class_sphere_plane`). The fork's driver here fails this body with status
/// 1001 at every revision tried, so the value is checked against the closed form only.
#[test]
fn a_cut_ball_s_rim_joins_on_its_shared_sections() {
    let q = 42.0_f64.sqrt();
    let rim = [[q, q, 4.0], [-q, -q, 4.0]];
    for blend in [true, false] {
        let mut k = all_fixes(blend);
        let s = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
        let cut = create_box(&mut k, Point3::new(-20.0, -20.0, 4.0), 40.0, 40.0, 20.0).unwrap();
        let mut b = boolean_difference(&mut k, s, cut).unwrap();
        let cs = corners(&mut k, &mut b.clone(), &rim, 2.0);
        eprintln!("{cs:?}");
        assert_eq!(cs.len(), 2);
        let two = cs.iter().find(|c| c.3 == 22).unwrap();
        let seam = cs.iter().find(|c| c.3 == 23).unwrap();
        assert_eq!(seam.1, FilletCornerKind::NByTwo);
        assert_eq!(seam.2.map(|e| e.0), blend.then_some(BlendCornerEvent::Join));
        if blend {
            assert_eq!(two.1, FilletCornerKind::NByTwo);
            assert_eq!(two.2.map(|e| e.0), Some(BlendCornerEvent::Join));
        } else {
            assert_eq!(two.1, FilletCornerKind::TwoByTwo);
            assert_eq!(two.2, None);
        }
        round(&mut k, &mut b, &rim, 2.0);
        let v = volume(&k, &mut b);
        eprintln!("blend {blend}: volume {v:.15}");
        clean(&mut k, &mut b);
        assert!((v - 3261.518762521006).abs() <= 1e-9 * v, "{v}");
        assert_eq!(faces(&b), 4);
    }
}

/// 543d25a: the tee's saddle, one closed edge meeting itself at its vertex on the
/// branch's seam, rounded r 3, is a closed Join (pinned: the closed Nx1 corner); the result is the
/// pinned corner's. The closed form (the fork's capi `exact_class_tee_is_fitted`) is
/// 129829.952905668950; this round is not the fork's fitted procedural blend, so the port
/// lands 4.5e-3 above it, with and without the flag alike.
#[test]
fn a_tee_s_closed_saddle_is_a_closed_join() {
    let h = 0.5_f64.sqrt() * 8.0;
    let saddle = [[h, h, 368.0_f64.sqrt()]];
    let mut out = Vec::new();
    for blend in [true, false] {
        let mut k = all_fixes(blend);
        let mut pipe = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 100.0).unwrap();
        let pl = Axis2Placement::new(Point3::new(-50.0, 0.0, 0.0), Vec3::Y, Vec3::Z);
        transform(&mut k, &mut pipe, &pl, None, false).unwrap();
        let branch = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 8.0, 40.0).unwrap();
        let mut b = boolean_union(&mut k, pipe, branch).unwrap();
        let cs = corners(&mut k, &mut b.clone(), &saddle, 3.0);
        assert_eq!(cs.len(), 1);
        if blend {
            assert_eq!(cs[0].1, FilletCornerKind::NByTwo);
            assert_eq!(cs[0].2.map(|e| e.0), Some(BlendCornerEvent::Join));
            assert_eq!(cs[0].3, 12);
        } else {
            assert_eq!(cs[0].1, FilletCornerKind::NByOneClosed);
        }
        round(&mut k, &mut b, &saddle, 3.0);
        clean(&mut k, &mut b);
        out.push((volume(&k, &mut b), faces(&b)));
    }
    eprintln!("{out:?}");
    assert_eq!(out[0], out[1]);
    assert_eq!(out[0].1, 6);
    assert!(
        (out[0].0 - 129829.952905668950).abs() <= 5e-3,
        "{}",
        out[0].0
    );
}
