// SPDX-License-Identifier: Apache-2.0
//! Native record writers for surfaces, curves, and pcurves.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_usize};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
    pcurve::{PcurveGeometry, PcurveInlineForm, PcurveNurbs, PcurveNurbsPoles},
    BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{PcurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::IncreasingParameterInterval;
use std::borrow::Cow;

use super::native_bytes::{
    native_curve_base, native_enum, native_f64, native_i64, native_ident,
    native_length_prefixed_string, native_point, native_ref, native_string, native_subident,
    native_surface_base, native_u16_string, native_vector,
};
use crate::writer::primitives::{native_bool, unique_knot_count};
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;
use cadmpeg_asm::nurbs::support_chart::NativeChart;

const UNSET_VARIABLE_BLEND_TANGENT: f64 = 1.0e37;

pub(super) fn native_smbh_header(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    let mut bytes = b"ASM BinaryFile8".to_vec();
    // Release word matching the product string, the zero region, then the
    // entity-count and flags words (bit 0: history partition present).
    bytes.extend_from_slice(&23100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&3u64.to_le_bytes());
    native_string(&mut bytes, "Autodesk Neutron")?;
    native_string(&mut bytes, "ASM 231.6.3.65535 OSX")?;
    native_string(&mut bytes, "Thu Jan  1 00:00:00 1970")?;
    native_f64(&mut bytes, 60.0);
    // Binary ASM stores length tolerances in centimetres.
    native_f64(&mut bytes, target.tolerances.linear.get() / 10.0);
    native_f64(&mut bytes, target.tolerances.angular.get());
    Ok(bytes)
}

pub(super) fn native_nurbs_surface(
    bytes: &mut Vec<u8>,
    surface: &NurbsSurface,
) -> Result<(), CodecError> {
    let poles = surface.pole_grid();
    native_ident(
        bytes,
        if matches!(poles, NurbsPoleGrid::Rational { .. }) {
            "nurbs"
        } else {
            "nubs"
        },
    )?;
    native_i64(bytes, i64::from(surface.u_degree()));
    native_i64(bytes, i64::from(surface.v_degree()));
    native_enum(bytes, if surface.u_periodic() { 2 } else { 0 });
    native_enum(bytes, if surface.v_periodic() { 2 } else { 0 });
    native_enum(bytes, 0);
    native_enum(bytes, 0);
    native_nurbs_knot_counts(bytes, [surface.u_knots(), surface.v_knots()])?;
    native_nurbs_knots(bytes, surface.u_knots())?;
    native_nurbs_knots(bytes, surface.v_knots())?;
    match poles {
        NurbsPoleGrid::Polynomial { rows } => {
            for v in 0..poles.v_count() {
                for row in rows {
                    let point = row[v];
                    native_f64(bytes, point.x / LEN_TO_MM);
                    native_f64(bytes, point.y / LEN_TO_MM);
                    native_f64(bytes, point.z / LEN_TO_MM);
                }
            }
        }
        NurbsPoleGrid::Rational { rows } => {
            for v in 0..poles.v_count() {
                for row in rows {
                    let pole = row[v];
                    native_f64(bytes, pole.point.x / LEN_TO_MM);
                    native_f64(bytes, pole.point.y / LEN_TO_MM);
                    native_f64(bytes, pole.point.z / LEN_TO_MM);
                    native_f64(bytes, pole.weight.get());
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn native_procedural_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    solved_surface: &Surface,
    solved_cache: &NurbsSurface,
) -> Result<bool, CodecError> {
    let written =
        native_procedural_surface_definition(bytes, target, solved_surface, solved_cache)?;
    if written {
        native_record_bounds(bytes, target, &solved_surface.id);
    }
    Ok(written)
}

/// Emit the four optional record-level U/V bound fields retained on the
/// surface's procedural construction, after its subtype scope closes.
fn native_record_bounds(bytes: &mut Vec<u8>, target: &CadIr, surface: &cadmpeg_ir::ids::SurfaceId) {
    let bounds = target
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| target.model.procedural_surface_owner(&procedural.id) == Some(surface))
        .and_then(cadmpeg_ir::geometry::ProceduralSurface::record_bounds);
    if let Some(bounds) = bounds {
        for bound in bounds.get() {
            native_optional_f64(bytes, bound);
        }
    }
}

fn native_procedural_surface_definition(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    solved_surface: &Surface,
    solved_cache: &NurbsSurface,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| {
            target.model.procedural_surface_owner(&procedural.id) == Some(&solved_surface.id)
        });
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "surface {} has multiple procedural constructions",
            solved_surface.id
        )));
    }
    match procedural.definition() {
        ProceduralSurfaceDefinition::Deformable(definition_payload) => {
            use cadmpeg_ir::geometry::DeformableSurfaceData;
            let construction = definition_payload.construction().to_raw();
            let cache_fit_tolerance = procedural.cache_fit_tolerance().ok_or_else(|| {
                CodecError::Malformed(
                    "deformable surface requires a native cache-fit tolerance".into(),
                )
            })?;
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == construction.support)
                .ok_or_else(|| CodecError::Malformed("deformable support is missing".into()))?;
            if let Some(form) = &construction.cache.form() {
                if form.revision.get() != 22_506 {
                    return Err(CodecError::Malformed(
                        "unsupported revision-gated deformable surface revision".into(),
                    ));
                }
                let DeformableSurfaceData::RevisionMode3 {
                    leading_vectors,
                    leading_parameter,
                    leading_flags,
                    trailing_point,
                    trailing_vectors,
                    frame_parameter,
                    frame_flags,
                    parameters,
                    trailing_flags,
                    trailing_parameter,
                    trailing_value,
                } = &construction.data
                else {
                    return Err(CodecError::Malformed(
                        "revision-gated deformable surface requires mode-3 data".into(),
                    ));
                };
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "defm_spl_sur")?;
                native_i64(bytes, form.revision.get());
                native_embedded_surface_with_bounds(
                    bytes,
                    &support.geometry,
                    &form.support_bounds,
                )?;
                native_i64(bytes, 3);
                for vector in leading_vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_f64(bytes, *leading_parameter);
                for flag in leading_flags {
                    bytes.push(native_bool(*flag));
                }
                native_point(
                    bytes,
                    [
                        trailing_point.x / LEN_TO_MM,
                        trailing_point.y / LEN_TO_MM,
                        trailing_point.z / LEN_TO_MM,
                    ],
                );
                for vector in trailing_vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_f64(bytes, *frame_parameter);
                for flag in frame_flags {
                    bytes.push(native_bool(*flag));
                }
                for parameter in parameters {
                    native_f64(bytes, *parameter);
                }
                for flag in trailing_flags {
                    bytes.push(native_bool(*flag));
                }
                native_f64(bytes, *trailing_parameter);
                native_i64(bytes, *trailing_value);
                native_revision_surface_tail(
                    bytes,
                    "deformable surface",
                    form,
                    Some(solved_cache),
                )?;
                bytes.push(0x10);
                return Ok(true);
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "defm_spl_sur")?;
            native_embedded_surface(bytes, &support.geometry)?;
            let write_frame =
                |bytes: &mut Vec<u8>, frame: &cadmpeg_ir::geometry::DeformableSurfaceFrame| {
                    for vector in frame.leading_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, frame.leading_parameter);
                    for flag in frame.leading_flags {
                        bytes.push(native_bool(flag));
                    }
                    for vector in frame.secondary_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, frame.secondary_parameter);
                    for flag in frame.secondary_flags {
                        bytes.push(native_bool(flag));
                    }
                    native_point(
                        bytes,
                        [
                            frame.point.x / LEN_TO_MM,
                            frame.point.y / LEN_TO_MM,
                            frame.point.z / LEN_TO_MM,
                        ],
                    );
                    for flag in frame.trailing_flags {
                        bytes.push(native_bool(flag));
                    }
                };
            match &construction.data {
                DeformableSurfaceData::Full {
                    leading_vectors,
                    leading_parameter,
                    leading_flags,
                    selector,
                    surface,
                    native_id,
                    flag,
                    first_parameter,
                    version_value,
                    second_parameter,
                    curve,
                    frames,
                    trailing_value,
                } => {
                    native_i64(bytes, 6);
                    for vector in leading_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, *leading_parameter);
                    for flag in leading_flags {
                        bytes.push(native_bool(*flag));
                    }
                    native_i64(bytes, *selector);
                    let secondary = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("deformable secondary surface is missing".into())
                        })?;
                    native_embedded_surface(bytes, &secondary.geometry)?;
                    native_i64(bytes, *native_id);
                    bytes.push(native_bool(*flag));
                    native_f64(bytes, *first_parameter);
                    if let Some(value) = version_value {
                        native_i64(bytes, *value);
                    }
                    native_f64(bytes, *second_parameter);
                    let curve = native_loft_curve(
                        target,
                        curve,
                        Some([*first_parameter, *second_parameter]),
                    )?;
                    native_nurbs_curve(bytes, &curve)?;
                    for frame in frames.iter() {
                        for vector in frame.vectors {
                            native_vector(bytes, [vector.x, vector.y, vector.z]);
                        }
                        native_f64(bytes, frame.parameter);
                        for flag in frame.flags {
                            bytes.push(native_bool(flag));
                        }
                    }
                    native_i64(bytes, *trailing_value);
                }
                DeformableSurfaceData::SurfaceCurve {
                    surface,
                    native_id,
                    flag,
                    first_parameter,
                    selector,
                    second_parameter,
                    curve,
                    vectors,
                    frame_parameter,
                    flags,
                    parameter_triples,
                } => {
                    native_i64(bytes, 5);
                    let secondary = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("deformable secondary surface is missing".into())
                        })?;
                    native_embedded_surface(bytes, &secondary.geometry)?;
                    native_i64(bytes, *native_id);
                    bytes.push(native_bool(*flag));
                    native_f64(bytes, *first_parameter);
                    native_i64(bytes, *selector);
                    native_f64(bytes, *second_parameter);
                    let curve = native_loft_curve(
                        target,
                        curve,
                        Some([*first_parameter, *second_parameter]),
                    )?;
                    native_nurbs_curve(bytes, &curve)?;
                    for vector in vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, *frame_parameter);
                    for flag in flags {
                        bytes.push(native_bool(*flag));
                    }
                    native_i64(
                        bytes,
                        i64::try_from(parameter_triples.len()).map_err(|_| {
                            CodecError::NotImplemented("deformable triple count exceeds i64".into())
                        })?,
                    );
                    for triple in parameter_triples {
                        for value in triple {
                            native_f64(bytes, *value);
                        }
                    }
                }
                DeformableSurfaceData::Plain {
                    frame,
                    parameter_triples,
                } => {
                    native_i64(bytes, 1);
                    write_frame(bytes, frame);
                    native_i64(
                        bytes,
                        i64::try_from(parameter_triples.len()).map_err(|_| {
                            CodecError::NotImplemented("deformable triple count exceeds i64".into())
                        })?,
                    );
                    for triple in parameter_triples {
                        for value in triple {
                            native_f64(bytes, *value);
                        }
                    }
                }
                DeformableSurfaceData::Guided {
                    frame,
                    selector,
                    guide_parameter,
                } => {
                    native_i64(bytes, 3);
                    write_frame(bytes, frame);
                    native_i64(bytes, *selector);
                    native_f64(bytes, *guide_parameter);
                }
                DeformableSurfaceData::Minimal { vectors, selector } => {
                    native_i64(bytes, 8);
                    for vector in vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_i64(bytes, *selector);
                }
                DeformableSurfaceData::RevisionMode3 { .. } => {
                    return Err(CodecError::Malformed(
                        "revision mode-3 deformable data requires its revision form".into(),
                    ));
                }
            }
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
            for values in &construction.discontinuities {
                native_compound_loft_float_array(bytes, values)?;
            }
            bytes.push(native_bool(construction.discontinuity_flag));
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::TSpline { construction } => {
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "t_spl_sur")?;
            if let Some(form) = construction
                .revision_form()
                .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw)
            {
                native_i64(bytes, form.revision.get());
                native_revision_surface_tail(bytes, "T-spline surface", &form, Some(solved_cache))?;
                for bound in &form.support_bounds {
                    native_optional_f64(bytes, *bound);
                }
                native_enum(bytes, construction.type_code());
            } else {
                native_nurbs_surface(bytes, solved_cache)?;
                native_solved_cache_fit_tolerance(
                    bytes,
                    "T-spline surface",
                    procedural
                        .cache_fit_tolerance()
                        .map(cadmpeg_ir::geometry::FitTolerance::get),
                )?;
                for values in
                    cadmpeg_ir::scalar::FiniteReal::raw_lanes(construction.discontinuities())
                {
                    native_compound_loft_float_array(bytes, &values)?;
                }
                bytes.push(native_bool(construction.discontinuity_flag()));
                for range in construction.parameter_ranges() {
                    for value in range.endpoints() {
                        native_f64(bytes, value / LEN_TO_MM);
                    }
                }
                native_i64(bytes, construction.type_code());
            }
            bytes.push(0x0f);
            let inline = construction.subtransform().inline();
            native_ident(bytes, "t_spl_subtrans_object")?;
            native_u16_string(bytes, inline.program.as_str())?;
            if let Some(separator) = inline.separator {
                bytes.push(native_bool(separator));
            }
            native_u16_string(bytes, inline.values.as_str())?;
            bytes.push(0x10);
            native_i64(bytes, construction.trailing_value());
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Exact(definition_payload) => {
            let spline = definition_payload.spline().to_raw();

            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "exact_spl_sur")?;
            match &spline {
                cadmpeg_ir::geometry::ExactSpline::Revision {
                    intervals,
                    extension,
                    form,
                } => {
                    native_i64(bytes, form.revision.get());
                    native_revision_surface_tail(
                        bytes,
                        "exact spline surface",
                        form,
                        Some(solved_cache),
                    )?;
                    for interval in intervals {
                        for bound in interval {
                            native_optional_f64(bytes, *bound);
                        }
                    }
                    native_enum(bytes, *extension);
                }
                cadmpeg_ir::geometry::ExactSpline::Legacy {
                    ranges, extension, ..
                } => {
                    native_nurbs_surface(bytes, solved_cache)?;
                    native_solved_cache_fit_tolerance(
                        bytes,
                        "exact spline surface",
                        procedural
                            .cache_fit_tolerance()
                            .map(cadmpeg_ir::geometry::FitTolerance::get),
                    )?;
                    for range in ranges {
                        for value in range {
                            native_f64(bytes, *value);
                        }
                    }
                    native_i64(bytes, *extension);
                }
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Compound(definition_payload) => {
            let components = definition_payload.components();

            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "comp_spl_sur")?;
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
                native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
            }
            native_i64(
                bytes,
                i64::try_from(components.len()).map_err(|_| {
                    CodecError::NotImplemented("compound surface count exceeds i64".into())
                })?,
            );
            for component in components {
                native_f64(bytes, component.parameter.get());
            }
            for item in components {
                let component = &item.component;
                let component = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *component)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "compound surface {} references missing component {component}",
                            procedural.id
                        ))
                    })?;
                native_embedded_surface(bytes, &component.geometry)?;
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::SubSurface(_) => {
            return Err(CodecError::malformed(format_args!(
                "sub-surface {} must use its exact cacheless carrier",
                procedural.id
            )));
        }
        ProceduralSurfaceDefinition::Taper(definition_payload) => {
            let support = definition_payload.support();
            let reference = definition_payload.reference();
            let pcurve = definition_payload.pcurve();
            let parameter = definition_payload.parameter();
            let taper = &definition_payload.taper().to_raw();
            let revision_form = definition_payload
                .revision_form()
                .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw);
            {
                let support = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *support)
                    .ok_or_else(|| {
                        CodecError::Malformed("taper support surface is missing".into())
                    })?;
                let reference = target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == *reference)
                    .ok_or_else(|| {
                        CodecError::Malformed("taper reference curve is missing".into())
                    })?;
                let reference = native_spline_field_curve(
                    reference.geometry.solved().ok_or_else(|| {
                        cadmpeg_core::CodecError::NotImplemented(
                            "carrier has no solved geometry".into(),
                        )
                    })?,
                    native_pcurve_knot_domain(pcurve.as_ref())?,
                )?;
                let subtype = match taper {
                    cadmpeg_ir::geometry::TaperSurfaceKind::Standard {} => "taper_spl_sur",
                    cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { .. } => "ortho_spl_sur",
                    cadmpeg_ir::geometry::TaperSurfaceKind::Edge { .. } => "edge_tpr_spl_sur",
                    cadmpeg_ir::geometry::TaperSurfaceKind::Shadow { .. } => "shadow_tpr_spl_sur",
                    cadmpeg_ir::geometry::TaperSurfaceKind::Ruled { .. } => "ruled_tpr_spl_sur",
                    cadmpeg_ir::geometry::TaperSurfaceKind::Swept { .. } => "swept_tpr_spl_sur",
                };
                if let Some(form) = &revision_form {
                    if !matches!(
                        taper,
                        cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { .. }
                    ) {
                        return Err(CodecError::Malformed(
                            "revision-gated taper generation requires the orthogonal subtype"
                                .into(),
                        ));
                    }
                    native_surface_base(bytes, "spline")?;
                    bytes.push(0x0f);
                    native_ident(bytes, "ortho_spl_sur")?;
                    native_i64(bytes, form.revision.get());
                    native_embedded_surface_with_bounds(
                        bytes,
                        &support.geometry,
                        &form.support_bounds,
                    )?;
                    native_nurbs_curve(bytes, &reference)?;
                    for value in form.reference_endpoints {
                        native_optional_f64(bytes, value);
                    }
                    native_optional_support_pcurve(bytes, &support.geometry, pcurve.as_ref())?;
                    native_f64(bytes, parameter.get());
                    native_revision_surface_tail(bytes, "taper surface", form, Some(solved_cache))?;
                    // Orthogonal sense is the record's own trailing logical, written
                    // after the shared tail's illegal-region flag.
                    if let cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense } = taper {
                        bytes.push(native_bool(*sense));
                    }
                    bytes.push(0x10);
                    return Ok(true);
                }
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, subtype)?;
                native_embedded_surface(bytes, &support.geometry)?;
                native_nurbs_curve(bytes, &reference)?;
                native_optional_support_pcurve(bytes, &support.geometry, pcurve.as_ref())?;
                native_f64(bytes, parameter.get());
                native_nurbs_surface(bytes, solved_cache)?;
                if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
                    native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
                }
                let write_draft = |bytes: &mut Vec<u8>, draft: Vector3| {
                    native_vector(bytes, [draft.x, draft.y, draft.z]);
                };
                match taper {
                    cadmpeg_ir::geometry::TaperSurfaceKind::Standard {} => {}
                    cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense } => {
                        bytes.push(native_bool(*sense));
                    }
                    cadmpeg_ir::geometry::TaperSurfaceKind::Edge { draft } => {
                        write_draft(bytes, *draft);
                    }
                    cadmpeg_ir::geometry::TaperSurfaceKind::Shadow {
                        draft,
                        sine,
                        cosine,
                    }
                    | cadmpeg_ir::geometry::TaperSurfaceKind::Swept {
                        draft,
                        sine,
                        cosine,
                    } => {
                        write_draft(bytes, *draft);
                        native_f64(bytes, *sine);
                        native_f64(bytes, *cosine);
                    }
                    cadmpeg_ir::geometry::TaperSurfaceKind::Ruled {
                        draft,
                        sine,
                        cosine,
                        factor,
                    } => {
                        write_draft(bytes, *draft);
                        native_f64(bytes, *sine);
                        native_f64(bytes, *cosine);
                        native_f64(bytes, *factor);
                    }
                }
                bytes.push(0x10);
            }
        }
        ProceduralSurfaceDefinition::Loft(definition_payload) => {
            let sections = definition_payload
                .sections()
                .each_ref()
                .map(cadmpeg_ir::geometry::LoftSection::to_raw);
            let revision_form = definition_payload
                .revision_form()
                .map(cadmpeg_ir::geometry::LoftRevisionForm::to_raw);
            let parameters = definition_payload.parameters().to_raw();
            let closures = definition_payload.closures();
            let singularities = definition_payload.singularities();
            let mode = definition_payload.mode();
            let bridge = definition_payload
                .bridge()
                .iter()
                .map(cadmpeg_ir::geometry::LoftBridgeToken::to_raw)
                .collect::<Vec<_>>();
            encode_native_loft(
                bytes,
                target,
                procedural,
                crate::writer::generate::native_geometry::LoftSurfaceRecord {
                    sections: &sections,
                    revision_form: revision_form.as_ref(),
                    parameters: &parameters,
                    closures,
                    singularities,
                    mode: *mode,
                    bridge: &bridge,
                    solved_cache: Some(solved_cache),
                },
            )?;
        }
        ProceduralSurfaceDefinition::CompoundLoft(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_compound_loft(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                solved_cache,
            )?;
        }
        ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_scaled_compound_loft(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                Some(solved_cache),
            )?;
        }
        ProceduralSurfaceDefinition::Skin(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_skin_surface(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                solved_cache,
            )?;
        }
        ProceduralSurfaceDefinition::Law(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_law_surface(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                Some(solved_cache),
            )?;
        }
        ProceduralSurfaceDefinition::Net(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_net_surface(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                solved_cache,
            )?;
        }
        ProceduralSurfaceDefinition::Sweep(definition_payload) => {
            match (
                definition_payload.profile(),
                definition_payload.spine(),
                definition_payload.native(),
            ) {
                (profile, spine, Some(construction)) => encode_native_sweep_surface(
                    bytes,
                    target,
                    procedural,
                    crate::writer::generate::native_geometry::SweepSurfaceRecord {
                        profile,
                        spine,
                        construction: &construction.to_raw(),
                        solved_cache: Some(solved_cache),
                    },
                )?,
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D sweep surface {} lacks its native construction graph",
                        procedural.id
                    )))
                }
            }
        }

        ProceduralSurfaceDefinition::G2Blend(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_g2_blend(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                solved_cache,
            )?;
        }
        ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } => {
            encode_native_revision_compound_loft(bytes, target, construction, Some(solved_cache))?;
        }
        ProceduralSurfaceDefinition::RevisionG2Blend { construction } => {
            encode_native_revision_g2_blend(bytes, target, construction, Some(solved_cache))?;
        }
        ProceduralSurfaceDefinition::VariableBlend(definition_payload) => {
            let construction = definition_payload.construction();

            encode_native_variable_blend(
                bytes,
                target,
                &construction.to_raw(),
                Some(solved_cache),
            )?;
        }
        ProceduralSurfaceDefinition::VertexBlend(..) => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D vertex blend {} must use its procedural carrier because VBL_SURF has no solved-cache field",
                procedural.id
            )));
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            let profiles = [first, second]
                .map(|id| {
                    target
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == *id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "ruled surface {} references missing profile {id}",
                                procedural.id
                            ))
                        })
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "rule_sur")?;
            let profile_range = [
                solved_cache.u_knots().first().copied().ok_or_else(|| {
                    CodecError::Malformed("ruled solved surface has no U knot domain".into())
                })?,
                solved_cache.u_knots().last().copied().ok_or_else(|| {
                    CodecError::Malformed("ruled solved surface has no U knot domain".into())
                })?,
            ];
            for profile in profiles {
                let profile = native_interval_curve(
                    profile.geometry.solved().ok_or_else(|| {
                        CodecError::NotImplemented(
                            "source-less F3D carrier has no solved geometry".into(),
                        )
                    })?,
                    profile_range,
                )?;
                native_nurbs_curve(bytes, &profile)?;
            }
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
                native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Sum(definition_payload) => {
            let first = definition_payload.first();
            let second = definition_payload.second();
            let basepoint = definition_payload.basepoint();
            if let Some(form) = &definition_payload
                .revision_form()
                .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw)
            {
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "sum_spl_sur")?;
                native_i64(bytes, form.revision.get());
                for (curve, endpoints) in [
                    (first, &form.reference_endpoints),
                    (second, &form.second_endpoints),
                ] {
                    let range = match endpoints {
                        [Some(lower), Some(upper)] => Some([*lower, *upper]),
                        _ => None,
                    };
                    let curve = native_loft_curve(target, curve, range)?;
                    native_nurbs_curve(bytes, &curve)?;
                    for endpoint in endpoints {
                        native_optional_f64(bytes, *endpoint);
                    }
                }
                native_point(
                    bytes,
                    [
                        basepoint.x / LEN_TO_MM,
                        basepoint.y / LEN_TO_MM,
                        basepoint.z / LEN_TO_MM,
                    ],
                );
                native_revision_surface_tail(bytes, "sum surface", form, Some(solved_cache))?;
                bytes.push(0x10);
                return Ok(true);
            }
            let curves = [first, second]
                .map(|id| {
                    target
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == *id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "sum surface {} references missing curve {id}",
                                procedural.id
                            ))
                        })
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "sum_spl_sur")?;
            let ranges = [solved_cache.u_knots(), solved_cache.v_knots()]
                .into_iter()
                .map(|knots| {
                    Ok::<_, CodecError>([
                        knots.first().copied().ok_or_else(|| {
                            CodecError::Malformed(
                                "sum solved surface has an empty knot domain".into(),
                            )
                        })?,
                        knots.last().copied().ok_or_else(|| {
                            CodecError::Malformed(
                                "sum solved surface has an empty knot domain".into(),
                            )
                        })?,
                    ])
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (curve, range) in curves.into_iter().zip(ranges) {
                let curve = native_interval_curve(
                    curve.geometry.solved().ok_or_else(|| {
                        CodecError::NotImplemented(
                            "source-less F3D carrier has no solved geometry".into(),
                        )
                    })?,
                    range,
                )?;
                native_nurbs_curve(bytes, &curve)?;
            }
            native_point(
                bytes,
                [
                    basepoint.x / LEN_TO_MM,
                    basepoint.y / LEN_TO_MM,
                    basepoint.z / LEN_TO_MM,
                ],
            );
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
                native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            let directrix = definition_payload.directrix();
            let axis_origin = definition_payload.axis_origin();
            let axis_direction = *definition_payload.axis_direction().as_raw();
            let angular_interval = definition_payload.angular_interval().endpoints();
            let angular_parameter_interval = definition_payload
                .angular_parameter_interval()
                .map(IncreasingParameterInterval::endpoints);
            let parameter_interval = definition_payload.parameter_interval();
            let transposed = definition_payload.transposed();
            let revision_form = definition_payload
                .revision_form()
                .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw);
            {
                if angular_parameter_interval
                    .is_some_and(|parameter_interval| parameter_interval != angular_interval)
                {
                    return Err(CodecError::NotImplemented(
                        "F3D rot_spl_sur cannot encode a distinct angular parameter interval"
                            .into(),
                    ));
                }
                if let Some(form) = &revision_form {
                    native_surface_base(bytes, "spline")?;
                    bytes.push(0x0f);
                    native_ident(bytes, "rot_spl_sur")?;
                    native_i64(bytes, form.revision.get());
                    let range = match form.reference_endpoints {
                        [Some(lower), Some(upper)] => Some([lower, upper]),
                        _ => None,
                    };
                    let profile = native_loft_curve(target, directrix, range)?;
                    native_nurbs_curve(bytes, &profile)?;
                    for endpoint in &form.reference_endpoints {
                        native_optional_f64(bytes, *endpoint);
                    }
                    native_point(
                        bytes,
                        [
                            axis_origin.x / LEN_TO_MM,
                            axis_origin.y / LEN_TO_MM,
                            axis_origin.z / LEN_TO_MM,
                        ],
                    );
                    native_vector(
                        bytes,
                        [axis_direction.x, axis_direction.y, axis_direction.z],
                    );
                    native_revision_surface_tail(
                        bytes,
                        "revolution surface",
                        form,
                        Some(solved_cache),
                    )?;
                    bytes.push(0x10);
                    return Ok(true);
                }
                let parameter_interval = (parameter_interval).ok_or_else(|| {
                    CodecError::NotImplemented(
                        "source-less F3D rot_spl_sur requires a directrix parameter interval"
                            .into(),
                    )
                })?;
                let directrix = target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == *directrix)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "revolution surface {} references a missing directrix",
                            procedural.id
                        ))
                    })?;
                let directrix = native_increasing_interval_curve(
                    directrix.geometry.solved().ok_or_else(|| {
                        CodecError::NotImplemented(
                            "source-less F3D carrier has no solved geometry".into(),
                        )
                    })?,
                    parameter_interval,
                )?;
                let parameter_interval = parameter_interval.endpoints();
                let native_parameter_interval = [
                    directrix.knots().first().copied().unwrap_or(0.0),
                    directrix.knots().last().copied().unwrap_or(0.0),
                ];
                let native_angular_interval = [
                    solved_cache.v_knots().first().copied().unwrap_or(0.0),
                    solved_cache.v_knots().last().copied().unwrap_or(0.0),
                ];
                if *transposed
                    || parameter_interval != native_parameter_interval
                    || angular_interval != native_angular_interval
                {
                    return Err(CodecError::NotImplemented(
                    "source-less F3D rot_spl_sur intervals must match its profile and solved cache and cannot be transposed".into(),
                ));
                }
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(bytes, "rot_spl_sur")?;
                native_nurbs_curve(bytes, &directrix)?;
                native_point(
                    bytes,
                    [
                        axis_origin.x / LEN_TO_MM,
                        axis_origin.y / LEN_TO_MM,
                        axis_origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(
                    bytes,
                    [axis_direction.x, axis_direction.y, axis_direction.z],
                );
                native_nurbs_surface(bytes, solved_cache)?;
                if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
                    native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
                }
                bytes.push(0x10);
            }
        }
        ProceduralSurfaceDefinition::Offset(definition_payload) => {
            let support = definition_payload.support();
            let distance = definition_payload.distance();
            let u_sense = definition_payload.u_sense();
            let v_sense = definition_payload.v_sense();
            let extension = &definition_payload.extension().to_raw();
            {
                let support = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *support)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "offset surface {} references a missing support",
                            procedural.id
                        ))
                    })?;
                let extension_flags = match extension {
                    cadmpeg_ir::geometry::OffsetExtension::Revision { form } => {
                        native_surface_base(bytes, "spline")?;
                        bytes.push(0x0f);
                        native_ident(bytes, "off_spl_sur")?;
                        native_i64(bytes, form.revision.get());
                        native_embedded_surface_with_bounds(
                            bytes,
                            &support.geometry,
                            &form.support_bounds,
                        )?;
                        native_f64(bytes, distance.get() / LEN_TO_MM);
                        // Leading sense pair, then the two-boolean ASM extension prefix.
                        for flag in form.flags {
                            bytes.push(native_bool(flag));
                        }
                        native_revision_surface_tail(
                            bytes,
                            "offset surface",
                            form,
                            Some(solved_cache),
                        )?;
                        bytes.push(0x10);
                        return Ok(true);
                    }
                    cadmpeg_ir::geometry::OffsetExtension::Legacy { flags, .. } => {
                        legacy_extension_flag_run(*flags)
                    }
                };
                let u_sense = (*u_sense).ok_or_else(|| {
                    CodecError::NotImplemented(
                        "source-less F3D offset surface requires a U sense".into(),
                    )
                })?;
                let v_sense = (*v_sense).ok_or_else(|| {
                    CodecError::NotImplemented(
                        "source-less F3D offset surface requires a V sense".into(),
                    )
                })?;
                native_surface_base(bytes, "spline")?;
                bytes.push(0x0f);
                native_ident(
                    bytes,
                    if extension_flags.is_empty() {
                        "offsur"
                    } else {
                        "off_spl_sur"
                    },
                )?;
                native_embedded_surface(bytes, &support.geometry)?;
                native_f64(bytes, distance.get() / LEN_TO_MM);
                native_enum(bytes, u_sense);
                native_enum(bytes, v_sense);
                for flag in extension_flags {
                    bytes.push(native_bool(flag));
                }
                native_nurbs_surface(bytes, solved_cache)?;
                if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
                    native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
                }
                bytes.push(0x10);
            }
        }
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            let directrix = definition_payload.directrix();
            let parameter_interval = definition_payload.parameter_interval();
            let direction = definition_payload.direction();
            let native_position = definition_payload.native_position();
            let revision_form = definition_payload
                .revision_form()
                .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw);
            encode_native_extrusion(
                bytes,
                target,
                procedural,
                crate::writer::generate::native_geometry::ExtrusionSurfaceRecord {
                    directrix,
                    parameter_interval: parameter_interval
                        .ok_or_else(|| {
                            CodecError::Malformed(
                                "source-less F3D extrusion lacks its native interval".into(),
                            )
                        })?
                        .get(),
                    direction: direction.get(),
                    native_position: native_position
                        .ok_or_else(|| {
                            CodecError::Malformed(
                                "source-less F3D extrusion lacks its native position".into(),
                            )
                        })?
                        .get(),
                    revision_form: revision_form.as_ref(),
                    solved_cache: Some(solved_cache),
                },
            )?;
        }
        ProceduralSurfaceDefinition::Blend(definition_payload) => {
            let supports = definition_payload.supports();
            let spine = definition_payload.spine();
            let radius = definition_payload.radius();
            let cross_section = definition_payload.cross_section();
            let native = definition_payload.native();

            if let Some(native) = native {
                encode_complete_native_rolling_ball(
                    bytes,
                    target,
                    &native.to_raw(),
                    Some(solved_cache),
                )?;
            } else {
                encode_native_rolling_ball(
                    bytes,
                    target,
                    procedural,
                    crate::writer::generate::native_geometry::RollingBallSurfaceRecord {
                        supports,
                        spine: spine.as_ref(),
                        radius,
                        cross_section,
                        solved_cache,
                    },
                )?;
            }
        }
        ProceduralSurfaceDefinition::Helix { .. } => {
            return Err(CodecError::malformed(format_args!(
                "source-less F3D helix surface {} must use its cacheless native carrier",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::RollingBallJet(_) => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::LinearSweep { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::AxisRevolution { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::ParallelOffset(_) => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::DegenerateTorus { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::CurveBounded { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::Subset(_) => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::Replica { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::Unknown { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
    }
    Ok(true)
}

fn native_bridge_token(
    bytes: &mut Vec<u8>,
    token: &cadmpeg_ir::geometry::LoftBridgeToken,
) -> Result<(), CodecError> {
    match token {
        cadmpeg_ir::geometry::LoftBridgeToken::Boolean(value) => {
            bytes.push(native_bool(*value));
        }
        cadmpeg_ir::geometry::LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
        cadmpeg_ir::geometry::LoftBridgeToken::Double(value) => native_f64(bytes, *value),
        cadmpeg_ir::geometry::LoftBridgeToken::Text(value) => native_string(bytes, value)?,
        cadmpeg_ir::geometry::LoftBridgeToken::Enum(value) => native_enum(bytes, *value),
    }
    Ok(())
}

fn native_g2_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::G2BlendSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("G2 support {} is missing", side.surface))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("G2 side curve {} is missing", side.curve))
        })?;
    let pcurve = side
        .pcurves
        .iter()
        .flatten()
        .find(|pcurve| matches!(pcurve, PcurveGeometry::Nurbs { .. }));
    let curve = native_spline_field_curve(
        curve.geometry.solved().ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
        })?,
        native_pcurve_knot_domain(pcurve)?,
    )?;
    native_nurbs_curve(bytes, &curve)?;
    native_optional_support_pcurve(bytes, &surface.geometry, side.pcurves[0].as_ref())?;
    native_vector(
        bytes,
        [side.direction.x, side.direction.y, side.direction.z],
    );
    native_optional_pcurve(bytes, side.pcurves[1].as_ref())?;
    Ok(())
}

fn encode_native_g2_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::G2BlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "g2_blend_spl_sur")?;
    native_g2_side(bytes, target, &construction.first)?;
    native_enum(bytes, construction.singularity);
    match &construction.first_shape {
        cadmpeg_ir::geometry::G2BlendFirstShape::Full { support } => match support {
            None => native_ident(bytes, "nullbs")?,
            Some(support) => {
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|candidate| candidate.id == support.surface)
                    .ok_or_else(|| {
                        CodecError::Malformed("G2 first exact surface is missing".into())
                    })?;
                let Some(SolvedSurfaceGeometry::Nurbs(surface)) = surface.geometry.solved() else {
                    return Err(CodecError::NotImplemented(
                        "source-less G2 full branch requires a NURBS exact surface".into(),
                    ));
                };
                native_nurbs_surface(bytes, surface)?;
                native_f64(bytes, support.tolerance.get() / LEN_TO_MM);
            }
        },
        cadmpeg_ir::geometry::G2BlendFirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        } => {
            for coefficient in coefficients {
                native_f64(bytes, *coefficient);
            }
            native_f64(bytes, tolerance.get() / LEN_TO_MM);
            if let Some(extension) = extension {
                native_bridge_token(bytes, extension)?;
            }
            native_optional_pcurve(bytes, pcurve.as_ref())?;
        }
    }
    native_g2_side(bytes, target, &construction.second)?;
    let second_exact = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == construction.second_exact_surface)
        .ok_or_else(|| CodecError::Malformed("G2 second exact surface is missing".into()))?;
    let Some(SolvedSurfaceGeometry::Nurbs(second_exact)) = second_exact.geometry.solved() else {
        return Err(CodecError::NotImplemented(
            "source-less G2 second exact surface must be NURBS".into(),
        ));
    };
    native_nurbs_surface(bytes, second_exact)?;
    let center_curve = native_loft_curve(
        target,
        &construction.center_curve,
        Some(construction.center_parameters),
    )?;
    native_nurbs_curve(bytes, &center_curve)?;
    for value in construction.center_parameters {
        native_f64(bytes, value);
    }
    native_i64(bytes, construction.center_flag);
    for range in construction.parameter_ranges {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for value in construction.trailing_parameters {
        native_f64(bytes, value);
    }
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
        native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
    }
    for discontinuities in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("G2 discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    bytes.push(0x10);
    Ok(())
}

