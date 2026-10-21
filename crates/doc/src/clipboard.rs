//! Copies of features (timeline Copy / Paste): references to sketches and features inside the
//! copied set are moved to the copies.

use std::collections::BTreeMap;

use crate::{FeatureKind, PlaneRef};

/// Remap persistent topology tokens without rewriting arbitrary user expressions or labels.
pub fn remap_topology(name: &str, ids: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>) -> String {
    // An older design's name keeps its mark; what follows it is remapped as any name.
    if let Some(old) = crate::naming::legacy_name(name) {
        return format!("{}{}", crate::naming::LEGACY_MARK, remap_topology(old, ids, bodies));
    }
    // A flat face of a sheet: `sheet:<body>:<panel>:<side>`, the panel a flange's feature token.
    if let Some((body, face)) = crate::sheet::split_sheet_face_name(name) {
        return crate::sheet::sheet_face_name(bodies.get(body).map_or(body, String::as_str), &remap_topology(face, ids, bodies));
    }
    // Imported names belong to the complete canonical body prefix, even when that prefix
    // happens to contain text resembling a feature token.
    if let Some((body, suffix)) = name.rsplit_once(":face:")
        && let Some(new) = bodies.get(body)
    {
        return format!("{new}:face:{suffix}");
    }
    let mut out = String::new();
    let mut at = 0;
    while at < name.len() {
        let rest = &name[at..];
        // A feature token starts a name, or one inside it: after `@`, `:` or `|`, or in a
        // piece's list of the faces around it (`[`, `,`).
        if rest.starts_with('F') && (at == 0 || matches!(name.as_bytes().get(at - 1), Some(b'@' | b':' | b'|' | b'[' | b','))) {
            let end = rest.as_bytes().iter().skip(1).take_while(|b| b.is_ascii_digit()).count() + 1;
            if end > 1
                && matches!(rest.as_bytes().get(end), Some(b':') | Some(b'.'))
                && let Ok(id) = rest[1..end].parse::<u64>()
                && let Some(new) = ids.get(&id)
            {
                out.push_str(&format!("F{new}"));
                at += end;
                continue;
            }
        }
        let Some(ch) = rest.chars().next() else { break };
        out.push(ch);
        at += ch.len_utf8();
    }
    // Import faces have a canonical body prefix rather than a feature token.
    if let Some((body, suffix)) = out.rsplit_once(":face:")
        && let Some(new) = bodies.get(body)
    {
        return format!("{new}:face:{suffix}");
    }
    out
}

fn mapped_name(n: &mut String, names: &BTreeMap<String, String>) {
    if let Some(new) = names.get(n) {
        *n = new.clone();
    }
}

/// Simultaneous generated-parameter substitution: map values may themselves be old keys.
pub fn clone_expression(source: &str, parameters: &BTreeMap<String, String>) -> String {
    let mut result = source.to_string();
    let mut temporary = Vec::new();
    for (index, (old, new)) in parameters.iter().enumerate() {
        let mut token = format!("__clone_reference_{index}");
        while source.contains(&token) || parameters.values().any(|n| n == &token) {
            token.push('_');
        }
        result = crate::expr::rename_reference(&result, old, &token);
        temporary.push((token, new));
    }
    for (token, new) in temporary {
        result = crate::expr::rename_reference(&result, &token, new);
    }
    result
}

fn remap_geo(g: &mut crate::construct::GeoRef, ids: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>, depth: usize) -> crate::Result<()> {
    use crate::construct::GeoRef;
    if depth > 64 {
        return Err(crate::DocError::Invalid("clone plane reference depth exceeded".into()));
    }
    match g {
        GeoRef::SketchPoint { sketch, .. } | GeoRef::SketchCurve { sketch, .. } => {
            if let Some(new) = ids.get(sketch) {
                *sketch = *new;
            }
        }
        GeoRef::Face { name: Some(n), .. } | GeoRef::Edge { name: Some(n), .. } => *n = remap_topology(n, ids, bodies),
        GeoRef::Vertex { edges: Some(names), .. } => {
            for n in names {
                *n = remap_topology(n, ids, bodies);
            }
        }
        GeoRef::Plane { plane } => remap_plane(plane, ids, bodies, depth + 1)?,
        _ => {}
    }
    Ok(())
}

