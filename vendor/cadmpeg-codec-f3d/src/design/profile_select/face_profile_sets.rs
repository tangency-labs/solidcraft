// SPDX-License-Identifier: Apache-2.0
//! Infer an Extrude's face profile from its own transition when face recipes
//! leave members unresolved.
//!
//! A blind Extrude from planar faces inserts, at its length from their plane,
//! planar end caps that repeat the swept region: the floor of a cut or the
//! top of a join. The preceding faces on that plane whose interiors lie
//! inside the caps are the profile when they are as many as the group's
//! members, include every resolved face, cover the caps' area, and have an
//! outline on the caps' boundary through every cap vertex, and when no other
//! plane, side or face that could be the profile contradicts that set. A
//! face could be the profile when it is resolved, when the transition
//! deletes or updates it, or when the Extrude makes a new body. When no cap
//! selects such a face, the faces that the transition deletes are the profile
//! when they are as many as the group's members, lie on the plane of the
//! resolved members and include the resolved faces. When both rules give a
//! set, the two sets agree or the profile stays native. A face with several
//! surface bindings, no surface binding or several plane rows, and an
//! inserted face on a surface without geometry rows, keep the profile native.
//! A preceding face on a surface without geometry rows counts on every plane
//! that holds all its vertices, as a face that cannot be read.
//!
//! Resolved members keep their faces at their member positions. The other
//! faces fill the remaining positions in face-slot order.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::loss::LossNote;

use super::cap_regions::{
    face_cap, face_outlines_cover_caps, oriented_loops, relation, Cap, PlaneFrame, AREA_AGREEMENT,
    CAP_EDGES, PARALLEL_CAP,
};
use crate::design::face_resolve::{
    extrude_profile_group_operand_indices, extrude_profile_group_roots,
    historical_face_selection_with_native,
};
use crate::design::geometry::{point_in_polygon, point_segment_distance};
use crate::history_records::{AsmHistoricalPlane, AsmHistoricalTopology, AsmHistory};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, extrude_selection::DesignExtrudeOperandRole,
    face::DesignFaceOperand,
};

/// Grid lines per axis searched for a point well inside a face.
const FACE_SAMPLE_GRID: u32 = 24;
/// How many linear tolerances a face's sample point keeps from its boundary.
const FACE_CLEARANCE: f64 = 10.0;

/// Local work caps of one cap search.
#[derive(Clone, Copy)]
struct SearchLimits {
    /// Most preceding faces on one candidate plane.
    pool_faces: usize,
    /// Most polyline points in the faces one candidate plane selects.
    outline_points: usize,
    /// Most loop edges of one cap or preceding face.
    face_edges: usize,
    /// Most inserted cap faces on one candidate plane.
    cap_faces: usize,
}

const SEARCH_LIMITS: SearchLimits = SearchLimits {
    pool_faces: 256,
    outline_points: 4096,
    face_edges: CAP_EDGES,
    cap_faces: 256,
};

/// The arenas that infer Extrude face profiles.
pub(crate) struct FaceProfileSetResolution<'a> {
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a [DesignFaceOperand],
    pub(crate) histories: &'a [AsmHistory],
    pub(crate) scope_histories: &'a HashMap<String, String>,
    pub(crate) linear_tolerance: f64,
    pub(crate) ctx: &'a DecodeContext<'a>,
}

/// The topologies and counts one end-cap search reads.
struct CapSearch<'a> {
    preceding: &'a AsmHistoricalTopology,
    topology: &'a AsmHistoricalTopology,
    inserted: &'a [i64],
    length: f64,
    members: usize,
    resolved: &'a [i64],
    /// The faces the transition deletes or updates.
    changed: &'a HashSet<i64>,
    /// Whether every preceding face could be the profile.
    new_body: bool,
    tolerance: f64,
    limits: SearchLimits,
}