fn native_loft_subdata(bytes: &mut Vec<u8>, subdata: &cadmpeg_ir::geometry::LoftSubdata) {
    native_i64(bytes, subdata.type_code());
    native_i64(bytes, subdata.row_count());
    native_i64(bytes, subdata.column_count());
    subdata.visit_rows(|parameters, columns, extra| {
        for value in parameters {
            native_f64(bytes, *value);
        }
        for column in columns {
            native_f64(bytes, column[0]);
            native_f64(bytes, column[1]);
        }
        if let Some(extra) = extra {
            native_f64(bytes, extra[0]);
            native_f64(bytes, extra[1]);
        }
    });
}

/// Emit the classic profile tail: the ASM integer, constraint subdata, and
/// optional direction selected by the second native flag.
fn native_loft_profile_tail(
    bytes: &mut Vec<u8>,
    data: &cadmpeg_ir::geometry::ClassicLoftProfileData,
) {
    native_i64(bytes, data.asm_extension);
    native_loft_subdata(bytes, &data.subdata);
    bytes.push(native_bool(data.direction.is_some()));
    if let Some(direction) = data.direction {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
}

fn native_loft_member_tail(bytes: &mut Vec<u8>, form: &cadmpeg_ir::geometry::LoftMemberForm) {
    let (asm_extension, subdata, direction) = match form {
        cadmpeg_ir::geometry::LoftMemberForm::Support {
            asm_extension,
            subdata,
            direction,
            ..
        }
        | cadmpeg_ir::geometry::LoftMemberForm::PcurvePair {
            asm_extension,
            subdata,
            direction,
            ..
        } => (asm_extension, subdata, direction),
    };
    if let Some(asm_extension) = asm_extension {
        native_i64(bytes, *asm_extension);
    }
    native_loft_subdata(bytes, subdata);
    bytes.push(native_bool(direction.is_some()));
    if let Some(direction) = direction {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
}

fn native_loft_section(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    section: &cadmpeg_ir::geometry::LoftSection,
    parameter_range: Option<[f64; 2]>,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(section.entries.len())
            .map_err(|_| CodecError::NotImplemented("loft section count exceeds i64".into()))?,
    );
    for entry in &section.entries {
        native_f64(bytes, entry.parameter);
        native_i64(
            bytes,
            i64::try_from(entry.profile.len())
                .map_err(|_| CodecError::NotImplemented("loft profile count exceeds i64".into()))?,
        );
        for member in &entry.profile {
            native_i64(bytes, member.form.type_code());
            let curve = native_loft_curve(target, &member.profile.id, parameter_range)?;
            native_nurbs_curve(bytes, &curve)?;
            if let Some(endpoints) = member.profile.endpoints {
                for value in endpoints {
                    native_optional_f64(bytes, value);
                }
            }
            match &member.form {
                cadmpeg_ir::geometry::LoftMemberForm::Support {
                    surface,
                    support_bounds,
                    pcurve,
                    first_flag,
                    ..
                } => {
                    if let Some(surface_id) = surface {
                        let surface = target
                            .model
                            .surfaces
                            .iter()
                            .find(|surface| surface.id == *surface_id)
                            .ok_or_else(|| {
                                CodecError::malformed(format_args!(
                                    "loft references missing surface {surface_id}"
                                ))
                            })?;
                        if member.profile.endpoints.is_some() {
                            native_embedded_surface_with_bounds(
                                bytes,
                                &surface.geometry,
                                support_bounds,
                            )?;
                        } else {
                            native_embedded_surface(bytes, &surface.geometry)?;
                        }
                        native_optional_support_pcurve(bytes, &surface.geometry, pcurve.as_ref())?;
                    } else {
                        native_ident(bytes, "null_surface")?;
                        native_optional_pcurve(bytes, pcurve.as_ref())?;
                    }
                    bytes.push(native_bool(*first_flag));
                }
                cadmpeg_ir::geometry::LoftMemberForm::PcurvePair {
                    pcurve,
                    secondary_pcurve,
                    ..
                } => {
                    native_optional_pcurve(bytes, pcurve.as_ref())?;
                    native_optional_pcurve(bytes, secondary_pcurve.as_ref())?;
                }
            }
            native_loft_member_tail(bytes, &member.form);
        }
        if let Some(path_curve) = &entry.path.path {
            let path = native_loft_curve(target, &path_curve.id, parameter_range)?;
            native_nurbs_curve(bytes, &path)?;
            if let Some(endpoints) = path_curve.endpoints {
                for value in endpoints {
                    native_optional_f64(bytes, value);
                }
            }
        } else {
            native_ident(bytes, "null_curve")?;
        }
        native_i64(
            bytes,
            i64::try_from(entry.path.auxiliaries.len()).map_err(|_| {
                CodecError::NotImplemented("loft auxiliary count exceeds i64".into())
            })?,
        );
        for auxiliary in &entry.path.auxiliaries {
            let auxiliary = native_loft_curve(target, auxiliary, parameter_range)?;
            native_nurbs_curve(bytes, &auxiliary)?;
        }
        native_i64(bytes, entry.path.flag);
    }
    Ok(())
}

fn native_loft_curve(
    target: &CadIr,
    id: &cadmpeg_ir::ids::CurveId,
    parameter_range: Option<[f64; 2]>,
) -> Result<NurbsCurve, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::malformed(format_args!("loft references missing curve {id}")))?;
    native_spline_field_curve(
        curve.geometry.solved().ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
        })?,
        parameter_range,
    )
}

fn native_compound_loft_scale(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    scale: &cadmpeg_ir::geometry::CompoundLoftScale,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(scale.members.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft member count exceeds i64".into())
        })?,
    );
    for member in &scale.members {
        native_i64(bytes, member.type_code);
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == member.curve)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "compound loft references missing member curve {}",
                    member.curve
                ))
            })?;
        let curve = native_spline_field_curve(
            curve.geometry.solved().ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
            })?,
            native_pcurve_knot_domain(member.data.pcurve.as_ref())?,
        )?;
        native_nurbs_curve(bytes, &curve)?;
        let surface_id = &member.data.surface;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "compound loft references missing surface {surface_id}"
                ))
            })?;
        native_embedded_surface(bytes, &surface.geometry)?;
        native_optional_support_pcurve(bytes, &surface.geometry, member.data.pcurve.as_ref())?;
        bytes.push(native_bool(member.data.first_flag));
        native_loft_profile_tail(bytes, &member.data);
    }
    native_nurbs_curve(bytes, &native_loft_curve(target, &scale.path, None)?)?;
    native_i64(
        bytes,
        i64::try_from(scale.auxiliaries.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft auxiliary count exceeds i64".into())
        })?,
    );
    for auxiliary in &scale.auxiliaries {
        native_nurbs_curve(bytes, &native_loft_curve(target, auxiliary, None)?)?;
    }
    for value in scale.tail {
        native_i64(bytes, value);
    }
    Ok(())
}

fn encode_native_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::CompoundLoftConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{CompoundLoftDirection, CompoundLoftTail};

    let cache_fit_tolerance = procedural.cache_fit_tolerance().ok_or_else(|| {
        CodecError::Malformed("compound-loft surface requires a native cache-fit tolerance".into())
    })?;

    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cl_loft_spl_sur")?;
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
    for scale in construction.scales.as_slice() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    match &construction.tail {
        CompoundLoftTail::Six {
            flags,
            scale,
            selector,
            direction,
            parameter_range,
            curve,
        } => {
            native_i64(bytes, 6);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
            native_compound_loft_scale(bytes, target, scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
            for value in parameter_range {
                native_f64(bytes, *value);
            }
            let curve = native_loft_curve(target, curve, Some(*parameter_range))?;
            native_nurbs_curve(bytes, &curve)?;
        }
        CompoundLoftTail::Seven {
            first_flag,
            first_scale,
            second_flag,
            second_scale,
            selector,
            direction,
            trailing_flags,
        } => {
            native_i64(bytes, 7);
            bytes.push(native_bool(*first_flag));
            if let Some(scale) = first_scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(*second_flag));
            native_compound_loft_scale(bytes, target, second_scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
        CompoundLoftTail::Zero {
            flags,
            direction,
            trailing_flags,
        } => {
            native_i64(bytes, 0);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
            native_i64(bytes, direction.selector());
            match direction {
                CompoundLoftDirection::Vector { value } => {
                    native_vector(bytes, [value.x, value.y, value.z]);
                }
                CompoundLoftDirection::Curve { curve, .. } => {
                    native_nurbs_curve(bytes, &native_loft_curve(target, curve, None)?)?;
                }
            }
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
    }
    bytes.push(0x10);
    Ok(())
}

fn native_compound_loft_float_array(bytes: &mut Vec<u8>, values: &[f64]) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(values.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft float-array count exceeds i64".into())
        })?,
    );
    for value in values {
        native_f64(bytes, *value);
    }
    Ok(())
}

