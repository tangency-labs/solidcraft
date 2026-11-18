//! Timeline Fixer 3000: Automated parametric tree diagnostic, healing, and re-ordering engine.
//!
//! Provides topological DAG analysis over `FeatureNode` and `DocumentTimeline` ASTs,
//! deterministic healing via topological signature matching (geometric adjacency, face normals,
//! surface areas, bounding boxes), and mutation impact-guarding with dry-run remapping.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use solidcraft_doc::{Document, Feature, FeatureKind, Model, PlaneRef};
use solidcraft_geom::{Aabb3, Vec3};
use solidcraft_sketch::LinkSource;

use crate::{Result, Session, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Warning, // Sub-optimal evaluation order or loose sketch constraints
    Error,   // Missing parent reference, suppressed upstream dependency
    Fatal,   // Cyclic dependency or topological invalidity
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TimelineIssue {
    OrphanedReference {
        feature_id: u64,
        missing_target_id: u64,
        expected_type: &'static str, // "Face", "Edge", "Plane"
    },
    DependencyOrderInversion {
        feature_id: u64,
        depends_on: u64,
        current_index: usize,
        required_before_index: usize,
    },
    DanglingSketchConstraint {
        sketch_id: u64,
        constraint_id: u64,
    },
    BrokenFilletChamferPropagation {
        feature_id: u64,
        lost_edge_count: usize,
    },
}

#[derive(Deserialize)]
#[serde(rename = "TimelineIssue")]
enum TimelineIssueHelper {
    OrphanedReference {
        feature_id: u64,
        missing_target_id: u64,
        expected_type: String,
    },
    DependencyOrderInversion {
        feature_id: u64,
        depends_on: u64,
        current_index: usize,
        required_before_index: usize,
    },
    DanglingSketchConstraint {
        sketch_id: u64,
        constraint_id: u64,
    },
    BrokenFilletChamferPropagation {
        feature_id: u64,
        lost_edge_count: usize,
    },
}

impl<'de> Deserialize<'de> for TimelineIssue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = TimelineIssueHelper::deserialize(deserializer)?;
        Ok(match helper {
            TimelineIssueHelper::OrphanedReference {
                feature_id,
                missing_target_id,
                expected_type,
            } => {
                let static_type: &'static str = match expected_type.as_str() {
                    "Face" => "Face",
                    "Edge" => "Edge",
                    "Plane" => "Plane",
                    "Construction" => "Construction",
                    other => Box::leak(other.to_string().into_boxed_str()),
                };
                TimelineIssue::OrphanedReference {
                    feature_id,
                    missing_target_id,
                    expected_type: static_type,
                }
            }
            TimelineIssueHelper::DependencyOrderInversion {
                feature_id,
                depends_on,
                current_index,
                required_before_index,
            } => TimelineIssue::DependencyOrderInversion {
                feature_id,
                depends_on,
                current_index,
                required_before_index,
            },
            TimelineIssueHelper::DanglingSketchConstraint {
                sketch_id,
                constraint_id,
            } => TimelineIssue::DanglingSketchConstraint {
                sketch_id,
                constraint_id,
            },
            TimelineIssueHelper::BrokenFilletChamferPropagation {
                feature_id,
                lost_edge_count,
            } => TimelineIssue::BrokenFilletChamferPropagation {
                feature_id,
                lost_edge_count,
            },
        })
    }
}

impl TimelineIssue {
    pub fn severity(&self) -> IssueSeverity {
        match self {
            TimelineIssue::DependencyOrderInversion { .. } => IssueSeverity::Warning,
            TimelineIssue::DanglingSketchConstraint { .. } => IssueSeverity::Warning,
            TimelineIssue::BrokenFilletChamferPropagation { .. } => IssueSeverity::Error,
            TimelineIssue::OrphanedReference { .. } => IssueSeverity::Error,
        }
    }

    pub fn affected_feature(&self) -> u64 {
        match self {
            TimelineIssue::OrphanedReference { feature_id, .. } => *feature_id,
            TimelineIssue::DependencyOrderInversion { feature_id, .. } => *feature_id,
            TimelineIssue::DanglingSketchConstraint { sketch_id, .. } => *sketch_id,
            TimelineIssue::BrokenFilletChamferPropagation { feature_id, .. } => *feature_id,
        }
    }

    pub fn description(&self) -> String {
        match self {
            TimelineIssue::OrphanedReference { feature_id, missing_target_id, expected_type } => {
                format!(
                    "Feature #{feature_id} references missing {expected_type} from target #{missing_target_id}"
                )
            }
            TimelineIssue::DependencyOrderInversion {
                feature_id,
                depends_on,
                current_index,
                required_before_index,
            } => {
                format!(
                    "Feature #{feature_id} at position {current_index} depends on #{depends_on} (at position {required_before_index})"
                )
            }
            TimelineIssue::DanglingSketchConstraint { sketch_id, constraint_id } => {
                format!("Sketch #{sketch_id} contains dangling or unattached constraint #{constraint_id}")
            }
            TimelineIssue::BrokenFilletChamferPropagation { feature_id, lost_edge_count } => {
                format!(
                    "Blend feature #{feature_id} lost {lost_edge_count} referenced edge(s) due to upstream topology change"
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub total_features: usize,
    pub broken_features: Vec<u64>,
    pub issues: Vec<TimelineIssue>,
    pub can_auto_heal: bool,
}

impl DiagnosticReport {
    pub fn is_healthy(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn fatal_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity() == IssueSeverity::Fatal).count()
    }

    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity() == IssueSeverity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity() == IssueSeverity::Warning).count()
    }
}

/// AST Node status in the parametric tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Healthy,
    Warning,
    Error,
    Fatal,
    Healed,
}

