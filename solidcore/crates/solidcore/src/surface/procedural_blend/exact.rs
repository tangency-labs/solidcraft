//! A round whose net is its definition (#2485): a round of an exact class (the cylinder on
//! a straight spine, the section turned about a common axis, a piece of the ball, the
//! rational surface on a conic spine) keeps its exact surface as its net, and beside it
//! the definition it was built from: the two supports the ball rolls on, the ball's
//! radius, the section (the ball's circle, order 1) and its leader. The net is S, so the
//! stored bound is 0, and it is re-certified by an identity, not a distance: the supports
//! classified again give the same class, and the net's NURBS form lies on that class's
//! surface, derived from the supports alone, to the rounding of checking it
//! ([`crate::surface::implicit::check_exact_net`]): an analytic surface, or for a rational
//! spine the canal surface about the round's centre line, which lies on both supports'
//! offsets.
//!
//! The round stays its kind ([`crate::surface::surface::SurfaceKind`]: a cone, surface of
//! revolution, sphere, ball patch, or the rational classes' B-spline), so every match on
//! that kind still sees it; its definition rides on the surface
//! ([`crate::surface::surface::Surface::procedural_blend`] is the one way to it, for every
//! blend face). The definition is a set's, not a parametrisation's: the net
//! reparametrised, reversed, swapped, trimmed or split is still on S, so those edits carry
//! it; a motion moves it with the net ([`ExactRound::transform`]). A rational class's
//! proof pairs the net's spans with its centre line's through a map ([`LeaderMap`]) that
//! the net's reversals, reparametrisations and swaps carry; a piece or a trim keeps it.
//!
//! A chamfer between planes (#2486) is the same: the chord between the feet of a line spine
//! is the plane through them ([`ExactClass::Chord`]), kept as a `Plane` with the chamfer's
//! sizes beside it, its section the chord (order 0).
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion: the supports
//! are kept as surfaces without definitions of their own (a support that is an exact
//! round, as a corner ball's rounds are, keeps its definition on its own face), so they
//! are walked one level.

use crate::blend::fillet_exact::{
    chord_implicit, class_surface, classify_supports, BlendSpine, ClassInputs, QuadricKind,
};
use crate::blend::fillet_section::series::Trunc;
use crate::blend::fillet_section::station::gn_solve_station;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::tol::are_same;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, ControlPointNet, SurfParamType};
use crate::nurbs::math::gamma;
use crate::surface::cone::Cone;
use crate::surface::implicit::{
    check_exact_net, lies_on, AnalyticSupport, ClassSurface, ExactNetCheck, Implicit, LeaderMap,
};
use crate::surface::procedural_blend::chamfer::ChamferDefinition;
use crate::surface::procedural_blend::ProceduralBlend;
use crate::surface::surface::{Surface, SurfaceKind};

const OP_TRANSFORM: &str = "surface::procedural_blend::exact::ExactRound::transform";
const OP_CIRCLE: &str = "surface::procedural_blend::exact::ExactRound::supports_distance";

fn finite_unit_direction(direction: Vec3) -> Option<Vec3> {
    let squared = direction.length_squared();
    if !(squared.is_finite() && squared > 0.0) {
        return None;
    }
    let unit = direction.unitize().ok()?;
    let squared = unit.length_squared();
    (squared.is_finite() && (squared - 1.0).abs() <= gamma(32)).then_some(unit)
}

