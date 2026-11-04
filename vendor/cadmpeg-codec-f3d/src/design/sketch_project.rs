// SPDX-License-Identifier: Apache-2.0
//! Project planar and spatial sketch geometry.

use crate::design::dimensions::{planar_point, sketch_normal_sign};
use crate::design::face_resolve::{placement_origin_scale, sketch_curve_is_spatial};
use crate::design::feature_project::{closed_spatial_sketch_profiles, SPATIAL_PROFILE_SEARCH_WORK};
use crate::design::geometry::closed_sketch_profiles;
use crate::ids::native_stream;
use crate::records::{
    sketch_geometry::{
        SketchCurveGeometry, SketchCurveIdentity, SketchPoint, SketchSurface, SketchText,
    },
    sketch_placement::DesignSketchPlacement,
    sketch_relations::{SketchConstraintKind, SketchRelation},
};
use cadmpeg_core::decode::{index_from_u32, DecodeContext, View};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

const EPS_SPATIAL_OWNER_DEPTH: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9: f64 = 1.0e-9;
const EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12: f64 = 1.0e-12;

fn record_spline_segment<'a>(
    segments: &mut HashMap<(&'a str, u32), Option<[Point3; 2]>>,
    scope: &'a str,
    record: u32,
    points: [Point3; 2],
) {
    segments
        .entry((scope, record))
        .and_modify(|existing| {
            if *existing != Some(points) {
                *existing = None;
            }
        })
        .or_insert(Some(points));
}

pub(crate) fn scan_construction_curve_identities(
    scan: &crate::container::ContainerScan<'_>,
    curves: &[SketchCurveIdentity],
    spatial_owners: &HashSet<(&str, u32)>,
    text_frame_curves: &HashSet<(&str, u32)>,
) -> HashSet<String> {
    let mut construction_ids = HashSet::new();
    for curve in curves {
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        if spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        if text_frame_curves.contains(&(scope, curve.record_index)) {
            continue;
        }
        let Some(geometry) = curve.geometry.as_ref() else {
            continue;
        };
        match geometry {
            SketchCurveGeometry::Line { .. } | SketchCurveGeometry::Arc { .. } => {}
            SketchCurveGeometry::Nurbs { .. } => continue,
        }
        let Some(entry) = scan.design_stream_entry_for_scope(
            cadmpeg_core::container::ContainerRole::Bulkstream,
            scope,
        ) else {
            continue;
        };
        let Ok(bytes) = scan.entry_bytes(&entry.name) else {
            continue;
        };
        let Some(g_off) = usize::try_from(curve.byte_offset).ok().and_then(|b| {
            usize::try_from(curve.geometry_offset)
                .ok()
                .and_then(|g| b.checked_add(g))
        }) else {
            continue;
        };
        let is_analytic_shape = match geometry {
            SketchCurveGeometry::Arc { .. } => true,
            SketchCurveGeometry::Line { .. } => View::f64_le_at(bytes, g_off + 72)
                .zip(View::f64_le_at(bytes, g_off + 80))
                .zip(View::f64_le_at(bytes, g_off + 88))
                .is_some_and(|((nx, ny), nz)| {
                    cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::new(nx, ny, nz))
                        .is_some()
                }),
            SketchCurveGeometry::Nurbs { .. } => false,
        };
        if !is_analytic_shape {
            continue;
        }
        let Some(tail_start) = g_off.checked_add(96) else {
            continue;
        };
        let Some(tail_end) = tail_start.checked_add(80).map(|e| e.min(bytes.len())) else {
            continue;
        };
        let Some(tail) = bytes.get(tail_start..tail_end) else {
            continue;
        };
        let mut n_refs = 0usize;
        let mut cur = 0usize;
        while cur + 11 <= tail.len() && tail[cur] == 1 && tail[cur + 5..cur + 11] == [0; 6] {
            n_refs += 1;
            cur += 11;
        }
        if !(1..=3).contains(&n_refs) {
            continue;
        }
        let pad = if n_refs == 1 { 5 } else { 3 };
        let flag_off = cur + pad + 4;
        if tail
            .get(cur..cur + pad)
            .is_some_and(|s| s.iter().all(|&b| b == 0))
            && flag_off + 5 <= tail.len()
            && View::u32_le_at(tail, flag_off) == Some(1)
            && tail[flag_off + 4] == 1
        {
            construction_ids.insert(curve.id.clone());
        }
    }
    construction_ids
}

fn distinct_return_member_indices(
    members: &[crate::records::sketch_relations::SketchRelationReturnMember],
) -> bool {
    let mut seen = HashSet::new();
    for member in members {
        let index = member.reference.record_index();
        if seen.contains(&index) {
            return false;
        }
        seen.insert(index);
    }
    true
}

