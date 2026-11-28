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

/// A model point's coordinates in the view's frame (right, up, towards the eye).
fn in_view(orbit: &Orbit, p: [f32; 3]) -> [f32; 3] {
    orbit.axes.map(|axis| axis.iter().zip(p).map(|(a, b)| a * b).sum())
}

#[test]
fn the_model_turns_as_a_ball_under_the_pointer_about_every_axis() {
    let (w, h) = (400.0, 300.0);
    let start = Orbit::default();
    // The model's point on the ball under the middle of the view faces the eye.
    let front: [f32; 3] = std::array::from_fn(|j| start.axes[2][j]);
    let grab = start.grab(200.0, 150.0, w, h).unwrap();
    // Dragging right and up carries that point right and up, after the pointer.
    let mut orbit = start;
    orbit.drag_to(&grab, 260.0, 190.0, w, h);
    let [x, y, z] = in_view(&orbit, front);
    assert!(x > 0.0 && y > 0.0 && z > 0.0, "{x} {y} {z}");
    assert!((y / x - 40.0 / 60.0).abs() < 1e-3, "along the drag");
    // Back to where the drag started, the model is where it was.
    orbit.drag_to(&grab, 200.0, 150.0, w, h);
    assert_eq!(orbit.axes, start.axes);
    // The model turns twice as far as the ball under the pointer: a drag a fifth of the view
    // across turns that front point by twice the ball's angle.
    let mut wide = start;
    wide.drag_to(&grab, 280.0, 150.0, w, h);
    let [x, _, z] = in_view(&wide, front);
    let ball = (80.0_f32 / 150.0).asin();
    assert!((x.atan2(z) - 2.0 * ball).abs() < 1e-3, "{x} {z}");
    // Two drags from the middle to the top show the model upside down: no pole stops it.
    let mut again = start;
    again.drag_to(&grab, 200.0, 290.0, w, h);
    let next = again.grab(200.0, 150.0, w, h).unwrap();
    again.drag_to(&next, 200.0, 290.0, w, h);
    assert!(in_view(&again, [0.0, 0.0, 1.0])[1] < 0.0, "Z points down after going over");
    // A drag around the edge turns the model mostly about the line of sight (roll).
    let mut rolled = start;
    let edge = rolled.grab(390.0, 150.0, w, h).unwrap();
    // The same distance from the middle, an eighth of the way round anticlockwise.
    rolled.drag_to(&edge, 200.0 + 190.0 * std::f32::consts::FRAC_1_SQRT_2, 150.0 + 190.0 * std::f32::consts::FRAC_1_SQRT_2, w, h);
    let before = in_view(&start, [0.0, 0.0, 1.0]);
    let after = in_view(&rolled, [0.0, 0.0, 1.0]);
    // Z on screen has turned anticlockwise about the middle, as the pointer did.
    let turned = after[1].atan2(after[0]) - before[1].atan2(before[0]);
    assert!(turned > 0.3, "{before:?} -> {after:?}");
    // The axes stay square however far it turns.
    for (i, a) in again.axes.iter().enumerate() {
        for (j, b) in again.axes.iter().enumerate() {
            let d: f32 = (0..3).map(|k| a[k] * b[k]).sum();
            assert!((d - if i == j { 1.0 } else { 0.0 }).abs() < 1e-4);
        }
    }
    // Hostile input changes nothing.
    assert!(start.grab(f32::NAN, 1.0, w, h).is_none());
    assert!(start.grab(1.0, 1.0, 0.0, h).is_none());
    let mut same = start;
    same.drag_to(&grab, f32::INFINITY, 1.0, w, h);
    same.drag_to(&grab, 1.0, 1.0, w, -1.0);
    assert_eq!(same, start);
}

#[test]
fn zoom_is_clamped_and_hostile_sizes_give_finite_matrices() {
    let mut orbit = Orbit::default();
    let before = orbit;
    orbit.zoom_by(f32::INFINITY);
    orbit.zoom_by(-1.0);
    assert_eq!(orbit, before);
    orbit.zoom_by(1e9);
    assert_eq!(orbit.zoom, 50.0);
    assert!(orbit.matrices(0.0, f32::NAN).clip.iter().all(|v| v.is_finite()));
    orbit.target = [f32::NAN, 0.0, 0.0];
    orbit.zoom = f32::NAN;
    assert!(orbit.matrices(10.0, 1.5).clip.iter().all(|v| v.is_finite()));
}

