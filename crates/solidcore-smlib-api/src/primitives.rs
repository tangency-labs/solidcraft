//! The primitive solids: box, cylinder, sphere, cone and torus.
//!
//! Source: tangency-labs/smlib `capi/src/smlib_c.cpp` (`smlib_make_box`) and
//! `capi/src/smlib_primitives.cpp` (`smlib_make_cylinder`, `smlib_make_sphere`,
//! `smlib_make_cone`, `smlib_make_torus`), replay/1.0.1 at f8fd83e.
//!
//! | Rust | C API (`SM_API` call) |
//! |------|-------|
//! | [`Smlib::make_box`] | `smlib_make_box` (`SmApiCreateBox`) |
//! | [`Smlib::make_cylinder`] | `smlib_make_cylinder` (`SmApiCreateCylinder`) |
//! | [`Smlib::make_sphere`] | `smlib_make_sphere` (`SmApiCreateSphere`) |
//! | [`Smlib::make_cone`] | `smlib_make_cone` (`SmApiCreateCone`) |
//! | [`Smlib::make_torus`] | `smlib_make_torus` (`SmApiCreateTorus`) |
//!
//! Deviations: none; each is the C API's argument check, then solidcore's port of the
//! `SM_API` constructor, then `take`.

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{
    create_box, create_cone, create_cylinder, create_sphere, create_torus,
};
use solidcore::topology::Brep;

use crate::session::{finite, num, take, vec, Smlib};
use crate::status::{Result, Status};

/// `positive`: finite and above zero.
fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

fn point(p: [f64; 3]) -> Point3 {
    let [x, y, z] = p;
    Point3::new(x, y, z)
}

impl Smlib {
    /// `smlib_make_box`: an axis-aligned box from its lower corner `origin` with edge
    /// lengths `size` (each > 0).
    pub fn make_box(&mut self, origin: [f64; 3], size: [f64; 3]) -> Result<Brep> {
        let [l, w, h] = size;
        if !finite(&origin) || !finite(&size) || !positive(l) || !positive(w) || !positive(h) {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || {
                format!(
                    "smlib_make_box(origin={}, size={})",
                    vec(&origin),
                    vec(&size)
                )
            },
            &[],
            |api| take(create_box(api.kernel_mut(), point(origin), l, w, h)?),
        )
    }

    /// `smlib_make_cylinder`: a cylinder about +Z: its base circle's centre `origin`,
    /// `radius` and `height` (> 0).
    pub fn make_cylinder(&mut self, origin: [f64; 3], radius: f64, height: f64) -> Result<Brep> {
        if !finite(&origin) || !positive(radius) || !positive(height) {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || {
                format!(
                    "smlib_make_cylinder(origin={}, radius={}, height={})",
                    vec(&origin),
                    num(radius),
                    num(height)
                )
            },
            &[],
            |api| {
                take(create_cylinder(
                    api.kernel_mut(),
                    point(origin),
                    radius,
                    height,
                )?)
            },
        )
    }

    /// `smlib_make_sphere`: a sphere from its `center` and `radius` (> 0).
    pub fn make_sphere(&mut self, center: [f64; 3], radius: f64) -> Result<Brep> {
        if !finite(&center) || !positive(radius) {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || {
                format!(
                    "smlib_make_sphere(center={}, radius={})",
                    vec(&center),
                    num(radius)
                )
            },
            &[],
            |api| take(create_sphere(api.kernel_mut(), point(center), radius)?),
        )
    }

    /// `smlib_make_cone`: a cone or frustum about +Z: its base centre `origin`, base radius
    /// (> 0), top radius (>= 0, 0 for a pointed cone, different from the base radius) and
    /// height (> 0).
    pub fn make_cone(
        &mut self,
        origin: [f64; 3],
        base_radius: f64,
        top_radius: f64,
        height: f64,
    ) -> Result<Brep> {
        if !finite(&origin)
            || !positive(base_radius)
            || !top_radius.is_finite()
            || top_radius < 0.0
            || top_radius == base_radius
            || !positive(height)
        {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || {
                format!(
                    "smlib_make_cone(origin={}, base_radius={}, top_radius={}, height={})",
                    vec(&origin),
                    num(base_radius),
                    num(top_radius),
                    num(height)
                )
            },
            &[],
            |api| {
                take(create_cone(
                    api.kernel_mut(),
                    point(origin),
                    base_radius,
                    top_radius,
                    height,
                )?)
            },
        )
    }

    /// `smlib_make_torus`: a ring torus about +Z from its `center`, with major radius >
    /// minor radius > 0.
    pub fn make_torus(
        &mut self,
        center: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
    ) -> Result<Brep> {
        if !finite(&center)
            || !positive(major_radius)
            || !positive(minor_radius)
            || minor_radius >= major_radius
        {
            return Err(Status::ERR_ARG);
        }
        self.run(
            || {
                format!(
                    "smlib_make_torus(center={}, major_radius={}, minor_radius={})",
                    vec(&center),
                    num(major_radius),
                    num(minor_radius)
                )
            },
            &[],
            |api| {
                take(create_torus(
                    api.kernel_mut(),
                    point(center),
                    major_radius,
                    minor_radius,
                )?)
            },
        )
    }
}
