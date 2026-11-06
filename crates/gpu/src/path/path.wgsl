// Original software BVH and unidirectional Monte Carlo integrator. All directions
// at a surface point away from it, matching render::surface's GGX/Smith oracle.
struct Triangle {
    p0: vec4<f32>, p1: vec4<f32>, p2: vec4<f32>,
    n0: vec4<f32>, n1: vec4<f32>, n2: vec4<f32>,
    // base.w: dielectric flag, parameters.w: thin-sheet thickness (-1 = solid).
    // optics.xyz: extinction per mm, optics.w: body/medium identity (1..16384).
    base: vec4<f32>, parameters: vec4<f32>, optics: vec4<f32>,
    coat: vec4<f32>, // weight, perceptual roughness, IOR, shadow transmission flag
    interior: vec4<f32>, // scattering/mm, HG g, flags (stage=1, scalar face=2), immediate parent triangle+1
    glass: vec4<f32>, // optional roughness, Abbe number, film thickness/nm, film IOR
};
struct Node { lo: vec3<f32>, first: u32, hi: vec3<f32>, count: u32 };
struct AreaLight { center: vec4<f32>, u: vec4<f32>, v: vec4<f32>, radiance: vec4<f32> };
struct DirectionalLight { direction: vec4<f32>, color: vec4<f32> };
struct PunctualLight { position: vec4<f32>, position_low: vec4<f32>, color: vec4<f32>, axis: vec4<f32>, cone: vec4<f32> };
struct Params {
    // width, height, sample index, pixel offset
    image: vec4<u32>,
    // seed, maximum depth, tile length, reserved
    sampling: vec4<u32>,
    eye: vec4<f32>, right: vec4<f32>, up: vec4<f32>, back: vec4<f32>,
    // ray epsilon, test environment radiance, studio scale, reserved
    lighting: vec4<f32>,
    environment: vec4<f32>, // RGB radiance, enabled flag
    light_count: vec4<f32>, // x: hard directional count, y: point/spot count, w: rectangle count
    area_lights: array<AreaLight,4>,
    environment_map: vec4<f32>, // cosine rotation, sine rotation, intensity, enabled
    sun_direction: vec4<f32>, // normalized toward sun, cosine angular radius
    sun_radiance: vec4<f32>, // linear RGB, cone density (zero when disabled)
    adaptive_budget: vec4<u32>, // pilot count, minimum production, Fresnel strata, maximum production
    adaptive_error: vec4<f32>, // relative RGB, absolute RGB, absolute coverage, current production stratum
    volume_min: vec4<f32>, // camera-relative bounds minimum, extinction/mm
    volume_max: vec4<f32>, // camera-relative bounds maximum, HG mean cosine
    volume_albedo: vec4<f32>, // RGB single-scattering albedo, active glass model flag
    aging: array<AgingLayer,8>,
    directional_lights: array<DirectionalLight,8>,
    punctual_lights: array<PunctualLight,8>,
    studio_weights: vec4<f32>,

};
@group(0) @binding(0) var<uniform> u: Params;
@group(0) @binding(1) var<storage, read> geometry: array<u32>;
@group(0) @binding(2) var<storage, read> nodes: array<Node>;
@group(0) @binding(3) var<storage, read_write> accumulation: array<vec4<f32>>;
struct EnvironmentTexel { rgb: vec3<f32>, pdf: f32, threshold: f32, alternate: u32, padding: vec2<f32> };
struct EnvironmentData { header: vec4<u32>, texels: array<EnvironmentTexel> };
@group(0) @binding(4) var<storage, read> environment_data: EnvironmentData;
// Count low, first pixel/bounce/sample/code, count high, first-writer flag, spare.
// Two atomic words count jobs with more than 2^32 paths without wrapping to zero.
@group(0) @binding(6) var<storage, read_write> failures: array<atomic<u32>>;
// Codes: 1 escaped inside a medium, 2 exit with no medium, 3 exit from another
// medium, 4 ambiguous origin/certificate, invalid entry or beyond16 nested media.
fn medium_failure(radiance: vec3<f32>, coverage: f32, pixel: u32, bounce: u32, code: u32) -> vec4<f32> {
    if atomicAdd(&failures[0],1u)==0xffffffffu { atomicAdd(&failures[5],1u); }
    if atomicExchange(&failures[6],1u)==0u {
        atomicStore(&failures[1],pixel); atomicStore(&failures[2],bounce);
        atomicStore(&failures[3],u.image.z); atomicStore(&failures[4],code);
    }
    // End the path with the light it has already gathered.
    return vec4<f32>(radiance,coverage);
}
const PI: f32 = 3.141592653589793;
const FAR: f32 = 1e30;
override OPAQUE_SCENE: bool = false;
override HAS_TEXTURES: bool = false;

// Header: triangle count, vertex offset, material offset, planar-patch offset (all words).
// Indexed triangles contain three vertex indices and one resolved material ID.
fn vector_at(word: u32) -> vec4<f32> {
    return bitcast<vec4<f32>>(vec4<u32>(geometry[word],geometry[word+1u],geometry[word+2u],geometry[word+3u]));
}
fn triangle(index: u32) -> Triangle {
    let word = 4u+index*4u;
    let a = geometry[1]+geometry[word]*8u;
    let b = geometry[1]+geometry[word+1u]*8u;
    let c = geometry[1]+geometry[word+2u]*8u;
    let has_model=(geometry[2]&0x80000000u)!=0u;
    let material = (geometry[2]&0x3fffffffu)+geometry[word+3u]*select(20u,24u,has_model);
    var glass=vec4<f32>(0.0);
    if has_model {glass=vector_at(material+20u);}
    return Triangle(vector_at(a),vector_at(b),vector_at(c),vector_at(a+4u),vector_at(b+4u),vector_at(c+4u),vector_at(material),vector_at(material+4u),vector_at(material+8u),vector_at(material+12u),vector_at(material+16u),glass);
}

// Exact s/p power reflectance and transmitted cosine; -1 denotes TIR.
fn dielectric(c: f32, ni: f32, nt: f32) -> vec3<f32> {
    if ni == nt { return vec3<f32>(0.0,0.0,c); }
    let eta = ni/nt;
    let s2 = eta*eta*max(0.0,1.0-c*c);
    if s2 >= 1.0 { return vec3<f32>(1.0,1.0,-1.0); }
    let ct = sqrt(1.0-s2);
    let rs = (ni*c-nt*ct)/(ni*c+nt*ct);
    let rp = (nt*c-ni*ct)/(nt*c+ni*ct);
    return vec3<f32>(rs*rs,rp*rp,ct);
}
struct Sheet { reflection: vec3<f32>, transmission: vec3<f32> };
fn sheet_polarization(r: f32, a: vec3<f32>) -> Sheet {
    let denominator = vec3<f32>(1.0)-r*r*a*a;
    // The grazing lossless limit is perfectly reflecting.
    if r >= 1.0 { return Sheet(vec3<f32>(1.0),vec3<f32>(0.0)); }
    return Sheet(vec3<f32>(r)+(1.0-r)*(1.0-r)*r*a*a/denominator,(1.0-r)*(1.0-r)*a/denominator);
}
fn sheet(t: Triangle, f: vec3<f32>) -> Sheet {
    if f.z < 0.0 { return Sheet(vec3<f32>(1.0),vec3<f32>(0.0)); }
    // An index-matched zero-thickness sheet has no optical path or interface,
    // including exact grazing. Avoid both 0/0 and an invented opaque boundary.
    var attenuation = vec3<f32>(1.0);
    if t.parameters.w > 0.0 {
        if f.z == 0.0 {
            // No finite path exists for a positive-thickness tangent sheet.
            // Share the retained diagnostic word without changing buffer layout.
            atomicOr(&failures[7],2u);
            return Sheet(vec3<f32>(0.0),vec3<f32>(0.0));
        }
        attenuation = exp(-t.optics.xyz*(t.parameters.w/f.z));
    }
    let s = sheet_polarization(f.x,attenuation);
    let p = sheet_polarization(f.y,attenuation);
    return Sheet((s.reflection+p.reflection)*0.5,(s.transmission+p.transmission)*0.5);
}
struct Medium { id: f32, ior: f32, sigma: vec3<f32>, scatter: vec2<f32> };
// Establish the complete certified containment stack even when the camera starts
// inside a liquid or a ray starts on an opaque object inside a container. Parent
// references are immutable CPU-certified material references, never AABB guesses.
fn initialize_media(hit_index:u32,d:vec3<f32>,stack:ptr<function,array<Medium,MEDIUM_CAPACITY>>,count:ptr<function,u32>) -> bool {
    return initialize_media_spectral(hit_index,d,stack,count,0.0);
}
fn initialize_media_spectral(hit_index:u32,d:vec3<f32>,stack:ptr<function,array<Medium,MEDIUM_CAPACITY>>,count:ptr<function,u32>,wavelength:f32) -> bool {
    let first=triangle(hit_index);
    if first.base.w<=0.0 || first.parameters.w>=0.0 {return true;}
    let outward=unit(cross(first.p1.xyz-first.p0.xyz,first.p2.xyz-first.p0.xyz));
    let entering=dot(outward,d)<0.0;
    var reference=select(hit_index+1u,u32(first.interior.w),entering);
    var length=0u;
    while reference>0u {
        if length>=MEDIUM_CAPACITY {return false;}
        let t=triangle(reference-1u);
        (*stack)[length]=Medium(t.optics.w,glass_ior(t.parameters.z,t.glass.y,wavelength),t.optics.xyz,t.interior.xy);
        reference=u32(t.interior.w);
        length+=1u;
    }
    for(var i=0u;i<length/2u;i+=1u) {
        let swap=(*stack)[i];(*stack)[i]=(*stack)[length-1u-i];(*stack)[length-1u-i]=swap;
    }
    *count=length;
    return true;
}

fn random(state: ptr<function, u32>) -> f32 {
    *state = *state * 747796405u + 2891336453u;
    let word = ((*state >> ((*state >> 28u) + 4u)) ^ *state) * 277803737u;
    return f32(((word >> 22u) ^ word) >> 8u) * (1.0 / 16777216.0);
}

fn unit(v: vec3<f32>) -> vec3<f32> {
    let scale = max(max(abs(v.x), abs(v.y)), abs(v.z));
    if scale == 0.0 { return vec3<f32>(0.0, 0.0, 1.0); }
    return normalize(v / scale);
}
fn fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    let t = 1.0 - clamp(c, 0.0, 1.0);
    return f0 + (vec3<f32>(1.0) - f0) * (t*t*t*t*t);
}
// An index-matched dielectric has no interface reflection at any angle.
// Metal fractions still contribute their independently weighted grazing limit.
fn base_fresnel(f0:vec3<f32>,cosine:f32,metallic:f32,ior:f32)->vec3<f32> {
    let x=1.0-clamp(cosine,0.0,1.0);
    let grazing=select(1.0,metallic,ior==1.0);
    return f0+(vec3<f32>(grazing)-f0)*(x*x*x*x*x);
}
fn distribution(nh: f32, a2: f32) -> f32 {
    let d = (1.0 - nh*nh) + nh*nh*a2;
    return a2 / (PI*d*d);
}
fn brdf(t: Triangle, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nv = clamp(dot(n,v), -1.0, 1.0);
    let nl = clamp(dot(n,l), -1.0, 1.0);
    if nv <= 0.0 || nl <= 0.0 { return vec3<f32>(0.0); }
    let h = unit(v+l);
    let a = t.parameters.y*t.parameters.y;
    let a2 = a*a;
    let d = distribution(clamp(dot(n,h), 0.0, 1.0), a2);
    let gv = 2.0 / (nv + sqrt(a2 + (1.0-a2)*nv*nv));
    let gl = 2.0 / (nl + sqrt(a2 + (1.0-a2)*nl*nl));
    let ratio = (t.parameters.z - 1.0) / (t.parameters.z + 1.0);
    let f0 = mix(vec3<f32>(ratio*ratio), t.base.xyz, t.parameters.x);
    let diffuse = t.base.xyz * ((1.0-t.parameters.x)/PI);
    let base = base_fresnel(f0,dot(v,h),t.parameters.x,t.parameters.z) * (d*0.25*gv*gl)
        + (vec3<f32>(1.0)-base_fresnel(f0,nv,t.parameters.x,t.parameters.z)) * (vec3<f32>(1.0)-base_fresnel(f0,nl,t.parameters.x,t.parameters.z)) * diffuse;
    if t.coat.x <= 0.0 || t.coat.z == 1.0 { return base; }
    let coat_a = t.coat.y*t.coat.y;
    let coat_a2 = coat_a*coat_a;
    let coat_d = distribution(clamp(dot(n,h),0.0,1.0),coat_a2);
    let coat_gv = 2.0/(nv+sqrt(coat_a2+(1.0-coat_a2)*nv*nv));
    let coat_gl = 2.0/(nl+sqrt(coat_a2+(1.0-coat_a2)*nl*nl));
    let coat_ratio = (t.coat.z-1.0)/(t.coat.z+1.0);
    let coat_f0 = vec3<f32>(coat_ratio*coat_ratio);
    let attenuation = (vec3<f32>(1.0)-t.coat.x*coat_energy_bound(coat_f0,nv))*(vec3<f32>(1.0)-t.coat.x*coat_energy_bound(coat_f0,nl));
    return base*attenuation+t.coat.x*fresnel(coat_f0,dot(v,h))*(coat_d*0.25*coat_gv*coat_gl);
}

