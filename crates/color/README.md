# solidcraft-color

Colour rules and data that any crate may share. It has no dependencies and holds no app types,
so the renderer, the document, the importers and the UI theme all use the same rule and the same
colours instead of keeping copies.

The sRGB transfer function (IEC 61966-2-1) lives here once: every encode and decode in Rust goes
through `srgb`, so a change to the rule is one change. Shaders can't call Rust and keep their own
copies.

## API

| Item | What it is |
|---|---|
| `srgb::encode(linear)` | Linear light (0..1) to an sRGB value (0..1) |
| `srgb::decode(encoded)` | An sRGB value (0..1) to linear light |
| `srgb::encode8(linear)` | Linear light to the nearest 8-bit sRGB code (out of range clamps, NaN reads as 0) |
| `srgb::decode8(code)` | An 8-bit sRGB code to linear light |
| `AnalysisColors` | The surface analysis palette (draft, accessibility, curvature, hatch), straight sRGB 0..1 as the shader takes it; the theme fills it and the renderer reads it |
| `AnalysisColors::DEFAULT` | The light theme's colours with no colour vision mode, as headless pictures use |
