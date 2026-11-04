// SPDX-License-Identifier: Apache-2.0
//! SMBH body encoders and edge/vertex/point normalization for source-less
//! generation.

use cadmpeg_asm::brep::records::{EndpointSlot, EvaluatedToleranceSlot};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::native::F3dNative;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::EvaluationFailure;
use cadmpeg_ir::geometry::{
    pcurve::{PcurveGeometry, PcurveMetadata},
    CurveGeometry, SolvedCurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::{ShellId, VertexId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Sense;

use super::{
    attributes::{
        edge_persistent_attribute_ref, encode_source_less_attributes, owner_attribute_ref,
        sketch_link_attribute_ref, source_less_body_key, timestamp_attribute_ref, AttributeIndex,
        AttributeOwner, AttributeOwnerStarts, SurfaceOwnerStarts,
    },
    index::NativeGenerationIndex,
    native_bytes::{
        native_curve_base, native_f64, native_history_tail, native_i64, native_ident, native_point,
        native_record_index, native_ref, native_string, native_surface_base, native_transform,
        native_vector,
    },
    native_geometry::{
        native_cacheless_procedural_curve, native_cacheless_procedural_surface, native_nurbs_curve,
        native_nurbs_surface, native_pcurve, native_procedural_curve, native_procedural_surface,
        native_ref_pcurve_companion, native_smbh_header, pcurve_support_geometry, NativePcurveForm,
    },
    preconditions::{validate_source_less_body_kinds, validate_source_less_wire_ownership},
    records::{native_tolerant_coedge_extension, tolerant_coedge_range},
};
use crate::writer::primitives::{native_bool, normalized_face_sense_to_native};
use cadmpeg_asm::nurbs::reader::LEN_TO_MM;

pub(super) fn encode_smbh(
    target: &CadIr,
    native: &F3dNative,
    attributes: &AttributeIndex<'_>,
) -> Result<Vec<u8>, CodecError> {
    let topology = NativeGenerationIndex::new(native);
    if target.model.faces.is_empty() {
        if !target
            .model
            .shells
            .iter()
            .any(|shell| !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty())
        {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation requires owned face topology or a nonempty wire shell"
                    .into(),
            ));
        }
        return encode_wire_body_smbh(target, native, attributes, &topology);
    }
    encode_face_topology_smbh(target, native, attributes, &topology)
}

#[derive(Debug)]
struct PlannedPcurve<'a> {
    geometry: &'a PcurveGeometry,
    support: &'a SurfaceGeometry,
    form: NativePcurveForm<'a>,
}

#[derive(Debug)]
struct NativeRecordPlan<'a> {
    body: i64,
    region: i64,
    shell: i64,
    wire: i64,
    face: i64,
    loop_: i64,
    surface: i64,
    curve: i64,
    pcurve: i64,
    coedge: i64,
    wire_coedge: i64,
    edge: i64,
    vertex: i64,
    point: i64,
    transform: i64,
    attribute: i64,
    pcurves: Vec<PlannedPcurve<'a>>,
}

impl<'a> NativeRecordPlan<'a> {
    fn for_model(model: &'a cadmpeg_ir::document::Model) -> Result<Self, CodecError> {
        let body_start = 1;
        let region_start = native_record_index(body_start, model.bodies.len())?;
        let shell_start = native_record_index(region_start, model.regions.len())?;
        let wire_count = model.shells.iter().map(source_less_wire_count).sum();
        let wire_start = native_record_index(shell_start, model.shells.len())?;
        let face_start = native_record_index(wire_start, wire_count)?;
        let loop_start = native_record_index(face_start, model.faces.len())?;
        let surface_start = native_record_index(loop_start, model.loops.len())?;
        let curve_start = native_record_index(surface_start, model.surfaces.len())?;
        let pcurve_start = native_record_index(curve_start, model.curves.len())?;
        let ref_pcurve_start = native_record_index(pcurve_start, model.pcurves.len())?;
        let mut ref_pcurve_count = 0usize;
        let mut pcurves = Vec::new();
        for pcurve in &model.pcurves {
            let form = match &pcurve.metadata {
                PcurveMetadata::AsmInline { form } => NativePcurveForm::Inline(Cow::Borrowed(form)),
                PcurveMetadata::General { form }
                    if form.wrapper_reversed.is_some() || form.fit_tolerance().is_some() =>
                {
                    NativePcurveForm::Inline(Cow::Owned(
                        cadmpeg_ir::geometry::pcurve::PcurveInlineForm::new(
                            form.wrapper_reversed.unwrap_or(false),
                            [false; 4],
                            form.uv_offset
                                .unwrap_or(cadmpeg_ir::units::FinitePoint2::ZERO.into()),
                            form.fit_tolerance()
                                .unwrap_or(cadmpeg_ir::geometry::FitTolerance::ZERO),
                        ),
                    ))
                }
                PcurveMetadata::General { form } => {
                    let offset = form
                        .uv_offset
                        .map_or([0.0, 0.0], cadmpeg_ir::units::FiniteVector::get);
                    let companion_ref = native_record_index(ref_pcurve_start, ref_pcurve_count)?;
                    ref_pcurve_count += 1;
                    NativePcurveForm::Reference {
                        uv_offset: offset,
                        companion_ref,
                    }
                }
            };
            pcurves.push(PlannedPcurve {
                geometry: &pcurve.geometry,
                support: pcurve_support_geometry(model, &pcurve.id)?,
                form,
            });
        }
        let pcurve_record_count = model
            .pcurves
            .len()
            .checked_add(ref_pcurve_count)
            .ok_or_else(|| {
                CodecError::NotImplemented("pcurve record count overflows usize".into())
            })?;
        let coedge_start = native_record_index(pcurve_start, pcurve_record_count)?;
        let wire_coedge_start = native_record_index(coedge_start, model.coedges.len())?;
        let wire_edge_count = model
            .shells
            .iter()
            .map(|shell| shell.wire_edges().len())
            .sum::<usize>();
        let edge_start = native_record_index(wire_coedge_start, wire_edge_count)?;
        let vertex_start = native_record_index(edge_start, model.edges.len())?;
        let point_start = native_record_index(vertex_start, model.vertices.len())?;
        let transform_start = native_record_index(point_start, model.points.len())?;
        let transform_count = model
            .bodies
            .iter()
            .filter(|body| body.transform.is_some())
            .count();
        let attribute_start = native_record_index(transform_start, transform_count)?;
        Ok(Self {
            body: body_start,
            region: region_start,
            shell: shell_start,
            wire: wire_start,
            face: face_start,
            loop_: loop_start,
            surface: surface_start,
            curve: curve_start,
            pcurve: pcurve_start,
            coedge: coedge_start,
            wire_coedge: wire_coedge_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            transform: transform_start,
            attribute: attribute_start,
            pcurves,
        })
    }
}

fn source_less_wire_count(shell: &cadmpeg_ir::topology::Shell) -> usize {
    usize::from(!shell.wire_edges().is_empty()) + shell.free_vertices().len()
}

fn source_less_wire_record_for_shell(
    model: &cadmpeg_ir::document::Model,
    wire_start: i64,
    shell_ordinal: usize,
) -> Result<i64, CodecError> {
    let wire_ordinal = model.shells[..shell_ordinal]
        .iter()
        .map(source_less_wire_count)
        .sum();
    native_record_index(wire_start, wire_ordinal)
}

