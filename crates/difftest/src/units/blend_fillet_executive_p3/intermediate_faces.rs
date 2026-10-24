//! Actual intermediate body: geometry plus directed topology in owning-list order.
use super::super::{label, topology};
use super::{FilletSession, SolidResult};
use solidcore::kernel::Kernel;
use solidcore::topology::{brep::p1, edge, edgeuse::p1 as eu, face::p2 as face};

pub(super) fn snapshot(
    k: &mut Kernel,
    session: &FilletSession<'_>,
) -> SolidResult<(Vec<f64>, Vec<f64>)> {
    let Some(body) = session.intermediate_brep() else {
        return Ok((vec![0.0], Vec::new()));
    };
    let vertices = p1::vertices(body, None)?;
    let edges = p1::edges(body, None)?;
    let faces = p1::faces(body, None)?;
    let regions = p1::regions(body, None)?;
    let shells = p1::shells(k, body, None)?;
    let mut uses = Vec::new();
    for &id in &edges {
        uses.extend(edge::edgeuses(body, id, None, None)?);
    }
    let mut state = vec![
        1.0,
        vertices.len() as f64,
        edges.len() as f64,
        faces.len() as f64,
        regions.len() as f64,
        shells.len() as f64,
        f64::from(body.editing_enabled()),
    ];
    topology(body, &mut state)?;
    state.push(uses.len() as f64);
    for &id in &uses {
        let record = body.edgeuse(id)?;
        let vertex = record
            .vertexuse
            .map(|id| body.vertexuse(id).map(|vu| vu.vertex))
            .transpose()?
            .flatten();
        state.extend([
            label(&edges, record.edge),
            f64::from(record.orientation as i32),
            label(&vertices, vertex),
            label(&uses, eu::mate(body, id)?),
            label(&uses, eu::radial(body, id)?),
            label(&uses, eu::cw_edgeuse(body, id)?),
            label(&uses, eu::ccw_edgeuse(body, id)?),
            label(&faces, eu::face(body, id)?),
        ]);
    }
    for id in faces {
        let (a, b) = face::faceuses(body, id)?;
        for id in [a, b] {
            state.push(f64::from(id.is_some()));
            if let Some(id) = id {
                let use_ = body.faceuse(id)?;
                state.extend([
                    f64::from(use_.orientation as i32),
                    label(&shells, use_.shell),
                    label(&regions, eu::faceuse_region(body, id)?),
                ]);
            }
        }
    }
    for id in shells {
        state.push(label(&regions, body.shell(id)?.region));
    }
    for id in regions {
        state.push(f64::from(body.region(id)?.is_void));
    }
    let mut geometry = vec![body.tolerance()];
    crate::util::primitive::append(k, body, &mut geometry)?;
    Ok((state, geometry))
}

/// Passive relationship between the original FG surface and the managed face.
pub(super) fn source_binding(session: &FilletSession<'_>) -> SolidResult<Vec<f64>> {
    use solidcore::blend::fillet_executive::session::FilletObject;
    let body = session.intermediate_brep();
    let faces = body
        .map(|body| p1::faces(body, None))
        .transpose()?
        .unwrap_or_default();
    let mut out = vec![session.executive().solvers.len() as f64];
    for object in &session.executive().solvers {
        let FilletObject::Solver(id) = *object else {
            return Err(super::invalid("actual solver required"));
        };
        let solver = session.solver(id)?.base();
        out.push(solver.fillet_geoms.len() as f64);
        for &id in &solver.fillet_geoms {
            let surface = session.fillet_surface(id)?;
            let owner = surface.and_then(solidcore::surface::surface::Surface::owner);
            let mut owner_face = if owner.is_some() { -2.0 } else { -1.0 };
            let mut alias_face = if surface.is_some() { -2.0 } else { -1.0 };
            if let Some(body) = body {
                for (index, &id) in faces.iter().enumerate() {
                    if let Some(face_surface) = body.face(id)?.surface.as_ref() {
                        let face_ref =
                            session.intermediate_identity(solidcore::EntityRef::Face(id))?;
                        if owner == Some(face_ref) {
                            owner_face = index as f64;
                        }
                        if surface.is_some_and(|surface| std::ptr::eq(surface, face_surface)) {
                            alias_face = index as f64;
                        }
                    }
                }
            }
            out.extend([
                f64::from(surface.is_some()),
                f64::from(owner.is_some()),
                owner.map_or(0.0, |owner| owner.type_code as f64),
                owner_face,
                alias_face,
            ]);
        }
    }
    Ok(out)
}
