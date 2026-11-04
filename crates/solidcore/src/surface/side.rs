//! The sides of a face's parameter box, and the certified queries on them that extending a
//! sheet past one side needs: whether a side collapses to a point
//! ([`Surface::side_diameter`]), how far a surface runs across a side
//! ([`Surface::length_across_side`]).
//!
//! Not upstream: the operations SolidCraft composed over the kernel, moved into it so the
//! modelling layer computes no geometry (docs/layers.md §1). Each query returns its value with
//! a certified enclosure and decides nothing itself (owner rule 13564): the caller compares
//! the enclosure with its tolerance.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::borrow::Cow;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::{BSplineSurface, IsoCurveOptions, SurfParamType};
use crate::nurbs::math::{gamma, NOW};
use crate::surface::implicit::AnalyticSupport;
use crate::surface::surface::{Surface, SurfaceKind};

const OP_DIAMETER: &str = "surface::side::Surface::side_diameter";
const OP_LENGTH: &str = "surface::side::Surface::length_across_side";

/// The most leaves [`Surface::length_across_side`] subdivides into.
const MAX_LEAVES: usize = 1 << 20;

/// One side of a parameter box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxSide {
    /// `u = umin`.
    UMin,
    /// `u = umax`.
    UMax,
    /// `v = vmin`.
    VMin,
    /// `v = vmax`.
    VMax,
}

impl BoxSide {
    /// Every side, in the order `UMin, UMax, VMin, VMax`.
    pub const ALL: [Self; 4] = [Self::UMin, Self::UMax, Self::VMin, Self::VMax];

    /// The side as the kernel's extension direction.
    #[must_use]
    pub const fn param(self) -> SurfParamType {
        match self {
            Self::UMin => SurfParamType::UMin,
            Self::UMax => SurfParamType::UMax,
            Self::VMin => SurfParamType::VMin,
            Self::VMax => SurfParamType::VMax,
        }
    }

    /// Whether the side is a `u = const` line (the box grows across it in u).
    #[must_use]
    pub const fn is_u(self) -> bool {
        matches!(self, Self::UMin | Self::UMax)
    }

    /// Whether the side is at the box's high end.
    #[must_use]
    pub const fn is_high(self) -> bool {
        matches!(self, Self::UMax | Self::VMax)
    }

    /// The point of the side at `t` in `[0, 1]` along it.
    #[must_use]
    pub fn point(self, dom: &Extent2d, t: f64) -> Point2 {
        let (lo, hi) = (dom.min(), dom.max());
        let across = |a: f64, b: f64| a + t * (b - a);
        match self {
            Self::UMin => Point2::new(lo.x, across(lo.y, hi.y)),
            Self::UMax => Point2::new(hi.x, across(lo.y, hi.y)),
            Self::VMin => Point2::new(across(lo.x, hi.x), lo.y),
            Self::VMax => Point2::new(across(lo.x, hi.x), hi.y),
        }
    }

    /// The side's fixed parameter on `dom`.
    #[must_use]
    pub fn at(self, dom: &Extent2d) -> f64 {
        match self {
            Self::UMin => dom.min().x,
            Self::UMax => dom.max().x,
            Self::VMin => dom.min().y,
            Self::VMax => dom.max().y,
        }
    }
}

/// A length across a side ([`Surface::length_across_side`]): its certified enclosure, and
/// the enclosure's width split into its parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Across {
    /// The certified enclosure of the length.
    pub enclosure: Bracket,
    /// The truncation part: the leaves' control polygons less their chords, at most `rel` of
    /// the length when every leaf converged.
    pub truncation: f64,
    /// The rounding part: the sums' rounding and the leaves' points' (`gamma` of the chords
    /// and polygons, `2 (1 + p) N d`).
    pub rounding: f64,
    /// Whether every leaf met `rel`; a leaf too short to cut again before it did leaves the
    /// length not measured to `rel`.
    pub converged: bool,
}

/// A certified enclosure of a computed quantity: the exact value of the quantity, for the
/// surfaces as defined, lies in `[lower, upper]` (rounding included).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bracket {
    /// A proved lower bound.
    pub lower: f64,
    /// A proved upper bound.
    pub upper: f64,
}

