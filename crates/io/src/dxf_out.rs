//! The flat pattern's DXF drawing, as AutoCAD 2000 (`AC1015`), the oldest release that has the
//! ELLIPSE and SPLINE entities.
//!
//! Curves stay exact: a circle is a CIRCLE, a circular arc an ARC, a whole ellipse one ELLIPSE
//! and a conic arc of weight below 1 an elliptical ELLIPSE arc; Béziers (TrueType glyph
//! outlines, spline pieces) and the other conics are SPLINEs of their own degree. The file holds
//! what the entities refer to and what release 2000 needs to read them: the version, code page,
//! handle seed, units and extents; the CLASSES section, naming the class-based objects; all nine
//! symbol tables, with the active viewport, the linetypes, layers (with their lineweight and plot
//! style), text style, the ACAD application id, the dimension style and the block records, each
//! naming its layout; the model and paper space blocks; handles, owners and the subclass markers
//! of each entity; the root dictionary with its group, layout and plot style name dictionaries,
//! the Normal plot style and the Model and Layout1 layouts (#16899: AutoCAD refuses a release
//! 2000 file without them). Layers can be dashed and a drawing can carry TEXT (for annotations).
//! Group codes follow Autodesk's public DXF reference.

use solidcraft_geom::prelude::*;
use std::f64::consts::TAU;
use std::fmt::Write as _;

use solidcore_record::{Loop2, MAX_BEZIER_DEGREE, Seg2, Vec2};

use crate::{IoError, Result};

mod export_compatibility;
mod export_profile;
pub use export_profile::{DXF_PROFILE_FORMAT, DXF_PROFILE_VERSION, DxfExportProfile, DxfRole, DxfRoleStyle, MAX_DXF_PROFILE_BYTES};
mod file_model;
pub use file_model::{
    DxfFile, DxfGeometryBounds, DxfSourceBlock, DxfTextRecordKind, DxfTextSource, Entity as DxfFileEntity, EntityWeight as DxfEntityWeight,
    Layer as DxfFileLayer, LayerId as DxfLayerId, StandardWeight as DxfStandardWeight,
};
mod length_decimal;
mod path;
mod text_encode;
use length_decimal::length_decimal;
use text_encode::{EncodedText, encode_text};

pub use path::{DxfPath, DxfPathStyle, DxfVertex, admit_path, path_shapes};

/// A layer's linetype, defined in the file's LTYPE table so a viewer needs nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DxfLinetype {
    #[default]
    Continuous,
    /// AutoCAD's DASHED: a dash of 12.7 mm and a gap of 6.35 mm.
    Dashed,
    /// Short 2 mm dashes and 1 mm gaps for hidden edges in millimetre drawings.
    Hidden,
}

impl DxfLinetype {
    fn name(self) -> &'static str {
        match self {
            DxfLinetype::Continuous => "Continuous",
            DxfLinetype::Dashed => "DASHED",
            DxfLinetype::Hidden => "HIDDEN",
        }
    }
}

/// A layer: its name, AutoCAD colour index (1 red, 3 green, 5 blue, 7 white or black) and linetype.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfLayer {
    pub name: String,
    pub color: i16,
    pub linetype: DxfLinetype,
}

impl DxfLayer {
    /// A continuous layer.
    pub fn new(name: impl Into<String>, color: i16) -> DxfLayer {
        DxfLayer { name: name.into(), color, linetype: DxfLinetype::Continuous }
    }

    /// The same layer with a dashed line.
    pub fn dashed(self) -> DxfLayer {
        DxfLayer { linetype: DxfLinetype::Dashed, ..self }
    }
}

/// The three horizontal TEXT modes in the nine-alignment subset.
/// This deliberately excludes DXF Aligned/Middle/Fit modes72=3/4/5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxfTextHorizontalAlignment {
    Left,
    Centre,
    Right,
}

impl DxfTextHorizontalAlignment {
    pub fn group72(self) -> i16 {
        match self {
            Self::Left => 0,
            Self::Centre => 1,
            Self::Right => 2,
        }
    }

    pub fn from_group72(code: i16) -> Result<Self> {
        match code {
            0 => Ok(Self::Left),
            1 => Ok(Self::Centre),
            2 => Ok(Self::Right),
            _ => Err(IoError::Invalid("not supported yet: DXF horizontal TEXT alignment outside left/centre/right".into())),
        }
    }
}

/// Baseline, middle and top only; DXF Bottom73=1 is outside this subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxfTextVerticalAlignment {
    Baseline,
    Middle,
    Top,
}

impl DxfTextVerticalAlignment {
    pub fn group73(self) -> i16 {
        match self {
            Self::Baseline => 0,
            Self::Middle => 2,
            Self::Top => 3,
        }
    }

    pub fn from_group73(code: i16) -> Result<Self> {
        match code {
            0 => Ok(Self::Baseline),
            2 => Ok(Self::Middle),
            3 => Ok(Self::Top),
            _ => Err(IoError::Invalid("not supported yet: DXF vertical TEXT alignment outside baseline/middle/top".into())),
        }
    }
}

/// MTEXT's attachment point 71: rows top, middle and bottom (1-3, 4-6, 7-9), each left, centre,
/// right. MTEXT has no baseline, so its bottom row stands for TEXT's baseline both ways.
fn mtext_attachment(horizontal: DxfTextHorizontalAlignment, vertical: DxfTextVerticalAlignment) -> i16 {
    let row = match vertical {
        DxfTextVerticalAlignment::Top => 0,
        DxfTextVerticalAlignment::Middle => 1,
        DxfTextVerticalAlignment::Baseline => 2,
    };
    row * 3 + horizontal.group72() + 1
}

/// The alignment an MTEXT attachment point stands for, or `None` outside 1-9.
pub(crate) fn alignment_from_mtext_attachment(code: f64) -> Option<(DxfTextHorizontalAlignment, DxfTextVerticalAlignment)> {
    use {DxfTextHorizontalAlignment as H, DxfTextVerticalAlignment as V};
    let index = (1..=9_u8).position(|n| f64::from(n) == code)?;
    let vertical = [V::Top, V::Middle, V::Baseline].get(index / 3).copied()?;
    let horizontal = [H::Left, H::Centre, H::Right].get(index % 3).copied()?;
    Some((horizontal, vertical))
}

/// The geometry of one entity (millimetres).
#[derive(Clone, Debug, PartialEq)]
pub enum DxfShape {
    /// Original INSERT fields. Block names are DXF symbols, not foreign file
    /// ownership handles. The SketchImport projection resolves them in this file.
    Insert {
        block: String,
        at: Vec2,
        scale: Vec2,
        angle_deg: f64,
        columns: usize,
        rows: usize,
        column_spacing: f64,
        row_spacing: f64,
    },
    /// Source spline data before the sketch consumer's fit/control projection.
    /// Unlike an owned clamped Spline, these records may carry fit points,
    /// unclamped knots or source flags. No knots or weights are synthesized.
    SourceEllipse {
        c: Vec2,
        major: Vec2,
        major_radius: f64,
        ratio: f64,
        t0: f64,
        t1: f64,
    },
    SourceSpline {
        degree: f64,
        flags: f64,
        knots: Vec<f64>,
        ctrl: Vec<Vec2>,
        fit: Vec<Vec2>,
        weights: Vec<f64>,
    },
    /// Original POLYLINE/LWPOLYLINE vertices, including unrestricted source
    /// bulges and duplicate points. Owned BulgedPolyline admission stays strict.
    SourcePolyline {
        vertices: Vec<DxfVertex>,
        closed: bool,
    },
    /// A source record intentionally omitted by the sketch projection. Retaining
    /// its actual kind keeps recursive work accounting faithful even when a
    /// block produces no geometry; geometry or raw tokens are not cached here.
    SourceOmitted {
        kind: String,
    },
    /// A planar POINT at this WCS location, with zero Z and default thickness/normal.
    Point(Vec2),
    Line(Vec2, Vec2),
    /// Connected straight segments, with continuous linetype generation through the vertices.
    Polyline {
        points: Vec<Vec2>,
    },
    /// A lightweight polyline (AutoCAD `LWPOLYLINE`) with bulges for arc segments.
    /// Each entry in `verts` is `(position, bulge)`, where bulge is `tan(sweep / 4)`.
    LwPolyline {
        verts: Vec<(Vec2, f64)>,
        closed: bool,
    },
    /// Lines and arcs with signed outgoing bulges. At most a half turn per edge.
    /// Open polylines have a terminal zero bulge; closed ones do not repeat the first vertex.
    BulgedPolyline {
        vertices: Vec<DxfVertex>,
        closed: bool,
    },
    Circle {
        c: Vec2,
        r: f64,
    },
    /// Counter-clockwise from `a0` to `a1` (degrees).
    Arc {
        c: Vec2,
        r: f64,
        a0: f64,
        a1: f64,
    },
    /// An ellipse or an arc of one, counter-clockwise from parameter `t0` to `t1` (radians):
    /// the point at `t` is `c + major·cos t + perp(major)·ratio·sin t`, `major` being the
    /// vector from the centre to the end of the major axis and `ratio` the minor radius over
    /// the major one (0 < ratio ≤ 1). A whole ellipse runs from 0 to 2π.
    Ellipse {
        c: Vec2,
        major: Vec2,
        ratio: f64,
        t0: f64,
        t1: f64,
    },
    /// A clamped B-spline of `degree` over `ctrl`, rational when `weights` are given.
    Spline {
        degree: u8,
        knots: Vec<f64>,
        ctrl: Vec<Vec2>,
        weights: Option<Vec<f64>>,
    },
    /// Text as a person typed it, centred on `at` (its baseline) and turned `angle_deg`
    /// counter-clockwise. One line of at most 256 characters is written as TEXT; a longer text,
    /// or one with line breaks, as MTEXT. `%` and `\` are literal; `°` is the degree sign.
    /// Characters beyond U+FFFF are refused by [`write_dxf_file`], never substituted with `?`.
    Text {
        at: Vec2,
        height: f64,
        angle_deg: f64,
        text: String,
    },
    /// TEXT with one of nine explicit alignments. `at` is the requested anchor:
    /// group10 for left/baseline, group11 otherwise. Both records carry `at`;
    /// readers resolve the effective anchor from72/73. Default OCS is planar.
    /// Encoding and Standard style are the same as Text, not a plain-text or
    /// font-layout change. Bounds require an explicit real layout callback.
    AlignedText {
        at: Vec2,
        height: f64,
        angle_deg: f64,
        text: String,
        horizontal: DxfTextHorizontalAlignment,
        vertical: DxfTextVerticalAlignment,
    },
}

/// An entity on a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfEntity {
    pub layer: String,
    pub shape: DxfShape,
}

/// The closed loop as one AutoCAD `LWPOLYLINE` entity, with circular arc segments represented
/// by their DXF bulge (tan of a quarter of the included angle). Free-form curves are sampled as lines.
pub fn loop_lwpolyline_scaled(lp: &Loop2, scale: f64) -> DxfShape {
    let mut verts: Vec<(Vec2, f64)> = Vec::new();
    for s in &lp.segs {
        match *s {
            Seg2::Line { a, .. } => {
                verts.push((a * scale, 0.0));
            }
            Seg2::Arc { center, radius, start, sweep } => {
                if (sweep.abs() - TAU).abs() < 1e-9 {
                    // Full circle: split into two 180° semicircles.
                    let b = if sweep >= 0.0 { 1.0 } else { -1.0 };
                    let p0 = (center + Vec2::from_angle(start) * radius) * scale;
                    let p1 = (center + Vec2::from_angle(start + std::f64::consts::PI) * radius) * scale;
                    verts.push((p0, b));
                    verts.push((p1, b));
                } else if sweep.abs() > std::f64::consts::PI + 1e-9 {
                    // Arc larger than 180°: split into two pieces.
                    let half = sweep * 0.5;
                    let b = (half / 4.0).tan();
                    let p0 = (center + Vec2::from_angle(start) * radius) * scale;
                    let p1 = (center + Vec2::from_angle(start + half) * radius) * scale;
                    verts.push((p0, b));
                    verts.push((p1, b));
                } else {
                    let b = (sweep / 4.0).tan();
                    let p0 = (center + Vec2::from_angle(start) * radius) * scale;
                    verts.push((p0, b));
                }
            }
            _ => {
                let poly = s.polyline(0.01);
                let count = poly.len().saturating_sub(1);
                for p in poly.iter().take(count) {
                    verts.push((*p * scale, 0.0));
                }
            }
        }
    }
    DxfShape::LwPolyline { verts, closed: true }
}

/// [`loop_lwpolyline_scaled`] at scale 1.0 (millimetres).
pub fn loop_lwpolyline(lp: &Loop2) -> DxfShape {
    loop_lwpolyline_scaled(lp, 1.0)
}

impl DxfShape {
    /// Points on the shape (or its control polygon) for the drawing's extents.
    fn sample(&self, out: &mut Vec<Vec2>) -> Result<()> {
        match self {
            DxfShape::Insert { .. }
            | DxfShape::SourceSpline { .. }
            | DxfShape::SourceEllipse { .. }
            | DxfShape::SourcePolyline { .. }
            | DxfShape::SourceOmitted { .. } => {
                return Err(IoError::Invalid("not supported yet: source DXF records require their SketchImport projection".into()));
            }
            DxfShape::AlignedText { .. } => return Err(IoError::Invalid("aligned DXF TEXT requires explicit layout bounds".into())),
            DxfShape::Point(point) => out.push(*point),
            DxfShape::Line(a, b) => out.extend([*a, *b]),
            DxfShape::Polyline { points } => out.extend(points.iter().copied()),
            DxfShape::LwPolyline { verts, closed } => {
                for (i, (p0, b)) in verts.iter().enumerate() {
                    out.push(*p0);
                    if b.abs() > 1e-12 {
                        let next_idx = if i + 1 < verts.len() {
                            Some(i + 1)
                        } else if *closed {
                            Some(0)
                        } else {
                            None
                        };
                        if let Some(next) = next_idx
                            && let Some((p1, _)) = verts.get(next)
                        {
                            let chord = *p1 - *p0;
                            let mid = (*p0 + *p1) * 0.5;
                            let arc_mid = mid + chord.perp() * (*b * 0.5);
                            if arc_mid.x.is_finite() && arc_mid.y.is_finite() {
                                out.push(arc_mid);
                            }
                        }
                    }
                }
            }
            DxfShape::BulgedPolyline { vertices, closed } => sample_bulged(vertices, *closed, out)?,
            DxfShape::Circle { c, r } => out.extend([*c - Vec2::new(*r, *r), *c + Vec2::new(*r, *r)]),
            DxfShape::Arc { c, r, a0, a1 } => {
                let sweep = (a1 - a0).rem_euclid(360.0);
                out.extend((0..=64).map(|i| *c + Vec2::from_angle((a0 + sweep * i as f64 / 64.0).to_radians()) * *r));
            }
            DxfShape::Ellipse { c, major, ratio, t0, t1 } => {
                let (minor, sweep) = (major.perp() * *ratio, if (t1 - t0 - TAU).abs() < 1e-12 { TAU } else { (t1 - t0).rem_euclid(TAU) });
                out.extend((0..=128).map(|i| {
                    let t = t0 + sweep * i as f64 / 128.0;
                    *c + *major * t.cos() + minor * t.sin()
                }));
            }
            // A spline lies within the hull of its control points.
            DxfShape::Spline { ctrl, .. } => out.extend(ctrl.iter().copied()),
            // Where a viewer puts the letters depends on its font: 0.8 heights for each, along the
            // longest line, and MTEXT's default spacing of 5/3 heights for each line below the first.
            DxfShape::Text { at, height, angle_deg, text } => {
                let (along, up) = (Vec2::from_angle(angle_deg.to_radians()), Vec2::from_angle(angle_deg.to_radians()).perp());
                let lines = text.lines().count().max(1);
                let longest = text.lines().map(|line| line.chars().count()).max().unwrap_or(0);
                let half = 0.4 * height * longest as f64;
                let below = up * (-5.0 / 3.0 * height * (lines - 1) as f64);
                out.extend([-half, half].map(|s| *at + along * s + below));
                out.extend([-half, half].map(|s| *at + along * s + up * *height));
            }
        }
        Ok(())
    }
}

// Field-for-field adapter for the existing sketch-export representation.
fn legacy_vertices(verts: &[(Vec2, f64)]) -> Result<Vec<DxfVertex>> {
    if !(2..=path::MAX_PATH_ITEMS).contains(&verts.len()) {
        return Err(IoError::Invalid("bulged polyline requires 2..=200000 vertices".into()));
    }
    let mut out = Vec::new();
    out.try_reserve_exact(verts.len()).map_err(|_| IoError::Invalid("DXF vertex allocation failed".into()))?;
    out.extend(verts.iter().map(|(at, bulge)| DxfVertex { at: *at, bulge: *bulge }));
    Ok(out)
}

// Shared admission for the new owned/copy route. Geometry bounds have a
// separate stricter interval domain and MUST NOT serve as total admission here.
pub(super) fn admit_bulged(vertices: &[DxfVertex], closed: bool) -> Result<()> {
    if !(2..=path::MAX_PATH_ITEMS).contains(&vertices.len()) {
        return Err(IoError::Invalid("bulged polyline requires 2..=200000 vertices".into()));
    }
    if vertices.iter().any(|v| !v.at.is_finite() || !v.bulge.is_finite() || v.bulge.abs() > 1.0) {
        return Err(IoError::Invalid("bulged polyline has a non-finite value or an arc beyond a half turn".into()));
    }
    if !closed && vertices.last().is_some_and(|v| v.bulge != 0.0) {
        return Err(IoError::Invalid("open bulged polyline must end with a zero bulge".into()));
    }
    if closed && vertices.first().zip(vertices.last()).is_some_and(|(first, last)| first.at == last.at) {
        return Err(IoError::Invalid("closed bulged polyline must not repeat its first vertex".into()));
    }
    let check = |from: DxfVertex, to: Vec2| -> Result<()> {
        if from.bulge != 0.0
            && solidcore_ops::path_export::bulge_arc(solidcore_ops::path_export::Vertex { at: from.at, bulge: from.bulge }, to).is_none()
        {
            return Err(IoError::Invalid("bulged polyline arc has collapsed or non-finite geometry".into()));
        }
        Ok(())
    };
    for (from, to) in vertices.iter().zip(vertices.iter().skip(1)) {
        check(*from, to.at)?;
    }
    if closed && let Some((last, first)) = vertices.last().zip(vertices.first()) {
        check(*last, first.at)?;
    }
    Ok(())
}

// Validate before sampling.
fn sample_bulged(vertices: &[DxfVertex], closed: bool, out: &mut Vec<Vec2>) -> Result<()> {
    if !(2..=path::MAX_PATH_ITEMS).contains(&vertices.len()) {
        return Err(IoError::Invalid("bulged polyline requires 2..=200000 vertices".into()));
    }
    if vertices.iter().any(|v| !v.at.is_finite() || !v.bulge.is_finite() || v.bulge.abs() > 1.0) {
        return Err(IoError::Invalid("bulged polyline has a non-finite value or an arc beyond a half turn".into()));
    }
    if !closed && vertices.last().is_some_and(|v| v.bulge != 0.0) {
        return Err(IoError::Invalid("open bulged polyline must end with a zero bulge".into()));
    }
    if closed && vertices.first().zip(vertices.last()).is_some_and(|(first, last)| first.at == last.at) {
        return Err(IoError::Invalid("closed bulged polyline must not repeat its first vertex".into()));
    }
    for (from, to) in vertices.iter().zip(vertices.iter().skip(1)) {
        sample_bulged_edge(*from, to.at, out)?;
    }
    if closed && let Some((from, to)) = vertices.last().zip(vertices.first()) {
        sample_bulged_edge(*from, to.at, out)?;
    }
    Ok(())
}

fn sample_bulged_edge(from: DxfVertex, to: Vec2, out: &mut Vec<Vec2>) -> Result<()> {
    let points = solidcore_ops::path_export::bulged_edge_points(solidcore_ops::path_export::Vertex { at: from.at, bulge: from.bulge }, to)
        .map_err(|error| IoError::Invalid(error.reason))?;
    out.try_reserve(points.len()).map_err(|_| IoError::Invalid("cannot allocate bulged polyline bounds".into()))?;
    out.extend(points);
    Ok(())
}

/// A fallible bounded sink for the owned-file path. Formatting remains exactly that of Out.
struct BoundedDxfString<'a> {
    text: &'a mut String,
    limit: usize,
    failure: Option<&'static str>,
}

impl std::fmt::Write for BoundedDxfString<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if self.failure.is_some() {
            return Err(std::fmt::Error);
        }
        if self.text.len().checked_add(value.len()).is_none_or(|n| n > self.limit) {
            self.failure = Some("DXF file output exceeds the drawing-byte limit");
            return Err(std::fmt::Error);
        }
        if self.text.try_reserve(value.len()).is_err() {
            self.failure = Some("DXF file output allocation failed");
            return Err(std::fmt::Error);
        }
        self.text.push_str(value);
        Ok(())
    }
}

/// The dimensionless DXF spelling: twelve decimals, trimmed, never "-0.0" (lengths: `length_decimal`).
fn number_value(value: f64) -> std::result::Result<String, &'static str> {
    let mut text = String::new();
    // A finite f64 in fixed notation needs at most 323 bytes, including sign.
    let mut sink = BoundedDxfString { text: &mut text, limit: 384, failure: None };
    if write!(sink, "{value:.12}").is_err() {
        return Err(sink.failure.unwrap_or("DXF number formatting failed"));
    }
    let end = text.trim_end_matches('0').len();
    text.truncate(end);
    if text.ends_with('.') {
        text.try_reserve(1).map_err(|_| "DXF number allocation failed")?;
        text.push('0');
    }
    if text == "-0.0" {
        text.clear();
        text.push_str("0.0");
    }
    Ok(text)
}

/// Group codes and handles, in file order.
struct Out {
    s: String,
    next: u64,
    limit: Option<usize>,
    failure: Option<std::borrow::Cow<'static, str>>,
}

