//! The smallest ball holding a set of points (Welzl's algorithm): a measure of a point set's size
//! that does not move with the points, whatever rigid motion carries them.

use crate::Vec3;

/// A ball: every point within `radius` of `center`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub center: Vec3,
    pub radius: f64,
}

/// How much more than `radius` a point may lie from the centre and still count as inside: the
/// rounding of the centre's and radius' arithmetic, so a point the ball was made through is never
/// taken for one outside it.
const ROUNDING: f64 = 1e-12;

impl Ball {
    /// The smallest ball holding every finite point of `points` (points that are not finite are
    /// skipped), or `None` when there are none. Welzl's algorithm in its incremental form, the
    /// points taken in a fixed shuffled order: expected `O(n)`. The ball is unique, so the order
    /// only changes the last bits of its centre and radius.
    pub fn enclosing(points: &[Vec3]) -> Option<Ball> {
        let mut p: Vec<Vec3> = points.iter().copied().filter(|q| q.is_finite()).collect();
        shuffle(&mut p);
        let mut ball = Ball { center: *p.first()?, radius: 0.0 };
        // Each loop keeps the smallest ball holding the points before it with the points of the
        // loops outside it on its boundary (Welzl 1991): at most four are, in three dimensions.
        for i in 1..p.len() {
            if ball.holds(p[i]) {
                continue;
            }
            ball = Ball { center: p[i], radius: 0.0 };
            for j in 0..i {
                if ball.holds(p[j]) {
                    continue;
                }
                ball = through(&[p[i], p[j]]);
                for k in 0..j {
                    if ball.holds(p[k]) {
                        continue;
                    }
                    ball = through(&[p[i], p[j], p[k]]);
                    for l in 0..k {
                        if !ball.holds(p[l]) {
                            ball = through(&[p[i], p[j], p[k], p[l]]);
                        }
                    }
                }
            }
        }
        Some(ball)
    }

    pub fn diameter(&self) -> f64 {
        2.0 * self.radius
    }

    /// Whether `p` lies in the ball, to the rounding of its arithmetic.
    pub fn holds(&self, p: Vec3) -> bool {
        p.dist(self.center) <= self.radius + ROUNDING * (self.radius + self.center.len())
    }
}

/// The smallest ball with every one of `r` (two to four points) on its boundary; where they are
/// degenerate (three in a line, four in a plane, which exact arithmetic never asks for but
/// rounding can), the smallest ball through some of them that holds them all.
fn through(r: &[Vec3]) -> Ball {
    let exact = match r {
        [a, b] => Some((*a + *b) * 0.5),
        [o, b, c] => circumcentre3(*o, *b, *c),
        [o, b, c, d] => circumcentre4(*o, *b, *c, *d),
        _ => None,
    };
    match exact {
        Some(center) => with_center(center, r),
        None => smallest_holding(r),
    }
}

/// The ball about `center` reaching the farthest of `r`.
fn with_center(center: Vec3, r: &[Vec3]) -> Ball {
    Ball { center, radius: r.iter().map(|q| q.dist(center)).fold(0.0, f64::max) }
}

/// The centre of the circle through `o`, `b` and `c`, or `None` when they are (nearly) in a line.
fn circumcentre3(o: Vec3, b: Vec3, c: Vec3) -> Option<Vec3> {
    let (a, b) = (b - o, c - o);
    let n = a.cross(b);
    let n2 = n.len2();
    if !(n2 > 1e-14 * a.len2() * b.len2()) {
        return None;
    }
    Some(o + (b.cross(n) * a.len2() + n.cross(a) * b.len2()) / (2.0 * n2))
}

/// The centre of the sphere through `o`, `b`, `c` and `d`, or `None` when they are (nearly) in a
/// plane.
fn circumcentre4(o: Vec3, b: Vec3, c: Vec3, d: Vec3) -> Option<Vec3> {
    let (a, b, c) = (b - o, c - o, d - o);
    let det = a.dot(b.cross(c));
    if !(det.abs() > 1e-7 * a.len() * b.len() * c.len()) {
        return None;
    }
    Some(o + (b.cross(c) * a.len2() + c.cross(a) * b.len2() + a.cross(b) * c.len2()) / (2.0 * det))
}

/// The smallest of the balls through two or three of `r` that holds all of them; a ball about
/// their mean where none does (never, for two to four points).
fn smallest_holding(r: &[Vec3]) -> Ball {
    let mut best: Option<Ball> = None;
    let mut consider = |ball: Ball| {
        if r.iter().all(|q| ball.holds(*q)) && best.is_none_or(|b| ball.radius < b.radius) {
            best = Some(ball);
        }
    };
    for i in 0..r.len() {
        for j in i + 1..r.len() {
            consider(with_center((r[i] + r[j]) * 0.5, r));
            for k in j + 1..r.len() {
                if let Some(c) = circumcentre3(r[i], r[j], r[k]) {
                    consider(with_center(c, r));
                }
            }
        }
    }
    best.unwrap_or_else(|| {
        let mean = r.iter().fold(Vec3::ZERO, |s, q| s + *q) / (r.len().max(1) as f64);
        with_center(mean, r)
    })
}

