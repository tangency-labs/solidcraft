//! Passive actual target/managed state around the complete insertion component.
use super::{
    edge_map_state, intermediate_faces, section, stitch_state, FilletSession, SolidResult,
};
use solidcore::blend::fillet_executive::session::{FilletBody, FilletObject};
use solidcore::kernel::Kernel;
use solidcore::topology::{brep::p1, Brep};
use solidcore::EntityRef;

fn counts(body: &Brep, out: &mut Vec<f64>) -> SolidResult<()> {
    out.extend([
        p1::vertices(body, None)?.len() as f64,
        p1::edges(body, None)?.len() as f64,
        p1::faces(body, None)?.len() as f64,
    ]);
    Ok(())
}
pub(super) fn snapshot(
    k: &Kernel,
    session: &FilletSession<'_>,
) -> SolidResult<(Vec<f64>, Vec<f64>)> {
    let target = session.target();
    let intermediate = session.intermediate_brep();
    let ti = &session.executive().intersector;
    let mut out = vec![
        f64::from(k.context.doing_boolean()),
        f64::from(ti.doing_fillets),
        f64::from(target.editing_enabled()),
        f64::from(ti.primary_brep() == Some(FilletObject::Body(FilletBody::Target))),
        f64::from(
            intermediate.is_some() && ti.other_brep() == Some(FilletObject::IntermediateBrep),
        ),
        f64::from(ti.topology_deleted),
    ];
    counts(target, &mut out)?;
    if let Some(body) = intermediate {
        counts(body, &mut out)?;
    } else {
        out.extend([0.0; 3]);
    }
    counts(session.pseudo_brep().brep(), &mut out)?;
    let (maps, intervals) = edge_map_state::snapshot(session)?;
    section(&mut out, &maps);
    section(&mut out, &intermediate_faces::source_binding(session)?);
    section(&mut out, &stitch_state::attributes(session)?);
    let faces = p1::faces(target, None)?;
    out.push(faces.len() as f64);
    for id in faces {
        let key = session.target_identity(EntityRef::Face(id)).key;
        let mut markers = Vec::new();
        for handle in session.attributes().attributes(key) {
            let attribute = session
                .attributes()
                .get(handle)
                .ok_or_else(|| super::invalid("missing actual target attribute"))?;
            if matches!(attribute.attribute_id(), 777 | 778) {
                markers.push(attribute.attribute_id() as f64);
            }
        }
        section(&mut out, &markers);
    }
    Ok((out, intervals))
}