/// What the inserted caps state about the profile faces.
#[derive(Debug, PartialEq)]
enum CapReading {
    /// One face set, sorted by slot, and the candidates it passes over.
    Selected(Vec<i64>, PassedOver),
    /// Why no face set holds: a face carrier cannot be read, an inserted
    /// planar face has no unit normal, a selection that could be the profile
    /// fails its checks, two such sets differ, or a cap over such faces or
    /// such a face that reaches into a cap is unreadable.
    Disputed(&'static str),
    /// No cap selects a preceding face set that could be the profile, and
    /// the candidates the search passes over.
    Absent(PassedOver),
    /// The search passed a local work cap.
    Capped,
}

/// The candidates that a cap search passes over.
#[derive(Debug, Default, PartialEq)]
struct PassedOver {
    /// Readable faces at the blind length from a cap plane whose interior
    /// lies outside the caps.
    outside: Vec<i64>,
    /// Faces at the blind length from a cap plane that cannot be read and
    /// stay clear of the caps.
    clear: Vec<i64>,
    /// Planes and sides at the blind length whose faces or selection no
    /// member resolves and the transition neither deletes nor updates.
    skipped: usize,
}

/// The faces that a transition deletes on the plane of the resolved faces,
/// with the coplanar preceding faces that it updates and keeps.
#[derive(Debug, PartialEq)]
struct DeletedFaces {
    faces: Vec<i64>,
    updated: Vec<i64>,
    kept: Vec<i64>,
}

/// The rule that infers a face set.
#[derive(Clone, Copy)]
enum SetRule {
    EndCaps,
    DeletedFaces,
}

impl PassedOver {
    fn describe(&self) -> String {
        format!(
            "The end-cap search passes over faces {:?} outside the caps, unreadable faces {:?} clear of the caps, and {} planes or sides whose faces no member resolves and the transition neither deletes nor updates.",
            self.outside, self.clear, self.skipped,
        )
    }
}

impl SetRule {
    fn name(self) -> &'static str {
        match self {
            Self::EndCaps => "end caps",
            Self::DeletedFaces => "deleted faces",
        }
    }
}

fn disputed(group: &str, reason: &str) -> LossNote {
    F3dLossCode::FeatureExtrudeProfileFaceSetDisputed.note(format!(
        "Extrude profile group {group} stays native: {reason}."
    ))
}

/// Replace each native single-group Extrude face profile whose set its own
/// transition infers with the inferred historical faces.
pub(crate) fn bind_extrude_face_profile_sets(
    features: &mut [cadmpeg_ir::features::Feature],
    resolution: &FaceProfileSetResolution<'_>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for feature in features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| resolution.scopes.iter().find(|scope| scope.id == id))
        else {
            continue;
        };
        let mut outcome = Ok(Vec::new());
        feature.evaluation.edit(|definition, _| {
            outcome = bind_one(definition, scope, resolution, SEARCH_LIMITS);
        });
        losses.extend(outcome?);
    }
    Ok(losses)
}

