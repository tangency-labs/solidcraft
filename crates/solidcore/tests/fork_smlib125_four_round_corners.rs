//! tangency-labs/smlib#125 (278bf77, 3e10318; solidcore#2359): corners of four or more
//! rounds. With `Kernel::fixes.fillet_blend_corner`, n >= 4 constant rounds of one radius
//! at a vertex of n planes whose offsets by the radius meet in one point are the blend
//! corner's Ball: the spherical n-gon of that ball between the rounds' end sections, fanned
//! from contact 0 into n - 2 rational patches exactly on the sphere. Without a common ball
//! they are a Setback. Without the flag the corner is the pinned `SmFilletConvexNxNCorner`
//! (or `SmFilletConcaveNxNCorner`).
//!
//! The bodies are those of the fork's capi tests `ball regular_apex_{4,5,6}`,
//! `setback four_face_apex` and `setback four_face_pocket_floor` (fc29fdb
//! `capi/tests/ops_kernel.cpp`), and the regular apex's ball comes from its closed form. The
//! regular apex has the box [0, 20]^2 x [0, 30], cut by n planes through (10, 10, 25). Their
//! out-of-material normals run along `(cos a_k, sin a_k, 1)`, `a_k = 2 pi k / n`, and the edges
//! are rounded 3. The ball of radius 3 touching every face is centred on the axis, `3 / n_z`
//! below the apex. The checks are geometric:
//! - `check_brep` at Level2 passes, and the face count is the sharp body's plus 2 n - 2.
//! - Each of the n - 2 patches lies on the sphere, along its edges too, with a radial normal.
//! - Every patch is G1 to its neighbours along its edges.
//!
//! The square pyramid of WR-502 (`fillet_corner_sphere_from_four_arcs`, 40 across the base
//! and 30 high, every edge R 2) is a regular apex of four. With `fillet_blend_corner` its apex
//! is the Ball: two patches on the ball touching the four faces (centre
//! `z = (600 - 2 sqrt 1300) / 20`), G1 to the rounds, with or without the WR-502 flag.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)] // test helpers

use solidcore::blend::blend_corner::BlendCornerEvent;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_pyramid};
use solidcore::query::api_queries::{edge_closest_point, get_faces, surface_closest_point};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, Brep, EdgeId, FaceId};
use std::f64::consts::PI;

/// Every opt-in fix on but `geometric_g2_continuity`, with `fillet_blend_corner` on or off.
fn kernel(blend: bool) -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && (blend || name != "fillet_blend_corner") {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    // (the log changes no result: issue_2489_blend_faces_are_definitions)
    k.blend_face_log = Some(solidcore::blend::definition_check::BlendFaceLog::default());
    k
}

/// jyviko's blend-definition rule (#2243, #2489): each face the blend made is a definition
/// (`blend::definition_check::is_definition`) or of a kind the ledger lists
/// (`data/blend_definition_ledger/`, one file per kind naming its follow-up issue).
fn assert_definitions_or_listed(name: &str, k: &Kernel, b: &Brep) {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/blend_definition_ledger"
    );
    let listed: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            e.unwrap()
                .path()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let made = k
        .blend_face_log
        .as_ref()
        .expect("the blend face log")
        .last()
        .to_vec();
    assert!(!made.is_empty(), "{name}: the blend made no face");
    for f in solidcore::blend::definition_check::bare_blend_faces(b, &made).unwrap() {
        let kind = f.kind.name().replace('/', "-");
        assert!(
            listed.contains(&kind),
            "{name}: bare face {} of kind {kind} ({}) is not in the ledger",
            f.face.index(),
            f.surface
        );
    }
}

fn unit(v: Vec3) -> Vec3 {
    v / v.length()
}

fn prism(k: &mut Kernel, pts: &[Point3], dir: Vec3, height: f64) -> Brep {
    let sides: Vec<Curve> = (0..pts.len())
        .map(|i| Curve::from(create_line_segment(pts[i], pts[(i + 1) % pts.len()]).unwrap()))
        .collect();
    let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
    create_linear_sweep(k, &refs, dir, height, true).unwrap()
}