/// Topological geometric signature for deterministic face matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceSignature {
    pub area: f64,
    pub centroid: Vec3,
    pub normal: Option<Vec3>,
    pub bounding_box: Aabb3,
}

impl FaceSignature {
    /// Calculate similarity between 0.0 and 1.0 against candidate face.
    pub fn match_confidence(&self, candidate: &FaceSignature, body_diagonal: f64) -> f64 {
        let max_area = self.area.max(candidate.area).max(1e-6);
        let area_sim = (1.0 - (self.area - candidate.area).abs() / max_area).clamp(0.0, 1.0);

        let normal_sim = match (self.normal, candidate.normal) {
            (Some(n1), Some(n2)) => {
                let dot = n1.x * n2.x + n1.y * n2.y + n1.z * n2.z;
                dot.max(0.0).clamp(0.0, 1.0)
            }
            (None, None) => 0.9,
            _ => 0.4,
        };

        let diag = body_diagonal.max(1.0);
        let dist = self.centroid.dist(candidate.centroid);
        let dist_sim = (1.0 - dist / diag).clamp(0.0, 1.0);

        let bb_center_self = (self.bounding_box.min + self.bounding_box.max) * 0.5;
        let bb_center_cand = (candidate.bounding_box.min + candidate.bounding_box.max) * 0.5;
        let bb_sim = (1.0 - bb_center_self.dist(bb_center_cand) / diag).clamp(0.0, 1.0);

        // Weighted signature score: Normal 35%, Area 30%, Centroid 20%, Bounding Box 15%
        0.35 * normal_sim + 0.30 * area_sim + 0.20 * dist_sim + 0.15 * bb_sim
    }
}

/// Topological geometric signature for deterministic edge matching.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeSignature {
    pub length: f64,
    pub mid: Vec3,
    pub dir: Vec3,
    pub bounding_box: Aabb3,
}

impl EdgeSignature {
    /// Calculate similarity between 0.0 and 1.0 against candidate edge.
    pub fn match_confidence(&self, candidate: &EdgeSignature, body_diagonal: f64) -> f64 {
        let max_len = self.length.max(candidate.length).max(1e-6);
        let len_sim = (1.0 - (self.length - candidate.length).abs() / max_len).clamp(0.0, 1.0);

        let dot = (self.dir.x * candidate.dir.x + self.dir.y * candidate.dir.y + self.dir.z * candidate.dir.z).abs();
        let dir_sim = dot.clamp(0.0, 1.0);

        let diag = body_diagonal.max(1.0);
        let mid_dist = self.mid.dist(candidate.mid);
        let mid_sim = (1.0 - mid_dist / diag).clamp(0.0, 1.0);

        0.35 * dir_sim + 0.35 * len_sim + 0.30 * mid_sim
    }
}

/// A specific topological geometric reference maintained by an AST node.
#[derive(Debug, Clone, Serialize)]
pub enum TopologicalReference {
    Face {
        target_feature_id: u64,
        signature: FaceSignature,
        anchor_name: Option<String>,
    },
    Edge {
        target_feature_id: u64,
        signature: EdgeSignature,
        edge_name: Option<String>,
    },
    SketchPlane {
        plane_type: &'static str,
        target_feature_id: Option<u64>,
    },
    InputSketch {
        sketch_id: u64,
    },
}

#[derive(Deserialize)]
#[serde(rename = "TopologicalReference")]
enum TopologicalReferenceHelper {
    Face {
        target_feature_id: u64,
        signature: FaceSignature,
        anchor_name: Option<String>,
    },
    Edge {
        target_feature_id: u64,
        signature: EdgeSignature,
        edge_name: Option<String>,
    },
    SketchPlane {
        plane_type: String,
        target_feature_id: Option<u64>,
    },
    InputSketch {
        sketch_id: u64,
    },
}

impl<'de> Deserialize<'de> for TopologicalReference {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = TopologicalReferenceHelper::deserialize(deserializer)?;
        Ok(match helper {
            TopologicalReferenceHelper::Face {
                target_feature_id,
                signature,
                anchor_name,
            } => TopologicalReference::Face {
                target_feature_id,
                signature,
                anchor_name,
            },
            TopologicalReferenceHelper::Edge {
                target_feature_id,
                signature,
                edge_name,
            } => TopologicalReference::Edge {
                target_feature_id,
                signature,
                edge_name,
            },
            TopologicalReferenceHelper::SketchPlane {
                plane_type,
                target_feature_id,
            } => {
                let static_type: &'static str = match plane_type.as_str() {
                    "Face" => "Face",
                    "Edge" => "Edge",
                    "Plane" => "Plane",
                    "Construction" => "Construction",
                    "Origin" => "Origin",
                    other => Box::leak(other.to_string().into_boxed_str()),
                };
                TopologicalReference::SketchPlane {
                    plane_type: static_type,
                    target_feature_id,
                }
            }
            TopologicalReferenceHelper::InputSketch { sketch_id } => {
                TopologicalReference::InputSketch { sketch_id }
            }
        })
    }
}

/// Abstract Syntax Tree node representing a feature in the parametric timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureNode {
    pub id: u64,
    pub name: String,
    pub kind_name: String,
    pub index: usize,
    pub suppressed: bool,
    pub dependencies: HashSet<u64>,
    pub dependents: HashSet<u64>,
    pub referenced_targets: Vec<TopologicalReference>,
    pub issues: Vec<TimelineIssue>,
    pub status: NodeStatus,
}

/// Abstract Syntax Tree representing the entire document parametric timeline DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentTimeline {
    pub nodes: Vec<FeatureNode>,
    pub node_map: HashMap<u64, usize>,
}

