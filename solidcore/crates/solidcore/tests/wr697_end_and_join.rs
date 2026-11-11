//! WR-697 (smlib#79 step 4d, the fork's tangency-labs/smlib@794946e, 3e26f76, 68e039c,
//! 543d25a, 53c47b8): a blend ending at a vertex
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
    volume_at(k, b, 1e-12)
        .or_else(|_| volume_at(k, b, 1e-9))
        .unwrap()
}

/// `SmApiBrepComputeVolume` at `accuracy`.
fn volume_at(k: &Kernel, b: &mut Brep, accuracy: f64) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, accuracy, &attrs)
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
/// radius 2 (80 pi, 6 faces), where pinned SMLib drops one round (7 faces). The fork
/// 5e8c9c1 prints 251.327412287185155 and 6 faces.
#[test]
fn a_square_bar_rounded_on_its_four_long_edges_is_a_rod() {
    let edges = [
        [10.0, 0.0, 0.0],
        [10.0, 4.0, 0.0],
        [10.0, 0.0, 4.0],
        [10.0, 4.0, 4.0],
    ];
    let mut k = all_fixes();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 4.0, 4.0).unwrap();
    round(&mut k, &mut b, &edges, 2.0);
    let v = volume(&k, &mut b);
    eprintln!("volume {v:.15}, {} faces", faces(&b));
    clean(&mut k, &mut b);
    assert!((v - 80.0 * PI).abs() <= 1e-9 * v, "{v}");
    assert_eq!(faces(&b), 6);
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
    let mut k = all_fixes();
    let mut b = cylinder_wall(&mut k);
    let cs = corners(
        &mut k,
        &mut b.clone_standing_for_self().unwrap(),
        &[[30.0, 0.0, 10.0]],
        2.0,
    );
    let (kind, event) = at(&cs, [20.0, 0.0, 10.0]);
    assert_eq!(kind, FilletCornerKind::NByOne);
    assert_eq!(event, Some((BlendCornerEvent::End, BlendEnd::OnEndFace)));
    round(&mut k, &mut b, &[[30.0, 0.0, 10.0]], 2.0);
    let v = volume(&k, &mut b);
    let off = nearest_vertex(&b, Point3::new(396.0_f64.sqrt(), 0.0, 8.0));
    eprintln!("volume {v:.15} want {want:.15}; rail end off {off:.3e}");
    clean(&mut k, &mut b);
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    assert!(off <= 1e-13, "{off}");
    let fork = 21407.602511807886913;
    assert!((v - fork).abs() <= 1e-10, "{v} vs the fork's {fork}");
}

/// The step with its upper block's profile (0, 10), (20, 10), (17, 20), (0, 20) swept
/// along y: a plane wall sloped back.
fn sloped_wall(k: &mut Kernel) -> Brep {
    use solidcore::curve::curve::Curve;
    use solidcore::primitive::api_curves::create_line_segment;
    use solidcore::sweep::api_brep::create_linear_sweep;
    let a = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0).unwrap();
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
    let c = create_linear_sweep(k, &refs, Vec3::Y, 30.0, true).unwrap();
    boolean_union(k, a, c).unwrap()
}

/// The same End on a plane wall sloped back (the upper block's profile (0, 10), (20, 10),
/// (17, 20), (0, 20) swept along y): the rail ends at (20.6, 0, 8), and the round takes
/// 20 (4 - pi) - 0.3 (20/3 - 2 pi) = 78 - 19.4 pi. The fork 5e8c9c1 prints
/// 17532.946897573881870 (9.4e-8 off the closed form) with the same rail end and counts.
#[test]
fn a_mixed_convexity_end_on_a_sloped_wall_has_its_closed_form() {
    let mut k = all_fixes();
    let mut b = sloped_wall(&mut k);
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
    let mut k = all_fixes();
    let s = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0).unwrap();
    let cut = create_box(&mut k, Point3::new(-20.0, -20.0, 4.0), 40.0, 40.0, 20.0).unwrap();
    let mut b = boolean_difference(&mut k, s, cut).unwrap();
    let cs = corners(&mut k, &mut b.clone_standing_for_self().unwrap(), &rim, 2.0);
    eprintln!("{cs:?}");
    assert_eq!(cs.len(), 2);
    let two = cs.iter().find(|c| c.3 == 22).unwrap();
    let seam = cs.iter().find(|c| c.3 == 23).unwrap();
    assert_eq!(seam.1, FilletCornerKind::NByTwo);
    assert_eq!(seam.2.map(|e| e.0), Some(BlendCornerEvent::Join));
    assert_eq!(two.1, FilletCornerKind::NByTwo);
    assert_eq!(two.2.map(|e| e.0), Some(BlendCornerEvent::Join));
    round(&mut k, &mut b, &rim, 2.0);
    let v = volume(&k, &mut b);
    eprintln!("volume {v:.15}");
    clean(&mut k, &mut b);
    assert!((v - 3261.518762521006).abs() <= 1e-9 * v, "{v}");
    assert_eq!(faces(&b), 4);
}

