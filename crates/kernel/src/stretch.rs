//! Stretch (#20512): make a body longer or shorter along one direction, keeping its details
//! their size. The body is cut across at a plane; the part on the plane's positive side moves
//! along the normal by the distance, and the gap is filled by the body's own section there.
//!
//! The section is taken from a band of the body just behind the plane, as deep as it can be. Where every face
//! crossing that band runs straight along the normal (a flat wall square to the plane, or a round
//! wall whose axis is the normal), the band is a prism, and lengthening it along the normal gives
//! exactly the body's section, as long as needed. A shrink takes out the slab in front of the
//! plane, which must be such a prism too. Anything else crossing the band (a sloped face, a
//! sideways hole, a round) is refused and named: the cut plane must move clear of it.

use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;

use crate::surface_kind::{FaceSurface, face_surface};
use crate::{Body, KernelError, Result, half_space, transform_matrix, union_all};

/// How far from square or parallel a face may be and still count as a wall or a cap (cosine).
const SQUARE: f64 = 1e-9;
/// Steps of the search for the longest shrink that is possible, when a shrink is refused.
const SHRINK_SEARCH_STEPS: usize = 12;
/// The most one pass lengthens the band behind the cut by (its new length over its old one).
const MAX_LENGTHEN: f64 = 8.0;

/// `body` stretched by `distance` (mm) along unit `normal` at the plane through `at`: the part
/// on `normal`'s side of the plane moves by `distance` (a negative distance shrinks it), the rest
/// stays where it is. Refused, with why, when the body is not a prism along `normal` at the plane
/// (see the module note), the plane misses the body, or a shrink would take out more than lies in
/// front of the plane.
pub fn stretch(body: &Body, at: Vec3, normal: Vec3, distance: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "stretch");
    body.require_brep("Stretch")?;
    let n = normal.normalized().ok_or_else(|| KernelError::Invalid("the stretch direction has no length".into()))?;
    if !(at.is_finite() && distance.is_finite()) {
        return Err(KernelError::Invalid("the cut plane and the distance must be numbers".into()));
    }
    let (lo, hi) = extent(body, n)?;
    let level = at.dot(n);
    let size = hi - lo;
    let eps = (size * 1e-9).max(1e-9);
    if !(level > lo + eps && level < hi - eps) {
        return Err(KernelError::Invalid("the cut plane does not cross the body: move it between the body's sides".into()));
    }
    if distance.abs() <= eps {
        return Ok(body.clone());
    }
    let plane = |h: f64| Plane::from_normal(n * h, n).ok_or_else(|| KernelError::Invalid("the stretch direction has no length".into()));
    if distance > 0.0 {
        let base_band = deepest_band(body, n, lo, level, size, eps, &plane)?;
        // Lengthening the band scales its edges' tolerances with it: each pass lengthens it at
        // most `MAX_LENGTHEN` times, and a long stretch takes several passes.
        let mut stretched = body.clone();
        let mut left = distance;
        while left > eps {
            let step = left.min(base_band * (MAX_LENGTHEN - 1.0));
            stretched = lengthen(&stretched, n, level - base_band, level, step, &plane)?;
            left -= step;
        }
        return Ok(stretched);
    }
    let shrink = -distance;
    if !(level + shrink < hi - eps) {
        return Err(KernelError::Invalid(format!(
            "a shrink of {} mm takes out everything in front of the cut plane, which is {} mm deep",
            round(shrink),
            round(hi - level)
        )));
    }
    let cut = slab(body, &plane(level)?, &plane(level + shrink)?)?;
    if let Err(why) = prism_along(&cut, n, level, level + shrink, eps) {
        let most = longest_shrink(body, n, level, shrink, eps, &plane)?;
        return Err(KernelError::Invalid(format!("{} The most it can shrink here is {} mm.", message(&why), round(most))));
    }
    let behind =
        half_space(body, &plane(level)?, false)?.0.ok_or_else(|| KernelError::Failed("nothing of the body lies behind the cut plane".into()))?;
    let ahead = half_space(body, &plane(level + shrink)?, true)?
        .0
        .ok_or_else(|| KernelError::Failed("nothing of the body lies ahead of the slice".into()))?;
    let moved = transform_matrix(&ahead, translate(n * -shrink))?;
    union_all(&[&behind, &moved])
}

