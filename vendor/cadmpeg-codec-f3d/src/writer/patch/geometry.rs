// SPDX-License-Identifier: Apache-2.0
//! Geometry record patchers and the `patch_*_definition` byte-patcher family.

use cadmpeg_core::convert::truncate_f64_to_u8;

use std::collections::BTreeMap;

use cadmpeg_core::bytes::assemble_u32_be;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Color, Sense};
use cadmpeg_ir::transform::Transform;

use super::edits::{
    NurbsCurveEdit, NurbsSurfaceEdit, PcurveEdit, ProceduralCurveEdit, ProceduralSurfaceEdit,
};
use crate::writer::primitives::unique_knot_count;
use cadmpeg_asm::brep::attributes::{attribute_chain_color_carrier, DirectColorCarrier};
use cadmpeg_asm::brep::records::EndpointSlot;
use cadmpeg_asm::edit::{
    AsmEditSet, CacheTarget, InlinePcurveEdit, NurbsCurveEdit as AsmNurbsCurveEdit,
    NurbsSurfaceEdit as AsmNurbsSurfaceEdit, PcurveEdit as AsmPcurveEdit,
};
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;
use cadmpeg_asm::nurbs::support_chart::{plane_record_chart, NativeChart};
use cadmpeg_asm::sab;

/// Whether an edited NURBS degree is one an ASM spline record states.
///
/// Every spline layout reader bounds the degree it reads to `1..=20`, so a
/// record outside that range is one no f3d document holds.
pub(super) const fn writable_nurbs_degree(after_degree: u32) -> bool {
    matches!(after_degree, 1..=20)
}

/// Whether one parametric direction of an edited NURBS carrier keeps the
/// distinct-knot count the record's knot table holds.
///
/// `AsmEditSet::patch_knot_structure` writes each distinct knot and its
/// multiplicity into the slot the baseline record already holds, so a
/// direction that states a different number of distinct knots has no slot to
/// take.
///
/// The carrier types state the rest of the knot contract on admission:
/// `NurbsCurve::new` and `PcurveNurbs::new` call `require_curve_cardinality`
/// and `require_nondecreasing_knots`, and `NurbsSurface::new` calls
/// `require_length` and `require_nondecreasing_knots` per axis. A knot count
/// that follows from the degree and the pole count, finite knots and
/// non-decreasing knots therefore hold for every value this reads.
pub(super) fn unchanged_unique_knot_count(before_knots: &[f64], after_knots: &[f64]) -> bool {
    unique_knot_count(after_knots) == unique_knot_count(before_knots)
}

