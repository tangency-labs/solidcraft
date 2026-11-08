// Legacy jobs retain their six-storage-buffer pipeline and own no photon map.
fn photon_gather(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,view:vec3<f32>) -> vec3<f32> {
    return vec3<f32>(0.0);
}
fn photon_enabled()->bool {return false;}
fn photon_gather_glass(t:Triangle,position:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,incident:vec3<f32>,ni:f32,nt:f32,wavelength:f32,side:i32)->vec3<f32> {return vec3<f32>(0.0);}
fn photon_gather_volume(position:vec3<f32>,incident:vec3<f32>,medium:Medium)->vec3<f32> {return vec3<f32>(0.0);}
