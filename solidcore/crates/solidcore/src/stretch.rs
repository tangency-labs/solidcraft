//! Stretch: a body made longer or shorter along one direction, its details kept their size.
//!
//! The body is cut across at a plane square to a unit direction `n`; the part on `n`'s side of
//! the plane moves along `n` by the distance and the gap is filled by the body's own section
//! there. The section is taken from a band of the body just behind the plane: where every face
//! crossing the band runs straight along `n` (a flat wall square to the plane, a cylinder whose
//! axis is `n`, a ruled net along `n`) and the band's other faces are its two caps, the band is
//! a prism along `n` and scaling it along `n` gives the body's section as long as needed. A
//! shrink takes out the slab in front of the plane, which must be such a prism too. Anything
//! else crossing the band is refused and named, with a point on it.
//!
//! This module holds the geometry: the classification of a band's faces ([`band`]), and every
//! number the operation needs, worked out by [`Stretch`]: the cut planes, how deep a band to try,
//! the passes that lengthen it with their maps, the shrink's slab and move, and the search for
//! the longest shrink that builds. `solidcore-ops` sequences the cuts, maps and unions.
//!
//! Not upstream: SolidCraft's Stretch (its issue 20512), moved here from SolidCraft's kernel
//! crate (docs/layers.md §1). Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::prism::measure::{
    cylinder_of, face_points, outward_is_natural, plane_of, walk, wall_error,
};
use crate::query::api_queries::{brep_bounding_box, get_faces};
use crate::query::face::p1::calculate_bounding_box;
use crate::topology::Brep;

/// How far from square or parallel a face may be and still count as a wall or a cap (one less
/// the cosine).
pub const SQUARE: f64 = 1e-9;
/// Halvings of the search for the longest shrink that builds, when a shrink is refused.
pub const SHRINK_SEARCH_STEPS: usize = 12;
/// The most one pass lengthens the band behind the cut by (its new length over its old one):
/// scaling a band scales its edges' tolerances with it, so a long stretch takes several passes.
pub const MAX_LENGTHEN: f64 = 8.0;

/// An affine map in the row-vector convention: rows 0 to 2 the images of x, y and z, row 3 the
/// translation (`p' = p·A + t`), the fourth column `(0, 0, 0, 1)`.
pub type Matrix = [[f64; 4]; 4];

/// What crosses a band that keeps it from being a prism along the direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crossing {
    /// A flat face square to the direction inside the band: a step.
    Step,
    /// A flat face neither square to the direction nor along it.
    SlopedFace,
    /// A cylinder whose axis is not the direction: a sideways hole or boss.
    SidewaysRound,
    /// Any other face: a cone, a ball, a round, a free-form face.
    CurvedFace,
}

/// Why a stretch is refused.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// The direction has no length, or a number is not finite.
    Argument,
    /// The cut plane does not cross the body.
    Misses,
    /// A face crossing the band or slab is not a wall along the direction: what it is and a
    /// point on it.
    Crosses {
        /// What crosses.
        crossing: Crossing,
        /// A point on it.
        at: [f64; 3],
    },
    /// A shrink that takes out everything in front of the cut plane: the shrink and how deep the
    /// body is in front of the plane.
    TooDeep {
        /// The shrink asked for.
        shrink: f64,
        /// How far the body reaches in front of the cut plane.
        room: f64,
    },
}

/// A plane a body is cut by, as a plane surface over a parameter box that covers the body:
/// `origin + s·u + t·v` for `s` in `domain[0]` and `t` in `domain[1]`. Its normal `u × v` is the
/// stretch direction, so the body's part "behind" it is the part that stays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutPlane {
    /// A point of the plane, near the body.
    pub origin: [f64; 3],
    /// Its first unit axis.
    pub u: [f64; 3],
    /// Its second unit axis.
    pub v: [f64; 3],
    /// The parameter box that covers the body.
    pub domain: [[f64; 2]; 2],
    /// Its level along the direction.
    pub level: f64,
}

