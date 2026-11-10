// Legacy jobs retain their six-storage-buffer pipeline and own no photon map.
fn photon_gather(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,view:vec3<f32>,remaining:u32) -> vec3<f32> {
    return vec3<f32>(0.0);
}
fn photon_owns_smooth_tail(events:u32,environment:bool)->bool {return false;}
fn photon_continues_smooth_caustics(roughness:f32)->bool {return false;}
fn photon_enabled()->bool {return false;}
fn photon_gather_glass(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,incident:vec3<f32>,ni:f32,nt:f32,wavelength:f32,side:i32)->vec3<f32> {return vec3<f32>(0.0);}
fn photon_gather_volume(position:vec3<f32>,incident:vec3<f32>,medium:Medium)->vec3<f32> {return vec3<f32>(0.0);}

struct PhotonGlassGather { reflected:vec3<f32>, transmitted:vec3<f32>, total:vec3<f32> };
fn photon_gather_glass_split(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,incident:vec3<f32>,ni:f32,nt:f32,wavelength:f32,side:i32)->PhotonGlassGather {
    return PhotonGlassGather(vec3<f32>(0.0),vec3<f32>(0.0),vec3<f32>(0.0));
}
fn photon_probe_begin(pixel:vec2<u32>,sample:u32,production:bool,stratum:u32) {}

fn photon_probe_bounce(depth:u32) {}
fn photon_probe_sheet_event(event:u32) {}
fn photon_probe_interface(events:u32) {}
fn photon_probe_camera_event(transmitted:bool) {}
fn photon_probe_close_camera() {}
fn photon_probe_set_prefix(prefix:vec3<f32>,roughness:f32) {}
fn photon_probe_photon(value:vec3<f32>)->vec3<f32> {return value;}
fn photon_probe_other(value:vec3<f32>)->vec3<f32> {return value;}
fn photon_probe_save_lane() {}
fn photon_probe_resume_lane(pixel:u32) {}
fn photon_probe_complete_lane(value:vec4<f32>) {}
