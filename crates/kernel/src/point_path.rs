//! Sweep paths through 3D sketch curves ([`PointPath`]): polylines (linked, projected and
//! included curves, which reach the kernel as their points) and drawn 3D splines, which it gets
//! as their fit points and so exactly ([`solidcraft_geom::catmull_rom`]). Truck sweeps the
//! path's polyline ([`PointPath::points`]: the splines' samples); SMLib follows the path's
//! segments ([`PointPath::segs`]), each spline span exactly as a [`PathSeg::Cubic`].

use solidcraft_geom::bspline::{ArcLength, CurveN};
use solidcraft_geom::catmull_rom;
use solidcraft_geom::{Plane, Region2, Vec3};

use crate::body::Body;
use crate::{KernelError, PathSeg, Result, SweepControl};

/// One curve of a [`PointPath`], in the path's direction of travel.
#[derive(Clone, Debug, PartialEq)]
pub enum PathPiece {
    /// Points joined by straight lines.
    Polyline(Vec<Vec3>),
    /// A drawn 3D spline: the centripetal Catmull–Rom spline through `fit`, from its last fit
    /// point back to its first when `reversed`.
    Spline { fit: Vec<Vec3>, reversed: bool },
}

impl PathPiece {
    /// The piece's points in its direction of travel: the polyline, or the spline's samples
    /// ([`catmull_rom::samples`]).
    pub fn points(&self) -> Vec<Vec3> {
        match self {
            PathPiece::Polyline(pts) => pts.clone(),
            PathPiece::Spline { fit, reversed } => {
                let mut pts = catmull_rom::samples(fit);
                if *reversed {
                    pts.reverse();
                }
                pts
            }
        }
    }

    /// The piece the other way round.
    pub fn reversed(self) -> PathPiece {
        match self {
            PathPiece::Polyline(mut pts) => {
                pts.reverse();
                PathPiece::Polyline(pts)
            }
            PathPiece::Spline { fit, reversed } => PathPiece::Spline { fit, reversed: !reversed },
        }
    }

    /// A spline of three or more fit points as its exact spans ([`PathSeg::Cubic`]), in the
    /// direction of travel; `None` for a polyline (and a spline of one or two points, a point or
    /// a line).
    pub(crate) fn spans(&self) -> Result<Option<Vec<PathSeg>>> {
        let PathPiece::Spline { fit, reversed } = self else { return Ok(None) };
        let spans = catmull_rom::beziers(fit).ok_or_else(|| KernelError::Invalid("the 3D sketch spline is not finite".into()))?;
        if spans.is_empty() {
            return Ok(None);
        }
        let seg = |p: [Vec3; 4]| PathSeg::Cubic { p };
        Ok(Some(if *reversed {
            spans.into_iter().rev().map(|[a, b, c, d]| seg([d, c, b, a])).collect()
        } else {
            spans.into_iter().map(seg).collect()
        }))
    }
}

/// A sweep path through 3D sketch curves: its pieces end to end, each starting where the one
/// before it ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointPath {
    pub pieces: Vec<PathPiece>,
}

impl PointPath {
    /// A path of one polyline.
    pub fn polyline(pts: Vec<Vec3>) -> PointPath {
        PointPath { pieces: vec![PathPiece::Polyline(pts)] }
    }

    /// The path as one polyline: the first piece's points, then each next piece's after its
    /// first (where the one before ended).
    pub fn points(&self) -> Vec<Vec3> {
        let mut chain: Vec<Vec3> = Vec::new();
        for p in &self.pieces {
            let pts = p.points();
            if chain.is_empty() {
                chain = pts;
            } else {
                chain.extend(pts.into_iter().skip(1));
            }
        }
        chain
    }

    /// The path as segments: each spline span exactly ([`PathSeg::Cubic`]), each polyline as the
    /// lines between its points, starting where the piece before it ended (as [`Self::points`]
    /// chains them).
    pub(crate) fn segs(&self) -> Result<Vec<PathSeg>> {
        let mut out: Vec<PathSeg> = Vec::new();
        for piece in &self.pieces {
            if let Some(spans) = piece.spans()? {
                out.extend(spans);
                continue;
            }
            let mut pts = piece.points();
            if let (Some(end), Some(first)) = (out.last().map(PathSeg::end), pts.first_mut()) {
                *first = end;
            }
            out.extend(pts.windows(2).filter(|w| w[0].dist(w[1]) > 1e-9).map(|w| PathSeg::Line { a: w[0], b: w[1] }));
        }
        Ok(out)
    }
}