/// One pass of a lengthening: the band from `base` to `cut` scaled by `scale`, the part ahead
/// of `cut` moved by `shift`, the part behind `base` kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pass {
    /// The band's far side.
    pub base: CutPlane,
    /// The cut plane.
    pub cut: CutPlane,
    /// The band's map: a scale along the direction about `base`.
    pub scale: Matrix,
    /// The map of the part ahead of the cut.
    pub shift: Matrix,
}

/// A shrink's work: the slab from `cut` to `far` that is taken out, and the move of the part
/// ahead of `far` back onto `cut`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shrink {
    /// The cut plane.
    pub cut: CutPlane,
    /// The slab's far side.
    pub far: CutPlane,
    /// The map of the part ahead of `far`.
    pub shift: Matrix,
}

/// One stretch of one body, worked out from its box: where it cuts, how far, and every plane
/// and map the operation needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretch {
    n: Vec3,
    u: Vec3,
    v: Vec3,
    centre: Point3,
    reach: f64,
    lo: f64,
    hi: f64,
    level: f64,
    distance: f64,
    eps: f64,
}

impl Stretch {
    /// The stretch of `b` by `distance` along `normal` at the plane through `at`, from the
    /// body's tight box.
    ///
    /// # Errors
    /// The kernel's, from the box. A refusal for a direction with no length, numbers not
    /// finite, or a plane that does not cross the body.
    pub fn new(
        k: &Kernel,
        b: &Brep,
        at: [f64; 3],
        normal: [f64; 3],
        distance: f64,
    ) -> crate::Result<Result<Stretch, Refusal>> {
        let bounds = brep_bounding_box(k, b, true)?;
        Ok(Self::of_box(
            bounds.min().to_array(),
            bounds.max().to_array(),
            at,
            normal,
            distance,
        ))
    }