/// 543d25a: the tee's saddle, one closed edge meeting itself at its vertex on the
/// branch's seam, rounded r 3, is a closed Join (pinned: the closed Nx1 corner); the result is the
/// pinned corner's. The closed form (the fork's capi `exact_class_tee_is_fitted`) is
/// 129829.952905668950; this round is not the fork's fitted procedural blend, so the port
/// lands 2.8e-4 above it. The volume is integrated at 1e-8, which lands within 6e-6 of the
/// value at 1e-12 (measured on this body), far inside the 5e-3 it is checked to.
#[test]
fn a_tee_s_closed_saddle_is_a_closed_join() {
    let h = 0.5_f64.sqrt() * 8.0;
    let saddle = [[h, h, 368.0_f64.sqrt()]];
    let mut k = all_fixes();
    let mut pipe = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 100.0).unwrap();
    let pl = Axis2Placement::new(Point3::new(-50.0, 0.0, 0.0), Vec3::Y, Vec3::Z);
    transform(&mut k, &mut pipe, &pl, None, false).unwrap();
    let branch = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 8.0, 40.0).unwrap();
    let mut b = boolean_union(&mut k, pipe, branch).unwrap();
    let cs = corners(
        &mut k,
        &mut b.clone_standing_for_self().unwrap(),
        &saddle,
        3.0,
    );
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].1, FilletCornerKind::NByTwo);
    assert_eq!(cs[0].2.map(|e| e.0), Some(BlendCornerEvent::Join));
    assert_eq!(cs[0].3, 12);
    round(&mut k, &mut b, &saddle, 3.0);
    clean(&mut k, &mut b);
    let (v, n) = (volume_at(&k, &mut b, 1e-8).unwrap(), faces(&b));
    eprintln!("volume {v:.15}, {n} faces");
    assert_eq!(n, 6);
    assert!((v - 129829.952905668950).abs() <= 5e-3, "{v}");
}

/// The teardrop's control points: one closed cubic B-spline from its tip (0, 0) round
/// through (15.5, 0) and back, with knots 0, 1/3, 2/3, 1 (ends of multiplicity 4). Its two
/// ends leave the tip along (1, -1) and (1, 1), at a right angle.
const TEARDROP: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [8.0, -8.0],
    [16.0, -4.0],
    [16.0, 4.0],
    [8.0, 8.0],
    [0.0, 0.0],
];
const TEARDROP_KNOTS: [f64; 10] = [0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0];

/// The teardrop profile swept 10 up z with its caps: three faces, and each rim one closed
/// edge meeting itself at a right angle at the tip, where the side face meets itself along
/// its seam.
fn teardrop_prism(k: &mut Kernel) -> Brep {
    use solidcore::curve::curve::Curve;
    use solidcore::nurbs::b_spline_curve::p1::{
        BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
    };
    use solidcore::sweep::api_brep::create_linear_sweep;
    let control = TEARDROP.map(|[x, y]| Vec3::new(x, y, 0.0));
    let profile = Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[4, 1, 1, 4],
            knots: &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    );
    create_linear_sweep(k, &[Some(&profile)], Vec3::Z, 10.0, true).unwrap()
}