/// The deepest band behind the plane at `level` that is a prism along `n`, as a depth: from
/// half of what lies behind the plane, halved until it is one (a deeper band needs fewer
/// passes). Refused, naming the face, when not even a thin band is.
/// A point on the cut plane of one stretch along unit `normal`: `at` mm along the normal from
/// the picked face point `from`, or with no face, from the side of `body` that stays put (its
/// least level along the normal).
pub fn stretch_cut(body: &Body, normal: Vec3, from: Option<Vec3>, at: f64) -> Result<Vec3> {
    let n = normal.normalized().ok_or_else(|| KernelError::Invalid("the stretch direction has no length".into()))?;
    let base = match from {
        Some(face) => face.dot(n),
        None => extent(body, n)?.0,
    };
    Ok(n * (base + at))
}

/// Where a straight feature on the stretched body goes (a cosmetic thread's axis): the axis
/// through `point` along unit `axis`, used from `start` to `end` along it, after the stretch of
/// [`stretch`] at `at` along `normal` by `distance`. Wholly beyond the cut, it moves with the
/// part. Running along the normal across the cut, its end beyond the cut moves and its length
/// changes with the body's. Anything else stays where it is.
pub fn stretch_axis(point: Vec3, axis: Vec3, start: f64, end: f64, at: Vec3, normal: Vec3, distance: f64) -> (Vec3, f64, f64) {
    let Some(n) = normal.normalized() else { return (point, start, end) };
    let level = at.dot(n);
    let beyond = |t: f64| (point + axis * t).dot(n) > level;
    match (beyond(start), beyond(end)) {
        (true, true) => (point + n * distance, start, end),
        (false, false) => (point, start, end),
        (start_beyond, _) => {
            let along = axis.dot(n);
            if (along.abs() - 1.0).abs() > SQUARE {
                return (point, start, end);
            }
            // Exactly one end is beyond the cut: it moves by the distance along the normal.
            let shift = distance * along;
            if start_beyond { (point, start + shift, end) } else { (point, start, end + shift) }
        }
    }
}

fn deepest_band(body: &Body, n: Vec3, lo: f64, level: f64, size: f64, eps: f64, plane: &dyn Fn(f64) -> Result<Plane>) -> Result<f64> {
    let thinnest = (size * 1e-3).max(1e-3).min((level - lo) / 2.0);
    let mut depth = (level - lo) / 2.0;
    loop {
        let band = slab(body, &plane(level - depth)?, &plane(level)?)?;
        match prism_along(&band, n, level - depth, level, eps) {
            Ok(()) => return Ok(depth),
            Err(why) if depth <= thinnest => return Err(KernelError::Invalid(message(&why))),
            Err(_) => depth = (depth / 2.0).max(thinnest),
        }
    }
}

/// `body` with its band between levels `base` and `level` along `n` lengthened by `step`: the
/// part ahead of `level` moves by `step`, and the band, scaled along `n`, fills the gap.
fn lengthen(body: &Body, n: Vec3, base: f64, level: f64, step: f64, plane: &dyn Fn(f64) -> Result<Plane>) -> Result<Body> {
    let band = slab(body, &plane(base)?, &plane(level)?)?;
    let lengthened = transform_matrix(&band, scale_along(n, base, (level - base + step) / (level - base)))?;
    let behind = half_space(body, &plane(base)?, false)?.0;
    let ahead =
        half_space(body, &plane(level)?, true)?.0.ok_or_else(|| KernelError::Failed("nothing of the body lies ahead of the cut plane".into()))?;
    let moved = transform_matrix(&ahead, translate(n * step))?;
    let parts: Vec<&Body> = behind.iter().chain([&lengthened, &moved]).collect();
    union_all(&parts)
}

/// The body's least and greatest level along unit `n`, from its exact box.
fn extent(body: &Body, n: Vec3) -> Result<(f64, f64)> {
    let b = body.exact_bounds()?;
    let corners = [b.min.x, b.max.x].into_iter().flat_map(|x| [b.min.y, b.max.y].into_iter().map(move |y| (x, y)));
    let levels: Vec<f64> = corners.flat_map(|(x, y)| [b.min.z, b.max.z].map(|z| Vec3::new(x, y, z).dot(n))).collect();
    let lo = levels.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !(lo.is_finite() && hi.is_finite() && hi > lo) {
        return Err(KernelError::Invalid("the body has no size along the stretch direction".into()));
    }
    Ok((lo, hi))
}

