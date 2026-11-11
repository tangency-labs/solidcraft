// Counter-based two-dimensional randomized Sobol blocks.
// Keep integer operations in agreement with solidcraft_render::sampling.
// Explicit domains identify physical events; this helper consumes no PCG state.
fn sampling_mix32(input: u32) -> u32 {
    var value = input;
    value ^= (value >> 16u);
    value *= 0x7feb352du;
    value ^= (value >> 15u);
    value *= 0x846ca68bu;
    return value ^ (value >> 16u);
}

fn sampling_sobol_bits(ordinal: u32) -> vec2<u32> {
    var gray = ordinal ^ (ordinal >> 1u);
    var result = vec2<u32>(0u);
    var direction_x = 0x80000000u;
    var direction_y = direction_x;
    loop {
        if gray == 0u { break; }
        if (gray & 1u) != 0u {
            result.x ^= direction_x;
            result.y ^= direction_y;
        }
        gray >>= 1u;
        direction_x >>= 1u;
        direction_y ^= (direction_y >> 1u);
    }
    return result;
}

// Each prefix has one binary permutation. Scramble 23 leading digits, then
// select the midpoint of that cell. The extra fractional bit is exact in f32;
// neither zero nor one can reach an inverse free-flight or other transport map.
fn sampling_nested_scramble(bits: u32, key: u32) -> u32 {
    var prefix = 0u;
    var result = 0u;
    for (var digit = 0u; digit < 23u; digit += 1u) {
        let bit = (bits >> (31u - digit)) & 1u;
        let flip = sampling_mix32(key ^ sampling_mix32(prefix ^ ((digit + 1u) * 0x9e3779b9u))) & 1u;
        result = (result << 1u) | (bit ^ flip);
        prefix = (prefix << 1u) | bit;
    }
    return result;
}

fn scrambled_sobol_pair(seed: u32, pixel: u32, ordinal: u32, domain: u32) -> vec2<f32> {
    var key = sampling_mix32(seed ^ 0x534f424fu);
    key = sampling_mix32(key ^ pixel);
    key = sampling_mix32(key); // CPU pixel high word is zero on the admitted GPU.
    key = sampling_mix32(key ^ domain);
    key = sampling_mix32(key); // This explicit shader domain is one u32.
    let bits = sampling_sobol_bits(ordinal);
    return vec2<f32>(
        f32(2u*sampling_nested_scramble(bits.x, sampling_mix32(key ^ 0x585f4158u))+1u),
        f32(2u*sampling_nested_scramble(bits.y, sampling_mix32(key ^ 0x595f4158u))+1u)
    ) / 16777216.0;
}

// First-Fresnel bins are half-open. A valid draw below one may round up when
// added to a later lane; cap that coordinate at the predecessor of its exact
// binary upper edge. One stratum retains the original draw without arithmetic.
fn first_fresnel_variate(uniform: f32, lane: u32, strata: u32) -> f32 {
    if strata==1u { return uniform; }
    let upper=f32(lane+1u)/f32(strata);
    let predecessor=bitcast<f32>(bitcast<u32>(upper)-1u);
    return min((f32(lane)+uniform)/f32(strata),predecessor);
}

// A forced child carries twice its physical mass because the complete camera
// observation averages two lanes. Zero-mass children complete only their prefix.
fn first_smooth_split_weight(reflectance:f32,transmission_exists:bool,lane:u32)->f32 {
    let reflected=select(1.0,reflectance,transmission_exists);
    return 2.0*select(reflected,1.0-reflected,lane==1u);
}

// Invocation-local camera identity. Photon and diagnostic entry points keep
// camera_sample_active=false, including when camera Sobol intent is enabled.
var<private> camera_sample_active: bool;
var<private> camera_sample_seed: u32;
var<private> camera_sample_pixel: u32;
var<private> camera_sample_ordinal: u32;
var<private> camera_sample_bounce: u32;
var<private> camera_sample_branch: u32;

fn camera_sample_pair(rng: ptr<function,u32>, domain: u32) -> vec2<f32> {
    if camera_sample_active {
        return scrambled_sobol_pair(camera_sample_seed ^ (camera_sample_branch * 0x85ebca6bu),camera_sample_pixel,camera_sample_ordinal,domain);
    }
    return vec2<f32>(random(rng),random(rng));
}

fn camera_sample_value(rng: ptr<function,u32>, domain: u32, axis: u32) -> f32 {
    if camera_sample_active {
        let pair=scrambled_sobol_pair(camera_sample_seed ^ (camera_sample_branch * 0x85ebca6bu),camera_sample_pixel,camera_sample_ordinal,domain);
        return select(pair.x,pair.y,axis==1u);
    }
    return random(rng);
}

// Event indices occupy one u16 per bounce; camera pixel/lens/wavelength domains
// are outside this range. An event never shifts when another event is skipped.
fn path_sample_pair(rng: ptr<function,u32>, event: u32) -> vec2<f32> {
    return camera_sample_pair(rng,0x10000000u+camera_sample_bounce*0x10000u+event);
}
fn path_sample_value(rng: ptr<function,u32>, event: u32, axis: u32) -> f32 {
    return camera_sample_value(rng,0x10000000u+camera_sample_bounce*0x10000u+event,axis);
}
