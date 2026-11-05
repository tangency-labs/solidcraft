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

// Exact signed-volume ray crossing matching `solidcraft_geom::mesh::ray_crosses`.
fn ray_crosses_tri(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, o: vec3<f32>, dir: vec3<f32>) -> bool {
    let normal = cross(b - a, c - a);
    let along = dot(dir, normal);
    if along == 0.0 {
        return false;
    }
    let pa = a - o;
    let pb = b - o;
    let pc = c - o;
    let v_ab = dot(dir, cross(pa, pb));
    let v_bc = dot(dir, cross(pb, pc));
    let v_ca = dot(dir, cross(pc, pa));

    let s_ab = select(
        (a.x < b.x) || (a.x == b.x && (a.y < b.y || (a.y == b.y && a.z < b.z))),
        v_ab > 0.0,
        v_ab != 0.0,
    );
    let s_bc = select(
        (b.x < c.x) || (b.x == c.x && (b.y < c.y || (b.y == c.y && b.z < c.z))),
        v_bc > 0.0,
        v_bc != 0.0,
    );
    let s_ca = select(
        (c.x < a.x) || (c.x == a.x && (c.y < a.y || (c.y == a.y && c.z < a.z))),
        v_ca > 0.0,
        v_ca != 0.0,
    );

    if (s_ab != s_bc) || (s_ab != s_ca) {
        return false;
    }
    return (dot(pa, normal) / along) > 0.0;
}

// Ray-triangle intersection distance (> 1e-7) or -1.0 if missed.
fn ray_hit_tri(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, o: vec3<f32>, dir: vec3<f32>) -> f32 {
    let e1 = b - a;
    let e2 = c - a;
    let h = cross(dir, e2);
    let det = dot(e1, h);
    if abs(det) < 1e-12 {
        return -1.0;
    }
    let inv_det = 1.0 / det;
    let s = o - a;
    let u = inv_det * dot(s, h);
    if u < 0.0 || u > 1.0 {
        return -1.0;
    }
    let q = cross(s, e1);
    let v = inv_det * dot(dir, q);
    if v < 0.0 || (u + v) > 1.0 {
        return -1.0;
    }
    let t = inv_det * dot(e2, q);
    return select(-1.0, t, t > 1e-7);
}

fn ray_aabb_dist(o: vec3<f32>, inv_d: vec3<f32>, n: BvhNode) -> f32 {
    let tx1 = (n.min_x - o.x) * inv_d.x;
    let tx2 = (n.max_x - o.x) * inv_d.x;
    var tmin = min(tx1, tx2);
    var tmax = max(tx1, tx2);

    let ty1 = (n.min_y - o.y) * inv_d.y;
    let ty2 = (n.max_y - o.y) * inv_d.y;
    tmin = max(tmin, min(ty1, ty2));
    tmax = min(tmax, max(ty1, ty2));

    let tz1 = (n.min_z - o.z) * inv_d.z;
    let tz2 = (n.max_z - o.z) * inv_d.z;
    tmin = max(tmin, min(tz1, tz2));
    tmax = min(tmax, max(tz1, tz2));

    if tmax >= max(tmin, 0.0) {
        return max(tmin, 0.0);
    }
    return -1.0;
}

// ── Kernel 2: Tiled Point-in-Solid Classification ─────────────────────────

struct MeshQueryParams {
    num_queries: u32,
    num_tris: u32,
    num_bvh_nodes: u32,
    pad: u32,
}

@group(0) @binding(0) var<uniform> mq_params: MeshQueryParams;
@group(0) @binding(1) var<storage, read> mq_points: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> mq_tris: array<GpuTri>;
@group(0) @binding(3) var<storage, read> mq_bvh: array<BvhNode>;
@group(0) @binding(4) var<storage, read_write> mq_out_u32: array<u32>;

var<workgroup> shared_tris: array<GpuTri, 128>;