/// The part of `body` between two parallel planes (`from` faces toward `to`).
fn slab(body: &Body, from: &Plane, to: &Plane) -> Result<Body> {
    let ahead = half_space(body, from, true)?.0.ok_or_else(|| KernelError::Invalid("the cut plane does not cross the body".into()))?;
    half_space(&ahead, to, false)?.0.ok_or_else(|| KernelError::Invalid("the cut plane does not cross the body".into()))
}

/// Why a slab is not a prism along the stretch direction.
#[derive(Debug)]
enum NotPrism {
    /// A face that does not run straight along the direction: its kind, and where it is.
    Face { kind: &'static str, at: Vec3 },
}

fn message(why: &NotPrism) -> String {
    match why {
        NotPrism::Face { kind, at } => format!(
            "The cut plane crosses {kind} near ({}, {}, {}), which a stretch would change: move the plane clear of it.",
            round(at.x),
            round(at.y),
            round(at.z)
        ),
    }
}

/// Is `band`, the body between levels `lo` and `hi` along `n`, a prism along `n`: every face a
/// cap at one of the two levels, a flat wall square to them, or a round wall about an axis along
/// `n`?
fn prism_along(band: &Body, n: Vec3, lo: f64, hi: f64, eps: f64) -> std::result::Result<(), NotPrism> {
    for face in 0..band.face_count() {
        match face_surface(band, face) {
            Some(FaceSurface::Plane { normal, point }) => {
                let c = n.dot(normal).abs();
                let level = point.dot(n);
                let cap = 1.0 - c <= SQUARE && ((level - lo).abs() <= eps || (level - hi).abs() <= eps);
                if 1.0 - c <= SQUARE && !cap {
                    return Err(NotPrism::Face { kind: "a step", at: point });
                }
                if !cap && c > SQUARE.sqrt() {
                    return Err(NotPrism::Face { kind: "a sloped face", at: point });
                }
            }
            Some(FaceSurface::Cylinder { axis, origin, .. }) => {
                if 1.0 - n.dot(axis).abs() > SQUARE {
                    return Err(NotPrism::Face { kind: "a round face across the direction", at: origin });
                }
            }
            Some(FaceSurface::Cone { apex, .. }) => return Err(NotPrism::Face { kind: "a cone or chamfered hole", at: apex }),
            Some(FaceSurface::Sphere { center, .. }) => return Err(NotPrism::Face { kind: "a ball", at: center }),
            Some(FaceSurface::Torus { center, .. }) => return Err(NotPrism::Face { kind: "a rounded edge", at: center }),
            None => {
                let at = band.face_plane(face).map(|p| p.origin).unwrap_or(Vec3::ZERO);
                return Err(NotPrism::Face { kind: "a free-form face", at });
            }
        }
    }
    Ok(())
}

/// The longest shrink at most `wanted` whose slab is a prism (found by halving).
fn longest_shrink(body: &Body, n: Vec3, level: f64, wanted: f64, eps: f64, plane: &dyn Fn(f64) -> Result<Plane>) -> Result<f64> {
    let (mut good, mut bad) = (0.0, wanted);
    for _ in 0..SHRINK_SEARCH_STEPS {
        let mid = (good + bad) / 2.0;
        let fits = slab(body, &plane(level)?, &plane(level + mid)?).ok().is_some_and(|s| prism_along(&s, n, level, level + mid, eps).is_ok());
        if fits {
            good = mid;
        } else {
            bad = mid;
        }
    }
    Ok(good)
}

/// Column-major matrix scaling by `k` along unit `n`, about the plane at level `base`.
fn scale_along(n: Vec3, base: f64, k: f64) -> [[f64; 4]; 4] {
    let n = [n.x, n.y, n.z];
    let mut m = [[0.0; 4]; 4];
    for (j, column) in m.iter_mut().take(3).enumerate() {
        for (i, cell) in column.iter_mut().take(3).enumerate() {
            *cell = if i == j { 1.0 } else { 0.0 } + (k - 1.0) * n[i] * n[j];
        }
    }
    for (i, ni) in n.iter().enumerate() {
        m[3][i] = -(k - 1.0) * ni * base;
    }
    m[3][3] = 1.0;
    m
}

fn translate(t: Vec3) -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [t.x, t.y, t.z, 1.0]]
}

fn round(x: f64) -> f64 {
    (x * 1000.0).floor() / 1000.0
}

#[cfg(test)]
#[path = "stretch_tests.rs"]
mod tests;
