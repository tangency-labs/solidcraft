//! Actual retained deletion attempt; removed identities are labeled before use.
use super::{
    insertion_state, invalid, original_selection, section, status, FilletSession, SolidResult,
};
use solidcore::blend::fillet_executive::session::{
    FilletBody, FilletObject, OriginalSurfaceDeletion,
};
use solidcore::geom::context::MarkType;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::{p1, p3::topology_ref};
use solidcore::topology::Brep;
use solidcore::EntityRef;

pub(super) struct Frame {
    pub(super) entities: [Vec<EntityRef>; 3],
    pub(super) marks: [Vec<u64>; 3],
}
pub(super) fn entities(body: &Brep) -> SolidResult<[Vec<EntityRef>; 3]> {
    Ok([
        p1::vertices(body, None)?
            .into_iter()
            .map(EntityRef::Vertex)
            .collect(),
        p1::edges(body, None)?
            .into_iter()
            .map(EntityRef::Edge)
            .collect(),
        p1::faces(body, None)?
            .into_iter()
            .map(EntityRef::Face)
            .collect(),
    ])
}
pub(super) fn frames(session: &mut FilletSession<'_>) -> SolidResult<Vec<Frame>> {
    let state = session.insertion_state()?;
    let managed = state
        .intermediate
        .notify_serial()
        .ok_or_else(|| invalid("deletion managed identity missing"))?;
    let mut frames = Vec::new();
    for (body, serial) in [
        (&*state.target, state.target_serial),
        (&*state.intermediate, managed),
    ] {
        let entities = entities(body)?;
        let mut marks = [Vec::new(), Vec::new(), Vec::new()];
        for (entities, marks) in entities.iter().zip(&mut marks) {
            for entity in entities {
                let record = state
                    .notification_records
                    .get(&topology_ref(serial, *entity).key)
                    .ok_or_else(|| invalid("predeletion actual record missing"))?;
                marks.push(record.get_mark(MarkType::Mark));
            }
        }
        frames.push(Frame { entities, marks });
    }
    Ok(frames)
}
pub(super) fn face_labels(values: &[FilletObject], frames: &[Frame]) -> SolidResult<Vec<f64>> {
    let mut out = Vec::new();
    for object in values {
        let (body, entity) = match object {
            FilletObject::Topology(FilletBody::Target, entity @ EntityRef::Face(_)) => (0, *entity),
            FilletObject::IntermediateTopology(entity @ EntityRef::Face(_)) => (1, *entity),
            _ => return Err(invalid("deletion output lacks actual body-qualified face")),
        };
        let faces = frames
            .get(body)
            .and_then(|f| f.entities.get(2))
            .ok_or_else(|| invalid("missing deletion input frame"))?;
        let index = faces
            .iter()
            .position(|value| *value == entity)
            .ok_or_else(|| invalid("deletion output not in pre-call owning traversal"))?;
        out.extend([(body + 1) as f64, index as f64]);
    }
    Ok(out)
}

pub(super) fn run(
    k: &mut Kernel,
    session: &mut FilletSession<'_>,
) -> SolidResult<(Vec<f64>, Vec<f64>, bool)> {
    let mut out = vec![f64::from(session.executive().options.global_merge)];
    let epochs = original_selection::MARK_TYPES.map(|mark| k.context.current_mark(mark));
    original_selection::context(k, &mut out, None)?;
    let before = frames(session)?;
    let mut deleted = OriginalSurfaceDeletion::default();
    let result = session.trim_and_delete_original_fillet_faces(k, &mut deleted);
    let complete = result.is_ok();
    out.push(match result {
        Ok(()) => 0.0,
        Err(error) => status(error)?,
    });
    original_selection::context(k, &mut out, Some(&epochs))?;
    out.extend([
        f64::from(k.context.doing_boolean()),
        f64::from(session.target().editing_enabled()),
        f64::from(session.executive().intersector.doing_fillets),
        f64::from(session.executive().intersector.topology_deleted),
    ]);
    // Identity-only lookups in pre-call arrays: never dereference removed faces.
    section(&mut out, &face_labels(&deleted.kept, &before)?);
    section(&mut out, &face_labels(&deleted.deleted, &before)?);
    {
        let state = session.insertion_state()?;
        let serial = state
            .intermediate
            .notify_serial()
            .ok_or_else(|| invalid("managed deletion identity missing"))?;
        for (body_index, (body, serial)) in [
            (&*state.target, state.target_serial),
            (&*state.intermediate, serial),
        ]
        .into_iter()
        .enumerate()
        {
            let frame = before
                .get(body_index)
                .ok_or_else(|| invalid("missing actual before frame"))?;
            for (kind, entities) in entities(body)?.into_iter().enumerate() {
                let previous = frame
                    .entities
                    .get(kind)
                    .ok_or_else(|| invalid("missing actual pre-call kind"))?;
                let previous_marks = frame
                    .marks
                    .get(kind)
                    .ok_or_else(|| invalid("missing actual pre-call marks"))?;
                let mut marks = vec![entities.len() as f64];
                for entity in entities {
                    let old = previous
                        .iter()
                        .position(|value| *value == entity)
                        .ok_or_else(|| invalid("deletion created unexpected owning topology"))?;
                    let raw_before = previous_marks
                        .get(old)
                        .ok_or_else(|| invalid("missing pre-call mark value"))?;
                    let record = state
                        .notification_records
                        .get(&topology_ref(serial, entity).key)
                        .ok_or_else(|| invalid("postdeletion actual record missing"))?;
                    let raw = record.get_mark(MarkType::Mark);
                    marks.extend([
                        old as f64,
                        f64::from(record.is_marked(&k.context, MarkType::Mark)),
                        f64::from(
                            k.context.current_mark(MarkType::Mark).checked_sub(1) == Some(raw),
                        ),
                        f64::from(*raw_before == raw),
                    ]);
                }
                section(&mut out, &marks);
            }
        }
    }
    // Do not dereference a managed FG source after its owning face was retired.
    // The ordinary-box observation requires an actual surviving source binding.
    let managed = session
        .intermediate_brep()
        .ok_or_else(|| invalid("missing deletion managed body"))?;
    let live_faces = p1::faces(managed, None)?;
    for object in &session.executive().solvers {
        let FilletObject::Solver(id) = *object else {
            return Err(invalid("actual deletion solver required"));
        };
        for &geometry in &session.solver(id)?.base().fillet_geoms {
            if let Some(surface) = session.fillet_surface(geometry)? {
                let mut live = false;
                for &face in &live_faces {
                    if managed
                        .face(face)?
                        .surface
                        .as_ref()
                        .is_some_and(|value| std::ptr::eq(value, surface))
                    {
                        live = true;
                    }
                }
                if !live {
                    return Err(invalid(
                        "post-deletion FG source not in surviving managed faces",
                    ));
                }
            }
        }
    }
    let (after, intervals) = insertion_state::snapshot(k, session)?;
    section(&mut out, &after);
    Ok((out, intervals, complete))
}