fn encode_source_less_wires(
    records: &mut Vec<u8>,
    target: &CadIr,
    topology: &NativeGenerationIndex<'_>,
    wire_start: i64,
    wire_coedge_start: i64,
    shell_start: i64,
    vertex_start: i64,
) -> Result<BTreeMap<VertexId, i64>, CodecError> {
    let model = &target.model;
    let vertex_ordinals = model
        .vertices
        .iter()
        .enumerate()
        .map(|(ordinal, vertex)| (&vertex.id, ordinal))
        .collect::<HashMap<_, _>>();
    let edge_vertex_ids = model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .collect::<BTreeSet<_>>();
    let mut free_vertex_owners = BTreeMap::new();
    let mut edge_base = 0usize;
    let mut wire_ordinal = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        let shell_wire_count = source_less_wire_count(shell);
        if !shell.wire_edges().is_empty() {
            native_ident(records, "wire")?;
            native_ref(records, -1);
            native_i64(records, -1);
            native_ref(records, -1);
            native_ref(
                records,
                if shell_wire_count > 1 {
                    native_record_index(wire_start, wire_ordinal + 1)?
                } else {
                    -1
                },
            );
            native_ref(records, native_record_index(wire_coedge_start, edge_base)?);
            native_ref(records, native_record_index(shell_start, shell_ordinal)?);
            native_ref(records, -1);
            records.push(native_wire_side(
                topology,
                &shell.id,
                shell.wire_edges(),
                None,
            )?);
            records.push(0x11);
            wire_ordinal += 1;
            edge_base += shell.wire_edges().len();
        }
        for (free_ordinal, vertex_id) in shell.free_vertices().iter().enumerate() {
            let vertex_ordinal = vertex_ordinals.get(vertex_id).copied().ok_or_else(|| {
                CodecError::InvalidInput(format!("wire references missing free vertex {vertex_id}"))
            })?;
            if edge_vertex_ids.contains(vertex_id) {
                return Err(CodecError::InvalidInput(format!(
                    "wire vertex {vertex_id} is both free and an edge endpoint"
                )));
            }
            let owner = native_record_index(wire_start, wire_ordinal)?;
            if free_vertex_owners
                .insert(vertex_id.clone(), owner)
                .is_some()
            {
                return Err(CodecError::InvalidInput(format!(
                    "free vertex {vertex_id} belongs to more than one wire"
                )));
            }
            native_ident(records, "wire")?;
            native_ref(records, -1);
            native_i64(records, -1);
            native_ref(records, -1);
            native_ref(
                records,
                if free_ordinal + 1 < shell.free_vertices().len() {
                    native_record_index(wire_start, wire_ordinal + 1)?
                } else {
                    -1
                },
            );
            native_ref(records, -1);
            native_ref(records, native_record_index(shell_start, shell_ordinal)?);
            native_ref(records, native_record_index(vertex_start, vertex_ordinal)?);
            records.push(native_wire_side(topology, &shell.id, &[], Some(vertex_id))?);
            records.push(0x11);
            wire_ordinal += 1;
        }
    }
    if edge_vertex_ids
        .iter()
        .any(|vertex| !vertex_ordinals.contains_key(vertex))
        || vertex_ordinals.keys().any(|vertex| {
            !edge_vertex_ids.contains(vertex) && !free_vertex_owners.contains_key(*vertex)
        })
    {
        return Err(CodecError::InvalidInput(
            "source-less F3D vertices must be edge endpoints or free wire vertices".into(),
        ));
    }
    Ok(free_vertex_owners)
}

fn encode_wire_body_smbh(
    target: &CadIr,
    native: &F3dNative,
    attributes: &AttributeIndex<'_>,
    topology: &NativeGenerationIndex<'_>,
) -> Result<Vec<u8>, CodecError> {
    let model = &target.model;
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || !model.faces.is_empty()
        || !model.loops.is_empty()
        || !model.coedges.is_empty()
        || !model.surfaces.is_empty()
        || !model.pcurves.is_empty()
        || model
            .shells
            .iter()
            .any(|shell| shell.wire_edges().is_empty() && shell.free_vertices().is_empty())
        || model
            .shells
            .iter()
            .flat_map(cadmpeg_ir::topology::Shell::wire_edges)
            .zip(&model.edges)
            .any(|(id, edge)| *id != edge.id)
        || model
            .shells
            .iter()
            .map(|shell| shell.wire_edges().len())
            .sum::<usize>()
            != model.edges.len()
        || model
            .bodies
            .iter()
            .any(|body| body.kind != cadmpeg_ir::topology::BodyKind::Wire)
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D wire generation requires face-less wire bodies with nonempty wire shells"
                .into(),
        ));
    }
    let ownership = validate_source_less_wire_ownership(target)?;
    let body_start = 1i64;
    let region_start = native_record_index(body_start, model.bodies.len())?;
    let shell_start = native_record_index(region_start, model.regions.len())?;
    let wire_start = native_record_index(shell_start, model.shells.len())?;
    let wire_count = model.shells.iter().map(source_less_wire_count).sum();
    let wire_coedge_start = native_record_index(wire_start, wire_count)?;
    let curve_start = native_record_index(wire_coedge_start, model.edges.len())?;
    let edge_start = native_record_index(curve_start, model.curves.len())?;
    let vertex_start = native_record_index(edge_start, model.edges.len())?;
    let point_start = native_record_index(vertex_start, model.vertices.len())?;
    let transform_start = native_record_index(point_start, model.points.len())?;
    let transform_count = model
        .bodies
        .iter()
        .filter(|body| body.transform.is_some())
        .count();
    let attribute_start = native_record_index(transform_start, transform_count)?;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);
    for ((ordinal, body), owned) in model.bodies.iter().enumerate().zip(ownership.bodies()) {
        let region_ordinal = owned.first_region;
        let shell_ordinal = owned.first_shell;
        let transform_ordinal = model.bodies[..ordinal]
            .iter()
            .filter(|candidate| candidate.transform.is_some())
            .count();
        native_ident(&mut records, "body")?;
        native_ref(
            &mut records,
            owner_attribute_ref(
                target,
                attributes,
                AttributeOwner::Body(body),
                ordinal,
                attribute_start,
            )?,
        );
        native_i64(
            &mut records,
            source_less_body_key(attributes, body, ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        native_ref(
            &mut records,
            source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?,
        );
        native_ref(
            &mut records,
            if body.transform.is_some() {
                native_record_index(transform_start, transform_ordinal)?
            } else {
                -1
            },
        );
        records.push(0x11);
    }
    for owned in ownership.regions() {
        let body_ordinal = owned.body;
        let next = owned
            .next_region
            .map(|position| native_record_index(region_start, position))
            .transpose()?
            .unwrap_or(-1);
        let shell_ordinal = owned.first_shell;
        native_ident(&mut records, "region")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, next);
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, native_record_index(body_start, body_ordinal)?);
        records.push(0x11);
    }
    for (ordinal, owned) in ownership.shells().iter().enumerate() {
        let region_ordinal = owned.region;
        let next = owned
            .next_shell
            .map(|position| native_record_index(shell_start, position))
            .transpose()?
            .unwrap_or(-1);
        native_ident(&mut records, "shell")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        for reference in [
            -1,
            next,
            -1,
            -1,
            source_less_wire_record_for_shell(model, wire_start, ordinal)?,
            native_record_index(region_start, region_ordinal)?,
        ] {
            native_ref(&mut records, reference);
        }
        records.push(0x11);
    }
    let free_vertex_owners = encode_source_less_wires(
        &mut records,
        target,
        topology,
        wire_start,
        wire_coedge_start,
        shell_start,
        vertex_start,
    )?;
    let mut edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        for ordinal in 0..shell.wire_edges().len() {
            let edge_ordinal = edge_base + ordinal;
            let next = edge_base + (ordinal + 1) % shell.wire_edges().len();
            let previous =
                edge_base + (ordinal + shell.wire_edges().len() - 1) % shell.wire_edges().len();
            native_ident(&mut records, "coedge")?;
            native_ref(&mut records, -1);
            native_i64(&mut records, -1);
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(wire_coedge_start, next)?);
            native_ref(
                &mut records,
                native_record_index(wire_coedge_start, previous)?,
            );
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(edge_start, edge_ordinal)?);
            records.push(0x0b);
            native_ref(
                &mut records,
                source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?,
            );
            native_i64(&mut records, 0);
            native_ref(&mut records, -1);
            records.push(0x11);
        }
        edge_base += shell.wire_edges().len();
    }
    encode_source_less_curves(&mut records, target)?;
    let mut wire_edge_owners = model
        .edges
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            native_record_index(wire_coedge_start, ordinal).map(|owner| (edge.id.clone(), owner))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    apply_native_edge_owners(target, topology, wire_coedge_start, &mut wire_edge_owners)?;
    encode_source_less_edges_vertices_points(
        &mut records,
        target,
        attributes,
        topology,
        SourceLessRecordStarts {
            curve: curve_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            attribute: attribute_start,
        },
        &wire_edge_owners,
        &free_vertex_owners,
    )?;
    for body in &model.bodies {
        if let Some(transform) = body.transform {
            native_transform(&mut records, topology, body, transform)?;
            records.push(0x11);
        }
    }
    encode_source_less_attributes(
        &mut records,
        target,
        attributes,
        attribute_start,
        AttributeOwnerStarts {
            body: body_start,
            surface: None,
            edge: edge_start,
            vertex: vertex_start,
        },
    )?;
    native_history_tail(&mut records, native)?;
    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