    /// [`Stretch::new`] for a body whose box is `min` to `max`.
    ///
    /// # Errors
    /// A refusal for a direction with no length, numbers not finite, or a plane that does not
    /// cross the box.
    pub fn of_box(
        min: [f64; 3],
        max: [f64; 3],
        at: [f64; 3],
        normal: [f64; 3],
        distance: f64,
    ) -> Result<Stretch, Refusal> {
        let finite = |p: [f64; 3]| p.iter().all(|c| c.is_finite());
        if !(finite(min) && finite(max) && finite(at) && finite(normal) && distance.is_finite()) {
            return Err(Refusal::Argument);
        }
        let n = Vec3::from(normal)
            .unitize()
            .map_err(|_| Refusal::Argument)?;
        let (min, max) = (Point3::from(min), Point3::from(max));
        let corners = (0..8).map(|i| {
            Point3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            )
            .dot(n)
        });
        let (lo, hi) = corners.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), c| {
            (l.min(c), h.max(c))
        });
        let size = hi - lo;
        if !(size.is_finite() && size > 0.0) {
            return Err(Refusal::Misses);
        }
        let eps = (size * 1e-9).max(1e-9);
        let level = Point3::from(at).dot(n);
        if !(level > lo + eps && level < hi - eps) {
            return Err(Refusal::Misses);
        }
        // The plane's axes: x the world axis n is least along made square to n, y = n × x,
        // so that x × y = n.
        let pick = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
            Vec3::new(1.0, 0.0, 0.0)
        } else if n.y.abs() <= n.z.abs() {
            Vec3::new(0.0, 1.0, 0.0)
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };
        let u = (pick - n * pick.dot(n))
            .unitize()
            .map_err(|_| Refusal::Argument)?;
        let v = n.cross(u);
        let centre = (min + max) * 0.5;
        // Every plane cuts within the box grown by the stretch: twice the box's diagonal and
        // the stretch's length cover any part of it, before or after a pass.
        let reach = ((max - min).length() + distance.abs()) * 2.0 + 1.0;
        Ok(Stretch {
            n,
            u,
            v,
            centre,
            reach,
            lo,
            hi,
            level,
            distance,
            eps,
        })
    }

    /// Whether the distance is too small to change the body: it stays as it is.
    #[must_use]
    pub fn is_nothing(&self) -> bool {
        self.distance.abs() <= self.eps
    }

    /// Whether the stretch lengthens the body (a positive distance); else it shrinks it.
    #[must_use]
    pub fn grows(&self) -> bool {
        self.distance > 0.0
    }

    /// The tolerance the band's caps are read with: the body's size along the direction times
    /// 10⁻⁹, at least 10⁻⁹.
    #[must_use]
    pub fn eps(&self) -> f64 {
        self.eps
    }

    /// The direction, unit.
    #[must_use]
    pub fn direction(&self) -> [f64; 3] {
        self.n.to_array()
    }

    /// The plane square to the direction at `level` along it.
    #[must_use]
    pub fn plane(&self, level: f64) -> CutPlane {
        let origin = self.centre + self.n * (level - self.centre.dot(self.n));
        CutPlane {
            origin: origin.to_array(),
            u: self.u.to_array(),
            v: self.v.to_array(),
            domain: [[-self.reach, self.reach], [-self.reach, self.reach]],
            level,
        }
    }

    /// The cut plane.
    #[must_use]
    pub fn cut(&self) -> CutPlane {
        self.plane(self.level)
    }

    /// The depths of band to try behind the cut, deepest first: half the body behind the cut,
    /// halved each time down to a thousandth of the body's size (at least 10⁻³ mm), whichever is
    /// deeper; each with the band's far side.
    #[must_use]
    pub fn band_depths(&self) -> Vec<(f64, CutPlane)> {
        let behind = self.level - self.lo;
        let thinnest = ((self.hi - self.lo) * 1e-3).max(1e-3).min(behind / 2.0);
        let mut depth = behind / 2.0;
        let mut out = Vec::new();
        loop {
            out.push((depth, self.plane(self.level - depth)));
            if depth <= thinnest || out.len() > 64 {
                return out;
            }
            depth = (depth / 2.0).max(thinnest);
        }
    }

    /// The passes that lengthen a band `depth` deep behind the cut by the distance, each at
    /// most [`MAX_LENGTHEN`] times the band's length. Empty for a shrink.
    #[must_use]
    pub fn passes(&self, depth: f64) -> Vec<Pass> {
        if !self.grows() || !(depth > 0.0) {
            return Vec::new();
        }
        let base = self.level - depth;
        let mut left = self.distance;
        let mut out = Vec::new();
        while left > self.eps && out.len() < 4096 {
            let step = left.min(depth * (MAX_LENGTHEN - 1.0));
            out.push(Pass {
                base: self.plane(base),
                cut: self.cut(),
                scale: scale_along(self.n, base, (depth + step) / depth),
                shift: translation(self.n * step),
            });
            left -= step;
        }
        out
    }

    /// A shrink's slab and move.
    ///
    /// # Errors
    /// [`Refusal::TooDeep`] when the shrink takes out everything in front of the cut plane.
    pub fn shrink(&self) -> Result<Shrink, Refusal> {
        let shrink = -self.distance;
        if !(self.level + shrink < self.hi - self.eps) {
            return Err(Refusal::TooDeep {
                shrink,
                room: self.hi - self.level,
            });
        }
        Ok(Shrink {
            cut: self.cut(),
            far: self.plane(self.level + shrink),
            shift: translation(self.n * -shrink),
        })
    }

    /// The search for the longest shrink that builds, as the shrink is refused.
    #[must_use]
    pub fn shrink_search(&self) -> ShrinkSearch {
        ShrinkSearch {
            good: 0.0,
            bad: (-self.distance).max(0.0),
            steps: 0,
        }
    }
}

/// The bisection for the longest shrink whose slab is a prism: ask [`ShrinkSearch::next`] for
/// a shrink to try, tell [`ShrinkSearch::found`] whether its slab is one, until `next` gives
/// `None`; [`ShrinkSearch::most`] is then the longest found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShrinkSearch {
    good: f64,
    bad: f64,
    steps: usize,
}

