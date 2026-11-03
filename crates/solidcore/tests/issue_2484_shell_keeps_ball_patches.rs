//! tangency-labs/solidcore#2484 (item 4 of #2405): a 30 x 20 x 10 box with all twelve
//! edges rounded r 3 (eight Ball corners, each face an `SmBallPatch`), shelled 2 in
//! (closed). The cavity is the box rounded r 1 about the same inner box, so each of its
//! eight corner faces is on the sphere of radius 1 concentric with its outer corner.
//! The shell's extended offset (`SmOffsetExecutive::GetCreateExtendedSurface`) used to
//! extend those patches tangentially (`SM_CT_G1R`) into plain B-splines, and 7 of the 8
//! inner corners lost their sphere; with every fix on, each is a ball patch on its
//! sphere, the volume is the closed form's and the body checks clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{get_edges, get_faces};
use solidcore::surface::ball_patch::BallPatch;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::topology_traverser::face_edges;
use solidcore::topology::{edge, FaceId};
use solidcore::{Brep, Kernel};
use std::f64::consts::PI;

const A: f64 = 30.0;
const B: f64 = 20.0;
const C: f64 = 10.0;
const R: f64 = 3.0;
const WALL: f64 = 2.0;

/// Every fix but `geometric_g2_continuity`.
fn all_fixes() -> Kernel {
    all_fixes_but("geometric_g2_continuity")
}

/// Every fix but `geometric_g2_continuity` and `off`.
fn all_fixes_but(off: &str) -> Kernel {
    let mut k = Kernel::default();
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" && name != off {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

/// The box rounded `R` on all twelve edges, shelled `WALL` in, closed.
fn shelled_box(k: &mut Kernel) -> Brep {
    let mut part = create_box(k, Point3::new(0.0, 0.0, 0.0), A, B, C).unwrap();
    let edges = get_edges(&part).unwrap();
    assert_eq!(edges.len(), 12);
    let xs = vec![FilletSurfaceGeneratorType::Circular; edges.len()];
    {
        let mut s = FilletSession::new(k, &mut part).unwrap();
        s.prepare_per_edge_constant_radius(k, &edges, &[R; 12], &xs, 1, 1.0)
            .unwrap();
        s.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, &mut part, &StitchAndOrientOptions::default()).unwrap();
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -WALL,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: false,
    };
    shell_brep_simple(k, &part, options, &[]).unwrap()
}

/// The centre of corner `corner` (bit 0: x, bit 1: y, bit 2: z at the far side): `R` in
/// from each of its three faces.
fn centre(corner: usize) -> Point3 {
    Point3::new(
        if corner & 1 != 0 { A - R } else { R },
        if corner & 2 != 0 { B - R } else { R },
        if corner & 4 != 0 { C - R } else { R },
    )
}

/// The faces with three vertices, all on the sphere about `centre(corner)` of radius
/// `radius`.
fn corner_faces(k: &Kernel, b: &Brep, corner: usize, radius: f64) -> Vec<FaceId> {
    let c = centre(corner);
    let mut out = Vec::new();
    for f in get_faces(b).unwrap() {
        let mut points: Vec<Point3> = Vec::new();
        for e in face_edges(b, f).unwrap() {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            for t in [ivl.min(), ivl.max()] {
                let p = curve.evaluate_point(k, t).unwrap();
                if !points.iter().any(|q| q.distance(p) <= 1e-9) {
                    points.push(p);
                }
            }
        }
        if points.len() == 3
            && points
                .iter()
                .all(|p| ((*p - c).length() - radius).abs() <= 1e-9)
        {
            out.push(f);
        }
    }
    out
}

/// The rounded box's volume: its inner box `x y z` swept by a ball of radius `r`
/// (Steiner's formula).
fn rounded_box_volume(r: f64) -> f64 {
    let (x, y, z) = (A - 2.0 * R, B - 2.0 * R, C - 2.0 * R);
    x * y * z
        + 2.0 * r * (x * y + y * z + z * x)
        + PI * r * r * (x + y + z)
        + 4.0 / 3.0 * PI * r.powi(3)
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-12, &attrs).unwrap()
}

