//! Moving timeline features (sketches included) into another component.
//!
//! It is an organisational change: ids, names and timeline positions stay, nothing is copied, and
//! the geometry stays where it is in the world (a moved feature's picks, planes and directions are
//! re-expressed in the new component's frame). Bodies follow the feature that made them. The move
//! is made on a scratch copy and the whole timeline is evaluated; the result is adopted only when
//! no feature, moved or left behind, gained an error. Otherwise nothing changes.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_doc::{Feature, FeatureKind, is_identity, mat_inverse, mat_mul};

use super::super::clipboard::explicit_planes;
use super::*;
use crate::params::name_or_id_list;

pub(super) fn move_features(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.move_features";
    let dest = if p.get("component").is_some() { component(s, p, "component", cmd)? } else { s.active_component };
    let keys = name_or_id_list(cmd, p, "features")?;
    if keys.is_empty() || keys.len() > 1000 {
        return Err(bad(cmd, "`features` must list 1…1000 features"));
    }
    if s.doc.marker.is_some() {
        return Err(bad(cmd, "finish timeline rollback before moving features"));
    }
    if !s.pending_moves.is_empty() {
        return Err(bad(cmd, "finish the component move in progress first"));
    }
    let mut ids = BTreeSet::new();
    for k in &keys {
        ids.insert(s.doc.find_feature(k).map(|f| f.id).ok_or_else(|| bad(cmd, format!("no feature `{k}`")))?);
    }
    let moving: BTreeSet<u64> = ids.into_iter().filter(|id| s.doc.feature(*id).is_some_and(|f| f.component != dest)).collect();
    if moving.is_empty() {
        return Err(bad(cmd, "already in that component"));
    }
    let moved: Vec<Feature> = s.doc.features.iter().filter(|f| moving.contains(&f.id)).cloned().collect();
    // One placement each: a component placed twice has no single frame to keep the geometry in.
    let placements = |c: u64| s.doc.occurrences.iter().filter(|o| o.component == c).count();
    let sources: BTreeSet<u64> = moved.iter().map(|f| f.component).collect();
    if sources.iter().chain([&dest]).any(|c| *c != 0 && placements(*c) > 1) {
        return Err(bad(cmd, "a component placed more than once can't take part in this move"));
    }
    if moved.iter().any(|f| matches!(f.kind, FeatureKind::BodyTransfer { .. })) {
        return Err(bad(cmd, "a recorded body transfer can't be moved"));
    }
    // The bodies that move with the features: those they are the origin of.
    let moved_bodies: BTreeSet<String> = s.body_origins().into_iter().filter(|(_, f)| moving.contains(f)).map(|(body, _)| body).collect();
    if moved_bodies.iter().any(|b| s.doc.body_components.contains_key(b) || s.doc.body_offsets.contains_key(b)) {
        return Err(bad(cmd, "a body that was transferred between components can't be moved with its features"));
    }
    // A join or boolean whose inputs would end up in two components.
    for f in &s.doc.features {
        let mut inputs: BTreeSet<String> = f.kind.input_bodies().into_iter().collect();
        inputs.extend(f.participants.iter().cloned());
        let (inside, outside) =
            (inputs.iter().filter(|b| moved_bodies.contains(*b)).count(), inputs.iter().filter(|b| !moved_bodies.contains(*b)).count());
        if inside > 0 && outside > 0 && inputs.len() > 1 {
            return Err(bad(cmd, format!("{} works on bodies that would be split between components", f.name)));
        }
    }
    let to_dest = mat_inverse(&s.doc.component_transform(dest)).ok_or_else(|| bad(cmd, "the target component's frame is not invertible"))?;
    let dest_path = s.doc.first_path(dest);
    let before_errors: BTreeMap<u64, bool> = s.doc.features.iter().map(|f| (f.id, s.model.result(f.id).is_some_and(|r| r.error.is_some()))).collect();

    let mut scratch = s.scratch();
    let doc = scratch.doc_mut();
    for old in &moved {
        let delta = mat_mul(&to_dest, &s.doc.component_transform(old.component));
        let Some(f) = doc.feature_mut(old.id) else { continue };
        f.component = dest;
        if !is_identity(&delta) {
            if let FeatureKind::Sketch { plane, .. } | FeatureKind::ConstructionPlane { plane } = &mut f.kind {
                explicit_planes(plane);
            }
            f.kind.transform_geometry(&delta);
        }
    }
    // Attached sketches and embossed faces carry the placement of the body they sit on and of
    // the component they were authored in: those that moved are rewritten, nothing else changes.
    for f in &mut doc.features {
        let authored_here = moving.contains(&f.id);
        let rewrite = |body: &str, path: &mut Vec<u64>, author_path: &mut Vec<u64>| {
            if moved_bodies.contains(body) {
                *path = dest_path.clone();
            }
            if authored_here {
                *author_path = dest_path.clone();
            }
        };
        match &mut f.kind {
            FeatureKind::Sketch { plane, .. } => {
                if let Some(anchor) = crate::cmd::sketch::face::anchor_mut(plane) {
                    let moved_body = moved_bodies.contains(&anchor.target.body);
                    let author_delta = if authored_here {
                        let old = s.doc.feature(f.id).ok_or_else(|| bad(cmd, "attached sketch no longer exists"))?;
                        mat_mul(&to_dest, &s.doc.component_transform(old.component))
                    } else {
                        solidcraft_doc::IDENTITY
                    };
                    let source_delta =
                        if moved_body { mat_mul(&to_dest, &s.doc.component_transform(anchor.component)) } else { solidcraft_doc::IDENTITY };
                    anchor.reframe_placement(&author_delta, &source_delta)?;
                    rewrite(&anchor.target.body, &mut anchor.path, &mut anchor.author_path);
                    if moved_body {
                        anchor.component = dest;
                    }
                }
            }
            FeatureKind::Emboss { face: Some(face), .. } => {
                let body = face.body.as_ref().to_string();
                rewrite(&body, &mut face.path, &mut face.author_path);
            }
            _ => {}
        }
    }
    scratch.refresh();
    // Any feature that gains an error, moved or left behind, stops the move.
    let broken = scratch
        .doc
        .features
        .iter()
        .filter(|f| !before_errors.get(&f.id).copied().unwrap_or(false))
        .find_map(|f| scratch.model.result(f.id).and_then(|r| r.error.clone()).map(|e| (f.name.clone(), e)));
    if let Some((name, error)) = broken {
        return Err(bad(cmd, format!("moving would break {name}: {error}")));
    }
    s.accept_computed(scratch);
    let names: Vec<&str> = moved.iter().map(|f| f.name.as_str()).collect();
    Ok(json!({"moved": names, "component": dest, "bodies": moved_bodies}))
}

impl Session {
    /// The feature each body depends on first, by body name: the earliest feature whose own result
    /// holds a body of that name (a converted mesh keeps its import), else the creator the body
    /// carries (the pieces of a split). Moving that feature moves the body.
    pub fn body_origins(&self) -> BTreeMap<String, u64> {
        let mut origins = BTreeMap::new();
        for f in &self.doc.features {
            if let Some(r) = self.model.result(f.id) {
                for b in r.state.bodies.iter().filter(|b| b.feature == f.id) {
                    origins.entry(b.name.clone()).or_insert(f.id);
                }
            }
        }
        for b in &self.model.state().bodies {
            origins.entry(b.name.clone()).or_insert(b.feature);
        }
        origins
    }
}