/// Curve identity pairs that more than one curve record of one owning Sketch
/// carries. Such a pair names no single curve record.
fn shared_curve_identities(curves: &[SketchCurveIdentity]) -> HashSet<(&str, u32, u64, u64)> {
    let mut seen = HashSet::new();
    let mut shared = HashSet::new();
    for curve in curves {
        let (Some(scope), Some(owner)) = (native_stream(&curve.id), curve.owner_reference) else {
            continue;
        };
        let key = (scope, owner, curve.primary_id.get(), curve.secondary_id);
        if !seen.insert(key) {
            shared.insert(key);
        }
    }
    shared
}

fn spatial_geometry_owners<'a>(
    points: &'a [SketchPoint],
    curves: &'a [SketchCurveIdentity],
) -> HashSet<(&'a str, u32)> {
    let mut owners = HashSet::new();
    for curve in curves.iter().filter(|curve| sketch_curve_is_spatial(curve)) {
        let (Some(scope), Some(owner)) = (native_stream(&curve.id), curve.owner_reference) else {
            continue;
        };
        if !owners.contains(&(scope, owner)) {
            owners.insert((scope, owner));
        }
    }
    for point in points
        .iter()
        .filter(|point| point.depth().abs() > EPS_SPATIAL_OWNER_DEPTH)
    {
        let (Some(scope), Some(owner)) = (native_stream(&point.id), point.owner_reference) else {
            continue;
        };
        if !owners.contains(&(scope, owner)) {
            owners.insert((scope, owner));
        }
    }
    owners
}

fn sketch_text_horizontal_alignment(
    code: Option<u32>,
) -> Option<cadmpeg_ir::sketches::SketchTextHorizontalAlignment> {
    code.map(|code| match code {
        1 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Left,
        2 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Right,
        3 => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Center,
        code => cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Native(code),
    })
}

fn sketch_text_vertical_alignment(
    code: Option<u32>,
) -> Option<cadmpeg_ir::sketches::SketchTextVerticalAlignment> {
    code.map(|code| match code {
        1 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Top,
        2 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Bottom,
        3 => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Middle,
        code => cadmpeg_ir::sketches::SketchTextVerticalAlignment::Native(code),
    })
}

fn text_frame_curve_records<'a>(
    relations: &'a [SketchRelation],
    curves: &'a [SketchCurveIdentity],
    texts: &'a [SketchText],
) -> HashSet<(&'a str, u32)> {
    let mut curve_owners = HashMap::new();
    for curve in curves {
        let (Some(scope), Some(owner)) = (native_stream(&curve.id), curve.owner_reference) else {
            continue;
        };
        let key = (scope, curve.record_index);
        curve_owners.insert(key, owner);
    }
    let mut text_owners = HashMap::new();
    for text in texts {
        let Some(scope) = native_stream(&text.id) else {
            continue;
        };
        let key = (scope, text.record_index);
        text_owners.insert(key, text.owner_reference);
    }
    let mut frame_curves = HashSet::new();
    for relation in relations {
        let Some(crate::records::sketch_relations::SketchPatternDefinition::TextFrame {
            text_reference,
        }) = relation.definition.pattern()
        else {
            continue;
        };
        let Some(scope) = native_stream(&relation.id) else {
            continue;
        };
        if crate::design::relation_kinds::sole_constraint_kind(relation).is_none()
            || relation
                .members()
                .first()
                .map(|member| member.reference.record_index())
                != Some(*text_reference)
            || !relation
                .auxiliary_references()
                .values()
                .copied()
                .eq([*text_reference])
            || relation.members().len() < 2
            || !relation
                .return_members()
                .iter()
                .map(|member| member.reference.record_index())
                .eq(relation
                    .members()
                    .iter()
                    .skip(1)
                    .map(|member| member.reference.record_index()))
            || text_owners.get(&(scope, *text_reference)) != Some(&relation.owner_reference)
        {
            continue;
        }
        if !relation.return_members().iter().all(|member| {
            curve_owners.get(&(scope, member.reference.record_index()))
                == Some(&relation.owner_reference)
        }) {
            continue;
        }
        for member in relation.return_members().iter() {
            let key = (scope, member.reference.record_index());
            if !frame_curves.contains(&key) {
                frame_curves.insert(key);
            }
        }
    }
    frame_curves
}

/// Borrowed inputs for projecting planar Design sketches and their entities.
#[derive(Clone, Copy)]
pub(crate) struct SketchProjectInputs<'a, 's, 'c> {
    pub(crate) scan: Option<&'s crate::container::ContainerScan<'c>>,
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
    pub(crate) relations: &'a [SketchRelation],
    pub(crate) texts: &'a [SketchText],
}

impl<'a, 's, 'c> SketchProjectInputs<'a, 's, 'c> {
    pub(crate) fn new(
        placements: &'a [DesignSketchPlacement],
        points: &'a [SketchPoint],
        curves: &'a [SketchCurveIdentity],
        relations: &'a [SketchRelation],
        texts: &'a [SketchText],
    ) -> Self {
        Self {
            scan: None,
            placements,
            points,
            curves,
            relations,
            texts,
        }
    }

    pub(crate) fn with_scan(mut self, scan: &'s crate::container::ContainerScan<'c>) -> Self {
        self.scan = Some(scan);
        self
    }
}

