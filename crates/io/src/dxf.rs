//! ASCII DXF (the public AutoCAD DXF reference): the ENTITIES section's LINE, CIRCLE, ARC,
//! LWPOLYLINE, POLYLINE/VERTEX, POINT, ELLIPSE, SPLINE, TEXT and MTEXT, scaled to millimetres by
//! `$INSUNITS`. Block references (INSERT, with scale, rotation and arrays) are expanded,
//! nested blocks too; paper-space entities are skipped. The writer emits an R12-style file of lines, circles,
//! arcs and points (free-form curves as polylines); the flat pattern's drawing, with ELLIPSE and SPLINE, is
//! [`crate::write_dxf_drawing`].

use std::collections::HashMap;

use solidcraft_geom::{MAX_BEZIER_DEGREE, Vec2};

use crate::sketch2d::{Geom2, MAX_DRAWING_BYTES, MAX_ENTITIES, bulge_arc};
// Shared X2 model; the partial reader below consumes the real metadata API.
use crate::{DxfEntityWeight, DxfFile, DxfLayerId, DxfLinetype, DxfShape, DxfStandardWeight, IoError, Result};

mod text_entity;
mod text_value;

/// One group-code parser: text values1/3 stay raw; other values normalize.
/// Legacy Ent::text access retains its historical trim. Layered callers read raw Ent::s.
fn pairs(text: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        let Ok(code) = c.trim().parse::<i32>() else { continue };
        let value = if matches!(code, 1 | 3) { v } else { v.trim() };
        out.push((code, value.to_string()));
    }
    out
}

fn units_scale(p: &[(i32, String)]) -> f64 {
    // $INSUNITS: 1 in, 2 ft, 4 mm, 5 cm, 6 m, 0 unitless (taken as mm).
    let i = p.iter().position(|(c, v)| *c == 9 && v == "$INSUNITS");
    match i.and_then(|i| p.get(i + 1)).and_then(|(_, v)| v.parse::<i32>().ok()) {
        Some(1) => 25.4,
        Some(2) => 304.8,
        Some(5) => 10.0,
        Some(6) => 1000.0,
        _ => 1.0,
    }
}

#[derive(Default)]
struct Ent {
    kind: String,
    f: Vec<(i32, f64)>,
    s: Vec<(i32, String)>,
}

impl Ent {
    fn get(&self, code: i32) -> Option<f64> {
        self.f.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }
    fn all(&self, code: i32) -> Vec<f64> {
        self.f.iter().filter(|(c, _)| *c == code).map(|(_, v)| *v).collect()
    }
    fn text(&self, code: i32) -> Option<&str> {
        self.s.iter().find(|(c, _)| *c == code).map(|(_, v)| v.trim())
    }
}

/// Layered-reader adapter over the EXISTING parsed entity. This does
/// not resolve layer/block defaults, create layers or inspect group6. The shared
/// X2 aliases are used by the production partial layered reader below.
fn layered_entity_weight(entity: &Ent) -> Result<Option<DxfEntityWeight>> {
    // Count both storage classes: a malformed present field is not omission,
    // and a numeric/text duplicate must not become a first-field-wins result.
    // This scans existing fields only, without allocation or a second parser.
    let mut fields = entity
        .f
        .iter()
        .filter(|(code, _)| *code == 370)
        .map(|(_, value)| Some(*value))
        .chain(entity.s.iter().filter(|(code, _)| *code == 370).map(|_| None));
    let Some(raw) = fields.next() else { return Ok(None) };
    if fields.next().is_some() {
        return Err(IoError::Invalid("duplicate DXF entity lineweight field".into()));
    }
    let value = raw.ok_or_else(|| IoError::Invalid("invalid DXF entity lineweight field".into()))?;
    let weight = if value == -1.0 {
        DxfEntityWeight::ByLayer
    } else if value == -2.0 {
        DxfEntityWeight::ByBlock
    } else if value == -3.0 {
        DxfEntityWeight::Default
    } else {
        // Raw370 is an enum code, not a millimetre length to snap. Domain and
        // integral checks precede the unsigned cast; from_code enforces X2's
        // standard table. In particular, do not route symbols through from_mm.
        if !value.is_finite() || !(0.0..=211.0).contains(&value) || value.fract() != 0.0 {
            return Err(IoError::Invalid("invalid DXF entity lineweight code".into()));
        }
        DxfEntityWeight::Explicit(DxfStandardWeight::from_code(value as u16)?)
    };
    Ok(Some(weight))
}

/// Read a deliberately partial layered ASCII DXF into the shared owned model.
///
/// POINT/LINE/CIRCLE/ARC/ELLIPSE, admitted straight/bulged LWPOLYLINE, clamped SPLINE
/// and nine-alignment TEXT retain
/// entity order, declared ASCII layers and raw lineweights. Coordinates/radii
/// scale to millimetres once; spline knots and weights stay dimensionless;
/// stored ARC angles remain degrees, as required by DxfShape. Valid paper space
/// (group67=1) is skipped before interpreting geometry as in the legacy reader;
/// malformed space flags are refused instead of silently classifying an entity.
/// INSERT and all other geometry return an explicit unsupported error. This
/// does not replace read_dxf or claim the complete #5320 import contract.
pub fn read_dxf_file(text: &str) -> Result<DxfFile<DxfShape>> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the DXF file is too large".into()));
    }
    if text.starts_with("AutoCAD Binary DXF") {
        return Err(IoError::Invalid("binary DXF is not supported; save as ASCII DXF".into()));
    }
    let p = pairs(text);
    let start = layered_section_start(&p, "ENTITIES")?.ok_or_else(|| IoError::Invalid("no ENTITIES section".into()))?;
    layered_integer_fields(&p, start, &[67])?;
    // Includes TEXT71/72/73: lexical short admission precedes section/f64.
    layered_array_integer_fields(&p, start)?;
    let k = layered_units_scale(&p)?;
    let mut work = 0;
    let (mut file, handles) = if let Some(tables) = layered_section_start(&p, "TABLES")? {
        layered_integer_fields(&p, tables, &[70, 72, 73, 74])?;
        let records = section(&p, tables)?;
        layered_tables(&records, k, &mut work)?
    } else {
        layered_tables(&[], k, &mut work)?
    };
    let mut geometry_bytes = 0;

    for entity in section(&p, start)? {
        layered_work(&mut work)?;
        let space = layered_number(&entity, 67)?.unwrap_or(0.0);
        if space == 1.0 {
            continue;
        }
        if space != 0.0 {
            return Err(IoError::Invalid("invalid layered DXF space flag; expected 0 or 1".into()));
        }
        if !matches!(entity.kind.as_str(), "TEXT" | "MTEXT") {
            layered_entity_fields(&entity)?;
        }
        let name = layered_text(&entity, 8)?.filter(|name| !name.is_empty()).unwrap_or("0");
        if !name.is_ascii() || name.len() > 255 {
            return Err(IoError::Invalid("not supported yet: non-ASCII or overlong DXF layer reference".into()));
        }
        let mut selected = None;
        for handle in &handles {
            layered_work(&mut work)?;
            if file.layer(*handle)?.name().eq_ignore_ascii_case(name) {
                selected = Some(*handle);
                break;
            }
        }
        let layer = selected.ok_or_else(|| IoError::Invalid("DXF entity references an undeclared layer".into()))?;
        let geometry = if matches!(entity.kind.as_str(), "TEXT" | "MTEXT") {
            text_entity::read_text_entity(&entity, k, 1750, 750, &mut geometry_bytes, &mut work)?
        } else {
            layered_geometry(&entity, k, file.layer(layer)?.linetype(), &mut geometry_bytes, &mut work)?
        };
        file.push(layer, geometry, layered_entity_weight(&entity)?)?;
    }
    Ok(file)
}

fn layered_work(work: &mut usize) -> Result<()> {
    *work = work.saturating_add(1);
    if *work > MAX_WORK {
        return Err(IoError::Invalid("the layered DXF drawing is too complex".into()));
    }
    Ok(())
}

/// Locate real SECTION/name pairs, rather than matching a block/layer named
/// ENTITIES. Use the existing parsed pairs; no second group-code parser.
fn layered_section_start(p: &[(i32, String)], name: &str) -> Result<Option<usize>> {
    let mut found = None;
    for (index, pair) in p.iter().enumerate() {
        if pair.0 != 0 || pair.1 != "SECTION" {
            continue;
        }
        let Some((2, section_name)) = p.get(index + 1) else {
            return Err(IoError::Invalid("DXF SECTION has no name".into()));
        };
        if section_name != name {
            continue;
        }
        if found.is_some() {
            return Err(IoError::Invalid("duplicate layered DXF section".into()));
        }
        found = Some(index + 1);
        let terminal = p.iter().skip(index + 2).find(|(code, value)| *code == 0 && matches!(value.as_str(), "ENDSEC" | "EOF" | "SECTION"));
        if !terminal.is_some_and(|(_, value)| value == "ENDSEC") {
            return Err(IoError::Invalid("unterminated layered DXF section".into()));
        }
    }
    Ok(found)
}

fn layered_units_scale(p: &[(i32, String)]) -> Result<f64> {
    let Some(header_start) = layered_section_start(p, "HEADER")? else {
        return Ok(1.0);
    };
    let header = layered_section_body(p, header_start)?;
    let mut found = false;
    let mut linetype_scale_found = false;
    for (index, (code, name)) in header.iter().enumerate() {
        if *code == 9 && name == "$LTSCALE" {
            if linetype_scale_found {
                return Err(IoError::Invalid("duplicate DXF LTSCALE".into()));
            }
            linetype_scale_found = true;
            let value = header.get(index + 1).and_then(|(field, value)| (*field == 40).then_some(value)).and_then(|value| value.parse::<f64>().ok());
            if value != Some(1.0) {
                return Err(IoError::Invalid("not supported yet: nondefault DXF global linetype scale".into()));
            }
        }
        if *code != 9 || name != "$INSUNITS" {
            continue;
        }
        if found {
            return Err(IoError::Invalid("duplicate DXF INSUNITS".into()));
        }
        found = true;
        let Some((70, value)) = header.get(index + 1) else {
            return Err(IoError::Invalid("invalid DXF INSUNITS field".into()));
        };
        match value.parse::<i16>() {
            Ok(0 | 1 | 2 | 4 | 5 | 6) => {}
            _ => return Err(IoError::Invalid("not supported yet: this DXF INSUNITS value".into())),
        }
    }
    // The existing helper owns the actual conversion factors; missing or
    // unitless values retain its documented millimetre interpretation.
    Ok(units_scale(header))
}

/// A bounded view of the EXISTING parsed pairs in a previously located section.
/// The caller's section locator validates uniqueness and the ENDSEC terminator.
fn layered_section_body(p: &[(i32, String)], start: usize) -> Result<&[(i32, String)]> {
    let rest = p.get(start.saturating_add(1)..).ok_or_else(|| IoError::Invalid("invalid layered DXF section start".into()))?;
    let end = rest
        .iter()
        .position(|(code, value)| *code == 0 && value == "ENDSEC")
        .ok_or_else(|| IoError::Invalid("unterminated layered DXF section".into()))?;
    rest.get(..end).ok_or_else(|| IoError::Invalid("invalid layered DXF section bounds".into()))
}

/// Validate integer spelling BEFORE the existing section() converts general
/// numeric fields through f64. This only audits shared parsed tokens in the
/// selected section: no second parser, stored model or legacy semantic change.
/// The existing typed consumers still enforce each field's permitted enum.
fn layered_integer_fields(p: &[(i32, String)], start: usize, codes: &[i32]) -> Result<()> {
    for (code, value) in layered_section_body(p, start)? {
        if codes.contains(code) && value.parse::<i16>().is_err() {
            return Err(IoError::Invalid(format!("invalid integer spelling for layered DXF group {code}")));
        }
    }
    Ok(())
}

/// A present malformed/duplicate numeric field is an error, never omission.
fn layered_number(entity: &Ent, code: i32) -> Result<Option<f64>> {
    let mut values = entity
        .f
        .iter()
        .filter(|(field, _)| *field == code)
        .map(|(_, value)| Some(*value))
        .chain(entity.s.iter().filter(|(field, _)| *field == code).map(|_| None));
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(IoError::Invalid(format!("duplicate layered DXF group {code}")));
    }
    value.map(Some).ok_or_else(|| IoError::Invalid(format!("invalid layered DXF group {code}")))
}

fn layered_text(entity: &Ent, code: i32) -> Result<Option<&str>> {
    let mut values = entity
        .s
        .iter()
        .filter(|(field, _)| *field == code)
        .map(|(_, value)| Some(value.as_str()))
        .chain(entity.f.iter().filter(|(field, _)| *field == code).map(|_| None));
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(IoError::Invalid(format!("duplicate layered DXF group {code}")));
    }
    value.map(Some).ok_or_else(|| IoError::Invalid(format!("invalid layered DXF group {code}")))
}

fn layered_required(entity: &Ent, code: i32) -> Result<f64> {
    layered_number(entity, code)?.ok_or_else(|| IoError::Invalid(format!("missing layered DXF group {code}")))
}

/// Layered-only metadata audit on the existing pairs, before section() can
/// round integer spellings through f64. Legacy parsing remains untouched.
fn layered_array_integer_fields(p: &[(i32, String)], start: usize) -> Result<()> {
    layered_integer_fields(p, start, &[70, 71, 72, 73, 74])?;
    for (code, value) in layered_section_body(p, start)? {
        if *code == 90 && value.parse::<i32>().is_err() {
            return Err(IoError::Invalid("invalid integer spelling for layered DXF group 90".into()));
        }
    }
    layered_polyline_underflow_fields(layered_section_body(p, start)?)
}

/// Audit only the existing raw LWPOLYLINE tokens before section() loses them.
/// parse::<f64>() validates the decimal/exponent grammar. Once it returns zero,
/// a nonzero digit in the SIGNIFICAND (never the exponent) proves underflow.
fn layered_polyline_underflow_fields(body: &[(i32, String)]) -> Result<()> {
    let mut index = 0;
    while let Some((code, kind)) = body.get(index) {
        if *code != 0 {
            index += 1;
            continue;
        }
        let rest = body.get(index + 1..).ok_or_else(|| IoError::Invalid("invalid DXF token range".into()))?;
        let count = rest.iter().position(|(code, _)| *code == 0).unwrap_or(rest.len());
        let fields = rest.get(..count).ok_or_else(|| IoError::Invalid("invalid DXF entity range".into()))?;
        // Valid paper space is skipped before geometry interpretation. Existing
        // lexical67 and typed duplicate checks still apply in the parent.
        if kind == "LWPOLYLINE" && !fields.iter().any(|(code, value)| *code == 67 && value.parse::<i16>() == Ok(1)) {
            for (code, value) in fields {
                if !matches!(code, 10 | 20 | 42) || !matches!(value.parse::<f64>(), Ok(number) if number == 0.0) {
                    continue;
                }
                let unsigned = value.strip_prefix('+').or_else(|| value.strip_prefix('-')).unwrap_or(value);
                let significand = unsigned.split_once(['e', 'E']).map_or(unsigned, |(significand, _)| significand);
                if significand.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
                    return Err(IoError::Invalid(format!("nonzero layered DXF LWPOLYLINE group {code} underflows to zero")));
                }
            }
        }
        index += 1 + count;
    }
    Ok(())
}

fn layered_count(entity: &Ent, code: i32, min: usize, max: usize) -> Result<usize> {
    let value = layered_number(entity, code)?
        .ok_or_else(|| IoError::Invalid(format!("not supported yet: layered DXF {} without explicit group {code}", entity.kind,)))?;
    // Integer tokens were checked in the existing section's pair slice. Keep
    // a local guard too; only a bounded integral value reaches the cast.
    if !value.is_finite() || value.fract() != 0.0 || value < min as f64 || value > max as f64 {
        return Err(IoError::Invalid(format!("invalid layered DXF {} count/flag {code}", entity.kind)));
    }
    Ok(value as usize)
}

fn layered_array_count(entity: &Ent, code: i32, expected: usize) -> Result<()> {
    if entity.s.iter().any(|(field, _)| *field == code) || entity.f.iter().filter(|(field, _)| *field == code).count() != expected {
        return Err(IoError::Invalid(format!("invalid layered DXF array length/value for group {code}")));
    }
    Ok(())
}

fn layered_array_budget<T>(count: usize, bytes: &mut usize, work: &mut usize) -> Result<()> {
    let added = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| IoError::Invalid("DXF geometry byte count overflow".into()))?;
    *bytes = bytes
        .checked_add(added)
        .filter(|total| *total <= MAX_DRAWING_BYTES)
        .ok_or_else(|| IoError::Invalid("layered DXF geometry byte budget exceeded".into()))?;
    *work = work
        .checked_add(count)
        .filter(|total| *total <= MAX_WORK)
        .ok_or_else(|| IoError::Invalid("layered DXF geometry work budget exceeded".into()))?;
    Ok(())
}

fn layered_points(entity: &Ent, count: usize, k: f64) -> Result<Vec<Vec2>> {
    let mut points = Vec::new();
    points.try_reserve_exact(count).map_err(|_| IoError::Invalid("DXF point allocation failed".into()))?;
    let xs = entity.f.iter().filter(|(code, _)| *code == 10).map(|(_, value)| *value);
    let ys = entity.f.iter().filter(|(code, _)| *code == 20).map(|(_, value)| *value);
    let mut previous: Option<(Vec2, Vec2)> = None;
    for (x, y) in xs.zip(ys) {
        let source = Vec2::new(x, y);
        let point = source * k;
        if !point.is_finite() {
            return Err(IoError::Invalid("nonfinite scaled DXF control/vertex".into()));
        }
        if let Some((before, after)) = previous
            && before != source
            && after == point
        {
            return Err(IoError::Invalid("DXF unit conversion collapses adjacent distinct points".into()));
        }
        if points.len() >= count {
            return Err(IoError::Invalid("extra DXF point beyond checked count".into()));
        }
        points.push(point);
        previous = Some((source, point));
    }
    if points.len() != count {
        return Err(IoError::Invalid("missing DXF points after count check".into()));
    }
    Ok(points)
}

fn layered_values(entity: &Ent, code: i32, count: usize) -> Result<Vec<f64>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| IoError::Invalid("DXF scalar-array allocation failed".into()))?;
    for (_, value) in entity.f.iter().filter(|(field, _)| *field == code) {
        // section() already admitted only finite, bounded numeric values.
        if !value.is_finite() {
            return Err(IoError::Invalid("nonfinite DXF array value".into()));
        }
        if values.len() >= count {
            return Err(IoError::Invalid("extra DXF scalar beyond checked count".into()));
        }
        values.push(*value);
    }
    if values.len() != count {
        return Err(IoError::Invalid("missing DXF values after count check".into()));
    }
    Ok(values)
}

fn layered_polyline(entity: &Ent, k: f64, linetype: DxfLinetype, bytes: &mut usize, work: &mut usize) -> Result<DxfShape> {
    let count = layered_count(entity, 90, 2, MAX_ENTITIES)?;
    let flags = layered_count(entity, 70, 0, 129)?;
    // Charge the common coordinate slots and bounded linear work to the REAL
    // file counters before scanning values to select either representation.
    layered_array_budget::<Vec2>(count, bytes, work)?;
    let nonzero_bulge = entity.f.iter().any(|(code, value)| *code == 42 && *value != 0.0);
    if flags == 129 || nonzero_bulge {
        if !matches!(flags, 128 | 129) {
            return Err(IoError::Invalid("not supported yet: bulged/closed DXF polyline requires flag128/129".into()));
        }
        layered_array_count(entity, 10, count)?;
        layered_array_count(entity, 20, count)?;
        if entity.s.iter().any(|(code, _)| *code == 42) || entity.f.iter().filter(|(code, _)| *code == 42).count() > count {
            return Err(IoError::Invalid("invalid layered DXF bulge array".into()));
        }
        // Complete the actual DxfVertex payload charge before allocation,
        // ordered vertex scan and admission; coordinate slots are paid above.
        let extra = std::mem::size_of::<crate::DxfVertex>()
            .checked_sub(std::mem::size_of::<Vec2>())
            .and_then(|extra| count.checked_mul(extra))
            .ok_or_else(|| IoError::Invalid("DXF vertex byte count overflow".into()))?;
        *bytes = bytes
            .checked_add(extra)
            .filter(|total| *total <= MAX_DRAWING_BYTES)
            .ok_or_else(|| IoError::Invalid("layered DXF geometry byte budget exceeded".into()))?;
        let closed = flags == 129;
        return Ok(DxfShape::BulgedPolyline { vertices: layered_bulged_vertices(entity, count, k, closed)?, closed });
    }
    // All-zero/open records keep the prior Polyline representation and flags.
    // Its wire cannot retain the original variant or signed-zero bulge bits.
    if flags != 128 && !(flags == 0 && linetype == DxfLinetype::Continuous) {
        return Err(IoError::Invalid("not supported yet: closed DXF polyline or unrepresented per-segment dash restart".into()));
    }
    layered_array_count(entity, 10, count)?;
    layered_array_count(entity, 20, count)?;
    let bulges = entity.f.iter().filter(|(code, _)| *code == 42);
    if entity.s.iter().any(|(code, _)| *code == 42) || bulges.clone().count() > count || bulges.clone().any(|(_, value)| *value != 0.0) {
        return Err(IoError::Invalid("not supported yet: bulged or invalid DXF polyline".into()));
    }
    Ok(DxfShape::Polyline { points: layered_points(entity, count, k)? })
}

