# Glass transport and verification matrix

CPU and GPU share saved Glass assignments and the mathematical dielectric
contract. Each producer validates the captured geometry and transport options.
The tables below state the supported domain and the remaining limits. They do
not turn a saved control, a passing unit test or a rendered example into a claim
that every glass material or light-transport effect is implemented.

## Optical quantities

All geometric distances are millimetres. IOR is a finite scalar in [1, 3]. The
three absorption coefficients are finite linear-RGB values in [0, 100] per mm.
Absorption is independent of the opaque base colour and scalar preview opacity.

For geometric path length d, channel transmission is `exp(-sigma * d)`.
A desired channel transmission T at positive distance d corresponds to
`sigma = -ln(T) / d`. T is linear transmission, not an encoded sRGB colour.
Zero absorption preserves radiance between interfaces. Positive absorption
depends on actual thickness and angle; tint is not a surface colour overlay.

Both producers use Snell refraction and exact unpolarized dielectric Fresnel.
The s and p power reflectances are computed separately and averaged. Radiance
transmission includes the squared incident/transmitted IOR ratio. Total internal
reflection produces a reflected continuation with no transmitted branch.
An index-matched interface reflects zero, including the limiting grazing case.

An uncoated smooth ideal sheet combines both parallel interfaces and all incoherent internal
reflections. It absorbs over `thickness / refracted_cosine` and leaves the
transmitted direction unchanged. Zero thickness still retains the two-interface
response. The sheet has no interior region or lateral displacement. An optional
advanced material adds rough interfaces, dispersion and a coherent single-layer
coating; see [Glass material models](render-glass-material-models.md). Geometric
sheet absorption, coating interference and a closed solid are distinct models.

## Geometry and transport

| Case | CPU ray tracing | GPU path tracing |
| --- | --- | --- |
| Clear closed glass | Explicit Indirect lighting (Path), progressive camera sampling, traced shadows and `primary_samples: 1`; smooth or explicitly authored rough interfaces. | Smooth or explicitly authored rough interfaces; progressive fixed or adaptive sampling. |
| Tinted or absorbing glass | RGB Beer attenuation over physical segments, including the segment from an inside eye or sampled pupil. | RGB Beer attenuation between geometric endpoints; numerical launch adjustment does not shorten absorption. |
| Ideal parallel sheet | Path accepts a connected exact planar tessellation, including concavity and holes, within its work/memory budget; explicit thickness and exact slab Fresnel/Beer response. The legacy route retains its one/two-triangle domain. | Captured sheet surface, explicit thickness and exact slab Fresnel/Beer response. |
| General closed shape | Exact captured-f64 closed-manifold, vertex-link, contact, self-intersection and outward-volume certificate. No seam welding or geometry repair. | Source and actual camera-relative uploaded-f32 topology, circular vertex-link and outward-orientation certificates with bounded seam matching. General absence of self-intersection is not proved. |
| Hollow vessel | One connected outward boundary may include inner walls and an open air cavity. Each real interface is traversed. | Same physical boundary model, subject to the GPU certificate and precision contract. |
| Disconnected shells in one selected body | Refused. Author separate optical bodies with unambiguous material boundaries. | Refused. A body must have one connected shell. |
| Strictly nested solids or an air bubble | Path proves a strict containment hierarchy with convex enclosing parents. Inner IOR 1 can represent an air region. | Explicit `nested_media: true` and its strict convex-parent containment certificate. |
| Touching, crossing or overlapping selected media | Refused when the optical certificate cannot establish a unique strict hierarchy. | Refused by the admitted hierarchy policy. No overlapping-medium repair or material-priority rule. |
| Camera or pupil inside a solid | Physical origin classification initializes the ordered medium stack. Boundary and near-clip crossings use the documented refusal policy. | Explicit nested mode performs the shader origin classification; default outside-origin policy remains. |
| TIR and grazing interfaces | Geometric winding and correct incident/transmitted medium select the physical branch. | Same mathematical branch policy within f32 intersection and origin limits. |
| Opaque objects behind or beside glass | Supported. Ordinary opaque surfaces retain their supported PBR, maps, aging and clear coat. | Supported within each opaque feature's GPU admission. |
| Opaque object embedded inside glass | Actual sampled paths remain physical; opaque contact ties preserve opaque material. | Actual sampled paths remain subject to medium classification; enabled solid shadow approximation has separate limits. |
| Glass surface plus opaque maps or clear coat | A single target cannot combine Glass with opaque maps or clear coat. Unsupported authored combinations refuse. | Same saved-target policy. |
| Selected-medium scattering | Homogeneous elastic free flight and HG phase inside a certified closed Path medium; surface Denoise must be off. | Homogeneous random walk inside selected closed media; Denoise off, one Fresnel stratum and no active external volume. |