/// Project placed Design sketches and their exact planar point/curve records.
pub(crate) fn project_sketch_design(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    inputs: &SketchProjectInputs<'_, '_, '_>,
    linear_tolerance: f64,
) -> Result<SketchDesign, cadmpeg_core::CodecError> {
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition};

    let placements = inputs.placements;
    let points = inputs.points;
    let curves = inputs.curves;
    let relations = inputs.relations;
    let texts = inputs.texts;
    let scan = inputs.scan;

    let text_frame_curves = text_frame_curve_records(relations, curves, texts);
    let mut placements_by_suffix = HashMap::new();
    for placement in placements {
        let (Some(scope), Ok(owner)) = (
            native_stream(&placement.id),
            u32::try_from(placement.entity_id.suffix()),
        ) else {
            continue;
        };
        placements_by_suffix.insert((scope, owner), placement);
    }
    let spatial_owners = spatial_geometry_owners(points, curves);
    let tail_construction_curves = scan.map_or_else(HashSet::new, |scan| {
        scan_construction_curve_identities(scan, curves, &spatial_owners, &text_frame_curves)
    });
    let mut sketches = Vec::new();
    for placement in placements {
        if u32::try_from(placement.entity_id.suffix()).is_ok_and(|owner| {
            native_stream(&placement.id)
                .is_some_and(|scope| spatial_owners.contains(&(scope, owner)))
        }) {
            continue;
        }
        let Ok(resolved) = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(
                placement.transform()[0][3] * placement_origin_scale(placement),
                placement.transform()[1][3] * placement_origin_scale(placement),
                placement.transform()[2][3] * placement_origin_scale(placement),
            ),
            Vector3::new(
                placement.transform()[0][2],
                placement.transform()[1][2],
                placement.transform()[2][2],
            ),
            Vector3::new(
                placement.transform()[0][0],
                placement.transform()[1][0],
                placement.transform()[2][0],
            ),
        ) else {
            continue;
        };
        let name = placement.entity_id.as_str().to_owned();
        let native_ref = placement.id.clone();
        sketches.push(Sketch {
            id: crate::ids::neutral_sketch_id(placement),
            name: Some(name),
            configuration: None,
            visible: placement
                .visibility
                .as_ref()
                .map(|visibility| visibility.visible),
            placement: resolved,
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: Some(native_ref),
        });
    }
    sketches.sort_by(|a, b| a.id.cmp(&b.id));

    let shared_identities = shared_curve_identities(curves);
    let mut entities = Vec::new();
    for point in points {
        let Some(owner) = point.owner_reference else {
            continue;
        };
        let Some(scope) = native_stream(&point.id) else {
            continue;
        };
        if spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let sketch = crate::ids::neutral_sketch_id(placement);
        let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: point.coordinates(),
        }) else {
            continue;
        };
        let native_ref = point.id.clone();
        entities.push(
            SketchEntity::new(
                point.persistent_id().map_or_else(
                    || crate::ids::neutral_sketch_record_id(&sketch, point.record_index),
                    |persistent_id| crate::ids::neutral_sketch_point_id(&sketch, persistent_id),
                ),
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
        );
    }
    for curve in curves {
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        if spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let Some(source) = curve.geometry.as_ref() else {
            continue;
        };
        let geometry = match source {
            SketchCurveGeometry::Line {
                start, end, normal, ..
            } if planar_point(start.as_raw())
                && planar_point(end.as_raw())
                && normal.as_raw().z != 0.0 =>
            {
                let start = start.as_raw();
                let end = end.as_raw();
                let Ok(geometry) = SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.x, start.y),
                    end: Point2::new(end.x, end.y),
                }) else {
                    continue;
                };
                geometry
            }
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } if planar_point(center.as_raw())
                && reference_direction.as_raw().z.abs()
                    <= EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9 =>
            {
                let Some(orientation) = sketch_normal_sign(normal.as_raw()) else {
                    continue;
                };
                let phase = reference_direction
                    .as_raw()
                    .y
                    .atan2(reference_direction.as_raw().x);
                let start_angle = phase + orientation * start_angle.get();
                let end_angle = phase + orientation * end_angle.get();
                let radius = Length::from(*radius);
                let definition = if (end_angle - start_angle).abs()
                    >= std::f64::consts::TAU - EPS_SKETCH_PROJECT_PROJECT_SKETCH_DESIGN_E9
                {
                    SketchGeometryDefinition::Circle {
                        center: Point2::new(center.get().x, center.get().y),
                        radius,
                    }
                } else {
                    let (Some(start_angle), Some(end_angle)) =
                        (Angle::new(start_angle), Angle::new(end_angle))
                    else {
                        continue;
                    };
                    SketchGeometryDefinition::Arc {
                        center: Point2::new(center.get().x, center.get().y),
                        radius,
                        start_angle,
                        end_angle,
                    }
                };
                let Ok(geometry) = SketchGeometry::try_from(definition) else {
                    continue;
                };
                geometry
            }
            SketchCurveGeometry::Nurbs { geometry, .. }
                if geometry.degree() != 0
                    && geometry.poles().point_count() > index_from_u32(geometry.degree())
                    && geometry.poles().points().all(planar_point) =>
            {
                let poles = geometry.poles();
                let planar_poles = poles
                    .points()
                    .map(|point| Point2::new(point.x, point.y))
                    .collect::<Vec<_>>();
                let weights = poles
                    .weights()
                    .next()
                    .is_some()
                    .then(|| poles.weights().collect::<Vec<_>>());
                SketchGeometry::nurbs(cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    geometry.degree(),
                    geometry.knots_copy(),
                    planar_poles,
                    weights,
                    false,
                )?)
            }
            _ => continue,
        };
        let sketch = crate::ids::neutral_sketch_id(placement);
        let native_ref = curve.id.clone();
        let entity_id = if shared_identities.contains(&(
            scope,
            owner,
            curve.primary_id.get(),
            curve.secondary_id,
        )) {
            crate::ids::neutral_sketch_record_id(&sketch, curve.record_index)
        } else {
            crate::ids::neutral_sketch_curve_id(&sketch, curve.primary_id.get(), curve.secondary_id)
        };
        let is_construction = text_frame_curves.contains(&(scope, curve.record_index))
            || tail_construction_curves.contains(curve.id.as_str());
        entities.push(
            SketchEntity::new(entity_id, sketch, geometry)
                .with_construction(is_construction)
                .with_native_ref(Some(native_ref)),
        );
    }
    for text in texts {
        if crate::design::decode::sketch::sketch_text_neutral_state(text) != Some(true) {
            continue;
        }
        let Some(scope) = native_stream(&text.id) else {
            continue;
        };
        // A model-space sketch has no planar sketch to own a text entity.
        if spatial_owners.contains(&(scope, text.owner_reference)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, text.owner_reference)) else {
            continue;
        };
        let sketch = crate::ids::neutral_sketch_id(placement);
        let Some(text_value) = cadmpeg_core::text::NonBlankString::new(text.text.clone()) else {
            continue;
        };
        let Some(font_family) = cadmpeg_core::text::NonBlankString::new(text.font_family.clone())
        else {
            continue;
        };
        let Ok(font_weight) = text.font_weight.try_into() else {
            continue;
        };
        let Ok(geometry) = SketchGeometry::from_parts(SketchGeometryDefinition::Text {
            text: text_value,
            font_family,
            font_weight,
            height: text.height,
            width_factor: text
                .width_factor()
                .and_then(|factor| cadmpeg_ir::scalar::PositiveReal::try_from(factor).ok()),
            placement: text.placement(),
            horizontal_alignment: sketch_text_horizontal_alignment(
                text.alignment().map(|alignment| alignment.horizontal),
            ),
            vertical_alignment: sketch_text_vertical_alignment(
                text.alignment().map(|alignment| alignment.vertical),
            ),
        }) else {
            continue;
        };
        let native_ref = text.id.clone();
        entities.push(
            SketchEntity::new(
                text.persistent_id.map_or_else(
                    || crate::ids::neutral_sketch_record_id(&sketch, text.record_index),
                    |persistent_id| crate::ids::neutral_sketch_text_id(&sketch, persistent_id),
                ),
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
        );
    }
    entities.sort_by(|a, b| a.id().cmp(b.id()));
    let mut losses = Vec::new();
    for sketch in &mut sketches {
        let closed = closed_sketch_profiles(ctx, &sketch.id, &entities, linear_tolerance)?;
        losses.extend(closed.losses);
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(closed.profiles) else {
            continue;
        };
        sketch.profiles = profiles;
    }
    Ok(SketchDesign {
        sketches,
        entities,
        losses,
    })
}

