// SPDX-License-Identifier: Apache-2.0
//! Admit historical face references against the input topologies that were emitted.
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::edge_treatments::{FullRoundFilletGroup, FullRoundSideSelection};
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeStart, FaceSelection, Feature, FeatureDefinition, FeatureId,
    FeatureInputTopology, FeatureOperation, LinearTermination,
};
use cadmpeg_ir::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{HashMap, HashSet};

type InputStates<'a> =
    HashMap<(&'a FeatureId, &'a FeatureInputTopologyId), Option<&'a FeatureInputTopology>>;
type FeatureInputs<'a> = HashMap<&'a FeatureId, Option<&'a FeatureInputTopology>>;

struct FaceInputs<'a> {
    topologies: &'a [FeatureInputTopology],
    current_faces: &'a [cadmpeg_ir::topology::Face],
    states: Option<InputStates<'a>>,
    features: Option<FeatureInputs<'a>>,
    current: Option<HashSet<&'a FaceId>>,
    members: HashMap<*const FeatureInputTopology, HashSet<&'a HistoricalFaceId>>,
}
impl<'a> FaceInputs<'a> {
    fn new(
        topologies: &'a [FeatureInputTopology],
        faces: &'a [cadmpeg_ir::topology::Face],
    ) -> Self {
        Self {
            topologies,
            current_faces: faces,
            states: None,
            features: None,
            current: None,
            members: HashMap::new(),
        }
    }
    fn state(
        &mut self,
        feature: &FeatureId,
        state: &FeatureInputTopologyId,
    ) -> Option<&'a FeatureInputTopology> {
        if self.states.is_none() {
            let mut states = HashMap::new();
            for topology in self.topologies {
                let key = (&topology.input_of, &topology.id);
                states
                    .entry(key)
                    .and_modify(|value| *value = None)
                    .or_insert(Some(topology));
            }
            self.states = Some(states);
        }
        self.states
            .as_ref()
            .and_then(|states| states.get(&(feature, state)))
            .copied()
            .flatten()
    }
    fn feature(&mut self, feature: &FeatureId) -> Option<&'a FeatureInputTopology> {
        if self.features.is_none() {
            let mut features = HashMap::new();
            for topology in self.topologies {
                features
                    .entry(&topology.input_of)
                    .and_modify(|value| *value = None)
                    .or_insert(Some(topology));
            }
            self.features = Some(features);
        }
        self.features
            .as_ref()
            .and_then(|features| features.get(feature))
            .copied()
            .flatten()
    }
    fn members(&mut self, topology: &'a FeatureInputTopology) -> &HashSet<&'a HistoricalFaceId> {
        self.members
            .entry(std::ptr::from_ref(topology))
            .or_insert_with(|| topology.faces.as_slice().iter().collect())
    }
    fn admit(
        &mut self,
        feature: &FeatureId,
        selection: &mut FaceSelection,
        losses: &mut Vec<LossNote>,
    ) {
        let (state, faces, native) = match selection {
            FaceSelection::Historical {
                state,
                faces,
                native,
            } => (&*state, faces.as_slice(), native.as_str()),
            FaceSelection::HistoricalPartial {
                state,
                faces,
                native,
                ..
            } => (&*state, faces.as_slice(), native.as_str()),
            _ => return,
        };
        let admitted = self.contains(feature, state, faces);
        if !admitted {
            let native = native.to_owned();
            record_loss(feature, losses);
            *selection = FaceSelection::Native(native);
        }
    }
    fn contains(
        &mut self,
        feature: &FeatureId,
        state: &FeatureInputTopologyId,
        faces: &[HistoricalFaceId],
    ) -> bool {
        let Some(topology) = self.state(feature, state) else {
            return false;
        };
        if faces.is_empty() {
            return true;
        }
        let members = self.members(topology);
        faces.iter().all(|face| members.contains(face))
    }

    fn profile(
        &mut self,
        feature: &FeatureId,
        scope: Option<&str>,
        profile: &mut cadmpeg_ir::features::PlanarProfileRef,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        let cadmpeg_ir::features::PlanarProfileRef::HistoricalFaces {
            state,
            faces,
            native,
        } = profile
        else {
            return Ok(());
        };
        if !self.contains(feature, state, faces.as_slice()) {
            let source = scope
                .or_else(|| native.as_slice().first().map(String::as_str))
                .ok_or_else(|| {
                    CodecError::malformed("historical profile has no native selection")
                })?;
            let source = source.to_owned();
            record_loss(feature, losses);
            *profile = cadmpeg_ir::features::PlanarProfileRef::Native(source);
        }
        Ok(())
    }
    fn hole(
        &mut self,
        feature: &FeatureId,
        selection: &mut FaceSelection,
        losses: &mut Vec<LossNote>,
    ) -> Result<(), CodecError> {
        if let FaceSelection::Resolved { faces, native } = selection {
            if self.current.is_none() {
                let mut current = HashSet::new();
                for face in self.current_faces {
                    current.insert(&face.id);
                }
                self.current = Some(current);
            }
            let current = self.current.as_ref().ok_or_else(|| {
                CodecError::malformed("current face admission index was not built")
            })?;
            if !faces.iter().all(|face| current.contains(face)) {
                let native = native.to_owned();
                let historical = (|| -> Option<FaceSelection> {
                    let topology = self.feature(feature)?;

                    let source = topology
                        .native_ref
                        .as_deref()
                        .and_then(super::historical_brep_source);
                    let state = topology
                        .id
                        .as_str()
                        .rsplit_once(':')
                        .and_then(|(_, state)| state.parse::<i64>().ok())?;
                    let prefix = crate::ids::feature_input_prefix(feature, state);
                    if crate::ids::history_input_state_id(&prefix) != topology.id {
                        return None;
                    }
                    let members = self.members(topology);
                    let mut selected = Vec::new();
                    for face in faces.iter() {
                        if !face.as_str().starts_with("f3d:brep:entity#")
                            && !source.is_some_and(|source| {
                                super::active_brep_face_matches_source(face, source)
                            })
                        {
                            return None;
                        }
                        let slot = super::stable_ref(face.as_str())?;
                        let id = crate::ids::history_input_face_id(&prefix, slot);
                        if !members.contains(&id) {
                            return None;
                        }
                        selected.push(id);
                    }
                    let state = topology.id.clone();
                    let identity = native.clone();
                    FaceSelection::historical(state, selected, identity).ok()
                })();
                *selection = match historical {
                    Some(value) => value,
                    None => {
                        record_loss(feature, losses);
                        FaceSelection::Native(native)
                    }
                };
            }
            return Ok(());
        }
        self.admit(feature, selection, losses);
        Ok(())
    }
}
fn record_loss(feature: &FeatureId, losses: &mut Vec<LossNote>) {
    let message = format!("Feature {feature} retains its native face selection because one matching emitted input topology and its face membership could not be proved");
    losses.push(crate::loss::F3dLossCode::HistoricalFaceSelectionUnbound.note(message));
}

