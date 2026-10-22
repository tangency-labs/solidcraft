//! Shared checks for the `primitives` example and the `primitives_solid` test: build a
//! box, cylinder, cone and pyramid through the public API and prove each is a closed
//! manifold solid with the analytic volume, area and bounding box.

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives as api;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::face::p2 as fl;
use solidcore::topology::Brep;
use std::f64::consts::PI;

/// Relative tolerance of the analytic comparisons.
pub const REL_TOL: f64 = 1e-9;
/// Accuracy passed to the mass-property integrator (its lower clamp).
const ACCURACY: f64 = 1e-8;

/// One primitive and its analytic properties.
pub struct Spec {
    /// Printed name.
    pub name: &'static str,
    /// Analytic volume.
    pub volume: f64,
    /// Analytic surface area.
    pub area: f64,
    /// Analytic bounding box (min, max).
    pub bbox: (Point3, Point3),
    /// Expected (faces, edges, vertices), when fixed by the construction.
    pub counts: Option<(usize, usize, usize)>,
    build: fn(&mut Kernel) -> solidcore::Result<Brep>,
}

/// What [`check`] measured.
#[derive(Debug)]
pub struct Report {
    /// Faces, edges, vertices, loops.
    pub counts: (usize, usize, usize, usize),
    /// `is_manifold_solid`.
    pub manifold_solid: bool,
    /// Integrated volume.
    pub volume: f64,
    /// Integrated area.
    pub area: f64,
    /// Bounding box (min, max).
    pub bbox: (Point3, Point3),
}

/// Box 5 x 10 x 20, cylinder r 5 h 20, cone frustum 5 -> 2 h 20, pyramid 5 x h 20, all at
/// the origin.
pub fn specs() -> Vec<Spec> {
    let o = Point3::new(0.0, 0.0, 0.0);
    let (rb, rt, h) = (5.0_f64, 2.0_f64, 20.0_f64);
    let slant = ((rb - rt) * (rb - rt) + h * h).sqrt();
    let l = 5.0_f64;
    vec![
        Spec {
            name: "box 5x10x20",
            volume: 5.0 * 10.0 * 20.0,
            area: 2.0 * (5.0 * 10.0 + 5.0 * 20.0 + 10.0 * 20.0),
            bbox: (o, Point3::new(5.0, 10.0, 20.0)),
            counts: Some((6, 12, 8)),
            build: |k| api::create_box(k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0, 20.0),
        },
        Spec {
            name: "cylinder r5 h20",
            volume: PI * rb * rb * h,
            area: 2.0 * PI * rb * rb + 2.0 * PI * rb * h,
            bbox: (Point3::new(-rb, -rb, 0.0), Point3::new(rb, rb, h)),
            counts: None,
            build: |k| api::create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 20.0),
        },
        Spec {
            name: "cone r5->2 h20",
            volume: PI * h / 3.0 * (rb * rb + rb * rt + rt * rt),
            area: PI * (rb + rt) * slant + PI * rb * rb + PI * rt * rt,
            bbox: (Point3::new(-rb, -rb, 0.0), Point3::new(rb, rb, h)),
            counts: None,
            build: |k| api::create_cone(k, Point3::new(0.0, 0.0, 0.0), 5.0, 2.0, 20.0),
        },
        Spec {
            name: "pyramid 5 h20",
            volume: l * l * h / 3.0,
            area: l * l + 2.0 * l * ((l / 2.0) * (l / 2.0) + h * h).sqrt(),
            bbox: (
                Point3::new(-l / 2.0, -l / 2.0, 0.0),
                Point3::new(l / 2.0, l / 2.0, h),
            ),
            counts: Some((5, 8, 5)),
            build: |k| api::create_pyramid(k, Point3::new(0.0, 0.0, 0.0), 5.0, 20.0),
        },
    ]
}

fn rel_ok(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= REL_TOL * scale
}

/// Builds `spec` and checks it; `Err` names the first failed check.
pub fn check(spec: &Spec) -> Result<Report, String> {
    let mut k = Kernel::default();
    let b = (spec.build)(&mut k).map_err(|e| format!("build failed: {e:?}"))?;
    let err = |e: solidcore::SolidError| format!("{e:?}");
    let faces = bl::faces(&b, None).map_err(err)?;
    let edges = bl::edges(&b, None).map_err(err)?;
    let vertices = bl::vertices(&b, None).map_err(err)?;
    let mut loops = 0;
    for f in &faces {
        loops += fl::loops(&b, *f, false, None).map_err(err)?.len();
    }
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let opts = BoundingBoxOptions {
        approx: false,
        tight: true,
        expand_by_zone_tol_3d: false,
    };
    let ext = calculate_bounding_box(&k, &b, opts).map_err(err)?;
    let r = Report {
        counts: (faces.len(), edges.len(), vertices.len(), loops),
        manifold_solid: is_manifold_solid(&b).map_err(err)?,
        volume: brep_volume(&k, &b, ACCURACY, &attributes).map_err(err)?,
        area: brep_area(&k, &b, ACCURACY).map_err(err)?,
        bbox: (
            Point3::new(ext.min_at(0), ext.min_at(1), ext.min_at(2)),
            Point3::new(ext.max_at(0), ext.max_at(1), ext.max_at(2)),
        ),
    };
    let (f, e, v, lo) = r.counts;
    if !r.manifold_solid {
        return Err(format!("not a closed manifold solid: {r:?}"));
    }
    // Euler-Poincare for one closed genus-0 shell: V - E + F - (L - F) = 2. With one
    // loop per face (no rings) this is V - E + F = 2.
    let chi = v as i64 - e as i64 + f as i64 - (lo as i64 - f as i64);
    if chi != 2 {
        return Err(format!(
            "Euler-Poincare V-E+F-(L-F) = {chi}, expected 2: {r:?}"
        ));
    }
    if let Some(c) = spec.counts {
        if (f, e, v) != c || lo != f {
            return Err(format!(
                "counts {:?}, expected {c:?} with one loop per face",
                r.counts
            ));
        }
    }
    if !rel_ok(r.volume, spec.volume, spec.volume.abs()) {
        return Err(format!("volume {} vs analytic {}", r.volume, spec.volume));
    }
    if !rel_ok(r.area, spec.area, spec.area.abs()) {
        return Err(format!("area {} vs analytic {}", r.area, spec.area));
    }
    let size = (spec.bbox.1 - spec.bbox.0).length();
    for (got, want) in [(r.bbox.0, spec.bbox.0), (r.bbox.1, spec.bbox.1)] {
        let d: Vec3 = got - want;
        if !(rel_ok(d.x, 0.0, size) && rel_ok(d.y, 0.0, size) && rel_ok(d.z, 0.0, size)) {
            return Err(format!(
                "bounding box {:?} vs analytic {:?}",
                r.bbox, spec.bbox
            ));
        }
    }
    Ok(r)
}
