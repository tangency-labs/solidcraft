//! Retain a proven evaluation across changes to assembly placement. Unknown feature kinds
//! use the ordinary evaluator; adding an eligible kind requires checking all of its inputs.

use super::{Document, Feature, FeatureKind, Model, ModelState, Operation, PlaneRef};
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
    /// Only explicitly supported local geometry qualifies. Geometry edits, unknown feature
    /// kinds, stale/primed results and `no_cache` continue through the ordinary evaluator.
    pub fn evaluate_shared(&mut self, doc: &Arc<Document>) -> bool {
        let reusable = !self.no_cache
            && self.primed.is_none()
            && self.placement_inputs.as_ref().is_some_and(|inputs| {
                inputs.state.upgrade().is_some_and(|state| Arc::ptr_eq(&state, &self.state()))
                    && !self.is_stale()
                    && (Arc::ptr_eq(doc, &inputs.document) || same_geometry_inputs(doc, &inputs.document))
            });
        if reusable {
            self.last_recomputed = 0;
            self.last_restored = 0;
            return false;
        }
        self.evaluate(doc);
        if !self.no_cache && self.primed.is_none() && self.placement_local {
            self.placement_inputs = Some(Inputs { document: doc.clone(), state: Arc::downgrade(&self.state()) });
        }
        true
    }

    /// [`Model::refingerprint`] for a shared document: a proof that held for the results before
    /// is kept for `doc`, since nothing was built differently (pinned body names, say).
    pub fn refingerprint_shared(&mut self, doc: &Arc<Document>) {
        let proven = !self.no_cache
            && self.primed.is_none()
            && self.placement_inputs.as_ref().is_some_and(|inputs| inputs.state.upgrade().is_some_and(|state| Arc::ptr_eq(&state, &self.state())))
            && !self.is_stale();
        self.refingerprint(doc);
        if proven && self.placement_local {
            self.placement_inputs = Some(Inputs { document: doc.clone(), state: Arc::downgrade(&self.state()) });
        }
    }
}

fn local_plane(plane: &PlaneRef, depth: usize) -> bool {
    if depth > crate::MAX_PLACEMENT_DEPTH {
        return false;
    }
    match plane {
        PlaneRef::Origin { name: _ } | PlaneRef::Custom { plane: _ } | PlaneRef::Placed { origin: _, x_dir: _, y_dir: _ } => true,
        PlaneRef::Offset { base, distance: _ }
        | PlaneRef::Flipped { base, about_y: _ }
        | PlaneRef::AtAngle { base, axis_origin: _, axis_dir: _, angle: _, line: None } => local_plane(base, depth + 1),
        _ => false,
    }
}

pub(super) fn local_geometry(f: &Feature, components: &std::collections::HashMap<u64, u64>) -> bool {
    // Persistent picked-face/edge attachments can move an otherwise literal input.
    if !f.face_names.is_empty() || !f.edge_names.is_empty() || !f.edge_refs.is_empty() {
        return false;
    }
    match &f.kind {
        // A cylinder placed by reference edges follows the body it stands on.
        FeatureKind::Cylinder { references, .. } if !references.is_empty() => false,
        FeatureKind::Box { corner: _, length: _, width: _, height: _, operation: Operation::NewBody }
        | FeatureKind::Cylinder { base: _, axis: _, radius: _, height: _, operation: Operation::NewBody, references: _ }
        | FeatureKind::Sphere { center: _, radius: _, operation: Operation::NewBody }
        | FeatureKind::Torus { center: _, major: _, minor: _, operation: Operation::NewBody }
        | FeatureKind::Import { file: _, step: _, components: _, skip: _ }
        | FeatureKind::MeshImport { file: _, meshes: _ } => true,
        FeatureKind::Sketch { plane, sketch } => sketch.links.is_empty() && sketch.wires.is_empty() && local_plane(plane, 0),
        FeatureKind::Extrude { sketch, profiles: _, extent, operation: Operation::NewBody, targets: _, thin: _ } => {
            let crate::Extent {
                distance: _,
                convention: _,
                direction: _,
                distance2: _,
                start_offset: _,
                through_all,
                taper: _,
                to,
                to_offset: _,
                from,
            } = extent;
            to.is_none() && from.is_none() && !through_all && components.get(&sketch.0) == Some(&f.component)
        }
        _ => false,
    }
}

/// Every non-placement field is named, so a new input requires an explicit decision here.
/// Assembly records do not feed any of the feature kinds accepted by `local_geometry`.
fn same_geometry_inputs(a: &Document, b: &Document) -> bool {
    let Document {
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
        sheet,
        plastic,
        parts,
        links,
        configs,
        favorites,
        canvases,
        param_comments,
        text_params,
        next_component_color_index,
        materials,
        user_materials,
        appearances,
        body_components,
        body_offsets,
        browser_groups,
        timeline_groups,
        browser_order,
        named_views,
        visibility,
        point_clouds,
        // Display only: the design's own view frame shapes nothing.
        view_frame: _,
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
        && *components == b.components
        && *sheet == b.sheet
        && *plastic == b.plastic
        && *parts == b.parts
        && *links == b.links
        && *configs == b.configs
        && *favorites == b.favorites
        && *canvases == b.canvases
        && *param_comments == b.param_comments
        && *text_params == b.text_params
        && *next_component_color_index == b.next_component_color_index
        && *materials == b.materials
        && *user_materials == b.user_materials
        && *appearances == b.appearances
        && *body_components == b.body_components
        && *body_offsets == b.body_offsets
        && *browser_groups == b.browser_groups
        && *timeline_groups == b.timeline_groups
        && *browser_order == b.browser_order
        && *named_views == b.named_views
        && *visibility == b.visibility
        && *point_clouds == b.point_clouds
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
        shell_piece_order,
        kernel_piece_order,
        inserted,
        link_faces,
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
        && *shell_piece_order == b.shell_piece_order
        && *kernel_piece_order == b.kernel_piece_order
        && *inserted == b.inserted
        && *link_faces == b.link_faces
        && match (kind, &b.kind) {
            (FeatureKind::MeshImport { file: a, meshes }, FeatureKind::MeshImport { file: b, meshes: other }) => {
                a == b
                    && meshes.len() == other.len()
                    && meshes.iter().zip(other).all(|(a, b)| {
                        let crate::MeshData { name, positions, triangles, color } = a;
                        *name == b.name && *color == b.color && Arc::ptr_eq(positions, &b.positions) && Arc::ptr_eq(triangles, &b.triangles)
                    })
            }
            (
                FeatureKind::Import { file: a, step, components, skip },
                FeatureKind::Import { file: b, step: other, components: other_components, skip: other_skip },
            ) => a == b && Arc::ptr_eq(step, other) && components == other_components && skip == other_skip,
            (a, b) => a == b,
        }
}