fn encode_source_less_curves(records: &mut Vec<u8>, target: &CadIr) -> Result<(), CodecError> {
    let model = &target.model;
    for carrier in &model.curves {
        match carrier.geometry.solved() {
            Some(SolvedCurveGeometry::Line(line_curve)) => {
                let origin = line_curve.origin().get();
                let direction = *line_curve.direction().as_raw();
                native_curve_base(records, "straight")?;
                native_point(
                    records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(records, [direction.x, direction.y, direction.z]);
            }
            Some(SolvedCurveGeometry::Circle(circle_curve)) => {
                let center = circle_curve.center().get();
                let axis = *circle_curve.frame().axis().as_raw();
                let ref_direction = *circle_curve.frame().reference().as_raw();
                let radius = circle_curve.radius().get();
                native_curve_base(records, "ellipse")?;
                native_point(
                    records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(records, [axis.x, axis.y, axis.z]);
                native_vector(
                    records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(records, 1.0);
            }
            Some(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                let center = ellipse_curve.center().get();
                let axis = *ellipse_curve.frame().axis().as_raw();
                let major_direction = *ellipse_curve.frame().reference().as_raw();
                let major_radius = ellipse_curve.major_radius().get();
                let minor_radius = ellipse_curve.minor_radius().get();
                native_curve_base(records, "ellipse")?;
                native_point(
                    records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(records, [axis.x, axis.y, axis.z]);
                native_vector(
                    records,
                    [
                        major_direction.x * major_radius / LEN_TO_MM,
                        major_direction.y * major_radius / LEN_TO_MM,
                        major_direction.z * major_radius / LEN_TO_MM,
                    ],
                );
                native_f64(records, minor_radius / major_radius);
            }
            Some(SolvedCurveGeometry::Nurbs(ref curve)) => {
                if !native_procedural_curve(records, target, &carrier.id, curve)? {
                    native_curve_base(records, "intcurve")?;
                    native_nurbs_curve(records, curve)?;
                }
            }
            None => {
                if !native_cacheless_procedural_curve(records, target, &carrier.id)? {
                    return Err(CodecError::malformed(format_args!(
                        "procedural curve carrier {} has no construction",
                        carrier.id
                    )));
                }
            }
            Some(SolvedCurveGeometry::Degenerate(degenerate_curve)) => {
                let point = degenerate_curve.point().get();
                native_curve_base(records, "degenerate_curve")?;
                native_point(
                    records,
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ],
                );
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less F3D does not support this curve carrier".into(),
                ))
            }
        }
        records.push(0x11);
    }
    Ok(())
}

fn encode_face_topology_smbh(
    target: &CadIr,
    native: &F3dNative,
    attributes: &AttributeIndex<'_>,
    topology: &NativeGenerationIndex<'_>,
) -> Result<Vec<u8>, CodecError> {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

    let model = &target.model;
    let region_ordinals: HashMap<_, _> = model
        .regions
        .iter()
        .enumerate()
        .map(|(ordinal, region)| (&region.id, ordinal))
        .collect();
    let shell_ordinals: HashMap<_, _> = model
        .shells
        .iter()
        .enumerate()
        .map(|(ordinal, shell)| (&shell.id, ordinal))
        .collect();
    let coedge_ordinals: HashMap<_, _> = model
        .coedges
        .iter()
        .enumerate()
        .map(|(ordinal, coedge)| (&coedge.id, ordinal))
        .collect();
    let edge_ordinals: HashMap<_, _> = model
        .edges
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| (&edge.id, ordinal))
        .collect();
    let loop_ordinals: HashMap<_, _> = model
        .loops
        .iter()
        .enumerate()
        .map(|(ordinal, lp)| (&lp.id, ordinal))
        .collect();
    let pcurve_ordinals: HashMap<_, _> = model
        .pcurves
        .iter()
        .enumerate()
        .map(|(ordinal, pcurve)| (&pcurve.id, ordinal))
        .collect();
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || model.faces.is_empty()
        || model.loops.len() < model.faces.len()
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation requires owned face topology".into(),
        ));
    }
    validate_source_less_body_kinds(model)?;

    let plan = NativeRecordPlan::for_model(model)?;
    let NativeRecordPlan {
        body: body_start,
        region: region_start,
        shell: shell_start,
        wire: wire_start,
        face: face_start,
        loop_: loop_start,
        surface: surface_start,
        curve: curve_start,
        pcurve: pcurve_start,
        coedge: coedge_start,
        wire_coedge: wire_coedge_start,
        edge: edge_start,
        vertex: vertex_start,
        point: point_start,
        transform: transform_start,
        attribute: attribute_start,
        pcurves,
    } = plan;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);
    for (body_ordinal, body) in model.bodies.iter().enumerate() {
        let first_region = body
            .regions
            .first()
            .ok_or_else(|| CodecError::malformed(format_args!("body {} has no region", body.id)))?;
        let region_ordinal = region_ordinals.get(first_region).copied().ok_or_else(|| {
            CodecError::malformed(format_args!(
                "body references missing region {first_region}"
            ))
        })?;
        let transform_ordinal = model.bodies[..body_ordinal]
            .iter()
            .filter(|candidate| candidate.transform.is_some())
            .count();
        let first_wire = body
            .regions
            .iter()
            .filter_map(|region_id| {
                region_ordinals
                    .get(region_id)
                    .map(|ordinal| &model.regions[*ordinal])
            })
            .flat_map(|region| &region.shells)
            .find_map(|shell_id| {
                shell_ordinals
                    .get(shell_id)
                    .copied()
                    .filter(|ordinal| source_less_wire_count(&model.shells[*ordinal]) != 0)
            })
            .map(|shell_ordinal| {
                source_less_wire_record_for_shell(model, wire_start, shell_ordinal)
            })
            .transpose()?
            .unwrap_or(-1);
        native_ident(&mut records, "body")?;
        native_ref(
            &mut records,
            owner_attribute_ref(
                target,
                attributes,
                AttributeOwner::Body(body),
                body_ordinal,
                attribute_start,
            )?,
        );
        native_i64(
            &mut records,
            source_less_body_key(attributes, body, body_ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        native_ref(&mut records, first_wire);
        native_ref(
            &mut records,
            if body.transform.is_some() {
                native_record_index(transform_start, transform_ordinal)?
            } else {
                -1
            },
        );
        records.push(0x11);
    }
    for region in &model.regions {
        let body_ordinal = model
            .bodies
            .iter()
            .position(|body| body.id == region.body)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("region {} has no body", region.id))
            })?;
        let body = &model.bodies[body_ordinal];
        let ordinal = body
            .regions
            .iter()
            .position(|id| *id == region.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("body does not own region {}", region.id))
            })?;
        let first_shell = region.shells.first().ok_or_else(|| {
            CodecError::malformed(format_args!("region {} has no shell", region.id))
        })?;
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "region references missing shell {first_shell}"
                ))
            })?;
        let next = if ordinal + 1 == body.regions.len() {
            -1
        } else {
            let id = &body.regions[ordinal + 1];
            let position = model
                .regions
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("body references missing region {id}"))
                })?;
            native_record_index(region_start, position)?
        };
        native_ident(&mut records, "region")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, next);
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, native_record_index(body_start, body_ordinal)?);
        records.push(0x11);
    }
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == shell.region)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("shell {} has no region", shell.id))
            })?;
        let region = &model.regions[region_ordinal];
        let ordinal = region
            .shells
            .iter()
            .position(|id| *id == shell.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("region does not own shell {}", shell.id))
            })?;
        let first_face = shell
            .faces()
            .first()
            .map(|first_face| {
                model
                    .faces
                    .iter()
                    .position(|face| face.id == *first_face)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "shell references missing face {first_face}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(face_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let next = if ordinal + 1 == region.shells.len() {
            -1
        } else {
            let id = &region.shells[ordinal + 1];
            let position = model
                .shells
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("region references missing shell {id}"))
                })?;
            native_record_index(shell_start, position)?
        };
        native_ident(&mut records, "shell")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, next);
        native_ref(&mut records, -1);
        native_ref(&mut records, first_face);
        native_ref(
            &mut records,
            if source_less_wire_count(shell) == 0 {
                -1
            } else {
                source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?
            },
        );
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        records.push(0x11);
    }

    let free_vertex_owners = encode_source_less_wires(
        &mut records,
        target,
        topology,
        wire_start,
        wire_coedge_start,
        shell_start,
        vertex_start,
    )?;

    for (face_ordinal_global, face) in model.faces.iter().enumerate() {
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == face.shell)
            .ok_or_else(|| CodecError::malformed(format_args!("face {} has no shell", face.id)))?;
        let shell = &model.shells[shell_ordinal];
        let ordinal = shell
            .faces()
            .iter()
            .position(|id| *id == face.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("shell does not own face {}", face.id))
            })?;
        let first_loop = face.loops.iter().next().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D face generation requires every face to own a loop".into(),
            )
        })?;
        let loop_position = model
            .loops
            .iter()
            .position(|loop_| &loop_.id == first_loop)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("face references missing loop {first_loop}"))
            })?;
        let surface_position = model
            .surfaces
            .iter()
            .position(|surface| surface.id == face.surface)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "face references missing surface {}",
                    face.surface
                ))
            })?;
        native_ident(&mut records, "face")?;
        native_ref(
            &mut records,
            owner_attribute_ref(
                target,
                attributes,
                AttributeOwner::Face(face),
                face_ordinal_global,
                attribute_start,
            )?,
        );
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            if ordinal + 1 == shell.faces().len() {
                -1
            } else {
                let id = &shell.faces()[ordinal + 1];
                let position = model
                    .faces
                    .iter()
                    .position(|item| item.id == *id)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!("shell references missing face {id}"))
                    })?;
                native_record_index(face_start, position)?
            },
        );
        native_ref(
            &mut records,
            native_record_index(loop_start, loop_position)?,
        );
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(surface_start, surface_position)?,
        );
        records.push(native_bool(
            native_face_sense(model, topology, face) == Sense::Reversed,
        ));
        native_face_sidedness(&mut records, topology, face);
        records.push(0x11);
    }

    for loop_ in &model.loops {
        let face_position = model
            .faces
            .iter()
            .position(|face| face.id == loop_.face)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("loop references missing face {}", loop_.face))
            })?;
        let first = loop_.coedges().first().ok_or_else(|| {
            CodecError::malformed(format_args!("loop {} has no coedges", loop_.id))
        })?;
        let coedge_position = model
            .coedges
            .iter()
            .position(|coedge| coedge.id == *first)
            .ok_or_else(|| {
                CodecError::malformed(format_args!("loop references missing coedge {first}"))
            })?;
        native_ident(&mut records, "loop")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        let face = &model.faces[face_position];
        let ordinal = face
            .loops
            .iter()
            .position(|id| *id == loop_.id)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "face {} does not own loop {}",
                    face.id, loop_.id
                ))
            })?;
        let next_loop = if let Some(next_id) = face.loops.iter().nth(ordinal + 1) {
            let position = model
                .loops
                .iter()
                .position(|candidate| candidate.id == *next_id)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("face references missing loop {next_id}"))
                })?;
            native_record_index(loop_start, position)?
        } else {
            -1
        };
        native_ref(&mut records, next_loop);
        native_ref(
            &mut records,
            native_record_index(coedge_start, coedge_position)?,
        );
        native_ref(
            &mut records,
            native_record_index(face_start, face_position)?,
        );
        records.push(0x11);
    }

    for surface in &model.surfaces {
        match surface.geometry.solved() {
            Some(SolvedSurfaceGeometry::Plane(plane_surface)) => {
                let origin = plane_surface.origin().get();
                let normal = *plane_surface.frame().axis().as_raw();
                let u_axis = *plane_surface.frame().reference().as_raw();
                native_surface_base(&mut records, "plane")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [normal.x, normal.y, normal.z]);
                native_vector(&mut records, [u_axis.x, u_axis.y, u_axis.z]);
                records.push(0x0b);
            }
            Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                let origin = cylinder_surface.origin().get();
                let axis = *cylinder_surface.frame().axis().as_raw();
                let ref_direction = *cylinder_surface.frame().reference().as_raw();
                let radius = cylinder_surface.radius().get();
                native_surface_base(&mut records, "cone")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, 1.0);
                records.extend_from_slice(&[0x0b, 0x0b]);
                native_f64(&mut records, 0.0);
                native_f64(&mut records, 1.0);
                native_f64(&mut records, radius / LEN_TO_MM);
                records.extend_from_slice(&[0x0b; 5]);
            }
            Some(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                let origin = cone_surface.origin().get();
                let axis = *cone_surface.frame().axis().as_raw();
                let ref_direction = *cone_surface.frame().reference().as_raw();
                let radius = cone_surface.radius().get();
                let ratio = cone_surface.ratio().get();
                let half_angle = cone_surface.half_angle().get();
                native_surface_base(&mut records, "cone")?;
                native_point(
                    &mut records,
                    [
                        origin.x / LEN_TO_MM,
                        origin.y / LEN_TO_MM,
                        origin.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / LEN_TO_MM,
                        ref_direction.y * radius / LEN_TO_MM,
                        ref_direction.z * radius / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, ratio);
                records.extend_from_slice(&[0x0b, 0x0b]);
                native_f64(&mut records, half_angle.sin());
                native_f64(&mut records, half_angle.cos());
                native_f64(&mut records, radius / LEN_TO_MM);
                records.extend_from_slice(&[0x0b; 5]);
            }
            Some(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                let center = sphere_surface.center().get();
                let axis = *sphere_surface.frame().axis().as_raw();
                let ref_direction = *sphere_surface.frame().reference().as_raw();
                let radius = sphere_surface.radius().get();
                native_surface_base(&mut records, "sphere")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_f64(&mut records, radius / LEN_TO_MM);
                native_vector(
                    &mut records,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                records.extend_from_slice(&[0x0b; 5]);
            }
            Some(SolvedSurfaceGeometry::Nurbs(ref nurbs)) => {
                if !native_procedural_surface(&mut records, target, surface, nurbs)? {
                    native_surface_base(&mut records, "spline")?;
                    native_nurbs_surface(&mut records, nurbs)?;
                }
            }
            Some(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                let center = torus_surface.center().get();
                let axis = *torus_surface.frame().axis().as_raw();
                let ref_direction = *torus_surface.frame().reference().as_raw();
                let major_radius = torus_surface.major_radius().get();
                let minor_radius = torus_surface.minor_radius().get();
                native_surface_base(&mut records, "torus")?;
                native_point(
                    &mut records,
                    [
                        center.x / LEN_TO_MM,
                        center.y / LEN_TO_MM,
                        center.z / LEN_TO_MM,
                    ],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_f64(&mut records, major_radius / LEN_TO_MM);
                native_f64(&mut records, minor_radius / LEN_TO_MM);
                native_vector(
                    &mut records,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                records.extend_from_slice(&[0x0b; 5]);
            }
            Some(SolvedSurfaceGeometry::Polygonal(_)) => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D face generation does not support polygonal surface carrier {}",
                    surface.id
                )));
            }
            Some(SolvedSurfaceGeometry::Transformed(_)) => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D face generation does not support transformed surface carrier {}",
                    surface.id
                )));
            }
            None | Some(SolvedSurfaceGeometry::Unknown { .. }) => {
                if !native_cacheless_procedural_surface(&mut records, target, surface)? {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D face generation does not support surface carrier {}",
                        surface.id
                    )));
                }
            }
        }
        records.push(0x11);
    }

    encode_source_less_curves(&mut records, target)?;

    for pcurve in &pcurves {
        native_pcurve(&mut records, pcurve.geometry, &pcurve.form, pcurve.support)?;
        records.push(0x11);
    }
    for pcurve in &pcurves {
        if let NativePcurveForm::Reference { uv_offset, .. } = pcurve.form {
            native_ref_pcurve_companion(&mut records, pcurve.geometry, uv_offset, pcurve.support)?;
            records.push(0x11);
        }
    }

    for (coedge_ordinal, coedge) in model.coedges.iter().enumerate() {
        if coedge.pcurves.len() > 1 {
            return Err(CodecError::NotImplemented(format!(
                "coedge {} has an ordered pcurve collection",
                coedge.id
            )));
        }
        let Some((next_id, previous_id)) =
            cadmpeg_ir::topology::coedge_ring_neighbors(&model.loops, coedge)
        else {
            return Err(CodecError::malformed(format_args!(
                "coedge {} is absent from its owning loop ring",
                coedge.id
            )));
        };
        let next = coedge_ordinals.get(&next_id).copied();
        let previous = coedge_ordinals.get(&previous_id).copied();
        let radial = coedge_ordinals.get(&coedge.radial_next).copied();
        let edge = edge_ordinals.get(&coedge.edge).copied();
        let owner = loop_ordinals.get(&coedge.owner_loop).copied();
        let (Some(next), Some(previous), Some(radial), Some(edge), Some(owner)) =
            (next, previous, radial, edge, owner)
        else {
            return Err(CodecError::malformed(format_args!(
                "coedge {} has an unresolved topology reference",
                coedge.id
            )));
        };
        let tolerant_range = tolerant_coedge_range(topology, &coedge.id);
        native_ident(
            &mut records,
            if tolerant_range.is_some() {
                "tcoedge"
            } else {
                "coedge"
            },
        )?;
        native_ref(
            &mut records,
            sketch_link_attribute_ref(target, attributes, coedge, coedge_ordinal, attribute_start)?,
        );
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(coedge_start, next)?);
        native_ref(&mut records, native_record_index(coedge_start, previous)?);
        native_ref(
            &mut records,
            if radial == coedge_ordinal {
                -1
            } else {
                native_record_index(coedge_start, radial)?
            },
        );
        native_ref(&mut records, native_record_index(edge_start, edge)?);
        records.push(native_bool(coedge.sense == Sense::Reversed));
        native_ref(&mut records, native_record_index(loop_start, owner)?);
        native_i64(&mut records, 0);
        let pcurve_ref = coedge
            .pcurves
            .first()
            .map(|use_| {
                let pcurve_id = &use_.pcurve;
                pcurve_ordinals
                    .get(pcurve_id)
                    .copied()
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "coedge references missing pcurve {pcurve_id}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(pcurve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        native_ref(&mut records, pcurve_ref);
        if let Some(range) = tolerant_range {
            native_f64(&mut records, range[0]);
            native_f64(&mut records, range[1]);
            native_tolerant_coedge_extension(&mut records, target, topology, &coedge.id)?;
        }
        records.push(0x11);
    }

    let mut wire_edge_owners = BTreeMap::new();
    let mut wire_edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        if shell.wire_edges().is_empty() {
            continue;
        }
        let wire_ref = source_less_wire_record_for_shell(model, wire_start, shell_ordinal)?;
        for (ordinal, edge_id) in shell.wire_edges().iter().enumerate() {
            let edge_ordinal = edge_ordinals.get(edge_id).copied().ok_or_else(|| {
                CodecError::malformed(format_args!("wire references missing edge {edge_id}"))
            })?;
            let coedge_ordinal = wire_edge_base + ordinal;
            let owner = native_record_index(wire_coedge_start, coedge_ordinal)?;
            if wire_edge_owners.insert(edge_id.clone(), owner).is_some() {
                return Err(CodecError::malformed(format_args!(
                    "wire edge {edge_id} belongs to more than one shell"
                )));
            }
            let next = wire_edge_base + (ordinal + 1) % shell.wire_edges().len();
            let previous = wire_edge_base
                + (ordinal + shell.wire_edges().len() - 1) % shell.wire_edges().len();
            native_ident(&mut records, "coedge")?;
            native_ref(&mut records, -1);
            native_i64(&mut records, -1);
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(wire_coedge_start, next)?);
            native_ref(
                &mut records,
                native_record_index(wire_coedge_start, previous)?,
            );
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(edge_start, edge_ordinal)?);
            records.push(0x0b);
            native_ref(&mut records, wire_ref);
            native_i64(&mut records, 0);
            native_ref(&mut records, -1);
            records.push(0x11);
        }
        wire_edge_base += shell.wire_edges().len();
    }
    apply_native_edge_owners(target, topology, coedge_start, &mut wire_edge_owners)?;

    encode_source_less_edges_vertices_points(
        &mut records,
        target,
        attributes,
        topology,
        SourceLessRecordStarts {
            curve: curve_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            attribute: attribute_start,
        },
        &wire_edge_owners,
        &free_vertex_owners,
    )?;
    for body in &model.bodies {
        if let Some(transform) = body.transform {
            native_transform(&mut records, topology, body, transform)?;
            records.push(0x11);
        }
    }
    encode_source_less_attributes(
        &mut records,
        target,
        attributes,
        attribute_start,
        AttributeOwnerStarts {
            body: body_start,
            surface: Some(SurfaceOwnerStarts {
                face: face_start,
                coedge: coedge_start,
            }),
            edge: edge_start,
            vertex: vertex_start,
        },
    )?;
    native_history_tail(&mut records, native)?;
    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

