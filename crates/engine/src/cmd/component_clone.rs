//! Independent copies of a component definition graph, assembled and evaluated atomically.
use solidcraft_doc::BodyName;
use std::collections::{BTreeMap, BTreeSet};

use crate::params::bad;
use crate::{Result, Sel, Session};
use serde_json::{Value, json};
use solidcraft_doc::{Document, Feature, FeatureKind, Mat, Occurrence};

const CMD: &str = "component.paste_new";
const MAX_WORK: usize = 10_000;

fn mesh_distance(mesh: &solidcraft_geom::Mesh, point: solidcraft_geom::Vec3) -> f64 {
    mesh.triangles
        .iter()
        .filter_map(|triangle| mesh.tri(triangle))
        .map(|[a, b, c]| {
            let normal = (b - a).cross(c - a);
            let squared = normal.dot(normal);
            let projected = if squared > 0. { point - normal * ((point - a).dot(normal) / squared) } else { point };
            if squared > 0. && [(a, b), (b, c), (c, a)].iter().all(|(start, end)| (*end - *start).cross(projected - *start).dot(normal) >= 0.) {
                return point.dist(projected);
            }
            [(a, b), (b, c), (c, a)]
                .iter()
                .map(|(start, end)| {
                    let edge = *end - *start;
                    let t = if edge.dot(edge) > 0. { ((point - *start).dot(edge) / edge.dot(edge)).clamp(0., 1.) } else { 0. };
                    point.dist(*start + edge * t)
                })
                .fold(f64::INFINITY, f64::min)
        })
        .fold(f64::INFINITY, f64::min)
}

fn material_probe_matches(a: &solidcraft_geom::Mesh, b: &solidcraft_geom::Mesh, point: solidcraft_geom::Vec3, tol_a: f64, tol_b: f64) -> bool {
    if !point.is_finite() || !tol_a.is_finite() || !tol_b.is_finite() || tol_a <= 0. || tol_b <= 0. {
        return false;
    }
    if a.contains(point) == b.contains(point) {
        return true;
    }
    // A chord approximation does not define material ON a boundary. Certify
    // nearby stock and void instead, outside both meshes' approximation strips.
    if mesh_distance(a, point) > tol_a || mesh_distance(b, point) > tol_b {
        return false;
    }
    let offset = 4. * tol_a.max(tol_b);
    let mut material = false;
    let mut void = false;
    for axis in [solidcraft_geom::Vec3::X, solidcraft_geom::Vec3::Y, solidcraft_geom::Vec3::Z] {
        for sign in [-1., 1.] {
            let q = point + axis * (sign * offset);
            if !q.is_finite() {
                return false;
            }
            if mesh_distance(a, q) <= tol_a || mesh_distance(b, q) <= tol_b {
                continue;
            }
            let inside = a.contains(q);
            if inside != b.contains(q) {
                return false;
            }
            material |= inside;
            void |= !inside;
        }
    }
    material && void
}

fn cloned_path(source: &Document, path: &[u64], root: u64, destination: &[u64], occurrences: &BTreeMap<u64, u64>) -> Result<Vec<u64>> {
    let start = path
        .iter()
        .position(|id| source.occurrences.iter().any(|o| o.id == *id && o.component == root))
        .ok_or_else(|| bad(CMD, "copied Emboss placement is outside the copied component tree"))?;
    let mut mapped = destination.to_vec();
    for id in path.iter().skip(start + 1) {
        mapped.push(*occurrences.get(id).ok_or_else(|| bad(CMD, "copied Emboss path has an external descendant"))?);
    }
    if mapped.len() > 64 {
        return Err(bad(CMD, "copied Emboss path exceeds placement depth limit"));
    }
    Ok(mapped)
}

/// A name for a copy of `base` that isn't in `used` (and then is): numbered, or with `keep`
/// (pasting into another design, where the original isn't) `base` itself while it is free.
fn unique(base: &str, used: &mut BTreeSet<String>, keep: bool) -> String {
    let name = if keep && !used.contains(base.trim()) { base.trim().to_string() } else { solidcraft_doc::copy_name(base, |n| used.contains(n)) };
    used.insert(name.clone());
    name
}