/// The patch's worst distance off its sphere over a 17 x 17 grid of its natural domain.
fn off_sphere(p: &BallPatch) -> f64 {
    let net = p.net();
    let dom = net.natural_uv_domain();
    let mut worst = 0.0f64;
    for i in 0..=16 {
        for j in 0..=16 {
            let q = net
                .evaluate_point(dom.evaluate(f64::from(i) / 16.0, f64::from(j) / 16.0))
                .unwrap();
            worst = worst.max(((q - p.centre()).length() - p.radius()).abs());
        }
    }
    worst
}

/// Every corner of the shell, outside (radius 3) and in the cavity (radius 1), is one
/// ball patch on its sphere: the outer ones' definitions are the Ball's, the inner ones'
/// the concentric sphere of radius `R - WALL`, centre and radius to the bit or 1e-12,
/// the net on the sphere over its whole domain to 1e-12.
#[test]
fn every_inner_corner_stays_a_ball_patch_on_its_sphere() {
    let mut k = all_fixes();
    let shell = shelled_box(&mut k);
    let mut kinds = Vec::new();
    for corner in 0..8 {
        for want in [R, R - WALL] {
            let faces = corner_faces(&k, &shell, corner, want);
            assert_eq!(faces.len(), 1, "corner {corner}, radius {want}");
            let s = face2::surface(&shell, faces[0]).unwrap().unwrap();
            let SurfaceKind::BallPatch(p) = s.kind() else {
                kinds.push(format!("corner {corner} r {want}: {:?}", s.type_code()));
                continue;
            };
            let c = centre(corner);
            assert!(
                p.centre().distance(c) <= 1e-12,
                "corner {corner} r {want}: centre {:?}",
                p.centre()
            );
            assert!(
                (p.radius() - want).abs() <= 1e-12,
                "corner {corner}: radius {} for {want}",
                p.radius()
            );
            let off = off_sphere(p);
            assert!(off <= 1e-12, "corner {corner} r {want}: {off:e} off");
            assert!(BallPatch::is_on_sphere(p.net(), p.centre(), p.radius(), 1e-12).unwrap());
        }
    }
    assert!(kinds.is_empty(), "not ball patches: {kinds:#?}");
}

/// The shell's volume is the outer rounded box's less the cavity's (the box rounded
/// `R - WALL` about the same inner box), and the body checks clean.
#[test]
fn the_shell_has_the_closed_form_volume_and_checks_clean() {
    let mut k = all_fixes();
    let mut shell = shelled_box(&mut k);
    let v = volume(&k, &mut shell);
    let want = rounded_box_volume(R) - rounded_box_volume(R - WALL);
    let lines = check_brep(&mut k, &mut shell, CheckLevel::Level2)
        .unwrap()
        .lines();
    eprintln!(
        "volume {v:.12} want {want:.12} ({:.3e}); {lines:?}",
        (v - want).abs() / want
    );
    assert!((v - want).abs() <= 1e-12 * want, "{v} vs {want}");
    assert!(lines.is_empty(), "{lines:?}");
}

/// Without `offset_extends_ball_patches_as_themselves` the extended offset extends a ball
/// patch tangentially, a plain B-spline: only the inner corner whose trims stay inside
/// its patch's domain to `EFF_ZERO` (corner 1, at (27, 3, 3)) keeps its patch; the other
/// seven reach 1.6e-12 to 2.4e-11 past it in their trims' rounding and keep the B-spline.
#[test]
fn without_the_fix_seven_inner_corners_lose_their_sphere() {
    let mut k = all_fixes_but("offset_extends_ball_patches_as_themselves");
    let shell = shelled_box(&mut k);
    let mut kept = Vec::new();
    for corner in 0..8 {
        let faces = corner_faces(&k, &shell, corner, R - WALL);
        assert_eq!(faces.len(), 1, "corner {corner}");
        let s = face2::surface(&shell, faces[0]).unwrap().unwrap();
        if matches!(s.kind(), SurfaceKind::BallPatch(_)) {
            kept.push(corner);
        }
    }
    assert_eq!(kept, [1]);
}