impl Bracket {
    /// The midpoint, the value to report.
    #[must_use]
    pub fn mid(self) -> f64 {
        0.5 * (self.lower + self.upper)
    }

    /// `value` with a rounding bound `bound` either side.
    #[must_use]
    pub fn about(value: f64, bound: f64) -> Self {
        Self {
            lower: value - bound,
            upper: value + bound,
        }
    }
}

/// A rational Bezier piece in homogeneous coordinates `(w x, w y, w z, w)`.
type Piece = Vec<[f64; 4]>;

/// The B-spline form a query on `s` measures, and its certified distance from `s` as
/// defined: 0 for a B-spline, a ball patch's net and an analytic kind's NURBS form (the
/// exact class, proved by [`AnalyticSupport::check`] first: a form off its label is
/// refused); a procedural blend's stored bound for its net.
fn form_of<'a>(
    k: &Kernel,
    s: &'a Surface,
    op: &'static str,
) -> Result<(Cow<'a, BSplineSurface>, f64)> {
    let bound = match s.kind() {
        SurfaceKind::ProceduralBlend(pb) => pb.def.bound,
        _ => 0.0,
    };
    // an analytic kind is measured on its NURBS form only once the form is proved on it
    if AnalyticSupport::of(s).is_some() {
        AnalyticSupport::check(k, s)?;
    }
    let form = crate::blend::fillet_section::station::as_b_spline(k, s)?
        .ok_or_else(|| SolidError::invalid_input(op, "the surface has no NURBS form"))?;
    Ok((form, bound))
}

/// How far a computed homogeneous point of a piece can be from the exact one, and from it
/// the Euclidean point's error: each de Casteljau or knot-insertion level is a combination
/// `(1 - a) x + a y` whose coefficient rounds relative to the knots it is made of
/// (`|da| <= gamma_3 (|t| + |u_i| + |u_j|) / (u_j - u_i)`) and whose result rounds by
/// `gamma_3`, so a level moves a coordinate by at most `c = gamma_3 (1 + 6 K / S) . 2 M`,
/// `K` the largest knot, `S` the least span, `M` the net's largest coordinate. `L` levels
/// move it by `L c`; the Euclidean point `H / w` then by
/// `sqrt3 (2 L c' + u) M_H w_max / w_min^2` (`c'` the relative part of `c`).
#[derive(Debug, Clone, Copy)]
struct Rounding {
    /// The relative move of one level.
    level: f64,
    /// `M_H w_max / w_min^2`.
    scale: f64,
    /// `M_H`, the net's largest homogeneous coordinate.
    homogeneous: f64,
}

impl Rounding {
    fn of(form: &BSplineSurface, extra: &[f64], op: &'static str) -> Result<Self> {
        let nurb = form.gw_nurb().ok_or_else(|| SolidError::sm_err(op))?;
        let (mut h, mut wmin, mut wmax) = (0.0_f64, f64::INFINITY, 0.0_f64);
        for row in nurb.rows() {
            for c in row {
                let w = if c.w == NOW { 1.0 } else { c.w };
                if w.is_nan() || w <= 0.0 {
                    return Err(SolidError::invalid_input(op, "a weight is not positive"));
                }
                h = h.max(c.x.abs()).max(c.y.abs()).max(c.z.abs());
                wmin = wmin.min(w);
                wmax = wmax.max(w);
            }
        }
        let (mut kmax, mut smin) = (0.0_f64, f64::INFINITY);
        for kv in [nurb.u_knots(), nurb.v_knots()] {
            for (&a, &b) in kv.iter().zip(kv.iter().skip(1)) {
                kmax = kmax.max(a.abs()).max(b.abs());
                if b > a {
                    smin = smin.min(b - a);
                }
            }
        }
        for &x in extra {
            kmax = kmax.max(x.abs());
        }
        let level = gamma(3) * (1.0 + 6.0 * kmax / smin) * 2.0;
        let scale = h * wmax / (wmin * wmin);
        if level.is_finite() && scale.is_finite() {
            Ok(Self {
                level,
                scale,
                homogeneous: h.max(wmax),
            })
        } else {
            Err(SolidError::invalid_input(
                op,
                "the net's rounding is not bounded",
            ))
        }
    }

