# Plugin shaders: panels (`shaders@0.2`)

A plugin can draw the background of the app's see-through panels: frosted glass, a lens at the
rim, a tinted sheet. It ships one small WGSL function as text. SolidCraft checks the shader when the
plugin is installed and again when it is drawn, wraps it in its own entry points and bindings, caps
how long any pixel may loop, and compiles it for the graphics card. If anything fails, the panels
keep the app's plain blur and the status bar says why.

SolidCraft draws no glass of its own, and a theme file's `glass` key is not read. A glass look is a
plugin that uses this hook.

A panel shader needs no WebAssembly component and runs no code on the CPU. It works the same way
as a view background ([`plugin-shaders.md`](plugin-shaders.md)), and one plugin may ship both.

## Using one

1. Install the plugin's `.zip` from Preferences › Plugins › Install plugin…, or with
   `plugin.install {"path": "frost.zip"}`.
2. Turn it on under its plugin's card in Preferences › Plugins: each panel shader the plugin
   offers has a switch there. One panel shader is on at a time, so turning one on turns any other
   off. All are off by default.
3. Its settings are sliders on the same card.

The choice is the `panel_shader` preference, `"<plugin-id>/<shader-id>"`, so a script or agent
sets it with `ui.prefs {"panel_shader": "frost/frost"}`. `""` (the default) is none. The sliders
are stored in the `shader_settings` preference that every plugin shader shares, as
`{"<plugin-id>/<shader-id>/<setting-id>": value}`; a setting that is missing uses its default. The card's switch and sliders are the handles
`prefs:panel_shader:<key>`. Turning the plugin off in Preferences › Plugins, or
removing it, stops it being drawn.

## Which panels get it

Only panels with a see-through fill: the theme's panel colours must have alpha below 1 (most themes
are opaque, and their panels are drawn as before). The shader draws
each such panel's background, a bar, a docked panel, a window or dialog, one panel at a time.
The dimmed backing behind a modal dialog covers the whole window, so it keeps the plain blur.

The app then draws the panel's own theme fill over what the shader returned, and the panel's text
and controls over that. So a fill with little alpha shows most of the shader, and a nearly opaque
fill hides it.

It is drawn only where the app's panel blur can run: on the graphics card, not in safe-graphics
mode, and not when the operating system asks to reduce transparency. Otherwise the panels are
drawn plain. It is desktop only for now, as plugins are.

## Worked example: Frost

[`plugin-examples/frost/`](plugin-examples/frost/) holds two files.

`plugin.json`:

```json
{
  "id": "frost",
  "name": "Frost",
  "version": "1.0.0",
  "description": "Frosted glass behind see-through panels, tinted towards the theme's text colour, with a light rim.",
  "capabilities": ["shaders@0.2"],
  "shaders": [
    {
      "id": "frost",
      "name": "Frost",
      "kind": "panel",
      "source": "frost.wgsl",
      "blur": 16,
      "settings": [
        {"id": "tint", "name": "Tint", "min": 0, "max": 1, "default": 0.2}
      ]
    }
  ]
}
```

`frost.wgsl`:

```wgsl
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
```

Zip the two files (at the top of the `.zip`, or in one folder) and install the `.zip`. Turn on
Frost on its card, pick a theme with see-through panels, and move the Tint slider.

A fuller shader can bend the backdrop like a lens near the rim, using `px.normal` and `px.edge` to
sample a few pixels past the edge, and frost more towards the rim.

## The manifest

`capabilities` must list `"shaders@0.2"`. Each panel shader is an entry of `shaders`:

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Lowercase letters, digits and `-`, unique in the plugin. The preference names it `<plugin-id>/<id>`. |
| `name` | yes | The switch's label on the card, 1 to 80 characters. |
| `kind` | yes | `"panel"`. |
| `source` | yes | A `.wgsl` file name in the plugin's folder (lowercase, no folders). |
| `blur` | no | The blur radius of `sc_blurred`, 1 to 60 pixels. Default 12. |
| `settings` | no | Up to 4 settings, shown as sliders on the card: `{"id", "name", "min", "max", "default"}`. `id` is lowercase letters, digits and `-`, unique in the shader; `name` is 1 to 40 characters; `min` < `max`, and `default` lies between them. |
| `textures` | no | Up to 4 pictures: `{"name": "<wgsl name>", "file": "<file>.png"}` or `.jpg`. |
| `animated` | no | `true`: redrawn about 30 times a second while shown, with `sc.time` running. `false` (the default): `sc.time` is 0. |

A plugin declares at most 8 shaders in all. A plugin with shaders and no `runtime` is fine.

## What a shader can use

Write one function:

```wgsl
fn panel(px: ScPanel) -> vec4<f32>
```

It returns a colour with straight (not premultiplied) alpha. The host clamps it to 0..1, masks it to
the panel's rounded rectangle with a smooth edge, premultiplies it and encodes it for the screen.
Alpha below 1 lets what is behind the panel show through, sharp.

**Colours are linear.** `sc.accent`, `sc.text`, `sc_backdrop`, `sc_blurred` and every texture
sample are linear light, and the colour you return is linear too. To do maths on screen values,
raise to `1.0 / 2.2` first and to `2.2` at the end.

`px: ScPanel`, the pixel being drawn:

