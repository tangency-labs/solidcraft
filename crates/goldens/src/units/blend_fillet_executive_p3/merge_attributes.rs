//! Actual cross-body attribute aliases and ordered users after body copying.
use super::{invalid, FilletObject, FilletSession, SolidResult};
use solidcore::geom::attribute::{ids, AttributeHandle, ObjectKey};
use solidcore::topology::brep::p1;
use solidcore::EntityRef;

fn users(
    session: &FilletSession<'_>,
    handle: AttributeHandle,
    keys: &[Vec<ObjectKey>],
    out: &mut Vec<f64>,
) -> SolidResult<()> {
    let store = session.attributes();
    let attribute = store
        .get(handle)
        .ok_or_else(|| invalid("actual merge attribute missing"))?;
    let users = store.users(handle)?;
    out.extend([
        attribute.attribute_id() as f64,
        attribute.type_code() as f64,
        users.len() as f64,
    ]);
    for user in users {
        let mut matches = Vec::new();
        for (body, entities) in keys.iter().enumerate() {
            if let Some(index) = entities.iter().position(|key| *key == user) {
                matches.push(((body + 1) as f64, index as f64));
            }
        }
        if matches.len() != 1 {
            return Err(invalid(
                "merge attribute user lacks unique current body edge",
            ));
        }
        let (body, edge) = matches
            .first()
            .copied()
            .ok_or_else(|| invalid("missing merge attribute user"))?;
        out.extend([body, edge]);
    }
    Ok(())
}

pub(super) fn snapshot(session: &FilletSession<'_>) -> SolidResult<Vec<f64>> {
    let target = p1::edges(session.target(), None)?;
    let managed = session
        .intermediate_brep()
        .ok_or_else(|| invalid("actual merge body missing"))?;
    let intermediate = p1::edges(managed, None)?;
    let keys = vec![
        target
            .into_iter()
            .map(|id| session.target_identity(EntityRef::Edge(id)).key)
            .collect(),
        intermediate
            .into_iter()
            .map(|id| {
                session
                    .intermediate_identity(EntityRef::Edge(id))
                    .map(|value| value.key)
            })
            .collect::<SolidResult<Vec<_>>>()?,
    ];
    let expected = match session.executive().edge_attribute {
        Some(FilletObject::Attribute(handle)) => Some(handle),
        None => None,
        _ => return Err(invalid("merge executive attribute is not actual handle")),
    };
    let mut out = vec![f64::from(expected.is_some())];
    if let Some(handle) = expected {
        users(session, handle, &keys, &mut out)?;
    }
    let store = session.attributes();
    let mut tags = Vec::new();
    for body in &keys {
        for &key in body {
            for handle in store.attributes(key) {
                let attribute = store
                    .get(handle)
                    .ok_or_else(|| invalid("merge edge attribute not live"))?;
                if attribute.attribute_id() == ids::FILLET_EDGES && !tags.contains(&handle) {
                    tags.push(handle);
                }
            }
        }
    }
    out.push(tags.len() as f64);
    for &handle in &tags {
        users(session, handle, &keys, &mut out)?;
    }
    for body in keys {
        out.push(body.len() as f64);
        for key in body {
            let mut own = Vec::new();
            for handle in store.attributes(key) {
                let attribute = store
                    .get(handle)
                    .ok_or_else(|| invalid("merge edge attribute not live"))?;
                if matches!(
                    attribute.attribute_id(),
                    ids::FILLET_EDGES | ids::ALL_FILLET_EDGES
                ) {
                    own.push(handle);
                }
            }
            out.push(own.len() as f64);
            for handle in own {
                let attribute = store
                    .get(handle)
                    .ok_or_else(|| invalid("merge edge attribute missing"))?;
                if attribute.attribute_id() == ids::ALL_FILLET_EDGES && Some(handle) != expected {
                    return Err(invalid("unexpected unrelated executive edge attribute"));
                }
                out.extend([
                    attribute.attribute_id() as f64,
                    f64::from(Some(handle) == expected),
                    tags.iter()
                        .position(|value| *value == handle)
                        .map_or(-1.0, |index| index as f64),
                ]);
            }
        }
    }
    Ok(out)
}
