//! The definition a certified offset sheet keeps: `S + d n` of its base, the bound its net is
//! certified within, and how the net's parameters reach the base's.
//!
//! | Rust                         | Role                                              |
//! |------------------------------|---------------------------------------------------|
//! | [`OffsetDefinition`]         | the offset sheet's definition: base, distance, bound, map |
//! | [`certified_sheet_surface`]  | the surface of a certified sheet, its definition kept |
//! | [`OffsetDefinition::evaluate_definition_partials`] | `S + d n` and its partials |
//! | [`OffsetDefinition::to_record`], [`OffsetDefinition::from_record`], [`OffsetDefinition::recertify`] | the record, and the net certified again on read |
//!
//! **The definition.** A certified sheet `G` (`offset::construction::sheets`) is a bicubic
//! B-spline within `bound` of `O(u, v) = S(u, v) + d n(u, v)` at the same parameters, `n`
//! the base's NURB normal `S_u x S_v / |S_u x S_v|` (Elber & Cohen 1991; the certificate
//! is the sheet's own). The net keeps `S`, `d` and `bound` as its definition
//! (`docs/definitions-and-certificates.md` §4.1): [`crate::surface::surface::p2::Surface::certified_bound`]
//! answers the bound, and [`OffsetDefinition::evaluate_definition`] the definition's point
//! at the net's parameters.
//!
//! **Parameters.** The base is kept as it was built and read through a map of the net's
//! parameters (an affine map per direction and a swap). Reparametrising the base itself
//! instead would flip its normal on every reversal and swap (`S_u x S_v` changes sign), so
//! `d` would have to change sign with it; the map keeps the base, its normal and `d` as
//! certified, and the net and the definition at the same parameters.
//!
//! **Evaluation.** The definition's partials are those of `O = S + d n` from the base's one
//! order higher, by Leibniz's rule on `N = S_u x S_v` and `L = |N|` (`n L = N`, `L^2 = N.N`;
//! the rational derivatives of Piegl & Tiller §4.5 with `L` for the weight), carried to the
//! net's parameters by the map.
//!
//! **Records.** A sheet's record keeps the base as stored, `d`, the bound and the map
//! ([`OffsetDefinition::to_record`]); read back, the net is certified against the definition
//! again before the face is an operand ([`OffsetDefinition::recertify`]), and a net not
//! within the bound refuses the face (`docs/definitions-and-certificates.md` §7 invariant 8).
//! The net is certified over its own domain, mapped to the base's parameters, and only where
//! each of its patches lies within one patch of the base; any other net is refused, never
//! certified by a guess.
//!
//! **Motions.** The definition follows a rigid motion and a uniform positive scale `f` of its
//! net: the base moves with it, `d` and the bound scale by `f`, and the bound takes in how
//! far the placement is from a rotation and the rounding of moving the base (the net's own
//! rounding is added by the surface that moves it, as a sweep's). A scale that is not
//! uniform leaves no offset of the moved base, and the net moves without its definition.
//! A placement is proper (its Z is `X x Y`), so a motion keeps the normal's sense.
//!
//! Deviations: no recursion; the base's own nesting is bounded by the surface's geometry
//! nesting check.

use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::blend::fillet_section::recertify::transform_rounding;
use crate::blend::fillet_section::station::as_b_spline;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::Point2;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::math::MAX_DERIVATIVE;
use crate::nurbs::srf_basic::Surface as Nurb;
use crate::offset::construction::sheets::{offset_net_bound, AxisMap};
use crate::surface::procedural_blend::convert::{surface_of, surface_record};
use crate::surface::procedural_blend::record::{OffsetDefinitionRecord, ParameterMapRecord};
use crate::surface::procedural_sweep::ProceduralSweep;
use crate::surface::surface::p2::{Surface, SurfaceKind};

const OP: &str = "surface::offset_definition";
/// How the net's parameters reach the base's: the base's parameter `i` is
/// `a[i] + b[i] t`, `t` the net's u for `i = 0` and its v for `i = 1`, or the other way
/// round where `swapped`.
#[derive(Debug, Clone, PartialEq)]
struct OffsetMap {
    a: [f64; 2],
    b: [f64; 2],
    swapped: bool,
}

impl Default for OffsetMap {
    fn default() -> Self {
        Self {
            a: [0.0, 0.0],
            b: [1.0, 1.0],
            swapped: false,
        }
    }
}