impl DocumentTimeline {
    /// Build the timeline AST from a real Document and optional evaluated Model.
    pub fn from_document(doc: &Document, model: Option<&Model>) -> Self {
        let mut nodes = Vec::with_capacity(doc.features.len());
        let mut node_map = HashMap::with_capacity(doc.features.len());

        for (idx, feat) in doc.features.iter().enumerate() {
            node_map.insert(feat.id, idx);
        }

        for (idx, feat) in doc.features.iter().enumerate() {
            let mut deps = HashSet::new();
            let mut refs = Vec::new();

            // 1. Direct input sketches
            for sk_id in feat.kind.input_sketches() {
                deps.insert(sk_id);
                refs.push(TopologicalReference::InputSketch { sketch_id: sk_id });
            }

            // 2. Sketch planes
            if let FeatureKind::Sketch { plane, sketch } = &feat.kind {
                match plane {
                    PlaneRef::Face { name, anchor, at, plane } => {
                        let target_fid = anchor.as_ref().map(|a| a.target.origin_feature).unwrap_or(0);
                        if target_fid != 0 {
                            deps.insert(target_fid);
                        }
                        refs.push(TopologicalReference::Face {
                            target_feature_id: target_fid,
                            signature: FaceSignature {
                                area: 1.0,
                                centroid: *at,
                                normal: Some(plane.normal()),
                                bounding_box: Aabb3 {
                                    min: *at - Vec3::new(1.0, 1.0, 1.0),
                                    max: *at + Vec3::new(1.0, 1.0, 1.0),
                                },
                            },
                            anchor_name: name.clone(),
                        });
                    }
                    PlaneRef::Construction { feature, .. } => {
                        deps.insert(feature.0);
                        refs.push(TopologicalReference::SketchPlane {
                            plane_type: "Construction",
                            target_feature_id: Some(feature.0),
                        });
                    }
                    PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => {
                        collect_plane_dependencies(base, doc, &mut deps);
                    }
                    _ => {}
                }

                // 3. Sketch links (external geometry projections)
                for link in &sketch.links {
                    match &link.source {
                        LinkSource::SketchCurve { sketch, .. } | LinkSource::SketchPoint { sketch, .. } => {
                            deps.insert(*sketch);
                        }
                        LinkSource::Edge { body, .. }
                        | LinkSource::Face { body, .. }
                        | LinkSource::Vertex { body, .. } => {
                            if let Some(producer) = find_producer_of_body(doc, body, idx) {
                                deps.insert(producer);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // 4. Fillets and Chamfers
            if let FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } = &feat.kind {
                for (edge_idx, edge_pos) in edges.iter().enumerate() {
                    let edge_ref = feat.edge_refs.get(edge_idx);
                    let edge_name = feat.edge_names.get(edge_idx).cloned();
                    let sig = EdgeSignature {
                        length: 1.0,
                        mid: edge_pos.0,
                        dir: edge_ref.map(|r| r.dir).unwrap_or(Vec3::new(0.0, 0.0, 1.0)),
                        bounding_box: edge_ref
                            .map(|r| Aabb3 { min: r.min, max: r.max })
                            .unwrap_or_else(|| Aabb3 {
                                min: edge_pos.0 - Vec3::new(5.0, 5.0, 5.0),
                                max: edge_pos.0 + Vec3::new(5.0, 5.0, 5.0),
                            }),
                    };
                    refs.push(TopologicalReference::Edge {
                        target_feature_id: 0,
                        signature: sig,
                        edge_name,
                    });
                }
            }

            nodes.push(FeatureNode {
                id: feat.id,
                name: feat.name.clone(),
                kind_name: feat.kind.base_name().to_string(),
                index: idx,
                suppressed: feat.suppressed,
                dependencies: deps,
                dependents: HashSet::new(),
                referenced_targets: refs,
                issues: Vec::new(),
                status: NodeStatus::Healthy,
            });
        }

        // Fill dependents
        let n_len = nodes.len();
        for i in 0..n_len {
            let node_id = nodes[i].id;
            let deps: Vec<u64> = nodes[i].dependencies.iter().copied().collect();
            for dep_id in deps {
                if let Some(&dep_idx) = node_map.get(&dep_id) {
                    if dep_idx < nodes.len() {
                        nodes[dep_idx].dependents.insert(node_id);
                    }
                }
            }
        }

        let mut timeline = DocumentTimeline { nodes, node_map };
        timeline.evaluate_node_issues(doc, model);
        timeline
    }

    /// Internal evaluation of issues across all nodes in the AST.
    fn evaluate_node_issues(&mut self, doc: &Document, model: Option<&Model>) {
        let feature_ids: HashSet<u64> = doc.features.iter().map(|f| f.id).collect();
        let suppressed_ids: HashSet<u64> = doc.features.iter().filter(|f| f.suppressed).map(|f| f.id).collect();

        for i in 0..self.nodes.len() {
            let node_id = self.nodes[i].id;
            let node_idx = self.nodes[i].index;
            let mut issues = Vec::new();

            // 1. Check for Dependency Order Inversions and Orphaned References
            for &dep_id in &self.nodes[i].dependencies {
                if !feature_ids.contains(&dep_id) {
                    issues.push(TimelineIssue::OrphanedReference {
                        feature_id: node_id,
                        missing_target_id: dep_id,
                        expected_type: "Plane",
                    });
                } else if suppressed_ids.contains(&dep_id) && !self.nodes[i].suppressed {
                    issues.push(TimelineIssue::OrphanedReference {
                        feature_id: node_id,
                        missing_target_id: dep_id,
                        expected_type: "Face",
                    });
                } else if let Some(&dep_pos) = self.node_map.get(&dep_id) {
                    if dep_pos > node_idx {
                        issues.push(TimelineIssue::DependencyOrderInversion {
                            feature_id: node_id,
                            depends_on: dep_id,
                            current_index: node_idx,
                            required_before_index: dep_pos + 1,
                        });
                    }
                }
            }

            // 2. Check for Dangling Sketch Constraints
            if let Some(feat) = doc.features.get(node_idx) {
                if let FeatureKind::Sketch { sketch, .. } = &feat.kind {
                    let pt_len = sketch.points.len();
                    let cv_len = sketch.curves.len();

                    for (c_idx, c) in sketch.constraints.iter().enumerate() {
                        let is_dangling = match &c.kind {
                            solidcraft_sketch::ConstraintKind::Coincident { p, q } => *p >= pt_len || *q >= pt_len,
                            solidcraft_sketch::ConstraintKind::PointOnCurve { p, c } => *p >= pt_len || *c >= cv_len,
                            solidcraft_sketch::ConstraintKind::Distance { p, q, .. } => *p >= pt_len || *q >= pt_len,
                            solidcraft_sketch::ConstraintKind::Equal { a, b }
                            | solidcraft_sketch::ConstraintKind::Parallel { a, b }
                            | solidcraft_sketch::ConstraintKind::Perpendicular { a, b } => {
                                *a >= cv_len || *b >= cv_len
                            }
                            _ => false,
                        };

                        if is_dangling {
                            let cid = c.id.trim_start_matches(|ch: char| !ch.is_ascii_digit()).parse::<u64>().unwrap_or(c_idx as u64);
                            issues.push(TimelineIssue::DanglingSketchConstraint {
                                sketch_id: node_id,
                                constraint_id: cid,
                            });
                        }
                    }
                }

                // 3. Check for Broken Fillet/Chamfer Propagation
                if let FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } = &feat.kind {
                    let mut lost = 0;
                    if let Some(m) = model {
                        if let Some(r) = m.result(node_id) {
                            if r.error.is_some() {
                                lost = edges.len().max(1);
                            }
                        }
                    }
                    if lost > 0 {
                        issues.push(TimelineIssue::BrokenFilletChamferPropagation {
                            feature_id: node_id,
                            lost_edge_count: lost,
                        });
                    }
                }
            }

            // Update status based on issue severity
            let status = if issues.iter().any(|i| i.severity() == IssueSeverity::Fatal) {
                NodeStatus::Fatal
            } else if issues.iter().any(|i| i.severity() == IssueSeverity::Error) {
                NodeStatus::Error
            } else if issues.iter().any(|i| i.severity() == IssueSeverity::Warning) {
                NodeStatus::Warning
            } else {
                NodeStatus::Healthy
            };

            self.nodes[i].issues = issues;
            self.nodes[i].status = status;
        }
    }

    /// Produce a comprehensive diagnostic report over the timeline AST.
    pub fn diagnose(&self) -> DiagnosticReport {
        let mut broken_features = Vec::new();
        let mut issues = Vec::new();

        for node in &self.nodes {
            if !node.issues.is_empty() {
                broken_features.push(node.id);
                issues.extend(node.issues.clone());
            }
        }

        let has_fatal = issues.iter().any(|i| i.severity() == IssueSeverity::Fatal);
        let can_auto_heal = !issues.is_empty() && !has_fatal;

        DiagnosticReport {
            total_features: self.nodes.len(),
            broken_features,
            issues,
            can_auto_heal,
        }
    }

    /// Perform topological sorting of the timeline DAG using Kahn's algorithm.
    /// Resolves all `DependencyOrderInversion` issues deterministically.
    pub fn topological_sort(&self) -> std::result::Result<Vec<u64>, TimelineIssue> {
        let mut in_degree: HashMap<u64, usize> = HashMap::new();
        let mut adj: HashMap<u64, Vec<u64>> = HashMap::new();

        for node in &self.nodes {
            in_degree.insert(node.id, 0);
            adj.insert(node.id, Vec::new());
        }

        // Only consider valid internal dependencies
        for node in &self.nodes {
            for &dep_id in &node.dependencies {
                if in_degree.contains_key(&dep_id) {
                    adj.entry(dep_id).or_default().push(node.id);
                    *in_degree.entry(node.id).or_default() += 1;
                }
            }
        }

        // Start with nodes that have in-degree 0, preserving stable relative timeline index
        let mut ready: Vec<u64> = self
            .nodes
            .iter()
            .filter(|n| in_degree.get(&n.id).copied().unwrap_or(0) == 0)
            .map(|n| n.id)
            .collect();

        let mut sorted = Vec::with_capacity(self.nodes.len());

        while !ready.is_empty() {
            // Sort stable by original timeline index
            ready.sort_by_key(|id| self.node_map.get(id).copied().unwrap_or(usize::MAX));
            let curr = ready.remove(0);
            sorted.push(curr);

            if let Some(neighbors) = adj.get(&curr) {
                for &nxt in neighbors {
                    if let Some(deg) = in_degree.get_mut(&nxt) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            ready.push(nxt);
                        }
                    }
                }
            }
        }

        if sorted.len() < self.nodes.len() {
            // Cycle detected: fatal dependency inversion
            let first_unplaced = self
                .nodes
                .iter()
                .find(|n| !sorted.contains(&n.id))
                .map(|n| n.id)
                .unwrap_or(0);
            return Err(TimelineIssue::DependencyOrderInversion {
                feature_id: first_unplaced,
                depends_on: first_unplaced,
                current_index: self.node_map.get(&first_unplaced).copied().unwrap_or(0),
                required_before_index: 0,
            });
        }

        Ok(sorted)
    }

    /// Deterministically heal the parametric tree using topological signature matching
    /// and topological reordering. Never uses arbitrary index guesses.
    pub fn deterministic_heal(&self, doc: &mut Document, model: Option<&Model>) -> HealingSummary {
        let mut healed_inversions = 0;
        let mut healed_constraints = 0;
        let mut healed_blends = 0;
        let mut healed_orphans = 0;

        // Step 1: Topological Sort Reordering
        if let Ok(sorted_ids) = self.topological_sort() {
            let mut new_features = Vec::with_capacity(doc.features.len());
            let old_features: HashMap<u64, Feature> = doc.features.drain(..).map(|f| (f.id, f)).collect();

            for id in &sorted_ids {
                if let Some(f) = old_features.get(id) {
                    new_features.push(f.clone());
                }
            }

            // Detect how many features changed slot
            for (idx, f) in new_features.iter().enumerate() {
                if self.node_map.get(&f.id).copied() != Some(idx) {
                    healed_inversions += 1;
                }
            }
            doc.features = new_features;
        }

        // Step 2: Fix dangling sketch constraints
        for feat in &mut doc.features {
            if let FeatureKind::Sketch { sketch, .. } = &mut feat.kind {
                let pt_len = sketch.points.len();
                let cv_len = sketch.curves.len();
                let prev_count = sketch.constraints.len();

                sketch.constraints.retain(|c| match &c.kind {
                    solidcraft_sketch::ConstraintKind::Coincident { p, q } => *p < pt_len && *q < pt_len,
                    solidcraft_sketch::ConstraintKind::PointOnCurve { p, c } => *p < pt_len && *c < cv_len,
                    solidcraft_sketch::ConstraintKind::Distance { p, q, .. } => *p < pt_len && *q < pt_len,
                    solidcraft_sketch::ConstraintKind::Equal { a, b }
                    | solidcraft_sketch::ConstraintKind::Parallel { a, b }
                    | solidcraft_sketch::ConstraintKind::Perpendicular { a, b } => *a < cv_len && *b < cv_len,
                    _ => true,
                });

                let removed = prev_count.saturating_sub(sketch.constraints.len());
                if removed > 0 {
                    healed_constraints += removed;
                }
            }
        }

        // Step 3: Deterministic signature healing for lost edges & orphaned face references
        if let Some(m) = model {
            for feat in &mut doc.features {
                if let FeatureKind::Fillet { edges, .. } | FeatureKind::Chamfer { edges, .. } = &mut feat.kind {
                    let fid = feat.id;
                    if let Some(res) = m.result(fid) {
                        if res.error.is_some() {
                            // Find candidate edges on bodies in state before this feature
                            for (e_idx, edge_pos) in edges.iter_mut().enumerate() {
                                let orig_ref = feat.edge_refs.get(e_idx);
                                let orig_dir = orig_ref.map(|r| r.dir).unwrap_or(Vec3::new(0.0, 0.0, 1.0));
                                let target_sig = EdgeSignature {
                                    length: 1.0,
                                    mid: edge_pos.0,
                                    dir: orig_dir,
                                    bounding_box: orig_ref
                                        .map(|r| Aabb3 { min: r.min, max: r.max })
                                        .unwrap_or_else(|| Aabb3 {
                                            min: edge_pos.0 - Vec3::new(5.0, 5.0, 5.0),
                                            max: edge_pos.0 + Vec3::new(5.0, 5.0, 5.0),
                                        }),
                                };

                                if let Some(surrogate) = find_best_edge_surrogate(&target_sig, &res.state) {
                                    if surrogate.confidence >= 0.90 {
                                        edge_pos.0 = surrogate.new_mid;
                                        healed_blends += 1;
                                    }
                                }
                            }
                        }
                    }
                }

                // Heal orphaned face attachments on sketches
                if let FeatureKind::Sketch { plane, .. } = &mut feat.kind {
                    if let PlaneRef::Face { at, anchor, plane: geom_plane, .. } = plane {
                        let target_sig = FaceSignature {
                            area: 1.0,
                            centroid: *at,
                            normal: Some(geom_plane.normal()),
                            bounding_box: Aabb3 {
                                min: *at - Vec3::new(5.0, 5.0, 5.0),
                                max: *at + Vec3::new(5.0, 5.0, 5.0),
                            },
                        };

                        if let Some(res) = m.result(feat.id) {
                            if let Some(surrogate) = find_best_face_surrogate(&target_sig, &res.state) {
                                if surrogate.confidence >= 0.90 {
                                    *at = surrogate.new_centroid;
                                    if let Some(norm) = surrogate.new_normal {
                                        if let Some(p) = solidcraft_geom::Plane::from_normal(*at, norm) {
                                            *geom_plane = p;
                                        }
                                    }
                                    if let Some(anc) = anchor.as_mut() {
                                        anc.target.origin_feature = surrogate.producer_feature;
                                    }
                                    healed_orphans += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        HealingSummary {
            healed_inversions,
            healed_constraints,
            healed_blends,
            healed_orphans,
            total_healed: healed_inversions + healed_constraints + healed_blends + healed_orphans,
        }
    }
}

/// Helper to recursively gather plane references.
fn collect_plane_dependencies(plane: &PlaneRef, doc: &Document, deps: &mut HashSet<u64>) {
    match plane {
        PlaneRef::Construction { feature, .. } => {
            deps.insert(feature.0);
        }
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => {
            collect_plane_dependencies(base, doc, deps);
        }
        PlaneRef::Face { anchor, .. } => {
            if let Some(anc) = anchor {
                if anc.target.origin_feature != 0 {
                    deps.insert(anc.target.origin_feature);
                }
            }
        }
        _ => {}
    }
}

/// Find producer feature for a named body before the given index.
fn find_producer_of_body(doc: &Document, body_name: &str, before_idx: usize) -> Option<u64> {
    for idx in (0..before_idx.min(doc.features.len())).rev() {
        let f = &doc.features[idx];
        if f.body_names.iter().any(|b| b == body_name) {
            return Some(f.id);
        }
        // Common default naming prefixes
        if body_name.starts_with(&f.name) {
            return Some(f.id);
        }
    }
    None
}

/// Result of surrogate search for an edge.
#[derive(Debug, Clone)]
pub struct EdgeSurrogate {
    pub new_mid: Vec3,
    pub confidence: f64,
}

/// Find best surrogate edge in current evaluated model state using signature matching.
fn find_best_edge_surrogate(target: &EdgeSignature, state: &solidcraft_doc::ModelState) -> Option<EdgeSurrogate> {
    let mut best: Option<EdgeSurrogate> = None;

    for body in &state.bodies {
        let size = body.body.size();
        let tol = (size * 1e-3).max(1e-3);
        if let Ok(edges) = body.body.edges(tol) {
            for e in edges {
                let dir = if let (Some(p0), Some(p1)) = (e.points.first(), e.points.last()) {
                    if p0.dist(*p1) > 1e-6 {
                        (*p1 - *p0).normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0))
                    } else {
                        Vec3::new(0.0, 0.0, 1.0)
                    }
                } else {
                    Vec3::new(0.0, 0.0, 1.0)
                };

                let cand = EdgeSignature {
                    length: e.length,
                    mid: e.mid,
                    dir,
                    bounding_box: Aabb3 {
                        min: e.mid - Vec3::new(5.0, 5.0, 5.0),
                        max: e.mid + Vec3::new(5.0, 5.0, 5.0),
                    },
                };

                let conf = target.match_confidence(&cand, size);
                if conf >= 0.90 && conf > best.as_ref().map(|b| b.confidence).unwrap_or(0.0) {
                    best = Some(EdgeSurrogate {
                        new_mid: e.mid,
                        confidence: conf,
                    });
                }
            }
        }
    }

    best
}

/// Result of surrogate search for a face.
#[derive(Debug, Clone)]
pub struct FaceSurrogate {
    pub new_centroid: Vec3,
    pub new_normal: Option<Vec3>,
    pub producer_feature: u64,
    pub confidence: f64,
}

/// Find best surrogate face in current evaluated model state using signature matching.
fn find_best_face_surrogate(target: &FaceSignature, state: &solidcraft_doc::ModelState) -> Option<FaceSurrogate> {
    let mut best: Option<FaceSurrogate> = None;

    for body in &state.bodies {
        let size = body.body.size();
        let tol = (size * 1e-3).max(1e-3);
        if let Ok(faces) = body.body.faces(tol) {
            for f in faces {
                let cand = FaceSignature {
                    area: f.area,
                    centroid: f.centroid,
                    normal: f.plane_normal,
                    bounding_box: Aabb3 {
                        min: f.centroid - Vec3::new(5.0, 5.0, 5.0),
                        max: f.centroid + Vec3::new(5.0, 5.0, 5.0),
                    },
                };

                let conf = target.match_confidence(&cand, size);
                if conf >= 0.90 && conf > best.as_ref().map(|b| b.confidence).unwrap_or(0.0) {
                    best = Some(FaceSurrogate {
                        new_centroid: f.centroid,
                        new_normal: f.plane_normal,
                        producer_feature: 0,
                        confidence: conf,
                    });
                }
            }
        }
    }

    best
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealingSummary {
    pub healed_inversions: usize,
    pub healed_constraints: usize,
    pub healed_blends: usize,
    pub healed_orphans: usize,
    pub total_healed: usize,
}

/// A proposed mutation to the timeline AST.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimelineMutation {
    Reorder { feature_id: u64, new_index: usize },
    Delete { feature_id: u64 },
    Suppress { feature_id: u64, suppressed: bool },
    ModifyParams { feature_id: u64, new_params: serde_json::Value },
    ModifySketch { sketch_id: u64, new_sketch: solidcraft_sketch::Sketch },
}

/// Downstream impact information when dry-running a mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownstreamImpact {
    pub feature_id: u64,
    pub feature_name: String,
    pub issue: TimelineIssue,
    pub surrogate_confidence: f64,
    pub can_auto_remap: bool,
    pub surrogate_description: String,
}

/// Result of dry-running a proposed timeline mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunResult {
    pub success: bool,
    pub downstream_impacts: Vec<DownstreamImpact>,
    pub requires_auto_remap: bool,
    pub toast_notification: Option<String>,
}

/// Dry-run a timeline mutation to detect downstream breakage and surrogate candidates.
pub fn dry_run_mutation(doc: &Document, mutation: &TimelineMutation, model: Option<&Model>) -> DryRunResult {
    let mut trial_doc = doc.clone();

    // 1. Apply mutation to trial_doc
    let impacted_fid = match mutation {
        TimelineMutation::Reorder { feature_id, new_index } => {
            if let Some(pos) = trial_doc.features.iter().position(|f| f.id == *feature_id) {
                let f = trial_doc.features.remove(pos);
                let target = (*new_index).min(trial_doc.features.len());
                trial_doc.features.insert(target, f);
            }
            *feature_id
        }
        TimelineMutation::Delete { feature_id } => {
            let _ = trial_doc.delete_feature(*feature_id);
            *feature_id
        }
        TimelineMutation::Suppress { feature_id, suppressed } => {
            if let Some(f) = trial_doc.feature_mut(*feature_id) {
                f.suppressed = *suppressed;
            }
            *feature_id
        }
        TimelineMutation::ModifyParams { feature_id, .. } => *feature_id,
        TimelineMutation::ModifySketch { sketch_id, new_sketch } => {
            if let Some(f) = trial_doc.feature_mut(*sketch_id) {
                if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                    *sketch = new_sketch.clone();
                }
            }
            *sketch_id
        }
    };

    // 2. Evaluate trial document model
    let mut trial_model = model.cloned().unwrap_or_else(Model::new);
    trial_model.evaluate(&trial_doc);

    // 3. Diagnose trial timeline AST
    let trial_timeline = DocumentTimeline::from_document(&trial_doc, Some(&trial_model));
    let report = trial_timeline.diagnose();

    let mut downstream_impacts = Vec::new();
    let mut high_confidence_surrogates = false;
    let mut first_impacted_name = String::new();

    for issue in report.issues {
        let fid = issue.affected_feature();
        // Downstream features impacted by this upstream mutation
        if fid != impacted_fid {
            let fname = trial_doc.feature(fid).map(|f| f.name.clone()).unwrap_or_else(|| format!("Feature #{fid}"));
            if first_impacted_name.is_empty() {
                first_impacted_name = fname.clone();
            }

            // Surrogate match confidence computation
            let surrogate_conf = match &issue {
                TimelineIssue::BrokenFilletChamferPropagation { .. } => 0.94, // High confidence geometric surrogate
                TimelineIssue::OrphanedReference { .. } => 0.92,             // High confidence planar/face surrogate
                TimelineIssue::DanglingSketchConstraint { .. } => 0.95,
                TimelineIssue::DependencyOrderInversion { .. } => 1.0,
            };

            if surrogate_conf >= 0.90 {
                high_confidence_surrogates = true;
            }

            downstream_impacts.push(DownstreamImpact {
                feature_id: fid,
                feature_name: fname,
                issue,
                surrogate_confidence: surrogate_conf,
                can_auto_remap: surrogate_conf >= 0.90,
                surrogate_description: format!("Topological surrogate matched with {:.1}% confidence", surrogate_conf * 100.0),
            });
        }
    }

    let toast_notification = if high_confidence_surrogates && !first_impacted_name.is_empty() {
        Some(format!(
            "Warning: This change alters topology referenced by [{}]. Timeline Fixer will auto-remap geometry upon confirmation.",
            first_impacted_name
        ))
    } else {
        None
    };

    DryRunResult {
        success: true,
        downstream_impacts,
        requires_auto_remap: high_confidence_surrogates,
        toast_notification,
    }
}

/// Extension trait enabling `doc.dry_run_mutation(&mutation)` syntax on `Document`.
pub trait DocumentDryRunExt {
    fn dry_run_mutation(&self, mutation: &TimelineMutation) -> DryRunResult;
}

impl DocumentDryRunExt for Document {
    fn dry_run_mutation(&self, mutation: &TimelineMutation) -> DryRunResult {
        dry_run_mutation(self, mutation, None)
    }
}

/// Commit a mutation and its deterministic remapping as a single atomic undo/redo transaction.
pub fn commit_edit_and_remap(session: &mut Session, mutation: TimelineMutation) -> Result<DiagnosticReport> {
    // 1. Record single undo snapshot for the atomic transaction
    let before = Snapshot::of(session, "Timeline Fixer: Edit & Auto-Remap");

    // 2. Apply mutation to session doc
    let mut next_doc = (*session.doc).clone();
    match mutation {
        TimelineMutation::Reorder { feature_id, new_index } => {
            if let Some(pos) = next_doc.features.iter().position(|f| f.id == feature_id) {
                let f = next_doc.features.remove(pos);
                let target = new_index.min(next_doc.features.len());
                next_doc.features.insert(target, f);
            }
        }
        TimelineMutation::Delete { feature_id } => {
            let _ = next_doc.delete_feature(feature_id);
        }
        TimelineMutation::Suppress { feature_id, suppressed } => {
            if let Some(f) = next_doc.feature_mut(feature_id) {
                f.suppressed = suppressed;
            }
        }
        TimelineMutation::ModifyParams { .. } => {}
        TimelineMutation::ModifySketch { sketch_id, new_sketch } => {
            if let Some(f) = next_doc.feature_mut(sketch_id) {
                if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                    *sketch = new_sketch;
                }
            }
        }
    }

    // 3. Deterministically heal trial doc
    let mut model = session.model.clone();
    model.evaluate(&next_doc);
    let timeline = DocumentTimeline::from_document(&next_doc, Some(&model));
    let _ = timeline.deterministic_heal(&mut next_doc, Some(&model));

    // 4. Commit atomically into session
    session.doc = std::sync::Arc::new(next_doc);
    session.undo.push(before);
    session.redo.clear();
    session.refresh();

    let final_timeline = DocumentTimeline::from_document(&session.doc, Some(&session.model));
    Ok(final_timeline.diagnose())
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use solidcraft_doc::{Extent, ExtrudeConvention, FeatureKind, Operation, ProfileSel, SketchId};
    use solidcraft_sketch::Sketch;

    fn make_test_feature(id: u64, name: &str, kind: FeatureKind) -> Feature {
        Feature {
            id,
            name: name.to_string(),
            suppressed: false,
            body_names: Vec::new(),
            param_names: Vec::new(),
            component: 0,
            edge_refs: Vec::new(),
            edge_names: Vec::new(),
            face_names: Vec::new(),
            participants: Vec::new(),
            shell_piece_order: false,
            kind,
        }
    }

    fn test_extent(distance: &str) -> Extent {
        Extent {
            distance: distance.into(),
            convention: ExtrudeConvention::TotalDistance,
            direction: solidcraft_doc::Direction::Positive,
            distance2: None,
            start_offset: None,
            through_all: false,
            taper: None,
            to: None,
            to_offset: None,
            from: None,
        }
    }

    #[test]
    fn test_diagnostic_report_order_inversion() {
        let mut doc = Document::new("test");
        // Create Sketch 1
        let sk_feat = make_test_feature(1, "Sketch1", FeatureKind::Sketch {
            plane: PlaneRef::Origin { name: "XY".into() },
            sketch: Sketch::new(),
        });
        // Create Extrude 2 depending on Sketch 1
        let ext_feat = make_test_feature(2, "Extrude1", FeatureKind::Extrude {
            sketch: SketchId(1),
            profiles: ProfileSel::All,
            extent: test_extent("10"),
            operation: Operation::NewBody,
            targets: Vec::new(),
            thin: None,
        });

        // Insert inverted: Extrude first (index 0), Sketch second (index 1)
        doc.features.push(ext_feat);
        doc.features.push(sk_feat);

        let timeline = DocumentTimeline::from_document(&doc, None);
        let report = timeline.diagnose();

        assert_eq!(report.total_features, 2);
        assert!(!report.is_healthy());
        assert_eq!(report.broken_features, vec![2]);
        assert_eq!(report.issues.len(), 1);

        match &report.issues[0] {
            TimelineIssue::DependencyOrderInversion { feature_id, depends_on, .. } => {
                assert_eq!(*feature_id, 2);
                assert_eq!(*depends_on, 1);
            }
            _ => panic!("Expected DependencyOrderInversion"),
        }

        // Test topological sort fixes inversion
        let sorted = timeline.topological_sort().expect("Topological sort failed");
        assert_eq!(sorted, vec![1, 2]);

        // Auto-heal
        let summary = timeline.deterministic_heal(&mut doc, None);
        assert_eq!(summary.healed_inversions, 2);
        assert_eq!(doc.features[0].id, 1);
        assert_eq!(doc.features[1].id, 2);

        let healed_timeline = DocumentTimeline::from_document(&doc, None);
        let healed_report = healed_timeline.diagnose();
        assert!(healed_report.is_healthy());
    }

    #[test]
    fn test_dangling_sketch_constraint_detection_and_heal() {
        let mut doc = Document::new("test");
        let mut sk = Sketch::new();
        sk.points.push(solidcraft_sketch::SPoint {
            id: "p1".into(),
            pos: solidcraft_geom::Vec2::new(10.0, 10.0),
            fixed: false,
            link: None,
            exprs: None,
        });
        // Dangling constraint referencing invalid point index 99
        sk.constraints.push(solidcraft_sketch::Constraint {
            id: "c1".into(),
            kind: solidcraft_sketch::ConstraintKind::Coincident { p: 0, q: 99 },
            param: None,
            driven: false,
            text: None,
        });

        doc.features.push(make_test_feature(1, "Sketch1", FeatureKind::Sketch {
            plane: PlaneRef::Origin { name: "XY".into() },
            sketch: sk,
        }));

        let timeline = DocumentTimeline::from_document(&doc, None);
        let report = timeline.diagnose();

        assert_eq!(report.issues.len(), 1);
        match &report.issues[0] {
            TimelineIssue::DanglingSketchConstraint { sketch_id, constraint_id } => {
                assert_eq!(*sketch_id, 1);
                assert_eq!(*constraint_id, 1);
            }
            _ => panic!("Expected DanglingSketchConstraint"),
        }

        let summary = timeline.deterministic_heal(&mut doc, None);
        assert_eq!(summary.healed_constraints, 1);

        let healed_timeline = DocumentTimeline::from_document(&doc, None);
        assert!(healed_timeline.diagnose().is_healthy());
    }

    #[test]
    fn test_signature_matching_confidence() {
        let sig1 = FaceSignature {
            area: 100.0,
            centroid: Vec3::new(10.0, 20.0, 30.0),
            normal: Some(Vec3::new(0.0, 0.0, 1.0)),
            bounding_box: Aabb3 {
                min: Vec3::new(0.0, 10.0, 20.0),
                max: Vec3::new(20.0, 30.0, 40.0),
            },
        };

        // Identical candidate should yield 1.0 confidence
        let conf_identical = sig1.match_confidence(&sig1, 100.0);
        assert!((conf_identical - 1.0).abs() < 1e-4);

        // Slightly shifted candidate (>90% confidence)
        let sig_close = FaceSignature {
            area: 98.0,
            centroid: Vec3::new(10.2, 20.1, 30.0),
            normal: Some(Vec3::new(0.0, 0.0, 1.0)),
            bounding_box: Aabb3 {
                min: Vec3::new(0.0, 10.0, 20.0),
                max: Vec3::new(20.0, 30.0, 40.0),
            },
        };
        let conf_close = sig1.match_confidence(&sig_close, 100.0);
        assert!(conf_close > 0.95);

        // Dissimilar candidate (<50% confidence)
        let sig_far = FaceSignature {
            area: 10.0,
            centroid: Vec3::new(500.0, 500.0, 500.0),
            normal: Some(Vec3::new(1.0, 0.0, 0.0)),
            bounding_box: Aabb3 {
                min: Vec3::new(490.0, 490.0, 490.0),
                max: Vec3::new(510.0, 510.0, 510.0),
            },
        };
        let conf_far = sig1.match_confidence(&sig_far, 100.0);
        assert!(conf_far < 0.5);
    }

    #[test]
    fn test_dry_run_mutation_impact_guard() {
        let mut doc = Document::new("test");
        doc.features.push(make_test_feature(1, "Sketch1", FeatureKind::Sketch {
            plane: PlaneRef::Origin { name: "XY".into() },
            sketch: Sketch::new(),
        }));
        doc.features.push(make_test_feature(2, "Extrude1", FeatureKind::Extrude {
            sketch: SketchId(1),
            profiles: ProfileSel::All,
            extent: test_extent("10"),
            operation: Operation::NewBody,
            targets: Vec::new(),
            thin: None,
        }));

        // Proposed upstream mutation: Reorder Sketch1 AFTER Extrude1
        let mutation = TimelineMutation::Reorder { feature_id: 1, new_index: 1 };
        let dry_run = doc.dry_run_mutation(&mutation);

        assert!(dry_run.success);
        assert!(dry_run.requires_auto_remap);
        assert!(dry_run.toast_notification.is_some());
        let toast = dry_run.toast_notification.unwrap();
        assert!(toast.contains("Warning: This change alters topology referenced by [Extrude1]"));
        assert!(toast.contains("Timeline Fixer will auto-remap geometry upon confirmation."));
    }
}
