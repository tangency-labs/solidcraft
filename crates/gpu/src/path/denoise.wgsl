// Original bounded edge-avoiding a-trous filter inspired by Dammertz et al.
// Inputs/outputs keep accumulated linear premultiplied radiance and raw alpha.
// window.z: bit0 optical filtering, bit1 camera uncertainty. work.w: firefly flag.
struct FilterParams { image: vec4<u32>, work: vec4<u32>, window: vec4<u32> };
@group(0) @binding(0) var<uniform> f: FilterParams;
@group(0) @binding(1) var<storage,read> guides: array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> source: array<vec4<f32>>;
@group(0) @binding(3) var<storage,read_write> filtered: array<vec4<f32>>;
struct Uncertainty { variance: vec3<f32>, count: u32 };
@group(0) @binding(4) var<storage,read> uncertainty: array<Uncertainty>;
fn filter_pixels()->u32 {return select(f.image.x*f.image.y,f.window.y,f.window.y!=0u);}

fn kernel(x:i32)->f32 {
    if x==0 { return 6.0; }
    if abs(x)==1 { return 4.0; }
    return 1.0;
}
fn max_component(v:vec3<f32>)->f32 { return max(max(v.x,v.y),v.z); }
// Both optical branches must agree; body identity alone cannot preserve
// material, normal or depth edges on a single reflected object.
fn geometry_weight(a:vec4<f32>,n:vec4<f32>,p:vec4<f32>,b:vec4<f32>,m:vec4<f32>,r:vec4<f32>,step:f32)->f32 {
    if a.w!=b.w {return 0.0;}
    if a.w==0.0 {return 1.0;}
    if (p.w<0.0)!=(r.w<0.0) {return 0.0;}
    if p.w<0.0 {
        if n.w!=m.w {return 0.0;}
        if n.w==0.0 {
            let footprint=max(abs(p.w),abs(r.w))*step;
            let chord=n.xyz-m.xyz;
            return exp(-dot(chord,chord)/max(4.0*footprint*footprint,1e-12));
        }
    }
    let da=a.xyz-b.xyz; let dp=r.xyz-p.xyz;
    let plane=abs(dot(dp,n.xyz))+abs(dot(dp,m.xyz));
    let footprint=max(abs(p.w),abs(r.w))*step;
    let depth=abs(n.w-m.w)/max(8.0*footprint,0.000001);
    return exp(-dot(da,da)/0.04-plane/max(0.05*footprint,0.000001)-depth)*pow(max(dot(n.xyz,m.xyz),0.0),32.0);
}
fn optical_route(pixel:u32)->u32 {
    let base=pixel*6u;
    if (bitcast<u32>(guides[base].w)&0xf8000000u)==0xa8000000u {return u32(guides[base+3u].w);}
    return 0u;
}
// Select only the camera branch of this exclusive component, retaining the
// primary identity and its silhouette. Selection zero keeps the legacy pair.
fn component_guide(pixel:u32,selection:u32)->array<vec4<f32>,6> {
    let base=pixel*6u;
    var value=array<vec4<f32>,6>(guides[base],guides[base+1u],guides[base+2u],guides[base+3u],guides[base+4u],guides[base+5u]);
    if selection==0u || value[0].w>=0.0 {return value;}
    let identity=bitcast<u32>(value[0].w);
    let primary=(identity>>18u)&511u;
    let reflected=(identity>>9u)&511u;
    let transmitted=identity&511u;
    if selection==2u && reflected!=0u {
        if transmitted!=0u {value[0]=value[3];value[1]=value[4];value[2]=value[5];}
        value[0].w=bitcast<f32>(0xa0000000u|(primary<<18u)|(reflected<<9u));
    } else if selection==3u && transmitted!=0u {
        value[0].w=bitcast<f32>(0xa0000000u|(primary<<18u)|transmitted);
    } else {value[0]=vec4<f32>(0.0);}
    value[3]=vec4<f32>(0.0);value[4]=vec4<f32>(0.0);value[5]=vec4<f32>(0.0);
    return value;
}
@compute @workgroup_size(64)
fn atrous(@builtin(global_invocation_id) gid:vec3<u32>) {
    let local=f.image.w+gid.x;
    let pixel=f.window.x+local;
    if gid.x>=f.work.x || local>=filter_pixels() || pixel>=f.image.x*f.image.y { return; }
    let c=source[local];
    let ag=component_guide(local,f.window.w);
    let a=ag[0];let n=ag[1];let p=ag[2];
    // Optical correspondence is a negative, finite tag in the same slot.
    let optical=a.w<0.0;
    if a.w==0.0 || c.w<f32(f.work.y)*0.999 || (optical && ((f.window.z&1u)==0u || f.image.z>2u)) { filtered[local]=c; return; }
    let xy=vec2<i32>(i32(pixel%f.image.x),i32(pixel/f.image.x));
    var mean=vec3<f32>(0.0); var total=0.0;
    let samples=f32(f.work.y);
    let color=c.xyz/samples;
    let center_peak=max_component(color);
    var strength=bitcast<f32>(f.work.z);
    var center_uncertainty=Uncertainty(vec3<f32>(0.0),0u);
    if (f.window.z&2u)!=0u {
        center_uncertainty=uncertainty[local];
        if center_uncertainty.count>=2u {
            let noise=max_component(center_uncertainty.variance);
            let floor=0.01+0.02*center_peak;
            strength*=noise/(noise+floor*floor);
        }
    }
    if strength==0.0 {filtered[local]=c;return;}
    let first_pass=f.image.z==1u && !optical && f.work.w!=0u;
    var support_count=0u;
    var support_peak=0.0;
    for(var y=-2;y<=2;y+=1) { for(var x=-2;x<=2;x+=1) {
        let q=xy+vec2<i32>(x,y)*i32(f.image.z);
        if any(q<vec2<i32>(0)) || q.x>=i32(f.image.x) || q.y>=i32(f.image.y) { continue; }
        let global=u32(q.y)*f.image.x+u32(q.x);
        if global<f.window.x || global-f.window.x>=filter_pixels() {continue;}
        let j=global-f.window.x;
        // Reflection is independent of the transmitted route. Whole beauty and
        // transmission require the original route before component projection.
        if f.window.w!=2u && optical_route(local)!=optical_route(j) {continue;}
        let bg=component_guide(j,f.window.w);
        let b=bg[0];let m=bg[1];let r=bg[2];
        let v=source[j];
        if b.w!=a.w || abs(v.w-c.w)>samples*0.01 { continue; }
        let guide_weight=geometry_weight(a,n,p,b,m,r,f32(f.image.z))
            *geometry_weight(ag[3],ag[4],ag[5],bg[3],bg[4],bg[5],f32(f.image.z));
        let tap_color=v.xyz/samples;
        let tap_peak=max_component(tap_color);
        // A genuine bright feature has bright compatible neighbours. Only the
        // immediate 3x3 support may identify an isolated first-pass outlier.
        if first_pass && abs(x)<=1 && abs(y)<=1 && (x!=0 || y!=0) && guide_weight>=0.5 {
            support_count+=1u;
            support_peak=max(support_peak,tap_peak);
        }
        let high=max(center_peak,tap_peak);
        let bound=16.0*(0.25+min(center_peak,tap_peak));
        var contrast=1.0;
        if f.work.w!=0u && high>bound {
            let ratio=bound/high;
            contrast=ratio*ratio;
        }
        let dc=max_component(abs(color-tap_color));
        let color_scale=0.25+0.5*high;
        var color_distance=dc/color_scale;
        if (f.window.z&2u)!=0u {
            let tap_uncertainty=uncertainty[j];
            if center_uncertainty.count>=2u && tap_uncertainty.count>=2u {
                let noise=max_component(center_uncertainty.variance+tap_uncertainty.variance);
                color_distance=dc*dc/max(9.0*noise,1e-12);
            }
        }
        let weight=kernel(x)*kernel(y)*guide_weight*exp(-select(1.0,4.0,optical)*color_distance)*contrast;
        if weight>0.0 {
            total+=weight;
            // Online convex averaging avoids overflow of radiance times kernel.
            mean+=(v.xyz-mean)*(weight/total);
        }
    }}
    if first_pass && support_count>=5u && center_peak>16.0*(0.25+support_peak) {
        let limit=2.0*support_peak+0.25;
        let mean_peak=max_component(mean/samples);
        if mean_peak>limit { mean*=limit/mean_peak; }
    }
    // The centre always contributes; retain original coverage without filtering.
    filtered[local]=vec4<f32>(mix(c.xyz,mean,strength),c.w);
}

// Stable DenoiseGuideClass codes. This diagnostic writes only the filter scratch
// output, never accumulation or guide data. Coverage matches the filter gate.
@compute @workgroup_size(64)
fn guide_classes(@builtin(global_invocation_id) gid:vec3<u32>) {
    let local=f.image.w+gid.x;
    let pixel=f.window.x+local;
    if gid.x>=f.work.x || local>=filter_pixels() || pixel>=f.image.x*f.image.y {return;}
    let guide=guides[local*6u];
    var reason=u32(guide.x);
    if guide.w!=0.0 {
        reason=select(1u,2u,guide.w<0.0);
        if source[local].w<f32(f.work.y)*0.999 {reason=16u;}
    }
    filtered[local]=vec4<f32>(f32(reason),0.0,0.0,1.0);
}
