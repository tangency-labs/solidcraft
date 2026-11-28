//! Exact projection workflows for spline controls and certified circular arcs.
//!
//! Every geometric computation is a kernel primitive. This layer converts the
//! record data and chooses the sketch's representation and control budget.

use crate::error::status;
use crate::prism::{PlanarPiece, PlanarSpline};
use crate::Result;
use solidcore::curve::conic_projection as k;
use solidcore_convert::{plane, record_vec2, vec2};
pub use solidcore_record::projection::{PlaneRelation, Projection};
use solidcore_record::{Plane, Vec2};

pub(crate) fn record_projection(p: k::Projection) -> Projection {
    match p {
        k::Projection::Segment(s) => Projection::Segment(solidcore_convert::record_seg2(&s)),
        k::Projection::Point(p) => Projection::Point(record_vec2(p)),
        k::Projection::Line(a, b) => Projection::Line(record_vec2(a), record_vec2(b)),
        k::Projection::Spline {
            spline,
            degree,
            implicit,
        } => Projection::Spline {
            spline: PlanarSpline {
                degree: spline.degree,
                knots: spline.knots,
                points: spline.points.into_iter().map(record_vec2).collect(),
                weights: spline.weights,
            },
            degree,
            implicit,
        },
        k::Projection::Conic { a, apex, b, rho } => Projection::Conic {
            a: record_vec2(a),
            apex: record_vec2(apex),
            b: record_vec2(b),
            rho,
        },
        k::Projection::Ellipse {
            center,
            major,
            minor,
        } => Projection::Ellipse {
            center: record_vec2(center),
            major: record_vec2(major),
            minor,
        },
    }
}

fn kernel_projection(p: Projection) -> k::Projection {
    match p {
        Projection::Segment(s) => k::Projection::Segment(solidcore_convert::seg2(&s)),
        Projection::Point(p) => k::Projection::Point(vec2(p)),
        Projection::Line(a, b) => k::Projection::Line(vec2(a), vec2(b)),
        Projection::Spline {
            spline,
            degree,
            implicit,
        } => k::Projection::Spline {
            spline: solidcore::prism::read::PlanarSpline {
                degree: spline.degree,
                knots: spline.knots,
                points: spline.points.into_iter().map(vec2).collect(),
                weights: spline.weights,
            },
            degree,
            implicit,
        },
        Projection::Conic { a, apex, b, rho } => k::Projection::Conic {
            a: vec2(a),
            apex: vec2(apex),
            b: vec2(b),
            rho,
        },
        Projection::Ellipse {
            center,
            major,
            minor,
        } => k::Projection::Ellipse {
            center: vec2(center),
            major: vec2(major),
            minor,
        },
    }
}

/// Ask the kernel for the relationship between two planes.
pub fn plane_relation(source: &Plane, target: &Plane) -> PlaneRelation {
    let r = k::plane_relation(&plane(source), &plane(target));
    PlaneRelation {
        parallel: r.parallel,
        edge_on: r.edge_on,
        flip: r.flip,
    }
}

/// Project points between two plane frames.
pub fn project_points(source: &Plane, target: &Plane, points: &[Vec2]) -> Vec<Vec2> {
    k::project_points(
        &plane(source),
        &plane(target),
        &points.iter().copied().map(vec2).collect::<Vec<_>>(),
    )
    .into_iter()
    .map(record_vec2)
    .collect()
}

/// Project a control spline, retaining the source's uniform or explicit representation.
pub fn project_control(
    source: &Plane,
    target: &Plane,
    points: &[Vec2],
    degree: usize,
    metadata: Option<(&[f64], &[f64])>,
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    k::project_control_between(
        &plane(source),
        &plane(target),
        &points.iter().copied().map(vec2).collect::<Vec<_>>(),
        degree,
        metadata,
        collapse,
        max_coord,
    )
    .map(|g| g.into_iter().map(record_projection).collect())
    .map_err(status)
}

/// Keep the complete image of one linear source once.
fn coalesce_linear(geometry: Vec<Projection>) -> Vec<Projection> {
    k::coalesce_linear(geometry.into_iter().map(kernel_projection).collect())
        .into_iter()
        .map(record_projection)
        .collect()
}

/// Project exact cubic spans, retaining one image per span and merging complete linear images.
pub fn project_beziers(
    source: &Plane,
    target: &Plane,
    pieces: &[[Vec2; 4]],
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let mut out = Vec::new();
    for piece in pieces {
        out.extend(project_control(
            source, target, piece, 3, None, collapse, max_coord,
        )?);
    }
    Ok(coalesce_linear(out))
}

