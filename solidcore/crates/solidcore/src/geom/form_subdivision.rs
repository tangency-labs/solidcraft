//! Coordinate stencils used by an editable Catmull–Clark cage.
use crate::eval::vec::VecExt;
use crate::Vec3;

/// Numerical edge data in the caller's stable topology order.
#[derive(Clone, Copy)]
pub struct StepEdge {
    /// Canonical endpoint identities.
    pub vertices: [usize; 2],
    /// One or two adjacent face identities in encounter order.
    pub faces: [Option<usize>; 2],
    /// Authored edge sharpness before decay.
    pub sharpness: f64,
}

/// Evaluation limits also used before portable-coordinate conversion.
pub const MAX_STEP_POINTS: usize = 600_000;
/// Maximum number of source faces in one evaluation step.
pub const MAX_STEP_FACES: usize = 500_000;

fn bad(reason: &'static str) -> crate::SolidError {
    crate::SolidError::invalid_input("Form subdivision", reason)
}

/// Coordinate centroid in supplied order; no sorted or parallel reduction.
pub fn centroid(points: impl ExactSizeIterator<Item = Vec3>) -> crate::Result<Vec3> {
    let count = points.len();
    if !(1..=MAX_STEP_POINTS).contains(&count) {
        return Err(bad("centroid point budget exceeded"));
    }
    let mut sum = Vec3::ZERO;
    for point in points {
        if !point.is_finite() {
            return Err(bad("centroid points must be finite"));
        }
        sum += point;
    }
    let center = sum / count as f64;
    if !center.is_finite() {
        return Err(bad("centroid is not finite"));
    }
    Ok(center)
}

/// Validate the common legacy face/edge extrusion displacement.
pub fn extrusion_delta(delta: Vec3) -> bool {
    delta.is_finite() && delta.len() >= 1e-9
}

/// Decay one uniform semi-sharp constraint, retaining the permanent value ten.
pub fn decay_sharpness(sharpness: f64) -> f64 {
    if sharpness >= 10.0 {
        10.0
    } else {
        (sharpness - 1.0).max(0.0)
    }
}

// Independently validate the numerical API's topology contract without changing
// the caller's reduction order. Each local fan is bounded by valence 65.
fn validate_step_incidence(
    count: usize,
    faces: &[Vec<usize>],
    edges: &[StepEdge],
    incident: &[Vec<usize>],
) -> crate::Result<()> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut lookup = BTreeMap::new();
    let mut degree = vec![0usize; count];
    for (id, edge) in edges.iter().enumerate() {
        let [a, b] = edge.vertices;
        if a >= b || lookup.insert([a, b], id).is_some() {
            return Err(bad("duplicate or noncanonical subdivision edge"));
        }
        for &vertex in &edge.vertices {
            let degree = degree
                .get_mut(vertex)
                .ok_or_else(|| bad("invalid subdivision endpoint"))?;
            *degree += 1;
            if *degree > 65 {
                return Err(bad("subdivision vertex fan budget exceeded"));
            }
        }
    }
    let mut uses = vec![0u8; edges.len()];
    let mut forward = vec![None; edges.len()];
    let mut unique_faces = BTreeSet::new();
    for (face_id, face) in faces.iter().enumerate() {
        let distinct: BTreeSet<_> = face.iter().copied().collect();
        if distinct.len() != face.len()
            || distinct.iter().any(|&id| id >= count)
            || !unique_faces.insert(distinct)
        {
            return Err(bad("invalid or duplicate subdivision face"));
        }
        for (&a, &b) in face
            .iter()
            .zip(face.iter().cycle().skip(1))
            .take(face.len())
        {
            let &id = lookup
                .get(&[a.min(b), a.max(b)])
                .ok_or_else(|| bad("face edge missing from subdivision incidence"))?;
            let edge = edges
                .get(id)
                .ok_or_else(|| bad("invalid subdivision edge"))?;
            let seen = uses
                .get_mut(id)
                .ok_or_else(|| bad("invalid subdivision edge count"))?;
            let direction = forward
                .get_mut(id)
                .ok_or_else(|| bad("invalid subdivision edge direction"))?;
            if edge.faces.get(usize::from(*seen)).copied().flatten() != Some(face_id)
                || direction.is_some_and(|first| first == (a < b))
            {
                return Err(bad("inconsistent subdivision face ownership or winding"));
            }
            *seen += 1;
            *direction = Some(a < b);
        }
    }
    if edges
        .iter()
        .zip(uses)
        .any(|(edge, used)| usize::from(used) != edge.faces.iter().flatten().count())
    {
        return Err(bad("subdivision edge has missing face ownership"));
    }
    for (vertex, adjacent) in incident.iter().enumerate() {
        if degree.get(vertex).copied() != Some(adjacent.len()) {
            return Err(bad("subdivision vertex incidence is incomplete"));
        }
        let mut seen = BTreeSet::new();
        for &id in adjacent {
            let edge = edges
                .get(id)
                .ok_or_else(|| bad("invalid subdivision edge"))?;
            if !edge.vertices.contains(&vertex) || !seen.insert(id) {
                return Err(bad("duplicate or unrelated subdivision vertex edge"));
            }
        }
        let fan: BTreeSet<_> = adjacent
            .iter()
            .filter_map(|&id| edges.get(id))
            .flat_map(|edge| edge.faces.into_iter().flatten())
            .collect();
        if fan.is_empty() || fan.len() > 64 {
            return Err(bad("invalid subdivision vertex fan"));
        }
        let mut reached = BTreeSet::new();
        let mut pending: Vec<_> = fan.first().copied().into_iter().collect();
        while let Some(face) = pending.pop() {
            if !reached.insert(face) {
                continue;
            }
            for &id in adjacent {
                let edge = edges
                    .get(id)
                    .ok_or_else(|| bad("invalid subdivision edge"))?;
                if edge.faces.contains(&Some(face)) {
                    pending.extend(
                        edge.faces
                            .into_iter()
                            .flatten()
                            .filter(|next| !reached.contains(next)),
                    );
                }
            }
        }
        if reached.len() != fan.len() {
            return Err(bad("disconnected subdivision vertex fan"));
        }
    }
    Ok(())
}

