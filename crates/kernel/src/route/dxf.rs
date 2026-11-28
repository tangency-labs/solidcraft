//! The router's DXF geometry ([`solidcraft_kernel_api::dxf::Dxf`]), as `solidcraft_kernel::dxf`:
//! what writing a DXF file asks of the kernel ([`export`]) and what reading one does ([`import`]).

use solidcraft_kernel_api::OpKind;

use super::{Which, which};
use crate::backend::Solidcore;

/// The kernel that runs DXF geometry.
fn kernel() -> Solidcore {
    match which(OpKind::Dxf) {
        Which::Solidcore => Solidcore,
    }
}

/// Writing DXF: a path's pieces converted for the file, their bounds, the drawing's scale and
/// translation, and the checks a written entity must pass.
pub mod export {
    use solidcore_record::{Seg2, Vec2};
    use solidcraft_kernel_api::dxf::Dxf;

    pub use solidcraft_kernel_api::dxf::{Bounds, PathRefusal, PathShape, PathStyle, ScalarEncoding, Vertex};

    use super::kernel;
    use crate::backend::Solidcore;

    /// Export geometry in the record's coordinates.
    pub type Shape = PathShape;

    /// The most source pieces, result entities or accumulated polyline vertices one path takes:
    /// the kernel's. This build holds solidcore only.
    pub const MAX_PATH_ITEMS: usize = <Solidcore as Dxf>::MAX_PATH_ITEMS;

    /// Check source geometry, style and budgets before any conversion.
    ///
    /// # Errors
    /// The kernel's refusal, naming the piece where it is about one.
    pub fn admit_path(segs: &[Seg2], style: PathStyle) -> Result<(), PathRefusal> {
        kernel().admit_path(segs, style)
    }

    /// Admit one piece, keeping its original index in any refusal.
    ///
    /// # Errors
    /// The kernel's refusal of the piece.
    pub fn admit_piece(seg: &Seg2, index: usize, total: usize) -> Result<(), PathRefusal> {
        kernel().admit_piece(seg, index, total)
    }

    /// A whole circle or ellipse recognised in a closed path, without converting separate pieces
    /// or fitting; `None` for an open path or one that is neither.
    ///
    /// # Errors
    /// The kernel's refusal of the path.
    pub fn recognized_path(segs: &[Seg2], closed: bool, style: PathStyle, encoding: ScalarEncoding) -> Result<Option<Shape>, PathRefusal> {
        kernel().recognized_path(segs, closed, style, encoding)
    }

    /// Admitted exact or polyline geometry converted; the caller fits free-form arcs.
    ///
    /// # Errors
    /// The kernel's refusal, naming the piece where it is about one.
    pub fn path_shapes(segs: &[Seg2], closed: bool, style: PathStyle, encoding: ScalarEncoding) -> Result<Vec<Shape>, PathRefusal> {
        kernel().path_shapes(segs, closed, style, encoding)
    }

    /// One source or fitted piece converted, with the original source index.
    ///
    /// # Errors
    /// The kernel's refusal of the piece.
    pub fn exact_piece(seg: &Seg2, index: usize, total: usize, encoding: ScalarEncoding) -> Result<Shape, PathRefusal> {
        kernel().exact_piece(seg, index, total, encoding)
    }

    /// The arc of a bulged edge from `from` to `to`, for checking a written vertex without
    /// changing its stored values; `None` where it has none.
    #[must_use]
    pub fn bulge_arc(from: Vertex, to: Vec2) -> Option<Seg2> {
        kernel().bulge_arc(from, to)
    }

    /// A bulged edge's endpoints and directed cardinal extrema, at most six points.
    ///
    /// # Errors
    /// The kernel's refusal of the edge.
    pub fn bulged_edge_points(from: Vertex, to: Vec2) -> Result<Vec<Vec2>, PathRefusal> {
        kernel().bulged_edge_points(from, to)
    }

    /// Whether degree-valued arc fields can be sampled finitely.
    #[must_use]
    pub fn arc_fields_finite(a0: f64, a1: f64) -> bool {
        kernel().arc_fields_finite(a0, a1)
    }