/// A tensor-product extrusion identity: `P_ij = Q_j + D_i A` and
/// `w_ij w_00 = w_i0 w_0j`. Substitution in the rational basis factors the
/// surface into a cross-section plus a scalar function times the fixed axis A.
/// Only control arithmetic's rounding is allowed, independently of model tolerance.
fn net_translation_axis(net: &ControlPointNet, along_u: bool) -> Option<Vec3> {
    let (along, across) = if along_u {
        (net.u_count, net.v_count)
    } else {
        (net.v_count, net.u_count)
    };
    if along < 2 || across < 2 {
        return None;
    }
    let at = |i: usize, j: usize| {
        let (u, v) = if along_u { (i, j) } else { (j, i) };
        let index = u.checked_mul(net.v_count)?.checked_add(v)?;
        let point = *net.points.get(index)?;
        let weight = if net.weights.is_empty() {
            1.0
        } else {
            *net.weights.get(index)?
        };
        (point.x.is_finite()
            && point.y.is_finite()
            && point.z.is_finite()
            && weight.is_finite()
            && weight > 0.0)
            .then_some((point, weight))
    };
    let (p00, w00) = at(0, 0)?;
    let mut direction = Vec3::ZERO;
    for i in 1..along {
        let candidate = at(i, 0)?.0 - p00;
        if candidate.length() > direction.length() {
            direction = candidate;
        }
    }
    let axis = finite_unit_direction(direction)?;
    for i in 0..along {
        let (pi0, wi0) = at(i, 0)?;
        let shift = pi0 - p00;
        for j in 0..across {
            let (p0j, w0j) = at(0, j)?;
            let (pij, wij) = at(i, j)?;
            let scale = p00.length() + pi0.length() + p0j.length() + pij.length();
            let bound = gamma(64) * scale;
            let lhs = wij * w00;
            let rhs = wi0 * w0j;
            if !(bound.is_finite()
                && lhs.is_finite()
                && rhs.is_finite()
                && lhs > 0.0
                && rhs > 0.0
                && ((pij - p0j) - shift).length() <= bound
                && shift.cross(axis).length() <= bound
                && (lhs - rhs).abs() <= gamma(8) * lhs.abs() + gamma(8) * rhs.abs())
            {
                return None;
            }
        }
    }
    Some(axis)
}

fn spline_translation_axis(surface: &BSplineSurface) -> Result<Option<Vec3>> {
    let net = surface.control_point_net()?;
    Ok(net_translation_axis(&net, true).or_else(|| net_translation_axis(&net, false)))
}

/// The shared translation of two extrusion supports, including a plane containing
/// that direction. The labelled constructions supply the identity; the angular
/// comparisons allow only the rounding of normalising and taking dot/cross products.
fn extrusion_support_axis(supports: [&Surface; 2]) -> Result<Option<Vec3>> {
    let mut axes = [None, None];
    for (slot, support) in axes.iter_mut().zip(supports) {
        *slot = match support.kind() {
            SurfaceKind::SurfOfExtrusion(e) => finite_unit_direction(e.extrusion_vector()),
            SurfaceKind::BSpline(b) => spline_translation_axis(b)?,
            _ => None,
        };
    }
    let Some(axis) = axes.iter().copied().flatten().next() else {
        return Ok(None);
    };
    let rounding = gamma(32);
    for (support, direction) in supports.into_iter().zip(axes) {
        if let Some(direction) = direction {
            if axis.cross(direction).length() > rounding {
                return Ok(None);
            }
            continue;
        }
        let invariant = match support.kind() {
            SurfaceKind::Plane(p) => {
                let normal = p.evaluate_normal();
                let squared = normal.length_squared();
                squared.is_finite()
                    && squared > 0.0
                    && axis.dot(normal).abs() <= rounding * squared.sqrt()
            }
            _ => false,
        };
        if !invariant {
            return Ok(None);
        }
    }
    Ok(Some(axis))
}

