// Rain: drops running down a window over a misted view of the theme's background. Each drop
// lives in a cell of a grid across the view, starts at a random time and slides down its cell,
// leaving a few small droplets behind it. Two grids of different sizes make near and far drops.

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.547);
}

// How much of one grid of drops covers `uv` (0..1), and the drop's slope there for the light.
fn drops(uv: vec2<f32>, cells: vec2<f32>, speed: f32) -> vec3<f32> {
    let p = uv * cells;
    let column = floor(p.x);
    // Each column slides at its own pace, so the rows do not line up.
    let shift = hash2(vec2<f32>(column, 3.0));
    let y = p.y - sc.time * speed * (0.6 + 0.8 * shift) + shift * 10.0;
    let cell = vec2<f32>(column, floor(y));
    let local = vec2<f32>(fract(p.x), fract(y)) - 0.5;
    let seed = hash2(cell);
    // A drop in about half the cells, wobbling a little from side to side as it falls.
    let at = vec2<f32>(0.3 * (hash2(cell + 7.0) - 0.5) + 0.05 * sin(y * 6.0 + seed * 6.2831853), 0.0);
    let d = (local - at) * vec2<f32>(1.0, 1.6);
    let drop = (1.0 - smoothstep(0.10, 0.16, length(d))) * step(0.5, seed);
    // Droplets left above it.
    let trail_at = vec2<f32>(at.x, local.y - fract(local.y * 4.0) / 4.0 + 0.125);
    let trail = (1.0 - smoothstep(0.02, 0.05, distance(local, trail_at))) * step(0.5, seed) * step(local.y, -0.05) * 0.8;
    let cover = max(drop, trail);
    return vec3<f32>(cover, d * cover);
}

fn background(px: ScPixel) -> vec4<f32> {
    let aspect = sc.size.x / max(sc.size.y, 1.0);
    let uv = vec2<f32>(px.uv.x * aspect, px.uv.y);
    // The misted view: the theme's background, a little darker and bluer.
    let view = mix(sc.top.rgb, sc.bottom.rgb, px.uv.y);
    let mist = mix(view, vec3<f32>(0.20, 0.26, 0.34) * (0.4 + 0.6 * sc.dark), 0.25);
    let near = drops(uv, vec2<f32>(12.0, 5.0), 0.35);
    let far = drops(uv + 0.37, vec2<f32>(26.0, 11.0), 0.25);
    let cover = clamp(near.x + 0.6 * far.x, 0.0, 1.0);
    // A drop shows the view through it, brighter on its upper side where it catches the light.
    let light = clamp(0.5 - 2.0 * (near.z + far.z), 0.0, 1.0);
    let through = mix(view, sc.text.rgb, 0.08) + light * 0.15;
    return vec4<f32>(mix(mist, through, cover), 1.0);
}