@compute @workgroup_size(128)
fn classify_points_mesh(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let q_idx = gid.x;
    let tid = lid.x;
    let is_active = q_idx < mq_params.num_queries;
    let p = select(vec3<f32>(0.0), mq_points[min(q_idx, max(mq_params.num_queries, 1u) - 1u)].xyz, is_active);

    // Three skewed ray directions matching `solidcraft_geom::mesh::RAY_DIRS`.
    let d0 = vec3<f32>(0.5773, 0.5774, 0.5776);
    let d1 = vec3<f32>(-0.6123, 0.3141, 0.7254);
    let d2 = vec3<f32>(0.2718, -0.8414, 0.4673);

    var c0: u32 = 0u;
    var c1: u32 = 0u;
    var c2: u32 = 0u;

    let num_tiles = (mq_params.num_tris + 127u) / 128u;
    for (var tile: u32 = 0u; tile < num_tiles; tile = tile + 1u) {
        let tri_idx = tile * 128u + tid;
        if tri_idx < mq_params.num_tris {
            shared_tris[tid] = mq_tris[tri_idx];
        }
        workgroupBarrier();

        if is_active {
            let count = min(128u, mq_params.num_tris - tile * 128u);
            for (var k: u32 = 0u; k < count; k = k + 1u) {
                let tri = shared_tris[k];
                let a = tri.v0.xyz;
                let b = tri.v1.xyz;
                let c = tri.v2.xyz;
                if ray_crosses_tri(a, b, c, p, d0) { c0 += 1u; }
                if ray_crosses_tri(a, b, c, p, d1) { c1 += 1u; }
                if ray_crosses_tri(a, b, c, p, d2) { c2 += 1u; }
            }
        }
        workgroupBarrier();
    }

    if is_active {
        let votes = (c0 & 1u) + (c1 & 1u) + (c2 & 1u);
        mq_out_u32[q_idx] = select(0u, 1u, votes >= 2u);
    }
}

// ── Kernel 3: BVH-Accelerated Point-in-Solid Classification (RT/BVH Tier) ──

fn bvh_ray_crossings(p: vec3<f32>, dir: vec3<f32>) -> u32 {
    if mq_params.num_bvh_nodes == 0u {
        return 0u;
    }
    let inv_d = vec3<f32>(1.0 / dir.x, 1.0 / dir.y, 1.0 / dir.z);
    var crossings: u32 = 0u;
    var stack: array<u32, 64>;
    var sp: u32 = 1u;
    stack[0] = 0u;

    while sp > 0u {
        sp -= 1u;
        let node = mq_bvh[stack[sp]];
        if ray_aabb_dist(p, inv_d, node) < 0.0 {
            continue;
        }
        if (node.right_or_count & 0x80000000u) != 0u {
            let first = node.left_or_prim;
            let count = node.right_or_count & 0x7FFFFFFFu;
            for (var i: u32 = 0u; i < count; i = i + 1u) {
                let tri = mq_tris[first + i];
                if ray_crosses_tri(tri.v0.xyz, tri.v1.xyz, tri.v2.xyz, p, dir) {
                    crossings += 1u;
                }
            }
        } else if sp + 2u <= 64u {
            stack[sp] = node.right_or_count;
            stack[sp + 1u] = node.left_or_prim;
            sp += 2u;
        }
    }
    return crossings;
}

@compute @workgroup_size(128)
fn classify_points_bvh(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_idx = gid.x;
    if q_idx >= mq_params.num_queries {
        return;
    }
    let p = mq_points[q_idx].xyz;
    let d0 = vec3<f32>(0.5773, 0.5774, 0.5776);
    let d1 = vec3<f32>(-0.6123, 0.3141, 0.7254);
    let d2 = vec3<f32>(0.2718, -0.8414, 0.4673);

    let c0 = bvh_ray_crossings(p, d0);
    let c1 = bvh_ray_crossings(p, d1);
    let c2 = bvh_ray_crossings(p, d2);
    let votes = (c0 & 1u) + (c1 & 1u) + (c2 & 1u);
    mq_out_u32[q_idx] = select(0u, 1u, votes >= 2u);
}

