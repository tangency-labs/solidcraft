//! tangency-labs/solidcore#460: a 30 x 20 x 10 box with all twelve edges rounded r3,
//! shelled 2 inward, closed and with the top face open. At the c9c5979 pin the shell
//! failed in the offset's piecewise merge at the corner patches, in the Rust port every
//! time and in the C++ depending on heap layout (WR-347, ND-261, UB-261); usd-brep v1.0.1
//! builds it in every layout, and the port follows v1.0.1. The open-top shell stays a
//! probe (`--ignored`): v1.0.1 returns success for it, but whether that body is a valid
//! open-top shell is not settled (the SMLib fork's fails the level-2 check).
//!
//! The rounded box is the core 24 x 14 x 4 grown by 3; its closed shell's cavity is
//! the same core grown by 1. Open at the top, the cavity also takes the 24 x 14 x 2
//! slab between the cavity's flat top (z = 8) and the removed top face (z = 10).
//!
//! tangency-labs/solidcraft#15108 is the same family at r5 on a 40 x 30 x 20 box: its open
//! shell failed only with the box centred on `y = 0` (the ball extension's farthest-point
//! angle, `surface::extend::ball`).
//!
//! tangency-labs/solidcraft#16983: a rib thinner than twice the wall, its top edges rounded
//! thinner than the wall, shelled open at the bottom. The offsets of the rib's two sides
//! cross over each other, so the offset of the rib's top and of the end faces across the
//! rib turn inside out; the shell was refused ("an edge of its offset closes to nothing").
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use core::f64::consts::PI;

use offset_fixture::{api_shell, planar_faces_at, report, shell, volume};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The core `a x b x c` grown by `r` (Minkowski sum with a ball).
fn grown_box(a: f64, b: f64, c: f64, r: f64) -> f64 {
    a * b * c
        + 2.0 * r * (a * b + b * c + a * c)
        + PI * r * r * (a + b + c)
        + 4.0 / 3.0 * PI * r * r * r
}

fn rounded_box(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let edges: Vec<_> = b.edges().ids().collect();
    fillet_edges(k, &mut b, &edges, 3.0, 1, 1, 1.0).unwrap();
    b
}

#[test]
fn issue_460_rounded_box_input() {
    let mut k = Kernel::default();
    let mut b = rounded_box(&mut k);
    let want = grown_box(24.0, 14.0, 4.0, 3.0);
    let got = volume(&k, &mut b).unwrap();
    eprintln!(
        "PROBE issue_460 input: faces={} volume={got} want={want}",
        b.face_list().len()
    );
    assert!((got - want).abs() <= 1e-5 * want, "{got} vs {want}");
}

#[test]
fn issue_460_closed_shell() {
    let mut k = Kernel::default();
    let b = rounded_box(&mut k);
    let want = grown_box(24.0, 14.0, 4.0, 3.0) - grown_box(24.0, 14.0, 4.0, 1.0);
    let result = shell(&mut k, b, -2.0, &[]);
    assert!(report(&k, "issue_460 closed", result, Some(want), 1e-5));
}

#[test]
#[ignore = "probe: the open-top shell's validity is not settled (see the module notes)"]
fn issue_460_open_top_shell() {
    let mut k = Kernel::default();
    let b = rounded_box(&mut k);
    let top = planar_faces_at(&k, &b, 2, 10.0);
    assert_eq!(top.len(), 1, "the top face");
    let want =
        grown_box(24.0, 14.0, 4.0, 3.0) - grown_box(24.0, 14.0, 4.0, 1.0) - 24.0 * 14.0 * 2.0;
    let result = shell(&mut k, b, -2.0, &top);
    assert!(report(&k, "issue_460 open top", result, Some(want), 1e-5));
}

