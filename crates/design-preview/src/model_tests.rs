use super::*;

/// An `SCMESH01` blob of `positions` (x, y, z triples) and `triangles` (index triples).
fn blob(positions: &[f64], triangles: &[u32]) -> Vec<u8> {
    let mut bytes = b"SCMESH01".to_vec();
    bytes.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(triangles.len() as u64).to_le_bytes());
    positions.iter().for_each(|v| bytes.extend_from_slice(&v.to_le_bytes()));
    triangles.iter().for_each(|i| bytes.extend_from_slice(&i.to_le_bytes()));
    bytes
}

/// The same with a normal per triangle corner (`SCMESH02`).
fn shaded(positions: &[f64], triangles: &[u32], normals: &[f64]) -> Vec<u8> {
    let mut bytes = b"SCMESH02".to_vec();
    bytes.extend_from_slice(&(positions.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(triangles.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(normals.len() as u64).to_le_bytes());
    positions.iter().for_each(|v| bytes.extend_from_slice(&v.to_le_bytes()));
    triangles.iter().for_each(|i| bytes.extend_from_slice(&i.to_le_bytes()));
    normals.iter().for_each(|v| bytes.extend_from_slice(&v.to_le_bytes()));
    bytes
}

fn manifest(entries: &[(usize, usize, Option<[f32; 3]>)]) -> Vec<u8> {
    let meshes: Vec<_> =
        entries.iter().map(|(offset, length, color)| serde_json::json!({"name":"Body","offset":offset,"length":length,"color":color})).collect();
    serde_json::to_vec(&serde_json::json!({"format":"solidcraft-preview/1","build":"0.1.0","document":"x","payload":"y","meshes":meshes})).unwrap()
}

/// A unit right triangle in the XY plane, at `x` along X.
fn triangle(x: f64) -> Vec<u8> {
    blob(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x, 1.0, 0.0], &[0, 1, 2])
}

#[test]
fn saved_display_meshes_become_centred_triangle_corners() {
    let (a, b) = (triangle(0.0), triangle(10.0));
    let data = [a.clone(), b.clone()].concat();
    let model = decode(&manifest(&[(0, a.len(), Some([1.0, 0.0, 0.0])), (a.len(), b.len(), None)]), &data).unwrap();
    assert_eq!(model.vertices.len(), 6);
    // The box runs from 0 to 11 in X and 0 to 1 in Y: its middle is (5.5, 0.5, 0).
    assert_eq!(model.vertices[0].position, [-5.5, -0.5, 0.0]);
    assert_eq!(model.vertices[0].normal, [0.0, 0.0, 1.0]);
    assert_eq!(model.vertices[0].color, [1.0, 0.0, 0.0]);
    assert_eq!(model.vertices[3].color, DEFAULT_COLOR);
    assert!((model.radius - (5.5_f32.powi(2) + 0.25).sqrt()).abs() < 1e-5);
    assert_eq!(std::mem::size_of::<Vertex>(), 36, "three packed_float3 per corner");
}

#[test]
fn corner_normals_are_kept() {
    let normals = [0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0];
    let mesh = shaded(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2], &normals);
    let model = decode(&manifest(&[(0, mesh.len(), None)]), &mesh).unwrap();
    let got: Vec<_> = model.vertices.iter().map(|v| v.normal).collect();
    assert_eq!(got, [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
}

#[test]
fn hostile_display_caches_are_misses() {
    let good = triangle(0.0);
    let ok = |m: &[u8], d: &[u8]| decode(m, d).is_some();
    assert!(ok(&manifest(&[(0, good.len(), None)]), &good));
    // Wrong format, no bodies, a gap or overlap, a slice past the end.
    assert!(!ok(&serde_json::to_vec(&serde_json::json!({"format":"other/1","meshes":[{"offset":0,"length":good.len()}]})).unwrap(), &good));
    assert!(!ok(&manifest(&[]), &good));
    assert!(!ok(&manifest(&[(1, good.len() - 1, None)]), &good));
    assert!(!ok(&manifest(&[(0, good.len() + 1, None)]), &good));
    assert!(!ok(b"{", &good));
    // An index past the vertices, a non-finite or huge coordinate, a length that lies.
    assert!(!ok(&manifest(&[(0, good.len(), None)]), &blob(&[0.0; 9], &[0, 1, 3])));
    assert!(!ok(&manifest(&[(0, good.len(), None)]), &blob(&[f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2])));
    assert!(!ok(&manifest(&[(0, good.len(), None)]), &blob(&[2e9, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2])));
    let mut lying = good.clone();
    lying[8] = 200;
    assert!(!ok(&manifest(&[(0, lying.len(), None)]), &lying));
    assert!(!ok(&manifest(&[(0, 8, None)]), b"SCMESH09"));
    // A colour outside 0..1 falls back to the default rather than failing.
    let model = decode(&manifest(&[(0, good.len(), Some([2.0, 0.0, 0.0]))]), &good).unwrap();
    assert_eq!(model.vertices[0].color, DEFAULT_COLOR);
    // Over the triangle budget.
    let many: Vec<u32> = (0..(MAX_TRIANGLES as u32 + 1) * 3).map(|i| i % 3).collect();
    let big = blob(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &many);
    assert!(!ok(&manifest(&[(0, big.len(), None)]), &big));
}

/// Where `clip` takes a model point, after the perspective divide (w is always 1 here).
fn project(m: &Matrices, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| (0..3).map(|col| m.clip[col * 4 + row] * p[col]).sum::<f32>() + m.clip[12 + row])
}

#[test]
fn the_orbit_starts_isometric_and_keeps_the_model_in_view() {
    let orbit = Orbit::default();
    let m = orbit.matrices(10.0, 1.5);
    assert_eq!(project(&m, [0.0; 3]), [0.0, 0.0, 0.5]);
    // Z is up on screen; the whole sphere is inside the view and the depth range.
    assert!(project(&m, [0.0, 0.0, 10.0])[1] > 0.0);
    for p in [[10.0, 0.0, 0.0], [0.0, -10.0, 0.0], [0.0, 0.0, -10.0], [-7.0, 7.0, 0.0]] {
        let [x, y, z] = project(&m, p);
        assert!(x.abs() < 1.0 && y.abs() < 1.0 && (0.0..=1.0).contains(&z), "{p:?} -> {x} {y} {z}");
    }
    // The isometric eye looks from +X, -Y, +Z: that corner is nearest.
    assert!(project(&m, [5.0, -5.0, 5.0])[2] < project(&m, [-5.0, 5.0, -5.0])[2]);
    // A tall, narrow view still fits the sphere across.
    let narrow = orbit.matrices(10.0, 0.5);
    assert!(project(&narrow, [7.0, 7.0, 0.0])[0].abs() < 1.0);
}

#[test]
fn dragging_turns_freely_over_the_top_and_zoom_is_clamped() {
    let mut orbit = Orbit::default();
    let m = orbit.matrices(10.0, 1.0);
    let front = [m.view[2], m.view[6], m.view[10]].map(|v| v * 5.0); // the point nearest the eye
    let before = project(&m, front);
    // Dragging right moves the front of the model right; dragging up moves it up.
    orbit.drag(20.0, 0.0);
    assert!(project(&orbit.matrices(10.0, 1.0), front)[0] > before[0]);
    let mut up = Orbit::default();
    up.drag(0.0, 20.0);
    assert!(project(&up.matrices(10.0, 1.0), front)[1] > before[1]);
    // A long vertical drag goes over the top: the model ends upside down, axes still square.
    let mut over = Orbit::default();
    for _ in 0..157 {
        over.drag(0.0, 2.0);
    }
    assert!(over.axes[1][2] < 0.0, "Z points down on screen after half a turn: {:?}", over.axes);
    for (i, a) in over.axes.iter().enumerate() {
        for (j, b) in over.axes.iter().enumerate() {
            let d: f32 = (0..3).map(|k| a[k] * b[k]).sum();
            assert!((d - if i == j { 1.0 } else { 0.0 }).abs() < 1e-4);
        }
    }
    // A full roll comes back to the start.
    let mut rolled = Orbit::default();
    (0..4).for_each(|_| rolled.roll(std::f32::consts::FRAC_PI_2));
    assert!((0..3).all(|i| (0..3).all(|k| (rolled.axes[i][k] - Orbit::default().axes[i][k]).abs() < 1e-4)));
    // Hostile input changes nothing.
    let before = orbit;
    orbit.drag(f32::NAN, 1.0);
    orbit.drag(0.0, 0.0);
    orbit.roll(f32::INFINITY);
    orbit.zoom_by(f32::INFINITY);
    orbit.zoom_by(-1.0);
    assert_eq!(orbit, before);
    orbit.zoom_by(1e9);
    assert_eq!(orbit.zoom, 50.0);
    // Hostile sizes still give finite matrices.
    assert!(orbit.matrices(0.0, f32::NAN).clip.iter().all(|v| v.is_finite()));
}
