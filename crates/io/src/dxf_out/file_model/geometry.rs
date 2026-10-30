//! Conservative bounds and positive uniform transforms for owned DXF geometry.
//!
//! POINT bounds are its stored location; the positive transform maps that point.
//! ARC bounds enclose its supporting circle; ELLIPSE uses an outward L1 box;
//! clamped continuous positive-weight SPLINE uses its control-point hull.
//! These bounds are conservative, not tight. TEXT requires explicit world-space
//! layout metrics rather than guessed glyph widths. Unsupported spline forms
//! return an error. Stored styles, parameters and array order are preserved.
//! Variable copy payload and geometry work are independently bounded before
//! array scans/copies using MAX_DRAWING_BYTES. Neither bound promises peak RAM,
//! final encoded file size, callback cost or exact floating-point equivalence.

use solidcraft_geom::Vec2;

use super::super::{DxfShape, DxfVertex};
use super::DxfFile;
use crate::{IoError, Result};

fn invalid(message: &str) -> IoError {
    IoError::Invalid(message.into())
}

/// Axis-aligned bounds in drawing millimetres, without physical stroke width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfGeometryBounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl DxfGeometryBounds {
    pub fn new(min: Vec2, max: Vec2) -> Result<Self> {
        if !min.is_finite() || !max.is_finite() || min.x > max.x || min.y > max.y {
            return Err(invalid("DXF bounds must be finite and ordered"));
        }
        Ok(Self { min, max })
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: Vec2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Vec2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }
}

fn finite_point(point: Vec2) -> Result<Vec2> {
    if point.is_finite() { Ok(point) } else { Err(invalid("nonfinite DXF point")) }
}

fn positive(value: f64) -> Result<f64> {
    if value.is_finite() && value > 0.0 { Ok(value) } else { Err(invalid("DXF dimension must be finite and positive")) }
}

fn supporting_circle(c: Vec2, r: f64) -> Result<DxfGeometryBounds> {
    finite_point(c)?;
    positive(r)?;
    // One outward ULP encloses each correctly rounded sum/difference, including
    // subnormal results. Overflow/infinite outward bounds are explicitly refused.
    DxfGeometryBounds::new(Vec2::new((c.x - r).next_down(), (c.y - r).next_down()), Vec2::new((c.x + r).next_up(), (c.y + r).next_up()))
}

fn valid_arc(a0: f64, a1: f64) -> Result<()> {
    // Preserve stored angles, including equal endpoints; resolving equal-endpoint
    // ARC semantics is the reader/writer's job, not an invented full-turn rule.
    if a0.is_finite() && a1.is_finite() { Ok(()) } else { Err(invalid("nonfinite DXF arc angle")) }
}

fn valid_text(at: Vec2, height: f64, angle_deg: f64) -> Result<()> {
    finite_point(at)?;
    positive(height)?;
    if angle_deg.is_finite() { Ok(()) } else { Err(invalid("nonfinite DXF text angle")) }
}

fn point_bounds(points: &[Vec2]) -> Result<DxfGeometryBounds> {
    let first = finite_point(*points.first().ok_or_else(|| invalid("DXF point array is empty"))?)?;
    let mut bounds = DxfGeometryBounds::new(first, first)?;
    for point in points.iter().skip(1) {
        let point = finite_point(*point)?;
        bounds = bounds.union(DxfGeometryBounds::new(point, point)?);
    }
    Ok(bounds)
}

fn valid_polyline(points: &[Vec2]) -> Result<()> {
    if points.len() < 2 {
        return Err(invalid("DXF polyline needs at least two vertices"));
    }
    for point in points {
        finite_point(*point)?;
    }
    Ok(())
}

fn ellipse_bounds(c: Vec2, major: Vec2, ratio: f64, t0: f64, t1: f64) -> Result<DxfGeometryBounds> {
    finite_point(c)?;
    finite_point(major)?;
    if major == Vec2::ZERO || !ratio.is_finite() || ratio <= 0.0 || ratio > 1.0 || !t0.is_finite() || !t1.is_finite() {
        return Err(invalid("invalid DXF ellipse support or parameters"));
    }
    // |x-cx| <= |major.x| + ratio*|major.y|, likewise for y.
    // Each nonnegative product and sum is rounded outward independently.
    // abs is exact for finite f64. No hypot/trig accuracy assumption is used.
    let x = (major.x.abs() + (ratio * major.y.abs()).next_up()).next_up();
    let y = (major.y.abs() + (ratio * major.x.abs()).next_up()).next_up();
    DxfGeometryBounds::new(Vec2::new((c.x - x).next_down(), (c.y - y).next_down()), Vec2::new((c.x + x).next_up(), (c.y + y).next_up()))
}

fn valid_spline(degree: u8, knots: &[f64], ctrl: &[Vec2], weights: Option<&[f64]>) -> Result<()> {
    let degree = usize::from(degree);
    if degree == 0 || ctrl.len() <= degree {
        return Err(invalid("DXF spline needs positive degree and at least degree+1 controls"));
    }
    let count = ctrl.len().checked_add(degree).and_then(|n| n.checked_add(1)).ok_or_else(|| invalid("DXF spline knot count overflow"))?;
    if knots.len() != count {
        return Err(invalid("invalid DXF spline knot count"));
    }
    for point in ctrl {
        finite_point(*point)?;
    }
    if knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| match pair {
            [a, b] => a > b,
            _ => true,
        })
    {
        return Err(invalid("DXF spline knots must be finite and nondecreasing"));
    }
    let start = *knots.get(degree).ok_or_else(|| invalid("missing DXF spline start knot"))?;
    let end = *knots.get(ctrl.len()).ok_or_else(|| invalid("missing DXF spline end knot"))?;
    if start >= end {
        return Err(invalid("DXF spline parameter domain must be nonempty"));
    }
    let prefix = knots.get(..=degree).ok_or_else(|| invalid("missing DXF spline start clamp"))?;
    let suffix = knots.get(ctrl.len()..).ok_or_else(|| invalid("missing DXF spline end clamp"))?;
    if prefix.iter().any(|value| *value != start) || suffix.iter().any(|value| *value != end) {
        return Err(invalid("not supported yet: unclamped DXF spline bounds/transform"));
    }
    let interior = knots.get(degree + 1..ctrl.len()).ok_or_else(|| invalid("invalid DXF spline interior knot range"))?;
    let mut previous = None;
    let mut repetitions = 0usize;
    for value in interior {
        if *value <= start || *value >= end {
            return Err(invalid("DXF spline endpoint knot multiplicity exceeds degree+1"));
        }
        repetitions = if previous == Some(*value) { repetitions + 1 } else { 1 };
        if repetitions > degree {
            return Err(invalid("not supported yet: discontinuous DXF spline interior knot"));
        }
        previous = Some(*value);
    }
    if let Some(weights) = weights {
        if weights.len() != ctrl.len() {
            return Err(invalid("invalid DXF spline weight count"));
        }
        for weight in weights {
            positive(*weight)?;
        }
    }
    Ok(())
}

fn copied_array<T: Copy>(input: &[T]) -> Result<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(input.len()).map_err(|_| invalid("DXF array allocation failed"))?;
    out.extend_from_slice(input);
    Ok(out)
}

fn transformed_points(points: &[Vec2], scale: f64, translation: Vec2) -> Result<Vec<Vec2>> {
    let mut out = Vec::new();
    out.try_reserve_exact(points.len()).map_err(|_| invalid("DXF point array allocation failed"))?;
    let mut previous: Option<(Vec2, Vec2)> = None;
    for point in points {
        let transformed = transformed_point(*point, scale, translation)?;
        if let Some((before, after)) = previous
            && before != *point
            && after == transformed
        {
            return Err(invalid("DXF transform collapses adjacent distinct points"));
        }
        out.push(transformed);
        previous = Some((*point, transformed));
    }
    Ok(out)
}