/// The box 400 wide on the far side (along `n`) of the plane through `at` with normal `n`.
fn half_space(k: &mut Kernel, at: Vec3, n: Vec3) -> Brep {
    let u = unit(n.cross(if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }));
    let v = n.cross(u);
    let h = 200.0;
    let c = [
        at + (u + v) * h,
        at + (v - u) * h,
        at - (u + v) * h,
        at + (u - v) * h,
    ];
    prism(k, &c, n, 400.0)
}

/// The capi's `apex_normals`: face k's unit normal out of the material.
fn apex_normals(leans: &[f64], pocket: bool) -> Vec<Vec3> {
    let n = leans.len();
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            let s = if pocket { -1.0 } else { 1.0 };
            unit(Vec3::new(s * a.cos(), s * a.sin(), leans[k]))
        })
        .collect()
}

/// The capi's `apex_body`.
fn apex_body(k: &mut Kernel, ns: &[Vec3], pocket: bool) -> Brep {
    if !pocket {
        let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 30.0).unwrap();
        for &v in ns {
            let cut = half_space(k, Vec3::new(10.0, 10.0, 25.0), v);
            b = boolean_difference(k, b, cut).unwrap();
        }
        return b;
    }
    let mut tool = create_box(k, Point3::new(-5.0, -5.0, 10.0), 30.0, 30.0, 30.0).unwrap();
    for &v in ns {
        let cut = half_space(k, Vec3::new(10.0, 10.0, 10.0), -v);
        tool = boolean_difference(k, tool, cut).unwrap();
    }
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    boolean_difference(k, block, tool).unwrap()
}

/// The capi's `apex_edges`: edge k between faces k and k + 1, away from the apex.
fn apex_edges(ns: &[Vec3], pocket: bool) -> Vec<Vec3> {
    (0..ns.len())
        .map(|k| {
            let d = unit(ns[k].cross(ns[(k + 1) % ns.len()]));
            if (d.z > 0.0) == pocket {
                d
            } else {
                -d
            }
        })
        .collect()
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    assert!(best.0 < 1e-6, "no edge at {q:?}");
    best.1.unwrap()
}

/// Rounds the edges `es` at radius `r` together: the corner event at `corner` as made, and
/// the result's check lines (Level2).
fn round(
    k: &mut Kernel,
    b: &mut Brep,
    es: &[EdgeId],
    r: f64,
    corner: Point3,
) -> (Option<BlendCornerEvent>, Vec<String>) {
    let radii = vec![r; es.len()];
    let event = {
        let mut s = FilletSession::new(k, b).unwrap();
        s.prepare_per_edge_constant_radius(
            k,
            es,
            &radii,
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(k, None).unwrap();
        let mut made = None;
        for c in s.fillet_corners().unwrap() {
            let fc = s.corner(c).unwrap();
            let at = fc.vertex.map(|v| s.target().vertex(v).unwrap().point);
            if at.is_some_and(|p| p.distance(corner) < 1e-9) {
                made = Some(fc.blend.as_ref().map(|b| b.event));
            }
        }
        s.do_filleting(k, None).unwrap();
        made.expect("a corner at the apex")
    };
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let lines = check_brep(k, b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .iter()
        .map(|l| format!("{l:?}"))
        .collect();
    (event, lines)
}

/// The faces of `b` that are rational B-splines on the sphere about `c` of radius `r`.
fn sphere_patches(k: &Kernel, b: &Brep, c: Point3, r: f64) -> Vec<FaceId> {
    get_faces(b)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            let s = face2::surface(b, f).unwrap().unwrap();
            // a Ball's patch is an SmBallPatch carrying its sphere (the fork's f8e6709);
            // a rational B-spline on the sphere is looked for too (the flag-off corner)
            let net = match s.kind() {
                SurfaceKind::BallPatch(patch) => {
                    if patch.centre().distance(c) > 1e-12 || (patch.radius() - r).abs() > 1e-12 {
                        return false;
                    }
                    patch.net()
                }
                SurfaceKind::BSpline(net) => net,
                _ => return false,
            };
            if !net.is_rational() {
                return false;
            }
            let dom = s.natural_uv_domain_with_nurb(k).unwrap();
            let p = s.evaluate_point(k, dom.evaluate(0.5, 0.5)).unwrap();
            ((p - c).length() - r).abs() < 1e-6
        })
        .collect()
}

