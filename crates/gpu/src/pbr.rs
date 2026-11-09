//! Original GPU translation of the L1 opaque surface and fixed studio reference.
//! Appended to the existing viewport shader so camera/output conventions are shared.

pub const VERTEX_SIZE: usize = 48;

pub const SOURCE: &str = r#"
struct PbrOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) base: vec3<f32>,
    @location(2) wp: vec3<f32>,
    @location(3) parameters: vec3<f32>,
};

fn pbr_unit(v: vec3<f32>) -> vec3<f32> {
    let scale = max(max(abs(v.x), abs(v.y)), abs(v.z));
    if (scale == 0.0) { return vec3<f32>(0.0, 0.0, 1.0); }
    let s = v / scale;
    return s / length(s);
}

fn pbr_encode(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), c > vec3<f32>(0.0031308));
}

@vertex
fn vs_pbr(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) base: vec3<f32>, @location(3) parameters: vec3<f32>) -> PbrOut {
    var o: PbrOut;
    o.pos = u.vp * vec4<f32>(p, 1.0);
    o.n = n;
    o.base = base;
    o.wp = p;
    o.parameters = parameters;
    return o;
}

fn pbr_fresnel(f0: vec3<f32>, cosine: f32, metallic: f32, ior: f32) -> vec3<f32> {
    let t = 1.0 - cosine;
    // Match the shared surface: an index-matched dielectric has no interface.
    let grazing = select(1.0, metallic, ior == 1.0);
    return f0 + (vec3<f32>(grazing) - f0) * (t * t * t * t * t);
}

fn pbr_brdf(base: vec3<f32>, parameters: vec3<f32>, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nv = clamp(dot(n, v), -1.0, 1.0);
    let nl = clamp(dot(n, l), -1.0, 1.0);
    if (nv <= 0.0 || nl <= 0.0) { return vec3<f32>(0.0); }
    let h = pbr_unit(v + l);
    let nh = clamp(dot(n, h), 0.0, 1.0);
    let vh = clamp(dot(v, h), 0.0, 1.0);
    let alpha = parameters.y * parameters.y;
    let a2 = alpha * alpha;
    let denominator = (1.0 - nh * nh) + nh * nh * a2;
    let distribution = a2 / (3.141592653589793 * denominator * denominator);
    let gv = 2.0 / (nv + sqrt(a2 + (1.0 - a2) * nv * nv));
    let gl = 2.0 / (nl + sqrt(a2 + (1.0 - a2) * nl * nl));
    let ratio = (parameters.z - 1.0) / (parameters.z + 1.0);
    let f0 = mix(vec3<f32>(ratio * ratio), base, parameters.x);
    let specular = pbr_fresnel(f0, vh, parameters.x, parameters.z) * (distribution * 0.25 * gv * gl);
    let diffuse = base * ((1.0 - parameters.x) / 3.141592653589793);
    return specular + (vec3<f32>(1.0) - pbr_fresnel(f0, nv, parameters.x, parameters.z)) * (vec3<f32>(1.0) - pbr_fresnel(f0, nl, parameters.x, parameters.z)) * diffuse;
}

@fragment
fn fs_pbr(i: PbrOut) -> @location(0) vec4<f32> {
    let n = pbr_unit(i.n);
    var view = u.back.xyz;
    if (u.eye.w > 0.5) { view = u.eye.xyz - i.wp; }
    let v = pbr_unit(view);
    let lights = array<vec4<f32>, 3>(vec4<f32>(0.6, 0.0, 0.8, 3.0), vec4<f32>(-0.8, 0.0, 0.6, 1.0), vec4<f32>(0.0, -0.6, 0.8, 1.5));
    var radiance = vec3<f32>(0.0);
    for (var k = 0u; k < 3u && u.product_lights_meta.x == 0.0; k += 1u) {
        let light = lights[k];
        radiance += pbr_brdf(i.base, i.parameters, n, v, light.xyz) * (light.w * max(dot(n, light.xyz), 0.0));
    }
    for (var k = 0u; k < min(u32(u.product_lights_meta.y),21u) && u.product_lights_meta.x > 0.0; k += 1u) {
        let source=u.product_lights[k*4u];
        let colour=u.product_lights[k*4u+1u];
        let axis=u.product_lights[k*4u+2u];
        let falloff=u.product_lights[k*4u+3u];
        if source.w == 4.0 {
            radiance += i.base*colour.xyz*(1.0-i.parameters.x)/3.141592653589793;
            continue;
        }
        let sample=product_light_sample(k,i.wp);
        radiance += pbr_brdf(i.base,i.parameters,n,v,sample[0])*sample[1]*max(dot(n,sample[0]),0.0);
    }
    let mapped = radiance / (vec3<f32>(1.0) + radiance);
    if (u.back.w > 0.5) { return vec4<f32>(mapped, 1.0); }
    return vec4<f32>(pbr_encode(mapped), 1.0);
}
"#;
