// Deterministic display guides use the production intersections and resolved
// appearance without changing the path random stream or failure counters.
struct GuideLook {
    albedo: vec3<f32>, normal: vec3<f32>, metallic: f32, roughness: f32,
};
struct GuideRay {
    point: PrecisePoint, direction: vec3<f32>, excluded: u32, minimum: f32,
};
struct GuideInterface {
    reflected: vec3<f32>, transmitted: vec3<f32>, fresnel: vec3<f32>,
};
const GUIDE_MISS: u32 = 0xffffffffu;

fn guide_outward(t:Triangle)->vec3<f32> {
    return unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
}
fn guide_normal(t:Triangle,bary:vec3<f32>,geometric:vec3<f32>,direction:vec3<f32>)->vec3<f32> {
    let interpolated=unit(t.n0.xyz*bary.x+t.n1.xyz*bary.y+t.n2.xyz*bary.z);
    let winding=select(interpolated,-interpolated,dot(interpolated,geometric)<0.0);
    return select(geometric,winding,dot(winding,-direction)>0.0);
}
fn guide_look(t:Triangle,hit:Hit,direction:vec3<f32>,position:vec3<f32>,travel:f32,bounce:u32)->GuideLook {
    let outward=guide_outward(t);
    let geometric=select(-outward,outward,dot(outward,-direction)>=0.0);
    let record=texture_record(t.optics.w);
    let gradients=texture_gradients(record,geometric,direction,hit.distance,travel,bounce);
    let mapped=texture_surface(record,t.base.xyz,t.parameters.x,t.parameters.y,position,gradients,(u32(t.interior.z)&2u)!=0u);
    var look=t;
    look.base=vec4<f32>(mapped.color,t.base.w);
    look.parameters=vec4<f32>(mapped.metallic,mapped.roughness,t.parameters.zw);
    look=aged_surface(look,position,outward);
    let normal=guide_normal(t,hit.bary,geometric,direction);
    let perturbed=texture_normal(record,mapped.normal,normal,geometric);
    return GuideLook(look.base.xyz,select(normal,perturbed,dot(perturbed,-direction)>0.0),look.parameters.x,look.parameters.y);
}
fn guide_sharp(t:Triangle,look:GuideLook)->bool {
    return (look.metallic>0.5 && look.roughness<0.25) || (t.coat.x>0.0 && t.coat.y<0.05);
}
fn guide_depth(position:vec3<f32>)->f32 {return dot(position-u.eye.xyz,-u.back.xyz);}
fn guide_footprint(depth:f32)->f32 {
    var footprint=2.0*length(u.up.xyz)/f32(u.image.y);
    if u.eye.w>0.5 {footprint*=max(depth,0.0)/u.back.w;}
    return max(footprint,0.000001);
}
fn guide_write(base:u32,id:f32,look:GuideLook,position:vec3<f32>,primary_footprint:f32) {
    let depth=guide_depth(position);
    accumulation[base]=vec4<f32>(look.albedo,id);
    accumulation[base+1u]=vec4<f32>(look.normal,depth);
    accumulation[base+2u]=vec4<f32>(position,max(primary_footprint,guide_footprint(depth)));
}
fn guide_vacuum_origin(ray:CameraRay)->bool {
    if u.lighting.w==0.0 {return true;}
    let boundary=origin_boundary(ray.origin,ray.direction);
    if !boundary.valid {return false;}
    if boundary.hit.triangle==GUIDE_MISS {return true;}
    let t=triangle(boundary.hit.triangle);
    return t.interior.w==0.0 && dot(guide_outward(t),ray.direction)<0.0;
}
fn guide_smooth_glass(t:Triangle)->bool {
    return t.base.w>0.0 && all(t.glass==vec4<f32>(0.0)) && t.interior.x==0.0 && t.interior.w==0.0;
}
fn guide_interface(t:Triangle,bary:vec3<f32>,direction:vec3<f32>,geometric:vec3<f32>,ni:f32,nt:f32)->GuideInterface {
    var normal=geometric;
    // The guide-only sampling.w bit zero mirrors photon geometric normals.
    if u.volume_albedo.w==0.0 && (u.sampling.w&1u)==0u {normal=guide_normal(t,bary,geometric,direction);}
    var fresnel=dielectric(clamp(dot(normal,-direction),0.0,1.0),ni,nt);
    var transmitted=refract(direction,normal,ni/nt);
    if (fresnel.z>=0.0 && dot(transmitted,geometric)>=0.0) || dot(reflect(direction,normal),geometric)<=0.0 {
        normal=geometric;
        fresnel=dielectric(clamp(dot(normal,-direction),0.0,1.0),ni,nt);
        transmitted=refract(direction,normal,ni/nt);
    }
    // Keep a numerical zero from Snell so a degenerate branch is rejected.
    if any(transmitted!=vec3<f32>(0.0)) {transmitted=unit(transmitted);}
    return GuideInterface(reflect(direction,normal),transmitted,fresnel);
}
fn guide_launch(t:Triangle,hit:Hit,geometric:vec3<f32>,direction:vec3<f32>)->GuideRay {
    if t.base.w>0.0 && t.parameters.w<0.0 {
        // Closed glass continues on the precise boundary, suppressing only the
        // producing triangle and keeping strictly positive adjacent crossings.
        return GuideRay(surface_point_precise(t,hit.bary),direction,hit.triangle,0.0);
    }
    let shift=select(geometric,direction,dot(direction,geometric)<0.0)*(2.0*u.lighting.x);
    if t.base.w>0.0 && (u.sampling.w&2u)!=0u {
        // Optical resolve uses a split launch for thin sheets.
        let point=surface_point_precise(t,hit.bary);
        return GuideRay(PrecisePoint(point.hi,point.lo+shift),direction,GUIDE_MISS,u.lighting.x);
    }
    return GuideRay(PrecisePoint(surface_point(t,hit.bary)+shift,vec3<f32>(0.0)),direction,GUIDE_MISS,u.lighting.x);
}

