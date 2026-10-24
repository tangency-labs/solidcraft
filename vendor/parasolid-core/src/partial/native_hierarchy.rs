// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Direct BODY/REGION/SHELL links for reviewed exact `SolidWorks` profiles.
//!
//! Attribute (00 51) dictionaries are not the topology body arena. This reader
//! follows native 00 0c -> 00 13 -> 00 0d -> 00 0e references instead. The
//! embedded declarations must match before their field offsets acquire meaning.

use std::collections::{BTreeMap, BTreeSet};

use super::view::View;
use crate::brep::BodyKind;

use super::{BodyRecord, RegionRecord, ShellRecord, topology::Tables};

const SCHEMA: &str = "SCH_3701229_37102_13006";
const BODY_DECLARATION: &[u8] = b"$CCCI\x07lattice\x00\xde\x00\x01CCCI\x04mesh\x03\xee\x00\x01I\x08polyline\x03\xf0\x00\x01CCCCCCCDI\x05owner\x04\x10\x00\x01CCCI\x10boundary_lattice\x00\xde\x00\x01CCCI\x0dboundary_mesh\x03\xee\x00\x01I\x11boundary_polyline\x03\xf0\x00\x01CCCA\x10index_map_offset\x00\x00\x00\x01\x01dA\x09index_map\x00R\x00\x01A\x11node_id_index_map\x00R\x00\x01A\x14schema_embedding_map\x00R\x00\x01A\x05child\x00\x0c\x00\x01A\x0elowest_node_id\x00\x00\x00\x01\x01dA\x10mesh_offset_data\x00\xce\x00\x01Z";
const REGION_DECLARATION: &[u8] = b"\x09CCCCCCI\x05frame\x00\xe6\x00\x01CA\x05owner\x00\x0c\x00\x01Z";

#[derive(Clone, Debug, PartialEq)]
struct Body {
    id: u16,
    offset: usize,
    next: u16,
    previous: u16,
    kind: BodyKind,
    shell: u16,
    region: u16,
}

#[derive(Clone, Debug, PartialEq)]
struct Region {
    id: u16,
    offset: usize,
    body: u16,
    next: u16,
    previous: u16,
    shell: u16,
    kind: u8,
}

#[derive(Clone, Debug, PartialEq)]
struct Shell {
    id: u16,
    offset: usize,
    body: u16,
    next: u16,
    region: u16,
    face: u16,
    front: u16,
}