/// Whether `built`, the bodies a feature made, are the ones its `stored` names give, one for one. A
/// name is built as stored, or as `stored (n)` when a body of that name exists already: the solids
/// of a STEP file that share a label are stored as `X`, `X`, `X` and built as `X`, `X (2)`, `X (3)`,
/// and a second import of a file builds `Aa (2)` for the stored `Aa`.
pub(super) fn named_by(stored: &[String], built: &[String]) -> bool {
    !stored.is_empty()
        && stored.len() == built.len()
        && stored.iter().zip(built).all(|(stored, built)| {
            let base = stored.trim();
            !base.is_empty()
                && (built == base
                    || built
                        .strip_prefix(base)
                        .and_then(|rest| rest.strip_prefix(" (")?.strip_suffix(')'))
                        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        })
}

fn graph(doc: &Document, root: u64) -> Result<Vec<u64>> {
    let definitions: BTreeSet<_> = doc.components.iter().map(|c| c.id).collect();
    let mut children: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for occurrence in &doc.occurrences {
        children.entry(occurrence.parent).or_default().push(occurrence.component);
    }
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![(root, Vec::<u64>::new())];
    let mut work = 0;
    while let Some((id, mut ancestors)) = stack.pop() {
        work += 1;
        if work > MAX_WORK || ancestors.len() >= 64 {
            return Err(bad(CMD, "component graph exceeds copy work limit"));
        }
        if ancestors.contains(&id) {
            return Err(bad(CMD, "component graph contains a cycle"));
        }
        if !definitions.contains(&id) {
            return Err(bad(CMD, "component graph has a missing definition"));
        }
        if !seen.insert(id) {
            continue;
        }
        output.push(id);
        ancestors.push(id);
        if let Some(children) = children.get(&id) {
            if work.saturating_add(stack.len()).saturating_add(children.len()) > MAX_WORK {
                return Err(bad(CMD, "component graph exceeds copy work limit"));
            }
            for child in children.iter().rev() {
                stack.push((*child, ancestors.clone()));
            }
        }
    }
    // A shared node may be reached by a longer path after its first DFS visit. Check the
    // longest path over the complete DAG, rather than trusting the first placement depth.
    let mut indegree: BTreeMap<u64, usize> = output.iter().map(|id| (*id, 0)).collect();
    for parent in &output {
        if let Some(edges) = children.get(parent) {
            for child in edges {
                if let Some(degree) = indegree.get_mut(child) {
                    *degree += 1;
                }
            }
        }
    }
    let mut ready = std::collections::VecDeque::from([root]);
    let mut depth = BTreeMap::from([(root, 1usize)]);
    let mut sorted = Vec::new();
    while let Some(parent) = ready.pop_front() {
        sorted.push(parent);
        let parent_depth = *depth.get(&parent).unwrap_or(&1);
        if let Some(edges) = children.get(&parent) {
            for child in edges {
                let next = parent_depth + 1;
                if next > 64 {
                    return Err(bad(CMD, "component graph exceeds copy depth limit"));
                }
                depth.entry(*child).and_modify(|d| *d = (*d).max(next)).or_insert(next);
                if let Some(degree) = indegree.get_mut(child) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push_back(*child);
                    }
                }
            }
        }
    }
    if sorted.len() != output.len() {
        return Err(bad(CMD, "component graph contains a cycle"));
    }
    Ok(sorted)
}

/// The design Paste New copies from: its document, its evaluated model and its placements moved
/// but not yet applied. The session pasted into is the target; they are the same design for a
/// paste within one design.
pub(super) struct Source<'a> {
    pub doc: &'a Document,
    pub model: &'a solidcraft_doc::Model,
    pub pending_moves: &'a BTreeMap<u64, Mat>,
    /// Another design than the target: the copy can't point at anything outside it (it is
    /// refused, saying what), and the user parameters it uses come along.
    pub other_design: bool,
}