fn bind_one(
    definition: &mut cadmpeg_ir::features::FeatureDefinition,
    scope: &DesignParameterScope,
    resolution: &FaceProfileSetResolution<'_>,
    limits: SearchLimits,
) -> Result<Vec<LossNote>, CodecError> {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection,
        FeatureDefinition, FeatureOperation, LinearTermination, PlanarProfileRef, ProfileRef,
    };

    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile,
        direction: ExtrudeDirection::ProfileNormal {} | ExtrudeDirection::ReversedProfileNormal {},
        start: ExtrudeStart::ProfilePlane {},
        extent:
            ExtrudeExtent::OneSided {
                side:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length },
                        draft: None,
                    },
            },
        op,
        ..
    }) = definition
    else {
        return Ok(Vec::new());
    };
    let ProfileRef::Planar(PlanarProfileRef::Native(native)) = profile else {
        return Ok(Vec::new());
    };
    let ctx = resolution.ctx;
    let Some(roots) = extrude_profile_group_roots(ctx, scope, resolution.groups)? else {
        return Ok(Vec::new());
    };
    let [root] = roots.as_slice() else {
        return Ok(Vec::new());
    };
    if root.id != *native
        || native_stream(&root.id) != native_stream(&scope.id)
        || root.extrude_role() != Some(DesignExtrudeOperandRole::Profile)
    {
        return Ok(Vec::new());
    }
    let Some(indices) =
        extrude_profile_group_operand_indices(ctx, root, resolution.groups, resolution.operands)?
    else {
        return Ok(Vec::new());
    };
    let mut members = Vec::new();
    for index in indices {
        let Some(operand) = resolution.operands.get(index) else {
            return Ok(Vec::new());
        };
        match (
            operand.resolved_face_slots.as_slice(),
            &operand.resolved_active_face,
        ) {
            ([], None) => members.push(None),
            ([slot], None) => members.push(Some(*slot)),
            _ => return Ok(Vec::new()),
        }
    }
    if members.iter().all(Option::is_some) {
        return Ok(Vec::new());
    }
    let resolved: Vec<i64> = members.iter().flatten().copied().collect();
    if (1..resolved.len()).any(|k| resolved[..k].contains(&resolved[k])) {
        return Ok(Vec::new());
    }
    let (Some(state_id), Some(previous_state_id)) =
        (scope.history_state_id(), scope.previous_history_state_id())
    else {
        return Ok(Vec::new());
    };
    let histories =
        super::histories_for_scope(&scope.id, resolution.scope_histories, resolution.histories);
    let Some((history, state, previous_state)) =
        crate::history::unique_history_state_pair(histories, state_id, previous_state_id)
    else {
        return Ok(Vec::new());
    };
    let Some(transition) = state
        .transition
        .as_ref()
        .filter(|transition| transition.previous_state_id == Some(previous_state_id))
    else {
        return Ok(Vec::new());
    };
    let (Some(topology), Some(preceding)) =
        (history.topology(state), history.topology(previous_state))
    else {
        return Ok(Vec::new());
    };
    let tolerance = resolution.linear_tolerance;
    let changes = &transition.topology.faces;
    let changed: HashSet<i64> = changes
        .deleted
        .iter()
        .chain(&changes.updated)
        .copied()
        .collect();
    let group = root.id.clone();
    let deleted = deleted_profile_faces(
        preceding,
        (&changes.deleted, &changes.updated),
        members.len(),
        &resolved,
        tolerance,
    );
    let (rule, faces, inference) = match cap_profile_faces(&CapSearch {
        preceding,
        topology,
        inserted: &changes.inserted,
        length: length.abs().get(),
        members: members.len(),
        resolved: &resolved,
        changed: &changed,
        new_body: *op == BooleanOp::NewBody,
        tolerance,
        limits,
    }) {
        CapReading::Selected(faces, passed) => {
            if let Some(other) = deleted.filter(|other| other.faces != faces) {
                return Ok(vec![disputed(
                    &group,
                    &format!(
                        "the faces {:?} that its transition deletes differ from the faces {faces:?} that its end caps cover",
                        other.faces
                    ),
                )]);
            }
            let note = F3dLossCode::FeatureExtrudeProfileCapFacesInferred.note(format!(
                "Extrude profile group {group} takes faces {faces:?}: the transition's end caps at the blind length cover these preceding faces. {}",
                passed.describe(),
            ));
            (SetRule::EndCaps, faces, note)
        }
        CapReading::Absent(passed) => {
            let Some(deleted) = deleted else {
                return Ok(Vec::new());
            };
            let note = F3dLossCode::FeatureExtrudeProfileDeletedFacesInferred.note(format!(
                "Extrude profile group {group} takes faces {:?}: no end cap selects a face set that a member resolves or the transition deletes or updates, and the transition deletes these faces, one per member, on the plane of the resolved faces {resolved:?}. It passes over the coplanar faces {:?} that the transition updates and {:?} that it keeps. {}",
                deleted.faces, deleted.updated, deleted.kept, passed.describe(),
            ));
            (SetRule::DeletedFaces, deleted.faces, note)
        }
        CapReading::Disputed(reason) => return Ok(vec![disputed(&group, reason)]),
        CapReading::Capped => {
            return Ok(vec![F3dLossCode::FeatureExtrudeProfileFaceSearchCapped.note(
                format!(
                    "Extrude profile group {group} stays native: its end-cap search passed a local cap of {} faces or {} caps on one plane, {} outline points, or {} edges on one face.",
                    limits.pool_faces, limits.cap_faces, limits.outline_points, limits.face_edges,
                ),
            )]);
        }
    };
    let Some(ordered) = member_order(&members, &faces) else {
        return Ok(vec![disputed(
            &group,
            &format!("its inferred faces {faces:?} do not fill its members"),
        )]);
    };
    let Some(FaceSelection::Historical {
        state,
        faces,
        native: selection_native,
    }) = historical_face_selection_with_native(scope, previous_state_id, ordered, root.id.clone())
    else {
        return Ok(vec![disputed(
            &group,
            "its inferred faces form no historical face selection",
        )]);
    };
    let Ok(native_ids) = vec![selection_native.as_str().to_owned()].try_into() else {
        return Ok(Vec::new());
    };
    let unresolved = members.iter().filter(|member| member.is_none()).count();
    *profile = ProfileRef::Planar(PlanarProfileRef::HistoricalFaces {
        state,
        faces,
        native: native_ids,
    });
    let mut losses = vec![inference];
    if unresolved > 1 {
        losses.push(F3dLossCode::FeatureExtrudeProfileFaceOrderAssumed.note(format!(
            "Extrude profile group {group}: the transition's {} give its {} faces; its {unresolved} unresolved members take the faces left after the resolved members in face-slot order.",
            rule.name(),
            members.len(),
        )));
    }
    Ok(losses)
}

