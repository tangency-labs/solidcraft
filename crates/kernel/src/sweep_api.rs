//! Sweep and loft inputs between this crate's types and the kernel interface's plain data
//! ([`solidcraft_kernel_api::sweep`]): copies field for field, so a path or option set comes back
//! exactly as it went.

use solidcraft_geom::bspline::HPoint;
use solidcraft_kernel_api::{PathSpan, PointPiece, SpaceSpan, SweepOptions};

use crate::point_path::{PathPiece, PointPath, SpaceBezier};
use crate::{KernelError, PathSeg, Result, SweepControl};

pub(crate) fn span(s: &PathSeg) -> PathSpan {
    match *s {
        PathSeg::Line { a, b } => PathSpan::Line { a, b },
        PathSeg::Arc { a, center, axis, angle } => PathSpan::Arc { a, center, axis, angle },
        PathSeg::Curve { plane, seg } => PathSpan::Curve { plane, seg },
        PathSeg::Bezier(b) => PathSpan::Bezier(SpaceSpan { ctrl: b.ctrl().iter().map(|q| [q.x, q.y, q.z, q.w]).collect() }),
    }
}

pub(crate) fn spans(path: &[PathSeg]) -> Vec<PathSpan> {
    path.iter().map(span).collect()
}

pub(crate) fn seg(s: &PathSpan) -> Result<PathSeg> {
    Ok(match s {
        PathSpan::Line { a, b } => PathSeg::Line { a: *a, b: *b },
        PathSpan::Arc { a, center, axis, angle } => PathSeg::Arc { a: *a, center: *center, axis: *axis, angle: *angle },
        PathSpan::Curve { plane, seg } => PathSeg::Curve { plane: *plane, seg: *seg },
        PathSpan::Bezier(b) => {
            let ctrl: Vec<HPoint> = b.ctrl.iter().map(|q| HPoint { x: q[0], y: q[1], z: q[2], w: q[3] }).collect();
            PathSeg::Bezier(
                SpaceBezier::new(&ctrl)
                    .ok_or_else(|| KernelError::Invalid("a path span needs 2 to 10 finite control points with positive weights".into()))?,
            )
        }
    })
}

pub(crate) fn segs(path: &[PathSpan]) -> Result<Vec<PathSeg>> {
    path.iter().map(seg).collect()
}

pub(crate) fn options(c: &SweepControl) -> SweepOptions {
    SweepOptions { orientation: c.orientation, twist: c.twist, scale: c.scale, rails: c.rails.clone(), rail_fit: c.rail_fit }
}

pub(crate) fn control(o: &SweepOptions) -> SweepControl {
    SweepControl { orientation: o.orientation, twist: o.twist, scale: o.scale, rails: o.rails.clone(), rail_fit: o.rail_fit }
}

pub(crate) fn point_path(p: &PointPath) -> solidcraft_kernel_api::PointPath {
    let piece = |x: &PathPiece| match x {
        PathPiece::Polyline(pts) => PointPiece::Polyline(pts.clone()),
        PathPiece::Spline { fit, reversed } => PointPiece::Spline { fit: fit.clone(), reversed: *reversed },
        PathPiece::Segs(s) => PointPiece::Spans(spans(s)),
    };
    solidcraft_kernel_api::PointPath { pieces: p.pieces.iter().map(piece).collect() }
}

pub(crate) fn path_of_points(p: &solidcraft_kernel_api::PointPath) -> Result<PointPath> {
    let piece = |x: &PointPiece| -> Result<PathPiece> {
        Ok(match x {
            PointPiece::Polyline(pts) => PathPiece::Polyline(pts.clone()),
            PointPiece::Spline { fit, reversed } => PathPiece::Spline { fit: fit.clone(), reversed: *reversed },
            PointPiece::Spans(s) => PathPiece::Segs(segs(s)?),
        })
    };
    Ok(PointPath { pieces: p.pieces.iter().map(piece).collect::<Result<_>>()? })
}