    /// The error of a homogeneous coordinate after `levels` levels.
    #[allow(clippy::cast_precision_loss)]
    fn homogeneous(self, levels: usize) -> f64 {
        (1.0 + 1e-12) * (2.0 * levels as f64 * self.level + f64::EPSILON) * self.homogeneous
    }

    /// The Euclidean error of a point after `levels` levels.
    #[allow(clippy::cast_precision_loss)]
    fn point(self, levels: usize) -> f64 {
        3f64.sqrt() * (1.0 + 1e-12) * (2.0 * levels as f64 * self.level + f64::EPSILON) * self.scale
    }
}

fn euclid(h: [f64; 4]) -> Vec3 {
    let [x, y, z, w] = h;
    Vec3::new(x / w, y / w, z / w)
}

/// The rounding a rational Bezier piece's length carries, from its homogeneous points as
/// computed, each coordinate within `made` of the exact piece's.
///
/// - **The leaves' cuts** (`cut`): `2 p` de Casteljau levels on the piece's points for
///   `t` in `[0, 1]`, each a convex combination rounding by `gamma_3` of the sum of the
///   magnitudes (at most `2 M_h`), so a leaf's homogeneous point is within
///   `4 p gamma_4 M_h` of the computed piece's, and its Euclidean point within that times
///   `(sqrt3 + M) / w_min` (`M` the largest Euclidean point, `w_min` the least weight).
/// - **The piece moved** by `made`: the length of the computed piece is within
///   `int |C~' - C'|` of the exact one's. With `C' = (P' - C w') / w`,
///   `C~' - C' = (dP' - C~ dw' - dC w') / w~ - C' dw / w~`; on `[0, 1]`, `|dP'| <= 2 p sqrt3
///   made` and `|dw'| <= 2 p made` (the derivative's control points are `p` times the
///   points' differences), `|dC| <= made (sqrt3 + M) / w_min`, `|w'| <= p max |w_i+1 - w_i|`
///   and `|C'| <= p (w_max / w_min)^2 max |E_i+1 - E_i|` (the rational derivative's bound),
///   each widened by `made` where it is taken from the computed points.
#[derive(Debug, Clone, Copy)]
struct PieceBounds {
    /// The degree.
    p: f64,
    /// The Euclidean error of a leaf's point from its cut.
    cut: f64,
    /// The bound on the length's move from the piece's rounding.
    moved: f64,
}

impl PieceBounds {
    /// `None` when a weight is not proved positive or a bound is not finite.
    fn of(piece: &[[f64; 4]], made: f64) -> Option<Self> {
        let n = piece.len().checked_sub(1)?;
        #[allow(clippy::cast_precision_loss)]
        let p = n as f64;
        let (mut mh, mut m, mut wmin, mut wmax) = (0.0_f64, 0.0_f64, f64::INFINITY, 0.0_f64);
        for h in piece {
            mh = h.iter().fold(mh, |x, c| x.max(c.abs()));
            m = m.max(euclid(*h).length());
            wmin = wmin.min(h[3]);
            wmax = wmax.max(h[3]);
        }
        let (mut leg, mut dw) = (0.0_f64, 0.0_f64);
        for (a, b) in piece.iter().zip(piece.iter().skip(1)) {
            leg = leg.max(euclid(*a).distance(euclid(*b)));
            dw = dw.max((b[3] - a[3]).abs());
        }
        // a few ulps on each bound computed here
        let slack = 1.0 + gamma(16);
        let sqrt3 = 3f64.sqrt() * slack;
        let cut_h = 4.0 * p * gamma(4) * mh;
        let low = wmin - made - cut_h;
        if low.is_nan() || low <= 0.0 {
            return None;
        }
        let cut = cut_h * (sqrt3 + m) / low * slack;
        let point = made * (sqrt3 + m) / low * slack;
        let w1 = p * (dw + 2.0 * made);
        let ratio = (wmax + made) / low;
        let d1 = p * ratio * ratio * (leg + 2.0 * point);
        let moved =
            (2.0 * p * sqrt3 * made + (m + point) * 2.0 * p * made + point * w1 + d1 * made) / low
                * slack;
        (cut.is_finite() && moved.is_finite()).then_some(Self { p, cut, moved })
    }
}

