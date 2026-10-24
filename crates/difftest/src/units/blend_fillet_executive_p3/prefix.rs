//! Actual fillet setup and retained corner/solver checkpoints.
//! Modes 1–5 omit smoothing on both sides. Later modes run the real prelude,
//! stopping after adjustment, direct first-solver geometry, or all-solver geometry
//! followed by each corner’s tangent adjustment, repeated vertices and edges,
//! actual rail trimming, enabled classification/repair, or intermediate faces.
#[path = "edge_geometry.rs"]
mod edge_geometry;
#[path = "generated_geometry.rs"]
mod generated_geometry;
#[path = "intermediate_faces.rs"]
mod intermediate_faces;
#[path = "rail_classification.rs"]
mod rail_classification;
#[path = "rail_metadata.rs"]
mod rail_metadata;
#[path = "shrink_state.rs"]
mod shrink_state;
#[path = "stitch_state.rs"]
mod stitch_state;
use super::{label, section, status, take_section, Checkpoint, RealSolid};
use crate::{HarnessError, Output};
use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession, SolverObject};
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, edgeuse::p1 as eu, Brep, EdgeId};
use solidcore::{EntityRef, Result as SolidResult, SolidError};
use solidcore_ffi::units::blend_fillet_executive_p3 as ffi;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("harness::fillet_prefix", reason)
}

fn snapshot(session: &FilletSession<'_>, parameters: &mut Vec<f64>) -> SolidResult<Vec<f64>> {
    let exec = session.executive();
    let target = session.target();
    let target_vertices = p1::vertices(target, None)?;
    let target_edges = p1::edges(target, None)?;
    let target_faces = p1::faces(target, None)?;
    let mut target_uses = Vec::new();
    for edge in target_edges {
        target_uses.extend(edge::edgeuses(target, edge, None, None)?);
    }
    let pseudo = session.pseudo_brep();
    let vertices = p1::vertices(pseudo.brep(), None)?;
    let edges = pseudo.edge_list();
    let mut corners = Vec::new();
    let mut corner_ids = Vec::new();
    for object in &exec.corners {
        let FilletObject::Corner(id) = object else {
            return Err(invalid("actual corner required"));
        };
        corner_ids.push(*id);
        corners.push(session.corner(*id)?);
    }
    let corner_vertices: Vec<_> = corners
        .iter()
        .flat_map(|c| c.vertices.iter().copied())
        .collect();
    let mut geometry_ids = Vec::new();
    for object in &exec.solvers {
        let FilletObject::Solver(id) = object else {
            return Err(invalid("actual solver required"));
        };
        geometry_ids.extend(session.solver(*id)?.base().fillet_geoms.iter().copied());
    }
    let mut state = vec![
        f64::from(exec.options.global_merge),
        exec.solvers.len() as f64,
        corners.len() as f64,
        vertices.len() as f64,
        edges.len() as f64,
    ];
    for corner in corners {
        parameters.extend([corner.approx_tol_3d, corner.tangency_tol_radians]);
        state.extend([
            f64::from(corner.corner_type() as u32),
            label(&target_vertices, corner.vertex),
            corner.vertices.len() as f64,
            corner.edges.len() as f64,
        ]);
        for &id in &corner.vertices {
            let vertex = pseudo.fillet_vertex(id)?;
            let object = vertex.point_class_object();
            let object_label = match object {
                None => -1.0,
                Some(EntityRef::Edgeuse(eu)) => label(&target_uses, Some(eu)),
                Some(_) => -2.0,
            };
            state.extend([
                label(&corner_vertices, Some(id)),
                label(&vertices, Some(id)),
                f64::from(vertex.fillet_vertex_type() as u32),
                label(&corner_ids, vertex.fillet_corner()),
                f64::from(vertex.point_class() as u32),
                object.map_or(0.0, |object| {
                    session.target_identity(object).type_code as f64
                }),
                object_label,
            ]);
        }
        for &edge in &corner.edges {
            state.push(label(edges, Some(edge)));
        }
    }
    for object in &exec.solvers {
        let FilletObject::Solver(id) = object else {
            return Err(invalid("actual solver required"));
        };
        let solver = session.solver(*id)?;
        let constant = if let SolverObject::ConstantRadius(solver) = solver {
            Some(solver)
        } else {
            None
        };
        let solver_type = match solver {
            SolverObject::ConstantRadius(s) => s.constant_radius_solver_type(),
            SolverObject::ConstantDistance(s) => s.solver_type(),
            SolverObject::VariableRadius(s) => s.solver_type(),
            SolverObject::SurfaceSurface(s) => s.solver_type(),
        };
        state.extend([
            f64::from(solver_type as u32),
            if constant.is_some() { 2.0 } else { 0.0 },
        ]);
        if let Some(constant) = constant {
            parameters.extend([constant.fillet_radius(0)?, constant.fillet_radius(1)?]);
        }
        let base = solver.base();
        parameters.extend([
            base.this_approx_tol_3d(),
            base.this_ang_tol_rad(),
            base.tangency_tolerance(),
        ]);
        state.push(base.fillet_geoms.len() as f64);
        for &geometry in &base.fillet_geoms {
            for rail in 0..2 {
                state.push(label(edges, pseudo.geom(geometry)?.rail(rail)));
            }
        }
    }
    for &id in edges {
        let edge = pseudo.fillet_edge(id)?;
        state.extend([
            f64::from(edge.fillet_edge_type() as u32),
            label(&target_faces, edge.original_face()),
            label(&corner_ids, edge.fillet_corner()),
        ]);
        let uses = edge::edgeuses(pseudo.brep(), id, None, None)?;
        state.push(uses.len() as f64);
        for use_ in uses {
            let edgeuse = pseudo.brep().edgeuse(use_)?;
            let vertex = edgeuse
                .vertexuse
                .map(|vu| pseudo.brep().vertexuse(vu).map(|vu| vu.vertex))
                .transpose()?
                .flatten();
            state.extend([
                f64::from(eu::orientation(pseudo.brep(), use_)? as i32),
                label(&corner_vertices, vertex),
                label(&vertices, vertex),
                label(&geometry_ids, pseudo.fillet_edgeuse(use_)?.fillet_geom()),
            ]);
        }
    }
    Ok(state)
}

