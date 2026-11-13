
struct U {
    vp: mat4x4<f32>,
    back: vec4<f32>,   // xyz = toward the eye, w = 1 for linear output
    screen: vec4<f32>, // viewport width, height (px), unused, silhouettes on (1)
    clip: vec4<f32>,   // section plane: normal, d (zero normal: no section)
    cap: vec4<f32>,    // colour of the inside of cut bodies
    ana: vec4<f32>,    // surface analysis: mode (1 zebra, 2 draft, 3 curvature, 6 values), parameter
    pull: vec4<f32>,   // draft pull direction
    acc_u: vec4<f32>,  // accessibility map axes (xyz, offset)
    acc_v: vec4<f32>,
    eye: vec4<f32>,    // eye position, w = 1 for perspective (0: orthographic, looking along -back)
    ao: vec4<f32>,     // x: radius in world units (zero: disabled), y: samples per pixel
    // analysis colours (`theme::AnalysisColors`): draft positive, between, negative; reachable,
    // blocked; curvature flat, middle, tight
    ana_c: array<vec4<f32>, 8>,
    ana_f: vec4<f32>,  // x = 1: stripe the negative draft and blocked faces
    depth: vec4<f32>,  // x = 1: reversed depth (1 near, 0 far); y, z: the faces' push (`FACE_BIAS`)
    // Where the drawn geometry is moved in the world: identity, or a dragged component's move
    // from where its scene was built (`MovingLayer`)
    model: mat4x4<f32>,
    tint: vec4<f32>,   // xyz added to a whole hovered body's colour before lighting
    sel: vec4<f32>,    // xyz a whole selected body's colour before lighting
    // the section hatch colours by part (`crate::hatch`), the first the cap's own
    hatch: array<vec4<f32>, 3>,
    product_lights_meta: vec4<f32>, // explicit preview flag and bounded count
    product_lights: array<vec4<f32>,84>, // position/kind, radiance, axis/cone, attenuation
    sky: vec4<f32>,    // xyz = world up; w = 1: shade in linear light with a sky fill (#7974)
};

/// A scene's point in the world (see `U.model`).
fn placed(p: vec3<f32>) -> vec3<f32> {
    return (u.model * vec4<f32>(p, 1.0)).xyz;
}

/// A scene's direction in the world (see `placed`).
fn turned(n: vec3<f32>) -> vec3<f32> {
    return (u.model * vec4<f32>(n, 0.0)).xyz;
}

fn reversed() -> bool {
    return u.depth.x > 0.5;
}

/// Clip-space depth measured the usual way (0 at the near plane, w at the far one), whichever
/// way the depth buffer runs.
fn near_z(c: vec4<f32>) -> f32 {
    return select(c.z, c.w - c.z, reversed());
}

@group(1) @binding(2) var acc_t: texture_2d<f32>;
@group(2) @binding(0) var ao_depth: texture_2d<u32>;
@group(2) @binding(1) var ao_map: texture_2d<f32>;

fn clipped(p: vec3<f32>) -> bool {
    return length(u.clip.xyz) > 0.5 && dot(u.clip.xyz, p) > u.clip.w;
}
@group(0) @binding(0) var<uniform> u: U;
// The scene's colours, 2048 entries to a row: an Rg32Uint texture rather than a storage buffer,
// so the viewport draws on WebGL2 (which has no storage buffers) as well. Each entry is the
// colour's bytes, then 1 + the section hatch of the body it colours (0: the cap's own).
@group(0) @binding(1) var pal: texture_2d<u32>;

fn palette_entry(i: u32) -> vec2<u32> {
    return textureLoad(pal, vec2<i32>(i32(i & 2047u), i32(i >> 11u)), 0).xy;
}

// The palette's PBR materials (`Material` in view.rs): for palette row r, texel row 2r holds each
// entry's linear base colour and a PBR flag, row 2r + 1 its metallic fraction, roughness and IOR.
@group(0) @binding(2) var pbr_materials: texture_2d<f32>;

/// Palette entry `i`'s hatch word (`hatch`, under 256) with the entry's PBR finish packed above
/// it: metallic in bits 8-15, roughness in 16-23, bit 24 set when the entry has one. An entry
/// past the material texture has none (a load out of range is not defined).
fn finish_h(i: u32, hatch: u32) -> u32 {
    let at = vec2<u32>(i & 2047u, (i >> 11u) * 2u);
    if (at.y + 1u >= textureDimensions(pbr_materials).y || textureLoad(pbr_materials, at, 0).w < 0.5) { return hatch; }
    let p = clamp(textureLoad(pbr_materials, at + vec2<u32>(0u, 1u), 0).xy, vec2<f32>(0.0), vec2<f32>(1.0));
    let q = vec2<u32>(round(p * 255.0));
    return hatch | (q.x << 8u) | (q.y << 16u) | (1u << 24u);
}

/// A palette entry's colour bytes as byte/255 (the old Unorm8x4 conversion, bit for bit).
fn unpack_color(v: u32) -> vec4<f32> {
    return vec4<f32>(f32(v & 255u), f32((v >> 8u) & 255u), f32((v >> 16u) & 255u), f32((v >> 24u) & 255u)) / 255.0;
}

/// The colour at palette entry `i`.
fn palette(i: u32) -> vec4<f32> {
    return unpack_color(palette_entry(i).x);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c, pow((c + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

fn out_color(c: vec4<f32>) -> vec4<f32> {
    if (u.back.w > 0.5) {
        return vec4<f32>(to_linear(c.rgb), c.a);
    }
    return c;
}

struct TOut {
    @invariant @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) c: vec4<f32>,
    @location(2) wp: vec3<f32>,
    // Low byte: 1 + the section hatch of the body (0: the cap's own), from the palette; above
    // it the entry's PBR finish (`finish_h`).
    @location(3) @interpolate(flat) h: u32,
    // The vertex's value (`AnalysisMode::Values`), and 1 where the triangle has values (0 for
    // every vertex shader that gives none: a struct's fields start at zero).
    @location(4) v: f32,
    @location(5) @interpolate(flat) hv: u32,
};

/// A built triangle's vertex: its value word is an f32's bits, or all ones for none
/// (`NO_VALUE`).
@vertex
fn vs_tri(@location(0) local: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: u32, @location(3) value: u32) -> TOut {
    var o = tri_vertex(local, n, c);
    let none = value == 0xffffffffu;
    o.v = select(bitcast<f32>(value), 0.0, none);
    o.hv = select(1u, 0u, none);
    return o;
}

/// A vertex evaluated from a display plan on the GPU (no value).
@vertex
fn vs_tri_planned(@location(0) local: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: u32) -> TOut {
    return tri_vertex(local, n, c);
}

/// A triangle vertex at `local` with normal `n` in palette entry `c`, without a value.
fn tri_vertex(local: vec3<f32>, n: vec3<f32>, c: u32) -> TOut {
    var o: TOut;
    let p = placed(local);
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.n = turned(n);
    let e = palette_entry(c);
    o.c = unpack_color(e.x);
    o.h = finish_h(c, e.y);
    o.wp = p;
    return o;
}

/// Shared triangles (in their part's own frame) drawn at one placement: the rows of its rigid
/// move (`r0`..`r2`, a point's coordinates are their dot products with (p, 1)) and its colour,
/// then moved like the rest of its scene (`placed`: a dragged component's layer).
/// The vertex colour (location 2) is not read.
@vertex
fn vs_tri_inst(
    @location(0) p: vec3<f32>,
    @location(1) n: vec3<f32>,
    @location(3) r0: vec4<f32>,
    @location(4) r1: vec4<f32>,
    @location(5) r2: vec4<f32>,
    @location(6) c: u32,
) -> TOut {
    let q = vec4<f32>(p, 1.0);
    let wp = placed(vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q)));
    let pn = vec3<f32>(dot(r0.xyz, n), dot(r1.xyz, n), dot(r2.xyz, n));
    var o: TOut;
    o.pos = u.vp * vec4<f32>(wp, 1.0);
    o.n = turned(pn);
    let e = palette_entry(c);
    o.c = unpack_color(e.x);
    o.h = finish_h(c, e.y);
    o.wp = wp;
    return o;
}

