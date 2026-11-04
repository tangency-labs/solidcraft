// SPDX-License-Identifier: Apache-2.0
//! Bound derived vertex tolerances by each owning body's point extent.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::{decode::curve_point, finite_or_refusal};
use cadmpeg_ir::geometry::ProceduralCurveDefinition;
use cadmpeg_ir::index::{ModelIndex, StandardIndex};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::units::COINCIDENCE_TOLERANCE;
use cadmpeg_ir::CadIr;

use crate::loss::F3dLossCode;

const MAX_BODY_DIAGONAL_FRACTION: f64 = 0.01;

#[derive(Default)]
struct EndpointMisses {
    maximum: f64,
    exact_miss: bool,
    incomplete: bool,
}

pub(super) fn admit(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    annotations: &mut [cadmpeg_asm::brep::annotations::AnnotationRecord],
) -> Result<(), CodecError> {
    if !ir
        .model
        .procedural_curves
        .iter()
        .any(|curve| matches!(curve.definition(), ProceduralCurveDefinition::Exact { .. }))
    {
        return Ok(());
    }
    let index = ModelIndex::new_model_only(ir);
    let candidates: HashSet<_> = ir
        .model
        .edges
        .iter()
        .filter(|edge| {
            edge.curve()
                .and_then(|id| index.curves(id.as_str(), StandardIndex))
                .filter(|curve| {
                    matches!(
                        curve.geometry,
                        cadmpeg_ir::geometry::CurveGeometry::Procedural {
                            cache: Some(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(_)),
                            ..
                        }
                    )
                })
                .and_then(|curve| curve.geometry.procedural_construction())
                .and_then(|id| index.procedural_curves(id.as_str(), StandardIndex))
                .is_some_and(|curve| {
                    matches!(curve.definition(), ProceduralCurveDefinition::Exact { .. })
                })
        })
        .flat_map(|edge| [edge.start.as_str(), edge.end.as_str()])
        .collect();
    if candidates.is_empty() {
        return Ok(());
    }
    let mut misses = HashMap::<String, EndpointMisses>::new();
    for edge in &ir.model.edges {
        if !candidates.contains(edge.start.as_str()) && !candidates.contains(edge.end.as_str()) {
            continue;
        }
        let carrier = edge
            .curve()
            .and_then(|id| index.curves(id.as_str(), StandardIndex));
        let construction = carrier
            .and_then(|curve| curve.geometry.procedural_construction())
            .and_then(|id| index.procedural_curves(id.as_str(), StandardIndex));
        let exact = construction.is_some_and(|curve| {
            matches!(curve.definition(), ProceduralCurveDefinition::Exact { .. })
        });
        let fit = construction
            .and_then(cadmpeg_ir::geometry::ProceduralCurve::cache_fit_tolerance)
            .map_or(0.0, cadmpeg_ir::geometry::FitTolerance::get);
        for (side, id) in [&edge.start, &edge.end].into_iter().enumerate() {
            if !candidates.contains(id.as_str()) {
                continue;
            }
            let entry = misses.entry(id.as_str().to_owned()).or_default();
            let point = index
                .vertices(id.as_str(), StandardIndex)
                .and_then(|vertex| index.points(vertex.point.as_str(), StandardIndex));
            let (Some(curve), Some(range), Some(point)) = (carrier, edge.param_range(), point)
            else {
                entry.incomplete = true;
                continue;
            };
            let Some(endpoint) =
                finite_or_refusal(curve_point(ctx, &curve.geometry, range.get()[side]))?
            else {
                entry.incomplete = true;
                continue;
            };
            let miss = Point3::distance(endpoint.get(), point.position().get());
            if !miss.is_finite() {
                entry.incomplete = true;
                continue;
            }
            entry.maximum = entry.maximum.max(miss);
            let vertex_tolerance = index
                .vertices(id.as_str(), StandardIndex)
                .and_then(|vertex| vertex.tolerance)
                .map_or(0.0, PositiveReal::get);
            let allowance = ir
                .tolerances
                .linear
                .get()
                .max(COINCIDENCE_TOLERANCE)
                .max(edge.tolerance.map_or(0.0, PositiveReal::get))
                .max(vertex_tolerance)
                .max(fit);
            entry.exact_miss |= exact && miss > allowance;
        }
    }
    for coedge in &ir.model.coedges {
        let Some(use_curve) = &coedge.use_curve else {
            continue;
        };
        let Some(edge) = index.edges(coedge.edge.as_str(), StandardIndex) else {
            continue;
        };
        let vertices = match coedge.sense {
            cadmpeg_ir::topology::Sense::Forward => [&edge.start, &edge.end],
            cadmpeg_ir::topology::Sense::Reversed => [&edge.end, &edge.start],
        };
        let curve = index.curves(use_curve.curve.as_str(), StandardIndex);
        for (id, parameter) in vertices
            .into_iter()
            .zip(use_curve.parameter_range.endpoints())
        {
            if !candidates.contains(id.as_str()) {
                continue;
            }
            let entry = misses.entry(id.as_str().to_owned()).or_default();
            let point = index
                .vertices(id.as_str(), StandardIndex)
                .and_then(|vertex| index.points(vertex.point.as_str(), StandardIndex));
            let (Some(curve), Some(point)) = (curve, point) else {
                entry.incomplete = true;
                continue;
            };
            let Some(endpoint) = finite_or_refusal(curve_point(ctx, &curve.geometry, parameter))?
            else {
                entry.incomplete = true;
                continue;
            };
            let miss = Point3::distance(endpoint.get(), point.position().get());
            if miss.is_finite() {
                entry.maximum = entry.maximum.max(miss);
            } else {
                entry.incomplete = true;
            }
        }
    }
    let bounds = vertex_bounds(ir, &index);
    let mut widened = HashSet::new();
    let mut maximum_widening = 0.0_f64;
    for vertex in &mut ir.model.vertices {
        let Some(miss) = misses.get(vertex.id.as_str()) else {
            continue;
        };
        let Some(bound) = bounds.get(vertex.id.as_str()) else {
            continue;
        };
        if miss.incomplete || !miss.exact_miss || miss.maximum > *bound {
            continue;
        }
        let previous = vertex.tolerance.map_or(0.0, PositiveReal::get);
        if miss.maximum <= previous {
            continue;
        }
        vertex.tolerance = PositiveReal::new(miss.maximum);
        widened.insert(vertex.id.as_str());
        maximum_widening = maximum_widening.max(miss.maximum - previous);
    }
    if !widened.is_empty() {
        for annotation in annotations {
            if widened.contains(annotation.id.as_str())
                && !annotation.derived_fields.contains(&"tolerance")
            {
                annotation.derived_fields.push("tolerance");
            }
        }
        let widened = widened.len();
        losses.push(F3dLossCode::VertexToleranceWidened.note(format!(
            "Widened {widened} vertex tolerance(s); maximum widening {maximum_widening:.17} mm"
        )));
    }
    Ok(())
}

