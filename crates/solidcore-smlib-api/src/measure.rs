//! Exact measures: volume, area, mass properties and the tight box.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_measure.cpp` (`faces_area`, `sheet_face`,
//! `smlib_brep_mass_properties`, `smlib_brep_area`, `smlib_brep_bounds`) and
//! `capi/src/smlib_c.cpp` (`smlib_brep_volume`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API (SMLib call) |
//! |------|-------|
//! | [`Smlib::brep_volume`] | `smlib_brep_volume` (`SmApiBrepComputeVolume`) |
//! | [`Smlib::brep_area`] | `smlib_brep_area` (`faces_area`: `SmFace::ComputePreciseProperties` per face) |
//! | [`Smlib::brep_mass_properties`] | `smlib_brep_mass_properties` (`SmBrep::ComputePreciseProperties`) |
//! | [`Smlib::brep_bounds`] | `smlib_brep_bounds` (`SmBrep::CalculateTightBoundingBox`) |
//! | `faces_area`, `sheet_face` (private) | `faces_area`, `sheet_face` |
//!
//! Deviations: the measures run on a copy that keeps its geometry owners
//! (`Brep::clone_standing_for_self`): SMLib makes missing UV trims on the `const` body it
//! is given, solidcore on the `&mut` body. On that copy, `make_missing_nurbs` first gives
//! every analytic face surface its NURB: SMLib's bounding box makes it lazily, solidcore's
//! `const` box cannot (plan/findings/query.brep--boolean-results-need-their-analytic-nurbs.md).
//! The check for `SM_AI_MASS_PROPERTIES` attributes is not made: a body of this API
//! carries no attributes. `smlib_brep_edge_face_distance` (`SmCrvSrfGapFunction`) follows.

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{
    brep_properties, brep_volume, face_properties, MassPropertyBehavior, PropertyAttributes,
};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::{brep_bounding_box, face_bounding_box};
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::topology::brep::p1::regions;
use solidcore::topology::edgeuse::p1::faceuse_region;
use solidcore::topology::face::p2::{faceuses, PPF_AREA, PPF_AREA_FIRST_MOMENTS};
use solidcore::topology::{Brep, FaceId, FaceuseId};
use solidcore::Kernel;

use crate::session::{finite, Fail, Smlib};
use crate::status::{Result, Status};

/// The C API's accuracy envelope: `relative_accuracy` clamped to `[1e-8, 1e-1]`.
fn clamp_accuracy(relative_accuracy: f64) -> f64 {
    relative_accuracy.clamp(1e-8, 1e-1)
}

impl Smlib {
    /// `smlib_brep_volume`: the enclosed volume at `relative_accuracy` (clamped by SMLib
    /// to `[1e-8, 1e-1]`).
    pub fn brep_volume(&mut self, brep: &Brep, relative_accuracy: f64) -> Result<f64> {
        if !relative_accuracy.is_finite() {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || format!("smlib_brep_volume(brep, relative_accuracy={relative_accuracy:?})"),
            &[("brep", brep)],
            |api| {
                let mut b = measured(api.kernel(), brep)?;
                let store = AttributeStore::default();
                let attributes = PropertyAttributes {
                    store: &store,
                    key_of: &|_| ObjectKey(0),
                };
                Ok(brep_volume(
                    api.kernel(),
                    &mut b,
                    relative_accuracy,
                    &attributes,
                )?)
            },
        )
    }

    /// `smlib_brep_area`: the total area of every face, each counted once (sheets and open
    /// shells too), at `relative_accuracy` clamped to `[1e-8, 1e-1]`.
    pub fn brep_area(&mut self, brep: &Brep, relative_accuracy: f64) -> Result<f64> {
        if !relative_accuracy.is_finite() {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || format!("smlib_brep_area(brep, relative_accuracy={relative_accuracy:?})"),
            &[("brep", brep)],
            |api| {
                let accuracy = clamp_accuracy(relative_accuracy);
                let mut b = measured(api.kernel(), brep)?;
                let (area, _) = faces_area(api.kernel(), &mut b, accuracy, Point3::ZERO, None)?;
                Ok(area)
            },
        )
    }

