//! Capability `shaders@0.2` (and 0.1, backgrounds only): a plugin writes one WGSL function,
//! `background` or `tint`; the host checks it and adds the entry points, uniforms, samplers,
//! textures, settings and a loop cap.
//!
//! A background shader is source like this, in the plugin's folder and named in its manifest:
//!
//! ```wgsl
//! fn background(px: ScPixel) -> vec4<f32> {
//!     let glow = 0.5 + 0.5 * px.ray.z;
//!     return mix(sc.bottom, sc.top, glow);
//! }
//! ```

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HostError, Result, invalid, valid_id};

pub const MAX_SHADERS: usize = 8;
/// The largest shader source, in bytes.
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_TEXTURES: usize = 4;
/// The most sliders one shader has on its plugin's card (`sc.settings`).
pub const MAX_SETTINGS: usize = 4;
/// The largest texture file, in bytes.
pub const MAX_TEXTURE_BYTES: usize = 16 * 1024 * 1024;
/// The longest side of a texture after the host loads it; larger pictures are scaled down.
pub const MAX_TEXTURE_SIDE: u32 = 8192;
/// Loop iterations and calls of the plugin's own functions one pixel may make, all together. A
/// loop that reaches the cap ends as if its condition had failed, and a call returns zero at once,
/// so the work per pixel is bounded by the budget times the size of the source.
pub const LOOP_BUDGET: u32 = 1024;

/// What a shader draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShaderKind {
    /// Behind the model in the 3D view, over the theme's background.
    Background,
    /// A colour for each ribbon icon, button block and panel the host asks about (`shaders@0.2`).
    Tint,
    /// Each see-through panel's background, from what is behind the panel (`shaders@0.2`).
    Panel,
}

impl ShaderKind {
    /// The function the plugin's source must define.
    fn function(self) -> &'static str {
        match self {
            ShaderKind::Background => "background",
            ShaderKind::Tint => "tint",
            ShaderKind::Panel => "panel",
        }
    }

    fn signature(self) -> &'static str {
        match self {
            ShaderKind::Background => "needs fn background(px: ScPixel) -> vec4<f32>",
            ShaderKind::Tint => "needs fn tint(r: ScRegion) -> vec4<f32>",
            ShaderKind::Panel => "needs fn panel(px: ScPanel) -> vec4<f32>",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShaderTexture {
    /// The name the shader uses for it, a WGSL identifier (`sky`, `city_lights`).
    pub name: String,
    /// A `.png` or `.jpg` file name in the plugin's folder.
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shader {
    /// Unique within the plugin; the app names it `<plugin-id>/<id>`.
    pub id: String,
    pub name: String,
    pub kind: ShaderKind,
    /// A `.wgsl` file name in the plugin's folder.
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<ShaderTexture>,
    /// Redrawn by itself, about 30 times a second, while it is shown. Off: drawn only when the
    /// view changes, and `sc.time` stays at 0.
    #[serde(default)]
    pub animated: bool,
    /// Sliders on the plugin's card, read as `sc.settings` in order (`shaders@0.2`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<ShaderSetting>,
    /// A panel shader's blur radius in pixels (1 to 60), for `sc_blurred`; default 12.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur: Option<f32>,
}

/// A panel shader's blur radius when its manifest names none.
pub const DEFAULT_BLUR: f32 = 12.0;

/// A slider on the plugin's card; the shader reads its value as one component of `sc.settings`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShaderSetting {
    pub id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

// The blur radius is never NaN (validation), so equality is total.
impl Eq for Shader {}

impl Eq for ShaderSetting {}

impl Shader {
    /// The blur radius the host prepares a panel shader's `sc_blurred` with.
    pub fn blur_radius(&self) -> f32 {
        self.blur.unwrap_or(DEFAULT_BLUR)
    }

    /// The values for `sc.settings`: each setting's value from `value` (by id) within its range,
    /// else its default; 0 past the last setting.
    pub fn setting_values(&self, value: impl Fn(&str) -> Option<f32>) -> [f32; MAX_SETTINGS] {
        let mut out = [0.0; MAX_SETTINGS];
        for (slot, s) in out.iter_mut().zip(&self.settings) {
            *slot = value(&s.id).filter(|v| v.is_finite()).map_or(s.default, |v| v.clamp(s.min, s.max));
        }
        out
    }
}

fn plain_file(name: &str, extensions: &[&str]) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else { return false };
    extensions.contains(&extension)
        && (1..=64).contains(&stem.len())
        && stem.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
        && !stem.starts_with(['-', '_'])
}

/// A WGSL identifier the host does not reserve: lowercase, digits and `_`, not starting with
/// `sc_` or a digit, at most 32 characters.
fn texture_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && !name.starts_with("sc_")
        && !matches!(name, "sc" | "background" | "tint" | "panel")
}