fn shape_bounds<F>(shape: &DxfShape, text_bounds: &mut F) -> Result<DxfGeometryBounds>
where
    F: FnMut(&DxfShape) -> Result<DxfGeometryBounds>,
{
    match shape {
        DxfShape::BulgedPolyline { vertices, closed } => {
            // The file-level length-only preflight already charged every
            // entity's interval work cumulatively before this value scan.
            super::bulges::bounds(vertices, *closed, super::super::path::MAX_PATH_ITEMS, crate::MAX_DRAWING_BYTES)
        }
        DxfShape::Point(point) => {
            let point = finite_point(*point)?;
            DxfGeometryBounds::new(point, point)
        }
        DxfShape::Line(a, b) => {
            finite_point(*a)?;
            finite_point(*b)?;
            DxfGeometryBounds::new(Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)))
        }
        DxfShape::Circle { c, r } => supporting_circle(*c, *r),
        DxfShape::Polyline { points } => {
            valid_polyline(points)?;
            point_bounds(points)
        }
        DxfShape::Ellipse { c, major, ratio, t0, t1 } => ellipse_bounds(*c, *major, *ratio, *t0, *t1),
        DxfShape::Spline { degree, knots, ctrl, weights } => {
            valid_spline(*degree, knots, ctrl, weights.as_deref())?;
            point_bounds(ctrl)
        }
        DxfShape::Arc { c, r, a0, a1 } => {
            valid_arc(*a0, *a1)?;
            supporting_circle(*c, *r)
        }
        DxfShape::Text { at, height, angle_deg, .. } | DxfShape::AlignedText { at, height, angle_deg, .. } => {
            valid_text(*at, *height, *angle_deg)?;
            let supplied = text_bounds(shape)?;
            // Public fields do not bypass validation of callback output.
            DxfGeometryBounds::new(supplied.min, supplied.max)
        }
    }
}

fn transformed_point(point: Vec2, scale: f64, translation: Vec2) -> Result<Vec2> {
    finite_point(point)?;
    // Explicit single-rounding semantics, not vector arithmetic with an
    // overflowing intermediate multiplication that translation might cancel.
    finite_point(Vec2::new(point.x.mul_add(scale, translation.x), point.y.mul_add(scale, translation.y)))
}

fn transformed_shape(shape: &DxfShape, scale: f64, translation: Vec2) -> Result<DxfShape> {
    match shape {
        DxfShape::BulgedPolyline { vertices, closed } => {
            super::super::admit_bulged(vertices, *closed)?;
            let mut copied = Vec::new();
            copied.try_reserve_exact(vertices.len()).map_err(|_| invalid("DXF bulged vertex allocation failed"))?;
            let mut previous: Option<(Vec2, Vec2)> = None;
            for vertex in vertices {
                let at = transformed_point(vertex.at, scale, translation)?;
                if let Some((old, last)) = previous
                    && old != vertex.at
                    && last == at
                {
                    return Err(invalid("DXF transform collapses adjacent distinct bulged vertices"));
                }
                copied.push(DxfVertex { at, bulge: vertex.bulge });
                previous = Some((vertex.at, at));
            }
            // This retains raw outgoing signs/zero bits and the original seam
            // flag; it does not reconstruct/re-fit arcs or invoke strict bounds.
            super::super::admit_bulged(&copied, *closed)?;
            Ok(DxfShape::BulgedPolyline { vertices: copied, closed: *closed })
        }
        DxfShape::Point(point) => Ok(DxfShape::Point(transformed_point(*point, scale, translation)?)),
        DxfShape::Line(a, b) => {
            let start = transformed_point(*a, scale, translation)?;
            let end = transformed_point(*b, scale, translation)?;
            if a != b && start == end {
                return Err(invalid("DXF transform collapses a nonzero line"));
            }
            Ok(DxfShape::Line(start, end))
        }
        DxfShape::Circle { c, r } => {
            positive(*r)?;
            let c = transformed_point(*c, scale, translation)?;
            let r = positive(*r * scale)?;
            supporting_circle(c, r)?;
            Ok(DxfShape::Circle { c, r })
        }
        DxfShape::Polyline { points } => {
            valid_polyline(points)?;
            Ok(DxfShape::Polyline { points: transformed_points(points, scale, translation)? })
        }
        DxfShape::Ellipse { c, major, ratio, t0, t1 } => {
            ellipse_bounds(*c, *major, *ratio, *t0, *t1)?;
            let c = transformed_point(*c, scale, translation)?;
            let major = finite_point(Vec2::new(major.x * scale, major.y * scale))?;
            ellipse_bounds(c, major, *ratio, *t0, *t1)?;
            Ok(DxfShape::Ellipse { c, major, ratio: *ratio, t0: *t0, t1: *t1 })
        }
        DxfShape::Spline { degree, knots, ctrl, weights } => {
            valid_spline(*degree, knots, ctrl, weights.as_deref())?;
            let ctrl = transformed_points(ctrl, scale, translation)?;
            let knots = copied_array(knots)?;
            let weights = weights.as_deref().map(copied_array).transpose()?;
            Ok(DxfShape::Spline { degree: *degree, knots, ctrl, weights })
        }
        DxfShape::Arc { c, r, a0, a1 } => {
            valid_arc(*a0, *a1)?;
            positive(*r)?;
            let c = transformed_point(*c, scale, translation)?;
            let r = positive(*r * scale)?;
            supporting_circle(c, r)?;
            Ok(DxfShape::Arc { c, r, a0: *a0, a1: *a1 })
        }
        DxfShape::Text { at, height, angle_deg, text } => {
            valid_text(*at, *height, *angle_deg)?;
            let at = transformed_point(*at, scale, translation)?;
            let height = positive(*height * scale)?;
            let mut copy = String::new();
            copy.try_reserve(text.len()).map_err(|_| invalid("DXF text allocation failed"))?;
            copy.push_str(text);
            Ok(DxfShape::Text { at, height, angle_deg: *angle_deg, text: copy })
        }
        DxfShape::AlignedText { at, height, angle_deg, text, horizontal, vertical } => {
            valid_text(*at, *height, *angle_deg)?;
            let at = transformed_point(*at, scale, translation)?;
            let height = positive(*height * scale)?;
            let mut copy = String::new();
            copy.try_reserve(text.len()).map_err(|_| invalid("DXF text allocation failed"))?;
            copy.push_str(text);
            Ok(DxfShape::AlignedText { at, height, angle_deg: *angle_deg, text: copy, horizontal: *horizontal, vertical: *vertical })
        }
    }
}

fn accumulated_text_bytes(total: usize, next: usize, limit: usize) -> Result<usize> {
    let total = total.checked_add(next).ok_or_else(|| invalid("DXF cumulative TEXT byte count overflow"))?;
    if total > limit {
        return Err(invalid("DXF cumulative TEXT payload exceeds drawing byte limit"));
    }
    Ok(total)
}

#[derive(Default)]
struct GeometryBudget {
    copy_bytes: usize,
    work: usize,
}

impl GeometryBudget {
    fn charge(&mut self, copy: usize, work: usize, copy_limit: usize, work_limit: usize) -> Result<()> {
        self.copy_bytes = self.copy_bytes.checked_add(copy).ok_or_else(|| invalid("DXF geometry copy byte count overflow"))?;
        self.work = self.work.checked_add(work).ok_or_else(|| invalid("DXF geometry work count overflow"))?;
        if self.copy_bytes > copy_limit {
            return Err(invalid("DXF cumulative geometry payload exceeds drawing byte limit"));
        }
        if self.work > work_limit {
            return Err(invalid("DXF cumulative geometry work exceeds drawing limit"));
        }
        Ok(())
    }

    fn array<T>(&mut self, len: usize, copy_limit: usize, work_limit: usize) -> Result<()> {
        let bytes = len.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| invalid("DXF array byte count overflow"))?;
        let work = len.checked_mul(8).ok_or_else(|| invalid("DXF array work count overflow"))?;
        self.charge(bytes, work, copy_limit, work_limit)
    }
}

impl DxfFile<DxfShape> {
    /// None means no entities. TEXT requires extents_with_text_bounds instead.
    /// ARC extents are conservative circle bounds, not minimum arc bounds.
    pub fn extents(&self) -> Result<Option<DxfGeometryBounds>> {
        self.extents_with_text_bounds(|_| Err(invalid("not supported yet: TEXT extents without font/layout bounds")))
    }