    /// `smlib_brep_mass_properties`: the exact mass properties at unit density, at
    /// `relative_accuracy` clamped to `[1e-8, 1e-1]`: `[0]` area, `[1]` volume, `[2..5)`
    /// centroid, `[5..8)` moments of inertia `Ixx, Iyy, Izz` about the centroid, `[8..11)`
    /// products of inertia `Iyz, Izx, Ixy` about the centroid. A body with no material
    /// region (a sheet, an open shell) has volume 0, no inertia, and its area's centroid.
    pub fn brep_mass_properties(
        &mut self,
        brep: &Brep,
        relative_accuracy: f64,
    ) -> Result<[f64; 11]> {
        if !relative_accuracy.is_finite() {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || format!("smlib_brep_mass_properties(brep, relative_accuracy={relative_accuracy:?})"),
            &[("brep", brep)],
            |api| mass_properties(api.kernel(), brep, relative_accuracy),
        )
    }

    /// `smlib_brep_bounds`: the exact axis-aligned box of `brep` (the extrema of its
    /// vertices, edges and faces on their exact geometry), as `(lo, hi)`. A body with no
    /// geometry is `SM_ERR_INVALID_INPUT`.
    pub fn brep_bounds(&mut self, brep: &Brep) -> Result<([f64; 3], [f64; 3])> {
        self.run(
            || "smlib_brep_bounds(brep)".to_string(),
            &[("brep", brep)],
            |api| {
                let b = measured(api.kernel(), brep)?;
                let bx = brep_bounding_box(api.kernel(), &b, true)?;
                let r = [
                    bx.u_min(),
                    bx.v_min(),
                    bx.w_min(),
                    bx.u_max(),
                    bx.v_max(),
                    bx.w_max(),
                ];
                let [x0, y0, z0, x1, y1, z1] = r;
                if !finite(&r) || x0 > x1 || y0 > y1 || z0 > z1 {
                    return Err(Fail::Status(Status::SM_ERR_INVALID_INPUT));
                }
                Ok(([x0, y0, z0], [x1, y1, z1]))
            },
        )
    }
}

/// The copy a measure runs on (see the module's deviations).
fn measured(k: &Kernel, brep: &Brep) -> core::result::Result<Brep, Fail> {
    let mut b = brep.clone_standing_for_self()?;
    make_missing_nurbs(k, &mut b)?;
    Ok(b)
}

/// Makes the NURB of every analytic face surface left without one (as `difftest`'s
/// SolidCraft runner does before its measures).
fn make_missing_nurbs(k: &Kernel, b: &mut Brep) -> solidcore::Result<()> {
    for f in b.face_list().to_vec() {
        let Ok(face) = b.face_mut(f) else { continue };
        let Some(surface) = face.surface.as_mut() else {
            continue;
        };
        let missing = match surface.kind() {
            SurfaceKind::Plane(s) => s.base().gw_nurb().is_none(),
            SurfaceKind::Cone(s) => s.rev().base().gw_nurb().is_none(),
            SurfaceKind::Sphere(s) => s.base().base().gw_nurb().is_none(),
            SurfaceKind::Torus(s) => s.base().base().gw_nurb().is_none(),
            SurfaceKind::SurfOfRevolution(s) => s.base().gw_nurb().is_none(),
            SurfaceKind::SurfOfExtrusion(s) => s.base().gw_nurb().is_none(),
            SurfaceKind::BSpline(_)
            | SurfaceKind::CurveBounded(_)
            | SurfaceKind::Step(_)
            | SurfaceKind::OffsetSurface(_)
            | SurfaceKind::SrfInVolume(_) => false,
        };
        if !missing {
            continue;
        }
        match surface.kind_mut() {
            SurfaceKind::Plane(s) => s.make_nurb()?,
            SurfaceKind::Cone(s) => s.make_nurb(k)?,
            SurfaceKind::Sphere(s) => s.make_nurb(k)?,
            SurfaceKind::Torus(s) => s.make_nurb(k)?,
            SurfaceKind::SurfOfRevolution(s) => s.make_nurb(k)?,
            SurfaceKind::SurfOfExtrusion(s) => s.make_nurb(k)?,
            _ => {}
        }
    }
    Ok(())
}

/// A face filter for [`faces_area`].
type FaceFilter<'a> = &'a dyn Fn(&Brep, FaceId) -> solidcore::Result<bool>;

