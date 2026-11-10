// Recombine exclusive filtered radiance in scene-linear space. Preserve beauty alpha.
struct Parameters { image:vec4<u32>, work:vec4<u32>, window:vec4<u32> };
@group(0) @binding(0) var<uniform> u:Parameters;
@group(0) @binding(1) var<storage,read> filtered:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> counts:array<vec4<f32>>;
@group(0) @binding(3) var<storage,read_write> sum:array<vec4<f32>>;
@compute @workgroup_size(64)
fn add(@builtin(global_invocation_id) gid:vec3<u32>) {
    let pixel=u.image.w+gid.x;
    if gid.x>=u.work.x || pixel>=u.window.y || counts[pixel].w==0.0 {return;}
    sum[pixel]=vec4<f32>(sum[pixel].xyz+filtered[pixel].xyz,sum[pixel].w);
}
