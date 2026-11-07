// Bounded light tracing for delta sources and opt-in area and environment
// sources. The latter two own only paths that encounter one or more smooth
// closed-glass events before the first opaque receiver.
// Header 16 words, hash heads, then 16-word immutable photon records.
@group(0) @binding(7) var<storage,read_write> photon_data:array<atomic<u32>>;
fn photon_word(index:u32)->u32 {return atomicLoad(&photon_data[index]);}
fn photon_float(index:u32)->f32 {return bitcast<f32>(photon_word(index));}
fn photon_vector(index:u32)->vec3<f32> {return vec3<f32>(photon_float(index),photon_float(index+1u),photon_float(index+2u));}
fn photon_store_vector(index:u32,value:vec3<f32>) {
    atomicStore(&photon_data[index],bitcast<u32>(value.x));
    atomicStore(&photon_data[index+1u],bitcast<u32>(value.y));
    atomicStore(&photon_data[index+2u],bitcast<u32>(value.z));
}
fn photon_cell(position:vec3<f32>)->vec3<i32> {
    return vec3<i32>(floor((position-photon_vector(6u))/photon_float(2u)));
}
fn photon_hash(cell:vec3<i32>)->u32 {
    let words=bitcast<vec3<u32>>(cell);
    return ((words.x*73856093u)^(words.y*19349663u)^(words.z*83492791u))&(photon_word(1u)-1u);
}
fn photon_record(index:u32)->u32 {return 16u+photon_word(1u)+index*16u;}
fn photon_bad_medium() {atomicOr(&failures[7],4u);}
fn photon_deposit(index:u32,position:vec3<f32>,incoming:vec3<f32>,flux:vec3<f32>,normal:vec3<f32>,receiver:u32,kind:u32,side:i32,source:u32,smooth_events:u32) {
    let record=photon_record(index);
    photon_store_vector(record,position);photon_store_vector(record+4u,incoming);
    photon_store_vector(record+8u,flux);photon_store_vector(record+12u,normal);
    atomicStore(&photon_data[record+7u],receiver);atomicStore(&photon_data[record+11u],kind);
    let side_code=select(select(0u,1u,side>0),2u,side<0);
    // Two side bits, four hard source bits, then up to 63 smooth closed-glass events.
    atomicStore(&photon_data[record+15u],side_code|(source<<2u)|(smooth_events<<6u));
    let previous=atomicExchange(&photon_data[16u+photon_hash(photon_cell(position))],index);
    atomicStore(&photon_data[record+3u],previous);
}
// One uniformly selected eligible receiver per emitted path. Final multiplication
// by the bounded visit count preserves every receiver's expected power without
// allocating photons*depth records. The separate RNG never changes transport.
struct PhotonReservoir {
    visits:u32,rng:u32,position:vec3<f32>,incoming:vec3<f32>,flux:vec3<f32>,normal:vec3<f32>,receiver:u32,kind:u32,side:i32,
    source:vec3<f32>,source_low:vec3<f32>,minimum:f32,source_index:u32,smooth_events:u32,
};
fn photon_offer(reservoir:ptr<function,PhotonReservoir>,point:PrecisePoint,incoming:vec3<f32>,flux:vec3<f32>,normal:vec3<f32>,receiver:u32,kind:u32,side:i32) {
    if (*reservoir).minimum>0.0 && length(precise_point_delta(point.hi,point.lo,(*reservoir).source,(*reservoir).source_low))<(*reservoir).minimum {punctual_count(0u);return;}
    if !all(flux>=vec3<f32>(0.0)) || !all(flux<=vec3<f32>(3.4e38)) {atomicOr(&failures[7],1u);return;}
    if !any(flux>vec3<f32>(0.0)) {return;}
    (*reservoir).visits+=1u;
    let count=(*reservoir).visits;
    // Naga30's SPIR-V writer cannot cache a pointer-to-field call argument
    // introduced in a short-circuit branch. Keep the same draws in a local RNG.
    var selected=count==1u;
    if count>1u {
        var selection_rng=(*reservoir).rng;
        selected=random(&selection_rng)<1.0/f32(count);
        (*reservoir).rng=selection_rng;
    }
    if selected {
        (*reservoir).position=point.hi+point.lo;(*reservoir).incoming=incoming;(*reservoir).flux=flux;
        (*reservoir).normal=normal;(*reservoir).receiver=receiver;(*reservoir).kind=kind;(*reservoir).side=side;
    }
}
@compute @workgroup_size(32)
fn photon_clear(@builtin(global_invocation_id) invocation:vec3<u32>) {
    if invocation.x>=u.sampling.z {return;}
    let index=photon_word(5u)+invocation.x;let heads=photon_word(1u);
    if index<heads {atomicStore(&photon_data[16u+index],0xffffffffu);}
    else if index<heads+photon_word(0u) {atomicStore(&photon_data[photon_record(index-heads)+11u],0u);}
}
fn photon_unit_sphere(rng:ptr<function,u32>)->vec3<f32> {
    let z=1.0-2.0*random(rng);let phi=2.0*PI*random(rng);let r=sqrt(max(0.0,1.0-z*z));
    return vec3<f32>(r*cos(phi),r*sin(phi),z);
}
// Project a parallel light bundle onto the disk enclosing the captured scene.
// w is its area, or zero if the captured bounds have no positive radius.
fn photon_project(direction:vec3<f32>,rng:ptr<function,u32>)->vec4<f32> {
    let center=(photon_vector(6u)+photon_vector(9u))*0.5;
    let radius=length(photon_vector(9u)-photon_vector(6u))*0.5;
    if !(radius>0.0) {return vec4<f32>(0.0);}
    let axis=select(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,1.0,0.0),abs(direction.z)>0.9);
    let x=unit(cross(axis,direction));let y=cross(direction,x);
    let r=radius*sqrt(random(rng));let phi=2.0*PI*random(rng);
    let origin=center-direction*(radius+max(u.lighting.x*2.0,radius*0.000001))+r*(cos(phi)*x+sin(phi)*y);
    return vec4<f32>(origin,PI*radius*radius);
}
@compute @workgroup_size(32)
fn photon_emit(@builtin(global_invocation_id) invocation:vec3<u32>) {
    if invocation.x>=u.sampling.z {return;}
    let index=photon_word(5u)+invocation.x;let count=photon_word(0u);
    if index>=count {return;}
    var reservoir:PhotonReservoir;
    reservoir.rng=index^(photon_word(4u)*0x85ebca6bu)^0x6b8b4567u;
    photon_trace(index,&reservoir);
    if reservoir.visits>0u {
        let flux=reservoir.flux*f32(reservoir.visits);
        if !all(flux>=vec3<f32>(0.0)) || !all(flux<=vec3<f32>(3.4e38)) {atomicOr(&failures[7],1u);return;}
        photon_deposit(index,reservoir.position,reservoir.incoming,flux,reservoir.normal,reservoir.receiver,reservoir.kind,reservoir.side,reservoir.source_index,reservoir.smooth_events);
    }
}
fn photon_trace(index:u32,reservoir:ptr<function,PhotonReservoir>) {
    let count=photon_word(0u);
    var rng=index^(photon_word(4u)*0x9e3779b9u)^0x8da6b343u;
    let studio=u.environment.w>0.0;
    let directions=select(3u,u32(u.light_count.x),studio);
    let punctual=select(0u,u32(u.light_count.y),studio);
    let hard_sources=directions+punctual;
    let maximum=photon_word(3u);
    let area_count=select(0u,u32(u.light_count.w),photon_word(12u)>0u && maximum>1u);
    let lit_environment=select(any(u.environment.xyz>vec3<f32>(0.0)),u.environment_map.z>0.0,u.environment_map.w>0.0);
    let environment_count=select(0u,1u,studio && lit_environment && photon_word(13u)>0u && maximum>1u);
    let sources=hard_sources+area_count+environment_count;
    if sources==0u {return;}
    let selected=min(sources-1u,u32(random(&rng)*f32(sources)));
    let area=selected>=hard_sources && selected<hard_sources+area_count;
    let continuous=selected>=hard_sources;
    let area_index=select(0xffffffffu,selected-hard_sources,area);
    (*reservoir).source_index=select(0u,selected-directions+1u,selected>=directions && !continuous);
    var origin=vec3<f32>(0.0);var origin_low=vec3<f32>(0.0);
    var direction=vec3<f32>(0.0,0.0,-1.0);var flux=vec3<f32>(0.0);
    var source_minimum=0.0;
    if selected<directions {
        var toward=FIXED_LIGHTS[min(selected,2u)].xyz;
        var irradiance=vec3<f32>(FIXED_LIGHTS[min(selected,2u)].w*u.lighting.z*u.studio_weights[min(selected,2u)]);
        if studio {let light=u.directional_lights[min(selected,7u)];toward=light.direction.xyz;irradiance=light.color.xyz*light.direction.w;}
        direction=-toward;
        let projected=photon_project(direction,&rng);
        if projected.w<=0.0 {return;}
        origin=projected.xyz;
        flux=irradiance*(projected.w*f32(sources)/f32(count));
    } else if !continuous {
        let light=u.punctual_lights[min(selected-min(selected,directions),7u)];
        origin=light.position.xyz;origin_low=light.position_low.xyz;source_minimum=light.position_low.w;
        var density=1.0/(4.0*PI);var cone=1.0;
        if light.cone.y>0.0 {
            let cosine=1.0-random(&rng)*(1.0-light.cone.x);let phi=2.0*PI*random(&rng);
            let sine=sqrt(max(0.0,1.0-cosine*cosine));
            direction=from_local(vec3<f32>(sine*cos(phi),sine*sin(phi),cosine),light.axis.xyz);
            density=1.0/(2.0*PI*(1.0-light.cone.x));
            let blend=clamp((cosine-light.cone.x)/(light.axis.w-light.cone.x),0.0,1.0);
            cone=blend*blend*(3.0-2.0*blend);
        } else {direction=photon_unit_sphere(&rng);}
        flux=light.color.xyz*(light.color.w*light.position.w*light.position.w*cone*f32(sources)/(density*f32(count)));
    } else if area {
        let light=u.area_lights[min(area_index,3u)];
        origin=light.center.xyz;
        origin_low=(2.0*random(&rng)-1.0)*light.u.xyz+(2.0*random(&rng)-1.0)*light.v.xyz;
        let normal=unit(cross(light.u.xyz,light.v.xyz));
        let radius_squared=random(&rng);let phi=2.0*PI*random(&rng);
        let radius=sqrt(radius_squared);
        direction=from_local(vec3<f32>(radius*cos(phi),radius*sin(phi),sqrt(1.0-radius_squared)),normal);
        // Uniform area and cosine solid-angle sampling cancel the emitted cosine.
        flux=light.radiance.xyz*(PI*light.center.w*f32(sources)/f32(count));
    } else {
        let sample=env_sample(&rng);
        if !(sample.pdf>0.0) || !any(sample.radiance>vec3<f32>(0.0)) {return;}
        direction=-sample.direction;
        let projected=photon_project(direction,&rng);
        if projected.w<=0.0 {return;}
        origin=projected.xyz;
        // The disk encloses scene geometry. Analytic rectangles can lie farther
        // toward the sampled sky, so test that part of the same line separately.
        if emitter_hit_precise(origin,origin_low,-direction,0.0).index!=0xffffffffu {return;}
        flux=sample.radiance*(projected.w*f32(sources)/(sample.pdf*f32(count)));
    }
    if !any(flux>vec3<f32>(0.0)) {return;}
    var wavelength=0.0;
    if (geometry[2]&0x40000000u)!=0u {
        let channel=min(2u,u32(random(&rng)*3.0));wavelength=glass_wavelength(channel);
        var mask=vec3<f32>(0.0);mask[channel]=3.0;flux*=mask;
    }
    transport_wavelength=wavelength;
    let source_position=origin;let source_position_low=origin_low;
    (*reservoir).source=source_position;(*reservoir).source_low=source_position_low;(*reservoir).minimum=source_minimum;
    var media:array<Medium,MEDIUM_CAPACITY>;var medium_count=0u;
    let first=origin_boundary(origin+origin_low,direction);
    if !first.valid {photon_bad_medium();return;}
    if first.hit.triangle!=0xffffffffu && !initialize_media_spectral(first.hit.triangle,direction,&media,&medium_count,wavelength) {photon_bad_medium();return;}
    var previous=origin;var previous_low=origin_low;var excluded=0xffffffffu;var minimum=0.0;var optical=false;
    for(var depth=0u;depth<maximum;depth+=1u) {
        let hit=trace_precise(origin,origin_low,direction,false,excluded,minimum,medium_count>0u);
        let emitter=emitter_hit_precise_except(origin,origin_low,direction,minimum,select(0xffffffffu,area_index,depth==0u));
        if medium_count>0u && media[medium_count-1u].scatter.x>0.0 {
            let current=media[medium_count-1u];let flight=-log(1.0-random(&rng))/current.scatter.x;
            if flight<min(hit.distance,emitter.distance) {
                if continuous {return;}
                let point=origin+origin_low+direction*flight;
                flux*=exp(-current.sigma*length(precise_point_delta(point,vec3<f32>(0.0),previous,previous_low)));
                if optical {photon_offer(reservoir,PrecisePoint(point,vec3<f32>(0.0)),-direction,flux,vec3<f32>(0.0),bitcast<u32>(current.id),3u,0);}
                direction=sample_hg(direction,current.scatter.y,&rng);
                // Rounded free-flight positions must remain in the same certified
                // chain. Reuse its existing stack; never move a point to fit it.
                let expected=medium_count;let expected_top=current.id;
                let collision=origin_boundary(point,direction);
                if !collision.valid || collision.hit.triangle==0xffffffffu {photon_bad_medium();return;}
                if !initialize_media_spectral(collision.hit.triangle,direction,&media,&medium_count,wavelength) {photon_bad_medium();return;}
                if medium_count!=expected || media[medium_count-1u].id!=expected_top {photon_bad_medium();return;}
                origin=point;origin_low=vec3<f32>(0.0);
                previous=point;previous_low=vec3<f32>(0.0);excluded=0xffffffffu;minimum=0.0;optical=true;continue;
            }
        }
        if emitter.distance<hit.distance {return;}
        if hit.triangle==0xffffffffu {if medium_count>0u {photon_bad_medium();}return;}
        let t=triangle(hit.triangle);let point=surface_point_precise(t,hit.bary);
        let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));let entering=dot(outward,direction)<0.0;
        let geometric=select(-outward,outward,entering);
        if medium_count>0u {flux*=exp(-media[medium_count-1u].sigma*length(precise_point_delta(point.hi,point.lo,previous,previous_low)));}
        if !all(flux>=vec3<f32>(0.0)) || !all(flux<=vec3<f32>(3.4e38)) {atomicOr(&failures[7],1u);return;}
        if !any(flux>vec3<f32>(0.0)) {return;}
        if t.base.w<=0.0 {
            if optical {
                if continuous {(*reservoir).smooth_events=depth;}
                photon_offer(reservoir,point,-direction,flux,geometric,hit.triangle,1u,0);
            }
            return;
        }
        if continuous && (t.parameters.w>=0.0 || t.glass.x>0.0) {return;}
        var normal=unit(t.n0.xyz*hit.bary.x+t.n1.xyz*hit.bary.y+t.n2.xyz*hit.bary.z);
        if dot(normal,geometric)<0.0 {normal=-normal;}
        if dot(normal,-direction)<=0.0 {normal=geometric;}
        // All light tracing belongs to the reciprocal physical-normal mode.
        normal=geometric;
        var ni=1.0;if medium_count>0u {ni=media[medium_count-1u].ior;}
        var nt=glass_ior(t.parameters.z,t.glass.y,wavelength);
        if t.parameters.w>=0.0 {
            if medium_count>0u {photon_bad_medium();return;}
            if glass_rough_active(t,ni,nt) {
                normal=glass_macro_normal(direction,normal,geometric,ni,nt);
                var internal=false;var escaped=false;
                for(var event=0u;event<maximum-depth;event+=1u) {
                    let macro_normal=select(-normal,normal,dot(direction,normal)<0.0);
                    let virtual_geometric=select(-geometric,geometric,dot(direction,normal)<0.0);
                    let face_outward=select(virtual_geometric,-virtual_geometric,internal);
                    let side=select(-1,1,dot(face_outward,outward)>0.0);
                    if optical {photon_offer(reservoir,point,-direction,flux,outward,hit.triangle,2u,side);}
                    let incident_ior=select(ni,nt,internal);let next_ior=select(nt,ni,internal);
                    let sampled=glass_interface(t,direction,macro_normal,virtual_geometric,incident_ior,next_ior,wavelength,false,0u,1u,&rng);
                    if !sampled.valid {return;}
                    flux*=sampled.weight;
                    direction=sampled.direction;optical=true;
                    if (!internal && !sampled.transmitted) || (internal && sampled.transmitted) {depth+=event;escaped=true;break;}
                    internal=true;let cosine=abs(dot(direction,normal));
                    if cosine==0.0 {return;}
                    if t.parameters.w>0.0 {flux*=exp(-t.optics.xyz*(t.parameters.w/cosine));}
                }
                if !escaped || dot(direction,normal)*dot(direction,geometric)<=0.0 {return;}
            } else {
                var f=dielectric(clamp(dot(normal,-direction),0.0,1.0),ni,nt);
                let bent=refract(direction,normal,ni/nt);
                if (f.z>=0.0 && dot(bent,geometric)>=0.0) || dot(reflect(direction,normal),geometric)<=0.0 {normal=geometric;f=dielectric(clamp(dot(normal,-direction),0.0,1.0),ni,nt);}
                let weights=glass_sheet(t,f,clamp(dot(normal,-direction),0.0,1.0),ni,nt,wavelength);
                let r=dot(weights.reflection,vec3<f32>(1.0/3.0));let tr=dot(weights.transmission,vec3<f32>(1.0/3.0));
                if r+tr<=0.0 {return;}let probability=r/(r+tr);
                if random(&rng)<probability {let outgoing=reflect(direction,normal);flux*=weights.reflection/probability;direction=outgoing;optical=true;}
                else {flux*=weights.transmission/(1.0-probability);optical=optical || t.coat.w<=0.0;}
            }
            origin=point.hi+point.lo+direction*(u.lighting.x*2.0);origin_low=vec3<f32>(0.0);excluded=0xffffffffu;minimum=u.lighting.x;
        } else {
            if entering {
                var parent=0.0;if t.interior.w>0.0 {parent=triangle(max(u32(t.interior.w),1u)-1u).optics.w;}
                var actual=0.0;if medium_count>0u {actual=media[medium_count-1u].id;}
                if parent!=actual {photon_bad_medium();return;}
            } else {
                if medium_count==0u || media[medium_count-1u].id!=t.optics.w {photon_bad_medium();return;}
                nt=1.0;if medium_count>1u {nt=media[medium_count-2u].ior;}
            }
            // Geometric-normal fallback matches smooth camera transport before
            // drawing a visible rough normal. Wrong-side rough samples are null.
            normal=glass_macro_normal(direction,normal,geometric,ni,nt);
            if optical && glass_rough_active(t,ni,nt) {photon_offer(reservoir,point,-direction,flux,outward,hit.triangle,2u,0);}
            let sampled=glass_interface(t,direction,normal,geometric,ni,nt,wavelength,false,0u,1u,&rng);
            if !sampled.valid {return;}
            flux*=sampled.weight;direction=sampled.direction;optical=true;
            if sampled.transmitted {
                if entering {
                    if medium_count>=MEDIUM_CAPACITY {photon_bad_medium();return;}
                    media[medium_count]=Medium(t.optics.w,nt,t.optics.xyz,t.interior.xy);medium_count+=1u;
                } else {medium_count-=1u;}
            }
            if abs(dot(direction,geometric))<1e-6 {return;}
            origin=point.hi;origin_low=point.lo;excluded=hit.triangle;minimum=0.0;
        }
        previous=point.hi;previous_low=point.lo;
    }
}
fn photon_gather(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,view:vec3<f32>,remaining:u32) -> vec3<f32> {
    let count=photon_word(0u);if count==0u || t.base.w>0.0 {return vec3<f32>(0.0);}
    let radius=photon_float(2u);let center=photon_cell(position);var result=vec3<f32>(0.0);
    for(var z=-1;z<=1;z+=1) {for(var y=-1;y<=1;y+=1) {for(var x=-1;x<=1;x+=1) {
        let cell=center+vec3<i32>(x,y,z);var index=photon_word(16u+photon_hash(cell));var visited=0u;
        loop {
            if index==0xffffffffu || index>=count || visited>=count {break;}visited+=1u;
            let record=photon_record(index);let point=photon_vector(record);let delta=point-position;
            let smooth_events=photon_word(record+15u)>>6u;
            // The camera needs one loop slot beyond the glass chain to see the emitter.
            if photon_word(record+11u)==1u && (smooth_events==0u || smooth_events<remaining) && photon_receiver_allowed(record,position) && all(photon_cell(point)==cell) && dot(delta,delta)<=radius*radius && abs(dot(delta,geometric))<=radius*0.1 && dot(photon_vector(record+12u),geometric)>0.0 && triangle(min(photon_word(record+7u),max(geometry[0],1u)-1u)).optics.w==t.optics.w {
                let incoming=photon_vector(record+4u);
                if dot(incoming,normal)>0.0 && dot(incoming,geometric)>0.0 {result+=brdf(t,normal,view,incoming)*photon_vector(record+8u)*(dot(incoming,normal)/dot(incoming,geometric));}
            }
            index=photon_word(record+3u);
        }
    }}}
    return result/(PI*radius*radius);
}
fn photon_enabled()->bool {return photon_word(0u)>0u;}
fn photon_owns_smooth_tail(events:u32,environment:bool)->bool {
    return events>1u && photon_word(select(12u,13u,environment))>0u && events<=photon_word(3u);
}
fn photon_receiver_allowed(record:u32,position:vec3<f32>)->bool {
    let source=(photon_word(record+15u)>>2u)&15u;
    if source==0u {return true;}
    if source>u32(u.light_count.y) {return false;}
    let light=u.punctual_lights[min(max(source,1u)-1u,7u)];
    return length(precise_point_delta(position,vec3<f32>(0.0),light.position.xyz,light.position_low.xyz))>=light.position_low.w;
}
fn photon_gather_glass(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,incident:vec3<f32>,ni:f32,nt:f32,wavelength:f32,side:i32)->vec3<f32> {
    let count=photon_word(0u);if count==0u {return vec3<f32>(0.0);}
    let radius=photon_float(2u);let center=photon_cell(position);var result=vec3<f32>(0.0);
    let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
    for(var z=-1;z<=1;z+=1) {for(var y=-1;y<=1;y+=1) {for(var x=-1;x<=1;x+=1) {
        let cell=center+vec3<i32>(x,y,z);var index=photon_word(16u+photon_hash(cell));var visited=0u;
        loop {
            if index==0xffffffffu || index>=count || visited>=count {break;}visited+=1u;
            let record=photon_record(index);let point=photon_vector(record);let delta=point-position;
            let side_code=select(select(0u,1u,side>0),2u,side<0);
            if photon_word(record+11u)==2u && (photon_word(record+15u)&3u)==side_code && photon_receiver_allowed(record,position) && all(photon_cell(point)==cell) && dot(delta,delta)<=radius*radius && abs(dot(delta,outward))<=radius*0.1 && dot(photon_vector(record+12u),outward)>0.0 && triangle(min(photon_word(record+7u),max(geometry[0],1u)-1u)).optics.w==t.optics.w {
                let incoming=photon_vector(record+4u);let projected=abs(dot(incoming,geometric));
                if projected>0.0 {result+=glass_evaluate_cosine(t.glass,incident,incoming,normal,geometric,ni,nt,wavelength)*photon_vector(record+8u)/projected;}
            }
            index=photon_word(record+3u);
        }
    }}}
    return result/(PI*radius*radius);
}
fn photon_gather_volume(position:vec3<f32>,incident:vec3<f32>,medium:Medium)->vec3<f32> {
    let count=photon_word(0u);if count==0u || medium.scatter.x<=0.0 {return vec3<f32>(0.0);}
    let radius=photon_float(2u);let center=photon_cell(position);var result=vec3<f32>(0.0);
    for(var z=-1;z<=1;z+=1) {for(var y=-1;y<=1;y+=1) {for(var x=-1;x<=1;x+=1) {
        let cell=center+vec3<i32>(x,y,z);var index=photon_word(16u+photon_hash(cell));var visited=0u;
        loop {
            if index==0xffffffffu || index>=count || visited>=count {break;}visited+=1u;
            let record=photon_record(index);let point=photon_vector(record);let delta=point-position;
            if photon_word(record+11u)==3u && photon_word(record+7u)==bitcast<u32>(medium.id) && photon_receiver_allowed(record,position) && all(photon_cell(point)==cell) && dot(delta,delta)<=radius*radius {
                result+=photon_vector(record+8u)*glass_hg_phase(dot(incident,photon_vector(record+4u)),medium.scatter.y);
            }
            index=photon_word(record+3u);
        }
    }}}
    return result*(3.0/(4.0*PI*radius*radius*radius*medium.scatter.x));
}