fn encode_native_scaled_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::ScaledCompoundLoftConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ScaledCompoundLoftBranch, ScaledCompoundLoftShape,
    };

    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "scaled_cloft_spl_sur")?;
    native_enum(bytes, construction.singularity);
    match &construction.shape {
        ScaledCompoundLoftShape::Full {} => {
            let solved_cache = solved_cache.ok_or_else(|| {
                CodecError::Malformed(
                    "scaled compound-loft full shape requires a solved NURBS cache".into(),
                )
            })?;
            let cache_fit_tolerance = procedural.cache_fit_tolerance().ok_or_else(|| {
                CodecError::Malformed(
                    "scaled compound-loft full shape requires a native cache-fit tolerance".into(),
                )
            })?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
        }
        ScaledCompoundLoftShape::None {
            parameter_ranges,
            parameters,
        } => {
            if procedural.cache_fit_tolerance().is_some() {
                return Err(CodecError::Malformed(
                    "scaled compound-loft none shape cannot carry a cache-fit tolerance".into(),
                ));
            }
            for range in parameter_ranges {
                for value in range {
                    native_f64(bytes, *value);
                }
            }
            for values in parameters {
                native_compound_loft_float_array(bytes, values)?;
            }
        }
    }
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    for scale in construction.scales.as_slice() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.selector);
    match &construction.branch {
        ScaledCompoundLoftBranch::ExtendedVector {
            first_scale,
            second_scale,
            selector,
            direction,
        } => {
            bytes.push(native_bool(true));
            if let Some(scale) = first_scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(true));
            native_compound_loft_scale(bytes, target, second_scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        ScaledCompoundLoftBranch::ExtendedCurve {
            scale,
            flag,
            singularity,
            curve,
        } => {
            bytes.push(native_bool(true));
            if let Some(scale) = scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(false));
            bytes.push(native_bool(*flag));
            native_enum(bytes, *singularity);
            native_nurbs_curve(bytes, &native_loft_curve(target, curve, None)?)?;
        }
        ScaledCompoundLoftBranch::Direct { flag, direction } => {
            bytes.push(native_bool(false));
            bytes.push(native_bool(*flag));
            native_i64(bytes, direction.selector());
            match direction {
                CompoundLoftDirection::Vector { value } => {
                    native_vector(bytes, [value.x, value.y, value.z]);
                }
                CompoundLoftDirection::Curve { curve, .. } => {
                    native_nurbs_curve(bytes, &native_loft_curve(target, curve, None)?)?;
                }
            }
        }
    }
    for flag in construction.trailing_flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.tail_kind);
    for direction in construction.tail_directions {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    native_enum(bytes, construction.tail_singularity);
    native_nurbs_curve(
        bytes,
        &native_loft_curve(target, &construction.tail_curve, None)?,
    )?;
    bytes.push(0x10);
    Ok(())
}

pub(super) fn native_cacheless_procedural_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    surface: &Surface,
) -> Result<bool, CodecError> {
    let written = native_cacheless_procedural_surface_definition(bytes, target, surface)?;
    if written {
        native_record_bounds(bytes, target, &surface.id);
    }
    Ok(written)
}

fn native_cacheless_procedural_surface_definition(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    surface: &Surface,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| {
            target.model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
        });
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "surface {} has multiple procedural constructions",
            surface.id
        )));
    }
    if let ProceduralSurfaceDefinition::Extrusion(definition_payload) = procedural.definition() {
        let directrix = definition_payload.directrix();
        let parameter_interval = definition_payload.parameter_interval();
        let direction = definition_payload.direction();
        let native_position = definition_payload.native_position();
        let revision_form = definition_payload
            .revision_form()
            .map(cadmpeg_ir::geometry::RevisionSurfaceForm::to_raw);
        encode_native_extrusion(
            bytes,
            target,
            procedural,
            crate::writer::generate::native_geometry::ExtrusionSurfaceRecord {
                directrix,
                parameter_interval: parameter_interval
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "source-less F3D extrusion lacks its native interval".into(),
                        )
                    })?
                    .get(),
                direction: direction.get(),
                native_position: native_position
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "source-less F3D extrusion lacks its native position".into(),
                        )
                    })?
                    .get(),
                revision_form: revision_form.as_ref(),
                solved_cache: None,
            },
        )?;
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Helix { construction } = procedural.definition() {
        use cadmpeg_ir::geometry::HelixSurfaceProfile;
        let angle_range = *construction.angle_range();
        let dimension_range = *construction.dimension_range();
        let path = construction.path();
        let profile = *construction.profile();
        let path_angle_range = *path.angle_range();
        let center = *path.center();
        let major = *path.major();
        let minor = *path.minor();
        let pitch = *path.pitch();
        let apex_factor = path.apex_factor();
        let axis = *path.axis();
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        let circular = matches!(profile, HelixSurfaceProfile::Circle(_));
        native_ident(
            bytes,
            if circular {
                "helix_spl_circ"
            } else {
                "helix_spl_line"
            },
        )?;
        for value in angle_range {
            native_f64(bytes, value);
        }
        for value in dimension_range {
            native_f64(bytes, value);
        }
        if let HelixSurfaceProfile::Circle(circle) = profile {
            native_f64(bytes, circle.angle().get());
        }
        for value in path_angle_range {
            native_f64(bytes, value);
        }
        native_point(
            bytes,
            [
                center.x / LEN_TO_MM,
                center.y / LEN_TO_MM,
                center.z / LEN_TO_MM,
            ],
        );
        for vector in [major, minor, pitch] {
            native_point(
                bytes,
                [
                    vector.x / LEN_TO_MM,
                    vector.y / LEN_TO_MM,
                    vector.z / LEN_TO_MM,
                ],
            );
        }
        native_f64(bytes, apex_factor.get());
        native_vector(bytes, [axis.x, axis.y, axis.z]);
        for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
            native_ident(bytes, sentinel)?;
        }
        match profile {
            HelixSurfaceProfile::Circle(circle) => {
                native_f64(bytes, circle.radius().get() / LEN_TO_MM);
            }
            HelixSurfaceProfile::Line(line) => {
                let direction = line.direction();
                native_point(bytes, [direction.x, direction.y, direction.z / LEN_TO_MM]);
            }
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Ruled { first, second, .. } = procedural.definition() {
        if procedural.cache_fit_tolerance().is_some() {
            return Err(CodecError::Malformed(
                "cacheless ruled surface cannot carry a cache-fit tolerance".into(),
            ));
        }
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "rule_sur")?;
        native_nurbs_curve(bytes, &native_loft_curve(target, first, None)?)?;
        native_nurbs_curve(bytes, &native_loft_curve(target, second, None)?)?;
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Sum(definition_payload) = procedural.definition() {
        if definition_payload.revision_form().is_none() {
            if procedural.cache_fit_tolerance().is_some() {
                return Err(CodecError::Malformed(
                    "cacheless sum surface cannot carry a cache-fit tolerance".into(),
                ));
            }
            let basepoint = definition_payload.basepoint();
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "sum_spl_sur")?;
            native_nurbs_curve(
                bytes,
                &native_loft_curve(target, definition_payload.first(), None)?,
            )?;
            native_nurbs_curve(
                bytes,
                &native_loft_curve(target, definition_payload.second(), None)?,
            )?;
            native_point(
                bytes,
                [
                    basepoint.x / LEN_TO_MM,
                    basepoint.y / LEN_TO_MM,
                    basepoint.z / LEN_TO_MM,
                ],
            );
            bytes.push(0x10);
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) =
        procedural.definition()
    {
        let construction = definition_payload.construction();

        if matches!(
            construction.shape,
            cadmpeg_ir::geometry::ScaledCompoundLoftShape::None { .. }
        ) {
            encode_native_scaled_compound_loft(
                bytes,
                target,
                procedural,
                &construction.to_raw(),
                None,
            )?;
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::Law(definition_payload) = procedural.definition() {
        let construction = definition_payload.construction();

        if !matches!(
            construction.tail,
            cadmpeg_ir::geometry::LawSurfaceTail::Full { .. }
        ) {
            encode_native_law_surface(bytes, target, procedural, &construction.to_raw(), None)?;
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::SubSurface(definition_payload) = procedural.definition() {
        let support = definition_payload.support();
        let parameter_ranges = definition_payload.parameter_ranges();
        let support = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *support)
            .ok_or_else(|| CodecError::Malformed("sub-surface support is missing".into()))?;
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "sub_spl_sur")?;
        for value in parameter_ranges.into_iter().flatten() {
            native_f64(bytes, value);
        }
        native_embedded_surface(bytes, &support.geometry)?;
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::VertexBlend(definition_payload) = procedural.definition() {
        let construction = definition_payload.construction();

        encode_native_vertex_blend(bytes, target, &construction.to_raw())?;
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::Sweep(definition_payload) = procedural.definition() {
        if let (profile, spine, Some(construction)) = (
            definition_payload.profile(),
            definition_payload.spine(),
            definition_payload.native(),
        ) {
            if construction
                .cache
                .form()
                .is_some_and(|form| form.cache.parameterization().is_some())
            {
                encode_native_sweep_surface(
                    bytes,
                    target,
                    procedural,
                    crate::writer::generate::native_geometry::SweepSurfaceRecord {
                        profile,
                        spine,
                        construction: &construction.to_raw(),
                        solved_cache: None,
                    },
                )?;
                return Ok(true);
            }
        }
    }
    // Tail form `2` stores the parameterization in place of a solved cache, so
    // a blend record carrying it regenerates without one.
    if let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() {
        if let (Some(construction),) = (definition_payload.native(),) {
            if construction.cache.parameterization().is_some() {
                encode_complete_native_rolling_ball(bytes, target, &construction.to_raw(), None)?;
                return Ok(true);
            }
        }
    }
    if let ProceduralSurfaceDefinition::VariableBlend(definition_payload) = procedural.definition()
    {
        let construction = definition_payload.construction();

        if construction.cache.parameterization().is_some() {
            encode_native_variable_blend(bytes, target, &construction.to_raw(), None)?;
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::Loft(definition_payload) = procedural.definition() {
        if let (sections, Some(form), parameters, closures, singularities, mode, bridge) = (
            definition_payload.sections(),
            definition_payload.revision_form(),
            definition_payload.parameters(),
            definition_payload.closures(),
            definition_payload.singularities(),
            definition_payload.mode(),
            definition_payload.bridge(),
        ) {
            if form.cache.parameterization().is_some() {
                encode_native_loft(
                    bytes,
                    target,
                    procedural,
                    crate::writer::generate::native_geometry::LoftSurfaceRecord {
                        sections: &sections
                            .each_ref()
                            .map(cadmpeg_ir::geometry::LoftSection::to_raw),
                        revision_form: Some(&form.to_raw()),
                        parameters: &parameters.to_raw(),
                        closures,
                        singularities,
                        mode: *mode,
                        bridge: &bridge
                            .iter()
                            .map(cadmpeg_ir::geometry::LoftBridgeToken::to_raw)
                            .collect::<Vec<_>>(),
                        solved_cache: None,
                    },
                )?;
                return Ok(true);
            }
        }
    }
    if let ProceduralSurfaceDefinition::RevisionCompoundLoft { construction } =
        procedural.definition()
    {
        if construction.cache().parameterization().is_some() {
            encode_native_revision_compound_loft(bytes, target, construction, None)?;
            return Ok(true);
        }
    }
    if let ProceduralSurfaceDefinition::RevisionG2Blend { construction } = procedural.definition() {
        if construction.cache().parameterization().is_some() {
            encode_native_revision_g2_blend(bytes, target, construction, None)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// The native token for the null law. The codec knows its own token; the IR
/// does not.
const NULL_LAW_TOKEN: &str = "null_law";

fn native_law_expression(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    expression: &cadmpeg_ir::geometry::LawExpression,
    depth: usize,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::LawExpression;
    if depth > 64 {
        return Err(CodecError::Malformed(
            "native law expression exceeds 64 recursive levels".into(),
        ));
    }
    match expression {
        LawExpression::Null {} => native_string(bytes, NULL_LAW_TOKEN)?,
        LawExpression::Text { value } => {
            return Err(CodecError::NotImplemented(format!(
                "law expression text {value:?} has no recursive native law spelling"
            )));
        }
        LawExpression::Integer { value } => native_i64(bytes, *value),
        LawExpression::Double { value } => native_f64(bytes, *value),
        LawExpression::Point { value } => {
            native_point(
                bytes,
                [
                    value.x / LEN_TO_MM,
                    value.y / LEN_TO_MM,
                    value.z / LEN_TO_MM,
                ],
            );
        }
        LawExpression::Vector { value } => {
            native_vector(bytes, [value.x, value.y, value.z]);
        }
        LawExpression::Transform { scalars, enums } => {
            native_string(bytes, "TRANS")?;
            for scalar in scalars {
                native_f64(bytes, *scalar);
            }
            for value in enums {
                native_enum(bytes, *value);
            }
        }
        LawExpression::TransformVec {
            vectors,
            scale,
            flags,
        } => {
            native_string(bytes, "TRANS")?;
            for vector in vectors {
                native_vector(bytes, [vector.x, vector.y, vector.z]);
            }
            native_f64(bytes, *scale);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
        }
        LawExpression::Edge { curve, parameters } => {
            native_string(bytes, "EDGE")?;
            let carrier = target
                .model
                .curves
                .iter()
                .find(|candidate| candidate.id == curve.id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("law edge curve {} is missing", curve.id))
                })?;
            let native_curve = native_interval_curve(
                carrier.geometry.solved().ok_or_else(|| {
                    CodecError::NotImplemented(
                        "source-less F3D carrier has no solved geometry".into(),
                    )
                })?,
                *parameters,
            )?;
            native_nurbs_curve(bytes, &native_curve)?;
            if let Some(endpoints) = &curve.endpoints {
                for value in endpoints {
                    native_optional_f64(bytes, *value);
                }
            }
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
        }
        LawExpression::Spline {
            native_id,
            knots,
            controls,
            point,
        } => {
            native_string(bytes, "SPLINE_LAW")?;
            native_i64(bytes, *native_id);
            native_compound_loft_float_array(bytes, knots)?;
            native_compound_loft_float_array(bytes, controls)?;
            native_point(
                bytes,
                [
                    point.x / LEN_TO_MM,
                    point.y / LEN_TO_MM,
                    point.z / LEN_TO_MM,
                ],
            );
        }
        LawExpression::Algebraic { operator, operands } => {
            let arity = match operator.as_str() {
                "COS" | "SIN" | "TAN" | "COT" | "SEC" | "CSC" | "COSH" | "SINH" | "TANH"
                | "COTH" | "SECH" | "CSCH" | "ARCCOS" | "ARCSIN" | "ARCTAN" | "ARCOT"
                | "ARCSEC" | "ARCCSC" | "ARCCOSH" | "ARCSINH" | "ARCTANH" | "ARCOTH"
                | "ARCSECH" | "ARCCSCH" | "ABS" | "EXP" | "LN" | "LOG" | "SIGN" | "SIZE"
                | "SET" | "SQRT" | "NORM" | "NOT" => 1,
                "CROSS" | "DOT" | "DCUR" | "O" | "ROTATE" | "TERM" => 2,
                "VEC" | "DSURF" => 3,
                "MIN" | "MAX" | "STEP" => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D law operator {operator} has unresolved variable arity"
                    )));
                }
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D law operator {operator} has no defined byte grammar"
                    )));
                }
            };
            if operands.len() != arity {
                return Err(CodecError::malformed(format_args!(
                    "F3D law operator {operator} requires {arity} operands, got {}",
                    operands.len()
                )));
            }
            native_string(bytes, operator)?;
            for operand in operands {
                native_law_expression(bytes, target, operand, depth + 1)?;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
enum SweepLawSlot {
    LegacyFirst,
    RevisionFirst,
    Second,
}

fn native_sweep_law(
    bytes: &mut Vec<u8>,
    expression: &cadmpeg_ir::geometry::LawExpression,
    slot: SweepLawSlot,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::LawExpression;
    use SweepLawSlot::{LegacyFirst, RevisionFirst, Second};

    // A string in a sweep slot is text, including recursive operator names.
    // The first slot also selects the layout: legacy excludes integers,
    // while the revision form requires a string.
    match (slot, expression) {
        (_, LawExpression::Text { value }) => native_string(bytes, value.as_str())?,
        (Second, LawExpression::Integer { value }) => native_i64(bytes, *value),
        (LegacyFirst | Second, LawExpression::Double { value }) => native_f64(bytes, *value),
        (LegacyFirst | Second, LawExpression::Point { value }) => native_point(
            bytes,
            [
                value.x / LEN_TO_MM,
                value.y / LEN_TO_MM,
                value.z / LEN_TO_MM,
            ],
        ),
        (LegacyFirst | Second, LawExpression::Vector { value }) => {
            native_vector(bytes, [value.x, value.y, value.z]);
        }
        (RevisionFirst, _) => {
            return Err(CodecError::NotImplemented(
                "revision sweep first law requires a text expression".into(),
            ));
        }
        (slot, _) => {
            return Err(CodecError::NotImplemented(format!(
                "sweep {slot:?} law has no native spelling for this expression variant"
            )));
        }
    }
    Ok(())
}

fn native_law_formula(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    formula: &cadmpeg_ir::geometry::LawFormula,
) -> Result<(), CodecError> {
    // The native stream spells the null law with its own token; the IR states
    // the variant and knows no such text. The match is exhaustive so a third
    // variant is a compile error here, not a silently written token.
    let (name, variables) = match formula {
        cadmpeg_ir::geometry::LawFormula::Null {} => {
            native_length_prefixed_string(bytes, NULL_LAW_TOKEN)?;
            return Ok(());
        }
        cadmpeg_ir::geometry::LawFormula::Named { name, variables } => (name, variables),
    };
    if name.as_str() == NULL_LAW_TOKEN {
        return Err(CodecError::InvalidInput(format!(
            "law formula named {NULL_LAW_TOKEN} reads back as the native null law; \
             the null law is stated by the variant, not by a name"
        )));
    }
    native_length_prefixed_string(bytes, name.as_str())?;
    native_i64(
        bytes,
        i64::try_from(variables.len())
            .map_err(|_| CodecError::NotImplemented("law variable count exceeds i64".into()))?,
    );
    for variable in variables {
        native_law_expression(bytes, target, variable, 0)?;
    }
    Ok(())
}

fn encode_native_law_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::LawSurfaceConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "law_spl_sur")?;
    if let Some(parameter_ranges) = construction.parameter_ranges {
        for range in parameter_ranges {
            for parameter in range {
                native_f64(bytes, parameter);
            }
        }
    }
    native_law_formula(bytes, target, &construction.primary)?;
    native_i64(
        bytes,
        i64::try_from(construction.additional.len()).map_err(|_| {
            CodecError::NotImplemented("law surface formula count exceeds i64".into())
        })?,
    );
    for formula in &construction.additional {
        native_law_formula(bytes, target, formula)?;
    }
    match &construction.tail {
        cadmpeg_ir::geometry::LawSurfaceTail::Full { .. } => {
            let cache_fit_tolerance = procedural.cache_fit_tolerance().ok_or_else(|| {
                CodecError::Malformed("full law surface requires a cache-fit tolerance".into())
            })?;
            if construction.parameter_ranges.is_none() {
                native_enum(bytes, 0);
            }
            native_nurbs_surface(
                bytes,
                solved_cache.ok_or_else(|| {
                    CodecError::Malformed("full law surface requires a solved cache".into())
                })?,
            )?;
            native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
        }
        cadmpeg_ir::geometry::LawSurfaceTail::Summary {
            parameters,
            fit_tolerance,
            closures,
            singularities,
        } => {
            native_enum(bytes, 1);
            for values in parameters {
                native_compound_loft_float_array(bytes, values)?;
            }
            native_f64(bytes, fit_tolerance.get() / LEN_TO_MM);
            for value in closures.iter().chain(singularities) {
                native_enum(bytes, *value);
            }
        }
        cadmpeg_ir::geometry::LawSurfaceTail::None {
            parameter_ranges,
            closures,
            singularities,
        } => {
            native_enum(bytes, 2);
            for value in parameter_ranges.iter().flatten() {
                native_f64(bytes, *value);
            }
            for value in closures.iter().chain(singularities) {
                native_enum(bytes, *value);
            }
        }
        cadmpeg_ir::geometry::LawSurfaceTail::Historical {} => native_enum(bytes, 3),
        cadmpeg_ir::geometry::LawSurfaceTail::Optimal {} => native_enum(bytes, 4),
    }
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(0x10);
    Ok(())
}

fn native_skin_profile_data(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    data: &cadmpeg_ir::geometry::ClassicLoftProfileData,
) -> Result<(), CodecError> {
    let surface_id = &data.surface;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("skin references missing surface {surface_id}"))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    native_optional_support_pcurve(bytes, &surface.geometry, data.pcurve.as_ref())?;
    bytes.push(native_bool(data.first_flag));
    native_loft_profile_tail(bytes, data);
    Ok(())
}

fn encode_native_skin_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::SkinSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::SkinSurfaceLayout;
    let cache_fit_tolerance = procedural.cache_fit_tolerance().ok_or_else(|| {
        CodecError::Malformed("skin surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "skin_spl_sur")?;
    native_enum(bytes, construction.surface_boolean);
    native_enum(bytes, construction.surface_normal);
    native_enum(bytes, construction.surface_direction);
    native_i64(bytes, construction.count);
    native_f64(bytes, construction.parameter);
    native_i64(bytes, construction.layout.inner_count()?);
    match &construction.layout {
        SkinSurfaceLayout::Profiles {
            profiles,
            path,
            tail,
        } => {
            for profile in profiles {
                native_i64(bytes, profile.type_code);
                let curve = target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == profile.curve)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "skin references missing profile curve {}",
                            profile.curve
                        ))
                    })?;
                let curve = native_spline_field_curve(
                    curve.geometry.solved().ok_or_else(|| {
                        cadmpeg_core::CodecError::NotImplemented(
                            "carrier has no solved geometry".into(),
                        )
                    })?,
                    native_pcurve_knot_domain(profile.data.pcurve.as_ref())?,
                )?;
                native_nurbs_curve(bytes, &curve)?;
                native_skin_profile_data(bytes, target, &profile.data)?;
            }
            native_nurbs_curve(bytes, &native_loft_curve(target, path, None)?)?;
            for value in tail {
                native_i64(bytes, *value);
            }
        }
        SkinSurfaceLayout::Compact {
            curve,
            subdata,
            first_tail,
            secondary_curve,
            second_tail,
            ..
        } => {
            native_nurbs_curve(bytes, &native_loft_curve(target, curve, None)?)?;
            native_loft_subdata(bytes, subdata);
            native_i64(bytes, *first_tail);
            native_nurbs_curve(bytes, &native_loft_curve(target, secondary_curve, None)?)?;
            native_i64(bytes, *second_tail);
        }
    }
    native_vector(
        bytes,
        [
            construction.direction.x,
            construction.direction.y,
            construction.direction.z,
        ],
    );
    native_f64(bytes, construction.trailing_parameter);
    native_law_formula(bytes, target, &construction.formula)?;
    native_nurbs_curve(
        bytes,
        &native_loft_curve(target, &construction.parameter_curve, None)?,
    )?;
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