/// Model-space planar sketches projected from one Design.
pub(crate) struct SketchDesign {
    pub(crate) sketches: Vec<cadmpeg_ir::sketches::Sketch>,
    pub(crate) entities: Vec<cadmpeg_ir::sketches::SketchEntity>,
    /// Losses of the projection, such as unprofiled curves.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

#[cfg(test)]
impl SketchDesign {
    pub(crate) fn into_tuple(
        self,
    ) -> (
        Vec<cadmpeg_ir::sketches::Sketch>,
        Vec<cadmpeg_ir::sketches::SketchEntity>,
    ) {
        (self.sketches, self.entities)
    }
}

/// Model-space spatial sketches projected from one Design.
pub(crate) struct SpatialSketchDesign {
    pub(crate) sketches: Vec<cadmpeg_ir::sketches::SpatialSketch>,
    pub(crate) entities: Vec<cadmpeg_ir::sketches::SpatialSketchEntity>,
    /// Losses of the projection, such as a capped profile search.
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Project non-planar Design sketch curves into model-space spatial sketches.
pub(crate) fn project_spatial_sketch_design(
    ctx: &DecodeContext<'_>,
    placements: &[DesignSketchPlacement],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    relations: &[SketchRelation],
    linear_tolerance: f64,
) -> Result<SpatialSketchDesign, cadmpeg_core::CodecError> {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };

    let mut placements_by_suffix = HashMap::new();
    for placement in placements {
        let (Some(scope), Ok(owner)) = (
            native_stream(&placement.id),
            u32::try_from(placement.entity_id.suffix()),
        ) else {
            continue;
        };
        placements_by_suffix.insert((scope, owner), placement);
    }
    let mut spatial_owners = spatial_geometry_owners(points, curves);
    for surface in surfaces {
        let (Some(scope), Some(owner)) = (native_stream(&surface.id), surface.owner_reference)
        else {
            continue;
        };
        spatial_owners.insert((scope, owner));
    }
    let mut curves_by_record = HashMap::new();
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        curves_by_record.insert((scope, curve.record_index), curve);
    }
    let mut spline_segments = HashMap::new();
    for relation in relations {
        // Only the second reference run of a relation record is in semantic
        // order: the control polygon ends with the spline there, and the
        // interleaved first run orders its members by nothing a reader can use.
        let members = relation.return_members();
        if crate::design::relation_kinds::unknown_constraint_bits(relation.definition.state()) != 0
            || crate::records::sketch_relations::constraint_kinds_iter(relation.definition.state())
                .ne([SketchConstraintKind::SplineGroup])
            || members.len() < 2
            || !distinct_return_member_indices(members)
        {
            continue;
        }
        let Some(scope) = native_stream(&relation.id) else {
            continue;
        };
        let Some(curve) = members
            .last()
            .and_then(|member| curves_by_record.get(&(scope, member.reference.record_index())))
        else {
            continue;
        };
        let Some(SketchCurveGeometry::Nurbs { geometry, .. }) = curve.geometry.as_ref() else {
            continue;
        };
        let poles = geometry.poles();
        if curve.owner_reference != Some(relation.owner_reference)
            || poles.point_count() != members.len()
        {
            continue;
        }
        let mut segments = Vec::new();
        let mut complete = true;
        for (member, (first, second)) in members[..members.len() - 1]
            .iter()
            .zip(poles.points().zip(poles.points().skip(1)))
        {
            let candidate = (|| {
                let record = member.reference.record_index();
                let member = curves_by_record.get(&(scope, record))?;
                if member.owner_reference != Some(relation.owner_reference) {
                    return None;
                }
                match member.geometry.as_ref() {
                    None => Some((record, [*first, *second])),
                    Some(SketchCurveGeometry::Line { start, end, .. })
                        if start.as_raw() == first && end.as_raw() == second =>
                    {
                        Some((record, [*first, *second]))
                    }
                    _ => None,
                }
            })();
            let Some(candidate) = candidate else {
                complete = false;
                break;
            };
            segments.push(candidate);
        }
        if !complete {
            continue;
        }
        for (record, points) in segments {
            record_spline_segment(&mut spline_segments, scope, record, points);
        }
    }
    let transform_point = |placement: &DesignSketchPlacement, point: &Point3| {
        let origin_scale = placement_origin_scale(placement);
        Point3::new(
            placement.transform()[0][0] * point.x
                + placement.transform()[0][1] * point.y
                + placement.transform()[0][2] * point.z
                + placement.transform()[0][3] * origin_scale,
            placement.transform()[1][0] * point.x
                + placement.transform()[1][1] * point.y
                + placement.transform()[1][2] * point.z
                + placement.transform()[1][3] * origin_scale,
            placement.transform()[2][0] * point.x
                + placement.transform()[2][1] * point.y
                + placement.transform()[2][2] * point.z
                + placement.transform()[2][3] * origin_scale,
        )
    };
    let transform_vector = |placement: &DesignSketchPlacement, vector: &Vector3| {
        Vector3::new(
            placement.transform()[0][0] * vector.x
                + placement.transform()[0][1] * vector.y
                + placement.transform()[0][2] * vector.z,
            placement.transform()[1][0] * vector.x
                + placement.transform()[1][1] * vector.y
                + placement.transform()[1][2] * vector.z,
            placement.transform()[2][0] * vector.x
                + placement.transform()[2][1] * vector.y
                + placement.transform()[2][2] * vector.z,
        )
    };

