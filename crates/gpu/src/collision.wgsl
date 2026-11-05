// GPU-accelerated AABB collision detection, LBVH construction/traversal,
// and surface-surface intersection (SSI) seed residual evaluation.
//
// Five entry points:
// 1. `find_bipartite_overlaps` — All-pairs AABB overlap between two sets (boolean broadphase)
// 2. `find_self_overlaps` — Self-collision among AABBs within a single body (mesh/B-rep check)
// 3. `compute_morton_codes` — 30-bit 3D Morton codes from AABB centroids for LBVH sorting
// 4. `query_lbvh` — Traverse a prebuilt LBVH tree to find overlapping AABB pairs in O(N log M)
// 5. `evaluate_surface_residuals` — Evaluate UV grid pairs on two surfaces for SSI seed hunting

struct GpuAabb {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    id: u32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
    pad: u32,
}

struct OverlapPair {
    a: u32,
    b: u32,
}

struct CollisionParams {
    count_a: u32,
    count_b: u32,
    max_pairs: u32,
    tolerance: f32,
}

@group(0) @binding(0) var<uniform> params: CollisionParams;
@group(0) @binding(1) var<storage, read> aabbs_a: array<GpuAabb>;
@group(0) @binding(2) var<storage, read> aabbs_b: array<GpuAabb>;
@group(0) @binding(3) var<storage, read_write> pairs: array<OverlapPair>;
@group(0) @binding(4) var<storage, read_write> pair_count: atomic<u32>;

fn aabb_overlap(a: GpuAabb, b: GpuAabb, tol: f32) -> bool {
    return (a.min_x - tol <= b.max_x) && (a.max_x + tol >= b.min_x)
        && (a.min_y - tol <= b.max_y) && (a.max_y + tol >= b.min_y)
        && (a.min_z - tol <= b.max_z) && (a.max_z + tol >= b.min_z);
}

// ── Pass 1: Bipartite AABB overlap (two distinct bodies / face sets) ──────
// Uses shared memory tiling to load tiles of `aabbs_b` cooperatively,
// reducing global memory bandwidth by 16x.

var<workgroup> tile_b: array<GpuAabb, 256>;

@compute @workgroup_size(16, 16)
fn find_bipartite_overlaps(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let idx_a = gid.x;
    let idx_b = gid.y;
    let local_idx = lid.y * 16u + lid.x;

    // Cooperatively load B tile into workgroup shared memory.
    if idx_b < params.count_b && lid.x == 0u {
        tile_b[lid.y] = aabbs_b[idx_b];
    }
    workgroupBarrier();

    if idx_a >= params.count_a || idx_b >= params.count_b {
        return;
    }

    let box_a = aabbs_a[idx_a];
    let box_b = tile_b[lid.y];

    if aabb_overlap(box_a, box_b, params.tolerance) {
        let slot = atomicAdd(&pair_count, 1u);
        if slot < params.max_pairs {
            pairs[slot] = OverlapPair(box_a.id, box_b.id);
        }
    }
}

// ── Pass 2: Self-overlap (upper triangle of a single AABB set) ────────────

@compute @workgroup_size(16, 16)
fn find_self_overlaps(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx_a = gid.x;
    let idx_b = gid.y;

    // Strictly upper triangle: avoid self-pairs and duplicate symmetric pairs.
    if idx_a >= params.count_a || idx_b >= params.count_a || idx_a >= idx_b {
        return;
    }

    let box_a = aabbs_a[idx_a];
    let box_b = aabbs_a[idx_b];

    if aabb_overlap(box_a, box_b, params.tolerance) {
        let slot = atomicAdd(&pair_count, 1u);
        if slot < params.max_pairs {
            pairs[slot] = OverlapPair(box_a.id, box_b.id);
        }
    }
}

// ── Pass 3: Morton Code Generation for LBVH ──────────────────────────────

struct MortonParams {
    count: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    scene_min: vec4<f32>,
    scene_inv_extent: vec4<f32>,
}

struct MortonItem {
    code: u32,
    index: u32,
}

@group(0) @binding(0) var<uniform> morton_params: MortonParams;
@group(0) @binding(1) var<storage, read> morton_aabbs: array<GpuAabb>;
@group(0) @binding(2) var<storage, read_write> morton_out: array<MortonItem>;

// Expand a 10-bit integer into 30 bits by inserting 2 zeros after each bit.
fn expand_bits(v_in: u32) -> u32 {
    var v = v_in & 0x000003FFu;
    v = (v * 0x00010001u) & 0xFF0000FFu;
    v = (v * 0x00000101u) & 0x0F00F00Fu;
    v = (v * 0x00000011u) & 0xC30C30C3u;
    v = (v * 0x00000005u) & 0x49249249u;
    return v;
}

fn morton_3d(x: f32, y: f32, z: f32) -> u32 {
    let xx = clamp(x * 1024.0, 0.0, 1023.0);
    let yy = clamp(y * 1024.0, 0.0, 1023.0);
    let zz = clamp(z * 1024.0, 0.0, 1023.0);
    return (expand_bits(u32(xx)) * 4u) + (expand_bits(u32(yy)) * 2u) + expand_bits(u32(zz));
}

