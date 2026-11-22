
struct U {
    vp: mat4x4<f32>,
    back: vec4<f32>,   // xyz = toward the eye, w = 1 for linear output
    screen: vec4<f32>, // viewport width, height (px), one past the last motion slot that moves (0: none), silhouettes on (1)
    clip: array<vec4<f32>, 6>, // section planes: normal, d each, the first ones (zero normal: unused)
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
    depth: vec4<f32>,  // x = 1: reversed depth (1 near, 0 far); y, z: the faces' push (`FACE_BIAS`); w: the first motion slot that moves
    // Where the drawn geometry is moved in the world: identity, or a dragged component's move
    // from where its scene was built (`MovingLayer`)
    model: mat4x4<f32>,
    tint: vec4<f32>,   // xyz added to a whole hovered body's colour before lighting
    sel: vec4<f32>,    // xyz a whole selected body's colour before lighting
    // the section hatch colours by part (`crate::hatch`), the first the cap's own
    hatch: array<vec4<f32>, 3>,
    product_lights_meta: vec4<f32>, // explicit preview flag and bounded count
    product_lights: array<vec4<f32>,84>, // position/kind, radiance, axis/cone, attenuation
    inv_vp: mat4x4<f32>, // the inverse of `vp`: a pixel's point in the world from its depth
    plane_hatch: array<vec4<f32>, 6>, // each section plane's hatch: stripe phase pixel · xy (`section_hatch`)
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
@group(2) @binding(2) var ao_normal: texture_2d<f32>;

/// How far past a plane `p` may lie and still be kept: the f32 rounding of positions, so a face
/// lying on a plane stays whole.
fn clip_slack(p: vec3<f32>, c: vec4<f32>) -> f32 {
    return 1e-5 * (1.0 + abs(c.w) + length(p));
}

/// Whether the section hides `p`: it lies on the positive side of any plane.
fn clipped(p: vec3<f32>) -> bool {
    if (!sectioned()) { return false; }
    for (var k = 0u; k < 6u; k++) {
        let c = u.clip[k];
        if (length(c.xyz) > 0.5 && dot(c.xyz, p) > c.w + clip_slack(p, c)) { return true; }
    }
    return false;
}

fn sectioned() -> bool {
    return length(u.clip[0].xyz) > 0.5;
}
@group(0) @binding(0) var<uniform> u: U;
// The scene's colours, 2048 entries to a row: an Rg32Uint texture rather than a storage buffer,
// so the viewport draws on WebGL2 (which has no storage buffers) as well. Each entry is the
// colour's bytes, then 1 + the section hatch of the body it colours (0: the cap's own).
@group(0) @binding(1) var pal: texture_2d<u32>;

fn palette_entry(i: u32) -> vec2<u32> {
    return textureLoad(pal, vec2<i32>(i32(i & 2047u), i32(i >> 11u)), 0).xy;
}

// The rigid motions bodies are drawn moved by (`motions.rs`): three texels a slot, the rows of
// the motion less the identity (a zeroed slot moves nothing), 512 slots to a row. A palette
// entry names its body's slot above the hatch byte of its second word. A texture like the
// palette, so WebGL2 reads it in the vertex stage too.
@group(0) @binding(7) var motions: texture_2d<f32>;

/// The motion slot palette entry `e` names when it is one that moves this frame, else 0: only
/// a slot from `U.depth.w` up to `U.screen.z` has a motion to read, so a frame that moves
/// nothing, and every body outside that range, reads nothing more than it did.
fn motion_of(e: vec2<u32>) -> u32 {
    let m = f32(e.y >> 8u);
    if (m >= u.screen.z || m < u.depth.w) { return 0u; }
    return e.y >> 8u;
}

/// A placement's motion: the slot `s` its own word names, else the one its entry `e` names
/// (`motion_of`). Copies of a part share their entries, not their motions.
fn placement_motion(s: u32, e: vec2<u32>) -> u32 {
    if (s == 0u) { return motion_of(e); }
    let m = f32(s);
    if (m >= u.screen.z || m < u.depth.w) { return 0u; }
    return s;
}

/// `motion_of` for palette index `c`, for a stage that does not read the entry otherwise.
fn motion_at(c: u32) -> u32 {
    if (u.screen.z < 0.5) { return 0u; }
    return motion_of(palette_entry(c));
}

/// Row `k` of motion `m`, less the identity.
fn motion_row(m: u32, k: i32) -> vec4<f32> {
    return textureLoad(motions, vec2<i32>(i32(m & 511u) * 3 + k, i32(m >> 9u)), 0);
}

/// A body's point moved by its motion `m`, then placed like the rest of its scene (`placed`).
fn placed_by(m: u32, p: vec3<f32>) -> vec3<f32> {
    if (m == 0u) { return placed(p); }
    let q = vec4<f32>(p, 1.0);
    return placed(p + vec3<f32>(dot(motion_row(m, 0), q), dot(motion_row(m, 1), q), dot(motion_row(m, 2), q)));
}

/// A body's direction turned by its motion `m`, then like the rest of its scene (`turned`).
fn turned_by(m: u32, n: vec3<f32>) -> vec3<f32> {
    if (m == 0u) { return turned(n); }
    return turned(n + vec3<f32>(dot(motion_row(m, 0).xyz, n), dot(motion_row(m, 1).xyz, n), dot(motion_row(m, 2).xyz, n)));
}

// The palette's PBR materials (`Material` in view.rs): for palette row r, texel row 4r holds each
// entry's linear base colour and a PBR flag, row 4r + 1 its metallic fraction, roughness and IOR,
// rows 4r + 2 and 4r + 3 its wood frame's origin and fibre axis (`wood_coordinates`).
@group(0) @binding(2) var pbr_materials: texture_2d<f32>;

/// Palette entry `i`'s hatch word (`hatch`, under 256) with the entry's PBR finish packed above
/// it: metallic in bits 8-15, roughness in 16-23, bit 24 set when the entry has one. An entry
/// past the material texture has none (a load out of range is not defined).
fn finish_h(i: u32, hatch: u32) -> u32 {
    let at = vec2<u32>(i & 2047u, (i >> 11u) * 4u);
    if (at.y + 1u >= textureDimensions(pbr_materials).y || textureLoad(pbr_materials, at, 0).w < 0.5) { return hatch; }
    let p = clamp(textureLoad(pbr_materials, at + vec2<u32>(0u, 1u), 0).xy, vec2<f32>(0.0), vec2<f32>(1.0));
    let q = vec2<u32>(round(p * 255.0));
    return hatch | (q.x << 8u) | (q.y << 16u) | (1u << 24u);
}

/// Palette entry `slot`'s wood grain coordinates at `p`: the offset across the fibre axis (xy, in
/// a pair of directions square to the axis that turns with the body, `Material::wood_turn`, so
/// its length is the distance from the growth axis), one more than the entry's wood kind (z,
/// `Material::wood_kind`) and the distance along
/// the axis (w). An original cylindrical growth field: continuous on every face, with rings on
/// the end faces. Zero where the entry has no wood frame, which keeps its uniform colour exactly.
fn wood_coordinates(p: vec3<f32>, slot: u32) -> vec4<f32> {
    let at = vec2<u32>(slot & 2047u, (slot >> 11u) * 4u);
    // An entry past the material texture has none (a load out of range is not defined).
    if (at.y + 3u >= textureDimensions(pbr_materials).y) { return vec4<f32>(0.0); }
    let origin_kind = textureLoad(pbr_materials, at + vec2<u32>(0u, 2u), 0);
    let origin = origin_kind.xyz;
    let axis = textureLoad(pbr_materials, at + vec2<u32>(0u, 3u), 0);
    let delta = (p - origin) * axis.w;
    let along = dot(delta, axis.xyz);
    // Two unit directions square to the axis and to each other, the same for every point of the
    // entry (the sign keeps the division away from zero for an axis near -Z).
    let s = select(-1.0, 1.0, axis.z >= -1e-4);
    let a = -1.0 / (s + axis.z);
    let b = axis.x * axis.y * a;
    let e1 = vec3<f32>(1.0 + s * axis.x * axis.x * a, s * b, -s * axis.x);
    let e2 = vec3<f32>(b, s + axis.y * axis.y * a, -axis.y);
    // The entry's own turn about the axis (the fraction of `origin_kind.w`, in whole turns): the
    // figure is not round, so it must turn with a body that turns about its fibre axis.
    let turn = fract(origin_kind.w) * 6.2831853;
    let across = vec2<f32>(dot(delta, e1), dot(delta, e2));
    let turned = vec2<f32>(cos(turn) * across.x + sin(turn) * across.y, cos(turn) * across.y - sin(turn) * across.x);
    return vec4<f32>(turned, (floor(origin_kind.w) + 1.0) * axis.w, along);
}

/// A palette entry's colour bytes as byte/255 (the old Unorm8x4 conversion, bit for bit).
fn unpack_color(v: u32) -> vec4<f32> {
    return vec4<f32>(f32(v & 255u), f32((v >> 8u) & 255u), f32((v >> 16u) & 255u), f32((v >> 24u) & 255u)) / 255.0;
}

/// The colour at palette entry `i`.
fn palette(i: u32) -> vec4<f32> {
    return unpack_color(palette_entry(i).x);
}

/// A display colour as the target wants it: decoded to linear light for an sRGB target, whose
/// encoding then gives the same bytes back.
fn out_color(c: vec4<f32>) -> vec4<f32> {
    if (u.back.w > 0.5) {
        return vec4<f32>(srgb_decode(c.rgb), c.a);
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
    // The wood grain coordinates (`wood_coordinates`): zero where the entry has no wood frame.
    @location(6) wood: vec4<f32>,
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
    let e = palette_entry(c);
    let m = motion_of(e);
    let p = placed_by(m, local);
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.n = turned_by(m, n);
    o.c = unpack_color(e.x);
    o.h = finish_h(c, e.y & 255u);
    o.wp = p;
    o.wood = wood_coordinates(local, c);
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
    @location(9) s: u32,
) -> TOut {
    let q = vec4<f32>(p, 1.0);
    let e = palette_entry(c);
    let m = placement_motion(s, e);
    let wp = placed_by(m, vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q)));
    let pn = vec3<f32>(dot(r0.xyz, n), dot(r1.xyz, n), dot(r2.xyz, n));
    var o: TOut;
    o.pos = u.vp * vec4<f32>(wp, 1.0);
    o.n = turned_by(m, pn);
    o.c = unpack_color(e.x);
    o.h = finish_h(c, e.y & 255u);
    o.wp = wp;
    o.wood = wood_coordinates(vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q)), c);
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
    @location(3) wood: vec4<f32>,
};