/// The teardrop's point and first derivative at `u`, by Cox-de Boor on its knots (written
/// out here, so that the profile's area and length do not come from the kernel).
fn teardrop_point(u: f64) -> ([f64; 2], [f64; 2]) {
    let t = TEARDROP_KNOTS;
    let span = (3..6).rev().find(|&i| u >= t[i]).unwrap_or(3);
    // n[p][j]: the degree-p basis function N_{span-p+j, p} at u
    let mut n = [[0.0; 4]; 4];
    n[0][0] = 1.0;
    for p in 1..=3 {
        for j in 0..=p {
            let i = span + j - p;
            let mut v = 0.0;
            if j >= 1 && t[i + p] > t[i] {
                v += (u - t[i]) / (t[i + p] - t[i]) * n[p - 1][j - 1];
            }
            if j < p && t[i + p + 1] > t[i + 1] {
                v += (t[i + p + 1] - u) / (t[i + p + 1] - t[i + 1]) * n[p - 1][j];
            }
            n[p][j] = v;
        }
    }
    let (mut c, mut d) = ([0.0; 2], [0.0; 2]);
    for (j, w) in n[3].iter().enumerate() {
        let pole = TEARDROP[span + j - 3];
        c[0] += w * pole[0];
        c[1] += w * pole[1];
    }
    // C' = sum of N_{i+1, 2} 3 (P_{i+1} - P_i) / (t_{i+4} - t_{i+1})
    for (j, w) in n[2].iter().take(3).enumerate() {
        let i = span + j - 3;
        let s = 3.0 / (t[i + 4] - t[i + 1]);
        d[0] += w * s * (TEARDROP[i + 1][0] - TEARDROP[i][0]);
        d[1] += w * s * (TEARDROP[i + 1][1] - TEARDROP[i][1]);
    }
    (c, d)
}

