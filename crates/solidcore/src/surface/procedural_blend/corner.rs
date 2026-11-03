//! A blend corner's face by its definition (#2488): the corner kinds the blend corner
//! builds ([`crate::blend::blend_corner`]) keep, beside the face's surface (their net), what
//! the surface was built from, with the contract of [`super::ProceduralBlend`] and of
//! [`super::exact::ExactRound`]: the definition, the net's certified distance from it (its
//! bound), re-certification against it, and a plain-data record
//! ([`super::record::CornerRecord`]).
//!
//! Each kind is defined by its construction:
//! - [`SweptDefinition`]: a Swept turn, the end section of two rounds revolved about the
//!   side edge, an exact torus (bound 0): the common face the rounds meet, the side edge's
//!   line, the balls' centres at the side vertex and their radius. Re-certified by its label
//!   (the net is a torus, its NURBS form on it, [`lies_on`]) and the torus derived from the
//!   definition being the net's, the definition on its common face, to the construction's
//!   tolerance.
//! - [`EndDefinition`]: an End's patch, its support face's surface (or that face's
//!   extension) bounded by the patch's loop (bound 0). Re-certified by the net lying on its
//!   support ([`lies_on`] on the support's analytic surface, or the same NURBS form).
//! - [`SetbackDefinition`]: one of a Setback's 2n fill patches, by the fill's definition
//!   ([`SetbackFill`]): its boundary and cross-boundary derivatives (the rounds at their end
//!   sections, the faces' planes the setback's edges lie on) and the fill rule
//!   ([`crate::blend::blend_setback::fill_hole`] at its order and tolerance). The net is the
//!   rule's patch (bound 0); its certificate is the rule's own: G1 (or G2) to the rounds
//!   and to the other patches exactly in homogeneous coordinates, a G1 fill certified convex,
//!   and the fill within the corner's half-spaces ([`crate::blend::blend_setback::containment`],
//!   certified by the convex hull). Re-certified by running the rule again from the
//!   definition: the fill and its certificates are made anew and the net must be its patch,
//!   number for number.
//! - [`MixedDefinition`]: one of a Mixed corner's two faces, by its rule: the bicubically
//!   blended Coons patch in R^4 (bilinear for chamfers) of its four sides and their ribbons
//!   ([`crate::blend::blend_corner::mixed::algebra::coons`]), the sides made from the two
//!   trimmed blends (the trimline and the blend's cross partial along it) and the conic
//!   inflection X-V in T. Its supports are the two blends and T. Re-certified by running
//!   the rule again on the sides (the net within the convex hull's distance of its patch,
//!   held to the construction's tolerance, its bound), the trimline ending on its blend
//!   and the conic lying in T.
//!
//! The corner face stays its own kind ([`crate::surface::surface::SurfaceKind`]: a torus, a
//! plane, a B-spline patch), so every match on that kind still sees it; its definition
//! rides on the surface and [`crate::surface::surface::Surface::procedural_blend`] is the one
//! way to it. Like an exact round's, a corner's definition is a set's: a piece, a copy or an
//! edit of the net's parametrization carries it unchanged; a motion moves it
//! ([`CornerDefinition::transform`]).
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion: the supports are
//! kept as surfaces without definitions of their own, walked one level.

use std::sync::Arc;

use crate::blend::blend_corner::mixed::algebra::{self, Crv, Side, V4};
use crate::blend::blend_setback::{containment, fill_hole, Face, Fill, Round};
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::tol::are_same;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::surface::implicit::{lies_on, AnalyticSupport};
use crate::surface::surface::Surface;

const OP_CREATE: &str = "surface::procedural_blend::corner::CornerDefinition::create";
const OP_RECERTIFY: &str = "surface::procedural_blend::corner::CornerDefinition::recertify";
const OP_TRANSFORM: &str = "surface::procedural_blend::corner::CornerDefinition::transform";

/// The kind of a blend corner's face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerKind {
    /// A Swept turn's torus.
    Swept,
    /// An End's patch on its support face.
    End,
    /// A Setback's fill patch.
    Setback,
    /// A Mixed corner's face.
    Mixed,
}

impl CornerKind {
    /// Its name, for a refusal.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Swept => "swept",
            Self::End => "end",
            Self::Setback => "setback",
            Self::Mixed => "mixed",
        }
    }
}

