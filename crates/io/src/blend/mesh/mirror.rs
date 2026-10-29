//! Mirror geometry is evaluated on the control cage before subdivision. All
//! switches come from the file's self-described record, not evaluated triangles.
use super::*;

fn coordinate(p: Vec3, axis: usize) -> f64 {
    match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}
fn point(m: Matrix, p: Vec3) -> Result<Vec3> {
    let v = [p.x, p.y, p.z, 1.];
    let mut out = [0.; 3];
    for (i, n) in out.iter_mut().enumerate() {
        *n = m[i].iter().zip(v).map(|(a, b)| a * b).sum();
    }
    let p = Vec3::new(out[0], out[1], out[2]);
    if !p.is_finite() {
        return Err(bad("non-finite Mirror transform"));
    }
    Ok(p)
}
fn inverse(m: Matrix) -> Result<Matrix> {
    let mut a = [[0.; 8]; 4];
    for i in 0..4 {
        for j in 0..4 {
            a[i][j] = m[i][j];
        }
        a[i][i + 4] = 1.;
    }
    for column in 0..4 {
        let pivot = (column..4).max_by(|&a0, &b| a[a0][column].abs().total_cmp(&a[b][column].abs())).ok_or_else(|| bad("Mirror matrix pivot"))?;
        if !a[pivot][column].is_finite() || a[pivot][column].abs() < 1e-15 {
            return Err(bad("singular Mirror frame"));
        }
        a.swap(pivot, column);
        let n = a[column][column];
        for v in &mut a[column] {
            *v /= n;
        }
        let row = a[column];
        for (i, r) in a.iter_mut().enumerate() {
            if i != column {
                let factor = r[column];
                for j in 0..8 {
                    r[j] -= factor * row[j];
                }
            }
        }
    }
    let mut out = identity();
    for i in 0..4 {
        out[i].copy_from_slice(&a[i][4..8]);
    }
    Ok(out)
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
    let mut out = Cage { points: cage.points.clone(), ..Default::default() };
    let mut cuts = BTreeMap::new();
    let distance = |p: Vec3| coordinate(p, axis) * if flip { -1. } else { 1. };
    for face in &cage.faces {
        let mut polygon = Vec::new();
        for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let pa = cage.point(a).map_err(|e| bad(e.to_string()))?;
            let pb = cage.point(b).map_err(|e| bad(e.to_string()))?;
            let da = distance(pa);
            let db = distance(pb);
            let inside = da >= -threshold;
            let next_inside = db >= -threshold;
            if inside {
                polygon.push(a);
            }
            if inside != next_inside {
                let key = (a.min(b), a.max(b));
                let id = if da.abs() <= 1e-12 {
                    a
                } else if db.abs() <= 1e-12 {
                    b
                } else if let Some(&i) = cuts.get(&key) {
                    i
                } else {
                    if out.points.len() >= MAX_POINTS {
                        return Err(bad("Mirror bisect point budget exceeded"));
                    }
                    let t = (da / (da - db)).clamp(0., 1.);
                    let p = pa + (pb - pa) * t;
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
        let edge = if da >= -threshold && db >= -threshold {
            Some([a, b])
        } else if let Some(&i) = cuts.get(&(a.min(b), a.max(b))) {
            Some([if da >= -threshold { a } else { b }, i])
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
fn reflect(cage: Cage, axis: usize, merge: bool, tolerance: f64) -> Result<Cage> {
    if cage.faces.len().saturating_mul(2) > MAX_FACES || cage.points.len().saturating_mul(2) > MAX_POINTS {
        return Err(bad("Mirror exceeds the editable cage budget"));
    }
    let mut out = cage.clone();
    let mut ids = Vec::with_capacity(cage.points.len());
    for (i, &p) in cage.points.iter().enumerate() {
        let q = match axis {
            0 => Vec3::new(-p.x, p.y, p.z),
            1 => Vec3::new(p.x, -p.y, p.z),
            _ => Vec3::new(p.x, p.y, -p.z),
        };
        if merge && coordinate(p, axis).abs() <= tolerance {
            let center = (p + q) * 0.5;
            *out.points.get_mut(i).ok_or_else(|| bad("Mirror seam index"))? = center;
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
        let target = object.file.view(object.file.data(mirror)?, "Object")?;
        let other = world(object.file, target, &mut BTreeSet::from([mirror]), 0)?;
        mul(inverse(other)?, own)
    };
    let back = inverse(frame)?;
    for p in &mut cage.points {
        *p = point(frame, *p)?;
    }
    for axis in 0..3 {
        if flags & (8 << axis) == 0 {
            continue;
        }
        if flags & (256 << axis) != 0 {
            cage = clip(cage, axis, flags & (2048 << axis) != 0, threshold)?;
        }
        cage = reflect(cage, axis, flags & 128 == 0, tolerance)?;
    }
    for p in &mut cage.points {
        *p = point(back, *p)?;
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
        let cage = reflect(half_quad(), 0, true, 0.001).unwrap();
        assert_eq!(cage.points.len(), 6);
        assert_eq!(cage.faces.len(), 2);
        assert_eq!(cage.edge_creases.len(), 2);
        assert_eq!(cage.vertex_creases.len(), 2);
        cage.validate().unwrap();
        let loose = reflect(half_quad(), 0, false, 0.001).unwrap();
        assert_eq!(loose.points.len(), 8);
        loose.validate().unwrap();
        let coplanar = reflect(half_quad(), 2, true, 0.001).unwrap();
        assert_eq!(coplanar.points.len(), 4);
        assert_eq!(coplanar.faces.len(), 1);
        coplanar.validate().unwrap();
        let xy = reflect(cage, 1, true, 0.001).unwrap();
        let xyz = reflect(xy, 2, true, 0.001).unwrap();
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
            let mirrored = reflect(clipped, 1, true, 0.001).unwrap();
            assert_eq!(mirrored.points.len(), 6);
            assert_eq!(mirrored.faces.len(), 2);
            mirrored.validate().unwrap();
        }
        let empty = clip(half_quad(), 0, true, 0.001).unwrap();
        assert!(empty.faces.is_empty());
        assert!(empty.validate().is_err());
    }
}