#[test]
fn pan_follows_the_pointer_and_keeps_the_panned_point_centred() {
    let radius = 10.0;
    for (w, h) in [(800.0_f32, 400.0), (400.0, 800.0)] {
        let aspect = w / h;
        let mut orbit = Orbit::default();
        orbit.zoom_by(2.5);
        let before = project(&orbit.matrices(radius, aspect), [0.0; 3]);
        let pan = orbit.pan_start(100.0, 120.0, w, h).unwrap();
        let (dx, dy) = (80.0, -60.0);
        orbit.pan_to(&pan, 100.0 + dx, 120.0 + dy, w, h);
        let after = project(&orbit.matrices(radius, aspect), [0.0; 3]);
        // The model moves by the exact clip-space delta of the pointer drag.
        assert!(((after[0] - before[0]) - 2.0 * dx / w).abs() < 1e-4, "{w}x{h}: {before:?} -> {after:?}");
        assert!(((after[1] - before[1]) - 2.0 * dy / h).abs() < 1e-4, "{w}x{h}: {before:?} -> {after:?}");
        assert_eq!(after[2], 0.5, "panning leaves depth centred on the model");
        // Returning to where the pan started puts the model back where it was.
        orbit.pan_to(&pan, 100.0, 120.0, w, h);
        assert_eq!(orbit.target, [0.0; 3]);
    }
    // Zooming and orbiting after a pan keep the panned model point at the middle of the view.
    let mut orbit = Orbit::default();
    let pan = orbit.pan_start(200.0, 150.0, 400.0, 300.0).unwrap();
    orbit.pan_to(&pan, 280.0, 190.0, 400.0, 300.0);
    let focus = orbit.target.map(|v| v * radius);
    orbit.zoom_by(8.0);
    let at_zoom = project(&orbit.matrices(radius, 4.0 / 3.0), focus);
    assert!(at_zoom[0].abs() < 1e-4 && at_zoom[1].abs() < 1e-4, "{at_zoom:?}");
    let grab = orbit.grab(200.0, 150.0, 400.0, 300.0).unwrap();
    orbit.drag_to(&grab, 260.0, 210.0, 400.0, 300.0);
    let after_turn = project(&orbit.matrices(radius, 4.0 / 3.0), focus);
    assert!(after_turn[0].abs() < 1e-4 && after_turn[1].abs() < 1e-4, "{after_turn:?}");
    // Every corner of the model stays within the depth range even at the pan limit.
    let far = orbit.pan_start(200.0, 150.0, 400.0, 300.0).unwrap();
    orbit.pan_to(&far, 200.0 + 1e5, 150.0, 400.0, 300.0);
    let dist = ball::dot(orbit.target, orbit.target).sqrt();
    assert!((dist - MAX_PAN).abs() < 1e-4, "target clamped to {MAX_PAN}, got {dist}");
    let m = orbit.matrices(radius, 1.5);
    for p in [[radius, 0.0, 0.0], [0.0, -radius, 0.0], [0.0, 0.0, radius], [0.0, 0.0, -radius]] {
        let z = project(&m, p)[2];
        assert!((0.0..=1.0).contains(&z), "{p:?} clipped at depth {z}");
    }
    // Re-anchoring an active pan after zoom keeps the next drag 1:1 from the new scale without jumping.
    let mut zoom_pan = Orbit::default();
    let p1 = zoom_pan.pan_start(200.0, 150.0, 400.0, 300.0).unwrap();
    zoom_pan.pan_to(&p1, 280.0, 150.0, 400.0, 300.0);
    zoom_pan.zoom_by(2.0);
    let before_next = project(&zoom_pan.matrices(radius, 4.0 / 3.0), [0.0; 3]);
    let p2 = zoom_pan.pan_start(280.0, 150.0, 400.0, 300.0).unwrap();
    zoom_pan.pan_to(&p2, 281.0, 150.0, 400.0, 300.0);
    let after_next = project(&zoom_pan.matrices(radius, 4.0 / 3.0), [0.0; 3]);
    assert!(((after_next[0] - before_next[0]) - 2.0 / 400.0).abs() < 1e-4, "{before_next:?} -> {after_next:?}");
    // Reset restores the default isometric view, target and zoom.
    orbit.reset();
    assert_eq!(orbit, Orbit::default());
    // Hostile input changes nothing.
    assert!(orbit.pan_start(f32::NAN, 1.0, 400.0, 300.0).is_none());
    assert!(orbit.pan_start(1.0, 1.0, 0.0, 300.0).is_none());
    assert!(orbit.pan_start(1.0, 1.0, f32::INFINITY, 300.0).is_none());
    let valid = orbit.pan_start(10.0, 10.0, 400.0, 300.0).unwrap();
    orbit.pan_to(&valid, f32::INFINITY, 10.0, 400.0, 300.0);
    orbit.pan_to(&valid, 20.0, 20.0, f32::INFINITY, 300.0);
    orbit.pan_to(&valid, 20.0, 20.0, 400.0, -1.0);
    assert_eq!(orbit, Orbit::default());
}
