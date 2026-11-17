//! Pass ordered Match samples and their control responses to the kernel.
use solidcore::eval::plan::SurfaceJet;
use solidcore::geom::form_match as kernel;
use solidcore_convert::{record_list, vec3};
use solidcore_record::Vec3;

pub use kernel::{MirrorColumns, Station};

/// Evaluated source or support derivatives in portable coordinates.
#[derive(Clone, Copy)]
pub struct Jet {
    /// Surface position.
    pub point: Vec3,
    /// First partial derivative along U.
    pub du: Vec3,
    /// First partial derivative along V.
    pub dv: Vec3,
    /// Second partial derivative along U.
    pub duu: Vec3,
    /// Mixed second partial derivative.
    pub duv: Vec3,
    /// Second partial derivative along V.
    pub dvv: Vec3,
}

impl Jet {
    fn kernel(self) -> SurfaceJet {
        SurfaceJet {
            point: vec3(self.point),
            du: vec3(self.du),
            dv: vec3(self.dv),
            duu: vec3(self.duu),
            duv: vec3(self.duv),
            dvv: vec3(self.dvv),
        }
    }
}

/// Exact coordinates and the support differential at a target station.
#[derive(Clone, Copy)]
pub struct Target {
    /// Genuine corner without a unique tangent or normal.
    pub corner: bool,
    /// Boundary position.
    pub point: Vec3,
    /// Boundary tangent before orientation.
    pub tangent: Vec3,
    /// Oriented support normal.
    pub normal: Vec3,
    /// Analytic support differential.
    pub support: Jet,
}

impl Target {
    fn kernel(self) -> kernel::Target {
        kernel::Target {
            corner: self.corner,
            point: vec3(self.point),
            tangent: vec3(self.tangent),
            normal: vec3(self.normal),
            support: self.support.kernel(),
        }
    }
}

/// Continue a quadratic control row.
pub fn quadratic_extension(point: Vec3, inward: Vec3, second: Vec3) -> solidcore::Result<Vec3> {
    kernel::quadratic_extension(vec3(point), vec3(inward), vec3(second))
        .map(solidcore_convert::record_vec3)
}

/// Whether the exact original target station is already within its retention band.
pub fn retain_station(point: Vec3, target: Vec3, position_tolerance: f64) -> bool {
    kernel::retain_station(vec3(point), vec3(target), position_tolerance)
}

/// Search one bounded periodic interval, preserving the application's callback errors.
pub fn closest<T, E>(
    point: Vec3,
    interval: [f64; 2],
    at: impl Fn(f64) -> Result<T, E>,
    position: impl Fn(&T) -> Vec3,
) -> Result<T, E> {
    kernel::closest(vec3(point), interval, at, |target| vec3(position(target)))
}

/// Measure position, tangent angle, and curvature residuals in the kernel.
pub fn residual(
    jet: Jet,
    target: Target,
    order: usize,
    diagonal: f64,
) -> solidcore::Result<[f64; 3]> {
    kernel::residual(jet.kernel(), target.kernel(), order, diagonal)
}

/// Solve one damped adjustment of the nearby control coordinates.
pub fn step(
    samples: impl ExactSizeIterator<Item = (Station, Target, Jet, Jet)>,
    response: &[Vec<[f64; 6]>],
    mirror: Option<&MirrorColumns>,
    order: usize,
    curvature_tolerance: f64,
    diagonal: f64,
) -> solidcore::Result<Vec<Vec3>> {
    let samples = samples.map(|(station, target, original, current)| {
        let target = kernel::Target {
            corner: target.corner,
            point: vec3(target.point),
            tangent: vec3(target.tangent),
            normal: vec3(target.normal),
            support: target.support.kernel(),
        };
        (station, target, original.kernel(), current.kernel())
    });
    kernel::step(
        samples,
        response,
        mirror,
        order,
        curvature_tolerance,
        diagonal,
    )
    .map(|delta| record_list(&delta))
}
