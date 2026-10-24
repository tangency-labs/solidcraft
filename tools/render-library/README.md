# Built-in render resource generation and native probes

All tools here are offline contributor tools, not application runtime dependencies.
They use original resources already attributed in the starter pack. Source
manifests and their authoring-readiness flags are never rewritten.

## Fixed built-in lookup

```sh
python tools/render-library/generate.py --check
```

Without `--check`, the generator writes `library_data.rs`. It verifies all 192
runtime eight-bit maps and 24 HDR files against their published encoded hashes.
The 48 sixteen-bit height maps remain authoring-only and are not silently decoded
as eight-bit color.

## Focused hardware GPU probe

```sh
python tools/render-library/run_native_smoke.py --out /new/output/directory --target-dir /existing/cargo/target
```

Requires a clean committed source and a Vulkan hardware adapter. The runner builds
only normal geom/render/gpu libraries, then links `native_smoke.rs` to the exact
Cargo-reported artifacts. It does not build the CAD kernel, engine, desktop UI or
all test binaries. The GPU must satisfy the explicit hardware policy; no software
or CPU fallback is allowed.

The original coupon is rendered three ways: four authored PBR maps with absolute
base color, legacy color modulation, and a scalar-control request using the
captured roughness/metallic values. The probe checks actual executor identity,
frame hashes, dimensions, PNG CRCs and bounded scanlines, distinguishable control
images and released reservations. The renderer process must log one non-software
adapter. Original SURF-001/HDR-001 source hashes and source-tree stability are
checked separately. Inspect the resulting PNGs before recording visual acceptance.

The output directory is new and caller-selected. Receipts/logs/images are not
committed. A passing probe is native renderer evidence for this original scene,
not GUI acceptance, a full-engine command test, measured material fidelity or a
performance benchmark.

## Bounded CPU scale probes

```sh
cargo build --locked --profile ci -p solidcraft-render --features raytrace-cpu --example cpu_resource_smoke
# Select one case per fresh output folder:
cpu_resource_smoke /new/output/directory 4k
cpu_resource_smoke /another/output/directory 8k
cpu_resource_smoke /another/output/directory dense
cpu_resource_smoke /another/output/directory bodies
```

`4k` renders an original analytic plate at 3840x2160 under Studio; `8k` renders it
at 7680x4320 under Workstation. `dense` renders a 500,000-triangle planar grid at
256x256; `bodies` renders 20,000 separated tiles at 512x256. These are deliberately
simple synthetic cases, not representative CAD assemblies or speed benchmarks.

Every case verifies that Compact refuses the oversized request, the selected
profile completes a real image containing geometry, and retained host reservations
return to zero. A separately identified 20,000-body admission-only control is not
confused with the rendered case. The job wait is bounded at 300 seconds; failure
or timeout is reported, never silently retried at reduced quality.


## Full-detail functional SELENE studio

`selene_studio.py` starts the explicitly selected native CLI with `mcp --render-gpu`, loads the pinned functional master once, restricts file writes to a new output directory, and retains request/preflight/export receipts. The immutable master hash and CLI binary hash are checked separately. It is an authoring workflow, not an automatic background service.

```python
# Start in this directory with an explicitly built CLI and an unused output path:
# python -i selene_studio.py --cli /path/to/solidcraft-cli --out /new/render/output
from selene_recipe import hero_request
request = hero_request(studio, width=3840, height=2400, samples=1024)
receipt = studio.render("selene-studio-4k", request, timeout=7200)
studio.close()
```

The original recipe fits all captured body bounding boxes inside an 8% image margin. It resolves six authored material families through public commands, preserves the native paint/PCB/rubber palette where specified, selects twelve explicitly certified closed dielectric bodies in this master, and appends a passive ground rectangle only to the captured scene. The authored three-area-light rig uses an explicit 1/16 creative radiance gain plus a low-intensity HDR fill. It is not the unmodified reference calibration, measured glass data, geometric displacement, LED volumetric emission, or a physics reassignment. Native pose and visibility stay unchanged.

`python -B test_selene_recipe.py` checks camera projection, scale covariance, input rejection and source-data nonmutation without a renderer build. A successful image export is not visual acceptance: inspect complete framing, fine detail, highlights, glass and noise before publishing an image. Keep provisional images and logs outside the repository; a deliberately selected final showcase export is published with its pinned recipe and attribution.


### Publishing the selected showcase export

After the complete PNG and unscaled detail crops have actually been inspected, `publish_selene.py --session OUTPUT_DIRECTORY --accept-visual-review` checks the completed native receipt, unchanged master, exact requested execution controls, captured geometry cardinality, original resource hashes, PNG dimensions/CRCs/bounded scanlines and final-frame SHA-256. It refuses an existing showcase destination and never modifies or resamples the pixels. The deliberately shipped output is `assets/render/selene/exports/SELENE-studio-4k.png`, paired with `scenes/studio-hero.json`; it is consumed by the model's `RENDER.md` showcase, not a general review-file inbox. Git publication remains a separate explicit operation.

`python -B test_publish_selene.py` exercises malformed/truncated/wrong-size/expanding PNG refusals and explicit-review consent before any filesystem reads or writes. No renderer build is needed for these tests.


The read-only `SELENE showcase package` workflow runs on selected integration-branch resource changes. It invokes `package_selene.py`, verifies the committed PNG/master/resource hashes and bounded PNG stream, and uploads only the native PNG, manifest, README and SHA-256 checksums. It does not rerender, build the application or install dependencies. Artifact success is delivery evidence only. Existing output directories are refused by the package tool.
