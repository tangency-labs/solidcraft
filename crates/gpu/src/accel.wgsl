// Vulkan / wgpu compute kernels for geometric CAD acceleration:
// 1. `eval_bicubic_patches` — Tensor-product 4x4 bicubic Bezier/B-spline patch evaluation
// 2. `classify_points_mesh` — Tiled 3-ray majority-vote watertight point-in-solid classification
// 3. `classify_points_bvh` — BVH-accelerated 3-ray watertight point-in-solid classification
// 4. `raycast_mesh_bvh` — BVH-accelerated closest ray-triangle hit (distance + triangle index)
// 5. `point_mesh_distance` — Tiled minimum squared distance from query points to triangle mesh
// 6. `mass_properties_reduce` — Workgroup reduction for divergence-theorem volume, area & moments

// ── Shared Types ──────────────────────────────────────────────────────────

struct GpuVec4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

struct GpuTri {
    v0: vec4<f32>, // xyz = vertex 0, w = face_id as f32
    v1: vec4<f32>, // xyz = vertex 1
    v2: vec4<f32>, // xyz = vertex 2
}

struct BvhNode {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    left_or_prim: u32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
    right_or_count: u32, // MSB 0x80000000 set if leaf; low 31 bits = prim count
}

// ── Kernel 1: Bicubic Patch Evaluation (4x4 Control Net) ─────────────────

struct PatchEvalParams {
    num_evals: u32,
    num_patches: u32,
    pad0: u32,
    pad1: u32,
}

struct PatchUvQuery {
    u: f32,
    v: f32,
    patch_idx: u32,
    pad: u32,
}

struct PatchEvalOut {
    pos: vec4<f32>,  // xyz = position, w = 1.0
    norm: vec4<f32>, // xyz = unit normal, w = du x dv magnitude
}

@group(0) @binding(0) var<uniform> patch_params: PatchEvalParams;
// 16 homogeneous control points (x*w, y*w, z*w, w) per patch
@group(0) @binding(1) var<storage, read> patch_cpts: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> patch_queries: array<PatchUvQuery>;
@group(0) @binding(3) var<storage, read_write> patch_out: array<PatchEvalOut>;

fn bernstein3(t: f32) -> vec4<f32> {
    let it = 1.0 - t;
    let it2 = it * it;
    let t2 = t * t;
    return vec4<f32>(
        it2 * it,
        3.0 * t * it2,
        3.0 * t2 * it,
        t2 * t,
    );
}

fn bernstein3_deriv(t: f32) -> vec4<f32> {
    let it = 1.0 - t;
    return vec4<f32>(
        -3.0 * it * it,
        3.0 * it * (1.0 - 3.0 * t),
        3.0 * t * (2.0 - 3.0 * t),
        3.0 * t * t,
    );
}

@compute @workgroup_size(64)
fn eval_bicubic_patches(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= patch_params.num_evals {
        return;
    }
    let q = patch_queries[idx];
    let p_base = min(q.patch_idx, max(patch_params.num_patches, 1u) - 1u) * 16u;

    let bu = bernstein3(q.u);
    let dbu = bernstein3_deriv(q.u);
    let bv = bernstein3(q.v);
    let dbv = bernstein3_deriv(q.v);

    var h = vec4<f32>(0.0);
    var dhu = vec4<f32>(0.0);
    var dhv = vec4<f32>(0.0);

    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        let bu_i = bu[i];
        let dbu_i = dbu[i];
        for (var j: u32 = 0u; j < 4u; j = j + 1u) {
            let cp = patch_cpts[p_base + i * 4u + j];
            let bv_j = bv[j];
            let dbv_j = dbv[j];
            h += cp * (bu_i * bv_j);
            dhu += cp * (dbu_i * bv_j);
            dhv += cp * (bu_i * dbv_j);
        }
    }

    let inv_w = select(1.0, 1.0 / h.w, abs(h.w) > 1e-12);
    let pos = h.xyz * inv_w;
    let du = (dhu.xyz - pos * dhu.w) * inv_w;
    let dv = (dhv.xyz - pos * dhv.w) * inv_w;
    let cross_uv = cross(du, dv);
    let len_n = length(cross_uv);
    let norm = select(vec3<f32>(0.0, 0.0, 1.0), cross_uv / len_n, len_n > 1e-12);

    patch_out[idx] = PatchEvalOut(vec4<f32>(pos, 1.0), vec4<f32>(norm, len_n));
}

// ── Watertight Ray-Triangle Crossing & Hit Helpers ────────────────────────