/// Checks a manifest's `shaders` list: count, distinct ids, file names and texture names.
pub fn validate(shaders: &[Shader]) -> Result<()> {
    if shaders.len() > MAX_SHADERS {
        return Err(invalid("shaders", format!("at most {MAX_SHADERS} shaders")));
    }
    let mut ids = BTreeSet::new();
    for shader in shaders {
        let field = format!("shaders.{}", shader.id);
        if !valid_id(&shader.id) || !ids.insert(&shader.id) {
            return Err(invalid("shaders", "each shader needs a distinct lowercase id"));
        }
        if !crate::plain(&shader.name, 80) {
            return Err(invalid(format!("{field}.name"), "requires 1..80 printable characters"));
        }
        if !plain_file(&shader.source, &["wgsl"]) {
            return Err(invalid(format!("{field}.source"), "must be one lowercase .wgsl file name in the plugin's folder"));
        }
        if shader.textures.len() > MAX_TEXTURES {
            return Err(invalid(format!("{field}.textures"), format!("at most {MAX_TEXTURES} textures")));
        }
        let mut names = BTreeSet::new();
        for texture in &shader.textures {
            if !texture_name(&texture.name) || !names.insert(&texture.name) {
                return Err(invalid(
                    format!("{field}.textures"),
                    "each texture needs a distinct WGSL name: lowercase letters, digits and _, not starting with sc_",
                ));
            }
            if !plain_file(&texture.file, &["png", "jpg", "jpeg"]) {
                return Err(invalid(format!("{field}.textures.{}", texture.name), "must be one lowercase .png or .jpg file name"));
            }
        }
        if shader.blur.is_some_and(|b| shader.kind != ShaderKind::Panel || !(b.is_finite() && (1.0..=60.0).contains(&b))) {
            return Err(invalid(format!("{field}.blur"), "belongs to a panel shader, 1 to 60 pixels"));
        }
        if shader.settings.len() > MAX_SETTINGS {
            return Err(invalid(format!("{field}.settings"), format!("at most {MAX_SETTINGS} settings")));
        }
        let mut setting_ids = BTreeSet::new();
        for s in &shader.settings {
            let range = s.min.is_finite() && s.max.is_finite() && s.min < s.max && (s.min..=s.max).contains(&s.default);
            if !valid_id(&s.id) || !setting_ids.insert(&s.id) || !crate::plain(&s.name, 80) || !range {
                return Err(invalid(
                    format!("{field}.settings"),
                    "each setting needs a distinct lowercase id, a name, and min < max with the default between them",
                ));
            }
        }
    }
    Ok(())
}

