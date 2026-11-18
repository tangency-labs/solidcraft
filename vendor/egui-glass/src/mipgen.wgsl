// Downsample one mip level into the next with the 13-tap filter from
// Jimenez (SIGGRAPH 2014): a smooth, alias-free pyramid, so sampling a
// fractional level reads like a Gaussian blur. The texture is sRGB, so the
// hardware filter averages in linear light.
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var out: VOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

fn tap(uv: vec2<f32>, o: vec2<f32>, t: vec2<f32>) -> vec3<f32> {
    return textureSample(src, samp, uv + o * t).rgb;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let uv = in.uv;
    let a = tap(uv, vec2<f32>(-2.0, -2.0), t);
    let b = tap(uv, vec2<f32>( 0.0, -2.0), t);
    let c = tap(uv, vec2<f32>( 2.0, -2.0), t);
    let d = tap(uv, vec2<f32>(-1.0, -1.0), t);
    let e = tap(uv, vec2<f32>( 1.0, -1.0), t);
    let f = tap(uv, vec2<f32>(-2.0,  0.0), t);
    let g = tap(uv, vec2<f32>( 0.0,  0.0), t);
    let h = tap(uv, vec2<f32>( 2.0,  0.0), t);
    let i = tap(uv, vec2<f32>(-1.0,  1.0), t);
    let j = tap(uv, vec2<f32>( 1.0,  1.0), t);
    let k = tap(uv, vec2<f32>(-2.0,  2.0), t);
    let l = tap(uv, vec2<f32>( 0.0,  2.0), t);
    let m = tap(uv, vec2<f32>( 2.0,  2.0), t);
    let rgb = g * 0.125 + (a + c + k + m) * 0.03125 + (b + f + h + l) * 0.0625 + (d + e + i + j) * 0.125;
    return vec4<f32>(rgb, 1.0);
}
