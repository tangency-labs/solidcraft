// Accent sweep: a soft band of the theme's accent sweeps across the window from left to right,
// lighting the ribbon's icons and button blocks as it passes. Panels are left as they are.

fn tint(r: ScRegion) -> vec4<f32> {
    let width = sc.settings.x;
    let period = sc.settings.y;
    // From just off the left edge to just off the right one, in points.
    let travel = sc.size.x / sc.scale + 2.0 * width;
    let centre = fract(sc.time / period) * travel - width;
    let d = (r.position.x - centre) / width;
    let band = exp(-d * d);
    if r.kind == SC_ICON {
        return vec4<f32>(sc.accent.rgb, band * r.enabled);
    }
    if r.kind == SC_BUTTON {
        return vec4<f32>(sc.accent.rgb, 0.12 * band * r.enabled);
    }
    return vec4<f32>(0.0);
}