impl Out {
    fn new(next: u64, limit: Option<usize>) -> Self {
        Self { s: String::new(), next, limit, failure: None }
    }
    fn check(&self) -> Result<()> {
        match &self.failure {
            Some(message) => Err(IoError::Invalid(message.to_string())),
            None => Ok(()),
        }
    }
    fn code(&mut self, c: i32, v: impl std::fmt::Display) {
        if self.failure.is_some() {
            return;
        }
        if let Some(limit) = self.limit {
            let mut sink = BoundedDxfString { text: &mut self.s, limit, failure: None };
            if write!(sink, "{c:>3}\n{v}\n").is_err() {
                self.failure = Some(sink.failure.unwrap_or("DXF file formatting failed").into());
            }
        } else {
            let _ = write!(self.s, "{c:>3}\n{v}\n");
        }
    }
    /// A dimensionless number (an angle, ratio, bulge, knot, weight or normal): up to twelve
    /// decimals, at least one. A non-finite number is refused, never written.
    fn num(&mut self, c: i32, v: f64) {
        if self.failure.is_some() {
            return;
        }
        if !v.is_finite() {
            self.failure = Some("DXF file contains a non-finite output number".into());
            return;
        }
        match number_value(v) {
            Ok(text) => self.code(c, text),
            Err(error) => self.failure = Some(error.into()),
        }
    }
    /// A length (a coordinate, radius, height, extent or dash): on the 1e-9 grid of the file's
    /// unit. A non-finite length, or one beyond ±9000000, is refused with its value.
    fn len(&mut self, c: i32, v: f64) {
        if self.failure.is_some() {
            return;
        }
        match length_decimal(v).and_then(|decimal| decimal.as_str().map(|text| self.code(c, text))) {
            Ok(()) => {}
            Err(error) => self.failure = Some(format!("{error} (the value is {v})").into()),
        }
    }
    /// A point in the plane at height 0: codes `c`, `c + 10`, `c + 20`.
    fn point(&mut self, c: i32, p: Vec2) {
        self.len(c, p.x);
        self.len(c + 10, p.y);
        self.len(c + 20, 0.0);
    }
    /// A 2D point: codes `c` and `c + 10`.
    fn point2(&mut self, c: i32, p: Vec2) {
        self.len(c, p.x);
        self.len(c + 10, p.y);
    }
    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next);
        if self.limit.is_some() {
            match self.next.checked_add(1) {
                Some(next) => self.next = next,
                None => {
                    self.failure.get_or_insert("DXF file handle space exhausted".into());
                }
            }
        } else {
            self.next += 1;
        }
        h
    }
    /// The start of a symbol table; returns its handle.
    fn table(&mut self, name: &str, entries: usize) -> String {
        let h = self.handle();
        self.code(0, "TABLE");
        self.code(2, name);
        self.code(5, &h);
        self.code(330, 0);
        self.code(100, "AcDbSymbolTable");
        self.code(70, entries);
        h
    }
    /// The start of a symbol table record.
    fn record(&mut self, kind: &str, subclass: &str, table: &str) {
        let h = self.handle();
        self.code(0, kind);
        self.code(5, h);
        self.code(330, table);
        self.code(100, "AcDbSymbolTableRecord");
        self.code(100, subclass);
    }
    /// An object's owner `owner`, which also has it among its reactors: what an object a
    /// dictionary holds carries.
    fn owned(&mut self, owner: &str) {
        self.code(102, "{ACAD_REACTORS");
        self.code(330, owner);
        self.code(102, "}");
        self.code(330, owner);
    }
    /// The LAYOUT object `l`: its plot settings (no plotter, ISO A3 landscape in millimetres at
    /// 1:1) and its layout settings. `extents` are the drawing's, which the model layout holds.
    fn layout(&mut self, l: &LayoutObject, extents: (Vec2, Vec2)) {
        // ISO 216 A3, landscape.
        const SHEET: Vec2 = Vec2::new(420.0, 297.0);
        self.code(0, "LAYOUT");
        self.code(5, l.handle);
        self.owned(l.owner);
        self.code(100, "AcDbPlotSettings");
        self.code(1, "");
        self.code(2, "none_device");
        self.code(4, "");
        self.code(6, "");
        for code in [40, 41, 42, 43] {
            self.len(code, 0.0);
        }
        self.len(44, SHEET.x);
        self.len(45, SHEET.y);
        for code in [46, 47, 48, 49, 140, 141] {
            self.len(code, 0.0);
        }
        self.num(142, 1.0);
        self.num(143, 1.0);
        // Plot layout flags: 16 standard scale, 32 plot styles, 128 print lineweights, 512 draw
        // viewports first, and 1024 for the model layout.
        self.code(70, 16 + 32 + 128 + 512 + if l.paper { 0 } else { 1024 });
        // Millimetres, no rotation, plot the layout, at the standard scale 1:1 (16).
        self.code(72, 1);
        self.code(73, 0);
        self.code(74, 5);
        self.code(7, "");
        self.code(75, 16);
        self.code(100, "AcDbLayout");
        self.code(1, l.name);
        // PSLTSCALE on.
        self.code(70, 1);
        self.code(71, l.tab);
        self.point2(10, Vec2::ZERO);
        self.point2(11, SHEET);
        self.point(12, Vec2::ZERO);
        let (lo, hi) = if l.paper { (Vec2::ZERO, Vec2::ZERO) } else { extents };
        self.point(14, lo);
        self.point(15, hi);
        self.len(146, 0.0);
        self.point(13, Vec2::ZERO);
        self.point(16, Vec2::X);
        self.point(17, Vec2::Y);
        self.code(76, 0);
        self.code(330, l.block_record);
    }
    /// A BLOCK and its ENDBLK, empty, owned by the block record `owner`.
    fn block(&mut self, owner: &str, name: &str, paper: bool) {
        for (kind, sub) in [("BLOCK", "AcDbBlockBegin"), ("ENDBLK", "AcDbBlockEnd")] {
            let h = self.handle();
            self.code(0, kind);
            self.code(5, h);
            self.code(330, owner);
            self.code(100, "AcDbEntity");
            if paper {
                self.code(67, 1);
            }
            self.code(8, "0");
            self.code(100, sub);
            if kind == "BLOCK" {
                self.code(2, name);
                self.code(70, 0);
                self.point(10, Vec2::ZERO);
                self.code(3, name);
                self.code(1, "");
            }
        }
    }
}

/// One LAYOUT object: its handle, the layout dictionary that holds it, its name and tab, the
/// block record it shows and whether that is paper space.
struct LayoutObject<'a> {
    handle: &'a str,
    owner: &'a str,
    name: &'a str,
    tab: i32,
    block_record: &'a str,
    paper: bool,
}

/// A DXF drawing (AutoCAD 2000, millimetres) of `entities`. `layers` are the layers they are on
/// (layer `0` is always there). A TEXT character beyond U+FFFF returns an error naming the
/// character, its position, the entity and its layer; no drawing is returned on failure.
/// Explicit AlignedText requires the owned writer with caller layout bounds.
/// Frozen legacy regression seam, enabled only by the explicit test feature.
/// Uses owned layers/entities and the same Out backend; no second serializer.
#[cfg(any(test, feature = "dxf-legacy-baseline-tests"))]
pub fn legacy_dxf_baseline(layers: &[DxfLayer], entities: &[DxfEntity]) -> Result<String> {
    if layers.len() > 1024 || entities.len() > 1_000_000 {
        return Err(IoError::Invalid("legacy DXF baseline metadata exceeds fixture budget".into()));
    }
    let mut values = 0usize;
    let mut text_bytes = 0usize;
    let mut file = export_compatibility::baseline_file()?;
    let mut declared = std::collections::BTreeMap::new();
    declared.insert("0".to_string(), file.default_layer());
    for layer in layers {
        if !declared.contains_key(&layer.name) {
            let id = file.declare_legacy_baseline_layer(&layer.name, layer.color, layer.linetype)?;
            declared.insert(layer.name.clone(), id);
        }
    }
    for (index, entity) in entities.iter().enumerate() {
        values = values
            .checked_add(
                shape_work(&entity.shape)
                    .map_err(|error| IoError::Invalid(format!("DXF entity {} on layer {:?}: {error}", index + 1, entity.layer)))?,
            )
            .filter(|count| *count <= MAX_DXF_FILE_VALUES)
            .ok_or_else(|| IoError::Invalid("legacy DXF baseline geometry exceeds fixture budget".into()))?;
        if let DxfShape::Text { text, .. } = &entity.shape {
            text_bytes = text_bytes
                .checked_add(text.len())
                .filter(|count| *count <= 40_004)
                .ok_or_else(|| IoError::Invalid("legacy DXF baseline text exceeds fixture byte budget".into()))?;
            if text.len() > 40_004 || text.chars().take(10_002).count() > 10_001 {
                return Err(IoError::Invalid("legacy DXF baseline text exceeds 10,001-scalar fixture budget".into()));
            }
        }
        let name = if entity.layer.trim().is_empty() { "0" } else { entity.layer.as_str() };
        let layer = match declared.get(name).copied() {
            Some(id) => id,
            None => {
                // Only Unicode-error fixtures use an undeclared leading layer;
                // they return no file. Successful baseline inputs declare theirs.
                let id = file.declare_legacy_baseline_layer(name, 7, DxfLinetype::Continuous)?;
                declared.insert(name.to_string(), id);
                id
            }
        };
        if let DxfShape::BulgedPolyline { vertices, closed } = &entity.shape {
            admit_bulged(vertices, *closed)
                .map_err(|error| IoError::Invalid(format!("DXF entity {} on layer {:?}: {error}", index + 1, entity.layer)))?;
        }
        file.push(layer, entity.shape.clone(), None)?;
    }
    write_dxf_file(&file)
}

/// The existing drawing API with explicit DXF units, using the owned writer's same backend.
pub fn write_dxf_drawing_units(layers: &[DxfLayer], entities: &[DxfEntity], insunits: i32) -> Result<String> {
    let mut extents = None;
    let mut samples = Vec::new();
    for (index, entity) in entities.iter().enumerate() {
        samples.clear();
        entity
            .shape
            .sample(&mut samples)
            .map_err(|error| IoError::Invalid(format!("DXF entity {} on layer {:?}: {error}", index + 1, entity.layer)))?;
        for point in samples.iter().copied().filter(|point| point.is_finite()) {
            extents = Some(extend_extents(extents, point));
        }
    }
    let mut layer_list = vec![DxfLayer::new("0", 7)];
    for layer in layers {
        if !layer_list.iter().any(|l| l.name == layer.name) {
            layer_list.push(layer.clone());
        }
    }
    let layers = layer_list.iter().map(|l| LayerView { name: &l.name, color: l.color, linetype: l.linetype, weight: None });
    let entities = entities.iter().map(|e| Ok(EntityView { layer: &e.layer, shape: &e.shape, weight: None, source_text: None }));
    write_drawing_views(layers, entities, None, extents.unwrap_or((Vec2::ZERO, Vec2::ZERO)), insunits)
}

// These limits bound the ordinary strict owned-file entry point. Explicit
// legacy producer profiles have separate input-derived checked ceilings.
// A unit is one sampled extent point or one additional SPLINE knot/weight.
const MAX_DXF_FILE_VALUES: usize = 1_000_000;

#[derive(Clone, Copy)]
struct LayerView<'a> {
    name: &'a str,
    color: i16,
    linetype: DxfLinetype,
    weight: Option<DxfStandardWeight>,
}

#[derive(Clone, Copy)]
struct EntityView<'a> {
    layer: &'a str,
    shape: &'a DxfShape,
    weight: Option<DxfEntityWeight>,
    source_text: Option<&'a DxfTextSource>,
}

/// Write owned geometry as AutoCAD 2000 using one group-code backend. Files from
/// DxfFile::new() keep strict finite/encoded-geometry admission,1,024 layers,
/// 1,000,000 entities/geometry values and 64 MiB output. Nothing relaxes those caps.
/// Explicit flat-pattern/current-view constructors instead retain their immutable
/// checked producer contracts and legacy accepted geometry and text. Every file
/// writes numbers by one rule, and none writes a non-finite number.
/// AlignedText needs write_dxf_file_with_text_bounds and actual caller metrics.
/// Extents remain the existing sampled/control-polygon approximation.
pub fn write_dxf_file(file: &DxfFile<DxfShape>) -> Result<String> {
    if let Some(budget) = file.compatibility {
        return export_compatibility::write(file, budget, None);
    }
    write_dxf_file_limited(file, crate::MAX_DRAWING_BYTES, MAX_DXF_FILE_VALUES)
}

/// Write a flat pattern's file as a DXF profile says: each role on the profile's layer, in its
/// colour and linetype, and the roles it turns off left out. The file is checked under the
/// flat-pattern contract first, as [`write_dxf_file`] checks it; the neutral profile writes the
/// same bytes. Any other file is refused.
pub fn write_dxf_file_as(file: &DxfFile<DxfShape>, profile: &DxfExportProfile) -> Result<String> {
    let Some(budget) = file.compatibility else {
        return Err(IoError::Invalid("a DXF profile styles the flat pattern's roles only".into()));
    };
    export_compatibility::write(file, budget, Some(profile))
}

/// Write owned geometry including explicit TEXT alignments. For AlignedText,
/// the caller supplies complete world-space font/layout bounds for the actual
/// anchor, height and rotation, including overhangs. Finite/order is checked;
/// this writer cannot certify font metrics. Legacy Text retains its old sampled
/// extents and bytes and does not call this provider. Errors return no file.
pub fn write_dxf_file_with_text_bounds<F>(file: &DxfFile<DxfShape>, text_bounds: F) -> Result<String>
where
    F: FnMut(&DxfShape) -> Result<DxfGeometryBounds>,
{
    if let Some(budget) = file.compatibility {
        return export_compatibility::write(file, budget, None);
    }
    write_dxf_file_limited_with_text_bounds(file, crate::MAX_DRAWING_BYTES, MAX_DXF_FILE_VALUES, text_bounds)
}

// Private tightening seam lets the resource boundary be checked with tiny original cases.
fn write_dxf_file_limited(file: &DxfFile<DxfShape>, bytes: usize, values: usize) -> Result<String> {
    write_dxf_file_limited_with_text_bounds(file, bytes, values, |_| Err(IoError::Invalid("aligned DXF TEXT requires explicit layout bounds".into())))
}

fn write_dxf_file_limited_with_text_bounds<F>(file: &DxfFile<DxfShape>, bytes: usize, values: usize, mut text_bounds: F) -> Result<String>
where
    F: FnMut(&DxfShape) -> Result<DxfGeometryBounds>,
{
    if file.is_sketch_import() {
        return Err(IoError::Invalid("not supported yet: writing SketchImport source records requires explicit owned conversion".into()));
    }
    let bytes = bytes.min(crate::MAX_DRAWING_BYTES);
    let values = values.min(MAX_DXF_FILE_VALUES);
    if file.layers().len() > 1_024 || file.entities().len() > 1_000_000 {
        return Err(IoError::Invalid("DXF file metadata count exceeds the supported limit".into()));
    }
    let mut total = 0usize;
    let mut samples = Vec::new();
    let mut extents = None;
    for (index, entity) in file.entities().iter().enumerate() {
        // Resolve the file-scoped handle even though normal model insertion already checks it.
        let layer = file.layer(entity.layer())?;
        let shape = entity.geometry();
        // Charge lengths before scanning any input-derived vector or sampling a curve.
        let count = shape_work(shape)?;
        total = total
            .checked_add(count)
            .filter(|n| *n <= values)
            .ok_or_else(|| IoError::Invalid("DXF file geometry payload exceeds the supported limit".into()))?;
        shape_preflight(shape)
            .and_then(|()| shape_emission_preflight(shape))
            .map_err(|error| IoError::Invalid(format!("DXF entity {} on layer {:?}: {error}", index + 1, layer.name())))?;
        samples.clear();
        samples.try_reserve(count).map_err(|_| IoError::Invalid("DXF extent allocation failed".into()))?;
        if matches!(shape, DxfShape::AlignedText { .. }) {
            let bounds = text_bounds(shape)
                .and_then(|bounds| DxfGeometryBounds::new(bounds.min, bounds.max))
                .map_err(|error| IoError::Invalid(format!("DXF aligned TEXT entity {} on layer {:?}: {error}", index + 1, layer.name())))?;
            samples.extend([bounds.min, bounds.max]);
        } else {
            shape.sample(&mut samples)?;
        }
        for point in &samples {
            if !point.is_finite() {
                return Err(IoError::Invalid(format!("DXF entity {} has non-finite derived extents", index + 1)));
            }
            extents = Some(extend_extents(extents, *point));
        }
    }
    let layers =
        file.layers().iter().map(|layer| LayerView { name: layer.name(), color: layer.color(), linetype: layer.linetype(), weight: layer.weight() });
    let entities = file.entities().iter().map(|entity| {
        Ok(EntityView {
            layer: file.layer(entity.layer())?.name(),
            shape: entity.geometry(),
            weight: entity.weight(),
            source_text: entity.source_text(),
        })
    });
    write_drawing_views(layers, entities, Some(bytes), extents.unwrap_or((Vec2::ZERO, Vec2::ZERO)), 4)
}

fn shape_preflight(shape: &DxfShape) -> Result<()> {
    let invalid = || IoError::Invalid("invalid or unsupported DXF geometry".into());
    let finite = match shape {
        DxfShape::Insert { .. }
        | DxfShape::SourceSpline { .. }
        | DxfShape::SourceEllipse { .. }
        | DxfShape::SourcePolyline { .. }
        | DxfShape::SourceOmitted { .. } => {
            return Err(IoError::Invalid("not supported yet: source DXF records require their SketchImport projection".into()));
        }
        DxfShape::BulgedPolyline { vertices, closed } => return admit_bulged(vertices, *closed),
        DxfShape::LwPolyline { verts, closed } => {
            return admit_bulged(&legacy_vertices(verts)?, *closed);
        }
        DxfShape::Point(point) => point.is_finite(),
        DxfShape::Line(a, b) => a.is_finite() && b.is_finite(),
        DxfShape::Polyline { points } => points.len() >= 2 && points.len() <= MAX_DXF_FILE_VALUES && points.iter().all(|p| p.is_finite()),
        DxfShape::Circle { c, r } => c.is_finite() && r.is_finite() && *r > 0.0,
        DxfShape::Arc { c, r, a0, a1 } => c.is_finite() && r.is_finite() && *r > 0.0 && solidcore_ops::export_geometry::arc_fields_finite(*a0, *a1),
        DxfShape::Ellipse { c, major, ratio, t0, t1 } => {
            c.is_finite()
                && solidcore_ops::export_geometry::axis_nonzero(*major)
                && ratio.is_finite()
                && *ratio > 0.0
                && *ratio <= 1.0
                && t0.is_finite()
                && t1.is_finite()
                && solidcore_ops::export_geometry::finite_span(*t0, *t1)
        }
        DxfShape::Spline { degree, knots, ctrl, weights } => {
            let degree = usize::from(*degree);
            if degree == 0
                || degree > MAX_BEZIER_DEGREE
                || ctrl.len() <= degree
                || ctrl.len() > MAX_DXF_FILE_VALUES
                || ctrl.len().checked_add(degree + 1) != Some(knots.len())
                || knots.len() > MAX_DXF_FILE_VALUES
                || weights.as_ref().is_some_and(|w| w.len() != ctrl.len())
            {
                return Err(invalid());
            }
            let first = knots.first().copied().ok_or_else(invalid)?;
            let last = knots.last().copied().ok_or_else(invalid)?;
            ctrl.iter().all(|p| p.is_finite())
                && knots.iter().all(|k| k.is_finite())
                && knots.windows(2).all(|pair| matches!(pair, [a, b] if a <= b))
                && first < last
                && knots.iter().take(degree + 1).all(|k| *k == first)
                && knots.iter().rev().take(degree + 1).all(|k| *k == last)
                && weights.as_ref().is_none_or(|w| w.iter().all(|v| v.is_finite() && *v > 0.0))
        }
        DxfShape::Text { at, height, angle_deg, text } | DxfShape::AlignedText { at, height, angle_deg, text, .. } => {
            text_encode::writable(text)?;
            at.is_finite() && height.is_finite() && *height > 0.0 && solidcore_ops::export_geometry::finite_degrees(*angle_deg)
        }
    };
    if !finite {
        return Err(invalid());
    }
    Ok(())
}

fn shape_work(shape: &DxfShape) -> Result<usize> {
    match shape {
        DxfShape::Insert { .. }
        | DxfShape::SourceSpline { .. }
        | DxfShape::SourceEllipse { .. }
        | DxfShape::SourcePolyline { .. }
        | DxfShape::SourceOmitted { .. } => {
            Err(IoError::Invalid("not supported yet: source DXF records require their SketchImport projection".into()))
        }
        DxfShape::BulgedPolyline { vertices, closed } => {
            let count = vertices.len();
            if !(2..=path::MAX_PATH_ITEMS).contains(&count) {
                return Err(IoError::Invalid("bulged polyline requires 2..=200000 vertices".into()));
            }
            let edges = if *closed { count } else { count - 1 };
            edges
                .checked_mul(6)
                .and_then(|samples| samples.checked_add(count))
                .ok_or_else(|| IoError::Invalid("bulged polyline payload size overflow".into()))
        }
        DxfShape::LwPolyline { verts, .. } => verts.len().checked_mul(2).ok_or_else(|| IoError::Invalid("DXF polyline payload size overflow".into())),
        DxfShape::Point(_) => Ok(1),
        DxfShape::Line(..) | DxfShape::Circle { .. } => Ok(2),
        DxfShape::Polyline { points } => Ok(points.len()),
        DxfShape::Arc { .. } => Ok(65),
        DxfShape::Ellipse { .. } => Ok(129),
        DxfShape::Text { .. } => Ok(4),
        DxfShape::AlignedText { .. } => Ok(2),
        DxfShape::Spline { knots, ctrl, weights, .. } => ctrl
            .len()
            .checked_add(knots.len())
            .and_then(|n| n.checked_add(weights.as_ref().map_or(0, Vec::len)))
            .ok_or_else(|| IoError::Invalid("DXF spline payload size overflow".into())),
    }
}

/// What a reader gets back from a length as `Out::len` writes it. Path admission uses it too, so
/// both certify exactly what is written.
fn written_length(value: f64) -> std::result::Result<f64, &'static str> {
    let decimal = length_decimal(value).map_err(|error| error.message())?;
    decimal.as_str().map_err(|error| error.message())?.parse::<f64>().map_err(|_| "DXF length cannot be read back")
}

/// What a reader gets back from a dimensionless number as `Out::num` writes it.
fn written_number(value: f64) -> std::result::Result<f64, &'static str> {
    number_value(value)?.parse::<f64>().ok().filter(|value| value.is_finite()).ok_or("DXF number cannot be read back")
}

fn emitted_length(value: f64) -> Result<f64> {
    written_length(value).map_err(|message| IoError::Invalid(format!("{message} (the value is {value})")))
}

fn emitted_number(value: f64) -> Result<f64> {
    written_number(value).map_err(|message| IoError::Invalid(message.into()))
}

fn emitted_point(point: Vec2) -> Result<Vec2> {
    Ok(Vec2::new(emitted_length(point.x)?, emitted_length(point.y)?))
}

fn collapse() -> IoError {
    IoError::Invalid("DXF geometry loses a required positive value or distinct point/knot span as written".into())
}

/// An LWPOLYLINE's vertices as written: distinct neighbours stay distinct, and a non-zero bulge is
/// not written as zero. `skipped` is the bulge magnitude at or below which the writer leaves the
/// bulge out.
fn vertex_emission(vertices: impl Iterator<Item = (Vec2, f64)>, closed: bool, skipped: f64) -> Result<()> {
    let mut first = None;
    let mut previous = None;
    for (point, bulge) in vertices {
        let emitted = emitted_point(point)?;
        if let Some((old, last)) = previous
            && old != point
            && last == emitted
        {
            return Err(collapse());
        }
        first.get_or_insert((point, emitted));
        previous = Some((point, emitted));
        if bulge != 0.0 && (bulge.abs() <= skipped || emitted_number(bulge)? == 0.0) {
            return Err(collapse());
        }
    }
    if closed
        && let (Some((a, encoded_a)), Some((b, encoded_b))) = (first, previous)
        && a != b
        && encoded_a == encoded_b
    {
        return Err(collapse());
    }
    Ok(())
}

