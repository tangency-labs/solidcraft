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