fn offset_snapshot(session: &FilletSession<'_>) -> SolidResult<(Vec<f64>, Vec<f64>)> {
    let mut geometries = Vec::new();
    for object in &session.executive().solvers {
        let FilletObject::Solver(id) = *object else {
            return Err(invalid("actual solver required"));
        };
        geometries.extend(session.solver(id)?.base().fillet_geoms.iter().copied());
    }
    let mut state = Vec::new();
    let mut distances = Vec::new();
    for (index, object) in session.executive().solvers.iter().enumerate() {
        let FilletObject::Solver(id) = *object else {
            return Err(invalid("actual solver required"));
        };
        let solver = session.solver(id)?.base();
        state.extend([
            index as f64,
            label(&geometries, solver.fillet_geom_for_surfaces()),
            solver.fillet_geoms.len() as f64,
        ]);
        for &geometry_id in &solver.fillet_geoms {
            let geometry = session.pseudo_brep().geom(geometry_id)?;
            state.extend([
                label(&geometries, Some(geometry_id)),
                f64::from(geometry.fillet_geom_type() as u32),
            ]);
            for rail in 0..2 {
                let own = geometry.off_surfs.get(rail).and_then(Option::as_ref);
                let original = solver
                    .surfaces
                    .get(rail)
                    .and_then(Option::as_ref)
                    .map(|slot| &slot.surface);
                let same_object = own.zip(original).is_some_and(|(a, b)| std::ptr::eq(a, b));
                let same_base = own
                    .zip(original)
                    .is_some_and(|(a, b)| a.base_id == b.base_id);
                state.extend([
                    rail as f64,
                    f64::from(own.is_some()),
                    f64::from(original.is_some()),
                    f64::from(own.is_some()),
                    f64::from(original.is_some()),
                    f64::from(same_object),
                    f64::from(same_base),
                    own.map_or(-1.0, |surface| surface.surface.type_code() as f64),
                    original.map_or(-1.0, |surface| surface.surface.type_code() as f64),
                    own.map_or(-1.0, |surface| surface.base_surface().type_code() as f64),
                    original.map_or(-1.0, |surface| surface.base_surface().type_code() as f64),
                ]);
                if let Some(surface) = own {
                    distances.push(surface.offset_distance());
                }
                if let Some(surface) = original {
                    distances.push(surface.offset_distance());
                }
            }
        }
    }
    Ok((state, distances))
}

