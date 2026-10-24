# Renderer capability and quality plan

## The quality bar

The objective is a production renderer that can hold up in demanding CAD visualization and product imagery, with the control expected from a general-purpose renderer. A large library helps users begin; believable geometry, transport, optics, color and reproducibility determine the final image. Comparison with established CAD tools or Blender is an outcome to demonstrate with independent scenes and timed tasks, not a feature count or a claim made by this specification.

The 120 records in [capabilities.json](capabilities.json) separate physical transport, geometry, light sources, camera, image processing, output, execution and CAD integration. A bloom slider, a new material thumbnail and a convergent glass transport model are different kinds of work and need different evidence.

## Foundation before effects

| Foundation | Why it matters | Required evidence |
|---|---|---|
| Scene geometry and precision | CAD parts range from tiny gaps and wires to large assemblies. | No cracks, lost small parts or self-shadow artifacts in mixed-scale references. |
| Global light transport | Contact, indirect illumination and reflected color anchor a product. | Analytic and converged reference scenes with no unexplained ambient term. |
| Layered appearance | Paint, oxide, coatings and dirt need distinct optical roles. | Coated conductor, dielectric substrate and masked wear tests with plausible energy. |
| Physical mapping | Wood grain, brushing and print layers communicate manufacturing scale. | Stable millimeter-scale detail across body, instance and scene size changes. |
| Color pipeline | An image cannot be assessed if map decoding, exposure and display mapping are confused. | Declared working space and matching preview/export transforms. |
| Camera and framing | Perspective, focus and lens behavior establish the photographed object. | Repeatable saved shots, clear units and verified depth-of-field references. |
| Execution contract | Users need predictable results on their selected device. | The same request is preflighted and executed, with bounded memory and cancellation. |

