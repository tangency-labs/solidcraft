//! Stretch: a body made longer or shorter along one direction, its details kept their size
//! ([`solidcore::stretch`] holds the geometry and the module note says how it works).
//!
//! The recipe: the kernel works the stretch out from the body ([`Stretch::new`]); a lengthening
//! finds the deepest band behind the cut that is a prism along the direction ([`band`]), then
//! for each pass cuts the body at the band's far side and at the cut plane ([`split_body`]),
//! scales the band and moves the part ahead ([`Body::transform`]) and unions the pieces
//! ([`union_all`]). A shrink checks the slab in front of the cut, cuts it out, moves the part
//! ahead back and unions; refused, it searches for the longest shrink that builds. Every plane,
//! depth and map comes from the kernel: this module only sequences.

use solidcore::stretch::{self as kernel, CutPlane, Stretch};
use solidcore_record::brep::Surface as RecordSurface;

pub use solidcore::stretch::{Crossing, Lies, Refusal, SketchPlane};

use crate::boolean::union_all;
use crate::session::{read_body, Answer};
use crate::split::{split_body, Tool};
use crate::{Body, Error, Result};

/// Why a stretch did not build.
#[derive(Clone, Debug, PartialEq)]
pub enum Refused {
    /// The kernel's refusal: a bad argument, a plane that misses, a face crossing the band, or a
    /// shrink deeper than the body in front of the plane.
    Stretch(Refusal),
    /// A shrink whose slab is no prism: why, and the longest shrink found that builds.
    Shrink {
        /// What crosses the slab.
        why: Refusal,
        /// The longest shrink that builds at this plane.
        most: f64,
    },
}

/// The kernel's working-out of a stretch, as an answer of the session.
struct Plan(std::result::Result<Stretch, Refusal>);
impl Answer for Plan {}

/// Whether a band is a prism, as an answer of the session.
struct Verdict(std::result::Result<(), Refusal>);
impl Answer for Verdict {}

/// `body` stretched by `distance` along `normal` at the plane through `at`: the part on the
/// normal's side of the plane moves by the distance (a negative distance shrinks it), the rest
/// stays where it is. `body` is unchanged.
///
/// # Errors
/// The session's and the kernel's, from the cuts, maps and unions: [`Error::Empty`] when a cut
/// leaves no part where the kernel's plan has one.
pub fn stretch(
    body: &Body,
    at: [f64; 3],
    normal: [f64; 3],
    distance: f64,
) -> Result<std::result::Result<Body, Refused>> {
    let Plan(plan) = read_body("stretch", body, |k, b| {
        Ok(Plan(Stretch::new(k, b, at, normal, distance)?))
    })?;
    let s = match plan {
        Ok(s) => s,
        Err(why) => return Ok(Err(Refused::Stretch(why))),
    };
    if s.is_nothing() {
        return Ok(Ok(body.copy()?));
    }
    if s.grows() {
        lengthen(body, &s)
    } else {
        shrink(body, &s)
    }
}

/// The lengthening: the deepest band behind the cut that is a prism, then its passes.
fn lengthen(body: &Body, s: &Stretch) -> Result<std::result::Result<Body, Refused>> {
    let cut = s.cut();
    let mut found = None;
    let mut why = Refusal::Misses;
    for (depth, base) in s.band_depths() {
        match prism(body, s, &base, &cut)? {
            Ok(()) => {
                found = Some(depth);
                break;
            }
            Err(w) => why = w,
        }
    }
    let Some(depth) = found else {
        return Ok(Err(Refused::Stretch(why)));
    };
    let mut current = body.copy()?;
    for pass in s.passes(depth) {
        let band = slab(&current, &pass.base, &pass.cut)?.ok_or(Error::Empty)?;
        band.transform(&pass.scale)?;
        let behind = halves(&current, &pass.base)?.map(|(behind, _)| behind);
        let ahead = halves(&current, &pass.cut)?
            .map(|(_, ahead)| ahead)
            .ok_or(Error::Empty)?;
        ahead.transform(&pass.shift)?;
        current = union_all(behind.into_iter().chain([band, ahead]).collect())?;
    }
    Ok(Ok(current))
}

/// The shrink: the slab in front of the cut taken out, the part ahead moved back.
fn shrink(body: &Body, s: &Stretch) -> Result<std::result::Result<Body, Refused>> {
    let plan = match s.shrink() {
        Ok(plan) => plan,
        Err(why) => return Ok(Err(Refused::Stretch(why))),
    };
    if let Err(why) = prism(body, s, &plan.cut, &plan.far)? {
        let mut search = s.shrink_search();
        while let Some((_, far)) = search.next(s) {
            let fits = prism(body, s, &plan.cut, &far)?.is_ok();
            search.found(fits);
        }
        return Ok(Err(Refused::Shrink {
            why,
            most: search.most(),
        }));
    }
    let behind = halves(body, &plan.cut)?
        .map(|(behind, _)| behind)
        .ok_or(Error::Empty)?;
    let ahead = halves(body, &plan.far)?
        .map(|(_, ahead)| ahead)
        .ok_or(Error::Empty)?;
    ahead.transform(&plan.shift)?;
    Ok(Ok(union_all(vec![behind, ahead])?))
}

