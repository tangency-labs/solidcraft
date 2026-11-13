//! Mirror geometry is evaluated on the control cage before subdivision. All
//! switches come from the file's self-described record, not evaluated triangles.
use super::*;

use solidcore_ops::{affine, mesh_edit};

fn point(matrix: &Matrix, point: Vec3) -> Result<Vec3> {
    let point = affine::point(matrix, point);
    if !point.is_finite() {
        return Err(bad("non-finite Mirror transform"));
    }
    Ok(point)
}

fn components(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn vector([x, y, z]: [f64; 3]) -> Vec3 {
    Vec3::new(x, y, z)
}

fn compact(mut cage: Cage) -> Result<Cage> {
    let used: BTreeSet<_> = cage.faces.iter().flatten().copied().collect();
    let mut ids = BTreeMap::new();
    let mut points = Vec::new();
    for i in used {
        ids.insert(i, points.len());
        points.push(cage.point(i).map_err(|e| bad(e.to_string()))?);
    }
    for face in &mut cage.faces {
        for i in face {
            *i = *ids.get(i).ok_or_else(|| bad("Mirror vertex remap"))?;
        }
    }
    cage.edge_creases = cage
        .edge_creases
        .into_iter()
        .filter_map(|c| Some(EdgeCrease { edge: [*ids.get(&c.edge[0])?, *ids.get(&c.edge[1])?], sharpness: c.sharpness }))
        .collect();
    cage.vertex_creases =
        cage.vertex_creases.into_iter().filter_map(|c| Some(VertexCrease { vertex: *ids.get(&c.vertex)?, sharpness: c.sharpness })).collect();
    cage.points = points;
    Ok(cage)
}
fn clip(cage: Cage, axis: usize, flip: bool, threshold: f64) -> Result<Cage> {
    let mut cage = cage;
    for point in &mut cage.points {
        *point = vector(mesh_edit::snap_to_plane(components(*point), axis, threshold));
    }
    let mut out = Cage { points: cage.points.clone(), ..Default::default() };
    let mut cuts = BTreeMap::new();
    let distance = |p: Vec3| if flip { -components(p)[axis] } else { components(p)[axis] };
    for face in &cage.faces {
        let mut polygon = Vec::new();
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let pa = cage.point(a).map_err(|e| bad(e.to_string()))?;
            let pb = cage.point(b).map_err(|e| bad(e.to_string()))?;
            let da = distance(pa);
            let db = distance(pb);
            let inside = da >= 0.0;
            let next_inside = db >= 0.0;
            if inside {
                polygon.push(a);
            }
            if inside != next_inside {
                let key = (a.min(b), a.max(b));
                let id = if da == 0.0 {
                    a
                } else if db == 0.0 {
                    b
                } else if let Some(&i) = cuts.get(&key) {
                    i
                } else {
                    if out.points.len() >= MAX_POINTS {
                        return Err(bad("Mirror bisect point budget exceeded"));
                    }
                    let p =
                        mesh_edit::crossing(components(pa), components(pb), axis).map(vector).ok_or_else(|| bad("invalid Mirror bisect crossing"))?;
                    let id = out.points.len();
                    out.points.push(p);
                    cuts.insert(key, id);
                    id
                };
                cuts.insert(key, id);
                polygon.push(id);
            }
        }
        polygon.dedup();
        if polygon.first() == polygon.last() {
            polygon.pop();
        }
        if polygon.len() >= 3 {
            out.faces.push(polygon);
        }
    }
    for c in &cage.edge_creases {
        let [a, b] = c.edge;
        let da = distance(cage.point(a).map_err(|e| bad(e.to_string()))?);
        let db = distance(cage.point(b).map_err(|e| bad(e.to_string()))?);
        let edge = if da >= 0.0 && db >= 0.0 {
            Some([a, b])
        } else if let Some(&i) = cuts.get(&(a.min(b), a.max(b))) {
            Some([if da >= 0.0 { a } else { b }, i])
        } else {
            None
        };
        if let Some(edge) = edge
            && edge[0] != edge[1]
        {
            out.edge_creases.push(EdgeCrease { edge, sharpness: c.sharpness });
        }
    }
    out.vertex_creases = cage.vertex_creases;
    compact(out)
}
fn reflect(cage: Cage, axis: usize, seam_limit: f64) -> Result<Cage> {
    if cage.faces.len().saturating_mul(2) > MAX_FACES || cage.points.len().saturating_mul(2) > MAX_POINTS {
        return Err(bad("Mirror exceeds the editable cage budget"));
    }
    let mut out = cage.clone();
    let mut ids = Vec::with_capacity(cage.points.len());
    for (i, &p) in cage.points.iter().enumerate() {
        let (point, merge) = mesh_edit::mirror(components(p), axis, seam_limit);
        let q = vector(point);
        if merge {
            *out.points.get_mut(i).ok_or_else(|| bad("Mirror seam index"))? = q;
            ids.push(i);
        } else {
            ids.push(out.points.len());
            out.points.push(q);
        }
    }
    for face in &cage.faces {
        if face.iter().all(|&v| ids.get(v) == Some(&v)) {
            continue;
        }
        out.faces.push(face.iter().rev().map(|&v| ids.get(v).copied().ok_or_else(|| bad("Mirror face index"))).collect::<Result<_>>()?);
    }
    let mut edges = BTreeMap::new();
    for c in &cage.edge_creases {
        for edge in
            [c.edge, [*ids.get(c.edge[0]).ok_or_else(|| bad("Mirror crease index"))?, *ids.get(c.edge[1]).ok_or_else(|| bad("Mirror crease index"))?]]
        {
            let k = [edge[0].min(edge[1]), edge[0].max(edge[1])];
            edges.entry(k).and_modify(|v: &mut f64| *v = v.max(c.sharpness)).or_insert(c.sharpness);
        }
    }
    out.edge_creases = edges.into_iter().map(|(edge, sharpness)| EdgeCrease { edge, sharpness }).collect();
    let mut vertices = BTreeMap::new();
    for c in &cage.vertex_creases {
        for vertex in [c.vertex, *ids.get(c.vertex).ok_or_else(|| bad("Mirror corner index"))?] {
            vertices.entry(vertex).and_modify(|v: &mut f64| *v = v.max(c.sharpness)).or_insert(c.sharpness);
        }
    }
    out.vertex_creases = vertices.into_iter().map(|(vertex, sharpness)| VertexCrease { vertex, sharpness }).collect();
    Ok(out)
}