// Explicit stored vertex anchor plus unadded local offset. Neither field is
// an error-free floating residual; do not combine them before relative tests.
// Shader backends may reassociate floating expressions, so the captured corner
// regressions remain required on each supported backend.
// Integer-defined IEEE-754 binary32 addition, round-to-nearest/ties-to-even.
// All alignment and rounding happen in u32, so shader floating reassociation
// cannot combine anchor subtraction with the subsequent local displacement.
// The bit interface also preserves subnormals for the arithmetic oracle; normal
// shader float consumers may still flush subnormal values under WGSL's rules.
fn fp_shift_jam(value: u32, amount: u32) -> u32 {
    if amount == 0u { return value; }
    if amount >= 32u { return select(0u,1u,value != 0u); }
    let lost = value << (32u-amount);
    return (value >> amount) | select(0u,1u,lost != 0u);
}
fn fp_add_bits(left: u32, right: u32) -> u32 {
    var a=left;
    var b=right;
    if (a&0x7fffffffu) < (b&0x7fffffffu) { a=right; b=left; }
    let sign=a&0x80000000u;
    let ea=(a>>23u)&255u;
    let eb=(b>>23u)&255u;
    let fa=a&0x7fffffu;
    let fb=b&0x7fffffu;
    if ea == 255u {
        if fa != 0u || (eb == 255u && fb != 0u) { return 0x7fc00000u; }
        if eb == 255u && ((a^b)&0x80000000u) != 0u { return 0x7fc00000u; }
        return a;
    }
    if (b&0x7fffffffu) == 0u {
        if (a&0x7fffffffu) == 0u { return (a&b)&0x80000000u; }
        return a;
    }
    var exponent=max(ea,1u);
    let other_exponent=max(eb,1u);
    let sa=(fa | select(0u,0x800000u,ea != 0u))<<3u;
    let sb=fp_shift_jam((fb | select(0u,0x800000u,eb != 0u))<<3u,exponent-other_exponent);
    var significand=0u;
    if ((a^b)&0x80000000u) == 0u {
        significand=sa+sb;
        if significand >= 0x8000000u {
            significand=fp_shift_jam(significand,1u);
            exponent+=1u;
        }
    } else {
        significand=sa-sb;
        if significand == 0u { return 0u; }
        // Nonzero significand is below bit 27. Shift to bit 26, or stop at
        // the subnormal exponent, exactly as the bounded normalization loop.
        let shift=min(countLeadingZeros(significand)-5u,exponent-1u);
        significand<<=shift;
        exponent-=shift;
    }
    let remainder=significand&7u;
    var rounded=significand>>3u;
    if remainder>4u || (remainder == 4u && (rounded&1u) != 0u) { rounded+=1u; }
    if rounded >= 0x1000000u { rounded>>=1u; exponent+=1u; }
    if exponent >= 255u { return sign|0x7f800000u; }
    if rounded < 0x800000u { exponent=0u; }
    return sign|(exponent<<23u)|(rounded&0x7fffffu);
}
fn fp_sub_bits(left: u32, right: u32) -> u32 {
    return fp_add_bits(left,right^0x80000000u);
}
// Exact identities, then a guarded Sterbenz subtraction. WGSL does not specify
// RTE for a general floating subtraction, so approximate differences must retain
// the integer implementation. The fast case has an exactly representable normal
// result, independent of rounding direction and permitted subnormal flushing.
fn fp_sub_barrier_bits(left: u32, right: u32) -> u32 {
    let a=left&0x7fffffffu;
    let b=right&0x7fffffffu;
    if a>=0x7f800000u || b>=0x7f800000u { return fp_sub_bits(left,right); }
    if a==0u {
        if b==0u { return left&(right^0x80000000u)&0x80000000u; }
        return right^0x80000000u;
    }
    if b==0u { return left; }
    if left==right { return 0u; }
    let small=min(a,b);
    let large=max(a,b);
    let small_exponent=small>>23u;
    let large_exponent=large>>23u;
    let same_sign=((left^right)&0x80000000u)==0u;
    let within_two=large_exponent==small_exponent ||
        (large_exponent==small_exponent+1u && (large&0x7fffffu)<=(small&0x7fffffu));
    // Encoded exponent 24 has spacing 2^-126. Any distinct pair admitted here
    // therefore has a normal difference, including the closest adjacent pair.
    if same_sign && small_exponent>=24u && within_two {
        return bitcast<u32>(bitcast<f32>(left)-bitcast<f32>(right));
    }
    return fp_sub_bits(left,right);
}
fn anchored_delta_bits(a: u32, a_low: u32, b: u32, b_low: u32) -> u32 {
    let offset=fp_sub_barrier_bits(a_low,b_low);
    // Identical finite anchors have +0 difference. Adding it preserves a
    // nonzero offset; either sign of a zero offset becomes +0.
    if a==b && (a&0x7fffffffu)<0x7f800000u {
        return select(offset,0u,(offset&0x7fffffffu)==0u);
    }
    let anchor=fp_sub_barrier_bits(a,b);
    if (offset&0x7fffffffu)==0u {
        // IEEE RTE addition of two zeros retains -0 only if both are -0.
        return select(anchor,anchor&offset,(anchor&0x7fffffffu)==0u);
    }
    // This nonlinear integer operation is the reassociation barrier. Keep it
    // even when both component differences used exact hardware subtraction.
    return fp_add_bits(anchor,offset);
}
fn anchored_delta_component(a: f32, a_low: f32, b: f32, b_low: f32) -> f32 {
    return bitcast<f32>(anchored_delta_bits(bitcast<u32>(a),bitcast<u32>(a_low),bitcast<u32>(b),bitcast<u32>(b_low)));
}

struct PrecisePoint { hi: vec3<f32>, lo: vec3<f32> };
fn precise_point_delta(a: vec3<f32>, a_low: vec3<f32>, b: vec3<f32>, b_low: vec3<f32>) -> vec3<f32> {
    if all(a_low==vec3<f32>(0.0)) && all(b_low==vec3<f32>(0.0)) { return a-b; }
    return vec3<f32>(anchored_delta_component(a.x,a_low.x,b.x,b_low.x),anchored_delta_component(a.y,a_low.y,b.y,b_low.y),anchored_delta_component(a.z,a_low.z,b.z,b_low.z));
}
fn ray_segment_precise(o: vec3<f32>, o_low: vec3<f32>, previous: vec3<f32>, previous_low: vec3<f32>, d: vec3<f32>, distance: f32) -> vec3<f32> {
    // Subtract the physical origins before adding a possibly tiny ray segment.
    return precise_point_delta(o,o_low,previous,previous_low)+d*distance;
}

