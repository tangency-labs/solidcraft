//! Rounds that overlap where a wall is thinner than both (SolidCraft #871): a plate with a
//! narrower block on top and a window through both, rounded along every edge of both end faces.
//! And rounds whose bands never meet, one of them ending at a crease (SolidCraft #16729).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;

use solidcore::blend::definition_check::surface_kind_name;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::Vec3;

use common::*;
use solidcore::kernel::Kernel;
use solidcore_ops::*;

const A: f64 = 12.744_373_655_385_26;
const B: f64 = 22.692_759_633_669_36;

/// The part from #871.
fn part() -> Body {
    built(|k| {
        let bx = |k: &mut Kernel, lo: [f64; 3], hi: [f64; 3]| {
            create_box(
                k,
                Vec3::new(lo[0], lo[1], lo[2]),
                hi[0] - lo[0],
                hi[1] - lo[1],
                hi[2] - lo[2],
            )
        };
        let base = bx(k, [9.5, 8.0, 0.0], [23.5, 29.5, 10.0])?;
        let top = bx(k, [9.5, A, 10.0], [23.5, B, 20.0])?;
        let joined = boolean_union(k, base, top)?;
        let window = bx(k, [9.5, 14.0, 7.0], [23.5, 19.2, 14.1])?;
        boolean_difference(k, joined, window)
    })
}

/// The 24 edges of both end faces.
fn end_edges(b: &Body) -> Vec<usize> {
    let yz = [
        (A, 15.0),
        (10.372_186_827_692_63, 10.0),
        (8.0, 5.0),
        (18.75, 0.0),
        (29.5, 5.0),
        (26.096_379_816_834_68, 10.0),
        (B, 15.0),
        (17.718_566_644_527_31, 20.0),
        (16.6, 7.0),
        (14.0, 10.55),
        (16.6, 14.1),
        (19.2, 10.55),
    ];
    [23.5, 9.5]
        .iter()
        .flat_map(|&x| yz.iter().map(move |&(y, z)| [x, y, z]))
        .map(|p| edge_at(b, p))
        .collect()
}