/// A flat-shaded placement (`vs_tri_inst_flat`): its vertices are shared between triangles, so
/// the face's normal is the triangle's own plane, worked out per fragment. The fragment gets its
/// position from the eye (`rp`): small near what is looked at, so the plane comes out exact at
/// any zoom however far from the origin the part stands, and few values per vertex (a tiled GPU
/// stores every vertex's outputs: more of them cost more than the triangles' shared vertices
/// save).
struct FOut {
    @invariant @builtin(position) pos: vec4<f32>,
    @location(0) rp: vec3<f32>,
    @location(1) c: vec4<f32>,
    @location(2) @interpolate(flat) h: u32,
};

@vertex
fn vs_tri_inst_flat(
    @location(0) p: vec3<f32>,
    @location(3) r0: vec4<f32>,
    @location(4) r1: vec4<f32>,
    @location(5) r2: vec4<f32>,
    @location(6) c: u32,
) -> FOut {
    let q = vec4<f32>(p, 1.0);
    let wp = placed(vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q)));
    var o: FOut;
    o.pos = u.vp * vec4<f32>(wp, 1.0);
    o.rp = wp - u.eye.xyz;
    let e = palette_entry(c);
    o.c = unpack_color(e.x);
    o.h = finish_h(c, e.y);
    return o;
}

/// The fragment of a flat-shaded placement as the other triangles' fragments are: its normal
/// the outward normal of its triangle (toward the eye on a front face, away on a back face, as
/// the triangles are wound outward).
fn flat_fragment(i: FOut, front: bool) -> TOut {
    var n = cross(dpdx(i.rp), dpdy(i.rp));
    let toward = select(u.back.xyz, -i.rp, u.eye.w > 0.5);
    n = n * inverseSqrt(max(dot(n, n), 1e-30));
    if ((dot(n, toward) < 0.0) == front) { n = -n; }
    var o: TOut;
    o.pos = i.pos;
    o.n = n;
    o.c = i.c;
    o.h = i.h;
    o.wp = i.rp + u.eye.xyz;
    return o;
}

@fragment
fn fs_solid_flat(i: FOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = flat_fragment(i, front);
    let c = solid(t, front);
    return c;
}

@fragment
fn fs_solid_cut_flat(i: FOut, @builtin(front_facing) front: bool) -> CutOut {
    let t = flat_fragment(i, front);
    var o: CutOut;
    o.depth = pushed(cap_depth(t.wp, t.pos.z));
    o.c = solid(t, front);
    return o;
}

/// Weighted blended order-independent transparency (`view/oit.rs`): a see-through fragment's
/// colour, premultiplied and weighted by its alpha and nearness, into the accumulation target,
/// and its alpha into the revealage target (which keeps what is behind it by 1 - alpha).
struct Oit {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: vec4<f32>,
};

fn oit(c: vec4<f32>, z: f32) -> Oit {
    let a = clamp(c.a, 0.0, 1.0);
    // Depth 0 near and 1 far, whichever way the buffer runs.
    let d = select(z, 1.0 - z, reversed());
    let w = clamp(a * max(1e-2, 3e3 * pow(1.0 - d, 3.0)), 1e-2, 3e3);
    var o: Oit;
    o.accum = vec4<f32>(c.rgb * a, a) * w;
    o.reveal = vec4<f32>(a);
    return o;
}

@fragment
fn fs_oit(i: TOut) -> Oit {
    if (clipped(i.wp)) { discard; }
    return oit(shade(i), i.pos.z);
}

@fragment
fn fs_oit_flat(i: FOut, @builtin(front_facing) front: bool) -> Oit {
    let t = flat_fragment(i, front);
    let o = oit(shade(t), t.pos.z);
    if (clipped(t.wp)) { discard; }
    return o;
}

/// A see-through flat-shaded placement (faded or glass), shaded like `fs_tri`.
@fragment
fn fs_tri_flat(i: FOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = flat_fragment(i, front);
    let c = shade(t);
    if (clipped(t.wp)) { discard; }
    return c;
}

@fragment
fn fs_ao_depth_flat(i: FOut, @builtin(front_facing) front: bool) -> AoDepth {
    let t = flat_fragment(i, front);
    return opaque_depth(t, front);
}

@fragment
fn fs_ao_term_flat(i: FOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = flat_fragment(i, front);
    return ao_term(t, front);
}

/// Opaque model surfaces: cut by the section plane/// Opaque model surfaces: cut by the section plane, the inside shows as a flat cap colour.
@fragment
fn fs_solid(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = solid(i, front);
    return c;
}

/// A section's crossing count (`Cut::count` in `view.rs`): nothing but the cut itself, so the
/// stencil counts only the faces behind the section plane. The colour is not written.
@fragment
fn fs_clip(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return vec4<f32>(0.0);
}

@fragment
fn fs_clip_flat(i: FOut) -> @location(0) vec4<f32> {
    if (clipped(i.rp + u.eye.xyz)) { discard; }
    return vec4<f32>(0.0);
}

/// Faces where the section plane is outside every closed body (`Cut::rest`): a back face seen
/// there is the inside of an open body (a surface body, a solid with faces left out), lit as it
/// is without a section rather than hatched, since there is no cut face to show.
@fragment
fn fs_solid_open(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = solid(i, true);
    return c;
}

@fragment
fn fs_solid_open_flat(i: FOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = flat_fragment(i, front);
    let c = solid(t, true);
    return c;
}

struct CutOut {
    @location(0) c: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

/// The section cap (where the section plane lies inside a body, drawn on the far walls' back
/// faces) again, at the depth of the section plane rather than of the
/// far wall, so the cut face hides what lies inside the body instead of showing it like a
/// pane of glass.
@fragment
fn fs_solid_cut(i: TOut, @builtin(front_facing) front: bool) -> CutOut {
    var o: CutOut;
    // Derivatives (in `pushed`) first, before `solid` may discard.
    o.depth = pushed(cap_depth(i.wp, i.pos.z));
    o.c = solid(i, front);
    return o;
}

/// A depth the shader writes itself, pushed back like the faces the GPU pushes (`FACE_BIAS`,
/// which does not apply then): its own slope over `depth.z` pixels and `depth.y` depth steps.
fn pushed(z: f32) -> f32 {
    let slope = max(abs(dpdx(z)), abs(dpdy(z)));
    // One step: 2⁻²⁴ in a 24-bit buffer; a float buffer's steps follow the value (2⁻²³ of it).
    let step = select(1.0 / 16777216.0, abs(z) * 1.1920929e-7, reversed());
    let push = slope * u.depth.z + step * u.depth.y;
    return clamp(z - select(-push, push, reversed()), 0.0, 1.0);
}

/// Depth of the section plane on the view ray through `wp` (a far wall seen through the cut),
/// never behind the wall itself (`z`).
fn cap_depth(wp: vec3<f32>, z: f32) -> f32 {
    var dir = -u.back.xyz;
    if (u.eye.w > 0.5) { dir = wp - u.eye.xyz; }
    let dn = dot(u.clip.xyz, dir);
    if (abs(dn) < 1e-9) { return z; }
    let p = wp + dir * ((u.clip.w - dot(u.clip.xyz, wp)) / dn);
    let c = u.vp * vec4<f32>(p, 1.0);
    if (c.w <= 0.0) { return z; }
    if (reversed()) { return clamp(c.z / c.w, z, 1.0); }
    return clamp(c.z / c.w, 0.0, z);
}

fn solid(i: TOut, front: bool) -> vec4<f32> {
    // Derivatives first, in uniform control flow (browsers' WGSL compilers reject them after
    // a branch that returns; the whole shader module then fails and nothing is drawn).
    let k = curvature(i);
    let ndv = dot(normalize(i.n), normalize(u.back.xyz));
    let fw = fwidth(ndv);
    if (clipped(i.wp)) { discard; }
    if (!front && length(u.clip.xyz) > 0.5) {
        // The cut face in its body's colour ("from component" colouring), tinted toward the
        // cap colour so it reads as a cut, hatched at 45 degrees on screen; by part, toward the
        // body's own hatch colour and at its angle (45 or 135 degrees). With the hatch off
        // (`u.cap.a` 0), plain in the body's own colour.
        if (u.cap.a < 0.5) { return out_color(vec4<f32>(i.c.rgb, 1.0)); }
        var tint = u.cap.rgb;
        var turn = 1.0;
        if ((i.h & 255u) > 0u) {
            let k = (i.h & 255u) - 1u;
            tint = u.hatch[min(k >> 1u, 2u)].rgb;
            turn = select(1.0, -1.0, (k & 1u) == 1u);
        }
        let base = mix(i.c.rgb, tint, 0.35);
        let h = fract((i.pos.x + turn * i.pos.y) / 9.0);
        return out_color(vec4<f32>(select(base, base * 0.6, h < 0.22), 1.0));
    }
    if (u.ana.x > 0.5 && length(i.n) > 0.5) {
        return analysis(i, k);
    }
    let c = lit(i, occlusion(i));
    // Silhouettes (styles with edges): where a curved surface turns away from the eye, a line
    // about 1.5 px wide, on the visible surface only (depth-correct).
    if (u.screen.w > 0.5 && length(i.n) > 0.5 && fw > 1e-6) {
        let a = 1.0 - smoothstep(fw * 0.8, fw * 2.0, abs(ndv));
        return vec4<f32>(mix(c.rgb, out_color(vec4<f32>(0.07, 0.08, 0.10, 1.0)).rgb, a), c.a);
    }
    return c;
}

// A single-sample opaque depth map, independent of the window's MSAA and its UI offsets.
// Preserve the depth's bits in R32Uint: GLSL cannot textureLoad a depth texture,
// and float colour attachments are not available on every GL adapter.
struct AoDepth {
    @builtin(frag_depth) depth: f32,
    @location(0) value: u32,
};

@fragment
fn fs_ao_depth(i: TOut, @builtin(front_facing) front: bool) -> AoDepth {
    return opaque_depth(i, front);
}

// The virtual ground preserves the viewport background; only contact occlusion is blended.
@fragment
fn fs_ground_ao(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0 - occlusion(i));
}

