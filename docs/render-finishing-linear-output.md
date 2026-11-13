# Native finishing, backgrounds and linear masters

This is the owner-directed personal render integration branch. The batch ledger records the actual validation state; this document specifies behavior, not a blanket release or complete-roadmap approval.

## Three independent layers

1. **Transport:** visible native geometry, explicit optical surfaces, light emitters and HDR environment determine scene-linear radiance. No display filter changes the path estimator or engineering materials.
2. **Physical stage:** `render.stage.backdrop` saves a finite floor/wall or an explicit 24-strip polygonal cyclorama using the captured subject bounds and saved camera azimuth. These are real nonemissive render-only surfaces. Their source geometry is not inserted into the engineering model. Existing-stage replacement requires explicit consent and the reviewed revision.
3. **Presentation:** optional camera-only gradients and finishing adjust the final PNG. They do not illuminate, appear in reflections, cast shadows or represent terrain. Raw linear masters exclude them.

The cyclorama is a polygonal approximation, not an analytic smooth CAD surface or an infinite background. The layout uses world +Z as up and at least a one-mm stage envelope, without scaling the subject. Geometry, lights, HDRs, camera, surfaces and physical properties remain independent. The eight native studio compositions resolve original authored HDR resources; broader `SCENE-*` specifications remain labelled as specifications rather than silently promoted to executable environments.

## GPU numerical intent

The following additional fields belong to saved `GpuJob` options or the corresponding explicit progressive runtime request:

```json
{
  "linear_exr": true,
  "finishing": {
    "rgb_gain": [1.0, 1.0, 1.0],
    "contrast": 1.0,
    "saturation": 1.0,
    "vignette": 0.0
  },
  "backdrop": {
    "mode": "vertical",
    "first_srgb": [240, 243, 248],
    "second_srgb": [80, 91, 109],
    "center": [0.5, 0.5],
    "radius": 0.75
  }
}
```

Omitted fields preserve neutral finishing, the existing flat background and PNG-only retention. `backdrop: null` keeps the old flat background. Modes are `vertical`, `horizontal` and `radial`; screen coordinates and radial center are normalized. Color interpolation is in linear display light. A gradient and transparent PNG are mutually exclusive; unsupported combinations are errors, not silently ignored.

The processing order is: raw sample average, optional display denoising, exposure and creative linear RGB gains, selected tone map, linear-display contrast/saturation/vignette, alpha/background composition, sRGB encoding. Contrast pivots about 18% gray; saturation uses Rec.709 linear luminance weights; the vignette is an explicitly creative screen-space attenuation, not a calibrated lens. These first finishing controls require a new render and do not retroactively modify an old completed image.

Accepted bounds are RGB gains 0.1..8, contrast 0.25..4, saturation 0..2 and vignette 0..1. Controls reject NaN/infinity and unknown fields. CPU/raster and registered executors that do not implement these operations refuse them instead of dropping them.

## Raw linear EXR companion

`linear_exr: true` retains an additional immutable file **before denoising, exposure, gains, tone mapping, backdrop or display finishing**. It is not a conversion of PNG pixels. RGB remains scene-linear sRGB/D65, with coverage-premultiplied alpha. The values are relative renderer radiance, not calibrated SI photometry. Values above display white are preserved.

The original encoder writes a single-part, uncompressed, increasing-Y OpenEXR scanline image with FLOAT32 A/B/G/R channels, explicit data/display windows, pixel aspect, D65 chromaticities and descriptive metadata. It is deliberately not a half-float, compressed, multipart, deep or arbitrary-AOV implementation. No new runtime dependency is added. The implementation was written from public format documentation, not copied from a third-party codec. Independent validation uses the official OpenEXR reader outside the production dependency graph.

The additional output capacity is admitted before the GPU work starts. Encoding advances through bounded scanline groups with cancellation, and the host hashes both companions in bounded chunks. The original work reservation remains live until publication, then shrinks to the capacities of both immutable files and bounded metadata. A retained frame retains its lease. A cancelled or malformed job does not publish a partial EXR.

Export selects an exact completed job:

```json
{"job":{"session":3,"serial":1,"generation":1},"path":"output.exr","format":"exr","overwrite":false}
```

Export metadata names the actual encoding and alpha convention explicitly. The `frame` field retains canonical job identity and PNG-companion metadata; `linear_master` supplies the raw file length, digest, scene-linear encoding and premultiplied coverage convention. Never compare an EXR file against `frame.png_sha256`.

The default export remains PNG. File suffix and explicit format must agree. Request scope, atomic create/overwrite and existing-file protections are unchanged. An old completed PNG without a raw companion cannot be exported as EXR, even if the current editor has since enabled retention. The asynchronous UI picker captures the original job, document, format and overwrite consent; it cannot silently substitute a newer result.

## Shared UI

The existing shared panel/form/menu/color/checkbox/number-field controls provide presentation filters under Output, camera-only gradients under Environment, and physical backgrounds under Scene. No parallel icon set or private widget library is introduced. The original scene/camera/light controls, saved numeric intent, undo and completed-image selection stay authoritative. The export-format choice is transient destination intent; raw-master retention is saved GPU intent.

## Public references and clean-room boundary

- OpenEXR file layout: https://openexr.com/en/latest/OpenEXRFileLayout.html
- Technical introduction, linear values and premultiplied channels: https://openexr.com/en/latest/TechnicalIntroduction.html
- Official validation-reader API: https://openexr.com/en/latest/python.html

Only the public file-layout and data conventions informed the original encoder. The external OpenEXR reader is a test oracle, not redistributed production code. No external scene imagery, commercial CAD artwork, GPL implementation or measured optical dataset is imported.

## Still separate

Adaptive sample scheduling and improved convergence of difficult transmission paths are not implemented merely by adding EXR or presentation controls. Bloom, volumetric effects, arbitrary AOVs, EXR compression and post-hoc interactive regrading of a completed master remain separate units. None is exposed as an operational control until its producer, admission, persistence and execution tests exist.