    /// The callback supplies complete world-space bounds for the real TEXT
    /// layout, including rotation and the actual stored alignment (centre on
    /// baseline for legacy Text). It owns metric
    /// accuracy; this adapter only checks finiteness/order. No text is serialized.
    pub fn extents_with_text_bounds<F>(&self, mut text_bounds: F) -> Result<Option<DxfGeometryBounds>>
    where
        F: FnMut(&DxfShape) -> Result<DxfGeometryBounds>,
    {
        self.preflight_geometry(crate::MAX_DRAWING_BYTES, crate::MAX_DRAWING_BYTES)?;
        let mut bounds: Option<DxfGeometryBounds> = None;
        for entity in self.entities() {
            self.layer(entity.layer())?;
            let next = shape_bounds(entity.geometry(), &mut text_bounds)?;
            bounds = Some(match bounds {
                None => next,
                Some(previous) => previous.union(next),
            });
        }
        Ok(bounds)
    }

    /// Returns a fresh owning file; source and all source handles remain intact
    /// even on refusal. Order, raw layer names, styles, and optional/symbolic
    /// weights are retained. Physical lineweights are NOT geometry-scaled.
    /// Scale must be finite and positive: reflections, zero and negative scales
    /// are unsupported. Only clamped continuous positive-weight splines are
    /// admitted. Variable geometry bytes and geometry work are preflighted.
    pub fn translated_scaled(&self, translation: Vec2, scale: f64) -> Result<Self> {
        self.translated_scaled_with_text_limit(translation, scale, crate::MAX_DRAWING_BYTES)
    }

    // Private small-limit seam for boundary tests without allocating 64 MiB.
    fn translated_scaled_with_text_limit(&self, translation: Vec2, scale: f64, text_limit: usize) -> Result<Self> {
        self.translated_scaled_with_limits(translation, scale, text_limit, crate::MAX_DRAWING_BYTES)
    }

    fn preflight_geometry(&self, copy_limit: usize, work_limit: usize) -> Result<()> {
        let mut budget = GeometryBudget::default();
        let mut text_bytes = 0usize;
        for entity in self.entities() {
            // Work units bound our geometry scans/copies, not callback work,
            // string comparison cost in layer declarations, or wall-clock time.
            budget.charge(0, 32, copy_limit, work_limit)?;
            match entity.geometry() {
                DxfShape::BulgedPolyline { vertices, closed } => {
                    let count = vertices.len();
                    if !(2..=super::super::path::MAX_PATH_ITEMS).contains(&count) {
                        return Err(invalid("bulged vertex count exceeds supported range"));
                    }
                    let edges = if *closed { count } else { count - 1 };
                    // Mirrors the exact pinned d8b helper's n+512*edges cost;
                    // array copy adds8*n. Charge globally BEFORE any values.
                    let interval_work = edges
                        .checked_mul(512)
                        .and_then(|work| work.checked_add(count))
                        .ok_or_else(|| invalid("DXF bulged interval work count overflow"))?;
                    budget.array::<DxfVertex>(count, copy_limit, work_limit)?;
                    budget.charge(0, interval_work, copy_limit, work_limit)?;
                }
                DxfShape::Text { text, .. } | DxfShape::AlignedText { text, .. } => {
                    text_bytes = accumulated_text_bytes(text_bytes, text.len(), copy_limit)?;
                    budget.charge(text.len(), text.len(), copy_limit, work_limit)?;
                }
                DxfShape::Polyline { points } => budget.array::<Vec2>(points.len(), copy_limit, work_limit)?,
                DxfShape::Spline { knots, ctrl, weights, .. } => {
                    budget.array::<f64>(knots.len(), copy_limit, work_limit)?;
                    budget.array::<Vec2>(ctrl.len(), copy_limit, work_limit)?;
                    if let Some(weights) = weights {
                        budget.array::<f64>(weights.len(), copy_limit, work_limit)?;
                    }
                }
                DxfShape::Point(_) | DxfShape::Line(..) | DxfShape::Circle { .. } | DxfShape::Arc { .. } | DxfShape::Ellipse { .. } => {}
            }
        }
        Ok(())
    }

    fn translated_scaled_with_limits(&self, translation: Vec2, scale: f64, copy_limit: usize, work_limit: usize) -> Result<Self> {
        finite_point(translation)?;
        positive(scale)?;
        // Length-only preflight before validation scans, output or copying.
        self.preflight_geometry(copy_limit, work_limit)?;
        // Layer0 is immutable after construction. Seed the fresh owner with
        // its actual source style before the normal layer-handle remap.
        let zero = self.layer(self.default_layer())?;
        let mut output = Self::with_layer_zero(zero.color(), zero.linetype(), zero.weight())?;
        let mut layers = Vec::new();
        layers.try_reserve(self.layers().len()).map_err(|_| invalid("DXF layer remap allocation failed"))?;
        for layer in self.layers() {
            layers.push(output.declare_layer(&layer.original, layer.color(), layer.linetype(), layer.weight())?);
        }
        for entity in self.entities() {
            self.layer(entity.layer())?;
            let layer = layers.get(entity.layer().index).copied().ok_or_else(|| invalid("missing DXF layer remap"))?;
            output.push(layer, transformed_shape(entity.geometry(), scale, translation)?, entity.weight())?;
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::DxfLinetype;
    use super::super::super::{DxfTextHorizontalAlignment, DxfTextVerticalAlignment};

    #[test]
    fn bulged_copy_preserves_vertices_raw_signs_seams_and_custom_zero_metadata() {
        let physical = StandardWeight::from_code(50).unwrap();
        let mut file = DxfFile::with_layer_zero(3, DxfLinetype::Hidden, Some(physical)).unwrap();
        let vertices = vec![
            DxfVertex { at: Vec2::new(0.0, 0.0), bulge: 0.5 },
            DxfVertex { at: Vec2::new(4.0, 0.0), bulge: -0.5 },
            DxfVertex { at: Vec2::new(8.0, 0.0), bulge: -0.0 },
        ];
        file.push(file.default_layer(), DxfShape::BulgedPolyline { vertices: vertices.clone(), closed: false }, Some(EntityWeight::ByLayer)).unwrap();
        let copied = file.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
        let DxfShape::BulgedPolyline { vertices: result, closed } = copied.entities()[0].geometry() else { panic!("expected real bulged shape") };
        assert!(!*closed);
        assert_eq!(result.iter().map(|v| v.at).collect::<Vec<_>>(), [Vec2::new(10.0, -5.0), Vec2::new(18.0, -5.0), Vec2::new(26.0, -5.0)]);
        assert_eq!(result.iter().map(|v| v.bulge.to_bits()).collect::<Vec<_>>(), vertices.iter().map(|v| v.bulge.to_bits()).collect::<Vec<_>>());
        assert_eq!(copied.effective_weight(0).unwrap(), EntityWeight::Explicit(physical));
        assert_eq!(copied.effective_linetype(0).unwrap(), DxfLinetype::Hidden);
        assert!(copied.layer(file.default_layer()).is_err());
        assert_eq!(file.entities()[0].geometry(), &DxfShape::BulgedPolyline { vertices, closed: false });
    }

    #[test]
    fn bulged_closed_copy_uses_last_bulge_and_conservative_geometry_bounds() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::BulgedPolyline {
                vertices: vec![DxfVertex { at: Vec2::new(-1.0, 0.0), bulge: 1.0 }, DxfVertex { at: Vec2::new(1.0, 0.0), bulge: 1.0 }],
                closed: true,
            },
            None,
        )
        .unwrap();
        let copied = file.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
        let DxfShape::BulgedPolyline { vertices, closed } = copied.entities()[0].geometry() else { panic!("expected real bulged shape") };
        assert!(*closed);
        assert_eq!(vertices.len(), 2);
        assert_eq!((vertices[0].at, vertices[1].at), (Vec2::new(8.0, -5.0), Vec2::new(12.0, -5.0)));
        assert_eq!((vertices[0].bulge, vertices[1].bulge), (1.0, 1.0));
        let b = copied.extents().unwrap().unwrap();
        for point in [Vec2::new(8.0, -5.0), Vec2::new(12.0, -5.0), Vec2::new(10.0, -7.0), Vec2::new(10.0, -3.0)] {
            assert!(b.min.x <= point.x && point.x <= b.max.x && b.min.y <= point.y && point.y <= b.max.y);
        }
    }

