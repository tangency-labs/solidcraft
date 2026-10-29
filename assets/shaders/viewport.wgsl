
struct U {
    vp: mat4x4<f32>,
    back: vec4<f32>,   // xyz = toward the eye, w = 1 for linear output
    screen: vec4<f32>, // viewport width, height (px), unused, silhouettes on (1)
    clip: vec4<f32>,   // section plane: normal, d (zero normal: no section)
    cap: vec4<f32>,    // colour of the inside of cut bodies
    ana: vec4<f32>,    // surface analysis: mode (1 zebra, 2 draft, 3 curvature), parameter
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
    tint: vec4<f32>,   // xyz added to a whole hovered body's colour before lighting
    sel: vec4<f32>,    // xyz a whole selected body's colour before lighting
    product_lights_meta: vec4<f32>, // explicit preview flag and bounded count
    product_lights: array<vec4<f32>,84>, // position/kind, radiance, axis/cone, attenuation
};

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

fn clipped(p: vec3<f32>) -> bool {
    return length(u.clip.xyz) > 0.5 && dot(u.clip.xyz, p) > u.clip.w;
}
@group(0) @binding(0) var<uniform> u: U;
// The scene's colours, 2048 entries to a row: an R32Uint texture rather than a storage buffer,
// so the viewport draws on WebGL2 (which has no storage buffers) as well.
@group(0) @binding(1) var pal: texture_2d<u32>;

/// The colour at palette entry `i`, as byte/255 (the old Unorm8x4 conversion, bit for bit).
fn palette(i: u32) -> vec4<f32> {
    let v = textureLoad(pal, vec2<i32>(i32(i & 2047u), i32(i >> 11u)), 0).x;
    return vec4<f32>(f32(v & 255u), f32((v >> 8u) & 255u), f32((v >> 16u) & 255u), f32((v >> 24u) & 255u)) / 255.0;
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
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) c: vec4<f32>,
    @location(2) wp: vec3<f32>,
};

@vertex
fn vs_tri(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: u32) -> TOut {
    var o: TOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.n = n;
    o.c = palette(c);
    o.wp = p;
    return o;
}

/// Shared triangles (in their part's own frame) drawn at one placement: the rows of its rigid
/// move (`r0`..`r2`, a point's coordinates are their dot products with (p, 1)) and its colour.
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
    let wp = vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q));
    var o: TOut;
    o.pos = u.vp * vec4<f32>(wp, 1.0);
    o.n = vec3<f32>(dot(r0.xyz, n), dot(r1.xyz, n), dot(r2.xyz, n));
    o.c = palette(c);
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
    @builtin(position) pos: vec4<f32>,
    @location(0) rp: vec3<f32>,
    @location(1) c: vec4<f32>,
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
    let wp = vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q));
    var o: FOut;
    o.pos = u.vp * vec4<f32>(wp, 1.0);
    o.rp = wp - u.eye.xyz;
    o.c = palette(c);
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

/// Opaque model surfaces: cut by the section plane/// Opaque model surfaces: cut by the section plane, the inside shows as a flat cap colour.
@fragment
fn fs_solid(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = solid(i, front);
    return c;
}

struct CutOut {
    @location(0) c: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

/// The section cap (the inside seen through the cut, drawn on the far walls' back faces where
/// they are the visible surface) again, at the depth of the section plane rather than of the
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
        // cap colour so it reads as a cut, hatched at 45 degrees on screen; with the hatch off
        // (`u.cap.a` 0), plain in the body's own colour.
        if (u.cap.a < 0.5) { return out_color(vec4<f32>(i.c.rgb, 1.0)); }
        let base = mix(i.c.rgb, u.cap.rgb, 0.35);
        let h = fract((i.pos.x + i.pos.y) / 9.0);
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

/// The AO prepass's depth of an opaque surface's fragment.
fn opaque_depth(i: TOut, front: bool) -> AoDepth {
    if (clipped(i.wp) || length(i.n) < 0.5) { discard; }
    var depth = i.pos.z;
    if (!front && length(u.clip.xyz) > 0.5) { depth = cap_depth(i.wp, i.pos.z); }
    return AoDepth(depth, bitcast<u32>(depth));
}

// Cosine-weighted hemisphere samples, the pattern turned by a different angle at each pixel.
// Projecting the sample and a point one radius toward the eye supplies a depth range in both
// orthographic and perspective views; distant foreground geometry cannot cast a dark halo onto
// a background surface.
fn occlusion(i: TOut) -> f32 {
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
    // stepped bands along contacts where it is small (#3407). Turned by interleaved gradient
    // noise (Jimenez 2014), neighbouring pixels sample different points and the copies break up
    // into an even grain.
    let turn = fract(52.9829189 * fract(dot(i.pos.xy, vec2<f32>(0.06711056, 0.00583715))));
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
        // Reflections of upright light bars standing around the viewer: the stripe follows
        // the reflected ray's sideways direction (horizontal on screen).
        let r = reflect(-v, n);
        var side = cross(v, vec3<f32>(0.0, 0.0, 1.0));
        if (length(side) < 1e-3) { side = vec3<f32>(1.0, 0.0, 0.0); }
        let s = sin(dot(r, normalize(side)) * u.ana.y * 3.14159265);
        let c = select(0.08, 0.95, s > 0.0);
        return out_color(vec4<f32>(vec3<f32>(c), 1.0));
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
    let c = select(mix(u.ana_c[6].rgb, u.ana_c[7].rgb, t * 2.0 - 1.0), mix(u.ana_c[5].rgb, u.ana_c[6].rgb, t * 2.0), t < 0.5);
    return out_color(vec4<f32>(c * lit, 1.0));
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
    return lit_color(i, 1.0, vec4<f32>(i.c.rgb + u.tint.rgb, i.c.a));
}

/// A whole selected body, lit over the model from its own plan triangles in the selection colour.
@fragment
fn fs_hl_selected(i: TOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp)) { discard; }
    return lit_color(i, 1.0, vec4<f32>(u.sel.rgb, i.c.a));
}

