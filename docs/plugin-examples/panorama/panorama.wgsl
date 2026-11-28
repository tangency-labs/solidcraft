// Panorama: a 360° picture (twice as wide as it is tall, Z up) around the view. It turns when you
// orbit and stays put when you zoom or pan.

const TAU: f32 = 6.2831853;
const PI: f32 = 3.1415927;

fn background(px: ScPixel) -> vec4<f32> {
    let ray = px.ray;
    let uv = vec2<f32>(atan2(ray.y, ray.x) / TAU + 0.5, 0.5 - asin(clamp(ray.z, -1.0, 1.0)) / PI);
    // Pick the mip level by hand: where u wraps from 1 back to 0 the GPU's own choice is a tiny
    // level and draws a seam. The picture spans PI radians over its height, and the view shows
    // 2 atan(|up|) radians over sc.size.y pixels.
    let texels = f32(textureDimensions(picture, 0).y);
    let view = 2.0 * atan(length(sc.up.xyz));
    let level = max(log2(texels / PI * view / max(sc.size.y, 1.0)), 0.0);
    return vec4<f32>(textureSampleLevel(picture, sc_repeat, uv, level).rgb, 1.0);
}
