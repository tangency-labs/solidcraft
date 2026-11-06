//! Actual ordered pseudo-edge mappings into the managed body, without evaluations.
use super::super::label;
use super::{FilletSession, SolidResult};
use solidcore::blend::fillet_executive::session::{FilletBody, FilletObject};
use solidcore::topology::{brep::p1, EdgeId};
use solidcore::EntityRef;

fn mapped(edges: &[EdgeId], value: Option<EdgeId>) -> SolidResult<f64> {
    let index = label(edges, value);
    if index < -1.0 {
        return Err(super::invalid("mapped edge is outside actual managed body"));
    }
    Ok(index)
}

pub(super) fn snapshot(session: &FilletSession<'_>) -> SolidResult<(Vec<f64>, Vec<f64>)> {
    let pseudo = session.pseudo_brep();
    let edges = p1::edges(pseudo.brep(), None)?;
    let managed = session
        .intermediate_brep()
        .map(|b| p1::edges(b, None))
        .transpose()?
        .unwrap_or_default();
    let target_edges = p1::edges(session.target(), None)?;
    let target_vertices = p1::vertices(session.target(), None)?;
    let error = &session.executive().error;
    let error_edge = match error.edge {
        None => -1.0,
        Some(FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id))) => {
            label(&target_edges, Some(id))
        }
        _ => return Err(super::invalid("error edge is not actual target edge")),
    };
    let error_vertex = match error.vertex {
        None => -1.0,
        Some(FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(id))) => {
            label(&target_vertices, Some(id))
        }
        _ => return Err(super::invalid("error vertex is not actual target vertex")),
    };
    let mut state = vec![
        1.0,
        edges.len() as f64,
        f64::from(error.code),
        error_edge,
        error_vertex,
        error.comment.len() as f64,
    ];
    state.extend(error.comment.bytes().map(f64::from));
    let mut values = Vec::new();
    for (index, id) in edges.into_iter().enumerate() {
        let fe = pseudo.fillet_edge(id)?;
        let raw = pseudo.brep().edge(id)?.interval;
        values.extend([raw.min(), raw.max()]);
        let maps = fe.fillet_brep_edges();
        state.extend([
            index as f64,
            f64::from(fe.fillet_edge_type() as i32),
            f64::from(fe.status() as i32),
            mapped(&managed, fe.fillet_brep_edge())?,
            maps.len() as f64,
        ]);
        for map in maps {
            state.push(mapped(&managed, Some(map))?);
        }
    }
    Ok((state, values))
}
