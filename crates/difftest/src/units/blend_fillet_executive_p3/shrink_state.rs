//! Passive domains, raw intervals and UV presence around direct first-FG shrink.
use super::super::label;
use super::{FilletSession, SolidResult};
use solidcore::blend::fillet_executive::session::FilletObject;
use solidcore::topology::{brep::p1, edge, edgeuse::p1 as eu};
use solidcore::EntityRef;

pub(super) fn snapshot(session: &FilletSession<'_>) -> SolidResult<(Vec<f64>, Vec<f64>)> {
    let mut state = vec![session.executive().solvers.len() as f64];
    let mut values = Vec::new();
    let body = session.intermediate_brep();
    let faces = body
        .map(|b| p1::faces(b, None))
        .transpose()?
        .unwrap_or_default();
    for object in &session.executive().solvers {
        let FilletObject::Solver(id) = *object else {
            return Err(super::invalid("actual solver required"));
        };
        let solver = session.solver(id)?.base();
        state.push(solver.fillet_geoms.len() as f64);
        let surface = solver
            .fillet_geoms
            .first()
            .map(|&g| session.fillet_surface(g))
            .transpose()?
            .flatten();
        state.push(f64::from(surface.is_some()));
        if let Some(surface) = surface {
            let owner = surface.owner();
            let mut face = None;
            if let Some(owner) = owner {
                for &f in &faces {
                    if session.intermediate_identity(EntityRef::Face(f))? == owner {
                        face = Some(f);
                        break;
                    }
                }
            }
            state.extend([
                surface.type_code() as f64,
                f64::from(owner.is_some()),
                owner.map_or(0.0, |o| o.type_code as f64),
                label(&faces, face),
            ]);
            let domain = surface.natural_uv_domain();
            values.extend([
                domain.min().x,
                domain.min().y,
                domain.max().x,
                domain.max().y,
            ]);
            if let (Some(body), Some(face)) = (body, face) {
                let d = body.face(face)?.uv_domain;
                state.push(1.0);
                values.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
            } else {
                state.push(0.0);
            }
        }
    }
    let edges = body
        .map(|b| p1::edges(b, None))
        .transpose()?
        .unwrap_or_default();
    state.push(edges.len() as f64);
    if let Some(body) = body {
        for id in edges {
            let e = body.edge(id)?;
            values.extend([e.interval.min(), e.interval.max()]);
            state.push(f64::from(e.curve.is_some()));
            if let Some(c) = e.curve.as_ref() {
                state.push(c.type_code() as f64);
                let d = c.natural_interval();
                values.extend([d.min(), d.max()]);
            }
            let uses = edge::edgeuses(body, id, None, None)?;
            state.push(uses.len() as f64);
            for id in uses {
                state.push(f64::from(eu::uv_trim_curve_pointer(body, id)?.is_some()));
            }
        }
    }
    Ok((state, values))
}