/// The per-entity BREP edit maps that the geometry patchers apply as a unit.
///
/// Every field is keyed by BREP entity id (or record index) and carries the
/// edited value for one geometry aspect. The maps travel together from
/// validation through `patch_geometry` into `patch_framed_geometry`, so they
/// are bundled rather than threaded positionally.
#[derive(Clone, Copy)]
pub(super) struct GeometryEdits<'a> {
    pub(in crate::writer) positions: &'a BTreeMap<String, Point3>,
    pub(in crate::writer) lines: &'a BTreeMap<String, (Point3, Vector3)>,
    pub(in crate::writer) conics: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    pub(in crate::writer) degenerate_curves: &'a BTreeMap<String, Point3>,
    pub(in crate::writer) planes: &'a BTreeMap<String, (Point3, Vector3, Vector3)>,
    pub(in crate::writer) spheres: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    pub(in crate::writer) tori: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    pub(in crate::writer) cones: &'a BTreeMap<String, (Point3, Vector3, Vector3, f64, f64, f64)>,
    pub(in crate::writer) body_transforms: &'a BTreeMap<String, Transform>,
    pub(in crate::writer) entity_colors: &'a BTreeMap<String, Color>,
    pub(in crate::writer) edge_ranges: &'a BTreeMap<String, [f64; 2]>,
    pub(in crate::writer) face_senses: &'a BTreeMap<String, Sense>,
    pub(in crate::writer) coedge_senses: &'a BTreeMap<String, Sense>,
    pub(in crate::writer) procedural_surface_edits: &'a BTreeMap<String, ProceduralSurfaceEdit>,
    pub(in crate::writer) nurbs_surfaces: &'a BTreeMap<String, NurbsSurfaceEdit>,
    pub(in crate::writer) nurbs_curves: &'a BTreeMap<String, NurbsCurveEdit>,
    pub(in crate::writer) pcurves: &'a BTreeMap<String, PcurveEdit>,
    pub(in crate::writer) procedural_curve_edits: &'a BTreeMap<String, ProceduralCurveEdit>,
    pub(in crate::writer) procedural_surface_fits: &'a BTreeMap<String, f64>,
    pub(in crate::writer) creation_timestamps: &'a BTreeMap<usize, f64>,
    pub(in crate::writer) edge_continuities: &'a BTreeMap<usize, (Sense, String)>,
    pub(in crate::writer) vertex_ownerships: &'a BTreeMap<usize, (i64, EndpointSlot)>,
    pub(in crate::writer) face_sidedness:
        &'a BTreeMap<usize, cadmpeg_asm::brep::records::FaceContainment>,
    pub(in crate::writer) tolerant_edges: &'a BTreeMap<usize, f64>,
    pub(in crate::writer) tolerant_vertices: &'a BTreeMap<usize, (f64, [f64; 2])>,
}

fn asm_nurbs_surface_edit(edit: &NurbsSurfaceEdit) -> AsmNurbsSurfaceEdit<'_> {
    AsmNurbsSurfaceEdit {
        surface: &edit.surface,
        periodic: edit.periodic,
    }
}

fn asm_nurbs_curve_edit(edit: &NurbsCurveEdit) -> AsmNurbsCurveEdit<'_> {
    AsmNurbsCurveEdit {
        curve: &edit.curve,
        periodic: edit.periodic,
    }
}

/// An edited pcurve that the writer converted with a unit plane chart, moved
/// into the chart of the retained plane record it lies on: that plane's
/// reference-vector length and v-sense.
fn retained_plane_pcurve(
    native: &PcurveNurbs,
    chart: NativeChart,
) -> Result<PcurveNurbs, CodecError> {
    let unit = NativeChart::Plane {
        reverse_v: false,
        scale: 1.0,
    };
    let mut poles = native.pole_rows().to_raw();
    poles.edit_points(|point| {
        *point = chart.native_point(unit.frame_point(*point));
        Ok(())
    })?;
    PcurveNurbs::new(
        native.degree(),
        native.knots().to_vec(),
        poles,
        native.periodic(),
    )
    .map_err(|error| CodecError::NotImplemented(error.to_string()))
}

/// A pcurve edit whose poles are already in the chart of its retained support.
fn asm_pcurve_edit<'a>(
    edit: &'a PcurveEdit,
    native_geometry: &'a PcurveNurbs,
) -> AsmPcurveEdit<'a> {
    match edit {
        PcurveEdit::Inline {
            periodic,
            wrapper_reversed,
            native_tail_flags,
            uv_offset,
            fit_tolerance,
            ..
        } => AsmPcurveEdit::Inline(InlinePcurveEdit::PcurveWrapper {
            native_geometry,
            periodic: *periodic,
            wrapper_reversed: *wrapper_reversed,
            native_tail_flags: *native_tail_flags,
            uv_offset: *uv_offset,
            fit_tolerance: *fit_tolerance,
        }),
        PcurveEdit::Ref { uv_offset, .. } => AsmPcurveEdit::Ref {
            uv_offset: *uv_offset,
        },
    }
}

pub(super) fn patch_geometry(bytes: &mut [u8], edits: &GeometryEdits) -> Result<(), CodecError> {
    AsmEditSet::apply(bytes, |bytes, asm_edits| {
        patch_asm_geometry(bytes, asm_edits, edits)
    })
}

