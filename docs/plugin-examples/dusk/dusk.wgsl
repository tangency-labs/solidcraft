// Dusk: a sky behind the model in the theme's own colours. The horizon follows the world's Z, so
// it tilts and turns with the camera, and a slow glow drifts along it.

fn background(px: ScPixel) -> vec4<f32> {
    // How far above the horizon this pixel looks: -1 straight down, 1 straight up.
    let height = px.ray.z;
    let sky = mix(sc.bottom.rgb, sc.top.rgb, smoothstep(-0.2, 0.6, height));
    // A band of the theme's accent just above the horizon, moving round it once a minute.
    let heading = atan2(px.ray.y, px.ray.x);
    let drift = 0.5 + 0.5 * cos(heading - sc.time * 6.2831853 / 60.0);
    let band = exp(-pow((height - 0.05) / 0.08, 2.0)) * mix(0.15, 0.4, drift);
    return vec4<f32>(mix(sky, sc.accent.rgb, band), 1.0);
}