/// The AO prepass's depth of an opaque surface's fragment.
fn opaque_depth(i: TOut, front: bool) -> AoDepth {
    if (clipped(i.wp) || length(i.n) < 0.5) { discard; }
    var depth = i.pos.z;
    if (!front && length(u.clip.xyz) > 0.5) { depth = cap_depth(i.wp, i.pos.z); }
    return AoDepth(depth, bitcast<u32>(depth));
}

/// The term for an opaque surface's fragment, on the prepass's depth. A section's cut face has none.
fn ao_term(i: TOut, front: bool) -> vec4<f32> {
    if (clipped(i.wp) || length(i.n) < 0.5) { discard; }
    if (!front && length(u.clip.xyz) > 0.5) { return vec4<f32>(1.0); }
    return vec4<f32>(occlusion_term(i));
}

@fragment
fn fs_ao_term(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return ao_term(i, front);
}

@vertex
fn vs_ao_blur(@builtin(vertex_index) v: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

/// The depth map at `p`, reading 0 near and 1 far whichever way the buffer runs.
fn map_depth(p: vec2<i32>) -> f32 {
    let at = clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(ao_depth)) - vec2<i32>(1));
    let d = bitcast<f32>(textureLoad(ao_depth, at, 0).x);
    return select(d, 1.0 - d, reversed());
}

/// The raw term (`ao_map` in this pass) averaged over a 4 x 4 window, which holds every turn of
/// [`ao_turn`]'s pattern once. A neighbour counts when it lies on this pixel's plane, the slope
/// taken from the nearer side across each axis, so no surface's shade crosses an edge.
@fragment
fn fs_ao_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = vec2<i32>(pos.xy);
    let d = map_depth(c);
    if (d >= 1.0) { return vec4<f32>(1.0); }
    let left = d - map_depth(c + vec2<i32>(-1, 0));
    let right = map_depth(c + vec2<i32>(1, 0)) - d;
    let above = d - map_depth(c + vec2<i32>(0, -1));
    let below = map_depth(c + vec2<i32>(0, 1)) - d;
    let dx = select(right, left, abs(left) < abs(right));
    let dy = select(below, above, abs(above) < abs(below));
    let tolerance = 1.5 * (abs(dx) + abs(dy)) + 1e-6;
    let last = vec2<i32>(textureDimensions(ao_map)) - vec2<i32>(1);
    var sum = 0.0;
    var weight = 0.0;
    for (var y = -2; y < 2; y = y + 1) {
        for (var x = -2; x < 2; x = x + 1) {
            let p = c + vec2<i32>(x, y);
            let s = map_depth(p);
            if (s < 1.0 && abs(s - (d + dx * f32(x) + dy * f32(y))) <= tolerance) {
                sum = sum + textureLoad(ao_map, clamp(p, vec2<i32>(0), last), 0).x;
                weight = weight + 1.0;
            }
        }
    }
    return vec4<f32>(sum / max(weight, 1.0));
}

/// A pixel's turn of the sample pattern: every 4 x 4 window holds each of 16 turns once.
fn ao_turn(pos: vec2<f32>) -> f32 {
    let x = u32(pos.x) & 3u;
    let y = u32(pos.y) & 3u;
    // WGSL won't mix shifts and `|` without parentheses; naga takes it, browsers refuse it (#11080).
    let v = (((x ^ y) & 1u) << 3u) | ((y & 1u) << 2u) | (((x ^ y) >> 1u) << 1u) | (y >> 1u);
    return (f32(v) + 0.5) / 16.0;
}

/// The lit pass's occlusion: the blurred term where this point falls in the map, whatever the
/// window's viewport offset and supersampling.
fn occlusion(i: TOut) -> f32 {
    let size = vec2<i32>(textureDimensions(ao_map));
    // The maps are 1 × 1 stand-ins until a frame draws into them (the product preview draws no
    // prepass, and AO off releases them): they tell nothing, so leave the surface unoccluded.
    if (u.ao.x <= 0.0 || length(i.n) < 0.5 || size.x < 2 || size.y < 2) { return 1.0; }
    let q = u.vp * vec4<f32>(i.wp, 1.0);
    if (q.w <= 0.0) { return 1.0; }
    let uv = vec2<f32>(q.x / q.w * 0.5 + 0.5, 0.5 - q.y / q.w * 0.5);
    return textureLoad(ao_map, clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1)), 0).x;
}

// Cosine-weighted hemisphere samples, the pattern turned by a different angle at each pixel.
// Projecting the sample and a point one radius toward the eye supplies a depth range in both
// orthographic and perspective views; distant foreground geometry cannot cast a dark halo onto
// a background surface.
fn occlusion_term(i: TOut) -> f32 {
    let radius = u.ao.x;
    if (radius <= 0.0 || length(i.n) < 0.5) { return 1.0; }
    var n = normalize(i.n);
    if (dot(n, u.back.xyz) < 0.0) { n = -n; }
    let axis = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.z) > 0.9);
    let tangent = normalize(cross(axis, n));
    let bitangent = cross(n, tangent);
    let size = vec2<i32>(textureDimensions(ao_depth));
    // The map is a 1 × 1 stand-in until a frame draws into it (the product preview draws no
    // prepass): sampling it tells nothing, so leave the surface unoccluded.
    if (size.x < 2 || size.y < 2) { return 1.0; }
    // The same offsets at every pixel made each sample darken its own shifted copy of a nearby
    // part's outline: faint ghosts of the part where the radius is large on screen (#4167),
    // stepped bands along contacts where it is small (#3407). Turned, neighbouring pixels sample
    // different points, the copies break up into grain, and the blur takes the grain out (#9327).
    let turn = ao_turn(i.pos.xy);
    let t = tangent * cos(turn * 6.28318531) + bitangent * sin(turn * 6.28318531);
    let b = cross(n, t);
    // One comparison for both depth directions: depths read as 0 near / 1 far (mirrored when
    // the buffer runs reversed). The map is cleared to its far plane either way, so a texel
    // with nothing in it reads as 1 and the `d < 1.0` test below skips it.
    let flip = reversed();
    // Fewer samples while the view moves (#7165), spread over the same hemisphere. 32 is the
    // still picture's count, `AO_SAMPLES` in view.rs.
    let count = u32(clamp(u.ao.y, 1.0, 32.0));
    var blocked = 0.0;
    for (var j = 0u; j < count; j = j + 1u) {
        let f = (f32(j) + 0.5) / f32(count);
        let angle = f32(j) * 2.39996323;
        let direction = t * (cos(angle) * sqrt(f)) + b * (sin(angle) * sqrt(f)) + n * sqrt(1.0 - f);
        let reach = radius * (0.2 + 0.8 * fract(f32(j) * 0.61803399 + 0.5));
        let sample = i.wp + n * (radius * 0.025) + direction * reach;
        let q = u.vp * vec4<f32>(sample, 1.0);
        let near = u.vp * vec4<f32>(sample + u.back.xyz * radius, 1.0);
        if (q.w <= 0.0 || near.w <= 0.0) { continue; }
        let ndc = q.xyz / q.w;
        let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0)) || ndc.z < 0.0 || ndc.z > 1.0) { continue; }
        let d0 = bitcast<f32>(textureLoad(ao_depth, vec2<i32>(uv * vec2<f32>(size)), 0).x);
        let zs = select(ndc.z, 1.0 - ndc.z, flip);
        let zn = select(near.z / near.w, 1.0 - near.z / near.w, flip);
        let d = select(d0, 1.0 - d0, flip);
        let span = max(zs - zn, 1e-7);
        let delta = (zs - d) / span;
        if (d < 1.0 && delta > 0.005 && delta < 1.0) {
            blocked = blocked + smoothstep(0.005, 0.15, delta) * (1.0 - smoothstep(0.5, 1.0, delta));
        }
    }
    return 1.0 - 0.85 * blocked / f32(count);
}

