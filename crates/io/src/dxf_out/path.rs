//! Bounded conversion of borrowed planar paths to DXF geometry.
//!
//! Admission precedes recognition and fitting. A failed conversion returns no partial path;
//! piece errors always refer to the original input, including after arc splitting or fitting.

use solidcore_ops::path_export as geometry;
use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_geom::MAX_FIT_PIECES;
use solidcraft_geom::prelude::*;

use super::DxfShape;
use crate::{IoError, Result};

/// A borrowed sequence of planar pieces, optionally carrying the caller's closed-loop intent.
/// This is not a certificate of continuity or topology. Whole-circle recognition uses geom's
/// bounded angular rounding cells, not bitwise endpoint identity. Polyline joins require
/// equal stored endpoints and otherwise emit separate open runs without filling or snapping.
#[derive(Clone, Copy, Debug)]
pub enum DxfPath<'a> {
    Open(&'a [Seg2]),
    /// Permit whole-loop recognition and an explicit closing flag when demonstrated.
    /// This does not certify or repair the input topology; recognition has the policies above.
    Closed(&'a Loop2),
}

/// How a path's curves are represented in the DXF file.
pub use geometry::DxfPathStyle;

/// A polyline vertex. `bulge` describes the edge leaving `at`: `tan(sweep / 4)`.
/// The terminal vertex of an open polyline has zero bulge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DxfVertex {
    pub at: Vec2,
    pub bulge: f64,
}

// Separate per-path budgets: one entity may contain many vertices. These do not bound an
// entire drawing made from many calls. Check before allocating from or expanding input.
pub(super) const MAX_PATH_ITEMS: usize = 200_000;
const MAX_FITTED_PATH_PIECES: usize = MAX_FIT_PIECES * 8;

#[derive(Clone, Copy)]
struct Piece {
    index: usize,
    total: usize,
}

impl Piece {
    fn invalid(self, reason: impl std::fmt::Display) -> IoError {
        IoError::Invalid(format!("DXF path: piece {} of {}: {reason}", self.index + 1, self.total))
    }
}

fn invalid(reason: &str) -> IoError {
    IoError::Invalid(format!("DXF path: {reason}"))
}

fn refusal(error: geometry::PathRefusal) -> IoError {
    let reason = error.reason.replace("writer precision", "DXF writer precision").replace("angular representation", "DXF angular representation");
    match error.piece {
        Some((index, total)) => Piece { index, total }.invalid(reason),
        None => invalid(&reason),
    }
}
// Match Out::num exactly for admission; do not replace unrounded output controls.
const ENCODING: geometry::ScalarEncoding = geometry::ScalarEncoding { round_trip: legacy_number, min_ellipse_ratio: 1e-12, turn_snap_radians: 1e-12 };
fn legacy_number(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    format!("{value:.12}").parse::<f64>().ok().filter(|v| v.is_finite())
}
fn parts(path: DxfPath<'_>) -> (&[Seg2], bool) {
    match path {
        DxfPath::Open(segs) => (segs, false),
        DxfPath::Closed(lp) => (&lp.segs, true),
    }
}

/// Check original source geometry, style and budget before recognition or wire rounding.
pub fn admit_path(path: DxfPath<'_>, style: DxfPathStyle) -> Result<()> {
    geometry::admit_path(parts(path).0, style).map_err(refusal)
}

/// Convert a bounded path without dropping, reordering or modifying its pieces.
/// The kernel owns exact geometry; the existing bounded fitter is the one arc fitter.
pub fn path_shapes(path: DxfPath<'_>, style: DxfPathStyle) -> Result<Vec<DxfShape>> {
    admit_path(path, style)?;
    let (segs, closed) = parts(path);
    if let Some(shape) = geometry::recognized_path(segs, closed, style, ENCODING).map_err(refusal)? {
        return Ok(vec![shape_record(shape)]);
    }
    match style {
        DxfPathStyle::Arcs { tolerance_mm } if segs.iter().any(|s| !s.is_line_or_arc()) => fitted(segs, tolerance_mm),
        _ => geometry::path_shapes(segs, closed, style, ENCODING).map(|v| v.into_iter().map(shape_record).collect()).map_err(refusal),
    }
}
fn exact(seg: &Seg2, piece: Piece) -> Result<DxfShape> {
    geometry::exact_piece(seg, piece.index, piece.total, ENCODING).map(shape_record).map_err(refusal)
}
fn shape_record(shape: geometry::Shape) -> DxfShape {
    use geometry::PathShape as S;
    match shape {
        S::Line(a, b) => DxfShape::Line(a, b),
        S::Circle { c, r } => DxfShape::Circle { c, r },
        S::Arc { c, r, a0, a1 } => DxfShape::Arc { c, r, a0, a1 },
        S::Ellipse { c, major, ratio, t0, t1 } => DxfShape::Ellipse { c, major, ratio, t0, t1 },
        S::Spline { degree, knots, ctrl, weights } => DxfShape::Spline { degree, knots, ctrl, weights },
        S::BulgedPolyline { vertices, closed } => {
            DxfShape::BulgedPolyline { vertices: vertices.into_iter().map(|v| DxfVertex { at: v.at, bulge: v.bulge }).collect(), closed }
        }
    }
}

fn fitted(segs: &[Seg2], tolerance: f64) -> Result<Vec<DxfShape>> {
    let mut shapes = Vec::new();
    let mut count = 0usize;
    for (index, seg) in segs.iter().enumerate() {
        let piece = Piece { index, total: segs.len() };
        if seg.is_line_or_arc() {
            count_fit(&mut count, 1, piece)?;
            push_shape(&mut shapes, exact(seg, piece)?)?;
            continue;
        }
        let adjusted = seg.fit_arcs(tolerance).ok_or_else(|| piece.invalid("curve could not be fitted within the arc fitting budget"))?;
        if adjusted.is_empty() {
            return Err(piece.invalid("fitting produced no pieces"));
        }
        if adjusted.len() > MAX_FIT_PIECES {
            return Err(piece.invalid("per-piece arc fitting budget exceeded"));
        }
        count_fit(&mut count, adjusted.len(), piece)?;
        for fitted in adjusted {
            if !fitted.is_line_or_arc() {
                return Err(piece.invalid("arc fitting produced a non-line/arc piece"));
            }
            geometry::admit_piece(&fitted, piece.index, piece.total).map_err(refusal)?;
            push_shape(&mut shapes, exact(&fitted, piece)?)?;
        }
    }
    Ok(shapes)
}

fn count_fit(count: &mut usize, additional: usize, piece: Piece) -> Result<()> {
    let total = count.checked_add(additional).ok_or_else(|| piece.invalid("path arc fitting budget exceeded"))?;
    if total > MAX_FITTED_PATH_PIECES {
        return Err(piece.invalid("path arc fitting budget exceeded"));
    }
    *count = total;
    Ok(())
}

fn push_shape(shapes: &mut Vec<DxfShape>, shape: DxfShape) -> Result<()> {
    if shapes.len() >= MAX_PATH_ITEMS {
        return Err(invalid("output entity budget exceeded"));
    }
    shapes.try_reserve(1).map_err(|_| invalid("cannot allocate output entities"))?;
    shapes.push(shape);
    Ok(())
}