/// Narrows the closed-shell failure: the box with one edge, the four vertical edges,
/// the four top edges and all twelve edges rounded r3, each shelled closed 2 inward
/// (reported only).
#[test]
#[ignore = "diagnostic probe, run on demand"]
fn issue_460_closed_shell_by_round_set() {
    for set in [
        "one vertical edge",
        "four vertical edges",
        "four top edges",
        "all twelve edges",
    ] {
        let mut k = Kernel::default();
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
        let edges: Vec<_> = b
            .edges()
            .iter()
            .filter(|(_, e)| {
                let c = e.curve.as_ref().unwrap();
                let p = c.evaluate_point(&k, e.interval.min()).unwrap();
                let q = c.evaluate_point(&k, e.interval.max()).unwrap();
                let vertical = (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9;
                let top = (p.z - 10.0).abs() < 1e-9 && (q.z - 10.0).abs() < 1e-9;
                match set {
                    "one vertical edge" => vertical && p.x.abs() < 1e-9 && p.y.abs() < 1e-9,
                    "four vertical edges" => vertical,
                    "four top edges" => top,
                    _ => true,
                }
            })
            .map(|(id, _)| id)
            .collect();
        let rounded = fillet_edges(&mut k, &mut b, &edges, 3.0, 1, 1, 1.0);
        eprintln!(
            "PROBE issue_460 {set}: {} edges rounded: {rounded:?}",
            edges.len()
        );
        if rounded.is_err() {
            continue;
        }
        let result = shell(&mut k, b, -2.0, &[]);
        let _ = report(&k, &format!("issue_460 closed, {set}"), result, None, 0.0);
    }
}

/// tangency-labs/solidcraft#15108: a 40 x 30 x 20 box centred at `c`, all twelve edges
/// rounded r5 and shelled 2 inward with the top open, with the options SolidCraft's
/// `smlib_shell` passes and SolidCraft's fix set: every opt-in fix. Returns the shell's
/// volume.
fn shell_15108(c: [f64; 3]) -> f64 {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let corner = Point3::new(c[0] - 20.0, c[1] - 15.0, c[2] - 10.0);
    let mut b = create_box(&mut k, corner, 40.0, 30.0, 20.0).unwrap();
    let edges: Vec<_> = b.edges().ids().collect();
    fillet_edges(&mut k, &mut b, &edges, 5.0, 1, 1, 1.0).unwrap();
    let top = planar_faces_at(&k, &b, 2, c[2] + 10.0);
    assert_eq!(top.len(), 1, "the top face");
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -2.0,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let mut r = api_shell(&mut k, &mut b, options, &top)
        .unwrap_or_else(|e| panic!("centre {c:?}: the shell fails: {e:?}"));
    solidcore::topology::integrity::check(&r).unwrap();
    assert!(brep_is_manifold_solid(&r).unwrap(), "centre {c:?}");
    // the check SolidCraft runs on every result (`CheckLevel::All`)
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "centre {c:?}: {:?}",
        check.failures().collect::<Vec<_>>()
    );
    volume(&k, &mut r).unwrap()
}

/// #15108: the shell builds wherever the box sits. Centred on `y = 0` its corner balls'
/// poles are normal to their third sides' planes, and the ball extension took the
/// farthest point of that side from a direction that was all rounding (a half turn,
/// refused as "not a triangle under a half turn"); shifted, it built. Each position gives
/// the closed form: the r5 rounded box less its cavity, the core 30 x 20 x 10 grown by 3,
/// less the 30 x 20 x 2 slab the open top adds to it (to the volume integrator's 1e-6),
/// and the two positions agree far tighter.
#[test]
fn issue_15108_open_shell_of_a_rounded_box_is_independent_of_position() {
    let want =
        grown_box(30.0, 20.0, 10.0, 5.0) - grown_box(30.0, 20.0, 10.0, 3.0) - 30.0 * 20.0 * 2.0;
    let shifted = shell_15108([20.0, 15.0, 10.0]);
    for c in [
        [0.0, 0.0, 0.0],
        [20.0, 0.0, 10.0],
        [0.1, 0.0, 0.0],
        [20.0, 15.0, 10.0],
    ] {
        let got = shell_15108(c);
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "centre {c:?}: {got} vs {want}"
        );
        assert!(
            (got - shifted).abs() <= 1e-9 * shifted,
            "centre {c:?}: {got} vs {shifted} at [20, 15, 10]"
        );
    }
}

