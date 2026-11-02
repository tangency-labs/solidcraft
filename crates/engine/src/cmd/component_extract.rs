//! Extract: a component's history becomes a design file of its own, and the component a link to
//! that file, as Insert Design would have made it. Replace with Link: a component that builds the
//! same bodies as a linked one becomes more placements of it.
//!
//! The new design is this one cut down to the component: its features, sketches and bodies move
//! to the new design's root, and its subcomponents, their placements and the joints among them
//! come along, all keeping their ids and names, with the parameters they use. It is evaluated on
//! its own and has to build the bodies the component has here. Then the component's history is
//! replaced by an Import feature of those bodies, linked to the file, and this design has to look
//! as it did before. Anything that crosses the component's boundary (a feature outside that uses
//! its sketches, bodies moved in or out, a joint to one of its subcomponents) is refused.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};
use solidcraft_doc::{DesignLink, Document, FeatureKind, ModelBody, ModelState};
use solidcraft_geom::Vec3;

use super::{absolute, component_key, file_sha256, inherit_properties, resolve, session_bodies, stored};
use crate::params::{bad, str_};
use crate::{EngineError, Result, Session};

const CMD: &str = "component.extract";

/// Drop the placements down to and including the extracted component's own from `path`, so it
/// starts at the new design's root.
fn local_path(path: &mut Vec<u64>, own: &BTreeSet<u64>) {
    if let Some(i) = path.iter().rposition(|o| own.contains(o)) {
        path.drain(..=i);
    }
}

/// Whether two solids measure the same, within what a STEP round trip changes in the measure
/// of a curved face (a cylinder's volume by about 1e-5).
fn same_solid(a: &solidcraft_kernel::Body, b: &solidcraft_kernel::Body) -> Result<bool> {
    let (ma, mb) = (solidcraft_kernel::measure(a)?, solidcraft_kernel::measure(b)?);
    let tolerance = 1e-4 * (1.0 + (ma.bbox.max - ma.bbox.min).len());
    Ok((ma.volume - mb.volume).abs() <= 1e-4 * (1.0 + ma.volume.abs())
        && (ma.area - mb.area).abs() <= 1e-4 * (1.0 + ma.area.abs())
        && ma.centroid.dist(mb.centroid) <= tolerance
        && ma.bbox.min.dist(mb.bbox.min) <= tolerance
        && ma.bbox.max.dist(mb.bbox.max) <= tolerance)
}

/// Whether two bodies are the same solid, each moved by its offset.
fn same_shape(a: &ModelBody, a_offset: &solidcraft_doc::Mat, b: &ModelBody, b_offset: &solidcraft_doc::Mat) -> Result<bool> {
    if a.body.same(&b.body) && a_offset == b_offset {
        return Ok(true);
    }
    same_solid(&solidcraft_kernel::transform_matrix(&a.body, *a_offset)?, &solidcraft_kernel::transform_matrix(&b.body, *b_offset)?)
}

/// Whether `a` and `b` are the same solids, whatever they are called.
fn same_solids(a: &[&ModelBody], b: &[&ModelBody]) -> Result<bool> {
    Ok(unmatched(a, b)?.is_none())
}

