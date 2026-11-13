//! Non-destructive timeline authoring of qualified refined Form seams.
use super::{params, parse};
use crate::doc::{BodyName, Document, FeatureId, FeatureKind, Operation, forms::Definition};
use crate::{EngineError, Result, Session};
use serde_json::{Value, json};

pub(super) fn attached(document: &Document, source: u64) -> bool {
    document.features.iter().any(|feature| match &feature.kind {
        FeatureKind::Stitch { form_seam: Some(seam), .. } => seam.sources().contains(&FeatureId(source)),
        _ => false,
    })
}

pub(super) fn check_source_edit(document: &Document, source: u64, next: &Definition, operation: Operation) -> Result<()> {
    let old = document.feature(source).ok_or_else(|| invalid("the welded source Form is missing"))?;
    let FeatureKind::Form { definition: previous, .. } = &old.kind else {
        return Err(invalid("the welded source no longer has its Form charts"));
    };
    let (Some(old), Some(new)) = (&previous.hierarchy, &next.hierarchy) else {
        return Err(invalid("remove the refined Weld before replacing its Form hierarchy"));
    };
    if operation != Operation::NewBody
        || previous.imported != next.imported
        || previous.parameters.kind != next.parameters.kind
        || old.base.faces != new.base.faces
        || old.base.points.len() != new.base.points.len()
        || old.leaves.len() > new.leaves.len()
        || old.leaves.iter().zip(&new.leaves).any(|(a, b)| a.root != b.root || a.domain != b.domain)
        || next.symmetry.is_some()
        || next.matching.is_some()
        || next.profile.is_some()
        || next.pipe.is_some()
    {
        return Err(invalid("remove the refined Weld before replacing its source charts or changing the source type"));
    }
    Ok(())
}

pub(super) fn author(s: &mut Session, p: &Value) -> Result<Value> {
    ready(s)?;
    let args: params::RefinedWeld = parse(p)?;
    let sources = args.features.map(FeatureId);
    let name = args.name.as_deref().unwrap_or("Weld Refined Forms").trim();
    if name.is_empty() || name.len() > 128 || name.contains('\0') {
        return Err(invalid("the refined Weld name must contain 1..128 bytes"));
    }
    if sources.iter().any(|id| attached(&s.doc, id.0)) {
        return Err(invalid("each source Form can have one refined Weld seam"));
    }
    let seam = if let Some(edges) = args.edges {
        crate::doc::forms::RefinedWeldSeam::capture(&s.doc, sources, edges)?
    } else {
        let tolerance = |id: FeatureId| {
            let feature = s.doc.feature(id.0).ok_or_else(|| invalid("refined Weld source Form is missing"))?;
            let FeatureKind::Form { tolerance, .. } = &feature.kind else { return Err(invalid("refined Weld requires saved Forms")) };
            Ok(s.doc.eval(tolerance.as_str(), crate::doc::expr::Kind::Length)?)
        };
        crate::doc::forms::RefinedWeldSeam::capture_aligned(&s.doc, sources, [tolerance(sources[0])?, tolerance(sources[1])?])?
    };
    let state = s.model.state();
    let body_name = |id: FeatureId| {
        if !s.doc.feature(id.0).is_some_and(|feature| feature.component == s.active_component) {
            return Err(invalid("activate the source Forms' component before welding"));
        }
        let mut bodies = state.bodies.iter().filter(|body| body.feature == id.0 && body.component == s.active_component);
        let body = bodies.next().filter(|_| bodies.next().is_none()).ok_or_else(|| invalid("refined Weld source must have one live body"))?;
        Ok(BodyName(body.name.clone()))
    };
    let [a, b] = sources;
    let bodies = vec![body_name(a)?, body_name(b)?];
    let (before_doc, before_model) = (s.doc.clone(), s.model.clone());
    let reply = super::super::features::add_feature(
        s,
        &json!({"name":name}),
        FeatureKind::Stitch { bodies, tolerance: "0".into(), form_seam: Some(Box::new(seam)) },
    )?;
    check_downstream(s, &before_doc, &before_model)?;
    Ok(reply)
}

pub(super) fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    ready(s)?;
    let args: params::RemoveRefinedWeld = parse(p)?;
    let feature = s.doc.feature(args.feature).ok_or_else(|| invalid("the refined Weld feature is missing"))?;
    let FeatureKind::Stitch { bodies, form_seam: Some(seam), .. } = &feature.kind else {
        return Err(invalid("select a refined Weld feature"));
    };
    let at = s.doc.feature_index(feature.id).ok_or_else(|| invalid("the refined Weld is not in the timeline"))?;
    let ids = seam.sources();
    if let Some(dependent) = s.doc.features.iter().skip(at + 1).find(|next| {
        next.kind.names_feature(feature.id, &feature.name)
            || next.kind.input_bodies().iter().any(|name| bodies.iter().any(|body| &body.0 == name))
            || ids.iter().any(|id| s.doc.feature(id.0).is_some_and(|source| next.kind.names_feature(id.0, &source.name)))
            || next
                .face_names
                .iter()
                .chain(&next.edge_names)
                .any(|name| crate::doc::persistent_name::made_by(name).is_some_and(|made| made == feature.id || ids.contains(&FeatureId(made))))
    }) {
        return Err(invalid(format!("remove the refined Weld dependency in `{}` first", dependent.name)));
    }
    let (before_doc, before_model) = (s.doc.clone(), s.model.clone());
    s.doc_mut().delete_feature(args.feature)?;
    s.refresh();
    check_downstream(s, &before_doc, &before_model)?;
    s.selection.clear();
    Ok(json!({"feature":args.feature}))
}

fn ready(s: &Session) -> Result<()> {
    if s.active_sketch.is_some() || s.forms_prototype.is_some() {
        return Err(invalid("finish or cancel the active sketch or Form first"));
    }
    if s.active_occurrence_path.is_some() || !s.pending_moves.is_empty() {
        return Err(invalid("refined Weld requires the source component's direct authoring frame"));
    }
    Ok(())
}

fn check_downstream(s: &Session, document: &Document, model: &crate::doc::Model) -> Result<()> {
    let broken = super::super::edit::newly_broken_in_history(document, model, &s.doc, &s.model);
    if let Some((name, error)) = broken.first() {
        return Err(invalid(format!("refined Weld would break `{name}`: {error}")));
    }
    Ok(())
}

fn invalid(why: impl Into<String>) -> EngineError {
    EngineError::Other(why.into())
}
