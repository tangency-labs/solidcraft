// Original deterministic first-hit guides. Uses the production intersection and
// material/normal texture evaluation, without advancing the path random stream.
@compute @workgroup_size(64)
fn denoise_guides(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pixel=u.image.w+gid.x;
    if gid.x>=u.sampling.z || pixel>=u.image.x*u.image.y { return; }
    let base=pixel*3u;
    accumulation[base]=vec4<f32>(0.0);
    accumulation[base+1u]=vec4<f32>(0.0);
    accumulation[base+2u]=vec4<f32>(0.0);
    let xy=vec2<f32>(f32(pixel%u.image.x)+0.5,f32(pixel/u.image.x)+0.5);
    let uv=vec2<f32>(xy.x/f32(u.image.x)*2.0-1.0,1.0-xy.y/f32(u.image.y)*2.0);
    let ray=primary_ray(uv,vec2<f32>(0.0));
    let hit=trace(ray.origin,ray.direction,false);
    if hit.triangle==0xffffffffu { return; }
    if u.environment.w>0.0 && emitter_hit_bounded(ray.origin,ray.direction,u.lighting.x).distance<hit.distance { return; }
    var t=triangle(hit.triangle);
    let position=surface_point(t,hit.bary);
    t=texture_surface(t,position);
    t=aged_surface(t,position,unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz)));
    // Single first-hit guides cannot describe refracted or sharp reflected images.
    if t.base.w>0.0 || (t.parameters.x>0.5 && t.parameters.y<0.25) || (t.coat.x>0.0 && t.coat.y<0.05) { return; }
    var geometric=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
    if dot(geometric,-ray.direction)<0.0 { geometric=-geometric; }
    var n=unit(t.n0.xyz*hit.bary.x+t.n1.xyz*hit.bary.y+t.n2.xyz*hit.bary.z);
    if dot(n,geometric)<0.0 { n=-n; }
    if dot(n,-ray.direction)<=0.0 { n=geometric; }
    let mapped=texture_normal(t,position,n,geometric);
    if dot(mapped,-ray.direction)>0.0 { n=mapped; }
    let depth=dot(position-u.eye.xyz,-u.back.xyz);
    var footprint=2.0*length(u.up.xyz)/f32(u.image.y);
    if u.eye.w>0.5 { footprint*=max(depth,0.0)/u.back.w; }
    accumulation[base]=vec4<f32>(t.base.xyz,t.optics.w);
    accumulation[base+1u]=vec4<f32>(n,depth);
    accumulation[base+2u]=vec4<f32>(position,max(footprint,0.000001));
}
