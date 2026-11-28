//! Retain a proven evaluation across changes to assembly placement. Unknown feature kinds
//! and any cross-component or external-placement reference use the ordinary evaluator; adding
//! an eligible kind requires checking all of its inputs.

use super::{AxisRef, Document, Feature, FeatureId, FeatureKind, Model, ModelState, Operation, PlaneRef, ScaleAbout};
use std::collections::HashMap;
use std::sync::{Arc, Weak};

#[derive(Clone, Debug)]
pub(super) struct Inputs {
    document: Arc<Document>,
    state: Weak<ModelState>,
}

impl Model {
    /// Evaluate a shared document, retaining a proof for subsequent placement-only changes.
    /// Returns whether the timeline was evaluated. The normal `evaluate` remains available
    /// for callers without a shared document and invalidates this proof.
    ///
    /// Only explicitly supported component-local geometry qualifies. Cross-component references,
    /// geometry edits, unknown feature kinds, stale/primed results and `no_cache` continue
    /// through the ordinary evaluator.
    pub fn evaluate_shared(&mut self, doc: &Arc<Document>) -> bool {
        let reusable = !self.no_cache
            && self.primed.is_empty()
            && !doc.has_capture_position_timeline()
            && !self.has_pending_import_checks()
            && self.placement_inputs.as_ref().is_some_and(|inputs| {
                inputs.state.upgrade().is_some_and(|state| Arc::ptr_eq(&state, &self.state()))
                    && !self.is_stale()
                    && (Arc::ptr_eq(doc, &inputs.document) || (same_geometry_inputs(doc, &inputs.document) && invertible_placements(doc)))
            });
        // Or the checks of the last evaluation still run ([`Model::set_background_checks`]) and
        // `doc` is what it evaluated: evaluating again would keep every result, after waiting
        // for the checks.
        if reusable || (!self.no_cache && self.checking_features() && self.is_current(doc)) {
            self.last_recomputed = 0;
            self.last_restored = 0;
            return false;
        }
        self.evaluate(doc);
        self.mark_fingerprinted(doc);
        if !self.no_cache && self.primed.is_empty() && self.placement_local && invertible_placements(doc) {
            self.placement_inputs = Some(Inputs { document: doc.clone(), state: Arc::downgrade(&self.state()) });
        }
        true
    }

    /// [`Model::refingerprint`] for a shared document: a proof that held for the results before
    /// is kept for `doc`, since nothing was built differently (pinned body names, say). The
    /// first `keep` results keep their fingerprints: [`Model::pinned_prefix`] of the document
    /// before the pins, or 0.
    pub fn refingerprint_shared(&mut self, doc: &Arc<Document>, keep: usize) {
        let proven = !self.no_cache
            && self.primed.is_empty()
            && self.placement_inputs.as_ref().is_some_and(|inputs| inputs.state.upgrade().is_some_and(|state| Arc::ptr_eq(&state, &self.state())))
            && !self.is_stale();
        self.refingerprint_from(doc, keep);
        self.mark_fingerprinted(doc);
        if proven && self.placement_local && invertible_placements(doc) {
            self.placement_inputs = Some(Inputs { document: doc.clone(), state: Arc::downgrade(&self.state()) });
        }
    }
}

fn invertible_placements(doc: &Document) -> bool {
    doc.occurrences.iter().all(|o| crate::mat_inverse(&o.transform).is_some())
}

pub(super) fn local_plane(plane: &PlaneRef, depth: usize) -> bool {
    if depth > crate::MAX_PLACEMENT_DEPTH {
        return false;
    }
    match plane {
        PlaneRef::Origin { name: _ } | PlaneRef::Custom { plane: _ } | PlaneRef::Placed { origin: _, x_dir: _, y_dir: _ } => true,
        PlaneRef::Offset { base, distance: _ }
        | PlaneRef::Flipped { base, about_y: _ }
        | PlaneRef::AtAngle { base, axis_origin: _, axis_dir: _, angle: _, line: None, pivot: None, .. } => local_plane(base, depth + 1),
        _ => false,
    }
}