fn encode_native_net_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::NetSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    let cache_fit_tolerance = procedural.cache_fit_tolerance().ok_or_else(|| {
        CodecError::Malformed("net surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "net_spl_sur")?;
    for section in construction.sections.iter() {
        native_loft_section(bytes, target, section, None)?;
    }
    for parameter in construction.frame_parameters {
        native_f64(bytes, parameter);
    }
    native_i64(bytes, construction.flag);
    for direction in construction.directions {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    for formula in construction.formulas.iter() {
        native_law_formula(bytes, target, formula)?;
    }
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

#[derive(Clone, Copy)]
struct SweepSurfaceRecord<'a> {
    profile: &'a cadmpeg_ir::ids::CurveId,
    spine: &'a cadmpeg_ir::ids::CurveId,
    construction: &'a cadmpeg_ir::geometry::SweepSurfaceConstruction,
    solved_cache: Option<&'a NurbsSurface>,
}

fn encode_native_sweep_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    input: SweepSurfaceRecord<'_>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::SweepSurfaceLayout;
    let SweepSurfaceRecord {
        profile,
        spine,
        construction,
        solved_cache,
    } = input;
    let cache_fit_tolerance = procedural.cache_fit_tolerance();
    if let Some(form) = &construction.cache.form() {
        if let cadmpeg_ir::geometry::SweepSurfaceLayout::LawDriven {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            first_law,
            first_mode,
            first_range,
            law_direction,
            path_mode,
            path_flag,
            path_range,
            path_parameter,
            second_law_flag,
            second_law,
            formula_mode,
            formula,
            trailing_flag,
        } = &construction.layout
        {
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "sweep_sur")?;
            native_i64(bytes, form.revision.get());
            bytes.push(native_bool(form.primary_flag));
            native_i64(bytes, *mode);
            let profile = native_loft_curve(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in form.profile_endpoints {
                native_optional_f64(bytes, value);
            }
            for value in profile_range {
                native_optional_f64(bytes, Some(*value));
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_sweep_law(bytes, first_law, SweepLawSlot::RevisionFirst)?;
            native_i64(bytes, *first_mode);
            for value in first_range {
                native_optional_f64(bytes, Some(*value));
            }
            native_vector(bytes, [law_direction.x, law_direction.y, law_direction.z]);
            native_i64(bytes, *path_mode);
            bytes.push(native_bool(*path_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in form.path_endpoints {
                native_optional_f64(bytes, value);
            }
            for value in path_range {
                native_optional_f64(bytes, Some(*value / LEN_TO_MM));
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*second_law_flag));
            native_sweep_law(bytes, second_law, SweepLawSlot::Second)?;
            native_i64(bytes, *formula_mode);
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
            native_revision_tail_head(bytes, "sweep surface", &form.cache, solved_cache)?;
            native_revision_tail_discontinuities(bytes, &construction.discontinuities)?;
            bytes.push(native_bool(construction.discontinuity_flag));
            bytes.push(0x10);
            return Ok(());
        }
        let SweepSurfaceLayout::ExplicitFormula {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            formula_flag,
            formula,
            trailing_flag,
        } = &construction.layout
        else {
            return Err(CodecError::Malformed(
                "revision-gated sweep generation requires the explicit formula layout".into(),
            ));
        };
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "sweep_sur")?;
        native_i64(bytes, form.revision.get());
        bytes.push(native_bool(form.primary_flag));
        native_i64(bytes, *mode);
        let profile = native_loft_curve(target, profile, Some(*profile_range))?;
        native_nurbs_curve(bytes, &profile)?;
        for value in form.profile_endpoints {
            native_optional_f64(bytes, value);
        }
        for value in profile_range {
            native_optional_f64(bytes, Some(*value));
        }
        bytes.push(native_bool(profile_frame.is_some()));
        if let Some((point, direction)) = profile_frame {
            native_point(
                bytes,
                [
                    point.x / LEN_TO_MM,
                    point.y / LEN_TO_MM,
                    point.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        native_point(
            bytes,
            [
                origin.x / LEN_TO_MM,
                origin.y / LEN_TO_MM,
                origin.z / LEN_TO_MM,
            ],
        );
        for direction in directions {
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        native_i64(bytes, 1);
        bytes.push(native_bool(*trajectory_flag));
        let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
        let spine = native_loft_curve(target, spine, Some(native_path_range))?;
        native_nurbs_curve(bytes, &spine)?;
        for value in form.path_endpoints {
            native_optional_f64(bytes, value);
        }
        for value in path_range {
            native_optional_f64(bytes, Some(*value / LEN_TO_MM));
        }
        native_f64(bytes, *path_parameter);
        bytes.push(native_bool(*formula_flag));
        native_law_formula(bytes, target, formula)?;
        bytes.push(native_bool(*trailing_flag));
        native_revision_tail_head(bytes, "sweep surface", &form.cache, solved_cache)?;
        native_revision_tail_discontinuities(bytes, &construction.discontinuities)?;
        bytes.push(native_bool(construction.discontinuity_flag));
        bytes.push(0x10);
        return Ok(());
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "sweep_spl_sur")?;
    native_enum(bytes, construction.primary_kind);
    match &construction.layout {
        SweepSurfaceLayout::ProfileFirst {
            secondary_kind,
            directions,
            origin,
            parameters,
            formulas,
        } => {
            native_nurbs_curve(bytes, &native_loft_curve(target, profile, None)?)?;
            native_nurbs_curve(bytes, &native_loft_curve(target, spine, None)?)?;
            native_enum(bytes, *secondary_kind);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for formula in formulas.iter() {
                native_law_formula(bytes, target, formula)?;
            }
        }
        SweepSurfaceLayout::ExplicitFormula {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            formula_flag,
            formula,
            trailing_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 1);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / LEN_TO_MM);
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*formula_flag));
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
        }
        SweepSurfaceLayout::ExplicitGuide {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            guide_flags,
            guide_curve,
            guide_range,
            guide_modes,
            guide_parameters,
            trailing_flags,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 2);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / LEN_TO_MM);
            }
            native_f64(bytes, *path_parameter);
            for flag in guide_flags {
                bytes.push(native_bool(*flag));
            }
            let guide_curve = native_loft_curve(target, guide_curve, Some(*guide_range))?;
            native_nurbs_curve(bytes, &guide_curve)?;
            for value in guide_range {
                native_f64(bytes, *value);
            }
            for mode in guide_modes {
                native_i64(bytes, *mode);
            }
            for parameter in guide_parameters {
                native_f64(bytes, *parameter);
            }
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
        SweepSurfaceLayout::ExplicitSurface {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            singularity,
            support_surface,
            auxiliary_curve,
            support_flag,
            legacy_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 3);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / LEN_TO_MM, path_range[1] / LEN_TO_MM];
            let spine = native_loft_curve(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / LEN_TO_MM);
            }
            native_f64(bytes, *path_parameter);
            native_enum(bytes, *singularity);
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support_surface)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "sweep references missing support surface {support_surface}"
                    ))
                })?;
            native_embedded_surface(bytes, &support.geometry)?;
            bytes.push(native_bool(auxiliary_curve.is_some()));
            if let Some(curve) = auxiliary_curve {
                native_nurbs_curve(bytes, &native_loft_curve(target, curve, None)?)?;
            }
            bytes.push(native_bool(*support_flag));
            if let Some(flag) = legacy_flag {
                bytes.push(native_bool(*flag));
            }
        }
        SweepSurfaceLayout::LawDriven {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            first_law,
            first_mode,
            first_range,
            law_direction,
            path_mode,
            path_flag,
            path_range,
            path_parameter,
            second_law_flag,
            second_law,
            formula_mode,
            formula,
            trailing_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(
                    bytes,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_sweep_law(bytes, first_law, SweepLawSlot::LegacyFirst)?;
            native_i64(bytes, *first_mode);
            for value in first_range {
                native_f64(bytes, *value);
            }
            native_vector(bytes, [law_direction.x, law_direction.y, law_direction.z]);
            native_i64(bytes, *path_mode);
            bytes.push(native_bool(*path_flag));
            let spine = native_loft_curve(target, spine, Some(*path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value);
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*second_law_flag));
            native_sweep_law(bytes, second_law, SweepLawSlot::Second)?;
            native_i64(bytes, *formula_mode);
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
        }
    }
    native_nurbs_surface(
        bytes,
        solved_cache.ok_or_else(|| {
            CodecError::Malformed("sweep surface requires a solved native cache".into())
        })?,
    )?;
    native_f64(
        bytes,
        cache_fit_tolerance
            .ok_or_else(|| {
                CodecError::Malformed("sweep surface requires a native cache-fit tolerance".into())
            })?
            .get()
            / LEN_TO_MM,
    );
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

#[derive(Clone, Copy)]
struct LoftSurfaceRecord<'a> {
    sections: &'a [cadmpeg_ir::geometry::LoftSection; 2],
    revision_form: Option<&'a cadmpeg_ir::geometry::LoftRevisionForm>,
    parameters: &'a cadmpeg_ir::geometry::SplineSurfaceParameters,
    closures: &'a [i64; 2],
    singularities: &'a [i64; 2],
    mode: i64,
    bridge: &'a [cadmpeg_ir::geometry::LoftBridgeToken],
    solved_cache: Option<&'a NurbsSurface>,
}

fn encode_native_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    input: LoftSurfaceRecord<'_>,
) -> Result<(), CodecError> {
    let LoftSurfaceRecord {
        sections,
        revision_form,
        parameters,
        closures,
        singularities,
        mode,
        bridge,
        solved_cache,
    } = input;
    if let Some(form) = revision_form {
        let cadmpeg_ir::geometry::SplineSurfaceParameters::RevisionRanges { intervals } =
            parameters
        else {
            return Err(CodecError::Malformed(
                "revision-gated loft requires revision-native parameter ranges".into(),
            ));
        };
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "loft_spl_sur")?;
        native_i64(bytes, form.revision.get());
        for section in sections {
            native_loft_section(bytes, target, section, None)?;
        }
        for interval in intervals {
            for bound in interval {
                native_optional_f64(bytes, *bound);
            }
        }
        for flag in form.flags {
            bytes.push(native_bool(flag));
        }
        for value in form.ints {
            native_i64(bytes, value);
        }
        native_revision_tail_head(bytes, "loft surface", &form.cache, solved_cache)?;
        native_revision_tail_discontinuities(bytes, &form.discontinuities)?;
        bytes.push(native_bool(form.tail_flag));
        bytes.push(0x10);
        return Ok(());
    }
    native_surface_base(bytes, "spline")?;
    let cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges { ranges } = parameters else {
        return Err(CodecError::Malformed(
            "legacy loft requires ordered parameter ranges".into(),
        ));
    };
    bytes.push(0x0f);
    native_ident(bytes, "loft_spl_sur")?;
    for (section, range) in sections.iter().zip(ranges) {
        native_loft_section(bytes, target, section, Some(*range))?;
    }
    for range in ranges {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for closure in closures {
        native_enum(bytes, *closure);
    }
    for singularity in singularities {
        native_enum(bytes, *singularity);
    }
    native_i64(bytes, mode);
    for token in bridge {
        match token {
            cadmpeg_ir::geometry::LoftBridgeToken::Boolean(value) => {
                bytes.push(native_bool(*value));
            }
            cadmpeg_ir::geometry::LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
            cadmpeg_ir::geometry::LoftBridgeToken::Double(value) => native_f64(bytes, *value),
            cadmpeg_ir::geometry::LoftBridgeToken::Text(value) => native_string(bytes, value)?,
            cadmpeg_ir::geometry::LoftBridgeToken::Enum(value) => native_enum(bytes, *value),
        }
    }
    let solved_cache = solved_cache
        .ok_or_else(|| CodecError::Malformed("legacy loft requires a solved NURBS cache".into()))?;
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
        native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
    }
    bytes.push(0x10);
    Ok(())
}

#[derive(Clone, Copy)]
struct ExtrusionSurfaceRecord<'a> {
    directrix: &'a cadmpeg_ir::ids::CurveId,
    parameter_interval: [f64; 2],
    direction: Vector3,
    native_position: cadmpeg_ir::math::Point3,
    revision_form: Option<&'a cadmpeg_ir::geometry::RevisionSurfaceForm>,
    solved_cache: Option<&'a NurbsSurface>,
}

fn encode_native_extrusion(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    input: ExtrusionSurfaceRecord<'_>,
) -> Result<(), CodecError> {
    let ExtrusionSurfaceRecord {
        directrix,
        parameter_interval,
        direction,
        native_position,
        revision_form,
        solved_cache,
    } = input;
    let directrix = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "procedural surface {} references missing directrix {directrix}",
                procedural.id
            ))
        })?;
    let directrix_cache = native_interval_curve(
        directrix.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("source-less F3D carrier has no solved geometry".into())
        })?,
        parameter_interval,
    )?;
    let direction = [
        direction.x / LEN_TO_MM,
        direction.y / LEN_TO_MM,
        direction.z / LEN_TO_MM,
    ];
    let native_position = [
        native_position.x / LEN_TO_MM,
        native_position.y / LEN_TO_MM,
        native_position.z / LEN_TO_MM,
    ];
    if let Some(form) = revision_form {
        // Revision-gated layout: revision integer, the directrix as a nested
        // `intcurve` scope behind its sense flag, the directrix parameter
        // interval in the optional bool-gated encoding, the sweep direction and
        // model-space position, then the shared surface tail.
        if form.flags.len() != 1 {
            return Err(CodecError::Malformed(
                "revision-gated cyl_spl_sur requires one directrix sense flag".into(),
            ));
        }
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        native_ident(bytes, "cyl_spl_sur")?;
        native_i64(bytes, form.revision.get());
        native_ident(bytes, "intcurve")?;
        bytes.push(native_bool(form.flags[0]));
        bytes.push(0x0f);
        native_ident(bytes, "exact_int_cur")?;
        native_nurbs_curve(bytes, &directrix_cache)?;
        bytes.push(0x10);
        native_optional_f64(bytes, Some(parameter_interval[0]));
        native_optional_f64(bytes, Some(parameter_interval[1]));
        native_vector(bytes, direction);
        native_point(bytes, native_position);
        native_revision_surface_tail(bytes, "extrusion surface", form, solved_cache)?;
        bytes.push(0x10);
        return Ok(());
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cyl_spl_sur")?;
    native_f64(bytes, parameter_interval[0]);
    native_f64(bytes, parameter_interval[1]);
    native_vector(bytes, direction);
    native_point(bytes, native_position);
    native_nurbs_curve(bytes, &directrix_cache)?;
    if let Some(solved_cache) = solved_cache {
        native_nurbs_surface(bytes, solved_cache)?;
        if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
            native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
        }
    } else if procedural.cache_fit_tolerance().is_some() {
        return Err(CodecError::Malformed(
            "cache-less F3D extrusion cannot carry a cache-fit tolerance".into(),
        ));
    }
    bytes.push(0x10);
    Ok(())
}

fn native_optional_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: Option<&PcurveGeometry>,
) -> Result<(), CodecError> {
    if let Some(pcurve) = pcurve {
        native_nurbs_pcurve_block(bytes, pcurve)
    } else {
        native_ident(bytes, "nullbs")
    }
}

/// Write a nullable pcurve on `support` in the native chart of the support
/// fields `native_embedded_surface` writes.
fn native_optional_support_pcurve(
    bytes: &mut Vec<u8>,
    support: &SurfaceGeometry,
    pcurve: Option<&PcurveGeometry>,
) -> Result<(), CodecError> {
    if let Some(pcurve) = pcurve {
        native_nurbs_pcurve_payload(bytes, &native_support_pcurve(support, pcurve)?)
    } else {
        native_ident(bytes, "nullbs")
    }
}

fn native_radius_function_pcurve_block(
    bytes: &mut Vec<u8>,
    function: &PcurveGeometry,
) -> Result<(), CodecError> {
    let PcurveGeometry::Nurbs { nurbs } = function else {
        return Err(CodecError::NotImplemented(
            "variable-blend radius function must be a NURBS pcurve".into(),
        ));
    };
    let native = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
            nurbs.degree(),
            nurbs.knots().clone(),
            nurbs
                .control_points()
                .iter()
                .map(|point| cadmpeg_ir::math::Point2::new(point.u / LEN_TO_MM, point.v))
                .collect::<Vec<_>>(),
            nurbs.weights(),
            nurbs.periodic(),
        )
        .map_err(|error| CodecError::NotImplemented(error.to_string()))?,
    };
    native_nurbs_pcurve_block(bytes, &native)
}

fn native_variable_blend_value(
    bytes: &mut Vec<u8>,
    value: &cadmpeg_ir::geometry::VariableBlendValue,
    depth: usize,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{VariableBlendTerminal, VariableBlendValuePayload};
    if depth > 32 {
        return Err(CodecError::NotImplemented(
            "variable blend-value recursion exceeds 32 levels".into(),
        ));
    }
    native_string(bytes, value.payload.native_name())?;
    if value.payload.discriminator() != 1 {
        native_i64(bytes, value.payload.discriminator());
    }
    native_enum(bytes, value.calibrated);
    bytes.push(native_bool(value.modern_flag));
    match &value.payload {
        VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => {
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for radius in radii {
                native_f64(bytes, *radius / LEN_TO_MM);
            }
        }
        VariableBlendValuePayload::FixedWidth {
            parameters, width, ..
        } => {
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            native_f64(bytes, *width);
        }
        VariableBlendValuePayload::EdgeOffset {
            scalars, lengths, ..
        } => {
            for scalar in scalars {
                native_f64(bytes, *scalar);
            }
            for length in lengths {
                native_f64(bytes, *length / LEN_TO_MM);
            }
        }
        VariableBlendValuePayload::Functional {
            parameter,
            radius,
            function,
            terminal,
            ..
        } => {
            native_f64(bytes, *parameter);
            native_f64(bytes, *radius / LEN_TO_MM);
            native_radius_function_pcurve_block(bytes, function)?;
            match terminal {
                VariableBlendTerminal::Double(value) => native_f64(bytes, *value),
                VariableBlendTerminal::Text(value) => native_string(bytes, value)?,
            }
        }
        VariableBlendValuePayload::Constant {
            parameters,
            radius,
            variable_chamfer,
            chamfer_type,
            nested,
            ..
        } => {
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            native_f64(bytes, *radius / LEN_TO_MM);
            native_enum(bytes, *variable_chamfer);
            native_enum(bytes, *chamfer_type);
            native_variable_blend_value(bytes, nested, depth + 1)?;
        }
        VariableBlendValuePayload::Interpolated {
            parameter,
            radius,
            function,
            enum_count,
            enum_tagged,
            points,
            ..
        } => {
            native_f64(bytes, *parameter);
            native_f64(bytes, *radius / LEN_TO_MM);
            native_radius_function_pcurve_block(bytes, function)?;
            if *enum_tagged {
                native_enum(bytes, *enum_count);
            } else {
                native_i64(bytes, *enum_count);
            }
            native_i64(
                bytes,
                i64::try_from(points.len()).map_err(|_| {
                    CodecError::NotImplemented("variable blend point count exceeds i64".into())
                })?,
            );
            for point in points {
                native_f64(bytes, point.parameter);
                native_f64(bytes, point.radius / LEN_TO_MM);
                for tangent in point.tangents {
                    native_f64(bytes, tangent.unwrap_or(UNSET_VARIABLE_BLEND_TANGENT));
                }
                native_point(
                    bytes,
                    [
                        point.location.x / LEN_TO_MM,
                        point.location.y / LEN_TO_MM,
                        point.location.z / LEN_TO_MM,
                    ],
                );
                native_vector(bytes, [point.normal.x, point.normal.y, point.normal.z]);
            }
        }
    }
    Ok(())
}

fn native_vertex_blend_boundary(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    boundary: &cadmpeg_ir::geometry::VertexBlendBoundary,
    revision: bool,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::VertexBlendBoundaryGeometry;
    let kind = match &boundary.geometry {
        VertexBlendBoundaryGeometry::Circle { .. } => "circle",
        VertexBlendBoundaryGeometry::Degenerate { .. } => "deg",
        VertexBlendBoundaryGeometry::Pcurve { .. } => "pcurve",
        VertexBlendBoundaryGeometry::Plane { .. } => "plane",
    };
    if revision {
        native_ident(bytes, kind)?;
    } else {
        native_string(bytes, kind)?;
    }
    bytes.push(native_bool(boundary.boundary_type));
    // The magic item is a unit direction or the zero vector, not a
    // length-bearing location, so it takes no unit conversion.
    if revision {
        native_vector(
            bytes,
            [boundary.magic.x, boundary.magic.y, boundary.magic.z],
        );
    } else {
        native_point(
            bytes,
            [boundary.magic.x, boundary.magic.y, boundary.magic.z],
        );
    }
    bytes.push(native_bool(boundary.u_smoothing));
    bytes.push(native_bool(boundary.v_smoothing));
    native_f64(bytes, boundary.fullness);
    match &boundary.geometry {
        VertexBlendBoundaryGeometry::Circle {
            curve,
            curve_endpoints,
            twists,
            parameters,
            sense,
        } => {
            let range = if revision { None } else { Some(*parameters) };
            let curve = native_loft_curve(target, curve, range)?;
            native_nurbs_curve(bytes, &curve)?;
            if revision {
                for endpoint in curve_endpoints {
                    native_optional_f64(bytes, *endpoint);
                }
            }
            native_enum(bytes, twists.form());
            for twist in twists.entries() {
                if revision {
                    native_vector(
                        bytes,
                        [
                            twist.x / LEN_TO_MM,
                            twist.y / LEN_TO_MM,
                            twist.z / LEN_TO_MM,
                        ],
                    );
                } else {
                    native_point(
                        bytes,
                        [
                            twist.x / LEN_TO_MM,
                            twist.y / LEN_TO_MM,
                            twist.z / LEN_TO_MM,
                        ],
                    );
                }
            }
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            bytes.push(native_bool(*sense));
        }
        VertexBlendBoundaryGeometry::Degenerate { location, normals } => {
            native_point(
                bytes,
                [
                    location.x / LEN_TO_MM,
                    location.y / LEN_TO_MM,
                    location.z / LEN_TO_MM,
                ],
            );
            for normal in normals {
                native_vector(bytes, [normal.x, normal.y, normal.z]);
            }
        }
        VertexBlendBoundaryGeometry::Pcurve {
            surface,
            support_bounds,
            pcurve,
            sense,
            fit_tolerance,
        } => {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|candidate| candidate.id == *surface)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("vertex-blend support {surface} is missing"))
                })?;
            if revision {
                native_embedded_surface_with_bounds(bytes, &surface.geometry, support_bounds)?;
            } else {
                native_embedded_surface(bytes, &surface.geometry)?;
            }
            native_optional_support_pcurve(bytes, &surface.geometry, pcurve.as_ref())?;
            bytes.push(native_bool(*sense));
            native_f64(bytes, fit_tolerance.get());
        }
        VertexBlendBoundaryGeometry::Plane {
            normal,
            parameters,
            curve,
            curve_endpoints,
        } => {
            native_vector(bytes, [normal.x, normal.y, normal.z]);
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            let range = if revision { None } else { Some(*parameters) };
            let curve = native_loft_curve(target, curve, range)?;
            native_nurbs_curve(bytes, &curve)?;
            if revision {
                for endpoint in curve_endpoints {
                    native_optional_f64(bytes, *endpoint);
                }
            }
        }
    }
    Ok(())
}

fn encode_native_vertex_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::VertexBlendConstruction,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "VBL_SURF")?;
    if let Some(revision) = construction.revision {
        native_i64(bytes, revision.get());
    }
    native_i64(
        bytes,
        i64::try_from(construction.boundaries.len()).map_err(|_| {
            CodecError::NotImplemented("vertex-blend boundary count exceeds i64".into())
        })?,
    );
    for boundary in &construction.boundaries {
        native_vertex_blend_boundary(bytes, target, boundary, construction.revision.is_some())?;
    }
    native_i64(bytes, construction.grid_size);
    native_f64(bytes, construction.fit_tolerance.get() / LEN_TO_MM);
    bytes.push(0x10);
    Ok(())
}

