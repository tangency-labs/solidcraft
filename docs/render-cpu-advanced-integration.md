# Advanced CPU recovery on the rendering branch

This document records the recovered local CPU port onto the personal branch's
CPU/GPU2 snapshot. That recovery did not merge an upstream branch or change the
document format. Subsequent numerical persistence and progressive publication
are described below and in [CPU progressive sampling](render-cpu-progressive.md).
The recovery baseline is source tree
`338535736593a8e49d9a6d1ccff8509dff618721`, with Git HEAD
`93a2922c5ee272fb605703ae84c05825b4a74607`.

The recovery input is the 34-path local patch against original base
`64339aedbc5e2f0d46e89dd915724deb06d5702d`; its SHA-256 is
`746f4e18b75ae9e6e4095e7c40812bad92c20078275979e2ccef7b3605924b77`.
It contains the project's original CPU prototypes and follow-ups associated
with 0xlillith's prior work. This integration adds no third-party assets, code,
rendering library or dependency. The source identity is provenance, not proof
that its old tests passed on this new combination.

## Historical recovery capabilities

The following table records the original recovery tree above, not the complete
published renderer. Current Path Glass, coats, saved maps, adaptive sampling and
qualification are listed in [renderer parity](render-parity.md). In particular,
the current legacy box certificate accepts valid tiled box faces; it does not
require exactly 12 triangles. Historical test/provenance claims below stay scoped
to their recorded source.

| Area | Original recovery consumer | Original recovery limits |
| --- | --- | --- |
| GGX visible-normal sampling (VNDF) | CPU `ImageTask` rough reflections and the certified anisotropic adapter | The original single-scattering Schlick/GGX/separable-Smith model; no layered coat transport, transmission proposal or diffuse GI. |
| Fixed primary AA | Existing CPU image job and visible 1/4/16 sample controls | Reuse only for completed, nonselected primary hits within one fixed spatial sample. Not a progressive sample prefix. |
| Surface emission | Primary and reflected hits add visible linear radiance through current throughput | No mesh-light sampling, illumination of neighbors, emissive GI or calibrated watts. |
| Surface effects | The shared CPU hit surface feeds existing direct lights, HDR and opaque reflections | Bounded periodic roughness/bump fields in world millimetres; no geometry displacement, texture lookup or physical-material change. |
| Dielectric preview | Existing CPU image job branches bounded reflected/transmitted paths | Only certified thin sheets and outward, axis-aligned 12-triangle boxes; no general closed mesh, nested/touching volumes, colored transmission shadows, caustics or GI. |
| Recovered film component | Public `FilmAccumulator` API, with a complete-pass view and `FilmPngTask` encoder | This statistical component remains independent. The subsequent native progressive image path uses `film::primary::PrimaryFilm` and the shared image host. |

The runtime API accepts new CPU options under `raytrace`: `emissions`,
`surface_effects`, `dielectrics` and `environment`. The last is constant secondary
radiance, separate from the captured HDR input at the top level. The subsequent
[CPU workspace panel](render-cpu-workspace.md) exposes body reflections, constant
secondary radiance, captured HDR input, directional lights and point lights.
Area lights, area sampling/seed and spot cones have no dedicated panel editors,
but complete saved numerical setups retain their values and pass them to engine
validation. Emission, surface effects, glass and anisotropic face bindings remain
runtime API inputs, outside portable setup persistence. Existing CPU primary AA,
exposure, shadow and studio preset controls remain in place. The GPU material
editor owns a different runtime schema selected explicitly with top-level
`progressive: true`. CPU progressive sampling instead uses `raytrace.progressive`
and supports the existing rectangular/continuous LED light integral with a fresh
film-aware sample identity. The subsequent progressive continuation also accepts
the existing certified thin-sheet/axis-aligned-box dielectric and planar anisotropy
subsets, preserving all geometry, HDR, clear-coat and optical-overlap checks.

