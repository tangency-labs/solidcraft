//! solidcore's DXF geometry ([`Dxf`]): each the solidcore-ops call this crate's callers made
//! before the interface existed, its values converted field for field and its refusals' words
//! kept whole.

use solidcore_ops::dxf::{export, import};
use solidcore_record::{Seg2, Vec2};
use solidcraft_kernel_api::dxf::{Bounds, Dxf, EllipseSupport, PathRefusal, PathShape, PathStyle, ScalarEncoding, Vertex};

use super::Solidcore;

fn refusal(r: export::PathRefusal) -> PathRefusal {
    PathRefusal { piece: r.piece, reason: r.reason }
}

fn style(s: PathStyle) -> export::PathStyle {
    match s {
        PathStyle::Exact => export::PathStyle::Exact,
        PathStyle::Polyline => export::PathStyle::Polyline,
        PathStyle::Arcs { tolerance_mm } => export::PathStyle::Arcs { tolerance_mm },
    }
}

fn encoding(e: ScalarEncoding) -> export::ScalarEncoding {
    export::ScalarEncoding {
        length: e.length,
        dimensionless: e.dimensionless,
        min_ellipse_ratio: e.min_ellipse_ratio,
        turn_snap_radians: e.turn_snap_radians,
    }
}

fn vertex(v: Vertex) -> export::PathVertex {
    export::PathVertex { at: v.at, bulge: v.bulge }
}

fn bounds(b: export::Bounds) -> Bounds {
    Bounds { min: b.min, max: b.max }
}

fn shape(s: export::Shape) -> PathShape {
    use export::PathShape as S;
    match s {
        S::Line(a, b) => PathShape::Line(a, b),
        S::Circle { c, r } => PathShape::Circle { c, r },
        S::Arc { c, r, a0, a1 } => PathShape::Arc { c, r, a0, a1 },
        S::Ellipse { c, major, ratio, t0, t1 } => PathShape::Ellipse { c, major, ratio, t0, t1 },
        S::Spline { degree, knots, ctrl, weights } => PathShape::Spline { degree, knots, ctrl, weights },
        S::BulgedPolyline { vertices, closed } => {
            PathShape::BulgedPolyline { vertices: vertices.into_iter().map(|v| Vertex { at: v.at, bulge: v.bulge }).collect(), closed }
        }
    }
}

impl Dxf for Solidcore {
    const MAX_PATH_ITEMS: usize = export::MAX_PATH_ITEMS;

    fn admit_path(&self, segs: &[Seg2], s: PathStyle) -> Result<(), PathRefusal> {
        export::admit_path(segs, style(s)).map_err(refusal)
    }

    fn admit_piece(&self, seg: &Seg2, index: usize, total: usize) -> Result<(), PathRefusal> {
        export::admit_piece(seg, index, total).map_err(refusal)
    }

    fn recognized_path(&self, segs: &[Seg2], closed: bool, s: PathStyle, e: ScalarEncoding) -> Result<Option<PathShape>, PathRefusal> {
        export::recognized_path(segs, closed, style(s), encoding(e)).map(|v| v.map(shape)).map_err(refusal)
    }

    fn path_shapes(&self, segs: &[Seg2], closed: bool, s: PathStyle, e: ScalarEncoding) -> Result<Vec<PathShape>, PathRefusal> {
        export::path_shapes(segs, closed, style(s), encoding(e)).map(|v| v.into_iter().map(shape).collect()).map_err(refusal)
    }

    fn exact_piece(&self, seg: &Seg2, index: usize, total: usize, e: ScalarEncoding) -> Result<PathShape, PathRefusal> {
        export::exact_piece(seg, index, total, encoding(e)).map(shape).map_err(refusal)
    }

    fn bulge_arc(&self, from: Vertex, to: Vec2) -> Option<Seg2> {
        export::bulge_arc(vertex(from), to)
    }

    fn bulged_edge_points(&self, from: Vertex, to: Vec2) -> Result<Vec<Vec2>, PathRefusal> {
        export::bulged_edge_points(vertex(from), to).map_err(refusal)
    }

    fn arc_fields_finite(&self, a0: f64, a1: f64) -> bool {
        export::arc_fields_finite(a0, a1)
    }

    fn distinct_degrees(&self, start: f64, end: f64) -> bool {
        export::distinct_degrees(start, end)
    }

    fn finite_degrees(&self, angle: f64) -> bool {
        export::finite_degrees(angle)
    }

    fn finite_span(&self, start: f64, end: f64) -> bool {
        export::finite_span(start, end)
    }

    fn scaled_positive(&self, value: f64, scale: f64) -> Result<f64, PathRefusal> {
        export::scaled_positive(value, scale).map_err(refusal)
    }

    fn union_bounds(&self, amin: Vec2, amax: Vec2, bmin: Vec2, bmax: Vec2) -> Bounds {
        bounds(export::union_bounds(amin, amax, bmin, bmax))
    }

    fn supporting_circle(&self, c: Vec2, r: f64) -> Result<Bounds, PathRefusal> {
        export::supporting_circle(c, r).map(bounds).map_err(refusal)
    }

    fn ellipse_bounds(&self, c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> Result<Bounds, PathRefusal> {
        export::ellipse_bounds(c, major, ratio, t0, t1).map(bounds).map_err(refusal)
    }

    fn point_bounds(&self, points: &[Vec2]) -> Result<Bounds, PathRefusal> {
        export::point_bounds(points).map(bounds).map_err(refusal)
    }

    fn transformed_point(&self, p: Vec2, scale: f64, translation: Vec2) -> Result<Vec2, PathRefusal> {
        export::transformed_point(p, scale, translation).map_err(refusal)
    }

    fn scaled_axis(&self, axis: Vec2, scale: f64) -> Result<Vec2, PathRefusal> {
        export::scaled_axis(axis, scale).map_err(refusal)
    }

    fn axis_nonzero(&self, axis: Vec2) -> bool {
        export::axis_nonzero(axis)
    }

    fn bulged_bounds(
        &self,
        vertices: &mut dyn Iterator<Item = Vertex>,
        count: usize,
        closed: bool,
        max_vertices: usize,
        max_work: usize,
    ) -> Result<Bounds, PathRefusal> {
        export::bulged_bounds(vertices.map(vertex), count, closed, max_vertices, max_work).map(bounds).map_err(refusal)
    }

    fn transform_anchor(&self, point: Vec2, scale: f64, translation: Vec2) -> Vec2 {
        export::transform_anchor(point, scale, translation)
    }

    fn scale_point(&self, point: Vec2, scale: f64) -> Vec2 {
        import::scale_point(point, scale)
    }

    fn scale_value(&self, value: f64, scale: f64) -> f64 {
        import::scale_value(value, scale)
    }

    fn collapsed_coordinate(&self, source: Vec2, scaled: Vec2) -> bool {
        import::collapsed_coordinate(source, scaled)
    }

    fn ellipse_support(&self, center: Vec2, major: Vec2, ratio: f64) -> EllipseSupport {
        match import::ellipse_support(center, major, ratio) {
            import::EllipseSupport::Valid => EllipseSupport::Valid,
            import::EllipseSupport::InvalidMajor => EllipseSupport::InvalidMajor,
            import::EllipseSupport::CollapsedMinor => EllipseSupport::CollapsedMinor,
        }
    }
}
