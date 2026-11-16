//! Actual executive attribute users and each intermediate edge's ordered attributes.
use super::{invalid, FilletObject, FilletSession, SolidResult};
use solidcore::geom::attribute::ids;
use solidcore::topology::brep::p1;
use solidcore::EntityRef;

pub(super) fn attributes(session: &FilletSession<'_>) -> SolidResult<Vec<f64>> {
    let Some(body) = session.intermediate_brep() else {
        return Ok(vec![0.0]);
    };
    let expected = match session.executive().edge_attribute {
        Some(FilletObject::Attribute(handle)) => Some(handle),
        None => None,
        _ => return Err(invalid("actual executive attribute required")),
    };
    let store = session.attributes();
    let attribute = expected
        .map(|id| {
            store
                .get(id)
                .ok_or_else(|| invalid("retained attribute missing"))
        })
        .transpose()?;
    let edges = p1::edges(body, None)?;
    let keys = edges
        .iter()
        .map(|&id| {
            session
                .intermediate_identity(EntityRef::Edge(id))
                .map(|object| object.key)
        })
        .collect::<SolidResult<Vec<_>>>()?;
    let users = expected
        .map(|id| store.users(id))
        .transpose()?
        .unwrap_or_default();
    let mut out = vec![
        1.0,
        f64::from(attribute.is_some()),
        attribute.map_or(0.0, |attribute| attribute.attribute_id() as f64),
        attribute.map_or(0.0, |attribute| attribute.type_code() as f64),
        users.len() as f64,
    ];
    for user in users {
        out.push(super::super::label(&keys, Some(user)));
    }
    out.push(edges.len() as f64);
    for key in keys {
        let found = store.find_attribute(key, ids::ALL_FILLET_EDGES);
        let own = store.attributes(key);
        out.extend([
            f64::from(found.is_some()),
            f64::from(found.is_some() && found == expected),
            own.len() as f64,
        ]);
        for id in own {
            let attribute = store
                .get(id)
                .ok_or_else(|| invalid("edge attribute missing"))?;
            out.extend([
                attribute.attribute_id() as f64,
                attribute.type_code() as f64,
            ]);
        }
    }
    Ok(out)
}
