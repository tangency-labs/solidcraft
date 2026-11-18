// Liquid Glass fragment shader. One instance = one rounded rect drawn as a
// full-viewport triangle; egui-wgpu sets the viewport to the callback rect.
// All coordinates are physical pixels (framebuffer space).

struct Uniforms {
    rect_min: vec2<f32>,
    rect_max: vec2<f32>,
    bd_min: vec2<f32>,     // where the full backdrop image is mapped on screen
    bd_max: vec2<f32>,
    tint: vec4<f32>,       // straight alpha
    radius: f32,
    blur: f32,             // blur radius in px
    refraction: f32,       // max edge displacement in px
    edge_width: f32,       // width of the lens zone in px
    chroma: f32,
    brightness: f32,
    saturation: f32,
    specular: f32,
    border: f32,
    shadow: f32,
    shadow_radius: f32,
    srgb_out: f32,
    light_dir: vec2<f32>,
    max_lod: f32,
    backdrop_scale: f32,
    corner_a: vec4<f32>,   // p, a, b, c   (smoothed corner, px, see renderer::corner_params)
    corner_b: vec4<f32>,   // d, r, theta3
    fill: vec4<f32>,       // colour shown where a sample falls outside the backdrop rect
    shadow_geo: vec4<f32>, // offset (down), spread, reach
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var bd_tex: texture_2d<f32>;
@group(0) @binding(2) var bd_samp: sampler;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // Full-viewport triangle.
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

const CORNER_SEGMENTS: i32 = 18;

fn bezier(p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>, p3: vec2<f32>, t: f32) -> vec2<f32> {
    let u = 1.0 - t;
    return u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3;
}

// Point `i` (0..=18) of the smoothed corner polyline in the corner's local
// frame: x along the edge away from the corner, y inward from the edge.
fn corner_point(i: i32) -> vec2<f32> {
    let p = u.corner_a.x;
    let a = u.corner_a.y;
    let b = u.corner_a.z;
    let c = u.corner_a.w;
    let d = u.corner_b.x;
    let r = u.corner_b.y;
    let theta3 = u.corner_b.z;
    let p3 = vec2<f32>(p - a - b - c, d);
    if (i <= 6) {
        return bezier(vec2<f32>(p, 0.0), vec2<f32>(p - a, 0.0), vec2<f32>(p - a - b, 0.0), p3, f32(i) / 6.0);
    } else if (i <= 12) {
        // Arc around (r, r) from theta3 to its mirror across the diagonal, clockwise.
        let theta = theta3 - (4.71238898 + 2.0 * theta3) * f32(i - 6) / 6.0;
        return vec2<f32>(r, r) + r * vec2<f32>(cos(theta), sin(theta));
    }
    return bezier(p3.yx, vec2<f32>(0.0, p - a - b), vec2<f32>(0.0, p - a), vec2<f32>(0.0, p), f32(i - 12) / 6.0);
}

fn seg_dist(q: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = q - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

// Signed distance to a box with smoothed (continuous) corners, negative inside.
// Folded into one quadrant; the boundary there is: top edge, corner curve,
// right edge, walked clockwise so the inside is on the right of each segment.
// Segments of (almost) zero length are skipped: their cross product is pure
// float noise, and a neighbouring segment's endpoint yields the same distance.
struct Nearest { dist: f32, cross: f32 }

fn consider(q: vec2<f32>, a: vec2<f32>, b: vec2<f32>, n: Nearest) -> Nearest {
    let ba = b - a;
    if (dot(ba, ba) < 1e-4) {
        return n;
    }
    let d = seg_dist(q, a, b);
    if (d < n.dist) {
        return Nearest(d, ba.x * (q - a).y - ba.y * (q - a).x);
    }
    return n;
}

fn sd_smooth_box(pos: vec2<f32>, half: vec2<f32>) -> f32 {
    let q = abs(pos);
    let p = u.corner_a.x;
    var n = Nearest(1e9, 0.0);
    var a = vec2<f32>(0.0, half.y);
    var b = vec2<f32>(half.x - p, half.y);
    n = consider(q, a, b, n);
    a = b;
    for (var i = 1; i <= CORNER_SEGMENTS; i++) {
        let l = corner_point(i);
        b = vec2<f32>(half.x - l.x, half.y - l.y);
        n = consider(q, a, b, n);
        a = b;
    }
    n = consider(q, a, vec2<f32>(half.x, 0.0), n);
    return select(n.dist, -n.dist, n.cross < 0.0);
}

// Smooth outward normal of a rounded box whose corner radius matches the smoothed
// corner span. Used for the lens direction and lighting: the polyline SDF's finite
// difference normal is piecewise constant and would show as spokes in the refraction.
fn rounded_normal(pos: vec2<f32>, half: vec2<f32>, r: f32) -> vec2<f32> {
    let q = abs(pos) - (half - vec2<f32>(r));
    var n: vec2<f32>;
    if (q.x > 0.0 && q.y > 0.0) {
        n = normalize(q + vec2<f32>(1e-5, 0.0));
    } else if (q.x > q.y) {
        n = vec2<f32>(1.0, 0.0);
    } else {
        n = vec2<f32>(0.0, 1.0);
    }
    let sgn = select(vec2<f32>(-1.0), vec2<f32>(1.0), pos >= vec2<f32>(0.0));
    return n * sgn;
}

// One backdrop tap at a screen position (px). Beyond the image it fades to the
// fill colour over the blur radius, so a blurred image edge stays smooth. A
// transparent fill means "nothing is outside" (the backdrop is the whole
// screen): clamp to the edge instead of fading.
fn tap(p: vec2<f32>, lod: f32) -> vec3<f32> {
    let uv = (p - u.bd_min) / (u.bd_max - u.bd_min);
    let edge = min(min(p.x - u.bd_min.x, u.bd_max.x - p.x), min(p.y - u.bd_min.y, u.bd_max.y - p.y));
    let r = max(u.blur * 0.5, 0.5);
    let inside = select(smoothstep(-r, r, edge), 1.0, u.fill.a < 0.001);
    return mix(u.fill.rgb, textureSampleLevel(bd_tex, bd_samp, uv, lod).rgb, inside);
}

fn sample_backdrop(p: vec2<f32>, lod: f32, blur: f32) -> vec3<f32> {
    // Centre tap plus an 8-tap ring at half the blur radius on top of the
    // (already smooth) pyramid level: reads as a Gaussian without banding.
    let s = max(blur, 0.0) * 0.5;
    var sum = tap(p, lod) * 2.0;
    for (var i = 0; i < 8; i++) {
        let a = f32(i) * 0.7853982 + 0.3927;
        sum += tap(p + s * vec2<f32>(cos(a), sin(a)), lod);
    }
    return sum / 10.0;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = frag.xy;
    let center = (u.rect_min + u.rect_max) * 0.5;
    let half = (u.rect_max - u.rect_min) * 0.5;
    let d = sd_smooth_box(p - center, half);
    let mask = 1.0 - smoothstep(-0.5, 0.5, d);

    // Shadow (outside the shape only): the shape pushed down and grown by the spread.
    let sh_off = vec2<f32>(0.0, u.shadow_geo.x);
    let ds = sd_smooth_box(p - center - sh_off, half) - u.shadow_geo.y;
    let sh_t = 1.0 - smoothstep(-u.shadow_radius * 0.25, u.shadow_radius, ds);
    let shadow = u.shadow * sh_t * sh_t * (1.0 - mask); // quadratic tail: soft, mostly near the edge

    if (mask <= 0.0 && shadow <= 0.0) {
        discard;
    }

    // Outward normal: analytic and smooth (see rounded_normal).
    let n = rounded_normal(p - center, half, min(u.corner_a.x, min(half.x, half.y)));

    // Lens profile: 0 deep inside, 1 at the edge. SolidCraft: a smooth ramp (no slope at
    // either end) instead of the circular falloff, whose bend steepened to a sharp line at the
    // rim.
    let t = clamp((d + u.edge_width) / max(u.edge_width, 1.0), 0.0, 1.0);
    let lens = t * t * (3.0 - 2.0 * t);
    // SolidCraft: the reach. At 1 the rim shows the backdrop up to `refraction` past the edge
    // (the part shows in the glass before it reaches the panel); at 0 the shift shrinks to the
    // edge, so the samples stay under the glass and the rim only magnifies what is there.
    let inset = max(-d, 0.0);
    let magnify = min(u.refraction / max(u.edge_width, 1.0), 0.75) * inset;
    let disp = n * lens * mix(magnify, u.refraction, u.shadow_geo.z);

    // SolidCraft: frosted more towards the rim than in the middle, over three lens widths.
    let band = max(u.edge_width * 3.0, 1.0);
    let rim_frost = smoothstep(0.0, 1.0, clamp((d + band) / band, 0.0, 1.0));
    let blur = u.blur * mix(0.65, 1.35, rim_frost);
    let lod = clamp(log2(max(blur * u.backdrop_scale * 0.5, 1.0)), 0.0, u.max_lod);
    var col: vec3<f32>;
    if (u.chroma > 0.001) {
        let k = u.chroma * 0.5;
        col = vec3<f32>(
            sample_backdrop(p + disp * (1.0 + k), lod, blur).r,
            sample_backdrop(p + disp, lod, blur).g,
            sample_backdrop(p + disp * (1.0 - k), lod, blur).b,
        );
    } else {
        col = sample_backdrop(p + disp, lod, blur);
    }

    // Vibrancy: saturation + brightness, then tint.
    let lum = dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
    col = mix(vec3<f32>(lum), col, u.saturation) * u.brightness;
    col = mix(col, u.tint.rgb, u.tint.a);

    // Specular rim: a thin bright crescent towards the light and a second, softer
    // one opposite (light bouncing inside the slab), as on a polished glass edge.
    let ndl = dot(n, u.light_dir);
    // Highlights gather where the edge curves (corners, capsule ends); straight edges
    // only get a faint line, as on a polished slab lit by a broad source.
    let curved = 4.0 * n.x * n.x * n.y * n.y;
    // Thin: the plate is flat, only its bevelled edge catches light. No top-down sheen,
    // that made the surface read as a dome.
    let rim = pow(t, 10.0) * (0.3 + 0.7 * curved) * (1.0 * max(ndl, 0.0) + 0.6 * max(-ndl, 0.0));
    col += vec3<f32>(1.0) * u.specular * rim;
    // A faint dark contour at the very edge reads as the plate's thickness and is what
    // separates glass from a white page (iOS shows no drop shadow there); a bit
    // stronger on the unlit side.
    col *= 1.0 - 0.6 * u.border * pow(t, 8.0) * (0.6 + 0.4 * (1.0 - max(ndl, 0.0)));

    // Hairline inner border, a little brighter on the lit side.
    let ring = 1.0 - smoothstep(0.0, 1.2, abs(d + 0.7));
    let ring_a = u.border * ring * (0.32 + 0.4 * max(ndl, 0.0) + 0.18 * max(-ndl, 0.0));
    col = mix(col, vec3<f32>(1.0), ring_a);

    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    if (u.srgb_out > 0.5) {
        col = srgb_to_linear(col);
    }

    // Premultiplied output: glass over shadow.
    let alpha = mask + shadow;
    return vec4<f32>(col * mask, alpha);
}