/// The host's uniforms, samplers and textures, the same for every kind. `{textures}` becomes one
/// binding per declared texture.
macro_rules! host_declarations {
    () => {
        r"
// ---- SolidCraft host (shaders@0.2) ----
struct ScHost {
    // The view's size in pixels (a tint: the window's).
    size: vec2<f32>,
    // Seconds, for an animated shader; 0 otherwise. Comes back to 0 each hour.
    time: f32,
    // Pixels per point (the display's scale).
    scale: f32,
    // The camera's right, up and forward axes in world space (Z up), right and up scaled to the
    // view's half width and half height at distance 1.
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    // The theme's colours, linear (not the sRGB numbers of a colour picker), alpha 1: the view's
    // background at its top and bottom, the accent, and text.
    top: vec4<f32>,
    bottom: vec4<f32>,
    accent: vec4<f32>,
    text: vec4<f32>,
    // 1 for a dark theme, 0 for a light one.
    dark: f32,
    sc_pad0: f32,
    sc_pad1: f32,
    sc_pad2: f32,
    // The values of the shader's settings, in the order its manifest lists them; 0 past the last.
    settings: vec4<f32>,
}

struct ScPixel {
    // 0..1 across the view, from its top left.
    uv: vec2<f32>,
    // In pixels from the view's top left.
    position: vec2<f32>,
    // The direction this pixel looks along, in world space, unit length.
    ray: vec3<f32>,
}

@group(0) @binding(0) var<uniform> sc: ScHost;
@group(0) @binding(1) var sc_clamp: sampler;
@group(0) @binding(2) var sc_repeat: sampler;
{textures}
"
    };
}

/// What the host declares around a background shader's source. The plugin's source comes first,
/// so the line numbers in an error are the plugin's own.
pub const PRELUDE: &str = concat!(
    host_declarations!(),
    r"struct ScVarying {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) ray: vec3<f32>,
}

@vertex
fn sc_vertex(@builtin(vertex_index) index: u32) -> ScVarying {
    let ndc = vec2<f32>(f32((index << 1u) & 2u) * 2.0 - 1.0, f32(index & 2u) * 2.0 - 1.0);
    var out: ScVarying;
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    out.ray = sc.forward.xyz + ndc.x * sc.right.xyz + ndc.y * sc.up.xyz;
    return out;
}

// The view shows the stored texel as a screen (sRGB) value and blends premultiplied colour on
// screen values, as egui does: store srgb(rgb) * a in a plain (not sRGB) target.
fn sc_encode(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

@fragment
fn sc_fragment(v: ScVarying) -> @location(0) vec4<f32> {
    let colour = clamp(background(ScPixel(v.uv, v.uv * sc.size, normalize(v.ray))), vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(sc_encode(colour.rgb) * colour.a, colour.a);
}
"
);

/// What the host declares around a tint shader's source: the region it asks about and the compute
/// entry point that answers a list of them.
pub const TINT_PRELUDE: &str = concat!(
    host_declarations!(),
    r"
const SC_ICON: u32 = 0u;
const SC_BUTTON: u32 = 1u;
const SC_PANEL: u32 = 2u;

struct ScRegion {
    // Left, top, right and bottom, in points from the window's top left.
    rect: vec4<f32>,
    // The region's own colour, linear.
    base: vec4<f32>,
    // The point asked about, in points: an icon's or button's centre, or a point on a panel.
    position: vec2<f32>,
    // SC_ICON, SC_BUTTON or SC_PANEL.
    kind: u32,
    // 1 for an enabled control, less for a disabled one.
    enabled: f32,
}

@group(1) @binding(0) var<storage, read> sc_regions: array<ScRegion>;
@group(1) @binding(1) var<storage, read_write> sc_tints: array<vec4<f32>>;

@compute @workgroup_size(64)
fn sc_tint_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&sc_tints) {
        return;
    }
    sc_tints[id.x] = clamp(tint(sc_regions[id.x]), vec4<f32>(0.0), vec4<f32>(1.0));
}
"
);

/// The bytes of one `ScRegion` in the host's list.
pub const TINT_REGION_BYTES: usize = 48;
/// The most regions the host asks about in one frame; more are left untinted.
pub const MAX_TINT_REGIONS: usize = 8192;
/// Threads in one `sc_tint_main` workgroup.
pub const TINT_WORKGROUP: u32 = 64;