/// Relative allowance of a closed edge vertex against its carrier's
/// evaluated traversal ends, per unit of the larger coordinate magnitude
/// and 1.
const EPS_CLOSED_EDGE_VERTEX: f64 = 1.0e-8;

/// The interval that a closed edge with no stored range traverses: the
/// carrier's closed traversal, when the edge vertex lies at both of its
/// ends. Any other edge states no traversal and is refused.
fn closed_edge_traversal(
    edge: &cadmpeg_ir::topology::Edge,
    carrier: &CurveGeometry,
    vertex: Point3,
    tolerance: f64,
) -> Result<[f64; 2], CodecError> {
    let no_traversal = || {
        CodecError::NotImplemented(format!(
            "source-less F3D edge generation requires a parameter range for edge {}",
            edge.id
        ))
    };
    let geometry = carrier
        .solved()
        .filter(|_| edge.start == edge.end)
        .ok_or_else(no_traversal)?;
    let range = geometry
        .closed_traversal_range()
        .ok_or_else(no_traversal)?
        .endpoints();
    for parameter in range {
        let end = match cadmpeg_ir::eval::decode::curve_point_solved(
            EvaluationAdmission::Standard,
            geometry,
            parameter,
        ) {
            Ok(end) => end.get(),
            Err(EvaluationFailure::ResourceLimit(limit)) => {
                return Err(CodecError::ResourceLimit(limit))
            }
            Err(
                EvaluationFailure::NoValue
                | EvaluationFailure::NonFinite(_)
                | EvaluationFailure::WorkExhausted,
            ) => return Err(no_traversal()),
        };
        let pairs = [(vertex.x, end.x), (vertex.y, end.y), (vertex.z, end.z)];
        let scale = pairs
            .iter()
            .fold(1.0_f64, |scale, (a, b)| scale.max(a.abs()).max(b.abs()));
        let allowed = (scale * EPS_CLOSED_EDGE_VERTEX).max(tolerance);
        if pairs.iter().any(|(a, b)| (a - b).abs() > allowed) {
            return Err(no_traversal());
        }
    }
    Ok(range)
}