/// The first body of `a` that `b` has no same solids for, or what `b` has more of, whatever they
/// are called. Bodies that changed are compared piece by piece: a link brings a body made of
/// separate pieces back as one body per piece.
fn unmatched(a: &[&ModelBody], b: &[&ModelBody]) -> Result<Option<String>> {
    let mut left: Vec<&ModelBody> = b.to_vec();
    let mut rest = Vec::new();
    for x in a {
        match left.iter().position(|y| y.name == x.name && y.body.same(&x.body)) {
            Some(i) => drop(left.swap_remove(i)),
            None => rest.push(*x),
        }
    }
    let mut pieces = Vec::new();
    for y in left {
        pieces.extend(y.body.lumps()?);
    }
    for x in rest {
        for lump in x.body.lumps()? {
            let mut found = None;
            for (i, y) in pieces.iter().enumerate() {
                if same_solid(&lump, y)? {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => drop(pieces.swap_remove(i)),
                None => return Ok(Some(format!("body {}", x.name))),
            }
        }
    }
    Ok((!pieces.is_empty()).then(|| format!("{} more pieces of bodies", pieces.len())))
}

/// Remove the parameters nothing in `doc` uses any more, directly or through other parameters.
fn prune_params(doc: &mut Document) {
    use solidcraft_doc::expr::references;
    while !doc.params.is_empty() {
        let mut used: BTreeSet<String> = BTreeSet::new();
        for f in &doc.features {
            used.extend(f.param_names.iter().cloned());
            used.extend(f.kind.expressions().into_iter().flat_map(references));
            if let FeatureKind::Sketch { sketch, .. } = &f.kind {
                used.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
                used.extend(sketch.text_exprs().into_iter().flat_map(|(_, e)| references(e)));
            }
        }
        used.extend(doc.assembly.exprs().into_iter().flat_map(|(_, _, e)| references(e)));
        used.extend(doc.params.iter().flat_map(|p| references(&p.expr)));
        let before = doc.params.len();
        doc.params.retain(|p| used.contains(&p.name));
        if doc.params.len() == before {
            break;
        }
    }
    let names: BTreeSet<String> = doc.params.iter().map(|p| p.name.clone()).chain(doc.features.iter().flat_map(|f| f.param_names.clone())).collect();
    doc.favorites.retain(|n| names.contains(n));
    doc.param_comments.retain(|n, _| names.contains(n));
}

/// A component's look on its bodies that have none of their own, for when the component goes.
fn keep_component_looks(doc: &mut Document, owners: &BTreeMap<String, u64>, components: &BTreeSet<u64>) {
    for (body, owner) in owners {
        if components.contains(owner)
            && !doc.appearances.bodies.contains_key(body)
            && let Some(look) = doc.appearances.components.get(owner).cloned()
        {
            doc.appearances.bodies.insert(body.clone(), look);
        }
    }
}

/// What the bodies `new` call the face or edge that `old` calls `name`, found where it is.
fn renamed(old: &ModelState, new: &[&ModelBody], name: &str, near: Vec3) -> Option<String> {
    use solidcraft_doc::{appearance::face_index_at, naming};
    if let Some((p, _)) = naming::point_on_face(old, name, near) {
        return new.iter().find_map(|b| face_index_at(b, p).and_then(|i| naming::face_names(b).get(i).cloned()));
    }
    let p = old.bodies.iter().find_map(|b| naming::find_edge(b, name, near).ok())?.0;
    new.iter().find(|b| face_index_at(b, p).is_some()).and_then(|b| naming::edge_names_at(b, &[p]).pop())
}

/// Joints on the placements `own` name faces of features that are gone: give them the names of
/// the same faces of the linked bodies (Import feature `import`).
fn rename_joint_faces(s: &mut Session, old: &ModelState, import: u64, own: &BTreeSet<u64>) {
    use solidcraft_doc::joints::{Coupling, JointOrigin, Size, Snap, cam::CamProfile};
    let state = s.model.state();
    let linked: Vec<&ModelBody> = state.bodies.iter().filter(|b| b.feature == import).collect();
    let rename = |name: &mut String, near: Vec3| {
        if let Some(new) = renamed(old, &linked, name, near) {
            *name = new;
        }
    };
    let fix = |o: &mut JointOrigin| {
        if own.contains(&o.occurrence) {
            let near = match &o.snap {
                Snap::FaceCenter { pick, .. } | Snap::CircleCenter { pick, .. } | Snap::Edge { pick, .. } => *pick,
                _ => Vec3::ZERO,
            };
            for name in o.snap.face_names_mut() {
                rename(name, near);
            }
        }
    };
    let mut asm = s.doc.assembly.clone();
    for j in &mut asm.joints {
        fix(&mut j.a);
        fix(&mut j.b);
    }
    for o in &mut asm.origins {
        fix(&mut o.origin);
    }
    for coupling in asm.links.iter_mut().filter_map(|l| l.coupling.as_mut()) {
        for size in coupling.sizes_mut() {
            if let Size::Circle(o) = size {
                fix(o);
            }
        }
        if let Coupling::Cam(cam) = coupling {
            if let Some(tip) = &mut cam.tip {
                fix(tip);
            }
            if let CamProfile::Edges { occurrence, names, .. } = &mut cam.profile
                && own.contains(occurrence)
            {
                for name in names.iter_mut() {
                    rename(name, Vec3::ZERO);
                }
            }
        }
    }
    if asm != s.doc.assembly {
        s.doc_mut().assembly = asm;
        super::super::joints::resolve(s);
    }
}

/// A component that can leave the design, and what goes with it.
struct Scope {
    comp: u64,
    name: String,
    /// The component and its subcomponents.
    subtree: BTreeSet<u64>,
    children: Vec<u64>,
    /// The component's own placements.
    own: BTreeSet<u64>,
    /// The placements of its subcomponents.
    inner: BTreeSet<u64>,
    /// Its features and its subcomponents'.
    inside: BTreeSet<u64>,
    /// Its bodies and its subcomponents'.
    bodies: BTreeSet<String>,
    /// The component of every body of the design.
    owners: BTreeMap<String, u64>,
    /// Placements in this design to placements in its own: the subcomponents' keep their ids, and
    /// the component's own, when it has only one, becomes the new design's root.
    occurrences: BTreeMap<u64, u64>,
}

/// What goes with component `comp`, or why it can't go on its own.
fn scope(s: &Session, comp: u64, cmd: &str) -> Result<Scope> {
    let source = &s.doc;
    let name = source.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).ok_or_else(|| bad(cmd, "pick a component, not the design"))?;
    if source.links.contains_key(&comp) {
        return Err(bad(cmd, format!("{name} is linked to a design file already")));
    }
    if source.parts.contains_key(&comp) {
        return Err(bad(cmd, format!("{name} is a standard part")));
    }
    if source.marker.is_some_and(|m| m < source.features.len()) {
        return Err(bad(cmd, "move the timeline marker to the end first"));
    }
    if !s.pending_moves.is_empty() {
        return Err(bad(cmd, "capture or revert the moved components first"));
    }
    let subtree: BTreeSet<u64> = source.components.iter().filter(|c| source.component_within(c.id, comp)).map(|c| c.id).collect();
    let children: Vec<u64> = subtree.iter().copied().filter(|c| *c != comp).collect();
    let own: BTreeSet<u64> = source.occurrences.iter().filter(|o| o.component == comp).map(|o| o.id).collect();
    let inner: BTreeSet<u64> = source.occurrences.iter().filter(|o| subtree.contains(&o.parent)).map(|o| o.id).collect();
    if let Some(o) = source.occurrences.iter().find(|o| children.contains(&o.component) && !subtree.contains(&o.parent)) {
        return Err(bad(cmd, format!("{} is placed outside {name} too", o.name)));
    }
    let inside: BTreeSet<u64> = source.features.iter().filter(|f| subtree.contains(&f.component)).map(|f| f.id).collect();
    if let Some(f) = source.features.iter().find(|f| inside.contains(&f.id) && s.model.result(f.id).is_some_and(|r| r.error.is_some())) {
        return Err(bad(cmd, format!("{} fails; fix it first", f.name)));
    }
    if let Some(f) = source.features.iter().find(|f| !inside.contains(&f.id) && f.kind.input_sketches().iter().any(|id| inside.contains(id))) {
        return Err(bad(cmd, format!("{} uses a sketch of {name}", f.name)));
    }
    if let Some(f) = source
        .features
        .iter()
        .find(|f| matches!(&f.kind, FeatureKind::BodyTransfer { from, to, .. } if subtree.contains(from) != subtree.contains(to)))
    {
        return Err(bad(cmd, format!("{} moves bodies into or out of {name}", f.name)));
    }
    let mut occurrences: BTreeMap<u64, u64> = inner.iter().map(|o| (*o, *o)).collect();
    if let [single] = own.iter().copied().collect::<Vec<_>>().as_slice() {
        occurrences.insert(*single, 0);
    }
    if let Some(j) = source.assembly.joints.iter().find(|j| {
        (inner.contains(&j.a.occurrence) || inner.contains(&j.b.occurrence))
            && !(occurrences.contains_key(&j.a.occurrence) && occurrences.contains_key(&j.b.occurrence))
    }) {
        return Err(bad(cmd, format!("joint {} connects a part of {name} to the rest of the design", j.name)));
    }
    let state = s.model.state();
    let owners: BTreeMap<String, u64> = state.bodies.iter().map(|b| (b.name.clone(), source.model_body_component(b))).collect();
    let mut bodies = BTreeSet::new();
    for b in &state.bodies {
        let owned = owners.get(&b.name).is_some_and(|c| subtree.contains(c));
        if owned != inside.contains(&b.feature) {
            return Err(bad(cmd, format!("body {} was moved into or out of {name}", b.name)));
        }
        if owned && b.body.is_mesh() {
            return Err(bad(cmd, format!("mesh body {} can't be linked", b.name)));
        }
        if owned {
            bodies.insert(b.name.clone());
        }
    }
    Ok(Scope { comp, name, subtree, children, own, inner, inside, bodies, owners, occurrences })
}