impl ShrinkSearch {
    /// The next shrink to try and its slab's far side, `None` when the search is done.
    #[must_use]
    pub fn next(&self, stretch: &Stretch) -> Option<(f64, CutPlane)> {
        (self.steps < SHRINK_SEARCH_STEPS).then(|| {
            let mid = (self.good + self.bad) / 2.0;
            (mid, stretch.plane(stretch.level + mid))
        })
    }

    /// Records whether the slab of the shrink [`ShrinkSearch::next`] gave is a prism.
    pub fn found(&mut self, fits: bool) {
        let mid = (self.good + self.bad) / 2.0;
        if fits {
            self.good = mid;
        } else {
            self.bad = mid;
        }
        self.steps += 1;
    }

    /// The longest shrink found to build.
    #[must_use]
    pub fn most(&self) -> f64 {
        self.good
    }
}

/// Whether the band of `b` from level `lo` to `hi` along unit `normal` is a prism along it:
/// every face a cap at `lo` or `hi` (within `eps`), a flat wall square to the planes, a
/// cylinder with its axis along `normal`, or a net ruled along it (within `eps`). Else what
/// crosses, with a point on it.
///
/// # Errors
/// The kernel's, reading the faces.
pub fn band(
    b: &Brep,
    normal: [f64; 3],
    lo: f64,
    hi: f64,
    eps: f64,
) -> crate::Result<Result<(), Refusal>> {
    let Ok(n) = Vec3::from(normal).unitize() else {
        return Ok(Err(Refusal::Argument));
    };
    let mut work = 2_000_000usize;
    for f in get_faces(b)? {
        let face = b.face(f)?;
        let point = outward_is_natural(b, f)
            .and_then(|natural| walk(b, f, natural))
            .and_then(|w| face_points(b, &w).next());
        let crosses = |crossing: Crossing, at: Option<Point3>| {
            Err(Refusal::Crosses {
                crossing,
                at: at.or(point).unwrap_or(Vec3::ZERO).to_array(),
            })
        };
        let Some(surface) = face.surface.as_ref() else {
            return Ok(crosses(Crossing::CurvedFace, None));
        };
        if let Some((_, pn)) = plane_of(surface) {
            let c = pn.dot(n).abs();
            if 1.0 - c <= SQUARE {
                let level = point.map_or(f64::NAN, |p| p.dot(n));
                if (level - lo).abs() <= eps || (level - hi).abs() <= eps {
                    continue;
                }
                return Ok(crosses(Crossing::Step, None));
            }
            if c > SQUARE.sqrt() {
                return Ok(crosses(Crossing::SlopedFace, None));
            }
            continue;
        }
        if let Some((origin, axis, _)) = cylinder_of(surface) {
            if 1.0 - axis.dot(n).abs() > SQUARE {
                return Ok(crosses(Crossing::SidewaysRound, Some(origin)));
            }
            continue;
        }
        match wall_error(surface, n, hi - lo, &mut work) {
            Some(e) if e <= eps => {}
            _ => return Ok(crosses(Crossing::CurvedFace, None)),
        }
    }
    Ok(Ok(()))
}

