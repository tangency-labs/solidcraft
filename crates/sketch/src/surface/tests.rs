//! Tests of the surface solver against analytic spheres, cylinders and a plane, so no body or
//! kernel record is needed.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use solidcore_record::Vec2;

use crate::surface::solver::{first_refusal, surface_solve};
use crate::surface::{SurfaceChart, SurfaceGeodesic, SurfaceGeodesicError, SurfaceRegion};
use crate::{Constraint, ConstraintKind, Sketch};

/// The 3D point of a sphere of radius r, in (longitude, latitude).
fn sphere_point(r: f64, u: f64, v: f64) -> [f64; 3] {
    let (sv, cv) = v.sin_cos();
    let (su, cu) = u.sin_cos();
    [r * cu * cv, r * su * cv, r * sv]
}

/// The metric-unit tangent, in (u, v), at a sphere point toward another.
fn sphere_tangent(r: f64, at: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    let pa = sphere_point(r, at[0], at[1]);
    let pb = sphere_point(r, to[0], to[1]);
    let r2 = r * r;
    let dot = pa[0] * pb[0] + pa[1] * pb[1] + pa[2] * pb[2];
    let mut t = [pb[0] - (dot / r2) * pa[0], pb[1] - (dot / r2) * pa[1], pb[2] - (dot / r2) * pa[2]];
    let n = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
    if n <= 1e-15 {
        return [0.0, 0.0];
    }
    t = [t[0] / n, t[1] / n, t[2] / n];
    let (u, v) = (at[0], at[1]);
    let (sv, cv) = v.sin_cos();
    let (su, cu) = u.sin_cos();
    let su_v = [r * cv * (-su), r * cv * cu, 0.0];
    let sv_v = [r * (-cu * sv), r * (-su * sv), r * cv];
    let e = r * r * cv * cv;
    if e.abs() < 1e-15 {
        return [0.0, 0.0];
    }
    let a = (t[0] * su_v[0] + t[1] * su_v[1] + t[2] * su_v[2]) / e;
    let b = (t[0] * sv_v[0] + t[1] * sv_v[1] + t[2] * sv_v[2]) / (r * r);
    [a, b]
}

/// An analytic sphere, identity chart: x = longitude, y = latitude. Metric (R^2 cos^2 y, 0, R^2).
struct Sphere {
    radius: f64,
}

impl SurfaceChart for Sphere {
    fn metric(&self, _x: f64, y: f64) -> Option<[f64; 3]> {
        let (_, cy) = y.sin_cos();
        let e = self.radius * self.radius * cy * cy;
        let g = self.radius * self.radius;
        Some([e, 0.0, g])
    }

    fn geodesic(&self, a: [f64; 2], b: [f64; 2]) -> Result<SurfaceGeodesic, SurfaceGeodesicError> {
        let pa = sphere_point(self.radius, a[0], a[1]);
        let pb = sphere_point(self.radius, b[0], b[1]);
        let r2 = self.radius * self.radius;
        let dot = pa[0] * pb[0] + pa[1] * pb[1] + pa[2] * pb[2];
        let length = self.radius * (dot / r2).clamp(-1.0, 1.0).acos();
        Ok(SurfaceGeodesic {
            samples: vec![a, b],
            length,
            tan0: sphere_tangent(self.radius, a, b),
            tan1: sphere_tangent(self.radius, b, a),
            windings: [0, 0],
        })
    }

    fn distance(&self, a: [f64; 2], b: [f64; 2]) -> Result<f64, SurfaceGeodesicError> {
        self.geodesic(a, b).map(|g| g.length)
    }

    fn point(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        Some(sphere_point(self.radius, x, y))
    }

    fn in_domain(&self, x: f64, y: f64) -> bool {
        x.is_finite() && y.is_finite() && y.abs() <= FRAC_PI_2 + 1e-9
    }

    fn region(&self, x: f64, y: f64) -> SurfaceRegion {
        if self.in_domain(x, y) { SurfaceRegion::OnFace } else { SurfaceRegion::OffSurface }
    }
}

/// An analytic cylinder, identity chart: x = angle, y = height. Metric (R^2, 0, 1).
struct Cylinder {
    radius: f64,
}

fn wrap_pi(x: f64) -> f64 {
    let mut x = x.rem_euclid(TAU);
    if x > PI {
        x -= TAU;
    }
    x
}

impl SurfaceChart for Cylinder {
    fn metric(&self, _x: f64, _y: f64) -> Option<[f64; 3]> {
        Some([self.radius * self.radius, 0.0, 1.0])
    }