    #[test]
    fn bulged_copy_and_bounds_preflight_cumulative_bytes_and_work_before_values() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::BulgedPolyline {
                vertices: vec![DxfVertex { at: Vec2::ZERO, bulge: 0.0 }, DxfVertex { at: Vec2::X, bulge: 0.0 }],
                closed: false,
            },
            None,
        )
        .unwrap();
        let bytes = 2 * std::mem::size_of::<DxfVertex>();
        // Fixed32 + copy8*n + helper(n+512*edges) =562.
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, bytes, 562).is_ok());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, bytes - 1, 562).is_err());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, bytes, 561).is_err());
        let mut hostile = DxfFile::new().unwrap();
        hostile
            .push(
                hostile.default_layer(),
                DxfShape::BulgedPolyline {
                    vertices: vec![DxfVertex { at: Vec2::new(f64::NAN, 0.0), bulge: 0.0 }, DxfVertex { at: Vec2::X, bulge: 0.0 }],
                    closed: false,
                },
                None,
            )
            .unwrap();
        assert!(hostile.translated_scaled_with_limits(Vec2::ZERO, 1.0, bytes, 561).unwrap_err().to_string().contains("work"));
        let second = file.entities()[0].geometry().clone();
        file.push(file.default_layer(), second, None).unwrap();
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 2 * bytes, 1124).is_ok());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 2 * bytes, 1123).is_err());
    }

    #[test]
    fn strict_bulged_bounds_are_not_total_copy_admission_and_collapsed_vertices_refuse() {
        let mut file = DxfFile::new().unwrap();
        let original = DxfShape::BulgedPolyline {
            vertices: vec![DxfVertex { at: Vec2::ZERO, bulge: 1e-200 }, DxfVertex { at: Vec2::new(1e-200, 0.0), bulge: 0.0 }],
            closed: false,
        };
        file.push(file.default_layer(), original.clone(), None).unwrap();
        assert!(file.extents().is_err()); // b*b underflows under interval policy.
        assert_eq!(file.translated_scaled(Vec2::ZERO, 1.0).unwrap().entities()[0].geometry(), &original);
        let mut line = DxfFile::new().unwrap();
        line.push(
            line.default_layer(),
            DxfShape::BulgedPolyline {
                vertices: vec![DxfVertex { at: Vec2::ZERO, bulge: 0.0 }, DxfVertex { at: Vec2::X, bulge: 0.0 }],
                closed: false,
            },
            None,
        )
        .unwrap();
        assert!(line.translated_scaled(Vec2::new(1e300, 0.0), 1.0).is_err());
        assert_eq!(file.entities()[0].geometry(), &original);
    }

    #[test]
    fn identity_and_positive_copy_preserve_custom_layer_zero_and_remap_all_handles() {
        let physical_zero = StandardWeight::from_code(0).unwrap();
        let physical_50 = StandardWeight::from_code(50).unwrap();
        let cut_weight = StandardWeight::from_code(25).unwrap();
        let explicit_13 = EntityWeight::Explicit(StandardWeight::from_code(13).unwrap());
        let original = [
            DxfShape::Line(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)),
            DxfShape::Point(Vec2::new(-1.0, 0.0)),
            DxfShape::Point(Vec2::new(0.0, 1.0)),
            DxfShape::Point(Vec2::new(2.0, -1.0)),
            DxfShape::Point(Vec2::new(4.0, 2.0)),
            DxfShape::Line(Vec2::new(0.0, -2.0), Vec2::new(2.0, -2.0)),
        ];
        let moved = [
            DxfShape::Line(Vec2::new(12.0, -1.0), Vec2::new(16.0, 3.0)),
            DxfShape::Point(Vec2::new(8.0, -5.0)),
            DxfShape::Point(Vec2::new(10.0, -3.0)),
            DxfShape::Point(Vec2::new(14.0, -7.0)),
            DxfShape::Point(Vec2::new(18.0, -1.0)),
            DxfShape::Line(Vec2::new(10.0, -9.0), Vec2::new(14.0, -9.0)),
        ];
        let weights = [None, Some(EntityWeight::ByLayer), Some(EntityWeight::ByBlock), Some(EntityWeight::Default), Some(explicit_13), None];
        for zero_weight in [None, Some(physical_zero), Some(physical_50)] {
            let mut source = DxfFile::with_layer_zero(3, DxfLinetype::Hidden, zero_weight).unwrap();
            let zero = source.default_layer();
            let cut = source.declare_layer("CUT", 5, DxfLinetype::Dashed, Some(cut_weight)).unwrap();
            for (index, shape) in original.iter().enumerate() {
                source.push(if index == 5 { cut } else { zero }, shape.clone(), weights[index]).unwrap();
            }
            for (translation, scale, expected) in [(Vec2::ZERO, 1.0, &original), (Vec2::new(10.0, -5.0), 2.0, &moved)] {
                let copied = source.translated_scaled(translation, scale).unwrap();
                assert_eq!(copied.layers().iter().map(|layer| layer.name()).collect::<Vec<_>>(), ["0", "CUT"]);
                let new_zero = copied.layer(copied.default_layer()).unwrap();
                assert_eq!((new_zero.color(), new_zero.linetype(), new_zero.weight()), (3, DxfLinetype::Hidden, zero_weight));
                let new_cut = copied.layer(copied.entities()[5].layer()).unwrap();
                assert_eq!(
                    (new_cut.name(), new_cut.color(), new_cut.linetype(), new_cut.weight()),
                    ("CUT", 5, DxfLinetype::Dashed, Some(cut_weight))
                );
                assert!(copied.layer(zero).is_err() && copied.layer(cut).is_err());
                assert!(source.layer(copied.default_layer()).is_err());
                assert!(source.layer(copied.entities()[5].layer()).is_err());
                for (index, (entity, shape)) in copied.entities().iter().zip(expected).enumerate() {
                    assert_eq!(entity.geometry(), shape);
                    assert_eq!(entity.weight(), weights[index]);
                    assert_eq!(copied.effective_linetype(index).unwrap(), if index == 5 { DxfLinetype::Dashed } else { DxfLinetype::Hidden });
                    if index < 5 {
                        assert_eq!(entity.layer(), copied.default_layer());
                    }
                }
                let inherited = zero_weight.map_or(EntityWeight::Default, EntityWeight::Explicit);
                assert_eq!(copied.effective_weight(0).unwrap(), inherited);
                assert_eq!(copied.effective_weight(1).unwrap(), inherited);
                assert_eq!(copied.effective_weight(2).unwrap(), EntityWeight::ByBlock);
                assert_eq!(copied.effective_weight(3).unwrap(), EntityWeight::Default);
                assert_eq!(copied.effective_weight(4).unwrap(), explicit_13);
                assert_eq!(copied.effective_weight(5).unwrap(), EntityWeight::Explicit(cut_weight));
            }
            assert_eq!(source.entities().len(), original.len());
            for (index, (entity, shape)) in source.entities().iter().zip(&original).enumerate() {
                assert_eq!(entity.geometry(), shape);
                assert_eq!(entity.weight(), weights[index]);
            }
            let original_zero = source.layer(zero).unwrap();
            assert_eq!((original_zero.color(), original_zero.linetype(), original_zero.weight()), (3, DxfLinetype::Hidden, zero_weight));
        }
    }

    #[test]
    fn all_nine_alignments_use_explicit_bounds_and_transform_without_losing_style() {
        for horizontal in [DxfTextHorizontalAlignment::Left, DxfTextHorizontalAlignment::Centre, DxfTextHorizontalAlignment::Right] {
            for vertical in [DxfTextVerticalAlignment::Baseline, DxfTextVerticalAlignment::Middle, DxfTextVerticalAlignment::Top] {
                let mut file = DxfFile::new().unwrap();
                let physical = StandardWeight::from_code(50).unwrap();
                let layer = file.declare_layer("NOTES", 4, DxfLinetype::Hidden, Some(physical)).unwrap();
                let original =
                    DxfShape::AlignedText { at: Vec2::new(2.0, 3.0), height: 4.0, angle_deg: 90.0, text: "A".into(), horizontal, vertical };
                file.push(layer, original.clone(), Some(EntityWeight::ByLayer)).unwrap();
                assert!(file.extents().is_err());
                let provided = DxfGeometryBounds::new(Vec2::new(-2.0, 1.0), Vec2::new(3.0, 6.0)).unwrap();
                assert_eq!(
                    file.extents_with_text_bounds(|shape| {
                        assert_eq!(shape, &original);
                        Ok(provided)
                    })
                    .unwrap(),
                    Some(provided)
                );
                let copied = file.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
                assert_eq!(
                    copied.entities()[0].geometry(),
                    &DxfShape::AlignedText { at: Vec2::new(14.0, 1.0), height: 8.0, angle_deg: 90.0, text: "A".into(), horizontal, vertical }
                );
                assert!(copied.extents().is_err());
                let moved_bounds = DxfGeometryBounds::new(Vec2::new(6.0, -3.0), Vec2::new(16.0, 7.0)).unwrap();
                assert_eq!(
                    copied
                        .extents_with_text_bounds(|shape| {
                            assert_eq!(shape, copied.entities()[0].geometry());
                            Ok(moved_bounds)
                        })
                        .unwrap(),
                    Some(moved_bounds)
                );
                assert_eq!(copied.entities()[0].weight(), Some(EntityWeight::ByLayer));
                assert_eq!(copied.effective_weight(0).unwrap(), EntityWeight::Explicit(physical));
                assert_eq!(copied.effective_linetype(0).unwrap(), DxfLinetype::Hidden);
                assert_eq!(file.entities()[0].geometry(), &original);
            }
        }
    }

    #[test]
    fn aligned_payload_participates_in_existing_cumulative_text_budget() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Text { at: Vec2::ZERO, height: 1.0, angle_deg: 0.0, text: "é".into() }, None).unwrap();
        file.push(
            file.default_layer(),
            DxfShape::AlignedText {
                at: Vec2::ZERO,
                height: 1.0,
                angle_deg: 0.0,
                text: "abc".into(),
                horizontal: DxfTextHorizontalAlignment::Left,
                vertical: DxfTextVerticalAlignment::Top,
            },
            None,
        )
        .unwrap();
        assert!(file.translated_scaled_with_text_limit(Vec2::ZERO, 1.0, 5).is_ok());
        assert!(file.translated_scaled_with_text_limit(Vec2::ZERO, 1.0, 4).is_err());
        assert_eq!(file.entities().len(), 2);
    }

    #[test]
    fn aligned_transform_and_callback_refusals_preserve_source() {
        let mut file = DxfFile::new().unwrap();
        let original = DxfShape::AlignedText {
            at: Vec2::new(f64::MAX, 0.0),
            height: 1.0,
            angle_deg: 0.0,
            text: "A".into(),
            horizontal: DxfTextHorizontalAlignment::Right,
            vertical: DxfTextVerticalAlignment::Top,
        };
        file.push(file.default_layer(), original.clone(), Some(EntityWeight::Default)).unwrap();
        assert!(file.translated_scaled(Vec2::ZERO, 2.0).is_err());
        assert!(file.extents_with_text_bounds(|_| Ok(DxfGeometryBounds { min: Vec2::new(1.0, 1.0), max: Vec2::ZERO })).is_err());
        assert_eq!(file.entities()[0].geometry(), &original);
        assert_eq!(file.entities()[0].weight(), Some(EntityWeight::Default));
    }

    #[test]
    fn point_has_exact_bounds_and_positive_transform_preserves_metadata() {
        let mut file = DxfFile::new().unwrap();
        let weight = StandardWeight::from_code(50).unwrap();
        let layer = file.declare_layer("DATUM", 4, DxfLinetype::Hidden, Some(weight)).unwrap();
        let point = Vec2::new(-3.0, 4.0);
        file.push(layer, DxfShape::Point(point), Some(EntityWeight::ByLayer)).unwrap();
        assert_eq!(file.extents().unwrap(), Some(DxfGeometryBounds::new(point, point).unwrap()));
        let copied = file.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
        assert_eq!(copied.entities()[0].geometry(), &DxfShape::Point(Vec2::new(4.0, 3.0)));
        assert_eq!(copied.entities()[0].weight(), Some(EntityWeight::ByLayer));
        assert_eq!(copied.effective_weight(0).unwrap(), EntityWeight::Explicit(weight));
        assert_eq!(copied.effective_linetype(0).unwrap(), DxfLinetype::Hidden);
        assert!(copied.layer(layer).is_err());
        assert_eq!(file.entities()[0].geometry(), &DxfShape::Point(point));
    }

    #[test]
    fn point_nonfinite_or_overflowing_transform_leaves_source_intact() {
        for point in [Vec2::new(f64::NAN, 0.0), Vec2::new(0.0, f64::INFINITY)] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), DxfShape::Point(point), None).unwrap();
            assert!(file.extents().is_err());
            assert!(file.translated_scaled(Vec2::ZERO, 1.0).is_err());
            assert_eq!(file.entities().len(), 1);
        }
        let mut file = DxfFile::new().unwrap();
        let point = Vec2::new(f64::MAX, 1.0);
        file.push(file.default_layer(), DxfShape::Point(point), Some(EntityWeight::ByBlock)).unwrap();
        assert!(file.translated_scaled(Vec2::ZERO, 2.0).is_err());
        assert_eq!(file.entities()[0].geometry(), &DxfShape::Point(point));
        assert_eq!(file.entities()[0].weight(), Some(EntityWeight::ByBlock));
    }

    #[test]
    fn point_costs_fixed_work_and_no_variable_copy_payload() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Point(Vec2::ZERO), None).unwrap();
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 0, 32).is_ok());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 0, 31).is_err());
    }
    use super::super::{EntityWeight, StandardWeight};
    use super::*;

    #[test]
    fn empty_bounds_and_invalid_transform_parameters() {
        let file = DxfFile::<DxfShape>::new().unwrap();
        assert_eq!(file.extents().unwrap(), None);
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(file.translated_scaled(Vec2::new(0.0, 0.0), scale).is_err());
        }
        assert!(file.translated_scaled(Vec2::new(f64::NAN, 0.0), 1.0).is_err());
    }

    #[test]
    fn line_bounds_have_analytic_endpoints() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Line(Vec2::new(-2.0, 7.0), Vec2::new(4.0, -3.0)), None).unwrap();
        assert_eq!(file.extents().unwrap(), Some(DxfGeometryBounds::new(Vec2::new(-2.0, -3.0), Vec2::new(4.0, 7.0)).unwrap()));
    }

    #[test]
    fn copy_preserves_raw_names_metadata_and_mints_new_handles() {
        let mut file = DxfFile::new().unwrap();
        let weight = StandardWeight::from_code(50).unwrap();
        let layer = file.declare_layer(" CUT ", 3, DxfLinetype::Hidden, Some(weight)).unwrap();
        file.push(layer, DxfShape::Line(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)), Some(EntityWeight::ByLayer)).unwrap();
        file.push(file.default_layer(), DxfShape::Circle { c: Vec2::new(0.0, 1.0), r: 2.0 }, None).unwrap();
        let mut copy = file.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
        assert_eq!(copy.entities()[0].geometry(), &DxfShape::Line(Vec2::new(12.0, -1.0), Vec2::new(16.0, 3.0)));
        assert_eq!(copy.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(10.0, -3.0), r: 4.0 });
        assert_eq!(copy.entities()[0].weight(), Some(EntityWeight::ByLayer));
        assert_eq!(copy.entities()[1].weight(), None);
        assert_eq!(copy.effective_weight(0).unwrap(), EntityWeight::Explicit(weight));
        assert_eq!(copy.effective_linetype(0).unwrap(), DxfLinetype::Hidden);
        assert!(copy.layer(layer).is_err());
        assert!(copy.declare_layer(" CUT ", 3, DxfLinetype::Hidden, Some(weight)).is_ok());
        assert!(copy.declare_layer("CUT", 3, DxfLinetype::Hidden, Some(weight)).is_err());
        assert_eq!(file.entities()[0].geometry(), &DxfShape::Line(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)));
    }

    #[test]
    fn wrapped_arc_bounds_enclose_support_and_transform_preserves_angles() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Arc { c: Vec2::new(3.0, -1.0), r: 2.0, a0: 350.0, a1: 10.0 }, Some(EntityWeight::ByBlock)).unwrap();
        let bounds = file.extents().unwrap().unwrap();
        assert!(bounds.min.x <= 1.0 && bounds.max.x >= 5.0);
        assert!(bounds.min.y <= -3.0 && bounds.max.y >= 1.0);
        let copy = file.translated_scaled(Vec2::new(1.0, 2.0), 3.0).unwrap();
        assert_eq!(copy.entities()[0].geometry(), &DxfShape::Arc { c: Vec2::new(10.0, -1.0), r: 6.0, a0: 350.0, a1: 10.0 });
        assert_eq!(copy.entities()[0].weight(), Some(EntityWeight::ByBlock));
    }

    #[test]
    fn text_uses_explicit_metrics_and_retains_alignment_payload() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::Text { at: Vec2::new(2.0, 3.0), height: 4.0, angle_deg: 90.0, text: "A".into() },
            Some(EntityWeight::Default),
        )
        .unwrap();
        assert!(file.extents().is_err());
        // Synthetic font contract: rotated glyph corners (1,2) and (3,6).
        let metrics = DxfGeometryBounds::new(Vec2::new(1.0, 2.0), Vec2::new(3.0, 6.0)).unwrap();
        assert_eq!(
            file.extents_with_text_bounds(|shape| {
                assert!(matches!(shape, DxfShape::Text { text, .. } if text == "A"));
                Ok(metrics)
            })
            .unwrap(),
            Some(metrics)
        );
        assert!(file.extents_with_text_bounds(|_| Ok(DxfGeometryBounds { min: Vec2::new(f64::NAN, 0.0), max: Vec2::new(1.0, 1.0) })).is_err());
        let copy = file.translated_scaled(Vec2::new(-1.0, 2.0), 2.0).unwrap();
        assert_eq!(copy.entities()[0].geometry(), &DxfShape::Text { at: Vec2::new(3.0, 8.0), height: 8.0, angle_deg: 90.0, text: "A".into() });
    }

    #[test]
    fn invalid_polyline_refuses_without_changing_the_source() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Polyline { points: Vec::new() }, None).unwrap();
        assert!(file.extents().is_err());
        assert!(file.translated_scaled(Vec2::new(0.0, 0.0), 1.0).is_err());
        assert_eq!(file.entities().len(), 1);
    }

    #[test]
    fn hostile_geometry_overflow_underflow_and_collapse_refuse() {
        for shape in [
            DxfShape::Line(Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 0.0)),
            DxfShape::Circle { c: Vec2::new(0.0, 0.0), r: -1.0 },
            DxfShape::Arc { c: Vec2::new(0.0, 0.0), r: 1.0, a0: f64::INFINITY, a1: 1.0 },
            DxfShape::Text { at: Vec2::new(0.0, 0.0), height: 0.0, angle_deg: 0.0, text: String::new() },
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape, None).unwrap();
            assert!(file.extents().is_err());
            assert!(file.translated_scaled(Vec2::new(0.0, 0.0), 1.0).is_err());
        }
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Circle { c: Vec2::new(0.0, 0.0), r: f64::MIN_POSITIVE }, None).unwrap();
        assert!(file.translated_scaled(Vec2::new(0.0, 0.0), f64::from_bits(1)).is_err());
        let mut line = DxfFile::new().unwrap();
        line.push(line.default_layer(), DxfShape::Line(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)), None).unwrap();
        assert!(line.translated_scaled(Vec2::new(1e300, 0.0), 1.0).is_err());
        assert!(line.translated_scaled(Vec2::new(0.0, 0.0), f64::MAX).unwrap().extents().is_ok());
        assert!(line.translated_scaled(Vec2::new(f64::MAX, 0.0), f64::MAX).is_err());
    }

    #[test]
    fn cumulative_text_byte_limit_accepts_boundary_and_preflights_all_payloads() {
        let mut file = DxfFile::new().unwrap();
        for text in ["é", "abc"] {
            file.push(file.default_layer(), DxfShape::Text { at: Vec2::new(0.0, 0.0), height: 1.0, angle_deg: 0.0, text: text.into() }, None)
                .unwrap();
        }
        let origin = Vec2::new(0.0, 0.0);
        assert!(file.translated_scaled_with_text_limit(origin, 1.0, 5).is_ok());
        assert!(file.translated_scaled_with_text_limit(origin, 1.0, 4).is_err());
        assert_eq!(file.entities().len(), 2);
        assert!(accumulated_text_bytes(usize::MAX, 1, usize::MAX).is_err());
        assert_eq!(accumulated_text_bytes(2, 3, 5).unwrap(), 5);
        // Even a preceding malformed shape cannot start output processing:
        // the text-resource refusal is observed before validating that shape.
        let mut unsupported_first = DxfFile::new().unwrap();
        unsupported_first.push(unsupported_first.default_layer(), DxfShape::Polyline { points: Vec::new() }, None).unwrap();
        unsupported_first
            .push(unsupported_first.default_layer(), DxfShape::Text { at: origin, height: 1.0, angle_deg: 0.0, text: "abc".into() }, None)
            .unwrap();
        let error = unsupported_first.translated_scaled_with_text_limit(origin, 1.0, 2).unwrap_err();
        assert!(error.to_string().contains("cumulative TEXT payload"));
    }

    #[test]
    fn polyline_bounds_transform_and_repeated_vertices_keep_order() {
        let mut file = DxfFile::new().unwrap();
        let points = vec![Vec2::new(-2.0, 3.0), Vec2::new(-2.0, 3.0), Vec2::new(4.0, -1.0)];
        file.push(file.default_layer(), DxfShape::Polyline { points }, None).unwrap();
        assert_eq!(file.extents().unwrap(), Some(DxfGeometryBounds::new(Vec2::new(-2.0, -1.0), Vec2::new(4.0, 3.0)).unwrap()));
        let copy = file.translated_scaled(Vec2::new(1.0, -2.0), 2.0).unwrap();
        assert_eq!(
            copy.entities()[0].geometry(),
            &DxfShape::Polyline { points: vec![Vec2::new(-3.0, 4.0), Vec2::new(-3.0, 4.0), Vec2::new(9.0, -4.0)] }
        );
    }

    #[test]
    fn rotated_ellipse_l1_support_is_conservative_and_parameters_survive_copy() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::Ellipse { c: Vec2::new(10.0, -2.0), major: Vec2::new(3.0, 4.0), ratio: 0.5, t0: 5.0, t1: 1.0 },
            None,
        )
        .unwrap();
        let bounds = file.extents().unwrap().unwrap();
        // Full-support L1 half extents: (3+4/2, 4+3/2) = (5, 5.5).
        assert!(bounds.min.x <= 5.0 && bounds.max.x >= 15.0);
        assert!(bounds.min.y <= -7.5 && bounds.max.y >= 3.5);
        let copy = file.translated_scaled(Vec2::new(1.0, 1.0), 2.0).unwrap();
        assert_eq!(
            copy.entities()[0].geometry(),
            &DxfShape::Ellipse { c: Vec2::new(21.0, -3.0), major: Vec2::new(6.0, 8.0), ratio: 0.5, t0: 5.0, t1: 1.0 }
        );
    }

    #[test]
    fn rational_spline_bounds_use_hull_and_copy_retains_knots_weights_and_degree() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::Spline {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                ctrl: vec![Vec2::new(0.0, 0.0), Vec2::new(2.0, 4.0), Vec2::new(4.0, 0.0)],
                weights: Some(vec![1.0, 0.5, 1.0]),
            },
            None,
        )
        .unwrap();
        assert_eq!(file.extents().unwrap(), Some(DxfGeometryBounds::new(Vec2::new(0.0, 0.0), Vec2::new(4.0, 4.0)).unwrap()));
        let copy = file.translated_scaled(Vec2::new(1.0, -2.0), 2.0).unwrap();
        assert_eq!(
            copy.entities()[0].geometry(),
            &DxfShape::Spline {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                ctrl: vec![Vec2::new(1.0, -2.0), Vec2::new(5.0, 6.0), Vec2::new(9.0, -2.0)],
                weights: Some(vec![1.0, 0.5, 1.0]),
            }
        );
        // A multi-span clamped nonrational curve is also in the declared subset.
        assert!(valid_spline(2, &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], &[Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::new(1.0, 1.0)], None).is_ok());
    }

    #[test]
    fn malformed_or_unsupported_splines_refuse_before_claiming_hull_bounds() {
        let ctrl = [Vec2::ZERO, Vec2::X, Vec2::Y];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        assert!(valid_spline(0, &knots, &ctrl, None).is_err());
        assert!(valid_spline(3, &knots, &ctrl, None).is_err());
        assert!(valid_spline(2, &knots[..5], &ctrl, None).is_err());
        assert!(valid_spline(2, &[0.0, 0.0, 0.0, 1.0, 0.5, 1.0], &ctrl, None).is_err());
        assert!(valid_spline(2, &[0.0; 6], &ctrl, None).is_err());
        assert!(valid_spline(2, &[0.0, 0.25, 0.5, 1.0, 1.0, 1.0], &ctrl, None).is_err());
        assert!(valid_spline(2, &[0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0], &ctrl, None).is_err());
        for weights in [vec![1.0, 1.0], vec![1.0, 0.0, 1.0], vec![1.0, -1.0, 1.0], vec![1.0, f64::INFINITY, 1.0]] {
            assert!(valid_spline(2, &knots, &ctrl, Some(&weights)).is_err());
        }
        assert!(valid_spline(2, &knots, &[Vec2::ZERO, Vec2::new(f64::NAN, 0.0), Vec2::Y], None).is_err());
        assert!(valid_spline(2, &[0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0], &[Vec2::ZERO; 6], None).is_err());
    }

    #[test]
    fn mixed_payload_copy_and_work_caps_have_independent_inclusive_boundaries() {
        let mut file = DxfFile::new().unwrap();
        for _ in 0..2 {
            file.push(file.default_layer(), DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::X] }, None).unwrap();
        }
        // Two point arrays: 2*2*16=64 bytes, 2*(32+2*8)=96 work units.
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 64, 96).is_ok());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 63, 96).is_err());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 64, 95).is_err());
        file.push(file.default_layer(), DxfShape::Text { at: Vec2::ZERO, height: 1.0, angle_deg: 0.0, text: "é".into() }, None).unwrap();
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 66, 130).is_ok());
        assert!(file.translated_scaled_with_limits(Vec2::ZERO, 1.0, 65, 130).is_err());
        assert!(GeometryBudget::default().array::<Vec2>(usize::MAX, usize::MAX, usize::MAX).is_err());
        let mut budget = GeometryBudget { copy_bytes: usize::MAX, work: 0 };
        assert!(budget.charge(1, 0, usize::MAX, usize::MAX).is_err());
    }

    #[test]
    fn later_invalid_array_leaves_all_source_geometry_and_weights_intact() {
        let mut file = DxfFile::new().unwrap();
        let first = DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::X] };
        file.push(file.default_layer(), first.clone(), Some(EntityWeight::ByBlock)).unwrap();
        file.push(file.default_layer(), DxfShape::Spline { degree: 2, knots: Vec::new(), ctrl: vec![Vec2::ZERO; 3], weights: None }, None).unwrap();
        assert!(file.translated_scaled(Vec2::new(1.0, 2.0), 2.0).is_err());
        assert!(file.extents().is_err());
        assert_eq!(file.entities().len(), 2);
        assert_eq!(file.entities()[0].geometry(), &first);
        assert_eq!(file.entities()[0].weight(), Some(EntityWeight::ByBlock));
    }

    #[test]
    fn ellipse_support_underflow_overflow_and_invalid_ratio_are_explicit() {
        assert!(ellipse_bounds(Vec2::ZERO, Vec2::ZERO, 0.5, 0.0, 1.0).is_err());
        assert!(ellipse_bounds(Vec2::ZERO, Vec2::X, 0.0, 0.0, 1.0).is_err());
        assert!(ellipse_bounds(Vec2::ZERO, Vec2::X, 1.1, 0.0, 1.0).is_err());
        assert!(ellipse_bounds(Vec2::ZERO, Vec2::X, 0.5, f64::NAN, 1.0).is_err());
        assert!(ellipse_bounds(Vec2::ZERO, Vec2::new(f64::MAX, f64::MAX), 1.0, 0.0, 1.0).is_err());
        let bounds = ellipse_bounds(Vec2::ZERO, Vec2::new(f64::from_bits(1), 0.0), f64::from_bits(1), 0.0, 1.0).unwrap();
        assert!(bounds.min.x < 0.0 && bounds.max.x > 0.0 && bounds.min.y < 0.0 && bounds.max.y > 0.0);
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::new(f64::from_bits(1), 0.0), ratio: 0.5, t0: 0.0, t1: 1.0 },
            None,
        )
        .unwrap();
        assert!(file.translated_scaled(Vec2::ZERO, 0.5).is_err());
    }
    #[test]
    fn imported_nine_text_modes_keep_custom_zero_when_a_bulged_shape_is_added_and_copied() {
        use crate::{
            DxfEntityWeight as Weight, DxfLinetype as LineType, DxfShape as Shape, DxfStandardWeight as Standard, DxfTextHorizontalAlignment as H,
            DxfTextVerticalAlignment as V, DxfVertex as Vertex, read_dxf_file,
        };
        use solidcraft_geom::Vec2;

        let modes = [
            (0, 0, H::Left, V::Baseline),
            (1, 0, H::Centre, V::Baseline),
            (2, 0, H::Right, V::Baseline),
            (0, 2, H::Left, V::Middle),
            (1, 2, H::Centre, V::Middle),
            (2, 2, H::Right, V::Middle),
            (0, 3, H::Left, V::Top),
            (1, 3, H::Centre, V::Top),
            (2, 3, H::Right, V::Top),
        ];
        // Literal millimetre DXF, with the canonical HIDDEN definition and a
        // nondefault layer0. No generated writer output supplies these oracles.
        let mut input = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n",
            "0\nLTYPE\n2\nHIDDEN\n72\n65\n73\n2\n40\n3\n49\n2\n74\n0\n49\n-1\n74\n0\n0\nENDTAB\n",
            "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n62\n3\n6\nHIDDEN\n370\n50\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n",
        )
        .to_string();
        for (h, v, _, _) in modes {
            input.push_str(&format!("0\nTEXT\n8\n0\n370\n-1\n10\n99\n20\n88\n11\n2\n21\n3\n40\n4\n50\n90\n72\n{h}\n73\n{v}\n1\n  %%d  \n"));
        }
        input.push_str("0\nENDSEC\n0\nEOF\n");
        let mut source = read_dxf_file(&input).unwrap();
        assert_eq!(source.entities().len(), 9);
        let physical = Standard::from_code(50).unwrap();
        let old_zero = source.default_layer();
        let zero = source.layer(old_zero).unwrap();
        assert_eq!((zero.name(), zero.color(), zero.linetype(), zero.weight()), ("0", 3, LineType::Hidden, Some(physical)));
        source
            .push(
                old_zero,
                Shape::BulgedPolyline {
                    vertices: vec![
                        Vertex { at: Vec2::ZERO, bulge: 0.5 },
                        Vertex { at: Vec2::new(4.0, 0.0), bulge: -0.5 },
                        Vertex { at: Vec2::new(8.0, 0.0), bulge: -0.0 },
                    ],
                    closed: false,
                },
                Some(Weight::ByBlock),
            )
            .unwrap();
        let original: Vec<_> = source.entities().iter().map(|e| e.geometry().clone()).collect();
        assert_eq!(original.len(), 10);
        let copied = source.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
        let zero = copied.layer(copied.default_layer()).unwrap();
        assert_eq!((zero.name(), zero.color(), zero.linetype(), zero.weight()), ("0", 3, LineType::Hidden, Some(physical)));
        assert_eq!(copied.layers().len(), 1);
        assert_eq!(copied.entities().len(), 10);
        assert!(copied.layer(old_zero).is_err());
        assert!(source.layer(copied.default_layer()).is_err());
        for (entity, (h, v, horizontal, vertical)) in copied.entities().iter().take(9).zip(modes) {
            // Left/baseline selects group10; other modes select group11. The
            // reader's centre/baseline canonical Text representation is retained.
            let at = if h == 0 && v == 0 { Vec2::new(208.0, 171.0) } else { Vec2::new(14.0, 1.0) };
            let expected = if h == 1 && v == 0 {
                Shape::Text { at, height: 8.0, angle_deg: 90.0, text: "  °  ".into() }
            } else {
                Shape::AlignedText { at, height: 8.0, angle_deg: 90.0, text: "  °  ".into(), horizontal, vertical }
            };
            assert_eq!(entity.geometry(), &expected);
            assert_eq!(entity.layer(), copied.default_layer());
            assert_eq!(entity.weight(), Some(Weight::ByLayer));
        }
        let Shape::BulgedPolyline { vertices, closed } = copied.entities()[9].geometry() else { panic!("expected bulged geometry") };
        assert!(!*closed);
        assert_eq!(vertices.iter().map(|v| v.at).collect::<Vec<_>>(), [Vec2::new(10.0, -5.0), Vec2::new(18.0, -5.0), Vec2::new(26.0, -5.0)]);
        assert_eq!(vertices.iter().map(|v| v.bulge.to_bits()).collect::<Vec<_>>(), [0.5f64.to_bits(), (-0.5f64).to_bits(), (-0.0f64).to_bits()]);
        for (index, original_shape) in original.iter().enumerate() {
            assert_eq!(copied.effective_linetype(index).unwrap(), LineType::Hidden);
            assert_eq!(copied.effective_weight(index).unwrap(), if index == 9 { Weight::ByBlock } else { Weight::Explicit(physical) });
            assert_eq!(source.entities()[index].geometry(), original_shape);
            assert_eq!(source.entities()[index].layer(), old_zero);
            assert_eq!(source.entities()[index].weight(), Some(if index == 9 { Weight::ByBlock } else { Weight::ByLayer }));
        }
    }

    #[test]
    fn copied_mixed_bulges_and_empty_aligned_text_keep_zero_style_and_literal_writer_fields() {
        use crate::{
            DxfEntityWeight as Weight, DxfFile as File, DxfGeometryBounds as Bounds, DxfLinetype as LineType, DxfShape as Shape,
            DxfStandardWeight as Standard, DxfTextHorizontalAlignment as H, DxfTextVerticalAlignment as V, DxfVertex as Vertex, write_dxf_file,
            write_dxf_file_with_text_bounds,
        };
        use solidcraft_geom::Vec2;

        fn records<'a>(pairs: &'a [(i32, &'a str)], kind: &str) -> Vec<&'a [(i32, &'a str)]> {
            pairs
                .iter()
                .enumerate()
                .filter_map(|(index, (code, value))| {
                    if *code != 0 || *value != kind {
                        return None;
                    }
                    let tail = &pairs[index + 1..];
                    let end = tail.iter().position(|(code, _)| *code == 0).unwrap_or(tail.len());
                    Some(&tail[..end])
                })
                .collect()
        }
        fn values<'a>(record: &[(i32, &'a str)], code: i32) -> Vec<&'a str> {
            record.iter().filter_map(|(field, value)| (*field == code).then_some(*value)).collect()
        }

        let modes = [
            (H::Left, V::Baseline, "0", "0"),
            (H::Centre, V::Baseline, "1", "0"),
            (H::Right, V::Baseline, "2", "0"),
            (H::Left, V::Middle, "0", "2"),
            (H::Centre, V::Middle, "1", "2"),
            (H::Right, V::Middle, "2", "2"),
            (H::Left, V::Top, "0", "3"),
            (H::Centre, V::Top, "1", "3"),
            (H::Right, V::Top, "2", "3"),
        ];
        for weight_code in [None, Some(0), Some(50)] {
            let weight = weight_code.map(|code| Standard::from_code(code).unwrap());
            let mut source = File::with_layer_zero(3, LineType::Hidden, weight).unwrap();
            source
                .push(
                    source.default_layer(),
                    Shape::BulgedPolyline {
                        vertices: vec![Vertex { at: Vec2::new(-1.0, 0.0), bulge: 1.0 }, Vertex { at: Vec2::new(1.0, 0.0), bulge: 1.0 }],
                        closed: true,
                    },
                    Some(Weight::ByLayer),
                )
                .unwrap();
            for (horizontal, vertical, _, _) in modes {
                source
                    .push(
                        source.default_layer(),
                        Shape::AlignedText { at: Vec2::new(2.0, 3.0), height: 4.0, angle_deg: 90.0, text: String::new(), horizontal, vertical },
                        Some(Weight::ByLayer),
                    )
                    .unwrap();
            }
            let copied = source.translated_scaled(Vec2::new(10.0, -5.0), 2.0).unwrap();
            assert!(copied.extents().is_err());
            let mut bounds_calls = 0;
            let bounds = copied
                .extents_with_text_bounds(|shape| {
                    let Shape::AlignedText { at, text, .. } = shape else { panic!("text bounds called for geometry") };
                    assert_eq!(*at, Vec2::new(14.0, 1.0));
                    assert!(text.is_empty());
                    bounds_calls += 1;
                    Bounds::new(Vec2::new(14.0, 1.0), Vec2::new(14.0, 1.0))
                })
                .unwrap()
                .unwrap();
            assert_eq!(bounds_calls, 9);
            // Conservative interval bounds may extend past the circle; the mixed
            // union must include both its literal cardinal support and the TEXT.
            assert!(bounds.min.x <= 8.0 && bounds.min.y <= -7.0);
            assert!(bounds.max.x >= 14.0 && bounds.max.y >= 1.0);
            assert!(write_dxf_file(&copied).is_err());
            let mut calls = 0;
            let output = write_dxf_file_with_text_bounds(&copied, |shape| {
                let Shape::AlignedText { at, height, angle_deg, text, horizontal, vertical } = shape else { panic!("provider called for non-text") };
                let (h, v, _, _) = modes[calls];
                assert_eq!((*at, *height, *angle_deg, *horizontal, *vertical), (Vec2::new(14.0, 1.0), 8.0, 90.0, h, v));
                assert!(text.is_empty());
                calls += 1;
                // Empty text has no glyph ink; this is a fixture-only provider,
                // not an invented font bound for nonempty text.
                Bounds::new(Vec2::new(14.0, 1.0), Vec2::new(14.0, 1.0))
            })
            .unwrap();
            assert_eq!(calls, 9);
            let mut lines = output.lines();
            let mut pairs = Vec::new();
            while let Some(code) = lines.next() {
                pairs.push((code.trim().parse::<i32>().unwrap(), lines.next().unwrap()));
            }
            let layers = records(&pairs, "LAYER");
            assert_eq!(layers.len(), 1);
            assert_eq!(values(layers[0], 2), ["0"]);
            assert_eq!(values(layers[0], 62), ["3"]);
            assert_eq!(values(layers[0], 6), ["HIDDEN"]);
            let expected_weight = match weight_code {
                None => vec![],
                Some(0) => vec!["0"],
                Some(50) => vec!["50"],
                _ => panic!("unexpected fixture weight"),
            };
            assert_eq!(values(layers[0], 370), expected_weight);
            let bulges = records(&pairs, "LWPOLYLINE");
            assert_eq!(bulges.len(), 1);
            for (code, expected) in [
                (8, vec!["0"]),
                (370, vec!["-1"]),
                (90, vec!["2"]),
                (70, vec!["129"]),
                (10, vec!["8", "12"]),
                (20, vec!["-5", "-5"]),
                (42, vec!["1", "1"]),
            ] {
                assert_eq!(values(bulges[0], code), expected);
            }
            let texts = records(&pairs, "TEXT");
            assert_eq!(texts.len(), 9);
            for (text, (_, _, h, v)) in texts.iter().zip(modes) {
                for (code, expected) in [
                    (8, "0"),
                    (370, "-1"),
                    (10, "14.0"),
                    (20, "1.0"),
                    (11, "14.0"),
                    (21, "1.0"),
                    (40, "8.0"),
                    (50, "90.0"),
                    (72, h),
                    (73, v),
                    (1, ""),
                ] {
                    assert_eq!(values(text, code), [expected]);
                }
            }
            for (name, expected) in [("$EXTMIN", [(10, "8"), (20, "-7"), (30, "0.0")]), ("$EXTMAX", [(10, "14"), (20, "1"), (30, "0.0")])] {
                let index = pairs.iter().position(|pair| *pair == (9, name)).unwrap();
                assert_eq!(&pairs[index + 1..index + 4], &expected);
            }
            for (index, entity) in copied.entities().iter().enumerate() {
                assert_eq!(entity.layer(), copied.default_layer());
                assert_eq!(entity.weight(), Some(Weight::ByLayer));
                assert_eq!(copied.effective_weight(index).unwrap(), weight.map_or(Weight::Default, Weight::Explicit));
                assert_eq!(copied.effective_linetype(index).unwrap(), LineType::Hidden);
            }
            assert!(copied.layer(source.default_layer()).is_err());
            assert_eq!(source.layers().len(), 1);
            assert_eq!(source.entities().len(), 10);
        }
    }
}