/// Resolved members' faces at their positions, then the other `faces` in
/// slot order at the remaining positions. `None` when the counts disagree.
fn member_order(members: &[Option<i64>], faces: &[i64]) -> Option<Vec<i64>> {
    let mut rest: Vec<i64> = faces
        .iter()
        .copied()
        .filter(|face| !members.contains(&Some(*face)))
        .collect();
    rest.sort_unstable();
    let mut rest = rest.into_iter();
    let ordered = members
        .iter()
        .map(|member| member.or_else(|| rest.next()))
        .collect::<Option<Vec<_>>>()?;
    rest.next().is_none().then_some(ordered)
}

/// What one face's surface binding states about its plane.
enum Carrier<'a> {
    /// One binding to a surface with one plane row.
    Plane(&'a AsmHistoricalPlane),
    /// One binding to a surface with a cylinder, axis or radius row and no
    /// plane row.
    Curved,
    /// One binding to a surface without any geometry row.
    Unknown,
    /// Several bindings, no binding, or several plane rows.
    Unreadable,
}

/// The surface carrier of each face of one topology.
struct FacePlanes<'a> {
    surfaces: HashMap<i64, Option<i64>>,
    planes: HashMap<i64, Option<&'a AsmHistoricalPlane>>,
    curved: HashSet<i64>,
}

impl<'a> FacePlanes<'a> {
    fn of(topology: &'a AsmHistoricalTopology) -> Self {
        let mut planes: HashMap<i64, Option<&AsmHistoricalPlane>> = HashMap::new();
        for plane in &topology.surface_planes {
            planes
                .entry(plane.surface)
                .and_modify(|unique| *unique = None)
                .or_insert(Some(plane));
        }
        let mut surfaces: HashMap<i64, Option<i64>> = HashMap::new();
        for binding in &topology.face_surfaces {
            surfaces
                .entry(binding.entity)
                .and_modify(|surface| *surface = None)
                .or_insert(Some(binding.carrier));
        }
        let curved = topology
            .surface_cylinders
            .iter()
            .map(|row| row.surface)
            .chain(topology.surface_axes.iter().map(|row| row.surface))
            .chain(topology.surface_radii.iter().map(|row| row.surface))
            .collect();
        Self {
            surfaces,
            planes,
            curved,
        }
    }

    fn carrier(&self, face: i64) -> Carrier<'a> {
        let Some(Some(surface)) = self.surfaces.get(&face) else {
            return Carrier::Unreadable;
        };
        match self.planes.get(surface) {
            Some(Some(plane)) => Carrier::Plane(plane),
            Some(None) => Carrier::Unreadable,
            None if self.curved.contains(surface) => Carrier::Curved,
            None => Carrier::Unknown,
        }
    }

    fn get(&self, face: i64) -> Option<&'a AsmHistoricalPlane> {
        match self.carrier(face) {
            Carrier::Plane(plane) => Some(plane),
            Carrier::Curved | Carrier::Unknown | Carrier::Unreadable => None,
        }
    }

    /// Whether `face` lies at `height` from `frame`: its plane row does, or
    /// its surface has no geometry row and every vertex it has does.
    fn at_height(
        &self,
        topology: &AsmHistoricalTopology,
        face: i64,
        frame: &PlaneFrame,
        height: f64,
        tolerance: f64,
    ) -> bool {
        match self.carrier(face) {
            Carrier::Plane(plane) => {
                parallel(plane.normal, frame)
                    && (frame.height(plane.origin) - height).abs() <= tolerance
            }
            Carrier::Unknown => oriented_loops(topology, face).is_none_or(|loops| {
                loops
                    .iter()
                    .flatten()
                    .all(|(_, from, _)| (frame.height(*from) - height).abs() <= tolerance)
            }),
            Carrier::Curved | Carrier::Unreadable => false,
        }
    }
}