/// Whether no feature after the mirror `f` can cut, join or otherwise build on bodies: only
/// features that read none, and further mirrors, which copy them. A boolean depends on how a
/// copy's revolved faces are turned, so a copy it may reach is made with
/// [`solidcraft_kernel::transform_matrix`]; one that is only shown needs no search for that. It
/// depends on the design alone, so a design evaluates the same however it was reached.
pub(super) fn copies_only_shown(doc: &Document, f: &Feature) -> bool {
    let mut later = doc.features.iter().skip_while(|g| g.id != f.id).skip(1);
    let mut components = HashMap::with_capacity(doc.features.len());
    for g in &doc.features {
        components.entry(g.id).or_insert(g.component);
    }
    let copies = |g: &Feature| matches!(&g.kind, FeatureKind::Mirror { features, combine: false, .. } if features.is_empty());
    later.all(|g| copies(g) || local_geometry(g, &components))
}

/// Whether `f` creates geometry purely from local inputs without reading any existing bodies
/// (used by [`copies_only_shown`] and [`super::reads_none_of`]).
pub(super) fn local_geometry(f: &Feature, components: &HashMap<u64, u64>) -> bool {
    // Persistent picked-face/edge attachments can move an otherwise literal input.
    if !f.face_names.is_empty() || !f.edge_names.is_empty() || !f.edge_refs.is_empty() {
        return false;
    }
    match &f.kind {
        // A cylinder placed by reference edges follows the body it stands on.
        FeatureKind::Cylinder { references, .. } if !references.is_empty() => false,
        FeatureKind::Box { corner: _, length: _, width: _, height: _, operation: Operation::NewBody, centred: _ }
        | FeatureKind::Cylinder { base: _, axis: _, radius: _, height: _, operation: Operation::NewBody, references: _ }
        | FeatureKind::Sphere { center: _, radius: _, operation: Operation::NewBody }
        | FeatureKind::Torus { center: _, major: _, minor: _, operation: Operation::NewBody }
        | FeatureKind::Import { file: _, step: _, components: _, skip: _, records: _ }
        | FeatureKind::MeshImport { file: _, meshes: _ } => true,
        FeatureKind::Sketch { plane, sketch } => sketch.links.is_empty() && sketch.wires.is_empty() && local_plane(plane, 0),
        FeatureKind::Extrude { sketch, profiles: _, extent, operation: Operation::NewBody, targets: _, thin: _ } => {
            let crate::Extent {
                distance: _,
                convention: _,
                direction: _,
                distance2: _,
                start_offset: _,
                symmetric_offset: _,
                through_all,
                second_follows_first: _,
                taper: _,
                taper2: _,
                to,
                to_offset: _,
                through_all2,
                to2,
                to_offset2: _,
                from,
                corner: _,
                to_body_apart: _,
                from_level: _,
                picked_placements: _,
                one_sided_direction: _,
                to_ref: _,
                from_ref: _,
                extend_to_face: _,
            } = extent;
            to.is_none() && from.is_none() && !through_all && !through_all2 && to2.is_none() && components.get(&sketch.0) == Some(&f.component)
        }
        _ => false,
    }
}

fn no_face_attachments(f: &Feature) -> bool {
    f.face_names.iter().all(String::is_empty)
}

fn no_edge_attachments(f: &Feature) -> bool {
    f.edge_names.iter().all(String::is_empty) && f.edge_refs.is_empty()
}

fn no_attachments(f: &Feature) -> bool {
    no_face_attachments(f) && no_edge_attachments(f)
}

fn body_owned_by(doc: &Document, st: &ModelState, name: &str, own: u64) -> bool {
    st.body(name).is_some_and(|b| b.component == own && doc.model_body_component(b) == own)
}

fn own_sketch(doc: &Document, components: &HashMap<u64, u64>, sketch: u64, own: u64) -> bool {
    components.get(&sketch) == Some(&own) && doc.sketch(sketch).is_ok_and(|s| s.wires.is_empty())
}

fn face_names_owned_by(doc: &Document, st: &ModelState, f: &Feature, own: u64) -> bool {
    f.face_names
        .iter()
        .filter(|n| !n.is_empty())
        .all(|n| crate::name_lookup::face_body(st, n).is_some_and(|b| b.component == own && doc.model_body_component(b) == own))
}