// Explicit parallel slab handling avoids 0*infinity/NaN at box boundaries.
// Returns the entry distance into the padded box within [0, limit], or -1.
fn box_entry_padded(node: Node, o: vec3<f32>, d: vec3<f32>, limit: f32, pad: f32) -> f32 {
    var near = 0.0;
    var far = limit;
    for (var k = 0u; k < 3u; k += 1u) {
        if abs(d[k]) < 1e-20 {
            if o[k] < node.lo[k]-pad || o[k] > node.hi[k]+pad { return -1.0; }
        } else {
            let a = (node.lo[k]-pad-o[k])/d[k];
            let b = (node.hi[k]+pad-o[k])/d[k];
            near = max(near, min(a,b)); far = min(far, max(a,b));
        }
    }
    return select(-1.0,near,far >= near);
}
// A strictly positive ray cannot reach a triangle when any coordinate starts
// on/outside its exact stored bound and moves away. These comparisons introduce
// no epsilon or rounded division; padded BVH bounds remain only broad phase.
fn triangle_ahead(t: Triangle, o: vec3<f32>, d: vec3<f32>) -> bool {
    let lo=min(t.p0.xyz,min(t.p1.xyz,t.p2.xyz));
    let hi=max(t.p0.xyz,max(t.p1.xyz,t.p2.xyz));
    if any((o<=lo)&(d<vec3<f32>(0.0))) || any((o>=hi)&(d>vec3<f32>(0.0))) { return false; }
    if any((d==vec3<f32>(0.0))&((o<lo)|(o>hi))) { return false; }
    return true;
}
fn triangle_ahead_precise(t: Triangle, o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>) -> bool {
    if all(o_low==vec3<f32>(0.0)) { return triangle_ahead(t,o,d); }
    let lo=precise_point_delta(min(t.p0.xyz,min(t.p1.xyz,t.p2.xyz)),vec3<f32>(0.0),o,o_low);
    let hi=precise_point_delta(max(t.p0.xyz,max(t.p1.xyz,t.p2.xyz)),vec3<f32>(0.0),o,o_low);
    if any((lo>=vec3<f32>(0.0))&(d<vec3<f32>(0.0))) || any((hi<=vec3<f32>(0.0))&(d>vec3<f32>(0.0))) { return false; }
    if any((d==vec3<f32>(0.0))&((lo>vec3<f32>(0.0))|(hi<vec3<f32>(0.0)))) { return false; }
    return true;
}
// The host keeps studio_weights.w at positive zero. The integer dependency
// materializes scalar intermediates and prevents floating reassociation.
fn split_materialize(value: f32) -> f32 {
    return bitcast<f32>(bitcast<u32>(value)^bitcast<u32>(u.studio_weights.w));
}
struct SplitScaled { value: f32, exponent: i32 };
fn split_scaled(value: f32) -> SplitScaled {
    let bits=bitcast<u32>(value);let exponent=(bits>>23u)&255u;let fraction=bits&0x7fffffu;
    if (bits&0x7fffffffu)==0u || exponent==255u { return SplitScaled(value,0); }
    let sig=fraction|select(0u,0x800000u,exponent!=0u);
    let shift=countLeadingZeros(sig)-8u;
    let normalized=(bits&0x80000000u)|0x3f800000u|((sig<<shift)&0x7fffffu);
    return SplitScaled(bitcast<f32>(normalized),select(-126,i32(exponent)-127,exponent!=0u)-i32(shift));
}
fn split_add(a: f32, b: f32) -> f32 {
    return bitcast<f32>(fp_add_bits(bitcast<u32>(a),bitcast<u32>(b)));
}
// With normalized operands, all four12-by12-bit partial products and these
// subtractions are exactly representable, even if p chose either faithful neighbor.
fn split_product(a: f32, b: f32) -> vec2<f32> {
    let ah=bitcast<f32>(bitcast<u32>(a)&0xfffff000u);let al=split_materialize(a-ah);
    let bh=bitcast<f32>(bitcast<u32>(b)&0xfffff000u);let bl=split_materialize(b-bh);
    let p=split_materialize(a*b);
    let e1=split_materialize(p-ah*bh);
    let e2=split_materialize(e1-al*bh);
    let e3=split_materialize(e2-ah*bl);
    return vec2<f32>(p,split_materialize(al*bl-e3));
}
fn split_two_sum(a: f32, b: f32) -> vec2<f32> {
    let swap=abs(a)<abs(b);let large=select(a,b,swap);let small=select(b,a,swap);
    let sum=split_add(large,small);
    let delta=split_materialize(sum-large);
    return vec2<f32>(sum,split_materialize(small-delta));
}
fn split_rescale(value: f32, exponent: i32) -> f32 {
    if value==0.0 { return value; }
    let scaled=split_scaled(value);let combined=scaled.exponent+exponent+1;
    if combined>128 { return bitcast<f32>((bitcast<u32>(value)&0x80000000u)|0x7f800000u); }
    return ldexp(scaled.value*0.5,combined);
}
fn split_difference_products(a: f32, b: f32, c: f32, d: f32) -> f32 {
    let aa=split_scaled(a);let bb=split_scaled(b);let cc=split_scaled(c);let dd=split_scaled(d);
    let p=split_product(aa.value,bb.value);let q=split_product(cc.value,dd.value);
    let ep=aa.exponent+bb.exponent;let eq=cc.exponent+dd.exponent;
    if p.x==0.0 { return split_rescale(-split_add(q.x,q.y),eq); }
    if q.x==0.0 { return split_rescale(split_add(p.x,p.y),ep); }
    let exponent=max(ep,eq);
    let pp=ldexp(p,vec2<i32>(ep-exponent));let qq=ldexp(q,vec2<i32>(eq-exponent));
    let high=split_materialize(pp.x-qq.x);
    let tail=split_two_sum(pp.y,-qq.y);
    if high==0.0 { return split_rescale(tail.x,exponent); }
    if tail.x==0.0 { return split_rescale(high,exponent); }
    let total=split_two_sum(high,tail.x);
    return split_rescale(split_add(total.x,split_add(total.y,tail.y)),exponent);
}
fn difference_products(a: f32, b: f32, c: f32, d: f32) -> f32 {
    let ab=a*b;let cd=c*d;
    // Four cancelled leading bits select the compensated method; this is an
    // arithmetic strategy, not a change to any intersection tolerance.
    if abs(ab-cd)>max(abs(ab),abs(cd))*(1.0/16.0) {
        let error=fma(-c,d,cd);
        return fma(a,b,-cd)+error;
    }
    return split_difference_products(a,b,c,d);
}
struct Hit { distance: f32, triangle: u32, bary: vec3<f32> };
// A closed continuation that leaves its producing triangle at a shared edge or
// vertex can meet an adjacent face of the same body within the origin's rounding
// error, crossing in the direction it just crossed (an entry while inside, an
// exit while outside). That is the same interface crossing seen again, not a
// medium change. Consistent crossings, however short, are always kept.
fn repeated_crossing(t: Triangle, source: Triangle, d: vec3<f32>, inside: bool, distance: f32, tolerance: f32) -> bool {
    if distance>tolerance || t.base.w<=0.0 || t.parameters.w>=0.0 || t.optics.w!=source.optics.w { return false; }
    let a=array<vec3<f32>,3>(t.p0.xyz,t.p1.xyz,t.p2.xyz);
    let b=array<vec3<f32>,3>(source.p0.xyz,source.p1.xyz,source.p2.xyz);
    var adjacent=false;
    for (var i=0u; i<3u; i+=1u) { for (var j=0u; j<3u; j+=1u) { adjacent = adjacent || all(a[i]==b[j]); } }
    let front=dot(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz),d)<0.0;
    return adjacent && front==inside;
}
// Hits are ordered by the unscaled dominant-axis depth: scaling by 1/d[kz]
// can round a sharp-rim entry and exit to one distance. On an exact depth tie
// prefer the face that changes the current medium state: the entry from
// outside, the exit from inside. Traversal order must not choose it.
fn tie_preferred(t: Triangle, d: vec3<f32>, inside: bool) -> bool {
    let front=dot(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz),d)<0.0;
    return front!=inside;
}
fn trace_precise(o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>, any_hit: bool, excluded: u32, minimum: f32, inside: bool) -> Hit {
    if all(o_low==vec3<f32>(0.0)) { return trace_ordered_within(o,d,any_hit,excluded,minimum,inside,FAR,-1.0); }
    return trace_precise_within(o,o_low,d,any_hit,excluded,minimum,inside,FAR,-1.0);
}
// Only hits closer than `limit` are reported, and nodes beyond it are culled.
// An opaque or shadow-blocking hit closer than `blocker` ends traversal at once:
// a shadow connection is blocked whichever such hit is closest.
fn trace_precise_within(o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>, any_hit: bool, excluded: u32, minimum: f32, inside: bool, limit: f32, blocker: f32) -> Hit {
    // One shared walk bounds native inlining for primary, surface and volume
    // connections. The ordinary branch retains its original candidate arithmetic;
    // only a retained low origin selects the compensated candidate policy.
    let has_origin_residual=any(o_low!=vec3<f32>(0.0));
    var hit = Hit(limit, 0xffffffffu, vec3<f32>(0.0));
    // Project every candidate into the same ray-aligned 2D coordinate system.
    // Shared edges then use identical products with opposite signs, avoiding the
    // independently rounded barycentric comparisons of Moller-Trumbore.
    var kz = 0u;
    if abs(d.y)>abs(d.x) { kz=1u; }
    if abs(d.z)>abs(d[kz]) { kz=2u; }
    let kx=(kz+1u)%3u; let ky=(kx+1u)%3u;
    let shear=vec3<f32>(d[kx]/d[kz],d[ky]/d[kz],1.0/d[kz]);
    let order=select(-1.0,1.0,d[kz]>0.0);
    var best=FAR;
    // Only candidates near the origin need the integer-defined deltas. The
    // rounded origin is within a few ULPs of the true anchor plus offset, so
    // boxes grown by that bound reject conservatively: every point the true
    // ray reaches at depth t lies in the grown box along the rounded ray at
    // the same t. A triangle whose bounds are farther from the rounded origin
    // than the near radius uses ordinary arithmetic from it: that error cannot
    // reorder such a hit, but it can move the ray across an edge, so decisions
    // within the error bound of an edge or bound fall back to the exact path.
    var rounded=o;
    if has_origin_residual { rounded=o+o_low; }
    let scale=max(max(abs(rounded.x),max(abs(rounded.y),abs(rounded.z))),1.0);
    let origin_error=scale*(1.0/1048576.0);
    var pad=u.lighting.x;
    if has_origin_residual { pad=u.lighting.x+origin_error; }
    var source: Triangle;
    if excluded!=0xffffffffu { source=triangle(excluded); }
    let near_radius=scale*(1.0/256.0);
    // Children are tested at their parent and visited near-first. The build
    // bounds tree depth to 30, so at most 31 nodes are pending.
    var stack: array<u32, 32>;
    var entry: array<f32, 32>;
    if nodes[0].first == 0xffffffffu { return hit; }
    entry[0] = box_entry_padded(nodes[0], rounded, d, hit.distance, pad);
    if entry[0] < 0.0 { return hit; }
    stack[0] = 0u;
    var size = 1u;
    loop {
        if size == 0u { break; }
        size -= 1u;
        if entry[size] > hit.distance { continue; }
        let node = nodes[stack[size]];
        if node.count == 0u {
            if size + 2u > 32u { break; }
            let left = box_entry_padded(nodes[node.first], rounded, d, hit.distance, pad);
            let right = box_entry_padded(nodes[node.first+1u], rounded, d, hit.distance, pad);
            var near_child = node.first; var near_entry = left;
            var far_child = node.first+1u; var far_entry = right;
            if right >= 0.0 && (left < 0.0 || right < left) {
                near_child = node.first+1u; near_entry = right;
                far_child = node.first; far_entry = left;
            }
            if far_entry >= 0.0 { stack[size] = far_child; entry[size] = far_entry; size += 1u; }
            if near_entry >= 0.0 { stack[size] = near_child; entry[size] = near_entry; size += 1u; }
            continue;
        }
        for (var i = node.first; i < node.first + node.count; i += 1u) {
            if i == excluded { continue; }
            // A straight continuation cannot revisit its producing exact plane.
            // IDs join only connected, exactly coplanar uploaded f32 triangles
            // of one body; curved facets and other bodies remain candidates.
            if excluded!=0xffffffffu && geometry[geometry[3]+i]==geometry[geometry[3]+excluded] { continue; }
            // The ordinary source uses exactly the former ordered candidate
            // branch. Sharing only the node walk avoids another inlined BVH.
            if !has_origin_residual {
                let t = triangle(i);
                if !triangle_ahead(t,o,d) { continue; }
                let pa=t.p0.xyz-o; let pb=t.p1.xyz-o; let pc=t.p2.xyz-o;
                let a=vec2<f32>(pa[kx]-shear.x*pa[kz],pa[ky]-shear.y*pa[kz]);
                let b=vec2<f32>(pb[kx]-shear.x*pb[kz],pb[ky]-shear.y*pb[kz]);
                let c=vec2<f32>(pc[kx]-shear.x*pc[kz],pc[ky]-shear.y*pc[kz]);
                let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
                if any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)) { continue; }
                let det=weights.x+weights.y+weights.z;
                if det==0.0 { continue; }
                // Retain all three directly evaluated weights: 1-u-v can erase a
                // small corner clearance when the other two weights sum to one.
                let bary=weights/det;
                let depth=surface_ray_depth(t,bary,o,kz);
                let distance=depth*shear.z;
                let key=depth*order;
                if excluded!=0xffffffffu && repeated_crossing(t,source,d,inside,distance,origin_error) { continue; }
                if distance > minimum && distance < limit && (key < best || (key == best && tie_preferred(t,d,inside))) {
                    hit = Hit(distance,i,bary); best=key;
                    if any_hit || (distance < blocker && (t.base.w<=0.0 || t.coat.w<=0.0)) { return hit; }
                }
                continue;
            }
            let t = triangle(i);
            let lo=min(t.p0.xyz,min(t.p1.xyz,t.p2.xyz));
            let hi=max(t.p0.xyz,max(t.p1.xyz,t.p2.xyz));
            let gap=max(lo-rounded,rounded-hi);
            if any(gap>vec3<f32>(near_radius)) {
                // The rounded origin is within origin_error of the true one on each
                // axis. Bound and edge decisions that close to their threshold could
                // flip, while a nearby neighbour across the edge is tested exactly:
                // a crossing would fall between them. Such candidates take the exact
                // path below.
                if any((rounded<=lo-vec3<f32>(origin_error))&(d<vec3<f32>(0.0))) || any((rounded>=hi+vec3<f32>(origin_error))&(d>vec3<f32>(0.0))) { continue; }
                if any((d==vec3<f32>(0.0))&((rounded<lo-vec3<f32>(origin_error))|(rounded>hi+vec3<f32>(origin_error)))) { continue; }
                let qa=t.p0.xyz-rounded; let qb=t.p1.xyz-rounded; let qc=t.p2.xyz-rounded;
                let fa=vec2<f32>(qa[kx]-shear.x*qa[kz],qa[ky]-shear.y*qa[kz]);
                let fb=vec2<f32>(qb[kx]-shear.x*qb[kz],qb[ky]-shear.y*qb[kz]);
                let fc=vec2<f32>(qc[kx]-shear.x*qc[kz],qc[ky]-shear.y*qc[kz]);
                let far_weights=vec3<f32>(difference_products(fb.x,fc.y,fb.y,fc.x),difference_products(fc.x,fa.y,fc.y,fa.x),difference_products(fa.x,fb.y,fa.y,fb.x));
                let edge_error=origin_error*vec3<f32>(length(fb-fc),length(fc-fa),length(fa-fb));
                if all(abs(far_weights)>edge_error) {
                    if any(far_weights<vec3<f32>(0.0)) && any(far_weights>vec3<f32>(0.0)) { continue; }
                    let far_det=far_weights.x+far_weights.y+far_weights.z;
                    let far_bary=far_weights/far_det;
                    let far_depth=surface_ray_depth(t,far_bary,rounded,kz);
                    let far_distance=far_depth*shear.z;
                    let far_key=far_depth*order;
                    if far_distance > minimum && far_distance < limit && (far_key < best || (far_key == best && tie_preferred(t,d,inside))) {
                        hit = Hit(far_distance,i,far_bary); best=far_key;
                        if any_hit || (far_distance < blocker && (t.base.w<=0.0 || t.coat.w<=0.0)) { return hit; }
                    }
                    continue;
                }
            }
            if !triangle_ahead_precise(t,o,o_low,d) { continue; }
            let pa=precise_point_delta(t.p0.xyz,vec3<f32>(0.0),o,o_low);
            let pb=precise_point_delta(t.p1.xyz,vec3<f32>(0.0),o,o_low);
            let pc=precise_point_delta(t.p2.xyz,vec3<f32>(0.0),o,o_low);
            let a=vec2<f32>(pa[kx]-shear.x*pa[kz],pa[ky]-shear.y*pa[kz]);
            let b=vec2<f32>(pb[kx]-shear.x*pb[kz],pb[ky]-shear.y*pb[kz]);
            let c=vec2<f32>(pc[kx]-shear.x*pc[kz],pc[ky]-shear.y*pc[kz]);
            let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
            if any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)) { continue; }
            let det=weights.x+weights.y+weights.z;
            if det==0.0 { continue; }
            // Retain all three directly evaluated weights: 1-u-v can erase a
            // small corner clearance when the other two weights sum to one.
            let bary=weights/det;
            let depth=surface_ray_depth_precise(t,bary,o,o_low,kz);
            let distance=depth*shear.z;
            let key=depth*order;
            if excluded!=0xffffffffu && repeated_crossing(t,source,d,inside,distance,origin_error) { continue; }
            if distance > minimum && distance < limit && (key < best || (key == best && tie_preferred(t,d,inside))) {
                hit = Hit(distance,i,bary); best=key;
                if any_hit || (distance < blocker && (t.base.w<=0.0 || t.coat.w<=0.0)) { return hit; }
            }
        }
    }
    return hit;
}