/// `faces_area`: every face counted once (`SmApiBrepComputeArea` with the `[1e-8, 1e-1]`
/// envelope): the total area and, with `first`, the area's first moments about `o`.
/// `counted` limits the faces.
fn faces_area(
    k: &Kernel,
    b: &mut Brep,
    accuracy: f64,
    o: Point3,
    counted: Option<FaceFilter<'_>>,
) -> core::result::Result<(f64, Vec3), Fail> {
    let mut area = 0.0;
    let mut sum = Vec3::new(0.0, 0.0, 0.0);
    let first = counted.is_none();
    let flags = if first {
        PPF_AREA | PPF_AREA_FIRST_MOMENTS
    } else {
        PPF_AREA
    };
    for f in b.face_list().to_vec() {
        if let Some(counted) = counted {
            if !counted(b, f)? {
                continue;
            }
        }
        let size = face_bounding_box(k, b, f, false)?.size();
        let edge = size.z.max(size.x.max(size.y));
        let p = face_properties(
            k,
            b,
            f,
            OrientType::Same,
            accuracy,
            o,
            edge * edge / 100.0,
            0.0,
            flags,
        )?;
        // The area and its moments share one integrand, so a trim boundary integrated the
        // other way round flips both.
        let sign = if p.area < 0.0 { -1.0 } else { 1.0 };
        area += sign * p.area;
        if first {
            if let Some(m) = p.moments.first() {
                sum += *m * sign;
            }
        }
    }
    if !area.is_finite() {
        return Err(Fail::Status(Status::SM_ERR_INVALID_INPUT));
    }
    Ok((area, sum))
}

/// `sheet_face`: a face with material on neither side (a sheet face, standing free or hung
/// on a solid).
fn sheet_face(b: &Brep, f: FaceId) -> solidcore::Result<bool> {
    let (a, c) = faceuses(b, f)?;
    let void_side = |u: Option<FaceuseId>| -> solidcore::Result<bool> {
        let Some(u) = u else { return Ok(true) };
        Ok(match faceuse_region(b, u)? {
            Some(r) => b.region(r)?.is_void() == 1,
            None => true,
        })
    };
    Ok(void_side(a)? && void_side(c)?)
}

/// `smlib_brep_mass_properties`' body.
fn mass_properties(
    k: &Kernel,
    brep: &Brep,
    relative_accuracy: f64,
) -> core::result::Result<[f64; 11], Fail> {
    let invalid = || Fail::Status(Status::SM_ERR_INVALID_INPUT);
    let mut b = measured(k, brep)?;
    let mut material = false;
    for r in regions(&b, None)? {
        if b.region(r)?.is_void() == 0 {
            material = true;
            break;
        }
    }
    let accuracy = clamp_accuracy(relative_accuracy);
    // Moments are taken about the middle of the body's box and moved to the centroid
    // below: about the world origin, a body far from it would lose its inertia to
    // cancellation.
    let o = brep_bounding_box(k, &b, false)?.mid();
    if !finite(&[o.x, o.y, o.z]) {
        return Err(invalid());
    }
    if !material {
        let (area, first) = faces_area(k, &mut b, accuracy, o, None)?;
        if area.is_nan() || area <= 0.0 {
            return Err(invalid());
        }
        let r = [
            area,
            0.0,
            o.x + first.x / area,
            o.y + first.y / area,
            o.z + first.z / area,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        return if finite(&r) { Ok(r) } else { Err(invalid()) };
    }
    let store = AttributeStore::default();
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let p = brep_properties(
        k,
        &mut b,
        accuracy,
        o,
        MassPropertyBehavior::default(),
        &attributes,
    )?;
    let (mass, volume) = (p.mass, p.volume);
    if !mass.is_finite() || mass <= 0.0 || volume.is_nan() || volume <= 0.0 {
        return Err(invalid());
    }
    // The area of every face of the body: the solid's boundary and the sheet faces.
    let counted: FaceFilter<'_> = &sheet_face;
    let (sheets, _) = faces_area(k, &mut b, accuracy, o, Some(counted))?;
    let area = p.area + sheets;
    // moments[4]: first moments about o; [7]: (y^2+z^2, z^2+x^2, x^2+y^2) about o;
    // [6]: (yz, zx, xy) about o. Parallel axes from o to the centroid (unit density:
    // mass = volume).
    let [_, _, _, _, m4, _, m6, m7] = p.moments;
    let (d0, d1, d2) = (m4.x / mass, m4.y / mass, m4.z / mass);
    let r = [
        area,
        volume,
        o.x + d0,
        o.y + d1,
        o.z + d2,
        m7.x - mass * (d1 * d1 + d2 * d2),
        m7.y - mass * (d0 * d0 + d2 * d2),
        m7.z - mass * (d0 * d0 + d1 * d1),
        m6.x - mass * d1 * d2,
        m6.y - mass * d2 * d0,
        m6.z - mass * d0 * d1,
    ];
    if finite(&r) {
        Ok(r)
    } else {
        Err(invalid())
    }
}