/// Emit one revision-gated compound-loft scale block: counted profile
/// members, nullable path curve with optional endpoints, counted auxiliary
/// curves, and the tail integer.
fn native_revision_cl_scale(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    profile: &[cadmpeg_ir::geometry::LoftProfileMember],
    path: &cadmpeg_ir::geometry::LoftPath,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(profile.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft member count exceeds i64".into())
        })?,
    );
    for member in profile {
        native_i64(bytes, member.form.type_code());
        let curve = native_loft_curve(target, &member.profile.id, None)?;
        native_nurbs_curve(bytes, &curve)?;
        let endpoints = member.profile.endpoints.ok_or_else(|| {
            CodecError::Malformed(
                "revision compound-loft members require optional endpoints".into(),
            )
        })?;
        for value in endpoints {
            native_optional_f64(bytes, value);
        }
        match &member.form {
            cadmpeg_ir::geometry::LoftMemberForm::Support {
                surface,
                support_bounds,
                pcurve,
                first_flag,
                ..
            } => {
                if let Some(surface_id) = surface {
                    let surface = target
                        .model
                        .surfaces
                        .iter()
                        .find(|surface| surface.id == *surface_id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "compound loft references missing surface {surface_id}"
                            ))
                        })?;
                    native_embedded_surface_with_bounds(bytes, &surface.geometry, support_bounds)?;
                    native_optional_support_pcurve(bytes, &surface.geometry, pcurve.as_ref())?;
                } else {
                    native_ident(bytes, "null_surface")?;
                    native_optional_pcurve(bytes, pcurve.as_ref())?;
                }
                bytes.push(native_bool(*first_flag));
            }
            cadmpeg_ir::geometry::LoftMemberForm::PcurvePair {
                pcurve,
                secondary_pcurve,
                ..
            } => {
                native_optional_pcurve(bytes, pcurve.as_ref())?;
                native_optional_pcurve(bytes, secondary_pcurve.as_ref())?;
            }
        }
        native_loft_member_tail(bytes, &member.form);
    }
    if let Some(path_curve) = &path.path {
        let curve = native_loft_curve(target, &path_curve.id, None)?;
        native_nurbs_curve(bytes, &curve)?;
        if let Some(endpoints) = path_curve.endpoints {
            for value in endpoints {
                native_optional_f64(bytes, value);
            }
        }
    } else {
        native_ident(bytes, "null_curve")?;
    }
    native_i64(
        bytes,
        i64::try_from(path.auxiliaries.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft auxiliary count exceeds i64".into())
        })?,
    );
    for auxiliary in &path.auxiliaries {
        let auxiliary = native_loft_curve(target, auxiliary, None)?;
        native_nurbs_curve(bytes, &auxiliary)?;
    }
    native_i64(bytes, path.flag);
    Ok(())
}

fn encode_native_revision_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::RevisionCompoundLoftConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cl_loft_spl_sur")?;
    native_i64(bytes, construction.revision().get());
    native_revision_tail_head(
        bytes,
        "compound-loft surface",
        &construction.cache().to_raw(),
        solved_cache,
    )?;
    native_revision_tail_discontinuities(
        bytes,
        &cadmpeg_ir::scalar::FiniteReal::raw_lanes(construction.discontinuities()),
    )?;
    bytes.push(native_bool(construction.tail_flag()));
    native_revision_cl_scale(
        bytes,
        target,
        &construction
            .base_profile()
            .iter()
            .map(cadmpeg_ir::geometry::LoftProfileMember::to_raw)
            .collect::<Vec<_>>(),
        &construction.base_path().to_raw(),
    )?;
    native_i64(
        bytes,
        i64::try_from(construction.entries().len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft entry count exceeds i64".into())
        })?,
    );
    for entry in construction.entries() {
        let entry = entry.to_raw();
        native_revision_cl_scale(bytes, target, &entry.profile, &entry.path)?;
        native_f64(bytes, entry.parameter);
    }
    for flag in construction.flags() {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, 0);
    for flag in construction.kind_flags() {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.direction().selector());
    match construction.direction() {
        cadmpeg_ir::geometry::CompoundLoftDirection::Vector { value } => {
            native_vector(bytes, [value.x, value.y, value.z]);
        }
        cadmpeg_ir::geometry::CompoundLoftDirection::Curve { curve, .. } => {
            let curve = native_loft_curve(target, curve, None)?;
            native_nurbs_curve(bytes, &curve)?;
        }
    }
    for value in construction.tail().interval() {
        native_optional_f64(bytes, value.map(cadmpeg_ir::scalar::FiniteReal::get));
    }
    if let Some(curve) = construction.tail().curve() {
        let curve = native_loft_curve(target, curve, None)?;
        native_nurbs_curve(bytes, &curve)?;
    }
    bytes.push(0x10);
    Ok(())
}

fn encode_native_revision_g2_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::RevisionG2BlendConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "g2_blend_spl_sur")?;
    native_i64(bytes, construction.revision().get());
    for parameter in construction.leading_parameters() {
        native_f64(bytes, parameter.get());
    }
    for side in construction.sides() {
        native_rolling_ball_side(bytes, target, &side.to_raw())?;
    }
    let center_range = match construction.center_range() {
        [Some(lower), Some(upper)] => Some([lower.get(), upper.get()]),
        _ => None,
    };
    let center = native_loft_curve(target, construction.center(), center_range)?;
    native_nurbs_curve(bytes, &center)?;
    for endpoint in construction.center_range() {
        native_optional_f64(bytes, endpoint.map(cadmpeg_ir::scalar::FiniteReal::get));
    }
    for radius in construction.radii() {
        native_f64(bytes, radius.get() / LEN_TO_MM);
    }
    match construction.radius_selector() {
        cadmpeg_ir::geometry::RollingBallRadiusSelector::None {} => native_enum(bytes, -1),
        cadmpeg_ir::geometry::RollingBallRadiusSelector::Value { value } => {
            native_enum(bytes, value.get());
        }
    }
    for range in [construction.u_range(), construction.v_range()] {
        for endpoint in range {
            native_optional_f64(bytes, endpoint.map(cadmpeg_ir::scalar::FiniteReal::get));
        }
    }
    native_i64(bytes, construction.shape_prefix());
    native_f64(bytes, construction.shape_parameter().get());
    native_f64(bytes, construction.shape_length().get() / LEN_TO_MM);
    native_i64(bytes, construction.shape_tail());
    native_revision_tail_head(
        bytes,
        "G2 blend",
        &construction.cache().to_raw(),
        solved_cache,
    )?;
    native_revision_tail_discontinuities(
        bytes,
        &cadmpeg_ir::scalar::FiniteReal::raw_lanes(construction.discontinuities()),
    )?;
    bytes.push(native_bool(construction.tail_flag()));
    for extension in construction.tail_extensions() {
        native_i64(bytes, extension);
    }
    bytes.push(0x10);
    Ok(())
}

fn encode_native_variable_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::VariableBlendConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(
        bytes,
        match construction.subtype {
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::VariableBlend => "var_blend_spl_sur",
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::SurfaceSurface => {
                "srf_srf_v_bl_spl_sur"
            }
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::CurveCurve => "crv_crv_v_bl_spl_sur",
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::CurveSurface => {
                "crv_srf_v_bl_spl_sur"
            }
            cadmpeg_ir::geometry::VariableBlendSurfaceSubtype::SurfaceCurveFree => {
                "sfcv_free_bl_spl_sur"
            }
        },
    )?;
    native_i64(bytes, construction.revision.get());
    for side in &construction.sides {
        native_rolling_ball_side(bytes, target, side)?;
    }
    let slice_range = match construction.slice_range {
        [Some(lower), Some(upper)] => Some([lower, upper]),
        _ => Some(construction.u_range),
    };
    let slice = native_loft_curve(target, &construction.slice, slice_range)?;
    native_nurbs_curve(bytes, &slice)?;
    for endpoint in construction.slice_range {
        bytes.push(native_bool(endpoint.is_some()));
        if let Some(value) = endpoint {
            native_f64(bytes, value);
        }
    }
    for offset in construction.offsets {
        native_f64(bytes, offset / LEN_TO_MM);
    }
    native_enum(
        bytes,
        match construction.radii {
            cadmpeg_ir::geometry::VariableBlendRadii::Single { .. } => 0,
            cadmpeg_ir::geometry::VariableBlendRadii::Two { .. } => 1,
        },
    );
    native_variable_blend_value(bytes, construction.radii.first(), 0)?;
    if let Some(second) = construction.radii.second() {
        native_variable_blend_value(bytes, second, 0)?;
    }
    if let Some(cross_section) = &construction.cross_section {
        use cadmpeg_ir::geometry::VariableBlendCrossSection as CrossSection;
        match cross_section {
            CrossSection::Circular {} => native_enum(bytes, 0),
            CrossSection::Thumbweights { parameters } => {
                native_enum(bytes, 1);
                for parameter in parameters {
                    native_f64(bytes, *parameter);
                }
            }
            CrossSection::RoundedChamfer { radius } => {
                native_enum(bytes, 3);
                bytes.push(native_bool(radius.is_some()));
                if let Some(radius) = radius {
                    native_variable_blend_value(bytes, radius, 0)?;
                }
            }
            CrossSection::G2Round { parameters } => {
                native_enum(bytes, 7);
                for parameter in parameters {
                    native_f64(bytes, *parameter);
                }
            }
            CrossSection::UnclassifiedBare { selector } => {
                native_enum(bytes, selector.native_selector());
            }
        }
    }
    for range in [construction.u_range.map(Some), [construction.v_lower, None]] {
        for endpoint in range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    }
    native_i64(bytes, construction.cache.shape_prefix());
    native_f64(bytes, construction.shape_parameter);
    native_f64(bytes, construction.shape_length / LEN_TO_MM);
    native_i64(bytes, construction.shape_tail);
    native_variable_blend_revision_tail_head(bytes, &construction.cache, solved_cache)?;
    native_revision_tail_discontinuities(bytes, &construction.discontinuities)?;
    bytes.push(native_bool(construction.tail_flag));
    for extension in construction.tail_extensions {
        native_i64(bytes, extension);
    }
    if let Some(secondary) = &construction.secondary_curve {
        let secondary_range = match secondary.parameter_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        };
        let curve = native_loft_curve(target, &secondary.curve, secondary_range)?;
        native_nurbs_curve(bytes, &curve)?;
        for endpoint in secondary.parameter_range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    } else {
        native_ident(bytes, "null_curve")?;
    }
    bytes.push(native_bool(matches!(
        construction.convexity,
        cadmpeg_ir::geometry::VariableBlendConvexity::Convex
    )));
    bytes.push(native_bool(matches!(
        construction.render_mode,
        cadmpeg_ir::geometry::VariableBlendRenderMode::RollingBallEnvelope
    )));
    for endpoint in construction.post_range {
        bytes.push(native_bool(endpoint.is_some()));
        if let Some(value) = endpoint {
            native_f64(bytes, value);
        }
    }
    native_optional_pcurve(bytes, construction.post_pcurve.as_ref())?;
    if let Some(post_curve) = &construction.post_curve {
        let post_range = match construction.post_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        };
        let post_curve = native_loft_curve(target, post_curve, post_range)?;
        native_nurbs_curve(bytes, &post_curve)?;
    } else {
        native_ident(bytes, "nullbs")?;
    }
    bytes.push(0x10);
    Ok(())
}

fn native_rolling_ball_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::RollingBallSide,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::VariableBlendSupportKind;
    native_string(
        bytes,
        match side.support_kind {
            VariableBlendSupportKind::CosineCurve => "blend_support_cos_curve",
            VariableBlendSupportKind::Curve => "blend_support_curve",
            VariableBlendSupportKind::PointCurve => "blend_support_point_curve",
            VariableBlendSupportKind::Surface => "blend_support_surface",
            VariableBlendSupportKind::ZeroCurve => "blend_support_zero_curve",
        },
    )?;
    let support_geometry = if let Some(support) = &side.surface {
        let id = &support.surface;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("rolling-ball support {id} is missing"))
            })?;
        native_embedded_surface(bytes, &surface.geometry)?;
        if matches!(
            surface.geometry,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(_)
                    | SolvedSurfaceGeometry::Cone(_)
                    | SolvedSurfaceGeometry::Sphere(_)
                    | SolvedSurfaceGeometry::Torus(_)
            )
        ) {
            bytes.truncate(bytes.len() - 4);
        }
        for range in support.parameter_ranges {
            for endpoint in range {
                bytes.push(native_bool(endpoint.is_some()));
                if let Some(value) = endpoint {
                    native_f64(bytes, value);
                }
            }
        }
        Some(&surface.geometry)
    } else {
        native_ident(bytes, "null_surface")?;
        None
    };
    if let Some(support) = &side.curve {
        let id = &support.curve;
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("rolling-ball side curve {id} is missing"))
            })?;
        let curve = native_spline_field_curve(
            curve.geometry.solved().ok_or_else(|| {
                cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
            })?,
            match support.parameter_range {
                [Some(lower), Some(upper)] => Some([lower, upper]),
                _ => native_pcurve_knot_domain(side.pcurve.as_ref())?,
            },
        )?;
        native_nurbs_curve(bytes, &curve)?;
        for endpoint in support.parameter_range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    } else {
        native_ident(bytes, "null_curve")?;
    }
    match support_geometry {
        Some(geometry) => native_optional_support_pcurve(bytes, geometry, side.pcurve.as_ref())?,
        None => native_optional_pcurve(bytes, side.pcurve.as_ref())?,
    }
    native_point(
        bytes,
        [
            side.location.x / LEN_TO_MM,
            side.location.y / LEN_TO_MM,
            side.location.z / LEN_TO_MM,
        ],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    if let Some(extension) = &side.extension {
        native_i64(bytes, extension.value);
        native_optional_pcurve(bytes, extension.pcurve.as_ref())?;
    }
    Ok(())
}

fn native_rolling_ball_third_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::RollingBallThirdSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "rolling-ball third support {} is missing",
                side.surface
            ))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "rolling-ball third-side curve {} is missing",
                side.curve
            ))
        })?;
    let pcurve = [
        side.pcurve.as_ref(),
        side.secondary_pcurve.as_ref(),
        side.tertiary_pcurve.as_ref(),
    ]
    .into_iter()
    .flatten()
    .find(|pcurve| matches!(pcurve, PcurveGeometry::Nurbs { .. }));
    let curve = native_spline_field_curve(
        curve.geometry.solved().ok_or_else(|| {
            cadmpeg_core::CodecError::NotImplemented("carrier has no solved geometry".into())
        })?,
        native_pcurve_knot_domain(pcurve)?,
    )?;
    native_nurbs_curve(bytes, &curve)?;
    native_optional_support_pcurve(bytes, &surface.geometry, side.pcurve.as_ref())?;
    native_vector(
        bytes,
        [side.direction.x, side.direction.y, side.direction.z],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    native_i64(bytes, side.extension);
    native_optional_pcurve(bytes, side.tertiary_pcurve.as_ref())?;
    bytes.push(native_bool(side.flag));
    Ok(())
}

fn encode_complete_native_rolling_ball(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::RollingBallConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(
        bytes,
        if construction.third.is_some() {
            "sss_blend_spl_sur"
        } else {
            "rb_blend_spl_sur"
        },
    )?;
    native_i64(bytes, construction.revision.get());
    for side in &construction.sides {
        native_rolling_ball_side(bytes, target, side)?;
    }
    let slice_range = match construction.slice_range {
        [Some(lower), Some(upper)] => Some([lower, upper]),
        _ => match construction.u_range {
            [Some(lower), Some(upper)] => Some([lower, upper]),
            _ => None,
        },
    };
    let slice = native_loft_curve(target, &construction.slice, slice_range)?;
    native_nurbs_curve(bytes, &slice)?;
    for endpoint in construction.slice_range {
        bytes.push(native_bool(endpoint.is_some()));
        if let Some(value) = endpoint {
            native_f64(bytes, value);
        }
    }
    for offset in construction.offsets {
        native_f64(bytes, offset / LEN_TO_MM);
    }
    match construction.radius_selector {
        cadmpeg_ir::geometry::RollingBallRadiusSelector::None {} => native_enum(bytes, -1),
        cadmpeg_ir::geometry::RollingBallRadiusSelector::Value { value } => {
            native_f64(bytes, value);
        }
    }
    for range in [construction.u_range, construction.v_range] {
        for endpoint in range {
            bytes.push(native_bool(endpoint.is_some()));
            if let Some(value) = endpoint {
                native_f64(bytes, value);
            }
        }
    }
    native_i64(bytes, construction.shape_prefix);
    for parameter in construction.parameters {
        native_f64(bytes, parameter);
    }
    native_i64(bytes, construction.tail);
    native_revision_tail_head(
        bytes,
        "rolling-ball blend",
        &construction.cache,
        solved_cache,
    )?;
    native_revision_tail_discontinuities(bytes, &construction.discontinuities)?;
    bytes.push(native_bool(construction.tail_flag));
    if let Some(third) = &construction.third {
        native_rolling_ball_third_side(bytes, target, third)?;
    }
    for extension in construction.tail_extensions {
        native_i64(bytes, extension);
    }
    bytes.push(0x10);
    Ok(())
}

#[derive(Clone, Copy)]
struct RollingBallSurfaceRecord<'a> {
    supports: &'a [Option<cadmpeg_ir::geometry::BlendSupport>; 2],
    spine: Option<&'a cadmpeg_ir::ids::CurveId>,
    radius: &'a BlendRadiusLaw,
    cross_section: &'a cadmpeg_ir::geometry::BlendCrossSection,
    solved_cache: &'a NurbsSurface,
}

fn encode_native_rolling_ball(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    input: RollingBallSurfaceRecord<'_>,
) -> Result<(), CodecError> {
    let RollingBallSurfaceRecord {
        supports,
        spine,
        radius,
        cross_section,
        solved_cache,
    } = input;
    if *cross_section != cadmpeg_ir::geometry::BlendCrossSection::Circular {
        return Err(CodecError::NotImplemented(
            "source-less rb_blend_spl_sur requires a circular cross-section".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "rb_blend_spl_sur")?;
    for (side, support) in supports.iter().enumerate() {
        let Some(support) = support else { continue };
        if support.reversed {
            return Err(CodecError::NotImplemented(
                "source-less rb_blend_spl_sur reversed support is not defined".into(),
            ));
        }
        let carrier = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support.surface)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "procedural surface {} references missing support {}",
                    procedural.id, support.surface
                ))
            })?;
        native_string(bytes, "blend_support_surface")?;
        native_subident(bytes, if side == 0 { "plane" } else { "sphere" })?;
        native_embedded_surface(bytes, &carrier.geometry)?;
    }
    let spine = spine.ok_or_else(|| {
        CodecError::Malformed("source-less rb_blend_spl_sur lacks a spine".into())
    })?;
    let spine = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *spine)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("blend references missing spine {spine}"))
        })?;
    let spine_range = [
        solved_cache.u_knots().first().copied().ok_or_else(|| {
            CodecError::Malformed("rolling-ball solved surface has no U knot domain".into())
        })?,
        solved_cache.u_knots().last().copied().ok_or_else(|| {
            CodecError::Malformed("rolling-ball solved surface has no U knot domain".into())
        })?,
    ];
    let spine = native_interval_curve(
        spine.geometry.solved().ok_or_else(|| {
            CodecError::NotImplemented("source-less F3D carrier has no solved geometry".into())
        })?,
        spine_range,
    )?;
    native_nurbs_curve(bytes, &spine)?;
    let (start, end) = match radius {
        BlendRadiusLaw::Constant { signed_radius } => (signed_radius.get(), signed_radius.get()),
        BlendRadiusLaw::Linear { start, end } => (start.get(), end.get()),
        BlendRadiusLaw::Law { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less rb_blend_spl_sur explicit radius law is not defined".into(),
            ))
        }
    };
    native_f64(bytes, start / LEN_TO_MM);
    native_f64(bytes, end / LEN_TO_MM);
    native_enum(bytes, -1);
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
        native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
    }
    bytes.push(0x10);
    Ok(())
}

pub(super) fn native_nurbs_curve(
    bytes: &mut Vec<u8>,
    curve: &NurbsCurve,
) -> Result<(), CodecError> {
    let poles = curve.pole_rows();
    native_ident(
        bytes,
        if matches!(poles, NurbsPoles3::Rational { .. }) {
            "nurbs"
        } else {
            "nubs"
        },
    )?;
    native_i64(bytes, i64::from(curve.degree()));
    native_enum(bytes, if curve.periodic() { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(curve.knots()))
            .map_err(|_| CodecError::NotImplemented("F3D unique-knot count exceeds i64".into()))?,
    );
    native_nurbs_knots(bytes, curve.knots())?;
    match poles {
        NurbsPoles3::Polynomial { points } => {
            for point in points {
                native_f64(bytes, point.x / LEN_TO_MM);
                native_f64(bytes, point.y / LEN_TO_MM);
                native_f64(bytes, point.z / LEN_TO_MM);
            }
        }
        NurbsPoles3::Rational { points } => {
            for pole in points {
                native_f64(bytes, pole.point.x / LEN_TO_MM);
                native_f64(bytes, pole.point.y / LEN_TO_MM);
                native_f64(bytes, pole.point.z / LEN_TO_MM);
                native_f64(bytes, pole.weight.get());
            }
        }
    }
    Ok(())
}

fn native_spline_field_curve(
    geometry: &SolvedCurveGeometry,
    parameter_range: Option<[f64; 2]>,
) -> Result<NurbsCurve, CodecError> {
    match (geometry, parameter_range) {
        (SolvedCurveGeometry::Nurbs(curve), _) => Ok(curve.clone()),
        (_, Some(range)) => native_interval_curve(geometry, range),
        (SolvedCurveGeometry::Circle(_), None) => {
            native_interval_curve(geometry, [0.0, std::f64::consts::TAU])
        }
        (SolvedCurveGeometry::Ellipse(_), None) => {
            native_interval_curve(geometry, [0.0, std::f64::consts::TAU])
        }
        _ => Err(CodecError::NotImplemented(
            "source-less F3D spline field lacks a finite curve domain".into(),
        )),
    }
}

fn native_pcurve_knot_domain(
    pcurve: Option<&PcurveGeometry>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let Some(PcurveGeometry::Nurbs { nurbs }) = pcurve else {
        return Ok(None);
    };
    Ok(Some([
        *nurbs
            .knots()
            .first()
            .ok_or_else(|| CodecError::Malformed("pcurve has no knot domain".into()))?,
        *nurbs
            .knots()
            .last()
            .ok_or_else(|| CodecError::Malformed("pcurve has no knot domain".into()))?,
    ]))
}

fn native_interval_curve(
    geometry: &SolvedCurveGeometry,
    parameter_range: [f64; 2],
) -> Result<NurbsCurve, CodecError> {
    let parameter_range = IncreasingParameterInterval::new(parameter_range).ok_or_else(|| {
        CodecError::NotImplemented(
            "source-less F3D interval curve requires a finite ordered range".into(),
        )
    })?;
    native_increasing_interval_curve(geometry, parameter_range)
}

fn native_increasing_interval_curve(
    geometry: &SolvedCurveGeometry,
    parameter_range: IncreasingParameterInterval,
) -> Result<NurbsCurve, CodecError> {
    let parameter_range = parameter_range.endpoints();
    match geometry {
        SolvedCurveGeometry::Nurbs(curve) => Ok(curve.clone()),
        SolvedCurveGeometry::Line(line_curve) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            let point = |parameter: f64| {
                Point3::new(
                    origin.x + parameter * direction.x,
                    origin.y + parameter * direction.y,
                    origin.z + parameter * direction.z,
                )
            };
            NurbsCurve::from_lanes(
                1,
                vec![
                    parameter_range[0],
                    parameter_range[0],
                    parameter_range[1],
                    parameter_range[1],
                ],
                vec![point(parameter_range[0]), point(parameter_range[1])],
                None,
                false,
            )
            .map_err(|error| CodecError::NotImplemented(error.to_string()))
        }
        SolvedCurveGeometry::Circle(circle_curve) => {
            let center = circle_curve.center().get();
            let axis = circle_curve.frame().axis().as_raw();
            let ref_direction = circle_curve.frame().reference().as_raw();
            let radius = circle_curve.radius().get();
            native_conic_interval_curve(
                center,
                *axis,
                *ref_direction,
                radius,
                radius,
                parameter_range,
            )
        }
        SolvedCurveGeometry::Ellipse(ellipse_curve) => {
            let center = ellipse_curve.center().get();
            let axis = ellipse_curve.frame().axis().as_raw();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            native_conic_interval_curve(
                center,
                *axis,
                *major_direction,
                major_radius,
                minor_radius,
                parameter_range,
            )
        }
        _ => Err(CodecError::NotImplemented(
            "source-less F3D interval construction requires a NURBS, line, circle, or ellipse source curve".into(),
        )),
    }
}

