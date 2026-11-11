// Optional camera statistics occupy appended film planes. Raw beauty keeps its
// existing address and arithmetic. One observation is the full Fresnel-strata
// average, never an individual correlated branch.
var<private> radiance_components:array<vec3<f32>,6>;
var<private> radiance_camera:u32;
var<private> radiance_camera_open:bool;
var<private> radiance_receiver:bool;
var<private> radiance_direct_weight:vec3<f32>;
var<private> radiance_optical_tail:bool;
// Only clamped programs need this bounded, invocation-local partition. Collect
// new light terms separately: subtracting a large previous sum would lose dim
// components, and clamping each component would change the requested estimator.
var<private> radiance_light_components:array<vec3<f32>,6>;
var<private> radiance_light_open:bool;
fn radiance_begin_clamped_light(bounce:u32) {
    radiance_light_open=clamp_indirect && bounce>0u && radiance_components_enabled();
    if !radiance_light_open {return;}
    for(var component=0u;component<6u;component+=1u) {radiance_light_components[component]=vec3<f32>(0.0);}
}
fn radiance_finish_clamped_light(total:vec3<f32>) {
    if !radiance_light_open {return;}
    let limit=bitcast<f32>(u.page.z);let peak=max(total.x,max(total.y,total.z));
    var scale=1.0;
    if limit>0.0 && peak>limit {scale=limit/peak;}
    for(var component=0u;component<6u;component+=1u) {radiance_components[component]+=radiance_light_components[component]*scale;}
    radiance_light_open=false;
}