/// What the host declares around a panel shader's source (`shaders@0.2`). One draw per see-through
/// panel, over the whole window with the panel's rect as the scissor; the host masks the result to
/// the panel's rounded rect and blends it under the panel's own fill.
pub const PANEL_PRELUDE: &str = concat!(
    host_declarations!(),
    r"
struct ScPanelRect {
    rect: vec4<f32>,
    corners: vec4<f32>,
    state: vec4<f32>,
    info: vec4<f32>,
}

struct ScPanel {
    // In pixels from the window's top left; sc_backdrop and sc_blurred take this.
    position: vec2<f32>,
    // In pixels from the panel's top left, and the panel's size.
    local: vec2<f32>,
    size: vec2<f32>,
    // 0..1 across the panel.
    uv: vec2<f32>,
    // The outward normal of the panel's rounded edge nearest this pixel.
    normal: vec2<f32>,
    // The corner radius in pixels, and the distance to the edge: negative inside.
    radius: f32,
    edge: f32,
    // 1 while the pointer is over the panel.
    hovered: f32,
    // 1 while the keyboard focus is in the panel.
    focused: f32,
    // 1 for a window or dialog over the others, 0 for a docked panel or bar.
    floating: f32,
    // The blur radius of sc_blurred, in pixels.
    blur: f32,
}

@group(0) @binding(7) var<uniform> sc_panel: ScPanelRect;
@group(0) @binding(8) var sc_scene: texture_2d<f32>;
@group(0) @binding(9) var sc_scene_blurred: texture_2d<f32>;

@vertex
fn sc_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}

fn sc_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

fn sc_gamma(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), c > vec3<f32>(0.0031308));
}

// What is behind the panels at window pixel p, linear: the 3D view and everything drawn before.
fn sc_backdrop(p: vec2<f32>) -> vec4<f32> {
    let c = textureSampleLevel(sc_scene, sc_clamp, p / vec2<f32>(textureDimensions(sc_scene)), 0.0);
    return vec4<f32>(select(sc_linear(c.rgb), c.rgb, sc_panel.info.y > 0.5), c.a);
}

// The same, blurred by sc_panel.info.x pixels. Valid within about two blur radii of a panel.
fn sc_blurred(p: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(sc_scene_blurred, sc_clamp, p / vec2<f32>(textureDimensions(sc_scene_blurred)), 0.0);
}

@fragment
fn sc_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let lo = sc_panel.rect.xy;
    let hi = sc_panel.rect.zw;
    let half = (hi - lo) * 0.5;
    let rel = at.xy - (lo + hi) * 0.5;
    let c = sc_panel.corners;
    let corner = select(select(c.w, c.z, rel.x < 0.0), select(c.y, c.x, rel.x < 0.0), rel.y < 0.0);
    let radius = min(corner, min(half.x, half.y));
    let q = abs(rel) - half + vec2<f32>(radius);
    let edge = min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - radius;
    var normal = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), q.x > q.y);
    if q.x > 0.0 && q.y > 0.0 {
        normal = normalize(q);
    }
    normal = normal * select(vec2<f32>(-1.0), vec2<f32>(1.0), rel >= vec2<f32>(0.0));
    let size = max(hi - lo, vec2<f32>(1.0));
    let s = sc_panel.state;
    let px = ScPanel(at.xy, at.xy - lo, hi - lo, (at.xy - lo) / size, normal, radius, edge, s.x, s.y, s.z, sc_panel.info.x);
    let colour = clamp(panel(px), vec4<f32>(0.0), vec4<f32>(1.0));
    // Premultiplied on screen values as egui blends; a linear (sRGB) target encodes it back.
    let alpha = colour.a * clamp(0.5 - edge, 0.0, 1.0);
    let premultiplied = sc_gamma(colour.rgb) * alpha;
    return vec4<f32>(select(premultiplied, sc_linear(premultiplied), sc_panel.info.y > 0.5), alpha);
}
"
);