fn parallel(normal: Vector3, frame: &PlaneFrame) -> bool {
    normal
        .unit()
        .is_some_and(|normal| (normal.dot(frame.normal()).abs() - 1.0).abs() <= PARALLEL_CAP)
}

/// What the inserted caps at the search length from a plane state about a
/// set of preceding faces, one per member, that holds every resolved face.
fn cap_profile_faces(search: &CapSearch<'_>) -> CapReading {
    let &CapSearch {
        preceding,
        topology,
        inserted,
        length,
        members,
        resolved,
        changed,
        new_body,
        tolerance,
        limits,
    } = search;
    let could_be_profile = |faces: &[i64]| {
        new_body
            || faces
                .iter()
                .any(|face| resolved.contains(face) || changed.contains(face))
    };
    let oversized = |topology: &AsmHistoricalTopology, face: i64| {
        edge_count(topology, face).is_some_and(|edges| edges > limits.face_edges)
    };
    let preceding_planes = FacePlanes::of(preceding);
    let current_planes = FacePlanes::of(topology);
    if inserted.iter().any(|face| {
        matches!(
            current_planes.carrier(*face),
            Carrier::Unknown | Carrier::Unreadable
        )
    }) || preceding
        .faces
        .iter()
        .any(|face| matches!(preceding_planes.carrier(*face), Carrier::Unreadable))
    {
        return CapReading::Disputed(
            "a face has several surface bindings, no surface binding or several plane rows, or an inserted face has a surface without geometry rows",
        );
    }
    let planar: Vec<(i64, &AsmHistoricalPlane)> = inserted
        .iter()
        .filter_map(|face| current_planes.get(*face).map(|plane| (*face, plane)))
        .collect();
    let mut grouped = vec![false; planar.len()];
    let mut selections: Vec<Vec<i64>> = Vec::new();
    let mut passed = PassedOver::default();
    for first in 0..planar.len() {
        if grouped[first] {
            continue;
        }
        let Some(frame) = PlaneFrame::of_plane(planar[first].1.origin, planar[first].1.normal)
        else {
            return CapReading::Disputed("an inserted planar face has no unit normal");
        };
        let mut cap_faces = Vec::new();
        for (index, (face, plane)) in planar.iter().enumerate() {
            if !grouped[index]
                && parallel(plane.normal, &frame)
                && frame.height(plane.origin).abs() <= tolerance
            {
                grouped[index] = true;
                cap_faces.push(*face);
            }
        }
        for height in [length, -length] {
            let pool: Vec<i64> = preceding
                .faces
                .iter()
                .copied()
                .filter(|face| {
                    preceding_planes.at_height(preceding, *face, &frame, height, tolerance)
                })
                .collect();
            if pool.is_empty() {
                continue;
            }
            if !could_be_profile(&pool) {
                passed.skipped += 1;
                continue;
            }
            if pool.len() > limits.pool_faces
                || cap_faces.len() > limits.cap_faces
                || cap_faces.iter().any(|face| oversized(topology, *face))
            {
                return CapReading::Capped;
            }
            let Some(caps) = cap_faces
                .iter()
                .map(|face| face_cap(&frame, topology, *face, tolerance))
                .collect::<Option<Vec<Cap>>>()
            else {
                return CapReading::Disputed(
                    "a cap over faces that can be the profile cannot be read",
                );
            };
            let mut selected = Vec::new();
            let mut outlines = Vec::new();
            let mut unread_in_cap = false;
            let mut unread_oversized = false;
            for face in pool {
                if oversized(preceding, face) {
                    if could_be_profile(&[face]) {
                        return CapReading::Capped;
                    }
                    unread_oversized = true;
                    continue;
                }
                let outline = preceding_planes
                    .get(face)
                    .and_then(|_| face_cap(&frame, preceding, face, tolerance));
                match outline
                    .as_ref()
                    .and_then(|outline| interior_point(outline, tolerance))
                {
                    Some(point) => {
                        if caps.iter().any(|cap| cap.contains(point)) {
                            selected.push(face);
                            outlines.extend(outline);
                        } else {
                            passed.outside.push(face);
                        }
                    }
                    None => {
                        let rings = match outline {
                            Some(outline) => Some(outline.loops),
                            None => face_vertex_rings(&frame, preceding, face),
                        };
                        if rings.is_none_or(|rings| reaches_a_cap(&rings, &caps, tolerance)) {
                            if could_be_profile(&[face]) {
                                return CapReading::Disputed(
                                    "a face that can be the profile cannot be read and reaches into a cap",
                                );
                            }
                            unread_in_cap = true;
                        } else {
                            passed.clear.push(face);
                        }
                    }
                }
            }
            if selected.is_empty() {
                continue;
            }
            if !could_be_profile(&selected) {
                passed.skipped += 1;
                continue;
            }
            if unread_oversized {
                return CapReading::Capped;
            }
            if unread_in_cap {
                return CapReading::Disputed(
                    "a face that cannot be read reaches into the caps of a selection that can be the profile",
                );
            }
            let points: usize = outlines
                .iter()
                .flat_map(|outline| outline.loops.iter())
                .map(Vec::len)
                .sum();
            if points > limits.outline_points {
                return CapReading::Capped;
            }
            let cap_area: f64 = caps.iter().map(|cap| cap.area).sum();
            let selected_area: f64 = outlines.iter().map(|outline| outline.area).sum();
            if selected.len() != members
                || !resolved.iter().all(|face| selected.contains(face))
                || (selected_area - cap_area).abs() > AREA_AGREEMENT * cap_area.max(1.0)
                || !face_outlines_cover_caps(&caps, &outlines, tolerance)
            {
                return CapReading::Disputed(
                    "a selection that can be the profile fails the member count, resolved face, area or outline check",
                );
            }
            selected.sort_unstable();
            if !selections.contains(&selected) {
                selections.push(selected);
            }
        }
    }
    passed.outside.sort_unstable();
    passed.outside.dedup();
    passed.clear.sort_unstable();
    passed.clear.dedup();
    let mut selections = selections.into_iter();
    match (selections.next(), selections.next()) {
        (Some(faces), None) => CapReading::Selected(faces, passed),
        (None, _) => CapReading::Absent(passed),
        (Some(_), Some(_)) => CapReading::Disputed("two selections that can be the profile differ"),
    }
}

