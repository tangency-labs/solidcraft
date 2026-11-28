//! Pass layout: a single-V groove's weld shown as passes 1, 2, 3: one piece each, numbered root
//! first, every point of the weld in a pass, no pass in a part, the cap standing proud; one pass
//! holding the whole weld; the same passes in a turned frame; layouts refused.

use solidcore_record::Vec3;
use solidcraft_kernel::profiles::ProfileCurve;

use super::*;

const T: f64 = 12.0;
const GAP: f64 = 2.0;
const LEN: f64 = 40.0;

fn vol(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

/// The polygon `p` in the plane z = 0, extruded LEN along z.
fn prism(p: &[[f64; 2]]) -> Body {
    let curves: Vec<ProfileCurve> = (0..p.len())
        .map(|i| ProfileCurve::Line { start: [p[i][0], p[i][1], 0.0], end: [p[(i + 1) % p.len()][0], p[(i + 1) % p.len()][1], 0.0] })
        .collect();
    profiles::extrude_curves(&curves, [0.0, 0.0, 1.0], LEN, true).unwrap()
}

/// A single-V butt joint, t 12, 60 degrees, gap 2, along z: the two plates and the weld filling
/// the groove flush.
fn vee() -> (Body, Body, Body) {
    let b = 30f64.to_radians().tan() * T;
    let left = prism(&[[-30.0, -T], [-GAP / 2.0, -T], [-GAP / 2.0 - b, 0.0], [-30.0, 0.0]]);
    let right = prism(&[[30.0, -T], [30.0, 0.0], [GAP / 2.0 + b, 0.0], [GAP / 2.0, -T]]);
    let weld = prism(&[[-GAP / 2.0, -T], [GAP / 2.0, -T], [GAP / 2.0 + b, 0.0], [-GAP / 2.0 - b, 0.0]]);
    (left, right, weld)
}

fn frame() -> Plane {
    Plane::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)).unwrap()
}

/// What is left of `body` with every one of `cutters` taken away (0 for nothing).
fn left_over(body: &Body, cutters: &[&Body]) -> f64 {
    let mut rest = body.clone();
    for c in cutters {
        match solidcraft_kernel::boolean(&rest, c, BoolOp::Cut).unwrap() {
            Some(r) => rest = r,
            None => return 0.0,
        }
    }
    vol(&rest)
}

#[test]
fn a_v_groove_s_passes_fill_it_round_bead_on_round_bead() {
    let (left, right, weld) = vee();
    let passes = weld_passes(&weld, &frame(), &[&left, &right], &[1, 2, 3]).unwrap();
    assert_eq!(passes.iter().map(|p| (p.layer, p.number)).collect::<Vec<_>>(), vec![(0, 1), (1, 2), (1, 3), (2, 4), (2, 5), (2, 6)]);
    for p in &passes {
        assert_eq!(p.body.lumps().unwrap().len(), 1, "pass {}", p.number);
        // no pass reaches into a plate
        for part in [&left, &right] {
            let inside = solidcraft_kernel::boolean(&p.body, part, BoolOp::Intersect).unwrap().map_or(0.0, |b| vol(&b));
            assert!(inside <= 1e-9 * vol(&weld), "pass {} enters a plate by {inside}", p.number);
        }
    }
    // every point of the weld lies in a pass
    let bodies: Vec<&Body> = passes.iter().map(|p| &p.body).collect();
    assert!(left_over(&weld, &bodies) <= 1e-9 * vol(&weld), "the passes leave a gap in the weld");
    // the cap passes stand proud of the weld's face (y = 0)
    for p in &passes[3..] {
        assert!(p.body.exact_bounds().unwrap().max.y > 0.1, "cap pass {} is not proud", p.number);
    }
}

#[test]
fn one_pass_holds_the_whole_weld() {
    let (left, right, weld) = vee();
    let passes = weld_passes(&weld, &frame(), &[&left, &right], &[1]).unwrap();
    assert_eq!(passes.len(), 1);
    assert!(left_over(&weld, &[&passes[0].body]) <= 1e-9 * vol(&weld));
}

#[test]
fn the_passes_turn_with_the_frame() {
    let (left, right, weld) = vee();
    let flat: Vec<f64> = weld_passes(&weld, &frame(), &[&left, &right], &[1, 2]).unwrap().iter().map(|p| vol(&p.body)).collect();
    let turn = affine::rotation([5.0, -3.0, 2.0], [1.0, 2.0, 3.0], 0.7).unwrap();
    let moved = |b: &Body| transform_matrix(b, turn).unwrap();
    let f = frame();
    let at = |p: Vec3| {
        let q = affine::apply_point(&turn, [p.x, p.y, p.z]);
        Vec3::new(q[0], q[1], q[2])
    };
    let dir = |v: Vec3| {
        let (a, o) = (at(v), at(Vec3::new(0.0, 0.0, 0.0)));
        Vec3::new(a.x - o.x, a.y - o.y, a.z - o.z)
    };
    let turned_frame = Plane::new(at(f.origin), dir(f.x), dir(f.y)).unwrap();
    let turned = weld_passes(&moved(&weld), &turned_frame, &[&moved(&left), &moved(&right)], &[1, 2]).unwrap();
    for (a, b) in flat.iter().zip(&turned) {
        assert!((a - vol(&b.body)).abs() <= 1e-6 * a, "pass {}: {a} turned {}", b.number, vol(&b.body));
    }
}

#[test]
fn layouts_out_of_range_are_refused() {
    let (left, right, weld) = vee();
    for layout in [vec![], vec![0], vec![1; MAX_LAYERS + 1], vec![MAX_PASSES_PER_LAYER + 1]] {
        let got = weld_passes(&weld, &frame(), &[&left, &right], &layout);
        assert!(matches!(got, Err(PassRefusal::Invalid(_))), "{layout:?}");
    }
}