    let shared_identities = shared_curve_identities(curves);
    let mut entities = Vec::new();
    for curve in curves {
        let Some(scope) = native_stream(&curve.id) else {
            continue;
        };
        let Some(owner) = curve.owner_reference else {
            continue;
        };
        if !spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let geometry =
            if let Some([start, end]) = spline_segments
                .get(&(scope, curve.record_index))
                .copied()
                .flatten()
            {
                let Ok(geometry) =
                    SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                        start: transform_point(placement, &start),
                        end: transform_point(placement, &end),
                    })
                else {
                    continue;
                };
                geometry
            } else {
                let Some(source) = curve.geometry.as_ref() else {
                    continue;
                };
                match source {
                    SketchCurveGeometry::Line { start, end, .. } => {
                        let Ok(geometry) = SpatialSketchGeometry::try_from(
                            SpatialSketchGeometryDefinition::Line {
                                start: transform_point(placement, start.as_raw()),
                                end: transform_point(placement, end.as_raw()),
                            },
                        ) else {
                            continue;
                        };
                        geometry
                    }
                    SketchCurveGeometry::Arc {
                        center,
                        normal,
                        reference_direction,
                        radius,
                        start_angle,
                        end_angle,
                    } => {
                        let center = transform_point(placement, center.as_raw());
                        let normal = transform_vector(placement, normal.as_raw());
                        let reference_direction =
                            transform_vector(placement, reference_direction.as_raw());
                        let radius = Length::from(*radius);
                        let definition = if (end_angle.get() - start_angle.get()).abs()
                            >= std::f64::consts::TAU
                                - EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_DESIGN_E9
                        {
                            SpatialSketchGeometryDefinition::Circle {
                                center,
                                normal,
                                reference_direction,
                                radius,
                            }
                        } else {
                            SpatialSketchGeometryDefinition::Arc {
                                center,
                                normal,
                                reference_direction,
                                radius,
                                start_angle: *start_angle,
                                end_angle: *end_angle,
                            }
                        };
                        let Ok(geometry) = SpatialSketchGeometry::try_from(definition) else {
                            continue;
                        };
                        geometry
                    }
                    SketchCurveGeometry::Nurbs { geometry, .. } => {
                        let poles = geometry.poles();
                        let transformed_poles = poles
                            .points()
                            .map(|point| transform_point(placement, point))
                            .collect::<Vec<_>>();
                        let weights = poles
                            .weights()
                            .next()
                            .is_some()
                            .then(|| poles.weights().collect::<Vec<_>>());
                        let curve3d = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                            geometry.degree(),
                            geometry.knots_copy(),
                            transformed_poles,
                            weights,
                            false,
                        )?;
                        let Ok(curve3d) = curve3d.try_into() else {
                            continue;
                        };
                        let Ok(geometry) = SpatialSketchGeometry::try_from(
                            SpatialSketchGeometryDefinition::Nurbs { curve: curve3d },
                        ) else {
                            continue;
                        };
                        geometry
                    }
                }
            };
        let sketch = crate::ids::neutral_spatial_sketch_id(placement);
        let native_ref = curve.id.clone();
        let entity_id = if shared_identities.contains(&(
            scope,
            owner,
            curve.primary_id.get(),
            curve.secondary_id,
        )) {
            crate::ids::neutral_spatial_sketch_record_id(&sketch, curve.record_index)
        } else {
            crate::ids::neutral_spatial_sketch_curve_id(
                &sketch,
                curve.primary_id.get(),
                curve.secondary_id,
            )
        };
        entities.push(
            SpatialSketchEntity::new(entity_id, sketch, geometry).with_native_ref(Some(native_ref)),
        );
    }
    for point in points {
        let Some(scope) = native_stream(&point.id) else {
            continue;
        };
        let Some(owner) = point.owner_reference else {
            continue;
        };
        if !spatial_owners.contains(&(scope, owner)) {
            continue;
        }
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let sketch = crate::ids::neutral_spatial_sketch_id(placement);
        let depth = point.depth();
        let Ok(geometry) =
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
                position: transform_point(
                    placement,
                    &Point3::new(point.coordinates().u, point.coordinates().v, depth),
                ),
            })
        else {
            continue;
        };
        let native_ref = point.id.clone();
        entities.push(
            SpatialSketchEntity::new(
                point.persistent_id().map_or_else(
                    || crate::ids::neutral_spatial_sketch_record_id(&sketch, point.record_index),
                    |persistent_id| {
                        crate::ids::neutral_spatial_sketch_point_id(&sketch, persistent_id)
                    },
                ),
                sketch,
                geometry,
            )
            .with_native_ref(Some(native_ref)),
        );
    }
    for surface in surfaces {
        let Some(scope) = native_stream(&surface.id) else {
            continue;
        };
        let Some(owner) = surface.owner_reference else {
            continue;
        };
        let Some(placement) = placements_by_suffix.get(&(scope, owner)) else {
            continue;
        };
        let sketch = crate::ids::neutral_spatial_sketch_id(placement);
        let entity_id =
            crate::ids::neutral_spatial_sketch_surface_id(&sketch, surface.persistent_id.get());
        let u_knots = surface
            .geometry
            .u_knots
            .iter()
            .copied()
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .collect::<Vec<_>>();
        let v_knots = surface
            .geometry
            .v_knots
            .iter()
            .copied()
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .collect::<Vec<_>>();
        let mut control_points = Vec::new();
        for row in &surface.geometry.control_points {
            let points = row
                .iter()
                .map(|point| transform_point(placement, &point.get()))
                .collect::<Vec<_>>();
            control_points.push(points);
        }
        let native_ref = surface.id.clone();
        entities.push(
            SpatialSketchEntity::new(
                entity_id,
                sketch,
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::NurbsSurface {
                    surface: cadmpeg_ir::geometry::nurbs::BsplineSurface::new(
                        surface.geometry.u_degree.get(),
                        surface.geometry.v_degree.get(),
                        u_knots,
                        v_knots,
                        control_points,
                    )
                    .map_err(|error| {
                        crate::design::text::malformed_design(format_args!(
                            "F3D spatial sketch surface {} is invalid: {error}",
                            surface.id
                        ))
                    })?,
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )
            .with_native_ref(Some(native_ref)),
        );
    }
    entities.sort_by(|a, b| a.id().cmp(b.id()));
    let mut entities_by_sketch = HashMap::<_, Vec<_>>::new();
    for entity in &entities {
        entities_by_sketch
            .entry(&entity.sketch)
            .or_default()
            .push(entity);
    }
    let mut sketches = Vec::new();
    let mut capped_profile_searches = 0usize;
    for placement in placements {
        let id = crate::ids::neutral_spatial_sketch_id(placement);
        let Some(sketch_entities) = entities_by_sketch.get(&id) else {
            continue;
        };
        let budget = cadmpeg_core::decode::WorkBudget::new(SPATIAL_PROFILE_SEARCH_WORK);
        let profiles = if let Some(profiles) =
            closed_spatial_sketch_profiles(ctx, sketch_entities, linear_tolerance, &budget)?
        {
            profiles
        } else {
            capped_profile_searches += 1;
            Vec::new()
        };
        let name = placement.entity_id.as_str().to_owned();
        let native_ref = placement.id.clone();
        sketches.push(SpatialSketch {
            profiles,
            id,
            name: Some(name),
            configuration: None,
            visible: placement
                .visibility
                .as_ref()
                .map(|visibility| visibility.visible),
            native_ref: Some(native_ref),
        });
    }
    sketches.sort_by(|a, b| a.id.cmp(&b.id));
    let mut losses = Vec::new();
    if capped_profile_searches != 0 {
        losses.push(crate::loss::F3dLossCode::SketchProfileSearchCapped.note(format!(
            "{capped_profile_searches} spatial sketch(es) have no closed profiles because their profile search exceeded the local work cap SPATIAL_PROFILE_SEARCH_WORK ({SPATIAL_PROFILE_SEARCH_WORK})."
        )));
    }
    Ok(SpatialSketchDesign {
        sketches,
        entities,
        losses,
    })
}