/// #16983: a 40 x 20 x 10 box with a 3 x 10 rib along its back face (y 0..3, z 10..20),
/// the rib's two top edges rounded r0.75, shelled `t` inward open at the bottom, with
/// SolidCraft's fix set (as [`shell_15108`]). Returns the shell's volume.
fn shell_ribbed_box(t: f64) -> f64 {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let base = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 20.0, 10.0).unwrap();
    let rib = create_box(&mut k, Point3::new(0.0, 0.0, 10.0), 40.0, 3.0, 10.0).unwrap();
    let mut b = boolean_union(&mut k, base, rib).unwrap();
    let top_edges: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(&k, e.interval.min()).unwrap();
            let q = c.evaluate_point(&k, e.interval.max()).unwrap();
            (p.z - 20.0).abs() < 1e-9 && (q.z - 20.0).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(top_edges.len(), 2, "the rib's top edges");
    fillet_edges(&mut k, &mut b, &top_edges, 0.75, 1, 1, 1.0).unwrap();
    let bottom = planar_faces_at(&k, &b, 2, 0.0);
    assert_eq!(bottom.len(), 1, "the bottom face");
    let options = ShellOptions {
        offset: OffsetOptions {
            distance: -t,
            extend_convex_edges: true,
            merge_results: true,
        },
        create_offset_solid: true,
        merge_sheet_side_walls: true,
    };
    let mut r = api_shell(&mut k, &mut b, options, &bottom)
        .unwrap_or_else(|e| panic!("wall {t}: the shell fails: {e:?}"));
    solidcore::topology::integrity::check(&r).unwrap();
    assert!(brep_is_manifold_solid(&r).unwrap(), "wall {t}");
    // the check SolidCraft runs on every result (`CheckLevel::All`)
    let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "wall {t}: {:?}",
        check.failures().collect::<Vec<_>>()
    );
    volume(&k, &mut r).unwrap()
}

/// #16983: walls of 1.6 and 2, thicker than half the 3 mm rib, leave the rib solid. The
/// cavity is the box less its walls, `(40 - 2t) x (20 - 2t) x (10 - t)`: the rib's sides'
/// offsets cross, and its rounds' offsets collapse (the wall is thicker than r0.75), so
/// nothing of the cavity reaches into the rib. The shell is the body (the box, the rib, less
/// the two rounds' `(1 - pi/4) r^2` over the 40 length) less that cavity.
#[test]
fn issue_16983_a_rib_thinner_than_two_walls_stays_solid() {
    let body = 40.0 * 20.0 * 10.0 + 40.0 * 3.0 * 10.0 - 2.0 * (1.0 - PI / 4.0) * 0.75 * 0.75 * 40.0;
    for t in [1.6, 2.0] {
        let want = body - (40.0 - 2.0 * t) * (20.0 - 2.0 * t) * (10.0 - t);
        let got = shell_ribbed_box(t);
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "wall {t}: {got} vs {want}"
        );
    }
}

/// #18801, stress corpus S0643 before its final rigid rotations: the 4 mm square
/// bosses are narrower than twice the 2.4 mm wall and must stay solid. The input
/// volume is 60*40*25 - 4.8*2.4*12.5 + 2*4*4*10. The mitred cavity is the inset
/// block, 55.2*35.2*20.2, less the notch's offset intrusion, 9.6*2.4*12.5.
/// Both directions of each boss top invert: its offset loop still has positive
/// area, so loop orientation alone cannot identify the vanished cavity branch.
#[test]
fn issue_18801_closed_shell_absorbs_square_bosses() {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let block = create_box(&mut k, Point3::new(-30.0, -20.0, 0.0), 60.0, 40.0, 25.0).unwrap();
    let notch = create_box(&mut k, Point3::new(-2.4, 17.6, 12.5), 4.8, 4.8, 12.5).unwrap();
    let mut b = boolean_difference(&mut k, block, notch).unwrap();
    for y in [-6.0, 6.0] {
        let boss = create_box(&mut k, Point3::new(-2.0, y - 2.0, 25.0), 4.0, 4.0, 10.0).unwrap();
        b = boolean_union(&mut k, b, boss).unwrap();
    }
    let input = 60.0 * 40.0 * 25.0 - 4.8 * 2.4 * 12.5 + 2.0 * 4.0 * 4.0 * 10.0;
    assert!((volume(&k, &mut b).unwrap() - input).abs() <= 1e-6 * input);
    let want = input - (55.2 * 35.2 * 20.2 - 9.6 * 2.4 * 12.5);
    let mut result = shell(&mut k, b, -2.4, &[]).unwrap();
    solidcore::topology::integrity::check(&result).unwrap();
    assert!(brep_is_manifold_solid(&result).unwrap());
    let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
    assert!(
        check.is_valid(),
        "{:?}",
        check.failures().collect::<Vec<_>>()
    );
    let got = volume(&k, &mut result).unwrap();
    assert!((got - want).abs() <= 1e-6 * want, "{got} vs {want}");
}