See [VNDF](render-vndf-prototype.md), [primary AA](render-primary-aa.md),
[emission](render-surface-emission.md), [surface effects](render-surface-patterns.md)
and [dielectrics](render-raytrace-dielectrics.md) for request and numerical bounds.

## Film and exact PNG contract

`FilmAccumulator` keeps per-pixel sample counts, linear means and Welford
variance. Each checked update contains at most 256 consecutive pixels. Work and
input checks precede its bounded commit, and cancellation is checked before
commit. `complete_pass` refuses zero samples or unequal pixel counts and returns
a borrowed immutable view. No partially completed pass is publishable through
that API.

`FilmPngTask` resolves the complete linear mean with exposure, the existing
Reinhard mapping and one sRGB transfer. It reuses the CPU PNG encoder, including
the sRGB chunk, rather than introducing another codec. Its maximum output is
4096 pixels per dimension and four million pixels in total. A step resolves at
most 256 pixels and writes at most four encoding rows. The encoded result owns
its host reservation and exposes bytes by borrow.

The subsequent [CPU progressive image path](render-cpu-progressive.md) connects
its camera-specific `PrimaryFilm` to sampler passes, the same cancellation token
and cumulative work counter, generation checks under the host's publication lock,
and retained immutable checkpoints. Only the final PNG is exportable. It does not
call the statistical `FilmAccumulator`/`FilmPngTask` API described above. That
independent API still expects an existing caller-owned film lease, held for the
entire film lifetime; its component tests alone do not qualify the integrated
progressive path.

## CPU/GPU compatibility and resources

The builtin CPU image limit stays 384 MiB per job; registered GPU jobs keep their
512 MiB limit. Film snapshots use the existing host pool, reject a registered
executor, and account for both borrowed film and encoding capacities before
allocation. Completed PNGs retain the remaining lease. New emission, surface and
dielectric option capacities are included in task and retained-request accounting.
No new process, background worker, device registry or pool is introduced.

The shared `OpaqueSurface` type can contain GPU2's clear coat. Applying a CPU
roughness field preserves that layer for direct BRDF evaluation. A coated body
or face selected for CPU reflections, anisotropy or dielectrics is explicitly
refused because the recovered transport estimator has no coat lobe. GPU2's own
coat and dielectric path remains unchanged. Captured GPU `transmission` data is
not reinterpreted as CPU dielectric options.

CPU admission still certifies immutable captured body identities, retains opaque
body/face alpha requirements, checks finite inputs and bounded capacities, and rejects
unsupported intersections between optical, emission and surface selections.
Geometry, document history, physical mass properties and saved materials are not
modified by these runtime inputs.

## Validation status and next acceptance gate

The recovery handoff was a source-only port. At that handoff, its author ran
Rust formatting, a whitespace check and a reproducible patch replay; the author
did not run a Cargo build, test, Clippy, wasm check, application launch or native
image acceptance for that isolated port. Existing
recovered tests are retained, with new integration regressions for preservation
of clear coat, refusal of unmatched coat transport and exclusion of registered
executors from CPU film reservations.

Subsequent integration has executed CPU renderer, saved-document and engine
regressions, default-feature host metadata/digest checks, and an earlier-snapshot
Render workspace suite with CPU enabled and a real GPU required. That earlier
workspace result does not qualify the new combined CPU additions. The workspace
suite includes an actual CPU Render action and export of that job's retained
native master; see [CPU output presentation](render-cpu-output.md#focused-validation)
for the exercised scope. The source-only qualification above describes the
original recovery handoff.

Each final combined revision still needs its repository-wide and platform checks,
including wasm, plus native equal-camera image acceptance. Historical results
from earlier source snapshots do not qualify later changes.


The optional [CPU opaque path lighting](render-cpu-path.md) consumer is part of the combined source and has executed renderer regressions. It adds bounded progressive diffuse/specular GI only when `path` is authored; existing restrictions and bytes above describe the absent-path route. Mixed glass/anisotropy/volume parity and combined document/engine/UI/native qualification remain incomplete.
