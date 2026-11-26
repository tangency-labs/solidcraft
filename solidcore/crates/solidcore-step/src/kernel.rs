//! The kernel's evaluation and exact NURBS forms (`solidcore::eval`) over the record's curves and
//! surfaces: each call converts its records through `solidcore-convert`, calls the kernel, and
//! converts what it returns back. The reader holds records; the kernel computes.

use solidcore::eval as k;
use solidcore_convert as c;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};

pub(crate) use solidcore::eval::uv::{Starts, unwrap};

/// Why a record cannot be evaluated: it does not convert (too deep), or the kernel refuses it.
pub(crate) type Error = String;

/// The kernel's geometric radial order of the faces leaving an edge. Record uses
/// are returned as a permutation, with their side flags unchanged.
pub(crate) fn radial_order(curve: &Curve, interval: [f64; 2], faces: &[(&Surface, &Curve2, bool)]) -> Result<Vec<usize>, Error> {
    let curve = c::curve(curve).map_err(|e| e.to_string())?;
    let converted = faces
        .iter()
        .map(|&(surface, pcurve, forward)| Ok((c::surface(surface).map_err(|e| e.to_string())?, c::curve2(pcurve), forward)))
        .collect::<Result<Vec<_>, Error>>()?;
    let faces: Vec<_> = converted.iter().map(|(surface, pcurve, forward)| k::pcurve::RadialFace { surface, pcurve, forward: *forward }).collect();
    k::pcurve::radial_order(&curve, interval, &faces).map_err(|e| e.to_string())
}

/// Whether `curve`'s length over `interval`, its ends and knots as given, is proved at most
/// `limit` ([`solidcore::curve::curve::Curve::proved_length_at_most`], settled rather than
/// narrowed, within `most` pieces): `Some(true)` where it is, `Some(false)` where it is proved
/// more, `None` where the kernel does not enclose that curve's kind, cannot make the curve, or
/// settles neither within `most`.
pub(crate) fn curve_length_at_most(curve: &Curve, interval: [f64; 2], limit: f64, most: usize) -> Result<Option<bool>, Error> {
    // A line record with one point is that point: its length is 0, while the kernel's line
    // constructor takes only a direction, which this one has none of.
    if matches!(curve, Curve::Line { start, end } if start == end) {
        return Ok(Some(0.0 <= limit));
    }
    let kernel = solidcore::kernel::Kernel::default();
    let Ok(record) = c::curve(curve) else { return Ok(None) };
    let Ok(made) = solidcore::curve::from_parameters::from_parameters(&kernel, &record, interval) else { return Ok(None) };
    match made.curve.proved_length_at_most(&kernel, &made.interval, limit, most) {
        Ok(settled) => Ok(settled),
        Err(_) => Ok(None),
    }
}

/// Whether every point of `a` over `a_on` lies within `tol` of `b` over `b_on`, and every point
/// of `b` within `tol` of `a` ([`solidcore::curve::curve::Curve::proved_together`], a whole-span
/// enclosure, never samples): `None` where the kernel cannot prove it, or make a curve.
pub(crate) fn curves_within(a: &Curve, a_on: [f64; 2], b: &Curve, b_on: [f64; 2], tol: f64) -> Result<Option<bool>, Error> {
    let kernel = solidcore::kernel::Kernel::default();
    let (Ok(a), Ok(b)) = (c::curve(a), c::curve(b)) else { return Ok(None) };
    let (Ok(a), Ok(b)) = (
        solidcore::curve::from_parameters::from_parameters(&kernel, &a, a_on),
        solidcore::curve::from_parameters::from_parameters(&kernel, &b, b_on),
    ) else {
        return Ok(None);
    };
    a.curve.proved_together(&kernel, &a.interval, &b.curve, &b.interval, tol, &[]).map(Some).map_err(|e| e.to_string())
}

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
    /// The point and first derivative at `t`; `None` for a traced intersection.
    pub(crate) fn derivs(&self, t: f64) -> Option<(Vec3, Vec3)> {
        self.0.derivs(t).map(|(p, d)| (c::record_vec3(p), c::record_vec3(d)))
    }
}