fn vertex_bounds<'a>(ir: &'a CadIr, index: &ModelIndex<'a>) -> HashMap<String, f64> {
    let mut owners = HashMap::<_, Vec<_>>::new();
    let body_for_shell = |shell: &str| {
        let shell = index.shells(shell, StandardIndex)?;
        let region = index.regions(shell.region.as_str(), StandardIndex)?;
        index.bodies(region.body.as_str(), StandardIndex)
    };
    for coedge in &ir.model.coedges {
        let body = index
            .loops(coedge.owner_loop.as_str(), StandardIndex)
            .and_then(|loop_| index.faces(loop_.face.as_str(), StandardIndex))
            .and_then(|face| body_for_shell(face.shell.as_str()));
        if let Some(body) = body {
            owners
                .entry(coedge.edge.as_str())
                .or_default()
                .push(body.id.as_str());
        }
    }
    for shell in &ir.model.shells {
        if let Some(body) = body_for_shell(shell.id.as_str()) {
            for edge in shell.wire_edges() {
                owners
                    .entry(edge.as_str())
                    .or_default()
                    .push(body.id.as_str());
            }
        }
    }
    let mut vertices = HashMap::<_, Vec<_>>::new();
    for edge in &ir.model.edges {
        let Some(bodies) = owners.get(edge.id.as_str()) else {
            continue;
        };
        for vertex in [&edge.start, &edge.end] {
            vertices
                .entry(vertex.as_str())
                .or_default()
                .extend(bodies.iter().copied());
        }
    }
    for shell in &ir.model.shells {
        if let Some(body) = body_for_shell(shell.id.as_str()) {
            for vertex in shell.free_vertices() {
                vertices
                    .entry(vertex.as_str())
                    .or_default()
                    .push(body.id.as_str());
            }
        }
    }
    for loop_ in &ir.model.loops {
        let Some((vertex, _uses)) = loop_.singular_vertex() else {
            continue;
        };
        let body = index
            .faces(loop_.face.as_str(), StandardIndex)
            .and_then(|face| body_for_shell(face.shell.as_str()));
        if let Some(body) = body {
            vertices
                .entry(vertex.as_str())
                .or_default()
                .push(body.id.as_str());
        }
    }
    let mut extents = HashMap::<_, (Point3, Point3)>::new();
    for (vertex, bodies) in &vertices {
        let point = index
            .vertices(vertex, StandardIndex)
            .and_then(|vertex| index.points(vertex.point.as_str(), StandardIndex));
        let Some(point) = point.map(|point| point.position().get()) else {
            continue;
        };
        for body in bodies {
            let (low, high) = extents.entry(*body).or_insert((point, point));
            low.x = low.x.min(point.x);
            low.y = low.y.min(point.y);
            low.z = low.z.min(point.z);
            high.x = high.x.max(point.x);
            high.y = high.y.max(point.y);
            high.z = high.z.max(point.z);
        }
    }
    vertices
        .into_iter()
        .filter_map(|(vertex, bodies)| {
            let bound = bodies.into_iter().try_fold(f64::INFINITY, |bound, body| {
                let (low, high) = extents.get(body)?;
                let candidate = Point3::distance(*low, *high) * MAX_BODY_DIAGONAL_FRACTION;
                (candidate.is_finite() && candidate > 0.0).then_some(bound.min(candidate))
            })?;
            Some((vertex.to_owned(), bound))
        })
        .collect()
}
