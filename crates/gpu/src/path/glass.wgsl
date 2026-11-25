// Optional dielectric extensions. Legacy materials do not execute these draws
// or allocate their extra packed material lane.
var<private> transport_wavelength: f32=0.0;
var<private> transport_medium_id: f32=0.0;
var<private> transport_medium_ior: f32=1.0;
var<private> transport_medium_sigma: vec3<f32>=vec3<f32>(0.0);
var<private> transport_medium_scatter: f32=0.0;
var<private> transport_source_triangle: u32=0xffffffffu;
var<private> transport_connection_origin: vec3<f32>=vec3<f32>(0.0);
var<private> transport_connection_origin_low: vec3<f32>=vec3<f32>(0.0);
var<private> transport_boundary_connection: bool=false;
fn glass_wavelength(channel: u32) -> f32 {
    return array<f32,3>(610.0,550.0,460.0)[channel];
}
fn glass_ior(base: f32, abbe: f32, wavelength: f32) -> f32 {
    if abbe<=0.0 || wavelength<=0.0 {return base;}
    let f=486.1327;let c=656.2725;let d=587.5618;
    let b=((base-1.0)/abbe)/(1.0/(f*f)-1.0/(c*c));
    return base+b*(1.0/(wavelength*wavelength)-1.0/(d*d));
}
fn glass_cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x*b.x-a.y*b.y,a.x*b.y+a.y*b.x);
}
fn glass_cdiv(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(dot(a,b),a.y*b.x-a.x*b.y)/dot(b,b);
}
fn glass_complex_cosine(incident_sine_squared: f32, ni: f32, nt: f32) -> vec2<f32> {
    let square=1.0-(ni/nt)*(ni/nt)*incident_sine_squared;
    return vec2<f32>(sqrt(max(square,0.0)),sqrt(max(-square,0.0)));
}
// Lossless coherent single film between the actual incident and transmitted
// media. Complex admittances retain evanescent waves and frustrated film TIR.
fn glass_film_polarization(cosine: f32, ni: f32, nt: f32, nf: f32, thickness: f32, wavelength: f32, p: bool) -> f32 {
    let sin2=max(0.0,1.0-cosine*cosine);
    let cf=glass_complex_cosine(sin2,ni,nf);
    let ct=glass_complex_cosine(sin2,ni,nt);
    let q0=vec2<f32>(select(ni*cosine,cosine/ni,p),0.0);
    let qf=cf*select(nf,1.0/nf,p);
    let qt=ct*select(nt,1.0/nt,p);
    let phase=cf*(2.0*PI*nf*thickness/wavelength);
    var numerator=vec2<f32>(0.0);var denominator=vec2<f32>(0.0);
    if dot(phase,phase)<0.0001 {
        // Characteristic-matrix limit avoids Airy's cancellation/0÷0 at the
        // film critical angle. sin(phase)/qf has a finite analytic limit.
        let phase2=glass_cmul(phase,phase);
        let phase4=glass_cmul(phase2,phase2);
        let cosine_phase=vec2<f32>(1.0,0.0)-phase2*0.5+phase4/24.0;
        let sinc=vec2<f32>(1.0,0.0)-phase2/6.0+phase4/120.0;
        let ratio=sinc*(2.0*PI*thickness/wavelength*select(1.0,nf*nf,p));
        let a=cosine_phase+glass_cmul(vec2<f32>(-ratio.y,ratio.x),qt);
        let qsin=glass_cmul(qf,glass_cmul(phase,sinc));
        let b=vec2<f32>(-qsin.y,qsin.x)+glass_cmul(cosine_phase,qt);
        numerator=glass_cmul(q0,a)-b;denominator=glass_cmul(q0,a)+b;
    } else {
        let r01=glass_cdiv(q0-qf,q0+qf);
        let r12=glass_cdiv(qf-qt,qf+qt);
        // This decaying form stays finite for thick evanescent films.
        let wave=exp(-2.0*phase.y)*vec2<f32>(cos(2.0*phase.x),sin(2.0*phase.x));
        let delayed=glass_cmul(r12,wave);
        numerator=r01+delayed;denominator=vec2<f32>(1.0,0.0)+glass_cmul(r01,delayed);
    }
    let reflected=glass_cdiv(numerator,denominator);
    return clamp(dot(reflected,reflected),0.0,1.0);
}
fn glass_film(cosine: f32, ni: f32, nt: f32, nf: f32, thickness: f32, wavelength: f32) -> vec2<f32> {
    let bare=dielectric(cosine,ni,nt);
    if thickness<=0.0 || nf==ni || nf==nt {return bare.xy;}
    if bare.z<0.0 {return vec2<f32>(1.0);}
    if cosine==0.0 {
        if ni!=nt || ni!=nf {return vec2<f32>(1.0);}
        return vec2<f32>(0.0);
    }
    return vec2<f32>(glass_film_polarization(cosine,ni,nt,nf,thickness,wavelength,false),glass_film_polarization(cosine,ni,nt,nf,thickness,wavelength,true));
}
fn glass_reflection(t: Triangle, cosine: f32, ni: f32, nt: f32, wavelength: f32) -> vec3<f32> {
    let bare=dielectric(cosine,ni,nt);
    if t.glass.z<=0.0 {return vec3<f32>((bare.x+bare.y)*0.5);}
    if wavelength>0.0 {
        let f=glass_film(cosine,ni,nt,t.glass.w,t.glass.z,wavelength);
        return vec3<f32>((f.x+f.y)*0.5);
    }
    var result=vec3<f32>(0.0);
    for(var channel=0u;channel<3u;channel+=1u) {
        let f=glass_film(cosine,ni,nt,t.glass.w,t.glass.z,glass_wavelength(channel));
        result[channel]=(f.x+f.y)*0.5;
    }
    return result;
}
fn glass_sheet(t: Triangle, f: vec3<f32>, cosine: f32, ni: f32, nt: f32, wavelength: f32) -> Sheet {
    if t.glass.z<=0.0 {return sheet(t,f);}
    if f.z<0.0 {return Sheet(vec3<f32>(1.0),vec3<f32>(0.0));}
    var attenuation=vec3<f32>(1.0);
    if t.parameters.w>0.0 {
        if f.z==0.0 {atomicOr(&failures[7],2u);return Sheet(vec3<f32>(0.0),vec3<f32>(0.0));}
        attenuation=exp(-t.optics.xyz*(t.parameters.w/f.z));
    }
    var result=Sheet(vec3<f32>(0.0),vec3<f32>(0.0));
    for(var channel=0u;channel<3u;channel+=1u) {
        let lambda=select(glass_wavelength(channel),wavelength,wavelength>0.0);
        let coated=glass_film(cosine,ni,nt,t.glass.w,t.glass.z,lambda);
        let s=sheet_polarization(coated.x,attenuation);
        let p=sheet_polarization(coated.y,attenuation);
        result.reflection[channel]=(s.reflection[channel]+p.reflection[channel])*0.5;
        result.transmission[channel]=(s.transmission[channel]+p.transmission[channel])*0.5;
    }
    return result;
}
fn glass_smith_weight(nv: f32, nl: f32, alpha: f32) -> f32 {
    let cv=abs(nv);let cl=abs(nl);
    if cv<=0.0 || cl<=0.0 {return 0.0;}
    let sv=sqrt(cv*cv+alpha*alpha*max(0.0,1.0-cv*cv));
    let sl=sqrt(cl*cl+alpha*alpha*max(0.0,1.0-cl*cl));
    // Height-correlated G2/G1 without squared tangent division or inf/inf.
    if cv>=cl {let ratio=cl/cv;return ratio*(cv+sv)/(sv*ratio+sl);}
    return (cv+sv)/(sv+sl*(cv/cl));
}
fn glass_visible_normal(n: vec3<f32>, view: vec3<f32>, alpha: f32, variates: vec2<f32>) -> vec3<f32> {
    let axis=select(vec3<f32>(0.0,0.0,1.0),vec3<f32>(1.0,0.0,0.0),abs(n.z)>=0.9);
    let x=unit(cross(axis,n));let y=cross(n,x);
    let v=vec3<f32>(dot(view,x),dot(view,y),dot(view,n));
    let stretched=unit(vec3<f32>(alpha*v.xy,v.z));
    let length_squared=dot(stretched.xy,stretched.xy);
    var t1=vec3<f32>(1.0,0.0,0.0);
    if length_squared>0.0 {t1=vec3<f32>(-stretched.y,stretched.x,0.0)/sqrt(length_squared);}
    let t2=cross(stretched,t1);
    let radius=sqrt(variates.x);let phi=2.0*PI*variates.y;
    let a=radius*cos(phi);var b=radius*sin(phi);
    let s=(1.0+stretched.z)*0.5;
    b=(1.0-s)*sqrt(max(0.0,1.0-a*a))+s*b;
    let normal=a*t1+b*t2+sqrt(max(0.0,1.0-a*a-b*b))*stretched;
    let h=unit(vec3<f32>(alpha*normal.xy,max(0.0,normal.z)));
    return unit(x*h.x+y*h.y+n*h.z);
}
struct GlassEvent {direction:vec3<f32>,weight:vec3<f32>,transmitted:bool,valid:bool};
fn glass_rough_active(t:Triangle,ni:f32,nt:f32)->bool {
    return t.glass.x>0.0 && !(ni==nt && (t.glass.z<=0.0 || t.glass.w==ni));
}
fn glass_macro_normal(incident:vec3<f32>,normal:vec3<f32>,geometric:vec3<f32>,ni:f32,nt:f32)->vec3<f32> {
    if glass_geometric_normals() {return geometric;}
    let f=dielectric(clamp(dot(normal,-incident),0.0,1.0),ni,nt);
    let transmitted=refract(incident,normal,ni/nt);
    if (f.z>=0.0 && dot(transmitted,geometric)>=0.0) || dot(reflect(incident,normal),geometric)<=0.0 {return geometric;}
    return normal;
}
fn glass_interface(t:Triangle,incident:vec3<f32>,macro_normal:vec3<f32>,geometric:vec3<f32>,ni:f32,nt:f32,wavelength:f32,radiance:bool,stratum:u32,strata:u32,rng:ptr<function,u32>) -> GlassEvent {
    var normal=macro_normal;
    let rough=glass_rough_active(t,ni,nt);
    if rough && dot(macro_normal,-incident)<=0.0 {return GlassEvent(incident,vec3<f32>(0.0),false,false);}
    if rough {normal=glass_visible_normal(macro_normal,-incident,t.glass.x*t.glass.x,vec2<f32>(random(rng),random(rng)));}
    let f=dielectric(clamp(dot(normal,-incident),0.0,1.0),ni,nt);
    let reflection=glass_reflection(t,clamp(dot(normal,-incident),0.0,1.0),ni,nt,wavelength);
    let probability=dot(reflection,vec3<f32>(1.0/3.0));
    var branch=random(rng);
    if strata>1u {branch=(f32(stratum)+branch)/f32(strata);*rng^=(stratum+1u)*0x85ebca6bu;}
    let reflected=branch<probability;
    var outgoing=reflect(incident,normal);var weight=vec3<f32>(0.0);
    if reflected {
        if dot(outgoing,geometric)<=0.0 {return GlassEvent(outgoing,weight,false,false);}
        weight=reflection/probability;
    } else {
        outgoing=unit(refract(incident,normal,ni/nt));
        if f.z<0.0 || dot(outgoing,geometric)>=0.0 {return GlassEvent(outgoing,weight,true,false);}
        weight=(vec3<f32>(1.0)-reflection)/(1.0-probability);
        if radiance {weight*=(ni/nt)*(ni/nt);}
    }
    if rough {weight*=glass_smith_weight(dot(macro_normal,-incident),dot(macro_normal,outgoing),t.glass.x*t.glass.x);}
    return GlassEvent(outgoing,weight,!reflected,true);
}
struct GlassSheetWalk {direction:vec3<f32>,weight:vec3<f32>,consumed:u32,valid:bool};
fn glass_rough_sheet(t:Triangle,incident:vec3<f32>,normal:vec3<f32>,outside:f32,inside:f32,wavelength:f32,radiance:bool,remaining:u32,stratum:u32,strata:u32,rng:ptr<function,u32>) -> GlassSheetWalk {
    var direction=incident;var weight=vec3<f32>(1.0);var internal=false;
    for(var event=0u;event<remaining;event+=1u) {
        let macro_normal=select(-normal,normal,dot(direction,normal)<0.0);
        let ni=select(outside,inside,internal);let nt=select(inside,outside,internal);
        let sampled=glass_interface(t,direction,macro_normal,macro_normal,ni,nt,wavelength,radiance,stratum,select(1u,strata,event==0u),rng);
        if !sampled.valid {return GlassSheetWalk(sampled.direction,vec3<f32>(0.0),event+1u,false);}
        direction=sampled.direction;weight*=sampled.weight;
        if (!internal && !sampled.transmitted) || (internal && sampled.transmitted) {return GlassSheetWalk(direction,weight,event+1u,true);}
        internal=true;
        let cosine=abs(dot(direction,normal));
        if cosine==0.0 {return GlassSheetWalk(direction,vec3<f32>(0.0),event+1u,false);}
        if t.parameters.w>0.0 {weight*=exp(-t.optics.xyz*(t.parameters.w/cosine));}
        if !any(weight>vec3<f32>(0.0)) {return GlassSheetWalk(direction,weight,event+1u,false);}
    }
    return GlassSheetWalk(direction,vec3<f32>(0.0),remaining,false);
}
