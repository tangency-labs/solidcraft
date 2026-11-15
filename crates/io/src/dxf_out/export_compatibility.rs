//! Explicit contracts for the two pre-existing DXF producers. These are not
//! looser limits on the generic owned-file writer. Each file retains its immutable
//! producer plan, and serialization charges the actual stored geometry to it.
//! The compatibility route deliberately retains legacy numeric/text admission;
//! the ordinary DxfFile constructor keeps the strict owned-file admission.

use super::{DxfFile, DxfLinetype, DxfShape, EntityView, LayerView, extend_extents, path, shape_work, write_drawing_views};
use crate::{IoError, Result};
use solidcore_record::Vec2;

#[derive(Clone, Copy, Debug)]
enum Producer {
    FlatPattern,
    CurrentView,
    CurrentViewArcs,
    #[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
    LegacyBaseline,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Budget {
    producer: Producer,
    pub(super) layers: usize,
    pub(super) entities: usize,
    values: usize,
    text_bytes: usize,
    bytes: usize,
}

fn invalid(message: &str) -> IoError {
    IoError::Invalid(format!("DXF producer compatibility: {message}"))
}

fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| invalid("producer plan count overflow"))
}

fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(|| invalid("producer plan count overflow"))
}

impl Budget {
    #[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
    pub(super) fn is_legacy_baseline(self) -> bool {
        matches!(self.producer, Producer::LegacyBaseline)
    }

    fn new(producer: Producer, layers: usize, entities: usize, values: usize, text_bytes: usize) -> Result<Self> {
        // A finite f64 in the retained fixed twelve-decimal spelling needs at
        // most 323 bytes. 2048 bytes per work value covers its coordinate tuple,
        // group codes and delimiters. Entity/layer allowances include handles,
        // subclasses, bounded generated names and TEXT's 250*7 encoded bytes.
        // This is an input-derived output ceiling, not a new 64MiB restriction
        // on producers that previously admitted larger output.
        let bytes = (values as u128)
            .checked_mul(2048)
            .and_then(|n| (entities as u128).checked_mul(4096).and_then(|v| n.checked_add(v)))
            .and_then(|n| (layers as u128).checked_mul(8192).and_then(|v| n.checked_add(v)))
            .and_then(|n| n.checked_add(16_384))
            .ok_or_else(|| invalid("producer byte-bound arithmetic overflow"))?;
        // String cannot represent a larger allocation on this target anyway.
        // Clamp only the conservative upper estimate to that intrinsic ceiling;
        // do not reject a valid 32-bit producer because its loose bound exceeds it.
        let addressable = isize::MAX as u128;
        let bytes = usize::try_from(bytes.min(addressable)).map_err(|_| invalid("producer byte bound is not addressable"))?;
        Ok(Self { producer, layers, entities, values, text_bytes, bytes })
    }

    pub(super) fn layer_name(self, name: &str, color: i16, linetype: DxfLinetype, weighted: bool) -> Result<()> {
        if weighted || name.len() > 700 {
            return Err(invalid("layer does not belong to the declared producer"));
        }
        #[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
        if self.is_legacy_baseline() {
            return if (1..=255).contains(&color) { Ok(()) } else { Err(invalid("legacy fixture layer color must be an index from 1 to 255")) };
        }
        if !name.is_ascii() {
            return Err(invalid("layer does not belong to the declared producer"));
        }
        let expected = match (self.producer, name) {
            (_, "0") => Some((7, DxfLinetype::Continuous)),
            (Producer::CurrentView | Producer::CurrentViewArcs, "VISIBLE") => Some((7, DxfLinetype::Continuous)),
            (Producer::CurrentView | Producer::CurrentViewArcs, "HIDDEN") => Some((8, DxfLinetype::Hidden)),
            (Producer::CurrentView | Producer::CurrentViewArcs, "SKETCH") => Some((5, DxfLinetype::Continuous)),
            (Producer::FlatPattern, "OUTLINE") => Some((7, DxfLinetype::Continuous)),
            (Producer::FlatPattern, "CUT") => Some((4, DxfLinetype::Continuous)),
            (Producer::FlatPattern, "TEXT") => Some((2, DxfLinetype::Continuous)),
            (Producer::FlatPattern, "ETCH") => Some((6, DxfLinetype::Continuous)),
            (Producer::FlatPattern, "BEND_UP") => Some((1, DxfLinetype::Dashed)),
            (Producer::FlatPattern, "BEND_DOWN") => Some((5, DxfLinetype::Dashed)),
            (Producer::FlatPattern, "BEND_NOTES") => Some((8, DxfLinetype::Continuous)),
            (Producer::FlatPattern, "PERFORATION") => Some((3, DxfLinetype::Continuous)),
            (Producer::FlatPattern, value) if pocket_name(value) => Some((3, DxfLinetype::Continuous)),
            _ => None,
        };
        if expected != Some((color, linetype)) {
            return Err(invalid("layer/style does not belong to the declared producer"));
        }
        Ok(())
    }

