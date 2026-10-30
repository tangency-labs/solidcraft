//! The validity checks of the surfaces that wrap another surface (`SmOffsetSurface`,
//! `SmSrfInVolume`); the volume checks a surface in a volume nests into are
//! [`Volume::assert_valid`](crate::surface::volume::Volume::assert_valid).
//!
//! Upstream: `source/SMLib/src/SmOffsetSurface.cpp`, `source/SMLib/src/SmSrfInVolume.cpp`.
//!
//! | Rust                                     | Upstream                              |
//! |------------------------------------------|---------------------------------------|
//! | `offset_surface_assert_valid` (private)  | `SmOffsetSurface::AssertValid`        |
//! | `srf_in_volume_assert_valid` (private)   | `SmSrfInVolume::AssertValid`          |
//!
//! Deviations:
//! - **Contained identities.** Reports about contained objects name keys derived from
//!   the container's key `key` (wrapping): `8 * key + 1` for a surface in a volume's
//!   base surface, `+ 2` for its volume and `+ 2 + n` for the `n`-th map of the volume's
//!   next-map chain. Upstream compares object addresses; only distinctness matters.
//! - **Context tests** (`SmOffsetSurface` 0, `SmSrfInVolume` 4, 5) compare contexts:
//!   one kernel is the only context, so only their owner-flag part can fail.
//! - **Owner links.** `SmSrfInVolume` 2, 3 read the contained objects' owners, which
//!   upstream sets to this object wherever an owner bit is set (constructors,
//!   `SetOwnerFlag`, the setters, `ReadFromDB`); the port does not model those links,
//!   so the tests are the owner bits. `SmOffsetSurface` 1 is that the offset owns its
//!   base and the base's owner is [`BASE_OWNER`] (a shared base's owner is its caller's).
//! - **Self-intersection** (`SmOffsetSurface` 2): upstream ignores the status of
//!   `GlobalSurfaceIntersect` and counts the curve types it left in the array; the port
//!   counts those of [`Surface::global_surface_intersect_with_partial`] the same way and
//!   propagates only [`SolidError::Unported`].
//! - **Recursion.** Before a surface in a volume checks its base surface, its whole
//!   owned geometry is checked to nest at most `MAX_GEOMETRY_NESTING` deep
//!   ([`check_geometry_nesting`]), which bounds the surface recursion.

use super::{Reporter, UNDEF};
use crate::error::{Result, SolidError};
use crate::geom::assert_array::labels::{OFFSET_SURFACE_TYPE, SRF_IN_VOLUME_TYPE};
use crate::geom::assert_array::ObjectKey;
use crate::kernel::Kernel;
use crate::surface::offset_surface::BASE_OWNER;
use crate::surface::surface::p2::{check_geometry_nesting, NestedGeometry};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::surface::assert_valid::wrapped";

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The key of the contained object in `slot` (see the module docs) of the object `key`.
const fn contained_key(key: ObjectKey, slot: u64) -> ObjectKey {
    ObjectKey(key.0.wrapping_mul(8).wrapping_add(slot))
}

// ---- SmOffsetSurface --------------------------------------------------------------------

/// `SmOffsetSurface::AssertValid`.
pub(super) fn offset_surface_assert_valid(
    k: &Kernel,
    s: &Surface,
    r: &mut Reporter<'_>,
) -> Result<bool> {
    let SurfaceKind::OffsetSurface(off) = s.kind() else {
        return Err(invalid("not an offset surface"));
    };
    let mut rtn = true;

    // call the base class AssertValid
    rtn &= if r.bases() {
        super::surface_assert_valid(k, s, r)?
    } else {
        true
    };

    /*  0 */
    // m_pSurface shares the same context (one kernel)
    rtn &= r.report(OFFSET_SURFACE_TYPE, 0, 0, true, UNDEF, UNDEF)?;

    /*  1 */
    // Bad m_pSurface Owner
    let owner_ok = off.owns_surface() && off.base_surface().owner() == Some(BASE_OWNER);
    rtn &= r.report(OFFSET_SURFACE_TYPE, 1, 0, owner_ok, UNDEF, UNDEF)?;

    /*  2 */
    // Self-intersecting surface interior (status ignored: the curve types left count)
    let uv_domain = s.natural_uv_domain_with_nurb(k)?;
    let (xsect, status) = s.global_surface_intersect_with_partial(
        k,
        &uv_domain,
        s,
        &uv_domain,
        [false, false],
        None,
        None,
    );
    if let Err(e) = status {
        if e.is_unported() {
            return Err(e);
        }
    }
    // If the surface does not intersect, then pOptCurveTypes is an empty array
    rtn &= r.report(
        OFFSET_SURFACE_TYPE,
        2,
        0,
        xsect.curve_types.is_empty(),
        UNDEF,
        UNDEF,
    )?;

    // all done
    Ok(rtn)
}

// ---- SmSrfInVolume ----------------------------------------------------------------------

/// `SmSrfInVolume::AssertValid` (no base-class call upstream).
pub(super) fn srf_in_volume_assert_valid(
    k: &Kernel,
    s: &Surface,
    r: &mut Reporter<'_>,
) -> Result<bool> {
    let SurfaceKind::SrfInVolume(siv) = s.kind() else {
        return Err(invalid("not a surface in a volume"));
    };
    // bounds the recursion into the base surface (and its own wrappers)
    check_geometry_nesting(NestedGeometry::Surface(s), 0, OP)?;
    let owner_flag = siv.owner_flag();

    /*  0 */
    // test contained surface
    let surface_ok =
        siv.surface()
            .assert_valid(k, r.list.as_deref_mut(), r.level, contained_key(r.key, 1))?;
    let mut rtn = r.report(SRF_IN_VOLUME_TYPE, 0, 0, surface_ok, UNDEF, UNDEF)?;

    /*  1 */
    // test contained volume
    let volume_key = contained_key(r.key, 2);
    let mut maps = 0_u64;
    let volume_ok =
        siv.volume()
            .assert_valid(r.list.as_deref_mut(), r.level, volume_key, &mut || {
                maps = maps.wrapping_add(1);
                Ok(ObjectKey(volume_key.0.wrapping_add(maps)))
            })?;
    rtn &= r.report(SRF_IN_VOLUME_TYPE, 1, 0, volume_ok, UNDEF, UNDEF)?;

    /*  2 */
    // Bad BaseSurface Owner (the owner is this object whenever bit 1 is set)
    rtn &= r.report(SRF_IN_VOLUME_TYPE, 2, 0, owner_flag & 1 != 0, UNDEF, UNDEF)?;

    /*  3 */
    // Bad BaseVolume Owner (the owner is this object whenever bit 2 is set)
    rtn &= r.report(SRF_IN_VOLUME_TYPE, 3, 0, owner_flag & 2 != 0, UNDEF, UNDEF)?;

    /*  4 */
    // Bad BaseSurface Context (one kernel)
    rtn &= r.report(SRF_IN_VOLUME_TYPE, 4, 0, owner_flag & 1 != 0, UNDEF, UNDEF)?;

    /*  5 */
    // Bad BaseVolume Context (one kernel)
    rtn &= r.report(SRF_IN_VOLUME_TYPE, 5, 0, owner_flag & 2 != 0, UNDEF, UNDEF)?;

    // all done
    Ok(rtn)
}
