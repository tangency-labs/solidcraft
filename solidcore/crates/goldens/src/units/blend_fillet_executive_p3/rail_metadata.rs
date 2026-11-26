//! Actual rail intervals and each directed use's own UV curve, without lazy lookup.
use super::super::{label, status};
use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession};
use solidcore::kernel::Kernel;
use solidcore::topology::edge;
use solidcore::{Result, SolidError};

pub(super) fn snapshot(
    k: &Kernel,
    session: &FilletSession<'_>,
    evaluate_uv: bool,
) -> Result<(Vec<f64>, Vec<f64>)> {
    let mut state = vec![f64::from(session.executive().options.classification)];
    let mut values = Vec::new();
    let pseudo = session.pseudo_brep();
    for (index, object) in session.executive().solvers.iter().enumerate() {
        let FilletObject::Solver(id) = *object else {
            return Err(SolidError::invalid_input(
                "harness::rail_trim",
                "actual solver required",
            ));
        };
        let solver = session.solver(id)?.base();
        state.extend([index as f64, solver.fillet_geoms.len() as f64]);
        for (index, &id) in solver.fillet_geoms.iter().enumerate() {
            let geometry = pseudo.geom(id)?;
            state.push(index as f64);
            for rail in 0..2 {
                let id = geometry.rail(rail);
                state.extend([rail as f64, f64::from(id.is_some())]);
                let Some(id) = id else {
                    continue;
                };
                let record = pseudo.brep().edge(id)?;
                state.extend([
                    label(pseudo.edge_list(), Some(id)),
                    f64::from(record.curve.is_some()),
                ]);
                if let Some(curve) = record.curve.as_ref() {
                    state.push(curve.type_code() as f64);
                    let interval = curve.natural_interval();
                    values.extend([
                        interval.min(),
                        interval.max(),
                        record.interval.min(),
                        record.interval.max(),
                    ]);
                }
                let uses = edge::edgeuses(pseudo.brep(), id, None, None)?;
                state.push(uses.len() as f64);
                for id in uses {
                    let use_ = pseudo.brep().edgeuse(id)?;
                    let curve = use_.uv_trim_curve.as_ref();
                    state.extend([
                        f64::from(use_.orientation as i32),
                        f64::from(curve.is_some()),
                    ]);
                    if let Some(curve) = curve {
                        state.extend([curve.type_code() as f64, curve.dim() as f64]);
                        let interval = curve.natural_interval();
                        values.extend([interval.min(), interval.max()]);
                        if evaluate_uv {
                            for fraction in [0.17, 0.37, 0.83] {
                                match curve.evaluate_point(k, interval.evaluate(fraction)) {
                                    Ok(point) => {
                                        state.push(0.0);
                                        values.extend([point.x, point.y]);
                                    }
                                    Err(error) => state.push(status(error)?),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok((state, values))
}