/// Where a stretch of `b` along unit `normal` is best cut when no place is given: the middle of
/// the longest run along the normal where the body is a prism (every face crossing it a wall
/// along the normal), as the distance from the body's least level along the normal. A flat face
/// square to the normal breaks the run at its level; any other face that is not such a wall
/// breaks it over its box's reach along the normal. Half the body's length when no run is left.
/// Measured from the picked face point `from` when there is one (as a stretch's Cut At is),
/// else from the body's least level along the normal. `None` for a direction with no length or
/// a body with no length along it.
///
/// # Errors
/// The kernel's, reading the faces and their boxes.
pub fn proposed_cut(
    k: &Kernel,
    b: &Brep,
    normal: [f64; 3],
    from: Option<[f64; 3]>,
) -> crate::Result<Option<f64>> {
    let Ok(n) = Vec3::from(normal).unitize() else {
        return Ok(None);
    };
    let [lo, hi] = brep_bounding_box(k, b, true)?.span_along(Point3::ZERO, n);
    let size = hi - lo;
    if !(size.is_finite() && size > 0.0) {
        return Ok(None);
    }
    let mut breaks: Vec<[f64; 2]> = Vec::new();
    let mut work = 2_000_000usize;
    for f in get_faces(b)? {
        let face = b.face(f)?;
        let wall = face.surface.as_ref().is_some_and(|surface| {
            if let Some((_, pn)) = plane_of(surface) {
                return pn.dot(n).abs() <= SQUARE.sqrt();
            }
            if let Some((_, axis, _)) = cylinder_of(surface) {
                return 1.0 - axis.dot(n).abs() <= SQUARE;
            }
            wall_error(surface, n, size, &mut work).is_some_and(|e| e <= size * 1e-9)
        });
        if wall {
            continue;
        }
        let [a, z] = calculate_bounding_box(k, b, f, true)?.span_along(Point3::ZERO, n);
        breaks.push([a, z]);
    }
    breaks.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let (mut best, mut best_len) = ((lo + hi) / 2.0, 0.0);
    let mut reached = lo;
    for [a, z] in breaks.iter().copied().chain([[hi, hi]]) {
        if a > reached && a - reached > best_len {
            best_len = a - reached;
            best = (a + reached) / 2.0;
        }
        reached = reached.max(z);
    }
    let base = from.map_or(lo, |p| Point3::from(p).dot(n));
    Ok(Some(best - base))
}

/// How a sketch plane with unit normal `plane_normal` lies to a stretch along `normal`: the
/// stretch runs in its plane, along its normal (with the normal or against it), or neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lies {
    /// The stretch runs in the plane.
    InPlane,
    /// The stretch runs along the plane's normal; `true` with it, `false` against it.
    Along(bool),
    /// Neither.
    Oblique,
}

/// How the plane with normal `plane_normal` lies to a stretch along `normal` ([`Lies`]).
#[must_use]
pub fn plane_lies(plane_normal: [f64; 3], normal: [f64; 3]) -> Lies {
    let (Ok(p), Ok(n)) = (
        Vec3::from(plane_normal).unitize(),
        Vec3::from(normal).unitize(),
    ) else {
        return Lies::Oblique;
    };
    let c = p.dot(n);
    if 1.0 - c.abs() < SQUARE {
        Lies::Along(c > 0.0)
    } else if c.abs() <= SQUARE {
        Lies::InPlane
    } else {
        Lies::Oblique
    }
}

/// A sketch plane: its origin and unit axes, a 2D point `[x, y]` on it being
/// `origin + x·x_axis + y·y_axis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SketchPlane {
    /// Its origin.
    pub origin: [f64; 3],
    /// Its x axis.
    pub x_axis: [f64; 3],
    /// Its y axis.
    pub y_axis: [f64; 3],
}

impl SketchPlane {
    fn world(&self, p: [f64; 2]) -> Point3 {
        Point3::from(self.origin) + Vec3::from(self.x_axis) * p[0] + Vec3::from(self.y_axis) * p[1]
    }

    /// How far the sketch point `p` stands past the cut plane through `cut` along `normal`
    /// (negative behind it): the side of the cut it is on.
    #[must_use]
    pub fn past_cut(&self, p: [f64; 2], cut: [f64; 3], normal: [f64; 3]) -> f64 {
        let n = Vec3::from(normal).unitize_or_z();
        (self.world(p) - Point3::from(cut)).dot(n)
    }

    /// The sketch point `p` moved `distance` along `normal` (a direction in the plane).
    #[must_use]
    pub fn moved(&self, p: [f64; 2], normal: [f64; 3], distance: f64) -> [f64; 2] {
        let n = Vec3::from(normal).unitize_or_z();
        [
            p[0] + n.dot(Vec3::from(self.x_axis)) * distance,
            p[1] + n.dot(Vec3::from(self.y_axis)) * distance,
        ]
    }