/// Base record indices for the source-less stream sections that
/// `encode_source_less_edges_vertices_points` cross-references.
#[derive(Clone, Copy)]
struct SourceLessRecordStarts {
    curve: i64,
    edge: i64,
    vertex: i64,
    point: i64,
    attribute: i64,
}

fn encode_source_less_edges_vertices_points(
    records: &mut Vec<u8>,
    target: &CadIr,
    attributes: &AttributeIndex<'_>,
    topology: &NativeGenerationIndex<'_>,
    starts: SourceLessRecordStarts,
    edge_owners: &BTreeMap<cadmpeg_ir::ids::EdgeId, i64>,
    free_vertex_owners: &BTreeMap<VertexId, i64>,
) -> Result<(), CodecError> {
    let SourceLessRecordStarts {
        curve: curve_start,
        edge: edge_start,
        vertex: vertex_start,
        point: point_start,
        attribute: attribute_start,
    } = starts;
    let model = &target.model;
    let vertex_ordinals: HashMap<_, _> = model
        .vertices
        .iter()
        .enumerate()
        .map(|(ordinal, vertex)| (&vertex.id, ordinal))
        .collect();
    let curve_ordinals: HashMap<_, _> = model
        .curves
        .iter()
        .enumerate()
        .map(|(ordinal, curve)| (&curve.id, ordinal))
        .collect();
    let point_ordinals: HashMap<_, _> = model
        .points
        .iter()
        .enumerate()
        .map(|(ordinal, point)| (&point.id, ordinal))
        .collect();
    for (edge_ordinal, edge) in model.edges.iter().enumerate() {
        let start = vertex_ordinals.get(&edge.start).copied();
        let end = vertex_ordinals.get(&edge.end).copied();
        let (Some(start), Some(end)) = (start, end) else {
            return Err(CodecError::malformed(format_args!(
                "edge {} has an unresolved vertex",
                edge.id
            )));
        };
        let curve_ref = edge
            .curve()
            .map(|curve_id| {
                curve_ordinals
                    .get(curve_id)
                    .copied()
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "edge references missing curve {curve_id}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(curve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let mut range = match edge.param_range() {
            Some(range) => range.get(),
            None if edge.start == edge.end => {
                let vertex = &model.vertices[start];
                let position = point_ordinals
                    .get(&vertex.point)
                    .map(|ordinal| model.points[*ordinal].position().get())
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "vertex {} has an unresolved carrier",
                            vertex.id
                        ))
                    })?;
                let tolerance = [edge.tolerance, vertex.tolerance]
                    .into_iter()
                    .flatten()
                    .map(cadmpeg_ir::scalar::PositiveReal::get)
                    .fold(0.0, f64::max);
                edge.curve()
                    .and_then(|curve_id| curve_ordinals.get(curve_id))
                    .map(|ordinal| {
                        closed_edge_traversal(
                            edge,
                            &model.curves[*ordinal].geometry,
                            position,
                            tolerance,
                        )
                    })
                    .transpose()?
                    .unwrap_or([0.0, 1.0])
            }
            None => [0.0, 1.0],
        };
        // Conic edge parameters are angles in both the IR and the native
        // stream; line parameters are arc lengths, millimeters in the IR
        // and centimeters natively.
        if edge.curve().is_some_and(|curve_id| {
            curve_ordinals.get(curve_id).is_some_and(|ordinal| {
                matches!(
                    model.curves[*ordinal].geometry,
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
                )
            })
        }) {
            range[0] /= LEN_TO_MM;
            range[1] /= LEN_TO_MM;
        }
        native_ident(
            records,
            if edge.tolerance.is_some() {
                "tedge"
            } else {
                "edge"
            },
        )?;
        let persistent =
            edge_persistent_attribute_ref(target, attributes, edge, edge_ordinal, attribute_start)?;
        native_ref(
            records,
            if let Some(reference) = persistent {
                reference
            } else {
                timestamp_attribute_ref(
                    target,
                    attributes,
                    &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
                    attribute_start,
                )?
                .unwrap_or(-1)
            },
        );
        native_i64(records, -1);
        native_ref(records, -1);
        native_ref(records, native_record_index(vertex_start, start)?);
        native_f64(records, range[0]);
        native_ref(records, native_record_index(vertex_start, end)?);
        native_f64(records, range[1]);
        native_ref(records, edge_owners.get(&edge.id).copied().unwrap_or(-1));
        native_ref(records, curve_ref);
        let (sense, continuity) = edge_record_metadata(topology, edge)?;
        records.push(native_bool(sense == Sense::Reversed));
        native_string(records, &continuity)?;
        native_tolerant_edge_tail(records, topology, edge);
        records.push(0x11);
    }
    for vertex in &model.vertices {
        let point = point_ordinals.get(&vertex.point).copied();
        let Some(point) = point else {
            return Err(CodecError::malformed(format_args!(
                "vertex {} has an unresolved carrier",
                vertex.id
            )));
        };
        let ownership = free_vertex_owners.get(&vertex.id).copied();
        native_ident(
            records,
            if vertex.tolerance.is_some()
                || topology.tolerant_vertices.contains_key(vertex.id.as_str())
            {
                "tvertex"
            } else {
                "vertex"
            },
        )?;
        native_ref(
            records,
            timestamp_attribute_ref(
                target,
                attributes,
                &cadmpeg_ir::attributes::AttributeTarget::Vertex(vertex.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1),
        );
        native_i64(records, -1);
        native_ref(records, -1);
        if let Some(wire) = ownership {
            native_ref(records, wire);
            native_i64(records, -1);
        } else {
            let (edge, endpoint_index) = vertex_ownership(target, topology, vertex)?;
            native_ref(records, native_record_index(edge_start, edge)?);
            native_i64(records, i64::from(endpoint_index.code()));
        }
        native_ref(records, native_record_index(point_start, point)?);
        native_tolerant_vertex_tail(records, topology, vertex);
        records.push(0x11);
    }
    for point in &model.points {
        native_ident(records, "point")?;
        native_ref(records, -1);
        native_i64(records, -1);
        native_ref(records, -1);
        native_point(
            records,
            [
                point.position().get().x / LEN_TO_MM,
                point.position().get().y / LEN_TO_MM,
                point.position().get().z / LEN_TO_MM,
            ],
        );
        records.push(0x11);
    }
    Ok(())
}

