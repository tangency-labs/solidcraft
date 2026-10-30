//! Conservative bounds and positive uniform transforms for owned DXF geometry.
//!
//! ARC bounds enclose its supporting circle; ELLIPSE uses an outward L1 box;
//! clamped continuous positive-weight SPLINE uses its control-point hull.
//! These bounds are conservative, not tight. TEXT requires explicit world-space
//! layout metrics rather than guessed glyph widths. Unsupported spline forms
//! return an error. Stored styles, parameters and array order are preserved.
//! Variable copy payload and geometry work are independently bounded before
//! array scans/copies using MAX_DRAWING_BYTES. Neither bound promises peak RAM,
//! final encoded file size, callback cost or exact floating-point equivalence.

use solidcraft_geom::Vec2;

use super::super::DxfShape;
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
        DxfShape::Text { at, height, angle_deg, .. } => {
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
    /// layout, including rotation and centre-baseline alignment. It owns metric
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
                DxfShape::Text { text, .. } => {
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
                DxfShape::Line(..) | DxfShape::Circle { .. } | DxfShape::Arc { .. } | DxfShape::Ellipse { .. } => {}
            }
        }
        Ok(())
    }

    fn translated_scaled_with_limits(&self, translation: Vec2, scale: f64, copy_limit: usize, work_limit: usize) -> Result<Self> {
        finite_point(translation)?;
        positive(scale)?;
        // Length-only preflight before validation scans, output or copying.
        self.preflight_geometry(copy_limit, work_limit)?;
        let mut output = Self::new()?;
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
}