/// The teardrop's area (Green's theorem) and perimeter, by 5-point Gauss-Legendre on 192
/// pieces (exact for the area: a cubic B-spline is a polynomial on each piece).
fn teardrop_area_and_length() -> (f64, f64) {
    let g = [
        (0.0, 128.0 / 225.0),
        (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
        (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
        (0.906_179_845_938_664, 0.236_926_885_056_189_1),
    ];
    let pieces = 192;
    let (mut area, mut length) = (0.0, 0.0);
    for p in 0..pieces {
        let a = f64::from(p) / f64::from(pieces);
        let b = f64::from(p + 1) / f64::from(pieces);
        for (x, w) in g {
            let (c, d) = teardrop_point(0.5 * (a + b) + 0.5 * (b - a) * x);
            let w = w * 0.5 * (b - a);
            area += w * 0.5 * (c[0] * d[1] - c[1] * d[0]);
            length += w * d[0].hypot(d[1]);
        }
    }
    (area, length)
}

/// #2498: the teardrop prism's top rim, one closed convex edge meeting itself at a right
/// angle at the tip, rounded r 0.5 and r 1 with every generic-blend flag on. The round runs
/// round the rim and meets itself on the tip's bisecting plane, a mitre. So its horizontal
/// section at depth t below z = 10 - r is the profile's inner parallel set at
/// d = r - sqrt(r^2 - t^2) (the profile's least radius of curvature is 4.69, far above r),
/// whose area is A - L d + d^2 ((pi + a) / 2 + cot(a / 2)) for a convex profile of area A
/// and perimeter L with one corner of interior angle a (here pi / 2). Integrated over t, the
/// round takes L r^2 (1 - pi/4) - (3 pi / 4 + 1) r^3 (5/3 - pi/2) off the prism's 10 A. The
/// body has the prism's three faces and the round's two halves, which meet each other at
/// the mitre and at the rim's far point. The volume lands 7.7e-7 (r 0.5) and 1.9e-7 (r 1)
/// of itself under the closed form; a corner built other than as the mitre (a ball or a
/// setback) moves it by some 0.3 r^3, 3.3e-5 of it at r 0.5. The top rails' runs past the
/// tip, which cross the rim near it, were taken for a cliff overflow, and the trim at the
/// creases then failed with "edge without primary edge use".
#[test]
fn a_closed_convex_rim_meeting_itself_at_an_angle_is_rounded() {
    let (area, length) = teardrop_area_and_length();
    let rim = [[15.5, 0.0, 10.0]];
    for r in [0.5, 1.0] {
        let want = 10.0 * area - length * r * r * (1.0 - PI / 4.0)
            + (3.0 * PI / 4.0 + 1.0) * r.powi(3) * (5.0 / 3.0 - PI / 2.0);
        let mut k = all_fixes();
        let mut b = teardrop_prism(&mut k);
        round(&mut k, &mut b, &rim, r);
        let v = volume(&k, &mut b);
        eprintln!(
            "r {r}: volume {v:.15} want {want:.15} ({:.3e}), {} faces",
            (v - want) / want,
            faces(&b)
        );
        clean(&mut k, &mut b);
        assert_eq!(faces(&b), 5);
        assert!((v - want).abs() <= 1e-6 * want, "{v} vs {want}");
    }
}

/// A teardrop boss 6 high on a 40 x 40 x 4 plate. The boss's profile is one closed cubic
/// B-spline from its tip (0, 0) round through (15.5, 0) and back, swept up z, so the boss's
/// foot is one closed edge that meets itself at a right angle at the tip, where the boss's
/// side face meets itself along its seam.
fn teardrop_boss(k: &mut Kernel) -> Brep {
    use solidcore::curve::curve::Curve;
    use solidcore::nurbs::b_spline_curve::p1::{
        BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
    };
    use solidcore::sweep::api_brep::create_linear_sweep;
    let control = TEARDROP.map(|[x, y]| Vec3::new(x, y, 0.0));
    let profile = Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &control,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[4, 1, 1, 4],
            knots: &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    );
    let boss = create_linear_sweep(k, &[Some(&profile)], Vec3::Z, 6.0, true).unwrap();
    let plate = create_box(k, Point3::new(-10.0, -20.0, -4.0), 40.0, 40.0, 4.0).unwrap();
    boolean_union(k, plate, boss).unwrap()
}

/// The closed-edge corner where a closed edge meets itself at an angle: the teardrop boss's
/// foot, rounded r 0.5 and r 1, is the blend corner's closed Turn, Mitred, at the tip
/// (#2349; 543d25a makes a closed edge meeting itself G1 the Join). It replaced the closed
/// Nx1 corner, the fork's angle case (smlib#113 caa67d0, 2ac7a17), with its construction
/// step for step. The volumes are the port's own (no closed form; the fork was not run on
/// this body), each body clean with the boss's round and the plate's. The foot's round,
/// run on past the teardrop's tip on the wall's extension, is its definition, the round
/// of the wall and the plate to rounding (#13774; before, the trace's skinned B-spline,
/// off that round by 1.5e-4 at its 90th percentile, and the volumes 3.1e-4 and 4.9e-4
/// less). Since #14405 the r 1 round's net has fewer spans (the arc's rows over a piece
/// bounded by Taylor models too), and its volume moved by 2.9e-7 (from
/// 7140.722593621167): both nets lie within their certified bounds of the same definition,
/// so the volumes differ by at most the round's area (68.2 over its two faces) times the
/// two bounds, which the new bound alone (4.0e-6) puts at 2.7e-4. With the feet over a
/// piece bounded by Taylor models too (#14405), the nets have fewer spans again and the
/// volumes moved by 5.5e-7 (r 0.5, from 7133.840421559223614) and 8.9e-6 (r 1, from
/// 7140.722593330715): each net, sampled at 401 x 9 points of each face, is within its
/// bound of its definition (r 0.5: 2.3e-6 of 3.9e-6; r 1: 3.2e-6 of 4.0e-6), and the
/// areas times the bounds (1.3e-4 and 2.7e-4) hold both moves.
#[test]
fn a_closed_edge_meeting_itself_at_an_angle_is_the_closed_turn() {
    let foot = [[15.5, 0.0, 0.0]];
    for (r, want) in [(0.5, 7133.840421010035243), (1.0, 7140.722584425828245)] {
        let mut k = all_fixes();
        let mut b = teardrop_boss(&mut k);
        let cs = corners(&mut k, &mut b.clone_standing_for_self().unwrap(), &foot, r);
        assert_eq!(cs.len(), 1);
        assert!(cs[0].0.distance(Point3::new(0.0, 0.0, 0.0)) <= 1e-12);
        assert_eq!(
            (cs[0].1, cs[0].2, cs[0].3),
            (
                FilletCornerKind::NByTwo,
                Some((BlendCornerEvent::Turn, BlendEnd::OnEndFace)),
                12
            )
        );
        round(&mut k, &mut b, &foot, r);
        let v = volume(&k, &mut b);
        eprintln!("r {r}: volume {v:.15}, {} faces", faces(&b));
        clean(&mut k, &mut b);
        assert_eq!(faces(&b), 10);
        assert!((v - want).abs() <= 1e-12 * want, "{v} vs {want}");
    }
}

// ---- #2488: an End's patch keeps its definition ----

use solidcore::surface::procedural_blend::convert::{corner_rebuild, corner_to_record};
use solidcore::surface::procedural_blend::corner::CornerKind;
use solidcore::surface::procedural_blend::record::{CornerRecord, SupportRecord};
use solidcore::surface::procedural_blend::BlendNet;
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as face2;

/// The step's front edge rounded r 2 into an End on `wall`, with or without
/// `fillet_round_as_definition`.
fn end_on(wall: fn(&mut Kernel) -> Brep, definition: bool) -> (Kernel, Brep) {
    let mut k = all_fixes();
    k.fixes.fillet_round_as_definition = definition;
    let mut b = wall(&mut k);
    round(&mut k, &mut b, &[[30.0, 0.0, 10.0]], 2.0);
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

/// `rec`'s NURBS form moved by `by` along x, y and z.
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
fn the_end_patches_are_their_definitions() {
    for wall in [cylinder_wall as fn(&mut Kernel) -> Brep, sloped_wall] {
        let (k, mut b) = end_on(wall, true);
        let ends = corner_faces(&b, CornerKind::End);
        assert_eq!(ends.len(), 1);
        for s in &ends {
            let d = s.procedural_blend().unwrap();
            assert!(matches!(d.net(), BlendNet::Exact(_)));
            assert_eq!(d.bound(), 0.0);
            assert!(d.support(0).is_some());
            assert_eq!(d.recertify(&k).unwrap(), 0.0);
        }
        // the volume and every surface bit for bit as without the definition
        let (k0, mut b0) = end_on(wall, false);
        assert!(corner_faces(&b0, CornerKind::End).is_empty());
        assert_eq!(volume(&k, &mut b).to_bits(), volume(&k0, &mut b0).to_bits());
        assert_eq!(face_bits(&k, &b), face_bits(&k0, &b0));
    }
}

#[test]
fn an_end_patch_round_trips_through_its_record() {
    for wall in [cylinder_wall as fn(&mut Kernel) -> Brep, sloped_wall] {
        let (k, b) = end_on(wall, true);
        for s in corner_faces(&b, CornerKind::End) {
            let rec = corner_to_record(&k, &s).unwrap();
            let json = serde_json::to_string(&rec).unwrap();
            let back = corner_rebuild(&k, &serde_json::from_str(&json).unwrap()).unwrap();
            assert_eq!(
                std::mem::discriminant(back.kind()),
                std::mem::discriminant(s.kind())
            );
            assert_eq!(corner_to_record(&k, &back).unwrap(), rec);
        }
    }
}

#[test]
fn an_end_patch_off_its_support_is_refused() {
    for wall in [cylinder_wall as fn(&mut Kernel) -> Brep, sloped_wall] {
        let (k, b) = end_on(wall, true);
        let s = &corner_faces(&b, CornerKind::End)[0];
        let CornerRecord::End(rec) = corner_to_record(&k, s).unwrap() else {
            panic!("not an end's record");
        };
        let mut off = rec.clone();
        off.support = moved(&rec.support, 1e-3);
        let why = corner_rebuild(&k, &CornerRecord::End(off))
            .unwrap_err()
            .to_string();
        assert!(why.contains("off its support face"), "{why}");
        let mut off = rec;
        off.net = moved(&off.net, 1e-3);
        let why = corner_rebuild(&k, &CornerRecord::End(off))
            .unwrap_err()
            .to_string();
        assert!(why.contains("off its support face"), "{why}");
    }
}
