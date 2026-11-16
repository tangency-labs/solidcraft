//! Retained bend identities and cut-end choices for a native sheet-metal feature.
//!
//! A bend refers to its producing feature and local index, rather than its position in a flat
//! pattern. The feature reference participates in copy/import remapping through `Walk`.

use serde::{Deserialize, Serialize};

use crate::walk::Walk;
use crate::{DocError, FeatureId, Result};

/// A captured bend selection. Its original tag is `F<feature>.<index>`; the
/// structural certificate resolves its physical child to the current sheet tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
pub struct SheetBendRef {
    pub feature: FeatureId,
    pub index: u32,
    /// Captured topology size. Dimensional edits retain it; a different count requires review
    /// of the explicit selection before its local indices can be used again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_bend_count: Option<usize>,
    /// Structural source identity captured only by an explicit reviewed selection.
    /// Older experimental references without it require selection review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_certificate: Option<BendSlotSourceCertificate>,
}

/// Bounded source topology, independent of coordinates and dimensional expressions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct BendSlotSourceCertificate {
    body: crate::BodyName,
    origin: FeatureId,
    path: Vec<BendSourceStep>,
    producers: Vec<BendSourceProducer>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct BendSourceStep {
    producer: FeatureId,
    child: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct BendSourceProducer {
    feature: FeatureId,
    kind: String,
    edge_names: Vec<String>,
    face_names: Vec<String>,
    sketches: Vec<crate::SketchId>,
    bodies: Vec<crate::BodyName>,
    topology: Vec<String>,
    curves: Vec<BendSourceCurve>,
    links: Vec<BendSourceLink>,
    patterns: Vec<BendSourcePattern>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct BendSourceLink {
    id: String,
    kind: String,
    body: Option<crate::BodyName>,
    sketch: Option<crate::SketchId>,
    entity: String,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct BendSourcePattern {
    id: String,
    kind: String,
    entities: Vec<String>,
    instances: Vec<BendPatternInstance>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct BendPatternInstance {
    index: [usize; 2],
    curves: Vec<String>,
    points: Vec<[String; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
struct BendSourceCurve {
    id: String,
    kind: String,
    points: Vec<String>,
    construction: bool,
    reversed: bool,
    infinite: bool,
    centerline: bool,
    link: Option<String>,
}

struct SourceBudget {
    records: usize,
    bytes: usize,
}

impl SourceBudget {
    fn new() -> Self {
        Self { records: 2048, bytes: 128 * 1024 }
    }
    fn records(&mut self, count: usize) -> Result<()> {
        self.records = self.records.checked_sub(count).ok_or_else(review)?;
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<()> {
        if text.len() > 4096 {
            return Err(review());
        }
        self.bytes = self.bytes.checked_sub(text.len()).ok_or_else(review)?;
        Ok(())
    }
}

impl BendSlotSourceCertificate {
    fn validate_shape(&self) -> Result<()> {
        if self.origin.0 == 0 || self.path.is_empty() || self.path.len() > 64 || self.producers.is_empty() || self.producers.len() > 64 {
            return Err(review());
        }
        let mut budget = SourceBudget::new();
        budget.text(self.body.as_ref())?;
        budget.records(self.path.len().saturating_add(self.producers.len()))?;
        if self.path.iter().any(|step| step.producer.0 == 0 || step.child.len() != 1) {
            return Err(review());
        }
        for producer in &self.producers {
            if producer.feature.0 == 0
                || producer.edge_names.len() > 256
                || producer.face_names.len() > 256
                || producer.curves.len() > 2000
                || producer.sketches.len() > 64
                || producer.bodies.len() > 64
                || producer.topology.len() > 256
            {
                return Err(review());
            }
            budget.records(
                producer
                    .curves
                    .len()
                    .saturating_add(producer.edge_names.len())
                    .saturating_add(producer.face_names.len())
                    .saturating_add(producer.sketches.len())
                    .saturating_add(producer.bodies.len())
                    .saturating_add(producer.topology.len()),
            )?;
            budget.text(&producer.kind)?;
            for text in producer.edge_names.iter().chain(&producer.face_names).chain(&producer.topology) {
                budget.text(text)?;
            }
            for body in &producer.bodies {
                budget.text(body.as_ref())?;
            }
            for curve in &producer.curves {
                if curve.points.len() > 128 {
                    return Err(review());
                }
                budget.records(curve.points.len())?;
                budget.text(&curve.id)?;
                budget.text(&curve.kind)?;
                for point in &curve.points {
                    budget.text(point)?;
                }
                if let Some(link) = &curve.link {
                    budget.text(link)?;
                }
            }
            if producer.links.len() > 128 || producer.patterns.len() > 128 {
                return Err(review());
            }
            budget.records(producer.links.len().saturating_add(producer.patterns.len()))?;
            for link in &producer.links {
                for text in [&link.id, &link.kind, &link.entity, &link.name] {
                    budget.text(text)?;
                }
                if let Some(body) = &link.body {
                    budget.text(body.as_ref())?;
                }
            }
            for pattern in &producer.patterns {
                if pattern.entities.len() > 2000 || pattern.instances.len() > 500 {
                    return Err(review());
                }
                budget.text(&pattern.id)?;
                budget.text(&pattern.kind)?;
                budget.records(pattern.entities.len().saturating_add(pattern.instances.len()))?;
                for entity in &pattern.entities {
                    budget.text(entity)?;
                }
                for instance in &pattern.instances {
                    if instance.curves.len() > 2000 || instance.points.len() > 2000 {
                        return Err(review());
                    }
                    budget.records(instance.curves.len().saturating_add(instance.points.len().saturating_mul(2)))?;
                    for text in instance.curves.iter().chain(instance.points.iter().flatten()) {
                        budget.text(text)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn remap_names(&mut self, features: &std::collections::BTreeMap<u64, u64>, bodies: &std::collections::BTreeMap<String, String>) {
        self.producers.sort_by_key(|producer| producer.feature.0);
        for producer in &mut self.producers {
            for name in producer.edge_names.iter_mut().chain(&mut producer.face_names) {
                *name = crate::clipboard::remap_topology(name, features, bodies);
            }
            for link in &mut producer.links {
                link.name = crate::clipboard::remap_topology(&link.name, features, bodies);
            }
        }
    }
}

impl SheetBendRef {
    pub fn tag(&self) -> String {
        format!("F{}.{}", self.feature.0, self.index)
    }
}

impl std::str::FromStr for SheetBendRef {
    type Err = DocError;

    fn from_str(value: &str) -> Result<Self> {
        let invalid = || DocError::Invalid("a bend reference must be F<feature id>.<bend index>".into());
        if value.len() > 32 {
            return Err(invalid());
        }
        let (feature, index) = value.strip_prefix('F').and_then(|v| v.split_once('.')).ok_or_else(invalid)?;
        if feature.is_empty() || index.is_empty() || !feature.bytes().chain(index.bytes()).all(|c| c.is_ascii_digit()) {
            return Err(invalid());
        }
        let feature = feature.parse::<u64>().map_err(|_| invalid())?;
        let index = index.parse::<u32>().map_err(|_| invalid())?;
        if feature == 0 || feature > crate::document::MAX_FEATURE_ID {
            return Err(invalid());
        }
        Ok(Self { feature: FeatureId(feature), index, producer_bend_count: None, source_certificate: None })
    }
}

fn review() -> DocError {
    DocError::Invalid("bend slots: source identity is missing or its topology changed; review and reselect the bend edges".into())
}

fn named_inputs(names: &[String], count: usize) -> Result<()> {
    if names.len() != count
        || count > 256
        || names.iter().any(|name| {
            name.is_empty()
                || name.len() > 1024
                || name.contains('~')
                || name.match_indices('#').any(|(index, _)| name.as_bytes().get(index + 1) != Some(&b'['))
                || name.contains(":face:")
        })
        || names.iter().collect::<std::collections::BTreeSet<_>>().len() != names.len()
    {
        return Err(review());
    }
    Ok(())
}

/// Feature tokens in existing persistent names. Text is remapped by `remap_topology`,
/// while the producer records themselves retain typed feature/sketch dependencies.
fn named_features(name: &str, ids: &mut std::collections::BTreeSet<u64>) -> Result<()> {
    let bytes = name.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'F' || (index > 0 && !matches!(bytes.get(index - 1), Some(b':' | b'|' | b'@' | b'(' | b'[' | b','))) {
            continue;
        }
        let suffix = name.get(index.saturating_add(1)..).ok_or_else(review)?;
        let count = suffix.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            continue;
        }
        if !matches!(suffix.as_bytes().get(count), Some(b'.' | b':')) {
            continue;
        }
        let id = suffix.get(..count).ok_or_else(review)?.parse::<u64>().map_err(|_| review())?;
        if id == 0 || id > crate::document::MAX_FEATURE_ID || ids.len() >= 256 {
            return Err(review());
        }
        ids.insert(id);
    }
    Ok(())
}

impl crate::Document {
    /// Capture a new reviewed physical source selection. Never call this to repair an old
    /// saved reference implicitly: use `validate_bend_slot_reference` when reopening/editing.
    pub fn capture_bend_slot_reference(&self, sheet: &crate::sheet::SheetBody, tag: &str) -> Result<SheetBendRef> {
        let mut reference: SheetBendRef = tag.parse()?;
        let prefix = format!("F{}.", reference.feature.0);
        let count = sheet.flanges.iter().filter(|flange| flange.tag.starts_with(&prefix)).count();
        if count == 0 || count > 256 || sheet.source_feature == 0 {
            return Err(review());
        }
        reference.producer_bend_count = Some(count);
        let mut budget = SourceBudget::new();
        budget.text(&sheet.body)?;
        let mut path = Vec::new();
        let mut current = sheet.flanges.iter().position(|flange| flange.tag == tag).ok_or_else(review)?;
        let mut ids = std::collections::BTreeSet::from([sheet.source_feature]);
        for _ in 0..64 {
            let flange = sheet.flanges.get(current).ok_or_else(review)?;
            let source: SheetBendRef = flange.tag.parse()?;
            if flange.source_path.len() != 1 {
                return Err(DocError::Invalid(
                    "bend slots: this producer has no certified child path; segmented, contour and point-only fold selections are not supported yet"
                        .into(),
                ));
            }
            budget.records(1)?;
            ids.insert(source.feature.0);
            path.push(BendSourceStep { producer: source.feature, child: flange.source_path.clone() });
            match flange.parent {
                Some(parent) => current = parent,
                None => break,
            }
        }
        if path.len() >= 64 {
            return Err(review());
        }
        let mut producers = Vec::new();
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = ids.iter().find(|id| !visited.contains(*id)).copied() {
            if visited.len() >= 64 {
                return Err(review());
            }
            visited.insert(id);
            let feature = self.feature(id).ok_or_else(review)?;
            if feature.suppressed {
                return Err(review());
            }
            budget.records(1)?;
            if feature.edge_names.len() > 256 || feature.face_names.len() > 256 {
                return Err(review());
            }
            budget.records(feature.edge_names.len().saturating_add(feature.face_names.len()))?;
            for name in feature.edge_names.iter().chain(&feature.face_names) {
                budget.text(name)?;
            }
            let mut producer = BendSourceProducer {
                feature: FeatureId(id),
                kind: feature.kind.kind_key().into(),
                edge_names: Vec::new(),
                face_names: Vec::new(),
                sketches: Vec::new(),
                bodies: Vec::new(),
                topology: Vec::new(),
                curves: Vec::new(),
                links: Vec::new(),
                patterns: Vec::new(),
            };
            if let crate::FeatureKind::Extrude { thin, operation, targets, .. } = &feature.kind
                && (thin.is_some() || *operation != crate::Operation::NewBody || targets.len() > 64)
            {
                return Err(DocError::Invalid("bend slots: thin or boolean source extrudes are not structurally certified yet".into()));
            }
            match &feature.kind {
                crate::FeatureKind::SheetBase { sketch, profiles, .. } | crate::FeatureKind::Extrude { sketch, profiles, .. } => {
                    if !matches!(profiles, crate::ProfileSel::All) {
                        return Err(DocError::Invalid(
                            "bend slots: only all-profile source selections are structurally certified; review the source profiles".into(),
                        ));
                    }
                    producer.sketches.push(*sketch);
                    producer.topology.push(serde_json::to_string(profiles).map_err(|_| review())?);
                    ids.insert(sketch.0);
                }
                crate::FeatureKind::SheetFlange { edges, .. } => {
                    named_inputs(&feature.edge_names, edges.len())?;
                    producer.edge_names = feature.edge_names.clone();
                    producer.topology = edges.iter().map(|edge| format!("segments:{}", edge.segments.len())).collect();
                }
                crate::FeatureKind::SheetConvert { bends, .. } => {
                    named_inputs(&feature.edge_names, bends.len())?;
                    named_inputs(&feature.face_names, 1)?;
                    producer.edge_names = feature.edge_names.clone();
                    producer.face_names = feature.face_names.clone();
                    if id == sheet.source_feature {
                        named_inputs(std::slice::from_ref(&sheet.source_root), 1)?;
                        if producer.face_names.first() != Some(&sheet.source_root) {
                            return Err(review());
                        }
                    }
                }
                crate::FeatureKind::Sketch { sketch, .. } => {
                    if sketch.curves.len() > 2000
                        || sketch.patterns.len() > 128
                        || sketch.links.len() > 128
                        || !sketch.offsets.is_empty()
                        || !sketch.freeform_offsets.is_empty()
                        || !sketch.wires.is_empty()
                        || !sketch.equations.is_empty()
                    {
                        return Err(DocError::Invalid("bend slots: source sketch topology is not certified yet".into()));
                    }
                    for link in &sketch.links {
                        use solidcraft_sketch::{LinkKind, LinkSource};
                        if link.lost || link.name_dropped || !matches!(link.kind, LinkKind::Project | LinkKind::Include) {
                            return Err(review());
                        }
                        budget.records(1)?;
                        budget.text(&link.id)?;
                        let mut identity = BendSourceLink {
                            id: link.id.clone(),
                            kind: format!("{:?}", link.kind),
                            body: None,
                            sketch: None,
                            entity: String::new(),
                            name: String::new(),
                        };
                        match &link.source {
                            LinkSource::Edge { .. } | LinkSource::Face { .. } | LinkSource::Vertex { .. } => {
                                return Err(DocError::Invalid(
                                    "bend slots: model-geometry projection source identities are not certified yet".into(),
                                ));
                            }
                            LinkSource::SketchCurve { sketch, curve } => {
                                budget.text(curve)?;
                                identity.kind.push_str(":sketch_curve");
                                identity.entity = curve.clone();
                                identity.sketch = Some(crate::SketchId(*sketch));
                                ids.insert(*sketch);
                            }
                            LinkSource::SketchPoint { sketch, point } => {
                                budget.text(point)?;
                                identity.kind.push_str(":sketch_point");
                                identity.entity = point.clone();
                                identity.sketch = Some(crate::SketchId(*sketch));
                                ids.insert(*sketch);
                            }
                            LinkSource::Origin => identity.entity = "origin".into(),
                            LinkSource::Axis { name } | LinkSource::SketchAxis { name } if matches!(name.as_str(), "X" | "Y" | "Z" | "x" | "y") => {
                                identity.entity = name.clone()
                            }
                            _ => {
                                return Err(DocError::Invalid("bend slots: this projected source has no structural identity certificate yet".into()));
                            }
                        }
                        producer.links.push(identity);
                    }
                    for pattern in &sketch.patterns {
                        budget.records(1)?;
                        budget.text(&pattern.id)?;
                        if pattern.entities.len() > 2000 || pattern.instances.len() > 500 {
                            return Err(review());
                        }
                        budget.records(pattern.entities.len().saturating_add(pattern.instances.len()))?;
                        for entity in &pattern.entities {
                            budget.text(entity)?;
                        }
                        let mut instances = Vec::new();
                        for instance in &pattern.instances {
                            if instance.curves.len() > 2000 || instance.points.len() > 2000 {
                                return Err(review());
                            }
                            budget.records(instance.curves.len().saturating_add(instance.points.len().saturating_mul(2)))?;
                            for text in instance.curves.iter().chain(instance.points.iter().flat_map(|(a, b)| [a, b])) {
                                budget.text(text)?;
                            }
                            instances.push(BendPatternInstance {
                                index: instance.index,
                                curves: instance.curves.clone(),
                                points: instance.points.iter().map(|(a, b)| [a.clone(), b.clone()]).collect(),
                            });
                        }
                        let kind = match &pattern.kind {
                            solidcraft_sketch::SketchPatternKind::Circular { .. } => "circular",
                            solidcraft_sketch::SketchPatternKind::Rectangular { .. } => "rectangular",
                        };
                        producer.patterns.push(BendSourcePattern {
                            id: pattern.id.clone(),
                            kind: kind.into(),
                            entities: pattern.entities.clone(),
                            instances,
                        });
                    }
                    for curve in &sketch.curves {
                        budget.records(1)?;
                        budget.text(&curve.id)?;
                        if let solidcraft_sketch::CurveKind::Spline { pts, handles, .. } = &curve.kind
                            && pts.len().saturating_add(handles.len()) > 128
                        {
                            return Err(review());
                        }
                        if curve.id.is_empty() || curve.id.len() > 128 {
                            return Err(review());
                        }
                        let kind = match &curve.kind {
                            solidcraft_sketch::CurveKind::Line { .. } => "line".into(),
                            solidcraft_sketch::CurveKind::Circle { .. } => "circle".into(),
                            solidcraft_sketch::CurveKind::Arc { .. } => "arc".into(),
                            solidcraft_sketch::CurveKind::Ellipse { .. } => "ellipse".into(),
                            solidcraft_sketch::CurveKind::Spline { degree, control, periodic, .. } => format!("spline:{degree}:{control}:{periodic}"),
                            solidcraft_sketch::CurveKind::Conic { .. } => "conic".into(),
                        };
                        let point_indices = curve.kind.point_ids();
                        budget.records(point_indices.len())?;
                        for index in &point_indices {
                            budget.text(&sketch.points.get(*index).ok_or_else(review)?.id)?;
                        }
                        let points = point_indices
                            .into_iter()
                            .map(|index| {
                                sketch
                                    .points
                                    .get(index)
                                    .map(|point| point.id.clone())
                                    .filter(|id| !id.is_empty() && id.len() <= 128)
                                    .ok_or_else(review)
                            })
                            .collect::<Result<Vec<_>>>()?;
                        producer.curves.push(BendSourceCurve {
                            id: curve.id.clone(),
                            kind,
                            points,
                            construction: curve.construction,
                            reversed: curve.reversed,
                            infinite: curve.infinite,
                            centerline: curve.centerline,
                            link: match &curve.link {
                                Some(link) => {
                                    budget.text(link)?;
                                    Some(link.clone())
                                }
                                None => None,
                            },
                        });
                    }
                }
                crate::FeatureKind::Box { .. } => {}
                crate::FeatureKind::Shell { faces, .. } => {
                    named_inputs(&feature.face_names, faces.len())?;
                    producer.face_names = feature.face_names.clone();
                }
                _ => {
                    return Err(DocError::Invalid(format!(
                        "bend slots: source producer {} is not structurally certified yet",
                        feature.kind.kind_key()
                    )));
                }
            }
            // Unsupported and over-budget source structures have returned before the
            // generic reference walk can allocate copies of their body-name inputs.
            let mut body_budget = Ok(());
            let mut body_count = 0usize;
            feature.kind.walk_refs(&mut |reference| {
                if let crate::feature_refs::Ref::Body(body) = reference
                    && body_budget.is_ok()
                {
                    body_count = body_count.saturating_add(1);
                    body_budget = if body_count > 64 { Err(review()) } else { budget.records(1).and_then(|()| budget.text(body.as_ref())) };
                }
            });
            body_budget?;
            producer.bodies = feature.kind.input_bodies().into_iter().map(crate::BodyName).collect();
            for name in producer.edge_names.iter().chain(&producer.face_names) {
                named_features(name, &mut ids)?;
            }
            producers.push(producer);
        }
        producers.sort_by_key(|producer| producer.feature.0);
        let certificate =
            BendSlotSourceCertificate { body: crate::BodyName(sheet.body.clone()), origin: FeatureId(sheet.source_feature), path, producers };
        certificate.validate_shape()?;
        reference.source_certificate = Some(certificate);
        Ok(reference)
    }

    /// Resolve a certified physical child to its current legacy sheet tag. Conversion BFS
    /// order may change with the flat frame; its public CAD names retain their old semantics.
    pub fn resolve_bend_slot_reference(&self, sheet: &crate::sheet::SheetBody, reference: &SheetBendRef) -> Result<String> {
        let certificate = reference.source_certificate.as_ref().ok_or_else(review)?;
        certificate.validate_shape()?;
        let expected = reference.producer_bend_count.ok_or_else(review)?;
        if expected == 0
            || expected > 256
            || reference.index as usize >= expected
            || certificate.path.first().map(|step| step.producer) != Some(reference.feature)
            || certificate.origin.0 != sheet.source_feature
            || certificate.body.as_ref() != sheet.body
        {
            return Err(review());
        }
        let prefix = format!("F{}.", reference.feature.0);
        let count = sheet.flanges.iter().filter(|flange| flange.tag.starts_with(&prefix)).count();
        if expected != count {
            return Err(review());
        }
        let mut matched = None;
        for (index, flange) in sheet.flanges.iter().enumerate().filter(|(_, flange)| flange.tag.starts_with(&prefix)) {
            let mut at = Some(index);
            let mut same = true;
            for step in &certificate.path {
                let Some(current) = at.and_then(|index| sheet.flanges.get(index)) else {
                    same = false;
                    break;
                };
                let producer = current.tag.parse::<SheetBendRef>().map_err(|_| review())?.feature;
                if producer != step.producer || current.source_path != step.child {
                    same = false;
                    break;
                }
                at = current.parent;
            }
            if same && at.is_none() {
                if matched.is_some() {
                    return Err(review());
                }
                matched = Some(flange.tag.as_str());
            }
        }
        let tag = matched.ok_or_else(review)?;
        let current = self.capture_bend_slot_reference(sheet, tag)?;
        if current.feature != reference.feature
            || current.producer_bend_count != reference.producer_bend_count
            || current.source_certificate != reference.source_certificate
        {
            return Err(review());
        }
        Ok(tag.to_string())
    }

    /// Validate without adopting another source or refreshing a saved certificate.
    pub fn validate_bend_slot_reference(&self, sheet: &crate::sheet::SheetBody, reference: &SheetBendRef) -> Result<()> {
        self.resolve_bend_slot_reference(sheet, reference).map(|_| ())
    }
}

/// The end shape of both slots and relief throats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(rename_all = "snake_case")]
pub enum SheetBendSlotProfile {
    #[default]
    Rounded,
    Rectangular,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bend_tags_round_trip_and_reject_unbounded_or_ambiguous_references() {
        let bend: SheetBendRef = "F12.3".parse().unwrap();
        assert_eq!(bend.feature, FeatureId(12));
        assert_eq!(bend.index, 3);
        assert_eq!(bend.tag(), "F12.3");
        for value in ["", "12.3", "F0.0", "F-1.0", "F1.-1", "F1.0.2", "F1.4294967296", "F18446744073709551615.0", "F1.+2"] {
            assert!(value.parse::<SheetBendRef>().is_err(), "{value}");
        }
    }

    #[test]
    fn bend_slot_dependencies_follow_feature_and_body_copies() {
        let mut feature: crate::FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "sheet_bend_slots", "body": "Panel", "bends": [{"feature": 12, "index": 3}],
            "slot_length": "7 mm", "slot_width": "1 mm", "bridge": "3 mm",
            "end_margin": "2 mm", "corner_radius": "3 mm", "profile": "rounded", "reliefs": true,
        }))
        .unwrap();
        feature.remap_refs(&std::collections::BTreeMap::from([(12, 42)]), &std::collections::BTreeMap::new());
        feature.remap_bodies(&std::collections::BTreeMap::from([("Panel".into(), "Panel copy".into())]));
        let value = serde_json::to_value(&feature).unwrap();
        assert_eq!(value["bends"][0]["feature"], 42);
        assert_eq!(value["bends"][0]["index"], 3);
        assert_eq!(value["body"], "Panel copy");
        assert!(feature.names_feature(42, ""));
        assert!(!feature.names_feature(12, ""));
    }

    fn certificate_fixture() -> BendSlotSourceCertificate {
        serde_json::from_value(serde_json::json!({
            "body": "Panel", "origin": 2,
            "path": [{"producer": 12, "child": [0]}],
            "producers": [
                {"feature": 1, "kind": "sketch", "edge_names": [], "face_names": [], "sketches": [], "bodies": [], "topology": [], "curves": [
                    {"id": "l1", "kind": "line", "points": ["p1","p2"], "construction": false, "reversed": false, "infinite": false, "centerline": false, "link": null}
                ], "links": [], "patterns": []},
                {"feature": 2, "kind": "sheet_base", "edge_names": [], "face_names": [], "sketches": [1], "bodies": [], "topology": ["all"], "curves": [], "links": [], "patterns": []},
                {"feature": 12, "kind": "sheet_flange", "edge_names": ["F2:top|F12:side#[F2:bottom]"], "face_names": ["sheet:Panel:F12.0:top"], "sketches": [], "bodies": ["Panel"], "topology": ["segments:0"], "curves": [], "links": [
                    {"id": "j1", "kind": "Project", "body": "Panel", "sketch": null, "entity": "edge", "name": "F2:top|F12:side"}
                ], "patterns": []}
            ]
        })).unwrap()
    }

    #[test]
    fn bend_slot_source_certificates_remap_typed_graph_and_embedded_persistent_names() {
        let mut feature: crate::FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "sheet_bend_slots", "body": "Panel", "bends": [{"feature": 12, "index": 0, "producer_bend_count": 2,
                "source_certificate": certificate_fixture()}],
            "slot_length": "7 mm", "slot_width": "1 mm", "bridge": "3 mm", "end_margin": "2 mm",
            "corner_radius": "3 mm", "profile": "rounded", "reliefs": false
        }))
        .unwrap();
        feature.remap_refs(&std::collections::BTreeMap::from([(1, 70), (2, 20), (12, 42)]), &std::collections::BTreeMap::new());
        feature.remap_bodies(&std::collections::BTreeMap::from([("Panel".into(), "Panel copy".into())]));
        let value = serde_json::to_value(&feature).unwrap();
        let certificate = &value["bends"][0]["source_certificate"];
        assert_eq!(value["bends"][0]["feature"], 42);
        assert_eq!(certificate["body"], "Panel copy");
        assert_eq!(certificate["origin"], 20);
        assert_eq!(certificate["path"][0]["producer"], 42);
        assert_eq!(certificate["producers"][0]["feature"], 20, "a nonmonotonic id map restores canonical producer order");
        assert_eq!(certificate["producers"][0]["sketches"][0], 70);
        assert_eq!(certificate["producers"][1]["edge_names"][0], "F20:top|F42:side#[F20:bottom]");
        assert_eq!(certificate["producers"][1]["face_names"][0], "sheet:Panel copy:F42.0:top");
        assert_eq!(certificate["producers"][1]["links"][0]["body"], "Panel copy");
        assert_eq!(certificate["producers"][1]["links"][0]["name"], "F20:top|F42:side");
        let reopened: crate::FeatureKind = serde_json::from_value(value).unwrap();
        assert_eq!(feature, reopened, "native save/reopen retains the complete certificate");
        for id in [20, 42, 70] {
            assert!(feature.names_feature(id, "") || feature.input_sketches().contains(&id));
        }
        assert!(!feature.names_feature(12, ""));
    }

    #[test]
    fn bend_slot_source_certificate_budgets_are_aggregate_and_checked_before_comparison() {
        let mut certificate = certificate_fixture();
        certificate.validate_shape().unwrap();
        let curve = certificate.producers[0].curves[0].clone();
        certificate.producers[0].curves = vec![curve.clone(); 1024];
        assert!(certificate.validate_shape().is_err(), "curve points count against the same aggregate record budget");
        certificate = certificate_fixture();
        certificate.producers[0].curves = vec![BendSourceCurve { id: "c".repeat(4096), points: vec![], ..curve }; 33];
        assert!(certificate.validate_shape().is_err(), "text bytes count across records, not per producer");
        certificate = certificate_fixture();
        certificate.path = vec![certificate.path[0].clone(); 65];
        assert!(certificate.validate_shape().is_err(), "deep ancestry is refused");
        assert!(named_inputs(&["F2:top#[F3:side]".into()], 1).is_ok(), "adjacency-qualified names remain usable");
        for name in ["F2:top#3", "F2:top~1", "Panel:face:3"] {
            assert!(named_inputs(&[name.into()], 1).is_err(), "{name}");
        }
    }
}
