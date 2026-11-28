//! A ruled strip along a chain of edges, for an application's law extension (Surface › Extend,
//! law mode): the kernel's checks of its angle, folding and closeness
//! ([`solidcore::surface::ruled_strip`]), and the sequence that finds how long a strip up to a
//! target is ([`up_to_target`]). Ops converts the arrays and runs the attempts the application
//! builds; the kernel computes.

use solidcore::surface::ruled_strip as kernel;
use solidcore::Vec3;

pub use solidcore::surface::ruled_strip::{ANGLE_LIMIT, MAX_HALVINGS};

fn vec(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// The angle from the surface's normal a ruled surface takes for a law extension at `angle` to
/// the surface (0: tangent), `None` past `±`[`ANGLE_LIMIT`] or not finite
/// ([`kernel::ruling_angle`]).
#[must_use]
pub fn ruling_angle(angle: f64) -> Option<f64> {
    kernel::ruling_angle(angle)
}

/// How near its rulings' ends the far edge of a law strip `length` long, on a body of `size`,
/// must pass ([`kernel::far_tolerance`]).
#[must_use]
pub fn far_tolerance(size: f64, length: f64) -> f64 {
    kernel::far_tolerance(size, length)
}

/// Whether a strip folds anywhere along it, `at(f)` giving its edge's and its far edge's
/// tangents at the parameter `f` from 0 to 1 ([`kernel::strip_folds`]). `None` where `at`
/// cannot evaluate one.
pub fn strip_folds(mut at: impl FnMut(f64) -> Option<([f64; 3], [f64; 3])>) -> Option<bool> {
    kernel::strip_folds(|f| at(f).map(|(a, b)| (vec(a), vec(b))))
}

/// Whether a far edge strays more than `tol` from its rulings' ends, `at(f)` giving the far
/// edge's point and the ruling's end at the parameter `f` from 0 to 1 ([`kernel::strays`]).
/// `None` where `at` cannot evaluate one. Measured at sample parameters only, not bounded
/// between them: what is guaranteed is in [`kernel::strays`].
pub fn strays(tol: f64, mut at: impl FnMut(f64) -> Option<([f64; 3], [f64; 3])>) -> Option<bool> {
    kernel::strays(tol, |f| at(f).map(|(a, b)| (vec(a), vec(b))))
}

/// Whether a straight strip's corners leave no more than `eps` of it along its edge's direction
/// ([`kernel::cut_away`]).
#[must_use]
pub fn cut_away(step: [f64; 3], direction: [f64; 3], eps: f64) -> bool {
    kernel::cut_away(vec(step), vec(direction), eps)
}

/// How many times a strip up to a target is halved while it folds ([`up_to_target`]).
pub const FOLD_HALVINGS: usize = 60;
/// How many times the length between the last strip that folded and the first that did not is
/// halved ([`up_to_target`]).
pub const FOLD_BISECTIONS: usize = 24;

/// What [`up_to_target`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum Reach<T> {
    /// The strip cut at the target.
    Cut(T),
    /// No strip that does not fold reaches the target, and none folds: the target is missed.
    Missed,
    /// The strip folds before it reaches the target (or so near the fold it can't be built).
    Folded,
}

