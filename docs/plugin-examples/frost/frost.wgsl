// Frost: what is behind the panel, blurred, tinted and edged with a 1 px light rim.

fn panel(px: ScPanel) -> vec4<f32> {
    let tint = sc.settings.x;
    let behind = sc_blurred(px.position).rgb;
    // Towards white on a light theme and towards the text colour (light) on a dark one.
    let towards = mix(vec3<f32>(1.0), sc.text.rgb, sc.dark);
    var colour = mix(behind, towards, tint);
    // edge runs from 0 at the panel's edge to negative inside; the last pixel gets the rim.
    let rim = 1.0 - smoothstep(0.0, 1.0, -px.edge);
    colour = mix(colour, vec3<f32>(1.0), 0.35 * rim);
    return vec4<f32>(colour, 1.0);
}
