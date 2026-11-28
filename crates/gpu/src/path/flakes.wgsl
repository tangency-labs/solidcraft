// Original procedural paint flakes under the clear coat: the GPU form of
// solidcraft_viewport::flakes, which is the f64 contract. See docs/render-flakes.md.
struct FlakeRecord {
    shape: vec4<f32>, // coverage, inverse flake size per mm, spread, depth
    metal: vec4<f32>, // linear RGB reflectance, metallic
    finish: vec4<f32>, // perceptual roughness; the camera target's fraction of a cell
    cell: vec4<u32>, // the camera target's whole cells modulo the 65536-cell period
};
const MAX_FLAKE_TILT: f32 = 0.7853981633974483;
// The integer cell hash of the CPU contract; cells wrap at the 65536-cell period.
// A unit tangent of `n` that turns continuously with `n`: the axis projections, each weighted
// by its own length, summed. It vanishes only at isolated normals, where one cross product stands in.
fn flake_tangent(n: vec3<f32>) -> vec3<f32> {
    let px=vec3<f32>(1.0,0.0,0.0)-n*n.x;
    let py=vec3<f32>(0.0,1.0,0.0)-n*n.y;
    let pz=vec3<f32>(0.0,0.0,1.0)-n*n.z;
    let sum=px*length(px)+py*length(py)+pz*length(pz);
    let size=length(sum);
    if size>1.0e-6 { return sum/size; }
    if abs(n.x)<0.9 { return unit(cross(vec3<f32>(1.0,0.0,0.0),n)); }
    return unit(cross(vec3<f32>(0.0,1.0,0.0),n));
}

fn flake_hash(cell: vec3<u32>, salt: u32) -> u32 {
    let c=cell&vec3<u32>(65535u);
    var h=(c.x*0x8da6b343u)^(c.y*0xd8163841u)^(c.z*0xcb1ab31fu)^(salt*0x9e3779b9u);
    h=(h^(h>>16u))*0x7feb352du;
    h=(h^(h>>15u))*0x846ca68bu;
    return h^(h>>16u);
}
fn flake_unit(hash: u32) -> f32 {
    return f32(hash>>8u)/16777216.0;
}
// The layer's table entry + 1 sits above the stage and scalar-face flag bits.
fn flake_layer(t: Triangle) -> u32 {
    return u32(t.interior.z)>>2u;
}
fn on_a_flake(t: Triangle) -> bool {
    return any(t.flake.xyz!=vec3<f32>(0.0));
}
// The surface on the flake under a camera-relative point, from the final shading
// normal; between flakes and without flakes it is unchanged. flake.w is the share
// of the authored coverage that aging left visible.
fn on_flake(t: Triangle, point: vec3<f32>, n: vec3<f32>) -> Triangle {
    let index=flake_layer(t);
    if index==0u || index>64u { return t; }
    let layer=u.flakes[index-1u];
    let coverage=layer.shape.x*t.flake.w;
    if coverage<=0.0 { return t; }
    let q=point*layer.shape.y+layer.finish.yzw;
    let lower=floor(q);
    let local=q-lower;
    // Wrapping u32 addition is exact modulo the power-of-two period.
    let base=bitcast<vec3<u32>>(vec3<i32>(lower))+layer.cell.xyz;
    var nearest=base;
    var least=1e30;
    for(var dz=-1;dz<=1;dz+=1) {
        for(var dy=-1;dy<=1;dy+=1) {
            for(var dx=-1;dx<=1;dx+=1) {
                let offset=vec3<i32>(dx,dy,dz);
                let cell=base+bitcast<vec3<u32>>(offset);
                let centre=vec3<f32>(offset)+vec3<f32>(flake_unit(flake_hash(cell,0u)),flake_unit(flake_hash(cell,1u)),flake_unit(flake_hash(cell,2u)));
                let along=centre-local;
                let distance=dot(along,along);
                if distance<least { least=distance; nearest=cell; }
            }
        }
    }
    if coverage<1.0 && flake_unit(flake_hash(nearest,3u))>=coverage { return t; }
    let depth=layer.shape.w*flake_unit(flake_hash(nearest,6u));
    var normal=n;
    if layer.shape.z>0.0 {
        // The square root spreads the tilted normals evenly over their cone's disk.
        let tilt=layer.shape.z*MAX_FLAKE_TILT*sqrt(flake_unit(flake_hash(nearest,4u)));
        let turn=2.0*PI*flake_unit(flake_hash(nearest,5u));
        let tangent=flake_tangent(n);
        let bitangent=cross(n,tangent);
        normal=unit(tangent*(sin(tilt)*cos(turn))+bitangent*(sin(tilt)*sin(turn))+n*cos(tilt));
    }
    // A flake on top is the authored metal; the base paint covers a deep one.
    var result=t;
    result.base=vec4<f32>(mix(layer.metal.xyz,t.base.xyz,depth),t.base.w);
    result.parameters.x=mix(layer.metal.w,t.parameters.x,depth);
    result.parameters.y=mix(layer.finish.x,t.parameters.y,depth);
    result.flake=vec4<f32>(normal,t.flake.w);
    return result;
}