/// Reference planes: shaded like `fs_tri`, but never cut by the section plane.
@fragment
fn fs_tri_all(i: TOut) -> @location(0) vec4<f32> {
    return shade(i);
}

fn shade(i: TOut) -> vec4<f32> {
    return lit(i, 1.0);
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

fn lit(i: TOut, visibility: f32) -> vec4<f32> {
    return lit_color(i, visibility, i.c);
}

/// [`lit`] in a colour of its own: a whole highlighted body is lit in its colour lifted by the
/// hover amount, or in the selection colour, from the model's own plan triangles.
fn lit_color(i: TOut, visibility: f32, base: vec4<f32>) -> vec4<f32> {
    // A zero normal marks flat colour (selection fills, translucent planes).
    if (length(i.n) < 0.5) {
        return out_color(base);
    }
    let v = normalize(u.back.xyz);
    var n = normalize(i.n);
    if (dot(n, v) < 0.0) { n = -n; }
    if u.product_lights_meta.x>0.0 {
        var radiance=vec3<f32>(0.0);
        for (var k=0u;k<min(u32(u.product_lights_meta.y),21u);k+=1u) {
            if u.product_lights[k*4u].w == 4.0 {radiance+=to_linear(base.rgb)*u.product_lights[k*4u+1u].xyz/3.141592653589793;continue;}
            let sample=product_light_sample(k,i.wp);
            radiance+=to_linear(base.rgb)*sample[1]*(max(dot(n,sample[0]),0.0)/3.141592653589793);
        }
        let mapped=radiance/(vec3<f32>(1.0)+radiance);
        let encoded=select(mapped*12.92,1.055*pow(mapped,vec3<f32>(1.0/2.4))-vec3<f32>(0.055),mapped>vec3<f32>(0.0031308));
        return out_color(vec4<f32>(encoded,base.a));
    }
    let key = normalize(v * 0.8 + vec3<f32>(-0.3, 0.2, 0.6));
    let diff = max(dot(n, key), 0.0);
    let fill = max(dot(n, v), 0.0);
    let h = normalize(key + v);
    let spec = pow(max(dot(n, h), 0.0), 40.0) * 0.25;
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
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) c: u32, @location(3) wbits: u32) -> LOut {
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
    if (len < 1e-6) { dir = vec2<f32>(1.0, 0.0); } else { dir = dir / len; }
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
        let g = grazing(p, octahedral(wbits & 4095u), octahedral((wbits >> 12u) & 4095u));
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

// Point clouds: each point is a round dot a few pixels across, facing the screen, depth-tested
// and written like the model's faces, so the model hides the points behind it and they hide it.
// The colour's alpha carries the dot's size (eighths of a pixel).

struct POut {
    @builtin(position) pos: vec4<f32>,
    @location(0) c: vec4<f32>,
    @location(1) wp: vec3<f32>,
    @location(2) at: vec2<f32>,
};

@vertex
fn vs_point(@builtin(vertex_index) vi: u32, @location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> POut {
    var xs = array<f32, 6>(-1.0, 1.0, 1.0, -1.0, 1.0, -1.0);
    var ys = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let at = vec2<f32>(xs[vi], ys[vi]);
    let size = max(c.a * 255.0 / 8.0, 1.0);
    let centre = u.vp * vec4<f32>(p, 1.0);
    let half = u.screen.xy * 0.5;
    var o: POut;
    o.pos = vec4<f32>(centre.xy + at * size * 0.5 / half * centre.w, centre.z, centre.w);
    // Behind the eye: nothing.
    if (near_z(centre) < 0.0) { o.pos = vec4<f32>(0.0, 0.0, 0.0, -1.0); }
    o.c = vec4<f32>(c.rgb, 1.0);
    o.wp = p;
    // Dots under two pixels are squares: a disc that small loses its centre pixel.
    o.at = at * select(1.0, 0.0, size < 2.0);
    return o;
}

@fragment
fn fs_point(i: POut) -> @location(0) vec4<f32> {
    if (clipped(i.wp) || dot(i.at, i.at) > 1.0) { discard; }
    return out_color(i.c);
}
