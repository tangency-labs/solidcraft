//! Read actual generated geometry after the direct first-solver component.
use super::super::status;
use solidcore::blend::fillet_executive::session::{FilletObject, FilletSession};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3};
use solidcore::{Result, SolidError};

pub(super) fn evaluated(
    state: &mut Vec<f64>,
    values: &mut Vec<f64>,
    result: Result<Point3>,
) -> Result<()> {
    match result {
        Ok(point) => {
            state.push(0.0);
            values.extend([point.x, point.y, point.z]);
        }
        Err(error) => state.push(status(error)?),
    }
    Ok(())
}

pub(super) fn snapshot(k: &Kernel, session: &FilletSession<'_>) -> Result<(Vec<f64>, Vec<f64>)> {
    let mut state = Vec::new();
    let mut values = Vec::new();
    for (solver_index, object) in session.executive().solvers.iter().enumerate() {
        let FilletObject::Solver(id) = *object else {
            return Err(SolidError::invalid_input(
                "harness::generated_fillet",
                "actual solver required",
            ));
        };
        let solver = session.solver(id)?.base();
        state.extend([solver_index as f64, solver.fillet_geoms.len() as f64]);
        for (index, &id) in solver.fillet_geoms.iter().enumerate() {
            let geometry = session.pseudo_brep().geom(id)?;
            let surface = session.fillet_surface(id)?;
            state.extend([index as f64, f64::from(surface.is_some())]);
            if let Some(surface) = surface {
                state.push(surface.type_code() as f64);
                let domain = surface.natural_uv_domain();
                values.extend([
                    domain.min().x,
                    domain.min().y,
                    domain.max().x,
                    domain.max().y,
                ]);
                for u in [0.17, 0.37, 0.83] {
                    for v in [0.17, 0.37, 0.83] {
                        evaluated(
                            &mut state,
                            &mut values,
                            surface.evaluate_point(
                                k,
                                Point2::new(
                                    domain.u_interval().evaluate(u),
                                    domain.v_interval().evaluate(v),
                                ),
                            ),
                        )?;
                    }
                }
            }
            state.push(f64::from(geometry.center_line_geometry().is_some()));
            if let Some(curve) = geometry.center_line_geometry() {
                state.push(curve.type_code() as f64);
                let interval = curve.natural_interval();
                values.extend([interval.min(), interval.max()]);
                for fraction in [0.17, 0.37, 0.83] {
                    evaluated(
                        &mut state,
                        &mut values,
                        curve.evaluate_point(k, interval.evaluate(fraction)),
                    )?;
                }
            }
            for rail in 0..2 {
                let edge = geometry
                    .rail(rail)
                    .map(|id| session.pseudo_brep().brep().edge(id))
                    .transpose()?;
                state.extend([rail as f64, f64::from(edge.is_some())]);
                if let Some(edge) = edge {
                    state.push(f64::from(edge.curve.is_some()));
                    if let Some(curve) = edge.curve.as_ref() {
                        state.push(curve.type_code() as f64);
                        let interval = curve.natural_interval();
                        values.extend([
                            interval.min(),
                            interval.max(),
                            edge.interval.min(),
                            edge.interval.max(),
                        ]);
                        for fraction in [0.17, 0.37, 0.83] {
                            evaluated(
                                &mut state,
                                &mut values,
                                curve.evaluate_point(k, interval.evaluate(fraction)),
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok((state, values))
}
