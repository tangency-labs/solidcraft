//! Rounds that overlap where a wall is thinner than both (SolidCraft #871): a plate with a
//! narrower block on top and a window through both, rounded along every edge of both end faces.
//! And rounds whose bands never meet, one of them ending at a crease (SolidCraft #16729).
//! And rounds on the top edges of a block whose corners a sphere trims, whose rails cross past
//! the edges' ends (SolidCraft #18563). And rounds along a tab's two top edges, the outer one's
//! tangent chain running on round a ring's top edge to meet the inner one at a corner
//! (SolidCraft #21749).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;
/// #16735's leak checks on a result body read into the kernel: test code measuring geometry,
/// which this crate itself does not (docs/layers.md §4).
#[path = "../../solidcore/tests/support/volume_leak.rs"]
#[allow(clippy::disallowed_methods)]
mod volume_leak;

use solidcore::blend::definition_check::surface_kind_name;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::Vec3;

use common::*;
use solidcore::kernel::Kernel;
use solidcore_ops::boolean::boolean;
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
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

/// The ring of SolidCraft #18178: a 40 high disc of radius 14.5 less an equilateral triangle of
/// side 21 about its axis (corners at radius 12.124), whose top ring is 2.376 wide at the
/// triangle's corners. Chamfered 0.8 on the outer circle and the triangle's three top edges,
/// the triangle's chamfers meet 0.8 / sin 30 = 1.6 out from each 60 degree corner, at radius
/// 13.724, past the outer chamfer's contact line at 13.7: the lines cross there for a few
/// hundredths, which the band check's samples step over, and the chamfer is refused as too
/// large for the faces (the pieces between the crossings judged too). At 0.5 the corners reach
/// 13.124 and the outer line lies at 14.0: the four chamfers on the one face stay apart and
/// build a valid body a little smaller than the part.
#[test]
fn chamfers_crossing_at_a_ring_corner_are_refused_and_apart_build() {
    const C: [[f64; 3]; 3] = [
        [12.124_355_652_982_143, 0.0, -1.0],
        [-6.062_177_826_491_071, 10.5, -1.0],
        [-6.062_177_826_491_071, -10.5, -1.0],
    ];
    const EDGES: [[f64; 3]; 4] = [
        [-14.5, 0.0, 40.0],
        [3.031_088_913_245_535_6, 5.25, 40.0],
        [-6.062_177_826_491_071, 0.0, 40.0],
        [3.031_088_913_245_535_6, -5.25, 40.0],
    ];
    all_fixes();
    let disc = make_cylinder([0.0; 3], 14.5, 40.0).unwrap();
    let sides: Vec<Curve> = (0..3)
        .map(|i| Curve::Line {
            start: C[i],
            end: C[(i + 1) % 3],
        })
        .collect();
    let triangle = extrude(
        &sides,
        [0.0, 0.0, 1.0],
        42.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let ring = boolean(disc, triangle, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let edges: Vec<usize> = EDGES.iter().map(|&p| edge_at(&ring, p)).collect();
    let c = ring.copy().unwrap();
    match blend(&c, Blend::Chamfer, &edges, &[0.8], &[], Corners::Filled) {
        Err(
            Error::NoRoom
            | Error::TooLarge {
                why: Some(RoomReason::BandsOverlap),
                ..
            },
        ) => {}
        other => panic!("{other:?}"),
    }
    let whole = volume(&ring);
    let c = ring.copy().unwrap();
    blend(&c, Blend::Chamfer, &edges, &[0.5], &[], Corners::Filled).unwrap();
    let v = volume(&c);
    assert!(v > 0.99 * whole && v < whole, "{v} of {whole}");
    assert!(c.check(CheckLevel::All).unwrap().is_valid());
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
    // A alone + B and C alone - the body before, and the counts the same sum
    assert!(
        rel(all.1, alone_a.1 + b_and_c.1 - before.1) < 1e-9,
        "A B C: {}",
        all.1
    );
    // The issue's figure, 4183.769446, came from the integrator before #16735, which leaked
    // through the tolerant edges' gaps, and from B and C's corner patch with the retired
    // `corner_patch_face_side_twist` on (4183.769449703 once #16735 closed the gaps). Pinned
    // at the default kernel's body (MEASURED: no closed form for B and C's corner patch), to
    // 1e-8: since #18640 retired that flag, the corner keeps the default kernel's patch, and
    // main with the flag off reads this value to the last digit. The flag-on patch read
    // farther from tangent on two of its sides (#18640). The move against the body's old
    // leak bound (sum g L max|z - o_z| over its edge uses, the gaps g sampled at the
    // closure's grid, `volume_leak::leak`) is printed as a diagnostic only; the volume's
    // independence of the origin is asserted.
    assert!(
        (all.1 - 4_183.775_516_040_999).abs() < 1e-8,
        "A B C: {}",
        all.1
    );
    {
        let b = creased_wall_arm(2.5);
        let edges: Vec<usize> = [edge_a, edge_b, edge_c]
            .iter()
            .map(|&p| edge_at(&b, p))
            .collect();
        blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled).unwrap();
        let (k, mut made) = kernel_copy(&b);
        let l = volume_leak::leak(&k, &mut made, 1e-10, 8);
        eprintln!(
            "A B C (diagnostic, MEASURED): {} is {:e} from the issue's 4183.769446; old leak \
             bound {:e} (sampled), origin spread {:e} (allowance {:e})",
            all.1,
            (all.1 - 4183.769446).abs(),
            l.old_bound,
            l.origin_spread,
            l.origin_allowance
        );
        assert!(l.origin_spread <= l.origin_allowance, "A B C: {l:?}");
    }
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

/// SolidCraft #18563: a 60 x 40 x 25 block (`s` times as large) and a sphere of radius 35.75
/// about its centre, intersected, so the sphere trims every vertical edge and the top face is
/// the rectangle less the corners outside the circle of radius `rho` with
/// `rho^2 = 35.75^2 - 12.5^2` about its centre. The four top edges end on the sphere and share
/// no vertex.
fn sphere_trimmed_block(s: f64) -> (Body, [usize; 4]) {
    all_fixes();
    let block = make_box([-30.0 * s, -20.0 * s, 0.0], [60.0 * s, 40.0 * s, 25.0 * s]).unwrap();
    let ball = make_sphere([0.0, 0.0, 12.5 * s], 35.75 * s).unwrap();
    let part = boolean(block, ball, BooleanOp::Intersect).unwrap().unwrap();
    let top = [
        [30.0, -10.0, 25.0],
        [-30.0, 10.0, 25.0],
        [-15.0, -20.0, 25.0],
        [15.0, 20.0, 25.0],
    ]
    .map(|p| edge_at(&part, p.map(|c| c * s)));
    (part, top)
}

/// Rounding the four top edges of [`sphere_trimmed_block`]: past the edges' ends, the rails of
/// two neighbouring rounds on the top face run on as the lines `x = 30 - r` and `y = 20 - r`
/// until they meet the sphere. They cross at `(30 - r, 20 - r)`, inside the top face while
/// `(30 - r)^2 + (20 - r)^2 < rho^2`, so the largest round that fits solves
/// `2 r^2 - 100 r + 1300 - rho^2 = 0`: `r = 25 - sqrt(625 - (1300 - rho^2) / 2)`, about 1.850.
/// (At that radius the rounds' surfaces meet only at that point: their mitre's other points lie
/// outside the sphere.) A larger round, on all four edges or on two neighbours alone, is refused
/// for want of room and leaves the part as it was (the band check finds the overlap past the
/// edges' ends; the largest size that fits is not named there). A smaller one builds a valid
/// closed body of 14 faces, 38 edges and 26 vertices: the part's 10 faces, 26 edges and 18
/// vertices (six planes and the four sphere faces between them; eight vertices round the top
/// face, eight round the bottom, and two that split the side circles on the faces x = 30 and
/// y = -20 at z = 12.5), and each round adds a face, three edges and two vertices.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn rounds_whose_rails_cross_past_their_edges_ends_on_a_sphere_are_refused() {
    let rho2 = 35.75_f64.powi(2) - 12.5_f64.powi(2);
    let fit = 25.0 - (625.0 - (1300.0 - rho2) / 2.0).sqrt();
    assert!((fit - 1.850).abs() < 1e-3, "{fit}");
    for s in [1.0, 40.0] {
        let (part, top) = sphere_trimmed_block(s);
        assert_eq!(part.counts().unwrap(), [10, 26, 18], "part at scale {s}");
        let before = volume(&part);
        // all four edges, and the two neighbours at one corner alone; the body is left as it was
        for edges in [&top[..], &[top[0], top[3]][..]] {
            for r in [2.0, fit + 0.01] {
                let c = part.copy().unwrap();
                match blend(&c, Blend::Round, edges, &[r * s], &[], Corners::Filled) {
                    Err(Error::NoRoom) => {}
                    other => panic!("scale {s}, {} edges, round {r}: {other:?}", edges.len()),
                }
                assert_eq!(c.counts().unwrap(), [10, 26, 18], "scale {s}, round {r}");
                assert!(rel(volume(&c), before) < 1e-12, "scale {s}, round {r}");
            }
        }
        // two opposite edges share the top face but their bands lie apart: they build at 2, two
        // faces, six edges and four vertices more than the part
        let c = part.copy().unwrap();
        blend(
            &c,
            Blend::Round,
            &[top[0], top[1]],
            &[2.0 * s],
            &[],
            Corners::Filled,
        )
        .unwrap_or_else(|e| panic!("scale {s}, opposite edges: {e:?}"));
        assert_eq!(
            c.counts().unwrap(),
            [12, 32, 22],
            "scale {s}, opposite edges"
        );
        assert!(c.check(CheckLevel::All).unwrap().is_valid(), "scale {s}");
        for r in [1.0, fit - 0.01] {
            let c = part.copy().unwrap();
            blend(&c, Blend::Round, &top, &[r * s], &[], Corners::Filled)
                .unwrap_or_else(|e| panic!("scale {s}, round {r}: {e:?}"));
            assert_eq!(c.counts().unwrap(), [14, 38, 26], "scale {s}, round {r}");
            assert!(
                c.check(CheckLevel::All).unwrap().is_valid(),
                "scale {s}, round {r}"
            );
        }
    }
}

/// SolidCraft #21417: a 230 x 96 x 18 slab with a slot cut down 13 mm from z = 18
/// (`(55, 2) -> (55 + skew, 11) -> (225, 11) -> (225, 2)`) and a trench cut 20 mm toward -X from
/// x = 54 (`y` in `[0.75, 12.25]`, `z` in `[11.25, 19]`), leaving a ~1 mm wide land between them on
/// z = 18.
fn skewed_land_part(skew: f64, trench_tilt: f64, with_hole: bool) -> (Body, [usize; 2]) {
    all_fixes();
    let base = make_box([0.0, 0.0, 0.0], [230.0, 96.0, 18.0]).unwrap();
    let slot_pts = [
        [55.0, 2.0, 18.0],
        [55.0 + skew, 11.0, 18.0],
        [225.0, 11.0, 18.0],
        [225.0, 2.0, 18.0],
    ];
    let slot_curves: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: slot_pts[i],
            end: slot_pts[(i + 1) % 4],
        })
        .collect();
    let slot = extrude(
        &slot_curves,
        [0.0, 0.0, -1.0],
        13.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let with_slot = boolean(base, slot, BooleanOp::Subtract).unwrap().unwrap();
    let trench = if trench_tilt == 0.0 {
        make_box([34.0, 0.75, 11.25], [20.0, 11.5, 7.75]).unwrap()
    } else {
        let pt = |u: f64, v: f64| [54.0 + u * trench_tilt, u, v];
        let corners = [
            pt(0.75, 11.25),
            pt(0.75, 19.0),
            pt(12.25, 19.0),
            pt(12.25, 11.25),
        ];
        let curves: Vec<Curve> = (0..4)
            .map(|i| Curve::Line {
                start: corners[i],
                end: corners[(i + 1) % 4],
            })
            .collect();
        // Extrude along the tilted plane's normal toward -X.
        extrude(
            &curves,
            [-1.0, trench_tilt, 0.0],
            20.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    };
    let mut part = boolean(with_slot, trench, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    if with_hole {
        let hole = make_cylinder([54.5, 6.5, 5.0], 0.25, 14.0).unwrap();
        part = boolean(part, hole, BooleanOp::Subtract).unwrap().unwrap();
    }
    let trench_mid = [54.0 + 6.5 * trench_tilt, 6.5, 18.0];
    let edges = [
        edge_at(&part, [55.0 + 0.5 * skew, 6.5, 18.0]),
        edge_at(&part, trench_mid),
    ];
    (part, edges)
}

#[test]
fn chamfers_consuming_a_skewed_land_are_refused_with_fit() {
    // Expected closed fits: `narrow_width * cos(theta) / (1 + cos(theta))`,
    // with `cos(theta) = 9 / sqrt(81 + skew^2)`.
    let cases = [
        (0.0, 0.5),
        (1.4e-14, 0.5),
        (1e-10, 0.5),
        (1e-6, 0.5),
        (1e-5, 0.5),
        (1e-4, 0.5),
        (1e-3, 0.499_999_998_456_793_9),
        (-1e-6, 0.499_999_5),
        (1.0, 0.498_466_243_236_750_5),
    ];
    for (skew, expected_fit) in cases {
        let (part, edges) = skewed_land_part(skew, 0.0, false);
        for d in [0.9, 1.0] {
            let c = part.copy().unwrap();
            let err = blend(&c, Blend::Chamfer, &edges, &[d], &[], Corners::Filled).unwrap_err();
            let Error::TooLarge {
                size,
                kind: FitKind::Distance,
                face: Some(_),
                why: Some(RoomReason::BandsOverlap),
                ..
            } = err
            else {
                panic!("skew={skew:e} d={d}: expected TooLarge(BandsOverlap), got {err:?}");
            };
            assert!(
                (size - expected_fit).abs() <= 1e-9,
                "skew={skew:e} d={d}: fit {size} vs {expected_fit}"
            );
        }
        // Control below the limit still builds a valid closed solid.
        let control = if skew == 1.0 { 0.498 } else { 0.49 };
        let c = part.copy().unwrap();
        blend(&c, Blend::Chamfer, &edges, &[control], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("skew={skew:e} d={control}: {e:?}"));
        assert!(
            c.check(CheckLevel::All).unwrap().is_valid(),
            "skew={skew:e} d={control} invalid"
        );
    }
}

#[test]
fn chamfers_on_a_land_with_tilted_trench_plane_or_hole_are_refused_with_fit() {
    for tilt in [1e-15, 1e-12, 1e-10] {
        let (part, edges) = skewed_land_part(0.0, tilt, false);
        let c = part.copy().unwrap();
        let err = blend(&c, Blend::Chamfer, &edges, &[1.0], &[], Corners::Filled).unwrap_err();
        let Error::TooLarge {
            size,
            kind: FitKind::Distance,
            face: Some(_),
            why: Some(RoomReason::BandsOverlap),
            ..
        } = err
        else {
            panic!("tilt={tilt:e}: expected TooLarge(BandsOverlap), got {err:?}");
        };
        assert!((size - 0.5).abs() <= 1e-7, "tilt={tilt:e}: fit {size}");
    }
    let (part, edges) = skewed_land_part(1e-6, 0.0, true);
    let c = part.copy().unwrap();
    let err = blend(&c, Blend::Chamfer, &edges, &[1.0], &[], Corners::Filled).unwrap_err();
    let Error::TooLarge {
        size,
        kind: FitKind::Distance,
        face: Some(_),
        why: Some(RoomReason::BandsOverlap),
        ..
    } = err
    else {
        panic!("hole: expected TooLarge(BandsOverlap), got {err:?}");
    };
    assert!((size - 0.5).abs() <= 1e-9, "hole: fit {size}");

    // Hole covering the narrow end (`y = 2`) of a tapered land (`skew = 1.0`,
    // width `1 + (y - 2) / 9`): the lower end of the bisector is off-face, so
    // the contact moves to the on-face hole rim at `y = 2.5` (`width = 1 + 0.5 / 9`).
    let (base_taper, _) = skewed_land_part(1.0, 0.0, false);
    let end_hole = make_cylinder([54.5, 2.0, 5.0], 0.5, 14.0).unwrap();
    let holed_end = boolean(base_taper, end_hole, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let holed_edges = [
        edge_at(&holed_end, [55.5, 6.5, 18.0]),
        edge_at(&holed_end, [54.0, 6.5, 18.0]),
    ];
    let err = blend(
        &holed_end,
        Blend::Chamfer,
        &holed_edges,
        &[1.0],
        &[],
        Corners::Filled,
    )
    .unwrap_err();
    let Error::TooLarge {
        size,
        kind: FitKind::Distance,
        face: Some(_),
        why: Some(RoomReason::BandsOverlap),
        ..
    } = err
    else {
        panic!("end hole: expected TooLarge(BandsOverlap), got {err:?}");
    };
    // At y ~ 2.5 the land is ~1.0556 wide, so the on-face bisector fit is ~0.526 mm
    // (strictly above the y = 2 cap fit 0.4985 mm, and well below 1.0 mm).
    assert!(size > 0.52 && size < 0.54, "end hole: fit {size}");
}

/// SolidCraft #21749: a ring (outer radius 13, inner 10, z 0 to 10) joined to a tab, the box
/// x -13..-10, y -30..0, z 0..10. The tab's outer face x = -13 runs tangentially into the
/// ring's outer cylinder at (-13, 0); its inner face x = -10 meets the cylinder at an angle at
/// (-10, -y0), y0 = sqrt(13^2 - 10^2). The cylinder's seam at +x splits the ring's outer top edge
/// in two arcs. The part has 8 faces (top, bottom, bore, the outer cylinder's two halves and the
/// tab's three side faces), 18 edges and 12 vertices; with the tab's two long top edges, the
/// four edges of their tangent chain are those two and the two arcs. Returns the part and those
/// four edges: inner, outer, the arc through (0, -13) and the arc through (0, 13).
fn ring_with_tab() -> (Body, [usize; 4]) {
    all_fixes();
    let part = built(|k| {
        let outer = create_cylinder(k, Vec3::new(0.0, 0.0, 0.0), 13.0, 10.0)?;
        let inner = create_cylinder(k, Vec3::new(0.0, 0.0, -1.0), 10.0, 12.0)?;
        let ring = boolean_difference(k, outer, inner)?;
        let tab = create_box(k, Vec3::new(-13.0, -30.0, 0.0), 3.0, 30.0, 10.0)?;
        boolean_union(k, ring, tab)
    });
    let edges = [
        [-10.0, -20.0, 10.0],
        [-13.0, -20.0, 10.0],
        [0.0, -13.0, 10.0],
        [0.0, 13.0, 10.0],
    ]
    .map(|p| edge_at(&part, p));
    (part, edges)
}

/// Rounding the four edges of [`ring_with_tab`]'s tangent chain with mitred corners. The inner
/// edge's round and the near arc's meet at the corner (-10, -y0, 10), where the tab's inner face
/// meets the cylinder in a concave side edge: past that corner both rails run on to their mitre,
/// and the near arc's rail runs on over the tab. The band check took that arc's band on past the
/// corner as a strip as wide as its section, across the tab's top, and found the outer edge's
/// rail (x = -13 + r) inside it from r = 1.3 on, refusing the round as larger than its faces
/// allow. Past an end where another rounded edge meets it the band ends at their mitre, so the
/// rounds build: a valid closed body of 12 faces, 27 edges and 17 vertices. Each round adds a
/// face (two cylinders on the straight edges, as cones, and two surfaces of revolution on the
/// arcs), and replaces its edge by two rails (+4 edges); the ends on the face y = -30 add two
/// edges and four vertices for the five top vertices the rounds take; the two smooth joins of
/// the chain (at (-13, 0) and at the seam) a section edge and two vertices each; the corner a
/// mitre edge from the rails' crossing on the top face down to the side edge, two vertices.
///
/// The volumes are SolidCraft 0.45.0's for this design, as the report gives them (2844.9272 at
/// 1.4 and 2841.1111 at 1.45), which matched the original part. A check from closed forms: the
/// part less the round's section `a r^2` (`a = 1 - pi/4`) swept along the straight edges
/// (30 and 30 - y0 long) and, by Pappus, round the arcs (the 2 pi - acos(10/13) the ring keeps,
/// at the section centroid's radius `13 - r + r / (6 a)`) leaves out what the mitre's run-on
/// takes past the corner, so the body is smaller than that.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed forms, not ops computing geometry
fn rounds_meeting_a_tangent_chain_at_a_tab_corner_build() {
    let (part, edges) = ring_with_tab();
    assert_eq!(part.counts().unwrap(), [8, 18, 12]);
    // 0.45.0's body before the round
    assert!((volume(&part) - 2897.31).abs() < 1e-4, "{}", volume(&part));
    let y0 = (13.0_f64.powi(2) - 10.0_f64.powi(2)).sqrt();
    let a = 1.0 - std::f64::consts::FRAC_PI_4;
    for (r, want) in [(1.4, 2844.9272), (1.45, 2841.1111)] {
        let c = part.copy().unwrap();
        let made = blend(&c, Blend::Round, &edges, &[r], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("round {r}: {e:?}"));
        assert_eq!(made.dropped, [false; 4], "round {r}");
        assert!(c.check(CheckLevel::All).unwrap().is_valid(), "round {r}");
        assert_eq!(c.solid_count().unwrap(), 1, "round {r}");
        assert_eq!(c.counts().unwrap(), [12, 27, 17], "round {r}");
        let (_, read) = kernel_copy(&c);
        let mut kinds: Vec<&str> = made
            .made
            .iter()
            .map(|&f| {
                let face = read.face_list()[f];
                surface_kind_name(read.face(face).unwrap().surface.as_ref().unwrap())
            })
            .collect();
        kinds.sort_unstable();
        assert_eq!(
            kinds,
            [
                "cone",
                "cone",
                "surface-of-revolution",
                "surface-of-revolution"
            ],
            "round {r}"
        );
        let v = volume(&c);
        assert!((v - want).abs() < 1e-4, "round {r}: {v}, 0.45.0 {want}");
        let section = a * r * r;
        let kept_turn = 2.0 * std::f64::consts::PI - (10.0_f64 / 13.0).acos();
        let swept = section * (30.0 + 30.0 - y0) + section * kept_turn * (13.0 - r + r / (6.0 * a));
        assert!(v < volume(&part) - swept, "round {r}: {v}");
    }
}

/// What the band check still refuses on [`ring_with_tab`]: past the corner the inner edge's
/// and the near arc's rounds take off only the wedge between their rails (from the corner up to
/// where the rails cross at x = -10 - r, R = 13 - r), and a round whose rail lies inside that
/// wedge overlaps them. A hole of radius 0.15 at (-10.8, -6) lies in it at r = 1.4 (0.6 in
/// from the inner rail x = -11.4, and at R = 12.37, 0.77 out from the arc's rail R = 11.6);
/// its top rim rounded at 0.1 with the chain is refused, and the part is left as it was. The rim
/// alone builds.
#[test]
fn rounds_overlapping_past_a_tab_corner_are_refused() {
    let refused = |body: &Body, edges: &[usize], sizes: &[f64]| {
        let c = body.copy().unwrap();
        let before = (c.counts().unwrap(), volume(&c));
        match blend(&c, Blend::Round, edges, sizes, &[], Corners::Filled) {
            Err(Error::NoRoom | Error::TooLarge { .. }) => {}
            other => panic!("{sizes:?}: {other:?}"),
        }
        assert_eq!(c.counts().unwrap(), before.0, "{sizes:?}");
        assert!(rel(volume(&c), before.1) < 1e-12, "{sizes:?}");
    };
    let holed = built(|k| {
        let outer = create_cylinder(k, Vec3::new(0.0, 0.0, 0.0), 13.0, 10.0)?;
        let inner = create_cylinder(k, Vec3::new(0.0, 0.0, -1.0), 10.0, 12.0)?;
        let ring = boolean_difference(k, outer, inner)?;
        let tab = create_box(k, Vec3::new(-13.0, -30.0, 0.0), 3.0, 30.0, 10.0)?;
        let joined = boolean_union(k, ring, tab)?;
        let hole = create_cylinder(k, Vec3::new(-10.8, -6.0, -1.0), 0.15, 12.0)?;
        boolean_difference(k, joined, hole)
    });
    let mut picked = [
        [-10.0, -20.0, 10.0],
        [-13.0, -20.0, 10.0],
        [0.0, -13.0, 10.0],
        [0.0, 13.0, 10.0],
    ]
    .map(|p| edge_at(&holed, p))
    .to_vec();
    picked.push(edge_at(&holed, [-10.65, -6.0, 10.0]));
    refused(&holed, &picked, &[1.4, 1.4, 1.4, 1.4, 0.1]);
    // the hole's rim alone builds
    let c = holed.copy().unwrap();
    blend(&c, Blend::Round, &picked[4..], &[0.1], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("rim alone: {e:?}"));
    assert!(c.check(CheckLevel::All).unwrap().is_valid());
}
