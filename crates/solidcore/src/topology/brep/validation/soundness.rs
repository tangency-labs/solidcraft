//! (not upstream) A body's soundness: its check at `SM_LEVEL_2`, and for a closed body,
//! whether it is a manifold solid of positive volume. The local operation measures its
//! input's [`standing`] (the check with the body's own owner naming, and the manifold
//! test) and its result's, and the result's [`volume`] only to test it is positive
//! ([`crate::topology::brep::local_operation::local_operation`]); the goldens soundness
//! probe (`goldens::soundness`) measures result bodies with [`soundness`] (the geometry's
//! recorded owners).

use std::collections::BTreeMap;

use crate::error::{EntityRef, Result};
use crate::geom::attribute::ObjectKey;
use crate::geom::object::ObjectRef;
use crate::kernel::Kernel;
use crate::topology::Brep;

use super::assert_valid::{check_brep_with_keys, BrepCheck, CheckLevel};

/// The relative accuracy the volume is integrated at: its sign is a verdict on the body,
/// which a closed solid's whole volume holds away from zero.
const VOLUME_ACCURACY: f64 = 1e-6;

/// What [`soundness`] measured of one body.
#[derive(Debug, Clone)]
pub struct Soundness {
    /// The check at [`CheckLevel::Level2`], its geometry's owners taken as the geometry
    /// records them.
    pub check: BrepCheck,
    /// Whether the body is a manifold solid ([`crate::topology::brep::is_manifold_solid`]).
    pub manifold: bool,
    /// Its volume, when it is a manifold solid.
    pub volume: Option<f64>,
}

impl Soundness {
    /// Whether the check is clean and, when the body is a manifold solid, its volume is
    /// positive (a NaN volume is not).
    #[must_use]
    pub fn sound(&self) -> bool {
        self.check.is_valid() && (!self.manifold || self.volume.is_some_and(|v| v > 0.0))
    }
}

/// What [`standing`] measured of one body: its check and whether it is a manifold solid,
/// not its volume.
#[derive(Debug, Clone)]
pub struct Standing {
    /// The check at [`CheckLevel::Level2`], its geometry's owners the identities the
    /// body's naming scheme names its entities by.
    pub check: BrepCheck,
    /// Whether the body is a manifold solid ([`crate::topology::brep::is_manifold_solid`]).
    pub manifold: bool,
}

/// Measures `b`: [`check_brep_with_keys`] at [`CheckLevel::Level2`], each geometry's owner
/// the identity the geometry records (an edge's curve's, a face's surface's, an edge use's
/// UV trim curve's; an edge use without a trim the identity the body names it by, which the
/// trim the check makes is owned under), then the manifold test and, for a manifold solid,
/// the volume. What is checked is the body's geometry, not a caller's naming of it (a
/// runner names a body's entities by identities of its own): the owner tests cannot fail.
/// [`standing`] checks the owners.
///
/// The check makes the UV trim curves and caches its evaluations need on `b`: measure a
/// private copy ([`Brep::clone_standing_for_self`]) to leave a body as it was.
///
/// # Errors
///
/// Those of the check, the manifold test and the volume.
pub fn soundness(k: &mut Kernel, b: &mut Brep) -> Result<Soundness> {
    let mut owners = BTreeMap::new();
    for (id, e) in b.edges().iter() {
        if let Some(o) = e.curve.as_ref().and_then(crate::curve::curve::Curve::owner) {
            owners.insert(EntityRef::Edge(id), o);
        }
    }
    for (id, f) in b.faces().iter() {
        if let Some(o) = f
            .surface
            .as_ref()
            .and_then(crate::surface::surface::Surface::owner)
        {
            owners.insert(EntityRef::Face(id), o);
        }
    }
    for (id, u) in b.edgeuses().iter() {
        let o = match u.uv_trim_curve.as_ref() {
            Some(trim) => trim.owner(),
            None => Some(b.owner_identity(EntityRef::Edgeuse(id))),
        };
        if let Some(o) = o {
            owners.insert(EntityRef::Edgeuse(id), o);
        }
    }
    let keys = |e: EntityRef| {
        owners.get(&e).copied().unwrap_or(ObjectRef {
            type_code: 0,
            key: ObjectKey(0),
        })
    };
    let check = check_brep_with_keys(k, b, CheckLevel::Level2, &keys)?;
    let manifold = crate::topology::brep::p2::is_manifold_solid(b)?;
    let volume = if manifold { Some(volume(k, b)?) } else { None };
    Ok(Soundness {
        check,
        manifold,
        volume,
    })
}

/// Measures `b`'s standing: [`check_brep_with_keys`] at [`CheckLevel::Level2`], each
/// geometry's owner the identity `b`'s naming scheme names the entity by
/// ([`Brep::owner_identity`], as the operations that bind geometry set it), so a curve or
/// surface owned under another entity's identity fails the owner tests; then the manifold
/// test. Its volume is not measured ([`volume`]).
///
/// As for [`soundness`], the check edits what it measures: measure a private copy.
///
/// # Errors
///
/// Those of the check and the manifold test.
pub fn standing(k: &mut Kernel, b: &mut Brep) -> Result<Standing> {
    // the B-rep is borrowed mutably by the check: its naming and recorded keys are copied
    let naming = b.naming();
    let recorded = b.geometry_owner_keys().clone();
    let keys =
        |e: EntityRef| crate::topology::owner_identity_of(naming, recorded.get(&e).copied(), e);
    let check = check_brep_with_keys(k, b, CheckLevel::Level2, &keys)?;
    let manifold = crate::topology::brep::p2::is_manifold_solid(b)?;
    Ok(Standing { check, manifold })
}

/// The volume of the manifold solid `b`, integrated at [`VOLUME_ACCURACY`].
///
/// # Errors
///
/// Those of [`crate::mass::integrator::properties::brep_volume_default`].
pub fn volume(k: &mut Kernel, b: &mut Brep) -> Result<f64> {
    crate::mass::integrator::properties::brep_volume_default(k, b, VOLUME_ACCURACY)
}