@compute @workgroup_size(64)
fn denoise_guides(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pixel=u.image.w+gid.x;
    if gid.x>=u.sampling.z || pixel>=u.image.x*u.image.y { return; }
    let base=pixel*6u;
    accumulation[base]=vec4<f32>(0.0);
    accumulation[base+1u]=vec4<f32>(0.0);
    accumulation[base+2u]=vec4<f32>(0.0);
    accumulation[base+3u]=vec4<f32>(0.0);
    accumulation[base+4u]=vec4<f32>(0.0);
    accumulation[base+5u]=vec4<f32>(0.0);
    let xy=vec2<f32>(f32(pixel%u.image.x)+0.5,f32(pixel/u.image.x)+0.5);
    let uv=vec2<f32>(xy.x/f32(u.image.x)*2.0-1.0,1.0-xy.y/f32(u.image.y)*2.0);
    let camera=primary_ray(uv,vec2<f32>(0.0));
    let primary=trace(camera.origin,camera.direction,false);
    if primary.triangle==GUIDE_MISS || (u.environment.w>0.0 && emitter_hit_bounded(camera.origin,camera.direction,u.lighting.x).distance<primary.distance) {return;}
    let t=triangle(primary.triangle);
    let position=surface_point(t,primary.bary);
    let outward=guide_outward(t);
    let entering=dot(outward,-camera.direction)>0.0;
    let geometric=select(-outward,outward,entering);
    let primary_footprint=guide_footprint(guide_depth(position));
    let primary_travel=length(position-camera.origin);
    let closed=t.base.w>0.0 && t.parameters.w<0.0;
    var reflected=vec3<f32>(0.0);
    var transmitted=vec3<f32>(0.0);
    var has_reflection=false;
    var has_transmission=false;
    if t.base.w<=0.0 {
        let look=guide_look(t,primary,camera.direction,position,primary_travel,0u);
        if !guide_sharp(t,look) {guide_write(base,t.optics.w,look,position,primary_footprint);return;}
        if look.metallic<0.99 || look.roughness>0.05 || t.coat.x>0.0 {return;}
        reflected=reflect(camera.direction,look.normal);
        has_reflection=true;
    } else {
        if !guide_smooth_glass(t) || (closed && !entering) {return;}
        let bend=guide_interface(t,primary.bary,camera.direction,geometric,1.0,t.parameters.z);
        reflected=bend.reflected;
        let f=bend.fresnel;
        has_reflection=max(f.x,f.y)>0.0;
        has_transmission=f.z>=0.0;
        transmitted=bend.transmitted;
        if !closed && has_transmission {
            transmitted=camera.direction;
            // Use the same sheet weights without calling the transport helper
            // that can write a failure counter on a tangent absorbing sheet.
            if t.parameters.w>0.0 && !(f.z>0.0) {return;}
            var attenuation=vec3<f32>(1.0);
            if t.parameters.w>0.0 {attenuation=exp(-t.optics.xyz*(t.parameters.w/f.z));}
            has_transmission=any(sheet_polarization(f.x,attenuation).transmission+sheet_polarization(f.y,attenuation).transmission>vec3<f32>(0.0));
        }
    }
    if t.optics.w<1.0 || t.optics.w>511.0 || (u.eye.w>0.5 && u.right.w>0.0) || u.volume_min.w>0.0 {return;}
    if u.sampling.y<2u || (closed && has_transmission && u.sampling.y<3u) || (!has_reflection && !has_transmission) {return;}
    if (has_reflection && dot(reflected,geometric)<=0.000001) || (has_transmission && dot(transmitted,geometric)>=-0.000001) {return;}
    // A certified root closed entry proves vacuum along this primary segment.
    // A mirror or sheet can be hit first even when a nested camera starts inside glass.
    if !closed && !guide_vacuum_origin(camera) {return;}

    var reflected_body=0u;
    var transmitted_body=0u;
    var reflected_look:GuideLook;
    var reflected_position=vec3<f32>(0.0);
    // Stage 0 is the reflection endpoint, 1 is the same closed body's exit,
    // and 2 is the forward endpoint. One call site bounds the extra BVH work.
    let forward_stage=select(2u,1u,closed);
    var stage=select(forward_stage,0u,has_reflection);
    var ray=guide_launch(t,primary,geometric,select(transmitted,reflected,has_reflection));
    var travel=primary_travel;
    var bounce=1u;
    for (var event=0u;event<3u;event+=1u) {
        let hit=trace_precise(ray.point.hi,ray.point.lo,ray.direction,false,ray.excluded,ray.minimum,stage==1u);
        if hit.triangle==GUIDE_MISS || (u.environment.w>0.0 && emitter_hit_precise(ray.point.hi,ray.point.lo,ray.direction,ray.minimum).distance<hit.distance) {return;}
        let next=triangle(hit.triangle);
        travel+=hit.distance;
        if stage==1u {
            let normal=-guide_outward(next);
            if next.optics.w!=t.optics.w || !guide_smooth_glass(next) || next.parameters.w>=0.0 || dot(normal,ray.direction)>=-0.000001 {return;}
            let exit=guide_interface(next,hit.bary,ray.direction,normal,t.parameters.z,1.0);
            if exit.fresnel.z<0.0 || dot(exit.transmitted,normal)>=-0.000001 {return;}
            ray=guide_launch(next,hit,normal,exit.transmitted);
            stage=2u;
            bounce=2u;
            continue;
        }
        if next.base.w>0.0 || next.optics.w<1.0 || next.optics.w>511.0 {return;}
        let endpoint_position=surface_point(next,hit.bary);
        let endpoint_look=guide_look(next,hit,ray.direction,endpoint_position,travel,bounce);
        if guide_sharp(next,endpoint_look) {return;}
        if stage==0u {
            reflected_body=u32(next.optics.w);
            if has_transmission {
                reflected_look=endpoint_look;
                reflected_position=endpoint_position;
                ray=guide_launch(t,primary,geometric,transmitted);
                stage=forward_stage;
                travel=primary_travel;
                continue;
            }
        } else {transmitted_body=u32(next.optics.w);}
        let identity=0xa0000000u|(u32(t.optics.w)<<18u)|(reflected_body<<9u)|transmitted_body;
        guide_write(base,bitcast<f32>(identity),endpoint_look,endpoint_position,primary_footprint);
        if reflected_body!=0u && transmitted_body!=0u {
            guide_write(base+3u,f32(reflected_body),reflected_look,reflected_position,primary_footprint);
        }
        return;
    }
}