fn declaration(data: &[u8], tag: u8, bytes: &[u8]) -> Option<(usize, usize)> {
    let mut matches = data.windows(bytes.len() + 2).enumerate().filter(|(_, w)| w[..2] == [0, tag] && &w[2..] == bytes);
    let (offset, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some((offset, offset + 2 + bytes.len()))
}

fn starts(data: &[u8], tag: u8, declared: Option<(usize, usize)>) -> Vec<(usize, usize)> {
    data.windows(2)
        .enumerate()
        .filter_map(|(offset, w)| {
            if w != [0, tag] {
                return None;
            }
            if let Some((at, start)) = declared {
                if offset == at {
                    return Some((offset, start));
                }
                if offset < start {
                    return None;
                }
            }
            let start = offset + 2 + usize::from(data.get(offset + 2) == Some(&0xff));
            Some((offset, start))
        })
        .collect()
}

fn body(data: &[u8], offset: usize, p: usize) -> Option<Body> {
    let raw = data.get(p..p.checked_add(89)?)?;
    let id = View::u16_be_at(raw, 0)?;
    let scale = View::f64_be_at(raw, 24)?;
    let tolerance = View::f64_be_at(raw, 32)?;
    if id <= 1
        || View::u32_be_at(raw, 2)? == 0
        || !scale.is_finite()
        || scale <= 0.0
        || !tolerance.is_finite()
        || tolerance <= 0.0
        || raw[46] != 1
        || View::u16_be_at(raw, 47)? != 2
        || raw[50] != 1
        || View::u16_be_at(raw, 87)? != 1
    {
        return None;
    }
    let kind = match raw[49] {
        1 => BodyKind::Solid,
        3 => BodyKind::Sheet,
        _ => return None,
    };
    Some(Body {
        id,
        offset,
        next: View::u16_be_at(raw, 42)?,
        previous: View::u16_be_at(raw, 44)?,
        kind,
        shell: View::u16_be_at(raw, 51)?,
        region: View::u16_be_at(raw, 65)?,
    })
}

fn region(data: &[u8], offset: usize, p: usize) -> Option<Region> {
    let raw = data.get(p..p.checked_add(21)?)?;
    let id = View::u16_be_at(raw, 0)?;
    if id <= 1 || View::u32_be_at(raw, 2)? == 0 || !matches!(raw[18], b'S' | b'V') || View::u16_be_at(raw, 16)? != 1 || View::u16_be_at(raw, 19)? != 1
    {
        return None;
    }
    Some(Region {
        id,
        offset,
        body: View::u16_be_at(raw, 8)?,
        next: View::u16_be_at(raw, 10)?,
        previous: View::u16_be_at(raw, 12)?,
        shell: View::u16_be_at(raw, 14)?,
        kind: raw[18],
    })
}

fn shell(data: &[u8], offset: usize, p: usize) -> Option<Shell> {
    let raw = data.get(p..p.checked_add(22)?)?;
    let id = View::u16_be_at(raw, 0)?;
    // This bounded profile has no wire edges or isolated vertices.
    if id <= 1 || View::u32_be_at(raw, 2)? == 0 || View::u16_be_at(raw, 14)? != 1 || View::u16_be_at(raw, 16)? != 1 {
        return None;
    }
    Some(Shell {
        id,
        offset,
        body: View::u16_be_at(raw, 8)?,
        front: View::u16_be_at(raw, 20)?,
        next: View::u16_be_at(raw, 10)?,
        region: View::u16_be_at(raw, 18)?,
        face: View::u16_be_at(raw, 12)?,
    })
}

fn unique<T>(values: impl Iterator<Item = (u16, T)>) -> Option<BTreeMap<u16, T>> {
    let mut out = BTreeMap::new();
    for (id, value) in values {
        if out.insert(id, value).is_some() {
            return None;
        }
    }
    Some(out)
}

pub fn recover(streams: &[(&[u8], &str, bool)], tables: &Tables) -> Option<Vec<BodyRecord>> {
    let partitions = streams.iter().filter(|(_, _, deltas)| !deltas).collect::<Vec<_>>();
    let [(data, schema, _)] = partitions.as_slice() else {
        return None;
    };
    for (delta, delta_schema, is_delta) in streams {
        if delta_schema != schema {
            return None;
        }
        if *is_delta
            && (starts(delta, 12, None).iter().any(|&(at, p)| body(delta, at, p).is_some())
                || starts(delta, 19, None).iter().any(|&(at, p)| region(delta, at, p).is_some())
                || starts(delta, 13, None).iter().any(|&(at, p)| shell(delta, at, p).is_some()))
        {
            // Hierarchy edits have no supported update ordering in this profile.
            return None;
        }
    }
    scan(data, schema, tables)
}

/// Return a complete native hierarchy, or leave the existing partial result.
pub fn scan(data: &[u8], schema: &str, tables: &Tables) -> Option<Vec<BodyRecord>> {
    if !matches!(schema, SCHEMA | "SCH_3601228_36001_13006") {
        return None;
    }
    let body_decl = declaration(data, 12, BODY_DECLARATION)?;
    let region_decl = declaration(data, 19, REGION_DECLARATION)?;
    let bodies = unique(starts(data, 12, Some(body_decl)).into_iter().filter_map(|(at, p)| body(data, at, p)).map(|v| (v.id, v)))?;
    let regions = unique(starts(data, 19, Some(region_decl)).into_iter().filter_map(|(at, p)| region(data, at, p)).map(|v| (v.id, v)))?;
    let shells = unique(starts(data, 13, None).into_iter().filter_map(|(at, p)| shell(data, at, p)).map(|v| (v.id, v)))?;
    let first = body(data, body_decl.0, body_decl.1)?;
    let mut at = first.id;
    let mut previous = 1;
    let mut seen = BTreeSet::new();
    let mut assigned_faces = BTreeSet::new();
    let mut all_regions = BTreeSet::new();
    let mut all_shells = BTreeSet::new();
    let mut out = Vec::new();
    while at > 1 {
        if !seen.insert(at) {
            return None;
        }
        let b = bodies.get(&at)?;
        if b.previous != previous || b.next == 0 || b.region <= 1 {
            return None;
        }
        let mut region_id = b.region;
        let mut previous_region = 1;
        let mut seen_regions = BTreeSet::new();
        let mut material_regions = Vec::new();
        let mut body_faces = Vec::new();
        while region_id > 1 {
            if !seen_regions.insert(region_id) || !all_regions.insert(region_id) {
                return None;
            }
            let r = regions.get(&region_id)?;
            if r.body != b.id || r.previous != previous_region || r.next == 0 || r.shell <= 1 {
                return None;
            }
            if b.kind == BodyKind::Sheet && (r.kind != b'V' || r.next != 1) {
                return None;
            }
            // Validate the exterior-void side as well, even though it is not
            // emitted as a second material region in the neutral model.
            let mut linked_shell = r.shell;
            while linked_shell > 1 {
                if !all_shells.insert(linked_shell) {
                    return None;
                }
                let shell = shells.get(&linked_shell)?;
                let legacy_body = if r.kind == b'V' && b.kind == BodyKind::Solid { 1 } else { b.id };
                if shell.body != legacy_body || shell.region != r.id || shell.next == 0 {
                    return None;
                }
                let head = if r.kind == b'V' && b.kind == BodyKind::Solid {
                    if shell.face != 1 {
                        return None;
                    }
                    shell.front
                } else {
                    shell.face
                };
                if !tables.bridges.contains_key(&head) {
                    return None;
                }
                linked_shell = shell.next;
            }
            // Exterior void regions do not represent material. A sheet's sole
            // void region carries both sides of its open boundary.
            if r.kind == b'S' || b.kind == BodyKind::Sheet {
                let mut shell_id = r.shell;
                let mut seen_shells = BTreeSet::new();
                let mut owned_shells = Vec::new();
                while shell_id > 1 {
                    if !seen_shells.insert(shell_id) {
                        return None;
                    }
                    let s = shells.get(&shell_id)?;
                    if s.region != r.id {
                        return None;
                    }
                    let mut face_id = s.face;
                    let mut previous_face = 1;
                    let mut faces = Vec::new();
                    while face_id > 1 {
                        if !assigned_faces.insert(face_id) {
                            return None;
                        }
                        let f = tables.bridges.get(&face_id)?;
                        if f.refs.get(3) != Some(&s.id) || f.refs.get(1) != Some(&previous_face) {
                            return None;
                        }
                        faces.push(face_id);
                        previous_face = face_id;
                        face_id = *f.refs.first()?;
                        if face_id == 0 {
                            return None;
                        }
                    }
                    if faces.is_empty() {
                        return None;
                    }
                    body_faces.extend(&faces);
                    owned_shells.push(ShellRecord { attr: s.id, offset: s.offset, refs: faces });
                    shell_id = s.next;
                }
                if owned_shells.is_empty() {
                    return None;
                }
                material_regions.push(RegionRecord { attr: r.id, offset: r.offset, shells: owned_shells });
            }
            previous_region = region_id;
            region_id = r.next;
        }
        if material_regions.is_empty() || !material_regions.iter().flat_map(|r| &r.shells).any(|s| s.attr == b.shell) {
            return None;
        }
        out.push(BodyRecord { attr: b.id, offset: b.offset, kind: b.kind, refs: body_faces, regions: material_regions });
        previous = at;
        at = b.next;
    }
    if seen.len() != bodies.len()
        || assigned_faces.len() != tables.bridges.len()
        || all_regions.len() != regions.len()
        || all_shells.len() != shells.len()
    {
        return None;
    }
    Some(out)
}

/// Emit only the fields read by this bounded, successfully validated hierarchy.
/// Unknown BODY slots remain gaps; matching declarations are typed framing.
pub fn verified_read_spans(data: &[u8], schema: &str, tables: &Tables) -> Vec<crate::partial::ReadSpan> {
    use crate::partial::ReadSpan;
    if scan(data, schema, tables).is_none() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (tag, definition, kind) in [(12, BODY_DECLARATION, "native.body.definition"), (19, REGION_DECLARATION, "native.region.definition")] {
        if let Some((start, end)) = declaration(data, tag, definition) {
            out.push(ReadSpan::new(start, end, kind, None));
        }
    }
    let body_decl = declaration(data, 12, BODY_DECLARATION);
    for (at, p) in starts(data, 12, body_decl) {
        if let Some(value) = body(data, at, p) {
            for (start, end) in [(p, p + 6), (p + 24, p + 40), (p + 42, p + 53), (p + 65, p + 67), (p + 87, p + 89)] {
                out.push(ReadSpan::new(start, end, "native.body.fields", Some(value.id)));
            }
            if body_decl.is_none_or(|(decl, _)| at != decl) {
                out.push(ReadSpan::new(at, p, "native.body.framing", Some(value.id)));
            }
        }
    }
    let region_decl = declaration(data, 19, REGION_DECLARATION);
    for (at, p) in starts(data, 19, region_decl) {
        if let Some(value) = region(data, at, p) {
            for (start, end) in [(p, p + 6), (p + 8, p + 21)] {
                out.push(ReadSpan::new(start, end, "native.region.fields", Some(value.id)));
            }
            if region_decl.is_none_or(|(decl, _)| at != decl) {
                out.push(ReadSpan::new(at, p, "native.region.framing", Some(value.id)));
            }
        }
    }
    for (at, p) in starts(data, 13, None) {
        if let Some(value) = shell(data, at, p) {
            for (start, end) in [(at, p + 6), (p + 8, p + 22)] {
                out.push(ReadSpan::new(start, end, "native.shell.fields", Some(value.id)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