| Field | Type | Meaning |
|---|---|---|
| `position` | `vec2<f32>` | Pixels from the window's top left. `sc_backdrop` and `sc_blurred` take this. |
| `local` | `vec2<f32>` | Pixels from the panel's top left. |
| `size` | `vec2<f32>` | The panel's size in pixels. |
| `uv` | `vec2<f32>` | 0..1 across the panel. |
| `normal` | `vec2<f32>` | The outward normal of the panel's rounded edge nearest this pixel, unit length, y down. |
| `radius` | `f32` | The corner radius nearest this pixel, in pixels. |
| `edge` | `f32` | The distance to the panel's edge in pixels: 0 on it, negative inside. |
| `hovered` | `f32` | 1 while the pointer is over the panel, else 0. |
| `focused` | `f32` | 1 while the keyboard focus is in the panel, else 0. |
| `floating` | `f32` | 1 for a window or dialog over the others, 0 for a docked panel or bar. |
| `blur` | `f32` | The blur radius of `sc_blurred`, in pixels. |

`sc: ScHost`, the same uniforms backgrounds and tints get ([plugin shaders](plugin-shaders.md)); a panel uses these:

| Field | Type | Meaning |
|---|---|---|
| `size` | `vec2<f32>` | The window's size in pixels. |
| `time` | `f32` | Seconds, for an animated shader (comes back to 0 each hour); 0 otherwise. |
| `scale` | `f32` | Pixels per point (the display's scaling). Multiply lengths in points by it. |
| `accent`, `text` | `vec4<f32>` | The theme's accent and text colours, alpha 1. |
| `dark` | `f32` | 1 for a dark theme, 0 for a light one. |
| `settings` | `vec4<f32>` | The card's settings in the manifest's order: `sc.settings.x` is the first, then `.y`, `.z`, `.w` (`sc.settings[i]` for setting `i`). Unused ones are 0. |

Functions:

- `sc_backdrop(p: vec2<f32>) -> vec4<f32>`: what is behind the panels at window pixel `p`, sharp
  and linear: the 3D view and everything drawn before the panel.
- `sc_blurred(p: vec2<f32>) -> vec4<f32>`: the same, blurred by `px.blur` pixels. It is valid
  within about two blur radii of the panel; further out it may show the plain backdrop or nothing.

Samplers: `sc_clamp` (clamps at the edges) and `sc_repeat` (wraps), both linear with mipmaps.
Textures: each `textures` entry is a `texture_2d<f32>` by its `name`, bound from binding 3 on in
manifest order (bindings 7 to 9 are the host's panel data).

Sample anywhere in the window, not only under the panel: a lens at the rim can show what lies a few
tens of pixels beyond the edge. Samples outside the window clamp to its edge.

Constants, helper functions, structs and `var<private>` variables of your own are fine. Names
starting with `sc` and `Sc` belong to the host.

## Rules and limits

The host refuses a shader, at install and at draw time, that:

- is over 64 KiB, or is not UTF-8 WGSL that parses and validates;
- has no `fn panel(px: ScPanel) -> vec4<f32>`;
- declares an entry point (`@vertex`, `@fragment`, `@compute`): the host supplies them;
- declares a binding or a `uniform`, `storage` or `workgroup` variable: it reads only what the
  host binds and its own textures;
- declares an `override` (use a `const`), or needs a WGSL extension (`enable`);
- declares a function whose name starts with `sc_`: those names belong to the host.

It also refuses a manifest whose `blur` is outside 1 to 60, with more than 4 `settings`, or with a
setting whose id repeats or whose `default` is outside `min` to `max`.

Errors name the file and the line, for example
`invalid manifest field shaders.frost: frost.wgsl: line 4: ...`.

**Loops and calls.** Every pixel gets a budget of 1024, spent by each loop iteration and each call
of a function of your own, nested or not. Calls to the host's `sc_backdrop` and `sc_blurred` do not
spend it. A loop that reaches the cap ends as if its condition had failed, and a call returns zero.
Keep loops small and fixed (`for (var i = 0; i < 8; i++)`): a panel covers many pixels, and every
sample costs time on each one. A few dozen samples a pixel is plenty.

**Sampling.** Use `textureSampleLevel`, not `textureSample`. `textureSample` needs every
neighbouring pixel to take the same path, which WGSL's uniformity rules check and refuse in loops
and branches.

**When it fails.** A shader that this graphics card will not compile, a picture that does not
decode, or a plugin that is removed or turned off leaves the panels with the app's plain blur,
writes one line in the status bar and is not tried again until the choice changes or the app
restarts. Nothing is drawn halfway.

## Testing a shader

- `plugin.install {"path": "frost.zip", "dry_run": true}` checks the package and compiles the
  shader without installing it; a refusal names the line.
- A test copy of SolidCraft started with `SOLIDCRAFT_SETTINGS_DIR` and `SOLIDCRAFT_RECOVERY_DIR`
  set to scratch folders tries a panel shader without touching your own preferences.
- `naga` or `wgsl-analyzer` catch most mistakes before you zip, if you paste the declarations from
  [`crates/craft-plugin/src/shader.rs`](../crates/craft-plugin/src/shader.rs) (`PANEL_PRELUDE`)
  after your source.

## Versions

`shaders@0.2` is a superset of `shaders@0.1`: backgrounds work the same under it, and 0.1
plugins keep working unchanged. While it is 0.x, a later minor version may change it, and SolidCraft then asks you to update the plugin rather than drawing it wrongly.
