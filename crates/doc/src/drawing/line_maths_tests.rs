use crate::drawing::measure::LinearDirection;
use solidcore_record::Vec2;

use super::*;

const TOL: f64 = 1e-6;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-9 * (1.0 + y.abs()))
}

#[test]
fn a_line_runs_right_or_up_whichever_way_its_points_go() {
    let right = LineCoords::forward(LinearDirection::Aligned, v(-3.0, 0.0)).unwrap();
    assert_eq!((right.unit, right.normal), (v(1.0, 0.0), v(0.0, 1.0)));
    let up = LineCoords::forward(LinearDirection::Aligned, v(0.0, -2.0)).unwrap();
    assert_eq!((up.unit, up.normal), (v(0.0, 1.0), v(-1.0, 0.0)));
    let slant = LineCoords::forward(LinearDirection::Along(v(-3.0, -4.0)), v(1.0, 1.0)).unwrap();
    assert!((slant.unit.x - 0.6).abs() < 1e-15 && (slant.unit.y - 0.8).abs() < 1e-15);
    assert_eq!(LineCoords::forward(LinearDirection::Horizontal, v(0.0, 5.0)).unwrap().unit, v(1.0, 0.0));
    assert_eq!(LineCoords::forward(LinearDirection::Vertical, v(5.0, 0.0)).unwrap().unit, v(0.0, 1.0));
}

#[test]
fn a_line_needs_a_direction() {
    assert!(LineCoords::forward(LinearDirection::Aligned, v(0.0, 0.0)).is_none());
    assert!(LineCoords::forward(LinearDirection::Along(v(f64::NAN, 1.0)), v(1.0, 0.0)).is_none());
}

#[test]
fn points_read_along_and_across_a_line() {
    let line = LineCoords::forward(LinearDirection::Along(v(3.0, 4.0)), v(1.0, 0.0)).unwrap();
    let p = v(3.0, 4.0) * 2.0 + v(-4.0, 3.0) * 0.5;
    assert!((line.along(p) - 10.0).abs() < 1e-12);
    assert!((line.across(p) - 2.5).abs() < 1e-12);
}

#[test]
fn the_reach_is_the_farthest_point_on_the_side_asked() {
    let line = LineCoords::forward(LinearDirection::Horizontal, v(1.0, 0.0)).unwrap();
    let points = [v(0.0, 2.0), v(5.0, -3.0), v(9.0, 1.0)];
    assert_eq!(line.reach(&points, 1.0), 2.0);
    assert_eq!(line.reach(&points, -1.0), -3.0);
    assert_eq!(line.reach(&[v(4.0, 7.0)], -1.0), 7.0);
}

#[test]
fn a_line_on_a_longer_one_is_covered_within_its_ends() {
    let by = [v(0.0, 0.0), v(10.0, 0.0)];
    assert!(covers(&[v(2.0, 0.0), v(8.0, 0.0)], &by, TOL));
    assert!(covers(&[v(10.0, 0.0), v(0.0, 0.0)], &by, TOL));
    assert!(covers(&[v(-0.9 * TOL, 0.0), v(10.0 + 0.9 * TOL, 0.9 * TOL)], &by, TOL));
    assert!(!covers(&[v(-1.1 * TOL, 0.0), v(5.0, 0.0)], &by, TOL));
    assert!(!covers(&[v(2.0, 1.1 * TOL), v(8.0, 0.0)], &by, TOL));
    assert!(!covers(&[v(5.0, -1.0), v(5.0, 1.0)], &by, TOL), "across it");
    assert!(!covers(&by, &[v(2.0, 0.0), v(8.0, 0.0)], TOL));
}

#[test]
fn other_lines_are_covered_only_by_the_same_line() {
    let arc = [v(0.0, 0.0), v(1.0, 1.0), v(2.0, 0.0)];
    assert!(covers(&arc, &arc, TOL));
    assert!(!covers(&arc, &[v(0.0, 0.0), v(1.0, 1.0)], TOL));
    let dot = [v(3.0, 3.0), v(3.0, 3.0)];
    assert!(covers(&dot, &dot, TOL));
    assert!(!covers(&[v(4.0, 3.0), v(4.0, 3.0)], &dot, TOL), "two points apart");
    assert!(!covers(&[v(3.0, 3.0), v(4.0, 3.0)], &dot, TOL));
    assert!(covers(&dot, &[v(3.0, 2.0), v(3.0, 4.0)], TOL));
}

