// Original periodic 3D masks, not measured weathering or corrosion simulation.
// A layer changes the same surface closure used by direct and indirect transport.
struct AgingLayer {
    identity: vec4<f32>, // kind, captured body+1 (zero = all), material class, seed
    mask: vec4<f32>, // strength, coverage, inverse mm scale, upward bias
    origin: vec4<f32>, // periodic field offset; w: base5 Scratch cell carries x/y/z
    color: vec4<f32>, // linear coat RGB, perceptual roughness
    finish: vec4<f32>, // metallic, reserved
};
fn aging_hash(cell: vec3<i32>, seed: u32) -> f32 {
    let c=bitcast<vec3<u32>>(cell)&vec3<u32>(4095u);
    var h=(c.x*0x8da6b343u)^(c.y*0xd8163841u)^(c.z*0xcb1ab31fu)^(seed*0x9e3779b9u);
    h=(h^(h>>16u))*0x7feb352du;
    h=(h^(h>>15u))*0x846ca68bu;
    h=h^(h>>16u);
    return f32(h>>8u)/16777216.0;
}
fn aging_noise(point: vec3<f32>, seed: u32) -> f32 {
    let base=vec3<i32>(floor(point));
    let f=fract(point);
    let w=f*f*(vec3<f32>(3.0)-2.0*f);
    let low=mix(mix(aging_hash(base,seed),aging_hash(base+vec3<i32>(1,0,0),seed),w.x),
                mix(aging_hash(base+vec3<i32>(0,1,0),seed),aging_hash(base+vec3<i32>(1,1,0),seed),w.x),w.y);
    let high=mix(mix(aging_hash(base+vec3<i32>(0,0,1),seed),aging_hash(base+vec3<i32>(1,0,1),seed),w.x),
                 mix(aging_hash(base+vec3<i32>(0,1,1),seed),aging_hash(base+vec3<i32>(1,1,1),seed),w.x),w.y);
    return mix(low,high,w.z);
}
// Scratch samples the same periodic noise from bounded cell/fraction parts.
// In particular, multiplying a large q by3 before fract must not discard the
// authored fine phase merely because the camera moved by an integer period.
fn aging_floor_mod4096(value:f32)->u32 {
    let bits=bitcast<u32>(floor(value));
    let biased=(bits>>23u)&255u;
    if biased<127u {return 0u;}
    let exponent=biased-127u;
    let significand=(bits&0x7fffffu)|0x800000u;
    var cell_index=0u;
    if exponent<23u {
        cell_index=(significand>>(23u-exponent))&4095u;
    } else if exponent<35u {
        cell_index=(significand<<(exponent-23u))&4095u;
    }
    if (bits&0x80000000u)!=0u {return (4096u-cell_index)&4095u;}
    return cell_index;
}
fn aging_noise_parts(base:vec3<i32>,f:vec3<f32>,seed:u32)->f32 {
    let w=f*f*(vec3<f32>(3.0)-2.0*f);
    let low=mix(mix(aging_hash(base,seed),aging_hash(base+vec3<i32>(1,0,0),seed),w.x),
                mix(aging_hash(base+vec3<i32>(0,1,0),seed),aging_hash(base+vec3<i32>(1,1,0),seed),w.x),w.y);
    let high=mix(mix(aging_hash(base+vec3<i32>(0,0,1),seed),aging_hash(base+vec3<i32>(1,0,1),seed),w.x),
                 mix(aging_hash(base+vec3<i32>(0,1,1),seed),aging_hash(base+vec3<i32>(1,1,1),seed),w.x),w.y);
    return mix(low,high,w.z);
}
// Exact modulo of the represented f32 integer floor. Avoid i32 saturation
// when admitted world/camera offsets are large compared with the layer scale.
// The ordinary (<2^23) field path uses one shift and remainder, no loop.
fn aging_floor_mod5120(value:f32)->u32 {
    let bits=bitcast<u32>(floor(value));
    let biased=(bits>>23u)&255u;
    if biased<127u {return 0u;}
    let exponent=biased-127u;
    let significand=(bits&0x7fffffu)|0x800000u;
    var cell_index=0u;
    if exponent<23u {
        cell_index=(significand>>(23u-exponent))%5120u;
    } else {
        cell_index=significand%5120u;
        // A finite f32 has at most104 integer bits beyond its significand.
        // The1e7 mm geometry rebase and0.01 mm minimum scale bound
        // admitted q near1e9 cells, requiring at most6 iterations.
        for(var shift=0u;shift<104u;shift+=1u) {
            if shift>=exponent-23u {break;}
            cell_index=(cell_index*2u)%5120u;
        }
    }
    if (bits&0x80000000u)!=0u && cell_index!=0u {return 5120u-cell_index;}
    return cell_index;
}
fn aging_scratch_band(value:f32, camera_cell:u32)->u32 {
    // floor(4*Q/5) repeats every5120 Q cells. Preserve its integer phase
    // separately from noise's4096-cell quotient; never form a20480-cell float.
    let cell_index=(aging_floor_mod5120(value)+4096u*camera_cell)%5120u;
    let numerator=4u*cell_index;
    let whole=numerator/5u;
    // Remainder0/1 cannot cross a band; the other exact thresholds are
    // 3/4,1/2,1/4. Integer/fraction separation avoids 0.8*q rounding a
    // value just below a boundary into the next band.
    let threshold=f32(5u-numerator%5u)*0.25;
    return (whole+select(0u,1u,fract(value)>=threshold))&4095u;
}
fn aging_mask(layer: AgingLayer, point:vec3<f32>, normal:vec3<f32>)->vec2<f32> {
    let q=point*layer.mask.z+layer.origin.xyz;
    let seed=u32(layer.identity.w);
    let kind=u32(layer.identity.x);
    var coarse=0.0;
    var fine=0.0;
    if kind==5u {
        let cell=vec3<u32>(aging_floor_mod4096(q.x),aging_floor_mod4096(q.y),aging_floor_mod4096(q.z));
        let fraction=fract(q);
        let scaled_fraction=fraction*3.0;
        let fine_cell=(cell*3u+vec3<u32>(floor(scaled_fraction))+vec3<u32>(19u,7u,3u))&vec3<u32>(4095u);
        coarse=aging_noise_parts(vec3<i32>(cell),fraction,seed);
        fine=aging_noise_parts(vec3<i32>(fine_cell),fract(scaled_fraction),seed+1u);
    } else {
        coarse=aging_noise(q,seed);
        fine=aging_noise(q*3.0+vec3<f32>(19.0,7.0,3.0),seed+1u);
    }
    let field=coarse*0.75+fine*0.25;
    let threshold=1.0-layer.mask.y;
    var coverage=smoothstep(threshold-0.09,threshold+0.09,field);
    if layer.mask.y>=1.0 {coverage=1.0;}
    if kind==5u {
        // Long shallow visual scratches in a dominant-axis projection. They do
        // not create grooves, silhouettes, cavity masks or geometric curvature.
        let n=abs(normal);
        var uv=q.xy;
        let phase_code=u32(layer.origin.w);
        var camera_cell=(phase_code/5u)%5u;
        if n.x>n.y && n.x>n.z {uv=q.yz;camera_cell=phase_code/25u;} else if n.y>n.z {uv=q.xz;camera_cell=phase_code/25u;}
        let band=i32(aging_scratch_band(uv.y,camera_cell));
        // The discarded whole u contributes an integer24 phase. Reducing
        // before multiplication preserves the shallow line at distant origins.
        let line=abs(fract(fract(uv.x)*24.0+aging_hash(vec3<i32>(band,0,0),seed)*0.7)-0.5);
        coverage*=1.0-smoothstep(0.025,0.09,line);
        coverage*=smoothstep(0.18,0.48,fine);
    }
    let slope=pow(max(normal.z,0.0),1.5);
    coverage*=mix(1.0,slope,layer.mask.w);
    return vec2<f32>(clamp(coverage*layer.mask.x,0.0,1.0),field);
}
fn aged_surface(original:Triangle,point:vec3<f32>,outward:vec3<f32>)->Triangle {
    if original.base.w>0.0 || original.interior.z>0.0 {return original;}
    var result=original;
    for(var index=0u;index<8u;index+=1u) {
        let layer=u.aging[index];
        if layer.identity.x==0.0 {continue;}
        if layer.identity.y>0.0 && layer.identity.y!=original.optics.w {continue;}
        let material_class=u32(layer.identity.z);
        if (material_class==1u && original.parameters.x<0.5) || (material_class==2u && original.parameters.x>=0.5) {continue;}
        let mask=aging_mask(layer,point,outward);
        var coat=layer.color.xyz;
        let kind=u32(layer.identity.x);
        if kind==1u || kind==2u || kind==4u {coat*=0.45+0.7*mask.y;}
        if kind==7u {coat=mix(result.base.xyz,layer.color.xyz,0.5);}
        result.base=vec4<f32>(mix(result.base.xyz,clamp(coat,vec3<f32>(0.0),vec3<f32>(1.0)),mask.x),result.base.w);
        result.parameters.x=mix(result.parameters.x,layer.finish.x,mask.x);
        result.parameters.y=mix(result.parameters.y,layer.color.w,mask.x);
        // Authored contamination/abrasion covers or removes the original clear coat.
        result.coat.x*=1.0-mask.x;
    }
    return result;
}