/// The design `sc` makes on its own, called `stem`, evaluated and checked to build the bodies it
/// has here; with notes on what stays behind.
fn part_design(s: &Session, sc: &Scope, stem: &str, cmd: &str) -> Result<(Session, Vec<String>)> {
    let source = &s.doc;
    let (comp, name, bodies) = (sc.comp, &sc.name, &sc.bodies);
    let to_root = |c: u64| if c == comp { 0 } else { c };
    let mut part = (**source).clone();
    part.name = stem.to_string();
    part.marker = None;
    part.features.retain(|f| sc.inside.contains(&f.id));
    for f in &mut part.features {
        f.component = to_root(f.component);
        // Bodies numbered or suffixed after the other bodies here (Body2, `X (2)`) keep those
        // names in the new design, where later features and sketches still look them up by them
        if let Some(result) = s.model.result(f.id) {
            let built: Vec<String> = result.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| b.name.clone()).collect();
            if !built.is_empty() && (f.body_names.is_empty() || super::super::component_clone::named_by(&f.body_names, &built)) {
                f.body_names = built;
            }
        }
        match &mut f.kind {
            FeatureKind::BodyTransfer { from, to, .. } => {
                *from = to_root(*from);
                *to = to_root(*to);
            }
            FeatureKind::Emboss { face: Some(face), .. } => {
                local_path(&mut face.path, &sc.own);
                local_path(&mut face.author_path, &sc.own);
            }
            FeatureKind::Sketch { plane, .. } => {
                if let Some(anchor) = crate::cmd::sketch::face::anchor_mut(plane) {
                    anchor.component = to_root(anchor.component);
                    local_path(&mut anchor.path, &sc.own);
                    local_path(&mut anchor.author_path, &sc.own);
                }
            }
            _ => {}
        }
    }
    part.components.retain(|c| sc.children.contains(&c.id));
    for c in &mut part.components {
        c.parent = to_root(c.parent);
    }
    part.occurrences.retain(|o| sc.inner.contains(&o.id));
    for o in &mut part.occurrences {
        o.parent = to_root(o.parent);
    }
    part.parts.retain(|c, _| sc.children.contains(c));
    part.links.retain(|c, _| sc.children.contains(c));
    part.body_components = part.body_components.iter().filter(|(_, c)| sc.subtree.contains(c)).map(|(b, c)| (b.clone(), to_root(*c))).collect();
    let moved = part.body_components.clone();
    part.body_offsets.retain(|b, _| moved.contains_key(b));
    keep_component_looks(&mut part, &sc.owners, &BTreeSet::from([comp]));
    part.materials.retain(|b, _| bodies.contains(b));
    part.appearances.bodies.retain(|b, _| bodies.contains(b));
    part.appearances.faces.retain(|f| bodies.contains(&f.body));
    part.appearances.components.retain(|c, _| sc.children.contains(c));
    part.canvases.retain(|c| sc.subtree.contains(&c.component));
    for c in &mut part.canvases {
        c.component = to_root(c.component);
    }
    part.browser_groups.retain(|g| sc.subtree.contains(&g.component));
    for g in &mut part.browser_groups {
        g.component = to_root(g.component);
    }
    part.browser_order = source
        .browser_order
        .iter()
        .filter_map(|(key, items)| {
            let (c, folder) = key.split_once('/')?;
            let c = c.parse::<u64>().ok().filter(|c| sc.subtree.contains(c))?;
            Some((format!("{}/{folder}", to_root(c)), items.clone()))
        })
        .collect();
    for g in &mut part.timeline_groups {
        g.features.retain(|f| sc.inside.contains(f));
    }
    part.named_views.clear();
    part.configs = Default::default();
    part.assembly = Default::default();
    let features: BTreeMap<u64, u64> = sc.inside.iter().map(|f| (*f, *f)).collect();
    let body_map: BTreeMap<String, String> = bodies.iter().map(|b| (b.clone(), b.clone())).collect();
    // Joints from the component to the rest of the design stay here, so they are not copied
    let mut scope = Document { assembly: source.assembly.clone(), ..Document::default() };
    let mapped = |o: &u64| sc.occurrences.contains_key(o);
    scope.assembly.joints.retain(|j| mapped(&j.a.occurrence) && mapped(&j.b.occurrence));
    scope.assembly.origins.retain(|o| mapped(&o.origin.occurrence));
    scope.assembly.contact_sets.retain(|c| c.occurrences.iter().all(mapped));
    scope.assembly.exploded.retain(|v| v.moves.iter().all(|(o, _)| mapped(o)));
    let (mut notices, joints) =
        super::super::component_clone_assembly::clone_assembly_with_joint_ids(&mut part, &scope, &sc.occurrences, &features, &body_map)?;
    // Extracted joints receive new ids. References to joints outside the extracted part go away.
    for group in &mut part.timeline_groups {
        group.joints = group.joints.iter().filter_map(|id| joints.get(id).copied()).collect();
    }
    let parents: BTreeSet<u64> = part.timeline_groups.iter().filter_map(|g| g.parent).collect();
    part.timeline_groups.retain(|g| !g.features.is_empty() || !g.joints.is_empty() || parents.contains(&g.id));
    prune_params(&mut part);
    if !source.configs.is_empty() {
        notices.push(format!("the configurations stay in this design; {stem} has none"));
    }
    let part = Session::new(part);
    let failing: Vec<String> = part.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| format!("{} ({e})", r.name))).take(5).collect();
    if !failing.is_empty() {
        return Err(bad(cmd, format!("{name} does not build on its own: {}", failing.join("; "))));
    }
    let (state, built) = (s.model.state(), part.model.state());
    if built.bodies.len() != bodies.len() {
        return Err(bad(cmd, format!("{name} builds {} bodies on its own instead of {}", built.bodies.len(), bodies.len())));
    }
    let identity = solidcraft_doc::IDENTITY;
    for b in bodies {
        let (Some(was), Some(now)) = (state.body(b), built.body(b)) else {
            return Err(bad(cmd, format!("{name} does not build body {b} on its own")));
        };
        if !same_shape(was, source.model_body_offset(was).unwrap_or(&identity), now, part.doc.model_body_offset(now).unwrap_or(&identity))? {
            return Err(bad(cmd, format!("body {b} comes out differently on its own: something outside {name} changes it")));
        }
    }
    Ok((part, notices))
}

