use super::{MAX_BVH_DEPTH, Node, Triangle, build};
use solidcraft_geom::Vec3;
use solidcraft_render::{job::CancelToken, surface::OpaqueSurface};

// Triangle centres spread over 60 decades: a sixteen-bin SAH split on the
// centre range peels only the few largest triangles off at each level, so an
// unbounded build is far deeper than the shader's traversal stack allows.
fn decades(count: usize) -> Vec<Triangle> {
    let surface = OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 0.5, 1.5).unwrap();
    (0..count)
        .map(|i| {
            let x = 10f64.powf(-30.0 + 60.0 * i as f64 / count as f64);
            Triangle {
                p: [Vec3::new(x, 0., 0.), Vec3::new(x * 1.5, 0., 0.), Vec3::new(x, x, x)],
                indices: [0, 1, 2].map(|c| (3 * i + c) as u32),
                surface,
                transmission: None,
                medium: 0,
            }
        })
        .collect()
}

#[test]
fn sah_tree_respects_the_shader_stack_depth_and_covers_every_triangle_once() {
    let count = 2048;
    let mut triangles = decades(count);
    let mut nodes = vec![Node::default()];
    build(&mut triangles, 0, &mut nodes, 0, 0, &CancelToken::default()).unwrap();
    assert!(nodes.len() < 2 * count, "{} nodes", nodes.len());
    let mut covered = vec![0; count];
    let mut deepest = 0;
    let mut pending = vec![(0usize, 0usize)];
    while let Some((index, depth)) = pending.pop() {
        let node = &nodes[index];
        let (first, size) = (node.first as usize, node.count as usize);
        deepest = deepest.max(depth);
        if size == 0 {
            for child in [first, first + 1] {
                let inner = &nodes[child];
                for axis in 0..3 {
                    assert!(node.lo[axis] <= inner.lo[axis] && inner.hi[axis] <= node.hi[axis], "child box escapes its parent");
                }
                pending.push((child, depth + 1));
            }
            continue;
        }
        assert!(size <= 4);
        for (slot, triangle) in covered[first..first + size].iter_mut().zip(&triangles[first..first + size]) {
            *slot += 1;
            for p in triangle.p {
                for (axis, value) in [p.x as f32, p.y as f32, p.z as f32].into_iter().enumerate() {
                    assert!(node.lo[axis] <= value && value <= node.hi[axis], "triangle escapes its leaf");
                }
            }
        }
    }
    assert!(covered.iter().all(|&c| c == 1), "{covered:?}");
    assert!(deepest <= MAX_BVH_DEPTH, "depth {deepest}");
    // Covering 2048 triangles in leaves of four needs at least nine levels.
    assert!(deepest >= 9, "depth {deepest}");
}