// The geometry a file holds must survive its own number rule: lengths on the grid, dimensionless
// values at twelve decimals.
fn shape_emission_preflight(shape: &DxfShape) -> Result<()> {
    let positive = |written: f64| if written > 0.0 { Ok(()) } else { Err(collapse()) };
    match shape {
        DxfShape::Insert { .. }
        | DxfShape::SourceSpline { .. }
        | DxfShape::SourceEllipse { .. }
        | DxfShape::SourcePolyline { .. }
        | DxfShape::SourceOmitted { .. } => {
            return Err(IoError::Invalid("not supported yet: source DXF records require their SketchImport projection".into()));
        }
        DxfShape::BulgedPolyline { vertices, closed } => vertex_emission(vertices.iter().map(|v| (v.at, v.bulge)), *closed, 0.0)?,
        // The legacy polyline leaves out a bulge of 1e-12 or less.
        DxfShape::LwPolyline { verts, closed } => vertex_emission(verts.iter().copied(), *closed, 1e-12)?,
        DxfShape::Point(point) => {
            emitted_point(*point)?;
        }
        DxfShape::Line(a, b) => {
            if emitted_point(*a)? == emitted_point(*b)? {
                return Err(collapse());
            }
        }
        DxfShape::Polyline { points } => {
            let mut previous = None;
            let mut distinct = false;
            for point in points {
                let emitted = emitted_point(*point)?;
                if let Some((old, last)) = previous {
                    if old != *point && last == emitted {
                        return Err(collapse());
                    }
                    distinct |= last != emitted;
                }
                previous = Some((*point, emitted));
            }
            if !distinct {
                return Err(collapse());
            }
        }
        DxfShape::Circle { r, .. } => positive(emitted_length(*r)?)?,
        DxfShape::Arc { r, a0, a1, .. } => {
            positive(emitted_length(*r)?)?;
            if !solidcore_ops::export_geometry::distinct_degrees(emitted_number(*a0)?, emitted_number(*a1)?) {
                return Err(collapse());
            }
        }
        DxfShape::Ellipse { major, ratio, t0, t1, .. } => {
            positive(emitted_number(*ratio)?)?;
            if emitted_point(*major)? == Vec2::ZERO || emitted_number(*t0)? == emitted_number(*t1)? {
                return Err(collapse());
            }
        }
        DxfShape::Spline { knots, ctrl, weights, .. } => {
            let mut previous = None;
            for knot in knots {
                let emitted = emitted_number(*knot)?;
                if let Some((old, last)) = previous
                    && old < *knot
                    && last >= emitted
                {
                    return Err(collapse());
                }
                previous = Some((*knot, emitted));
            }
            let mut first = None;
            let mut distinct = false;
            for point in ctrl {
                let emitted = emitted_point(*point)?;
                distinct |= first.is_some_and(|first| first != emitted);
                first.get_or_insert(emitted);
            }
            if !distinct {
                return Err(collapse());
            }
            for weight in weights.iter().flatten() {
                positive(emitted_number(*weight)?)?;
            }
        }
        DxfShape::Text { height, .. } | DxfShape::AlignedText { height, .. } => positive(emitted_length(*height)?)?,
    }
    Ok(())
}

fn extend_extents(extents: Option<(Vec2, Vec2)>, point: Vec2) -> (Vec2, Vec2) {
    extents.map_or((point, point), |(lo, hi)| {
        let b = solidcore_ops::export_geometry::union_bounds(lo, hi, point, point);
        (b.min, b.max)
    })
}

/// TEXT's first insertion point: an imported TEXT keeps its own; anything else has one anchor.
fn first_text_anchor(source: Option<&DxfTextSource>, at: Vec2) -> Result<Vec2> {
    match source {
        Some(source) if source.kind() == DxfTextRecordKind::Text => {
            source.first_anchor_mm().ok_or_else(|| IoError::Invalid("not supported yet: source TEXT has no first insertion anchor".into()))
        }
        _ => Ok(at),
    }
}

/// Where and how a text entity sits, as its groups say it.
struct TextPlacement<'a> {
    source: Option<&'a DxfTextSource>,
    at: Vec2,
    height: f64,
    angle_deg: f64,
    horizontal: DxfTextHorizontalAlignment,
    vertical: DxfTextVerticalAlignment,
}

/// A text entity's groups after the common header: TEXT with its two anchors and alignment, or
/// MTEXT with its insertion point, attachment point, chunks and direction vector.
fn text_groups(o: &mut Out, encoded: Option<&EncodedText>, place: TextPlacement) -> Result<()> {
    let TextPlacement { source, at, height, angle_deg, horizontal, vertical } = place;
    match encoded {
        Some(EncodedText::Text(value)) => {
            o.code(100, "AcDbText");
            o.point(10, first_text_anchor(source, at)?);
            o.len(40, height);
            o.code(1, value);
            o.num(50, angle_deg);
            o.code(72, horizontal.group72());
            o.point(11, at);
            o.code(100, "AcDbText");
            o.code(73, vertical.group73());
        }
        Some(EncodedText::MText(groups)) => {
            o.code(100, "AcDbMText");
            o.point(10, at);
            o.len(40, height);
            // No wrapping width; left to right.
            o.num(41, 0.0);
            o.code(71, mtext_attachment(horizontal, vertical));
            o.code(72, 1);
            for (code, value) in groups {
                o.code(*code, value);
            }
            let direction = Vec2::from_angle(angle_deg.to_radians());
            o.num(11, direction.x);
            o.num(21, direction.y);
            o.num(31, 0.0);
        }
        None => return Err(IoError::Invalid("DXF text entity was not encoded".into())),
    }
    Ok(())
}

fn write_drawing_views<'a, L, E>(layers: L, entities: E, limit: Option<usize>, extents: (Vec2, Vec2), insunits: i32) -> Result<String>
where
    L: Iterator<Item = LayerView<'a>> + Clone + ExactSizeIterator,
    E: Iterator<Item = Result<EntityView<'a>>> + Clone,
{
    let (lo, hi) = extents;

    // Everything but the header, which needs the handle seed.
    let mut o = Out::new(0x20, limit);
    let (model, paper) = (o.handle(), o.handle());
    let root = o.handle();
    // The objects the tables point at: the layout dictionary and the layout of each space, the
    // plot style name dictionary and its Normal plot style, which every layer names.
    let (layouts, model_layout, paper_layout) = (o.handle(), o.handle(), o.handle());
    let (plot_styles, normal) = (o.handle(), o.handle());

    // Release 2000 readers expect the CLASSES section between HEADER and TABLES; it names the
    // object types the OBJECTS section holds that are not built in: record name, C++ class,
    // application, proxy flags, was-a-proxy, is-an-entity.
    o.code(0, "SECTION");
    o.code(2, "CLASSES");
    for (record, class) in [("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault"), ("ACDBPLACEHOLDER", "AcDbPlaceHolder"), ("LAYOUT", "AcDbLayout")] {
        o.code(0, "CLASS");
        o.code(1, record);
        o.code(2, class);
        o.code(3, "ObjectDBX Classes");
        o.code(90, 0);
        o.code(280, 0);
        o.code(281, 0);
    }
    o.code(0, "ENDSEC");

    // All nine symbol tables, in the reference's order, even those with nothing to hold: a
    // strict reader (AutoCAD among them, #9144) refuses a file that leaves one out.
    o.code(0, "SECTION");
    o.code(2, "TABLES");
    // The active viewport, looking down on the whole drawing so it opens zoomed to fit.
    let t = o.table("VPORT", 1);
    o.record("VPORT", "AcDbViewportTableRecord", &t);
    o.code(2, "*Active");
    o.code(70, 0);
    o.point2(10, Vec2::ZERO);
    o.point2(11, Vec2::new(1.0, 1.0));
    o.point2(12, (lo + hi) * 0.5);
    o.point2(13, Vec2::ZERO);
    o.point2(14, Vec2::new(10.0, 10.0));
    o.point2(15, Vec2::new(10.0, 10.0));
    o.num(16, 0.0);
    o.num(26, 0.0);
    o.num(36, 1.0);
    o.point(17, Vec2::ZERO);
    const ASPECT: f64 = 1.5;
    let size = hi - lo;
    let height = (size.y.max(size.x / ASPECT) * 1.1).max(1.0);
    o.len(40, if height.is_finite() { height } else { 1.0 });
    o.num(41, ASPECT);
    o.num(42, 50.0);
    o.num(43, 0.0);
    o.num(44, 0.0);
    o.num(50, 0.0);
    o.num(51, 0.0);
    for (code, value) in [(71, 0), (72, 1000), (73, 1), (74, 3), (75, 0), (76, 0), (77, 0), (78, 0)] {
        o.code(code, value);
    }
    o.code(0, "ENDTAB");
    // The three linetypes every drawing has, and DASHED, which the dashed layers name: dash and
    // gap lengths (a gap is negative) after the pattern's total.
    let mut linetypes: Vec<(&str, &str, &[f64])> = vec![
        ("ByBlock", "", &[]),
        ("ByLayer", "", &[]),
        ("Continuous", "Solid line", &[]),
        ("DASHED", "Dashed __ __ __ __ __ __ __ __ __ __", &[12.7, -6.35]),
    ];
    if layers.clone().any(|l| l.linetype == DxfLinetype::Hidden) {
        linetypes.push(("HIDDEN", "Hidden edges", &[2.0, -1.0]));
    }
    let t = o.table("LTYPE", linetypes.len());
    for (name, description, pattern) in linetypes {
        o.record("LTYPE", "AcDbLinetypeTableRecord", &t);
        o.code(2, name);
        o.code(70, 0);
        o.code(3, description);
        o.code(72, 65);
        o.code(73, pattern.len());
        o.len(40, pattern.iter().map(|d| d.abs()).sum());
        for d in pattern {
            o.len(49, *d);
            o.code(74, 0);
        }
    }
    o.code(0, "ENDTAB");
    let t = o.table("LAYER", layers.len());
    for l in layers {
        o.record("LAYER", "AcDbLayerTableRecord", &t);
        o.code(2, l.name);
        o.code(70, 0);
        o.code(62, l.color);
        o.code(6, l.linetype.name());
        // R2000 requires a lineweight and plot style on every layer. Preserve
        // explicit physical weights; an unset weight uses the default (-3).
        o.code(370, l.weight.map_or(-3_i32, |weight| i32::from(weight.code())));
        o.code(390, &normal);
    }
    o.code(0, "ENDTAB");
    // The one text style, which a TEXT entity uses when it names none.
    let t = o.table("STYLE", 1);
    o.record("STYLE", "AcDbTextStyleTableRecord", &t);
    o.code(2, "Standard");
    o.code(70, 0);
    o.len(40, 0.0);
    o.num(41, 1.0);
    o.num(50, 0.0);
    o.code(71, 0);
    o.len(42, 2.5);
    o.code(3, "txt");
    o.code(4, "");
    o.code(0, "ENDTAB");
    for name in ["VIEW", "UCS"] {
        o.table(name, 0);
        o.code(0, "ENDTAB");
    }
    // The application id every drawing registers.
    let t = o.table("APPID", 1);
    o.record("APPID", "AcDbRegAppTableRecord", &t);
    o.code(2, "ACAD");
    o.code(70, 0);
    o.code(0, "ENDTAB");
    // The one dimension style, with the default settings. The table has a subclass of its own
    // and its records keep their handle under 105, not 5.
    let t = o.table("DIMSTYLE", 1);
    o.code(100, "AcDbDimStyleTable");
    let h = o.handle();
    o.code(0, "DIMSTYLE");
    o.code(105, h);
    o.code(330, &t);
    o.code(100, "AcDbSymbolTableRecord");
    o.code(100, "AcDbDimStyleTableRecord");
    o.code(2, "Standard");
    o.code(70, 0);
    o.code(0, "ENDTAB");
    let t = o.table("BLOCK_RECORD", 2);
    for (h, name, layout) in [(&model, "*Model_Space", &model_layout), (&paper, "*Paper_Space", &paper_layout)] {
        o.code(0, "BLOCK_RECORD");
        o.code(5, h);
        o.code(330, &t);
        o.code(100, "AcDbSymbolTableRecord");
        o.code(100, "AcDbBlockTableRecord");
        o.code(2, name);
        o.code(340, layout);
    }
    o.code(0, "ENDTAB");
    o.code(0, "ENDSEC");

    o.code(0, "SECTION");
    o.code(2, "BLOCKS");
    o.block(&model, "*Model_Space", false);
    o.block(&paper, "*Paper_Space", true);
    o.code(0, "ENDSEC");

    o.code(0, "SECTION");
    o.code(2, "ENTITIES");
    for (index, e) in entities.enumerate() {
        let e = e?;
        let encoded = match e.shape {
            DxfShape::Text { text, .. } | DxfShape::AlignedText { text, .. } => Some(
                encode_text(text, crate::MAX_DRAWING_BYTES)
                    .map_err(|error| IoError::Invalid(format!("DXF TEXT entity {} on layer {:?}: {error}", index + 1, e.layer)))?,
            ),
            _ => None,
        };
        let kind = match e.shape {
            DxfShape::Insert { .. }
            | DxfShape::SourceSpline { .. }
            | DxfShape::SourceEllipse { .. }
            | DxfShape::SourcePolyline { .. }
            | DxfShape::SourceOmitted { .. } => {
                return Err(IoError::Invalid("not supported yet: source DXF records require their SketchImport projection".into()));
            }
            DxfShape::Point(_) => "POINT",
            DxfShape::Line(..) => "LINE",
            DxfShape::Polyline { .. } | DxfShape::LwPolyline { .. } | DxfShape::BulgedPolyline { .. } => "LWPOLYLINE",
            DxfShape::Circle { .. } => "CIRCLE",
            DxfShape::Arc { .. } => "ARC",
            DxfShape::Ellipse { .. } => "ELLIPSE",
            DxfShape::Spline { .. } => "SPLINE",
            DxfShape::Text { .. } | DxfShape::AlignedText { .. } => match encoded {
                Some(EncodedText::MText(_)) => "MTEXT",
                _ => "TEXT",
            },
        };
        let h = o.handle();
        o.code(0, kind);
        o.code(5, h);
        o.code(330, &model);
        o.code(100, "AcDbEntity");
        o.code(8, if e.layer.trim().is_empty() { "0" } else { e.layer });
        if let Some(weight) = e.weight {
            o.code(370, weight.group370());
        }
        match e.shape {
            DxfShape::LwPolyline { verts, closed } => {
                o.code(100, "AcDbPolyline");
                o.code(90, verts.len());
                // Autodesk DXF reference: bit 1 closed, bit 128 (PLINEGEN) continues dash pattern.
                o.code(70, 128 + if *closed { 1 } else { 0 });
                for (p, b) in verts {
                    o.point2(10, *p);
                    if b.abs() > 1e-12 {
                        o.num(42, *b);
                    }
                }
            }
            DxfShape::Insert { .. }
            | DxfShape::SourceSpline { .. }
            | DxfShape::SourceEllipse { .. }
            | DxfShape::SourcePolyline { .. }
            | DxfShape::SourceOmitted { .. } => {
                return Err(IoError::Invalid("not supported yet: source DXF records require their SketchImport projection".into()));
            }
            DxfShape::BulgedPolyline { vertices, closed } => {
                o.code(100, "AcDbPolyline");
                o.code(90, vertices.len());
                o.code(70, 128 + i32::from(*closed));
                for vertex in vertices {
                    o.point2(10, vertex.at);
                    o.num(42, vertex.bulge);
                }
            }
            // The owned route validates payload/geometry and receives real bounds before reaching
            // Out. The legacy route refuses AlignedText in sample(), before writing any drawing.
            DxfShape::AlignedText { at, height, angle_deg, horizontal, vertical, .. } => {
                let place = TextPlacement {
                    source: e.source_text,
                    at: *at,
                    height: *height,
                    angle_deg: *angle_deg,
                    horizontal: *horizontal,
                    vertical: *vertical,
                };
                text_groups(&mut o, encoded.as_ref(), place)?;
            }
            DxfShape::Point(point) => {
                // POINT uses WCS10/20/30, not an OCS displacement. The planar
                // default normal/thickness need no extra optional fields.
                if !point.is_finite() {
                    return Err(IoError::Invalid(format!("DXF POINT entity {} on layer {:?}: non-finite location", index + 1, e.layer)));
                }
                o.code(100, "AcDbPoint");
                o.point(10, *point);
            }
            DxfShape::Polyline { points } => {
                o.code(100, "AcDbPolyline");
                o.code(90, points.len());
                // Autodesk DXF reference: bit 128 (PLINEGEN) continues the dash pattern.
                o.code(70, 128);
                for p in points {
                    o.point2(10, *p);
                }
            }
            DxfShape::Line(a, b) => {
                o.code(100, "AcDbLine");
                o.point(10, *a);
                o.point(11, *b);
            }
            DxfShape::Circle { c, r } => {
                o.code(100, "AcDbCircle");
                o.point(10, *c);
                o.len(40, *r);
            }
            DxfShape::Arc { c, r, a0, a1 } => {
                o.code(100, "AcDbCircle");
                o.point(10, *c);
                o.len(40, *r);
                o.code(100, "AcDbArc");
                o.num(50, *a0);
                o.num(51, *a1);
            }
            DxfShape::Ellipse { c, major, ratio, t0, t1 } => {
                o.code(100, "AcDbEllipse");
                o.point(10, *c);
                o.point(11, *major);
                o.num(210, 0.0);
                o.num(220, 0.0);
                o.num(230, 1.0);
                o.num(40, *ratio);
                o.num(41, *t0);
                o.num(42, *t1);
            }
            DxfShape::Spline { degree, knots, ctrl, weights } => {
                o.code(100, "AcDbSpline");
                o.num(210, 0.0);
                o.num(220, 0.0);
                o.num(230, 1.0);
                // Flags: 4 rational, 8 planar.
                o.code(70, 8 + if weights.is_some() { 4 } else { 0 });
                o.code(71, degree);
                o.code(72, knots.len());
                o.code(73, ctrl.len());
                o.code(74, 0);
                // Knots, then weights, then control points: the order of the reference.
                for k in knots {
                    o.num(40, *k);
                }
                for w in weights.iter().flatten() {
                    o.num(41, *w);
                }
                for p in ctrl {
                    o.point(10, *p);
                }
            }
            // Generated centred text uses coincident anchors. Imported text
            // keeps its real first source anchor and effective second anchor.
            DxfShape::Text { at, height, angle_deg, .. } => {
                let place = TextPlacement {
                    source: e.source_text,
                    at: *at,
                    height: *height,
                    angle_deg: *angle_deg,
                    horizontal: DxfTextHorizontalAlignment::Centre,
                    vertical: DxfTextVerticalAlignment::Baseline,
                };
                text_groups(&mut o, encoded.as_ref(), place)?;
            }
        }
        o.check().map_err(|error| IoError::Invalid(format!("DXF {kind} entity {} on layer {:?}: {error}", index + 1, e.layer)))?;
    }
    o.code(0, "ENDSEC");

    o.code(0, "SECTION");
    o.code(2, "OBJECTS");
    // The root dictionary, which must name the group dictionary (empty: no groups), the layout
    // dictionary and the plot style name dictionary.
    let groups = o.handle();
    o.code(0, "DICTIONARY");
    o.code(5, root.as_str());
    o.code(330, 0);
    o.code(100, "AcDbDictionary");
    o.code(281, 1);
    for (name, h) in [("ACAD_GROUP", &groups), ("ACAD_LAYOUT", &layouts), ("ACAD_PLOTSTYLENAME", &plot_styles)] {
        o.code(3, name);
        o.code(350, h);
    }
    o.code(0, "DICTIONARY");
    o.code(5, &groups);
    o.owned(&root);
    o.code(100, "AcDbDictionary");
    o.code(281, 1);
    o.code(0, "DICTIONARY");
    o.code(5, &layouts);
    o.owned(&root);
    o.code(100, "AcDbDictionary");
    o.code(281, 1);
    for (name, h) in [("Layout1", &paper_layout), ("Model", &model_layout)] {
        o.code(3, name);
        o.code(350, h);
    }
    // The plot style names, with Normal the default.
    o.code(0, "ACDBDICTIONARYWDFLT");
    o.code(5, &plot_styles);
    o.owned(&root);
    o.code(100, "AcDbDictionary");
    o.code(281, 1);
    o.code(3, "Normal");
    o.code(350, &normal);
    o.code(100, "AcDbDictionaryWithDefault");
    o.code(340, &normal);
    o.code(0, "ACDBPLACEHOLDER");
    o.code(5, &normal);
    o.owned(&plot_styles);
    for l in [
        LayoutObject { handle: &model_layout, owner: &layouts, name: "Model", tab: 0, block_record: &model, paper: false },
        LayoutObject { handle: &paper_layout, owner: &layouts, name: "Layout1", tab: 1, block_record: &paper, paper: true },
    ] {
        o.layout(&l, (lo, hi));
    }
    o.code(0, "ENDSEC");
    o.code(0, "EOF");

    o.check()?;
    let mut head = Out::new(0, limit);
    head.code(0, "SECTION");
    head.code(2, "HEADER");
    head.code(9, "$ACADVER");
    head.code(1, "AC1015");
    // Every character in the file is ASCII (other letters are `\U+XXXX`), which this code page
    // reads as itself.
    head.code(9, "$DWGCODEPAGE");
    head.code(3, "ANSI_1252");
    head.code(9, "$HANDSEED");
    head.code(5, format!("{:X}", o.next));
    head.code(9, "$INSUNITS");
    head.code(70, insunits);
    head.code(9, "$MEASUREMENT");
    head.code(70, if insunits == 1 || insunits == 2 { 0 } else { 1 });
    head.code(9, "$EXTMIN");
    head.point(10, lo);
    head.code(9, "$EXTMAX");
    head.point(10, hi);
    head.code(0, "ENDSEC");
    head.check()?;
    if let Some(limit) = limit {
        if head.s.len().checked_add(o.s.len()).is_none_or(|n| n > limit) {
            return Err(IoError::Invalid("DXF file output exceeds the drawing-byte limit".into()));
        }
        head.s.try_reserve_exact(o.s.len()).map_err(|_| IoError::Invalid("DXF file output allocation failed".into()))?;
        head.s.push_str(&o.s);
        Ok(head.s)
    } else {
        Ok(head.s + &o.s)
    }
}

#[cfg(test)]
fn bezier_spline(pts: &[Vec2], weights: Option<Vec<f64>>) -> DxfShape {
    let degree = pts.len().saturating_sub(1).max(1);
    let knots = std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect();
    DxfShape::Spline { degree: degree as u8, knots, ctrl: pts.to_vec(), weights }
}

