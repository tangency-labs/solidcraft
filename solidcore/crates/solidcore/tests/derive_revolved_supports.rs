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
//! And two surfaces of revolution swept under half a degree, a chained round's short pieces
//! (`data/short_swept_revolution_nets.jsonl`), and one of the same round swept 286 degrees in
//! four spans (`data/long_swept_revolution_net.jsonl`), derived with `derive_revolution`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::surface::implicit::{lies_on, AnalyticSupport, Implicit, SupportKind};

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

    /// The same form with u and v swapped.
    fn transposed(&self) -> BSplineSurface {
        let rows: Vec<Vec<CPoint>> = (0..self.v_count)
            .map(|j| {
                (0..self.u_count)
                    .map(|i| {
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
            self.v_degree,
            self.u_degree,
            self.v_knots.clone(),
            self.u_knots.clone(),
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

/// A shallow circular strip amplifies rounding when its centre is recovered from the
/// sagitta. Recovering the radius independently then leaves its endpoints on opposite
/// sides of the reconstructed cylinder (#16172).
#[test]
fn a_shallow_cylinders_endpoints_stay_on_the_derived_support() {
    let sweep = 0.02_f64;
    let k = Kernel::default();
    let (x, y, z) = tilted();
    let radius = 1000.0;
    let start = 2.0;
    let weight = (sweep * 0.5).cos();
    let rows = [0.0, 12.0]
        .into_iter()
        .map(|height| {
            [
                (start, 1.0),
                (start + sweep * 0.5, weight),
                (start + sweep, 1.0),
            ]
            .into_iter()
            .map(|(angle, w)| {
                let p = (x * angle.cos() + y * angle.sin()) * (radius / w) + z * height;
                CPoint::new(p.x * w, p.y * w, p.z * w, w)
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
    let form = BSplineSurface::from_nurb(nurb, true);
    // Certify the constructed form against its intended cylinder before recognition:
    // the shallow sweep must not hide malformed weights or control points.
    assert!(lies_on(
        &form,
        &Implicit::Cone {
            origin: Vec3::ZERO,
            axis: z,
            radius,
            tan_angle: 0.0,
        },
    )
    .unwrap());
    let support = AnalyticSupport::derive(&k, SupportKind::Cylinder, &form).unwrap();
    assert_eq!(support.kind(), SupportKind::Cylinder);
    if let AnalyticSupport::Cylinder { placement, radius } = support {
        let implicit = Implicit::Cone {
            origin: placement.origin(),
            axis: placement.x_axis().cross(placement.y_axis()),
            radius,
            tan_angle: 0.0,
        };
        assert!(lies_on(&form, &implicit).unwrap());
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

/// The two short pieces of a chained 2 mm round where a plane meets a 45 degree cone about the
/// world z axis (tangency-labs/solidcraft#17004), their nets as the body's record holds them
/// (`data/short_swept_revolution_nets.jsonl`): surfaces of revolution swept 0.4466 degrees at a
/// radius of about 15.6 mm, mirror images of each other in y = 0, sweeping along v. The
/// recogniser's axis is a circle's through three evaluated points of so short an arc; its
/// centre lay 4e-11 mm off sideways, which turned the second net's swept points past their
/// rounding, so the body could not be read back. Each net, and the same net with u and v
/// swapped, derives about the z axis through the origin, over its own sweep, and evaluates as
/// its form within 64 ulps of the form's size between the control points too.
#[test]
fn short_swept_revolution_nets_derive_about_their_own_axis() {
    let forms: Vec<Form> = include_str!("data/short_swept_revolution_nets.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(forms.len(), 2);
    for form in &forms {
        // the sweep about the z axis from the net's first to its last control point, which
        // are on the surface
        let angle = |n: usize| {
            let [x, y, _] = form.points[n];
            y.atan2(x)
        };
        let sweep = (angle(form.v_count - 1) - angle(0)).abs().to_degrees();
        assert!((sweep - 0.4466).abs() < 1e-4, "{sweep}");
        derives_about_the_z_axis(form, sweep);
    }
}

/// The 2 mm round's piece of the same body (tangency-labs/solidcraft#17004) on the cone's
/// lower rim, its net as the body's record holds it (`data/long_swept_revolution_net.jsonl`):
/// a surface of revolution swept 286.26 degrees, from 36.87 degrees through 180 to 323.13,
/// in four Bezier spans along u. Its generator's two ends, the control rows of radius 14.17
/// and 15.59 mm, start 5e-14 radians apart; a frame whose x axis pointed to the outer row's
/// start turned every converted angle by that against the polar converter's circle, the
/// generator's start, which put the swept points 7.5e-13 mm off. The net, and the same net
/// with u and v swapped, derives about the z axis over its own sweep and evaluates as its
/// form within 64 ulps of the form's size between the control points too.
#[test]
fn a_revolution_net_swept_past_half_a_turn_derives_about_its_own_axis() {
    let form: Form =
        serde_json::from_str(include_str!("data/long_swept_revolution_net.jsonl").trim()).unwrap();
    // the sweep about the z axis from the net's first to its last control point, which are
    // on the surface, the long way round
    let angle = |n: usize| {
        let [x, y, _] = form.points[n];
        y.atan2(x)
    };
    let last = (form.u_count - 1) * form.v_count;
    let sweep = 360.0 - (angle(0) - angle(last)).to_degrees();
    assert!((sweep - 286.2602).abs() < 1e-4, "{sweep}");
    derives_about_the_z_axis(&form, sweep);
}

/// `form`, and `form` with u and v swapped, derives as a surface of revolution about the
/// world z axis over `sweep` degrees and evaluates as `form` within 64 ulps of its size at
/// points between the control points.
#[allow(clippy::panic)] // a test helper
fn derives_about_the_z_axis(form: &Form, sweep: f64) {
    use solidcore::surface::surface::SurfaceKind;
    let k = Kernel::default();
    assert_eq!(form.kind, "Revolution");
    let size = form
        .points
        .iter()
        .zip(form.weights.as_ref().unwrap())
        .map(|(&[x, y, z], &w)| Vec3::new(x, y, z).length() / w)
        .fold(0.0, f64::max);
    for surface in [form.surface(), form.transposed()] {
        let derived = AnalyticSupport::derive_revolution(&k, &surface).unwrap();
        let SurfaceKind::SurfOfRevolution(rev) = derived.kind() else {
            panic!("not a surface of revolution: {:?}", derived.kind());
        };
        let position = rev.position();
        let axis = position.x_axis().cross(position.y_axis());
        assert!(axis.x.hypot(axis.y) < 1e-9, "axis {axis:?}");
        let origin = position.origin();
        assert!(origin.x.hypot(origin.y) < 1e-9, "origin {origin:?}");
        let domain = rev.step_uv_domain();
        let derived_sweep = domain.max().x - domain.min().x;
        assert!(
            (derived_sweep - sweep).abs() < 1e-9,
            "{derived_sweep} for {sweep}"
        );
        let d = surface.natural_uv_domain();
        for (a, b) in [(0.1, 0.2), (0.5, 0.5), (0.9, 0.8), (0.3, 0.7)] {
            let uv = Point2::new(
                d.min().x + (d.max().x - d.min().x) * a,
                d.min().y + (d.max().y - d.min().y) * b,
            );
            let step = derived.convert_uv_from_nurbs_to_step(&k, uv).unwrap();
            let off = (derived.evaluate_step_point(&k, step).unwrap()
                - surface.evaluate_point(uv).unwrap())
            .length();
            assert!(off <= 64.0 * f64::EPSILON * size, "{off:e} off at {uv:?}");
        }
    }
}

/// One rational span of a right circular cone about +X: radius `r0` at `x0`, radius `r1`
/// at `x1`, turned by `sweep_deg` and centred on the +Y meridian. The middle control is
/// the tangents' intersection, weight `cos` of half the sweep (The NURBS Book, §7.5).
fn cone_span(
    x0: f64,
    r0: f64,
    x1: f64,
    r1: f64,
    sweep_deg: f64,
    weight_sign: f64,
) -> BSplineSurface {
    let half = sweep_deg.to_radians() * 0.5;
    let (c, s, w) = (half.cos(), half.sin(), half.cos());
    let row = |x: f64, r: f64| {
        let pts = [
            (x, r * c, -r * s, weight_sign),
            (x, r / c, 0.0, w * weight_sign),
            (x, r * c, r * s, weight_sign),
        ];
        pts.iter()
            .map(|&(x, y, z, wt)| CPoint::new(x * wt, y * wt, z * wt, wt))
            .collect()
    };
    let nurb = solidcore::nurbs::srf_basic::Surface::new(
        vec![row(x0, r0), row(x1, r1)],
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
    .unwrap();
    BSplineSurface::from_nurb(nurb, true)
}

/// The rib cone a 2 mm fillet wrote as a support when the 15° edge was rounded together
/// with the rib's end edges: one 12° rational span (weight `cos 6°`) between the root
/// and the outer generator. Deriving it must build on this form. Dropping the cone label
/// leaves the round a freeform support, and the round then does not certify.
fn rib_support_form() -> BSplineSurface {
    // (x, y, z, weight), u the 12° arc, v the generator.
    let raw = [
        [
            [18.0, 28.3438740179958, -2.9790612031281256, 1.0],
            [
                26.74312899999874,
                33.17091333596729,
                -3.486403479240729,
                1.0,
            ],
        ],
        [
            [
                17.901394116628865,
                28.50000000000001,
                1.2145552174048781e-13,
                0.994521895368273,
            ],
            [
                26.596627341156914,
                33.35362799999897,
                2.2590727043730735e-13,
                0.994521895368273,
            ],
        ],
        [
            [
                17.99999999999991,
                28.343874017995784,
                2.9790612031283272,
                1.0,
            ],
            [
                26.743128999998635,
                33.17091333596727,
                3.4864034792412224,
                1.0,
            ],
        ],
    ];
    // The record stores homogeneous coordinates (`wx, wy, wz`) beside the weight.
    // Multiplying by the weight again pulls the tangent point off the circle.
    let net = raw
        .iter()
        .map(|row| {
            row.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
                .collect()
        })
        .collect();
    let nurb = solidcore::nurbs::srf_basic::Surface::new(
        net,
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 8.743129, 8.743129],
    )
    .unwrap();
    BSplineSurface::from_nurb(nurb, true)
}

/// A 12° rib on the frustum of #19724 is a right circular cone. The centre built from the
/// tangent leg lies 6.8×10⁻¹³ mm off the axis, 1.06 times the rounding `build` allows, once
/// the control points carry the fillet's conversions. The support was stored without its
/// cone label and the round would not certify. The chord and the middle weight put the
/// centre 2.7×10⁻¹⁴ mm off, and the form proves. A span past 180°, and a row whose
/// weights are all negated, are the same circle and prove too. A one-degree span's recovered
/// centre lies about `R / θ²` off the axis, about 8×10⁻¹² mm here, past the evaluation bound,
/// so `build` returns `InvalidInput`.
#[test]
fn a_twelve_degree_rib_cone_derives_from_the_fillet_support() {
    let k = Kernel::default();
    let mut forms = vec![("recorded fillet support".to_string(), rib_support_form())];
    for sweep in [1.0_f64, 3.0, 6.0, 12.0, 90.0, 179.0, 270.0] {
        forms.push((
            format!("constructed {sweep} degree cone"),
            cone_span(18.0, 28.5, 26.743_129, 33.353_628, sweep, 1.0),
        ));
    }
    // Negating every weight leaves the circle. The ends are negative together.
    forms.push((
        "constructed 12 degree cone, weights negated".to_string(),
        cone_span(18.0, 28.5, 26.743_129, 33.353_628, 12.0, -1.0),
    ));
    for (name, form) in forms {
        let derived = AnalyticSupport::derive(&k, SupportKind::Cone, &form)
            .unwrap_or_else(|e| panic!("{name} does not derive: {e}"));
        let short = name.contains("1 degree");
        if !short {
            let margin = derived.parametrisation_margin(&k, &form).unwrap();
            assert!(
                margin.is_some_and(|m| m <= 1.0),
                "{name} parametrisation margin {margin:?}"
            );
        }
        let built = derived.build(&k, form);
        if short {
            let err = built.expect_err("{name} published a centre the arc does not prove");
            assert!(
                matches!(err, solidcore::error::SolidError::InvalidInput { .. }),
                "{name}: {err}"
            );
            continue;
        }
        built.unwrap_or_else(|e| panic!("{name} does not build: {e}"));
    }
}