/// [`crate::sweep_path_with`] along a path through 3D sketch curves. Truck sweeps the path's
/// polyline ([`PointPath::points`]); SMLib follows its splines exactly and its polylines as lines.
pub fn sweep_point_path_with(plane: &Plane, region: &Region2, path: &PointPath, ctl: &SweepControl) -> Result<Body> {
    ctl.check()?;
    if crate::backend()? == crate::Backend::Smlib {
        return crate::sweep_path::smlib_sweep_points(plane, region, path, ctl);
    }
    crate::sweep_path_with(plane, region, &path.points(), ctl)
}

/// A cubic Bézier in space (a [`PathSeg::Cubic`]): its point at `x` (0…1).
pub(crate) fn cubic_at(p: &[Vec3; 4], x: f64) -> Vec3 {
    let y = 1.0 - x;
    p[0] * (y * y * y) + p[1] * (3.0 * x * y * y) + p[2] * (3.0 * x * x * y) + p[3] * (x * x * x)
}

/// Its derivative at `x`.
pub(crate) fn cubic_derivative(p: &[Vec3; 4], x: f64) -> Vec3 {
    let y = 1.0 - x;
    (p[1] - p[0]) * (3.0 * y * y) + (p[2] - p[1]) * (6.0 * x * y) + (p[3] - p[2]) * (3.0 * x * x)
}

/// The cubic split at `x` (de Casteljau): the part before and the part after.
#[cfg_attr(not(feature = "smlib"), allow(dead_code))]
pub(crate) fn cubic_split(p: &[Vec3; 4], x: f64) -> ([Vec3; 4], [Vec3; 4]) {
    let [a, b, c, d] = *p;
    let (ab, bc, cd) = (a.lerp(b, x), b.lerp(c, x), c.lerp(d, x));
    let (abc, bcd) = (ab.lerp(bc, x), bc.lerp(cd, x));
    let m = abc.lerp(bcd, x);
    ([a, ab, abc, m], [m, bcd, cd, d])
}

#[cfg_attr(not(feature = "smlib"), allow(dead_code))]
fn cubic_curve(p: &[Vec3; 4]) -> CurveN<3> {
    CurveN { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], ctrl: p.iter().map(|q| [q.x, q.y, q.z]).collect() }
}

/// Its length (`None` for a cubic that does not evaluate).
#[cfg_attr(not(feature = "smlib"), allow(dead_code))]
pub(crate) fn cubic_length(p: &[Vec3; 4]) -> Option<f64> {
    ArcLength::of(&cubic_curve(p)).map(|a| a.total())
}

/// The point `f` (0…1) of the way along the cubic by length, and the unit direction there.
#[cfg_attr(not(feature = "smlib"), allow(dead_code))]
pub(crate) fn cubic_along(p: &[Vec3; 4], f: f64) -> Option<(Vec3, Vec3)> {
    let c = cubic_curve(p);
    let arc = ArcLength::of(&c)?;
    let x = arc.param_at(&c, f * arc.total())?;
    Some((cubic_at(p, x), cubic_derivative(p, x).normalized()?))
}

/// Points along the cubic, without its start, the chords within `tol` of it (by the bound on
/// its second derivative, 6 max |P(i) - 2 P(i+1) + P(i+2)|: a chord of a piece `h` long in the
/// parameter is within h^2/8 of that of the curve).
pub(crate) fn cubic_polyline(p: &[Vec3; 4], tol: f64) -> Vec<Vec3> {
    let bend = (p[0] - p[1] * 2.0 + p[2]).len().max((p[1] - p[2] * 2.0 + p[3]).len()) * 6.0;
    let n = ((bend / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(1, 512);
    (1..=n).map(|i| cubic_at(p, i as f64 / n as f64)).collect()
}