    fn shape(self, layer: &str, shape: &DxfShape) -> Result<()> {
        let admitted = match self.producer {
            Producer::CurrentView => matches!(shape, DxfShape::Line(..) | DxfShape::Polyline { .. }),
            Producer::CurrentViewArcs => {
                matches!(shape, DxfShape::Line(..) | DxfShape::Polyline { .. })
                    || (layer == "VISIBLE" && matches!(shape, DxfShape::Circle { .. } | DxfShape::Arc { .. }))
            }
            Producer::FlatPattern => match layer {
                "BEND_UP" | "BEND_DOWN" => matches!(shape, DxfShape::Line(..)),
                "BEND_NOTES" => matches!(shape, DxfShape::Text { .. }),
                "PERFORATION" => matches!(shape, DxfShape::Circle { .. }),
                _ => matches!(
                    shape,
                    DxfShape::Line(..)
                        | DxfShape::Polyline { .. }
                        | DxfShape::BulgedPolyline { .. }
                        | DxfShape::Circle { .. }
                        | DxfShape::Arc { .. }
                        | DxfShape::Ellipse { .. }
                        | DxfShape::Spline { .. }
                        | DxfShape::Text { .. }
                ),
            },
            #[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
            Producer::LegacyBaseline => matches!(
                shape,
                DxfShape::Point(_)
                    | DxfShape::Line(..)
                    | DxfShape::Polyline { .. }
                    | DxfShape::LwPolyline { .. }
                    | DxfShape::BulgedPolyline { .. }
                    | DxfShape::Circle { .. }
                    | DxfShape::Arc { .. }
                    | DxfShape::Ellipse { .. }
                    | DxfShape::Spline { .. }
                    | DxfShape::Text { .. }
            ),
        };
        if !admitted {
            return Err(invalid("geometry does not belong to the declared producer"));
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
pub(super) fn baseline_file() -> Result<DxfFile<DxfShape>> {
    // The original 53 include the whole-text non-BMP witness after 10,000 BMP
    // scalars. The adapter admits no larger text work and is not production API.
    let mut budget = Budget::new(Producer::LegacyBaseline, 1024, 1_000_000, 1_000_000, 40_004)?;
    budget.bytes = crate::MAX_DRAWING_BYTES;
    DxfFile::with_export_budget(budget)
}

fn pocket_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("POCKET_") else { return false };
    let rest = ["TOP_Z", "BOTTOM_Z", "INTERNAL_Z"].into_iter().find_map(|prefix| rest.strip_prefix(prefix));
    let Some(rest) = rest.and_then(|value| value.strip_suffix("_MM")) else { return false };
    let Some((low, high)) = rest.split_once("_TO_") else { return false };
    let number = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'-' | b'.'));
    number(low) && number(high)
}

impl DxfFile<DxfShape> {
    /// Compatibility contract for the existing flat-pattern producer after its
    /// source geometry is admitted. Counts name contours supplied to path_shapes,
    /// bend records and already generated perforation holes. Actual consumption
    /// is checked again by the one writer; a smaller forged plan cannot extend it.
    pub fn for_flat_pattern_export(contours: usize, bends: usize, perforations: usize) -> Result<Self> {
        let path_entities = mul(contours, path::MAX_PATH_ITEMS)?;
        let entities = add(add(path_entities, mul(bends, 2)?)?, perforations)?;
        // An exact path item has at most 129 sampled extent points (ELLIPSE),
        // or <= 40 SPLINE values; bulged path vertices also fit this ceiling.
        let values = add(add(mul(path_entities, 129)?, mul(bends, 6)?)?, mul(perforations, 2)?)?;
        let budget = Budget::new(Producer::FlatPattern, add(contours, 9)?, entities, values, mul(bends, 2048)?)?;
        Self::with_export_budget(budget)
    }

