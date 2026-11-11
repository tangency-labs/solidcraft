// Optional pilot evidence uses appended, admitted film planes. Assessment maps
// are independent identities at one reference radius. These are allocation and
// descriptive support statistics, never calibrated production error or bias.
fn adaptive_evidence_enabled()->bool {return (u.sampling.w&256u)!=0u;}
fn adaptive_component_pilots()->bool {return (u.sampling.w&128u)!=0u;}
fn adaptive_pilot_active()->bool {return adaptive_evidence_enabled() && u.image.z<u.adaptive_budget.x;}
fn adaptive_evidence_groups()->u32 {return select(1u,7u,adaptive_component_pilots());}
fn adaptive_evidence_moments()->u32 {return select(2u,5u,(u.page.w>>1u)>0u);}
fn adaptive_evidence_moment_planes()->u32 {return adaptive_evidence_groups()*adaptive_evidence_moments();}
fn adaptive_evidence_plane_count()->u32 {
    if !adaptive_evidence_enabled() {return 0u;}
    return adaptive_evidence_moment_planes()+3u+select(0u,6u,adaptive_component_pilots());
}
fn adaptive_evidence_address(plane:u32,pixel:u32)->u32 {
    return (radiance_base()+radiance_plane_count()+plane)*film_pixels()+film_index(pixel);
}
fn adaptive_component_scratch(component:u32,pixel:u32)->u32 {
    return adaptive_evidence_address(adaptive_evidence_moment_planes()+3u+component,pixel);
}
var<private> adaptive_query_support:vec4<f32>; // positive events, max luminance, scaled sum, scaled square sum
var<private> adaptive_query_active:bool;
var<private> adaptive_query_invalid:bool;
var<private> adaptive_observation_support:vec4<f32>; // queries, positive queries, invalid queries, minimum inverse concentration
fn adaptive_support_reset() {
    if !adaptive_pilot_active() {return;}
    adaptive_query_support=vec4<f32>(0.0);adaptive_query_active=false;adaptive_query_invalid=false;
    adaptive_observation_support=vec4<f32>(0.0);
}
fn adaptive_photon_end() {
    if !adaptive_pilot_active() || !adaptive_query_active {return;}
    adaptive_observation_support.x+=1.0;
    if adaptive_query_invalid {adaptive_observation_support.z+=1.0;}
    if adaptive_query_support.x>0.0 {
        let normalized=adaptive_query_support.z/sqrt(adaptive_query_support.w);
        let support=clamp(normalized*normalized,1.0,adaptive_query_support.x);
        adaptive_observation_support.y+=1.0;
        if adaptive_observation_support.w==0.0 {adaptive_observation_support.w=support;}
        else {adaptive_observation_support.w=min(adaptive_observation_support.w,support);}
    }
    adaptive_query_active=false;
}
fn adaptive_photon_begin() {
    if !adaptive_pilot_active() {return;}
    adaptive_photon_end();adaptive_query_support=vec4<f32>(0.0);adaptive_query_invalid=false;adaptive_query_active=true;
}
fn adaptive_photon_contribution(value:vec3<f32>) {
    if !adaptive_pilot_active() || !adaptive_query_active {return;}
    if !radiance_value_valid(value) {adaptive_query_invalid=true;return;}
    let luminance=dot(value,vec3<f32>(0.2126,0.7152,0.0722));
    if !(luminance<=3.4e38) {adaptive_query_invalid=true;return;}
    if luminance<=0.0 {return;}
    if luminance>adaptive_query_support.y {
        let scale=adaptive_query_support.y/luminance;
        adaptive_query_support.z*=scale;adaptive_query_support.w*=scale*scale;adaptive_query_support.y=luminance;
    }
    let weight=luminance/adaptive_query_support.y;
    adaptive_query_support.x+=1.0;adaptive_query_support.z+=weight;adaptive_query_support.w+=weight*weight;
}
fn adaptive_support_save(pixel:u32) {
    if !adaptive_pilot_active() {return;}
    adaptive_photon_end();
    accumulation[adaptive_evidence_address(adaptive_evidence_moment_planes()+1u,pixel)]=adaptive_observation_support;
}
fn adaptive_support_load(pixel:u32) {
    if !adaptive_pilot_active() {return;}
    adaptive_query_active=false;adaptive_query_invalid=false;
    adaptive_observation_support=accumulation[adaptive_evidence_address(adaptive_evidence_moment_planes()+1u,pixel)];
}
fn adaptive_evidence_valid(value:vec4<f32>)->bool {
    return all(value>=vec4<f32>(0.0)) && all(value<=vec4<f32>(3.4e38));
}
fn adaptive_evidence_tolerance(mean:vec4<f32>,group:u32)->vec4<f32> {
    let absolute=u.adaptive_error.y/select(1.0,6.0,group>0u);
    var tolerance=vec4<f32>(vec3<f32>(absolute)+mean.xyz*u.adaptive_error.x,u.adaptive_error.z);
    if group>0u {tolerance.w=1.0;}
    return tolerance;
}
fn adaptive_evidence_observe(pixel:u32,value:vec4<f32>)->vec2<f32> {
    if !adaptive_pilot_active() {return vec2<f32>(0.0);}
    adaptive_photon_end();
    if adaptive_component_pilots() {
        var recombined=vec3<f32>(0.0);
        for(var component=0u;component<6u;component+=1u) {recombined+=radiance_components[component];}
        if !radiance_value_valid(recombined) || any(abs(recombined-value.xyz)>2e-4*max(abs(value.xyz),vec3<f32>(1.0))) {atomicOr(&failures[7],32u);}
    }
    let batches=u.page.w>>1u;
    var per_batch=u.adaptive_budget.x;
    if batches>0u {per_batch/=batches;}
    let local_count=u.image.z%per_batch+1u;let batch_count=u.image.z/per_batch+1u;
    let last=u.image.z+1u==u.adaptive_budget.x;
    var requested_samples=u.adaptive_budget.y;var limited=false;var component_limited=false;var invalid=false;var map_ratio=0.0;
    let ceiling=sqrt(f32(u.adaptive_budget.w));
    for(var group=0u;group<adaptive_evidence_groups();group+=1u) {
        var group_invalid=false;
        var sample=value;
        if group>0u {sample=vec4<f32>(radiance_components[group-1u],0.0);}
        let base=group*adaptive_evidence_moments();
        let mean_address=adaptive_evidence_address(base,pixel);let m2_address=adaptive_evidence_address(base+1u,pixel);
        var mean=vec4<f32>(0.0);var m2=vec4<f32>(0.0);
        if local_count>1u {mean=accumulation[mean_address];m2=accumulation[m2_address];}
        let delta=sample-mean;mean+=delta/f32(local_count);m2+=delta*(sample-mean);
        accumulation[mean_address]=mean;accumulation[m2_address]=m2;
        var overall_mean=mean;var within=m2/f32(per_batch-1u);var map_variance=vec4<f32>(0.0);
        if batches>0u && local_count==per_batch {
            let means_address=adaptive_evidence_address(base+2u,pixel);let between_address=adaptive_evidence_address(base+3u,pixel);
            let within_address=adaptive_evidence_address(base+4u,pixel);
            var means=vec4<f32>(0.0);var between=vec4<f32>(0.0);var within_sum=vec4<f32>(0.0);
            if batch_count>1u {means=accumulation[means_address];between=accumulation[between_address];within_sum=accumulation[within_address];}
            let difference=mean-means;means+=difference/f32(batch_count);between+=difference*(mean-means);within_sum+=m2;
            accumulation[means_address]=means;accumulation[between_address]=between;accumulation[within_address]=within_sum;
            if last {
                overall_mean=means;within=within_sum/f32(batches*(per_batch-1u));
                if !adaptive_evidence_valid(means) || !adaptive_evidence_valid(between) || !adaptive_evidence_valid(within_sum) {group_invalid=true;}
                map_variance=max(vec4<f32>(0.0),between/f32(batches-1u)-within/f32(per_batch));
            }
        }
        if last {
            var variance=within;
            if (u.page.w&1u)!=0u {variance+=map_variance;}
            if !adaptive_evidence_valid(overall_mean) || !adaptive_evidence_valid(m2) || !adaptive_evidence_valid(variance) || !adaptive_evidence_valid(map_variance) {group_invalid=true;}
            if !group_invalid {
                let tolerance=adaptive_evidence_tolerance(overall_mean,group);
                let ratio=sqrt(variance)/tolerance;let bounded=min(ratio,vec4<f32>(ceiling));let need=ceil(bounded*bounded);
                requested_samples=max(requested_samples,min(u.adaptive_budget.w,u32(max(max(need.x,need.y),max(need.z,need.w)))));
                let group_limited=any(ratio>vec4<f32>(ceiling));limited=limited || group_limited;component_limited=component_limited || (group>0u && group_limited);
                let scatter=sqrt(map_variance)/tolerance;
                map_ratio=max(map_ratio,max(max(scatter.x,scatter.y),max(scatter.z,scatter.w)));
            }
            invalid=invalid || group_invalid;
            component_limited=component_limited || (group>0u && group_invalid);
        }
    }
    let support_address=adaptive_evidence_address(adaptive_evidence_moment_planes(),pixel);
    var support=vec4<f32>(0.0);
    if u.image.z>0u {support=accumulation[support_address];}
    support.x+=select(0.0,1.0,adaptive_observation_support.x>0.0);
    support.y+=select(0.0,1.0,adaptive_observation_support.x>0.0 && adaptive_observation_support.y==0.0);
    if adaptive_observation_support.w>0.0 {
        if support.z==0.0 {support.z=adaptive_observation_support.w;}else {support.z=min(support.z,adaptive_observation_support.w);}
    }
    if adaptive_observation_support.z>0.0 {support.w=1.0;}
    accumulation[support_address]=support;
    if last {
        invalid=invalid || support.w>0.0;
        if invalid {requested_samples=u.adaptive_budget.w;limited=true;}
        // A nonfinite diagnostic is marked unavailable, never serialized as a
        // finite confidence value. The conservative allocation remains capped.
        if !(map_ratio<=3.4e38) {map_ratio=0.0;invalid=true;requested_samples=u.adaptive_budget.w;limited=true;}
        accumulation[adaptive_evidence_address(adaptive_evidence_moment_planes()+2u,pixel)]=vec4<f32>(select(0.0,1.0,component_limited),map_ratio,select(0.0,1.0,invalid),0.0);
    }
    return vec2<f32>(f32(requested_samples),select(0.0,1.0,limited));
}
