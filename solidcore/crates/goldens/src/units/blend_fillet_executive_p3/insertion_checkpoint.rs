//! One call to the actual retained insertion endpoint under its production scope.
use super::{
    insertion_state, intermediate_faces, original_deletion, original_merge, original_selection,
    section, status, Checkpoint, FilletSession, RealSolid, SolidResult,
};
use solidcore::kernel::Kernel;
use solidcore::topology::{Brep, EdgeId};

pub(super) fn run(
    k: &mut Kernel,
    body: &mut Brep,
    edges: &[EdgeId],
    input: &RealSolid,
    before: &[f64],
) -> SolidResult<Vec<f64>> {
    let mut session = FilletSession::new(k, body)?;
    let mut statuses = [-1.0; 5];
    statuses[0] = match session.prepare_constant_radius_edges(
        k,
        edges,
        input.radius,
        input.xsect,
        input.continuity,
        input.thumbweight,
    ) {
        Ok(()) => 0.0,
        Err(error) => status(error)?,
    };
    if statuses[0] == 0.0 {
        statuses[1] = match session.create_corners(k, None) {
            Ok(()) => 0.0,
            Err(error) => status(error)?,
        };
    }
    let pack = |k: &mut Kernel,
                session: &FilletSession<'_>,
                statuses: &[f64],
                prior: &(Vec<f64>, Vec<f64>),
                after: &(Vec<f64>, Vec<f64>),
                complete: bool,
                selection: &[f64],
                selection_intervals: &[f64]|
     -> SolidResult<Vec<f64>> {
        let mut out = Vec::new();
        section(&mut out, before);
        section(&mut out, statuses);
        for (state, values) in [prior, after] {
            section(&mut out, state);
            section(&mut out, values);
        }
        out.push(f64::from(complete));
        if complete {
            let target = intermediate_faces::body_snapshot(k, session.target())?;
            let managed = intermediate_faces::snapshot(k, session)?;
            for (state, values) in [&target, &managed] {
                section(&mut out, state);
                section(&mut out, values);
            }
        }
        if input.checkpoint == Checkpoint::SelectedOriginalSurfaces {
            section(&mut out, selection);
        }
        if matches!(
            input.checkpoint,
            Checkpoint::DeletedOriginalFaces | Checkpoint::MergedFilletFaces
        ) {
            section(&mut out, selection);
            section(&mut out, selection_intervals);
        }
        Ok(out)
    };
    if statuses[1] != 0.0 {
        return pack(
            k,
            &session,
            &statuses,
            &(Vec::new(), Vec::new()),
            &(Vec::new(), Vec::new()),
            false,
            &[],
            &[],
        );
    }
    session.with_filleting_context(k, |session, k| {
        statuses[2] = match session.create_fillet_brep(k, None) {
            Ok(()) => 0.0,
            Err(error) => status(error)?,
        };
        let mut prior = (Vec::new(), Vec::new());
        let mut after = (Vec::new(), Vec::new());
        if statuses[2] == 0.0 {
            statuses[3] = match session.shrink_fillet_faces(k) {
                Ok(()) => 0.0,
                Err(error) => status(error)?,
            };
            if statuses[3] == 0.0 {
                prior = insertion_state::snapshot(k, session)?;
                statuses[4] = match session.insert_fillet_intersection_topology(k) {
                    Ok(()) => 0.0,
                    Err(error) => status(error)?,
                };
                after = insertion_state::snapshot(k, session)?;
            }
        }
        let (selection, intervals, complete) = if statuses[4] == 0.0
            && input.checkpoint == Checkpoint::MergedFilletFaces
        {
            original_merge::run(k, session)?
        } else if statuses[4] == 0.0 && input.checkpoint == Checkpoint::DeletedOriginalFaces {
            original_deletion::run(k, session)?
        } else if statuses[4] == 0.0 && input.checkpoint == Checkpoint::SelectedOriginalSurfaces {
            let (selection, complete) = original_selection::run(k, session)?;
            (selection, Vec::new(), complete)
        } else {
            (Vec::new(), Vec::new(), statuses[4] == 0.0)
        };
        pack(
            k, session, &statuses, &prior, &after, complete, &selection, &intervals,
        )
    })
}