/// Evaluate one Catmull–Clark step, retaining the supplied face/edge incidence
/// and reduction order. Output order is old vertices, edge points, face points.
/// Connectivity and persistent logical identities remain the caller's records.
pub fn step_points(
    points: &[Vec3],
    faces: &[Vec<usize>],
    edges: &[StepEdge],
    incident: &[Vec<usize>],
    corners: &[f64],
) -> crate::Result<Vec<Vec3>> {
    let size = points
        .len()
        .checked_add(edges.len())
        .and_then(|n| n.checked_add(faces.len()))
        .ok_or_else(|| bad("step point budget exceeded"))?;
    if points.is_empty()
        || faces.is_empty()
        || size > MAX_STEP_POINTS
        || faces.len() > MAX_STEP_FACES
        || incident.len() != points.len()
        || corners.len() != points.len()
        || points.iter().any(|p| !p.is_finite())
        || corners
            .iter()
            .any(|s| !s.is_finite() || !(0.0..=10.0).contains(s))
        || faces.iter().any(|face| !(3..=32).contains(&face.len()))
        || incident.iter().any(|row| row.is_empty() || row.len() > 65)
        || edges.iter().any(|edge| {
            edge.vertices[0] == edge.vertices[1]
                || edge.vertices.iter().any(|&id| id >= points.len())
                || edge.faces.iter().flatten().any(|&id| id >= faces.len())
                || edge.faces[0].is_none()
                || edge.faces[0] == edge.faces[1]
                || !edge.sharpness.is_finite()
                || !(0.0..=10.0).contains(&edge.sharpness)
        })
    {
        return Err(bad("invalid bounded subdivision incidence"));
    }
    validate_step_incidence(points.len(), faces, edges, incident)?;
    let point = |id| {
        points
            .get(id)
            .copied()
            .ok_or_else(|| bad("invalid subdivision vertex"))
    };
    let centers = faces
        .iter()
        .map(|face| {
            let controls = face
                .iter()
                .map(|&id| point(id))
                .collect::<crate::Result<Vec<_>>>()?;
            centroid(controls.into_iter())
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let mut output = Vec::with_capacity(size);
    for (vertex, (&p, adjacent)) in points.iter().zip(incident).enumerate() {
        let mut boundary = Vec::new();
        let mut face_ids = std::collections::BTreeSet::new();
        let mut midpoint = Vec3::ZERO;
        let mut creases = Vec::new();
        for &id in adjacent {
            let edge = edges
                .get(id)
                .ok_or_else(|| bad("invalid subdivision edge"))?;
            let [a, b] = edge.vertices;
            let other = if a == vertex {
                b
            } else if b == vertex {
                a
            } else {
                return Err(bad("edge does not belong to its incident vertex"));
            };
            let q = point(other)?;
            midpoint += (p + q) / 2.0;
            face_ids.extend(edge.faces.into_iter().flatten());
            let sharpness = if edge.faces[1].is_none() {
                boundary.push(q);
                10.0
            } else {
                edge.sharpness
            };
            if sharpness > 0.0 {
                creases.push((sharpness, other));
            }
        }
        let smooth = if boundary.len() == 2 {
            let [a, b] = boundary.as_slice() else {
                return Err(bad("invalid subdivision boundary"));
            };
            (p * 6.0 + *a + *b) / 8.0
        } else if boundary.is_empty() {
            if face_ids.is_empty() || face_ids.len() > 64 {
                return Err(bad("invalid subdivision vertex fan"));
            }
            let mut average = Vec3::ZERO;
            for &id in &face_ids {
                average += *centers
                    .get(id)
                    .ok_or_else(|| bad("invalid subdivision face"))?;
            }
            let n = adjacent.len() as f64;
            (average / face_ids.len() as f64 + midpoint * (2.0 / n) + p * (n - 3.0)) / n
        } else {
            return Err(bad("nonmanifold subdivision boundary"));
        };
        creases.sort_by(|a, b| b.0.total_cmp(&a.0));
        let corner = *corners
            .get(vertex)
            .ok_or_else(|| bad("invalid corner sharpness"))?;
        let surviving: Vec<_> = creases.iter().copied().filter(|(s, _)| *s > 1.0).collect();
        let rule = |corner: bool, count: usize| if corner { 3 } else { count.min(3) };
        let parent = rule(corner > 0.0, creases.len());
        let child = rule(corner > 1.0, surviving.len());
        let mask = |rule, creases: &[(f64, usize)]| -> crate::Result<Vec3> {
            match rule {
                0 | 1 => Ok(smooth),
                2 => {
                    let (_, a) = creases
                        .first()
                        .ok_or_else(|| bad("missing crease endpoint"))?;
                    let (_, b) = creases
                        .get(1)
                        .ok_or_else(|| bad("missing crease endpoint"))?;
                    Ok((p * 6.0 + point(*a)? + point(*b)?) / 8.0)
                }
                _ => Ok(p),
            }
        };
        let value = if parent < 2 {
            smooth
        } else if parent == child {
            mask(parent, &creases)?
        } else {
            let mut transitions: Vec<_> = creases
                .iter()
                .filter(|(s, _)| *s <= 1.0)
                .map(|(s, _)| *s)
                .collect();
            if corner > 0.0 && corner <= 1.0 {
                transitions.push(corner);
            }
            if transitions.is_empty() {
                return Err(bad("missing crease rule transition"));
            }
            let weight = transitions.iter().sum::<f64>() / transitions.len() as f64;
            mask(parent, &creases)? * weight + mask(child, &surviving)? * (1.0 - weight)
        };
        output.push(value);
    }
    for edge in edges {
        let [a, b] = edge.vertices;
        let mut p = point(a)? + point(b)?;
        if edge.faces[1].is_some() {
            for id in edge.faces.into_iter().flatten() {
                p += *centers
                    .get(id)
                    .ok_or_else(|| bad("invalid subdivision face"))?;
            }
            p /= 4.0;
        } else {
            p /= 2.0;
        }
        let s = edge.sharpness.min(1.0);
        output.push(p * (1.0 - s) + (point(a)? + point(b)?) * (s / 2.0));
    }
    output.extend(centers);
    if output.iter().any(|p| !p.is_finite()) {
        return Err(bad("subdivision coordinates are not finite"));
    }
    Ok(output)
}

/// Area-weighted display normals from an explicitly supplied fan triangulation.
/// Degenerate vertices keep the existing display-only positive-Z fallback.
pub fn vertex_normals(points: &[Vec3], triangles: &[[u32; 3]]) -> crate::Result<Vec<Vec3>> {
    if points.len() > MAX_STEP_POINTS
        || triangles.len() > 2 * MAX_STEP_FACES
        || points.iter().any(|p| !p.is_finite())
    {
        return Err(bad("display normal budget exceeded"));
    }
    let mut normals = vec![Vec3::ZERO; points.len()];
    for &[a, b, c] in triangles {
        let at = |id: u32| {
            points
                .get(id as usize)
                .copied()
                .ok_or_else(|| bad("invalid display triangle"))
        };
        let (pa, pb, pc) = (at(a)?, at(b)?, at(c)?);
        let normal = (pb - pa).cross(pc - pa);
        for id in [a, b, c] {
            *normals
                .get_mut(id as usize)
                .ok_or_else(|| bad("invalid normal slot"))? += normal;
        }
    }
    for normal in &mut normals {
        *normal = normal.normalized().unwrap_or(Vec3::Z);
    }
    Ok(normals)
}

/// Limit point of a corner along a boundary or a pair of sharp edges.
pub fn crease_limit(point: Vec3, [a, b]: [Vec3; 2]) -> Vec3 {
    (point * 4.0 + a + b) / 6.0
}

/// Limit point of a smooth quad fan. Supply radial and opposite vertices
/// in source order so the stencil keeps the evaluator's rounding order.
pub fn smooth_limit(
    point: Vec3,
    neighbors: impl IntoIterator<Item = Vec3>,
    diagonals: impl IntoIterator<Item = Vec3>,
    valence: usize,
) -> Vec3 {
    let neighbors = neighbors.into_iter().fold(Vec3::ZERO, |sum, p| sum + p);
    let diagonals = diagonals.into_iter().fold(Vec3::ZERO, |sum, p| sum + p);
    let n = valence as f64;
    (point * (n * n) + neighbors * 4.0 + diagonals) / (n * (n + 5.0))
}

/// Whether a semi-sharp corner's successive values have converged.
pub fn converged_limit(current: Vec3, previous: Vec3) -> bool {
    current.distance(previous) < 1e-12 * current.length().max(1.0)
}

/// Uniform cubic B-spline span to Bézier in both directions, U fastest.
pub fn regular_bezier(net: [Vec3; 16]) -> [Vec3; 16] {
    let weights = [
        [1.0, 4.0, 1.0, 0.0],
        [0.0, 4.0, 2.0, 0.0],
        [0.0, 2.0, 4.0, 0.0],
        [0.0, 1.0, 4.0, 1.0],
    ];
    let mut output = [Vec3::ZERO; 16];
    for (point, (u, v)) in output.iter_mut().zip(
        weights
            .into_iter()
            .flat_map(|v| weights.into_iter().map(move |u| (u, v))),
    ) {
        let product = v
            .into_iter()
            .flat_map(|v| u.into_iter().map(move |u| u * v / 36.0));
        for (control, weight) in net.into_iter().zip(product) {
            *point += control * weight;
        }
    }
    output
}

/// Project displacement onto its reflection plane and refuse a zero or nonfinite offset.
pub fn mirror_tangent(delta: Vec3, axis: usize) -> Option<Vec3> {
    let mut projected = delta.to_array();
    *projected.get_mut(axis)? = 0.0;
    let result = Vec3::from_array(projected);
    (delta.is_finite() && result.len() >= 1e-9).then_some(result)
}

/// Coordinate geometry of a derived ruled trim-wall preview.
pub struct TrimWallGeometry {
    /// Translated sampled boundary, in the original directed order.
    pub top: Vec<Vec3>,
    /// Unit normals in reverse sampled-boundary order, as used by the ruled strips.
    pub normals: Vec<Vec3>,
    /// Translated logical endpoint controls.
    pub controls: [Vec3; 2],
}

/// Validate a nonzero derived-wall displacement within the Form extent.
pub fn validate_trim_wall_delta(delta: Vec3) -> crate::Result<()> {
    if !delta.is_finite()
        || delta.length() < 1e-9
        || delta.x.abs().max(delta.y.abs()).max(delta.z.abs()) > 200_000.0
    {
        return Err(crate::SolidError::invalid_input(
            "Form trim wall",
            "trim wall needs a finite nonzero displacement within the Form bounds",
        ));
    }
    Ok(())
}

/// Add an editing displacement and validate before publishing the new wall.
pub fn move_trim_wall(delta: Vec3, offset: Vec3) -> crate::Result<Vec3> {
    let moved = delta + offset;
    validate_trim_wall_delta(moved)?;
    Ok(moved)
}

/// Build bounded ruled-wall positions and normals, refusing an along-seam displacement.
pub fn trim_wall(
    base: &[Vec3],
    controls: [Vec3; 2],
    delta: Vec3,
) -> crate::Result<TrimWallGeometry> {
    let bad = |reason| crate::SolidError::invalid_input("Form trim wall", reason);
    validate_trim_wall_delta(delta)?;
    if !(2..=1_000_000).contains(&base.len()) {
        return Err(bad("trim preview path needs bounded stations"));
    }
    let bounded = |p: &Vec3| p.is_finite() && p.x.abs().max(p.y.abs()).max(p.z.abs()) <= 100_000.0;
    let top: Vec<_> = base.iter().map(|&p| p + delta).collect();
    if base.iter().chain(&top).any(|p| !bounded(p)) {
        return Err(bad("trim wall positions must remain within 100000 mm"));
    }
    let controls = controls.map(|p| p + delta);
    if controls.iter().any(|p| !bounded(p)) {
        return Err(bad("trim wall controls must remain within 100000 mm"));
    }
    let normals = (0..base.len())
        .map(|i| {
            let next = base.len() - 1 - i.saturating_sub(1);
            let prev = base.len() - 1 - (i + 1).min(base.len() - 1);
            let a = *base.get(prev).ok_or_else(|| bad("invalid trim station"))?;
            let b = *base.get(next).ok_or_else(|| bad("invalid trim station"))?;
            (a - b)
                .cross(delta)
                .normalized()
                .ok_or_else(|| bad("trim wall displacement is along its seam"))
        })
        .collect::<crate::Result<Vec<_>>>()?;
    Ok(TrimWallGeometry {
        top,
        normals,
        controls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incidence(faces: &[Vec<usize>], count: usize) -> (Vec<StepEdge>, Vec<Vec<usize>>) {
        let mut lookup = std::collections::BTreeMap::<[usize; 2], usize>::new();
        let mut edges = Vec::<StepEdge>::new();
        for (face, vertices) in faces.iter().enumerate() {
            for (&a, &b) in vertices
                .iter()
                .zip(vertices.iter().cycle().skip(1))
                .take(vertices.len())
            {
                let key = [a.min(b), a.max(b)];
                if let Some(&id) = lookup.get(&key) {
                    edges.get_mut(id).unwrap().faces[1] = Some(face);
                } else {
                    lookup.insert(key, edges.len());
                    edges.push(StepEdge {
                        vertices: key,
                        faces: [Some(face), None],
                        sharpness: 0.0,
                    });
                }
            }
        }
        let mut incident = vec![Vec::new(); count];
        for (id, edge) in edges.iter().enumerate() {
            for &vertex in &edge.vertices {
                incident[vertex].push(id);
            }
        }
        (edges, incident)
    }

    #[test]
    fn quad_step_keeps_boundary_order_fractional_corners_and_affine_covariance() {
        let points = [
            Vec3::ZERO,
            Vec3::X * 2.0,
            (Vec3::X + Vec3::Y) * 2.0,
            Vec3::Y * 2.0,
        ];
        let faces = vec![vec![0, 1, 2, 3]];
        let (edges, incident) = incidence(&faces, 4);
        let result = step_points(&points, &faces, &edges, &incident, &[0.0; 4]).unwrap();
        assert_eq!(
            result,
            vec![
                Vec3::new(0.25, 0.25, 0.0),
                Vec3::new(1.75, 0.25, 0.0),
                Vec3::new(1.75, 1.75, 0.0),
                Vec3::new(0.25, 1.75, 0.0),
                Vec3::X,
                Vec3::new(2.0, 1.0, 0.0),
                Vec3::new(1.0, 2.0, 0.0),
                Vec3::Y,
                Vec3::new(1.0, 1.0, 0.0)
            ]
        );
        let transformed = points.map(|p| p * 2.0 + Vec3::Z * 7.0);
        assert_eq!(
            step_points(&transformed, &faces, &edges, &incident, &[0.0; 4]).unwrap(),
            result
                .iter()
                .map(|&p| p * 2.0 + Vec3::Z * 7.0)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            step_points(&points, &faces, &edges, &incident, &[0.5, 0.0, 0.0, 0.0]).unwrap()[0],
            Vec3::new(0.125, 0.125, 0.0)
        );
        assert_eq!(
            step_points(&points, &faces, &edges, &incident, &[10.0, 0.0, 0.0, 0.0]).unwrap()[0],
            Vec3::ZERO
        );
        assert_eq!([0.5, 1.5, 10.0].map(decay_sharpness), [0.0, 0.5, 10.0]);
        let mut hostile = incident.clone();
        hostile[0][0] = usize::MAX;
        assert!(step_points(&points, &faces, &edges, &hostile, &[0.0; 4]).is_err());
        assert!(step_points(
            &points,
            &[vec![0, 1, 2, usize::MAX]],
            &edges,
            &incident,
            &[0.0; 4]
        )
        .is_err());
    }

    #[test]
    fn step_refuses_forged_incidence_before_evaluating_coordinates() {
        let cage = crate::geom::form_cage::build(
            crate::geom::form_cage::Primitive::Box,
            [2.0; 3],
            [1; 3],
            [0.0; 2],
            &[],
        )
        .unwrap();
        let (edges, incident) = incidence(&cage.faces, cage.points.len());
        let corners = vec![0.0; cage.points.len()];
        let evaluate = |faces: &[Vec<usize>], edges: &[StepEdge], incident: &[Vec<usize>]| {
            step_points(&cage.points, faces, edges, incident, &corners)
        };
        assert!(evaluate(&cage.faces, &edges, &incident).is_ok());
        let mut duplicate = incident.clone();
        let first = duplicate[0][0];
        duplicate[0].push(first);
        assert!(evaluate(&cage.faces, &edges, &duplicate).is_err());
        let mut missing = incident.clone();
        missing[0].pop();
        assert!(evaluate(&cage.faces, &edges, &missing).is_err());
        let mut repeated = incident.clone();
        repeated[0][1] = repeated[0][0];
        assert!(evaluate(&cage.faces, &edges, &repeated).is_err());
        let mut wrong_owner = edges.clone();
        let old = wrong_owner[0].faces[0].unwrap();
        wrong_owner[0].faces[0] = Some((old + 2) % cage.faces.len());
        assert!(evaluate(&cage.faces, &wrong_owner, &incident).is_err());
        let mut wrong_direction = cage.faces.clone();
        wrong_direction[0].reverse();
        assert!(evaluate(&wrong_direction, &edges, &incident).is_err());
        let mut duplicate_edge = edges.clone();
        duplicate_edge[1] = duplicate_edge[0];
        assert!(evaluate(&cage.faces, &duplicate_edge, &incident).is_err());
        let mut disconnected = crate::geom::form_cage::CageGeometry {
            points: cage.points.clone(),
            faces: cage.faces.clone(),
        };
        let offset = disconnected.points.len() - 1;
        disconnected
            .points
            .extend(cage.points.iter().skip(1).copied());
        disconnected.faces.extend(cage.faces.iter().map(|face| {
            face.iter()
                .map(|&id| if id == 0 { 0 } else { id + offset })
                .collect()
        }));
        let (separate_edges, separate_incident) =
            incidence(&disconnected.faces, disconnected.points.len());
        assert!(step_points(
            &disconnected.points,
            &disconnected.faces,
            &separate_edges,
            &separate_incident,
            &vec![0.0; disconnected.points.len()]
        )
        .is_err());
        let mut reversed_incident = incident.clone();
        for row in &mut reversed_incident {
            row.reverse();
        }
        assert!(
            evaluate(&cage.faces, &edges, &reversed_incident).is_ok(),
            "valid reduction order is retained"
        );
    }

    #[test]
    fn closed_cube_step_uses_smooth_and_fractional_two_crease_masks() {
        let cage = crate::geom::form_cage::build(
            crate::geom::form_cage::Primitive::Box,
            [2.0; 3],
            [1; 3],
            [0.0; 2],
            &[],
        )
        .unwrap();
        let (mut edges, incident) = incidence(&cage.faces, cage.points.len());
        let result = step_points(
            &cage.points,
            &cage.faces,
            &edges,
            &incident,
            &vec![0.0; cage.points.len()],
        )
        .unwrap();
        for (&source, actual) in cage.points.iter().zip(&result) {
            assert!(actual.distance(source * (5.0 / 9.0)) < 1e-14);
        }
        edges[incident[0][0]].sharpness = 0.25;
        edges[incident[0][1]].sharpness = 0.75;
        let fractional = step_points(
            &cage.points,
            &cage.faces,
            &edges,
            &incident,
            &vec![0.0; cage.points.len()],
        )
        .unwrap();
        assert!(fractional[0].distance(Vec3::new(-47.0 / 72.0, -47.0 / 72.0, 7.0 / 9.0)) < 1e-14);
    }

    #[test]
    fn ordered_centroids_and_area_normals_keep_degenerate_display_fallback() {
        assert_eq!(
            centroid([Vec3::X * 1e16, Vec3::X, -Vec3::X * 1e16].into_iter()).unwrap(),
            Vec3::ZERO
        );
        assert!(centroid([].into_iter()).is_err());
        assert!(centroid([Vec3::new(f64::NAN, 0.0, 0.0)].into_iter()).is_err());
        let points = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        let normals = vertex_normals(&points, &[[0, 1, 2], [0, 2, 3]]).unwrap();
        assert!(normals[0].distance((Vec3::X + Vec3::Z) / 2.0_f64.sqrt()) < 1e-14);
        assert_eq!(
            vertex_normals(&points, &[[0, 0, 0]]).unwrap(),
            vec![Vec3::Z; 4]
        );
        assert!(vertex_normals(&points, &[[0, 1, u32::MAX]]).is_err());
        assert!(!extrusion_delta(Vec3::ZERO));
        assert!(!extrusion_delta(Vec3::new(f64::NAN, 0.0, 0.0)));
        assert!(extrusion_delta(Vec3::Z));
    }

    #[test]
    fn trim_wall_checks_all_geometry_before_returning_directed_strips() {
        let base = [Vec3::ZERO, Vec3::X, Vec3::X * 2.0];
        let wall = trim_wall(&base, [Vec3::ZERO, Vec3::X * 2.0], Vec3::Z).unwrap();
        assert_eq!(
            wall.top,
            vec![Vec3::Z, Vec3::X + Vec3::Z, Vec3::X * 2.0 + Vec3::Z]
        );
        assert_eq!(wall.controls, [Vec3::Z, Vec3::X * 2.0 + Vec3::Z]);
        assert!(wall
            .normals
            .iter()
            .all(|normal| normal.distance(Vec3::Y) < 1e-12));
        assert!(trim_wall(&base, [Vec3::ZERO, Vec3::X], Vec3::X).is_err());
        assert!(trim_wall(&base, [Vec3::ZERO, Vec3::new(100_000.0, 0.0, 0.0)], Vec3::X).is_err());
        assert!(move_trim_wall(Vec3::Z, -Vec3::Z).is_err());
    }

    #[test]
    fn regular_planar_net_converts_to_uniform_bezier_coordinates() {
        let net = std::array::from_fn(|i| Vec3::new((i % 4) as f64, (i / 4) as f64, 5.0));
        for (i, p) in regular_bezier(net).into_iter().enumerate() {
            let expected = Vec3::new(1.0 + (i % 4) as f64 / 3.0, 1.0 + (i / 4) as f64 / 3.0, 5.0);
            assert!(p.distance(expected) < 1e-14);
        }
        assert_eq!(
            mirror_tangent(Vec3::new(6.0, 2.0, -5.0), 0),
            Some(Vec3::new(0.0, 2.0, -5.0))
        );
        assert_eq!(mirror_tangent(Vec3::X, 0), None);
    }
}