/// The deleted faces, sorted by slot, when they are as many as the members,
/// are preceding faces, hold every resolved face, and all lie on the
/// resolved faces' plane, with the other preceding faces on that plane that
/// the transition updates and keeps.
fn deleted_profile_faces(
    preceding: &AsmHistoricalTopology,
    (deleted, updated): (&[i64], &[i64]),
    members: usize,
    resolved: &[i64],
    tolerance: f64,
) -> Option<DeletedFaces> {
    let mut faces = deleted.to_vec();
    faces.sort_unstable();
    faces.dedup();
    if faces.len() != deleted.len()
        || faces.len() != members
        || !resolved.iter().all(|face| faces.contains(face))
        || !faces.iter().all(|face| preceding.faces.contains(face))
    {
        return None;
    }
    let planes = FacePlanes::of(preceding);
    let plane = planes.get(*resolved.first()?)?;
    let frame = PlaneFrame::of_plane(plane.origin, plane.normal)?;
    if !faces.iter().all(|face| {
        planes.get(*face).is_some() && planes.at_height(preceding, *face, &frame, 0.0, tolerance)
    }) {
        return None;
    }
    let (mut changed, mut kept): (Vec<i64>, Vec<i64>) = preceding
        .faces
        .iter()
        .copied()
        .filter(|face| {
            !faces.contains(face) && planes.at_height(preceding, *face, &frame, 0.0, tolerance)
        })
        .partition(|face| updated.contains(face));
    changed.sort_unstable();
    kept.sort_unstable();
    Some(DeletedFaces {
        faces,
        updated: changed,
        kept,
    })
}

/// The vertex positions of each loop of `face` in `frame`, in loop order.
fn face_vertex_rings(
    frame: &PlaneFrame,
    topology: &AsmHistoricalTopology,
    face: i64,
) -> Option<Vec<Vec<Point2>>> {
    Some(
        oriented_loops(topology, face)?
            .into_iter()
            .map(|steps| {
                steps
                    .into_iter()
                    .map(|(_, from, _)| frame.project(from))
                    .collect()
            })
            .collect(),
    )
}