impl<'a> Source<'a> {
    /// The session's own design.
    pub fn of(s: &'a Session) -> Source<'a> {
        Source { doc: &s.doc, model: &s.model, pending_moves: &s.pending_moves, other_design: false }
    }

    /// What was copied in another design.
    pub fn copied(copy: &'a crate::ForeignCopy) -> Source<'a> {
        Source { doc: &copy.doc, model: &copy.model, pending_moves: &copy.pending_moves, other_design: true }
    }
}

/// An independent copy of `component` (and what it holds), from the session's own design, into
/// `parent`.
pub(super) fn paste(
    s: &mut Session,
    component: u64,
    source_occurrence: Option<Occurrence>,
    transform: Mat,
    parent: u64,
    params: &Value,
) -> Result<Value> {
    let (scratch, out) = build(s, &Source::of(s), component, source_occurrence, transform, parent, None, params)?;
    s.accept_computed(scratch);
    Ok(out)
}

/// [`paste`] from `origin` into `s`: the computed session to accept, and what the paste made.
/// `parent_path`: the placement of `parent` the copy is placed by (default: the source
/// placement's parent's).
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    s: &Session,
    origin: &Source<'_>,
    component: u64,
    source_occurrence: Option<Occurrence>,
    transform: Mat,
    parent: u64,
    parent_path: Option<Vec<u64>>,
    params: &Value,
) -> Result<(Session, Value)> {
    let source = origin.doc;
    let source_parameters: BTreeMap<_, _> = source.params.iter().map(|p| (p.name.as_str(), p)).collect();
    let generated_parameters: BTreeSet<_> = source
        .params
        .iter()
        .filter(|p| p.model)
        .map(|p| p.name.as_str())
        .chain(source.features.iter().flat_map(|f| f.param_names.iter().map(String::as_str)))
        .collect();
    let definitions = graph(source, component)?;
    let definition_records: BTreeMap<_, _> = source.components.iter().map(|c| (c.id, c)).collect();
    let selected: BTreeSet<_> = definitions.iter().copied().collect();
    if source.marker.is_some_and(|m| m < source.features.len()) {
        return Err(bad(CMD, "move the timeline marker to the end before Paste New"));
    }
    if origin.other_design && s.doc.marker.is_some_and(|m| m < s.doc.features.len()) {
        return Err(bad(CMD, "move the timeline marker to the end in this design before pasting"));
    }
    let feats: Vec<Feature> = source.features.iter().filter(|f| selected.contains(&f.component)).cloned().collect();
    if feats.len() + source.features.len() > MAX_WORK {
        return Err(bad(CMD, "too many features to copy"));
    }
    for f in &feats {
        if origin.model.result(f.id).is_some_and(|r| r.error.is_some()) {
            return Err(bad(CMD, format!("source feature {} has an evaluation error", f.name)));
        }
    }
    let state = origin.model.state();
    if state.bodies.len() > MAX_WORK {
        return Err(bad(CMD, "body set exceeds copy work limit"));
    }
    if source.next_id.checked_add(100_000).is_none() {
        return Err(bad(CMD, "copy id allocation exceeds supported range"));
    }
    let mut scratch = s.scratch();
    let mut components = BTreeMap::new();
    let mut occurrences = BTreeMap::new();
    let mut features = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut bodies = BTreeMap::new();
    let mut parameters = BTreeMap::new();
    // Names are numbered among their own kind, against the names the target has: a component
    // named after its body takes the same number as the body's copy, and neither skips one the
    // other took.
    let mut used_components: BTreeSet<String> = s.doc.components.iter().map(|c| c.name.clone()).collect();
    let mut used_features: BTreeSet<String> = s.doc.features.iter().map(|f| f.name.clone()).collect();
    let mut used_bodies: BTreeSet<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let mut new_ids = Vec::new();
    let mut frozen = Vec::new();
    // Allocate definitions, discarding their automatic placeholder occurrence except root.
    for old in &definitions {
        let c = definition_records.get(old).ok_or_else(|| bad(CMD, "missing definition"))?;
        let mapped_parent = if *old == component {
            parent
        } else {
            components.get(&c.parent).copied().unwrap_or(*components.get(&component).ok_or_else(|| bad(CMD, "missing clone root"))?)
        };
        let name = unique(&c.name, &mut used_components, origin.other_design);
        let new = scratch.doc_mut().add_component(Some(&name), mapped_parent)?;
        // A copy keeps the definition's part properties.
        if let Some(added) = scratch.doc_mut().components.iter_mut().find(|x| x.id == new) {
            added.metadata = c.metadata.clone();
            added.properties = c.properties.clone();
            added.identification_color = c.identification_color;
        }
        components.insert(*old, new);
        if *old != component {
            scratch.doc_mut().occurrences.retain(|o| o.component != new);
        }
    }
    let root = *components.get(&component).ok_or_else(|| bad(CMD, "missing clone root"))?;
    let root_occurrence = scratch.doc.occurrence_of(root).map(|o| o.id).ok_or_else(|| bad(CMD, "missing cloned placement"))?;
    let mut clone_path = if let Some(path) = parent_path {
        path
    } else if let Some(o) = &source_occurrence {
        let mut path = super::component::occurrence_path(s, params, o.id, CMD)?;
        path.pop();
        path
    } else {
        s.doc.first_path(parent)
    };
    clone_path.push(root_occurrence);
    if let Some(o) = scratch.doc_mut().occurrences.iter_mut().find(|o| o.id == root_occurrence) {
        o.transform = transform;
        // A copy starts free, even of a grounded part (#2804); the parts inside it keep theirs.
        o.grounded = false;
    }
    if let Some(o) = &source_occurrence {
        occurrences.insert(o.id, root_occurrence);
    }
    for old in source.occurrences.iter().filter(|o| selected.contains(&o.parent)) {
        let child = *components.get(&old.component).ok_or_else(|| bad(CMD, "missing child definition"))?;
        let owner = *components.get(&old.parent).ok_or_else(|| bad(CMD, "missing parent definition"))?;
        let pose = origin.pending_moves.get(&old.id).copied().unwrap_or(old.transform);
        if !super::component::valid_position(&pose) {
            return Err(bad(CMD, "invalid internal pending occurrence transform"));
        }
        let id = scratch.doc_mut().add_occurrence(child, owner, pose)?;
        if let Some(o) = scratch.doc_mut().occurrences.iter_mut().find(|o| o.id == id) {
            o.grounded = old.grounded;
        }
        occurrences.insert(old.id, id);
    }
    // Collect historical outputs too: a later Combine may have consumed a named tool body.
    let mut outputs: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    // The names the bodies of a feature have in the model, where they are its stored names, numbered.
    let mut shown: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for f in &feats {
        if let Some(result) = origin.model.result(f.id) {
            let created: Vec<_> = result.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.clone()).collect();
            for name in &created {
                bodies.entry(name.clone()).or_insert_with(|| unique(name, &mut used_bodies, origin.other_design));
            }
            if named_by(&f.body_names, &created) {
                shown.insert(f.id, created.clone());
            }
            outputs.insert(f.id, created);
        }
        // Later features name a body as the model shows it. Where that differs from the stored name,
        // the stored name is only a label (the first body of another import may be called so).
        if !shown.contains_key(&f.id) {
            for name in &f.body_names {
                if !name.is_empty() {
                    bodies.entry(name.clone()).or_insert_with(|| unique(name, &mut used_bodies, origin.other_design));
                }
            }
        }
        if bodies.len() > MAX_WORK {
            return Err(bad(CMD, "historical body set exceeds copy work limit"));
        }
    }
    // Body assignment may put externally authored geometry into this graph. Freeze it explicitly.
    let owned_features: BTreeSet<_> = feats.iter().map(|f| f.id).collect();
    for body in state.bodies.iter().filter(|b| selected.contains(&source.model_body_component(b)) && !owned_features.contains(&b.feature)) {
        if body.body.is_mesh() {
            return Err(bad(CMD, "Paste New of a reassigned mesh body is not supported yet"));
        }
        let offset = source.model_body_offset(body).copied().unwrap_or(solidcraft_doc::IDENTITY);
        // An external transfer establishes the local baseline in this graph.
        // Freeze AFTER that boundary, then replay it as an identity ownership event.
        let entry = feats.iter().find(|f| {
            !f.suppressed
                && matches!(&f.kind, FeatureKind::BodyTransfer { bodies, from, to, .. }
            if !selected.contains(from) && bodies.iter().any(|b| *b == body.name)
                && origin.model.result(f.id).is_some_and(|result| !result.skipped && result.error.is_none()
                    && result.state.body(&body.name).is_some_and(|entered| source.model_body_component(entered) == *to)))
        });
        let before = feats.iter().find_map(|f| {
            let before = origin.model.state_before(f.id);
            let prior = before.body(&body.name)?;
            let after = origin.model.result(f.id)?.state.body(&body.name);
            if after.is_none_or(|after| !prior.body.same(&after.body)) { Some(before) } else { None }
        });
        let entered = entry.and_then(|f| origin.model.result(f.id)).map(|r| r.state.clone());
        let base = entered.as_ref().and_then(|st| st.body(&body.name)).or_else(|| before.as_ref().and_then(|st| st.body(&body.name))).unwrap_or(body);
        let baseline_component = source.model_body_component(base);
        let geometry = solidcraft_kernel::transform_matrix(&base.body, offset)?;
        let step = solidcraft_kernel::step_export(&[&geometry], "SolidCraft independent reassigned body")?;
        let new_name = unique(&body.name, &mut used_bodies, origin.other_design);
        bodies.insert(body.name.clone(), new_name.clone());
        // From another design it is named as what it is there, an imported body, numbered as
        // the timeline numbers its features ("BodyImport1").
        let feature_name = if origin.other_design {
            let name =
                (1..=MAX_WORK + 1).map(|n| format!("BodyImport{n}")).find(|n| !used_features.contains(n)).unwrap_or_else(|| "BodyImport".into());
            used_features.insert(name.clone());
            name
        } else {
            unique(&format!("{} Base", body.name), &mut used_features, false)
        };
        let id = scratch.doc_mut().add_feature(
            FeatureKind::Import { file: format!("{} independent base", body.name), step: step.into(), components: Vec::new(), skip: Vec::new() },
            Some(&feature_name),
        )?;
        if let Some(f) = scratch.doc_mut().feature_mut(id) {
            f.component = *components.get(&baseline_component).ok_or_else(|| bad(CMD, "reassigned baseline is outside copied component graph"))?;
            f.body_names = vec![new_name];
        }
        new_ids.push(id);
        frozen.push(body.name.clone());
    }
    // Allocate all history IDs before rewriting any forward reference.
    for f in &feats {
        let name = unique(&f.name, &mut used_features, origin.other_design);
        // A transfer is checked as it is added: it names the copy's components (another design
        // has none of the original ones).
        let mut kind = f.kind.clone();
        transfer_into_copy(&mut kind, &components)?;
        let id = scratch.doc_mut().add_feature(kind, Some(&name))?;
        if let Some(new) = scratch.doc.feature(id) {
            for (old, pn) in f.param_names.iter().zip(&new.param_names) {
                parameters.insert(old.clone(), pn.clone());
            }
        }
        features.insert(f.id, id);
        names.insert(f.name.clone(), name);
        new_ids.push(id);
    }
    // Constraint dimensions own independent generated parameters; public user parameters stay shared.
    for f in &feats {
        if let FeatureKind::Sketch { sketch, .. } = &f.kind {
            for c in &sketch.constraints {
                if let Some(name) = &c.param
                    && !parameters.contains_key(name)
                    && let Some(p) = source_parameters.get(name.as_str())
                    && p.model
                {
                    if scratch.doc.params.len() >= solidcraft_doc::MAX_PARAMS {
                        return Err(bad(CMD, "too many copied parameters"));
                    }
                    let new = scratch.doc_mut().new_model_param(&p.expr, &p.unit);
                    parameters.insert(name.clone(), new);
                }
            }
        }
    }
    // From another design, the user parameters the copy uses come along (and the ones they use),
    // renamed `w_2` where this design has a `w` already.
    if origin.other_design {
        copy_user_parameters(&mut scratch, source, &feats, &mut parameters)?;
    }
    for (old, new) in &parameters {
        if let Some(comment) = source.param_comments.get(old) {
            scratch.doc_mut().param_comments.insert(new.clone(), comment.clone());
        }
        if let Some(p) = source_parameters.get(old.as_str())
            && p.model
        {
            let expr = solidcraft_doc::clone_expression(&p.expr, &parameters);
            if let Some(p) = scratch.doc_mut().params.iter_mut().find(|p| p.name == *new) {
                p.expr = expr;
                p.comment = source_parameters.get(old.as_str()).map(|p| p.comment.clone()).unwrap_or_default();
            }
        }
    }
    for old in &feats {
        let mut f = old.clone();
        f.id = *features.get(&old.id).ok_or_else(|| bad(CMD, "missing feature mapping"))?;
        f.name = names.get(&old.name).cloned().ok_or_else(|| bad(CMD, "missing feature name"))?;
        f.component = *components.get(&old.component).ok_or_else(|| bad(CMD, "missing component mapping"))?;
        if let Some(names) = shown.get(&old.id) {
            // Stored as `X`, `X`, `X` and built as `X`, `X (2)`, `X (3)`: the copy is stored under the
            // names of the bodies, so that it builds the names the copied features after it use.
            f.body_names = names.clone();
        } else if f.body_names.is_empty() {
            f.body_names = outputs.get(&old.id).cloned().unwrap_or_default();
        }
        f.remap_clone(&features, &names, &bodies, &parameters)?;
        transfer_into_copy(&mut f.kind, &components)?;
        if let FeatureKind::Emboss { face: Some(face), .. } = &mut f.kind
            && !face.path.is_empty()
            && let FeatureKind::Emboss { face: Some(old_face), .. } = &old.kind
            && bodies.contains_key(old_face.body.as_ref())
        {
            face.path = cloned_path(source, &old_face.path, component, &clone_path, &occurrences)?;
        }
        if let FeatureKind::Emboss { face: Some(face), .. } = &mut f.kind
            && !face.author_path.is_empty()
        {
            face.author_path = cloned_path(source, &face.author_path, component, &clone_path, &occurrences)?;
        }
        if let (
            FeatureKind::Sketch { plane, .. } | FeatureKind::FramePlate { plane, .. },
            FeatureKind::Sketch { plane: old_plane, .. } | FeatureKind::FramePlate { plane: old_plane, .. },
        ) = (&mut f.kind, &old.kind)
            && let (Some(anchor), Some(original)) = (super::sketch::face::anchor_mut(plane), super::sketch::face::anchor(old_plane))
        {
            if bodies.contains_key(&original.target.body) {
                anchor.component =
                    *components.get(&original.component).ok_or_else(|| bad(CMD, "anchored source component is outside copied graph"))?;
                anchor.path = cloned_path(source, &original.path, component, &clone_path, &occurrences)?;
            }
            anchor.author_path = cloned_path(source, &original.author_path, component, &clone_path, &occurrences)?;
        }
        if let Some(slot) = scratch.doc_mut().feature_mut(f.id) {
            *slot = f;
        }
    }
    for (old, new) in &bodies {
        if let Some(mat) = source.materials.get(old) {
            scratch.doc_mut().materials.insert(new.clone(), mat.clone());
        }
        if let Some(look) = source.appearances.bodies.get(old) {
            scratch.doc_mut().appearances.bodies.insert(new.clone(), look.clone());
        }
        if !frozen.contains(old) {
            if let Some(comp) = source.body_components.get(old).and_then(|c| components.get(c)) {
                scratch.doc_mut().body_components.insert(new.clone(), *comp);
            }
            if let Some(offset) = source.body_offsets.get(old) {
                scratch.doc_mut().body_offsets.insert(new.clone(), *offset);
            }
        }
    }
    for (old, new) in &components {
        if let Some(part) = source.parts.get(old) {
            scratch.doc_mut().parts.insert(*new, part.clone());
        }
        if let Some(look) = source.appearances.components.get(old) {
            scratch.doc_mut().appearances.components.insert(*new, look.clone());
        }
    }
    // A creator's history can belong here while its final body was reassigned outside.
    // Retain the independent history, but reproduce the current ownership at its end.
    let moved_out: Vec<_> = state
        .bodies
        .iter()
        .filter(|b| owned_features.contains(&b.feature) && !selected.contains(&source.model_body_component(b)))
        .filter_map(|b| bodies.get(&b.name).cloned())
        .collect();
    if !moved_out.is_empty() {
        let name = unique("Assigned-out bodies", &mut used_features, origin.other_design);
        let id = scratch.doc_mut().add_feature(FeatureKind::Remove { bodies: moved_out.into_iter().map(BodyName).collect() }, Some(&name))?;
        if let Some(f) = scratch.doc_mut().feature_mut(id) {
            f.component = root;
        }
        new_ids.push(id);
    }
    for face in &source.appearances.faces {
        if let Some(new) = bodies.get(&face.body) {
            let mut face = face.clone();
            if frozen.contains(&face.body) {
                if face.strict_name {
                    return Err(bad(CMD, "cannot copy strict face paint through uncertified foreign re-export"));
                }
                if let Some(name) = &face.name {
                    face.point = solidcraft_doc::naming::point_on_face(&state, name, face.point)
                        .map(|(point, _)| point)
                        .ok_or_else(|| bad(CMD, "source frozen face appearance lost its identity"))?;
                }
                let source_body = state.body(&face.body).ok_or_else(|| bad(CMD, "source frozen face appearance lost its body"))?;
                face.point = solidcraft_doc::apply_point(source.model_body_offset(source_body).unwrap_or(&solidcraft_doc::IDENTITY), face.point);
                face.name = None;
            } else if let Some(name) = &mut face.name {
                *name = solidcraft_doc::remap_topology(name, &features, &bodies);
            }
            face.body = new.clone();
            if let Some(certificate) = &mut face.certificate {
                certificate.remap_features(&features);
            }
            scratch.doc_mut().appearances.faces.push(face);
        }
    }
    // Typed configuration and assembly helpers preserve original targets (another design's
    // configurations stay with it: this design's table has its own targets).
    if !origin.other_design {
        scratch.doc_mut().configs.clone_targets(&solidcraft_doc::config::ConfigCloneMaps {
            features: features.clone(),
            components: components.clone(),
            bodies: bodies.clone(),
            parameters: parameters.clone(),
        })?;
    }
    let mut notices = super::component_clone_assembly::clone_assembly(scratch.doc_mut(), source, &occurrences, &features, &bodies)?;
    // Joints and motion to the outside are left behind (those notices); what follows is what
    // the copied features point at outside it.
    let assembly_notices = notices.len();
    let mut reference_work = 0usize;
    for feature in &feats {
        if let FeatureKind::Emboss { face: Some(face), .. } = &feature.kind
            && !face.path.is_empty()
            && !bodies.contains_key(face.body.as_ref())
        {
            notices.push(format!("{} retains external selected occurrence path {:?}", feature.name, face.path));
        }
        if let FeatureKind::Sketch { sketch, .. } = &feature.kind {
            use solidcraft_sketch::LinkSource as L;
            let mut pending: Vec<_> = sketch.links.iter().map(|l| &l.source).collect();
            let mut work = 0;
            while let Some(link) = pending.pop() {
                work += 1;
                reference_work += 1;
                if work > MAX_WORK || reference_work > 100_000 {
                    return Err(bad(CMD, "linked references exceed copy work limit"));
                }
                match link {
                    L::Edge { body, .. }
                    | L::Face { body, .. }
                    | L::Body { body }
                    | L::Vertex { body, .. }
                    | L::Iso { body, .. }
                    | L::Spun { body, .. } => {
                        if !bodies.contains_key(body) {
                            notices.push(format!("{} retains external linked body {body}", feature.name));
                        }
                    }
                    L::SketchCurve { sketch, .. } | L::SketchPoint { sketch, .. } => {
                        if !features.contains_key(sketch) {
                            notices.push(format!("{} retains external linked sketch {sketch}", feature.name));
                        }
                    }
                    L::OnSurface { sketch, body, .. } => {
                        if !features.contains_key(sketch) || !bodies.contains_key(body) {
                            notices.push(format!("{} retains external surface link", feature.name));
                        }
                    }
                    L::Intersection { a, b } => {
                        pending.push(a);
                        pending.push(b);
                    }
                    L::Plane { name } if source.find_feature(name).is_some() && !names.contains_key(name) => {
                        notices.push(format!("{} retains external linked plane {name}", feature.name));
                    }
                    _ => {}
                }
            }
        }
        if let FeatureKind::Sketch { plane, .. } | FeatureKind::FramePlate { plane, .. } = &feature.kind
            && let Some(anchor) = super::sketch::face::anchor(plane)
            && !bodies.contains_key(&anchor.target.body)
        {
            notices.push(format!(
                "{} retains external exact face attachment to {} (producer {})",
                feature.name, anchor.target.body, anchor.target.origin_feature
            ));
        }
        for id in feature.kind.input_sketches() {
            if !features.contains_key(&id) {
                notices.push(format!("{} retains an external sketch reference to {id}", feature.name));
            }
        }
        if let FeatureKind::Pattern { features: references, .. } | FeatureKind::Mirror { features: references, .. } = &feature.kind {
            for id in references {
                if !features.contains_key(&id.0) {
                    notices.push(format!("{} retains external feature reference {id}", feature.name));
                }
            }
        }
        for expression in feature.kind.expressions() {
            for name in solidcraft_doc::expr::references(expression) {
                reference_work += 1;
                if reference_work > 100_000 {
                    return Err(bad(CMD, "expression references exceed copy work limit"));
                }
                if !parameters.contains_key(&name) && generated_parameters.contains(name.as_str()) {
                    notices.push(format!("{} retains external model parameter {name}", feature.name));
                }
            }
        }
    }
    if origin.other_design {
        // What the copy points at outside itself isn't in this design.
        let mut outside = outside_references(notices.get(assembly_notices..).unwrap_or_default(), &feats, &bodies);
        if !outside.is_empty() {
            outside.sort();
            outside.dedup();
            let name = definition_records.get(&component).map(|c| c.name.clone()).unwrap_or_default();
            return Err(bad(CMD, format!("{name} uses what isn't in it, so it can't be pasted into another design: {}", outside.join("; "))));
        }
    }
    notices.sort();
    notices.dedup();
    scratch.refresh();
    let original_errors: BTreeMap<_, _> = s.model.results.iter().map(|result| (result.id, result.error.as_ref())).collect();
    let refreshed_errors: BTreeMap<_, _> = scratch.model.results.iter().map(|result| (result.id, result.error.as_ref())).collect();
    if s.doc.features.iter().any(|feature| original_errors.get(&feature.id) != refreshed_errors.get(&feature.id)) {
        return Err(bad(CMD, "copy would change an original feature's error state"));
    }
    // Base imports acquire new face identities. Resolve copied face overrides against the
    // verified new geometry once, rather than weakening strict persistent-name semantics.
    let resolved = scratch.model.state();
    let mut face_updates = Vec::new();
    for (index, face) in scratch.doc.appearances.faces.iter().enumerate() {
        if bodies.iter().any(|(old, new)| frozen.contains(old) && *new == face.body) {
            let body = resolved.body(&face.body).ok_or_else(|| bad(CMD, "missing frozen face body"))?;
            let face_index =
                solidcraft_doc::appearance::face_index_at(body, face.point).ok_or_else(|| bad(CMD, "cannot reproduce frozen face appearance"))?;
            let name = solidcraft_doc::naming::face_names(body).get(face_index).cloned().ok_or_else(|| bad(CMD, "missing frozen face identity"))?;
            face_updates.push((index, name));
        }
    }
    for (index, name) in face_updates {
        if let Some(face) = scratch.doc_mut().appearances.faces.get_mut(index) {
            face.name = Some(name);
        }
    }
    for id in &new_ids {
        if let Some(error) = scratch.model.result(*id).and_then(|r| r.error.as_ref()) {
            return Err(bad(CMD, format!("copied feature failed: {error}")));
        }
    }
    let copied = scratch.model.state();
    let copied_bodies: BTreeMap<_, _> = copied.bodies.iter().map(|body| (body.name.as_str(), body)).collect();
    // Adding placements can invalidate cached source evaluation. Compare clone effects
    // against that same evaluation's source prefix, not older shared B-rep pointers.
    if scratch.doc.features.get(..s.doc.features.len()) != Some(s.doc.features.as_slice())
        || scratch.doc.params.get(..s.doc.params.len()) != Some(s.doc.params.as_slice())
    {
        return Err(bad(CMD, "copy would change original feature history or parameters"));
    }
    let original_state = new_ids.first().map(|id| scratch.model.state_before(*id)).unwrap_or_else(|| copied.clone());
    let all_columns = std::mem::take(&mut scratch.doc_mut().configs.columns);
    scratch.doc_mut().configs.columns = all_columns.iter().skip(s.doc.configs.columns.len()).cloned().collect();
    let style_validation = scratch.doc.configuration_style_status(&copied);
    scratch.doc_mut().configs.columns = all_columns;
    style_validation?;
    let mut proof_work = 0usize;
    let expected_count = state.bodies.iter().filter(|b| selected.contains(&source.model_body_component(b))).count();
    let copied_components: BTreeSet<_> = components.values().copied().collect();
    let actual_count = copied.bodies.iter().filter(|b| copied_components.contains(&scratch.doc.model_body_component(b))).count();
    if actual_count != expected_count {
        return Err(bad(CMD, "copy produced an incomplete or unexpected set of bodies"));
    }
    for original in &original_state.bodies {
        let Some(after) = copied_bodies.get(original.name.as_str()) else {
            return Err(bad(CMD, "copy would remove a source body outside its independent history"));
        };
        if !original.body.same(&after.body) {
            return Err(bad(CMD, "copy would modify a source body; external destructive dependencies are not supported"));
        }
    }
    for body in state.bodies.iter().filter(|b| selected.contains(&source.model_body_component(b))) {
        let new = bodies.get(&body.name).ok_or_else(|| bad(CMD, format!("body {} has no clone identity", body.name)))?;
        let cloned = copied_bodies.get(new.as_str()).ok_or_else(|| bad(CMD, format!("copy did not reproduce body {}", body.name)))?;
        let a = solidcraft_kernel::transform_matrix(&body.body, *source.model_body_offset(body).unwrap_or(&solidcraft_doc::IDENTITY))?;
        let b = solidcraft_kernel::transform_matrix(&cloned.body, *scratch.doc.model_body_offset(cloned).unwrap_or(&solidcraft_doc::IDENTITY))?;
        let ma = solidcraft_kernel::measure(&a)?;
        let mb = solidcraft_kernel::measure(&b)?;
        let tolerance = 1e-5 * (1.0 + (ma.bbox.max - ma.bbox.min).len()) + 64.0 * f64::EPSILON * (ma.bbox.min.len() + ma.bbox.max.len());
        let mut material_matches = true;
        let tol_a = (a.size() * 1e-3).max(1e-3).max(a.size() * 1e-6);
        let tol_b = (b.size() * 1e-3).max(1e-3).max(b.size() * 1e-6);
        let mesh_a = a.display_mesh(tol_a)?;
        let mesh_b = b.display_mesh(tol_b)?;
        // Precharge all 27 candidates, six possible replacements, their triangle
        // distances and the three containment rays before any proof traversal.
        proof_work = proof_work.saturating_add(mesh_a.triangles.len().saturating_add(mesh_b.triangles.len()).saturating_mul(27 * 7 * 4));
        if proof_work > 50_000_000 {
            return Err(bad(CMD, "geometry verification exceeds copy work limit"));
        }
        for x in [0.2, 0.5, 0.8] {
            for y in [0.2, 0.5, 0.8] {
                for z in [0.2, 0.5, 0.8] {
                    let d = ma.bbox.max - ma.bbox.min;
                    let p = ma.bbox.min + solidcraft_geom::Vec3::new(d.x * x, d.y * y, d.z * z);
                    let roundoff = 64. * f64::EPSILON * (1. + p.len());
                    material_matches &= material_probe_matches(&mesh_a, &mesh_b, p, tol_a + roundoff, tol_b + roundoff);
                }
            }
        }
        // Meshes of the same current circular section can have different
        // chord-volume biases after STEP. Only independently certified current
        // sections may replace that volume comparison; shape mismatches still
        // refuse even when their volumes happen to agree.
        let volume_matches = match solidcraft_kernel::certified_prism_equivalent(&a, &b)? {
            Some(same) => same,
            None => (ma.volume - mb.volume).abs() <= 1e-5 * (1.0 + ma.volume.abs()),
        };
        if !volume_matches
            || (ma.area - mb.area).abs() > 1e-5 * (1.0 + ma.area.abs())
            || ma.centroid.dist(mb.centroid) > tolerance
            || ma.bbox.min.dist(mb.bbox.min) > tolerance
            || ma.bbox.max.dist(mb.bbox.max) > tolerance
            || !material_matches
        {
            return Err(bad(CMD, format!("copy geometry differs for body {}", body.name)));
        }
    }
    scratch.selection = vec![Sel::Occurrence { path: clone_path }];
    let out = json!({"component":root,"occurrence":root_occurrence,"features":new_ids,"components":components.len(),"bodies":expected_count,"frozen_bodies":frozen,"notices":notices});
    Ok((scratch, out))
}