fn remap_plane(p: &mut PlaneRef, ids: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>, depth: usize) -> crate::Result<()> {
    if depth > 64 {
        return Err(crate::DocError::Invalid("clone plane reference depth exceeded".into()));
    }
    match p {
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => remap_plane(base, ids, bodies, depth + 1)?,
        PlaneRef::AtAngle { base, line, .. } => {
            remap_plane(base, ids, bodies, depth + 1)?;
            if let Some(g) = line {
                remap_geo(g, ids, bodies, depth + 1)?;
            }
        }
        PlaneRef::Face { name, anchor, .. } => {
            if let Some(name) = name {
                *name = remap_topology(name, ids, bodies);
            }
            if let Some(anchor) = anchor {
                anchor.remap_target(ids, bodies)?;
            }
        }
        _ => {
            for r in p.refs_mut() {
                remap_geo(r, ids, bodies, depth + 1)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn remap_link(
    source: &mut solidcraft_sketch::LinkSource,
    ids: &BTreeMap<u64, u64>,
    names: &BTreeMap<String, String>,
    bodies: &BTreeMap<String, String>,
    depth: usize,
) {
    use solidcraft_sketch::LinkSource;
    if depth > 64 {
        return;
    }
    match source {
        LinkSource::Edge { body, .. }
        | LinkSource::Face { body, .. }
        | LinkSource::Body { body }
        | LinkSource::Vertex { body, .. }
        | LinkSource::Iso { body, .. }
        | LinkSource::Spun { body, .. } => mapped_name(body, bodies),
        LinkSource::SketchCurve { sketch, .. } | LinkSource::SketchPoint { sketch, .. } => {
            if let Some(new) = ids.get(sketch) {
                *sketch = *new;
            }
        }
        LinkSource::OnSurface { sketch, body, .. } => {
            if let Some(new) = ids.get(sketch) {
                *sketch = *new;
            }
            mapped_name(body, bodies);
        }
        LinkSource::Intersection { a, b } => {
            remap_link(a, ids, names, bodies, depth + 1);
            remap_link(b, ids, names, bodies, depth + 1);
        }
        LinkSource::Plane { name } => mapped_name(name, names),
        _ => {}
    }
}

impl crate::Feature {
    /// Clone identities through typed reference fields. Sketch-local entity IDs stay local.
    pub fn remap_clone(
        &mut self,
        ids: &BTreeMap<u64, u64>,
        names: &BTreeMap<String, String>,
        bodies: &BTreeMap<String, String>,
        parameters: &BTreeMap<String, String>,
    ) -> crate::Result<()> {
        use FeatureKind as K;
        match &self.kind {
            K::Sketch { plane, sketch } => {
                checked_plane(plane, 0)?;
                for link in &sketch.links {
                    checked_link(&link.source, 0)?;
                }
            }
            K::ConstructionPlane { plane }
            | K::Split { plane, .. }
            | K::SplitFace { plane, .. }
            | K::Mirror { plane, .. }
            | K::SurfaceTrim { plane, .. } => checked_plane(plane, 0)?,
            K::Draft { neutral, .. } => checked_plane(neutral, 0)?,
            K::ReplaceFace { target, .. } => checked_plane(target, 0)?,
            K::SurfaceExtend { up_to: Some(crate::ExtendTo::Plane(plane)), .. } => checked_plane(plane, 0)?,
            K::ConstructionAxis { def } => {
                for r in def.refs() {
                    checked_geo(r, 0)?;
                }
            }
            K::ConstructionPoint { def } => {
                for r in def.refs() {
                    checked_geo(r, 0)?;
                }
            }
            K::Revolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::SurfaceRevolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::Pattern { pattern: crate::PatternKind::Circular { axis_ref: Some(geo), .. }, .. } => checked_geo(geo, 0)?,
            _ => {}
        }
        // Feature names are remapped once here; the geometry pass below handles only
        // sketch ids and topology, so a copy whose new name is also an old key is not remapped twice.
        self.kind.remap_refs(ids, names);
        for n in &mut self.body_names {
            mapped_name(n, bodies);
        }
        for n in &mut self.participants {
            mapped_name(n, bodies);
        }
        for n in &mut self.param_names {
            mapped_name(n, parameters);
        }
        for n in self.face_names.iter_mut().chain(&mut self.edge_names) {
            *n = remap_topology(n, ids, bodies);
        }
        for (_, e, _) in self.kind.inputs_mut() {
            *e = clone_expression(e, parameters).into();
        }
        for e in self.kind.placement_exprs_mut() {
            *e = clone_expression(e, parameters).into();
        }
        self.kind.remap_bodies(bodies);
        match &mut self.kind {
            K::Sketch { plane, sketch } => {
                remap_plane(plane, ids, bodies, 0)?;
                for c in &mut sketch.constraints {
                    if let Some(n) = &mut c.param {
                        mapped_name(n, parameters);
                    }
                }
                for e in sketch.text_exprs_mut() {
                    *e = clone_expression(e, parameters);
                }
                for link in &mut sketch.links {
                    remap_link(&mut link.source, ids, names, bodies, 0);
                    for n in link.face_name.iter_mut().chain(&mut link.edge_name) {
                        *n = remap_topology(n, ids, bodies);
                    }
                }
            }
            K::ConstructionPlane { plane }
            | K::Split { plane, .. }
            | K::SplitFace { plane, .. }
            | K::Mirror { plane, .. }
            | K::SurfaceTrim { plane, .. } => remap_plane(plane, ids, bodies, 0)?,
            K::Draft { neutral, .. } => remap_plane(neutral, ids, bodies, 0)?,
            K::ReplaceFace { target, .. } => remap_plane(target, ids, bodies, 0)?,
            K::SurfaceExtend { up_to: Some(crate::ExtendTo::Plane(plane)), .. } => remap_plane(plane, ids, bodies, 0)?,
            K::ConstructionAxis { def } => {
                for r in def.refs_mut() {
                    remap_geo(r, ids, bodies, 0)?;
                }
            }
            K::ConstructionPoint { def } => {
                for r in def.refs_mut() {
                    remap_geo(r, ids, bodies, 0)?;
                }
            }
            K::Revolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::SurfaceRevolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::Pattern { pattern: crate::PatternKind::Circular { axis_ref: Some(geo), .. }, .. } => remap_geo(geo, ids, bodies, 0)?,
            _ => {}
        }
        Ok(())
    }
}

fn checked_geo(g: &crate::construct::GeoRef, depth: usize) -> crate::Result<()> {
    if depth > 64 {
        return Err(crate::DocError::Invalid("copy reference nesting exceeds 64".into()));
    }
    if let crate::construct::GeoRef::Plane { plane } = g {
        checked_plane(plane, depth + 1)?;
    }
    Ok(())
}
fn checked_plane(p: &PlaneRef, depth: usize) -> crate::Result<()> {
    if depth > 64 {
        return Err(crate::DocError::Invalid("copy reference nesting exceeds 64".into()));
    }
    match p {
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => checked_plane(base, depth + 1)?,
        PlaneRef::AtAngle { base, line, .. } => {
            checked_plane(base, depth + 1)?;
            if let Some(g) = line {
                checked_geo(g, depth + 1)?;
            }
        }
        PlaneRef::Midplane { a, b } | PlaneRef::TwoEdges { a, b } => {
            checked_geo(a, depth + 1)?;
            checked_geo(b, depth + 1)?;
        }
        PlaneRef::Perpendicular { plane, line } => {
            checked_geo(plane, depth + 1)?;
            checked_geo(line, depth + 1)?;
        }
        PlaneRef::ThreePoints { a, b, c } => {
            checked_geo(a, depth + 1)?;
            checked_geo(b, depth + 1)?;
            checked_geo(c, depth + 1)?;
        }
        PlaneRef::Tangent { face, .. } => checked_geo(face, depth + 1)?,
        PlaneRef::AlongPath { path, .. } => checked_geo(path, depth + 1)?,
        _ => {}
    }
    Ok(())
}
fn checked_link(source: &solidcraft_sketch::LinkSource, depth: usize) -> crate::Result<()> {
    if depth > 64 {
        return Err(crate::DocError::Invalid("copy link nesting exceeds 64".into()));
    }
    if let solidcraft_sketch::LinkSource::Intersection { a, b } = source {
        checked_link(a, depth + 1)?;
        checked_link(b, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod clone_tests {
    use super::*;

    #[test]
    fn construction_feature_names_are_remapped_only_once_when_copied() {
        use serde_json::json;

        let names = BTreeMap::from([("Old".into(), "New".into()), ("New".into(), "Wrong".into())]);
        let origin = json!({"type": "plane", "plane": {"type": "construction", "name": "Old"}});
        for kind in [
            json!({"type": "construction_axis", "def": {"type": "two_points", "a": {"type": "construct_point", "name": "Old"}, "b": {"type": "point", "p": [1, 2, 3]}}}),
            json!({"type": "construction_point", "def": {"type": "edge_and_plane", "edge": {"type": "axis", "name": "Old"}, "plane": origin}}),
            json!({"type": "construction_plane", "plane": {"type": "perpendicular", "plane": origin, "line": {"type": "axis", "name": "Old"}}}),
        ] {
            let mut value = kind;
            value["id"] = json!(1);
            value["name"] = json!("Copy");
            let mut feature: crate::Feature = serde_json::from_value(value).unwrap();
            let before = serde_json::to_string(&feature).unwrap();
            feature.remap_clone(&BTreeMap::new(), &names, &BTreeMap::new(), &BTreeMap::new()).unwrap();
            assert_eq!(serde_json::to_string(&feature).unwrap(), before.replace("\"Old\"", "\"New\""));
        }
    }

    #[test]
    fn imported_names_and_topology_tokens_have_separate_identity() {
        let ids = BTreeMap::from([(1, 2), (10, 20)]);
        let bodies = BTreeMap::from([("F1:Housing".into(), "F1:Housing (copy)".into())]);
        assert_eq!(remap_topology("F1:Housing:face:0", &ids, &bodies), "F1:Housing (copy):face:0");
        assert_eq!(remap_topology("F1:+x|F10:end@F1.2", &ids, &bodies), "F2:+x|F20:end@F2.2");
        assert_eq!(remap_topology("UserF1:label", &ids, &bodies), "UserF1:label");
    }
    #[test]
    fn a_sheet_face_follows_its_body_and_its_flange() {
        let ids = BTreeMap::from([(1, 2), (10, 20)]);
        let bodies = BTreeMap::from([("Body1".into(), "Body1 (copy)".into())]);
        assert_eq!(remap_topology("sheet:Body1:F1.0:top", &ids, &bodies), "sheet:Body1 (copy):F2.0:top");
        assert_eq!(remap_topology("sheet:Body1:base:bottom", &ids, &bodies), "sheet:Body1 (copy):base:bottom");
        assert_eq!(remap_topology("sheet:Other:F10.1:top", &ids, &bodies), "sheet:Other:F20.1:top");
    }
    #[test]
    fn parameter_remap_is_simultaneous_and_token_scoped() {
        let map = BTreeMap::from([("d1".into(), "d2".into()), ("d2".into(), "d3".into())]);
        assert_eq!(clone_expression("d1 + d2 + d10 + 2 mm", &map), "d2 + d3 + d10 + 2 mm");
    }
}