/// Take the history of `sc` out of this design: its features, subcomponents, their placements
/// and the joints among them. The component itself and its own placements stay.
fn clear_history(s: &mut Session, sc: &Scope) {
    let d = s.doc_mut();
    for id in &sc.inside {
        // A sketch's own features went with it already.
        let _ = d.delete_feature(*id);
    }
    let comp = sc.comp;
    let (children, inner) = (&sc.children, &sc.inner);
    keep_component_looks(d, &sc.owners, &children.iter().copied().collect());
    d.components.retain(|c| !children.contains(&c.id));
    d.remove_configuration_components(children);
    d.occurrences.retain(|o| !inner.contains(&o.id));
    d.body_components.retain(|_, c| !sc.subtree.contains(c));
    d.body_offsets.retain(|b, _| !sc.bodies.contains(b));
    d.parts.retain(|c, _| !children.contains(c));
    d.links.retain(|c, _| !children.contains(c));
    d.appearances.components.retain(|c, _| !children.contains(c));
    d.canvases.retain(|c| !sc.subtree.contains(&c.component));
    d.browser_groups.retain(|g| !children.contains(&g.component) && (g.component != comp || g.folder == "bodies"));
    d.browser_order.retain(|key, _| {
        key.split_once('/')
            .and_then(|(c, folder)| Some((c.parse::<u64>().ok()?, folder)))
            .is_none_or(|(c, folder)| !children.contains(&c) && (c != comp || folder == "bodies"))
    });
    let a = &mut d.assembly;
    a.joints.retain(|j| !inner.contains(&j.a.occurrence) && !inner.contains(&j.b.occurrence));
    let joints: BTreeSet<u64> = a.joints.iter().map(|j| j.id).collect();
    a.links.retain(|l| joints.contains(&l.a) && joints.contains(&l.b));
    a.studies.retain(|study| study.keys.iter().all(|k| joints.contains(&k.joint)));
    for set in &mut a.contact_sets {
        set.occurrences.retain(|o| !inner.contains(o));
    }
    for view in &mut a.exploded {
        view.moves.retain(|(o, _)| !inner.contains(o));
    }
    s.visibility.hidden_sketches.retain(|f| !sc.inside.contains(f));
    s.visibility.shown_sketches.retain(|f| !sc.inside.contains(f));
    s.visibility.hidden_occurrences.retain(|p| !p.iter().any(|o| inner.contains(o)));
    if children.contains(&s.active_component) {
        s.active_component = 0;
        s.active_occurrence_path = None;
    }
    s.selection.clear();
}

/// The design as it was before a swap, to check the swap against.
struct Before {
    world: std::sync::Arc<ModelState>,
    placements: BTreeMap<u64, solidcraft_doc::Mat>,
    errors: BTreeMap<u64, bool>,
    joint_errors: BTreeSet<String>,
}

impl Before {
    fn take(s: &Session) -> Before {
        let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        Before {
            world: s.world_state(),
            placements: s.doc.occurrences.iter().map(|o| (o.id, o.transform)).collect(),
            errors: s.model.results.iter().map(|r| (r.id, r.error.is_some())).collect(),
            joint_errors: sol.errors.iter().chain(&sol.conflicts).map(|(_, n, _)| n.clone()).collect(),
        }
    }

    /// Refuse a swap that made a feature fail or changed a body; warn about joints it broke and
    /// placements the joints changed.
    fn check(&self, s: &Session, cmd: &str, name: &str) -> Result<Vec<String>> {
        let broken: Vec<&str> =
            s.model.results.iter().filter(|r| r.error.is_some() && self.errors.get(&r.id) == Some(&false)).map(|r| r.name.as_str()).take(5).collect();
        if !broken.is_empty() {
            return Err(bad(cmd, format!("{} depend on the history of {name}", broken.join(", "))));
        }
        // With nothing grounded, the solver holds the first joint's part still, so a joint that
        // goes with the history can make it place a chain it left alone before. Bodies are
        // compared where the placements were.
        let mut placed = (*s.doc).clone();
        let mut moved = Vec::new();
        for o in &mut placed.occurrences {
            if let Some(t) = self.placements.get(&o.id).filter(|t| **t != o.transform) {
                moved.push(o.name.clone());
                o.transform = *t;
            }
        }
        let world = if moved.is_empty() { s.world_state() } else { std::sync::Arc::new(solidcraft_doc::world_state(&placed, &s.model.state())) };
        let (was, now): (Vec<&ModelBody>, Vec<&ModelBody>) = (self.world.bodies.iter().collect(), world.bodies.iter().collect());
        if let Some(what) = unmatched(&was, &now)? {
            return Err(bad(cmd, format!("{what} would look different without the history of {name}")));
        }
        let mut warnings = Vec::new();
        if !moved.is_empty() {
            warnings.push(format!("the joints now place {}: nothing in their chain is grounded", moved.join(", ")));
        }
        let sol = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        warnings.extend(
            sol.errors
                .iter()
                .chain(&sol.conflicts)
                .filter(|(_, n, _)| !self.joint_errors.contains(n))
                .map(|(_, joint, why)| format!("joint {joint}: {why}")),
        );
        Ok(warnings)
    }
}

/// The components other than `linked`, a linked component, whose design on its own builds the
/// same bodies as its link.
fn copies_of(s: &Session, linked: u64) -> Result<Vec<u64>> {
    let Some(link) = s.doc.links.get(&linked) else { return Ok(Vec::new()) };
    let state = s.model.state();
    let reference: Vec<&ModelBody> = state.bodies.iter().filter(|b| b.feature == link.feature).collect();
    let volume = |bodies: &[&ModelBody]| bodies.iter().filter_map(|b| solidcraft_kernel::measure(&b.body).ok()).map(|m| m.volume).sum::<f64>();
    let expected = volume(&reference);
    let mut out = Vec::new();
    for c in &s.doc.components {
        if c.id == linked
            || s.doc.links.contains_key(&c.id)
            || s.doc.parts.contains_key(&c.id)
            || s.doc.component_within(linked, c.id)
            || s.doc.component_within(c.id, linked)
        {
            continue;
        }
        // Cheap first: as many bodies with as much material, within what the link's STEP round
        // trip changes (see same_solid)
        let owned: Vec<&ModelBody> = state.bodies.iter().filter(|b| s.doc.component_within(s.doc.model_body_component(b), c.id)).collect();
        if reference.is_empty() || owned.len() != reference.len() || (volume(&owned) - expected).abs() > 1e-4 * (1.0 + expected.abs()) {
            continue;
        }
        let Ok(sc) = scope(s, c.id, CMD) else { continue };
        let Ok((part, _)) = part_design(s, &sc, &c.name, CMD) else { continue };
        let world = part.world_state();
        let built: Vec<&ModelBody> = world.bodies.iter().collect();
        if same_solids(&reference, &built)? {
            out.push(c.id);
        }
    }
    Ok(out)
}