/// The strip up to a target, found by building strips of several lengths: `attempt(length)`
/// builds the strip `length` long and cuts it at the target, `Ok(Some(cut))` where the target
/// cuts it, `Ok(None)` where the target misses it (wholly or in part), `Err(e)` where it fails,
/// `folds(&e)` telling a strip that folds over itself.
///
/// The first strip is `reach` long. While it folds it is halved (at most [`FOLD_HALVINGS`]
/// times); the first that does not fold is taken if the target cuts it. One that the target
/// misses short of a strip that folded is lengthened again by bisection between the two (at
/// most [`FOLD_BISECTIONS`] times; a strip that fails to build there is nearly folded, its far
/// edge shrinking to a point, and is taken as folding). So only a target the strip reaches before
/// it folds is accepted.
///
/// # Errors
/// The first error of `attempt` that is not a fold, from the halving.
pub fn up_to_target<T, E>(
    reach: f64,
    mut attempt: impl FnMut(f64) -> Result<Option<T>, E>,
    folds: impl Fn(&E) -> bool,
) -> Result<Reach<T>, E> {
    let mut length = reach;
    let mut fold_at = None;
    let mut found = None;
    for _ in 0..FOLD_HALVINGS {
        match attempt(length) {
            Ok(cut) => {
                found = Some(cut);
                break;
            }
            Err(e) if folds(&e) => {
                fold_at = Some(length);
                length = kernel::halved_length(length);
            }
            Err(e) => return Err(e),
        }
    }
    let Some(cut) = found else {
        return Ok(Reach::Folded);
    };
    Ok(match (cut, fold_at) {
        (Some(cut), _) => Reach::Cut(cut),
        (None, None) => Reach::Missed,
        (None, Some(mut hi)) => {
            let mut lo = length;
            for _ in 0..FOLD_BISECTIONS {
                let mid = kernel::length_between(lo, hi);
                match attempt(mid) {
                    Ok(Some(cut)) => return Ok(Reach::Cut(cut)),
                    Ok(None) => lo = mid,
                    Err(_) => hi = mid,
                }
            }
            Reach::Folded
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Fail {
        Fold,
        Other,
    }

    /// A strip that folds from `fold` long on, and a target `target` out along it: the cut is
    /// the target's distance.
    fn strip(fold: f64, target: f64) -> impl FnMut(f64) -> Result<Option<f64>, Fail> {
        move |l| {
            if l >= fold {
                Err(Fail::Fold)
            } else {
                Ok((l >= target).then_some(target))
            }
        }
    }

    fn folds(e: &Fail) -> bool {
        *e == Fail::Fold
    }

    /// The cylinder strip's arc leaning 60° in folds 10 / sin 60° = 11.547 out. From 2000 long,
    /// halved to 7.8125 (2000 / 2⁸), it reaches a target 4 out; a target 10 out (past 7.8125,
    /// short of the fold) is found by bisection toward 15.625; one 14 out (past the fold) is a
    /// fold. A strip that never folds and never reaches: missed. Always folding: a fold.
    #[test]
    fn a_target_short_of_the_fold_is_reached_and_one_past_it_folds() {
        let fold = 11.547_005_383_792_516; // 10 / sin 60° = 20 / sqrt 3 (ops computes no geometry: no sin here)
        assert_eq!(
            up_to_target(2000.0, strip(fold, 4.0), folds),
            Ok(Reach::Cut(4.0))
        );
        assert_eq!(
            up_to_target(2000.0, strip(fold, 10.0), folds),
            Ok(Reach::Cut(10.0))
        );
        assert_eq!(
            up_to_target(2000.0, strip(fold, 14.0), folds),
            Ok(Reach::Folded)
        );
        assert_eq!(
            up_to_target(2000.0, strip(f64::INFINITY, 5000.0), folds),
            Ok(Reach::Missed)
        );
        assert_eq!(
            up_to_target(2000.0, strip(0.0, 1.0), folds),
            Ok(Reach::<f64>::Folded)
        );
    }

    #[test]
    fn an_error_that_is_not_a_fold_is_passed_on() {
        let failing = |_: f64| -> Result<Option<f64>, Fail> { Err(Fail::Other) };
        assert_eq!(up_to_target(10.0, failing, folds), Err(Fail::Other));
    }

    #[test]
    fn the_arrays_reach_the_kernel() {
        assert_eq!(ruling_angle(0.0), Some(std::f64::consts::FRAC_PI_2));
        assert!((far_tolerance(10.0, 5.0) - 1.5e-4).abs() < 1e-18);
        let x = [1.0, 0.0, 0.0];
        assert_eq!(strip_folds(|_| Some((x, [-1.0, 0.0, 0.0]))), Some(true));
        assert_eq!(strip_folds(|_| Some((x, x))), Some(false));
        assert_eq!(
            strays(1e-3, |f| Some(([f, 0.0, 0.0], [f, 0.0, 0.0]))),
            Some(false)
        );
        assert!(cut_away([0.0, 1.0, 0.0], x, 1e-9));
    }
}