fn trace_bounded(o: vec3<f32>, d: vec3<f32>, any_hit: bool, excluded: u32, minimum: f32) -> Hit {
    return trace_ordered_within(o,d,any_hit,excluded,minimum,false,FAR,-1.0);
}
fn trace_ordered_within(o: vec3<f32>, d: vec3<f32>, any_hit: bool, excluded: u32, minimum: f32, inside: bool, limit: f32, blocker: f32) -> Hit {
    var hit = Hit(limit, 0xffffffffu, vec3<f32>(0.0));
    let origin_error=max(max(abs(o.x),max(abs(o.y),abs(o.z))),1.0)*(1.0/1048576.0);
    var source: Triangle;
    if excluded!=0xffffffffu { source=triangle(excluded); }
    // Project every candidate into the same ray-aligned 2D coordinate system.
    // Shared edges then use identical products with opposite signs, avoiding the
    // independently rounded barycentric comparisons of Moller-Trumbore.
    var kz = 0u;
    if abs(d.y)>abs(d.x) { kz=1u; }
    if abs(d.z)>abs(d[kz]) { kz=2u; }
    let kx=(kz+1u)%3u; let ky=(kx+1u)%3u;
    let shear=vec3<f32>(d[kx]/d[kz],d[ky]/d[kz],1.0/d[kz]);
    let order=select(-1.0,1.0,d[kz]>0.0);
    var best=FAR;
    // Children are tested at their parent and visited near-first. The build
    // bounds tree depth to 30, so at most 31 nodes are pending.
    var stack: array<u32, 32>;
    var entry: array<f32, 32>;
    if nodes[0].first == 0xffffffffu { return hit; }
    entry[0] = box_entry_padded(nodes[0], o, d, hit.distance, u.lighting.x);
    if entry[0] < 0.0 { return hit; }
    stack[0] = 0u;
    var size = 1u;
    loop {
        if size == 0u { break; }
        size -= 1u;
        if entry[size] > hit.distance { continue; }
        let node = nodes[stack[size]];
        if node.count == 0u {
            if size + 2u > 32u { break; }
            let left = box_entry_padded(nodes[node.first], o, d, hit.distance, u.lighting.x);
            let right = box_entry_padded(nodes[node.first+1u], o, d, hit.distance, u.lighting.x);
            var near_child = node.first; var near_entry = left;
            var far_child = node.first+1u; var far_entry = right;
            if right >= 0.0 && (left < 0.0 || right < left) {
                near_child = node.first+1u; near_entry = right;
                far_child = node.first; far_entry = left;
            }
            if far_entry >= 0.0 { stack[size] = far_child; entry[size] = far_entry; size += 1u; }
            if near_entry >= 0.0 { stack[size] = near_child; entry[size] = near_entry; size += 1u; }
            continue;
        }
        for (var i = node.first; i < node.first + node.count; i += 1u) {
            if i == excluded { continue; }
            // A straight continuation cannot revisit its producing exact plane.
            // IDs join only connected, exactly coplanar uploaded f32 triangles
            // of one body; curved facets and other bodies remain candidates.
            if excluded!=0xffffffffu && geometry[geometry[3]+i]==geometry[geometry[3]+excluded] { continue; }
            let t = triangle(i);
            if !triangle_ahead(t,o,d) { continue; }
            let pa=t.p0.xyz-o; let pb=t.p1.xyz-o; let pc=t.p2.xyz-o;
            let a=vec2<f32>(pa[kx]-shear.x*pa[kz],pa[ky]-shear.y*pa[kz]);
            let b=vec2<f32>(pb[kx]-shear.x*pb[kz],pb[ky]-shear.y*pb[kz]);
            let c=vec2<f32>(pc[kx]-shear.x*pc[kz],pc[ky]-shear.y*pc[kz]);
            let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
            if any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)) { continue; }
            let det=weights.x+weights.y+weights.z;
            if det==0.0 { continue; }
            // Retain all three directly evaluated weights: 1-u-v can erase a
            // small corner clearance when the other two weights sum to one.
            let bary=weights/det;
            let depth=surface_ray_depth(t,bary,o,kz);
            let distance=depth*shear.z;
            let key=depth*order;
            if excluded!=0xffffffffu && repeated_crossing(t,source,d,inside,distance,origin_error) { continue; }
            if distance > minimum && distance < limit && (key < best || (key == best && tie_preferred(t,d,inside))) {
                hit = Hit(distance,i,bary); best=key;
                if any_hit || (distance < blocker && (t.base.w<=0.0 || t.coat.w<=0.0)) { return hit; }
            }
        }
    }
    return hit;
}
fn trace(o: vec3<f32>, d: vec3<f32>, any_hit: bool) -> Hit {
    return trace_bounded(o,d,any_hit,0xffffffffu,u.lighting.x);
}
// Origin classification is separate from visible/shadow traversal. A nearer
// opaque triangle, thin sheet or analytic emitter cannot hide a closed boundary.
// This uses the existing uploaded-f32 intersection domain and no launch epsilon:
// exact numeric contacts, silhouette edges/vertices and tied nearest boundaries
// are ambiguous certificates, not an invitation to guess a containing medium.
struct OriginBoundary { hit: Hit, valid: bool };
struct OriginEdge { start: vec3<f32>, end: vec3<f32> };
fn origin_edge(t: Triangle, zero: u32) -> OriginEdge {
    if zero==0u { return OriginEdge(t.p1.xyz,t.p2.xyz); }
    if zero==1u { return OriginEdge(t.p2.xyz,t.p0.xyz); }
    return OriginEdge(t.p0.xyz,t.p1.xyz);
}
fn origin_internal_edge(a: Triangle, ai: u32, az: u32, b: Triangle, bi: u32, bz: u32, d: vec3<f32>) -> bool {
    if geometry[geometry[3]+ai]!=geometry[geometry[3]+bi] { return false; }
    // The host patch proves connected exact uploaded-f32 coplanarity in one
    // body. Still require the same actual region/parent/medium coefficients.
    if a.parameters.z!=b.parameters.z || any(a.optics!=b.optics) || any(a.interior!=b.interior) { return false; }
    let ae=origin_edge(a,az);let be=origin_edge(b,bz);
    if any(ae.start!=be.end) || any(ae.end!=be.start) { return false; }
    let at=dot(cross(a.p1.xyz-a.p0.xyz,a.p2.xyz-a.p0.xyz),d);
    let bt=dot(cross(b.p1.xyz-b.p0.xyz,b.p2.xyz-b.p0.xyz),d);
    return at!=0.0 && bt!=0.0 && (at<0.0)==(bt<0.0);
}
fn origin_on_boundary(t: Triangle, o: vec3<f32>) -> bool {
    let lo=min(t.p0.xyz,min(t.p1.xyz,t.p2.xyz));
    let hi=max(t.p0.xyz,max(t.p1.xyz,t.p2.xyz));
    if any(o<lo) || any(o>hi) { return false; }
    let normal=cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz);
    if all(normal==vec3<f32>(0.0)) { return true; }
    if dot(normal,o-t.p0.xyz)!=0.0 { return false; }
    var axis=0u;
    if abs(normal.y)>abs(normal.x) { axis=1u; }
    if abs(normal.z)>abs(normal[axis]) { axis=2u; }
    let x=(axis+1u)%3u;let y=(x+1u)%3u;
    let pa=t.p0.xyz-o;let pb=t.p1.xyz-o;let pc=t.p2.xyz-o;
    let a=vec2<f32>(pa[x],pa[y]);let b=vec2<f32>(pb[x],pb[y]);let c=vec2<f32>(pc[x],pc[y]);
    let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
    return !(any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)));
}
fn origin_boundary(o: vec3<f32>, d: vec3<f32>) -> OriginBoundary {
    var hit=Hit(FAR,0xffffffffu,vec3<f32>(0.0));
    var best=FAR;
    var ambiguous=FAR;
    var edge_index=3u;
    var edge_count=0u;
    var kz=0u;
    if abs(d.y)>abs(d.x) { kz=1u; }
    if abs(d.z)>abs(d[kz]) { kz=2u; }
    let kx=(kz+1u)%3u;let ky=(kx+1u)%3u;
    let inverse_depth=1.0/d[kz];
    let order=select(-1.0,1.0,d[kz]>0.0);
    // The same host-built depth<=30 BVH bounds both visible and origin queries.
    // These fixed arrays replace no image/buffer and require no storage upload.
    var stack: array<u32,32>;
    var entry: array<f32,32>;
    if nodes[0].first==0xffffffffu { return OriginBoundary(hit,true); }
    entry[0]=box_entry_padded(nodes[0],o,d,FAR,u.lighting.x);
    if entry[0]<0.0 { return OriginBoundary(hit,true); }
    // Reuse this already-computed bound: an outside origin is vacuum, and
    // entering even the padded root after the primary minimum proves its
    // launch interval clear. Near-boundary origins still take the full query.
    if entry[0]>u.lighting.x && (any(o<nodes[0].lo) || any(o>nodes[0].hi)) {return OriginBoundary(hit,true);}
    stack[0]=0u;var size=1u;
    loop {
        if size==0u { break; }
        size-=1u;
        if entry[size]>hit.distance { continue; }
        let node=nodes[stack[size]];
        if node.count==0u {
            if size+2u>32u { return OriginBoundary(hit,false); }
            let left=box_entry_padded(nodes[node.first],o,d,hit.distance,u.lighting.x);
            let right=box_entry_padded(nodes[node.first+1u],o,d,hit.distance,u.lighting.x);
            var near_child=node.first;var near_entry=left;
            var far_child=node.first+1u;var far_entry=right;
            if right>=0.0 && (left<0.0 || right<left) {
                near_child=node.first+1u;near_entry=right;
                far_child=node.first;far_entry=left;
            }
            if far_entry>=0.0 { stack[size]=far_child;entry[size]=far_entry;size+=1u; }
            if near_entry>=0.0 { stack[size]=near_child;entry[size]=near_entry;size+=1u; }
            continue;
        }
        for(var i=node.first;i<node.first+node.count;i+=1u) {
            let t=triangle(i);
            if t.base.w<=0.0 || t.parameters.w>=0.0 { continue; }
            // Check the actual point before the forward-ray fast rejection. A
            // boundary origin moving away must not be assigned vacuum by it.
            if origin_on_boundary(t,o) { return OriginBoundary(hit,false); }
            if !triangle_ahead(t,o,d) { continue; }
            let pa=t.p0.xyz-o;let pb=t.p1.xyz-o;let pc=t.p2.xyz-o;
            // Keep the ray projection homogeneous: a rounded d[kx]/d[kz]
            // must not turn an exact uploaded-f32 edge tangent into a crossing.
            // All projected coordinates share d[kz], so barycentrics are invariant.
            let a=vec2<f32>(difference_products(pa[kx],d[kz],d[kx],pa[kz]),difference_products(pa[ky],d[kz],d[ky],pa[kz]));
            let b=vec2<f32>(difference_products(pb[kx],d[kz],d[kx],pb[kz]),difference_products(pb[ky],d[kz],d[ky],pb[kz]));
            let c=vec2<f32>(difference_products(pc[kx],d[kz],d[kx],pc[kz]),difference_products(pc[ky],d[kz],d[ky],pc[kz]));
            let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
            if any(weights<vec3<f32>(0.0)) && any(weights>vec3<f32>(0.0)) { continue; }
            let det=weights.x+weights.y+weights.z;
            if det==0.0 { continue; }
            let bary=weights/det;
            let depth=surface_ray_depth(t,bary,o,kz);
            let distance=depth*inverse_depth;
            let key=depth*order;
            if distance==0.0 { return OriginBoundary(hit,false); }
            if distance<0.0 || distance>=FAR { continue; }
            let transverse=dot(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz),d);
            let zeros=select(0u,1u,weights.x==0.0)+select(0u,1u,weights.y==0.0)+select(0u,1u,weights.z==0.0);
            // A vertex needs a complete planar fan proof, which this query does
            // not infer. Tangencies and actual boundary origins stay refusals.
            if zeros>1u || transverse==0.0 {
                ambiguous=min(ambiguous,key);
                continue;
            }
            var zero=3u;
            if zeros==1u {
                if weights.x==0.0 { zero=0u; }
                else if weights.y==0.0 { zero=1u; }
                else { zero=2u; }
            }
            if key==best {
                // Only the exact opposite directed edge of two certified
                // coplanar triangles proves an internal triangulation edge.
                // Two arbitrary same-patch hits, perimeter edges and key-near
                // interfaces are insufficient; there is no distance tolerance.
                if edge_count==1u && zero<3u && origin_internal_edge(triangle(hit.triangle),hit.triangle,edge_index,t,i,zero,d) {
                    edge_count=2u;
                } else { ambiguous=min(ambiguous,key); }
            }
            if key<best {
                hit=Hit(distance,i,bary);best=key;edge_index=zero;
                edge_count=select(0u,1u,zero<3u);
            }
        }
    }
    // The ordinary primary uses this existing lower interval bound. A real
    // interface inside it cannot be skipped and treated as constant air/glass.
    let clipped=hit.triangle!=0xffffffffu && hit.distance<=u.lighting.x;
    return OriginBoundary(hit,!(ambiguous<FAR && ambiguous<=best) && edge_count!=1u && !clipped);
}


fn dominant_vertex(bary: vec3<f32>) -> u32 {
    var anchor=0u;
    if bary.y>bary.x { anchor=1u; }
    if bary.z>bary[anchor] { anchor=2u; }
    return anchor;
}
// The vertex after `a`, as (a+1)%3. Not written with `%`: NVIDIA's Vulkan compiler crashes on
// that index after a BVH walk (#6705).
fn next_vertex(a: u32) -> u32 {
    return select(a+1u,0u,a>=2u);
}
fn surface_point(t: Triangle, bary: vec3<f32>) -> vec3<f32> {
    // Anchor near the hit, retaining the two other directly computed weights.
    // A tiny weight on a distant vertex can encode an entire corner clearance.
    // Do not recover that weight by subtracting the other two from one.
    let p=array<vec3<f32>,3>(t.p0.xyz,t.p1.xyz,t.p2.xyz);
    let a=dominant_vertex(bary); let b=next_vertex(a); let c=next_vertex(b);
    return p[a]+((p[b]-p[a])*bary[b]+(p[c]-p[a])*bary[c]);
}
fn surface_point_precise(t: Triangle, bary: vec3<f32>) -> PrecisePoint {
    let p=array<vec3<f32>,3>(t.p0.xyz,t.p1.xyz,t.p2.xyz);
    let a=dominant_vertex(bary); let b=next_vertex(a); let c=next_vertex(b);
    // Keep the world anchor separate from its local displacement.
    return PrecisePoint(p[a],(p[b]-p[a])*bary[b]+(p[c]-p[a])*bary[c]);
}
fn surface_ray_depth_precise(t: Triangle, bary: vec3<f32>, origin: vec3<f32>, origin_low: vec3<f32>, axis: u32) -> f32 {
    if all(origin_low==vec3<f32>(0.0)) { return surface_ray_depth(t,bary,origin,axis); }
    let point=surface_point_precise(t,bary);
    return anchored_delta_component(point.hi[axis],point.lo[axis],origin[axis],origin_low[axis]);
}
fn surface_ray_depth(t: Triangle, bary: vec3<f32>, origin: vec3<f32>, axis: u32) -> f32 {
    let p=array<vec3<f32>,3>(t.p0.xyz,t.p1.xyz,t.p2.xyz);
    let a=dominant_vertex(bary); let b=next_vertex(a); let c=next_vertex(b);
    // Original vertex differences avoid subtracting two rounded ray-relative
    // coordinates; adding the anchor last preserves distant thin-hit ordering.
    return (p[a][axis]-origin[axis])+((p[b][axis]-p[a][axis])*bary[b]+(p[c][axis]-p[a][axis])*bary[c]);
}

fn from_local(p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let axis = select(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0,1.0,0.0),abs(n.z)>0.9);
    let x = unit(cross(axis,n));
    return unit(x*p.x + cross(n,x)*p.y + n*p.z);
}
fn sample_surface(t: Triangle, n: vec3<f32>, v: vec3<f32>, rng: ptr<function,u32>) -> vec3<f32> {
    let component = random(rng); let x = random(rng); let y = random(rng);
    return sample_surface_values(t,n,v,vec3<f32>(component,x,y));
}
// Conditional opaque mixture density; branch selection is accounted for by callers.
fn surface_pdf(t: Triangle, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>) -> f32 {
    let nl = dot(n,l);
    if nl<=0.0 || dot(n,v)<=0.0 { return 0.0; }
    let h = unit(v+l); let vh = max(dot(v,h),1e-20);
    let nh = max(dot(n,h),0.0);
    let alpha = t.parameters.y*t.parameters.y;
    let base = 0.5*nl/PI+0.5*distribution(nh,alpha*alpha)*nh/(4.0*vh);
    if t.coat.x<=0.0 { return base; }
    let coat_alpha = t.coat.y*t.coat.y;
    let coat_pdf = distribution(nh,coat_alpha*coat_alpha)*nh/(4.0*vh);
    return (1.0-t.coat.x*0.5)*base+t.coat.x*0.5*coat_pdf;
}
fn power_weight(a: f32, b: f32) -> f32 {
    // Ratio form avoids overflow for grazing/very distant rectangular samples.
    if a <= 0.0 { return 0.0; }
    let ratio = b/a;
    return 1.0/(1.0+ratio*ratio);
}
struct EmitterHit { distance: f32, index: u32 };
fn emitter_hit_precise(o: vec3<f32>, o_low: vec3<f32>, d: vec3<f32>, minimum: f32) -> EmitterHit {
    var hit = EmitterHit(FAR,0xffffffffu);
    for (var i=0u; i<u32(u.light_count.w); i+=1u) {
        let light = u.area_lights[i];
        let normal = unit(cross(light.u.xyz,light.v.xyz));
        let cosine = dot(normal,d);
        if abs(cosine)<1e-8 { continue; }
        let to_light=precise_point_delta(light.center.xyz,vec3<f32>(0.0),o,o_low);
        let distance = dot(to_light,normal)/cosine;
        if distance <= minimum || distance >= hit.distance { continue; }
        var q = o+d*distance-light.center.xyz;
        if any(o_low!=vec3<f32>(0.0)) { q=fma(d,vec3<f32>(distance),-to_light); }
        if abs(dot(q,light.u.xyz))<=dot(light.u.xyz,light.u.xyz) && abs(dot(q,light.v.xyz))<=dot(light.v.xyz,light.v.xyz) {
            hit = EmitterHit(distance,i);
        }
    }
    return hit;
}
fn emitter_hit_bounded(o: vec3<f32>, d: vec3<f32>, minimum: f32) -> EmitterHit {
    return emitter_hit_precise(o,vec3<f32>(0.0),d,minimum);
}
fn emitter_hit(o: vec3<f32>, d: vec3<f32>) -> EmitterHit {
    return emitter_hit_bounded(o,d,u.lighting.x);
}