/// How fast the normal turns across the pixel (per mm).
fn curvature(i: TOut) -> f32 {
    let nn = normalize(i.n);
    let kx = length(dpdx(nn)) / max(length(dpdx(i.wp)), 1e-6);
    let ky = length(dpdy(nn)) / max(length(dpdy(i.wp)), 1e-6);
    return max(kx, ky);
}

/// Surface analysis colours: zebra stripes, draft angle bands, curvature map (`k`: the
/// surface's curvature here, worked out by the caller).
fn analysis(i: TOut, k: f32) -> vec4<f32> {
    let v = normalize(u.back.xyz);
    var n = normalize(i.n);
    if (dot(n, v) < 0.0) { n = -n; }
    let lit = 0.55 + 0.45 * max(dot(n, v), 0.0);
    if (u.ana.x < 1.5) {
        // Reflections of light bars along `pull` (upright, +Z, standing around the viewer by
        // default): the stripe follows the reflected ray's direction across the bars.
        let r = reflect(-v, n);
        var side = cross(v, u.pull.xyz);
        if (length(side) < 1e-3) { side = vec3<f32>(1.0, 0.0, 0.0); }
        let x = dot(r, normalize(side));
        // White for `ana.z` of each period (half: as `sin(x * stripes * pi) > 0` always drew it).
        let s = fract(x * u.ana.y * 0.5);
        // A face whose normal lies in the plane of the bars through the view (every horizontal
        // face, under upright bars) reflects straight ahead: its sideways part is zero, on the
        // edge between a black and a white stripe, and rounding picked one or the other from
        // frame to frame while the view turned (#2153). Straight ahead is white.
        let c = select(0.08, 0.95, s < u.ana.z || abs(x) < 1e-4);
        return out_color(vec4<f32>(vec3<f32>(c), 1.0));
    }
    if (u.ana.x > 5.5) {
        // By the vertices' values, `ana.y` (flat) to `ana.z` (tight); a face without values by
        // how fast its normal turns, as the curvature map at a radius of 1 mm.
        var t = clamp(k, 0.0, 1.0);
        if (i.hv == 1u) { t = clamp((i.v - u.ana.y) / max(u.ana.z - u.ana.y, 1e-12), 0.0, 1.0); }
        return out_color(vec4<f32>(ramp(t) * lit, 1.0));
    }
    if (u.ana.x > 4.5) {
        // Reachable from the access direction: facing it and nothing above.
        let d = normalize(u.pull.xyz);
        let size = i32(u.ana.w);
        let px = vec2<i32>(floor(vec2<f32>(dot(i.wp, u.acc_u.xyz) + u.acc_u.w, dot(i.wp, u.acc_v.xyz) + u.acc_v.w) * u.ana.z));
        var top = -1e30;
        if (px.x >= 0 && px.y >= 0 && px.x < size && px.y < size) {
            top = textureLoad(acc_t, px, 0).x;
        }
        let cn = dot(normalize(i.n), d);
        let facing = cn > 0.02;
        // Slack of a pixel and a half, more on slopes (the map holds the pixel's highest point).
        let slack = u.ana.y * (1.0 + sqrt(max(1.0 - cn * cn, 0.0)) / max(cn, 0.05));
        let clear = dot(i.wp, d) >= top - slack;
        var c = u.ana_c[4].rgb;
        if (facing && clear) { c = u.ana_c[3].rgb; } else { c = stripe(i, c); }
        return out_color(vec4<f32>(c * lit, 1.0));
    }
    if (u.ana.x > 3.5) {
        // A studio around the model: sky above, a dark floor below, a bright horizon band and
        // two soft light panels, seen in a mirror.
        let r = reflect(-v, n);
        var c = mix(vec3<f32>(0.32, 0.34, 0.38), vec3<f32>(0.80, 0.86, 0.95), smoothstep(-0.05, 0.6, r.z));
        c = c + vec3<f32>(0.6) * exp(-abs(r.z) * 40.0);
        let side = atan2(r.y, r.x);
        c = c + vec3<f32>(0.9) * smoothstep(0.92, 0.98, cos(side * 2.0)) * smoothstep(0.1, 0.3, r.z) * smoothstep(0.8, 0.6, r.z);
        if (r.z < -0.02) {
            // The floor: lighter toward the horizon, with light-panel streaks along it.
            c = mix(vec3<f32>(0.55, 0.56, 0.60), vec3<f32>(0.20, 0.20, 0.22), clamp(-r.z * 1.6, 0.0, 1.0));
            c = c + vec3<f32>(0.35) * smoothstep(0.95, 0.99, cos(side * 3.0));
        }
        return out_color(vec4<f32>(min(c, vec3<f32>(1.0)), 1.0));
    }
    if (u.ana.x < 2.5) {
        let a = degrees(asin(clamp(dot(normalize(i.n), normalize(u.pull.xyz)), -1.0, 1.0)));
        var c = u.ana_c[1].rgb;
        if (a >= u.ana.y) { c = u.ana_c[0].rgb; }
        if (a <= -u.ana.y) { c = stripe(i, u.ana_c[2].rgb); }
        return out_color(vec4<f32>(c * lit, 1.0));
    }
    // Curvature from how fast the normal turns across the pixel.
    let t = clamp(k * u.ana.y, 0.0, 1.0);
    return out_color(vec4<f32>(ramp(t) * lit, 1.0));
}

/// The curvature colours' ramp at `t` (0 to 1): flat, through the middle colour, to tight.
fn ramp(t: f32) -> vec3<f32> {
    return select(mix(u.ana_c[6].rgb, u.ana_c[7].rgb, t * 2.0 - 1.0), mix(u.ana_c[5].rgb, u.ana_c[6].rgb, t * 2.0), t < 0.5);
}

/// A "bad" analysis face (negative draft, blocked): with the stripes on (colour vision modes), the
/// section cap's 45 degree hatch over it, so colour is not the only cue.
fn stripe(i: TOut, c: vec3<f32>) -> vec3<f32> {
    if (u.ana_f.x < 0.5) { return c; }
    let h = fract((i.pos.x + i.pos.y) / 9.0);
    return select(c, c * 0.6, h < 0.22);
}

@fragment
fn fs_tri(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return shade(i);
}

/// A whole hovered body, lit over the model from its own plan triangles in a lifted colour.
@fragment
fn fs_hl_hover(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    // An opaque body keeps its finish under the hover; a faded or see-through one stays plain.
    return lit_color(i, 1.0, vec4<f32>(i.c.rgb + u.tint.rgb, i.c.a), i.c.a > 0.99);
}

/// A whole selected body, lit over the model from its own plan triangles in the selection colour.
@fragment
fn fs_hl_selected(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    // The selection colour stays flat and plain, so it reads the same on every finish.
    return lit_color(i, 1.0, vec4<f32>(u.sel.rgb, i.c.a), false);
}

/// Reference planes: shaded like `fs_tri`, but never cut by the section plane.
@fragment
fn fs_tri_all(i: TOut) -> @location(0) vec4<f32> {
    return shade(i);
}