The saved editor resolves definition, feature, component and occurrence targets.
The raw runtime adapter instead binds an exact captured ordinal and body name.
These are separate binding lanes; mixing them is refused. Runtime overrides do
not change design appearances, engineering materials, mass or geometry.

The common runtime command contract admits at most eight overrides. Internal GPU
scene capacities are not a promise that a saved or runtime command accepts more.
CPU and GPU retain their individual memory, geometry, work and path-depth bounds.
Certification failure preserves authored input and produces no partial proof.

Path sheet certification charges each captured face, edge operation, vertex link
and exact contact check through the existing cooperative budget. Boundary edges
have one incident face; interior edges have two oppositely directed incident
faces. Vertex fans form one connected cycle or one boundary path. Folded,
disconnected, duplicated, overlapping, nonmanifold and pinched sheets refuse.
T-junctions are not repaired. Equal authored coordinates can join without relying
on shared vertex indices; signed zero has one exact geometric identity.

The GPU additionally certifies the positions actually uploaded after subtracting
the camera target and rounding to f32. Its legacy source-degenerate triangle
skip remains compatible with redundant sphere pole faces in thin and closed
bodies. A selected optical body with no retained boundary refuses. Each retained
face must remain valid after conversion: f32 collapse or orientation reversal
refuses before rendering. Closed boundaries also need the complete topology and
orientation proofs. Bounds, hierarchy and optical BVH geometry use the consumed
positions. This prevents a valid source mesh from advertising a proof for
different consumed geometry. It does not remove finite-precision limits or add
a general self-intersection proof.

## Lighting, sampling and output

| Effect or combination | CPU | GPU |
| --- | --- | --- |
| Environment, HDR or analytic rectangle reached by a glass path | Physical sampled specular continuations reach the endpoint. | Physical sampled specular continuations reach the endpoint. |
| Thin-sheet transmitted direct shadows | Explicit Path shadow transmission uses the smooth RGB slab, including dispersion/film, and matching light/BSDF MIS probabilities. Rough sheets refuse straight shadows. | Enabled by raw smooth-sheet default; slab transmission and matching MIS history, including optional spectral/film response. Rough sheets refuse straight shadows. |
| Closed-solid transmitted direct shadows | Explicit `shadow_transmission: true` refuses. Physical sampled glass paths still reach extended emitters. | Opt-in straight-through NEE approximation; 64-interface limit. It replaces the corresponding transmitted direct estimate. |
| Hard directional, point or spot illumination at rough Glass or selected HG collisions | Explicit direct connections use dielectric projected BSDF or phase; supported visibility connections and finite depth apply. | Same estimator, including implicit studio directions. Ideal pure-delta source endpoints retain their existing visibility convention. |
| Focused caustics from hard directional, point or spot sources | Opt-in bounded photon tracing with rough/interface, HG and opaque receivers, a uniform bounded receiver reservoir and finite-radius density estimation. | The same receiver domain and reservoir under the seven-storage-buffer variant. Solid shadow approximation remains separate. |
| External participating atmosphere plus glass | Refused. The bounded atmosphere consumer is an opaque-Path route. | Refused by the active homogeneous-volume route. |
| Physical depth of field | Explicit lens under the CPU optical/pupil certificate. | Explicit lens under GPU origin and nested-medium admission. |
| Adaptive sampling | Independent pilots and retained production samples under the selected CPU profile. | Independent pilots and bounded per-pixel production allocation. |
| Denoising | Surface guide filter bypasses glass and sharp first hits; raw EXR remains unfiltered. Active scattering combinations refuse. | Same surface-filter limitation; not a glass-transport or depth-of-field reconstruction model. |
| PNG and raw EXR | Completed frame owns its display PNG and optional raw scene-linear master. | Completed frame owns its display PNG and optional raw scene-linear master. |
| Transparent background | Coverage alpha is not a general transmissive compositing matte. | Same distinction. |

A finite maximum depth truncates paths; a completed render does not prove
convergence. The CPU and GPU use different sampling streams, geometry precision
and light estimators. Equal settings do not imply equal noise or identical pixels.

## Material models and remaining limits

| Requested appearance or phenomenon | Current status |
| --- | --- |
| Frosted, etched or sandblasted rough glass | Explicit isotropic single-scattering GGX reflection/transmission is supported. General anisotropic or measured microsurface models remain absent. |
| Chromatic dispersion, rainbows and Abbe numbers | Explicit bounded Abbe/Cauchy IOR is supported at three representative RGB wavelengths; not dense spectral rendering or measured Sellmeier glass data. |
| Thin-film interference, iridescence or coherent films | One lossless coherent dielectric coating is supported. Multilayer stacks, absorbing coatings and spectral anti-aliasing remain absent. |
| Polarized transport, birefringence or anisotropic crystals | Fresnel uses averaged s/p power; no retained polarization state or tensor IOR. |
| Heterogeneous, textured or spatially varying interior media | No heterogeneous selected-medium density or RGB scattering albedo. |
| Fractional mixtures of opaque and transmitting closures | No fractional transmission-weight field. Scalar opacity remains a separate raster-preview property. |
| General nonconvex nested parents | No general nesting proof for nonconvex enclosing parents. |
| Arbitrarily intersecting media, touching liquid menisci or overlapping priorities | No implicit boundary repair, epsilon gap creation or medium-priority solver. |
| Full refracted direct-light silhouettes and general caustics | Hard-source photon caustics use a finite-radius biased estimate. No general unbiased specular-chain solver or convergence guarantee. The GPU optional straight shadow connection is separately approximate. |

