@group(0) @binding(5) var<storage, read> texture_words: array<u32>;
fn texture_vector(word:u32)->vec3<f32> {
    return bitcast<vec3<f32>>(vec3<u32>(texture_words[word],texture_words[word+1u],texture_words[word+2u]));
}
fn texture_record(body_id:f32)->u32 {
    if !HAS_TEXTURES {return 0u;}
    let body=u32(body_id);
    if body>=texture_words[0] {return 0u;}
    return texture_words[4u+body];
}
fn texture_pixel(offset:u32,width:u32,x:u32,y:u32)->vec3<f32> {
    return texture_vector(offset+4u*(y*width+x));
}
fn texture_bilinear(offset:u32,size:vec2<u32>,uv:vec2<f32>)->vec3<f32> {
    let position=fract(uv)*vec2<f32>(size)-vec2<f32>(0.5);
    let lower=vec2<i32>(floor(position));
    // fract restricts lower to[-1,size-1]. Avoid integer modulo after a BVH
    // walk: native shader compilers have already failed on that pattern.
    let bounded=vec2<u32>(max(lower,vec2<i32>(0)));
    let first=select(bounded,size-vec2<u32>(1u),lower<vec2<i32>(0));
    let next=first+vec2<u32>(1u);
    let second=select(next,vec2<u32>(0u),next>=size);
    let fraction=fract(position);
    let a=texture_pixel(offset,size.x,first.x,first.y);
    let b=texture_pixel(offset,size.x,second.x,first.y);
    let c=texture_pixel(offset,size.x,first.x,second.y);
    let d=texture_pixel(offset,size.x,second.x,second.y);
    return mix(mix(a,b,fraction.x),mix(c,d,fraction.x),fraction.y);
}
struct TextureGradients { dx:vec2<f32>, dy:vec2<f32> };
fn texture_lod(size:vec2<u32>,gradients:TextureGradients,last:u32)->f32 {
    // Same largest-singular-value policy as select_isotropic_lod. Differentiate
    // unwrapped coordinates: crossing a repeat seam never enlarges a footprint.
    let columns=vec4<f32>(gradients.dx*vec2<f32>(size),gradients.dy*vec2<f32>(size));
    let magnitude=abs(columns);
    let scale=max(max(magnitude.x,magnitude.y),max(magnitude.z,magnitude.w));
    if scale<=0.0 {return 0.0;}
    let q=columns/scale;
    let a=dot(q.xy,q.xy);let b=dot(q.zw,q.zw);let c=dot(q.xy,q.zw);
    let radius=scale*sqrt(max(0.0,0.5*(a+b+length(vec2<f32>(a-b,2.0*c)))));
    return clamp(log2(max(radius,1.0)),0.0,f32(last));
}
fn texture_level(descriptor:u32,level:u32,uv:vec2<f32>)->vec3<f32> {
    let record=descriptor+4u+level*4u;
    return texture_bilinear(texture_words[record],vec2<u32>(texture_words[record+1u],texture_words[record+2u]),uv);
}
fn texture_sample(descriptor:u32,size:vec2<u32>,uv:vec2<f32>,gradients:TextureGradients)->vec3<f32> {
    let last=texture_words[descriptor]-1u;
    var lod=0.0;
    if last>0u {lod=texture_lod(size,gradients,last);}
    let low=u32(floor(lod));let high=min(low+1u,last);
    let count=min(texture_words[descriptor+1u],select(2u,1u,low==high || fract(lod)==0.0));
    var color=vec3<f32>(0.0);
    // One bilinear call site keeps native compilation bounded when this sampler
    // shares a path with glass traversal. The admitted descriptor allows 1..2 levels.
    for(var sample=0u;sample<count;sample+=1u) {
        let rgb=texture_level(descriptor,select(low,high,sample>0u),uv);
        if sample==0u {color=rgb;} else {color=mix(color,rgb,fract(lod));}
    }
    return color;
}
fn texture_uv(record:u32,position:vec3<f32>)->vec2<f32> {
    let delta=position-texture_vector(record);
    let uv=vec2<f32>(dot(delta,texture_vector(record+4u)),dot(delta,texture_vector(record+8u)));
    return vec2<f32>(uv.x,select(uv.y,-uv.y,(texture_words[record+19u]&1u)!=0u));
}
fn texture_plane_differential(normal:vec3<f32>,direction:vec3<f32>,delta:vec3<f32>)->vec3<f32> {
    return delta-direction*(dot(normal,delta)/dot(normal,direction));
}
fn texture_camera_direction_delta(direction:vec3<f32>,delta:vec3<f32>)->vec3<f32> {
    // Fixed-lens focus_scale/length(q) equals forward cosine / camera distance.
    // Divide the pixel delta first to avoid a large intermediate reciprocal.
    let scaled=delta/u.back.w;
    return (scaled-direction*dot(direction,scaled))*dot(direction,-unit(u.back.xyz));
}
fn texture_world_gradients(record:u32,dx:vec3<f32>,dy:vec3<f32>)->TextureGradients {
    let inverse_u=texture_vector(record+4u);let inverse_v=texture_vector(record+8u);
    let sign_v=select(1.0,-1.0,(texture_words[record+19u]&1u)!=0u);
    return TextureGradients(vec2<f32>(dot(dx,inverse_u),dot(dx,inverse_v)*sign_v),vec2<f32>(dot(dy,inverse_u),dot(dy,inverse_v)*sign_v));
}
fn texture_gradients(record:u32,normal:vec3<f32>,direction:vec3<f32>,distance:f32,path_length:f32,bounce:u32)->TextureGradients {
    if record==0u {return TextureGradients(vec2<f32>(0.0),vec2<f32>(0.0));}
    if bounce==0u && abs(dot(normal,direction))>1e-6 {
        var dx=2.0*u.right.xyz/f32(u.image.x);
        var dy=-2.0*u.up.xyz/f32(u.image.y);
        if u.eye.w>0.5 {
            // For a fixed lens sample, focus_scale/length(q) is exactly the
            // forward direction cosine / camera distance. Derive at this hit;
            // no camera differential stays live across the medium stack.
            dx=texture_camera_direction_delta(direction,dx)*distance;
            dy=texture_camera_direction_delta(direction,dy)*distance;
        }
        dx=texture_plane_differential(normal,direction,dx);
        dy=texture_plane_differential(normal,direction,dy);
        return texture_world_gradients(record,dx,dy);
    }
    // Secondary paths use a finite camera-pixel cone, expanded by total path
    // length and grazing angle; focused specular differentials are not tracked.
    let pixel_span=2.0*max(length(u.right.xyz)/f32(u.image.x),length(u.up.xyz)/f32(u.image.y));
    let distance_scale=select(1.0,path_length/max(u.back.w,1e-6),u.eye.w>0.5);
    let span=min(pixel_span*distance_scale/max(abs(dot(normal,direction)),1e-6),1e20);
    let axis=select(vec3<f32>(1.0,0.0,0.0),vec3<f32>(0.0,0.0,1.0),abs(normal.z)<0.9);
    let tangent=unit(cross(normal,axis));
    return texture_world_gradients(record,tangent*span,cross(normal,tangent)*span);
}
// Sampling changes RGB, metallic and roughness only. Transmission, IOR and
// thickness never cross the sampler boundary, even in mixed glass/map scenes.
struct TextureSurface { color:vec3<f32>, metallic:f32, roughness:f32, normal:vec4<f32> };
fn texture_surface(record:u32,color:vec3<f32>,metallic:f32,roughness:f32,position:vec3<f32>,gradients:TextureGradients,scalar_face:bool)->TextureSurface {
    if record==0u {return TextureSurface(color,metallic,roughness,vec4<f32>(0.5,0.5,1.0,0.0));}
    var mapped_color=color;
    var mapped_metallic=metallic;
    var mapped_roughness=roughness;
    var mapped_normal=vec4<f32>(0.5,0.5,1.0,0.0);
    let uv=texture_uv(record,position);
    // A single sampling call site keeps the native path compiler from inlining
    // four complete trilinear samplers while its traversal/medium state is live.
    // The runtime bound excludes unused trailing roles and prevents unroll.
    for(var role=0u;role<min(texture_words[record+23u],4u);role+=1u) {
        var descriptor=texture_words[record+3u];
        var size=vec2<u32>(texture_words[record+7u],texture_words[record+11u]);
        if role==1u {descriptor=texture_words[record+12u];size=vec2<u32>(texture_words[record+13u],texture_words[record+14u]);}
        if role==2u {descriptor=texture_words[record+16u];size=vec2<u32>(texture_words[record+17u],texture_words[record+18u]);}
        if role==3u {descriptor=texture_words[record+20u];size=vec2<u32>(texture_words[record+21u],texture_words[record+22u]);}
        if descriptor==0u {continue;}
        // Only newly imported maps yield to explicit scalar face appearances.
        // Existing built-in/temporary roles retain their previous precedence,
        // including a request that mixes legacy and portable roles.
        if scalar_face && texture_words[descriptor+2u]!=0u {continue;}
        let rgb=texture_sample(descriptor,size,uv,gradients);
        if role==0u {
            let replace=(texture_words[record+19u]&2u)!=0u;
            mapped_color=select(color*rgb,rgb,replace);
        } else if role==1u {mapped_normal=vec4<f32>(rgb,1.0);}
        // Scalars are linear data. Alpha, IOR, coat and medium identity survive.
        else if role==2u {mapped_roughness=clamp(rgb.x,0.045,1.0);}
        else {mapped_metallic=clamp(rgb.x,0.0,1.0);}
    }
    return TextureSurface(mapped_color,mapped_metallic,mapped_roughness,mapped_normal);
}
fn texture_normal(record:u32,sampled:vec4<f32>,n:vec3<f32>,geometric:vec3<f32>)->vec3<f32> {
    if record==0u || sampled.w==0.0 {return n;}
    let rgb=sampled.xyz;
    let u=unit(texture_vector(record+4u));
    let tangent=u-n*dot(n,u);
    if dot(tangent,tangent)<1e-12 {return n;}
    let tangent_unit=unit(tangent);
    let handedness=select(1.0,-1.0,dot(n,cross(texture_vector(record+4u),texture_vector(record+8u)))<0.0);
    let strength=bitcast<f32>(texture_words[record+15u]);
    let local=vec3<f32>((2.0*rgb.xy-vec2<f32>(1.0))*strength,2.0*rgb.z-1.0);
    if dot(local,local)<1e-12 || local.z<=0.0 {return n;}
    let mapped=unit(tangent_unit*local.x+cross(n,tangent_unit)*(handedness*local.y)+n*local.z);
    return select(mapped,n,dot(mapped,geometric)<=0.0);
}
@compute @workgroup_size(1)
fn texture_reference() {
    let t=triangle(0u);
    let record=texture_record(t.optics.w);
    let gradients=TextureGradients(vec2<f32>(0.0),vec2<f32>(0.0));
    let mapped=texture_surface(record,t.base.xyz,t.parameters.x,t.parameters.y,u.eye.xyz,gradients,(u32(t.interior.z)&2u)!=0u);
    accumulation[0]=vec4<f32>(mapped.color,1.0);
    accumulation[1]=vec4<f32>(texture_normal(record,mapped.normal,u.up.xyz,u.back.xyz)+vec3<f32>(2.0),1.0);
}

