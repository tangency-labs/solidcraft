// Original periodic 3D masks, not measured weathering or corrosion simulation.
// A layer changes the same surface closure used by direct and indirect transport.
struct AgingLayer {
    identity: vec4<f32>, // kind, captured body+1 (zero = all), material class, seed
    mask: vec4<f32>, // strength, coverage, inverse mm scale, upward bias
    origin: vec4<f32>, // rebased periodic field offset
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
fn aging_mask(layer: AgingLayer, point:vec3<f32>, normal:vec3<f32>)->vec2<f32> {
    let q=point*layer.mask.z+layer.origin.xyz;
    let seed=u32(layer.identity.w);
    let coarse=aging_noise(q,seed);
    let fine=aging_noise(q*3.0+vec3<f32>(19.0,7.0,3.0),seed+1u);
    let field=coarse*0.75+fine*0.25;
    let threshold=1.0-layer.mask.y;
    var coverage=smoothstep(threshold-0.09,threshold+0.09,field);
    if layer.mask.y>=1.0 {coverage=1.0;}
    let kind=u32(layer.identity.x);
    if kind==5u {
        // Long shallow visual scratches in a dominant-axis projection. They do
        // not create grooves, silhouettes, cavity masks or geometric curvature.
        let n=abs(normal);
        var uv=q.xy;
        if n.x>n.y && n.x>n.z {uv=q.yz;} else if n.y>n.z {uv=q.xz;}
        let band=i32(floor(uv.y*0.8));
        let line=abs(fract(uv.x*24.0+aging_hash(vec3<i32>(band,0,0),seed)*0.7)-0.5);
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