/// See-through, faded and reference faces: lit as before, without a material finish.
fn shade(i: TOut) -> vec4<f32> {
    return lit_color(i, 1.0, i.c, false);
}

fn product_light_unit(v: vec3<f32>) -> vec3<f32> {
    let scale=max(max(abs(v.x),abs(v.y)),abs(v.z));
    if scale == 0.0 { return vec3<f32>(0.0,0.0,1.0); }
    return normalize(v/scale);
}

// Direction and incident radiance for an unshadowed analytic preview sample.
fn product_light_sample(k: u32, p: vec3<f32>) -> mat2x3<f32> {
    let source=u.product_lights[k*4u];
    let colour=u.product_lights[k*4u+1u];
    let axis=u.product_lights[k*4u+2u];
    let falloff=u.product_lights[k*4u+3u];
    var direction=product_light_unit(source.xyz);
    var weight=1.0;
    if source.w > 0.5 {
        let delta=source.xyz-p;
        let distance=max(length(delta),falloff.y);
        direction=product_light_unit(delta);
        weight=falloff.x/max(distance*distance,0.000001);
        if source.w == 2.0 {
            let cosine=dot(product_light_unit(axis.xyz),-direction);
            var cone=select(0.0,1.0,cosine>=colour.w);
            if axis.w-colour.w>0.000001 {cone=smoothstep(colour.w,axis.w,cosine);}
            weight *= cone;
        }
        if source.w == 3.0 {weight *= max(dot(product_light_unit(axis.xyz),-direction),0.0);}
    }
    return mat2x3<f32>(direction,colour.xyz*weight);
}

/// An opaque face, with its material finish where the viewport shows them.
fn lit(i: TOut, visibility: f32) -> vec4<f32> {
    return lit_color(i, visibility, i.c, true);
}

/// sRGB to linear light, exact (the straight segment too, which `to_linear` leaves out).
fn srgb_decode(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

fn srgb_encode(c: vec3<f32>) -> vec3<f32> {
    let l = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(l * 12.92, 1.055 * pow(l, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), l > vec3<f32>(0.0031308));
}

/// The studio light summed in linear light, encoded once (#7974): the key and the eye fill as in
/// [`lit_color`], over a sky fill that grows as the face turns up, and the 4% every dielectric
/// reflects of that sky (a little more toward grazing), so a near-black face keeps a gradient. The sky
/// and the fill are what ambient occlusion (`visibility`) takes away. sRGB out.
fn sky_lit(base: vec3<f32>, n: vec3<f32>, diff: f32, fill: f32, visibility: f32) -> vec3<f32> {
    let up = dot(n, normalize(u.sky.xyz)) * 0.5 + 0.5;
    let sky = mix(0.18, 0.42, up);
    // Schlick's rise toward grazing, tempered so mid-tones only gain a soft rim.
    let fresnel = 0.04 + 0.2 * pow(1.0 - fill, 5.0);
    let albedo = srgb_decode(base);
    // Scaled so light and mid-grey faces land near where the sRGB shading puts them.
    let light = albedo * (0.41 * diff + (0.66 * sky + 0.2 * fill) * visibility) + vec3<f32>(fresnel * sky * visibility);
    return srgb_encode(light);
}

/// Whether opaque faces get a material finish (`finished`): the modeling viewport sends the
/// world's up with bit 1 of `sky.w` set (Preferences › Graphics); the product preview and
/// captures send none.
fn finishes() -> bool {
    return (u32(u.sky.w) & 2u) != 0u && dot(u.sky.xyz, u.sky.xyz) > 0.25;
}

// The studio finished faces are lit by (`Environment` in environment.rs): model-viewer's neutral
// room, as is, at model-viewer's exposure for it (`STUDIO_EXPOSURE`), prefiltered for image-based
// lighting (split-sum). Mip k of the specular map
// is the studio through a GGX lobe of perceptual roughness k / (mips - 1); the diffuse map is its
// cosine-weighted irradiance / π; the DFG table is the GGX environment BRDF's scale and bias.
@group(0) @binding(3) var studio_specular: texture_2d<f32>;
@group(0) @binding(4) var studio_diffuse: texture_2d<f32>;
@group(0) @binding(5) var studio_s: sampler;
@group(0) @binding(6) var studio_dfg: texture_2d<f32>;
/// The specular map's mips (`SPECULAR_LEVELS`; WebGL2 cannot ask a texture for its count).
const STUDIO_MIPS: f32 = 6.0;
/// model-viewer's exposure for its neutral room under Khronos PBR Neutral (`COMMERCE_EXPOSURE`).
const STUDIO_EXPOSURE: f32 = 1.3;

/// Where the direction `d` (in the studio's frame, y up) lands on its equirectangular maps.
fn studio_uv(d: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(0.5 + atan2(d.x, -d.z) / 6.283185307179586, acos(clamp(d.y, -1.0, 1.0)) / 3.141592653589793);
}

/// Khronos PBR Neutral tone mapping (KhronosGroup/ToneMapping, PBR_Neutral): base colours come
/// out as assigned under unit white light, highlights roll off toward white.
fn pbr_neutral(light: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;
    let x = min(light.r, min(light.g, light.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    var c = light - offset;
    let peak = max(c.r, max(c.g, c.b));
    if (peak < start_compression) { return c; }
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(c, vec3<f32>(new_peak), g);
}

/// Khronos glTF Sample Renderer, getIBLGGXFresnel (Fdez-Agüera 2019): the split-sum's
/// environment BRDF with roughness-dependent Fresnel and multiple scattering.
fn ibl_ggx_fresnel(nv: f32, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    // The table holds texel centres; clamping to them keeps the sampler's repeat from wrapping.
    let half_texel = 0.5 / vec2<f32>(textureDimensions(studio_dfg));
    let f_ab = textureSampleLevel(studio_dfg, studio_s, clamp(vec2<f32>(nv, roughness), half_texel, 1.0 - half_texel), 0.0).rg;
    let fr = max(vec3<f32>(1.0 - roughness), f0) - f0;
    let k_s = f0 + fr * pow(1.0 - nv, 5.0);
    let fss_ess = k_s * f_ab.x + f_ab.y;
    let ems = 1.0 - (f_ab.x + f_ab.y);
    let f_avg = f0 + (1.0 - f0) / 21.0;
    let fms_ems = ems * fss_ess * f_avg / (1.0 - f_avg * ems);
    return fss_ess + fms_ems;
}

/// An opaque face with a material finish, lit by the studio (image-based, split-sum, as the
/// Khronos glTF Sample Renderer): the PBR appearance's metallic and roughness (`finish_h`), else
/// a plain dielectric of roughness 0.5 (a product choice: satin). The studio turns with
/// the viewer (its frame is the view's, kept level: `front` toward the eye), so whatever faces the
/// eye is lit the same from any orbit angle. Ambient occlusion (`visibility`) shades the diffuse
/// light and, by Lagarde's approximation, the specular (Filament). Khronos PBR Neutral, then
/// encoded as `lit_color` does.
fn finished(i: TOut, n: vec3<f32>, visibility: f32, base: vec4<f32>) -> vec4<f32> {
    let pbr = (i.h >> 24u) != 0u;
    let metallic = select(0.0, f32((i.h >> 8u) & 255u) / 255.0, pbr);
    let roughness = select(0.5, f32((i.h >> 16u) & 255u) / 255.0, pbr);
    let a = roughness * roughness;
    // The view per pixel in perspective: a flat face then reflects a gradient, not one colour.
    let back = normalize(u.back.xyz);
    var v = back;
    if (u.eye.w > 0.5) { v = normalize(u.eye.xyz - i.wp); }
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let up = normalize(u.sky.xyz);
    var front = back - up * dot(back, up);
    if (dot(front, front) < 1e-6) { front = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(up.x) > 0.9); }
    front = normalize(front);
    let side = cross(up, front);
    // Filament getSpecularDominantDirection, whose roughness is a (perceptual squared).
    let r = normalize(mix(reflect(-v, n), n, a * a));
    let rl = vec3<f32>(dot(r, side), dot(r, up), dot(r, front));
    let nl = vec3<f32>(dot(n, side), dot(n, up), dot(n, front));
    // Khronos getIBLRadianceGGX: lod = perceptual roughness × (mips - 1).
    let lod = roughness * (STUDIO_MIPS - 1.0);
    // Filament SpecularAO_Lagarde, whose roughness is a.
    let specular_ao = clamp(pow(nv + visibility, exp2(-16.0 * a - 1.0)) - 1.0 + visibility, 0.0, 1.0);
    let specular = textureSampleLevel(studio_specular, studio_s, studio_uv(rl), lod).rgb * specular_ao;
    let albedo = srgb_decode(base.rgb);
    let diffuse = textureSampleLevel(studio_diffuse, studio_s, studio_uv(nl), 0.0).rgb * albedo * visibility;
    // Khronos pbr.frag: a dielectric (F0 0.04, glTF's IOR 1.5) and a metal, mixed by metallic.
    let dielectric = mix(diffuse, specular, ibl_ggx_fresnel(nv, roughness, vec3<f32>(0.04)));
    let metal = specular * ibl_ggx_fresnel(nv, roughness, albedo);
    let mapped = pbr_neutral(mix(dielectric, metal, metallic) * STUDIO_EXPOSURE);
    if (u.back.w > 0.5) { return vec4<f32>(mapped, base.a); }
    return vec4<f32>(srgb_encode(mapped), base.a);
}

/// [`lit`] in a colour of its own: a whole highlighted body is lit in its colour lifted by the
/// hover amount, or in the selection colour, from the model's own plan triangles. `finish`: an
/// opaque face, which takes its material finish (`finished`) where the viewport shows them.
fn lit_color(i: TOut, visibility: f32, base: vec4<f32>, finish: bool) -> vec4<f32> {
    // A zero normal marks flat colour (selection fills, translucent planes).
    if (length(i.n) < 0.5) {
        return out_color(base);
    }
    let v = normalize(u.back.xyz);
    var n = normalize(i.n);
    if (dot(n, v) < 0.0) { n = -n; }
    if u.product_lights_meta.x>0.0 {
        var radiance=vec3<f32>(0.0);
        let albedo=srgb_decode(base.rgb);
        for (var k=0u;k<min(u32(u.product_lights_meta.y),21u);k+=1u) {
            if u.product_lights[k*4u].w == 4.0 {radiance+=albedo*u.product_lights[k*4u+1u].xyz/3.141592653589793;continue;}
            let sample=product_light_sample(k,i.wp);
            radiance+=albedo*sample[1]*(max(dot(n,sample[0]),0.0)/3.141592653589793);
        }
        let mapped=radiance/(vec3<f32>(1.0)+radiance);
        // A linear attachment takes linear light as it is; any other is encoded once, as the CPU
        // image is (decoding the encoded value again through `out_color` would flatten the dark end).
        if (u.back.w > 0.5) {
            return vec4<f32>(mapped,base.a);
        }
        let encoded=select(mapped*12.92,1.055*pow(mapped,vec3<f32>(1.0/2.4))-vec3<f32>(0.055),mapped>vec3<f32>(0.0031308));
        return vec4<f32>(encoded,base.a);
    }
    if (finish && finishes()) {
        return finished(i, n, visibility, base);
    }
    let key = normalize(v * 0.8 + vec3<f32>(-0.3, 0.2, 0.6));
    let diff = max(dot(n, key), 0.0);
    let fill = max(dot(n, v), 0.0);
    let h = normalize(key + v);
    let spec = pow(max(dot(n, h), 0.0), 40.0) * 0.25;
    if (u32(u.sky.w) & 1u) != 0u {
        return out_color(vec4<f32>(min(sky_lit(base.rgb, n, diff, fill, visibility) + vec3<f32>(spec), vec3<f32>(1.0)), base.a));
    }
    let k = 0.42 + 0.38 * diff + 0.25 * fill - (0.42 + 0.25 * fill) * (1.0 - visibility);
    return out_color(vec4<f32>(min(base.rgb * k + vec3<f32>(spec), vec3<f32>(1.0)), base.a));
}

// Lines, decals and ghosts are drawn at their own depth: opaque faces are pushed back by their
// slope instead (`FACE_BIAS`, #1434), so what lies on a face draws over it at any angle.

struct LOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) c: vec4<f32>,
    @location(1) wp: vec3<f32>,
};