/// A Ball patch's worst deviations: from the sphere over a 17 x 17 grid, its edges' from
/// the sphere, its interior normal from the radial line, and the angle between its normal
/// and its neighbours' normals at nine points along each of its edges (G1 at the seams).
fn ball_patch_offs(k: &Kernel, b: &Brep, f: FaceId, c: Point3, r: f64) -> [f64; 4] {
    let s = face2::surface(b, f).unwrap().unwrap();
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let (mut surf, mut radial, mut edges, mut seam) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..=16 {
        for j in 0..=16 {
            let uv = dom.evaluate(f64::from(i) / 16.0, f64::from(j) / 16.0);
            let p = s.evaluate_point(k, uv).unwrap();
            surf = surf.max(((p - c).length() - r).abs());
            if i == 0 || j == 0 || i == 16 || j == 16 {
                continue; // the pole is the patch's degenerate side
            }
            let n = unit(s.evaluate_normal(k, uv, Side::Right, Side::Right).unwrap());
            radial = radial.max(1.0 - n.dot(unit(p - c)).abs());
        }
    }
    let others: Vec<FaceId> = get_faces(b)
        .unwrap()
        .into_iter()
        .filter(|&g| g != f)
        .collect();
    for e in solidcore::topology::topology_traverser::face_edges(b, f).unwrap() {
        let curve = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        for t in 0..=8 {
            let p = curve
                .evaluate_point(k, ivl.min() + (ivl.max() - ivl.min()) * f64::from(t) / 8.0)
                .unwrap();
            edges = edges.max(((p - c).length() - r).abs());
            if t == 0 || t == 8 {
                continue;
            }
            let want = unit(p - c);
            for &g in &others {
                let gs = face2::surface(b, g).unwrap().unwrap();
                let Ok(cp) = surface_closest_point(k, gs, p) else {
                    continue;
                };
                let Some(&uv) = cp.uvs.first() else {
                    continue;
                };
                if gs.evaluate_point(k, uv).unwrap().distance(p) > 1e-7 {
                    continue;
                }
                let Ok(n) = gs.evaluate_normal(k, uv, Side::Right, Side::Right) else {
                    continue;
                };
                // the angle between the normals' lines, as its sine (acos near 1 resolves
                // only to 2e-8)
                seam = seam.max(unit(n).cross(want).length());
            }
        }
    }
    [surf, edges, radial, seam]
}

/// The regular apex of `n` faces, rounded 3.
fn regular_apex(n: usize, blend: bool) {
    let mut k = kernel(blend);
    let r = 3.0;
    let apex = Point3::new(10.0, 10.0, 25.0);
    let ns = apex_normals(&vec![1.0; n], false);
    let es = apex_edges(&ns, false);
    // the common ball: on the axis, r / n_z below the apex
    let c = Point3::new(10.0, 10.0, 25.0 - r / ns[0].z);
    for v in &ns {
        assert!((v.dot(c - apex) + r).abs() < 1e-12);
    }
    let mut b = apex_body(&mut k, &ns, false);
    let before = b.faces().ids().count();
    let picks: Vec<EdgeId> = es.iter().map(|&d| pick(&k, &b, apex + d * 8.0)).collect();
    let (event, lines) = round(&mut k, &mut b, &picks, r, apex);
    let faces = b.faces().ids().count();
    if !blend {
        assert_eq!(event, None, "the pinned NxN corner");
        assert!(sphere_patches(&k, &b, c, r).is_empty());
        return;
    }
    assert_eq!(event, Some(BlendCornerEvent::Ball));
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(faces, before + 2 * n - 2);
    let patches = sphere_patches(&k, &b, c, r);
    assert_eq!(patches.len(), n - 2);
    assert_definitions_or_listed("the Ball", &k, &b);
    for f in patches {
        let [surf, edges, radial, seam] = ball_patch_offs(&k, &b, f, c, r);
        eprintln!(
            "apex {n}: surf {surf:.3e} edges {edges:.3e} radial {radial:.3e} seam {seam:.3e}"
        );
        assert!(
            surf <= 1e-12 * 30.0 && edges <= 1e-12 * 30.0,
            "{surf} {edges}"
        );
        assert!(radial <= 1e-12, "{radial}");
        assert!(seam <= 1e-9, "{seam}");
    }
}