/// A panel shader's per-panel uniform binding, after the most textures a shader may have.
pub const PANEL_RECT_BINDING: u32 = FIRST_TEXTURE_BINDING + MAX_TEXTURES as u32;
/// The size of `ScPanelRect`, one per panel.
pub const PANEL_RECT_UNIFORM_BYTES: usize = 64;

/// The first texture's binding; the textures follow in manifest order.
pub const FIRST_TEXTURE_BINDING: u32 = 3;
/// The size of `ScHost` in bytes, as the host writes it.
pub const HOST_UNIFORM_BYTES: usize = 160;

/// A shader, checked and ready for the GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    /// The whole module: the plugin's source with the loop cap, and the host's declarations.
    /// Entry points `sc_vertex` and `sc_fragment` for a background, `sc_tint_main` for a tint.
    pub wgsl: String,
    /// What the module binds in group 0, in binding order, for the host to match its layout.
    pub bindings: Vec<(u32, Resource)>,
}

/// One resource a compiled shader binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// A uniform buffer of this many bytes.
    Uniform(u64),
    Sampler,
    /// A filterable 2D float texture.
    Texture,
    /// Anything else (a tint's storage buffers).
    Other,
}

fn bindings(module: &naga::Module) -> Vec<(u32, Resource)> {
    let mut out: Vec<(u32, Resource)> = module
        .global_variables
        .iter()
        .filter_map(|(_, g)| {
            let binding = g.binding.as_ref().filter(|b| b.group == 0)?;
            let resource = match (&module.types[g.ty].inner, g.space) {
                (naga::TypeInner::Sampler { .. }, _) => Resource::Sampler,
                (naga::TypeInner::Image { .. }, _) => Resource::Texture,
                (inner, naga::AddressSpace::Uniform) => Resource::Uniform(u64::from(inner.size(module.to_ctx()))),
                _ => Resource::Other,
            };
            Some((binding.binding, resource))
        })
        .collect();
    out.sort_by_key(|(binding, _)| *binding);
    out
}

/// The host's own functions in a kind's prelude (besides its entry points). They spend no budget,
/// so a pixel that spent it all still gets its colour encoded for the screen.
fn host_functions(kind: ShaderKind) -> &'static [&'static str] {
    match kind {
        ShaderKind::Background | ShaderKind::Tint => &["sc_encode"],
        ShaderKind::Panel => &["sc_linear", "sc_gamma", "sc_backdrop", "sc_blurred"],
    }
}

fn refuse(shader: &Shader, reason: impl Into<String>) -> HostError {
    invalid(format!("shaders.{}", shader.id), format!("{}: {}", shader.source, reason.into()))
}