/// Quarter-turn spans one generated conic NURBS may carry. Each span adds two
/// control points, so this bounds the generated curve before it is built.
const MAX_GENERATED_CONIC_SPANS: usize = 1 << 16;

fn native_conic_interval_curve(
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
    parameter_range: [f64; 2],
) -> Result<NurbsCurve, CodecError> {
    let axis_norm = axis.norm();
    let axis = axis.scale(1.0 / axis_norm);
    let major_norm = major_direction.norm();
    let major_direction = major_direction.scale(1.0 / major_norm);
    let minor_direction = axis.cross(major_direction);
    let minor_norm = minor_direction.norm();
    if !minor_norm.is_finite() || minor_norm == 0.0 {
        return Err(CodecError::NotImplemented(
            "source-less F3D conic axis and major direction must not be parallel".into(),
        ));
    }
    let minor_direction = minor_direction.scale(1.0 / minor_norm);
    // `native_interval_curve`, the only caller, refuses a range that is not
    // finite and ordered before it reaches either conic arm, so `delta` is
    // positive and the quarter-turn count is at least one span. No floor
    // stands here.
    let delta = parameter_range[1] - parameter_range[0];
    let span_count = (delta / std::f64::consts::FRAC_PI_2).ceil();
    let too_large = || {
        CodecError::NotImplemented(
            "source-less F3D conic interval exceeds addressable NURBS cardinality".into(),
        )
    };
    let spans = truncate_f64_to_usize(span_count)
        .filter(|spans| *spans <= MAX_GENERATED_CONIC_SPANS)
        .ok_or_else(too_large)?;
    let step = delta / f64_from_index(spans).ok_or_else(too_large)?;

    let mut control_points = Vec::new();
    let mut weights = Vec::new();
    let mut knots = Vec::new();
    let point = |angle: f64, scale: f64| {
        let major_scale = major_radius * angle.cos() * scale;
        let minor_scale = minor_radius * angle.sin() * scale;
        Point3::new(
            center.x + major_direction.x * major_scale + minor_direction.x * minor_scale,
            center.y + major_direction.y * major_scale + minor_direction.y * minor_scale,
            center.z + major_direction.z * major_scale + minor_direction.z * minor_scale,
        )
    };
    for span in 0..spans {
        let start = parameter_range[0] + step * f64_from_index(span).ok_or_else(too_large)?;
        let end = start + step;
        let middle = (start + end) * 0.5;
        let weight = (step * 0.5).cos();
        if !weight.is_finite() || weight <= 0.0 {
            return Err(CodecError::NotImplemented(
                "source-less F3D conic interval has an invalid rational span".into(),
            ));
        }
        if span == 0 {
            control_points.push(point(start, 1.0));
            weights.push(1.0);
            knots.extend([start, start, start]);
        } else {
            knots.extend([start, start]);
        }
        control_points.push(point(middle, 1.0 / weight));
        weights.push(weight);
        control_points.push(point(end, 1.0));
        weights.push(1.0);
        if span + 1 == spans {
            knots.extend([end, end, end]);
        }
    }
    NurbsCurve::from_lanes(2, knots, control_points, Some(weights), false)
        .map_err(|error| CodecError::NotImplemented(error.to_string()))
}

#[cfg(test)]
mod native_interval_curve_tests {
    use super::{native_interval_curve, native_spline_field_curve};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};

    const EPS_GENERATED_CURVE: f64 = 1.0e-12;
    const MAX_INDEX_FLOAT: f64 = if usize::BITS == 64 {
        18_446_744_073_709_551_616.0
    } else {
        4_294_967_295.0
    };

    #[test]
    fn generated_circle_interval_lowers_to_exact_rational_nurbs() {
        let curve = native_interval_curve(
            &SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    Point3::new(2.0, 3.0, 4.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    5.0,
                )
                .unwrap(),
            ),
            [0.0, std::f64::consts::PI],
        )
        .expect("generated circle interval");
        let midpoint = cadmpeg_ir::eval::decode::nurbs_curve_point_at(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve,
            std::f64::consts::FRAC_PI_2,
        )
        .expect("evaluate generated circle interval");
        assert!((midpoint.x - 2.0).abs() < EPS_GENERATED_CURVE);
        assert!((midpoint.y - 8.0).abs() < EPS_GENERATED_CURVE);
        assert!((midpoint.z - 4.0).abs() < EPS_GENERATED_CURVE);
    }

    /// The quarter-turn span count needs no floor. `native_interval_curve`
    /// refuses a range that is not finite and ordered before either conic arm
    /// is reached, so the shortest range that can reach one still states a
    /// whole span, and a range that states no span is refused at the route.
    #[test]
    fn the_shortest_admitted_conic_range_states_one_span() {
        let circle = SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        );
        let curve = native_interval_curve(&circle, [0.0, f64::MIN_POSITIVE])
            .expect("the shortest admitted range");
        assert_eq!(curve.degree(), 2);
        assert_eq!(curve.knots().len(), 6);
        assert_eq!(curve.control_points().len(), 3);

        let error =
            native_interval_curve(&circle, [1.0, 1.0]).expect_err("a range that states no span");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
        assert!(
            error.to_string().contains("finite ordered range"),
            "{error}"
        );
    }

    #[test]
    fn generated_ellipse_interval_preserves_both_radii() {
        let curve = native_interval_curve(
            &SolvedCurveGeometry::Ellipse(
                cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                    Point3::new(-1.0, 2.0, 0.5),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    6.0,
                    2.0,
                )
                .unwrap(),
            ),
            [0.0, std::f64::consts::FRAC_PI_2],
        )
        .expect("generated ellipse interval");
        assert_eq!(curve.control_points()[0], Point3::new(5.0, 2.0, 0.5));
        assert!((curve.control_points()[2].x + 1.0).abs() < EPS_GENERATED_CURVE);
        assert_eq!(curve.control_points()[2].y, 4.0);
        assert_eq!(curve.control_points()[2].z, 0.5);
        assert_eq!(
            curve.knots().as_slice(),
            vec![
                0.0,
                0.0,
                0.0,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ]
        );
    }

    #[test]
    fn generated_domainless_circle_uses_its_full_natural_domain() {
        let geometry = SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                3.0,
            )
            .unwrap(),
        );
        let curve = native_spline_field_curve(&geometry, None)
            .expect("generated domainless circle spline field");
        assert_eq!(curve.knots().first().copied(), Some(0.0));
        assert_eq!(curve.knots().last().copied(), Some(std::f64::consts::TAU));
        assert_eq!(curve.control_points().len(), 9);
        assert_eq!(curve.weights().map(|weights| weights.len()), Some(9));
    }

    #[test]
    fn generated_domainless_line_remains_rejected() {
        let geometry = SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        );
        assert!(native_spline_field_curve(&geometry, None).is_err());
    }

    #[test]
    fn source_less_sweep_refuses_unallocatable_conic_intervals_before_writing() {
        use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use cadmpeg_ir::geometry::surface_payloads::SweepSurfacePayload;
        use cadmpeg_ir::geometry::{analytic::CircleCurve, SweepSurfaceLayout};
        use std::io::Cursor;

        let decoded = crate::F3dCodec
            .decode(
                &mut Cursor::new(crate::test_support::zip_test::f3d_with_smbh(
                    &crate::test_support::smbh_surfaces_test::synthetic_law_driven_sweep_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .unwrap();
        let (mut target, _, _) = decoded.into_parts();
        target.source = None;
        target.native = cadmpeg_ir::native::Native::default();

        for (range, representable) in [
            ([0.0, std::f64::consts::PI], true),
            ([-f64::MAX, f64::MAX], false),
            ([0.0, f64::MAX], false),
            ([0.0, MAX_INDEX_FLOAT], false),
            ([0.0, (MAX_INDEX_FLOAT) * 0.75], false),
            ([0.0, (MAX_INDEX_FLOAT) * 0.125], false),
        ] {
            let procedural = &mut target.model.procedural_surfaces[0];
            let ProceduralSurfaceDefinition::Sweep(payload) = procedural.definition() else {
                panic!("sweep construction")
            };
            let profile = payload.profile().clone();
            let mut native = payload
                .native()
                .as_deref()
                .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw)
                .unwrap();
            let SweepSurfaceLayout::LawDriven { profile_range, .. } = &mut native.layout else {
                panic!("law-driven layout")
            };
            *profile_range = range;
            let replacement = SweepSurfacePayload::try_new(
                profile.clone(),
                payload.spine().clone(),
                Some(Box::new(native)),
            )
            .expect("finite interval endpoints are admitted by the construction");
            procedural.edit_definition(|definition| {
                *definition = ProceduralSurfaceDefinition::Sweep(replacement);
            });
            target
                .model
                .curves
                .iter_mut()
                .find(|curve| curve.id == profile)
                .unwrap()
                .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                CircleCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    10.0,
                )
                .unwrap(),
            ));
            let mut output = vec![0x93, 0x2a];
            let result = crate::F3dCodec
                .plan(EncodeInput::new(&target, None), TargetRequest::Inherit)
                .and_then(|plan| plan.write_to(&mut output));
            if representable {
                result.expect("ordinary circle interval writes");
                let decoded = crate::F3dCodec
                    .decode(&mut Cursor::new(&output[2..]), &DecodeOptions::default())
                    .expect("ordinary circle interval decodes");
                assert_eq!(decoded.ir().model.procedural_surfaces.len(), 1);
            } else {
                let error = result.expect_err("unallocatable conic interval must be refused");
                assert!(error.to_string().contains("conic"), "{error}");
                assert_eq!(output, [0x93, 0x2a]);
            }
        }
    }
}

fn native_spring_pcurve(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    support: &cadmpeg_ir::geometry::SpringSupport,
    pcurve: &PcurveGeometry,
) -> Result<(), CodecError> {
    match support {
        cadmpeg_ir::geometry::SpringSupport::Surface(surface_id) => {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "spring references missing support {surface_id}"
                    ))
                })?;
            let native = native_support_pcurve(&surface.geometry, pcurve)?;
            native_nurbs_pcurve_payload(bytes, &native)
        }
        cadmpeg_ir::geometry::SpringSupport::Ranges(_) => native_nurbs_pcurve_block(bytes, pcurve),
    }
}

pub(crate) fn native_procedural_curve(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    curve_id: &cadmpeg_ir::ids::CurveId,
    solved_cache: &NurbsCurve,
) -> Result<bool, CodecError> {
    let mut definitions =
        target.model.procedural_curves.iter().filter(|procedural| {
            target.model.procedural_curve_owner(&procedural.id) == Some(curve_id)
        });
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "curve {curve_id} has multiple procedural constructions"
        )));
    }
    if matches!(
        procedural.definition(),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. }
    ) {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D unknown procedural curve {} cannot be regenerated losslessly",
            procedural.id
        )));
    }
    let write_cache_fit_tolerance = |bytes: &mut Vec<u8>| {
        if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance() {
            native_f64(bytes, cache_fit_tolerance.get() / LEN_TO_MM);
        }
    };
    if matches!(
        procedural.definition(),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact { .. }
    ) {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "exact_int_cur")?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Law {
        context,
        version,
        extension,
        primary,
        additional,
        ..
    } = procedural.definition()
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "law_int_cur")?;
        if let Some(version) = version {
            native_i64(bytes, version.stamp());
            native_enum(bytes, version.post_enum());
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        if let Some(version) = version {
            native_law_version_context(bytes, target, context, version.parameter_range())?;
        } else {
            native_intcurve_support_context(bytes, target, context)?;
        }
        native_i64(bytes, *extension);
        native_law_formula(bytes, target, &primary.formula().to_raw())?;
        native_i64(
            bytes,
            i64::try_from(additional.len())
                .map_err(|_| CodecError::NotImplemented("law formula count exceeds i64".into()))?,
        );
        for formula in additional {
            native_law_formula(bytes, target, &formula.formula().to_raw())?;
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable(definition_payload) =
        procedural.definition()
    {
        let context = definition_payload.context();
        let cache_first = &definition_payload.cache_first().to_raw();
        let source = definition_payload.source();
        let source_parameter_range = definition_payload.source_parameter_range();
        let data = &definition_payload.data().to_raw();
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "defm_int_cur")?;
        native_cache_first_curve_context(bytes, target, context, cache_first, Some(solved_cache))?;
        match source {
            cadmpeg_ir::geometry::DeformableCurveSource::Curve { curve } => {
                let source = target
                    .model
                    .curves
                    .iter()
                    .find(|candidate| candidate.id == *curve)
                    .ok_or_else(|| {
                        CodecError::Malformed("deformable source curve is missing".into())
                    })?;
                let source_range = [
                    source_parameter_range[0].map_or(
                        context.parameter_range().endpoints()[0],
                        cadmpeg_ir::scalar::FiniteReal::get,
                    ),
                    source_parameter_range[1].map_or(
                        context.parameter_range().endpoints()[1],
                        cadmpeg_ir::scalar::FiniteReal::get,
                    ),
                ];
                let source_curve = native_interval_curve(
                    source.geometry.solved().ok_or_else(|| {
                        CodecError::NotImplemented(
                            "source-less F3D carrier has no solved geometry".into(),
                        )
                    })?,
                    source_range,
                )?;
                native_nurbs_curve(bytes, &source_curve)?;
            }
            cadmpeg_ir::geometry::DeformableCurveSource::NativeReference { flag, index } => {
                native_ident(bytes, "intcurve")?;
                bytes.push(native_bool(*flag));
                bytes.push(0x0f);
                native_ident(bytes, "ref")?;
                native_i64(bytes, *index);
                bytes.push(0x10);
            }
        }
        for bound in source_parameter_range {
            native_optional_f64(bytes, bound.map(cadmpeg_ir::scalar::FiniteReal::get));
        }
        match data {
            cadmpeg_ir::geometry::DeformableCurveData::VectorField {
                vectors,
                parameter_pairs,
            } => {
                native_i64(bytes, 8);
                for vector in vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_i64(
                    bytes,
                    i64::try_from(parameter_pairs.len()).map_err(|_| {
                        CodecError::NotImplemented(
                            "deformable parameter-pair count exceeds i64".into(),
                        )
                    })?,
                );
                for pair in parameter_pairs {
                    native_f64(bytes, pair[0]);
                    native_f64(bytes, pair[1]);
                }
            }
            cadmpeg_ir::geometry::DeformableCurveData::Mode3 {
                leading_vectors,
                leading_parameter,
                leading_flags,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                frame_flags,
                parameters,
                trailing_flags,
                trailing_parameter,
                trailing_value,
            } => {
                native_i64(bytes, 3);
                for vector in leading_vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_f64(bytes, *leading_parameter);
                for flag in leading_flags {
                    bytes.push(native_bool(*flag));
                }
                native_point(
                    bytes,
                    [
                        trailing_point.x / LEN_TO_MM,
                        trailing_point.y / LEN_TO_MM,
                        trailing_point.z / LEN_TO_MM,
                    ],
                );
                for vector in trailing_vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_f64(bytes, *frame_parameter);
                for flag in frame_flags {
                    bytes.push(native_bool(*flag));
                }
                for parameter in parameters {
                    native_f64(bytes, *parameter);
                }
                for flag in trailing_flags {
                    bytes.push(native_bool(*flag));
                }
                native_f64(bytes, *trailing_parameter);
                native_i64(bytes, *trailing_value);
            }
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection(definition_payload) =
        procedural.definition()
    {
        let context = definition_payload.context();
        let discontinuity_flag = definition_payload.discontinuity_flag();
        let source = definition_payload.source();
        let tail = definition_payload.tail();

        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("projection source curve is missing".into()))?;
        let source = native_interval_curve(
            source.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("source-less F3D carrier has no solved geometry".into())
            })?,
            context.parameter_range().endpoints(),
        )?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "proj_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        native_nurbs_curve(bytes, &source)?;
        match tail {
            cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag } => {
                bytes.push(native_bool(*flag));
                bytes.push(0x10);
                native_nurbs_curve(bytes, solved_cache)?;
                write_cache_fit_tolerance(bytes);
            }
            cadmpeg_ir::geometry::ProjectionTail::Ranged {
                flag,
                parameter_range,
                role,
            } => {
                bytes.push(native_bool(*flag));
                for value in parameter_range {
                    native_f64(bytes, value.get());
                }
                native_string(bytes, role.as_str())?;
                native_nurbs_curve(bytes, solved_cache)?;
                write_cache_fit_tolerance(bytes);
                bytes.push(0x10);
            }
        }
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(compound) =
        procedural.definition()
    {
        let parameters = compound.parameters();
        let components = compound.components();

        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "comp_int_cur")?;
        native_i64(
            bytes,
            i64::try_from(parameters.len()).map_err(|_| {
                CodecError::NotImplemented("compound parameter count exceeds i64".into())
            })?,
        );
        for value in parameters {
            native_f64(bytes, value.get());
        }
        native_i64(
            bytes,
            i64::try_from(components.len()).map_err(|_| {
                CodecError::NotImplemented("compound component count exceeds i64".into())
            })?,
        );
        for item in components {
            native_f64(bytes, item.parameter.get());
        }
        bytes.push(0x0b);
        for (ordinal, component) in components.iter().enumerate() {
            let component = &component.component;
            let component = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *component)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "compound curve references missing component {component}"
                    ))
                })?;
            let parameter_range = if matches!(
                component.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
            ) {
                None
            } else {
                let range = parameters.get(ordinal..ordinal + 2).ok_or_else(|| {
                    CodecError::Malformed("compound component has no construction interval".into())
                })?;
                Some([range[0].get(), range[1].get()])
            };
            let component = native_spline_field_curve(
                component.geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?,
                parameter_range,
            )?;
            native_nurbs_curve(bytes, &component)?;
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
        ..
    } = procedural.definition()
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "int_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { family } =
        procedural.definition()
    {
        let (name, context, tail) = match family {
            cadmpeg_ir::geometry::SurfaceCurveFamily::Blend { context, tail } => (
                "blend_int_cur",
                context,
                tail.as_ref().map(|value| (&value.form, value.flags, None)),
            ),
            cadmpeg_ir::geometry::SurfaceCurveFamily::SurfaceConstrained { context, tail } => (
                "surf_int_cur",
                context,
                tail.as_ref().map(|value| (&value.form, value.flags, None)),
            ),
            cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric { context, tail } => (
                "par_int_cur",
                context,
                tail.as_ref()
                    .map(|value| (&value.form, value.flags.flag, value.flags.second_flag)),
            ),
            cadmpeg_ir::geometry::SurfaceCurveFamily::Skin { context, tail } => (
                "skin_int_cur",
                context,
                tail.as_ref().map(|value| (&value.form, value.flags, None)),
            ),
        };
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, name)?;
        if let Some((tail, flag, second_flag)) = tail {
            native_cache_first_curve_context(
                bytes,
                target,
                context,
                &cadmpeg_ir::geometry::CacheFirstCurveForm {
                    revision: tail.revision(),
                    cache: tail.cache().to_raw(),
                    support_bounds: tail
                        .support_bounds()
                        .map(cadmpeg_ir::geometry::RecordBounds::get),
                    solved_range: tail
                        .solved_range()
                        .map(|value| value.map(cadmpeg_ir::scalar::FiniteReal::get)),
                    extension: tail.extension(),
                },
                Some(solved_cache),
            )?;
            bytes.push(native_bool(flag));
            if let Some(second_flag) = second_flag {
                bytes.push(native_bool(second_flag));
            }
        } else {
            native_intcurve_support_context(bytes, target, context)?;
            native_nurbs_curve(bytes, solved_cache)?;
            write_cache_fit_tolerance(bytes);
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette(definition_payload) =
        procedural.definition()
    {
        let context = definition_payload.context();
        let cast_surface = definition_payload.cast_surface();
        let light_direction = definition_payload.light_direction();
        let (name, draft_factor) = match definition_payload.silhouette() {
            cadmpeg_ir::geometry::SilhouetteKind::Standard {} => ("silh_int_cur", None),
            cadmpeg_ir::geometry::SilhouetteKind::Parametric {} => ("para_silh_int_cur", None),
            cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => {
                ("taper_silh_int_cur", Some(draft_factor.get()))
            }
        };
        let cast_surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *cast_surface)
            .ok_or_else(|| CodecError::Malformed("silhouette cast surface is missing".into()))?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, name)?;
        native_intcurve_support_context(bytes, target, context)?;
        native_embedded_surface(bytes, &cast_surface.geometry)?;
        native_vector(
            bytes,
            [light_direction.x, light_direction.y, light_direction.z],
        );
        if let Some(draft_factor) = draft_factor {
            native_f64(bytes, draft_factor);
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset(definition_payload) =
        procedural.definition()
    {
        let context = definition_payload.context();
        let discontinuity_flag = definition_payload.discontinuity_flag();
        let base_u_range = definition_payload.base_u_range();
        let base_v_range = definition_payload.base_v_range();
        let base = definition_payload.base();
        let base_range = definition_payload.base_range();
        let base_endpoints = definition_payload.base_endpoints();
        let cache_first = definition_payload
            .cache_first()
            .map(cadmpeg_ir::geometry::CacheFirstCurveForm::to_raw);
        let distance = definition_payload.distance();
        let shift = definition_payload.shift();
        let scale = definition_payload.scale();
        let base = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *base)
            .ok_or_else(|| CodecError::Malformed("surface-offset base curve is missing".into()))?;
        let base = native_interval_curve(
            base.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("source-less F3D carrier has no solved geometry".into())
            })?,
            base_range.endpoints(),
        )?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "off_surf_int_cur")?;
        if let Some(form) = &cache_first {
            native_cache_first_curve_context(bytes, target, context, form, Some(solved_cache))?;
            for range in [base_u_range, base_v_range] {
                for value in range.endpoints() {
                    native_optional_f64(bytes, Some(value));
                }
            }
            native_nurbs_curve(bytes, &base)?;
            for value in base_endpoints {
                native_optional_f64(bytes, value.map(cadmpeg_ir::scalar::FiniteReal::get));
            }
            for value in base_range.endpoints() {
                native_optional_f64(bytes, Some(value));
            }
            native_f64(bytes, distance.get() / LEN_TO_MM);
            native_f64(bytes, shift.get());
            native_f64(bytes, scale.get());
        } else {
            native_intcurve_support_context(bytes, target, context)?;
            bytes.push(native_bool(*discontinuity_flag));
            for range in [base_u_range, base_v_range] {
                for value in range.endpoints() {
                    native_f64(bytes, value);
                }
            }
            native_nurbs_curve(bytes, &base)?;
            for value in base_range.endpoints() {
                native_f64(bytes, value);
            }
            native_f64(bytes, distance.get() / LEN_TO_MM);
            native_f64(bytes, shift.get());
            native_f64(bytes, scale.get());
            native_nurbs_curve(bytes, solved_cache)?;
            write_cache_fit_tolerance(bytes);
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring(definition_payload) =
        procedural.definition()
    {
        let layout = &definition_payload.layout().to_raw();
        let direction = definition_payload.direction();

        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "spring_int_cur")?;
        let (
            supports,
            first_pcurve,
            second_pcurve,
            parameter_range,
            discontinuities,
            discontinuity_flag,
        ) = match layout {
            cadmpeg_ir::geometry::SpringLayout::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                discontinuity_flag,
                ..
            } => (
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                discontinuity_flag,
            ),
            cadmpeg_ir::geometry::SpringLayout::CacheFirst { context, form } => {
                native_cache_first_curve_context(bytes, target, context, form, Some(solved_cache))?;
                native_enum(bytes, *direction);
                bytes.push(0x10);
                return Ok(true);
            }
        };
        for support in supports {
            match support {
                cadmpeg_ir::geometry::SpringSupport::Surface(surface_id) => {
                    let surface = target
                        .model
                        .surfaces
                        .iter()
                        .find(|surface| surface.id == *surface_id)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "spring references missing support {surface_id}"
                            ))
                        })?;
                    native_embedded_surface(bytes, &surface.geometry)?;
                }
                cadmpeg_ir::geometry::SpringSupport::Ranges(ranges) => {
                    native_ident(bytes, "null_surface")?;
                    for range in ranges {
                        for value in range {
                            native_f64(bytes, *value);
                        }
                    }
                }
            }
        }
        match first_pcurve.as_ref() {
            cadmpeg_ir::geometry::SpringPcurve::Pcurve(pcurve) => {
                native_spring_pcurve(bytes, target, &supports[0], pcurve)?;
            }
            cadmpeg_ir::geometry::SpringPcurve::Range(range) => {
                native_ident(bytes, "nullbs")?;
                for value in range {
                    native_f64(bytes, *value);
                }
            }
        }
        if let Some(pcurve) = second_pcurve {
            native_spring_pcurve(bytes, target, &supports[1], pcurve)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
        for value in parameter_range {
            native_f64(bytes, *value);
        }
        for discontinuities in discontinuities {
            native_i64(
                bytes,
                i64::try_from(discontinuities.len()).map_err(|_| {
                    CodecError::NotImplemented("discontinuity count exceeds i64".into())
                })?,
            );
            for value in discontinuities {
                native_f64(bytes, *value);
            }
        }
        bytes.push(native_bool(*discontinuity_flag));
        native_enum(bytes, *direction);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection(
        definition_payload,
    ) = procedural.definition()
    {
        let context = definition_payload.context();
        let selector = definition_payload.selector();
        let third = definition_payload.third();

        let surface_id = third.surface.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D sss_int_cur requires a third support surface".into(),
            )
        })?;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "three-surface intersection references missing support {surface_id}"
                ))
            })?;
        let pcurve = third.pcurve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D sss_int_cur requires a third support pcurve".into(),
            )
        })?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "sss_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        native_i64(bytes, *selector);
        native_embedded_surface(bytes, &surface.geometry)?;
        let pcurve = native_support_pcurve(&surface.geometry, &pcurve.geometry)?;
        native_nurbs_pcurve_payload(bytes, &pcurve)?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        procedural.definition()
    {
        let context = definition_payload.context();
        let discontinuity_flag = definition_payload.discontinuity_flag();
        let offsets = definition_payload.offsets();
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "off_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        for offset in offsets.get() {
            native_f64(bytes, offset / LEN_TO_MM);
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset(definition_payload) =
        procedural.definition()
    {
        let source = definition_payload.source();
        let parameter_range = definition_payload.parameter_range();
        let offset = definition_payload.offset();
        let roles = definition_payload.roles();
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("vector offset source curve is missing".into()))?;
        let source = native_interval_curve(
            source.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("source-less F3D carrier has no solved geometry".into())
            })?,
            parameter_range.endpoints(),
        )?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "offset_int_cur")?;
        bytes.push(0x0b);
        native_nurbs_curve(bytes, &source)?;
        native_f64(bytes, parameter_range.endpoints()[0]);
        native_f64(bytes, parameter_range.endpoints()[1]);
        native_vector(
            bytes,
            [
                offset.x / LEN_TO_MM,
                offset.y / LEN_TO_MM,
                offset.z / LEN_TO_MM,
            ],
        );
        native_string(bytes, "source")?;
        native_i64(bytes, roles.source);
        native_string(bytes, "offset")?;
        native_i64(bytes, roles.offset);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(definition_payload) =
        procedural.definition()
    {
        let source = definition_payload.source();
        let parameter_range = definition_payload.parameter_range();
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("subset source curve is missing".into()))?;
        let source = native_interval_curve(
            source.geometry.solved().ok_or_else(|| {
                CodecError::NotImplemented("source-less F3D carrier has no solved geometry".into())
            })?,
            parameter_range.endpoints(),
        )?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "subset_int_cur")?;
        native_nurbs_curve(bytes, &source)?;
        native_f64(bytes, parameter_range.endpoints()[0]);
        native_f64(bytes, parameter_range.endpoints()[1]);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    let (angle_range, center, major, minor, pitch, apex_factor, axis) =
        match procedural.definition() {
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix_payload) => (
                helix_payload.angle_range(),
                helix_payload.center().as_raw(),
                helix_payload.major(),
                helix_payload.minor(),
                helix_payload.pitch(),
                &helix_payload.apex_factor(),
                helix_payload.axis(),
            ),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::SpatialOffset(_) => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D offset curve {} lacks a defined native offset-law grammar",
                    procedural.id
                )))
            }
            cadmpeg_ir::geometry::ProceduralCurveDefinition::BlendSpine { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D blend-spine curve {} lacks its native blend construction",
                    procedural.id
                )))
            }
            cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Law { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::TolerantIntersection { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection(..)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring(..)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection(..)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(_)
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Replica { .. }
            | cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D procedural curve {} has no native helix grammar",
                    procedural.id
                )))
            }
        };
    native_curve_base(bytes, "intcurve")?;
    bytes.push(0x0f);
    native_ident(bytes, "helix_int_cur")?;
    for value in *angle_range {
        bytes.push(0x0a);
        native_f64(bytes, value);
    }
    native_point(
        bytes,
        [
            center.x / LEN_TO_MM,
            center.y / LEN_TO_MM,
            center.z / LEN_TO_MM,
        ],
    );
    for vector in [major, minor, pitch] {
        native_point(
            bytes,
            [
                vector.x / LEN_TO_MM,
                vector.y / LEN_TO_MM,
                vector.z / LEN_TO_MM,
            ],
        );
    }
    native_f64(bytes, apex_factor.get());
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_nurbs_curve(bytes, solved_cache)?;
    write_cache_fit_tolerance(bytes);
    bytes.push(0x10);
    Ok(true)
}

