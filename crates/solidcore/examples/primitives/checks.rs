//! Shared checks for the `primitives` example and the `primitives_solid` test: build a
//! box, cylinder, cone and pyramid, and a sphere and a torus with their variants, through
//! the public API and prove each is a closed manifold solid with the analytic
//! Euler-Poincare characteristic, volume, area and bounding box.

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{
    brep_properties, MassPropertyBehavior, PropertyAttributes,
};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives as api;
use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge as el;
use solidcore::topology::face::p2 as fl;
use solidcore::topology::Brep;
use std::f64::consts::PI;

/// Relative tolerance of the analytic comparisons.
pub const REL_TOL: f64 = 1e-9;
/// Requested integration accuracy, tighter than the independent analytic check.
/// The `SM_API` area/volume wrappers clamp requests to coarser values.
const ACCURACY: f64 = 1e-10;

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
    /// Euler-Poincare characteristic `2 * (shells - genus)`: 2 for a ball, 0 for a
    /// solid torus.
    pub euler: i64,
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
            euler: 2,
            build: |k| api::create_box(k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0, 20.0),
        },
        Spec {
            name: "cylinder r5 h20",
            volume: PI * rb * rb * h,
            area: 2.0 * PI * rb * rb + 2.0 * PI * rb * h,
            bbox: (Point3::new(-rb, -rb, 0.0), Point3::new(rb, rb, h)),
            counts: None,
            euler: 2,
            build: |k| api::create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 20.0),
        },
        Spec {
            name: "cone r5->2 h20",
            volume: PI * h / 3.0 * (rb * rb + rb * rt + rt * rt),
            area: PI * (rb + rt) * slant + PI * rb * rb + PI * rt * rt,
            bbox: (Point3::new(-rb, -rb, 0.0), Point3::new(rb, rb, h)),
            counts: None,
            euler: 2,
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
            euler: 2,
            build: |k| api::create_pyramid(k, Point3::new(0.0, 0.0, 0.0), 5.0, 20.0),
        },
    ]
}

/// Sphere r 3 (with poles, without poles, and its 0..90 degree wedge) and torus R 5 r 1.5
/// (whole and its 0..90 degree sector), centred off the origin.
pub fn curved_specs() -> Vec<Spec> {
    let (r, big, small) = (3.0_f64, 5.0_f64, 1.5_f64);
    let o = big + small;
    let sphere_box = (
        Point3::new(1.0 - r, -2.0 - r, 0.5 - r),
        Point3::new(1.0 + r, -2.0 + r, 0.5 + r),
    );
    vec![
        Spec {
            name: "sphere r3",
            volume: 4.0 / 3.0 * PI * r * r * r,
            area: 4.0 * PI * r * r,
            bbox: sphere_box,
            counts: None,
            euler: 2,
            build: |k| api::create_sphere(k, Point3::new(1.0, -2.0, 0.5), 3.0),
        },
        Spec {
            name: "sphere r3 (no pole)",
            volume: 4.0 / 3.0 * PI * r * r * r,
            area: 4.0 * PI * r * r,
            bbox: sphere_box,
            counts: None,
            euler: 2,
            build: |k| {
                api::create_sphere_no_pole_with_tolerance(k, Point3::new(1.0, -2.0, 0.5), 3.0, 1e-9)
            },
        },
        Spec {
            name: "partial sphere r3 0..90",
            // a quarter of the ball; a quarter of the sphere plus two half discs
            volume: PI * r * r * r / 3.0,
            area: PI * r * r + PI * r * r,
            bbox: (
                Point3::new(1.0, -2.0, 0.5 - r),
                Point3::new(1.0 + r, -2.0 + r, 0.5 + r),
            ),
            counts: None,
            euler: 2,
            build: |k| api::create_partial_sphere(k, Point3::new(1.0, -2.0, 0.5), 3.0, 0.0, 90.0),
        },
        Spec {
            name: "torus R5 r1.5",
            volume: 2.0 * PI * PI * big * small * small,
            area: 4.0 * PI * PI * big * small,
            bbox: (
                Point3::new(-o, 1.0 - o, -1.0 - small),
                Point3::new(o, 1.0 + o, -1.0 + small),
            ),
            counts: None,
            euler: 0,
            build: |k| api::create_torus(k, Point3::new(0.0, 1.0, -1.0), 5.0, 1.5),
        },
        Spec {
            name: "partial torus R5 r1.5 0..90",
            // a quarter of the ring plus its two end discs
            volume: PI * PI * big * small * small / 2.0,
            area: PI * PI * big * small + 2.0 * PI * small * small,
            bbox: (
                Point3::new(0.0, 1.0, -1.0 - small),
                Point3::new(o, 1.0 + o, -1.0 + small),
            ),
            counts: None,
            euler: 2,
            build: |k| {
                api::create_partial_torus(k, Point3::new(0.0, 1.0, -1.0), 5.0, 1.5, 0.0, 90.0)
            },
        },
    ]
}