/// A normal from its 12-bit octahedral form (`octahedral6` in view.rs); 0 is unknown (zero).
fn octahedral(q: u32) -> vec3<f32> {
    let qx = q & 63u;
    let qy = (q >> 6u) & 63u;
    if (qx == 0u || qy == 0u) { return vec3<f32>(0.0); }
    var x = f32(qx - 1u) / 31.0 - 1.0;
    var y = f32(qy - 1u) / 31.0 - 1.0;
    let z = 1.0 - abs(x) - abs(y);
    if (z < 0.0) {
        let ox = x;
        let oy = y;
        x = (1.0 - abs(oy)) * select(-1.0, 1.0, ox >= 0.0);
        y = (1.0 - abs(ox)) * select(-1.0, 1.0, oy >= 0.0);
    }
    return normalize(vec3<f32>(x, y, z));
}

/// How steeply the faces beside an edge are seen at `p`: the depth a face gains per pixel across
/// the edge, in pixels (tan of the angle between its normal and the view), the larger of the two;
/// 1 when neither is known. Capped, so a face seen exactly edge on does not lift the band far.
fn grazing(p: vec3<f32>, na: vec3<f32>, nb: vec3<f32>) -> f32 {
    var toward = normalize(u.back.xyz);
    if (u.eye.w > 0.5) { toward = normalize(u.eye.xyz - p); }
    var g = 1.0;
    for (var i = 0; i < 2; i++) {
        let n = select(nb, na, i == 0);
        if (dot(n, n) < 0.25) { continue; }
        let d = abs(dot(n, toward));
        g = max(g, sqrt(max(1.0 - d * d, 0.0)) / max(d, 0.02));
    }
    return min(g, 50.0);
}

/// Clip z of the world point `p` (clip position `c`) moved `px` pixels toward the eye: the front
/// of a thin cylinder around an edge, so the band drawn along it is not buried by a face next to
/// the edge seen at a grazing angle. Measured on the screen, it is the same at every zoom.
fn lifted_z(p: vec3<f32>, c: vec4<f32>, px: f32, half: vec2<f32>) -> f32 {
    let b = normalize(u.back.xyz);
    var a = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(b.z) > 0.9) { a = vec3<f32>(1.0, 0.0, 0.0); }
    let side = normalize(cross(b, a));
    let cs = u.vp * vec4<f32>(p + side, 1.0);
    if (cs.w <= 1e-6) { return c.z; }
    // Pixels per millimetre across the view at the point's depth.
    let ppm = length((cs.xy / cs.w - c.xy / c.w) * half);
    if (ppm < 1e-9) { return c.z; }
    var toward = b;
    if (u.eye.w > 0.5) { toward = normalize(u.eye.xyz - p); }
    let q = u.vp * vec4<f32>(p + toward * (px / ppm), 1.0);
    if (q.w <= 1e-6) { return c.z; }
    // The same depth at the original w, so the band stays where it was on the screen.
    return q.z / q.w * c.w;
}

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) local_a: vec3<f32>, @location(1) local_b: vec3<f32>, @location(2) c: u32, @location(3) wbits: u32) -> LOut {
    return line_vertex(vi, local_a, local_b, c, wbits);
}

/// A scene's own lines drawn again in another colour and width (a whole hovered body's edges,
/// `LitEdges`): the draw's first vertex carries them, as `6 * style + corner`, so the lines are
/// drawn from the buffer they are already in. `style`: the palette index from bit 8, a raised
/// line (`lifted_width`, no normals) at bit 7, and the width in eighths of a pixel below it.
@vertex
fn vs_line_lit(@builtin(vertex_index) vi: u32, @location(0) local_a: vec3<f32>, @location(1) local_b: vec3<f32>, @location(2) c: u32, @location(3) wbits: u32) -> LOut {
    let style = vi / 6u;
    let eighths = style & 127u;
    var w = bitcast<u32>(f32(eighths) / 8.0);
    if ((style & 128u) != 0u) { w = 0x80000000u | (eighths << 24u); }
    return line_vertex(vi % 6u, local_a, local_b, style >> 8u, w);
}

