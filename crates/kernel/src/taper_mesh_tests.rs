use super::*;
use mt::{ParameterDivision2D, ParametricSurface, ParametricSurface3D, SearchNearestParameter, SearchParameter};
use truck_meshalgo::prelude::*;

fn wedge() -> (Body, RadiusSpline) {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let outer = Loop2::polygon(&[Vec2::new(0., 0.), Vec2::new(40., 0.), Vec2::new(0., 20.)]);
    let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 15.).unwrap().pop().unwrap();
    let at = Vec3::new(20., 0., 15.);
    let (filleted, tapers) = fillet_tapered(&body, &[at], 16.).unwrap();
    let taper = tapers.into_iter().flatten().next().unwrap();
    (filleted, taper.spline)
}

fn expected(law: &RadiusSpline) -> f64 {
    let n = 20_000;
    let integral = (0..n).map(|i| law.eval((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64;
    6000. - (1. - std::f64::consts::FRAC_PI_4) * 40. * integral
}

fn mesh_volume(polygon: &PolygonMesh) -> f64 {
    let positions = polygon.positions();
    polygon
        .faces()
        .triangle_iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| positions[v.pos].to_vec());
            a.dot(b.cross(c)) / 6.
        })
        .sum()
}

#[test]
fn collapsed_taper_mesh_stays_accurate_as_tolerance_decreases() {
    let (body, law) = wedge();
    let wanted = expected(&law);
    for tol in [0.002, 0.001, 0.0005, 0.0002] {
        for refine in [false, true] {
            let mesh = body.mesh(&crate::MeshTolerance::chord(tol)).unwrap();
            assert!(mesh.positions.iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
            let got = mesh.measure().volume;
            assert!((got - wanted).abs() / wanted < 1e-4, "tol={tol}, refine={refine}: {got} vs {wanted}");
        }
    }
}

/// A valid exact parameter search can choose any parameter along a collapsed
/// edge, even far outside its nominal range. Force that answer on every platform
/// instead of relying on the Windows Newton solve that originally exposed #796.
#[derive(Clone)]
struct ArbitraryPole(mt::Surface);
impl ParametricSurface for ArbitraryPole {
    type Point = mt::Point3;
    type Vector = mt::Vector3;
    fn subs(&self, u: f64, v: f64) -> Self::Point {
        self.0.subs(u, v)
    }
    fn uder(&self, u: f64, v: f64) -> Self::Vector {
        self.0.uder(u, v)
    }
    fn vder(&self, u: f64, v: f64) -> Self::Vector {
        self.0.vder(u, v)
    }
    fn uuder(&self, u: f64, v: f64) -> Self::Vector {
        self.0.uuder(u, v)
    }
    fn uvder(&self, u: f64, v: f64) -> Self::Vector {
        self.0.uvder(u, v)
    }
    fn vvder(&self, u: f64, v: f64) -> Self::Vector {
        self.0.vvder(u, v)
    }
    fn parameter_range(&self) -> (mt::ParameterRange, mt::ParameterRange) {
        self.0.parameter_range()
    }
    fn u_period(&self) -> Option<f64> {
        self.0.u_period()
    }
    fn v_period(&self) -> Option<f64> {
        self.0.v_period()
    }
}
impl ParametricSurface3D for ArbitraryPole {
    fn normal(&self, u: f64, v: f64) -> mt::Vector3 {
        self.0.normal(u, v)
    }
}
impl ParameterDivision2D for ArbitraryPole {
    fn parameter_division(&self, range: ((f64, f64), (f64, f64)), tol: f64) -> (Vec<f64>, Vec<f64>) {
        self.0.parameter_division(range, tol)
    }
}
impl SearchParameter<mt::D2> for ArbitraryPole {
    type Point = mt::Point3;
    fn search_parameter<H: Into<mt::SPHint2D>>(&self, point: mt::Point3, hint: H, trials: usize) -> Option<(f64, f64)> {
        if matches!(self.0, mt::Surface::NurbsSurface(_)) && (point - mt::Point3::new(40., 0., 15.)).magnitude2() < 1e-20 {
            Some((1., -1e14))
        } else {
            self.0.search_parameter(point, hint, trials)
        }
    }
}
impl SearchNearestParameter<mt::D2> for ArbitraryPole {
    type Point = mt::Point3;
    fn search_nearest_parameter<H: Into<mt::SPHint2D>>(&self, point: mt::Point3, hint: H, trials: usize) -> Option<(f64, f64)> {
        self.0.search_nearest_parameter(point, hint, trials)
    }
}

#[test]
fn both_mesh_searches_keep_an_arbitrary_exact_pole_inside_the_domain() {
    let (body, law) = wedge();
    let shell = body.solid.boundaries()[0].mapped(Clone::clone, Clone::clone, |s| ArbitraryPole(s.clone()));
    for robust in [false, true] {
        let mesh = if robust { shell.robust_triangulation(0.0005) } else { shell.triangulation(0.0005) };
        for (source, face) in shell.face_iter().zip(mesh.face_iter()) {
            let polygon = face.surface().expect("valid pole must not discard its face");
            if matches!(source.surface().0, mt::Surface::NurbsSurface(_)) {
                assert!(polygon.uv_coords().iter().all(|p| (-1e-6..=1. + 1e-6).contains(&p.x) && (-1e-6..=1. + 1e-6).contains(&p.y)));
            }
        }
        let polygon = mesh.to_polygon();
        let volume = mesh_volume(&polygon);
        assert!((volume - expected(&law)).abs() / expected(&law) < 1e-4, "robust={robust}: {volume}");
    }
}

#[test]
fn bounded_projection_does_not_clip_a_planes_unbounded_support() {
    let (body, law) = wedge();
    let shell = body.solid.boundaries()[0].mapped(Clone::clone, Clone::clone, |surface| {
        if let mt::Surface::Plane(plane) = surface {
            let origin = plane.subs(0., 0.);
            mt::Surface::Plane(mt::Plane::new(origin, origin + plane.uder(0., 0.).normalize(), origin + plane.vder(0., 0.).normalize()))
        } else {
            surface.clone()
        }
    });
    for robust in [false, true] {
        let mesh = if robust { shell.robust_triangulation(0.0005) } else { shell.triangulation(0.0005) };
        let mut outside = false;
        for (source, face) in shell.face_iter().zip(mesh.face_iter()) {
            let polygon = face.surface().unwrap();
            if matches!(source.surface(), mt::Surface::Plane(_)) {
                outside |= polygon.uv_coords().iter().any(|p| p.x.abs() > 2. || p.y.abs() > 2.);
            }
        }
        assert!(outside, "fixture must extend beyond the plane's nominal unit box");
        let got = mesh_volume(&mesh.to_polygon());
        assert!((got - expected(&law)).abs() / expected(&law) < 1e-4, "robust={robust}: {got}");
    }
}

#[test]
fn provisional_preview_bounds_the_same_arbitrary_pole() {
    use mt::{BoundedCurve, ParameterDivision1D, ParametricCurve};
    let (body, _) = wedge();
    let face = body.solid.face_iter().find(|f| matches!(f.surface(), mt::Surface::NurbsSurface(_))).unwrap();
    let loops: Vec<Vec<mt::Point3>> = face
        .absolute_boundaries()
        .iter()
        .map(|wire| {
            wire.edge_iter()
                .flat_map(|edge| {
                    let curve = edge.oriented_curve();
                    let (mut parameters, _) = curve.parameter_division(curve.range_tuple(), 0.01);
                    parameters.pop();
                    parameters.into_iter().map(move |t| curve.subs(t))
                })
                .collect()
        })
        .collect();
    let mesh = truck_meshalgo::tessellation::preview_face(&ArbitraryPole(face.surface()), &loops, 8).unwrap();
    assert!(mesh.faces().triangle_iter().next().is_some());
    assert!(mesh.positions().iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
    assert!(mesh.uv_coords().iter().all(|p| (-1e-6..=1. + 1e-6).contains(&p.x) && (-1e-6..=1. + 1e-6).contains(&p.y)));
}
