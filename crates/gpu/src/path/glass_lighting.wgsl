// Legacy pipelines specialize this entire optical-lighting arc away. The lazy
// rough/HG pipeline uses the same six storage buffers as ordinary images.
override GLASS_DELTA_LIGHTS: bool=false;
// Bit zero of the aligned material offset marks a non-identity optical model.
// HG alone needs delta-light connections while keeping smooth shading normals.
fn glass_geometric_normals()->bool {return u.volume_albedo.w>0.0 || photon_enabled();}
fn glass_vacuum()->Medium {return Medium(0.0,1.0,vec3<f32>(0.0),vec2<f32>(0.0));}
fn glass_set_medium(m:Medium) {
    transport_medium_id=m.id;transport_medium_ior=m.ior;
    transport_medium_sigma=m.sigma;transport_medium_scatter=m.scatter.x;
}
fn glass_reflection_controls(model: vec4<f32>, cosine: f32, ni: f32, nt: f32, wavelength: f32) -> vec3<f32> {
    let bare=dielectric(cosine,ni,nt);
    if model.z<=0.0 {return vec3<f32>((bare.x+bare.y)*0.5);}
    if wavelength>0.0 {
        let f=glass_film(cosine,ni,nt,model.w,model.z,wavelength);
        return vec3<f32>((f.x+f.y)*0.5);
    }
    var result=vec3<f32>(0.0);
    for(var channel=0u;channel<3u;channel+=1u) {
        let f=glass_film(cosine,ni,nt,model.w,model.z,glass_wavelength(channel));
        result[channel]=(f.x+f.y)*0.5;
    }
    return result;
}
// Positive arithmetic is checked before divisions/products can generate an
// infinity. Invalid interface response is a scene precision refusal, distinct
// from the existing policy that omits an unbounded punctual source sample.
fn glass_response_product(a:f32,b:f32)->f32 {
    if !(a>=0.0 && a<=3.4e38 && b>=0.0 && b<=3.4e38) || a>3.4e38/max(b,1.0) {atomicOr(&failures[7],16u);return 0.0;}
    return a*b;
}
fn glass_response_ratio(numerator:f32,scale:f32,denominator:f32)->f32 {
    if numerator==0.0 || scale==0.0 {return 0.0;}
    if !(numerator>0.0 && numerator<=3.4e38 && scale>0.0 && scale<=3.4e38 && denominator>=1.17549435e-38 && denominator<=3.4e38) {atomicOr(&failures[7],16u);return 0.0;}
    // Multiply a reducing factor first; divide by a reducing denominator first.
    // Every rejected overflow would also overflow the complete positive ratio.
    var value=numerator;
    if scale<=1.0 {value*=scale;}
    if value>3.4e38*min(denominator,1.0) {atomicOr(&failures[7],16u);return 0.0;}
    value/=max(denominator,1.17549435e-38);
    if scale>1.0 {value=glass_response_product(value,scale);}
    return value;
}
fn glass_evaluate_cosine(model:vec4<f32>,incident:vec3<f32>,outgoing:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,ni:f32,nt:f32,wavelength:f32)->vec3<f32> {
    let view=-incident;let cv=dot(view,normal);let cl=dot(outgoing,normal);
    if !(model.x>0.0 && !(ni==nt && (model.z<=0.0 || model.w==ni))) || cv<=0.0 || cl==0.0 || cl*dot(outgoing,geometric)<=0.0 || (cl<0.0 && ni==nt) {return vec3<f32>(0.0);}
    var sum=view+outgoing;
    if cl<0.0 {sum=view*ni+outgoing*nt;}
    if dot(sum,sum)==0.0 {return vec3<f32>(0.0);}
    var facet=unit(sum);if dot(facet,normal)<0.0 {facet=-facet;}
    let vm=dot(view,facet);let lm=dot(outgoing,facet);
    if vm<=0.0 || (cl>0.0 && lm<=0.0) || (cl<0.0 && lm>=0.0) {return vec3<f32>(0.0);}
    let alpha=model.x*model.x;let a2=alpha*alpha;
    let cm=clamp(dot(facet,normal),0.0,1.0);
    let sine=cross(facet,normal);
    let denominator=dot(sine,sine)+a2*cm*cm;
    let distribution_root=(alpha/denominator)/sqrt(PI);
    let distribution=distribution_root*distribution_root;
    let root=sqrt(cv*cv+a2*max(0.0,1.0-cv*cv));
    let g2_over_cv=(2.0/(cv+root))*glass_smith_weight(cv,cl,alpha);
    let fresnel=glass_reflection_controls(model,clamp(vm,0.0,1.0),ni,nt,wavelength);
    var coefficient=fresnel;var scale=g2_over_cv*0.25;var divisor=1.0;
    if cl<0.0 {
        let jacobian_denominator=ni*vm+nt*lm;
        if jacobian_denominator==0.0 {return vec3<f32>(0.0);}
        coefficient=vec3<f32>(1.0)-fresnel;
        scale=g2_over_cv*abs(vm*lm)*ni*ni;
        divisor=jacobian_denominator*jacobian_denominator;
    }
    var response=vec3<f32>(0.0);
    for(var channel=0u;channel<3u;channel+=1u) {response[channel]=glass_response_ratio(glass_response_product(coefficient[channel],distribution),scale,divisor);}
    return response;
}
fn glass_hg_phase(cosine:f32,g:f32)->f32 {
    let denominator=1.0+g*g-2.0*g*clamp(cosine,-1.0,1.0);
    return (1.0-g*g)/(4.0*PI*denominator*sqrt(denominator));
}
fn glass_importance_normal(incident:vec3<f32>,outgoing:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>)->f32 {
    let numerator=abs(dot(-incident,normal))*abs(dot(outgoing,geometric));
    let denominator=abs(dot(-incident,geometric))*abs(dot(outgoing,normal));
    if denominator==0.0 {return 0.0;}
    let correction=numerator/denominator;
    if !(correction>=0.0) || !(correction<=3.4e38) {atomicOr(&failures[7],1u);return 0.0;}
    return correction;
}
// One uniformly retained hard-light vertex keeps native optical visibility at
// one call site with96 bytes of private payload. For K<=16 eligible camera
// vertices, K*prefix[J]*direct[J] estimates their complete sum. Selection uses
// its own RNG, leaving transport draws unchanged. Null/depth exits retain the
// estimator; multi-vertex map-off paths acquire additional sampling variance.
// mode0 closed; mode1 HG; mode2 first sheet reflection; mode3 internal exit;
// mode4 closed reflection only after an opaque straight-shadow replaced prefix.
// Raw lanes avoid composite array copies across function pointers and preserve
// point/prefix bits independently of floating-point optimization in the walk.
fn glass_offer_light(lights:ptr<function,array<vec4<u32>,6>>,count:ptr<function,u32>,selection_rng:ptr<function,u32>,point:PrecisePoint,incident:vec3<f32>,prefix:vec3<f32>,ni:f32,nt:f32,source_triangle:u32,mode:u32,medium:Medium) {
    let studio=u.environment.w>0.0;
    if select(3u,u32(u.light_count.x)+u32(u.light_count.y),studio)==0u {return;}
    let index=*count;
    if index>=16u || index>=u.sampling.y {atomicOr(&failures[7],8u);return;}
    *count=index+1u;
    if index>0u && random(selection_rng)>=1.0/f32(index+1u) {return;}
    (*lights)[0]=vec4<u32>(bitcast<vec3<u32>>(point.hi),source_triangle);
    (*lights)[1]=vec4<u32>(bitcast<vec3<u32>>(point.lo),mode);
    (*lights)[2]=bitcast<vec4<u32>>(vec4<f32>(incident,ni));
    (*lights)[3]=bitcast<vec4<u32>>(vec4<f32>(prefix,nt));
    (*lights)[4]=bitcast<vec4<u32>>(vec4<f32>(medium.sigma,medium.scatter.x));
    (*lights)[5]=bitcast<vec4<u32>>(vec4<f32>(medium.id,medium.ior,medium.scatter.y,0.0));
}
// Eight vec4 lanes per path in one bounded dispatch tile. The first six
// contain the selected request, then count/wavelength and the camera path value.
// Film planes precede this scratch and retain their historical offsets. Integer
// metadata uses exact numeric f32 encoding rather than subnormal bit patterns.
fn glass_tile_address(pixel:u32)->u32 {
    return u.image.x*u.image.y*select(1u,3u,u.adaptive_budget.x>0u)+(pixel-u.image.w)*8u;
}
fn glass_capture_light(lights:ptr<function,array<vec4<u32>,6>>,count:u32,wavelength:f32,pixel:u32) {
    let address=glass_tile_address(pixel);
    if count>0u {
        for(var lane=0u;lane<6u;lane+=1u) {accumulation[address+lane]=bitcast<vec4<f32>>((*lights)[lane]);}
        accumulation[address].w=f32(select((*lights)[0].w,0u,(*lights)[1].w==1u));accumulation[address+1u].w=f32((*lights)[1].w);
    }
    accumulation[address+6u]=vec4<f32>(f32(count),wavelength,0.0,0.0);
}
fn glass_resolve_light(lights:ptr<function,array<vec4<u32>,6>>,count:u32,wavelength:f32)->vec3<f32> {
    if !GLASS_DELTA_LIGHTS || count==0u {return vec3<f32>(0.0);}
    let point=(*lights)[0];let low=(*lights)[1];let mode=low.w;
    let incident=bitcast<vec4<f32>>((*lights)[2]);let prefix=bitcast<vec4<f32>>((*lights)[3]);
    let medium_sigma=bitcast<vec4<f32>>((*lights)[4]);let medium_other=bitcast<vec4<f32>>((*lights)[5]);
    var model=vec4<f32>(0.0);var normal=vec3<f32>(0.0);var geometric=vec3<f32>(0.0);
    var current=glass_vacuum();var next=glass_vacuum();var source_triangle=0xffffffffu;
    var escape=0;
    if mode==1u {
        current=Medium(medium_other.x,medium_other.y,medium_sigma.xyz,vec2<f32>(medium_sigma.w,medium_other.z));
        next=current;
    } else {
        source_triangle=point.w;let t=triangle(min(source_triangle,max(geometry[0],1u)-1u));model=t.glass;
        let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
        let entering=dot(outward,-incident.xyz)>=0.0;
        geometric=select(-outward,outward,entering);normal=geometric;
        if mode==0u || mode==4u {
            var parent=glass_vacuum();
            if t.interior.w>0.0 {
                let p=triangle(max(u32(t.interior.w),1u)-1u);
                parent=Medium(p.optics.w,glass_ior(p.parameters.z,p.glass.y,wavelength),p.optics.xyz,p.interior.xy);
            }
            let own=Medium(t.optics.w,glass_ior(t.parameters.z,t.glass.y,wavelength),t.optics.xyz,t.interior.xy);
            current=own;next=parent;if entering {current=parent;next=own;}
            current.ior=incident.w;next.ior=prefix.w;
            if mode==4u {escape=1;}
        } else {escape=select(1,-1,mode==3u);}
    }
    let physical=PrecisePoint(bitcast<vec3<f32>>(point.xyz),bitcast<vec3<f32>>(low.xyz));
    let studio=u.environment.w>0.0;
    let directions=select(3u,u32(u.light_count.x),studio);
    let source_count=directions+select(0u,u32(u.light_count.y),studio);
    var result=vec3<f32>(0.0);
    let saved_id=transport_medium_id;let saved_ior=transport_medium_ior;
    let saved_sigma=transport_medium_sigma;let saved_scatter=transport_medium_scatter;
    let saved_source=transport_source_triangle;let saved_boundary=transport_boundary_connection;
    let saved_origin=transport_connection_origin;let saved_low=transport_connection_origin_low;
    transport_connection_origin=physical.hi;transport_connection_origin_low=physical.lo;
    transport_source_triangle=source_triangle;transport_boundary_connection=mode!=1u;
    for(var source=0u;source<source_count;source+=1u) {
        var direction=FIXED_LIGHTS[min(source,2u)].xyz;
        var radiance=vec3<f32>(FIXED_LIGHTS[min(source,2u)].w*u.lighting.z*u.studio_weights[min(source,2u)]);
        var distance=FAR;
        if source<directions {
            if studio {let light=u.directional_lights[min(source,7u)];direction=light.direction.xyz;radiance=light.color.xyz*light.direction.w;}
        } else {
            let point_index=min(source-min(source,directions),7u);
            let sample=punctual_sample(u.punctual_lights[point_index],physical);
            if sample.omitted!=0u {continue;}
            direction=sample.direction;radiance=sample.radiance;distance=sample.distance;
        }
        if !any(radiance>vec3<f32>(0.0)) {continue;}
        var response=vec3<f32>(0.0);var connection_medium=current;
        if mode==1u {response=vec3<f32>(glass_hg_phase(dot(incident.xyz,direction),current.scatter.y));}
        else {
            let cosine=dot(normal,direction);
            if (escape==1 && cosine<=0.0) || (escape == -1 && cosine>=0.0) {continue;}
            response=glass_evaluate_cosine(model,incident.xyz,direction,normal,geometric,incident.w,prefix.w,wavelength);
            if cosine<0.0 {connection_medium=next;}
        }
        if !all(response>=vec3<f32>(0.0)) || !all(response<=vec3<f32>(3.4e38)) {atomicOr(&failures[7],16u);continue;}
        if !any(response>vec3<f32>(0.0)) {continue;}
        var contribution=vec3<f32>(0.0);
        for(var channel=0u;channel<3u;channel+=1u) {contribution[channel]=glass_response_product(response[channel],radiance[channel]);}
        glass_set_medium(connection_medium);
        let endpoint=select(FAR,punctual_endpoint(distance),source>=directions);
        if emitter_hit_precise(physical.hi,physical.lo,direction,0.0).distance<endpoint {continue;}
        let visibility=shadow_transmittance(physical.hi,physical.lo,direction,endpoint,0.0);
        let atmosphere=volume_transmittance_precise(physical.hi,physical.lo,direction,distance);
        result+=contribution*visibility.transmittance*atmosphere;
    }
    transport_medium_id=saved_id;transport_medium_ior=saved_ior;
    transport_medium_sigma=saved_sigma;transport_medium_scatter=saved_scatter;
    transport_source_triangle=saved_source;transport_boundary_connection=saved_boundary;
    transport_connection_origin=saved_origin;transport_connection_origin_low=saved_low;
    return prefix.xyz*result*f32(count);
}
struct GlassRadiantSheet {walk:GlassSheetWalk,direct:vec3<f32>,continuous:bool};
fn glass_camera_sheet(t:Triangle,point:PrecisePoint,source_triangle:u32,incident:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,outside:f32,inside:f32,wavelength:f32,remaining:u32,stratum:u32,strata:u32,prior_continuous:bool,prefix:vec3<f32>,lights:ptr<function,array<vec4<u32>,6>>,light_count:ptr<function,u32>,selection_rng:ptr<function,u32>,rng:ptr<function,u32>)->GlassRadiantSheet {
    var direction=incident;var weight=vec3<f32>(1.0);var internal=false;
    var direct=vec3<f32>(0.0);var continuous=prior_continuous;
    let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
    for(var event=0u;event<remaining;event+=1u) {
        let macro_normal=select(-normal,normal,dot(direction,normal)<0.0);
        let ni=select(outside,inside,internal);let nt=select(inside,outside,internal);
        let actual_geometric=select(-geometric,geometric,dot(direction,normal)<0.0);
        let face_outward=select(actual_geometric,-actual_geometric,internal);
        let side=select(-1,1,dot(face_outward,outward)>0.0);
        if !photon_enabled() || !continuous {
            glass_offer_light(lights,light_count,selection_rng,point,direction,prefix*weight,ni,nt,source_triangle,select(2u,3u,internal),glass_vacuum());
            direct+=weight*photon_gather_glass(t,point.hi+point.lo,macro_normal,actual_geometric,direction,ni,nt,wavelength,side);
        }
        let sampled=glass_interface(t,direction,macro_normal,actual_geometric,ni,nt,wavelength,true,stratum,select(1u,strata,event==0u),rng);
        if !sampled.valid {return GlassRadiantSheet(GlassSheetWalk(sampled.direction,vec3<f32>(0.0),event+1u,false),direct,continuous);}
        if glass_rough_active(t,ni,nt) && !(sampled.transmitted && ni==nt) {continuous=true;}
        direction=sampled.direction;weight*=sampled.weight;
        if (!internal && !sampled.transmitted) || (internal && sampled.transmitted) {
            let valid=dot(direction,normal)*dot(direction,geometric)>0.0;
            return GlassRadiantSheet(GlassSheetWalk(direction,weight,event+1u,valid),direct,continuous);
        }
        internal=true;
        let cosine=abs(dot(direction,normal));
        if cosine==0.0 {return GlassRadiantSheet(GlassSheetWalk(direction,vec3<f32>(0.0),event+1u,false),direct,continuous);}
        if t.parameters.w>0.0 {weight*=exp(-t.optics.xyz*(t.parameters.w/cosine));}
        if !any(weight>vec3<f32>(0.0)) {return GlassRadiantSheet(GlassSheetWalk(direction,weight,event+1u,false),direct,continuous);}
    }
    return GlassRadiantSheet(GlassSheetWalk(direction,vec3<f32>(0.0),remaining,false),direct,continuous);
}