Use a layered appearance abstraction informed by public specifications such as [OpenPBR](https://academysoftwarefoundation.github.io/OpenPBR/), while independently implementing behavior allowed by repository policy. The catalog does not mandate a third-party implementation or permit otherwise prohibited dependencies. The public specification describes a common surface authoring model; specialized optics and granular surface response still need their own validated treatment.

## Effects and specialized features

**Light transport and sources.** Progressive path tracing, multiple importance sampling, adaptive sampling, explicit bounce limits, compensated termination, area/point/spot/Sun sources, importance-sampled HDR environments, sampled emissive meshes and many-light acceleration. Add IES/LDT only with documented units, orientation and normalization. Light groups can rebalance fixed linear contributions; arbitrary recoloring or linking is an identified artistic operation.

**Surfaces.** Dielectric and conductor Fresnel, roughness, anisotropy, independent coats, fibers, flakes, layered masks, decals and wear. A gold-colored polymer is not a conductor. Painted metal remains a dielectric outer layer until wear reveals the substrate. Oxide, dust and contamination need their own layers. Avoid a single metalness control that silently treats every surface of an aerospace assembly as metal.

**Optics.** Smooth/rough transmission, path-length absorption, nested media, subsurface transport and refractive caustics. LED and sample-vial shots make geometry thickness and medium ownership especially important. Later spectral dispersion, thin-film interference, polarization and measured BSDF import require dedicated reference fixtures and explicit model limits. They should not delay a robust opaque product workflow.

**Geometry detail.** Stable tessellation, render-only microbevel, normal/bump detail, displacement, texture filtering, instancing, terrain level of detail and contact tracks. A microbevel must not alter the engineering B-rep. Dust that changes silhouette needs geometry or a declared approximation; normal mapping cannot provide a missing contact shape.

**Environment.** Procedural terrestrial sky and Sun, three-dimensional stages with parallax, physical ground and optional atmosphere/weather. Keep background visibility and illumination linked by default; expose intentional decoupling. Lunar vacuum scenes exclude atmospheric effects but still need reflected illumination and geometric occlusion.

**Camera and finishing.** Perspective/orthographic views, depth of field, aperture shape, lens shift, calibrated backplates, motion blur and additional projections where justified. Exposure, white balance, view transforms and clipping diagnostics come before bloom, flare, vignette, distortion, chromatic aberration, grain and sharpening. Effects remain independently reversible, with an untouched linear master where supported.

**Production.** Raw/denoised comparison, EXR and presentation encoders, diagnostic passes, object masks, Cryptomatte or a verified interoperable equivalent, queues, variants, resumable jobs, portable packs and resource manifests. Denoising must preserve circuit traces, labels, fine machining and dust detail. An image sequence must retain consistent resources, color treatment and motion across frames.

**CAD behavior.** Face/body/instance scopes, engineering/appearance separation, explicit overrides, one-action undo, nondestructive staging, section/exploded presentation, shared UI/CLI/MCP commands and strict capability errors. A render library becomes trustworthy when changing an appearance cannot corrupt the design or unexpectedly change assembly mass.

## Asset production

Prefer deterministic procedural construction for machining, brushing, print layers, repeated fibers, surface noise and stage geometry. Use original or properly admitted source imagery for irregular grain, reference surfaces or specialized maps. AI can accelerate concept exploration, but material maps need independent review for baked lighting, seams, map consistency, scale and unsupported physical claims.

Use a common appearance test object with a smooth region, sharp edge, bevel, curved region, thickness variation and a known-size mark. Add domain tests: flakes on a curved panel, grain around a wood joint, traces on a PCB, a light guide and LED diffuser, glass with a closed sample, and dust on a wheel. A material pack is published only when its actual resource, preview, dependencies and provenance agree.

Catalog parameters should expose meaningful manufacturing choices: brushed direction and scratch scale; print layer height and wall orientation; wood species family and grain direction; paint coat and flake density; textile weave; polymer translucency; contamination amount. Color variations belong to a parameterized family when they do not justify a separate optical model. Distinct manufacturing processes and layered structures justify distinct entries.

## Reference suite

| Reference | What it isolates |
|---|---|
| Diffuse box and area emitter | Direct/indirect energy and convergence. |
| Conductor/dielectric angular strip | Fresnel, roughness and coating response. |
| Brushed cylinder and rotated instances | Tangent orientation and anisotropy. |
| Paint panel with sparse flakes | Coats, sparkle filtering and temporal stability. |
| Hollow glass vial with closed sample | Thickness, interfaces, absorption and nested media. |
| LED bar with diffuser and light guide | Source ownership, transmitted light and difficult paths. |
| PCB with labels and thin traces | Geometry precision, map decoding and denoising preservation. |
| FDM part with known layer scale | Physical mapping, silhouette and view-distance transitions. |
| Wood corner joint and coated board | Grain orientation, pores, coat and mapping continuity. |
| Aerospace foil and solar-cell assembly | Layered surfaces, repeated detail and meaningful optical limits. |
| SELENE on procedural lunar terrain | Scale, ground contact, Sun ownership, terrain and shadows. |
| Baked vial release/catch sequence | Shutter-time motion, contact continuity and frame stability. |

Each reference records scene version, resources, camera, exposure, display transform, executor and tolerance. For numeric cases compare with analytic values or independently produced reference data that passes clean-room policy. For artistic judgments use fixed-camera A/B review with raw images, annotated defects and multiple viewpoints. Do not compare screenshots taken under different tone mapping and call the difference a transport result.

## Performance and success criteria

Record time to first useful preview, preparation time, time to an agreed error or perceptual threshold, peak memory, interruption latency and recovery behavior. Report hardware and scene size. Use a quality/time curve rather than a single samples-per-second figure. Set platform-specific targets after profiling the actual implementation; this document does not invent device performance.

The first release gate is a reliable product workflow and a small verified resource pack. The next gate is glass/LED/aerospace optics and consistent production output. The SELENE lunar still then becomes a demanding integrated reference. Future simulation and animation add new acceptance criteria; a beautiful still does not validate dynamics.
