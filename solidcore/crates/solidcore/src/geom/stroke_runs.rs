//! Original: the runs a polyline is stroked in so that its sharp turns get round joins; no
//! implementation is ported from another product.
//!
//! A stroker that only mitres its joins draws a round-joined line by stroking each run between
//! two sharp turns on its own and putting a disc one line width across on every run end. A turn
//! is sharp when the cosine of its angle is below the caller's bound; where two points coincide
//! the turn has no angle and counts as sharp.

use crate::geom::vector2d::Vec2;

/// One run of a polyline: the points from one sharp turn or line end to the next, or a whole
/// loop that has no sharp turn.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// The run's points in order. A sharp turn is the last point of one run and the first of
    /// the next.
    pub points: Vec<[f64; 2]>,
    /// Whether the run is a loop without ends: its last point joins its first.
    pub closed: bool,
}

/// Whether the turn at `here`, coming from `prev` and leaving for `next`, is sharper than the
/// angle whose cosine is `smooth_turn_cos`.
fn sharp(prev: Vec2, here: Vec2, next: Vec2, smooth_turn_cos: f64) -> bool {
    let (into, out) = (here - prev, next - here);
    let scale = into.length() * out.length();
    !(scale > 0.0 && into.dot(out) >= smooth_turn_cos * scale)
}

/// Whether the last of `points` is closer than `min_step` to the first.
fn repeats_start(points: &[Vec2], min_step: f64) -> bool {
    match (points.first(), points.last()) {
        (Some(first), Some(last)) => last.distance(*first) < min_step,
        _ => false,
    }
}