#[test]
fn regular_apex_of_four_is_the_ball() {
    regular_apex(4, true);
}

#[test]
fn regular_apex_of_five_is_the_ball() {
    regular_apex(5, true);
}

#[test]
fn regular_apex_of_six_is_the_ball() {
    regular_apex(6, true);
}

#[test]
fn regular_apex_of_four_without_the_flag_is_the_pinned_corner() {
    regular_apex(4, false);
}

/// The capi's `setback_apex`: an apex (or a pocket's floor) of four faces leaning unequally,
/// no common ball, rounded 1: a Setback, sound, the sharp body's faces plus 3 n.
fn setback_apex(leans: &[f64], pocket: bool) {
    let mut k = kernel(true);
    let n = leans.len();
    let r = 1.0;
    let apex = if pocket {
        Point3::new(10.0, 10.0, 10.0)
    } else {
        Point3::new(10.0, 10.0, 25.0)
    };
    let ns = apex_normals(leans, pocket);
    let es = apex_edges(&ns, pocket);
    let mut b = apex_body(&mut k, &ns, pocket);
    let before = b.faces().ids().count();
    let picks: Vec<EdgeId> = es.iter().map(|&d| pick(&k, &b, apex + d * 3.0)).collect();
    let (event, lines) = round(&mut k, &mut b, &picks, r, apex);
    assert_eq!(event, Some(BlendCornerEvent::Setback));
    assert!(lines.is_empty(), "{lines:?}");
    assert_eq!(b.faces().ids().count(), before + 3 * n);
    assert_definitions_or_listed("the Setback", &k, &b);
}

/// Its G1 fill is certified only with the exact zeros of `l` and `m` along the patches'
/// sides on the face planes (3e10318): without them the fillet fails (`SM_ERR`).
#[test]
fn four_face_apex_without_a_common_ball_is_the_setback() {
    setback_apex(&[1.0, 1.4, 0.8, 1.2], false);
}

/// An all-concave corner of four edges without a common ball: the concave Setback, its fill
/// in the pocket.
#[test]
fn four_face_pocket_floor_is_the_concave_setback() {
    setback_apex(&[0.6, 0.8, 0.5, 0.7], true);
}

/// WR-502's pyramid: the worst deviation of its apex's patches from the ball touching the
/// four faces (and their count), whether the apex is the Ball.
fn rounded_pyramid(blend: bool, four_arcs: bool) -> (f64, usize, Option<BlendCornerEvent>) {
    let mut k = Kernel::default();
    k.fixes.fillet_blend_corner = blend;
    k.fixes.fillet_corner_sphere_from_four_arcs = four_arcs;
    let mut b = create_pyramid(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0).unwrap();
    let edges = b.edge_list().to_vec();
    let (event, lines) = round(&mut k, &mut b, &edges, 2.0, Point3::new(0.0, 0.0, 30.0));
    assert!(lines.is_empty(), "{lines:?}");
    let c = Point3::new(0.0, 0.0, (600.0 - 2.0 * 1300.0_f64.sqrt()) / 20.0);
    let patches = sphere_patches(&k, &b, c, 2.0);
    let mut off = 0.0f64;
    for &f in &patches {
        let [surf, edges, radial, seam] = ball_patch_offs(&k, &b, f, c, 2.0);
        off = off.max(surf).max(edges).max(radial).max(seam);
    }
    (off, patches.len(), event)
}

#[test]
fn pyramid_apex_is_the_ball_with_or_without_the_four_arc_sphere() {
    for four_arcs in [false, true] {
        let (off, patches, event) = rounded_pyramid(true, four_arcs);
        assert_eq!(event, Some(BlendCornerEvent::Ball));
        assert_eq!(patches, 2);
        assert!(off <= 1e-9, "{off}");
    }
}