fn area_pdf(light: AreaLight, delta: vec3<f32>) -> f32 {
    let cosine = dot(unit(cross(light.u.xyz,light.v.xyz)),-unit(delta));
    if cosine<=0.0 { return 0.0; }
    return dot(delta,delta)/(light.center.w*cosine);
}
struct EnvironmentSample { direction: vec3<f32>, radiance: vec3<f32>, pdf: f32 };
fn environment_to_world(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(u.environment_map.x*d.x-u.environment_map.y*d.y,u.environment_map.y*d.x+u.environment_map.x*d.y,d.z);
}
fn environment_index(d: vec3<f32>) -> u32 {
    let local=vec3<f32>(u.environment_map.x*d.x+u.environment_map.y*d.y,-u.environment_map.y*d.x+u.environment_map.x*d.y,d.z);
    var phi=atan2(local.y,local.x); if phi<0.0 {phi+=2.0*PI;}
    let dims=environment_data.header.xy;
    let x=min(u32(phi/(2.0*PI)*f32(dims.x)),dims.x-1u);
    let y=min(u32(acos(clamp(local.z,-1.0,1.0))/PI*f32(dims.y)),dims.y-1u);
    return y*dims.x+x;
}
fn env_eval(d: vec3<f32>) -> vec3<f32> {
    if u.environment_map.w>0.0 {return environment_data.texels[environment_index(d)].rgb*u.environment_map.z;}
    return u.environment.xyz;
}
fn env_pdf(d: vec3<f32>) -> f32 {
    if u.environment_map.w>0.0 {return environment_data.texels[environment_index(d)].pdf;}
    return 1.0/(4.0*PI);
}
fn env_sample(rng: ptr<function,u32>) -> EnvironmentSample {
    if u.environment_map.w>0.0 {
        let dims=environment_data.header.xy;
        let choice=random(rng)*f32(environment_data.header.z);
        let initial=min(u32(choice),environment_data.header.z-1u);
        let entry=environment_data.texels[initial];
        let index=select(entry.alternate,initial,random(rng)<entry.threshold);
        let texel=environment_data.texels[index];
        let row=index/dims.x; let column=index%dims.x;
        let top=cos(PI*f32(row)/f32(dims.y)); let bottom=cos(PI*f32(row+1u)/f32(dims.y));
        let z=mix(top,bottom,random(rng));
        let phi=2.0*PI*(f32(column)+random(rng))/f32(dims.x);
        let radius=sqrt(max(0.0,1.0-z*z));
        let direction=environment_to_world(vec3<f32>(radius*cos(phi),radius*sin(phi),z));
        return EnvironmentSample(direction,texel.rgb*u.environment_map.z,texel.pdf);
    }
    let z=1.0-2.0*random(rng);let phi=2.0*PI*random(rng);
    let radius=sqrt(max(0.0,1.0-z*z));
    return EnvironmentSample(vec3<f32>(radius*cos(phi),radius*sin(phi),z),u.environment.xyz,1.0/(4.0*PI));
}
fn sun_pdf(d:vec3<f32>)->f32 {
    return select(0.0,u.sun_radiance.w,dot(d,u.sun_direction.xyz)>=u.sun_direction.w);
}
fn sun_eval(d:vec3<f32>)->vec3<f32> {
    return select(vec3<f32>(0.0),u.sun_radiance.xyz,sun_pdf(d)>0.0);
}
fn sun_sample(rng:ptr<function,u32>)->EnvironmentSample {
    // Derive 1-cos(radius) from the CPU's stable solid angle, not f32 subtraction.
    let z=1.0-random(rng)/(2.0*PI*u.sun_radiance.w);
    let phi=2.0*PI*random(rng);let radius=sqrt(max(0.0,1.0-z*z));
    let direction=from_local(vec3<f32>(radius*cos(phi),radius*sin(phi),z),u.sun_direction.xyz);
    return EnvironmentSample(direction,u.sun_radiance.xyz,u.sun_radiance.w);
}
// The three fixed lights of the preview without studio lighting: direction and intensity.
const FIXED_LIGHTS = array<vec4<f32>,3>(vec4<f32>(0.6,0.0,0.8,3.0),vec4<f32>(-0.8,0.0,0.6,1.0),vec4<f32>(0.0,-0.6,0.8,1.5));
// Next-event estimation for every light source family through ONE shadow-ray call site, like
// volume_direct. Native compilers inline each call of the BVH walk; one walk per source family
// made `main` too large for NVIDIA's Vulkan compiler (#6705). Studio lighting visits every area
// light, then the environment, then the sun, drawing random numbers in that order; hard
// directions follow without consuming random numbers. Without explicit lighting
// the three fixed lights are used.
fn direct_light(t: Triangle, position: vec3<f32>, physical: PrecisePoint, o: vec3<f32>, n: vec3<f32>, geometric: vec3<f32>, v: vec3<f32>, remaining: u32, rng: ptr<function,u32>) -> vec3<f32> {
    transport_connection_origin=physical.hi;transport_connection_origin_low=physical.lo;
    var result = vec3<f32>(0.0);
    let opaque = 1.0-t.base.w;
    let studio = u.environment.w > 0.0;
    if studio && opaque<=0.0 { return result; }
    let area_count = u32(u.light_count.w);
    let count = select(3u, area_count+2u+u32(u.light_count.x)+u32(u.light_count.y), studio);
    for (var source=0u; source<count; source+=1u) {
        var sample = EnvironmentSample(vec3<f32>(0.0,0.0,1.0),vec3<f32>(0.0),0.0);
        var shadow_direction = vec3<f32>(0.0,0.0,1.0);
        var shadow_distance = FAR;
        var shadow_origin = o;
        var shadow_origin_low = vec3<f32>(0.0);
        if transport_medium_id!=0.0 {shadow_origin=physical.hi;shadow_origin_low=physical.lo;}
        var punctual = false;
        var physical_distance = FAR;
        if !studio {
            let light = FIXED_LIGHTS[source];
            if u.lighting.z <= 0.0 || dot(geometric,light.xyz)<=0.0 || dot(n,light.xyz)<=0.0 { continue; }
            sample = EnvironmentSample(light.xyz,vec3<f32>(light.w*u.lighting.z*u.studio_weights[source]),1.0);
            shadow_direction = light.xyz;
        } else {
            if source<area_count {
                let light = u.area_lights[source];
                let point = light.center.xyz + light.u.xyz*(2.0*random(rng)-1.0) + light.v.xyz*(2.0*random(rng)-1.0);
                let delta = point-position;
                sample = EnvironmentSample(unit(delta),light.radiance.xyz,area_pdf(light,delta));
                let shadow = precise_point_delta(point,vec3<f32>(0.0),shadow_origin,shadow_origin_low);
                shadow_direction = unit(shadow);
                shadow_distance = length(shadow);
            } else if source==area_count {
                if u.environment_map.w<=0.0 && !any(u.environment.xyz>vec3<f32>(0.0)) { continue; }
                sample = env_sample(rng);
                shadow_direction = sample.direction;
            } else if source==area_count+1u {
                if u.sun_radiance.w<=0.0 { continue; }
                sample = sun_sample(rng);
                shadow_direction = sample.direction;
            } else if source<area_count+2u+u32(u.light_count.x) {
                let light = u.directional_lights[source-area_count-2u];
                if light.direction.w<=0.0 || !any(light.color.xyz>vec3<f32>(0.0)) { continue; }
                sample = EnvironmentSample(light.direction.xyz,light.color.xyz*light.direction.w,1.0);
                shadow_direction = sample.direction;
            } else {
                punctual = true;
                let light = u.punctual_lights[source-area_count-2u-u32(u.light_count.x)];
                let value = punctual_sample(light,physical);
                if value.omitted!=0u { continue; }
                sample = EnvironmentSample(value.direction,value.radiance,1.0);
                physical_distance = value.distance;
                var contribution=vec3<f32>(0.0);
                if dot(n,sample.direction)>0.0 && dot(geometric,sample.direction)>0.0 {
                    contribution=brdf(t,n,v,sample.direction)*(opaque*dot(n,sample.direction)*sample.radiance);
                }
                if !punctual_radiance_valid(contribution) { punctual_count(1u); continue; }
                // Irradiance uses the physical point; the robust connection is
                // separately re-aimed from an offset of that same precise point.
                let shifted=punctual_offset(physical,select(geometric*(u.lighting.x*2.0),vec3<f32>(0.0),transport_medium_id!=0.0));
                shadow_origin=shifted.hi;
                shadow_origin_low=shifted.lo;
                let connection=precise_point_delta(light.position.xyz,light.position_low.xyz,shadow_origin,shadow_origin_low);
                shadow_distance=length(connection);
                if !(shadow_distance>0.0) || dot(connection,sample.direction)<=0.0 {
                    punctual_count(2u); continue;
                }
                shadow_direction=connection/shadow_distance;
                shadow_distance=punctual_endpoint(shadow_distance);
            }
            if sample.pdf<=0.0 || dot(n,sample.direction)<=0.0 || dot(geometric,sample.direction)<=0.0 { continue; }
        }
        // Studio emitters block a connection that passes them; studio volumes attenuate it.
        var visibility = blocked_shadow();
        let shadow_minimum=select(u.lighting.x,0.0,punctual || transport_medium_id!=0.0);
        let emitter_limit=shadow_distance-select(u.lighting.x*2.0,0.0,punctual);
        if !studio || emitter_hit_precise(shadow_origin,shadow_origin_low,shadow_direction,shadow_minimum).distance>=emitter_limit {
            visibility = shadow_transmittance(shadow_origin,shadow_origin_low,shadow_direction,shadow_distance,shadow_minimum);
            if !OPAQUE_SCENE && studio && u.volume_min.w>0.0 {
                var attenuation=volume_transmittance_precise(shadow_origin,shadow_origin_low,shadow_direction,shadow_distance);
                if punctual { attenuation=volume_transmittance_precise(physical.hi,physical.lo,sample.direction,physical_distance); }
                visibility.transmittance*=attenuation;
                visibility.branch_pdf*=attenuation;
            }
        }
        let nl = dot(n,sample.direction);
        // A hard source is a delta distribution. A continuous BSDF continuation
        // has no competing probability mass at that direction, so its MIS weight is one.
        if !studio || source>=area_count+2u {
            result += visibility.transmittance * brdf(t,n,v,sample.direction) * (opaque*nl*sample.radiance);
            continue;
        }
        let competing=opaque*surface_pdf(t,n,v,sample.direction)*visibility.branch_pdf;
        let weight=select(power_weight(sample.pdf,competing),1.0,visibility.solid || visibility.crossings>=remaining);
        result += visibility.transmittance*brdf(t,n,v,sample.direction)*sample.radiance*(opaque*nl*weight/sample.pdf);
    }
    return result;
}
struct CameraRay { origin: vec3<f32>, direction: vec3<f32> };
// The lens lies in the eye plane. A uniform-area disk (r=sqrt(u), phi=2pi*v)
// aims at the pinhole ray's intersection with the axial focus plane.
fn primary_ray(uv: vec2<f32>, lens_sample: vec2<f32>) -> CameraRay {
    let offset = u.right.xyz*uv.x + u.up.xyz*uv.y;
    var o = u.eye.xyz + offset;
    var d = -u.back.xyz;
    if u.eye.w > 0.5 { o = u.eye.xyz; d = unit(offset-u.back.xyz*u.back.w); }
    if u.eye.w > 0.5 && u.right.w > 0.0 {
        let radius = sqrt(lens_sample.x)*u.right.w;
        let phi = 2.0*PI*lens_sample.y;
        let displacement = radius*(cos(phi)*unit(u.right.xyz)+sin(phi)*unit(u.up.xyz));
        // Construct the focal point directly, avoiding division by a tiny ray cosine.
        let focal = offset*(u.up.w/u.back.w)-u.back.xyz*u.up.w;
        o += displacement;
        d = unit(focal-displacement);
    }
    return CameraRay(o,d);
}
fn sample_path_lane(pixel: u32, lane: u32) -> vec4<f32> {
    if GLASS_DELTA_LIGHTS {transport_boundary_connection=false;}
    transport_wavelength=0.0;transport_medium_id=0.0;transport_medium_ior=1.0;
    transport_medium_sigma=vec3<f32>(0.0);transport_medium_scatter=0.0;transport_source_triangle=0xffffffffu;
    transport_connection_origin=vec3<f32>(0.0);transport_connection_origin_low=vec3<f32>(0.0);
    var ordinal = u.image.z;
    var salt = 0u;
    if u.adaptive_budget.x > 0u {
        if ordinal < u.adaptive_budget.x { salt = 0xa511e9b3u; }
        else { ordinal -= u.adaptive_budget.x; }
    }
    var rng = pixel ^ (ordinal * 0x9e3779b9u) ^ u.sampling.x ^ salt;
    var stratified = false;
    let xy = vec2<f32>(f32(pixel % u.image.x)+random(&rng), f32(pixel / u.image.x)+random(&rng));
    let uv = vec2<f32>(xy.x/f32(u.image.x)*2.0-1.0, 1.0-xy.y/f32(u.image.y)*2.0);
    var lens_sample = vec2<f32>(0.0);
    // Zero aperture consumes no variates: preserve the pinhole sample stream exactly.
    if u.eye.w > 0.5 && u.right.w > 0.0 { lens_sample = vec2<f32>(random(&rng),random(&rng)); }
    let primary = primary_ray(uv,lens_sample);
    var o = primary.origin;
    var o_low = vec3<f32>(0.0);
    var d = primary.direction;
    var previous_position = o;
    var previous_position_low = vec3<f32>(0.0);
    var ray_excluded = 0xffffffffu;
    var ray_minimum = u.lighting.x;
    var previous_scatter_position = o;
    var previous_scatter_position_low = vec3<f32>(0.0);
    // NEE-only opt-in solid approximation replaces direct transmitted emitter
    // paths from an opaque bounce. Camera and reflected paths stay physical.
    var suppress_direct = false;
    var throughput = vec3<f32>(1.0);
    var wavelength=0.0;
    if (geometry[2]&0x40000000u)!=0u {
        let channel=min(2u,u32(random(&rng)*3.0));
        wavelength=glass_wavelength(channel);
        throughput=vec3<f32>(0.0);throughput[channel]=3.0;
    }
    transport_wavelength=wavelength;
    var radiance = vec3<f32>(0.0);
    var coverage = 0.0;
    var previous_pdf = 0.0;
    var previous_delta = true;
    var continuous_prefix=false;
    var optical_lights:array<vec4<u32>,6>;var optical_light_count=0u;
    var optical_light_rng=pixel*0x68bc21ebu ^ ordinal*0x02e5be93u ^ u.sampling.x ^ salt ^ lane*0x967a889bu ^ 0x5bd1e995u;
    var media: array<Medium,MEDIUM_CAPACITY>;
    var medium_count = 0u;
    var texture_path_length = 0.0;
    for (var bounce = 0u; bounce < u.sampling.y; bounce += 1u) {
        // Classify the actual sampled primary/pupil origin once, before an opaque
        // hit, emitter endpoint, subsurface flight or escape can consume its segment.
        // No random draw or extra traversal is added to nested_media=false paths.
        if !OPAQUE_SCENE && bounce==0u && u.lighting.w>0.0 {
            let boundary=origin_boundary(o,d);
            if !boundary.valid { return medium_failure(radiance,coverage,pixel,0u,4u); }
            if boundary.hit.triangle!=0xffffffffu {
                let first=triangle(boundary.hit.triangle);
                // Match initialize_media's exact uploaded-f32 entry predicate. A
                // certified first entry with no parent already proves vacuum; its
                // initializer would only write the existing zero count again.
                let vacuum=first.interior.w==0.0 && dot(unit(cross(first.p1.xyz-first.p0.xyz,first.p2.xyz-first.p0.xyz)),d)<0.0;
                if !vacuum {
                    if !initialize_media_spectral(boundary.hit.triangle,d,&media,&medium_count,wavelength) { return medium_failure(radiance,coverage,pixel,0u,4u); }
                }
            }
        }
        // Closed-glass continuations set an excluded producing triangle; every
        // other launch (camera, opaque or thin-sheet surface) does not.
        let continued = ray_excluded != 0xffffffffu;
        let hit = trace_precise(o,o_low,d,false,ray_excluded,ray_minimum,medium_count>0u);
        // The complete origin query exclusively owns primary classification.
        // Retain the established fallback for later opaque/thin surface launches.
        if !OPAQUE_SCENE && u.lighting.w>0.0 && bounce>0u && medium_count==0u && hit.triangle!=0xffffffffu {
            if !initialize_media_spectral(hit.triangle,d,&media,&medium_count,wavelength) {return medium_failure(radiance,coverage,pixel,bounce,4u);}
        }
        // Opaque jobs cannot enter a material medium or have a scene volume.
        // Specialize both walks out before the driver compiles the mapped shader.
        if !OPAQUE_SCENE && medium_count>0u && media[medium_count-1u].scatter.x>0.0 {
            let current=media[medium_count-1u];
            let boundary=min(hit.distance,emitter_hit_precise(o,o_low,d,ray_minimum).distance);
            let travel=-log(1.0-random(&rng))/current.scatter.x;
            if travel<boundary {
                texture_path_length+=travel;
                let position=o+o_low+d*travel;
                let traversed=precise_point_delta(position,vec3<f32>(0.0),previous_position,previous_position_low);
                throughput*=exp(-current.sigma*length(traversed));
                coverage=1.0;
                if !any(throughput>vec3<f32>(0.0)) {break;}
                if GLASS_DELTA_LIGHTS {
                    if !photon_enabled() || !continuous_prefix {
                        glass_offer_light(&optical_lights,&optical_light_count,&optical_light_rng,PrecisePoint(position,vec3<f32>(0.0)),d,throughput,current.ior,current.ior,0xffffffffu,1u,current);
                        radiance+=throughput*photon_gather_volume(position,d,current);
                    }
                    continuous_prefix=true;
                }
                // HG delta-source NEE above covers contained straight connections;
                // refracting-boundary suffixes use photons when enabled. Finite
                // source endpoints retain the random walk's complete weight.
                d=sample_hg(d,current.scatter.y,&rng);
                previous_delta=true;previous_pdf=0.0;suppress_direct=false;
                previous_position=position;previous_position_low=vec3<f32>(0.0);
                previous_scatter_position=position;previous_scatter_position_low=vec3<f32>(0.0);
                o=position;o_low=vec3<f32>(0.0);ray_excluded=0xffffffffu;ray_minimum=0.0;
                continue;
            }
            // The competing BSDF endpoint density includes the probability of
            // traversing this physical segment without an HG event, as does NEE.
            let segment=ray_segment_precise(o,o_low,previous_position,previous_position_low,d,boundary);
            previous_pdf*=exp(-current.scatter.x*length(segment));
        }
        if !OPAQUE_SCENE && u.volume_min.w>0.0 {
            let emitter_distance=emitter_hit_precise(o,o_low,d,ray_minimum).distance;
            var segment=vec2<f32>(0.0);
            if bounce==0u && u.eye.w<=0.5 {
                // The orthographic camera has no physical eye plane. Its fog
                // line starts at the finite box entry, even before the artificial
                // geometry origin; the existing forward endpoint stays fixed.
                segment=volume_orthographic_segment(o+o_low,d,min(hit.distance,emitter_distance));
            } else {
                segment=volume_segment(o+o_low,d,min(hit.distance,emitter_distance));
            }
            let length=segment.y-segment.x;
            if length>0.0 {
                let travel=-log(1.0-random(&rng))/u.volume_min.w;
                if travel<length {
                    texture_path_length+=segment.x+travel;
                    let position=o+o_low+d*(segment.x+travel);
                    coverage=1.0;
                    throughput*=u.volume_albedo.xyz;
                    if !any(throughput>vec3<f32>(0.0)) { break; }
                    let remaining=u.sampling.y-bounce-1u;
                    var physical=PrecisePoint(o,o_low+d*(segment.x+travel));
                    if u.light_count.y>0.0 {physical=punctual_offset(physical,vec3<f32>(0.0));}
                    radiance+=throughput*volume_direct(position,physical,d,remaining,&rng);
                    if remaining==0u { break; }
                    let next=volume_sample(d,&rng);
                    previous_pdf=volume_phase(dot(d,next));
                    previous_delta=false;
                    previous_scatter_position=position;
                    previous_scatter_position_low=vec3<f32>(0.0);
                    previous_position=position;
                    previous_position_low=vec3<f32>(0.0);
                    suppress_direct=false;
                    o=position;
                    o_low=vec3<f32>(0.0);
                    d=next;
                    ray_excluded=0xffffffffu;
                    ray_minimum=0.0;
                    continue;
                }
                // Analog survival already supplies attenuation in expectation.
                // Only the competing path density acquires the no-event factor.
                previous_pdf*=exp(-u.volume_min.w*length);
            }
        }
        if u.environment.w > 0.0 {
            let emitter = emitter_hit_precise(o,o_low,d,ray_minimum);
            if emitter.distance < hit.distance {
                let light = u.area_lights[emitter.index];
                let segment = ray_segment_precise(o,o_low,previous_position,previous_position_low,d,emitter.distance);
                if medium_count>0u {
                    throughput *= exp(-media[medium_count-1u].sigma*length(segment));
                }
                let scatter_segment = ray_segment_precise(o,o_low,previous_scatter_position,previous_scatter_position_low,d,emitter.distance);
                let pdf = area_pdf(light,scatter_segment);
                if pdf>0.0 && !suppress_direct {
                    let weight = select(power_weight(previous_pdf,pdf),1.0,previous_delta);
                    radiance += throughput*light.radiance.xyz*weight;
                }
                coverage = 1.0;
                break;
            }
        }
        if hit.triangle == 0xffffffffu {
            // Camera-inside and intersecting/open volumes are deliberately unsupported.
            if medium_count > 0u { return medium_failure(radiance,coverage,pixel,bounce,1u); }
            if u.environment.w > 0.0 {
                // The backplate remains request.background; environment lights covered paths.
                if bounce>0u && !suppress_direct {
                    let weight = select(power_weight(previous_pdf,env_pdf(d)),1.0,previous_delta);
                    radiance += throughput*env_eval(d)*weight;
                    let sun_weight=select(power_weight(previous_pdf,sun_pdf(d)),1.0,previous_delta);
                    radiance += throughput*sun_eval(d)*sun_weight;
                }
            } else if u.sampling.w == 0u || d.z < 0.0 { radiance += throughput * u.lighting.y; }
            break;
        }
        coverage = 1.0;
        ray_excluded = 0xffffffffu; ray_minimum = u.lighting.x;
        let captured = triangle(hit.triangle); let v = -d;
        let outward = unit(cross(captured.p1.xyz-captured.p0.xyz,captured.p2.xyz-captured.p0.xyz));
        let entering = dot(outward,v)>=0.0;
        let geometric = select(-outward,outward,entering);
        var ni = 1.0;
        if medium_count > 0u {ni = media[medium_count-1u].ior;}
        var position = surface_point(captured,hit.bary);
        var position_low = vec3<f32>(0.0);
        if (captured.base.w>0.0 && (captured.parameters.w<0.0 || GLASS_DELTA_LIGHTS)) || medium_count>0u {
            let point=surface_point_precise(captured,hit.bary);
            position=point.hi;position_low=point.lo;
        }
        let shading_position=position+position_low;
        texture_path_length+=length(precise_point_delta(position,position_low,o,o_low));
        if medium_count > 0u {
            let segment=precise_point_delta(position,position_low,previous_position,previous_position_low);
            throughput *= exp(-media[medium_count-1u].sigma*length(segment));
        }
        previous_position = position; previous_position_low=position_low;
        o = position + geometric*(u.lighting.x*2.0); o_low=vec3<f32>(0.0);
        var physical=PrecisePoint(position,position_low);
        if u.light_count.y>0.0 { physical=punctual_offset(surface_point_precise(captured,hit.bary),vec3<f32>(0.0)); }
        transport_medium_id=0.0;transport_medium_ior=1.0;transport_medium_sigma=vec3<f32>(0.0);transport_medium_scatter=0.0;
        transport_source_triangle=hit.triangle;
        if medium_count>0u {
            let current=media[medium_count-1u];transport_medium_id=current.id;transport_medium_ior=current.ior;
            transport_medium_sigma=current.sigma;transport_medium_scatter=current.scatter.x;
        }
        // Glass has no opaque closure. Finish the dielectric event before
        // making the texture sampling temporaries live.
        if !OPAQUE_SCENE && captured.base.w > 0.0 && random(&rng) < captured.base.w {
            let t=captured;
            let interpolated=unit(t.n0.xyz*hit.bary.x+t.n1.xyz*hit.bary.y+t.n2.xyz*hit.bary.z);
            let winding_normal=select(interpolated,-interpolated,dot(interpolated,geometric)<0.0);
            var n=select(geometric,winding_normal,dot(winding_normal,v)>0.0);
            if glass_geometric_normals() {n=geometric;}
            var nt = glass_ior(t.parameters.z,t.glass.y,wavelength);
            if t.parameters.w < 0.0 && entering && medium_count > 0u && u.lighting.w==0.0 {
                return medium_failure(radiance,coverage,pixel,bounce,4u);
            }
            if t.parameters.w<0.0 && entering && u.lighting.w>0.0 {
                var parent_id=0.0;
                if t.interior.w>0.0 {parent_id=triangle(u32(t.interior.w)-1u).optics.w;}
                var actual=0.0;
                if medium_count>0u {actual=media[medium_count-1u].id;}
                if actual!=parent_id {return medium_failure(radiance,coverage,pixel,bounce,4u);}
            }
            if t.parameters.w < 0.0 && !entering {
                if medium_count == 0u {
                    // A ray launched from a surface (not the camera, not a glass
                    // continuation) whose first crossing exits closed glass started
                    // inside that body: the surface touches or lies within the glass,
                    // like a tumbler resting on a table. Start the path in its medium.
                    if continued || bounce == 0u { return medium_failure(radiance,coverage,pixel,bounce,2u); }
                    media[0] = Medium(t.optics.w,nt,t.optics.xyz,t.interior.xy);
                    medium_count = 1u;
                    ni = nt;
                    throughput *= exp(-t.optics.xyz*hit.distance);
                }
                if media[medium_count-1u].id != t.optics.w { return medium_failure(radiance,coverage,pixel,bounce,3u); }
                nt = 1.0;
                if medium_count > 1u { nt = media[medium_count-2u].ior; }
            }
            let rough=glass_rough_active(t,ni,nt);
            if rough {n=glass_macro_normal(d,n,geometric,ni,nt);}
            if t.parameters.w>=0.0 && rough {
                var strata=1u;
                if !stratified && u.adaptive_budget.z>1u && u.image.z>=u.adaptive_budget.x {strata=u.adaptive_budget.z;stratified=true;}
                var walk:GlassSheetWalk;
                if GLASS_DELTA_LIGHTS {
                    let sampled=glass_camera_sheet(t,physical,hit.triangle,d,n,geometric,ni,nt,wavelength,u.sampling.y-bounce,lane,strata,continuous_prefix,throughput,&optical_lights,&optical_light_count,&optical_light_rng,&rng);
                    radiance+=throughput*sampled.direct;continuous_prefix=sampled.continuous;walk=sampled.walk;
                } else {walk=glass_rough_sheet(t,d,n,ni,nt,wavelength,true,u.sampling.y-bounce,lane,strata,&rng);}
                if !walk.valid || dot(walk.direction,n)*dot(walk.direction,geometric)<=0.0 {break;}
                throughput*=walk.weight;d=walk.direction;
                bounce+=walk.consumed-1u;
                previous_delta=true;previous_pdf=0.0;suppress_direct=false;
                previous_scatter_position=position;previous_scatter_position_low=position_low;
                // The virtual slab exits at the same physical sheet point.
                // Keep its barycentric displacement and the launch offset.
                o=position; o_low=position_low+d*(u.lighting.x*2.0);
                continue;
            }
            if GLASS_DELTA_LIGHTS && rough && t.parameters.w<0.0 && (!photon_enabled() || !continuous_prefix) {
                // Enabled opaque straight-shadow connections already replace this
                // prefix's transmitted family. Reflection remains a distinct path.
                let light_mode=select(0u,4u,t.coat.w>0.0 && (suppress_direct || !previous_delta));
                glass_offer_light(&optical_lights,&optical_light_count,&optical_light_rng,physical,d,throughput,ni,nt,hit.triangle,light_mode,glass_vacuum());
                radiance+=throughput*photon_gather_glass(t,shading_position,n,geometric,d,ni,nt,wavelength,0);
            }
            let macro_normal=n;
            if rough {n=glass_visible_normal(n,v,t.glass.x*t.glass.x,vec2<f32>(random(&rng),random(&rng)));}
            var f = dielectric(clamp(dot(n,v),0.0,1.0),ni,nt);
            var transmitted = refract(d,n,ni/nt);
            // A bent shading normal must not send a transmitted ray into the wrong
            // geometric hemisphere. Fall back to the geometric interface there.
            if !rough && ((f.z >= 0.0 && dot(transmitted,geometric) >= 0.0) || dot(reflect(d,n),geometric) <= 0.0) {
                n = geometric;
                f = dielectric(clamp(dot(n,v),0.0,1.0),ni,nt);
                transmitted = refract(d,n,ni/nt);
            }
            let reflected_weight=glass_reflection(t,clamp(dot(n,v),0.0,1.0),ni,nt,wavelength);
            let reflection_probability=dot(reflected_weight,vec3<f32>(1.0/3.0));
            // The first Fresnel choice is stratified conditionally on the same
            // primary/prefix path. Each stratum is weighted equally; later random
            // dimensions use decorrelated streams, never clamped radiance.
            var branch = random(&rng);
            if !stratified && u.adaptive_budget.z > 1u && u.image.z >= u.adaptive_budget.x {
                branch = (f32(lane) + branch) / f32(u.adaptive_budget.z);
                rng ^= (lane + 1u) * 0x85ebca6bu;
                stratified = true;
            }
            if t.parameters.w >= 0.0 {
                let weights = glass_sheet(t,f,clamp(dot(n,v),0.0,1.0),ni,nt,wavelength);
                let mean_r = dot(weights.reflection,vec3<f32>(1.0/3.0));
                let mean_t = dot(weights.transmission,vec3<f32>(1.0/3.0));
                if mean_r+mean_t <= 0.0 { break; }
                let probability = mean_r/(mean_r+mean_t);
                if branch < probability {
                    previous_delta=true; previous_scatter_position=position; previous_scatter_position_low=position_low; suppress_direct=false;
                    throughput *= weights.reflection/probability;
                    d = reflect(d,n);
                } else {
                    throughput *= weights.transmission/(1.0-probability);
                    if t.coat.w>0.0 { previous_pdf*=1.0-probability; }
                    else { previous_delta=true; previous_scatter_position=position; previous_scatter_position_low=position_low; suppress_direct=false; }
                    // Keep a straight connection through sheets: geometric-normal
                    // offsets would change emitter solid-angle PDFs along this chain.
                    o = position+d*(u.lighting.x*2.0);
                    // Parallel thin sheet: unchanged direction, no optical displacement.
                }
            } else if branch < reflection_probability {
                previous_delta=true; previous_scatter_position=position; previous_scatter_position_low=position_low; suppress_direct=false;
                if t.glass.z>0.0 {throughput*=reflected_weight/reflection_probability;}
                d = reflect(d,n);
                if rough && dot(d,geometric)<=0.0 {break;}
            } else {
                if t.coat.w>0.0 { suppress_direct=suppress_direct || !previous_delta; }
                else { suppress_direct=false; }
                previous_delta=true; previous_scatter_position=position; previous_scatter_position_low=position_low;
                if rough && (f.z<0.0 || dot(transmitted,geometric)>=0.0) {break;}
                if t.glass.z>0.0 {throughput*=(vec3<f32>(1.0)-reflected_weight)/(1.0-reflection_probability);}
                if entering {
                    if medium_count >= MEDIUM_CAPACITY { return medium_failure(radiance,coverage,pixel,bounce,4u); }
                    media[medium_count] = Medium(t.optics.w,nt,t.optics.xyz,t.interior.xy);
                    medium_count += 1u;
                } else { medium_count -= 1u; }
                throughput *= (ni/nt)*(ni/nt);
                d = unit(transmitted);
                o = position-geometric*(u.lighting.x*2.0);
            }
            if t.parameters.w<0.0 && rough {throughput*=glass_smith_weight(dot(macro_normal,v),dot(macro_normal,d),t.glass.x*t.glass.x);}
            if GLASS_DELTA_LIGHTS && rough && !(ni==nt && dot(d,geometric)<0.0) {continuous_prefix=true;}
            if dot(d,geometric) > 0.0 { o = position+geometric*(u.lighting.x*2.0); }
            if t.parameters.w<0.0 {
                // A finite normal offset can cross the next face near a closed
                // corner. Launch on the reconstructed boundary, suppress only
                // the producing triangle, and retain every strictly positive exit.
                // Consistent medium crossings retain even arbitrarily short
                // exits; repeated adjacent crossings are handled by traversal.
                // A continuation within 1e-6 of tangent skims its producing face:
                // the reconstructed point's rounding then decides which side of
                // the neighbouring faces it is on, so its medium cannot be
                // classified. Such directions carry negligible energy; end the path.
                if abs(dot(d,geometric))<1e-6 { break; }
                o=position; o_low=position_low; ray_excluded=hit.triangle; ray_minimum=0.0;
            } else if GLASS_DELTA_LIGHTS {
                // Even an index-matched or smooth sheet can share the active
                // optical producer and acquire a split physical hit point.
                let offset_direction=select(d,geometric,dot(d,geometric)>0.0);
                o=position; o_low=position_low+offset_direction*(u.lighting.x*2.0);
            }
            continue;
        }
        let map_record=texture_record(captured.optics.w);
        let gradients=texture_gradients(map_record,geometric,d,hit.distance,texture_path_length,bounce);
        let mapped_surface=texture_surface(map_record,captured.base.xyz,captured.parameters.x,captured.parameters.y,shading_position,gradients,(u32(captured.interior.z)&2u)!=0u);
        // Retain immutable geometry and medium data across optical map evaluation.
        let textured=Triangle(captured.p0,captured.p1,captured.p2,captured.n0,captured.n1,captured.n2,vec4<f32>(mapped_surface.color,captured.base.w),vec4<f32>(mapped_surface.metallic,mapped_surface.roughness,captured.parameters.zw),captured.optics,captured.coat,captured.interior,captured.glass);
        let t=aged_surface(textured,shading_position,outward);
        let interpolated=unit(captured.n0.xyz*hit.bary.x+captured.n1.xyz*hit.bary.y+captured.n2.xyz*hit.bary.z);
        let winding_normal=select(interpolated,-interpolated,dot(interpolated,geometric)<0.0);
        let original_normal=select(geometric,winding_normal,dot(winding_normal,v)>0.0);
        let mapped_normal=texture_normal(map_record,mapped_surface.normal,original_normal,geometric);
        let n=select(original_normal,mapped_normal,dot(mapped_normal,v)>0.0);
        radiance += throughput*direct_light(t,shading_position,physical,o,n,geometric,v,u.sampling.y-bounce-1u,&rng);
        radiance += throughput*photon_gather(t,shading_position,n,geometric,v);
        d = sample_surface(t,n,v,&rng);
        let nl = dot(n,d);
        if nl<=0.0 || dot(geometric,d)<=0.0 { break; }
        let pdf = surface_pdf(t,n,v,d);
        if GLASS_DELTA_LIGHTS {continuous_prefix=true;}
        previous_pdf = (1.0-t.base.w)*pdf;
        previous_scatter_position=position; previous_scatter_position_low=position_low;
        previous_delta = false;
        suppress_direct=false;
        throughput *= brdf(t,n,v,d) * (nl/pdf);
        if medium_count>0u {
            o=position;o_low=position_low;ray_excluded=hit.triangle;ray_minimum=0.0;
        }
        if bounce>=3u {
            let survive = clamp(max(max(throughput.x,throughput.y),throughput.z),0.05,0.95);
            if random(&rng)>survive { break; }
            throughput /= survive;
        }
    }
    if GLASS_DELTA_LIGHTS {glass_capture_light(&optical_lights,optical_light_count,wavelength,pixel);}
    return vec4<f32>(radiance,coverage);
}

