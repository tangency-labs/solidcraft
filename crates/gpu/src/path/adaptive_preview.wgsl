// Display copy only: never writes the independently sampled production film.
struct Parameters { image:vec4<u32>, work:vec4<u32> };
@group(0) @binding(0) var<uniform> u:Parameters;
@group(0) @binding(1) var<storage,read> film:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read_write> display:array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {
    let pixels=u.image.x*u.image.y;
    let pixel=u.image.w+gid.x;
    if gid.x>=u.work.x || pixel>=pixels { return; }
    if u.image.z<=u.work.y {
        display[pixel]=film[pixels+pixel];
    } else {
        let completed=u.image.z-u.work.y;
        let budget=u32(film[2u*pixels+pixel].x);
        // Completed pixels were divided in place by the production shader already.
        if completed<budget { display[pixel]=film[pixel]/f32(completed); }
        else { display[pixel]=film[pixel]; }
    }
}