/// Radius 1 on both the window's edges and the block's sides: the wall between them is 1.26
/// thick, so the rounds would overlap and the end faces' boundaries cross. The kernel refuses
/// it before building (its bands overlap). Below half the wall the rounds fit (SolidCraft's
/// closed form: each end face's outline and window, 107.6 long together, give up
/// r^2 (1 - pi/4) per length at both ends, the mitred corners cancelling), and the rounds' faces
/// and their neighbours cross nowhere (`rounds_cross` passes, the check finds nothing).
#[test]
fn overlapping_rounds_are_refused_and_rounds_that_fit_close() {
    all_fixes();
    let p = part();
    let edges = end_edges(&p);
    let part_volume = 14.0 * (21.5 * 10.0 + (B - A) * 10.0 - 5.2 * 7.1);
    let v = p.volume(1e-8).unwrap();
    assert!((v - part_volume).abs() < 1e-9 * part_volume, "{v}");
    let c = p.copy().unwrap();
    match blend(&c, Blend::Round, &edges, &[1.0], &[], Corners::Filled) {
        Err(Error::TooLarge {
            why: Some(RoomReason::BandsOverlap),
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    for r in [0.5, 0.62] {
        let c = p.copy().unwrap();
        blend(&c, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
        let length = 2.0 * (21.5 + 20.0) + 2.0 * (5.2 + 7.1);
        let removed = 2.0 * length * r * r * (1.0 - std::f64::consts::FRAC_PI_4);
        let v = c.volume(1e-8).unwrap();
        assert!(
            (v - (part_volume - removed)).abs() < 1e-7 * part_volume,
            "{r}: {v} vs {}",
            part_volume - removed
        );
        assert!(c.check(CheckLevel::All).unwrap().is_valid());
    }
}

/// tan 4 degrees: the slope of the creased wall's turned piece.
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn crease_t() -> f64 {
    4.0_f64.to_radians().tan()
}

/// The creased wall and arm of SolidCraft #16729: a block x -10..12, y 0..12, z 0..16 cut back
/// to a wall at y = 10 that turns 4 degrees about the vertical at x = `crease` (the plane y = 10
/// for x > `crease`, y = 10 + (`crease` - x) tan 4 for x < `crease`), and an arm x 0..12,
/// y 10..16, z 0..8 joined against it.
fn creased_wall_arm(crease: f64) -> Body {
    built(|k| {
        let bx = |k: &mut Kernel, lo: [f64; 3], hi: [f64; 3]| {
            create_box(
                k,
                Vec3::new(lo[0], lo[1], lo[2]),
                hi[0] - lo[0],
                hi[1] - lo[1],
                hi[2] - lo[2],
            )
        };
        let block = bx(k, [-10.0, 0.0, 0.0], [12.0, 12.0, 16.0])?;
        let straight = bx(k, [crease, 10.0, -1.0], [crease + 20.0, 20.0, 17.0])?;
        let block = boolean_difference(k, block, straight)?;
        let mut turned = bx(k, [-30.0, 0.0, -1.0], [0.0, 20.0, 17.0])?;
        let z = Vec3::new(0.0, 0.0, 1.0);
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut turned)),
            Vec3::ZERO,
            z,
            -4.0,
        )?;
        let to_crease = Vec3::new(crease, 10.0, 0.0);
        api_translate(k, Some(TransformObject::Brep(&mut turned)), to_crease)?;
        let block = boolean_difference(k, block, turned)?;
        let arm = bx(k, [0.0, 10.0, 0.0], [12.0, 16.0, 8.0])?;
        boolean_union(k, block, arm)
    })
}

/// A body's face, edge, vertex and shell counts, volume and its faces' surface kinds, sorted. One
/// closed solid has a plain block's shells (the kernel's two, its inside and outside).
type Measured = ([usize; 4], f64, Vec<String>);

/// Rounds r 0.87 on the edges through `points` of the creased wall and arm, its crease at
/// x = 2.5: what [`Measured`] holds, or the refusal.
fn round_creased(points: &[[f64; 3]]) -> std::result::Result<Measured, String> {
    let b = creased_wall_arm(2.5);
    if !points.is_empty() {
        let edges: Vec<usize> = points.iter().map(|&p| edge_at(&b, p)).collect();
        blend(&b, Blend::Round, &edges, &[0.87], &[], Corners::Filled)
            .map_err(|e| e.to_string())?;
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{points:?}");
    }
    let (_, made) = kernel_copy(&b);
    let mut kinds: Vec<String> = made
        .face_list()
        .iter()
        .map(|&f| surface_kind_name(made.face(f).unwrap().surface.as_ref().unwrap()).to_string())
        .collect();
    kinds.sort();
    let [faces, edges, vertices] = b.counts().unwrap();
    let counts = [faces, edges, vertices, made.shells().len()];
    Ok((counts, b.volume(1e-10).unwrap(), kinds))
}

/// Three rounds r 0.87 on the creased wall and arm: A where the arm's top meets the straight wall
/// (x 2.5..12), which ends at the crease, where its edge runs on 4 degrees off line into the
/// short piece on the turned wall, left sharp; B where the arm's end face x = 0 meets the turned
/// wall; C, the arm's top end edge at x = 0. A's band on the arm's top lies on x >= 2.5 and C's
/// on x <= 0.87: the rounds never meet, so A with C, and A with B and C, build, and what each
/// changes adds up. The band check took A's rail on past the crease, where the corners had not
/// yet placed its end, through C's band, and refused them as overlapping.
///
/// The closed forms: the body before, the block less both cuts and the arm less its overlap
/// with the block (the wedge between the turned wall and y = 10 over 0 <= x <= 2.5),
/// `4096 + 1225 t` (`t = tan 4`). A, a concave round of a right angle 9.5 long, adds
/// `9.5 a r^2` (`a = 1 - pi/4`). C, a convex one, runs from the turned wall to the arm's side
/// y = 16; a point of its section `u` from its edge lies `6 - (2.5 - u) t` long, so it takes
/// `(6 - 2.5 t) a r^2 + t m r^3` (`m = 5/6 - pi/4`, the section's moment about its edge).
///
/// With the crease at x = 0.5, A's band on the arm's top starts at x = 0.5, inside C's: the
/// bands do overlap there, and A with C is still refused as overlapping.
#[test]
fn a_round_ending_at_a_crease_is_not_refused_over_rounds_it_never_reaches() {
    all_fixes();
    let (r, t) = (0.87, crease_t());
    let (a, m) = (
        1.0 - std::f64::consts::FRAC_PI_4,
        5.0 / 6.0 - std::f64::consts::FRAC_PI_4,
    );
    let edge_a = [6.0, 10.0, 8.0];
    let edge_b = [0.0, 10.0 + 2.5 * t, 4.0];
    let edge_c = [0.0, 13.0, 8.0];
    let before = round_creased(&[]).unwrap();
    let alone_a = round_creased(&[edge_a]).unwrap();
    let alone_c = round_creased(&[edge_c]).unwrap();
    let b_and_c = round_creased(&[edge_b, edge_c]).unwrap();
    let a_and_c = round_creased(&[edge_a, edge_c]);
    let all = round_creased(&[edge_a, edge_b, edge_c]);
    let solid = kernel_copy(&block()).1.shells().len();
    let v0 = 4096.0 + 1225.0 * t;
    let gain_a = 9.5 * a * r * r;
    let loss_c = (6.0 - 2.5 * t) * a * r * r + t * m * r * r * r;
    assert!(rel(before.1, v0) < 1e-12, "before: {}", before.1);
    assert!(rel(alone_a.1, v0 + gain_a) < 1e-9, "A: {}", alone_a.1);
    assert!(rel(alone_c.1, v0 - loss_c) < 1e-9, "C: {}", alone_c.1);
    // each round a cylinder (a cone of no angle), A's end at the crease a plane cap, B and C's
    // corner a patch; every other face a plane
    let kinds = |cones: usize, patches: usize, planes: usize| {
        let mut k = vec!["b-spline".to_string(); patches];
        k.extend(vec!["cone".to_string(); cones]);
        k.extend(vec!["plane".to_string(); planes]);
        k
    };
    assert_eq!(
        (before.0, &before.2),
        ([10, 24, 16, solid], &kinds(0, 0, 10))
    );
    assert_eq!(
        (alone_a.0, &alone_a.2),
        ([12, 29, 19, solid], &kinds(1, 0, 11))
    );
    assert_eq!(
        (alone_c.0, &alone_c.2),
        ([11, 27, 18, solid], &kinds(1, 0, 10))
    );
    assert_eq!(
        (b_and_c.0, &b_and_c.2),
        ([13, 32, 21, solid], &kinds(2, 1, 10))
    );
    let a_and_c = a_and_c.expect("A with C builds");
    assert!(
        rel(a_and_c.1, v0 + gain_a - loss_c) < 1e-9,
        "A C: {}",
        a_and_c.1
    );
    assert_eq!(
        (a_and_c.0, &a_and_c.2),
        ([13, 32, 21, solid], &kinds(2, 0, 11))
    );
    let all = all.expect("A with B and C builds");
    // A alone + B and C alone - the body before, the issue's 4183.769446 (each measured to its
    // six decimals), and the counts the same sum
    assert!(
        rel(all.1, alone_a.1 + b_and_c.1 - before.1) < 1e-9,
        "A B C: {}",
        all.1
    );
    assert!((all.1 - 4183.769446).abs() < 2e-6, "A B C: {}", all.1);
    assert_eq!((all.0, &all.2), ([15, 37, 24, solid], &kinds(3, 1, 11)));

    let overlapping = creased_wall_arm(0.5);
    let edges = [edge_at(&overlapping, edge_a), edge_at(&overlapping, edge_c)];
    match blend(
        &overlapping,
        Blend::Round,
        &edges,
        &[r],
        &[],
        Corners::Filled,
    ) {
        Err(Error::TooLarge {
            why: Some(RoomReason::BandsOverlap),
            ..
        }) => {}
        other => panic!("crease at 0.5, A with C: {other:?}"),
    }
}