/// A Fisher–Yates shuffle from a fixed seed (SplitMix64), so the same points give the same ball.
fn shuffle(p: &mut [Vec3]) {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for i in (1..p.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        p.swap(i, (z % (i as u64 + 1)) as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points from a fixed generator in the cube [-1, 1]³.
    fn cloud(n: usize, seed: u64) -> Vec<Vec3> {
        let mut s = seed;
        let mut next = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        (0..n).map(|_| Vec3::new(next(), next(), next())).collect()
    }

    /// The smallest ball by brute force: of the balls through two, three or four of the points,
    /// the smallest that holds them all.
    fn brute(p: &[Vec3]) -> f64 {
        let mut best = f64::INFINITY;
        let n = p.len();
        let mut try_ball = |b: Ball| {
            if p.iter().all(|q| q.dist(b.center) <= b.radius * (1.0 + 1e-9)) {
                best = best.min(b.radius);
            }
        };
        for i in 0..n {
            for j in i + 1..n {
                try_ball(through(&[p[i], p[j]]));
                for k in j + 1..n {
                    try_ball(through(&[p[i], p[j], p[k]]));
                    for l in k + 1..n {
                        try_ball(through(&[p[i], p[j], p[k], p[l]]));
                    }
                }
            }
        }
        best
    }

    #[test]
    fn the_smallest_ball_holds_every_point_and_matches_brute_force() {
        for seed in 1..40 {
            let p = cloud(3 + seed as usize % 9, seed);
            let ball = Ball::enclosing(&p).unwrap();
            assert!(p.iter().all(|q| ball.holds(*q)), "seed {seed}: a point outside");
            let want = brute(&p);
            assert!((ball.radius - want).abs() <= 1e-12 * want, "seed {seed}: {} against {want}", ball.radius);
        }
    }

    #[test]
    fn known_balls() {
        let cube: Vec<Vec3> = (0..8).map(|i| Vec3::new((i & 1) as f64, (i >> 1 & 1) as f64, (i >> 2 & 1) as f64)).collect();
        let b = Ball::enclosing(&cube).unwrap();
        assert!(b.center.dist(Vec3::new(0.5, 0.5, 0.5)) < 1e-15 && (b.radius - 3f64.sqrt() / 2.0).abs() < 1e-15);
        let s = 3f64.sqrt();
        let triangle = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(-0.5, s / 2.0, 0.0), Vec3::new(-0.5, -s / 2.0, 0.0)];
        assert!((Ball::enclosing(&triangle).unwrap().radius - 1.0).abs() < 1e-15);
        // An obtuse triangle's ball is its longest side's.
        let obtuse = [Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.1, 0.0)];
        assert!((Ball::enclosing(&obtuse).unwrap().radius - 1.0).abs() < 1e-15);
        let tetra = [Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, -1.0, -1.0), Vec3::new(-1.0, 1.0, -1.0), Vec3::new(-1.0, -1.0, 1.0)];
        assert!((Ball::enclosing(&tetra).unwrap().radius - s).abs() < 1e-15);
        // Points in a line and in a plane, repeated points, one point, none.
        let line: Vec<Vec3> = (0..10).map(|i| Vec3::new(i as f64, 2.0 * i as f64, 0.0)).collect();
        assert!((Ball::enclosing(&line).unwrap().diameter() - 9.0 * 5f64.sqrt()).abs() < 1e-12);
        let square: Vec<Vec3> = (0..4).flat_map(|i| [Vec3::new((i & 1) as f64, (i >> 1) as f64, 0.0); 3]).collect();
        assert!((Ball::enclosing(&square).unwrap().diameter() - 2f64.sqrt()).abs() < 1e-15);
        assert_eq!(Ball::enclosing(&[Vec3::X]), Some(Ball { center: Vec3::X, radius: 0.0 }));
        assert_eq!(Ball::enclosing(&[]), None);
        assert_eq!(Ball::enclosing(&[Vec3::new(f64::NAN, 0.0, 0.0)]), None);
    }

    /// The ball's size is the set's own: a rigid motion leaves it, and points crowding one end
    /// do not stretch it (twice the farthest point's distance from the centroid would be nearly
    /// twice the diameter here).
    #[test]
    fn the_size_does_not_move_with_the_points_or_their_crowding() {
        let p = cloud(200, 7);
        let d = Ball::enclosing(&p).unwrap().diameter();
        let (c, s) = (0.7f64.cos(), 0.7f64.sin());
        let moved: Vec<Vec3> = p.iter().map(|q| Vec3::new(c * q.x - s * q.y + 30.0, s * q.x + c * q.y - 12.0, q.z + 5.0)).collect();
        assert!((Ball::enclosing(&moved).unwrap().diameter() - d).abs() <= 1e-12 * d);
        let mut crowded = vec![Vec3::ZERO; 99];
        crowded.push(Vec3::new(1.0, 0.0, 0.0));
        assert!((Ball::enclosing(&crowded).unwrap().diameter() - 1.0).abs() < 1e-15);
    }
}