/// The complete cylinder carrying a constant circular blend whose supports' offsets
/// meet in a line. The carrier is read from the definition, independently of the net's
/// finite rational chart, for an end cap that continues the support past that chart.
/// Returns `None` for every other blend class.
///
/// # Errors
/// Those of the definition's station solve, support classification and cylinder construction.
pub(crate) fn cylinder_support(
    k: &Kernel,
    pb: &ProceduralBlend,
    tol: f64,
) -> Result<Option<Surface>> {
    let def = pb.definition();
    let [r0, r1] = def.offsets;
    if def.order != 1
        || def.law.is_some()
        || !(r0.is_finite() && r0 > 0.0 && r0 == r1 && tol.is_finite() && tol > 0.0)
    {
        return Ok(None);
    }
    let s = def.leader.natural_interval().evaluate(0.5);
    let [seed0, seed1] = &def.seeds;
    let (seed0, seed1) = (seed0.evaluate_point(s)?, seed1.evaluate_point(s)?);
    let [support0, support1] = &def.supports;
    let supports = [support0.as_ref(), support1.as_ref()];
    let station = gn_solve_station(
        k,
        supports,
        &def.leader,
        s,
        [seed0.x, seed0.y, seed1.x, seed1.y],
        def.offsets,
        def.sides,
        Trunc::new(1, 0)?,
        tol,
    )?;
    let [u0, v0, u1, v1] = station.x;
    let [cx, cy, cz] = station.v.a;
    let [side0, side1] = def.sides;
    let inputs = ClassInputs {
        uv: [
            crate::math::Point2::new(u0.at(0, 0), v0.at(0, 0)),
            crate::math::Point2::new(u1.at(0, 0), v1.at(0, 0)),
        ],
        offsets: [r0 * side0, r1 * side1],
        centre: Point3::new(cx.at(0, 0), cy.at(0, 0), cz.at(0, 0)),
        tol,
        ball_on_collapsed: false,
        radii_can_change: false,
    };
    if !inputs.centre.to_array().into_iter().all(f64::is_finite) {
        return Ok(None);
    }
    let (origin, axis, radius) = if let Some(axis) = extrusion_support_axis(supports)? {
        // Translating one solved ball preserves both support contacts and their
        // offsets, so its section sweeps the cylinder about this centre line.
        (inputs.centre, axis, r0)
    } else {
        // Two labelled planes have a straight offset intersection. Other classes
        // need their own identity; a tolerance-based spine classification alone
        // cannot certify an exact cylindrical continuation.
        if !supports
            .iter()
            .all(|surface| matches!(surface.kind(), SurfaceKind::Plane(_)))
        {
            return Ok(None);
        }
        let blend = classify_supports(k, supports, &inputs)?;
        if blend.spine != BlendSpine::Line {
            return Ok(None);
        }
        (blend.point, blend.dir, blend.radius)
    };
    if !origin.to_array().into_iter().all(f64::is_finite) {
        return Ok(None);
    }
    let Some(axis) = finite_unit_direction(axis) else {
        return Ok(None);
    };
    let d = pb.evaluate_definition(k, s, 0.5, 1, 1)?;
    let partial = |i: usize, j: usize| {
        d.get(i).and_then(|row| row.get(j)).copied().ok_or_else(|| {
            SolidError::surface_failed(OP_CIRCLE, "the circular section has no requested partial")
        })
    };
    let radial = partial(0, 0)? - inputs.centre;
    // Put the cylinder's periodic seam opposite the original circular section.
    // Its end cap must not straddle an arbitrary seam through the contact arc.
    let Some(phase) = finite_unit_direction(radial - axis * radial.dot(axis)) else {
        return Ok(None);
    };
    let x = -phase;
    let Some(y) = finite_unit_direction(axis.cross(x)) else {
        return Ok(None);
    };
    let placement = Axis2Placement::new(origin, x, y);
    let mut carrier = Surface::from(Cone::create_canonical_cylinder(k, &placement, radius)?);
    let mut normal = partial(1, 0)?.cross(partial(0, 1)?);
    let (_, map_b, swapped) = pb.definition_map();
    let [bs, bt] = map_b;
    if !(bs.is_finite() && bt.is_finite() && bs != 0.0 && bt != 0.0) {
        return Ok(None);
    }
    if ((bs < 0.0) != (bt < 0.0)) != swapped {
        normal = -normal;
    }
    let sense = normal.dot(radial);
    if !sense.is_finite() || sense == 0.0 {
        return Ok(None);
    }
    if sense < 0.0 {
        carrier.reverse(k, SurfParamType::U)?;
    }
    Ok(Some(carrier))
}