@compute @workgroup_size(1)
fn texture_scalar_reference() {
    let t=triangle(0u);
    let mapped=texture_surface(texture_record(t.optics.w),t.base.xyz,t.parameters.x,t.parameters.y,u.eye.xyz,TextureGradients(vec2<f32>(0.0),vec2<f32>(0.0)),(u32(t.interior.z)&2u)!=0u);
    accumulation[0]=vec4<f32>(vec3<f32>(mapped.metallic,mapped.roughness,t.parameters.z),1.0);
}

// Validation invokes the production trilinear sampler with explicit unwrapped
// gradients, independently of camera inference.
@compute @workgroup_size(1)
fn texture_mip_reference() {
    let t=triangle(0u);let record=texture_record(t.optics.w);
    let gradients=TextureGradients(u.right.xy,u.up.xy);
    let mapped=texture_surface(record,t.base.xyz,t.parameters.x,t.parameters.y,u.eye.xyz,gradients,(u32(t.interior.z)&2u)!=0u);
    accumulation[0]=vec4<f32>(mapped.color,1.0);
    let last=texture_words[texture_words[record+3u]]-1u;
    var lod=0.0;
    if last>0u {lod=texture_lod(vec2<u32>(texture_words[record+7u],texture_words[record+11u]),gradients,last);}
    accumulation[1]=vec4<f32>(lod,0.0,0.0,1.0);
}