fn rel_ok(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= REL_TOL * scale
}

/// A value one check compared.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    /// A yes/no property.
    Bool(bool),
    /// An integer (Euler-Poincare characteristic).
    Int(i64),
    /// A real quantity (volume, area).
    Num(f64),
    /// (faces, edges, vertices).
    Counts(usize, usize, usize),
    /// Bounding box (min, max).
    BBox(Point3, Point3),
}

/// One check of a built primitive.
#[derive(Debug, Clone)]
pub struct Check {
    /// Short id: `manifold`, `euler`, `counts`, `volume`, `area` or `bbox`.
    pub name: &'static str,
    /// Whether it passed.
    pub pass: bool,
    /// Analytic value.
    pub expected: Value,
    /// Measured value; `None` when measuring it failed.
    pub actual: Option<Value>,
    /// Relative error of a real-valued check (bbox: largest corner deviation over the
    /// analytic diagonal).
    pub rel_err: Option<f64>,
    /// Why it failed; empty when it passed.
    pub message: String,
}

/// What measuring a built primitive returned, one result per measurement.
#[derive(Debug)]
pub struct Measured {
    /// Faces, edges, vertices, loops.
    pub counts: solidcore::Result<(usize, usize, usize, usize)>,
    /// `is_manifold_solid`.
    pub manifold_solid: solidcore::Result<bool>,
    /// Integrated volume.
    pub volume: solidcore::Result<f64>,
    /// Integrated area.
    pub area: solidcore::Result<f64>,
    /// Bounding box (min, max).
    pub bbox: solidcore::Result<(Point3, Point3)>,
}

/// Everything [`evaluate`] found out about one primitive.
#[derive(Debug)]
pub struct Evaluation {
    /// The build's error, if it failed (then `measured` is `None`).
    pub build_error: Option<solidcore::SolidError>,
    /// Measurements of the built solid.
    pub measured: Option<Measured>,
    /// Every check, in order; empty when the build failed.
    pub checks: Vec<Check>,
}

impl Evaluation {
    /// Built and every check passed.
    pub fn pass(&self) -> bool {
        self.build_error.is_none() && !self.checks.is_empty() && self.checks.iter().all(|c| c.pass)
    }

    /// The first error met, build first, then measurements in [`check`]'s order.
    pub fn first_error(&self) -> Option<&solidcore::SolidError> {
        if let Some(e) = &self.build_error {
            return Some(e);
        }
        let m = self.measured.as_ref()?;
        m.counts
            .as_ref()
            .err()
            .or(m.bbox.as_ref().err())
            .or(m.manifold_solid.as_ref().err())
            .or(m.volume.as_ref().err())
            .or(m.area.as_ref().err())
    }

    /// `(unit, upstream symbol)` of the first unported call that stopped this primitive.
    pub fn blocker(&self) -> Option<(&'static str, &'static str)> {
        let first = self
            .build_error
            .iter()
            .chain(self.measured.iter().flat_map(|m| {
                [
                    m.counts.as_ref().err(),
                    m.bbox.as_ref().err(),
                    m.manifold_solid.as_ref().err(),
                    m.volume.as_ref().err(),
                    m.area.as_ref().err(),
                ]
                .into_iter()
                .flatten()
            }));
        for e in first {
            if let solidcore::SolidError::Unported { upstream, unit } = e {
                return Some((unit, upstream));
            }
        }
        None
    }
}