pub(super) fn native_cacheless_procedural_curve(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    curve_id: &cadmpeg_ir::ids::CurveId,
) -> Result<bool, CodecError> {
    let mut definitions =
        target.model.procedural_curves.iter().filter(|procedural| {
            target.model.procedural_curve_owner(&procedural.id) == Some(curve_id)
        });
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::malformed(format_args!(
            "curve {curve_id} has multiple procedural constructions"
        )));
    }
    if procedural.cache_fit_tolerance().is_some() {
        return Err(CodecError::malformed(format_args!(
            "cacheless procedural curve {} carries a cache-fit tolerance",
            procedural.id
        )));
    }
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix(helix_payload) =
        procedural.definition()
    else {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize cacheless procedural curve {}",
            procedural.id
        )));
    };
    let angle_range = helix_payload.angle_range();
    let center = helix_payload.center().as_raw();
    let major = helix_payload.major();
    let minor = helix_payload.minor();
    let pitch = helix_payload.pitch();
    let apex_factor = helix_payload.apex_factor();
    let axis = helix_payload.axis();

    native_curve_base(bytes, "intcurve")?;
    bytes.push(0x0f);
    native_ident(bytes, "helix_int_cur")?;
    for value in *angle_range {
        bytes.push(0x0a);
        native_f64(bytes, value);
    }
    native_point(
        bytes,
        [
            center.x / LEN_TO_MM,
            center.y / LEN_TO_MM,
            center.z / LEN_TO_MM,
        ],
    );
    for vector in [major, minor, pitch] {
        native_point(
            bytes,
            [
                vector.x / LEN_TO_MM,
                vector.y / LEN_TO_MM,
                vector.z / LEN_TO_MM,
            ],
        );
    }
    native_f64(bytes, apex_factor.get());
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    bytes.push(0x10);
    Ok(true)
}

fn native_embedded_surface(
    bytes: &mut Vec<u8>,
    geometry: &SurfaceGeometry,
) -> Result<(), CodecError> {
    let Some(geometry) = geometry.solved() else {
        return Err(CodecError::NotImplemented(
            "source-less F3D carrier has no solved geometry".into(),
        ));
    };
    match geometry {
        SolvedSurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin().get();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            native_ident(bytes, "plane")?;
            native_point(
                bytes,
                [
                    origin.x / LEN_TO_MM,
                    origin.y / LEN_TO_MM,
                    origin.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [normal.x, normal.y, normal.z]);
            native_vector(bytes, [u_axis.x, u_axis.y, u_axis.z]);
            bytes.push(0x0b);
        }
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            native_embedded_cone(bytes, origin, *axis, *ref_direction, radius, 1.0, 0.0)?;
        }
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            native_embedded_cone(
                bytes,
                origin,
                *axis,
                *ref_direction,
                radius,
                ratio,
                half_angle,
            )?;
        }
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            native_ident(bytes, "sphere")?;
            native_point(
                bytes,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_f64(bytes, radius / LEN_TO_MM);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            bytes.extend_from_slice(&[0x0b; 5]);
        }
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            native_ident(bytes, "torus")?;
            native_point(
                bytes,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            native_f64(bytes, major_radius / LEN_TO_MM);
            native_f64(bytes, minor_radius / LEN_TO_MM);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            bytes.extend_from_slice(&[0x0b; 5]);
        }
        SolvedSurfaceGeometry::Nurbs(surface) => {
            native_ident(bytes, "spline")?;
            native_nurbs_surface(bytes, surface)?;
        }
        SolvedSurfaceGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded procedural or unknown support surfaces are unsupported"
                    .into(),
            ));
        }
        SolvedSurfaceGeometry::Polygonal(_) => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded polygonal support surfaces are unsupported".into(),
            ));
        }
        SolvedSurfaceGeometry::Transformed(_) => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded transformed support surfaces are unsupported".into(),
            ));
        }
    }
    Ok(())
}

pub(in crate::writer) fn native_support_pcurve(
    geometry: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
) -> Result<PcurveNurbs, CodecError> {
    native_support_pcurve_for_range(geometry, pcurve, [0.0, 1.0])
}

fn native_support_pcurve_with_offset(
    support: &SurfaceGeometry,
    geometry: &PcurveGeometry,
    offset: [f64; 2],
) -> Result<PcurveNurbs, CodecError> {
    let mut native = native_support_pcurve(support, geometry)?;
    native.try_map_control_points(|_, point| {
        let point = point.get();
        cadmpeg_ir::units::FinitePoint2::new(Point2::new(point.u - offset[0], point.v - offset[1]))
            .ok_or_else(|| CodecError::malformed("pcurve UV offset subtraction overflows"))
    })?;
    Ok(native)
}

fn native_support_pcurve_for_range(
    geometry: &SurfaceGeometry,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
) -> Result<PcurveNurbs, CodecError> {
    let native = native_pcurve_geometry(pcurve, range)?;
    // The chart of the native support fields `native_embedded_surface` writes:
    // the neutral frame with a unit plane reference vector, forward senses,
    // and `u_scale` equal to the radius.
    let chart = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => Some(NativeChart::Plane {
            reverse_v: false,
            scale: 1.0,
        }),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            NativeChart::cone(1.0, cylinder_surface.radius().get() / LEN_TO_MM, false)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => NativeChart::cone(
            cone_surface.half_angle().get().cos(),
            cone_surface.radius().get() / LEN_TO_MM,
            false,
        ),
        SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Sphere(_) | SolvedSurfaceGeometry::Torus(_),
        ) => Some(NativeChart::LatitudeFirst { reverse_v: false }),
        _ => return Ok(native.into_owned()),
    };
    let chart = chart.ok_or_else(|| {
        CodecError::NotImplemented(
            "intcurve support has an invalid cone axial parameter scale".into(),
        )
    })?;
    let mut poles = native.pole_rows().to_raw();
    poles.edit_points(|point| {
        let frame = match geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface))
                if sphere_surface.radius().get() < 0.0 =>
            {
                Point2::new(point.u - std::f64::consts::PI, -point.v)
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface))
                if torus_surface.minor_radius().get() < 0.0 =>
            {
                Point2::new(point.u, point.v - std::f64::consts::PI)
            }
            _ => *point,
        };
        *point = chart.native_point(frame);
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

#[cfg(test)]
mod pcurve_chart_tests {
    use super::native_support_pcurve;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::{pcurve::PcurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::math::{Point3, Vector3};

    #[test]
    fn generated_spring_refuses_overflowing_pcurve_poles_before_writing() {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        use cadmpeg_ir::geometry::{
            pcurve::LinePcurve, ProceduralCurveDefinition, SpringLayout, SpringPcurve,
        };
        use std::io::Cursor;

        let decoded = crate::F3dCodec
            .decode(
                &mut Cursor::new(crate::test_support::zip_test::f3d_with_smbh(
                    &crate::test_support::smbh_curves_test::synthetic_geometry_with_null_support_spring_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("spring fixture decode");
        let (mut target, _, _) = decoded.into_parts();
        target.source = None;
        target.native = cadmpeg_ir::native::Native::default();

        for (origin, direction) in [(1.0, 2.0), (f64::MAX, f64::MAX)] {
            target.model.procedural_curves[0].edit_definition(|definition| {
                let ProceduralCurveDefinition::Spring(payload) = definition else {
                    panic!("fixture must retain its spring construction");
                };
                let mut layout = payload.layout().to_raw();
                let SpringLayout::ContextFirst { first_pcurve, .. } = &mut layout else {
                    panic!("fixture must retain its context-first spring layout");
                };
                **first_pcurve = SpringPcurve::Pcurve(PcurveGeometry::Line(
                    LinePcurve::try_new(Point2::new(origin, 0.0), Point2::new(direction, 0.0))
                        .expect("finite nonzero line parameters"),
                ));
                *payload = cadmpeg_ir::geometry::curve_payloads::SpringCurvePayload::try_new(
                    layout,
                    *payload.direction(),
                )
                .expect("line pcurve is an admitted spring input");
            });
            let mut output = vec![0x93, 0x2a];
            let result = crate::writer::generate::write_new(&target, &mut output);
            if origin == 1.0 {
                result.expect("finite pcurve poles write successfully");
                crate::F3dCodec
                    .decode(&mut Cursor::new(&output[2..]), &DecodeOptions::default())
                    .expect("finite pcurve output decodes");
            } else {
                let error = result.expect_err("pcurve evaluation overflows a pole coordinate");
                assert!(matches!(error, CodecError::NotImplemented(_)), "{error}");
                assert!(error.to_string().contains("control_points"), "{error}");
                assert_eq!(output, [0x93, 0x2a]);
            }
        }
    }

    /// The native pcurve chart of each support `native_embedded_surface`
    /// writes: plane lengths in centimetres, cone `(axial / (|cos| r),
    /// azimuth)` about the written axis, and sphere and torus latitude first.
    #[test]
    fn writer_native_support_pcurves_follow_the_native_charts() {
        use cadmpeg_ir::geometry::analytic::{
            ConeSurface, CylinderSurface, PlaneSurface, SphereSurface, TorusSurface,
        };
        let origin = Point3::new(0.0, 0.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        let neutral = [Point2::new(1.25, 15.0), Point2::new(2.5, -3.0)];
        let pcurve = PcurveGeometry::Nurbs {
            nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                neutral.to_vec(),
                None,
                false,
            )
            .expect("valid test pcurve"),
        };
        let solved = |geometry| SurfaceGeometry::Solved(geometry);
        let mut cases = vec![
            (
                solved(SolvedSurfaceGeometry::Plane(
                    PlaneSurface::try_new(origin, axis, reference).unwrap(),
                )),
                neutral.map(|p| Point2::new(p.u / 10.0, p.v / 10.0)),
            ),
            (
                solved(SolvedSurfaceGeometry::Cylinder(
                    CylinderSurface::try_new(origin, axis, reference, 12.0).unwrap(),
                )),
                neutral.map(|p| Point2::new(p.v / 12.0, p.u)),
            ),
            (
                solved(SolvedSurfaceGeometry::Sphere(
                    SphereSurface::try_new(origin, axis, reference, 12.0).unwrap(),
                )),
                neutral.map(|p| Point2::new(p.v, p.u)),
            ),
            (
                solved(SolvedSurfaceGeometry::Sphere(
                    SphereSurface::try_new(origin, axis, reference, -12.0).unwrap(),
                )),
                neutral.map(|p| Point2::new(-p.v, p.u - std::f64::consts::PI)),
            ),
            (
                solved(SolvedSurfaceGeometry::Torus(
                    TorusSurface::try_new(origin, axis, reference, 12.0, 3.0).unwrap(),
                )),
                neutral.map(|p| Point2::new(p.v, p.u)),
            ),
            (
                solved(SolvedSurfaceGeometry::Torus(
                    TorusSurface::try_new(origin, axis, reference, 12.0, -3.0).unwrap(),
                )),
                neutral.map(|p| Point2::new(p.v - std::f64::consts::PI, p.u)),
            ),
        ];
        for half_angle in [0.5_f64, -0.5] {
            cases.push((
                solved(SolvedSurfaceGeometry::Cone(
                    ConeSurface::try_new(origin, axis, reference, 12.0, 1.0, half_angle).unwrap(),
                )),
                neutral.map(|p| Point2::new(p.v / (12.0 * half_angle.cos()), p.u)),
            ));
        }
        for (support, expected) in cases {
            let nurbs = native_support_pcurve(&support, &pcurve).expect("native support chart");
            for (point, expected) in nurbs.control_points().into_iter().zip(expected) {
                let point = point.get();
                assert!(
                    (point.u - expected.u).abs() <= f64::EPSILON * 16.0
                        && (point.v - expected.v).abs() <= f64::EPSILON * 16.0,
                    "{support:?}: {point:?} != {expected:?}"
                );
            }
        }
    }
}

pub(in crate::writer) fn pcurve_support_geometry<'a>(
    model: &'a Model,
    pcurve_id: &PcurveId,
) -> Result<&'a SurfaceGeometry, CodecError> {
    let surface_id = pcurve_support_surface(model, pcurve_id)?;
    model
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)
        .map(|surface| &surface.geometry)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D pcurve {pcurve_id} references missing support {surface_id}"
            ))
        })
}

/// The surface of the faces that use a pcurve.
pub(in crate::writer) fn pcurve_support_surface<'a>(
    model: &'a Model,
    pcurve_id: &PcurveId,
) -> Result<&'a SurfaceId, CodecError> {
    fn surface_for_loop<'a>(
        model: &'a Model,
        loop_: &cadmpeg_ir::topology::Loop,
        pcurve_id: &PcurveId,
    ) -> Result<&'a SurfaceId, CodecError> {
        let face = model
            .faces
            .iter()
            .find(|face| face.id == loop_.face)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "pcurve {pcurve_id} is used by loop {} with missing face {}",
                    loop_.id, loop_.face
                ))
            })?;
        Ok(&face.surface)
    }

    fn record_surface<'a>(
        selected: &mut Option<&'a SurfaceId>,
        surface: &'a SurfaceId,
        pcurve_id: &PcurveId,
    ) -> Result<(), CodecError> {
        if selected.is_some_and(|selected| selected != surface) {
            return Err(CodecError::NotImplemented(format!(
                "F3D pcurve {pcurve_id} is shared by faces with different support surfaces"
            )));
        }
        selected.get_or_insert(surface);
        Ok(())
    }

    let mut surface_id = None;
    for coedge in &model.coedges {
        if coedge.pcurves.iter().any(|use_| use_.pcurve == *pcurve_id) {
            let loop_ = model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve {pcurve_id} is used by missing loop {}",
                        coedge.owner_loop
                    ))
                })?;
            let surface = surface_for_loop(model, loop_, pcurve_id)?;
            record_surface(&mut surface_id, surface, pcurve_id)?;
        }
    }
    for loop_ in &model.loops {
        if loop_.vertex_pcurves().any(|use_| use_.pcurve == *pcurve_id) {
            let surface = surface_for_loop(model, loop_, pcurve_id)?;
            record_surface(&mut surface_id, surface, pcurve_id)?;
        }
    }
    surface_id.ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3D pcurve {pcurve_id} is not used by a coedge or pole vertex"
        ))
    })
}

fn native_intcurve_support_context(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
) -> Result<(), CodecError> {
    if context
        .sides()
        .iter()
        .any(|side| side.pcurve_parameter_range().is_some())
    {
        return Err(CodecError::NotImplemented(
            "F3D intcurve writing does not encode independent support-pcurve parameter intervals"
                .into(),
        ));
    }
    for side in context.sides() {
        if let Some(surface_id) = &side.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "intcurve references missing support {surface_id}"
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
        } else {
            native_ident(bytes, "null_surface")?;
        }
    }
    for side in context.sides() {
        if let Some(pcurve) = &side.pcurve {
            let native = if let Some(surface_id) = &side.surface {
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "intcurve references missing support {surface_id}"
                        ))
                    })?;
                Cow::Owned(native_support_pcurve(&surface.geometry, &pcurve.geometry)?)
            } else {
                native_pcurve_geometry(&pcurve.geometry, [0.0, 1.0])?
            };
            native_nurbs_pcurve_payload(bytes, &native)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
    }
    for value in context.parameter_range().endpoints() {
        native_f64(bytes, value);
    }
    for discontinuities in context.discontinuities() {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, value.get());
        }
    }
    Ok(())
}

/// Emit the stamped `law_int_cur` support context: two support surfaces (or the
/// `null_surface` sentinel), two nullable pcurves, the version-form interval as
/// `0x0a`/`0x0b` optional bounds, and three discontinuity arrays.
fn native_law_version_context(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
    parameter_range: [Option<cadmpeg_ir::scalar::FiniteReal>; 2],
) -> Result<(), CodecError> {
    if context
        .sides()
        .iter()
        .any(|side| side.pcurve_parameter_range().is_some())
    {
        return Err(CodecError::NotImplemented(
            "F3D intcurve writing does not encode independent support-pcurve parameter intervals"
                .into(),
        ));
    }
    for side in context.sides() {
        if let Some(surface_id) = &side.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "intcurve references missing support {surface_id}"
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
        } else {
            native_ident(bytes, "null_surface")?;
        }
    }
    for side in context.sides() {
        if let Some(pcurve) = &side.pcurve {
            let native = if let Some(surface_id) = &side.surface {
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "law intcurve references missing support {surface_id}"
                        ))
                    })?;
                Cow::Owned(native_support_pcurve(&surface.geometry, &pcurve.geometry)?)
            } else {
                native_pcurve_geometry(&pcurve.geometry, [0.0, 1.0])?
            };
            native_nurbs_pcurve_payload(bytes, &native)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
    }
    for bound in parameter_range {
        native_optional_f64(bytes, bound.map(cadmpeg_ir::scalar::FiniteReal::get));
    }
    for discontinuities in context.discontinuities() {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, value.get());
        }
    }
    Ok(())
}

fn native_optional_f64(bytes: &mut Vec<u8>, value: Option<f64>) {
    match value {
        Some(value) => {
            bytes.push(0x0a);
            native_f64(bytes, value);
        }
        None => bytes.push(0x0b),
    }
}

/// Emit an embedded support surface followed by its four optional U/V bound
/// fields when the surface kind carries them.
fn native_embedded_surface_with_bounds(
    bytes: &mut Vec<u8>,
    geometry: &SurfaceGeometry,
    bounds: &[Option<f64>; 4],
) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
            native_embedded_cone_with_bounds(bytes, geometry, bounds)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
            native_embedded_cone_with_bounds(bytes, geometry, bounds)?;
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            native_ident(bytes, "sphere")?;
            native_point(
                bytes,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_f64(bytes, radius / LEN_TO_MM);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            bytes.push(0x0b);
            for bound in bounds {
                native_optional_f64(bytes, *bound);
            }
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            native_ident(bytes, "torus")?;
            native_point(
                bytes,
                [
                    center.x / LEN_TO_MM,
                    center.y / LEN_TO_MM,
                    center.z / LEN_TO_MM,
                ],
            );
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            native_f64(bytes, major_radius / LEN_TO_MM);
            native_f64(bytes, minor_radius / LEN_TO_MM);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            bytes.push(0x0b);
            for bound in bounds {
                native_optional_f64(bytes, *bound);
            }
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => {
            native_embedded_surface(bytes, geometry)?;
            for bound in bounds {
                native_optional_f64(bytes, *bound);
            }
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => {
            native_embedded_surface(bytes, geometry)?;
            for bound in bounds {
                native_optional_f64(bytes, *bound);
            }
        }
        SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Unknown { .. }
            | SolvedSurfaceGeometry::Polygonal(_)
            | SolvedSurfaceGeometry::Transformed(_),
        ) => {
            return Err(CodecError::Malformed(
                "support bounds require an embeddable analytic or spline support".into(),
            ));
        }
    }
    Ok(())
}