#[cfg(test)]
pub(in crate::writer) fn patch_framed_geometry(
    bytes: &mut [u8],
    records: &[sab::Record],
    edits: &GeometryEdits,
    header_scale: f64,
) -> Result<(), CodecError> {
    let asm_edits = AsmEditSet::from_framed(
        records.to_vec(),
        cadmpeg_asm::asm_header::parse(bytes)
            .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
                header.width
            }),
        header_scale,
    );
    patch_asm_geometry(bytes, &asm_edits, edits)
}

fn patch_asm_geometry(
    bytes: &mut [u8],
    asm_edits: &AsmEditSet,
    edits: &GeometryEdits,
) -> Result<(), CodecError> {
    let records = asm_edits.records();
    let GeometryEdits {
        positions,
        lines,
        conics,
        degenerate_curves,
        planes,
        spheres,
        tori,
        cones,
        body_transforms,
        entity_colors,
        edge_ranges,
        face_senses,
        coedge_senses,
        procedural_surface_edits,
        nurbs_surfaces,
        nurbs_curves,
        pcurves,
        procedural_curve_edits,
        procedural_surface_fits,
        creation_timestamps,
        edge_continuities,
        vertex_ownerships,
        face_sidedness,
        tolerant_edges,
        tolerant_vertices,
    } = *edits;
    let records_by_index = records
        .iter()
        .map(|record| (record.index, record))
        .collect::<BTreeMap<_, _>>();
    let transform_records = records
        .iter()
        .filter(|record| record.head() == "body")
        .filter_map(|body| {
            body_transforms
                .get(&crate::ids::brep_entity_id(body.index))
                .and_then(|transform| {
                    body.ref_at(5)
                        .and_then(|reference| usize::try_from(reference).ok())
                        .map(|reference| (reference, *transform))
                })
        })
        .collect::<BTreeMap<_, _>>();
    let plane_charts = records
        .iter()
        .filter_map(|record| {
            let chart = plane_record_chart(record.head(), &record.tokens)?;
            (chart
                != NativeChart::Plane {
                    reverse_v: false,
                    scale: 1.0,
                })
            .then(|| (crate::ids::brep_entity_id(record.index), chart))
        })
        .collect::<BTreeMap<_, _>>();
    let pcurve_geometry = pcurves
        .iter()
        .map(|(id, edit)| {
            let mut geometry = match plane_charts.get(edit.support()) {
                Some(chart) => retained_plane_pcurve(edit.native_geometry(), *chart)?,
                None => edit.native_geometry().clone(),
            };
            let offset = edit.stored_uv_offset();
            geometry.try_map_control_points(|_, point| {
                let point = point.get();
                cadmpeg_ir::units::FinitePoint2::new(cadmpeg_ir::math::Point2::new(
                    point.u - offset[0],
                    point.v - offset[1],
                ))
                .ok_or_else(|| CodecError::malformed("pcurve UV offset subtraction overflows"))
            })?;
            Ok((id.as_str(), geometry))
        })
        .collect::<Result<BTreeMap<_, _>, CodecError>>()?;
    let ref_pcurve_geometry = records
        .iter()
        .filter(|record| record.head() == "pcurve")
        .filter_map(|record| {
            let id = crate::ids::brep_entity_id(record.index);
            let PcurveEdit::Ref { periodic, .. } = pcurves.get(&id)? else {
                return None;
            };
            let native_geometry = pcurve_geometry.get(id.as_str())?;
            let target = usize::try_from(record.ref_at(4)?).ok()?;
            Some((
                target,
                AsmPcurveEdit::Inline(InlinePcurveEdit::IntcurveCache {
                    native_geometry,
                    periodic: *periodic,
                    fit_tolerance: None,
                }),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut color_records = BTreeMap::new();
    for entity in records
        .iter()
        .filter(|record| record.head() == "body" || record.head() == "face")
    {
        let id = crate::ids::brep_entity_id(entity.index);
        let Some(color) = entity_colors.get(&id) else {
            continue;
        };
        let carrier = attribute_chain_color_carrier(entity, records_by_index.len(), |index| {
            usize::try_from(index)
                .ok()
                .and_then(|index| records_by_index.get(&index).copied())
        });
        let Some((attribute, decoded)) = carrier else {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity color {id} has no writable exact direct-color attribute"
            )));
        };
        if let Some((existing, existing_carrier)) =
            color_records.insert(attribute.index, (*color, decoded.carrier))
        {
            if existing != *color || existing_carrier != decoded.carrier {
                return Err(CodecError::malformed(format_args!(
                    "F3D entities share conflicting color attribute {}",
                    attribute.index
                )));
            }
        }
    }
    for record in records {
        if let Some(timestamp) = creation_timestamps.get(&record.index) {
            if !record.head().contains("ATTRIB_CUSTOM") {
                return Err(CodecError::malformed(format_args!(
                    "F3D timestamp record {} has the wrong attribute family",
                    record.index
                )));
            }
            // Position in chunk space, because the index feeds `chunk()` below
            // and chunk indices skip payload identifiers.
            let Some(family) = record.chunks().position(
                |token| matches!(token, sab::Token::Str(value) if value == "Timestamp_attrib_def"),
            ) else {
                return Err(CodecError::malformed(format_args!(
                    "F3D timestamp record {} has the wrong attribute family",
                    record.index
                )));
            };
            if !matches!(record.chunk(family + 1), Some(sab::Token::Long(1))) {
                return Err(CodecError::malformed(format_args!(
                    "F3D timestamp record {} lacks marker 1 after its family",
                    record.index
                )));
            }
            let offset = asm_edits.required_payload_field(bytes, record, family + 2, 0x06)?;
            AsmEditSet::patch_f64_payload(bytes, offset + 1, *timestamp)?;
            continue;
        }
        if let Some((sense, continuity)) = edge_continuities.get(&record.index) {
            if !matches!(record.head(), "edge" | "tedge") {
                return Err(CodecError::malformed(format_args!(
                    "F3D edge-continuity record {} is not an edge",
                    record.index
                )));
            }
            asm_edits.patch_sense_field(bytes, record, 9, *sense)?;
            asm_edits.patch_ascii_field(bytes, record, 10, continuity)?;
        }
        if let Some((owning_edge, endpoint_index)) = vertex_ownerships.get(&record.index) {
            if !matches!(record.head(), "vertex" | "tvertex") {
                return Err(CodecError::malformed(format_args!(
                    "F3D vertex-ownership record {} is not a vertex",
                    record.index
                )));
            }
            for (index, tag, value) in [
                (3usize, 0x0c, *owning_edge),
                (4, 0x04, i64::from(endpoint_index.code())),
            ] {
                asm_edits.patch_integer_field(bytes, record, index, tag, value)?;
            }
        }
        if let Some(containment) = face_sidedness.get(&record.index) {
            if record.head() != "face" || !matches!(record.chunk(9), Some(sab::Token::True)) {
                return Err(CodecError::malformed(format_args!(
                    "F3D face-sidedness record {} is not double-sided",
                    record.index
                )));
            }
            let sense = match containment {
                cadmpeg_asm::brep::records::FaceContainment::In => Sense::Reversed,
                cadmpeg_asm::brep::records::FaceContainment::Out => Sense::Forward,
            };
            asm_edits.patch_sense_field(bytes, record, 10, sense)?;
        }
        if let Some((tolerance, leading)) = tolerant_vertices.get(&record.index) {
            if record.head() != "tvertex" {
                return Err(CodecError::malformed(format_args!(
                    "F3D tolerant-vertex record {} is not a tvertex",
                    record.index
                )));
            }
            // The record's three f64 tolerance slots: the two leading slots
            // verbatim and the evaluated tolerance last.
            for (index, value) in [(6usize, leading[0]), (7, leading[1]), (8, *tolerance)] {
                let offset = asm_edits.required_payload_field(bytes, record, index, 0x06)?;
                AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
            }
        }
        if let Some(tolerance) = tolerant_edges.get(&record.index) {
            if record.head() != "tedge"
                || !matches!(record.chunk(12), Some(sab::Token::Long(_)))
                || !matches!(record.chunk(13), Some(sab::Token::Long(_)))
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D tolerant-edge record {} has the wrong layout",
                    record.index
                )));
            }
            let offset = asm_edits.required_payload_field(bytes, record, 11, 0x06)?;
            AsmEditSet::patch_f64_payload(bytes, offset + 1, *tolerance)?;
        }
        if let Some((color, carrier)) = color_records.get(&record.index) {
            match carrier {
                DirectColorCarrier::NormalizedRgb { fields } => {
                    for (index, value) in fields.iter().copied().zip([
                        f64::from(color.r()),
                        f64::from(color.g()),
                        f64::from(color.b()),
                    ]) {
                        let offset =
                            asm_edits.required_payload_field(bytes, record, index, 0x06)?;
                        AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
                    }
                }
                DirectColorCarrier::AutodeskTrueColor { field } => {
                    let [red, green, blue] = exact_8_bit_rgb(*color, record)?;
                    let packed = assemble_u32_be([0xc2, red, green, blue]);
                    asm_edits.patch_truecolor_field(bytes, record, *field, packed)?;
                }
                DirectColorCarrier::DecimalRgb { field } => {
                    let [red, green, blue] = exact_8_bit_rgb(*color, record)?;
                    let packed = assemble_u32_be([0, red, green, blue]);
                    asm_edits.patch_decimal_rgb_field(bytes, record, *field, packed)?;
                }
            }
            continue;
        }
        if let Some(transform) = transform_records.get(&record.index) {
            asm_edits.patch_transform(bytes, record, *transform)?;
            continue;
        }
        let id = crate::ids::brep_entity_id(record.index);
        if let Some(edit) = ref_pcurve_geometry.get(&record.index) {
            asm_edits.patch_pcurve(bytes, record, *edit)?;
        }
        if let (Some(edit), Some(native_geometry)) =
            (pcurves.get(&id), pcurve_geometry.get(id.as_str()))
        {
            asm_edits.patch_pcurve(bytes, record, asm_pcurve_edit(edit, native_geometry))?;
        }
        if let Some(edit) = nurbs_curves.get(&id) {
            asm_edits.patch_nurbs_curve(
                bytes,
                record,
                asm_nurbs_curve_edit(edit),
                CacheTarget::First,
            )?;
        }
        let tolerant_curve_id = format!("f3d:brep:tolerant-coedge-curve#{}", record.index);
        if let Some(edit) = nurbs_curves.get(&tolerant_curve_id) {
            if record.head() != "tcoedge" {
                return Err(CodecError::malformed(format_args!(
                    "F3D tolerant use-curve carrier {tolerant_curve_id} is not a tcoedge record"
                )));
            }
            if matches!(record.chunk(15), Some(sab::Token::True)) {
                let mut native_curve = edit.curve.clone();
                native_curve.reverse_parameterization();
                asm_edits.patch_nurbs_curve(
                    bytes,
                    record,
                    AsmNurbsCurveEdit {
                        curve: &native_curve,
                        periodic: edit.periodic,
                    },
                    CacheTarget::First,
                )?;
            } else {
                asm_edits.patch_nurbs_curve(
                    bytes,
                    record,
                    asm_nurbs_curve_edit(edit),
                    CacheTarget::First,
                )?;
            }
        }
        let procedural_curve_id = format!("f3d:brep:procedural_curve#{}", record.index);
        if let Some(edit) = procedural_curve_edits.get(&procedural_curve_id) {
            if let Some(tolerance) = edit.fit_tolerance() {
                asm_edits.patch_procedural_curve_fit(bytes, record, tolerance)?;
            }
            if let Some(definition) = edit.definition() {
                asm_edits.patch_procedural_curve_definition(bytes, record, definition)?;
            }
        }
        let directrix_id = format!("f3d:brep:procedural_surface#{}:directrix", record.index);
        if let Some(edit) = nurbs_curves.get(&directrix_id) {
            asm_edits.patch_nurbs_curve(
                bytes,
                record,
                asm_nurbs_curve_edit(edit),
                CacheTarget::First,
            )?;
        }
        let spine_id = format!("f3d:brep:procedural_surface#{}:spine", record.index);
        if let Some(edit) = nurbs_curves.get(&spine_id) {
            asm_edits.patch_nurbs_curve(
                bytes,
                record,
                asm_nurbs_curve_edit(edit),
                CacheTarget::Final,
            )?;
        }
        if let Some(edit) = nurbs_surfaces.get(&id) {
            asm_edits.patch_nurbs_surface(bytes, record, asm_nurbs_surface_edit(edit), None)?;
        }
        for side in 0..2 {
            let support_id = format!("f3d:brep:procedural_surface#{}:support{side}", record.index);
            if let Some(edit) = nurbs_surfaces.get(&support_id) {
                asm_edits.patch_nurbs_surface(
                    bytes,
                    record,
                    asm_nurbs_surface_edit(edit),
                    Some(side),
                )?;
            }
        }
        let procedural_id = format!("f3d:brep:procedural_surface#{}", record.index);
        if let Some(tolerance) = procedural_surface_fits.get(&procedural_id) {
            asm_edits.patch_procedural_surface_fit(bytes, record, *tolerance)?;
        }
        if let Some(edit) = procedural_surface_edits.get(&procedural_id) {
            if record.head() != "spline" {
                return Err(CodecError::malformed(format_args!(
                    "F3D extrusion carrier {procedural_id} is not a spline record"
                )));
            }
            match edit {
                ProceduralSurfaceEdit::Extrusion {
                    parameter_interval,
                    direction,
                    native_position,
                } => {
                    asm_edits.patch_extrusion_definition(
                        bytes,
                        record,
                        *parameter_interval,
                        *direction,
                        *native_position,
                    )?;
                }
                ProceduralSurfaceEdit::BlendRadii(radii) => {
                    asm_edits.patch_blend_radii(bytes, record, *radii)?;
                }
            }
        }
        if record.head() == "face" {
            if let Some(sense) = face_senses.get(&id) {
                asm_edits.patch_sense_field(bytes, record, 8, *sense)?;
            }
        } else if matches!(record.head(), "coedge" | "tcoedge") {
            if let Some(sense) = coedge_senses.get(&id) {
                asm_edits.patch_sense_field(bytes, record, 7, *sense)?;
            }
        } else if matches!(record.head(), "edge" | "tedge") {
            if let Some(range) = edge_ranges.get(&id) {
                for (index, value) in [(4usize, range[0]), (6, range[1])] {
                    let offset = asm_edits.required_payload_field(bytes, record, index, 0x06)?;
                    AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
                }
            }
        } else if record.head() == "point" {
            if let Some(position) = positions.get(&id) {
                let offset = asm_edits.required_payload_field(bytes, record, 3, 0x13)?;
                for (component, value) in [
                    position.x / LEN_TO_MM,
                    position.y / LEN_TO_MM,
                    position.z / LEN_TO_MM,
                ]
                .into_iter()
                .enumerate()
                {
                    let at = offset + 1 + component * 8;
                    AsmEditSet::patch_f64_payload(bytes, at, value)?;
                }
            }
        } else if record.head() == "straight" {
            if let Some((origin, direction)) = lines.get(&id) {
                let field_indices = match record.name.as_str() {
                    "straight" => [0, 1],
                    "straight-curve" => [3, 4],
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "straight record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    asm_edits.required_payload_field(bytes, record, field_indices[0], 0x13)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[1], 0x14)?,
                ];
                for (offset, values) in fields.into_iter().zip([
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                    [direction.x, direction.y, direction.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        AsmEditSet::patch_f64_payload(bytes, at, value)?;
                    }
                }
            }
        } else if record.head() == "degenerate_curve" {
            if let Some(point) = degenerate_curves.get(&id) {
                let field_index = match record.name.as_str() {
                    "degenerate_curve" => 0,
                    "degenerate_curve-curve" => 3,
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "degenerate-curve record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let offset = asm_edits.required_payload_field(bytes, record, field_index, 0x13)?;
                for (component, value) in [
                    point.x / LEN_TO_MM,
                    point.y / LEN_TO_MM,
                    point.z / LEN_TO_MM,
                ]
                .into_iter()
                .enumerate()
                {
                    let at = offset + 1 + component * 8;
                    AsmEditSet::patch_f64_payload(bytes, at, value)?;
                }
            }
        } else if record.head() == "ellipse" {
            if let Some((center, axis, direction, major_radius, minor_radius)) = conics.get(&id) {
                let field_indices = match record.name.as_str() {
                    "ellipse" => [0, 1, 2, 3],
                    "ellipse-curve" => [3, 4, 5, 6],
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "ellipse record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    asm_edits.required_payload_field(bytes, record, field_indices[0], 0x13)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[1], 0x14)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[2], 0x14)?,
                ];
                let (ratio_offset, old_ratio) =
                    asm_edits.required_payload_double(bytes, record, field_indices[3])?;
                let major = major_radius / LEN_TO_MM;
                for (offset, values) in fields.iter().zip([
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                    [axis.x, axis.y, axis.z],
                    [
                        direction.x * major,
                        direction.y * major,
                        direction.z * major,
                    ],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        AsmEditSet::patch_f64_payload(bytes, at, value)?;
                    }
                }
                let ratio = minor_radius / major_radius;
                let signed_ratio = if old_ratio.is_sign_negative() {
                    -ratio
                } else {
                    ratio
                };
                AsmEditSet::patch_f64_payload(bytes, ratio_offset + 1, signed_ratio)?;
            }
        } else if record.head() == "plane" {
            if let Some((origin, normal, u_axis)) = planes.get(&id) {
                let field_indices = match record.name.as_str() {
                    "plane" => [0, 1, 2],
                    "plane-surface" => [3, 4, 5],
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "plane record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    asm_edits.required_payload_field(bytes, record, field_indices[0], 0x13)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[1], 0x14)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[2], 0x14)?,
                ];
                // The reference-vector length scales the plane's pcurve
                // coordinates, so the patched vector keeps the stored length.
                let (_, [x, y, z]) =
                    asm_edits.required_payload_vector(bytes, record, field_indices[2])?;
                let scale = x.hypot(y).hypot(z);
                for (offset, values) in fields.into_iter().zip([
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                    [normal.x, normal.y, normal.z],
                    [u_axis.x * scale, u_axis.y * scale, u_axis.z * scale],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        AsmEditSet::patch_f64_payload(bytes, at, value)?;
                    }
                }
            }
        } else if record.head() == "sphere" {
            if let Some((center, axis, ref_direction, radius)) = spheres.get(&id) {
                let field_indices = match record.name.as_str() {
                    "sphere" => [0, 1, 2, 3],
                    "sphere-surface" => [3, 4, 5, 6],
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "sphere record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    asm_edits.required_payload_field(bytes, record, field_indices[0], 0x13)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[1], 0x06)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[2], 0x14)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[3], 0x14)?,
                ];
                for (offset, values) in [fields[0], fields[2], fields[3]].into_iter().zip([
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                    [axis.x, axis.y, axis.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        AsmEditSet::patch_f64_payload(bytes, at, value)?;
                    }
                }
                AsmEditSet::patch_f64_payload(bytes, fields[1] + 1, radius / LEN_TO_MM)?;
            }
        } else if record.head() == "torus" {
            if let Some((center, axis, ref_direction, major_radius, minor_radius)) = tori.get(&id) {
                let field_indices = match record.name.as_str() {
                    "torus" => [0, 1, 2, 3, 4],
                    "torus-surface" => [3, 4, 5, 6, 7],
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "torus record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    asm_edits.required_payload_field(bytes, record, field_indices[0], 0x13)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[1], 0x14)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[2], 0x06)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[3], 0x06)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[4], 0x14)?,
                ];
                for (offset, values) in [fields[0], fields[1], fields[4]].into_iter().zip([
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        AsmEditSet::patch_f64_payload(bytes, at, value)?;
                    }
                }
                for (offset, value) in [fields[2], fields[3]]
                    .into_iter()
                    .zip([major_radius / LEN_TO_MM, minor_radius / LEN_TO_MM])
                {
                    AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
                }
            }
        } else if record.head() == "cone" {
            if let Some((origin, axis, ref_direction, radius, ratio, half_angle)) = cones.get(&id) {
                let field_indices = match record.name.as_str() {
                    "cone" => [0, 1, 2, 3, 4, 5, 6],
                    "cone-surface" => [3, 4, 5, 6, 9, 10, 11],
                    _ => {
                        return Err(CodecError::malformed(format_args!(
                            "cone record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    asm_edits.required_payload_field(bytes, record, field_indices[0], 0x13)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[1], 0x14)?,
                    asm_edits.required_payload_field(bytes, record, field_indices[2], 0x14)?,
                ];
                let ratio_offset =
                    asm_edits.required_payload_field(bytes, record, field_indices[3], 0x06)?;
                let (sine_offset, old_sine) =
                    asm_edits.required_payload_double(bytes, record, field_indices[4])?;
                let (cosine_offset, old_cosine) =
                    asm_edits.required_payload_double(bytes, record, field_indices[5])?;
                let radius_offset =
                    asm_edits.required_payload_field(bytes, record, field_indices[6], 0x06)?;
                let sine_sign = if old_sine < 0.0 { -1.0 } else { 1.0 };
                let cosine_sign = if old_cosine < 0.0 { -1.0 } else { 1.0 };
                let native_axis = if *half_angle > 0.0 && sine_sign * cosine_sign < 0.0 {
                    Vector3::new(-axis.x, -axis.y, -axis.z)
                } else {
                    *axis
                };
                let scaled_radius = radius / LEN_TO_MM;
                for (offset, values) in fields.iter().zip([
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                    [native_axis.x, native_axis.y, native_axis.z],
                    [
                        ref_direction.x * scaled_radius,
                        ref_direction.y * scaled_radius,
                        ref_direction.z * scaled_radius,
                    ],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        AsmEditSet::patch_f64_payload(bytes, at, value)?;
                    }
                }
                for (offset, value) in [
                    (ratio_offset, *ratio),
                    (sine_offset, sine_sign * half_angle.sin()),
                    (cosine_offset, cosine_sign * half_angle.cos()),
                    (radius_offset, scaled_radius),
                ] {
                    AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
                }
            }
        }
    }
    Ok(())
}

fn exact_8_bit_rgb(color: Color, record: &sab::Record) -> Result<[u8; 3], CodecError> {
    let channels = [color.r(), color.g(), color.b()];
    let encoded = channels.map(|channel| truncate_f64_to_u8(f64::from((channel * 255.0).round())));
    let [Some(red), Some(green), Some(blue)] = encoded else {
        return Err(CodecError::NotImplemented(format!(
            "{} record {} requires exactly representable 8-bit RGB channels",
            record.head(),
            record.index
        )));
    };
    let encoded = [red, green, blue];
    let decoded = encoded.map(|channel| f32::from(channel) / 255.0);
    if decoded != channels {
        return Err(CodecError::NotImplemented(format!(
            "{} record {} requires exactly representable 8-bit RGB channels",
            record.head(),
            record.index
        )));
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests;