/// Whether every body modified, removed, or created between `before` and `after` belongs strictly
/// to component `own` (and, when `require_any` is true, at least one body in `own` changed).
fn modified_or_created_bodies_owned_by(doc: &Document, before: &ModelState, after: &ModelState, own: u64, require_any: bool) -> bool {
    let owned = |b: &super::ModelBody| b.component == own && doc.model_body_component(b) == own;
    let before_by_name: HashMap<&str, &super::ModelBody> = before.bodies.iter().map(|b| (b.name.as_str(), b)).collect();
    let after_by_name: HashMap<&str, &super::ModelBody> = after.bodies.iter().map(|b| (b.name.as_str(), b)).collect();
    if before_by_name.len() != before.bodies.len() || after_by_name.len() != after.bodies.len() {
        return false;
    }
    let mut any = false;
    for old in &before.bodies {
        match after_by_name.get(old.name.as_str()) {
            Some(now) if now.body.same(&old.body) => {}
            Some(now) => {
                any = true;
                if !owned(old) || !owned(now) {
                    return false;
                }
            }
            None => {
                any = true;
                if !owned(old) {
                    return false;
                }
            }
        }
    }
    for now in &after.bodies {
        if !before_by_name.contains_key(now.name.as_str()) {
            any = true;
            if !owned(now) {
                return false;
            }
        }
    }
    !require_any || any
}

fn boolean_targets_local(doc: &Document, st: &ModelState, scope: &Feature, op: Operation, targets: &[String]) -> bool {
    let effective: &[String] = if targets.is_empty() { &scope.participants } else { targets };
    if op == Operation::NewBody {
        return effective.iter().all(|n| n != "*" && body_owned_by(doc, st, n, scope.component));
    }
    if effective.iter().any(|n| n == "*") {
        return false;
    }
    if effective.is_empty() {
        st.bodies
            .iter()
            .filter(|b| b.in_default_scope(scope))
            .all(|b| b.component == scope.component && doc.model_body_component(b) == scope.component)
    } else {
        effective.iter().all(|n| body_owned_by(doc, st, n, scope.component))
    }
}

fn boolean_body_targets_local(doc: &Document, st: &ModelState, scope: &Feature, op: Operation, targets: &[crate::BodyName]) -> bool {
    if targets.is_empty() {
        return boolean_targets_local(doc, st, scope, op, &[]);
    }
    if targets.iter().any(|n| n.0 == "*") {
        return false;
    }
    targets.iter().all(|n| body_owned_by(doc, st, &n.0, scope.component))
}

fn local_axis(doc: &Document, components: &HashMap<u64, u64>, axis: &AxisRef, own: u64) -> bool {
    match axis {
        AxisRef::SketchAxis { axis: _ }
        | AxisRef::World { axis: _ }
        | AxisRef::Line { origin: _, dir: _ }
        | AxisRef::SketchLine { curve: _, sketch: None } => true,
        AxisRef::SketchLine { curve: _, sketch: Some(sketch) } => own_sketch(doc, components, *sketch, own),
        AxisRef::Model { .. } | AxisRef::Construction { .. } => false,
    }
}

fn local_pattern_kind(doc: &Document, before: &ModelState, components: &HashMap<u64, u64>, pattern: &crate::PatternKind, own: u64) -> bool {
    match pattern {
        crate::PatternKind::Rectangular {
            dir1: _,
            count1: _,
            spacing1: _,
            dir2: _,
            count2: _,
            spacing2: _,
            alignment: _,
            alignment2: _,
            reversed: _,
            reversed2: _,
            distance1: _,
            distance2: _,
            extent1: _,
            extent2: _,
            copy_at_end1: _,
            copy_at_end2: _,
            spacing_list1: _,
            spacing_list2: _,
        } => true,
        crate::PatternKind::Circular { origin: _, axis: _, count: _, angle: _, alignment: _, reversed: _, axis_ref, axis_feature } => {
            axis_ref.is_none() && axis_feature.is_none()
        }
        crate::PatternKind::Path {
            path_sketch,
            path: _,
            path_body,
            path_edges: _,
            count: _,
            spacing: _,
            extent: _,
            flip: _,
            orient: _,
            start: _,
            start_fraction: _,
            follow_frame: _,
        } => match path_body {
            Some(body) => body_owned_by(doc, before, &body.0, own),
            None => own_sketch(doc, components, path_sketch.0, own),
        },
    }
}

