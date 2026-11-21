// Galaxy: stars and a band of glowing dust round the sky, fixed to the world so they turn with
// the view. Not animated: it is drawn again only when the view turns, resizes or the theme changes.

fn hash3(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.547);
}

// Smooth value noise in 3D.
fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let b = mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let c = mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let d = mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}

// One layer of stars: the cells of a grid round the view direction, one star in some of them.
fn stars(ray: vec3<f32>, cells: f32, density: f32) -> f32 {
    let p = ray * cells;
    let cell = floor(p);
    let seed = hash3(cell);
    let centre = cell + 0.5 + 0.35 * (vec3<f32>(hash3(cell + 1.7), hash3(cell + 3.1), hash3(cell + 5.3)) - 0.5);
    let size = 0.04 + 0.06 * hash3(cell + 9.2);
    let glow = 1.0 - smoothstep(0.0, size, distance(p, centre));
    return glow * step(1.0 - density, seed) * (0.4 + 0.6 * hash3(cell + 4.4));
}

fn background(px: ScPixel) -> vec4<f32> {
    let ray = px.ray;
    // The galactic band: a great circle tilted against the horizon.
    let pole = normalize(vec3<f32>(0.35, -0.45, 0.82));
    let across = dot(ray, pole);
    var dust = 0.0;
    var amplitude = 0.5;
    var scale = 3.0;
    for (var i = 0; i < 5; i++) {
        dust += amplitude * noise3(ray * scale);
        amplitude *= 0.5;
        scale *= 2.0;
    }
    let band = exp(-across * across / 0.03) * smoothstep(0.35, 0.75, dust);
    let core = exp(-across * across / 0.004) * 0.6;
    let deep = vec3<f32>(0.004, 0.005, 0.016);
    let violet = vec3<f32>(0.10, 0.05, 0.22);
    let blue = vec3<f32>(0.05, 0.12, 0.30);
    var colour = deep + band * mix(violet, blue, noise3(ray * 6.0 + 11.0)) + core * band * vec3<f32>(0.35, 0.28, 0.22);
    // Stars, denser in the band.
    let many = stars(ray, 220.0, 0.06 + 0.25 * band) + stars(ray, 90.0, 0.03) * 1.5;
    colour += many * vec3<f32>(0.9, 0.92, 1.0);
    return vec4<f32>(colour, 1.0);
}