/// The torus a round of the circle class is: a surface of revolution's, about its
/// placement's axis, of the circle its generating arc is on (the centre of a rational quadratic
/// arc from its first span's control points: the midpoint of the chord pushed away from the
/// middle point by `a^2 / h`, `a` the half chord, `h` the middle point's distance from the
/// chord's midpoint); or a torus label's own. `None` for any other surface.
///
/// # Errors
///
/// Those of reading the arc's control polygon.
fn own_torus(net: &Surface) -> Result<Option<Implicit>> {
    use crate::curve::curve::{Curve, CurveKind};
    use crate::surface::surface::SurfaceKind;
    match net.kind() {
        SurfaceKind::Torus(_) => Ok(AnalyticSupport::of(net)
            .as_ref()
            .map(AnalyticSupport::implicit)),
        SurfaceKind::SurfOfRevolution(rev) => {
            let Some(CurveKind::BSpline(arc)) = rev.gen_curve().map(Curve::kind) else {
                return Ok(None);
            };
            let (points, _) = arc.control_polygon()?;
            let [p0, p1, p2, ..] = points.as_slice() else {
                return Ok(None);
            };
            let mid = (*p0 + *p2) * 0.5;
            let (a, h) = ((*p0 - mid).length(), (mid - *p1).length());
            if h.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
                return Ok(None);
            }
            let centre = mid + (mid - *p1) * (a * a / (h * h));
            let position = rev.position();
            let (o, axis) = (
                position.origin(),
                position.x_axis().cross(position.y_axis()),
            );
            let along = (centre - o).dot(axis);
            let across = (centre - o) - axis * along;
            Ok(Some(Implicit::Torus {
                centre: o + axis * along,
                axis,
                major: across.length(),
                minor: (*p0 - centre).length(),
            }))
        }
        _ => Ok(None),
    }
}

/// The exact class of a round whose net is its definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactClass {
    /// A straight spine: the cylinder of the ball's radius about it.
    Line,
    /// A circle about the supports' common axis: the ball's section turned about it.
    Circle,
    /// One point: a piece of the ball.
    Point,
    /// A chamfer between planes (#2486): the chord between the feet of the line its
    /// offsets meet in, the plane through them.
    Chord,
    /// A plane section of a cylinder or cone: the canal surface on the conic.
    Conic,
    /// One of two plane conics of two cylinders or cones: the canal surface on it.
    ConicPair,
    /// A quartic with a node where the offsets touch: the canal surface on it.
    NodalQuartic,
}

impl ExactClass {
    /// The class of a classified spine, if it is one kept as a definition.
    #[must_use]
    pub const fn of_spine(spine: BlendSpine) -> Option<Self> {
        match spine {
            BlendSpine::Line => Some(Self::Line),
            BlendSpine::Circle => Some(Self::Circle),
            BlendSpine::Point => Some(Self::Point),
            BlendSpine::Conic => Some(Self::Conic),
            BlendSpine::ConicPair => Some(Self::ConicPair),
            BlendSpine::NodalQuartic => Some(Self::NodalQuartic),
            BlendSpine::Traced => None,
        }
    }

    /// Its name, for a refusal.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Circle => "circle",
            Self::Point => "point",
            Self::Chord => "chord",
            Self::Conic => "conic",
            Self::ConicPair => "conic pair",
            Self::NodalQuartic => "nodal quartic",
        }
    }
}