fn local_replayed_features(doc: &Document, f: &Feature, before: &ModelState, after: &ModelState, features: &[FeatureId]) -> bool {
    if features.is_empty() || !features.iter().all(|id| super::source_feature(doc, *id).is_ok()) {
        return false;
    }
    let sources = super::replayed_sources(doc, features);
    if sources.is_empty() {
        return false;
    }
    for src in &sources {
        if src.component != f.component || super::kept_in(doc, before, f.component, src).is_some() {
            return false;
        }
        match &src.kind {
            FeatureKind::Pattern { features: inner, bodies, components, .. }
                if bodies.is_empty() && components.is_empty() && !inner.iter().all(|id| super::source_feature(doc, *id).is_ok()) =>
            {
                return false;
            }
            FeatureKind::Mirror { features: inner, bodies, .. }
                if bodies.is_empty() && !inner.iter().all(|id| super::source_feature(doc, *id).is_ok()) =>
            {
                return false;
            }
            _ => {}
        }
        if let Some(tc) = super::tools_component(doc, src) {
            if tc != f.component {
                return false;
            }
            let (op, targets, scope) = super::copy_targets(before, f, src);
            if scope.component != f.component || !boolean_targets_local(doc, before, &scope, op, &targets) {
                return false;
            }
        }
    }
    modified_or_created_bodies_owned_by(doc, before, after, f.component, false)
}

