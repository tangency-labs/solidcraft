//! Attached classification state without geometry evaluation or cache warming.
use super::{invalid, FilletObject, FilletSession, SolidResult};
use solidcore::blend::fillet_classification::{FilletBody, FilletClassObject};
use solidcore::intersect::point_class::PointClassification;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, Brep};
use solidcore::EntityRef;

fn object(
    value: Option<FilletClassObject>,
    target: &Brep,
    pseudo: &Brep,
    out: &mut Vec<f64>,
) -> SolidResult<()> {
    let Some(value) = value else {
        out.extend([0.0, 0.0, -1.0]);
        return Ok(());
    };
    let FilletClassObject::Topology(body, entity) = value else {
        // Preserve an unexpected role instead of guessing a topology owner.
        out.extend([-1.0, -1.0, -2.0]);
        return Ok(());
    };
    let brep = match body {
        FilletBody::Target => target,
        FilletBody::Pseudo => pseudo,
    };
    let (kind, label) = match entity {
        EntityRef::Vertex(id) => (
            1.0,
            super::super::label(&p1::vertices(brep, None)?, Some(id)),
        ),
        EntityRef::Edge(id) => (2.0, super::super::label(&p1::edges(brep, None)?, Some(id))),
        EntityRef::Face(id) => (3.0, super::super::label(&p1::faces(brep, None)?, Some(id))),
        EntityRef::Edgeuse(id) => {
            let mut uses = Vec::new();
            for edge in p1::edges(brep, None)? {
                uses.extend(edge::edgeuses(brep, edge, None, None)?);
            }
            (4.0, super::super::label(&uses, Some(id)))
        }
        _ => (-1.0, -2.0),
    };
    out.extend([
        match body {
            FilletBody::Target => 1.0,
            FilletBody::Pseudo => 2.0,
        },
        kind,
        label,
    ]);
    Ok(())
}
fn point(
    point: &PointClassification<FilletClassObject>,
    target: &Brep,
    pseudo: &Brep,
    state: &mut Vec<f64>,
    values: &mut Vec<f64>,
) -> SolidResult<()> {
    state.push(f64::from(point.point_class() as i32));
    object(point.object(), target, pseudo, state)?;
    object(point.face(), target, pseudo, state)?;
    state.push(f64::from(point.are_parameters_set()));
    let uv = point.uv_param();
    let uvw = point.uvw_param();
    values.extend([
        point.t_param(),
        uv.x,
        uv.y,
        uvw.x,
        uvw.y,
        uvw.z,
        point.pre_snap_param(),
    ]);
    Ok(())
}
pub(super) fn snapshot(
    k: &Kernel,
    session: &FilletSession<'_>,
) -> SolidResult<(Vec<f64>, Vec<f64>)> {
    let target = session.target();
    let pseudo = session.pseudo_brep();
    let mut state = vec![
        f64::from(session.executive().options.classification),
        f64::from(k.context.doing_boolean()),
    ];
    let mut values = Vec::new();
    for object_id in &session.executive().solvers {
        let FilletObject::Solver(id) = *object_id else {
            return Err(invalid("actual solver required"));
        };
        for &geometry in &session.solver(id)?.base().fillet_geoms {
            let geometry = pseudo.geom(geometry)?;
            for rail_index in 0..2 {
                let rail_id = geometry
                    .rail(rail_index)
                    .ok_or_else(|| invalid("actual rail required"))?;
                let rail = pseudo.fillet_edge(rail_id)?;
                state.extend([
                    super::super::label(pseudo.edge_list(), Some(rail_id)),
                    f64::from(rail.curve_class().is_some()),
                ]);
                let Some(cc) = rail.curve_class() else {
                    continue;
                };
                let curve = pseudo.brep().edge(rail_id)?.curve.as_ref();
                let primary =
                    solidcore::topology::edgeuse::p1::primary_edgeuse(pseudo.brep(), rail_id)?;
                let mate = primary
                    .map(|id| solidcore::topology::edgeuse::p1::mate(pseudo.brep(), id))
                    .transpose()?
                    .flatten();
                let mate_uv = mate.and_then(|id| {
                    pseudo
                        .brep()
                        .edgeuse(id)
                        .ok()?
                        .uv_trim_curve
                        .as_ref()
                        .map(|_| FilletClassObject::TrimCurve(FilletBody::Pseudo, id))
                });
                let face = rail.original_face();
                let expected_surface = face.and_then(|id| {
                    target
                        .face(id)
                        .ok()?
                        .surface
                        .as_ref()
                        .map(|_| FilletClassObject::Surface(FilletBody::Target, id))
                });
                let source_curve = match cc.curve() {
                    Some(FilletClassObject::Curve(body, id)) => match body {
                        FilletBody::Target => target.edge(id)?.curve.as_ref(),
                        FilletBody::Pseudo => pseudo.brep().edge(id)?.curve.as_ref(),
                    },
                    Some(FilletClassObject::TrimCurve(body, id)) => match body {
                        FilletBody::Target => target.edgeuse(id)?.uv_trim_curve.as_ref(),
                        FilletBody::Pseudo => pseudo.brep().edgeuse(id)?.uv_trim_curve.as_ref(),
                    },
                    _ => None,
                };
                state.extend([
                    f64::from(cc.curve().is_some()),
                    f64::from(
                        cc.curve() == Some(FilletClassObject::Curve(FilletBody::Pseudo, rail_id))
                            && curve.is_some(),
                    ),
                    source_curve.map_or(0.0, |curve| curve.type_code() as f64),
                ]);
                object(cc.face(), target, pseudo.brep(), &mut state)?;
                object(
                    face.map(|id| {
                        FilletClassObject::Topology(FilletBody::Target, EntityRef::Face(id))
                    }),
                    target,
                    pseudo.brep(),
                    &mut state,
                )?;
                state.extend([
                    f64::from(cc.any_uv_curve1().is_some()),
                    f64::from(mate_uv.is_some()),
                    f64::from(cc.any_uv_curve1().is_some() && cc.any_uv_curve1() == mate_uv),
                    f64::from(cc.any_uv_curve2().is_some()),
                    f64::from(cc.surface1().is_some()),
                    f64::from(cc.surface1().is_some() && cc.surface1() == expected_surface),
                    f64::from(cc.surface2().is_some()),
                    f64::from(cc.doing_fillets()),
                    f64::from(cc.is_curve_current()),
                    f64::from(cc.ignore_tangency()),
                    f64::from(cc.ok_to_set_tolerance()),
                    f64::from(cc.tgt_loops_only()),
                    f64::from(cc.other_classification().is_some()),
                    cc.size() as f64,
                ]);
                let interval = cc.interval();
                values.extend([interval.min(), interval.max()]);
                for interval in cc.intervals() {
                    values.extend([interval.interval.min(), interval.interval.max()]);
                    for point_class in [&interval.start, &interval.mid, &interval.end] {
                        point(point_class, target, pseudo.brep(), &mut state, &mut values)?;
                    }
                }
            }
        }
    }
    let vertices = p1::vertices(target, None)?;
    state.push(vertices.len() as f64);
    for id in vertices {
        values.push(target.vertex(id)?.tolerance);
    }
    let edges = p1::edges(target, None)?;
    state.push(edges.len() as f64);
    for id in edges {
        values.push(target.edge(id)?.tolerance);
    }
    Ok((state, values))
}