// ── Kernel 4: BVH-Accelerated Raycast (Closest Hit + Triangle Index) ──────

struct GpuRay {
    origin: vec4<f32>,
    dir: vec4<f32>,
}

struct GpuRayHit {
    t: f32,
    tri_idx: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(0) var<uniform> rc_params: MeshQueryParams;
@group(0) @binding(1) var<storage, read> rc_rays: array<GpuRay>;
@group(0) @binding(2) var<storage, read> rc_tris: array<GpuTri>;
@group(0) @binding(3) var<storage, read> rc_bvh: array<BvhNode>;
@group(0) @binding(4) var<storage, read_write> rc_hits: array<GpuRayHit>;

@compute @workgroup_size(128)
fn raycast_mesh_bvh(@builtin(global_invocation_id) gid: vec3<u32>) {
    let r_idx = gid.x;
    if r_idx >= rc_params.num_queries {
        return;
    }
    let ray = rc_rays[r_idx];
    let o = ray.origin.xyz;
    let d = ray.dir.xyz;
    let inv_d = vec3<f32>(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);

    var best_t: f32 = 1e30;
    var best_tri: u32 = 0xFFFFFFFFu;

    if rc_params.num_bvh_nodes > 0u {
        var stack: array<u32, 64>;
        var sp: u32 = 1u;
        stack[0] = 0u;

        while sp > 0u {
            sp -= 1u;
            let node = rc_bvh[stack[sp]];
            let t_box = ray_aabb_dist(o, inv_d, node);
            if t_box < 0.0 || t_box > best_t {
                continue;
            }
            if (node.right_or_count & 0x80000000u) != 0u {
                let first = node.left_or_prim;
                let count = node.right_or_count & 0x7FFFFFFFu;
                for (var i: u32 = 0u; i < count; i = i + 1u) {
                    let tri = rc_tris[first + i];
                    let t_hit = ray_hit_tri(tri.v0.xyz, tri.v1.xyz, tri.v2.xyz, o, d);
                    let orig_idx = bitcast<u32>(tri.v0.w);
                    if t_hit > 0.0 && (t_hit < best_t || (t_hit == best_t && orig_idx < best_tri)) {
                        best_t = t_hit;
                        best_tri = orig_idx;
                    }
                }
            } else if sp + 2u <= 64u {
                let l = node.left_or_prim;
                let r = node.right_or_count;
                let tl = ray_aabb_dist(o, inv_d, rc_bvh[l]);
                let tr = ray_aabb_dist(o, inv_d, rc_bvh[r]);
                if tl >= 0.0 && tr >= 0.0 {
                    if tl <= tr {
                        stack[sp] = r;
                        stack[sp + 1u] = l;
                    } else {
                        stack[sp] = l;
                        stack[sp + 1u] = r;
                    }
                    sp += 2u;
                } else if tl >= 0.0 {
                    stack[sp] = l;
                    sp += 1u;
                } else if tr >= 0.0 {
                    stack[sp] = r;
                    sp += 1u;
                }
            }
        }
    } else {
        for (var i: u32 = 0u; i < rc_params.num_tris; i = i + 1u) {
            let tri = rc_tris[i];
            let t_hit = ray_hit_tri(tri.v0.xyz, tri.v1.xyz, tri.v2.xyz, o, d);
            if t_hit > 0.0 && t_hit < best_t {
                best_t = t_hit;
                best_tri = i;
            }
        }
    }

    rc_hits[r_idx] = GpuRayHit(select(-1.0, best_t, best_tri != 0xFFFFFFFFu), best_tri, 0u, 0u);
}

// ── Kernel 5: Point-to-Mesh Minimum Squared Distance ──────────────────────

@group(0) @binding(0) var<uniform> pd_params: MeshQueryParams;
@group(0) @binding(1) var<storage, read> pd_points: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> pd_tris: array<GpuTri>;
@group(0) @binding(3) var<storage, read_write> pd_dist_sq: array<f32>;

fn closest_point_on_tri_dist_sq(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> f32 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return dot(ap, ap);
    }
    let bp = p - b;
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return dot(bp, bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        let diff = p - (a + ab * v);
        return dot(diff, diff);
    }
    let cp = p - c;
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return dot(cp, cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        let diff = p - (a + ac * w);
        return dot(diff, diff);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let diff = p - (b + (c - b) * w);
        return dot(diff, diff);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    let diff = p - (a + ab * v + ac * w);
    return dot(diff, diff);
}

@compute @workgroup_size(128)
fn point_mesh_distance(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let q_idx = gid.x;
    let tid = lid.x;
    let is_active = q_idx < pd_params.num_queries;
    let p = select(vec3<f32>(0.0), pd_points[min(q_idx, max(pd_params.num_queries, 1u) - 1u)].xyz, is_active);

    var best_d2: f32 = 1e30;
    let num_tiles = (pd_params.num_tris + 127u) / 128u;
    for (var tile: u32 = 0u; tile < num_tiles; tile = tile + 1u) {
        let tri_idx = tile * 128u + tid;
        if tri_idx < pd_params.num_tris {
            shared_tris[tid] = pd_tris[tri_idx];
        }
        workgroupBarrier();

        if is_active {
            let count = min(128u, pd_params.num_tris - tile * 128u);
            for (var k: u32 = 0u; k < count; k = k + 1u) {
                let tri = shared_tris[k];
                let d2 = closest_point_on_tri_dist_sq(p, tri.v0.xyz, tri.v1.xyz, tri.v2.xyz);
                best_d2 = min(best_d2, d2);
            }
        }
        workgroupBarrier();
    }

    if is_active {
        pd_dist_sq[q_idx] = best_d2;
    }
}

// ── Kernel 6: Divergence Theorem Mass Properties Workgroup Reduction ──────

struct MassReduceOut {
    v6_area: vec4<f32>, // x = v6 (6 * volume), y = surface area, z = 0, w = 0
    moment1: vec4<f32>, // xyz = sum((a + b + c) * det), w = 0
}

@group(0) @binding(0) var<uniform> mp_params: MeshQueryParams;
@group(0) @binding(1) var<storage, read> mp_tris: array<GpuTri>;
@group(0) @binding(2) var<storage, read_write> mp_partials: array<MassReduceOut>;

var<workgroup> wg_v6: array<f32, 256>;
var<workgroup> wg_area: array<f32, 256>;
var<workgroup> wg_mom: array<vec3<f32>, 256>;

@compute @workgroup_size(256)
fn mass_properties_reduce(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let idx = gid.x;
    let tid = lid.x;

    var v6: f32 = 0.0;
    var area: f32 = 0.0;
    var mom = vec3<f32>(0.0);

    if idx < mp_params.num_tris {
        let p0 = mp_tris[0].v0.xyz;
        let tri = mp_tris[idx];
        let a = tri.v0.xyz - p0;
        let b = tri.v1.xyz - p0;
        let c = tri.v2.xyz - p0;
        let det = dot(a, cross(b, c));
        v6 = det;
        area = length(cross(b - a, c - a)) * 0.5;
        mom = (a + b + c) * det;
    }

    wg_v6[tid] = v6;
    wg_area[tid] = area;
    wg_mom[tid] = mom;
    workgroupBarrier();

    var stride: u32 = 128u;
    while stride > 0u {
        if tid < stride {
            wg_v6[tid] += wg_v6[tid + stride];
            wg_area[tid] += wg_area[tid + stride];
            wg_mom[tid] += wg_mom[tid + stride];
        }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if tid == 0u {
        mp_partials[wid.x] = MassReduceOut(
            vec4<f32>(wg_v6[0], wg_area[0], 0.0, 0.0),
            vec4<f32>(wg_mom[0], 0.0),
        );
    }
}
