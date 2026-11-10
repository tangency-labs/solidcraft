// Optional bounded probe: two independently ranked top-eight lists plus four
// trace-pixel slots. A slot has one writer per dispatch; host Fresnel lanes and
// production observations are ordered. Pilots never write this storage.
const PHOTON_PROBE_WORDS:u32=1024u;
const PHOTON_PROBE_TOP:u32=8u;
const PHOTON_PROBE_RECORD_WORDS:u32=40u;
const PHOTON_PROBE_RECORD_BASE:u32=128u;
const PHOTON_PROBE_PIXEL_BASE:u32=768u;
const PHOTON_PROBE_PIXEL_WORDS:u32=64u;
var<private> photon_probe_active:bool=false;
var<private> photon_probe_camera_active:bool=false;
var<private> photon_probe_stratum:u32=0u;
var<private> photon_probe_sample:u32=0u;
var<private> photon_probe_slot:u32=0u;
var<private> photon_probe_photon_sum:vec3<f32>;
var<private> photon_probe_other_sum:vec3<f32>;
var<private> photon_probe_camera:u32=0u;
var<private> photon_probe_camera_open:bool=true;
var<private> photon_probe_interfaces:u32=0u;
var<private> photon_probe_depth:u32=0u;
var<private> photon_probe_base_depth:u32=0u;
var<private> photon_probe_prefix:vec3<f32>;
var<private> photon_probe_roughness:f32=-1.0;
var<private> photon_probe_query:u32=0u;
var<private> photon_probe_query_eligible:u32=0u;
var<private> photon_probe_query_positive:u32=0u;
var<private> photon_probe_support:vec3<f32>;
fn photon_probe_base()->u32 {return arrayLength(&photon_data)-6u-PHOTON_PROBE_WORDS;}
fn photon_probe_enabled()->bool {return (photon_word(14u)&16u)!=0u;}
fn photon_probe_valid(value:vec3<f32>)->bool {return all(value>=vec3<f32>(0.0)) && all(value<=vec3<f32>(3.4e38));}
fn photon_probe_fail() {atomicOr(&failures[7],1u);}
fn photon_probe_begin(pixel:vec2<u32>,sample:u32,production:bool,stratum:u32) {
    photon_probe_active=false;photon_probe_camera_active=false;
    if !photon_probe_enabled() || !production {return;}
    let base=photon_probe_base();
    if photon_word(base)==0u {return;}
    let scale=photon_word(base+17u);
    if scale!=1u && scale!=2u {photon_probe_fail();return;}
    let selected=vec2<u32>(photon_word(base+1u),photon_word(base+2u));
    let first=(selected/scale)*scale;
    if any(pixel<first) || any(pixel>=first+vec2<u32>(scale)) {return;}
    photon_probe_camera_active=true;photon_probe_stratum=stratum;photon_probe_sample=sample;
    photon_probe_slot=base+PHOTON_PROBE_PIXEL_BASE+((pixel.y-first.y)*scale+pixel.x-first.x)*PHOTON_PROBE_PIXEL_WORDS;
    photon_probe_photon_sum=vec3<f32>(0.0);photon_probe_other_sum=vec3<f32>(0.0);
    photon_probe_camera=0u;photon_probe_camera_open=true;photon_probe_interfaces=0u;
    photon_probe_depth=0u;photon_probe_base_depth=0u;photon_probe_prefix=vec3<f32>(1.0);
    var planned=max(1u,photon_word(base+18u));
    if u.adaptive_budget.x>0u {planned=u32(accumulation[2u*film_pixels()+film_index(pixel.y*u.image.x+pixel.x)].x);}
    if planned==0u || sample>=planned || stratum>=max(1u,u.adaptive_budget.z) {photon_probe_fail();return;}
    let prior=photon_word(photon_probe_slot+2u);
    if prior!=0u && prior!=planned {photon_probe_fail();return;}
    atomicStore(&photon_data[photon_probe_slot+2u],planned);
    if any(pixel!=selected) || sample!=photon_word(base+3u) {return;}
    photon_probe_active=true;
    if photon_word(base+4u)==0u {
        atomicStore(&photon_data[base+4u],1u);
        atomicStore(&photon_data[base+5u],photon_word(15u));
        atomicStore(&photon_data[base+6u],photon_word(4u));
        atomicStore(&photon_data[base+7u],bitcast<u32>(photon_surface_radius()));
        atomicStore(&photon_data[base+8u],photon_word(2u));
    }
}
fn photon_probe_bounce(depth:u32) {
    if photon_probe_camera_active {photon_probe_base_depth=depth;photon_probe_depth=depth;}
}
fn photon_probe_sheet_event(event:u32) {
    if photon_probe_camera_active {photon_probe_depth=photon_probe_base_depth+event;}
}
fn photon_probe_interface(events:u32) {
    if photon_probe_camera_active {photon_probe_interfaces+=events;}
}
fn photon_probe_camera_event(transmitted:bool) {
    if photon_probe_camera_active && photon_probe_camera_open && photon_probe_camera==0u {
        photon_probe_camera=select(1u,2u,transmitted);
    }
}
fn photon_probe_close_camera() {if photon_probe_camera_active {photon_probe_camera_open=false;}}
fn photon_probe_set_prefix(prefix:vec3<f32>,roughness:f32) {
    if photon_probe_active {photon_probe_prefix=prefix;photon_probe_roughness=roughness;}
}
fn photon_probe_photon(value:vec3<f32>)->vec3<f32> {
    if photon_probe_camera_active {photon_probe_photon_sum+=value;if !photon_probe_valid(photon_probe_photon_sum) {photon_probe_fail();}}
    return value;
}
fn photon_probe_other(value:vec3<f32>)->vec3<f32> {
    if photon_probe_camera_active {photon_probe_other_sum+=value;if !photon_probe_valid(photon_probe_other_sum) {photon_probe_fail();}}
    return value;
}
fn photon_probe_save_lane() {
    if !photon_probe_camera_active {return;}
    let slot=photon_probe_slot;
    if photon_word(slot+34u)!=0u {photon_probe_fail();return;}
    photon_store_vector(slot+28u,photon_probe_photon_sum);
    photon_store_vector(slot+31u,photon_probe_other_sum);
    atomicStore(&photon_data[slot+34u],1u);
    atomicStore(&photon_data[slot+53u],photon_probe_stratum);
    atomicStore(&photon_data[slot+54u],photon_probe_sample);
}
fn photon_probe_resume_lane(pixel:u32) {
    let pilots=u.adaptive_budget.x;
    photon_probe_begin(vec2<u32>(pixel%u.image.x,pixel/u.image.x),u.image.z-pilots,u.image.z>=pilots,u32(u.adaptive_error.w));
    if !photon_probe_camera_active {return;}
    let slot=photon_probe_slot;
    if photon_word(slot+34u)!=1u || photon_word(slot+53u)!=photon_probe_stratum || photon_word(slot+54u)!=photon_probe_sample {
        photon_probe_fail();return;
    }
    photon_probe_photon_sum=photon_vector(slot+28u);photon_probe_other_sum=photon_vector(slot+31u);
    atomicStore(&photon_data[slot+34u],0u);
}
fn photon_probe_complete_lane(value:vec4<f32>) {
    if !photon_probe_camera_active {return;}
    let slot=photon_probe_slot;let strata=max(1u,u.adaptive_budget.z);let lane=photon_probe_stratum;
    if photon_word(slot)!=photon_probe_sample || photon_word(slot+3u)!=lane || (value.w!=0.0 && value.w!=1.0)
        || !photon_probe_valid(value.xyz) || !photon_probe_valid(photon_probe_photon_sum) || !photon_probe_valid(photon_probe_other_sum)
        || any(abs(value.xyz-photon_probe_photon_sum-photon_probe_other_sum)>2e-4*max(value.xyz,vec3<f32>(1.0))) {
        photon_probe_fail();return;
    }
    if lane==0u {
        photon_store_vector(slot+16u,vec3<f32>(0.0));photon_store_vector(slot+19u,vec3<f32>(0.0));photon_store_vector(slot+22u,vec3<f32>(0.0));
        atomicStore(&photon_data[slot+25u],bitcast<u32>(value.w));
    } else if photon_float(slot+25u)!=value.w {photon_probe_fail();return;}
    let photon=photon_vector(slot+16u)+photon_probe_photon_sum/f32(strata);
    let other=photon_vector(slot+19u)+photon_probe_other_sum/f32(strata);
    let beauty=photon_vector(slot+22u)+value.xyz/f32(strata);
    if !photon_probe_valid(photon) || !photon_probe_valid(other) || !photon_probe_valid(beauty) {photon_probe_fail();return;}
    photon_store_vector(slot+16u,photon);photon_store_vector(slot+19u,other);photon_store_vector(slot+22u,beauty);
    atomicStore(&photon_data[slot+3u],lane+1u);
    if lane+1u<strata {return;}
    let total_photon=photon_vector(slot+6u)+photon;
    let total_other=photon_vector(slot+9u)+other;
    let total_beauty=photon_vector(slot+12u)+beauty;
    if !photon_probe_valid(total_photon) || !photon_probe_valid(total_other) || !photon_probe_valid(total_beauty) {photon_probe_fail();return;}
    photon_store_vector(slot+6u,total_photon);photon_store_vector(slot+9u,total_other);photon_store_vector(slot+12u,total_beauty);
    atomicStore(&photon_data[slot],photon_probe_sample+1u);
    atomicStore(&photon_data[slot+1u],photon_word(slot+1u)+u32(value.w));
    atomicStore(&photon_data[slot+3u],0u);
    if photon_probe_active {
        if photon_word(slot+51u)!=0u {photon_probe_fail();return;}
        photon_store_vector(slot+40u,photon);photon_store_vector(slot+43u,other);photon_store_vector(slot+46u,beauty);
        atomicStore(&photon_data[slot+49u],bitcast<u32>(value.w));atomicStore(&photon_data[slot+50u],strata);
        atomicStore(&photon_data[slot+51u],1u);
    }
}
fn photon_probe_increment(offset:u32) {
    let at=photon_probe_base()+offset;let old=photon_word(at);
    if old==0xffffffffu {photon_probe_fail();return;}
    atomicStore(&photon_data[at],old+1u);
}
fn photon_probe_gather_begin() {
    if !photon_probe_active {return;}
    photon_probe_query=photon_word(photon_probe_base()+9u);
    photon_probe_increment(9u);photon_probe_query_eligible=0u;photon_probe_query_positive=0u;photon_probe_support=vec3<f32>(0.0);
}
fn photon_probe_gather_end() {
    if !photon_probe_active {return;}
    let base=photon_probe_base();
    for(var list=0u;list<2u;list+=1u) {
        let count=photon_word(base+select(13u,19u,list==1u));
        for(var rank=0u;rank<count;rank+=1u) {
            let at=base+PHOTON_PROBE_RECORD_BASE+(list*PHOTON_PROBE_TOP+rank)*PHOTON_PROBE_RECORD_WORDS;
            if photon_word(at+32u)==photon_probe_query {
                atomicStore(&photon_data[at+34u],photon_probe_query_eligible);
                atomicStore(&photon_data[at+35u],photon_probe_query_positive);
                photon_store_vector(at+36u,photon_probe_support);
            }
        }
    }
}
fn photon_probe_visit() {if photon_probe_active {photon_probe_increment(10u);}}
fn photon_probe_deposit(index:u32,reservoir:PhotonReservoir) {
    if !photon_probe_enabled() {return;}
    let record=photon_record(index)+16u;
    atomicStore(&photon_data[record],reservoir.global_source);
    atomicStore(&photon_data[record+1u],bitcast<u32>(reservoir.source_probability));
    atomicStore(&photon_data[record+2u],bitcast<u32>(reservoir.emission_density));
    atomicStore(&photon_data[record+3u],reservoir.emission_measure);
    atomicStore(&photon_data[record+4u],reservoir.visits);
    atomicStore(&photon_data[record+5u],reservoir.selected_depth);
}
fn photon_probe_top(source:array<u32,40>,camera:bool) {
    let base=photon_probe_base();let score=select(21u,28u,camera);let first=select(0u,PHOTON_PROBE_TOP,camera);
    var entry=source;
    for(var rank=0u;rank<PHOTON_PROBE_TOP;rank+=1u) {
        let slot=base+PHOTON_PROBE_RECORD_BASE+(first+rank)*PHOTON_PROBE_RECORD_WORDS;
        if bitcast<f32>(entry[score])>photon_float(slot+score) {
            for(var word=0u;word<PHOTON_PROBE_RECORD_WORDS;word+=1u) {
                let displaced=photon_word(slot+word);atomicStore(&photon_data[slot+word],entry[word]);entry[word]=displaced;
            }
        }
    }
    let count=base+select(13u,19u,camera);
    atomicStore(&photon_data[count],min(PHOTON_PROBE_TOP,photon_word(count)+1u));
}
fn photon_probe_contribution(index:u32,record:u32,response:vec3<f32>,measure:f32,radius:f32,radiance:vec3<f32>,transmitted:bool) {
    if !photon_probe_active {return;}
    photon_probe_increment(11u);photon_probe_query_eligible+=1u;
    let value=dot(radiance,vec3<f32>(0.2126,0.7152,0.0722));let camera=photon_probe_prefix*radiance;
    let camera_value=dot(camera,vec3<f32>(0.2126,0.7152,0.0722));
    if !photon_probe_valid(radiance) || !photon_probe_valid(camera) || !(value<=3.4e38) || !(camera_value<=3.4e38) {photon_probe_fail();return;}
    if value<=0.0 {return;}
    photon_probe_query_positive+=1u;
    if value>photon_probe_support.x {
        let ratio=photon_probe_support.x/value;
        photon_probe_support=vec3<f32>(value,photon_probe_support.y*ratio+1.0,photon_probe_support.z*ratio*ratio+1.0);
    } else {
        let ratio=value/photon_probe_support.x;
        photon_probe_support.y+=ratio;photon_probe_support.z+=ratio*ratio;
    }
    photon_probe_increment(12u);
    let base=photon_probe_base();let bin=u32(clamp(floor(log2(value))+16.0,0.0,31.0));
    photon_probe_increment(32u+bin);
    let sum=photon_vector(base+14u)+radiance;
    if !photon_probe_valid(sum) {photon_probe_fail();return;}
    photon_store_vector(base+14u,sum);
    var entry:array<u32,40>;
    entry[0u]=index;entry[1u]=photon_word(record+16u);entry[2u]=photon_word(record+17u);entry[3u]=photon_word(record+18u);
    entry[4u]=photon_word(record+19u);entry[5u]=photon_word(record+21u);entry[6u]=photon_word(record+20u);
    entry[7u]=photon_word(record+11u);entry[8u]=photon_word(record+7u);entry[9u]=photon_probe_stratum;
    for(var axis=0u;axis<3u;axis+=1u) {
        entry[10u+axis]=photon_word(record+8u+axis);entry[13u+axis]=bitcast<u32>(response[axis]);
        entry[18u+axis]=bitcast<u32>(radiance[axis]);entry[22u+axis]=bitcast<u32>(photon_probe_prefix[axis]);entry[25u+axis]=bitcast<u32>(camera[axis]);
    }
    entry[16u]=bitcast<u32>(measure);entry[17u]=bitcast<u32>(radius);entry[21u]=bitcast<u32>(value);entry[28u]=bitcast<u32>(camera_value);
    entry[29u]=photon_probe_depth;entry[30u]=photon_probe_interfaces;entry[31u]=bitcast<u32>(photon_probe_roughness);entry[32u]=photon_probe_query;
    var camera_path=photon_probe_camera;
    if entry[7u]==2u && photon_probe_camera_open && camera_path==0u {camera_path=select(1u,2u,transmitted);}
    entry[33u]=camera_path;entry[39u]=max(1u,u.adaptive_budget.z);
    photon_probe_top(entry,false);
    if camera_value>0.0 {
        photon_probe_increment(20u);
        photon_probe_increment(64u+u32(clamp(floor(log2(camera_value))+16.0,0.0,31.0)));
        let camera_sum=photon_vector(base+21u)+camera;
        if !photon_probe_valid(camera_sum) {photon_probe_fail();return;}
        photon_store_vector(base+21u,camera_sum);
        photon_probe_top(entry,true);
    }
}