/// The definition of an exact round: its supports, its ball, its section, its leader and
/// what its class was read with.
#[derive(Debug, Clone)]
pub struct ExactRound {
    /// The class.
    pub(crate) class: ExactClass,
    /// The supports S1, S2 the ball rolls on (the faces the class was read off).
    pub(crate) supports: [Box<Surface>; 2],
    /// The ball's radius (0 for the chord, whose offsets are its inputs').
    pub(crate) radius: f64,
    /// The section's order: 1, the ball's circle; 0, the chord.
    pub(crate) order: u32,
    /// A chamfer's sizes (the chord, #2486).
    pub(crate) chamfer: Option<ChamferDefinition>,
    /// The leader (the centre line), where the class's construction makes one.
    pub(crate) leader: Option<BSplineCurve>,
    /// What the class was read with beside the supports.
    pub(crate) inputs: ClassInputs,
    /// How the net's parameter runs along the leader (the identity in u as built; the
    /// net's reversals, reparametrisations and swaps move it).
    pub(crate) map: LeaderMap,
}

impl ExactRound {
    /// A definition of class `class` on copies of `supports`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a radius not finite and positive; those of copying a support.
    pub fn create(
        k: &Kernel,
        class: ExactClass,
        supports: [&Surface; 2],
        radius: f64,
        leader: Option<&BSplineCurve>,
        inputs: ClassInputs,
    ) -> Result<Self> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(SolidError::invalid_input(
                "surface::procedural_blend::exact::ExactRound::create",
                "the ball's radius is finite and positive",
            ));
        }
        // a support that is itself an exact round (a corner ball's rounds) is kept as its
        // surface: its definition is its own face's, so definitions never nest here
        let support = |s: &Surface| -> Result<Box<Surface>> {
            let mut c = s.copy(k)?;
            c.set_exact_round(None);
            Ok(Box::new(c))
        };
        let [a, b] = supports;
        Ok(Self {
            class,
            supports: [support(a)?, support(b)?],
            radius,
            order: 1,
            chamfer: None,
            leader: leader.cloned(),
            inputs,
            map: LeaderMap::default(),
        })
    }

    /// A chamfer between planes (#2486): the chord, of the sizes `chamfer`, on copies of
    /// `supports`, its offsets the inputs' (their sizes `chamfer` gives).
    ///
    /// # Errors
    ///
    /// `InvalidInput` where the sizes do not give the inputs' offsets; those of copying a
    /// support.
    pub fn chord(
        k: &Kernel,
        supports: [&Surface; 2],
        chamfer: ChamferDefinition,
        leader: Option<&BSplineCurve>,
        inputs: ClassInputs,
    ) -> Result<Self> {
        if !chamfer.holds(inputs.offsets.map(f64::abs)) {
            return Err(SolidError::invalid_input(
                "surface::procedural_blend::exact::ExactRound::chord",
                "the chamfer's sizes do not give its offsets",
            ));
        }
        let support = |s: &Surface| -> Result<Box<Surface>> {
            let mut c = s.copy(k)?;
            c.set_exact_round(None);
            Ok(Box::new(c))
        };
        let [a, b] = supports;
        Ok(Self {
            class: ExactClass::Chord,
            supports: [support(a)?, support(b)?],
            radius: 0.0,
            order: 0,
            chamfer: Some(chamfer),
            leader: leader.cloned(),
            inputs,
            map: LeaderMap::default(),
        })
    }

    /// A chamfer's sizes (the chord).
    #[must_use]
    pub const fn chamfer(&self) -> Option<&ChamferDefinition> {
        self.chamfer.as_ref()
    }

    /// The class.
    #[must_use]
    pub const fn class(&self) -> ExactClass {
        self.class
    }

    /// `GetSupport(i)`: the surface S1 (0) or S2 (1) the ball rolls on.
    #[must_use]
    pub fn support(&self, i: usize) -> Option<&Surface> {
        self.supports.get(i).map(AsRef::as_ref)
    }

    /// The ball's radius.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius
    }

    /// The section's order (1: the ball's circle; 0: the chord).
    #[must_use]
    pub const fn order(&self) -> u32 {
        self.order
    }

    /// The leader, where the class's construction makes one.
    #[must_use]
    pub const fn leader(&self) -> Option<&BSplineCurve> {
        self.leader.as_ref()
    }

    /// What the class was read with beside the supports.
    #[must_use]
    pub const fn inputs(&self) -> &ClassInputs {
        &self.inputs
    }

    /// How the net's parameter runs along the leader.
    #[must_use]
    pub const fn leader_map(&self) -> &LeaderMap {
        &self.map
    }

    /// The net's parameter in `u` (or v) became `w'` with `w = alpha + beta w'` (a
    /// reversal or reparametrisation of the net): the leader's map follows.
    pub(crate) fn reparametrize_map(&mut self, u: bool, alpha: f64, beta: f64) {
        self.map.reparametrize(u, alpha, beta);
    }

    /// The net's u and v swapped: the leader's map follows.
    pub(crate) fn swap_map(&mut self) {
        self.map.swap();
    }

    /// The net's certified distance from the definition: 0, the net is S.
    #[must_use]
    pub const fn bound(&self) -> f64 {
        0.0
    }

    /// Re-certifies `net` against this definition: the supports classified again are of
    /// this class, and the net lies on the class's surface derived from them
    /// ([`check_exact_net`]). A round of the circle class is certified on its own torus
    /// instead ([`ExactRound::supports_distance`]). Returns the distance proved, 0.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming what fails: supports of another class, a class with no
    /// surface from them, a net off it; those of reading the supports' quadrics and the
    /// net's NURBS form.
    pub fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        if self.class == ExactClass::Circle {
            self.supports_distance(k, net)?;
            return Ok(0.0);
        }
        let [a, b] = &self.supports;
        let blend = classify_supports(k, [a, b], &self.inputs)?;
        // the chord (#2486): a line spine between planes, of sizes that give its offsets
        let (reclassified, surface) = if self.class == ExactClass::Chord {
            let planes = blend.faces.iter().all(|f| f.kind == QuadricKind::Plane);
            let sized = self
                .chamfer
                .is_some_and(|c| c.holds(self.inputs.offsets.map(f64::abs)));
            let chord = blend.spine == BlendSpine::Line && planes && sized;
            (
                chord.then_some(ExactClass::Chord.name()),
                chord_implicit(&blend).map(ClassSurface::Implicit),
            )
        } else {
            (
                ExactClass::of_spine(blend.spine).map(ExactClass::name),
                class_surface(&blend, self.leader.as_ref(), &self.map),
            )
        };
        check_exact_net(
            k,
            net,
            &ExactNetCheck {
                class: self.class.name(),
                reclassified,
                surface,
            },
        )?;
        Ok(0.0)
    }

    /// A round of the circle class: the torus `net` is ([`own_torus`]: its revolution's
    /// axis and generating circle, or its torus label), proved to hold its form to rounding
    /// ([`lies_on`]),
    /// and how far that torus is from the class's torus derived from the supports
    /// ([`class_surface`]): returned, refused past the tolerance the class was read within.
    ///
    /// The class is read within the body's tolerance. Supports only that close to it (a plane
    /// 1e-13 off square to a cylinder's axis, read from a record) have no torus touching both
    /// exactly, so the round the solver builds, tangent to the supports as they are, is on a
    /// torus of its own. Its definition is that torus, at bound 0, a stated distance from the
    /// supports' class.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for supports of another class or with no torus, a net of no torus, a
    /// net off its torus, or a torus further from the class's than the tolerance; those of
    /// reading the supports' quadrics, the net's NURBS form and [`lies_on`].
    pub fn supports_distance(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        let refused = |why| SolidError::invalid_input(OP_CIRCLE, why);
        let [a, b] = &self.supports;
        let blend = classify_supports(k, [a, b], &self.inputs)?;
        if ExactClass::of_spine(blend.spine) != Some(ExactClass::Circle) {
            return Err(refused(
                "the supports classified again are not of the class the definition names",
            ));
        }
        let Some(ClassSurface::Implicit(Implicit::Torus {
            centre,
            axis,
            major,
            minor,
        })) = class_surface(&blend, self.leader.as_ref(), &self.map)
        else {
            return Err(refused("the supports give the class no torus"));
        };
        let form = crate::blend::fillet_section::station::as_b_spline(k, net)?
            .ok_or_else(|| refused("the net has no NURBS form to check"))?;
        let own = own_torus(net)?.ok_or_else(|| refused("the net is no torus"))?;
        if !lies_on(&form, &own)? {
            return Err(refused("the net is off its own torus"));
        }
        let Implicit::Torus {
            centre: c,
            axis: d,
            major: big_r,
            minor: r,
        } = own
        else {
            return Err(refused("the net's own surface is no torus"));
        };
        // every point of the net's torus moves to the class's torus by at most: the turn of
        // its axis (an angle, over the torus's reach from its centre), the move of its
        // centre, and the change of its radii
        let turn = d.cross(axis).length().atan2(d.dot(axis).abs());
        let distance =
            turn * (big_r + r) + (c - centre).length() + (big_r - major).abs() + (r - minor).abs();
        let within = matches!(
            distance.partial_cmp(&self.inputs.tol),
            Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
        );
        if !within {
            return Err(refused(
                "the net's torus is further from the class's than the tolerance",
            ));
        }
        Ok(distance)
    }

    /// Moves the definition with its net (`Transform`): the supports, the leader and the
    /// centre by the same motion; a uniform scale scales the radius, the offsets and the
    /// tolerance with them.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a scale that is not uniform and positive (no exact round stays
    /// one under it); those of the supports' and the leader's transforms.
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
                    "an exact round moves rigidly or by a uniform scale",
                ));
            }
            factor = sc.x;
        }
        for support in &mut self.supports {
            support.transform_at_depth(k, rotate_n_move, scale, depth + 1)?;
        }
        if let Some(leader) = self.leader.as_mut() {
            leader.transform(rotate_n_move, scale)?;
        }
        let c: Point3 = self.inputs.centre * factor;
        self.inputs.centre = rotate_n_move.transform_point(c);
        for o in &mut self.inputs.offsets {
            *o *= factor;
        }
        self.inputs.tol *= factor;
        self.radius *= factor;
        self.chamfer = self.chamfer.map(|c| c.scaled(factor));
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::{BSplineSurfaceForm, CanonicalSurfaceData};

    /// A parabolic cylinder with a rational, degree-two axial parameter: the same
    /// free B-spline representation retained when an extrusion support is recorded.
    #[test]
    fn a_free_spline_support_keeps_its_exact_translation_identity() {
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for (height, axial_weight) in [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0)] {
            for (x, y) in [(-1.0, 1.0), (0.0, -1.0), (1.0, 1.0)] {
                points.push(Point3::new(x, y, height));
                weights.push(axial_weight);
            }
        }
        let form = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 2,
            v_degree: 2,
            control_points: &points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[3, 3],
            v_multiplicities: &[3, 3],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::PiecewiseBezierKnots,
            weights: Some(&weights),
            uv_domain: None,
        })
        .unwrap();
        let axis = spline_translation_axis(&form).unwrap().unwrap();
        assert!(axis.cross(Vec3::Z).length() < 1e-14);
        let mut net = form.control_point_net().unwrap();
        net.points[4].x += 0.01;
        assert!(net_translation_axis(&net, true).is_none());
        let mut net = form.control_point_net().unwrap();
        net.weights[4] *= 1.01;
        assert!(net_translation_axis(&net, true).is_none());
        // The products remain finite at this scale, but adding them before
        // multiplying by their rounding factor would overflow and accept any error.
        let mut net = form.control_point_net().unwrap();
        net.weights.fill(1e154);
        assert!(net_translation_axis(&net, true).is_some());
        net.weights[4] *= 1.01;
        assert!(net_translation_axis(&net, true).is_none());
    }
}