/// Whether `f`'s evaluation (from `before` to `after`) depends strictly on geometry and inputs
/// inside `f.component`, with zero cross-component or external-placement references.
///
/// Any unknown variant or any reference outside `f.component` conservatively returns `false`,
/// keeping the design on the ordinary evaluation path.
pub(super) fn placement_local_feature(doc: &Document, f: &Feature, before: &ModelState, after: &ModelState, components: &HashMap<u64, u64>) -> bool {
    if f.profile_frame.is_some() {
        return false;
    }
    match &f.kind {
        FeatureKind::Import { file: _, step: _, components: _, skip: _, records: _ } | FeatureKind::MeshImport { file: _, meshes: _ } => {
            no_attachments(f)
        }
        FeatureKind::Sketch { plane, sketch } => {
            no_attachments(f)
                && sketch.links.is_empty()
                && sketch.wires.is_empty()
                && sketch.wire_constraints.iter().all(|c| c.kind.model_plane().is_none())
                && local_plane(plane, 0)
        }
        FeatureKind::Box { corner: _, length: _, width: _, height: _, operation, centred: _ }
        | FeatureKind::Sphere { center: _, radius: _, operation }
        | FeatureKind::Torus { center: _, major: _, minor: _, operation }
        | FeatureKind::Cone { base: _, axis: _, radius: _, top_radius: _, height: _, operation } => {
            no_attachments(f) && boolean_targets_local(doc, before, f, *operation, &[])
        }
        FeatureKind::Cylinder { base: _, axis: _, radius: _, height: _, operation, references } => {
            no_attachments(f) && references.is_empty() && boolean_targets_local(doc, before, f, *operation, &[])
        }
        FeatureKind::Extrude { sketch, profiles: _, extent, operation, targets, thin: _ } => {
            let crate::Extent {
                distance: _,
                convention: _,
                direction: _,
                distance2: _,
                start_offset: _,
                symmetric_offset: _,
                through_all: _,
                second_follows_first: _,
                taper: _,
                taper2: _,
                corner: _,
                to,
                to_offset: _,
                from,
                through_all2: _,
                to2,
                to_offset2: _,
                from_level: _,
                to_body_apart: _,
                picked_placements,
                one_sided_direction: _,
                to_ref,
                from_ref,
                extend_to_face: _,
            } = extent;
            no_attachments(f)
                && to.is_none()
                && from.is_none()
                && to2.is_none()
                && to_ref.is_none()
                && from_ref.is_none()
                && picked_placements.is_empty()
                && own_sketch(doc, components, sketch.0, f.component)
                && boolean_body_targets_local(doc, before, f, *operation, targets)
        }
        FeatureKind::Revolve {
            sketch,
            profiles: _,
            axis,
            angle: _,
            operation,
            targets,
            angle2: _,
            symmetric: _,
            convention: _,
            join_profile_arcs: _,
            to,
        } => {
            no_attachments(f)
                && to.is_none()
                && own_sketch(doc, components, sketch.0, f.component)
                && local_axis(doc, components, axis, f.component)
                && boolean_body_targets_local(doc, before, f, *operation, targets)
        }
        FeatureKind::Sweep {
            sketch,
            profiles: _,
            path_sketch,
            path,
            path_body,
            path_edges: _,
            orientation: _,
            twist: _,
            scale: _,
            rails,
            rail_fit: _,
            operation,
            targets,
        } => {
            let path_ok = match path_body {
                Some(body) => body_owned_by(doc, before, &body.0, f.component),
                None => {
                    own_sketch(doc, components, path_sketch.0, f.component)
                        && path.iter().all(|entry| own_sketch(doc, components, entry.split(*path_sketch).0.0, f.component))
                }
            };
            no_attachments(f)
                && own_sketch(doc, components, sketch.0, f.component)
                && path_ok
                && rails.iter().all(|r| own_sketch(doc, components, r.sketch.0, f.component))
                && boolean_body_targets_local(doc, before, f, *operation, targets)
        }
        FeatureKind::Loft { sections, operation, targets, rails, rail_fit: _ } => {
            no_attachments(f)
                && !sections.is_empty()
                && sections.iter().all(|s| own_sketch(doc, components, s.sketch.0, f.component))
                && rails.iter().all(|r| own_sketch(doc, components, r.sketch.0, f.component))
                && boolean_body_targets_local(doc, before, f, *operation, targets)
        }
        FeatureKind::Fillet { edges: _, radius: _, body, style: _, fit: _, corner: _, setbacks: _, turn: _, tangent_chain: _, edge_radii: _ }
        | FeatureKind::Chamfer { edges: _, distance: _, distance2: _, angle: _, flip: _, body, tangent_chain: _ } => {
            no_face_attachments(f)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => modified_or_created_bodies_owned_by(doc, before, after, f.component, true),
                }
        }
        FeatureKind::Shell { faces: _, thickness: _, body, direction: _, tangent_chain: _ } => {
            no_edge_attachments(f)
                && face_names_owned_by(doc, before, f, f.component)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => modified_or_created_bodies_owned_by(doc, before, after, f.component, true),
                }
        }
        FeatureKind::Draft { faces: _, angle: _, neutral, pull: _, body } => {
            no_edge_attachments(f)
                && local_plane(neutral, 0)
                && face_names_owned_by(doc, before, f, f.component)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => modified_or_created_bodies_owned_by(doc, before, after, f.component, true),
                }
        }
        FeatureKind::Hole {
            position: _,
            at: _,
            direction: _,
            diameter: _,
            depth: _,
            hole: _,
            points,
            thread: _,
            thread_depth: _,
            hand: _,
            thread_class: _,
            modeled: _,
            ease_out: _,
            to,
            references,
            old_surface_start: _,
            hardware: _,
        } => {
            no_edge_attachments(f)
                && to.is_none()
                && references.is_empty()
                && face_names_owned_by(doc, before, f, f.component)
                && points.as_ref().is_none_or(|sp| own_sketch(doc, components, sp.sketch.0, f.component))
                && boolean_targets_local(doc, before, f, Operation::Cut, &[])
        }
        FeatureKind::Pattern { features, pattern, bodies, placements, components: pat_components, occurrences: _, join: _ } => {
            if !no_attachments(f) || !pat_components.is_empty() || !placements.is_empty() {
                return false;
            }
            if !local_pattern_kind(doc, before, components, pattern, f.component) {
                return false;
            }
            if !bodies.is_empty() {
                features.is_empty() && bodies.iter().all(|b| body_owned_by(doc, before, &b.0, f.component))
            } else {
                local_replayed_features(doc, f, before, after, features)
            }
        }
        FeatureKind::Mirror { features, plane, bodies, combine: _, placements } => {
            if !no_attachments(f) || !placements.is_empty() || !local_plane(plane, 0) {
                return false;
            }
            if !bodies.is_empty() {
                features.is_empty() && bodies.iter().all(|b| body_owned_by(doc, before, &b.0, f.component))
            } else {
                local_replayed_features(doc, f, before, after, features)
            }
        }
        FeatureKind::Move { bodies, translate: _, rotate_axis: _, rotate_about, origin: _, angle: _, copy: _, copy_frames: _, carry, leave } => {
            no_attachments(f)
                && rotate_about.is_none()
                && carry.is_empty()
                && leave.is_empty()
                && !bodies.is_empty()
                && bodies.iter().all(|b| body_owned_by(doc, before, &b.0, f.component))
        }
        FeatureKind::Scale { bodies, origin: _, about, factor: _, factors: _ } => {
            no_attachments(f)
                && matches!(about, None | Some(ScaleAbout::Centre))
                && !bodies.is_empty()
                && bodies.iter().all(|b| body_owned_by(doc, before, &b.0, f.component))
        }
        FeatureKind::Split { body, plane, tool, tool_plane: _, tool_sheet: _, tool_faces: _, tool_bodies, sketch, line: _, curves: _, extend: _ } => {
            let tool_ok = if let Some(sk) = sketch {
                own_sketch(doc, components, sk.0, f.component)
            } else if let Some(t) = tool {
                body_owned_by(doc, before, &t.body.0, f.component) && tool_bodies.iter().all(|tb| body_owned_by(doc, before, &tb.0, f.component))
            } else {
                local_plane(plane, 0)
            };
            no_edge_attachments(f) && face_names_owned_by(doc, before, f, f.component) && body_owned_by(doc, before, &body.0, f.component) && tool_ok
        }
        FeatureKind::Combine { target, tools, operation: _, keep_tools: _, offset: _, offset_edges: _, offset_faces: _, offset_radius: _ } => {
            no_edge_attachments(f)
                && face_names_owned_by(doc, before, f, f.component)
                && body_owned_by(doc, before, &target.0, f.component)
                && !tools.is_empty()
                && tools.iter().all(|t| body_owned_by(doc, before, &t.0, f.component))
        }
        FeatureKind::SheetBase { sketch, profiles: _, rule: _, flip: _ }
        | FeatureKind::SheetContour { sketch, curves: _, distance: _, radius: _, rule: _, orientation: _, symmetric: _, flip: _, reverse: _ } => {
            no_attachments(f) && own_sketch(doc, components, sketch.0, f.component)
        }
        FeatureKind::SheetFlange {
            edges: _,
            height: _,
            angle: _,
            radius: _,
            position: _,
            flip: _,
            body,
            corner_relief_shape: _,
            corner_relief: _,
            height_from: _,
            bend_relief_shape: _,
            corners: _,
            offset: _,
        }
        | FeatureKind::SheetHem { edges: _, length: _, gap: _, flip: _, body } => {
            no_face_attachments(f)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => modified_or_created_bodies_owned_by(doc, before, after, f.component, true),
                }
        }
        FeatureKind::SheetPerforate { diameter: _, pitch: _, margin: _, pattern: _, body, sketch, profiles: _ } => {
            no_attachments(f)
                && sketch.is_none_or(|sk| own_sketch(doc, components, sk.0, f.component))
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => before.surface_sheets.last().is_some_and(|s| body_owned_by(doc, before, &s.body, f.component)),
                }
        }
        FeatureKind::SheetFold { sketch, curve: _, a: _, b: _, angle: _, radius: _, position: _, flip: _, fixed: _, stays: _, body } => {
            no_attachments(f)
                && sketch.is_none_or(|sk| own_sketch(doc, components, sk.0, f.component))
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => {
                        before.surface_sheets.iter().all(|s| body_owned_by(doc, before, &s.body, f.component))
                            && modified_or_created_bodies_owned_by(doc, before, after, f.component, true)
                    }
                }
        }
        FeatureKind::SheetPath { edge: _, sketch, curves: _, radius: _, body } | FeatureKind::SheetTab { sketch, profiles: _, body } => {
            no_attachments(f)
                && own_sketch(doc, components, sketch.0, f.component)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => modified_or_created_bodies_owned_by(doc, before, after, f.component, true),
                }
        }
        FeatureKind::SheetBead {
            sketch,
            curves: _,
            shape: _,
            depth: _,
            width: _,
            angle: _,
            crown_width: _,
            base_radius: _,
            crown_radius: _,
            end_length: _,
            margin: _,
            flip: _,
            body,
        } => {
            no_attachments(f)
                && own_sketch(doc, components, sketch.0, f.component)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => {
                        before.surface_sheets.iter().all(|s| body_owned_by(doc, before, &s.body, f.component))
                            && modified_or_created_bodies_owned_by(doc, before, after, f.component, true)
                    }
                }
        }
        FeatureKind::SheetUnfold { body, refold: _, fixed: _ } => {
            no_attachments(f)
                && match body {
                    Some(name) => body_owned_by(doc, before, &name.0, f.component),
                    None => modified_or_created_bodies_owned_by(doc, before, after, f.component, true),
                }
        }
        FeatureKind::FrameMember {
            path_sketch,
            lines: _,
            continuous: _,
            profile,
            anchor: _,
            locate: _,
            corner: _,
            flip: _,
            fit_ends,
            coped: _,
            corners: _,
            gap: _,
            angle: _,
            mirror: _,
            up: _,
            body_name: _,
            names: _,
            pairs,
            next_pair: _,
            groups: _,
        } => {
            let profile_ok = match profile {
                crate::frame::MemberProfile::Library { profile: _ } => true,
                crate::frame::MemberProfile::Sketch { sketch, profiles: _ } => own_sketch(doc, components, sketch.0, f.component),
            };
            let fit_ok = !*fit_ends
                || before
                    .bodies
                    .iter()
                    .all(|b| b.frame_member_line.is_none() || (b.component == f.component && doc.model_body_component(b) == f.component));
            no_attachments(f)
                && pairs.is_empty()
                && path_sketch.is_some_and(|sk| own_sketch(doc, components, sk.0, f.component))
                && profile_ok
                && fit_ok
        }
        FeatureKind::FrameMemberTurn { member, angle: _ } => no_attachments(f) && components.get(&member.feature.0) == Some(&f.component),
        FeatureKind::FrameTrim { members, to, end: _, gap: _ } => {
            no_attachments(f)
                && !members.is_empty()
                && members.iter().all(|m| doc.find_feature(m.feature.as_ref()).is_some_and(|mf| mf.component == f.component))
                && !to.is_empty()
                && to.iter().all(|target| match target {
                    crate::frame::TrimTarget::Member { member, side: _ } => {
                        doc.find_feature(member.feature.as_ref()).is_some_and(|mf| mf.component == f.component)
                    }
                    crate::frame::TrimTarget::Plane { plane } => local_plane(plane, 0),
                })
        }
        FeatureKind::FrameCope { member, tools, end: _ } => {
            no_attachments(f)
                && body_owned_by(doc, before, &member.0, f.component)
                && !tools.is_empty()
                && tools.iter().all(|t| body_owned_by(doc, before, &t.0, f.component))
        }
        _ => false,
    }
}