/// A short, wide boss still contains cavity above the inset block: its artificial
/// attachment plane is not an exposed face and must not be offset as a bottom.
/// With wall 2, the block cavity is 36*26*16. A 3-wide branch contributes nothing;
/// an 8-by-8 branch contributes (8-4)*(8-4)*height, even at height 0.5.
/// A second 3-by-10-by-5 boss on the same support always contributes no cavity;
/// closing its footprint must preserve the wider boss's neighboring footprint.
#[test]
fn issue_18801_branch_extinction_requires_crossed_exposed_planes() {
    for (width, depth, height, extra_cavity) in [
        (3.0, 10.0, 5.0, 0.0),
        (8.0, 8.0, 0.5, 4.0 * 4.0 * 0.5),
        (8.0, 8.0, 5.0, 4.0 * 4.0 * 5.0),
    ] {
        let mut k = Kernel::default();
        let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let boss = create_box(&mut k, Point3::new(16.0, 10.0, 20.0), width, depth, height).unwrap();
        let body = boolean_union(&mut k, block, boss).unwrap();
        let narrow = create_box(&mut k, Point3::new(3.0, 10.0, 20.0), 3.0, 10.0, 5.0).unwrap();
        let body = boolean_union(&mut k, body, narrow).unwrap();
        let want = 40.0 * 30.0 * 20.0 + 3.0 * 10.0 * 5.0 + width * depth * height
            - (36.0 * 26.0 * 16.0 + extra_cavity);
        let mut result = shell(&mut k, body, -2.0, &[])
            .unwrap_or_else(|e| panic!("boss {width} x {depth} x {height}: {e:?}"));
        solidcore::topology::integrity::check(&result).unwrap();
        assert!(brep_is_manifold_solid(&result).unwrap());
        let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
        assert!(
            check.is_valid(),
            "boss {width} x {depth} x {height}: {:?}",
            check.failures().collect::<Vec<_>>()
        );
        let got = volume(&k, &mut result).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "boss {width} x {depth} x {height}: {got} vs {want}"
        );
    }
}

/// `body` turned by `deg` about the vertical axis through the origin, then moved `dx` along x,
/// with the affine matrix a body move hands the kernel (`p' = p A + t`).
fn turned(k: &mut Kernel, mut body: Brep, deg: f64, dx: f64) -> Brep {
    let (s, c) = deg.to_radians().sin_cos();
    let m = [
        [c, s, 0.0, 0.0],
        [-s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [dx, 0.0, 0.0, 1.0],
    ];
    transform_matrix(k, &mut body, &m, true).unwrap();
    body
}

/// tangency-labs/solidcraft#22784: a 4.8 mm cube pad on a 60 x 40 x 25 block, turned about its
/// own vertical axis and moved along x, shelled closed with a 3 mm wall (more than half the
/// pad). The pad's opposite sides' offsets cross, so it stays solid at every angle and
/// position, as it does unturned: the cavity is the block's alone, 54 x 34 x 19, and the shell
/// is 60*40*25 + 4.8^3 - 54*34*19 = 25226.592. A turned pad's opposite normals agree only up to
/// rounding (cos 90 deg is 6e-17), and its offset planes' axes are not exactly orthogonal to its
/// source normals; the crossing was certified only for bit-equal ones.
#[test]
fn issue_22784_a_turned_narrow_pad_stays_solid_at_every_angle_and_position() {
    let want = 60.0 * 40.0 * 25.0 + 4.8 * 4.8 * 4.8 - 54.0 * 34.0 * 19.0;
    for (deg, dx) in [
        (0.0, 0.0),
        (10.0, 0.0),
        (30.0, 0.0),
        (44.0, 0.0),
        (45.0, 0.0),
        (46.0, 0.0),
        (60.0, 0.0),
        (90.0, 0.0),
        (45.0, 1.0),
        (45.0, 3.0),
        (45.0, 7.5),
        (45.0, 10.0),
        (45.0, 20.0),
        (30.0, -12.5),
    ] {
        let mut k = Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        };
        let block = create_box(&mut k, Point3::new(-30.0, -20.0, 0.0), 60.0, 40.0, 25.0).unwrap();
        let pad = create_box(&mut k, Point3::new(-2.4, -2.4, 25.0), 4.8, 4.8, 4.8).unwrap();
        let pad = turned(&mut k, pad, deg, dx);
        let b = boolean_union(&mut k, block, pad).unwrap();
        let mut result = shell(&mut k, b, -3.0, &[])
            .unwrap_or_else(|e| panic!("turned {deg} deg, moved {dx}: {e:?}"));
        solidcore::topology::integrity::check(&result).unwrap();
        assert!(brep_is_manifold_solid(&result).unwrap(), "{deg} deg, {dx}");
        let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
        assert!(
            check.is_valid(),
            "{deg} deg, {dx}: {:?}",
            check.failures().collect::<Vec<_>>()
        );
        let got = volume(&k, &mut result).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "turned {deg} deg, moved {dx}: {got} vs {want}"
        );
    }
}

