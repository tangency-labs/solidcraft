//! Source-native, schema-framed SolidWorks saved B-reps. Metres become millimetres once.
//! Configuration identity is selected by the caller; journals and feature history are not replayed.

mod colors;
mod intersection;
mod pcurves;
mod step;

use parasolid_core::brep::{BodyKind, map_xb_brep};
use parasolid_core::{BuiltinProfileRegistry, DocumentLimits, InspectionLimits, SchemaKey, inspect_xb, parse_xb};

pub(super) struct SourcePart {
    pub step: Option<String>,
    pub solids: usize,
    pub sheets: usize,
    pub wires: usize,
    pub faces: usize,
    pub entities: usize,
}

pub(super) fn read(bytes: &[u8], base: usize, cached: bool) -> Result<SourcePart, String> {
    if bytes.len() > 64 << 20 {
        return Err("SLDPRT: Parasolid input exceeds size limit".into());
    }
    let header = inspect_xb(bytes, InspectionLimits { max_file_size: 64 << 20, max_string_bytes: 1 << 20 })
        .map_err(|e| format!("SLDPRT: geometry header: {e}"))?;
    let expected = if cached {
        ": TRANSMIT FILE created by modeller version 3501251"
    } else {
        ": TRANSMIT FILE (partition) created by modeller version 3501251"
    };
    if header.schema_key != "SCH_3501251_35102_13006" || header.modeller_version != expected {
        return Err(format!("SLDPRT: unsupported saved B-rep schema or stream: {}", header.schema_key));
    }
    let registry = BuiltinProfileRegistry::compiled().map_err(|e| e.to_string())?;
    let key = SchemaKey::parse(&header.schema_key).map_err(|e| e.to_string())?;
    let provider = registry.provider_for_key(&key).ok_or("SLDPRT: missing exact geometry schema")?;
    let document = parse_xb(
        bytes,
        &provider,
        DocumentLimits {
            max_file_size: 64 << 20,
            max_nodes: 250_000,
            max_variable_elements: 1_000_000,
            max_fields_per_type: 256,
            max_schema_types: 256,
            max_string_bytes: 1 << 20,
        },
    )
    .map_err(|e| format!("SLDPRT: saved B-rep framing: {e}"))?;
    if !cached && document.nodes.len() == 1 && document.nodes.first().is_some_and(|n| n.node_type == 101) {
        let empty = document
            .nodes
            .first()
            .and_then(|n| n.fields.iter().find(|f| f.definition.name == "body_head"))
            .is_some_and(|f| matches!(f.values.as_slice(), [parasolid_core::FieldValue::PointerIndex(0)]));
        if !empty {
            return Err("SLDPRT: partition references missing bodies".into());
        }
        return Ok(SourcePart { step: None, solids: 0, sheets: 0, wires: 0, faces: 0, entities: 0 });
    }
    let model = map_xb_brep(&document).map_err(|e| format!("SLDPRT: saved B-rep topology: {e}"))?;
    if !model.complete || !model.topology.valid || model.bodies.iter().any(|b| b.kind == BodyKind::General) {
        return Err(format!("SLDPRT: incomplete or non-manifold source geometry: {:?}", model.diagnostics));
    }
    let solids = model.bodies.iter().filter(|b| b.kind == BodyKind::Solid).count();
    let sheets = model.bodies.iter().filter(|b| b.kind == BodyKind::Sheet).count();
    let wires = model.bodies.iter().filter(|b| b.kind == BodyKind::Wire).count();
    let faces = model.faces.len();
    let (step, entities) = if solids == 0 && sheets == 0 {
        (None, 0)
    } else {
        let (text, count) = step::write_colored(&model, &colors::read(&document.nodes)?, base)?;
        (Some(text), count)
    };
    Ok(SourcePart { step, solids, sheets, wires, faces, entities })
}

fn word(bytes: &[u8], at: usize) -> Result<usize, String> {
    let b = <[u8; 4]>::try_from(bytes.get(at..at + 4).ok_or("SLDPRT: truncated cache framing")?).map_err(|e| e.to_string())?;
    usize::try_from(u32::from_le_bytes(b)).map_err(|e| e.to_string())
}

fn cached_streams(bytes: &[u8], imported: &str) -> Result<Vec<Vec<u8>>, String> {
    if bytes.len() > 64 << 20 || word(bytes, 0)? != 1 || bytes.get(4..7) != Some(&[0xff, 0xfe, 0xff]) {
        return Err("SLDPRT: unsupported imported cache framing".into());
    }
    let length = usize::from(*bytes.get(7).ok_or("SLDPRT: cache name length")?);
    if length == 0 || length > 128 {
        return Err("SLDPRT: invalid imported cache name".into());
    }
    let end = 8 + length * 2;
    let name = bytes.get(8..end).ok_or("SLDPRT: cache name")?.as_chunks::<2>().0.iter().map(|v| u16::from_le_bytes(*v)).collect::<Vec<_>>();
    if String::from_utf16(&name).map_err(|e| e.to_string())? != imported {
        return Err("SLDPRT: cache identity differs from active imported feature".into());
    }
    let count = word(bytes, end + 4)?;
    if count == 0 || count > 4096 {
        return Err("SLDPRT: imported body count exceeds limit".into());
    }
    let mut cursor = end + 8;
    let mut streams = Vec::new();
    let mut total = 0usize;
    for _ in 0..count {
        let wrapper = word(bytes, cursor + 10)?;
        let outer = word(bytes, cursor + 5)?;
        if word(bytes, cursor)? != 0 || bytes.get(cursor + 4) != Some(&0) || bytes.get(cursor + 9) != Some(&0) || outer != wrapper + 5 {
            return Err("SLDPRT: unsupported imported body envelope".into());
        }
        let stop = cursor.checked_add(14).and_then(|v| v.checked_add(wrapper)).filter(|v| *v <= bytes.len()).ok_or("SLDPRT: cache body extent")?;
        let mut framed = u32::try_from(wrapper).map_err(|e| e.to_string())?.to_le_bytes().to_vec();
        framed.extend_from_slice(bytes.get(cursor + 14..stop).ok_or("SLDPRT: cache body")?);
        let mut body = super::selection::geometry_streams(&framed)?;
        if body.len() != 1 {
            return Err("SLDPRT: cache body has ambiguous geometry streams".into());
        }
        let body = body.pop().ok_or("SLDPRT: empty cached geometry")?;
        total = total.checked_add(body.len()).filter(|v| *v <= 256 << 20).ok_or("SLDPRT: expanded body cache exceeds limit")?;
        streams.push(body);
        cursor = stop;
    }
    if bytes.get(cursor..).is_none_or(|v| v.iter().any(|v| *v != 0)) {
        return Err("SLDPRT: trailing cache records are unsupported".into());
    }
    Ok(streams)
}