fn native_embedded_cone_with_bounds(
    bytes: &mut Vec<u8>,
    geometry: &SurfaceGeometry,
    bounds: &[Option<f64>; 4],
) -> Result<(), CodecError> {
    let (origin, axis, ref_direction, radius, ratio, half_angle) = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let ref_direction = cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            (origin, *axis, *ref_direction, radius, 1.0, 0.0)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let radius = cone_surface.radius().get();
            let ratio = cone_surface.ratio().get();
            let half_angle = cone_surface.half_angle().get();
            (origin, *axis, *ref_direction, radius, ratio, half_angle)
        }
        _ => {
            return Err(CodecError::Malformed(
                "cone bounds helper requires an analytic cone support".into(),
            ));
        }
    };
    native_ident(bytes, "cone")?;
    native_point(
        bytes,
        [
            origin.x / LEN_TO_MM,
            origin.y / LEN_TO_MM,
            origin.z / LEN_TO_MM,
        ],
    );
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_vector(
        bytes,
        [
            ref_direction.x * radius / LEN_TO_MM,
            ref_direction.y * radius / LEN_TO_MM,
            ref_direction.z * radius / LEN_TO_MM,
        ],
    );
    native_f64(bytes, ratio);
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    native_f64(bytes, half_angle.sin());
    native_f64(bytes, half_angle.cos());
    native_f64(bytes, radius / LEN_TO_MM);
    bytes.push(0x0b);
    for bound in bounds {
        native_optional_f64(bytes, *bound);
    }
    Ok(())
}

/// Emit the shared revision-gated surface tail: the approximation-cache form
/// enum and its payload, six discontinuity arrays, the tail boolean, and the
/// trailing booleans. Form `0` writes the solved cache and its fit tolerance;
/// form `2` writes the U and V parameter intervals followed by the U closure,
/// V closure, U singularity, and V singularity enums.
fn native_revision_surface_tail<F: Default>(
    bytes: &mut Vec<u8>,
    carrier: &str,
    form: &cadmpeg_ir::geometry::RevisionSurfaceForm<F>,
    solved_cache: Option<&cadmpeg_ir::geometry::nurbs::NurbsSurface>,
) -> Result<(), CodecError> {
    native_revision_tail_head(bytes, carrier, &form.cache, solved_cache)?;
    native_revision_tail_discontinuities(bytes, &form.discontinuities)?;
    bytes.push(native_bool(form.tail_flag));
    for flag in &form.trailing_flags {
        bytes.push(native_bool(*flag));
    }
    Ok(())
}

/// Emit the fit tolerance stored immediately after a solved cache. Every
/// carrier that stores a cache stores its tolerance with it, so an absent
/// tolerance is an inconsistent construction rather than a defaulted field.
fn native_solved_cache_fit_tolerance(
    bytes: &mut Vec<u8>,
    carrier: &str,
    cache_fit_tolerance: Option<f64>,
) -> Result<(), CodecError> {
    let cache_fit_tolerance = cache_fit_tolerance.ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{carrier} requires a native cache-fit tolerance"
        ))
    })?;
    native_f64(bytes, cache_fit_tolerance / LEN_TO_MM);
    Ok(())
}

/// Emit the form-selecting head of the shared revision-gated surface tail: the
/// enum and the payload it selects. Form `0` writes the solved cache and its
/// fit tolerance; form `2` writes the U parameter interval and the V parameter
/// interval followed by the U closure, V closure, U singularity, and V
/// singularity enums. No other value has a defined payload. Form `2` stores no
/// cache, so it needs no solved carrier.
fn native_revision_tail_head(
    bytes: &mut Vec<u8>,
    carrier: &str,
    cache: &cadmpeg_ir::geometry::RevisionCacheForm,
    solved_cache: Option<&cadmpeg_ir::geometry::nurbs::NurbsSurface>,
) -> Result<(), CodecError> {
    match cache {
        cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache { fit_tolerance } => {
            native_enum(bytes, 0);
            let solved_cache = solved_cache.ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{carrier} tail form `0` requires a solved cache"
                ))
            })?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, fit_tolerance.get() / LEN_TO_MM);
        }
        cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(parameterization) => {
            native_enum(bytes, 2);
            for bound in parameterization
                .u_interval
                .iter()
                .chain(&parameterization.v_interval)
            {
                native_optional_f64(bytes, *bound);
            }
            for value in [
                parameterization.u_closure,
                parameterization.v_closure,
                parameterization.u_singularity,
                parameterization.v_singularity,
            ] {
                native_enum(bytes, value);
            }
        }
    }
    Ok(())
}

fn native_variable_blend_revision_tail_head(
    bytes: &mut Vec<u8>,
    cache: &cadmpeg_ir::geometry::VariableBlendCache,
    solved_cache: Option<&cadmpeg_ir::geometry::nurbs::NurbsSurface>,
) -> Result<(), CodecError> {
    match cache {
        cadmpeg_ir::geometry::VariableBlendCache::Current { fit_tolerance, .. } => {
            native_enum(bytes, 0);
            let solved_cache = solved_cache.ok_or_else(|| {
                CodecError::Malformed("variable blend tail form `0` requires a solved cache".into())
            })?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, fit_tolerance.get() / LEN_TO_MM);
        }
        cadmpeg_ir::geometry::VariableBlendCache::Stale {} => {
            return Err(CodecError::Malformed(
                "variable blend requires a native cache-fit tolerance".into(),
            ));
        }
        cadmpeg_ir::geometry::VariableBlendCache::Parameterization {
            parameterization, ..
        } => {
            native_enum(bytes, 2);
            for bound in parameterization
                .u_interval
                .iter()
                .chain(&parameterization.v_interval)
            {
                native_optional_f64(bytes, *bound);
            }
            for value in [
                parameterization.u_closure,
                parameterization.v_closure,
                parameterization.u_singularity,
                parameterization.v_singularity,
            ] {
                native_enum(bytes, value);
            }
        }
    }
    Ok(())
}

/// Emit the six counted discontinuity arrays closing the shared revision-gated
/// surface tail.
fn native_revision_tail_discontinuities(
    bytes: &mut Vec<u8>,
    discontinuities: &[Vec<f64>; 6],
) -> Result<(), CodecError> {
    for values in discontinuities {
        native_i64(
            bytes,
            i64::try_from(values.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in values {
            native_f64(bytes, *value);
        }
    }
    Ok(())
}

/// Emit the shared cache-first intcurve context: revision, the cache-form enum
/// and the payload it selects, bounded supports, nullable pcurves, optional
/// solved-interval endpoints, discontinuity arrays, and the extension integer.
/// Form `0` writes the solved cache and its fit tolerance; form `2` writes the
/// curve interval and the closed-form enum in their place.
fn native_cache_first_curve_context(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
    form: &cadmpeg_ir::geometry::CacheFirstCurveForm,
    solved_cache: Option<&cadmpeg_ir::geometry::nurbs::NurbsCurve>,
) -> Result<(), CodecError> {
    native_i64(bytes, form.revision.get());
    match &form.cache {
        cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache { fit_tolerance } => {
            native_enum(bytes, 0);
            let solved_cache = solved_cache.ok_or_else(|| {
                CodecError::Malformed(
                    "cache-first intcurve context form `0` requires a solved cache".into(),
                )
            })?;
            native_nurbs_curve(bytes, solved_cache)?;
            native_f64(bytes, fit_tolerance.get() / LEN_TO_MM);
        }
        cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(parameterization) => {
            native_enum(bytes, 2);
            for bound in parameterization.interval {
                native_optional_f64(bytes, bound);
            }
            native_enum(bytes, parameterization.closed_form);
        }
    }
    for (side, bounds) in context.sides().iter().zip(&form.support_bounds) {
        if let Some(surface_id) = &side.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "cache-first intcurve references missing support {surface_id}"
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
            if matches!(
                surface.geometry,
                SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Nurbs(_) | SolvedSurfaceGeometry::Plane(_)
                )
            ) {
                for bound in bounds {
                    native_optional_f64(bytes, *bound);
                }
            } else if bounds.iter().any(Option::is_some) {
                return Err(CodecError::Malformed(
                    "cache-first support bounds require a spline or plane support".into(),
                ));
            }
        } else {
            native_ident(bytes, "null_surface")?;
            if bounds.iter().any(Option::is_some) {
                return Err(CodecError::Malformed(
                    "cache-first support bounds require a non-null support".into(),
                ));
            }
        }
    }
    for side in context.sides() {
        if let Some(pcurve) = &side.pcurve {
            let native = if let Some(surface_id) = &side.surface {
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "cache-first intcurve references missing support {surface_id}"
                        ))
                    })?;
                Cow::Owned(native_support_pcurve(&surface.geometry, &pcurve.geometry)?)
            } else {
                native_pcurve_geometry(&pcurve.geometry, [0.0, 1.0])?
            };
            native_nurbs_pcurve_payload(bytes, &native)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
    }
    for value in form.solved_range {
        native_optional_f64(bytes, value);
    }
    for discontinuities in context.discontinuities() {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, value.get());
        }
    }
    native_i64(bytes, form.extension);
    Ok(())
}

fn native_embedded_cone(
    bytes: &mut Vec<u8>,
    origin: cadmpeg_ir::math::Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    ratio: f64,
    half_angle: f64,
) -> Result<(), CodecError> {
    native_ident(bytes, "cone")?;
    native_point(
        bytes,
        [
            origin.x / LEN_TO_MM,
            origin.y / LEN_TO_MM,
            origin.z / LEN_TO_MM,
        ],
    );
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_vector(
        bytes,
        [
            ref_direction.x * radius / LEN_TO_MM,
            ref_direction.y * radius / LEN_TO_MM,
            ref_direction.z * radius / LEN_TO_MM,
        ],
    );
    native_f64(bytes, ratio);
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    native_f64(bytes, half_angle.sin());
    native_f64(bytes, half_angle.cos());
    native_f64(bytes, radius / LEN_TO_MM);
    bytes.extend_from_slice(&[0x0b; 5]);
    Ok(())
}

#[derive(Debug)]
pub(super) enum NativePcurveForm<'a> {
    Inline(Cow<'a, PcurveInlineForm>),
    Reference {
        uv_offset: [f64; 2],
        companion_ref: i64,
    },
}

pub(super) fn native_pcurve(
    bytes: &mut Vec<u8>,
    geometry: &PcurveGeometry,
    form: &NativePcurveForm<'_>,
    support: &SurfaceGeometry,
) -> Result<(), CodecError> {
    let inline = match form {
        NativePcurveForm::Reference {
            uv_offset,
            companion_ref,
        } => {
            native_ident(bytes, "pcurve")?;
            native_ref(bytes, -1);
            native_i64(bytes, -1);
            native_ref(bytes, -1);
            native_i64(bytes, 2);
            native_ref(bytes, *companion_ref);
            native_f64(bytes, uv_offset[0]);
            native_f64(bytes, uv_offset[1]);
            return Ok(());
        }
        NativePcurveForm::Inline(inline) => inline,
    };
    let uv_offset = inline.uv_offset();
    let native_geometry = native_support_pcurve_with_offset(support, geometry, uv_offset.get())?;
    native_ident(bytes, "pcurve")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    native_i64(bytes, 0);
    bytes.push(native_bool(inline.wrapper_reversed));
    bytes.push(0x0f);
    native_ident(bytes, "exp_par_cur")?;
    native_nurbs_pcurve_payload(bytes, &native_geometry)?;
    native_f64(bytes, inline.fit_tolerance().get());
    bytes.push(0x10);
    for flag in inline.native_tail_flags {
        bytes.push(native_bool(flag));
    }
    native_f64(bytes, uv_offset[0]);
    native_f64(bytes, uv_offset[1]);
    Ok(())
}

pub(super) fn native_ref_pcurve_companion(
    bytes: &mut Vec<u8>,
    geometry: &PcurveGeometry,
    uv_offset: [f64; 2],
    support: &SurfaceGeometry,
) -> Result<(), CodecError> {
    let native_geometry = native_support_pcurve_with_offset(support, geometry, uv_offset)?;
    let lifted = native_geometry
        .lift(|point| Point3::new(point.u * 10.0, point.v * 10.0, 0.0))
        .map_err(|error| CodecError::NotImplemented(error.to_string()))?;
    native_curve_base(bytes, "intcurve")?;
    native_nurbs_curve(bytes, &lifted)?;
    native_nurbs_pcurve_payload(bytes, &native_geometry)?;
    Ok(())
}

/// NURBS form the source-less writer states for `geometry` over `range`.
///
/// The recursion over a trimmed basis is bounded by the carrier: each pcurve
/// nesting constructor refuses a chain past
/// [`MAX_GEOMETRY_NESTING`](cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING).
fn native_pcurve_geometry(
    geometry: &PcurveGeometry,
    range: [f64; 2],
) -> Result<Cow<'_, PcurveNurbs>, CodecError> {
    match geometry {
        PcurveGeometry::Line(line_pcurve) => {
            let origin = line_pcurve.origin().as_raw();
            let direction = line_pcurve.direction().as_raw();
            if !range.iter().all(|value| value.is_finite()) || range[0] >= range[1] {
                return Err(CodecError::NotImplemented(
                    "source-less F3D line pcurve requires an ordered finite range".into(),
                ));
            }
            PcurveNurbs::from_lanes(
                1,
                vec![range[0], range[0], range[1], range[1]],
                vec![
                    cadmpeg_ir::math::Point2::new(
                        origin.u + range[0] * direction.u,
                        origin.v + range[0] * direction.v,
                    ),
                    cadmpeg_ir::math::Point2::new(
                        origin.u + range[1] * direction.u,
                        origin.v + range[1] * direction.v,
                    ),
                ],
                None,
                false,
            )
            .map(Cow::Owned)
            .map_err(|error| CodecError::NotImplemented(error.to_string()))
        }
        PcurveGeometry::Circle(_) => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::Ellipse(_) => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::Harmonic(_) => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::Hyperbolic(_) => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::PolarHarmonic(_) => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::PolarNurbs { .. } => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::SphericalGreatCircle(_) => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::Nurbs { nurbs } => Ok(Cow::Borrowed(nurbs)),
        PcurveGeometry::Trimmed(trimmed_pcurve) => {
            let parameter_range = trimmed_pcurve.parameter_range();
            let basis = trimmed_pcurve.basis();
            native_pcurve_geometry(basis, parameter_range.endpoints())
        }
        PcurveGeometry::Parabola(_) => Err(CodecError::NotImplemented(
            "F3D writing of this exact pcurve family is not implemented".into(),
        )),
        PcurveGeometry::Hyperbola(_) => Err(CodecError::NotImplemented(
            "F3D writing of this exact pcurve family is not implemented".into(),
        )),
        PcurveGeometry::Offset(_) => Err(CodecError::NotImplemented(
            "F3D writing of this exact pcurve family is not implemented".into(),
        )),
        PcurveGeometry::Transformed(_) => Err(CodecError::NotImplemented(
            "F3D writing of this exact pcurve family is not implemented".into(),
        )),
    }
}

fn native_nurbs_pcurve_block(
    bytes: &mut Vec<u8>,
    geometry: &PcurveGeometry,
) -> Result<(), CodecError> {
    let native = native_pcurve_geometry(geometry, [0.0, 1.0])?;
    native_nurbs_pcurve_payload(bytes, &native)
}

fn native_nurbs_pcurve_payload(bytes: &mut Vec<u8>, nurbs: &PcurveNurbs) -> Result<(), CodecError> {
    let poles = nurbs.pole_rows();
    native_ident(
        bytes,
        match poles {
            PcurveNurbsPoles::Polynomial { .. } => "nubs",
            PcurveNurbsPoles::Rational { .. } => "nurbs",
        },
    )?;
    native_i64(bytes, i64::from(nurbs.degree()));
    native_enum(bytes, if nurbs.periodic() { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(nurbs.knots())).map_err(|_| {
            CodecError::NotImplemented("F3D pcurve unique-knot count exceeds i64".into())
        })?,
    );
    native_nurbs_knots(bytes, nurbs.knots())?;
    match poles {
        PcurveNurbsPoles::Polynomial { points } => {
            for point in points {
                native_f64(bytes, point.u);
                native_f64(bytes, point.v);
            }
        }
        PcurveNurbsPoles::Rational { points } => {
            for pole in points {
                native_f64(bytes, pole.point.u);
                native_f64(bytes, pole.point.v);
                native_f64(bytes, pole.weight.get());
            }
        }
    }
    Ok(())
}

fn native_nurbs_knot_counts(bytes: &mut Vec<u8>, knots: [&[f64]; 2]) -> Result<(), CodecError> {
    for knots in knots {
        native_i64(
            bytes,
            i64::try_from(unique_knot_count(knots)).map_err(|_| {
                CodecError::NotImplemented("F3D unique-knot count exceeds i64".into())
            })?,
        );
    }
    Ok(())
}

fn native_nurbs_knots(bytes: &mut Vec<u8>, knots: &[f64]) -> Result<(), CodecError> {
    let mut runs = Vec::<(f64, usize)>::new();
    for knot in knots {
        if let Some((value, count)) = runs.last_mut() {
            if *value == *knot {
                *count += 1;
                continue;
            }
        }
        runs.push((*knot, 1));
    }
    let run_count = runs.len();
    for (index, (value, expanded)) in runs.into_iter().enumerate() {
        let endpoint_extra = usize::from(index == 0 || index + 1 == run_count);
        let stored = expanded
            .checked_sub(endpoint_extra)
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                CodecError::Malformed("F3D NURBS endpoint multiplicity is invalid".into())
            })?;
        native_f64(bytes, value);
        native_i64(
            bytes,
            i64::try_from(stored).map_err(|_| {
                CodecError::NotImplemented("F3D knot multiplicity exceeds i64".into())
            })?,
        );
    }
    Ok(())
}

#[cfg(test)]
mod revision_surface_tail_tests {
    use super::{native_revision_tail_discontinuities, native_revision_tail_head};
    use crate::writer::primitives::native_bool;

    /// The writer's form-`2` tail head and discontinuity block are what the
    /// decoder reads back: the U parameter interval, the V parameter interval,
    /// the U closure, V closure, U singularity, and V singularity enums, six
    /// counted arrays, and the closing boolean, consuming the exact span.
    #[test]
    fn parameterized_tail_head_round_trips_through_the_decoder() {
        let parameterization = cadmpeg_ir::geometry::RevisionSurfaceParameterization {
            u_interval: [Some(0.25), None],
            v_interval: [Some(-1.5), Some(3.5)],
            u_closure: 1,
            v_closure: 2,
            u_singularity: 3,
            v_singularity: 4,
        };
        let discontinuities = [
            vec![0.125],
            Vec::new(),
            vec![0.25, 0.375],
            vec![0.5],
            Vec::new(),
            vec![0.625, 0.75],
        ];
        let mut bytes = Vec::new();
        let cache =
            cadmpeg_ir::geometry::RevisionCacheForm::Parameterization(parameterization.clone());
        native_revision_tail_head(&mut bytes, "test carrier", &cache, None)
            .expect("parameterized tail head");
        native_revision_tail_discontinuities(&mut bytes, &discontinuities)
            .expect("tail discontinuities");
        bytes.push(native_bool(false));

        let toks = cadmpeg_asm::nurbs::toks::lex_test_span(
            &bytes,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .expect("valid single-record byte fixture");
        let mut cur = cadmpeg_asm::nurbs::toks::Cur::at(&toks, 0);
        let tail = cadmpeg_asm::nurbs::proc_surface::revision_surface_tail(&mut cur)
            .expect("decoded parameterized tail");
        assert_eq!(cur.pos(), toks.len());
        let cadmpeg_asm::nurbs::proc_surface::RevisionSurfaceCache::Parameterized(actual) =
            tail.cache
        else {
            panic!("parameterized tail cache")
        };
        assert_eq!(actual, parameterization);
        assert_eq!(tail.discontinuities, discontinuities);
        assert!(!tail.tail_flag);
    }

    /// Form `0` stores a solved cache. Without a carrier to store there the
    /// writer refuses rather than emitting a tail whose enum claims one.
    #[test]
    fn solved_tail_form_without_a_cache_is_refused() {
        let mut bytes = Vec::new();
        let cache = cadmpeg_ir::geometry::RevisionCacheForm::SolvedCache {
            fit_tolerance: cadmpeg_ir::geometry::FitTolerance::try_new(0.5).unwrap(),
        };
        assert!(native_revision_tail_head(&mut bytes, "test carrier", &cache, None).is_err());
    }
}

/// The pre-revision offset-surface extension gate in its positional native run.
fn legacy_extension_flag_run(flags: cadmpeg_ir::geometry::LegacyExtensionFlags) -> Vec<bool> {
    match flags {
        cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {} => Vec::new(),
        cadmpeg_ir::geometry::LegacyExtensionFlags::Disabled {} => vec![false],
        cadmpeg_ir::geometry::LegacyExtensionFlags::Enabled {
            secondary,
            tertiary,
        } => {
            let mut run = vec![true, secondary];
            run.extend(tertiary);
            run
        }
    }
}

#[cfg(test)]
mod null_law_token_tests {
    use super::{native_law_expression, native_law_formula, NULL_LAW_TOKEN};
    use cadmpeg_ir::document::CadIr;

    /// The native null law is stated by the variant. A `Named` law whose name
    /// is the native token would read back as `Null`, so the writer refuses
    /// it; the `Null` variant writes the token, and the reader reads it back
    /// as `Null`.
    #[test]
    fn the_writer_cannot_emit_a_named_law_that_reads_back_as_null() {
        let target = CadIr::empty();

        let mut bytes = Vec::new();
        native_law_formula(
            &mut bytes,
            &target,
            &cadmpeg_ir::geometry::LawFormula::Null {},
        )
        .expect("the null law writes its token");
        assert!(
            bytes
                .windows(NULL_LAW_TOKEN.len())
                .any(|window| window == NULL_LAW_TOKEN.as_bytes()),
            "the null law writes {NULL_LAW_TOKEN}"
        );

        let named = cadmpeg_ir::geometry::LawFormula::Named {
            name: cadmpeg_core::text::NonBlankString::new(NULL_LAW_TOKEN.to_string())
                .expect("the token is a non-blank string"),
            variables: Vec::new(),
        };
        let Err(error) = native_law_formula(&mut Vec::new(), &target, &named) else {
            panic!("a named null law has no native spelling");
        };
        let error = error.to_string();
        assert!(error.contains(NULL_LAW_TOKEN), "{error}");

        let text = cadmpeg_ir::geometry::LawExpression::Text {
            value: cadmpeg_core::text::NonBlankString::new(NULL_LAW_TOKEN.to_string())
                .expect("the token is a non-blank string"),
        };
        let Err(error) = native_law_expression(&mut Vec::new(), &target, &text, 0) else {
            panic!("a law expression text of the token has no native spelling");
        };
        let error = error.to_string();
        assert!(error.contains(NULL_LAW_TOKEN), "{error}");
    }
}

#[cfg(test)]
mod law_tests;

#[cfg(test)]
mod pcurve_nesting_tests;