/// Whether the slab of `body` from `from` to `to` is a prism along the stretch's direction.
fn prism(
    body: &Body,
    s: &Stretch,
    from: &CutPlane,
    to: &CutPlane,
) -> Result<std::result::Result<(), Refusal>> {
    let Some(piece) = slab(body, from, to)? else {
        return Ok(Err(Refusal::Misses));
    };
    let Verdict(v) = read_body("stretch band", &piece, |_, b| {
        Ok(Verdict(kernel::band(
            b,
            s.direction(),
            from.level,
            to.level,
            s.eps(),
        )?))
    })?;
    Ok(v)
}

/// The part of `body` between the planes `from` and `to`, `None` when the planes leave none.
fn slab(body: &Body, from: &CutPlane, to: &CutPlane) -> Result<Option<Body>> {
    let Some((_, ahead)) = halves(body, from)? else {
        return Ok(None);
    };
    Ok(halves(&ahead, to)?.map(|(behind, _)| behind))
}

/// `body` cut by `plane`: the part behind it (the side that stays) and the part ahead.
fn halves(body: &Body, plane: &CutPlane) -> Result<Option<(Body, Body)>> {
    split_body(
        body,
        &Tool {
            surface: RecordSurface::Plane {
                origin: plane.origin,
                u_axis: plane.u,
                v_axis: plane.v,
            },
            domain: plane.domain,
        },
    )
}

/// Where a stretch of `body` along `normal` is best cut when no place is given, as the distance
/// from the picked face point `from`, else from its least level along the normal
/// ([`solidcore::stretch::proposed_cut`]): the middle of the longest run where the body is a
/// prism along it. `None` for a direction with no length.
///
/// # Errors
/// Those of the session and the kernel's, reading the faces.
pub fn proposed_cut(body: &Body, normal: [f64; 3], from: Option<[f64; 3]>) -> Result<Option<f64>> {
    read_body("stretch proposed_cut", body, |k, b| {
        kernel::proposed_cut(k, b, normal, from)
    })
}

/// How a sketch plane with normal `plane_normal` lies to a stretch along `normal`
/// ([`solidcore::stretch::plane_lies`]).
#[must_use]
pub fn plane_lies(plane_normal: [f64; 3], normal: [f64; 3]) -> Lies {
    kernel::plane_lies(plane_normal, normal)
}

/// Whether two bodies, each as its box and volume, are the same part to a millionth of its size
/// ([`solidcore::stretch::same_part`]).
#[must_use]
pub fn same_part(a: ([f64; 3], [f64; 3], f64), b: ([f64; 3], [f64; 3], f64)) -> bool {
    kernel::same_part(a, b)
}

/// A point on the cut plane of a stretch along `normal` ([`solidcore::stretch::cut_point`]):
/// `at` along the normal from the picked face point `from`, or from `low`, the body's least
/// level along the normal. `None` for a direction with no length.
#[must_use]
pub fn cut_point(normal: [f64; 3], from: Option<[f64; 3]>, low: f64, at: f64) -> Option<[f64; 3]> {
    kernel::cut_point(normal, from, low, at)
}