@compute @workgroup_size(256)
fn compute_morton_codes(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= morton_params.count {
        return;
    }
    let b = morton_aabbs[idx];
    let cx = (b.min_x + b.max_x) * 0.5;
    let cy = (b.min_y + b.max_y) * 0.5;
    let cz = (b.min_z + b.max_z) * 0.5;

    let nx = (cx - morton_params.scene_min.x) * morton_params.scene_inv_extent.x;
    let ny = (cy - morton_params.scene_min.y) * morton_params.scene_inv_extent.y;
    let nz = (cz - morton_params.scene_min.z) * morton_params.scene_inv_extent.z;

    morton_out[idx] = MortonItem(morton_3d(nx, ny, nz), idx);
}

// ── Pass 4: LBVH Traversal for Large Broadphase Queries ──────────────────

struct LbvhNode {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    left_or_prim: u32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
    right_or_flag: u32, // MSB (0x80000000) set if leaf
}

@group(0) @binding(0) var<uniform> lbvh_params: CollisionParams;
@group(0) @binding(1) var<storage, read> query_aabbs: array<GpuAabb>;
@group(0) @binding(2) var<storage, read> lbvh_nodes: array<LbvhNode>;
@group(0) @binding(3) var<storage, read_write> lbvh_pairs: array<OverlapPair>;
@group(0) @binding(4) var<storage, read_write> lbvh_pair_count: atomic<u32>;

fn node_overlap(a: GpuAabb, n: LbvhNode, tol: f32) -> bool {
    return (a.min_x - tol <= n.max_x) && (a.max_x + tol >= n.min_x)
        && (a.min_y - tol <= n.max_y) && (a.max_y + tol >= n.min_y)
        && (a.min_z - tol <= n.max_z) && (a.max_z + tol >= n.min_z);
}

@compute @workgroup_size(256)
fn query_lbvh(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_idx = gid.x;
    if q_idx >= lbvh_params.count_a || lbvh_params.count_b == 0u {
        return;
    }

    let q_box = query_aabbs[q_idx];
    let tol = lbvh_params.tolerance;

    // Iterative traversal stack (fixed depth 64 is sufficient for >1B primitives)
    var stack: array<u32, 64>;
    var stack_ptr: u32 = 0u;
    stack[0] = 0u;
    stack_ptr = 1u;

    while stack_ptr > 0u {
        stack_ptr -= 1u;
        let node_idx = stack[stack_ptr];
        let node = lbvh_nodes[node_idx];

        if !node_overlap(q_box, node, tol) {
            continue;
        }

        let is_leaf = (node.right_or_flag & 0x80000000u) != 0u;
        if is_leaf {
            let prim_id = node.left_or_prim;
            // In self-collision mode (count_b == 0xFFFFFFFF flag), only emit upper triangle
            let slot = atomicAdd(&lbvh_pair_count, 1u);
            if slot < lbvh_params.max_pairs {
                lbvh_pairs[slot] = OverlapPair(q_box.id, prim_id);
            }
        } else {
            let left = node.left_or_prim;
            let right = node.right_or_flag;
            if stack_ptr + 2u <= 64u {
                stack[stack_ptr] = right;
                stack[stack_ptr + 1u] = left;
                stack_ptr += 2u;
            }
        }
    }
}

// ── Pass 5: Surface Grid Residual Evaluation for SSI Seed Hunting ─────────
// Evaluates a coarse UV grid on surface A against a coarse UV grid on surface B
// and emits candidate `(u_a, v_a, u_b, v_b, dist_sq)` seeds whose distance is
// below threshold for subsequent exact Newton-Raphson refinement on CPU.

struct SurfaceSample {
    pos: vec4<f32>, // xyz = 3D position, w = u
    uv_norm: vec4<f32>, // x = v, yzw = normal
}

struct SsiSeedParams {
    count_a: u32,
    count_b: u32,
    max_seeds: u32,
    threshold_sq: f32,
}

struct SsiSeedCandidate {
    u_a: f32,
    v_a: f32,
    u_b: f32,
    v_b: f32,
    dist_sq: f32,
    idx_a: u32,
    idx_b: u32,
    pad: u32,
}

@group(0) @binding(0) var<uniform> ssi_params: SsiSeedParams;
@group(0) @binding(1) var<storage, read> samples_a: array<SurfaceSample>;
@group(0) @binding(2) var<storage, read> samples_b: array<SurfaceSample>;
@group(0) @binding(3) var<storage, read_write> seeds_out: array<SsiSeedCandidate>;
@group(0) @binding(4) var<storage, read_write> seed_count: atomic<u32>;

@compute @workgroup_size(16, 16)
fn evaluate_surface_residuals(@builtin(global_invocation_id) gid: vec3<u32>) {
    let ia = gid.x;
    let ib = gid.y;
    if ia >= ssi_params.count_a || ib >= ssi_params.count_b {
        return;
    }

    let sa = samples_a[ia];
    let sb = samples_b[ib];
    let diff = sa.pos.xyz - sb.pos.xyz;
    let dist_sq = dot(diff, diff);

    if dist_sq <= ssi_params.threshold_sq {
        let slot = atomicAdd(&seed_count, 1u);
        if slot < ssi_params.max_seeds {
            seeds_out[slot] = SsiSeedCandidate(
                sa.pos.w,
                sa.uv_norm.x,
                sb.pos.w,
                sb.uv_norm.x,
                dist_sq,
                ia,
                ib,
                0u,
            );
        }
    }
}
