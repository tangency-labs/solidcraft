use super::*;

fn ring(pts: &[(f64, f64)]) -> Vec<Vec2> {
    pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
}

/// A circle of `n` points, counter-clockwise, or clockwise for a hole.
fn circle(c: (f64, f64), r: f64, n: usize, hole: bool) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> =
        (0..n).map(|k| std::f64::consts::TAU * k as f64 / n as f64).map(|t| Vec2::new(c.0 + r * t.cos(), c.1 + r * t.sin())).collect();
    if hole {
        pts.reverse();
    }
    pts
}

fn point(loops: &[Vec<Vec2>], (l, i): (usize, usize)) -> Vec2 {
    loops[l][i]
}

/// Every part convex and counter-clockwise, every boundary point used, the region's area
/// covered exactly.
fn check(loops: &[Vec<Vec2>], parts: &[Vec<(usize, usize)>]) {
    let mut covered = 0.0;
    let mut used = HashSet::new();
    for part in parts {
        assert!(part.len() >= 3);
        let pts: Vec<Vec2> = part.iter().map(|&c| point(loops, c)).collect();
        let n = pts.len();
        for k in 0..n {
            let (a, v, b) = (pts[(k + n - 1) % n], pts[k], pts[(k + 1) % n]);
            assert!(turn(a, v, b) >= -1e-9 * a.dist(v) * v.dist(b), "part {part:?} turns right at {k}");
        }
        let a: f64 = (0..n).map(|k| pts[k].cross(pts[(k + 1) % n])).sum::<f64>() * 0.5;
        assert!(a > 0.0);
        covered += a;
        used.extend(part.iter().copied());
    }
    let region: f64 = loops.iter().map(|l| (0..l.len()).map(|k| l[k].cross(l[(k + 1) % l.len()])).sum::<f64>() * 0.5).sum();
    assert!((covered - region).abs() < 1e-9 * region.abs(), "{covered} vs {region}");
    assert_eq!(used.len(), loops.iter().map(Vec::len).sum::<usize>(), "every boundary point is a corner");
}

#[test]
fn a_convex_loop_is_one_part() {
    let loops = vec![circle((0.0, 0.0), 1.0, 12, false)];
    let parts = convex_parts(&loops).unwrap();
    check(&loops, &parts);
    assert_eq!(parts.len(), 1);
}

#[test]
fn a_square_ring_is_four_parts() {
    let loops = vec![ring(&[(-2.0, -2.0), (2.0, -2.0), (2.0, 2.0), (-2.0, 2.0)]), ring(&[(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)])];
    let parts = convex_parts(&loops).unwrap();
    check(&loops, &parts);
    assert!(parts.len() <= 5, "{parts:?}");
}

#[test]
fn a_plate_with_round_holes_is_a_few_parts() {
    let mut loops = vec![ring(&[(0.0, 0.0), (60.0, 0.0), (60.0, 30.0), (0.0, 30.0)])];
    loops.push(circle((15.0, 15.0), 5.0, 32, true));
    loops.push(circle((45.0, 15.0), 5.0, 32, true));
    let parts = convex_parts(&loops).unwrap();
    check(&loops, &parts);
    // No convex part holds two neighbouring rim sides (the region turns 191° at a rim point),
    // so about one part per rim side; fewer than the 70 triangles.
    assert!(parts.len() <= 64 + 4, "{} parts", parts.len());
}

#[test]
fn points_along_straight_sides_stay_corners() {
    // An L with three points on its long sides and a square hole.
    let outer = ring(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0), (0.0, 5.0)]);
    let hole = ring(&[(1.0, 1.0), (1.0, 3.0), (3.0, 3.0), (3.0, 1.0)]);
    let loops = vec![outer, hole];
    let parts = convex_parts(&loops).unwrap();
    check(&loops, &parts);
}

#[test]
fn a_hole_left_of_another_bridges_past_it() {
    // Holes in a row: the right one bridges to the outer loop, the left one to the right one or
    // past it.
    let mut loops = vec![ring(&[(0.0, 0.0), (40.0, 0.0), (40.0, 10.0), (0.0, 10.0)])];
    for x in [5.0, 15.0, 25.0, 35.0] {
        loops.push(circle((x, 5.0), 2.0, 8, true));
    }
    let parts = convex_parts(&loops).unwrap();
    check(&loops, &parts);
}

#[test]
fn loops_that_cannot_be_cut_are_refused() {
    let square = ring(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    // Wound the wrong way, a hole wound like the outer loop, a point that is not a number, too
    // few points, nothing at all, too many points.
    assert!(convex_parts(&[square.iter().rev().copied().collect()]).is_none());
    assert!(convex_parts(&[square.clone(), ring(&[(0.2, 0.2), (0.4, 0.2), (0.4, 0.4)])]).is_none());
    assert!(convex_parts(&[ring(&[(0.0, 0.0), (f64::NAN, 0.0), (1.0, 1.0)])]).is_none());
    assert!(convex_parts(&[ring(&[(0.0, 0.0), (1.0, 0.0)])]).is_none());
    assert!(convex_parts(&[]).is_none());
    assert!(convex_parts(&[circle((0.0, 0.0), 1.0, MAX_CONVEX_POINTS + 1, false)]).is_none());
    // All on one line.
    assert!(convex_parts(&[ring(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)])]).is_none());
}

#[test]
fn hostile_loops_never_panic() {
    let square = ring(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)]);
    let bow = ring(&[(0.0, 0.0), (4.0, 4.0), (4.0, 0.0), (0.0, 4.0)]);
    let cases: Vec<Vec<Vec<Vec2>>> = vec![
        vec![bow.clone()],
        vec![square.clone(), bow.iter().rev().copied().collect()],
        // A hole sticking out of the outer loop, a hole touching it, holes overlapping.
        vec![square.clone(), circle((4.0, 2.0), 1.0, 8, true)],
        vec![square.clone(), ring(&[(0.0, 0.0), (1.0, 2.0), (2.0, 1.0)]).iter().rev().copied().collect()],
        vec![square.clone(), circle((2.0, 2.0), 1.0, 8, true), circle((2.5, 2.0), 1.0, 8, true)],
        // Repeated points, huge and tiny coordinates.
        vec![ring(&[(0.0, 0.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (1.0, 1.0)])],
        vec![ring(&[(0.0, 0.0), (1e300, 0.0), (1e300, 1e300), (0.0, 1e300)])],
        vec![ring(&[(0.0, 0.0), (1e-300, 0.0), (1e-300, 1e-300), (0.0, 1e-300)])],
    ];
    for loops in cases {
        if let Some(parts) = convex_parts(&loops) {
            for part in &parts {
                assert!(part.iter().all(|&(l, i)| l < loops.len() && i < loops[l].len()));
            }
        }
    }
}
