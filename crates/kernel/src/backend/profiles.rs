//! solidcore's bodies from profiles ([`Profiles`]): solidcore-ops' sweeps of space curves
//! (`solidcore_ops::sweep`) and its sphere, each the call the weld bodies made before the
//! interface existed, the body it makes taken as its record.

use solidcore_ops::sweep::{self, Curve};
use solidcore_record::brep;
use solidcraft_kernel_api::affine::Matrix;
use solidcraft_kernel_api::profiles::{ProfileCurve, ProfileRefused, Profiles};

use super::Solidcore;
use crate::body::Body;

/// The kernel's error, in its words.
fn refused(e: solidcore_ops::Error) -> ProfileRefused {
    ProfileRefused(e.to_string())
}

/// solidcore-ops' curve of a profile curve, field for field.
fn ops_curve(c: &ProfileCurve) -> Curve {
    match c {
        ProfileCurve::Line { start, end } => Curve::Line { start: *start, end: *end },
        ProfileCurve::Arc { center, axis, reference, radius, start, end } => {
            Curve::Arc { center: *center, axis: *axis, reference: *reference, radius: *radius, start: *start, end: *end }
        }
        ProfileCurve::Nurbs { degree, points, knots } => Curve::Nurbs { degree: *degree, points: points.clone(), knots: knots.clone() },
    }
}

/// A profile curve of solidcore-ops' curve, field for field.
fn profile_curve(c: Curve) -> ProfileCurve {
    match c {
        Curve::Line { start, end } => ProfileCurve::Line { start, end },
        Curve::Arc { center, axis, reference, radius, start, end } => ProfileCurve::Arc { center, axis, reference, radius, start, end },
        Curve::Nurbs { degree, points, knots } => ProfileCurve::Nurbs { degree, points, knots },
    }
}

fn ops_curves(profile: &[ProfileCurve]) -> Vec<Curve> {
    profile.iter().map(ops_curve).collect()
}

/// solidcore-ops' body, moved by `place` when there is one, as the application's: its record.
fn body(made: solidcore_ops::Result<solidcore_ops::Body>, place: Option<&Matrix>) -> Result<Body, ProfileRefused> {
    let made = made.map_err(refused)?;
    if let Some(m) = place {
        made.transform(m).map_err(refused)?;
    }
    let (record, _) = made.to_record().map_err(refused)?;
    Body::from_record(record).map_err(|e| ProfileRefused(e.to_string()))
}

impl Profiles for Solidcore {
    const MAX_COORD: f64 = sweep::MAX_COORD;

    fn zone_tolerance(&self) -> Result<f64, ProfileRefused> {
        sweep::zone_tolerance().map_err(refused)
    }

    fn revolve_curves(
        &self,
        profile: &[ProfileCurve],
        origin: [f64; 3],
        axis: [f64; 3],
        angle: f64,
        cap: bool,
        place: Option<&Matrix>,
    ) -> Result<Body, ProfileRefused> {
        body(sweep::revolve(&ops_curves(profile), origin, axis, angle, cap), place)
    }

    fn extrude_curves(&self, profile: &[ProfileCurve], direction: [f64; 3], distance: f64, cap: bool) -> Result<Body, ProfileRefused> {
        body(sweep::extrude(&ops_curves(profile), direction, distance, 0.0, sweep::DraftCorner::default(), cap), None)
    }

    fn planar_faces(&self, loops: &[ProfileCurve]) -> Result<Body, ProfileRefused> {
        body(sweep::planar_faces(&ops_curves(loops)), None)
    }

    fn ball(&self, center: [f64; 3], radius: f64, place: Option<&Matrix>) -> Result<Body, ProfileRefused> {
        body(solidcore_ops::make_sphere(center, radius), place)
    }

    fn curve_pieces(&self, curve: &brep::Curve, interval: [f64; 2]) -> Result<Vec<ProfileCurve>, ProfileRefused> {
        Ok(Curve::from_record(curve, interval).map_err(refused)?.into_iter().map(profile_curve).collect())
    }
}