/// A Swept turn's definition: the end section of its two rounds (balls of radius `radius`
/// centred at `centres` at the side vertex) revolved about the side edge's line
/// (`axis_point`, the unit `axis`), the torus about the centres' common foot on the line,
/// its major radius their distance from it, its minor radius the balls'; the rounds' common
/// face, a plane, square to the line and tangent to the tube.
#[derive(Debug, Clone)]
pub struct SweptDefinition {
    /// The face both rounds meet (a plane).
    pub(crate) common: Box<Surface>,
    /// A point of the side edge's line.
    pub(crate) axis_point: Point3,
    /// The side edge's unit direction.
    pub(crate) axis: Vec3,
    /// The two rounds' balls' centres at the side vertex.
    pub(crate) centres: [Point3; 2],
    /// The balls' radius.
    pub(crate) radius: f64,
    /// The tolerance the construction decided the turn within (the rounds').
    pub(crate) tol: f64,
}

/// An End's definition: its support face's surface (the face the round ends on, or its
/// extension) and the patch's boundary, its loop's curves.
#[derive(Debug, Clone)]
pub struct EndDefinition {
    /// The support face's surface.
    pub(crate) support: Box<Surface>,
    /// The patch's loop's curves, as NURBS curves.
    pub(crate) boundary: Vec<BSplineCurve>,
}

/// A round at a Setback, as the fill reads it ([`Round`]): its surface, its rails'
/// direction, the rail parameter of its end section and the corner's side of it.
#[derive(Debug, Clone)]
pub struct SetbackRound {
    /// Its surface (the round's net, a support of the fill).
    pub(crate) surface: Box<Surface>,
    /// Its rails' direction.
    pub(crate) along: SurfParamType,
    /// The rail parameter of its end section.
    pub(crate) end: f64,
    /// Whether the corner lies at rail parameters above `end`.
    pub(crate) corner_above: bool,
}

/// A Setback's fill by its definition: the faces (planes, or beside a curved face the
/// quadrics they are, #2343), the rounds at their end sections,
/// the fill's order, the tolerance it matched them to, the corner's convexity, and the
/// certificate it was built with (the most it stands past a face).
#[derive(Debug, Clone)]
pub struct SetbackFill {
    /// The faces, normals out of the material: planes, or the quadrics they are.
    pub(crate) faces: Vec<Face>,
    /// The rounds, in the corner's order.
    pub(crate) rounds: Vec<SetbackRound>,
    /// The fill's order (1: G1, 2: G2).
    pub(crate) order: usize,
    /// The length the faces and the rounds' ends are matched to.
    pub(crate) tol: f64,
    /// Whether the corner's edges are convex.
    pub(crate) convex: bool,
    /// The certified containment: the most the fill stands past a face (at most `tol`).
    pub(crate) containment: f64,
}

/// One of a Setback's fill patches: the fill's definition (shared by its patches) and the
/// patch's index in it (setback patches `0 .. n`, interior parts `n .. 2 n`).
#[derive(Debug, Clone)]
pub struct SetbackDefinition {
    /// The fill's definition.
    pub(crate) fill: Arc<SetbackFill>,
    /// The patch's index.
    pub(crate) patch: usize,
}

/// One of a Mixed corner's faces by its rule: the Coons patch of its four sides (`c0`, `c1`,
/// `d0`, `d1`), bicubically blended (or bilinear, for chamfers), turned (`u`, `v` swapped)
/// where its normal must leave the material; the two trimmed blends and T, its supports; the
/// trimline's side, the blend it lies on and its uv on it; the conic X-V's side in T.
#[derive(Debug, Clone)]
pub struct MixedDefinition {
    /// The two blends B1 (convex) and B2 (concave).
    pub(crate) blends: [Box<Surface>; 2],
    /// T, the plane the blends' rails cross on.
    pub(crate) common: Box<Surface>,
    /// The four sides and their ribbons.
    pub(crate) sides: Box<[Side; 4]>,
    /// Whether the rule is the bilinear Coons patch (chamfers, G0).
    pub(crate) bilinear: bool,
    /// Whether the patch is turned.
    pub(crate) transposed: bool,
    /// The blend the trimline side lies on (0: B1, 1: B2).
    pub(crate) blend: usize,
    /// The trimline's side.
    pub(crate) trim_side: usize,
    /// The trimline's uv on its blend.
    pub(crate) trim_uv: Crv,
    /// The conic X-V's side, in T.
    pub(crate) conic_side: usize,
    /// The tolerance the construction matched its curves to.
    pub(crate) tol: f64,
}

