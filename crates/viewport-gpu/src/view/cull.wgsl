// The instanced placements in view, and the level each is drawn from, chosen on the GPU
// (see `gpu_cull.rs`). Three steps over one frame's view:
//   pick:  each placement in view takes its triangles' and its edges' level (as `PlacedBox` does
//          on the CPU) and a slot among that level's placements (an atomic count in the level's
//          draw arguments);
//   scan:  one workgroup turns the counts into where each level's placements start;
//   place: each placement copies itself to its slot.
// While the view moves, `pick` also leaves out the placements the last frame's depth hides.

struct View {
    vp: mat4x4<f32>,
    size: vec2<f32>,
    lod_px: f32,
    edge_px: f32,
    // placements, where the parts start in `scene`, where the levels start in `scene`,
    // the triangle entries culled here (the first ones), the edge entries
    n: u32,
    parts_at: u32,
    levels_at: u32,
    tri_entries: u32,
    edge_entries: u32,
    // where the edges' arguments start in `args`, and their placements in `out` (words)
    edge_args_at: u32,
    edge_out_at: u32,
    _pad: u32,
    // While the view moves: the view the last frame's depth pyramid (`hiz.rs`) was drawn with,
    // and the pyramid's frame width and height (pixels), its level count, and flags (1: leave
    // out the placements it hides, 2: the depth buffer is 1 at the near plane).
    prev_vp: mat4x4<f32>,
    hiz: vec4<u32>,
    // x: the whole-body hovered placement index, y: its hover palette entry, z: 1 when set.
    hover: vec4<u32>,
}

@group(0) @binding(0) var<uniform> view: View;
// Placements (17 words: the 16 of a placement, then its part), then parts (16 words: low corner
// and a word that is 1 for a box that is not finite, high corner and a pad, then the triangles'
// first level, level count and whether they are culled here, a pad, then the edges' first
// level, level count and whether the part has edges here, a pad), then levels (4 words: the
// limit (f32), the draw entry, two pads).
@group(0) @binding(1) var<storage, read> scene: array<u32>;
// The triangles' draw arguments (5 words each), then the edges' (4 words each).
@group(0) @binding(2) var<storage, read_write> args: array<atomic<u32>>;
// The triangles' placements in view (16 words each), then the edges'.
@group(0) @binding(3) var<storage, read_write> out: array<u32>;
// Per placement: its triangles' entry and slot, its edges' entry and slot (NONE: not drawn).
@group(0) @binding(4) var<storage, read_write> picks: array<u32>;
@group(0) @binding(5) var hiz: texture_2d<f32>;