/// The numeric Ent fields retain their original relative order. Invalid fields
/// stored as text are rejected before this scan; sparse42 stays on its vertex.
fn layered_bulged_vertices(entity: &Ent, count: usize, k: f64, closed: bool) -> Result<Vec<crate::DxfVertex>> {
    let mut vertices: Vec<crate::DxfVertex> = Vec::new();
    vertices.try_reserve_exact(count).map_err(|_| IoError::Invalid("DXF bulged vertex allocation failed".into()))?;
    let mut has_y = false;
    let mut has_bulge = false;
    for (code, value) in &entity.f {
        match code {
            10 => {
                if (!vertices.is_empty() && !has_y) || vertices.len() >= count {
                    return Err(IoError::Invalid("missing Y or excess layered DXF bulged vertex".into()));
                }
                vertices.push(crate::DxfVertex { at: Vec2::new(*value, 0.0), bulge: 0.0 });
                has_y = false;
                has_bulge = false;
            }
            20 | 42 => {
                let vertex = vertices.last_mut().ok_or_else(|| IoError::Invalid("orphan layered DXF vertex component".into()))?;
                if *code == 20 {
                    if has_y {
                        return Err(IoError::Invalid("duplicate layered DXF vertex Y".into()));
                    }
                    vertex.at.y = *value;
                    has_y = true;
                } else {
                    if has_bulge {
                        return Err(IoError::Invalid("duplicate layered DXF outgoing bulge".into()));
                    }
                    vertex.bulge = *value;
                    has_bulge = true;
                }
            }
            _ => {}
        }
    }
    if vertices.len() != count || !has_y {
        return Err(IoError::Invalid("missing layered DXF bulged vertices/Y".into()));
    }
    let first_raw = vertices.first().map(|vertex| vertex.at);
    let mut previous: Option<(Vec2, Vec2)> = None;
    for vertex in &mut vertices {
        let raw = vertex.at;
        let scaled = raw * k;
        if !scaled.is_finite() {
            return Err(IoError::Invalid("nonfinite scaled DXF bulged vertex".into()));
        }
        if let Some((before, after)) = previous
            && before != raw
            && after == scaled
        {
            return Err(IoError::Invalid("DXF unit conversion collapses adjacent distinct bulged vertices".into()));
        }
        vertex.at = scaled;
        previous = Some((raw, scaled));
    }
    if closed
        && let Some((first, (last, scaled_last))) = first_raw.zip(previous)
        && first != last
        && vertices.first().is_some_and(|vertex| vertex.at == scaled_last)
    {
        return Err(IoError::Invalid("DXF unit conversion collapses distinct bulged seam vertices".into()));
    }
    // Existing producer admission with W1's separate pub(super) delta.
    // Do not substitute interval bounds, sampling or arc reconstruction here.
    crate::dxf_out::admit_bulged(&vertices, closed)?;
    Ok(vertices)
}

fn layered_spline(entity: &Ent, k: f64, bytes: &mut usize, work: &mut usize) -> Result<DxfShape> {
    let degree = layered_count(entity, 71, 1, MAX_BEZIER_DEGREE)?;
    let controls = layered_count(entity, 73, degree + 1, MAX_ENTITIES)?;
    let knots = layered_count(entity, 72, 1, MAX_ENTITIES)?;
    let flags = layered_count(entity, 70, 0, 31)?;
    if !matches!(flags, 0 | 4 | 8 | 12) || layered_number(entity, 74)?.unwrap_or(0.0) != 0.0 {
        return Err(IoError::Invalid("not supported yet: closed/periodic/linear/fit-point DXF spline".into()));
    }
    if controls.checked_add(degree).and_then(|n| n.checked_add(1)) != Some(knots) {
        return Err(IoError::Invalid("invalid layered DXF spline knot/control count".into()));
    }
    layered_array_count(entity, 10, controls)?;
    layered_array_count(entity, 20, controls)?;
    layered_array_count(entity, 40, knots)?;
    let rational = flags & 4 != 0;
    layered_array_count(entity, 41, if rational { controls } else { 0 })?;
    if entity.f.iter().any(|(code, value)| *code == 41 && *value <= 0.0) {
        return Err(IoError::Invalid("DXF spline weights must be positive".into()));
    }
    // Charge cumulative geometry work/payload before knot scans and every new
    // array allocation. Declared counts and parsed values are already bounded.
    layered_array_budget::<Vec2>(controls, bytes, work)?;
    layered_array_budget::<f64>(knots, bytes, work)?;
    if rational {
        layered_array_budget::<f64>(controls, bytes, work)?;
    }
    // The current DxfShape contract is a clamped B-spline. Validate its knot
    // domain and multiplicities on borrowed values before allocating copies.
    let first = entity.get(40).ok_or_else(|| IoError::Invalid("missing spline start knot".into()))?;
    let last = entity
        .f
        .iter()
        .rev()
        .find(|(code, _)| *code == 40)
        .map(|(_, value)| *value)
        .ok_or_else(|| IoError::Invalid("missing spline end knot".into()))?;
    if first >= last {
        return Err(IoError::Invalid("empty DXF spline knot domain".into()));
    }
    let mut previous = None;
    let mut repeats = 0usize;
    for (index, value) in entity.f.iter().filter(|(code, _)| *code == 40).map(|(_, value)| *value).enumerate() {
        if previous.is_some_and(|old| old > value) {
            return Err(IoError::Invalid("decreasing DXF spline knots".into()));
        }
        if index <= degree {
            if value != first {
                return Err(IoError::Invalid("not supported yet: unclamped DXF spline start".into()));
            }
        } else if index >= controls {
            if value != last {
                return Err(IoError::Invalid("not supported yet: unclamped DXF spline end".into()));
            }
        } else {
            repeats = if previous == Some(value) { repeats + 1 } else { 1 };
            if value <= first || value >= last || repeats > degree {
                return Err(IoError::Invalid("not supported yet: excess DXF spline knot multiplicity".into()));
            }
        }
        previous = Some(value);
    }
    let ctrl = layered_points(entity, controls, k)?;
    let knot_values = layered_values(entity, 40, knots)?;
    let weights = if rational { Some(layered_values(entity, 41, controls)?) } else { None };
    // degree was checked against MAX_BEZIER_DEGREE (9), not clamped to7.
    Ok(DxfShape::Spline { degree: degree as u8, knots: knot_values, ctrl, weights })
}

/// Exact parameter storage for the existing DxfShape::Ellipse contract.
/// No spline fitting, trig sampling, angle normalization or new full/arc flag.
fn layered_ellipse(entity: &Ent, k: f64) -> Result<DxfShape> {
    let x =
        layered_number(entity, 10)?.ok_or_else(|| IoError::Invalid("not supported yet: layered DXF ELLIPSE without center coordinates".into()))?;
    let c = Vec2::new(x, layered_required(entity, 20)?) * k;
    // DXF11/21 is RELATIVE to the center. The model also stores this vector;
    // adding the center here would corrupt translated ellipses.
    let major = Vec2::new(layered_required(entity, 11)?, layered_required(entity, 21)?) * k;
    let ratio = layered_required(entity, 40)?;
    let t0 = layered_required(entity, 41)?;
    let t1 = layered_required(entity, 42)?;
    let major_radius = major.len();
    if !c.is_finite()
        || !major.is_finite()
        || major == Vec2::ZERO
        || !major_radius.is_finite()
        || major_radius <= 0.0
        || !ratio.is_finite()
        || ratio <= 0.0
        || ratio > 1.0
    {
        return Err(IoError::Invalid("invalid or degenerate layered DXF ellipse support".into()));
    }
    let minor = major.perp() * ratio;
    let minor_radius = major_radius * ratio;
    if !minor.is_finite() || minor == Vec2::ZERO || !minor_radius.is_finite() || minor_radius <= 0.0 {
        return Err(IoError::Invalid("DXF ellipse minor axis collapses in the represented units".into()));
    }
    // This partial reader admits one-turn parameters in radians. Existing Out
    // spells TAU as this exact12-decimal value, slightly above stored TAU.
    // Admit that ONE additional value by equality, never an epsilon band or
    // a normalization back to TAU. Both parsed parameters are stored unchanged.
    // Both integers are exact in binary64; their one rounded quotient is the
    // exact same f64 as the decimal token, not a truncated TAU approximation.
    const WRITER_TAU: f64 = 6_283_185_307_180.0 / 1_000_000_000_000.0;
    let in_domain = |t: f64| t.is_finite() && ((0.0..=std::f64::consts::TAU).contains(&t) || t == WRITER_TAU);
    if !in_domain(t0) || !in_domain(t1) || t0 == t1 || (t1 == 0.0 && (t0 == std::f64::consts::TAU || t0 == WRITER_TAU)) {
        return Err(IoError::Invalid("not supported yet: empty, ambiguous or out-of-domain DXF ellipse parameters".into()));
    }
    // t1<t0 retains the model's CCW crossing of parameter zero. The canonical
    // 0/TAU pair and its writer spelling also remain raw parameters in the
    // same variant; import does not infer original pre-serialization precision.
    Ok(DxfShape::Ellipse { c, major, ratio, t0, t1 })
}

fn layered_geometry(entity: &Ent, k: f64, linetype: DxfLinetype, bytes: &mut usize, work: &mut usize) -> Result<DxfShape> {
    if entity.kind == "ELLIPSE" {
        return layered_ellipse(entity, k);
    }
    if entity.kind == "LWPOLYLINE" {
        return layered_polyline(entity, k, linetype, bytes, work);
    }
    if entity.kind == "SPLINE" {
        return layered_spline(entity, k, bytes, work);
    }
    if !matches!(entity.kind.as_str(), "POINT" | "LINE" | "CIRCLE" | "ARC") {
        return Err(IoError::Invalid(format!("not supported yet: layered DXF {} geometry", entity.kind)));
    }
    // No coordinate defaults are invented for an incomplete POINT. Present
    // malformed/duplicate values still fail through the real typed helper.
    if entity.kind == "POINT" && layered_number(entity, 10)?.is_none() && layered_number(entity, 20)?.is_none() {
        return Err(IoError::Invalid("not supported yet: layered DXF POINT without coordinates".into()));
    }
    let raw = Vec2::new(layered_required(entity, 10)?, layered_required(entity, 20)?);
    let c = raw * k;
    if !c.is_finite() {
        return Err(IoError::Invalid("nonfinite scaled DXF point".into()));
    }
    if entity.kind == "POINT" {
        return Ok(DxfShape::Point(c));
    }
    if entity.kind == "LINE" {
        let raw_end = Vec2::new(layered_required(entity, 11)?, layered_required(entity, 21)?);
        let end = raw_end * k;
        if !end.is_finite() || (raw != raw_end && c == end) {
            return Err(IoError::Invalid("invalid or collapsed scaled DXF line".into()));
        }
        return Ok(DxfShape::Line(c, end));
    }
    let radius = layered_required(entity, 40)? * k;
    if !radius.is_finite() || radius <= 0.0 {
        return Err(IoError::Invalid("DXF radius must be finite and positive".into()));
    }
    if entity.kind == "CIRCLE" {
        return Ok(DxfShape::Circle { c, r: radius });
    }
    let a0 = layered_required(entity, 50)?;
    let a1 = layered_required(entity, 51)?;
    Ok(DxfShape::Arc { c, r: radius, a0, a1 })
}

fn layered_entity_fields(entity: &Ent) -> Result<()> {
    // Preserve 2D meaning instead of silently projecting tilted/elevated OCS.
    for (code, default) in [(30, 0.0), (31, 0.0), (39, 0.0), (210, 0.0), (220, 0.0), (230, 1.0)] {
        if code == 30 && entity.kind == "SPLINE" {
            let zs = entity.f.iter().filter(|(field, _)| *field == 30);
            if entity.s.iter().any(|(field, _)| *field == 30)
                || zs.clone().any(|(_, value)| *value != 0.0)
                || zs.count() > entity.f.iter().filter(|(field, _)| *field == 10).count()
            {
                return Err(IoError::Invalid("not supported yet: nonplanar or invalid DXF spline controls".into()));
            }
            continue;
        }
        if layered_number(entity, code)?.is_some_and(|value| value != default) {
            return Err(IoError::Invalid("not supported yet: nonplanar or thick layered DXF entity".into()));
        }
    }
    if entity.kind == "LWPOLYLINE" {
        for code in [38, 43] {
            if layered_number(entity, code)?.is_some_and(|value| value != 0.0) {
                return Err(IoError::Invalid("not supported yet: elevated or wide DXF polyline".into()));
            }
        }
    }
    for code in entity.f.iter().map(|(code, _)| *code).chain(entity.s.iter().map(|(code, _)| *code)) {
        let geometry = match entity.kind.as_str() {
            "POINT" => matches!(code, 10 | 20),
            "LINE" => matches!(code, 10 | 20 | 11 | 21),
            "CIRCLE" => matches!(code, 10 | 20 | 40),
            "ELLIPSE" => matches!(code, 10 | 20 | 11 | 21 | 40 | 41 | 42),
            "ARC" => matches!(code, 10 | 20 | 40 | 50 | 51),
            "LWPOLYLINE" => matches!(code, 10 | 20 | 38 | 42 | 43 | 70 | 90),
            "SPLINE" => matches!(code, 10 | 20 | 40 | 41 | 70 | 71 | 72 | 73 | 74),
            _ => false,
        };
        if geometry || matches!(code, 5 | 8 | 30 | 31 | 39 | 67 | 100 | 210 | 220 | 230 | 330 | 370) {
            continue;
        }
        // Includes explicit group6, entity colors/visibility, per-vertex
        // widths, spline fit/tangent data and unrepresented entity metadata.
        return Err(IoError::Invalid(format!("not supported yet: layered DXF entity group {code}")));
    }
    Ok(())
}

#[derive(Default)]
struct LayeredLinetypes {
    dashed: bool,
    hidden: bool,
}

/// Validate the real tables and resolve layer0 BEFORE constructing a file or
/// minting its handles. No late setter or parallel layer representation.
fn layered_tables(records: &[Ent], k: f64, work: &mut usize) -> Result<(DxfFile<DxfShape>, Vec<DxfLayerId>)> {
    let mut table = None;
    let mut types = LayeredLinetypes::default();
    // Definitions may follow their layer references in the file. Validate
    // their real patterns first instead of guessing a style from its name.
    for record in records {
        layered_work(work)?;
        match record.kind.as_str() {
            "TABLE" if table.is_none() => {
                table = Some(layered_text(record, 2)?.ok_or_else(|| IoError::Invalid("unnamed DXF table".into()))?);
            }
            "ENDTAB" if table.is_some() => {
                table = None;
            }
            "LTYPE" if table == Some("LTYPE") => layered_linetype_definition(record, k, &mut types)?,
            "LAYER" if table == Some("LAYER") => {}
            "STYLE" if table == Some("STYLE") => {}
            "BLOCK_RECORD" if table == Some("BLOCK_RECORD") => {}
            _ => return Err(IoError::Invalid("not supported yet: malformed or unsupported layered DXF table record".into())),
        }
    }
    if table.is_some() {
        return Err(IoError::Invalid("unterminated DXF table".into()));
    }

    let mut zero = None;
    for record in records.iter().filter(|record| record.kind == "LAYER") {
        layered_work(work)?;
        let (name, color, linetype, weight) = layered_layer_style(record, &types)?;
        if name.is_empty() || name == "0" {
            if zero.is_some() {
                return Err(IoError::Invalid("duplicate DXF layer zero declaration".into()));
            }
            zero = Some((color, linetype, weight));
        }
    }
    // Pending model dependency, owned by W2: use its real constructor only.
    // The omission branch keeps the existing new() default contract unchanged.
    let mut file = match zero {
        Some((color, linetype, weight)) => DxfFile::with_layer_zero(color, linetype, weight)?,
        None => DxfFile::new()?,
    };
    let mut handles = Vec::new();
    handles.try_reserve(1).map_err(|_| IoError::Invalid("DXF layer lookup allocation failed".into()))?;
    handles.push(file.default_layer());
    for record in records.iter().filter(|record| record.kind == "LAYER") {
        let name = layered_text(record, 2)?.ok_or_else(|| IoError::Invalid("unnamed DXF layer".into()))?;
        if name.is_empty() || name == "0" {
            continue;
        }
        // Bound both the shared model's declaration scan and our handle scan.
        *work = work.saturating_add(file.layers().len()).saturating_add(handles.len()).saturating_add(1);
        if *work > MAX_WORK {
            return Err(IoError::Invalid("the layered DXF drawing is too complex".into()));
        }
        layered_declare_layer(record, &types, &mut file, &mut handles)?;
    }
    Ok((file, handles))
}

fn layered_linetype_definition(record: &Ent, k: f64, types: &mut LayeredLinetypes) -> Result<()> {
    let name = layered_text(record, 2)?.ok_or_else(|| IoError::Invalid("unnamed DXF linetype".into()))?;
    let expected: &[f64] = if name.eq_ignore_ascii_case("DASHED") {
        &[12.7, -6.35]
    } else if name.eq_ignore_ascii_case("HIDDEN") {
        &[2.0, -1.0]
    } else if ["Continuous", "ByLayer", "ByBlock"].iter().any(|known| name.eq_ignore_ascii_case(known)) {
        &[]
    } else {
        return Err(IoError::Invalid("not supported yet: custom DXF linetype".into()));
    };
    if !expected.is_empty() && k != 1.0 {
        return Err(IoError::Invalid("not supported yet: non-millimetre patterned DXF linetype".into()));
    }
    if layered_number(record, 70)?.unwrap_or(0.0) != 0.0
        || layered_required(record, 72)? != 65.0
        || layered_required(record, 73)? != expected.len() as f64
    {
        return Err(IoError::Invalid("not supported yet: noncanonical DXF linetype definition".into()));
    }
    // Public canonical decimal lengths, not a recomputation whose binary sum
    // may differ from the decimal field emitted by Out's fixed precision.
    let total = if name.eq_ignore_ascii_case("DASHED") {
        19.05
    } else if name.eq_ignore_ascii_case("HIDDEN") {
        3.0
    } else {
        0.0
    };
    if layered_required(record, 40)? != total {
        return Err(IoError::Invalid("not supported yet: noncanonical DXF linetype length".into()));
    }
    let mut patterns = record.f.iter().filter(|(code, _)| *code == 49).map(|(_, value)| *value);
    for value in expected {
        if patterns.next() != Some(*value) {
            return Err(IoError::Invalid("not supported yet: noncanonical DXF linetype pattern".into()));
        }
    }
    if patterns.next().is_some() || record.s.iter().any(|(code, _)| *code == 49) {
        return Err(IoError::Invalid("invalid DXF linetype pattern fields".into()));
    }
    if record.s.iter().any(|(code, _)| *code == 74) {
        return Err(IoError::Invalid("invalid DXF linetype element flags".into()));
    }
    for (code, value) in &record.f {
        if *code == 74 && *value != 0.0 {
            return Err(IoError::Invalid("not supported yet: complex DXF linetype element".into()));
        }
    }
    for code in record.f.iter().map(|(code, _)| *code).chain(record.s.iter().map(|(code, _)| *code)) {
        if !matches!(code, 2 | 3 | 5 | 40 | 49 | 70 | 72 | 73 | 74 | 100 | 330) {
            return Err(IoError::Invalid("not supported yet: complex DXF linetype definition".into()));
        }
    }
    if name.eq_ignore_ascii_case("DASHED") {
        types.dashed = true;
    }
    if name.eq_ignore_ascii_case("HIDDEN") {
        types.hidden = true;
    }
    Ok(())
}