/// A blend corner's face's definition.
#[derive(Debug, Clone)]
pub enum CornerDefinition {
    /// A Swept turn's torus.
    Swept(SweptDefinition),
    /// An End's patch.
    End(EndDefinition),
    /// A Setback's fill patch.
    Setback(SetbackDefinition),
    /// A Mixed corner's face.
    Mixed(MixedDefinition),
}

/// A support kept as its surface, any definition of its own dropped (definitions never
/// nest here).
fn bare(k: &Kernel, s: &Surface) -> Result<Box<Surface>> {
    let mut c = s.copy(k)?;
    c.set_exact_round(None);
    Ok(Box::new(c))
}

fn refuse(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP_RECERTIFY, reason)
}

/// The NURBS form of `s` ([`crate::blend::fillet_section::station::as_b_spline`]).
fn form_of(k: &Kernel, s: &Surface) -> Result<BSplineSurface> {
    crate::blend::fillet_section::station::as_b_spline(k, s)?
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| refuse("a surface has no NURBS form to check"))
}

/// Whether two NURBS forms are the same numbers: degrees, knots, points and weights.
fn same_form(a: &BSplineSurface, b: &BSplineSurface) -> bool {
    match (a.gw_nurb(), b.gw_nurb()) {
        (Some(a), Some(b)) => {
            a.p() == b.p()
                && a.q() == b.q()
                && a.u_knots() == b.u_knots()
                && a.v_knots() == b.v_knots()
                && a.rows().eq(b.rows())
        }
        _ => false,
    }
}

/// A certified distance between two NURBS forms that are one parametrization up to the
/// symmetries of the parameter square (`u`, `v` swapped, either reversed: the edits a face's
/// net takes, `SwapUV` and the reversals, which keep the set): over the orientations of `b`
/// with `a`'s degrees and knot vectors, the least of [`control_distance`]'s bounds: with the
/// same weights (bit for bit) the largest distance between their control points, the
/// difference of the two a convex combination of the control points' differences (the
/// weights positive), so no point of one is farther from the other's at the same
/// parameters. `None` where no orientation matches but for the control points and weights
/// (another parametrization, a piece).
fn net_distance(a: &BSplineSurface, b: &BSplineSurface) -> Option<f64> {
    let (a, b) = (a.gw_nurb()?, b.gw_nurb()?);
    let grid = |s: &crate::nurbs::srf_basic::Surface| -> Vec<Vec<crate::nurbs::math::CPoint>> {
        s.rows().map(<[_]>::to_vec).collect()
    };
    let reversed = |k: &[f64]| -> Option<Vec<f64>> {
        let (lo, hi) = (*k.first()?, *k.last()?);
        Some(k.iter().rev().map(|x| lo + hi - x).collect())
    };
    let (ga, gb) = (grid(a), grid(b));
    let mut best: Option<f64> = None;
    for swap in [false, true] {
        // b in a's (u, v): its rows, degrees and knots, swapped or not
        let (pts, p, q, uk, vk) = if swap {
            let cols = gb.first().map_or(0, Vec::len);
            let t: Vec<Vec<_>> = (0..cols)
                .map(|j| gb.iter().filter_map(|r| r.get(j).copied()).collect())
                .collect();
            (t, b.q(), b.p(), b.v_knots().to_vec(), b.u_knots().to_vec())
        } else {
            (
                gb.clone(),
                b.p(),
                b.q(),
                b.u_knots().to_vec(),
                b.v_knots().to_vec(),
            )
        };
        if p != a.p() || q != a.q() {
            continue;
        }
        for (rev_u, rev_v) in [(false, false), (true, false), (false, true), (true, true)] {
            let uk = if rev_u { reversed(&uk)? } else { uk.clone() };
            let vk = if rev_v { reversed(&vk)? } else { vk.clone() };
            if uk != a.u_knots() || vk != a.v_knots() {
                continue;
            }
            let mut rows = pts.clone();
            if rev_u {
                rows.reverse();
            }
            if rev_v {
                for r in &mut rows {
                    r.reverse();
                }
            }
            if let Some(d) = control_distance(&ga, &rows) {
                best = Some(best.map_or(d, |x: f64| x.min(d)));
            }
        }
    }
    best
}