pub(super) fn apply(object: View<'_, '_>, modifier: View<'_, '_>, mut cage: Cage) -> Result<Cage> {
    let flags = modifier.uint("flag", 2)?;
    let tolerance = object.file.numbers.float(modifier.field("tolerance")?, 0)?;
    let threshold = object.file.numbers.float(modifier.field("bisect_threshold")?, 0)?;
    if !tolerance.is_finite() || !threshold.is_finite() || !(0.0..=10000.).contains(&tolerance) || !(0.0..=10000.).contains(&threshold) {
        return Err(bad("invalid Mirror tolerances"));
    }
    let mirror = modifier.ptr("mirror_ob")?;
    let frame = if mirror == 0 {
        identity()
    } else {
        let own = world(object.file, object, &mut BTreeSet::new(), 0)?;
        let target = object.file.view(object.file.block(mirror)?, "Object")?;
        let other = world(object.file, target, &mut BTreeSet::from([mirror]), 0)?;
        let inverse = affine::inverse(&to_affine(other)).map_err(|_| bad("singular Mirror object frame"))?;
        affine::then(&to_affine(own), &inverse).map_err(|_| bad("invalid Mirror object frame"))?
    };
    let back = affine::inverse(&frame).map_err(|_| bad("singular Mirror frame"))?;
    for p in &mut cage.points {
        *p = point(&frame, *p)?;
    }
    for axis in [0, 1, 2] {
        if flags & (8 << axis) == 0 {
            continue;
        }
        if flags & (256 << axis) != 0 {
            cage = clip(cage, axis, flags & (2048 << axis) != 0, threshold)?;
        }
        // A reflected pair differs by twice its signed plane coordinate. Map
        // that axis into object space, where the merge threshold is measured.
        let seam_limit = if flags & 128 == 0 {
            mesh_edit::mirror_seam_limit(&back, axis, tolerance).ok_or_else(|| bad("invalid Mirror frame scale"))?
        } else {
            0.0
        };
        cage = reflect(cage, axis, seam_limit)?;
    }
    for p in &mut cage.points {
        *p = point(&back, *p)?;
    }
    if cage.faces.iter().any(|f| f.len() != 4) {
        return Err(bad("Mirror bisect produced non-quad faces"));
    }
    cage.validate().map_err(|e| bad(e.to_string()))?;
    Ok(cage)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn half_quad() -> Cage {
        Cage {
            points: vec![Vec3::new(0., -1., 0.), Vec3::new(1., -1., 0.), Vec3::new(1., 1., 0.), Vec3::new(0., 1., 0.)],
            faces: vec![vec![0, 1, 2, 3]],
            edge_creases: vec![EdgeCrease { edge: [0, 1], sharpness: 2.5 }],
            vertex_creases: vec![VertexCrease { vertex: 1, sharpness: 5.625 }],
        }
    }
    #[test]
    fn mirror_merges_seams_preserves_creases_and_omits_coplanar_duplicate_faces() {
        let cage = reflect(half_quad(), 0, 0.001).unwrap();
        assert_eq!(cage.points.len(), 6);
        assert_eq!(cage.faces.len(), 2);
        assert_eq!(cage.edge_creases.len(), 2);
        assert_eq!(cage.vertex_creases.len(), 2);
        cage.validate().unwrap();
        let loose = reflect(half_quad(), 0, 0.0).unwrap();
        assert_eq!(loose.points.len(), 8);
        loose.validate().unwrap();
        let coplanar = reflect(half_quad(), 2, 0.001).unwrap();
        assert_eq!(coplanar.points.len(), 4);
        assert_eq!(coplanar.faces.len(), 1);
        coplanar.validate().unwrap();
        let xy = reflect(cage, 1, 0.001).unwrap();
        let xyz = reflect(xy, 2, 0.001).unwrap();
        assert_eq!(xyz.points.len(), 12);
        assert_eq!(xyz.faces.len(), 4);
        xyz.validate().unwrap();
    }
    #[test]
    fn mirror_bisect_preserves_quad_openings_and_qualifies_empty_sides() {
        for flip in [false, true] {
            let clipped = clip(half_quad(), 1, flip, 0.001).unwrap();
            assert_eq!(clipped.faces.len(), 1);
            assert_eq!(clipped.faces[0].len(), 4);
            assert!(clipped.points.iter().all(|p| if flip { p.y <= 0.0 } else { p.y >= 0.0 }));
            let mirrored = reflect(clipped, 1, 0.001).unwrap();
            assert_eq!(mirrored.points.len(), 6);
            assert_eq!(mirrored.faces.len(), 2);
            mirrored.validate().unwrap();
        }
        let empty = clip(half_quad(), 0, true, 0.001).unwrap();
        assert!(empty.faces.is_empty());
        assert!(empty.validate().is_err());
    }

    #[test]
    fn mirror_seam_merge_is_strictly_below_the_distance_limit() {
        for (distance, points) in [(0.00049, 6), (0.0005, 8), (0.00051, 8)] {
            let mut cage = half_quad();
            cage.points[0].x = distance;
            cage.points[3].x = distance;
            let mirrored = reflect(cage, 0, 0.0005).unwrap();
            assert_eq!(mirrored.points.len(), points);
            assert_eq!(mirrored.points[0].x, if points == 6 { 0.0 } else { distance });
            mirrored.validate().unwrap();
        }
    }

    #[test]
    fn bisect_snaps_both_sides_even_with_a_smaller_merge_tolerance() {
        let mut cage = half_quad();
        cage.points[0].x = -0.0008;
        cage.points[3].x = 0.0008;
        let clipped = clip(cage, 0, false, 0.001).unwrap();
        assert_eq!(clipped.points[0].x, 0.0);
        assert_eq!(clipped.points[3].x, 0.0);
        let reflected = reflect(clipped, 0, 0.0001).unwrap();
        assert_eq!(reflected.points.len(), 6);
        reflected.validate().unwrap();
    }
}