/// Every non-placement field is named, so a new input requires an explicit decision here.
/// Assembly records do not feed any of the feature kinds accepted by `local_geometry`.
fn same_geometry_inputs(a: &Document, b: &Document) -> bool {
    let Document {
        newer_file: _,
        render,
        format,
        format_pending,
        name,
        units,
        metadata,
        params,
        features,
        marker,
        next_id,
        reserved_body_names,
        components,
        occurrences: _,
        assembly: _,
        // Drawings draw the model; they feed none of it.
        drawings: _,
        sheet,
        plastic,
        parts,
        links,
        // What a file keeps of its links; a design in memory holds their bodies either way.
        linked_bodies_left_out: _,
        configs,
        favorites,
        canvases,
        param_comments,
        text_params,
        next_component_color_index: _,
        materials,
        print_faces,
        // Read only by robot exports: no feature's geometry depends on it.
        collision_shapes: _,
        // Display only: no feature's geometry depends on it.
        edgeless_bodies: _,
        user_materials,
        appearances,
        body_components,
        body_offsets,
        browser_groups,
        timeline_groups,
        browser_order,
        named_views,
        home_view,
        saved_sections,
        visibility,
        point_clouds,
        // Display only: a plane's drawn size shapes nothing.
        plane_extents: _,
        // Display only: the design's own view frame shapes nothing.
        view_frame: _,
        notes: _,
        // Plugin data is kept beside the design; no feature reads it.
        plugin_data: _,
        // Weld graphics do not feed feature evaluation.
        welds: _,
    } = a;
    (Arc::ptr_eq(render, &b.render) || *render == b.render)
        && *format == b.format
        && *format_pending == b.format_pending
        && *name == b.name
        && *units == b.units
        && *metadata == b.metadata
        && *params == b.params
        && features.len() == b.features.len()
        && features.iter().zip(&b.features).all(|(a, b)| same_feature(a, b))
        && *marker == b.marker
        && *next_id == b.next_id
        && *reserved_body_names == b.reserved_body_names
        && components.len() == b.components.len()
        && components.iter().zip(&b.components).all(|(a, b)| same_eval_component(a, b))
        && *sheet == b.sheet
        && *plastic == b.plastic
        && *parts == b.parts
        && *links == b.links
        && *configs == b.configs
        && *favorites == b.favorites
        && *canvases == b.canvases
        && *param_comments == b.param_comments
        && *text_params == b.text_params
        && *materials == b.materials
        && *print_faces == b.print_faces
        && *user_materials == b.user_materials
        && *appearances == b.appearances
        && *body_components == b.body_components
        && *body_offsets == b.body_offsets
        && *browser_groups == b.browser_groups
        && *timeline_groups == b.timeline_groups
        && *browser_order == b.browser_order
        && *named_views == b.named_views
        && *home_view == b.home_view
        && *saved_sections == b.saved_sections
        && *visibility == b.visibility
        && *point_clouds == b.point_clouds
}