/// The largest distance between two control nets of one shape and the same weights (bit
/// for bit, positive). Where their positive weights differ (a curved Setback's fill made
/// again after a move, its weights computed afresh, #2343), within a ratio `rho < 1`
/// (`|w_a / w_b - 1| <= rho`), the largest distance between their Euclidean points plus
/// `2 rho / (1 - rho)` times the radius R of b's points about their centroid: with the
/// patches' convex weights `l = B w / sum B w`, `S_a - S_b = sum l_a (P_a - P_b) +
/// sum (l_a - l_b) (P_b - c)`, and `l_a = l_b (1 + d) / (1 + dbar)` (`|d|, |dbar| <= rho`)
/// gives `sum |l_a - l_b| <= 2 rho / (1 - rho)`. `None` otherwise.
fn control_distance(
    a: &[Vec<crate::nurbs::math::CPoint>],
    b: &[Vec<crate::nurbs::math::CPoint>],
) -> Option<f64> {
    use crate::nurbs::math::NOW;
    if a.len() != b.len() {
        return None;
    }
    let weight = |w: f64| if w == NOW { 1.0 } else { w };
    let mut worst = 0.0_f64;
    let mut same = true;
    for (ra, rb) in a.iter().zip(b) {
        if ra.len() != rb.len() {
            return None;
        }
        for (pa, pb) in ra.iter().zip(rb) {
            let (wa, wb) = (weight(pa.w), weight(pb.w));
            if wa.is_nan() || wa <= 0.0 || wb.is_nan() || wb <= 0.0 {
                return None;
            }
            if pa.w.to_bits() != pb.w.to_bits() {
                same = false;
                continue;
            }
            let d = Vec3::new(pa.x - pb.x, pa.y - pb.y, pa.z - pb.z).length() / wa;
            worst = worst.max(d);
        }
    }
    if same {
        return Some(worst);
    }
    let euclid = |p: &crate::nurbs::math::CPoint| {
        let w = weight(p.w);
        Vec3::new(p.x / w, p.y / w, p.z / w)
    };
    let (mut far, mut rho, mut count) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut centre = Vec3::ZERO;
    for (ra, rb) in a.iter().zip(b) {
        for (pa, pb) in ra.iter().zip(rb) {
            far = far.max((euclid(pa) - euclid(pb)).length());
            rho = rho.max((weight(pa.w) / weight(pb.w) - 1.0).abs());
            centre += euclid(pb);
            count += 1.0;
        }
    }
    if rho.is_nan() || rho >= 1.0 || count == 0.0 {
        return None;
    }
    centre *= 1.0 / count;
    let mut radius = 0.0_f64;
    for rb in b {
        for pb in rb {
            radius = radius.max((euclid(pb) - centre).length());
        }
    }
    Some(far + 2.0 * rho / (1.0 - rho) * radius)
}

/// `|a x b| <= sin`, `a` and `b` unit: the two directions are parallel to within `sin`.
fn parallel(a: Vec3, b: Vec3, sin: f64) -> bool {
    a.cross(b).length() <= sin
}

impl SweptDefinition {
    /// The torus the definition gives: its centre (the centres' common foot on the side
    /// edge's line), its major radius (their distance from it) and its minor radius, the
    /// two centres' feet and distances agreeing within the tolerance.
    fn torus(&self) -> Option<(Point3, f64)> {
        let [c0, c1] = self.centres;
        let foot = |c: Point3| self.axis_point + self.axis * (c - self.axis_point).dot(self.axis);
        let (f0, f1) = (foot(c0), foot(c1));
        let (r0, r1) = ((c0 - f0).length(), (c1 - f1).length());
        (f0.distance(f1) <= self.tol && (r0 - r1).abs() <= self.tol).then_some((f0, r0))
    }

    fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        // the label: the net is a torus, its NURBS form on it
        let Some(AnalyticSupport::Torus {
            placement,
            major_radius,
            minor_radius,
        }) = AnalyticSupport::of(net)
        else {
            return Err(refuse("a swept turn's net is a torus"));
        };
        let on_torus = AnalyticSupport::Torus {
            placement,
            major_radius,
            minor_radius,
        }
        .implicit();
        if !lies_on(&form_of(k, net)?, &on_torus)? {
            return Err(refuse("a swept turn's net is off its torus"));
        }
        // the torus is the definition's: the end section revolved about the side edge
        let Some((centre, major)) = self.torus() else {
            return Err(refuse(
                "a swept turn's balls are not one end section about the side edge",
            ));
        };
        let (tol, r) = (self.tol, self.radius);
        let sin = tol / r;
        let net_axis = placement.x_axis().cross(placement.y_axis());
        // (the construction makes a horn torus of two radii within the tolerance exactly one)
        if placement.origin().distance(centre) > tol
            || !parallel(net_axis, self.axis, sin)
            || (major_radius - major).abs() > tol
            || (minor_radius - r).abs() > tol
        {
            return Err(refuse(
                "a swept turn's net is not the torus of its end section about its side edge",
            ));
        }
        // on its supports: the common face a plane square to the side edge, the balls on
        // it (the tube tangent to it)
        let planar = self.common.is_planar(k, tol)?;
        if !planar.is_planar || !parallel(planar.normal, self.axis, sin) {
            return Err(refuse(
                "a swept turn's common face is not a plane square to its side edge",
            ));
        }
        for c in self.centres {
            if ((c - planar.point).dot(planar.normal).abs() - r).abs() > tol {
                return Err(refuse("a swept turn's balls do not touch its common face"));
            }
        }
        Ok(0.0)
    }
}

impl EndDefinition {
    fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        let (form, support) = (form_of(k, net)?, form_of(k, &self.support)?);
        // the net is its support: both forms on one analytic surface, or one form
        let analytic = AnalyticSupport::of(net).or_else(|| AnalyticSupport::of(&self.support));
        let on = match analytic {
            Some(a) => {
                let a = a.implicit();
                lies_on(&form, &a)? && lies_on(&support, &a)?
            }
            // (the same NURBS form, in any orientation of its parameters)
            None => net_distance(&support, &form) == Some(0.0),
        };
        if !on {
            return Err(refuse("an end's patch is off its support face"));
        }
        Ok(0.0)
    }
}

impl SetbackFill {
    /// A fill's definition on copies of its rounds' surfaces.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an order other than 1 or 2, a tolerance not finite and positive,
    /// faces and rounds of different counts or fewer than three; those of copying.
    pub fn create(
        k: &Kernel,
        faces: &[Face],
        rounds: &[Round<'_>],
        order: usize,
        tol: f64,
        convex: bool,
        containment: f64,
    ) -> Result<Self> {
        if !(order == 1 || order == 2)
            || !(tol.is_finite() && tol > 0.0)
            || faces.len() != rounds.len()
            || faces.len() < 3
            || !containment.is_finite()
        {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a setback's fill is of order 1 or 2, with a positive tolerance, a round per face, three or more",
            ));
        }
        let rounds = rounds
            .iter()
            .map(|r| {
                Ok(SetbackRound {
                    surface: bare(k, r.surface)?,
                    along: r.along,
                    end: r.end,
                    corner_above: r.corner_above,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            faces: faces.to_vec(),
            rounds,
            order,
            tol,
            convex,
            containment,
        })
    }

    /// The fill the definition gives: the rule run on it ([`fill_hole`]), and its
    /// containment certified within the tolerance ([`containment`]).
    ///
    /// # Errors
    ///
    /// The rule's refusal; `InvalidInput` for a fill that stands past a face farther than
    /// the tolerance; those of the rule's evaluations.
    pub fn refill(&self, k: &Kernel) -> Result<Fill> {
        let rounds: Vec<Round<'_>> = self
            .rounds
            .iter()
            .map(|r| Round {
                surface: &r.surface,
                along: r.along,
                end: r.end,
                corner_above: r.corner_above,
            })
            .collect();
        let fill = fill_hole(k, &self.faces, &rounds, self.order, self.tol).map_err(|r| r.error)?;
        if containment(&fill, &self.faces, self.convex)? > self.tol {
            return Err(refuse("a setback's fill stands past a face it meets"));
        }
        Ok(fill)
    }

    /// Whether `fill` is this definition's patches, number for number.
    #[must_use]
    pub fn is_fill(&self, fill: &Fill, patches: &[BSplineSurface]) -> bool {
        fill.patches.len() == patches.len()
            && fill
                .patches
                .iter()
                .zip(patches)
                .all(|(a, b)| same_form(a, b))
    }
}

impl SetbackDefinition {
    /// The rule run again from the definition, and the net's certified distance from its
    /// patch ([`net_distance`]: 0 where it is the patch), held to the fill's tolerance.
    fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        let fill = self.fill.refill(k)?;
        let patch = fill
            .patches
            .get(self.patch)
            .ok_or_else(|| refuse("a setback's patch is not one of its fill's"))?;
        match net_distance(patch, &form_of(k, net)?) {
            Some(d) if d <= self.fill.tol => Ok(d),
            _ => Err(refuse("a setback's patch is not its fill's")),
        }
    }
}

