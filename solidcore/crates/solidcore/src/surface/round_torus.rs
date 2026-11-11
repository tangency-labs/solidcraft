//! The torus a round of the exact circle class is (#2485): a plane square to a cylinder's
//! axis, two coaxial cones, a sphere and a coaxial cone, rounded by a ball whose centre runs
//! on a circle about their common axis. Its net is the ball's arc turned about that axis, a
//! surface of revolution carrying the round's definition.
//!
//! [`circle_round_torus`] gives the round's exact surface: the torus its supports give (the
//! supports classified again, the class's surface), with a frame about its axis, and the
//! certificate that the net is that torus: the net lies on its own torus, and that torus is
//! within the tolerance the class was read in of the supports' (`ExactRound::supports_distance`).
//! The two are one surface within that tolerance, their frames differing only in where the
//! angle about the axis starts. Only a round the definition names as the circle class is read
//! so: no other surface is recognised as a torus here.
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_exact::{class_surface, classify_supports};
use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::surface::implicit::{ClassSurface, Implicit};
use crate::surface::procedural_blend::exact::ExactClass;
use crate::surface::procedural_blend::BlendRef;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::round_torus::circle_round_torus";

/// A circle-class round's torus, the round's exact surface, and its certificate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundTorus {
    /// The torus's centre.
    pub centre: Point3,
    /// Its unit axis.
    pub axis: Vec3,
    /// A unit reference direction square to the axis: where the angle about it starts.
    pub x_axis: Vec3,
    /// The major radius: of the circle the ball's centre runs on.
    pub major: f64,
    /// The minor radius: the ball's.
    pub minor: f64,
    /// The certificate: how far the net's own torus is from this one, at most the tolerance
    /// the class was read in.
    pub distance: f64,
}

/// The torus `surface` is when it is a round of the exact circle class: a surface of
/// revolution whose definition is an exact round of that class, read and certified as the
/// module says. `None` for every other surface.
///
/// # Errors
///
/// `InvalidInput` for a circle-class round without its two supports or whose supports give
/// no torus; those of classifying the supports and of `ExactRound::supports_distance`
/// (supports no longer of the class, a net off its own torus or further from the class's
/// than the tolerance: the definition and its net disagree); those of the frame about a
/// zero axis.
pub fn circle_round_torus(k: &Kernel, surface: &Surface) -> Result<Option<RoundTorus>> {
    // (a round whose net is a torus already is one: it needs no reading)
    if !matches!(surface.kind(), SurfaceKind::SurfOfRevolution(_)) {
        return Ok(None);
    }
    let Some(BlendRef::Exact { round, net }) = surface.procedural_blend() else {
        return Ok(None);
    };
    if round.class() != ExactClass::Circle {
        return Ok(None);
    }
    let (Some(a), Some(b)) = (round.support(0), round.support(1)) else {
        return Err(SolidError::invalid_input(
            OP,
            "an exact round without its two supports",
        ));
    };
    let blend = classify_supports(k, [a, b], round.inputs())?;
    let Some(ClassSurface::Implicit(Implicit::Torus {
        centre,
        axis,
        major,
        minor,
    })) = class_surface(&blend, round.leader(), round.leader_map())
    else {
        return Err(SolidError::invalid_input(
            OP,
            "a round of the circle class whose supports give no torus",
        ));
    };
    let distance = round.supports_distance(k, net)?;
    // `[axis, y, z]`: y and z square to the axis, z = axis x y, so (y, z) is a frame about it
    let [axis, x_axis, _] = axis.make_unit_ortho_vectors(None)?;
    Ok(Some(RoundTorus {
        centre,
        axis,
        x_axis,
        major,
        minor,
        distance,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::torus::{Torus, TorusOptions};

    /// A torus that is no round's is not read here, nor is any surface but a round's net.
    #[test]
    fn only_a_circle_class_round_is_read() {
        let k = Kernel::default();
        let torus = Torus::create_canonical(
            &k,
            &Axis2Placement::default(),
            10.0,
            3.0,
            TorusOptions::default(),
            None,
        )
        .unwrap();
        let surface = Surface::from(torus.clone());
        assert!(circle_round_torus(&k, &surface).unwrap().is_none());
        let revolution = Surface::from(torus.base().clone());
        assert!(
            circle_round_torus(&k, &revolution).unwrap().is_none(),
            "a surface of revolution with no definition"
        );
    }
}