    fn geodesic(&self, a: [f64; 2], b: [f64; 2]) -> Result<SurfaceGeodesic, SurfaceGeodesicError> {
        let du = wrap_pi(a[0] - b[0]);
        let dv = a[1] - b[1];
        let length = ((self.radius * du).powi(2) + dv * dv).sqrt();
        Ok(SurfaceGeodesic { samples: vec![a, b], length, tan0: [-du, -dv], tan1: [du, dv], windings: [0, 0] })
    }

    fn distance(&self, a: [f64; 2], b: [f64; 2]) -> Result<f64, SurfaceGeodesicError> {
        let du = wrap_pi(a[0] - b[0]);
        let dv = a[1] - b[1];
        Ok(((self.radius * du).powi(2) + dv * dv).sqrt())
    }

    fn point(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        Some([self.radius * x.cos(), self.radius * x.sin(), y])
    }

    fn in_domain(&self, x: f64, y: f64) -> bool {
        x.is_finite() && y.is_finite()
    }

    fn region(&self, x: f64, y: f64) -> SurfaceRegion {
        if self.in_domain(x, y) { SurfaceRegion::OnFace } else { SurfaceRegion::OffSurface }
    }
}

/// An identity-metric plane, for the metric-free constraints (coincidence and fix).
struct Plane;

impl SurfaceChart for Plane {
    fn metric(&self, _x: f64, _y: f64) -> Option<[f64; 3]> {
        Some([1.0, 0.0, 1.0])
    }

    fn geodesic(&self, a: [f64; 2], b: [f64; 2]) -> Result<SurfaceGeodesic, SurfaceGeodesicError> {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = (dx * dx + dy * dy).sqrt();
        Ok(SurfaceGeodesic { samples: vec![a, b], length, tan0: [dx, dy], tan1: [-dx, -dy], windings: [0, 0] })
    }

    fn point(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        Some([x, y, 0.0])
    }

    fn in_domain(&self, x: f64, y: f64) -> bool {
        x.is_finite() && y.is_finite()
    }

    fn region(&self, x: f64, y: f64) -> SurfaceRegion {
        if self.in_domain(x, y) { SurfaceRegion::OnFace } else { SurfaceRegion::OffSurface }
    }
}

/// A spherical triangle with three 90-degree angles: the octant, whose corners are three
/// mutually orthogonal unit vectors (sides pi/2 * R, angle sum 3 pi / 2). On a plane the same
/// three right angles are impossible, so solving proves the metric is used, not a flattened chart.
#[test]
fn a_spherical_triangle_with_three_right_angles_solves() {
    let chart = Sphere { radius: 1.0 };
    let mut sk = Sketch::new();
    // Away from the octant: the solve has to travel to it, so a no-op solver fails this.
    let a = sk.add_point(Vec2::new(0.25, -0.15), Some("a")).unwrap();
    let b = sk.add_point(Vec2::new(1.1, 0.55), Some("b")).unwrap();
    let c = sk.add_point(Vec2::new(-1.25, 0.85), Some("c")).unwrap();
    let start = [sk.points[a].pos, sk.points[b].pos, sk.points[c].pos];
    let ab = sk.add_line_pts(a, b, Some("ab")).unwrap();
    let bc = sk.add_line_pts(b, c, Some("bc")).unwrap();
    let ca = sk.add_line_pts(c, a, Some("ca")).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value: FRAC_PI_2 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: FRAC_PI_2 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: c, q: a, value: FRAC_PI_2 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Angle { a: ab, b: ca, value: FRAC_PI_2, flip: false }, None).unwrap();
    sk.add_constraint(ConstraintKind::Angle { a: bc, b: ab, value: FRAC_PI_2, flip: false }, None).unwrap();
    sk.add_constraint(ConstraintKind::Angle { a: ca, b: bc, value: FRAC_PI_2, flip: false }, None).unwrap();

    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved(), "max_residual {}", report.max_residual);
    assert!(report.max_residual < 1e-6);
    let moved = [sk.points[a].pos, sk.points[b].pos, sk.points[c].pos]
        .iter()
        .zip(start.iter())
        .any(|(now, then)| (now.x - then.x).abs() > 1e-3 || (now.y - then.y).abs() > 1e-3);
    assert!(moved, "a solve that never moved is not a solve");

    // The lifted corners are mutually orthogonal unit vectors.
    let pa = chart.point(sk.points[a].pos.x, sk.points[a].pos.y).unwrap();
    let pb = chart.point(sk.points[b].pos.x, sk.points[b].pos.y).unwrap();
    let pc = chart.point(sk.points[c].pos.x, sk.points[c].pos.y).unwrap();
    for (p, q) in [(&pa, &pb), (&pb, &pc), (&pc, &pa)] {
        let dot = p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
        assert!(dot.abs() < 1e-6, "corner dot product {dot}");
    }
}