/// Refuses entry points, resource bindings or overrides of the plugin's own, and a missing
/// `background` or `tint` function (by the shader's kind).
pub fn compile(shader: &Shader, source: &str) -> Result<Compiled> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(refuse(shader, format!("larger than {} KiB", MAX_SOURCE_BYTES / 1024)));
    }
    let textures: String = shader
        .textures
        .iter()
        .enumerate()
        .map(|(i, t)| format!("@group(0) @binding({}) var {}: texture_2d<f32>;\n", FIRST_TEXTURE_BINDING as usize + i, t.name))
        .collect();
    let lines = source.lines().count();
    let prelude = match shader.kind {
        ShaderKind::Background => PRELUDE,
        ShaderKind::Tint => TINT_PRELUDE,
        ShaderKind::Panel => PANEL_PRELUDE,
    };
    let (function, needs) = (shader.kind.function(), shader.kind.signature());
    let full = format!("{source}\n{}", prelude.replace("{textures}", &textures));
    let at = |location: Option<naga::SourceLocation>| match location {
        Some(l) if l.line_number as usize <= lines => format!("line {}: ", l.line_number),
        Some(_) => "where it meets the host's declarations (a name starting sc or ScHost?): ".to_string(),
        None => String::new(),
    };
    let mut module = naga::front::wgsl::parse_str(&full).map_err(|e| {
        // Without the plugin's own function, the host's call to it is what fails to parse.
        let in_host = e.location(&full).is_some_and(|l| l.line_number as usize > lines);
        if in_host && e.message().contains(function) {
            refuse(shader, needs)
        } else {
            refuse(shader, format!("{}{}", at(e.location(&full)), e.message()))
        }
    })?;
    if !module.functions.iter().any(|(_, f)| f.name.as_deref() == Some(function)) {
        return Err(refuse(shader, needs));
    }
    let mut host: BTreeSet<&str> = ["sc", "sc_clamp", "sc_repeat"].into_iter().chain(shader.textures.iter().map(|t| t.name.as_str())).collect();
    if shader.kind == ShaderKind::Tint {
        host.extend(["sc_regions", "sc_tints"]);
    }
    if shader.kind == ShaderKind::Panel {
        host.extend(["sc_panel", "sc_scene", "sc_scene_blurred"]);
    }
    if module.entry_points.iter().any(|e| !matches!(e.name.as_str(), "sc_vertex" | "sc_fragment" | "sc_tint_main")) {
        return Err(refuse(shader, "declares an entry point (@vertex, @fragment or @compute); the host supplies them"));
    }
    if let Some((_, global)) = module
        .global_variables
        .iter()
        .find(|(_, g)| !(g.space == naga::AddressSpace::Private && g.binding.is_none()) && !host.contains(g.name.as_deref().unwrap_or("")))
    {
        return Err(refuse(
            shader,
            format!(
                "declares the resource `{}`; a shader reads only what the host binds: sc, sc_clamp, sc_repeat, the textures its manifest names, a tint's sc_regions and sc_tints, and a panel's sc_panel, sc_scene and sc_scene_blurred",
                global.name.as_deref().unwrap_or("?")
            ),
        ));
    }
    // The host's names start with sc_: a plugin function taking one could pass for the host's.
    if let Some((_, function)) = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref().is_some_and(|name| name.starts_with("sc_") && !host_functions(shader.kind).contains(&name)))
    {
        return Err(refuse(
            shader,
            format!("declares the function `{}`; names starting sc_ belong to the host", function.name.as_deref().unwrap_or("?")),
        ));
    }
    // The region list and the answers are the entry point's: a plugin function reading or writing
    // them would see or change other regions.
    let entry_only: Vec<_> =
        module.global_variables.iter().filter(|(_, g)| matches!(g.name.as_deref(), Some("sc_regions" | "sc_tints"))).map(|(h, _)| h).collect();
    for (_, function) in module.functions.iter() {
        let used = function.expressions.iter().find(|(_, e)| matches!(e, naga::Expression::GlobalVariable(g) if entry_only.contains(g)));
        if let Some((handle, naga::Expression::GlobalVariable(global))) = used {
            let name = module.global_variables[*global].name.as_deref().unwrap_or("?");
            let span = function.expressions.get_span(handle);
            let location = span.is_defined().then(|| span.location(&full));
            return Err(refuse(shader, format!("{}uses `{name}`; the host passes the region to tint and keeps its answer", at(location))));
        }
    }
    if !module.overrides.is_empty() {
        return Err(refuse(shader, "declares an override; use a const"));
    }
    budget_loops(&mut module, host_functions(shader.kind));
    let info =
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).map_err(|e| {
            // The innermost span is the expression or statement at fault, not its whole function.
            let location = e.spans().filter(|(span, _)| span.is_defined()).last().map(|(span, _)| span.location(&full));
            refuse(shader, format!("{}{}", at(location), error_chain(&e)))
        })?;
    let wgsl = naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
        .map_err(|e| refuse(shader, format!("could not be prepared: {e}")))?;
    Ok(Compiled { wgsl, bindings: bindings(&module) })
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(e) = source {
        text.push_str(": ");
        text.push_str(&e.to_string());
        source = e.source();
    }
    text
}