fn vertex_ownership(
    target: &CadIr,
    topology: &NativeGenerationIndex<'_>,
    vertex: &cadmpeg_ir::topology::Vertex,
) -> Result<(usize, EndpointSlot), CodecError> {
    let model = &target.model;
    if let Some(metadata) = topology.vertex_ownerships.get(vertex.id.as_str()) {
        let ordinal = model
            .edges
            .iter()
            .position(|edge| edge.id == metadata.owning_edge)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "vertex {} references missing owning edge {}",
                    vertex.id, metadata.owning_edge
                ))
            })?;
        let edge = &model.edges[ordinal];
        let valid = match metadata.endpoint_index {
            EndpointSlot::Start => edge.start == vertex.id,
            EndpointSlot::End => edge.end == vertex.id,
        };
        if !valid {
            return Err(CodecError::malformed(format_args!(
                "vertex {} endpoint slot {} conflicts with owning edge {}",
                vertex.id,
                metadata.endpoint_index.code(),
                metadata.owning_edge
            )));
        }
        return Ok((ordinal, metadata.endpoint_index));
    }
    model
        .edges
        .iter()
        .enumerate()
        .find_map(|(ordinal, edge)| {
            if edge.start == vertex.id {
                Some((ordinal, EndpointSlot::Start))
            } else if edge.end == vertex.id {
                Some((ordinal, EndpointSlot::End))
            } else {
                None
            }
        })
        .ok_or_else(|| CodecError::malformed(format_args!("vertex {} has no edge", vertex.id)))
}

