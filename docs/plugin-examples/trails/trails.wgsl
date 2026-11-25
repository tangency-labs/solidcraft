// Trails: a state example. The state keeps a brightness for every texel; each step it fades a
// little and lights the texels under a spot that circles the view. The background shows the
// brightness as the theme's accent over its background.

fn state(px: ScStatePixel) -> vec4<f32> {
    let fade = textureLoad(sc_state_prev, vec2<i32>(px.texel), 0).r * 0.97;
    let spot = vec2<f32>(0.5 + 0.3 * cos(sc.frame * 0.05), 0.5 + 0.3 * sin(sc.frame * 0.05));
    return vec4<f32>(max(fade, 1.0 - smoothstep(0.02, 0.05, distance(px.uv, spot))), 0.0, 0.0, 1.0);
}

fn background(px: ScPixel) -> vec4<f32> {
    let trail = textureLoad(sc_state, vec2<i32>(px.uv * sc.state_size), 0).r;
    return vec4<f32>(mix(sc.bottom.rgb, sc.accent.rgb, trail), 1.0);
}
