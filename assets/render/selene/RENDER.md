# SELENE native rendering and reproducible studio showcase

## Full-detail 4K path-traced studio image (2026-10-22)

![SELENE native studio render](exports/SELENE-studio-4k.png)

The selected [3840 x 2400 PNG](exports/SELENE-studio-4k.png) is a completed native SolidCraft GPU-path render of the latest functional master, not a Design screenshot, a generated illustration, a painted geometry replacement or an upscaled preview. The exact request, original resource hashes, editable source and executable identities are retained in [studio-hero.json](scenes/studio-hero.json).

| Property | Verified value |
|---|---|
| Editable authority | `models/SELENE-functional.solidcraft`, original bytes unchanged |
| Functional source commit | `cf8e651022deee528b26c303fde58bf3eb8a22a5` |
| Native model SHA-256 | `3bd2bfa1c6336394c4a4208d30b52c80d06658a3bd7ea57cc93b774411d1bd68` |
| Evaluated native bodies | 2,116; zero timeline errors or warnings |
| Original visible subject | 2,094 bodies; 2,144,099 triangles |
| Separate render-only stage | One passive ground rectangle; 2 triangles |
| Optical bodies | 12 explicitly selected closed dielectric bodies; topology and medium-overlap checks retained |
| Authored surface detail | 48 exact body bindings across six original surface families |
| Output | 3840 x 2400, RGBA8 PNG, sRGB |
| Sampling | 1,024 samples per pixel; maximum path depth 16; seed 21026 |
| Presentation | ACES-fitted tone mapping and edge-aware a-trous denoising |
| Lighting | Original RIG-002 rectangular emitters, explicit 1/16 creative radiance gain, original HDR-002 fill at intensity 0.12 |
| Camera | Perspective fit to all captured body bounding boxes; 8% frame margin |
| Native executable source | `895704a2fdc96dabb9a2afb2e548a8896e4b1e79` |
| Native executable SHA-256 | `3421d538c571647c1f24fd74d63487c4ba4ab597980cfeea111a01c0db94e699` |
| Completed PNG SHA-256 | `40a492e7979137d0f4027300da3d8af7ca1008c1aab2c309259e65d309a17316` |
| PNG length | 2,388,114 bytes; chunk CRCs, bounded scanlines and final-frame hash verified |

The image uses the original saved visibility and neutral pose. There is no mesh simplification, geometry removal, hidden resolution downgrade, engineering-material reassignment or software-device fallback. The native job's logical admission estimate was 2,814,770,732 bytes; this is not a peak-RSS or GPU-memory measurement. Hardware-only policy and Vulkan were enforced on Pegasus; the hardware survey identifies its AMD Radeon 8060S.

### Reproduction

Build the native CLI once, then reuse the loaded model and device:

```sh
cargo build --locked --profile ci -p solidcraft-cli --bin solidcraft-cli
python -i tools/render-library/selene_studio.py --cli /path/to/solidcraft-cli   --out /new/output/directory
```

In the resulting interactive studio:

```python
from selene_recipe import hero_request
request = hero_request(studio, width=3840, height=2400, samples=1024)
receipt = studio.render("selene-studio-4k", request, timeout=7200)
studio.close()
```

The helper checks the pinned master and binary, public-command preparation, resource hashes, actual executor, exact requested dimensions and exported frame bytes. The frame is not automatically declared visually accepted. The original camera-fit/publication tools and seven independent Python tests are under `tools/render-library/`; provisional images, native-pixel crops and execution logs stay outside the shipped resource collection.

### Quality and scope

The complete image and four unscaled detail crops were visually reviewed: subject and gripper framing, mechanical edges, electronic connectors, glass and highlights remain readable. Residual Monte Carlo grain is visible at native pixel scale in some vial/lens regions; the current denoiser deliberately does not reconstruct those transmission paths. This is a high-quality native showcase, not proof of zero residual noise or a completed maximum-quality renderer roadmap.

RGB absorption and material recipes are illustrative authored optical choices, not measured glass spectra or engineering properties. The explicit rig gain is a creative lighting adjustment, not an unchanged photometric calibration. Normal maps alter shading, not CAD geometry. LED diffusers are transmitted surfaces, not validated volumetric/mesh emitters. The static image does not establish collision-free motion, contact physics, lunar dust or dynamics.

Native renderer tests passed 457/457. The hardware GPU suite passed 95 tests with three explicitly ignored tests, and the official 12-crate library/web WASM check passed. Whole-workspace type checking, strict lint, resource/persistence command checks and camera/publication tests have their own evidence. Interactive GUI acceptance and whole-project CI are not inferred from this image. The focused Render-workspace harness also passed 58 tests with no failures; its click-stability repair and remaining whole-project acceptance scope are recorded in the batch ledger and PR #7.

## Historical Design previews and earlier CPU-product evidence

The sections below preserve the earlier `10ada1b4` CPU-product/Design observations. Their resource ceilings describe that legacy raster executor, **not** the current native GPU path or the explicit larger CPU raytrace profiles. The whole-master GPU limitation described in that historical workflow is now resolved by the completed image above.


The enhanced native master is the editable authority. Its ten named camera and render-setup records make close inspections reproducible. Body appearances distinguish metals, painted structure, elastomer, FR4, packages, contacts and optical surfaces. The orange and charcoal identity is retained.

## Reusable Design previews

The package ships three previews generated directly by SolidCraft from the final neutral master: a whole-rover overview, the Raspberry Pi assembly and an exposed elbow bearing. They use native Design shading. They are intended for the model's README and showcase preparation; they are not photographs or path-traced optical renders. No raster image generator or painted replacement geometry is involved.