fn native_face_sidedness(
    records: &mut Vec<u8>,
    topology: &NativeGenerationIndex<'_>,
    face: &cadmpeg_ir::topology::Face,
) {
    let containment = topology
        .face_sidedness
        .get(face.id.as_str())
        .and_then(|metadata| metadata.containment);
    records.push(native_bool(containment.is_some()));
    if let Some(containment) = containment {
        records.push(match containment {
            cadmpeg_asm::brep::records::FaceContainment::In => 0x0a,
            cadmpeg_asm::brep::records::FaceContainment::Out => 0x0b,
        });
    }
}

fn native_face_sense(
    model: &cadmpeg_ir::document::Model,
    topology: &NativeGenerationIndex<'_>,
    face: &cadmpeg_ir::topology::Face,
) -> Sense {
    let carrier_flipped = topology.face_sidedness.get(face.id.as_str()).map_or_else(
        || {
            model
                .surfaces
                .iter()
                .find(|surface| surface.id == face.surface)
                .is_some_and(|surface| {
                    matches!(
                        surface.geometry.solved(),
                        Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Torus(torus))
                            if torus.minor_radius().get() < 0.0
                    )
                })
        },
        |metadata| metadata.carrier_flipped,
    );
    normalized_face_sense_to_native(face.sense, carrier_flipped)
}

fn native_wire_side(
    topology: &NativeGenerationIndex<'_>,
    shell: &ShellId,
    edges: &[cadmpeg_ir::ids::EdgeId],
    free_vertex: Option<&VertexId>,
) -> Result<u8, CodecError> {
    let mut matches = topology
        .wires_by_shell
        .get(shell.as_str())
        .into_iter()
        .flatten()
        .copied()
        .filter(|wire| wire.members.edges() == edges && wire.members.free_vertex() == free_vertex);
    let side = match (matches.next(), matches.next()) {
        (None, _) => cadmpeg_asm::brep::records::WireSide::Out,
        (Some(wire), None) => wire.side,
        (Some(_), Some(_)) => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D generation has duplicate native wire metadata on shell {shell}"
            )))
        }
    };
    Ok(match side {
        cadmpeg_asm::brep::records::WireSide::In => 0x0a,
        cadmpeg_asm::brep::records::WireSide::Out => 0x0b,
    })
}

fn native_tolerant_vertex_tail(
    records: &mut Vec<u8>,
    topology: &NativeGenerationIndex<'_>,
    vertex: &cadmpeg_ir::topology::Vertex,
) {
    let stored = topology.tolerant_vertices.get(vertex.id.as_str()).copied();
    // The unset evaluated slot has no neutral tolerance; the native tail
    // carries the fact and the sentinel is written back.
    let tolerance = match (vertex.tolerance, stored.map(|tail| tail.evaluated_slot)) {
        (Some(tolerance), _) => Some(tolerance.get()),
        (None, Some(EvaluatedToleranceSlot::Unset { .. })) => Some(-1.0),
        // The source record ended before the evaluated slot; only the
        // leading slots are written back.
        (
            None,
            Some(EvaluatedToleranceSlot::Absent {} | EvaluatedToleranceSlot::Evaluated { .. }),
        ) => None,
        (None, None) => return,
    };
    // The record stores three f64 tolerance slots: the two leading slots
    // verbatim (default: the -1 unevaluated sentinel) and the evaluated
    // tolerance last, followed by a version-gated trailing integer (0 or 1,
    // retained verbatim and elided when absent in the source; fresh
    // generation writes 0). A negative tolerance is the
    // unevaluated sentinel, stored verbatim; a non-negative tolerance
    // converts from millimetres to centimetres.
    let leading = stored.as_ref().map_or([-1.0; 2], |tail| {
        tail.leading_tolerances
            .map(cadmpeg_ir::scalar::FiniteReal::get)
    });
    let trailing = stored
        .as_ref()
        .map_or(Some(0), |tail| tail.evaluated_slot.trailing());
    for value in leading {
        native_f64(records, value);
    }
    let Some(tolerance) = tolerance else {
        return;
    };
    native_f64(
        records,
        if tolerance < 0.0 {
            tolerance
        } else {
            tolerance / LEN_TO_MM
        },
    );
    if let Some(trailing) = trailing {
        native_i64(records, trailing);
    }
}