/// A distance on a cylinder is the unwrapped straight line, not the chart step: with radius 2 the
/// geodesic from (0, 0) to (pi/2, 0) is R * (pi/2) = pi, not pi/2.
#[test]
fn a_cylinder_distance_solves() {
    let chart = Cylinder { radius: 2.0 };
    let d = chart.distance([0.0, 0.0], [FRAC_PI_2, 0.0]).unwrap();
    assert!((d - PI).abs() < 1e-12, "cylinder distance {d}");

    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), Some("a")).unwrap();
    // Too short to start with (2.2 mm); the solve has to lengthen it to pi.
    let b = sk.add_point(Vec2::new(1.1, 0.3), Some("b")).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value: PI }, None).unwrap();
    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved(), "max_residual {}", report.max_residual);
    let reached = chart.distance([sk.points[a].pos.x, sk.points[a].pos.y], [sk.points[b].pos.x, sk.points[b].pos.y]).unwrap();
    assert!((reached - PI).abs() < 1e-6, "cylinder distance {reached}");
}

/// A fixed point and a coincident point meet exactly.
#[test]
fn a_coincident_and_fix_solve() {
    let chart = Plane;
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), Some("a")).unwrap();
    let b = sk.add_point(Vec2::new(1.0, 2.0), Some("b")).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: a, q: b }, None).unwrap();
    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved());
    let pa = sk.points[a].pos;
    let pb = sk.points[b].pos;
    assert!((pa.x - pb.x).abs() < 1e-9 && (pa.y - pb.y).abs() < 1e-9);
}

/// The intrinsic length of a line on a sphere is the drawn chart path measured with the metric: a
/// line along latitude pi/4 from longitude 0 to pi/2 has length R cos(pi/4) * (pi/2), not the
/// chart step pi/2.
#[test]
fn an_intrinsic_length_on_a_sphere_solves() {
    let chart = Sphere { radius: 1.0 };
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, FRAC_PI_4), Some("a")).unwrap();
    // Off the wanted length to start with, so the solve has to change the drawn path.
    let b = sk.add_point(Vec2::new(FRAC_PI_2, 0.55), Some("b")).unwrap();
    let l = sk.add_line_pts(a, b, Some("l")).unwrap();
    let value = FRAC_PI_2 * FRAC_PI_4.cos();
    sk.add_constraint(ConstraintKind::Length { l, value }, None).unwrap();
    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved(), "max_residual {}", report.max_residual);
    assert!(report.max_residual < 1e-6);
    let reached = intrinsic_length(&chart, sk.points[a].pos, sk.points[b].pos);
    assert!((reached - value).abs() < 1e-6, "length {reached} vs {value}");
}

/// The drawn chart segment measured with the metric, by Simpson's rule (the solver's own rule).
fn intrinsic_length(chart: &Sphere, a: Vec2, b: Vec2) -> f64 {
    let steps: usize = 64;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let mut sum = 0.0;
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let e = chart.metric(a.x + dx * t, a.y + dy * t).unwrap_or([0.0, 0.0, 0.0]);
        let speed = (e[0] * dx * dx + 2.0 * e[1] * dx * dy + e[2] * dy * dy).max(0.0).sqrt();
        let weight = if k == 0 || k == steps {
            1.0
        } else if k % 2 == 0 {
            2.0
        } else {
            4.0
        };
        sum += weight * speed;
    }
    sum / (3.0 * steps as f64)
}

/// A 40-degree corner on a plane, made a right angle: the residual is the metric angle at the
/// shared point, not a chart slope.
#[test]
fn a_perpendicular_corner_solves() {
    let chart = Plane;
    let mut sk = Sketch::new();
    let o = sk.add_point(Vec2::new(0.0, 0.0), Some("o")).unwrap();
    let a = sk.add_point(Vec2::new(2.0, 0.0), Some("a")).unwrap();
    let b = sk.add_point(Vec2::new(1.4, 0.9), Some("b")).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: o }, None).unwrap();
    let l1 = sk.add_line_pts(o, a, Some("l1")).unwrap();
    let l2 = sk.add_line_pts(o, b, Some("l2")).unwrap();
    sk.add_constraint(ConstraintKind::Perpendicular { a: l1, b: l2 }, None).unwrap();
    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved(), "max_residual {}", report.max_residual);
    let (po, pa, pb) = (sk.points[o].pos, sk.points[a].pos, sk.points[b].pos);
    let d1 = [pa.x - po.x, pa.y - po.y];
    let d2 = [pb.x - po.x, pb.y - po.y];
    let cosine = (d1[0] * d2[0] + d1[1] * d2[1]) / (d1[0].hypot(d1[1]) * d2[0].hypot(d2[1]));
    assert!(cosine.abs() < 1e-6, "corner cosine {cosine}");
}

