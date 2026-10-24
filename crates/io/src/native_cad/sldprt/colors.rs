//! Resolve named cached face-color attributes by their owner and REAL_VALUES ids.
//!
//! cadmpeg 0.6.1 admits these RGB assets but its compact bridge's first attribute
//! need not be the color attribute. Read the named SDL/TYSA_COLOUR assignment's
//! owner instead; never pair an adjacent RGB record or rely on face order.
//! Record layouts follow cadmpeg's Apache-2.0 metadata decoder and published spec.

use std::collections::BTreeMap;

use cadmpeg_ir::CadIr;
use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::AppearanceBindingId;

const MAX_RECORDS: usize = 8192;

fn word(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn integer(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn unique<K: Ord, V: PartialEq>(map: &mut BTreeMap<K, Option<V>>, key: K, value: V) -> Result<(), String> {
    if map.len() >= MAX_RECORDS && !map.contains_key(&key) {
        return Err("SLDPRT: cached color record limit exceeded".into());
    }
    map.entry(key)
        .and_modify(|old| {
            if old.as_ref() != Some(&value) {
                *old = None;
            }
        })
        .or_insert(Some(value));
    Ok(())
}

pub(crate) fn restore_colors(ir: &mut CadIr, bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() > super::MAX_BODY_BYTES
        || ir.model.faces.len() > 512
        || ir.model.bodies.len() > 512
        || ir.model.appearances.len() > MAX_RECORDS
        || ir.model.appearance_bindings.len() > MAX_RECORDS
    {
        return Err("SLDPRT: cached color input limit exceeded".into());
    }
    let mut keys = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    // Attribute id -> newest version (sequence, definition, owner, RGB asset).
    let mut attributes = BTreeMap::new();
    for (at, tag) in bytes.windows(2).enumerate() {
        let p = at + 2 + usize::from(bytes.get(at + 2) == Some(&0xff));
        match tag {
            [0, 0x4f] => {
                let (Some(n), Some(id)) = (integer(bytes, p), word(bytes, p + 4)) else { continue };
                if id <= 1 || n == 0 || n > 256 {
                    continue;
                }
                let Some(text) = bytes.get(p + 6..p + 6 + n as usize) else { continue };
                if text.iter().all(u8::is_ascii_graphic) {
                    unique(&mut keys, id, text)?;
                }
            }
            [0, 0x50] if integer(bytes, p) == Some(1) => {
                if let (Some(id), Some(key)) = (word(bytes, p + 4), word(bytes, p + 8))
                    && id > 1
                    && key > 1
                {
                    unique(&mut definitions, id, key)?;
                }
            }
            [0, 0x51] if integer(bytes, p).is_some_and(|flags| flags & 0xff == 1) => {
                let (Some(id), Some(seq), Some(definition), Some(owner), Some(color)) =
                    (word(bytes, p + 4), integer(bytes, p + 6), word(bytes, p + 10), word(bytes, p + 12), word(bytes, p + 22))
                else {
                    continue;
                };
                if id <= 1 || seq == 0 || owner <= 1 {
                    continue;
                }
                let value = (seq, definition, owner, color);
                // Equal-sequence conflicting records stay unresolved, even if repeated later.
                match attributes.get(&id).copied() {
                    Some((previous, _)) if previous > seq => {}
                    Some((previous, _)) if previous == seq => {
                        if let Some((_, old)) = attributes.get_mut(&id)
                            && *old != Some(value)
                        {
                            *old = None;
                        }
                    }
                    _ => {
                        if attributes.len() >= MAX_RECORDS && !attributes.contains_key(&id) {
                            return Err("SLDPRT: cached color record limit exceeded".into());
                        }
                        attributes.insert(id, (seq, Some(value)));
                    }
                }
            }
            _ => {}
        }
    }
    if attributes.values().any(|(_, value)| value.is_none()) {
        return Err("SLDPRT: conflicting cached color metadata".into());
    }
    let assets: BTreeMap<_, _> = ir
        .model
        .appearances
        .iter()
        .filter_map(|a| {
            let id = a.id.as_str().strip_prefix("sldprt:appearance:entity53#")?.parse::<u16>().ok()?;
            Some((id, (a.id.clone(), a.base_color?)))
        })
        .collect();
    let mut targets = BTreeMap::new();
    for face in &ir.model.faces {
        if let Some(id) = face.id.as_str().strip_prefix("sldprt:brep:face#").and_then(|s| s.parse::<u16>().ok()) {
            targets.insert(id, AppearanceTarget::Face(face.id.clone()));
        }
    }
    for body in &ir.model.bodies {
        if let Some(id) = body.id.as_str().strip_prefix("sldprt:brep:body#").and_then(|s| s.parse::<u16>().ok())
            && targets.insert(id, AppearanceTarget::Body(body.id.clone())).is_some()
        {
            return Err("SLDPRT: ambiguous cached color owner".into());
        }
    }
    let mut assignments = BTreeMap::new();
    for (_, record) in attributes.values() {
        let Some((_, definition, owner, asset)) = record else { continue };
        let family = definitions.get(definition).and_then(Option::as_ref).and_then(|key| keys.get(key)).and_then(Option::as_ref);
        if family.copied() != Some(b"SDL/TYSA_COLOUR".as_slice()) {
            continue;
        }
        // The compact bridge only supplies the face RGB assets for this layout.
        // Body attributes have another layout and are handled by the framed reader.
        let Some(target @ AppearanceTarget::Face(_)) = targets.get(owner) else { continue };
        let Some((appearance, color)) = assets.get(asset) else { return Err("SLDPRT: cached color asset is missing".into()) };
        unique(&mut assignments, target.clone(), (appearance.clone(), *color))?;
    }
    // Validate every assignment before changing any face, body or appearance binding.
    if assignments.values().any(Option::is_none) {
        return Err("SLDPRT: conflicting cached colors".into());
    }
    for (target, assignment) in &assignments {
        let Some((appearance, color)) = assignment else { continue };
        let existing = match target {
            AppearanceTarget::Face(id) => ir.model.faces.iter().find(|f| f.id == *id).and_then(|f| f.color),
            AppearanceTarget::Body(id) => ir.model.bodies.iter().find(|b| b.id == *id).and_then(|b| b.color),
            _ => None,
        };
        if existing.is_some_and(|old| old != *color)
            || ir.model.appearance_bindings.iter().any(|b| b.target == *target && b.appearance != *appearance)
        {
            return Err("SLDPRT: conflicting decoded and cached colors".into());
        }
    }
    let mut pending = Vec::new();
    for (target, assignment) in &assignments {
        let Some((appearance, _)) = assignment else { continue };
        if ir.model.appearance_bindings.iter().any(|b| b.target == *target) {
            continue;
        }
        let (kind, id) = match target {
            AppearanceTarget::Face(id) => ("Face", id.as_str()),
            AppearanceTarget::Body(id) => ("Body", id.as_str()),
            _ => return Err("SLDPRT: invalid color target".into()),
        };
        pending.push(AppearanceBinding {
            id: AppearanceBindingId::mint(format!(
                "sldprt:appearance:binding#cached:{}:{}",
                kind.to_ascii_lowercase(),
                id.rsplit_once('#').ok_or("SLDPRT: invalid color owner id")?.1
            ))
            .map_err(|e| e.to_string())?,
            target: target.clone(),
            appearance: appearance.clone(),
            source_entity_id: None,
            object_type: Some(kind.into()),
            visible: None,
            channels: BTreeMap::new(),
        });
    }
    for face in &mut ir.model.faces {
        if let Some(Some((_, color))) = assignments.get(&AppearanceTarget::Face(face.id.clone())) {
            face.color = Some(*color);
        }
    }
    for body in &mut ir.model.bodies {
        if let Some(Some((_, color))) = assignments.get(&AppearanceTarget::Body(body.id.clone())) {
            body.color = Some(*color);
        }
    }
    ir.model.appearance_bindings.extend(pending);
    Ok(assignments.len())
}
