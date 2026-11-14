//! Whether two faces across an edge lie on one surface with the same outward side, proved on
//! the surfaces' definitions: the test an application grows a pick across (a hole's wall
//! stored in two halves opens whole in a shell).
//!
//! Upstream: none (not in SMLib; `SmSurface::CoincidenceCheck` samples a grid of point
//! drops, [`crate::surface::surface::Surface::coincidence_check`], so it proves nothing).
//!
//! | Rust               | Upstream |
//! |--------------------|----------|
//! | [`faces_continue`] | (none)   |
//! | [`Continues`]      | (none)   |
//!
//! **The proof.** Two surfaces are one surface when their definitions are identical: two
//! planes with the same placement (origin and axes), two cones or cylinders with the same
//! axis placement, radius at the origin, semi-angle and parameterisation flags, two spheres
//! or two tori with the same placement, radii and flags. Every number is compared bit for
//! bit, so no tolerance enters. Equal definitions have equal natural normals everywhere, so
//! the faces' outward sides agree exactly when the uses of the two faces that face out of
//! their material (for a sheet, its upward use) are both the same way round or both the
//! opposite way round to their surfaces. Anything else (other kinds, definitions that differ
//! in any number, a face with material on both sides) is [`Continues::Undecided`], never a
//! guess.
//!
//! Deviations: not upstream (tangency-labs/solidcore#3138). No recursion.

use crate::error::{Result, SolidError};
use crate::mass::integrator::properties::region_material;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::p2::{face_side_regions, faceuses, surface};
use crate::topology::{edge, Brep, EdgeId, FaceId, RegionId};

const OP: &str = "query::faces_continue";

/// What [`faces_continue`] proves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continues {
    /// The two faces lie on one surface, their outward sides the same: proved.
    Proved,
    /// Not proved either way: the surfaces' definitions differ in some number or kind (they
    /// may still be one surface), or a face has no side that faces out.
    Undecided,
}

/// Whether faces `f` and `g`, both bounded by edge `e`, lie on one surface with the same
/// outward side (see the module's proof).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `f` and `g` the same face or an edge that does not bound
/// both; `UnsureRegion` for a side the kernel is unsure is material; `NotFound` for a dangling
/// id.
pub fn faces_continue(b: &Brep, f: FaceId, g: FaceId, e: EdgeId) -> Result<Continues> {
    if f == g {
        return Err(SolidError::invalid_input(OP, "one face"));
    }
    for face in [f, g] {
        if edge::edgeuse_of_face(b, e, Some(face))?.is_none() {
            return Err(SolidError::invalid_input(
                OP,
                "the edge does not bound the face",
            ));
        }
    }
    let (Some(sf), Some(sg)) = (surface(b, f)?, surface(b, g)?) else {
        return Ok(Continues::Undecided);
    };
    if !same_definition(sf, sg) {
        return Ok(Continues::Undecided);
    }
    Ok(
        match (outward_orientation(b, f)?, outward_orientation(b, g)?) {
            (Some(a), Some(c)) if a == c && a != OrientType::Unknown => Continues::Proved,
            _ => Continues::Undecided,
        },
    )
}

/// Whether `a` and `b` have identical definitions, every number bit for bit.
fn same_definition(a: &Surface, b: &Surface) -> bool {
    let bits = |x: f64, y: f64| x.to_bits() == y.to_bits();
    match (a.kind(), b.kind()) {
        (SurfaceKind::Plane(p), SurfaceKind::Plane(q)) => p.position() == q.position(),
        (SurfaceKind::Cone(c), SurfaceKind::Cone(d)) => {
            c.rev.position == d.rev.position
                && c.rev.swap_uv == d.rev.swap_uv
                && bits(c.base_radius, d.base_radius)
                && bits(c.semi_angle_deg, d.semi_angle_deg)
                && c.inside_out == d.inside_out
                && c.cylinder == d.cylinder
        }
        (SurfaceKind::Sphere(s), SurfaceKind::Sphere(t)) => {
            s.base.position == t.base.position
                && s.base.swap_uv == t.base.swap_uv
                && bits(s.radius, t.radius)
                && s.inside_out == t.inside_out
        }
        (SurfaceKind::Torus(s), SurfaceKind::Torus(t)) => {
            s.base.position == t.base.position
                && s.base.swap_uv == t.base.swap_uv
                && bits(s.major_radius, t.major_radius)
                && bits(s.minor_radius, t.minor_radius)
                && s.inside_out == t.inside_out
        }
        _ => false,
    }
}

/// The orientation, to its surface, of face `f`'s use that faces out of its material: the
/// use on the void side where material is on one side only; for a sheet (material on
/// neither side) its primary use, the upward one; `None` with material on both sides or a
/// use missing.
fn outward_orientation(b: &Brep, f: FaceId) -> Result<Option<OrientType>> {
    let (Some(x), Some(y)) = faceuses(b, f)? else {
        return Ok(None);
    };
    let material = |r: Option<RegionId>| match r {
        Some(r) => region_material(b, r),
        None => Ok(false),
    };
    let [rx, ry] = face_side_regions(b, f)?;
    let out = match (material(rx)?, material(ry)?) {
        (true, false) => y,
        (false, _) => x,
        (true, true) => return Ok(None),
    };
    eu::faceuse_orientation(b, out).map(Some)
}