/// The piece `[lo, hi]` of `[0, 1]` of a Bezier piece (de Casteljau at `hi`, then the left
/// part's right at `lo / hi`): `2 p` levels.
fn cut(piece: &[[f64; 4]], lo: f64, hi: f64) -> Piece {
    let split = |c: &[[f64; 4]], t: f64, left: bool| -> Piece {
        let mut level = c.to_vec();
        let (mut l, mut r) = (Vec::new(), Vec::new());
        while let (Some(&f), Some(&e)) = (level.first(), level.last()) {
            l.push(f);
            r.push(e);
            level = level
                .iter()
                .zip(level.iter().skip(1))
                .map(|(x, y)| {
                    let mut c = [0.0; 4];
                    for (ci, (xi, yi)) in c.iter_mut().zip(x.iter().zip(y)) {
                        *ci = xi * (1.0 - t) + yi * t;
                    }
                    c
                })
                .collect();
        }
        r.reverse();
        if left {
            l
        } else {
            r
        }
    };
    let mut c = if hi < 1.0 {
        split(piece, hi, true)
    } else {
        piece.to_vec()
    };
    if lo > 0.0 {
        c = split(&c, lo / hi, false);
    }
    c
}

/// The Bezier pieces of the iso curve of `form` at `fixed` (constant v when `along_u`, else
/// constant u) between the parameters `from` and `to`, in order from `from`, and how many
/// rounding levels made them.
fn iso_pieces(
    k: &Kernel,
    form: &BSplineSurface,
    along_u: bool,
    fixed: f64,
    from: f64,
    to: f64,
) -> Result<(Vec<Piece>, usize)> {
    let param = if along_u {
        SurfParamType::V
    } else {
        SurfParamType::U
    };
    // within the form's knots, exactly: the iso extraction clamps a parameter outside them
    // (by up to 1e-6) and the pieces would be cut to them, neither said
    let nurb = form
        .gw_nurb()
        .ok_or_else(|| SolidError::sm_err(OP_LENGTH))?;
    let (held, along) = if along_u {
        (nurb.v_knots(), nurb.u_knots())
    } else {
        (nurb.u_knots(), nurb.v_knots())
    };
    let inside = |x: f64, k: &[f64]| matches!((k.first(), k.last()), (Some(&a), Some(&b)) if x >= a && x <= b);
    if !(inside(fixed, held) && inside(from, along) && inside(to, along)) {
        return Err(SolidError::invalid_input(
            OP_LENGTH,
            "a parameter is outside the surface's knots",
        ));
    }
    let opts = IsoCurveOptions {
        domain: None,
        uv_iso_curve: false,
    };
    let iso = form
        .create_iso_parametric_curve(k, param, fixed, opts)?
        .curve;
    let nurb = iso
        .nurb
        .as_ref()
        .ok_or_else(|| SolidError::sm_err(OP_LENGTH))?;
    let (a, b) = if from <= to { (from, to) } else { (to, from) };
    let mut out = Vec::new();
    let mut p = 0;
    for seg in nurb.decompose_bez()? {
        p = seg.degree();
        let (Some(&s0), Some(&s1)) = (seg.knots().first(), seg.knots().last()) else {
            continue;
        };
        let (lo, hi) = (a.max(s0), b.min(s1));
        if lo.partial_cmp(&hi) != Some(core::cmp::Ordering::Less) {
            continue;
        }
        let h: Piece = seg
            .cpts()
            .iter()
            .map(|c| {
                if c.w == NOW {
                    [c.x, c.y, c.z, 1.0]
                } else {
                    [c.x, c.y, c.z, c.w]
                }
            })
            .collect();
        out.push(cut(&h, (lo - s0) / (s1 - s0), (hi - s0) / (s1 - s0)));
    }
    if from > to {
        out.reverse();
        for piece in &mut out {
            piece.reverse();
        }
    }
    // the iso extraction (at most the fixed direction's degree + 1 levels), the
    // decomposition (p) and the cut (2 p)
    let q = form.degree(param);
    Ok((out, q + 1 + 3 * p))
}

