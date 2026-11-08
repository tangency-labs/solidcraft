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
fn volume_transmittance_precise(o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>, distance: f32) -> f32 {
    if all(o_low==vec3<f32>(0.0)) { return volume_transmittance(o,d,distance); }
    if u.volume_min.w<=0.0 { return 1.0; }
    let lo=precise_point_delta(u.volume_min.xyz,vec3<f32>(0.0),o,o_low);
    let hi=precise_point_delta(u.volume_max.xyz,vec3<f32>(0.0),o,o_low);
    var enter=0.0; var leave=distance;
    for(var axis=0u;axis<3u;axis+=1u) {
        if d[axis]==0.0 { if lo[axis]>0.0 || hi[axis]<0.0 { return 1.0; } }
        else { let a=lo[axis]/d[axis]; let b=hi[axis]/d[axis]; enter=max(enter,min(a,b));leave=min(leave,max(a,b)); }
        if enter>=leave { return 1.0; }
    }
    return exp(-u.volume_min.w*(leave-enter));
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
fn volume_visible(o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>, distance: f32, punctual: bool) -> ShadowResult {
    // Volume launches are interior points, not offsets from a surface. Glass is
    // refused by admission, so the ordinary opaque traversal needs no interface walk.
    if emitter_hit_precise(o,o_low,d,0.0).distance<distance-select(u.lighting.x*2.0,0.0,punctual) { return blocked_shadow(); }
    let hit=trace_precise_within(o,o_low,d,true,0xffffffffu,0.0,false,distance,-1.0);
    if hit.triangle!=0xffffffffu { return blocked_shadow(); }
    let attenuation=volume_transmittance_precise(o,o_low,d,distance);
    return ShadowResult(vec3<f32>(attenuation),attenuation,0u,false);
}
fn volume_direct(position: vec3<f32>, physical: PrecisePoint, incoming: vec3<f32>, remaining: u32, rng: ptr<function,u32>) -> vec3<f32> {
    var radiance=vec3<f32>(0.0);
    let count=u32(u.light_count.w);
    // One shared visibility evaluation avoids cloning the complete BVH walk
    // separately for area, environment and sun source families in this branch.
    for (var source=0u; source<count+2u+u32(u.light_count.x)+u32(u.light_count.y); source+=1u) {
        var sample=EnvironmentSample(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0),0.0);
        var distance=FAR;
        var origin=position;
        var origin_low=vec3<f32>(0.0);
        var punctual=false;
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
        } else if source==count+1u {
            if u.sun_radiance.w<=0.0 { continue; }
            sample=sun_sample(rng);
        } else if source<count+2u+u32(u.light_count.x) {
            let light=u.directional_lights[source-count-2u];
            if light.direction.w<=0.0 || !any(light.color.xyz>vec3<f32>(0.0)) { continue; }
            sample=EnvironmentSample(light.direction.xyz,light.color.xyz*light.direction.w,1.0);
        } else {
            punctual=true;
            let light=u.punctual_lights[source-count-2u-u32(u.light_count.x)];
            let value=punctual_sample(light,physical);
            if value.omitted!=0u { continue; }
            sample=EnvironmentSample(value.direction,value.radiance,1.0);
            origin=physical.hi; origin_low=physical.lo;
            distance=punctual_endpoint(value.distance);
        }
        if sample.pdf<=0.0 { continue; }
        let phase=volume_phase(dot(incoming,sample.direction));
        if punctual && !punctual_radiance_valid(sample.radiance*phase) { punctual_count(1u); continue; }
        let visibility=volume_visible(origin,origin_low,sample.direction,distance,punctual);
        let weight=select(power_weight(sample.pdf,phase*visibility.branch_pdf),1.0,remaining==0u || source>=count+2u);
        radiance+=visibility.transmittance*sample.radiance*(phase*weight/sample.pdf);
    }
    return radiance;
}

// Only the primary orthographic optical line uses signed entry. Secondary,
// shadow and perspective rays retain volume_segment's physical forward origin.
// The caller supplies the original forward geometry/emitter endpoint unchanged.
fn volume_orthographic_segment(o: vec3<f32>, d: vec3<f32>, limit: f32) -> vec2<f32> {
    var enter = -FAR;
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