#[cfg(test)]
mod tests {
    #[test]
    fn owned_lwpolyline_refuses_distinct_vertices_lost_at_wire_precision() {
        use super::*;
        for (verts, closed) in [
            (vec![(Vec2::ZERO, 0.0), (Vec2::new(1e-20, 0.0), 0.0)], false),
            (vec![(Vec2::ZERO, 0.0), (Vec2::new(1.0, 0.0), 0.0), (Vec2::new(1e-20, 0.0), 0.0)], true),
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), DxfShape::LwPolyline { verts, closed }, None).unwrap();
            assert!(write_dxf_file(&file).unwrap_err().to_string().contains("distinct point"));
        }
        let shape = DxfShape::LwPolyline { verts: vec![(Vec2::ZERO, 0.0), (Vec2::ZERO, 0.0), (Vec2::new(1.0, 0.0), 0.0)], closed: false };
        assert!(shape_emission_preflight(&shape).is_ok(), "intentional repeated vertices remain distinct from encoding collapse");
    }

    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

    use solidcore_record::{Loop2, Seg2};

    use super::*;
    use crate::dxf::{test_wire_entities as entities, test_wire_pairs as pairs, test_wire_table_records as table_records};
    use crate::{Geom2, read_dxf};

    // Keep the existing ordinary writer regressions on the new public conversion path.
    fn seg_shape(seg: &Seg2) -> DxfShape {
        let shapes = path_shapes(DxfPath::Open(std::slice::from_ref(seg)), DxfPathStyle::Exact).unwrap();
        assert_eq!(shapes.len(), 1);
        shapes.into_iter().next().unwrap()
    }

    fn loop_shapes(lp: &Loop2) -> Vec<DxfShape> {
        path_shapes(DxfPath::Closed(lp), DxfPathStyle::Exact).unwrap()
    }

    #[test]
    fn bulged_literal_groups_keep_signed_outgoing_values_and_closed_flag() {
        for (bulge, spelling) in [(1.0, "1.0"), (-1.0, "-1.0")] {
            let mut file = DxfFile::new().unwrap();
            file.push(
                file.default_layer(),
                DxfShape::BulgedPolyline {
                    vertices: vec![DxfVertex { at: Vec2::new(-1.0, 0.0), bulge }, DxfVertex { at: Vec2::new(1.0, 0.0), bulge: -0.0 }],
                    closed: false,
                },
                Some(DxfEntityWeight::ByLayer),
            )
            .unwrap();
            let output = write_dxf_file(&file).unwrap();
            let records = entities(&output);
            let record = records.first().unwrap();
            assert_eq!(record.0, "LWPOLYLINE");
            assert_eq!((field(&record.1, 90), field(&record.1, 70)), ("2", "128"));
            for (code, expected) in [
                (90, vec!["2"]),
                (70, vec!["128"]),
                (10, vec!["-1.0", "1.0"]),
                (20, vec!["0.0", "0.0"]),
                (42, vec![spelling, "0.0"]),
                (370, vec!["-1"]),
            ] {
                assert_eq!(record.1.iter().filter(|(field, _)| *field == code).map(|(_, value)| value.as_str()).collect::<Vec<_>>(), expected);
            }
        }
        let closed = DxfShape::BulgedPolyline {
            vertices: vec![DxfVertex { at: Vec2::new(-1.0, 0.0), bulge: 1.0 }, DxfVertex { at: Vec2::new(1.0, 0.0), bulge: 1.0 }],
            closed: true,
        };
        let output = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape: closed }], 4).unwrap();
        let records = entities(&output);
        assert_eq!((field(&records[0].1, 90), field(&records[0].1, 70)), ("2", "129"));
        assert_eq!(values(&records[0], 70), [129.0]);
        assert_eq!(values(&records[0], 90), [2.0]);
        assert_eq!(values(&records[0], 42), [1.0, 1.0]);
        let groups = pairs(&output);
        let min = groups.iter().position(|(code, value)| *code == 9 && value == "$EXTMIN").unwrap();
        let max = groups.iter().position(|(code, value)| *code == 9 && value == "$EXTMAX").unwrap();
        // X1 directed-cardinal sampling, not inflated supporting-circle boxes.
        assert_eq!(&groups[min + 1..min + 4], &[(10, "-1.0".into()), (20, "-1.0".into()), (30, "0.0".into())]);
        assert_eq!(&groups[max + 1..max + 4], &[(10, "1.0".into()), (20, "1.0".into()), (30, "0.0".into())]);
    }

    #[test]
    fn bulged_zero_line_is_written_and_a_tiny_arc_lost_on_the_grid_is_refused() {
        let mut file = DxfFile::new().unwrap();
        let repeated = vec![DxfVertex { at: Vec2::ZERO, bulge: 0.0 }; 2];
        file.push(file.default_layer(), DxfShape::BulgedPolyline { vertices: repeated, closed: false }, None).unwrap();
        let output = write_dxf_file(&file).unwrap();
        assert_eq!(field(&entities(&output)[0].1, 90), "2");
        assert_eq!(values(&entities(&output)[0], 10), [0.0, 0.0]);
        assert_eq!(values(&entities(&output)[0], 42), [0.0, 0.0]);
        // Two distinct vertices 1e-200 apart, and a bulge of 1e-200, are both written as zero.
        let tiny = vec![DxfVertex { at: Vec2::ZERO, bulge: 1e-200 }, DxfVertex { at: Vec2::new(1e-200, 0.0), bulge: 0.0 }];
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::BulgedPolyline { vertices: tiny, closed: false }, None).unwrap();
        assert!(write_dxf_file(&file).unwrap_err().to_string().contains("as written"));
        let invalid = DxfShape::BulgedPolyline {
            vertices: vec![DxfVertex { at: Vec2::ZERO, bulge: 0.0 }, DxfVertex { at: Vec2::X, bulge: 0.0 }, DxfVertex { at: Vec2::ZERO, bulge: 0.0 }],
            closed: true,
        };
        assert!(write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape: invalid }], 4).is_err());
    }

    #[test]
    fn bulged_writer_precharges_samples_before_hostile_vertex_scans() {
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
        // n+6*edges =2+6=8. No old twelve-decimal collapse admission applies.
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 8).is_ok());
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 7).is_err());
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
        assert!(write_dxf_file_limited(&hostile, crate::MAX_DRAWING_BYTES, 7).unwrap_err().to_string().contains("geometry payload"));
    }

    /// The conic quarter of the ellipse `c ± u`, `c ± v` from `c + u` to `c + v`.
    fn quarter(c: Vec2, u: Vec2, v: Vec2) -> Seg2 {
        Seg2::Conic { a: c + u, apex: c + u + v, b: c + v, w: FRAC_1_SQRT_2 }
    }

    #[test]
    fn bulged_polyline_fields_and_bounds_are_written_on_the_length_grid() {
        let shape = DxfShape::BulgedPolyline {
            vertices: vec![DxfVertex { at: Vec2::new(1e-14, -0.0), bulge: 0.0 }, DxfVertex { at: Vec2::new(2e-14, 1e-14), bulge: 0.0 }],
            closed: false,
        };
        let text = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).unwrap();
        let e = entities(&text);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].0, "LWPOLYLINE");
        assert_eq!(values(&e[0], 10), vec![0.0, 0.0]);
        assert_eq!(values(&e[0], 20), vec![0.0, 0.0]);
        assert!(!values(&e[0], 20)[0].is_sign_negative());
        assert_eq!(values(&e[0], 42), vec![0.0, 0.0]);
        let pairs = pairs(&text);
        let min = pairs.iter().position(|p| *p == (9, "$EXTMIN".into())).unwrap();
        let max = pairs.iter().position(|p| *p == (9, "$EXTMAX".into())).unwrap();
        assert_eq!(pairs[min + 1].1, "0.0");
        assert_eq!(pairs[max + 1].1, "0.0");
    }

    #[test]
    fn bulged_polyline_writer_rejects_invalid_vertices_before_serializing() {
        let first = DxfVertex { at: Vec2::ZERO, bulge: 0.0 };
        let last = DxfVertex { at: Vec2::X, bulge: 0.0 };
        let invalid = [
            (vec![], false),
            (vec![first], true),
            (vec![DxfVertex { at: Vec2::new(f64::NAN, 0.0), ..first }, last], false),
            (vec![DxfVertex { bulge: f64::INFINITY, ..first }, last], false),
            (vec![DxfVertex { bulge: 1.1, ..first }, last], false),
            (vec![first, DxfVertex { bulge: 0.5, ..last }], false),
            (vec![DxfVertex { bulge: 0.5, ..first }, first], false),
        ];
        for (vertices, closed) in invalid {
            let shape = DxfShape::BulgedPolyline { vertices, closed };
            let error = write_dxf_drawing_units(&[], &[DxfEntity { layer: "CUT".into(), shape }], 4).unwrap_err().to_string();
            assert!(error.contains("entity 1") && error.contains("CUT"), "{error}");
        }
    }

    #[test]
    fn owned_point_uses_real_wcs_record_and_retains_layer_and_each_optional_weight() {
        let mut file = DxfFile::new().unwrap();
        let physical = DxfStandardWeight::from_code(50).unwrap();
        let layer = file.declare_layer("DATUM", 4, DxfLinetype::Hidden, Some(physical)).unwrap();
        let weights = [
            None,
            Some(DxfEntityWeight::ByLayer),
            Some(DxfEntityWeight::ByBlock),
            Some(DxfEntityWeight::Default),
            Some(DxfEntityWeight::Explicit(physical)),
        ];
        for weight in weights {
            file.push(layer, DxfShape::Point(Vec2::new(1.25, -2.5)), weight).unwrap();
        }
        let dxf = write_dxf_file(&file).unwrap();
        let records = entities(&dxf);
        assert_eq!(records.len(), 5);
        for (record, weight) in records.iter().zip(weights) {
            assert_eq!(record.0, "POINT");
            assert_eq!(field(&record.1, 8), "DATUM");
            assert_eq!(values(record, 10), [1.25]);
            assert_eq!(values(record, 20), [-2.5]);
            assert_eq!(values(record, 30), [0.0]);
            assert!(record.1.contains(&(100, "AcDbPoint".into())));
            assert!(record.1.contains(&(100, "AcDbEntity".into())));
            assert!(!field(&record.1, 5).is_empty() && !field(&record.1, 330).is_empty());
            assert_eq!(values(record, 370), weight.map(|value| vec![f64::from(value.group370())]).unwrap_or_default());
            assert!(values(record, 39).is_empty() && values(record, 210).is_empty());
        }
        let fields = pairs(&dxf);
        for header in ["$EXTMIN", "$EXTMAX"] {
            let at = fields.iter().position(|(code, value)| *code == 9 && value == header).unwrap();
            assert_eq!(&fields[at + 1..at + 4], &[(10, "1.25".into()), (20, "-2.5".into()), (30, "0.0".into())]);
        }
    }

    #[test]
    fn point_uses_one_writer_work_unit_and_existing_output_byte_budget() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Point(Vec2::ZERO), None).unwrap();
        assert_eq!(shape_work(file.entities()[0].geometry()).unwrap(), 1);
        let output = write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 1).unwrap();
        assert!(write_dxf_file_limited(&file, output.len(), 1).is_ok());
        assert!(write_dxf_file_limited(&file, output.len() - 1, 1).is_err());
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 0).is_err());
    }

    #[test]
    fn point_nonfinite_locations_refuse_in_owned_and_legacy_routes() {
        for point in [Vec2::new(f64::NAN, 0.0), Vec2::new(0.0, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, 0.0)] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), DxfShape::Point(point), None).unwrap();
            let error = write_dxf_file(&file).unwrap_err().to_string();
            assert!(error.contains("entity 1") && error.contains("layer"));
            assert!(write_dxf_drawing_units(&[], &[DxfEntity { layer: "DATUM".into(), shape: DxfShape::Point(point) }], 4).is_err());
            assert_eq!(file.entities().len(), 1);
        }
    }

    #[test]
    fn finite_point_shares_writer_with_legacy_and_does_not_change_line_record_order() {
        let shapes = [DxfShape::Point(Vec2::new(-3.0, 4.0)), DxfShape::Line(Vec2::ZERO, Vec2::X)];
        let mut file = DxfFile::new().unwrap();
        for shape in &shapes {
            file.push(file.default_layer(), shape.clone(), None).unwrap();
        }
        let legacy: Vec<_> = shapes.into_iter().map(|shape| DxfEntity { layer: "0".into(), shape }).collect();
        let owned_output = write_dxf_file(&file).unwrap();
        assert_eq!(owned_output, write_dxf_drawing_units(&[], &legacy, 4).unwrap());
        assert_eq!(entities(&owned_output).iter().map(|record| record.0.as_str()).collect::<Vec<_>>(), ["POINT", "LINE"]);
    }

    #[test]
    fn all_nine_text_alignments_emit_spec_codes_and_both_anchor_points() {
        for (horizontal, code72) in
            [(DxfTextHorizontalAlignment::Left, 0), (DxfTextHorizontalAlignment::Centre, 1), (DxfTextHorizontalAlignment::Right, 2)]
        {
            for (vertical, code73) in
                [(DxfTextVerticalAlignment::Baseline, 0), (DxfTextVerticalAlignment::Middle, 2), (DxfTextVerticalAlignment::Top, 3)]
            {
                let mut file = DxfFile::new().unwrap();
                let layer = file.declare_layer("NOTES", 4, DxfLinetype::Hidden, None).unwrap();
                file.push(
                    layer,
                    DxfShape::AlignedText { at: Vec2::new(10.0, 20.0), height: 2.5, angle_deg: 30.0, text: "A".into(), horizontal, vertical },
                    Some(DxfEntityWeight::ByLayer),
                )
                .unwrap();
                let mut calls = 0;
                // Explicit fixture from an external synthetic layout provider;
                // its bounds are inputs, never inferred by the writer.
                let bounds = DxfGeometryBounds::new(Vec2::new(7.0, 18.0), Vec2::new(15.0, 25.0)).unwrap();
                let output = write_dxf_file_with_text_bounds(&file, |shape| {
                    assert!(matches!(shape, DxfShape::AlignedText { horizontal: h, vertical: v, .. } if *h == horizontal && *v == vertical));
                    calls += 1;
                    Ok(bounds)
                })
                .unwrap();
                assert_eq!(calls, 1);
                let records = entities(&output);
                let record = records.first().unwrap();
                assert_eq!(record.0, "TEXT");
                assert_eq!(values(record, 72), [f64::from(code72)]);
                assert_eq!(values(record, 73), [f64::from(code73)]);
                for code in [10, 11] {
                    assert_eq!(values(record, code), [10.0]);
                }
                for code in [20, 21] {
                    assert_eq!(values(record, code), [20.0]);
                }
                for code in [30, 31] {
                    assert_eq!(values(record, code), [0.0]);
                }
                assert_eq!(field(&record.1, 8), "NOTES");
                assert_eq!(values(record, 370), [-1.0]);
                assert_eq!(values(record, 40), [2.5]);
                assert_eq!(values(record, 50), [30.0]);
                assert_eq!(record.1.iter().filter(|(code, value)| *code == 100 && value == "AcDbText").count(), 2);
                let headers = pairs(&output);
                let min = headers.iter().position(|(code, value)| *code == 9 && value == "$EXTMIN").unwrap();
                let max = headers.iter().position(|(code, value)| *code == 9 && value == "$EXTMAX").unwrap();
                assert_eq!(&headers[min + 1..min + 4], &[(10, "7.0".into()), (20, "18.0".into()), (30, "0.0".into())]);
                assert_eq!(&headers[max + 1..max + 4], &[(10, "15.0".into()), (20, "25.0".into()), (30, "0.0".into())]);
            }
        }
    }

    #[test]
    fn alignment_codes_reject_bottom_fit_and_unrepresented_modes() {
        for code in [-1, 3, 4, 5, i16::MAX] {
            assert!(DxfTextHorizontalAlignment::from_group72(code).is_err());
        }
        for code in [-1, 1, 4, i16::MAX] {
            assert!(DxfTextVerticalAlignment::from_group73(code).is_err());
        }
        for (code, expected) in
            [(0, DxfTextHorizontalAlignment::Left), (1, DxfTextHorizontalAlignment::Centre), (2, DxfTextHorizontalAlignment::Right)]
        {
            assert_eq!(DxfTextHorizontalAlignment::from_group72(code).unwrap(), expected);
            assert_eq!(expected.group72(), code);
        }
        for (code, expected) in [(0, DxfTextVerticalAlignment::Baseline), (2, DxfTextVerticalAlignment::Middle), (3, DxfTextVerticalAlignment::Top)] {
            assert_eq!(DxfTextVerticalAlignment::from_group73(code).unwrap(), expected);
            assert_eq!(expected.group73(), code);
        }
    }

    #[test]
    fn explicit_centre_baseline_matches_legacy_bytes_when_given_the_same_layout_bounds() {
        let at = Vec2::new(5.0, 6.0);
        let legacy = DxfShape::Text { at, height: 2.5, angle_deg: 0.0, text: "AB".into() };
        let legacy_output = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape: legacy }], 4).unwrap();
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::AlignedText {
                at,
                height: 2.5,
                angle_deg: 0.0,
                text: "AB".into(),
                horizontal: DxfTextHorizontalAlignment::Centre,
                vertical: DxfTextVerticalAlignment::Baseline,
            },
            None,
        )
        .unwrap();
        // Synthetic metrics chosen analytically: centre at5,6; width4,height2.5.
        let output = write_dxf_file_with_text_bounds(&file, |_| DxfGeometryBounds::new(Vec2::new(3.0, 6.0), Vec2::new(7.0, 8.5))).unwrap();
        assert_eq!(output, legacy_output);
        let mut old_file = DxfFile::new().unwrap();
        old_file.push(old_file.default_layer(), DxfShape::Text { at, height: 2.5, angle_deg: 0.0, text: "AB".into() }, None).unwrap();
        assert_eq!(write_dxf_file_with_text_bounds(&old_file, |_| panic!("legacy Text must not request new metrics")).unwrap(), legacy_output);
    }

    #[test]
    fn explicit_alignment_refuses_missing_or_invalid_layout_bounds_without_a_partial_file() {
        let shape = DxfShape::AlignedText {
            at: Vec2::ZERO,
            height: 1.0,
            angle_deg: 0.0,
            text: "A".into(),
            horizontal: DxfTextHorizontalAlignment::Left,
            vertical: DxfTextVerticalAlignment::Top,
        };
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Point(Vec2::X), None).unwrap();
        file.push(file.default_layer(), shape.clone(), None).unwrap();
        assert!(write_dxf_file(&file).is_err());
        assert!(write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).is_err());
        for bounds in
            [DxfGeometryBounds { min: Vec2::new(f64::NAN, 0.0), max: Vec2::X }, DxfGeometryBounds { min: Vec2::new(1.0, 1.0), max: Vec2::ZERO }]
        {
            assert!(write_dxf_file_with_text_bounds(&file, |_| Ok(bounds)).is_err());
        }
        assert!(write_dxf_file_with_text_bounds(&file, |_| Err(IoError::Invalid("layout unavailable".into()))).is_err());
        assert_eq!(file.entities().len(), 2);
    }

    #[test]
    fn aligned_text_hostile_payloads_refuse_before_requesting_metrics() {
        for (at, height, angle_deg, text) in [
            (Vec2::new(f64::NAN, 0.0), 1.0, 0.0, "A".into()),
            (Vec2::ZERO, 0.0, 0.0, "A".into()),
            (Vec2::ZERO, -1.0, 0.0, "A".into()),
            (Vec2::ZERO, f64::INFINITY, 0.0, "A".into()),
            (Vec2::ZERO, 1.0, f64::NAN, "A".into()),
            (Vec2::ZERO, 1e-20, 0.0, "A".into()),
            (Vec2::ZERO, 1.0, 0.0, "𠀋".into()),
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(
                file.default_layer(),
                DxfShape::AlignedText {
                    at,
                    height,
                    angle_deg,
                    text,
                    horizontal: DxfTextHorizontalAlignment::Right,
                    vertical: DxfTextVerticalAlignment::Middle,
                },
                None,
            )
            .unwrap();
            let mut requested = false;
            assert!(
                write_dxf_file_with_text_bounds(&file, |_| {
                    requested = true;
                    DxfGeometryBounds::new(Vec2::ZERO, Vec2::X)
                })
                .is_err()
            );
            assert!(!requested);
        }
    }

    #[test]
    fn aligned_text_retains_existing_encoding_and_resource_boundaries() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            DxfShape::AlignedText {
                at: Vec2::ZERO,
                height: 1.0,
                angle_deg: 0.0,
                text: "A%%d\nB°\\{}".into(),
                horizontal: DxfTextHorizontalAlignment::Left,
                vertical: DxfTextVerticalAlignment::Baseline,
            },
            None,
        )
        .unwrap();
        let metrics = |_: &DxfShape| DxfGeometryBounds::new(Vec2::ZERO, Vec2::new(8.0, 1.0));
        let output = write_dxf_file_limited_with_text_bounds(&file, crate::MAX_DRAWING_BYTES, 2, metrics).unwrap();
        // Two lines: MTEXT, its `%`, `\\`, braces and degree sign escaped as typed.
        let [(kind, groups)] = &entities(&output)[..] else { panic!("{output}") };
        assert_eq!((kind.as_str(), field(groups, 1)), ("MTEXT", "A\\U+0025\\U+0025d\\PB\\U+00B0\\\\\\{\\}"));
        assert!(write_dxf_file_limited_with_text_bounds(&file, output.len(), 2, metrics).is_ok());
        assert!(write_dxf_file_limited_with_text_bounds(&file, output.len() - 1, 2, metrics).is_err());
        assert!(
            write_dxf_file_limited_with_text_bounds(&file, crate::MAX_DRAWING_BYTES, 1, |_| panic!("over-budget text must not request metrics"))
                .is_err()
        );
    }

    /// The ellipse of 50 × 24 round (50, 40): its four quarters, counter-clockwise from (75, 40).
    fn oval() -> [Seg2; 4] {
        let (c, u, v) = (Vec2::new(50.0, 40.0), Vec2::new(25.0, 0.0), Vec2::new(0.0, 12.0));
        [quarter(c, u, v), quarter(c, v, -u), quarter(c, -u, -v), quarter(c, -v, u)]
    }

    fn ellipse_point(e: &DxfShape, t: f64) -> Vec2 {
        let DxfShape::Ellipse { c, major, ratio, .. } = e else { panic!("{e:?}") };
        *c + *major * t.cos() + major.perp() * (*ratio * t.sin())
    }

    fn values(e: &(String, Vec<(i32, String)>), code: i32) -> Vec<f64> {
        e.1.iter().filter(|(c, _)| *c == code).map(|(_, v)| v.parse().unwrap()).collect()
    }

    fn near(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn a_conic_arc_below_weight_one_is_an_elliptical_arc() {
        // By hand: the first quarter of the 25 × 12 ellipse round (50, 40) runs from (75, 40)
        // over the corner (75, 52) to (50, 52) with weight cos 45°. Its centre is
        // m − (apex − m)·w²/(1 − w²) = (62.5, 46) − (12.5, 6)·1 = (50, 40); the semi-diameters
        // u = (m − c)/w and v = (b − m)/sin 45° are (25, 12) and (−25, 12) over √2, which are
        // equally long, so the principal axes are 45° from them: the major one is
        // (u − v)/√2 = (25, 0), the minor one (u + v)/√2 = (0, 12), the ratio 12/25 and, from
        // (75, 40) to (50, 52), the parameters 0 to π/2.
        let [first, second, ..] = oval();
        let DxfShape::Ellipse { c, major, ratio, t0, t1 } = seg_shape(&first) else { panic!("{:?}", seg_shape(&first)) };
        assert!(c.dist(Vec2::new(50.0, 40.0)) < 1e-9 && major.dist(Vec2::new(25.0, 0.0)) < 1e-9, "{c:?} {major:?}");
        assert!((ratio - 0.48).abs() < 1e-12 && t0.abs() < 1e-9 && (t1 - FRAC_PI_2).abs() < 1e-9, "{ratio} {t0} {t1}");
        // Whatever axis the second quarter is described by, its arc starts and ends at the
        // segment's ends and every point of it lies on the segment.
        let tilt = |v: Vec2| Vec2::from_angle(0.3) * v.x + Vec2::from_angle(0.3).perp() * v.y;
        let (centre, u, v) = (Vec2::new(5.0, -2.0), tilt(Vec2::new(15.0, 0.0)), tilt(Vec2::new(0.0, 8.0)));
        let general = quarter(centre, u, v).split_at(0.3).1;
        for seg in [first, second, quarter(centre, u, v), quarter(centre, v, -u).reversed(), general, general.reversed()] {
            let e = seg_shape(&seg);
            let DxfShape::Ellipse { t0, t1, .. } = e else { panic!("{e:?}") };
            let sweep = (t1 - t0).rem_euclid(TAU);
            for i in 0..=20 {
                let p = ellipse_point(&e, t0 + sweep * i as f64 / 20.0);
                assert!(seg.point_at(seg.closest_param(p)).dist(p) < 1e-9, "{seg:?} {p:?}");
            }
            let ends = [ellipse_point(&e, t0), ellipse_point(&e, t0 + sweep)];
            assert!(ends.iter().any(|p| p.dist(seg.start()) < 1e-9) && ends.iter().any(|p| p.dist(seg.end()) < 1e-9), "{ends:?} {seg:?}");
        }
    }

    #[test]
    fn four_quarters_are_one_ellipse() {
        let quarters = oval();
        let reversed = Loop2 { segs: quarters.iter().rev().map(Seg2::reversed).collect() };
        let shifted = Loop2 { segs: quarters.iter().cycle().skip(2).take(4).copied().collect() };
        for lp in [Loop2 { segs: quarters.to_vec() }, reversed, shifted] {
            let shapes = loop_shapes(&lp);
            let [DxfShape::Ellipse { c, major, ratio, t0, t1 }] = shapes.as_slice() else { panic!("{shapes:?}") };
            assert!(
                c.dist(Vec2::new(50.0, 40.0)) < 1e-9 && (major.len() - 25.0).abs() < 1e-9 && major.cross(Vec2::X).abs() < 1e-9,
                "{c:?} {major:?}"
            );
            assert!((ratio - 0.48).abs() < 1e-12 && *t0 == 0.0 && *t1 == TAU, "{ratio} {t0} {t1}");
        }
        // Three quarters and a line are pieces: three ELLIPSE arcs and a LINE.
        let (q, line) = (oval(), Seg2::Line { a: Vec2::new(50.0, 28.0), b: Vec2::new(50.0, 28.0) + Vec2::X });
        let open = loop_shapes(&Loop2 { segs: vec![q[0], q[1], q[2], line] });
        assert!(matches!(open.as_slice(), [DxfShape::Ellipse { t1, .. }, _, _, DxfShape::Line(..)] if (*t1 - FRAC_PI_2).abs() < 1e-9), "{open:?}");
        // A circle of arcs is the caller's business, but a whole turn is one CIRCLE.
        let turn = Seg2::Arc { center: Vec2::new(3.0, 4.0), radius: 2.0, start: -PI, sweep: -TAU };
        assert_eq!(seg_shape(&turn), DxfShape::Circle { c: Vec2::new(3.0, 4.0), r: 2.0 });
    }

    #[test]
    fn beziers_and_the_other_conics_are_splines() {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 2.0), Vec2::new(3.0, 3.0), Vec2::new(4.0, 0.0)];
        let cubic = seg_shape(&Seg2::Cubic { p0: p[0], p1: p[1], p2: p[2], p3: p[3] });
        assert_eq!(cubic, DxfShape::Spline { degree: 3, knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], ctrl: p.to_vec(), weights: None });
        // A quadratic (a glyph curve) is a parabola: degree 2, not rational. Weight 2 is a
        // hyperbola: rational. Weight 1 − 10⁻⁹ is no ellipse a reader could place: rational too.
        let conic = |w: f64| seg_shape(&Seg2::Conic { a: p[0], apex: p[1], b: p[3], w });
        let degree_2 = |s: &DxfShape, weights: Option<Vec<f64>>| {
            *s == DxfShape::Spline { degree: 2, knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], ctrl: vec![p[0], p[1], p[3]], weights }
        };
        assert!(degree_2(&conic(1.0), None));
        assert!(degree_2(&conic(2.0), Some(vec![1.0, 2.0, 1.0])));
        assert!(degree_2(&conic(1.0 - 1e-9), Some(vec![1.0, 1.0 - 1e-9, 1.0])));
        // A quintic piece: six control points and twelve knots.
        let mut q = [Vec2::ZERO; solidcore_record::MAX_BEZIER_CTRL];
        for (i, s) in q.iter_mut().enumerate() {
            *s = Vec2::new(i as f64, (i * i) as f64);
        }
        let DxfShape::Spline { degree, knots, ctrl, weights } = seg_shape(&Seg2::Bezier { n: 5, p: q }) else { panic!() };
        assert_eq!((degree, knots.len(), ctrl.len(), weights), (5, 12, 6, None));
        assert!(knots[..6].iter().all(|k| *k == 0.0) && knots[6..].iter().all(|k| *k == 1.0) && ctrl[5] == q[5]);
    }

    #[test]
    fn a_bezier_is_a_spline_of_its_own_degree_over_all_its_control_points() {
        let mut q = [Vec2::ZERO; solidcore_record::MAX_BEZIER_CTRL];
        for (i, s) in q.iter_mut().enumerate() {
            *s = Vec2::new(i as f64, (i * i) as f64);
        }
        // Every degree a Bézier piece can have, up to a G4 blend's 9: n + 1 control points, all of
        // them, and 2(n + 1) clamped knots.
        for n in 4..=solidcore_record::MAX_BEZIER_DEGREE as u8 {
            let k = usize::from(n) + 1;
            let DxfShape::Spline { degree, knots, ctrl, weights } = seg_shape(&Seg2::Bezier { n, p: q }) else { panic!("degree {n}") };
            assert_eq!((degree, knots.len(), weights), (n, 2 * k, None), "degree {n}");
            assert_eq!(ctrl, q[..k], "degree {n}");
        }
        // The fallible path API refuses malformed storage before any helper can clamp it.
        for n in [0, 1, 2, 3, 10, u8::MAX] {
            assert!(path_shapes(DxfPath::Open(&[Seg2::Bezier { n, p: q }]), DxfPathStyle::Exact).is_err());
        }
    }

    #[test]
    fn drawings_carry_version_units_extents_and_handles() {
        let ents = vec![
            DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Line(Vec2::new(-1.0, 2.0), Vec2::new(10.0, 2.0)) },
            DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 4.0 } },
            DxfEntity { layer: "CUT".into(), shape: DxfShape::Line(Vec2::ZERO, Vec2::X) },
        ];
        let dxf = write_dxf_drawing_units(&[DxfLayer::new("OUTLINE", 7), DxfLayer::new("CUT", 1)], &ents, 4).unwrap();
        let p = pairs(&dxf);
        let after = |var: &str| &p[p.iter().position(|(c, v)| (*c, v.as_str()) == (9, var)).unwrap() + 1..];
        assert_eq!(after("$ACADVER")[0], (1, "AC1015".to_string()));
        assert_eq!(after("$INSUNITS")[0], (70, "4".to_string()));
        // The circle reaches (1, 1) to (9, 9), the first line starts at x = −1 and the last at y = 0.
        assert_eq!(&after("$EXTMIN")[..3], &[(10, "-1.0".to_string()), (20, "0.0".to_string()), (30, "0.0".to_string())]);
        assert_eq!(&after("$EXTMAX")[..2], &[(10, "10.0".to_string()), (20, "9.0".to_string())]);
        // Layer 0 and the two layers are in the LAYER table, each entity is on one of them,
        // and the entities belong to the model space block record.
        let layer_table = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "LAYER")).unwrap();
        let names: Vec<&str> = p[layer_table..]
            .iter()
            .take_while(|(c, v)| (*c, v.as_str()) != (0, "ENDTAB"))
            .filter(|(c, _)| *c == 2)
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(names, ["LAYER", "0", "OUTLINE", "CUT"]);
        // (The first "*Model_Space" is the BLOCK_RECORD, whose handle is four pairs before its name.)
        let model_at = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "*Model_Space")).unwrap();
        let model = p[model_at - 4].1.clone();
        assert_eq!(p[model_at - 4].0, 5);
        for e in entities(&dxf) {
            assert!(e.1.contains(&(330, model.clone())) && e.1.contains(&(100, "AcDbEntity".to_string())), "{e:?}");
            assert!(names.contains(&e.1.iter().find(|(c, _)| *c == 8).unwrap().1.as_str()));
        }
        // Handles are unique and the seed is above all of them.
        let seed_at = p.iter().position(|(c, v)| (*c, v.as_str()) == (9, "$HANDSEED")).unwrap() + 1;
        let handles: Vec<u64> =
            p.iter().enumerate().filter(|(i, (c, _))| *i != seed_at && *c == 5).map(|(_, (_, v))| u64::from_str_radix(v, 16).unwrap()).collect();
        let mut sorted = handles.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), handles.len());
        assert!(handles.len() > ents.len() && handles.iter().all(|h| *h < u64::from_str_radix(&p[seed_at].1, 16).unwrap()));
        assert!(dxf.ends_with("  0\nEOF\n"));
        // No entities: a valid, empty drawing.
        assert_eq!(entities(&write_dxf_drawing_units(&[], &[], 4).unwrap()).len(), 0);
    }

    #[test]
    fn what_is_written_is_what_the_reader_gets_back() {
        let quarter_one = seg_shape(&oval()[0]);
        let cubic = [Vec2::new(0.0, 0.0), Vec2::new(5.0, 10.0), Vec2::new(15.0, 10.0), Vec2::new(20.0, 0.0)];
        let shapes = vec![
            DxfShape::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
            DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 2.0 },
            DxfShape::Arc { c: Vec2::ZERO, r: 3.0, a0: 0.0, a1: 90.0 },
            loop_shapes(&Loop2 { segs: oval().to_vec() }).remove(0),
            quarter_one,
            seg_shape(&Seg2::Cubic { p0: cubic[0], p1: cubic[1], p2: cubic[2], p3: cubic[3] }),
        ];
        let ents: Vec<DxfEntity> = shapes.into_iter().map(|shape| DxfEntity { layer: "0".into(), shape }).collect();
        let dxf = write_dxf_drawing_units(&[], &ents, 4).unwrap();
        let back = read_dxf(&dxf).unwrap();
        assert_eq!(back.len(), 6, "{back:?}");
        assert!(matches!(&back[0], Geom2::Line(a, b) if a.dist(Vec2::ZERO) < 1e-9 && b.dist(Vec2::new(10.0, 0.0)) < 1e-9));
        assert!(matches!(&back[1], Geom2::Circle(c, r) if c.dist(Vec2::new(5.0, 5.0)) < 1e-9 && (r - 2.0).abs() < 1e-9));
        assert!(
            matches!(&back[2], Geom2::Arc { c, a, b } if c.dist(Vec2::ZERO) < 1e-9 && a.dist(Vec2::new(3.0, 0.0)) < 1e-9 && b.dist(Vec2::new(0.0, 3.0)) < 1e-9)
        );
        // The whole ellipse: centre, end of the major axis, minor radius.
        assert!(
            matches!(&back[3], Geom2::Ellipse { c, major, minor } if c.dist(Vec2::new(50.0, 40.0)) < 1e-9 && major.dist(Vec2::new(75.0, 40.0)) < 1e-9 && (minor - 12.0).abs() < 1e-9),
            "{:?}",
            back[3]
        );
        // The quarter of the ellipse comes back as the one conic it was: from (75, 40) to (50, 52)
        // with its apex at (75, 52) and its weight (#8155).
        let Geom2::Conic { a, apex, b, rho } = &back[4] else { panic!("{:?}", back[4]) };
        assert!(a.dist(Vec2::new(75.0, 40.0)) < 1e-9 && apex.dist(Vec2::new(75.0, 52.0)) < 1e-9 && b.dist(Vec2::new(50.0, 52.0)) < 1e-9);
        assert!((rho - FRAC_1_SQRT_2 / (1.0 + FRAC_1_SQRT_2)).abs() < 1e-12, "{rho}");
        // The cubic: degree and control points.
        let Geom2::Spline { pts, control: true, degree: 3 } = &back[5] else { panic!("{:?}", back[5]) };
        assert!(pts.iter().zip(&cubic).all(|(a, b)| a.dist(*b) < 1e-9) && pts.len() == 4);
        // A degree-9 Bézier (a G4 blend) comes back at its own degree over all ten points.
        let mut q = [Vec2::ZERO; solidcore_record::MAX_BEZIER_CTRL];
        for (i, s) in q.iter_mut().enumerate() {
            *s = Vec2::new(i as f64, (i % 3) as f64);
        }
        let nine = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape: seg_shape(&Seg2::Bezier { n: 9, p: q }) }], 4).unwrap();
        let back_nine = read_dxf(&nine).unwrap();
        assert!(
            matches!(&back_nine[..], [Geom2::Spline { pts, control: true, degree: 9 }] if pts.len() == 10 && pts.iter().zip(&q).all(|(a, b)| a.dist(*b) < 1e-9)),
            "{back_nine:?}"
        );

        // The layered reader keeps the actual ellipse and spline parameters,
        // including the knots and optional rational weights.
        let file = crate::read_dxf_file(&dxf).unwrap();
        let records = file.entities();
        assert_eq!(records.len(), 6);
        assert!(matches!(records[0].geometry(), DxfShape::Line(..)));
        assert!(matches!(records[1].geometry(), DxfShape::Circle { .. }));
        assert!(matches!(records[2].geometry(), DxfShape::Arc { .. }));
        let DxfShape::Ellipse { c, major, ratio, t0, t1 } = records[3].geometry() else { panic!("{records:?}") };
        assert!(near(&[c.x, c.y, major.x, major.y], &[50.0, 40.0, 25.0, 0.0]));
        assert!(near(&[*ratio, *t0, *t1], &[0.48, 0.0, TAU]));
        let DxfShape::Ellipse { ratio, t0, t1, .. } = records[4].geometry() else { panic!("{records:?}") };
        assert!(near(&[*t0, *t1, *ratio], &[0.0, FRAC_PI_2, 0.48]));
        let DxfShape::Spline { degree, knots, ctrl, weights } = records[5].geometry() else { panic!("{records:?}") };
        assert_eq!((*degree, knots.len(), ctrl.len()), (3, 8, 4));
        assert!(near(knots, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]));
        assert!(near(&ctrl.iter().map(|p| p.x).collect::<Vec<_>>(), &[0.0, 5.0, 15.0, 20.0]));
        assert!(near(&ctrl.iter().map(|p| p.y).collect::<Vec<_>>(), &[0.0, 10.0, 10.0, 0.0]));
        assert!(weights.is_none());
        // Keep independent wire assertions for the explicit planar normal and
        // flags, rather than inventing those fields from the semantic model.
        let wire = entities(&dxf);
        assert!(near(&values(&wire[3], 230), &[1.0]));
        assert_eq!((values(&wire[5], 70), values(&wire[5], 74)), (vec![8.0], vec![0.0]));
    }

    #[test]
    fn a_rational_spline_lists_knots_then_weights_then_control_points() {
        let hyperbola = seg_shape(&Seg2::Conic { a: Vec2::ZERO, apex: Vec2::new(1.0, 2.0), b: Vec2::new(3.0, 0.0), w: 2.0 });
        let dxf = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape: hyperbola }], 4).unwrap();
        let e = &entities(&dxf)[0];
        let file = crate::read_dxf_file(&dxf).unwrap();
        let DxfShape::Spline { knots, weights: Some(weights), .. } = file.entities()[0].geometry() else { panic!("{file:?}") };
        assert!(near(weights, &[1.0, 2.0, 1.0]) && near(knots, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]));
        assert_eq!(values(e, 70), [12.0]);
        let at = |code: i32| e.1.iter().position(|(c, _)| *c == code).unwrap();
        let last = |code: i32| e.1.iter().rposition(|(c, _)| *c == code).unwrap();
        assert!(last(40) < at(41) && last(41) < at(10), "{:?}", e.1);
    }

    fn field(record: &[(i32, String)], code: i32) -> &str {
        record.iter().find(|(c, _)| *c == code).map_or("", |(_, v)| v.as_str())
    }

    #[test]
    fn a_dashed_layer_names_its_linetype_and_the_table_defines_it() {
        let dxf = write_dxf_drawing_units(&[DxfLayer::new("BEND", 1).dashed(), DxfLayer::new("CUT", 4)], &[], 4).unwrap();
        let p = pairs(&dxf);
        // The linetypes of every drawing, then DASHED: a dash of 12.7 mm and a gap of 6.35 mm, 19.05 mm in all.
        let types = table_records(&p, "LTYPE");
        assert_eq!(types.iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["ByBlock", "ByLayer", "Continuous", "DASHED"]);
        let dashed = &types[3];
        let numbers = |code: i32| dashed.iter().filter(|(c, _)| *c == code).map(|(_, v)| v.parse::<f64>().unwrap()).collect::<Vec<_>>();
        assert!(near(&numbers(49), &[12.7, -6.35]) && near(&numbers(40), &[19.05]) && near(&numbers(73), &[2.0]) && near(&numbers(72), &[65.0]));
        // The table says how many records it holds.
        let at = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, "LTYPE")).unwrap();
        assert_eq!(p[at..].iter().find(|(c, _)| *c == 70).map(|(_, v)| v.as_str()), Some("4"));
        // Each layer names its linetype.
        let layers = table_records(&p, "LAYER");
        assert_eq!(
            layers.iter().map(|r| (field(r, 2), field(r, 62), field(r, 6))).collect::<Vec<_>>(),
            [("0", "7", "Continuous"), ("BEND", "1", "DASHED"), ("CUT", "4", "Continuous")]
        );
    }

    #[test]
    fn hidden_polylines_continue_short_dashes_across_chords_and_round_trip() {
        let points = vec![Vec2::ZERO, Vec2::new(1., 0.), Vec2::new(2., 1.), Vec2::new(3., 1.)];
        let layer = DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", 8) };
        let text = write_dxf_drawing_units(&[layer], &[DxfEntity { layer: "HIDDEN".into(), shape: DxfShape::Polyline { points } }], 4).unwrap();
        let records = entities(&text);
        assert_eq!(records[0].0, "LWPOLYLINE");
        assert_eq!(field(&records[0].1, 70), "128");
        assert_eq!(field(&records[0].1, 90), "4");
        let types = table_records(&pairs(&text), "LTYPE");
        let hidden = types.iter().find(|r| field(r, 2) == "HIDDEN").unwrap();
        assert_eq!(hidden.iter().filter(|(c, _)| *c == 49).map(|(_, v)| v.parse::<f64>().unwrap()).collect::<Vec<_>>(), [2., -1.]);
        assert_eq!(read_dxf(&text).unwrap().len(), 3);
    }

    #[test]
    fn text_is_centred_on_its_point_and_the_reader_gets_it_back() {
        let at = Vec2::new(30.0, 12.0);
        let note = DxfShape::Text { at, height: 3.0, angle_deg: 90.0, text: "AB%%d\tC\u{b0}".into() };
        let dxf = write_dxf_drawing_units(&[DxfLayer::new("NOTES", 8)], &[DxfEntity { layer: "NOTES".into(), shape: note }], 4).unwrap();
        let file = entities(&dxf);
        let [text] = file.as_slice() else { panic!("{file:?}") };
        assert_eq!(text.0, "TEXT");
        // One line (the tab is a space), each typed `%` as `%%%`, and `%%d` the degree sign.
        assert_eq!(text.1.iter().find(|(c, _)| *c == 1).map(|(_, v)| v.as_str()), Some("AB%%%%%%d C%%d"));
        // Centred (72 = 1) on the second alignment point, which is the first as well; 3 mm high, turned 90°.
        assert!(
            near(&values(text, 11), &[30.0])
                && near(&values(text, 21), &[12.0])
                && near(&values(text, 10), &[30.0])
                && near(&values(text, 20), &[12.0])
        );
        assert!(
            near(&values(text, 72), &[1.0]) && near(&values(text, 73), &[0.0]) && near(&values(text, 40), &[3.0]) && near(&values(text, 50), &[90.0])
        );
        // Its style is in the STYLE table, and the layer it is on in the LAYER table.
        let p = pairs(&dxf);
        assert_eq!(table_records(&p, "STYLE").iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["Standard"]);
        assert!(text.1.contains(&(8, "NOTES".to_string())));
        // The reader finds the text where it was put, as high and as turned.
        let back = read_dxf(&dxf).unwrap();
        assert!(
            matches!(&back[..], [Geom2::Text { at: found, height, angle, .. }] if found.dist(at) < 1e-9 && (height - 3.0).abs() < 1e-9 && (angle - FRAC_PI_2).abs() < 1e-9),
            "{back:?}"
        );
    }

    #[test]
    fn two_lines_are_mtext_at_the_same_point_and_the_strict_reader_gets_them_back_as_typed() {
        let at = Vec2::new(30.0, 12.0);
        let typed = "AB%%d\n50% C\u{b0} {x}\\";
        let note = DxfShape::Text { at, height: 3.0, angle_deg: 90.0, text: typed.into() };
        let dxf = write_dxf_drawing_units(&[DxfLayer::new("NOTES", 8)], &[DxfEntity { layer: "NOTES".into(), shape: note.clone() }], 4).unwrap();
        let [entity] = &entities(&dxf)[..] else { panic!("{dxf}") };
        let groups = &entity.1;
        assert_eq!(entity.0, "MTEXT");
        assert_eq!(field(groups, 1), "AB\\U+0025\\U+0025d\\P50\\U+0025 C\\U+00B0 \\{x\\}\\\\");
        // Its insertion point is the point; bottom centre (71 = 8); no wrapping; turned 90° by its direction.
        assert!(near(&values(entity, 10), &[30.0]) && near(&values(entity, 20), &[12.0]) && near(&values(entity, 40), &[3.0]));
        assert!(near(&values(entity, 71), &[8.0]) && near(&values(entity, 41), &[0.0]) && near(&values(entity, 72), &[1.0]));
        assert!(near(&values(entity, 11), &[0.0]) && near(&values(entity, 21), &[1.0]) && values(entity, 50).is_empty());
        let back = crate::read_dxf_file(&dxf).unwrap();
        let DxfShape::Text { at: found, height, angle_deg, text } = back.entities()[0].geometry() else { panic!("{back:?}") };
        assert!(found.dist(at) < 1e-9 && *height == 3.0 && (angle_deg - 90.0).abs() < 1e-9, "{back:?}");
        assert_eq!(text, typed);
    }

    #[test]
    fn non_bmp_text_is_never_silently_replaced() {
        for c in ['𠀋', '𝑥', '😀', '\u{10000}', '\u{10ffff}'] {
            let shape = DxfShape::Text { at: Vec2::ZERO, height: 3.0, angle_deg: 0.0, text: format!("中{c}") };
            let entities =
                [DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Line(Vec2::ZERO, Vec2::X) }, DxfEntity { layer: "NOTES".into(), shape }];
            let error = write_dxf_drawing_units(&[DxfLayer::new("NOTES", 8)], &entities, 4).unwrap_err();
            assert!(matches!(error, IoError::Invalid(_)), "{error}");
            let message = error.to_string();
            assert!(message.contains("DXF TEXT entity 2 on layer \"NOTES\""), "{message}");
            assert!(message.contains(&format!("{c:?} (U+{:04X}) at position 2", u32::from(c))), "{message}");
            assert!(message.contains("only characters through U+FFFF are supported"), "{message}");
        }
    }

    #[test]
    fn non_bmp_text_past_the_output_limit_is_also_refused() {
        // Even a character beyond the existing output limit must not vanish unnoticed.
        for prefix in [249, 250, 10_000] {
            let shape = DxfShape::Text { at: Vec2::ZERO, height: 3.0, angle_deg: 0.0, text: format!("{}𠀋", "中".repeat(prefix)) };
            let error = write_dxf_drawing_units(&[], &[DxfEntity { layer: "NOTES".into(), shape }], 4).unwrap_err().to_string();
            assert!(error.contains(&format!("U+2000B) at position {}", prefix + 1)), "{error}");
            assert!(error.len() < 300, "the error must not repeat unbounded input text");
        }
    }

    #[test]
    fn bmp_text_is_written_as_typed_through_ffff() {
        for (text, expected) in [
            ("ASCII %%d %%p %%c ° 中\t\u{ffff}".into(), "ASCII %%%%%%d %%%%%%p %%%%%%c %%d \\U+4E2D \\U+FFFF".into()),
            ("\u{ffff}".repeat(256), "\\U+FFFF".repeat(256)),
        ] {
            let shape = DxfShape::Text { at: Vec2::ZERO, height: 3.0, angle_deg: 0.0, text };
            let dxf = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).unwrap();
            let file = entities(&dxf);
            let [entity] = file.as_slice() else { panic!("{file:?}") };
            assert_eq!(field(&entity.1, 1), expected);
            assert!(dxf.contains("AC1015"));
        }
    }

    #[test]
    fn a_strict_reader_finds_every_section_table_and_the_group_dictionary() {
        // #9144: AutoCAD refused the exported view. A release 2000 file has the CLASSES section,
        // all nine symbol tables with their standard records, and a root dictionary naming
        // ACAD_GROUP.
        let ents = [
            DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Line(Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0)) },
            DxfEntity { layer: "VISIBLE".into(), shape: DxfShape::Circle { c: Vec2::new(15.0, 5.0), r: 5.0 } },
        ];
        let dxf = write_dxf_drawing_units(&[DxfLayer::new("VISIBLE", 7)], &ents, 4).unwrap();
        let p = pairs(&dxf);
        let sections: Vec<&str> = p.windows(2).filter(|w| w[0] == (0, "SECTION".into())).map(|w| w[1].1.as_str()).collect();
        assert_eq!(sections, ["HEADER", "CLASSES", "TABLES", "BLOCKS", "ENTITIES", "OBJECTS"]);
        let tables: Vec<&str> = p.windows(2).filter(|w| w[0] == (0, "TABLE".into())).map(|w| w[1].1.as_str()).collect();
        assert_eq!(tables, ["VPORT", "LTYPE", "LAYER", "STYLE", "VIEW", "UCS", "APPID", "DIMSTYLE", "BLOCK_RECORD"]);
        // Each table says how many records it holds, and holds them.
        for table in tables {
            let at = p.iter().position(|(c, v)| (*c, v.as_str()) == (2, table)).unwrap();
            let count: usize = p[at..].iter().find(|(c, _)| *c == 70).unwrap().1.parse().unwrap();
            assert_eq!(table_records(&p, table).len(), count, "{table}");
        }
        assert_eq!(table_records(&p, "APPID").iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["ACAD"]);
        let dimstyles = table_records(&p, "DIMSTYLE");
        // (The DIMSTYLE table's own subclass comes before its record.)
        let [standard] = dimstyles.as_slice() else { panic!("{dimstyles:?}") };
        assert_eq!(field(standard, 2), "Standard");
        assert!(!field(standard, 105).is_empty() && field(standard, 5).is_empty(), "{standard:?}");
        // The active viewport looks down on the middle of the drawing (0, 0)..(30, 10), all of it in view.
        let vports = table_records(&p, "VPORT");
        let [active] = vports.as_slice() else { panic!("{vports:?}") };
        assert_eq!(field(active, 2), "*Active");
        assert_eq!((field(active, 12), field(active, 22), field(active, 36)), ("15.0", "5.0", "1.0"));
        let (height, aspect): (f64, f64) = (field(active, 40).parse().unwrap(), field(active, 41).parse().unwrap());
        assert!(height >= 10.0 && height * aspect >= 30.0, "{height} {aspect}");
        // The root dictionary names the group dictionary, which it owns.
        let dictionaries = crate::dxf::test_wire_section_records(&p, "OBJECTS");
        let root = &dictionaries[0];
        let groups = dictionaries.iter().find(|d| field(d, 5) == field(root, 350)).unwrap();
        assert_eq!((field(root, 330), field(root, 3)), ("0", "ACAD_GROUP"));
        assert_eq!(field(root, 350), field(groups, 5));
        assert_eq!(field(groups, 330), field(root, 5));
        // Handles, under 5 and the DIMSTYLE's 105, are unique and below the seed.
        let seed_at = p.iter().position(|(c, v)| (*c, v.as_str()) == (9, "$HANDSEED")).unwrap() + 1;
        let seed = u64::from_str_radix(&p[seed_at].1, 16).unwrap();
        let mut handles: Vec<u64> = p
            .iter()
            .enumerate()
            .filter(|(i, (c, _))| *i != seed_at && (*c == 5 || *c == 105))
            .map(|(_, (_, v))| u64::from_str_radix(v, 16).unwrap())
            .collect();
        let n = handles.len();
        handles.sort_unstable();
        handles.dedup();
        assert_eq!(handles.len(), n);
        assert!(handles.iter().all(|h| *h < seed));
        // Every owner (330) is 0 or a handle in the file.
        for (_, v) in p.iter().filter(|(c, _)| *c == 330) {
            let h = u64::from_str_radix(v, 16).unwrap();
            assert!(h == 0 || handles.binary_search(&h).is_ok(), "owner {v}");
        }
        // SolidCraft's own reader still reads it.
        assert_eq!(read_dxf(&dxf).unwrap().len(), 2);
    }

    /// The objects of the OBJECTS section, or the classes of the CLASSES section, each as its
    /// type and its group codes.
    fn section_objects(p: &[(i32, String)], section: &str) -> Vec<(String, Vec<(i32, String)>)> {
        crate::dxf::test_wire_section_objects(p, section)
    }

    #[test]
    fn release_2000_layouts_link_their_block_records_and_layers_their_plot_style() {
        // #16899: AutoCAD refused a release 2000 file with no layouts. The root dictionary names
        // ACAD_LAYOUT, which holds a Model and a Layout1 LAYOUT; each space's block record names
        // its layout under 340 and the layout names the block record back under 330.
        let ents = [DxfEntity { layer: "OUTLINE".into(), shape: DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 4.0 } }];
        let dxf = write_dxf_drawing_units(&[DxfLayer::new("OUTLINE", 7)], &ents, 4).unwrap();
        let p = pairs(&dxf);
        let objects = section_objects(&p, "OBJECTS");
        let by_handle = |h: &str| objects.iter().find(|(_, o)| field(o, 5) == h).unwrap_or_else(|| panic!("no object {h}"));
        let entry = |dict: &[(i32, String)], name: &str| {
            let at = dict.iter().position(|(c, v)| (*c, v.as_str()) == (3, name)).unwrap_or_else(|| panic!("no entry {name}"));
            assert_eq!(dict[at + 1].0, 350);
            dict[at + 1].1.clone()
        };
        let (root_kind, root) = &objects[0];
        assert_eq!((root_kind.as_str(), field(root, 330)), ("DICTIONARY", "0"));
        let layouts = by_handle(&entry(root, "ACAD_LAYOUT"));
        assert_eq!(layouts.0, "DICTIONARY");
        let records = table_records(&p, "BLOCK_RECORD");
        for (space, name) in [("*Model_Space", "Model"), ("*Paper_Space", "Layout1")] {
            let record = records.iter().find(|r| field(r, 2) == space).unwrap();
            let (kind, layout) = by_handle(&entry(&layouts.1, name));
            assert_eq!(kind, "LAYOUT");
            assert_eq!(field(record, 340), field(layout, 5), "{space}");
            // The layout's owner is the layout dictionary; after its AcDbLayout marker, its
            // block record.
            assert_eq!(field(layout, 330), field(&layouts.1, 5));
            let own = layout.iter().position(|(c, v)| (*c, v.as_str()) == (100, "AcDbLayout")).unwrap();
            assert_eq!((field(&layout[own..], 1), field(&layout[own..], 330)), (name, field(record, 5)), "{space}");
        }
        // Every layer has a lineweight and names the default plot style of ACAD_PLOTSTYLENAME.
        let (kind, styles) = by_handle(&entry(root, "ACAD_PLOTSTYLENAME"));
        assert_eq!(kind, "ACDBDICTIONARYWDFLT");
        let normal = entry(styles, "Normal");
        assert_eq!(field(styles, 340), normal);
        assert_eq!(by_handle(&normal).0, "ACDBPLACEHOLDER");
        let layers = table_records(&p, "LAYER");
        assert_eq!(layers.len(), 2);
        for layer in &layers {
            assert_eq!((field(layer, 370), field(layer, 390)), ("-3", normal.as_str()), "{layer:?}");
        }
        // Each object type that is not built in is a class.
        let classes: Vec<String> = section_objects(&p, "CLASSES").iter().map(|(_, c)| field(c, 1).to_string()).collect();
        for (kind, _) in &objects {
            assert!(kind == "DICTIONARY" || classes.contains(kind), "{kind} is not in CLASSES {classes:?}");
        }
        // Every handle is unique, the seed is past the largest, and every pointer (330, 340, 350,
        // 390) names one of them.
        let seed_at = p.iter().position(|(c, v)| (*c, v.as_str()) == (9, "$HANDSEED")).unwrap() + 1;
        let seed = u64::from_str_radix(&p[seed_at].1, 16).unwrap();
        let mut handles: Vec<u64> = p
            .iter()
            .enumerate()
            .filter(|(i, (c, _))| *i != seed_at && (*c == 5 || *c == 105))
            .map(|(_, (_, v))| u64::from_str_radix(v, 16).unwrap())
            .collect();
        let n = handles.len();
        handles.sort_unstable();
        handles.dedup();
        assert_eq!(handles.len(), n);
        assert!(handles.last().is_some_and(|h| *h < seed));
        for (_, v) in p.iter().filter(|(c, _)| matches!(*c, 330 | 340 | 350 | 390)) {
            let h = u64::from_str_radix(v, 16).unwrap();
            assert!(h == 0 || handles.binary_search(&h).is_ok(), "pointer {v}");
        }
    }

    #[test]
    fn the_extents_hold_the_text() {
        // Five letters of 3 mm, turned to read upwards: 0.8 × 3 × 5 = 12 mm long around y = 12, and
        // 3 mm to the left of the baseline at x = 30.
        let shape = DxfShape::Text { at: Vec2::new(30.0, 12.0), height: 3.0, angle_deg: 90.0, text: "ABCDE".into() };
        let dxf = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).unwrap();
        let p = pairs(&dxf);
        let after = |var: &str| &p[p.iter().position(|(c, v)| (*c, v.as_str()) == (9, var)).unwrap() + 1..];
        assert_eq!(&after("$EXTMIN")[..2], &[(10, "27.0".to_string()), (20, "6.0".to_string())]);
        assert_eq!(&after("$EXTMAX")[..2], &[(10, "30.0".to_string()), (20, "18.0".to_string())]);
    }

    #[test]
    fn lwpolyline_with_bulge_and_units_round_trip() {
        // Closed loop with a line and a 90° arc segment (bulge tan(pi / 8)).
        let sweep = FRAC_PI_2;
        let bulge = (sweep / 4.0).tan();
        let loop2 = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(0.0, 0.0) },
                Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(0.0, 10.0) },
                Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: FRAC_PI_2, sweep: -FRAC_PI_2 },
            ],
        };
        let shape = loop_lwpolyline(&loop2);
        let DxfShape::LwPolyline { ref verts, closed } = shape else { panic!("expected LwPolyline") };
        assert!(closed);
        assert_eq!(verts.len(), 3);
        // Third segment is clockwise (-90°), bulge is negative
        assert!((verts[2].1 - (-bulge)).abs() < 1e-12, "bulge was {}", verts[2].1);

        // Write with inch units (insunits = 1)
        let dxf = write_dxf_drawing_units(
            &[DxfLayer::new("CUT", 4)],
            &[DxfEntity { layer: "CUT".into(), shape: loop_lwpolyline_scaled(&loop2, 1.0 / 25.4) }],
            1,
        )
        .unwrap();
        // Wire checks through the shared lexer: the layered reader doesn't take an inch file with
        // patterned linetypes yet, and $INSUNITS is a header value it converts away.
        let [(kind, groups)] = &entities(&dxf)[..] else { panic!("{dxf}") };
        assert_eq!((kind.as_str(), field(groups, 8)), ("LWPOLYLINE", "CUT"));
        let p = pairs(&dxf);
        let after = |var: &str| &p[p.iter().position(|(c, v)| (*c, v.as_str()) == (9, var)).unwrap() + 1..];
        assert_eq!(after("$INSUNITS")[0], (70, "1".to_string()));

        // Read back via read_dxf: should scale from inches back to mm
        let geom = read_dxf(&dxf).unwrap();
        assert_eq!(geom.len(), 3);
        // Arcs and lines should be recovered in millimetres
        let Some((c, a, b)) = geom.iter().find_map(|g| match *g {
            Geom2::Arc { c, a, b } => Some((c, a, b)),
            _ => None,
        }) else {
            panic!("arc not recovered");
        };
        assert!(c.dist(Vec2::ZERO) < 1e-6);
        assert!((c.dist(a) - 10.0).abs() < 1e-6);
        assert!((c.dist(b) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn lwpolyline_open_sampling_does_not_wrap_around() {
        // Unclosed polyline with 2 vertices: (0, 0) and (10, 0), with a bulge on the last vertex.
        // For an unclosed polyline, the last vertex cannot form an arc segment to vertex 0.
        let shape = DxfShape::LwPolyline { verts: vec![(Vec2::new(0.0, 0.0), 0.0), (Vec2::new(10.0, 0.0), 1.0)], closed: false };
        let mut pts = Vec::new();
        shape.sample(&mut pts).unwrap();
        // Only the two vertices should be sampled, not an arc chord back to (0, 0)
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0], Vec2::new(0.0, 0.0));
        assert_eq!(pts[1], Vec2::new(10.0, 0.0));
    }

    #[test]
    fn owned_file_and_legacy_drawing_have_identical_bytes_without_weights() {
        assert_eq!(write_dxf_file(&DxfFile::new().unwrap()).unwrap(), write_dxf_drawing_units(&[], &[], 4).unwrap());
        let shapes = vec![
            DxfShape::Line(Vec2::new(-0.0, -1.25), Vec2::new(1e-15, 8.0)),
            DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::X, Vec2::new(4.0, -2.0)] },
            DxfShape::Circle { c: Vec2::new(3.0, 2.0), r: 2.0 },
            DxfShape::Arc { c: Vec2::ZERO, r: 3.0, a0: 350.0, a1: 20.0 },
            DxfShape::Ellipse { c: Vec2::new(-2.0, 3.0), major: Vec2::new(4.0, 2.0), ratio: 0.5, t0: 0.0, t1: TAU },
            bezier_spline(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::X], None),
            bezier_spline(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::X], Some(vec![1.0, 0.5, 1.0])),
            DxfShape::Text { at: Vec2::new(5.0, 6.0), height: 2.0, angle_deg: 90.0, text: "中 ° %%d\nA".into() },
        ];
        for linetype in [DxfLinetype::Continuous, DxfLinetype::Dashed, DxfLinetype::Hidden] {
            let layers = [DxfLayer { name: "CUT".into(), color: 3, linetype }];
            let legacy: Vec<_> = shapes.iter().cloned().map(|shape| DxfEntity { layer: "CUT".into(), shape }).collect();
            let mut file = DxfFile::new().unwrap();
            let cut = file.declare_layer("CUT", 3, linetype, None).unwrap();
            for shape in &shapes {
                file.push(cut, shape.clone(), None).unwrap();
            }
            assert_eq!(write_dxf_file(&file).unwrap(), write_dxf_drawing_units(&layers, &legacy, 4).unwrap());
        }
    }

    #[test]
    fn owned_layer_table_preserves_order_weight_zero_and_omission() {
        let mut file = DxfFile::<DxfShape>::new().unwrap();
        for (name, code, linetype) in
            [("CUT", Some(50), DxfLinetype::Dashed), ("ZERO", Some(0), DxfLinetype::Hidden), ("NOTES", None, DxfLinetype::Continuous)]
        {
            file.declare_layer(name, 3, linetype, code.map(|c| DxfStandardWeight::from_code(c).unwrap())).unwrap();
        }
        let output = write_dxf_file(&file).unwrap();
        let p = pairs(&output);
        let records = table_records(&p, "LAYER");
        assert_eq!(records.iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["0", "CUT", "ZERO", "NOTES"]);
        let weights: Vec<Vec<_>> = records.iter().map(|r| r.iter().filter(|(c, _)| *c == 370).map(|(_, v)| v.as_str()).collect()).collect();
        assert_eq!(weights, [vec!["-3"], vec!["50"], vec!["0"], vec!["-3"]]);
        // The internal omission stays distinct from physical zero. R2000 spells
        // omitted layer weights as -3 without changing the owned records.
        assert_eq!(file.layers().iter().map(|layer| layer.weight().map(|weight| weight.code())).collect::<Vec<_>>(), [None, Some(50), Some(0), None]);
        let objects = section_objects(&p, "OBJECTS");
        let (_, styles) = objects.iter().find(|(kind, _)| kind == "ACDBDICTIONARYWDFLT").unwrap();
        let normal = field(styles, 340);
        assert!(objects.iter().any(|(kind, object)| kind == "ACDBPLACEHOLDER" && field(object, 5) == normal));
        for record in &records {
            assert_eq!(field(record, 390), normal);
            assert_eq!(record.iter().filter(|(code, _)| *code == 390).count(), 1);
        }
        assert_eq!(records.iter().map(|r| field(r, 6)).collect::<Vec<_>>(), ["Continuous", "DASHED", "HIDDEN", "Continuous"]);
        assert!(table_records(&p, "LTYPE").iter().any(|r| field(r, 2) == "HIDDEN"));
        let layer_table = p.windows(2).position(|w| w[0] == (0, "TABLE".into()) && w[1] == (2, "LAYER".into())).unwrap();
        assert!(p[layer_table..].iter().take_while(|(c, v)| (*c, v.as_str()) != (0, "ENDTAB")).any(|pair| pair == &(70, "4".into())));
    }

    #[test]
    fn owned_entities_keep_every_stored_weight_distinct_from_inheritance() {
        let mut file = DxfFile::new().unwrap();
        let cut = file.declare_layer("CUT", 1, DxfLinetype::Continuous, Some(DxfStandardWeight::from_code(50).unwrap())).unwrap();
        for weight in [
            None,
            Some(DxfEntityWeight::ByLayer),
            Some(DxfEntityWeight::ByBlock),
            Some(DxfEntityWeight::Default),
            Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap())),
            Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(211).unwrap())),
        ] {
            file.push(cut, DxfShape::Line(Vec2::ZERO, Vec2::X), weight).unwrap();
        }
        let output = write_dxf_file(&file).unwrap();
        let records = entities(&output);
        let weights: Vec<Vec<_>> = records.iter().map(|r| r.1.iter().filter(|(c, _)| *c == 370).map(|(_, v)| v.as_str()).collect()).collect();
        assert_eq!(weights, [vec![], vec!["-1"], vec!["-2"], vec!["-3"], vec!["0"], vec!["211"]]);
        assert!(records.iter().all(|r| field(&r.1, 8) == "CUT"));
        assert_eq!(file.entities()[0].weight(), None);
        assert_eq!(file.entities()[1].weight(), Some(DxfEntityWeight::ByLayer));
    }

    #[test]
    fn rational_spline_weights_and_entity_lineweight_are_independent() {
        let mut file = DxfFile::new().unwrap();
        file.push(
            file.default_layer(),
            bezier_spline(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::X], Some(vec![1.0, 0.5, 1.0])),
            Some(DxfEntityWeight::ByBlock),
        )
        .unwrap();
        let output = write_dxf_file(&file).unwrap();
        let records = entities(&output);
        assert_eq!(records[0].0, "SPLINE");
        assert_eq!(values(&records[0], 41), [1.0, 0.5, 1.0]);
        assert_eq!(values(&records[0], 370), [-2.0]);
    }

    #[test]
    fn owned_geometry_refuses_nonfinite_fields_instead_of_zeroing_them() {
        for shape in [
            DxfShape::Line(Vec2::new(f64::NAN, 0.0), Vec2::X),
            DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::new(0.0, f64::INFINITY)] },
            DxfShape::Circle { c: Vec2::ZERO, r: f64::INFINITY },
            DxfShape::Arc { c: Vec2::ZERO, r: 1.0, a0: 0.0, a1: f64::NAN },
            DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::X, ratio: f64::NAN, t0: 0.0, t1: TAU },
            DxfShape::Spline { degree: 1, knots: vec![0.0, 0.0, f64::INFINITY, f64::INFINITY], ctrl: vec![Vec2::ZERO, Vec2::X], weights: None },
            bezier_spline(&[Vec2::ZERO, Vec2::X], Some(vec![1.0, f64::NAN])),
            DxfShape::Text { at: Vec2::ZERO, height: 1.0, angle_deg: f64::INFINITY, text: "A".into() },
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape, None).unwrap();
            let error = write_dxf_file(&file).unwrap_err().to_string();
            assert!(error.contains("DXF entity 1 on layer \"0\""), "{error}");
        }
    }

    #[test]
    fn owned_geometry_checks_structural_requirements_and_derived_extents() {
        for shape in [
            DxfShape::Polyline { points: vec![Vec2::ZERO] },
            DxfShape::Circle { c: Vec2::ZERO, r: -1.0 },
            DxfShape::Arc { c: Vec2::ZERO, r: 0.0, a0: 0.0, a1: 10.0 },
            DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::ZERO, ratio: 0.5, t0: 0.0, t1: TAU },
            DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::X, ratio: 2.0, t0: 0.0, t1: TAU },
            DxfShape::Spline { degree: 1, knots: vec![0.0, 0.0, 1.0], ctrl: vec![Vec2::ZERO, Vec2::X], weights: None },
            DxfShape::Spline { degree: 1, knots: vec![0.0, 1.0, 0.0, 1.0], ctrl: vec![Vec2::ZERO, Vec2::X], weights: None },
            bezier_spline(&[Vec2::ZERO, Vec2::X], Some(vec![1.0])),
            bezier_spline(&[Vec2::ZERO, Vec2::X], Some(vec![1.0, 0.0])),
            DxfShape::Text { at: Vec2::ZERO, height: 0.0, angle_deg: 0.0, text: "A".into() },
            DxfShape::Circle { c: Vec2::new(f64::MAX, 0.0), r: f64::MAX },
            DxfShape::Text { at: Vec2::ZERO, height: f64::MAX, angle_deg: 0.0, text: "AAAA".into() },
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape, None).unwrap();
            assert!(write_dxf_file(&file).is_err());
        }
    }

    #[test]
    fn owned_text_is_never_cut_and_only_characters_beyond_ffff_refuse() {
        for text in ["中".repeat(250), "A\tB\n%%d °".into(), "A".repeat(257), "中".repeat(2_000)] {
            let shape = DxfShape::Text { at: Vec2::ZERO, height: 1.0, angle_deg: 0.0, text };
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape.clone(), None).unwrap();
            assert_eq!(write_dxf_file(&file).unwrap(), write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).unwrap());
        }
        for text in ["😀".to_string(), format!("{}😀", "A".repeat(250))] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), DxfShape::Text { at: Vec2::ZERO, height: 1.0, angle_deg: 0.0, text }, None).unwrap();
            let error = write_dxf_file(&file).unwrap_err().to_string();
            assert!(error.contains("U+1F600") && error.contains("U+FFFF"), "{error}");
        }
        // 257 characters are MTEXT: one 250-byte chunk, then the tail.
        let shape = DxfShape::Text { at: Vec2::ZERO, height: 1.0, angle_deg: 0.0, text: "A".repeat(257) };
        let legacy = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).unwrap();
        let [(kind, groups)] = &entities(&legacy)[..] else { panic!("{legacy}") };
        assert_eq!((kind.as_str(), field(groups, 3), field(groups, 1)), ("MTEXT", "A".repeat(250).as_str(), "AAAAAAA"));
    }

    #[test]
    fn owned_work_budget_charges_curve_samples_and_spline_payload() {
        for (shape, work) in [
            (DxfShape::Arc { c: Vec2::ZERO, r: 1.0, a0: 0.0, a1: 90.0 }, 65),
            (DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::X, ratio: 0.5, t0: 0.0, t1: TAU }, 129),
            (bezier_spline(&[Vec2::ZERO, Vec2::new(1.0, 2.0), Vec2::X], Some(vec![1.0, 0.5, 1.0])), 12),
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape, None).unwrap();
            assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, work).is_ok());
            assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, work - 1).unwrap_err().to_string().contains("geometry payload"));
        }
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Arc { c: Vec2::ZERO, r: 1.0, a0: 0.0, a1: 90.0 }, None).unwrap();
        file.push(file.default_layer(), DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::X, ratio: 0.5, t0: 0.0, t1: TAU }, None).unwrap();
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 194).is_ok());
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 193).is_err());
    }

    #[test]
    fn owned_work_budget_is_admitted_before_large_geometry_is_scanned() {
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Polyline { points: vec![Vec2::new(f64::NAN, 0.0); 3] }, None).unwrap();
        let error = write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 2).unwrap_err().to_string();
        assert!(error.contains("geometry payload"), "{error}");
        let mut file = DxfFile::new().unwrap();
        for _ in 0..2 {
            file.push(file.default_layer(), DxfShape::Line(Vec2::ZERO, Vec2::X), None).unwrap();
        }
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 4).is_ok());
        assert!(write_dxf_file_limited(&file, crate::MAX_DRAWING_BYTES, 3).is_err());
    }

    #[test]
    fn owned_byte_budget_includes_header_and_body_at_the_exact_boundary() {
        let file = DxfFile::<DxfShape>::new().unwrap();
        let full = write_dxf_file(&file).unwrap();
        assert_eq!(write_dxf_file_limited(&file, full.len(), 0).unwrap(), full);
        assert!(write_dxf_file_limited(&file, full.len() - 1, 0).unwrap_err().to_string().contains("drawing-byte limit"));
        assert!(write_dxf_file_limited(&file, 0, 0).is_err());
    }

    #[test]
    fn bounded_out_keeps_first_error_and_never_repairs_nonfinite_values() {
        let mut out = Out::new(0x20, Some(0));
        out.code(0, "SECTION");
        let first = out.check().unwrap_err().to_string();
        out.num(40, f64::NAN);
        out.code(0, "EOF");
        assert_eq!(out.check().unwrap_err().to_string(), first);
        assert!(out.s.is_empty());
        let mut out = Out::new(0x20, Some(100));
        out.num(40, f64::NAN);
        assert!(out.check().unwrap_err().to_string().contains("non-finite output number"));
        assert!(out.s.is_empty());
        let mut out = Out::new(u64::MAX, Some(100));
        out.handle();
        assert!(out.check().unwrap_err().to_string().contains("handle space"));
    }

    #[test]
    fn bounded_and_legacy_numbers_have_identical_finite_spelling() {
        for value in [-0.0, -1e-15, 1e-15, -1.25, 2.0, 12345.6789012345, f64::MIN_POSITIVE, f64::MAX, -f64::MAX] {
            // Frozen legacy expression is a spelling compatibility check, not a geometry oracle.
            let expected = format!("{value:.12}");
            let expected = expected.trim_end_matches('0');
            let expected = if expected.ends_with('.') { format!("{expected}0") } else { expected.to_string() };
            let expected = if expected == "-0.0" { "0.0" } else { expected.as_str() };
            let mut old = Out::new(0, None);
            let mut bounded = Out::new(0, Some(1024));
            old.num(10, value);
            bounded.num(10, value);
            bounded.check().unwrap();
            assert_eq!(old.s, bounded.s);
            assert_eq!(bounded.s, format!(" 10\n{expected}\n"));
        }
    }

    #[test]
    fn owned_geometry_refuses_literal_positive_values_rounded_to_zero() {
        assert_eq!(number_value(1e-20).unwrap(), "0.0");
        let mut representable = DxfFile::new().unwrap();
        representable.push(representable.default_layer(), DxfShape::Circle { c: Vec2::ZERO, r: 1e-9 }, None).unwrap();
        assert_eq!(number_value(1e-12).unwrap(), "0.000000000001");
        assert_eq!(written_length(1e-9), Ok(1e-9));
        assert_eq!(written_length(1e-12), Ok(0.0));
        assert!(write_dxf_file(&representable).is_ok());
        for shape in [
            DxfShape::Circle { c: Vec2::ZERO, r: 1e-20 },
            DxfShape::Arc { c: Vec2::ZERO, r: 1e-20, a0: 0.0, a1: 90.0 },
            DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::X, ratio: 1e-20, t0: 0.0, t1: TAU },
            DxfShape::Text { at: Vec2::ZERO, height: 1e-20, angle_deg: 0.0, text: "A".into() },
            bezier_spline(&[Vec2::ZERO, Vec2::X], Some(vec![1.0, 1e-20])),
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape.clone(), None).unwrap();
            assert!(write_dxf_file(&file).unwrap_err().to_string().contains("as written"));
            // This stricter admission is exclusive to the owned API.
            assert!(write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).is_ok());
        }
    }

    #[test]
    fn owned_geometry_refuses_distinct_points_axes_and_knot_spans_that_collapse() {
        for shape in [
            DxfShape::Line(Vec2::ZERO, Vec2::new(1e-20, 0.0)),
            DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::new(1e-20, 0.0), Vec2::X] },
            DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::new(1e-20, 0.0), ratio: 0.5, t0: 0.0, t1: TAU },
            DxfShape::Arc { c: Vec2::ZERO, r: 1.0, a0: 0.0, a1: 1e-20 },
            DxfShape::Spline { degree: 1, ctrl: vec![Vec2::ZERO, Vec2::X], knots: vec![0.0, 0.0, 1e-20, 1e-20], weights: None },
            DxfShape::Spline {
                degree: 1,
                ctrl: vec![Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)],
                knots: vec![0.0, 0.0, 1e-20, 1.0, 1.0],
                weights: None,
            },
            bezier_spline(&[Vec2::ZERO, Vec2::new(1e-20, 0.0)], None),
        ] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape, None).unwrap();
            assert!(write_dxf_file(&file).unwrap_err().to_string().contains("as written"));
        }
    }

    #[test]
    fn legacy_layer_deduplication_stays_unchanged() {
        // A non-finite number is refused on this route too: see
        // a_length_outside_the_writable_range_or_not_finite_is_refused_naming_its_entity.
        let layers = [DxfLayer::new("0", 1), DxfLayer::new("中", 3), DxfLayer::new("中", 5), DxfLayer::new("cut", 1), DxfLayer::new("CUT", 2)];
        let output = write_dxf_drawing_units(&layers, &[DxfEntity { layer: " \t".into(), shape: DxfShape::Line(Vec2::ZERO, Vec2::X) }], 4).unwrap();
        let p = pairs(&output);
        let layers = table_records(&p, "LAYER");
        assert_eq!(layers.iter().map(|r| field(r, 2)).collect::<Vec<_>>(), ["0", "中", "cut", "CUT"]);
        assert_eq!(layers.iter().map(|r| field(r, 62)).collect::<Vec<_>>(), ["7", "3", "1", "2"]);
        assert!(layers.iter().all(|layer| field(layer, 370) == "-3"));
        let records = entities(&output);
        assert!(records.iter().all(|(_, entity)| !entity.iter().any(|(code, _)| *code == 370)));
        assert_eq!(field(&records[0].1, 8), "0");
        assert_eq!(values(&records[0], 10), [0.0]);
    }

    // PRIVATE append fragment for dxf_out.rs::tests. NOT APPLIED / NOT RUN.
    // Requires writer44b220e2 and W2 modeldf0d3a33; uses their production APIs
    // and the shared production parser's wire records plus field queries.

    #[test]
    fn customized_layer_zero_serializes_once_with_hidden_pattern_and_inherited_weight() {
        let physical = DxfStandardWeight::from_code(50).unwrap();
        let mut file = DxfFile::with_layer_zero(3, DxfLinetype::Hidden, Some(physical)).unwrap();
        let zero = file.default_layer();
        file.push(zero, DxfShape::Line(Vec2::ZERO, Vec2::new(2.0, 0.0)), None).unwrap();
        file.push(zero, DxfShape::Point(Vec2::new(3.0, 4.0)), Some(DxfEntityWeight::ByLayer)).unwrap();
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(physical));
        assert_eq!(file.effective_weight(1).unwrap(), DxfEntityWeight::Explicit(physical));
        assert_eq!(file.effective_linetype(0).unwrap(), DxfLinetype::Hidden);
        let output = write_dxf_file_with_text_bounds(&file, |_| panic!("LINE/POINT must not request TEXT metrics")).unwrap();
        let groups = pairs(&output);
        let layers = table_records(&groups, "LAYER");
        assert_eq!(layers.len(), 1);
        assert_eq!(layers.iter().filter(|record| field(record, 2) == "0").count(), 1);
        let layer = layers.first().unwrap();
        assert_eq!((field(layer, 2), field(layer, 62), field(layer, 6), field(layer, 370)), ("0", "3", "HIDDEN", "50"));
        let linetypes = table_records(&groups, "LTYPE");
        assert_eq!(linetypes.iter().map(|record| field(record, 2)).collect::<Vec<_>>(), ["ByBlock", "ByLayer", "Continuous", "DASHED", "HIDDEN"]);
        let hidden = linetypes.iter().find(|record| field(record, 2) == "HIDDEN").unwrap();
        assert_eq!((field(hidden, 40), field(hidden, 72), field(hidden, 73)), ("3.0", "65", "2"));
        assert_eq!(hidden.iter().filter(|(code, _)| *code == 49).map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["2.0", "-1.0"]);
        let records = entities(&output);
        assert_eq!(records.iter().map(|record| record.0.as_str()).collect::<Vec<_>>(), ["LINE", "POINT"]);
        assert_eq!(records.iter().map(|record| field(&record.1, 8)).collect::<Vec<_>>(), ["0", "0"]);
        assert_eq!(
            records
                .iter()
                .map(|record| record.1.iter().filter(|(code, _)| *code == 370).map(|(_, value)| value.as_str()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [vec![], vec!["-1"]]
        );
        assert!(records.iter().all(|record| field(&record.1, 6).is_empty() && field(&record.1, 62).is_empty()));
    }

    #[test]
    fn layer_zero_physical_zero_and_omission_keep_distinct_table_and_entity_spellings() {
        let zero = DxfStandardWeight::from_code(0).unwrap();
        for (layer_weight, layer_raw, inherited) in [(None, "-3", DxfEntityWeight::Default), (Some(zero), "0", DxfEntityWeight::Explicit(zero))] {
            let mut file = DxfFile::with_layer_zero(5, DxfLinetype::Dashed, layer_weight).unwrap();
            let override_weight = DxfStandardWeight::from_code(13).unwrap();
            let weights = [
                None,
                Some(DxfEntityWeight::ByLayer),
                Some(DxfEntityWeight::ByBlock),
                Some(DxfEntityWeight::Default),
                Some(DxfEntityWeight::Explicit(override_weight)),
            ];
            for (index, weight) in weights.into_iter().enumerate() {
                file.push(file.default_layer(), DxfShape::Point(Vec2::new(index as f64, 0.0)), weight).unwrap();
            }
            assert_eq!(file.effective_weight(0).unwrap(), inherited);
            assert_eq!(file.effective_weight(1).unwrap(), inherited);
            assert_eq!(file.effective_weight(2).unwrap(), DxfEntityWeight::ByBlock);
            assert_eq!(file.effective_weight(3).unwrap(), DxfEntityWeight::Default);
            assert_eq!(file.effective_weight(4).unwrap(), DxfEntityWeight::Explicit(override_weight));
            let output = write_dxf_file_with_text_bounds(&file, |_| panic!("POINT must not request TEXT metrics")).unwrap();
            let groups = pairs(&output);
            let layers = table_records(&groups, "LAYER");
            assert_eq!(layers.len(), 1);
            let layer = layers.first().unwrap();
            assert_eq!((field(layer, 2), field(layer, 62), field(layer, 6), field(layer, 370)), ("0", "5", "DASHED", layer_raw));
            assert_eq!(layer.iter().filter(|(code, _)| *code == 370).count(), 1);
            assert_eq!(file.layer(file.default_layer()).unwrap().weight(), layer_weight);
            assert!(!field(layer, 390).is_empty());
            let linetypes = table_records(&groups, "LTYPE");
            assert_eq!(linetypes.iter().filter(|record| field(record, 2) == "DASHED").count(), 1);
            assert!(!linetypes.iter().any(|record| field(record, 2) == "HIDDEN"));
            let dashed = linetypes.iter().find(|record| field(record, 2) == "DASHED").unwrap();
            assert_eq!((field(dashed, 40), field(dashed, 72), field(dashed, 73)), ("19.05", "65", "2"));
            assert_eq!(dashed.iter().filter(|(code, _)| *code == 49).map(|(_, value)| value.as_str()).collect::<Vec<_>>(), ["12.7", "-6.35"]);
            let records = entities(&output);
            assert_eq!(records.len(), 5);
            assert!(records.iter().all(|record| record.0 == "POINT" && field(&record.1, 8) == "0"));
            assert_eq!(
                records
                    .iter()
                    .map(|record| record.1.iter().filter(|(code, _)| *code == 370).map(|(_, value)| value.as_str()).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
                [vec![], vec!["-1"], vec!["-2"], vec!["-3"], vec!["13"]]
            );
        }
    }

    #[test]
    fn default_layer_zero_constructor_keeps_new_and_legacy_writer_bytes_identical() {
        let shapes = [DxfShape::Line(Vec2::ZERO, Vec2::new(2.0, 0.0)), DxfShape::Point(Vec2::new(3.0, 4.0))];
        let mut ordinary = DxfFile::new().unwrap();
        let mut explicit = DxfFile::with_layer_zero(7, DxfLinetype::Continuous, None).unwrap();
        for shape in &shapes {
            ordinary.push(ordinary.default_layer(), shape.clone(), None).unwrap();
            explicit.push(explicit.default_layer(), shape.clone(), None).unwrap();
        }
        let ordinary_output = write_dxf_file(&ordinary).unwrap();
        let explicit_output = write_dxf_file_with_text_bounds(&explicit, |_| panic!("LINE/POINT must not request TEXT metrics")).unwrap();
        let legacy: Vec<_> = shapes.into_iter().map(|shape| DxfEntity { layer: "0".into(), shape }).collect();
        assert_eq!(explicit_output, ordinary_output);
        assert_eq!(explicit_output, write_dxf_drawing_units(&[], &legacy, 4).unwrap());
        let groups = pairs(&explicit_output);
        let layers = table_records(&groups, "LAYER");
        assert_eq!(layers.len(), 1);
        let layer = layers.first().unwrap();
        assert_eq!((field(layer, 2), field(layer, 62), field(layer, 6), field(layer, 370)), ("0", "7", "Continuous", "-3"));
        assert_eq!(layer.iter().filter(|(code, _)| *code == 370).count(), 1);
        assert!(!field(layer, 390).is_empty());
        assert_eq!(ordinary.layer(ordinary.default_layer()).unwrap().weight(), None);
        assert_eq!(explicit.layer(explicit.default_layer()).unwrap().weight(), None);
        assert!(entities(&explicit_output).iter().all(|record| !record.1.iter().any(|(code, _)| *code == 370)));
        let linetypes = table_records(&groups, "LTYPE");
        assert_eq!(linetypes.iter().map(|record| field(record, 2)).collect::<Vec<_>>(), ["ByBlock", "ByLayer", "Continuous", "DASHED"]);
    }
    #[test]
    fn caller_profiles_preserve_tiny_and_zero_lines_without_relaxing_generic_admission() {
        for end in [Vec2::ZERO, Vec2::new(1e-14, 0.0)] {
            let shape = DxfShape::Line(Vec2::ZERO, end);
            let mut strict = DxfFile::new().unwrap();
            strict.push(strict.default_layer(), shape.clone(), None).unwrap();
            assert!(write_dxf_file(&strict).unwrap_err().to_string().contains("as written"));
            let mut view = DxfFile::for_current_view_export(1, 0, 0).unwrap();
            let layer = view.declare_layer("VISIBLE", 7, DxfLinetype::Continuous, None).unwrap();
            view.push(layer, shape.clone(), None).unwrap();
            let actual = write_dxf_file(&view).unwrap();
            let old = legacy_dxf_baseline(&[DxfLayer::new("VISIBLE", 7)], &[DxfEntity { layer: "VISIBLE".into(), shape }]).unwrap();
            assert_eq!(actual.as_bytes(), old.as_bytes());
            assert_eq!(entities(&actual).len(), 1);
            assert_eq!(values(&entities(&actual)[0], 11), [0.0]);
        }
    }

    #[test]
    fn immutable_view_plan_refuses_underdeclared_geometry_before_sampling() {
        let mut file = DxfFile::for_current_view_export(0, 1, 2).unwrap();
        let sketch = file.declare_layer("SKETCH", 5, DxfLinetype::Continuous, None).unwrap();
        file.push(sketch, DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::X, Vec2::new(f64::NAN, 0.0)] }, None).unwrap();
        let error = write_dxf_file(&file).unwrap_err().to_string();
        assert!(error.contains("immutable producer plan"), "{error}");
        assert_eq!(file.entities().len(), 1);
        assert!(DxfFile::for_current_view_export(1_000_001, 0, 0).is_err());
        assert!(DxfFile::for_current_view_export(0, 2, 3).is_err());
        assert!(DxfFile::for_current_view_export(usize::MAX, 1, 2).is_err());
        let mut empty = DxfFile::for_current_view_export(0, 0, 0).unwrap();
        assert!(empty.push(empty.default_layer(), DxfShape::Line(Vec2::ZERO, Vec2::X), None).is_err());
        assert!(empty.entities().is_empty());
    }

    #[test]
    fn producer_profiles_do_not_accept_foreign_shapes_styles_or_weights() {
        let mut view = DxfFile::for_current_view_export(1, 0, 0).unwrap();
        assert!(view.declare_layer("NOTES", 8, DxfLinetype::Continuous, None).is_err());
        assert!(view.declare_layer("VISIBLE", 8, DxfLinetype::Continuous, None).is_err());
        assert!(view.declare_layer("VISIBLE", 7, DxfLinetype::Dashed, None).is_err());
        assert_eq!(view.layers().len(), 1);
        let visible = view.declare_layer("VISIBLE", 7, DxfLinetype::Continuous, None).unwrap();
        view.push(visible, DxfShape::Circle { c: Vec2::ZERO, r: 1.0 }, None).unwrap();
        assert!(write_dxf_file(&view).unwrap_err().to_string().contains("declared producer"));
        let mut weighted = DxfFile::for_current_view_export(1, 0, 0).unwrap();
        let visible = weighted.declare_layer("VISIBLE", 7, DxfLinetype::Continuous, None).unwrap();
        weighted.push(visible, DxfShape::Line(Vec2::ZERO, Vec2::X), Some(DxfEntityWeight::ByLayer)).unwrap();
        assert!(write_dxf_file(&weighted).unwrap_err().to_string().contains("weight overrides"));
    }

    #[test]
    fn flat_profile_keeps_sorted_legacy_tables_and_authored_entity_order() {
        let mut file = DxfFile::for_flat_pattern_export(1, 1, 1).unwrap();
        let outline = file.declare_layer("OUTLINE", 7, DxfLinetype::Continuous, None).unwrap();
        let cut = file.declare_layer("CUT", 4, DxfLinetype::Continuous, None).unwrap();
        file.declare_layer("BEND_UP", 1, DxfLinetype::Dashed, None).unwrap();
        file.declare_layer("BEND_DOWN", 5, DxfLinetype::Dashed, None).unwrap();
        let perforation = file.declare_layer("PERFORATION", 3, DxfLinetype::Continuous, None).unwrap();
        let shapes =
            [DxfShape::Line(Vec2::ZERO, Vec2::X), DxfShape::Circle { c: Vec2::new(5.0, 5.0), r: 1.0 }, DxfShape::Line(Vec2::X, Vec2::new(1.0, 2.0))];
        file.push(cut, shapes[0].clone(), None).unwrap();
        file.push(perforation, shapes[1].clone(), None).unwrap();
        file.push(outline, shapes[2].clone(), None).unwrap();
        let before: Vec<_> = file.layers().iter().map(|l| l.name().to_string()).collect();
        let actual = write_dxf_file(&file).unwrap();
        let legacy_layers = [
            DxfLayer::new("BEND_DOWN", 5).dashed(),
            DxfLayer::new("BEND_UP", 1).dashed(),
            DxfLayer::new("CUT", 4),
            DxfLayer::new("OUTLINE", 7),
            DxfLayer::new("PERFORATION", 3),
        ];
        let legacy_entities = [
            DxfEntity { layer: "CUT".into(), shape: shapes[0].clone() },
            DxfEntity { layer: "PERFORATION".into(), shape: shapes[1].clone() },
            DxfEntity { layer: "OUTLINE".into(), shape: shapes[2].clone() },
        ];
        assert_eq!(actual.as_bytes(), legacy_dxf_baseline(&legacy_layers, &legacy_entities).unwrap().as_bytes());
        assert_eq!(before, file.layers().iter().map(|l| l.name().to_string()).collect::<Vec<_>>());
        assert_eq!(entities(&actual).iter().map(|e| field(&e.1, 8)).collect::<Vec<_>>(), ["CUT", "PERFORATION", "OUTLINE"]);
        let groups = pairs(&actual);
        assert_eq!(
            table_records(&groups, "LAYER").iter().map(|r| field(r, 2)).collect::<Vec<_>>(),
            ["0", "BEND_DOWN", "BEND_UP", "CUT", "OUTLINE", "PERFORATION"]
        );
        let tables: Vec<_> = groups
            .windows(2)
            .filter_map(|p| match p {
                [(0, a), (2, b)] if a == "TABLE" => Some(b.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tables, ["VPORT", "LTYPE", "LAYER", "STYLE", "VIEW", "UCS", "APPID", "DIMSTYLE", "BLOCK_RECORD"]);
        assert!(actual.contains("$DWGCODEPAGE\n"));
        assert!(actual.contains("ACAD_GROUP\n"));
    }

    #[test]
    fn flat_profile_charges_note_source_work_and_rejects_overflow_plans() {
        assert!(DxfFile::for_flat_pattern_export(usize::MAX, 0, 0).is_err());
        assert!(DxfFile::for_flat_pattern_export(0, usize::MAX, 0).is_err());
        let mut file = DxfFile::for_flat_pattern_export(0, 1, 0).unwrap();
        let layer = file.declare_layer("BEND_NOTES", 8, DxfLinetype::Continuous, None).unwrap();
        file.push(layer, DxfShape::Text { at: Vec2::ZERO, height: 3.0, angle_deg: 0.0, text: "a".repeat(2049) }, None).unwrap();
        assert!(write_dxf_file(&file).unwrap_err().to_string().contains("note text"));
        let mut wrong = DxfFile::for_flat_pattern_export(0, 1, 0).unwrap();
        let layer = wrong.declare_layer("BEND_UP", 1, DxfLinetype::Dashed, None).unwrap();
        wrong.push(layer, DxfShape::Circle { c: Vec2::ZERO, r: 1.0 }, None).unwrap();
        assert!(write_dxf_file(&wrong).unwrap_err().to_string().contains("declared producer"));
    }

    // A rolled flat's ROLL layer holds the roll's lines only, green and continuous, and a flat with
    // every named role and a pocket under each contour fits the plan's layer count.
    #[test]
    fn flat_profile_admits_roll_lines_on_their_own_layer() {
        let mut file = DxfFile::for_flat_pattern_export(1, 1, 0).unwrap();
        let roll = file.declare_layer("ROLL", 3, DxfLinetype::Continuous, None).unwrap();
        file.push(roll, DxfShape::Line(Vec2::ZERO, Vec2::X), None).unwrap();
        let written = write_dxf_file(&file).unwrap();
        assert_eq!(entities(&written).iter().map(|e| field(&e.1, 8)).collect::<Vec<_>>(), ["ROLL"]);

        let mut circle = DxfFile::for_flat_pattern_export(1, 1, 0).unwrap();
        let roll = circle.declare_layer("ROLL", 3, DxfLinetype::Continuous, None).unwrap();
        circle.push(roll, DxfShape::Circle { c: Vec2::ZERO, r: 1.0 }, None).unwrap();
        assert!(write_dxf_file(&circle).unwrap_err().to_string().contains("declared producer"));
        let mut dashed = DxfFile::for_flat_pattern_export(1, 1, 0).unwrap();
        let declared = dashed.declare_layer("ROLL", 3, DxfLinetype::Dashed, None);
        assert!(declared.unwrap_err().to_string().contains("declared producer"));

        let mut every = DxfFile::for_flat_pattern_export(1, 1, 1).unwrap();
        for (name, color, linetype) in [
            ("OUTLINE", 7, DxfLinetype::Continuous),
            ("CUT", 4, DxfLinetype::Continuous),
            ("TEXT", 2, DxfLinetype::Continuous),
            ("ETCH", 6, DxfLinetype::Continuous),
            ("BEND_UP", 1, DxfLinetype::Dashed),
            ("BEND_DOWN", 5, DxfLinetype::Dashed),
            ("BEND_NOTES", 8, DxfLinetype::Continuous),
            ("PERFORATION", 3, DxfLinetype::Continuous),
            ("ROLL", 3, DxfLinetype::Continuous),
            ("POCKET_TOP_Z0.000000_TO_1.000000_MM", 3, DxfLinetype::Continuous),
        ] {
            every.declare_layer(name, color, linetype, None).unwrap();
        }
        assert_eq!(every.layers().len(), 11);
        write_dxf_file(&every).unwrap();
    }

    /// A flat pattern's file with every role's standard layer and one entity of its kind on each,
    /// in the order of `DxfRole::ALL`.
    fn flat_with_every_role() -> DxfFile<DxfShape> {
        let neutral = DxfExportProfile::neutral();
        let mut file = DxfFile::for_flat_pattern_export(4, 4, 1).unwrap();
        for role in DxfRole::ALL {
            let style = neutral.role(role).unwrap();
            let name = if role == DxfRole::Pockets { format!("{}_TOP_Z0.000000_TO_1.000000_MM", style.layer) } else { style.layer.clone() };
            // The neutral profile holds what the producer's contract admits, or this is refused.
            let layer = file.declare_layer(&name, style.color, style.linetype, None).unwrap();
            let shape = match role {
                DxfRole::BendNotes => DxfShape::Text { at: Vec2::ZERO, height: 3.0, angle_deg: 0.0, text: "B1 UP 90° R1".into() },
                DxfRole::Perforation => DxfShape::Circle { c: Vec2::ZERO, r: 1.0 },
                _ => DxfShape::Line(Vec2::ZERO, Vec2::new(10.0, 0.0)),
            };
            file.push(layer, shape, None).unwrap();
        }
        file
    }

    /// The LAYER table as (name, colour, linetype), in the file's order.
    fn layer_rows(dxf: &str) -> Vec<(String, String, String)> {
        table_records(&pairs(dxf), "LAYER").iter().map(|r| (field(r, 2).to_string(), field(r, 62).to_string(), field(r, 6).to_string())).collect()
    }

    fn profile(roles: serde_json::Value) -> Result<DxfExportProfile> {
        DxfExportProfile::from_json(&serde_json::json!({"format": "solidcraft-dxf-profile", "version": 1, "roles": roles}))
    }

    #[test]
    fn the_neutral_dxf_profile_writes_the_bytes_of_no_profile() {
        let file = flat_with_every_role();
        let plain = write_dxf_file(&file).unwrap();
        assert_eq!(write_dxf_file_as(&file, &DxfExportProfile::neutral()).unwrap(), plain);
        // A profile that names no role, and the neutral profile read back from its own file, are
        // the neutral profile.
        assert_eq!(write_dxf_file_as(&file, &profile(serde_json::json!({})).unwrap()).unwrap(), plain);
        let neutral = DxfExportProfile::neutral();
        assert_eq!(DxfExportProfile::read(neutral.to_json().to_string().as_bytes()).unwrap(), neutral);
        // The format page prints the neutral profile in full: its first JSON block is that profile.
        let page = include_str!("../../../docs/formats/dxf-profiles.md");
        let example = page.split("```json").nth(1).and_then(|rest| rest.split("```").next()).unwrap();
        assert_eq!(DxfExportProfile::read(example.as_bytes()).unwrap(), neutral);
        // The page's second block is a shop's profile, and reads.
        let shop = page.split("```json").nth(2).and_then(|rest| rest.split("```").next()).unwrap();
        assert_ne!(DxfExportProfile::read(shop.as_bytes()).unwrap(), neutral);
    }

    #[test]
    fn a_dxf_profile_renames_restyles_shares_and_leaves_out_layers() {
        let file = flat_with_every_role();
        let shop = profile(serde_json::json!({
            "outline": {"layer": "CUT", "color": 7},
            "cut": {"color": 7},
            "cut_text": {"layer": "#LETTERS"},
            "bend_up": {"layer": "FOLD", "color": 3, "linetype": "Hidden"},
            "bend_down": {"layer": "fold", "color": 3, "linetype": "hidden"},
            "bend_notes": {"on": false},
            "pockets": {"layer": "MILL", "color": 6, "linetype": "dashed"},
            "roll": {"on": false, "layer": "UNUSED", "color": 200}
        }))
        .unwrap();
        let dxf = write_dxf_file_as(&file, &shop).unwrap();
        // 0 first, then by name; outline and cut share CUT, both bend directions share FOLD (layer
        // names ignore case), the notes and the roll have no layer, and what the profile leaves
        // alone (ETCH, PERFORATION, the letters' colour) is as it was.
        let row = |name: &str, color: &str, linetype: &str| (name.to_string(), color.to_string(), linetype.to_string());
        assert_eq!(
            layer_rows(&dxf),
            [
                row("0", "7", "Continuous"),
                row("#LETTERS", "2", "Continuous"),
                row("CUT", "7", "Continuous"),
                row("ETCH", "6", "Continuous"),
                row("FOLD", "3", "HIDDEN"),
                row("MILL_TOP_Z0.000000_TO_1.000000_MM", "6", "DASHED"),
                row("PERFORATION", "3", "Continuous"),
            ]
        );
        // The entities, in the file's order, each on its role's layer; the note and the roll's line
        // are not written.
        let written = entities(&dxf);
        assert_eq!(
            written.iter().map(|e| (e.0.as_str(), field(&e.1, 8))).collect::<Vec<_>>(),
            [
                ("LINE", "CUT"),
                ("LINE", "CUT"),
                ("LINE", "#LETTERS"),
                ("LINE", "ETCH"),
                ("LINE", "FOLD"),
                ("LINE", "fold"),
                ("LINE", "MILL_TOP_Z0.000000_TO_1.000000_MM"),
                ("CIRCLE", "PERFORATION"),
            ]
        );
        // The linetypes the layers name are defined in the file.
        let types = table_records(&pairs(&dxf), "LTYPE");
        let types: Vec<&str> = types.iter().map(|r| field(r, 2)).collect();
        assert!(types.contains(&"HIDDEN") && types.contains(&"DASHED"), "{types:?}");
        assert_eq!(crate::read_dxf_file(&dxf).unwrap().entities().len(), 8);

        // Every role off: a file with layer 0 only.
        let off: serde_json::Map<String, serde_json::Value> =
            DxfRole::ALL.iter().map(|r| (r.key().to_string(), serde_json::json!({"on": false}))).collect();
        let empty = write_dxf_file_as(&file, &profile(off.into()).unwrap()).unwrap();
        assert_eq!(layer_rows(&empty), [row("0", "7", "Continuous")]);
        assert!(entities(&empty).is_empty());
    }

    #[test]
    fn a_dxf_profile_still_checks_the_flat_pattern_and_styles_nothing_else() {
        // A shape that is not its role's is refused before the profile is looked at, even when
        // the profile leaves that role out.
        let mut wrong = DxfFile::for_flat_pattern_export(0, 1, 0).unwrap();
        let layer = wrong.declare_layer("BEND_UP", 1, DxfLinetype::Dashed, None).unwrap();
        wrong.push(layer, DxfShape::Circle { c: Vec2::ZERO, r: 1.0 }, None).unwrap();
        let off = profile(serde_json::json!({"bend_up": {"on": false}})).unwrap();
        assert!(write_dxf_file_as(&wrong, &off).unwrap_err().to_string().contains("declared producer"));
        // The current view's file and an ordinary file have no roles.
        let neutral = DxfExportProfile::neutral();
        for file in [DxfFile::for_current_view_export(0, 0, 0).unwrap(), DxfFile::new().unwrap()] {
            assert!(write_dxf_file_as(&file, &neutral).unwrap_err().to_string().contains("flat pattern"));
        }
        // A pocket layer and a plain role's layer that come to one name must agree on its style.
        let file = flat_with_every_role();
        let clash = profile(serde_json::json!({"pockets": {"layer": "M"}, "cut": {"layer": "M_TOP_Z0.000000_TO_1.000000_MM"}})).unwrap();
        assert!(write_dxf_file_as(&file, &clash).unwrap_err().to_string().contains("share a layer"));
    }

    #[test]
    fn a_dxf_profile_that_cannot_be_used_is_refused_and_says_why() {
        use serde_json::json;
        for (given, why) in [
            (json!([]), "a profile is a JSON object"),
            (json!({}), "`format` must be \"solidcraft-dxf-profile\""),
            (json!({"format": "solidcraft-sheet-rules"}), "`format` must be"),
            (json!({"format": "solidcraft-dxf-profile", "version": 2}), "the profile is version 2; this SolidCraft reads version 1"),
            (json!({"format": "solidcraft-dxf-profile", "version": "1"}), "`version` must be 1"),
            (json!({"format": "solidcraft-dxf-profile", "units": "inch"}), "`units` is not known"),
            (json!({"format": "solidcraft-dxf-profile", "name": "a\nb"}), "`name` must be text of at most 128 characters on one line"),
            (json!({"format": "solidcraft-dxf-profile", "roles": []}), "`roles` must be an object"),
        ] {
            let error = DxfExportProfile::from_json(&given).unwrap_err().to_string();
            assert!(error.starts_with("DXF profile: ") && error.contains(why), "{given}: {error}");
        }
        for (roles, why) in [
            (
                json!({"bend_sideways": {}}),
                "role `bend_sideways` is not known; the roles are outline, cut, cut_text, etch, bend_up, bend_down, bend_notes, pockets, perforation, roll",
            ),
            (json!({"OUTLINE": {}}), "role `OUTLINE` is not known"),
            (json!({"cut": true}), "role `cut` must be an object"),
            (json!({"cut": {"lineweight": 0.25}}), "role `cut`: `lineweight` is not known; a role has on, layer, color and linetype"),
            (json!({"cut": {"on": "no"}}), "role `cut`: `on` must be true or false"),
            (json!({"cut": {"color": 0}}), "role `cut`: `color` must be an AutoCAD colour index, a whole number from 1 to 255, not 0"),
            (json!({"cut": {"color": 256}}), "not 256"),
            (json!({"cut": {"color": 1.5}}), "not 1.5"),
            (json!({"cut": {"color": "red"}}), "not \"red\""),
            (json!({"cut": {"color": [255, 0, 0]}}), "role `cut`: `color` must be"),
            (json!({"bend_up": {"linetype": "center"}}), "role `bend_up`: `linetype` must be continuous, dashed or hidden, not \"center\""),
            (json!({"bend_up": {"linetype": 2}}), "role `bend_up`: `linetype` must be"),
            (json!({"etch": {"layer": ""}}), "role `etch`: `layer` must be 1 to 200 characters"),
            (json!({"etch": {"layer": "M".repeat(201)}}), "role `etch`: `layer` must be 1 to 200 characters"),
            (json!({"etch": {"layer": 7}}), "role `etch`: `layer` must be text"),
            (json!({"etch": {"layer": "0"}}), "role `etch`: `layer` cannot be 0"),
            (json!({"etch": {"layer": "MARK/ETCH"}}), "role `etch`: layer \"MARK/ETCH\" cannot be a DXF layer name"),
            (json!({"etch": {"layer": " MARK"}}), "cannot be a DXF layer name"),
            (json!({"etch": {"layer": "GRAVÉ"}}), "cannot be a DXF layer name"),
            (json!({"etch": {"layer": "A\nB"}}), "cannot be a DXF layer name"),
            (
                json!({"bend_up": {"layer": "FOLD"}, "bend_down": {"layer": "Fold"}}),
                "roles `bend_up` and `bend_down` share layer \"FOLD\" but give it different colours or linetypes",
            ),
        ] {
            let error = profile(roles.clone()).unwrap_err().to_string();
            assert!(error.starts_with("DXF profile: ") && error.contains(why), "{roles}: {error}");
        }
        // A role that is off may name a layer another role styles differently: it has no layer.
        profile(json!({"bend_up": {"layer": "FOLD"}, "bend_down": {"layer": "FOLD", "on": false}})).unwrap();
        // A file: UTF-8 JSON (a byte order mark is passed over), at most 1 MiB.
        let neutral = DxfExportProfile::neutral();
        let text = format!("\u{feff}{}", neutral.to_json());
        assert_eq!(DxfExportProfile::read(text.as_bytes()).unwrap(), neutral);
        assert!(DxfExportProfile::read(&[0xff, 0xfe]).unwrap_err().to_string().contains("not UTF-8"));
        assert!(DxfExportProfile::read(b"{format").unwrap_err().to_string().contains("not valid JSON"));
        assert!(DxfExportProfile::read(&vec![b' '; MAX_DXF_PROFILE_BYTES + 1]).unwrap_err().to_string().contains("at most 1048576 bytes"));
    }

    #[test]
    fn legacy_baseline_text_feature_preserves_cap_and_far_unicode_error_with_bounded_work() {
        let layers = [DxfLayer::new("NOTES", 8)];
        let note = |text| DxfEntity { layer: "NOTES".into(), shape: DxfShape::Text { at: Vec2::ZERO, height: 3.0, angle_deg: 0.0, text } };
        let output = legacy_dxf_baseline(&layers, &[note("\u{ffff}".repeat(251))]).unwrap();
        assert_eq!(field(&entities(&output)[0].1, 1), "\\U+FFFF".repeat(251), "nothing is cut");
        let error = legacy_dxf_baseline(&layers, &[note(format!("{}\u{2000b}", "中".repeat(10_000)))]).unwrap_err().to_string();
        assert!(error.contains("U+2000B"));
        assert!(error.contains("at position 10001"), "{error}");
        assert!(legacy_dxf_baseline(&layers, &[note("a".repeat(10_002))]).unwrap_err().to_string().contains("10,001"));
    }
    #[test]
    fn current_view_arc_plan_retains_original_line_plan_limits() {
        let arc = DxfShape::Arc { c: Vec2::ZERO, r: 5.0, a0: 0.0, a1: 90.0 };
        let mut file = DxfFile::for_current_view_arc_export(4, 0, 0).unwrap();
        let visible = file.declare_layer("VISIBLE", 7, DxfLinetype::Continuous, None).unwrap();
        file.push(visible, arc.clone(), None).unwrap();
        let expected =
            write_dxf_drawing_units(&[DxfLayer::new("VISIBLE", 7)], &[DxfEntity { layer: "VISIBLE".into(), shape: arc.clone() }], 4).unwrap();
        assert_eq!(write_dxf_file(&file).unwrap().as_bytes(), expected.as_bytes());
        let mut lines = DxfFile::for_current_view_export(4, 0, 0).unwrap();
        let visible = lines.declare_layer("VISIBLE", 7, DxfLinetype::Continuous, None).unwrap();
        lines.push(visible, arc, None).unwrap();
        assert!(write_dxf_file(&lines).unwrap_err().to_string().contains("geometry does not belong"));
        assert!(DxfFile::for_current_view_arc_export(1_000_001, 0, 0).is_err());
    }

    #[test]
    fn owned_current_lwpolyline_keeps_upstream_wire_representation() {
        let shape = DxfShape::LwPolyline { verts: vec![(Vec2::new(-1.0, 0.0), 1.0), (Vec2::new(1.0, 0.0), 1.0)], closed: true };
        let mut file = DxfFile::new().unwrap();
        let cut = file.declare_layer("CUT", 4, DxfLinetype::Continuous, None).unwrap();
        file.push(cut, shape.clone(), None).unwrap();
        let expected = write_dxf_drawing_units(&[DxfLayer::new("CUT", 4)], &[DxfEntity { layer: "CUT".into(), shape }], 4).unwrap();
        assert_eq!(write_dxf_file(&file).unwrap(), expected);
        let copied = file.translated_scaled(Vec2::new(3.0, 4.0), 2.0).unwrap();
        assert_eq!(
            copied.entities()[0].geometry(),
            &DxfShape::LwPolyline { verts: vec![(Vec2::new(1.0, 4.0), 1.0), (Vec2::new(5.0, 4.0), 1.0)], closed: true }
        );
    }
    #[test]
    fn flat_profile_167_rectangles_do_not_overflow_a_32_bit_work_estimate() {
        // 167 * 200_000 * 129 = 4_308_600_000 exceeds u32::MAX,
        // although these contours contain only 668 actual line entities.
        assert!(167u64 * 200_000 * 129 > u64::from(u32::MAX));
        let mut file = DxfFile::for_flat_pattern_export(167, 0, 0).unwrap();
        let layer = file.declare_layer("OUTLINE", 7, DxfLinetype::Continuous, None).unwrap();
        for i in 0..167 {
            let x = f64::from(i) * 2.0;
            let points = [Vec2::new(x, 0.0), Vec2::new(x + 1.0, 0.0), Vec2::new(x + 1.0, 1.0), Vec2::new(x, 1.0), Vec2::new(x, 0.0)];
            for pair in points.windows(2) {
                file.push(layer, DxfShape::Line(pair[0], pair[1]), None).unwrap();
            }
        }
        let output = write_dxf_file(&file).unwrap();
        assert_eq!(entities(&output).len(), 668);
        assert!(output.len() < 1_000_000);
    }

    #[test]
    fn lengths_are_written_on_the_grid_and_dimensionless_values_at_twelve_decimals() {
        // A third has digits past both rules, so each field's decimals show which rule wrote it.
        let third = 1.0 / 3.0;
        let p = Vec2::new(10.0 + third, 20.0 + third);
        let q = p + Vec2::new(5.0, 0.0);
        let shapes: [(DxfShape, &[i32], &[i32]); 10] = [
            (DxfShape::Point(p), &[10, 20], &[]),
            (DxfShape::Line(p, q), &[10, 20, 11], &[]),
            (DxfShape::Polyline { points: vec![p, q] }, &[10, 20], &[]),
            (DxfShape::LwPolyline { verts: vec![(p, third), (q, 0.0)], closed: false }, &[10, 20], &[42]),
            (
                DxfShape::BulgedPolyline { vertices: vec![DxfVertex { at: p, bulge: third }, DxfVertex { at: q, bulge: 0.0 }], closed: false },
                &[10, 20],
                &[],
            ),
            (DxfShape::Circle { c: p, r: 1.0 + third }, &[10, 20, 40], &[]),
            (DxfShape::Arc { c: p, r: 1.0 + third, a0: third, a1: 90.0 + third }, &[10, 20, 40], &[50, 51]),
            (DxfShape::Ellipse { c: p, major: Vec2::new(2.0 + third, 0.0), ratio: third, t0: third, t1: PI }, &[10, 20, 11], &[40, 41]),
            (bezier_spline(&[p, q], Some(vec![1.0, 1.0 + third])), &[10, 20], &[]),
            (DxfShape::Text { at: p, height: 2.0 + third, angle_deg: third, text: "A".into() }, &[10, 20, 40, 11, 21], &[50]),
        ];
        let decimals = |text: &str| text.split_once('.').map_or(0, |(_, digits)| digits.len());
        for (shape, lengths, numbers) in shapes {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape.clone(), None).unwrap();
            let output = write_dxf_file(&file).unwrap();
            let records = entities(&output);
            let record = &records.first().unwrap().1;
            for (codes, places) in [(lengths, 9), (numbers, 12)] {
                for code in codes {
                    let first = record.iter().find(|(c, _)| c == code).map(|(_, value)| value.as_str());
                    assert_eq!(first.map(decimals), Some(places), "{shape:?}: group {code} is {first:?}");
                }
            }
            // The header's extents are lengths too.
            for name in ["$EXTMIN", "$EXTMAX"] {
                let mut lines = output.lines().skip_while(|line| *line != name);
                let (x, y) = (lines.nth(2).unwrap(), lines.nth(1).unwrap());
                assert!(decimals(x) <= 9 && decimals(y) <= 9, "{shape:?}: {name} {x} {y}");
            }
        }
        // A bulge, a knot and a weight stay dimensionless.
        let bulged = DxfShape::BulgedPolyline { vertices: vec![DxfVertex { at: p, bulge: third }, DxfVertex { at: q, bulge: 0.0 }], closed: false };
        let rational = bezier_spline(&[p, q], Some(vec![1.0, 1.0 + third]));
        for (shape, code, expected) in [(bulged, 42, "0.333333333333"), (rational, 41, "1.333333333333")] {
            let mut file = DxfFile::new().unwrap();
            file.push(file.default_layer(), shape, None).unwrap();
            let output = write_dxf_file(&file).unwrap();
            assert!(entities(&output)[0].1.iter().any(|(c, value)| *c == code && value == expected), "{output}");
        }
    }

    #[test]
    fn a_length_outside_the_writable_range_or_not_finite_is_refused_naming_its_entity() {
        let far = DxfShape::Circle { c: Vec2::new(9_000_001.0, 0.0), r: 1.0 };
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), far.clone(), None).unwrap();
        let error = write_dxf_file(&file).unwrap_err().to_string();
        assert!(error.contains("entity 1") && error.contains("9000000"), "{error}");
        // The legacy route no longer writes a non-finite or out-of-range length as a number.
        for shape in [far, DxfShape::Line(Vec2::new(f64::NAN, 0.0), Vec2::X)] {
            let error = write_dxf_drawing_units(&[], &[DxfEntity { layer: "0".into(), shape }], 4).unwrap_err().to_string();
            assert!(error.contains("CIRCLE entity 1") || error.contains("LINE entity 1"), "{error}");
        }
        let mut out = Out::new(0x20, None);
        out.len(10, 9_000_000.0);
        assert_eq!(out.s, " 10\n9000000.0\n");
        out.len(10, f64::INFINITY);
        assert!(out.check().unwrap_err().to_string().contains("finite"));
    }

    #[test]
    fn path_admission_and_the_writer_agree_at_half_a_tick() {
        // Just under half a tick, both ends of a line are written as 0.0: the path converter
        // refuses it as the writer's own admission does. Just over, it is one tick long.
        let under = 0.5e-9_f64.next_down();
        let over = 0.5e-9_f64.next_up();
        let line = |x: f64| Seg2::Line { a: Vec2::ZERO, b: Vec2::new(x, 0.0) };
        assert!(path_shapes(DxfPath::Open(&[line(under)]), DxfPathStyle::Exact).unwrap_err().to_string().contains("collapse"));
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::Line(Vec2::ZERO, Vec2::new(under, 0.0)), None).unwrap();
        assert!(write_dxf_file(&file).is_err());
        let shapes = path_shapes(DxfPath::Open(&[line(over)]), DxfPathStyle::Exact).unwrap();
        let mut file = DxfFile::new().unwrap();
        for shape in shapes {
            file.push(file.default_layer(), shape, None).unwrap();
        }
        assert_eq!(values(&entities(&write_dxf_file(&file).unwrap())[0], 11), [1e-9]);
        // A bulged polyline's vertices that meet on the grid are refused, not written as one point.
        let vertices = vec![
            DxfVertex { at: Vec2::ZERO, bulge: 0.0 },
            DxfVertex { at: Vec2::new(under, 0.0), bulge: 0.0 },
            DxfVertex { at: Vec2::X, bulge: 0.0 },
        ];
        let mut file = DxfFile::new().unwrap();
        file.push(file.default_layer(), DxfShape::BulgedPolyline { vertices, closed: false }, None).unwrap();
        assert!(write_dxf_file(&file).unwrap_err().to_string().contains("distinct point"));
    }
}