| Preview | Resource | Purpose |
|---|---|---|
| Whole rover | [SELENE-overview.png](exports/SELENE-overview.png) | Overall form, component placement and retained visual identity. |
| Electronics | [SELENE-electronics.png](exports/SELENE-electronics.png) | Board, mounting, packages and connector mouths, viewed from the +X side. |
| Open bearing | [SELENE-bearing.png](exports/SELENE-bearing.png) | Grooved races, individual balls and the brass cage. |

Regenerate them in a fresh output directory:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 assets/render/selene/tools/export_views.py \
  --cli /path/to/solidcraft-cli \
  --model assets/render/selene/models/SELENE-functional.solidcraft \
  --out target/selene/previews
```

The exporter opens the entire native model once, changes only transient body visibility and cameras, and requests three native MCP screenshots. It saves no changes to the master and verifies its SHA-256 afterward. The output report records the actual capture metadata, image hashes, dimensions, cameras and executor hash. Its default size is 1400 × 1000 pixels.

The body selector for electronics retains the PCB, all `Pi5 ` componentry and the `Pi M2.5` mounting hardware. The bearing selector isolates `Bearing Elbow port`. The full overview hides the optional track and source flexible-hose display geometry. None of these visibility changes delete geometry.

## Actual product-render checks

The personal fork at `10ada1b45a4eb285ce752ac72f9aaa968ff8b5c4` was compiled and exercised using its native `cpu-product` executor. The binary reports version 0.18.2 and writes native format 29. Its binary SHA-256 was `2a5485f7c7832d29b6340988837316e247b94eeeb9d4f7333ee616cea38c6826`.

Two bounded opaque PBR images passed native preflight and rendering:

| Actual subject | Visible bodies | Triangles | Admitted job estimate | Output |
|---|---:|---:|---:|---|
| Fresh single arm-bearing fixture generated by `bearings.py` | 20 | 93,654 | 43,594,876 bytes | 768 × 576 PNG, 51,705 bytes |
| Pi isolated from the actual integrated stage-03 model | 504 | 103,004 | 50,848,960 bytes | 768 × 576 PNG, 42,351 bytes |

Both returned no preflight warnings and were visually inspected. The Pi geometry was taken from the integrated rover checkpoint, not a replacement board illustration. The single-bearing image was a reduced fixture of the production geometry; it was not a rendering of the whole rover. Their raw logs, transient native files and review images belong under ignored `target/selene/render-review/`. The final model and reusable Design previews have separate hashes in [VALIDATION.md](VALIDATION.md).

That historical `cpu-product` raster path uses three fixed directional studio lights. Pure metals look dark away from their highlights because it does not provide an ambient or HDRI contribution in this path. This behavior does not mean the material assignments were lost. Use the Design previews to inspect geometry, and inspect PBR behavior with the product executor's actual lighting limits in mind.

## Historical whole-model CPU-product resource limit

At this fork revision the CPU product-image path admits at most **262,144 triangles**, **128 MiB of source geometry**, **384 MiB per job estimate** and **128 million projected pixel tests**. A single arm ball has 10,224 reference-mesh triangles; the 64 arm balls alone account for 654,336 triangles, before the head bearings or rover are included. The full-detail model therefore exceeds this CPU product triangle cap.

Reducing output dimensions reduces image buffers, not reference-mesh triangle count. Changing viewport display quality does not lower the meshes used by this product renderer. Do not bypass the admission check or silently remove mechanical detail from the master. Use the whole native Design preview, isolate a component for current CPU PBR work, or author a separately documented reduced-visibility render scene. A future whole-rover render executor must admit the complete scene or explicitly support a documented mesh-detail policy.

CAD evaluation is additional to the render-job estimate. Opening the integrated stage-03 model during these checks took about 102 seconds and peaked near 3.76 GiB RSS. Later assemblies are larger. Run the native builder, validation and full-model image export **sequentially**. The provided tools constrain native worker threads and use an explicit allocator policy; detailed evidence records resource failures and recovered attempts rather than treating them as passes.

## Material and optical boundaries

- Opaque PBR values are authored visual parameters, not measured BRDF data. Painted orange and charcoal surfaces retain dielectric/coated appearances; exposed races, balls, contacts and shafts have separate metal responses.
- Engineering-material density is a nominal proxy. It does not establish the complete component mass, electrical specification, thermal performance or contact friction.
- Each glass vial is one hollow native solid with an integral bottom. This supplies coherent geometry for later closed-medium admission.
- Viewport opacity is not physical refraction. Camera lenses, diffusers and vials require explicit optical descriptors, lighting and medium validation; the current studio recipe above supplies and validates them without rewriting the native master.
- No lunar terrain, illumination ephemeris, dust simulation, HDRI or manufacturer PCB artwork is bundled in the native derivative.

## Preparing animation shots

Use the seven focused studies to inspect the actual joint chains. Hide the six rigid suspension coils and twelve motor leads while the suspension moves; these neutral render details do not deform. The `motion` profile in `presentation.py` selects that view. Removed arm hydraulic display cylinders do not reappear during motion.

Keep the final master neutral and export motion/render copies separately. The adapted author tour and focused studies are kinematic authoring resources. A complete animated shot still needs its selected combined path checked for collisions, a timing choice, flexible-detail treatment and any physical contact model it intends to claim. Throwing and catching a free vial remains future dynamics work.