    /// Compatibility contract for the existing view producer's actual admitted
    /// projected lines and sketch paths. The shared upstream output counter
    /// already limits lines+paths to 1,000,000; connecting hidden lines reduces it.
    /// Sketch point counts may exceed the generic writer's 1,000,000-value cap.
    pub fn for_current_view_export(lines: usize, sketch_paths: usize, sketch_points: usize) -> Result<Self> {
        Self::current_view_export_plan(lines, sketch_paths, sketch_points, false)
    }

    /// The current view's optional recognized-circle/arc output, with unchanged source limits.
    /// Each source line can contribute at most one ARC's 65 legacy extent samples;
    /// recognition only reduces the source entity count. Generic owned limits are unchanged.
    pub fn for_current_view_arc_export(lines: usize, sketch_paths: usize, sketch_points: usize) -> Result<Self> {
        Self::current_view_export_plan(lines, sketch_paths, sketch_points, true)
    }

    fn current_view_export_plan(lines: usize, sketch_paths: usize, sketch_points: usize, arcs: bool) -> Result<Self> {
        let entities = add(lines, sketch_paths)?;
        if entities > 1_000_000 || sketch_points < mul(sketch_paths, 2)? {
            return Err(invalid("view plan does not match admitted output counts"));
        }
        let producer = if arcs { Producer::CurrentViewArcs } else { Producer::CurrentView };
        let values_per_line = if arcs { 65 } else { 2 };
        let budget = Budget::new(producer, 4, entities, add(mul(lines, values_per_line)?, sketch_points)?, 0)?;
        Self::with_export_budget(budget)
    }
}

pub(super) fn write(file: &DxfFile<DxfShape>, budget: Budget) -> Result<String> {
    if file.layers().len() > budget.layers || file.entities().len() > budget.entities {
        return Err(invalid("stored metadata exceeds the immutable producer plan"));
    }
    for layer in file.layers() {
        budget.layer_name(layer.name(), layer.color(), layer.linetype(), layer.weight().is_some())?;
    }
    let mut values = 0usize;
    let mut text_bytes = 0usize;
    let mut samples = Vec::new();
    let mut extents = None;
    for (index, entity) in file.entities().iter().enumerate() {
        let layer = file.layer(entity.layer())?;
        if entity.weight().is_some() {
            return Err(invalid("producer entities do not have weight overrides"));
        }
        budget.shape(layer.name(), entity.geometry())?;
        let count = shape_work(entity.geometry())?;
        values = add(values, count)?;
        if values > budget.values {
            return Err(invalid("stored geometry exceeds the immutable producer plan"));
        }
        if let DxfShape::Text { text, .. } = entity.geometry() {
            text_bytes = add(text_bytes, text.len())?;
            if text_bytes > budget.text_bytes {
                return Err(invalid("stored note text exceeds the immutable producer plan"));
            }
        }
        samples.clear();
        samples.try_reserve(count).map_err(|_| invalid("extent allocation failed"))?;
        entity.geometry().sample(&mut samples).map_err(|e| IoError::Invalid(format!("DXF entity {} on layer {:?}: {e}", index + 1, layer.name())))?;
        // Identical legacy sampled points/order and finite filtering. Storing
        // just the running min/max avoids the old global sample allocation.
        for point in samples.iter().copied().filter(|point| point.is_finite()) {
            extents = Some(extend_extents(extents, point));
        }
    }
    let mut layer_views = Vec::new();
    layer_views.try_reserve_exact(file.layers().len()).map_err(|_| invalid("layer-view allocation failed"))?;
    layer_views.extend(file.layers().iter().map(|l| LayerView { name: l.name(), color: l.color(), linetype: l.linetype(), weight: l.weight() }));
    if matches!(budget.producer, Producer::FlatPattern) {
        // Legacy flat files put 0 first, then role/depth names in lexical order.
        // Sort borrowed metadata once, without rewriting in-memory handles.
        layer_views.sort_by(|a, b| a.name.cmp(b.name));
    }
    let layers = layer_views.into_iter();
    let entities = file.entities().iter().map(|e| Ok(EntityView { layer: file.layer(e.layer())?.name(), shape: e.geometry(), weight: e.weight() }));
    write_drawing_views(layers, entities, Some(budget.bytes), extents.unwrap_or((Vec2::ZERO, Vec2::ZERO)), true, 4)
}