/// Fit the source spline exactly, project its controls, and retain its complete image.
pub fn project_fit(
    source: &Plane,
    target: &Plane,
    points: &[Vec2],
    periodic: bool,
    limit: usize,
    collapse: bool,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let source_frame = plane(source);
    let target_frame = plane(target);
    let points = points.iter().copied().map(vec2).collect::<Vec<_>>();
    let scale = k::projection_scale(&source_frame, &target_frame, &points);
    let mut out = Vec::new();
    for s in
        solidcore::curve::fit_projection::fit_splines(&points, periodic, limit).map_err(status)?
    {
        let points = k::project_points(&source_frame, &target_frame, &s.points);
        out.extend(
            k::classify_control(
                points,
                s.degree,
                Some((&s.knots, &s.weights)),
                scale,
                collapse,
                max_coord,
            )
            .map_err(status)?
            .into_iter()
            .map(record_projection),
        );
    }
    Ok(coalesce_linear(out))
}

/// Project the circular arcs `arcs` of the `source` plane onto `target` exactly: a full circle
/// as an ellipse, arcs as rational quadratic conics, and a circle or arc seen edge on as the
/// whole line it covers, out to its own extremes (not to those of samples of it). The arcs must be
/// one chain on one circle, as a sketch circle or arc gives them: the same centre and radius,
/// each starting where the one before ends, turning one way, at most a full turn in all. `None`
/// when a piece is not an arc or the arcs are not such a chain (two arcs over the same half turn
/// cover half the circle, not the whole of it).
pub fn project_arcs(
    source: &Plane,
    target: &Plane,
    arcs: &[solidcore_record::Seg2],
    max_coord: f64,
) -> Result<Option<Vec<Projection>>> {
    let pieces: Vec<_> = arcs
        .iter()
        .map(|s| solidcore::prism::read::PlanarPiece::Seg(solidcore_convert::seg2(s)))
        .collect();
    if !k::one_arc_chain(&pieces) {
        return Ok(None);
    }
    k::project_arcs(&plane(target), &plane(source), &pieces, max_coord)
        .map(|g| g.map(|g| g.into_iter().map(record_projection).collect()))
        .map_err(status)
}

/// Classify already projected native pieces, using the kernel's exact spline splitting.
pub fn classify_pieces(
    target: &Plane,
    pieces: &[PlanarPiece],
    limit: usize,
    max_coord: f64,
) -> Result<Vec<Projection>> {
    let pieces: Vec<_> = pieces
        .iter()
        .map(|p| match p {
            PlanarPiece::Seg(s) => {
                solidcore::prism::read::PlanarPiece::Seg(solidcore_convert::seg2(s))
            }
            PlanarPiece::Spline(s) => {
                solidcore::prism::read::PlanarPiece::Spline(solidcore::prism::read::PlanarSpline {
                    degree: s.degree,
                    knots: s.knots.clone(),
                    points: s.points.iter().copied().map(vec2).collect(),
                    weights: s.weights.clone(),
                })
            }
        })
        .collect();
    k::classify_pieces(&plane(target), &pieces, limit, max_coord)
        .map(|g| g.into_iter().map(record_projection).collect())
        .map_err(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::{Seg2, Vec3};
    use std::f64::consts::{FRAC_1_SQRT_2, PI};

    fn half(start: f64) -> Seg2 {
        Seg2::Arc {
            center: Vec2::new(-35.0, 0.0),
            radius: 6.0,
            start,
            sweep: PI,
        }
    }

    /// A plane tilted 45 degrees about X, so a circle on XY is seen as an ellipse.
    fn tilted() -> Plane {
        Plane {
            origin: Vec3::ZERO,
            x: Vec3::X,
            y: Vec3::new(0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        }
    }

    #[test]
    fn two_identical_semicircles_are_not_a_full_circle() {
        let got = project_arcs(&Plane::XY, &tilted(), &[half(0.0), half(0.0)], 1e6).unwrap();
        assert!(got.is_none(), "{got:?}");
        let gap = project_arcs(&Plane::XY, &tilted(), &[half(0.0), half(PI + 0.1)], 1e6).unwrap();
        assert!(gap.is_none(), "{gap:?}");
    }

    #[test]
    fn two_halves_end_to_end_are_the_whole_circle() {
        let got = project_arcs(&Plane::XY, &tilted(), &[half(0.0), half(PI)], 1e6).unwrap();
        assert!(
            matches!(got.as_deref(), Some([Projection::Ellipse { .. }])),
            "{got:?}"
        );
    }

    #[test]
    fn a_circle_seen_edge_on_is_its_whole_diameter() {
        let got = project_arcs(&Plane::XY, &Plane::XZ, &[half(0.0), half(PI)], 1e6).unwrap();
        let Some([Projection::Line(a, b)]) = got.as_deref() else {
            panic!("{got:?}");
        };
        let (lo, hi) = (a.x.min(b.x), a.x.max(b.x));
        assert!(
            (lo + 41.0).abs() < 1e-9 && (hi + 29.0).abs() < 1e-9,
            "{a:?} {b:?}"
        );
        assert!(a.y.abs() < 1e-9 && b.y.abs() < 1e-9, "{a:?} {b:?}");
    }
}
