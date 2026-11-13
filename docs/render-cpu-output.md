# CPU output presentation and native masters

CPU ray tracing can apply the same presentation operators as GPU path tracing to
its captured transport result. Presentation is independent of the optional CPU
physical lens, guided noise reduction and opaque path lighting; those options
keep their own admission and compatibility checks. CPU adaptive sampling and
conditional Path Glass/subsurface transport also use this output pipeline. Their
compatibility limits remain independent; see [renderer parity](render-parity.md).
Raw EXR is unfiltered film data, including when the displayed PNG is denoised.

A portable CPU job may include:

```json
{
  "width": 512, "height": 512, "exposure_ev": 1,
  "job": { "method": "cpu_raytrace", "options": {
    "progressive": { "samples_per_pixel": 16, "seed": 81 },
    "output": {
      "tone_map": "aces_fitted",
      "finishing": { "rgb_gain": [1.2, 0.9, 1],
        "bloom": { "strength": 0.2, "threshold": 1.5, "radius": 8 },
        "photographic": { "grain": 0.02, "seed": 42 }
      },
      "linear_exr": true
    }
  }}
}
```

The runtime equivalent is `raytrace.output`. Output never owns another exposure,
background or transparency value. Setup exposure still lowers once into CPU
raytrace exposure; the captured image request owns the display background and
transparent-background flag. Opaque remains the default; explicit true exports
straight-alpha geometric coverage. Unknown or GPU transport fields inside output
are refused.

With native resolution and denoising disabled, missing/null output and identity
presentation keep the original f64 PNG exactly. EXR-only retention also preserves
that display path. Nonidentity presentation
converts one complete pass to FLOAT32, premultiplied by primary coverage, before
shared Display/Bloom/Photographic stages. Fixed CPU captures raw radiance before
its existing per-pixel display write; progressive CPU converts the complete f64
film mean. Every provisional pass has the same effects and deterministic seeds
as a standalone final target at that pass count.

A raw master is created only for final completion, before exposure, tone mapping,
finishing, bloom, photographic effects and the display backdrop. RGB is relative
scene-linear sRGB/D65 and coverage-premultiplied; A is primary coverage. The CPU
producer has its own exact native software metadata profile. Regrading accepts
only the exact retained native CPU/GPU profiles; it does not import arbitrary EXR.

The shared final output envelope is 8192 pixels per side and 33,554,432 pixels in
total. A larger CPU profile does not raise that separate envelope. Active output
is validated before HDR IO or scene capture, and offline saved setups validate it
as well. X2 reconstructs the internal trace grid to these selected final dimensions;
see [quality controls](render-quality-controls.md) for internal-grid admission.
Native CPU images without these output stages retain their existing profile bounds.

Admission adds one 16-byte-per-pixel conversion, bounded bloom scratch (24 bytes
per pixel), optional photographic scratch (8 bytes per pixel plus 16 KiB), final
EXR capacity and fixed stage/options metadata. The existing RGBA/PNG reservation
is moved into the output stage, not duplicated. Conversion, each bounded output
operation and final digest scans debit the same cumulative CPU work budget.
Cancellation cannot install a late PNG or raw master.

Raw masters, final PNGs and at most two provisional PNG slots use the existing
host pool. A final has its own lease and cannot borrow a pinned preview slot.
Externally retained final/preview Arcs remain charged until their last owner is
dropped. A cancelled job leaves the previous completed image intact.

The Render workspace saves and reloads CPU presentation through the shared
Effects, Environment and Output editors. EXR export is enabled only by an actual
selected completed image with a retained master. Enabling retention for a future
job or showing a provisional PNG cannot manufacture an exportable EXR.

## Focused validation

Executed renderer regressions cover the original f64 PNG identity, analytic raw
radiance and coverage, every complete progressive prefix, worker quantum
independence, each output stage's cancellation and cumulative work ceiling, and
retained preview/final leases. Earlier saved-document and engine runs exercised
strict round trips, cold output refusal before HDR IO, actual CPU rendering,
raw-master export and regrading; they do not qualify later combined additions.

An earlier-snapshot Render workspace suite passed with the CPU feature enabled
and a real GPU required. It included the real Render button's saved CPU effects
and exported EXR, presentation Undo/reload, camera preservation, progressive
controls, live selection overlays, cancellation and clean cold-entry shot
switching. That earlier result does not qualify the later combined alpha,
denoising, GI or saved-optics additions, whose document/engine/UI/native workflow
gates remain pending. Executed combined renderer checks do not replace
repository-wide or platform CI.


## Transparent PNG coverage

CPU runtime requests accept the existing top-level `transparent_background` flag.
Saved CPU jobs keep it beside `options`, with false as the default, like the other
image methods. Omission and false retain the original opaque f64 PNG and work
route. The Output checkbox and Environment Clear action use this same flag;
there is no second alpha setting inside `options` or `output`.

True exports straight RGB and 8-bit alpha from primary geometric coverage for
fixed 1/4/16 AA and progressive camera sampling. The scene radiance is divided by
coverage before exposure/tone/colour adjustments, never multiplied by alpha in
the PNG. Alpha is rounded geometric coverage; fully clear pixels have canonical
zero RGB. Background colour is not composited into a transparent PNG. Body and
face opacity must remain 100%: this is background removal/edge coverage, not
partial material opacity or a general transmissive-glass matte. A primary glass
hit retains its existing geometry footprint.

The shared colour presentation is available with alpha. Active camera backdrops,
bloom and photographic operators still require opaque output and refuse without
removing authored values. Raw EXR always retains the same premultiplied linear
radiance and coverage, byte for byte, independent of PNG background choice.
No additional image plane, ray, reservation envelope or work limit is introduced.
Only the completed final PNG is exportable; previews remain nonexportable and
cancellation retains the existing publication barrier. Export writes the exact
retained straight-alpha PNG bytes.

For explicit CPU independent-pilot allocation, raw linear means and coverage use
each pixel's production sample count. Completed-pass presentation borrows the
film's exact count plane for integer PNG alpha rounding; it does not copy a new
image-sized count plane or write pilot radiance to the final raw EXR. Captured
sampling metadata remains in the existing job report rather than a second,
conflicting linear-master schema. This outside integration requires compilation
and numerical/export qualification before advertising the CPU adaptive route.