/// Texts at least `pitch` apart, moved as little as can be: those with room stay, those that would
/// overlap move apart, centred on their ideal places, whatever their number.
#[test]
fn spread_moves_only_what_would_overlap() {
    let pitch = 4.5;
    let spread = |ideal: &[f64]| spread_along(ideal, pitch).unwrap();
    assert_eq!(spread(&[7.0]), [7.0]);
    assert_eq!(spread(&[0.0, 10.0, 30.0]), [0.0, 10.0, 30.0], "room for all");
    assert_eq!(spread(&[0.0, 1.0]), [-1.75, 2.75], "a pair, about its middle");
    assert_eq!(spread(&[5.0, 5.0, 5.0]), [0.5, 5.0, 9.5], "three at one place");
    // A block that grows into a neighbour takes it in: 0, 1 and 4 pool round 5/3.
    let expected = [5.0 / 3.0 - pitch, 5.0 / 3.0, 5.0 / 3.0 + pitch, 20.0];
    assert!(close(&spread(&[0.0, 1.0, 4.0, 20.0]), &expected));
    let all = spread(&[3.0; MAX_ITEMS]);
    let n = MAX_ITEMS as f64;
    assert!(close(&[all.iter().sum::<f64>() / n], &[3.0]) && all.windows(2).all(|w| close(&[w[1] - w[0]], &[pitch])), "{all:?}");
}

/// Any places, in order, at any spacing, many at one place among them: as many places come back, at
/// least the pitch apart, finite, each run of touching texts centred on its members' ideal places.
#[test]
fn spread_fuzz_keeps_its_rules() {
    let mut state = 0x1305_2c0d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..4000 {
        let count = match round % 4 {
            0 => 0,
            1 => 1,
            2 => MAX_ITEMS,
            _ => (next() % 12) as usize,
        };
        let pitch = [0.1, 1.0, 4.5, 30.0][(next() % 4) as usize];
        let spacing = [0.0, 0.01, 1.0, 5.0, 50.0][(next() % 5) as usize];
        let mut ideal: Vec<f64> = (0..count).map(|_| (next() % 1000) as f64 * spacing * 0.01 - 3.0).collect();
        ideal.sort_by(f64::total_cmp);
        let got = spread_along(&ideal, pitch).unwrap();
        assert_eq!(got.len(), ideal.len());
        assert!(got.iter().all(|x| x.is_finite()), "{got:?}");
        assert!(got.windows(2).all(|w| w[1] - w[0] >= pitch - 1e-9), "{ideal:?} -> {got:?}");
        // Runs of texts that touch, each centred on its members' ideal places.
        let mut from = 0;
        for i in 1..=got.len() {
            if i == got.len() || got[i] - got[i - 1] > pitch + 1e-9 {
                let (a, b) = (&ideal[from..i], &got[from..i]);
                let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
                assert!((mean(a) - mean(b)).abs() < 1e-6, "{a:?} -> {b:?}");
                from = i;
            }
        }
    }
}

#[test]
fn what_cannot_be_spaced_is_refused() {
    assert!(spread_along(&[1.0, 0.0], 1.0).is_err());
    assert!(spread_along(&[0.0, 1.0], 0.0).is_err());
    assert!(spread_along(&[0.0, f64::NAN], 1.0).is_err());
    assert!(spread_along(&[0.0, 1.0], f64::INFINITY).is_err());
    assert!(spread_along(&[f64::MAX, f64::MAX], 1.0).is_err());
    let too_many = vec![0.0; MAX_ITEMS + 1];
    let message = spread_along(&too_many, 1.0).unwrap_err().to_string();
    assert!(message.contains(&MAX_ITEMS.to_string()), "{message}");
    assert!(spread_along(&vec![0.0; MAX_ITEMS], 1.0).is_ok());
    assert_eq!(spread_along(&[], 1.0).unwrap(), Vec::<f64>::new());
}

/// For random ordered places, the result is the least-squares fit under the order: the places less
/// their spacing are constant on blocks, rise from block to block, each block sits at the mean of its
/// members' places less their spacing, and every prefix of (wanted - got) sums to 0 or more.
#[test]
fn random_places_get_the_least_squares_fit() {
    let mut seed = 0x0001_3052_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..500 {
        let n = 1 + (next() * 40.0) as usize;
        let pitch = 0.5 + next() * 5.0;
        let mut ideal: Vec<f64> = (0..n).map(|_| next() * 60.0 - 30.0).collect();
        ideal.sort_by(f64::total_cmp);
        let got = spread_along(&ideal, pitch).unwrap();
        let less = |xs: &[f64]| xs.iter().enumerate().map(|(i, x)| x - pitch * i as f64).collect::<Vec<_>>();
        let (want, have) = (less(&ideal), less(&got));
        for pair in got.windows(2) {
            assert!(pair[1] - pair[0] >= pitch - 1e-9, "{pair:?} closer than {pitch}");
        }
        for pair in have.windows(2) {
            assert!(pair[1] >= pair[0] - 1e-9, "blocks fall: {pair:?}");
        }
        let mut start = 0;
        while start < n {
            let end = (start..n).find(|&j| (have[j] - have[start]).abs() > 1e-9).unwrap_or(n);
            let mean = want[start..end].iter().sum::<f64>() / (end - start) as f64;
            assert!((have[start] - mean).abs() < 1e-9, "block {start}..{end} not at its mean");
            start = end;
        }
        let mut sum = 0.0;
        for (w, h) in want.iter().zip(&have) {
            sum += w - h;
            assert!(sum >= -1e-9, "a prefix sums below 0");
        }
    }
}
