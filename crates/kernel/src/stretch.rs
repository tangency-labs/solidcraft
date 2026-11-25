//! Stretch (#20512): make a body longer or shorter along one direction, keeping its details
//! their size. The geometry and the recipe are solidcore's ([`solidcore_ops::stretch`]): this
//! module hands the body over, takes the result back and words the refusals.

use solidcore_ops::stretch::{Crossing, Refusal, Refused};
use solidcore_record::Vec3;

use crate::{Body, KernelError, Result};

/// `body` stretched by `distance` (mm) along unit `normal` at the plane through `at`: the part
/// on `normal`'s side of the plane moves by `distance` (a negative distance shrinks it), the rest
/// stays where it is. Refused, with why, when the body is not a prism along `normal` at the plane,
/// the plane misses the body, or a shrink would take out more than lies in front of the plane.
pub fn stretch(body: &Body, at: Vec3, normal: Vec3, distance: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "stretch");
    body.require_brep("Stretch")?;
    let form = body.ops_body().ok_or_else(|| KernelError::Failed("Stretch: the body could not be read".into()))?;
    let (ops, _) = &*form;
    let failed = |e: solidcore_ops::Error| KernelError::Failed(format!("Stretch: {e}"));
    match solidcore_ops::stretch::stretch(ops, arr(at), arr(normal), distance).map_err(failed)? {
        Ok(made) => {
            let (record, _) = made.to_record().map_err(failed)?;
            Body::from_record(record)
        }
        Err(refused) => Err(KernelError::Invalid(words(&refused))),
    }
}

/// A point on the cut plane of a stretch of `body` along `normal`: `at` mm along it from the
/// picked face point `from`, or with no face, from the side of the body that stays put.
pub fn stretch_cut(body: &Body, normal: Vec3, from: Option<Vec3>, at: f64) -> Result<Vec3> {
    let b = body.exact_bounds()?;
    let invalid = || KernelError::Invalid("the stretch direction has no length".into());
    let [low, _] = solidcore_ops::affine::span_along(arr(b.min), arr(b.max), [0.0; 3], arr(normal)).map_err(|_| invalid())?;
    let p = solidcore_ops::stretch::cut_point(arr(normal), from.map(arr), low, at).ok_or_else(invalid)?;
    Ok(Vec3::new(p[0], p[1], p[2]))
}

/// Where a straight feature on the stretched body goes (a cosmetic thread's axis): the axis
/// through `point` along unit `axis`, used from `start` to `end`, after the stretch at `at`
/// along `normal` by `distance` ([`solidcore_ops::stretch::moved_axis`]).
pub fn stretch_axis(point: Vec3, axis: Vec3, start: f64, end: f64, at: Vec3, normal: Vec3, distance: f64) -> (Vec3, f64, f64) {
    let (p, s, e) = solidcore_ops::stretch::moved_axis(arr(point), arr(axis), start, end, arr(at), arr(normal), distance);
    (Vec3::new(p[0], p[1], p[2]), s, e)
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The refusal in the user's words.
fn words(refused: &Refused) -> String {
    match refused {
        Refused::Stretch(why) => refusal(why),
        Refused::Shrink { why, most } => format!("{} The most it can shrink here is {} mm.", refusal(why), shown(*most)),
    }
}

fn refusal(why: &Refusal) -> String {
    match why {
        Refusal::Argument => "the stretch direction has no length, or the cut plane or the distance is not a number".into(),
        Refusal::Misses => "the cut plane does not cross the body: move it between the body's sides".into(),
        Refusal::Crosses { crossing, at } => {
            let kind = match crossing {
                Crossing::Step => "a step",
                Crossing::SlopedFace => "a sloped face",
                Crossing::SidewaysRound => "a round face across the direction",
                Crossing::CurvedFace => "a curved face (a cone, a ball, a rounded edge or a free-form face)",
            };
            format!(
                "The cut plane crosses {kind} near ({}, {}, {}), which a stretch would change: move the plane clear of it.",
                shown(at[0]),
                shown(at[1]),
                shown(at[2])
            )
        }
        Refusal::TooDeep { shrink, room } => {
            format!("a shrink of {} mm takes out everything in front of the cut plane, which is {} mm deep", shown(*shrink), shown(*room))
        }
    }
}

/// A length as shown in a message: to a thousandth of a millimetre, never more than it is.
fn shown(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.get(..s.len().saturating_sub(1)).unwrap_or(&s);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
#[path = "stretch_tests.rs"]
mod tests;