fn named(s: &Session, ids: &[u64]) -> Vec<Value> {
    ids.iter().map(|id| json!({"component": id, "name": s.doc.components.iter().find(|c| c.id == *id).map(|c| c.name.clone())})).collect()
}

impl Session {
    /// Whether component `id` can leave the design for a file of its own or a link: what
    /// `component.extract` and `component.replace_with_link` check before they build anything.
    pub fn can_extract(&self, id: u64) -> bool {
        scope(self, id, CMD).is_ok()
    }
}

pub(super) fn extract(s: &mut Session, p: &Value) -> Result<Value> {
    let comp = component_key(s, p, "component", CMD)?;
    let sc = scope(s, comp, CMD)?;
    let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(CMD, "`path` is required"))?;
    let file = absolute(Path::new(given));
    if !solidcraft_io::is_design_container_path(&file) {
        return Err(bad(CMD, "the new design must be a .sc file"));
    }
    if s.path.as_deref().is_some_and(|own| absolute(Path::new(own)) == file) {
        return Err(bad(CMD, "choose another file than this design's own"));
    }
    let stem = file.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| sc.name.clone());
    let (mut part, notices) = part_design(s, &sc, &stem, CMD)?;

    // This design: the component's history becomes an Import feature of the new design's bodies
    let shown = file.display().to_string();
    let (_, sf, faces, _, properties) = session_bodies(&part, &shown)?;
    // The link has a body per piece: a body in pieces keeps its name on the first
    let mut solids: Vec<String> = Vec::new();
    for b in part.world_state().bodies.iter().filter(|b| !b.body.is_mesh()) {
        let pieces = b.body.lumps()?.len().max(1);
        solids.extend((1..=pieces).map(|n| if n == 1 { b.name.clone() } else { format!("{} ({n})", b.name) }));
    }
    let state = s.model.state();
    let before = Before::take(s);
    let last = s.doc.features.iter().rposition(|f| sc.inside.contains(&f.id)).unwrap_or(s.doc.features.len());
    let at = s.doc.features.iter().take(last).filter(|f| !sc.inside.contains(&f.id)).count();
    clear_history(s, &sc);
    let d = s.doc_mut();
    let id = d.add_feature(sf.kind, Some(&stem))?;
    if let Some(i) = d.feature_index(id)
        && i != at
    {
        let f = d.features.remove(i);
        d.features.insert(at.min(d.features.len()), f);
    }
    if let Some(f) = d.feature_mut(id) {
        f.component = comp;
        f.body_names = if solids.len() == sf.body_names.len() { solids } else { sf.body_names };
        f.link_faces = faces;
    }
    let path = stored(s.path.as_deref(), &file);
    s.doc_mut().links.insert(comp, DesignLink { path: file.to_string_lossy().into_owned(), feature: id, ..Default::default() });
    s.refresh();
    rename_joint_faces(s, &state, id, &sc.own);
    if let Some(e) = s.model.result(id).and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    inherit_properties(s, comp, &properties)?;
    s.refresh();
    let mut warnings = sf.warnings;
    warnings.extend(before.check(s, CMD, &sc.name)?);
    let copies = copies_of(s, comp)?;

    part.path = s.path.clone();
    part.execute("file.save", &json!({"path": shown}))?;
    // The link keeps what the new file holds, as Insert Design's does (#9739).
    if let Some(read) = file_sha256(&file, s.path.as_deref())
        && let Some(link) = s.doc_mut().links.get_mut(&comp)
    {
        link.file_sha256 = Some(read);
    }
    Ok(json!({"component": comp, "feature": id, "path": path, "file": shown, "bodies": sc.bodies, "notices": notices, "warnings": warnings,
        "copies": named(s, &copies)}))
}

pub(super) fn copies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.copies";
    let linked = component_key(s, p, "component", cmd)?;
    if !s.doc.links.contains_key(&linked) {
        return Err(bad(cmd, "that component is not linked to a design"));
    }
    let found = copies_of(s, linked)?;
    Ok(json!({"copies": named(s, &found)}))
}