fn measure(k: &Kernel, b: &mut Brep) -> Measured {
    let counts = (|| {
        let faces = bl::faces(b, None)?;
        let edges = bl::edges(b, None)?;
        let vertices = bl::vertices(b, None)?;
        let mut loops = 0;
        for f in &faces {
            loops += fl::loops(b, *f, false, None)?.len();
        }
        Ok((faces.len(), edges.len(), vertices.len(), loops))
    })();
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
    let bbox = calculate_bounding_box(k, &mut *b, opts).map(|ext| {
        (
            Point3::new(ext.min_at(0), ext.min_at(1), ext.min_at(2)),
            Point3::new(ext.max_at(0), ext.max_at(1), ext.max_at(2)),
        )
    });
    let properties =
        calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).and_then(|ext| {
            brep_properties(
                k,
                b,
                ACCURACY,
                ext.mid(),
                MassPropertyBehavior::default(),
                &attributes,
            )
        });
    Measured {
        counts,
        manifold_solid: is_manifold_solid(b),
        volume: properties.as_ref().map(|p| p.volume).map_err(Clone::clone),
        area: properties.map(|p| p.area),
        bbox,
    }
}

fn real_check(name: &'static str, got: &solidcore::Result<f64>, want: f64) -> Check {
    let (actual, rel_err, pass, message) = match got {
        Ok(v) => {
            let pass = rel_ok(*v, want, want.abs());
            let msg = if pass {
                String::new()
            } else {
                format!("{name} {v} vs analytic {want}")
            };
            (
                Some(Value::Num(*v)),
                Some((v - want).abs() / want.abs()),
                pass,
                msg,
            )
        }
        Err(e) => (None, None, false, format!("{e:?}")),
    };
    Check {
        name,
        pass,
        expected: Value::Num(want),
        actual,
        rel_err,
        message,
    }
}

fn unmeasured(name: &'static str, expected: Value, e: &solidcore::SolidError) -> Check {
    Check {
        name,
        pass: false,
        expected,
        actual: None,
        rel_err: None,
        message: format!("{e:?}"),
    }
}

/// Builds `spec`, measures it and runs every check (none stops the others).
pub fn evaluate(spec: &Spec) -> Evaluation {
    let mut k = Kernel::default();
    // SMLib 1.0.1's precise properties cannot converge at ACCURACY on a sphere (WR-472);
    // the opt-in fix holds every component to the area's share of its size.
    k.fixes.precise_properties_body_length_scale = true;
    let mut b = match (spec.build)(&mut k) {
        Ok(b) => b,
        Err(e) => {
            return Evaluation {
                build_error: Some(e),
                measured: None,
                checks: Vec::new(),
            }
        }
    };
    let m = measure(&k, &mut b);
    let mut checks = Vec::new();
    checks.push(match &m.manifold_solid {
        Ok(ok) => Check {
            name: "manifold",
            pass: *ok,
            expected: Value::Bool(true),
            actual: Some(Value::Bool(*ok)),
            rel_err: None,
            message: if *ok {
                String::new()
            } else {
                "not a closed manifold solid".to_string()
            },
        },
        Err(e) => unmeasured("manifold", Value::Bool(true), e),
    });
    match &m.counts {
        Ok((f, e, v, lo)) => {
            // Euler-Poincare for one closed shell of genus g: V - E + F - (L - F) =
            // 2 - 2g. With one loop per face (no rings) this is V - E + F.
            let chi = *v as i64 - *e as i64 + *f as i64 - (*lo as i64 - *f as i64);
            checks.push(Check {
                name: "euler",
                pass: chi == spec.euler,
                expected: Value::Int(spec.euler),
                actual: Some(Value::Int(chi)),
                rel_err: None,
                message: if chi == spec.euler {
                    String::new()
                } else {
                    format!(
                        "Euler-Poincare V-E+F-(L-F) = {chi}, expected {}",
                        spec.euler
                    )
                },
            });
            if let Some(c) = spec.counts {
                let pass = (*f, *e, *v) == c && lo == f;
                checks.push(Check {
                    name: "counts",
                    pass,
                    expected: Value::Counts(c.0, c.1, c.2),
                    actual: Some(Value::Counts(*f, *e, *v)),
                    rel_err: None,
                    message: if pass {
                        String::new()
                    } else {
                        format!(
                            "counts {:?}, expected {c:?} with one loop per face",
                            (f, e, v, lo)
                        )
                    },
                });
            }
        }
        Err(err) => {
            checks.push(unmeasured("euler", Value::Int(spec.euler), err));
            if let Some(c) = spec.counts {
                checks.push(unmeasured("counts", Value::Counts(c.0, c.1, c.2), err));
            }
        }
    }
    checks.push(real_check("volume", &m.volume, spec.volume));
    checks.push(real_check("area", &m.area, spec.area));
    let want = Value::BBox(spec.bbox.0, spec.bbox.1);
    checks.push(match &m.bbox {
        Ok(got) => {
            let size = (spec.bbox.1 - spec.bbox.0).length();
            let mut pass = true;
            let mut dev = 0.0_f64;
            for (g, w) in [(got.0, spec.bbox.0), (got.1, spec.bbox.1)] {
                let d: Vec3 = g - w;
                if !(rel_ok(d.x, 0.0, size) && rel_ok(d.y, 0.0, size) && rel_ok(d.z, 0.0, size)) {
                    pass = false;
                }
                dev = dev.max(d.x.abs()).max(d.y.abs()).max(d.z.abs());
            }
            Check {
                name: "bbox",
                pass,
                expected: want,
                actual: Some(Value::BBox(got.0, got.1)),
                rel_err: Some(dev / size),
                message: if pass {
                    String::new()
                } else {
                    format!("bounding box {got:?} vs analytic {:?}", spec.bbox)
                },
            }
        }
        Err(e) => unmeasured("bbox", want, e),
    });
    Evaluation {
        build_error: None,
        measured: Some(m),
        checks,
    }
}