fn line_vertex(vi: u32, local_a: vec3<f32>, local_b: vec3<f32>, c: u32, wbits: u32) -> LOut {
    let a = placed(local_a);
    let b = placed(local_b);
    var pa = a;
    var pb = b;
    var ca = u.vp * vec4<f32>(a, 1.0);
    var cb = u.vp * vec4<f32>(b, 1.0);
    // An end behind the near plane (clip z < 0) has no screen point: its divide flips the
    // line or drops it whole (ground grid lines running past the eye at a low angle). Cut
    // the segment at the plane instead; clip space is linear in the world point.
    let za = near_z(ca);
    let zb = near_z(cb);
    if (za < 0.0 && zb > 0.0) {
        let k = za / (za - zb);
        ca = mix(ca, cb, k);
        pa = mix(a, b, k);
    } else if (zb < 0.0 && za > 0.0) {
        let k = zb / (zb - za);
        cb = mix(cb, ca, k);
        pb = mix(b, a, k);
    }
    let half = u.screen.xy * 0.5;
    let sa = ca.xy / max(ca.w, 1e-6) * half;
    let sb = cb.xy / max(cb.w, 1e-6) * half;
    var dir = sb - sa;
    let len = length(dir);
    // Intel OpenGL can miscompile vector/scalar division here (#8053). Scalar
    // quotients keep the same direction and the fallback for a point.
    if (len < 1e-6) { dir = vec2<f32>(1.0, 0.0); } else { dir = vec2<f32>(dir.x / len, dir.y / len); }
    let nrm = vec2<f32>(-dir.y, dir.x);
    // Two triangles: corners (t, side).
    var ts = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var ss = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let t = ts[vi];
    let s = ss[vi];
    let base = select(ca, cb, t > 0.5);
    // A line thinner than a pixel is drawn a pixel wide, faded by the width it lacks: a narrower
    // quad covers no pixel centre when it falls between two rows, so without multisampling thin
    // lines (the grid's) came and went as the view zoomed (#2823).
    // The top bit marks a lifted highlight (`GpuScene::lifted_line`, `lifted_width`); a plain
    // line's slot is its width as an f32.
    let lifted = (wbits & 0x80000000u) != 0u;
    var wa = bitcast<f32>(wbits);
    if (lifted) { wa = f32((wbits >> 24u) & 127u) / 8.0; }
    let wd = max(wa, 1.0);
    let off = nrm * s * wd * 0.5 + dir * (t * 2.0 - 1.0) * wd * 0.5;
    var z = base.z;
    if (lifted && base.w > 1e-6) {
        let p = select(pa, pb, t > 0.5);
        let g = grazing(p, turned(octahedral(wbits & 4095u)), turned(octahedral((wbits >> 12u) & 4095u)));
        z = lifted_z(p, base, wd * 0.5 * g + 1.0, half);
    }
    var o: LOut;
    o.pos = vec4<f32>(base.xy + off / half * base.w, z, base.w);
    let col = palette(c);
    o.c = vec4<f32>(col.rgb, col.a * (wa / wd));
    o.wp = select(pa, pb, t > 0.5);
    return o;
}

@fragment
fn fs_line(i: LOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return out_color(i.c);
}

struct IOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) a: f32,
    @location(2) wp: vec3<f32>,
};

@group(1) @binding(0) var img_t: texture_2d<f32>;
@group(1) @binding(1) var img_s: sampler;

@vertex
fn vs_img(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) a: f32) -> IOut {
    var o: IOut;
    // On a face it shows over it: the face is pushed back (`FACE_BIAS`).
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.uv = uv;
    o.a = a;
    o.wp = p;
    return o;
}

@fragment
fn fs_img(i: IOut) -> @location(0) vec4<f32> {
    let c = textureSample(img_t, img_s, i.uv);
    if (clipped(i.wp)) { discard; }
    return out_color(vec4<f32>(c.rgb, c.a * i.a));
}

/// Lines the section never cuts (the grid, highlights drawn on top).
@fragment
fn fs_line_all(i: LOut) -> @location(0) vec4<f32> {
    return out_color(i.c);
}

// Point clouds and their cells (`view/clouds.rs`): dots, segments as beads (or lines when they
// have no size) and triangles, from one record per element. The cloud's style colours them by
// their own colour, a ramp or a palette, and shows only those whose filtered values lie in its
// ranges or sets; values vary across an element, so a range cuts it where its value crosses an end.

struct Cloud {
    style: vec4<f32>,               // x: colour (0 own, 1 ramp, 2 palette), y: dot size (px), z, w: ramp range
    filters: array<vec4<f32>, 4>,   // x: kind (0 off, 1 range y…z, 2 whole values y + i for the bits i of w)
};
@group(1) @binding(3) var<uniform> cloud: Cloud;

struct CIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) c: vec3<f32>,
    @location(8) before: vec3<f32>,
    @location(9) after: vec3<f32>,
    @location(3) v: vec3<f32>,
    @location(4) f0: vec3<f32>,
    @location(5) f1: vec3<f32>,
    @location(10) f2: vec3<f32>,
    @location(11) f3: vec3<f32>,
    @location(6) size: vec2<f32>,
    @location(7) rgb: vec4<f32>,
};

struct COut {
    @builtin(position) pos: vec4<f32>,
    @location(0) wp: vec3<f32>,
    @location(1) n: vec3<f32>,     // zero: not lit
    @location(2) v: f32,
    @location(3) f: vec4<f32>,
    @location(4) rgb: vec3<f32>,
    @location(5) at: vec2<f32>,    // a dot's place on its disc
    @location(6) @interpolate(flat) has: u32,
};

fn cloud_out(i: CIn, w: vec3<f32>, wp: vec3<f32>, pos: vec4<f32>, n: vec3<f32>) -> COut {
    var o: COut;
    o.pos = pos;
    o.wp = wp;
    o.n = n;
    o.v = dot(w, i.v);
    o.f = vec4<f32>(dot(w, i.f0), dot(w, i.f1), dot(w, i.f2), dot(w, i.f3));
    o.rgb = i.rgb.rgb;
    o.at = vec2<f32>(0.0);
    o.has = u32(round(i.rgb.a * 255.0));
    return o;
}

fn behind(c: vec4<f32>) -> bool {
    return near_z(c) < 0.0;
}

@vertex
fn vs_cloud_dot(@builtin(vertex_index) vi: u32, i: CIn) -> COut {
    var xs = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var ys = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let at = vec2<f32>(xs[vi], ys[vi]);
    let size = max(cloud.style.y, 1.0);
    let centre = u.vp * vec4<f32>(i.a, 1.0);
    var pos = vec4<f32>(centre.xy + at * size / u.screen.xy * centre.w, centre.z, centre.w);
    if (behind(centre)) { pos = vec4<f32>(0.0, 0.0, 0.0, -1.0); }
    var o = cloud_out(i, vec3<f32>(1.0, 0.0, 0.0), i.a, pos, vec3<f32>(0.0));
    // Dots under two pixels are squares: a disc that small loses its centre pixel.
    o.at = at * select(1.0, 0.0, size < 2.0);
    return o;
}

/// The least half-width of a segment on screen, in pixels.
const MIN_HALF_PX: f32 = 0.75;

/// The frame of a bead along `d`: along it, across it level, and up, its height axis.
fn bead_frame(d: vec3<f32>) -> mat3x3<f32> {
    var t = vec3<f32>(1.0, 0.0, 0.0);
    if (length(d) > 1e-9) { t = d / length(d); }
    var side = cross(vec3<f32>(0.0, 0.0, 1.0), t);
    if (length(side) < 1e-6) { side = vec3<f32>(1.0, 0.0, 0.0); }
    side = normalize(side);
    return mat3x3<f32>(t, side, cross(t, side));
}

/// Corner `j` (0…9) of a bead's cross-section, a stadium: its place across and up, and its
/// normal. Corners 0…4 go up the right half-disc, 5…9 down the left one, so 4–5 is the flat top
/// and 9–0 the flat bottom.
fn stadium(j: u32, size: vec2<f32>) -> vec4<f32> {
    let k = j % 10u;
    let right = k < 5u;
    let a = -1.5707963 + 0.7853982 * f32(k % 5u) + select(3.1415927, 0.0, right);
    let r = 0.5 * size.y;
    let flat_half = 0.5 * max(size.x - size.y, 0.0);
    return vec4<f32>(select(-flat_half, flat_half, right) + r * cos(a), r * sin(a), cos(a), sin(a));
}

