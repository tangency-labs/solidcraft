//! Select a declared SolidWorks part configuration without replaying feature history.
//! All geometry/display payloads are retained byte-for-byte after CRC validation.

use quick_xml::{Reader, events::Event};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const MAX_XML: usize = 8 << 20;
const MAX_SECTION: usize = 64 << 20;

pub(super) struct Selection {
    pub partition: Vec<u8>,
    pub source_index: u32,
    pub needs_update: bool,
}

fn xml_text(bytes: &[u8]) -> Result<String, String> {
    let bytes = bytes.strip_prefix(&[0x86]).unwrap_or(bytes);
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (words, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            return Err("SLDPRT: truncated UTF-16 manifest".into());
        }
        let words: Vec<_> = words.iter().map(|v| u16::from_le_bytes(*v)).collect();
        String::from_utf16(&words).map_err(|e| format!("SLDPRT: configuration manifest: {e}"))
    } else {
        std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map(str::to_owned)
            .map_err(|e| format!("SLDPRT: configuration manifest: {e}"))
    }
}

fn configuration(xml: &str, requested: &str, saved: bool) -> Result<(u32, String, bool), String> {
    let mut reader = Reader::from_str(xml);
    let mut models = BTreeMap::new();
    let mut configs = Vec::new();
    let mut version = None;
    let mut depth = 0usize;
    let mut count = 0usize;
    loop {
        let event = reader.read_event().map_err(|e| format!("SLDPRT: configuration XML: {e}"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                count += 1;
                if !empty {
                    depth += 1;
                }
                if count > 10_000 || depth > 64 {
                    return Err("SLDPRT: configuration XML limit exceeded".into());
                }
                let mut attributes = BTreeMap::new();
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    let key = std::str::from_utf8(a.key.as_ref()).map_err(|e| e.to_string())?.to_owned();
                    let value =
                        a.decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder()).map_err(|e| e.to_string())?.into_owned();
                    if key.len() > 128 || value.len() > 8192 || attributes.len() >= 64 || attributes.insert(key, value).is_some() {
                        return Err("SLDPRT: invalid configuration attributes".into());
                    }
                }
                match e.local_name().as_ref() {
                    b"swSolidWorks" => {
                        if version.is_some() {
                            return Err("SLDPRT: duplicate configuration manifest root".into());
                        }
                        version = attributes.get("swVersion").cloned();
                    }
                    b"swModel" => {
                        let id = attributes.get("id").ok_or("SLDPRT: configuration model id is missing")?.clone();
                        if models.insert(id, attributes).is_some() {
                            return Err("SLDPRT: duplicate configuration model".into());
                        }
                    }
                    b"swConfiguration" => configs.push(attributes),
                    _ => {}
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or("SLDPRT: malformed configuration XML")?,
            Event::DocType(_) | Event::GeneralRef(_) => return Err("SLDPRT: XML declarations with entities are unsupported".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 {
        return Err("SLDPRT: unclosed configuration XML".into());
    }
    let mut names = BTreeSet::new();
    let mut indices = BTreeSet::new();
    let mut selected = None;
    for c in configs {
        let name = c
            .get("swName")
            .or_else(|| c.get("swModelRef").and_then(|id| models.get(id)).and_then(|m| m.get("swConfigurationName")))
            .ok_or("SLDPRT: configuration name is missing")?;
        let text = c.get("swID").ok_or("SLDPRT: configuration source index is missing")?;
        if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
            return Err("SLDPRT: invalid configuration source index".into());
        }
        let index = text.parse::<u32>().map_err(|e| e.to_string())?;
        if !names.insert(name.clone()) || !indices.insert(index) {
            return Err("SLDPRT: ambiguous configuration declarations".into());
        }
        if name == requested {
            if c.get("swConfigurationNeedsUpdate").is_some_and(|v| v != "NO" && v != "YES") {
                return Err("SLDPRT: invalid configuration regeneration flag".into());
            }
            if !saved && c.get("swConfigurationNeedsUpdate").is_some_and(|v| v != "NO") {
                return Err("SLDPRT: selected configuration needs regeneration".into());
            }
            if let Some(model) = c.get("swModelRef").and_then(|id| models.get(id))
                && (model.get("swConfigurationId").is_some_and(|v| v != text) || model.get("swConfigurationName").is_some_and(|v| v != name))
            {
                return Err("SLDPRT: inconsistent configuration identity".into());
            }
            selected = Some((index, c.get("swConfigurationNeedsUpdate").is_some_and(|v| v != "NO")));
        }
    }
    let (index, needs_update) = selected.ok_or_else(|| format!("SLDPRT: requested configuration {requested:?} is missing"))?;
    let version = version.ok_or("SLDPRT: source version is missing")?;
    if version.is_empty() || !version.bytes().all(|c| c.is_ascii_digit()) {
        return Err("SLDPRT: invalid source version".into());
    }
    Ok((index, version, needs_update))
}

pub(super) fn append_section(output: &mut Vec<u8>, name: &str, payload: &[u8]) -> Result<(), String> {
    if payload.len() > MAX_SECTION || name.len() > 512 {
        return Err("SLDPRT: projected section exceeds size limit".into());
    }
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload).map_err(|e| e.to_string())?;
    let compressed = encoder.finish().map_err(|e| e.to_string())?;
    output.extend_from_slice(&[0x14, 0, 6, 0, 8, 0]);
    for value in [
        0,
        crc32fast::hash(payload),
        u32::try_from(compressed.len()).map_err(|e| e.to_string())?,
        u32::try_from(payload.len()).map_err(|e| e.to_string())?,
        u32::try_from(name.len()).map_err(|e| e.to_string())?,
    ] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend(name.bytes().map(|c| c.rotate_left(4)));
    output.extend_from_slice(&compressed);
    if output.len() > crate::MAX_STEP_BYTES {
        return Err("SLDPRT: configuration projection exceeds size limit".into());
    }
    Ok(())
}

pub(super) fn select(bytes: &[u8], requested: &str) -> Result<Selection, String> {
    select_inner(bytes, requested, false)
}
pub(super) fn select_stored(bytes: &[u8], requested: &str) -> Result<Selection, String> {
    select_inner(bytes, requested, true)
}
fn select_inner(bytes: &[u8], requested: &str, saved: bool) -> Result<Selection, String> {
    if requested.is_empty() || requested.len() > 8192 || bytes.get(4..8) != Some(&[0, 0, 0, 4]) {
        return Err("SLDPRT: unsupported configuration selection container".into());
    }
    let xml = super::assembly::section(bytes, "swXmlContents/Features", MAX_XML)?.ok_or("SLDPRT: configuration manifest is missing")?;
    let (source_index, _version, needs_update) = configuration(&xml_text(&xml)?, requested, saved)?;
    let name = format!("Contents/Config-{source_index}-Partition");
    let partition = super::assembly::section(bytes, &name, MAX_SECTION)?.ok_or("SLDPRT: requested configuration has no stored partition")?;
    Ok(Selection { partition, source_index, needs_update })
}

/// Expand the documented chained zlib frames inside a CRC-checked partition.
/// Source bytes are read only; lengths, zlib checksums and member extents must agree.
pub(super) fn geometry_streams(bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    use std::io::Read;
    const MAGIC: &[u8] = &[0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99];
    if bytes.len() > MAX_SECTION {
        return Err("SLDPRT: partition exceeds size limit".into());
    }
    if bytes.starts_with(b"PS\0\0") {
        return Ok(vec![bytes.to_vec()]);
    }
    let word = |at: usize| -> Result<usize, String> {
        let v = bytes.get(at..at + 4).ok_or("SLDPRT: truncated partition frame")?;
        let v = <[u8; 4]>::try_from(v).map_err(|e| e.to_string())?;
        usize::try_from(u32::from_le_bytes(v)).map_err(|e| e.to_string())
    };
    let mut streams = Vec::new();
    let mut at = 0usize;
    let mut total = 0usize;
    while at < bytes.len() {
        if streams.len() >= 4096 {
            return Err("SLDPRT: too many partition streams".into());
        }
        let length = word(at)?;
        let end = at.checked_add(4).and_then(|p| p.checked_add(length)).filter(|p| *p <= bytes.len()).ok_or("SLDPRT: invalid partition extent")?;
        if length < 24 || bytes.get(at + 4..at + 20) != Some(MAGIC) {
            return Err("SLDPRT: unsupported partition frame".into());
        }
        let mut frame = at + 20;
        let mut payload = Vec::new();
        let mut frames = 0usize;
        while frame < end {
            let tail = bytes.get(frame..end).ok_or("SLDPRT: truncated partition padding")?;
            if tail.iter().all(|b| *b == 0) {
                break;
            }
            frames += 1;
            if frames > 4096 || end - frame < 8 {
                return Err("SLDPRT: invalid partition member count".into());
            }
            let size = word(frame)?;
            let compressed = word(frame + 4)?;
            total = total.checked_add(size).filter(|n| *n <= MAX_SECTION).ok_or("SLDPRT: partition expansion exceeds size limit")?;
            if size == 0 || compressed == 0 {
                return Err("SLDPRT: empty partition member".into());
            }
            let start = frame + 8;
            frame = start.checked_add(compressed).filter(|p| *p <= end).ok_or("SLDPRT: truncated partition member")?;
            let member = bytes.get(start..frame).ok_or("SLDPRT: truncated partition member")?;
            let mut decoder = flate2::bufread::ZlibDecoder::new(member);
            let before = payload.len();
            decoder.by_ref().take((size + 1) as u64).read_to_end(&mut payload).map_err(|e| format!("SLDPRT: partition zlib: {e}"))?;
            if payload.len() - before != size || decoder.total_in() != compressed as u64 {
                return Err("SLDPRT: partition member size mismatch".into());
            }
        }
        if frames == 0 || !payload.starts_with(b"PS\0\0") {
            return Err("SLDPRT: partition contains no Parasolid stream".into());
        }
        streams.push(payload);
        at = end;
    }
    Ok(streams)
}

/// A read-only metadata projection for cache eligibility. Geometry is read separately
/// from the original selected partition/cache, never inferred from this projection.
pub(super) fn metadata(bytes: &[u8], selected: u32) -> Result<Vec<u8>, String> {
    let mut out = bytes.get(..8).ok_or("SLDPRT: container header")?.to_vec();
    let names = [
        "Header2".to_owned(),
        "Contents/CMgrHdr2".into(),
        "Contents/Definition".into(),
        "swXmlContents/KeyWords".into(),
        "swXmlContents/Features".into(),
        format!("Contents/Config-{selected}-ModelHeader"),
        format!("Contents/Config-{selected}-ResolvedFeatures"),
        format!("Contents/Config-{selected}-Partition"),
    ];
    let mut total = 0usize;
    for name in names {
        if let Some(raw) = super::assembly::section(bytes, &name, 32 << 20)? {
            total = total.checked_add(raw.len()).filter(|v| *v <= 48 << 20).ok_or("SLDPRT: feature metadata budget")?;
            let payload = if name == "swXmlContents/Features" { metadata_xml(&raw)? } else { raw };
            append_section(&mut out, &name, &payload)?;
        }
    }
    Ok(out)
}

// cadmpeg's manifest lookups expect unqualified tags. Remove only the official
// default namespace in the metadata projection; original XML and geometry stay intact.
fn metadata_xml(raw: &[u8]) -> Result<Vec<u8>, String> {
    let xml = xml_text(raw)?;
    let mut reader = Reader::from_str(&xml);
    let mut writer = quick_xml::Writer::new(Vec::new());
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) if e.name().as_ref() == b"swSolidWorks" => {
                let mut root = quick_xml::events::BytesStart::new("swSolidWorks");
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    if a.key.as_ref() == b"xmlns" {
                        if a.value.as_ref() != b"http://www.solidworks.com/sw2003/schema" {
                            return Err("SLDPRT: unsupported metadata namespace".into());
                        }
                    } else {
                        root.push_attribute(a);
                    }
                }
                writer.write_event(Event::Start(root)).map_err(|e| e.to_string())?;
            }
            Event::DocType(_) | Event::GeneralRef(_) => return Err("SLDPRT: metadata XML entities are unsupported".into()),
            Event::Decl(_) => {}
            Event::Eof => break,
            e => writer.write_event(e).map_err(|e| e.to_string())?,
        }
    }
    Ok(writer.into_inner())
}
