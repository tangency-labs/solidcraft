//! The checks a record passes before the kernel takes it: the record's structural validation
//! ([`Brep::validate`], [`Brep::validate_built`]) and then the kernel's checks of every edge's
//! and face's geometric records ([`solidcore::eval::check`]: frames orthonormal, planes that span
//! two directions, placed revolutions about a unit axis under a non-singular placement).

use solidcore::eval::check as k;
use solidcore_record::brep::{Brep, Curve, Frame, Invalid, Result, Surface};

/// [`Brep::validate`] (a record from outside the kernel), then [`geometry`].
///
/// # Errors
/// The first check the record fails.
pub fn validate(b: &Brep) -> Result<()> {
    b.validate()?;
    geometry(b)
}

/// [`Brep::validate_built`] (a record the kernel built), then [`geometry`].
///
/// # Errors
/// The first check the record fails.
pub fn validate_built(b: &Brep) -> Result<()> {
    b.validate_built()?;
    geometry(b)
}

/// Every edge's and face's geometric records ([`curve`], [`surface`]). Call on a record its
/// structural validation accepted.
///
/// # Errors
/// The first geometric record the kernel refuses.
pub fn geometry(b: &Brep) -> Result<()> {
    for e in &b.edges {
        curve(&e.curve)?;
        if let Some(n) = &e.nurbs {
            curve(n)?;
        }
    }
    for f in &b.faces {
        surface(&f.surface)?;
        if let Some(n) = &f.nurbs {
            surface(n)?;
        }
    }
    Ok(())
}

/// A frame's axes are unit and perpendicular, to 1e-8 ([`solidcore::eval::check::frame`]).
///
/// # Errors
/// They are not.
pub fn frame(f: &Frame) -> Result<()> {
    k::frame(&solidcore_convert::frame(f)).map_err(Invalid)
}

/// A curve's geometric records ([`solidcore::eval::check::curve`]). Call on a curve its
/// structural validation accepted.
///
/// # Errors
/// The record the kernel refuses.
pub fn curve(c: &Curve) -> Result<()> {
    k::curve(&solidcore_convert::curve(c)?).map_err(Invalid)
}

/// A surface's geometric records ([`solidcore::eval::check::surface`]). Call on a surface its
/// structural validation accepted.
///
/// # Errors
/// The record the kernel refuses.
pub fn surface(s: &Surface) -> Result<()> {
    k::surface(&solidcore_convert::surface(s)?).map_err(Invalid)
}