    /// Whether rounded degree endpoints remain distinct modulo one turn.
    #[must_use]
    pub fn distinct_degrees(start: f64, end: f64) -> bool {
        kernel().distinct_degrees(start, end)
    }

    /// Whether a degree value has a finite radian representation.
    #[must_use]
    pub fn finite_degrees(angle: f64) -> bool {
        kernel().finite_degrees(angle)
    }

    /// Whether an interval's difference is finite.
    #[must_use]
    pub fn finite_span(start: f64, end: f64) -> bool {
        kernel().finite_span(start, end)
    }

    /// A dimension scaled, refusing a result that is not positive and finite.
    ///
    /// # Errors
    /// The kernel's refusal of the scaled value.
    pub fn scaled_positive(value: f64, scale: f64) -> Result<f64, PathRefusal> {
        kernel().scaled_positive(value, scale)
    }

    /// Two validated bounds united.
    #[must_use]
    pub fn union_bounds(amin: Vec2, amax: Vec2, bmin: Vec2, bmax: Vec2) -> Bounds {
        kernel().union_bounds(amin, amax, bmin, bmax)
    }

    /// A whole circle's bounds, rounded outward.
    ///
    /// # Errors
    /// The kernel's refusal of the circle.
    pub fn supporting_circle(c: Vec2, r: f64) -> Result<Bounds, PathRefusal> {
        kernel().supporting_circle(c, r)
    }

    /// A whole ellipse's bounds, outward along its axes.
    ///
    /// # Errors
    /// The kernel's refusal of the ellipse.
    pub fn ellipse_bounds(c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> Result<Bounds, PathRefusal> {
        kernel().ellipse_bounds(c, major, ratio, t0, t1)
    }

    /// Stored points' bounds.
    ///
    /// # Errors
    /// The kernel's refusal of the points.
    pub fn point_bounds(points: &[Vec2]) -> Result<Bounds, PathRefusal> {
        kernel().point_bounds(points)
    }

    /// `p` under a positive uniform scale and a translation.
    ///
    /// # Errors
    /// The kernel's refusal of the scale or the result.
    pub fn transformed_point(p: Vec2, scale: f64, translation: Vec2) -> Result<Vec2, PathRefusal> {
        kernel().transformed_point(p, scale, translation)
    }

    /// An axis scaled, not translated.
    ///
    /// # Errors
    /// The kernel's refusal of the scale or the result.
    pub fn scaled_axis(axis: Vec2, scale: f64) -> Result<Vec2, PathRefusal> {
        kernel().scaled_axis(axis, scale)
    }

    /// Whether an axis has a finite nonzero length.
    #[must_use]
    pub fn axis_nonzero(axis: Vec2) -> bool {
        kernel().axis_nonzero(axis)
    }

    /// A bulged path's bounds: `count` vertices, at most `max_vertices` and `max_work`, checked
    /// before any vertex is read.
    ///
    /// # Errors
    /// The kernel's refusal: a count or work past the budget, a vertex count that differs from
    /// `count`, or a vertex it cannot bound.
    pub fn bulged_bounds(
        vertices: impl IntoIterator<Item = Vertex>,
        count: usize,
        closed: bool,
        max_vertices: usize,
        max_work: usize,
    ) -> Result<Bounds, PathRefusal> {
        kernel().bulged_bounds(&mut vertices.into_iter(), count, closed, max_vertices, max_work)
    }

    /// A retained source anchor under a scale and a translation.
    #[must_use]
    pub fn transform_anchor(point: Vec2, scale: f64, translation: Vec2) -> Vec2 {
        kernel().transform_anchor(point, scale, translation)
    }
}

/// Reading DXF: a source's unit scale and the checks a read entity must pass.
pub mod import {
    use solidcore_record::Vec2;
    use solidcraft_kernel_api::dxf::Dxf;

    pub use solidcraft_kernel_api::dxf::EllipseSupport;

    use super::kernel;

    /// Coordinates scaled by a source's unit.
    #[must_use]
    pub fn scale_point(point: Vec2, scale: f64) -> Vec2 {
        kernel().scale_point(point, scale)
    }

    /// A stored scalar scaled by a source's unit.
    #[must_use]
    pub fn scale_value(value: f64, scale: f64) -> f64 {
        kernel().scale_value(value, scale)
    }