impl Surface {
    /// The diameter of side `side` of the box `dom` (in the surface's NURBS parameters), as
    /// an enclosure: the side collapses to a point (a pole) within `tol` when `upper <= tol`,
    /// and does not when `lower > tol`.
    ///
    /// - **Upper:** the diameter of the control points of the side's iso curve (its Bezier
    ///   pieces cut to the side): a rational curve with positive weights lies in their hull.
    ///   At a pole the row collapses, so the bound is at rounding level.
    /// - **Lower:** the largest distance between the curve's points at the pieces' ends and
    ///   at the side's middle (SolidCraft's three points are among them).
    ///
    /// Both are widened by the rounding of making the pieces ([`Rounding`]) and, for a
    /// procedural blend measured on its net, twice its bound.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a surface without a NURBS form or with a weight not positive;
    /// those of the iso extraction and the decomposition.
    pub fn side_diameter(&self, k: &Kernel, dom: &Extent2d, side: BoxSide) -> Result<Bracket> {
        let (form, bound) = form_of(k, self, OP_DIAMETER)?;
        let (lo, hi) = (dom.min(), dom.max());
        let (from, to) = if side.is_u() {
            (lo.y, hi.y)
        } else {
            (lo.x, hi.x)
        };
        let mid = 0.5 * (from + to);
        let fixed = side.at(dom);
        let rounding = Rounding::of(&form, &[fixed, from, to], OP_DIAMETER)?;
        let mut pieces = Vec::new();
        let mut levels = 0;
        for (a, b) in [(from, mid), (mid, to)] {
            let (p, l) = iso_pieces(k, &form, !side.is_u(), fixed, a, b)?;
            pieces.extend(p);
            levels = levels.max(l);
        }
        let ends: Vec<Vec3> = pieces
            .iter()
            .flat_map(|p| [p.first(), p.last()])
            .flatten()
            .map(|h| euclid(*h))
            .collect();
        let hull: Vec<Vec3> = pieces.iter().flatten().map(|h| euclid(*h)).collect();
        if hull.is_empty() {
            return Err(SolidError::invalid_input(
                OP_DIAMETER,
                "the side has no extent",
            ));
        }
        let diameter = |pts: &[Vec3]| {
            let mut d = 0.0_f64;
            for (i, a) in pts.iter().enumerate() {
                for b in pts.iter().skip(i + 1) {
                    d = d.max(a.distance(*b));
                }
            }
            d
        };
        let e = 2.0 * rounding.point(levels) + 2.0 * bound;
        let (dl, du) = (diameter(&ends), diameter(&hull));
        Ok(Bracket {
            lower: (dl * (1.0 - gamma(6)) - e).max(0.0),
            upper: du * (1.0 + gamma(6)) + e,
        })
    }