pub(super) fn part(bytes: &[u8], file: &str, selection: &super::selection::Selection) -> Result<(String, Vec<String>), String> {
    let streams = super::selection::geometry_streams(&selection.partition)?;
    let partition = streams.first().ok_or("SLDPRT: selected partition is empty")?;
    let source = read(partition, 0, false)?;
    if let Some(step) = source.step {
        return Ok((step, Vec::new()));
    }
    if source.faces != 0 || source.sheets != 0 || source.wires != 0 {
        return Err("SLDPRT: selected partition contains no supported B-rep geometry".into());
    }
    let metadata = super::selection::metadata(bytes, selection.source_index)?;
    let (_, decoded) = super::convert::decode(file, "sldprt", &metadata, &["sldprt"])?;
    let (ir, _report, _fidelity) = decoded.into_parts();
    let name = super::sldprt::unchanged_import(&ir, selection.source_index)?
        .ok_or("SLDPRT: selected partition has no solids or unchanged imported-body cache")?;
    let stream = format!("Config-{}-FeatureBodies/LocalBodies", selection.source_index);
    let cache = super::assembly::section(bytes, &stream, 64 << 20)?.ok_or("SLDPRT: imported-body cache is missing")?;
    let cached = cached_streams(&cache, &name)?;
    let mut text = String::new();
    let mut next = 0usize;
    let (mut solids, mut sheets, mut wires, mut faces) = (0usize, 0usize, 0usize, 0usize);
    for (i, bytes) in cached.iter().enumerate() {
        let source = read(bytes, next, true).map_err(|e| format!("{file}: imported body {i}: {e}"))?;
        solids += source.solids;
        sheets += source.sheets;
        wires += source.wires;
        faces += source.faces;
        if let Some(step) = source.step {
            let (header, data) = step.split_once("DATA;\n").ok_or("SLDPRT: generated STEP header")?;
            let data = data.strip_suffix("ENDSEC;\nEND-ISO-10303-21;\n").ok_or("SLDPRT: generated STEP end")?;
            if text.is_empty() {
                text.push_str(header);
                text.push_str("DATA;\n");
            }
            text.push_str(data);
            next = next.checked_add(source.entities).filter(|v| *v <= 4_000_000).ok_or("SLDPRT: STEP entity count limit")?;
            if text.len() > crate::MAX_STEP_BYTES {
                return Err("SLDPRT: combined imported bodies exceed size limit".into());
            }
        }
    }
    if solids + sheets == 0 || faces == 0 {
        return Err("SLDPRT: imported cache contains no supported B-rep geometry".into());
    }
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok((text, vec![format!("SLDPRT: recovered {solids} stored solids and {sheets} surface bodies; {wires} source wire bodies are excluded")]))
}

#[cfg(test)]
pub(super) fn test_emit(model: &parasolid_core::brep::BrepModel, colors: &std::collections::BTreeMap<u32, [f64; 3]>) -> Result<String, String> {
    step::write_colored(model, colors, 0).map(|(step, _)| step)
}

#[cfg(test)]
pub(super) fn test_intersection_samples(mut m: parasolid_core::brep::BrepModel, id: u32) -> Result<Vec<[f64; 3]>, String> {
    pcurves::recover_intersections(&mut m)?;
    let c = m.curves.iter().find(|c| c.id == id).ok_or("fixture curve")?;
    let parasolid_core::brep::CurveKind::Nurbs(n) = &c.kind else { return Err("intersection was not restated".into()) };
    let a = *n.knots.first().ok_or("curve range")?;
    let b = *n.knots.last().ok_or("curve range")?;
    (0..=128).map(|i| pcurves::curve(&m, id, a + (b - a) * f64::from(i) / 128., 0)).collect()
}

#[cfg(test)]
pub(super) fn test_colors(nodes: &[parasolid_core::RawNode]) -> Result<std::collections::BTreeMap<u32, [f64; 3]>, String> {
    colors::read(nodes)
}

#[cfg(test)]
pub(super) fn test_surface(m: &parasolid_core::brep::BrepModel, uv: [f64; 3]) -> Result<[f64; 3], String> {
    pcurves::surface(m, 0, uv)
}