fn checked_sample(pixel: u32, lane: u32) -> vec4<f32> {
    let value = sample_path_lane(pixel, lane);
    if !all(value >= vec4<f32>(0.0)) || !all(value <= vec4<f32>(3.4e38)) {
        atomicOr(&failures[7], 1u);
    }
    return value;
}
fn sample_path(pixel: u32) -> vec4<f32> {
    // One bounded path per invocation. Strata advance on the host, avoiding a
    // nested path loop and its severe native-driver compilation cost.
    return checked_sample(pixel, u32(u.adaptive_error.w));
}

fn adaptive_target(mean: vec4<f32>, m2: vec4<f32>) -> u32 {
    let maximum = u.adaptive_budget.w;
    // Nonfinite/overflowing pilot estimates never report convergence.
    if !all(mean >= vec4<f32>(0.0)) || !all(mean <= vec4<f32>(3.4e38)) ||
       !all(m2 >= vec4<f32>(0.0)) || !all(m2 <= vec4<f32>(3.4e38)) { return maximum; }
    let tolerance = vec4<f32>(vec3<f32>(u.adaptive_error.y) + mean.xyz*u.adaptive_error.x, u.adaptive_error.z);
    let deviation = sqrt(m2/f32(u.adaptive_budget.x-1u));
    let ratio = min(deviation/tolerance, vec4<f32>(sqrt(f32(maximum))));
    let need = ceil(ratio*ratio);
    return clamp(u32(max(max(need.x,need.y),max(need.z,need.w))),u.adaptive_budget.y,maximum);
}

