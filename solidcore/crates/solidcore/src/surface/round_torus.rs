//! The torus a round of the exact circle class is (#2485): a plane square to a cylinder's
//! axis, two coaxial cones, a sphere and a coaxial cone, rounded by a ball whose centre runs
//! on a circle about their common axis. Its net is the ball's arc turned about that axis, a
//! surface of revolution carrying the round's definition.
//!
//! [`circle_round_torus`] reads that torus off the net as an SMLib torus is read off its
//! NURBS form ([`Torus::is_nurb_surface_torus`]), so its frame and radii describe the net in
//! its own parameterisation, and certifies it against the definition: the net lies on its own
//! torus, and that torus is within the tolerance the class was read in of the torus the
//! supports give (`ExactRound::supports_distance`). Only a round the definition names as
//! the circle class is read so: no other surface is recognised as a torus here.
//!
//! Not upstream. Deviations: none (no upstream counterpart). No recursion.

use crate::blend::fillet_section::station::as_b_spline;
use crate::error::Result;
use crate::kernel::Kernel;
use crate::surface::procedural_blend::exact::ExactClass;
use crate::surface::procedural_blend::BlendRef;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::torus::Torus;

/// A circle-class round's torus and its certificate.
#[derive(Debug, Clone)]
pub struct RoundTorus {
    /// The net's torus, read off its NURBS form: its placement (origin the torus's centre, z
    /// its axis), major and minor radii, and its NURBS (the form, as the reading made it).
    pub torus: Torus,
    /// The certificate: how far the net's torus is from the torus the round's supports give,
    /// at most the tolerance the class was read in.
    pub distance: f64,
}

/// The torus `surface` is when it is a round of the exact circle class: a surface of
/// revolution whose definition is an exact round of that class, read and certified as the
/// module says. `None` for every other surface, and for a round whose net does not read as a
/// torus ([`Torus::is_nurb_surface_torus`] answers no).
///
/// # Errors
///
/// Those of `ExactRound::supports_distance` (supports no longer of the class, a net off
/// its own torus or further from the class's than the tolerance: the definition and its
/// net disagree), of making the net's NURB, and of reading the torus other than its answer.
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
    let distance = round.supports_distance(k, net)?;
    let Some(form) = as_b_spline(k, net)? else {
        return Ok(None);
    };
    let torus = match Torus::is_nurb_surface_torus(k, &form, 1.0) {
        Ok(found) => found,
        Err(e) if e.is_status_answer() => None,
        Err(e) => return Err(e),
    };
    Ok(torus.map(|torus| RoundTorus { torus, distance }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::surface::torus::TorusOptions;

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
