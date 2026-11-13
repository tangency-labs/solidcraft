//! Observe actual retained corner boundary curves after their shared calculation.
use super::super::label;
use super::generated_geometry::evaluated;
use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession};
use solidcore::kernel::Kernel;
use solidcore::{Result, SolidError};

pub(super) fn snapshot(k: &Kernel, session: &FilletSession<'_>) -> Result<(Vec<f64>, Vec<f64>)> {
    let mut state = Vec::new();
    let mut values = Vec::new();
    let pseudo = session.pseudo_brep();
    for (index, object) in session.executive().corners.iter().enumerate() {
        let FilletObject::Corner(id) = *object else {
            return Err(SolidError::invalid_input(
                "harness::corner_edges",
                "actual corner required",
            ));
        };
        let corner = session.corner(id)?;
        state.extend([index as f64, corner.edges.len() as f64]);
        for &id in &corner.edges {
            let fillet = pseudo.fillet_edge(id)?;
            let edge = pseudo.brep().edge(id)?;
            state.extend([
                label(pseudo.edge_list(), Some(id)),
                f64::from(fillet.fillet_edge_type() as u32),
                f64::from(fillet.status as u32),
                f64::from(edge.curve.is_some()),
            ]);
            if let Some(curve) = edge.curve.as_ref() {
                state.push(curve.type_code() as f64);
                let interval = curve.natural_interval();
                values.extend([
                    interval.min(),
                    interval.max(),
                    edge.interval.min(),
                    edge.interval.max(),
                ]);
                for fraction in [0.0, 0.17, 0.37, 0.83, 1.0] {
                    evaluated(
                        &mut state,
                        &mut values,
                        curve.evaluate_point(k, interval.evaluate(fraction)),
                    )?;
                }
            }
        }
    }
    Ok((state, values))
}