// Both phases finish through the same film update. Optical camera invocations
// publish one bounded request per tile; a separate native program resolves its
// visibility before entering this update, including final adaptive normalization.
fn accumulate_path(pixel:u32,value:vec4<f32>) {
    let pixels=u.image.x*u.image.y;let pilots=u.adaptive_budget.x;
    let strata=max(1u,u.adaptive_budget.z);let lane=u32(u.adaptive_error.w);
    var budget_count=0u;
    if pilots>0u && u.image.z>=pilots {budget_count=u32(accumulation[2u*pixels+pixel].x);}
    if pilots == 0u {
        let sample = value/f32(strata);
        if u.image.z == 0u && lane == 0u { accumulation[pixel] = sample; }
        else { accumulation[pixel] += sample; }
        return;
    }
    if u.image.z < pilots {
        var mean = accumulation[pixels+pixel];
        var m2 = accumulation[2u*pixels+pixel];
        if u.image.z == 0u { mean=vec4<f32>(0.0); m2=vec4<f32>(0.0); }
        let delta=value-mean;
        mean += delta/f32(u.image.z+1u);
        m2 += delta*(value-mean);
        accumulation[pixels+pixel]=mean;
        accumulation[2u*pixels+pixel]=m2;
        if u.image.z+1u == pilots {
            accumulation[2u*pixels+pixel]=vec4<f32>(f32(adaptive_target(mean,m2)),0.0,0.0,0.0);
            accumulation[pixel]=vec4<f32>(0.0);
        }
        return;
    }
    let ordinal=u.image.z-pilots;
    accumulation[pixel] += value/f32(strata);
    // Per-pixel production means enter both raw EXR and denoising. Pilots do not.
    if ordinal+1u == budget_count && lane+1u == strata { accumulation[pixel] /= f32(budget_count); }
}
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {
    let pixel=u.image.w+gid.x;let pixels=u.image.x*u.image.y;
    if gid.x>=u.sampling.z || pixel>=pixels {return;}
    let pilots=u.adaptive_budget.x;
    if pilots>0u && u.image.z>=pilots && u.image.z-pilots>=u32(accumulation[2u*pixels+pixel].x) {return;}
    if GLASS_DELTA_LIGHTS {accumulation[glass_tile_address(pixel)+6u]=vec4<f32>(0.0);}
    let value=sample_path(pixel);
    if GLASS_DELTA_LIGHTS {accumulation[glass_tile_address(pixel)+7u]=value;return;}
    accumulate_path(pixel,value);
}

@compute @workgroup_size(32)
fn optical_resolve(@builtin(global_invocation_id) gid:vec3<u32>) {
    if !GLASS_DELTA_LIGHTS {return;}
    let pixel=u.image.w+gid.x;let pixels=u.image.x*u.image.y;
    if gid.x>=u.sampling.z || pixel>=pixels {return;}
    let pilots=u.adaptive_budget.x;
    if pilots>0u && u.image.z>=pilots && u.image.z-pilots>=u32(accumulation[2u*pixels+pixel].x) {return;}
    let address=glass_tile_address(pixel);let header=accumulation[address+6u];
    var packet:array<vec4<u32>,6>;
    if header.x>0.0 {
        for(var lane=0u;lane<6u;lane+=1u) {packet[lane]=bitcast<vec4<u32>>(accumulation[address+lane]);}
        packet[0].w=u32(accumulation[address].w);packet[1].w=u32(accumulation[address+1u].w);
    }
    transport_wavelength=header.y;
    var value=accumulation[address+7u];
    value=vec4<f32>(value.xyz+glass_resolve_light(&packet,u32(header.x),header.y),value.w);
    if !all(value>=vec4<f32>(0.0)) || !all(value<=vec4<f32>(3.4e38)) {atomicOr(&failures[7],1u);}
    accumulate_path(pixel,value);
}

// Validation entry point: exact same BRDF used by production paths, read back in tests.
@compute @workgroup_size(1)
fn reference() {
    accumulation[0] = vec4<f32>(brdf(triangle(0u),u.up.xyz,u.back.xyz,u.right.xyz),1.0);
}

@compute @workgroup_size(1)
fn dielectric_reference() {
    let f = dielectric(u.up.x,u.up.y,u.up.z);
    accumulation[0] = vec4<f32>(f.xy,max(f.z,0.0),1.0);
}

@compute @workgroup_size(1)
fn camera_reference() {
    let ray = primary_ray(u.lighting.xy,u.lighting.zw);
    // Bias signed test coordinates for the radiance readback's nonnegative contract.
    accumulation[0] = vec4<f32>(ray.origin+vec3<f32>(16.0),1.0);
    accumulation[1] = vec4<f32>(ray.direction+vec3<f32>(2.0),1.0);
    accumulation[2] = vec4<f32>(texture_camera_direction_delta(ray.direction,2.0*u.right.xyz/f32(u.image.x))+vec3<f32>(16.0),1.0);
    accumulation[3] = vec4<f32>(texture_camera_direction_delta(ray.direction,-2.0*u.up.xyz/f32(u.image.y))+vec3<f32>(16.0),1.0);
}

