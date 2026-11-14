//! The kernel's evaluation and exact NURBS forms (`solidcore::eval`) over the record's curves and
//! surfaces: each call converts its records through `solidcore-convert`, calls the kernel, and
//! converts what it returns back. The reader holds records; the kernel computes.

use solidcore::eval as k;
use solidcore_convert as c;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};

pub(crate) use solidcore::eval::uv::Starts;

/// Why a record cannot be evaluated: it does not convert (too deep), or the kernel refuses it.
pub(crate) type Error = String;

/// A curve record ready to evaluate.
pub(crate) struct CurveEval(k::CurveEval);

impl CurveEval {
    pub(crate) fn new(curve: &Curve) -> Result<Self, Error> {
        let curve = c::curve(curve).map_err(|e| e.to_string())?;
        k::CurveEval::new(&curve).map(CurveEval).map_err(|e| e.to_string())
    }
    pub(crate) fn point(&self, t: f64) -> Option<Vec3> {
        self.0.point(t).map(c::record_vec3)
    }
}

/// A closed spline or polyline with its seam moved to `start`, exactly.
pub(crate) fn restart_curve(curve: &Curve, start: f64) -> Result<Curve, Error> {
    let curve = c::curve(curve).map_err(|e| e.to_string())?;
    let restarted = k::pcurve::restart_curve(&curve, start).map_err(|e| match e {
        solidcore::SolidError::InvalidInput { reason, .. }
        | solidcore::SolidError::Unsupported { reason, .. }
        | solidcore::SolidError::PortInvariant { reason, .. } => reason.to_string(),
        other => other.to_string(),
    })?;
    c::record_curve(restarted).map_err(|e| e.to_string())
}

/// A surface record ready to evaluate.
pub(crate) struct SurfaceEval(k::SurfaceEval);

impl SurfaceEval {
    pub(crate) fn new(surface: &Surface) -> Result<Self, Error> {
        let surface = c::surface(surface).map_err(|e| e.to_string())?;
        k::SurfaceEval::new(&surface).map(SurfaceEval).map_err(|e| e.to_string())
    }
    pub(crate) fn point(&self, uv: [f64; 2]) -> Option<Vec3> {
        self.0.point(uv).map(c::record_vec3)
    }
    pub(crate) fn derivs(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        self.0.derivs(u, v).map(|(p, su, sv)| (c::record_vec3(p), c::record_vec3(su), c::record_vec3(sv)))
    }
    pub(crate) fn domain(&self) -> k::Domain {
        self.0.domain()
    }
    pub(crate) fn project(&self, p: Vec3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        self.0.project(c::vec3(p), near, accept)
    }
    pub(crate) fn project_with(&self, starts: &Starts, p: Vec3, near: Option<[f64; 2]>, accept: f64) -> Option<[f64; 2]> {
        self.0.project_with(starts, c::vec3(p), near, accept)
    }
    /// Whether `uv` is at a pole along each direction, or within `reach` mm of one
    /// ([`k::SurfaceEval::pole_within`]); refused where that cannot be decided.
    pub(crate) fn pole_within(&self, uv: [f64; 2], reach: f64) -> Result<[bool; 2], String> {
        self.0.pole_within(uv, reach).map_err(|e| e.to_string())
    }
}

/// The flux of `field` through a face of `s` bounded by `loops` ([`k::face_flux`]).
pub(crate) fn face_flux(s: &SurfaceEval, v0: f64, loops: &[Vec<(&Curve2, bool)>], field: &dyn Fn(Vec3) -> Vec3) -> Option<f64> {
    let converted: Vec<Vec<(k::Curve2, bool)>> = loops.iter().map(|l| l.iter().map(|(pc, fwd)| (c::curve2(pc), *fwd)).collect()).collect();
    let borrowed: Vec<Vec<(&k::Curve2, bool)>> = converted.iter().map(|l| l.iter().map(|(pc, fwd)| (pc, *fwd)).collect()).collect();
    k::face_flux(&s.0, v0, &borrowed, &|p| c::vec3(field(c::record_vec3(p))))
}

/// The exact NURBS forms of the analytic kinds ([`k::form`]), over records.
pub(crate) mod form {
    use super::{Curve, Frame, Surface, c, k};

