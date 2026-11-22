// Original relative point/spot transport. No sampling RNG, photometry or clamp.
// Diagnostics count actual sampled surface/volume paths (adaptive pilots included).
fn punctual_count(kind:u32) {
    let word=8u+kind*2u;
    if atomicAdd(&failures[word],1u)==0xffffffffu { atomicAdd(&failures[word+1u],1u); }
}
fn punctual_radiance_valid(value:vec3<f32>)->bool {
    return all(value>=vec3<f32>(0.0)) && all(value<=vec3<f32>(1e12));
}
struct PunctualSample { direction:vec3<f32>, radiance:vec3<f32>, distance:f32, omitted:u32 };
fn punctual_sample(light:PunctualLight,position:PrecisePoint)->PunctualSample {
    // A barycentric anchor may have a large local displacement. Canonicalize
    // both parts before subtracting a nearby canonically packed source.
    let physical=punctual_offset(position,vec3<f32>(0.0));
    let delta=precise_point_delta(light.position.xyz,light.position_low.xyz,physical.hi,physical.lo);
    let distance=length(delta);
    // Count near-source before testing intensity/colour/cone, like the CPU.
    if !(distance>=light.position_low.w) {
        punctual_count(0u);
        return PunctualSample(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0),distance,1u);
    }
    let direction=delta/distance;
    var cone=1.0;
    if light.cone.y>0.0 {
        let cosine=dot(light.axis.xyz,-direction);
        let t=clamp((cosine-light.cone.x)/(light.axis.w-light.cone.x),0.0,1.0);
        cone=t*t*(3.0-2.0*t);
    }
    let ratio=light.position.w/distance;
    let incident=light.color.xyz*(light.color.w*ratio*ratio*cone);
    if !punctual_radiance_valid(incident) {
        punctual_count(1u);
        return PunctualSample(direction,vec3<f32>(0.0),distance,2u);
    }
    return PunctualSample(direction,incident,distance,0u);
}

// Finite delta sources exclude their endpoint, as the CPU next_down interval does.
fn punctual_endpoint(distance:f32)->f32 { return bitcast<f32>(bitcast<u32>(distance)-1u); }

// Canonicalize an anchored point before adding a tiny geometric offset. Adding
// directly to an arbitrarily large local barycentric displacement can erase it.
fn punctual_offset(point:PrecisePoint,offset:vec3<f32>)->PrecisePoint {
    var hi=vec3<f32>(0.0);var lo=vec3<f32>(0.0);
    for(var axis=0u;axis<3u;axis+=1u) {
        hi[axis]=bitcast<f32>(fp_add_bits(bitcast<u32>(point.hi[axis]),bitcast<u32>(point.lo[axis])));
        lo[axis]=anchored_delta_component(point.hi[axis],point.lo[axis],hi[axis],0.0)+offset[axis];
    }
    return PrecisePoint(hi,lo);
}
