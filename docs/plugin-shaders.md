# Plugin shaders: view backgrounds and tints (`shaders@0.2`)

A plugin can draw the background behind the model in the 3D view: a sky, a picture that turns with
the camera, a pattern that moves. It ships a small WGSL fragment shader as text. SolidCraft checks the
shader when the plugin is installed and again when it is drawn, wraps it in its own entry points
and bindings, caps how long any pixel may loop, and compiles it for the graphics card. If anything
fails, the theme's own background stays and the status bar says why.

A shader plugin needs no WebAssembly component and runs no code on the CPU. It can also live in
the same plugin as commands (`commands@0.1`). [RFC 0015, amendment 1](rfcs/0015-plugin-ui-workspaces.md#amendment-1-shaders)
gives the design and the reasons for it.

## Using one

1. Install the plugin's `.zip` from Preferences › Plugins › Install plugin…, or with
   `plugin.install {"path": "dusk.zip"}`.
2. Turn it on under its plugin's card in Preferences › Plugins: each background the plugin offers
   has a switch there. One background is on at a time, so turning one on turns any other off.

The choice is the `background_shader` preference, `"<plugin-id>/<shader-id>"`, so a script or agent
sets it with `ui.prefs {"background_shader": "dusk/dusk"}`. `""` (the default) is none. Turning
the plugin off in Preferences › Plugins, or removing it, stops it being drawn.

The background is drawn over the theme's background and under the model, the grid and everything
else, like the Background picture preference. It is desktop only for now, as plugins are.

## Worked example 1: Dusk, a sky in the theme's colours

[`plugin-examples/dusk/`](plugin-examples/dusk/) holds two files.

`plugin.json`:

```json
{
  "id": "dusk",
  "name": "Dusk",
  "version": "1.0.0",
  "description": "A sky in the theme's colours behind the model, with a glow drifting along the horizon.",
  "capabilities": ["shaders@0.1"],
  "shaders": [
    {
      "id": "dusk",
      "name": "Dusk",
      "kind": "background",
      "source": "dusk.wgsl",
      "animated": true
    }
  ]
}
```

`dusk.wgsl`:

```wgsl
fn background(px: ScPixel) -> vec4<f32> {
    // How far above the horizon this pixel looks: -1 straight down, 1 straight up.
    let height = px.ray.z;
    let sky = mix(sc.bottom.rgb, sc.top.rgb, smoothstep(-0.2, 0.6, height));
    // A band of the theme's accent just above the horizon, moving round it once a minute.
    let heading = atan2(px.ray.y, px.ray.x);
    let drift = 0.5 + 0.5 * cos(heading - sc.time * 6.2831853 / 60.0);
    let offset = (height - 0.05) / 0.08;
    let band = exp(-offset * offset) * mix(0.15, 0.4, drift);
    return vec4<f32>(mix(sky, sc.accent.rgb, band), 1.0);
}
```

Zip the two files (at the top of the `.zip`, or in one folder) and install the `.zip`. The sky
uses the theme's view colours and accent, so it suits light and dark themes alike, and the horizon
tilts as you orbit because it follows the world's Z axis.

## Worked example 2: Panorama, a picture that turns with the view

[`plugin-examples/panorama/`](plugin-examples/panorama/) adds a texture. Put a 360° picture
(equirectangular, twice as wide as tall, horizon across the middle) beside the files as
`panorama.jpg`.

```json
"shaders": [
  {
    "id": "panorama",
    "name": "Panorama",
    "kind": "background",
    "source": "panorama.wgsl",
    "textures": [{"name": "picture", "file": "panorama.jpg"}]
  }
]
```

```wgsl
const TAU: f32 = 6.2831853;
const PI: f32 = 3.1415927;

fn background(px: ScPixel) -> vec4<f32> {
    let ray = px.ray;
    let uv = vec2<f32>(atan2(ray.y, ray.x) / TAU + 0.5, 0.5 - asin(clamp(ray.z, -1.0, 1.0)) / PI);
    // Pick the mip level by hand: where u wraps from 1 back to 0 the GPU's own choice is a tiny
    // level and draws a seam.
    let texels = f32(textureDimensions(picture, 0).y);
    let view = 2.0 * atan(length(sc.up.xyz));
    let level = max(log2(texels / PI * view / max(sc.size.y, 1.0)), 0.0);
    return vec4<f32>(textureSampleLevel(picture, sc_repeat, uv, level).rgb, 1.0);
}
```

The texture's `name` is the name the shader uses. The host loads the picture, scales it down to at
most 8192 pixels on a side (or the graphics card's limit), and builds its mipmaps.

## The manifest

`capabilities` must list `"shaders@0.2"` (or `"shaders@0.1"`, for backgrounds with no settings,
as before). Each entry of `shaders`:

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Lowercase letters, digits and `-`, unique in the plugin. The preference names it `<plugin-id>/<id>`. |
| `name` | yes | What people pick it by, 1 to 80 characters. |
| `kind` | yes | `"background"`, or `"tint"` ([Tints](#tints), 0.2). |
| `source` | yes | A `.wgsl` file name in the plugin's folder (lowercase, no folders). |
| `textures` | no | Up to 4 pictures: `{"name": "<wgsl name>", "file": "<file>.png"}` or `.jpg`. |
| `animated` | no | `true`: redrawn about 30 times a second while shown (10 behind another window), with `sc.time` running. `false` (the default): drawn again only when the view turns or resizes or the theme changes; `sc.time` is 0. |
| `settings` | no | 0.2: up to 4 sliders on the plugin's card, `{"id", "name", "min", "max", "default"}`; the shader reads their values, in order, as `sc.settings.x` to `.w`. |

A plugin declares at most 8 shaders. A plugin with shaders and no `runtime` is fine.

## What a shader can use

Write one function:

```wgsl
fn background(px: ScPixel) -> vec4<f32>
```

It returns a colour with alpha. The host clamps it to 0..1 and blends it over the theme's
background on the screen's values, so alpha below 1 lets the theme show through as it does in any
picture editor.

**Colours are linear.** The theme colours in `sc` and every texture sample are linear light, not the
sRGB numbers you see in a colour picker, and the colour you return is linear too. The host converts
for the screen. To do maths on screen values, raise to `1.0 / 2.2` first and to `2.2` at the end.

`px: ScPixel`, the pixel being drawn:

| Field | Type | Meaning |
|---|---|---|
| `uv` | `vec2<f32>` | 0..1 across the view, from its top left. |
| `position` | `vec2<f32>` | Pixels from the view's top left. |
| `ray` | `vec3<f32>` | The direction this pixel looks along in world space (Z up), unit length. It turns with the camera and ignores zoom and pan. |

`sc: ScHost`, the host's uniforms:

| Field | Type | Meaning |
|---|---|---|
| `size` | `vec2<f32>` | The view's size in pixels. |
| `time` | `f32` | Seconds, for an animated shader (comes back to 0 each hour); 0 otherwise. |
| `scale` | `f32` | Pixels per point (the display's scaling). |
| `right`, `up`, `forward` | `vec4<f32>` | The camera's axes in world space (`.xyz`). `right` and `up` are scaled to half the view's width and height at distance 1; `px.ray` is `normalize(forward + x * right + y * up)`. |
| `top`, `bottom` | `vec4<f32>` | The theme's view background at its top and bottom. |
| `accent`, `text` | `vec4<f32>` | The theme's accent and text colours. |
| `dark` | `f32` | 1 for a dark view background, 0 for a light one. |
| `settings` | `vec4<f32>` | 0.2: the values of the shader's `settings` sliders, in order; 0 past the last. |

Samplers: `sc_clamp` (clamps at the edges) and `sc_repeat` (wraps), both linear with mipmaps.
Textures: each `textures` entry is a `texture_2d<f32>` by its `name`.

Constants, helper functions, structs and `var<private>` variables of your own are fine. Names
starting with `sc` and `Sc` belong to the host.

## Rules and limits

The host refuses a shader, at install and at draw time, that:

- is over 64 KiB, or is not UTF-8 WGSL that parses and validates;
- has no `fn background`;
- declares an entry point (`@vertex`, `@fragment`, `@compute`): the host supplies them;
- declares a binding or a `uniform`, `storage` or `workgroup` variable: it reads only `sc`, the two
  samplers and its own textures;
- declares an `override`, or needs a WGSL extension (`enable`);
- declares a function whose name starts with `sc_`: those names belong to the host.

Errors name the file and the line, for example
`invalid manifest field shaders.dusk: dusk.wgsl: line 3: ...`.

**Loops and calls.** Every pixel gets a budget of 1024, spent by each loop iteration and each
call of a function of your own, nested or not. A loop that reaches the cap ends as if its
condition had failed, and a call returns zero. Write loops with a fixed count that fits
(`for (var i = 0; i < 16; i++)`) and keep helper calls out of deep loops; an ordinary shader
spends well under 100. The cap bounds the work per pixel; it cannot promise that a slow
graphics card never resets. If the graphics device is lost while a plugin background is drawn,
SolidCraft turns the background off before it restarts and says so when it starts again.

**Sampling in a loop.** Use `textureSampleLevel` inside loops and in code that only some pixels
run. `textureSample` needs every neighbouring pixel to take the same path, which WGSL's uniformity
rules check and refuse there.

**Pictures.** `.png` or `.jpg`, at most 16 MiB each, 4 per shader; scaled down to 8192 pixels on a
side.

**When it fails.** A shader that this graphics card will not compile, a picture that does not
decode, or a plugin that is removed or turned off leaves the theme's background, writes one line in
the status bar ("Couldn't draw the background effect dusk/dusk: ...") and is not tried again until
the choice changes, the app restarts, or (when it was turned off) its plugin is turned on again.
Nothing is drawn halfway.

## Testing a shader

- `plugin.install {"path": "dusk.zip", "dry_run": true}` checks the package and compiles the
  shader without installing it; a refusal names the line.
- A test copy of SolidCraft started with `SOLIDCRAFT_SETTINGS_DIR` and `SOLIDCRAFT_RECOVERY_DIR`
  set to scratch folders tries a background without touching your own preferences.
- `wgsl-analyzer` or `naga` on the command line catch most mistakes before you zip; they will not
  know `ScPixel` and `sc` unless you paste the declarations from
  [`crates/craft-plugin/src/shader.rs`](../crates/craft-plugin/src/shader.rs) (`PRELUDE`) after
  your source.

## Tints

A shader of kind `tint` colours the ribbon's command icons, its button blocks and the panels each
frame. The host draws nothing of its own: with no tint switched on (`tint_shader`, empty by
default), they look as they always do and no shader runs. Each tint has a **Tint** switch on its
plugin's card; one tint is on at a time, and only while its plugin is.

```wgsl
fn tint(r: ScRegion) -> vec4<f32>
```

The host asks about every region it draws and calls `tint` once for each, in a compute shader (so
sample textures with `textureSampleLevel`):

| `ScRegion` field | Type | Meaning |
|---|---|---|
| `kind` | `u32` | `SC_ICON`, `SC_BUTTON` or `SC_PANEL`. |
| `rect` | `vec4<f32>` | Left, top, right, bottom, in points from the window's top left. |
| `position` | `vec2<f32>` | The point asked about: an icon's or button's centre, or a point on a panel's grid (at most 48 points apart, its corners included). |
| `base` | `vec4<f32>` | The region's own colour, linear: the theme's accent for an icon, the fill for a button or panel. |
| `enabled` | `f32` | 1 for an enabled control, less for a disabled one. |

`sc` is the same `ScHost`, with `size` the window's size in pixels and no camera (`right`, `up`
and `forward` are 0).

It returns a linear colour and how strongly it shows, alpha 0 to 1. Alpha 0 leaves the region as
it is. An **icon**'s coloured parts take the colour at that strength, keeping their shading, and its
whites and greys stay as they are; a **button** gets the colour filled over its block; a **panel**
gets it laid over its fill, blended smoothly between the points asked about, inside its corners.

The answers are read back for a later frame, so a tint follows a region a frame late; a region not
answered yet (just opened or scrolled) takes the nearest answer of its kind. At most 8192 regions
are asked about in a frame. An animated tint runs about 30 times a second; a still one runs when
what it colours moves or changes.

[`plugin-examples/accent-sweep`](plugin-examples/accent-sweep) sweeps a band of the theme's accent
across the icons and buttons, with its width and speed as settings.

## Panels

`"kind": "panel"` (`shaders@0.2`) draws the background of every see-through panel from what is
behind it: the 3D view and the panels under it, sharp and blurred. The app draws no look of its own
there, and with none chosen the panels draw as before. See [plugin panels](plugin-panels.md).

## Versions

`shaders@0.2` adds tints and settings; `shaders@0.1` plugins install and draw as before.
`shaders@0.1` is version 0.1 of the capability. While it is 0.x, a later minor version may change
it, and SolidCraft then asks you to update the plugin rather than drawing it wrongly. The amendment
in RFC 0015 lists what is planned next (glass panel materials, a previous-frame texture for effects
with memory).