impl OffsetMap {
    /// The base's parameters at the net's `uv`.
    fn base_uv(&self, uv: Point2) -> Point2 {
        let (s, t) = if self.swapped {
            (uv.y, uv.x)
        } else {
            (uv.x, uv.y)
        };
        let [a0, a1] = self.a;
        let [b0, b1] = self.b;
        Point2::new(a0 + b0 * s, a1 + b1 * t)
    }

    /// The net `g` turned, exactly, so that its parameters reach the base's rising, and the
    /// map that then takes them there: its rows and columns exchanged where the map swaps,
    /// and a direction the map reverses (`b < 0`) reversed by negating its knots (`t -> -t`,
    /// exact) with its control points, so `a + b t = a + (-b)(-t)`. No knot is mapped here:
    /// the certificate maps the net's breakpoints and bounds the rounding of doing so
    /// ([`offset_net_bound`]).
    ///
    /// # Errors
    ///
    /// Those of making the surface.
    fn oriented(&self, g: &Nurb) -> Result<(Nurb, [AxisMap; 2])> {
        let (p, q) = g.degrees();
        let mut rows = g.cpts();
        let (mut ku, mut kv, mut du, mut dv) = (g.u_knots().to_vec(), g.v_knots().to_vec(), p, q);
        if self.swapped {
            let width = rows.first().map_or(0, Vec::len);
            rows = (0..width)
                .map(|j| rows.iter().filter_map(|r| r.get(j).copied()).collect())
                .collect();
            core::mem::swap(&mut ku, &mut kv);
            core::mem::swap(&mut du, &mut dv);
        }
        let negated = |k: Vec<f64>| k.into_iter().rev().map(|t| -t).collect::<Vec<f64>>();
        let [a0, a1] = self.a;
        let [b0, b1] = self.b;
        if b0 < 0.0 {
            rows.reverse();
            ku = negated(ku);
        }
        if b1 < 0.0 {
            for row in &mut rows {
                row.reverse();
            }
            kv = negated(kv);
        }
        Ok((
            Nurb::new(rows, du, dv, ku, kv)?,
            [(a0, b0.abs()), (a1, b1.abs())],
        ))
    }
}

/// How far `rotate_n_move` is from a rotation and a translation: `eps = 3 eta`, `eta` the
/// largest of `| |X|^2 - 1 |`, `| |Y|^2 - 1 |`, `|X . Y|`, enclosed with the rounding of
/// computing it, so the motion is `R + E` with `R` a rotation and `|E| <= eps`
/// ([`OffsetDefinition::transform`]).
///
/// # Errors
///
/// `InvalidInput` for a placement too far from a rotation (`eps >= 1`, NaN included).
fn motion_deviation(rotate_n_move: &Axis2Placement) -> Result<f64> {
    let (x, y) = (rotate_n_move.x_axis(), rotate_n_move.y_axis());
    // eta enclosed with its rounding (rounded intervals), so eps holds the exact one
    let ivl = |v: Vec3| [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)];
    let dot = |[a0, a1, a2]: [Ivl; 3], [b0, b1, b2]: [Ivl; 3]| a0 * b0 + a1 * b1 + a2 * b2;
    let (xi, yi, one) = (ivl(x), ivl(y), Ivl::from(1.0));
    let eta = (dot(xi, xi) - one)
        .mag()
        .max((dot(yi, yi) - one).mag())
        .max(dot(xi, yi).mag());
    let eps = round_up(3.0 * eta);
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
    if !(eps < 1.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the placement is a rotation and a translation",
        ));
    }
    Ok(eps)
}

/// The bound of a net within `bound` of an offset by `distance` after both move by
/// `rotate_n_move` after the uniform scale `f` ([`OffsetDefinition::transform`]):
/// `f (1 + eps) bound + f |d| 3 eps / (1 - eps) + rounding`, rounded up, `eps` the
/// placement's distance from a rotation ([`motion_deviation`]; the image of the normal is
/// within `eps` of the rotated one, the moved base's unit normal within `2 eps / (1 - eps)`
/// of it) and `rounding` that of moving what was moved
/// ([`crate::blend::fillet_section::recertify::transform_rounding`]).
///
/// # Errors
///
/// Those of [`motion_deviation`]; `InvalidInput` for a bound that is not finite.
pub(crate) fn moved_offset_bound(
    rotate_n_move: &Axis2Placement,
    f: f64,
    bound: f64,
    distance: f64,
    rounding: f64,
) -> Result<f64> {
    let eps = motion_deviation(rotate_n_move)?;
    let bound =
        round_up(f * bound * (1.0 + eps) + f * distance.abs() * 3.0 * eps / (1.0 - eps) + rounding);
    if !bound.is_finite() {
        return Err(SolidError::invalid_input(OP, "the moved bound is finite"));
    }
    Ok(bound)
}