/// Where a piece `on` of `curve`, its angle about the axis through `origin` (from unit `x`
/// towards unit `y`) monotone and under half a turn, reaches angle `theta`
/// ([`k::ruling::angle_crossing`]): `(true, point, parameter)` where it does, else `(false, the
/// end nearer in angle, its parameter)`. `None` where the curve can't be evaluated.
pub(crate) fn angle_crossing(curve: &CurveEval, origin: Vec3, x: Vec3, y: Vec3, on: [f64; 2], theta: f64) -> Option<(bool, Vec3, f64)> {
    use k::ruling::AngleCrossing as A;
    Some(match k::ruling::angle_crossing(&curve.0, c::vec3(origin), c::vec3(x), c::vec3(y), on, theta)? {
        A::At { point, t } => (true, c::record_vec3(point), t),
        A::Nearer { point, t } => (false, c::record_vec3(point), t),
    })
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
    /// Whether the piece of the iso-line along `d` through `uv` over the `d` parameters `range`
    /// lies within `reach` mm of `centre`, certified ([`k::SurfaceEval::iso_within`]); refused
    /// where that cannot be decided.
    pub(crate) fn iso_within(&self, uv: [f64; 2], d: usize, range: (f64, f64), centre: Vec3, reach: f64) -> Result<bool, String> {
        self.0.iso_within(uv, d, range, c::vec3(centre), reach).map_err(|e| e.to_string())
    }
    /// `pc` with its control points put on the knot range and a certified bound on how far that
    /// moves the surface under it; `None` when it is on the range, refused where the move is past
    /// `tolerance` or not bounded ([`k::SurfaceEval::pcurve_onto_knots`]).
    pub(crate) fn pcurve_onto_knots(&self, pc: &Curve2, tolerance: f64) -> Result<Option<(Curve2, f64)>, String> {
        let moved = self.0.pcurve_onto_knots(&c::curve2(pc), tolerance).map_err(|e| e.to_string())?;
        Ok(moved.map(|m| (c::record_curve2(m.pcurve), m.displacement)))
    }
}

/// The flux of `field` through a face of `s` bounded by `loops` ([`k::face_flux`]).
pub(crate) fn face_flux(s: &SurfaceEval, v0: f64, loops: &[Vec<(&Curve2, bool)>], field: &dyn Fn(Vec3) -> Vec3) -> Option<f64> {
    let converted: Vec<Vec<(k::Curve2, bool)>> = loops.iter().map(|l| l.iter().map(|(pc, fwd)| (c::curve2(pc), *fwd)).collect()).collect();
    let borrowed: Vec<Vec<(&k::Curve2, bool)>> = converted.iter().map(|l| l.iter().map(|(pc, fwd)| (pc, *fwd)).collect()).collect();
    k::face_flux(&s.0, v0, &borrowed, &|p| c::vec3(field(c::record_vec3(p))))
}

/// The sign of one closed shell, certified by the kernel's volume enclosure.
/// No numerical estimate decides a repaired shell's absolute direction.
pub(crate) fn shell_volume_positive(record: &solidcore_record::brep::Brep, shell: usize) -> Result<bool, Error> {
    use solidcore::mass::enclosure::{VolumeSign, volume_sign};
    use solidcore_record::brep::Region;

    let shell = u32::try_from(shell).map_err(|_| "too many shells".to_string())?;
    let mut single = record.clone();
    single.regions = vec![Region { shells: vec![shell], void: false }, Region { shells: vec![shell], void: false }];
    let single = single.canonical().map_err(|e| e.to_string())?.brep;
    let mut kernel = solidcore::kernel::Kernel::default();
    let mut body = c::brep::to_kernel_body(&mut kernel, &single, true).map_err(|e| e.to_string())?;
    match volume_sign(&kernel, &mut body).map_err(|e| e.to_string())? {
        VolumeSign::Positive(_) => Ok(true),
        VolumeSign::Negative(_) => Ok(false),
        VolumeSign::Undecided(_) => Err("the repaired shell's volume sign is not certified".into()),
    }
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

    pub(crate) fn winds(domain: [[f64; 2]; 2]) -> bool {
        k::form::winds(domain)
    }
    pub(crate) fn wound_surface(s: &Surface, domain: [[f64; 2]; 2], tolerance: f64) -> Option<(Surface, [[f64; 2]; 2])> {
        let (form, d) = k::form::wound_surface(&surface_in(s)?, domain, tolerance)?;
        Some((surface_out(form)?, d))
    }
    pub(crate) use solidcore::eval::form::{angle, param};

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
