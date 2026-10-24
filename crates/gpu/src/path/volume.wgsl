// Original bounded homogeneous-medium transport. Propagation directions define
// the HG cosine; positive anisotropy is forward. No density textures or emission.
fn volume_segment(o: vec3<f32>, d: vec3<f32>, limit: f32) -> vec2<f32> {
    if u.volume_min.w <= 0.0 { return vec2<f32>(0.0); }
    var enter = 0.0;
    var leave = limit;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        if d[axis] == 0.0 {
            if o[axis] < u.volume_min[axis] || o[axis] > u.volume_max[axis] { return vec2<f32>(0.0); }
        } else {
            let a = (u.volume_min[axis]-o[axis])/d[axis];
            let b = (u.volume_max[axis]-o[axis])/d[axis];
            enter=max(enter,min(a,b));
            leave=min(leave,max(a,b));
        }
        if enter >= leave { return vec2<f32>(0.0); }
    }
    return vec2<f32>(enter,leave);
}
fn volume_transmittance(o: vec3<f32>, d: vec3<f32>, distance: f32) -> f32 {
    let segment = volume_segment(o,d,distance);
    return exp(-u.volume_min.w*(segment.y-segment.x));
}
fn volume_phase(cosine: f32) -> f32 {
    let g=u.volume_max.w;
    let denominator=1.0+g*g-2.0*g*clamp(cosine,-1.0,1.0);
    return (1.0-g*g)/(4.0*PI*denominator*sqrt(denominator));
}
fn hg_cosine(uniform: f32,g: f32) -> f32 {
    if abs(g)<0.1 {
        // Algebraic inverse-CDF form continuous at g=0, not an isotropic
        // approximation paired with a different HG evaluation density.
        let t=2.0*uniform-1.0;
        let numerator=t+0.5*g*(3.0+t*t)+g*g*t+0.5*g*g*g*(t*t-1.0);
        let denominator=1.0+g*t;
        return clamp(numerator/(denominator*denominator),-1.0,1.0);
    }
    let ratio=(1.0-g*g)/(1.0-g+2.0*g*uniform);
    return clamp((1.0+g*g-ratio*ratio)/(2.0*g),-1.0,1.0);
}
fn volume_cosine(uniform: f32) -> f32 {return hg_cosine(uniform,u.volume_max.w);}
fn sample_hg(direction: vec3<f32>,g:f32,rng: ptr<function,u32>) -> vec3<f32> {
    let cosine=hg_cosine(random(rng),g);
    let angle=2.0*PI*random(rng);
    let radius=sqrt(max(0.0,1.0-cosine*cosine));
    return from_local(vec3<f32>(radius*cos(angle),radius*sin(angle),cosine),direction);
}
fn volume_sample(direction: vec3<f32>,rng:ptr<function,u32>) -> vec3<f32> {
    return sample_hg(direction,u.volume_max.w,rng);
}
fn volume_visible(o: vec3<f32>, d: vec3<f32>, distance: f32) -> ShadowResult {
    // Volume launches are interior points, not offsets from a surface. Glass is
    // refused by admission, so the ordinary opaque traversal needs no interface walk.
    if emitter_hit_bounded(o,d,0.0).distance<distance-u.lighting.x*2.0 { return blocked_shadow(); }
    let hit=trace_ordered_within(o,d,true,0xffffffffu,0.0,false,distance,-1.0);
    if hit.triangle!=0xffffffffu { return blocked_shadow(); }
    let attenuation=volume_transmittance(o,d,distance);
    return ShadowResult(vec3<f32>(attenuation),attenuation,0u,false);
}
fn volume_direct(position: vec3<f32>, incoming: vec3<f32>, remaining: u32, rng: ptr<function,u32>) -> vec3<f32> {
    var radiance=vec3<f32>(0.0);
    let count=u32(u.light_count.w);
    // One shared visibility evaluation avoids cloning the complete BVH walk
    // separately for area, environment and sun source families in this branch.
    for (var source=0u; source<count+2u; source+=1u) {
        var sample=EnvironmentSample(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0),0.0);
        var distance=FAR;
        if source<count {
            let light=u.area_lights[source];
            let point=light.center.xyz+light.u.xyz*(2.0*random(rng)-1.0)+light.v.xyz*(2.0*random(rng)-1.0);
            let delta=point-position;
            distance=length(delta);
            if distance<=u.lighting.x { continue; }
            sample=EnvironmentSample(unit(delta),light.radiance.xyz,area_pdf(light,delta));
        } else if source==count {
            if u.environment_map.w<=0.0 && !any(u.environment.xyz>vec3<f32>(0.0)) { continue; }
            sample=env_sample(rng);
        } else {
            if u.sun_radiance.w<=0.0 { continue; }
            sample=sun_sample(rng);
        }
        if sample.pdf<=0.0 { continue; }
        let visibility=volume_visible(position,sample.direction,distance);
        let phase=volume_phase(dot(incoming,sample.direction));
        let weight=select(power_weight(sample.pdf,phase*visibility.branch_pdf),1.0,remaining==0u);
        radiance+=visibility.transmittance*sample.radiance*(phase*weight/sample.pdf);
    }
    return radiance;
}