/// One shared per-pixel [`LOOP_BUDGET`] for every loop and plugin function call in `module`; a
/// loop in a loop shares the same count, so nesting does not multiply it.
fn budget_loops(module: &mut naga::Module, host_functions: &[&str]) {
    use naga::{Expression, Literal, Scalar, Span, Type, TypeInner};
    let u32_ty = module.types.insert(Type { name: None, inner: TypeInner::Scalar(Scalar::U32) }, Span::UNDEFINED);
    let init = module.global_expressions.append(Expression::Literal(Literal::U32(LOOP_BUDGET)), Span::UNDEFINED);
    let counter = module.global_variables.append(
        naga::GlobalVariable {
            name: Some("sc_loop_budget".into()),
            space: naga::AddressSpace::Private,
            binding: None,
            ty: u32_ty,
            init: Some(init),
            memory_decorations: Default::default(),
        },
        Span::UNDEFINED,
    );
    for (_, function) in module.functions.iter_mut() {
        budget_function(function, counter);
        // Calls spend the budget too, so fan-out without loops stays bounded. Host helpers don't,
        // so a spent pixel still gets encoded; `compile` refuses plugin functions named sc_.
        if function.name.as_deref().is_some_and(|name| host_functions.contains(&name)) {
            continue;
        }
        let zero = function.result.as_ref().map(|result| function.expressions.append(Expression::ZeroValue(result.ty), Span::UNDEFINED));
        let entry = spend(&mut function.expressions, counter, naga::Statement::Return { value: zero });
        function.body.splice(0..0, entry);
    }
    for entry in &mut module.entry_points {
        budget_function(&mut entry.function, counter);
    }
}

fn budget_function(function: &mut naga::Function, counter: naga::Handle<naga::GlobalVariable>) {
    let mut body = std::mem::take(&mut function.body);
    budget_block(&mut body, &mut function.expressions, counter);
    function.body = body;
}

fn budget_block(block: &mut naga::Block, expressions: &mut naga::Arena<naga::Expression>, counter: naga::Handle<naga::GlobalVariable>) {
    use naga::Statement;
    for statement in block.iter_mut() {
        match statement {
            Statement::Block(inner) => budget_block(inner, expressions, counter),
            Statement::If { accept, reject, .. } => {
                budget_block(accept, expressions, counter);
                budget_block(reject, expressions, counter);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    budget_block(&mut case.body, expressions, counter);
                }
            }
            Statement::Loop { body, continuing, .. } => {
                budget_block(body, expressions, counter);
                budget_block(continuing, expressions, counter);
                body.splice(0..0, spend(expressions, counter, Statement::Break));
            }
            _ => {}
        }
    }
}

/// `if sc_loop_budget == 0u { <out> } sc_loop_budget = sc_loop_budget - 1u;`, where `out` is
/// `break` at the top of a loop and `return` (zero) at the top of a function.
fn spend(expressions: &mut naga::Arena<naga::Expression>, counter: naga::Handle<naga::GlobalVariable>, out: naga::Statement) -> naga::Block {
    use naga::{BinaryOperator, Expression, Literal, Span, Statement};
    let pointer = expressions.append(Expression::GlobalVariable(counter), Span::UNDEFINED);
    let zero = expressions.append(Expression::Literal(Literal::U32(0)), Span::UNDEFINED);
    let one = expressions.append(Expression::Literal(Literal::U32(1)), Span::UNDEFINED);
    let left = expressions.append(Expression::Load { pointer }, Span::UNDEFINED);
    let spent = expressions.append(Expression::Binary { op: BinaryOperator::Equal, left, right: zero }, Span::UNDEFINED);
    let less = expressions.append(Expression::Binary { op: BinaryOperator::Subtract, left, right: one }, Span::UNDEFINED);
    let mut block = naga::Block::new();
    block.push(Statement::Emit(naga::Range::new_from_bounds(left, less)), Span::UNDEFINED);
    block.push(Statement::If { condition: spent, accept: naga::Block::from_vec(vec![out]), reject: naga::Block::new() }, Span::UNDEFINED);
    block.push(Statement::Store { pointer, value: less }, Span::UNDEFINED);
    block
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