/// An offset sheet's definition: the base `S`, the distance `d` along its NURB normal, and
/// the certified bound of the net from `S + d n` (module docs).
#[derive(Debug, Clone)]
pub struct OffsetDefinition {
    /// The surface the offset is of (the sub-net the sheet was certified on).
    pub(crate) base: Surface,
    /// The signed distance along the base's NURB normal.
    distance: f64,
    /// The certified bound of the net from the definition at the same parameters.
    bound: f64,
    /// How the net's parameters reach the base's.
    map: OffsetMap,
}

impl OffsetDefinition {
    /// The offset of `base` by `distance`, its net certified within `bound`, at the base's
    /// own parameters.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a distance not finite or a bound not finite and non-negative.
    pub fn new(base: Surface, distance: f64, bound: f64) -> Result<Self> {
        if !distance.is_finite() {
            return Err(SolidError::invalid_input(OP, "the distance is finite"));
        }
        if !(bound.is_finite() && bound >= 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "the bound is finite and non-negative",
            ));
        }
        Ok(Self {
            base,
            distance,
            bound,
            map: OffsetMap::default(),
        })
    }

    /// The base.
    #[must_use]
    pub const fn base(&self) -> &Surface {
        &self.base
    }

    /// The signed distance along the base's NURB normal.
    #[must_use]
    pub const fn distance(&self) -> f64 {
        self.distance
    }

    /// The net's certified distance from the definition.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        self.bound
    }

    /// The definition's point at the net's `uv`: `S + d n` at the base's parameters the map
    /// gives ([`OffsetDefinition::evaluate_definition_partials`]).
    ///
    /// # Errors
    ///
    /// Those of [`OffsetDefinition::evaluate_definition_partials`].
    pub fn evaluate_definition(&self, k: &Kernel, uv: Point2) -> Result<Point3> {
        self.evaluate_definition_partials(k, uv, 0, 0)?
            .first()
            .and_then(|row| row.first())
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "the definition gave no point"))
    }

    /// The definition's point and partials at the net's `uv`, `D[i][j]` taken `i` times in
    /// the net's u and `j` times in its v, `i <= highest_u_deriv`, `j <= highest_v_deriv`
    /// (each at most [`MAX_PARTIAL_ORDER`]): those of `O = S + d n` at the base's
    /// parameters ([`offset_partials`], from the base's partials one order higher), carried
    /// to the net's by the map (an affine map per direction scales the `i`-th partial by
    /// `b^i`; a swap exchanges the orders).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an order past [`MAX_PARTIAL_ORDER`]; `Degenerate` where the base's
    /// normal vanishes; those of the base's evaluation.
    pub fn evaluate_definition_partials(
        &self,
        k: &Kernel,
        uv: Point2,
        highest_u_deriv: usize,
        highest_v_deriv: usize,
    ) -> Result<Vec<Vec<Vec3>>> {
        if highest_u_deriv > MAX_PARTIAL_ORDER || highest_v_deriv > MAX_PARTIAL_ORDER {
            return Err(SolidError::invalid_input(
                OP,
                "an offset's definition gives partials to order three",
            ));
        }
        let swapped = self.map.swapped;
        let (pu, pv) = if swapped {
            (highest_v_deriv, highest_u_deriv)
        } else {
            (highest_u_deriv, highest_v_deriv)
        };
        // (a rational base's partials do not vanish past its degrees: every order is the
        // base's own, rational, evaluation)
        let base = self.base.evaluate(
            k,
            self.map.base_uv(uv),
            pu + 1,
            pv + 1,
            EvaluateFlags::default(),
        )?;
        let o = offset_partials(&base, self.distance, (pu, pv))?;
        let [b0, b1] = self.map.b;
        let power = |x: f64, n: usize| (0..n).fold(1.0, |acc, _| acc * x);
        (0..=highest_u_deriv)
            .map(|i| {
                (0..=highest_v_deriv)
                    .map(|j| {
                        let (p, q) = if swapped { (j, i) } else { (i, j) };
                        o.get(p * (pv + 1) + q)
                            .map(|x| *x * (power(b0, p) * power(b1, q)))
                            .ok_or_else(|| SolidError::invalid_input(OP, "a missing partial"))
                    })
                    .collect()
            })
            .collect()
    }

    /// Whether the definition follows a motion of its net by `scale`: under a rigid motion
    /// and a uniform positive scale only, the rule a sweep's follows
    /// ([`ProceduralSweep::follows`]).
    #[must_use]
    pub fn follows(scale: Option<Vec3>) -> bool {
        ProceduralSweep::follows(scale)
    }

    /// Moves the definition with its net by `rotate_n_move` after the scale `scale` (one it
    /// [`OffsetDefinition::follows`]). With `f` the scale and `eps = 3 eta` (`eta` the
    /// largest of `| |X|^2 - 1 |`, `| |Y|^2 - 1 |`, `|X . Y|`, enclosed with the rounding of
    /// computing it), the motion is `f (R + E) + t`,
    /// `R` a rotation and `|E| <= eps`; the moved net is then within
    /// `f (1 + eps) bound + f |d| 3 eps / (1 - eps)` of the moved base's offset by `f d`
    /// (the image of `n` is within `eps` of `R n`, the moved base's unit normal within
    /// `2 eps / (1 - eps)` of it), to which the rounding of moving a B-spline base adds
    /// its own ([`transform_rounding`]).
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a scale it does not follow or a placement too far from a rotation
    /// (`eps >= 1`); those of the base's motion.
    pub(crate) fn transform(
        &mut self,
        k: &Kernel,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        depth: usize,
    ) -> Result<()> {
        if !Self::follows(scale) {
            return Err(SolidError::invalid_input(
                OP,
                "an offset's definition moves rigidly or by a uniform scale",
            ));
        }
        let f = scale.map_or(1.0, |s| s.x);
        // (a placement too far from a rotation refuses before the base moves)
        motion_deviation(rotate_n_move)?;
        let mut base = self.base.clone();
        base.transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
        let rounding = match base.kind() {
            SurfaceKind::BSpline(net) => transform_rounding(net)?,
            _ => 0.0,
        };
        let bound = moved_offset_bound(rotate_n_move, f, self.bound, self.distance, rounding)?;
        self.base = base;
        self.distance *= f;
        self.bound = bound;
        Ok(())
    }

    /// Adds the rounding of moving the net `net` (already moved) to the bound, as a moved
    /// sweep's ([`ProceduralSweep::add_motion_rounding`]).
    ///
    /// # Errors
    ///
    /// Those of reading the net.
    pub(crate) fn add_motion_rounding(&mut self, net: &BSplineSurface) -> Result<()> {
        self.bound = round_up(self.bound + transform_rounding(net)?);
        Ok(())
    }

    /// The definition as its record: the base's NURBS form as stored, the distance, the bound
    /// and the map.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a base without a NURBS form; those of making it and of reading it.
    pub fn to_record(&self, k: &Kernel) -> Result<OffsetDefinitionRecord> {
        let base = as_b_spline(k, &self.base)?
            .ok_or_else(|| SolidError::invalid_input(OP, "an offset's base has a NURBS form"))?;
        Ok(OffsetDefinitionRecord {
            base: surface_record(&base)?,
            distance: self.distance,
            bound: self.bound,
            map: ParameterMapRecord {
                a: self.map.a,
                b: self.map.b,
                swapped: self.map.swapped,
            },
        })
    }

    /// The definition a record holds, exactly: the base as its stored numbers, the distance,
    /// the bound and the map. Not checked against a net: [`OffsetDefinition::recertify`]
    /// does that.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a record its structural check refuses
    /// ([`OffsetDefinitionRecord::validate`]) or a map that is singular; those of the base's
    /// NURBS and of [`OffsetDefinition::new`].
    pub fn from_record(_k: &Kernel, rec: &OffsetDefinitionRecord) -> Result<Self> {
        rec.validate()
            .map_err(|e| SolidError::invalid_input(OP, e.reason))?;
        if rec.distance == 0.0 {
            return Err(SolidError::invalid_input(
                OP,
                "an offset by zero is its base",
            ));
        }
        if rec.map.b.contains(&0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "an offset's map is not singular",
            ));
        }
        let base = Surface::from(surface_of(&rec.base)?);
        let mut def = Self::new(base, rec.distance, rec.bound)?;
        def.map = OffsetMap {
            a: rec.map.a,
            b: rec.map.b,
            swapped: rec.map.swapped,
        };
        Ok(def)
    }

    /// The distance the net `net` (a net as written or read back) is proved within of this
    /// definition: the bound of `net` over its own domain, whatever piece or basis of the certified
    /// net it is (a face split, knots inserted, the net moved), mapped to the base's parameters, from
    /// `S + d n` by the sheet's own certificate
    /// ([`crate::offset::construction::sheets::offset_net_bound`], Elber & Cohen on every
    /// patch). The caller compares it with the stored bound: a writer stores this value, and a
    /// reader refuses a net proved past the stored bound.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a net or base without a NURBS form; those of the certificate (a rational
    /// net, a net past the base's domain, a cancel).
    pub fn recertify(&self, k: &Kernel, net: &BSplineSurface) -> Result<f64> {
        proved_net_bound(k, &self.base, net, &self.map, self.distance)
    }

    /// The net's parameter in `u` (or v) became `w'` with `w = alpha + beta w'` (a reversal
    /// or reparametrisation of the net): the map follows.
    pub(crate) fn reparametrize_map(&mut self, u: bool, alpha: f64, beta: f64) {
        // the net's u reaches the base's first parameter unless swapped
        let idx = usize::from(u == self.map.swapped);
        if let (Some(a), Some(&b)) = (self.map.a.get_mut(idx), self.map.b.get(idx)) {
            *a += b * alpha;
        }
        if let Some(b) = self.map.b.get_mut(idx) {
            *b *= beta;
        }
    }

    /// The net's u and v swapped: the map follows.
    pub(crate) fn swap_map(&mut self) {
        self.map.swapped = !self.map.swapped;
    }
}