// Deterministic image-space ball-lens check. Uses the production BVH/intersection
// and interpolated normal data, with Snell transport through both interfaces.
@compute @workgroup_size(1)
fn lens_reference() {
    let direction = u.back.xyz;
    let entry = trace(u.eye.xyz,direction,false);
    if entry.triangle == 0xffffffffu { accumulation[0] = vec4<f32>(0.0); return; }
    let a = triangle(entry.triangle);
    let pa = surface_point(a,entry.bary);
    let na = unit(a.n0.xyz*entry.bary.x+a.n1.xyz*entry.bary.y+a.n2.xyz*entry.bary.z);
    let inside = refract(direction,na,1.0/a.parameters.z);
    let exit = trace(pa-na*u.lighting.x*2.0,inside,false);
    if exit.triangle == 0xffffffffu { accumulation[0] = vec4<f32>(0.0); return; }
    let b = triangle(exit.triangle);
    let pb = surface_point(b,exit.bary);
    let nb = unit(b.n0.xyz*exit.bary.x+b.n1.xyz*exit.bary.y+b.n2.xyz*exit.bary.z);
    let outgoing = refract(inside,-nb,b.parameters.z);
    let spot = pb+outgoing*((-u.back.w-pb.z)/outgoing.z);
    accumulation[0] = vec4<f32>(spot+vec3<f32>(1.0,1.0,3.0),1.0);
}

// GPU/CPU orientation and matched-density check uses the production lookup helpers.
@compute @workgroup_size(1)
fn environment_reference() {
    accumulation[0]=vec4<f32>(env_eval(u.right.xyz),env_pdf(u.right.xyz));
}

fn coat_energy_bound(f0: vec3<f32>, cosine: f32) -> vec3<f32> {
    let minimum_vh = cosine/sqrt(2.0*(1.0+sqrt(max(0.0,1.0-cosine*cosine))));
    return fresnel(f0,minimum_vh);
}

fn sample_surface_values(t: Triangle, n: vec3<f32>, v: vec3<f32>, variates: vec3<f32>) -> vec3<f32> {
    let component = variates.x;
    let x = variates.y; let phi = 2.0*PI*variates.z;
    let coat_probability = t.coat.x*0.5;
    if component < (1.0-coat_probability)*0.5 {
        let radius = sqrt(x);
        return from_local(vec3<f32>(radius*cos(phi),radius*sin(phi),sqrt(1.0-x)),n);
    }
    let roughness = select(t.parameters.y,t.coat.y,component >= 1.0-coat_probability);
    let alpha = roughness*roughness;
    let cosine = sqrt((1.0-x)/(1.0+(alpha*alpha-1.0)*x));
    let sine = sqrt(max(0.0,1.0-cosine*cosine));
    let h = from_local(vec3<f32>(sine*cos(phi),sine*sin(phi),cosine),n);
    return reflect(-v,h);
}

@compute @workgroup_size(1)
fn surface_sample_reference() {
    let d = sample_surface_values(triangle(0u),u.up.xyz,u.back.xyz,u.lighting.xyz);
    accumulation[0] = vec4<f32>(d+vec3<f32>(2.0),1.0);
}

@compute @workgroup_size(1)
fn pdf_reference() {
    accumulation[0] = vec4<f32>(surface_pdf(triangle(0u),u.up.xyz,u.back.xyz,u.right.xyz),0.0,0.0,1.0);
}

// Diagnostic entry point for independently specified closed-corner fixtures.
@compute @workgroup_size(1)
fn boundary_reference() {
    let entry=trace(u.eye.xyz,u.back.xyz,false);
    if entry.triangle==0xffffffffu {accumulation[0]=vec4<f32>(9.0,9.0,9.0,1.0);return;}
    let t=triangle(entry.triangle);
    let position=surface_point(t,entry.bary);
    let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
    let shifted=position-outward*(2.0*u.lighting.x);
    let legacy=trace(shifted,u.right.xyz,false);
    let low_min=trace_bounded(shifted,u.right.xyz,false,entry.triangle,0.0);
    let exact=trace_bounded(position,u.right.xyz,false,entry.triangle,0.0);
    accumulation[0]=vec4<f32>(select(0.0,1.0,legacy.triangle!=0xffffffffu),select(0.0,1.0,low_min.triangle!=0xffffffffu),select(0.0,1.0,exact.triangle!=0xffffffffu),1.0);
    accumulation[1]=vec4<f32>(exact.distance,0.0,0.0,1.0);
    accumulation[2]=vec4<f32>(abs(position),1.0);
    accumulation[3]=vec4<f32>(f32(entry.triangle),entry.bary.yz,1.0);
}

@compute @workgroup_size(1)
fn positive_triangle_reference() {
    let hit=trace_bounded(u.eye.xyz,u.right.xyz,false,0xffffffffu,0.0);
    accumulation[0]=vec4<f32>(select(0.0,1.0,hit.triangle!=0xffffffffu),0.0,0.0,1.0);
}

// Producing hit barycentrics are prescribed independently to exercise f32 point
// reconstruction on a translated oblique split plane, then a grazing reflection.
@compute @workgroup_size(1)
fn coplanar_boundary_reference() {
    let position=surface_point(triangle(0u),vec3<f32>(1.0-u.eye.x-u.eye.y,u.eye.xy));
    let hit=trace_bounded(position,u.right.xyz,false,0u,0.0);
    accumulation[0]=vec4<f32>(select(0.0,1.0,hit.triangle!=0xffffffffu),hit.distance,0.0,1.0);
    accumulation[1]=vec4<f32>(position,1.0);
}

fn ordering_candidate(t: Triangle, o: vec3<f32>, d: vec3<f32>) -> vec2<f32> {
    var kz=0u;
    if abs(d.y)>abs(d.x) {kz=1u;}
    if abs(d.z)>abs(d[kz]) {kz=2u;}
    let kx=(kz+1u)%3u; let ky=(kx+1u)%3u;
    let shear=vec3<f32>(d[kx]/d[kz],d[ky]/d[kz],1.0/d[kz]);
    let pa=t.p0.xyz-o; let pb=t.p1.xyz-o; let pc=t.p2.xyz-o;
    let a=vec2<f32>(pa[kx]-shear.x*pa[kz],pa[ky]-shear.y*pa[kz]);
    let b=vec2<f32>(pb[kx]-shear.x*pb[kz],pb[ky]-shear.y*pb[kz]);
    let c=vec2<f32>(pc[kx]-shear.x*pc[kz],pc[ky]-shear.y*pc[kz]);
    let weights=vec3<f32>(difference_products(b.x,c.y,b.y,c.x),difference_products(c.x,a.y,c.y,a.x),difference_products(a.x,b.y,a.y,b.x));
    let det=weights.x+weights.y+weights.z;
    let bary=weights/det;
    let distance=surface_ray_depth(t,bary,o,kz)*shear.z;
    let valid=!(any(weights<vec3<f32>(0.0))&&any(weights>vec3<f32>(0.0))) && det!=0.0;
    return vec2<f32>(distance,select(0.0,1.0,valid));
}
// Closed continuation from an anchor (eye) plus local offset (right) along up,
// excluding triangle 0's producing patch; eye.w > 0.5 starts inside a medium.
@compute @workgroup_size(1)
fn continuation_reference() {
    let hit=trace_precise(u.eye.xyz,u.right.xyz,u.up.xyz,false,0u,0.0,u.eye.w>0.5);
    accumulation[0]=vec4<f32>(f32(hit.triangle),hit.distance,0.0,1.0);
}
@compute @workgroup_size(1)
fn ordering_reference() {
    let hit=trace(u.eye.xyz,u.right.xyz,false);
    let a=ordering_candidate(triangle(0u),u.eye.xyz,u.right.xyz);
    let b=ordering_candidate(triangle(1u),u.eye.xyz,u.right.xyz);
    accumulation[0]=vec4<f32>(f32(hit.triangle),hit.distance,0.0,1.0);
    accumulation[1]=vec4<f32>(a,0.0,1.0);
    accumulation[2]=vec4<f32>(b,0.0,1.0);
}

@compute @workgroup_size(1)
fn refracted_corner_reference() {
    let entry=trace(u.eye.xyz,u.right.xyz,false);
    if entry.triangle==0xffffffffu {accumulation[0]=vec4<f32>(0.0,0.0,0.0,1.0);return;}
    let t=triangle(entry.triangle);
    let position=surface_point(t,entry.bary);
    let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
    let inside=refract(u.right.xyz,outward,1.0/t.parameters.z);
    let exit=trace_ordered_within(position,inside,false,entry.triangle,0.0,true,FAR,-1.0);
    var valid_exit=false;
    if exit.triangle!=0xffffffffu {
        let other=triangle(exit.triangle);
        let normal=unit(cross(other.p1.xyz-other.p0.xyz,other.p2.xyz-other.p0.xyz));
        valid_exit=other.optics.w==t.optics.w && dot(normal,inside)>0.0;
    }
    accumulation[0]=vec4<f32>(select(0.0,1.0,dot(outward,u.right.xyz)<0.0),select(0.0,1.0,exit.triangle!=0xffffffffu),select(0.0,1.0,valid_exit),1.0);
    accumulation[1]=vec4<f32>(exit.distance,0.0,0.0,1.0);
    accumulation[2]=vec4<f32>(abs(position),1.0);
    accumulation[3]=vec4<f32>(inside+vec3<f32>(2.0),1.0);
}

@compute @workgroup_size(1)
fn rib_residual_reference() {
    var source=0xffffffffu;
    for(var i=0u;i<geometry[0];i+=1u) {
        let t=triangle(i);
        if all(t.p0.xyz==u.eye.xyz)&&all(t.p1.xyz==u.right.xyz)&&all(t.p2.xyz==u.up.xyz) {source=i;break;}
    }
    if source==0xffffffffu {accumulation[0]=vec4<f32>(9.0,9.0,9.0,1.0);return;}
    let t=triangle(source);
    let point=surface_point_precise(t,u.environment.xyz);
    let outward=unit(cross(t.p1.xyz-t.p0.xyz,t.p2.xyz-t.p0.xyz));
    let direction=reflect(u.back.xyz,outward);
    let f=dielectric(abs(dot(outward,u.back.xyz)),t.parameters.z,1.0);
    let dropped=trace_bounded(point.hi+point.lo,direction,false,source,0.0);
    let retained=trace_precise(point.hi,point.lo,direction,false,source,0.0,true);
    var valid_exit=false;
    if retained.triangle!=0xffffffffu {
        let other=triangle(retained.triangle);
        let n=unit(cross(other.p1.xyz-other.p0.xyz,other.p2.xyz-other.p0.xyz));
        valid_exit=other.optics.w==t.optics.w&&dot(n,direction)>0.0;
    }
    accumulation[0]=vec4<f32>(select(0.0,1.0,dropped.triangle!=0xffffffffu),select(0.0,1.0,retained.triangle!=0xffffffffu),select(0.0,1.0,valid_exit),1.0);
    accumulation[1]=vec4<f32>(retained.distance,select(0.0,1.0,f.z<0.0),0.0,1.0);
    accumulation[2]=vec4<f32>(abs(point.hi),1.0);
    accumulation[3]=vec4<f32>(abs(point.lo),1.0);
}

// Results are encoded as exact positive 16-bit integers in floats so the normal
// radiance readback can transport every binary32 bit pattern, including NaNs.
@compute @workgroup_size(64)
fn integer_float_reference(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let i=invocation.x;
    if i>=geometry[0] { return; }
    let a=geometry[1u+i*2u];
    let b=geometry[2u+i*2u];
    let sum=fp_add_bits(a,b);
    let difference=fp_sub_barrier_bits(a,b);
    accumulation[i]=vec4<f32>(f32(sum&65535u),f32(sum>>16u),f32(difference&65535u),f32(difference>>16u));
}

@compute @workgroup_size(1)
fn emitter_segment_reference() {
    // A local offset large compared with this real ray segment. Accumulating
    // the endpoint into the offset first needlessly rounds away part of d*t.
    let anchor=vec3<f32>(-163.7,-55.0,-55.0);
    let offset=vec3<f32>(0.000007887696,0.000009279658,-10.753371);
    let direction=vec3<f32>(0.6,0.0,0.8);
    let distance=0.00001;
    let segment=ray_segment_precise(anchor,offset,anchor,offset,direction,distance);
    let light=AreaLight(vec4<f32>(0.0,0.0,0.0,16.0),vec4<f32>(0.0,2.0,0.0,0.0),vec4<f32>(2.0,0.0,0.0,0.0),vec4<f32>(1.0));
    accumulation[0]=vec4<f32>(length(segment),exp(-5000.0*length(segment)),area_pdf(light,segment),1.0);
}

@compute @workgroup_size(64)
fn integer_relative_reference(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let i=invocation.x;
    if i>=geometry[0] { return; }
    let a=geometry[1u+i*4u];
    let al=geometry[2u+i*4u];
    let b=geometry[3u+i*4u];
    let bl=geometry[4u+i*4u];
    let reference=fp_add_bits(fp_sub_bits(a,b),fp_sub_bits(al,bl));
    let candidate=anchored_delta_bits(a,al,b,bl);
    accumulation[i]=vec4<f32>(f32(reference&65535u),f32(reference>>16u),f32(candidate&65535u),f32(candidate>>16u));
}

@compute @workgroup_size(32)
fn split_predicate_reference(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x>=geometry[0] { return; }
    let word=1u+gid.x*4u;
    let a=bitcast<f32>(geometry[word]);let b=bitcast<f32>(geometry[word+1u]);
    let c=bitcast<f32>(geometry[word+2u]);let d=bitcast<f32>(geometry[word+3u]);
    let product=split_product(split_scaled(a).value,split_scaled(b).value);
    let p=bitcast<u32>(product.x);let e=bitcast<u32>(product.y);
    let direct=bitcast<u32>(split_difference_products(a,b,c,d));let wrapped=bitcast<u32>(difference_products(a,b,c,d));
    accumulation[2u*gid.x]=vec4<f32>(f32(p&65535u),f32(p>>16u),f32(e&65535u),f32(e>>16u));
    accumulation[2u*gid.x+1u]=vec4<f32>(f32(direct&65535u),f32(direct>>16u),f32(wrapped&65535u),f32(wrapped>>16u));
}