fn layered_layer_style<'a>(record: &'a Ent, types: &LayeredLinetypes) -> Result<(&'a str, i16, DxfLinetype, Option<DxfStandardWeight>)> {
    for code in record.f.iter().map(|(code, _)| *code).chain(record.s.iter().map(|(code, _)| *code)) {
        if !matches!(code, 2 | 5 | 6 | 62 | 70 | 100 | 330 | 370) {
            return Err(IoError::Invalid(format!("not supported yet: DXF layer group {code}")));
        }
    }
    if layered_number(record, 70)?.unwrap_or(0.0) != 0.0 {
        return Err(IoError::Invalid("not supported yet: frozen, locked or dependent DXF layer".into()));
    }
    let name = layered_text(record, 2)?.ok_or_else(|| IoError::Invalid("unnamed DXF layer".into()))?;
    let color = layered_number(record, 62)?.unwrap_or(7.0);
    // section() classified group62 lexically as i16 BEFORE f64. The range
    // here therefore cannot admit a rounded decimal pretending to be an ACI.
    if !(1.0..=255.0).contains(&color) || color.fract() != 0.0 {
        return Err(IoError::Invalid("not supported yet: off or non-indexed DXF layer color".into()));
    }
    let raw_type = layered_text(record, 6)?.unwrap_or("Continuous");
    let linetype = if raw_type.eq_ignore_ascii_case("Continuous") {
        DxfLinetype::Continuous
    } else if raw_type.eq_ignore_ascii_case("DASHED") && types.dashed {
        DxfLinetype::Dashed
    } else if raw_type.eq_ignore_ascii_case("HIDDEN") && types.hidden {
        DxfLinetype::Hidden
    } else {
        return Err(IoError::Invalid("not supported yet: undefined or custom DXF layer linetype".into()));
    };
    let weight = match layered_entity_weight(record)? {
        None => None,
        Some(DxfEntityWeight::Explicit(value)) => Some(value),
        _ => return Err(IoError::Invalid("not supported yet: symbolic DXF LAYER lineweight".into())),
    };
    Ok((name, color as i16, linetype, weight))
}

fn layered_declare_layer(record: &Ent, types: &LayeredLinetypes, file: &mut DxfFile<DxfShape>, handles: &mut Vec<DxfLayerId>) -> Result<()> {
    let (name, color, linetype, weight) = layered_layer_style(record, types)?;
    let handle = file.declare_layer(name, color, linetype, weight)?;
    // Keep model normalization/collision rules authoritative; never silently
    // import a renamed symbol, or merge even an identical duplicate record.
    if file.layer(handle)?.name() != name {
        return Err(IoError::Invalid("not supported yet: DXF layer name requires normalization".into()));
    }
    if handles.contains(&handle) {
        return Err(IoError::Invalid("duplicate DXF layer declaration".into()));
    }
    handles.try_reserve(1).map_err(|_| IoError::Invalid("DXF layer lookup allocation failed".into()))?;
    handles.push(handle);
    Ok(())
}