    /// The length of the iso curve through the middle of side `side` of the box `dom` (in
    /// the surface's NURBS parameters), from the side across it to the end of the
    /// surface's natural domain: how far an extended surface runs past the side its face
    /// had. As an enclosure whose width is about `rel` times the length, plus rounding.
    ///
    /// **Method.** The curve's Bezier pieces are subdivided (each leaf cut from its piece
    /// directly, so the cuts' rounding does not accumulate with depth) until on every leaf
    /// the control polygon's length `q` and the chord `c` differ by at most `rel c`. The
    /// leaf's arc length `s` satisfies `c <= s <= q`: the chord is the shortest path, and a
    /// rational Bezier curve with positive weights crosses any plane no more often than its
    /// control polygon (variation diminishing), so by the Crofton formula it is no longer
    /// than the polygon.
    ///
    /// **Convergence.** On a leaf of parameter length `h` the gap `q - c` is `O(h^2)` times
    /// the leaf's length (for a circular arc of angle `t`, about `t^2 / 8` of it), so
    /// halving a leaf divides it by about 4: a quarter circle needs about `0.6 / sqrt(rel)`
    /// leaves. More than [`MAX_LEAVES`] is refused.
    ///
    /// **Rounding**, in two parts. The leaves enclose the length of the pieces as computed:
    /// each leaf's points are within its cut's rounding of that piece's (`2 p` de Casteljau
    /// levels, sized on the piece's own points), so its chord is within twice that and its
    /// polygon within `2 p` times it, and each distance rounds by `gamma_6`; over the leaves,
    /// `gamma_(N + 6 + p)` of the sums. The pieces' points are within the iso extraction's and
    /// decomposition's rounding of the exact curve's ([`Rounding`]), and the length moves by
    /// at most the integral of the derivative's change, bounded per piece in closed form
    /// ([`PieceBounds`]): a cost per piece, not per leaf, so a finer `rel` costs only the
    /// leaves' cuts.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `rel` not in `(0, 1)`, a procedural blend (its net's length
    /// bounds nothing of its definition's), a surface without a NURBS form or with a
    /// weight not positive; `NotConverged` past [`MAX_LEAVES`]; those of the iso
    /// extraction and the decomposition.
    pub fn length_across_side(
        &self,
        k: &Kernel,
        dom: &Extent2d,
        side: BoxSide,
        rel: f64,
    ) -> Result<Across> {
        if !(rel > 0.0 && rel < 1.0) {
            return Err(SolidError::invalid_input(OP_LENGTH, "rel is not in (0, 1)"));
        }
        if matches!(self.kind(), SurfaceKind::ProceduralBlend(_)) {
            return Err(SolidError::invalid_input(
                OP_LENGTH,
                "a blend's net bounds no length of its definition",
            ));
        }
        let (form, _) = form_of(k, self, OP_LENGTH)?;
        let natural = self.natural_uv_domain_with_nurb(k)?;
        let middle = side.point(dom, 0.5);
        let (fixed, from) = if side.is_u() {
            (middle.y, middle.x)
        } else {
            (middle.x, middle.y)
        };
        let to = match side {
            BoxSide::UMin => natural.min().x,
            BoxSide::UMax => natural.max().x,
            BoxSide::VMin => natural.min().y,
            BoxSide::VMax => natural.max().y,
        };
        if from == to {
            return Ok(Across {
                enclosure: Bracket {
                    lower: 0.0,
                    upper: 0.0,
                },
                truncation: 0.0,
                rounding: 0.0,
                converged: true,
            });
        }
        let rounding = Rounding::of(&form, &[fixed, from, to], OP_LENGTH)?;
        let (pieces, levels) = iso_pieces(k, &form, side.is_u(), fixed, from, to)?;
        // the pieces' homogeneous points as computed, within this of the exact ones
        let made = rounding.homogeneous(levels);
        let (mut chords, mut polygons, mut leaves) = (0.0_f64, 0.0_f64, 0_usize);
        let (mut chord_cuts, mut polygon_cuts, mut moved) = (0.0_f64, 0.0_f64, 0.0_f64);
        let mut converged = true;
        for piece in &pieces {
            // a weight not proved positive past the rounding: no measurement
            let Some(bounds) = PieceBounds::of(piece, made) else {
                return Ok(Across {
                    enclosure: Bracket {
                        lower: 0.0,
                        upper: f64::INFINITY,
                    },
                    truncation: f64::INFINITY,
                    rounding: f64::INFINITY,
                    converged: false,
                });
            };
            let mut n = 0_usize;
            let mut work = vec![(0.0_f64, 1.0_f64)];
            while let Some((a, b)) = work.pop() {
                let leaf = cut(piece, a, b);
                let pts: Vec<Vec3> = leaf.iter().map(|h| euclid(*h)).collect();
                let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
                    continue;
                };
                let c = first.distance(*last);
                let q: f64 = pts
                    .iter()
                    .zip(pts.iter().skip(1))
                    .map(|(a, b)| a.distance(*b))
                    .sum();
                if q - c > rel * c {
                    if b - a > f64::EPSILON {
                        let m = 0.5 * (a + b);
                        work.push((m, b));
                        work.push((a, m));
                        continue;
                    }
                    // a leaf too short to cut again before it met `rel`
                    converged = false;
                }
                leaves += 1;
                n += 1;
                if leaves > MAX_LEAVES {
                    return Err(SolidError::NotConverged {
                        operation: OP_LENGTH,
                        iterations: u32::try_from(MAX_LEAVES).unwrap_or(u32::MAX),
                    });
                }
                chords += c;
                polygons += q;
            }
            #[allow(clippy::cast_precision_loss)]
            let n = n as f64;
            // each leaf's chord within 2 of its cut's point error, its polygon within 2 p
            chord_cuts += n * 2.0 * bounds.cut;
            polygon_cuts += n * 2.0 * bounds.p * bounds.cut;
            moved += bounds.moved;
        }
        let g = gamma(leaves + 6 + pieces.iter().map(Vec::len).max().unwrap_or(0));
        // the leaves enclose the length of the pieces as computed; the exact curve's is within
        // `moved` of it (the pieces' points' rounding, through their derivatives)
        let rounding = g * (chords + polygons) + chord_cuts + polygon_cuts + 2.0 * moved;
        Ok(Across {
            enclosure: Bracket {
                lower: (chords * (1.0 - g) - chord_cuts - moved).max(0.0),
                upper: polygons * (1.0 + g) + polygon_cuts + moved,
            },
            truncation: polygons - chords,
            rounding,
            converged,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::cone::Cone;
    use crate::surface::sphere::{Sphere, SphereOptions};
    use std::f64::consts::{PI, SQRT_2};

    fn k() -> Kernel {
        Kernel::default()
    }

    fn sphere(r: f64, lon: [f64; 2], lat: [f64; 2]) -> Surface {
        let opts = SphereOptions {
            swap_uv: false,
            inside_out: false,
            circle_nurb: None,
        };
        let anal = Extent2d::from_bounds(lon[0], lat[0], lon[1], lat[1]);
        Surface::from(
            Sphere::new(
                &k(),
                Vec3::new(0.5, 1.0, -2.0),
                Vec3::X,
                Vec3::Y,
                anal,
                r,
                opts,
            )
            .unwrap(),
        )
    }

    /// A cylinder of radius `r` about z through (1, 2, 3), over the STEP box.
    fn cylinder(r: f64, a: [f64; 2], h: [f64; 2]) -> Cone {
        let k = k();
        let pl = Axis2Placement::new(Vec3::new(1.0, 2.0, 3.0), Vec3::X, Vec3::Y);
        let mut c = Cone::create_canonical(&k, &pl, r, r, 1.0, false, false).unwrap();
        c.adjust_step_uv_domain(&k, &Extent2d::from_bounds(a[0], h[0], a[1], h[1]))
            .unwrap();
        c
    }

    /// The NURBS box of the STEP box `[a0, a1] x [h0, h1]` on `c`.
    fn nurbs_box(c: &Cone, a: [f64; 2], h: [f64; 2]) -> Extent2d {
        let k = k();
        let p = c
            .convert_uv_from_step_to_nurbs(&k, Point2::new(a[0], h[0]))
            .unwrap();
        let q = c
            .convert_uv_from_step_to_nurbs(&k, Point2::new(a[1], h[1]))
            .unwrap();
        Extent2d::from_bounds(p.x.min(q.x), p.y.min(q.y), p.x.max(q.x), p.y.max(q.y))
    }

    /// The side of `dom` whose STEP angle (or height, `in_height`) is `value` along it.
    fn side_at(c: &Cone, dom: &Extent2d, in_height: bool, value: f64) -> BoxSide {
        let k = k();
        BoxSide::ALL
            .into_iter()
            .find(|s| {
                [0.0, 1.0].iter().all(|&t| {
                    let p = c
                        .convert_uv_from_nurbs_to_step(&k, s.point(dom, t))
                        .unwrap();
                    ((if in_height { p.y } else { p.x }) - value).abs() < 1e-9
                })
            })
            .unwrap()
    }

    fn holds(b: Bracket, x: f64) {
        assert!(b.lower <= x && x <= b.upper, "{b:?} does not hold {x}");
    }

    #[test]
    fn a_pole_is_a_point_and_the_equator_its_chord() {
        let s = sphere(2.0, [0.0, 90.0], [0.0, 90.0]);
        let dom = s.natural_uv_domain_with_nurb(&k()).unwrap();
        let d: Vec<Bracket> = BoxSide::ALL
            .iter()
            .map(|&side| s.side_diameter(&k(), &dom, side).unwrap())
            .collect();
        // one side is the pole: proved a point at rounding level
        assert_eq!(d.iter().filter(|b| b.upper < 1e-10).count(), 1);
        // the equator's quarter: its chord r sqrt2, which is its hull's diameter too
        let eq = d
            .iter()
            .filter(|b| (b.mid() - 2.0 * SQRT_2).abs() < 1e-9)
            .count();
        assert!(eq >= 1, "{d:?}");
        for b in &d {
            assert!(b.upper - b.lower < 1e-10, "{b:?}");
        }
    }

    #[test]
    fn a_half_circle_is_enclosed_between_its_chord_and_its_hull() {
        // the equator over a half turn: diameter 2r; the hull of its two quarter arcs is
        // sqrt5 r across
        let s = sphere(1.5, [0.0, 180.0], [0.0, 45.0]);
        let dom = s.natural_uv_domain_with_nurb(&k()).unwrap();
        let b = BoxSide::ALL
            .iter()
            .map(|&side| s.side_diameter(&k(), &dom, side).unwrap())
            .find(|b| (b.lower - 3.0).abs() < 1e-9)
            .unwrap();
        holds(b, 3.0);
        assert!((b.upper - 1.5 * 5f64.sqrt()).abs() < 1e-9, "{b:?}");
    }

    #[test]
    fn across_a_cylinder_round_and_along_its_axis() {
        let c = cylinder(2.0, [0.0, 180.0], [0.0, 5.0]);
        let dom = nurbs_box(&c, [0.0, 90.0], [0.0, 3.0]);
        let s = Surface::from(c.clone());
        // round: from angle 90 to 180, a quarter of radius 2
        let side = side_at(&c, &dom, false, 90.0);
        let b = s
            .length_across_side(&k(), &dom, side, 1e-8)
            .unwrap()
            .enclosure;
        holds(b, PI);
        // the leaves' share and the rounding's at part scale (coordinates about 3)
        assert!(b.upper - b.lower < 1e-6, "{b:?}");
        // along: from height 3 to 5
        let side = side_at(&c, &dom, true, 3.0);
        let b = s
            .length_across_side(&k(), &dom, side, 1e-10)
            .unwrap()
            .enclosure;
        holds(b, 2.0);
        assert!(b.upper - b.lower < 1e-10, "{b:?}");
        // the low height side is the domain's own end: nothing across
        let side = side_at(&c, &dom, true, 0.0);
        let b = s
            .length_across_side(&k(), &dom, side, 1e-10)
            .unwrap()
            .enclosure;
        assert_eq!((b.lower, b.upper), (0.0, 0.0));
    }

    #[test]
    fn a_coarser_ask_gives_a_wider_enclosure() {
        let c = cylinder(1.0, [0.0, 270.0], [0.0, 1.0]);
        let dom = nurbs_box(&c, [0.0, 90.0], [0.0, 1.0]);
        let s = Surface::from(c.clone());
        let side = side_at(&c, &dom, false, 90.0);
        let fine = s
            .length_across_side(&k(), &dom, side, 1e-8)
            .unwrap()
            .enclosure;
        let coarse = s
            .length_across_side(&k(), &dom, side, 1e-4)
            .unwrap()
            .enclosure;
        holds(fine, PI);
        holds(coarse, PI);
        assert!(coarse.upper - coarse.lower > fine.upper - fine.lower);
        assert!(s.length_across_side(&k(), &dom, side, 0.0).is_err());
    }

    #[test]
    fn a_box_past_the_knots_is_refused_not_clamped() {
        let c = cylinder(2.0, [0.0, 90.0], [0.0, 3.0]);
        let s = Surface::from(c.clone());
        let dom = s.natural_uv_domain_with_nurb(&k()).unwrap();
        let (lo, hi) = (dom.min(), dom.max());
        let past = Extent2d::from_bounds(lo.x, lo.y, hi.x + 1e-7, hi.y);
        assert!(s.side_diameter(&k(), &past, BoxSide::UMax).is_err());
        assert!(s.side_diameter(&k(), &past, BoxSide::VMin).is_err());
        assert!(s
            .length_across_side(&k(), &past, BoxSide::UMax, 1e-8)
            .is_err());
        assert!(s.side_diameter(&k(), &dom, BoxSide::UMax).is_ok());
    }

    #[test]
    fn a_form_moved_off_its_label_is_refused() {
        let mut c = cylinder(2.0, [0.0, 90.0], [0.0, 3.0]);
        let dom = nurbs_box(&c, [0.0, 90.0], [0.0, 3.0]);
        let shift = Axis2Placement::new(Vec3::new(1e-6, 0.0, 0.0), Vec3::X, Vec3::Y);
        c.rev.base.transform(&shift, None).unwrap();
        let s = Surface::from(c);
        assert!(s.side_diameter(&k(), &dom, BoxSide::UMin).is_err());
        assert!(s
            .length_across_side(&k(), &dom, BoxSide::UMin, 1e-9)
            .is_err());
    }
}