/// Builds `spec` and checks it; `Err` names the first failed check.
#[allow(dead_code)] // the primitives_solid test's entry point; the example uses `report`
pub fn check(spec: &Spec) -> Result<Report, String> {
    report(&evaluate(spec))
}

/// [`check`]'s verdict on an [`Evaluation`]: the measurements, or the first failure.
pub fn report(ev: &Evaluation) -> Result<Report, String> {
    if let Some(e) = &ev.build_error {
        return Err(format!("build failed: {e:?}"));
    }
    if let Some(e) = ev.first_error() {
        return Err(format!("{e:?}"));
    }
    let r = match &ev.measured {
        Some(Measured {
            counts: Ok(counts),
            manifold_solid: Ok(manifold_solid),
            volume: Ok(volume),
            area: Ok(area),
            bbox: Ok(bbox),
        }) => Report {
            counts: *counts,
            manifold_solid: *manifold_solid,
            volume: *volume,
            area: *area,
            bbox: *bbox,
        },
        _ => return Err("not measured".to_string()),
    };
    match ev.checks.iter().find(|c| !c.pass) {
        // The manifold and Euler messages carry the measurements, as before.
        Some(c) if matches!(c.name, "manifold" | "euler") => Err(format!("{}: {r:?}", c.message)),
        Some(c) => Err(c.message.clone()),
        None => Ok(r),
    }
}

/// Builds `spec` and samples the 3D curve of each of its edges at `samples` parameters
/// spread evenly over the edge's interval (both ends included). An edge without a curve,
/// or whose curve fails to evaluate anywhere, is skipped. `None` when the build fails.
#[allow(dead_code)] // the example's `--wire` export; the test does not use it
pub fn wireframe(spec: &Spec, samples: usize) -> Option<Vec<Vec<Point3>>> {
    let mut k = Kernel::default();
    let b = (spec.build)(&mut k).ok()?;
    let Ok(edges) = bl::edges(&b, None) else {
        return Some(Vec::new());
    };
    let n = samples.max(2);
    let mut wire = Vec::new();
    for e in edges {
        let (Ok(Some(curve)), Ok(ivl)) = (el::curve(&b, e), el::interval(&b, e)) else {
            continue;
        };
        let (t0, t1) = (ivl.min(), ivl.max());
        let points: Option<Vec<Point3>> = (0..n)
            .map(|i| {
                let t = t0 + (t1 - t0) * i as f64 / (n - 1) as f64;
                curve.evaluate_point(&k, t).ok()
            })
            .collect();
        if let Some(points) = points {
            wire.push(points);
        }
    }
    Some(wire)
}
