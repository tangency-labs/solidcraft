// Original three-pass edge-avoiding a-trous filter inspired by Dammertz et al.
// Inputs/outputs keep accumulated linear premultiplied radiance and raw alpha.
struct FilterParams { image: vec4<u32>, work: vec4<u32> };
@group(0) @binding(0) var<uniform> f: FilterParams;
@group(0) @binding(1) var<storage,read> guides: array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> source: array<vec4<f32>>;
@group(0) @binding(3) var<storage,read_write> filtered: array<vec4<f32>>;
fn kernel(x:i32)->f32 {
    if x==0 { return 6.0; }
    if abs(x)==1 { return 4.0; }
    return 1.0;
}
fn max_component(v:vec3<f32>)->f32 { return max(max(v.x,v.y),v.z); }
@compute @workgroup_size(64)
fn atrous(@builtin(global_invocation_id) gid:vec3<u32>) {
    let pixel=f.image.w+gid.x;
    if gid.x>=f.work.x || pixel>=f.image.x*f.image.y { return; }
    let c=source[pixel];
    let a=guides[pixel*3u]; let n=guides[pixel*3u+1u]; let p=guides[pixel*3u+2u];
    if a.w==0.0 || c.w<f32(f.work.y)*0.999 { filtered[pixel]=c; return; }
    let xy=vec2<i32>(i32(pixel%f.image.x),i32(pixel/f.image.x));
    var mean=vec3<f32>(0.0); var total=0.0;
    let samples=f32(f.work.y);
    let color=c.xyz/samples;
    for(var y=-2;y<=2;y+=1) { for(var x=-2;x<=2;x+=1) {
        let q=xy+vec2<i32>(x,y)*i32(f.image.z);
        if any(q<vec2<i32>(0)) || q.x>=i32(f.image.x) || q.y>=i32(f.image.y) { continue; }
        let j=u32(q.y)*f.image.x+u32(q.x);
        let b=guides[j*3u]; let m=guides[j*3u+1u]; let r=guides[j*3u+2u];
        let v=source[j];
        if b.w!=a.w || abs(v.w-c.w)>samples*0.01 { continue; }
        let da=a.xyz-b.xyz; let dp=r.xyz-p.xyz;
        let plane=abs(dot(dp,n.xyz))+abs(dot(dp,m.xyz));
        let scale=max(0.05*max(p.w,r.w)*f32(f.image.z),0.000001);
        let depth=abs(n.w-m.w)/max(8.0*max(p.w,r.w)*f32(f.image.z),0.000001);
        let dc=max_component(abs(color-v.xyz/samples));
        let color_scale=0.25+0.5*max(max_component(color),max_component(v.xyz/samples));
        let weight=kernel(x)*kernel(y)*exp(-dot(da,da)/0.04-plane/scale-depth-dc/color_scale)
                   *pow(max(dot(n.xyz,m.xyz),0.0),32.0);
        if weight>0.0 {
            total+=weight;
            // Online convex averaging avoids overflow of radiance times kernel.
            mean+=(v.xyz-mean)*(weight/total);
        }
    }}
    // The centre always contributes; retain original coverage without filtering.
    filtered[pixel]=vec4<f32>(mean,c.w);
}