/// The runs of the polyline through `points`, cut at every turn sharper than the angle whose
/// cosine is `smooth_turn_cos`.
///
/// A point closer than `min_step` to the one before it is left out, and so is the end of a
/// `closed` line that repeats its start. A `closed` line without a sharp turn is one closed run;
/// one with a sharp turn starts and ends at its first sharp turn. A single point is a run of one
/// point, and no points are no runs.
pub fn runs(points: &[[f64; 2]], closed: bool, min_step: f64, smooth_turn_cos: f64) -> Vec<Run> {
    let mut distinct: Vec<Vec2> = Vec::new();
    for point in points.iter().copied().map(Vec2::from_array) {
        let repeated = distinct
            .last()
            .is_some_and(|last| last.distance(point) < min_step);
        if !repeated {
            distinct.push(point);
        }
    }
    if closed {
        while distinct.len() > 1 && repeats_start(&distinct, min_step) {
            distinct.pop();
        }
    }
    let n = distinct.len();
    if closed && n > 2 {
        let before = distinct.iter().cycle().skip(n.saturating_sub(1));
        let after = distinct.iter().cycle().skip(1);
        let start = distinct
            .iter()
            .zip(before)
            .zip(after)
            .position(|((here, prev), next)| sharp(*prev, *here, *next, smooth_turn_cos));
        let Some(start) = start else {
            let points = distinct.into_iter().map(Vec2::to_array).collect();
            return vec![Run {
                points,
                closed: true,
            }];
        };
        // An open line from its first sharp turn round to that turn again.
        distinct = distinct
            .iter()
            .cycle()
            .skip(start)
            .take(n.saturating_add(1))
            .copied()
            .collect();
    }
    let Some(first) = distinct.first() else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    let mut run = vec![first.to_array()];
    let turns = distinct
        .iter()
        .zip(distinct.iter().skip(1))
        .zip(distinct.iter().skip(2));
    for ((prev, here), next) in turns {
        run.push(here.to_array());
        if sharp(*prev, *here, *next, smooth_turn_cos) {
            let points = std::mem::replace(&mut run, vec![here.to_array()]);
            runs.push(Run {
                points,
                closed: false,
            });
        }
    }
    if let Some(last) = distinct.last().filter(|_| distinct.len() > 1) {
        run.push(last.to_array());
    }
    runs.push(Run {
        points: run,
        closed: false,
    });
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 30 degrees: the turn a regular 12-gon makes at each corner.
    const COS_30: f64 = 0.866;

    fn ring(n: usize) -> Vec<[f64; 2]> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                [50.0 + 5.0 * a.cos(), 50.0 + 5.0 * a.sin()]
            })
            .collect()
    }

    #[test]
    fn an_open_line_is_cut_at_its_sharp_turns_only() {
        // A right angle at (10, 0), then a 10 degree bend at (10, 10).
        let bend = [
            10.0 + 10.0 * 10f64.to_radians().sin(),
            10.0 + 10.0 * 10f64.to_radians().cos(),
        ];
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], bend];
        assert_eq!(
            runs(&line, false, 1e-3, COS_30),
            [
                Run {
                    points: vec![[0.0, 0.0], [10.0, 0.0]],
                    closed: false
                },
                Run {
                    points: vec![[10.0, 0.0], [10.0, 10.0], bend],
                    closed: false
                },
            ]
        );
    }

    #[test]
    fn a_smooth_loop_is_one_closed_run_with_or_without_its_start_repeated() {
        let loop_ = ring(48);
        let expected = [Run {
            points: loop_.clone(),
            closed: true,
        }];
        assert_eq!(runs(&loop_, true, 1e-3, COS_30), expected);
        // A full circle flattened from 0 to a whole turn ends on its first point again.
        let mut repeated = loop_.clone();
        repeated.push(loop_[0]);
        assert_eq!(runs(&repeated, true, 1e-3, COS_30), expected);
        // Not closed, the same points are an open line with two ends.
        assert_eq!(
            runs(&repeated, false, 1e-3, COS_30),
            [Run {
                points: repeated.clone(),
                closed: false
            }]
        );
    }

    #[test]
    fn a_loop_with_corners_starts_and_ends_at_its_first_corner() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let sides = runs(&square, true, 1e-3, COS_30);
        let sides: Vec<_> = sides
            .iter()
            .map(|run| (run.points.clone(), run.closed))
            .collect();
        assert_eq!(
            sides,
            [
                (vec![[0.0, 0.0], [10.0, 0.0]], false),
                (vec![[10.0, 0.0], [10.0, 10.0]], false),
                (vec![[10.0, 10.0], [0.0, 10.0]], false),
                (vec![[0.0, 10.0], [0.0, 0.0]], false),
            ]
        );
        // A drop: an arc from 30 to 330 degrees and the point where its end tangents meet, the
        // only corner (a 60 degree turn). The one run goes from that point round to it again.
        let mut drop_: Vec<[f64; 2]> = (0..=40)
            .map(|i| {
                let a = (30.0 + 7.5 * f64::from(i)).to_radians();
                [5.0 * a.cos(), 5.0 * a.sin()]
            })
            .collect();
        let tip = [5.0 / 30f64.to_radians().cos(), 0.0];
        drop_.push(tip);
        let round = runs(&drop_, true, 1e-3, COS_30);
        assert_eq!(round.len(), 1);
        assert_eq!(round[0].points.len(), 43);
        assert_eq!(
            (round[0].points[0], round[0].points[42], round[0].closed),
            (tip, tip, false)
        );
        assert_eq!(round[0].points[1..42], drop_[..41]);
    }

    #[test]
    fn points_closer_than_the_step_are_one_point() {
        let line = [
            [0.0, 0.0],
            [0.0, 0.0004],
            [10.0, 0.0],
            [10.0, 0.0],
            [20.0, 0.0],
        ];
        assert_eq!(
            runs(&line, false, 1e-3, COS_30),
            [Run {
                points: vec![[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]],
                closed: false
            }]
        );
        let dot = [[3.0, 4.0], [3.0, 4.0]];
        assert_eq!(
            runs(&dot, false, 1e-3, COS_30),
            [Run {
                points: vec![[3.0, 4.0]],
                closed: false
            }]
        );
        assert_eq!(
            runs(&dot, true, 1e-3, COS_30),
            [Run {
                points: vec![[3.0, 4.0]],
                closed: false
            }]
        );
        assert!(runs(&[], true, 1e-3, COS_30).is_empty());
    }

    #[test]
    fn a_point_that_is_not_a_number_cuts_the_line_and_nothing_panics() {
        let line = [
            [0.0, 0.0],
            [10.0, 0.0],
            [f64::NAN, 0.0],
            [30.0, 0.0],
            [40.0, 0.0],
        ];
        for closed in [false, true] {
            let cut = runs(&line, closed, 1e-3, COS_30);
            assert!(cut.len() > 1, "{cut:?}");
            assert!(cut.iter().all(|run| !run.closed));
        }
        assert_eq!(
            runs(&[[0.0, 0.0], [1.0, 0.0]], false, f64::NAN, f64::NAN).len(),
            1
        );
    }
}
