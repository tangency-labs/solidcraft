//! Advanced NURBS surface routines: grid evaluation of points and normals, surface
//! extension by a distance or a parameter distance, derivatives of the unit normal,
//! the swung, skinned and translational sweep surface constructions, Coons surfaces
//! and cross-boundary derivative tools.
//!
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp` (declarations in
//! `source/SMLib/inc/NL_SrfAdv.h`). The file is split over several plan units
//! (`nlib.srf_adv_p1`, `_p2`, ...); each lives in its own submodule.

pub mod p1;
pub mod p2;
pub mod p3;
pub mod p4;
