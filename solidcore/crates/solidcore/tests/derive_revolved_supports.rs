//! `AnalyticSupport::derive` reads a cylinder's, cone's and torus's numbers off their NURBS form's
//! control structure (#15010), so a support derived from its form builds on that form however
//! far from the world origin and however tilted it lies: the form, then derive, then build.
//! The forms are quarter cylinders and cones made of rational quadratic quarter circles (the
//! circle's own parametrisation, as `wr698_procedural_blend_definition` makes them) in a frame
//! tilted off every axis, at a distance of about 1e3 and 1e5 from the origin. And the cylinder
//! supports of `box_intersect_splits_the_end_off_a_raked_wedge_after_its_end_rounds` (SolidCraft's
//! engine regression), their forms as the blend definitions' records hold them
//! (`data/raked_wedge_supports.jsonl`), whose recognised numbers `build` refused, and those of
//! `the_pleats_join_at_every_wall_from_2_4_to_2_6_mm` (`data/pleated_case_supports.jsonl`).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::implicit::{AnalyticSupport, SupportKind};

/// A frame tilted off every world axis: its axis along (1, 2, 3), its x square to it.
fn tilted() -> (Vec3, Vec3, Vec3) {
    let z = Vec3::new(1.0, 2.0, 3.0);
    let z = z * (1.0 / z.length());
    let x = Vec3::new(3.0, -1.0, 0.0);
    let x = x - z * x.dot(z);
    let x = x * (1.0 / x.length());
    (x, z.cross(x), z)
}

