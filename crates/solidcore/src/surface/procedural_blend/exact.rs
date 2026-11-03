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
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::tol::are_same;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::implicit::{check_exact_net, ClassSurface, ExactNetCheck, LeaderMap};
use crate::surface::procedural_blend::chamfer::ChamferDefinition;
use crate::surface::surface::Surface;

const OP_TRANSFORM: &str = "surface::procedural_blend::exact::ExactRound::transform";

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
    /// ([`check_exact_net`]). Returns the distance proved, 0.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming what fails: supports of another class, a class with no
    /// surface from them, a net off it; those of reading the supports' quadrics and the
    /// net's NURBS form.
    pub fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
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