/// Where a straight feature on a stretched body goes, its axis through `point` along `axis`
/// used from `start` to `end` ([`solidcore::stretch::moved_axis`]).
#[must_use]
pub fn moved_axis(
    point: [f64; 3],
    axis: [f64; 3],
    start: f64,
    end: f64,
    at: [f64; 3],
    normal: [f64; 3],
    distance: f64,
) -> ([f64; 3], f64, f64) {
    kernel::moved_axis(point, axis, start, end, at, normal, distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: [f64; 3] = [1.0, 0.0, 0.0];

    fn size(b: &Body) -> [f64; 3] {
        let (lo, hi) = b.bounds().unwrap();
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    }

    #[test]
    fn a_block_stretched_along_x_is_longer_and_its_left_side_stays() {
        let block = crate::body::make_box([0.0; 3], [40.0, 20.0, 10.0]).unwrap();
        let out = stretch(&block, [20.0, 0.0, 0.0], X, 15.0).unwrap().unwrap();
        let (lo, _) = out.bounds().unwrap();
        assert!((size(&out)[0] - 55.0).abs() < 1e-6 && lo[0].abs() < 1e-6);
        assert!((out.volume(1e-9).unwrap() - 55.0 * 20.0 * 10.0).abs() < 1e-6);
        // A long stretch takes several passes and is still one solid of the right size.
        let out = stretch(&block, [20.0, 0.0, 0.0], X, 500.0)
            .unwrap()
            .unwrap();
        assert!((size(&out)[0] - 540.0).abs() < 1e-6);
        assert!((out.volume(1e-9).unwrap() - 540.0 * 20.0 * 10.0).abs() < 1e-5);
    }

    #[test]
    fn a_block_shrunk_loses_a_slice_and_too_deep_a_shrink_is_refused() {
        let block = crate::body::make_box([0.0; 3], [40.0, 20.0, 10.0]).unwrap();
        let out = stretch(&block, [20.0, 0.0, 0.0], X, -5.0).unwrap().unwrap();
        assert!((size(&out)[0] - 35.0).abs() < 1e-6);
        assert_eq!(
            stretch(&block, [20.0, 0.0, 0.0], X, -20.0).unwrap().err(),
            Some(Refused::Stretch(Refusal::TooDeep {
                shrink: 20.0,
                room: 20.0
            }))
        );
    }

    #[test]
    fn the_proposed_cut_is_the_middle_of_the_longest_plain_run() {
        // A 40 mm block with a hole down through it at x = 10 (7 to 13): the plain runs along x
        // are 0 to 7 and 13 to 40; the longest's middle is at 26.5.
        let block = crate::body::make_box([0.0; 3], [40.0, 20.0, 10.0]).unwrap();
        let hole = crate::body::make_cylinder([10.0, 10.0, -5.0], 3.0, 20.0).unwrap();
        let holed = crate::boolean::boolean(block, hole, crate::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let at = proposed_cut(&holed, X, None).unwrap().unwrap();
        assert!((at - 26.5).abs() < 1e-6, "{at}");
        // From a face at x = 40 the same cut is 13.5 behind it.
        let at = proposed_cut(&holed, X, Some([40.0, 3.0, 3.0]))
            .unwrap()
            .unwrap();
        assert!((at + 13.5).abs() < 1e-6, "{at}");
        // Along z the hole runs with the direction: the whole height is one run.
        let at = proposed_cut(&holed, [0.0, 0.0, 1.0], None)
            .unwrap()
            .unwrap();
        assert!((at - 5.0).abs() < 1e-6, "{at}");
    }

    #[test]
    fn sketch_points_are_sided_and_moved_in_their_plane() {
        // The XZ plane: x along world x, y along world z.
        let plane = SketchPlane {
            origin: [0.0, 5.0, 0.0],
            x_axis: X,
            y_axis: [0.0, 0.0, 1.0],
        };
        assert!(plane.past_cut([30.0, 2.0], [20.0, 0.0, 0.0], X) > 0.0);
        assert!(plane.past_cut([10.0, 2.0], [20.0, 0.0, 0.0], X) < 0.0);
        assert_eq!(plane.moved([30.0, 2.0], X, 15.0), [45.0, 2.0]);
        assert_eq!(
            plane.moved([30.0, 2.0], [0.0, 0.0, -1.0], 4.0),
            [30.0, -2.0]
        );
        assert_eq!(plane.runs([0.0, 0.0], [10.0, 0.0], X), Some(true));
        assert_eq!(plane.runs([0.0, 0.0], [0.0, 10.0], X), Some(false));
        assert_eq!(plane.runs([0.0, 0.0], [10.0, 10.0], X), None);
        assert_eq!(plane_lies([0.0, -1.0, 0.0], X), Lies::InPlane);
        assert_eq!(
            plane_lies([0.0, -1.0, 0.0], [0.0, 1.0, 0.0]),
            Lies::Along(false)
        );
        assert!(same_part(
            ([0.0; 3], [10.0; 3], 1000.0),
            ([0.0; 3], [10.0; 3], 1000.0)
        ));
        assert!(!same_part(
            ([0.0; 3], [10.0; 3], 1000.0),
            ([0.0; 3], [11.0; 3], 1000.0)
        ));
    }

    #[test]
    fn a_cut_through_a_sideways_hole_is_refused_and_names_it() {
        // A block with a round hole straight down through it at (20, 10): a cut square to x at
        // x = 20 crosses it sideways.
        let block = crate::body::make_box([0.0; 3], [40.0, 20.0, 10.0]).unwrap();
        let hole = crate::body::make_cylinder([20.0, 10.0, -5.0], 3.0, 20.0).unwrap();
        let holed = crate::boolean::boolean(block, hole, crate::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let refused = stretch(&holed, [20.0, 0.0, 0.0], X, 10.0).unwrap().err();
        assert!(
            matches!(
                refused,
                Some(Refused::Stretch(Refusal::Crosses {
                    crossing: Crossing::SidewaysRound,
                    ..
                }))
            ),
            "{refused:?}"
        );
        // Clear of the hole the same block stretches.
        let out = stretch(&holed, [32.0, 0.0, 0.0], X, 10.0).unwrap().unwrap();
        assert!((size(&out)[0] - 50.0).abs() < 1e-6);
    }
}