/// A quarter of a surface of revolution about the frame's axis through `origin`: the quarter
/// circle of radius `r0` at height 0 and of radius `r1` at height `h` (a cylinder when they
/// are equal), each the rational quadratic arc with end weights 1.
fn quarter(origin: Vec3, r0: f64, r1: f64, h: f64) -> BSplineSurface {
    let (x, y, z) = tilted();
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let rows: Vec<Vec<CPoint>> = [(0.0, r0), (h, r1)]
        .iter()
        .map(|&(height, r)| {
            [(r, 0.0, 1.0), (r, r, w), (0.0, r, 1.0)]
                .iter()
                .map(|&(a, b, wt)| {
                    let p = origin + x * a + y * b + z * height;
                    CPoint::new(p.x * wt, p.y * wt, p.z * wt, wt)
                })
                .collect()
        })
        .collect();
    let nurb = solidcore::nurbs::srf_basic::Surface::new(
        rows,
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
    .unwrap();
    BSplineSurface::from_nurb(nurb, true)
}

/// The form, then derive, then build; the built support evaluates as the form within 64
/// ulps of the form's size.
fn round_trips(k: &Kernel, kind: SupportKind, form: &BSplineSurface, size: f64) {
    let derived = AnalyticSupport::derive(k, kind, form).unwrap();
    assert_eq!(derived.kind(), kind);
    let built = derived.build(k, form.copy()).unwrap();
    // at fractions of the form's own parameter box
    let d = form.natural_uv_domain();
    let at = |a: f64, b: f64| {
        Point2::new(
            d.min().x + (d.max().x - d.min().x) * a,
            d.min().y + (d.max().y - d.min().y) * b,
        )
    };
    for uv in [at(0.1, 0.2), at(0.5, 0.5), at(0.9, 0.8)] {
        let step = built.convert_uv_from_nurbs_to_step(k, uv).unwrap();
        let off = (built.evaluate_step_point(k, step).unwrap() - form.evaluate_point(uv).unwrap())
            .length();
        assert!(
            off <= 64.0 * f64::EPSILON * size,
            "{kind:?} at {size:e}: {off:e} off at {uv:?}"
        );
    }
}

#[test]
fn an_off_axis_tilted_cylinder_far_from_the_origin_round_trips() {
    let k = Kernel::default();
    for d in [1.0e3, 1.0e5] {
        let origin = Vec3::new(d * 0.6, -d * 0.48, d * 0.64);
        round_trips(
            &k,
            SupportKind::Cylinder,
            &quarter(origin, 1.5, 1.5, 12.0),
            d,
        );
    }
}

#[test]
fn an_off_axis_tilted_cone_far_from_the_origin_round_trips() {
    let k = Kernel::default();
    for d in [1.0e3, 1.0e5] {
        let origin = Vec3::new(-d * 0.36, d * 0.8, d * 0.48);
        round_trips(&k, SupportKind::Cone, &quarter(origin, 3.0, 1.0, 4.0), d);
    }
}

/// A tilted quarter cone up to its apex (#15010: the fillet rig's cone that runs to its tip,
/// `cov-fillet-exact-cone-cylinder-sharing-a-ball-r1`): the form's row at the apex is one point,
/// the axis's point of radius 0, at the form's end or start.
#[test]
fn an_off_axis_tilted_cone_up_to_its_apex_round_trips() {
    let k = Kernel::default();
    for d in [1.0e3, 1.0e5] {
        let origin = Vec3::new(d * 0.6, -d * 0.48, d * 0.64);
        round_trips(&k, SupportKind::Cone, &quarter(origin, 3.0, 0.0, 4.0), d);
        round_trips(&k, SupportKind::Cone, &quarter(origin, 0.0, 3.0, 4.0), d);
    }
}

/// A NURBS surface form as a blend definition's record holds it: control points weighted
/// when rational, u-major.
#[derive(serde::Deserialize)]
struct Form {
    kind: String,
    u_degree: usize,
    v_degree: usize,
    u_count: usize,
    v_count: usize,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    points: Vec<[f64; 3]>,
    weights: Option<Vec<f64>>,
}

impl Form {
    fn surface(&self) -> BSplineSurface {
        let rows: Vec<Vec<CPoint>> = (0..self.u_count)
            .map(|i| {
                (0..self.v_count)
                    .map(|j| {
                        let n = i * self.v_count + j;
                        let [x, y, z] = self.points[n];
                        let w = self
                            .weights
                            .as_ref()
                            .map_or(solidcore::nurbs::math::NOW, |w| w[n]);
                        CPoint::new(x, y, z, w)
                    })
                    .collect()
            })
            .collect();
        let nurb = solidcore::nurbs::srf_basic::Surface::new(
            rows,
            self.u_degree,
            self.v_degree,
            self.u_knots.clone(),
            self.v_knots.clone(),
        )
        .unwrap();
        BSplineSurface::from_nurb(nurb, true)
    }
}

#[test]
fn the_raked_wedges_cylinder_supports_round_trip() {
    let k = Kernel::default();
    let forms: Vec<Form> = include_str!("data/raked_wedge_supports.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(forms.len(), 10);
    for form in &forms {
        assert_eq!(form.kind, "Cylinder");
        round_trips(&k, SupportKind::Cylinder, &form.surface(), 100.0);
    }
}

/// The cylinder supports of SolidCraft's `the_pleats_join_at_every_wall_from_2_4_to_2_6_mm`
/// engine regression (a shelled case at walls of 2.4, 2.5 and 2.6 mm, with rounds), their forms
/// as derive met them (`data/pleated_case_supports.jsonl`). Among them is a shallow arc, 20 mm
/// across at a radius of 77.6 mm, whose rows' centres tilt their line by 1.3e-14: a cylinder's
/// axis is its columns' direction, which an extrusion stores exactly.
#[test]
fn the_pleated_cases_cylinder_supports_round_trip() {
    let k = Kernel::default();
    let forms: Vec<Form> = include_str!("data/pleated_case_supports.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(forms.len(), 31);
    for form in &forms {
        assert_eq!(form.kind, "Cylinder");
        round_trips(&k, SupportKind::Cylinder, &form.surface(), 300.0);
    }
}

/// Tensor-product rational arcs: a 120-degree sweep and a partial r8 tube about
/// the circle R33. The middle controls are tangent intersections, not samples.
#[test]
fn partial_tori_round_trip_on_their_control_circles() {
    let k = Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    };
    let (x, y, z) = tilted();
    for origin in [Vec3::ZERO, Vec3::new(600.0, -480.0, 640.0)] {
        for alpha in [0.6_f64.asin(), -0.6_f64.asin()] {
            for swap in [false, true] {
                let rows: Vec<Vec<CPoint>> = (0..5)
                    .map(|i| {
                        let u = f64::from(i) * std::f64::consts::PI / 6.0;
                        let wu = if i % 2 == 0 {
                            1.0
                        } else {
                            (std::f64::consts::PI / 6.0).cos()
                        };
                        let radial = (x * u.cos() + y * u.sin()) * (1.0 / wu);
                        (0..3)
                            .map(|j| {
                                let v = f64::from(j) * alpha / 2.0;
                                let wv = if j == 1 { (alpha / 2.0).cos() } else { 1.0 };
                                let p = origin
                                    + radial * (33.0 + 8.0 * v.cos() / wv)
                                    + z * (8.0 * v.sin() / wv);
                                let w = wu * wv;
                                CPoint::new(p.x * w, p.y * w, p.z * w, w)
                            })
                            .collect()
                    })
                    .collect();
                let (net, uk, vk) = if swap {
                    (
                        (0..3)
                            .map(|j| (0..5).map(|i| rows[i][j]).collect())
                            .collect(),
                        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                        vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
                    )
                } else {
                    (
                        rows,
                        vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
                        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    )
                };
                let nurb = solidcore::nurbs::srf_basic::Surface::new(net, 2, 2, uk, vk).unwrap();
                round_trips(
                    &k,
                    SupportKind::Torus,
                    &BSplineSurface::from_nurb(nurb, true),
                    origin.length() + 41.0,
                );
            }
        }
    }
}

#[test]
fn horn_tori_round_trip_from_a_collapsed_boundary_sweep() {
    let k = Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    };
    let (x, y, z) = tilted();
    let w = std::f64::consts::FRAC_1_SQRT_2;
    for origin in [Vec3::ZERO, Vec3::new(600.0, -480.0, 640.0)] {
        for sign in [-1.0, 1.0] {
            for swap in [false, true] {
                // The tube has centre radius 8 and radius 8. Its first point is
                // on the axis, so the entire first sweep boundary is one point.
                let rows: Vec<Vec<CPoint>> = [(1.0, 0.0, 1.0), (1.0, 1.0, w), (0.0, 1.0, 1.0)]
                    .into_iter()
                    .map(|(a, b, wu)| {
                        [(0.0, 0.0, 1.0), (0.0, 8.0, w), (8.0, 8.0, 1.0)]
                            .into_iter()
                            .map(|(radius, height, wv)| {
                                let p = origin + (x * a + y * b) * radius + z * (sign * height);
                                let weight = wu * wv;
                                CPoint::new(p.x * weight, p.y * weight, p.z * weight, weight)
                            })
                            .collect()
                    })
                    .collect();
                let net = if swap {
                    (0..3)
                        .map(|j| (0..3).map(|i| rows[i][j]).collect())
                        .collect()
                } else {
                    rows
                };
                let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
                let nurb =
                    solidcore::nurbs::srf_basic::Surface::new(net, 2, 2, knots.clone(), knots)
                        .unwrap();
                round_trips(
                    &k,
                    SupportKind::Torus,
                    &BSplineSurface::from_nurb(nurb, true),
                    origin.length() + 16.0,
                );
            }
        }
    }
}