fn native_tolerant_edge_tail(
    records: &mut Vec<u8>,
    topology: &NativeGenerationIndex<'_>,
    edge: &cadmpeg_ir::topology::Edge,
) {
    let Some(tolerance) = edge.tolerance else {
        return;
    };
    native_f64(records, tolerance.get() / LEN_TO_MM);
    let (revision, trailing) = topology
        .tolerant_edges
        .get(edge.id.as_str())
        .map_or((23100, Some(0)), |tail| {
            (tail.entity_revision, tail.trailing_field)
        });
    native_i64(records, revision);
    if let Some(trailing) = trailing {
        native_i64(records, trailing);
    }
}

fn edge_record_metadata(
    topology: &NativeGenerationIndex<'_>,
    edge: &cadmpeg_ir::topology::Edge,
) -> Result<(Sense, String), CodecError> {
    let metadata = topology.edge_continuities.get(edge.id.as_str()).copied();
    let sense = metadata
        .as_ref()
        .map_or(Sense::Forward, |metadata| metadata.sense);
    let continuity = metadata.map_or_else(
        || "unknown".to_owned(),
        |metadata| metadata.continuity.clone(),
    );
    if continuity != "tangent" && continuity != "unknown" {
        return Err(CodecError::malformed(format_args!(
            "F3D edge {} has unsupported continuity token {continuity}",
            edge.id
        )));
    }
    Ok((sense, continuity))
}

fn apply_native_edge_owners(
    target: &CadIr,
    topology: &NativeGenerationIndex<'_>,
    coedge_start: i64,
    owners: &mut BTreeMap<cadmpeg_ir::ids::EdgeId, i64>,
) -> Result<(), CodecError> {
    for ownership in topology.edge_ownerships {
        if !target
            .model
            .edges
            .iter()
            .any(|edge| edge.id == ownership.edge)
        {
            return Err(CodecError::malformed(format_args!(
                "F3D edge ownership {} references missing edge {}",
                ownership.id(),
                ownership.edge
            )));
        }
        let owner = match &ownership.owner_coedge {
            None => -1,
            Some(owner) => {
                if let Some((ordinal, coedge)) = target
                    .model
                    .coedges
                    .iter()
                    .enumerate()
                    .find(|(_, coedge)| coedge.id == *owner)
                {
                    if coedge.edge != ownership.edge {
                        return Err(CodecError::malformed(format_args!(
                            "F3D edge ownership {} selects a coedge of another edge",
                            ownership.id()
                        )));
                    }
                    native_record_index(coedge_start, ordinal)?
                } else if owners.contains_key(&ownership.edge) {
                    continue;
                } else {
                    return Err(CodecError::malformed(format_args!(
                        "F3D edge ownership {} references missing coedge {owner}",
                        ownership.id()
                    )));
                }
            }
        };
        owners.insert(ownership.edge.clone(), owner);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    use crate::test_support::smbh_geometry_test::{
        synthetic_free_vertex_body_smbh, synthetic_wire_body_smbh,
    };
    use crate::test_support::zip_test::f3d_with_smbh;
    use crate::writer::generate::write_new;

    mod closed_edge_range;

    /// `asm.md` states that the native sense on a negative-minor torus is
    /// relative to its inward normal. A face with no native metadata states
    /// the neutral sense, so only the negative-minor carrier writes its reverse.
    #[test]
    fn source_less_face_on_a_negative_minor_torus_writes_the_native_sense() {
        use super::{native_face_sense, NativeGenerationIndex};
        use cadmpeg_ir::geometry::analytic::TorusSurface;
        use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
        use cadmpeg_ir::math::{Point3, Vector3};
        use cadmpeg_ir::topology::Sense;

        let native = crate::native::F3dNative::default();
        let index = NativeGenerationIndex::new(&native);
        let mut model = cadmpeg_ir::examples::unit_cube()
            .expect("unit cube fixture is admitted")
            .model;
        let surface = model.faces[0].surface.clone();
        for (minor, expected) in [(-2.0, Sense::Forward), (2.0, Sense::Reversed)] {
            model
                .surfaces
                .iter_mut()
                .find(|candidate| candidate.id == surface)
                .expect("face surface")
                .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                TorusSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    6.0,
                    minor,
                )
                .expect("torus"),
            ));
            let mut face = model.faces[0].clone();
            face.sense = Sense::Reversed;
            assert_eq!(native_face_sense(&model, &index, &face), expected);
        }
    }

    fn decoded_wire(bytes: &[u8]) -> cadmpeg_ir::CadIr {
        let decoded = crate::F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(bytes)),
                &DecodeOptions::default(),
            )
            .expect("source wire is admitted");
        let (mut target, _, _) = decoded.into_parts();
        target.source = None;
        target.native = cadmpeg_ir::native::Native::default();
        target
    }

    #[test]
    fn wire_vertex_admission_refuses_invalid_membership_before_output() {
        let target = decoded_wire(&synthetic_free_vertex_body_smbh());
        let mut valid_output = Vec::new();
        write_new(&target, &mut valid_output).expect("unmodified free wire writes");
        assert!(!valid_output.is_empty());

        let mut missing = target.clone();
        missing.model.vertices.clear();

        let mut unowned = target.clone();
        let mut extra_vertex = unowned.model.vertices[0].clone();
        extra_vertex.id = cadmpeg_ir::ids::VertexId::mint("test:wire:vertex#unowned")
            .expect("test identity is admitted");
        unowned.model.vertices.push(extra_vertex);

        let mut duplicated = target;
        duplicated.model.shells[0].add_free_vertex(duplicated.model.vertices[0].id.clone());

        let mut endpoint = decoded_wire(&synthetic_wire_body_smbh());
        endpoint.model.shells[0].add_free_vertex(endpoint.model.edges[0].start.clone());

        for (target, reason) in [
            (missing, "wire references missing free vertex"),
            (
                unowned,
                "source-less F3D vertices must be edge endpoints or free wire vertices",
            ),
            (duplicated, "belongs to more than one wire"),
            (endpoint, "is both free and an edge endpoint"),
        ] {
            let mut output = vec![0x93, 0x2a];
            let error = write_new(&target, &mut output)
                .expect_err("invalid wire ownership must refuse the write");
            assert!(error.to_string().contains(reason), "{error}");
            assert_eq!(output, [0x93, 0x2a], "failed write changed caller output");
        }
    }
}