/// #22784's control: turned, a boss wider than twice the wall still holds its cavity. On the
/// 40 x 30 x 20 block with wall 2 (cavity 36*26*16), an 8 x 8 x 5 boss turned about its own
/// axis adds (8-4)*(8-4)*5 of cavity at every angle: its sides' offsets do not cross, so the
/// certificate on its cell must fail rather than take the boss for solid.
#[test]
fn issue_22784_a_turned_wide_boss_keeps_its_cavity() {
    let want = 40.0 * 30.0 * 20.0 + 8.0 * 8.0 * 5.0 - (36.0 * 26.0 * 16.0 + 4.0 * 4.0 * 5.0);
    for deg in [10.0, 30.0, 45.0, 90.0] {
        let mut k = Kernel::default();
        let block = create_box(&mut k, Point3::new(-20.0, -15.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let boss = create_box(&mut k, Point3::new(-4.0, -4.0, 20.0), 8.0, 8.0, 5.0).unwrap();
        let boss = turned(&mut k, boss, deg, 0.0);
        let body = boolean_union(&mut k, block, boss).unwrap();
        let mut result =
            shell(&mut k, body, -2.0, &[]).unwrap_or_else(|e| panic!("turned {deg} deg: {e:?}"));
        solidcore::topology::integrity::check(&result).unwrap();
        assert!(brep_is_manifold_solid(&result).unwrap(), "{deg} deg");
        let got = volume(&k, &mut result).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "turned {deg} deg: {got} vs {want}"
        );
    }
}

/// tangency-labs/solidcraft#22783: a pad on a 60 x 40 x 25 block shelled closed with a wall of
/// exactly half the pad's width. The offsets of the pad's two opposite sides land on one plane,
/// so what the pad would hold of the cavity is part of that plane, of zero volume: the pad stays
/// solid, as it does at any thicker wall, and the cavity is the block's alone,
/// (60 - 2w) x (40 - 2w) x (25 - 2w). Refused before as walls meeting across the pad, naming
/// the very wall it refused as the largest that fits. A 4 x 8 pad at 2 meets only across its
/// narrow width; the 8 mm one keeps a 4 mm span, still of zero volume between the meeting
/// sides. Each corner and wall is a short binary fraction or a sum of them, so the opposite
/// offsets coincide exactly.
#[test]
fn issue_22783_a_pad_shelled_at_exactly_half_its_width_stays_solid() {
    for (width, depth, height, wall) in [
        (4.8, 4.8, 4.8, 2.4),
        (5.0, 5.0, 5.0, 2.5),
        (4.0, 8.0, 4.0, 2.0),
    ] {
        let mut k = Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        };
        let block = create_box(&mut k, Point3::new(-30.0, -20.0, 0.0), 60.0, 40.0, 25.0).unwrap();
        let pad = create_box(
            &mut k,
            Point3::new(-width / 2.0, -depth / 2.0, 25.0),
            width,
            depth,
            height,
        )
        .unwrap();
        let b = boolean_union(&mut k, block, pad).unwrap();
        let want = 60.0 * 40.0 * 25.0 + width * depth * height
            - (60.0 - 2.0 * wall) * (40.0 - 2.0 * wall) * (25.0 - 2.0 * wall);
        let mut result = shell(&mut k, b, -wall, &[])
            .unwrap_or_else(|e| panic!("pad {width} x {depth} at {wall}: {e:?}"));
        solidcore::topology::integrity::check(&result).unwrap();
        assert!(
            brep_is_manifold_solid(&result).unwrap(),
            "pad {width} x {depth}"
        );
        let check = check_brep(&mut k, &mut result, CheckLevel::Level2).unwrap();
        assert!(
            check.is_valid(),
            "pad {width} x {depth} at {wall}: {:?}",
            check.failures().collect::<Vec<_>>()
        );
        let got = volume(&k, &mut result).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "pad {width} x {depth} at {wall}: {got} vs {want}"
        );
    }
}