    /// Whether a unit's scale collapsed a nonzero coordinate of `source` to zero in `scaled`.
    #[must_use]
    pub fn collapsed_coordinate(source: Vec2, scaled: Vec2) -> bool {
        kernel().collapsed_coordinate(source, scaled)
    }

    /// Whether a degree value has a finite radian representation.
    #[must_use]
    pub fn finite_degrees(angle: f64) -> bool {
        kernel().finite_degrees(angle)
    }

    /// Whether a read ellipse's supporting axes can be taken as they are, without changing either.
    #[must_use]
    pub fn ellipse_support(center: Vec2, major: Vec2, ratio: f64) -> EllipseSupport {
        kernel().ellipse_support(center, major, ratio)
    }
}

#[cfg(test)]
mod tests {
    use solidcore_ops::dxf as ops;
    use solidcore_record::{Seg2, Vec2};

    use super::{export, import};

    const ENCODING: export::ScalarEncoding =
        export::ScalarEncoding { length: Some, dimensionless: Some, min_ellipse_ratio: 1e-12, turn_snap_radians: 1e-12 };
    const OPS_ENCODING: ops::export::ScalarEncoding =
        ops::export::ScalarEncoding { length: Some, dimensionless: Some, min_ellipse_ratio: 1e-12, turn_snap_radians: 1e-12 };

    #[test]
    fn routed_dxf_geometry_is_solidcores() {
        let p = Vec2::new;
        assert_eq!(export::MAX_PATH_ITEMS, ops::export::MAX_PATH_ITEMS);
        let segs = [Seg2::Line { a: p(0.0, 0.0), b: p(4.0, 0.0) }, Seg2::Line { a: p(4.0, 0.0), b: p(4.0, 3.0) }];
        let routed = export::path_shapes(&segs, false, export::PathStyle::Polyline, ENCODING).unwrap();
        let direct = ops::export::path_shapes(&segs, false, ops::export::PathStyle::Polyline, OPS_ENCODING).unwrap();
        assert_eq!(format!("{routed:?}"), format!("{direct:?}"));
        let b = export::union_bounds(p(0.0, 0.0), p(1.0, 1.0), p(-1.0, 0.5), p(0.5, 2.0));
        let k = ops::export::union_bounds(p(0.0, 0.0), p(1.0, 1.0), p(-1.0, 0.5), p(0.5, 2.0));
        assert_eq!((b.min, b.max), (k.min, k.max));
        let v = export::Vertex { at: p(0.0, 0.0), bulge: 1.0 };
        let kv = ops::export::PathVertex { at: p(0.0, 0.0), bulge: 1.0 };
        assert_eq!(export::bulged_edge_points(v, p(2.0, 0.0)).unwrap(), ops::export::bulged_edge_points(kv, p(2.0, 0.0)).unwrap());
        assert_eq!(export::bulge_arc(v, p(2.0, 0.0)), ops::export::bulge_arc(kv, p(2.0, 0.0)));
        assert_eq!(import::scale_point(p(1.5, 2.0), 25.4), ops::import::scale_point(p(1.5, 2.0), 25.4));
        assert_eq!(import::ellipse_support(p(0.0, 0.0), p(2.0, 0.0), 0.5), import::EllipseSupport::Valid);
        assert_eq!(import::ellipse_support(p(0.0, 0.0), p(0.0, 0.0), 0.5), import::EllipseSupport::InvalidMajor);
        // a refusal carries solidcore's words and piece whole
        let bad = [Seg2::Line { a: p(f64::NAN, 0.0), b: p(4.0, 0.0) }];
        let k = ops::export::admit_path(&bad, ops::export::PathStyle::Exact).unwrap_err();
        let r = export::admit_path(&bad, export::PathStyle::Exact).unwrap_err();
        assert_eq!((r.piece, r.reason), (k.piece, k.reason));
        let k = ops::export::scaled_positive(-1.0, 2.0).unwrap_err();
        let r = export::scaled_positive(-1.0, 2.0).unwrap_err();
        assert_eq!((r.piece, r.reason), (k.piece, k.reason));
    }
}
