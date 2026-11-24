//! Copies of features (timeline Copy / Paste): references to sketches and features inside the
//! copied set are moved to the copies.

use std::collections::{BTreeMap, BTreeSet};

use crate::persistent_name::{Base, EdgeName, FaceName, FeatureId, Rounds, Suffix};
use crate::{FeatureKind, PlaneRef};

/// Remap persistent topology tokens without rewriting arbitrary user expressions or labels.
pub fn remap_topology(name: &str, ids: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>) -> String {
    // An older design's name keeps its mark; what follows it is remapped as any name.
    if let Some(old) = crate::naming::legacy_name(name) {
        return format!("{}{}", crate::naming::LEGACY_MARK, remap_topology(old, ids, bodies));
    }
    // Imported names belong to the complete canonical body prefix, even when that prefix
    // happens to contain text resembling a feature token.
    let body_face = name.rsplit_once(":face:").and_then(|(body, suffix)| Some((bodies.get(body)?, body, suffix)));
    // An edge name (`A|B`) is remapped one face at a time, so a body face on one side doesn't
    // stop the other side's feature or body from being remapped (#13097). A body whose own
    // name holds a `|` is a single face, not an edge.
    if !body_face.is_some_and(|(_, body, _)| body.contains('|'))
        && let Ok(edge) = EdgeName::parse(name)
        && edge.faces.len() > 1
        && edge.to_string() == name
    {
        let faces: Vec<String> = edge.faces.iter().map(|face| remap_topology(&face.to_string(), ids, bodies)).collect();
        let tie = edge.tie.map(|k| format!("~{k}")).unwrap_or_default();
        return format!("{}{tie}", faces.join("|"));
    }
    if let Some((new, _, suffix)) = body_face {
        return format!("{new}:face:{suffix}");
    }
    // A name that reads is remapped by its structure: every feature it names, and a body
    // face's body. Text of no known form is remapped token by token below.
    if let Ok(face) = FaceName::parse(name)
        && !matches!(face.base, Base::Other(_))
        && let Some(mapped) = face.map(&mut |id, _| Some(ids.get(&id.0).map_or(id, |n| FeatureId(*n))), &mut |b| bodies.get(b).cloned())
    {
        return mapped.to_string();
    }
    let mut out = String::new();
    let mut at = 0;
    while at < name.len() {
        let rest = &name[at..];
        // A feature token starts a name, or one inside it: after `@`, `:` or `|`, in a piece's
        // list of the faces around it (`[`, `,`), or in a sheet panel's name after the panel it
        // was made on (`.`: `F15.F12.base.l2.far`).
        if rest.starts_with('F') && (at == 0 || matches!(name.as_bytes().get(at - 1), Some(b'@' | b':' | b'|' | b'[' | b',' | b'.'))) {
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

/// Rewrite faces through an exact snapshot correspondence. An unavailable or ambiguous
/// correspondence is `None`: a reference to it is refused, never picked again by proximity.
/// Replacements are simultaneous, including the faces inside blend names and piece qualifiers.
pub fn remap_topology_faces(
    name: &str,
    ids: &BTreeMap<u64, u64>,
    bodies: &BTreeMap<String, String>,
    faces: &BTreeMap<String, Option<String>>,
) -> crate::Result<String> {
    fn replacement(name: &str, faces: &BTreeMap<String, Option<String>>) -> crate::Result<Option<FaceName>> {
        let Some(mapped) = faces.get(name) else { return Ok(None) };
        let mapped = mapped.as_ref().ok_or_else(|| crate::DocError::Invalid(format!("copied face `{name}` has no unique exact correspondence")))?;
        FaceName::parse(mapped).map(Some).map_err(|_| crate::DocError::Invalid("invalid copied face name".into()))
    }
    fn face(
        name: FaceName,
        ids: &BTreeMap<u64, u64>,
        bodies: &BTreeMap<String, String>,
        faces: &BTreeMap<String, Option<String>>,
        changed: &mut bool,
    ) -> crate::Result<FaceName> {
        if name.suffixes.len() > 64 {
            return Err(crate::DocError::Invalid("copied face has too many qualifiers".into()));
        }
        // Find the frozen ancestor, keeping any later split or pattern qualifiers. New
        // snapshot ids must not pass through the original feature-id map a second time.
        let mut ancestor = name.clone();
        let mut trailing = Vec::new();
        let (mut mapped, suffixes) = loop {
            if let Some(mapped) = replacement(&ancestor.to_string(), faces)? {
                *changed = true;
                trailing.reverse();
                break (mapped, trailing);
            }
            if let Some(suffix) = ancestor.suffixes.pop() {
                trailing.push(suffix);
                continue;
            }
            let mut base = name.base;
            let base_text = FaceName { base: base.clone(), suffixes: Vec::new() }.to_string();
            let body_face = base_text.rsplit_once(":face:").is_some_and(|(body, _)| bodies.contains_key(body));
            if body_face {
                let text = remap_topology(&base_text, ids, bodies);
                base = FaceName::parse(&text).map_or_else(|_| Base::Other(text), |name| name.base);
            } else {
                match &mut base {
                    Base::Blend { rounds: Rounds::Edge(edge), .. } => {
                        let mut edge_changed = false;
                        for item in &mut edge.faces {
                            *item = face(item.clone(), ids, bodies, faces, &mut edge_changed)?;
                        }
                        check_edge_tie(edge, edge_changed)?;
                        *changed |= edge_changed;
                    }
                    Base::Inner { offsets, .. } => **offsets = face((**offsets).clone(), ids, bodies, faces, changed)?,
                    Base::BodyFace { body, .. } => mapped_name(body, bodies),
                    Base::Other(text) => *text = remap_topology(text, ids, bodies),
                    _ => {}
                }
                if let Base::Made { feature, .. } | Base::Blend { feature, .. } | Base::Inner { feature, .. } = &mut base {
                    feature.0 = ids.get(&feature.0).copied().unwrap_or(feature.0);
                }
            }
            break (FaceName { base, suffixes: Vec::new() }, name.suffixes);
        };
        for suffix in suffixes {
            mapped.suffixes.push(match suffix {
                Suffix::Copy { feature, instance } => {
                    Suffix::Copy { feature: FeatureId(ids.get(&feature.0).copied().unwrap_or(feature.0)), instance }
                }
                Suffix::Piece(q) => {
                    let around = q.around().iter().cloned().map(|n| face(n, ids, bodies, faces, changed)).collect::<crate::Result<Vec<_>>>()?;
                    Suffix::Piece(crate::persistent_name::Qualifier::new(around, q.tie).unwrap_or(q))
                }
                other => other,
            });
        }
        Ok(mapped)
    }
    fn check_edge_tie(edge: &EdgeName, changed: bool) -> crate::Result<()> {
        // A face correspondence does not prove the order of several edges between those
        // faces. Their mesh-derived ~k numbers can change when the snapshot is imported.
        if changed && edge.tie.is_some() {
            return Err(crate::DocError::Invalid("copied numbered edge has no unique exact correspondence".into()));
        }
        Ok(())
    }
    if faces.is_empty() {
        return Ok(remap_topology(name, ids, bodies));
    }
    if let Some(old) = crate::naming::legacy_name(name) {
        return Ok(format!("{}{}", crate::naming::LEGACY_MARK, remap_topology_faces(old, ids, bodies, faces)?));
    }
    // A canonical body name may itself contain `|`; check the whole face before parsing an edge.
    if let Some(mapped) = replacement(name, faces)? {
        return Ok(mapped.to_string());
    }
    if name.rsplit_once(":face:").is_some_and(|(body, _)| body.contains('|') && bodies.contains_key(body)) {
        return Ok(remap_topology(name, ids, bodies));
    }
    let Ok(mut edge) = EdgeName::parse(name) else { return Ok(remap_topology(name, ids, bodies)) };
    let mut changed = false;
    for item in &mut edge.faces {
        *item = face(item.clone(), ids, bodies, faces, &mut changed)?;
    }
    check_edge_tie(&edge, changed)?;
    Ok(edge.to_string())
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

/// The occurrence IDs allocated for a component copy and the root of the copied subtree.
pub struct ClonePlacements<'a> {
    pub occurrences: &'a BTreeMap<u64, u64>,
    /// All occurrences of the source root definition, including the one used to author a feature.
    pub source_roots: &'a BTreeSet<u64>,
    /// The selected destination path, ending at the copy's root placement. A copy into the second
    /// placement of a parent placed twice goes through that placement, not the parent's first one.
    pub root_path: &'a [u64],
    pub doc: &'a crate::Document,
    pub same_document: bool,
}

fn remap_geo(
    g: &mut crate::construct::GeoRef,
    ids: &BTreeMap<u64, u64>,
    bodies: &BTreeMap<String, String>,
    depth: usize,
    placements: Option<&ClonePlacements<'_>>,
) -> crate::Result<()> {
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
        GeoRef::PlacedEdge { path, author_path, name, .. } => {
            *name = remap_topology(name, ids, bodies);
            // The copy has one placement, so an edge on a sibling placement of the copied
            // component cannot keep its offset from the placement the feature is written in.
            if let Some(p) = placements {
                let root_of = |path: &[u64]| path.iter().find(|id| p.source_roots.contains(id)).copied();
                if let (Some(edge), Some(author)) = (root_of(path), root_of(author_path))
                    && edge != author
                {
                    return Err(crate::DocError::Invalid(
                        "a copied reference picks an edge on another placement of the copied component: pick the edge again on this placement".into(),
                    ));
                }
            }
            if !path.is_empty() {
                remap_occurrence_path(path, placements, true)?;
            }
            if !author_path.is_empty() {
                remap_occurrence_path(author_path, placements, false)?;
            }
        }
        GeoRef::Vertex { edges: Some(names), .. } => {
            for n in names {
                *n = remap_topology(n, ids, bodies);
            }
        }
        GeoRef::Plane { plane } => remap_plane(plane, ids, bodies, depth + 1, placements)?,
        GeoRef::Datum { datum } => remap_occurrence_path(&mut datum.path, placements, true)?,
        GeoRef::Snap { of, .. } => remap_geo(of, ids, bodies, depth + 1, placements)?,
        _ => {}
    }
    Ok(())
}

fn remap_plane(
    p: &mut PlaneRef,
    ids: &BTreeMap<u64, u64>,
    bodies: &BTreeMap<String, String>,
    depth: usize,
    placements: Option<&ClonePlacements<'_>>,
) -> crate::Result<()> {
    if depth > 64 {
        return Err(crate::DocError::Invalid("clone plane reference depth exceeded".into()));
    }
    match p {
        PlaneRef::Datum { datum } => remap_occurrence_path(&mut datum.path, placements, true)?,
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => remap_plane(base, ids, bodies, depth + 1, placements)?,
        PlaneRef::ThroughPoint { base, point } => {
            remap_plane(base, ids, bodies, depth + 1, placements)?;
            remap_geo(point, ids, bodies, depth + 1, placements)?;
        }
        PlaneRef::AtAngle { base, line, pivot, .. } => {
            remap_plane(base, ids, bodies, depth + 1, placements)?;
            if let Some(g) = line {
                remap_geo(g, ids, bodies, depth + 1, placements)?;
            }
            if let Some(g) = pivot {
                remap_geo(g, ids, bodies, depth + 1, placements)?;
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
                remap_geo(r, ids, bodies, depth + 1, placements)?;
            }
        }
    }
    Ok(())
}

/// Map the selected occurrences inside the copied subtree through the destination's ancestors.
/// A reference may keep a valid external placement in the same document.
fn remap_occurrence_path(path: &mut Vec<u64>, placements: Option<&ClonePlacements<'_>>, keep_external: bool) -> crate::Result<()> {
    if path.is_empty() {
        return Err(crate::DocError::Invalid("a copied reference names no placement".into()));
    }
    let outside = || crate::DocError::Invalid("the copied reference names a placement outside the copied tree".into());
    let placements = placements.ok_or_else(outside)?;
    let broken = || crate::DocError::Invalid("the copied placement's path is broken".into());
    let root = *placements.root_path.last().ok_or_else(broken)?;
    let mut mapped = path
        .iter()
        .map(|id| if placements.source_roots.contains(id) { Some(root) } else { placements.occurrences.get(id).copied() })
        .skip_while(Option::is_none)
        .peekable();
    if mapped.peek().is_none() {
        return if keep_external && placements.same_document { placements.doc.placement_frame(path).map(|_| ()) } else { Err(outside()) };
    }
    let mapped = mapped.collect::<Option<Vec<_>>>().ok_or_else(outside)?;
    let mut remapped = placements.root_path.to_vec();
    if mapped.first() == Some(&root) {
        remapped.pop();
    }
    remapped.extend(mapped);
    placements.doc.placement_frame(&remapped).map_err(|_| broken())?;
    *path = remapped;
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
        LinkSource::Plane { name } | LinkSource::ConstructionPoint { name } => mapped_name(name, names),
        // A construction axis, by name (#9324); the origin axes are no feature's.
        LinkSource::Axis { name } if !matches!(name.as_str(), "X" | "Y" | "Z") => mapped_name(name, names),
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
        placements: Option<&ClonePlacements<'_>>,
    ) -> crate::Result<()> {
        use FeatureKind as K;
        match &self.kind {
            K::Sketch { plane, sketch } => {
                checked_plane(plane, 0)?;
                for link in &sketch.links {
                    checked_link(&link.source, 0)?;
                }
            }
            K::FrameEndCap { ends, .. } => {
                for end in ends {
                    checked_plane(&end.plane, 0)?;
                }
            }
            K::FrameTrim { to, .. } => {
                for target in to {
                    if let crate::frame::TrimTarget::Plane { plane } = target {
                        checked_plane(plane, 0)?;
                    }
                }
            }
            K::FrameGusset { supports, .. } => {
                for support in supports {
                    checked_plane(&support.plane, 0)?;
                }
            }
            K::FramePlate { plane, .. }
            | K::ConstructionPlane { plane }
            | K::Split { plane, .. }
            | K::SplitFace { plane, .. }
            | K::Mirror { plane, .. }
            | K::SurfaceTrim { plane, .. } => checked_plane(plane, 0)?,
            K::Draft { neutral, .. } => checked_plane(neutral, 0)?,
            K::ReplaceFace { target, .. } => checked_plane(target, 0)?,
            K::SurfaceExtend { up_to: Some(crate::ExtendTo::Plane(plane)), .. } => checked_plane(plane, 0)?,
            K::Thread { up_to: Some(crate::ThreadLimit::Plane(plane)), .. } => checked_plane(plane, 0)?,
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
            | K::Coil { axis_ref: Some(crate::AxisRef::Model { geo }), .. }
            | K::SurfaceRevolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::Pattern { pattern: crate::PatternKind::Circular { axis_ref: Some(geo), .. }, .. }
            | K::Move { rotate_about: Some(geo), .. } => checked_geo(geo, 0)?,
            K::FrameMember { pairs, .. } => {
                for p in pairs {
                    checked_geo(&p.a, 0)?;
                    checked_geo(&p.b, 0)?;
                }
            }
            K::Scale { about: Some(crate::ScaleAbout::Pick { at }), .. } => checked_geo(at, 0)?,
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
                remap_plane(plane, ids, bodies, 0, placements)?;
                for n in sketch.dimension_params_mut().into_iter().flatten() {
                    mapped_name(n, parameters);
                }
                for e in sketch.text_exprs_mut() {
                    *e = clone_expression(e, parameters);
                }
                // An equation curve's variable is its own: a parameter of that name is not renamed in it.
                for (variable, e) in sketch.equation_exprs_mut() {
                    let others: BTreeMap<String, String> =
                        parameters.iter().filter(|(old, _)| old.as_str() != variable).map(|(o, n)| (o.clone(), n.clone())).collect();
                    *e = clone_expression(e, &others);
                }
                for link in &mut sketch.links {
                    remap_link(&mut link.source, ids, names, bodies, 0);
                    for pin in &mut link.sketch_placements {
                        if let Some(id) = ids.get(&pin.sketch) {
                            pin.sketch = *id;
                        }
                    }
                    for n in link.face_name.iter_mut().chain(&mut link.edge_name).chain(&mut link.vertex_name).chain(&mut link.curve_edges) {
                        *n = remap_topology(n, ids, bodies);
                    }
                }
            }
            K::FrameEndCap { ends, .. } => {
                for end in ends {
                    remap_plane(&mut end.plane, ids, bodies, 0, placements)?;
                }
            }
            K::FrameTrim { to, .. } => {
                for target in to {
                    if let crate::frame::TrimTarget::Plane { plane } = target {
                        remap_plane(plane, ids, bodies, 0, placements)?;
                    }
                }
            }
            K::FrameGusset { supports, .. } => {
                for support in supports {
                    remap_plane(&mut support.plane, ids, bodies, 0, placements)?;
                }
            }
            K::FramePlate { plane, .. }
            | K::ConstructionPlane { plane }
            | K::Split { plane, .. }
            | K::SplitFace { plane, .. }
            | K::Mirror { plane, .. }
            | K::SurfaceTrim { plane, .. } => remap_plane(plane, ids, bodies, 0, placements)?,
            K::Draft { neutral, .. } => remap_plane(neutral, ids, bodies, 0, placements)?,
            K::ReplaceFace { target, .. } => remap_plane(target, ids, bodies, 0, placements)?,
            K::SurfaceExtend { up_to: Some(crate::ExtendTo::Plane(plane)), .. } => remap_plane(plane, ids, bodies, 0, placements)?,
            K::Thread { up_to: Some(crate::ThreadLimit::Plane(plane)), .. } => remap_plane(plane, ids, bodies, 0, placements)?,
            K::ConstructionAxis { def } => {
                for r in def.refs_mut() {
                    remap_geo(r, ids, bodies, 0, placements)?;
                }
            }
            K::ConstructionPoint { def } => {
                for r in def.refs_mut() {
                    remap_geo(r, ids, bodies, 0, placements)?;
                }
            }
            K::Revolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::Coil { axis_ref: Some(crate::AxisRef::Model { geo }), .. }
            | K::SurfaceRevolve { axis: crate::AxisRef::Model { geo }, .. }
            | K::Pattern { pattern: crate::PatternKind::Circular { axis_ref: Some(geo), .. }, .. }
            | K::Move { rotate_about: Some(geo), .. } => remap_geo(geo, ids, bodies, 0, placements)?,
            K::FrameMember { pairs, .. } => {
                for p in pairs {
                    remap_geo(&mut p.a, ids, bodies, 0, placements)?;
                    remap_geo(&mut p.b, ids, bodies, 0, placements)?;
                }
            }
            K::Scale { about: Some(crate::ScaleAbout::Pick { at }), .. } => remap_geo(at, ids, bodies, 0, placements)?,
            // An Align frame's joint origin finds its faces and edges by name, as the feature's
            // own face names are found.
            K::Align { from_frame, to_frame, .. } => {
                for fr in [from_frame, to_frame].into_iter().flatten() {
                    if let crate::AlignFrame::Origin { origin } = fr {
                        if let Some(sk) = origin.snap.sketch_feature_mut()
                            && let Some(&mapped) = ids.get(sk)
                        {
                            *sk = mapped;
                        }
                        for n in origin.snap.face_names_mut() {
                            *n = remap_topology(n, ids, bodies);
                        }
                    }
                }
            }
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
        PlaneRef::AtAngle { base, line, pivot, .. } => {
            checked_plane(base, depth + 1)?;
            if let Some(g) = line {
                checked_geo(g, depth + 1)?;
            }
            if let Some(g) = pivot {
                checked_geo(g, depth + 1)?;
            }
        }
        PlaneRef::Midplane { a, b, .. } | PlaneRef::TwoEdges { a, b } | PlaneRef::AxisPoint { axis: a, point: b } => {
            checked_geo(a, depth + 1)?;
            checked_geo(b, depth + 1)?;
        }
        PlaneRef::Perpendicular { plane, line } => {
            checked_geo(plane, depth + 1)?;
            checked_geo(line, depth + 1)?;
        }
        PlaneRef::ThroughPoint { base, point } => {
            checked_plane(base, depth + 1)?;
            checked_geo(point, depth + 1)?;
        }
        PlaneRef::ThreePoints { a, b, c } => {
            checked_geo(a, depth + 1)?;
            checked_geo(b, depth + 1)?;
            checked_geo(c, depth + 1)?;
        }
        PlaneRef::Tangent { face, reference, .. } => {
            checked_geo(face, depth + 1)?;
            if let Some(r) = reference {
                checked_geo(r, depth + 1)?;
            }
        }
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
    fn external_datum_paths_require_an_existing_placement_in_the_same_document() {
        let mut doc = crate::Document::new("datum paths");
        let external = doc.add_component(Some("External"), 0).unwrap();
        let external_placement = doc.occurrence_of(external).unwrap().id;
        let copy = doc.add_component(Some("Copy"), 0).unwrap();
        let root = doc.occurrence_of(copy).unwrap().id;
        let placements =
            ClonePlacements { occurrences: &BTreeMap::new(), source_roots: &BTreeSet::new(), root_path: &[root], doc: &doc, same_document: true };
        let original = vec![external_placement];
        let mut path = original.clone();
        remap_occurrence_path(&mut path, Some(&placements), true).unwrap();
        assert_eq!(path, original);
        let mut broken = vec![external_placement, root];
        assert!(remap_occurrence_path(&mut broken, Some(&placements), true).is_err());
        let other_document = ClonePlacements { same_document: false, ..placements };
        assert!(remap_occurrence_path(&mut path, Some(&other_document), true).is_err());
    }

    #[test]
    fn an_edge_on_a_sibling_placement_of_the_copied_component_is_refused() {
        let doc = crate::Document::new("sibling placements");
        let roots = BTreeSet::from([1, 2]);
        let placements = ClonePlacements { occurrences: &BTreeMap::new(), source_roots: &roots, root_path: &[9], doc: &doc, same_document: true };
        let mut edge = crate::construct::GeoRef::PlacedEdge {
            body: "Box".into(),
            path: vec![2],
            author_path: vec![1],
            at: solidcore_record::Vec3::ZERO,
            name: "e".into(),
        };
        let error = remap_geo(&mut edge, &BTreeMap::new(), &BTreeMap::new(), 0, Some(&placements)).unwrap_err();
        assert!(error.to_string().contains("another placement"), "{error}");
    }

    #[test]
    fn construction_feature_names_are_remapped_only_once_when_copied() {
        use serde_json::json;

        let names = BTreeMap::from([("Old".into(), "New".into()), ("New".into(), "Wrong".into())]);
        // Construction plane references are keyed by id (#1134): a copy remap moves the id map,
        // not this one; the axis and point names below are what a rename and a copy rewrite.
        let origin = json!({"type": "plane", "plane": {"type": "construction", "feature": 7}});
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
            feature.remap_clone(&BTreeMap::new(), &names, &BTreeMap::new(), &BTreeMap::new(), None).unwrap();
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
    fn an_edge_name_with_a_body_face_maps_every_face() {
        // #13097: a body face on either side of an edge must not stop the other side's
        // feature or body from being remapped.
        let ids: BTreeMap<u64, u64> = [(2, 20)].into_iter().collect();
        let bodies: BTreeMap<String, String> =
            [("Plate".to_string(), "Plate (copy)".to_string()), ("A|B".to_string(), "A|B (copy)".to_string())].into_iter().collect();
        assert_eq!(remap_topology("Plate:face:3|F2:top", &ids, &bodies), "Plate (copy):face:3|F20:top");
        assert_eq!(remap_topology("F2:top|Plate:face:3", &ids, &bodies), "F20:top|Plate (copy):face:3");
        assert_eq!(remap_topology("Plate:face:3|F2:top~1", &ids, &bodies), "Plate (copy):face:3|F20:top~1");
        // A body whose name holds a `|` is still one face.
        assert_eq!(remap_topology("A|B:face:0", &ids, &bodies), "A|B (copy):face:0");
    }
    #[test]
    fn frozen_face_references_follow_nested_names_and_later_pieces() {
        let faces = BTreeMap::from([
            ("F1:+z".into(), Some("F8:face:5".into())),
            ("F1:-y".into(), Some("F8:face:2".into())),
            // Replacement values are not looked up again.
            ("F8:face:5".into(), Some("F99:face:0".into())),
            ("F2:side".into(), None),
        ]);
        for (old, new) in [
            ("F1:+z|F1:-y", "F8:face:5|F8:face:2"),
            ("F10:blend(F1:+z|F1:-y)", "F10:blend(F8:face:5|F8:face:2)"),
            ("F10:inner(F1:+z)", "F10:inner(F8:face:5)"),
            ("F1:+z#[F11:side,F1:-y]", "F8:face:5#[F11:side,F8:face:2]"),
            ("F11:side#[F1:+z]", "F11:side#[F8:face:5]"),
            ("F1:+z@F11.1", "F8:face:5@F11.1"),
            ("F11:side", "F11:side"),
        ] {
            assert_eq!(remap_topology_faces(old, &BTreeMap::new(), &BTreeMap::new(), &faces).unwrap(), new);
        }
        for name in ["F2:side", "F1:+z|F2:side", "F10:blend(F1:+z|F2:side)", "F11:side#[F2:side]", "F1:+z|F1:-y~2", "F10:blend(F1:+z|F1:-y~2)"] {
            assert!(remap_topology_faces(name, &BTreeMap::new(), &BTreeMap::new(), &faces).is_err(), "{name}");
        }
        // Pasting into another design can reuse original ids for unrelated copied features.
        let ids = BTreeMap::from([(11, 1), (8, 12)]);
        assert_eq!(remap_topology_faces("F11:side|F12:top~2", &ids, &BTreeMap::new(), &faces).unwrap(), "F1:side|F12:top~2");
        assert_eq!(remap_topology_faces("F1:+z|F11:side", &ids, &BTreeMap::new(), &faces).unwrap(), "F8:face:5|F1:side");
        assert_eq!(remap_topology_faces("F1:+z@F11.1", &ids, &BTreeMap::new(), &faces).unwrap(), "F8:face:5@F1.1");
        let bodies = BTreeMap::from([("Body1".into(), "Body1 (copy)".into()), ("F11:Housing".into(), "F11:Housing (copy)".into())]);
        for name in ["F11:Housing:face:0", "Body1:face:0"] {
            assert_eq!(remap_topology_faces(name, &ids, &bodies, &faces).unwrap(), remap_topology(name, &ids, &bodies));
        }
        assert_eq!(remap_topology_faces("F1:+z|F11:Housing:face:0", &ids, &bodies, &faces).unwrap(), "F8:face:5|F11:Housing (copy):face:0");
    }
    #[test]
    fn parameter_remap_is_simultaneous_and_token_scoped() {
        let map = BTreeMap::from([("d1".into(), "d2".into()), ("d2".into(), "d3".into())]);
        assert_eq!(clone_expression("d1 + d2 + d10 + 2 mm", &map), "d2 + d3 + d10 + 2 mm");
    }
}