/// Project exact aggregate relations owned by model-space spatial sketches.
pub(crate) fn project_spatial_sketch_constraints(
    placements: &[DesignSketchPlacement],
    relations: &[SketchRelation],
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
    surfaces: &[SketchSurface],
    entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    use cadmpeg_ir::sketches::{
        SpatialSketchConstraint, SpatialSketchConstraintDefinitionInput as Definition,
        SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    };

    let spatial_ids: HashSet<_> = entities.iter().map(|entity| &entity.sketch).collect();
    let mut sketches = HashMap::new();
    for placement in placements {
        let id = crate::ids::neutral_spatial_sketch_id(placement);
        if !spatial_ids.contains(&id) {
            continue;
        }
        let Some((key, value)) = (|| {
            Some((
                (
                    native_stream(&placement.id)?,
                    u32::try_from(placement.entity_id.suffix()).ok()?,
                ),
                (id, placement),
            ))
        })() else {
            continue;
        };
        sketches.insert(key, value);
    }
    let mut record_indices = HashMap::new();
    for (native_ref, record_index) in curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve.record_index))
        .chain(
            points
                .iter()
                .map(|point| (point.id.as_str(), point.record_index)),
        )
        .chain(
            surfaces
                .iter()
                .map(|surface| (surface.id.as_str(), surface.record_index)),
        )
    {
        record_indices.insert(native_ref, record_index);
    }
    let mut projected = HashMap::new();
    for entity in entities {
        let Some((key, value)) = (|| {
            let native_ref = entity.native_ref.as_deref()?;
            Some((
                (native_stream(native_ref)?, *record_indices.get(native_ref)?),
                entity,
            ))
        })() else {
            continue;
        };
        projected.insert(key, value);
    }
    let mut constraints = Vec::new();
    for relation in relations {
        let Some(constraint) = (|| -> Option<SpatialSketchConstraint> {
            let sole_kind = crate::design::relation_kinds::sole_constraint_kind(relation)?;
            let scope = native_stream(&relation.id)?;
            let (sketch, placement) = sketches.get(&(scope, relation.owner_reference))?;
            // The second relation run is the semantic member order. The first
            // run interleaves per-member relation ordinals and has no role
            // order, so it cannot define a neutral spatial constraint.
            let mut semantic_entities = Vec::new();
            for member in relation.return_members().iter() {
                let entity = projected
                    .get(&(scope, member.reference.record_index()))
                    .copied()?;
                semantic_entities.push(entity);
            }
            let members: Vec<_> = semantic_entities
                .iter()
                .map(|entity| entity.id().clone())
                .collect();
            let mut distinct = HashSet::new();
            for member in &members {
                distinct.insert(member);
            }
            if distinct.len() != members.len() {
                return None;
            }
            let definition = match sole_kind {
                SketchConstraintKind::Coincident => {
                    let [first, second] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let point_on_surface =
                        match (first.geometry.definition(), second.geometry.definition()) {
                            (
                                SpatialSketchGeometryDefinition::Point { .. },
                                SpatialSketchGeometryDefinition::NurbsSurface { .. },
                            ) => Some((first, second)),
                            (
                                SpatialSketchGeometryDefinition::NurbsSurface { .. },
                                SpatialSketchGeometryDefinition::Point { .. },
                            ) => Some((second, first)),
                            _ => None,
                        };
                    if let Some((point, surface)) = point_on_surface {
                        Definition::PointOnSurface {
                            point: point.id().clone(),
                            surface: surface.id().clone(),
                        }
                    } else {
                        let (
                            SpatialSketchGeometryDefinition::Point {
                                position: first_position,
                            },
                            SpatialSketchGeometryDefinition::Point {
                                position: second_position,
                            },
                        ) = (first.geometry.definition(), second.geometry.definition())
                        else {
                            return None;
                        };
                        let scale = 1.0
                            + first_position
                                .x
                                .abs()
                                .max(first_position.y.abs())
                                .max(first_position.z.abs())
                                .max(second_position.x.abs())
                                .max(second_position.y.abs())
                                .max(second_position.z.abs());
                        if (first_position.x - second_position.x).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (first_position.y - second_position.y).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                            || (first_position.z - second_position.z).abs()
                                > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        {
                            return None;
                        }
                        Definition::Coincident {
                            first: first.id().clone(),
                            second: second.id().clone(),
                        }
                    }
                }
                SketchConstraintKind::SplineGroup if members.len() >= 2 => {
                    Definition::SplineGroup { entities: members }
                }
                SketchConstraintKind::Tangent => {
                    let [first, second] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let curve = |geometry: &SpatialSketchGeometry| {
                        matches!(
                            (geometry).definition(),
                            SpatialSketchGeometryDefinition::Line { .. }
                                | SpatialSketchGeometryDefinition::Circle { .. }
                                | SpatialSketchGeometryDefinition::Arc { .. }
                                | SpatialSketchGeometryDefinition::Nurbs { .. }
                        )
                    };
                    if !curve(&first.geometry) || !curve(&second.geometry) {
                        return None;
                    }
                    Definition::Tangent {
                        first: first.id().clone(),
                        second: second.id().clone(),
                    }
                }
                SketchConstraintKind::Midpoint => {
                    let [first, second] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let (point, line, position, start, end) =
                        match (first.geometry.definition(), second.geometry.definition()) {
                            (
                                SpatialSketchGeometryDefinition::Point { position },
                                SpatialSketchGeometryDefinition::Line { start, end },
                            ) => (first, second, position, start, end),
                            (
                                SpatialSketchGeometryDefinition::Line { start, end },
                                SpatialSketchGeometryDefinition::Point { position },
                            ) => (second, first, position, start, end),
                            _ => return None,
                        };
                    let midpoint = Point3::new(
                        (start.x + end.x) * 0.5,
                        (start.y + end.y) * 0.5,
                        (start.z + end.z) * 0.5,
                    );
                    let scale = 1.0 + midpoint.x.abs().max(midpoint.y.abs()).max(midpoint.z.abs());
                    if (position.x - midpoint.x).abs()
                        > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        || (position.y - midpoint.y).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                        || (position.z - midpoint.z).abs()
                            > scale * EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9
                    {
                        return None;
                    }
                    Definition::Midpoint {
                        point: point.id().clone(),
                        entity: line.id().clone(),
                    }
                }
                kind @ (SketchConstraintKind::Horizontal | SketchConstraintKind::Vertical) => {
                    let [entity] = semantic_entities.as_slice() else {
                        return None;
                    };
                    let SpatialSketchGeometryDefinition::Line { start, end } =
                        *entity.geometry.definition()
                    else {
                        return None;
                    };
                    let column = usize::from(kind == SketchConstraintKind::Vertical);
                    let direction = Vector3::new(
                        placement.transform()[0][column],
                        placement.transform()[1][column],
                        placement.transform()[2][column],
                    );
                    let line = end.vector_from(start.get());
                    let cross = line.cross(direction);
                    if line.norm() <= EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E12
                        || cross.norm()
                            > EPS_SKETCH_PROJECT_PROJECT_SPATIAL_SKETCH_CONSTRAINTS_E9 * line.norm()
                    {
                        return None;
                    }
                    Definition::ParallelToDirection {
                        entity: entity.id().clone(),
                        direction,
                    }
                }
                _ => return None,
            };
            let Ok(definition) =
                cadmpeg_ir::sketches::SpatialSketchConstraintDefinition::try_from(definition)
            else {
                return None;
            };
            let sketch = sketch.clone();
            let native_ref = relation.id.clone();
            Some(SpatialSketchConstraint {
                id: crate::ids::neutral_sketch_constraint_id(&relation.id, relation.record_index),
                sketch,
                definition,
                native_ref: Some(native_ref),
            })
        })() else {
            continue;
        };
        constraints.push(constraint);
    }
    constraints.sort_by(|a, b| a.id.cmp(&b.id));
    constraints
}

#[cfg(test)]
mod tests;