/// The distance `net` is within of the offset by `d` of `base`, under `map`: proved from the
/// stored numbers alone ([`offset_net_bound`]). The writer stores this value and the reader proves
/// it again from the same numbers, so the two are equal.
///
/// # Errors
///
/// `InvalidInput` for a base or a net without a NURBS form, and those of the certificate.
fn proved_net_bound(
    k: &Kernel,
    base: &Surface,
    net: &BSplineSurface,
    map: &OffsetMap,
    d: f64,
) -> Result<f64> {
    let no_form = || SolidError::invalid_input(OP, "a net without a NURBS form");
    let base = as_b_spline(k, base)?
        .ok_or_else(|| SolidError::invalid_input(OP, "an offset's base has a NURBS form"))?;
    let s = base.gw_nurb().ok_or_else(no_form)?;
    let (g, oriented) = map.oriented(net.gw_nurb().ok_or_else(no_form)?)?;
    offset_net_bound(k, s, &g, oriented, d)
}

/// The bound a fresh certified sheet is stored with: the distance its net `net` proves from the
/// offset of `base` by `d`, at the base's own parameters (the identity map).
///
/// # Errors
///
/// Those of [`proved_net_bound`].
pub(crate) fn sheet_bound(k: &Kernel, base: &Surface, net: &BSplineSurface, d: f64) -> Result<f64> {
    proved_net_bound(k, base, net, &OffsetMap::default(), d)
}