/// Whether filter `f` keeps the value `v`.
fn cloud_keeps(f: vec4<f32>, v: f32) -> bool {
    if (f.x < 0.5) { return true; }
    if (f.x < 1.5) { return v >= f.y && v <= f.z; }
    let k = i32(round(v)) - i32(round(f.y));
    return k >= 0 && k < 32 && ((bitcast<u32>(f.w) >> u32(k)) & 1u) != 0u;
}

/// Whether filter `f` leaves out a whole element with values `v` at its corners: all on one
/// side of a range, or one value at every corner, not kept. A set of values with corners that
/// differ may keep part of the element: its fragments decide.
fn cloud_drops(f: vec4<f32>, v: vec3<f32>) -> bool {
    if (f.x > 1.5) { return all(v == vec3<f32>(v.x)) && !cloud_keeps(f, v.x); }
    return f.x > 0.5 && (all(v < vec3<f32>(f.y)) || all(v > vec3<f32>(f.z)));
}

/// Whether a filter leaves out the whole element.
fn filtered_out(i: CIn) -> bool {
    let has = u32(round(i.rgb.a * 255.0));
    let values = array<vec3<f32>, 4>(i.f0, i.f1, i.f2, i.f3);
    for (var k = 0u; k < 4u; k++) {
        if (((has >> k) & 1u) != 0u && cloud_drops(cloud.filters[k], values[k])) { return true; }
    }
    return false;
}

/// The part in front of the near plane of the segment between clip-space `a` and `b`, as
/// fractions of it from `a`; empty (first above second) when all of it is behind. Clip-space
/// coordinates are affine along a line, so the crossing is exact.
fn in_front(a: vec4<f32>, b: vec4<f32>) -> vec2<f32> {
    let za = near_z(a);
    let zb = near_z(b);
    if (za < 0.0 && zb < 0.0) { return vec2<f32>(1.0, 0.0); }
    let t = za / (za - zb);
    return vec2<f32>(select(0.0, t, za < 0.0), select(1.0, t, zb < 0.0));
}

@vertex
fn vs_cloud_segment(@builtin(vertex_index) vi: u32, i: CIn) -> COut {
    // Corner `vi % 10` of the cross-section at the start (`vi < 10`) or the end; the index
    // buffer joins them into the bead's sides.
    let pa = u.vp * vec4<f32>(i.a, 1.0);
    let pb = u.vp * vec4<f32>(i.b, 1.0);
    let part = in_front(pa, pb);
    if (filtered_out(i) || part.x > part.y) {
        return cloud_out(i, vec3<f32>(1.0, 0.0, 0.0), i.a, vec4<f32>(0.0, 0.0, 0.0, -1.0), vec3<f32>(0.0));
    }
    // Where the near plane cuts it, the bead ends there, square; its numbers are exact at `e`.
    let at_end = vi >= 10u;
    let e = select(part.x, part.y, at_end);
    let cut = e != select(0.0, 1.0, at_end);
    let f = bead_frame(i.b - i.a);
    let c = stadium(vi % 10u, i.size);
    let axis = mix(i.a, i.b, e);
    // Where it meets the next or the previous bead, the cross-section lies in the plane halfway
    // between their directions, stretched across to keep its width: both beads end on the same
    // section. A turn sharper than 120° ends square instead.
    let other = select(i.a - i.before, i.after - i.b, at_end);
    var across_dir = f[1];
    var stretch = 1.0;
    if (!cut && length(other) > 1e-9) {
        let g = bead_frame(other);
        let mid = f[1] + g[1];
        if (length(mid) > 1e-6 && dot(normalize(mid), f[1]) > 0.5) {
            across_dir = normalize(mid);
            stretch = 1.0 / dot(across_dir, f[1]);
        }
    }
    let wp = axis + across_dir * (c.x * stretch) + f[2] * c.y;
    let ca = u.vp * vec4<f32>(axis, 1.0);
    let cw = u.vp * vec4<f32>(wp, 1.0);
    let half = u.screen.xy * 0.5;
    var pos = cw;
    // At least a pixel and a half across: a bead seen from afar, or a line without size. A corner
    // behind the eye has no place on the screen and needs no widening: the rasteriser clips it.
    if (cw.w > 0.0) {
        let c0 = mix(pa, pb, part.x);
        let c1 = mix(pa, pb, part.y);
        let along = (c1.xy / c1.w - c0.xy / c0.w) * half;
        var across = vec2<f32>(-along.y, along.x);
        if (length(across) < 1e-6) { across = vec2<f32>(1.0, 0.0); }
        across = normalize(across);
        var px = (cw.xy / cw.w - ca.xy / ca.w) * half;
        if (length(px) < MIN_HALF_PX * abs(c.z)) {
            px = across * c.z * MIN_HALF_PX;
            pos = vec4<f32>((ca.xy / ca.w + px / half) * cw.w, cw.z, cw.w);
        }
    }
    let n = select(f[1] * c.z + f[2] * c.w, vec3<f32>(0.0), i.size.y <= 0.0);
    return cloud_out(i, vec3<f32>(1.0 - e, e, 0.0), wp, pos, n);
}

@vertex
fn vs_cloud_triangle(@builtin(vertex_index) vi: u32, i: CIn) -> COut {
    // Not `w[vi] = 1.0`: the Windows D3D12 shader compiler cannot write through a dynamic vector index.
    let w = vec3<f32>(f32(vi == 0u), f32(vi == 1u), f32(vi == 2u));
    let wp = i.a * w.x + i.b * w.y + i.c * w.z;
    var n = cross(i.b - i.a, i.c - i.a);
    if (length(n) > 0.0) { n = normalize(n); }
    // The true clip-space corner, even behind the eye: the rasteriser clips a triangle exactly.
    return cloud_out(i, w, wp, u.vp * vec4<f32>(wp, 1.0), n);
}

fn cloud_ramp(t: f32) -> vec3<f32> {
    var stops = array<vec3<f32>, 5>(
        vec3<f32>(40.0, 70.0, 200.0), vec3<f32>(30.0, 170.0, 230.0), vec3<f32>(60.0, 200.0, 90.0),
        vec3<f32>(240.0, 220.0, 60.0), vec3<f32>(230.0, 70.0, 50.0));
    let k = clamp(t, 0.0, 1.0) * 4.0;
    let j = min(u32(k), 3u);
    return mix(stops[j], stops[j + 1u], k - f32(j)) / 255.0;
}

fn cloud_palette(v: f32) -> vec3<f32> {
    var colours = array<vec3<f32>, 10>(
        vec3<f32>(240.0, 140.0, 40.0), vec3<f32>(245.0, 205.0, 65.0), vec3<f32>(230.0, 65.0, 65.0),
        vec3<f32>(165.0, 75.0, 190.0), vec3<f32>(65.0, 190.0, 115.0), vec3<f32>(75.0, 150.0, 220.0),
        vec3<f32>(240.0, 120.0, 180.0), vec3<f32>(140.0, 105.0, 75.0), vec3<f32>(120.0, 215.0, 215.0),
        vec3<f32>(190.0, 190.0, 80.0));
    if (v < -0.5) { return vec3<f32>(150.0, 160.0, 176.0) / 255.0; }
    return colours[u32(round(v)) % 10u] / 255.0;
}


@fragment
fn fs_cloud(o: COut) -> @location(0) vec4<f32> {
    var out = clipped(o.wp) || dot(o.at, o.at) > 1.0;
    for (var k = 0u; k < 4u; k++) {
        out = out || (((o.has >> k) & 1u) != 0u && !cloud_keeps(cloud.filters[k], o.f[k]));
    }
    if (out) { discard; }
    var c = o.rgb;
    let mode = cloud.style.x;
    if ((o.has & 16u) != 0u && mode > 1.5) {
        c = cloud_palette(o.v);
    } else if ((o.has & 16u) != 0u && mode > 0.5) {
        c = cloud_ramp((o.v - cloud.style.z) / max(cloud.style.w - cloud.style.z, 1e-12));
    }
    if (length(o.n) > 0.5) {
        c = c * (0.45 + 0.55 * abs(dot(normalize(o.n), normalize(u.back.xyz))));
    }
    return out_color(vec4<f32>(c, 1.0));
}
