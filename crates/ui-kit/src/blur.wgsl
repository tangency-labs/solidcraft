// Original SolidCraft backdrop dual-Kawase and rounded surface composition.
struct Params { a: vec4<f32>, rect: vec4<f32>, corners: vec4<f32>, top: vec4<f32>, bottom: vec4<f32>, outside: vec4<f32> }
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> p: Params;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) i: u32) -> Vertex {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Vertex(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
fn linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}
fn gamma(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), c > vec3<f32>(0.0031308));
}
fn load_linear(at: vec2<i32>) -> vec4<f32> {
    let c = textureLoad(image, clamp(at, vec2<i32>(0), vec2<i32>(textureDimensions(image)) - vec2<i32>(1)), 0);
    // Only the scene/composition input is gamma encoded on unorm targets. Decoding
    // each texel BEFORE bilinear interpolation avoids filtering encoded colors.
    return vec4<f32>(linear(c.rgb), c.a);
}
fn sample_at(uv: vec2<f32>) -> vec4<f32> {
    if p.a.y < 0.5 { return textureSampleLevel(image, filtering, uv, 0.0); }
    let q = uv * vec2<f32>(textureDimensions(image)) - vec2<f32>(0.5);
    let lo = vec2<i32>(floor(q)); let f = fract(q);
    return mix(mix(load_linear(lo), load_linear(lo + vec2<i32>(1, 0)), f.x),
               mix(load_linear(lo + vec2<i32>(0, 1)), load_linear(lo + vec2<i32>(1, 1)), f.x), f.y);
}
@fragment fn down(v: Vertex) -> @location(0) vec4<f32> {
    let d = vec2<f32>(p.a.x) / vec2<f32>(textureDimensions(image));
    return (4.0 * sample_at(v.uv) + sample_at(v.uv + d) + sample_at(v.uv - d)
        + sample_at(v.uv + d * vec2<f32>(1.0, -1.0)) + sample_at(v.uv + d * vec2<f32>(-1.0, 1.0))) / 8.0;
}
@fragment fn up(v: Vertex) -> @location(0) vec4<f32> {
    let d = vec2<f32>(p.a.x) / vec2<f32>(textureDimensions(image));
    return (sample_at(v.uv + vec2<f32>(-2.0*d.x, 0.0)) + sample_at(v.uv + vec2<f32>(2.0*d.x, 0.0))
        + sample_at(v.uv + vec2<f32>(0.0, -2.0*d.y)) + sample_at(v.uv + vec2<f32>(0.0, 2.0*d.y))
        + 2.0 * (sample_at(v.uv + d) + sample_at(v.uv - d)
        + sample_at(v.uv + d * vec2<f32>(1.0, -1.0)) + sample_at(v.uv + d * vec2<f32>(-1.0, 1.0)))) / 12.0;
}
@fragment fn background(v: Vertex) -> @location(0) vec4<f32> {
    let q = v.position.xy;
    var c = p.outside;
    if all(q >= p.rect.xy) && all(q < p.rect.zw) && all(q >= p.corners.xy) && all(q < p.corners.zw) {
        c = mix(p.top, p.bottom, clamp((q.y - p.rect.y) / max(1.0, p.rect.w - p.rect.y), 0.0, 1.0));
    }
    if p.a.z > 0.5 { return vec4<f32>(linear(c.rgb), 1.0); }
    return vec4<f32>(c.rgb, 1.0);
}
@fragment fn panel(v: Vertex) -> @location(0) vec4<f32> {
    let center = (p.rect.xy + p.rect.zw) * 0.5;
    let half_size = (p.rect.zw - p.rect.xy) * 0.5;
    let relative = v.position.xy - center;
    let upper = relative.y < 0.0;
    let radius = select(select(p.corners.w, p.corners.z, relative.x < 0.0), select(p.corners.y, p.corners.x, relative.x < 0.0), upper);
    let r = min(radius, min(half_size.x, half_size.y));
    let d = abs(relative) - half_size + vec2<f32>(r);
    let distance = min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0))) - r;
    let alpha = clamp(0.5 - distance, 0.0, 1.0) * p.a.w;
    let c = textureSample(image, filtering, v.position.xy / vec2<f32>(textureDimensions(image)));
    // Match egui: coverage/opacity multiply gamma-premultiplied RGB BEFORE the
    // framebuffer conversion. The sampled backdrop remains linear color.
    let premultiplied_gamma = gamma(c.rgb) * alpha;
    let rgb = select(premultiplied_gamma, linear(premultiplied_gamma), p.a.z > 0.5);
    return vec4<f32>(rgb, alpha);
}
@fragment fn present(v: Vertex) -> @location(0) vec4<f32> {
    return textureSample(image, filtering, v.uv);
}