/// The most partials in either parameter [`OffsetDefinition::evaluate_definition_partials`]
/// gives: the base's partials one order higher reach the B-spline evaluation's cap
/// ([`MAX_DERIVATIVE`]); three, the order an offset's normal derivative needs.
pub const MAX_PARTIAL_ORDER: usize = MAX_DERIVATIVE - 1;

/// `C(n, k)`, `k <= n <= MAX_PARTIAL_ORDER + 1`: exact as a double.
fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| {
        #[allow(clippy::cast_precision_loss)] // n and i are at most MAX_PARTIAL_ORDER + 1
        let (num, den) = ((n - i) as f64, (i + 1) as f64);
        acc * num / den
    })
}

/// The partials of `O = S + d n` at one point, `n = N / L`, `N = S_u x S_v`, `L = |N|`,
/// to orders `(pu, pv)`, flat with `[p][q]` at `p (pv + 1) + q`, from the base's partials
/// `s[i][j]` to orders `(pu + 1, pv + 1)` (a rational base's, by the rational rule of
/// Piegl & Tiller A4.4, from the base's evaluation). By Leibniz's rule in increasing (p, q):
/// `N^(p,q) = sum C(p,a) C(q,c) S^(a+1,c) x S^(p-a,q-c+1)`; `L^2 = N.N` gives
/// `2 L L^(p,q) = (N.N)^(p,q) - sum' C C L^(a,c) L^(p-a,q-c)` (`sum'` past both ends) and
/// `n L = N` gives `L n^(p,q) = N^(p,q) - sum C C L^(a,c) n^(p-a,q-c)` over `(a, c) != 0`
/// (the rational derivatives of Piegl & Tiller, *The NURBS Book* §4.5, A4.4, with `L`
/// for the weight).
///
/// # Errors
///
/// `InvalidInput` for missing partials; `Degenerate` where `N` vanishes (or is not a
/// number).
fn offset_partials(s: &[Vec<Vec3>], d: f64, (pu, pv): (usize, usize)) -> Result<Vec<Vec3>> {
    let short = || SolidError::invalid_input(OP, "the base gave too few partials");
    let sd = |i: usize, j: usize| s.get(i).and_then(|r| r.get(j)).copied().ok_or_else(short);
    let at = |v: &[Vec3], p: usize, q: usize| v.get(p * (pv + 1) + q).copied().ok_or_else(short);
    let at1 = |v: &[f64], p: usize, q: usize| v.get(p * (pv + 1) + q).copied().ok_or_else(short);
    let mut nn: Vec<Vec3> = Vec::new();
    let mut ll: Vec<f64> = Vec::new();
    let mut un: Vec<Vec3> = Vec::new();
    let mut out: Vec<Vec3> = Vec::new();
    let mut l0 = 0.0;
    for p in 0..=pu {
        for q in 0..=pv {
            let mut n_pq = Vec3::ZERO;
            for a in 0..=p {
                for c in 0..=q {
                    let w = binomial(p, a) * binomial(q, c);
                    n_pq += sd(a + 1, c)?.cross(sd(p - a, q - c + 1)?) * w;
                }
            }
            nn.push(n_pq);
            // (N.N)^(p,q) and the sum over the lower partials of L
            let mut m = 0.0;
            let mut l_sum = 0.0;
            for a in 0..=p {
                for c in 0..=q {
                    let w = binomial(p, a) * binomial(q, c);
                    m += w * at(&nn, a, c)?.dot(at(&nn, p - a, q - c)?);
                    if (a, c) != (0, 0) && (a, c) != (p, q) {
                        l_sum += w * at1(&ll, a, c)? * at1(&ll, p - a, q - c)?;
                    }
                }
            }
            if (p, q) == (0, 0) {
                l0 = m.sqrt();
                #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
                if !(l0 > 0.0) {
                    return Err(SolidError::Degenerate {
                        operation: OP,
                        entity: None,
                    });
                }
                ll.push(l0);
            } else {
                ll.push((m - l_sum) / (2.0 * l0));
            }
            // the sum over the lower partials of n, L^(p,q) now among them
            let mut n_sum = Vec3::ZERO;
            for a in 0..=p {
                for c in 0..=q {
                    if (a, c) != (0, 0) {
                        let w = binomial(p, a) * binomial(q, c);
                        n_sum += at(&un, p - a, q - c)? * (w * at1(&ll, a, c)?);
                    }
                }
            }
            let n_unit = (n_pq - n_sum) / l0;
            un.push(n_unit);
            out.push(sd(p, q)? + n_unit * d);
        }
    }
    Ok(out)
}