/// Compare component fields that geometry evaluation can read; display and metadata fields
/// (`history_hidden`, `identification_color`, `metadata`, etc.) never affect geometry (#22662).
fn same_eval_component(a: &crate::Component, b: &crate::Component) -> bool {
    let crate::Component {
        id,
        name,
        parent,
        identification_color: _,
        identification_color_index: _,
        metadata: _,
        properties: _,
        name_split: _,
        history_hidden: _,
        physical_override: _,
    } = a;
    *id == b.id && *name == b.name && *parent == b.parent
}

/// Compare feature definitions without traversing immutable imported payloads. A replacement
/// allocation conservatively evaluates again, even when it happens to contain equal bytes.
fn same_feature(a: &Feature, b: &Feature) -> bool {
    let Feature {
        id,
        name,
        suppressed,
        body_names,
        param_names,
        component,
        edge_refs,
        edge_names,
        face_names,
        participants,
        touching,
        shell_piece_order,
        kernel_piece_order,
        least_face_piece_order,
        pattern_names_in_order,
        inserted,
        link_faces,
        link_threads,
        profile_frame,
        kind,
    } = a;
    *id == b.id
        && *name == b.name
        && *suppressed == b.suppressed
        && *body_names == b.body_names
        && *param_names == b.param_names
        && *component == b.component
        && *edge_refs == b.edge_refs
        && *edge_names == b.edge_names
        && *face_names == b.face_names
        && *participants == b.participants
        && *touching == b.touching
        && *profile_frame == b.profile_frame
        && *shell_piece_order == b.shell_piece_order
        && *kernel_piece_order == b.kernel_piece_order
        && *least_face_piece_order == b.least_face_piece_order
        && *pattern_names_in_order == b.pattern_names_in_order
        && *inserted == b.inserted
        && *link_faces == b.link_faces
        && *link_threads == b.link_threads
        && match (kind, &b.kind) {
            (FeatureKind::MeshImport { file: a, meshes }, FeatureKind::MeshImport { file: b, meshes: other }) => {
                a == b
                    && meshes.len() == other.len()
                    && meshes.iter().zip(other).all(|(a, b)| {
                        let crate::MeshData { name, positions, triangles, corner_normals, color } = a;
                        *name == b.name
                            && *color == b.color
                            && Arc::ptr_eq(positions, &b.positions)
                            && Arc::ptr_eq(triangles, &b.triangles)
                            && Arc::ptr_eq(corner_normals, &b.corner_normals)
                    })
            }
            (
                FeatureKind::Import { file: a, step, components, skip, records },
                FeatureKind::Import { file: b, step: other, components: other_components, skip: other_skip, records: other_records },
            ) => {
                let same_records = match (records, other_records) {
                    (Some(r), Some(o)) => crate::SharedRecords::ptr_eq(r, o),
                    (None, None) => true,
                    _ => false,
                };
                a == b && Arc::ptr_eq(step, other) && components == other_components && skip == other_skip && same_records
            }
            (a, b) => a == b,
        }
}