fn radiance_enabled()->bool {return (u.sampling.w&32u)!=0u;}
fn radiance_production_components_enabled()->bool {return (u.sampling.w&64u)!=0u;}
fn radiance_components_enabled()->bool {return radiance_production_components_enabled() || (adaptive_component_pilots() && adaptive_pilot_active());}
fn radiance_base()->u32 {return select(1u,3u,u.adaptive_budget.x>0u);}
fn radiance_plane_count()->u32 {
    if !radiance_enabled() {return 0u;}
    return select(3u,27u,radiance_production_components_enabled());
}
fn radiance_address(plane:u32,pixel:u32)->u32 {
    return (radiance_base()+plane)*film_pixels()+film_index(pixel);
}
fn radiance_reset() {
    radiance_light_open=false;
    adaptive_support_reset();
    if !radiance_components_enabled() {return;}
    radiance_camera=0u;radiance_camera_open=true;radiance_receiver=false;radiance_optical_tail=false;
    for(var component=0u;component<6u;component+=1u) {radiance_components[component]=vec3<f32>(0.0);}
}
fn radiance_camera_event(transmitted:bool,continuous:bool) {
    photon_probe_camera_event(transmitted);
    if !radiance_components_enabled() {return;}
    if radiance_camera_open && radiance_camera==0u {radiance_camera=select(1u,2u,transmitted);}
    if radiance_receiver {radiance_optical_tail=true;}
    radiance_receiver=radiance_receiver || continuous;
}
fn radiance_close_camera() {photon_probe_close_camera();if radiance_components_enabled() {radiance_camera_open=false;}}
fn radiance_receiver_event() {
    photon_probe_close_camera();
    if !radiance_components_enabled() {return;}
    radiance_close_camera();radiance_receiver=true;radiance_optical_tail=false;
}
fn radiance_tag()->u32 {if !radiance_components_enabled() {return 0u;}return radiance_camera+select(4u,0u,radiance_camera_open)+select(0u,8u,radiance_receiver)+select(0u,16u,radiance_optical_tail);}
fn radiance_set_tag(tag:u32) {if !radiance_components_enabled() {return;}radiance_camera=tag&3u;radiance_camera_open=(tag&4u)==0u;radiance_receiver=(tag&8u)!=0u;radiance_optical_tail=(tag&16u)!=0u;}
fn radiance_record_class(value:vec3<f32>,camera:u32,caustic:bool) {
    if !radiance_components_enabled() {return;}
    let index=min(camera,2u)*2u+select(0u,1u,caustic);
    if radiance_light_open {radiance_light_components[index]+=value;}
    else {radiance_components[index]+=value;}
}
fn radiance_record(value:vec3<f32>,caustic:bool)->vec3<f32> {
    radiance_record_class(value,radiance_camera,caustic);return value;
}
fn radiance_optical_record(value:vec3<f32>,transmitted:bool,caustic:bool) {
    if !radiance_components_enabled() {return;}
    var camera=radiance_camera;
    if radiance_camera_open && camera==0u {camera=select(1u,2u,transmitted);}
    radiance_record_class(value,camera,caustic);
}
fn radiance_save(pixel:u32) {
    adaptive_support_save(pixel);
    if !radiance_components_enabled() {return;}
    for(var component=0u;component<6u;component+=1u) {
        var address=radiance_address(21u+component,pixel);
        if adaptive_component_pilots() && adaptive_pilot_active() {address=adaptive_component_scratch(component,pixel);}
        accumulation[address]=vec4<f32>(radiance_components[component],0.0);
    }
}
fn radiance_load(pixel:u32) {
    adaptive_support_load(pixel);
    if !radiance_components_enabled() {return;}
    for(var component=0u;component<6u;component+=1u) {
        var address=radiance_address(21u+component,pixel);
        if adaptive_component_pilots() && adaptive_pilot_active() {address=adaptive_component_scratch(component,pixel);}
        radiance_components[component]=accumulation[address].xyz;
    }
}
// Squared deviations can overflow even when every camera observation and its
// beauty variance are finite (exclusive components may exchange equal energy).
// Reject invalid statistics before max() could mask them on a native backend.
fn radiance_value_valid(value:vec3<f32>)->bool {
    return all(value>=vec3<f32>(0.0)) && all(value<=vec3<f32>(3.4e38));
}
fn radiance_observe(pixel:u32,value:vec4<f32>) {
    if !radiance_enabled() || u.image.z<u.adaptive_budget.x {return;}
    let ordinal=u.image.z-u.adaptive_budget.x;
    let lane=u32(u.adaptive_error.w);let strata=max(1u,u.adaptive_budget.z);
    let pending=radiance_address(2u,pixel);
    var sample=vec3<f32>(0.0);
    if lane>0u {sample=accumulation[pending].xyz;}
    sample+=value.xyz/f32(strata);
    if !radiance_value_valid(sample) {atomicOr(&failures[7],1u);return;}
    accumulation[pending]=vec4<f32>(sample,0.0);
    if radiance_components_enabled() {
        for(var component=0u;component<6u;component+=1u) {
            let address=radiance_address(15u+component,pixel);
            var sum=vec3<f32>(0.0);
            if lane>0u {sum=accumulation[address].xyz;}
            sum+=radiance_components[component]/f32(strata);
            if !radiance_value_valid(sum) {atomicOr(&failures[7],1u);return;}
            accumulation[address]=vec4<f32>(sum,0.0);
        }
    }
    if lane+1u<strata {return;}
    let mean_address=radiance_address(0u,pixel);let m2_address=radiance_address(1u,pixel);
    var mean=vec3<f32>(0.0);var m2=vec3<f32>(0.0);var reconstruction_error=0.0;
    if ordinal>0u {mean=accumulation[mean_address].xyz;m2=accumulation[m2_address].xyz;reconstruction_error=accumulation[m2_address].w;}
    let count=f32(ordinal+1u);let delta=sample-mean;
    mean+=delta/count;m2+=delta*(sample-mean);
    if !radiance_value_valid(mean) || !radiance_value_valid(m2) {atomicOr(&failures[7],1u);return;}
    accumulation[mean_address]=vec4<f32>(mean,count);
    accumulation[m2_address]=vec4<f32>(max(m2,vec3<f32>(0.0)),reconstruction_error);
    if radiance_components_enabled() {
        var sum=vec3<f32>(0.0);
        for(var component=0u;component<6u;component+=1u) {
            let sample_component=accumulation[radiance_address(15u+component,pixel)].xyz;
            let cm_address=radiance_address(3u+component,pixel);let cv_address=radiance_address(9u+component,pixel);
            var cm=vec3<f32>(0.0);var cv=vec3<f32>(0.0);
            if ordinal>0u {cm=accumulation[cm_address].xyz;cv=accumulation[cv_address].xyz;}
            let difference=sample_component-cm;cm+=difference/count;cv+=difference*(sample_component-cm);
            if !radiance_value_valid(sample_component) || !radiance_value_valid(cm) || !radiance_value_valid(cv) {atomicOr(&failures[7],1u);return;}
            accumulation[cm_address]=vec4<f32>(cm,0.0);
            accumulation[cv_address]=vec4<f32>(max(cv,vec3<f32>(0.0)),0.0);
            sum+=sample_component;
        }
        let residual=abs(sum-sample);
        accumulation[m2_address].w=max(reconstruction_error,max(max(residual.x,residual.y),residual.z));
        if any(abs(sum-sample)>2e-4*max(abs(sample),vec3<f32>(1.0))) {atomicOr(&failures[7],32u);}
    }
}

fn radiance_direct(value:vec3<f32>,caustic:bool)->vec3<f32> {
    radiance_record_class(radiance_direct_weight*value,radiance_camera,caustic || radiance_optical_tail);return value;
}
fn radiance_glass_photons(prefix:vec3<f32>,t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,incident:vec3<f32>,ni:f32,nt:f32,wavelength:f32,side:i32)->vec3<f32> {
    photon_probe_set_prefix(prefix,t.glass.x);
    if !radiance_components_enabled() {return photon_gather_glass(t,position,normal,geometric,incident,ni,nt,wavelength,side);}
    let gathered=photon_gather_glass_split(t,position,normal,geometric,incident,ni,nt,wavelength,side);
    radiance_optical_record(prefix*gathered.reflected,false,true);
    radiance_optical_record(prefix*gathered.transmitted,true,true);
    return gathered.total;
}