pub(super) fn run(
    k: &mut Kernel,
    body: &mut Brep,
    selected: &[EdgeId],
    input: &RealSolid,
    before: &[f64],
) -> SolidResult<Vec<f64>> {
    let mut session = FilletSession::new(k, body)?;
    let mut statuses = [-1.0; 12];
    let prepare = session.prepare_constant_radius_edges(
        k,
        selected,
        input.radius,
        input.xsect,
        input.continuity,
        input.thumbweight,
    );
    statuses[0] = match prepare {
        Ok(()) => 0.0,
        Err(error) => status(error)?,
    };
    let mut prior = Vec::new();
    let mut offsets_before = (Vec::new(), Vec::new());
    let mut rails_before = (Vec::new(), Vec::new());
    let mut classes_before = (Vec::new(), Vec::new());
    let mut adjusted = Vec::new();
    let mut attributes_before = Vec::new();
    let mut stitch_outcome = None;
    let mut attributes_after = Vec::new();
    let mut shrink_before = (Vec::new(), Vec::new());
    let mut shrink_after = (Vec::new(), Vec::new());
    if statuses[0] == 0.0 && input.checkpoint != Checkpoint::Prepared {
        statuses[1] = match session.create_corners(k, None) {
            Ok(()) => 0.0,
            Err(error) => status(error)?,
        };
        if input.checkpoint.has_offsets() {
            offsets_before = offset_snapshot(&session)?;
        }
        if statuses[1] == 0.0 && input.checkpoint.all_corners() {
            prior = snapshot(&session, &mut Vec::new())?;
            let vertices_index = if input.checkpoint.code() >= 6 {
                statuses[2] = match session.process_smooth_edge_sequences(k) {
                    Ok(()) => 0.0,
                    Err(error) => status(error)?,
                };
                3
            } else {
                2
            };
            if vertices_index == 2 || statuses[2] == 0.0 {
                *statuses
                    .get_mut(vertices_index)
                    .ok_or_else(|| invalid("corner status slot"))? =
                    match session.create_corner_vertices(k) {
                        Ok(()) => 0.0,
                        Err(error) => status(error)?,
                    };
            }
            if input.checkpoint.code() >= 6 && statuses[3] == 0.0 {
                statuses[4] = match session.adjust_corner_topology(k) {
                    Ok(()) => 0.0,
                    Err(error) => status(error)?,
                };
                if input.checkpoint == Checkpoint::SolverGeometry && statuses[4] == 0.0 {
                    if let Some(FilletObject::Solver(id)) =
                        session.executive().solvers.first().copied()
                    {
                        statuses[5] = match session.calc_solver_geometry(k, id) {
                            Ok(()) => 0.0,
                            Err(error) => status(error)?,
                        };
                    }
                }
                if input.checkpoint.code() >= 8 && statuses[4] == 0.0 {
                    statuses[5] = match session.calculate_solver_geometry(k) {
                        Ok(()) => 0.0,
                        Err(error) => status(error)?,
                    };
                    if statuses[5] == 0.0 {
                        statuses[6] = match session.calculate_corner_edges(k) {
                            Ok(()) => 0.0,
                            Err(error) => status(error)?,
                        };
                    }
                    if input.checkpoint.code() >= 9 && statuses[6] == 0.0 {
                        rails_before = rail_metadata::snapshot(k, &session, false)?;
                        statuses[7] = match session.trim_solver_rails(k) {
                            Ok(()) => 0.0,
                            Err(error) => status(error)?,
                        };
                        if input.checkpoint.code() >= 10 && statuses[7] == 0.0 {
                            classes_before = rail_classification::snapshot(k, &session)?;
                            statuses[8] = match session.classify_solver_rails_with_adjusted(k) {
                                Ok(ids) => {
                                    adjusted = ids;
                                    0.0
                                }
                                Err(error) => status(error)?,
                            };
                            if input.checkpoint.code() >= 11 && statuses[8] == 0.0 {
                                statuses[9] = match session.build_fillet_faces(k) {
                                    Ok(()) => 0.0,
                                    Err(error) => status(error)?,
                                };
                                if input.checkpoint.code() >= 12 && statuses[9] == 0.0 {
                                    attributes_before = stitch_state::attributes(&session)?;
                                    statuses[10] = match session.stitch_fillet_faces(k) {
                                        Ok(attempt) => match attempt.status {
                                            None => {
                                                stitch_outcome = Some((
                                                    attempt.stitched,
                                                    attempt.lamina,
                                                    attempt.max_vertex_gap,
                                                    attempt.max_edge_gap,
                                                ));
                                                0.0
                                            }
                                            Some(error) => status(error)?,
                                        },
                                        Err(error) => status(error)?,
                                    };
                                    attributes_after = stitch_state::attributes(&session)?;
                                    if input.checkpoint == Checkpoint::ShrunkFaces
                                        && statuses[10] == 0.0
                                    {
                                        shrink_before = shrink_state::snapshot(&session)?;
                                        statuses[11] = match session.shrink_fillet_faces(k) {
                                            Ok(()) => 0.0,
                                            Err(error) => status(error)?,
                                        };
                                        shrink_after = shrink_state::snapshot(&session)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if statuses[1] == 0.0
            && matches!(
                input.checkpoint,
                Checkpoint::FirstTopology | Checkpoint::FirstVertexGeometry
            )
        {
            prior = snapshot(&session, &mut Vec::new())?;
            if let Some(FilletObject::Corner(id)) = session.executive().corners.first().copied() {
                statuses[2] = match session.make_nx1_corner_topology(k, id) {
                    Ok(()) => 0.0,
                    Err(error) => status(error)?,
                };
                if statuses[2] == 0.0 && input.checkpoint == Checkpoint::FirstVertexGeometry {
                    statuses[3] = match session.calc_nx1_corner_vert_geom(k, id) {
                        Ok(()) => 0.0,
                        Err(error) => status(error)?,
                    };
                }
            }
        }
    }
    let mut parameters = Vec::new();
    let state = snapshot(&session, &mut parameters)?;
    let mut out = Vec::new();
    section(&mut out, before);
    let count = input.checkpoint.status_count();
    out.extend_from_slice(
        statuses
            .get(..count)
            .ok_or_else(|| invalid("status record length"))?,
    );
    section(&mut out, &prior);
    section(&mut out, &state);
    section(&mut out, &parameters);
    if input.checkpoint == Checkpoint::FirstVertexGeometry {
        let mut geometry_state = Vec::new();
        let mut geometry_values = Vec::new();
        if let Some(FilletObject::Corner(id)) = session.executive().corners.first().copied() {
            for (index, &fv) in session.corner(id)?.vertices.iter().enumerate() {
                let vertex = session.pseudo_brep().fillet_vertex(fv)?;
                geometry_state.extend([
                    index as f64,
                    f64::from(vertex.status() as u32),
                    f64::from(vertex.is_processed()),
                ]);
                if vertex.is_processed() {
                    let record = session.pseudo_brep().brep().vertex(fv)?;
                    geometry_values.extend([
                        record.point.x,
                        record.point.y,
                        record.point.z,
                        record.tolerance,
                        vertex.original_t_param(),
                    ]);
                }
            }
        }
        section(&mut out, &geometry_state);
        section(&mut out, &geometry_values);
    }
    if input.checkpoint.all_corners() {
        let mut flags = Vec::new();
        let mut vertex_state = Vec::new();
        let mut geometry = Vec::new();
        for (corner_index, object) in session.executive().corners.iter().enumerate() {
            let FilletObject::Corner(id) = *object else {
                return Err(invalid("executive corner is not a corner"));
            };
            let corner = session.corner(id)?;
            flags.extend([
                corner_index as f64,
                f64::from(corner.is_blending_corner()),
                f64::from(corner.is_tangent_corner()),
                f64::from(corner.is_calc_as_const_rad()),
            ]);
            for (index, &fv) in corner.vertices.iter().enumerate() {
                let vertex = session.pseudo_brep().fillet_vertex(fv)?;
                vertex_state.extend([
                    corner_index as f64,
                    index as f64,
                    f64::from(vertex.status() as u32),
                    f64::from(vertex.is_processed()),
                ]);
                if vertex.is_processed() {
                    let record = session.pseudo_brep().brep().vertex(fv)?;
                    geometry.extend([
                        record.point.x,
                        record.point.y,
                        record.point.z,
                        record.tolerance,
                        vertex.original_t_param(),
                    ]);
                }
            }
        }
        section(&mut out, &flags);
        section(&mut out, &vertex_state);
        section(&mut out, &geometry);
    }
    if input.checkpoint.has_offsets() {
        let offsets_after = offset_snapshot(&session)?;
        section(&mut out, &offsets_before.0);
        section(&mut out, &offsets_before.1);
        section(&mut out, &offsets_after.0);
        section(&mut out, &offsets_after.1);
    }
    if input.checkpoint.code() >= 6 {
        let flags: Vec<_> = session
            .executive()
            .corners
            .iter()
            .enumerate()
            .map(|(index, object)| {
                let FilletObject::Corner(id) = *object else {
                    return Err(invalid("actual corner required"));
                };
                let corner = session.corner(id)?;
                Ok([
                    index as f64,
                    f64::from(corner.is_degenerate_corner()),
                    f64::from(corner.topology_adjusted),
                ])
            })
            .collect::<SolidResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        section(&mut out, &flags);
    }
    if input.checkpoint.code() >= 7 {
        let (state, values) = generated_geometry::snapshot(k, &session)?;
        section(&mut out, &state);
        section(&mut out, &values);
    }
    if input.checkpoint.code() >= 8 {
        let (state, values) = edge_geometry::snapshot(k, &session)?;
        section(&mut out, &state);
        section(&mut out, &values);
    }
    if input.checkpoint.code() >= 9 {
        let rails_after = rail_metadata::snapshot(k, &session, true)?;
        section(&mut out, &rails_before.0);
        section(&mut out, &rails_before.1);
        section(&mut out, &rails_after.0);
        section(&mut out, &rails_after.1);
    }
    if input.checkpoint.code() >= 10 {
        let after = rail_classification::snapshot(k, &session)?;
        section(&mut out, &classes_before.0);
        section(&mut out, &classes_before.1);
        section(&mut out, &after.0);
        section(&mut out, &after.1);
        let labels: Vec<_> = adjusted
            .iter()
            .map(|id| label(&session.executive().corners, Some(*id)))
            .collect();
        section(&mut out, &labels);
    }
    if input.checkpoint.code() >= 11 {
        let (state, values) = intermediate_faces::snapshot(k, &session)?;
        section(&mut out, &state);
        section(&mut out, &values);
        section(&mut out, &intermediate_faces::source_binding(&session)?);
    }
    if input.checkpoint.code() >= 12 {
        section(&mut out, &attributes_before);
        section(
            &mut out,
            &if attributes_after.is_empty() {
                stitch_state::attributes(&session)?
            } else {
                attributes_after
            },
        );
        let (state, values) = match stitch_outcome {
            Some((stitched, lamina, vertex_gap, edge_gap)) => (
                vec![1.0, stitched as f64, lamina as f64],
                vec![vertex_gap, edge_gap],
            ),
            None => (vec![0.0], Vec::new()),
        };
        section(&mut out, &state);
        section(&mut out, &values);
    }
    if input.checkpoint == Checkpoint::ShrunkFaces {
        for (state, values) in [&shrink_before, &shrink_after] {
            section(&mut out, state);
            section(&mut out, values);
        }
    }
    Ok(out)
}

pub(super) fn output(mut values: &[f64], checkpoint: Checkpoint) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    out.values.insert(
        "selected_edge_before".into(),
        take_section(&mut values)?.to_vec(),
    );
    let count = checkpoint.status_count();
    let statuses = values
        .get(..count)
        .ok_or_else(|| HarnessError::new("missing prefix statuses"))?;
    if statuses.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
        return Err(HarnessError::new("invalid prefix statuses"));
    }
    out.ints.insert(
        "prefix_statuses".into(),
        statuses.iter().map(|v| *v as i64).collect(),
    );
    values = values
        .get(count..)
        .ok_or_else(|| HarnessError::new("missing prefix state"))?;
    for key in ["before_direct_topology", "prefix_state"] {
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid prefix state"));
        }
        out.ints
            .insert(key.into(), state.iter().map(|v| *v as i64).collect());
    }
    out.values.insert(
        "actual_solver_corner_parameters".into(),
        take_section(&mut values)?.to_vec(),
    );
    if checkpoint == Checkpoint::FirstVertexGeometry {
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid vertex geometry state"));
        }
        out.ints.insert(
            "calculated_vertex_state".into(),
            state.iter().map(|v| *v as i64).collect(),
        );
        out.values.insert(
            "calculated_vertices".into(),
            take_section(&mut values)?.to_vec(),
        );
    }
    if checkpoint.all_corners() {
        for key in ["corner_calculation_flags", "all_corner_vertex_state"] {
            let state = take_section(&mut values)?;
            if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
                return Err(HarnessError::new("invalid all-corner state"));
            }
            out.ints
                .insert(key.into(), state.iter().map(|v| *v as i64).collect());
        }
        out.values.insert(
            "all_corner_vertices".into(),
            take_section(&mut values)?.to_vec(),
        );
    }
    if checkpoint.has_offsets() {
        for phase in ["before", "after"] {
            let state = take_section(&mut values)?;
            if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
                return Err(HarnessError::new("invalid offset ownership state"));
            }
            out.ints.insert(
                format!("offsets_{phase}_state"),
                state.iter().map(|v| *v as i64).collect(),
            );
            out.values.insert(
                format!("offsets_{phase}_distances"),
                take_section(&mut values)?.to_vec(),
            );
        }
    }
    if checkpoint.code() >= 6 {
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid adjustment flags"));
        }
        out.ints.insert(
            "corner_adjustment_flags".into(),
            state.iter().map(|v| *v as i64).collect(),
        );
    }
    if checkpoint.code() >= 7 {
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid generated geometry state"));
        }
        out.ints.insert(
            "generated_geometry_state".into(),
            state.iter().map(|v| *v as i64).collect(),
        );
        out.values.insert(
            "generated_geometry_samples".into(),
            take_section(&mut values)?.to_vec(),
        );
    }
    if checkpoint.code() >= 8 {
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid corner edge state"));
        }
        out.ints.insert(
            "corner_edge_geometry_state".into(),
            state.iter().map(|v| *v as i64).collect(),
        );
        out.values.insert(
            "corner_edge_geometry_samples".into(),
            take_section(&mut values)?.to_vec(),
        );
    }
    if checkpoint.code() >= 9 {
        for phase in ["before", "after"] {
            let state = take_section(&mut values)?;
            if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
                return Err(HarnessError::new("invalid rail trim state"));
            }
            out.ints.insert(
                format!("rails_{phase}_trim_state"),
                state.iter().map(|v| *v as i64).collect(),
            );
            out.values.insert(
                format!("rails_{phase}_trim_geometry"),
                take_section(&mut values)?.to_vec(),
            );
        }
    }
    if checkpoint.code() >= 10 {
        for phase in ["before", "after"] {
            let state = take_section(&mut values)?;
            if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
                return Err(HarnessError::new("invalid classification state"));
            }
            out.ints.insert(
                format!("rails_{phase}_classification_state"),
                state.iter().map(|v| *v as i64).collect(),
            );
            out.values.insert(
                format!("rails_{phase}_classification_parameters"),
                take_section(&mut values)?.to_vec(),
            );
        }
        let ids = take_section(&mut values)?;
        if ids
            .iter()
            .any(|v| !v.is_finite() || v.fract() != 0.0 || *v < 0.0)
        {
            return Err(HarnessError::new("invalid adjusted-corner labels"));
        }
        out.ints.insert(
            "classification_adjusted_corners".into(),
            ids.iter().map(|v| *v as i64).collect(),
        );
    }
    if checkpoint.code() >= 11 {
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid intermediate topology state"));
        }
        out.ints.insert(
            "intermediate_face_topology".into(),
            state.iter().map(|v| *v as i64).collect(),
        );
        out.values.insert(
            "intermediate_face_geometry".into(),
            take_section(&mut values)?.to_vec(),
        );
        let state = take_section(&mut values)?;
        if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            return Err(HarnessError::new("invalid source surface binding"));
        }
        out.ints.insert(
            "source_surface_binding".into(),
            state.iter().map(|v| *v as i64).collect(),
        );
    }
    if checkpoint.code() >= 12 {
        for name in [
            "intermediate_attributes_before_stitch",
            "intermediate_attributes_after_stitch",
            "intermediate_stitch_outcome",
        ] {
            let state = take_section(&mut values)?;
            if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
                return Err(HarnessError::new("invalid stitch state"));
            }
            out.ints
                .insert(name.into(), state.iter().map(|v| *v as i64).collect());
        }
        out.values.insert(
            "intermediate_stitch_gaps".into(),
            take_section(&mut values)?.to_vec(),
        );
    }
    if checkpoint == Checkpoint::ShrunkFaces {
        for phase in ["before", "after"] {
            let state = take_section(&mut values)?;
            if state.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
                return Err(HarnessError::new("invalid shrink state"));
            }
            out.ints.insert(
                format!("intermediate_shrink_{phase}"),
                state.iter().map(|v| *v as i64).collect(),
            );
            out.values.insert(
                format!("intermediate_shrink_{phase}"),
                take_section(&mut values)?.to_vec(),
            );
        }
    }
    if !values.is_empty() {
        return Err(HarnessError::new("trailing prefix data"));
    }
    Ok(out)
}
pub(super) fn cpp(input: &RealSolid) -> Result<Output, HarnessError> {
    let values = ffi::real_prefix(&input.wire(), input.checkpoint.code())
        .map_err(|e| HarnessError::new(e.to_string()))?;
    output(&values, input.checkpoint)
}