impl MixedDefinition {
    /// The rule run on the sides: the patch.
    ///
    /// # Errors
    ///
    /// Those of the rule.
    pub fn patch(&self) -> Result<BSplineSurface> {
        let [c0, c1, d0, d1] = &*self.sides;
        let mut srf = if self.bilinear {
            algebra::coons_bilinear(c0, c1, d0, d1)?
        } else {
            algebra::coons(c0, c1, d0, d1)?
        };
        if self.transposed {
            srf = algebra::srf_transpose(&srf)?;
        }
        algebra::srf_to_bspline(&srf)
    }

    fn side(&self, i: usize) -> Result<&Side> {
        self.sides
            .get(i)
            .ok_or_else(|| refuse("a mixed face has four sides"))
    }

    fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        let tol = self.tol;
        // the rule
        let d = match net_distance(&self.patch()?, &form_of(k, net)?) {
            Some(d) if d <= tol => d,
            _ => return Err(refuse("a mixed face's net is not its rule's patch")),
        };
        // the trimline ends on its blend
        let blend = self
            .blends
            .get(self.blend)
            .ok_or_else(|| refuse("a mixed face's trimline is on one of its blends"))?;
        let trim = self.side(self.trim_side)?;
        for t in [0.0, 1.0] {
            let uv = algebra::eval(&self.trim_uv, t)?;
            let on =
                blend.evaluate_point(k, crate::math::Point2::new(uv[0] / uv[3], uv[1] / uv[3]))?;
            if on.distance(algebra::project(algebra::eval(&trim.c, t)?)) > tol {
                return Err(refuse("a mixed face's trimline is off its blend"));
            }
        }
        // the conic lies in T
        let planar = self.common.is_planar(k, tol)?;
        if !planar.is_planar {
            return Err(refuse("a mixed corner's T is a plane"));
        }
        for seg in &self.side(self.conic_side)?.c.seg {
            for q in seg {
                let p = algebra::project(*q);
                if (p - planar.point).dot(planar.normal).abs() > tol {
                    return Err(refuse("a mixed face's inflection is off T"));
                }
            }
        }
        Ok(d)
    }

    /// The rigid motion of every homogeneous point and derivative the sides hold:
    /// `(w x, w) -> (R (w x) + w o, w)`.
    fn transform(&mut self, rotate_n_move: &Axis2Placement) {
        let o = rotate_n_move.transform_point(Point3::default());
        let map = |q: &mut V4| {
            let r = rotate_n_move.transform_vector(Vec3::new(q[0], q[1], q[2]));
            *q = [r.x + q[3] * o.x, r.y + q[3] * o.y, r.z + q[3] * o.z, q[3]];
        };
        let crv = |c: &mut Crv| {
            for seg in &mut c.seg {
                seg.iter_mut().for_each(map);
            }
        };
        for s in self.sides.iter_mut() {
            crv(&mut s.c);
            crv(&mut s.dir);
            crv(&mut s.t);
            s.dir_val.iter_mut().for_each(map);
            s.dir_der.iter_mut().for_each(map);
            s.dir_k.iter_mut().for_each(map);
        }
    }
}