/// A copied body transfer moves its bodies between the copies of its components; one from
/// outside the copy is where the copy starts (it stays in place, in its destination).
fn transfer_into_copy(kind: &mut FeatureKind, components: &BTreeMap<u64, u64>) -> Result<()> {
    if let FeatureKind::BodyTransfer { from, to, transform, .. } = kind {
        *to = *components.get(to).ok_or_else(|| bad(CMD, "transfer destination is outside the copied graph"))?;
        if let Some(mapped) = components.get(from) {
            *from = *mapped;
        } else {
            *from = *to;
            *transform = solidcraft_doc::IDENTITY;
        }
    }
    Ok(())
}

/// What copied features point at outside the copy, in words: the external references Paste New
/// keeps within one design (`notices`), and the bodies they work on that aren't copied.
fn outside_references(notices: &[String], feats: &[Feature], bodies: &BTreeMap<String, String>) -> Vec<String> {
    let mut out: Vec<String> =
        notices.iter().map(|n| n.replace(" retains an external ", " uses a ").replace(" retains external ", " uses ")).collect();
    for f in feats {
        for body in f.kind.input_bodies() {
            if !bodies.contains_key(&body) {
                out.push(format!("{} uses body {body}", f.name));
            }
        }
    }
    out
}

/// The user parameters copied features use (their expressions and their dimensions'), and the
/// ones those use, copied from `source` into `scratch`: named as they were where that name is
/// free, else `w_2`, `w_3`…; `parameters` maps each to its copy.
fn copy_user_parameters(scratch: &mut Session, source: &Document, feats: &[Feature], parameters: &mut BTreeMap<String, String>) -> Result<()> {
    let user: BTreeMap<&str, &solidcraft_doc::Param> = source.params.iter().filter(|p| !p.model).map(|p| (p.name.as_str(), p)).collect();
    let mut pending: Vec<String> = feats.iter().flat_map(|f| f.kind.expressions()).map(str::to_string).collect();
    pending.extend(parameters.keys().filter_map(|old| source.params.iter().find(|p| p.model && p.name == *old)).map(|p| p.expr.clone()));
    let mut used: BTreeSet<&str> = BTreeSet::new();
    let mut work = 0usize;
    while let Some(expr) = pending.pop() {
        for name in solidcraft_doc::expr::references(&expr) {
            work += 1;
            if work > 100_000 {
                return Err(bad(CMD, "parameter references exceed copy work limit"));
            }
            if let Some((key, p)) = user.get_key_value(name.as_str())
                && used.insert(key)
            {
                pending.push(p.expr.clone());
            }
        }
    }
    let copied: Vec<&solidcraft_doc::Param> = source.params.iter().filter(|p| used.contains(p.name.as_str())).collect();
    if scratch.doc.params.len() + copied.len() > solidcraft_doc::MAX_PARAMS {
        return Err(bad(CMD, "too many copied parameters"));
    }
    let mut taken: BTreeSet<String> = scratch.doc.params.iter().map(|p| p.name.clone()).collect();
    for p in &copied {
        let mut name = p.name.clone();
        let mut n = 2;
        while taken.contains(&name) || Document::param_name_problem(&name).is_some() {
            name = format!("{}_{n}", p.name);
            n += 1;
            if n > 10_000 {
                return Err(bad(CMD, format!("no free name for parameter {}", p.name)));
            }
        }
        taken.insert(name.clone());
        parameters.insert(p.name.clone(), name);
    }
    for p in copied {
        let name = parameters.get(&p.name).cloned().unwrap_or_default();
        let expr = solidcraft_doc::clone_expression(&p.expr, parameters);
        scratch.doc_mut().params.push(solidcraft_doc::Param { name, expr, unit: p.unit.clone(), comment: p.comment.clone(), model: false });
    }
    Ok(())
}

#[cfg(test)]
#[path = "component_clone_tests.rs"]
mod tests;
