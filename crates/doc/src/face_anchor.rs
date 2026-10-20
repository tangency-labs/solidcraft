//! Exact ordinary face attachments. Missing identities never invoke a proximity fallback; the one
//! fallback is an extrude cap whose sketch loop changed (`Document::anchored_face_indices`).
use crate::config::ConfigFaceTarget;
use crate::{DocError, Document, EmbossFace, FeatureKind, Model, ModelBody, ModelState, PlaneRef, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_geom::Plane;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceAnchor {
    pub target: ConfigFaceTarget,
    pub component: u64,
    pub path: Vec<u64>,
    pub author_path: Vec<u64>,
    /// Origin and x direction in the exact canonical face plane's coordinates.
    pub coordinates: [f64; 2],
    pub x_axis: [f64; 2],
}

impl FaceAnchor {
    pub fn remap_target(
        &mut self,
        features: &std::collections::BTreeMap<u64, u64>,
        bodies: &std::collections::BTreeMap<String, String>,
    ) -> Result<()> {
        if bodies.contains_key(&self.target.body) && !features.contains_key(&self.target.origin_feature) {
            return Err(DocError::Invalid("cannot clone an attached face without its certified producer".into()));
        }
        self.target.name = crate::remap_topology(&self.target.name, features, bodies);
        if let Some(body) = bodies.get(&self.target.body) {
            self.target.body = body.clone();
        }
        if let Some(feature) = features.get(&self.target.origin_feature) {
            self.target.origin_feature = *feature;
        }
        if let Some(certificate) = &mut self.target.certificate {
            certificate.remap_features(features);
        }
        self.validate()
    }
    pub fn validate(&self) -> Result<()> {
        self.target.validate()?;
        if self.target.certificate.is_none()
            || self.path.len() > 64
            || self.author_path.len() > 64
            || self.path.iter().chain(&self.author_path).any(|id| *id == 0)
            || (self.component == 0 && !self.path.is_empty())
            || (self.component != 0 && self.path.is_empty())
            || self.coordinates.iter().any(|value| !value.is_finite() || value.abs() > 1e9)
            || self.x_axis.iter().any(|value| !value.is_finite())
            || (self.x_axis[0].hypot(self.x_axis[1]) - 1.).abs() > 1e-6
        {
            return Err(DocError::Invalid("invalid exact face attachment".into()));
        }
        Ok(())
    }
    pub fn author_frame(&self, doc: &Document, author: u64) -> Result<crate::Mat> {
        if author != 0 && self.author_path.is_empty() {
            return Err(DocError::Invalid(
                "attached sketch requires an explicit author occurrence path; activate a placement or choose a fixed plane".into(),
            ));
        }
        EmbossFace::component_frame(doc, author, &self.author_path)
    }
    pub fn source_to_author(&self, doc: &Document, author: u64, body: &ModelBody) -> Result<crate::Mat> {
        self.validate()?;
        if doc.model_body_component(body) != self.component {
            return Err(DocError::Invalid("attached face's component ownership changed; reattach explicitly".into()));
        }
        let inverse = crate::mat_inverse(&self.author_frame(doc, author)?)
            .ok_or_else(|| DocError::Invalid("attached sketch author frame is invalid".into()))?;
        let source = EmbossFace {
            body: self.target.body.clone().into(),
            at: crate::FacePick(self.target.point),
            path: self.path.clone(),
            author_path: self.author_path.clone(),
        };
        Ok(crate::mat_mul(&inverse, &source.body_frame(doc, body)?))
    }
}

pub fn plane_face_anchor(plane: &PlaneRef) -> Option<&FaceAnchor> {
    match plane {
        PlaneRef::Face { anchor, .. } => anchor.as_deref(),
        PlaneRef::Offset { base, .. } | PlaneRef::ThroughPoint { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => {
            plane_face_anchor(base)
        }
        _ => None,
    }
}

fn has_face_reference(plane: &PlaneRef) -> bool {
    match plane {
        PlaneRef::Face { .. } => true,
        PlaneRef::Offset { base, .. } | PlaneRef::ThroughPoint { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => {
            has_face_reference(base)
        }
        _ => false,
    }
}

impl Document {
    pub fn resolve_attached_plane(
        &self,
        vals: &std::collections::BTreeMap<String, crate::expr::Value>,
        plane: &PlaneRef,
        author: u64,
        state: &ModelState,
        framed: &ModelState,
    ) -> Result<Plane> {
        self.resolve_attached_plane_noted(vals, plane, author, state, framed).map(|(plane, _)| plane)
    }

    /// [`Document::resolve_attached_plane`] with the note of an attachment found by fallback
    /// ([`Document::anchored_face_indices`]).
    pub fn resolve_attached_plane_noted(
        &self,
        vals: &std::collections::BTreeMap<String, crate::expr::Value>,
        plane: &PlaneRef,
        author: u64,
        state: &ModelState,
        framed: &ModelState,
    ) -> Result<(Plane, Option<String>)> {
        fn replace(doc: &Document, plane: &mut PlaneRef, author: u64, state: &ModelState, depth: usize) -> Result<Option<String>> {
            if depth > 64 {
                return Err(DocError::Invalid("attached plane nesting exceeds budget".into()));
            }
            match plane {
                PlaneRef::Face { anchor: Some(anchor), .. } => {
                    let (resolved, note) = doc.resolve_face_anchor_noted(anchor, author, state)?;
                    *plane = PlaneRef::Custom { plane: resolved };
                    Ok(note)
                }
                PlaneRef::Offset { base, .. }
                | PlaneRef::ThroughPoint { base, .. }
                | PlaneRef::AtAngle { base, .. }
                | PlaneRef::Flipped { base, .. } => replace(doc, base, author, state, depth + 1),
                _ => Ok(None),
            }
        }
        let mut resolved = plane.clone();
        let note = replace(self, &mut resolved, author, state, 0)?;
        Ok((self.resolve_plane_in(vals, &resolved, 0, Some(framed))?, note))
    }
}

/// All status planes are world-space. A lost plane is a repair guide only. `Sheet` is a sketch on
/// a flat face of a sheet metal body, attached by the name of its panel (`sheet::sheet_face_name`).
#[derive(Clone, Debug)]
pub enum FaceAttachmentStatus {
    Fixed { plane: Option<Plane> },
    Legacy { plane: Option<Plane> },
    Sheet { plane: Option<Plane> },
    Healthy { plane: Plane, target: ConfigFaceTarget, path: Vec<u64> },
    Lost { message: String, repair_plane: Option<Plane> },
}

fn map_plane(matrix: &crate::Mat, plane: &Plane) -> Result<Plane> {
    Plane::new(crate::apply_point(matrix, plane.origin), crate::apply_vector(matrix, plane.x), crate::apply_vector(matrix, plane.y))
        .ok_or_else(|| DocError::Invalid("attached plane transform is degenerate".into()))
}

impl Document {
    /// Capture a known selected face in the definition model, without another nearest pick.
    pub fn capture_face_anchor(
        &self,
        body: &ModelBody,
        index: usize,
        picked: &Plane,
        target_path: &[u64],
        author: u64,
        author_path: &[u64],
    ) -> Result<FaceAnchor> {
        // A construction certificate that names this face alone, else its exact history name.
        let certified = body
            .body
            .face_plane(index)
            .ok_or_else(|| DocError::Invalid("this face has no exact planar source; choose a fixed plane instead".into()))
            .and_then(|source| {
                let target = self.strong_face_target(body, index, source.origin)?;
                if target.certificate.is_none() {
                    return Err(DocError::Invalid("this face has no supported construction certificate; choose a fixed plane instead".into()));
                }
                let indices = self.configuration_face_indices(body, &target)?;
                self.coplanar_pieces(body, &indices, |_| true)?;
                Ok(target)
            });
        let target = match certified {
            Ok(target) => target,
            Err(error) => match body.body.planar_face(index).and_then(|plane| self.lineage_face_target(body, index, plane.origin)) {
                Some(target) => target,
                None => return Err(error),
            },
        };
        let source = self
            .anchor_face_plane(body, index, target.certificate.as_ref())
            .ok_or_else(|| DocError::Invalid("this face has no exact planar source; choose a fixed plane instead".into()))?;
        let mut anchor = FaceAnchor {
            target,
            component: self.model_body_component(body),
            path: target_path.to_vec(),
            author_path: author_path.to_vec(),
            coordinates: [0., 0.],
            x_axis: [1., 0.],
        };
        let canonical = map_plane(&anchor.source_to_author(self, author, body)?, &source)?;
        if picked.normal().dot(canonical.normal()) < 1. - 1e-7 || (picked.origin - canonical.origin).dot(canonical.normal()).abs() > 1e-7 {
            return Err(DocError::Invalid("picked plane does not match its exact source face".into()));
        }
        let delta = picked.origin - canonical.origin;
        anchor.coordinates = [delta.dot(canonical.x), delta.dot(canonical.y)];
        anchor.x_axis = [picked.x.dot(canonical.x), picked.x.dot(canonical.y)];
        anchor.validate()?;
        Ok(anchor)
    }

    /// The faces an attachment stands for, and a note when they were found by fallback.
    ///
    /// An extrude's cap is certified by the loop of sketch curves around it, so editing that
    /// sketch's outline (a curve that now cuts the profile, a side deleted and drawn again, a
    /// notch trimmed into it) leaves no face with the exact certificate. The attachment then
    /// takes the cap of the same (solid) extrude, at the same end, from the same sketch, on the
    /// same body, whose loop shares the most of the stored curves, at least half of them (a tie
    /// is not guessed). It never does while the exact cap is still on another body (profiles
    /// reordered: the bodies swapped names). The note names what changed. Every other identity
    /// stays exact.
    pub(crate) fn anchored_face_indices(
        &self,
        state: &ModelState,
        body: &ModelBody,
        target: &ConfigFaceTarget,
    ) -> Result<(Vec<usize>, Option<String>)> {
        let error = match self.configuration_face_indices(body, target) {
            Ok(indices) => return Ok((indices, None)),
            Err(error) => error,
        };
        let Some(crate::config::FaceCertificate::ExtrudeCap { sketch, high, outer, holes }) = &target.certificate else { return Err(error) };
        let exact = |source: &Option<solidcraft_kernel::FaceSource>| {
            matches!(source, Some(solidcraft_kernel::FaceSource::CertifiedExtrudeCap { producer, sketch: from, high: end, outer: around, holes: inside, .. })
                if *producer == target.origin_feature && from == sketch && end == high && around == outer && inside == holes)
        };
        if state.bodies.iter().any(|other| other.body.face_sources().is_some_and(|sources| sources.iter().any(exact))) {
            return Err(error);
        }
        // A thin wall's cap is signed by the curves the wall follows: a wall along other curves
        // is another wall, whose cap never stands for this one.
        let solid = self.feature(target.origin_feature).is_some_and(|feature| matches!(feature.kind, FeatureKind::Extrude { thin: None, .. }));
        if !solid || self.certify_config_face(target).is_err() || body.body.is_mesh() || body.definition_name() != target.body {
            return Err(error);
        }
        // The producer's caps at that end, grouped by the loop each was made from.
        let mut caps: std::collections::BTreeMap<&Vec<String>, Vec<usize>> = std::collections::BTreeMap::new();
        for (index, source) in body.body.face_sources().into_iter().flatten().enumerate() {
            if let Some(solidcraft_kernel::FaceSource::CertifiedExtrudeCap { producer, sketch: from, high: end, outer: around, .. }) = source
                && *producer == target.origin_feature
                && from == sketch
                && end == high
            {
                caps.entry(around).or_default().push(index);
            }
        }
        let stored: std::collections::BTreeSet<&str> = outer.iter().map(String::as_str).collect();
        let shared = |around: &[String]| around.iter().filter(|id| stored.contains(id.as_str())).count();
        let best = caps.keys().map(|around| shared(around)).max().unwrap_or(0);
        if best == 0 || best.saturating_mul(2) < stored.len() {
            return Err(error);
        }
        let mut chosen = caps.iter().filter(|(around, _)| shared(around) == best);
        let (Some((around, indices)), None) = (chosen.next(), chosen.next()) else { return Err(error) };
        let indices = self.certified_pieces(body, indices.clone(), target.origin_feature).map_err(|_| error)?;
        let now: std::collections::BTreeSet<&str> = around.iter().map(String::as_str).collect();
        let list = |ids: Vec<&str>| ids.iter().map(|id| format!("`{id}`")).collect::<Vec<_>>().join(", ");
        let mut changes = Vec::new();
        let gone: Vec<&str> = stored.difference(&now).copied().collect();
        if !gone.is_empty() {
            changes.push(format!("{} no longer around it", list(gone)));
        }
        let new: Vec<&str> = now.difference(&stored).copied().collect();
        if !new.is_empty() {
            changes.push(format!("{} new", list(new)));
        }
        let feature = self.feature(target.origin_feature).map_or_else(|| format!("F{}", target.origin_feature), |f| f.name.clone());
        let note = format!(
            "the face this sketch sits on, {}, changed when its sketch was edited ({}); using the {} cap of {feature} as it is now",
            self.describe_face_target(target),
            changes.join(", "),
            if *high { "top" } else { "bottom" },
        );
        Ok((indices, Some(note)))
    }

    pub fn resolve_face_anchor(&self, anchor: &FaceAnchor, author: u64, state: &ModelState) -> Result<Plane> {
        self.resolve_face_anchor_noted(anchor, author, state).map(|(plane, _)| plane)
    }

    /// [`Document::resolve_face_anchor`] with the note of an attachment found by fallback
    /// ([`Document::anchored_face_indices`]).
    pub fn resolve_face_anchor_noted(&self, anchor: &FaceAnchor, author: u64, state: &ModelState) -> Result<(Plane, Option<String>)> {
        anchor.validate()?;
        let body =
            state.body(&anchor.target.body).ok_or_else(|| DocError::Unknown(format!("attached body `{}` no longer exists", anchor.target.body)))?;
        let (indices, note) = self.anchored_face_indices(state, body, &anchor.target)?;
        let first = indices.first().copied().ok_or_else(|| DocError::Unknown("attached face no longer exists".into()))?;
        let certificate = anchor.target.certificate.as_ref();
        let plane = self
            .anchor_face_plane(body, first, certificate)
            .ok_or_else(|| DocError::Invalid("attached face no longer has an exact planar source".into()))?;
        for index in indices.iter().skip(1) {
            let other = self
                .anchor_face_plane(body, *index, certificate)
                .ok_or_else(|| DocError::Invalid("attached face pieces are not exactly planar".into()))?;
            if plane.normal().dot(other.normal()) < 1. - 1e-8 || (other.origin - plane.origin).dot(plane.normal()).abs() > 1e-7 {
                return Err(DocError::Invalid("attached face pieces no longer share one oriented plane".into()));
            }
        }
        let canonical = map_plane(&anchor.source_to_author(self, author, body)?, &plane)?;
        let origin = canonical.origin + canonical.x * anchor.coordinates[0] + canonical.y * anchor.coordinates[1];
        let x = canonical.x * anchor.x_axis[0] + canonical.y * anchor.x_axis[1];
        let plane =
            Plane::new(origin, x, canonical.normal().cross(x)).ok_or_else(|| DocError::Invalid("attached sketch basis is degenerate".into()))?;
        Ok((plane, note))
    }

    pub fn sketch_face_attachment(&self, feature: u64, model: &Model) -> FaceAttachmentStatus {
        let Some(feature) = self.feature(feature) else {
            return FaceAttachmentStatus::Lost { message: "sketch no longer exists".into(), repair_plane: None };
        };
        let FeatureKind::Sketch { plane, .. } = &feature.kind else {
            return FaceAttachmentStatus::Fixed { plane: None };
        };
        let Some(anchor) = plane_face_anchor(plane) else {
            let path = self.first_path(feature.component);
            let unique = path.iter().all(|id| {
                self.occurrences
                    .iter()
                    .find(|o| o.id == *id)
                    .is_some_and(|selected| self.occurrences.iter().filter(|o| o.component == selected.component).count() == 1)
            });
            let world = unique.then(|| EmbossFace::component_frame(self, feature.component, &path).ok()).flatten();
            let current = model.state().sketch(feature.id).map(|sketch| sketch.plane).or_else(|| {
                let (vals, _) = self.param_values();
                self.resolve_plane(&vals, plane, 0).ok()
            });
            let current = world.and_then(|world| current.and_then(|plane| map_plane(&world, &plane).ok()));
            if crate::sheet::plane_sheet_face(plane).is_some() {
                let state = model.state_before(feature.id);
                return match crate::eval::sheet_face_plane_in_frame(self, &state, feature.component, plane) {
                    Ok(_) => FaceAttachmentStatus::Sheet { plane: current },
                    Err(error) => FaceAttachmentStatus::Lost { message: error.to_string(), repair_plane: current },
                };
            }
            return if has_face_reference(plane) {
                FaceAttachmentStatus::Legacy { plane: current }
            } else {
                FaceAttachmentStatus::Fixed { plane: current }
            };
        };
        let (vals, _) = self.param_values();
        let picked = self.resolve_plane(&vals, plane, 0).ok();
        let world = anchor.author_frame(self, feature.component);
        let repair_plane = world.as_ref().ok().and_then(|matrix| picked.as_ref().and_then(|picked| map_plane(matrix, picked).ok()));
        let state = model.state_before(feature.id);
        match crate::state_in_face_anchor_frame(self, &state, anchor, feature.component)
            .and_then(|framed| self.resolve_attached_plane(&vals, plane, feature.component, &state, &framed))
            .and_then(|plane| map_plane(&world?, &plane))
        {
            Ok(plane) => FaceAttachmentStatus::Healthy { plane, target: anchor.target.clone(), path: anchor.path.clone() },
            Err(error) => FaceAttachmentStatus::Lost { message: format!("lost face attachment on `{}`: {error}", anchor.target.body), repair_plane },
        }
    }
}

pub fn validate_face_anchor_json(value: &Value) -> Result<()> {
    let fields = value.as_object().ok_or_else(|| DocError::Invalid("face anchor must be an object".into()))?;
    if fields.len() != 6 || fields.keys().any(|key| !["target", "component", "path", "author_path", "coordinates", "x_axis"].contains(&key.as_str()))
    {
        return Err(DocError::Invalid("unknown or missing face anchor fields".into()));
    }
    let target = fields.get("target").ok_or_else(|| DocError::Invalid("face anchor target missing".into()))?;
    crate::config::validate_face_target_json(target)?;
    if target.get("certificate").is_none_or(Value::is_null) {
        return Err(DocError::Invalid("face anchor needs a source certificate".into()));
    }
    let component =
        fields.get("component").and_then(Value::as_u64).ok_or_else(|| DocError::Invalid("face anchor component must be an integer".into()))?;
    for key in ["path", "author_path"] {
        if fields.get(key).and_then(Value::as_array).is_none_or(|path| {
            path.len() > 64
                || path.iter().any(|id| id.as_u64().is_none_or(|id| id == 0))
                || (key == "path" && component == 0 && !path.is_empty())
                || (key == "path" && component != 0 && path.is_empty())
        }) {
            return Err(DocError::Invalid("face anchor path is invalid or oversized".into()));
        }
    }
    for key in ["coordinates", "x_axis"] {
        let values = fields
            .get(key)
            .and_then(Value::as_array)
            .filter(|values| values.len() == 2)
            .ok_or_else(|| DocError::Invalid("face anchor vectors require exactly two values".into()))?;
        if values.iter().any(|value| value.as_f64().is_none_or(|value| !value.is_finite() || value.abs() > 1e9)) {
            return Err(DocError::Invalid("face anchor vector is invalid".into()));
        }
        if key == "x_axis" && (values[0].as_f64().unwrap_or(0.).hypot(values[1].as_f64().unwrap_or(0.)) - 1.).abs() > 1e-6 {
            return Err(DocError::Invalid("face anchor basis must be normalized".into()));
        }
    }
    Ok(())
}

/// Walk reference-bearing JSON before typed decoding, excluding bulk geometry payloads.
pub fn validate_face_anchors_json(value: &Value) -> Result<()> {
    let mut stack = vec![(value, 0usize)];
    let mut visited = 0usize;
    while let Some((value, depth)) = stack.pop() {
        visited += 1;
        if visited > 2_000_000 || depth > 128 {
            return Err(DocError::Invalid("face anchor structure exceeds work limits".into()));
        }
        match value {
            Value::Object(fields) => {
                if fields.get("type").and_then(Value::as_str) == Some("face")
                    && let Some(anchor) = fields.get("anchor")
                    && !anchor.is_null()
                {
                    validate_face_anchor_json(anchor)?;
                }
                for (key, child) in fields {
                    if !["sketch", "meshes", "step", "configs", "appearances", "materials"].contains(&key.as_str())
                        && matches!(child, Value::Array(_) | Value::Object(_))
                    {
                        stack.push((child, depth + 1));
                    }
                }
            }
            Value::Array(values) => {
                for child in values {
                    if matches!(child, Value::Array(_) | Value::Object(_)) {
                        stack.push((child, depth + 1));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec3;
    fn fixture() -> (Document, ModelBody, usize, Plane) {
        let mut doc = Document::default();
        let kind: FeatureKind =
            serde_json::from_value(serde_json::json!({"type":"box","corner":[0,0,0],"length":"10","width":"10","height":"10"})).unwrap();
        let id = doc.add_feature(kind, Some("Base")).unwrap();
        let body = ModelBody::new(
            "Base".into(),
            solidcraft_kernel::box_solid(Vec3::default(), Vec3::new(10., 10., 10.)).unwrap().with_box_certificate(id).unwrap(),
            id,
        );
        let index = body
            .body
            .face_sources()
            .unwrap()
            .iter()
            .position(|source| matches!(source, Some(solidcraft_kernel::FaceSource::Box { axis: 2, high: true, .. })))
            .unwrap();
        let canonical = body.body.face_plane(index).unwrap();
        let picked = Plane::new(canonical.origin + canonical.x * 2. + canonical.y * 3., canonical.y, -canonical.x).unwrap();
        (doc, body, index, picked)
    }
    #[test]
    fn exact_anchor_preserves_picked_basis_and_follows_height_without_parallel_rescue() {
        let (doc, body, index, picked) = fixture();
        let anchor = doc.capture_face_anchor(&body, index, &picked, &[], 0, &[]).unwrap();
        let anchor: FaceAnchor = serde_json::from_str(&serde_json::to_string(&anchor).unwrap()).unwrap();
        assert!(anchor.author_frame(&doc, 1).is_err(), "strong author attachment cannot infer a first placement");
        let mut missing_source = anchor.clone();
        missing_source.component = 1;
        assert!(missing_source.validate().is_err(), "strong source attachment needs an explicit path");
        assert!(validate_face_anchor_json(&serde_json::to_value(missing_source).unwrap()).is_err());
        let mut state = ModelState::default();
        state.bodies.push(body);
        let initial = doc.resolve_face_anchor(&anchor, 0, &state).unwrap();
        assert!((initial.origin - picked.origin).len() < 1e-9);
        assert!((initial.x - picked.x).len() < 1e-9);
        state.bodies[0] = ModelBody::new(
            "Base".into(),
            solidcraft_kernel::box_solid(Vec3::default(), Vec3::new(10., 10., 20.))
                .unwrap()
                .with_box_certificate(anchor.target.origin_feature)
                .unwrap(),
            anchor.target.origin_feature,
        );
        let changed = doc.resolve_face_anchor(&anchor, 0, &state).unwrap();
        assert!((changed.origin.z - 20.).abs() < 1e-9);
        assert!((changed.origin.x - initial.origin.x).abs() < 1e-9);
        state.bodies[0] = ModelBody::new(
            "Control".into(),
            solidcraft_kernel::box_solid(Vec3::default(), Vec3::new(10., 10., 30.))
                .unwrap()
                .with_box_certificate(anchor.target.origin_feature)
                .unwrap(),
            anchor.target.origin_feature,
        );
        assert!(doc.resolve_face_anchor(&anchor, 0, &state).is_err());
        state.bodies[0].name = "Base".into();
        state.bodies[0].body = solidcraft_kernel::box_solid(Vec3::default(), Vec3::new(10., 10., 30.))
            .unwrap()
            .with_box_certificate(anchor.target.origin_feature + 1)
            .unwrap();
        assert!(doc.resolve_face_anchor(&anchor, 0, &state).is_err(), "reused name cannot replace the producer");
    }
    #[test]
    fn nested_anchor_offset_tracks_source_and_raw_budgets_precede_decode() {
        let (doc, body, index, picked) = fixture();
        let anchor = doc.capture_face_anchor(&body, index, &picked, &[], 0, &[]).unwrap();
        let plane = PlaneRef::Offset {
            base: Box::new(PlaneRef::Face { plane: picked, at: picked.origin, name: None, anchor: Some(Box::new(anchor.clone())) }),
            distance: "2".into(),
        };
        let mut state = ModelState::default();
        state.bodies.push(body);
        let (vals, _) = doc.param_values();
        let result = doc.resolve_attached_plane(&vals, &plane, 0, &state, &state).unwrap();
        assert!((result.origin.z - 12.).abs() < 1e-9);
        state.bodies.clear();
        assert!(doc.resolve_attached_plane(&vals, &plane, 0, &state, &state).is_err());
        let mut raw = serde_json::to_value(anchor).unwrap();
        assert!(validate_face_anchor_json(&raw).is_ok());
        raw["path"] = serde_json::json!([1]);
        assert!(validate_face_anchor_json(&raw).is_err());
        raw["path"] = serde_json::json!([]);
        raw["target"]["name"] = Value::String("x".repeat(1_000_000));
        assert!(validate_face_anchor_json(&raw).is_err());
    }
    #[test]
    fn extrude_cap_profile_reordering_never_retargets_an_ordinal_body() {
        use solidcraft_geom::Vec2;
        let mut sketch = solidcraft_sketch::Sketch::new();
        for offset in [0., 30.] {
            let points: Vec<_> = [(0., 0.), (10., 0.), (10., 10.), (0., 10.)]
                .iter()
                .map(|(x, y)| sketch.add_point(Vec2::new(x + offset, *y), None).unwrap())
                .collect();
            for edge in 0..4 {
                sketch.add_line_pts(points[edge], points[(edge + 1) % 4], None).unwrap();
            }
        }
        let mut doc = Document::default();
        let source = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let extent: crate::Extent = serde_json::from_value(serde_json::json!({"distance":"10"})).unwrap();
        let producer = doc
            .add_feature(
                FeatureKind::Extrude {
                    thin: None,
                    sketch: crate::SketchId(source),
                    profiles: crate::ProfileSel::Indices { indices: vec![0, 1] },
                    extent,
                    operation: crate::Operation::NewBody,
                    targets: vec![],
                },
                None,
            )
            .unwrap();
        let mut model = Model::new();
        model.evaluate(&doc);
        let state = model.state();
        assert_eq!(state.bodies.len(), 2);
        let solved = state.sketch(source).unwrap();
        let regions = crate::select_profiles(solved, &crate::ProfileSel::All).unwrap();
        let raw = solidcraft_kernel::extrude(&solved.plane, &regions, 0., 10.).unwrap();
        assert!(
            crate::config::extrude_cap_bindings(solved, &raw, 0).is_none(),
            "exhausted certification work must leave the complete producer uncertified"
        );
        let bindings = crate::config::extrude_cap_bindings(solved, &raw, 10_000_000).unwrap();
        assert_eq!(bindings.iter().flatten().flatten().count(), 4, "two profiles each retain both exact caps within budget");
        let mut duplicate = raw.clone();
        duplicate.extend(raw);
        let ambiguous = crate::config::extrude_cap_bindings(solved, &duplicate, 10_000_000).unwrap();
        assert!(ambiguous.iter().flatten().all(Option::is_none), "duplicated profile identities cannot stamp ambiguous caps");
        let body = &state.bodies[0];
        let index = body
            .body
            .face_sources()
            .unwrap()
            .iter()
            .position(|source| matches!(source, Some(solidcraft_kernel::FaceSource::CertifiedExtrudeCap { high: true, .. })))
            .unwrap();
        let picked = body.body.face_plane(index).unwrap();
        let anchor = doc.capture_face_anchor(body, index, &picked, &[], 0, &[]).unwrap();
        let bounds = body.mesh().bounds();
        let FeatureKind::Extrude { profiles, .. } = &mut doc.feature_mut(producer).unwrap().kind else {
            panic!("fixture must retain its extrude feature")
        };
        *profiles = crate::ProfileSel::Indices { indices: vec![1, 0] };
        model.evaluate(&doc);
        let changed = model.state();
        assert_eq!(changed.bodies.len(), 2);
        if doc.resolve_face_anchor(&anchor, 0, &changed).is_ok() {
            let current = changed.body(&anchor.target.body).unwrap().mesh().bounds();
            assert!((current.min.x - bounds.min.x).abs() < 1e-9, "a same-role cap of the other profile must never replace the source");
        }
    }
    #[test]
    fn wrapped_legacy_face_is_reported_as_legacy_without_inferred_upgrade() {
        let (mut doc, _, _, picked) = fixture();
        let id = doc
            .add_feature(
                FeatureKind::Sketch {
                    plane: PlaneRef::Offset {
                        base: Box::new(PlaneRef::Face { plane: picked, at: picked.origin, name: None, anchor: None }),
                        distance: "2".into(),
                    },
                    sketch: solidcraft_sketch::Sketch::new(),
                },
                None,
            )
            .unwrap();
        let mut model = Model::new();
        model.evaluate(&doc);
        let FaceAttachmentStatus::Legacy { plane: Some(plane) } = doc.sketch_face_attachment(id, &model) else {
            panic!("wrapped legacy attachment must retain its classification and fixed-plane escape");
        };
        assert!((plane.origin.z - 12.).abs() < 1e-9);
        let FeatureKind::Sketch { plane, .. } = &doc.feature(id).unwrap().kind else { panic!("fixture must retain its sketch feature") };
        assert!(plane_face_anchor(plane).is_none());
    }
    #[test]
    fn warm_root_body_offset_edit_matches_cold_attached_sketch_and_downstream_bounds() {
        use solidcraft_geom::Vec2;
        let (mut doc, _, _, _) = fixture();
        let mut warm = Model::new();
        warm.evaluate(&doc);
        let source = warm.state();
        let body = &source.bodies[0];
        let index = body
            .body
            .face_sources()
            .unwrap()
            .iter()
            .position(|source| matches!(source, Some(solidcraft_kernel::FaceSource::Box { axis: 2, high: true, .. })))
            .unwrap();
        let picked = body.body.face_plane(index).unwrap();
        let anchor = doc.capture_face_anchor(body, index, &picked, &[], 0, &[]).unwrap();
        let source_name = body.name.clone();
        let mut sketch = solidcraft_sketch::Sketch::new();
        let points: Vec<_> =
            [(2., 2.), (4., 2.), (4., 4.), (2., 4.)].iter().map(|(x, y)| sketch.add_point(Vec2::new(*x, *y), None).unwrap()).collect();
        for edge in 0..4 {
            sketch.add_line_pts(points[edge], points[(edge + 1) % 4], None).unwrap();
        }
        let sk = doc
            .add_feature(
                FeatureKind::Sketch {
                    plane: PlaneRef::Face { plane: picked, at: picked.origin, name: None, anchor: Some(Box::new(anchor)) },
                    sketch,
                },
                None,
            )
            .unwrap();
        let extent = serde_json::from_value(serde_json::json!({"distance":"2"})).unwrap();
        let boss = doc
            .add_feature(
                FeatureKind::Extrude {
                    thin: None,
                    sketch: crate::SketchId(sk),
                    profiles: crate::ProfileSel::All,
                    extent,
                    operation: crate::Operation::NewBody,
                    targets: vec![],
                },
                Some("Boss"),
            )
            .unwrap();
        warm.evaluate(&doc);
        let initial = warm.state();
        let before = initial.bodies.iter().find(|body| body.feature == boss).unwrap().mesh().bounds();
        let mut offset = crate::IDENTITY;
        offset[3][0] = 5.;
        offset[3][2] = 3.;
        doc.body_offsets.insert(source_name, offset);
        warm.evaluate(&doc);
        let mut cold = Model::new();
        cold.evaluate(&doc);
        let warm_state = warm.state();
        let cold_state = cold.state();
        let after = warm_state.bodies.iter().find(|body| body.feature == boss).unwrap().mesh().bounds();
        let rebuilt = cold_state.bodies.iter().find(|body| body.feature == boss).unwrap().mesh().bounds();
        assert!(
            (after.min - before.min - Vec3::new(5., 0., 3.)).len() < 1e-7,
            "before={before:?}, warm={after:?}, cold={rebuilt:?}, warm_plane={:?}, cold_plane={:?}",
            warm_state.sketch(sk).map(|sk| sk.plane),
            cold_state.sketch(sk).map(|sk| sk.plane)
        );
        assert!((after.min - rebuilt.min).len() < 1e-7);
        assert!((after.max - rebuilt.max).len() < 1e-7);
        let FaceAttachmentStatus::Healthy { plane, .. } = doc.sketch_face_attachment(sk, &warm) else {
            panic!("offset source must remain attached");
        };
        assert!((plane.origin - warm_state.sketch(sk).unwrap().plane.origin).len() < 1e-9);
        assert!((warm_state.sketch(sk).unwrap().plane.origin - cold_state.sketch(sk).unwrap().plane.origin).len() < 1e-9);
    }
}