impl CornerDefinition {
    /// A Mixed face's definition on copies of its blends and T.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a side index past four, a blend past two, or a tolerance not
    /// finite and positive; those of copying.
    #[allow(clippy::too_many_arguments)]
    pub fn mixed(
        k: &Kernel,
        blends: [&Surface; 2],
        common: &Surface,
        sides: [Side; 4],
        (bilinear, transposed): (bool, bool),
        (blend, trim_side, trim_uv): (usize, usize, Crv),
        conic_side: usize,
        tol: f64,
    ) -> Result<Self> {
        if blend > 1 || trim_side > 3 || conic_side > 3 || !(tol.is_finite() && tol > 0.0) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a mixed face has four sides on two blends and a positive tolerance",
            ));
        }
        let [b1, b2] = blends;
        Ok(Self::Mixed(MixedDefinition {
            blends: [bare(k, b1)?, bare(k, b2)?],
            common: bare(k, common)?,
            sides: Box::new(sides),
            bilinear,
            transposed,
            blend,
            trim_side,
            trim_uv,
            conic_side,
            tol,
        }))
    }

    /// A Swept turn's definition on a copy of its common face.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a radius or tolerance not finite and positive, or an axis that is
    /// not a direction; those of copying the face.
    pub fn swept(
        k: &Kernel,
        common: &Surface,
        axis_point: Point3,
        axis: Vec3,
        centres: [Point3; 2],
        radius: f64,
        tol: f64,
    ) -> Result<Self> {
        let positive = |x: f64| x.is_finite() && x > 0.0;
        if !(positive(radius) && positive(tol)) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a swept turn's radius and tolerance are finite and positive",
            ));
        }
        let axis = axis.unitize()?;
        Ok(Self::Swept(SweptDefinition {
            common: bare(k, common)?,
            axis_point,
            axis,
            centres,
            radius,
            tol,
        }))
    }

    /// An End's definition on a copy of its support face's surface, bounded by `boundary`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for an empty boundary; those of copying the surface.
    pub fn end(k: &Kernel, support: &Surface, boundary: Vec<BSplineCurve>) -> Result<Self> {
        if boundary.is_empty() {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "an end's patch is bounded",
            ));
        }
        Ok(Self::End(EndDefinition {
            support: bare(k, support)?,
            boundary,
        }))
    }

    /// Patch `patch` of the Setback fill `fill`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a patch past the fill's 2 n.
    pub fn setback(fill: Arc<SetbackFill>, patch: usize) -> Result<Self> {
        if patch >= 2 * fill.rounds.len() {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "a setback's fill has two patches per round",
            ));
        }
        Ok(Self::Setback(SetbackDefinition { fill, patch }))
    }

    /// The kind.
    #[must_use]
    pub const fn kind(&self) -> CornerKind {
        match self {
            Self::Swept(_) => CornerKind::Swept,
            Self::End(_) => CornerKind::End,
            Self::Setback(_) => CornerKind::Setback,
            Self::Mixed(_) => CornerKind::Mixed,
        }
    }

    /// Whether the net is an analytic surface that is the definition's (an exact kind); a
    /// Setback's patch is the B-spline its rule makes.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Swept(_) | Self::End(_))
    }

    /// The net's certified distance from the definition: 0 for an exact kind (the net is
    /// the definition's surface); for a Setback's patch, the fill's tolerance, within which
    /// re-certification holds the net to the rule's patch made again (the net is that patch
    /// where it was built; moved or read back, it is within the rounding of making it again).
    #[must_use]
    pub fn bound(&self) -> f64 {
        match self {
            Self::Swept(_) | Self::End(_) => 0.0,
            Self::Setback(d) => d.fill.tol,
            Self::Mixed(d) => d.tol,
        }
    }

    /// The order the face meets its neighbours to (1: G1, 2: G2).
    #[must_use]
    pub fn order(&self) -> u32 {
        match self {
            Self::Swept(_) | Self::End(_) => 1,
            Self::Setback(d) => u32::try_from(d.fill.order).unwrap_or(u32::MAX),
            Self::Mixed(d) => u32::from(!d.bilinear),
        }
    }

    /// The `i`-th surface the face is built on: a Swept turn's common face, an End's
    /// support face.
    #[must_use]
    pub fn support(&self, i: usize) -> Option<&Surface> {
        match (self, i) {
            (Self::Swept(d), 0) => Some(&d.common),
            (Self::End(d), 0) => Some(&d.support),
            (Self::Setback(d), i) => d.fill.rounds.get(i).map(|r| &*r.surface),
            (Self::Mixed(d), 2) => Some(&d.common),
            (Self::Mixed(d), i) => d.blends.get(i).map(|b| &**b),
            _ => None,
        }
    }

    /// A Swept turn's definition.
    #[must_use]
    pub const fn as_swept(&self) -> Option<&SweptDefinition> {
        match self {
            Self::Swept(d) => Some(d),
            Self::End(_) | Self::Setback(_) | Self::Mixed(_) => None,
        }
    }

    /// An End's definition.
    #[must_use]
    pub const fn as_end(&self) -> Option<&EndDefinition> {
        match self {
            Self::End(d) => Some(d),
            Self::Swept(_) | Self::Setback(_) | Self::Mixed(_) => None,
        }
    }

    /// A Setback patch's definition.
    #[must_use]
    pub const fn as_setback(&self) -> Option<&SetbackDefinition> {
        match self {
            Self::Setback(d) => Some(d),
            Self::Swept(_) | Self::End(_) | Self::Mixed(_) => None,
        }
    }

    /// A Mixed face's definition.
    #[must_use]
    pub const fn as_mixed(&self) -> Option<&MixedDefinition> {
        match self {
            Self::Mixed(d) => Some(d),
            Self::Swept(_) | Self::End(_) | Self::Setback(_) => None,
        }
    }

    /// Re-certifies `net` against this definition: the distance proved (0 for an exact
    /// kind).
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming what fails; those of the checks' evaluations.
    pub fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        match self {
            Self::Swept(d) => d.recertify(k, net),
            Self::End(d) => d.recertify(k, net),
            Self::Setback(d) => d.recertify(k, net),
            Self::Mixed(d) => d.recertify(k, net),
        }
    }

    /// Moves the definition with its net (`Transform`): its surfaces, curves, points and
    /// directions by the same motion; a uniform scale scales its lengths with them.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a scale that is not uniform and positive (a corner's construction
    /// is not kept under it); those of the surfaces' and curves' transforms.
    pub fn transform(
        &mut self,
        k: &Kernel,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        depth: usize,
    ) -> Result<()> {
        let mut factor = 1.0;
        if let Some(sc) = scale {
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // refuses NaN too
            if !are_same(sc.x, sc.y) || !are_same(sc.x, sc.z) || !(sc.x > EFF_ZERO) {
                return Err(SolidError::invalid_input(
                    OP_TRANSFORM,
                    "a corner's face moves rigidly or by a uniform scale",
                ));
            }
            factor = sc.x;
        }
        let point = |p: Point3| rotate_n_move.transform_point(p * factor);
        match self {
            Self::Swept(d) => {
                d.common
                    .transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
                d.axis_point = point(d.axis_point);
                d.axis = rotate_n_move.transform_vector(d.axis).unitize()?;
                for c in &mut d.centres {
                    *c = point(*c);
                }
                d.radius *= factor;
                d.tol *= factor;
            }
            Self::End(d) => {
                d.support
                    .transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
                for c in &mut d.boundary {
                    c.transform(rotate_n_move, scale)?;
                }
            }
            Self::Setback(d) => {
                let fill = Arc::make_mut(&mut d.fill);
                for f in &mut fill.faces {
                    f.point = point(f.point);
                    f.normal = rotate_n_move.transform_vector(f.normal).unitize()?;
                    // a quadric's axis turns with it and its radius scales; a cone's slope,
                    // a ratio, and the side the material is on are kept (#2343)
                    if f.is_curved() {
                        f.axis = rotate_n_move.transform_vector(f.axis).unitize()?;
                        f.radius *= factor;
                    }
                }
                for r in &mut fill.rounds {
                    r.surface
                        .transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
                }
                fill.tol *= factor;
                fill.containment *= factor;
            }
            Self::Mixed(d) => {
                // (its ribbons' scalars are not kept under a scale)
                if scale.is_some() {
                    return Err(SolidError::invalid_input(
                        OP_TRANSFORM,
                        "a mixed corner's face moves rigidly",
                    ));
                }
                for b in &mut d.blends {
                    b.transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
                }
                d.common
                    .transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
                d.transform(rotate_n_move);
            }
        }
        Ok(())
    }
}