    /// How the segment from sketch point `a` to `b` runs to `normal`: `Some(true)` along it,
    /// `Some(false)` square to it, `None` neither or a segment of no length.
    #[must_use]
    pub fn runs(&self, a: [f64; 2], b: [f64; 2], normal: [f64; 3]) -> Option<bool> {
        let d = (self.world(b) - self.world(a)).unitize().ok()?;
        let c = d.dot(Vec3::from(normal).unitize_or_z()).abs();
        if c > 1.0 - SQUARE {
            Some(true)
        } else if c < SQUARE {
            Some(false)
        } else {
            None
        }
    }
}

/// Whether two bodies are the same part to a millionth of its size: their boxes' corners within
/// a millionth of `b`'s diagonal (at least 1) and their volumes within a millionth of `b`'s.
#[must_use]
pub fn same_part(a: ([f64; 3], [f64; 3], f64), b: ([f64; 3], [f64; 3], f64)) -> bool {
    let (a_lo, a_hi, a_vol) = a;
    let (b_lo, b_hi, b_vol) = b;
    let size = (Point3::from(b_hi) - Point3::from(b_lo)).length().max(1.0);
    let near =
        |p: [f64; 3], q: [f64; 3]| (Point3::from(p) - Point3::from(q)).length() <= size * 1e-6;
    near(a_lo, b_lo) && near(a_hi, b_hi) && (a_vol - b_vol).abs() <= b_vol.abs() * 1e-6
}

/// A point on the cut plane of a stretch along `normal`: `at` along the unit normal from the
/// picked face point `from`, or with no face, from the side that stays put: `low`, the body's
/// least level along the normal. `None` for a direction with no length.
#[must_use]
pub fn cut_point(normal: [f64; 3], from: Option<[f64; 3]>, low: f64, at: f64) -> Option<[f64; 3]> {
    let n = Vec3::from(normal).unitize().ok()?;
    let base = from.map_or(low, |p| Point3::from(p).dot(n));
    Some((n * (base + at)).to_array())
}

/// Where a straight feature on a stretched body goes (a cosmetic thread's axis): the axis
/// through `point` along unit `axis`, used from `start` to `end` along it, after the stretch at
/// the plane through `at` along `normal` by `distance`. Wholly beyond the cut, it moves with the
/// part; running along the normal across the cut, its end beyond the cut moves and its length
/// changes with the body's; anything else stays where it is.
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
    let Ok(n) = Vec3::from(normal).unitize() else {
        return (point, start, end);
    };
    let (p, a) = (Point3::from(point), Vec3::from(axis));
    let level = Point3::from(at).dot(n);
    let beyond = |t: f64| (p + a * t).dot(n) > level;
    match (beyond(start), beyond(end)) {
        (true, true) => ((p + n * distance).to_array(), start, end),
        (false, false) => (point, start, end),
        (start_beyond, _) => {
            let along = a.dot(n);
            if (along.abs() - 1.0).abs() > SQUARE {
                return (point, start, end);
            }
            // Exactly one end is beyond the cut: it moves by the distance along the normal.
            let shift = distance * along;
            if start_beyond {
                (point, start + shift, end)
            } else {
                (point, start, end + shift)
            }
        }
    }
}

/// The scale by `k` along unit `n` about the plane at level `base` along it.
fn scale_along(n: Vec3, base: f64, k: f64) -> Matrix {
    let n = [n.x, n.y, n.z];
    let mut m = [[0.0; 4]; 4];
    for (j, (row, nj)) in m.iter_mut().zip(n).enumerate() {
        for (i, (cell, ni)) in row.iter_mut().zip(n).enumerate() {
            *cell = if i == j { 1.0 } else { 0.0 } + (k - 1.0) * ni * nj;
        }
    }
    for (cell, ni) in m[3].iter_mut().zip(n) {
        *cell = -(k - 1.0) * ni * base;
    }
    m[3][3] = 1.0;
    m
}

/// The move by `t`.
fn translation(t: Vec3) -> Matrix {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [t.x, t.y, t.z, 1.0],
    ]
}