/// A midpoint on a plane: the point has to travel to the middle of the line.
#[test]
fn a_midpoint_solves() {
    let chart = Plane;
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), Some("a")).unwrap();
    let b = sk.add_point(Vec2::new(2.0, 0.0), Some("b")).unwrap();
    let p = sk.add_point(Vec2::new(0.4, 0.3), Some("p")).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: b }, None).unwrap();
    let l = sk.add_line_pts(a, b, Some("l")).unwrap();
    sk.add_constraint(ConstraintKind::Midpoint { p, l }, None).unwrap();
    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved(), "max_residual {}", report.max_residual);
    let mid = sk.points[p].pos;
    assert!((mid.x - 1.0).abs() < 1e-6 && mid.y.abs() < 1e-6, "midpoint {mid:?}");
}

/// Two lines on a sphere made equal: the intrinsic lengths have to agree, not the chart steps.
#[test]
fn equal_lengths_on_a_sphere_solve() {
    let chart = Sphere { radius: 1.0 };
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), Some("a")).unwrap();
    let b = sk.add_point(Vec2::new(0.2, 0.0), Some("b")).unwrap();
    let c = sk.add_point(Vec2::new(0.5, 0.0), Some("c")).unwrap();
    let d = sk.add_point(Vec2::new(0.9, 0.0), Some("d")).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: c }, None).unwrap();
    let l1 = sk.add_line_pts(a, b, Some("l1")).unwrap();
    let l2 = sk.add_line_pts(c, d, Some("l2")).unwrap();
    sk.add_constraint(ConstraintKind::Equal { a: l1, b: l2 }, None).unwrap();
    let report = surface_solve(&mut sk, &chart).unwrap();
    assert!(report.solved(), "max_residual {}", report.max_residual);
    let one = intrinsic_length(&chart, sk.points[a].pos, sk.points[b].pos);
    let two = intrinsic_length(&chart, sk.points[c].pos, sk.points[d].pos);
    assert!((one - two).abs() < 1e-6, "lengths {one} and {two}");
}

/// Every deferred constraint kind is refused with the "not supported on a curved face" words.
#[test]
fn every_unsupported_kind_is_refused() {
    let mut base = Sketch::new();
    let p = base.add_point(Vec2::new(0.0, 0.0), Some("p")).unwrap();
    let q = base.add_point(Vec2::new(1.0, 0.0), Some("q")).unwrap();
    let r = base.add_point(Vec2::new(0.0, 1.0), Some("r")).unwrap();
    let l1 = base.add_line_pts(p, q, Some("l1")).unwrap();
    let l2 = base.add_line_pts(p, r, Some("l2")).unwrap();
    let c1 = base.add_circle(Vec2::new(3.0, 0.0), 1.0, None, Some("c1")).unwrap();
    let c2 = base.add_circle(Vec2::new(5.0, 0.0), 1.0, None, Some("c2")).unwrap();

    let kinds: Vec<ConstraintKind> = vec![
        ConstraintKind::Horizontal { l: l1 },
        ConstraintKind::Vertical { l: l1 },
        ConstraintKind::DistanceX { p, q, value: 1.0 },
        ConstraintKind::DistanceY { p, q, value: 1.0 },
        ConstraintKind::Parallel { a: l1, b: l2 },
        ConstraintKind::Tangent { a: l1, b: l2 },
        ConstraintKind::Symmetric { p, q, l: l1 },
        ConstraintKind::Radius { c: c1, value: 1.0 },
        ConstraintKind::Diameter { c: c1, value: 2.0 },
        ConstraintKind::Concentric { a: c1, b: c2 },
    ];

    for kind in kinds {
        let mut sk = base.clone();
        sk.constraints.push(Constraint { id: "k".to_string(), kind, param: None, driven: false, text: None });
        let refusal = first_refusal(&sk).expect("a refusal");
        assert!(refusal.message().contains("not supported on a curved face"), "{refusal:?}");
    }
}