/// Read the entities of an ASCII DXF file.
pub fn read_dxf(text: &str) -> Result<Vec<Geom2>> {
    if text.len() > MAX_DRAWING_BYTES {
        return Err(IoError::Invalid("the DXF file is too large".into()));
    }
    if text.starts_with("AutoCAD Binary DXF") {
        return Err(IoError::Invalid("binary DXF is not supported; save as ASCII DXF".into()));
    }
    let p = pairs(text);
    let k = units_scale(&p);
    let start = p.iter().position(|(c, v)| *c == 2 && v == "ENTITIES").ok_or_else(|| IoError::Invalid("no ENTITIES section".into()))?;
    let ents = section(&p, start)?;
    // Blocks: name → (base point, entities).
    let mut blocks: HashMap<String, (Vec2, Vec<Ent>)> = HashMap::new();
    if let Some(bs) = p.iter().position(|(c, v)| *c == 2 && v == "BLOCKS") {
        let all = section(&p, bs)?;
        let mut cur: Option<(String, Vec2, Vec<Ent>)> = None;
        for e in all {
            match e.kind.as_str() {
                "BLOCK" => {
                    let base = Vec2::new(e.get(10).unwrap_or(0.0) * k, e.get(20).unwrap_or(0.0) * k);
                    cur = Some((e.text(2).unwrap_or("").to_string(), base, Vec::new()));
                }
                "ENDBLK" => {
                    if let Some((name, base, list)) = cur.take() {
                        blocks.insert(name, (base, list));
                    }
                }
                _ => {
                    if let Some(c) = cur.as_mut() {
                        c.2.push(e);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    convert(&ents, k, &blocks, 0, &mut out, &mut 0)?;
    Ok(out)
}

/// The entities of the section whose name is at `start`.
fn section(p: &[(i32, String)], start: usize) -> Result<Vec<Ent>> {
    let mut ents: Vec<Ent> = Vec::new();
    for (c, v) in p.iter().skip(start + 1) {
        if *c == 0 {
            if v == "ENDSEC" || v == "EOF" {
                break;
            }
            ents.push(Ent { kind: v.clone(), ..Default::default() });
            if ents.len() > MAX_ENTITIES {
                return Err(IoError::Invalid("too many entities".into()));
            }
            continue;
        }
        let Some(e) = ents.last_mut() else { continue };
        // Groups62/370 are signed 16-bit integer tokens, not floating lengths.
        // Preserve an invalid present lexeme as text for the layered adapter;
        // converting through f64 first could round it into a valid color/weight.
        if matches!(*c, 62 | 370) {
            match v.parse::<i16>() {
                Ok(value) => e.f.push((*c, f64::from(value))),
                Err(_) => e.s.push((*c, v.clone())),
            }
            continue;
        }
        let numeric = matches!(*c, 10..=59 | 60..=99 | 140..=147 | 210..=239);
        match v.parse::<f64>() {
            Ok(x) if numeric && x.is_finite() && x.abs() < 1e12 => e.f.push((*c, x)),
            _ => e.s.push((*c, v.clone())),
        }
    }
    Ok(ents)
}

/// Deepest block nesting expanded.
const MAX_BLOCK_DEPTH: usize = 16;
/// Entities visited and array cells placed while expanding blocks. Every bound below this one
/// is on the geometry that comes out, and a block that emits none trips none of them: N blocks
/// that each insert the same empty block N times fan out to N^MAX_BLOCK_DEPTH visits for no
/// output at all. This counts the work instead.
const MAX_WORK: usize = 4 * MAX_ENTITIES;

/// Entities to geometry (millimetres), expanding block references.
fn convert(ents: &[Ent], k: f64, blocks: &HashMap<String, (Vec2, Vec<Ent>)>, depth: usize, out: &mut Vec<Geom2>, work: &mut usize) -> Result<()> {
    let pt = |x: f64, y: f64| Vec2::new(x * k, y * k);
    let mut i = 0;
    while i < ents.len() {
        let Some(e) = ents.get(i) else { break };
        i += 1;
        *work = work.saturating_add(1);
        if out.len() > MAX_ENTITIES || *work > MAX_WORK {
            return Err(IoError::Invalid("the drawing is too complex".into()));
        }
        // Paper space (layouts) is not the drawing.
        if e.get(67).is_some_and(|v| v != 0.0) {
            continue;
        }
        match e.kind.as_str() {
            "INSERT" => {
                let Some((base, list)) = e.text(2).and_then(|n| blocks.get(n)) else { continue };
                if depth >= MAX_BLOCK_DEPTH {
                    continue;
                }
                let mut inner = Vec::new();
                convert(list, k, blocks, depth + 1, &mut inner, work)?;
                if inner.is_empty() {
                    continue;
                }
                let (sx, sy) = (e.get(41).unwrap_or(1.0), e.get(42).unwrap_or(1.0));
                let rot = e.get(50).unwrap_or(0.0).to_radians();
                let ins = pt(e.get(10).unwrap_or(0.0), e.get(20).unwrap_or(0.0));
                let (cols, rows) = (e.get(70).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize, e.get(71).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize);
                let cells = cols.saturating_mul(rows);
                *work = work.saturating_add(cells);
                if cells > MAX_ENTITIES || cells.saturating_mul(inner.len()) > MAX_ENTITIES || *work > MAX_WORK {
                    return Err(IoError::Invalid("the drawing is too complex".into()));
                }
                let (dc, dr) = (e.get(44).unwrap_or(0.0) * k, e.get(45).unwrap_or(0.0) * k);
                for r in 0..rows {
                    for c in 0..cols {
                        // Array offsets run along the rotated block axes.
                        let off = Vec2::from_angle(rot) * (dc * c as f64) + Vec2::from_angle(rot).perp() * (dr * r as f64);
                        let t = Affine::insert(ins + off, *base, sx, sy, rot);
                        for g in &inner {
                            t.apply(g, out);
                        }
                    }
                }
            }
            "LINE" => {
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (e.get(10), e.get(20), e.get(11), e.get(21)) {
                    out.push(Geom2::Line(pt(x0, y0), pt(x1, y1)));
                }
            }
            "POINT" => {
                if let (Some(x), Some(y)) = (e.get(10), e.get(20)) {
                    out.push(Geom2::Point(pt(x, y)));
                }
            }
            "CIRCLE" => {
                if let (Some(x), Some(y), Some(r)) = (e.get(10), e.get(20), e.get(40)) {
                    out.push(Geom2::Circle(pt(x, y), r * k));
                }
            }
            "ARC" => {
                if let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) = (e.get(10), e.get(20), e.get(40), e.get(50), e.get(51)) {
                    let c = pt(x, y);
                    let at = |deg: f64| c + Vec2::from_angle(deg.to_radians()) * (r * k);
                    out.push(Geom2::Arc { c, a: at(a0), b: at(a1) });
                }
            }
            "LWPOLYLINE" => {
                let ys = e.all(20);
                // Bulges follow their vertex: walk the pairs in order.
                let mut verts: Vec<(Vec2, f64)> = Vec::new();
                let mut xi = 0;
                for (c, v) in &e.f {
                    match *c {
                        10 => {
                            if let Some(y) = ys.get(xi) {
                                verts.push((pt(*v, *y), 0.0));
                            }
                            xi += 1;
                        }
                        42 => {
                            if let Some(last) = verts.last_mut() {
                                last.1 = *v;
                            }
                        }
                        _ => {}
                    }
                }
                let closed = e.get(70).is_some_and(|f| (f as i64) & 1 == 1);
                polyline(&verts, closed, out);
            }
            "POLYLINE" => {
                let closed = e.get(70).is_some_and(|f| (f as i64) & 1 == 1);
                let mut verts = Vec::new();
                while let Some(v) = ents.get(i) {
                    if v.kind != "VERTEX" {
                        break;
                    }
                    if let (Some(x), Some(y)) = (v.get(10), v.get(20)) {
                        verts.push((pt(x, y), v.get(42).unwrap_or(0.0)));
                    }
                    i += 1;
                }
                if ents.get(i).is_some_and(|x| x.kind == "SEQEND") {
                    i += 1;
                }
                polyline(&verts, closed, out);
            }
            "ELLIPSE" => {
                if let (Some(x), Some(y), Some(mx), Some(my), Some(ratio)) = (e.get(10), e.get(20), e.get(11), e.get(21), e.get(40)) {
                    let c = pt(x, y);
                    let major = c + Vec2::new(mx, my) * k;
                    let a = Vec2::new(mx, my).len() * k;
                    let (t0, t1) = (e.get(41).unwrap_or(0.0), e.get(42).unwrap_or(std::f64::consts::TAU));
                    // The arc runs counter-clockwise from t0 to t1, a turn at most; whole turns
                    // more or less make no difference.
                    let sweep = (t1 - t0).rem_euclid(std::f64::consts::TAU);
                    if sweep < 1e-9 || std::f64::consts::TAU - sweep < 1e-9 {
                        out.push(Geom2::Ellipse { c, major, minor: a * ratio });
                    } else {
                        // Partial ellipse (an elliptical arc, as Fusion and SolidCraft's own DXF
                        // export write one): exact conic pieces of at most a quarter turn each, not
                        // a spline fitted through samples, which strays from the ellipse between
                        // them (#8155). A piece from s to e is the rational quadratic with its apex
                        // where the end tangents meet and the middle weight cos((e - s) / 2), the
                        // ellipse's image of a circle's arc; its rho is weight / (1 + weight).
                        let u = (major - c) / a.max(1e-12);
                        let v = u.perp() * (a * ratio);
                        let at = |t: f64| c + u * (a * t.cos()) + v * t.sin();
                        // (A quarter turn written out is a hair over or under one; it stays one piece.)
                        let pieces = (sweep / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().clamp(1.0, 4.0) as usize;
                        let step = sweep / pieces as f64;
                        let w = (step / 2.0).cos();
                        for j in 0..pieces {
                            let (from, to) = (t0 + step * j as f64, t0 + step * (j + 1) as f64);
                            let mid = (from + to) / 2.0;
                            let apex = c + (u * (a * mid.cos()) + v * mid.sin()) / w;
                            out.push(Geom2::Conic { a: at(from), apex, b: at(to), rho: w / (1.0 + w) });
                        }
                    }
                }
            }
            "SPLINE" => {
                // The degrees a sketch spline takes (a G4 blend's Bézier is degree 9).
                let degree = e.get(71).unwrap_or(3.0).clamp(1.0, MAX_BEZIER_DEGREE as f64) as u8;
                let (fx, fy) = (e.all(11), e.all(21));
                let (cx, cy) = (e.all(10), e.all(20));
                if fx.len() >= 2 && fx.len() == fy.len() {
                    out.push(Geom2::Spline { pts: fx.iter().zip(&fy).map(|(x, y)| pt(*x, *y)).collect(), control: false, degree: 3 });
                } else if cx.len() >= 2 && cx.len() == cy.len() {
                    out.push(Geom2::Spline { pts: cx.iter().zip(&cy).map(|(x, y)| pt(*x, *y)).collect(), control: true, degree });
                }
            }
            "TEXT" | "MTEXT" => {
                if let (Some(x), Some(y), Some(h), Some(t)) = (e.get(10), e.get(20), e.get(40), e.text(1)) {
                    // MTEXT paragraph codes: keep the plain text.
                    let t = t.replace("\\P", "\n");
                    out.push(Geom2::Text { text: t, at: pt(x, y), height: h * k, angle: e.get(50).unwrap_or(0.0).to_radians() });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// p ↦ m·p + t.
#[derive(Clone, Copy)]
struct Affine {
    m: [f64; 4],
    t: Vec2,
}

impl Affine {
    /// A block placed at `ins`: its base point moves there, scaled then rotated.
    fn insert(ins: Vec2, base: Vec2, sx: f64, sy: f64, rot: f64) -> Affine {
        let (s, c) = rot.sin_cos();
        let m = [c * sx, -s * sy, s * sx, c * sy];
        let mb = Vec2::new(m[0] * base.x + m[1] * base.y, m[2] * base.x + m[3] * base.y);
        Affine { m, t: ins - mb }
    }
    fn p(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.m[0] * p.x + self.m[1] * p.y, self.m[2] * p.x + self.m[3] * p.y) + self.t
    }
    fn v(&self, v: Vec2) -> Vec2 {
        Vec2::new(self.m[0] * v.x + self.m[1] * v.y, self.m[2] * v.x + self.m[3] * v.y)
    }
    fn det(&self) -> f64 {
        self.m[0] * self.m[3] - self.m[1] * self.m[2]
    }
    /// The scale when it is the same in every direction.
    fn uniform(&self) -> Option<f64> {
        let (a, b) = (self.v(Vec2::new(1.0, 0.0)), self.v(Vec2::new(0.0, 1.0)));
        ((a.len() - b.len()).abs() <= 1e-9 * a.len().max(b.len()) && a.dot(b).abs() <= 1e-9 * a.len() * b.len()).then_some(a.len())
    }
    fn apply(&self, g: &Geom2, out: &mut Vec<Geom2>) {
        let flip = self.det() < 0.0;
        let sampled = |f: &dyn Fn(f64) -> Vec2, closed: bool| {
            let n = 32;
            let mut pts: Vec<Vec2> = (0..=n).map(|j| self.p(f(j as f64 / n as f64))).collect();
            if closed {
                pts.pop();
                if let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
            }
            Geom2::Spline { pts, control: false, degree: 3 }
        };
        out.push(match g {
            Geom2::Point(p) => Geom2::Point(self.p(*p)),
            Geom2::Line(a, b) => Geom2::Line(self.p(*a), self.p(*b)),
            Geom2::Circle(c, r) => match self.uniform() {
                Some(s) => Geom2::Circle(self.p(*c), r * s),
                None => {
                    // The scaled axes are the ellipse's axes (scale comes before rotation).
                    let (u, w) = (self.v(Vec2::new(*r, 0.0)), self.v(Vec2::new(0.0, *r)));
                    let (maj, min) = if u.len() >= w.len() { (u, w) } else { (w, u) };
                    Geom2::Ellipse { c: self.p(*c), major: self.p(*c) + maj, minor: min.len() }
                }
            },
            Geom2::Arc { c, a, b } => match self.uniform() {
                Some(_) if flip => Geom2::Arc { c: self.p(*c), a: self.p(*b), b: self.p(*a) },
                Some(_) => Geom2::Arc { c: self.p(*c), a: self.p(*a), b: self.p(*b) },
                None => {
                    let (r, a0) = ((*a - *c).len(), (*a - *c).angle());
                    let mut sweep = (*b - *c).angle() - a0;
                    while sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    sampled(&|t| *c + Vec2::from_angle(a0 + sweep * t) * r, false)
                }
            },
            Geom2::Ellipse { c, major, minor } => {
                let u = *major - *c;
                let w = u.perp() * (minor / u.len().max(1e-300));
                match self.uniform() {
                    Some(s) => Geom2::Ellipse { c: self.p(*c), major: self.p(*major), minor: minor * s },
                    None => sampled(&|t| *c + u * (t * std::f64::consts::TAU).cos() + w * (t * std::f64::consts::TAU).sin(), true),
                }
            }
            Geom2::Spline { pts, control, degree } => {
                Geom2::Spline { pts: pts.iter().map(|p| self.p(*p)).collect(), control: *control, degree: *degree }
            }
            Geom2::Conic { a, apex, b, rho } => Geom2::Conic { a: self.p(*a), apex: self.p(*apex), b: self.p(*b), rho: *rho },
            Geom2::Text { text, at, height, angle } => {
                let dir = self.v(Vec2::from_angle(*angle));
                let up = self.v(Vec2::from_angle(*angle).perp());
                Geom2::Text { text: text.clone(), at: self.p(*at), height: height * up.len(), angle: dir.angle() }
            }
        });
    }
}

fn polyline(verts: &[(Vec2, f64)], closed: bool, out: &mut Vec<Geom2>) {
    let n = verts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for j in 0..segs {
        let (Some((p0, b)), Some((p1, _))) = (verts.get(j), verts.get((j + 1) % n)) else { continue };
        if p0.dist(*p1) < 1e-12 {
            continue;
        }
        if let Some(g) = bulge_arc(*p0, *p1, *b) {
            out.push(g);
        }
    }
}

/// Write 2D geometry (millimetres) as an R12-style ASCII DXF.
pub fn write_dxf(geom: &[Geom2]) -> String {
    let mut s = String::new();
    let mut w = |c: i32, v: &str| {
        s.push_str(&format!("{c}\n{v}\n"));
    };
    w(0, "SECTION");
    w(2, "HEADER");
    w(9, "$INSUNITS");
    w(70, "4");
    w(0, "ENDSEC");
    w(0, "SECTION");
    w(2, "ENTITIES");
    let f = |x: f64| format!("{x:.9}");
    for g in geom {
        match g {
            Geom2::Point(p) => {
                w(0, "POINT");
                w(8, "0");
                w(10, &f(p.x));
                w(20, &f(p.y));
            }
            Geom2::Line(a, b) => {
                w(0, "LINE");
                w(8, "0");
                w(10, &f(a.x));
                w(20, &f(a.y));
                w(11, &f(b.x));
                w(21, &f(b.y));
            }
            Geom2::Circle(c, r) => {
                w(0, "CIRCLE");
                w(8, "0");
                w(10, &f(c.x));
                w(20, &f(c.y));
                w(40, &f(*r));
            }
            Geom2::Arc { c, a, b } => {
                w(0, "ARC");
                w(8, "0");
                w(10, &f(c.x));
                w(20, &f(c.y));
                w(40, &f(c.dist(*a)));
                w(50, &f((*a - *c).angle().to_degrees()));
                w(51, &f((*b - *c).angle().to_degrees()));
            }
            Geom2::Text { text, at, height, angle } => {
                w(0, "TEXT");
                w(8, "0");
                w(10, &f(at.x));
                w(20, &f(at.y));
                w(40, &f(*height));
                w(50, &f(angle.to_degrees()));
                w(1, &text.replace('\n', " "));
            }
            // Free-form curves are written by the caller as polylines (lines).
            Geom2::Ellipse { .. } | Geom2::Spline { .. } | Geom2::Conic { .. } => {}
        }
    }
    w(0, "ENDSEC");
    w(0, "EOF");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DXF is group codes and values the user did not write, and blocks recurse. None of these
    /// may panic (solidcraft#469).
    #[test]
    fn hostile_dxf_never_panics() {
        let good = write_dxf(&[Geom2::Line(Vec2::ZERO, Vec2::new(10.0, 0.0)), Geom2::Circle(Vec2::new(5.0, 5.0), 2.0)]);
        let mut cases: Vec<String> = vec![
            String::new(),
            "0".into(),
            "0\n".into(),
            "garbage".into(),
            "0\nSECTION\n2\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n4\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n-1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n99999999\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\nnan\n20\ninf\n40\n-1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1e400\n20\n1e400\n40\n1e400\n0\nENDSEC\n0\nEOF\n".into(),
            // SPLINE degrees past any spline's, below 1, and above its control points' count.
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n255\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n-5\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n71\n1e400\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n".into(),
            format!("0\nSECTION\n2\nBLOCKS\n{}0\nENDSEC\n0\nEOF\n", "0\nBLOCK\n2\nb\n0\nENDBLK\n".repeat(300)),
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\na\n1\nINSERT\n2\na\n0\nENDBLK\n0\nENDSEC\n0\nEOF\n".into(),
            format!("0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n1\n".repeat(5000)),
            // An ELLIPSE arc ending far before it starts (once counted up a turn at a time) and
            // one with no length to its axes.
            "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n0\n20\n0\n11\n5\n21\n0\n40\n0.5\n41\n99999999999\n42\n0\n0\nENDSEC\n0\nEOF\n".into(),
            "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n0\n20\n0\n11\n0\n21\n0\n40\n0\n41\n1\n42\n2\n0\nENDSEC\n0\nEOF\n".into(),
        ];
        for cut in (0..good.len()).step_by(11) {
            cases.push(good.get(..cut).unwrap_or("").to_string());
        }
        for (from, to) in [("0\n", "1\n"), ("SECTION", "SECTI0N"), ("EOF", "EOFF")] {
            cases.push(good.replacen(from, to, 3));
        }
        for c in &cases {
            let r = std::panic::catch_unwind(|| read_dxf(c));
            assert!(r.is_ok(), "panicked on {:?}", c.get(..c.len().min(200)));
        }
    }

    /// #8155: an ELLIPSE arc (as Fusion writes one) reads as conic pieces of at most a quarter turn,
    /// end to end from the arc's start to its end, that lie on the ellipse all along, not as a
    /// spline fitted through samples. A whole turn stays one ellipse, and whole turns more or less
    /// make no difference.
    #[test]
    fn an_elliptical_arc_reads_as_exact_conic_pieces() {
        use std::f64::consts::TAU;
        // Centre (5, 2), major radius 10 along 30°, minor radius 4 (ratio 0.4).
        let (c, u) = (Vec2::new(5.0, 2.0), Vec2::new(30f64.to_radians().cos(), 30f64.to_radians().sin()));
        let ellipse = |t0: f64, t1: f64| {
            format!(
                "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n5\n20\n2\n11\n{}\n21\n{}\n40\n0.4\n41\n{t0}\n42\n{t1}\n0\nENDSEC\n0\nEOF\n",
                10.0 * u.x,
                10.0 * u.y
            )
        };
        let at = |t: f64| c + u * (10.0 * t.cos()) + u.perp() * (4.0 * t.sin());
        // How far off the ellipse a point is, in its own equation.
        let off = |p: Vec2| {
            let q = p - c;
            ((q.dot(u) / 10.0).powi(2) + (q.dot(u.perp()) / 4.0).powi(2) - 1.0).abs()
        };
        // A point of the rational quadratic a sketch conic is, at t in 0..1.
        let point = |a: Vec2, apex: Vec2, b: Vec2, rho: f64, t: f64| {
            let w = rho / (1.0 - rho);
            let (k0, k1, k2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
            (a * k0 + apex * k1 + b * k2) / (k0 + k1 + k2)
        };
        for (t0, t1, pieces) in [(20f64, 250f64, 3), (0.0, 90.0, 1), (300.0, 40.0, 2), (10.0, 359.0, 4)] {
            let (t0, t1) = (t0.to_radians(), t1.to_radians());
            let back = read_dxf(&ellipse(t0, t1)).unwrap();
            assert_eq!(back.len(), pieces, "{back:?}");
            let mut worst = 0f64;
            let mut from = at(t0);
            for g in &back {
                let Geom2::Conic { a, apex, b, rho } = g else { panic!("not a conic: {g:?}") };
                assert!(a.dist(from) < 1e-9 && *rho > 0.0 && *rho < 0.5, "{g:?}");
                for k in 0..=50 {
                    worst = worst.max(off(point(*a, *apex, *b, *rho, f64::from(k) / 50.0)));
                }
                from = *b;
            }
            assert!(from.dist(at(t1)) < 1e-9 && worst < 1e-9, "{worst}: {back:?}");
        }
        for (t0, t1) in [(0.0, TAU), (1.0, 1.0), (1.0, 1.0 + 2.0 * TAU)] {
            let back = read_dxf(&ellipse(t0, t1)).unwrap();
            assert!(matches!(&back[..], [Geom2::Ellipse { minor, .. }] if (minor - 4.0).abs() < 1e-9), "{t0}..{t1}: {back:?}");
        }
        let (once, again) = (read_dxf(&ellipse(1.0, 2.0)).unwrap(), read_dxf(&ellipse(1.0, 2.0 + 3.0 * TAU)).unwrap());
        assert_eq!(once.len(), again.len());
    }

    /// A SPLINE keeps its degree up to the 9 a sketch spline (a G4 blend) has (solidcraft#5269); a
    /// degree past that is held to 9, and one that isn't a finite number (which the reader drops)
    /// reads as a missing one, 3.
    #[test]
    fn a_splines_degree_is_read_up_to_nine() {
        let spline = |degree: Option<&str>| {
            let points: String = (0..10).map(|i| format!("10\n{i}\n20\n{}\n", i % 3)).collect();
            let degree = degree.map(|d| format!("71\n{d}\n")).unwrap_or_default();
            format!("0\nSECTION\n2\nENTITIES\n0\nSPLINE\n{degree}{points}0\nENDSEC\n0\nEOF\n")
        };
        for (given, read) in
            [(Some("9"), 9), (Some("8"), 8), (Some("7"), 7), (Some("10"), 9), (Some("0"), 1), (Some("nan"), 3), (Some("inf"), 3), (None, 3)]
        {
            let back = read_dxf(&spline(given)).unwrap();
            assert!(matches!(&back[..], [Geom2::Spline { pts, control: true, degree }] if *degree == read && pts.len() == 10), "{given:?}: {back:?}");
        }
    }

    #[test]
    fn round_trip_and_polyline_bulges() {
        let g = vec![
            Geom2::Line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
            Geom2::Circle(Vec2::new(5.0, 5.0), 2.0),
            Geom2::Arc { c: Vec2::ZERO, a: Vec2::new(3.0, 0.0), b: Vec2::new(0.0, 3.0) },
        ];
        let back = read_dxf(&write_dxf(&g)).unwrap();
        assert_eq!(back.len(), 3);
        match &back[2] {
            Geom2::Arc { c, a, b } => {
                assert!(c.dist(Vec2::ZERO) < 1e-6 && a.dist(Vec2::new(3.0, 0.0)) < 1e-6 && b.dist(Vec2::new(0.0, 3.0)) < 1e-6);
            }
            x => panic!("{x:?}"),
        }
        // A closed LWPOLYLINE slot: two lines and two half circles (bulge 1), in inches.
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n4\n70\n1\n10\n0\n20\n0\n10\n2\n20\n0\n42\n1\n10\n2\n20\n1\n10\n0\n20\n1\n42\n1\n0\nENDSEC\n0\nEOF\n";
        let g = read_dxf(dxf).unwrap();
        assert_eq!(g.len(), 4, "{g:?}");
        match &g[1] {
            Geom2::Arc { c, a, .. } => {
                assert!(c.dist(Vec2::new(50.8, 12.7)) < 1e-9, "{c:?}");
                assert!((c.dist(*a) - 12.7).abs() < 1e-9);
            }
            x => panic!("{x:?}"),
        }
        assert!(read_dxf("garbage").is_err());
        // A block of a unit square and a circle, inserted twice: rotated 90° at (10, 0), and as a
        // 2 × 1 array scaled 2 × 1 at (0, 20) (the circle becomes an ellipse). Nested once more.
        let dxf = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nSQ\n10\n0\n20\n0\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nCIRCLE\n10\n0\n20\n0\n40\n1\n0\nENDBLK\n0\nBLOCK\n2\nOUTER\n10\n0\n20\n0\n0\nINSERT\n2\nSQ\n10\n5\n20\n5\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nSQ\n10\n10\n20\n0\n50\n90\n0\nINSERT\n2\nSQ\n10\n0\n20\n20\n41\n2\n42\n1\n70\n2\n44\n10\n0\nINSERT\n2\nOUTER\n10\n100\n20\n0\n0\nINSERT\n2\nNOPE\n0\nLINE\n67\n1\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n0\nEOF\n";
        let g = read_dxf(dxf).unwrap();
        assert_eq!(g.len(), 8, "{g:?}");
        match &g[0] {
            Geom2::Line(a, b) => assert!(a.dist(Vec2::new(10.0, 0.0)) < 1e-9 && b.dist(Vec2::new(10.0, 1.0)) < 1e-9, "{a:?} {b:?}"),
            x => panic!("{x:?}"),
        }
        match &g[5] {
            Geom2::Ellipse { c, major, minor } => {
                assert!(c.dist(Vec2::new(10.0, 20.0)) < 1e-9 && major.dist(Vec2::new(12.0, 20.0)) < 1e-9 && (minor - 1.0).abs() < 1e-9)
            }
            x => panic!("{x:?}"),
        }
        assert!(matches!(&g[7], Geom2::Circle(c, r) if c.dist(Vec2::new(105.0, 5.0)) < 1e-9 && (*r - 1.0).abs() < 1e-12));
        // A block that inserts itself stops at the depth limit.
        let selfref = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n0\nINSERT\n2\nA\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(read_dxf(selfref).unwrap().len(), 16);
        assert!(read_dxf("AutoCAD Binary DXF\r\n").is_err());
    }

    /// Bounds on the geometry that comes out do not see a block that emits none: six
    /// self-inserts fan out to 6^16 visits for no output. Both shapes must be refused quickly.
    #[test]
    fn empty_blocks_do_not_fan_out() {
        let fanout = format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nA\n{}0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nA\n0\nENDSEC\n0\nEOF\n",
            "0\nINSERT\n2\nA\n".repeat(6)
        );
        assert!(read_dxf(&fanout).is_err(), "a block of six self-inserts must be refused");

        // An empty block placed as a 10,000 by 10,000 array, a thousand times over.
        let array = format!(
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nE\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n",
            "0\nINSERT\n2\nE\n70\n10000\n71\n10000\n".repeat(1000)
        );
        let g = read_dxf(&array).unwrap_or_default();
        assert!(g.is_empty(), "an empty block has no geometry to place");
    }
    // X3 partial preparation: raw fields only. Resolving DXF's symbolic weight
    // values into the shared DxfFile model belongs to the later X2/X3 adapter.
    #[test]
    fn section_keeps_raw_lineweight_values_and_distinguishes_absence() {
        for raw in [-3, -2, -1, 0, 50, 211] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n123\n6\n007\n370\n{raw}\n10\n0\n20\n0\n11\n2\n21\n1\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            assert_eq!(parsed.len(), 1);
            let entity = parsed.first().unwrap();
            assert_eq!(entity.kind, "LINE");
            assert_eq!(entity.get(370), Some(f64::from(raw)));
            assert_eq!(entity.text(370), None);
            // Names that look numeric remain names; this is not a widening of
            // all group codes to numeric fields.
            assert_eq!(entity.text(8), Some("123"));
            assert_eq!(entity.text(6), Some("007"));
            assert_eq!(entity.get(8), None);
            assert_eq!(entity.get(6), None);
        }
        let absent = "0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n";
        let parsed = section(&pairs(absent), 1).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.first().unwrap().get(370), None);
        assert_eq!(parsed.first().unwrap().text(370), None);
    }

    #[test]
    fn section_keeps_lineweight_with_its_entity_without_inheriting_the_previous_one() {
        let text = concat!(
            "0\nSECTION\n2\nENTITIES\n",
            "0\nLINE\n8\nCUT\n6\nDASHED\n370\n50\n10\n0\n20\n0\n11\n2\n21\n1\n",
            "0\nPOINT\n8\nMARKS\n370\n211\n10\n3\n20\n4\n",
            "0\nPOINT\n8\nPLAIN\n10\n5\n20\n6\n",
            "0\nENDSEC\n0\nEOF\n"
        );
        let parsed = section(&pairs(text), 1).unwrap();
        assert_eq!(parsed.len(), 3);
        let line = parsed.first().unwrap();
        let point = parsed.get(1).unwrap();
        let plain = parsed.get(2).unwrap();
        assert_eq!((line.kind.as_str(), line.text(8), line.text(6), line.get(370)), ("LINE", Some("CUT"), Some("DASHED"), Some(50.0)));
        assert_eq!((point.kind.as_str(), point.text(8), point.text(6), point.get(370)), ("POINT", Some("MARKS"), None, Some(211.0)));
        assert_eq!((plain.kind.as_str(), plain.text(8), plain.get(370)), ("POINT", Some("PLAIN"), None));
    }

    #[test]
    fn raw_lineweight_metadata_preserves_legacy_inch_projection_and_text_fallback() {
        let drawing = |metadata: &str| {
            format!(
                concat!(
                    "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n",
                    "0\nSECTION\n2\nENTITIES\n0\nLINE\n{metadata}",
                    "10\n1\n20\n2\n11\n2\n21\n1\n",
                    "0\nPOINT\n10\n0\n20\n1\n",
                    "0\nLINE\n67\n1\n370\n211\n10\n0\n20\n0\n11\n1\n21\n1\n",
                    "0\nENDSEC\n0\nEOF\n"
                ),
                metadata = metadata
            )
        };
        // These values come from the authored inch coordinates, not write_dxf.
        let expected = vec![Geom2::Line(Vec2::new(25.4, 50.8), Vec2::new(50.8, 25.4)), Geom2::Point(Vec2::new(0.0, 25.4))];
        assert_eq!(read_dxf(&drawing("")).unwrap(), expected);
        for raw in ["-3", "-2", "-1", "0", "50", "211"] {
            let metadata = format!("8\nCUT\n6\nDASHED\n370\n{raw}\n");
            assert_eq!(read_dxf(&drawing(&metadata)).unwrap(), expected);
        }
        // Retain the parser's existing fallback. This partial tranche does not
        // authorize a physical-weight validator or a new malformed-input policy.
        for raw in ["nan", "inf", "1e400", "not-a-number"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n370\n{raw}\n10\n0\n20\n1\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            assert_eq!(parsed.len(), 1);
            assert_eq!(parsed.first().unwrap().get(370), None);
            assert_eq!(parsed.first().unwrap().text(370), Some(raw));
            assert_eq!(read_dxf(&drawing(&format!("370\n{raw}\n"))).unwrap(), expected);
        }
    }

    #[test]
    fn layered_weight_keeps_absence_symbols_and_explicit_codes_distinct() {
        let absent = "0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n";
        let parsed = section(&pairs(absent), 1).unwrap();
        assert_eq!(layered_entity_weight(parsed.first().unwrap()).unwrap(), None);
        for (raw, expected) in [
            ("-1", DxfEntityWeight::ByLayer),
            ("-2", DxfEntityWeight::ByBlock),
            ("-3", DxfEntityWeight::Default),
            ("0", DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap())),
            ("-0", DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap())),
            ("50", DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap())),
            ("211", DxfEntityWeight::Explicit(DxfStandardWeight::from_code(211).unwrap())),
        ] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n6\nDASHED\n370\n{raw}\n10\n0\n20\n0\n11\n1\n21\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            let entity = parsed.first().unwrap();
            assert_eq!(layered_entity_weight(entity).unwrap(), Some(expected));
            // This helper preserves other parsed metadata; it does not adopt
            // layer0's style or flatten an entity's group6 into layer metadata.
            assert_eq!(entity.text(8), Some("0"));
            assert_eq!(entity.text(6), Some("DASHED"));
        }
    }

    #[test]
    fn layered_weight_refuses_nonstandard_fractional_or_malformed_values_without_changing_legacy() {
        for raw in ["6", "-4", "212", "65536", "1e11", "50.5", "nan", "inf", "-inf", "1e400", "bad"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n370\n{raw}\n10\n0\n20\n0\n11\n1\n21\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            assert!(layered_entity_weight(parsed.first().unwrap()).is_err(), "raw370={raw}");
            assert_eq!(read_dxf(&text).unwrap(), vec![Geom2::Line(Vec2::ZERO, Vec2::new(1.0, 2.0))]);
        }
    }

    #[test]
    fn layered_weight_refuses_duplicates_including_numeric_text_mixtures() {
        // Duplicate rejection is proposed only for this new layered seam;
        // read_dxf's existing geometry projection is deliberately unchanged.
        for metadata in ["370\n50\n370\n50\n", "370\n50\n370\n211\n", "370\nbad\n370\n50\n", "370\n50\n370\nbad\n", "370\nbad\n370\ninf\n"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n{metadata}10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            assert!(layered_entity_weight(parsed.first().unwrap()).is_err());
            assert_eq!(read_dxf(&text).unwrap(), vec![Geom2::Point(Vec2::new(1.0, 2.0))]);
        }
    }

    #[test]
    fn layered_weight_uses_each_entity_code_without_inheritance_or_inch_rescaling() {
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n",
            "0\nPOINT\n8\nCUT\n370\n50\n10\n1\n20\n2\n",
            "0\nPOINT\n8\nCUT\n10\n2\n20\n1\n",
            "0\nPOINT\n8\n0\n6\nBYBLOCK\n370\n-2\n10\n0\n20\n1\n",
            "0\nENDSEC\n0\nEOF\n"
        );
        let pairs = pairs(text);
        let start = pairs.iter().position(|(code, value)| *code == 2 && value == "ENTITIES").unwrap();
        let parsed = section(&pairs, start).unwrap();
        assert_eq!(parsed.len(), 3);
        let weights: Vec<_> = parsed.iter().map(|entity| layered_entity_weight(entity).unwrap()).collect();
        assert_eq!(weights, vec![Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap())), None, Some(DxfEntityWeight::ByBlock)]);
        assert_eq!(parsed.get(2).unwrap().text(6), Some("BYBLOCK"));
        assert_eq!(
            read_dxf(text).unwrap(),
            vec![Geom2::Point(Vec2::new(25.4, 50.8)), Geom2::Point(Vec2::new(50.8, 25.4)), Geom2::Point(Vec2::new(0.0, 25.4)),]
        );
    }

    #[test]
    fn section_does_not_round_lineweight_spellings_through_f64() {
        // All are invalid signed-integer spellings. Some would round to valid
        // codes if parsed through f64 before the domain check; keep the text.
        for raw in ["50.0000000000000001", "211.00000000000000001", "-1.00000000000000001", "1e-400", "5e1", "-1e0", "50.0", "-0.0", "+5e1"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n370\n{raw}\n10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            assert_eq!(parsed.len(), 1);
            let entity = parsed.first().unwrap();
            assert_eq!(entity.get(370), None, "raw370={raw}");
            assert_eq!(entity.text(370), Some(raw));
            assert!(layered_entity_weight(entity).is_err(), "raw370={raw}");
            assert_eq!(read_dxf(&text).unwrap(), vec![Geom2::Point(Vec2::new(1.0, 2.0))]);
        }
    }

    #[test]
    fn section_preserves_signed_integer_limits_before_standard_weight_validation() {
        for (raw, code) in [("+50", 50_u16), ("-0", 0_u16)] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n370\n{raw}\n10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            let entity = parsed.first().unwrap();
            assert_eq!(entity.get(370), Some(f64::from(code)));
            assert_eq!(entity.text(370), None);
            assert_eq!(layered_entity_weight(entity).unwrap(), Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(code).unwrap())));
        }
        for (raw, value) in [("-32768", i16::MIN), ("32767", i16::MAX)] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n370\n{raw}\n10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            let entity = parsed.first().unwrap();
            // Parsing the integer domain is distinct from approving a physical
            // standard weight: these fit i16 but are not valid entity weights.
            assert_eq!(entity.get(370), Some(f64::from(value)));
            assert_eq!(entity.text(370), None);
            assert!(layered_entity_weight(entity).is_err());
        }
        for raw in ["32768", "-32769"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n370\n{raw}\n10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            let entity = parsed.first().unwrap();
            assert_eq!(entity.get(370), None);
            assert_eq!(entity.text(370), Some(raw));
            assert!(layered_entity_weight(entity).is_err());
        }
    }

    #[test]
    fn layered_weight_rejects_duplicates_when_one_integer_spelling_was_invalid() {
        for metadata in
            ["370\n50.0000000000000001\n370\n50\n", "370\n50\n370\n50.0000000000000001\n", "370\n1e-400\n370\n0\n", "370\n5e1\n370\n50.0\n"]
        {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n{metadata}10\n1\n20\n2\n0\nENDSEC\n0\nEOF\n");
            let parsed = section(&pairs(&text), 1).unwrap();
            assert_eq!(parsed.len(), 1);
            assert!(layered_entity_weight(parsed.first().unwrap()).is_err());
            assert_eq!(read_dxf(&text).unwrap(), vec![Geom2::Point(Vec2::new(1.0, 2.0))]);
        }
    }

    #[test]
    fn layered_read_retains_declared_layers_three_shapes_and_raw_weights_in_inches() {
        // Original hand-authored fixture. The expected millimetre lengths and
        // metadata below are independent of the writer and layered reader.
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n2\n",
            "0\nLAYER\n2\n0\n62\n7\n6\nContinuous\n",
            "0\nLAYER\n2\nCUT\n62\n1\n6\nContinuous\n370\n50\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n",
            "0\nLINE\n8\ncut\n370\n-1\n10\n0\n20\n0\n11\n2\n21\n1\n",
            "0\nCIRCLE\n370\n-3\n10\n1\n20\n2\n40\n0.5\n",
            "0\nARC\n8\nCUT\n370\n-2\n10\n0\n20\n0\n40\n1\n50\n350\n51\n10\n",
            "0\nINSERT\n67\n1\n2\nNOT_EXPANDED_PAPER\n",
            "0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        assert_eq!(file.layers().iter().map(|layer| layer.name()).collect::<Vec<_>>(), ["0", "CUT"]);
        assert_eq!(file.layers()[1].color(), 1);
        assert_eq!(file.layers()[1].weight().unwrap().code(), 50);
        assert_eq!(file.entities().len(), 3);
        let line = &file.entities()[0];
        assert_eq!(file.layer(line.layer()).unwrap().name(), "CUT");
        assert_eq!(line.weight(), Some(DxfEntityWeight::ByLayer));
        assert_eq!(line.geometry(), &DxfShape::Line(Vec2::ZERO, Vec2::new(50.8, 25.4)));
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap()));
        assert_eq!(file.entities()[1].layer(), file.default_layer());
        assert_eq!(file.entities()[1].weight(), Some(DxfEntityWeight::Default));
        assert_eq!(file.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(25.4, 50.8), r: 12.7 });
        assert_eq!(file.entities()[2].weight(), Some(DxfEntityWeight::ByBlock));
        assert_eq!(file.entities()[2].geometry(), &DxfShape::Arc { c: Vec2::ZERO, r: 25.4, a0: 350.0, a1: 10.0 });
    }

    #[test]
    fn layered_read_omission_symbols_and_zero_are_distinct_on_real_entities() {
        let text = concat!(
            "0\nSECTION\n2\nENTITIES\n",
            "0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n",
            "0\nLINE\n370\n-1\n10\n1\n20\n0\n11\n2\n21\n0\n",
            "0\nLINE\n370\n-2\n10\n2\n20\n0\n11\n3\n21\n0\n",
            "0\nLINE\n370\n-3\n10\n3\n20\n0\n11\n4\n21\n0\n",
            "0\nLINE\n370\n0\n10\n4\n20\n0\n11\n5\n21\n0\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        assert_eq!(
            file.entities().iter().map(|entity| entity.weight()).collect::<Vec<_>>(),
            [
                None,
                Some(DxfEntityWeight::ByLayer),
                Some(DxfEntityWeight::ByBlock),
                Some(DxfEntityWeight::Default),
                Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap())),
            ]
        );
    }

    #[test]
    fn layered_read_uses_existing_unit_factors_once_and_refuses_unknown_units() {
        for (unit, millimetres) in [("0", 1.0), ("1", 25.4), ("2", 304.8), ("4", 1.0), ("5", 10.0), ("6", 1000.0)] {
            let text = format!(
                "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{unit}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1\n20\n0\n40\n1\n0\nENDSEC\n0\nEOF\n"
            );
            let file = read_dxf_file(&text).unwrap();
            assert_eq!(file.entities()[0].geometry(), &DxfShape::Circle { c: Vec2::new(millimetres, 0.0), r: millimetres });
        }
        for value in ["3", "7", "1.0", "nan", "32768"] {
            let text = format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{value}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).is_err(), "{value}");
        }
        let duplicate = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(duplicate).unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn layered_read_validates_actual_dashed_and_hidden_definitions() {
        let text = concat!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n",
            "0\nLTYPE\n2\nByBlock\n72\n65\n73\n0\n40\n0\n",
            "0\nLTYPE\n2\nByLayer\n72\n65\n73\n0\n40\n0\n",
            "0\nLTYPE\n2\nContinuous\n72\n65\n73\n0\n40\n0\n",
            "0\nLTYPE\n2\nDASHED\n72\n65\n73\n2\n40\n19.05\n49\n12.7\n74\n0\n49\n-6.35\n74\n0\n",
            "0\nLTYPE\n2\nHIDDEN\n72\n65\n73\n2\n40\n3\n49\n2\n74\n0\n49\n-1\n74\n0\n0\nENDTAB\n",
            "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nBEND\n62\n2\n6\nDASHED\n",
            "0\nLAYER\n2\nBACK\n62\n8\n6\nHIDDEN\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nBEND\n10\n0\n20\n0\n11\n1\n21\n0\n",
            "0\nCIRCLE\n8\nBACK\n10\n0\n20\n0\n40\n1\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        assert_eq!(file.effective_linetype(0).unwrap(), DxfLinetype::Dashed);
        assert_eq!(file.effective_linetype(1).unwrap(), DxfLinetype::Hidden);
        for bad in [
            text.replacen("49\n12.7\n", "49\n5\n", 1),
            text.replacen("40\n19.05\n", "40\n19\n", 1),
            text.replacen("74\n0\n", "74\n2\n", 1),
            text.replacen("73\n2\n", "73\n1\n", 1),
            text.replacen("49\n-1\n", "49\nnan\n", 1),
            text.replacen("2\nDASHED\n", "2\nCUSTOM\n", 1),
            format!("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n{text}"),
        ] {
            assert!(read_dxf_file(&bad).is_err());
        }
    }

    #[test]
    fn layered_read_refuses_explicit_entity_styles_without_losing_them() {
        for field in ["6\nByLayer\n", "6\nByBlock\n", "6\nContinuous\n", "6\nCUSTOM\n", "62\n7\n", "48\n1\n", "60\n1\n", "420\n123\n"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n{field}10\n0\n20\n0\n11\n1\n21\n0\n0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).unwrap_err().to_string().contains("not supported yet"), "{field}");
            assert_eq!(read_dxf(&text).unwrap().len(), 1, "legacy importer unchanged");
        }
    }

    #[test]
    fn layered_read_refuses_a_global_linetype_scale_the_model_cannot_retain() {
        for scale in ["0", "2", "nan", "1e400"] {
            let text = format!("0\nSECTION\n2\nHEADER\n9\n$LTSCALE\n40\n{scale}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).unwrap_err().to_string().contains("linetype scale"));
        }
        let identity = "0\nSECTION\n2\nHEADER\n9\n$LTSCALE\n40\n1\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(identity).is_ok());
    }

    #[test]
    fn layered_read_refuses_unrepresented_layer_zero_weights_and_colors() {
        for (name, fields) in [
            ("0", "6\nCUSTOM\n"),
            ("CUT", "370\n-3\n"),
            ("CUT", "370\n-1\n"),
            ("CUT", "62\n-7\n"),
            ("CUT", "62\n0\n"),
            ("CUT", "62\n256\n"),
            ("CUT", "70\n1\n"),
            ("CUT", "6\nDASHED\n"),
            ("CUT", "6\nCUSTOM\n"),
        ] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n{name}\n{fields}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
            );
            assert!(read_dxf_file(&text).is_err(), "{name}/{fields}");
        }
    }

    #[test]
    fn group62_integer_lexemes_are_preserved_before_float_rounding() {
        for raw in ["7.0000000000000001", "255.00000000000000001", "1e-400", "7e0", "7.0", "32768", "-32769", "nan"] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nCUT\n62\n{raw}\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
            );
            let parsed = pairs(&text);
            let records = section(&parsed, 1).unwrap();
            let layer = records.iter().find(|record| record.kind == "LAYER").unwrap();
            assert_eq!(layer.text(62), Some(raw));
            assert_eq!(layer.get(62), None);
            assert!(read_dxf_file(&text).is_err(), "{raw}");
        }
        for raw in ["+7", "7", "255"] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nCUT\n62\n{raw}\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
            );
            assert!(read_dxf_file(&text).is_ok(), "{raw}");
        }
        let duplicate = "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nCUT\n62\n7\n62\n7.0000000000000001\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(duplicate).unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn lexical_color_and_lineweight_metadata_do_not_change_legacy_geometry() {
        let plain = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n2\n21\n3\n0\nCIRCLE\n10\n4\n20\n5\n40\n2\n0\nENDSEC\n0\nEOF\n";
        for raw in ["7", "7.0000000000000001", "nan", "32768", "1e-400"] {
            let metadata = plain.replacen("0\nLINE\n", &format!("0\nLINE\n62\n{raw}\n370\n50\n"), 1);
            let projected = read_dxf(&metadata).unwrap();
            assert_eq!(projected.len(), 2);
            assert!(matches!(&projected[0], Geom2::Line(a, b) if *a == Vec2::ZERO && *b == Vec2::new(2.0, 3.0)));
            assert!(matches!(&projected[1], Geom2::Circle(c, r) if *c == Vec2::new(4.0, 5.0) && *r == 2.0));
        }
    }

    #[test]
    fn layered_read_rejects_missing_colliding_or_renamed_layer_symbols() {
        for declaration in ["", "0\nLAYER\n2\nOTHER\n", "0\nLAYER\n2\nC?T\n", "0\nLAYER\n2\nÉ\n", "0\nLAYER\n2\nCUT\n0\nLAYER\n2\ncut\n"] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n{declaration}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nCUT\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDSEC\n0\nEOF\n"
            );
            assert!(read_dxf_file(&text).is_err(), "{declaration}");
        }
    }

    #[test]
    fn layered_read_does_not_return_a_partial_file_for_unsupported_geometry() {
        for kind in ["INSERT", "POINT", "LWPOLYLINE", "POLYLINE", "ELLIPSE", "SPLINE", "MTEXT", "UNKNOWN"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\n{kind}\n0\nENDSEC\n0\nEOF\n");
            let saved = text.clone();
            let error = read_dxf_file(&text).unwrap_err().to_string();
            assert!(error.contains("not supported yet") && error.contains(kind), "{error}");
            assert_eq!(text, saved);
        }
        // TEXT is now implemented; preserve this exact empty input as
        // malformed TEXT, rather than inventing an unsupported dispatch.
        let kind = "TEXT";
        let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\n{kind}\n0\nENDSEC\n0\nEOF\n");
        let saved = text.clone();
        let error = read_dxf_file(&text).unwrap_err().to_string();
        assert!(error.contains("missing layered DXF group 10"), "{error}");
        assert_eq!(text, saved);
    }

    #[test]
    fn layered_read_refuses_malformed_geometry_and_nonplanar_semantics() {
        for fields in [
            "10\n0\n20\n0\n11\n1\n",
            "10\n0\n20\n0\n11\n1\n21\n0\n10\n0\n",
            "10\nnan\n20\n0\n11\n1\n21\n0\n",
            "10\n0\n20\n0\n11\n1\n21\n0\n30\n1\n",
            "10\n0\n20\n0\n11\n1\n21\n0\n230\n-1\n",
            "10\n0\n20\n0\n11\n1\n21\n0\n39\n2\n",
        ] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n{fields}0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).is_err());
        }
        for radius in ["0", "-1", "nan", "inf", "1e400", "1e-400"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n0\n20\n0\n40\n{radius}\n0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).is_err(), "{radius}");
        }
    }

    #[test]
    fn layered_read_checks_section_and_table_framing() {
        for text in [
            "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n",
            "0\nSECTION\n2\nENTITIES\n0\nEOF\n",
            "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nLAYER\n2\nCUT\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nCUT\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n",
        ] {
            assert!(read_dxf_file(text).is_err(), "{text}");
        }
    }

    #[test]
    fn layered_read_keeps_input_caps_and_binary_refusal() {
        assert!(read_dxf_file("AutoCAD Binary DXF\r\n").is_err());
        let over_limit = " ".repeat(MAX_DRAWING_BYTES + 1);
        assert!(read_dxf_file(&over_limit).unwrap_err().to_string().contains("too large"));
        let valid = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDSEC\n0\nEOF\n";
        for cut in (0..valid.len()).step_by(7) {
            let _ = read_dxf_file(&valid[..cut]);
        }
    }

    #[test]
    fn layered_header_variables_in_objects_do_not_set_units_or_global_style() {
        let circle = "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n1\n20\n0\n40\n1\n0\nENDSEC\n";
        let objects = "0\nSECTION\n2\nOBJECTS\n0\nXRECORD\n9\n$INSUNITS\n70\n1\n9\n$LTSCALE\n40\n2\n0\nENDSEC\n0\nEOF\n";
        for header in ["", "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n9\n$LTSCALE\n40\n1\n0\nENDSEC\n"] {
            let text = format!("{header}{circle}{objects}");
            let before = text.clone();
            let file = read_dxf_file(&text).unwrap();
            assert_eq!(file.entities()[0].geometry(), &DxfShape::Circle { c: Vec2::new(1.0, 0.0), r: 1.0 });
            assert_eq!(text, before);
        }
        // A record named HEADER is not a HEADER section either.
        let fake_header = "0\nSECTION\n2\nOBJECTS\n0\nXRECORD\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n";
        let text = format!("{fake_header}{circle}0\nEOF\n");
        assert_eq!(read_dxf_file(&text).unwrap().entities()[0].geometry(), &DxfShape::Circle { c: Vec2::new(1.0, 0.0), r: 1.0 });
    }

    #[test]
    fn layered_read_checks_header_framing_even_without_repeated_variables() {
        let entities = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        for header in [
            "0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nSECTION\n2\nHEADER\n0\nENDSEC\n",
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nHEADER\n9\n$LTSCALE\n40\n1\n0\nENDSEC\n",
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n",
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n0\nENDSEC\n",
        ] {
            assert!(read_dxf_file(&format!("{header}{entities}")).is_err(), "{header}");
        }
    }

    #[test]
    fn layered_table_integer_flags_never_accept_rounded_decimal_or_underflow_tokens() {
        let text = concat!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n",
            "0\nLTYPE\n2\nDASHED\n70\n0\n72\n65\n73\n2\n40\n19.05\n49\n12.7\n74\n0\n49\n-6.35\n74\n0\n0\nENDTAB\n",
            "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nCUT\n70\n0\n62\n1\n6\nDASHED\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nCUT\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDSEC\n0\nEOF\n",
        );
        assert!(read_dxf_file(text).is_ok());
        for (field, value) in [
            ("70\n0\n", "70\n1e-400\n"),
            ("72\n65\n", "72\n65.0000000000000001\n"),
            ("73\n2\n", "73\n2.0000000000000001\n"),
            ("74\n0\n", "74\n1e-400\n"),
            ("2\nCUT\n70\n0\n", "2\nCUT\n70\n1e-400\n"),
            ("70\n0\n", "70\n32768\n"),
            ("74\n0\n", "74\n-0.0\n"),
        ] {
            let hostile = text.replacen(field, value, 1);
            let before = hostile.clone();
            assert!(read_dxf_file(&hostile).unwrap_err().to_string().contains("integer spelling"), "{value}");
            assert_eq!(hostile, before);
        }
        let signed = text
            .replacen("70\n0\n", "70\n+0\n", 1)
            .replacen("72\n65\n", "72\n+65\n", 1)
            .replacen("73\n2\n", "73\n+2\n", 1)
            .replacen("74\n0\n", "74\n-0\n", 1);
        assert!(read_dxf_file(&signed).is_ok());
        // Integer spelling alone does not make a different enum canonical.
        for (field, value) in [("70\n0\n", "70\n1\n"), ("72\n65\n", "72\n64\n"), ("73\n2\n", "73\n3\n"), ("74\n0\n", "74\n2\n")] {
            assert!(read_dxf_file(&text.replacen(field, value, 1)).is_err());
        }
    }

    #[test]
    fn layered_paper_flag_also_requires_integer_spelling_before_space_filtering() {
        for raw in ["1e-400", "0.0", "1e0", "32768", "0.5", "1.0000000000000001", "2", "-1"] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n67\n{raw}\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).is_err(), "{raw}");
        }
        let paper = "0\nSECTION\n2\nENTITIES\n0\nINSERT\n67\n1\n2\nPAPER_ONLY\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(paper).unwrap().entities().is_empty());
        let model = "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n67\n0\n10\n0\n20\n0\n40\n1\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(read_dxf_file(model).unwrap().entities().len(), 1);
        for repeated in ["67\n0\n67\n1\n", "67\n0\n67\n1e-400\n", "67\n0\n67\nnan\n"] {
            assert!(read_dxf_file(&model.replacen("67\n0\n", repeated, 1)).is_err());
        }
    }

    #[test]
    fn layered_integer_preflight_does_not_change_legacy_polyline_flag_parsing() {
        // The existing legacy parser accepts this float spelling for its
        // closed-polyline flag. Do not make section() globally lexical-i16.
        let text = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n70\n1.0\n90\n3\n10\n0\n20\n0\n10\n2\n20\n0\n10\n0\n20\n2\n0\nENDSEC\n0\nEOF\n";
        let geometry = read_dxf(text).unwrap();
        assert_eq!(geometry.len(), 3);
        assert_eq!(geometry[2], Geom2::Line(Vec2::new(0.0, 2.0), Vec2::ZERO));
    }

    #[test]
    fn layered_core_shapes_admit_neutral_z_thickness_and_ocs_but_refuse_projection() {
        let neutral = concat!(
            "0\nSECTION\n2\nENTITIES\n",
            "0\nLINE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n39\n0\n210\n0\n220\n0\n230\n1\n",
            "0\nCIRCLE\n10\n2\n20\n3\n30\n0\n40\n1\n39\n0\n210\n0\n220\n0\n230\n1\n",
            "0\nARC\n10\n0\n20\n0\n30\n0\n40\n1\n50\n0\n51\n90\n39\n0\n210\n0\n220\n0\n230\n1\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(neutral).unwrap();
        assert_eq!(file.entities().len(), 3);
        assert_eq!(file.entities()[0].geometry(), &DxfShape::Line(Vec2::ZERO, Vec2::new(1.0, 0.0)));
        assert_eq!(file.entities()[1].geometry(), &DxfShape::Circle { c: Vec2::new(2.0, 3.0), r: 1.0 });
        assert_eq!(file.entities()[2].geometry(), &DxfShape::Arc { c: Vec2::ZERO, r: 1.0, a0: 0.0, a1: 90.0 });
        for (field, changed) in [
            ("30\n0\n", "30\n2\n"),
            ("31\n0\n", "31\n2\n"),
            ("39\n0\n", "39\n1\n"),
            ("210\n0\n", "210\n1\n"),
            ("220\n0\n", "220\n1\n"),
            ("230\n1\n", "230\n-1\n"),
        ] {
            assert!(read_dxf_file(&neutral.replacen(field, changed, 1)).is_err());
        }
    }

    #[test]
    fn text_pairs_keep_only_group1_and3_values_raw() {
        let parsed = pairs(" 1 \n \t lead  \t\n 3 \n\n 2 \n ENTITIES \n 10 \n 1.25 \n 8 \n NOTES \n");
        assert_eq!(parsed, vec![(1, " \t lead  \t".into()), (3, String::new()), (2, "ENTITIES".into()), (10, "1.25".into()), (8, "NOTES".into()),]);
    }

    #[test]
    fn raw_mtext_fields_keep_order_empty_chunks_and_final_spaces() {
        let parsed = pairs("0\nSECTION\n2\nENTITIES\n0\nMTEXT\n3\n  first  \n3\n\n1\n last \n0\nENDSEC\n0\nEOF\n");
        let entities = section(&parsed, 1).unwrap();
        assert_eq!(entities.len(), 1);
        let raw: Vec<_> = entities[0].s.iter().filter(|(code, _)| matches!(*code, 1 | 3)).map(|(code, value)| (*code, value.as_str())).collect();
        assert_eq!(raw, [(3, "  first  "), (3, ""), (1, " last ")]);
        assert_eq!(entities[0].text(1), Some("last"), "legacy access stays normalized");
        assert_eq!(entities[0].text(3), Some("first"), "legacy access still takes the first requested field");
    }

    #[test]
    fn empty_group1_and_whitespace_only_text_remain_distinct_raw_values() {
        let parsed = pairs("0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n\n0\nTEXT\n1\n \t \n0\nENDSEC\n0\nEOF\n");
        let entities = section(&parsed, 1).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].s, [(1, String::new())]);
        assert_eq!(entities[1].s, [(1, " \t ".into())]);
        assert_eq!(entities[0].text(1), Some(""));
        assert_eq!(entities[1].text(1), Some(""));
    }

    #[test]
    fn crlf_delimiters_and_unicode_spaces_preserve_raw_but_not_legacy_padding() {
        let parsed = pairs("0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nTEXT\r\n1\r\n\u{2003} café \u{2003}\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n");
        let entities = section(&parsed, 1).unwrap();
        assert_eq!(entities[0].s, [(1, "\u{2003} café \u{2003}".into())]);
        assert_eq!(entities[0].text(1), Some("café"));
    }

    #[test]
    fn legacy_geometry_units_section_names_and_text_paragraph_policy_stay_current() {
        let text = "0\n SECTION \n2\n HEADER \n9\n $INSUNITS \n70\n 1 \n0\n ENDSEC \n0\n SECTION \n2\n ENTITIES \n0\n LINE \n8\n CUT \n10\n 1 \n20\n 2 \n11\n 2 \n21\n 1 \n0\n MTEXT \n10\n 1 \n20\n 2 \n40\n 3 \n3\n ignored prefix \n1\n  a\\Pb  \n0\n ENDSEC \n0\n EOF \n";
        let parsed = pairs(text);
        assert_eq!(units_scale(&parsed), 25.4);
        let start = parsed.iter().position(|(code, value)| *code == 2 && value == "ENTITIES").unwrap();
        let entities = section(&parsed, start).unwrap();
        assert_eq!(entities[0].kind, "LINE");
        assert_eq!(entities[0].text(8), Some("CUT"));
        assert_eq!(entities[0].get(10), Some(1.0));
        assert_eq!(entities[1].text(1), Some("a\\Pb"));
        let geometry = read_dxf(text).unwrap();
        assert_eq!(geometry.len(), 2);
        assert!(matches!(&geometry[0], Geom2::Line(a, b) if *a == Vec2::new(25.4, 50.8) && *b == Vec2::new(50.8, 25.4)));
        assert!(
            matches!(&geometry[1], Geom2::Text { text, at, height, angle } if text == "a\nb" && *at == Vec2::new(25.4, 50.8) && (*height - 76.2).abs() < 1e-12 && *angle == 0.0)
        );
    }

    #[test]
    fn existing_input_byte_cap_is_inclusive_before_parsing() {
        let prefix = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n999\n";
        let mut text = format!("{prefix}{}\n", " ".repeat(MAX_DRAWING_BYTES - prefix.len() - 1));
        assert_eq!(text.len(), MAX_DRAWING_BYTES);
        assert!(read_dxf(&text).unwrap().is_empty());
        text.push(' ');
        let error = read_dxf(&text).unwrap_err().to_string();
        assert!(error.contains("the DXF file is too large"), "{error}");
    }

    #[test]
    fn layered_straight_polyline_keeps_order_duplicates_units_and_layer_metadata() {
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nCUT\n62\n2\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\nCUT\n370\n-1\n90\n3\n70\n128\n",
            "10\n0\n20\n0\n42\n0\n10\n2\n20\n1\n10\n2\n20\n1\n0\nENDSEC\n0\nEOF\n",
        );
        for flags in ["128", "0"] {
            let text = text.replacen("70\n128\n", &format!("70\n{flags}\n"), 1);
            let before = text.clone();
            let file = read_dxf_file(&text).unwrap();
            assert_eq!(file.entities().len(), 1);
            let entity = &file.entities()[0];
            assert_eq!(file.layer(entity.layer()).unwrap().name(), "CUT");
            assert_eq!(file.layer(entity.layer()).unwrap().color(), 2);
            assert_eq!(entity.weight(), Some(DxfEntityWeight::ByLayer));
            assert_eq!(entity.geometry(), &DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::new(50.8, 25.4), Vec2::new(50.8, 25.4)] });
            assert_eq!(text, before);
        }
    }

    #[test]
    fn layered_polyline_dash_restart_uses_the_actual_layer_style_not_its_name() {
        let text = concat!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n2\nDASHED\n72\n65\n73\n2\n40\n19.05\n49\n12.7\n49\n-6.35\n0\nENDTAB\n",
            "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nContinuous\n62\n1\n6\nDASHED\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\nContinuous\n90\n2\n70\n128\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        assert_eq!(file.effective_linetype(0).unwrap(), DxfLinetype::Dashed);
        assert!(read_dxf_file(&text.replacen("70\n128\n", "70\n0\n", 1)).is_err());
        for replacement in ["", "70\n1\n", "70\n2\n"] {
            assert!(read_dxf_file(&text.replacen("70\n128\n", replacement, 1)).is_err());
        }
        // Exact formerly-refused129 input is now the admitted closed subset.
        let closed = read_dxf_file(&text.replacen("70\n128\n", "70\n129\n", 1)).unwrap();
        assert_eq!(closed.effective_linetype(0).unwrap(), DxfLinetype::Dashed);
        assert_eq!(
            closed.entities()[0].geometry(),
            &DxfShape::BulgedPolyline {
                vertices: vec![crate::DxfVertex { at: Vec2::ZERO, bulge: 0. }, crate::DxfVertex { at: Vec2::new(1., 1.), bulge: 0. },],
                closed: true
            }
        );
    }

    #[test]
    fn layered_polyline_refuses_bulges_widths_bad_counts_and_malformed_integers() {
        let base = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n2\n70\n128\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n0\nEOF\n";
        for (from, to) in [
            ("90\n2\n", "90\n1\n"),
            ("90\n2\n", "90\n-1\n"),
            ("90\n2\n", "90\n2147483647\n"),
            ("90\n2\n", "90\n2147483648\n"),
            ("90\n2\n", "90\n2.0000000000000001\n"),
            ("90\n2\n", "90\n2\n90\n2\n"),
            ("70\n128\n", "70\n128.0000000000000001\n"),
            ("70\n128\n", "70\n128\n70\n0\n"),
            ("20\n1\n", ""),
            ("10\n1\n", "10\nnan\n"),
            ("10\n1\n", "10\n1\n42\n1\n"),
            ("10\n1\n", "10\n1\n42\nnan\n"),
            ("10\n1\n", "10\n1\n40\n1\n"),
            ("10\n1\n", "10\n1\n38\n2\n"),
            ("10\n1\n", "10\n1\n43\n1\n"),
        ] {
            assert!(read_dxf_file(&base.replacen(from, to, 1)).is_err(), "{to}");
        }
        let over = base.replacen("90\n2\n", &format!("90\n{}\n", MAX_ENTITIES + 1), 1);
        assert!(read_dxf_file(&over).unwrap_err().to_string().contains("count/flag"));
    }

    #[test]
    fn layered_spline_preserves_degree_nine_knots_controls_and_raw_weight_metadata() {
        // Original degree-nine Bézier-equivalent spline in the parameter
        // interval[2,5]. Ten controls and twenty knots, with no degree clamp.
        let text = concat!(
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n370\n0\n70\n8\n71\n9\n72\n20\n73\n10\n74\n0\n",
            "40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n",
            "40\n5\n40\n5\n40\n5\n40\n5\n40\n5\n40\n5\n40\n5\n40\n5\n40\n5\n40\n5\n",
            "10\n0\n20\n0\n10\n1\n20\n1\n10\n2\n20\n0\n10\n3\n20\n1\n10\n4\n20\n0\n",
            "10\n5\n20\n1\n10\n6\n20\n0\n10\n7\n20\n1\n10\n8\n20\n0\n10\n9\n20\n1\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        let entity = &file.entities()[0];
        assert_eq!(entity.weight(), Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap())));
        assert_eq!(
            entity.geometry(),
            &DxfShape::Spline {
                degree: 9,
                knots: vec![2., 2., 2., 2., 2., 2., 2., 2., 2., 2., 5., 5., 5., 5., 5., 5., 5., 5., 5., 5.],
                ctrl: vec![
                    Vec2::new(0., 0.),
                    Vec2::new(1., 1.),
                    Vec2::new(2., 0.),
                    Vec2::new(3., 1.),
                    Vec2::new(4., 0.),
                    Vec2::new(5., 1.),
                    Vec2::new(6., 0.),
                    Vec2::new(7., 1.),
                    Vec2::new(8., 0.),
                    Vec2::new(9., 1.)
                ],
                weights: None,
            }
        );
        assert!(read_dxf_file(&text.replacen("71\n9\n", "71\n10\n", 1)).is_err());
    }

    fn original_rational_spline_input() -> &'static str {
        concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n70\n12\n71\n2\n72\n6\n73\n3\n74\n0\n",
            "40\n2\n40\n2\n40\n2\n40\n5\n40\n5\n40\n5\n41\n1\n41\n0.5\n41\n1\n",
            "10\n0\n20\n0\n30\n0\n10\n1\n20\n2\n30\n0\n10\n3\n20\n0\n30\n0\n",
            "210\n0\n220\n0\n230\n1\n0\nENDSEC\n0\nEOF\n",
        )
    }

    #[test]
    fn layered_rational_spline_scales_controls_only_and_preserves_weight_presence() {
        let text = original_rational_spline_input();
        let file = read_dxf_file(text).unwrap();
        assert_eq!(
            file.entities()[0].geometry(),
            &DxfShape::Spline {
                degree: 2,
                knots: vec![2., 2., 2., 5., 5., 5.],
                ctrl: vec![Vec2::ZERO, Vec2::new(10., 20.), Vec2::new(30., 0.)],
                weights: Some(vec![1., 0.5, 1.]),
            }
        );
        let ones = text.replacen("41\n0.5\n", "41\n1\n", 1);
        assert!(
            matches!(read_dxf_file(&ones).unwrap().entities()[0].geometry(), DxfShape::Spline { weights: Some(values), .. } if values == &[1.,1.,1.])
        );
        let unweighted = text.replacen("70\n12\n", "70\n8\n", 1).replace("41\n1\n", "").replace("41\n0.5\n", "");
        assert!(matches!(read_dxf_file(&unweighted).unwrap().entities()[0].geometry(), DxfShape::Spline { weights: None, .. }));
    }

    #[test]
    fn layered_spline_refuses_invalid_counts_flags_fit_data_and_nonpositive_weights() {
        let text = original_rational_spline_input();
        for (from, to) in [
            ("71\n2\n", "71\n0\n"),
            ("71\n2\n", "71\n2.0000000000000001\n"),
            ("72\n6\n", "72\n32768\n"),
            ("72\n6\n", "72\n7\n"),
            ("73\n3\n", "73\n2\n"),
            ("72\n6\n", "72\n6\n72\n6\n"),
            ("70\n12\n", "70\n13\n"),
            ("70\n12\n", "70\n14\n"),
            ("70\n12\n", "70\n28\n"),
            ("70\n12\n", ""),
            ("74\n0\n", "74\n1\n"),
            ("41\n0.5\n", "41\n0\n"),
            ("41\n0.5\n", "41\n-1\n"),
            ("41\n0.5\n", "41\nnan\n"),
            ("41\n0.5\n", ""),
            ("41\n0.5\n", "41\n0.5\n41\n1\n"),
            ("20\n2\n", "20\ninf\n"),
            ("30\n0\n", "30\n1\n"),
            ("30\n0\n", "30\nnan\n"),
            ("10\n1\n", "11\n1\n"),
            ("230\n1\n", "230\n-1\n"),
            ("74\n0\n", "74\n0\n12\n1\n"),
        ] {
            assert!(read_dxf_file(&text.replacen(from, to, 1)).is_err(), "{to}");
        }
    }

    #[test]
    fn layered_spline_refuses_decreasing_unclamped_empty_or_excess_endpoint_knots() {
        let text = original_rational_spline_input();
        for knots in [
            "40\n2\n40\n1\n40\n2\n40\n5\n40\n5\n40\n5\n",
            "40\n2\n40\n2\n40\n3\n40\n5\n40\n5\n40\n5\n",
            "40\n2\n40\n2\n40\n2\n40\n5\n40\n5\n40\n6\n",
            "40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n40\n2\n",
            "40\nnan\n40\n2\n40\n2\n40\n5\n40\n5\n40\n5\n",
        ] {
            let changed = text.replacen("40\n2\n40\n2\n40\n2\n40\n5\n40\n5\n40\n5\n", knots, 1);
            assert!(read_dxf_file(&changed).is_err());
        }
    }

    #[test]
    fn layered_geometry_budgets_are_cumulative_before_array_reservation() {
        let poly_pairs = pairs("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n2\n70\n128\n10\n0\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n");
        let poly = section(&poly_pairs, 1).unwrap();
        let spline_pairs = pairs(original_rational_spline_input());
        let start = layered_section_start(&spline_pairs, "ENTITIES").unwrap().unwrap();
        let spline = section(&spline_pairs, start).unwrap();
        let (poly, spline) = (&poly[0], &spline[0]);
        // This real rational shape costs3 point slots +6 knots +3 weights.
        // It fits alone, but cannot fit after another two-point shape when
        // only13 work slots remain. No large fixture allocation is needed.
        let mut bytes = 0;
        let mut work = MAX_WORK - 13;
        assert!(layered_geometry(spline, 1.0, DxfLinetype::Continuous, &mut bytes, &mut work).is_ok());
        let mut bytes = 0;
        let mut work = MAX_WORK - 13;
        assert!(layered_geometry(poly, 1.0, DxfLinetype::Continuous, &mut bytes, &mut work).is_ok());
        assert!(layered_geometry(spline, 1.0, DxfLinetype::Continuous, &mut bytes, &mut work).unwrap_err().to_string().contains("work budget"));
        let mut bytes = MAX_DRAWING_BYTES - 2 * std::mem::size_of::<Vec2>();
        let mut work = 0;
        assert!(layered_geometry(poly, 1.0, DxfLinetype::Continuous, &mut bytes, &mut work).is_ok());
        assert!(layered_geometry(spline, 1.0, DxfLinetype::Continuous, &mut bytes, &mut work).unwrap_err().to_string().contains("byte budget"));
        let mut bytes = 0;
        let mut work = 0;
        assert!(layered_array_budget::<Vec2>(usize::MAX, &mut bytes, &mut work).is_err());
    }

    #[test]
    fn layered_spline_keeps_legal_interior_knots_and_refuses_excess_multiplicity() {
        let text = concat!(
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n70\n8\n71\n2\n72\n8\n73\n5\n",
            "40\n0\n40\n0\n40\n0\n40\n0.5\n40\n0.5\n40\n1\n40\n1\n40\n1\n",
            "10\n0\n20\n0\n10\n1\n20\n1\n10\n2\n20\n0\n10\n3\n20\n1\n10\n4\n20\n0\n0\nENDSEC\n0\nEOF\n",
        );
        assert!(
            matches!(read_dxf_file(text).unwrap().entities()[0].geometry(), DxfShape::Spline { knots, .. } if knots == &[0.,0.,0.,0.5,0.5,1.,1.,1.])
        );
        let discontinuous = text
            .replacen("72\n8\n73\n5\n", "72\n9\n73\n6\n", 1)
            .replacen("40\n0.5\n40\n0.5\n", "40\n0.5\n40\n0.5\n40\n0.5\n", 1)
            .replacen("0\nENDSEC\n", "10\n5\n20\n1\n0\nENDSEC\n", 1);
        assert!(read_dxf_file(&discontinuous).unwrap_err().to_string().contains("multiplicity"));
        let excess_endpoint = text.replacen("40\n0.5\n", "40\n0\n", 1);
        assert!(read_dxf_file(&excess_endpoint).is_err());
    }

    #[test]
    fn layered_polyline_refuses_adjacent_distinct_vertices_collapsed_by_centimetres() {
        // Literal binary64 witnesses supplied by the independent source review.
        // At x=17.5, one output ULP is2^-48. Scaling the two distinct source
        // neighbours by10 yields offsets0.625 and1.25 outputULPs, both rounding
        // to the same+1ULP result under ordinary round-to-nearest arithmetic.
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n70\n128\n90\n2\n",
            "10\n1.7500000000000002\n20\n0\n10\n1.7500000000000004\n20\n0\n0\nENDSEC\n0\nEOF\n",
        );
        let before = text.to_owned();
        assert!(read_dxf_file(text).unwrap_err().to_string().contains("collapses adjacent distinct"));
        assert_eq!(text, before);
    }

    #[test]
    fn layered_spline_refuses_adjacent_distinct_controls_collapsed_by_centimetres() {
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n70\n8\n71\n1\n72\n4\n73\n2\n",
            "40\n0\n40\n0\n40\n1\n40\n1\n",
            "10\n1.7500000000000002\n20\n0\n10\n1.7500000000000004\n20\n0\n0\nENDSEC\n0\nEOF\n",
        );
        let before = text.to_owned();
        assert!(read_dxf_file(text).unwrap_err().to_string().contains("collapses adjacent distinct"));
        assert_eq!(text, before);
    }

    #[test]
    fn layered_unit_conversion_preserves_explicitly_repeated_vertices_and_controls() {
        let header = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n";
        let points = "10\n1.7500000000000002\n20\n0\n10\n1.7500000000000002\n20\n0\n0\nENDSEC\n0\nEOF\n";
        let repeated = vec![Vec2::new(17.500000000000004, 0.0), Vec2::new(17.500000000000004, 0.0)];
        let polyline = format!("{header}0\nLWPOLYLINE\n70\n128\n90\n2\n{points}");
        assert_eq!(read_dxf_file(&polyline).unwrap().entities()[0].geometry(), &DxfShape::Polyline { points: repeated.clone() });
        let spline = format!("{header}0\nSPLINE\n70\n8\n71\n1\n72\n4\n73\n2\n40\n0\n40\n0\n40\n1\n40\n1\n{points}");
        assert_eq!(
            read_dxf_file(&spline).unwrap().entities()[0].geometry(),
            &DxfShape::Spline { degree: 1, knots: vec![0., 0., 1., 1.], ctrl: repeated, weights: None }
        );
    }

    #[test]
    fn layered_point_uses_real_layer_weight_metadata_and_scales_inches_once() {
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nDATUMS\n62\n6\n6\nContinuous\n370\n50\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nPOINT\n8\ndatums\n370\n-1\n10\n2\n20\n-1\n30\n0\n210\n0\n220\n0\n230\n1\n",
            "0\nPOINT\n10\n0\n20\n0.5\n0\nENDSEC\n0\nEOF\n",
        );
        let before = text.to_owned();
        let file = read_dxf_file(text).unwrap();
        assert_eq!(file.entities().len(), 2);
        let point = &file.entities()[0];
        assert_eq!(point.geometry(), &DxfShape::Point(Vec2::new(50.8, -25.4)));
        assert_eq!(file.layer(point.layer()).unwrap().name(), "DATUMS");
        assert_eq!(file.layer(point.layer()).unwrap().color(), 6);
        assert_eq!(point.weight(), Some(DxfEntityWeight::ByLayer));
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap()));
        assert_eq!(file.effective_linetype(0).unwrap(), DxfLinetype::Continuous);
        assert_eq!(file.entities()[1].geometry(), &DxfShape::Point(Vec2::new(0.0, 12.7)));
        assert_eq!(file.entities()[1].layer(), file.default_layer());
        assert_eq!(file.entities()[1].weight(), None);
        assert_eq!(text, before);
    }

    #[test]
    fn layered_point_uses_each_existing_unit_factor_without_length_defaults() {
        for (unit, mm) in [("0", 1.0), ("1", 25.4), ("2", 304.8), ("4", 1.0), ("5", 10.0), ("6", 1000.0)] {
            let text = format!(
                "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n{unit}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n1\n20\n0\n0\nENDSEC\n0\nEOF\n"
            );
            assert_eq!(read_dxf_file(&text).unwrap().entities()[0].geometry(), &DxfShape::Point(Vec2::new(mm, 0.0)));
        }
        let zero = "0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(read_dxf_file(zero).unwrap().entities()[0].geometry(), &DxfShape::Point(Vec2::ZERO));
    }

    #[test]
    fn layered_point_refuses_missing_duplicate_malformed_or_nonfinite_coordinates() {
        for fields in [
            "",
            "10\n1\n",
            "20\n1\n",
            "10\n0\n10\n1\n20\n0\n",
            "10\n0\n20\n0\n20\nnan\n",
            "10\nnan\n20\n0\n",
            "10\n0\n20\ninf\n",
            "10\n-Infinity\n20\n0\n",
            "10\n1e400\n20\n0\n",
            "10\n1000000000000\n20\n0\n",
        ] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n{fields}0\nENDSEC\n0\nEOF\n");
            let before = text.clone();
            assert!(read_dxf_file(&text).is_err(), "{fields}");
            assert_eq!(text, before);
        }
    }

    #[test]
    fn layered_point_refuses_unrepresented_ocs_thickness_and_entity_styles_atomically() {
        for field in [
            "30\n1\n",
            "30\n0\n30\n0\n",
            "39\n1\n",
            "210\n1\n",
            "220\n1\n",
            "230\n0\n",
            "230\n-1\n",
            "230\nnan\n",
            "6\nByLayer\n",
            "62\n7\n",
            "60\n1\n",
            "50\n0\n",
            "40\n1\n",
        ] {
            // A valid preceding entity must not escape as a partial file.
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n0\n20\n0\n40\n1\n0\nPOINT\n10\n0\n20\n0\n{field}0\nENDSEC\n0\nEOF\n");
            assert!(read_dxf_file(&text).is_err(), "{field}");
        }
    }

    #[test]
    fn layered_point_keeps_paper_space_and_raw_symbolic_weights_distinct() {
        let paper = "0\nSECTION\n2\nENTITIES\n0\nPOINT\n67\n1\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(paper).unwrap().entities().is_empty());
        for (raw, expected) in [
            ("", None),
            ("370\n-1\n", Some(DxfEntityWeight::ByLayer)),
            ("370\n-2\n", Some(DxfEntityWeight::ByBlock)),
            ("370\n-3\n", Some(DxfEntityWeight::Default)),
            ("370\n0\n", Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap()))),
        ] {
            let text = format!("0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n1\n20\n2\n{raw}0\nENDSEC\n0\nEOF\n");
            assert_eq!(read_dxf_file(&text).unwrap().entities()[0].weight(), expected);
        }
        let malformed = "0\nSECTION\n2\nENTITIES\n0\nPOINT\n67\n0.5\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(malformed).is_err());
    }

    #[test]
    fn layered_zero_color_and_physical_weight_are_supported_now_not_discarded() {
        // Exact two input rows and format string moved from the old refusal
        // test under Engineer's explicit feature-contract update. No waiver.
        for (name, fields, color, weight) in [("0", "62\n3\n", 3, None), ("0", "370\n50\n", 7, Some(50))] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n{name}\n{fields}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
            );
            let before = text.clone();
            let mut file = read_dxf_file(&text).unwrap();
            assert_eq!(file.layers().len(), 1);
            let handle = file.default_layer();
            assert_eq!(file.layers()[0].name(), "0");
            let zero = file.layer(handle).unwrap();
            assert_eq!(
                (zero.name(), zero.color(), zero.linetype(), zero.weight().map(DxfStandardWeight::code)),
                ("0", color, DxfLinetype::Continuous, weight)
            );
            assert!(file.entities().is_empty());
            file.push(handle, DxfShape::Line(Vec2::ZERO, Vec2::new(1., 0.)), None).unwrap();
            assert_eq!(file.entities()[0].layer(), handle);
            assert_eq!(
                file.effective_weight(0).unwrap(),
                weight.map_or(DxfEntityWeight::Default, |code| DxfEntityWeight::Explicit(DxfStandardWeight::from_code(code).unwrap()))
            );
            let recovered = read_dxf_file(&crate::write_dxf_file(&file).unwrap()).unwrap();
            let zero = recovered.layer(recovered.default_layer()).unwrap();
            assert_eq!((zero.color(), zero.weight().map(DxfStandardWeight::code)), (color, weight));
            assert_eq!(recovered.entities()[0].layer(), recovered.default_layer());
            assert_eq!(text, before);
        }
    }

    #[test]
    fn layered_zero_is_first_even_when_declared_between_layers_with_linetypes_afterwards() {
        let text = concat!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n",
            "0\nLAYER\n2\nCUT\n62\n1\n0\nLAYER\n2\n0\n62\n3\n6\nHIDDEN\n370\n0\n",
            "0\nLAYER\n2\nTAIL\n62\n2\n6\nDASHED\n0\nENDTAB\n",
            "0\nTABLE\n2\nLTYPE\n0\nLTYPE\n2\nHIDDEN\n72\n65\n73\n2\n40\n3\n49\n2\n49\n-1\n",
            "0\nLTYPE\n2\nDASHED\n72\n65\n73\n2\n40\n19.05\n49\n12.7\n49\n-6.35\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n1\n20\n2\n",
            "0\nLINE\n8\n0\n370\n-1\n10\n0\n20\n0\n11\n1\n21\n0\n",
            "0\nCIRCLE\n8\nCUT\n370\n-2\n10\n0\n20\n0\n40\n1\n0\nINSERT\n67\n1\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        assert_eq!(file.layers().iter().map(|layer| layer.name()).collect::<Vec<_>>(), ["0", "CUT", "TAIL"]);
        assert_eq!(file.entities().len(), 3);
        assert_eq!(file.entities()[0].layer(), file.default_layer());
        assert_eq!(file.entities()[1].layer(), file.default_layer());
        assert_eq!(file.layer(file.default_layer()).unwrap().color(), 3);
        assert_eq!(file.effective_linetype(0).unwrap(), DxfLinetype::Hidden);
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap()));
        assert_eq!(file.entities()[0].weight(), None);
        assert_eq!(file.entities()[1].weight(), Some(DxfEntityWeight::ByLayer));
        assert_eq!(file.entities()[2].weight(), Some(DxfEntityWeight::ByBlock));
        assert_eq!(file.layer(file.entities()[2].layer()).unwrap().name(), "CUT");
        assert_eq!(file.entities()[0].geometry(), &DxfShape::Point(Vec2::new(1., 2.)));
    }

    #[test]
    fn layered_zero_weight_omission_and_explicit_physical_zero_remain_distinct() {
        for (field, expected) in [("", None), ("370\n0\n", Some(0))] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n{field}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOINT\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n"
            );
            let file = read_dxf_file(&text).unwrap();
            assert_eq!(file.layers()[0].weight().map(DxfStandardWeight::code), expected);
            assert_eq!(file.entities()[0].weight(), None);
            assert_eq!(
                file.effective_weight(0).unwrap(),
                expected.map_or(DxfEntityWeight::Default, |code| DxfEntityWeight::Explicit(DxfStandardWeight::from_code(code).unwrap()))
            );
        }
    }

    #[test]
    fn layered_zero_pattern_is_real_and_polyline_restart_cannot_assume_continuous() {
        let text = concat!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n2\nDASHED\n72\n65\n73\n2\n40\n19.05\n49\n12.7\n49\n-6.35\n0\nENDTAB\n",
            "0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n6\nDASHED\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n2\n70\n128\n10\n0\n20\n0\n10\n1\n20\n0\n0\nENDSEC\n0\nEOF\n",
        );
        assert_eq!(read_dxf_file(text).unwrap().effective_linetype(0).unwrap(), DxfLinetype::Dashed);
        assert!(read_dxf_file(&text.replacen("70\n128\n", "70\n0\n", 1)).is_err());
        assert!(read_dxf_file(&text.replacen("49\n12.7\n", "49\n1\n", 1)).is_err());
    }

    #[test]
    fn layered_zero_duplicate_aliases_and_other_layer_collisions_refuse_explicitly() {
        for declarations in [
            "0\nLAYER\n2\n0\n0\nLAYER\n2\n0\n",
            "0\nLAYER\n2\n0\n62\n3\n0\nLAYER\n2\n0\n62\n4\n",
            "0\nLAYER\n2\n\n0\nLAYER\n2\n0\n",
            "0\nLAYER\n2\n\n0\nLAYER\n2\n\n",
            "0\nLAYER\n2\nCUT\n0\nLAYER\n2\nCUT\n",
            "0\nLAYER\n2\nCUT\n0\nLAYER\n2\ncut\n",
            "0\nLAYER\n2\nCUT?A\n0\nLAYER\n2\nCUT_A\n",
        ] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n{declarations}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
            );
            assert!(read_dxf_file(&text).is_err(), "{declarations}");
        }
    }

    #[test]
    fn layered_zero_malformed_or_unrepresented_metadata_never_falls_back_to_defaults() {
        for fields in [
            "62\n7.0000000000000001\n",
            "62\n0\n",
            "62\n-7\n",
            "62\n256\n",
            "62\n7\n62\n3\n",
            "370\n-1\n",
            "370\n-2\n",
            "370\n-3\n",
            "370\n6\n",
            "370\n50.0000000000000001\n",
            "370\n0\n370\n50\n",
            "70\n1e-400\n",
            "6\nDASHED\n",
            "6\nCUSTOM\n",
            "2\n0\n",
        ] {
            let text = format!(
                "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n{fields}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"
            );
            assert!(read_dxf_file(&text).is_err(), "{fields}");
        }
    }

    #[test]
    fn layered_zero_empty_tables_or_absence_preserve_new_default_while_single_blank_name_aliases_zero() {
        for tables in ["", "0\nSECTION\n2\nTABLES\n0\nENDSEC\n", "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nENDTAB\n0\nENDSEC\n"] {
            let text = format!("{tables}0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n");
            let file = read_dxf_file(&text).unwrap();
            let zero = file.layer(file.default_layer()).unwrap();
            assert_eq!(file.layers().len(), 1);
            assert_eq!((zero.name(), zero.color(), zero.linetype(), zero.weight()), ("0", 7, DxfLinetype::Continuous, None));
            assert!(file.entities().is_empty());
        }
        let blank =
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n\n62\n3\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(read_dxf_file(blank).unwrap().layers()[0].color(), 3);
        assert!(read_dxf_file("").is_err());
    }

    #[test]
    fn layered_ellipse_scales_center_and_relative_major_once_with_raw_style_metadata() {
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nELL\n62\n4\n370\n50\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n8\nELL\n370\n-1\n",
            "10\n10\n20\n20\n30\n0\n11\n3\n21\n4\n31\n0\n40\n0.5\n41\n1\n42\n2\n210\n0\n220\n0\n230\n1\n0\nENDSEC\n0\nEOF\n",
        );
        let before = text.to_owned();
        let file = read_dxf_file(text).unwrap();
        let entity = &file.entities()[0];
        assert_eq!(entity.geometry(), &DxfShape::Ellipse { c: Vec2::new(100., 200.), major: Vec2::new(30., 40.), ratio: 0.5, t0: 1., t1: 2. });
        assert_eq!(file.layer(entity.layer()).unwrap().name(), "ELL");
        assert_eq!(file.layer(entity.layer()).unwrap().color(), 4);
        assert_eq!(entity.weight(), Some(DxfEntityWeight::ByLayer));
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap()));
        assert_eq!(text, before);
    }

    #[test]
    fn layered_ellipse_preserves_full_encoding_wrapped_arc_and_circle_ratio_as_ellipse() {
        let encoded_tau = "6.28318530718".parse::<f64>().unwrap();
        for (start, end, ratio, t0, t1) in [
            ("0", "6.283185307179586", "0.5", 0., std::f64::consts::TAU),
            ("0", "6.28318530718", "0.5", 0., encoded_tau),
            ("5", "1", "0.5", 5., 1.),
            ("0.25", "1.75", "1", 0.25, 1.75),
        ] {
            let text =
                format!("0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n2\n20\n3\n11\n2\n21\n0\n40\n{ratio}\n41\n{start}\n42\n{end}\n0\nENDSEC\n0\nEOF\n");
            let expected_ratio = if ratio == "1" { 1.0 } else { 0.5 };
            assert_eq!(
                read_dxf_file(&text).unwrap().entities()[0].geometry(),
                &DxfShape::Ellipse { c: Vec2::new(2., 3.), major: Vec2::new(2., 0.), ratio: expected_ratio, t0, t1 }
            );
        }
    }

    #[test]
    fn layered_ellipse_parameter_bound_accepts_exact_writer_encoding_and_refuses_next_value() {
        let encoded_tau = "6.28318530718".parse::<f64>().unwrap();
        let base = "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n0\n20\n0\n11\n2\n21\n0\n40\n0.5\n41\n0\n42\n6.28318530718\n0\nENDSEC\n0\nEOF\n";
        let file = read_dxf_file(base).unwrap();
        let DxfShape::Ellipse { t1, .. } = file.entities()[0].geometry() else { panic!("expected ellipse") };
        assert_eq!(*t1, encoded_tau);
        assert_ne!(*t1, std::f64::consts::TAU, "never normalize the stored encoding");
        // Independent IEEE adjacent-value fixture, not generated by our DXF
        // writer/reader. Seventeen fractional digits round-trip this magnitude.
        let outside = format!("{:.17}", encoded_tau.next_up());
        assert!(read_dxf_file(&base.replacen("6.28318530718", &outside, 1)).is_err());
        assert!(read_dxf_file(&base.replacen("6.28318530718", "6.283185307180001", 1)).is_err());
        // No arbitrary interval is admitted between TAU and its encoded value.
        let gap = format!("{:.17}", std::f64::consts::TAU.next_up());
        assert!(read_dxf_file(&base.replacen("6.28318530718", &gap, 1)).is_err());
    }

    #[test]
    fn layered_ellipse_real_producer_full_spelling_is_kept_without_recovering_original_tau() {
        let encoded_tau = "6.28318530718".parse::<f64>().unwrap();
        let mut source = DxfFile::new().unwrap();
        source
            .push(
                source.default_layer(),
                DxfShape::Ellipse { c: Vec2::new(2., 3.), major: Vec2::new(4., 0.), ratio: 0.5, t0: 0., t1: std::f64::consts::TAU },
                None,
            )
            .unwrap();
        let written = crate::write_dxf_file(&source).unwrap();
        let file = read_dxf_file(&written).unwrap();
        assert_eq!(
            file.entities()[0].geometry(),
            &DxfShape::Ellipse { c: Vec2::new(2., 3.), major: Vec2::new(4., 0.), ratio: 0.5, t0: 0., t1: encoded_tau }
        );
        assert_eq!(file.entities()[0].weight(), None);
    }

    #[test]
    fn layered_ellipse_rejects_missing_duplicate_malformed_nonfinite_and_nonplanar_fields() {
        let base = "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n0\n20\n0\n11\n2\n21\n0\n40\n0.5\n41\n1\n42\n2\n0\nENDSEC\n0\nEOF\n";
        for (from, to) in [
            ("10\n0\n", ""),
            ("20\n0\n", ""),
            ("11\n2\n", ""),
            ("21\n0\n", ""),
            ("40\n0.5\n", ""),
            ("41\n1\n", ""),
            ("42\n2\n", ""),
            ("10\n0\n", "10\n0\n10\n1\n"),
            ("11\n2\n", "11\n2\n11\nnan\n"),
            ("40\n0.5\n", "40\n0.5\n40\n0.5\n"),
            ("41\n1\n", "41\n1\n41\n1\n"),
            ("42\n2\n", "42\n2\n42\n2\n"),
            ("10\n0\n", "10\nnan\n"),
            ("11\n2\n", "11\ninf\n"),
            ("40\n0.5\n", "40\n1e400\n"),
            ("41\n1\n", "41\nnan\n"),
            ("42\n2\n", "42\ninf\n"),
            ("11\n2\n", "11\n2\n30\n1\n"),
            ("11\n2\n", "11\n2\n31\n1\n"),
            ("11\n2\n", "11\n2\n230\n-1\n"),
            ("11\n2\n", "11\n2\n210\n1\n"),
            ("11\n2\n", "11\n2\n39\n1\n"),
            ("11\n2\n", "11\n2\n50\n0\n"),
        ] {
            let text = base.replacen(from, to, 1);
            let before = text.clone();
            assert!(read_dxf_file(&text).is_err(), "{to}");
            assert_eq!(text, before);
        }
    }

    #[test]
    fn layered_ellipse_rejects_degenerate_axes_ratios_and_ambiguous_or_multiturn_parameters() {
        let base = "0\nSECTION\n2\nENTITIES\n0\nELLIPSE\n10\n0\n20\n0\n11\n2\n21\n0\n40\n0.5\n41\n1\n42\n2\n0\nENDSEC\n0\nEOF\n";
        for (from, to) in [
            ("11\n2\n", "11\n0\n"),
            ("11\n2\n", "11\n5e-324\n"),
            ("40\n0.5\n", "40\n0\n"),
            ("40\n0.5\n", "40\n-0.5\n"),
            ("40\n0.5\n", "40\n1.0001\n"),
            ("40\n0.5\n", "40\n1e-400\n"),
            ("41\n1\n", "41\n2\n"),
            ("41\n1\n", "41\n-1\n"),
            ("42\n2\n", "42\n12.566370614359172\n"),
            ("41\n1\n42\n2\n", "41\n6.283185307179586\n42\n0\n"),
            ("41\n1\n42\n2\n", "41\n6.28318530718\n42\n0\n"),
        ] {
            assert!(read_dxf_file(&base.replacen(from, to, 1)).is_err(), "{to}");
        }
        // Vec2::len uses hypot, not sqrt(x*x+y*y). This small normal axis
        // and its5e-301 minor remain positive and must not be rejected.
        let small_valid = base.replacen("11\n2\n", "11\n1e-300\n", 1);
        assert_eq!(
            read_dxf_file(&small_valid).unwrap().entities()[0].geometry(),
            &DxfShape::Ellipse { c: Vec2::ZERO, major: Vec2::new(1e-300, 0.), ratio: 0.5, t0: 1., t1: 2. }
        );
        // Decimal5e-324 rounds to binary64's least positive subnormal2^-1074.
        // Multiplication by0.5 is exactly halfway to zero, and RNE chooses
        // the even zero significand; both minor-vector components become zero.
        // The negative row above is therefore a genuine represented collapse.
        let collapsed_minor = base.replacen("11\n2\n", "11\n1e-100\n", 1).replacen("40\n0.5\n", "40\n1e-300\n", 1);
        assert!(read_dxf_file(&collapsed_minor).unwrap_err().to_string().contains("minor axis collapses"));
    }

    #[test]
    fn parent_text_parser_handles_all_nine_modes_units_and_raw_padding_with_actual_anchors() {
        use crate::DxfTextHorizontalAlignment::{Centre, Left, Right};
        use crate::DxfTextVerticalAlignment::{Baseline, Middle, Top};
        for (h, v, horizontal, vertical) in [
            (0, 0, Left, Baseline),
            (1, 0, Centre, Baseline),
            (2, 0, Right, Baseline),
            (0, 2, Left, Middle),
            (1, 2, Centre, Middle),
            (2, 2, Right, Middle),
            (0, 3, Left, Top),
            (1, 3, Centre, Top),
            (2, 3, Right, Top),
        ] {
            let text = format!(
                "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n99\n20\n88\n11\n1\n21\n2\n40\n0.5\n50\n-90\n72\n{h}\n73\n{v}\n1\n  A  \n0\nENDSEC\n0\nEOF\n"
            );
            let before = text.clone();
            let file = read_dxf_file(&text).unwrap();
            let at = if h == 0 && v == 0 { Vec2::new(990., 880.) } else { Vec2::new(10., 20.) };
            let expected = if h == 1 && v == 0 {
                DxfShape::Text { at, height: 5., angle_deg: -90., text: "  A  ".into() }
            } else {
                DxfShape::AlignedText { at, height: 5., angle_deg: -90., text: "  A  ".into(), horizontal, vertical }
            };
            assert_eq!(file.entities()[0].geometry(), &expected, "72={h},73={v}");
            assert_eq!(file.entities()[0].layer(), file.default_layer());
            assert_eq!(text, before);
        }
    }

    #[test]
    fn parent_text_defaults_and_cif_decoder_preserve_real_layer_and_weight_metadata() {
        let text = concat!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nNOTES\n62\n6\n370\n50\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\nnotes\n370\n-1\n10\n2\n20\n3\n40\n1\n1\n  %%d %%p %%c \\U+4E2D  \n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        let entity = &file.entities()[0];
        assert_eq!(
            entity.geometry(),
            &DxfShape::AlignedText {
                at: Vec2::new(2., 3.),
                height: 1.,
                angle_deg: 0.,
                text: "  ° ± ∅ 中  ".into(),
                horizontal: crate::DxfTextHorizontalAlignment::Left,
                vertical: crate::DxfTextVerticalAlignment::Baseline,
            }
        );
        assert_eq!((file.layer(entity.layer()).unwrap().name(), file.layer(entity.layer()).unwrap().color()), ("NOTES", 6));
        assert_eq!(entity.weight(), Some(DxfEntityWeight::ByLayer));
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap()));
    }

    #[test]
    fn parent_text_raw_decoding_does_not_migrate_legacy_trim_or_anchor_semantics() {
        let text = "0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n99\n20\n88\n11\n1\n21\n2\n40\n1\n72\n1\n1\n  A  \n0\nENDSEC\n0\nEOF\n";
        assert_eq!(
            read_dxf_file(text).unwrap().entities()[0].geometry(),
            &DxfShape::Text { at: Vec2::new(1., 2.), height: 1., angle_deg: 0., text: "  A  ".into() }
        );
        assert_eq!(read_dxf(text).unwrap(), vec![Geom2::Text { text: "A".into(), at: Vec2::new(99., 88.), height: 1., angle: 0. }]);
    }

    #[test]
    fn parent_text_exact_cif_caps_allow_250_bmp_and_refuse_251_without_truncation() {
        let encoded = "\\U+4E2D".repeat(250);
        let text = format!("0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n0\n20\n0\n11\n1\n21\n2\n40\n1\n72\n1\n1\n{encoded}\n0\nENDSEC\n0\nEOF\n");
        assert_eq!(encoded.len(), 1750);
        let file = read_dxf_file(&text).unwrap();
        assert_eq!(file.entities()[0].geometry(), &DxfShape::Text { at: Vec2::new(1., 2.), height: 1., angle_deg: 0., text: "中".repeat(250) });
        for too_many in ["\\U+4E2D".repeat(251), "A".repeat(251)] {
            let hostile = text.replacen(&encoded, &too_many, 1);
            assert!(read_dxf_file(&hostile).is_err());
        }
    }

    #[test]
    fn parent_text_work_budget_is_shared_across_the_whole_actual_file() {
        // Each entity charges1750 encoded +750 decoded bytes plus the parent
        // entity/layer visits. Under existing MAX_WORK800000,319 fit,320 do not.
        // Input is below1MiB: no giant resource fixture or counter injection.
        let encoded = "\\U+4E2D".repeat(250);
        let entity = format!("0\nTEXT\n10\n0\n20\n0\n11\n1\n21\n2\n40\n1\n72\n1\n1\n{encoded}\n");
        let accepted_input = format!("0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", entity.repeat(319));
        let file = read_dxf_file(&accepted_input).unwrap();
        assert_eq!(file.entities().len(), 319);
        assert!(matches!(file.entities().last().unwrap().geometry(), DxfShape::Text { text, .. } if text == &"中".repeat(250)));
        let refused_input = format!("0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n", entity.repeat(320));
        let before = refused_input.clone();
        assert!(read_dxf_file(&refused_input).unwrap_err().to_string().contains("budget"));
        assert_eq!(refused_input, before);
    }

    #[test]
    fn parent_text_lexical_and_geometry_negatives_styles_mtext_and_paper_rules_are_real() {
        let base = "0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n0\n20\n0\n11\n1\n21\n2\n40\n1\n72\n1\n73\n0\n1\nA\n0\nENDSEC\n0\nEOF\n";
        for (from, to) in [
            ("72\n1\n", "72\n1.0000000000000000001\n"),
            ("72\n1\n", "72\n32768\n"),
            ("72\n1\n", "72\n999999999999999999999\n"),
            ("73\n0\n", "73\n1e-400\n"),
            ("73\n0\n", "73\n0.0\n"),
            ("73\n0\n", "73\n0\n71\n1e-400\n"),
        ] {
            assert!(read_dxf_file(&base.replacen(from, to, 1)).unwrap_err().to_string().contains("integer spelling"));
        }
        for (from, to) in [
            ("10\n0\n", ""),
            ("11\n1\n", ""),
            ("21\n2\n", "21\nnan\n"),
            ("40\n1\n", "40\n0\n"),
            ("1\nA\n", "1\nA\n7\nSTANDARD\n"),
            ("1\nA\n", "1\nA\n7\nCustom\n"),
            ("1\nA\n", "1\nA\n6\nByLayer\n"),
            ("1\nA\n", "1\nA\n230\n-1\n"),
            ("72\n1\n", "72\n3\n"),
            ("73\n0\n", "73\n1\n"),
            ("1\nA\n", "1\nA\n1\nB\n"),
            ("1\nA\n", "1\n\\U+D800\n"),
            ("1\nA\n", "1\n\\U+0025\\U+0025d\n"),
        ] {
            assert!(read_dxf_file(&base.replacen(from, to, 1)).is_err(), "{to}");
        }
        assert!(read_dxf_file(&base.replacen("0\nTEXT\n", "0\nMTEXT\n", 1)).unwrap_err().to_string().contains("MTEXT"));
        let paper = base.replacen("0\nTEXT\n", "0\nMTEXT\n67\n1\n", 1);
        assert!(read_dxf_file(&paper).unwrap().entities().is_empty());
    }

    #[test]
    fn parent_text_real_writer_roundtrips_nine_empty_shapes_with_explicit_justified_bounds() {
        use crate::DxfTextHorizontalAlignment::{Centre, Left, Right};
        use crate::DxfTextVerticalAlignment::{Baseline, Middle, Top};
        let modes = [
            (Left, Baseline),
            (Centre, Baseline),
            (Right, Baseline),
            (Left, Middle),
            (Centre, Middle),
            (Right, Middle),
            (Left, Top),
            (Centre, Top),
            (Right, Top),
        ];
        let mut source = DxfFile::new().unwrap();
        let layer = source.declare_layer("LABEL", 2, DxfLinetype::Continuous, Some(DxfStandardWeight::from_code(50).unwrap())).unwrap();
        for (horizontal, vertical) in modes {
            source
                .push(
                    layer,
                    DxfShape::AlignedText { at: Vec2::new(2., 3.), height: 1., angle_deg: 0., text: String::new(), horizontal, vertical },
                    Some(DxfEntityWeight::ByLayer),
                )
                .unwrap();
        }
        assert!(crate::write_dxf_file(&source).is_err(), "AlignedText requires a provider");
        let mut calls = 0;
        let written = crate::write_dxf_file_with_text_bounds(&source, |shape| {
            // Empty payload has no glyph ink. This single-point world box
            // conservatively encloses the EMPTY set for this exact fixture;
            // it is not a font, measured layout or a general text provider.
            match shape {
                DxfShape::AlignedText { at, height, angle_deg, text, .. }
                    if *at == Vec2::new(2., 3.) && *height == 1. && *angle_deg == 0. && text.is_empty() =>
                {
                    calls += 1;
                    crate::DxfGeometryBounds::new(Vec2::new(2., 3.), Vec2::new(2., 3.))
                }
                _ => Err(IoError::Invalid("fixture bounds are only justified for the literal empty TEXT".into())),
            }
        })
        .unwrap();
        assert_eq!(calls, 9);
        let file = read_dxf_file(&written).unwrap();
        assert_eq!(file.entities().len(), 9);
        for (entity, (horizontal, vertical)) in file.entities().iter().zip(modes) {
            let expected = if horizontal == Centre && vertical == Baseline {
                DxfShape::Text { at: Vec2::new(2., 3.), height: 1., angle_deg: 0., text: String::new() }
            } else {
                DxfShape::AlignedText { at: Vec2::new(2., 3.), height: 1., angle_deg: 0., text: String::new(), horizontal, vertical }
            };
            assert_eq!(entity.geometry(), &expected);
            let layer = file.layer(entity.layer()).unwrap();
            assert_eq!((layer.name(), layer.color(), layer.weight().map(DxfStandardWeight::code)), ("LABEL", 2, Some(50)));
            assert_eq!(entity.weight(), Some(DxfEntityWeight::ByLayer));
        }
    }

    fn original_layered_bulged_input() -> &'static str {
        "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n2\n70\n128\n10\n0\n20\n0\n42\n0.5\n10\n4\n20\n0\n42\n0\n0\nENDSEC\n0\nEOF\n"
    }

    #[test]
    fn layered_bulged_sparse_outgoing_signs_scale_once_and_keep_actual_metadata() {
        let text = concat!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n",
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\nBENDS\n62\n6\n370\n50\n0\nENDTAB\n0\nENDSEC\n",
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\nbends\n370\n-1\n90\n4\n70\n128\n",
            "10\n0\n42\n0.5\n20\n0\n10\n4\n20\n0\n10\n4\n20\n4\n42\n-0.5\n10\n0\n20\n4\n42\n-0.0\n0\nENDSEC\n0\nEOF\n",
        );
        let file = read_dxf_file(text).unwrap();
        let entity = &file.entities()[0];
        let DxfShape::BulgedPolyline { vertices, closed } = entity.geometry() else { panic!("expected actual bulged shape") };
        assert!(!closed);
        assert_eq!(
            vertices,
            &vec![
                crate::DxfVertex { at: Vec2::ZERO, bulge: 0.5 },
                crate::DxfVertex { at: Vec2::new(40., 0.), bulge: 0. },
                crate::DxfVertex { at: Vec2::new(40., 40.), bulge: -0.5 },
                crate::DxfVertex { at: Vec2::new(0., 40.), bulge: -0. },
            ]
        );
        assert_eq!(vertices[3].bulge.to_bits(), (-0.0_f64).to_bits());
        assert_eq!((file.layer(entity.layer()).unwrap().name(), file.layer(entity.layer()).unwrap().color()), ("BENDS", 6));
        assert_eq!(entity.weight(), Some(DxfEntityWeight::ByLayer));
        assert_eq!(file.effective_weight(0).unwrap(), DxfEntityWeight::Explicit(DxfStandardWeight::from_code(50).unwrap()));
        let physical_zero = read_dxf_file(&text.replacen("370\n-1\n", "370\n0\n", 1)).unwrap();
        assert_eq!(physical_zero.entities()[0].weight(), Some(DxfEntityWeight::Explicit(DxfStandardWeight::from_code(0).unwrap())));
    }

    #[test]
    fn layered_bulged_closed_semicircles_use_last_outgoing_bulge_without_duplicate_seam() {
        for sign in ["1", "-1"] {
            let text = format!(
                "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n2\n70\n129\n10\n-2\n20\n0\n42\n{sign}\n10\n2\n20\n0\n42\n{sign}\n0\nENDSEC\n0\nEOF\n"
            );
            let file = read_dxf_file(&text).unwrap();
            let bulge = if sign == "1" { 1. } else { -1. };
            assert_eq!(
                file.entities()[0].geometry(),
                &DxfShape::BulgedPolyline {
                    vertices: vec![crate::DxfVertex { at: Vec2::new(-2., 0.), bulge }, crate::DxfVertex { at: Vec2::new(2., 0.), bulge },],
                    closed: true
                }
            );
            assert!(read_dxf_file(&text.replacen("70\n129\n", "70\n1\n", 1)).is_err());
            assert!(read_dxf_file(&text.replacen("70\n129\n", "70\n128\n", 1)).is_err());
            assert!(read_dxf_file(&text.replacen("10\n2\n", "10\n-2\n", 1)).is_err());
        }
        // An authored repeated zero-bulge neighbor is retained inside a closed
        // polyline; only the duplicated first/last seam is disallowed.
        let repeated = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n3\n70\n129\n10\n0\n20\n0\n10\n0\n20\n0\n10\n2\n20\n0\n0\nENDSEC\n0\nEOF\n";
        assert!(
            matches!(read_dxf_file(repeated).unwrap().entities()[0].geometry(), DxfShape::BulgedPolyline { vertices, closed: true } if vertices.len() == 3 && vertices[0].at == vertices[1].at)
        );
    }

    #[test]
    fn layered_bulged_real_producer_roundtrip_keeps_literal_vertices_closure_and_weights() {
        for closed in [false, true] {
            let expected = vec![
                crate::DxfVertex { at: Vec2::new(-3., 0.), bulge: 1. },
                crate::DxfVertex { at: Vec2::new(3., 0.), bulge: 0. },
                crate::DxfVertex { at: Vec2::new(3., 4.), bulge: -0.5 },
                crate::DxfVertex { at: Vec2::new(0., 4.), bulge: if closed { 0.5 } else { 0. } },
            ];
            let mut source = DxfFile::with_layer_zero(2, DxfLinetype::Continuous, Some(DxfStandardWeight::from_code(50).unwrap())).unwrap();
            let mut input_vertices = expected.clone();
            // Actual producer precise_num emits either zero sign as "0.0".
            // Raw negative-zero input is covered separately by the reader test.
            if !closed {
                input_vertices.last_mut().unwrap().bulge = -0.0;
            }
            source
                .push(source.default_layer(), DxfShape::BulgedPolyline { vertices: input_vertices, closed }, Some(DxfEntityWeight::ByBlock))
                .unwrap();
            let written = crate::write_dxf_file(&source).unwrap();
            let before = written.clone();
            let file = read_dxf_file(&written).unwrap();
            let entity = &file.entities()[0];
            assert_eq!(entity.geometry(), &DxfShape::BulgedPolyline { vertices: expected.clone(), closed });
            let DxfShape::BulgedPolyline { vertices, .. } = entity.geometry() else { panic!("expected actual bulged shape") };
            assert_eq!(vertices.last().unwrap().bulge.to_bits(), expected.last().unwrap().bulge.to_bits());
            assert_eq!(entity.weight(), Some(DxfEntityWeight::ByBlock));
            assert_eq!(entity.layer(), file.default_layer());
            assert_eq!(file.layer(file.default_layer()).unwrap().color(), 2);
            assert_eq!(file.layer(file.default_layer()).unwrap().weight().unwrap().code(), 50);
            assert_eq!(written, before);
        }
    }

    #[test]
    fn layered_bulged_refuses_bad_components_counts_flags_and_unrepresentable_arcs() {
        let base = original_layered_bulged_input();
        for (from, to) in [
            ("90\n2\n", "90\n1\n"),
            ("90\n2\n", "90\n3\n"),
            ("90\n2\n", "90\n2\n90\n2\n"),
            ("90\n2\n", "90\n2.0000000000000001\n"),
            ("90\n2\n", "90\n2147483648\n"),
            ("70\n128\n", ""),
            ("70\n128\n", "70\n0\n"),
            ("70\n128\n", "70\n1\n"),
            ("70\n128\n", "70\n130\n"),
            ("70\n128\n", "70\n128\n70\n128\n"),
            ("70\n128\n", "70\n128.0000000000000001\n"),
            ("10\n0\n", "20\n0\n10\n0\n"),
            ("10\n0\n", "42\n0\n10\n0\n"),
            ("20\n0\n42\n0.5\n", "42\n0.5\n"),
            ("20\n0\n42\n0.5\n", "20\n0\n20\n1\n42\n0.5\n"),
            ("42\n0.5\n", "42\n0.5\n42\n0\n"),
            ("42\n0.5\n", "42\n1.0001\n"),
            ("42\n0.5\n", "42\nnan\n"),
            ("42\n0.5\n", "42\ninf\n"),
            ("10\n4\n", "10\n0\n"),
            ("42\n0\n", "42\n0.5\n"),
            ("10\n4\n", "10\nbad\n"),
            ("20\n0\n", "20\nnan\n"),
        ] {
            let text = base.replacen(from, to, 1);
            let before = text.clone();
            assert!(read_dxf_file(&text).is_err(), "{from} -> {to}");
            assert_eq!(text, before);
        }
        let misplaced =
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n3\n70\n129\n10\n0\n20\n0\n42\n0.5\n10\n1\n42\n0\n10\n2\n20\n0\n20\n1\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(misplaced).is_err(), "matching global X/Y counts cannot repair vertex ownership");
    }

    #[test]
    fn layered_bulged_keeps_real_dash_policy_paper_filter_and_straight_legacy_contracts() {
        for style in [DxfLinetype::Dashed, DxfLinetype::Hidden] {
            let mut source = DxfFile::with_layer_zero(3, style, None).unwrap();
            source
                .push(
                    source.default_layer(),
                    DxfShape::BulgedPolyline {
                        vertices: vec![crate::DxfVertex { at: Vec2::ZERO, bulge: 0.5 }, crate::DxfVertex { at: Vec2::new(4., 0.), bulge: 0. }],
                        closed: false,
                    },
                    None,
                )
                .unwrap();
            let text = crate::write_dxf_file(&source).unwrap();
            assert_eq!(read_dxf_file(&text).unwrap().effective_linetype(0).unwrap(), style);
            for flags in ["0", "1"] {
                assert!(read_dxf_file(&text.replacen("70\n128\n", &format!("70\n{flags}\n"), 1)).is_err());
            }
            assert!(matches!(
                read_dxf_file(&text.replacen("70\n128\n", "70\n129\n", 1)).unwrap().entities()[0].geometry(),
                DxfShape::BulgedPolyline { closed: true, .. }
            ));
        }
        let base = original_layered_bulged_input();
        for field in ["40\n1\n", "41\n1\n", "43\n1\n", "38\n1\n", "39\n1\n", "230\n-1\n", "91\n1\n", "6\nByLayer\n"] {
            assert!(read_dxf_file(&base.replacen("90\n2\n", &format!("90\n2\n{field}"), 1)).is_err());
        }
        let paper = base.replacen("0\nLWPOLYLINE\n", "0\nLWPOLYLINE\n67\n1\n", 1).replacen("42\n0.5\n", "42\n1e-400\n", 1);
        assert!(read_dxf_file(&paper).unwrap().entities().is_empty());
        let zero = base.replacen("42\n0.5\n", "42\n-0.0\n", 1);
        for flags in ["128", "0"] {
            assert_eq!(
                read_dxf_file(&zero.replacen("70\n128\n", &format!("70\n{flags}\n"), 1)).unwrap().entities()[0].geometry(),
                &DxfShape::Polyline { points: vec![Vec2::ZERO, Vec2::new(4., 0.)] }
            );
        }
        // The unchanged legacy projection still reads the old underflow-to-zero
        // numeric behavior; only the layered reader's new lexical audit refuses.
        let old_underflow = base.replacen("42\n0.5\n", "42\n1e-400\n", 1);
        assert_eq!(read_dxf(&old_underflow).unwrap(), vec![Geom2::Line(Vec2::ZERO, Vec2::new(4., 0.))]);
    }

    #[test]
    fn layered_bulged_lexical_underflow_shared_budgets_and_scaled_seam_checks_are_bounded() {
        let base = original_layered_bulged_input();
        for code in [10, 20, 42] {
            let from = if code == 42 { "42\n0.5\n".to_owned() } else { format!("{code}\n0\n") };
            for raw in ["1e-400", "-1E-400", "+.0001e-999", "0.001E-999"] {
                let text = base.replacen(&from, &format!("{code}\n{raw}\n"), 1);
                assert!(read_dxf_file(&text).unwrap_err().to_string().contains("underflows to zero"), "{code}={raw}");
            }
        }
        for raw in ["0e-400", "-0.00E-999", "+.000e123", "000.000e-1"] {
            assert!(read_dxf_file(&base.replacen("20\n0\n", &format!("20\n{raw}\n"), 1)).is_ok(), "zero mantissa {raw}");
        }
        let tiny = base.replacen("42\n0.5\n", "42\n1e-200\n", 1).replacen("10\n4\n", "10\n1e-200\n", 1);
        assert!(
            matches!(read_dxf_file(&tiny).unwrap().entities()[0].geometry(), DxfShape::BulgedPolyline { vertices, .. } if vertices[0].bulge == 1e-200 && vertices[1].at.x == 1e-200)
        );
        let subnormal =
            "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n3\n70\n129\n10\n0\n20\n0\n10\n5e-324\n20\n0\n10\n0\n20\n5e-324\n0\nENDSEC\n0\nEOF\n";
        assert!(
            matches!(read_dxf_file(subnormal).unwrap().entities()[0].geometry(), DxfShape::BulgedPolyline { vertices, .. } if vertices[1].at.x == f64::from_bits(1) && vertices[2].at.y == f64::from_bits(1))
        );
        let seam = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n5\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n3\n70\n129\n10\n1.7500000000000002\n20\n0\n10\n0\n20\n1\n10\n1.7500000000000004\n20\n0\n0\nENDSEC\n0\nEOF\n";
        assert!(read_dxf_file(seam).unwrap_err().to_string().contains("collapses distinct bulged seam"));
        let adjacent = seam.replacen("10\n0\n20\n1\n", "10\n1.7500000000000004\n20\n0\n", 1).replacen(
            "10\n1.7500000000000004\n20\n0\n0\nENDSEC",
            "10\n0\n20\n1\n0\nENDSEC",
            1,
        );
        assert!(read_dxf_file(&adjacent).unwrap_err().to_string().contains("collapses adjacent distinct"));
        let tokens = pairs(base);
        let start = layered_section_start(&tokens, "ENTITIES").unwrap().unwrap();
        let entities = section(&tokens, start).unwrap();
        let entity = &entities[0];
        let mut bytes = MAX_DRAWING_BYTES - 2 * std::mem::size_of::<crate::DxfVertex>();
        let mut work = 0;
        assert!(layered_geometry(entity, 1., DxfLinetype::Continuous, &mut bytes, &mut work).is_ok());
        assert!(layered_geometry(entity, 1., DxfLinetype::Continuous, &mut bytes, &mut work).unwrap_err().to_string().contains("byte budget"));
        let mut bytes = 0;
        let mut work = MAX_WORK - 3;
        assert!(layered_geometry(entity, 1., DxfLinetype::Continuous, &mut bytes, &mut work).is_ok());
        let invalid_tokens = pairs(&base.replacen("42\n0.5\n", "42\n2\n", 1));
        let invalid = section(&invalid_tokens, start).unwrap();
        assert!(
            layered_geometry(&invalid[0], 1., DxfLinetype::Continuous, &mut bytes, &mut work).unwrap_err().to_string().contains("work budget"),
            "budget precedes allocation and producer admission"
        );
    }
}