/// The surface of a certified sheet: `net` (the bicubic `G` over `base`'s parameters), its
/// definition the offset of `base` by `distance` within `bound` kept
/// ([`crate::surface::surface::p2::Surface::offset_definition`]).
///
/// # Errors
///
/// Those of [`OffsetDefinition::new`].
pub fn certified_sheet_surface(
    net: Nurb,
    base: Surface,
    distance: f64,
    bound: f64,
) -> Result<Surface> {
    let definition = OffsetDefinition::new(base, distance, bound)?;
    let mut surface = Surface::from(BSplineSurface::from_nurb(net, false));
    surface.set_offset_definition(definition);
    Ok(surface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::offset::construction::sheets::certified_offset;
    use crate::surface::procedural_blend::convert::{
        definition_surface, face_definition, surface_of, surface_record,
    };
    use crate::surface::procedural_blend::record::FaceDefinition;

    /// The quadratic Bezier `(-10, 0), (0, 10), (10, 0)` in xy extruded 20 along z.
    fn parabola() -> Nurb {
        let net = [(-10.0, 0.0), (0.0, 10.0), (10.0, 0.0)]
            .iter()
            .map(|&(x, y)| vec![CPoint::new(x, y, 0.0, NOW), CPoint::new(x, y, 20.0, NOW)])
            .collect();
        Nurb::new(
            net,
            2,
            1,
            vec![0., 0., 0., 1., 1., 1.],
            vec![0., 0., 1., 1.],
        )
        .unwrap()
    }

    /// The parabola's certified offset by 2, wrapped with its definition, and its bound.
    fn sheet(k: &Kernel) -> (Surface, f64) {
        let base = parabola();
        let made = certified_offset(k, &base, 2.0, 1e-6).unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let net = BSplineSurface::from_nurb(made.surface.clone(), false);
        let bound = sheet_bound(k, &base, &net, 2.0).unwrap();
        let s = certified_sheet_surface(made.surface, base, 2.0, bound).unwrap();
        (s, bound)
    }

    /// 35 parameters off any grid the certificate used: the largest `|net - definition|`.
    fn worst(k: &Kernel, s: &Surface) -> f64 {
        let def = s.offset_definition().unwrap();
        let mut most = 0.0_f64;
        for i in 0..35 {
            let t = f64::from(i);
            let uv = Point2::new((0.0137 + t * 0.0281) % 1.0, (0.311 + t * 0.6180) % 1.0);
            let net = s.evaluate(k, uv, 0, 0, EvaluateFlags::default()).unwrap()[0][0];
            let on = def.evaluate_definition(k, uv).unwrap();
            most = most.max((net - on).length());
        }
        most
    }

    /// The net as a reader builds it from the face's stored numbers, and the definition the
    /// writer records for `s`.
    fn written(k: &Kernel, s: &Surface) -> (FaceDefinition, Surface) {
        let SurfaceKind::BSpline(net) = s.kind() else {
            panic!("a certified sheet is a B-spline net");
        };
        let rec = face_definition(k, s)
            .unwrap()
            .expect("a certified sheet's definition is recorded");
        let json = serde_json::to_string(&rec).unwrap();
        let rec = serde_json::from_str(&json).unwrap();
        (
            rec,
            Surface::from(surface_of(&surface_record(net).unwrap()).unwrap()),
        )
    }

    #[test]
    fn a_certified_sheet_keeps_its_offset_definition() {
        let k = Kernel::new(&KernelOptions::default());
        let (s, bound) = sheet(&k);
        assert!(bound > 0.0);
        assert_eq!(s.certified_bound(), bound);
        assert!(s.procedural_definition().is_some());
        assert!(s.procedural_blend().is_none());
        let e = worst(&k, &s);
        assert!(e <= bound, "{e} past {bound}");
        // the definition is the offset: 2 from the base along its normal
        let def = s.offset_definition().unwrap();
        let uv = Point2::new(0.5, 0.5);
        let p = def
            .base()
            .evaluate(&k, uv, 0, 0, EvaluateFlags::default())
            .unwrap()[0][0];
        let q = def.evaluate_definition(&k, uv).unwrap();
        assert!(((q - p).length() - 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_read_sheet_is_certified_again_and_a_moved_one_is_refused() {
        let k = Kernel::new(&KernelOptions::default());
        let (s, bound) = sheet(&k);
        let (rec, net) = written(&k, &s);
        let read = definition_surface(&k, &rec, &net).unwrap().unwrap();
        assert_eq!(read.certified_bound(), bound);
        assert!(worst(&k, &read) <= bound);
        // one control point moved by 1e-3 is past the bound: refused, not kept
        let SurfaceKind::BSpline(net_nurb) = net.kind() else {
            panic!("a B-spline net");
        };
        let mut moved = surface_record(net_nurb).unwrap();
        moved.points[0][2] += 1e-3;
        let moved = Surface::from(surface_of(&moved).unwrap());
        let FaceDefinition::Offset(stored) = &rec else {
            panic!("an offset");
        };
        let bound = stored.bound;
        assert!(
            matches!(definition_surface(&k, &rec, &moved), Ok(Err(eps)) if eps > bound),
            "a moved net is not certified within the stored bound"
        );
        // the distance changed by 1e-3: refused too
        let mut shifted = rec.clone();
        if let FaceDefinition::Offset(o) = &mut shifted {
            o.distance += 1e-3;
        }
        assert!(
            matches!(definition_surface(&k, &shifted, &net), Ok(Err(eps)) if eps > bound),
            "a changed distance is not certified within the stored bound"
        );
    }

    /// A base with an interior knot at 1/3 and the sheet reversed in u (map `a = 1, b = -1`): the
    /// writer proves the bound from the reversed net and map it writes, and the reader proves the
    /// same, so the sheet goes round.
    #[test]
    fn a_reversed_sheet_on_a_base_knotted_at_a_third_goes_round() {
        use crate::nurbs::b_spline_surface::p1::SurfParamType;
        use crate::nurbs::srf_basic::Direction;
        use crate::nurbs::srf_tool::p1::insert_knot;
        let k = Kernel::new(&KernelOptions::default());
        let base = insert_knot(&parabola(), 1.0 / 3.0, 1, Direction::U).unwrap();
        let made = certified_offset(&k, &base, 2.0, 1e-6).unwrap();
        let base = Surface::from(BSplineSurface::from_nurb(base, false));
        let net = BSplineSurface::from_nurb(made.surface.clone(), false);
        let bound = sheet_bound(&k, &base, &net, 2.0).unwrap();
        let mut s = certified_sheet_surface(made.surface, base, 2.0, bound).unwrap();
        s.reverse(&k, SurfParamType::U).unwrap();
        let (rec, net) = written(&k, &s);
        let FaceDefinition::Offset(stored) = &rec else {
            panic!("an offset");
        };
        assert_eq!(stored.map.b[0], -1.0);
        let read = definition_surface(&k, &rec, &net).unwrap().unwrap();
        assert_eq!(read.certified_bound(), stored.bound);
        assert!(worst(&k, &read) <= stored.bound);
    }

    #[test]
    fn the_definition_follows_a_motion_and_a_uniform_scale() {
        let k = Kernel::new(&KernelOptions::default());
        let (mut s, bound) = sheet(&k);
        let c = core::f64::consts::FRAC_1_SQRT_2;
        let place = Axis2Placement::new(
            Point3::new(3.0, -4.0, 5.0),
            Vec3::new(c, c, 0.0),
            Vec3::new(-c, c, 0.0),
        );
        s.transform(&k, &place, Some(Vec3::new(2.0, 2.0, 2.0)))
            .unwrap();
        let def = s.offset_definition().unwrap();
        assert_eq!(def.distance(), 4.0);
        let b = s.certified_bound();
        assert!(b >= 2.0 * bound, "{b} vs {bound}");
        let e = worst(&k, &s);
        assert!(e <= b, "{e} past {b}");
    }

    #[test]
    fn a_non_uniform_scale_drops_the_definition() {
        let k = Kernel::new(&KernelOptions::default());
        let (mut s, _) = sheet(&k);
        s.transform(
            &k,
            &Axis2Placement::IDENTITY,
            Some(Vec3::new(1.0, 2.0, 1.0)),
        )
        .unwrap();
        assert!(s.offset_definition().is_none());
        assert_eq!(s.certified_bound(), 0.0);
    }

    #[test]
    fn an_offset_by_zero_is_refused_when_read() {
        let k = Kernel::new(&KernelOptions::default());
        let (s, _) = sheet(&k);
        let (mut rec, net) = written(&k, &s);
        if let FaceDefinition::Offset(o) = &mut rec {
            o.distance = 0.0;
        }
        assert!(definition_surface(&k, &rec, &net).is_err());
    }

    /// A rational net (unit weights) is refused by the certificate for its own reason, passed on
    /// typed, not reported as a bound it misses.
    #[test]
    fn a_rational_net_is_refused_for_its_reason() {
        let k = Kernel::new(&KernelOptions::default());
        let (s, _) = sheet(&k);
        let (rec, net) = written(&k, &s);
        let SurfaceKind::BSpline(net_nurb) = net.kind() else {
            panic!("a B-spline net");
        };
        let g = net_nurb.gw_nurb().unwrap();
        let rows: Vec<Vec<CPoint>> = g
            .cpts()
            .iter()
            .map(|r| r.iter().map(|p| CPoint::new(p.x, p.y, p.z, 1.0)).collect())
            .collect();
        let (du, dv) = g.degrees();
        let rational = Nurb::new(rows, du, dv, g.u_knots().to_vec(), g.v_knots().to_vec()).unwrap();
        let rational = Surface::from(BSplineSurface::from_nurb(rational, false));
        let error = definition_surface(&k, &rec, &rational)
            .unwrap_err()
            .to_string();
        assert!(error.contains("a net is a bicubic spline"), "{error}");
    }
}