    fn surface_in(s: &Surface) -> Option<k::Surface> {
        c::surface(s).ok()
    }
    fn curve_in(cv: &Curve) -> Option<k::Curve> {
        c::curve(cv).ok()
    }
    fn surface_out(s: k::Surface) -> Option<Surface> {
        c::record_surface(s).ok()
    }
    fn curve_out(cv: k::Curve) -> Option<Curve> {
        c::record_curve(cv).ok()
    }

    pub(crate) use solidcore::eval::form::param;

    pub(crate) fn frame_of(s: &Surface) -> Option<Frame> {
        k::form::frame_of(&surface_in(s)?).map(|f| c::record_frame(&f))
    }
    pub(crate) fn turned(s: &Surface, angle: f64) -> Option<Surface> {
        surface_out(k::form::turned(&surface_in(s)?, angle)?)
    }
    pub(crate) fn tube_turned(s: &Surface, angle: f64) -> Option<Surface> {
        surface_out(k::form::tube_turned(&surface_in(s)?, angle)?)
    }
    pub(crate) fn turn_quarters(s: &Surface, domain: [[f64; 2]; 2], tolerance: f64) -> Option<usize> {
        k::form::turn_quarters(&surface_in(s)?, domain, tolerance)
    }
    pub(crate) fn tube_quarters(s: &Surface, domain: [[f64; 2]; 2], tolerance: f64) -> Option<usize> {
        k::form::tube_quarters(&surface_in(s)?, domain, tolerance)
    }
    pub(crate) fn surface_within(s: &Surface, domain: [[f64; 2]; 2], tolerance: f64) -> Option<(Surface, [[f64; 2]; 2])> {
        let (form, d) = k::form::surface_within(&surface_in(s)?, domain, tolerance)?;
        Some((surface_out(form)?, d))
    }
    pub(crate) fn curve(cv: &Curve, interval: [f64; 2]) -> Option<(Curve, [f64; 2])> {
        let (form, i) = k::form::curve(&curve_in(cv)?, interval)?;
        Some((curve_out(form)?, i))
    }
    pub(crate) fn on_frame(cv: &Curve, interval: [f64; 2], frame: &Frame, tol: f64) -> Option<(Curve, [f64; 2])> {
        let (form, i) = k::form::on_frame(&curve_in(cv)?, interval, &c::frame(frame), tol)?;
        Some((curve_out(form)?, i))
    }
    pub(crate) fn tube_on_grid(cv: &Curve, interval: [f64; 2], frame: &Frame, torus: &Surface, tol: f64) -> Option<bool> {
        k::form::tube_on_grid(&curve_in(cv)?, interval, &c::frame(frame), &surface_in(torus)?, tol)
    }
    /// An edge's label, interval and NURBS form on the first of `frames` (or `tori`) it lies on
    /// ([`k::form::edge_on`]).
    pub(crate) fn edge_on<'a>(
        cv: &Curve,
        interval: [f64; 2],
        frames: impl IntoIterator<Item = &'a Frame>,
        tori: &[Surface],
        tol: f64,
    ) -> Option<(Curve, [f64; 2], Option<(Curve, [f64; 2])>)> {
        let frames: Vec<k::Frame> = frames.into_iter().map(c::frame).collect();
        let tori: Vec<k::Surface> = tori.iter().map(surface_in).collect::<Option<_>>()?;
        let (label, i, form) = k::form::edge_on(&curve_in(cv)?, interval, &frames, &tori, tol)?;
        let form = match form {
            Some((f, fi)) => Some((curve_out(f)?, fi)),
            None => None,
        };
        Some((curve_out(label)?, i, form))
    }
    /// The parameter box of a face on `label` over its boundary ([`k::form::extent`]), a boundary
    /// point on a pole where the label is collapsed there within `reach` mm (the face's
    /// tolerance).
    pub(crate) fn extent(label: &Surface, loops: &[Vec<(&Curve, [f64; 2], bool)>], reach: f64) -> Option<[[f64; 2]; 2]> {
        let converted: Vec<Vec<(k::Curve, [f64; 2], bool)>> =
            loops.iter().map(|l| l.iter().map(|(cv, i, f)| Some((curve_in(cv)?, *i, *f))).collect::<Option<_>>()).collect::<Option<_>>()?;
        let borrowed: Vec<Vec<(&k::Curve, [f64; 2], bool)>> = converted.iter().map(|l| l.iter().map(|(cv, i, f)| (cv, *i, *f)).collect()).collect();
        k::form::extent(&surface_in(label)?, &borrowed, reach).ok().flatten()
    }
}
