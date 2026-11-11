// One normalized scene-linear component and its camera-conditional uncertainty.
struct Parameters { image:vec4<u32>, work:vec4<u32>, window:vec4<u32> };
struct Uncertainty { variance:vec3<f32>, count:u32 };
@group(0) @binding(0) var<uniform> u:Parameters;
@group(0) @binding(1) var<storage,read> moments:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> counts:array<vec4<f32>>;
@group(0) @binding(3) var<storage,read> beauty:array<vec4<f32>>;
@group(0) @binding(4) var<storage,read_write> prepared:array<vec4<f32>>;
@group(0) @binding(5) var<storage,read_write> uncertainty:array<Uncertainty>;
@group(0) @binding(6) var<storage,read_write> sum:array<vec4<f32>>;
@compute @workgroup_size(64)
fn prepare(@builtin(global_invocation_id) gid:vec3<u32>) {
    let pixel=u.image.w+gid.x;
    if gid.x>=u.work.x || pixel>=u.window.y {return;}
    let count=counts[pixel].w;
    let original=beauty[pixel]/f32(u.work.y);
    prepared[pixel]=vec4<f32>(prepared[pixel].xyz,original.w);
    var variance=vec3<f32>(0.0);
    if count>=2.0 {variance=moments[pixel].xyz/(count*(count-1.0));}
    uncertainty[pixel]=Uncertainty(variance,u32(count));
    if u.window.w==0u {
        // Independent adaptive pilots have no production components yet.
        sum[pixel]=select(vec4<f32>(vec3<f32>(0.0),original.w),original,count==0.0);
    }
}