pub(super) fn replace_with_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.replace_with_link";
    let mut targets = Vec::new();
    if p.get("component").is_some() {
        targets.push(component_key(s, p, "component", cmd)?);
    }
    for v in p.get("components").and_then(Value::as_array).into_iter().flatten().take(1000) {
        targets.push(component_key(s, &json!({"component": v}), "component", cmd)?);
    }
    targets.dedup();
    if targets.is_empty() {
        return Err(bad(cmd, "`component` or `components` is required"));
    }
    // The linked component to place instead: given, one already linked to the file, or a new one
    let mut inserted = None;
    let linked = if p.get("with").is_some() {
        component_key(s, p, "with", cmd)?
    } else {
        let given = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`with` or `path` is required"))?;
        let file = absolute(Path::new(given));
        let existing = s.doc.links.iter().find(|(_, l)| absolute(&resolve(s.path.as_deref(), &l.path)) == file).map(|(c, _)| *c);
        match existing {
            Some(c) => c,
            None => {
                let parent = targets.first().and_then(|t| s.doc.components.iter().find(|c| c.id == *t)).map(|c| c.parent).unwrap_or(0);
                let made = super::insert_design(s, &json!({"path": given, "parent": parent}))?;
                inserted = made["occurrence"].as_u64();
                made["component"].as_u64().ok_or_else(|| bad(cmd, "the linked component was not created"))?
            }
        }
    };
    let link = s.doc.links.get(&linked).cloned().ok_or_else(|| bad(cmd, "the replacement is not linked to a design"))?;
    let lname = s.doc.components.iter().find(|c| c.id == linked).map(|c| c.name.clone()).unwrap_or_default();
    let mut warnings = Vec::new();
    let mut replaced = Vec::new();
    for t in targets {
        if t == linked || s.doc.component_within(linked, t) || s.doc.component_within(t, linked) {
            return Err(bad(cmd, format!("{lname} can't replace itself")));
        }
        let sc = scope(s, t, cmd)?;
        let (part, _) = part_design(s, &sc, &sc.name, cmd)?;
        let state = s.model.state();
        let reference: Vec<&ModelBody> = state.bodies.iter().filter(|b| b.feature == link.feature).collect();
        let world = part.world_state();
        let built: Vec<&ModelBody> = world.bodies.iter().collect();
        if !same_solids(&reference, &built)? {
            return Err(bad(cmd, format!("{} is not the same as {lname}", sc.name)));
        }
        let before = Before::take(s);
        clear_history(s, &sc);
        let d = s.doc_mut();
        // Named as the linked component's next placements, as add_occurrence names them
        let mut k = d.occurrences.iter().filter(|o| o.component == linked).count();
        for o in d.occurrences.iter_mut().filter(|o| o.component == t) {
            k += 1;
            o.component = linked;
            o.name = format!("{lname}:{k}");
        }
        d.components.retain(|c| c.id != t);
        d.remove_configuration_components(&[t]);
        d.appearances.components.remove(&t);
        d.materials.retain(|b, _| !sc.bodies.contains(b));
        d.appearances.bodies.retain(|b, _| !sc.bodies.contains(b));
        d.appearances.faces.retain(|f| !sc.bodies.contains(&f.body));
        d.browser_groups.retain(|g| g.component != t);
        d.browser_order.retain(|key, _| key.split_once('/').is_none_or(|(c, _)| c != t.to_string()));
        if s.active_component == t {
            s.active_component = 0;
            s.active_occurrence_path = None;
        }
        s.refresh();
        rename_joint_faces(s, &state, link.feature, &sc.own);
        warnings.extend(before.check(s, cmd, &sc.name)?);
        replaced.push(sc.name);
    }
    if let Some(o) = inserted {
        s.doc_mut().occurrences.retain(|x| x.id != o);
        s.refresh();
    }
    Ok(json!({"component": linked, "name": lname, "replaced": replaced, "warnings": warnings}))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solidcraft-extract-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn volumes(s: &Session) -> BTreeMap<String, i64> {
        s.world_state()
            .bodies
            .iter()
            .map(|b| (b.name.clone(), (solidcraft_kernel::measure(&b.body).unwrap().volume * 1000.0).round() as i64))
            .collect()
    }

    fn centroids(s: &Session) -> BTreeMap<String, [i64; 3]> {
        s.world_state()
            .bodies
            .iter()
            .map(|b| {
                let c = solidcraft_kernel::measure(&b.body).unwrap().centroid;
                (b.name.clone(), [c.x, c.y, c.z].map(|v| (v * 1000.0).round() as i64))
            })
            .collect()
    }

    /// Where the bodies are, whatever they are called: volumes to the mm³ and centres to 0.01 mm,
    /// coarser than what a link's STEP round trip changes in a round face.
    fn shapes(s: &Session) -> Vec<[i64; 4]> {
        let mut out: Vec<[i64; 4]> = s
            .world_state()
            .bodies
            .iter()
            .map(|b| {
                let m = solidcraft_kernel::measure(&b.body).unwrap();
                let c = [m.centroid.x, m.centroid.y, m.centroid.z].map(|v| (v * 100.0).round() as i64);
                [m.volume.round() as i64, c[0], c[1], c[2]]
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// A design with a moved Plate component (a box sized by a parameter, with a Pin subcomponent
    /// held to it by a joint) next to an Other component.
    fn assembly() -> (Session, u64, u64) {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "plate_h", "expression": "12 mm"}));
        run(&mut s, "parameters.add", json!({"name": "unused", "expression": "3 mm"}));
        let plate = run(&mut s, "component.create", json!({"name": "Plate", "grounded": false}));
        let (plate, plate_occ) = (plate["component"].as_u64().unwrap(), plate["occurrence"].as_u64().unwrap());
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": "plate_h", "body_name": "PlateBody"}));
        let pin = run(&mut s, "component.create", json!({"name": "Pin", "parent": plate}));
        let pin_occ = pin["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 20, "body_name": "PinBody"}));
        run(&mut s, "occurrence.move", json!({"occurrence": pin_occ, "translate": [5, 5, 0]}));
        run(
            &mut s,
            "joint.create",
            json!({"name": "PinHold", "type": "rigid", "a": {"occurrence": plate_occ, "point": [0, 0, 0]}, "b": {"occurrence": pin_occ, "point": [5, 5, 0]}, "quiet": true}),
        );
        run(&mut s, "component.activate", json!({"component": 0}));
        let other = run(&mut s, "component.create", json!({"name": "Other"}))["component"].as_u64().unwrap();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "OtherBody"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "occurrence.move", json!({"occurrence": plate_occ, "translate": [100, 0, 0]}));
        (s, plate, other)
    }

    #[test]
    fn extract_moves_the_history_to_its_own_design_and_links_it() {
        let dir = folder("basic");
        let (mut s, plate, other) = assembly();
        run(&mut s, "timeline.group", json!({"name":"Internal joints", "joints":["PinHold"]}));
        run(&mut s, "file.save", json!({"path": dir.join("asm.solidcraft")}));
        let (vol, cen) = (volumes(&s), centroids(&s));
        let other_features: Vec<u64> = s.doc.features.iter().filter(|f| f.component == other).map(|f| f.id).collect();
        let file = dir.join("parts").join("plate.solidcraft");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let out = run(&mut s, "component.extract", json!({"component": "Plate", "path": file}));
        assert_eq!(out["path"], "parts/plate.solidcraft", "{out}");
        assert_eq!(out["copies"], json!([]), "{out}");

        // This design: Plate holds one linked Import feature, its subcomponent and joint are gone
        let plate_features: Vec<&solidcraft_doc::Feature> = s.doc.features.iter().filter(|f| f.component == plate).collect();
        assert_eq!(plate_features.len(), 1);
        assert!(matches!(plate_features[0].kind, FeatureKind::Import { .. }));
        assert_eq!(s.doc.links.get(&plate).map(|l| l.feature), Some(plate_features[0].id));
        assert!(s.doc.components.iter().all(|c| c.name != "Pin"));
        assert!(s.doc.assembly.joints.is_empty());
        assert_eq!(s.doc.features.iter().filter(|f| f.component == other).map(|f| f.id).collect::<Vec<_>>(), other_features);
        assert_eq!(volumes(&s), vol);
        assert_eq!(centroids(&s), cen);
        assert!(s.model.results.iter().all(|r| r.error.is_none()));
        let links = run(&mut s, "component.links", json!({}));
        assert_eq!(links["links"][0]["found"], true, "{links}");

        // The new design: Plate's history at its root, with Pin, the joint and the parameter it uses
        let mut part = Session::default();
        run(&mut part, "doc.open", json!({"path": file}));
        assert_eq!(part.doc.name, "plate");
        assert_eq!(part.doc.components.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Pin"]);
        assert_eq!(part.doc.occurrences.len(), 1);
        assert_eq!(part.doc.assembly.joints.iter().map(|j| j.name.as_str()).collect::<Vec<_>>(), ["PinHold"]);
        assert_eq!(part.doc.timeline_groups[0].joints, [part.doc.assembly.joints[0].id], "joint folders follow the extracted ids");
        assert!(part.doc.param("plate_h").is_some() && part.doc.param("unused").is_none());
        assert!(part.model.results.iter().all(|r| r.error.is_none()));
        let part_vol = volumes(&part);
        assert_eq!(part_vol.get("PlateBody"), vol.get("PlateBody"));
        assert_eq!(part_vol.get("PinBody"), vol.get("PinBody"));
        assert!(!part_vol.contains_key("OtherBody"));
        // At the new design's own origin, not where Plate was placed
        let plate_x = |c: &BTreeMap<String, [i64; 3]>| c["PlateBody"][0];
        assert_eq!(plate_x(&cen) - plate_x(&centroids(&part)), 100_000);

        // Driving the new design and updating the link brings the change in
        run(&mut part, "parameters.change", json!({"name": "plate_h", "expression": "24 mm"}));
        run(&mut part, "file.save", json!({}));
        assert_eq!(run(&mut s, "component.update_links", json!({}))["updated"], json!(["Plate"]));
        assert_eq!(volumes(&s)["PlateBody"], 2 * vol["PlateBody"]);

        // Undo restores the history; the file stays
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.links.is_empty());
        assert!(s.doc.components.iter().any(|c| c.name == "Pin"));
        assert_eq!(volumes(&s), vol);
        assert!(file.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_refuses_what_it_cannot_move_and_changes_nothing() {
        let dir = folder("refuse");
        let (mut s, plate, other) = assembly();
        let file = dir.join("plate.solidcraft");
        let refuse = |s: &mut Session, p: Value, why: &str| {
            let before = serde_json::to_value(&*s.doc).unwrap();
            let err = s.execute("component.extract", &p).unwrap_err().to_string();
            assert!(err.contains(why), "{p}: {err}");
            assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
            assert!(!file.exists());
        };
        refuse(&mut s, json!({"component": 0, "path": file}), "not the design");
        refuse(&mut s, json!({"component": "Plate"}), "`path` is required");
        refuse(&mut s, json!({"component": "Plate", "path": dir.join("plate.step")}), ".sc file");

        // A joint from Plate's subcomponent to the rest of the design
        let pin_occ = s.doc.occurrences.iter().find(|o| o.name.starts_with("Pin")).map(|o| o.id).unwrap();
        let other_occ = s.doc.occurrence_of(other).map(|o| o.id).unwrap();
        run(
            &mut s,
            "joint.create",
            json!({"name": "PinToOther", "type": "rigid", "a": {"occurrence": pin_occ, "point": [0, 0, 0]}, "b": {"occurrence": other_occ, "point": [0, 0, 0]}, "quiet": true}),
        );
        refuse(&mut s, json!({"component": "Plate", "path": file}), "PinToOther");
        run(&mut s, "edit.undo", json!({}));

        // A feature outside that uses one of Plate's sketches
        run(&mut s, "component.activate", json!({"component": plate}));
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        let sketch = s.doc.features.last().map(|f| f.id).unwrap();
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "solid.extrude", json!({"sketch": sketch, "distance": 3, "operation": "new"}));
        refuse(&mut s, json!({"component": "Plate", "path": file}), "uses a sketch of Plate");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn joints_on_the_extracted_component_keep_their_faces() {
        let dir = folder("joints");
        let (mut s, plate, other) = assembly();
        let plate_occ = s.doc.occurrence_of(plate).map(|o| o.id).unwrap();
        let other_occ = s.doc.occurrence_of(other).map(|o| o.id).unwrap();
        let top = {
            let w = s.world_state();
            let m = solidcraft_kernel::measure(&w.body("PlateBody").unwrap().body).unwrap();
            [(m.bbox.min.x + m.bbox.max.x) / 2.0, (m.bbox.min.y + m.bbox.max.y) / 2.0, m.bbox.max.z]
        };
        let bottom = {
            let w = s.world_state();
            let m = solidcraft_kernel::measure(&w.body("OtherBody").unwrap().body).unwrap();
            [(m.bbox.min.x + m.bbox.max.x) / 2.0, (m.bbox.min.y + m.bbox.max.y) / 2.0, m.bbox.min.z]
        };
        run(
            &mut s,
            "joint.create",
            json!({"name": "Stack", "type": "rigid", "a": {"occurrence": plate_occ, "face": top}, "b": {"occurrence": other_occ, "face": bottom}}),
        );
        let cen = centroids(&s);
        let out = run(&mut s, "component.extract", json!({"component": "Plate", "path": dir.join("plate.solidcraft")}));
        assert_eq!(out["warnings"], json!([]), "{out}");
        assert_eq!(out["notices"], json!([]), "{out}");
        let joint = run(&mut s, "joint.inspect", json!({"joint": "Stack"}));
        assert_eq!(joint["joints"][0]["a"]["resolves"], true, "{joint}");
        assert_eq!(joint["joints"][0]["errors"], json!([]), "{joint}");
        run(&mut s, "joint.solve", json!({}));
        assert_eq!(centroids(&s), cen);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn numbered_body_names_and_sketches_on_their_faces_survive_extraction() {
        let dir = folder("numbered");
        let mut s = Session::default();
        // The box's body is Body1, so the cylinder's is numbered Body2 here
        run(&mut s, "component.create", json!({"name": "box"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "component.activate", json!({"component": 0}));
        let cylinder = run(&mut s, "component.create", json!({"name": "cylinder"}));
        let occurrence = cylinder["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [50, 0, 0]}));
        assert!(s.model.state().body("Body2").is_some());
        run(&mut s, "sketch.create", json!({"plane": {"face": [50, 0, 10], "path": [occurrence]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 2}));
        run(&mut s, "sketch.finish", json!({}));
        // The circle is off the face, so Body2 is the cylinder and a separate boss
        run(&mut s, "solid.extrude", json!({"distance": 3, "operation": "join"}));
        run(&mut s, "component.activate", json!({"component": 0}));
        let vol = volumes(&s);
        let out = run(&mut s, "component.extract", json!({"component": "cylinder", "path": dir.join("cylinder.solidcraft")}));
        assert_eq!(out["bodies"], json!(["Body2"]), "{out}");
        let linked = volumes(&s);
        assert_eq!(linked.keys().collect::<Vec<_>>(), ["Body1", "Body2", "Body2 (2)"]);
        assert!((linked["Body2"] + linked["Body2 (2)"] - vol["Body2"]).abs() < 100, "{linked:?} {vol:?}");
        let mut part = Session::default();
        run(&mut part, "doc.open", json!({"path": dir.join("cylinder.solidcraft")}));
        assert!(part.model.results.iter().all(|r| r.error.is_none()));
        assert_eq!(volumes(&part).get("Body2"), vol.get("Body2"));
        // A new body in the extracted design is numbered on from there
        run(&mut part, "solid.box", json!({"length": 1, "width": 1, "height": 1}));
        assert!(part.model.state().bodies.iter().all(|b| b.name != "Body2" || b.feature != part.doc.features.last().unwrap().id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_extracted_component_with_a_body_changed_from_outside_is_refused() {
        let dir = folder("outside");
        let (mut s, _, _) = assembly();
        let file = dir.join("plate.solidcraft");
        // A cut owned by the design root that takes a piece out of Plate's body
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 50, "base": [110, 10, -5], "operation": "cut", "targets": ["PlateBody"]}));
        let before = serde_json::to_value(&*s.doc).unwrap();
        let err = s.execute("component.extract", &json!({"component": "Plate", "path": file})).unwrap_err().to_string();
        assert!(err.contains("PlateBody") || err.contains("depend"), "{err}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_copies_are_found_and_become_placements_of_the_link() {
        let dir = folder("copies");
        let (mut s, plate, other) = assembly();
        let plate_occ = s.doc.occurrence_of(plate).map(|o| o.id).unwrap();
        // A round hole: its volume moves a little through the link's STEP round trip
        run(&mut s, "component.activate", json!({"component": plate}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 5, "height": 40, "base": [120, -5, 6], "axis": "Y", "operation": "cut", "targets": ["PlateBody"]}),
        );
        run(&mut s, "component.activate", json!({"component": 0}));
        let copy = run(&mut s, "component.paste_new", json!({"component": plate, "occurrence": plate_occ, "translate": [0, 80, 0]}));
        let (copy, copy_occ) = (copy["component"].as_u64().unwrap(), copy["occurrence"].as_u64().unwrap());
        // A face joint on the copy, which has to follow it onto the link
        let other_occ = s.doc.occurrence_of(other).map(|o| o.id).unwrap();
        let top = {
            let st = s.model.state();
            let body = st.bodies.iter().find(|b| s.doc.model_body_component(b) == copy && b.name.starts_with("PlateBody")).unwrap();
            let m = solidcraft_kernel::measure(&body.body).unwrap();
            let w = solidcraft_doc::joints::occurrence_world(&s.doc, copy_occ);
            solidcraft_doc::apply_point(&w, Vec3::new((m.bbox.min.x + m.bbox.max.x) / 2.0, (m.bbox.min.y + m.bbox.max.y) / 2.0, m.bbox.max.z))
        };
        run(
            &mut s,
            "joint.create",
            json!({"name": "OnCopy", "type": "rigid", "a": {"occurrence": copy_occ, "face": [top.x, top.y, top.z]}, "b": {"occurrence": other_occ, "point": [0, 0, 0]}, "quiet": true}),
        );
        // A component that only looks alike is no copy
        run(&mut s, "component.create", json!({"name": "Taller"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 13}));
        run(&mut s, "component.activate", json!({"component": 0}));
        // Nothing is grounded, so OnCopy floats until PinHold, the first joint, goes with Plate's
        // history: then the copy holds still and Other is placed on it
        let out = run(&mut s, "component.extract", json!({"component": "Plate", "path": dir.join("plate.solidcraft")}));
        assert_eq!(out["copies"], json!([{"component": copy, "name": s.doc.components.iter().find(|c| c.id == copy).unwrap().name}]), "{out}");
        assert!(out["warnings"].to_string().contains("the joints now place Other"), "{out}");
        assert_eq!(run(&mut s, "component.copies", json!({"component": plate}))["copies"], out["copies"]);
        let shapes_before = shapes(&s);

        let replaced = run(&mut s, "component.replace_with_link", json!({"components": [copy], "with": plate}));
        assert_eq!(replaced["warnings"], json!([]), "{replaced}");
        assert!(s.doc.components.iter().all(|c| c.id != copy));
        assert_eq!(s.doc.occurrences.iter().filter(|o| o.component == plate).count(), 2);
        let placed = s.doc.occurrences.iter().find(|o| o.id == copy_occ).unwrap();
        assert_eq!((placed.component, placed.name.as_str()), (plate, "Plate:2"));
        assert_eq!(shapes(&s), shapes_before);
        let joint = run(&mut s, "joint.inspect", json!({"joint": "OnCopy"}));
        assert_eq!(joint["joints"][0]["a"]["resolves"], true, "{joint}");
        assert_eq!(run(&mut s, "component.copies", json!({"component": plate}))["copies"], json!([]));

        // Not the same: refused, nothing changes
        let before = serde_json::to_value(&*s.doc).unwrap();
        let err = s.execute("component.replace_with_link", &json!({"component": "Taller", "with": plate})).unwrap_err().to_string();
        assert!(err.contains("not the same"), "{err}");
        assert_eq!(serde_json::to_value(&*s.doc).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_with_link_from_a_file_inserts_the_link_once() {
        let dir = folder("from-file");
        let (mut s, plate, _) = assembly();
        let plate_occ = s.doc.occurrence_of(plate).map(|o| o.id).unwrap();
        let file = dir.join("plate.solidcraft");
        // The design file made elsewhere: the same plate, Pin and all
        {
            let mut other = s.scratch();
            run(&mut other, "component.extract", json!({"component": "Plate", "path": file}));
        }
        let shapes_before = shapes(&s);
        let components = s.doc.components.len();
        let out = run(&mut s, "component.replace_with_link", json!({"component": plate, "path": file}));
        let linked = out["component"].as_u64().unwrap();
        assert_ne!(linked, plate);
        assert!(s.doc.links.contains_key(&linked));
        // Plate and Pin went, the linked component came: placed only where Plate was
        assert_eq!(s.doc.components.len(), components - 1);
        assert_eq!(s.doc.occurrences.iter().filter(|o| o.component == linked).map(|o| o.id).collect::<Vec<_>>(), [plate_occ]);
        assert_eq!(shapes(&s), shapes_before);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