@vertex
fn vs_tri_inst_flat(
    @location(0) p: vec3<f32>,
    @location(3) r0: vec4<f32>,
    @location(4) r1: vec4<f32>,
    @location(5) r2: vec4<f32>,
    @location(6) c: u32,
    @location(9) s: u32,
) -> FOut {
    let q = vec4<f32>(p, 1.0);
    let e = palette_entry(c);
    let wp = placed_by(placement_motion(s, e), vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q)));
    var o: FOut;
    o.pos = u.vp * vec4<f32>(wp, 1.0);
    o.rp = wp - u.eye.xyz;
    o.c = unpack_color(e.x);
    o.h = finish_h(c, e.y & 255u);
    o.wood = wood_coordinates(vec3<f32>(dot(r0, q), dot(r1, q), dot(r2, q)), c);
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
    o.wood = i.wood;
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
    if (!past_entry(kept_span(t.wp))) { discard; }
    o.c = vec4<f32>(0.0);
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


/// Opaque model surfaces: cut by the section, the inside shows as a hatched cap.
@fragment
fn fs_solid(i: TOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = solid(i, front);
    return c;
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

/// The section cap's depth (`Cut::cap`; the inside seen through the cut, drawn on the far walls'
/// back faces where the stencil marks it) at the section plane rather than at the far wall, so
/// the cut face hides what lies inside the body instead of showing it like a pane of glass.
@fragment
fn fs_solid_cut(i: TOut, @builtin(front_facing) front: bool) -> CutOut {
    var o: CutOut;
    // Derivatives (in `pushed`) first, before the discard.
    o.depth = pushed(cap_depth(i.wp, i.pos.z));
    if (!past_entry(kept_span(i.wp))) { discard; }
    o.c = vec4<f32>(0.0);
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

fn view_ray(wp: vec3<f32>) -> vec3<f32> {
    if (u.eye.w > 0.5) { return wp - u.eye.xyz; }
    return -u.back.xyz;
}

/// The stretch `lo < t < hi` of the view ray `wp + view_ray(wp) * t` the section keeps (inside
/// every plane), and the plane `k` it comes in through (6: none).
struct Span {
    lo: f32,
    hi: f32,
    k: u32,
};

fn kept_span(wp: vec3<f32>) -> Span {
    let dir = view_ray(wp);
    var s = Span(-1e30, 1e30, 6u);
    for (var k = 0u; k < 6u; k++) {
        let c = u.clip[k];
        if (length(c.xyz) < 0.5) { continue; }
        // Kept: n·(wp + dir·t) <= d + slack, t·dn <= r.
        let dn = dot(c.xyz, dir);
        let r = c.w + clip_slack(wp, c) - dot(c.xyz, wp);
        if (abs(dn) < 1e-9) {
            if (r < 0.0) { return Span(1e30, -1e30, 6u); }
        } else if (dn > 0.0) {
            s.hi = min(s.hi, r / dn);
        } else if (r / dn > s.lo) {
            s.lo = r / dn;
            s.k = k;
        }
    }
    return s;
}

/// Whether the fragment lies past where its view ray comes into what the section keeps, that
/// point in front of the eye: the surfaces that say whether the point is inside a body (the cap).
fn past_entry(s: Span) -> bool {
    return s.k < 6u && s.lo < 0.0 && s.lo < s.hi && (u.eye.w < 0.5 || s.lo > -1.0);
}

/// Depth of the cut on the view ray through `wp` (a far wall seen through the cut), never
/// behind the wall itself (`z`): where the ray comes into what the section keeps.
fn cap_depth(wp: vec3<f32>, z: f32) -> f32 {
    let s = kept_span(wp);
    if (!past_entry(s)) { return z; }
    let c = u.vp * vec4<f32>(wp + view_ray(wp) * s.lo, 1.0);
    if (c.w <= 0.0) { return z; }
    if (reversed()) { return clamp(c.z / c.w, z, 1.0); }
    return clamp(c.z / c.w, 0.0, z);
}

/// The cut face in its body's colour ("from component" colouring), tinted toward the cap colour
/// so it reads as a cut, hatched at the angle of the plane it lies on; by part, toward the body's
/// own hatch colour, a turned part's stripes a quarter turn round. With the hatch off (`u.cap.a`
/// 0), plain in the body's own colour.
fn cap_color(i: TOut) -> vec4<f32> {
    if (u.cap.a < 0.5) { return out_color(vec4<f32>(i.c.rgb, 1.0)); }
    var tint = u.cap.rgb;
    var s = u.plane_hatch[min(kept_span(i.wp).k, 5u)].xy;
    if ((i.h & 255u) > 0u) {
        let k = (i.h & 255u) - 1u;
        tint = u.hatch[min(k >> 1u, 2u)].rgb;
        if ((k & 1u) == 1u) { s = vec2<f32>(-s.y, s.x); }
    }
    let base = mix(i.c.rgb, tint, 0.35);
    let h = fract(dot(i.pos.xy, s));
    return out_color(vec4<f32>(select(base, base * 0.6, h < 0.22), 1.0));
}

/// The section's crossing count (`Cut::count` in `view.rs`): each surface past where its view ray
/// comes into what the section keeps steps the stencil, back faces up and front faces down,
/// leaving whether that point (the cap) is inside a body. The colour is not written.
@fragment
fn fs_section_count(i: TOut) -> @location(0) vec4<f32> {
    if (!past_entry(kept_span(i.wp))) { discard; }
    return vec4<f32>(0.0);
}

@fragment
fn fs_section_count_flat(i: FOut) -> @location(0) vec4<f32> {
    if (!past_entry(kept_span(i.rp + u.eye.xyz))) { discard; }
    return vec4<f32>(0.0);
}

/// The cap where the count says the cut lies inside a body (`Cut::fill`): the nearest back face
/// past where the view ray comes into what the section keeps, in its cut colour. That face may be
/// hidden itself, where the material behind the cut reaches past another plane before its far
/// wall.
@fragment
fn fs_section_fill(i: TOut) -> @location(0) vec4<f32> {
    if (!past_entry(kept_span(i.wp))) { discard; }
    return cap_color(i);
}

@fragment
fn fs_section_fill_flat(i: FOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = flat_fragment(i, front);
    if (!past_entry(kept_span(t.wp))) { discard; }
    return cap_color(t);
}

fn solid(i: TOut, front: bool) -> vec4<f32> {
    // Derivatives first, in uniform control flow (browsers' WGSL compilers reject them after
    // a branch that returns; the whole shader module then fails and nothing is drawn).
    let k = curvature(i);
    let ndv = dot(normalize(i.n), normalize(u.back.xyz));
    let fw = fwidth(ndv);
    let wood_dx = dpdx(i.wood);
    let wood_dy = dpdy(i.wood);
    if (clipped(i.wp)) { discard; }
    if (!front && sectioned()) { return cap_color(i); }
    if (u.ana.x > 0.5 && length(i.n) > 0.5) {
        return analysis(i, k);
    }
    let c = lit(i, occlusion(i), wood_dx, wood_dy);
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
    // The surface's normal (0..1 per axis), alpha 1 where the surface takes occlusion: not on a
    // section's cut face (a back face of a sectioned body), nor where nothing is drawn (cleared).
    @location(1) normal: vec4<f32>,
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

/// The AO prepass's depth of an opaque surface's fragment, at its own depth: a section's caps
/// neither cast nor take occlusion (a cap's depth on a back face behind a kept face would be a
/// wall in mid-air, darkening the faces beside the cut).
fn opaque_depth(i: TOut, front: bool) -> AoDepth {
    if (clipped(i.wp) || length(i.n) < 0.5) { discard; }
    let takes = select(0.0, 1.0, front || !sectioned());
    return AoDepth(i.pos.z, bitcast<u32>(i.pos.z), vec4<f32>(normalize(i.n) * 0.5 + 0.5, takes));
}

/// The term of the surface the prepass left at this pixel of the maps: its point in the world from
/// its depth, its normal from the normal map. A pixel with no surface, or a section's cut face,
/// has none. Worked out once per pixel over the maps, without drawing the triangles again.
@fragment
fn fs_ao_term_screen(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = vec2<i32>(pos.xy);
    let normal = textureLoad(ao_normal, c, 0);
    if (normal.a < 0.5) { return vec4<f32>(1.0); }
    let z = bitcast<f32>(textureLoad(ao_depth, c, 0).x);
    let uv = pos.xy / vec2<f32>(textureDimensions(ao_depth));
    let h = u.inv_vp * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, z, 1.0);
    if (abs(h.w) < 1e-30) { return vec4<f32>(1.0); }
    var t: TOut;
    t.pos = vec4<f32>(pos.xy, z, 1.0);
    t.n = normal.xyz * 2.0 - 1.0;
    t.wp = h.xyz / h.w;
    return vec4<f32>(occlusion_term(t));
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

fn hover_color(i: TOut) -> vec4<f32> {
    // Derivatives first, in uniform control flow.
    let wood_dx = dpdx(i.wood);
    let wood_dy = dpdy(i.wood);
    if (clipped(i.wp)) { discard; }
    // An opaque body keeps its finish and its wood figure under the hover; a faded or
    // see-through one stays plain.
    var c = i.c.rgb;
    if (i.c.a > 0.99 && any(i.wood != vec4<f32>(0.0))) { c = wood_figure(c, i.wood, wood_dx, wood_dy); }
    return lit_color(i, 1.0, vec4<f32>(c + u.tint.rgb, i.c.a), i.c.a > 0.99);
}

/// A hovered body or face, lit over the model in a lifted colour.
@fragment
fn fs_hl_hover(i: TOut) -> @location(0) vec4<f32> {
    let c = hover_color(i);
    return c;
}

@fragment
fn fs_hl_hover_flat(i: FOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = flat_fragment(i, front);
    let c = hover_color(t);
    return c;
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

/// A repeatable value in 0..1 for a lattice point (an integer hash; the same on every GPU).
fn wood_hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    var h = (bitcast<u32>(x) * 0x8da6b343u) ^ (bitcast<u32>(y) * 0xd8163841u) ^ (bitcast<u32>(z) * 0xcb1ab31fu) ^ seed;
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

/// Smooth value noise in 0..1 over the unit lattice.
fn wood_noise(p: vec3<f32>, seed: u32) -> f32 {
    let i = floor(p);
    let f = p - i;
    let t = f * f * (vec3<f32>(3.0) - 2.0 * f);
    let x = i32(i.x);
    let y = i32(i.y);
    let z = i32(i.z);
    let x00 = mix(wood_hash(x, y, z, seed), wood_hash(x + 1, y, z, seed), t.x);
    let x10 = mix(wood_hash(x, y + 1, z, seed), wood_hash(x + 1, y + 1, z, seed), t.x);
    let x01 = mix(wood_hash(x, y, z + 1, seed), wood_hash(x + 1, y, z + 1, seed), t.x);
    let x11 = mix(wood_hash(x, y + 1, z + 1, seed), wood_hash(x + 1, y + 1, z + 1, seed), t.x);
    return mix(mix(x00, x10, t.y), mix(x01, x11, t.y), t.z);
}

/// A grain point (along, across, across) scaled for a noise lookup: fibres are long, so every
/// feature is stretched along the grain.
fn wood_scaled(g: vec3<f32>, along: f32, across: f32) -> vec3<f32> {
    return vec3<f32>(g.x * along, g.y * across, g.z * across);
}

/// `smoothstep` with the edges in either order kept apart (a plain cubic ramp from `e0` to `e1`).
fn wood_smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

/// How one kind of wood grows and looks. Its colours are multiples of the face's own colour, so
/// the figure follows the appearance's colour: the dark late wood of each ring, the dark streaks
/// along the grain and the pale zones. Then the millimetres between rings, how strongly the late
/// wood shows, how far the growth axis wanders (mm), the pores and the streaks (0 to 1), and how
/// it knots (toward 1: smaller knots and more pin knots).
struct WoodCharacter {
    late: vec3<f32>,
    streak: vec3<f32>,
    light: vec3<f32>,
    ring_mm: f32,
    contrast: f32,
    figure_mm: f32,
    pores: f32,
    streaks: f32,
    pins: f32,
}

/// The character of wood kind `kind` (`Material::wood_kind`): 1 to 8 are the starter woods in the
/// starter pack's order, anything else a general hardwood. Original values, chosen by eye; not
/// measured from any timber.
fn wood_character(kind: u32) -> WoodCharacter {
    switch kind {
        // oak-honey
        case 1u: { return WoodCharacter(vec3<f32>(0.735, 0.649, 0.568), vec3<f32>(0.633, 0.568, 0.523), vec3<f32>(1.133, 1.216, 1.341), 3.0, 0.7, 2.5, 0.8, 0.3, 0.5); }
        // oak-smoked
        case 2u: { return WoodCharacter(vec3<f32>(0.566, 0.524, 0.500), vec3<f32>(0.434, 0.405, 0.406), vec3<f32>(1.245, 1.262, 1.281), 3.0, 0.7, 2.5, 0.8, 0.3, 0.5); }
        // walnut-dark
        case 3u: { return WoodCharacter(vec3<f32>(0.625, 0.614, 0.659), vec3<f32>(0.500, 0.514, 0.614), vec3<f32>(1.268, 1.314, 1.273), 4.0, 0.55, 4.5, 0.6, 0.45, 0.15); }
        // maple-light
        case 4u: { return WoodCharacter(vec3<f32>(0.886, 0.830, 0.747), vec3<f32>(0.816, 0.740, 0.667), vec3<f32>(1.061, 1.100, 1.173), 3.5, 0.3, 2.0, 0.1, 0.2, 0.6); }
        // cherry-warm
        case 5u: { return WoodCharacter(vec3<f32>(0.773, 0.712, 0.667), vec3<f32>(0.625, 0.558, 0.545), vec3<f32>(1.148, 1.288, 1.333), 3.0, 0.4, 2.5, 0.2, 0.35, 0.7); }
        // ash-pale
        case 6u: { return WoodCharacter(vec3<f32>(0.734, 0.639, 0.526), vec3<f32>(0.670, 0.588, 0.500), vec3<f32>(1.083, 1.113, 1.171), 3.5, 0.75, 2.5, 0.75, 0.2, 0.2); }
        // birch-cream
        case 7u: { return WoodCharacter(vec3<f32>(0.904, 0.856, 0.793), vec3<f32>(0.835, 0.779, 0.707), vec3<f32>(1.061, 1.096, 1.159), 3.0, 0.3, 2.0, 0.1, 0.2, 0.7); }
        // teak-golden
        case 8u: { return WoodCharacter(vec3<f32>(0.713, 0.677, 0.625), vec3<f32>(0.506, 0.484, 0.500), vec3<f32>(1.172, 1.274, 1.406), 4.5, 0.5, 3.0, 0.45, 0.5, 0.1); }
        default: { return WoodCharacter(vec3<f32>(0.74, 0.66, 0.60), vec3<f32>(0.62, 0.56, 0.54), vec3<f32>(1.12, 1.17, 1.25), 3.5, 0.6, 3.0, 0.5, 0.3, 0.3); }
    }
}

/// How many knots a board carries, 0 to 1: the share of 300 mm lengths of the log that grew a
/// branch. A face shows a knot only where it cuts that branch.
const WOOD_KNOTS: f32 = 0.5;

/// The wood figure of a face in colour `base` at grain coordinates `w` (`wood_coordinates`), whose
/// change across one pixel is `dx` and `dy`: growth rings of uneven width around a wandering
/// growth axis, each with pale early wood and a dark late-wood band, pores, fibres, long dark
/// streaks and knots, in the character of the wood's kind (`wood_character`). A knot is a branch
/// that grew out from the growth axis: the branch and the trunk are one growth field, so the
/// trunk's rings turn into the branch's rings and bend around it. An original procedural field,
/// not measured from any timber. Every feature fades to its average as it shrinks below a pixel,
/// so a distant or steeply seen board stays calm.
fn wood_figure(base: vec3<f32>, w: vec4<f32>, dx: vec4<f32>, dy: vec4<f32>) -> vec3<f32> {
    let kind = u32(max(w.z, 1.0) - 0.5);
    let ch = wood_character(kind);
    let s = kind * 0x9e3779b9u;
    let late_colour = base * ch.late;
    let light_colour = min(base * ch.light, vec3<f32>(1.0));
    let streak_colour = base * ch.streak;
    var g = vec3<f32>(w.w, w.x, w.y);
    let fx = vec3<f32>(dx.w, dx.x, dx.y);
    let fy = vec3<f32>(dy.w, dy.x, dy.y);
    // A pixel's size in millimetres across and along the grain.
    let across = max(length(fx.yz), length(fy.yz));
    let along = max(abs(fx.x), abs(fy.x));
    let ring_mm = ch.ring_mm * (0.7 + 0.7 * wood_hash(0, 0, 9, s));
    // Knots. `lift` pushes the trunk's rings outward around a branch; `bw`, `bt`, `bd` and
    // `bthick` describe the branch's own growth where the point is near one.
    var lift = 0.0;
    var knot = 0.0;
    var kd = 0.0;
    var kring = 0.0;
    var halo = 0.0;
    var bdark = 0.0;
    var bw = 0.0;
    var bt = 0.0;
    var bthick = 1.0;
    var bd = 0.0;
    {
        // One branch at most in each 300 mm of the log, in a direction of its own.
        let cell = floor(g.x / 300.0);
        let ci = i32(cell);
        if (wood_hash(ci, 1, 11, s ^ 0x7f4a7c15u) < WOOD_KNOTS) {
            let h1 = wood_hash(ci, 2, 11, s ^ 0x7f4a7c15u);
            let h2 = wood_hash(ci, 3, 11, s ^ 0x7f4a7c15u);
            let h3 = wood_hash(ci, 4, 11, s ^ 0x7f4a7c15u);
            let h4 = wood_hash(ci, 5, 11, s ^ 0x7f4a7c15u);
            let h5 = wood_hash(ci, 6, 11, s ^ 0x7f4a7c15u);
            let ay = 2.0 * h2 - 1.0;
            var az = sqrt(max(1.0 - ay * ay, 0.0));
            if (h3 < 0.5) { az = -az; }
            let outward = g.y * ay + g.z * az;
            let side = g.z * ay - g.y * az;
            let lean = 0.25 + 0.4 * h5;
            let reach = max(outward, 0.0);
            let u = g.x - (cell + 0.2 + 0.25 * h1) * 300.0 - lean * 160.0 * reach / (reach + 160.0);
            bw = wood_smooth(0.0, 8.0, outward) * (1.0 - wood_smooth(35.0, 60.0, abs(u)));
            let d0 = sqrt(u * u / (1.0 + lean * lean) + side * side);
            let wob = wood_noise(vec3<f32>(1.6 * u / (d0 + 1.0e-3) + 40.0 * h1, 1.6 * side / (d0 + 1.0e-3) + 40.0 * h2, outward * 0.03), s ^ 0x4cf5ad43u) - 0.5;
            let d = sqrt(u * u * 0.2 / (1.0 + lean * lean) + side * side) * (1.0 + 0.3 * wob);
            bthick = (0.08 + 0.14 * h4) * (1.0 - 0.45 * ch.pins);
            let slow = d * d * d * d / 10000.0;
            bt = reach / 6.0 + d * (1.0 + slow) / bthick;
            bd = d;
            let rk = min(0.8 * bthick * reach, 14.0) + 2.0;
            var tail = 9.0;
            if (u < 0.0) { tail = 26.0; }
            let lens = sqrt(side * side / (rk * rk) + 0.1) + u * u / (tail * rk * rk);
            let fade = max(1.0 - u * u / 3600.0, 0.0);
            let close = wood_smooth(0.0, 8.0, outward) * fade * fade;
            lift = close * 1.2 * rk / (1.0 + 0.5 * lens * lens);
            let wide = 1.5 * rk;
            let part = -close * side * wide * wide / (u * u * 0.16 + side * side + wide * wide);
            g = vec3<f32>(g.x, g.y - part * az, g.z + part * ay);
            bdark = 0.15 + 0.6 * h3 * h3;
        }
    }
    if (ch.pins > 0.0) {
        // Pin knots: small dark dots, at most one in each 80 mm.
        let cell = floor(g.x / 80.0);
        let ci = i32(cell);
        if (wood_hash(ci, 1, 13, s ^ 0x7f4a7c15u) < WOOD_KNOTS * ch.pins) {
            let h1 = wood_hash(ci, 2, 13, s ^ 0x7f4a7c15u);
            let h2 = wood_hash(ci, 3, 13, s ^ 0x7f4a7c15u);
            let h3 = wood_hash(ci, 4, 13, s ^ 0x7f4a7c15u);
            let h4 = wood_hash(ci, 5, 13, s ^ 0x7f4a7c15u);
            let ay = 2.0 * h2 - 1.0;
            var az = sqrt(max(1.0 - ay * ay, 0.0));
            if (h3 < 0.5) { az = -az; }
            let outward = g.y * ay + g.z * az;
            let side = g.z * ay - g.y * az;
            let u = g.x - (cell + 0.35 + 0.3 * h1) * 80.0;
            let rk = 1.0 + 2.2 * h4;
            let start = 150.0 * wood_hash(ci, 6, 13, s ^ 0x7f4a7c15u);
            let there = wood_smooth(start, start + 10.0, outward) * (1.0 - wood_smooth(12.0, 24.0, abs(u)));
            let pd = sqrt(u * u + side * side) / rk;
            let lens = sqrt(side * side / (rk * rk) + 0.1) + u * u / (12.0 * rk * rk);
            lift = lift + there * 1.5 * rk / (1.0 + 0.8 * lens * lens);
            halo = there / (1.0 + 0.3 * pd * pd * pd * pd);
            let pin = there * 0.55 * (1.0 - wood_smooth(0.5, 1.1, pd));
            if (pin > knot) {
                knot = pin;
                kd = pd;
                kring = pd * 2.0;
            }
        }
    }
    // The growth axis wanders: a slow drift and a slower, larger one.
    let n1 = wood_scaled(g, 0.004, 0.02);
    let d1 = vec2<f32>(wood_noise(n1, s), wood_noise(n1 + vec3<f32>(17.0, 31.0, 47.0), s ^ 0x9e3779b9u)) - vec2<f32>(0.5);
    let n2 = wood_scaled(g, 0.0012, 0.006);
    let d2 = vec2<f32>(wood_noise(n2 + vec3<f32>(5.0, 11.0, 23.0), s ^ 0x510e527fu), wood_noise(n2 + vec3<f32>(41.0, 3.0, 29.0), s ^ 0x9b05688cu)) - vec2<f32>(0.5);
    let yz = g.yz + ch.figure_mm * (2.0 * d1 + 9.0 * d2);
    let r = length(yz);
    let q = vec3<f32>(g.x, yz);
    // Rings are not round: lobes around the axis, a slight taper along it.
    let around = yz / (r + 1e-3);
    let lobe = wood_noise(vec3<f32>(g.x * 0.0015 + 3.0, 1.3 * around + vec2<f32>(7.0, 13.0)), s ^ 0x243f6a88u) - 0.5;
    let lobe2 = wood_noise(vec3<f32>(g.x * 0.004 + 11.0, 3.1 * around + vec2<f32>(5.0, 29.0)), s ^ 0xb7e15162u) - 0.5;
    let trunk = r * (1.0 + 0.22 * lobe + 0.07 * lobe2 * wood_smooth(5.0, 60.0, r)) + 0.006 * g.x - lift;
    // Where a branch grew, the later of the two growths shows, joined smoothly: one field.
    var rr = trunk;
    var branch = 0.0;
    if (bw > 0.0) {
        let width = 0.8 * max(trunk, 0.0) + 2.0;
        let late_0 = bt + 8.0 * width * (1.0 - bw) * (1.0 - bw) + 2.0 * (max(trunk, 0.0) + width) * wood_smooth(22.0, 34.0, bd);
        let late_b = min(late_0, trunk + width);
        let h = clamp(0.5 + 0.5 * (late_b - trunk) / width, 0.0, 1.0);
        rr = mix(late_b, trunk, h) - width * h * (1.0 - h);
        branch = 1.0 - h;
    }
    knot = max(knot, bdark * branch);
    // Rings per pixel: past one the rings blur to their average, well below it they are drawn sharp.
    let fr0 = (abs(dot(yz, fx.yz)) + abs(dot(yz, fy.yz))) / (max(r, 1e-3) * ring_mm);
    let fr = max(fr0, branch * max(across, along) / (bthick * ring_mm));
    let rings = (1.0 - wood_smooth(0.35, 1.0, fr)) * (1.0 - 0.55 * branch);
    let thin = (1.0 - wood_smooth(0.08, 0.35, fr)) * (1.0 - 0.6 * branch);
    let ring0 = rr / ring_mm;
    // Good and lean decades and years, and a fine waviness of each ring.
    let decade = wood_noise(vec3<f32>(ring0 * 0.11 + 3.0, g.x * 0.0008, 2.0 * around.x + 2.0 * around.y), s ^ 0x13c6ef37u) - 0.5;
    let year = wood_noise(vec3<f32>(ring0 * 0.37 + 9.0, g.x * 0.0015, 3.0 * around.x - 3.0 * around.y), s ^ 0x5be0cd19u) - 0.5;
    let ring = ring0 + 3.6 * decade + 0.7 * year
        + 0.6 * (wood_noise(wood_scaled(q, 0.01, 0.04), s ^ 0x85ebca6bu) - 0.5)
        + 0.25 * (wood_noise(wood_scaled(q, 0.03, 0.12), s ^ 0x1b873593u) - 0.5);
    // Each ring's own width: its two edges are moved by a hash of their numbers.
    let n0 = floor(ring);
    let ni = i32(n0);
    let e0 = n0 + 0.6 * (wood_hash(ni, 0, 7, s ^ 0x6c62272eu) - 0.5);
    let e1 = n0 + 1.0 + 0.6 * (wood_hash(ni + 1, 0, 7, s ^ 0x6c62272eu) - 0.5);
    var lo = e0;
    var hi = e1;
    var nr = ni;
    if (ring < e0) {
        lo = n0 - 1.0 + 0.6 * (wood_hash(ni - 1, 0, 7, s ^ 0x6c62272eu) - 0.5);
        hi = e0;
        nr = ni - 1;
    } else if (ring >= e1) {
        lo = e1;
        hi = n0 + 2.0 + 0.6 * (wood_hash(ni + 2, 0, 7, s ^ 0x6c62272eu) - 0.5);
        nr = ni + 1;
    }
    let t = clamp((ring - lo) / (hi - lo), 0.0, 1.0);
    // Where in the ring its late wood starts, and how dark that ring's is.
    let share = 0.3 + 0.5 * wood_hash(nr, 1, 7, s ^ 0x6c62272eu);
    let depth = 0.5 + 0.7 * wood_hash(nr, 2, 7, s ^ 0x6c62272eu);
    let wrap = 1.0 - wood_smooth(1.0 - clamp(fr, 0.02, 0.5), 1.0, t);
    let late = mix(0.25, min(wood_smooth(share, 0.95, t) * wrap * depth, 1.0), rings) * ch.contrast;
    let line = wood_smooth(0.88, 0.97, t) * wrap * thin * ch.contrast * min(depth, 1.0);
    // Paler zones and long dark streaks, both following the rings.
    let turn = 20.0 * yz.x / (r + 1.0);
    let zone = wood_smooth(0.35, 0.8, wood_noise(vec3<f32>(g.x * 0.0015, r * 0.05, turn * 0.05), s ^ 0x165667b1u));
    let streak = wood_smooth(0.6, 0.92, wood_noise(vec3<f32>(g.x * 0.004, r * 0.25, turn * 0.03), s ^ 0xc2b2ae35u)) * ch.streaks;
    // Fine detail, each kind shown only while a pixel is small enough to hold it.
    let fine = 1.0 - wood_smooth(0.08, 0.25, across);
    let coarse = 1.0 - wood_smooth(0.25, 0.8, across);
    let mid = 1.0 - wood_smooth(0.8, 2.5, across);
    let pore = wood_smooth(1.0 - 0.3 * ch.pores, 1.0, wood_noise(wood_scaled(q, 0.1, 1.8), s ^ 0x27d4eb2fu)) * ch.pores * (1.0 - 0.5 * late) * coarse * (1.0 - wood_smooth(3.0, 10.0, along));
    let fleck = wood_smooth(0.8, 0.97, wood_noise(wood_scaled(q, 0.3, 1.6), s ^ 0x6a09e667u)) * (0.25 + 0.5 * ch.pores) * coarse * (1.0 - wood_smooth(1.0, 3.3, along));
    let fibre = mix(1.0, 0.92 + 0.16 * wood_noise(wood_scaled(q, 0.03, 1.2), s ^ 0x3c6ef372u), coarse)
        * mix(1.0, 0.94 + 0.12 * wood_noise(wood_scaled(q, 0.05, 4.0), s ^ 0xbb67ae85u), fine)
        * mix(1.0, 0.95 + 0.1 * wood_noise(wood_scaled(q, 0.012, 0.45), s ^ 0xa54ff53au), mid);
    let ripple = mix(1.0, 0.95 + 0.1 * wood_noise(wood_scaled(g, 0.06, 0.012), s ^ 0x1f83d9abu), 1.0 - wood_smooth(5.0, 16.0, along));
    let ringed = mix(mix(base, light_colour, zone), late_colour, late);
    let streaked = mix(ringed, streak_colour, streak);
    // A knot is tighter, slightly darker wood: the colour of a ring's late wood, deepened.
    let knot_ring = max(wood_smooth(0.5, 1.0, abs(kring - floor(kring) - 0.5) * 2.0), wood_smooth(0.7, 1.0, kd));
    let flecked = mix(mix(streaked, light_colour, 0.35 * fleck), late_colour, 0.18 * halo);
    let heart = late_colour * mix(streak_colour, vec3<f32>(1.0), 0.45) * (0.95 - 0.55 * late_colour.y);
    let knotted = mix(flecked, heart, knot * (0.7 + 0.3 * knot_ring));
    return clamp(knotted * (1.0 - 0.7 * pore) * (1.0 - 0.2 * line) * fibre * ripple, vec3<f32>(0.0), vec3<f32>(1.0));
}

/// An opaque face, with its material finish where the viewport shows them, and the wood figure
/// (`wood_figure`) where its entry has a wood frame (`wood_coordinates`). `wood_dx` and `wood_dy`:
/// the screen derivatives of `i.wood`, taken by the caller before its first branch.
fn lit(i: TOut, visibility: f32, wood_dx: vec4<f32>, wood_dy: vec4<f32>) -> vec4<f32> {
    if (all(i.wood == vec4<f32>(0.0))) { return lit_color(i, visibility, i.c, true); }
    return lit_color(i, visibility, vec4<f32>(wood_figure(i.c.rgb, i.wood, wood_dx, wood_dy), i.c.a), true);
}

/// sRGB to linear light, exact, with the straight segment near black.
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
    return line_vertex(vi, local_a, local_b, c, wbits, motion_at(c));
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
    // Moved with the body whose line it is: the entry it was built with names the motion.
    return line_vertex(vi % 6u, local_a, local_b, style >> 8u, w, motion_at(c));
}

/// A line from `local_a` to `local_b` in palette entry `c`, moved by motion `m` (0: none).
fn line_vertex(vi: u32, local_a: vec3<f32>, local_b: vec3<f32>, c: u32, wbits: u32, m: u32) -> LOut {
    let a = placed_by(m, local_a);
    let b = placed_by(m, local_b);
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
        let g = grazing(p, turned_by(m, octahedral(wbits & 4095u)), turned_by(m, octahedral((wbits >> 12u) & 4095u)));
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

/// The model's edges' depth alone (`line_depth`): where an edge covers the whole pixel, cut by a
/// section as it is. A line thinner than a pixel (faded by the width it lacks) or a see-through one
/// leaves the rest of the pixel to the faces behind it, so it writes none.
@fragment
fn fs_line_depth(i: LOut) -> @location(0) vec4<f32> {
    if (clipped(i.wp) || i.c.a < 0.999) { discard; }
    return vec4<f32>(0.0);
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