/// The loop edges of `face`, when its loops are readable.
fn edge_count(topology: &AsmHistoricalTopology, face: i64) -> Option<usize> {
    relation(&topology.face_loops, face)?
        .iter()
        .try_fold(0usize, |edges, loop_| {
            edges.checked_add(relation(&topology.loop_coedges, *loop_)?.len())
        })
}

/// Whether a face drawn by its rings reaches into a cap: a ring point is not
/// finite, no ring has three points, a ring point lies inside a cap away from its boundary, a cap
/// vertex lies inside the rings away from them, or the mean of the largest
/// ring's points or a point of a grid over the rings' bounds lies inside
/// both, away from their boundaries.
fn reaches_a_cap(rings: &[Vec<Point2>], caps: &[Cap], tolerance: f64) -> bool {
    if rings
        .iter()
        .flatten()
        .any(|point| !point.u.is_finite() || !point.v.is_finite())
    {
        return true;
    }
    let inside_a_cap = |point: Point2| {
        caps.iter()
            .any(|cap| cap.contains(point) && cap.boundary_distance(point) > tolerance)
    };
    let ring_area = |ring: &[Point2]| {
        cadmpeg_ir::math::planar::polygon_area_twice(ring).map_or(0.0, |area| area.get().abs())
    };
    let Some(largest) = rings
        .iter()
        .filter(|ring| ring.len() >= 3)
        .max_by(|a, b| ring_area(a).total_cmp(&ring_area(b)))
    else {
        return true;
    };
    let (u, v, count) = largest
        .iter()
        .fold((0.0, 0.0, 0.0), |(u, v, count), point| {
            (u + point.u, v + point.v, count + 1.0)
        });
    let mean = Point2::new(u / count, v / count);
    let inside_the_face = |point: Point2| {
        let parity = rings
            .iter()
            .filter(|ring| point_in_polygon(point, ring))
            .count()
            % 2
            == 1;
        let clearance = rings
            .iter()
            .flat_map(|ring| (0..ring.len()).map(move |k| (ring[k], ring[(k + 1) % ring.len()])))
            .map(|segment| point_segment_distance(point, segment))
            .fold(f64::INFINITY, f64::min);
        parity && clearance > tolerance
    };
    let (low, high) = bounds(rings.iter().flatten());
    let grid = (1..FACE_SAMPLE_GRID).flat_map(|i| {
        (1..FACE_SAMPLE_GRID).map(move |j| {
            Point2::new(
                low.u + (high.u - low.u) * f64::from(i) / f64::from(FACE_SAMPLE_GRID),
                low.v + (high.v - low.v) * f64::from(j) / f64::from(FACE_SAMPLE_GRID),
            )
        })
    });
    std::iter::once(mean)
        .chain(grid)
        .any(|point| inside_the_face(point) && inside_a_cap(point))
        || rings.iter().flatten().any(|point| inside_a_cap(*point))
        || caps
            .iter()
            .flat_map(|cap| cap.corners.iter())
            .any(|corner| inside_the_face(*corner))
}

/// The lower and upper corners of the bounds of `points`.
fn bounds<'a>(points: impl Iterator<Item = &'a Point2>) -> (Point2, Point2) {
    points.fold(
        (
            Point2::new(f64::INFINITY, f64::INFINITY),
            Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        ),
        |(low, high), point| {
            (
                Point2::new(low.u.min(point.u), low.v.min(point.v)),
                Point2::new(high.u.max(point.u), high.v.max(point.v)),
            )
        },
    )
}

/// A point inside `face`, outside its holes, farthest from its boundary on a
/// grid over its loops' bounds.
fn interior_point(face: &Cap, tolerance: f64) -> Option<Point2> {
    let (low, high) = bounds(face.loops.iter().flatten());
    let mut best: Option<(Point2, f64)> = None;
    for i in 1..FACE_SAMPLE_GRID {
        for j in 1..FACE_SAMPLE_GRID {
            let point = Point2::new(
                low.u + (high.u - low.u) * f64::from(i) / f64::from(FACE_SAMPLE_GRID),
                low.v + (high.v - low.v) * f64::from(j) / f64::from(FACE_SAMPLE_GRID),
            );
            if !face.contains(point) {
                continue;
            }
            let room = face.boundary_distance(point);
            if best.is_none_or(|(_, most)| room > most) {
                best = Some((point, room));
            }
        }
    }
    let (point, room) = best?;
    (room > FACE_CLEARANCE * tolerance).then_some(point)
}

#[cfg(test)]
mod tests;