struct FaceAdmission<'a, 'l> {
    inputs: FaceInputs<'a>,
    losses: &'l mut Vec<LossNote>,
}

impl FaceAdmission<'_, '_> {
    fn operation(
        &mut self,
        id: &FeatureId,
        scope: Option<&str>,
        operation: &mut FeatureOperation,
    ) -> Result<(), CodecError> {
        let inputs = &mut self.inputs;
        let losses = &mut *self.losses;
        if let FeatureOperation::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Planar(profile),
            ..
        } = operation
        {
            inputs.profile(id, scope, profile, losses)?;
        }
        let mut requires_native = |selection: &FaceSelection| -> Result<bool, CodecError> {
            let (state, selected) = match selection {
                FaceSelection::Historical { state, faces, .. } => (state, faces.as_slice()),
                FaceSelection::HistoricalPartial { state, faces, .. } => (state, faces.as_slice()),
                _ => return Ok(false),
            };
            Ok(!inputs.contains(id, state, selected))
        };
        let mut edit_pair = false;
        match operation {
            FeatureOperation::FaceBlend { operands, .. } => {
                edit_pair = requires_native(operands.first_faces())?
                    || requires_native(operands.second_faces())?;
            }
            FeatureOperation::ReplaceFace { operands, .. } => {
                edit_pair = requires_native(operands.targets())?
                    || requires_native(operands.replacements())?;
            }
            FeatureOperation::FullRoundFillet { groups } => {
                for group in groups.as_slice() {
                    edit_pair |= requires_native(group.center_faces())?;
                    for side in [group.side_one_faces(), group.side_two_faces()] {
                        if let FullRoundSideSelection::Explicit(faces) = side {
                            edit_pair |= requires_native(faces)?;
                        }
                    }
                }
            }
            _ => {}
        }
        let mut admit = |selection: &mut FaceSelection, hole: bool| {
            if hole || matches!(selection, FaceSelection::Resolved { .. }) {
                inputs.hole(id, selection, losses)
            } else {
                inputs.admit(id, selection, losses);
                Ok(())
            }
        };
        match operation {
            FeatureOperation::Extrude { start, extent, .. } => {
                if let ExtrudeStart::FromFace { face, .. } = start {
                    admit(face, false)?;
                }
                let mut side = |side: &mut cadmpeg_ir::features::ExtrudeSide| {
                    if let LinearTermination::ToFace { face, .. } = &mut side.termination {
                        admit(face, false)?;
                    }
                    Ok::<_, CodecError>(())
                };
                match extent {
                    ExtrudeExtent::OneSided { side: value }
                    | ExtrudeExtent::Symmetric { side: value } => side(value)?,
                    ExtrudeExtent::TwoSided { first, second } => {
                        side(first)?;
                        side(second)?;
                    }
                }
            }
            FeatureOperation::Hole {
                face: Some(face), ..
            } => admit(face, true)?,
            FeatureOperation::FullRoundFillet { groups } if edit_pair => {
                let mut admitted = Vec::new();
                for group in groups.as_slice() {
                    let mut center = group.center_faces().clone();
                    admit(&mut center, false)?;
                    let mut side = |value: &FullRoundSideSelection| -> Result<FullRoundSideSelection, CodecError> {
                        Ok(match value {
                            FullRoundSideSelection::Explicit(faces) => {
                                let mut faces = faces.clone();
                                admit(&mut faces, false)?;
                                FullRoundSideSelection::Explicit(faces)
                            }
                            FullRoundSideSelection::Automatic => FullRoundSideSelection::Automatic,
                            FullRoundSideSelection::Unresolved => FullRoundSideSelection::Unresolved,
                        })
                    };
                    let first = side(group.side_one_faces())?;
                    let second = side(group.side_two_faces())?;
                    admitted.push(
                        FullRoundFilletGroup::new(center, first, second)
                            .map_err(CodecError::malformed)?,
                    );
                }
                *groups = cadmpeg_ir::features::NonEmptyMembers::try_from(admitted)
                    .map_err(CodecError::malformed)?;
            }
            FeatureOperation::FaceBlend { operands, .. } if edit_pair => {
                let mut first = operands.first_faces().clone();
                let mut second = operands.second_faces().clone();
                admit(&mut first, false)?;
                admit(&mut second, false)?;
                *operands = cadmpeg_ir::features::FaceBlendOperands::new(first, second)
                    .map_err(CodecError::malformed)?;
            }
            FeatureOperation::ReplaceFace { operands, .. } if edit_pair => {
                let mut first = operands.targets().clone();
                let mut second = operands.replacements().clone();
                admit(&mut first, false)?;
                admit(&mut second, false)?;
                *operands = cadmpeg_ir::features::ReplaceFaceOperands::new(first, second)
                    .map_err(CodecError::malformed)?;
            }
            FeatureOperation::CosmeticThread { face, .. } => admit(face, false)?,
            FeatureOperation::Thicken { faces, .. }
            | FeatureOperation::OffsetSurface { faces, .. }
            | FeatureOperation::KnitSurface { faces, .. }
            | FeatureOperation::TrimSurface { faces, .. }
            | FeatureOperation::ExtendSurface { faces, .. }
            | FeatureOperation::DeleteFace { faces, .. }
            | FeatureOperation::MoveFace { faces, .. }
            | FeatureOperation::Dome { faces, .. }
            | FeatureOperation::Decal { faces, .. } => admit(faces, false)?,
            FeatureOperation::FilledSurface { support_faces, .. }
            | FeatureOperation::RuledSurface { support_faces, .. } => admit(support_faces, false)?,
            FeatureOperation::SplitFace { targets, .. } => admit(targets, false)?,
            FeatureOperation::SplitBody {
                tool: cadmpeg_ir::features::SplitBodyTool::Faces(tools),
                ..
            }
            | FeatureOperation::CutWithSurface { tools, .. } => admit(tools, false)?,
            FeatureOperation::Shell { removed_faces, .. } => admit(removed_faces, false)?,
            FeatureOperation::MirrorShape {
                plane_reference: Some(face),
                ..
            } => admit(face, false)?,
            FeatureOperation::Pattern { seeds, .. } => {
                for seed in seeds {
                    if let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed {
                        admit(faces, false)?;
                    }
                }
            }
            FeatureOperation::Draft { faces, anchor, .. } => {
                admit(faces, false)?;
                let (cadmpeg_ir::features::DraftAnchor::NeutralPlane { plane: anchor, .. }
                | cadmpeg_ir::features::DraftAnchor::PartingLine { tool: anchor, .. }) = anchor;
                admit(anchor, false)?;
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn admit_feature_input_faces(
    features: &mut [Feature],
    topologies: &[FeatureInputTopology],
    faces: &[cadmpeg_ir::topology::Face],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut admission = FaceAdmission {
        inputs: FaceInputs::new(topologies, faces),
        losses,
    };
    for feature in features {
        let id = &feature.id;
        let scope = feature.native_ref.as_deref();
        let mut result = Ok(());
        feature.evaluation.edit(|definition, _| {
            let operation = match definition {
                FeatureDefinition::Operation(operation)
                | FeatureDefinition::PostProcess { operation, .. } => operation,
            };
            result = admission.operation(id, scope, operation);
        });
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
