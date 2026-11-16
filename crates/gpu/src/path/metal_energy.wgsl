// Original directional-loss fit for SolidCraft's separable Smith GGX. Keep the
// coefficients and cosine moments in sync with render/src/surface.rs.
struct MetalPolynomial { low: vec4<f32>, high: vec4<f32> };
fn metal_chebyshev(x: f32) -> MetalPolynomial {
    let x2 = 2.0*x*x-1.0;
    let x3 = 2.0*x*x2-x;
    let x4 = 2.0*x*x3-x2;
    let x5 = 2.0*x*x4-x3;
    let x6 = 2.0*x*x5-x4;
    let x7 = 2.0*x*x6-x5;
    return MetalPolynomial(vec4<f32>(1.0,x,x2,x3), vec4<f32>(x4,x5,x6,x7));
}
fn metal_loss_curve(roughness: f32) -> MetalPolynomial {
    let rough = metal_chebyshev(2.0*(roughness-0.35)/0.65-1.0);
    return MetalPolynomial(
        vec4<f32>(
        dot(rough.low, vec4<f32>(0.266099230744, 0.234143736162, 0.030578608252, -0.006555488201))
            + dot(rough.high.xy, vec2<f32>(-0.001408821529, 0.000338338127)) - 0.003,
        dot(rough.low, vec4<f32>(0.062815590935, 0.109326249066, -0.000168741912, -0.008651318409))
            + dot(rough.high.xy, vec2<f32>(-0.000234806591, 0.000537478808)),
        dot(rough.low, vec4<f32>(-0.012483235801, 0.028318374577, 0.005800323841, -0.006920257497))
            + dot(rough.high.xy, vec2<f32>(0.000309127847, 0.000486378250)),
        dot(rough.low, vec4<f32>(-0.004382182237, -0.011917917997, 0.008966558426, -0.002826443547))
            + dot(rough.high.xy, vec2<f32>(-0.000881066054, 0.000652337664))
        ),
        vec4<f32>(
        dot(rough.low, vec4<f32>(0.004792378381, -0.005261311499, 0.000874914479, 0.001861663053))
            + dot(rough.high.xy, vec2<f32>(-0.001449625147, 0.000284442998)),
        dot(rough.low, vec4<f32>(-0.001334206261, 0.003343183987, -0.002951722839, 0.001555479216))
            + dot(rough.high.xy, vec2<f32>(-0.000159594280, -0.000303749560)),
        dot(rough.low, vec4<f32>(-0.000540498387, 0.000522348781, 0.000261748019, -0.000812746047))
            + dot(rough.high.xy, vec2<f32>(0.000729308912, -0.000316627573)),
        dot(rough.low, vec4<f32>(0.000576930968, -0.001089750952, 0.000855587383, -0.000437065280))
            + dot(rough.high.xy, vec2<f32>(0.000037708235, 0.000134962401))
        )
    );
}
fn metal_directional_loss(curve: MetalPolynomial, mu: f32) -> f32 {
    let direction = metal_chebyshev(2.0*sqrt(mu)-1.0);
    return dot(curve.low,direction.low)+dot(curve.high,direction.high);
}
fn metal_multiple_scattering(color: vec3<f32>, metallic: f32, roughness: f32, nv: f32, nl: f32) -> vec3<f32> {
    let curve = metal_loss_curve(roughness);
    let mean_loss = dot(curve.low, vec4<f32>(1.000000000000, 0.600000000000, -0.066666666667, -0.314285714286))
        + dot(curve.high, vec4<f32>(-0.219047619048, -0.111111111111, -0.066666666667, -0.046464646465));
    let strength = smoothstep(0.35,0.45,roughness);
    let lobe = metallic*strength*metal_directional_loss(curve,nv)*metal_directional_loss(curve,nl)/(PI*mean_loss);
    let mean_fresnel = color+(vec3<f32>(1.0)-color)/21.0;
    return lobe*mean_fresnel*mean_fresnel*(1.0-mean_loss)/(vec3<f32>(1.0)-mean_fresnel*mean_loss);
}