The [material-model guide](render-glass-material-models.md) details the authored
models, representative wavelength limits and finite-radius caustic bias.
Remaining limits require new transport, material-authoring or certification work;
removing an admission guard does not implement the missing effect.

## Reproducible headless images

The original [`glass_comparison` example](../crates/gpu/examples/glass_comparison.rs)
uses the public CPU and GPU hosts. It builds fourteen public-safe scenes from explicit
triangle positions without the CAD kernel or external assets:

| Case | Geometry and visible purpose |
| --- | --- |
| `clear` | Nonabsorbing rectangular solid in front of an opaque checker board. |
| `tinted` | The same solid with RGB absorption `[0.065, 0.012, 0.003]` per mm. |
| `hollow` | One connected 3 mm thick square vessel, inner walls and solid bottom. |
| `nested` | Absorbing outer box and strictly enclosed index-1 air box. |
| `sheet` | One ideal 3 mm sheet with opaque checker board and floor. |
| `rough` | Closed glass with explicit GGX roughness 0.35. |
| `dispersion` | An original triangular prism with an illustrative Abbe number of 15. |
| `film` | A 400 nm, IOR 1.35 lossless dielectric film on clear glass. |
| `rough_sheet` | A 3 mm virtual sheet with independent rough interfaces. |
| `caustics` | A faceted sphere over a receiver, one hard directional source, 65,536 photons and 0.75 mm gather radius. |
| `rough_lights` | A rough closed solid illuminated only by a blue directional, red point and green spot light; no environment or area emission. |
| `rough_sheet_lights` | The same hard-only source family on a 3 mm rough virtual sheet. |
| `rough_caustics` | A roughness-0.2 faceted sphere, hard directional light and the surface photon map; no environment or area emission. |
| `scattering_caustics` | A closed HG medium with scattering 0.06/mm and anisotropy 0.35; hard directional light, 65,536 photons and a 2 mm volume/surface gather radius. |

All scenes use IOR 1.5 for the glass, 16 path vertices, seed 81, no denoising and
no display effects. The sphere caustic scenes use a higher camera elevation and
a receiver 5 mm below the sphere. The common camera is perspective with a 0.65
radian vertical field of view. The original ten cases use constant environment
radiance `[0.7, 0.8, 1]` and one rectangle with radiance `[4, 4, 4]`. The four
additional hard-source cases explicitly disable both. The CPU uses four samples
per enabled area connection; GPU uses its native estimator. The source code and
each saved receipt contain every hard-light position, direction, distance/cone
parameter and intensity. Neither producer enables the GPU solid-shadow
approximation. Adding a reproducible scene is not a claim that it has passed a
physical backend or image-quality review; publication records name actual runs.

From the repository root, choose an output directory that does not yet exist:

```sh
cargo run --locked --profile ci -p solidcraft-gpu \
  --features solidcraft-render/raytrace-cpu --example glass_comparison -- \
  /tmp/solidcraft-glass-evidence both 32 all 256
```

Arguments after the directory select `cpu|gpu|both`, 1–64 samples per pixel,
one case from the table or `all`, and width 64–1024. Height is three quarters
of width, rounded down. Missing GPU support produces an error; it does not count
as a successful GPU execution or silently substitute a CPU image.

Each completed case writes its actual PNG, raw EXR and a JSON receipt. Receipts
record the exact request, options, captured vertices/triangles, per-face geometric
normals, face IDs, material/optical values, executor metadata,
frame identity and raw-master digest. Failure or timeout does not write a PASS
receipt. The example refuses an existing output directory to preserve evidence.
Generated media belong outside the repository and are not source assets.

This example verifies headless job completion and same-frame image ownership.
It does not exercise desktop controls, saved stable bindings, Save/reopen,
cancellation, every adapter, default quality or image-quality acceptance.
Mathematical oracles and negative geometry tests provide independent evidence
for their asserted quantities; a pleasing image cannot replace those checks.
See [renderer parity](render-parity.md), [CPU general Glass](render-cpu-general-glass.md),
[GPU boundaries](render-glass-boundaries.md) and [shadow connections](render-glass-shadows.md)
for the existing algorithms and their separately qualified scopes.