// Whether a box (corners `lo`, `hi` moved by the rows) lies wholly behind the depth the last
// frame drew where the box would be seen in that frame's view: its nearest point farther than the
// farthest depth of the pyramid's texels its outline covers (at the level where that is at most
// 2 × 2 texels). A box with a corner behind that view's eye, or off its frame, is not.
fn hidden(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, lo: vec3<f32>, hi: vec3<f32>) -> bool {
    let frame = vec2<f32>(f32(view.hiz.x), f32(view.hiz.y));
    var pmin = vec2<f32>(1e30, 1e30);
    var pmax = vec2<f32>(-1e30, -1e30);
    let reversed = (view.hiz.w & 2u) != 0u;
    var near = 1e30;
    // How far the placement's own depth may have moved since the frame the pyramid holds: the
    // most any corner of it gained (or lost), this frame's view against the pyramid's. A
    // placement cannot be its own occluder by this much, so the margin below is at least it.
    var shift = 0.0;
    var outside = false;
    for (var k = 0u; k < 8u; k = k + 1u) {
        let p = vec4<f32>(corner(lo, hi, k), 1.0);
        let placed = vec4<f32>(dot(r0, p), dot(r1, p), dot(r2, p), 1.0);
        let c = view.prev_vp * placed;
        if (!(c.w > 1e-6)) { return false; }
        let ndc = c.xyz / c.w;
        let px = vec2<f32>((ndc.x * 0.5 + 0.5) * frame.x, (0.5 - ndc.y * 0.5) * frame.y);
        if (px.x < 0.0 || px.x >= frame.x || px.y < 0.0 || px.y >= frame.y) { outside = true; }
        pmin = min(pmin, px);
        pmax = max(pmax, px);
        let depth = select(ndc.z, 1.0 - ndc.z, reversed);
        near = min(near, depth);
        // `view.vp` is the cull's own view, unturned (the frustum tests take it so): its depth
        // needs no turning to compare with the pyramid's, which `depth` above turned.
        let now = view.vp * placed;
        if (now.w > 1e-6) {
            shift = max(shift, abs(now.z / now.w - depth));
        }
    }
    // A box crossing the frame's edge may have a part the current view shows: the depth kept
    // inside the strip says nothing about the rest of it.
    if (outside) { return false; }
    let a = max(pmin, vec2<f32>(0.0, 0.0)) * 0.5;
    let b = min(pmax, frame - vec2<f32>(1.0, 1.0)) * 0.5;
    let span = max(b.x - a.x, b.y - a.y);
    var level = 0u;
    if (span > 1.0) { level = u32(ceil(log2(span))); }
    level = min(level, view.hiz.z - 1u);
    let dims = vec2<i32>(textureDimensions(hiz, level));
    let scale = 1.0 / f32(1u << level);
    let t0 = clamp(vec2<i32>(floor(a * scale)), vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    let t1 = clamp(vec2<i32>(floor(b * scale)), vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    var far = 0.0;
    for (var y = t0.y; y <= t1.y; y = y + 1) {
        for (var x = t0.x; x <= t1.x; x = x + 1) {
            far = max(far, textureLoad(hiz, vec2<i32>(x, y), i32(level)).x);
        }
    }
    // Left out only when it is farther than everything the last frame drew under it by more than
    // its own depth change since then. The absolute floor is the depth buffer's own resolution
    // at the near plane: two depths within it are the same surface, not one behind the other.
    return near > far + max(shift, 1e-6);
}

const NONE: u32 = 0xffffffffu;
// Words of a placement as it is drawn ([`INSTANCE_SIZE`]), and of one in `scene`: the placement,
// then its part.
const PLACEMENT_WORDS: u32 = 16u;
const SCENE_PLACEMENT_WORDS: u32 = 17u;

fn f(i: u32) -> f32 {
    return bitcast<f32>(scene[i]);
}

fn index(id: vec3<u32>, groups: vec3<u32>) -> u32 {
    return id.x + id.y * groups.x * 64u;
}

fn moved(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, p: vec3<f32>) -> vec4<f32> {
    let q = vec4<f32>(p, 1.0);
    return view.vp * vec4<f32>(dot(r0, q), dot(r1, q), dot(r2, q), 1.0);
}

fn screen(c: vec4<f32>) -> vec2<f32> {
    return vec2<f32>(c.x / c.w * view.size.x * 0.5, c.y / c.w * view.size.y * 0.5);
}

fn corner(lo: vec3<f32>, hi: vec3<f32>, i: u32) -> vec3<f32> {
    return select(lo, hi, vec3<bool>((i & 1u) != 0u, (i & 2u) != 0u, (i & 4u) != 0u));
}

// The coarsest of `count` levels from `first` whose limit stays under `px` at `scale` pixels per
// millimetre (level 0 when none does, or when the placement is drawn in full).
fn level_for(first: u32, count: u32, scale: f32, px: f32, full: bool) -> u32 {
    if full {
        return 0u;
    }
    var k = count;
    loop {
        if k <= 1u {
            break;
        }
        k = k - 1u;
        let limit = f(view.levels_at + (first + k) * 4u);
        if limit * scale <= px {
            return k;
        }
    }
    return 0u;
}

@compute @workgroup_size(64)
fn pick(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = index(id, groups);
    if i >= view.n {
        return;
    }
    picks[i * 4u] = NONE;
    picks[i * 4u + 2u] = NONE;
    let at = i * SCENE_PLACEMENT_WORDS;
    let r0 = vec4<f32>(f(at), f(at + 1u), f(at + 2u), f(at + 3u));
    let r1 = vec4<f32>(f(at + 4u), f(at + 5u), f(at + 6u), f(at + 7u));
    let r2 = vec4<f32>(f(at + 8u), f(at + 9u), f(at + 10u), f(at + 11u));
    let part = view.parts_at + scene[at + PLACEMENT_WORDS] * 16u;
    let lo = vec3<f32>(f(part), f(part + 1u), f(part + 2u));
    let hi = vec3<f32>(f(part + 4u), f(part + 5u), f(part + 6u));
    let finite = scene[part + 3u] == 0u;
    let tri_first = scene[part + 8u];
    let tri_count = scene[part + 9u];
    let tri_here = scene[part + 10u] != 0u;
    let edge_first = scene[part + 12u];
    let edge_count = scene[part + 13u];
    let edge_here = scene[part + 14u] != 0u;

    var corners: array<vec4<f32>, 8>;
    var full = !finite;
    if finite {
        // Per plane, whether every corner is outside it: x > w, -x > w, y > w, -y > w, z < 0, z > w.
        var out_px = true;
        var out_nx = true;
        var out_py = true;
        var out_ny = true;
        var out_near = true;
        var out_far = true;
        for (var k = 0u; k < 8u; k = k + 1u) {
            let c = moved(r0, r1, r2, corner(lo, hi, k));
            corners[k] = c;
            out_px = out_px && c.x > c.w;
            out_nx = out_nx && -c.x > c.w;
            out_py = out_py && c.y > c.w;
            out_ny = out_ny && -c.y > c.w;
            out_near = out_near && c.z < 0.0;
            out_far = out_far && c.z > c.w;
        }
        if out_px || out_nx || out_py || out_ny || out_near || out_far {
            return;
        }
        if (view.hiz.w & 1u) != 0u && hidden(r0, r1, r2, lo, hi) {
            return;
        }
    }

    // Pixels a millimetre of the part covers where it is nearest the eye, along its box's axes
    // (`scale`) and in any direction (`stretch`).
    var scale = 0.0;
    var stretch = 0.0;
    if !full && (tri_count > 1u || edge_count > 1u) {
        var nearest = 0u;
        for (var k = 0u; k < 8u; k = k + 1u) {
            let w = corners[k].w;
            if !(w > 1e-6) {
                full = true;
            }
            if w < corners[nearest].w {
                nearest = k;
            }
        }
        if !full {
            let c = corner(lo, hi, nearest);
            let o = screen(corners[nearest]);
            var axes: array<vec2<f32>, 3>;
            for (var k = 0u; k < 3u; k = k + 1u) {
                let step = max((hi[k] - lo[k]) * 0.01, 1e-3);
                var q = c;
                q[k] = q[k] + step;
                let qc = moved(r0, r1, r2, q);
                if !(qc.w > 1e-6) {
                    full = true;
                }
                axes[k] = (screen(qc) - o) / step;
            }
            var a = 0.0;
            var b = 0.0;
            var d = 0.0;
            for (var k = 0u; k < 3u; k = k + 1u) {
                let x = axes[k].x;
                let y = axes[k].y;
                scale = max(scale, sqrt(x * x + y * y));
                a = a + x * x;
                b = b + x * y;
                d = d + y * y;
            }
            let half = (a - d) * 0.5;
            stretch = sqrt((a + d) * 0.5 + sqrt(half * half + b * b));
            // Not finite (or NaN): drawn in full.
            if !(scale <= 3.0e38) || !(stretch <= 3.0e38) {
                full = true;
            }
        }
    }

    if tri_here {
        let level = level_for(tri_first, tri_count, scale, view.lod_px, full);
        let entry = scene[view.levels_at + (tri_first + level) * 4u + 1u];
        picks[i * 4u] = entry;
        picks[i * 4u + 1u] = atomicAdd(&args[entry * 5u + 1u], 1u);
    }
    if edge_here {
        let level = level_for(edge_first, edge_count, stretch, view.edge_px, full);
        let entry = scene[view.levels_at + (edge_first + level) * 4u + 1u];
        picks[i * 4u + 2u] = entry;
        picks[i * 4u + 3u] = atomicAdd(&args[view.edge_args_at + entry * 4u + 1u], 1u);
    }
}

var<workgroup> sums: array<u32, 256>;

// Where each culled entry's placements start: the entries' counts summed in order, the
// triangles' from 0 and the edges' from 0 in their own half of `out`. One workgroup of 256.
@compute @workgroup_size(256)
fn scan(@builtin(local_invocation_index) t: u32) {
    for (var side = 0u; side < 2u; side = side + 1u) {
        let entries = select(view.tri_entries, view.edge_entries, side == 1u);
        let stride = select(5u, 4u, side == 1u);
        let base = select(0u, view.edge_args_at, side == 1u);
        let first_at = select(4u, 3u, side == 1u);
        let chunk = (entries + 255u) / 256u;
        let begin = min(t * chunk, entries);
        let end = min(begin + chunk, entries);
        var total = 0u;
        for (var e = begin; e < end; e = e + 1u) {
            total = total + atomicLoad(&args[base + e * stride + 1u]);
        }
        sums[t] = total;
        workgroupBarrier();
        if t == 0u {
            var running = 0u;
            for (var k = 0u; k < 256u; k = k + 1u) {
                let s = sums[k];
                sums[k] = running;
                running = running + s;
            }
        }
        workgroupBarrier();
        var first = sums[t];
        for (var e = begin; e < end; e = e + 1u) {
            atomicStore(&args[base + e * stride + first_at], first);
            first = first + atomicLoad(&args[base + e * stride + 1u]);
        }
        workgroupBarrier();
    }
}

@compute @workgroup_size(64)
fn place(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = index(id, groups);
    if i >= view.n {
        return;
    }
    let at = i * SCENE_PLACEMENT_WORDS;
    let tri = picks[i * 4u];
    if tri != NONE {
        let slot = atomicLoad(&args[tri * 5u + 4u]) + picks[i * 4u + 1u];
        for (var w = 0u; w < PLACEMENT_WORDS; w = w + 1u) {
            out[slot * PLACEMENT_WORDS + w] = scene[at + w];
        }
        // A whole-body hover is the model's own draw in the hover palette entry: one word of
        // this placement's row, so there is no second copy of the geometry and the model's own
        // lighting and ambient occlusion apply.
        if view.hover.z != 0u && i == view.hover.x {
            out[slot * PLACEMENT_WORDS + 12u] = view.hover.y;
        }
    }
    let edge = picks[i * 4u + 2u];
    if edge != NONE {
        let slot = atomicLoad(&args[view.edge_args_at + edge * 4u + 3u]) + picks[i * 4u + 3u];
        for (var w = 0u; w < PLACEMENT_WORDS; w = w + 1u) {
            out[view.edge_out_at + slot * PLACEMENT_WORDS + w] = scene[at + w];
        }
    }
}
